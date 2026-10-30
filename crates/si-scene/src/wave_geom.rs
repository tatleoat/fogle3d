// SPDX-License-Identifier: MIT
//! A wave moving the points of a mesh, a patch or a NURBS surface (`wave_update.c`):
//! `si_waveGeomUpdate`, which `SI_waveTransform` (`crate::wave_sys`) asks for by address
//! for those kinds of model (its callers route the address here; `crate::wave_update` has
//! the spline's twin).
//!
//! Read from the instruction listing (the order of the x87 operations, which values stay in
//! the register and which are rounded to a float on a store; the three jump tables at
//! 0x08661450, 0x08661478 and 0x086614a0 read from the binary). **Compared with the emulated
//! original** (`re/tools/si-scene/emu_gen_wave.py`, `scene-emucheck re/out/si-scene/vec_wave`):
//! made-up waves, graphs, icons, patches and NURBS surfaces; `si_waveComputeVectors`, the
//! helpers of `wave_util.c`, `SI_modelState`, `SI_edGetElemFromWave` and `SI_calloc` run as
//! they are; the element look-ups, the curves and their values, the point reads and writes,
//! the normals, the copies and their disposals are recorded stand-ins.

use crate::wave_mdl::{spline_index_to_value, wave_compute_vectors, wave_dot_product, wave_find_graph_value, wave_norm, SI_NURBS_SRF_COPY, SI_PATCH_COPY};
use crate::wave_update::{copy3, direction, WaveOutside, HALF, SI_ED_FIND_FCV, SI_FCURVE_VALUE, SI_FRAME_RATE};
use si_mem::{Mem, Ptr32};
use si_x87::{x, Round, F80};

/// `SI_edGetElemFromModel (0x080fcf20)`, `edtools.c`: (model, &index).
pub const SI_ED_GET_ELEM_FROM_MODEL: u32 = 0x080f_cf20;
/// `SI_elemGetNbVertices (0x084769b4)`: (element, chapter, number).
pub const SI_ELEM_GET_NB_VERTICES: u32 = 0x0847_69b4;
/// `SI_elemGetVertex (0x08476b2c)`: (dictionary, element, chapter, geometry type, number,
/// index, &point, &tagged).
pub const SI_ELEM_GET_VERTEX: u32 = 0x0847_6b2c;
/// `SI_elemSetVertex (0x08476d70)`: (dictionary, element, chapter, geometry type, number,
/// index, &point).
pub const SI_ELEM_SET_VERTEX: u32 = 0x0847_6d70;
/// `SI_computeMeshVertexNormal (0x08458960)`: (&result, mesh, vertex); the result, three
/// floats, through the hidden first argument.
pub const SI_COMPUTE_MESH_VERTEX_NORMAL: u32 = 0x0845_8960;
/// `SI_patchValueAndNormal (0x0847fe84)`: (patch, u, v, &point, &normal), u and v floats.
pub const SI_PATCH_VALUE_AND_NORMAL: u32 = 0x0847_fe84;
/// `SI_nurbsSrfValueAndNormal (0x084d6824)`: (&point, surface, u, v, &normal, 0, 0); u and
/// v doubles (two words each), the point (three doubles) through the hidden first argument,
/// the normal three doubles.
pub const SI_NURBS_SRF_VALUE_AND_NORMAL: u32 = 0x084d_6824;
/// `SI_patchDispose (0x0847a530)`, `SI_nurbsSrfDispose (0x084dba5c)`: (&definition).
pub const SI_PATCH_DISPOSE: u32 = 0x0847_a530;
pub const SI_NURBS_SRF_DISPOSE: u32 = 0x084d_ba5c;
/// The chapter word the point routines are given (0x08781680, the models' chapter of
/// `SI_cid`).
pub const CHAPTER: u32 = 0x0878_1680;

