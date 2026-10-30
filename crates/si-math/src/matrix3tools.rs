// SPDX-License-Identifier: MIT
//! 3 × 3 float matrices: the original's `matrix3tools.c` (finding 0008).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Each port repeats the original's x87 instruction sequence: the
//! same operations in the same order on [`F80`] register values, rounded to `f32` exactly
//! where the original stores to memory. The doc comment of each function gives its exactness
//! class:
//!
//! - **copy**: only moves values.
//! - **single**: every stored value is one operation on floats, so plain `f32` arithmetic has
//!   the same bits (for operands that are not NaNs; the code below goes through [`F80`] all
//!   the same, which also gives the unit's NaN results).
//! - **model**: several operations accumulate in a register before a store; bit-exact given
//!   the x87 model of [`crate::x87`].
//! - **libm**: goes through `sin` and `cos` ([`crate::trig`]); equal to the original only as
//!   far as those are, which cannot be shown.
//!
//! Conventions: a [`Matrix3`] is nine floats, `m[row][column]`, rows one after another
//! (element `3 × row + column`); angles are radians; the rotation by three angles is the
//! product `X × Y × Z` of the three axis rotations (all read from the code). Vectors are rows
//! and multiply from the left (`v' = v × M`), so in that product the X rotation acts first:
//! this is the crate's convention ([`crate::types`]); no function of this file multiplies a
//! vector by a matrix.
//!
//! A value the original moves with `fld m32` / `fstp m32` is moved here with `fmov`: the
//! bits are unchanged except that a signalling NaN comes out quiet. Constants the original
//! writes with integer instructions (`0.0`, `1.0`) are written as constants.

// the index loops are the original's
#![allow(clippy::needless_range_loop)]

use crate::trig::{cos, sin};
use crate::types::{Matrix, Matrix3, Vector, IDENTITY3};
use crate::text::printf_f;
use crate::x87::{fmov, x, F80};

/// The determinant below which both inverses refuse to work: the **double**
/// 1.1754943508222875e-32 = 10⁶ × 2⁻¹²⁶, that is one million times the smallest normal float
/// (0x08662530 for [`matrix3_inverse`], 0x08662538 for [`matrix3_fast_inverse`], both
/// 0x394e848000000000). The original compares an unrounded 80-bit determinant with it.
pub const DET_NULL: f64 = f64::from_bits(0x394e_8480_0000_0000);

/// What [`matrix3_inverse`] writes to the standard error stream when it refuses (0x086dc0c0).
pub fn inverse_error() -> &'static str {
    si_rt::text(0x086d_c0c0)
}
/// What [`matrix3_fast_inverse`] writes to the standard error stream when it refuses
/// (0x086dc100).
pub fn fast_inverse_error() -> &'static str {
    si_rt::text(0x086d_c100)
}

/// The line of 32 dashes that opens and closes both dumps (0x086dbf60, 0x086dbfc0,
/// 0x086dc000, 0x086dc080: four copies of the same text).
const DUMP_RULE: &str = "--------------------------------\n";

/// `%f` of a float argument: the float is widened to a double (`fld m32`, `fstp m64`, exact)
/// and printed with six decimals.
fn pf(v: f32) -> String {
    printf_f(x(v).to_f64(), 0, 6)
}

// ------------------------------------------------------------------ determinant

/// The determinant of a 3 × 3 matrix, **unrounded**: the 80-bit register value the original
/// leaves on the x87 stack [soft 0x0851af40 matrix3Det]. A file-local helper of the
/// original; its only caller is [`matrix3_inverse`].
///
/// Order: `((((m02·m10)·m21 + ((m01·m12)·m20 + m22·(m00·m11))) − m20·(m02·m11)) −
/// (m00·m12)·m21) − (m10·m01)·m22`. Each triple product of floats is exact after its first
/// multiplication and rounded once (to 64 bits) after the second, so only the order of the
/// additions matters. Class: model.
pub fn matrix3_det(m: &Matrix3) -> F80 {
    let p1 = x(m[2][2]) * (x(m[0][0]) * m[1][1]);
    let p2 = (x(m[0][1]) * m[1][2]) * m[2][0];
    let s = p2 + p1;
    let q1 = (x(m[0][2]) * m[1][0]) * m[2][1];
    let r = x(m[2][0]) * (x(m[0][2]) * m[1][1]);
    let q2 = (x(m[0][0]) * m[1][2]) * m[2][1];
    let q3 = (x(m[1][0]) * m[0][1]) * m[2][2];
    (((q1 + s) - r) - q2) - q3
}

// ------------------------------------------------------------------ element by element

/// Every element of `a` times `s` [soft 0x0851afb0 SI_matrix3Scalar]. The original's
/// argument order is (scalar, input, output). Class: single. The original works element by
/// element, reading each before writing it, so the output may be the input.
pub fn matrix3_scalar(s: f32, a: &Matrix3) -> Matrix3 {
    let s = x(s);
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (x(a[i][j]) * s).to_f32();
        }
    }
    out
}

/// `a + b`, element by element [soft 0x0851afdc SI_matrix3Add]. Class: single. The output
/// may be either input.
pub fn matrix3_add(a: &Matrix3, b: &Matrix3) -> Matrix3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (x(a[i][j]) + b[i][j]).to_f32();
        }
    }
    out
}

/// `a − b`, element by element [soft 0x0851b00c SI_matrix3Sub]. Class: single. The output
/// may be either input.
pub fn matrix3_sub(a: &Matrix3, b: &Matrix3) -> Matrix3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (x(a[i][j]) - b[i][j]).to_f32();
        }
    }
    out
}

/// The "tilde" matrix of a vector, the skew-symmetric matrix of the cross product
/// [soft 0x0851b03c SI_matrix3Tilde] (the vector is passed by value):
///
/// ```text
/// |  0  −z   y |
/// |  z   0  −x |
/// | −y   x   0 |
/// ```
///
/// With rows multiplying from the left, `w × T = w × v` (cross product); as a column
/// operator, `T × w = v × w`. The diagonal is written as integer zeros (+0.0); the other
/// six go through the x87 unit, three of them with `fchs`. Class: copy.
pub fn matrix3_tilde(v: Vector) -> Matrix3 {
    let neg = |c: f32| (-x(c)).to_f32();
    [[0.0, neg(v.z), fmov(v.y)], [fmov(v.z), 0.0, neg(v.x)], [neg(v.y), fmov(v.x), 0.0]]
}

