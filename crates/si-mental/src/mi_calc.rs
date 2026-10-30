// SPDX-License-Identifier: MIT
//! `mi_calc.c`: the translator's vector and 4x4 matrix helpers (package 63, finding 0042).
//!
//! Everything is the x87's: a `float` is loaded exactly into an 80-bit register, sums, products and
//! quotients stay in 80 bits, and a store rounds to `float`. Values the original keeps in a register
//! between two stores are kept here as [`F80`]. Where the original stores a partial result into a
//! `float` local and reloads it, the value is rounded to `float` first. `sin` and `cos` are the C
//! library's ([`si_x87::trig`]). The routines read and write the image in the original's order, so a
//! result that overlaps an operand comes out as it does there.
//!
//! `sdebug` and `swarning` are other-unit callees (file `mi_error.c`, ported in [`crate::mi_error`]):
//! called through [`Outside`] with the address of the message.

use si_mem::{Mem, Outside, Routine};
use si_x87::{trig, F80};

/// `sdebug (0x08598cdc)`.
pub const SDEBUG: Routine = Routine::new("sdebug", 0x0859_8cdc);
/// `swarning (0x08598c44)`.
pub const SWARNING: Routine = Routine::new("swarning", 0x0859_8c44);

fn ld(m: &Mem, a: u32) -> F80 {
    F80::from_f32(m.f32(a))
}

fn st(m: &mut Mem, a: u32, v: F80) {
    m.set_f32(a, v.to_f32());
}

/// `fstp f32` then `fld f32`: what a spill to a `float` local does to a register.
fn spill(v: F80) -> F80 {
    F80::from_f32(v.to_f32())
}

/// `vector_plus_vector (0x08598270)`: `out = a + b`, three floats.
/// [soft 0x08598270 vector_plus_vector]
pub fn vector_plus_vector(m: &mut Mem, out: u32, a: u32, b: u32) {
    for i in 0..3 {
        let v = ld(m, a + 4 * i) + ld(m, b + 4 * i);
        st(m, out + 4 * i, v);
    }
}

/// `vector_minus_vector (0x08598298)`: `out = a - b`.
/// [soft 0x08598298 vector_minus_vector]
pub fn vector_minus_vector(m: &mut Mem, out: u32, a: u32, b: u32) {
    for i in 0..3 {
        let v = ld(m, a + 4 * i) - ld(m, b + 4 * i);
        st(m, out + 4 * i, v);
    }
}

/// `vector_X_scalar (0x085982c0)`: `v *= s` (the scalar is the float argument).
/// [soft 0x085982c0 vector_X_scalar]
pub fn vector_x_scalar(m: &mut Mem, v: u32, s: f32) {
    let s = F80::from_f32(s);
    for i in 0..3 {
        let r = ld(m, v + 4 * i) * s;
        st(m, v + 4 * i, r);
    }
}

/// `vector_dot_vector (0x085982e8)`: `(a0*b0 + a1*b1) + a2*b2`, left in `st0` unrounded.
/// [soft 0x085982e8 vector_dot_vector]
pub fn vector_dot_vector(m: &Mem, a: u32, b: u32) -> F80 {
    let p = |i: u32| ld(m, a + 4 * i) * ld(m, b + 4 * i);
    (p(0) + p(1)) + p(2)
}

/// `vector_X_vector (0x08598310)`: the cross product `out = a x b`; each component is stored before the next is read.
/// [soft 0x08598310 vector_X_vector]
pub fn vector_x_vector(m: &mut Mem, out: u32, a: u32, b: u32) {
    let c = |m: &Mem, i: u32, j: u32| ld(m, a + 4 * i) * ld(m, b + 4 * j);
    let v = c(m, 1, 2) - c(m, 2, 1);
    st(m, out, v);
    let v = c(m, 2, 0) - c(m, 0, 2);
    st(m, out + 4, v);
    let v = c(m, 0, 1) - c(m, 1, 0);
    st(m, out + 8, v);
}

/// `normalize_vector (0x0859835c)`: `v /= |v|`; a length of 1e-9 or less (or a NaN-free compare that is not greater)
/// prints "failed to normalize a vector" through `sdebug` and leaves `(0, 0, 1)`. The length is rounded to float first.
/// [soft 0x0859835c normalize_vector]
pub fn normalize_vector(m: &mut Mem, o: &mut dyn Outside, v: u32) {
    let (x, y, z) = (ld(m, v), ld(m, v + 4), ld(m, v + 8));
    let sum = ((y * y) + (x * x)) + (z * z);
    let len = spill(sum.sqrt());
    // `fcom` against 1e-9, then `jae` over "equal or less": greater and unordered divide
    if len.gt(F80::from_f64(1e-9)) || len.unordered(F80::from_f64(1e-9)) {
        st(m, v, x / len);
        st(m, v + 4, y / len);
        st(m, v + 8, z / len);
    } else {
        o.call(m, SDEBUG, &[0x086f_53a0]);
        m.set_u32(v, 0);
        m.set_u32(v + 4, 0);
        m.set_u32(v + 8, 0x3f80_0000);
    }
}

