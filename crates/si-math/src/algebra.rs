// SPDX-License-Identifier: MIT
//! Vectors, 4 × 4 matrices, rotations and transformations: the original's `algebra.c`
//! (finding 0008).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Each port repeats the original's x87 instruction sequence: the
//! same operations in the same order on [`F80`] register values, rounded to `f32`/`f64`
//! exactly where the original stores to memory. The doc comment of each function gives its
//! exactness class:
//!
//! - **copy**: only moves values.
//! - **single**: every stored value is one operation on floats, so plain `f32` arithmetic has
//!   the same bits.
//! - **model**: several operations accumulate in a register before a store; bit-exact given
//!   the x87 model of [`crate::x87`].
//! - **libm**: goes through `sin`, `cos`, `asin` or `acos` ([`crate::trig`]); equal to the
//!   original only as far as those are, which cannot be shown.
//!
//! Conventions, all read from the code: vectors are rows and multiply from the left
//! (`v' = v × M`), translation is row 3, angles are radians, and a rotation by three angles is
//! `X × Y × Z` (the X rotation acts first).

use crate::text::printf_f;
use crate::trig;
use crate::types::{Hvector, Matrix, Matrix3, MatrixD, Vector, IDENTITY};
use crate::x87::{fmov, fmov64, x, Round, F80};

/// The tolerance most tests in this file use: the **double** 0.0001. The original compares
/// 80-bit values with it, so it must not be shortened to a float.
const EPS_1E4: f64 = 1e-4;
/// π, 2π and 1/2π as the doubles the original has in its read-only data (0x08662260,
/// 0x08662270, 0x08662268).
const PI_D: f64 = f64::from_bits(0x4009_21fb_5444_2d18);
const TWO_PI_D: f64 = f64::from_bits(0x4019_21fb_5444_2d18);
const INV_TWO_PI_D: f64 = f64::from_bits(0x3fc4_5f30_6dc9_c883);

/// C's `_BOUND(v, -1, 1)` as the compiler laid it out: −1 for v ≤ −1, 1 for v ≥ 1, otherwise
/// v itself, unrounded; a NaN passes through.
fn bound_unit(v: F80) -> F80 {
    if v.le(x(-1.0f64)) {
        x(-1.0f32)
    } else if F80::ONE.le(v) {
        F80::ONE
    } else {
        v
    }
}

/// `v − floor(v × 1/2π) × 2π`: an angle brought into [0, 2π), rounded to float. The floor is
/// stored to a double in between (exact for any float angle below 2^53 turns).
fn wrap_two_pi(v: f32) -> f32 {
    let turns = (x(v) * INV_TWO_PI_D).round_int(Round::Down).to_f64();
    (x(v) - x(turns) * TWO_PI_D).to_f32()
}

// ------------------------------------------------------------------ angles between vectors

/// The angle at `b` between the directions to `a` and to `c`, in [0, π]
/// [soft 0x08515ed0 SI_pointsAngle]. Class: libm (through [`vector_angle`]).
pub fn points_angle(a: &Vector, b: &Vector, c: &Vector) -> f32 {
    let u = Vector::new(a.x - b.x, a.y - b.y, a.z - b.z);
    let v = Vector::new(c.x - b.x, c.y - b.y, c.z - b.z);
    vector_angle(&u, &v)
}

/// The cosine of the angle between `b − a` and `c − b`: the turn at `b` on the way from `a`
/// to `c`, not the angle of [`points_angle`] [soft 0x08515f28 SI_pointsCosAngle].
/// Class: model.
pub fn points_cos_angle(a: &Vector, b: &Vector, c: &Vector) -> f32 {
    let u = Vector::new(b.x - a.x, b.y - a.y, b.z - a.z);
    let v = Vector::new(c.x - b.x, c.y - b.y, c.z - b.z);
    vector_cos_angle(&u, &v)
}

/// [`points_angle`] with a sign: multiplied by [`point_side`]`(c, a, b)` when that is not
/// zero, so the sign says on which side of the line a–b, seen in the XY plane, `c` lies
/// [soft 0x08515f80 SI_pointsAngleSigned]. Class: libm.
pub fn points_angle_signed(a: &Vector, b: &Vector, c: &Vector) -> f32 {
    let angle = points_angle(a, b, c);
    match point_side(c, a, b) {
        0 => angle,
        side => (F80::from_i32(side) * angle).to_f32(), // ±1 × angle: exact
    }
}

/// The angle between two vectors, in [0, π] [soft 0x08516004 SI_vectorAngle].
///
/// The cosine is formed in two steps, each rounded to float: dot / |a|, then that / |b|
/// (not dot / (|a| |b|)). It is clamped to [−1, 1] and passed to `acos`. A zero vector gives
/// 0/0, a NaN, which goes through the clamp and comes back as NaN. Class: libm.
pub fn vector_angle(a: &Vector, b: &Vector) -> f32 {
    let dot = (x(a.y) * b.y + x(a.x) * b.x) + x(a.z) * b.z;
    let len_a = (x(a.z) * a.z + (x(a.y) * a.y + x(a.x) * a.x)).sqrt();
    let c = (dot / len_a).to_f32();
    let len_b = ((x(b.y) * b.y + x(b.x) * b.x) + x(b.z) * b.z).sqrt();
    let c = (x(c) / len_b).to_f32();
    let c = if x(c).le(x(-1.0f64)) {
        -1.0
    } else if F80::ONE.le(x(c)) {
        1.0
    } else {
        c
    };
    trig::acos(x(c).to_f64()).to_f32()
}

/// The cosine of the angle between two vectors [soft 0x085160c4 SI_vectorCosAngle].
///
/// `dot / (|a| × |b|)` with the product of the two lengths kept in the register; if that
/// product is below 0.0001 the result is 1. Not clamped: it can exceed 1 by rounding.
/// Class: model.
pub fn vector_cos_angle(a: &Vector, b: &Vector) -> f32 {
    let len_a = ((x(a.y) * a.y + x(a.x) * a.x) + x(a.z) * a.z).sqrt();
    let len_b = ((x(b.y) * b.y + x(b.x) * b.x) + x(b.z) * b.z).sqrt();
    let lengths = len_a * len_b;
    if lengths.abs().lt(x(EPS_1E4)) {
        return 1.0;
    }
    let dot = x(a.z) * b.z + (x(b.y) * a.y + x(a.x) * b.x);
    (dot / lengths).to_f32()
}

/// Scales a vector to length 1, in place [soft 0x08516158 SI_vectorNormalize].
///
/// The length is rounded to float, then its reciprocal is rounded to float, and each
/// component is multiplied by that (it does not divide by the length). A vector whose rounded
/// length is not above 0.0001 (or is NaN) is left alone. Class: model for the length, single
/// for the rest.
pub fn vector_normalize(v: &mut Vector) {
    let len = ((x(v.y) * v.y + x(v.x) * v.x) + x(v.z) * v.z).sqrt().to_f32();
    if x(len).abs().gt(x(EPS_1E4)) {
        let inv = (F80::ONE / len).to_f32();
        v.x = (x(v.x) * inv).to_f32();
        v.z = (x(v.z) * inv).to_f32();
        v.y = (x(inv) * v.y).to_f32();
    }
}

/// The distance between two points [soft 0x08517004 SI_vectorDistance]. The three differences
/// stay in the register unrounded; the root is rounded to float before it is returned.
/// Class: model.
pub fn vector_distance(a: &Vector, b: &Vector) -> f32 {
    let (dx, dy, dz) = (x(b.x) - a.x, x(b.y) - a.y, x(b.z) - a.z);
    ((dy * dy + dx * dx) + dz * dz).sqrt().to_f32()
}

/// The part of `b` that is perpendicular to `a`, in the plane of the two
/// [soft 0x08519894 SI_vectorOrthogonize].
///
/// With c = a × b and d = c × a the result is d × (b · d) / (d · d). `None` (the original
/// returns 0 and writes nothing) when |c.x| + |c.y| + |c.z| is below 0.0001: the vectors are
/// parallel, or one is zero. Everything up to the three final stores stays in the register.
/// Class: model.
pub fn vector_orthogonize(a: &Vector, b: &Vector) -> Option<Vector> {
    let cx = x(a.y) * b.z - x(a.z) * b.y;
    let cy = x(b.x) * a.z - x(b.z) * a.x;
    let cz = x(b.y) * a.x - x(a.y) * b.x;
    if (cz.abs() + (cy.abs() + cx.abs())).lt(x(EPS_1E4)) {
        return None;
    }
    let dx = cy * a.z - cz * a.y;
    let dz = cx * a.y - x(a.x) * cy;
    let dy = cz * a.x - cx * a.z;
    let k = (x(b.z) * dz + (x(b.y) * dy + x(b.x) * dx)) / ((dy * dy + dx * dx) + dz * dz);
    Some(Vector::new((k * dx).to_f32(), (dy * k).to_f32(), (dz * k).to_f32()))
}

// ------------------------------------------------------------------ points, lines, triangles

/// A line: a point and a direction (`SI_Line`, 24 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Line {
    pub point: Vector,
    pub dir: Vector,
}

/// A plane: a point and a normal (`SI_Plane`, 24 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Plane {
    pub point: Vector,
    pub normal: Vector,
}

/// On which side of the line a–b the point `p` lies, **in the XY plane** (z is ignored):
/// −1, 0 or 1 [soft 0x08516ecc pointSide].
///
/// The value tested is (p.y − a.y)(b.x − a.x) − (p.x − a.x)(b.y − a.y), all in the register;
/// below −0.0001 gives −1, above 0.0001 gives 1, anything else, a NaN included, 0.
/// Class: model.
pub fn point_side(p: &Vector, a: &Vector, b: &Vector) -> i32 {
    let (px, py) = (x(p.x) - a.x, x(p.y) - a.y);
    let (bx, by) = (x(b.x) - a.x, x(b.y) - a.y);
    let cross = py * bx - px * by;
    if cross.lt(x(-EPS_1E4)) {
        -1
    } else {
        cross.gt(x(EPS_1E4)) as i32
    }
}

/// Whether `p` lies on the segment from `a` towards `b`, in the XY plane
/// [soft 0x08516f4c SI_pointBetween].
///
/// False unless [`point_side`] is 0. Then the position along the segment is taken from y if
/// |b.y − a.y| exceeds 0.0001, else from x if |b.x − a.x| does, else the answer is false; it
/// must lie strictly between −0.0001 and 0.9999. So `a` itself counts and `b` does not.
/// Class: model.
pub fn point_between(p: &Vector, a: &Vector, b: &Vector) -> bool {
    if point_side(p, a, b) != 0 {
        return false; // the original runs the range test below on −1, which fails it
    }
    let eps = x(EPS_1E4);
    let dy = x(b.y) - a.y;
    let t = if eps.lt(dy.abs()) {
        (x(p.y) - a.y) / dy
    } else {
        let dx = x(b.x) - a.x;
        if !dx.abs().gt(eps) {
            return false;
        }
        (x(p.x) - a.x) / dx
    };
    t.gt(x(-EPS_1E4)) && t.lt(x(0.9999f64))
}

/// Where `p` is relative to the triangle a, b, c in the XY plane
/// [soft 0x08517f2c SI_pointInsideTriangle].
///
/// With s = [`point_side`]`(a, b, c)` (the triangle's orientation, ±1): 0 if the triangle is
/// degenerate (s = 0) or `p` is on the outer side of any edge; s if `p` is on one edge; 2s if
/// it is strictly inside. A point on two edges at once, a corner, also gives 0: the original
/// computes `s << (n − 2)` for n = the number of edges `p` is properly inside of, truncated to
/// 16 bits, and for n < 2 the shift count wraps to 30 or 31.
pub fn point_inside_triangle(p: &Vector, a: &Vector, b: &Vector, c: &Vector) -> i16 {
    let s = point_side(a, b, c);
    if s == 0 {
        return 0;
    }
    let s1 = point_side(p, a, b);
    if s1 == -s {
        return 0;
    }
    let s2 = point_side(p, b, c);
    if s2 == -s {
        return 0;
    }
    let s3 = point_side(p, c, a);
    if s3 == -s {
        return 0;
    }
    let n = (s1 + s2 + s3).abs();
    ((s as u32) << ((n - 2) as u32 & 31)) as i16
}

/// Three points on one line? True when the squared length of (a − b) × (c − b) is below 1e-8
/// [soft 0x08518348 SI_pointsColinear]. The tolerance is absolute, so the answer depends on
/// the scale of the data. Class: model.
pub fn points_colinear(a: &Vector, b: &Vector, c: &Vector) -> bool {
    let (ux, uy, uz) = (x(a.x) - b.x, x(a.y) - b.y, x(a.z) - b.z);
    let (vx, vy, vz) = (x(c.x) - b.x, x(c.y) - b.y, x(c.z) - b.z);
    let cy = vx * uz - vz * ux;
    let cx = uy * vz - uz * vy;
    let cz = vy * ux - uy * vx;
    ((cy * cy + cx * cx) + cz * cz).abs().lt(x(1e-8f64))
}

/// The same test from `a`, with the far looser tolerance 0.05
/// [soft 0x085183d0 SI_pointsColinearDelta]. Class: model.
pub fn points_colinear_delta(a: &Vector, b: &Vector, c: &Vector) -> bool {
    let (ux, uy, uz) = (x(b.x) - a.x, x(b.y) - a.y, x(b.z) - a.z);
    let (vx, vy, vz) = (x(c.x) - a.x, x(c.y) - a.y, x(c.z) - a.z);
    let cx = uy * vz - uz * vy;
    let cy = uz * vx - ux * vz;
    let cz = vy * ux - vx * uy;
    (cz * cz + (cy * cy + cx * cx)).abs().lt(x(0.05f64))
}

/// The unit normal of the triangle a, b, c: (b − a) × (c − a), normalised
/// [soft 0x08519480 SI_triangleNormal].
///
/// Returns the normal and the original's result: `false` with a zero vector when the cross
/// product's length, rounded to float, is not above 1e-5. The length's reciprocal is rounded
/// to float and multiplies the unrounded cross product. Class: model.
pub fn triangle_normal(a: &Vector, b: &Vector, c: &Vector) -> (Vector, bool) {
    let (ux, uy, uz) = (x(b.x) - a.x, x(b.y) - a.y, x(b.z) - a.z);
    let (vx, vy, vz) = (x(c.x) - a.x, x(c.y) - a.y, x(c.z) - a.z);
    let nx = uy * vz - uz * vy;
    let nz = vy * ux - vx * uy;
    let ny = uz * vx - ux * vz;
    let len = (nz * nz + (nx * nx + ny * ny)).sqrt().to_f32();
    if !x(len).abs().gt(x(1e-5f64)) {
        return (Vector::default(), false);
    }
    let inv = (F80::ONE / len).to_f32();
    (Vector::new((nx * inv).to_f32(), (ny * inv).to_f32(), (nz * inv).to_f32()), true)
}

