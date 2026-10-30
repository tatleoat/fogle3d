// SPDX-License-Identifier: MIT
//! The checks of the third pass: the image forms of `si_kine::image` (the solver, the
//! joints' rotation limits, the two limit constraints, the direction constraints) run on the
//! first pass's vectors (`re/tools/si-kine/emu_gen.py`), with `kine-emucheck --image <dir>`.
//!
//! A case is not rebuilt in the crate's terms (that is what the first pass's check does with
//! its `World`): its buffers are placed in a fresh `si_mem::Mem` (the five globals of the
//! vectors at their addresses, everything else in the arena in the case's order), the image
//! form runs on them, and every output buffer of the case is compared with memory, word for
//! word, with the pointers between buffers (and to the heap blocks the port took, `heap<i>`
//! in the order of its `malloc` calls) resolved by name; the routine's result, the count of
//! the solver's rounds and the calls of the classical-scaling solver likewise.
//!
//! The stand-ins of the generator for the scene dictionary (`emu_gen.py`, `install_stubs`)
//! are answered here by the same rules, from the spare words of the node records. What ran
//! as the binary's own code in the emulator is answered by ports on the image:
//! `SI_modelUpdateSonState` by `si-model`'s image form; `SI_modelSetRotation` and
//! `SI_modelSetGlobalTranslation`, which have no image form yet, by the two stand-ins
//! `motion-evalcheck` uses (written again below, unchecked: these cases check them too).
//!
//! A case in which the port left the original's path (a null pointer the original reads
//! through: a fault recorded on the image or [`si_kine::fault`]; a matrix inverse that does
//! not exist: `si_model::env::stale`) is skipped, as in the first pass. So is one in which a
//! node setter is asked for `SI_modelCompensateMatrix`'s compensation, which the stand-ins
//! leave out. `THIRD_DEBUG=1` prints the chain's nodes before and after the port and every
//! word of a differing buffer.

use si_kine::image::{self as img, Node};
use si_kine::outside::{Outside, Ret as ORet, Routine};
use si_math::algebra as alg;
use si_mem::{Mem, Names, Ptr32, Word as W};
use si_types::geom::Vector;
use si_x87::vectors::{Case, Check, Verdict, Word};
use si_x87::{x, F80};

use crate::image::words;
use crate::world::mutate;

/// The globals of the first pass's vectors, at their addresses (`emu_gen.py`, `GLOBALS`).
const GLOBALS: &[(&str, u32)] = &[("manip", 0x0878_1560), ("upd", 0x0867_9678), ("hold", 0x0878_1486), ("inter", 0x0867_953e), ("pdp", 0x0867_9314)];

/// The fields that are what the routine returns, not buffers.
const RESULTS: &[&str] = &["eax", "ax", "st0", "ptr", "rounds", "freed"];

/// The generator's stand-ins, and the ports that ran as the binary's code in the emulator.
#[derive(Default)]
struct EmuOut {
    last: u32,
    /// A node setter was asked for the compensation of `SI_modelCompensateMatrix` (a mode
    /// other than 1, or geometry), which the stand-ins do not do: the case is skipped.
    compensated: bool,
    classic: Vec<(i32, [u32; 3], i16)>,
    unknown: Vec<&'static str>,
}

const EL: u32 = 0x2c8;

fn spare(m: &Mem, node: u32) -> u32 {
    m.u16(node + 0x0e) as u32
}

/// `si-model`'s outside world inside the node update: nothing the made-up chains reach.
struct ModelOut;

impl si_model::model_image::Outside for ModelOut {
    fn call(&mut self, _m: &mut Mem, _c: &si_model::model_image::Callee, _a: &[u32]) -> u32 {
        0
    }
}

fn vec_at(m: &Mem, a: u32) -> Vector {
    Vector { x: m.f32(a), y: m.f32(a + 4), z: m.f32(a + 8) }
}

