// SPDX-License-Identifier: MIT
//! A chain world rebuilt from a vector's input buffers, and compared with its outputs.
//!
//! `re/tools/si-kine/emu_gen.py` lays a chain out in emulator memory as node records
//! `m<k>`, their inverse-kinematics records `ik<k>`, the root's two matrices and, per node,
//! a rotation-limit relation entry `rl<k>` with its data `rd<k>` (the script's header says
//! what each buffer holds and which spare words of a node record answer for the scene
//! dictionary). [`load`] builds the same chain in the crate's terms: node k is `NodeId` k,
//! its record is record k of the store (pointer value k + 1), the Jacobian is matrix 1 and
//! its pseudo-inverse matrix 2.

use si_kine::rec::{IkStore, InvKin};
use si_kine::{Globals, Host, RotlimData, RotlimRelation, SerialRelation};
use si_math::matrix_gen_dbl::GenMatrixDbl;
use si_model::node::Models;
use si_model::NodeId;
use si_types::geom::Vector;
use si_types::model::Model;
use si_types::Ptr32;
use si_x87::vectors::{Case, Field, Verdict, Word};

pub const WORDS: usize = 0x430 / 4;
pub const IK_WORDS: usize = 0x74 / 4;

/// `EMUCHECK_MUTATE=1`: one bit of everything the port produces is flipped before it is
/// compared, to show that the comparisons are live: every case that compares anything
/// must then differ.
pub fn mutate() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("EMUCHECK_MUTATE").is_some())
}

pub fn mut32(v: u32) -> u32 {
    if mutate() { v ^ 0x0001_0000 } else { v }
}

fn number(name: &str, prefix: &str) -> Option<u32> {
    name.strip_prefix(prefix).and_then(|k| k.parse().ok())
}

/// A word of a buffer as the number the port keeps in its place: a pointer to node k or to
/// record k is k + 1; the matrices are 1 and 2; a relation entry `rl<k>` is 0x1000 + k; a
/// block the routine allocated is 0x2000 + its number; any other pointer 0x3000.
pub fn value(w: &Word) -> u32 {
    match w {
        Word::Value { value, .. } => *value as u32,
        Word::Unwritten { .. } => 0,
        Word::Ref { name, offset } => {
            if *offset != 0 {
                return 0x3000 + *offset;
            }
            if let Some(k) = number(name, "m").or_else(|| number(name, "ik")) {
                k + 1
            } else if name == "ja" {
                1
            } else if name == "jb" {
                2
            } else if let Some(k) = number(name, "rl") {
                0x1000 + k
            } else if let Some(k) = number(name, "heap") {
                0x2000 + k
            } else {
                0x3000
            }
        }
    }
}

pub fn values(f: &Field) -> Vec<u32> {
    f.words().iter().map(value).collect()
}

fn f(w: u32) -> f32 {
    f32::from_bits(w)
}

pub fn record(f: &Field) -> Model {
    let w = values(f);
    assert_eq!(w.len(), WORDS);
    // every field of the record is plain data: any bit pattern is a value
    unsafe { core::ptr::read_unaligned(w.as_ptr() as *const Model) }
}

pub fn ik_record(f: &Field) -> InvKin {
    let w = values(f);
    assert_eq!(w.len(), IK_WORDS);
    unsafe { core::ptr::read_unaligned(w.as_ptr() as *const InvKin) }
}

pub fn words_of(m: &Model) -> Vec<u32> {
    let mut w = vec![0u32; WORDS];
    unsafe { core::ptr::copy_nonoverlapping(m as *const Model as *const u8, w.as_mut_ptr() as *mut u8, 0x430) };
    w
}

pub fn ik_words(r: &InvKin) -> Vec<u32> {
    let mut w = vec![0u32; IK_WORDS];
    unsafe { core::ptr::copy_nonoverlapping(r as *const InvKin as *const u8, w.as_mut_ptr() as *mut u8, 0x74) };
    w
}

/// The stand-in for the scene dictionary and for `classic.c`, answering from the spare
/// words of the node records as the generator's stand-ins do.
#[derive(Default)]
pub struct EmuHost {
    /// Per node: the relation entry `rl<k>` (activation flag, data) as the vector has it.
    pub relations: Vec<(i16, i32, Option<RotlimData>)>,
    /// Calls of `SI_classicSolvePartialInvKin`: n, goal bits, first pass.
    pub classic: Vec<(i32, [u32; 3], i16)>,
    /// Per node: the selection state of its element's graf node (`gn<k>` + 8).
    pub selection: Vec<i32>,
    /// Per node: the buffer `rd<k>` read as a position limit's data.
    pub poslim: Vec<si_kine::limits::PoslimData>,
    /// Per node: the word the rotation limit's update left for relation 116.
    pub states: Vec<Option<i16>>,
    /// The model at the other end of an up-vector relation, for the node whose spare word
    /// at +0x35c is set.
    pub up_object: Option<NodeId>,
    /// How often `SI_displayModelScal`, `SI_modelRotationRefMatrixSet` and
    /// `si_classicSkeMoveJointBranch` were called.
    pub tools: [u32; 3],
}