/// A copy of `a` [soft 0x0851b088 SI_matrix3Copy]: nine `fld`/`fstp` pairs, first element
/// first. Class: copy.
pub fn matrix3_copy(a: &Matrix3) -> Matrix3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = fmov(a[i][j]);
        }
    }
    out
}

/// The 4 × 4 matrix with `a` in its upper left corner, zeros in the rest of the last row and
/// column and 1 in the corner [soft 0x0851b0ac SI_matrix3Put]. All sixteen elements are
/// written: nothing of the previous content of the output survives. Class: copy.
pub fn matrix3_put(a: &Matrix3) -> Matrix {
    let mut out = [[0.0; 4]; 4];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = fmov(a[i][j]);
        }
    }
    out[3][3] = 1.0;
    out
}

/// The upper left 3 × 3 corner of a 4 × 4 matrix [soft 0x0851b120 SI_matrix3Get].
/// Class: copy.
pub fn matrix3_get(m: &Matrix) -> Matrix3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = fmov(m[i][j]);
        }
    }
    out
}

// ------------------------------------------------------------------ products

/// The product `a × b` [soft 0x0851b164 SI_matrix3Multiply]. With rows multiplying from the
/// left, `a` acts first.
///
/// Every element is `(a[i][0]·b[0][j] + a[i][1]·b[1][j]) + a[i][2]·b[2][j]`: three products
/// kept in the register (each exact: 48 bits) and two additions, rounded to float once.
/// Class: model. The original computes into a local matrix and copies that to the output
/// at the end, so the output may be either input.
pub fn matrix3_multiply(a: &Matrix3, b: &Matrix3) -> Matrix3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = ((x(a[i][0]) * b[0][j] + x(a[i][1]) * b[1][j]) + x(a[i][2]) * b[2][j]).to_f32();
        }
    }
    out
}

/// The transpose of `a`, for an output that is another object than the input
/// [soft 0x0851b2b8 SI_matrix3Transpose]. Class: copy.
///
/// The original writes `out[j][i] = a[i][j]` straight into the output, row by row of the
/// input, without a temporary. Called with the output being the input it does **not**
/// transpose: see [`matrix3_transpose_in_place`].
pub fn matrix3_transpose(a: &Matrix3) -> Matrix3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[j][i] = fmov(a[i][j]);
        }
    }
    out
}

/// What [soft 0x0851b2b8 SI_matrix3Transpose] does when its output is its input: the
/// elements below the diagonal are overwritten with their mirror images before they are
/// read, so the result is the symmetric matrix made of the upper triangle of the input, not
/// the transpose. Class: copy.
pub fn matrix3_transpose_in_place(m: &mut Matrix3) {
    for i in 0..3 {
        for j in 0..3 {
            m[j][i] = fmov(m[i][j]);
        }
    }
}

// ------------------------------------------------------------------ rotations

/// `cos` and `sin` of a float angle as the three axis rotations take them: the angle is
/// widened to a double (exact), the C library is called, and each 80-bit result is stored to
/// a float local before it is used.
fn cos_sin(angle: f32) -> (f32, f32) {
    let a = x(angle).to_f64();
    (cos(a).to_f32(), sin(a).to_f32())
}

/// The same pair with `sin` called first, as `SI_matrix3XyzRotation` does for each angle.
/// The order changes no result; it lets a replay of the original's library calls
/// (finding 0011) follow call by call.
fn sin_cos(angle: f32) -> (f32, f32) {
    let a = x(angle).to_f64();
    let s = sin(a).to_f32();
    (s, cos(a).to_f32())
}

/// The rotation by `angle` radians about X [soft 0x0851b310 SI_matrix3XRotation]:
///
/// ```text
/// | 1   0   0 |
/// | 0   c   s |
/// | 0  −s   c |
/// ```
///
/// A row vector times this matrix turns from Y toward Z for a positive angle. The original
/// copies the identity (its global `SI_matrix3ID`, 0x086dc220) and then stores `c` to
/// elements `[2][2]` and `[1][1]`, `s` to `[1][2]` and `−s` (an `fchs` of the float `s`) to
/// `[2][1]`: for an angle of 0 that last element is −0.0. Class: libm.
pub fn matrix3_x_rotation(angle: f32) -> Matrix3 {
    let mut out = matrix3_copy(&IDENTITY3);
    let (c, s) = cos_sin(angle);
    out[2][2] = c;
    out[1][1] = c;
    out[1][2] = s;
    out[2][1] = (-x(s)).to_f32();
    out
}

/// The rotation by `angle` radians about Y [soft 0x0851b370 SI_matrix3YRotation]:
///
/// ```text
/// | c   0  −s |
/// | 0   1   0 |
/// | s   0   c |
/// ```
///
/// A row vector turns from Z toward X for a positive angle. Stores `c` to `[2][2]` and
/// `[0][0]`, `s` to `[2][0]`, `−s` to `[0][2]`. Class: libm.
pub fn matrix3_y_rotation(angle: f32) -> Matrix3 {
    let mut out = matrix3_copy(&IDENTITY3);
    let (c, s) = cos_sin(angle);
    out[2][2] = c;
    out[0][0] = c;
    out[2][0] = s;
    out[0][2] = (-x(s)).to_f32();
    out
}

/// The rotation by `angle` radians about Z [soft 0x0851b3d0 SI_matrix3ZRotation]:
///
/// ```text
/// |  c   s   0 |
/// | −s   c   0 |
/// |  0   0   1 |
/// ```
///
/// A row vector turns from X toward Y for a positive angle. Stores `c` to `[1][1]` and
/// `[0][0]`, `s` to `[0][1]`, `−s` to `[1][0]`. Class: libm.
pub fn matrix3_z_rotation(angle: f32) -> Matrix3 {
    let mut out = matrix3_copy(&IDENTITY3);
    let (c, s) = cos_sin(angle);
    out[1][1] = c;
    out[0][0] = c;
    out[0][1] = s;
    out[1][0] = (-x(s)).to_f32();
    out
}