/// `motion-evalcheck`'s stand-in for `SI_modelSetRotation (0x08463f54)` (its
/// `setters::set_rotation`): the angles, `mrot`, flag 4, the node update.
fn set_rotation(m: &mut Mem, node: Node, rot: u32) {
    let r = vec_at(m, rot);
    m[node].vrot = r;
    m[node].mrot = alg::matrix_xyz_rotation(&si_math::types::Vector::new(r.x, r.y, r.z));
    m[node].update_flags |= 4;
    m[node].update_flags2 |= 4;
    let f = m[node].father;
    si_model::model_image::state::update_son_state(m, &mut ModelOut, f, node, 1);
}

/// `motion-evalcheck`'s stand-in for `SI_modelSetGlobalTranslation (0x0846412c)`: the
/// position through the inverse of the father's `gmat` (itself for a root), flag 8, the
/// node update.
fn set_global_translation(m: &mut Mem, node: Node, pos: u32) {
    let p = vec_at(m, pos);
    let f = m[node].father;
    let t = if f.is_null() {
        p
    } else {
        match alg::matrix_inverse(&m[f].gmat) {
            Some(i) => {
                let c = |j: usize| (((x(p.y) * i[1][j] + x(p.x) * i[0][j]) + x(p.z) * i[2][j]) + i[3][j]).to_f32();
                Vector { x: c(0), y: c(1), z: c(2) }
            }
            None => {
                si_model::env::stale::note();
                p
            }
        }
    };
    m[node].trans = t;
    m[node].update_flags |= 8;
    m[node].update_flags2 |= 8;
    si_model::model_image::state::update_son_state(m, &mut ModelOut, f, node, 1);
}

impl Outside for EmuOut {
    fn call(&mut self, m: &mut Mem, r: &'static Routine, a: &[u32]) -> ORet {
        let p = |k: usize| a.get(k).copied().unwrap_or(0);
        let v = match r.name {
            "SI_edGetElemFromModel" => p(0).wrapping_add(EL),
            "SI_elemdictFindRelatedElem" => {
                let node = p(1).wrapping_sub(EL);
                self.last = p(4);
                match p(4) {
                    0x67 => spare(m, node) & 1,
                    0x74 => (spare(m, node) >> 8) & 1,
                    0x80 => m.u32(node + 0x35c),
                    0x66 => 1,
                    _ => 0,
                }
            }
            "SI_elemdictGetCurRelation" => {
                let node = p(1).wrapping_sub(EL);
                m.u32(node + if self.last == 0x74 { 0x358 } else { 0x2cc })
            }
            "SI_constrLocation" => {
                let b = m.bytes(p(0) + 0x1b8, 12).to_vec();
                m.set_bytes(p(2), &b);
                0
            }
            "SI_edIsItemConstrained" => match si_kine::invkin::BREAK_RELATIONS.iter().position(|&x| x as u32 == p(3)) {
                Some(i) => (spare(m, p(1)) >> (1 + i)) & 1,
                None => 0,
            },
            "SI_elemdictFindSerialRelation" => {
                let node = p(1).wrapping_sub(EL);
                if (spare(m, node) >> 7) & 1 == 0 || p(6) == 0 {
                    0
                } else {
                    m.u32(node + 0x2cc)
                }
            }
            "SI_getExprFcurveUsed" => {
                m.set_i32(p(1), 1);
                let b = m.malloc(8);
                m.set_i32(b, 0x8c);
                m.set_i32(b + 4, 0);
                b
            }
            "SI_classicSolvePartialInvKin" => {
                self.classic.push((p(2) as i32, [p(3), p(4), p(5)], p(6) as i16));
                0
            }
            "SI_modelUpdateSonState" => {
                si_model::model_image::state::update_son_state(m, &mut ModelOut, Ptr32::new(p(0)), Ptr32::new(p(1)), p(2) as i16);
                0
            }
            "SI_modelSetRotation" => {
                self.compensated |= p(2) != 1 || p(3) as u16 != 0;
                set_rotation(m, Ptr32::new(p(0)), p(1));
                0
            }
            "SI_modelSetGlobalTranslation" => {
                self.compensated |= p(2) != 1 || p(3) as u16 != 0;
                set_global_translation(m, Ptr32::new(p(0)), p(1));
                0
            }
            other => {
                self.unknown.push(other);
                0
            }
        };
        ORet::int(v)
    }
}

