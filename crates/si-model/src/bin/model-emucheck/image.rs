// SPDX-License-Identifier: MIT
//! Checks of the image forms (`si_model::model_image`; vectors of `emu_gen4.py`, and with
//! `MODEL_IMAGE=1` the earlier passes' vectors of the routines that exist in both forms).
//!
//! A case is not rebuilt in structures of the check's own. Its buffers go into `si-mem`'s
//! image as they are (`load_case_at`: the arena buffers where the harness lays them out,
//! a field named `g<address>` at that address), the port runs on that memory, and then
//!
//! 1. every call the original made out of the package must have been made by the port, in
//!    order, with the same arguments (a pointer compared as "this buffer at this offset",
//!    a string by its text); the port gets the recorded answer;
//! 2. every buffer the vector lists as output is held against memory, word for word,
//!    wherever the original wrote; so is every heap block, by its number in the order of
//!    allocation, and the number of blocks;
//! 3. the result (`eax`, `ax`, `al`, a pointer, the x87 register) and the blocks freed, in
//!    order, must be the original's;
//! 4. the port must not have faulted (a case in which the original did is not compared).
//!
//! Each case is run twice, on zeroed and on patterned fresh memory (see `run`).
//! `EMUCHECK_MUTATE=1` flips one bit of every run of bytes the port changed in the image,
//! of its result and of the first argument of every call it makes, before the comparison.

use si_mem::{FieldAt, Mem, Names, Ptr32, Word as W};
use si_model::model_image::{self as mi, Callee, Outside};
use si_x87::vectors::{Case, Check, Field, Verdict, Word};
use si_x87::F80;

use crate::world::mutate;

thread_local! {
    /// What the last dump routine wrote.
    static STDOUT: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// What a port returned.
#[allow(dead_code)]
pub enum Ret {
    None,
    Int(u32),
    St0(F80),
}

/// The address a field lies at when the vector fixes it: `g<8 hex digits>` is the
/// address; `manip` and `upd` are the two globals of the first pass's vectors.
fn fixed(name: &str) -> Option<u32> {
    match name {
        "manip" => Some(0x0878_1560),
        "upd" => Some(0x0867_9678),
        // the globals of the second pass's setter vectors (`SETG` of `emu_gen2.py`)
        "ipc" => Some(0x0878_17a0),
        "hold" => Some(0x0878_1486),
        "time" => Some(0x0869_bca2),
        "frame" => Some(0x0878_2040),
        "rate" => Some(0x0878_204c),
        "inter" => Some(0x0867_953e),
        "gis" => Some(0x086d_5e20),
        "pdp" => Some(0x0867_9314),
        "glp" => Some(0x0867_9498),
        _ => name.strip_prefix('g').filter(|h| h.len() == 8).and_then(|h| u32::from_str_radix(h, 16).ok()),
    }
}

/// An argument passed by value (not a buffer): named `v_…`, or one of the first pass's.
fn by_value(f: &Field) -> bool {
    f.name.starts_with("v_")
        || matches!(f.name.as_str(), "recurse" | "mode" | "geom" | "brothers")
        || (matches!(f.name.as_str(), "node" | "top" | "at" | "first") && f.words.as_ref().is_some_and(|w| w.len() == 1))
}

fn convert(f: &Field) -> Option<Vec<W>> {
    f.words.as_ref().map(|ws| {
        ws.iter()
            .map(|w| match w {
                Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
                Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
                Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
            })
            .collect()
    })
}

/// The arguments of a case: where its buffers lie, and its values.
pub struct Args<'a> {
    at: Vec<(String, u32)>,
    pub c: &'a Case,
}