/// The rotation by the three angles of `r` (radians), X first, then Y, then Z: the product
/// `X(r.x) × Y(r.y) × Z(r.z)` of the three matrices above, written out
/// [soft 0x0851b430 SI_matrix3XyzRotation]:
///
/// ```text
/// | cy·cz              cy·sz              −sy   |
/// | sx·sy·cz − cx·sz   sx·sy·sz + cx·cz   sx·cy |
/// | cx·sy·cz + sx·sz   cx·sy·sz − sx·cz   cx·cy |
/// ```
///
/// The six sines and cosines are each rounded to float first (stored to float locals). The
/// products `sx·sy` and `cx·sy` are then kept unrounded in the register and used for two
/// elements each: `[1][0] = (sx·sy)·cz − cx·sz`, `[1][1] = (sx·sy)·sz + cx·cz`,
/// `[2][0] = sx·sz + (cx·sy)·cz`, `[2][1] = sz·(cx·sy) − cz·sx`; every element is rounded to
/// float once. All nine elements are written (no identity copy here), after all six library
/// calls. Class: libm.
pub fn matrix3_xyz_rotation(r: &Vector) -> Matrix3 {
    let (sx, cx) = sin_cos(r.x);
    let (sy, cy) = sin_cos(r.y);
    let (sz, cz) = sin_cos(r.z);
    let mut out = [[0.0; 3]; 3];
    out[0][0] = (x(cy) * cz).to_f32();
    out[0][1] = (x(cy) * sz).to_f32();
    out[0][2] = (-x(sy)).to_f32();
    let t1 = x(sx) * sy;
    out[1][0] = (t1 * cz - x(cx) * sz).to_f32();
    out[1][1] = (t1 * sz + x(cx) * cz).to_f32();
    out[1][2] = (x(sx) * cy).to_f32();
    let t2 = x(cx) * sy;
    out[2][0] = (x(sx) * sz + t2 * cz).to_f32();
    out[2][1] = (x(sz) * t2 - x(cz) * sx).to_f32();
    out[2][2] = (x(cx) * cy).to_f32();
    out
}

// ------------------------------------------------------------------ dumps

/// The second line of both dumps, or `None` where the original faults (see
/// [`matrix3_dump`]).
fn dump_title(name: i8) -> Option<&'static str> {
    if name == 0 {
        Some("DUMP matrix: (null)\n")
    } else {
        None
    }
}

/// The text [soft 0x0851b548 SI_matrix3Dump] prints on the standard output: a line of 32
/// dashes, the title line, the three rows as `"| %f %f %f |\n"` (0x086dbf93, 0x086dbfa1,
/// 0x086dbfaf) and the dashes again. Nothing in the program calls this. Class: copy.
///
/// `name` is the original's `char` parameter, and the title line is where the original goes
/// wrong: its format is `"DUMP matrix: %s\n"` (0x086dbf82; 0x086dc022 in the 4 × 4 version)
/// but the argument it passes is the character itself, sign-extended, where `%s` wants a
/// pointer to text. For the character 0 that is a null pointer, for which the GNU C library
/// prints `(null)` (the library's behaviour, not something in the binary). For any other
/// character the pointer is an address below 128 or above 0xffffff7f, which no process has
/// mapped: the original dies there with a memory fault, after the line of dashes. `None`
/// stands for that.
pub fn matrix3_dump(m: &Matrix3, name: i8) -> Option<String> {
    let mut s = String::from(DUMP_RULE);
    s.push_str(dump_title(name)?);
    for row in m {
        s.push_str(&format!("| {} {} {} |\n", pf(row[0]), pf(row[1]), pf(row[2])));
    }
    s.push_str(DUMP_RULE);
    Some(s)
}

/// The same for a 4 × 4 matrix, rows as `"| %f %f %f %f |\n"` (0x086dc033, 0x086dc044,
/// 0x086dc055, 0x086dc066) [soft 0x0851b610 SI_matrix4Dump]. `None` as for
/// [`matrix3_dump`]. Nothing in the program calls this. Class: copy.
pub fn matrix4_dump(m: &Matrix, name: i8) -> Option<String> {
    let mut s = String::from(DUMP_RULE);
    s.push_str(dump_title(name)?);
    for row in m {
        s.push_str(&format!("| {} {} {} {} |\n", pf(row[0]), pf(row[1]), pf(row[2]), pf(row[3])));
    }
    s.push_str(DUMP_RULE);
    Some(s)
}

// ------------------------------------------------------------------ inverses

/// The inverse of `a` by Gauss–Jordan elimination **without pivoting**
/// [soft 0x0851b72c SI_matrix3Inverse]. Nothing in the program calls this.
///
/// 1. The determinant is taken with [`matrix3_det`]. If its magnitude is below
///    [`DET_NULL`] (strictly; a NaN determinant is not), the original prints
///    [`inverse_error`] on the standard error stream and returns with the output untouched:
///    `Err` with that text.
/// 2. Otherwise the output is set to the identity and `a` is copied to a local matrix `t`,
///    **in that order**.
/// 3. For each row `i` = 0, 1, 2: `f = 1 / t[i][i]` is kept unrounded; every element of row
///    `i` of `t` and of the output is replaced by `element × f` (rounded to float); then for
///    each row `k` below, with `g = t[k][i]` read once before the row changes,
///    `row k = row k − row i × g` in both matrices (each element `e − p·g`, rounded once).
/// 4. Back substitution for `i` = 2, 1 and each row `k` = `i − 1` down to 0: the same row
///    operation with `g = t[k][i]`.
///
/// There is no pivot search: a zero on the diagonal during step 3 divides by zero and the
/// result fills with infinities and NaNs although the determinant test passed. Class: model.
///
/// Aliasing: because of the order in step 2, the original called with the output being the
/// input copies the identity over the input before it copies the input, and so returns the
/// identity (for a determinant that passes). This function takes two objects; for that case
/// write the identity yourself.
pub fn matrix3_inverse(a: &Matrix3, out: &mut Matrix3) -> Result<(), &'static str> {
    if matrix3_det(a).abs().lt(x(DET_NULL)) {
        return Err(inverse_error());
    }
    *out = matrix3_copy(&IDENTITY3);
    let mut t = matrix3_copy(a);
    for i in 0..3 {
        let f = F80::ONE / t[i][i];
        for j in 0..3 {
            t[i][j] = (x(t[i][j]) * f).to_f32();
            out[i][j] = (x(out[i][j]) * f).to_f32();
        }
        for k in i + 1..3 {
            eliminate(&mut t, out, k, i);
        }
    }
    for i in (0..3).rev() {
        for k in (0..i).rev() {
            eliminate(&mut t, out, k, i);
        }
    }
    Ok(())
}

/// One row operation of [`matrix3_inverse`]: `row k −= row i × t[k][i]` in both matrices.
/// The factor is loaded once, before the loop overwrites `t[k][i]`.
fn eliminate(t: &mut Matrix3, out: &mut Matrix3, k: usize, i: usize) {
    let g = x(t[k][i]);
    for j in 0..3 {
        t[k][j] = (t[k][j] - x(t[i][j]) * g).to_f32();
        out[k][j] = (out[k][j] - x(out[i][j]) * g).to_f32();
    }
}