/// A case in memory: the buffers and their names.
struct Loaded {
    m: Mem,
    names: Names,
}

fn load(c: &Case) -> Result<Loaded, String> {
    let mut m = Mem::new();
    // the constants `model.c` reads at their addresses (`SI_identite` …), as in the binary
    si_model::model_image::init(&mut m);
    let mut fields: Vec<(String, Option<u32>, Option<Vec<W>>)> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for f in &c.inputs {
        seen.insert(f.name.clone());
        let at = GLOBALS.iter().find(|g| g.0 == f.name).map(|g| g.1);
        fields.push((f.name.clone(), at, words(f)));
    }
    // a buffer the routine only writes: room of its size, holding the fill
    for f in &c.outputs {
        if seen.contains(&f.name) || RESULTS.contains(&f.name.as_str()) || f.name.starts_with("heap") {
            continue;
        }
        let w = words(f).map(|ws| ws.iter().map(|w| W::Unwritten { bytes: w_bytes(w) }).collect());
        fields.push((f.name.clone(), None, w));
    }
    let refs: Vec<si_mem::FieldAt<'_>> = fields.iter().map(|(n, a, w)| (n.as_str(), *a, w.as_deref())).collect();
    let names = si_mem::load_case_at(&mut m, &refs)?;
    Ok(Loaded { m, names })
}

fn w_bytes(w: &W) -> u8 {
    match w {
        W::Value { bytes, .. } | W::Unwritten { bytes } => *bytes,
        W::Ref { .. } => 4,
    }
}

impl Loaded {
    fn addr(&self, name: &str) -> u32 {
        self.names.addr(&self.m, name).unwrap_or_else(|| panic!("no buffer `{name}`"))
    }
    fn word(&self, name: &str) -> u32 {
        self.m.u32(self.addr(name))
    }
    fn i(&self, name: &str) -> i32 {
        self.word(name) as i32
    }
    fn s(&self, name: &str) -> i16 {
        self.m.i16(self.addr(name))
    }
    fn f(&self, name: &str) -> f32 {
        self.m.f32(self.addr(name))
    }
    fn node(&self, name: &str) -> Node {
        Ptr32::new(self.word(name))
    }
    fn v(&self, name: &str) -> Vector {
        vec_at(&self.m, self.addr(name))
    }
    fn put_v(&mut self, name: &str, v: Vector) {
        let a = self.addr(name);
        self.m.set_f32(a, v.x);
        self.m.set_f32(a + 4, v.y);
        self.m.set_f32(a + 8, v.z);
    }
}

/// What a port returned.
enum Res {
    None,
    Eax(u32),
    Ax(u32),
    St0(F80),
    Ptr(u32),
}

type Port = fn(&mut Loaded, &mut EmuOut) -> Res;

