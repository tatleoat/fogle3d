// SPDX-License-Identifier: MIT
//! nurbs13: the linear systems of the skin fit (`cross.c`) on the memory image (finding 0025, section 33).
//!
//! The open-curve helpers (setup_open_system, LU_decomp_tridiagonal, setup_open_input, solve_tridiagonal_LU) are the ones
//! `si-model` ports and cites (`cross_image.rs`); this crate cannot use `si-model` (it depends on this one), so the bodies are
//! repeated here without the cite and are covered by the check of `SI_skin2NurbsFarin` through `geom13`. The closed-curve ones
//! (setup_closed_system, setup_closed_input, LU_decomp, solve_LU) are ported here.

use si_hrc::image::nurbs::crv;
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

/// The tridiagonal system of an open curve's interpolation (setup_open_system, as in si-model):
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

/// LU decomposition of that tridiagonal matrix (LU_decomp_tridiagonal, as in si-model): `l` and
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

/// The right-hand side of the open interpolation (setup_open_input, as in si-model): `src` holds
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

/// Forward and back substitution of the tridiagonal system (solve_tridiagonal_LU, as in si-model):
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


fn row(m: &Mem, p: u32, i: i32) -> u32 {
    m.u32(p.wrapping_add((i as u32).wrapping_mul(4)))
}

fn el(r: u32, j: i32) -> u32 {
    r.wrapping_add((j as u32).wrapping_mul(8))
}

/// The cyclic system of a closed curve's interpolation [soft 0x08147070 setup_closed_system]: from the `n + 1` knots `k`
/// and the size `n`, the matrix `mat` (`n` row pointers to `n` doubles; a cyclic band of three). For row `i` the knot
/// differences `d7 = k[i+1]-k[i]`, `d2` (the next one, wrapping to `k[1]-k[0]`), `d6` (the previous one, wrapping to the
/// last) and `d5` (the one before, wrapping) are taken in the register; the left entry (column `i-1`, or `n-1` in row 0)
/// is `d7² / (d7+d6+d5)`, the diagonal `(d5+d6)·d7 / (d7+d6+d5)` plus `(d2+d7)·d6 / (d7+d6+d2)` (the first stored as a
/// double, the second added to it), the right entry (column `i+1`, or 0 in the last row) `d6² / (d7+d6+d2)`; stored in that
/// order, each as a double.
pub fn setup_closed_system(m: &mut Mem, k: u32, n: i32, mat: u32) {
    let mut i = 0;
    while i < n {
        let d7 = kn(m, k, i + 1) - kn(m, k, i);
        let d2 = if i + 2 > n { kn(m, k, 1) - kn(m, k, 0) } else { kn(m, k, i + 2) - kn(m, k, i + 1) };
        let d6 = if i - 1 < 0 { kn(m, k, n - i) - kn(m, k, n - i - 1) } else { kn(m, k, i) - kn(m, k, i - 1) };
        let d5 = if i - 2 < 0 { kn(m, k, n - i - 1) - kn(m, k, n - i - 2) } else { kn(m, k, i - 1) - kn(m, k, i - 2) };
        let a1 = d7 + d6;
        let inv3 = x(1.0f64) / (a1 + d5);
        let r = row(m, mat, i);
        let left = if i > 0 { i - 1 } else { n - 1 };
        let t = (d7 * d7) * inv3;
        m.set_f64(el(r, left), t.to_f64());
        let u = d5 + d6;
        let c = a1 + d2;
        let p = (u * d7) * inv3;
        let inv4 = x(1.0f64) / c;
        m.set_f64(el(r, i), p.to_f64());
        let e = d2 + d7;
        let q = (e * d6) * inv4;
        let sum = (q + x(m.f64(el(r, i)))).to_f64();
        m.set_f64(el(r, i), sum);
        let rc = if i < n - 1 { i + 1 } else { 0 };
        let right = inv4 * (d6 * d6);
        m.set_f64(el(r, rc), right.to_f64());
        i += 1;
    }
}

/// The right-hand side of the closed interpolation [soft 0x08147648 setup_closed_input]: row `i` of `dst` (floats) is the
/// sum of the two knot spans around knot `i` (`k[i+1]-k[i]` and `k[i]-k[i-1]`, the second `k[n]-k[n-1]` for row 0, in the
/// register) times the three floats of `src` row `i` (rounded to float one by one); the fourth float is not touched.
pub fn setup_closed_input(m: &mut Mem, k: u32, src: u32, dst: u32, n: i32) {
    let mut i = 0;
    while i < n {
        let first = kn(m, k, i + 1) - kn(m, k, i);
        let second = if i - 1 < 0 { kn(m, k, n) - kn(m, k, n - 1) } else { kn(m, k, i) - kn(m, k, i - 1) };
        let l = first + second;
        for c in 0..3u32 {
            set4(m, dst, i, c, (l * f32s(m, src, i, c)).to_f32());
        }
        i += 1;
    }
}