/// The inverse of `a` as adjugate over determinant (Cramer's rule): "fast" because it is a
/// closed formula, not because it assumes anything about the matrix
/// [soft 0x0851b8ec SI_matrix3FastInverse].
///
/// With `A = m11·m22 − m12·m21`, `B = −(m10·m22 − m12·m20)`, `C = m10·m21 − m11·m20`
/// (unrounded), the determinant is `(m00·A + m01·B) + m02·C`, unrounded: an expansion along
/// the first row, another order of operations than [`matrix3_det`]. If its magnitude is
/// below [`DET_NULL`] (strictly; a NaN is not), the original prints [`fast_inverse_error`]
/// on the standard error stream and **copies the input to the output**: `Err` with that
/// text, `out` holding the copy. Otherwise `r = 1 / det` (unrounded) and
///
/// ```text
/// out[0][0] = A·r                         out[0][1] = −(m01·m22 − m02·m21)·r   out[0][2] = (m01·m12 − m02·m11)·r
/// out[1][0] = B·r                         out[1][1] = (m00·m22 − m02·m20)·r    out[1][2] = −(m00·m12 − m02·m10)·r
/// out[2][0] = C·r                         out[2][1] = −(m00·m21 − m01·m20)·r   out[2][2] = (m00·m11 − m01·m10)·r
/// ```
///
/// each rounded to float once (the negations are done before the multiplication by `r`).
/// Class: model. The original has read every input element before its first store, so the
/// output may be the input; here pass a copy of the input.
pub fn matrix3_fast_inverse(a: &Matrix3, out: &mut Matrix3) -> Result<(), &'static str> {
    let [[m00, m01, m02], [m10, m11, m12], [m20, m21, m22]] = *a;
    let ca = x(m22) * m11 - x(m12) * m21;
    let cb = -(x(m10) * m22 - x(m12) * m20);
    let cc = x(m10) * m21 - x(m11) * m20;
    let det = (x(m00) * ca + x(m01) * cb) + x(m02) * cc;
    if det.abs().lt(x(DET_NULL)) {
        *out = matrix3_copy(a);
        return Err(fast_inverse_error());
    }
    let r = F80::ONE / det;
    out[0][0] = (ca * r).to_f32();
    out[0][1] = (-(x(m01) * m22 - x(m02) * m21) * r).to_f32();
    out[0][2] = ((x(m01) * m12 - x(m02) * m11) * r).to_f32();
    out[1][0] = (cb * r).to_f32();
    out[1][1] = ((x(m00) * m22 - x(m02) * m20) * r).to_f32();
    out[2][0] = (cc * r).to_f32();
    out[1][2] = (-(x(m00) * m12 - x(m02) * m10) * r).to_f32();
    out[2][1] = (-(x(m00) * m21 - x(m01) * m20) * r).to_f32();
    out[2][2] = ((x(m00) * m11 - x(m01) * m10) * r).to_f32();
    Ok(())
}

// ------------------------------------------------------------------ cleaning

/// Sets to +0.0 every element whose magnitude is strictly below `eps`, in place
/// [soft 0x0851ba60 SI_matrix3Clean]. Nothing else: no rounding to "nice" values, no
/// re-orthogonalisation. An element equal to `eps`, a NaN element, and every element when
/// `eps` is a NaN are left alone. Class: copy.
pub fn matrix3_clean(m: &mut Matrix3, eps: f32) {
    for e in m.iter_mut().flatten() {
        if x(*e).abs().lt(x(eps)) {
            *e = 0.0;
        }
    }
}

/// [`matrix3_clean`] with `eps`, then two more passes over the nine elements, in place
/// [soft 0x0851bb48 SI_matrix3RotClean]: every element strictly above `one` becomes 1.0,
/// then every element strictly below `−one` becomes −1.0. So elements near 0 and near ±1
/// are snapped, which suits a rotation about a coordinate axis by a multiple of 90°; the
/// function does not check or restore orthogonality. The three passes run one after the
/// other on the stored values. That matters only for a senseless negative `one`: an element
/// between `one` and `−one` meets both conditions and ends as 1, because the second pass has
/// raised it before the third looks at it. NaNs are left alone.
/// Nothing in the program calls this. Class: copy.
pub fn matrix3_rot_clean(m: &mut Matrix3, eps: f32, one: f32) {
    matrix3_clean(m, eps);
    for e in m.iter_mut().flatten() {
        if x(one).lt(x(*e)) {
            *e = 1.0;
        }
    }
    let minus_one = -x(one);
    for e in m.iter_mut().flatten() {
        if minus_one.gt(x(*e)) {
            *e = -1.0;
        }
    }
}

/// Scales each **column** of `a` to unit length and writes it to the same column of `out`
/// [soft 0x0851bd94 SI_matrix3Orthonormal]. Despite the name nothing is made orthogonal:
/// there is no Gram–Schmidt step, the columns keep their directions.
///
/// For each column `c`: `len = sqrt((a[0][c]² + a[1][c]²) + a[2][c]²)`, computed in the
/// register and rounded to float; if `len` equals 0 the column of `out` is **not written**
/// (it keeps what the caller had there); otherwise each element is `a[r][c] / len`, one
/// division rounded to float. A NaN length is not equal to 0, so the column becomes NaNs.
/// Class: model (the length), single (the divisions).
///
/// Aliasing: each column is read completely before it is written, so the original gives the
/// same result with the output being the input (a zero column then simply stays zero); here
/// pass a copy of the input and the matrix itself as `out`.
pub fn matrix3_orthonormal(a: &Matrix3, out: &mut Matrix3) {
    for c in 0..3 {
        let len = ((x(a[0][c]) * a[0][c] + x(a[1][c]) * a[1][c]) + x(a[2][c]) * a[2][c]).sqrt().to_f32();
        if x(len).eq(F80::ZERO) {
            continue;
        }
        for r in 0..3 {
            out[r][c] = (x(a[r][c]) / len).to_f32();
        }
    }
}

// ------------------------------------------------------------------ packing a rotation

/// The first two **columns** of `m` as two vectors: six floats instead of nine
/// [soft 0x0851be0c SI_matrix3RotPack]. The third column is dropped; for a rotation matrix
/// [`matrix3_rot_unpack`] rebuilds it. Nothing in the program calls this. Class: copy.
pub fn matrix3_rot_pack(m: &Matrix3) -> (Vector, Vector) {
    (
        Vector::new(fmov(m[0][0]), fmov(m[1][0]), fmov(m[2][0])),
        Vector::new(fmov(m[0][1]), fmov(m[1][1]), fmov(m[2][1])),
    )
}