fn run(c: &Case, port: Port) -> Verdict {
    let mut l = match load(c) {
        Ok(l) => l,
        Err(e) => return Verdict::Different(format!("loading the case: {e}")),
    };
    si_kine::fault::reset();
    si_model::env::stale::reset();
    si_kine::jacobian::rounds::take();
    let mut o = EmuOut::default();
    let show = |l: &Loaded, when: &str| {
        if std::env::var_os("THIRD_DEBUG").is_some() {
            for k in 0..12 {
                let Some(a) = l.names.addr(&l.m, &format!("m{k}")) else { break };
                let n: Node = Ptr32::new(a);
                eprintln!("{when} m{k} {a:#x} father {:#x} son {:#x} kind {} algo {} hrot00 {} grot00 {} gmat00 {} def00 {}", l.m[n].father.addr(), l.m[n].son.addr(), l.m[n].kind, l.m[n].algo_type, l.m[n].hrot[0][0], l.m[n].grot[0][0], l.m[n].gmat[0][0], l.m[n].def_matrix[0][0]);
            }
        }
    };
    show(&l, "before");
    let res = port(&mut l, &mut o);
    show(&l, "after");
    let rounds = si_kine::jacobian::rounds::take();
    if si_kine::fault::count() != 0 || si_model::env::stale::count() != 0 || l.m.fault_count() != 0 || o.compensated {
        return Verdict::Skip;
    }
    if let Some(u) = o.unknown.first() {
        return Verdict::Different(format!("the port called {u}, which the generator does not stand in for"));
    }
    // the heap blocks the port took, by the harness's names
    let blocks: Vec<(u32, u32)> = l.m.heap_blocks().to_vec();
    for (i, (a, n)) in blocks.iter().enumerate() {
        l.names.add(&format!("heap{i}"), *a, *n);
    }
    let flip = |v: u32| v ^ mutate() as u32;
    // the result
    match res {
        Res::None => {}
        Res::Eax(v) => {
            let want = c.output("eax").u32();
            if want != flip(v) {
                return Verdict::Different(format!("eax {want:#x} against the port's {:#x}", flip(v)));
            }
        }
        Res::Ax(v) => {
            let want = c.output("ax").words()[0].value().unwrap_or(0) as u32 & 0xffff;
            if want != flip(v) & 0xffff {
                return Verdict::Different(format!("ax {want} against the port's {}", flip(v)));
            }
        }
        Res::St0(v) => {
            let v = if mutate() {
                let (mm, se) = v.to_bits();
                F80::from_bits(mm ^ 1, se)
            } else {
                v
            };
            if !c.output("st0").same_f80(v) {
                return Verdict::Different(format!("st0 {:?} against the port's {v:?}", c.output("st0").f80()));
            }
        }
        Res::Ptr(v) => {
            let want = match &c.output("ptr").words()[0] {
                Word::Ref { name, offset } => l.names.addr(&l.m, name).map(|a| a + offset),
                Word::Value { value, .. } => Some(*value as u32),
                Word::Unwritten { .. } => None,
            };
            if want != Some(flip(v)) {
                return Verdict::Different(format!("ptr {want:x?} against the port's {:#x}", flip(v)));
            }
        }
    }
    if let Some(r) = c.try_output("rounds") {
        let got = [flip(rounds[0]), rounds[1], rounds[2]];
        if !r.same_u32(&got) {
            return Verdict::Different(format!("rounds (planar steps, spatial steps, pseudo-inverses): {:?} against the port's {got:?}", r.u32s()));
        }
    }
    let want_classic: Vec<(i32, [u32; 3], i16)> = c
        .calls
        .iter()
        .filter(|k| k.name == "SI_classicSolvePartialInvKin")
        .map(|k| {
            let a: Vec<u32> = k.args.iter().map(|w| w.value().unwrap_or(0) as u32).collect();
            (a[2] as i32, [a[3], a[4], a[5]], a[6] as i16)
        })
        .collect();
    if want_classic != o.classic {
        return Verdict::Different(format!("calls of SI_classicSolvePartialInvKin {want_classic:x?} against the port's {:x?}", o.classic));
    }
    // the blocks given back with `free`, in order
    if let Some(f) = c.try_output("freed") {
        let want: Vec<Option<u32>> = f
            .words()
            .iter()
            .map(|w| match w {
                Word::Ref { name, offset } => l.names.addr(&l.m, name).map(|a| a + offset),
                Word::Value { value, .. } => Some(*value as u32),
                Word::Unwritten { .. } => None,
            })
            .collect();
        let got: Vec<Option<u32>> = l.m.freed().iter().map(|&a| Some(a)).collect();
        if want != got {
            return Verdict::Different(format!("freed: {} blocks in the original, {} in the port", want.len(), got.len()));
        }
    }
    // every output buffer
    let mut flipped = !mutate();
    for f in &c.outputs {
        if RESULTS.contains(&f.name.as_str()) {
            continue;
        }
        let Some(addr) = l.names.addr(&l.m, &f.name) else {
            return Verdict::Different(format!("the original wrote `{}`, which the port has no buffer for", f.name));
        };
        let Some(ws) = words(f) else { continue };
        if !flipped && matches!(ws.first(), Some(W::Value { .. })) {
            let b = l.m.u8(addr);
            l.m.set_u8(addr, b ^ 1);
            flipped = true;
        }
        if let Err(e) = l.m.compare_words(addr, &ws, &l.names) {
            if std::env::var_os("THIRD_DEBUG").is_some() {
                // every differing word of the buffer, for reading
                let mut at = addr;
                for (i, w) in ws.iter().enumerate() {
                    let n = w_bytes(w) as u32;
                    if let Err(e) = l.m.compare_words(at, std::slice::from_ref(w), &l.names) {
                        eprintln!("{} {} word {i}: {e}", c.name, f.name);
                    }
                    at += n;
                }
            }
            return Verdict::Different(format!("{}: {e}", f.name));
        }
    }
    Verdict::Same
}

