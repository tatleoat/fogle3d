// SPDX-License-Identifier: MIT
//! Vectors of three doubles and 4 × 4 matrices of doubles: the original's `vector.c`
//! (0x0857c1a0–0x0857c8d2, 9 functions, all named `VC_…`).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Everything here works on **doubles**, so nothing is in the
//! "single" class: a product of two doubles needs 106 bits and the 64-bit register rounds it
//! before the store rounds again. Every port goes through [`F80`] and rounds with `.to_f64()`
//! exactly where the original stores.
//!
//! Conventions, all read from the code: vectors are rows (`v' = v × M`), translation is row 3
//! of the matrix, angles are radians, and the model transformation of
//! [`vc_mat4x4_create_model_xform`] is scaling, then rotation about X, then Y, then Z, then
//! translation.
//!
//! The functions return the original's integer status: 0 for success and one of the three
//! codes below. The names of the codes are ours; only the numbers are in the binary.

use crate::trig;
use crate::types::MatrixD;
use crate::x87::{fmov64, x, F80};

/// Status 0: success.
pub const VC_OK: i32 = 0;
/// Status 11 (0xb): [`vc_vec3_normalize`] was given a vector of length zero.
pub const VC_ERR_ZERO_LENGTH: i32 = 0xb;
/// Status 12 (0xc): [`vc_mat4x4_invert_upper_left`] found a determinant of zero.
pub const VC_ERR_SINGULAR: i32 = 0xc;
/// Status 13 (0xd): [`vc_mat4x4_extract_model_xform`] found a matrix row of length zero.
pub const VC_ERR_ZERO_SCALE: i32 = 0xd;

/// Scales `v` to length 1 and writes its former length to `len`
/// [soft 0x0857c1a0 VC_Vec3Normalize]. Returns [`VC_OK`] or [`VC_ERR_ZERO_LENGTH`].
///
/// The length is `sqrt((v0² + v1²) + v2²)` in the register; `len` gets it rounded to double.
/// If it compares equal to zero the vector is left alone and 11 is returned (`len` is then
/// +0.0). Otherwise **v[0] is divided by the unrounded 80-bit length, v[1] and v[2] by the
/// double just stored in `len`**, which the original reads back from memory: the three
/// components are not divided by the same number, and (1, 1, 0) comes out with two
/// different values. A NaN length is not "equal to zero": the divisions are made.
///
/// Aliasing: the original re-reads `*len` after storing v[0], so `len` must not point into
/// `v`; Rust's borrows guarantee that here. Class: model.
pub fn vc_vec3_normalize(v: &mut [f64; 3], len: &mut f64) -> i32 {
    let length = ((x(v[0]) * v[0] + x(v[1]) * v[1]) + x(v[2]) * v[2]).sqrt();
    *len = length.to_f64();
    if length.eq(F80::ZERO) {
        return VC_ERR_ZERO_LENGTH;
    }
    v[0] = (x(v[0]) / length).to_f64();
    v[1] = (x(v[1]) / *len).to_f64();
    v[2] = (x(v[2]) / *len).to_f64();
    VC_OK
}

/// True (the original's 1) when the two vectors are equal component by component, compared
/// exactly [soft 0x0857c1fc VC_Vec3Equal]. No tolerance; +0 equals −0; a NaN anywhere gives
/// false (0). Class: copy (three comparisons, no arithmetic).
pub fn vc_vec3_equal(a: &[f64; 3], b: &[f64; 3]) -> bool {
    (0..3).all(|i| x(b[i]).eq(x(a[i])))
}

/// The dot product of the row vector (v, ·) with column `j` of `m`, without the translation:
/// `(v0 m[0][j] + v1 m[1][j]) + v2 m[2][j]` in the register.
fn row_times_column(v: &[f64; 3], m: &MatrixD, j: usize) -> F80 {
    (x(v[0]) * m[0][j] + x(v[1]) * m[1][j]) + x(v[2]) * m[2][j]
}

/// The point `v` transformed by `m` with the perspective division: `(v, 1) × m`, the first
/// three components divided by the fourth [soft 0x0857c248 VC_Vec3Xform].
///
/// Each of the four sums is `((v0 m[0][j] + v1 m[1][j]) + v2 m[2][j]) + m[3][j]` in the
/// register; the three quotients are formed from the **unrounded** sums and rounded to
/// double once. A fourth component of zero is not checked (infinity or NaN).
///
/// Aliasing: the original stores each unrounded sum (rounded to double) into the output as
/// soon as it has it and reads `v` again for the next one, so with the output on top of the
/// input the later sums use overwritten components: that case is
/// [`vc_vec3_xform_in_place`]. This function is the case of distinct objects. Class: model.
pub fn vc_vec3_xform(v: &[f64; 3], m: &MatrixD) -> [f64; 3] {
    let sx = row_times_column(v, m, 0) + m[3][0];
    let sy = row_times_column(v, m, 1) + m[3][1];
    let sz = row_times_column(v, m, 2) + m[3][2];
    let w = row_times_column(v, m, 3) + m[3][3];
    [(sx / w).to_f64(), (sy / w).to_f64(), (sz / w).to_f64()]
}