/// LU decomposition of that cyclic matrix [soft 0x08147c9c LU_decomp]: `a`, `l`, `u` are `n` row pointers to `n` doubles.
/// A Crout scheme: `l` takes the first column of `a`, `u` gets the first row divided by `l[0][0]` and a unit diagonal, then
/// for each `i` below the last one `l[i][i] = a[i][i] - Σ l[i][k]·u[k][i]`, row `i` of `u` and column `i` of `l` the same way
/// (the row of `u` divided by `l[i][i]`), and the last diagonal of `l` closes it. Every update `x - l·u` is computed in the
/// register (the product at 80 bits) and stored as a double.
pub fn lu_decomp(m: &mut Mem, a: u32, n: i32, l: u32, u: u32) {
    let sub = |m: &mut Mem, dst: u32, p: u32, q: u32| {
        let v = (x(m.f64(dst)) - x(m.f64(p)) * x(m.f64(q))).to_f64();
        m.set_f64(dst, v);
    };
    let (a0, l0, u0) = (row(m, a, 0), row(m, l, 0), row(m, u, 0));
    let v = m.f64(a0);
    m.set_f64(l0, v);
    m.set_f64(u0, 1.0);
    let mut i = 1;
    while i < n {
        let q = (x(m.f64(el(row(m, a, 0), i))) / x(m.f64(el(row(m, l, 0), 0)))).to_f64();
        m.set_f64(el(row(m, u, 0), i), q);
        let v = m.f64(el(row(m, a, i), 0));
        m.set_f64(el(row(m, l, i), 0), v);
        i += 1;
    }
    let mut i = 1;
    while i < n - 1 {
        let (ui, li, ai) = (row(m, u, i), row(m, l, i), row(m, a, i));
        m.set_f64(el(ui, i), 1.0);
        let v = m.f64(el(ai, i));
        m.set_f64(el(li, i), v);
        for k in 0..i {
            sub(m, el(li, i), el(li, k), el(row(m, u, k), i));
        }
        let mut j = i + 1;
        while j < n {
            let v = m.f64(el(ai, j));
            m.set_f64(el(ui, j), v);
            for k in 0..i {
                sub(m, el(ui, j), el(li, k), el(row(m, u, k), j));
            }
            let q = (x(m.f64(el(ui, j))) / x(m.f64(el(li, i)))).to_f64();
            m.set_f64(el(ui, j), q);
            let (lj, aj) = (row(m, l, j), row(m, a, j));
            let v = m.f64(el(aj, i));
            m.set_f64(el(lj, i), v);
            for k in 0..i {
                sub(m, el(lj, i), el(lj, k), el(row(m, u, k), i));
            }
            j += 1;
        }
        i += 1;
    }
    let ending = i;
    let r = n - 1;
    let (ur, lr, ar) = (row(m, u, r), row(m, l, r), row(m, a, r));
    m.set_f64(el(ur, r), 1.0);
    let v = m.f64(el(ar, r));
    m.set_f64(el(lr, r), v);
    for k in 0..ending {
        sub(m, el(lr, r), el(lr, k), el(row(m, u, k), r));
    }
}

/// Forward and back substitution of the cyclic system [soft 0x081478d4 solve_LU]: `u` and `l` the row pointers of
/// [`lu_decomp`], `rhs` the right-hand side (`n` rows of floats), `c` the curve whose points (stride 0x24) receive the
/// answer in slots 0 to `n - 1` (x, y, z only). The forward pass keeps its vectors as floats (a block of `n` rows from
/// `SI_calloc`, the fourth float 1.0, freed at the end), the three sums as floats built up with the products at 80 bits;
/// the reciprocal of a diagonal is taken in the register; the back pass stores doubles.
pub fn solve_lu(m: &mut Mem, u: u32, l: u32, rhs: u32, c: u32, n: i32) {
    let z = m.si_calloc(n as u32, 0x10);
    for i in 0..n.max(0) {
        for (k, v) in [0.0f32, 0.0, 0.0, 1.0].into_iter().enumerate() {
            set4(m, z, i, k as u32, v);
        }
    }
    let inv = x(1.0f64) / x(m.f64(el(row(m, l, 0), 0)));
    for k in 0..3u32 {
        set4(m, z, 0, k, (inv * f32s(m, rhs, 0, k)).to_f32());
    }
    let mut i = 1;
    while i < n {
        let li = row(m, l, i);
        let mut s = [0.0f32; 3];
        for k in 0..i {
            for (cc, sc) in s.iter_mut().enumerate() {
                *sc = (f32s(m, z, k, cc as u32) * x(m.f64(el(li, k))) + x(*sc)).to_f32();
            }
        }
        let r = x(1.0f64) / x(m.f64(el(li, i)));
        let nr = -r;
        for (cc, sc) in s.iter().enumerate() {
            let v = (nr * x(*sc) + r * f32s(m, rhs, i, cc as u32)).to_f32();
            set4(m, z, i, cc as u32, v);
        }
        i += 1;
    }
    let pts = m.u32(c + crv::POINTS);
    let pt = |i: i32| pts.wrapping_add((i as u32).wrapping_mul(0x24));
    let r = x(1.0f64) / x(m.f64(el(row(m, u, n - 1), n - 1)));
    for cc in 0..3u32 {
        let v = (r * f32s(m, z, n - 1, cc)).to_f64();
        m.set_f64(pt(n - 1) + 8 * cc, v);
    }
    let mut i = n - 2;
    while i >= 0 {
        let ui = row(m, u, i);
        let mut s = [0.0f32; 3];
        let mut k = i + 1;
        while k < n {
            for (cc, sc) in s.iter_mut().enumerate() {
                *sc = (x(m.f64(el(ui, k))) * x(m.f64(pt(k) + 8 * cc as u32)) + x(*sc)).to_f32();
            }
            k += 1;
        }
        let r = x(1.0f64) / x(m.f64(el(ui, i)));
        let nr = -r;
        for (cc, sc) in s.iter().enumerate() {
            let v = (nr * x(*sc) + r * f32s(m, z, i, cc as u32)).to_f64();
            m.set_f64(pt(i) + 8 * cc as u32, v);
        }
        i -= 1;
    }
    m.free(z);
}