impl Args<'_> {
    /// A pointer argument: the address of the buffer of that name (0 for a null one), or
    /// the value of the argument `v_<name>`.
    pub fn p(&self, name: &str) -> u32 {
        match self.at.iter().find(|b| b.0 == name) {
            Some(b) => b.1,
            None => self.w(name),
        }
    }

    /// A node argument.
    pub fn n(&self, name: &str) -> Ptr32<si_types::model::Model> {
        Ptr32::new(self.p(name))
    }

    /// An argument by value (`v_<name>`, or `<name>` in the first pass's vectors), as the
    /// word pushed: a narrow one sign-extended, `&buffer` its address.
    pub fn w(&self, name: &str) -> u32 {
        let f = self.c.try_input(&format!("v_{name}")).unwrap_or_else(|| self.c.input(name));
        match &f.words()[0] {
            Word::Value { bytes, value } => match bytes {
                1 => *value as u8 as i8 as i32 as u32,
                2 => *value as u16 as i16 as i32 as u32,
                _ => *value as u32,
            },
            Word::Ref { name, offset } => self.at.iter().find(|b| &b.0 == name).unwrap_or_else(|| panic!("no buffer `{name}`")).1 + offset,
            Word::Unwritten { .. } => 0,
        }
    }

    /// A vector passed by value: three floats.
    pub fn f3(&self, name: &str) -> [f32; 3] {
        let f = self.c.input(&format!("v_{name}")).f32s();
        [f[0], f[1], f[2]]
    }

    /// A 16-bit argument by value.
    pub fn i(&self, name: &str) -> i16 {
        self.w(name) as i16
    }
}

/// The C library's mathematics, which `si_x87::trig` replays by itself.
const LIBM: &[&str] = &["sin", "cos", "tan", "asin", "acos", "atan", "atan2", "exp", "log", "log10", "pow", "cbrt", "sinf", "cosf", "tanf", "asinf", "acosf", "atanf", "expf", "powf", "fpatan", "sqrt"];

/// The outside world of a case: the recorded calls, in order.
pub struct Replay<'a> {
    c: &'a Case,
    names: &'a Names,
    at: usize,
    pub wrong: Option<String>,
    /// The port called a routine the original ran as its own code in this vector (the
    /// second pass's vectors ran `SI_findGlData` of the binary), or the original called a
    /// stand-in for a routine the port runs itself (they stood in `SI_modelComputeBounding`):
    /// the case cannot be compared call for call.
    pub foreign: bool,
}

/// Routines an image form calls through [`Outside`] that the earlier passes' vectors did
/// not replace by stand-ins.
const RAN_IN_ORIGINAL: &[&str] = &["SI_findGlData"];

/// Routines the earlier passes' vectors replaced by stand-ins that the image forms of the
/// sixth pass run as ports.
const STOOD_IN: &[&str] = &["SI_modelComputeBounding"];

impl<'a> Replay<'a> {
    fn new(c: &'a Case, names: &'a Names) -> Replay<'a> {
        Replay { c, names, at: 0, wrong: None, foreign: false }
    }

    fn skip_libm(&mut self) {
        while self.at < self.c.calls.len() && LIBM.contains(&self.c.calls[self.at].name.as_str()) {
            self.at += 1;
        }
    }

    fn fail(&mut self, text: String) {
        if self.wrong.is_none() {
            self.wrong = Some(text);
        }
    }

    /// Every recorded call was made.
    fn finished(&mut self) -> bool {
        self.skip_libm();
        self.at == self.c.calls.len()
    }
}

/// Stand-ins of `emu_gen2.py` that log only some of the arguments (`emu_world.answering`
/// with `-` for the others): (name, the arguments logged).
const PARTLY: &[(&str, &[usize])] = &[("SI_modelCompensateAngle", &[1])];

/// Routines whose stand-in records what one argument points to: (name, argument, words).
const DEREF: &[(&str, usize, usize)] = &[("SI_modelTransformationMatrix", 1, 16), ("checkLow", 0, 3), ("si_metaballContourTraversePnts", 1, 16), ("SI_nurbsCrvDispose", 0, 1)];

/// The routines the generators hand in as callbacks, by address: a call the port makes
/// through a pointer is held against the recorded call of that name.
const CALLBACKS: &[(u32, &str)] = &[(0x081f_88a0, "checkLow")];

fn in_stack(a: u32) -> bool {
    (0x7fd0_0000..0x7ff0_0000).contains(&a)
}

