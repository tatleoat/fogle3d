// SPDX-License-Identifier: MIT
//! Reading a made-up spline out of a vector case and comparing what the port leaves with
//! what the original left (the field layout is described in `re/tools/si-spline/emu_gen.py`).

use std::cell::RefCell;

use si_spline::{Curve, Host, Spline, SplineKey, Vector};
use si_types::{Ptr32, Zero};
use si_x87::vectors::{Case, Field, Verdict, Word};
use si_x87::F80;

/// `EMUCHECK_MUTATE=1`: one bit of the first value the port produces for each comparison is
/// flipped, so that every compared case must differ. Shows the comparisons are live.
/// `EMUCHECK_MUTATE=calls` (the image checks only) flips a bit of the first argument of every
/// call the port makes and nothing else: the port must be shown to make the same calls.
pub fn mutate() -> bool {
    matches!(mode(), Mode::Memory)
}

/// The first argument of every call the port makes is perturbed (`1` and `calls`).
pub fn mutate_calls() -> bool {
    !matches!(mode(), Mode::No)
}

enum Mode {
    No,
    Memory,
    Calls,
}

fn mode() -> Mode {
    match std::env::var("EMUCHECK_MUTATE").ok().as_deref() {
        None | Some("") | Some("0") => Mode::No,
        Some("calls") => Mode::Calls,
        Some(_) => Mode::Memory,
    }
}

/// A word of a field as a number: a pointer into a buffer of the case reads as `PTR`.
pub const PTR: u32 = 0xffff_fff1;

pub fn nums(f: &Field) -> Vec<Option<u32>> {
    f.words()
        .iter()
        .map(|w| match w {
            Word::Value { value, .. } => Some(*value as u32),
            Word::Unwritten { .. } => None,
            Word::Ref { .. } => Some(PTR),
        })
        .collect()
}

fn ptr<T>(w: Option<u32>) -> Ptr32<T> {
    match w {
        Some(0) | None => Ptr32::NULL,
        Some(_) => Ptr32::new(1),
    }
}

fn vec3(w: &[Option<u32>]) -> Vector {
    let f = |i: usize| f32::from_bits(w[i].unwrap_or(0));
    Vector { x: f(0), y: f(1), z: f(2) }
}

pub fn key_from(w: &[Option<u32>]) -> SplineKey {
    let mut k = SplineKey::zero();
    k.prev = vec3(&w[0..3]);
    k.pos = vec3(&w[3..6]);
    k.next = vec3(&w[6..9]);
    k.unk_24 = w[9].unwrap_or(0).to_le_bytes();
    k.constraint = w[10].unwrap_or(0) as i16;
    k.segment = w[11].unwrap_or(0) as i16;
    k.flag = w[12].unwrap_or(0) as i16;
    k.pad_2e = (w[13].unwrap_or(0) as u16).to_le_bytes();
    k
}

pub fn key_words(k: &SplineKey) -> [u32; 14] {
    [
        k.prev.x.to_bits(), k.prev.y.to_bits(), k.prev.z.to_bits(), k.pos.x.to_bits(), k.pos.y.to_bits(), k.pos.z.to_bits(),
        k.next.x.to_bits(), k.next.y.to_bits(), k.next.z.to_bits(), u32::from_le_bytes(k.unk_24), k.constraint as u16 as u32,
        k.segment as u16 as u32, k.flag as u16 as u32, u16::from_le_bytes(k.pad_2e) as u32,
    ]
}