/// What [soft 0x0857c248 VC_Vec3Xform] computes when its output pointer is its input
/// pointer: not the transformed point. The x sum overwrites v[0] (rounded to double) before
/// the y sum reads it, and so on; the divisor is formed from the three overwritten
/// components. The final quotients still use the unrounded sums.
pub fn vc_vec3_xform_in_place(v: &mut [f64; 3], m: &MatrixD) {
    let sx = row_times_column(v, m, 0) + m[3][0];
    v[0] = sx.to_f64();
    let sy = row_times_column(v, m, 1) + m[3][1];
    v[1] = sy.to_f64();
    let sz = row_times_column(v, m, 2) + m[3][2];
    v[2] = sz.to_f64();
    let w = row_times_column(v, m, 3) + m[3][3];
    *v = [(sx / w).to_f64(), (sy / w).to_f64(), (sz / w).to_f64()];
}

/// The direction `v` transformed by the upper-left 3 × 3 block of `m`: no translation, no
/// division [soft 0x0857c2e0 VC_Vec3XformSimple].
///
/// Component j is `(v0 m[0][j] + v1 m[1][j]) + v2 m[2][j]`, rounded to double once.
/// Aliasing as for [`vc_vec3_xform`]: each component is stored before the next is computed,
/// see [`vc_vec3_xform_simple_in_place`]. Class: model.
pub fn vc_vec3_xform_simple(v: &[f64; 3], m: &MatrixD) -> [f64; 3] {
    [row_times_column(v, m, 0).to_f64(), row_times_column(v, m, 1).to_f64(), row_times_column(v, m, 2).to_f64()]
}

/// What [soft 0x0857c2e0 VC_Vec3XformSimple] computes when its output pointer is its input
/// pointer: each component is overwritten before the next one is computed from it.
pub fn vc_vec3_xform_simple_in_place(v: &mut [f64; 3], m: &MatrixD) {
    for j in 0..3 {
        v[j] = row_times_column(v, m, j).to_f64();
    }
}

