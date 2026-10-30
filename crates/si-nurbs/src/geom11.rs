// SPDX-License-Identifier: MIT
//! nurbsgeom11: `fitNurbsPoints` (`curveFit.c`) on the memory image (finding 0025, section 31).

use crate::fit::{
    adjust_open_data, compute_nurbs_evaluation_matrix, compute_normal_equations, free_segment_list, mult_vec_by_eval_mat,
    mult_vec_by_nrml_eqns, point_vec_gaxpy, point_vec_inner_product, point_vec_subtract_xyz, SI_SHOW_ACTIVITY,
};
use crate::fit2::{allocate_fixed_data, calculate_error, mat_dispose, mult_vec_by_eval_mat_trans, update_knot_vector};
use crate::geom9::update_nurbs_evaluation_matrix;
use si_mem::{Mem, Outside};
use si_x87::{x, F80};

/// The double 1e-4 at 0x08661630 (moment threshold scale).
const C_SCALE: f64 = f64::from_bits(0x3f1a_36e2_eb1c_432d);
/// The double 1e-12 at 0x08661638 (smallest previous moment that gives a ratio).
const C_TINY: f64 = f64::from_bits(0x3d71_9799_812d_ea11);
/// The double 0.95 at 0x08661640.
const C_RATIO: f64 = f64::from_bits(0x3fee_6666_6666_6666);
/// The double 1e-6 at 0x08661648.
const C_EPS: f64 = f64::from_bits(0x3eb0_c6f7_a0b5_ed8d);

fn free_clear(m: &mut Mem, slot: u32) {
    let v = m.u32(slot);
    if v != 0 {
        m.free(v);
        m.set_u32(slot, 0);
    }
}

fn free_knots_and_dir(m: &mut Mem, ac: u32) {
    let k = m.u32(ac.wrapping_add(0xc));
    if k != 0 {
        m.free(k);
        m.set_u32(ac.wrapping_add(0xc), 0);
    }
    m.free(ac);
}

/// [soft 0x084d081c fitNurbsPoints]: `(dir, tol, n, pts, params)` (the tolerance is a double by value): fits one piece of
/// a curve through the `n` sample points `pts` (0x24 bytes each; `params` are their parameters) with the direction
/// record `dir` (order at +0, point count +4, knot count +8, knots +0xc, closed flag +0x20) as the starting knot
/// vector, and returns a new `SI_calloc` block of control points (its count is written back at `dir` + 4), or 0.
/// With `n == 2` the answer is the two points; with `n` less than the order the fit runs with a copy of the direction record
/// of order `n` and its end knots cut, and the answer is blended back up to the full order afterwards (de Boor-like
/// blend with weights `j / (order + i)`); an open curve that already has too many control points is cut back to
/// `2 (order - 1)` knots in place (this changes `dir`'s own knots when no copy was made, as the original does). The
/// loop that follows builds the evaluation matrix once and updates it after each knot insertion, forms the normal
/// equations, and solves them by conjugate gradients on the three coordinates at once (thresholds 1e-4 of the first
/// moments, a break when all three moment ratios exceed 0.95, step sizes guarded by 1e-6); `calculateError` and
/// `updateKnotVector` then decide whether more knots go in. Where the original passes the z ratio twice to the second
/// `SI_NurbsPointVecGaxpy` (the y ratio is dropped from the x87 stack by the ratio test), the port does the same.
/// The register values (ratios, the z threshold of the first test) stay at 80 bits.
pub fn fit_nurbs_points(m: &mut Mem, o: &mut dyn Outside, dir: u32, tol: f64, n: i32, pts: u32, params: u32) -> u32 {
    let sp = m.sp();
    let r = fit_body(m, o, dir, tol, n, pts, params);
    m.set_sp(sp);
    r
}