/// The matrix whose first column is `a`, whose second column is `b` and whose third column
/// is the cross product `a × b` [soft 0x0851be40 SI_matrix3RotUnpack] (both vectors are
/// passed by value). The inverse of [`matrix3_rot_pack`] for a proper rotation; nothing is
/// normalised or checked.
///
/// Third column: `(a.y·b.z − a.z·b.y, a.z·b.x − a.x·b.z, a.x·b.y − a.y·b.x)`, each two
/// products and a subtraction in the register, rounded to float once. Nothing in the
/// program calls this. Class: model.
pub fn matrix3_rot_unpack(a: Vector, b: Vector) -> Matrix3 {
    let c0 = (x(a.y) * b.z - x(a.z) * b.y).to_f32();
    let c1 = (x(a.z) * b.x - x(a.x) * b.z).to_f32();
    let c2 = (x(a.x) * b.y - x(a.y) * b.x).to_f32();
    [[fmov(a.x), fmov(b.x), c0], [fmov(a.y), fmov(b.y), c1], [fmov(a.z), fmov(b.z), c2]]
}

#[cfg(test)]
#[allow(clippy::excessive_precision)]
mod tests {
    use super::*;
    use crate::x87::testing::Rng;

    const P32: f32 = 4294967296.0; // 2^32
    const P24: f32 = 16777216.0; // 2^24

    fn bits(m: &Matrix3) -> [[u32; 3]; 3] {
        m.map(|r| r.map(f32::to_bits))
    }

    fn close(a: &Matrix3, b: &Matrix3, tol: f32) -> bool {
        (0..3).all(|i| (0..3).all(|j| (a[i][j] - b[i][j]).abs() <= tol))
    }

    const A: Matrix3 = [[2.0, -1.0, 0.5], [0.25, 3.0, -2.0], [1.5, 0.75, 4.0]];

    #[test]
    fn determinant_values() {
        assert_eq!(matrix3_det(&IDENTITY3).to_f64(), 1.0);
        // 2(12 + 1.5) + 1(1 + 3) + 0.5(0.1875 − 4.5) = 27 + 4 − 2.15625
        assert_eq!(matrix3_det(&A).to_f64(), 28.84375);
        assert_eq!(matrix3_det(&[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]).to_f64(), 0.0);
    }

    #[test]
    fn determinant_is_unrounded() {
        // diagonal (1 + 2^-23, 1 + 2^-23, 2^30): the product needs 47 bits, which the
        // register holds; float arithmetic would have rounded (1 + 2^-23)^2 to 1 + 2^-22
        let e = 1.0 + f32::EPSILON;
        let m = [[e, 0.0, 0.0], [0.0, e, 0.0], [0.0, 0.0, 1073741824.0]];
        let want = 1073741824.0f64 + 256.0 + 2.0f64.powi(-16);
        assert_eq!(matrix3_det(&m).to_f64(), want);
        assert_ne!(want as f32 as f64, want);
    }

    #[test]
    fn determinant_order_of_additions() {
        // the true determinant is 1: 2^64 + 1 − 2^64. The original adds 1 to 2^64 first
        // (65 bits: rounds back to 2^64) and subtracts 2^64 afterwards: 0.
        let m = [[P32, 1.0, 0.0], [0.0, P32, 1.0], [1.0, P32, 1.0]];
        assert_eq!(matrix3_det(&m).to_bits(), F80::ZERO.to_bits());
    }

    #[test]
    fn scalar_add_sub() {
        let s = matrix3_scalar(0.5, &A);
        assert_eq!(s, [[1.0, -0.5, 0.25], [0.125, 1.5, -1.0], [0.75, 0.375, 2.0]]);
        let sum = matrix3_add(&A, &s);
        assert_eq!(sum, [[3.0, -1.5, 0.75], [0.375, 4.5, -3.0], [2.25, 1.125, 6.0]]);
        assert_eq!(matrix3_sub(&sum, &s), A);
        assert_eq!(matrix3_sub(&A, &A), [[0.0; 3]; 3]);
        // one operation, rounded to float: 2^24 + 1 is a tie and goes to the even neighbour
        let one = [[1.0; 3]; 3];
        let big = [[P24; 3]; 3];
        assert_eq!(matrix3_add(&big, &one), big);
        assert_eq!(matrix3_scalar(3.0, &[[0.1; 3]; 3])[1][1], 0.1f32 * 3.0);
    }

    #[test]
    fn single_operations_equal_float_arithmetic() {
        // the class "single" claim: one operation on floats stored to a float has the bits
        // of plain float arithmetic (NaN results aside, whose payloads differ by processor)
        let same = |got: f32, want: f32| got.to_bits() == want.to_bits() || (got.is_nan() && want.is_nan());
        let mut rng = Rng(0x5eed_0003);
        let mut float = move || f32::from_bits(rng.next() as u32);
        for _ in 0..20_000 {
            let a: Matrix3 = core::array::from_fn(|_| core::array::from_fn(|_| float()));
            let b: Matrix3 = core::array::from_fn(|_| core::array::from_fn(|_| float()));
            let s = float();
            let (scaled, sum, diff) = (matrix3_scalar(s, &a), matrix3_add(&a, &b), matrix3_sub(&a, &b));
            for i in 0..3 {
                for j in 0..3 {
                    assert!(same(scaled[i][j], a[i][j] * s));
                    assert!(same(sum[i][j], a[i][j] + b[i][j]));
                    assert!(same(diff[i][j], a[i][j] - b[i][j]));
                }
            }
        }
    }

    #[test]
    fn tilde_is_the_cross_product_matrix() {
        let v = Vector::new(1.0, 2.0, 3.0);
        let t = matrix3_tilde(v);
        assert_eq!(t, [[0.0, -3.0, 2.0], [3.0, 0.0, -1.0], [-2.0, 1.0, 0.0]]);
        // w × T (row vector from the left) = w × v
        let w = [4.0f32, -5.0, 6.0];
        let row: Vec<f32> = (0..3).map(|j| w[0] * t[0][j] + w[1] * t[1][j] + w[2] * t[2][j]).collect();
        let cross = [w[1] * v.z - w[2] * v.y, w[2] * v.x - w[0] * v.z, w[0] * v.y - w[1] * v.x];
        assert_eq!(row, cross);
        // the diagonal is +0, the negated elements of a zero vector are −0
        let z = matrix3_tilde(Vector::new(0.0, 0.0, 0.0));
        assert_eq!(bits(&z), [[0, 0x8000_0000, 0], [0, 0, 0x8000_0000], [0x8000_0000, 0, 0]]);
    }