/// The identity matrix [soft 0x0857c344 VC_Mat4x4Identity]. Written as integer words, two
/// per double, in a double loop. Class: copy.
pub fn vc_mat4x4_identity() -> MatrixD {
    let mut m = [[0.0f64; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    m
}

/// The matrix product `a × b` [soft 0x0857c39c VC_Mat4x4Multiply].
///
/// A plain triple loop that accumulates **in memory**: element [i][j] starts as the double 0
/// and for k = 0..3 becomes `a[i][k] × b[k][j] + element`, the product and the sum formed in
/// the register and the result rounded to double at every step. That is neither double
/// arithmetic (the product is not rounded to double before the addition) nor the register
/// accumulation of the float routines. The sixteen results are built in a local matrix and
/// copied to the destination at the end, so in the original the destination may be either
/// operand. Class: model.
pub fn vc_mat4x4_multiply(a: &MatrixD, b: &MatrixD) -> MatrixD {
    let mut out = [[0.0f64; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            let mut acc = 0.0f64;
            for k in 0..4 {
                acc = (x(a[i][k]) * b[k][j] + acc).to_f64();
            }
            out[i][j] = acc;
        }
    }
    out
}

/// Inverts the upper-left 3 × 3 block of `m` into `out`; the fourth row and column of `out`
/// are copies of `m`'s [soft 0x0857c434 VC_Mat4x4InvertUpperLeft]. Returns [`VC_OK`] or
/// [`VC_ERR_SINGULAR`].
///
/// `out` first receives a copy of all of `m`. Then the nine cofactors are written into the
/// block in transposed position (the adjugate), each a difference of two products formed in
/// the register and rounded to double, the "minus" ones negated in the register. The
/// determinant is `C02 m[0][2] + (C01 m[0][1] + C00 m[0][0])` from the **unrounded**
/// cofactors of row 0 and is never stored. If it compares equal to zero, 12 is returned and
/// `out` keeps the copy of `m` with the undivided adjugate in its block. Otherwise each of
/// the nine stored doubles is divided by the 80-bit determinant and rounded again. A NaN
/// determinant is not "equal to zero" and divides.
///
/// Aliasing: `m` is read while `out` is being written, so the two must be different
/// matrices; Rust's borrows guarantee that here. Class: model.
pub fn vc_mat4x4_invert_upper_left(m: &MatrixD, out: &mut MatrixD) -> i32 {
    *out = *m; // rep movs: a copy of the bits
    let minor = |a: (usize, usize), b: (usize, usize), c: (usize, usize), d: (usize, usize)| x(m[a.0][a.1]) * m[b.0][b.1] - x(m[c.0][c.1]) * m[d.0][d.1];
    let c00 = minor((1, 1), (2, 2), (1, 2), (2, 1));
    out[0][0] = c00.to_f64();
    out[0][1] = (-minor((0, 1), (2, 2), (0, 2), (2, 1))).to_f64();
    out[0][2] = minor((0, 1), (1, 2), (0, 2), (1, 1)).to_f64();
    let c01 = -minor((1, 0), (2, 2), (1, 2), (2, 0));
    out[1][0] = c01.to_f64();
    out[1][1] = minor((0, 0), (2, 2), (0, 2), (2, 0)).to_f64();
    out[1][2] = (-minor((0, 0), (1, 2), (0, 2), (1, 0))).to_f64();
    let c02 = minor((1, 0), (2, 1), (1, 1), (2, 0));
    out[2][0] = c02.to_f64();
    out[2][1] = (-minor((0, 0), (2, 1), (0, 1), (2, 0))).to_f64();
    out[2][2] = minor((0, 0), (1, 1), (0, 1), (1, 0)).to_f64();
    let det = c02 * m[0][2] + (c01 * m[0][1] + c00 * m[0][0]);
    if F80::ZERO.eq(det) {
        return VC_ERR_SINGULAR;
    }
    for row in out.iter_mut().take(3) {
        for e in row.iter_mut().take(3) {
            *e = (x(*e) / det).to_f64();
        }
    }
    VC_OK
}

/// Splits a model matrix into translation, scaling and three rotation angles: the inverse
/// of [`vc_mat4x4_create_model_xform`] [soft 0x0857c570 VC_Mat4x4ExtractModelXform].
/// Returns [`VC_OK`] or [`VC_ERR_ZERO_SCALE`].
///
/// The matrix is `S × Rx × Ry × Rz` with the translation in row 3 (row vectors: scale first,
/// then turn about X, Y, Z, then move). Working on a local copy:
///
/// 1. `trans` = row 3, columns 0..2.
/// 2. For each row i of the 3 × 3 block: `scale[i]` = its length
///    `sqrt((r0² + r1²) + r2²)`, stored rounded to double; if the length compares equal to
///    zero the function **returns 13 at once** (`trans` and `scale[0..=i]` are written,
///    `rot` is not touched); otherwise the row is divided by the unrounded length.
/// 3. The determinant of the normalised block; if it is negative (0 > det; a NaN is not),
///    all three scales and all nine elements are negated. A mirrored matrix therefore comes
///    back as three negative scales, never as one.
/// 4. `rot[1]` = `asin(−r[0][2])`. Then `cos(rot[1])` is compared with zero:
///    - not equal (the general case): `rot[0]` = atan2(r[1][2], r[2][2]),
///      `rot[2]` = atan2(r[0][1], r[0][0]);
///    - equal (gimbal lock): `rot[0]` = atan2(r[1][0], r[1][1]) and `rot[2]` = 0.
///
/// The angles are radians, the atan2 is the unit's `fpatan` inline. The gimbal-lock branch
/// needs a cosine of exactly zero, which `cos` of the double nearest ±π/2 is not (it is
/// about 6e-17): in practice the branch is dead. At ±90 degrees about Y the general formulas
/// are applied instead: a matrix built by [`vc_mat4x4_create_model_xform`] carries that
/// 6e-17 as a common factor of the four elements involved and its angles still come back,
/// but a matrix with exact zeros there gets atan2(0, 0) = 0 for both other angles, whatever
/// its rotation about X was. Kept as it is.
///
/// The original copies the matrix first, so its outputs may overlap the matrix.
/// Class: libm.
pub fn vc_mat4x4_extract_model_xform(m: &MatrixD, trans: &mut [f64; 3], scale: &mut [f64; 3], rot: &mut [f64; 3]) -> i32 {
    let mut r = *m;
    for j in 0..3 {
        trans[j] = fmov64(r[3][j]);
    }
    for i in 0..3 {
        let len = ((x(r[i][0]) * r[i][0] + x(r[i][1]) * r[i][1]) + x(r[i][2]) * r[i][2]).sqrt();
        scale[i] = len.to_f64();
        if len.eq(F80::ZERO) {
            return VC_ERR_ZERO_SCALE;
        }
        for j in 0..3 {
            r[i][j] = (x(r[i][j]) / len).to_f64();
        }
    }
    let det = (x(r[2][1]) * r[1][0] - x(r[2][0]) * r[1][1]) * r[0][2]
        + ((x(r[1][1]) * r[2][2] - x(r[1][2]) * r[2][1]) * r[0][0] - (x(r[2][2]) * r[1][0] - x(r[1][2]) * r[2][0]) * r[0][1]);
    if F80::ZERO.gt(det) {
        for i in 0..3 {
            scale[i] = (-x(scale[i])).to_f64();
            for j in 0..3 {
                r[i][j] = (-x(r[i][j])).to_f64();
            }
        }
    }
    let ry = trig::asin((-x(r[0][2])).to_f64()).to_f64();
    rot[1] = ry;
    if F80::ZERO.eq(trig::cos(ry)) {
        rot[0] = trig::fpatan(x(r[1][0]), x(r[1][1])).to_f64();
        rot[2] = 0.0;
    } else {
        rot[0] = trig::fpatan(x(r[1][2]), x(r[2][2])).to_f64();
        rot[2] = trig::fpatan(x(r[0][1]), x(r[0][0])).to_f64();
    }
    VC_OK
}

/// Builds a model matrix from translation, scaling and three rotation angles in radians
/// [soft 0x0857c720 VC_Mat4x4CreateModelXform].
///
/// ```text
/// M = S × Rx × Ry × Rz, then row 3 ← (trans, 1)
///
/// Rx: 1  0  0     Ry: c  0 -s     Rz:  c  s  0
///     0  c  s         0  1  0         -s  c  0
///     0 -s  c         s  0  c          0  0  1
/// ```
///
/// With row vectors that is: scale, turn about X, then Y, then Z, then translate. Each `cos`
/// and `sin` (the double functions, called once per angle) is rounded to double when stored
/// into its matrix; the second cosine entry is a copy of the first. The three products go
/// through [`vc_mat4x4_multiply`]. The translation is written over row 3 of the product
/// afterwards, it is not multiplied in.
///
/// The original clears its output before it reads the inputs, so the output must not
/// overlap them. Class: libm.
pub fn vc_mat4x4_create_model_xform(trans: &[f64; 3], scale: &[f64; 3], rot: &[f64; 3]) -> MatrixD {
    let mut s = vc_mat4x4_identity();
    let mut rx = vc_mat4x4_identity();
    let mut ry = vc_mat4x4_identity();
    let mut rz = vc_mat4x4_identity();
    for i in 0..3 {
        s[i][i] = fmov64(scale[i]);
    }
    let (c, sn) = (trig::cos(rot[0]).to_f64(), trig::sin(rot[0]));
    rx[1][1] = c;
    rx[1][2] = sn.to_f64();
    rx[2][1] = (-sn).to_f64();
    rx[2][2] = c;
    let (c, sn) = (trig::cos(rot[1]).to_f64(), trig::sin(rot[1]));
    ry[0][0] = c;
    ry[2][0] = sn.to_f64();
    ry[2][2] = c;
    ry[0][2] = (-sn).to_f64();
    let (c, sn) = (trig::cos(rot[2]).to_f64(), trig::sin(rot[2]));
    rz[0][0] = c;
    rz[0][1] = sn.to_f64();
    rz[1][0] = (-sn).to_f64();
    rz[1][1] = c;
    let mut m = vc_mat4x4_multiply(&s, &rx);
    m = vc_mat4x4_multiply(&m, &ry);
    m = vc_mat4x4_multiply(&m, &rz);
    for j in 0..3 {
        m[3][j] = fmov64(trans[j]);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::{FRAC_PI_2, PI};

    const P30: f64 = 1073741824.0; // 2^30
    const P32: f64 = 4294967296.0; // 2^32

    fn close(a: &[f64], b: &[f64], tol: f64) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (p - q).abs() <= tol)
    }

    fn flat(m: &MatrixD) -> Vec<f64> {
        m.iter().flatten().copied().collect()
    }

    #[test]
    fn normalize_values_and_zero() {
        let (mut v, mut len) = ([3.0, 4.0, 0.0], -1.0);
        assert_eq!(vc_vec3_normalize(&mut v, &mut len), VC_OK);
        assert_eq!((v, len), ([0.6, 0.8, 0.0], 5.0));
        let (mut v, mut len) = ([0.0, -0.0, 0.0], -1.0);
        assert_eq!(vc_vec3_normalize(&mut v, &mut len), VC_ERR_ZERO_LENGTH);
        assert_eq!(len.to_bits(), 0); // +0.0 written, the vector untouched
        assert_eq!(v.map(f64::to_bits), [0, 0x8000_0000_0000_0000, 0]);
        // the squares do not underflow in the register as they would in doubles: 1e-340 and
        // even 2^-2148 (the square of the smallest double) are register values
        for tiny in [1e-170, f64::from_bits(1)] {
            let (mut v, mut len) = ([tiny, 0.0, 0.0], -1.0);
            assert_eq!(vc_vec3_normalize(&mut v, &mut len), VC_OK);
            assert_eq!((v[0], len), (1.0, tiny));
        }
        // a NaN is not a zero length: the divisions are made
        let (mut v, mut len) = ([f64::NAN, 1.0, 2.0], 0.0);
        assert_eq!(vc_vec3_normalize(&mut v, &mut len), VC_OK);
        assert!(len.is_nan() && v.iter().all(|c| c.is_nan()));
    }

    #[test]
    fn normalize_divides_the_first_component_by_the_unrounded_length() {
        // (1, 1, 0): the length is √2. v[0] = 1/√2 with √2 to 64 bits: 0.70710678118654752440,
        // above the midpoint of the two neighbouring doubles, so the upper one. v[1] divides
        // by the double √2 (which is 9.7e-17 too large): 0.70710678118654747608, the lower one.
        let (mut v, mut len) = ([1.0, 1.0, 0.0], 0.0);
        assert_eq!(vc_vec3_normalize(&mut v, &mut len), VC_OK);
        assert_eq!(len.to_bits(), 0x3ff6_a09e_667f_3bcd);
        assert_eq!(v[0].to_bits(), 0x3fe6_a09e_667f_3bcd);
        assert_eq!(v[1].to_bits(), 0x3fe6_a09e_667f_3bcc);
        assert_eq!(v[2], 0.0);
        assert_eq!(v[1], 1.0 / 2.0f64.sqrt()); // the second is what double arithmetic gives
        assert_ne!(v[0], v[1]);
    }

    #[test]
    fn equal_is_exact() {
        assert!(vc_vec3_equal(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]));
        assert!(vc_vec3_equal(&[0.0, 2.0, 3.0], &[-0.0, 2.0, 3.0]));
        assert!(!vc_vec3_equal(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0000000000000004]));
        assert!(!vc_vec3_equal(&[1.0, 2.0, 3.0], &[1.0, 2.5, 3.0]));
        assert!(!vc_vec3_equal(&[1.5, 2.0, 3.0], &[1.0, 2.0, 3.0]));
        assert!(!vc_vec3_equal(&[f64::NAN, 2.0, 3.0], &[f64::NAN, 2.0, 3.0]));
    }

    #[test]
    fn xform_values() {
        let id = vc_mat4x4_identity();
        assert_eq!(vc_vec3_xform(&[1.0, 2.0, 3.0], &id), [1.0, 2.0, 3.0]);
        assert_eq!(vc_vec3_xform_simple(&[1.0, 2.0, 3.0], &id), [1.0, 2.0, 3.0]);
        // rows are the images of the axes, row 3 is the translation
        let m = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0], [10.0, 20.0, 30.0, 1.0]];
        assert_eq!(vc_vec3_xform(&[1.0, 2.0, 3.0], &m), [8.0, 21.0, 36.0]);
        assert_eq!(vc_vec3_xform_simple(&[1.0, 2.0, 3.0], &m), [-2.0, 1.0, 6.0]);
        // the fourth column divides (xform only)
        let mut p = m;
        (p[0][3], p[3][3]) = (1.0, 3.0); // w = x + 3
        assert_eq!(vc_vec3_xform(&[1.0, 2.0, 3.0], &p), [2.0, 5.25, 9.0]);
        assert_eq!(vc_vec3_xform_simple(&[1.0, 2.0, 3.0], &p), [-2.0, 1.0, 6.0]);
        // w = 0 is not checked
        p[3][3] = -1.0;
        let r = vc_vec3_xform(&[1.0, 2.0, 3.0], &p);
        assert!(r[0].is_infinite() && r[1].is_infinite() && r[2].is_infinite());
    }

    #[test]
    fn xform_accumulates_in_the_register_left_to_right() {
        // (2^60 + 1) − 2^60 is 1 in the register, 0 in double arithmetic
        let mut m = [[0.0f64; 4]; 4];
        (m[0][0], m[1][0], m[2][0], m[3][3]) = (P30, 1.0, P30, 1.0);
        assert_eq!(vc_vec3_xform_simple(&[P30, 1.0, -P30], &m)[0], 1.0);
        assert_eq!(vc_vec3_xform(&[P30, 1.0, -P30], &m)[0], 1.0);
        // (2^64 + 1) does not fit 64 bits and rounds to 2^64: left to right gives 0, any
        // other order 1
        (m[0][0], m[2][0]) = (P32, P32);
        assert_eq!(vc_vec3_xform_simple(&[P32, 1.0, -P32], &m)[0], 0.0);
        assert_eq!(vc_vec3_xform(&[P32, 1.0, -P32], &m)[0], 0.0);
        // the translation is added last: with the products 2^64, 0, −2^64 and m[3][0] = 1 the
        // sum is ((2^64 + 0) − 2^64) + 1 = 1 (added first, the 1 would be lost)
        (m[1][0], m[3][0]) = (0.0, 1.0);
        assert_eq!(vc_vec3_xform(&[P32, 1.0, -P32], &m)[0], 1.0);
        // the quotient is formed from the unrounded sum: 2^53 + 1 = 3 × 3002399751580331 is
        // exact in the register, so dividing by w = 3 gives that odd integer; from the sum
        // rounded to double (2^53) it would be 3002399751580330.5
        let big = 9007199254740992.0; // 2^53
        let mut q = [[0.0f64; 4]; 4];
        (q[0][0], q[3][0], q[3][3]) = (big, 1.0, 3.0);
        assert_eq!(vc_vec3_xform(&[1.0, 0.0, 0.0], &q)[0], 3002399751580331.0);
        assert_eq!((big + 1.0) / 3.0, 3002399751580330.5);
    }

    #[test]
    fn xform_in_place_reads_overwritten_components() {
        // a swap of x and y: separate objects give (2, 1, 3); in place the y sum reads the
        // x that was just overwritten with 2
        let m = [[0.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert_eq!(vc_vec3_xform(&[1.0, 2.0, 3.0], &m), [2.0, 1.0, 3.0]);
        assert_eq!(vc_vec3_xform_simple(&[1.0, 2.0, 3.0], &m), [2.0, 1.0, 3.0]);
        let mut v = [1.0, 2.0, 3.0];
        vc_vec3_xform_in_place(&mut v, &m);
        assert_eq!(v, [2.0, 2.0, 3.0]);
        let mut v = [1.0, 2.0, 3.0];
        vc_vec3_xform_simple_in_place(&mut v, &m);
        assert_eq!(v, [2.0, 2.0, 3.0]);
        // the divisor too comes from the overwritten vector: w = x' = 2 instead of 1
        let mut p = m;
        (p[0][3], p[3][3]) = (1.0, 0.0);
        assert_eq!(vc_vec3_xform(&[1.0, 2.0, 3.0], &p), [2.0, 1.0, 3.0]);
        let mut v = [1.0, 2.0, 3.0];
        vc_vec3_xform_in_place(&mut v, &p);
        assert_eq!(v, [1.0, 1.0, 1.5]);
    }

    #[test]
    fn multiply_values() {
        let id = vc_mat4x4_identity();
        assert_eq!(id, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        let a = [[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], [9.0, 10.0, 11.0, 12.0], [13.0, 14.0, 15.0, 16.0]];
        let b = [[2.0, 0.0, 0.0, 1.0], [0.0, 3.0, 0.0, 0.0], [1.0, 0.0, 4.0, 0.0], [0.0, 0.0, 0.0, 5.0]];
        assert_eq!(vc_mat4x4_multiply(&a, &id), a);
        assert_eq!(vc_mat4x4_multiply(&id, &a), a);
        assert_eq!(vc_mat4x4_multiply(&a, &b), [[5.0, 6.0, 12.0, 21.0], [17.0, 18.0, 28.0, 45.0], [29.0, 30.0, 44.0, 69.0], [41.0, 42.0, 60.0, 93.0]]);
        assert_eq!(vc_mat4x4_multiply(&b, &a)[0], [15.0, 18.0, 21.0, 24.0]);
    }

    #[test]
    fn multiply_rounds_to_double_at_every_step() {
        // row (2^30, 1, −2^30, 0) × column (2^30, 1, 2^30, 0): the running sum is stored as a
        // double after each term, so 2^60 + 1 becomes 2^60 and the result is 0. The register
        // accumulation of vc_vec3_xform_simple gives 1 for the same numbers.
        let mut a = [[0.0f64; 4]; 4];
        let mut b = [[0.0f64; 4]; 4];
        a[0] = [P30, 1.0, -P30, 0.0];
        (b[0][0], b[1][0], b[2][0]) = (P30, 1.0, P30);
        assert_eq!(vc_mat4x4_multiply(&a, &b)[0][0], 0.0);
        assert_eq!(vc_vec3_xform_simple(&[P30, 1.0, -P30], &b)[0], 1.0);
        // but the product itself is not rounded to double before it is added:
        // (2^27 + 1)² = 2^54 + 2^28 + 1 needs 55 bits; added to −2^54 in the register it
        // leaves 2^28 + 1, where double arithmetic (product rounded to 2^54 + 2^28) leaves 2^28
        let c = 134217729.0; // 2^27 + 1
        a[0] = [-18014398509481984.0, c, 0.0, 0.0]; // −2^54
        (b[0][0], b[1][0], b[2][0]) = (1.0, c, 0.0);
        assert_eq!(vc_mat4x4_multiply(&a, &b)[0][0], 268435457.0);
        assert_eq!(-18014398509481984.0 + c * c, 268435456.0);
    }

    #[test]
    fn invert_upper_left_values() {
        // a classic with determinant 1
        let mut m = [[1.0, 2.0, 3.0, 91.0], [0.0, 1.0, 4.0, 92.0], [5.0, 6.0, 0.0, 93.0], [94.0, 95.0, 96.0, 97.0]];
        let mut out = [[0.0f64; 4]; 4];
        assert_eq!(vc_mat4x4_invert_upper_left(&m, &mut out), VC_OK);
        // the fourth row and column are copies of the source, not an inverse
        assert_eq!(out, [[-24.0, 18.0, 5.0, 91.0], [20.0, -15.0, -4.0, 92.0], [-5.0, 4.0, 1.0, 93.0], [94.0, 95.0, 96.0, 97.0]]);
        m = [[2.0, 0.0, 0.0, 0.0], [0.0, 4.0, 0.0, 0.0], [0.0, 0.0, 8.0, 0.0], [1.0, 2.0, 3.0, 1.0]];
        assert_eq!(vc_mat4x4_invert_upper_left(&m, &mut out), VC_OK);
        assert_eq!(out, [[0.5, 0.0, 0.0, 0.0], [0.0, 0.25, 0.0, 0.0], [0.0, 0.0, 0.125, 0.0], [1.0, 2.0, 3.0, 1.0]]);
        // singular: status 12, and the block holds the undivided adjugate
        m = [[1.0, 2.0, 3.0, 7.0], [2.0, 4.0, 6.0, 7.0], [0.0, 0.0, 1.0, 7.0], [8.0, 8.0, 8.0, 9.0]];
        assert_eq!(vc_mat4x4_invert_upper_left(&m, &mut out), VC_ERR_SINGULAR);
        assert_eq!(out, [[4.0, -2.0, 0.0, 7.0], [-2.0, 1.0, 0.0, 7.0], [0.0, 0.0, 0.0, 7.0], [8.0, 8.0, 8.0, 9.0]]);
        // a NaN determinant is not zero: everything in the block becomes NaN, status 0
        m[0][0] = f64::NAN;
        assert_eq!(vc_mat4x4_invert_upper_left(&m, &mut out), VC_OK);
        assert!(out[1][1].is_nan() && out[3][3] == 9.0);
    }

    #[test]
    fn invert_upper_left_forms_cofactors_and_determinant_in_the_register() {
        // a = 2^30 + 1, b = 2^30: the cofactor a² − b² = 2^31 + 1 is exact in the register
        // (a² needs 61 bits); double arithmetic would round a² to 2^60 + 2^31 and get 2^31.
        // With m[0][0] = 1 the determinant is 2^31 + 1 too, and
        // out[1][1] = a/(2^31 + 1) = 0.5 (1 + 2^-30)/(1 + 2^-31) = 0.5 + 2^-32 − 2^-63 + …,
        // the double 0.5 + 2^-32. From a determinant of 2^31 it would be 0.5 + 2^-31.
        let a = P30 + 1.0;
        let m = [[1.0, 0.0, 0.0, 0.0], [0.0, a, P30, 0.0], [0.0, P30, a, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let mut out = [[0.0f64; 4]; 4];
        assert_eq!(vc_mat4x4_invert_upper_left(&m, &mut out), VC_OK);
        assert_eq!(out[0][0], 1.0);
        assert_eq!(out[1][1], 0.5 + 2.0f64.powi(-32));
        assert_eq!(out[2][2], 0.5 + 2.0f64.powi(-32));
        // −b/(2^31 + 1) = −0.5/(1 + 2^-31) = −0.5 + 2^-32 − 2^-63 + …
        assert_eq!(out[1][2], -0.5 + 2.0f64.powi(-32));
        assert_eq!(out[2][1], -0.5 + 2.0f64.powi(-32));
    }

    #[test]
    fn create_model_xform_layout_and_order() {
        // no rotation: the scales on the diagonal, the translation in row 3
        let m = vc_mat4x4_create_model_xform(&[5.0, 6.0, 7.0], &[2.0, 3.0, 4.0], &[0.0, 0.0, 0.0]);
        assert_eq!(m, [[2.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 4.0, 0.0], [5.0, 6.0, 7.0, 1.0]]);
        let one = [1.0, 1.0, 1.0];
        let zero = [0.0, 0.0, 0.0];
        let image = |m: &MatrixD, v: [f64; 3]| vc_vec3_xform(&v, m);
        let near = |a: [f64; 3], b: [f64; 3]| close(&a, &b, 1e-15);
        // radians, right-handed for row vectors: about X, Y goes to Z; about Y, Z to X; about
        // Z, X to Y
        assert!(near(image(&vc_mat4x4_create_model_xform(&zero, &one, &[FRAC_PI_2, 0.0, 0.0]), [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
        assert!(near(image(&vc_mat4x4_create_model_xform(&zero, &one, &[0.0, FRAC_PI_2, 0.0]), [0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]));
        assert!(near(image(&vc_mat4x4_create_model_xform(&zero, &one, &[0.0, 0.0, FRAC_PI_2]), [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
        // X acts before Y: Y → Z (about X) → X (about Y). The other order would leave Y → Z.
        assert!(near(image(&vc_mat4x4_create_model_xform(&zero, &one, &[FRAC_PI_2, FRAC_PI_2, 0.0]), [0.0, 1.0, 0.0]), [1.0, 0.0, 0.0]));
        // Y acts before Z: Z → X (about Y) → Y (about Z)
        assert!(near(image(&vc_mat4x4_create_model_xform(&zero, &one, &[0.0, FRAC_PI_2, FRAC_PI_2]), [0.0, 0.0, 1.0]), [0.0, 1.0, 0.0]));
        // X acts before Z: Y → Z (about X), which Z leaves alone
        assert!(near(image(&vc_mat4x4_create_model_xform(&zero, &one, &[FRAC_PI_2, 0.0, FRAC_PI_2]), [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
        // the scale acts before the rotation, the translation after everything
        let m = vc_mat4x4_create_model_xform(&[10.0, 20.0, 30.0], &[2.0, 3.0, 4.0], &[0.0, 0.0, FRAC_PI_2]);
        assert!(close(&image(&m, [1.0, 0.0, 0.0]), &[10.0, 22.0, 30.0], 1e-14));
        assert!(close(&image(&m, [0.0, 1.0, 0.0]), &[7.0, 20.0, 30.0], 1e-14));
        assert_eq!(m[3], [10.0, 20.0, 30.0, 1.0]);
        // the matrices as stored: sin and −sin of the same call, the cosine copied
        let a = 0.3f64;
        let m = vc_mat4x4_create_model_xform(&zero, &one, &[a, 0.0, 0.0]);
        assert!(close(&flat(&m), &[1.0, 0.0, 0.0, 0.0, 0.0, a.cos(), a.sin(), 0.0, 0.0, -a.sin(), a.cos(), 0.0, 0.0, 0.0, 0.0, 1.0], 1e-15));
        assert_eq!(m[1][2], -m[2][1]);
        assert_eq!(m[1][1], m[2][2]);
    }

    #[test]
    fn extract_model_xform_inverts_create() {
        let (mut t, mut s, mut r) = ([0.0; 3], [0.0; 3], [0.0; 3]);
        let m = vc_mat4x4_create_model_xform(&[5.0, 6.0, 7.0], &[2.0, 3.0, 4.0], &[0.0, 0.0, 0.0]);
        assert_eq!(vc_mat4x4_extract_model_xform(&m, &mut t, &mut s, &mut r), VC_OK);
        assert_eq!((t, s, r), ([5.0, 6.0, 7.0], [2.0, 3.0, 4.0], [0.0, 0.0, 0.0]));
        assert_eq!(r[1].to_bits(), 0x8000_0000_0000_0000); // asin(−0) = −0

        for (trans, scale, rot) in [
            ([1.0, -2.0, 3.0], [1.0, 1.0, 1.0], [0.3, -0.4, 0.5]),
            ([0.0, 0.0, 0.0], [2.0, 0.5, 3.0], [-1.2, 1.0, 2.9]),
            ([9.0, 8.0, 7.0], [0.1, 10.0, 1.0], [3.0, -1.5, -3.0]),
            ([0.0, 1.0, 0.0], [4.0, 4.0, 4.0], [0.0, 1.5, 0.0]),
        ] {
            let m = vc_mat4x4_create_model_xform(&trans, &scale, &rot);
            assert_eq!(vc_mat4x4_extract_model_xform(&m, &mut t, &mut s, &mut r), VC_OK);
            assert_eq!(t, trans);
            assert!(close(&s, &scale, 1e-14), "{s:?}");
            assert!(close(&r, &rot, 1e-13), "{r:?}");
        }
        // an angle about Y beyond ±90 degrees cannot come back: asin returns the supplement
        // and the other two angles absorb the difference; the matrix is the same
        let m = vc_mat4x4_create_model_xform(&[0.0; 3], &[1.0; 3], &[0.2, 2.0, 0.3]);
        assert_eq!(vc_mat4x4_extract_model_xform(&m, &mut t, &mut s, &mut r), VC_OK);
        assert!(close(&r, &[0.2 - PI, PI - 2.0, 0.3 - PI], 1e-13), "{r:?}");
        let back = vc_mat4x4_create_model_xform(&t, &s, &r);
        assert!(close(&flat(&back), &flat(&m), 1e-14));
    }

    #[test]
    fn extract_model_xform_mirror_zero_scale_and_gimbal_lock() {
        let (mut t, mut s, mut r) = ([9.0; 3], [9.0; 3], [9.0; 3]);
        // a mirror in X: negative determinant, so all three scales are negated and the
        // rotation becomes half a turn about X
        let m = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 2.0, 3.0, 1.0]];
        assert_eq!(vc_mat4x4_extract_model_xform(&m, &mut t, &mut s, &mut r), VC_OK);
        assert_eq!((t, s), ([1.0, 2.0, 3.0], [-1.0, -1.0, -1.0]));
        assert_eq!(r, [-PI, 0.0, 0.0]); // atan2(−0, −1) = −π
        let back = vc_mat4x4_create_model_xform(&t, &s, &r);
        assert!(close(&flat(&back), &flat(&m), 1e-15));
        // a negative scale on one axis and a rotation: same treatment
        let m = vc_mat4x4_create_model_xform(&[0.0; 3], &[1.0, -2.0, 3.0], &[0.3, 0.4, 0.5]);
        assert_eq!(vc_mat4x4_extract_model_xform(&m, &mut t, &mut s, &mut r), VC_OK);
        assert!(close(&s, &[-1.0, -2.0, -3.0], 1e-14), "{s:?}");
        assert!(close(&flat(&vc_mat4x4_create_model_xform(&t, &s, &r)), &flat(&m), 1e-14));

        // a zero row: status 13 at that row; later scales and the angles are not written
        let (mut t, mut s, mut r) = ([9.0; 3], [9.0; 3], [9.0; 3]);
        let m = [[2.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 2.0, 3.0, 1.0]];
        assert_eq!(vc_mat4x4_extract_model_xform(&m, &mut t, &mut s, &mut r), VC_ERR_ZERO_SCALE);
        assert_eq!((t, s, r), ([1.0, 2.0, 3.0], [2.0, 0.0, 9.0], [9.0; 3]));

        // 90 degrees about Y: the cosine of the double nearest π/2 is 6.1e-17, not zero, so
        // the gimbal-lock branch is not taken. A matrix made by create keeps that 6.1e-17 as
        // a common factor of the four elements the general formulas divide, and the angles
        // come back (the lock branch would have returned a Z angle of 0)
        let m = vc_mat4x4_create_model_xform(&[0.0; 3], &[1.0; 3], &[0.3, FRAC_PI_2, 0.1]);
        let (mut t, mut s, mut r) = ([0.0; 3], [0.0; 3], [0.0; 3]);
        assert_eq!(vc_mat4x4_extract_model_xform(&m, &mut t, &mut s, &mut r), VC_OK);
        assert!(close(&r, &[0.3, FRAC_PI_2, 0.1], 1e-13), "{r:?}");
        // a matrix with exact zeros there (an exact quarter turn about Y after a turn of 0.3
        // about X) takes the general branch too and gets atan2(0, 0) = 0 twice: the turn
        // about X is lost. The lock branch would have found it as atan2(r[1][0], r[1][1]).
        let (sx, cx) = (0.3f64.sin(), 0.3f64.cos());
        let m = [[0.0, 0.0, -1.0, 0.0], [sx, cx, 0.0, 0.0], [cx, -sx, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert_eq!(vc_mat4x4_extract_model_xform(&m, &mut t, &mut s, &mut r), VC_OK);
        assert_eq!(r, [0.0, FRAC_PI_2, 0.0]);
        assert!((sx.atan2(cx) - 0.3).abs() < 1e-15);
    }
}
