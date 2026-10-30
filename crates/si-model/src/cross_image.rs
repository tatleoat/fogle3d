// SPDX-License-Identifier: MIT
//! The linear-system helpers of `cross.c` and the Bessel end conditions of a NURBS curve, on
//! `si-mem`'s memory image (finding 0023, unit "prim2", section 44). A function takes the
//! image and the arguments the original takes and makes the original's allocation calls in its
//! order.
//!
//! Floating point follows finding 0008: the original keeps each intermediate value in an x87
//! register and rounds it only where it stores it (to `double` in the matrices, to `float` in
//! the vectors); the order of the operations below is the order of the listing.

use si_hrc::image::nurbs::{crv, nurbs_crv_allocate, nurbs_points_allocate};
use si_mem::Mem;
use si_x87::{x, F80};

/// 1e-6 (0x0864a288): the smallest knot ratio for which the end tangent is scaled.
const EPS: f64 = 1e-6;
/// 1/3 (0x0864a278) and 2/3 (0x0864a280).
const THIRD: f64 = f64::from_bits(0x3fd5_5555_5555_5555);
const TWO_THIRDS: f64 = f64::from_bits(0x3fe5_5555_5555_5555);

fn kn(m: &Mem, k: u32, i: i32) -> F80 {
    x(m.f64(k.wrapping_add((i as u32).wrapping_mul(8))))
}

/// The tridiagonal system of an open curve's interpolation [soft 0x08146f7c setup_open_system]:
/// from the knots `k` and the size `n`, the three diagonals `a` (below), `b` (main), `c`
/// (above), `n` doubles each. The first and the last row are the identity (0, 1, 0); row `i` in
/// between takes the knot differences `d3 = k[i+1]-k[i]`, `d5 = k[i+2]-k[i+1]`,
/// `d4 = k[i+3]-k[i+2]`, `d1 = k[i+4]-k[i+3]` in the register.
pub fn setup_open_system(m: &mut Mem, k: u32, n: i32, a: u32, b: u32, c: u32) {
    let at = |p: u32, i: i32| p.wrapping_add((i as u32).wrapping_mul(8));
    m.set_f64(a, 0.0);
    m.set_f64(b, 1.0);
    m.set_f64(c, 0.0);
    let last = n.wrapping_sub(1);
    let mut i = 1;
    while i < last {
        let d4 = kn(m, k, i + 3) - kn(m, k, i + 2);
        let d5 = kn(m, k, i + 2) - kn(m, k, i + 1);
        let d3 = kn(m, k, i + 1) - kn(m, k, i);
        let d1 = kn(m, k, i + 4) - kn(m, k, i + 3);
        let t2 = d4 + d5;
        let inv1 = x(1.0f64) / (t2 + d3);
        let inv2 = x(1.0f64) / (t2 + d1);
        m.set_f64(at(a, i), ((d4 * d4) * inv1).to_f64());
        let first = ((d1 + d4) * d5) * inv2;
        let second = inv1 * ((d3 + d5) * d4);
        m.set_f64(at(b, i), (first + second).to_f64());
        m.set_f64(at(c, i), (inv2 * (d5 * d5)).to_f64());
        i += 1;
    }
    m.set_f64(at(a, last), 0.0);
    m.set_f64(at(b, last), 1.0);
    m.set_f64(at(c, last), 0.0);
}

