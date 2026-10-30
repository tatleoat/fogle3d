// SPDX-License-Identifier: MIT
//! The extend / merge / blend group of `blend.c` (package 14, nurbsgeom2), on the memory image (finding 0025,
//! section 17).
//!
//! The matrix calls (`SI_generalMatrix*Dbl`) run natively: allocation, identity, product and disposal are the
//! image forms of `fit2.rs`; the pseudo-inverse is [`greville_inverse`], the same arithmetic as `si-kine`'s image
//! form (which this crate cannot depend on: `si-kine` -> `si-model` -> `si-nurbs`) and checked here through the
//! functions that call it. `SI_nurbsMatrixBasisFunction` is `fit2`'s port.

use crate::blend::{build_delta_vector, extend_begin_knot_from_deltas, extend_end_knot_from_deltas, merge_knot_sequence, reverse_knot_seq};
use crate::fit::x;
use crate::fit2::{mat_alloc, mat_at, mat_dispose, mat_identity, mat_multiply, nurbs_matrix_basis_function};
use crate::fit::SI_SHOW_ACTIVITY;
use crate::image::{crv_copy, srf_copy, srf_dispose, SI_GBL_INDEX};
use si_hrc::image::nurbs::{knots_allocate, nurbs_crv_allocate, nurbs_crv_dispose, nurbs_points_allocate, nurbs_srf_allocate};
use si_mem::{Mem, Outside};
use si_x87::{fmov64 as fmovd, F80};

/// `SI_gblIndex2` (0x08788970): the second renumbering table of the editing routines.
pub const SI_GBL_INDEX2: u32 = 0x0878_8970;

// ---- the pseudo-inverse on the image ----

fn vtn_anorm_dbl(v: &[f64]) -> F80 {
    let mut sum = F80::ZERO;
    for &e in v {
        sum = x(e).abs() + sum;
    }
    sum
}

fn vtn_inv_dbl(v: &[f64], out: &mut [f64]) {
    let mut s = F80::ZERO;
    for &e in v {
        s = x(e) * x(e) + s;
    }
    if x(1e-10).lt(s) {
        let r = F80::ONE / s;
        for (o, &e) in out.iter_mut().zip(v) {
            *o = (x(e) * r).to_f64();
        }
    } else {
        for o in out.iter_mut() {
            *o = 0.0;
        }
    }
}

/// The pseudo-inverse of `a` into `out` by Greville's column recursion (the callee `SI_generalMatrixGrevilleInverseDbl`
/// 0x0851c710 of the extend routines; its port is `si-kine`'s `greville_inverse_dbl`, copied here for the reason
/// in the module note). Four work vectors from `SI_malloc` and given back with `free`. Always 1.
pub(crate) fn greville_inverse(m: &mut Mem, a: u32, out: u32) -> i32 {
    let rows = m.i32(a);
    let cols = m.i32(a.wrapping_add(4));
    let mu = rows.max(0) as usize;
    let nu = cols.max(0) as usize;
    let b_col = m.si_malloc((rows as u32).wrapping_mul(8));
    let b_kv = m.si_malloc((rows as u32).wrapping_mul(8));
    let b_c = m.si_malloc((rows as u32).wrapping_mul(8));
    let b_d = m.si_malloc((cols as u32) << 3);
    let load = |m: &Mem, b: u32, n: usize| (0..n).map(|i| m.f64(b + 8 * i as u32)).collect::<Vec<f64>>();
    let mut col = load(m, b_col, mu);
    let mut kv = load(m, b_kv, mu);
    let mut c = load(m, b_c, mu);
    let mut d = load(m, b_d, nu);
    let get = |m: &Mem, mat: u32, i: usize, j: usize| m.f64(mat_at(m, mat, i as i32, j as i32));
    let set = |m: &mut Mem, mat: u32, i: usize, j: usize, v: f64| {
        let at = mat_at(m, mat, i as i32, j as i32);
        m.set_f64(at, v);
    };
    for i in 0..mu {
        col[i] = fmovd(get(m, a, i, 0));
    }
    vtn_inv_dbl(&col, &mut kv);
    for i in 0..mu {
        set(m, out, 0, i, fmovd(kv[i]));
    }
    for k in 1..nu {
        for i in 0..mu {
            col[i] = fmovd(get(m, a, i, k));
        }
        for j in 0..k {
            d[j] = 0.0;
            for i in 0..mu {
                d[j] = (x(get(m, out, j, i)) * col[i] + d[j]).to_f64();
            }
        }
        for i in 0..mu {
            c[i] = fmovd(col[i]);
            for j in 0..k {
                c[i] = (c[i] - x(get(m, a, i, j)) * d[j]).to_f64();
            }
        }
        let limit = vtn_anorm_dbl(&col) * x(0.001);
        let rest = vtn_anorm_dbl(&c);
        if limit.gt(rest) {
            let mut s = F80::ONE;
            for &e in &d[..k] {
                s = x(e) * x(e) + s;
            }
            let r = F80::ONE / s;
            for i in 0..mu {
                let mut t = F80::ZERO;
                for j in 0..k {
                    t = x(get(m, out, j, i)) * d[j] + t;
                }
                kv[i] = (t * r).to_f64();
            }
        } else {
            vtn_inv_dbl(&c, &mut kv);
        }
        for i in 0..mu {
            for j in 0..k {
                let v = (get(m, out, j, i) - x(d[j]) * kv[i]).to_f64();
                set(m, out, j, i, v);
            }
            set(m, out, k, i, fmovd(kv[i]));
        }
    }
    for (b, v) in [(b_col, &col), (b_kv, &kv), (b_c, &c), (b_d, &d)] {
        for (i, e) in v.iter().enumerate() {
            m.set_f64(b + 8 * i as u32, *e);
        }
    }
    for b in [b_col, b_kv, b_c, b_d] {
        if b != 0 {
            m.free(b);
        }
    }
    1
}

// ---- extending a curve or a surface's U direction by the end or the beginning ----

/// The shared body of `nurbsCrvExtendEndKnot`, `nurbsCrvExtendBeginKnot` and the surface forms: after the knots are
/// extended, the last (`end`) or first `order` control points of every row of the direction (`groups` rows of
/// `npts` points, `4 * groups` matrix rows) are multiplied by the pseudo-inverse of the basis matrix of the
/// parameter `t`. The curve has one group at `pts` (curve +0x2c); the surface `+0x28` groups at +0x54.
fn extend_points(m: &mut Mem, rec: u32, pts: u32, npts: i32, groups: i32, order: i32, t: f64, end: bool) {
    let sp0 = m.sp();
    let rows = groups.wrapping_shl(2);
    let a = mat_alloc(m, rows, order);
    let b = mat_alloc(m, rows, order);
    let first = if end { npts.wrapping_sub(order) } else { 0 };
    let at = |g: i32, k: i32| pts.wrapping_add(((g.wrapping_mul(npts)).wrapping_add(first).wrapping_add(k) as u32).wrapping_mul(0x24));
    let mut g = 0;
    while g < groups {
        let mut k = 0;
        while k < order {
            for c in 0..4u32 {
                let v = m.f64(at(g, k).wrapping_add(8 * c));
                let dst = mat_at(m, a, 4 * g + c as i32, k);
                m.set_f64(dst, v);
            }
            k += 1;
        }
        g += 1;
    }
    let ident = mat_identity(m, order);
    let idx = m.local(4);
    let basis = nurbs_matrix_basis_function(m, rec, t, idx, if end { 0 } else { 1 });
    greville_inverse(m, basis, ident);
    mat_multiply(m, a, ident, b);
    let mut g = 0;
    while g < groups {
        let mut k = 0;
        while k < order {
            for c in 0..4u32 {
                let src = mat_at(m, b, 4 * g + c as i32, k);
                let v = m.f64(src);
                m.set_f64(at(g, k).wrapping_add(8 * c), v);
            }
            k += 1;
        }
        g += 1;
    }
    let (p0, p1, p2, p3) = (m.local(4), m.local(4), m.local(4), m.local(4));
    m.set_u32(p0, basis);
    m.set_u32(p1, ident);
    m.set_u32(p2, a);
    m.set_u32(p3, b);
    mat_dispose(m, p0);
    mat_dispose(m, p1);
    mat_dispose(m, p2);
    mat_dispose(m, p3);
    m.set_sp(sp0);
}