    #[test]
    fn copies_and_corners() {
        assert_eq!(matrix3_copy(&A), A);
        let m4 = matrix3_put(&A);
        assert_eq!(
            m4,
            [[2.0, -1.0, 0.5, 0.0], [0.25, 3.0, -2.0, 0.0], [1.5, 0.75, 4.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
        );
        assert_eq!(matrix3_get(&m4), A);
        let full: Matrix = [[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], [9.0, 10.0, 11.0, 12.0], [13.0, 14.0, 15.0, 16.0]];
        assert_eq!(matrix3_get(&full), [[1.0, 2.0, 3.0], [5.0, 6.0, 7.0], [9.0, 10.0, 11.0]]);
        // a move through the unit quiets a signalling NaN and changes nothing else
        let snan = f32::from_bits(0x7f80_0001);
        let mut n = A;
        n[0][0] = snan;
        n[1][1] = f32::from_bits(0x8000_0001); // a negative denormal survives
        let c = matrix3_copy(&n);
        assert_eq!(c[0][0].to_bits(), 0x7fc0_0001);
        assert_eq!(c[1][1].to_bits(), 0x8000_0001);
    }

    #[test]
    fn multiply_values() {
        assert_eq!(matrix3_multiply(&A, &IDENTITY3), A);
        assert_eq!(matrix3_multiply(&IDENTITY3, &A), A);
        let b = [[1.0, 2.0, 0.0], [0.0, 1.0, -1.0], [3.0, 0.0, 2.0]];
        // rows of A times columns of b
        assert_eq!(matrix3_multiply(&A, &b), [[3.5, 3.0, 2.0], [-5.75, 3.5, -7.0], [13.5, 3.75, 7.25]]);
        // rows multiply from the left: (v × A) × b = v × (A × b)
        let ab = matrix3_multiply(&A, &b);
        let v = [1.0f32, 2.0, 4.0];
        let va: Vec<f32> = (0..3).map(|j| v[0] * A[0][j] + v[1] * A[1][j] + v[2] * A[2][j]).collect();
        let vab: Vec<f32> = (0..3).map(|j| va[0] * b[0][j] + va[1] * b[1][j] + va[2] * b[2][j]).collect();
        let direct: Vec<f32> = (0..3).map(|j| v[0] * ab[0][j] + v[1] * ab[1][j] + v[2] * ab[2][j]).collect();
        assert_eq!(vab, direct);
    }

    #[test]
    fn multiply_order_and_precision() {
        // element [0][0] = (2^32·2^32 + 1·1) + (−2^32)·2^32: the first sum needs 65 bits and
        // rounds back to 2^64, so the result is 0; adding the last two terms first gives 1
        let a = [[P32, 1.0, -P32], [0.0; 3], [0.0; 3]];
        let b = [[P32, 0.0, 0.0], [1.0, 0.0, 0.0], [P32, 0.0, 0.0]];
        assert_eq!(matrix3_multiply(&a, &b)[0][0].to_bits(), 0);
        // (2^12·2^12 + 1) − 2^24 = 1 in the register; rounding each step to float gives 0
        let a = [[4096.0, 1.0, -4096.0], [0.0; 3], [0.0; 3]];
        let b = [[4096.0, 0.0, 0.0], [1.0, 0.0, 0.0], [4096.0, 0.0, 0.0]];
        assert_eq!(matrix3_multiply(&a, &b)[0][0], 1.0);
        assert_eq!((4096.0f32 * 4096.0 + 1.0) - 4096.0 * 4096.0, 0.0);
    }

    #[test]
    fn transpose_and_its_alias_case() {
        let t = matrix3_transpose(&A);
        assert_eq!(t, [[2.0, 0.25, 1.5], [-1.0, 3.0, 0.75], [0.5, -2.0, 4.0]]);
        assert_eq!(matrix3_transpose(&t), A);
        // in place the original mirrors the upper triangle instead
        let mut m = A;
        matrix3_transpose_in_place(&mut m);
        assert_eq!(m, [[2.0, -1.0, 0.5], [-1.0, 3.0, -2.0], [0.5, -2.0, 4.0]]);
    }

    #[test]
    fn axis_rotations() {
        // angle 0: the identity, with −0 where the original stores −sin
        assert_eq!(bits(&matrix3_x_rotation(0.0)), bits(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -0.0, 1.0]]));
        assert_eq!(bits(&matrix3_y_rotation(0.0)), bits(&[[1.0, 0.0, -0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]));
        assert_eq!(bits(&matrix3_z_rotation(0.0)), bits(&[[1.0, 0.0, 0.0], [-0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]));
        let a = 0.7f32;
        let (c, s) = ((a as f64).cos() as f32, (a as f64).sin() as f32);
        let tol = 1e-7;
        assert!(close(&matrix3_x_rotation(a), &[[1.0, 0.0, 0.0], [0.0, c, s], [0.0, -s, c]], tol));
        assert!(close(&matrix3_y_rotation(a), &[[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]], tol));
        assert!(close(&matrix3_z_rotation(a), &[[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]], tol));
        // the two cosine elements are one float, the two sine elements differ only in sign
        let m = matrix3_x_rotation(a);
        assert_eq!(m[1][1].to_bits(), m[2][2].to_bits());
        assert_eq!(m[1][2].to_bits() ^ 0x8000_0000, m[2][1].to_bits());
        // a quarter turn about X takes the row vector Y to Z, about Y takes Z to X, about Z X to Y
        let q = core::f32::consts::FRAC_PI_2;
        assert!(close(&matrix3_x_rotation(q), &[[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]], tol));
        assert!(close(&matrix3_y_rotation(q), &[[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]], tol));
        assert!(close(&matrix3_z_rotation(q), &[[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], tol));
    }

    #[test]
    fn xyz_rotation_is_x_then_y_then_z() {
        let r = Vector::new(0.3, -1.1, 2.0);
        let xyz = matrix3_xyz_rotation(&r);
        let product = matrix3_multiply(&matrix3_multiply(&matrix3_x_rotation(r.x), &matrix3_y_rotation(r.y)), &matrix3_z_rotation(r.z));
        assert!(close(&xyz, &product, 2e-7));
        // not any other order
        let zyx = matrix3_multiply(&matrix3_multiply(&matrix3_z_rotation(r.z), &matrix3_y_rotation(r.y)), &matrix3_x_rotation(r.x));
        assert!(!close(&xyz, &zyx, 1e-2));
        // one angle at a time it is the axis rotation itself (zeros may differ in sign)
        assert_eq!(matrix3_xyz_rotation(&Vector::new(0.7, 0.0, 0.0)), matrix3_x_rotation(0.7));
        assert_eq!(matrix3_xyz_rotation(&Vector::new(0.0, 0.7, 0.0)), matrix3_y_rotation(0.7));
        assert_eq!(matrix3_xyz_rotation(&Vector::new(0.0, 0.0, 0.7)), matrix3_z_rotation(0.7));
        // [0][2] is −sin(y) exactly, the corner elements are single products of floats
        let (cx, sx) = cos_sin(r.x);
        let (cy, sy) = cos_sin(r.y);
        assert_eq!(xyz[0][2], -sy);
        assert_eq!(xyz[1][2], sx * cy);
        assert_eq!(xyz[2][2], cx * cy);
        // the rows of a rotation are unit vectors
        for row in xyz {
            let n: f32 = row.iter().map(|v| v * v).sum();
            assert!((n - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn dumps() {
        let m = [[1.0, -2.5, 0.0], [0.1, 1e10, -0.0], [f32::INFINITY, f32::from_bits(0xffc0_0000), 0.0078125]];
        let want = "--------------------------------\n\
                    DUMP matrix: (null)\n\
                    | 1.000000 -2.500000 0.000000 |\n\
                    | 0.100000 10000000000.000000 -0.000000 |\n\
                    | inf -nan 0.007812 |\n\
                    --------------------------------\n";
        assert_eq!(matrix3_dump(&m, 0).unwrap(), want);
        // any name character other than 0 is used as a text pointer by the original: a fault
        assert_eq!(matrix3_dump(&m, b'A' as i8), None);
        assert_eq!(matrix3_dump(&m, -1), None);
        let m4 = matrix3_put(&A);
        let want = "--------------------------------\n\
                    DUMP matrix: (null)\n\
                    | 2.000000 -1.000000 0.500000 0.000000 |\n\
                    | 0.250000 3.000000 -2.000000 0.000000 |\n\
                    | 1.500000 0.750000 4.000000 0.000000 |\n\
                    | 0.000000 0.000000 0.000000 1.000000 |\n\
                    --------------------------------\n";
        assert_eq!(matrix4_dump(&m4, 0).unwrap(), want);
        assert_eq!(matrix4_dump(&m4, 1), None);
    }

    #[test]
    fn inverse_values() {
        let mut out = [[9.0; 3]; 3];
        assert_eq!(matrix3_inverse(&[[2.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 8.0]], &mut out), Ok(()));
        assert_eq!(out, [[0.5, 0.0, 0.0], [0.0, 0.25, 0.0], [0.0, 0.0, 0.125]]);
        assert_eq!(matrix3_inverse(&A, &mut out), Ok(()));
        assert!(close(&matrix3_multiply(&A, &out), &IDENTITY3, 1e-6));
        assert!(close(&matrix3_multiply(&out, &A), &IDENTITY3, 1e-6));
        let mut fast = [[9.0; 3]; 3];
        assert_eq!(matrix3_fast_inverse(&A, &mut fast), Ok(()));
        assert!(close(&fast, &out, 1e-6));
        // a shear: every step is exact
        let shear = [[1.0, 2.0, 3.0], [0.0, 1.0, 4.0], [0.0, 0.0, 1.0]];
        let want = [[1.0, -2.0, 5.0], [0.0, 1.0, -4.0], [0.0, 0.0, 1.0]];
        assert_eq!(matrix3_inverse(&shear, &mut out), Ok(()));
        assert_eq!(out, want);
        assert_eq!(matrix3_fast_inverse(&shear, &mut fast), Ok(()));
        assert_eq!(fast, want);
    }

    #[test]
    fn inverse_refuses_small_determinants() {
        si_rt::skip_without_install!();
        let singular = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
        let mut out = [[9.0; 3]; 3];
        assert_eq!(matrix3_inverse(&singular, &mut out), Err(inverse_error()));
        assert_eq!(out, [[9.0; 3]; 3]); // untouched
        assert_eq!(matrix3_fast_inverse(&singular, &mut out), Err(fast_inverse_error()));
        assert_eq!(out, singular); // the input, copied
        // the limit is 10^6 × 2^-126 and the test is strict: a determinant equal to it passes
        assert_eq!(DET_NULL, 1.1754943508222875e-32);
        assert_eq!(DET_NULL, 1e6 * 2.0f64.powi(-126));
        let s = 2.0f32.powi(-42); // s³ = 2^-126
        let at = [[1e6 * s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]];
        assert_eq!(matrix3_det(&at).to_f64(), DET_NULL);
        assert_eq!(matrix3_inverse(&at, &mut out), Ok(()));
        assert_eq!(matrix3_fast_inverse(&at, &mut out), Ok(()));
        let below = [[999999.94 * s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]];
        assert!(matrix3_inverse(&below, &mut out).is_err());
        assert!(matrix3_fast_inverse(&below, &mut out).is_err());
        // a NaN determinant is not "below": both go on and produce NaNs
        let nan = [[f32::NAN, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        assert_eq!(matrix3_inverse(&nan, &mut out), Ok(()));
        assert!(out[0][0].is_nan());
        assert_eq!(matrix3_fast_inverse(&nan, &mut out), Ok(()));
        assert!(out[0][0].is_nan());
    }

    #[test]
    fn inverse_has_no_pivoting() {
        // a permutation matrix: determinant −1, but the first pivot is 0
        let p = [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let mut out = [[0.0; 3]; 3];
        assert_eq!(matrix3_inverse(&p, &mut out), Ok(()));
        assert!(out.iter().flatten().any(|v| v.is_nan()));
        // the closed formula has no such trouble
        assert_eq!(matrix3_fast_inverse(&p, &mut out), Ok(()));
        assert_eq!(out, p);
    }

    #[test]
    fn inverse_precision() {
        // 4097² = 2^24 + 2^13 + 1 is not a float, m11 = 4097² + 1 is. The determinant is 1 and
        // the inverse is exact in the register; float arithmetic would lose the odd 1.
        let m = [[1.0, 4097.0, 0.0], [4097.0, 16785410.0, 0.0], [0.0, 0.0, 1.0]];
        assert_eq!(4097.0f32 * 4097.0, 16785408.0);
        assert_eq!(matrix3_det(&m).to_f64(), 1.0);
        let want = [[16785410.0, -4097.0, 0.0], [-4097.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let mut out = [[0.0; 3]; 3];
        assert_eq!(matrix3_inverse(&m, &mut out), Ok(()));
        assert_eq!(out, want);
        assert_eq!(matrix3_fast_inverse(&m, &mut out), Ok(()));
        assert_eq!(out, want);
        // the negated cofactors of the closed formula give −0 where they vanish
        assert_eq!(out[1][2].to_bits(), 0x8000_0000);
        assert_eq!(out[2][1].to_bits(), 0x8000_0000);
        assert_eq!(out[0][2].to_bits(), 0);
    }

    #[test]
    fn fast_inverse_determinant_order() {
        si_rt::skip_without_install!();
        // A = m11·m22 − m12·m21 = 2^64, B = −(m10·m22 − m12·m20) = 1, C = m10·m21 − m11·m20 =
        // −2^32, so det = (1·2^64 + 1·1) + 2^32·(−2^32): the first sum needs 65 bits and
        // rounds back to 2^64, the result is 0 and the matrix is refused, although its true
        // determinant is 1
        let m = [[1.0, 1.0, P32], [0.0, P32, 1.0], [1.0, 0.0, P32]];
        let mut out = [[0.0; 3]; 3];
        assert_eq!(matrix3_fast_inverse(&m, &mut out), Err(fast_inverse_error()));
        assert_eq!(out, m);
    }

    #[test]
    fn clean_and_rot_clean() {
        let mut m = [[0.5, -0.5, 0.25], [-0.25, 0.75, f32::NAN], [0.0, -0.0, 1.5]];
        matrix3_clean(&mut m, 0.5);
        // strictly below: ±0.5 stay; −0 becomes +0; the NaN stays
        assert_eq!(bits(&m)[0], [0.5f32.to_bits(), (-0.5f32).to_bits(), 0]);
        assert_eq!(m[1][0].to_bits(), 0);
        assert_eq!(m[1][1], 0.75);
        assert!(m[1][2].is_nan());
        assert_eq!(bits(&m)[2], [0, 0, 1.5f32.to_bits()]);
        let before = m;
        matrix3_clean(&mut m, f32::NAN);
        assert_eq!(bits(&m), bits(&before));

        let mut r = [[0.999, 0.001, -0.001], [-0.999, 0.9, -0.9], [1.0, -1.0, f32::NAN]];
        matrix3_rot_clean(&mut r, 0.01, 0.99);
        assert_eq!(r[0], [1.0, 0.0, 0.0]);
        assert_eq!(r[1], [-1.0, 0.9, -0.9]);
        assert_eq!(r[2][..2], [1.0, -1.0]);
        assert!(r[2][2].is_nan());
        // strict comparisons: an element equal to the limit is not snapped
        let mut e = [[0.99, -0.99, 0.01], [0.0; 3], [0.0; 3]];
        matrix3_rot_clean(&mut e, 0.01, 0.99);
        assert_eq!(e[0], [0.99, -0.99, 0.01]);
        // the passes run one after the other: with the limit −0.5, the element −0.3 is above
        // the limit (so it becomes 1) and below 0.5 (but by then it is 1); only −0.7 ends at −1
        let mut odd = [[0.3, -0.3, 2.0], [-0.7, 0.0, 0.0], [0.0; 3]];
        matrix3_rot_clean(&mut odd, 0.0, -0.5);
        assert_eq!(odd, [[1.0, 1.0, 1.0], [-1.0, 1.0, 1.0], [1.0, 1.0, 1.0]]);
    }

    #[test]
    fn orthonormal_only_normalises_columns() {
        // columns (3, 4, 0), (0, 0, 0), (1, 1, 1): not orthogonal before, not after
        let a = [[3.0, 0.0, 1.0], [4.0, 0.0, 1.0], [0.0, 0.0, 1.0]];
        let mut out = [[7.0; 3]; 3];
        matrix3_orthonormal(&a, &mut out);
        let third = 1.0f32 / 3.0f32.sqrt();
        assert_eq!(out, [[0.6, 7.0, third], [0.8, 7.0, third], [0.0, 7.0, third]]); // the zero column is not written
        let dot = out[0][0] * out[0][2] + out[1][0] * out[1][2] + out[2][0] * out[2][2];
        assert!(dot > 0.5);
        // the squared length is summed in the register: 4096² + 4097² + 1 = 33562626 exactly,
        // whose root rounds to 5793.326171875; summing in floats gives 5793.32568359375
        let a = [[4096.0, 0.0, 0.0], [4097.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        let len = 5793.326171875f32;
        assert_eq!((33562626.0f64.sqrt()) as f32, len);
        assert_eq!((4096.0f32 * 4096.0 + 4097.0 * 4097.0 + 1.0).sqrt(), 5793.32568359375);
        let mut out = [[0.0; 3]; 3];
        matrix3_orthonormal(&a, &mut out);
        assert_eq!([out[0][0], out[1][0], out[2][0]], [4096.0 / len, 4097.0 / len, 1.0 / len]);
        // a NaN column is not "zero length": it is written
        let a = [[f32::NAN, 1.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 2.0]];
        matrix3_orthonormal(&a, &mut out);
        assert!(out[0][0].is_nan() && out[1][0].is_nan() && out[2][0].is_nan());
        assert_eq!([out[0][1], out[1][1], out[2][1]], [1.0, 0.0, 0.0]);
        assert_eq!([out[0][2], out[1][2], out[2][2]], [0.0, 0.0, 1.0]);
    }

    #[test]
    fn rot_pack_and_unpack() {
        let z90 = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let (a, b) = matrix3_rot_pack(&z90);
        assert_eq!(a, Vector::new(0.0, -1.0, 0.0)); // first column
        assert_eq!(b, Vector::new(1.0, 0.0, 0.0)); // second column
        assert_eq!(matrix3_rot_unpack(a, b), z90);
        let r = matrix3_xyz_rotation(&Vector::new(0.4, 1.3, -2.2));
        let (a, b) = matrix3_rot_pack(&r);
        assert!(close(&matrix3_rot_unpack(a, b), &r, 1e-6));
        // nothing is checked: any two vectors give their cross product as third column
        let m = matrix3_rot_unpack(Vector::new(1.0, 2.0, 3.0), Vector::new(4.0, 5.0, 6.0));
        assert_eq!(m, [[1.0, 4.0, -3.0], [2.0, 5.0, 6.0], [3.0, 6.0, -3.0]]);
        // two products and a difference in the register: 4097·4097 − 16785410·1 = −1
        let m = matrix3_rot_unpack(Vector::new(0.0, 4097.0, 16785410.0), Vector::new(0.0, 1.0, 4097.0));
        assert_eq!(m[0][2], -1.0);
        assert_eq!(4097.0f32 * 4097.0 - 16785410.0, -2.0);
    }
}