impl si_model::transform::Hooks for EmuHost {}
impl si_model::transform::GeometryHooks for EmuHost {}
impl si_model::hierarchy::Hooks for EmuHost {}
impl si_model::node_copy::Hooks for EmuHost {}

fn spare(models: &Models, node: NodeId) -> u16 {
    u16::from_le_bytes(models.m(node).pad_0e)
}

impl EmuHost {
    fn relation(&self, models: &Models, node: NodeId) -> Option<(i16, i32, Option<RotlimData>)> {
        let h = models.m(node).unk_2cc[0];
        if h == 0 { None } else { self.relations.get((h - 0x1000) as usize).copied() }
    }
}

impl Host for EmuHost {
    fn find_rotlim(&mut self, models: &Models, node: NodeId) -> bool {
        spare(models, node) & 1 != 0
    }

    fn cur_rotlim(&mut self, models: &Models, node: NodeId) -> Option<RotlimRelation> {
        self.relation(models, node).map(|(active, _, data)| RotlimRelation { active, data })
    }

    fn classic_solve_partial_inv_kin(&mut self, _: &mut Models, _: &mut IkStore, _root: NodeId, _first: NodeId, n: i32, goal: Vector, first_pass: i16, _stop: &mut i16) {
        self.classic.push((n, [goal.x.to_bits(), goal.y.to_bits(), goal.z.to_bits()], first_pass));
    }

    fn cur_poslim(&mut self, models: &Models, node: NodeId) -> Option<si_kine::limits::PoslimData> {
        let (_, _, data) = self.relation(models, node)?;
        data.map(|_| self.poslim[(models.m(node).unk_2cc[0] - 0x1000) as usize])
    }

    fn up_vector_object(&mut self, models: &Models, node: NodeId) -> Option<NodeId> {
        if models.m(node).unk_358[1] != 0 { self.up_object } else { None }
    }

    fn elem_selection(&mut self, _: &Models, node: NodeId) -> i32 {
        self.selection.get(node).copied().unwrap_or(0)
    }

    fn set_rotlim_state(&mut self, models: &Models, node: NodeId, state: i16) {
        if spare(models, node) & 0x100 != 0 {
            self.states[node] = Some(state);
        }
    }

    fn display_model_scal(&mut self, _: &Models, _: NodeId) {
        self.tools[0] += 1;
    }

    fn model_rotation_ref_matrix_set(&mut self, _: &mut Models, _: Option<&si_model::setters::View>, _: NodeId, _: Vector) {
        self.tools[1] += 1;
    }

    fn classic_ske_move_joint_branch(&mut self, _: &mut Models, _: &mut IkStore, _: NodeId, _: Vector, _: i16, _: i16) {
        self.tools[2] += 1;
    }

    fn ed_is_item_constrained(&mut self, models: &Models, node: NodeId, relation: i32, _flag: i16, _time: f32) -> bool {
        match si_kine::invkin::BREAK_RELATIONS.iter().position(|&r| r == relation) {
            Some(i) => (spare(models, node) >> (1 + i)) & 1 != 0,
            None => false,
        }
    }

    fn find_serial_relation(&mut self, models: &Models, node: NodeId, first: bool) -> Option<SerialRelation> {
        if spare(models, node) & 0x80 == 0 || !first {
            return None;
        }
        self.relation(models, node).map(|(active, activation, _)| SerialRelation { active, activation })
    }

    fn expr_fcurve_used(&mut self, _: &Models, _: NodeId) -> Vec<i32> {
        vec![0x8c]
    }
}

/// A rotation-limit record from its buffer (`f32*6,u16*6,f32*6,f32*2`).
pub fn rotlim_data(field: &Field) -> RotlimData {
    let d = values(field);
    let v3 = |i: usize| Vector { x: f(d[i]), y: f(d[i + 1]), z: f(d[i + 2]) };
    RotlimData {
        min_deg: v3(0),
        max_deg: v3(3),
        min_active: [d[6] as i16, d[7] as i16, d[8] as i16],
        max_active: [d[9] as i16, d[10] as i16, d[11] as i16],
        min: v3(12),
        max: v3(15),
        damping: [f(d[18]), f(d[19])],
    }
}