fn extend_record(m: &mut Mem, rec: u32, pts: u32, npts: i32, groups: i32, deltas: u32, count: i32, end: bool) {
    let order = m.i32(rec);
    let nk = m.i32(rec.wrapping_add(8));
    let knots = m.u32(rec.wrapping_add(0xc));
    let sp0 = m.sp();
    let countp = m.local(4);
    m.set_i32(countp, count);
    if end {
        extend_end_knot_from_deltas(m, knots, nk, deltas, countp);
    } else {
        extend_begin_knot_from_deltas(m, knots, nk, deltas, countp);
    }
    let kat = |i: i32| knots.wrapping_add((i as u32).wrapping_mul(8));
    let lo = m.f64(kat(order.wrapping_sub(2)));
    m.set_f64(rec.wrapping_add(0x10), lo);
    let hi = m.f64(kat(nk.wrapping_sub(order).wrapping_add(1)));
    m.set_f64(rec.wrapping_add(0x18), hi);
    let c = m.i32(countp);
    let t = if end { m.f64(kat(nk.wrapping_sub(c).wrapping_sub(1))) } else { m.f64(kat(c)) };
    extend_points(m, rec, pts, npts, groups, order, t, end);
    m.set_sp(sp0);
}

/// [soft 0x084cb168 nurbsCrvExtendEndKnot]: `(crv, deltas, count)`: the end knots are extended by the deltas
/// (`extendEndKnotFromDeltas`), the curve's range set from the knots, and the last `order` control points
/// multiplied by the pseudo-inverse of the basis matrix at `knots[nk - count - 1]`.
pub fn nurbs_crv_extend_end_knot(m: &mut Mem, crv: u32, deltas: u32, count: i32) {
    let (npts, pts) = (m.i32(crv.wrapping_add(4)), m.u32(crv.wrapping_add(0x2c)));
    extend_record(m, crv, pts, npts, 1, deltas, count, true);
}

/// [soft 0x084cb334 nurbsCrvExtendBeginKnot]: the same at the beginning: `extendBeginKnotFromDeltas`, the first
/// `order` control points, the basis matrix at `knots[count]` (`flag` 1).
pub fn nurbs_crv_extend_begin_knot(m: &mut Mem, crv: u32, deltas: u32, count: i32) {
    let (npts, pts) = (m.i32(crv.wrapping_add(4)), m.u32(crv.wrapping_add(0x2c)));
    extend_record(m, crv, pts, npts, 1, deltas, count, false);
}

/// [soft 0x084ccc98 nurbsSrfExtendEndKnot]: the surface's U direction, the matrix with `4 * nv` rows (`nv` the V
/// point count at +0x28), the points at +0x54, row `j` at `j * nu`.
pub fn nurbs_srf_extend_end_knot(m: &mut Mem, srf: u32, deltas: u32, count: i32) {
    let (npts, pts, nv) = (m.i32(srf.wrapping_add(4)), m.u32(srf.wrapping_add(0x54)), m.i32(srf.wrapping_add(0x28)));
    extend_record(m, srf, pts, npts, nv, deltas, count, true);
}

/// [soft 0x084ccfd0 nurbsSrfExtendBeginKnot]: the beginning of the surface's U direction.
pub fn nurbs_srf_extend_begin_knot(m: &mut Mem, srf: u32, deltas: u32, count: i32) {
    let (npts, pts, nv) = (m.i32(srf.wrapping_add(4)), m.u32(srf.wrapping_add(0x54)), m.i32(srf.wrapping_add(0x28)));
    extend_record(m, srf, pts, npts, nv, deltas, count, false);
}

fn free_nz(m: &mut Mem, p: u32) {
    if p != 0 {
        m.free(p);
    }
}

/// The shared tree of `si_nurbsCrvExtendKnot` and `si_nurbsSrfExtendKnot`: `end` / `begin` are the two extenders.
/// `swap_even` is the order of the two extreme values in the even case's second delta vector (the curve reverses
/// them, the surface does not).
fn extend_knot(
    m: &mut Mem,
    a: u32,
    b: u32,
    n: i32,
    p4: f64,
    p5: f64,
    p6: f64,
    end: fn(&mut Mem, u32, u32, i32),
    begin: fn(&mut Mem, u32, u32, i32),
    swap_even: bool,
) {
    if n == 2 {
        let d = build_delta_vector(m, 2, p5, 0.0);
        end(m, a, d, 1);
        free_nz(m, d);
        let d = build_delta_vector(m, 2, p5, 0.0);
        begin(m, b, d, 1);
        free_nz(m, d);
    } else if n == 3 {
        let d = build_delta_vector(m, 3, p4, p5);
        end(m, a, d, 2);
        free_nz(m, d);
        let d = build_delta_vector(m, 3, p5, p6);
        begin(m, b, d, 2);
        free_nz(m, d);
    } else if n & 1 == 0 {
        let d = build_delta_vector(m, n, p4, p6);
        end(m, a, d, n.wrapping_sub(1));
        free_nz(m, d);
        let d2 = if swap_even { build_delta_vector(m, n, p6, p4) } else { build_delta_vector(m, n, p4, p6) };
        let r = reverse_knot_seq(m, d2, n.wrapping_sub(1));
        begin(m, b, r, n.wrapping_sub(1));
        free_nz(m, r);
        free_nz(m, d2);
    } else if n != 1 {
        let d = build_delta_vector(m, n, p5, 0.0);
        end(m, a, d, n.wrapping_sub(1));
        free_nz(m, d);
        let d2 = build_delta_vector(m, n, p5, 0.0);
        let r = reverse_knot_seq(m, d2, n.wrapping_sub(1));
        begin(m, b, r, n.wrapping_sub(1));
        free_nz(m, r);
        free_nz(m, d2);
    }
}

/// [soft 0x084cac38 si_nurbsCrvExtendKnot]: `(a, b, n, p4, p5, p6)`: extends the end of `a` and the beginning of `b`
/// by the delta vectors `buildDeltaVector` makes of the three spans (`n` 2, 3, even, odd; 1 does nothing).
pub fn si_nurbs_crv_extend_knot(m: &mut Mem, a: u32, b: u32, n: i32, p4: f64, p5: f64, p6: f64) {
    extend_knot(m, a, b, n, p4, p5, p6, nurbs_crv_extend_end_knot, nurbs_crv_extend_begin_knot, true);
}

