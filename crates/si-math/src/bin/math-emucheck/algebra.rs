// SPDX-License-Identifier: MIT
//! Checks for `algebra.c` (`si_math::algebra`). Specs: `re/tools/si-math/emu/algebra.spec`,
//! structured inputs: `re/tools/si-math/emu/gen_math.py`.
use crate::util::*;
use si_math::algebra as a;
use si_math::algebra::{Line, Plane};
use si_math::types::{Hvector, Vector};
use si_x87::vectors::{Case, Check, Verdict};

fn line(c: &Case, name: &str) -> Line {
    let v = c.input(name).f32s();
    Line { point: Vector::new(v[0], v[1], v[2]), dir: Vector::new(v[3], v[4], v[5]) }
}

fn plane(c: &Case, name: &str) -> Plane {
    let v = c.input(name).f32s();
    Plane { point: Vector::new(v[0], v[1], v[2]), normal: Vector::new(v[3], v[4], v[5]) }
}

fn v(c: &Case, name: &str) -> Vector {
    vec3(c.input(name))
}

fn m(c: &Case, name: &str) -> si_math::types::Matrix {
    mat(c.input(name))
}

/// The preference word at 0x08781580 that `SI_matrixExtractXyzRotation` reads.
fn keep(c: &Case) -> bool {
    c.input("keep").u16s()[0] != 0
}

