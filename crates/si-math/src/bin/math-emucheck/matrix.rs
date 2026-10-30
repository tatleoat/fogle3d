// SPDX-License-Identifier: MIT
//! Checks for `matrix.c` (`si_math::matrix`). Spec: `re/tools/si-math/emu/matrix.spec`. The
//! camera matrices are globals of the original; the spec pokes them and the checks build a
//! `RenderCamera` from the same values.
use crate::util::*;
use si_math::matrix as r;
use si_math::matrix::RenderCamera;
use si_math::types::{Pspoint, Vector};
use si_x87::vectors::{Case, Check, Verdict};

/// What the routine printed (on either stream) against the port's message.
fn printed(c: &Case, msg: Option<&str>) -> Verdict {
    let got: Vec<u8> = ["stdout", "stderr"].iter().filter_map(|n| c.try_output(n)).flat_map(|f| f.u8s()).collect();
    let mut want = msg.unwrap_or("").as_bytes().to_vec();
    if mutate() {
        want.push(b'?');
    }
    Verdict::from(got == want, || format!("port prints {msg:?}, original {:?}", String::from_utf8_lossy(&got)))
}

const PV: Vector = Vector { x: SENTINEL, y: SENTINEL, z: SENTINEL };

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "si_ident" => |c: &Case| m4(c, "m", &r::ident()),
        "SI_mmul" => |c: &Case| {
            let dst = if c.try_output("out").is_some() { "out" } else if c.name.ends_with("-a") { "a" } else { "b" };
            m4(c, dst, &r::mmul(&mat(c.input("a")), &mat(c.input("b"))))
        },
        "SI_transpose" => |c: &Case| m4(c, "out", &r::transpose(&mat(c.input("a")))),
        "SI_transpose#inplace" => |c: &Case| {
            let mut a = mat(c.input("a"));
            r::transpose_in_place(&mut a);
            m4(c, "a", &a)
        },
        "SI_invert" => |c: &Case| match r::invert(&mat(c.input("a"))) {
            Some(inv) => eax(c, 1).and(|| m4(c, "out", &inv)),
            None => eax(c, 0).and(|| untouched(c, "out")),
        },
        "SI_invert#inplace" => |c: &Case| match r::invert(&mat(c.input("a"))) {
            Some(inv) => eax(c, 1).and(|| m4(c, "a", &inv)),
            None => eax(c, 0).and(|| m4(c, "a", &mat(c.input("a")))),
        },
        "si_cameraRotate" => |c: &Case| {
            let mut cam = RenderCamera { raw_to_world_mat: mat(c.input("g")), ..Default::default() };
            let msg = cam.camera_rotate(c.input("axis").i16(), c.input("a").f32(), c.input("b").f32());
            m4(c, "g", &cam.raw_to_world_mat).and(|| printed(c, msg))
        },
        "si_cameraTranslate" => |c: &Case| {
            let mut cam = RenderCamera { raw_to_world_mat: mat(c.input("g")), ..Default::default() };
            cam.camera_translate(c.input("x").f32(), c.input("y").f32(), c.input("z").f32());
            m4(c, "g", &cam.raw_to_world_mat)
        },
        "SI_rawToWorld" => |c: &Case| {
            let cam = RenderCamera { raw_to_world_mat: mat(c.input("g")), ..Default::default() };
            let mut out = PV;
            let msg = cam.raw_to_world(vec3(c.input("v")), &mut out);
            f32s_exact(c, "out", &out.to_array()).and(|| printed(c, msg))
        },
        "SI_rawToWorldVect" => |c: &Case| {
            let cam = RenderCamera { vect_mat: mat(c.input("g")), ..Default::default() };
            let mut out = PV;
            let msg = cam.raw_to_world_vect(vec3(c.input("v")), &mut out);
            f32s_exact(c, "out", &out.to_array()).and(|| printed(c, msg))
        },
        "SI_worldToPrsp1" => |c: &Case| {
            let cam = RenderCamera { prsp_mat: mat(c.input("p")), near_z: c.input("nearz").f32(), ..Default::default() };
            let mut out = Pspoint { x: SENTINEL, y: SENTINEL };
            let msg = cam.world_to_prsp1(vec3(c.input("v")), &mut out);
            f32s_exact(c, "out", &[out.x, out.y]).and(|| printed(c, msg))
        },
        "SI_worldToPrsp2" => |c: &Case| {
            let cam = RenderCamera { prsp_mat: mat(c.input("p")), ..Default::default() };
            let mut out = Pspoint { x: SENTINEL, y: SENTINEL };
            let msg = cam.world_to_prsp2(vec3(c.input("v")), &mut out);
            f32s_exact(c, "out", &[out.x, out.y]).and(|| printed(c, msg))
        },
        "si_worldToRaw" => |c: &Case| {
            let cam = RenderCamera { world_to_raw_mat: mat(c.input("w")), ..Default::default() };
            v3(c, "out", &cam.world_to_raw(vec3(c.input("v"))))
        },
        "si_cameraToLight" => |c: &Case| {
            let mut out = PV;
            let msg = r::camera_to_light(vec3(c.input("v")), &mat(c.input("zmat")), &mat(c.input("xymat")), &mut out);
            f32s_exact(c, "out", &out.to_array()).and(|| printed(c, msg))
        },
        _ => return None,
    })
}