/// [soft 0x084cae78 si_nurbsSrfExtendKnot]: the same for surfaces; the even case's second delta vector keeps the
/// order `(p4, p6)` (the curve's has `(p6, p4)`).
pub fn si_nurbs_srf_extend_knot(m: &mut Mem, a: u32, b: u32, n: i32, p4: f64, p5: f64, p6: f64) {
    extend_knot(m, a, b, n, p4, p5, p6, nurbs_srf_extend_end_knot, nurbs_srf_extend_begin_knot, false);
}

// ---- merging and blending curves ----

fn copy9(m: &mut Mem, from: u32, to: u32) {
    for k in 0..9 {
        let v = m.u32(from.wrapping_add(4 * k));
        m.set_u32(to.wrapping_add(4 * k), v);
    }
}

/// The new curve record both `SI_nurbsCrvMerge` and `SI_nurbsCrvBlend` start: `order` of `a`, `npts` points,
/// `order - 2 + npts` knots (zeroed), `+0x20 = 0`, `+0x24 = 2`, `+0x28` 0 only when both inputs' +0x28 words are 0,
/// the points (`SI_nurbsPointsAllocate`) and `+0x34` the larger of the two inputs'.
fn new_joined_curve(m: &mut Mem, a: u32, b: u32, npts: i32) -> u32 {
    let out = nurbs_crv_allocate(m).addr();
    let order = m.i32(a);
    m.set_i32(out, order);
    m.set_i32(out.wrapping_add(4), npts);
    let nk = order.wrapping_sub(2).wrapping_add(npts);
    m.set_i32(out.wrapping_add(8), nk);
    let k = knots_allocate(m, nk);
    m.set_u32(out.wrapping_add(0xc), k);
    m.set_u16(out.wrapping_add(0x20), 0);
    m.set_i32(out.wrapping_add(0x24), 2);
    let flag = if m.i16(a.wrapping_add(0x28)) == 0 && m.i16(b.wrapping_add(0x28)) == 0 { 0 } else { 1 };
    m.set_u16(out.wrapping_add(0x28), flag);
    let p = nurbs_points_allocate(m, m.i32(out.wrapping_add(4)));
    m.set_u32(out.wrapping_add(0x2c), p);
    let (sb, sa) = (m.i32(b.wrapping_add(0x34)), m.i32(a.wrapping_add(0x34)));
    m.set_i32(out.wrapping_add(0x34), if sb < sa { sa } else { sb });
    out
}

fn set_range(m: &mut Mem, out: u32) {
    let k = m.u32(out.wrapping_add(0xc));
    let order = m.i32(out);
    let nk = m.i32(out.wrapping_add(8));
    let lo = m.f64(k.wrapping_add((order as u32).wrapping_mul(8)).wrapping_sub(0x10));
    m.set_f64(out.wrapping_add(0x10), lo);
    let hi = m.f64(k.wrapping_add((nk.wrapping_sub(order) as u32).wrapping_mul(8)).wrapping_add(8));
    m.set_f64(out.wrapping_add(0x18), hi);
}

fn crv_dispose_local(m: &mut Mem, c: u32) {
    let sp0 = m.sp();
    let slot = m.local(4);
    m.set_u32(slot, c);
    nurbs_crv_dispose(m, slot);
    m.set_sp(sp0);
}

/// [soft 0x084cb560 SI_nurbsCrvMerge]: `(a, b, p3, p4, p5)`: two copies are extended at the joint
/// ([`si_nurbs_crv_extend_knot`] with `order - 1`), then joined into a new curve of `npts(a) + npts(b)` points
/// whose knots `mergeKnotSequence` builds; the points of `a`, then those of `b`, copied; with `SI_gblIndex2`
/// the second part's new positions are written. The copies are disposed of; returns the new curve.
pub fn si_nurbs_crv_merge(m: &mut Mem, ca: u32, cb: u32, p3: f64, p4: f64, p5: f64) -> u32 {
    let a = crv_copy(m, ca);
    let b = crv_copy(m, cb);
    let order = m.i32(a);
    si_nurbs_crv_extend_knot(m, a, b, order.wrapping_sub(1), p3, p4, p5);
    let (na, nb) = (m.i32(a.wrapping_add(4)), m.i32(b.wrapping_add(4)));
    let out = new_joined_curve(m, a, b, nb.wrapping_add(na));
    merge_knot_sequence(m, a, b, out, order.wrapping_sub(1));
    set_range(m, out);
    let (pa, pb, po) = (m.u32(a.wrapping_add(0x2c)), m.u32(b.wrapping_add(0x2c)), m.u32(out.wrapping_add(0x2c)));
    let mut i = 0;
    while i < m.i32(a.wrapping_add(4)) {
        copy9(m, pa.wrapping_add(0x24 * i as u32), po.wrapping_add(0x24 * i as u32));
        i += 1;
    }
    let mut j = 0;
    while j < m.i32(b.wrapping_add(4)) {
        copy9(m, pb.wrapping_add(0x24 * j as u32), po.wrapping_add(((j.wrapping_add(m.i32(a.wrapping_add(4)))) as u32).wrapping_mul(0x24)));
        let g2 = m.u32(SI_GBL_INDEX2);
        if g2 != 0 {
            let v = m.i32(a.wrapping_add(4)).wrapping_add(j);
            m.set_i32(g2.wrapping_add(4 * j as u32), v);
        }
        j += 1;
    }
    crv_dispose_local(m, a);
    crv_dispose_local(m, b);
    out
}