/// The frame, at the original's offsets below `ebp` (what callees write through a pointer
/// and what the original may read before writing it lives there).
struct Frame(u32);
const SIZE: u32 = 0x140;
impl Frame {
    fn at(&self, off: u32) -> u32 {
        self.0 + SIZE - off
    }
}
const POSITION: u32 = 0x0c;
const U: u32 = 0x18;
const V: u32 = 0x24;
const SCALING: u32 = 0x30;
const DELTA: u32 = 0x3c;
const DIR: u32 = 0x48;
const NORMAL: u32 = 0x54;
const PATCH_POINT: u32 = 0x60;
const SRF_POINT: u32 = 0x78;
const POINT: u32 = 0x84;
const INV: u32 = 0x88;
/// Where the mesh's normal is answered: under a wave of type 1, and under the other two.
const MESH_NORMAL_1: u32 = 0xa4;
const RESULT: u32 = 0xc0;
const INDEX: u32 = 0xc4;
const TAGGED: u32 = 0xc6;
const U_PARAM: u32 = 0xcc;
const V_PARAM: u32 = 0xd0;
const PATCH_COPY: u32 = 0xd4;
const SRF_COPY: u32 = 0xd8;
const KIND: u32 = 0xe0;
const PHASE: u32 = 0xe4;
const DIST: u32 = 0xe8;
const ALONG: u32 = 0xec;
const FREQ_RAW: u32 = 0xf0;
const AMP_RAW: u32 = 0xf4;
const FREQ: u32 = 0xf8;
const AMP: u32 = 0xfc;
const U_COUNT: u32 = 0x10e;
const V_COUNT: u32 = 0x110;
const MESH: u32 = 0x114;
const PATCH: u32 = 0x118;
const SRF: u32 = 0x11c;
const U_KNOT: u32 = 0x128;
const V_KNOT: u32 = 0x130;

