// SPDX-License-Identifier: MIT
//! The matrices of the native renderer: the original's `matrix.c`
//! (0x08278190–0x082799be, 12 functions).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Each port repeats the original's x87 instruction sequence on
//! [`F80`] register values and rounds to `f32` exactly where the original stores.
//!
//! The renderer keeps its matrices as `float *` to sixteen floats, row after row; that is the
//! layout of [`Matrix`], which is used here. Vectors are rows (`v' = v × M`), translation is
//! row 3. Three coordinate systems appear in the names: **raw** (the coordinates a model is
//! stored in), **world** (what the renderer shades in: the camera at the origin, looking
//! along −Z) and **prsp** (after the perspective division).
//!
//! Four of the functions are pure ([`ident`], [`mmul`], [`transpose`], [`invert`]), one takes
//! its matrices as arguments ([`camera_to_light`]), and seven read or write global matrices
//! of the renderer: they are methods of [`RenderCamera`], which stands for that global state.
//!
//! Six functions print a line with `printf` when they meet a divisor of zero or a bad
//! argument and then carry on in a defined way. The Rust returns the text they print
//! (`Option<&'static str>`, `None` when nothing is printed) and performs the same writes.
//!
//! Exactness classes as in the owner's `algebra.rs`: copy, single, model, libm.

use crate::trig;
use crate::types::{Matrix, Pspoint, Vector};
use crate::x87::{fmov, x, F80};

/// The pivot tolerance of [`invert`]: the **double** 0.0001 at 0x08654658
/// (bits 0x3f1a36e2eb1c432d). Values are compared with it in the register, some of them
/// unrounded, so it must not be shortened to a float.
const PIVOT_EPS: f64 = 1e-4;

/// What [`RenderCamera::camera_rotate`] prints for an axis other than 1, 2, 3 (0x0869dbc0).
pub fn msg_camera_rotate() -> &'static str {
    si_rt::text(0x0869dbc0)
}
/// What [`RenderCamera::raw_to_world`] prints for a zero divisor (0x0869dc00).
pub fn msg_raw_to_world() -> &'static str {
    si_rt::text(0x0869dc00)
}
/// What [`RenderCamera::raw_to_world_vect`] prints for a zero divisor (0x0869dc40). The
/// name in the text, "itapply", is not the function's.
pub fn msg_raw_to_world_vect() -> &'static str {
    si_rt::text(0x0869dc40)
}
/// What [`RenderCamera::world_to_prsp1`] prints for a zero divisor (0x0869dc80).
pub fn msg_world_to_prsp1() -> &'static str {
    si_rt::text(0x0869dc80)
}
/// What [`RenderCamera::world_to_prsp2`] prints for a zero divisor (0x0869dcc0).
pub fn msg_world_to_prsp2() -> &'static str {
    si_rt::text(0x0869dcc0)
}
/// What [`camera_to_light`] prints for a zero divisor (two copies of the same text, at
/// 0x0869dd00 for the first matrix and 0x0869dd40 for the second).
pub fn msg_camera_to_light() -> &'static str {
    si_rt::text(0x0869dd00)
}

// ------------------------------------------------------------------ pure matrix routines

/// The identity matrix [soft 0x08278190 si_ident]. The original zeroes the sixteen floats
/// from the last to the first and then sets the diagonal, all as integer words. Class: copy.
pub fn ident() -> Matrix {
    let mut m = [[0.0f32; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    m
}

/// The matrix product `a × b` [soft 0x082781cc SI_mmul].
///
/// Straight-line code for the sixteen elements; the compiler ordered the additions
/// differently from element to element, but every one of them is, up to the commutativity of
/// `+`, ((a[i][0] b[0][j] + a[i][1] b[1][j]) + a[i][2] b[2][j]) + a[i][3] b[3][j]: exact
/// products, the sum accumulated in the register, one rounding to float. The same bits as
/// `algebra.c`'s `SI_matrixMultiply`. The results go to a local matrix and are copied out at
/// the end, so in the original the destination may be either operand (the camera functions
/// rely on that). Class: model.
pub fn mmul(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[i][j] = (((x(a[i][0]) * b[0][j] + x(a[i][1]) * b[1][j]) + x(a[i][2]) * b[2][j]) + x(a[i][3]) * b[3][j]).to_f32();
        }
    }
    out
}

/// The transpose [soft 0x08278498 SI_transpose]. Class: copy.
///
/// The original writes the destination element by element in storage order, reading
/// `a[j][i]` for each, with no temporary: the destination must not be the source. What
/// happens if it is, is [`transpose_in_place`].
pub fn transpose(a: &Matrix) -> Matrix {
    let mut out = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[i][j] = fmov(a[j][i]);
        }
    }
    out
}

/// What [soft 0x08278498 SI_transpose] leaves when source and destination are the same
/// matrix: not the transpose. Elements above the diagonal are overwritten with their mirror
/// images first; when the elements below the diagonal are reached they read those copies
/// back. The lower triangle is copied onto the upper one and the matrix comes out symmetric.
pub fn transpose_in_place(m: &mut Matrix) {
    for i in 0..4 {
        for j in 0..4 {
            m[i][j] = fmov(m[j][i]);
        }
    }
}