/// [soft 0x084cb730 SI_nurbsCrvBlend]: `(a, b, p3, p4, p5)`: the end of a copy of `a` and the beginning of a copy
/// of `b` are extended by the two deltas `(p3, p5)` (`nurbsCrvExtendEndKnot` / `nurbsCrvExtendBeginKnot`, count
/// 2); the new curve has `npts(a) + npts(b) - 1` points; its knots are those of `a`, then extended by the spans
/// of `b`'s knots from the fourth on; the points of `a` but the last are copied, the joint point is the
/// weight-averaged blend (`p4` the blend factor: `(1 - p4)` of `a`'s last point and `p4` of `b`'s first, each
/// divided by its weight, scaled by half the weights' sum), and `b`'s other points follow. `SI_gblIndex` /
/// `SI_gblIndex2` receive -1 for the joint and the new positions.
pub fn si_nurbs_crv_blend(m: &mut Mem, ca: u32, cb: u32, p3: f64, p4: f64, p5: f64) -> u32 {
    let a = crv_copy(m, ca);
    let b = crv_copy(m, cb);
    let d = knots_allocate(m, 2);
    m.set_f64(d, p3);
    m.set_f64(d.wrapping_add(8), p5);
    nurbs_crv_extend_end_knot(m, a, d, 2);
    m.set_f64(d, p3);
    m.set_f64(d.wrapping_add(8), p5);
    nurbs_crv_extend_begin_knot(m, b, d, 2);
    free_nz(m, d);
    let (na, nb) = (m.i32(a.wrapping_add(4)), m.i32(b.wrapping_add(4)));
    let out = new_joined_curve(m, a, b, nb.wrapping_add(na).wrapping_sub(1));
    let (ka, kb, ko) = (m.u32(a.wrapping_add(0xc)), m.u32(b.wrapping_add(0xc)), m.u32(out.wrapping_add(0xc)));
    let mut i = 0;
    while i < m.i32(a.wrapping_add(8)) {
        let v = m.f64(ka.wrapping_add(8 * i as u32));
        m.set_f64(ko.wrapping_add(8 * i as u32), v);
        i += 1;
    }
    let mut c = 3i32;
    let mut i = 0;
    while i < m.i32(out.wrapping_add(8)).wrapping_sub(m.i32(a.wrapping_add(8))) {
        let hi = x(m.f64(kb.wrapping_add(8 * c as u32)));
        let lo = m.f64(kb.wrapping_add(8 * c as u32).wrapping_sub(8));
        let at = (i.wrapping_add(m.i32(a.wrapping_add(8))) as u32).wrapping_mul(8);
        let prev = m.f64(ko.wrapping_add(at).wrapping_sub(8));
        m.set_f64(ko.wrapping_add(at), ((hi - lo) + prev).to_f64());
        c = c.wrapping_add(1);
        i += 1;
    }
    set_range(m, out);
    let (pa, pb, po) = (m.u32(a.wrapping_add(0x2c)), m.u32(b.wrapping_add(0x2c)), m.u32(out.wrapping_add(0x2c)));
    let mut i = 0;
    while i < na.wrapping_sub(1) {
        copy9(m, pa.wrapping_add(0x24 * i as u32), po.wrapping_add(0x24 * i as u32));
        i += 1;
    }
    let g1 = m.u32(SI_GBL_INDEX);
    if g1 != 0 {
        m.set_i32(g1.wrapping_add(4 * i as u32), -1);
    }
    let g2 = m.u32(SI_GBL_INDEX2);
    if g2 != 0 {
        m.set_i32(g2, -1);
    }
    let o = 0x24u32.wrapping_mul(i as u32);
    let t = p4;
    let s = F80::ONE - x(t);
    let (aw, bw) = (m.f64(pa.wrapping_add(o).wrapping_add(0x18)), m.f64(pb.wrapping_add(0x18)));
    let q = |m: &Mem, base: u32, off: u32| x(m.f64(base.wrapping_add(off))) / x(m.f64(base.wrapping_add(0x18)));
    // the stack keeps the divisions unrounded; the three `fst` stores are not read back, the stack copies are
    let (ax, ay, az) = (q(m, pa.wrapping_add(o), 0), q(m, pa.wrapping_add(o), 8), q(m, pa.wrapping_add(o), 0x10));
    let (bx, by, bz) = (q(m, pb, 0), q(m, pb, 8), q(m, pb, 0x10));
    let bxt = bx * x(t);
    let byt = by * x(t);
    let bzt = bz * x(t);
    let xx = ax * s + bxt;
    let yy = (ay * s + byt).to_f64();
    let zz = (az * s + bzt).to_f64();
    let sum = x(aw) + x(bw);
    let half = x(0.5);
    m.set_f64(po.wrapping_add(o), ((sum * xx) * half).to_f64());
    m.set_f64(po.wrapping_add(o).wrapping_add(8), ((sum * x(yy)) * half).to_f64());
    m.set_f64(po.wrapping_add(o).wrapping_add(0x10), ((sum * x(zz)) * half).to_f64());
    m.set_f64(po.wrapping_add(o).wrapping_add(0x18), (half * sum).to_f64());
    let mut j = 1i32;
    while j < m.i32(b.wrapping_add(4)) {
        let dst = po.wrapping_add(((j.wrapping_add(na)) as u32).wrapping_mul(0x24)).wrapping_sub(0x24);
        copy9(m, pb.wrapping_add(0x24 * j as u32), dst);
        let g2 = m.u32(SI_GBL_INDEX2);
        if g2 != 0 {
            m.set_i32(g2.wrapping_add(4 * j as u32), j.wrapping_add(na).wrapping_sub(1));
        }
        j += 1;
    }
    crv_dispose_local(m, a);
    crv_dispose_local(m, b);
    out
}

/// [soft 0x084cbb0c SI_nurbsCrvMergeLinear]: `(a, b, p3)`: a new curve of `order(a)`, `nk(a) + nk(b)` knots and
/// `nk - order + 2` points: the knots of `a`, one more `p3` above its last, then the spans of `b`'s knots (from the
/// second) added on; the points of `a`, then the points between (the count the knot total leaves, each the
/// weight-averaged blend of `a`'s last and `b`'s first point at the parameter `k/(n+1)`, scaled by half the
/// weights' sum), then `b`'s; `SI_gblIndex2` receives the new positions of `b`'s points. The blend keeps the
/// divided components unrounded on the stack, but `a`'s second component enters through its stored double.
pub fn si_nurbs_crv_merge_linear(m: &mut Mem, a: u32, b: u32, p3: f64) -> u32 {
    let out = nurbs_crv_allocate(m).addr();
    let order = m.i32(a);
    m.set_i32(out, order);
    let nk = m.i32(b.wrapping_add(8)).wrapping_add(m.i32(a.wrapping_add(8)));
    m.set_i32(out.wrapping_add(8), nk);
    m.set_i32(out.wrapping_add(4), nk.wrapping_sub(order).wrapping_add(2));
    let k = knots_allocate(m, nk);
    m.set_u32(out.wrapping_add(0xc), k);
    m.set_u16(out.wrapping_add(0x20), 0);
    m.set_i32(out.wrapping_add(0x24), 2);
    let flag = if m.i16(a.wrapping_add(0x28)) == 0 && m.i16(b.wrapping_add(0x28)) == 0 { 0 } else { 1 };
    m.set_u16(out.wrapping_add(0x28), flag);
    let p = nurbs_points_allocate(m, m.i32(out.wrapping_add(4)));
    m.set_u32(out.wrapping_add(0x2c), p);
    let (sb, sa) = (m.i32(b.wrapping_add(0x34)), m.i32(a.wrapping_add(0x34)));
    m.set_i32(out.wrapping_add(0x34), if sb < sa { sa } else { sb });
    let (ka, kb) = (m.u32(a.wrapping_add(0xc)), m.u32(b.wrapping_add(0xc)));
    let mut i = 0i32;
    while i < m.i32(a.wrapping_add(8)) {
        let v = m.f64(ka.wrapping_add(8 * i as u32));
        m.set_f64(k.wrapping_add(8 * i as u32), v);
        i += 1;
    }
    let v = (x(m.f64(ka.wrapping_add(8 * i as u32).wrapping_sub(8))) + x(p3)).to_f64();
    m.set_f64(k.wrapping_add(8 * i as u32), v);
    let mut j = 1i32;
    while j < m.i32(b.wrapping_add(8)) {
        i = i.wrapping_add(1);
        let hi = x(m.f64(kb.wrapping_add(8 * j as u32)));
        let lo = m.f64(kb.wrapping_add(8 * j as u32).wrapping_sub(8));
        let prev = m.f64(k.wrapping_add(8 * i as u32).wrapping_sub(8));
        m.set_f64(k.wrapping_add(8 * i as u32), ((hi - lo) + prev).to_f64());
        j += 1;
    }
    set_range(m, out);
    let (pa, pb, po) = (m.u32(a.wrapping_add(0x2c)), m.u32(b.wrapping_add(0x2c)), m.u32(out.wrapping_add(0x2c)));
    let na = m.i32(a.wrapping_add(4));
    let mut c = 0i32;
    while c < na {
        copy9(m, pa.wrapping_add(0x24 * c as u32), po.wrapping_add(0x24 * c as u32));
        c += 1;
    }
    let cnt = m.i32(out.wrapping_add(8)).wrapping_sub(m.i32(out)).wrapping_sub(na.wrapping_sub(2)).wrapping_sub(m.i32(b.wrapping_add(4)));
    let d10 = (F80::ONE / F80::from_i32(cnt.wrapping_add(1))).to_f64();
    let mut t = d10;
    let mut left = cnt;
    if cnt > 0 {
        while left != 0 {
            let l = pa.wrapping_add(0x24u32.wrapping_mul(na as u32));
            let q = |base: u32, off: u32| x(m.f64(base.wrapping_add(off))) / x(m.f64(base.wrapping_add(0x18)));
            let (x1, y1, z1) = (q(l.wrapping_sub(0x24), 0), q(l.wrapping_sub(0x24), 8), q(l.wrapping_sub(0x24), 0x10));
            let aw = m.f64(l.wrapping_sub(0xc));
            let (x2, y2, z2) = (q(pb, 0), q(pb, 8), q(pb, 0x10));
            let bw = m.f64(pb.wrapping_add(0x18));
            let y1r = y1.to_f64();
            let tt = x(t);
            let s = F80::ONE - tt;
            let x1s = x1 * s;
            let sy = s * x(y1r);
            let z1s = z1 * s;
            let x2t = x2 * tt;
            let y2t = y2 * tt;
            let z2t = z2 * tt;
            let xs = x2t + x1s;
            let ysum = (y2t + sy).to_f64();
            let zsum = (z1s + z2t).to_f64();
            let sum = x(aw) + x(bw);
            let half = x(0.5);
            let o = po.wrapping_add(0x24u32.wrapping_mul(c as u32));
            m.set_f64(o, ((sum * xs) * half).to_f64());
            m.set_f64(o.wrapping_add(8), ((sum * x(ysum)) * half).to_f64());
            m.set_f64(o.wrapping_add(0x10), ((sum * x(zsum)) * half).to_f64());
            m.set_f64(o.wrapping_add(0x18), (sum * half).to_f64());
            t = (x(t) + x(d10)).to_f64();
            c += 1;
            left -= 1;
        }
    }
    let mut j = 0i32;
    while j < m.i32(b.wrapping_add(4)) {
        copy9(m, pb.wrapping_add(0x24 * j as u32), po.wrapping_add(0x24u32.wrapping_mul(c as u32)));
        let g2 = m.u32(SI_GBL_INDEX2);
        if g2 != 0 {
            m.set_i32(g2.wrapping_add(4 * j as u32), c);
        }
        j += 1;
        c += 1;
    }
    out
}