/// `init_matrix (0x085983ec)`: the identity, sixteen floats.
/// [soft 0x085983ec init_matrix]
pub fn init_matrix(m: &mut Mem, mat: u32) {
    for r in 0..4u32 {
        for c in 0..4u32 {
            m.set_u32(mat + 16 * r + 4 * c, if r == c { 0x3f80_0000 } else { 0 });
        }
    }
}

/// `vector_X_matrix (0x08598430)`: `out = v * mat` for a point (`v0*m[0] + v1*m[4] + v2*m[8] + m[12]`, and so on).
/// [soft 0x08598430 vector_X_matrix]
pub fn vector_x_matrix(m: &mut Mem, out: u32, v: u32, mat: u32) {
    for c in 0..3u32 {
        let r = ((ld(m, v) * ld(m, mat + 4 * c)) + (ld(m, v + 4) * ld(m, mat + 0x10 + 4 * c))) + (ld(m, v + 8) * ld(m, mat + 0x20 + 4 * c));
        let r = r + ld(m, mat + 0x30 + 4 * c);
        st(m, out + 4 * c, r);
    }
}

/// `matrix_X_matrix (0x0859849c)`: `out = a * b`, row by row, each element stored before the next is read.
/// [soft 0x0859849c matrix_X_matrix]
pub fn matrix_x_matrix(m: &mut Mem, out: u32, a: u32, b: u32) {
    for r in 0..4u32 {
        for c in 0..4u32 {
            let t = |m: &Mem, k: u32| ld(m, a + 16 * r + 4 * k) * ld(m, b + 16 * k + 4 * c);
            let v = ((t(m, 0) + t(m, 1)) + t(m, 2)) + t(m, 3);
            st(m, out + 16 * r + 4 * c, v);
        }
    }
}

/// `rotate_matrix (0x085986f4)`: the rotation matrix of three angles (radians, `rot[0..3]`), with `sin` and `cos` of the C library
/// rounded to float: row 0 `(cy*cz, cy*sz, -sy)`, rows 1 and 2 from `sx`, `cx`; the last row and column are the identity's.
/// [soft 0x085986f4 rotate_matrix]
pub fn rotate_matrix(m: &mut Mem, out: u32, rot: u32) {
    let sc = |m: &Mem, i: u32| {
        let x = m.f32(rot + 4 * i) as f64;
        (spill(trig::sin(x)), spill(trig::cos(x)))
    };
    let (sx, cx) = sc(m, 0);
    let (sy, cy) = sc(m, 1);
    let (sz, cz) = sc(m, 2);
    st(m, out, cy * cz);
    st(m, out + 4, cy * sz);
    st(m, out + 8, -sy);
    let t1 = sx * sy;
    st(m, out + 0x10, (t1 * cz) - (cx * sz));
    st(m, out + 0x14, (t1 * sz) + (cx * cz));
    st(m, out + 0x18, sx * cy);
    let t2 = cx * sy;
    st(m, out + 0x20, (sx * sz) + (t2 * cz));
    st(m, out + 0x24, (sz * t2) - (cz * sx));
    for off in [0x2c, 0x1c, 0xc, 0x38, 0x34] {
        m.set_u32(out + off, 0);
    }
    st(m, out + 0x28, cx * cy);
    m.set_u32(out + 0x30, 0);
    m.set_u32(out + 0x3c, 0x3f80_0000);
}