impl Outside for Replay<'_> {
    fn call(&mut self, m: &mut Mem, callee: &Callee, args: &[u32]) -> u32 {
        self.skip_libm();
        if RAN_IN_ORIGINAL.contains(&callee.name) && self.c.calls.get(self.at).is_none_or(|r| r.name != callee.name) {
            self.foreign = true;
            return 0;
        }
        let Some(rec) = self.c.calls.get(self.at) else {
            self.fail(format!("call {}: the port calls {}{args:08x?}, the original nothing more", self.at, callee.name));
            return 0;
        };
        if STOOD_IN.contains(&rec.name.as_str()) && rec.name != callee.name {
            self.foreign = true;
            return 0;
        }
        self.at += 1;
        let callee = &match CALLBACKS.iter().find(|c| callee.name == mi::THROUGH_POINTER && c.0 == callee.addr) {
            Some(c) => Callee { name: c.1, addr: c.0 },
            None => *callee,
        };
        if rec.name != callee.name {
            self.fail(format!("call {}: the port calls {}, the original {}", self.at - 1, callee.name, rec.name));
            return 0;
        }
        // a routine whose argument points to a local of the caller: the stand-in recorded
        // what was there, after the arguments
        // (only where the record has those words: the fourth pass's vectors record
        // `SI_nurbsCrvDispose` without)
        let pointed = DEREF.iter().find(|d| d.0 == callee.name && (d.0 == "SI_modelTransformationMatrix" || rec.args.len() == args.len() + d.2));
        let extra = pointed.map_or(0, |d| d.2);
        if let Some(&(_, index, count)) = pointed {
            if rec.args.len() == args.len() + count {
                for i in 0..count {
                    let got = m.u32(args[index] + 4 * i as u32) ^ (mutate() && i == 0) as u32;
                    if rec.args[args.len() + i].value().map(|v| v as u32) != Some(got) {
                        self.fail(format!("call {} {}: word {i} of what argument {index} points to is {:?}, the port's {got:08x}", self.at - 1, callee.name, rec.args[args.len() + i]));
                    }
                }
            }
        }
        // a stand-in of the second pass that logs only some of its arguments
        let logged: Vec<(usize, u32)> = match PARTLY.iter().find(|p| p.0 == callee.name && rec.args.len() == p.1.len() && args.len() > p.1.len()) {
            Some(p) => p.1.iter().map(|&k| (k, args[k])).collect(),
            None => args.iter().copied().enumerate().collect(),
        };
        if rec.args.len() != logged.len() + extra {
            self.fail(format!("{}: {} arguments against the port's {}", callee.name, rec.args.len(), args.len()));
            return 0;
        }
        for (i, (want, (k, got))) in rec.args.iter().zip(logged).enumerate() {
            let got = got ^ (mutate() && i == 0) as u32;
            let same = match want {
                Word::Value { value, .. } => *value as u32 == got || (in_stack(*value as u32) && in_stack(got)),
                Word::Unwritten { .. } => true,
                Word::Ref { name, offset } => match self.names.addr(m, name) {
                    Some(a) => a.wrapping_add(*offset) == got,
                    // a string argument: the text the original passed
                    None => match self.c.try_output(name) {
                        Some(text) => text.to_bytes() == m.cstr(got),
                        None => false,
                    },
                },
            };
            if !same {
                let where_ = self.names.name_of(m, got).map_or(format!("{got:08x}"), |(n, o)| format!("&{n}+{o:#x}"));
                self.fail(format!("call {} {}: argument {k} is {want:?}, the port's {where_}", self.at - 1, callee.name));
            }
        }
        let result = match &rec.result {
            Word::Value { value, .. } => *value as u32,
            Word::Ref { name, offset } => self.names.addr(m, name).unwrap_or(0).wrapping_add(*offset),
            Word::Unwritten { .. } => 0,
        };
        effects(m, callee, args, result);
        result
    }
}

/// What a stand-in of `emu_gen4.py` does besides answering (each mirrors the Python
/// function of the same routine there).
fn effects(m: &mut Mem, callee: &Callee, args: &[u32], result: u32) {
    // the element look-ups leave the element's number at their second argument
    // (`wrote_id`): here the answer + 0x11
    if callee.name == "SI_edGetElemFromModel" || callee.name == "SI_edGetElemFromShape" {
        m.set_u32(args[1], result.wrapping_add(0x11));
    }
    // the curve and the point of a face's index (`local_index` of emu_gen5.py)
    if callee.name == "SI_faceGlobalToLocalIndex" {
        m.set_u32(args[2], result & 3);
        m.set_u32(args[3], (result >> 4) & 7);
    }
    // the point and the frame along the fitting's curve: twelve floats made from the
    // distance's bits and the answer (`wrote_frenet` of emu_gen7.py)
    if callee.name == "SI_curveDistanceToValueFrenet" {
        for i in 0..12u32 {
            let v = frenet_word(args[1], result, i);
            m.set_u32(if i < 3 { args[2] + 4 * i } else { args[4] + 4 * (i - 3) }, v);
        }
    }
    // the rotation of a mode: sixteen floats made from the answer (`wrote_rotation`)
    if callee.name == "SI_matrixRotationRefer" {
        for i in 0..16u32 {
            m.set_f32(args[2] + 4 * i, (((result + 7 * i) % 13) as f32 - 6.0) * 0.25);
        }
    }
}

