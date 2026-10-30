// SPDX-License-Identifier: MIT
//! Reading a made-up curve or surface out of a vector case and comparing what the port
//! leaves with what the original left (the field layout is described in
//! `re/tools/si-nurbs/emu_gen.py`).

use si_nurbs::own::{Curve, Surface};
use si_nurbs::{NurbsCrv, NurbsDir, NurbsPoint, NurbsSrf, Statics};
use si_types::{Double, Ptr32, Zero};
use si_x87::vectors::{Case, Field, Verdict, Word};

/// `EMUCHECK_MUTATE=1`: one bit of the first value the port produces for each comparison is
/// flipped, so that every compared case must differ. Shows the comparisons are live.
pub fn mutate() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some()
}

/// A word of a field as a number: a pointer into a buffer of the case reads as `PTR`, a
/// value no 32-bit word of the original can have.
pub const PTR: u64 = 0x1_0000_0000;

pub fn nums(f: &Field) -> Vec<Option<u64>> {
    f.words()
        .iter()
        .map(|w| match w {
            Word::Value { value, .. } => Some(*value as u64),
            Word::Unwritten { .. } => None,
            Word::Ref { .. } => Some(PTR),
        })
        .collect()
}

/// A pointer word of a case: null, a pointer into a buffer (any non-null marker), or a
/// number the generator put there as a stale pointer, which is carried as it is.
fn ptr<T: ?Sized>(w: Option<u64>) -> Ptr32<T> {
    match w {
        Some(0) | None => Ptr32::NULL,
        Some(PTR) => Ptr32::new(1),
        Some(v) => Ptr32::new(v as u32),
    }
}

fn pw<T: ?Sized>(p: Ptr32<T>) -> u64 {
    p.addr() as u64
}

fn d(w: Option<u64>) -> Double {
    Double(f64::from_bits(w.unwrap_or(0)))
}

pub fn dir_from(w: &[Option<u64>]) -> NurbsDir {
    let v = |i: usize| w[i].unwrap_or(0);
    let mut r = NurbsDir::zero();
    r.order = v(0) as i32;
    r.nb_points = v(1) as i32;
    r.nb_knots = v(2) as i32;
    r.knots = ptr(w[3]);
    r.range_start = d(w[4]);
    r.range_end = d(w[5]);
    r.closed = v(6) as i16;
    r.pad_22 = (v(7) as u16).to_le_bytes();
    r
}

pub fn dir_words(r: &NurbsDir) -> Vec<u64> {
    vec![
        r.order as u32 as u64, r.nb_points as u32 as u64, r.nb_knots as u32 as u64, pw(r.knots), r.range_start.0.to_bits(),
        r.range_end.0.to_bits(), r.closed as u16 as u64, u16::from_le_bytes(r.pad_22) as u64,
    ]
}

pub fn crv_rec_from(w: &[Option<u64>]) -> NurbsCrv {
    let v = |i: usize| w[i].unwrap_or(0);
    let mut r = NurbsCrv::zero();
    r.dir = dir_from(&w[0..8]);
    r.param = v(8) as i32;
    r.unk_28 = v(9) as i16;
    r.pad_2a = (v(10) as u16).to_le_bytes();
    r.points = ptr(w[11]);
    r.closing = v(12) as i32;
    r.step = v(13) as i32;
    r.flags_38 = v(14) as u32;
    r.unk_3c = [v(15) as u32, v(16) as u32];
    r.unk_44 = v(17) as i16;
    r.pad_46 = (v(18) as u16).to_le_bytes();
    r.unk_48 = v(19) as i32;
    r.keyframed = v(20) as i16;
    r.unk_4e = v(21) as i16;
    r.length_data = [v(22) as u32, pw(ptr::<u8>(w[23])) as u32, pw(ptr::<u8>(w[24])) as u32];
    r.unk_5c = [pw(ptr::<u8>(w[25])) as u32, v(26) as u32];
    let _ = PTR;
    r
}