/// **Twice** the area of the triangle a, b, c: the length of b × c + a × b + c × a
/// [soft 0x08517fc8 SI_areaTriangle3D]. The name says area; nothing halves it.
///
/// Two of the six products of the z component are rounded to float on the way (the compiler
/// ran out of registers and spilled them), the rest stays in the register. Class: model.
pub fn area_triangle_3d(a: Vector, b: Vector, c: Vector) -> f32 {
    let cx_ay = (x(c.x) * a.y).to_f32();
    let cy_ax = (x(c.y) * a.x).to_f32();
    let nx = ((x(b.y) * c.z - x(b.z) * c.y) + (x(a.y) * b.z - x(a.z) * b.y)) + (x(c.y) * a.z - x(c.z) * a.y);
    let ny = ((x(b.z) * c.x - x(b.x) * c.z) + (x(a.z) * b.x - x(a.x) * b.z)) + (x(c.z) * a.x - x(c.x) * a.z);
    let nz = ((x(b.x) * c.y - x(b.y) * c.x) + (x(a.x) * b.y - x(a.y) * b.x)) + (x(cx_ay) - cy_ax);
    (nz * nz + (ny * ny + nx * nx)).sqrt().to_f32()
}

/// The distance from `p` to the plane of the triangle a, b, c
/// [soft 0x08519eb0 SI_pointToPlaneDist].
///
/// |n · p − n · a| / |n| with n from [`triangle_normal`], whose failure is ignored: for a
/// degenerate triangle n is zero and the result is 0/0, a NaN. Class: model.
pub fn point_to_plane_dist(p: Vector, a: Vector, b: Vector, c: Vector) -> f32 {
    let (n, _) = triangle_normal(&a, &b, &c);
    let dp = x(n.z) * p.z + (x(n.x) * p.x + x(n.y) * p.y);
    let da = x(n.z) * a.z + (x(n.y) * a.y + x(n.x) * a.x);
    let len = ((x(n.y) * n.y + x(n.x) * n.x) + x(n.z) * n.z).sqrt();
    ((dp + -da).abs() / len).to_f32()
}

/// Where a line meets a plane [soft 0x08518974 SI_linePlaneIntersection].
///
/// point + t × dir with t = n · (plane.point − line.point) / (n · dir). When |n · dir| is not
/// above 1e-6 (the line is parallel to the plane) the original skips the division and uses
/// the numerator itself as t: the result is then a meaningless point, not an error.
/// Class: model.
pub fn line_plane_intersection(line: &Line, plane: &Plane) -> Vector {
    let n = &plane.normal;
    let (dx, dy, dz) = (x(plane.point.x) - line.point.x, x(plane.point.y) - line.point.y, x(plane.point.z) - line.point.z);
    let denom = (x(line.dir.x) * n.x + x(line.dir.y) * n.y) + x(n.z) * line.dir.z;
    let num = (x(n.y) * dy + x(n.x) * dx) + dz * n.z;
    let t = if denom.abs().gt(x(1e-6f64)) { num / denom } else { num };
    Vector::new(
        (t * line.dir.x + line.point.x).to_f32(),
        (t * line.dir.y + line.point.y).to_f32(),
        (t * line.dir.z + line.point.z).to_f32(),
    )
}

/// Where two lines cross **in the YZ plane** [soft 0x08519f34 SI_lineLineIntersection].
///
/// `out.x` always receives the first line's point x. If the directions' YZ cross product is
/// below 0.0001 in size the answer is `false` and `out.y`, `out.z` are not written; otherwise
/// they get the crossing point, computed along the second line. Class: model.
pub fn line_line_intersection(l1: &Line, l2: &Line, out: &mut Vector) -> bool {
    out.x = fmov(l1.point.x);
    let cross = x(l1.dir.y) * l2.dir.z - x(l1.dir.z) * l2.dir.y;
    if cross.abs().lt(x(EPS_1E4)) {
        return false;
    }
    let t = (x(l1.dir.z) * (x(l2.point.y) - l1.point.y) - x(l1.dir.y) * (x(l2.point.z) - l1.point.z)) / cross;
    out.y = (x(l2.dir.y) * t + l2.point.y).to_f32();
    out.z = (t * l2.dir.z + l2.point.z).to_f32();
    true
}

/// Not an on-the-segment test, whatever the name says: true when `p` is no farther from `a`
/// than `b` is, and no farther from `b` than `a` is, each with 0.0001 to spare
/// [soft 0x08519fb8 SI_pointInEdge]. That is the lens between two spheres around the end
/// points. The three distances are rounded to float before they are compared. Class: model.
pub fn point_in_edge(p: Vector, a: Vector, b: Vector) -> bool {
    let dist = |u: &Vector, v: &Vector| -> f32 {
        let (dx, dy, dz) = (x(u.x) - v.x, x(u.y) - v.y, x(u.z) - v.z);
        ((dy * dy + dx * dx) + dz * dz).sqrt().to_f32()
    };
    let (ab, pa, pb) = (dist(&a, &b), dist(&p, &a), dist(&p, &b));
    let within = |d: f32| x(d).lt(x(ab)) || (x(d) - ab).abs().lt(x(EPS_1E4));
    within(pa) && within(pb)
}

// ------------------------------------------------------------------ 4 × 4 matrices

/// Element-wise comparison with the tolerance 1e-6 (a double): true when every |a − b| is
/// below it; a NaN anywhere makes the matrices unequal [soft 0x085161c4 SI_matrixEqual].
/// The SDK's `SAA_matrixIsEqual` takes the tolerance as an argument; this function has it
/// built in. Class: model (the difference is compared unrounded).
pub fn matrix_equal(a: &Matrix, b: &Matrix) -> bool {
    for i in 0..4 {
        for j in 0..4 {
            if !(x(a[i][j]) - b[i][j]).abs().lt(x(1e-6f64)) {
                return false;
            }
        }
    }
    true
}

/// Copies sixteen floats, each through the unit [soft 0x08516224 SI_matrixCopy]: a signalling
/// NaN arrives quiet, everything else unchanged. Class: copy.
pub fn matrix_copy(src: &Matrix) -> Matrix {
    src.map(|row| row.map(fmov))
}

/// The matrix product `a × b` [soft 0x08516244 SI_matrixMultiply].
///
/// Element [i][j] is ((a[i][0] b[0][j] + a[i][1] b[1][j]) + a[i][2] b[2][j]) + a[i][3] b[3][j]:
/// the four products are exact in the register, the sum is accumulated left to right at 64
/// bits and rounded to float once. All sixteen results go to locals first and are copied to
/// the destination at the end, so in the original the destination may be either operand.
/// With row vectors `a × b` applies `a` first. Class: model.
pub fn matrix_multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[i][j] = (((x(a[i][0]) * b[0][j] + x(a[i][1]) * b[1][j]) + x(a[i][2]) * b[2][j]) + x(a[i][3]) * b[3][j]).to_f32();
        }
    }
    out
}

/// The first three rows and columns of `a × b`, with the last column set to (0, 0, 0, 1) and
/// the last row to (0, 0, 0, 1) [soft 0x0851650c SI_matrixMultiplySub3].
///
/// This is not the product of the two 3 × 3 blocks: each element still includes the fourth
/// term a[i][3] × b[3][j], so a non-zero last column of `a` leaks `b`'s translation into the
/// result. For matrices whose last column is (0, 0, 0, 1) it is the rotation-and-scale part
/// of the product. Same accumulation and aliasing rule as [`matrix_multiply`]. Class: model.
pub fn matrix_multiply_sub3(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = IDENTITY;
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (((x(a[i][0]) * b[0][j] + x(a[i][1]) * b[1][j]) + x(a[i][2]) * b[2][j]) + x(a[i][3]) * b[3][j]).to_f32();
        }
    }
    out
}

/// The transpose [soft 0x085166ec SI_matrixTranspose]. Class: copy.
///
/// The original writes `out[j][i] = m[i][j]` straight into the destination, row by row of the
/// source, so the destination must not be the source: [`matrix_transpose_in_place`] is what
/// happens if it is.
pub fn matrix_transpose(m: &Matrix) -> Matrix {
    let mut out = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[j][i] = fmov(m[i][j]);
        }
    }
    out
}

/// What [soft 0x085166ec SI_matrixTranspose] leaves when called with the same matrix as
/// source and destination: not the transpose. Each element below the diagonal is overwritten
/// with its mirror image before it is read, so the upper triangle is copied onto the lower
/// one and the matrix comes out symmetric.
pub fn matrix_transpose_in_place(m: &mut Matrix) {
    for i in 0..4 {
        for j in 0..4 {
            m[j][i] = fmov(m[i][j]);
        }
    }
}

/// The inverse by Gauss–Jordan elimination in doubles
/// [soft 0x0851672c SI_matrixInverse]. `None` is the original's 0: no usable pivot, and the
/// destination is left untouched.
///
/// The matrix is widened to doubles next to an identity (4 × 8). For each column k in turn:
/// if |pivot| < 1e-6, the first row below with |entry| ≥ 1e-6 in that column is swapped in
/// (none: fail); row k is multiplied by 1/pivot; every other row i gets row k × (−entry)
/// added, **whatever the entry's size**. There is no search for the largest pivot, so a
/// pivot of 2e-6 is used as it is. Rounding: 1/pivot and the negated entry stay in the
/// register for a whole row (64 bits), each product is added in the register and the sum is
/// rounded to double on the store; the result is rounded to float at the end. Class: model
/// (plain double arithmetic differs in the last bit).
pub fn matrix_inverse(m: &Matrix) -> Option<Matrix> {
    let mut a = [[0.0f64; 8]; 4];
    for i in 0..4 {
        for j in 0..4 {
            a[i][j] = x(m[i][j]).to_f64();
            a[i][4 + j] = if i == j { 1.0 } else { 0.0 };
        }
    }
    gauss_jordan(&mut a)?;
    let mut out = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[i][j] = x(a[i][4 + j]).to_f32();
        }
    }
    Some(out)
}

/// The same elimination on a matrix of doubles, result in doubles
/// [soft 0x085169a8 SI_matrixInverseDouble]. Class: model.
pub fn matrix_inverse_double(m: &MatrixD) -> Option<MatrixD> {
    let mut a = [[0.0f64; 8]; 4];
    for i in 0..4 {
        for j in 0..4 {
            a[i][j] = fmov64(m[i][j]);
            a[i][4 + j] = if i == j { 1.0 } else { 0.0 };
        }
    }
    gauss_jordan(&mut a)?;
    let mut out = [[0.0f64; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[i][j] = fmov64(a[i][4 + j]);
        }
    }
    Some(out)
}

/// The elimination shared by the two inverses (the two originals are the same code but for
/// the element type of the argument).
fn gauss_jordan(a: &mut [[f64; 8]; 4]) -> Option<()> {
    let eps = x(1e-6f64);
    for k in 0..4 {
        if eps.gt(x(a[k][k]).abs()) {
            let mut r = k + 1;
            while r < 4 && x(a[r][k]).abs().lt(eps) {
                r += 1;
            }
            if r == 4 {
                return None;
            }
            a.swap(r, k);
        }
        let inv = F80::ONE / a[k][k];
        for j in 0..8 {
            a[k][j] = (x(a[k][j]) * inv).to_f64();
        }
        for i in 0..4 {
            if i != k {
                let f = -x(a[i][k]);
                for j in 0..8 {
                    a[i][j] = (x(a[k][j]) * f + a[i][j]).to_f64();
                }
            }
        }
    }
    Some(())
}

/// The inverse of `b` times `a`: the matrix that takes `b` to `a`, D with `b × D = a`
/// [soft 0x08518104 si_matrixDifference]. `None` (the original returns 0 and writes nothing)
/// when `b` cannot be inverted. Class: model.
pub fn matrix_difference(a: &Matrix, b: &Matrix) -> Option<Matrix> {
    let inv = matrix_inverse(b)?;
    Some(matrix_multiply(&inv, a))
}

/// The transpose of the inverse, for transforming normals, with the inverse's translation
/// row cleared first so the result's last column is (0, 0, 0, 1)
/// [soft 0x08518a3c SI_matrixTransposeInverse].
///
/// Returns the matrix and the original's result; when the inverse fails the matrix is the
/// identity and the result `false`. Class: model.
pub fn matrix_transpose_inverse(m: &Matrix) -> (Matrix, bool) {
    match matrix_inverse(m) {
        None => (matrix_copy(&IDENTITY), false),
        Some(mut inv) => {
            inv[3][0] = 0.0;
            inv[3][1] = 0.0;
            inv[3][2] = 0.0;
            (matrix_transpose(&inv), true)
        }
    }
}

/// The determinant of the upper-left 3 × 3 block
/// [soft 0x08519cd4 SI_matrixDeterminant3].
///
/// Expanded along the first row as (m21 m10 − m20 m11) m02 + ((m11 m22 − m12 m21) m00 −
/// (m22 m10 − m12 m20) m01), all in the register. The original returns the register value
/// without rounding it to float, hence the `F80`: what the caller does with it decides the
/// rounding. Class: model.
pub fn matrix_determinant3(m: &Matrix) -> F80 {
    (x(m[2][1]) * m[1][0] - x(m[2][0]) * m[1][1]) * m[0][2]
        + ((x(m[1][1]) * m[2][2] - x(m[1][2]) * m[2][1]) * m[0][0] - (x(m[2][2]) * m[1][0] - x(m[1][2]) * m[2][0]) * m[0][1])
}

/// Transforms homogeneous points: `v × m`, all four components
/// [soft 0x08516c28 SI_hvectorMultiplyMatrix].
///
/// Unlike [`matrix_multiply`] the running sum lives in memory: for each output component it
/// starts at 0 and after each of the four terms is rounded to float
/// (t = float(v[k] × m[k][j] + t), k = 0..3). The original takes a count and two arrays and
/// finishes each vector before storing it, so source and destination may be the same array.
/// Class: model (each step is a product and a sum before the store).
pub fn hvector_multiply_matrix(v: &Hvector, m: &Matrix) -> Hvector {
    let v = v.to_array();
    let mut out = [0.0f32; 4];
    for j in 0..4 {
        let mut t = 0.0f32;
        for k in 0..4 {
            t = (x(v[k]) * m[k][j] + t).to_f32();
        }
        out[j] = fmov(t);
    }
    Hvector::from_array(out)
}

/// The same on doubles: four-component rows times a 4 × 4 matrix of doubles, the running sum
/// rounded to double after each term [soft 0x08516cd0 SI_hvectorMultiplyMatrixDouble].
/// Class: model.
pub fn hvector_multiply_matrix_double(v: &[f64; 4], m: &MatrixD) -> [f64; 4] {
    let mut out = [0.0f64; 4];
    for j in 0..4 {
        let mut t = 0.0f64;
        for k in 0..4 {
            t = (x(v[k]) * m[k][j] + t).to_f64();
        }
        out[j] = fmov64(t);
    }
    out
}