/// LU decomposition of that tridiagonal matrix [soft 0x08147bbc LU_decomp_tridiagonal]: `l` and
/// `u` are `n` row pointers to `n` doubles each. `l` takes the sub-diagonal and the diagonal,
/// `u` a unit diagonal and the super-diagonal, every entry stored as a double.
#[allow(clippy::too_many_arguments)]
pub fn lu_decomp_tridiagonal(m: &mut Mem, a: u32, b: u32, c: u32, n: i32, l: u32, u: u32) {
    let row = |m: &Mem, p: u32, i: i32| m.u32(p.wrapping_add((i as u32).wrapping_mul(4)));
    let el = |r: u32, j: i32| r.wrapping_add((j as u32).wrapping_mul(8));
    let ai = |p: u32, i: i32| p.wrapping_add((i as u32).wrapping_mul(8));
    let l0 = row(m, l, 0);
    let v = m.f64(b);
    m.set_f64(l0, v);
    let u0 = row(m, u, 0);
    m.set_f64(u0, 1.0);
    let q = (x(m.f64(c)) / x(m.f64(l0))).to_f64();
    m.set_f64(el(u0, 1), q);
    let mut i = 1;
    while i < n.wrapping_sub(1) {
        let li = row(m, l, i);
        let v = m.f64(ai(a, i));
        m.set_f64(el(li, i - 1), v);
        let um = row(m, u, i - 1);
        let d = (x(m.f64(ai(b, i))) - x(m.f64(el(li, i - 1))) * x(m.f64(el(um, i)))).to_f64();
        m.set_f64(el(li, i), d);
        let ui = row(m, u, i);
        m.set_f64(el(ui, i), 1.0);
        let q = (x(m.f64(ai(c, i))) / x(m.f64(el(li, i)))).to_f64();
        m.set_f64(el(ui, i + 1), q);
        i += 1;
    }
    let r = n.wrapping_sub(1);
    let lr = row(m, l, r);
    let v = m.f64(ai(a, r));
    m.set_f64(el(lr, r - 1), v);
    let um = row(m, u, r - 1);
    let d = (x(m.f64(ai(b, r))) - x(m.f64(el(lr, r - 1))) * x(m.f64(el(um, r)))).to_f64();
    m.set_f64(el(lr, r), d);
    let ur = row(m, u, r);
    m.set_f64(el(ur, r), 1.0);
}

fn f32s(m: &Mem, p: u32, i: i32, k: u32) -> F80 {
    x(m.f32(p.wrapping_add((i as u32).wrapping_mul(16)).wrapping_add(4 * k)))
}

fn set4(m: &mut Mem, p: u32, i: i32, k: u32, v: f32) {
    m.set_f32(p.wrapping_add((i as u32).wrapping_mul(16)).wrapping_add(4 * k), v);
}

/// The right-hand side of the open interpolation [soft 0x08147184 setup_open_input]: `src` holds
/// the `n` control points as four floats each (an `SI_Hvector`), `dst` receives the tangent
/// rows. Row 0 and row `n-1` are the end conditions (a quadratic fit through the first or the
/// last three points, scaled by the knot ratio `d1 = (k[4]-k[3])/(k[4]-k[2])`, blended with a
/// third and two thirds of the end point), row `i` in between is `(k[i+3]-k[i+1]) · src[i]`.
/// The ratio and its complement are stored as doubles, the complement computed from the
/// register's ratio; the scale `1/(2·d1·d2)` is 0 when either is below 1e-6.
pub fn setup_open_input(m: &mut Mem, k: u32, src: u32, dst: u32, n: i32) {
    // first end
    let r80 = (kn(m, k, 4) - kn(m, k, 3)) / (kn(m, k, 4) - kn(m, k, 2));
    let d1 = r80.to_f64();
    let t1 = x(1.0f64) - r80;
    let d2 = t1.to_f64();
    let mut c = 0.0f64;
    if !x(EPS).gt(x(d1)) && !x(EPS).gt(t1) {
        c = (x(1.0f64) / (t1 * (x(d1) + x(d1)))).to_f64();
    }
    let t2 = x(d1) * x(d1);
    let t3 = x(d2) * x(d2);
    for kk in 0..4u32 {
        let a = (t2 * f32s(m, src, 0, kk) + t3 * f32s(m, src, 2, kk)).to_f32();
        let b = (-x(a) + f32s(m, src, 1, kk)).to_f32(); // fchs, then faddp: a NaN `a` comes out positive
        let cc = (x(b) * x(c)).to_f32();
        let d = (x(cc) * x(d2) + x(d1) * f32s(m, src, 0, kk)).to_f32();
        let r = (x(d) * x(TWO_THIRDS) + x(THIRD) * f32s(m, src, 0, kk)).to_f32();
        set4(m, dst, 0, kk, r);
    }
    // last end
    let nn = n;
    let r80 = (kn(m, k, nn + 1) - kn(m, k, nn)) / (kn(m, k, nn + 1) - kn(m, k, nn - 1));
    let d1 = r80.to_f64();
    let t4 = x(1.0f64) - r80;
    let d2 = t4.to_f64();
    let c;
    if x(EPS).gt(x(d1)) || x(EPS).gt(t4) {
        c = 0.0f64;
    } else {
        c = (x(1.0f64) / ((x(d1) + x(d1)) * x(d2))).to_f64();
    }
    let r = nn.wrapping_sub(1);
    let t5 = x(d1) * x(d1);
    let t6 = x(d2) * x(d2);
    for kk in 0..4u32 {
        let a = (t5 * f32s(m, src, r - 2, kk) + t6 * f32s(m, src, r, kk)).to_f32();
        let b = (-x(a) + f32s(m, src, r - 1, kk)).to_f32();
        let cc = (x(c) * x(b)).to_f32();
        let d = (x(d2) * f32s(m, src, r, kk) + x(d1) * x(cc)).to_f32();
        let o = (x(d) * x(TWO_THIRDS) + x(THIRD) * f32s(m, src, r, kk)).to_f32();
        set4(m, dst, r, kk, o);
    }
    // the rows between
    let mut i = 1;
    while i < nn.wrapping_sub(1) {
        let w = kn(m, k, i + 3) - kn(m, k, i + 1);
        for kk in 0..4u32 {
            set4(m, dst, i, kk, (w * f32s(m, src, i, kk)).to_f32());
        }
        i += 1;
    }
}