pub fn crv_rec_words(r: &NurbsCrv) -> Vec<u64> {
    let p = |a: u32| a as u64;
    let mut w = dir_words(&r.dir);
    w.extend([
        r.param as u32 as u64, r.unk_28 as u16 as u64, u16::from_le_bytes(r.pad_2a) as u64, pw(r.points), r.closing as u32 as u64,
        r.step as u32 as u64, r.flags_38 as u64, r.unk_3c[0] as u64, r.unk_3c[1] as u64, r.unk_44 as u16 as u64,
        u16::from_le_bytes(r.pad_46) as u64, r.unk_48 as u32 as u64, r.keyframed as u16 as u64, r.unk_4e as u16 as u64,
        r.length_data[0] as u64, p(r.length_data[1]), p(r.length_data[2]), p(r.unk_5c[0]), r.unk_5c[1] as u64,
    ]);
    w
}

pub fn pts_from(w: &[Option<u64>]) -> Vec<NurbsPoint> {
    w.chunks(5)
        .map(|c| {
            let mut p = NurbsPoint::zero();
            p.x = d(c[0]);
            p.y = d(c[1]);
            p.z = d(c[2]);
            p.w = d(c[3]);
            p.flag = c[4].unwrap_or(0) as i32;
            p
        })
        .collect()
}

pub fn pt_words(p: &NurbsPoint) -> [u64; 5] {
    [p.x.0.to_bits(), p.y.0.to_bits(), p.z.0.to_bits(), p.w.0.to_bits(), p.flag as u32 as u64]
}

pub fn pts_words(p: &[NurbsPoint]) -> Vec<u64> {
    p.iter().flat_map(pt_words).collect()
}

pub fn f64w(v: &[f64]) -> Vec<u64> {
    v.iter().map(|f| f.to_bits()).collect()
}

fn f64s_of(c: &Case, name: &str) -> Option<Vec<f64>> {
    c.try_input(name).map(|f| nums(f).iter().map(|w| f64::from_bits(w.unwrap_or(0))).collect())
}

/// The knots of a bare direction: the buffers `dir`, `knots`.
pub fn dir_of(c: &Case) -> (NurbsDir, Vec<f64>) {
    (dir_from(&nums(c.input("dir"))), f64s_of(c, "knots").unwrap_or_default())
}

/// The curve of a case: the buffers `crv<tag>`, `knots<tag>`, `pts<tag>`, `lp<tag>`,
/// `ld<tag>`.
pub fn curve(c: &Case, tag: &str) -> Curve {
    let rec = crv_rec_from(&nums(c.input(&format!("crv{tag}"))));
    Curve {
        rec,
        knots: f64s_of(c, &format!("knots{tag}")),
        points: c.try_input(&format!("pts{tag}")).map(|f| pts_from(&nums(f))),
        length_param: f64s_of(c, &format!("lp{tag}")),
        length_dist: f64s_of(c, &format!("ld{tag}")),
    }
}

pub fn srf_rec_from(w: &[Option<u64>]) -> NurbsSrf {
    let v = |i: usize| w[i].unwrap_or(0);
    let mut r = NurbsSrf::zero();
    r.u = dir_from(&w[0..8]);
    r.v = dir_from(&w[8..16]);
    r.nb_points = v(16) as i32;
    r.param = v(17) as i32;
    r.unk_50 = v(18) as i16;
    r.pad_52 = (v(19) as u16).to_le_bytes();
    r.points = ptr(w[20]);
    r.unk_58 = v(21) as i32;
    r.border = v(22) as i16;
    r.pad_5e = (v(23) as u16).to_le_bytes();
    r.nb_trims = v(24) as i32;
    r.trims = ptr(w[25]);
    r.nb_projected = v(26) as i32;
    r.projected = ptr(w[27]);
    r.u_step = v(28) as i32;
    r.v_step = v(29) as i32;
    r.u_nb_curves = v(30) as i32;
    r.v_nb_curves = v(31) as i32;
    r.lines0 = Ptr32::new(v(32) as u32);
    r.lines1 = ptr(w[33]);
    r.u_bottom = v(34) as i16;
    r.u_top = v(35) as i16;
    r.v_bottom = v(36) as i16;
    r.v_top = v(37) as i16;
    r.unk_90 = [v(38) as i32, v(39) as i32];
    r.mr_approx = v(40) as i16;
    r.pad_9a = (v(41) as u16).to_le_bytes();
    r.mr_view = v(42) as i16;
    r.pad_9e = (v(43) as u16).to_le_bytes();
    r.mr_edge = f32::from_bits(v(44) as u32);
    r.mr_ucurv = f32::from_bits(v(45) as u32);
    r.mr_vcurv = f32::from_bits(v(46) as u32);
    r.mr_min = v(47) as i16;
    r.mr_max = v(48) as i16;
    r.mr_rec = v(49) as i16;
    r.pad_b2 = (v(50) as u16).to_le_bytes();
    r.unk_b4 = [v(51) as i16, v(52) as i16];
    r.unk_b8 = [d(w[53]), d(w[54])];
    // the tree pointer is carried as the case gave it: the port only moves it
    r.unk_c8 = [v(55) as u32, v(56) as u32, v(57) as u32, v(58) as u32, v(59) as u32];
    r
}

