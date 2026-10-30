// SPDX-License-Identifier: MIT
//! Degree elevation on the memory image (`nrb_misc.c`; finding 0025, section 21): the knots of the next order
//! (`nurbsDirElevateOrder`) and the new curve or surface made by the blossoming recurrence the two entry points share.
//! Both return a NEW copy (the argument is not changed); the copy's old knot and (surface) point arrays leak, as in the original.

use crate::image::{at8, crv_copy, point_copy, srf_copy, Halt, LEFT};
use crate::{Dir, NurbsDir, Statics};
use si_hrc::image::nurbs::{knots_allocate, nurbs_points_allocate};
use si_mem::{Mem, Outside, Ptr32};
use si_x87::{x, F80};

/// `_DAT_08661968`: two knots closer than this are the same knot.
const SAME: f64 = 1e-5;

/// [soft 0x084e8f60 nurbsDirElevateOrder] (`src`, `dst`: direction records, `dst` a copy of `src`): `dst` gets the order + 1,
/// a fresh knot array (`si_knotsAllocate((nk - order) * 2 + 2)`) and these knots: the first order - 1 of `src`, then every
/// distinct value from index order - 1 to nk - order (equal within 1e-5, the difference in the register; compared against the
/// first of the group) written as often as it occurs PLUS ONE, then the last order - 1. The new count (+8) and +4 = count -
/// order are set at the end; +8 starts at 0 and counts the writes. The copy's old knot array is not freed.
pub fn dir_elevate_order(m: &mut Mem, src: u32, dst: u32) {
    let n = knots_allocate(m, m.i32(src + 8).wrapping_sub(m.i32(src)).wrapping_mul(2).wrapping_add(2));
    m.set_u32(dst + 0xc, n);
    m.set_i32(dst, m.i32(src).wrapping_add(1));
    m.set_i32(dst + 8, 0);
    let put = |m: &mut Mem, v: f64| {
        let at = m.u32(dst + 0xc).wrapping_add(8u32.wrapping_mul(m.i32(dst + 8) as u32));
        m.set_f64(at, v);
        m.set_i32(dst + 8, m.i32(dst + 8).wrapping_add(1));
    };
    let mut i: i32 = 0;
    while i < m.i32(src).wrapping_sub(1) {
        let v = m.f64(at8(m.u32(src + 0xc), i));
        put(m, si_x87::fmov64(v));
        i += 1;
    }
    let order = m.i32(src);
    let last = m.i32(src + 8).wrapping_sub(order);
    let mut cur = m.f64(at8(m.u32(src + 0xc), order.wrapping_sub(1)));
    let mut mult: i32 = 1;
    let tol = x(SAME);
    let mut i = order;
    while i <= last {
        let v = m.f64(at8(m.u32(src + 0xc), i));
        if (x(v) - x(cur)).abs().lt(tol) {
            mult = mult.wrapping_add(1);
        } else {
            for _ in 0..mult.wrapping_add(1).max(0) {
                put(m, cur);
            }
            mult = 1;
            cur = v;
        }
        i += 1;
    }
    for _ in 0..mult.wrapping_add(1).max(0) {
        put(m, cur);
    }
    let mut i = m.i32(src + 8).wrapping_sub(m.i32(src)).wrapping_add(1);
    while i < m.i32(src + 8) {
        let v = m.f64(at8(m.u32(src + 0xc), i));
        put(m, si_x87::fmov64(v));
        i += 1;
    }
    m.set_i32(dst + 4, m.i32(dst + 8).wrapping_sub(m.i32(dst)));
}

fn zero_point(m: &mut Mem, at: u32) {
    // the ninth word is an uninitialised stack word in the original; it is scratch in a block freed at the end
    for w in 0..9u32 {
        m.set_u32(at.wrapping_add(4 * w), 0);
    }
}