/// The points of `model`'s working definition (+0x48) moved by `wave` at `time`
/// [soft 0x084c81d8 si_waveGeomUpdate]: `si_waveGeomUpdate(wave, model, time, check)`.
///
/// Both elements are looked up first (`SI_edGetElemFromWave`, `SI_edGetElemFromModel`, the
/// same index local); nothing without either, or without either curve of the wave's element
/// (`SI_edFindFcv(element, 0, 0x266)` and `0x267`). The frame and the cycling are the
/// spline update's (`(int)(time · SI_frameRate + 0.5)` truncated; the time a float local).
/// Then `si_waveComputeVectors(wave, P, U, V, S)` and the amplitude `A` = curve 0x266's
/// value (`SI_fcurveValue(curve, time, check)`): **exactly 0 in the register ends the
/// routine**; the frequency `B` = curve 0x267's, rounded.
///
/// By the model's kind (+0x30): a mesh (4) is moved in place; a patch (1) and a NURBS
/// surface (10) are copied first (`SI_patchCopy`, `SI_nurbsSrfCopy`; the definition itself
/// when that answers null): the normals are taken of the copy, the points moved in the
/// definition, and the copy disposed of at the end when it is not the definition (the kind
/// read again then).
///
/// For wave types (+8) 0 and 1 the frequency is `B / S.x` (rounded) unless S.x is 0; for 0,
/// 1 and 2 the amplitude is `A · S.y` (rounded). The travel `τ` = time − first frame /
/// `SI_frameRate` and `φ = (τ·0.5·accel + speed)·τ` rounded once. The number of points is
/// `SI_elemGetNbVertices(element, chapter, 0)`; a patch's two 16-bit counts (+0xc, +0xe)
/// are kept. The direction is U for mode (+0xc) 0, V for mode 1, else not set.
///
/// For each point `i` (`SI_elemGetVertex(dictionary, element, chapter, 2, 0, i, …)`):
/// `D` = point − P (floats); `d` = D·U in the register, the distance `|d|` rounded; at or
/// beyond the reach (+0x24) the point is left (not written). Else by type:
/// - 1: as the spline's (`e` = D·V, D −= d·U + e·V, `r` = |D| / S.z unless S.z is 0,
///   amplitude `A·graph(+0x4c, r)` rounded; **exactly 0 leaves the point**); with mode 2
///   the direction is the point's normal (below), with other modes it stays;
/// - 0: D −= d·U, the distance along = |D| rounded; mode 1: D's direction (1/|D| rounded,
///   times D) when |D| > 0.0001, else 0; mode 2: the normal's; other modes: as it is;
/// - 2: the distance along = |D| rounded; modes 0 and 1: D's direction as above; mode 2:
///   the normal's; negative and other modes: as it is;
/// - other: as it is (the amplitude and the distance along what the locals hold).
///
/// The normal, by the model's kind: a patch's at `(u, v)` = `si_splineIndexToValue` of
/// `i mod count_u` and `i div count_u` (its kinds at +4 and +8) on the copy
/// (`SI_patchValueAndNormal`); a mesh's `SI_computeMeshVertexNormal(mesh, (short) i)`; a
/// NURBS surface's at the knots `u[deg_u + i div n·… − 3]` (below) of the copy
/// (`SI_nurbsSrfValueAndNormal` into a 24-byte `SI_calloc` block, rounded to floats, the
/// block freed); kinds 6 and 9 leave the point; others keep the normal local as it is. The
/// direction is then the normal's: 1/|N| rounded times N when |N| (in the register) is
/// above 0.0001 (the double at 0x08661448), else 0.
///
/// The NURBS indices: with `r = i mod (+0x28)`, `q = (i − r) / (+4)`, `iu = (+0) + q − 3`
/// clamped to `[(+0) − 1, (+8) − (+0)]` when the 16-bit word +0x20 is 0, else to
/// `[0, (+8) − 1]` (the low bound tested first); `iv = (+0x24) + r − 3` likewise with +0x44,
/// +0x24 and +0x2c; the knots are the doubles at +0xc and +0x30. A divisor of 0 faults in
/// the original; here it gives 0.
///
/// The move: along = (along − φ) rounded, `h = amp · graph(+0x2c, (B · along) rounded)`;
/// when the reach's start (+0x20) is below the distance, `h` is scaled by `(reach −
/// distance) / (reach − start)`; the point += h·direction, each component rounded, and
/// written back (`SI_elemSetVertex(dictionary, element, chapter, 2, 0, i, &point)`).
///
/// Where the original reads a local it has not written (the direction, the normal, the
/// amplitude or the distance along in the cases named above) the port reads the same local
/// of its own frame on the image (not compared: the emulated original's stack holds other
/// bytes).
pub fn wave_geom_update<O: WaveOutside + ?Sized>(o: &mut O, m: &mut Mem, wave: u32, model: u32, time: f32, check: i16) {
    let f = Frame(m.local(SIZE));
    geom_update_in(o, m, &f, wave, model, time, check);
    m.release(SIZE);
}

/// What the type's part leaves: move the point, or leave it.
#[derive(PartialEq)]
enum Then {
    Move,
    Leave,
}

