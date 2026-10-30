// SPDX-License-Identifier: MIT
//! `extrusion.c` (seventh slice): the helpers of the extrusions along a polyline path.
//! Finding 0034, "geomupd8".

use si_math::algebra::{matrix_axis_rotation, matrix_copy, matrix_multiply, matrix_scaling, matrix_transpose};
use si_math::types::{Matrix, Vector, IDENTITY};
use si_mem::Mem;
use si_x87::{fmov, trig, x, F80};

/// The double nearest 0.0001 (0x0864a5a0).
const EPS: f64 = 0.0001;

/// The matrix that widens a section where a polyline path bends
/// [soft 0x0815bf5c SI_createMatrixZScalingForExtrusion]: `axis` is the bend's rotation axis,
/// `point` a point of the path, `angle` the half angle of the bend and `max` the largest
/// widening allowed.
///
/// 1. The widening is `1 / sin(angle)` rounded to float, at most `max`; 1 when the sine (left in
///    the register) is not above 0.0001 (a NaN included). The sine is taken a second time for the
///    division.
/// 2. The scaling (1, 1, widening). When the squared length of `axis` is not above 0.0001 the
///    answer is the identity instead (the scaling is dropped).
/// 3. Otherwise the point is turned by the rotation of `angle` about `axis`
///    (`SI_matrixAxisRotation`): `q = point × R` with the translation row, each coordinate summed
///    in the register as `((p.x·R[0][j] + p.y·R[1][j]) + p.z·R[2][j]) + R[3][j]`. The frame `F`
///    is the identity with row 0 = `q × axis` (from the unrounded `q`, each component rounded to
///    float once), row 1 = `axis`, row 2 = `q` rounded to float. The answer is
///    `Fᵀ × S × F`: the scaling applied along `q`'s direction in that frame.
pub fn create_matrix_z_scaling_for_extrusion(axis: Vector, point: Vector, angle: f32, max: f32) -> Matrix {
    let z = if trig::sinf(angle).gt(F80::from_f64(EPS)) {
        let r = (F80::ONE / trig::sinf(angle)).to_f32();
        if x(r).gt(x(max)) { fmov(max) } else { r }
    } else {
        1.0
    };
    let mut m1 = matrix_scaling(&Vector::new(1.0, 1.0, z));
    let len2 = (x(axis.x) * axis.x + x(axis.y) * axis.y) + x(axis.z) * axis.z;
    if !len2.gt(F80::from_f64(EPS)) {
        return matrix_copy(&IDENTITY);
    }
    let r = matrix_axis_rotation(angle, &axis);
    let (px, py, pz) = (x(point.x), x(point.y), x(point.z));
    let q: [F80; 3] = [0, 1, 2].map(|j| ((px * r[0][j] + py * r[1][j]) + pz * r[2][j]) + r[3][j]);
    let (qx, qy, qz) = (q[0], q[1], q[2]);
    let e = (qy * axis.z - qz * axis.y).to_f32();
    let f = (qz * axis.x - qx * axis.z).to_f32();
    let g = (qx * axis.y - qy * axis.x).to_f32();
    let mut frame = matrix_copy(&IDENTITY);
    frame[0][0] = e;
    frame[0][1] = f;
    frame[0][2] = g;
    frame[1][0] = fmov(axis.x);
    frame[1][1] = fmov(axis.y);
    frame[1][2] = fmov(axis.z);
    frame[2][0] = qx.to_f32();
    frame[2][1] = qy.to_f32();
    frame[2][2] = qz.to_f32();
    let t = matrix_transpose(&frame);
    m1 = matrix_multiply(&t, &m1);
    matrix_multiply(&m1, &frame)
}

/// [`create_matrix_z_scaling_for_extrusion`] on the image: the matrix is written at `out`
/// (sixteen floats), the arguments as the original takes them.
pub fn create_matrix_z_scaling_for_extrusion_at(m: &mut Mem, out: u32, axis: Vector, point: Vector, angle: f32, max: f32) {
    let r = create_matrix_z_scaling_for_extrusion(axis, point, angle, max);
    for (i, row) in r.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            m.set_f32(out + 16 * i as u32 + 4 * j as u32, fmov(*v));
        }
    }
}
