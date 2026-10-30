// SPDX-License-Identifier: MIT
//! A NURBS surface as the renderer's triangles: the untrimmed path of the original's
//! `nrb_convert.c` (finding 0025, section 13), on the memory image.
//!
//! `SI_nurbs2Triangle` is what the renderer's `SI_makeVertices` calls for a node of kind 10.
//! An untrimmed surface is sampled at the parameters `SI_SampleKnotVector` gives in each
//! direction (the surface's display steps, +0x70 and +0x74, to a span of average length),
//! with every sample at an inner knot of multiplicity `order − 1` or more given twice
//! (`repeatKnotsInSampling`: the surface may have a crease there, and each side of it gets
//! its own vertex). For each sample u the whole curve in V at u and the curve of its
//! derivative with respect to U are extracted; each vertex is the first evaluated at v, its
//! normal the cross product of the derivative across (the second, by the quotient rule) and
//! the derivative along (the first's own), each searched for nearby where it vanishes. A
//! cell of the grid is two triangles; the repeated samples are stepped over, so a crease
//! has no cells of zero width.
//!
//! Render vertices (`SI_RVertex`, 0x4c bytes) and triangles (`tn`, 0x60 bytes) are written
//! as the original writes them: `si_math::geometry` has the two records.

use crate::image::{crv_value, crv_value4d, nurbs_crv_dispose, sample_knot_vector, srf_extract_curve, Halt};
use si_mem::Mem;
use si_x87::F80;

/// `uNoPoints` (0x088221c0), `vNoPoints` (0x088221c4): the vertex grid's two counts, which
/// the triangulation leaves for whoever reads them after it.
pub const U_NO_POINTS: u32 = 0x0882_21c0;
pub const V_NO_POINTS: u32 = 0x0882_21c4;
/// `needTrianglesWithIndex` (0x086d9fc0), `trianglesWithIndex` (0x086d9fc4): what the
/// trimmed path is also handed.
pub const NEED_TRIANGLES_WITH_INDEX: u32 = 0x086d_9fc0;
pub const TRIANGLES_WITH_INDEX: u32 = 0x086d_9fc4;

/// The doubles of `.rodata` the triangulation reads, by address.
/// 0x086617b8: below this a scale of the texture coordinates is replaced by 1, and within
/// it of the range end a search steps backwards.
const TOL_SCALE: f64 = 0.0001;
/// 0x086617c0: a sample repeated at a knot is evaluated this far before it.
const BEFORE_KNOT: f64 = 0.001;
/// 0x086617c8: a derivative whose squared length is below this has vanished.
const VANISHED: f64 = 1e-6;
/// 0x086617d0, 0x086617d8: the steps of the search for a derivative.
const NUDGE: f64 = 0.01;
/// 0x086617e0 (`repeatKnotsInSampling`), 0x086617e8
/// (`EvaluateNonTrimmedNurbsFromBaseGeometry`): coinciding knots, coinciding ranges.
const SAME: f64 = 1e-6;

/// How often the triangulation searched for a vanished derivative (across, along) and how
/// many of those searches extracted curves anew (every third try): counts for the checks'
/// reports (`EMUCHECK_TRACE=1`), not a part of the original.
pub static SEARCHES: [std::sync::atomic::AtomicU64; 3] = [const { std::sync::atomic::AtomicU64::new(0) }; 3];