/// The inverse of a 4 × 4 matrix by Gauss–Jordan elimination in **floats**
/// [soft 0x08278504 SI_invert]. `None` is the original's return value 0: no usable pivot;
/// the destination is then left untouched. `Some` is its 1.
///
/// The original is 3,619 bytes of straight-line code (the loops of the source were unrolled
/// by hand or by a macro), working on a local 4 × 8 array of floats: the matrix next to an
/// identity. For each column k = 0, 1, 2, 3:
///
/// 1. **Pivot.** If |w[k][k]| is below the double 0.0001, the rows below are tried from top
///    to bottom and the first whose entry in column k is **above** 0.0001 in magnitude is
///    swapped with row k (all eight columns). If there is none the function returns 0. For
///    k = 3 there is no row left: |pivot| < 0.0001 returns 0. No search for the largest
///    pivot is made, and a pivot of exactly the tolerance is used as it is.
/// 2. **Scale.** `r = 1 / pivot`, rounded to float. Row k is multiplied by `r`.
/// 3. **Eliminate.** Every other row i gets `(row k × r) × (−w[i][k]) + row i`, including
///    column k itself, so the entries that should become 0 become `w − (pivot × r) × w`,
///    which is 0 only when `pivot × r` is exactly 1.
///
/// What makes the result bit-specific is which values the compiler kept in the register and
/// which it reloaded from float variables. All of it is reproduced:
///
/// - In step 1 the comparison uses the float w[k][k] for k = 0 and 1, but for k = 2 and 3
///   the **unrounded** value left in the register by the previous elimination; the
///   candidate rows for k = 1 and 2 are compared unrounded as well.
/// - The scaled pivot row `row k × r` is used in step 3 **unrounded** (a 48-bit product) for
///   some columns and **rounded to float** for the others: for k = 0 and 1, unrounded in
///   columns 0–4 and rounded in 5–7; for k = 2, unrounded in columns 0, 1, 3, 6 for rows 0
///   and 1 but only in columns 0, 1 for row 3; for k = 3 (only columns 4–7 reach the
///   output) rounded everywhere except column 5 of row 0.
/// - For k = 3 the reciprocal is taken of the unrounded pivot, and row 3's own entries in
///   columns 4–6 are the floats stored by the previous step while column 7 is unrounded.
/// - Each eliminated entry is one multiplication and one addition in the register, rounded
///   to float when stored.
///
/// The left half of the array is not needed after the last step; the original still
/// computes and stores it, the Rust leaves that out. The original reads the whole source
/// before it writes the destination, so the two may be the same matrix. A NaN pivot passes
/// every test (the comparisons are false for NaN in the direction that continues) and
/// produces NaNs. Class: model.
pub fn invert(a: &Matrix) -> Option<Matrix> {
    let eps = x(PIVOT_EPS);
    let mut w = [[0.0f32; 8]; 4];
    for i in 0..4 {
        for j in 0..4 {
            w[i][j] = fmov(a[i][j]);
        }
        w[i][4 + i] = 1.0;
    }
    // the unrounded results of the latest elimination, by row and column
    let mut reg = [[F80::ZERO; 8]; 4];

    // One elimination step with pivot row k: `keep[i][c]` says whether row i uses the
    // unrounded product for column c. Returns nothing; updates w and reg.
    fn eliminate(w: &mut [[f32; 8]; 4], reg: &mut [[F80; 8]; 4], k: usize, keep: [u8; 4]) {
        let r = (F80::ONE / w[k][k]).to_f32();
        let scaled: [F80; 8] = core::array::from_fn(|c| x(w[k][c]) * r);
        let stored: [f32; 8] = scaled.map(F80::to_f32);
        for i in 0..4 {
            if i == k {
                continue;
            }
            let minus = -x(w[i][k]);
            for c in 0..8 {
                let factor = if keep[i] >> c & 1 == 1 { scaled[c] } else { x(stored[c]) };
                reg[i][c] = factor * minus + w[i][c];
            }
            for c in 0..8 {
                w[i][c] = reg[i][c].to_f32();
            }
        }
        w[k] = stored;
    }
    const COLS_0_TO_4: u8 = 0b0001_1111;
    const COLS_0_1_3_6: u8 = 0b0100_1011;
    const COLS_0_1: u8 = 0b0000_0011;

    // column 0: the entries are the source's floats
    if eps.gt(x(w[0][0]).abs()) {
        let other = (1..4).find(|&i| x(w[i][0]).abs().gt(eps))?;
        w.swap(0, other);
    }
    eliminate(&mut w, &mut reg, 0, [0, COLS_0_TO_4, COLS_0_TO_4, COLS_0_TO_4]);

    // column 1: the pivot is compared as a float, the candidates unrounded
    if x(w[1][1]).abs().lt(eps) {
        let other = (2..4).find(|&i| reg[i][1].abs().gt(eps))?;
        w.swap(1, other);
    }
    eliminate(&mut w, &mut reg, 1, [COLS_0_TO_4, 0, COLS_0_TO_4, COLS_0_TO_4]);

    // column 2: pivot and candidate both unrounded
    if reg[2][2].abs().lt(eps) {
        if !reg[3][2].abs().gt(eps) {
            return None;
        }
        w.swap(2, 3);
    }
    eliminate(&mut w, &mut reg, 2, [COLS_0_1_3_6, COLS_0_1_3_6, 0, COLS_0_1]);

    // column 3: the unrounded pivot, no row left to swap with
    let pivot = reg[3][3];
    if pivot.abs().lt(eps) {
        return None;
    }
    let r = (F80::ONE / pivot).to_f32();
    let scaled = [x(w[3][4]) * r, x(w[3][5]) * r, x(w[3][6]) * r, x(r) * reg[3][7]];
    let mut out = [[0.0f32; 4]; 4];
    out[3] = scaled.map(F80::to_f32);
    for i in 0..3 {
        let minus = -x(w[i][3]);
        for c in 0..4 {
            // the destination's row 3 is read back, except once
            let factor = if i == 0 && c == 1 { scaled[1] } else { x(out[3][c]) };
            out[i][c] = (factor * minus + w[i][4 + c]).to_f32();
        }
    }
    Some(out)
}

// ------------------------------------------------------------------ the renderer's camera state

/// The matrices of the renderer's current view, which seven functions of `matrix.c` read or
/// write as globals.
///
/// None of them has a symbol of its own: they are members of the one global structure
/// `SI_database` (symbol at 0x08790640 in `.bss`, 1,440 bytes up to the next symbol). The
/// four matrices lie one after another from 0x087907ec; the code addresses all of them
/// relative to that address, and the caller `transForm` saves the four as one block of 256
/// bytes. The field names are ours, chosen after what the code does with each.
///
/// How they are filled is not in `matrix.c`: `SI_camera` (0x08272450) builds
/// [`RenderCamera::raw_to_world_mat`] with [`RenderCamera::camera_rotate`] and
/// [`RenderCamera::camera_translate`], inverts it into [`RenderCamera::world_to_raw_mat`]
/// and sets up [`RenderCamera::prsp_mat`]; `transForm` and `si_lights` premultiply a model's
/// matrix onto `raw_to_world_mat` and store the transpose of its inverse in
/// [`RenderCamera::vect_mat`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderCamera {
    /// 0x087907ec = `SI_database` + 0x1ac: the inverse of the camera matrix, as `SI_camera`
    /// leaves it. [`RenderCamera::world_to_raw`] reads its upper-left 3 × 3 block.
    pub world_to_raw_mat: Matrix,
    /// 0x0879082c = `SI_database` + 0x1ec: the current raw-to-world matrix (camera, with the
    /// current model's matrix multiplied on in front while a model is transformed).
    /// Written by [`RenderCamera::camera_rotate`] and [`RenderCamera::camera_translate`],
    /// read by [`RenderCamera::raw_to_world`].
    pub raw_to_world_mat: Matrix,
    /// 0x0879086c = `SI_database` + 0x22c: the matrix for directions and normals (the
    /// transpose of the inverse of `raw_to_world_mat`). Read by
    /// [`RenderCamera::raw_to_world_vect`].
    pub vect_mat: Matrix,
    /// 0x087908ac = `SI_database` + 0x26c: the perspective matrix. Read by
    /// [`RenderCamera::world_to_prsp1`] and [`RenderCamera::world_to_prsp2`].
    pub prsp_mat: Matrix,
    /// 0x087907c0 = `SI_database` + 0x180: the Z of the near clipping plane in world
    /// coordinates (set elsewhere to 0 − the float at `SI_database` + 0x34, so a negative
    /// number for a plane in front of the camera). [`RenderCamera::world_to_prsp1`] does not
    /// project points whose Z is above it.
    pub near_z: f32,
}