/// One new control point `j` of the elevated direction: `old` is the old direction record (order, count, knots), `newk` the
/// new knot array, `pts` the first old point of the row (curve: the net; surface: row v), `a` and `b` the two scratch
/// blocks of `order` points, `dest` the address of the new point. The blossoming recurrence of the original, with its x87 order:
/// the factor `1 / (K1 - K2)` stays unrounded in a register and the weights `(d2 - K2) * inv` etc. too; the new A row is
/// `p * r1 + q * r2` with x unrounded and y, z, w from two stored products; the B row adds A to
/// `b * r3 + q * r4` (y, z, w through a rounded sum).
fn elevate_point(m: &mut Mem, old: u32, newk: u32, pts: u32, a: u32, b: u32, dest: u32, j: i32) -> Result<(), Halt> {
    let order = m.i32(old);
    let n = m.i32(old + 4);
    let rec: NurbsDir = m.read(Ptr32::new(old));
    let kv = crate::fit2::knot_vec(m, &rec);
    let mut st = Statics { left: m.i32(LEFT) };
    let r = crate::knots::interv(&mut st, Dir { rec: &rec, knots: &kv }, m.f64(at8(newk, j)), 0);
    m.set_i32(LEFT, st.left);
    let iv = r.ok_or(Halt::Refused("elevate"))?;
    let p = |i: i32| 0x24u32.wrapping_mul(i as u32);
    let mut k = 0;
    while k < order {
        zero_point(m, a.wrapping_add(p(k)));
        zero_point(m, b.wrapping_add(p(k)));
        k += 1;
    }
    let hi0 = order.wrapping_add(n).wrapping_sub(iv).min(order);
    let mut k = order.wrapping_sub(iv).max(0);
    while k < hi0 {
        let from = pts.wrapping_add(p(iv.wrapping_sub(order).wrapping_add(k)));
        point_copy(m, from, a.wrapping_add(p(k)));
        point_copy(m, from, b.wrapping_add(p(k)));
        k += 1;
    }
    let mut e = order.wrapping_sub(1);
    while e > 0 {
        let d1 = m.f64(at8(newk, j.wrapping_add(e)));
        let d2 = m.f64(at8(newk, j.wrapping_add(e).wrapping_add(1)));
        let hi = n.wrapping_add(order.wrapping_mul(2)).wrapping_sub(e).wrapping_sub(iv).min(order);
        let lo = order.wrapping_sub(iv).wrapping_add(1).max(order.wrapping_sub(e).wrapping_add(1));
        if lo <= hi {
            let mut c0 = iv.wrapping_add(e).wrapping_sub(order).wrapping_add(hi);
            let mut c8 = iv.wrapping_sub(order).wrapping_add(hi);
            let mut pa = a.wrapping_add(p(hi)).wrapping_sub(0xc);
            let mut pb = b.wrapping_add(p(hi)).wrapping_sub(0xc);
            let mut h = hi;
            while h >= lo {
                let kb = m.u32(old + 0xc);
                let k1 = x(m.f64(at8(kb, c0.wrapping_sub(1))));
                let k2 = x(m.f64(at8(kb, c8.wrapping_sub(1))));
                let inv = x(1.0) / (k1 - k2);
                let (d1x, d2x) = (x(d1), x(d2));
                let r1 = (d2x - k2) * inv;
                let r2 = (k1 - d2x) * inv;
                let f = |m: &Mem, at: u32| x(m.f64(at));
                // the A row: element h - 1 from itself and h - 2
                let (ax, ay, az, aw) = (pa.wrapping_sub(0x18), pa.wrapping_sub(0x10), pa.wrapping_sub(8), pa);
                let (qx, qy, qz, qw) = (pa.wrapping_sub(0x3c), pa.wrapping_sub(0x34), pa.wrapping_sub(0x2c), pa.wrapping_sub(0x24));
                let t1 = f(m, ax) * r1;
                let t2 = f(m, qx) * r2;
                let sy = (f(m, ay) * r1).to_f64();
                let sz = (f(m, az) * r1).to_f64();
                let sw = (r1 * f(m, aw)).to_f64();
                let uy = (f(m, qy) * r2).to_f64();
                let uz = (f(m, qz) * r2).to_f64();
                let uw = (r2 * f(m, qw)).to_f64();
                m.set_f64(ax, (t2 + t1).to_f64());
                m.set_f64(ay, (x(sy) + x(uy)).to_f64());
                m.set_f64(az, (x(sz) + x(uz)).to_f64());
                m.set_f64(aw, (x(sw) + x(uw)).to_f64());
                // the B row
                let r3 = (d1x - k2) * inv;
                let r4 = inv * (k1 - d1x);
                let (bx, by, bz, bw) = (pb.wrapping_sub(0x18), pb.wrapping_sub(0x10), pb.wrapping_sub(8), pb);
                let (px, py, pz, pw) = (pb.wrapping_sub(0x3c), pb.wrapping_sub(0x34), pb.wrapping_sub(0x2c), pb.wrapping_sub(0x24));
                let e1 = f(m, bx) * r3;
                let e2 = f(m, by) * r3;
                let e3 = f(m, bz) * r3;
                let e4 = r3 * f(m, bw);
                let f1 = f(m, px) * r4;
                let f2 = f(m, py) * r4;
                let f3 = f(m, pz) * r4;
                let f4 = r4 * f(m, pw);
                let g1 = f1 + e1;
                let g2 = e2 + f2;
                let g3 = e3 + f3;
                let g4 = e4 + f4;
                m.set_f64(bx, (g1 + f(m, ax)).to_f64());
                m.set_f64(by, (x(g2.to_f64()) + f(m, ay)).to_f64());
                m.set_f64(bz, (x(g3.to_f64()) + f(m, az)).to_f64());
                m.set_f64(bw, (x(g4.to_f64()) + f(m, aw)).to_f64());
                c0 = c0.wrapping_sub(1);
                c8 = c8.wrapping_sub(1);
                pa = pa.wrapping_sub(0x24);
                pb = pb.wrapping_sub(0x24);
                h = h.wrapping_sub(1);
            }
        }
        e -= 1;
    }
    let inv = F80::ONE / F80::from_i32(order);
    let src = b.wrapping_add(p(order)).wrapping_sub(0x24);
    for off in [0u32, 8, 0x10] {
        let v = x(m.f64(src.wrapping_add(off))) * inv;
        m.set_f64(dest.wrapping_add(off), v.to_f64());
    }
    let v = inv * x(m.f64(src.wrapping_add(0x18)));
    m.set_f64(dest.wrapping_add(0x18), v.to_f64());
    Ok(())
}