fn geom_update_in<O: WaveOutside + ?Sized>(o: &mut O, m: &mut Mem, f: &Frame, wave: u32, model: u32, time: f32, check: i16) {
    let w = |off: u32| wave.wrapping_add(off);
    for off in [MESH, PATCH, SRF] {
        m.set_u32(f.at(off), 0);
    }
    let elem = crate::edtools::ed_get_elem_from_wave(o, m, wave, Ptr32::new(f.at(INDEX)));
    let target = o.call_addr(m, SI_ED_GET_ELEM_FROM_MODEL, &[model, f.at(INDEX)]);
    if elem.is_null() || target == 0 {
        return;
    }
    let curve_a = o.call_addr(m, SI_ED_FIND_FCV, &[elem.0, 0, 0x266]);
    let curve_b = o.call_addr(m, SI_ED_FIND_FCV, &[elem.0, 0, 0x267]);
    if curve_a == 0 || curve_b == 0 {
        return;
    }
    let mut t = time;
    let rate = m.f32(SI_FRAME_RATE);
    let mut frame = (x(time) * rate + HALF).to_i32(Round::Zero);
    let first = m.i32(w(0x10));
    if frame < first {
        return;
    }
    if m.u32(w(0x28)) != 0 {
        let last = m.i32(w(0x14));
        let period = last.wrapping_sub(first);
        let span = F80::from_i32(period) / rate;
        if period <= 0 {
            return;
        }
        while frame > last {
            t = (x(t) - span).to_f32();
            frame = frame.wrapping_sub(period);
        }
    } else if frame > m.i32(w(0x14)) {
        return;
    }
    let (p, u, v, s) = (f.at(POSITION), f.at(U), f.at(V), f.at(SCALING));
    wave_compute_vectors(o, m, wave, p, u, v, s);
    let ck = check as i32 as u32;
    let a = o.call_f80(m, SI_FCURVE_VALUE, &[curve_a, time.to_bits(), ck]);
    m.set_f32(f.at(AMP_RAW), a.to_f32());
    // `fldz`, `fucompp`, `and 0x45`, `cmp 0x40`: exactly 0 (in the register) ends it
    if a.eq(F80::ZERO) {
        return;
    }
    let b = o.call_f80(m, SI_FCURVE_VALUE, &[curve_b, time.to_bits(), ck]).to_f32();
    m.set_f32(f.at(FREQ), b);
    m.set_f32(f.at(FREQ_RAW), b);
    let kind = m.i32(model.wrapping_add(0x30));
    m.set_i32(f.at(KIND), kind);
    let def = m.u32(model.wrapping_add(0x48));
    match kind {
        4 => m.set_u32(f.at(MESH), def),
        1 | 10 => {
            let (own, copy, routine) = if kind == 1 { (PATCH, PATCH_COPY, SI_PATCH_COPY) } else { (SRF, SRF_COPY, SI_NURBS_SRF_COPY) };
            m.set_u32(f.at(own), def);
            let c = o.call_addr(m, routine, &[def]);
            m.set_u32(f.at(copy), if c == 0 { def } else { c });
        }
        _ => {}
    }
    let wtype = m.i32(w(8));
    if wtype == 0 || wtype == 1 {
        let sx = m.f32(s);
        if !x(sx).eq(F80::ZERO) {
            m.set_f32(f.at(FREQ), (x(m.f32(f.at(FREQ_RAW))) / sx).to_f32());
        }
    }
    if (0..=2).contains(&wtype) {
        let amp = (x(m.f32(f.at(AMP_RAW))) * m.f32(s + 4)).to_f32();
        m.set_f32(f.at(AMP), amp);
        m.set_f32(f.at(AMP_RAW), amp);
    }
    let tau = x(t) - F80::from_i32(m.i32(w(0x10))) / m.f32(SI_FRAME_RATE);
    let phase = ((tau * HALF * m.f32(w(0x1c)) + m.f32(w(0x18))) * tau).to_f32();
    m.set_f32(f.at(PHASE), phase);
    let chapter = m.u32(CHAPTER);
    let count = o.call_addr(m, SI_ELEM_GET_NB_VERTICES, &[target, chapter, 0]) as i32;
    if m.i32(f.at(KIND)) == 1 {
        let patch = m.u32(f.at(PATCH));
        let (cu, cv) = (m.u16(patch.wrapping_add(0xc)), m.u16(patch.wrapping_add(0xe)));
        m.set_u16(f.at(U_COUNT), cu);
        m.set_u16(f.at(V_COUNT), cv);
    }
    match m.i32(w(0xc)) {
        0 => copy3(m, u, f.at(DIR)),
        1 => copy3(m, v, f.at(DIR)),
        _ => {}
    }
    let mut i = 0i32;
    while i < count {
        m.set_i32(f.at(INDEX), i);
        point(o, m, f, wave, target, i);
        i += 1;
    }
    let (own, copy, routine) = match m.i32(model.wrapping_add(0x30)) {
        1 => (PATCH, PATCH_COPY, SI_PATCH_DISPOSE),
        10 => (SRF, SRF_COPY, SI_NURBS_SRF_DISPOSE),
        _ => return,
    };
    if m.u32(f.at(copy)) != m.u32(f.at(own)) {
        o.call_addr(m, routine, &[f.at(copy)]);
    }
}

/// The dictionary and chapter words of the point routines, read at each call.
fn where_(m: &Mem) -> (u32, u32) {
    (crate::world::current_dict(m).0, m.u32(CHAPTER))
}

