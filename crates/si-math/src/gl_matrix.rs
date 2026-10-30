// SPDX-License-Identifier: MIT
//! IRIS GL style matrix builders on 4 × 4 float matrices: the original's `gl_matrix.c`
//! (0x0851a100–0x0851af38, 17 functions).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Each port repeats the original's x87 instruction sequence on
//! [`F80`] register values and rounds to `f32`/`f64` exactly where the original stores.
//!
//! What the file is: the matrices that IRIS GL's `translate`, `scale`, `rot`, `rotate`,
//! `ortho2`, `ortho`, `perspective`, `window`, `polarview` and `lookat` would multiply onto
//! the current matrix, built as plain values so the program can transform points itself.
//!
//! Conventions, all read from the code:
//!
//! - Vectors are rows (`v' = v × M`), translation is row 3: IRIS GL's own layout.
//! - **The elementary builders do not multiply.** [`matrix_gl_translate`], [`matrix_gl_scale`],
//!   [`matrix_gl_rot`] and [`matrix_gl_rotate`] overwrite their output with the elementary
//!   matrix; nothing of what the output held before survives. Composition is done by the
//!   callers with `SI_matrixMultiply(a, b, out)`:
//!   [`matrix_gl_polarview`] and [`matrix_gl_lookat`] compute `m ← m × L` for each new
//!   elementary matrix `L` (the new one acts last on a row vector), and
//!   [`matrix_stereo_perspective`] computes `m ← T × m` (the translation acts first).
//!   IRIS GL's `translate`/`scale`/`rot` replace the current matrix `C` by `L × C`, so the
//!   most recently issued call acts first on a point. The composites here therefore equal
//!   what IRIS GL accumulates for the corresponding call sequence on an identity matrix.
//! - Angles: [`matrix_gl_rot`] takes degrees (IRIS GL `rot`); [`matrix_gl_rotate`], the three
//!   angles of [`matrix_gl_polarview`], the twist of [`matrix_gl_lookat`] and the field of
//!   view of [`matrix_gl_perspective`] and [`stereo_window`] are in tenths of degrees (IRIS
//!   GL's `Angle`), passed as floats.
//! - Degenerate arguments (an extent below the **double** 1e-6, or a NaN) make the projection
//!   builders return the identity instead of failing.
//!
//! Exactness classes as in the owner's `algebra.rs`: copy, single, model, libm.

use crate::algebra::matrix_multiply;
use crate::trig;
use crate::types::{Matrix, Vector, IDENTITY};
use crate::x87::{fmov, x, F80};

/// The tolerance of every test in this file: the **double** 1e-6 (bits 0x3eb0c6f7a0b5ed8d;
/// the compiler emitted one copy per function, at 0x08662470, 0x08662488, 0x086624b0,
/// 0x086624c0, 0x086624d0 and 0x08662528).
const EPS_1E6: f64 = 1e-6;
/// π/180 as the double at 0x086623d0 (bits 0x3f91df46a2529d39).
const DEG_TO_RAD: f64 = 0.017453292519943295;
/// π as the double at 0x086624a8, 0x086624e8 and 0x08662520 (bits 0x400921fb54442d18).
const PI_D: f64 = core::f64::consts::PI;

// ------------------------------------------------------------------ elementary matrices

/// The translation matrix: identity with row 3 = (tx, ty, tz, 1)
/// [soft 0x0851a100 matrixGlTranslate].
///
/// The original takes the output matrix as its fourth argument and overwrites all sixteen
/// elements; it does not multiply with what was there. This is the matrix IRIS GL's
/// `translate(x, y, z)` premultiplies onto the current matrix. Class: copy.
pub fn matrix_gl_translate(tx: f32, ty: f32, tz: f32) -> Matrix {
    let mut m = IDENTITY;
    m[3][0] = fmov(tx);
    m[3][1] = fmov(ty);
    m[3][2] = fmov(tz);
    m
}

/// The scaling matrix: diagonal (sx, sy, sz, 1), everything else 0
/// [soft 0x0851a178 matrixGlScale].
///
/// Overwrites its output like [`matrix_gl_translate`]. The other builders of this file call
/// it with (1, 1, 1) to produce an identity for degenerate arguments. Class: copy.
pub fn matrix_gl_scale(sx: f32, sy: f32, sz: f32) -> Matrix {
    let mut m = [[0.0f32; 4]; 4];
    m[0][0] = fmov(sx);
    m[1][1] = fmov(sy);
    m[2][2] = fmov(sz);
    m[3][3] = 1.0;
    m
}

/// The rotation by `angle` **degrees** about the axis named by the character `axis`
/// (`'x'`, `'y'`, `'z'`, either case) [soft 0x0851a1f0 matrixGlRot]. IRIS GL's `rot`.
///
/// The angle is multiplied by the double π/180 (0x086623d0) and rounded to float; `cosf` and
/// `sinf` of that float give c and s, whose 80-bit results are rounded to float when stored
/// into the matrix (−s is negated in the register first, the same bits). With row vectors:
///
/// ```text
/// x: 1  0  0  0     y: c  0 -s  0     z:  c  s  0  0
///    0  c  s  0        0  1  0  0        -s  c  0  0
///    0 -s  c  0        s  0  c  0         0  0  1  0
///    0  0  0  1        0  0  0  1         0  0  0  1
/// ```
///
/// Any other character gives the identity (through [`matrix_gl_scale`]`(1, 1, 1)`); the two
/// library calls are made all the same. Overwrites its output, no multiplication. An angle of
/// 0 leaves −0.0 in the "−s" element. Class: libm.
pub fn matrix_gl_rot(angle: f32, axis: u8) -> Matrix {
    let a = (x(angle) * DEG_TO_RAD).to_f32();
    let c = trig::cosf(a);
    let s = trig::sinf(a);
    let (c, s, minus_s) = (c.to_f32(), s.to_f32(), (-s).to_f32());
    let mut m = IDENTITY;
    // the original subtracts 'X' from the character and jumps through a table of 0x23
    // entries (0x086623d8): 0, 1, 2 and 0x20, 0x21, 0x22 are the three axes
    match axis {
        b'X' | b'x' => {
            m[1][1] = c;
            m[1][2] = s;
            m[2][2] = c;
            m[2][1] = minus_s;
        }
        b'Y' | b'y' => {
            m[0][0] = c;
            m[2][0] = s;
            m[2][2] = c;
            m[0][2] = minus_s;
        }
        b'Z' | b'z' => {
            m[0][0] = c;
            m[0][1] = s;
            m[1][1] = c;
            m[1][0] = minus_s;
        }
        _ => return matrix_gl_scale(1.0, 1.0, 1.0),
    }
    m
}