fn count(i: usize) {
    SEARCHES[i].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

/// Address of element `i` (any sign: the arithmetic of a 32-bit pointer) of size `size`.
fn at(base: u32, i: i32, size: u32) -> u32 {
    base.wrapping_add((i as u32).wrapping_mul(size))
}

/// Two doubles of the image compared as `fucom` and a test of "equal" do: ordered and equal.
fn same(a: F80, b: F80) -> bool {
    a.eq(b)
}

/// [soft 0x084e28fc repeatKnotsInSampling]: `repeatKnotsInSampling(dir, &count, &array)`.
///
/// The inner knots of the direction (open: indices `order − 1` up to `knots − order + 1`,
/// closed: 0 up to `knots − 1`) are walked by runs of knots within 1e-6 of the run's first
/// (an absolute difference in the register, strictly below); the first index of every run
/// at least `order − 1` long is noted, in a block of `SI_calloc(knots, 4)`. When there is
/// one, a new array of `SI_calloc(count + noted, 8)` gets every sample, and a sample within
/// 1e-6 of the next noted knot twice (after which the next noted knot is looked at; past
/// the last noted one the block's next word is read, which `SI_calloc` cleared: knot 0);
/// the old array is freed, the new one takes its place and the count grows. The block of
/// noted indices is freed.
pub fn repeat_knots_in_sampling(m: &mut Mem, dir: u32, count: u32, array: u32) {
    let nk = m.i32(dir + 8);
    let noted = m.si_calloc(nk as u32, 4);
    let knots = m.u32(dir + 0x0c);
    let km1 = m.i32(dir).wrapping_sub(1);
    let (first, end) = if m.u16(dir + 0x20) != 0 { (0, nk.wrapping_sub(1)) } else { (km1, nk.wrapping_sub(km1)) };
    let eps = x(SAME);
    let mut n = 0i32;
    let mut i = first;
    while i < end {
        let mut j = i.wrapping_add(1);
        while j < end && (x(m.f64(at(knots, i, 8))) - x(m.f64(at(knots, j, 8)))).abs().lt(eps) {
            j += 1;
        }
        let run = j.wrapping_sub(i);
        if run >= km1 {
            m.set_i32(at(noted, n, 4), i);
            n += 1;
        }
        i = i.wrapping_add(run);
    }
    if n > 0 {
        let new = m.si_calloc(m.i32(count).wrapping_add(n) as u32, 8);
        let (mut o, mut k, mut c) = (0i32, 0i32, 0i32);
        while c < m.i32(count) {
            let old = m.u32(array);
            let s = x(m.f64(at(old, c, 8)));
            m.set_f64(at(new, o, 8), s.to_f64());
            let knot = m.i32(at(noted, k, 4));
            o += 1;
            if (s - x(m.f64(at(knots, knot, 8)))).abs().lt(eps) {
                m.set_f64(at(new, o, 8), s.to_f64());
                o += 1;
                k += 1;
            }
            c += 1;
        }
        let old = m.u32(array);
        if old != 0 {
            m.free(old);
            m.set_u32(array, 0);
        }
        m.set_u32(array, new);
        let grown = m.i32(count).wrapping_add(n);
        m.set_i32(count, grown);
    }
    if noted != 0 {
        m.free(noted);
    }
}

/// The source's absolute value of a difference, a macro expanded around `a − b` without
/// parentheses: `a − b` when that is not below 0, else **`−a − b`** (read from the
/// listing: the negative case loads `a`, changes its sign and subtracts `b`).
fn abs_macro(a: F80, b: F80) -> F80 {
    let d = a - b;
    if d.lt(F80::ZERO) || d.unordered(F80::ZERO) {
        -a - b
    } else {
        d
    }
}

/// The samples of one direction brought from the range of `srf` to that of `shape` (the
/// ranges at +0x10 and +0x18 of each direction), each `((s − a0) / (a1 − a0)) × (b1 − b0) +
/// b0` in the register, rounded once. Done when the starts **or** the ends agree to 1e-6 as
/// [`abs_macro`] measures them; otherwise the samples are left alone.
fn remap(m: &mut Mem, shape_dir: u32, srf_dir: u32, array: u32, n: i32) {
    let (a0, a1) = (x(m.f64(srf_dir + 0x10)), x(m.f64(srf_dir + 0x18)));
    let (b0, b1) = (x(m.f64(shape_dir + 0x10)), x(m.f64(shape_dir + 0x18)));
    let eps = x(SAME);
    if abs_macro(a0, b0).lt(eps) || abs_macro(a1, b1).lt(eps) {
        let (wa, wb) = (a1 - a0, b1 - b0);
        let mut i = 0;
        while i < n {
            let p = at(array, i, 8);
            let s = x(m.f64(p));
            m.set_f64(p, ((((s - a0) / wa) * wb) + b0).to_f64());
            i += 1;
        }
    }
}

/// [soft 0x084e2ad0 EvaluateNonTrimmedNurbsFromBaseGeometry]:
/// `EvaluateNonTrimmedNurbsFromBaseGeometry(shape, srf, verts, nu, nv, us, vs)`: the
/// positions the vertices lend to texture projection (+0x20, +0x24, +0x28) from the surface
/// that lends them (`shape`), at the samples of `srf` brought to `shape`'s ranges (the two
/// sample arrays are changed in place: [`remap`], U then V). For each sample u above the
/// last one the curve in V of `shape` at u is extracted (the first u is compared with the
/// first sample less 1, in the register), and kept for the next sample when that is equal;
/// each vertex is its homogeneous point at v times `1 / w` (in the register), rounded
/// straight to float.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_non_trimmed_from_base(m: &mut Mem, shape: u32, srf: u32, verts: u32, nu: i32, nv: i32, us: u32, vs: u32) -> Result<(), Halt> {
    remap(m, shape, srf, us, nu);
    remap(m, shape + 0x24, srf + 0x24, vs, nv);
    let slot = m.local(4);
    m.set_u32(slot, 0);
    let mut prev = x(m.f64(us)) - F80::ONE;
    let mut k = 0i32;
    let mut i = 0;
    let r = (|| {
        while i < nu {
            let u = m.f64(at(us, i, 8));
            if x(u).gt(prev) {
                let c = srf_extract_curve(m, shape, u, 0, 0)?;
                m.set_u32(slot, c);
            }
            let mut j = 0;
            while j < nv {
                let p = crv_value4d(m, m.u32(slot), m.f64(at(vs, j, 8)))?;
                let inv = F80::ONE / x(p.w.0);
                let v = at(verts, k, 0x4c);
                m.set_f32(v + 0x20, (x(p.x.0) * inv).to_f32());
                m.set_f32(v + 0x24, (x(p.y.0) * inv).to_f32());
                m.set_f32(v + 0x28, (inv * x(p.z.0)).to_f32());
                k += 1;
                j += 1;
            }
            if i == nu.wrapping_sub(1) || !same(x(m.f64(at(us, i + 1, 8))), x(u)) {
                nurbs_crv_dispose(m, slot);
            }
            prev = x(u);
            i += 1;
        }
        Ok(())
    })();
    m.release(4);
    r
}