fn free_scratch(m: &mut Mem, a: u32, b: u32) {
    if a != 0 {
        m.free(a);
    }
    if b != 0 {
        m.free(b);
    }
}

/// [soft 0x084e99d0 SI_nurbsCrvElevateDegree]: a new curve of order + 1 with the same shape: two scratch blocks of `order`
/// points (`SI_nurbsPointsAllocate`), `SI_nurbsCrvCopy`, [`dir_elevate_order`] on the copy, a new points block (the copy's old
/// one leaks), then per new point [`elevate_point`]; the scratch blocks freed (A, then B). Answers the new curve.
pub fn crv_elevate_degree(m: &mut Mem, c: u32) -> Result<u32, Halt> {
    let order = m.i32(c);
    let a = nurbs_points_allocate(m, order);
    let b = nurbs_points_allocate(m, order);
    let d = crv_copy(m, c);
    dir_elevate_order(m, c, d);
    let np = nurbs_points_allocate(m, m.i32(d + 4));
    m.set_u32(d + 0x2c, np);
    let mut j = 0;
    while j < m.i32(d + 4) {
        let dest = m.u32(d + 0x2c).wrapping_add(0x24u32.wrapping_mul(j as u32));
        elevate_point(m, c, m.u32(d + 0xc), m.u32(c + 0x2c), a, b, dest, j)?;
        j += 1;
    }
    free_scratch(m, a, b);
    Ok(d)
}

/// [soft 0x084e90a0 SI_nurbsSrfElevateDegree]: the same in the U direction of a surface, row by row (V unchanged): the copy's
/// point total (+0x48) is new count × nv, with a new points block; for every row `v` and new column `j` the point is
/// computed from row `v` of the old net (`v × nu` points in). `Err` where the span search would not end.
pub fn srf_elevate_degree(m: &mut Mem, o: &mut dyn Outside, s: u32) -> Result<u32, Halt> {
    let order = m.i32(s);
    let a = nurbs_points_allocate(m, order);
    let b = nurbs_points_allocate(m, order);
    let d = srf_copy(m, o, s);
    dir_elevate_order(m, s, d);
    let total = m.i32(d + 4).wrapping_mul(m.i32(d + 0x28));
    m.set_i32(d + 0x48, total);
    let np = nurbs_points_allocate(m, total);
    m.set_u32(d + 0x54, np);
    let mut v = 0;
    while v < m.i32(d + 0x28) {
        let mut j = 0;
        while j < m.i32(d + 4) {
            let row = m.u32(s + 0x54).wrapping_add(0x24u32.wrapping_mul(v.wrapping_mul(m.i32(s + 4)) as u32));
            let dest = m.u32(d + 0x54).wrapping_add(0x24u32.wrapping_mul(v.wrapping_mul(m.i32(d + 4)).wrapping_add(j) as u32));
            elevate_point(m, s, m.u32(d + 0xc), row, a, b, dest, j)?;
            j += 1;
        }
        v += 1;
    }
    free_scratch(m, a, b);
    Ok(d)
}