/// [soft 0x084cbe80 SI_nurbsCrvMergeJoin]: `(a, b)`: a copy of `b` is moved so that its first point (divided by its
/// weight) lies on `a`'s last: each point is divided by its weight, the difference of the two joint points added
/// (the differences are stored as doubles; the x component of the sum stays unrounded into the product), and
/// multiplied by the weight again; then `SI_nurbsCrvBlend(a, copy, 0, 0.5, 0)`, and the copy is disposed of.
pub fn si_nurbs_crv_merge_join(m: &mut Mem, a: u32, b: u32) -> u32 {
    let c = crv_copy(m, b);
    let la = m.u32(a.wrapping_add(0x2c)).wrapping_add(0x24u32.wrapping_mul(m.i32(a.wrapping_add(4)) as u32));
    let q = |m: &Mem, base: u32, off: u32| x(m.f64(base.wrapping_add(off))) / x(m.f64(base.wrapping_add(0x18)));
    let (ax, ay, az) = (q(m, la.wrapping_sub(0x24), 0), q(m, la.wrapping_sub(0x24), 8), q(m, la.wrapping_sub(0x24), 0x10));
    let pc = m.u32(c.wrapping_add(0x2c));
    let (bx, by, bz) = (q(m, pc, 0), q(m, pc, 8), q(m, pc, 0x10));
    let (dx, dy, dz) = ((ax - bx).to_f64(), (ay - by).to_f64(), (az - bz).to_f64());
    let mut i = 0i32;
    while i < m.i32(c.wrapping_add(4)) {
        let p = m.u32(c.wrapping_add(0x2c)).wrapping_add(0x24u32.wrapping_mul(i as u32));
        let (px, py, pz) = (q(m, p, 0), q(m, p, 8), q(m, p, 0x10));
        let w = x(m.f64(p.wrapping_add(0x18)));
        let pxd = px + x(dx);
        let pyd = (py + x(dy)).to_f64();
        let pzd = (pz + x(dz)).to_f64();
        m.set_f64(p, (pxd * w).to_f64());
        m.set_f64(p.wrapping_add(8), (x(pyd) * w).to_f64());
        m.set_f64(p.wrapping_add(0x10), (x(pzd) * w).to_f64());
        i += 1;
    }
    let out = si_nurbs_crv_blend(m, a, c, 0.0, 0.5, 0.0);
    crv_dispose_local(m, c);
    out
}

// ---- merging and blending surfaces ----

fn mov64(m: &mut Mem, from: u32, to: u32) {
    for k in [0u32, 4] {
        let v = m.u32(from.wrapping_add(k));
        m.set_u32(to.wrapping_add(k), v);
    }
}

fn srf_dispose_local(m: &mut Mem, o: &mut dyn Outside, s: u32) {
    let sp0 = m.sp();
    let slot = m.local(4);
    m.set_u32(slot, s);
    srf_dispose(m, o, slot);
    m.set_sp(sp0);
}

/// The U direction's range of a record whose knot array is built: `k[order - 2]` and `k[nk - order + 1]`.
fn srf_range_u(m: &mut Mem, out: u32) {
    set_range(m, out);
}

/// The V direction's range: for an open direction (+0x44 is 0) `k[order - 2]` and `k[nk - order + 1]`, for a
/// closed one the first and last knot.
fn srf_range_v(m: &mut Mem, out: u32) {
    let k = m.u32(out.wrapping_add(0x30));
    let (order, nk) = (m.i32(out.wrapping_add(0x24)), m.i32(out.wrapping_add(0x2c)));
    if m.i16(out.wrapping_add(0x44)) == 0 {
        mov64(m, k.wrapping_add((order as u32).wrapping_mul(8)).wrapping_sub(0x10), out.wrapping_add(0x34));
        mov64(m, k.wrapping_add((nk.wrapping_sub(order) as u32).wrapping_mul(8)).wrapping_add(8), out.wrapping_add(0x3c));
    } else {
        mov64(m, k, out.wrapping_add(0x34));
        mov64(m, k.wrapping_add((nk.wrapping_sub(1) as u32).wrapping_mul(8)), out.wrapping_add(0x3c));
    }
}

fn copy_v_knots(m: &mut Mem, a: u32, out: u32) {
    let (ka, ko) = (m.u32(a.wrapping_add(0x30)), m.u32(out.wrapping_add(0x30)));
    let mut i = 0i32;
    while i < m.i32(a.wrapping_add(0x2c)) {
        mov64(m, ka.wrapping_add(8 * i as u32), ko.wrapping_add(8 * i as u32));
        i += 1;
    }
}

/// The words every surface join sets after the points are allocated: the points' flag word (+0x50, 0 only when
/// both inputs' are 0), the allocated points and the four larger-of-the-two step counts (+0x70 to +0x7c).
fn srf_join_tail(m: &mut Mem, a: u32, b: u32, out: u32) {
    let flag = if m.i16(a.wrapping_add(0x50)) == 0 && m.i16(b.wrapping_add(0x50)) == 0 { 0 } else { 1 };
    m.set_u16(out.wrapping_add(0x50), flag);
    let p = nurbs_points_allocate(m, m.i32(out.wrapping_add(0x48)));
    m.set_u32(out.wrapping_add(0x54), p);
    for off in [0x70u32, 0x74, 0x78, 0x7c] {
        let (vb, va) = (m.i32(b.wrapping_add(off)), m.i32(a.wrapping_add(off)));
        m.set_i32(out.wrapping_add(off), if vb < va { va } else { vb });
    }
}

