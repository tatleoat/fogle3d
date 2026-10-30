// SPDX-License-Identifier: MIT
//! Reading a made-up patch out of a vector case and comparing what the port leaves with
//! what the original left (the field layout is `PAT` in `re/tools/si-spline/emu_gen.py`).

use si_spline::patch::Surface;
use si_spline::Vector;
use si_types::geom::Patch;
use si_types::{Ptr32, Zero};
use si_x87::vectors::{Case, Verdict};

use crate::util::{mutate, nums, same, same_out, PTR};

/// The padding words of the record, which no routine sets.
pub const PADS: [usize; 6] = [112, 120, 132, 135, 142, 144];

fn ptr<T>(w: Option<u32>) -> Ptr32<T> {
    match w {
        Some(0) | None => Ptr32::NULL,
        Some(_) => Ptr32::new(1),
    }
}

pub fn pat_from(w: &[Option<u32>]) -> Patch {
    let v = |i: usize| w[i].unwrap_or(0);
    let f = |i: usize| f32::from_bits(v(i));
    let mut r = Patch::zero();
    r.unk_000 = v(0) as i32;
    r.u_type = v(1) as i32;
    r.v_type = v(2) as i32;
    r.nb_u = v(3) as u16;
    r.nb_v = v(4) as u16;
    r.u_closed = v(5) as i16;
    r.v_closed = v(6) as i16;
    r.u_tension = f(7);
    r.v_tension = f(8);
    r.u_step = v(9) as u16;
    r.v_step = v(10) as u16;
    r.u_nb_curves = v(11) as u16;
    r.v_nb_curves = v(12) as u16;
    for k in 0..2 {
        for i in 0..16 {
            r.basis[k][i / 4][i % 4] = f(13 + 16 * k + i);
        }
    }
    for k in 0..4 {
        for i in 0..16 {
            r.matrices[k][i / 4][i % 4] = f(45 + 16 * k + i);
        }
    }
    r.points = ptr(w[109]);
    r.tags = ptr(w[110]);
    r.unk_1ac = v(111) as i16;
    r.pad_1ae = (v(112) as u16).to_le_bytes();
    r.surface0 = ptr(w[113]);
    r.surface0_dims = [v(114) as u16, v(115) as u16];
    r.surface1 = ptr(w[116]);
    r.surface1_dims = [v(117) as u16, v(118) as u16];
    r.unk_1c0 = v(119) as i16;
    r.pad_1c2 = (v(120) as u16).to_le_bytes();
    r.subdiv0 = ptr(w[121]);
    r.subdiv1 = ptr(w[122]);
    r.u_bottom = v(123) as i16;
    r.u_top = v(124) as i16;
    r.v_bottom = v(125) as i16;
    r.v_top = v(126) as i16;
    r.unk_1d4 = v(127) as i16;
    r.unk_1d6 = v(128) as i16;
    r.modif0 = ptr(w[129]);
    r.modif1 = ptr(w[130]);
    r.unk_1e0 = v(131) as i16;
    r.pad_1e2 = (v(132) as u16).to_le_bytes();
    r.unk_1e4 = v(133) as i32;
    r.mr_view = v(134) as i16;
    r.pad_1ea = (v(135) as u16).to_le_bytes();
    r.mr_edge = f(136);
    r.mr_ucurv = f(137);
    r.mr_vcurv = f(138);
    r.mr_min = v(139) as i16;
    r.mr_max = v(140) as i16;
    r.mr_rec = v(141) as i16;
    r.pad_1fe = (v(142) as u16).to_le_bytes();
    r.mr_approx = v(143) as i16;
    r.pad_202 = (v(144) as u16).to_le_bytes();
    r
}

pub fn pat_words(r: &Patch) -> Vec<u32> {
    let p = |null: bool| if null { 0 } else { PTR };
    let s = |v: i16| v as u16 as u32;
    let pad = |b: [u8; 2]| u16::from_le_bytes(b) as u32;
    let mut w = vec![
        r.unk_000 as u32, r.u_type as u32, r.v_type as u32, r.nb_u as u32, r.nb_v as u32, s(r.u_closed), s(r.v_closed),
        r.u_tension.to_bits(), r.v_tension.to_bits(), r.u_step as u32, r.v_step as u32, r.u_nb_curves as u32, r.v_nb_curves as u32,
    ];
    w.extend(r.basis.iter().flatten().flatten().map(|f| f.to_bits()));
    w.extend(r.matrices.iter().flatten().flatten().map(|f| f.to_bits()));
    w.extend([
        p(r.points.is_null()), p(r.tags.is_null()), s(r.unk_1ac), pad(r.pad_1ae), p(r.surface0.is_null()), r.surface0_dims[0] as u32,
        r.surface0_dims[1] as u32, p(r.surface1.is_null()), r.surface1_dims[0] as u32, r.surface1_dims[1] as u32, s(r.unk_1c0),
        pad(r.pad_1c2), p(r.subdiv0.is_null()), p(r.subdiv1.is_null()), s(r.u_bottom), s(r.u_top), s(r.v_bottom), s(r.v_top),
        s(r.unk_1d4), s(r.unk_1d6), p(r.modif0.is_null()), p(r.modif1.is_null()), s(r.unk_1e0), pad(r.pad_1e2), r.unk_1e4 as u32,
        s(r.mr_view), pad(r.pad_1ea), r.mr_edge.to_bits(), r.mr_ucurv.to_bits(), r.mr_vcurv.to_bits(), s(r.mr_min), s(r.mr_max),
        s(r.mr_rec), pad(r.pad_1fe), s(r.mr_approx), pad(r.pad_202),
    ]);
    w
}

