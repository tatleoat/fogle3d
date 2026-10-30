// SPDX-License-Identifier: MIT
//! The basis matrices of the four spline types and the forward-difference matrix.
//!
//! A segment's point at the local parameter `u` is `[u³ u² u 1] × B × G` (row vectors, as
//! everywhere in the program: finding 0008), `B` being one of the matrices here and `G` the
//! segment's four rows of geometry. All constants are single floats in the original's
//! read-only data unless stated.

use si_math::algebra::matrix_multiply;
use si_math::types::Matrix;
use si_x87::{fmov, x, F80};

use crate::{Curve, BEZIER, BSPLINE, CARDINAL, LINEAR};

/// The Bézier basis [soft 0x0848b348 SI_matrixBezier]: the cubic Bernstein polynomials,
/// rows for u³, u², u, 1 and columns for the four control points. Class: copy.
pub fn matrix_bezier() -> Matrix {
    [[-1.0, 3.0, -3.0, 1.0], [3.0, -6.0, 3.0, 0.0], [-3.0, 3.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]
}

/// The uniform cubic B-spline basis [soft 0x0848b3c0 si_matrixBspline]. A sixth is the
/// float 0x3e2aaaab and two thirds the float 0x3f2aaaab, each the nearest float (so the
/// four weights of a point do not sum to exactly 1). Class: copy.
pub fn matrix_bspline() -> Matrix {
    let sixth = f32::from_bits(0x3e2a_aaab);
    let two_thirds = f32::from_bits(0x3f2a_aaab);
    [[-sixth, 0.5, -0.5, sixth], [0.5, -1.0, 0.5, 0.0], [-0.5, 0.0, 0.5, 0.0], [sixth, two_thirds, sixth, 0.0]]
}

/// The linear basis [soft 0x0848b430 si_matrixLinear]: the point runs from the first row of
/// geometry to the second; the cubic and quadratic rows are zero. Class: copy.
pub fn matrix_linear() -> Matrix {
    [[0.0; 4], [0.0; 4], [-1.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]
}

/// The cardinal basis for a tension `t` [soft 0x0848b480 si_matrixCardinal]: the curve
/// passes through the second and third rows of geometry with the tangents
/// `t × (third − first)` and `t × (fourth − second)`. With t = 0.5 this is the Catmull-Rom
/// spline. The constants 2 and 3 are doubles; each element is one operation on the float
/// tension, rounded to float. Class: single.
pub fn matrix_cardinal(t: f32) -> Matrix {
    let t2 = (x(t) + t).to_f32();
    let nt = (-x(t)).to_f32();
    [
        [nt, (x(2.0f64) - t).to_f32(), (x(t) + -2.0f64).to_f32(), fmov(t)],
        [t2, (x(t) - 3.0f64).to_f32(), (x(3.0f64) - (x(t) + t)).to_f32(), nt],
        [nt, 0.0, fmov(t), 0.0],
        [0.0, 1.0, 0.0, 0.0],
    ]
}

/// Writes the basis of a spline type into `m` [soft 0x0847a7c0 SI_splineBasisMatrix]
/// (`patch.c`). The tension matters to the cardinal type only. **Any other type leaves `m`
/// untouched**: the original's callers hand in an uninitialised local, so for a type
/// outside 0..=3 they go on with whatever the stack held; a port's caller hands in zeros
/// and says so. Class: copy.
pub fn spline_basis_matrix(kind: i32, tension: f32, m: &mut Matrix) {
    match kind {
        LINEAR => *m = matrix_linear(),
        BEZIER => *m = matrix_bezier(),
        BSPLINE => *m = matrix_bspline(),
        CARDINAL => *m = matrix_cardinal(tension),
        _ => {}
    }
}

/// The basis of a spline's type, with zeros standing for the uninitialised matrix the
/// original would use for a type outside 0..=3.
pub(crate) fn basis_of(kind: i32, tension: f32) -> Matrix {
    let mut m = [[0.0f32; 4]; 4];
    spline_basis_matrix(kind, tension, &mut m);
    m
}

/// The forward-difference matrix for `n` steps a segment
/// [soft 0x0847ad24 SI_matrixPrecision] (`patch.c`). With d = 1/n rounded to float:
///
/// ```text
/// | 6d³   0    0   0 |
/// | 6d³  2d²   0   0 |
/// |  d³   d²   d   0 |
/// |  0    0    0   1 |
/// ```
///
/// Multiplied onto a segment's polynomial coefficients it gives, top to bottom, the third,
/// second and first forward differences and the value at u = 0. d² and d³ are formed in the
/// register from the rounded d and rounded once each; 6d³ is rounded once from the
/// unrounded d³. Class: model.
pub fn matrix_precision(n: i32) -> Matrix {
    let d = (F80::ONE / F80::from_i32(n)).to_f32();
    let d2 = x(d) * d;
    let d3 = d2 * d;
    let six_d3 = (x(6.0f64) * d3).to_f32();
    let mut m = [[0.0f32; 4]; 4];
    m[2][1] = d2.to_f32();
    m[1][1] = (d2 + d2).to_f32();
    m[3][3] = 1.0;
    m[2][2] = fmov(d);
    m[2][0] = d3.to_f32();
    m[1][0] = six_d3;
    m[0][0] = six_d3;
    m
}

/// Recomputes the spline's own matrix (+0x24) [soft 0x0848abd0 SI_splineUpdateMatrix]: the
/// forward-difference matrix for the spline's step count (one step for a linear spline,
/// whatever its step says) times the basis of its type. The drawing code multiplies it onto
/// each segment's geometry. Class: model.
pub fn update_matrix(c: &mut Curve) {
    let steps = if c.rec.kind != 0 { c.rec.step as i32 } else { 1 };
    let basis = basis_of(c.rec.kind, c.rec.tension);
    c.rec.matrix = matrix_multiply(&matrix_precision(steps), &basis);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bases_reproduce_their_end_points() {
        // u = 0 picks the last row; u = 1 the sum of the rows
        let sum = |m: &Matrix, j: usize| m[0][j] + m[1][j] + m[2][j] + m[3][j];
        let b = matrix_bezier();
        assert_eq!(b[3], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!([sum(&b, 0), sum(&b, 1), sum(&b, 2), sum(&b, 3)], [0.0, 0.0, 0.0, 1.0]);
        let c = matrix_cardinal(0.5);
        assert_eq!(c[0], [-0.5, 1.5, -1.5, 0.5]);
        assert_eq!(c[1], [1.0, -2.5, 2.0, -0.5]);
        assert_eq!([sum(&c, 0), sum(&c, 1), sum(&c, 2), sum(&c, 3)], [0.0, 0.0, 1.0, 0.0]);
        let l = matrix_linear();
        assert_eq!([sum(&l, 0), sum(&l, 1)], [0.0, 1.0]);
        let s = matrix_bspline();
        assert_eq!(s[3][1].to_bits(), 0x3f2a_aaab);
        assert_eq!(s[0][0].to_bits(), 0xbe2a_aaab);
    }

    #[test]
    fn precision_matrix_of_ten_steps() {
        let m = matrix_precision(10);
        let d = 0.1f32;
        assert_eq!(m[2][2], d);
        assert_eq!(m[2][1], ((d as f64) * (d as f64)) as f32);
        assert_eq!(m[0][0], m[1][0]);
        assert_eq!(m[3], [0.0, 0.0, 0.0, 1.0]);
        // an unknown type leaves the caller's matrix alone
        let mut keep = [[7.0f32; 4]; 4];
        spline_basis_matrix(4, 0.0, &mut keep);
        assert_eq!(keep, [[7.0f32; 4]; 4]);
    }
}