/// [soft 0x084cc920 SI_nurbsSrfMerge]: `(a, b, p3, p4, p5)`: copies of both are extended at the joint in U
/// ([`si_nurbs_srf_extend_knot`], `order - 1`) and joined into a new surface of `nu(a) + nu(b)` columns and
/// `nv(a)` rows (V direction, order, closed word and step counts from `a`); `mergeKnotSequence` builds the U knots,
/// the V knots are `a`'s; each row is the row of `a`, then that of `b` (`SI_showActivity` before every point);
/// `SI_gblIndex` gets `a`'s points' new positions, `SI_gblIndex2` `b`'s. The copies are disposed of.
pub fn si_nurbs_srf_merge(m: &mut Mem, o: &mut dyn Outside, sa: u32, sb: u32, p3: f64, p4: f64, p5: f64) -> u32 {
    let a = srf_copy(m, o, sa);
    let b = srf_copy(m, o, sb);
    let order = m.i32(a);
    si_nurbs_srf_extend_knot(m, a, b, order.wrapping_sub(1), p3, p4, p5);
    let out = nurbs_srf_allocate(m).addr();
    let o1 = m.i32(a);
    m.set_i32(out, o1);
    let ov = m.i32(a.wrapping_add(0x24));
    m.set_i32(out.wrapping_add(0x24), ov);
    let nu = m.i32(b.wrapping_add(4)).wrapping_add(m.i32(a.wrapping_add(4)));
    m.set_i32(out.wrapping_add(4), nu);
    let nv = m.i32(a.wrapping_add(0x28));
    m.set_i32(out.wrapping_add(0x28), nv);
    m.set_i32(out.wrapping_add(0x48), nv.wrapping_mul(nu));
    let nk = o1.wrapping_sub(2).wrapping_add(nu);
    m.set_i32(out.wrapping_add(8), nk);
    let nkv = m.i32(a.wrapping_add(0x2c));
    m.set_i32(out.wrapping_add(0x2c), nkv);
    let k = knots_allocate(m, nk);
    m.set_u32(out.wrapping_add(0xc), k);
    let kv = knots_allocate(m, m.i32(out.wrapping_add(0x2c)));
    m.set_u32(out.wrapping_add(0x30), kv);
    m.set_u16(out.wrapping_add(0x20), 0);
    let cl = m.u16(a.wrapping_add(0x44));
    m.set_u16(out.wrapping_add(0x44), cl);
    m.set_i32(out.wrapping_add(0x4c), 2);
    srf_join_tail(m, a, b, out);
    merge_knot_sequence(m, a, b, out, order.wrapping_sub(1));
    srf_range_u(m, out);
    copy_v_knots(m, a, out);
    srf_range_v(m, out);
    let (pa, pb, po) = (m.u32(a.wrapping_add(0x54)), m.u32(b.wrapping_add(0x54)), m.u32(out.wrapping_add(0x54)));
    let (nua, nub, nuo) = (m.i32(a.wrapping_add(4)), m.i32(b.wrapping_add(4)), m.i32(out.wrapping_add(4)));
    let mut r = 0i32;
    while r < m.i32(out.wrapping_add(0x28)) {
        let mut c = 0i32;
        while c < nua {
            o.call(m, SI_SHOW_ACTIVITY, &[]);
            copy9(m, pa.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nua).wrapping_add(c) as u32)), po.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nuo).wrapping_add(c) as u32)));
            let g = m.u32(SI_GBL_INDEX);
            if g != 0 {
                m.set_i32(g.wrapping_add(4u32.wrapping_mul(r.wrapping_mul(nua).wrapping_add(c) as u32)), r.wrapping_mul(nuo).wrapping_add(c));
            }
            c += 1;
        }
        c = 0;
        while c < nub {
            o.call(m, SI_SHOW_ACTIVITY, &[]);
            copy9(m, pb.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nub).wrapping_add(c) as u32)), po.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nuo).wrapping_add(c).wrapping_add(nua) as u32)));
            let g = m.u32(SI_GBL_INDEX2);
            if g != 0 {
                m.set_i32(g.wrapping_add(4u32.wrapping_mul(r.wrapping_mul(nub).wrapping_add(c) as u32)), r.wrapping_mul(nuo).wrapping_add(nua).wrapping_add(c));
            }
            c += 1;
        }
        r += 1;
    }
    srf_dispose_local(m, o, a);
    srf_dispose_local(m, o, b);
    out
}