fn floats(c: &Case, name: &str) -> Vec<f32> {
    c.try_input(name).map(|f| nums(f).iter().map(|w| f32::from_bits(w.unwrap_or(0))).collect()).unwrap_or_default()
}

fn shorts(c: &Case, name: &str) -> Vec<i16> {
    c.try_input(name).map(|f| nums(f).iter().map(|w| w.unwrap_or(0) as i16).collect()).unwrap_or_default()
}

fn vectors(f: &[f32]) -> Vec<Vector> {
    f.chunks_exact(3).map(|p| Vector { x: p[0], y: p[1], z: p[2] }).collect()
}

/// The patch of a case: the record `pat<tag>` and the arrays `pts<tag>`, `tags<tag>`, …
pub fn surface(c: &Case, tag: &str) -> Surface {
    let n = |name: &str| format!("{name}{tag}");
    Surface {
        rec: pat_from(&nums(c.input(&n("pat")))),
        points: floats(c, &n("pts")).chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect(),
        tags: shorts(c, &n("tags")),
        surface0: vectors(&floats(c, &n("s0"))),
        modif0: shorts(c, &n("m0")),
        surface1: vectors(&floats(c, &n("s1"))),
        modif1: shorts(c, &n("m1")),
        subdiv0: vectors(&floats(c, &n("d0"))),
        subdiv1: vectors(&floats(c, &n("d1"))),
    }
}

fn vw(v: &[Vector]) -> Vec<u32> {
    v.iter().flat_map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]).collect()
}

/// A dumped array against the port's: the dump has as many words as the record's counts
/// say after the call; a word the original left unwritten is not compared; the port's
/// array must reach every word that has a value.
fn same_array(c: &Case, name: &str, null: bool, got: &[u32]) -> Verdict {
    let Some(f) = c.try_output(name) else {
        return Verdict::Different(format!("the case has no output `{name}`"));
    };
    let want = nums(f);
    if null {
        return Verdict::from(want.is_empty(), || format!("{name}: the original has an array, the port a null pointer"));
    }
    let mut flip = mutate();
    for (i, w) in want.iter().enumerate() {
        let Some(w) = w else { continue };
        let Some(g) = got.get(i) else {
            return Verdict::Different(format!("{name}[{i}]: original {w:08x}, the port's array ends at {}", got.len()));
        };
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

/// The arrays of a patch against the dump `<o>.pts`, `<o>.tags`, …
pub fn same_arrays(c: &Case, o: &str, got: &Surface) -> Verdict {
    let r = &got.rec;
    let pts: Vec<u32> = got.points.iter().flatten().map(|f| f.to_bits()).collect();
    let s16 = |v: &[i16]| v.iter().map(|t| *t as u16 as u32).collect::<Vec<u32>>();
    same_array(c, &format!("{o}.pts"), r.points.is_null(), &pts)
        .and(|| same_array(c, &format!("{o}.tags"), r.tags.is_null(), &s16(&got.tags)))
        .and(|| same_array(c, &format!("{o}.s0"), r.surface0.is_null(), &vw(&got.surface0)))
        .and(|| same_array(c, &format!("{o}.m0"), r.modif0.is_null(), &s16(&got.modif0)))
        .and(|| same_array(c, &format!("{o}.s1"), r.surface1.is_null(), &vw(&got.surface1)))
        .and(|| same_array(c, &format!("{o}.m1"), r.modif1.is_null(), &s16(&got.modif1)))
        .and(|| same_array(c, &format!("{o}.d0"), r.subdiv0.is_null(), &vw(&got.subdiv0)))
        .and(|| same_array(c, &format!("{o}.d1"), r.subdiv1.is_null(), &vw(&got.subdiv1)))
}

/// The record `pat<tag>` (when the routine changed it) and the arrays after the call.
pub fn same_surface(c: &Case, tag: &str, o: &str, got: &Surface) -> Verdict {
    let rec = match c.try_output(&format!("pat{tag}")) {
        Some(f) => same(&format!("pat{tag}"), f, &pat_words(&got.rec)),
        None => Verdict::Same,
    };
    rec.and(|| same_arrays(c, o, got))
}

/// The record of a patch the original allocated (`n.rec`: without the padding words).
pub fn same_new_pat(c: &Case, name: &str, got: &Patch) -> Verdict {
    let w: Vec<u32> = pat_words(got).into_iter().enumerate().filter(|(i, _)| !PADS.contains(i)).map(|(_, w)| w).collect();
    same_out(c, name, &w)
}

pub fn unwritten(c: &Case, name: &str) -> Verdict {
    Verdict::from(c.output(name).written().iter().all(|w| w.is_none()) && !mutate(), || format!("the original wrote `{name}`"))
}