/// Float `i` (0–2 the point, 3–11 the frame) the stand-in of
/// `SI_curveDistanceToValueFrenet` writes for distance bits `u` and answer `r`: a float
/// with a full 24-bit significand in [−2, 2); with answer 8, word `u mod 12` an infinity
/// or a quiet NaN instead (the twin of `frenet_word` in emu_gen7.py).
fn frenet_word(u: u32, r: u32, i: u32) -> u32 {
    if r == 8 && u % 12 == i {
        return if u & 0x100 != 0 { 0x7fc0_0000 } else { 0xff80_0000 };
    }
    let h = (u ^ r.wrapping_mul(0x9e37_79b9)).wrapping_add(i.wrapping_mul(0x7f4a_7c15)).wrapping_mul(0x9e37_79b1);
    let v = ((h >> 8) & 0xff_ffff) as f32 / 4_194_304.0 - 2.0;
    v.to_bits()
}

fn compare(m: &Mem, names: &Names, c: &Case, ret: &Ret) -> Verdict {
    for f in &c.outputs {
        let name = f.name.as_str();
        let int = |bits: u32| -> Verdict {
            let Ret::Int(v) = ret else {
                return Verdict::Different(format!("{name}: the port returns nothing"));
            };
            let got = (*v ^ mutate() as u32) & (u32::MAX >> (32 - bits));
            match &f.words()[0] {
                Word::Value { value, .. } => Verdict::from(*value as u32 == got, || format!("{name}: {value:x} against the port's {got:x}")),
                Word::Ref { name: b, offset } => {
                    let want = names.addr(m, b).map(|a| a + offset);
                    Verdict::from(want == Some(got), || format!("{name}: &{b}+{offset:#x} against the port's {got:08x}"))
                }
                Word::Unwritten { .. } => Verdict::Same,
            }
        };
        let v = match name {
            "eax" | "ptr" => int(32),
            "ax" => int(16),
            "al" => int(8),
            "st0" => match ret {
                Ret::St0(v) => {
                    let v = if mutate() { F80::from_bits(v.to_bits().0 ^ 1, v.to_bits().1) } else { *v };
                    Verdict::from(f.same_f80(v), || format!("st0: {:?} against the port's {v:?}", f.words()))
                }
                _ => Verdict::from(f.words().is_empty(), || "st0: the port returns nothing".to_string()),
            },
            // the text a dump wrote to its FILE (vectors of re/tools/si-model/gen_dump.py)
            "stdout" => {
                let mut got = STDOUT.with(|s| s.borrow().clone());
                if mutate() {
                    got.push(b'!');
                }
                let want = f.to_bytes();
                Verdict::from(got == want, || format!("stdout: {:?} against the port's {:?}", String::from_utf8_lossy(&want), String::from_utf8_lossy(&got)))
            }
            "nheap" => {
                let want = f.u32() as usize;
                Verdict::from(want == m.heap_blocks().len(), || format!("{want} heap blocks against the port's {}", m.heap_blocks().len()))
            }
            "freed" => {
                let got: Vec<u32> = m.freed().to_vec();
                let want: Vec<Option<u32>> = f
                    .words()
                    .iter()
                    .map(|w| match w {
                        Word::Ref { name, offset } => names.addr(m, name).map(|a| a + offset),
                        Word::Value { value, .. } => Some(*value as u32),
                        Word::Unwritten { .. } => None,
                    })
                    .collect();
                Verdict::from(!mutate() && want.len() == got.len() && want.iter().zip(&got).all(|(a, b)| *a == Some(*b)), || {
                    format!("freed: {:?} against the port's {got:08x?}", f.words())
                })
            }
            _ => match names.addr(m, name) {
                Some(0) | None if name.starts_with("heap") => Verdict::Different(format!("the port has no block {name}")),
                Some(0) | None => Verdict::Same,
                Some(a) => match m.compare_words(a, &convert(f).unwrap_or_default(), names) {
                    Ok(()) => Verdict::Same,
                    Err(e) => Verdict::Different(format!("{name}: {e}")),
                },
            },
        };
        if v != Verdict::Same {
            return v;
        }
    }
    if !c.outputs.iter().any(|f| f.name == "freed") && !m.freed().is_empty() {
        return Verdict::Different(format!("the port frees {} blocks, the original none", m.freed().len()));
    }
    Verdict::Same
}