/// [soft 0x084cd2a0 SI_nurbsSrfBlend]: `(a, b, p3, p4, p5)`: the surface form of [`si_nurbs_crv_blend`]: copies are
/// extended with the deltas `(p3, p5)` and joined into `nu(a) + nu(b) - 1` columns; the U knots are `a`'s, then
/// extended by the spans of `b`'s from the fourth on; the V knots `a`'s. Every row's joint point (the column
/// `nu(a) - 1`) is the weight-averaged blend of `a`'s last and `b`'s first point of the row, with the factor `p4`
/// (`1 - p4` rounded to a double) and half the weights' sum; the other points are copied. `SI_gblIndex` receives
/// the new positions of `a`'s points but the last of each row, `SI_gblIndex2` those of `b`'s from the second on
/// (the joint is not written to either); `SI_showActivity` once per row of the last pass.
pub fn si_nurbs_srf_blend(m: &mut Mem, o: &mut dyn Outside, sa: u32, sb: u32, p3: f64, p4: f64, p5: f64) -> u32 {
    let a = srf_copy(m, o, sa);
    let b = srf_copy(m, o, sb);
    let d = knots_allocate(m, 2);
    m.set_f64(d, p3);
    m.set_f64(d.wrapping_add(8), p5);
    nurbs_srf_extend_end_knot(m, a, d, 2);
    m.set_f64(d, p3);
    m.set_f64(d.wrapping_add(8), p5);
    nurbs_srf_extend_begin_knot(m, b, d, 2);
    free_nz(m, d);
    let out = nurbs_srf_allocate(m).addr();
    let o1 = m.i32(a);
    m.set_i32(out, o1);
    let ov = m.i32(a.wrapping_add(0x24));
    m.set_i32(out.wrapping_add(0x24), ov);
    let nua = m.i32(a.wrapping_add(4));
    let nu = m.i32(b.wrapping_add(4)).wrapping_add(nua).wrapping_sub(1);
    m.set_i32(out.wrapping_add(4), nu);
    let nv = m.i32(a.wrapping_add(0x28));
    m.set_i32(out.wrapping_add(0x28), nv);
    m.set_i32(out.wrapping_add(0x48), nv.wrapping_mul(nu));
    let nk = o1.wrapping_sub(2).wrapping_add(nu);
    m.set_i32(out.wrapping_add(8), nk);
    let k = knots_allocate(m, nk);
    m.set_u32(out.wrapping_add(0xc), k);
    let nkv = m.i32(a.wrapping_add(0x2c));
    m.set_i32(out.wrapping_add(0x2c), nkv);
    let kv = knots_allocate(m, nkv);
    m.set_u32(out.wrapping_add(0x30), kv);
    m.set_u16(out.wrapping_add(0x20), 0);
    let cl = m.u16(a.wrapping_add(0x44));
    m.set_u16(out.wrapping_add(0x44), cl);
    m.set_i32(out.wrapping_add(0x4c), 2);
    srf_join_tail(m, a, b, out);
    let (ka, kb) = (m.u32(a.wrapping_add(0xc)), m.u32(b.wrapping_add(0xc)));
    let mut i = 0i32;
    while i < m.i32(a.wrapping_add(8)) {
        mov64(m, ka.wrapping_add(8 * i as u32), k.wrapping_add(8 * i as u32));
        i += 1;
    }
    let mut c = 3i32;
    let mut i = 0i32;
    while i < m.i32(out.wrapping_add(8)).wrapping_sub(m.i32(a.wrapping_add(8))) {
        let hi = x(m.f64(kb.wrapping_add(8 * c as u32)));
        let lo = m.f64(kb.wrapping_add(8 * c as u32).wrapping_sub(8));
        let at = (i.wrapping_add(m.i32(a.wrapping_add(8))) as u32).wrapping_mul(8);
        let prev = m.f64(k.wrapping_add(at).wrapping_sub(8));
        m.set_f64(k.wrapping_add(at), ((hi - lo) + prev).to_f64());
        c = c.wrapping_add(1);
        i += 1;
    }
    srf_range_u(m, out);
    copy_v_knots(m, a, out);
    srf_range_v(m, out);
    let (pa, pb, po) = (m.u32(a.wrapping_add(0x54)), m.u32(b.wrapping_add(0x54)), m.u32(out.wrapping_add(0x54)));
    let nub = m.i32(b.wrapping_add(4));
    let mut col = 0i32;
    let mut r = 0i32;
    while r < nv {
        col = 0;
        while col < nua.wrapping_sub(1) {
            copy9(m, pa.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nua).wrapping_add(col) as u32)), po.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nu).wrapping_add(col) as u32)));
            let g = m.u32(SI_GBL_INDEX);
            if g != 0 {
                m.set_i32(g.wrapping_add(4u32.wrapping_mul(r.wrapping_mul(nua).wrapping_add(col) as u32)), r.wrapping_mul(nu).wrapping_add(col));
            }
            col += 1;
        }
        r += 1;
    }
    // the joint column is the stale column counter of the loop above
    let sr = (F80::ONE - x(p4)).to_f64();
    let mut r = 0i32;
    while r < nv {
        let pta = pa.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nua).wrapping_add(col) as u32));
        let ptb = pb.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nub) as u32));
        let q = |m: &Mem, base: u32, off: u32| x(m.f64(base.wrapping_add(off))) / x(m.f64(base.wrapping_add(0x18)));
        let (ax, ay, az) = (q(m, pta, 0), q(m, pta, 8), q(m, pta, 0x10));
        let (bx, by, bz) = (q(m, ptb, 0), q(m, ptb, 8), q(m, ptb, 0x10));
        let s = x(sr);
        let tt = x(p4);
        let xj = ax * s + bx * tt;
        let yj = (ay * s + by * tt).to_f64();
        let zj = (az * s + bz * tt).to_f64();
        let sum = x(m.f64(pta.wrapping_add(0x18))) + x(m.f64(ptb.wrapping_add(0x18)));
        let half = x(0.5);
        let dst = po.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nu).wrapping_add(col) as u32));
        m.set_f64(dst, ((sum * xj) * half).to_f64());
        m.set_f64(dst.wrapping_add(8), ((sum * x(yj)) * half).to_f64());
        m.set_f64(dst.wrapping_add(0x10), ((sum * x(zj)) * half).to_f64());
        m.set_f64(dst.wrapping_add(0x18), (sum * half).to_f64());
        r += 1;
    }
    let mut r = 0i32;
    while r < m.i32(out.wrapping_add(0x28)) {
        o.call(m, SI_SHOW_ACTIVITY, &[]);
        let mut j = 1i32;
        while j < nub {
            let dst = j.wrapping_add(nua).wrapping_add(r.wrapping_mul(nu)).wrapping_sub(1);
            copy9(m, pb.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nub).wrapping_add(j) as u32)), po.wrapping_add(0x24u32.wrapping_mul(dst as u32)));
            let g = m.u32(SI_GBL_INDEX2);
            if g != 0 {
                m.set_i32(g.wrapping_add(4u32.wrapping_mul(r.wrapping_mul(nub).wrapping_add(j) as u32)), dst);
            }
            j += 1;
        }
        r += 1;
    }
    srf_dispose_local(m, o, a);
    srf_dispose_local(m, o, b);
    out
}

/// The interpolated point [`si_nurbs_crv_merge_linear`] and [`si_nurbs_srf_merge_linear`] write between the last
/// point of `a` and the first of `b` at the parameter `t`: the components divided by their weight stay unrounded
/// except `a`'s second (which enters through its stored double), `s = 1 - t` unrounded; the second and third
/// results are rounded to doubles before the product with the weights' sum and 0.5.
fn linear_point(m: &mut Mem, la: u32, pb: u32, t: f64, dst: u32) {
    let q = |m: &Mem, base: u32, off: u32| x(m.f64(base.wrapping_add(off))) / x(m.f64(base.wrapping_add(0x18)));
    let (x1, y1, z1) = (q(m, la, 0), q(m, la, 8), q(m, la, 0x10));
    let aw = m.f64(la.wrapping_add(0x18));
    let (x2, y2, z2) = (q(m, pb, 0), q(m, pb, 8), q(m, pb, 0x10));
    let bw = m.f64(pb.wrapping_add(0x18));
    let y1r = y1.to_f64();
    let tt = x(t);
    let s = F80::ONE - tt;
    let xs = (x2 * tt) + (x1 * s);
    let ysum = ((y2 * tt) + (s * x(y1r))).to_f64();
    let zsum = ((z1 * s) + (z2 * tt)).to_f64();
    let sum = x(aw) + x(bw);
    let half = x(0.5);
    m.set_f64(dst, ((sum * xs) * half).to_f64());
    m.set_f64(dst.wrapping_add(8), ((sum * x(ysum)) * half).to_f64());
    m.set_f64(dst.wrapping_add(0x10), ((sum * x(zsum)) * half).to_f64());
    m.set_f64(dst.wrapping_add(0x18), (sum * half).to_f64());
}