fn fit_body(m: &mut Mem, o: &mut dyn Outside, dir: u32, tol: f64, n: i32, pts: u32, params: u32) -> u32 {
    // the stack cells whose addresses go to callees
    let e4c = m.local(4);
    let e48 = m.local(4);
    let e44 = m.local(4);
    let e40 = m.local(4);
    let e3c = m.local(4);
    let e38 = m.local(4);
    let e34 = m.local(4);
    let e30 = m.local(4);
    let l88 = m.local(4);
    let l8c = m.local(4);
    let l84 = m.local(4);
    let l68 = m.local(8);
    let l60 = m.local(8);
    let l58 = m.local(8);
    let l80 = m.local(8);
    let l78 = m.local(8);
    let l70 = m.local(8);
    let zp = m.local(0x24);
    for w in 0..9 {
        m.set_u32(zp + 4 * w, 0);
    }
    m.set_u32(zp + 0x1c, 0x3ff0_0000);
    m.set_u32(e44, 0);
    m.set_u32(e40, 0);
    m.set_u32(l84, 0);
    // (e4c is left as the stack gave it, as in the original)

    let closed = m.i16(dir.wrapping_add(0x20)) as i32;
    let mut mm = n; // [ebp-0x94]
    if closed == 0 {
        mm = n.wrapping_sub(2);
    }
    let mut order = m.i32(dir); // [ebp-0x98]
    let mut res = 0u32; // [ebp-0xa4]
    let mut ac = 0u32; // [ebp-0xa8]
    let mut ok = true;

    if n == 2 {
        res = m.si_calloc(2, 0x24);
        if res == 0 {
            return 0;
        }
        order = 2;
        m.copy(res, pts, 0x24);
        m.copy(res.wrapping_add(0x24), pts.wrapping_add(0x24), 0x24);
    } else {
        if n < order {
            ac = m.si_calloc(1, 0x24);
            if ac == 0 {
                return 0;
            }
            let nk = m.i32(dir.wrapping_add(8)).wrapping_sub(order.wrapping_sub(n).wrapping_mul(2));
            m.set_i32(ac.wrapping_add(8), nk);
            let kn = m.si_calloc(nk as u32, 8);
            m.set_u32(ac.wrapping_add(0xc), kn);
            if kn == 0 {
                m.free(ac);
                return 0;
            }
            let src = m.u32(dir.wrapping_add(0xc)).wrapping_add(8u32.wrapping_mul(order.wrapping_sub(n) as u32));
            let mut i = 0i32;
            while i < m.i32(ac.wrapping_add(8)) {
                let v = m.f64(src.wrapping_add(8 * i as u32));
                m.set_f64(kn.wrapping_add(8 * i as u32), v);
                i += 1;
            }
            order = n;
            m.set_i32(ac, n);
        } else {
            ac = dir;
        }
        let nk = m.i32(ac.wrapping_add(8));
        let nc = if closed == 0 { nk.wrapping_add(2).wrapping_sub(order) } else { nk.wrapping_sub(1) };
        m.set_i32(ac.wrapping_add(4), nc);
        let a0: u32;
        if closed == 0 {
            if nc.wrapping_sub(2) > mm {
                let kn = m.u32(ac.wrapping_add(0xc));
                let mut i = 1i32;
                while i < order {
                    let v = m.f64(kn.wrapping_add(8u32.wrapping_mul(nk.wrapping_sub(order).wrapping_add(1) as u32)));
                    m.set_f64(kn.wrapping_add(8u32.wrapping_mul(i.wrapping_add(order).wrapping_sub(2) as u32)), v);
                    i += 1;
                }
                let nk2 = order.wrapping_sub(1).wrapping_mul(2);
                m.set_i32(ac.wrapping_add(8), nk2);
                m.set_i32(ac.wrapping_add(4), nk2.wrapping_sub(order).wrapping_add(2));
            }
            a0 = m.si_calloc(mm as u32, 0x24);
            if a0 == 0 {
                if ac == dir {
                    return 0;
                }
                free_knots_and_dir(m, ac);
                return 0;
            }
        } else {
            a0 = pts;
        }
        if allocate_fixed_data(m, mm, order, e48, e44, e40, e3c, e38, e34, e30) & 0xffff == 0 {
            if closed == 0 && a0 != 0 {
                m.free(a0);
            }
            if ac == dir {
                return 0;
            }
            free_knots_and_dir(m, ac);
            return 0;
        }
        let ncur = |m: &Mem| m.i32(ac.wrapping_add(4)).wrapping_sub(if closed == 0 { 2 } else { 0 });
        let mut i = 0i32;
        while i < ncur(m) {
            m.copy(m.u32(e3c).wrapping_add(0x24u32.wrapping_mul(i as u32)), zp, 0x24);
            i += 1;
        }
        let mut first = true;
        'main: loop {
            let cur = ncur(m);
            if cur > mm {
                break;
            }
            let mat40 = m.u32(e40);
            m.set_i32(mat40, cur);
            m.set_i32(mat40.wrapping_add(4), if order.wrapping_mul(2).wrapping_sub(1) <= cur { order.wrapping_mul(2).wrapping_sub(1) } else { cur });
            let mat44 = m.u32(e44);
            if first {
                first = false;
                let r = compute_nurbs_evaluation_matrix(m, o, ac, mat44, e4c, n, params);
                if r & 0xffff == 0 {
                    ok = false;
                    break 'main;
                }
            } else {
                let seg = m.u32(e4c);
                let r = update_nurbs_evaluation_matrix(m, o, ac, mat44, seg, params);
                if r & 0xffff == 0 {
                    ok = false;
                    break 'main;
                }
            }
            let seg = m.u32(e4c);
            if closed == 0 {
                adjust_open_data(m, mm, a0, pts, seg, mat44);
            }
            compute_normal_equations(m, seg, closed, order, cur, mat40);
            let r = mult_vec_by_eval_mat_trans(m, seg, mat44, closed, a0, cur, m.u32(e38));
            if r & 0xffff == 0 {
                ok = false;
                break 'main;
            }
            let (c3c, c38, c34, c30, c40) = (m.u32(e3c), m.u32(e38), m.u32(e34), m.u32(e30), mat40);
            mult_vec_by_nrml_eqns(m, c40, order, c3c, c34);
            point_vec_subtract_xyz(m, cur, c38, c34, c38);
            point_vec_inner_product(m, cur, c38, c38, l68, l60, l58);
            let (rx, ry, rz) = (m.f64(l68), m.f64(l60), m.f64(l58));
            let thr_x = (x(C_SCALE) * x(rx)).to_f64();
            let thr_y = (x(C_SCALE) * x(ry)).to_f64();
            let z80 = x(C_SCALE) * x(rz);
            let thr_z = z80.to_f64();
            let mut j = 0i32;
            let (mut pv_x, mut pv_y, mut pv_z) = (0.0f64, 0.0f64, 0.0f64); // [ebp-0xb0], [ebp-0xb8], [ebp-0xc0]
            let enter = cur > 0 && (thr_x < rx || thr_y < ry || z80.lt(x(rz)));
            if enter {
                loop {
                    o.call(m, SI_SHOW_ACTIVITY, &[]);
                    let next_j;
                    if j == 0 {
                        let mut k = 0u32;
                        while k < cur as u32 {
                            m.copy(c34.wrapping_add(0x24 * k), c38.wrapping_add(0x24 * k), 0x24);
                            k += 1;
                        }
                        next_j = 1;
                    } else {
                        let (qx, qy, qz) = (m.f64(l68), m.f64(l60), m.f64(l58));
                        let bx = if x(pv_x).gt(x(C_TINY)) { x(qx) / x(pv_x) } else { F80::from_f64(0.0) };
                        let by = if x(C_TINY).lt(x(pv_y)) { x(qy) / x(pv_y) } else { F80::from_f64(0.0) };
                        let bz = if x(C_TINY).lt(x(pv_z)) { x(qz) / x(pv_z) } else { F80::from_f64(0.0) };
                        let lim = x(C_RATIO);
                        if lim.lt(bx) && lim.lt(by) && bz.gt(lim) {
                            break;
                        }
                        // the y ratio is dropped from the register stack: z goes in twice
                        point_vec_gaxpy(m, cur, [bx.to_f64(), bz.to_f64(), bz.to_f64()], c34, c38, c34);
                        next_j = j + 1;
                    }
                    mult_vec_by_nrml_eqns(m, c40, order, c34, c30);
                    point_vec_inner_product(m, cur, c34, c30, l80, l78, l70);
                    let step = |m: &Mem, num: u32, den: u32| -> f64 {
                        let d = m.f64(den);
                        if x(d).gt(x(C_EPS)) {
                            (x(m.f64(num)) / x(d)).to_f64()
                        } else {
                            0.0
                        }
                    };
                    let (ax, ay, az) = (step(m, l68, l80), step(m, l60, l78), step(m, l58, l70));
                    point_vec_gaxpy(m, cur, [ax, ay, az], c34, c3c, c3c);
                    point_vec_gaxpy(m, cur, [-ax, -ay, -az], c30, c38, c38);
                    pv_x = m.f64(l68);
                    pv_y = m.f64(l60);
                    pv_z = m.f64(l58);
                    point_vec_inner_product(m, cur, c38, c38, l68, l60, l58);
                    j = next_j;
                    if j >= cur {
                        break;
                    }
                    let (nx, ny, nz) = (m.f64(l68), m.f64(l60), m.f64(l58));
                    if !(thr_x < nx || thr_y < ny || thr_z < nz) {
                        break;
                    }
                }
            }
            mult_vec_by_eval_mat(m, seg, mat44, closed, cur, c3c, m.u32(e48));
            let r = calculate_error(m, ac, a0, m.u32(e48), mm, l8c, params, l88, l84, tol);
            if r & 0xffff == 0 {
                ok = false;
                break 'main;
            }
            if m.i32(l88) > 0 {
                let r = update_knot_vector(m, o, ac, e3c, m.i32(l8c), l88, m.u32(l84), mm, params, pts);
                if r & 0xffff == 0 {
                    free_clear(m, l84);
                    ok = false;
                    break 'main;
                }
            }
            free_clear(m, l84);
            if m.i32(l88) <= 0 {
                break;
            }
        }
        if ok {
            let nc = m.i32(ac.wrapping_add(4));
            res = m.si_calloc(nc as u32, 0x24);
            if res == 0 {
                ok = false;
            } else {
                let c3c = m.u32(e3c);
                if closed == 0 {
                    m.copy(res, pts, 0x24);
                    let mut i = 1i32;
                    while i < m.i32(ac.wrapping_add(4)).wrapping_sub(1) {
                        m.copy(res.wrapping_add(0x24 * i as u32), c3c.wrapping_add(0x24 * (i - 1) as u32), 0x24);
                        i += 1;
                    }
                    let last = m.i32(ac.wrapping_add(4)).wrapping_sub(1) as u32;
                    m.copy(res.wrapping_add(0x24u32.wrapping_mul(last)), pts.wrapping_add(0x24u32.wrapping_mul((n - 1) as u32)), 0x24);
                } else {
                    let mut i = 0i32;
                    while i < m.i32(ac.wrapping_add(4)) {
                        m.copy(res.wrapping_add(0x24 * i as u32), c3c.wrapping_add(0x24 * i as u32), 0x24);
                        i += 1;
                    }
                }
                let mut i = 0i32;
                while i < m.i32(ac.wrapping_add(4)) {
                    m.set_u32(res.wrapping_add(0x24 * i as u32 + 0x20), 0);
                    i += 1;
                }
            }
        }
        // 0x084d143c: give the working storage back
        free_segment_list(m, e4c);
        for s in [e30, e34, e38, e3c] {
            free_clear(m, s);
        }
        let mat40 = m.u32(e40);
        if mat40 != 0 {
            m.set_i32(mat40, mm);
            m.set_i32(mat40.wrapping_add(4), order.wrapping_mul(2).wrapping_sub(1));
            mat_dispose(m, e40);
        }
        let mat44 = m.u32(e44);
        if mat44 != 0 {
            m.set_i32(mat44, mm);
            mat_dispose(m, e44);
        }
        free_clear(m, e48);
        if closed == 0 && a0 != 0 {
            m.free(a0);
        }
    }
    // 0x084d1543
    if !ok {
        return res;
    }
    let dir0 = m.i32(dir);
    if n >= dir0 {
        return res;
    }
    let dn = m.i32(dir.wrapping_add(8));
    let nc = if closed == 0 { dn.wrapping_add(2).wrapping_sub(dir0) } else { dn.wrapping_sub(1) };
    m.set_i32(dir.wrapping_add(4), nc);
    let nw = m.si_calloc(nc as u32, 0x24);
    if nw == 0 {
        if res != 0 {
            m.free(res);
        }
        if ac != 0 {
            free_knots_and_dir(m, ac);
        }
        return 0;
    }
    let mut k = 0u32;
    let mut i = 0i32;
    while i < m.i32(dir.wrapping_add(4)) {
        let src = if i < m.i32(dir).wrapping_sub(order) {
            zp
        } else {
            let s = res.wrapping_add(k);
            k += 0x24;
            s
        };
        m.copy(nw.wrapping_add(0x24 * i as u32), src, 0x24);
        i += 1;
    }
    let mut i = 0i32;
    while i < m.i32(dir).wrapping_sub(order) {
        let den = order.wrapping_add(i);
        let inv = F80::ONE / F80::from_i32(den);
        let top = m.i32(dir).wrapping_sub(order).wrapping_sub(i);
        let mut a = 0x24u32.wrapping_mul(top.wrapping_sub(1) as u32);
        let mut b = 0x24u32.wrapping_mul(top as u32);
        let mut j = 0i32;
        while j < den {
            let t = inv * F80::from_i32(j);
            let u = F80::ONE - t;
            for c in 0..4u32 {
                let pa = nw.wrapping_add(a).wrapping_add(8 * c);
                let pb = nw.wrapping_add(b).wrapping_add(8 * c);
                let v = t * x(m.f64(pa)) + u * x(m.f64(pb));
                m.set_f64(pa, v.to_f64());
            }
            a = a.wrapping_add(0x24);
            b = b.wrapping_add(0x24);
            j += 1;
        }
        i += 1;
    }
    if res != 0 {
        m.free(res);
    }
    res = nw;
    if ac != 0 {
        free_knots_and_dir(m, ac);
    }
    res
}