/// `invert_matrix (0x0859883c)`: the inverse of an affine matrix by cofactors (the upper 3x3 part, then the translation row).
/// Answers 1; a zero determinant prints "cannot invert singular object transform matrix" through `swarning` and answers 0
/// (the cofactors already written to `out` stay).
/// [soft 0x0859883c invert_matrix]
pub fn invert_matrix(m: &mut Mem, o: &mut dyn Outside, out: u32, inp: u32) -> u32 {
    let a = |m: &Mem, i: u32| ld(m, inp + 4 * i);
    // cofactors; the ones the original keeps in registers stay 80-bit, the others go through a float local
    let t1 = (a(m, 5) * a(m, 10)) - (a(m, 6) * a(m, 9));
    st(m, out, t1);
    let p = spill(a(m, 9) * a(m, 2));
    let t2 = p - (a(m, 10) * a(m, 1));
    let l14 = spill(t2);
    st(m, out + 4, t2);
    let t3 = (a(m, 1) * a(m, 6)) - (a(m, 2) * a(m, 5));
    st(m, out + 8, t3);
    let t4 = (a(m, 6) * a(m, 8)) - (a(m, 4) * a(m, 10));
    st(m, out + 0x10, t4);
    let t5 = (a(m, 10) * a(m, 0)) - (a(m, 8) * a(m, 2));
    st(m, out + 0x14, t5);
    let p = spill(a(m, 2) * a(m, 4));
    let t6 = p - (a(m, 0) * a(m, 6));
    let l18 = spill(t6);
    st(m, out + 0x18, t6);
    let t7 = (a(m, 4) * a(m, 9)) - (a(m, 5) * a(m, 8));
    st(m, out + 0x20, t7);
    let p = spill(a(m, 8) * a(m, 1));
    let t8 = p - (a(m, 9) * a(m, 0));
    let l1c = spill(t8);
    st(m, out + 0x24, t8);
    let p = spill(a(m, 0) * a(m, 5));
    let t9 = p - (a(m, 1) * a(m, 4));
    let l20 = spill(t9);
    st(m, out + 0x28, t9);
    let det = ((a(m, 0) * t1) + (a(m, 1) * t4)) + (a(m, 2) * t7);
    if F80::ZERO.compare(det) == Some(core::cmp::Ordering::Equal) {
        o.call(m, SWARNING, &[0x086f_53c0]);
        return 0;
    }
    let mut r = [F80::ZERO; 11];
    for (i, num) in [(0, t1), (1, l14), (2, t3), (4, t4), (5, t5), (6, l18), (8, t7), (9, l1c), (10, l20)] {
        let q = num / det;
        st(m, out + 4 * i as u32, q);
        r[i] = spill(q);
    }
    for c in 0..2u32 {
        let v = ((-r[c as usize] * a(m, 12)) - (r[4 + c as usize] * a(m, 13))) - (r[8 + c as usize] * a(m, 14));
        st(m, out + 0x30 + 4 * c, v);
    }
    let v = ((-r[2] * a(m, 12)) - (r[6] * a(m, 13))) - (r[10] * a(m, 14));
    for off in [0xc, 0x1c, 0x2c] {
        m.set_u32(out + off, 0);
    }
    m.set_u32(out + 0x3c, 0x3f80_0000);
    st(m, out + 0x38, v);
    1
}

/// `matrixZAlign (0x08598a60)`: the matrix whose third row is the vector and whose first row is `(z, 0, -x)` normalised
/// (a near-vertical vector, `x*x + z*z <= 0.0001`, gets the fixed rows `(0, 0, +-1)` by the sign of `y`).
/// [soft 0x08598a60 matrixZAlign]
pub fn matrix_z_align(m: &mut Mem, v: u32, out: u32) {
    init_matrix(m, out);
    let (x, y, z) = (ld(m, v), ld(m, v + 4), ld(m, v + 8));
    let t1 = -x;
    let sum = (t1 * t1) + ((z * z) + F80::ZERO);
    let lim = F80::from_f64(0.0001);
    if sum.gt(lim) || sum.unordered(lim) {
        let k = spill(F80::ONE / sum.sqrt());
        let t2 = t1 * k;
        let t3 = k * z;
        let t4 = F80::ZERO * k;
        let t5 = (y * t2) - (z * t4);
        let t6 = (z * t3) - (x * t2);
        let t7 = (x * t4) - (y * t3);
        st(m, out, t3);
        st(m, out + 4, t4);
        st(m, out + 8, t2);
        st(m, out + 0x10, t5);
        st(m, out + 0x14, t6);
        st(m, out + 0x18, t7);
        for i in 0..3u32 {
            let c = ld(m, v + 4 * i);
            st(m, out + 0x20 + 4 * i, c);
        }
    } else if y.lt(F80::ZERO) {
        m.set_u32(out + 0x28, 0);
        m.set_u32(out + 0x14, 0);
        m.set_u32(out + 0x18, 0x3f80_0000);
        m.set_u32(out + 0x24, 0xbf80_0000);
    } else {
        m.set_u32(out + 0x28, 0);
        m.set_u32(out + 0x14, 0);
        m.set_u32(out + 0x18, 0xbf80_0000);
        m.set_u32(out + 0x24, 0x3f80_0000);
    }
}