// ---------------------------------------------------------------------------- the ports

fn limb_length(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let (sum, max, min) = img::tools::limb_length(&l.m, l.node("node"), l.i("n"));
    let (amx, amn) = (l.addr("mx"), l.addr("mn"));
    l.m.set_f32(amn, min);
    l.m.set_f32(amx, max);
    Res::St0(sum)
}

fn volume_projection(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let (code, p) = img::tools::volume_projection(&l.m, l.node("node"), l.i("n"), l.v("goal"));
    l.put_v("out", p);
    Res::Eax(code as u32)
}

fn limb_position(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let p = img::tools::limb_position(&l.m, l.node("node"), l.i("n"));
    l.put_v("ret", p);
    Res::None
}

fn special_update(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let (f, n) = (l.node("father"), l.node("node"));
    img::tools::model_special_update_son_state(&mut l.m, f, n);
    Res::None
}

fn chain_update(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, s) = (l.node("node"), l.s("sons"));
    img::tools::chain_update(&mut l.m, o, n, s);
    Res::None
}

fn joint_angle_in_range(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let n = l.node("node");
    img::jacobian::joint_angle_in_range(&mut l.m, n);
    Res::None
}

fn compute_jacobian_2d(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let (n, j, k) = (l.node("node"), Ptr32::new(l.word("j")), l.i("n"));
    img::jacobian::compute_jacobian_2d(&mut l.m, n, j, k);
    Res::None
}

fn compute_jacobian_3d(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let (n, j, k) = (l.node("node"), Ptr32::new(l.word("j")), l.i("n"));
    img::jacobian::compute_jacobian_3d(&mut l.m, n, j, k);
    Res::None
}

fn compute_joints_2d(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (d, n, k, j, inv, rc, tol, t) = (l.i("depth"), l.node("node"), l.i("n"), Ptr32::new(l.word("j")), Ptr32::new(l.word("inv")), l.s("recompute"), l.f("tol"), l.v("target"));
    img::jacobian::compute_joints_2d(&mut l.m, o, d, n, k, j, inv, rc != 0, tol, t);
    Res::None
}

fn with_stop(l: &mut Loaded, f: impl FnOnce(&mut Mem, &mut i16)) {
    let a = l.addr("stop");
    let mut stop = l.m.i16(a);
    f(&mut l.m, &mut stop);
    l.m.set_i16(a, stop);
}