/// The step of a search in one direction: −0.01 where the parameter is within 1e-4 of the
/// direction's range end (strictly, the difference's absolute value in the register) or
/// where the next sample equals it (a repeated sample: the vertex is the end of a piece),
/// else +0.01. The next sample is read only when the first test fails.
fn nudge(m: &Mem, t: F80, end: f64, next: u32) -> f64 {
    if x(TOL_SCALE).gt((t - x(end)).abs()) || same(x(m.f64(next)), t) {
        -NUDGE
    } else {
        NUDGE
    }
}

/// The squared length of three doubles, `(x² + y²) + z²` in the register.
fn squared(v: [f64; 3]) -> F80 {
    let (a, b, c) = (x(v[0]), x(v[1]), x(v[2]));
    (a * a + b * b) + c * c
}

/// The derivative across, from the homogeneous point `d` of the derivative curve and the
/// point `p` (rounded doubles) with `inv` = 1 / w: `inv × (D − D.w × p)` a coordinate, in
/// the register (the quotient rule for a first derivative).
fn across(d: &crate::NurbsPoint, p: [f64; 3], inv: F80) -> [F80; 3] {
    let w = x(d.w.0);
    [inv * (x(d.x.0) - w * x(p[0])), inv * (x(d.y.0) - w * x(p[1])), inv * (x(d.z.0) - w * x(p[2]))]
}

fn rounded(v: [F80; 3]) -> [f64; 3] {
    [v[0].to_f64(), v[1].to_f64(), v[2].to_f64()]
}

/// Below this a scale of texture coordinates is replaced by 1: `|s| < 1e-4` (a NaN is
/// kept). The test as the listing makes it: the sign first (`0 > s`, or unordered, is the
/// negative branch), then `1e-4 > s` or `1e-4 > −s`.
fn scale_or_one(s: f64) -> f64 {
    let sx = x(s);
    let neg = F80::ZERO.gt(sx) || F80::ZERO.unordered(sx);
    let small = if neg { x(TOL_SCALE).gt(-sx) } else { x(TOL_SCALE).gt(sx) };
    if small {
        1.0
    } else {
        s
    }
}