/// Transforms points: `(x, y, z, 1) × m`, three components
/// [soft 0x08516d74 SI_vectorMultiplyMatrix].
///
/// The running sum starts at the translation m[3][j] and is rounded to float after each
/// term: t = float(v[k] × m[k][j] + t) for k = 0, 1, 2. So the order is
/// ((m3j + x m0j) + y m1j) + z m2j with three roundings, which differs in the last bits from
/// [`bbox_multiply_matrix`], which computes the same expression another way. Count and
/// aliasing as for [`hvector_multiply_matrix`]. Class: model.
pub fn vector_multiply_matrix(v: &Vector, m: &Matrix) -> Vector {
    let v = v.to_array();
    let mut out = [0.0f32; 3];
    for j in 0..3 {
        let mut t = fmov(m[3][j]);
        for k in 0..3 {
            t = (x(v[k]) * m[k][j] + t).to_f32();
        }
        out[j] = fmov(t);
    }
    Vector::from_array(out)
}

/// Transforms the eight corners of a box: `(x, y, z, 1) × m` for each
/// [soft 0x08516e28 SI_bboxMultiplyMatrix].
///
/// Here each coordinate is ((x m0j + y m1j) + z m2j) + m3j accumulated in the register and
/// rounded once. The original checks whether source and destination are the same array and
/// then works in a local copy, so they may be. Class: model.
pub fn bbox_multiply_matrix(corners: &[[f32; 3]; 8], m: &Matrix) -> [[f32; 3]; 8] {
    let mut out = [[0.0f32; 3]; 8];
    for (p, o) in corners.iter().zip(out.iter_mut()) {
        for j in 0..3 {
            o[j] = (((x(p[0]) * m[0][j] + x(p[1]) * m[1][j]) + x(p[2]) * m[2][j]) + m[3][j]).to_f32();
        }
    }
    out
}

/// Solves a linear system in place by Gauss–Jordan elimination on a flat array of floats:
/// `rows` rows of `cols` floats, eliminating the first `n` columns
/// [soft 0x08519a60 SI_resolveLinearSystem]. `false` when a pivot column has no entry of at
/// least 1e-6 at or below the diagonal (the array is left half reduced).
///
/// For each k below `n`: if |a[k][k]| < 1e-6 the first row below with a larger entry is
/// swapped in; the row is multiplied by the reciprocal of the pivot **rounded to float**,
/// but only if |pivot − 1| > 1e-6; every other row whose entry in column k exceeds 1e-6 in
/// size gets row k × (−entry) added, each element as float(a[k][j] × (−entry) + a[i][j]).
/// The array must hold `rows × cols` floats (the original does not check). Class: model.
pub fn resolve_linear_system(a: &mut [f32], rows: i16, cols: i16, n: i16) -> bool {
    let (rows, cols, n) = (rows as i32, cols as i32, n as i32);
    let at = |r: i32, c: i32| (r * cols + c) as usize;
    let eps = x(1e-6f64);
    let mut k = 0;
    while k < n {
        if x(a[at(k, k)]).abs().lt(eps) {
            let mut r = k + 1;
            while r < rows && x(a[at(r, k)]).abs().lt(eps) {
                r += 1;
            }
            if r == rows {
                return false;
            }
            for j in 0..cols {
                let (lo, hi) = (fmov(a[at(r, j)]), fmov(a[at(k, j)]));
                a[at(r, j)] = hi;
                a[at(k, j)] = lo;
            }
        }
        let pivot = a[at(k, k)];
        if (x(pivot) - F80::ONE).abs().gt(eps) {
            let inv = (F80::ONE / pivot).to_f32();
            for j in 0..cols {
                a[at(k, j)] = (x(a[at(k, j)]) * inv).to_f32();
            }
        }
        for i in 0..rows {
            if i != k {
                let entry = a[at(i, k)];
                if x(entry).abs().gt(eps) {
                    let f = -x(entry);
                    for j in 0..cols {
                        a[at(i, j)] = (x(a[at(k, j)]) * f + a[at(i, j)]).to_f32();
                    }
                }
            }
        }
        k += 1;
    }
    true
}

// ------------------------------------------------------------------ building matrices

/// A scaling matrix: the identity with (x, y, z) on the diagonal
/// [soft 0x08517048 SI_matrixScaling]. Class: copy.
pub fn matrix_scaling(s: &Vector) -> Matrix {
    let mut m = matrix_copy(&IDENTITY);
    m[0][0] = fmov(s.x);
    m[1][1] = fmov(s.y);
    m[2][2] = fmov(s.z);
    m
}

/// A translation matrix: the identity with the vector in row 3
/// [soft 0x08517e58 SI_matrixTranslation]. Class: copy.
pub fn matrix_translation(t: &Vector) -> Matrix {
    let mut m = matrix_copy(&IDENTITY);
    m[3][0] = fmov(t.x);
    m[3][1] = fmov(t.y);
    m[3][2] = fmov(t.z);
    m
}

/// The cosine and sine of an angle as the rotation builders obtain them: the float angle
/// widened to double, `cos` and `sin` of the C library, each result rounded to float.
fn cos_sin(angle: f32) -> (f32, f32) {
    let a = x(angle).to_f64();
    (trig::cos(a).to_f32(), trig::sin(a).to_f32())
}

/// The same two values obtained in the other order, `sin` first: what
/// `SI_matrixXyzRotation` does for each of its angles. The order changes no result; it is
/// kept so that a replay of the original's library calls (finding 0011) follows call by call.
fn sin_cos(angle: f32) -> (f32, f32) {
    let a = x(angle).to_f64();
    let s = trig::sin(a).to_f32();
    (s, trig::cos(a).to_f32())
}

/// Rotation about X by an angle in radians [soft 0x08517080 SI_matrixXRotation]:
/// m[1][1] = m[2][2] = cos, m[1][2] = sin, m[2][1] = −sin. With row vectors that turns +Y
/// towards +Z. Class: libm.
pub fn matrix_x_rotation(angle: f32) -> Matrix {
    let mut m = matrix_copy(&IDENTITY);
    let (c, s) = cos_sin(angle);
    m[2][2] = c;
    m[1][1] = c;
    m[1][2] = s;
    m[2][1] = -s;
    m
}

/// Rotation about Y [soft 0x085170e0 SI_matrixYRotation]: m[0][0] = m[2][2] = cos,
/// m[2][0] = sin, m[0][2] = −sin (+Z towards +X). Class: libm.
pub fn matrix_y_rotation(angle: f32) -> Matrix {
    let mut m = matrix_copy(&IDENTITY);
    let (c, s) = cos_sin(angle);
    m[2][2] = c;
    m[0][0] = c;
    m[2][0] = s;
    m[0][2] = -s;
    m
}

/// Rotation about Z [soft 0x08517140 SI_matrixZRotation]: m[0][0] = m[1][1] = cos,
/// m[0][1] = sin, m[1][0] = −sin (+X towards +Y). Class: libm.
pub fn matrix_z_rotation(angle: f32) -> Matrix {
    let mut m = matrix_copy(&IDENTITY);
    let (c, s) = cos_sin(angle);
    m[1][1] = c;
    m[0][0] = c;
    m[0][1] = s;
    m[1][0] = -s;
    m
}

/// Rotation by an angle about an axis through the origin
/// [soft 0x085171a0 SI_matrixAxisRotation]. The axis is used as given: it is not normalised,
/// and a non-unit axis gives a matrix that is not a rotation.
///
/// With c, s the float cosine and sine and t = 1 − c: the diagonal is c + t·a², the
/// off-diagonal pairs are t·a·b ± s·(third component), signs as in [`matrix_x_rotation`].
/// Rounding, as the compiler arranged it: t·x is formed from the unrounded t and rounded to
/// float; t·y and t·z² use t rounded to **double** and are rounded to float; every element is
/// then one product pair summed in the register and rounded once. Class: libm.
pub fn matrix_axis_rotation(angle: f32, axis: &Vector) -> Matrix {
    let mut m = matrix_copy(&IDENTITY);
    let (c, s) = cos_sin(angle);
    let t = F80::ONE - c;
    let t_d = t.to_f64();
    let tx = (t * axis.x).to_f32();
    let ty = (x(t_d) * axis.y).to_f32();
    let tzz = (x(axis.z) * axis.z * t_d).to_f32();
    let (ax, ay, az) = (axis.x, axis.y, axis.z);
    m[0][0] = (x(ax) * tx + c).to_f32();
    m[0][1] = (x(az) * s + x(ay) * tx).to_f32();
    m[0][2] = (-x(ay) * s + x(tx) * az).to_f32();
    m[1][0] = (x(ay) * tx + -x(az) * s).to_f32();
    m[1][1] = (x(ay) * ty + c).to_f32();
    m[1][2] = (x(ax) * s + x(az) * ty).to_f32();
    m[2][0] = (x(tx) * az + x(ay) * s).to_f32();
    m[2][1] = (x(az) * ty + x(s) * -x(ax)).to_f32();
    m[2][2] = (x(tzz) + c).to_f32();
    m
}

/// The rotation by three angles in radians: `X × Y × Z`, the X rotation acting first
/// [soft 0x0851882c SI_matrixXyzRotation].
///
/// The six sines and cosines are rounded to float; every element is then built from those
/// floats in the register and rounded once:
/// row 0 = (cy cz, cy sz, −sy), row 1 = (sx sy cz − cx sz, sx sy sz + cx cz, sx cy),
/// row 2 = (sx sz + cx sy cz, cx sy sz − sx cz, cx cy), with sx·sy and cx·sy formed first.
/// The result equals the product of the three single rotations only up to rounding.
/// Class: libm.
pub fn matrix_xyz_rotation(rot: &Vector) -> Matrix {
    let (sx, cx) = sin_cos(rot.x);
    let (sy, cy) = sin_cos(rot.y);
    let (sz, cz) = sin_cos(rot.z);
    let mut m = [[0.0f32; 4]; 4];
    m[0][0] = (x(cy) * cz).to_f32();
    m[0][1] = (x(cy) * sz).to_f32();
    m[0][2] = -sy;
    let sxsy = x(sx) * sy;
    m[1][0] = (sxsy * cz - x(cx) * sz).to_f32();
    m[1][1] = (sxsy * sz + x(cx) * cz).to_f32();
    m[1][2] = (x(sx) * cy).to_f32();
    let cxsy = x(cx) * sy;
    m[2][0] = (x(sx) * sz + cxsy * cz).to_f32();
    m[2][1] = (x(sz) * cxsy - x(cz) * sx).to_f32();
    m[2][2] = (x(cx) * cy).to_f32();
    m[3][3] = 1.0;
    m
}

/// A model's transformation matrix from scaling, rotation (radians) and translation:
/// `S × R × T` with R = [`matrix_xyz_rotation`] [soft 0x085199b8 SI_matrixScalRotTrans].
///
/// Not computed as a product: row i of the rotation is multiplied by the i-th scaling factor
/// (one multiplication per element), the translation is copied into row 3 and the last
/// column is (0, 0, 0, 1). Class: libm (single apart from the rotation).
pub fn matrix_scal_rot_trans(scl: &Vector, rot: &Vector, trs: &Vector) -> Matrix {
    let r = matrix_xyz_rotation(rot);
    let s = scl.to_array();
    let mut m = [[0.0f32; 4]; 4];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = (x(r[i][j]) * s[i]).to_f32();
        }
    }
    m[3][0] = fmov(trs.x);
    m[3][1] = fmov(trs.y);
    m[3][2] = fmov(trs.z);
    m[3][3] = 1.0;
    m
}

/// The reflection in the plane through the origin with unit normal n: I − 2 n nᵀ
/// [soft 0x08517e90 SI_matrixSymmetry]. The diagonal is 1 − n·(n + n), the other elements
/// (−2 n_i) n_j, each rounded once. The normal is not normalised. Class: model.
pub fn matrix_symmetry(n: &Vector) -> Matrix {
    let mut m = matrix_copy(&IDENTITY);
    m[0][0] = (F80::ONE - x(n.x) * (x(n.x) + n.x)).to_f32();
    m[1][1] = (F80::ONE - x(n.y) * (x(n.y) + n.y)).to_f32();
    m[2][2] = (F80::ONE - x(n.z) * (x(n.z) + n.z)).to_f32();
    let xy = (x(-2.0f64) * n.x * n.y).to_f32();
    m[1][0] = xy;
    m[0][1] = xy;
    let xz = (x(-2.0f64) * n.x * n.z).to_f32();
    m[2][0] = xz;
    m[0][2] = xz;
    let yz = (x(-2.0f64) * n.y * n.z).to_f32();
    m[2][1] = yz;
    m[1][2] = yz;
    m
}

/// A rotation whose third **column** is the vector `v` (the SDK's
/// `SAA_matrixAlignWithZAxis`) [soft 0x08518aa0 SI_matrixZAlign].
///
/// With d² = v.x² + v.y²: if d² is not above 0.0001 (v along Z) the result is the identity,
/// or for v.z < 0 the identity with m[1][1] = m[2][2] = −1. Otherwise, with
/// p = v.y / d and q = −v.x / d (unrounded): column 0 = (q v.z, −p v.z, v.y p − v.x q),
/// column 1 = (p, q, 0), column 2 = v. `v` is taken as it is, not normalised. Class: model.
pub fn matrix_z_align(v: &Vector) -> Matrix {
    let nx = -x(v.x);
    let d2 = x(v.y) * v.y + nx * nx;
    if !(d2.gt(x(EPS_1E4)) || d2.unordered(x(EPS_1E4))) {
        let mut m = matrix_copy(&IDENTITY);
        if x(v.z).lt(F80::ZERO) {
            m[2][2] = -1.0;
            m[1][1] = -1.0;
        }
        return m;
    }
    let d = d2.sqrt();
    let p = x(v.y) / d;
    let q = nx / d;
    let mut m = [[0.0f32; 4]; 4];
    m[0][0] = (q * v.z).to_f32();
    m[1][0] = (-p * v.z).to_f32();
    m[2][0] = (x(v.y) * p - x(v.x) * q).to_f32();
    m[0][1] = p.to_f32();
    m[1][1] = q.to_f32();
    m[0][2] = fmov(v.x);
    m[1][2] = fmov(v.y);
    m[2][2] = fmov(v.z);
    m[3][3] = 1.0;
    m
}

/// A rotation whose second column is `v` (`SAA_matrixAlignWithYAxis`)
/// [soft 0x08518bcc SI_matrixYAlign].
///
/// With d² = v.x² + v.z²: not above 0.0001 gives the identity, or for v.y < 0 the identity
/// with m[0][0] = m[1][1] = −1. Otherwise i = 1/d **rounded to float**, and with
/// a = −v.x i, b = v.z i, o = 0 × i (all unrounded): column 0 = (b, o, a), column 1 = v,
/// column 2 = (o v.z − a v.y, a v.x − v.z b, b v.y − o v.x). Class: model.
pub fn matrix_y_align(v: &Vector) -> Matrix {
    let mut m = matrix_copy(&IDENTITY);
    let nx = -x(v.x);
    let d2 = nx * nx + (x(v.z) * v.z + F80::ZERO);
    if !(d2.gt(x(EPS_1E4)) || d2.unordered(x(EPS_1E4))) {
        if x(v.y).lt(F80::ZERO) {
            m[0][0] = -1.0;
            m[1][1] = -1.0;
        }
        return m;
    }
    let inv = (F80::ONE / d2.sqrt()).to_f32();
    let a = nx * inv;
    let b = x(inv) * v.z;
    let o = F80::ZERO * inv;
    m[0][0] = b.to_f32();
    m[1][0] = o.to_f32();
    m[2][0] = a.to_f32();
    m[0][1] = fmov(v.x);
    m[1][1] = fmov(v.y);
    m[2][1] = fmov(v.z);
    m[0][2] = (o * v.z - a * v.y).to_f32();
    m[1][2] = (a * v.x - x(v.z) * b).to_f32();
    m[2][2] = (b * v.y - o * v.x).to_f32();
    m
}