fn compute_joints_3d(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (d, n, k, j, inv, rc, tol, t) = (l.i("depth"), l.node("node"), l.i("n"), Ptr32::new(l.word("j")), Ptr32::new(l.word("inv")), l.s("recompute"), l.f("tol"), l.v("target"));
    with_stop(l, |m, stop| img::jacobian::compute_joints_3d(m, o, d, n, k, j, inv, rc != 0, tol, t, stop));
    Res::None
}

fn iter_compute_joints_2d(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (r, n, k, reach, frac, tol, g) = (l.node("root"), l.node("node"), l.i("n"), l.f("reach"), l.f("frac"), l.f("tol"), l.v("goal"));
    img::jacobian::iter_compute_joints_2d(&mut l.m, o, r, n, k, reach, frac, tol, g);
    Res::None
}

fn iter_compute_joints_3d(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (r, n, k, reach, frac, tol, g) = (l.node("root"), l.node("node"), l.i("n"), l.f("reach"), l.f("frac"), l.f("tol"), l.v("goal"));
    with_stop(l, |m, stop| img::jacobian::iter_compute_joints_3d(m, o, r, n, k, reach, frac, tol, g, stop));
    Res::None
}

fn map_goal_to_root(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let p = img::jacobian::map_goal_to_root(&l.m, l.node("node"), l.v("goal"));
    l.put_v("out", p);
    Res::None
}

fn find_resolution_plane(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let (n, t, r) = (l.node("node"), l.i("type"), l.s("roll"));
    let mut g = l.v("goal");
    let plane = img::jacobian::find_resolution_plane(&mut l.m, n, t, r, &mut g);
    l.put_v("goal", g);
    let a = l.addr("plane");
    for (i, row) in plane.iter().enumerate() {
        for (k, v) in row.iter().enumerate() {
            l.m.set_f32(a + 16 * i as u32 + 4 * k as u32, *v);
        }
    }
    Res::None
}

fn chain_2d_roll_apply(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let (n, k) = (l.node("node"), l.i("n"));
    img::jacobian::chain_2d_roll_apply(&mut l.m, n, k);
    Res::None
}

fn invkin0(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, g) = (l.node("node"), l.v("goal"));
    img::jacobian::invkin0(&mut l.m, o, n, g);
    Res::None
}

fn invkin1(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, g) = (l.node("node"), l.v("goal"));
    img::jacobian::invkin1(&mut l.m, o, n, g);
    Res::None
}

fn invkin2d(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (r, n, k, g, f) = (l.node("root"), l.node("node"), l.i("n"), l.v("goal"), l.s("first"));
    img::jacobian::invkin2d(&mut l.m, o, r, n, k, g, f);
    Res::None
}

fn invkin3d(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (r, n, k, g, f) = (l.node("root"), l.node("node"), l.i("n"), l.v("goal"), l.s("first"));
    with_stop(l, |m, stop| img::jacobian::invkin3d(m, o, r, n, k, g, f, stop));
    Res::None
}

fn check_branch_orient(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, g, k) = (l.node("node"), l.v("goal"), l.i("n"));
    Res::Ax(img::jacobian::check_branch_orient(&mut l.m, o, n, g, k) as u32)
}

fn check_branch_pullout(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, g, k) = (l.node("node"), l.v("goal"), l.i("n"));
    Res::Ax(img::jacobian::check_branch_pullout(&mut l.m, o, n, g, k) as u32)
}

fn solve_partial_inv_kin(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (r, f, k, g, p) = (l.node("root"), l.node("first"), l.i("n"), l.v("goal"), l.s("pass"));
    with_stop(l, |m, stop| img::jacobian::solve_partial_inv_kin(m, o, r, f, k, g, p, stop));
    Res::None
}

fn model_solve_ik(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (r, f, e, g, k) = (l.node("root"), l.node("first"), l.node("eff"), l.v("goal"), l.i("n"));
    img::invkin::model_solve_ik(&mut l.m, o, r, f, e, g, k);
    Res::None
}