pub fn rec_from(w: &[Option<u32>]) -> Spline {
    let v = |i: usize| w[i].unwrap_or(0);
    let mut r = Spline::zero();
    r.name = ptr(w[0]);
    r.unk_04 = v(1) as i16;
    r.phantom = v(2) as i16;
    r.closed = v(3) as i16;
    r.pad_0a = (v(4) as u16).to_le_bytes();
    r.kind = v(5) as i32;
    r.nb_keys = v(6) as i16;
    r.pad_12 = (v(7) as u16).to_le_bytes();
    r.keys = ptr(w[8]);
    r.unk_18 = v(9) as i32;
    r.tension = f32::from_bits(v(10));
    r.step = v(11) as i16;
    r.pad_22 = (v(12) as u16).to_le_bytes();
    for i in 0..16 {
        r.matrix[i / 4][i % 4] = f32::from_bits(v(13 + i));
    }
    r.unk_64 = v(29) as i16;
    r.unk_66 = v(30) as i16;
    r.nb_lengths = v(31) as u16;
    r.pad_6a = (v(32) as u16).to_le_bytes();
    r.lengths = ptr(w[33]);
    r.unk_70 = v(34) as i16;
    r.pad_72 = (v(35) as u16).to_le_bytes();
    r
}

pub fn rec_words(r: &Spline) -> Vec<u32> {
    let p = |null: bool| if null { 0 } else { PTR };
    let mut w = vec![
        p(r.name.is_null()), r.unk_04 as u16 as u32, r.phantom as u16 as u32, r.closed as u16 as u32, u16::from_le_bytes(r.pad_0a) as u32,
        r.kind as u32, r.nb_keys as u16 as u32, u16::from_le_bytes(r.pad_12) as u32, p(r.keys.is_null()), r.unk_18 as u32,
        r.tension.to_bits(), r.step as u16 as u32, u16::from_le_bytes(r.pad_22) as u32,
    ];
    w.extend(r.matrix.iter().flatten().map(|f| f.to_bits()));
    w.extend([
        r.unk_64 as u16 as u32, r.unk_66 as u16 as u32, r.nb_lengths as u32, u16::from_le_bytes(r.pad_6a) as u32, p(r.lengths.is_null()),
        r.unk_70 as u16 as u32, u16::from_le_bytes(r.pad_72) as u32,
    ]);
    w
}

/// The spline of a case: the buffers `spl<tag>`, `keys<tag>`, `len<tag>`.
pub fn curve(c: &Case, tag: &str) -> Curve {
    let rec = rec_from(&nums(c.input(&format!("spl{tag}"))));
    let keys = match c.try_input(&format!("keys{tag}")) {
        Some(f) => nums(f).chunks(14).map(key_from).collect(),
        None => Vec::new(),
    };
    let lengths = match c.try_input(&format!("len{tag}")) {
        Some(f) => nums(f).iter().map(|w| f32::from_bits(w.unwrap_or(0))).collect(),
        None => Vec::new(),
    };
    Curve { rec, name: None, keys, lengths }
}

/// Compares the port's words with a field: an unwritten word of the original is not
/// compared; a pointer is compared for being null or not.
pub fn same(name: &str, f: &Field, got: &[u32]) -> Verdict {
    let want = nums(f);
    if want.len() != got.len() {
        return Verdict::Different(format!("{name}: the original has {} words, the port {}", want.len(), got.len()));
    }
    let mut flip = mutate();
    for (i, (w, g)) in want.iter().zip(got).enumerate() {
        let Some(w) = w else { continue };
        let mut g = *g;
        if flip {
            g ^= 1;
            flip = false;
        }
        if *w != g {
            return Verdict::Different(format!("{name}[{i}]: original {w:08x}, port {g:08x}"));
        }
    }
    Verdict::Same
}

pub fn same_out(c: &Case, name: &str, got: &[u32]) -> Verdict {
    match c.try_output(name) {
        Some(f) => same(name, f, got),
        None => Verdict::Different(format!("the case has no output `{name}`")),
    }
}

pub fn f32w(v: &[f32]) -> Vec<u32> {
    v.iter().map(|f| f.to_bits()).collect()
}