impl Default for RenderCamera {
    /// The state of zero-initialised memory, which is what the globals are before
    /// `SI_camera` runs: all matrices zero (not identity).
    fn default() -> RenderCamera {
        RenderCamera { world_to_raw_mat: [[0.0; 4]; 4], raw_to_world_mat: [[0.0; 4]; 4], vect_mat: [[0.0; 4]; 4], prsp_mat: [[0.0; 4]; 4], near_z: 0.0 }
    }
}

/// `(v, 1) × m` for column `j`: `((v.x m[0][j] + v.y m[1][j]) + v.z m[2][j]) + m[3][j]` in
/// the register. The compiler added the first two products in either order from place to
/// place, which gives the same bits.
fn point_times_column(v: &Vector, m: &Matrix, j: usize) -> F80 {
    ((x(v.x) * m[0][j] + x(v.y) * m[1][j]) + x(v.z) * m[2][j]) + m[3][j]
}

impl RenderCamera {
    /// Premultiplies the raw-to-world matrix by a rotation about one coordinate axis
    /// [soft 0x08279328 si_cameraRotate]: `raw_to_world_mat ← R × raw_to_world_mat`, so the
    /// rotation acts first on a row vector.
    ///
    /// The meaning of the two floats depends on the axis:
    ///
    /// - `axis` 1 (X) and 2 (Y): `a` is the **sine** and `b` the **cosine** of the angle;
    ///   no library call is made. The caller passes ratios of lengths.
    /// - `axis` 3 (Z): `a` is the **angle in radians**; `cosf` and `sinf` are called, the
    ///   cosine is rounded to float at once (it is stored over the argument `b`, which is
    ///   otherwise ignored), the sine when it is written into the matrix.
    ///
    /// The rotation layouts are those of `gl_matrix.c` (X: [1][1] = c, [1][2] = s,
    /// [2][1] = −s, [2][2] = c; Y: [0][0] = c, [0][2] = −s, [2][0] = s, [2][2] = c;
    /// Z: [0][0] = c, [0][1] = s, [1][0] = −s, [1][1] = c).
    ///
    /// Any other axis prints [`MSG_CAMERA_ROTATE`] (returned here) and still multiplies, by
    /// the identity; that multiplication is carried out, so a −0 or an infinity in the
    /// matrix does not necessarily survive it. Class: libm for axis 3, model otherwise.
    pub fn camera_rotate(&mut self, axis: i16, a: f32, b: f32) -> Option<&'static str> {
        let mut r = ident();
        let mut printed = None;
        match axis {
            1 => {
                r[1][2] = fmov(a);
                r[1][1] = fmov(b);
                r[2][2] = fmov(b);
                r[2][1] = (-x(a)).to_f32();
            }
            2 => {
                r[2][0] = fmov(a);
                r[0][0] = fmov(b);
                r[2][2] = fmov(b);
                r[0][2] = (-x(a)).to_f32();
            }
            3 => {
                let c = trig::cosf(a).to_f32();
                let s = trig::sinf(a);
                r[0][0] = c;
                r[0][1] = s.to_f32();
                r[1][1] = c;
                r[1][0] = (-s).to_f32();
            }
            _ => printed = Some(msg_camera_rotate()),
        }
        self.raw_to_world_mat = mmul(&r, &self.raw_to_world_mat);
        printed
    }

    /// Premultiplies the raw-to-world matrix by a translation
    /// [soft 0x0827940c si_cameraTranslate]: `raw_to_world_mat ← T × raw_to_world_mat` with
    /// T the identity whose row 3 is (tx, ty, tz, 1). The translation acts first on a row
    /// vector. Class: model (through [`mmul`]).
    pub fn camera_translate(&mut self, tx: f32, ty: f32, tz: f32) {
        let mut t = ident();
        t[3][0] = fmov(tx);
        t[3][1] = fmov(ty);
        t[3][2] = fmov(tz);
        self.raw_to_world_mat = mmul(&t, &self.raw_to_world_mat);
    }

    /// Transforms a point from raw to world coordinates: `(v, 1) × raw_to_world_mat` with
    /// the division by the fourth component [soft 0x0827944c SI_rawToWorld].
    ///
    /// The four sums stay in the register and the three quotients are rounded to float
    /// once. If the fourth component compares equal to zero, [`MSG_RAW_TO_WORLD`] is printed
    /// and `out` is **not written**. A NaN divisor is not zero and divides.
    ///
    /// All of `v` is read before `out` is written: the original may be called with the same
    /// vector for both. Class: model.
    pub fn raw_to_world(&self, v: Vector, out: &mut Vector) -> Option<&'static str> {
        project3(&v, &self.raw_to_world_mat, out, msg_raw_to_world())
    }

    /// The same with the matrix for directions, `vect_mat`
    /// [soft 0x08279520 SI_rawToWorldVect]: identical code on the next matrix, translation
    /// and division included. Prints [`MSG_RAW_TO_WORLD_VECT`] and leaves `out` alone when
    /// the divisor is zero. Class: model.
    pub fn raw_to_world_vect(&self, v: Vector, out: &mut Vector) -> Option<&'static str> {
        project3(&v, &self.vect_mat, out, msg_raw_to_world_vect())
    }

    /// Projects a world point with the perspective matrix, clipping at the near plane
    /// [soft 0x08279628 SI_worldToPrsp1].
    ///
    /// - If `v.z` is above `near_z` or the comparison is unordered (a NaN): `out` = (0, 0)
    ///   and nothing else happens.
    /// - Otherwise x, y and the fourth component of `(v, 1) × prsp_mat` are formed in the
    ///   register (z is not computed) and `out` = (x/w, y/w), rounded once.
    /// - If w compares equal to zero: [`MSG_WORLD_TO_PRSP1`] is printed and `out` gets the
    ///   point's own x and y unprojected.
    ///
    /// Class: model.
    pub fn world_to_prsp1(&self, v: Vector, out: &mut Pspoint) -> Option<&'static str> {
        if !x(v.z).le(x(self.near_z)) {
            *out = Pspoint { x: 0.0, y: 0.0 };
            return None;
        }
        self.world_to_prsp2_named(v, out, msg_world_to_prsp1())
    }

    /// The same projection without the near-plane test
    /// [soft 0x08279734 SI_worldToPrsp2]; prints [`MSG_WORLD_TO_PRSP2`] for a zero divisor
    /// and then copies the point's x and y. Class: model.
    pub fn world_to_prsp2(&self, v: Vector, out: &mut Pspoint) -> Option<&'static str> {
        self.world_to_prsp2_named(v, out, msg_world_to_prsp2())
    }

    /// The part the two projections share (in the original it is written out twice).
    fn world_to_prsp2_named(&self, v: Vector, out: &mut Pspoint, message: &'static str) -> Option<&'static str> {
        let m = &self.prsp_mat;
        let px = point_times_column(&v, m, 0);
        let py = point_times_column(&v, m, 1);
        let pw = point_times_column(&v, m, 3);
        if pw.eq(F80::ZERO) {
            *out = Pspoint { x: fmov(v.x), y: fmov(v.y) };
            return Some(message);
        }
        *out = Pspoint { x: (px / pw).to_f32(), y: (py / pw).to_f32() };
        None
    }

    /// Turns a world direction back into raw coordinates with the upper-left 3 × 3 block of
    /// `world_to_raw_mat` [soft 0x08279814 si_worldToRaw]: no translation, no division, no
    /// test. Component j is `(v.x m[0][j] + v.y m[1][j]) + v.z m[2][j]`, rounded once.
    ///
    /// All of `v` is read before anything is stored; the one caller (`reflect`) passes the
    /// same vector as input and output. Class: model.
    pub fn world_to_raw(&self, v: Vector) -> Vector {
        let m = &self.world_to_raw_mat;
        let column = |j: usize| ((x(v.x) * m[0][j] + x(v.y) * m[1][j]) + x(v.z) * m[2][j]).to_f32();
        Vector { x: column(0), y: column(1), z: column(2) }
    }
}