/// `SI_fitNurbsCurve` (`curveFit.c`), the ported callees called directly: the angle (degrees) is scaled to radians
/// in the 80-bit stack and its cosine goes to `partitionLinearData`; each piece is fitted by `fitNurbsPoints` with
/// the tolerance `a2`/`a3` (the two words of a double) and the pieces are joined by `nurbsCrvFromComponents`.
/// Returns the curve itself for two data points, 0 when the partition or a fit fails. [soft 0x084ce350 SI_fitNurbsCurve]
pub fn si_fit_nurbs_curve(m: &mut Mem, o: &mut dyn Outside, crv: u32, n: i32, a2: u32, a3: u32, angle: f64) -> u32 {
    use crate::fit::cleanup_partition;
    use crate::fit2::nurbs_crv_from_components;
    let c = (x(angle) * x(f64::from_bits(0x3f91_df48_1358_e711))).to_f64();
    let cs = si_x87::trig::cos(c).to_f64();
    if n == 2 {
        return crv;
    }
    let sp = m.sp();
    let (l1c, l18, l14, l10, l0c, l08, l04) = (m.local(4), m.local(4), m.local(4), m.local(4), m.local(4), m.local(4), m.local(4));
    let r = crate::geom10::partition_linear_data(m, crv, cs, n, l1c, l18, l14, l10, l0c, l08, l04);
    if r & 0xffff == 0 {
        m.set_sp(sp);
        return 0;
    }
    let (v1c, v18, v14, v10, v08, v04) = (m.u32(l1c), m.u32(l18) as i32, m.u32(l14), m.u32(l10), m.u32(l08), m.u32(l04));
    let cond14 = if m.u32(crv.wrapping_add(0x2c)) == v14 { 0 } else { v14 };
    let tol = f64::from_bits(a2 as u64 | ((a3 as u64) << 32));
    let res = m.si_calloc(v18 as u32, 4);
    let mut out = 0;
    if res != 0 {
        let mut off = 0u32;
        let mut i = 0i32;
        let mut failed = false;
        while i < v18 {
            o.call(m, SI_SHOW_ACTIVITY, &[]);
            let first = m.u32(v08.wrapping_add(4 * i as u32));
            let pn = m.u32(v04.wrapping_add(4 * i as u32)) as i32;
            let r = fit_nurbs_points(m, o, off.wrapping_add(v1c), tol, pn, v14.wrapping_add(0x24u32.wrapping_mul(first)), v10.wrapping_add(8u32.wrapping_mul(first)));
            m.set_u32(res.wrapping_add(4 * i as u32), r);
            if r == 0 {
                for j in 0..i as u32 {
                    let p = m.u32(res.wrapping_add(4 * j));
                    if p != 0 {
                        m.free(p);
                        m.set_u32(res.wrapping_add(4 * j), 0);
                    }
                }
                m.free(res);
                failed = true;
                break;
            }
            off = off.wrapping_add(0x24);
            i += 1;
        }
        if !failed {
            let closed = m.i16(crv.wrapping_add(0x20)) as i32;
            out = nurbs_crv_from_components(m, v1c, v18, res, closed);
            for j in 0..v18 as u32 {
                let p = m.u32(res.wrapping_add(4 * j));
                if p != 0 {
                    m.free(p);
                    m.set_u32(res.wrapping_add(4 * j), 0);
                }
            }
            m.free(res);
        }
    }
    cleanup_partition(m, v1c, v18, cond14, v10, v08, v04);
    m.set_sp(sp);
    out
}