/// A rotation whose first column is `v` (`SAA_matrixAlignWithXAxis`)
/// [soft 0x08518cf8 SI_matrixXAlign].
///
/// With d² = v.y² + v.z²: not above 0.0001 gives the identity, or for v.x < 0 the identity
/// with m[0][0] = m[2][2] = −1. Otherwise i = 1/d rounded to float, and with a = i v.y,
/// b = −v.z i, o = 0 × i: column 0 = v, column 1 = (b v.z − a v.y, a v.x − o v.z,
/// o v.y − b v.x), column 2 = (o, b, a). Class: model.
pub fn matrix_x_align(v: &Vector) -> Matrix {
    let mut m = matrix_copy(&IDENTITY);
    let nz = -x(v.z);
    let d2 = x(v.y) * v.y + (nz * nz + F80::ZERO);
    if !(d2.gt(x(EPS_1E4)) || d2.unordered(x(EPS_1E4))) {
        if x(v.x).lt(F80::ZERO) {
            m[0][0] = -1.0;
            m[2][2] = -1.0;
        }
        return m;
    }
    let inv = (F80::ONE / d2.sqrt()).to_f32();
    let a = x(inv) * v.y;
    let o = F80::ZERO * inv;
    let b = nz * inv;
    m[0][0] = fmov(v.x);
    m[1][0] = fmov(v.y);
    m[2][0] = fmov(v.z);
    m[0][1] = (b * v.z - a * v.y).to_f32();
    m[1][1] = (a * v.x - o * v.z).to_f32();
    m[2][1] = (o * v.y - b * v.x).to_f32();
    m[0][2] = o.to_f32();
    m[1][2] = b.to_f32();
    m[2][2] = a.to_f32();
    m
}

fn near_zero(v: &Vector) -> bool {
    // the first component is compared the other way round in the original (ε against |x|),
    // which matters only for a NaN: a NaN x counts as "not near zero" either way
    let eps = x(EPS_1E4);
    eps.gt((x(v.x) - 0.0f32).abs()) && (x(v.y) - 0.0f32).abs().lt(eps) && (x(v.z) - 0.0f32).abs().lt(eps)
}

/// Divides a vector by its length, each quotient rounded to float, if the length rounded to
/// float is above 0.0001 (the SDK's `_VCT_NORMALIZE` macro, which the two functions below
/// expand in line; [`vector_normalize`] multiplies by a reciprocal instead).
fn normalize_by_division(v: &mut Vector) {
    let len = ((x(v.y) * v.y + x(v.x) * v.x) + x(v.z) * v.z).sqrt().to_f32();
    if x(len).abs().gt(x(EPS_1E4)) {
        v.x /= len;
        v.y /= len;
        v.z /= len;
    }
}

/// An orthonormal frame whose second row is the direction of `y` and whose first row is as
/// close to `xv` as that allows; rows are X, Y, Z = X × Y
/// [soft 0x08518e18 SI_matrixAlignYAxes]. `None` (the original returns 0 and writes nothing)
/// when every component of `y` is below 0.0001 in size.
///
/// If `xv` is that small it is replaced by y + (1, 0, 0). Both are normalised (see
/// [`vector_normalize`] for when that is skipped). Z = X × Y; if Z is tiny (X parallel to Y)
/// X becomes Y × (0, 0, 1) and Z = X × Y with neither normalised; otherwise Z is normalised
/// and X = Y × Z. Class: model.
pub fn matrix_align_y_axes(xv: &Vector, y: &Vector) -> Option<Matrix> {
    if near_zero(y) {
        return None;
    }
    let eps = x(EPS_1E4);
    let mut xa = *xv;
    if near_zero(&xa) {
        xa = Vector::new((F80::ONE + y.x).to_f32(), fmov(y.y), fmov(y.z));
    }
    let mut ya = *y;
    normalize_by_division(&mut ya);
    normalize_by_division(&mut xa);
    let zx = x(xa.y) * ya.z - x(xa.z) * ya.y;
    let zz = x(xa.x) * ya.y - x(xa.y) * ya.x;
    let zy = x(xa.z) * ya.x - x(xa.x) * ya.z;
    let mut za = Vector::new(zx.to_f32(), zy.to_f32(), zz.to_f32());
    if (zx - 0.0f32).abs().lt(eps) && (zy - 0.0f32).abs().lt(eps) && (zz - 0.0f32).abs().lt(eps) {
        let wz = x(ya.x) * 0.0f32 - F80::ZERO * ya.y;
        let wx = x(ya.y) - x(ya.z) * 0.0f32;
        let wy = x(ya.z) * 0.0f32 - ya.x;
        xa = Vector::new(wx.to_f32(), wy.to_f32(), wz.to_f32());
        za.x = (wy * ya.z - wz * ya.y).to_f32();
        za.z = (x(ya.y) * wx - x(ya.x) * wy).to_f32();
        za.y = (wz * ya.x - x(ya.z) * wx).to_f32();
    } else {
        normalize_by_division(&mut za);
        xa.x = (x(ya.y) * za.z - x(ya.z) * za.y).to_f32();
        xa.z = (x(za.y) * ya.x - x(za.x) * ya.y).to_f32();
        xa.y = (x(ya.z) * za.x - x(za.z) * ya.x).to_f32();
    }
    Some(frame(&xa, &ya, &za))
}

fn frame(r0: &Vector, r1: &Vector, r2: &Vector) -> Matrix {
    let mut m = [[0.0f32; 4]; 4];
    for (row, v) in [r0, r1, r2].into_iter().enumerate() {
        m[row][0] = fmov(v.x);
        m[row][1] = fmov(v.y);
        m[row][2] = fmov(v.z);
    }
    m[3][3] = 1.0;
    m
}

/// An orthonormal frame whose first row is the direction of `xv` and whose second row is as
/// close to `y` as that allows (`SAA_matrixAlignWithXYAxes`); rows are X, Y = Z × X, Z
/// [soft 0x085191ac SI_matrixAlignAxes]. Returns the matrix and the original's result: the
/// identity and `false` when every component of `xv` is below 0.0001 in size.
///
/// Z = X × Y is formed from the vectors as given; then X is normalised; if Z is tiny the
/// second vector is replaced by (X.x, X.y + 1, X.z) and Z recomputed; Z is normalised and the
/// second row is Z × X, computed in the register and rounded once. `y` itself is never
/// normalised. Class: model.
pub fn matrix_align_axes(xv: &Vector, y: &Vector) -> (Matrix, bool) {
    let eps = x(EPS_1E4);
    let small = |v: f32| (x(v) - 0.0f32).abs().lt(eps);
    let mut xa = *xv;
    if small(xa.x) && small(xa.y) && small(xa.z) {
        return (matrix_copy(&IDENTITY), false);
    }
    let mut za = Vector::new(
        (x(xa.y) * y.z - x(xa.z) * y.y).to_f32(),
        (x(xa.z) * y.x - x(y.z) * xa.x).to_f32(),
        (x(y.y) * xa.x - x(y.x) * xa.y).to_f32(),
    );
    let len = ((x(xa.y) * xa.y + x(xa.x) * xa.x) + x(xa.z) * xa.z).sqrt().to_f32();
    if eps.lt(x(len).abs()) {
        xa.x /= len;
        xa.y /= len;
        xa.z /= len;
    }
    if small(za.x) && small(za.y) && small(za.z) {
        let w = (F80::ONE + xa.y).to_f32();
        za.y = (x(xa.z) * xa.x - x(xa.z) * xa.x).to_f32();
        za.x = (x(xa.y) * xa.z - x(xa.z) * w).to_f32();
        za.z = (x(xa.x) * w - x(xa.y) * xa.x).to_f32();
    }
    normalize_by_division(&mut za);
    let y_new = Vector::new(
        (x(za.y) * xa.z - x(za.z) * xa.y).to_f32(),
        (x(za.z) * xa.x - x(za.x) * xa.z).to_f32(),
        (x(za.x) * xa.y - x(za.y) * xa.x).to_f32(),
    );
    (frame(&xa, &y_new, &za), true)
}

// ------------------------------------------------------------------ taking matrices apart

/// The scaling of a transformation matrix: the lengths of its first three rows, each
/// accumulated in the register as (m_i0² + m_i1²) + m_i2² and rounded to float after the
/// square root [soft 0x085182e4 SI_matrixExtractScaling]. Always positive: a mirrored matrix
/// is not recognised. Class: model.
pub fn matrix_extract_scaling(m: &Matrix) -> Vector {
    let len = |r: &[f32; 4]| ((x(r[0]) * r[0] + x(r[1]) * r[1]) + x(r[2]) * r[2]).sqrt().to_f32();
    Vector::new(len(&m[0]), len(&m[1]), len(&m[2]))
}

/// Scaling, the rotation as a matrix, and translation of a transformation matrix
/// [soft 0x08518228 SI_matrixExtractTransformations2].
///
/// The rotation matrix is the identity with each of the first three rows of `m` divided by
/// that row's length (one division per element). A zero row divides by zero. The translation
/// is row 3. Class: model for the lengths, single for the rest.
pub fn matrix_extract_transformations2(m: &Matrix) -> (Vector, Matrix, Vector) {
    let scl = matrix_extract_scaling(m);
    let s = scl.to_array();
    let mut rot = matrix_copy(&IDENTITY);
    for j in 0..3 {
        for i in 0..3 {
            rot[i][j] = (x(m[i][j]) / s[i]).to_f32();
        }
    }
    (scl, rot, Vector::new(fmov(m[3][0]), fmov(m[3][1]), fmov(m[3][2])))
}

/// Scaling, rotation angles and translation of a transformation matrix: the inverse of
/// [`matrix_scal_rot_trans`] [soft 0x08518144 SI_matrixExtractTransformations].
///
/// The rows are divided by their lengths as in [`matrix_extract_transformations2`] and the
/// angles come from [`matrix_extract_xyz_rotation`], which needs the previous angles (in the
/// original the rotation argument is read before it is written) and the "keep turns"
/// setting. Class: libm.
pub fn matrix_extract_transformations(m: &Matrix, previous_rot: &Vector, keep_turns: bool) -> (Vector, Vector, Vector) {
    let (scl, rot, trs) = matrix_extract_transformations2(m);
    (scl, matrix_extract_xyz_rotation(&rot, previous_rot, keep_turns), trs)
}

/// The angle whose sine is `s` and cosine `c`, in [−π, π] [soft 0x08519d28 arcSinCos].
///
/// Both are clamped to [−1, 1]. If |c| > 0.70710679 the angle comes from `asin(s)`, mirrored
/// to π − a or −π − a when c is negative; otherwise from `acos(c)` with the sign of
/// `asin(s)`. The two inverse functions' results are rounded to float; the mirrored forms
/// are returned unrounded, hence the `F80`. Class: libm.
pub fn arc_sin_cos(s: f64, c: f64) -> F80 {
    let a = trig::asin(bound_unit(x(s)).to_f64()).to_f32();
    let c = bound_unit(x(c)).to_f64();
    let b = trig::acos(c).to_f32();
    if x(c).abs().gt(x(f64::from_bits(0x3fe6_a09e_6b3a_8b97))) {
        if F80::ZERO.gt(x(c)) {
            if x(a).gt(F80::ZERO) {
                x(PI_D) - a
            } else {
                x(-PI_D) - a
            }
        } else {
            x(a)
        }
    } else if F80::ZERO.gt(x(a)) {
        -x(b)
    } else {
        x(b)
    }
}

/// How far apart two angles are on the circle, in [0, π] [soft 0x08519e04 angleDistance].
///
/// Each angle is brought into [0, 2π) and rounded to float; the result is the absolute
/// difference d, unrounded, or float(2π − d) when d exceeds π. Class: model.
pub fn angle_distance(a: f32, b: f32) -> F80 {
    let d = (x(wrap_two_pi(a)) - wrap_two_pi(b)).abs();
    if d.gt(x(PI_D)) {
        x((x(TWO_PI_D) - d).to_f32())
    } else {
        d
    }
}