/// A case is run twice: on fresh memory of zeros, and on fresh memory holding a pattern
/// (blocks from `malloc`, locals, the words of a buffer the vector gives as unwritten). A
/// word the original stored that the port left alone is found by the second run when its
/// value is zero, by the first otherwise. The mutation is applied in the first run only.
fn run(c: &Case) -> Verdict {
    match pass(c, false) {
        Verdict::Same if !mutate() => {
            // the C library's recorded answers are replayed once more for the second run
            let tape = c.tape();
            if !tape.is_empty() {
                if let Err(e) = si_x87::trig::replay::finish() {
                    return Verdict::Different(e);
                }
                si_x87::trig::replay::start(tape);
            }
            pass(c, true)
        }
        other => other,
    }
}

fn pass(c: &Case, pattern: bool) -> Verdict {
    let mut m = Mem::new();
    if pattern {
        m.set_fill(si_mem::Fill::Pattern { second: false, salt: 0x51 });
    }
    mi::init(&mut m);
    let owned: Vec<(&str, Option<Vec<W>>)> = c.inputs.iter().filter(|f| !by_value(f)).map(|f| (f.name.as_str(), convert(f))).collect();
    let fields: Vec<FieldAt<'_>> = owned.iter().map(|(n, w)| (*n, fixed(n), w.as_deref())).collect();
    let names = match si_mem::load_case_at(&mut m, &fields) {
        Ok(n) => n,
        Err(e) => return Verdict::Different(e),
    };
    let before = m.snapshot();
    let args = Args { at: names.iter().map(|(n, a, _)| (n.to_string(), a)).collect(), c };
    let mut o = Replay::new(c, &names);
    si_model::env::stale::reset();
    let base = c.name.split('#').next().unwrap();
    let Some(ret) = port(base, &mut m, &args, &mut o) else {
        return Verdict::Skip;
    };
    if si_model::env::stale::count() != 0 {
        // the original went on with what its stack held: not reproducible (finding 0015)
        return Verdict::Skip;
    }
    if o.foreign || o.c.calls.get(o.at).is_some_and(|r| STOOD_IN.contains(&r.name.as_str())) {
        return Verdict::Skip;
    }
    if let Some(f) = m.fault() {
        return Verdict::Different(format!("the port faulted: {f:?}"));
    }
    if let Some(w) = o.wrong {
        return Verdict::Different(w);
    }
    if !o.finished() {
        return Verdict::Different(format!("the original goes on to call {}", c.calls[o.at].name));
    }
    if mutate() {
        for (a, bytes) in m.changed(&before) {
            m.set_u8(a, bytes[0] ^ 1);
        }
    }
    compare(&m, &names, c, &ret)
}