/// The threshold 1e-4 at 0x08661e28 (a vector shorter than this is not normalised).
const F_SHORT: f64 = f64::from_bits(0x3f1a_36e2_eb1c_432d);
/// The double 0.5 at 0x08661e30 (the turn between two tangents below which the frame is carried over unrotated).
const F_HALF: f64 = 0.5;

fn fl(m: &Mem, a: u32) -> F80 {
    x(m.f32(a))
}

fn vec_at(m: &Mem, a: u32) -> si_math::types::Vector {
    si_math::types::Vector::new(m.f32(a), m.f32(a.wrapping_add(4)), m.f32(a.wrapping_add(8)))
}

/// `((v0 * c0 + v1 * c1) + v2 * c2) + c3` as the register sum of the listing: the three products, then the
/// translation, not rounded between the terms.
fn row_sum(v: [F80; 3], col: [f32; 4]) -> F80 {
    ((v[0] * x(col[0]) + v[1] * x(col[1])) + v[2] * x(col[2])) + x(col[3])
}

/// The bottom row of a frame (the binormal, +0x18): `position × tangent`, the position still in the register.
fn put_binormal(m: &mut Mem, fr: u32, p: [F80; 3]) {
    let (tx, ty, tz) = (fl(m, fr.wrapping_add(0xc)), fl(m, fr.wrapping_add(0x10)), fl(m, fr.wrapping_add(0x14)));
    m.set_f32(fr.wrapping_add(0x18), (p[1] * tz - p[2] * ty).to_f32());
    m.set_f32(fr.wrapping_add(0x1c), (p[2] * tx - p[0] * tz).to_f32());
    m.set_f32(fr.wrapping_add(0x20), (p[0] * ty - p[1] * tx).to_f32());
}