/// The rotation by `angle` **tenths of degrees** about `axis`
/// [soft 0x0851a3b4 matrixGlRotate]. IRIS GL's `rotate`, with a float instead of an integer.
///
/// `angle / 10.0` (the double at 0x08662468) is rounded to float and handed to
/// [`matrix_gl_rot`]. Class: libm.
pub fn matrix_gl_rotate(angle: f32, axis: u8) -> Matrix {
    matrix_gl_rot((x(angle) / 10.0f64).to_f32(), axis)
}

// ------------------------------------------------------------------ projections

/// The 2D orthographic projection of the rectangle left..right × bottom..top onto −1..1
/// [soft 0x0851a3e8 SI_matrixGlOrtho2]. IRIS GL's `ortho2`.
///
/// ```text
/// 2/w   0    0  0        w = right − left, h = top − bottom
///  0   2/h   0  0
///  0    0   -1  0
/// -(right+left)/w  -(top+bottom)/h  0  1
/// ```
///
/// `w` and `h` stay in the register, **unrounded**, for the tests and for the four divisions
/// (unlike [`matrix_gl_ortho`], which rounds its extents to float first). If `w` or `h` is
/// below 1e-6 or a NaN the result is the identity. Class: model.
pub fn matrix_gl_ortho2(left: f32, right: f32, bottom: f32, top: f32) -> Matrix {
    let eps = x(EPS_1E6);
    let w = x(right) - left;
    let h = x(top) - bottom;
    if !w.ge(eps) || !h.ge(eps) {
        return matrix_gl_scale(1.0, 1.0, 1.0);
    }
    let mut m = [[0.0f32; 4]; 4];
    m[1][1] = (x(2.0f64) / h).to_f32();
    m[2][2] = -1.0;
    m[0][0] = (x(2.0f64) / w).to_f32();
    m[3][0] = (-(x(right) + left) / w).to_f32();
    m[3][3] = 1.0;
    m[3][1] = (-(x(top) + bottom) / h).to_f32();
    m
}