/// `out = (v, 1) × m` divided by its fourth component, or a message and no write when that is
/// zero: the body shared by `SI_rawToWorld` and `SI_rawToWorldVect`.
fn project3(v: &Vector, m: &Matrix, out: &mut Vector, message: &'static str) -> Option<&'static str> {
    let px = point_times_column(v, m, 0);
    let py = point_times_column(v, m, 1);
    let pz = point_times_column(v, m, 2);
    let pw = point_times_column(v, m, 3);
    if pw.eq(F80::ZERO) {
        return Some(message);
    }
    *out = Vector { x: (px / pw).to_f32(), y: (py / pw).to_f32(), z: (pz / pw).to_f32() };
    None
}

/// Transforms a point from camera (world) space into a light's space for a shadow lookup:
/// depth from one matrix, x and y from another [soft 0x08279888 si_cameraToLight].
///
/// With `(v, 1) × m` written as (x, y, z, w) for each matrix:
///
/// 1. w of `depth_mat`. If it compares equal to zero: [`MSG_CAMERA_TO_LIGHT`] is printed and
///    `out` = `v`.
/// 2. `out.z` = z/w of `depth_mat`, **stored now**.
/// 3. w of `xy_mat`. If it compares equal to zero: the message is printed and `out` = `v`,
///    which also replaces the z just stored.
/// 4. `out.x` = x/w and `out.y` = y/w of `xy_mat`.
///
/// Every sum is `((v.x m[0][j] + v.y m[1][j]) + v.z m[2][j]) + m[3][j]` in the register,
/// every quotient rounded to float once.
///
/// Aliasing: the original reads `v` again after step 2, so if `out` were `v` itself, steps 3
/// and 4 would use the new z. The only caller passes two different vectors; this function
/// takes `v` by value and is that case. Class: model.
pub fn camera_to_light(v: Vector, depth_mat: &Matrix, xy_mat: &Matrix, out: &mut Vector) -> Option<&'static str> {
    let copy = Vector { x: fmov(v.x), y: fmov(v.y), z: fmov(v.z) };
    let w = point_times_column(&v, depth_mat, 3);
    if w.eq(F80::ZERO) {
        *out = copy;
        return Some(msg_camera_to_light());
    }
    out.z = (point_times_column(&v, depth_mat, 2) / w).to_f32();
    let px = point_times_column(&v, xy_mat, 0);
    let py = point_times_column(&v, xy_mat, 1);
    let w = point_times_column(&v, xy_mat, 3);
    if w.eq(F80::ZERO) {
        *out = copy;
        return Some(msg_camera_to_light());
    }
    out.x = (px / w).to_f32();
    out.y = (py / w).to_f32();
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const P12: f32 = 4096.0;
    const P32: f32 = 4294967296.0;

    /// The product in doubles, for checking inverses of well-conditioned test matrices.
    fn product_error(a: &Matrix, b: &Matrix) -> f64 {
        let mut worst = 0.0f64;
        for i in 0..4 {
            for j in 0..4 {
                let s: f64 = (0..4).map(|k| a[i][k] as f64 * b[k][j] as f64).sum();
                worst = worst.max((s - if i == j { 1.0 } else { 0.0 }).abs());
            }
        }
        worst
    }

    #[test]
    fn ident_mmul_transpose() {
        let id = ident();
        assert_eq!(id, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        let a = [[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], [9.0, 10.0, 11.0, 12.0], [13.0, 14.0, 15.0, 16.0]];
        let b = [[2.0, 0.0, 0.0, 1.0], [0.0, 3.0, 0.0, 0.0], [1.0, 0.0, 4.0, 0.0], [0.0, 0.0, 0.0, 5.0]];
        assert_eq!(mmul(&a, &id), a);
        assert_eq!(mmul(&id, &a), a);
        assert_eq!(mmul(&a, &b), [[5.0, 6.0, 12.0, 21.0], [17.0, 18.0, 28.0, 45.0], [29.0, 30.0, 44.0, 69.0], [41.0, 42.0, 60.0, 93.0]]);
        assert_eq!(transpose(&a), [[1.0, 5.0, 9.0, 13.0], [2.0, 6.0, 10.0, 14.0], [3.0, 7.0, 11.0, 15.0], [4.0, 8.0, 12.0, 16.0]]);
        assert_eq!(transpose(&transpose(&a)), a);
        // in place: the lower triangle is mirrored onto the upper one
        let mut m = a;
        transpose_in_place(&mut m);
        assert_eq!(m, [[1.0, 5.0, 9.0, 13.0], [5.0, 6.0, 10.0, 14.0], [9.0, 10.0, 11.0, 15.0], [13.0, 14.0, 15.0, 16.0]]);
    }

    #[test]
    fn mmul_accumulates_in_the_register_left_to_right() {
        let mut a = [[0.0f32; 4]; 4];
        let mut b = [[0.0f32; 4]; 4];
        // (2^24 + 1) − 2^24: 1 in the register, 0 if each step were rounded to float
        a[2] = [P12, 1.0, -P12, 0.0];
        (b[0][3], b[1][3], b[2][3]) = (P12, 1.0, P12);
        assert_eq!(mmul(&a, &b)[2][3], 1.0);
        // (2^64 + 1) rounds to 2^64 in the register: left to right 0, any other order 1
        a[2] = [P32, 1.0, -P32, 0.0];
        (b[0][3], b[1][3], b[2][3]) = (P32, 1.0, P32);
        assert_eq!(mmul(&a, &b)[2][3], 0.0);
        // the fourth product is added last: (2^64 − 2^64) + 1 = 1
        a[2] = [P32, -P32, 0.0, 1.0];
        (b[0][3], b[1][3], b[2][3], b[3][3]) = (P32, P32, 0.0, 1.0);
        assert_eq!(mmul(&a, &b)[2][3], 1.0);
        a[2] = [P32, 0.0, 1.0, -P32];
        (b[0][3], b[1][3], b[2][3], b[3][3]) = (P32, 0.0, 1.0, P32);
        assert_eq!(mmul(&a, &b)[2][3], 0.0); // (2^64 + 1) − 2^64
    }

    #[test]
    fn invert_simple_matrices() {
        assert_eq!(invert(&ident()), Some(ident()));
        let d = [[2.0, 0.0, 0.0, 0.0], [0.0, 4.0, 0.0, 0.0], [0.0, 0.0, 8.0, 0.0], [0.0, 0.0, 0.0, 0.5]];
        assert_eq!(invert(&d), Some([[0.5, 0.0, 0.0, 0.0], [0.0, 0.25, 0.0, 0.0], [0.0, 0.0, 0.125, 0.0], [0.0, 0.0, 0.0, 2.0]]));
        // a translation
        let t = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [3.0, -4.0, 5.0, 1.0]];
        assert_eq!(invert(&t), Some([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [-3.0, 4.0, -5.0, 1.0]]));
        // integers with determinant 1: every step is exact
        let m = [[1.0, 2.0, 3.0, 0.0], [0.0, 1.0, 4.0, 0.0], [5.0, 6.0, 0.0, 0.0], [1.0, 1.0, 1.0, 1.0]];
        assert_eq!(invert(&m), Some([[-24.0, 18.0, 5.0, 0.0], [20.0, -15.0, -4.0, 0.0], [-5.0, 4.0, 1.0, 0.0], [9.0, -7.0, -2.0, 1.0]]));
        // a general matrix: the product with the inverse is the identity to float accuracy
        let g = [[0.9, 0.1, -0.3, 0.0], [0.2, 1.1, 0.4, 0.0], [-0.5, 0.3, 0.8, 0.0], [10.0, -20.0, 30.0, 1.0]];
        let inv = invert(&g).unwrap();
        assert!(product_error(&g, &inv) < 1e-4 && product_error(&inv, &g) < 1e-4);
        // singular
        assert_eq!(invert(&[[0.0; 4]; 4]), None);
        let s = [[1.0, 2.0, 3.0, 4.0], [2.0, 4.0, 6.0, 8.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert_eq!(invert(&s), None);
    }

    #[test]
    fn invert_pivot_search() {
        // each permutation matrix is its own transpose-inverse; these need a swap in column
        // 0 with row 1, 2, 3, and in later columns
        let perm = |p: [usize; 4]| {
            let mut m = [[0.0f32; 4]; 4];
            for (i, &j) in p.iter().enumerate() {
                m[i][j] = 1.0;
            }
            m
        };
        for p in [[1, 0, 2, 3], [2, 1, 0, 3], [3, 1, 2, 0], [0, 2, 1, 3], [0, 3, 2, 1], [0, 1, 3, 2], [1, 2, 3, 0], [3, 0, 1, 2], [2, 3, 0, 1]] {
            let m = perm(p);
            assert_eq!(invert(&m), Some(transpose(&m)), "{p:?}");
        }
        // the first row that qualifies is taken, not the largest: with column 0 = (0, 2, 8, 0)
        // row 1 is swapped in. Both choices give the same inverse here; the result is exact.
        let m = [[0.0, 1.0, 0.0, 0.0], [2.0, 0.0, 0.0, 0.0], [8.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert_eq!(invert(&m), Some([[0.0, 0.5, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, -4.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]));
    }

    #[test]
    fn invert_tolerance_is_the_double_and_asymmetric() {
        let diag = |d: [f32; 4]| {
            let mut m = [[0.0f32; 4]; 4];
            for i in 0..4 {
                m[i][i] = d[i];
            }
            m
        };
        // the float nearest 0.0001 is below the double 0.0001: too small for a pivot, and
        // nothing below it to swap with, so a perfectly good matrix is "singular"
        let small = 1e-4f32;
        assert!((small as f64) < 1e-4);
        let next = f32::from_bits(small.to_bits() + 1);
        for k in 0..4 {
            let mut d = [1.0f32; 4];
            d[k] = small;
            assert_eq!(invert(&diag(d)), None, "column {k}");
            d[k] = -small;
            assert_eq!(invert(&diag(d)), None, "column {k}");
            d[k] = next;
            let inv = invert(&diag(d)).unwrap();
            assert_eq!(inv[k][k], (F80::ONE / next).to_f32());
        }
        // a pivot below the tolerance is replaced by the first entry above it: column 0
        // offers 2^-15 (too small), 0 (row 1) and 2^-13 = 0.000122 (row 2, taken). All steps
        // are exact with these powers of two.
        let (e15, e13) = (2.0f32.powi(-15), 2.0f32.powi(-13));
        let m = [[e15, 1.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [e13, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert_eq!(invert(&m), Some([[32768.0, -32768.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [-4.0, 4.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]));
        // … and a pivot of 2^-13 is used as it is although 1.0 stands below it: there is no
        // search for the largest pivot
        let m = [[e13, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert_eq!(invert(&m), Some([[0.0, 1.0, 0.0, 0.0], [1.0, -e13, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]));
    }

    #[test]
    fn invert_compares_the_first_two_pivots_as_floats_and_the_last_two_unrounded() {
        // The block [[1, 0.4], [−2^-37, s]] with s the float below 0.0001 (13743895 × 2^-37;
        // the double 0.0001 is 13743895.347 × 2^-37). Eliminating below the 1 turns s into
        // s + 0.4 × 2^-37 = 13743895.4 × 2^-37 in the register: above the tolerance. Stored
        // as a float it is s again: below the tolerance.
        let s = 1e-4f32;
        assert_eq!(s.to_bits(), 0x38d1_b717);
        let k = 2.0f32.powi(-37);
        let block_at = |p: usize| {
            let mut m = ident();
            (m[p][p + 1], m[p + 1][p], m[p + 1][p + 1]) = (0.4, -k, s);
            m
        };
        // as pivot of column 1 the float is compared: refused (and no row to swap with)
        assert_eq!(invert(&block_at(0)), None);
        // as pivot of column 2 or 3 the register value is compared: accepted, and the
        // reciprocal 1/s (column 2: of the float; column 3: of the register value) is the
        // float 10000 either way
        let big = 10000.0f32;
        assert_eq!(invert(&block_at(1)), Some([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, -4000.0, 0.0], [0.0, big * k, big, 0.0], [0.0, 0.0, 0.0, 1.0]]));
        assert_eq!(invert(&block_at(2)), Some([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, -4000.0], [0.0, 0.0, big * k, big]]));
        // without the 0.4 the register holds the float itself and all three are refused
        for p in 0..3 {
            let mut m = block_at(p);
            m[p][p + 1] = 0.0;
            assert_eq!(invert(&m), None);
        }
    }

    #[test]
    fn invert_keeps_some_products_unrounded() {
        // Worked by hand through the original's four steps.
        //   [1 1 0  1]      steps 0 and 2 change nothing (the entries to clear are 0);
        //   [0 1 0 -1]      step 1 (pivot 1) leaves row 0 = (1 0 0 2 | 1 −1 0 0) and
        //   [0 0 1  0]      row 3 = (0 0 0 6 | 0 −3 0 1).
        //   [0 3 0  3]
        // Step 3: r = float(1/6) = 11184811 × 2^-26. Row 3 × r in column 5 is
        // −3r = −(2^25 + 1) × 2^-26 = −(0.5 + 2^-26): 26 significant bits. Rounded to float
        // (as the destination's row 3 stores it) it is −0.5.
        //   out[0][1] uses the UNROUNDED product: (−(0.5 + 2^-26)) × (−2) + (−1) = 2^-25,
        //     where the exact inverse has 0 and the rounded product would give 0.
        //   out[1][1] uses the ROUNDED one: (−0.5) × 1 + 1 = 0.5 exactly (unrounded:
        //     0.5 − 2^-26).
        let m = [[1.0, 1.0, 0.0, 1.0], [0.0, 1.0, 0.0, -1.0], [0.0, 0.0, 1.0, 0.0], [0.0, 3.0, 0.0, 3.0]];
        let inv = invert(&m).unwrap();
        let sixth = f32::from_bits(0x3e2a_aaab);
        assert_eq!(sixth, 1.0f32 / 6.0);
        assert_eq!(inv[0][1].to_bits(), 0x3300_0000); // 2^-25
        assert_eq!(inv[1][1].to_bits(), 0x3f00_0000); // 0.5
        assert_eq!(inv, [[1.0, 2.0f32.powi(-25), 0.0, -2.0 * sixth], [0.0, 0.5, 0.0, sixth], [0.0, 0.0, 1.0, 0.0], [0.0, -0.5, 0.0, sixth]]);
        assert_eq!(inv[0][3].to_bits(), 0xbeaa_aaab);
    }

    #[test]
    fn invert_multiplies_by_a_float_reciprocal() {
        // Pivot 3: r = float(1/3) = 11184811 × 2^-25, so the scaled pivot is 3r = 1 + 2^-25
        // in the register (stored as 1). The entry below it becomes 3 − (1 + 2^-25) × 3 =
        // −3 × 2^-25 rather than 0; nothing reads that residue again. Row 1 of the right
        // half gets r × (−3) + 0 = −(1 + 2^-25), which rounds to −1: the exact inverse of
        // [[3, 0], [3, 1]], [[1/3, 0], [−1, 1]], comes out, with 1/3 as that float.
        let m = [[3.0, 0.0, 0.0, 0.0], [3.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let third = f32::from_bits(0x3eaa_aaab);
        assert_eq!(invert(&m), Some([[third, 0.0, 0.0, 0.0], [-1.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]));
        // a pivot of 3 in the last column: the reciprocal is taken of the register value and
        // the last row is that float; 7 × r = 2.3333335 for the entry above
        let m = [[1.0, 0.0, 0.0, 7.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 3.0]];
        let inv = invert(&m).unwrap();
        assert_eq!(inv[3], [0.0, 0.0, 0.0, third]);
        assert_eq!(inv[0], [1.0, 0.0, 0.0, (x(-7.0f32) * third).to_f32()]);
        assert_eq!(inv[0][3].to_bits(), 0xc015_5556);
    }

    fn camera_with(m: Matrix) -> RenderCamera {
        RenderCamera { raw_to_world_mat: m, ..RenderCamera::default() }
    }

    #[test]
    fn camera_translate_and_rotate_premultiply() {
        si_rt::skip_without_install!();
        let base = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [10.0, 20.0, 30.0, 1.0]];
        let mut cam = camera_with(base);
        cam.camera_translate(1.0, 2.0, 3.0);
        // T × M: the new translation goes through M's rotation: (1,2,3) → (−2, 1, 3)
        assert_eq!(cam.raw_to_world_mat, [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [8.0, 21.0, 33.0, 1.0]]);
        assert_eq!(cam.world_to_raw_mat, [[0.0; 4]; 4]); // nothing else is touched

        // axes 1 and 2 take sine and cosine
        let (s, c) = (0.6f32, 0.8f32);
        let mut cam = camera_with(ident());
        assert_eq!(cam.camera_rotate(1, s, c), None);
        assert_eq!(cam.raw_to_world_mat, [[1.0, 0.0, 0.0, 0.0], [0.0, c, s, 0.0], [0.0, -s, c, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        let mut cam = camera_with(ident());
        assert_eq!(cam.camera_rotate(2, s, c), None);
        assert_eq!(cam.raw_to_world_mat, [[c, 0.0, -s, 0.0], [0.0, 1.0, 0.0, 0.0], [s, 0.0, c, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        // axis 3 takes an angle in radians and ignores the third argument
        let quarter = f32::from_bits(0x3fc9_0fdb); // the float nearest π/2
        let tiny = -4.371139e-8f32; // its cosine
        for ignored in [0.0f32, 123.0, f32::NAN] {
            let mut cam = camera_with(ident());
            assert_eq!(cam.camera_rotate(3, quarter, ignored), None);
            assert_eq!(cam.raw_to_world_mat, [[tiny, 1.0, 0.0, 0.0], [-1.0, tiny, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        }
        // R × M, not M × R: after a translation matrix the rotation leaves row 3 alone
        let mut cam = camera_with(base);
        cam.camera_rotate(1, 1.0, 0.0);
        assert_eq!(cam.raw_to_world_mat[3], base[3]);
        assert_eq!(cam.raw_to_world_mat[1], base[2]); // new row 1 = 0·row1 + 1·row2
        assert_eq!(cam.raw_to_world_mat[2], [1.0, 0.0, 0.0, 0.0]); // −row1

        // a bad axis prints and multiplies by the identity all the same: −0 becomes +0 and
        // an infinity spreads NaNs along its row (0 × ∞)
        for axis in [0i16, 4, -1, 259] {
            let mut odd = base;
            odd[0][0] = -0.0;
            odd[2][3] = f32::INFINITY;
            let mut cam = camera_with(odd);
            assert_eq!(cam.camera_rotate(axis, 0.5, 0.5), Some(msg_camera_rotate()));
            let m = cam.raw_to_world_mat;
            assert_eq!(m[0][0].to_bits(), 0);
            assert!(m[0][3].is_nan() && m[1][3].is_nan() && m[3][3].is_nan() && m[2][3] == f32::INFINITY);
            assert_eq!(m[3][..3], base[3][..3]);
        }
    }

    #[test]
    fn raw_to_world_and_vect() {
        si_rt::skip_without_install!();
        let m = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0], [10.0, 20.0, 30.0, 1.0]];
        let n = [[1.0, 0.0, 0.0, 0.5], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let cam = RenderCamera { raw_to_world_mat: m, vect_mat: n, ..RenderCamera::default() };
        let mut out = Vector::new(9.0, 9.0, 9.0);
        assert_eq!(cam.raw_to_world(Vector::new(1.0, 2.0, 3.0), &mut out), None);
        assert_eq!(out, Vector::new(8.0, 21.0, 36.0));
        // the "vector" variant is a full point transformation with another matrix: here
        // w = 0.5 x + 1 = 2
        assert_eq!(cam.raw_to_world_vect(Vector::new(2.0, 4.0, 6.0), &mut out), None);
        assert_eq!(out, Vector::new(1.0, 2.0, 3.0));
        // w = 0: a message and no write
        let mut out = Vector::new(9.0, 9.0, 9.0);
        assert_eq!(cam.raw_to_world_vect(Vector::new(-2.0, 4.0, 6.0), &mut out), Some(msg_raw_to_world_vect()));
        assert_eq!(out, Vector::new(9.0, 9.0, 9.0));
        let zero = RenderCamera::default();
        assert_eq!(zero.raw_to_world(Vector::new(1.0, 2.0, 3.0), &mut out), Some(msg_raw_to_world()));
        assert_eq!(out, Vector::new(9.0, 9.0, 9.0));
        // a NaN divisor is not zero
        let mut bad = cam;
        bad.raw_to_world_mat[3][3] = f32::NAN;
        assert_eq!(bad.raw_to_world(Vector::new(1.0, 2.0, 3.0), &mut out), None);
        assert!(out.x.is_nan() && out.y.is_nan() && out.z.is_nan());
    }

    #[test]
    fn point_transforms_accumulate_in_the_register_left_to_right() {
        si_rt::skip_without_install!();
        // column 0 = (2^12, 1, 2^12, 0) and the point (2^12, 1, −2^12): (2^24 + 1) − 2^24 = 1
        let mut m = [[0.0f32; 4]; 4];
        (m[0][0], m[1][0], m[2][0], m[3][3]) = (P12, 1.0, P12, 1.0);
        let mut cam = RenderCamera { raw_to_world_mat: m, vect_mat: m, prsp_mat: m, world_to_raw_mat: m, near_z: 0.0 };
        let v = Vector::new(P12, 1.0, -P12);
        let mut out = Vector::default();
        cam.raw_to_world(v, &mut out);
        assert_eq!(out.x, 1.0);
        cam.raw_to_world_vect(v, &mut out);
        assert_eq!(out.x, 1.0);
        assert_eq!(cam.world_to_raw(v).x, 1.0);
        let mut p = Pspoint::default();
        cam.world_to_prsp1(v, &mut p);
        assert_eq!(p.x, 1.0);
        cam.world_to_prsp2(v, &mut p);
        assert_eq!(p.x, 1.0);
        camera_to_light(v, &m, &m, &mut out);
        assert_eq!(out.x, 1.0);
        // 2^64 + 1 rounds to 2^64: (x·m0 + y·m1) + z·m2 gives 0, other orders 1
        (m[0][0], m[2][0]) = (P32, P32);
        cam = RenderCamera { raw_to_world_mat: m, vect_mat: m, prsp_mat: m, world_to_raw_mat: m, near_z: 0.0 };
        let v = Vector::new(P32, 1.0, -P32);
        cam.raw_to_world(v, &mut out);
        assert_eq!(out.x, 0.0);
        cam.raw_to_world_vect(v, &mut out);
        assert_eq!(out.x, 0.0);
        assert_eq!(cam.world_to_raw(v).x, 0.0);
        cam.world_to_prsp1(v, &mut p);
        assert_eq!(p.x, 0.0);
        cam.world_to_prsp2(v, &mut p);
        assert_eq!(p.x, 0.0);
        camera_to_light(v, &m, &m, &mut out);
        assert_eq!(out.x, 0.0);
        // the translation is added after the three products: (2^64 − 2^64) + 1 = 1
        (m[1][0], m[3][0]) = (0.0, 1.0);
        cam.raw_to_world_mat = m;
        cam.raw_to_world(v, &mut out);
        assert_eq!(out.x, 1.0);
        // the quotient is formed from the unrounded sum: x' = 2^24 + 1 is exact in the
        // register and (2^24 + 1)/5 = 3355443.4 rounds to 3355443.5; from the sum rounded to
        // float (2^24) it would be 3355443.2, which rounds to 3355443.25
        let mut q = [[0.0f32; 4]; 4];
        (q[0][0], q[3][0], q[3][3]) = (16777216.0, 1.0, 5.0);
        cam = RenderCamera { raw_to_world_mat: q, vect_mat: q, prsp_mat: q, world_to_raw_mat: q, near_z: 0.0 };
        let v = Vector::new(1.0, 0.0, 0.0);
        cam.raw_to_world(v, &mut out);
        assert_eq!(out.x, 3355443.5);
        cam.raw_to_world_vect(v, &mut out);
        assert_eq!(out.x, 3355443.5);
        cam.world_to_prsp1(v, &mut p);
        assert_eq!(p.x, 3355443.5);
        cam.world_to_prsp2(v, &mut p);
        assert_eq!(p.x, 3355443.5);
        camera_to_light(v, &q, &q, &mut out);
        assert_eq!(out.x, 3355443.5);
        assert_eq!((16777216.0f32 + 1.0) / 5.0, 13421773.0 / 4.0); // 3355443.25
    }

    #[test]
    fn world_to_prsp_clipping_and_zero_divisor() {
        si_rt::skip_without_install!();
        // a simple perspective: x' = x, y' = 2y, w = −z
        let p = [[1.0, 0.0, 0.0, 0.0], [0.0, 2.0, 0.0, 0.0], [0.0, 0.0, 1.0, -1.0], [0.0, 0.0, 0.0, 0.0]];
        let cam = RenderCamera { prsp_mat: p, near_z: -1.0, ..RenderCamera::default() };
        let mut out = Pspoint { x: 9.0, y: 9.0 };
        assert_eq!(cam.world_to_prsp1(Vector::new(3.0, 4.0, -2.0), &mut out), None);
        assert_eq!(out, Pspoint { x: 1.5, y: 4.0 });
        // on the plane: projected; in front of it (z above near_z) or NaN: (0, 0)
        assert_eq!(cam.world_to_prsp1(Vector::new(3.0, 4.0, -1.0), &mut out), None);
        assert_eq!(out, Pspoint { x: 3.0, y: 8.0 });
        for z in [-0.999f32, 0.0, 5.0, f32::NAN] {
            out = Pspoint { x: 9.0, y: 9.0 };
            assert_eq!(cam.world_to_prsp1(Vector::new(3.0, 4.0, z), &mut out), None);
            assert_eq!(out, Pspoint { x: 0.0, y: 0.0 });
            // the second variant has no such test
            cam.world_to_prsp2(Vector::new(3.0, 4.0, 5.0), &mut out);
            assert_eq!(out, Pspoint { x: -0.6, y: -1.6 });
        }
        // a NaN plane rejects everything
        let nan_plane = RenderCamera { near_z: f32::NAN, ..cam };
        assert_eq!(nan_plane.world_to_prsp1(Vector::new(3.0, 4.0, -2.0), &mut out), None);
        assert_eq!(out, Pspoint { x: 0.0, y: 0.0 });
        // w = 0: a message, and the point's own x and y
        let far = RenderCamera { near_z: 1.0, ..cam };
        assert_eq!(far.world_to_prsp1(Vector::new(3.0, 4.0, 0.0), &mut out), Some(msg_world_to_prsp1()));
        assert_eq!(out, Pspoint { x: 3.0, y: 4.0 });
        out = Pspoint { x: 9.0, y: 9.0 };
        assert_eq!(cam.world_to_prsp2(Vector::new(3.0, 4.0, 0.0), &mut out), Some(msg_world_to_prsp2()));
        assert_eq!(out, Pspoint { x: 3.0, y: 4.0 });
    }

    #[test]
    fn world_to_raw_is_the_three_by_three_block() {
        let m = [[0.0, 1.0, 0.0, 7.0], [-1.0, 0.0, 0.0, 7.0], [0.0, 0.0, 2.0, 7.0], [10.0, 20.0, 30.0, 7.0]];
        let cam = RenderCamera { world_to_raw_mat: m, ..RenderCamera::default() };
        // no translation, no division
        assert_eq!(cam.world_to_raw(Vector::new(1.0, 2.0, 3.0)), Vector::new(-2.0, 1.0, 6.0));
        assert_eq!(RenderCamera::default().world_to_raw(Vector::new(1.0, 2.0, 3.0)), Vector::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn camera_to_light_takes_depth_and_position_from_two_matrices() {
        si_rt::skip_without_install!();
        // depth matrix: z' = 2z + 1, w = 4; position matrix: x' = x + 10, y' = 3y, w = z
        let depth = [[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0], [0.0, 0.0, 1.0, 4.0]];
        let xy = [[1.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [10.0, 0.0, 0.0, 0.0]];
        let v = Vector::new(2.0, 4.0, 8.0);
        let mut out = Vector::new(9.0, 9.0, 9.0);
        assert_eq!(camera_to_light(v, &depth, &xy, &mut out), None);
        assert_eq!(out, Vector::new(1.5, 1.5, 4.25));
        // first divisor zero: message, the point copied
        let zero = [[0.0f32; 4]; 4];
        out = Vector::new(9.0, 9.0, 9.0);
        assert_eq!(camera_to_light(v, &zero, &xy, &mut out), Some(msg_camera_to_light()));
        assert_eq!(out, v);
        // second divisor zero: the same, and the depth already stored is replaced
        out = Vector::new(9.0, 9.0, 9.0);
        assert_eq!(camera_to_light(Vector::new(2.0, 4.0, 0.0), &depth, &xy, &mut out), Some(msg_camera_to_light()));
        assert_eq!(out, Vector::new(2.0, 4.0, 0.0));
        // a NaN divisor is not zero
        let mut bad = xy;
        bad[3][3] = f32::NAN;
        assert_eq!(camera_to_light(v, &depth, &bad, &mut out), None);
        assert!(out.x.is_nan() && out.y.is_nan() && out.z == 4.25);
    }

    #[test]
    fn camera_setup_as_the_caller_does_it() {
        si_rt::skip_without_install!();
        // The sequence SI_camera uses (in another source file): identity, roll about Z, tilt
        // about X, pan about Y, translate by minus the position, then invert. A camera at
        // (0, 0, 10) with no rotation: world = raw − (0, 0, 10).
        let mut cam = camera_with(ident());
        cam.camera_rotate(3, -0.0, 0.0);
        cam.camera_rotate(1, -0.0, 1.0);
        cam.camera_rotate(2, 0.0, 1.0);
        cam.camera_translate(-0.0, -0.0, -10.0);
        assert_eq!(cam.raw_to_world_mat, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, -10.0, 1.0]]);
        cam.world_to_raw_mat = invert(&cam.raw_to_world_mat).unwrap();
        assert_eq!(cam.world_to_raw_mat[3], [0.0, 0.0, 10.0, 1.0]);
        let mut world = Vector::default();
        assert_eq!(cam.raw_to_world(Vector::new(1.0, 2.0, 3.0), &mut world), None);
        assert_eq!(world, Vector::new(1.0, 2.0, -7.0));
        assert_eq!(mmul(&cam.raw_to_world_mat, &cam.world_to_raw_mat), ident());
    }
}