pub fn srf_rec_words(r: &NurbsSrf) -> Vec<u64> {
    let mut w = dir_words(&r.u);
    w.extend(dir_words(&r.v));
    w.extend([
        r.nb_points as u32 as u64, r.param as u32 as u64, r.unk_50 as u16 as u64, u16::from_le_bytes(r.pad_52) as u64, pw(r.points),
        r.unk_58 as u32 as u64, r.border as u16 as u64, u16::from_le_bytes(r.pad_5e) as u64, r.nb_trims as u32 as u64, pw(r.trims),
        r.nb_projected as u32 as u64, pw(r.projected), r.u_step as u32 as u64, r.v_step as u32 as u64, r.u_nb_curves as u32 as u64,
        r.v_nb_curves as u32 as u64, r.lines0.addr() as u64, pw(r.lines1), r.u_bottom as u16 as u64, r.u_top as u16 as u64,
        r.v_bottom as u16 as u64, r.v_top as u16 as u64, r.unk_90[0] as u32 as u64, r.unk_90[1] as u32 as u64, r.mr_approx as u16 as u64,
        u16::from_le_bytes(r.pad_9a) as u64, r.mr_view as u16 as u64, u16::from_le_bytes(r.pad_9e) as u64, r.mr_edge.to_bits() as u64,
        r.mr_ucurv.to_bits() as u64, r.mr_vcurv.to_bits() as u64, r.mr_min as u16 as u64, r.mr_max as u16 as u64, r.mr_rec as u16 as u64,
        u16::from_le_bytes(r.pad_b2) as u64, r.unk_b4[0] as u16 as u64, r.unk_b4[1] as u16 as u64, r.unk_b8[0].0.to_bits(),
        r.unk_b8[1].0.to_bits(), r.unk_c8[0] as u64, r.unk_c8[1] as u64, r.unk_c8[2] as u64, r.unk_c8[3] as u64,
        r.unk_c8[4] as u64,
    ]);
    w
}

/// The surface of a case: `srf<tag>`, `ku<tag>`, `kv<tag>`, `pts<tag>`, `lines<tag>`.
pub fn surface(c: &Case, tag: &str) -> Surface {
    surface_named(c, &format!("srf{tag}"), tag)
}

/// The same with the record in the buffer `name`.
pub fn surface_named(c: &Case, name: &str, tag: &str) -> Surface {
    let rec = srf_rec_from(&nums(c.input(name)));
    Surface {
        rec,
        knots_u: f64s_of(c, &format!("ku{tag}")),
        knots_v: f64s_of(c, &format!("kv{tag}")),
        points: c.try_input(&format!("pts{tag}")).map(|f| pts_from(&nums(f))),
        trims: None,
        projected: None,
        lines: c.try_input(&format!("lines{tag}")).map(|f| nums(f).chunks(5).map(|l| core::array::from_fn(|i| l[i].unwrap_or(0) as u32)).collect()),
    }
}

pub fn statics(c: &Case) -> Statics {
    Statics { left: c.try_input("left").map(|f| f.i32()).unwrap_or(0) }
}

/// Compares the port's words with a field: an unwritten word of the original is not
/// compared; a pointer is compared for being null or not.
pub fn same(name: &str, f: &Field, got: &[u64]) -> Verdict {
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
        // a pointer into a buffer or a heap block: the port must hold something there
        let ok = if *w == PTR { g != 0 } else { *w == g };
        if !ok {
            return Verdict::Different(format!("{name}[{i}]: original {w:x}, port {g:x}"));
        }
    }
    Verdict::Same
}