/// The three angles (radians) of a rotation matrix built as `X × Y × Z`: the inverse of
/// [`matrix_xyz_rotation`] [soft 0x085172b0 SI_matrixExtractXyzRotation].
///
/// A rotation has two sets of such angles; the original picks the one nearer to the angles
/// its output argument held on entry (`previous`), so that an object being manipulated does
/// not flip. In the original the result replaces `previous` in place.
///
/// - y₁ = asin(−m[0][2]) (clamped), y₂ = ±π − y₁ with the sign of y₁.
/// - If |cos y₁| > 1e-5: x and z for each y from [`arc_sin_cos`] of
///   (m[1][2], m[2][2]) / cos y and (m[0][1], m[0][0]) / cos y; the set with the smaller sum
///   of [`angle_distance`]s to `previous` wins (the second on a tie).
/// - Otherwise (gimbal lock; only x − z or x + z is determined): the determined combination
///   comes from `arc_sin_cos(±m[1][0], m[1][1])` and the other one is taken from `previous`;
///   then the same choice between (x, y₁, z) and (x + π, y₂, z + π) is made on x and z, and y
///   is chosen again on its own.
/// - `keep_turns` is the short at 0x08781580, inside the global `SI_glbManip` (0x08781560),
///   set to 1 at start-up and switched from the preferences menu: when set, each angle is
///   moved by whole turns to within π of `previous`.
///
/// Class: libm. The rounding points are those of the original, so given the same `asin`,
/// `acos`, `cos` values the result is the same bits.
pub fn matrix_extract_xyz_rotation(m: &Matrix, previous: &Vector, keep_turns: bool) -> Vector {
    // the previous angles, each brought into [−π, π)
    let wrap_pi = |v: f32| -> f32 {
        let turns = ((x(v) + PI_D) * INV_TWO_PI_D).round_int(Round::Down).to_f64();
        (x(v) - x(turns) * TWO_PI_D).to_f32()
    };
    let (px, py, pz) = (wrap_pi(previous.x), wrap_pi(previous.y), wrap_pi(previous.z));

    let ry1 = trig::asin(bound_unit(-x(m[0][2])).to_f64()).to_f32();
    let ry2 = (if F80::ZERO.ge(x(ry1)) { x(-PI_D) } else { x(PI_D) } - ry1).to_f32();
    let cy = trig::cos(x(ry1).to_f64()).to_f32();

    let mut rot;
    if x(cy).abs().gt(x(1e-5f64)) {
        let solve = |cy: f32| -> (f32, f32) {
            let rx = arc_sin_cos((x(m[1][2]) / cy).to_f64(), (x(m[2][2]) / cy).to_f64()).to_f32();
            let rz = arc_sin_cos((x(m[0][1]) / cy).to_f64(), (x(m[0][0]) / cy).to_f64()).to_f32();
            (rx, rz)
        };
        let (rx1, rz1) = solve(cy);
        let (rx2, rz2) = solve(trig::cos(x(ry2).to_f64()).to_f32());
        let dx = angle_distance(px, rx1).to_f32();
        let dy = angle_distance(py, ry1).to_f32();
        let sum1 = (angle_distance(pz, rz1) + (x(dx) + dy)).to_f32();
        let dx = angle_distance(px, rx2).to_f32();
        let dy = angle_distance(py, ry2).to_f32();
        let sum2 = angle_distance(pz, rz2) + (x(dx) + dy);
        rot = if x(sum1).lt(sum2) { Vector::new(rx1, ry1, rz1) } else { Vector::new(rx2, ry2, rz2) };
    } else {
        let half = 0.5f64;
        let (rx1, rz1);
        if x(ry1).gt(F80::ZERO) {
            // y = +π/2: the matrix fixes x − z
            let r = arc_sin_cos(x(m[1][0]).to_f64(), x(m[1][1]).to_f64());
            let diff = x(px) - pz;
            let r = if diff.abs().lt(x(1e-5f64)) { r } else { r - diff };
            let h = r * half;
            rx1 = (x(px) + h).to_f32();
            rz1 = (x(pz) - h).to_f32();
        } else {
            // y = −π/2 (or 0, or NaN): the matrix fixes x + z
            let r = arc_sin_cos((-x(m[1][0])).to_f64(), x(m[1][1]).to_f64());
            let r = if r.abs().lt(x(1e-12f64)) { (x(px) + pz) * half } else { r };
            let sum = x(px) + pz;
            let r = if sum.abs().lt(x(1e-5f64)) { r } else { r - sum };
            let h = r * half;
            rx1 = (x(px) + h).to_f32();
            rz1 = (x(pz) + h).to_f32();
        }
        let rx2 = wrap_two_pi((x(rx1) + PI_D).to_f32());
        let rz2 = wrap_two_pi((x(PI_D) + rz1).to_f32());
        let (rx1, rz1) = (wrap_two_pi(rx1), wrap_two_pi(rz1));
        let d = angle_distance(px, rx1).to_f32();
        let sum1 = (angle_distance(pz, rz1) + d).to_f32();
        let d = angle_distance(px, rx2).to_f32();
        let sum2 = angle_distance(pz, rz2) + d;
        rot = if x(sum1).lt(sum2) { Vector::new(rx1, ry1, rz1) } else { Vector::new(rx2, ry2, rz2) };
        let d1 = angle_distance(py, ry1).to_f32();
        rot.y = if angle_distance(py, ry2).gt(x(d1)) { ry1 } else { ry2 };
    }
    if keep_turns {
        let near = |prev: f32, v: f32| -> f32 {
            let turns = (((x(prev) + PI_D) - v) / TWO_PI_D).round_int(Round::Down).to_f64();
            (x(turns) * TWO_PI_D + v).to_f32()
        };
        rot = Vector::new(near(previous.x, rot.x), near(previous.y, rot.y), near(previous.z, rot.z));
    }
    rot
}

/// Angles (radians) of a rotation matrix built as `Z × X × Y`, roll first: x = pitch,
/// y = yaw, z = roll [soft 0x08518474 SI_matrixExtractYawPitchRoll].
///
/// pitch = asin(−m[2][1]). If |cos pitch| > 0.0001: roll is asin(m[0][1]/c) when m[1][1] ≥ 0,
/// else ±acos(m[1][1]/c) with the sign of m[0][1]; yaw likewise from m[2][0], m[2][2].
/// Otherwise yaw = 0 and roll is asin(−m[1][0]) when m[0][0] ≥ 0, else ∓acos(m[0][0]) by
/// the sign of m[1][0]. Every argument is clamped to [−1, 1]; the quotients are rounded to
/// double when they are passed. Unlike the XYZ extraction there is no choice of solution and
/// no previous value. Class: libm.
pub fn matrix_extract_yaw_pitch_roll(m: &Matrix) -> Vector {
    let asin = |v: F80| trig::asin(bound_unit(v).to_f64());
    let acos = |v: F80| trig::acos(bound_unit(v).to_f64());
    let pitch = asin(-x(m[2][1])).to_f32();
    let c = trig::cos(x(pitch).to_f64()).to_f32();
    let zero = F80::ZERO;
    if !x(c).abs().gt(x(EPS_1E4)) {
        let roll = if zero.le(x(m[0][0])) {
            asin(-x(m[1][0]))
        } else if zero.ge(x(m[1][0])) {
            acos(x(m[0][0]))
        } else {
            -acos(x(m[0][0]))
        };
        return Vector::new(pitch, 0.0, roll.to_f32());
    }
    let roll = if x(m[1][1]).ge(zero) {
        asin(x(m[0][1]) / c)
    } else if zero.le(x(m[0][1])) {
        acos(x(m[1][1]) / c)
    } else {
        -acos(x(m[1][1]) / c)
    };
    let yaw = if x(m[2][2]).ge(zero) {
        asin(x(m[2][0]) / c)
    } else if zero.le(x(m[2][0])) {
        acos(x(m[2][2]) / c)
    } else {
        -acos(x(m[2][2]) / c)
    };
    Vector::new(pitch, yaw.to_f32(), roll.to_f32())
}

// ------------------------------------------------------------------ quaternions

/// A quaternion as the original stores it: four doubles, w first, then x, y, z.
pub type Quaternion = [f64; 4];

/// Makes a quaternion unit length, in place [soft 0x085179d0 SI_quaternionNormalize].
///
/// n = (z² + (x² + w²))… in the original's order (q2² + (q1² + q0²)) + q3², kept in the
/// register. If |n − 1| < 1e-12 (or n is NaN) nothing is scaled. Else if n < 1e-12 the
/// quaternion becomes (1, 0, 0, 0). Else every component is multiplied by 1/√n, also kept in
/// the register, and rounded to double. In all but the second case w is finally clamped to
/// [−1, 1]. Class: model.
pub fn quaternion_normalize(q: &mut Quaternion) {
    let (q0, q1, q2, q3) = (x(q[0]), x(q[1]), x(q[2]), x(q[3]));
    let n = (q2 * q2 + (q1 * q1 + q0 * q0)) + q3 * q3;
    let eps = x(1e-12f64);
    let d = (n - F80::ONE).abs();
    let w = if eps.gt(d) || eps.unordered(d) {
        q0
    } else if eps.gt(n) {
        *q = [1.0, 0.0, 0.0, 0.0];
        return;
    } else {
        let inv = F80::ONE / n.sqrt();
        let w = q0 * inv;
        q[0] = w.to_f64();
        q[1] = (q1 * inv).to_f64();
        q[2] = (q2 * inv).to_f64();
        q[3] = (q3 * inv).to_f64();
        w
    };
    if w.le(x(-1.0f64)) {
        q[0] = -1.0;
    } else if F80::ONE.le(w) {
        q[0] = 1.0;
    }
}

/// The Hamilton product `a ⊗ b` of two quaternions [soft 0x08517af0 SI_quaternionMul].
///
/// Each component is four products summed in the register and rounded to double once:
/// w = ((a₀b₀ − a₁b₁) − a₂b₂) − a₃b₃, x = (a₂b₃ + (a₀b₁ + a₁b₀)) − a₃b₂,
/// y = a₃b₁ + ((a₀b₂ + a₂b₀) − a₁b₃), z = (a₁b₂ + (a₀b₃ + a₃b₀)) − a₂b₁.
/// All inputs are read before the first store, so the destination may be an operand.
/// Class: model.
pub fn quaternion_mul(a: &Quaternion, b: &Quaternion) -> Quaternion {
    let (a0, a1, a2, a3) = (x(a[0]), x(a[1]), x(a[2]), x(a[3]));
    let (b0, b1, b2, b3) = (x(b[0]), x(b[1]), x(b[2]), x(b[3]));
    [
        (((b0 * a0 - a1 * b1) - a2 * b2) - a3 * b3).to_f64(),
        ((a2 * b3 + (a0 * b1 + b0 * a1)) - b2 * a3).to_f64(),
        (b1 * a3 + ((a0 * b2 + b0 * a2) - a1 * b3)).to_f64(),
        ((a1 * b2 + (b3 * a0 + a3 * b0)) - b1 * a2).to_f64(),
    ]
}

/// The unit quaternion of a 3 × 3 rotation matrix [soft 0x08517bb4 SI_matrixToQuaternion].
/// The argument really is 3 × 3 (rows of three floats), not the upper block of a 4 × 4.
///
/// The usual four-way method: with i the index of the largest diagonal element (ties go to
/// the lower index for 0 against 1, and to the earlier choice against 2), if the trace is
/// larger than that element w = √(trace + 1)/2 and the vector part is the antisymmetric part
/// times 0.25/w; otherwise component i is √(1 + m_ii − m_jj − m_kk)/2, w and the other two
/// come from 0.25 over it, and the whole quaternion is negated if w came out negative.
/// Everything stays in the register until the stores to double. The result then goes
/// through [`quaternion_normalize`]. Class: model.
pub fn matrix_to_quaternion(m: &Matrix3) -> Quaternion {
    let trace = (x(m[0][0]) + m[1][1]) + m[2][2];
    let mut i = if x(m[0][0]).lt(x(m[1][1])) { 1 } else { 0 };
    if x(m[2][2]).gt(x(m[i][i])) {
        i = 2;
    }
    let half = 0.5f64;
    let quarter = 0.25f64;
    let mut q = [0.0f64; 4];
    if trace.gt(x(m[i][i])) {
        let w = (trace + F80::ONE).sqrt() * half;
        q[0] = w.to_f64();
        let t = x(quarter) / w;
        q[1] = ((x(m[1][2]) - m[2][1]) * t).to_f64();
        q[2] = (t * (x(m[2][0]) - m[0][2])).to_f64();
        q[3] = (t * (x(m[0][1]) - m[1][0])).to_f64();
    } else {
        let j = (i + 1) % 3;
        let k = (j + 1) % 3;
        let s = (((x(m[i][i]) + F80::ONE) - m[j][j]) - m[k][k]).sqrt() * half;
        let mut v = [0.0f64; 3];
        v[i] = s.to_f64();
        let t = x(quarter) / s;
        let w = (x(m[j][k]) - m[k][j]) * t;
        v[j] = (t * (x(m[i][j]) + m[j][i])).to_f64();
        v[k] = (t * (x(m[i][k]) + m[k][i])).to_f64();
        if w.lt(F80::ZERO) {
            q = [(-w).to_f64(), -v[0], -v[1], -v[2]];
        } else {
            q = [w.to_f64(), v[0], v[1], v[2]];
        }
    }
    quaternion_normalize(&mut q);
    q
}

/// The rotation axis and angle (radians) of a unit quaternion
/// [soft 0x08517dcc SI_quaternionToAxisAngle].
///
/// angle = acos(w) + acos(w), rounded to double; s = sin of that same sum times 0.5 (rounded
/// to double for the call); the axis is the vector part times 1/s, each rounded to float, or
/// (0, 0, 0) when |s| < 1e-12. Class: libm.
pub fn quaternion_to_axis_angle(q: &Quaternion) -> (Vector, f64) {
    let half_angle = trig::acos(q[0]);
    let angle = half_angle + half_angle;
    let s = trig::sin((angle * 0.5f64).to_f64());
    if s.abs().lt(x(1e-12f64)) {
        return (Vector::default(), angle.to_f64());
    }
    let inv = F80::ONE / s;
    (Vector::new((x(q[1]) * inv).to_f32(), (x(q[2]) * inv).to_f32(), (inv * q[3]).to_f32()), angle.to_f64())
}

// ------------------------------------------------------------------ splines and odds and ends

/// A point on a uniform cubic B-spline segment with control points p0..p3 at parameter u
/// [soft 0x08519770 SI_bsplineInterpolate].
///
/// The four weights are built from floats rounded at every step (u³, 3u³, 3u², 6u², 3u each
/// stored as float): (1 − u)³ as 1 + ((−u³ + 3u²) − 3u), 3u³ − 6u² + 4, and
/// (3u + (3u² − 3u³)) + 1, and u³. Each coordinate is ((w0 p0 + w1 p1) + w2 p2) + w3 p3 in
/// the register, times the **double constant 0.1666667** (seven digits as written in the
/// source, not 1/6), rounded once. Class: model.
pub fn bspline_interpolate(u: f32, p0: &Vector, p1: &Vector, p2: &Vector, p3: &Vector) -> Vector {
    let uu = x(u) * u;
    let u3 = (uu * u).to_f32();
    let a = (x(3.0f64) * u3).to_f32();
    let b = (uu * 3.0f64).to_f32();
    let c = (x(b) + b).to_f32();
    let d = (x(u) * 3.0f64).to_f32();
    let w0 = (F80::ONE + ((-x(u3) + b) - d)).to_f32();
    let w1 = ((x(a) - c) + 4.0f64).to_f32();
    let w2 = ((x(d) + (x(b) + -x(a))) + F80::ONE).to_f32();
    let sixth = f64::from_bits(0x3fc5_5555_9cea_86f2);
    let blend = |c0: f32, c1: f32, c2: f32, c3: f32| -> f32 { ((((x(w0) * c0 + x(w1) * c1) + x(w2) * c2) + x(u3) * c3) * sixth).to_f32() };
    Vector::new(blend(p0.x, p1.x, p2.x, p3.x), blend(p0.y, p1.y, p2.y, p3.y), blend(p0.z, p1.z, p2.z, p3.z))
}

/// The derivative with respect to u of [`bspline_interpolate`]
/// [soft 0x085196a0 SI_bsplineTangentInterpolate].
///
/// With a = float((u × 0.5) × u), b = float(3a), c = float(u + u), the weights, unrounded,
/// are (−a + u) − 0.5, b − c, (−b + u) + 0.5 and a; each coordinate is
/// ((w1 p1 + w0 p0) + w2 p2) + a p3, rounded once. Class: model.
pub fn bspline_tangent_interpolate(u: f32, p0: &Vector, p1: &Vector, p2: &Vector, p3: &Vector) -> Vector {
    let half = 0.5f64;
    let a = (x(u) * half * u).to_f32();
    let b = (x(3.0f64) * a).to_f32();
    let c = (x(u) + u).to_f32();
    let w1 = x(b) - c;
    let w0 = (-x(a) + u) - half;
    let w2 = (-x(b) + u) + half;
    let blend = |c0: f32, c1: f32, c2: f32, c3: f32| -> f32 { (((w1 * c1 + w0 * c0) + w2 * c2) + x(a) * c3).to_f32() };
    Vector::new(blend(p0.x, p1.x, p2.x, p3.x), blend(p0.y, p1.y, p2.y, p3.y), blend(p0.z, p1.z, p2.z, p3.z))
}

/// A floor-style remainder: b × (a/b − float(floor(a/b))) [soft 0x085180c4 SI_myfmod].
///
/// Not C's `fmod`: the result has the sign of `b`, and because the floored quotient is
/// rounded to float before it is subtracted, quotients beyond 2^24 lose their low bits. The
/// product is returned in the register unrounded, hence the `F80`. Class: model.
pub fn myfmod(a: f32, b: f32) -> F80 {
    let q = x(a) / b;
    let whole = x(q.round_int(Round::Down).to_f64()).to_f32();
    x(b) * (q - whole)
}