/// Scales three register values by the float length `s` of `(a^2 + b^2) + c^2`, when it is above 1e-4; the values come back as floats.
fn norm3(v: [f32; 3]) -> [f32; 3] {
    let (a, b, c) = (x(v[0]), x(v[1]), x(v[2]));
    let s = ((a * a + b * b) + c * c).sqrt().to_f32();
    if x(s).abs().gt(x(F_SHORT)) {
        [(a / x(s)).to_f32(), (b / x(s)).to_f32(), (c / x(s)).to_f32()]
    } else {
        v
    }
}

/// `SI_nurbsUpdateFrenet` (`nrb_geometry.c`) on the image: marks the curve (+0x3b |= 0x60), refreshes its animation
/// and length tables, and builds the `SI_calloc(count, 0x24)` block of frames at `*slot` (position +0, tangent +0xc,
/// binormal +0x18, floats): the unit tangent of the curve at each table parameter (+0x54); the first frame from
/// `SI_matrixZAlign` of its tangent (the inverse applied to the X axis); each next frame the previous one carried
/// by the rotation about `prev tangent x tangent` through their angle (unrotated when the cross vector's
/// absolute sum is not above 0.5); for a closed curve (+0x20) the whole run is then twisted back by a share of the
/// angle between the first and the last frame's positions (sic) per frame. [soft 0x085020a8 SI_nurbsUpdateFrenet]
pub fn nurbs_update_frenet(m: &mut Mem, o: &mut dyn Outside, crv: u32, slot: u32) -> Result<(), crate::image::Halt> {
    use crate::image::{crv_value, nurbs_crv_anim_update, nurbs_curve_length};
    use si_math::algebra::{matrix_axis_rotation, matrix_inverse, matrix_z_align, vector_angle, vector_multiply_matrix};
    use si_math::types::{Matrix, Vector, IDENTITY};
    m.or_u8(crv.wrapping_add(0x3b), 0x60);
    nurbs_crv_anim_update(m, o, crv);
    nurbs_curve_length(m, o, crv)?;
    let n = m.i32(crv.wrapping_add(0x50));
    let blk = m.si_calloc(n as u32, 0x24);
    m.set_u32(slot, blk);
    if blk == 0 {
        return Err(crate::image::Halt::Refused("SI_nurbsUpdateFrenet: no frames"));
    }
    // the unit tangents
    let ts = m.u32(crv.wrapping_add(0x54));
    for i in 0..n {
        let fr = blk.wrapping_add(0x24u32.wrapping_mul(i as u32));
        let t = m.f64(ts.wrapping_add(8 * i as u32));
        let v = crv_value(m, crv, t, 1)?;
        let (tx, ty, tz) = (x(v[0]).to_f32(), x(v[1]).to_f32(), x(v[2]).to_f32());
        let s = (x(tz) * tz + (x(tx) * tx + x(ty) * ty)).sqrt().to_f32();
        let w = if x(s).abs().gt(x(F_SHORT)) { [(x(tx) / x(s)).to_f32(), (x(ty) / x(s)).to_f32(), (x(tz) / x(s)).to_f32()] } else { [tx, ty, tz] };
        m.set_f32(fr.wrapping_add(0xc), w[0]);
        m.set_f32(fr.wrapping_add(0x10), w[1]);
        m.set_f32(fr.wrapping_add(0x14), w[2]);
    }
    // the first frame: the X axis seen from the frame whose Z axis is the tangent
    let zm: Matrix = matrix_z_align(&vec_at(m, blk.wrapping_add(0xc)));
    let inv: Matrix = matrix_inverse(&zm).unwrap_or([[0.0; 4]; 4]);
    let axis = [x(1.0f32), x(0.0f32), x(0.0f32)]; // SI_xAxis (0x086dc1ec)
    let p = [0, 1, 2].map(|c| row_sum(axis, [inv[0][c], inv[1][c], inv[2][c], inv[3][c]]));
    for c in 0..3 {
        m.set_f32(blk.wrapping_add(4 * c as u32), p[c].to_f32());
    }
    put_binormal(m, blk, p);
    for i in 1..n {
        let prev = blk.wrapping_add(0x24u32.wrapping_mul((i - 1) as u32));
        let cur = prev.wrapping_add(0x24);
        let pt = [fl(m, prev.wrapping_add(0xc)), fl(m, prev.wrapping_add(0x10)), fl(m, prev.wrapping_add(0x14))];
        let ct = [fl(m, cur.wrapping_add(0xc)), fl(m, cur.wrapping_add(0x10)), fl(m, cur.wrapping_add(0x14))];
        let x1 = pt[1] * ct[2] - pt[2] * ct[1];
        let x2 = pt[2] * ct[0] - pt[0] * ct[2];
        let x3 = pt[0] * ct[1] - pt[1] * ct[0];
        let mut nv = [x1.to_f32(), x2.to_f32(), x3.to_f32()];
        let s = ((x1 * x1 + x2 * x2) + x3 * x3).sqrt().to_f32();
        if x(s).abs().gt(x(F_SHORT)) {
            nv = [(x1 / x(s)).to_f32(), (x2 / x(s)).to_f32(), (x3 / x(s)).to_f32()];
        }
        let rot: Matrix = if (x(nv[2]).abs() + (x(nv[1]).abs() + x(nv[0]).abs())).gt(x(F_HALF)) {
            let ang = vector_angle(&vec_at(m, prev.wrapping_add(0xc)), &vec_at(m, cur.wrapping_add(0xc)));
            matrix_axis_rotation(ang, &Vector::new(nv[0], nv[1], nv[2]))
        } else {
            IDENTITY
        };
        let pp = [fl(m, prev), fl(m, prev.wrapping_add(4)), fl(m, prev.wrapping_add(8))];
        let np = [0, 1, 2].map(|c| row_sum(pp, [rot[0][c], rot[1][c], rot[2][c], rot[3][c]]));
        for c in 0..3 {
            m.set_f32(cur.wrapping_add(4 * c as u32), np[c].to_f32());
        }
        put_binormal(m, cur, np);
    }
    if m.i16(crv.wrapping_add(0x20)) != 0 {
        let f0 = m.u32(slot);
        let a = vec_at(m, f0);
        let t0 = [m.f32(f0.wrapping_add(0xc)), m.f32(f0.wrapping_add(0x10)), m.f32(f0.wrapping_add(0x14))];
        let last = f0.wrapping_add(0x24u32.wrapping_mul(n as u32)).wrapping_sub(0x24);
        let b = vec_at(m, last);
        let mut ang = x(vector_angle(&a, &b));
        let (ax, ay, az, bx, by, bz) = (x(a.x), x(a.y), x(a.z), x(b.x), x(b.y), x(b.z));
        let k1 = ay * bz - az * by;
        let k2 = az * bx - ax * bz;
        let k3 = ax * by - ay * bx;
        let z = F80::ZERO;
        let flip = (k1.gt(z) && x(t0[0]).lt(z))
            || (k1.lt(z) && z.lt(x(t0[0])))
            || (k2.gt(z) && x(t0[1]).lt(z))
            || (k2.lt(z) && z.lt(x(t0[1])))
            || (k3.gt(z) && x(t0[2]).lt(z))
            || (k3.lt(z) && z.lt(x(t0[2])));
        if flip {
            ang = -ang;
        }
        let mut j = 1i32;
        while j < m.i32(crv.wrapping_add(0x50)) {
            let nn = m.i32(crv.wrapping_add(0x50));
            let fr = m.u32(slot).wrapping_add(0x24u32.wrapping_mul(j as u32));
            let q = (((-ang) / F80::from_i32(nn)) * F80::from_i32(j)).to_f32();
            let rot = matrix_axis_rotation(q, &vec_at(m, fr.wrapping_add(0xc)));
            for off in [0u32, 0xc, 0x18] {
                let v = vector_multiply_matrix(&vec_at(m, fr.wrapping_add(off)), &rot);
                let w = norm3([v.x, v.y, v.z]);
                let at = m.u32(slot).wrapping_add(0x24u32.wrapping_mul(j as u32)).wrapping_add(off);
                m.set_f32(at, w[0]);
                m.set_f32(at.wrapping_add(4), w[1]);
                m.set_f32(at.wrapping_add(8), w[2]);
            }
            j += 1;
        }
    }
    Ok(())
}