/// One point of the loop (0x084c8550 … 0x084c8e69).
fn point<O: WaveOutside + ?Sized>(o: &mut O, m: &mut Mem, f: &Frame, wave: u32, target: u32, i: i32) {
    let w = |off: u32| wave.wrapping_add(off);
    let (dict, chapter) = where_(m);
    let pt = f.at(POINT);
    o.call_addr(m, SI_ELEM_GET_VERTEX, &[dict, target, chapter, 2, 0, i as u32, pt, f.at(TAGGED)]);
    let (p, u, v, s, d) = (f.at(POSITION), f.at(U), f.at(V), f.at(SCALING), f.at(DELTA));
    let fl = |m: &Mem, a: u32| m.f32(a);
    for c in 0..3u32 {
        let val = (x(fl(m, pt + 4 * c)) - fl(m, p + 4 * c)).to_f32();
        m.set_f32(d + 4 * c, val);
    }
    let dot = wave_dot_product(m, d, u);
    let dist = if dot.lt(F80::ZERO) { (-dot).to_f32() } else { dot.to_f32() };
    m.set_f32(f.at(DIST), dist);
    let reach = m.f32(w(0x24));
    // `fcomp`, `and 5`, `je`: at or beyond the reach (ordered) leaves the point
    if !(x(dist).lt(x(reach)) || x(dist).unordered(x(reach))) {
        return;
    }
    let mode = |m: &Mem| m.i32(w(0xc));
    let then = match m.i32(w(8)) {
        1 => {
            let e = wave_dot_product(m, d, v);
            let ef = e.to_f32();
            m.set_f32(f.at(ALONG), ef);
            let sx = e * fl(m, v) + dot * fl(m, u);
            m.set_f32(d, (x(fl(m, d)) - sx).to_f32());
            let sy = x(ef) * fl(m, v + 4) + dot * fl(m, u + 4);
            let sz = dot * fl(m, u + 8) + x(ef) * fl(m, v + 8);
            let ny = (x(fl(m, d + 4)) - sy).to_f32();
            let nz = (x(fl(m, d + 8)) - sz).to_f32();
            m.set_f32(d + 4, ny);
            m.set_f32(d + 8, nz);
            let n = wave_norm(m, d);
            let scz = x(fl(m, s + 8));
            let r = if scz.eq(F80::ZERO) { n } else { n / scz };
            let g = wave_find_graph_value(m, w(0x4c), r.to_f32());
            let a = g * fl(m, f.at(AMP_RAW));
            m.set_f32(f.at(AMP), a.to_f32());
            if a.eq(F80::ZERO) {
                Then::Leave
            } else if mode(m) != 2 {
                Then::Move
            } else {
                by_normal(o, m, f, i, MESH_NORMAL_1)
            }
        }
        0 => {
            for c in 0..3u32 {
                let val = (x(fl(m, d + 4 * c)) - dot * fl(m, u + 4 * c)).to_f32();
                m.set_f32(d + 4 * c, val);
            }
            let n = wave_norm(m, d).to_f32();
            m.set_f32(f.at(ALONG), n);
            match mode(m) {
                1 => {
                    direction(m, f.at(INV), f.at(DIR), d, x(fl(m, f.at(ALONG))));
                    Then::Move
                }
                2 => by_normal(o, m, f, i, RESULT),
                _ => Then::Move,
            }
        }
        2 => {
            let n = wave_norm(m, d).to_f32();
            m.set_f32(f.at(ALONG), n);
            match mode(m) {
                0 | 1 => {
                    direction(m, f.at(INV), f.at(DIR), d, x(fl(m, f.at(ALONG))));
                    Then::Move
                }
                2 => by_normal(o, m, f, i, RESULT),
                _ => Then::Move,
            }
        }
        _ => Then::Move,
    };
    if then == Then::Leave {
        return;
    }
    let along = (x(fl(m, f.at(ALONG))) - fl(m, f.at(PHASE))).to_f32();
    m.set_f32(f.at(ALONG), along);
    let arg = (x(fl(m, f.at(FREQ))) * along).to_f32();
    let g = wave_find_graph_value(m, w(0x2c), arg);
    let mut h = g * fl(m, f.at(AMP));
    let start = x(m.f32(w(0x20)));
    let dist = fl(m, f.at(DIST));
    // `fcom`, `and 0x45`, `cmp 1`: the start below the distance
    if start.lt(x(dist)) {
        let reach = x(m.f32(w(0x24)));
        h = h * ((reach - dist) / (reach - start));
    }
    let dir = f.at(DIR);
    let vx = (h * fl(m, dir) + fl(m, pt)).to_f32();
    m.set_f32(pt, vx);
    let vy = (h * fl(m, dir + 4) + fl(m, pt + 4)).to_f32();
    m.set_f32(pt + 4, vy);
    let vz = (x(fl(m, pt + 8)) + h * fl(m, dir + 8)).to_f32();
    m.set_f32(pt + 8, vz);
    let (dict, chapter) = where_(m);
    o.call_addr(m, SI_ELEM_SET_VERTEX, &[dict, target, chapter, 2, 0, i as u32, pt]);
}