pub struct World {
    pub models: Models,
    pub ik: IkStore,
    pub g: Globals,
    pub host: EmuHost,
    pub nn: usize,
}

/// Node k of a case, from a pointer argument; `None` for null.
pub fn node_arg(c: &Case, name: &str) -> Option<NodeId> {
    match &c.input(name).words()[0] {
        Word::Ref { name, .. } => number(name, "m").map(|k| k as usize),
        _ => None,
    }
}

pub fn vec3(c: &Case, name: &str) -> Vector {
    let v = c.input(name).f32s();
    Vector { x: v[0], y: v[1], z: v[2] }
}

fn matrix(c: &Case, head: &str, data: &str) -> Option<GenMatrixDbl> {
    let h = values(c.try_input(head)?);
    let (rows, cols) = ((h[0] as i32).max(0) as usize, (h[1] as i32).max(0) as usize);
    let d = c.input(data).f64s();
    Some(GenMatrixDbl { rows, cols, data: d[..rows * cols].to_vec() })
}

pub fn load(c: &Case) -> World {
    let mut models = Models::default();
    let mut ik = IkStore::default();
    let mut host = EmuHost::default();
    let mut nn = 0;
    while let Some(fm) = c.try_input(&format!("m{nn}")) {
        let id = models.allocate(None);
        *models.m_mut(id) = record(fm);
        ik.recs.push(Some(ik_record(c.input(&format!("ik{nn}")))));
        let rl = c.input(&format!("rl{nn}"));
        let rw = values(rl);
        let data = match &rl.words()[7] {
            Word::Ref { .. } => {
                Some(rotlim_data(c.input(&format!("rd{nn}"))))
            }
            _ => None,
        };
        host.relations.push((rw[5] as i16, rw[2] as i32, data));
        host.selection.push(c.try_input(&format!("gn{nn}")).map_or(0, |g| values(g)[2] as i32));
        host.states.push(None);
        let d = values(c.input(&format!("rd{nn}")));
        let v3 = |i: usize| Vector { x: f(d[i]), y: f(d[i + 1]), z: f(d[i + 2]) };
        host.poslim.push(si_kine::limits::PoslimData {
            min: v3(0),
            max: v3(3),
            min_active: [d[6] as i16, d[7] as i16, d[8] as i16],
            max_active: [d[9] as i16, d[10] as i16, d[11] as i16],
            damping: [f(d[12]), f(d[13])],
            is_ref_world: d[14] as i16,
            is_area_box: (d[14] >> 16) as i16,
            radius: f(d[15]),
        });
        nn += 1;
    }
    host.up_object = c.try_input("ue").and_then(|u| match &u.words()[1] {
        Word::Ref { name, .. } => number(name, "m").map(|k| k as usize),
        _ => None,
    });
    ik.mats.push(matrix(c, "ja", "jd"));
    ik.mats.push(matrix(c, "jb", "jbd"));
    for k in 0..nn {
        si_kine::rec::push_tx(&mut models, &ik, k);
    }
    let manip = c.try_input("manip").map(values).unwrap_or_else(|| vec![0; 17]);
    let mut g = Globals::default();
    g.model.classical_scaling = manip[0] == 1;
    g.uniform_scaling = manip[1] as i32;
    g.model.rot_mode = manip[2] as i32;
    g.model.trans_mode = manip[3] as i32;
    g.model.keep_turns = manip[9] != 0;
    g.old_joint_roll = manip[15] != 0;
    g.pref_follows_input = manip[11] != 0;
    g.model.viewer_upd_opt = c.try_input("upd").map_or(0, |f| f.u32());
    g.model.envelope_hold = c.try_input("hold").map_or(0, |f| values(f)[0] as i16);
    g.interactive = c.try_input("inter").is_some_and(|f| values(f)[0] != 0);
    g.model.interactive = g.interactive;
    World { models, ik, g, host, nn }
}

fn diff(name: &str, want: &Field, got: &[u32]) -> Verdict {
    let w = want.words();
    if w.len() != got.len() {
        return Verdict::Different(format!("{name}: {} words against the port's {}", w.len(), got.len()));
    }
    for (i, (a, &b)) in w.iter().zip(got).enumerate() {
        if matches!(a, Word::Unwritten { .. }) {
            continue;
        }
        let b = mut32(b);
        if value(a) != b {
            return Verdict::Different(format!("{name} +{:#x}: {:08x} ({}) against the port's {b:08x} ({})", 4 * i, value(a), f(value(a)), f(b)));
        }
    }
    Verdict::Same
}