/// [soft 0x084e17f8 TriangulateNonTrimmedNurbsSrf]:
/// `TriangulateNonTrimmedNurbsSrf(shape, srf, materials, elem_id, &n_verts, &n_tris,
/// &verts, &tris)`.
///
/// 1. `SI_SampleKnotVector` of U with the step at +0x70 and of V with +0x74. The scales of
///    the texture coordinates, `1 / (last − first)` of each array, rounded to double and
///    replaced by 1 when below 1e-4 in size ([`scale_or_one`]). Then
///    [`repeat_knots_in_sampling`] on both; `uNoPoints`, `vNoPoints` and `n_verts` = the
///    product of the new counts; the vertices from `SI_calloc(n_verts, 0x4c)`.
/// 2. For each sample u (index i): the curve in V at u (`SI_nurbsSrfExtractCurve(srf, u, 0,
///    0)`) when u is above the previous sample (the first is compared with the first
///    sample less 1, in the register); the curve of the derivative with respect to U at u,
///    or at u − 0.001 when the next sample equals u (the left side of a crease).
/// 3. For each sample v (index j; the first compared with the first sample less 1, rounded
///    to double): the homogeneous point P (`SI_nurbsCrvValue4D`) when v is above the
///    previous sample or the previous vertex needed a search for its derivative across;
///    else the last P stands. `inv` = 1 / P.w in the register; the position p is P × inv,
///    each coordinate rounded to double; the vertex's +0x08 and +0x20 get x **rounded from
///    the register straight to float**, y and z from the rounded doubles (+0x0c, +0x24,
///    +0x10, +0x28).
/// 4. Across: the derivative curve's point at v (v − 0.001 before a repeated sample), by
///    [`across`]; its squared length in the register **from the unrounded values**. Below
///    1e-6: the search. Steps by [`nudge`]; tried again with the squared length of the
///    rounded doubles; then while it stays below 1e-6, up to the 99th try: tries 1 and 2
///    (not multiples of 3) evaluate the current derivative curve at v + the accumulated V
///    step, with the point and `inv` as they stand, and add a V step; every third try frees
///    the curves the search extracted (not the two of the sample), extracts both anew at u +
///    the accumulated U step, takes P, `inv` and p from the new position curve at v + the V
///    steps, evaluates the new derivative curve there, and adds a U step. Curves the search
///    extracted are freed at its end. The previous-vertex mark is set by the search and
///    cleared by every vertex before it.
/// 5. Along: `SI_nurbsCrvValue(curve in V, v (or v − 0.001), 1)`; when its squared length
///    (from the doubles) is below 1e-6 a search like the one above, on the position curve
///    alone: every third try extracts it anew at u + the U steps.
/// 6. The normal: across × along from the doubles, each coordinate in the register rounded
///    to float (+0x14, +0x18, +0x1c); its length the square root of the floats' squares,
///    rounded to float; when its size is above 1e-4 (as doubles) each coordinate is divided
///    by it. The texture pair: `1 − (u − first u) × scale` and `(v − first v) × scale`,
///    rounded from the register to float (+0x00, +0x04); +0x40 is 0, +0x44 0xffb2b2b2.
/// 7. After the row: the curve in V is freed unless the next sample equals u (it is then
///    the next row's); the derivative curve is freed.
/// 8. Triangles: `n_tris = 2 × (V samples − 1) × (U samples − 1)` **with the counts before
///    the repetition**, from `SI_calloc(n_tris, 0x60)`; each linked to the next (+0x5c),
///    +0x58 the element number, +0x4c the first material (`*materials`) or 0. Corners: for
///    each cell, the index in each direction stepping over a repeated sample, (a, b, c) and
///    (a, c, d) with a = (i, j), b = (i + 1, j), c = (i + 1, j + 1), d = (i, j + 1) in the
///    grid of the counts after the repetition.
/// 9. With a `shape` other than `srf`, [`evaluate_non_trimmed_from_base`]. Both sample
///    arrays are freed.
#[allow(clippy::too_many_arguments)]
pub fn triangulate_non_trimmed(m: &mut Mem, shape: u32, srf: u32, materials: u32, elem_id: i32, n_verts: u32, n_tris: u32, verts_out: u32, tris_out: u32) -> Result<(), Halt> {
    let frame = m.local(32);
    let (us, nus, vs, nvs) = (frame, frame + 4, frame + 8, frame + 12);
    let (c0, c1, e8, ec) = (frame + 16, frame + 20, frame + 24, frame + 28);
    let r = triangulate_body(m, shape, srf, materials, elem_id, n_verts, n_tris, verts_out, tris_out, [us, nus, vs, nvs, c0, c1, e8, ec]);
    m.release(32);
    r
}