/// [soft 0x0851a0b8 SI_start3]: returns 13. Called from nowhere in the program.
pub fn start3() -> i32 {
    13
}

/// [soft 0x0851a0c4 SI_add]: the sum of the first two of three integer arguments; the third
/// is ignored. Referenced from the selection code (`rectSelect`, `SI_gLSelectSelected`).
pub fn add(a: i32, b: i32, _c: i32) -> i32 {
    b.wrapping_add(a)
}

/// [soft 0x0851a0d4 SI_sub]: first minus second; the third argument is ignored.
pub fn sub(a: i32, b: i32, _c: i32) -> i32 {
    a.wrapping_sub(b)
}

/// [soft 0x0851a0e8 SI_end3]: returns 12. Called from nowhere in the program.
pub fn end3() -> i32 {
    12
}

// ------------------------------------------------------------------ text dumps

/// The text the original writes to a stream for a matrix: a rule, `DUMP matrix: <title>`,
/// four rows `| %f %f %f %f |`, a rule [soft 0x08519578 SI_matrixDump]. (The original then
/// flushes the stream.)
pub fn matrix_dump(m: &Matrix, title: &str) -> String {
    let rule = "--------------------------------\n";
    let mut s = format!("{rule}DUMP matrix: {title}\n");
    for row in m {
        let f = |v: f32| printf_f(x(v).to_f64(), 0, 6);
        s += &format!("| {} {} {} {} |\n", f(row[0]), f(row[1]), f(row[2]), f(row[3]));
    }
    s + rule
}