fn model_solve_inv_kin(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, g, t) = (l.node("node"), l.v("goal"), l.f("time"));
    img::invkin::model_solve_inv_kin(&mut l.m, o, n, g, t);
    Res::None
}

fn inv_kin_allocate(l: &mut Loaded, _: &mut EmuOut) -> Res {
    Res::Ptr(img::invkin::inv_kin_allocate(&mut l.m).addr())
}

fn reset_pref_angle(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let (n, g, a) = (l.node("node"), l.word("group"), l.v("angles"));
    Res::Ax(img::invkin::inv_kin_reset_pref_angle(&mut l.m, n, g, a) as u16 as u32)
}

fn check_scale(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let r = img::invkin::check_scale_before_ik(&l.m, l.node("first"), l.node("last"), l.i("n"));
    if let Some(cut) = r {
        let a = l.addr("out");
        l.m.set_u32(a, cut.addr());
    }
    Res::Ax(r.is_some() as u32)
}

fn sub_goal(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let p = img::invkin::find_sub_goal_before_ik(&l.m, l.node("first"), l.node("cut"), l.node("eff"), l.v("goal"));
    l.put_v("out", p);
    Res::None
}

fn is_break(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, f, t) = (l.node("node"), l.s("flag"), l.f("time"));
    Res::Ax(img::invkin::inv_kin_is_break_jnt(&mut l.m, o, n, f, t) as u32)
}

fn first_jnt(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, f, t) = (l.node("node"), l.s("flag"), l.f("time"));
    let (p, k) = img::invkin::inv_kin_get_sub_chain_first_jnt(&mut l.m, o, n, f, t);
    let a = l.addr("cnt");
    l.m.set_i32(a, k);
    Res::Ptr(p.addr())
}

fn sub_effector(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, f, t) = (l.node("node"), l.s("flag"), l.f("time"));
    let (p, k) = img::invkin::inv_kin_get_sub_chain_effector(&mut l.m, o, n, f, t);
    let a = l.addr("cnt");
    l.m.set_i32(a, k);
    Res::Ptr(p.addr())
}

fn set_goal(l: &mut Loaded, _: &mut EmuOut) -> Res {
    let (n, g) = (l.node("node"), l.v("goal"));
    img::invkin::inv_kin_set_goal(&mut l.m, n, g);
    Res::None
}

fn set_cns_position(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, k, p, t) = (l.node("node"), l.i("type"), l.v("pos"), l.f("time"));
    img::invkin::inv_kin_set_cns_position(&mut l.m, o, n, k, p, t);
    Res::None
}

fn set_flag(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, k) = (l.node("node"), l.i("n"));
    img::rotlim::jnt_rotlim_set_flag(&mut l.m, o, n, k);
    Res::None
}

fn constrained_branch(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, k) = (l.node("node"), l.i("n"));
    Res::Ax(img::rotlim::rotlim_constrained_branch(&mut l.m, o, n, k) as u32)
}

fn check_jnt_rot_lim(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let n = l.node("node");
    Res::Eax(img::rotlim::check_jnt_rot_lim(&mut l.m, o, n) as u32)
}

fn check_rotation_limit(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (pn, pc) = (l.addr("pn"), l.addr("pc"));
    let mut node: Node = Ptr32::new(l.m.u32(pn));
    let mut n = l.m.i32(pc);
    let r = img::rotlim::check_rotation_limit(&mut l.m, o, &mut node, &mut n);
    l.m.set_u32(pn, node.addr());
    l.m.set_i32(pc, n);
    Res::Eax(r as u32)
}

fn constrained_jnt(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, k) = (l.node("node"), l.i("n"));
    Res::Eax(img::rotlim::constrained_jnt(&mut l.m, o, n, k) as u32)
}

fn rotlim_constraint(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, i) = (l.node("node"), l.i("index"));
    img::limits::rotlim_constraint(&mut l.m, o, n, i);
    Res::None
}