/// The direction from the point's normal (the three switches on the model's kind): kinds 6
/// and 9 leave the point; a patch, a mesh (its normal answered at `mesh_at`) and a NURBS
/// surface write the normal local; the rest keep it as it is. Then the normal's direction.
fn by_normal<O: WaveOutside + ?Sized>(o: &mut O, m: &mut Mem, f: &Frame, i: i32, mesh_at: u32) -> Then {
    let n = f.at(NORMAL);
    match m.i32(f.at(KIND)) {
        1 => {
            let cu = m.u16(f.at(U_COUNT)) as i32;
            let rem = i.checked_rem(cu).unwrap_or(0);
            let row = i.wrapping_sub(rem).checked_div(cu).unwrap_or(0);
            let patch = m.u32(f.at(PATCH));
            let (ku, nu) = (m.i16(patch.wrapping_add(4)), m.i16(f.at(U_COUNT)));
            spline_index_to_value(m, ku, nu, rem as i16, f.at(U_PARAM));
            let (kv, nv) = (m.i16(patch.wrapping_add(8)), m.i16(f.at(V_COUNT)));
            spline_index_to_value(m, kv, nv, row as i16, f.at(V_PARAM));
            let pu = si_x87::fmov(m.f32(f.at(U_PARAM)));
            let pv = si_x87::fmov(m.f32(f.at(V_PARAM)));
            let copy = m.u32(f.at(PATCH_COPY));
            o.call_addr(m, SI_PATCH_VALUE_AND_NORMAL, &[copy, pu.to_bits(), pv.to_bits(), f.at(PATCH_POINT), n]);
        }
        4 => {
            let mesh = m.u32(f.at(MESH));
            let at = f.at(mesh_at);
            o.call_addr(m, SI_COMPUTE_MESH_VERTEX_NORMAL, &[at, mesh, i as i16 as i32 as u32]);
            for c in 0..3u32 {
                let word = m.u32(at + 4 * c);
                m.set_u32(n + 4 * c, word);
            }
        }
        6 | 9 => return Then::Leave,
        10 => srf_normal(o, m, f, i),
        _ => {}
    }
    let len = wave_norm(m, n);
    direction(m, f.at(INV), f.at(DIR), n, len);
    Then::Move
}