/// A record the routine allocated (`heap<i>`) against a record of the port's store.
pub fn same_new_record(c: &Case, heap: &str, got: Option<&InvKin>) -> Verdict {
    let Some(got) = got else { return Verdict::Different(format!("{heap}: the port made no record")) };
    let want = c.output(heap);
    let mut g = ik_words(got);
    // the two pointers are compared by what they lead to elsewhere
    for i in [11, 12] {
        if matches!(want.words()[i], Word::Ref { .. }) {
            g[i] = value(&want.words()[i]) ^ if mutate() { 0x0001_0000 } else { 0 };
        }
    }
    diff(heap, want, &g)
}

/// Everything a case's world holds after the call against the port's: every node record,
/// every inverse-kinematics record, both matrices, and the count of solver rounds.
pub fn same(c: &Case, w: &World) -> Verdict {
    for k in 0..w.nn {
        let name = format!("m{k}");
        let Some(want) = c.try_output(&name) else { continue };
        let mut got = words_of(w.models.m(k));
        // a record the routine allocated for the node: compared by content
        if let Word::Ref { name: h, .. } = &want.words()[0x318 / 4] {
            if h.starts_with("heap") {
                let v = same_new_record(c, h, w.ik.of(&w.models, k));
                if v != Verdict::Same {
                    return v;
                }
                got[0x318 / 4] = value(&want.words()[0x318 / 4]) ^ if mutate() { 0x0001_0000 } else { 0 };
            }
        }
        let v = diff(&name, want, &got);
        if v != Verdict::Same {
            return v;
        }
    }
    for k in 0..w.nn {
        let name = format!("ik{k}");
        let Some(want) = c.try_output(&name) else { continue };
        let Some(rec) = w.ik.recs[k].as_ref() else { continue };
        let v = diff(&name, want, &ik_words(rec));
        if v != Verdict::Same {
            return v;
        }
    }
    for (i, name) in ["jd", "jbd"].iter().enumerate() {
        let (Some(want), Some(m)) = (c.try_output(name), w.ik.mats[i].as_ref()) else { continue };
        let wv = want.written();
        for (j, v) in m.data.iter().enumerate() {
            let got = v.to_bits() ^ if mutate() { 1 << 20 } else { 0 };
            if let Some(Some(x)) = wv.get(j) {
                if *x as u64 != got {
                    return Verdict::Different(format!("{name}[{j}]: {:016x} ({}) against the port's {got:016x} ({})", *x as u64, f64::from_bits(*x as u64), f64::from_bits(got)));
                }
            }
        }
    }
    for k in 0..w.nn {
        let (Some(want), Some(state)) = (c.try_output(&format!("rw{k}")), w.host.states[k]) else { continue };
        let got = (state as u16 as u32) ^ mutate() as u32;
        if value(&want.words()[0]) != got {
            return Verdict::Different(format!("rw{k}: the word of relation 116 is {:#x} against the port's {got:#x}", value(&want.words()[0])));
        }
    }
    if let Some(r) = c.try_output("rounds") {
        let got = si_kine::jacobian::rounds::take();
        let got = [mut32(got[0]), mut32(got[1]), mut32(got[2])];
        if !r.same_u32(&got) {
            return Verdict::Different(format!("rounds (planar steps, spatial steps, pseudo-inverses): {:?} against the port's {got:?}", r.u32s()));
        }
    }
    Verdict::Same
}

/// The calls of the three tool stand-ins the emulator recorded against the host's counts.
pub fn same_tools(c: &Case, w: &World) -> Verdict {
    let count = |name: &str| c.calls.iter().filter(|k| k.name == name).count() as u32;
    let want = [count("SI_displayModelScal"), count("SI_modelRotationRefMatrixSet"), count("si_classicSkeMoveJointBranch")];
    let got = [w.host.tools[0] + mutate() as u32, w.host.tools[1], w.host.tools[2]];
    Verdict::from(want == got, || format!("calls of the display, the reference rotation and the classical move: {want:?} against the port's {got:?}"))
}

/// A pointer result (`ptr`) against a node of the port.
pub fn same_node(c: &Case, got: Option<NodeId>) -> Verdict {
    let want = match &c.output("ptr").words()[0] {
        Word::Ref { name, offset: 0 } => number(name, "m").map(|k| k as usize),
        _ => None,
    };
    let got = if mutate() { got.map(|g| g ^ 1) } else { got };
    Verdict::from(want == got, || format!("node {want:?} against the port's {got:?}"))
}

pub fn ptr(p: u32) -> Ptr32<si_types::model::InvKin> {
    Ptr32::new(p)
}