/// The ports, by the routine's name: each takes its arguments from the case as the spec
/// line of `emu_gen4.py` names them.
fn port(name: &str, m: &mut Mem, a: &Args, o: &mut Replay) -> Option<Ret> {
    use mi::{bounds, defs, kine, points, setters, setup, state, touch, tree};
    Some(match name {
        "SI_modelGetData" => Ret::Int(points::get_data(m, a.n("node"), a.w("type") as i32, a.w("number") as i32)),
        "SI_elemGetVertex" => Ret::Int(points::elem_get_vertex(m, o, a.w("dict"), a.p("el"), a.w("chapter"), a.w("type") as i32, a.w("number") as i32, a.w("index") as i32, a.p("out"), a.p("tagged"))),
        "SI_elemSetVertex" => Ret::Int(points::elem_set_vertex(m, o, a.w("dict"), a.p("el"), a.w("chapter"), a.w("type") as i32, a.w("number") as i32, a.w("index") as i32, a.p("p"))),
        "SI_edGetCtrlVertex" => Ret::Int(points::ed_get_ctrl_vertex(m, o, a.w("dict"), a.p("el"), a.w("chapter"), a.w("type") as i32, a.w("number") as i32, a.w("index") as i32, a.p("cv"))),
        "SI_edSetCtrlVertex" => Ret::Int(points::ed_set_ctrl_vertex(m, o, a.w("dict"), a.p("el"), a.w("chapter"), a.w("type") as i32, a.w("number") as i32, a.w("index") as i32, a.p("cv"))),
        "SI_modelTraverseCVs" => Ret::Int(points::traverse_cvs(m, o, a.n("node"), a.w("callback"), a.p("data"), a.i("own")) as u16 as u32),
        "SI_modelTraversePnts" => Ret::Int(points::traverse_pnts(m, o, a.n("node"), a.w("callback"), a.p("data"), a.i("own")) as u16 as u32),
        "SI_modelGetPnt" => {
            points::get_pnt(m, o, a.n("node"), a.w("number") as i32, a.p("out"));
            Ret::None
        }
        "SI_elemConvertPos" => Ret::Int(points::elem_convert_pos(m, o, a.n("from"), a.n("to"), a.p("p"), a.p("out"))),
        "SI_elemConvertOri" => Ret::Int(points::elem_convert_ori(m, o, a.n("from"), a.n("to"), a.p("p"), a.p("out"))),
        "SI_modelComputeBoundingCP" => {
            bounds::compute_bounding_cp(m, a.n("node"), a.p("min"), a.p("max"));
            Ret::None
        }
        "SI_modelHierarchyBounding" => {
            bounds::hierarchy_bounding(m, a.n("root"), a.p("min"), a.p("max"));
            Ret::None
        }
        "SI_modelComputeBoundingEL" => Ret::Int(bounds::compute_bounding_el(m, a.n("node"), a.p("min"), a.p("max"), a.p("matrix")) as u32),
        "SI_modelComputeBounding" => {
            bounds::compute_bounding(m, a.n("node"));
            Ret::None
        }
        "SI_modelUpdateSonState" => {
            state::update_son_state(m, o, a.n("father"), a.n("node"), a.i("recurse"));
            Ret::None
        }
        "SI_modelUpdateCScalingInfo" => {
            state::update_cscaling_info(m, a.n("father"), a.n("node"), a.i("recurse"));
            Ret::None
        }
        "toggleEffRotConstraint" => {
            state::toggle_eff_rot_constraint(m, a.n("node"));
            Ret::None
        }
        "SI_modelSplineFitting" => Ret::Int(defs::spline_fitting(m, a.n("node"), a.p("set"))),
        "SI_modelPatchFitting" => Ret::Int(defs::patch_fitting(m, a.n("node"), a.p("set"))),
        "SI_modelLattice" => Ret::Int(defs::lattice(m, a.n("node"), a.p("set"))),
        "SI_modelChangeName" => {
            defs::change_name(m, a.n("node"), a.w("name"));
            Ret::None
        }
        "SI_modelUnselectedParent" => Ret::Int(defs::unselected_parent(m, o, a.p("graf"), a.p("out"))),
        "SI_modelAddIDefinition" => Ret::Int(defs::add_idefinition(m, a.n("node"), a.i("position"), a.w("kind") as i32, a.w("definition"))),
        "SI_modelAddILattice" => {
            defs::add_ilattice(m, a.n("node"), a.i("set"), a.i("position"), a.w("lattice"));
            Ret::None
        }
        "SI_isModelDeformed" => Ret::Int(defs::is_model_deformed(m, o, a.n("node"))),
        "SI_modelDeleteIDefinition" => Ret::Int(defs::delete_idefinition(m, o, a.n("node"), a.i("number"))),
        "SI_modelIsTransformable" => Ret::Int(defs::is_transformable(m, o, a.n("node"), a.w("transformation") as i32, a.w("select") as i32, a.i("check"))),
        "SI_modelTransfoModesGet" => Ret::Int(defs::transfo_modes_get(m, o, a.n("node"), a.w("transformation") as i32, a.p("ref"), a.p("hier"), a.p("manip"))),
        "SI_modelSubstituteIDefinition" => Ret::Int(defs::substitute_idefinition(m, o, a.n("node"), a.i("number"), a.w("definition"))),
        "SI_modelValidateShape" => Ret::Int(defs::validate_shape(m, o, a.n("node")) as u32),
        "SI_modelSubstituteGeometry" => Ret::Int(defs::substitute_geometry(m, o, a.n("a"), a.n("b"), a.i("recurse"))),
        "SI_modelSwapGeometry" => Ret::Int(defs::swap_geometry(m, o, a.n("node"), a.p("kind"), a.p("count"), a.p("array"), a.p("original"))),
        "SI_modelSetCurrentDefinition" => Ret::Int(defs::set_current_definition(m, o, a.n("node"), a.i("number"))),
        "SI_modelUpdateAll" => {
            defs::update_all(m, o, a.n("node"), a.i("settings"), a.i("readjust"));
            Ret::None
        }
        "SI_modelInverse" => {
            defs::inverse(m, o, a.n("node"));
            Ret::None
        }
        "SI_userDataCopyFromSetup" => {
            setup::user_data_copy_from_setup(m, a.w("source"), a.p("out"));
            Ret::None
        }
        "SI_subelUserDataCopyFromSetup" => {
            setup::subel_user_data_copy_from_setup(m, o, a.p("cluster"), a.p("chains"), a.p("outc"), a.p("outu"));
            Ret::None
        }
        "SI_vrtPolEdgUserDataCopyFromSetup" => {
            setup::vrt_pol_edg_user_data_copy_from_setup(m, o, a.n("from"), a.n("to"));
            Ret::None
        }
        "klusterCopyFromSetup" => {
            setup::kluster_copy_from_setup(m, o, a.n("from"), a.n("to"));
            Ret::None
        }
        "SI_modelTransfoAnimCopy" => {
            setup::transfo_anim_copy(m, o, a.n("from"), a.n("to"));
            Ret::None
        }
        "shapeCopyFromSetup" => {
            setup::shape_copy_from_setup(m, o, a.n("from"), a.n("to"));
            Ret::None
        }
        "SI_modelCopyFromSetup" => {
            setup::copy_from_setup(m, o, a.n("from"), a.n("to"));
            Ret::None
        }
        "SI_modelDump" => {
            let mut out = Vec::new();
            mi::dump::model_dump(m, &mut out, a.n("model"));
            STDOUT.with(|s| *s.borrow_mut() = out);
            Ret::None
        }
        "SI_modelDumpTextures" => {
            let mut out = Vec::new();
            mi::dump::model_dump_textures(m, &mut out, a.n("model"), a.p("title"));
            STDOUT.with(|s| *s.borrow_mut() = out);
            Ret::None
        }
        "SI_modelRoot" => Ret::Int(tree::root(m, a.n("node")).addr()),
        "SI_modelNextNode" => Ret::Int(tree::next_node(m, a.n("node")).addr()),
        "SI_modelNextSon" => Ret::Int(tree::next_son(m, a.n("top"), a.n("at")).addr()),
        "SI_modelCutFather" => {
            tree::cut_father(m, o, a.n("node"));
            Ret::None
        }
        "SI_modelCopyTransformation" => {
            state::copy_transformation(m, o, a.n("from"), a.n("to"));
            Ret::None
        }
        "SI_modelCompensateParentingMatrix" => {
            state::compensate_parenting_matrix(m, o, a.n("node"), a.n("father"));
            Ret::None
        }
        "SI_modelCompensateCuttingMatrix" => {
            state::compensate_cutting_matrix(m, a.n("node"), a.n("father"));
            Ret::None
        }
        "SI_modelCut" => Ret::Int(tree::cut(m, o, a.p("graf"), a.p("message"))),
        "SI_modelGetVertexPtr" => Ret::Int(points::get_vertex_ptr(m, a.p("def"), a.w("kind") as i32, a.w("index") as i32, a.p("tagged"))),
        "SI_elemGetNbVertices" => Ret::Int(points::elem_get_nb_vertices(m, o, a.p("el"), a.w("chapter"), a.w("number") as i32)),
        "SI_correctTransOfRefMatrix" => {
            state::correct_trans_of_ref_matrix(m, a.f3("p"), a.p("matrix"));
            Ret::None
        }
        "SI_matrixRotationReferTag" => {
            state::matrix_rotation_refer_tag(m, o, a.p("view"), a.p("angles"), a.p("frame"), a.p("out"));
            Ret::None
        }
        "SI_matrixTranslationReferTag" => Ret::Int(state::matrix_translation_refer_tag(m, a.p("out"), a.n("node"), a.f3("vector"), a.p("frame"), a.i("keep"))),
        // ---- the setters and the compensation (this pass)
        "SI_modelSetScaling" | "SI_modelSetRotation" | "SI_modelSetTranslation" | "SI_modelSetGlobalScaling" | "SI_modelSetGlobalRotation" | "SI_modelSetGlobalTranslation" => {
            let f = match name {
                "SI_modelSetScaling" => setters::set_scaling,
                "SI_modelSetRotation" => setters::set_rotation,
                "SI_modelSetTranslation" => setters::set_translation,
                "SI_modelSetGlobalScaling" => setters::set_global_scaling,
                "SI_modelSetGlobalRotation" => setters::set_global_rotation,
                _ => setters::set_global_translation,
            };
            f(m, o, a.n("node"), a.p("v"), a.w("mode") as i32, a.i("geom"));
            Ret::None
        }
        "SI_modelSetState" => {
            setters::set_state(m, o, a.n("node"), a.p("scal"), a.p("rot"), a.p("trans"));
            Ret::None
        }
        "SI_modelCompensateMatrix" => {
            setters::compensate_matrix(m, o, a.n("node"), a.w("mode") as i32, a.i("geom"));
            Ret::None
        }
        "computeInit_srtGlobalTransfo" => {
            setters::compute_init_srt_global_transfo(m, o, a.n("node"), a.n("first"));
            Ret::None
        }
        "compensateInit_srt" => {
            setters::compensate_init_srt(m, a.n("node"), a.n("first"));
            Ret::None
        }
        "SI_modelGlobalState" => {
            // `#i-shared`: one block for the scaling and the angles (the spec line's `r==s`)
            let r = if a.c.name.ends_with("-shared") { a.p("s") } else { a.p("r") };
            setters::global_state(m, a.n("node"), a.p("s"), r, a.p("t"));
            Ret::None
        }
        "computeInit_srtParenting" => {
            setters::compute_init_srt_parenting(m, a.n("first"));
            Ret::None
        }
        "SI_modelTouchDefinition" => {
            touch::touch_definition(m, o, a.n("node"));
            Ret::None
        }
        "SI_modelTouchGeometry" => {
            touch::touch_geometry(m, o, a.n("node"));
            Ret::None
        }
        "SI_modelMultiplyDefinition" => {
            touch::multiply_definition(m, o, a.n("node"), a.p("m"));
            Ret::None
        }
        // (the second pass's vectors name the cursor `cursor`)
        "SI_modelNextDefGet" => {
            let cursor = if a.c.try_input("cursor").is_some() { a.p("cursor") } else { a.p("cur") };
            Ret::Int(touch::next_def_get(m, a.n("node"), cursor))
        }
        "SI_modelDraw" => {
            touch::draw(m, o, a.n("node"), a.i("brothers"));
            Ret::None
        }
        "SI_splineFit" => {
            mi::fit::spline_fit(m, o, a.p("fit"), a.w("kind") as i32, a.p("src"), a.p("dst"));
            Ret::None
        }
        "SI_modelKineStateCompute" => {
            kine::kine_state_compute(m, a.n("node"), f32::from_bits(a.w("dt")));
            Ret::None
        }
        "SI_modelRotationRefMatrixSet" => {
            state::rotation_ref_matrix_set(m, o, a.p("view"), a.n("node"), a.p("angles"), a.w("select"), a.i("flag"));
            Ret::None
        }
        _ => return None,
    })
}

