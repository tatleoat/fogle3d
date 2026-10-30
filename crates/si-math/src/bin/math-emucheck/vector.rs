// SPDX-License-Identifier: MIT
//! Checks for `vector.c` (`si_math::vector`). Spec: `re/tools/si-math/emu/vector.spec`;
//! scale-and-rotation matrices: `gen_math.py`.
use crate::util::*;
use si_math::vector as vc;
use si_x87::vectors::{Case, Check, Verdict};

fn v(c: &Case, name: &str) -> [f64; 3] {
    let a = c.input(name).f64s();
    [a[0], a[1], a[2]]
}

const P3: [f64; 3] = [SENTINEL64; 3];

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "VC_Vec3Normalize" => |c: &Case| {
            let (mut a, mut len) = (v(c, "v"), SENTINEL64);
            let r = vc::vc_vec3_normalize(&mut a, &mut len);
            eax(c, r).and(|| f64s(c, "v", &a)).and(|| f64s_exact(c, "len", &[len]))
        },
        "VC_Vec3Equal" => |c: &Case| eax(c, vc::vc_vec3_equal(&v(c, "a"), &v(c, if c.try_input("b").is_some() { "b" } else { "a" })) as i32),
        "VC_Vec3Xform" => |c: &Case| f64s(c, "out", &vc::vc_vec3_xform(&v(c, "v"), &matd(c.input("m")))),
        "VC_Vec3Xform#inplace" => |c: &Case| {
            let mut a = v(c, "v");
            vc::vc_vec3_xform_in_place(&mut a, &matd(c.input("m")));
            f64s(c, "v", &a)
        },
        "VC_Vec3XformSimple" => |c: &Case| f64s(c, "out", &vc::vc_vec3_xform_simple(&v(c, "v"), &matd(c.input("m")))),
        "VC_Vec3XformSimple#inplace" => |c: &Case| {
            let mut a = v(c, "v");
            vc::vc_vec3_xform_simple_in_place(&mut a, &matd(c.input("m")));
            f64s(c, "v", &a)
        },
        "VC_Mat4x4Identity" => |c: &Case| f64s(c, "m", &flatd(&vc::vc_mat4x4_identity())),
        "VC_Mat4x4Multiply" => |c: &Case| {
            let p = vc::vc_mat4x4_multiply(&matd(c.input("a")), &matd(c.input("b")));
            let dst = if c.try_output("out").is_some() { "out" } else if c.name.ends_with("-a") { "a" } else { "b" };
            f64s(c, dst, &flatd(&p))
        },
        "VC_Mat4x4InvertUpperLeft" => |c: &Case| {
            let mut out = [[SENTINEL64; 4]; 4];
            let r = vc::vc_mat4x4_invert_upper_left(&matd(c.input("m")), &mut out);
            eax(c, r).and(|| f64s_exact(c, "out", &flatd(&out)))
        },
        "VC_Mat4x4ExtractModelXform" => |c: &Case| {
            let (mut t, mut s, mut r) = (P3, P3, P3);
            let code = vc::vc_mat4x4_extract_model_xform(&matd(c.input("m")), &mut t, &mut s, &mut r);
            eax(c, code).and(|| f64s_exact(c, "t", &t)).and(|| f64s_exact(c, "s", &s)).and(|| f64s_exact(c, "r", &r))
        },
        "VC_Mat4x4CreateModelXform" => |c: &Case| {
            f64s(c, "out", &flatd(&vc::vc_mat4x4_create_model_xform(&v(c, "t"), &v(c, "s"), &v(c, "r"))))
        },
        _ => return None,
    })
}

#[allow(dead_code)]
fn unused(_: Verdict) {}