#[allow(clippy::too_many_arguments)]
fn triangulate_body(m: &mut Mem, shape: u32, srf: u32, materials: u32, elem_id: i32, n_verts: u32, n_tris: u32, verts_out: u32, tris_out: u32, slots: [u32; 8]) -> Result<(), Halt> {
    let [us, nus, vs, nvs, c0, c1, e8, ec] = slots;
    let tiny = x(VANISHED);
    sample_knot_vector(m, srf, m.i32(srf + 0x70), us, nus)?;
    sample_knot_vector(m, srf + 0x24, m.i32(srf + 0x74), vs, nvs)?;
    let (nu0, nv0) = (m.i32(nus), m.i32(nvs));
    let scale_of = |m: &Mem, arr: u32, n: i32| -> f64 {
        let a = m.u32(arr);
        (F80::ONE / (x(m.f64(at(a, n.wrapping_sub(1), 8))) - x(m.f64(a)))).to_f64()
    };
    let scale_u = scale_or_one(scale_of(m, us, nu0));
    let scale_v = scale_or_one(scale_of(m, vs, nv0));
    repeat_knots_in_sampling(m, srf, nus, us);
    repeat_knots_in_sampling(m, srf + 0x24, nvs, vs);
    let (nu, nv) = (m.i32(nus), m.i32(nvs));
    m.set_i32(U_NO_POINTS, nu);
    m.set_i32(V_NO_POINTS, nv);
    let total = nu.wrapping_mul(nv);
    m.set_i32(n_verts, total);
    let verts = m.si_calloc(total as u32, 0x4c);
    m.set_u32(verts_out, verts);
    let (u_end, v_end) = (m.f64(srf + 0x18), m.f64(srf + 0x3c));
    // the frame's records that live from one vertex to the next: P (the last homogeneous
    // point; the original's is stack content until the first is taken), p, across
    let mut big_p = <crate::NurbsPoint as si_types::Zero>::zero();
    let mut k = 0i32;
    let mut mark = false;
    let mut prev_u = x(m.f64(m.u32(us))) - F80::ONE;
    let mut i = 0i32;
    while i < m.i32(nus) {
        let u = m.f64(at(m.u32(us), i, 8));
        let uf = x(u);
        if uf.gt(prev_u) {
            let c = srf_extract_curve(m, srf, u, 0, 0)?;
            m.set_u32(c0, c);
        }
        let tu = if i < m.i32(nus).wrapping_sub(1) && same(x(m.f64(at(m.u32(us), i + 1, 8))), uf) { (uf - x(BEFORE_KNOT)).to_f64() } else { u };
        let c = srf_extract_curve(m, srf, tu, 0, 1)?;
        m.set_u32(c1, c);
        let mut prev_v = (x(m.f64(m.u32(vs))) - F80::ONE).to_f64();
        let mut j = 0i32;
        while j < m.i32(nvs) {
            let v = m.f64(at(m.u32(vs), j, 8));
            let vf = x(v);
            if vf.gt(x(prev_v)) || mark {
                big_p = crv_value4d(m, m.u32(c0), v)?;
            }
            let mut inv = F80::ONE / x(big_p.w.0);
            prev_v = v;
            let px80 = x(big_p.x.0) * inv;
            let mut p = [px80.to_f64(), (x(big_p.y.0) * inv).to_f64(), (x(big_p.z.0) * inv).to_f64()];
            let vert = at(m.u32(verts_out), k, 0x4c);
            let fx = px80.to_f32();
            m.set_f32(vert + 0x20, fx);
            m.set_f32(vert + 0x08, fx);
            let fy = x(p[1]).to_f32();
            m.set_f32(vert + 0x24, fy);
            m.set_f32(vert + 0x0c, fy);
            let fz = x(p[2]).to_f32();
            m.set_f32(vert + 0x28, fz);
            m.set_f32(vert + 0x10, fz);
            let repeated_v = j < m.i32(nvs).wrapping_sub(1) && same(x(m.f64(at(m.u32(vs), j + 1, 8))), vf);
            let tv = if repeated_v { (vf - x(BEFORE_KNOT)).to_f64() } else { v };
            let d = crv_value4d(m, m.u32(c1), tv)?;
            let a80 = across(&d, p, inv);
            mark = false;
            let mut a = rounded(a80);
            let (a0, a1, a2) = (a80[0], a80[1], a80[2]);
            if ((a0 * a0 + a1 * a1) + a2 * a2).lt(tiny) {
                count(0);
                mark = true;
                let su = nudge(m, uf, u_end, at(m.u32(us), i + 1, 8));
                let sv = nudge(m, vf, v_end, at(m.u32(vs), j + 1, 8));
                let (mut acc_u, mut acc_v) = (su, sv);
                m.set_u32(e8, m.u32(c0));
                m.set_u32(ec, m.u32(c1));
                let mut tries = 1i32;
                if squared(a).lt(tiny) {
                    loop {
                        if tries % 3 != 0 {
                            let d = crv_value4d(m, m.u32(ec), (vf + x(acc_v)).to_f64())?;
                            a = rounded(across(&d, p, inv));
                            acc_v = (x(acc_v) + x(sv)).to_f64();
                        } else {
                            if m.u32(e8) != m.u32(c0) {
                                nurbs_crv_dispose(m, e8);
                            }
                            if m.u32(ec) != m.u32(c1) {
                                nurbs_crv_dispose(m, ec);
                            }
                            count(2);
                            let at_u = (uf + x(acc_u)).to_f64();
                            let c = srf_extract_curve(m, srf, at_u, 0, 0)?;
                            m.set_u32(e8, c);
                            let c = srf_extract_curve(m, srf, at_u, 0, 1)?;
                            m.set_u32(ec, c);
                            let at_v = (vf + x(acc_v)).to_f64();
                            big_p = crv_value4d(m, m.u32(e8), at_v)?;
                            inv = F80::ONE / x(big_p.w.0);
                            p = [(x(big_p.x.0) * inv).to_f64(), (x(big_p.y.0) * inv).to_f64(), (x(big_p.z.0) * inv).to_f64()];
                            let d = crv_value4d(m, m.u32(ec), at_v)?;
                            a = rounded(across(&d, p, inv));
                            acc_u = (x(acc_u) + x(su)).to_f64();
                        }
                        tries += 1;
                        if !(squared(a).lt(tiny) && tries <= 99) {
                            break;
                        }
                    }
                }
                if m.u32(e8) != m.u32(c0) {
                    nurbs_crv_dispose(m, e8);
                }
                if m.u32(ec) != m.u32(c1) {
                    nurbs_crv_dispose(m, ec);
                }
            }
            let mut along = crv_value(m, m.u32(c0), tv, 1)?;
            if squared(along).lt(tiny) {
                count(1);
                let su = nudge(m, uf, u_end, at(m.u32(us), i + 1, 8));
                let sv = nudge(m, vf, v_end, at(m.u32(vs), j + 1, 8));
                let (mut acc_u, mut acc_v) = (su, sv);
                let mut tries = 1i32;
                m.set_u32(e8, m.u32(c0));
                if squared(along).lt(tiny) {
                    loop {
                        if tries % 3 != 0 {
                            along = crv_value(m, m.u32(e8), (vf + x(acc_v)).to_f64(), 1)?;
                            acc_v = (x(acc_v) + x(sv)).to_f64();
                        } else {
                            if m.u32(e8) != m.u32(c0) {
                                nurbs_crv_dispose(m, e8);
                            }
                            count(2);
                            let c = srf_extract_curve(m, srf, (uf + x(acc_u)).to_f64(), 0, 0)?;
                            m.set_u32(e8, c);
                            along = crv_value(m, c, (vf + x(acc_v)).to_f64(), 1)?;
                            acc_u = (x(acc_u) + x(su)).to_f64();
                        }
                        tries += 1;
                        if !(squared(along).lt(tiny) && tries <= 99) {
                            break;
                        }
                    }
                }
                if m.u32(e8) != m.u32(c0) {
                    nurbs_crv_dispose(m, e8);
                }
            }
            // the normal, across × along, from the doubles
            let (ax, ay, az) = (x(a[0]), x(a[1]), x(a[2]));
            let (lx, ly, lz) = (x(along[0]), x(along[1]), x(along[2]));
            let vert = at(m.u32(verts_out), k, 0x4c);
            m.set_f32(vert + 0x14, (ay * lz - az * ly).to_f32());
            let vert = at(m.u32(verts_out), k, 0x4c);
            m.set_f32(vert + 0x18, (az * lx - ax * lz).to_f32());
            let vert = at(m.u32(verts_out), k, 0x4c);
            m.set_f32(vert + 0x1c, (ax * ly - ay * lx).to_f32());
            let vert = at(m.u32(verts_out), k, 0x4c);
            let (nx, ny, nz) = (F80::from_f32(m.f32(vert + 0x14)), F80::from_f32(m.f32(vert + 0x18)), F80::from_f32(m.f32(vert + 0x1c)));
            let len = F80::from_f32(((nx * nx + ny * ny) + nz * nz).sqrt().to_f32());
            if len.abs().gt(x(TOL_SCALE)) {
                for off in [0x14, 0x18, 0x1c] {
                    let vert = at(m.u32(verts_out), k, 0x4c);
                    let c = F80::from_f32(m.f32(vert + off));
                    m.set_f32(vert + off, (c / len).to_f32());
                }
            }
            let vert = at(m.u32(verts_out), k, 0x4c);
            m.set_f32(vert, (F80::ONE - (uf - x(m.f64(m.u32(us)))) * x(scale_u)).to_f32());
            m.set_f32(vert + 4, ((vf - x(m.f64(m.u32(vs)))) * x(scale_v)).to_f32());
            m.set_u32(vert + 0x40, 0);
            m.set_u32(vert + 0x44, 0xffb2_b2b2);
            k = k.wrapping_add(1);
            j += 1;
        }
        if i == m.i32(nus).wrapping_sub(1) || !same(x(m.f64(at(m.u32(us), i + 1, 8))), uf) {
            nurbs_crv_dispose(m, c0);
        }
        nurbs_crv_dispose(m, c1);
        prev_u = uf;
        i += 1;
    }
    // the triangles, counted before the repetition
    let count = nv0.wrapping_sub(1).wrapping_mul(2).wrapping_mul(nu0.wrapping_sub(1));
    m.set_i32(n_tris, count);
    let tris = m.si_calloc(count as u32, 0x60);
    m.set_u32(tris_out, tris);
    let first_material = if materials != 0 { m.u32(materials) } else { 0 };
    let mut t = 0i32;
    while t < m.i32(n_tris) {
        let base = m.u32(tris_out);
        let rec = at(base, t, 0x60);
        let next = if t < m.i32(n_tris).wrapping_sub(1) { at(m.u32(tris_out), t + 1, 0x60) } else { 0 };
        m.set_u32(rec + 0x5c, next);
        m.set_i16(rec + 0x58, elem_id as i16);
        m.set_u32(at(m.u32(tris_out), t, 0x60) + 0x4c, first_material);
        t += 1;
    }
    let mut off = 0u32;
    let mut iu = 0i32;
    let mut row = 0i32;
    while row < nu0.wrapping_sub(1) {
        let a = m.u32(us);
        if same(x(m.f64(at(a, iu, 8))), x(m.f64(at(a, iu + 1, 8)))) {
            iu += 1;
        }
        let mut iv = 0i32;
        let mut cells = nv0.wrapping_sub(1);
        if cells > 0 {
            loop {
                let b = m.u32(vs);
                if same(x(m.f64(at(b, iv, 8))), x(m.f64(at(b, iv + 1, 8)))) {
                    iv += 1;
                }
                let n = m.i32(nvs);
                let verts = m.u32(verts_out);
                let va = at(verts, iu.wrapping_mul(n).wrapping_add(iv), 0x4c);
                let vb = at(verts, (iu + 1).wrapping_mul(n).wrapping_add(iv), 0x4c);
                let vc = at(verts, (iu + 1).wrapping_mul(n).wrapping_add(iv + 1), 0x4c);
                let vd = at(verts, iu.wrapping_mul(n).wrapping_add(iv + 1), 0x4c);
                let base = m.u32(tris_out);
                for (o, v) in [(0, va), (4, vb), (8, vc), (0x60, va), (0x64, vc), (0x68, vd)] {
                    m.set_u32(base.wrapping_add(off + o), v);
                }
                off = off.wrapping_add(0xc0);
                iv += 1;
                cells -= 1;
                if cells == 0 {
                    break;
                }
            }
        }
        row += 1;
        iu += 1;
    }
    if shape != 0 && shape != srf {
        evaluate_non_trimmed_from_base(m, shape, srf, m.u32(verts_out), m.i32(nus), m.i32(nvs), m.u32(us), m.u32(vs))?;
    }
    let b = m.u32(vs);
    if b != 0 {
        m.free(b);
        m.set_u32(vs, 0);
    }
    let a = m.u32(us);
    if a != 0 {
        m.free(a);
        m.set_u32(us, 0);
    }
    Ok(())
}