/// Forward and back substitution of the tridiagonal system [soft 0x081476bc solve_tridiagonal_LU]:
/// `u` and `l` the row pointers of [`lu_decomp_tridiagonal`] (`u` first), `rhs` the right-hand
/// side (`n` rows of four floats), `c` the curve whose points (stride 0x24) receive the answer
/// in slots 1 to `n`. The forward pass keeps its vectors as floats (a block of `n` rows from
/// `SI_calloc`, freed at the end); the back pass computes in the register and stores doubles.
pub fn solve_tridiagonal_lu(m: &mut Mem, u: u32, l: u32, rhs: u32, c: u32, n: i32) {
    let z = m.si_calloc(n as u32, 0x10);
    for i in 0..n.max(0) {
        for (k, v) in [0.0f32, 0.0, 0.0, 1.0].into_iter().enumerate() {
            set4(m, z, i, k as u32, v);
        }
    }
    let row = |m: &Mem, p: u32, i: i32| m.u32(p.wrapping_add((i as u32).wrapping_mul(4)));
    let el = |r: u32, j: i32| r.wrapping_add((j as u32).wrapping_mul(8));
    let l0 = row(m, l, 0);
    let t1 = x(1.0f64) / x(m.f64(l0));
    for k in 0..4u32 {
        let v = (t1 * f32s(m, rhs, 0, k)).to_f32();
        set4(m, z, 0, k, v);
    }
    let mut i = 1;
    while i < n {
        let li = row(m, l, i);
        let t2 = x(1.0f64) / x(m.f64(el(li, i)));
        let t3 = -t2 * x(m.f64(el(li, i - 1)));
        for k in 0..4u32 {
            let v = (t2 * f32s(m, rhs, i, k) + t3 * f32s(m, z, i - 1, k)).to_f32();
            set4(m, z, i, k, v);
        }
        i += 1;
    }
    let pts = m.u32(c + crv::POINTS);
    let pt = |i: i32| pts.wrapping_add((i as u32).wrapping_mul(0x24));
    for k in 0..4u32 {
        let v = m.f32(z.wrapping_add(((n - 1) as u32).wrapping_mul(16)).wrapping_add(4 * k));
        m.set_f64(pt(n) + 8 * k, v as f64);
    }
    let mut iv = n - 2;
    while iv >= 0 {
        let ur = row(m, u, iv);
        let uu = x(m.f64(el(ur, iv + 1)));
        for k in 0..4u32 {
            let prev = x(m.f64(pt(iv + 2) + 8 * k));
            let v = ((-uu) * prev + f32s(m, z, iv, k)).to_f64();
            m.set_f64(pt(iv + 1) + 8 * k, v);
        }
        iv -= 1;
    }
    m.free(z);
}