pub fn vw(v: Vector) -> Vec<u32> {
    vec![v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
}

/// The spline after the call against the outputs `spl<tag>` (the record), `<o>.keys` (the
/// keys in use) and `<o>.len` (the length table).
pub fn same_curve(c: &Case, tag: &str, o: &str, got: &Curve) -> Verdict {
    let rec = match c.try_output(&format!("spl{tag}")) {
        Some(f) => same(&format!("spl{tag}"), f, &rec_words(&got.rec)),
        None => Verdict::Same, // an input-only record the original did not change
    };
    rec.and(|| same_body(c, o, got))
}

pub fn same_body(c: &Case, o: &str, got: &Curve) -> Verdict {
    let keys: Vec<u32> = got.used().iter().flat_map(key_words).collect();
    let keys = if got.rec.keys.is_null() { Vec::new() } else { keys };
    let table = if got.rec.lengths.is_null() { Vec::new() } else { f32w(&got.lengths) };
    same_out(c, &format!("{o}.keys"), &keys).and(|| {
        let f = c.output(&format!("{o}.len"));
        // the dump reads samples × segments + 1 entries whatever the block's size
        if f.len() != table.len() && got.rec.lengths.is_null() {
            return Verdict::Different(format!("{o}.len: the original has a table, the port none"));
        }
        same(&format!("{o}.len"), f, &table)
    })
}

pub fn same_eax(c: &Case, got: u32) -> Verdict {
    same_out(c, "eax", &[got])
}

pub fn same_ax(c: &Case, got: u16) -> Verdict {
    let w = c.output("ax").values()[0] as u16;
    let got = if mutate() { got ^ 1 } else { got };
    Verdict::from(w == got, || format!("ax: original {w:04x}, port {got:04x}"))
}

pub fn same_st0(c: &Case, got: F80) -> Verdict {
    let f = c.output("st0");
    if f.is_empty() {
        return Verdict::Different("the original left nothing in st0".to_string());
    }
    let got = if mutate() { F80::from_bits(got.to_bits().0 ^ 1, got.to_bits().1) } else { got };
    Verdict::from(f.same_f80(got), || format!("st0: original {:?}, port {:?}", f.f80(), got))
}

thread_local! {
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// A host that writes down every call the way the harness records the original's.
pub struct Rec {
    pub index: Option<Vec<i32>>,
    pub index2: Option<Vec<i32>>,
}

impl Rec {
    pub fn new() -> Rec {
        LOG.with(|l| l.borrow_mut().clear());
        Rec { index: None, index2: None }
    }
}

fn log(text: String) {
    LOG.with(|l| l.borrow_mut().push(text));
}

/// The handle the recorded `SI_findPathFcurveOwner` answers with.
pub const OWNER: u32 = 0x5150;

impl Host for Rec {
    fn has_index(&self) -> bool {
        self.index.is_some()
    }
    fn set_index(&mut self, at: i32, value: i32) {
        if let Some(t) = self.index.as_mut() {
            if let Some(e) = t.get_mut(at as usize) {
                *e = value;
            }
        }
    }
    fn has_index2(&self) -> bool {
        self.index2.is_some()
    }
    fn index2(&self, at: i32) -> i32 {
        self.index2.as_ref().and_then(|t| t.get(at as usize).copied()).unwrap_or(0)
    }
    fn set_index2(&mut self, at: i32, value: i32) {
        if let Some(e) = self.index2.as_mut().and_then(|t| t.get_mut(at as usize)) {
            *e = value;
        }
    }
    fn find_path_fcurve_owner(&mut self, _c: &Curve) -> u32 {
        log("SI_findPathFcurveOwner(&spl)".to_string());
        OWNER
    }
    fn update_fcurve_path_keys(&mut self, f: u32, _c: &mut Curve) {
        log(format!("SI_updateFcurvePathKeys({f:08x},&spl)"));
    }
    fn check_spline_to_fcurve(&mut self, f: u32, _c: &mut Curve) {
        log(format!("SI_checkSplineToFcurve({f:08x},&spl)"));
    }
    fn refresh_fcurve(&mut self, a: i32) {
        log(format!("SI_refreshFcurve({a:08x})"));
    }
    fn add_fcurve_path_key(&mut self, f: u32, p: i32) {
        log(format!("SI_addFcurvePathKey({f:08x},{p:08x})"));
    }
    fn delete_fcurve_path_key(&mut self, f: u32, p: i16) {
        log(format!("SI_deleteFcurvePathKey({f:08x},{:08x})", p as i32));
    }
    fn invert_fcurve_path_keys(&mut self, f: u32, _c: &mut Curve) {
        log(format!("SI_invertFcurvePathKeys({f:08x},&spl)"));
    }
}

/// The recorded calls of the case against the calls the port made through [`Rec`]. A
/// pointer argument is compared by the buffer it points into, without its offset; a second
/// spline's buffers are named `spl2`, which the port's log does not tell apart from `spl`.
pub fn same_calls(c: &Case) -> Verdict {
    let want: Vec<String> = c
        .calls
        .iter()
        .filter(|k| k.name.starts_with("SI_"))
        .map(|k| {
            let args: Vec<String> = k
                .args
                .iter()
                .map(|w| match w {
                    Word::Ref { name, .. } => format!("&{}", name.trim_end_matches(char::is_numeric)),
                    Word::Value { value, .. } => format!("{:08x}", *value as u32),
                    Word::Unwritten { .. } => "--".to_string(),
                })
                .collect();
            format!("{}({})", k.name, args.join(","))
        })
        .collect();
    let got = LOG.with(|l| l.borrow().clone());
    let got = if mutate() && !got.is_empty() { got[1..].to_vec() } else { got };
    Verdict::from(want == got, || format!("calls: original {want:?}, port {got:?}"))
}

/// The record of a spline the original allocated: the dump leaves out the five padding
/// words, which the allocator does not set.
pub fn same_new_rec(c: &Case, name: &str, got: &Spline) -> Verdict {
    let w: Vec<u32> = rec_words(got).into_iter().enumerate().filter(|(i, _)| ![4, 7, 12, 32, 35].contains(i)).map(|(_, w)| w).collect();
    same_out(c, name, &w)
}

pub fn same_keys(c: &Case, name: &str, got: &Curve) -> Verdict {
    let keys: Vec<u32> = got.used().iter().flat_map(key_words).collect();
    same_out(c, name, &if got.rec.keys.is_null() { Vec::new() } else { keys })
}

/// A host with the index table of the case, when it has one.
pub fn host(c: &Case) -> Rec {
    let mut h = Rec::new();
    h.index = c.try_input("idx").map(|f| f.i32s());
    h.index2 = c.try_input("idx2").map(|f| f.i32s());
    h
}

pub fn same_index2(c: &Case, h: &Rec) -> Verdict {
    match (&h.index2, c.try_output("idx2")) {
        (Some(t), Some(f)) => same("idx2", f, &t.iter().map(|v| *v as u32).collect::<Vec<_>>()),
        (None, None) => Verdict::Same,
        _ => Verdict::Different("second index table present on one side only".to_string()),
    }
}

pub fn same_index(c: &Case, h: &Rec) -> Verdict {
    match (&h.index, c.try_output("idx")) {
        (Some(t), Some(f)) => same("idx", f, &t.iter().map(|v| *v as u32).collect::<Vec<_>>()),
        (None, None) => Verdict::Same,
        _ => Verdict::Different("index table present on one side only".to_string()),
    }
}

/// The offset of the pointer a routine returned, with the buffer it points into.
pub fn ptr_out(c: &Case) -> Option<(String, u32)> {
    match c.output("ptr").words().first() {
        Some(Word::Ref { name, offset }) => Some((name.clone(), *offset)),
        _ => None,
    }
}

/// A spline the routine returned (`ptr`, with the dump `n.rec`, `n.keys`, `n.name`) against
/// the port's; `None` is the original's null pointer.
pub fn same_new(c: &Case, got: &Option<Curve>) -> Verdict {
    let null = matches!(c.output("ptr").words().first(), Some(Word::Value { value: 0, .. }));
    match got {
        None => Verdict::from(null && !mutate(), || "the original returned a spline, the port none".to_string()),
        Some(_) if null => Verdict::Different("the original returned null, the port a spline".to_string()),
        Some(n) => {
            let name = c.output("n.name").to_bytes();
            Verdict::from(Some(&name) == n.name.as_ref(), || format!("name: original {name:?}, port {:?}", n.name))
                .and(|| same_new_rec(c, "n.rec", &n.rec))
                .and(|| same_keys(c, "n.keys", n))
        }
    }
}