/// A clamp as the original tests it: below `lo` gives `lo`, else above `hi` gives `hi`.
fn clamp(v: i32, lo: i32, hi: i32) -> i32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// The NURBS surface's normal at the knots of point `i` (0x084c8800 … 0x084c8923).
fn srf_normal<O: WaveOutside + ?Sized>(o: &mut O, m: &mut Mem, f: &Frame, i: i32) {
    let srf = m.u32(f.at(SRF));
    let at = |off: u32| srf.wrapping_add(off);
    let rem = i.checked_rem(m.i32(at(0x28))).unwrap_or(0);
    let q = i.wrapping_sub(rem).checked_div(m.i32(at(4))).unwrap_or(0);
    let (a, b) = (m.i32(at(0)), m.i32(at(0x24)));
    let iu = a.wrapping_add(q).wrapping_sub(3);
    let iv = b.wrapping_add(rem).wrapping_sub(3);
    let iu = if m.u16(at(0x20)) == 0 { clamp(iu, a.wrapping_sub(1), m.i32(at(8)).wrapping_sub(a)) } else { clamp(iu, 0, m.i32(at(8)).wrapping_sub(1)) };
    let iv = if m.u16(at(0x44)) == 0 { clamp(iv, b.wrapping_sub(1), m.i32(at(0x2c)).wrapping_sub(b)) } else { clamp(iv, 0, m.i32(at(0x2c)).wrapping_sub(1)) };
    let ku = m.u32(at(0xc)).wrapping_add((iu as u32).wrapping_mul(8));
    let kv = m.u32(at(0x30)).wrapping_add((iv as u32).wrapping_mul(8));
    // `fld qword`, `fstp qword`: the knots moved through the unit
    let (uk, vk) = (si_x87::fmov64(m.f64(ku)), si_x87::fmov64(m.f64(kv)));
    m.set_f64(f.at(U_KNOT), uk);
    m.set_f64(f.at(V_KNOT), vk);
    let block = m.si_calloc(1, 0x18);
    let copy = m.u32(f.at(SRF_COPY));
    let (ub, vb) = (uk.to_bits(), vk.to_bits());
    o.call_addr(m, SI_NURBS_SRF_VALUE_AND_NORMAL, &[f.at(RESULT), copy, ub as u32, (ub >> 32) as u32, vb as u32, (vb >> 32) as u32, block, 0, 0]);
    for k in 0..6u32 {
        let word = m.u32(f.at(RESULT) + 4 * k);
        m.set_u32(f.at(SRF_POINT) + 4 * k, word);
    }
    let n = f.at(NORMAL);
    for c in 0..3u32 {
        let v = F80::from_f64(m.f64(block.wrapping_add(8 * c))).to_f32();
        m.set_f32(n + 4 * c, v);
    }
    if block != 0 {
        m.free(block);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edtools::{ByAddr, SI_ELEMDICT_GET_ELEM_FROM_DATA};
    use si_dict::globals::SI_ELEM_DICTS;

    /// Every call logged; the wave's element 0x5000, the model's 0x5100, the curves 0x6000
    /// (amplitude `amp`) and 0x6100 (frequency 1), three points, the patch copy 0x7700.
    struct Log {
        calls: Vec<(u32, Vec<u32>)>,
        points: [[f32; 3]; 3],
        set: Vec<(u32, [f32; 3])>,
        amp: f32,
    }
    impl Log {
        fn new(amp: f32) -> Log {
            Log { calls: vec![], points: [[0.0, 0.0, 3.0], [0.0, 20.0, 0.0], [1.0, 0.0, 0.0]], set: vec![], amp }
        }
        fn called(&self, f: u32) -> usize {
            self.calls.iter().filter(|c| c.0 == f).count()
        }
    }
    impl ByAddr for Log {
        fn call_addr(&mut self, m: &mut Mem, f: u32, a: &[u32]) -> u32 {
            self.calls.push((f, a.to_vec()));
            match f {
                SI_ELEMDICT_GET_ELEM_FROM_DATA => 0x5000,
                SI_ED_GET_ELEM_FROM_MODEL => 0x5100,
                SI_ED_FIND_FCV => 0x6000 + if a[2] == 0x267 { 0x100 } else { 0 },
                SI_ELEM_GET_NB_VERTICES => 3,
                SI_ELEM_GET_VERTEX => {
                    for c in 0..3 {
                        m.set_f32(a[6] + 4 * c, self.points[a[5] as usize][c as usize]);
                    }
                    a[6]
                }
                SI_ELEM_SET_VERTEX => {
                    self.set.push((a[5], [m.f32(a[6]), m.f32(a[6] + 4), m.f32(a[6] + 8)]));
                    0
                }
                SI_PATCH_COPY => 0x7700,
                _ => 0,
            }
        }
    }
    impl WaveOutside for Log {
        fn call_f80(&mut self, _: &mut Mem, f: u32, a: &[u32]) -> F80 {
            self.calls.push((f, a.to_vec()));
            x(if a[0] == 0x6000 { self.amp } else { 1.0 })
        }
    }

    /// A wave of type 2 at the origin (U = y, V = x, scaling 1), reach 10, a flat profile of
    /// 1, direction mode `mode`; and a model of kind `kind`.
    fn world(m: &mut Mem, mode: i32, kind: i32) -> (u32, u32) {
        si_mem::init_globals(m, si_dict::globals::ALL);
        let dicts = m.malloc(0x70);
        m[SI_ELEM_DICTS] = Ptr32::new(dicts);
        m.set_f32(SI_FRAME_RATE, 30.0);
        let icon = m.malloc(0x170);
        for (off, v) in [(0x100u32, 1.0f32), (0x104, 1.0), (0x108, 1.0), (0x118, 1.0), (0x12c, 1.0)] {
            m.set_f32(icon + off, v);
        }
        let wave = m.malloc(0x84);
        m.set_i32(wave + 8, 2);
        m.set_i32(wave + 0xc, mode);
        m.set_i32(wave + 0x14, 100);
        m.set_f32(wave + 0x24, 10.0);
        let pts = m.malloc(16);
        for (k, v) in [-100.0f32, 1.0, 100.0, 1.0].into_iter().enumerate() {
            m.set_f32(pts + 4 * k as u32, v);
        }
        m.set_i32(wave + 0x2c, 2);
        m.set_u32(wave + 0x2c + 0x1c, pts);
        m.set_u32(wave + 0x74, icon);
        let model = m.malloc(0x50);
        m.set_i32(model + 0x30, kind);
        let def = m.malloc(0x20);
        m.set_u16(def + 0xc, 1);
        m.set_u16(def + 0xe, 1);
        m.set_u32(model + 0x48, def);
        (wave, model)
    }

    #[test]
    fn a_point_within_the_reach_moves_along_its_direction() {
        let mut m = Mem::new();
        let (wave, model) = world(&mut m, 0, 4);
        let mut o = Log::new(2.0);
        wave_geom_update(&mut o, &mut m, wave, model, 0.0, 0);
        // the point beyond the reach (y = 20) is not written
        assert_eq!(o.set, [(0, [0.0, 0.0, 5.0]), (2, [3.0, 0.0, 0.0])]);
        assert_eq!(o.called(SI_ELEM_GET_VERTEX), 3);
    }

    #[test]
    fn an_amplitude_of_zero_ends_it_before_the_copies() {
        let mut m = Mem::new();
        let (wave, model) = world(&mut m, 0, 1);
        let mut o = Log::new(0.0);
        wave_geom_update(&mut o, &mut m, wave, model, 0.0, 0);
        assert!(o.set.is_empty());
        assert_eq!((o.called(SI_PATCH_COPY), o.called(SI_ELEM_GET_NB_VERTICES)), (0, 0));
    }

    #[test]
    fn a_patch_is_copied_and_its_copy_disposed_of() {
        let mut m = Mem::new();
        let (wave, model) = world(&mut m, 0, 1);
        let mut o = Log::new(2.0);
        wave_geom_update(&mut o, &mut m, wave, model, 0.0, 0);
        assert_eq!(o.set.len(), 2);
        let dispose: Vec<&(u32, Vec<u32>)> = o.calls.iter().filter(|c| c.0 == SI_PATCH_DISPOSE).collect();
        assert_eq!(dispose.len(), 1);
        assert_eq!(o.calls.last().map(|c| c.0), Some(SI_PATCH_DISPOSE));
    }

    #[test]
    fn under_mode_2_a_spline_or_face_kind_leaves_every_point() {
        let mut m = Mem::new();
        let (wave, model) = world(&mut m, 2, 6);
        let mut o = Log::new(2.0);
        wave_geom_update(&mut o, &mut m, wave, model, 0.0, 0);
        assert!(o.set.is_empty());
        assert_eq!(o.called(SI_ELEM_GET_VERTEX), 3);
    }
}