/// [soft 0x084e16f8 SI_nurbs2Triangle]: `SI_nurbs2Triangle(shape, srf, materials, elem_id,
/// &n_verts, &verts, &n_tris, &tris, flag)`, what `SI_makeVertices` calls for a node of
/// kind 10 (its definition that lends positions, its current surface, +0x20c, +0x354, and
/// the addresses of +0x228, +0x22c, +0x230, +0x234).
///
/// Nothing at all for a null surface, one without points or knots in either direction.
/// A `flag` above 0 is put in both display steps (+0x70, +0x74) of the surface, and of
/// `shape` when it is another record, for the duration and put back after. A surface
/// without trims (+0x60 is 0) goes to [`triangulate_non_trimmed`], whose two counts land in
/// locals that are then copied to `*n_verts` and `*n_tris`; a trimmed one goes to
/// `TriangulateTrimmedNurbsSrf (0x084fd14c)` with `needTrianglesWithIndex` (0x086d9fc0) and
/// `&trianglesWithIndex` (0x086d9fc4), which is not ported: [`Halt::Unported`], with the
/// steps put back.
#[allow(clippy::too_many_arguments)]
pub fn nurbs2triangle(m: &mut Mem, shape: u32, srf: u32, materials: u32, elem_id: i32, n_verts: u32, verts: u32, n_tris: u32, tris: u32, flag: i16) -> Result<(), Halt> {
    if srf == 0 || m.i32(srf + 4) <= 0 || m.u32(srf + 0x0c) == 0 || m.i32(srf + 0x28) <= 0 || m.u32(srf + 0x30) == 0 {
        return Ok(());
    }
    let (kept_u, kept_v) = (m.u32(srf + 0x70), m.u32(srf + 0x74));
    let other = shape != 0 && shape != srf;
    if flag > 0 {
        let f = flag as i32;
        m.set_i32(srf + 0x74, f);
        m.set_i32(srf + 0x70, f);
        if other {
            m.set_i32(shape + 0x74, f);
            m.set_i32(shape + 0x70, f);
        }
    }
    let put_back = |m: &mut Mem| {
        if flag > 0 {
            m.set_u32(srf + 0x70, kept_u);
            m.set_u32(srf + 0x74, kept_v);
            if other {
                m.set_u32(shape + 0x70, kept_u);
                m.set_u32(shape + 0x74, kept_v);
            }
        }
    };
    if m.u32(srf + 0x60) != 0 {
        put_back(m);
        return Err(Halt::Unported("TriangulateTrimmedNurbsSrf"));
    }
    let counts = m.local(8);
    let r = triangulate_non_trimmed(m, shape, srf, materials, elem_id, counts, counts + 4, verts, tris);
    if r.is_ok() {
        let (a, b) = (m.u32(counts), m.u32(counts + 4));
        m.set_u32(n_verts, a);
        m.set_u32(n_tris, b);
        put_back(m);
    }
    m.release(8);
    r
}