/// The text the original prints for a linear system stored as `rows` rows of `cols` floats:
/// per row its number as `%hd)  `, each value as ` %4.2f` with ` |` before column `bar`, and
/// a blank line; one more newline at the end [soft 0x08519c3c SI_dumpLinearSystem].
pub fn dump_linear_system(a: &[f32], rows: i32, cols: i32, bar: i32) -> String {
    let mut s = String::new();
    let mut next = 0usize;
    let mut i = 0;
    while i < rows {
        s += &format!("{})  ", i as i16);
        let mut j = 0;
        while j < cols {
            if j == bar {
                s += " |";
            }
            s += &format!(" {}", printf_f(x(a[next]).to_f64(), 4, 2));
            next += 1;
            j += 1;
        }
        s += "\n\n";
        i += 1;
    }
    s + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x87::testing::Rng;

    const P32: f32 = 4294967296.0; // 2^32
    const P12: f32 = 4096.0;
    const P30: f32 = 1073741824.0;

    fn v(x: f32, y: f32, z: f32) -> Vector {
        Vector::new(x, y, z)
    }

    fn approx(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    fn mat_approx(a: &Matrix, b: &Matrix, tol: f32) -> bool {
        a.iter().flatten().zip(b.iter().flatten()).all(|(p, q)| approx(*p, *q, tol))
    }

    fn rand_unit(rng: &mut Rng) -> f32 {
        (rng.next() >> 40) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
    }

    fn rand_matrix(rng: &mut Rng) -> Matrix {
        let mut m = [[0.0f32; 4]; 4];
        for e in m.iter_mut().flatten() {
            *e = rand_unit(rng) * 3.0;
        }
        m
    }

    #[test]
    fn multiply_accumulates_in_the_register_left_to_right() {
        // row · column = 2^64 + 1 − 2^64 + 1, exactly 2.
        // Left to right at 64 bits: 2^64 + 1 does not fit and rounds back to 2^64 (tie to
        // even); − 2^64 gives 0; + 1 gives 1. Any other association gives 0 or 2, and
        // float-by-float or double arithmetic gives 1 only by the same route.
        let mut a = IDENTITY;
        let mut b = IDENTITY;
        a[0] = [P32, 1.0, -P32, 1.0];
        for (k, val) in [P32, 1.0, P32, 1.0].into_iter().enumerate() {
            b[k][0] = val;
        }
        assert_eq!(matrix_multiply(&a, &b)[0][0], 1.0);
        // 2^24 + 1 − 2^24: 1 in the register; 0 if each step were rounded to float
        a[0] = [P12, 1.0, -P12, 0.0];
        for (k, val) in [P12, 1.0, P12, 0.0].into_iter().enumerate() {
            b[k][0] = val;
        }
        assert_eq!(matrix_multiply(&a, &b)[0][0], 1.0);
        assert_eq!((P12 * P12 + 1.0) - P12 * P12, 0.0);
        // 2^60 + 1 − 2^60: 1 in the register; 0 in double arithmetic
        a[0] = [P30, 1.0, -P30, 0.0];
        for (k, val) in [P30, 1.0, P30, 0.0].into_iter().enumerate() {
            b[k][0] = val;
        }
        assert_eq!(matrix_multiply(&a, &b)[0][0], 1.0);
        assert_eq!((P30 as f64 * P30 as f64 + 1.0) - P30 as f64 * P30 as f64, 0.0);
        // the same holds for the 3 × 3 variant, which keeps the fourth term
        a[0] = [P32, 1.0, -P32, 1.0];
        for (k, val) in [P32, 1.0, P32, 1.0].into_iter().enumerate() {
            b[k][0] = val;
        }
        assert_eq!(matrix_multiply_sub3(&a, &b)[0][0], 1.0);
    }

    #[test]
    fn multiply_identities() {
        let mut rng = Rng(11);
        for _ in 0..200 {
            let a = rand_matrix(&mut rng);
            let b = rand_matrix(&mut rng);
            assert_eq!(matrix_multiply(&a, &IDENTITY), a);
            assert_eq!(matrix_multiply(&IDENTITY, &a), a);
            // (a b)ᵀ = bᵀ aᵀ, within rounding (the two sides add in different orders)
            let lhs = matrix_transpose(&matrix_multiply(&a, &b));
            let rhs = matrix_multiply(&matrix_transpose(&b), &matrix_transpose(&a));
            assert!(mat_approx(&lhs, &rhs, 1e-4));
            // sub3: the product's 3 × 3 block, last row and column of the identity
            let s = matrix_multiply_sub3(&a, &b);
            let full = matrix_multiply(&a, &b);
            for i in 0..3 {
                assert_eq!(s[i][..3], full[i][..3]);
                assert_eq!(s[i][3], 0.0);
            }
            assert_eq!(s[3], [0.0, 0.0, 0.0, 1.0]);
        }
    }

    #[test]
    fn transpose_onto_itself_is_not_a_transpose() {
        let m: Matrix = [[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], [9.0, 10.0, 11.0, 12.0], [13.0, 14.0, 15.0, 16.0]];
        assert_eq!(matrix_transpose(&m)[1], [2.0, 6.0, 10.0, 14.0]);
        let mut same = m;
        matrix_transpose_in_place(&mut same);
        // the upper triangle has been copied over the lower one
        assert_eq!(same, [[1.0, 2.0, 3.0, 4.0], [2.0, 6.0, 7.0, 8.0], [3.0, 7.0, 11.0, 12.0], [4.0, 8.0, 12.0, 16.0]]);
    }

    #[test]
    fn copy_goes_through_the_unit() {
        let mut m = IDENTITY;
        m[2][1] = f32::from_bits(0x7f80_0001); // a signalling NaN
        assert_eq!(matrix_copy(&m)[2][1].to_bits(), 0x7fc0_0001);
        assert_eq!(matrix_copy(&IDENTITY), IDENTITY);
    }

    #[test]
    fn equal_uses_a_fixed_tolerance() {
        let mut b = IDENTITY;
        b[1][2] = 9e-7;
        assert!(matrix_equal(&IDENTITY, &b));
        b[1][2] = 1.1e-6;
        assert!(!matrix_equal(&IDENTITY, &b));
        // the difference is compared, unrounded, with the double 1e-6: the float nearest
        // 1e-6 lies just below it and still counts as equal, the next float up does not
        b[1][2] = 1e-6;
        assert!((1e-6f32 as f64) < 1e-6 && matrix_equal(&IDENTITY, &b));
        b[1][2] = f32::from_bits(1e-6f32.to_bits() + 1);
        assert!(!matrix_equal(&IDENTITY, &b));
        b[1][2] = f32::NAN;
        assert!(!matrix_equal(&b, &b));
    }

    #[test]
    fn inverse_cases() {
        assert_eq!(matrix_inverse(&IDENTITY), Some(IDENTITY));
        // scaling by powers of two and a translation: exact
        let m: Matrix = [[2.0, 0.0, 0.0, 0.0], [0.0, 4.0, 0.0, 0.0], [0.0, 0.0, 0.5, 0.0], [6.0, -8.0, 3.0, 1.0]];
        let inv = matrix_inverse(&m).unwrap();
        assert_eq!(inv, [[0.5, 0.0, 0.0, 0.0], [0.0, 0.25, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0], [-3.0, 2.0, -6.0, 1.0]]);
        // a zero pivot with a usable row below: rows are swapped
        let p: Matrix = [[0.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert_eq!(matrix_inverse(&p), Some(p));
        // no usable pivot: failure
        let mut singular = IDENTITY;
        singular[2][2] = 0.0;
        assert_eq!(matrix_inverse(&singular), None);
        singular[2][2] = 9e-7; // below the tolerance: still "singular"
        assert_eq!(matrix_inverse(&singular), None);
        singular[2][2] = 2e-6; // above it: used as a pivot
        assert_eq!(matrix_inverse(&singular).unwrap()[2][2], 500000.0);
        // a NaN pivot is not "small": the elimination runs and returns NaNs
        let mut nan = IDENTITY;
        nan[0][0] = f32::NAN;
        assert!(matrix_inverse(&nan).unwrap()[0][0].is_nan());
        // m × m⁻¹ = identity within rounding
        let mut rng = Rng(5);
        for _ in 0..300 {
            let mut a = rand_matrix(&mut rng);
            for i in 0..4 {
                a[i][i] += 6.0; // keep it well conditioned
            }
            let inv = matrix_inverse(&a).unwrap();
            assert!(mat_approx(&matrix_multiply(&a, &inv), &IDENTITY, 1e-5));
            let d: MatrixD = a.map(|r| r.map(|e| e as f64));
            let inv_d = matrix_inverse_double(&d).unwrap();
            for i in 0..4 {
                for j in 0..4 {
                    assert_eq!(inv_d[i][j] as f32, inv[i][j]);
                }
            }
            assert!(mat_approx(&matrix_difference(&a, &a).unwrap(), &IDENTITY, 1e-5));
        }
    }

    #[test]
    fn inverse_is_not_plain_double_arithmetic() {
        // the same elimination with every step rounded to double, as C code compiled for a
        // unit without extended registers would do it
        fn plain(m: &MatrixD) -> MatrixD {
            let mut a = [[0.0f64; 8]; 4];
            for i in 0..4 {
                for j in 0..4 {
                    a[i][j] = m[i][j];
                    a[i][4 + j] = (i == j) as u8 as f64;
                }
            }
            for k in 0..4 {
                let inv = 1.0 / a[k][k];
                for j in 0..8 {
                    a[k][j] *= inv;
                }
                for i in 0..4 {
                    if i != k {
                        let f = -a[i][k];
                        for j in 0..8 {
                            a[i][j] = a[k][j] * f + a[i][j];
                        }
                    }
                }
            }
            core::array::from_fn(|i| core::array::from_fn(|j| a[i][4 + j]))
        }
        let mut rng = Rng(99);
        let (mut differing, n) = (0, 200);
        for _ in 0..n {
            let mut a = rand_matrix(&mut rng);
            for i in 0..4 {
                a[i][i] += 6.0;
            }
            let d: MatrixD = a.map(|r| r.map(|e| e as f64));
            let ours = matrix_inverse_double(&d).unwrap();
            let theirs = plain(&d);
            differing += (ours != theirs) as i32;
            // but they agree to about the last bit
            for i in 0..4 {
                for j in 0..4 {
                    assert!((ours[i][j] - theirs[i][j]).abs() <= 1e-13);
                }
            }
        }
        assert!(differing > n / 2, "{differing} of {n}");
    }

    #[test]
    fn transpose_inverse_and_determinant() {
        let m = matrix_scal_rot_trans(&v(2.0, 3.0, 4.0), &v(0.3, -0.4, 1.1), &v(5.0, 6.0, 7.0));
        let (ti, ok) = matrix_transpose_inverse(&m);
        assert!(ok);
        assert_eq!([ti[0][3], ti[1][3], ti[2][3], ti[3][3]], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(ti[3], [0.0, 0.0, 0.0, 1.0]);
        let (id, ok) = matrix_transpose_inverse(&[[0.0; 4]; 4]);
        assert!(!ok);
        assert_eq!(id, IDENTITY);
        // the expansion's order: A m02 + (B m00 − C m01) with A m02 = 1 and both other terms
        // 2^64. As written the result is 1 + (2^64 − 2^64) = 1; summed left to right it would
        // be (1 + 2^64) − 2^64 = 0, because 2^64 + 1 does not fit the register
        let d: Matrix = [[P32, P32, 1.0, 0.0], [1.0, 1.0, 0.0, 0.0], [0.0, 1.0, P32, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert_eq!(matrix_determinant3(&d).to_bits(), F80::ONE.to_bits());
        // determinant of a scaled rotation: the product of the scalings
        assert!(approx(matrix_determinant3(&m).to_f32(), 24.0, 1e-4));
        let s: Matrix = [[2.0, 0.0, 0.0, 9.0], [0.0, 3.0, 0.0, 9.0], [0.0, 0.0, 5.0, 9.0], [9.0, 9.0, 9.0, 9.0]];
        assert_eq!(matrix_determinant3(&s).to_f64(), 30.0);
    }

    #[test]
    fn three_ways_to_transform_a_point_round_differently() {
        // x m00 = 2^24, y m10 = −2^24, z m20 = 0, translation 1
        let mut m = IDENTITY;
        m[0][0] = P12;
        m[1][0] = -P12;
        m[3][0] = 1.0;
        let p = v(P12, P12, 0.0);
        // running sum in memory starting at the translation: 1 + 2^24 rounds to 2^24, so 0
        assert_eq!(vector_multiply_matrix(&p, &m).x, 0.0);
        // one accumulation ending with the translation: (2^24 − 2^24) + 0 + 1 = 1
        assert_eq!(bbox_multiply_matrix(&[[P12, P12, 0.0]; 8], &m)[7][0], 1.0);
        // homogeneous: running sum from 0 with w = 1 last: also 1
        assert_eq!(hvector_multiply_matrix(&Hvector::new(P12, P12, 0.0, 1.0), &m).x, 1.0);
        // and with the terms in the order 2^24, 1, −2^24 the memory sum loses the 1
        let mut m2 = IDENTITY;
        m2[0][0] = P12;
        m2[1][0] = 1.0;
        m2[2][0] = -P12;
        assert_eq!(hvector_multiply_matrix(&Hvector::new(P12, 1.0, P12, 0.0), &m2).x, 0.0);
        let mut a = IDENTITY;
        a[0] = [P12, 1.0, P12, 0.0];
        assert_eq!(matrix_multiply(&a, &m2)[0][0], 1.0);
        // doubles: the same at 2^53
        let big = 9007199254740992.0f64;
        let mut md = [[0.0f64; 4]; 4];
        md[0][0] = big;
        md[1][0] = 1.0;
        md[2][0] = -big;
        assert_eq!(hvector_multiply_matrix_double(&[1.0, 1.0, 1.0, 0.0], &md)[0], 0.0);
        // ordinary data: all agree within rounding
        let t = matrix_scal_rot_trans(&v(1.5, 2.0, 0.5), &v(0.2, 0.3, 0.4), &v(1.0, 2.0, 3.0));
        let q = v(0.25, -1.5, 2.0);
        let r1 = vector_multiply_matrix(&q, &t);
        let r2 = bbox_multiply_matrix(&[[0.25, -1.5, 2.0]; 8], &t)[3];
        let r3 = hvector_multiply_matrix(&Hvector::new(0.25, -1.5, 2.0, 1.0), &t);
        assert!(approx(r1.x, r2[0], 1e-6) && approx(r1.y, r2[1], 1e-6) && approx(r1.z, r2[2], 1e-6));
        assert!(approx(r1.x, r3.x, 1e-6) && approx(r1.z, r3.z, 1e-6) && r3.w == 1.0);
    }

    #[test]
    fn linear_system() {
        // 2x + y = 5, x + 3y = 10
        let mut a = [2.0, 1.0, 5.0, 1.0, 3.0, 10.0];
        assert!(resolve_linear_system(&mut a, 2, 3, 2));
        assert!(approx(a[2], 1.0, 1e-6) && approx(a[5], 3.0, 1e-6));
        assert!(approx(a[0], 1.0, 1e-6) && a[1] == 0.0 && a[3] == 0.0);
        // a zero pivot: the rows are swapped
        let mut a = [0.0, 2.0, 4.0, 1.0, 0.0, 3.0];
        assert!(resolve_linear_system(&mut a, 2, 3, 2));
        assert_eq!(a, [1.0, 0.0, 3.0, 0.0, 1.0, 2.0]);
        // no pivot at all
        let mut a = [0.0, 1.0, 1.0, 0.0, 1.0, 2.0];
        assert!(!resolve_linear_system(&mut a, 2, 3, 2));
        // the row is scaled by a float reciprocal: 41 × float(1/41) is not 1
        let mut a = [41.0, 41.0];
        assert!(resolve_linear_system(&mut a, 1, 2, 1));
        assert_eq!(a[1].to_bits(), 0x3f7f_ffff);
        assert_eq!(dump_linear_system(&[1.0, 2.5, -3.0, 4.125], 2, 2, 1), "0)   1.00 | 2.50\n\n1)   -3.00 | 4.12\n\n\n");
    }

    #[test]
    fn rotations_follow_the_row_vector_convention() {
        let quarter = core::f32::consts::FRAC_PI_2;
        // the float nearest π/2 is a little above π/2, so its cosine is a small negative
        // number: −4.371139e-8, the well-known (float)cos((float)M_PI_2)
        let rx = matrix_x_rotation(quarter);
        assert_eq!(rx[1][1].to_bits(), 0xb33b_bd2e);
        assert_eq!((rx[1][2], rx[2][1]), (1.0, -1.0));
        assert_eq!(matrix_x_rotation(0.0), IDENTITY);
        // X turns +Y to +Z, Y turns +Z to +X, Z turns +X to +Y (vectors are rows)
        let turn = |m: &Matrix, p: Vector| vector_multiply_matrix(&p, m);
        let near = |a: Vector, b: Vector| approx(a.x, b.x, 1e-6) && approx(a.y, b.y, 1e-6) && approx(a.z, b.z, 1e-6);
        assert!(near(turn(&rx, v(0.0, 1.0, 0.0)), v(0.0, 0.0, 1.0)));
        assert!(near(turn(&matrix_y_rotation(quarter), v(0.0, 0.0, 1.0)), v(1.0, 0.0, 0.0)));
        assert!(near(turn(&matrix_z_rotation(quarter), v(1.0, 0.0, 0.0)), v(0.0, 1.0, 0.0)));
        // the three-angle rotation is X × Y × Z, and is not Z × Y × X
        let r = v(0.3, -0.7, 1.9);
        let xyz = matrix_xyz_rotation(&r);
        let (mx, my, mz) = (matrix_x_rotation(r.x), matrix_y_rotation(r.y), matrix_z_rotation(r.z));
        assert!(mat_approx(&xyz, &matrix_multiply(&matrix_multiply(&mx, &my), &mz), 1e-6));
        assert!(!mat_approx(&xyz, &matrix_multiply(&matrix_multiply(&mz, &my), &mx), 1e-2));
        // an axis rotation about a coordinate axis is that axis's rotation
        assert!(mat_approx(&matrix_axis_rotation(0.6, &v(1.0, 0.0, 0.0)), &matrix_x_rotation(0.6), 1e-7));
        assert!(mat_approx(&matrix_axis_rotation(0.6, &v(0.0, 1.0, 0.0)), &matrix_y_rotation(0.6), 1e-7));
        assert!(mat_approx(&matrix_axis_rotation(0.6, &v(0.0, 0.0, 1.0)), &matrix_z_rotation(0.6), 1e-7));
        // and leaves its axis where it is
        let axis = v(2.0 / 3.0, 1.0 / 3.0, -2.0 / 3.0);
        assert!(near(turn(&matrix_axis_rotation(1.1, &axis), axis), axis));
    }

    #[test]
    fn scaling_rotation_translation_round_trip() {
        let (s, r, t) = (v(2.0, 0.5, 3.0), v(0.4, -0.9, 2.5), v(-1.0, 4.0, 0.25));
        let m = matrix_scal_rot_trans(&s, &r, &t);
        // S × R × T with row vectors: scale first, translate last
        let product = matrix_multiply(&matrix_multiply(&matrix_scaling(&s), &matrix_xyz_rotation(&r)), &matrix_translation(&t));
        assert!(mat_approx(&m, &product, 1e-6));
        assert_eq!(m[3], [-1.0, 4.0, 0.25, 1.0]);
        let (s2, r2, t2) = matrix_extract_transformations(&m, &r, true);
        assert!(approx(s2.x, 2.0, 1e-6) && approx(s2.y, 0.5, 1e-6) && approx(s2.z, 3.0, 1e-6));
        assert!(approx(r2.x, r.x, 1e-5) && approx(r2.y, r.y, 1e-5) && approx(r2.z, r.z, 1e-5));
        assert_eq!(t2, t);
        let (s3, rm, t3) = matrix_extract_transformations2(&m);
        assert_eq!((s3, t3), (s2, t2));
        assert!(mat_approx(&rm, &matrix_xyz_rotation(&r), 1e-6));
        assert_eq!(matrix_extract_scaling(&m), s2);
        // exact lengths: a 3-4-5 row
        let mut e = IDENTITY;
        e[0] = [3.0, 0.0, 4.0, 0.0];
        assert_eq!(matrix_extract_scaling(&e), v(5.0, 1.0, 1.0));
    }

    #[test]
    fn xyz_extraction_follows_the_previous_angles() {
        let two_pi = 2.0 * core::f32::consts::PI;
        let mut rng = Rng(3);
        for _ in 0..500 {
            let r = v(rand_unit(&mut rng) * 3.0, rand_unit(&mut rng) * 1.5, rand_unit(&mut rng) * 3.0);
            let m = matrix_xyz_rotation(&r);
            // starting from the true angles the same angles come back
            let back = matrix_extract_xyz_rotation(&m, &r, true);
            assert!(approx(back.x, r.x, 2e-4) && approx(back.y, r.y, 2e-4) && approx(back.z, r.z, 2e-4), "{r:?} {back:?}");
            // starting from anywhere, the angles describe the same rotation
            let start = v(rand_unit(&mut rng) * 20.0, rand_unit(&mut rng) * 20.0, rand_unit(&mut rng) * 20.0);
            for keep in [false, true] {
                let other = matrix_extract_xyz_rotation(&m, &start, keep);
                assert!(mat_approx(&matrix_xyz_rotation(&other), &m, 2e-4), "{r:?} {start:?} {other:?}");
                if keep {
                    // each angle within half a turn of where it started
                    for (a, b) in other.to_array().into_iter().zip(start.to_array()) {
                        assert!((a - b).abs() <= core::f32::consts::PI + 1e-4);
                    }
                }
            }
            // previous angles a whole number of turns away give the same angles, shifted
            let far = v(r.x + 3.0 * two_pi, r.y - 2.0 * two_pi, r.z + two_pi);
            let kept = matrix_extract_xyz_rotation(&m, &far, true);
            assert!(approx(kept.x, far.x, 1e-3) && approx(kept.y, far.y, 1e-3) && approx(kept.z, far.z, 1e-3));
            let wrapped = matrix_extract_xyz_rotation(&m, &far, false);
            assert!(wrapped.to_array().iter().all(|a| a.abs() <= core::f32::consts::PI + 1e-4));
        }
        // gimbal lock: y = ±π/2, only x ∓ z is determined; x + z (or x − z) is kept from
        // the previous angles
        for (y, prev) in [(core::f32::consts::FRAC_PI_2, v(0.5, 1.5, 0.1)), (-core::f32::consts::FRAC_PI_2, v(-0.3, -1.5, 0.4))] {
            let r = v(0.7, y, 0.2);
            let m = matrix_xyz_rotation(&r);
            let got = matrix_extract_xyz_rotation(&m, &prev, false);
            assert!(mat_approx(&matrix_xyz_rotation(&got), &m, 1e-5), "{got:?}");
            assert!(approx(got.y, y, 1e-6));
        }
        // the helpers
        assert_eq!(angle_distance(0.25, 0.25).to_f64(), 0.0);
        assert!(approx(angle_distance(0.1, two_pi - 0.1).to_f32(), 0.2, 1e-5));
        assert!(approx(angle_distance(-3.0, 3.0).to_f32(), two_pi - 6.0, 1e-5));
        for a in [-3.0f64, -2.0, -1.0, -0.2, 0.0, 0.3, 1.0, 2.0, 3.1] {
            assert!(approx(arc_sin_cos(a.sin(), a.cos()).to_f32(), a as f32, 1e-6), "{a}");
        }
        assert!(approx(arc_sin_cos(5.0, 0.0).to_f32(), core::f32::consts::FRAC_PI_2, 1e-6)); // clamped
    }

    #[test]
    fn yaw_pitch_roll_is_z_x_y() {
        let mut rng = Rng(8);
        for _ in 0..300 {
            let (pitch, yaw, roll) = (rand_unit(&mut rng) * 1.4, rand_unit(&mut rng) * 3.0, rand_unit(&mut rng) * 3.0);
            let m = matrix_multiply(&matrix_multiply(&matrix_z_rotation(roll), &matrix_x_rotation(pitch)), &matrix_y_rotation(yaw));
            let got = matrix_extract_yaw_pitch_roll(&m);
            assert!(approx(got.x, pitch, 1e-3) && approx(got.y, yaw, 1e-3) && approx(got.z, roll, 1e-3), "{pitch} {yaw} {roll} {got:?}");
        }
        // looking straight up: yaw is reported as 0 and the roll takes everything
        for roll in [0.4f32, 2.5, -2.5, -0.4] {
            for pitch in [core::f32::consts::FRAC_PI_2, -core::f32::consts::FRAC_PI_2] {
                let m = matrix_multiply(&matrix_z_rotation(roll), &matrix_x_rotation(pitch));
                let got = matrix_extract_yaw_pitch_roll(&m);
                assert!(approx(got.x, pitch, 1e-6) && got.y == 0.0 && approx(got.z, roll, 1e-6), "{roll} {pitch} {got:?}");
            }
        }
    }

    #[test]
    fn alignment_matrices() {
        let unit = |p: Vector| {
            let mut q = p;
            vector_normalize(&mut q);
            q
        };
        let orthonormal = |m: &Matrix| mat_approx(&matrix_multiply(m, &matrix_transpose(m)), &IDENTITY, 1e-5);
        let d = unit(v(1.0, -2.0, 0.5));
        let z = matrix_z_align(&d);
        assert!(orthonormal(&z) && approx(z[0][2], d.x, 0.0) && approx(z[1][2], d.y, 0.0) && approx(z[2][2], d.z, 0.0));
        let y = matrix_y_align(&d);
        assert!(orthonormal(&y) && (y[0][1], y[1][1], y[2][1]) == (d.x, d.y, d.z));
        let xm = matrix_x_align(&d);
        assert!(orthonormal(&xm) && (xm[0][0], xm[1][0], xm[2][0]) == (d.x, d.y, d.z));
        // along the axis itself: the identity, or a half turn for the negative direction
        assert_eq!(matrix_z_align(&v(0.0, 0.0, 1.0)), IDENTITY);
        let flip = matrix_z_align(&v(0.0, 0.0, -1.0));
        assert_eq!((flip[0][0], flip[1][1], flip[2][2]), (1.0, -1.0, -1.0));
        let flip = matrix_y_align(&v(0.0, -1.0, 0.0));
        assert_eq!((flip[0][0], flip[1][1], flip[2][2]), (-1.0, -1.0, 1.0));
        let flip = matrix_x_align(&v(-1.0, 0.0, 0.0));
        assert_eq!((flip[0][0], flip[1][1], flip[2][2]), (-1.0, 1.0, -1.0));
        // two axes
        let (m, ok) = matrix_align_axes(&v(2.0, 0.0, 0.0), &v(1.0, 3.0, 0.0));
        assert!(ok);
        assert_eq!(m, IDENTITY);
        let (m, ok) = matrix_align_axes(&v(0.0, 0.0, 5.0), &v(0.0, 1.0, 1.0));
        assert!(ok && orthonormal(&m));
        assert_eq!((m[0], m[1]), ([0.0, 0.0, 1.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        assert_eq!(matrix_align_axes(&v(0.0, 0.0, 0.0), &v(0.0, 1.0, 0.0)), (IDENTITY, false));
        let (m, ok) = matrix_align_axes(&v(1.0, 0.0, 0.0), &v(3.0, 0.0, 0.0)); // parallel
        assert!(ok && orthonormal(&m) && m[0] == [1.0, 0.0, 0.0, 0.0]);
        let m = matrix_align_y_axes(&v(1.0, 0.0, 0.0), &v(0.0, 4.0, 0.0)).unwrap();
        assert_eq!(m, IDENTITY);
        let m = matrix_align_y_axes(&v(1.0, 1.0, 0.0), &v(0.0, 0.0, 2.0)).unwrap();
        assert!(orthonormal(&m) && m[1] == [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(matrix_align_y_axes(&v(1.0, 0.0, 0.0), &v(0.0, 0.0, 0.0)), None);
        let m = matrix_align_y_axes(&v(0.0, 0.0, 0.0), &v(0.0, 1.0, 0.0)).unwrap(); // x replaced by y + (1, 0, 0)
        assert!(orthonormal(&m) && m[1] == [0.0, 1.0, 0.0, 0.0]);
        let m = matrix_align_y_axes(&v(0.0, 2.0, 0.0), &v(0.0, 1.0, 0.0)).unwrap(); // parallel
        assert_eq!(m, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        // a reflection: twice is the identity
        let n = unit(v(1.0, 2.0, 2.0));
        let s = matrix_symmetry(&n);
        assert!(mat_approx(&matrix_multiply(&s, &s), &IDENTITY, 1e-6));
        assert_eq!(matrix_symmetry(&v(0.0, 1.0, 0.0))[1][1], -1.0);
    }

    #[test]
    fn vectors() {
        // multiplying by the float reciprocal: 41 × float(1/41) is one unit below 1
        let mut p = v(41.0, 0.0, 0.0);
        vector_normalize(&mut p);
        assert_eq!(p.x.to_bits(), 0x3f7f_ffff);
        let mut p = v(3.0, 4.0, 0.0);
        vector_normalize(&mut p);
        assert_eq!(p, v(0.6, 0.8, 0.0));
        let mut tiny = v(5e-5, 0.0, 0.0);
        vector_normalize(&mut tiny);
        assert_eq!(tiny, v(5e-5, 0.0, 0.0)); // too short: untouched
        assert_eq!(vector_distance(&v(1.0, 2.0, 3.0), &v(4.0, 6.0, 3.0)), 5.0);
        // the squares are summed in the register: 2^24, 1, 2^24 … √(2^48 + 1 + …)
        assert_eq!(vector_distance(&v(0.0, 0.0, 0.0), &v(3e20, 4e20, 0.0)), 5e20); // no overflow at float range
        assert_eq!(vector_angle(&v(1.0, 0.0, 0.0), &v(0.0, 3.0, 0.0)), core::f32::consts::FRAC_PI_2);
        assert_eq!(vector_angle(&v(2.0, 0.0, 0.0), &v(5.0, 0.0, 0.0)), 0.0);
        assert_eq!(vector_angle(&v(2.0, 0.0, 0.0), &v(-5.0, 0.0, 0.0)), core::f32::consts::PI);
        assert!(vector_angle(&v(0.0, 0.0, 0.0), &v(1.0, 0.0, 0.0)).is_nan());
        assert_eq!(vector_cos_angle(&v(1.0, 0.0, 0.0), &v(0.0, 3.0, 0.0)), 0.0);
        assert_eq!(vector_cos_angle(&v(0.0, 0.0, 0.0), &v(0.0, 3.0, 0.0)), 1.0); // degenerate: 1
        assert_eq!(vector_cos_angle(&v(0.0, 0.0, 2.0), &v(0.0, 0.0, -3.0)), -1.0);
        assert_eq!(points_angle(&v(1.0, 0.0, 0.0), &v(0.0, 0.0, 0.0), &v(0.0, 2.0, 0.0)), core::f32::consts::FRAC_PI_2);
        // the cosine version measures the turn at b, not the angle there: straight on is 1
        assert_eq!(points_cos_angle(&v(-1.0, 0.0, 0.0), &v(0.0, 0.0, 0.0), &v(2.0, 0.0, 0.0)), 1.0);
        assert_eq!(points_angle(&v(-1.0, 0.0, 0.0), &v(0.0, 0.0, 0.0), &v(2.0, 0.0, 0.0)), core::f32::consts::PI);
        assert_eq!(points_angle_signed(&v(1.0, 0.0, 0.0), &v(0.0, 0.0, 0.0), &v(0.0, 2.0, 0.0)), -core::f32::consts::FRAC_PI_2);
        assert_eq!(points_angle_signed(&v(1.0, 0.0, 0.0), &v(0.0, 0.0, 0.0), &v(0.0, -2.0, 0.0)), core::f32::consts::FRAC_PI_2);
        // the part of b perpendicular to a
        assert_eq!(vector_orthogonize(&v(1.0, 0.0, 0.0), &v(3.0, 4.0, 0.0)), Some(v(0.0, 4.0, 0.0)));
        assert_eq!(vector_orthogonize(&v(1.0, 0.0, 0.0), &v(3.0, 0.0, 0.0)), None);
    }

    #[test]
    fn points_lines_and_triangles() {
        let (a, b) = (v(0.0, 0.0, 9.0), v(4.0, 0.0, -9.0)); // z plays no part
        // the sign convention: a point to the left of the direction a → b is on side 1
        assert_eq!(point_side(&v(1.0, 1.0, 0.0), &a, &b), 1);
        assert_eq!(point_side(&v(1.0, -1.0, 0.0), &a, &b), -1);
        assert_eq!(point_side(&v(1.0, 0.00001, 5.0), &a, &b), 0);
        assert_eq!(point_side(&v(f32::NAN, 0.0, 0.0), &a, &b), 0);
        // the differences are not rounded to float: with a.y = 2^-30 the value tested is
        // (1 − 2^-30) 2^20 − (2^20 − 2^-30) = −2^-10 + 2^-30, below −0.0001. If p.y − a.y and
        // b.y − a.y were floats (1 and 2^20) it would be 0
        let tiny = 1.0 / P30;
        assert_eq!(point_side(&v(1.0, 1.0, 0.0), &v(0.0, tiny, 0.0), &v(1048576.0, 1048576.0, 0.0)), -1);
        assert_eq!((1.0f32 - tiny) * 1048576.0 - 1.0 * (1048576.0f32 - tiny), 0.0);
        assert!(point_between(&v(0.0, 0.0, 0.0), &a, &b)); // the start counts
        assert!(point_between(&v(2.0, 0.0, 0.0), &a, &b));
        assert!(!point_between(&v(4.0, 0.0, 0.0), &a, &b)); // the end does not
        assert!(!point_between(&v(5.0, 0.0, 0.0), &a, &b));
        assert!(!point_between(&v(2.0, 1.0, 0.0), &a, &b));
        assert!(!point_between(&v(0.0, 0.0, 0.0), &a, &a)); // a degenerate segment
        let (ta, tb, tc) = (v(0.0, 0.0, 0.0), v(4.0, 0.0, 0.0), v(0.0, 4.0, 0.0));
        let s = point_side(&ta, &tb, &tc);
        assert_eq!(s, 1);
        assert_eq!(point_inside_triangle(&v(1.0, 1.0, 0.0), &ta, &tb, &tc), 2); // inside: 2s
        assert_eq!(point_inside_triangle(&v(2.0, 0.0, 0.0), &ta, &tb, &tc), 1); // on an edge: s
        assert_eq!(point_inside_triangle(&v(0.0, 0.0, 0.0), &ta, &tb, &tc), 0); // on a corner: 0
        assert_eq!(point_inside_triangle(&v(5.0, 5.0, 0.0), &ta, &tb, &tc), 0);
        assert_eq!(point_inside_triangle(&v(1.0, 1.0, 0.0), &ta, &tc, &tb), -2); // the other winding
        assert_eq!(point_inside_triangle(&v(1.0, 1.0, 0.0), &ta, &tb, &tb), 0); // degenerate
        assert!(points_colinear(&v(0.0, 0.0, 0.0), &v(1.0, 1.0, 1.0), &v(3.0, 3.0, 3.0)));
        assert!(!points_colinear(&v(0.0, 0.0, 0.0), &v(1.0, 1.0, 1.0), &v(3.0, 3.0, 3.01)));
        assert!(points_colinear_delta(&v(0.0, 0.0, 0.0), &v(1.0, 1.0, 1.0), &v(3.0, 3.0, 3.01)));
        assert!(!points_colinear_delta(&v(0.0, 0.0, 0.0), &v(1.0, 1.0, 1.0), &v(3.0, 3.0, 4.0)));
        assert_eq!(triangle_normal(&ta, &tb, &tc), (v(0.0, 0.0, 1.0), true));
        assert_eq!(triangle_normal(&ta, &tb, &tb), (v(0.0, 0.0, 0.0), false));
        // legs 3 and 4: the area is 6, the function returns 12
        assert_eq!(area_triangle_3d(v(1.0, 1.0, 1.0), v(4.0, 1.0, 1.0), v(1.0, 5.0, 1.0)), 12.0);
        assert_eq!(point_to_plane_dist(v(1.0, 1.0, -7.0), ta, tb, tc), 7.0);
        assert!(point_to_plane_dist(v(1.0, 1.0, -7.0), ta, tb, tb).is_nan());
        // a line meeting a plane
        let line = Line { point: v(1.0, 2.0, 10.0), dir: v(0.0, 0.0, -1.0) };
        let plane = Plane { point: v(0.0, 0.0, 3.0), normal: v(0.0, 0.0, 1.0) };
        assert_eq!(line_plane_intersection(&line, &plane), v(1.0, 2.0, 3.0));
        // parallel: no division, the numerator (−7) is used as the parameter
        let along = Line { point: v(1.0, 2.0, 10.0), dir: v(1.0, 0.0, 0.0) };
        assert_eq!(line_plane_intersection(&along, &plane), v(-6.0, 2.0, 10.0));
        // two lines, in the YZ plane only
        let l1 = Line { point: v(7.0, 0.0, 0.0), dir: v(0.0, 1.0, 0.0) };
        let l2 = Line { point: v(-3.0, 2.0, -5.0), dir: v(0.0, 0.0, 1.0) };
        let mut hit = v(99.0, 99.0, 99.0);
        assert!(line_line_intersection(&l1, &l2, &mut hit));
        assert_eq!(hit, v(7.0, 2.0, 0.0));
        let mut miss = v(99.0, 99.0, 99.0);
        assert!(!line_line_intersection(&l1, &l1, &mut miss));
        assert_eq!(miss, v(7.0, 99.0, 99.0)); // x is written even so
        // "in edge": anywhere not farther from each end than the edge is long
        let (ea, eb) = (v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0));
        assert!(point_in_edge(v(1.0, 0.0, 0.0), ea, eb));
        assert!(point_in_edge(v(1.0, 1.5, 0.0), ea, eb)); // well off the segment
        assert!(!point_in_edge(v(3.0, 0.0, 0.0), ea, eb));
        assert!(point_in_edge(v(2.0, 0.0, 0.0), ea, eb));
    }

    #[test]
    fn quaternions() {
        assert_eq!(matrix_to_quaternion(&crate::types::IDENTITY3), [1.0, 0.0, 0.0, 0.0]);
        let r = matrix_x_rotation(core::f32::consts::FRAC_PI_2);
        let m3: Matrix3 = core::array::from_fn(|i| core::array::from_fn(|j| r[i][j]));
        let q = matrix_to_quaternion(&m3);
        let h = core::f64::consts::FRAC_1_SQRT_2;
        assert!((q[0] - h).abs() < 1e-7 && (q[1] - h).abs() < 1e-7 && q[2] == 0.0 && q[3] == 0.0);
        let (axis, angle) = quaternion_to_axis_angle(&q);
        assert!(approx(axis.x, 1.0, 1e-6) && axis.y == 0.0 && (angle - core::f64::consts::FRAC_PI_2).abs() < 1e-6);
        assert_eq!(quaternion_to_axis_angle(&[1.0, 0.0, 0.0, 0.0]), (v(0.0, 0.0, 0.0), 0.0));
        // a half turn about Z: the trace is −1, the largest diagonal element is m22
        let rz = matrix_z_rotation(core::f32::consts::PI);
        let m3: Matrix3 = core::array::from_fn(|i| core::array::from_fn(|j| rz[i][j]));
        let q = matrix_to_quaternion(&m3);
        assert!(q[0].abs() < 1e-7 && (q[3].abs() - 1.0).abs() < 1e-7);
        // i, j, k
        let (i, j, k) = ([0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(quaternion_mul(&i, &j), k);
        assert_eq!(quaternion_mul(&j, &i), [0.0, 0.0, 0.0, -1.0]);
        assert_eq!(quaternion_mul(&i, &i), [-1.0, 0.0, 0.0, 0.0]);
        // w = a0 b0 − a1 b1 − …: (2^53 + 1) − 2^53 keeps the 1 in the register
        let big = 9007199254740992.0f64;
        assert_eq!(quaternion_mul(&[big, 1.0, 1.0, 0.0], &[1.0, -1.0, big, 0.0])[0], 1.0);
        assert_eq!((big * 1.0 - 1.0 * -1.0) - 1.0 * big, 0.0); // double arithmetic loses it
        let mut q = [0.0, 3.0, 0.0, 4.0];
        quaternion_normalize(&mut q);
        assert_eq!(q, [0.0, 0.6, 0.0, 0.8]);
        let mut q = [1e-7, 0.0, 0.0, 0.0];
        quaternion_normalize(&mut q);
        assert_eq!(q, [1.0, 0.0, 0.0, 0.0]);
        let mut q = [1.0 + 1e-13, 0.0, 0.0, 0.0]; // unit within the tolerance: only w is clamped
        quaternion_normalize(&mut q);
        assert_eq!(q, [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn splines_and_remainders() {
        let p = v(1.0, 1.0, 1.0);
        // all control points equal: the weights sum to 6 and 6 × 0.1666667 is 1.0000002, so
        // the curve misses the point by two units in the last place
        for u in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(bspline_interpolate(u, &p, &p, &p, &p).x.to_bits(), 0x3f80_0002, "{u}");
            assert_eq!(bspline_tangent_interpolate(u, &p, &p, &p, &p), v(0.0, 0.0, 0.0));
        }
        // at u = 0 the point is (p0 + 4 p1 + p2)/6 and the tangent (p2 − p0)/2
        let (p0, p1, p2, p3) = (v(0.0, 0.0, 6.0), v(6.0, 0.0, 0.0), v(12.0, 6.0, 0.0), v(100.0, 100.0, 100.0));
        let at0 = bspline_interpolate(0.0, &p0, &p1, &p2, &p3);
        assert!(approx(at0.x, 6.0, 2e-6) && approx(at0.y, 1.0, 1e-6) && approx(at0.z, 1.0, 1e-6));
        assert_eq!(bspline_tangent_interpolate(0.0, &p0, &p1, &p2, &p3), v(6.0, 3.0, -3.0));
        // the remainder has the sign of the divisor
        assert_eq!(myfmod(7.0, 3.0).to_f32(), 1.0);
        assert_eq!(myfmod(-7.0, 3.0).to_f32(), 2.0);
        assert_eq!(myfmod(7.0, -3.0).to_f32(), -2.0);
        assert_eq!(myfmod(6.0, 3.0).to_f64(), 0.0);
        // and is not rounded: 3 × (7/3 − 2) is 1 only to within the register's last bits
        assert_ne!(myfmod(7.0, 3.0).to_bits(), F80::ONE.to_bits());
        assert_eq!((start3(), end3(), add(2, 5, 99), sub(2, 5, 99)), (13, 12, 7, -3));
    }

    #[test]
    fn dump_text() {
        let mut m = IDENTITY;
        m[0][1] = -2.5;
        m[3][0] = f32::INFINITY;
        let want = "--------------------------------\nDUMP matrix: test\n| 1.000000 -2.500000 0.000000 0.000000 |\n| 0.000000 1.000000 0.000000 0.000000 |\n| 0.000000 0.000000 1.000000 0.000000 |\n| inf 0.000000 0.000000 1.000000 |\n--------------------------------\n";
        assert_eq!(matrix_dump(&m, "test"), want);
    }
}