fn quat(c: &Case, name: &str) -> [f64; 4] {
    let q = c.input(name).f64s();
    [q[0], q[1], q[2], q[3]]
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        // ---- angles between vectors
        "SI_pointsAngle" => |c: &Case| st0_f32(c, a::points_angle(&v(c, "a"), &v(c, "b"), &v(c, "c"))),
        "SI_pointsCosAngle" => |c: &Case| st0_f32(c, a::points_cos_angle(&v(c, "a"), &v(c, "b"), &v(c, "c"))),
        "SI_pointsAngleSigned" => |c: &Case| st0_f32(c, a::points_angle_signed(&v(c, "a"), &v(c, "b"), &v(c, "c"))),
        "SI_vectorAngle" => |c: &Case| st0_f32(c, a::vector_angle(&v(c, "a"), &v(c, "b"))),
        "SI_vectorCosAngle" => |c: &Case| st0_f32(c, a::vector_cos_angle(&v(c, "a"), &v(c, "b"))),
        "SI_vectorNormalize" => |c: &Case| {
            let mut p = v(c, "v");
            a::vector_normalize(&mut p);
            v3(c, "v", &p)
        },
        "SI_vectorDistance" => |c: &Case| st0_f32(c, a::vector_distance(&v(c, "a"), &v(c, "b"))),
        "SI_vectorOrthogonize" => |c: &Case| match a::vector_orthogonize(&v(c, "a"), &v(c, "b")) {
            Some(o) => eax(c, 1).and(|| v3(c, "out", &o)),
            None => eax(c, 0).and(|| untouched(c, "out")),
        },

        // ---- points, lines, triangles
        "pointSide" => |c: &Case| eax(c, a::point_side(&v(c, "p"), &v(c, "a"), &v(c, "b"))),
        "SI_pointBetween" => |c: &Case| ax(c, a::point_between(&v(c, "p"), &v(c, "a"), &v(c, "b")) as i16),
        "SI_pointInsideTriangle" => |c: &Case| ax(c, a::point_inside_triangle(&v(c, "p"), &v(c, "a"), &v(c, "b"), &v(c, "c"))),
        "SI_pointsColinear" => |c: &Case| ax(c, a::points_colinear(&v(c, "a"), &v(c, "b"), &v(c, "c")) as i16),
        "SI_pointsColinearDelta" => |c: &Case| ax(c, a::points_colinear_delta(&v(c, "a"), &v(c, "b"), &v(c, "c")) as i16),
        "SI_triangleNormal" => |c: &Case| {
            let (n, ok) = a::triangle_normal(&v(c, "a"), &v(c, "b"), &v(c, "c"));
            ax(c, ok as i16).and(|| v3(c, "out", &n))
        },
        "SI_areaTriangle3D" => |c: &Case| st0_f32(c, a::area_triangle_3d(v(c, "a"), v(c, "b"), v(c, "c"))),
        "SI_pointToPlaneDist" => |c: &Case| st0_f32(c, a::point_to_plane_dist(v(c, "p"), v(c, "a"), v(c, "b"), v(c, "c"))),
        "SI_linePlaneIntersection" => |c: &Case| v3(c, "ret", &a::line_plane_intersection(&line(c, "line"), &plane(c, "plane"))),
        "SI_lineLineIntersection" => |c: &Case| {
            let mut out = Vector::new(SENTINEL, SENTINEL, SENTINEL);
            let ok = a::line_line_intersection(&line(c, "l1"), &line(c, "l2"), &mut out);
            ax(c, ok as i16).and(|| f32s_exact(c, "out", &out.to_array()))
        },
        "SI_pointInEdge" => |c: &Case| ax(c, a::point_in_edge(v(c, "p"), v(c, "a"), v(c, "b")) as i16),

        // ---- 4 x 4 matrices
        "SI_matrixEqual" => |c: &Case| ax(c, a::matrix_equal(&m(c, "a"), &m(c, c.try_input("b").map_or("a", |_| "b"))) as i16),
        "SI_matrixCopy" => |c: &Case| m4(c, "dst", &a::matrix_copy(&m(c, "src"))),
        "SI_matrixMultiply" => |c: &Case| m4(c, "out", &a::matrix_multiply(&m(c, "a"), &m(c, "b"))),
        "SI_matrixMultiply#inplace-a" => |c: &Case| m4(c, "a", &a::matrix_multiply(&m(c, "a"), &m(c, "b"))),
        "SI_matrixMultiply#inplace-b" => |c: &Case| m4(c, "b", &a::matrix_multiply(&m(c, "a"), &m(c, "b"))),
        "SI_matrixMultiplySub3" => |c: &Case| m4(c, "out", &a::matrix_multiply_sub3(&m(c, "a"), &m(c, "b"))),
        "SI_matrixMultiplySub3#inplace-a" => |c: &Case| m4(c, "a", &a::matrix_multiply_sub3(&m(c, "a"), &m(c, "b"))),
        "SI_matrixTranspose" => |c: &Case| m4(c, "out", &a::matrix_transpose(&m(c, "m"))),
        "SI_matrixTranspose#inplace" => |c: &Case| {
            let mut t = m(c, "m");
            a::matrix_transpose_in_place(&mut t);
            m4(c, "m", &t)
        },
        "SI_matrixInverse" => |c: &Case| match a::matrix_inverse(&m(c, "m")) {
            Some(inv) => eax(c, 1).and(|| m4(c, "out", &inv)),
            None => eax(c, 0).and(|| untouched(c, "out")),
        },
        "SI_matrixInverse#inplace" => |c: &Case| match a::matrix_inverse(&m(c, "m")) {
            Some(inv) => eax(c, 1).and(|| m4(c, "m", &inv)),
            None => eax(c, 0).and(|| m4(c, "m", &m(c, "m"))),
        },
        "SI_matrixInverseDouble" => |c: &Case| match a::matrix_inverse_double(&matd(c.input("m"))) {
            Some(inv) => eax(c, 1).and(|| f64s(c, "out", &flatd(&inv))),
            None => eax(c, 0).and(|| untouched(c, "out")),
        },
        "si_matrixDifference" => |c: &Case| match a::matrix_difference(&m(c, "a"), &m(c, "b")) {
            Some(d) => eax(c, 1).and(|| m4(c, "out", &d)),
            None => eax(c, 0).and(|| untouched(c, "out")),
        },
        "SI_matrixTransposeInverse" => |c: &Case| {
            let (t, ok) = a::matrix_transpose_inverse(&m(c, "m"));
            eax(c, ok as i32).and(|| m4(c, "out", &t))
        },
        "SI_matrixDeterminant3" => |c: &Case| st0(c, a::matrix_determinant3(&m(c, "m"))),
        "SI_hvectorMultiplyMatrix" => |c: &Case| {
            let n = c.input("n").i16().max(0) as usize;
            let src = c.input("v").f32s();
            let mut out = vec![SENTINEL; src.len()];
            for i in 0..n {
                let h = a::hvector_multiply_matrix(&Hvector::new(src[4 * i], src[4 * i + 1], src[4 * i + 2], src[4 * i + 3]), &m(c, "m"));
                out[4 * i..4 * i + 4].copy_from_slice(&h.to_array());
            }
            f32s_exact(c, c.try_output("out").map_or("v", |_| "out"), &out)
        },
        "SI_hvectorMultiplyMatrixDouble" => |c: &Case| {
            let n = c.input("n").i16().max(0) as usize;
            let src = c.input("v").f64s();
            let md = matd(c.input("m"));
            let mut out = Vec::new();
            for i in 0..n {
                out.extend(a::hvector_multiply_matrix_double(&[src[4 * i], src[4 * i + 1], src[4 * i + 2], src[4 * i + 3]], &md));
            }
            f64s(c, c.try_output("out").map_or("v", |_| "out"), &out)
        },
        "SI_vectorMultiplyMatrix" => |c: &Case| {
            let n = c.input("n").i16().max(0) as usize;
            let src = c.input("v").f32s();
            let mut out = Vec::new();
            for i in 0..n {
                out.extend(a::vector_multiply_matrix(&Vector::new(src[3 * i], src[3 * i + 1], src[3 * i + 2]), &m(c, "m")).to_array());
            }
            f32s(c, c.try_output("out").map_or("v", |_| "out"), &out)
        },
        "SI_bboxMultiplyMatrix" => |c: &Case| {
            let src = c.input("in").f32s();
            let corners: [[f32; 3]; 8] = core::array::from_fn(|i| [src[3 * i], src[3 * i + 1], src[3 * i + 2]]);
            let out: Vec<f32> = a::bbox_multiply_matrix(&corners, &m(c, "m")).iter().flatten().copied().collect();
            f32s(c, c.try_output("out").map_or("in", |_| "out"), &out)
        },
        "SI_resolveLinearSystem" => |c: &Case| {
            let mut sys = c.input("a").f32s();
            let ok = a::resolve_linear_system(&mut sys, c.input("rows").i16(), c.input("cols").i16(), c.input("n").i16());
            ax(c, ok as i16).and(|| f32s(c, "a", &sys))
        },

        // ---- building transformations
        "SI_matrixScaling" => |c: &Case| m4(c, "out", &a::matrix_scaling(&v(c, "s"))),
        "SI_matrixTranslation" => |c: &Case| m4(c, "out", &a::matrix_translation(&v(c, "t"))),
        "SI_matrixXRotation" => |c: &Case| m4(c, "out", &a::matrix_x_rotation(c.input("angle").f32())),
        "SI_matrixYRotation" => |c: &Case| m4(c, "out", &a::matrix_y_rotation(c.input("angle").f32())),
        "SI_matrixZRotation" => |c: &Case| m4(c, "out", &a::matrix_z_rotation(c.input("angle").f32())),
        "SI_matrixAxisRotation" => |c: &Case| m4(c, "out", &a::matrix_axis_rotation(c.input("angle").f32(), &v(c, "axis"))),
        "SI_matrixXyzRotation" => |c: &Case| m4(c, "out", &a::matrix_xyz_rotation(&v(c, "rot"))),
        "SI_matrixScalRotTrans" => |c: &Case| m4(c, "out", &a::matrix_scal_rot_trans(&v(c, "s"), &v(c, "r"), &v(c, "t"))),
        "SI_matrixSymmetry" => |c: &Case| m4(c, "out", &a::matrix_symmetry(&v(c, "n"))),
        "SI_matrixZAlign" => |c: &Case| m4(c, "out", &a::matrix_z_align(&v(c, "v"))),
        "SI_matrixYAlign" => |c: &Case| m4(c, "out", &a::matrix_y_align(&v(c, "v"))),
        "SI_matrixXAlign" => |c: &Case| m4(c, "out", &a::matrix_x_align(&v(c, "v"))),
        "SI_matrixAlignYAxes" => |c: &Case| match a::matrix_align_y_axes(&v(c, "x"), &v(c, "y")) {
            Some(o) => eax(c, 1).and(|| m4(c, "out", &o)),
            None => eax(c, 0).and(|| untouched(c, "out")),
        },
        "SI_matrixAlignAxes" => |c: &Case| {
            let (o, ok) = a::matrix_align_axes(&v(c, "x"), &v(c, "y"));
            eax(c, ok as i32).and(|| m4(c, "out", &o))
        },

        // ---- taking transformations apart
        "SI_matrixExtractScaling" => |c: &Case| v3(c, "s", &a::matrix_extract_scaling(&m(c, "m"))),
        "SI_matrixExtractTransformations2" => |c: &Case| {
            let (s, r, t) = a::matrix_extract_transformations2(&m(c, "m"));
            v3(c, "s", &s).and(|| m4(c, "rotm", &r)).and(|| v3(c, "t", &t))
        },
        "SI_matrixExtractTransformations" => |c: &Case| {
            let (s, r, t) = a::matrix_extract_transformations(&m(c, "m"), &v(c, "r"), keep(c));
            v3(c, "s", &s).and(|| v3(c, "r", &r)).and(|| v3(c, "t", &t))
        },
        "SI_matrixExtractXyzRotation" => |c: &Case| v3(c, "rot", &a::matrix_extract_xyz_rotation(&m(c, "m"), &v(c, "rot"), keep(c))),
        "SI_matrixExtractYawPitchRoll" => |c: &Case| v3(c, "out", &a::matrix_extract_yaw_pitch_roll(&m(c, "m"))),
        "arcSinCos" => |c: &Case| st0(c, a::arc_sin_cos(c.input("s").f64(), c.input("c").f64())),
        "angleDistance" => |c: &Case| st0(c, a::angle_distance(c.input("a").f32(), c.input("b").f32())),

        // ---- quaternions, splines, the rest
        "SI_quaternionNormalize" => |c: &Case| {
            let mut q = quat(c, "q");
            a::quaternion_normalize(&mut q);
            f64s(c, "q", &q)
        },
        "SI_quaternionMul" => |c: &Case| f64s(c, c.try_output("out").map_or("a", |_| "out"), &a::quaternion_mul(&quat(c, "a"), &quat(c, "b"))),
        "SI_matrixToQuaternion" => |c: &Case| f64s(c, "q", &a::matrix_to_quaternion(&mat3(c.input("m")))),
        "SI_quaternionToAxisAngle" => |c: &Case| {
            let (axis, angle) = a::quaternion_to_axis_angle(&quat(c, "q"));
            v3(c, "axis", &axis).and(|| f64s(c, "angle", &[angle]))
        },
        "SI_bsplineInterpolate" => |c: &Case| {
            v3(c, "out", &a::bspline_interpolate(c.input("u").f32(), &v(c, "p0"), &v(c, "p1"), &v(c, "p2"), &v(c, "p3")))
        },
        "SI_bsplineTangentInterpolate" => |c: &Case| {
            v3(c, "out", &a::bspline_tangent_interpolate(c.input("u").f32(), &v(c, "p0"), &v(c, "p1"), &v(c, "p2"), &v(c, "p3")))
        },
        "SI_myfmod" => |c: &Case| st0(c, a::myfmod(c.input("a").f32(), c.input("b").f32())),
        "SI_matrixDump" => |c: &Case| {
            let title = String::from_utf8(c.input("title").u8s()).unwrap();
            text(c, "f", &a::matrix_dump(&m(c, "m"), &title))
        },
        "SI_dumpLinearSystem" => |c: &Case| {
            let s = a::dump_linear_system(&c.input("a").f32s(), c.input("rows").i32(), c.input("cols").i32(), c.input("bar").i32());
            text(c, "stdout", &s)
        },
        "SI_start3" => |c: &Case| eax(c, a::start3()),
        "SI_end3" => |c: &Case| eax(c, a::end3()),
        "SI_add" => |c: &Case| eax(c, a::add(c.input("a").i32(), c.input("b").i32(), c.input("c").i32())),
        "SI_sub" => |c: &Case| eax(c, a::sub(c.input("a").i32(), c.input("b").i32(), c.input("c").i32())),
        _ => return None,
    })
}

#[allow(dead_code)]
fn unused(_: Verdict) {}