/// A new curve with the Bessel end conditions [soft 0x08147e80 SI_besselEndNurbsCrv]: from the
/// `n` points of `c`, the curve with `n + 2` points that interpolates them through the tridiagonal
/// system of the knot differences (`si_knotsAllocate(n + 4)` knots counted up from the third,
/// repeated at both ends). With `flag` the first and the last tangent rows are replaced by the
/// given vectors `t0` and `t1`; otherwise they stay what [`setup_open_input`] made. Points 0 and
/// `n + 1` of the result are the first and last point of `c` (read as floats); the order
/// (`+0`) and the uniformity (`+0x24`) are copied. All work areas are freed.
pub fn bessel_end_nurbs_crv(m: &mut Mem, c: u32, flag: i16, t0: [f32; 4], t1: [f32; 4]) -> u32 {
    let n = m.i32(c + crv::NB_POINTS);
    let nk = n.wrapping_add(4);
    let knots = m.si_calloc(nk as u32, 8);
    let mut i = 1;
    while i < nk {
        let prev = m.f64(knots.wrapping_add((i as u32).wrapping_mul(8)).wrapping_sub(8));
        let v = if i > 2 && i <= n.wrapping_add(1) { (x(prev) + 1.0f64).to_f64() } else { prev };
        m.set_f64(knots.wrapping_add((i as u32).wrapping_mul(8)), v);
        i += 1;
    }
    let sz = nk.wrapping_sub(4);
    let a = m.si_calloc(sz as u32, 8);
    let b = m.si_calloc(sz as u32, 8);
    let cc = m.si_calloc(sz as u32, 8);
    let u = m.si_calloc(sz as u32, 4);
    for i in 0..sz.max(0) {
        let r = m.si_calloc(sz as u32, 8);
        m.set_u32(u + 4 * i as u32, r);
    }
    let l = m.si_calloc(sz as u32, 4);
    for i in 0..sz.max(0) {
        let r = m.si_calloc(sz as u32, 8);
        m.set_u32(l + 4 * i as u32, r);
    }
    setup_open_system(m, knots, sz, a, b, cc);
    lu_decomp_tridiagonal(m, a, b, cc, sz, l, u);
    let dst = m.si_calloc(nk as u32, 0x10);
    for i in 0..nk.max(0) {
        for (k, v) in [0.0f32, 0.0, 0.0, 1.0].into_iter().enumerate() {
            set4(m, dst, i, k as u32, v);
        }
    }
    let src = m.si_calloc(nk as u32, 0x10);
    for i in 0..nk.max(0) {
        for (k, v) in [0.0f32, 0.0, 0.0, 1.0].into_iter().enumerate() {
            set4(m, src, i, k as u32, v);
        }
    }
    let out = nurbs_crv_allocate(m).addr();
    let np = n.wrapping_add(2);
    let pts = nurbs_points_allocate(m, np);
    m.set_u32(out + crv::POINTS, pts);
    m.set_i32(out + crv::NB_POINTS, np);
    let old = m.u32(c + crv::POINTS);
    for i in 0..n.max(0) {
        let p = old.wrapping_add((i as u32).wrapping_mul(0x24));
        for k in 0..4u32 {
            let v = m.f64(p + 8 * k) as f32;
            set4(m, src, i, k, v);
        }
    }
    setup_open_input(m, knots, src, dst, n);
    if flag != 0 {
        for k in 0..4u32 {
            set4(m, dst, 0, k, t0[k as usize]);
            set4(m, dst, n - 1, k, t1[k as usize]);
        }
    }
    solve_tridiagonal_lu(m, u, l, dst, out, n);
    let np = m.u32(out + crv::POINTS);
    for k in 0..4u32 {
        let v = m.f32(src.wrapping_add(4 * k));
        m.set_f64(np + 8 * k, v as f64);
    }
    let lastp = np.wrapping_add(((n + 1) as u32).wrapping_mul(0x24));
    for k in 0..4u32 {
        let v = m.f32(src.wrapping_add(((n - 1) as u32).wrapping_mul(16)).wrapping_add(4 * k));
        m.set_f64(lastp + 8 * k, v as f64);
    }
    for p in [knots, a, b, cc] {
        m.free(p);
    }
    for i in 0..sz.max(0) {
        let r = m.u32(u + 4 * i as u32);
        if r != 0 {
            m.free(r);
            m.set_u32(u + 4 * i as u32, 0);
        }
        let r = m.u32(l + 4 * i as u32);
        if r != 0 {
            m.free(r);
            m.set_u32(l + 4 * i as u32, 0);
        }
    }
    for p in [u, l, dst, src] {
        m.free(p);
    }
    let v = m.u32(c);
    m.set_u32(out, v);
    let v = m.u32(c + crv::UNIFORMITY);
    m.set_u32(out + crv::UNIFORMITY, v);
    out
}
