// SPDX-License-Identifier: MIT
//! Checks for `matrix3tools.c` (`si_math::matrix3tools`). Spec: `re/tools/si-math/emu/matrix3tools.spec`.
use crate::util::*;
use si_math::matrix3tools as t;
use si_math::types::Matrix3;
use si_x87::vectors::{Case, Check, Verdict};

fn m(c: &Case, name: &str) -> Matrix3 {
    mat3(c.input(name))
}

fn m3(c: &Case, name: &str, got: &Matrix3) -> Verdict {
    f32s(c, name, &flat3(got))
}

/// The output buffer's name: `out`, or the input it is aliased to.
fn dst<'a>(c: &Case, alias: &'a str) -> &'a str {
    if c.try_output("out").is_some() {
        "out"
    } else {
        alias
    }
}

const PRIMED: Matrix3 = [[SENTINEL; 3]; 3];

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "matrix3Det" => |c: &Case| st0(c, t::matrix3_det(&m(c, "m"))),
        "SI_matrix3Scalar" => |c: &Case| m3(c, dst(c, "a"), &t::matrix3_scalar(c.input("s").f32(), &m(c, "a"))),
        "SI_matrix3Add" => |c: &Case| m3(c, dst(c, "a"), &t::matrix3_add(&m(c, "a"), &m(c, "b"))),
        "SI_matrix3Sub" => |c: &Case| m3(c, dst(c, "b"), &t::matrix3_sub(&m(c, "a"), &m(c, "b"))),
        "SI_matrix3Tilde" => |c: &Case| m3(c, "out", &t::matrix3_tilde(vec3(c.input("v")))),
        "SI_matrix3Copy" => |c: &Case| m3(c, "out", &t::matrix3_copy(&m(c, "a"))),
        "SI_matrix3Put" => |c: &Case| m4(c, "out", &t::matrix3_put(&m(c, "a"))),
        "SI_matrix3Get" => |c: &Case| m3(c, "out", &t::matrix3_get(&mat(c.input("m")))),
        "SI_matrix3Multiply" => |c: &Case| m3(c, "out", &t::matrix3_multiply(&m(c, "a"), &m(c, "b"))),
        "SI_matrix3Multiply#inplace-a" => |c: &Case| m3(c, "a", &t::matrix3_multiply(&m(c, "a"), &m(c, "b"))),
        "SI_matrix3Multiply#inplace-b" => |c: &Case| m3(c, "b", &t::matrix3_multiply(&m(c, "a"), &m(c, "b"))),
        "SI_matrix3Transpose" => |c: &Case| m3(c, "out", &t::matrix3_transpose(&m(c, "a"))),
        "SI_matrix3Transpose#inplace" => |c: &Case| {
            let mut a = m(c, "a");
            t::matrix3_transpose_in_place(&mut a);
            m3(c, "a", &a)
        },
        "SI_matrix3XRotation" => |c: &Case| m3(c, "out", &t::matrix3_x_rotation(c.input("angle").f32())),
        "SI_matrix3YRotation" => |c: &Case| m3(c, "out", &t::matrix3_y_rotation(c.input("angle").f32())),
        "SI_matrix3ZRotation" => |c: &Case| m3(c, "out", &t::matrix3_z_rotation(c.input("angle").f32())),
        "SI_matrix3XyzRotation" => |c: &Case| m3(c, "out", &t::matrix3_xyz_rotation(&vec3(c.input("r")))),
        // a name other than 0 makes the original read a wild pointer: those cases stop in the
        // harness (the port answers `None`) and are counted as skipped
        "SI_matrix3Dump" => |c: &Case| match t::matrix3_dump(&m(c, "m"), c.input("name").u8s()[0] as i8) {
            Some(s) => text(c, "stdout", &s),
            None => Verdict::Different("the port says the original faults, the emulated original did not".into()),
        },
        "SI_matrix4Dump" => |c: &Case| match t::matrix4_dump(&mat(c.input("m")), c.input("name").u8s()[0] as i8) {
            Some(s) => text(c, "stdout", &s),
            None => Verdict::Different("the port says the original faults, the emulated original did not".into()),
        },
        "SI_matrix3Inverse" => |c: &Case| {
            let mut out = PRIMED;
            match t::matrix3_inverse(&m(c, "a"), &mut out) {
                Ok(()) => f32s_exact(c, "out", &flat3(&out)).and(|| text(c, "stderr", "")),
                Err(msg) => f32s_exact(c, "out", &flat3(&out)).and(|| text(c, "stderr", msg)),
            }
        },
        // with the output being the input the original overwrites the input with the identity
        // before it copies it: the result is the identity whenever the determinant passes
        "SI_matrix3Inverse#inplace" => |c: &Case| {
            let mut out = m(c, "a");
            match t::matrix3_inverse(&m(c, "a"), &mut out) {
                Ok(()) => m3(c, "a", &t::matrix3_copy(&si_math::types::IDENTITY3)).and(|| text(c, "stderr", "")),
                Err(msg) => m3(c, "a", &m(c, "a")).and(|| text(c, "stderr", msg)),
            }
        },
        "SI_matrix3FastInverse" => |c: &Case| {
            let mut out = PRIMED;
            match t::matrix3_fast_inverse(&m(c, "a"), &mut out) {
                Ok(()) => f32s_exact(c, dst(c, "a"), &flat3(&out)).and(|| text(c, "stderr", "")),
                Err(msg) => f32s_exact(c, dst(c, "a"), &flat3(&out)).and(|| text(c, "stderr", msg)),
            }
        },
        "SI_matrix3Clean" => |c: &Case| {
            let mut a = m(c, "m");
            t::matrix3_clean(&mut a, c.input("eps").f32());
            m3(c, "m", &a)
        },
        "SI_matrix3RotClean" => |c: &Case| {
            let mut a = m(c, "m");
            t::matrix3_rot_clean(&mut a, c.input("eps").f32(), c.input("one").f32());
            m3(c, "m", &a)
        },
        "SI_matrix3Orthonormal" => |c: &Case| {
            let mut out = PRIMED;
            t::matrix3_orthonormal(&m(c, "a"), &mut out);
            f32s_exact(c, "out", &flat3(&out))
        },
        "SI_matrix3Orthonormal#inplace" => |c: &Case| {
            let mut out = m(c, "a");
            t::matrix3_orthonormal(&m(c, "a"), &mut out);
            m3(c, "a", &out)
        },
        "SI_matrix3RotPack" => |c: &Case| {
            let (a, b) = t::matrix3_rot_pack(&m(c, "m"));
            v3(c, "a", &a).and(|| v3(c, "b", &b))
        },
        "SI_matrix3RotUnpack" => |c: &Case| m3(c, "out", &t::matrix3_rot_unpack(vec3(c.input("a")), vec3(c.input("b")))),
        _ => return None,
    })
}
