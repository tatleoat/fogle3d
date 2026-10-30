// SPDX-License-Identifier: MIT
//! Checks for `gl_matrix.c` (`si_math::gl_matrix`). Spec: `re/tools/si-math/emu/gl_matrix.spec`.
use crate::util::*;
use si_math::gl_matrix as g;
use si_x87::vectors::{Case, Check, Verdict};

fn f(c: &Case, name: &str) -> f32 {
    c.input(name).f32()
}

fn vp(c: &Case) -> [f32; 4] {
    let v = c.input("vp").f32s();
    [v[0], v[1], v[2], v[3]]
}

fn one(c: &Case, name: &str, got: f32) -> Verdict {
    f32s(c, name, &[got])
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "matrixGlTranslate" => |c: &Case| m4(c, "m", &g::matrix_gl_translate(f(c, "x"), f(c, "y"), f(c, "z"))),
        "matrixGlScale" => |c: &Case| m4(c, "m", &g::matrix_gl_scale(f(c, "x"), f(c, "y"), f(c, "z"))),
        "matrixGlRot" => |c: &Case| m4(c, "m", &g::matrix_gl_rot(f(c, "angle"), c.input("axis").u8s()[0])),
        "matrixGlRotate" => |c: &Case| m4(c, "m", &g::matrix_gl_rotate(f(c, "angle"), c.input("axis").u8s()[0])),
        "SI_matrixGlOrtho2" => |c: &Case| m4(c, "m", &g::matrix_gl_ortho2(f(c, "l"), f(c, "r"), f(c, "b"), f(c, "t"))),
        "SI_matrixGlOrtho" => |c: &Case| m4(c, "m", &g::matrix_gl_ortho(f(c, "l"), f(c, "r"), f(c, "b"), f(c, "t"), f(c, "n"), f(c, "f"))),
        "SI_matrixGlPerspective" => |c: &Case| m4(c, "m", &g::matrix_gl_perspective(f(c, "fovy"), f(c, "aspect"), f(c, "n"), f(c, "f"))),
        "matrixGlWindow" => |c: &Case| m4(c, "m", &g::matrix_gl_window(f(c, "l"), f(c, "r"), f(c, "b"), f(c, "t"), f(c, "n"), f(c, "f"))),
        "stereoWindow" => |c: &Case| match g::stereo_window(f(c, "fovy"), f(c, "aspect"), f(c, "near"), f(c, "conv"), f(c, "eye")) {
            Some(w) => one(c, "l", w.left).and(|| one(c, "r", w.right)).and(|| one(c, "b", w.bottom)).and(|| one(c, "t", w.top)),
            None => untouched(c, "l").and(|| untouched(c, "r")).and(|| untouched(c, "b")).and(|| untouched(c, "t")),
        },
        // with a convergence below 1e-6 the original builds the matrix from four floats of its
        // stack frame that nothing has written: where that changes the result the case carries
        // UNINIT and is skipped; where it does not, the port must have the result
        "SI_matrixStereoPerspective" => |c: &Case| {
            match g::matrix_stereo_perspective(f(c, "fovy"), f(c, "aspect"), f(c, "near"), f(c, "far"), f(c, "conv"), f(c, "eye")) {
                Some(m) => m4(c, "m", &m),
                // the port refuses (the window's four floats are uninitialised and the depth
                // test passes). Where the two garbage windows of the vector happened to give
                // the same matrix the case is not marked UNINIT; it is undefined all the same
                None => Verdict::Skip,
            }
        },
        "SI_viewportToNormalized" => |c: &Case| {
            v3(c, "out", &g::viewport_to_normalized(f(c, "px"), f(c, "py"), f(c, "pz"), &vp(c), f(c, "near"), f(c, "far")))
        },
        "SI_viewportToNormalized2D" => |c: &Case| {
            let (x, y) = g::viewport_to_normalized_2d(f(c, "px"), f(c, "py"), &vp(c));
            one(c, "ox", x).and(|| one(c, "oy", y))
        },
        "SI_normalizedToViewport" => |c: &Case| {
            let (x, y, z) = g::normalized_to_viewport(&vec3(c.input("v")), &vp(c), f(c, "near"), f(c, "far"));
            one(c, "ox", x).and(|| one(c, "oy", y)).and(|| one(c, "oz", z))
        },
        "SI_normalizedToViewport2D" => |c: &Case| {
            let (x, y) = g::normalized_to_viewport_2d(f(c, "nx"), f(c, "ny"), &vp(c));
            one(c, "ox", x).and(|| one(c, "oy", y))
        },
        "SI_matrixGlPolarview" => |c: &Case| m4(c, "m", &g::matrix_gl_polarview(f(c, "dist"), f(c, "azim"), f(c, "inc"), f(c, "twist"))),
        "SI_matrixGlLookat" => |c: &Case| {
            m4(c, "m", &g::matrix_gl_lookat(f(c, "vx"), f(c, "vy"), f(c, "vz"), f(c, "px"), f(c, "py"), f(c, "pz"), f(c, "twist")))
        },
        _ => return None,
    })
}