pub fn same_out(c: &Case, name: &str, got: &[u64]) -> Verdict {
    match c.try_output(name) {
        Some(f) => same(name, f, got),
        None => Verdict::Different(format!("the case has no output `{name}`")),
    }
}

pub fn same_eax(c: &Case, got: u32) -> Verdict {
    same_out(c, "eax", &[got as u64])
}

/// The static of the span search after the call.
pub fn same_left(c: &Case, st: &Statics) -> Verdict {
    match c.try_output("left") {
        Some(f) => same("left", f, &[st.left as u32 as u64]),
        None => Verdict::Same,
    }
}

/// A curve the original returned or left (`<o>.rec`, `<o>.knots`, `<o>.pts`, `<o>.lp`,
/// `<o>.ld`) against the port's.
pub fn same_curve(c: &Case, o: &str, got: &Curve) -> Verdict {
    same_out(c, &format!("{o}.rec"), &crv_rec_words(&got.rec))
        .and(|| same_body(c, o, got))
}

pub fn same_body(c: &Case, o: &str, got: &Curve) -> Verdict {
    let n = got.rec.dir.nb_points.max(0) as usize;
    let nk = got.rec.dir.nb_knots.max(0) as usize;
    let nl = got.nb_length().max(0) as usize;
    let part = |v: &Option<Vec<f64>>, n: usize| v.as_ref().map(|k| f64w(&k[..n.min(k.len())])).unwrap_or_default();
    let pts = got.points.as_ref().map(|p| pts_words(&p[..n.min(p.len())])).unwrap_or_default();
    same_out(c, &format!("{o}.knots"), &part(&got.knots, nk))
        .and(|| same_out(c, &format!("{o}.pts"), &pts))
        .and(|| match c.try_output(&format!("{o}.lp")) {
            Some(f) => same(&format!("{o}.lp"), f, &part(&got.length_param, nl)),
            None => Verdict::Same,
        })
        .and(|| match c.try_output(&format!("{o}.ld")) {
            Some(f) => same(&format!("{o}.ld"), f, &part(&got.length_dist, nl)),
            None => Verdict::Same,
        })
}

/// A surface after the call: `<o>.rec`, `<o>.ku`, `<o>.kv`, `<o>.pts`, `<o>.lines`.
pub fn same_surface(c: &Case, o: &str, got: &Surface) -> Verdict {
    let nb = got.rec.nb_points.max(0) as usize;
    let part = |v: &Option<Vec<f64>>, n: i32| v.as_ref().map(|k| f64w(&k[..(n.max(0) as usize).min(k.len())])).unwrap_or_default();
    let pts = got.points.as_ref().map(|p| pts_words(&p[..nb.min(p.len())])).unwrap_or_default();
    let lines: Vec<u64> = got.lines.as_ref().map(|l| l.iter().flatten().map(|w| *w as u64).collect()).unwrap_or_default();
    same_out(c, &format!("{o}.rec"), &srf_rec_words(&got.rec))
        .and(|| same_out(c, &format!("{o}.ku"), &part(&got.knots_u, got.rec.u.nb_knots)))
        .and(|| same_out(c, &format!("{o}.kv"), &part(&got.knots_v, got.rec.v.nb_knots)))
        .and(|| same_out(c, &format!("{o}.pts"), &pts))
        .and(|| same_out(c, &format!("{o}.lines"), &lines))
}

/// How many cases the port refused (all on inputs marked hostile or undefined).
pub static REFUSED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The port refused the input (`None`): the original reads or writes outside its arrays
/// there, divides by zero, or does not return. That is expected, and not compared, exactly
/// where the generator made the input hostile on purpose (the case's `bad` word) or the
/// harness found the original's result to depend on memory nobody wrote (`UNINIT`).
/// Anywhere else a refusal is a difference.
pub fn refused(c: &Case, what: &str) -> Verdict {
    let hostile = c.try_input("bad").map(|f| f.u32() != 0).unwrap_or(false);
    if hostile || c.has_note("UNINIT") {
        REFUSED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Verdict::Skip
    } else {
        Verdict::Different(format!("the port refuses ({what}) where the original returned"))
    }
}