fn poslim_constraint(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, c, i) = (l.node("node"), l.v("centre"), l.i("index"));
    img::limits::poslim_constraint(&mut l.m, o, n, c, i);
    Res::None
}

fn constr_3pts(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, a, b) = (l.node("node"), l.v("a"), l.v("b"));
    Res::Ax(img::mapdir::constr_3pts_map_direction(&mut l.m, o, n, a, b) as u32)
}

fn nor_constr(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, a, b) = (l.node("node"), l.v("a"), l.v("b"));
    Res::Ax(img::mapdir::nor_constr_map_direction(&mut l.m, o, n, a, b) as u32)
}

fn tan_constr(l: &mut Loaded, o: &mut EmuOut) -> Res {
    let (n, a) = (l.node("node"), l.v("a"));
    Res::Ax(img::mapdir::tan_constr_map_direction(&mut l.m, o, n, a) as u32)
}

/// The image form of a first-pass routine, by the name its vector files carry.
fn port_of(name: &str) -> Option<Port> {
    Some(match name {
        "si_limbLength" => limb_length,
        "SI_volumeProjection" => volume_projection,
        "si_limbPosition" => limb_position,
        "SI_modelSpecialUpdateSonState" => special_update,
        "SI_chainUpdate" => chain_update,
        "jointAngleInRange" => joint_angle_in_range,
        "computeJacobian2d" => compute_jacobian_2d,
        "computeJacobian3d" => compute_jacobian_3d,
        "computeJoints2d" => compute_joints_2d,
        "computeJoints3d" => compute_joints_3d,
        "iterComputeJoints2d" => iter_compute_joints_2d,
        "iterComputeJoints3d" => iter_compute_joints_3d,
        "mapGoalToRoot" => map_goal_to_root,
        "findResolutionPlane" => find_resolution_plane,
        "chain2DRollApply" => chain_2d_roll_apply,
        "invkin0" => invkin0,
        "invkin1" => invkin1,
        "invkin2d" => invkin2d,
        "invkin3d" => invkin3d,
        "checkBranchOrient" => check_branch_orient,
        "si_checkBranchPullout" => check_branch_pullout,
        "SI_solvePartialInvKin" => solve_partial_inv_kin,
        "modelSolveIK" => model_solve_ik,
        "SI_modelSolveInvKin" => model_solve_inv_kin,
        "SI_invKinAllocate" => inv_kin_allocate,
        "SI_invKinResetPrefAngle" => reset_pref_angle,
        "checkScaleBeforeIK" => check_scale,
        "findSubGoalBeforeIK" => sub_goal,
        "SI_invKinIsBreakJnt" => is_break,
        "SI_invKinGetSubChainFirstJnt" => first_jnt,
        "SI_invKinGetSubChainEffector" => sub_effector,
        "SI_invKinSetGoal" => set_goal,
        "SI_invKinSetCnsPosition" => set_cns_position,
        "SI_jntRotlimSetFlag" => set_flag,
        "SI_rotlimConstrainedBranch" => constrained_branch,
        "si_checkJntRotLim" => check_jnt_rot_lim,
        "SI_checkRotationLimit" => check_rotation_limit,
        "SI_constrainedJnt" => constrained_jnt,
        "si_rotlimConstraint" => rotlim_constraint,
        "si_poslimConstraint" => poslim_constraint,
        "si_constr3PtsMapDirection" => constr_3pts,
        "si_norConstrMapDirection" => nor_constr,
        "si_tanConstrMapDirection" => tan_constr,
        _ => return None,
    })
}

/// One case of a routine [`port_of`] knows, by the case's name (the routine's, before `#`).
fn check(c: &Case) -> Verdict {
    match port_of(c.name.split('#').next().unwrap_or("")) {
        Some(p) => run(c, p),
        None => Verdict::Skip,
    }
}

/// The check of a vector file's routine, when its image form exists.
pub fn lookup(name: &str) -> Option<Check> {
    port_of(name).map(|_| check as Check)
}