/// [soft 0x084cd9b8 SI_nurbsSrfMergeLinear]: `(a, b, p3)`: the surface form of [`si_nurbs_crv_merge_linear`]: the
/// U knots are `a`'s, one more `p3` above the last, then the spans of `b`'s from the second; the V direction is
/// `a`'s. Per row (`SI_showActivity` before each pass): the row of `a`, then the interpolated points between its
/// last point and the first of `b`'s row (the same count for every row, the parameter restarting at `1/(n+1)`),
/// then the row of `b`; `SI_gblIndex` and `SI_gblIndex2` get the new positions.
pub fn si_nurbs_srf_merge_linear(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, p3: f64) -> u32 {
    let out = nurbs_srf_allocate(m).addr();
    let o1 = m.i32(a);
    m.set_i32(out, o1);
    let ov = m.i32(a.wrapping_add(0x24));
    m.set_i32(out.wrapping_add(0x24), ov);
    let nk = m.i32(b.wrapping_add(8)).wrapping_add(m.i32(a.wrapping_add(8)));
    m.set_i32(out.wrapping_add(8), nk);
    let k = knots_allocate(m, nk);
    m.set_u32(out.wrapping_add(0xc), k);
    let nkv = m.i32(a.wrapping_add(0x2c));
    m.set_i32(out.wrapping_add(0x2c), nkv);
    let kv = knots_allocate(m, nkv);
    m.set_u32(out.wrapping_add(0x30), kv);
    let nu = nk.wrapping_sub(o1).wrapping_add(2);
    m.set_i32(out.wrapping_add(4), nu);
    let nv = m.i32(a.wrapping_add(0x28));
    m.set_i32(out.wrapping_add(0x28), nv);
    m.set_i32(out.wrapping_add(0x48), nu.wrapping_mul(nv));
    m.set_u16(out.wrapping_add(0x20), 0);
    let cl = m.u16(a.wrapping_add(0x44));
    m.set_u16(out.wrapping_add(0x44), cl);
    m.set_i32(out.wrapping_add(0x4c), 2);
    srf_join_tail(m, a, b, out);
    let (ka, kb) = (m.u32(a.wrapping_add(0xc)), m.u32(b.wrapping_add(0xc)));
    let mut i = 0i32;
    while i < m.i32(a.wrapping_add(8)) {
        mov64(m, ka.wrapping_add(8 * i as u32), k.wrapping_add(8 * i as u32));
        i += 1;
    }
    let v = (x(m.f64(ka.wrapping_add(8 * i as u32).wrapping_sub(8))) + x(p3)).to_f64();
    m.set_f64(k.wrapping_add(8 * i as u32), v);
    let mut j = 1i32;
    while j < m.i32(b.wrapping_add(8)) {
        i = i.wrapping_add(1);
        let hi = x(m.f64(kb.wrapping_add(8 * j as u32)));
        let lo = m.f64(kb.wrapping_add(8 * j as u32).wrapping_sub(8));
        let prev = m.f64(k.wrapping_add(8 * i as u32).wrapping_sub(8));
        m.set_f64(k.wrapping_add(8 * i as u32), ((hi - lo) + prev).to_f64());
        j += 1;
    }
    srf_range_u(m, out);
    copy_v_knots(m, a, out);
    srf_range_v(m, out);
    let (pa, pb, po) = (m.u32(a.wrapping_add(0x54)), m.u32(b.wrapping_add(0x54)), m.u32(out.wrapping_add(0x54)));
    let (nua, nub) = (m.i32(a.wrapping_add(4)), m.i32(b.wrapping_add(4)));
    let nuo = m.i32(out.wrapping_add(4));
    let mut r = 0i32;
    while r < nv {
        o.call(m, SI_SHOW_ACTIVITY, &[]);
        let mut c = 0i32;
        while c < nua {
            copy9(m, pa.wrapping_add(0x24u32.wrapping_mul(c.wrapping_add(nua.wrapping_mul(r)) as u32)), po.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nuo).wrapping_add(c) as u32)));
            let g = m.u32(SI_GBL_INDEX);
            if g != 0 {
                m.set_i32(g.wrapping_add(4u32.wrapping_mul(r.wrapping_mul(nua).wrapping_add(c) as u32)), r.wrapping_mul(nuo).wrapping_add(c));
            }
            c += 1;
        }
        r += 1;
    }
    let cnt = m.i32(out.wrapping_add(8)).wrapping_sub(m.i32(out)).wrapping_sub(nua.wrapping_sub(2)).wrapping_sub(nub);
    let mut r = 0i32;
    while r < nv {
        o.call(m, SI_SHOW_ACTIVITY, &[]);
        let d9 = (F80::ONE / F80::from_i32(cnt.wrapping_add(1))).to_f64();
        let mut t = d9;
        let mut c = 0i32;
        while c < cnt {
            let la = pa.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nua).wrapping_add(nua) as u32)).wrapping_sub(0x24);
            let pbr = pb.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nub) as u32));
            let dst = po.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nuo).wrapping_add(nua).wrapping_add(c) as u32));
            linear_point(m, la, pbr, t, dst);
            t = (x(t) + x(d9)).to_f64();
            c += 1;
        }
        r += 1;
    }
    let mut r = 0i32;
    while r < m.i32(out.wrapping_add(0x28)) {
        o.call(m, SI_SHOW_ACTIVITY, &[]);
        let mut c = 0i32;
        while c < nub {
            let dst = r.wrapping_mul(nuo).wrapping_add(cnt).wrapping_add(nua).wrapping_add(c);
            copy9(m, pb.wrapping_add(0x24u32.wrapping_mul(c.wrapping_add(nub.wrapping_mul(r)) as u32)), po.wrapping_add(0x24u32.wrapping_mul(dst as u32)));
            let g = m.u32(SI_GBL_INDEX2);
            if g != 0 {
                m.set_i32(g.wrapping_add(4u32.wrapping_mul(r.wrapping_mul(nub).wrapping_add(c) as u32)), dst);
            }
            c += 1;
        }
        r += 1;
    }
    out
}

/// [soft 0x084ce028 SI_nurbsSrfMergeJoin]: `(a, b)`: the surface form of [`si_nurbs_crv_merge_join`]: a copy of
/// `b` is moved row by row (`SI_showActivity` per row) so that the first point of each of its rows lies on the
/// last point of the same row of `a`; then `SI_nurbsSrfBlend(a, copy, 0, 0.5, 0)`, and the copy is disposed of.
pub fn si_nurbs_srf_merge_join(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32) -> u32 {
    let c = srf_copy(m, o, b);
    let q = |m: &Mem, base: u32, off: u32| x(m.f64(base.wrapping_add(off))) / x(m.f64(base.wrapping_add(0x18)));
    let mut r = 0i32;
    while r < m.i32(a.wrapping_add(0x28)) {
        o.call(m, SI_SHOW_ACTIVITY, &[]);
        let nua = m.i32(a.wrapping_add(4));
        let la = m.u32(a.wrapping_add(0x54)).wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nua).wrapping_add(nua) as u32));
        let (ax, ay, az) = (q(m, la.wrapping_sub(0x24), 0), q(m, la.wrapping_sub(0x24), 8), q(m, la.wrapping_sub(0x24), 0x10));
        let nuc = m.i32(c.wrapping_add(4));
        let pc = m.u32(c.wrapping_add(0x54));
        let first = pc.wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(nuc) as u32));
        let (bx, by, bz) = (q(m, first, 0), q(m, first, 8), q(m, first, 0x10));
        let (dx, dy, dz) = ((ax - bx).to_f64(), (ay - by).to_f64(), (az - bz).to_f64());
        let mut i = 0i32;
        while i < m.i32(c.wrapping_add(4)) {
            let p = m.u32(c.wrapping_add(0x54)).wrapping_add(0x24u32.wrapping_mul(r.wrapping_mul(m.i32(c.wrapping_add(4))).wrapping_add(i) as u32));
            let (px, py, pz) = (q(m, p, 0), q(m, p, 8), q(m, p, 0x10));
            let w = x(m.f64(p.wrapping_add(0x18)));
            let pxd = px + x(dx);
            let pyd = (py + x(dy)).to_f64();
            let pzd = (pz + x(dz)).to_f64();
            m.set_f64(p, (pxd * w).to_f64());
            m.set_f64(p.wrapping_add(8), (x(pyd) * w).to_f64());
            m.set_f64(p.wrapping_add(0x10), (x(pzd) * w).to_f64());
            i += 1;
        }
        r += 1;
    }
    let out = si_nurbs_srf_blend(m, o, a, c, 0.0, 0.5, 0.0);
    srf_dispose_local(m, o, c);
    out
}