/// The orthographic projection of the box left..right × bottom..top × near..far
/// [soft 0x0851a4d8 SI_matrixGlOrtho]. IRIS GL's `ortho`.
///
/// ```text
/// 2/w   0    0   0       w = right − left, h = top − bottom, d = far − near,
///  0   2/h   0   0       each rounded to float before use
///  0    0  -2/d  0
/// -(right+left)/w  -(top+bottom)/h  -(far+near)/d  1
/// ```
///
/// If `w`, `h` or `d` (as floats) is below the double 1e-6 or a NaN the result is the
/// identity. Class: model (the sums are divided in the register).
pub fn matrix_gl_ortho(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Matrix {
    let eps = x(EPS_1E6);
    let w = (x(right) - left).to_f32();
    let h = (x(top) - bottom).to_f32();
    let d = (x(far) - near).to_f32();
    if !x(w).ge(eps) || !x(h).ge(eps) || !x(d).ge(eps) {
        return matrix_gl_scale(1.0, 1.0, 1.0);
    }
    let mut m = [[0.0f32; 4]; 4];
    m[1][1] = (x(2.0f64) / h).to_f32();
    m[0][0] = (x(2.0f64) / w).to_f32();
    m[2][2] = (x(-2.0f64) / d).to_f32();
    m[3][0] = (-(x(right) + left) / w).to_f32();
    m[3][1] = (-(x(top) + bottom) / h).to_f32();
    m[3][3] = 1.0;
    m[3][2] = (-(x(far) + near) / d).to_f32();
    m
}

/// The perspective projection with vertical field of view `fovy` in **tenths of degrees**,
/// width/height ratio `aspect` and clipping distances `near` and `far`
/// [soft 0x0851a5f4 SI_matrixGlPerspective]. IRIS GL's `perspective`.
///
/// ```text
/// cot/aspect  0     0                     0      cot = 1 / tan(fovy/2)
///  0         cot    0                     0
///  0          0   -(far+near)/(far−near) -1
///  0          0   -(far+far)·near/(far−near)  0
/// ```
///
/// The half angle is `(fovy / 3600.0) × π` (doubles at 0x086624a0 and 0x086624a8) rounded to
/// float; `tanf` of it stays in the register for the test and the reciprocal, and `cot` is
/// rounded to float before it is stored and divided by `aspect`. `far − near` stays in the
/// register. The identity is returned when `aspect`, the tangent or `far − near` is below
/// 1e-6 or a NaN (so also for a field of view of 0, and of 180 to 360 degrees, where the
/// tangent of the half angle is negative). Class: libm.
pub fn matrix_gl_perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> Matrix {
    let eps = x(EPS_1E6);
    let half = ((x(fovy) / 3600.0f64) * PI_D).to_f32();
    let tan = trig::tanf(half);
    let d = x(far) - near;
    if !x(aspect).ge(eps) || !tan.ge(eps) || !d.ge(eps) {
        return matrix_gl_scale(1.0, 1.0, 1.0);
    }
    let cot = (F80::ONE / tan).to_f32();
    let mut m = [[0.0f32; 4]; 4];
    m[1][1] = cot;
    m[0][0] = (x(cot) / aspect).to_f32();
    m[2][2] = (-(x(far) + near) / d).to_f32();
    m[2][3] = -1.0;
    m[3][2] = (-((x(far) + far) * near) / d).to_f32();
    m
}

/// The perspective projection of the frustum whose near rectangle is left..right ×
/// bottom..top at distance `near` [soft 0x0851a71c matrixGlWindow]. IRIS GL's `window`.
///
/// ```text
/// 2·near/w     0           0                      0     w = right − left
///  0         2·near/h      0                      0     h = top − bottom
/// (right+left)/w  (top+bottom)/h  -(far+near)/d  -1     d = far − near
///  0           0     -(far+far)·near/d            0
/// ```
///
/// `w` and `h` stay in the register. `d` is used in two roundings: element [2][2] divides by
/// `d` rounded to **float**, element [3][2] by `d` rounded to **double** (the compiler
/// spilled the same value twice), and the test compares the double. The identity is returned
/// when `w`, `h` or `d` is below 1e-6 or a NaN. Class: model.
pub fn matrix_gl_window(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Matrix {
    let eps = x(EPS_1E6);
    let d = x(far) - near;
    let d32 = d.to_f32();
    let w = x(right) - left;
    let h = x(top) - bottom;
    if !w.ge(eps) || !h.ge(eps) {
        return matrix_gl_scale(1.0, 1.0, 1.0);
    }
    let d64 = d.to_f64();
    if !x(d64).ge(eps) {
        return matrix_gl_scale(1.0, 1.0, 1.0);
    }
    let near2 = x(near) + near;
    let mut m = [[0.0f32; 4]; 4];
    m[1][1] = (near2 / h).to_f32();
    m[0][0] = (near2 / w).to_f32();
    m[2][0] = ((x(right) + left) / w).to_f32();
    m[2][1] = ((x(top) + bottom) / h).to_f32();
    m[2][2] = (-(x(far) + near) / d32).to_f32();
    m[2][3] = -1.0;
    m[3][2] = (-((x(far) + far) * near) / d64).to_f32();
    m
}

// ------------------------------------------------------------------ stereo

/// The near rectangle of one eye's frustum, the four values [`stereo_window`] writes through
/// its output pointers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StereoWindow {
    pub left: f32,
    pub right: f32,
    pub bottom: f32,
    pub top: f32,
}

/// The off-axis frustum of one eye of a stereo pair [soft 0x0851a844 stereoWindow].
///
/// Arguments: vertical field of view `fovy` in **tenths of degrees**, `aspect`, the near
/// distance, the convergence distance `conv` (where the two frusta coincide) and the eye's
/// sideways offset `eye`. With doubles from 0x086624d8–0x086624f0:
///
/// ```text
/// top    =  tan(((0.5 × fovy) / 10 × π) / 180) × near        bottom = −top
/// t      =  tan((((fovy × aspect) × 0.5) / 10 × π) / 180)
/// left   = −t × near − (eye / conv) × near
/// right  =  t × near − (eye / conv) × near
/// ```
///
/// Each `tan` (the double function) is rounded to float before use; the angle is rounded to
/// double when pushed. Note that `aspect` multiplies the **angle**, not the tangent: the
/// horizontal half angle is taken as aspect × the vertical one, which is the code of SGI's
/// stereo example and not the frustum `perspective` builds.
///
/// `None` is the original's silent return when `conv` is below the double 1e-6: it then
/// writes nothing at all through its four pointers (a NaN `conv` is not caught and
/// computes). Class: libm.
pub fn stereo_window(fovy: f32, aspect: f32, near: f32, conv: f32, eye: f32) -> Option<StereoWindow> {
    if x(conv).lt(x(EPS_1E6)) {
        return None;
    }
    let angle = ((((x(0.5f64) * fovy) / 10.0f64) * PI_D) / 180.0f64).to_f64();
    let tan = trig::tan(angle).to_f32();
    let top = x(tan) * near;
    let (top, bottom) = (top.to_f32(), (-top).to_f32());
    let angle = (((((x(fovy) * aspect) * 0.5f64) / 10.0f64) * PI_D) / 180.0f64).to_f64();
    let tan = trig::tan(angle).to_f32();
    let shift = (x(eye) / conv) * near;
    let left = ((-x(tan)) * near - shift).to_f32();
    let right = (x(tan) * near - shift).to_f32();
    Some(StereoWindow { left, right, bottom, top })
}

/// One eye's stereo projection as a matrix: the [`matrix_gl_window`] of the frustum from
/// [`stereo_window`], premultiplied by a translation of `−eye` along X
/// [soft 0x0851a9a4 SI_matrixStereoPerspective].
///
/// Arguments `(fovy, aspect, near, far, conv, eye)`. The product is `T(−eye, 0, 0) × W`
/// (`SI_matrixMultiply(T, m, m)`), so a point is shifted first and projected second: what
/// IRIS GL holds after `window(…); translate(−eye, 0, 0)`.
///
/// When `conv` is below 1e-6, [`stereo_window`] writes nothing and the original goes on to
/// build the window matrix from four **uninitialised** stack floats. [`matrix_gl_window`]
/// answers with the identity whatever those four hold exactly when its depth test fails
/// (`far − near`, rounded to double, below 1e-6 or a NaN): then the result is the translation
/// alone, and it is returned. Otherwise the result depends on what the stack held and cannot
/// be reproduced: `None`. (Seen in the emulation check, finding 0011: the original's result
/// is the same for two different stack contents in the first case and differs in the second.)
/// Class: libm.
pub fn matrix_stereo_perspective(fovy: f32, aspect: f32, near: f32, far: f32, conv: f32, eye: f32) -> Option<Matrix> {
    let m = match stereo_window(fovy, aspect, near, conv, eye) {
        Some(win) => matrix_gl_window(win.left, win.right, win.bottom, win.top, near, far),
        None => {
            let d64 = (x(far) - near).to_f64();
            if x(d64).ge(x(EPS_1E6)) {
                return None;
            }
            matrix_gl_scale(1.0, 1.0, 1.0)
        }
    };
    let t = matrix_gl_translate((-x(eye)).to_f32(), 0.0, 0.0);
    Some(matrix_multiply(&t, &m))
}

// ------------------------------------------------------------------ viewport mapping

/// Maps a point given in viewport coordinates to the cube −1..1
/// [soft 0x0851aa50 SI_viewportToNormalized].
///
/// `viewport` is (left, right, bottom, top); `near`, `far` are the depth range. Each
/// coordinate is `(v − (lo + hi) × 0.5) / ((hi − lo) × 0.5)` with the double 0.5
/// (0x086624f8), evaluated in the register and rounded once. A viewport of zero extent
/// divides by zero (infinity or NaN); nothing is checked.
///
/// Aliasing: the original stores x before it reads viewport[2..4], so the output must not
/// overlap the second half of the viewport array; otherwise a by-value result is the same.
/// Class: model.
pub fn viewport_to_normalized(px: f32, py: f32, pz: f32, viewport: &[f32; 4], near: f32, far: f32) -> Vector {
    let half = x(0.5f64);
    let vp = viewport;
    Vector {
        x: ((x(px) - (x(vp[0]) + vp[1]) * half) / ((x(vp[1]) - vp[0]) * half)).to_f32(),
        y: ((x(py) - half * (x(vp[2]) + vp[3])) / ((x(vp[3]) - vp[2]) * half)).to_f32(),
        z: ((x(pz) - (x(near) + far) * half) / (half * (x(far) - near))).to_f32(),
    }
}

/// The same for x and y only [soft 0x0851aac8 SI_viewportToNormalized2D]. Returns the two
/// floats the original writes through its last two pointers (x is stored before
/// viewport[2..4] is read, so they must not overlap that half of the array). Class: model.
pub fn viewport_to_normalized_2d(px: f32, py: f32, viewport: &[f32; 4]) -> (f32, f32) {
    let half = x(0.5f64);
    let vp = viewport;
    (
        ((x(px) - (x(vp[0]) + vp[1]) * half) / ((x(vp[1]) - vp[0]) * half)).to_f32(),
        ((x(py) - (x(vp[2]) + vp[3]) * half) / ((x(vp[3]) - vp[2]) * half)).to_f32(),
    )
}

/// Maps a point of the cube −1..1 to viewport coordinates and depth: the inverse of
/// [`viewport_to_normalized`] [soft 0x0851ab20 SI_normalizedToViewport].
///
/// Each coordinate is `(hi + lo) × 0.5 + ((hi − lo) × v) × 0.5` with the double 0.5
/// (0x08662508): the extent is multiplied by the coordinate first and halved second.
/// Returns (x, y, z), which the original writes through three float pointers (x first; they
/// must not overlap the inputs). Class: model.
pub fn normalized_to_viewport(v: &Vector, viewport: &[f32; 4], near: f32, far: f32) -> (f32, f32, f32) {
    let half = x(0.5f64);
    let vp = viewport;
    (
        ((x(vp[1]) + vp[0]) * half + ((x(vp[1]) - vp[0]) * v.x) * half).to_f32(),
        ((x(vp[3]) + vp[2]) * half + half * ((x(vp[3]) - vp[2]) * v.y)).to_f32(),
        (((x(far) - near) * v.z) * half + (x(far) + near) * half).to_f32(),
    )
}

/// The same for x and y only [soft 0x0851aba4 SI_normalizedToViewport2D]; x is stored before
/// viewport[2..4] is read. Class: model.
pub fn normalized_to_viewport_2d(nx: f32, ny: f32, viewport: &[f32; 4]) -> (f32, f32) {
    let half = x(0.5f64);
    let vp = viewport;
    (
        ((x(vp[1]) + vp[0]) * half + ((x(vp[1]) - vp[0]) * nx) * half).to_f32(),
        (((x(vp[3]) - vp[2]) * ny) * half + (x(vp[3]) + vp[2]) * half).to_f32(),
    )
}

// ------------------------------------------------------------------ viewing transformations

/// The viewing matrix of a camera on a sphere around the origin
/// [soft 0x0851ac04 SI_matrixGlPolarview]. IRIS GL's `polarview(dist, azim, inc, twist)`.
///
/// All three angles are in **tenths of degrees** (they go through [`matrix_gl_rotate`]). The
/// result is
///
/// ```text
/// Rz(−azim) × Rx(−inc) × Rz(−twist) × T(0, 0, −dist)
/// ```
///
/// built left to right with `m ← m × L` (`SI_matrixMultiply(m, L, m)`), so a row vector is
/// turned by −azim about Z first and moved away along −Z last. IRIS GL reaches the same
/// matrix by premultiplying in the opposite order (translate, rot z, rot x, rot z).
/// Class: libm.
pub fn matrix_gl_polarview(dist: f32, azim: f32, inc: f32, twist: f32) -> Matrix {
    let neg = |v: f32| (-x(v)).to_f32();
    let m = matrix_gl_rotate(neg(azim), b'z');
    let m = matrix_multiply(&m, &matrix_gl_rotate(neg(inc), b'x'));
    let m = matrix_multiply(&m, &matrix_gl_rotate(neg(twist), b'z'));
    matrix_multiply(&m, &matrix_gl_translate(0.0, 0.0, neg(dist)))
}

/// The two lengths [`matrix_gl_lookat`] works with, for the view direction (dx, dy, dz):
/// its length in the XZ plane, `sqrt(dx² + dz²)`, left in the register, and its full length
/// as a float. The full length is **not** `sqrt(dx² + dy² + dz²)` rounded once: `dy² + dx²`
/// is rounded to float first, `dz²` is added to that float in the register, and the square
/// root of the sum is rounded to float.
fn lookat_lengths(dx: F80, dy: F80, dz: F80) -> (F80, f32) {
    let dx2 = dx * dx;
    let dz2 = dz * dz;
    let dy2 = dy * dy;
    let flat = (dx2 + dz2).sqrt();
    let partial = (dy2 + dx2).to_f32();
    let full = (dz2 + partial).sqrt().to_f32();
    (flat, full)
}

/// The viewing matrix of a camera at (vx, vy, vz) looking at (px, py, pz), rolled by `twist`
/// **tenths of degrees** about the line of sight [soft 0x0851aca8 SI_matrixGlLookat].
/// IRIS GL's `lookat`.
///
/// ```text
/// T(−vx, −vy, −vz) × Ry × Rx × Rz        each step m ← m × L
/// Ry: cos = (vz − pz)/flat, sin = (px − vx)/flat     flat = sqrt(dx² + dz²)
/// Rx: cos = flat/full,      sin = (vy − py)/full     full = length of p − v
/// Rz: angle (−twist / 1800.0) × π, rounded to float; cosf, sinf
/// ```
///
/// with the rotation layouts of [`matrix_gl_rot`]. The camera position goes to the origin
/// and the target onto the −Z axis. `flat` stays in the register (also across the calls);
/// `full` is a float formed as described at [`lookat_lengths`]; the differences
/// `px − vx` and `vz − pz` are divided unrounded.
///
/// Degenerate cases: the Y rotation is left out when `flat` is below the double 1e-6 or a
/// NaN (looking straight up or down), the X rotation when `full` is below it or a NaN
/// (camera on its target). The Z rotation is always applied, and `cosf`/`sinf` are each
/// called twice with the same argument. Class: libm (model when `twist` is 0 apart from
/// cos 0 and sin 0).
pub fn matrix_gl_lookat(vx: f32, vy: f32, vz: f32, px: f32, py: f32, pz: f32, twist: f32) -> Matrix {
    let eps = x(EPS_1E6);
    let angle = ((-x(twist) / 1800.0f64) * PI_D).to_f32();
    let dx = x(px) - vx;
    let (flat, full) = lookat_lengths(dx, x(py) - vy, x(pz) - vz);
    let neg = |v: f32| (-x(v)).to_f32();
    let mut m = matrix_gl_translate(neg(vx), neg(vy), neg(vz));
    if flat.ge(eps) {
        let c = (x(vz) - pz) / flat;
        let s = dx / flat;
        let mut r = IDENTITY;
        r[0][0] = c.to_f32();
        r[2][0] = s.to_f32();
        r[2][2] = c.to_f32();
        r[0][2] = (-s).to_f32();
        m = matrix_multiply(&m, &r);
    }
    if eps.le(x(full)) {
        let s = (x(vy) - py) / full;
        let c = flat / full;
        let mut r = IDENTITY;
        r[1][2] = s.to_f32();
        r[1][1] = c.to_f32();
        r[2][2] = c.to_f32();
        r[2][1] = (-s).to_f32();
        m = matrix_multiply(&m, &r);
    }
    let mut r = IDENTITY;
    r[0][0] = trig::cosf(angle).to_f32();
    r[0][1] = trig::sinf(angle).to_f32();
    r[1][0] = (-trig::sinf(angle)).to_f32();
    r[1][1] = trig::cosf(angle).to_f32();
    matrix_multiply(&m, &r)
}

#[cfg(test)]
mod tests {
    use super::*;

    const P24: f32 = 16777216.0; // 2^24

    fn bits(m: &Matrix) -> [[u32; 4]; 4] {
        m.map(|r| r.map(f32::to_bits))
    }

    /// `v × m` for a row vector with w = 1, in plain doubles: for geometric checks only.
    fn apply(v: [f64; 3], m: &Matrix) -> [f64; 3] {
        let v = [v[0], v[1], v[2], 1.0];
        let mut o = [0.0f64; 4];
        for j in 0..4 {
            for i in 0..4 {
                o[j] += v[i] * m[i][j] as f64;
            }
        }
        [o[0] / o[3], o[1] / o[3], o[2] / o[3]]
    }

    fn near(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() <= tol)
    }

    #[test]
    fn constants_have_the_bits_of_the_binary() {
        assert_eq!(EPS_1E6.to_bits(), 0x3eb0_c6f7_a0b5_ed8d);
        assert_eq!(DEG_TO_RAD.to_bits(), 0x3f91_df46_a252_9d39);
        assert_eq!(PI_D.to_bits(), 0x4009_21fb_5444_2d18);
    }

    #[test]
    fn translate_and_scale_overwrite_with_the_elementary_matrix() {
        assert_eq!(matrix_gl_translate(1.0, 2.0, 3.0), [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 2.0, 3.0, 1.0]]);
        assert_eq!(matrix_gl_scale(2.0, 3.0, 4.0), [[2.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 4.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        assert_eq!(matrix_gl_scale(1.0, 1.0, 1.0), IDENTITY);
        // a float passes through the unit: a signalling NaN arrives quiet
        let snan = f32::from_bits(0x7f80_0001);
        assert_eq!(matrix_gl_translate(snan, 0.0, 0.0)[3][0].to_bits(), 0x7fc0_0001);
    }

    #[test]
    fn private_multiply_accumulates_in_the_register() {
        // row (2^12, 1, −2^12, 0) times column (2^12, 1, 2^12, 0): (2^24 + 1) − 2^24 is 1 in
        // the register and 0 if each step were rounded to float
        let mut a = [[0.0f32; 4]; 4];
        let mut b = [[0.0f32; 4]; 4];
        a[0] = [4096.0, 1.0, -4096.0, 0.0];
        (b[0][0], b[1][0], b[2][0]) = (4096.0, 1.0, 4096.0);
        assert_eq!(matrix_multiply(&a, &b)[0][0], 1.0);
        // (2^64 + 1) does not fit 64 bits: left to right gives 0, any other order 1
        a[0] = [4294967296.0, 1.0, -4294967296.0, 0.0];
        (b[0][0], b[1][0], b[2][0]) = (4294967296.0, 1.0, 4294967296.0);
        assert_eq!(matrix_multiply(&a, &b)[0][0], 0.0);
    }

    #[test]
    fn rot_layouts_axes_and_units() {
        // angle 0: c = 1, s = +0 and the negated sine is −0
        let m = matrix_gl_rot(0.0, b'x');
        assert_eq!(m, IDENTITY);
        assert_eq!(m[1][2].to_bits(), 0x0000_0000);
        assert_eq!(m[2][1].to_bits(), 0x8000_0000);
        assert_eq!(matrix_gl_rot(0.0, b'y')[0][2].to_bits(), 0x8000_0000);
        assert_eq!(matrix_gl_rot(0.0, b'z')[1][0].to_bits(), 0x8000_0000);
        // 90 degrees: the angle is the float nearest π/2, which lies above π/2, so the cosine
        // is the small negative number every correct cosf gives there
        let c = -4.371139e-8f32;
        assert_eq!(c.to_bits(), 0xb33b_bd2e);
        assert_eq!(matrix_gl_rot(90.0, b'x'), [[1.0, 0.0, 0.0, 0.0], [0.0, c, 1.0, 0.0], [0.0, -1.0, c, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        assert_eq!(matrix_gl_rot(90.0, b'y'), [[c, 0.0, -1.0, 0.0], [0.0, 1.0, 0.0, 0.0], [1.0, 0.0, c, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        assert_eq!(matrix_gl_rot(90.0, b'z'), [[c, 1.0, 0.0, 0.0], [-1.0, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        // capitals are the same axes, anything else is the identity
        for (lower, upper) in [(b'x', b'X'), (b'y', b'Y'), (b'z', b'Z')] {
            assert_eq!(bits(&matrix_gl_rot(33.0, lower)), bits(&matrix_gl_rot(33.0, upper)));
        }
        for other in [b'w', b'W', b'[', b'{', 0u8, 0x80, 0xff, b'0'] {
            assert_eq!(bits(&matrix_gl_rot(33.0, other)), bits(&IDENTITY));
        }
        // rotate takes tenths of degrees
        assert_eq!(bits(&matrix_gl_rotate(900.0, b'z')), bits(&matrix_gl_rot(90.0, b'z')));
        assert_eq!(bits(&matrix_gl_rotate(-300.0, b'x')), bits(&matrix_gl_rot(-30.0, b'x')));
        // a right-handed turn for row vectors: X goes to Y under a Z rotation
        assert!(near(apply([1.0, 0.0, 0.0], &matrix_gl_rot(90.0, b'z')), [0.0, 1.0, 0.0], 1e-7));
        assert!(near(apply([0.0, 1.0, 0.0], &matrix_gl_rot(90.0, b'x')), [0.0, 0.0, 1.0], 1e-7));
        assert!(near(apply([0.0, 0.0, 1.0], &matrix_gl_rot(90.0, b'y')), [1.0, 0.0, 0.0], 1e-7));
    }

    #[test]
    fn ortho2_and_ortho_values_and_degenerate_cases() {
        assert_eq!(matrix_gl_ortho2(0.0, 2.0, 0.0, 4.0), [[1.0, 0.0, 0.0, 0.0], [0.0, 0.5, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [-1.0, -1.0, 0.0, 1.0]]);
        assert_eq!(matrix_gl_ortho(0.0, 2.0, 0.0, 4.0, 1.0, 9.0), [[1.0, 0.0, 0.0, 0.0], [0.0, 0.5, 0.0, 0.0], [0.0, 0.0, -0.25, 0.0], [-1.0, -1.0, -1.25, 1.0]]);
        // an extent below 1e-6, a negative one or a NaN: identity
        assert_eq!(matrix_gl_ortho2(0.0, 5e-7, 0.0, 1.0), IDENTITY);
        assert_eq!(matrix_gl_ortho2(0.0, 1.0, 1.0, 0.0), IDENTITY);
        assert_eq!(matrix_gl_ortho2(f32::NAN, 1.0, 0.0, 1.0), IDENTITY);
        assert_eq!(matrix_gl_ortho(0.0, 1.0, 0.0, 1.0, 2.0, 2.0), IDENTITY);
        assert_eq!(matrix_gl_ortho(0.0, 1.0, 0.0, f32::NAN, 0.0, 1.0), IDENTITY);
        assert_eq!(matrix_gl_ortho(3.0, 1.0, 0.0, 1.0, 0.0, 1.0), IDENTITY);
        // the float nearest 1e-6 is below the double 1e-6: rejected; the next float is not
        let below = 1e-6f32;
        assert!((below as f64) < 1e-6);
        assert_eq!(matrix_gl_ortho2(0.0, below, 0.0, 1.0), IDENTITY);
        assert_ne!(matrix_gl_ortho2(0.0, f32::from_bits(below.to_bits() + 1), 0.0, 1.0), IDENTITY);
    }

    #[test]
    fn ortho2_divides_by_the_unrounded_extent_and_ortho_by_the_float() {
        // right − left = 2^24 + 1 needs 25 bits: exact in the register, 2^24 as a float.
        // ortho2: 2/(2^24+1) = 2^-23 (1 − 2^-24 + …) rounds to the float below 2^-23, and
        // −(2^24 − 1)/(2^24 + 1) = −(1 − 2^-23 + 2^-47 − …) rounds to −(1 − 2^-23).
        let m = matrix_gl_ortho2(-1.0, P24, 0.0, 1.0);
        assert_eq!(m[0][0].to_bits(), 0x33ff_ffff);
        assert_eq!(m[3][0].to_bits(), 0xbf7f_fffe);
        // ortho: the extent is the float 2^24, so 2/2^24 = 2^-23 and −(2^24 − 1)/2^24
        let m = matrix_gl_ortho(-1.0, P24, 0.0, 1.0, 0.0, 1.0);
        assert_eq!(m[0][0].to_bits(), 0x3400_0000);
        assert_eq!(m[3][0].to_bits(), 0xbf7f_ffff);
    }

    #[test]
    fn perspective_values_and_degenerate_cases() {
        // 90 degrees = 900 tenths: the half angle is the float nearest π/4 (above π/4), its
        // tangent 1 + 4.4e-8, the cotangent 1 − 4.4e-8, which rounds to the float below 1
        let m = matrix_gl_perspective(900.0, 1.0, 1.0, 3.0);
        let cot = f32::from_bits(0x3f7f_ffff);
        assert_eq!(m, [[cot, 0.0, 0.0, 0.0], [0.0, cot, 0.0, 0.0], [0.0, 0.0, -2.0, -1.0], [0.0, 0.0, -3.0, 0.0]]);
        // aspect divides the float cotangent
        assert_eq!(matrix_gl_perspective(900.0, 2.0, 1.0, 3.0)[0][0].to_bits(), 0x3eff_ffff);
        assert_eq!(matrix_gl_perspective(900.0, 0.0, 1.0, 3.0), IDENTITY);
        assert_eq!(matrix_gl_perspective(900.0, 1.0, 3.0, 3.0), IDENTITY);
        assert_eq!(matrix_gl_perspective(0.0, 1.0, 1.0, 3.0), IDENTITY); // tan 0 < 1e-6
        assert_eq!(matrix_gl_perspective(2700.0, 1.0, 1.0, 3.0), IDENTITY); // tan 135° < 0
        assert_eq!(matrix_gl_perspective(f32::NAN, 1.0, 1.0, 3.0), IDENTITY);
        assert_eq!(matrix_gl_perspective(900.0, 1.0, f32::NAN, 3.0), IDENTITY);
    }

    #[test]
    fn window_values_and_the_two_roundings_of_the_depth() {
        let m = matrix_gl_window(-1.0, 1.0, -1.0, 1.0, 1.0, 3.0);
        assert_eq!(m, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -2.0, -1.0], [0.0, 0.0, -3.0, 0.0]]);
        // off-axis: (right+left)/w and (top+bottom)/h in row 2
        let m = matrix_gl_window(0.0, 2.0, 1.0, 3.0, 1.0, 3.0);
        assert_eq!(m[2], [1.0, 2.0, -2.0, -1.0]);
        assert_eq!(matrix_gl_window(0.0, 0.0, -1.0, 1.0, 1.0, 3.0), IDENTITY);
        assert_eq!(matrix_gl_window(-1.0, 1.0, 2.0, 1.0, 1.0, 3.0), IDENTITY);
        assert_eq!(matrix_gl_window(-1.0, 1.0, -1.0, 1.0, 3.0, 3.0), IDENTITY);
        assert_eq!(matrix_gl_window(-1.0, 1.0, -1.0, 1.0, f32::NAN, 3.0), IDENTITY);
        // far − near = 2^24 + 1: [2][2] divides by the float 2^24, giving −(2^24 − 1)/2^24;
        // [3][2] divides by the double 2^24 + 1: 2^25/(2^24 + 1) = 2 − 2^-23 + 2^-47 − …
        let m = matrix_gl_window(-1.0, 1.0, -1.0, 1.0, -1.0, P24);
        assert_eq!(m[2][2].to_bits(), 0xbf7f_ffff);
        assert_eq!(m[3][2].to_bits(), 0x3fff_ffff);
    }

    #[test]
    fn stereo_window_and_stereo_perspective() {
        // 90 degrees: ((0.5 × 900)/10 × π)/180 is exactly the double π/4, whose tangent
        // rounds to the float 1
        let w = stereo_window(900.0, 1.0, 2.0, 4.0, 0.5).unwrap();
        assert_eq!(w, StereoWindow { left: -2.25, right: 1.75, bottom: -2.0, top: 2.0 });
        // the aspect multiplies the angle: 0.5 gives tan 22.5°, not half of tan 45°
        let w = stereo_window(900.0, 0.5, 1.0, 1.0, 0.0).unwrap();
        assert!((w.right as f64 - 0.41421356237).abs() < 1e-7 && w.left == -w.right && w.top == 1.0);
        // below the tolerance nothing is computed; a NaN is not caught
        assert_eq!(stereo_window(900.0, 1.0, 2.0, 5e-7, 0.5), None);
        assert_eq!(stereo_window(900.0, 1.0, 2.0, -1.0, 0.5), None);
        assert!(stereo_window(900.0, 1.0, 2.0, f32::NAN, 0.5).unwrap().left.is_nan());
        assert_eq!(matrix_stereo_perspective(900.0, 1.0, 2.0, 6.0, 0.0, 0.5), None);

        // T(−eye) × window: the eye position projects like the origin of the plain window
        let m = matrix_stereo_perspective(900.0, 1.0, 2.0, 6.0, 4.0, 0.5).unwrap();
        let win = matrix_gl_window(-2.25, 1.75, -2.0, 2.0, 2.0, 6.0);
        assert_eq!(bits(&m), bits(&matrix_multiply(&matrix_gl_translate(-0.5, 0.0, 0.0), &win)));
        assert_eq!(m[3], [-0.5, 0.0, -6.0, 0.0]); // row 3 = (−eye, 0, 0, 1) × window
        assert_eq!(m[0], win[0]);
        // a point on the axis at the convergence distance lands in the middle of the screen
        assert!(near(apply([0.0, 0.0, -4.0], &m), [0.0, 0.0, 0.5], 1e-6));
    }

    #[test]
    fn viewport_mappings() {
        let vp = [0.0f32, 640.0, 0.0, 480.0];
        assert_eq!(viewport_to_normalized(320.0, 240.0, 0.5, &vp, 0.0, 1.0), Vector::new(0.0, 0.0, 0.0));
        assert_eq!(viewport_to_normalized(640.0, 0.0, 1.0, &vp, 0.0, 1.0), Vector::new(1.0, -1.0, 1.0));
        assert_eq!(viewport_to_normalized(160.0, 360.0, 0.25, &vp, 0.0, 1.0), Vector::new(-0.5, 0.5, -0.5));
        assert_eq!(viewport_to_normalized_2d(160.0, 360.0, &vp), (-0.5, 0.5));
        assert_eq!(normalized_to_viewport(&Vector::new(-0.5, 0.5, -0.5), &vp, 0.0, 1.0), (160.0, 360.0, 0.25));
        assert_eq!(normalized_to_viewport(&Vector::new(1.0, -1.0, 1.0), &vp, 2.0, 10.0), (640.0, 0.0, 10.0));
        assert_eq!(normalized_to_viewport_2d(-0.5, 0.5, &vp), (160.0, 360.0));
        // zero extent: a division by zero, unchecked
        let flat = [5.0f32, 5.0, 0.0, 1.0];
        assert!(viewport_to_normalized_2d(6.0, 0.0, &flat).0.is_infinite());
        assert!(viewport_to_normalized_2d(5.0, 0.0, &flat).0.is_nan());
    }

    #[test]
    fn viewport_mappings_keep_the_register_precision() {
        // lo + hi = 2^24 + 1 is exact in the register: the centre is 8388608.5, the point
        // 8388608 lies half a unit off it and the result is 0.5/8388607.5 = 1/(2^24 − 1) =
        // 2^-24 (1 + 2^-24 + 2^-48 + …), just above a rounding tie: 2^-24 (1 + 2^-23).
        // Float arithmetic step by step would give a centre of 8388608 and a result of 0.
        let vp = [P24, 1.0, P24, 1.0];
        let want = 0x3380_0001;
        assert_eq!(viewport_to_normalized_2d(8388608.0, 8388608.0, &vp), (f32::from_bits(want), f32::from_bits(want)));
        let v = viewport_to_normalized(8388608.0, 8388608.0, 8388608.0, &vp, P24, 1.0);
        assert_eq!([v.x.to_bits(), v.y.to_bits(), v.z.to_bits()], [want; 3]);
        // back: (hi + lo) × 0.5 + ((hi − lo) × v) × 0.5 with v = 2^-24: 8388608.5 +
        // (−(2^24 − 1) × 2^-24) × 0.5 = 8388608.5 − 0.5 + 2^-25 rounds to 8388608; with a
        // float centre of 8388608 the sum would be 8388607.5
        assert_eq!(normalized_to_viewport_2d(f32::from_bits(0x3380_0000), f32::from_bits(0x3380_0000), &vp), (8388608.0, 8388608.0));
        let v = Vector::new(f32::from_bits(0x3380_0000), f32::from_bits(0x3380_0000), f32::from_bits(0x3380_0000));
        assert_eq!(normalized_to_viewport(&v, &vp, P24, 1.0), (8388608.0, 8388608.0, 8388608.0));
    }

    #[test]
    fn polarview_order_and_units() {
        // no angles: just the camera pulled back along +Z, i.e. the scene moved to −Z
        assert_eq!(matrix_gl_polarview(5.0, 0.0, 0.0, 0.0), matrix_gl_translate(0.0, 0.0, -5.0));
        // the origin always ends up at distance dist straight ahead
        let m = matrix_gl_polarview(7.0, 300.0, 450.0, 1234.0);
        assert_eq!(m[3], [0.0, 0.0, -7.0, 1.0]);
        // Rz(−azim) × Rx(−inc) × Rz(−twist) × T, built left to right
        let want = matrix_multiply(
            &matrix_multiply(&matrix_multiply(&matrix_gl_rot(-30.0, b'z'), &matrix_gl_rot(-45.0, b'x')), &matrix_gl_rot(-123.4, b'z')),
            &matrix_gl_translate(0.0, 0.0, -7.0),
        );
        assert_eq!(bits(&m), bits(&want));
        // azimuth 90° (900 tenths), incidence 90°: the camera sits on the +X axis at distance
        // 7 and looks at the origin, so the point (7, 0, 0) is the eye
        let m = matrix_gl_polarview(7.0, 900.0, 900.0, 0.0);
        assert!(near(apply([7.0, 0.0, 0.0], &m), [0.0, 0.0, 0.0], 1e-5));
        // and the order of the two outer rotations matters: this is not Rx × Rz
        let wrong = matrix_multiply(&matrix_multiply(&matrix_gl_rot(-90.0, b'x'), &matrix_gl_rot(-90.0, b'z')), &matrix_gl_translate(0.0, 0.0, -7.0));
        assert!(!near(apply([7.0, 0.0, 0.0], &wrong), [0.0, 0.0, 0.0], 1e-3));
    }

    #[test]
    fn lookat_geometry_and_degenerate_cases() {
        // along −Z from (0, 0, 5): only the translation remains
        assert_eq!(matrix_gl_lookat(0.0, 0.0, 5.0, 0.0, 0.0, 0.0, 0.0), matrix_gl_translate(0.0, 0.0, -5.0));
        // from +X: the eye goes to the origin, the target to (0, 0, −5), up stays up
        let m = matrix_gl_lookat(5.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(m, [[0.0, 0.0, 1.0, 0.0], [0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, -5.0, 1.0]]);
        // a general view: eye to the origin, target straight ahead at its distance
        let m = matrix_gl_lookat(1.0, 2.0, 3.0, -2.0, 6.0, 15.0, 0.0);
        assert!(near(apply([1.0, 2.0, 3.0], &m), [0.0, 0.0, 0.0], 1e-5));
        assert!(near(apply([-2.0, 6.0, 15.0], &m), [0.0, 0.0, -13.0], 1e-5));
        // straight down: flat = 0, the Y rotation is skipped, the X rotation turns −Y to −Z
        let m = matrix_gl_lookat(0.0, 5.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(m, [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, -5.0, 1.0]]);
        // eye on the target: both skipped, only the translation and the twist
        let m = matrix_gl_lookat(1.0, 2.0, 3.0, 1.0, 2.0, 3.0, 900.0);
        assert_eq!(bits(&m), bits(&matrix_multiply(&matrix_gl_translate(-1.0, -2.0, -3.0), &matrix_gl_rot(-90.0, b'z'))));
        // the twist is in tenths of degrees and negated: the same float angle as rot(−90)
        // (compared as numbers: the skipped-nothing path turns the −0 of row 3 into +0)
        let m = matrix_gl_lookat(0.0, 0.0, 5.0, 0.0, 0.0, 0.0, 900.0);
        assert_eq!(m, matrix_multiply(&matrix_gl_translate(0.0, 0.0, -5.0), &matrix_gl_rot(-90.0, b'z')));
        // a NaN makes both lengths NaN: both rotations skipped, the NaN stays in row 3
        let m = matrix_gl_lookat(f32::NAN, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert!(m[3][0].is_nan() && m[0][0] == 1.0 && m[1][1] == 1.0 && m[2][2] == 1.0);
    }

    #[test]
    fn lookat_full_length_goes_through_a_float() {
        // dx = 1, dy = 2^-12, dz = 2^-12 (1 + 2^-10). dy² + dx² = 1 + 2^-24 is a tie and
        // rounds to the float 1; adding dz² = 2^-24 + 2^-33 + 2^-44 gives a square root of
        // 1 + 2^-25 + …, the float 1. Rounded once, the sum 1 + 2^-23 + 2^-33 + … would
        // have the root 1 + 2^-24 + 2^-34 − …, just above a tie: the next float after 1.
        let dz = f32::from_bits(0x3980_2000);
        assert_eq!(dz, 2.0f32.powi(-12) * (1.0 + 2.0f32.powi(-10)));
        let (flat, full) = lookat_lengths(x(1.0f32), x(2.0f32.powi(-12)), x(dz));
        assert_eq!(full.to_bits(), 0x3f80_0000);
        // the flat length is the unrounded root of 1 + dz²
        assert_eq!(flat.to_bits(), (x(1.0f32) + x(dz) * dz).sqrt().to_bits());
        assert!(flat.gt(F80::ONE));
        // (3, 4, 12): 5 and 13
        let (flat, full) = lookat_lengths(x(3.0f32), x(12.0f32), x(4.0f32));
        assert_eq!((flat.to_f64(), full), (5.0, 13.0));
    }
}