/// Routines that exist in both forms: with `MODEL_IMAGE` set, the earlier passes' vectors
/// go through the image form.
const BOTH: &[&str] = &["SI_modelUpdateSonState", "SI_modelRoot", "SI_modelNextNode", "SI_modelNextSon"];

/// The routines of the sixth pass that exist in both forms: with `MODEL_IMAGE=2` (and only
/// then, so that `MODEL_IMAGE=1` keeps the fourth pass's numbers), the second pass's vectors
/// of these go through the image form too.
const BOTH6: &[&str] = &[
    "SI_modelSetScaling",
    "SI_modelSetRotation",
    "SI_modelSetTranslation",
    "SI_modelSetGlobalScaling",
    "SI_modelSetGlobalRotation",
    "SI_modelSetGlobalTranslation",
    "SI_modelSetState",
    "SI_modelCompensateMatrix",
    "computeInit_srtGlobalTransfo",
    "compensateInit_srt",
    "SI_modelNextDefGet",
];

pub fn lookup(name: &str) -> Option<Check> {
    let (base, variant) = name.split_once('#').unwrap_or((name, ""));
    let image_vector = variant.starts_with("i-") || variant == "i";
    let switched = (std::env::var_os("MODEL_IMAGE").is_some() && BOTH.contains(&base)) || (std::env::var("MODEL_IMAGE").as_deref() == Ok("2") && BOTH6.contains(&base));
    (image_vector || switched).then_some(run as Check)
}
