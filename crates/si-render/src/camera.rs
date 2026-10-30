// SPDX-License-Identifier: MIT
//! The camera set-up `SI_camera`, the depth of field arithmetic and the pixel filter of the
//! original's `camera.c` (finding 0012, sections 4 and 8), with `si_initFade` and, ported
//! in place on the memory image, `si_initDof`.
//!
//! The four `find…` functions return their value in the x87 register, unrounded; here they
//! return [`F80`]. What the three float parameters *are* is not said by the code; by the
//! formulas (the hyperfocal distance f² / (N c) and the near and far limits of sharpness of
//! a lens focused at s) they are named focal length, aperture number, circle of confusion
//! and focus distance. Those names are a reading of the formulas.

use crate::types::Vector;
use crate::update::{Settings, View};
use si_math::matrix::{ident, invert, mmul, RenderCamera};
use si_x87::{fmov, trig, x, Round, F80};

/// 0x08654490, float: the least circle of confusion `findCoc` works with (the float nearest
/// 1e-6).
const COC_MIN: f32 = f32::from_bits(0x3586_37bd);

/// `(a × a) / (b × c)`, unrounded [soft 0x0827299c findHyperfocal]. Read as the hyperfocal
/// distance: focal length² / (aperture number × circle of confusion). Class: model.
pub fn find_hyperfocal(a: f32, b: f32, c: f32) -> F80 {
    (x(a) * a) / (x(b) * c)
}

/// `(b × a) / ((a + b) − c)`, unrounded [soft 0x082729b4 findNear]. Read as the near limit of
/// sharpness with a the hyperfocal distance, b the focus distance, c the focal length.
/// Class: model.
pub fn find_near(a: f32, b: f32, c: f32) -> F80 {
    (x(b) * a) / ((x(a) + b) - c)
}

/// `(b × a) / ((a − b) − c)`, unrounded [soft 0x082729d0 findFar]: the far limit; negative
/// once the focus distance is beyond the hyperfocal distance. Class: model.
pub fn find_far(a: f32, b: f32, c: f32) -> F80 {
    (x(b) * a) / ((x(a) - b) - c)
}

/// `findFar(float(findHyperfocal(a, b, float(d × 0.85))), c, a) / 12`, unrounded
/// [soft 0x08272c48 findOutoFocus]. 0.85 is the double at 0x086544c0, 12 the float at
/// 0x086544c8. Class: model.
pub fn find_outo_focus(a: f32, b: f32, c: f32, d: f32) -> F80 {
    let coc = (x(d) * 0.85f64).to_f32();
    let h = find_hyperfocal(a, b, coc).to_f32();
    find_far(h, c, a) / 12.0f32
}

/// Searches for the circle of confusion at which the far limit of sharpness stops being
/// positive [soft 0x082729ec findCoc]: `findCoc(a, b, c)` with
/// far(coc) = `findFar(float(findHyperfocal(a, b, coc)), c, a)`.
///
/// Four passes with steps 0.01, 0.001, 0.0001 and 0.00001 (doubles at 0x086544a0 to
/// 0x086544b8). The first starts at 1e-6 and runs while the stepped value is below 0.1 (the
/// double at 0x08654498) and far is above zero. Each later pass starts 0.01 below where the
/// pass before stopped (**0.01 every time, not the pass's own step**; 1e-6 if that is
/// negative) and runs while below that stopping value and far is above zero. The result is
/// the last pass's value less 0.00001, or 1e-6 if that is negative. Every value is rounded
/// to float as it is stepped. The value is evaluated before the step and tested after it,
/// so each pass overshoots by one step. Class: model.
pub fn find_coc(a: f32, b: f32, c: f32) -> f32 {
    let far = |coc: f32| find_far(find_hyperfocal(a, b, coc).to_f32(), c, a);
    let floor = |v: f32| if x(0.0f32).gt(x(v)) { COC_MIN } else { v };
    let mut coc = COC_MIN;
    loop {
        let f = far(coc);
        coc = (x(0.01f64) + coc).to_f32();
        if !x(coc).lt(x(0.1f64)) || !x(0.0f32).lt(f) {
            break;
        }
    }
    for step in [0.001f64, 0.0001, 0.00001] {
        let limit = coc;
        coc = floor((x(limit) - 0.01f64).to_f32());
        if x(coc).lt(x(limit)) {
            loop {
                let f = far(coc);
                coc = (x(coc) + step).to_f32();
                if !x(coc).lt(x(limit)) || !x(0.0f32).lt(f) {
                    break;
                }
            }
        }
    }
    floor((x(coc) - 0.00001f64).to_f32())
}

/// Fills a square table with a cone-shaped weight [soft 0x08273218 makeFilter]:
/// `makeFilter(rows, radius, size)`. With h = size / 2 (C division) and r² = float(radius² +
/// 0.5), entry [i + h][j + h] for i, j in −h..=h is (r² − (i² + j²)) / r² where i² + j² is
/// below r², else 0. The weights are not normalised here. `rows` must hold 2h + 1 rows of
/// 2h + 1 floats (the original writes through the row pointers it is given). Class: model.
pub fn make_filter(rows: &mut [Vec<f32>], radius: f32, size: i32) {
    let h = size / 2;
    let r2 = (x(radius) * radius + 0.5f64).to_f32();
    for i in -h..=h {
        for j in -h..=h {
            let d = i * i + j * j;
            rows[(i + h) as usize][(h + j) as usize] = if x(d).lt(x(r2)) { ((x(r2) - x(d)) / r2).to_f32() } else { 0.0 };
        }
    }
}

/// 0x086544d0 (double), 0x086544d8, 0x086544dc, 0x086544e8 (floats), 0x086544e0, 0x086544f0
/// (doubles): the constants of `si_initDof`.
const MM_TO_INCH: f64 = 0.0394;
const TWELVE: f32 = 12.0;
const COC_DEFAULT: f32 = f32::from_bits(0x3a83_126f);
const FAR_DEFAULT: f32 = 32000.0;
const COC_LIMIT: f64 = 0.2;
const BEYOND: f64 = 0.001;
/// 0x0869cecb and 0x0869ced6 (the same text twice), 0x0869cf00, 0x0869cf20.
pub const ENV_DOF_COC: &[u8] = b"SI_DOF_COC";
pub fn msg_coc_negative() -> &'static str {
    si_rt::text(0x0869_cf00)
}
pub const FMT_COC_LIMIT: &str = "Max COC exceeds limit. Clamping at ";

/// `makeFilter (0x08273218)`(rows, radius, size) through row pointers in the image: the
/// ported [`make_filter`], its floats stored where the original stores them.
fn make_filter_at(m: &mut si_mem::Mem, rows: u32, radius: f32, size: i32) {
    let n = (size / 2 * 2 + 1).max(0) as usize;
    let mark = f32::from_bits(0x7fa5_a5a5);
    let mut t = vec![vec![mark; n]; n];
    make_filter(&mut t, radius, size);
    for (i, row) in t.iter().enumerate() {
        let p = m.u32(rows.wrapping_add(4 * i as u32));
        for (j, v) in row.iter().enumerate() {
            if v.to_bits() != mark.to_bits() {
                m.set_f32(p.wrapping_add(4 * j as u32), *v);
            }
        }
    }
}

/// The depth of field of a frame [soft 0x08272c98 si_initDof]: `si_initDof()`, ported in
/// place (on the memory image). Nothing happens when the camera copy's mode (0x087906ac,
/// camera +0x6c) is 0.
///
/// **Mode 1, from the lens.** f = r32(camera +0x70 × 0.0394) (a focal length in
/// millimetres to inches), d = r32(camera +0x74 × 12) (a focus distance in feet to inches),
/// N = camera +0x94 (the aperture number). The circle of confusion c is 0.001, or what the
/// environment variable `SI_DOF_COC` holds (`strtod`, rounded to float) when it is set and
/// above 0 (a NaN is taken as it is). H = `findHyperfocal`(f, N, c); near = r32(r32(
/// `findNear`(r32 H, d, f)) / 12), far = r32(r32(`findFar`(r32 H, d, f)) / 12); the
/// widest circle = r32(**the width of `SI_defaultControl`'s picture (0x08781d9a)** ×
/// `findCoc`(f, N, d)); the out-of-focus distance = r32(`findOutoFocus`(f, N, d, that
/// `findCoc`)).
///
/// **Any other mode, given.** The circle is camera +0x98, near +0x7c, far +0x84, the
/// out-of-focus distance +0x8c. A circle below 0 becomes 0 (the message [`MSG_COC_NEGATIVE`]
/// is formatted and never shown); a circle above 0.2 × the picture's width (0x0879117a)
/// becomes r32 of that, with the status line [`FMT_COC_LIMIT`] and the value as `%f`.
///
/// **Then:** near is at least 0; far below near becomes 32000; an out-of-focus distance
/// below far becomes r32(0.001 + far); the three are stored negated at 0x087906c0,
/// 0x087906c8, 0x087906d0 (camera +0x80, +0x88, +0x90 of the copy). The number of blur
/// lines (0x08790b28) = trunc(sampling level × circle), 5 if below 5, made odd by adding 1;
/// half (0x08790b2c) = (lines − 1) / 2; the default radius (0x08790b30) = lines / 4. The
/// window's x max and y max (0x087907b4, 0x087907bc) grow by lines. The filter table
/// (0x08790b38) comes from chunk memory: half + 2 tables of `lines` row pointers and
/// `lines` rows of `lines` floats, table k filled by `makeFilter`(table, k, lines). Last,
/// the limit `dofSubdivid` refines below (0x08790b24) = r32(out-of-focus depth ÷ half),
/// with the depth's sign flipped first when the quotient is below 0 or not a number.
///
/// Class: model.
pub fn init_dof(m: &mut si_mem::Mem, env: &mut dyn crate::img::Outside) {
    use crate::img::{f, g};
    const CAM: u32 = g::SI_DATABASE;
    // the routine works on a copy of the camera it takes first
    let c = |m: &si_mem::Mem, o: u32| m.f32(CAM + o);
    let (f70, f74, f7c, f84, f8c, f94, f98) = (c(m, 0x70), c(m, 0x74), c(m, 0x7c), c(m, 0x84), c(m, 0x8c), c(m, 0x94), c(m, 0x98));
    let mode = m.i32(CAM + 0x6c);
    if mode == 0 {
        return;
    }
    let (mut circle, mut near, mut far, mut oof);
    if mode == 1 {
        let focal = (x(f70) * MM_TO_INCH).to_f32();
        let focus = (x(f74) * TWELVE).to_f32();
        let mut coc = COC_DEFAULT;
        if env.call(m, f::GETENV, &[0x0869_cecb]) != 0 {
            let text = env.call(m, f::GETENV, &[0x0869_ced6]);
            coc = crate::img::strtod(&m.cstr(text)) as f32;
        }
        if !(x(0.0f32).lt(x(coc)) || coc.is_nan()) {
            coc = COC_DEFAULT;
        }
        let hyper = find_hyperfocal(focal, f94, coc).to_f32();
        near = find_near(hyper, focus, focal).to_f32();
        far = find_far(hyper, focus, focal).to_f32();
        near = (x(near) / x(TWELVE)).to_f32();
        far = (x(far) / x(TWELVE)).to_f32();
        let widest = find_coc(focal, f94, focus);
        oof = find_outo_focus(focal, f94, focus, widest).to_f32();
        circle = (x(m.i16(0x0878_1d9a) as i32) * x(widest)).to_f32();
    } else {
        (circle, near, far, oof) = (f98, f7c, f84, f8c);
        if x(0.0f32).gt(x(circle)) {
            // (the original formats MSG_COC_NEGATIVE into a local and shows nothing)
            circle = 0.0;
        } else {
            let limit = x(m.i16(g::WIDTH) as i32) * COC_LIMIT;
            if limit.lt(x(circle)) {
                circle = limit.to_f32();
                let text = format!("{FMT_COC_LIMIT}{:.6}", circle as f64);
                let buffer = m.local(256);
                m.put_cstr(buffer, text.as_bytes());
                env.call(m, f::SI_STATUS_DISPLAY_MESSAGE, &[buffer, 2]);
                m.release(256);
            }
        }
    }
    if x(0.0f32).gt(x(near)) {
        near = 0.0;
    }
    if x(far).lt(x(near)) {
        far = FAR_DEFAULT;
    }
    if x(oof).lt(x(far)) {
        oof = (x(BEYOND) + x(far)).to_f32();
    }
    m.set_f32(CAM + 0x80, (-x(near)).to_f32());
    m.set_f32(CAM + 0x88, (-x(far)).to_f32());
    m.set_f32(CAM + 0x90, (-x(oof)).to_f32());
    const LINES: u32 = 0x0879_0b28;
    const HALF: u32 = 0x0879_0b2c;
    const TABLE: u32 = 0x0879_0b38;
    let mut lines = (x(m.i16(0x0879_0d38) as i32) * x(circle)).to_i32(Round::Zero);
    if lines <= 4 {
        lines = 5;
    }
    if lines.wrapping_sub(lines.wrapping_add(((lines as u32) >> 31) as i32) & !1) != 1 {
        lines = lines.wrapping_add(1);
    }
    m.set_i32(LINES, lines);
    let half = lines.wrapping_sub(1) / 2;
    m.set_i32(HALF, half);
    m.set_i32(0x0879_0b30, lines / 4);
    let x_max = (x(m.f32(0x0879_07b4)) + x(lines)).to_f32();
    let y_max = (x(lines) + x(m.f32(0x0879_07bc))).to_f32();
    m.set_f32(0x0879_07b4, x_max);
    m.set_f32(0x0879_07bc, y_max);
    let chunk = m.u32(g::SI_REN_CHUNK);
    let table = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, (half as u32).wrapping_mul(4).wrapping_add(8)]);
    m.set_u32(TABLE, table);
    let mut k: i32 = 0;
    while k < m.i32(HALF).wrapping_add(2) {
        let chunk = m.u32(g::SI_REN_CHUNK);
        let rows = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, (m.i32(LINES) as u32) << 2]);
        let slot = m.u32(TABLE).wrapping_add(4 * k as u32);
        m.set_u32(slot, rows);
        let mut i: i32 = 0;
        while i < m.i32(LINES) {
            let chunk = m.u32(g::SI_REN_CHUNK);
            let row = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, (m.i32(LINES) as u32) << 2]);
            let rows = m.u32(m.u32(TABLE).wrapping_add(4 * k as u32));
            m.set_u32(rows.wrapping_add(4 * i as u32), row);
            i += 1;
        }
        let rows = m.u32(m.u32(TABLE).wrapping_add(4 * k as u32));
        make_filter_at(m, rows, x(k).to_f32(), m.i32(LINES));
        k += 1;
    }
    let depth = x(m.f32(CAM + 0x90));
    let half = x(m.i32(HALF));
    let mut q = depth / half;
    if x(0.0f32).gt(q) || q.is_nan() {
        q = (-depth) / half;
    }
    m.set_f32(0x0879_0b24, q.to_f32());
}

/// What the render set-up gives `si_initFade`: 0x08790ce4, 0x08790cec and 0x08790d10.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fade {
    /// 0x08790ce4: the distance at which the fade starts.
    pub start: f32,
    /// 0x08790cec: the distance at which it is complete.
    pub end: f32,
    /// 0x08790d10: one minus the greatest fade (a guess at its meaning: the fade's
    /// transparency; the routine only takes its complement).
    pub amount: f32,
}

/// The double 1e-6 (0x08654500): the least distance between the fade's start and end.
const FADE_LEAST: f64 = 1e-6;

/// The background and the fade (depth cue) of a frame [soft 0x082732c8 si_initFade]:
/// `si_initFade()`.
///
/// - Fade off (0x08790ce0 = 0): the background (0x08790cf8, six floats) becomes the blank
///   sample (0x08790b88).
/// - Fade on: an end closer than 1e-6 beyond the start is moved to r32(start + 1e-6) (in
///   the caller's value too); the background's alpha = r32(1 − the float at 0x08790d10);
///   red, green, blue and that alpha are held to 0 … 1 (a NaN passes); the start and the
///   end are copied to 0x08790ce8 and 0x08790cf0 and the factor (0x08790cf4) =
///   r32(alpha / (end − start)).
///
/// Then, either way: the four background bytes (0x08790d14) = the low byte of
/// trunc(255.0 × component); the first `width` (the picture's width, a 16-bit count) bytes
/// of the four byte lines (0x08790b40 …) are set to them; the background's depth and its
/// sixth float become −100000. Returns the four bytes. Class: model.
pub fn init_fade(glb: &mut crate::shade::ShadeGlobals, fade: &mut Fade, blank: crate::types::Rgbazo, width: i16, lines: [&mut [u8]; 4]) -> [u8; 4] {
    if glb.fade == 0 {
        glb.background = blank;
    } else {
        if x(FADE_LEAST).gt(x(fade.end) - fade.start) {
            fade.end = (x(fade.start) + x(FADE_LEAST)).to_f32();
        }
        let b = &mut glb.background;
        b.a = (F80::ONE - fade.amount).to_f32();
        for c in [&mut b.r, &mut b.g, &mut b.b, &mut b.a] {
            if x(0.0f32).gt(x(*c)) {
                *c = 0.0;
            } else if x(*c).gt(F80::ONE) {
                *c = 1.0;
            }
        }
        glb.fade_end = fmov(fade.end);
        glb.fade_start = fmov(fade.start);
        glb.fade_scale = (x(b.a) / (x(fade.end) - fade.start)).to_f32();
    }
    let b = &mut glb.background;
    let bytes = [b.r, b.g, b.b, b.a].map(crate::renutil::channel_byte);
    for (line, byte) in lines.into_iter().zip(bytes) {
        // (a line shorter than the picture is a write past it in the original)
        for p in line.iter_mut().take(width.max(0) as usize) {
            *p = byte;
        }
    }
    b.z = -100000.0;
    b.o = -100000.0;
    bytes
}

#[cfg(test)]
mod tests {
    #[test]
    fn fade_sets_the_background_and_its_bytes() {
        use crate::shade::ShadeGlobals;
        use crate::types::Rgbazo;
        let blank = Rgbazo { z: -100000.0, o: -100000.0, ..Rgbazo::default() };
        let mut glb = ShadeGlobals { fade: 1, background: Rgbazo { r: 1.5, g: 0.5, b: -1.0, a: 9.0, z: 1.0, o: 2.0 }, ..ShadeGlobals::default() };
        let mut fade = Fade { start: 10.0, end: 10.0, amount: 0.25 };
        let mut lines = [[7u8; 4]; 4];
        let [a, b, c, d] = &mut lines;
        let bytes = init_fade(&mut glb, &mut fade, blank, 3, [a, b, c, d]);
        // the end is moved off the start; the colour is held to 0..1; alpha is 1 - amount
        assert_eq!(fade.end, 10.000001);
        assert_eq!(glb.background, Rgbazo { r: 1.0, g: 0.5, b: 0.0, a: 0.75, z: -100000.0, o: -100000.0 });
        assert_eq!((glb.fade_start, glb.fade_end), (10.0, 10.000001));
        assert_eq!(glb.fade_scale, 0.75 / (10.000001f32 - 10.0));
        assert_eq!(bytes, [255, 127, 0, 191]);
        assert_eq!(lines[1], [127, 127, 127, 7]);
        // fade off: the background is the blank sample
        glb.fade = 0;
        let [a, b, c, d] = &mut lines;
        assert_eq!(init_fade(&mut glb, &mut fade, blank, 0, [a, b, c, d]), [0, 0, 0, 0]);
        assert_eq!(glb.background, blank);
    }

    use super::*;

    #[test]
    fn lens_formulas() {
        // f = 50, N = 2, c = 0.05: H = 2500 / 0.1 (the float 0.05 doubled is not 0.1 exactly)
        let h = find_hyperfocal(50.0, 2.0, 0.5);
        assert_eq!(h.to_f32(), 2500.0);
        assert_eq!(find_near(2500.0, 500.0, 50.0).to_f32(), 1250000.0 / 2950.0);
        assert_eq!(find_far(2500.0, 500.0, 50.0).to_f32(), 1250000.0 / 1950.0);
        // beyond the hyperfocal distance the far limit is negative
        assert!(find_far(100.0, 500.0, 50.0).to_f32() < 0.0);
        // the hyperfocal value is rounded to float between the two calls, and the result is
        // not: a third stays a third to 64 bits
        let o = find_outo_focus(1.0, 1.0, 0.0, 1.0);
        assert_eq!(o.to_f32(), 0.0);
    }

    #[test]
    fn coc_search_stops_where_the_far_limit_turns() {
        // far > 0 iff H > s + f, H = f² / (N c): with f = 1, N = 1, s = 9: c < 0.1 always
        // positive up to c = 0.1: the first pass runs out at 0.1; the later passes refine
        // towards the limit from 0.01 below
        let c = find_coc(1.0, 1.0, 9.0);
        assert!(c > 0.09 && c < 0.101, "{c}");
        // with s = 99 the far limit turns at c = 0.01
        let c = find_coc(1.0, 1.0, 99.0);
        assert!((c - 0.01).abs() < 2e-4, "{c}");
        // nothing is ever sharp to infinity: the search ends at once and the result is small
        let c = find_coc(1.0, 1.0e9, 99.0);
        assert!(c < 0.02, "{c}");
    }

    #[test]
    fn cone_filter() {
        let mut rows = vec![vec![9.0f32; 3]; 3];
        make_filter(&mut rows, 1.0, 3);
        // r² = 1.5: centre 1, edges (1.5 − 1) / 1.5, corners 0 (2 is not below 1.5)
        assert_eq!(rows[1][1], 1.0);
        assert_eq!(rows[0][1], (x(0.5f32) / 1.5f32).to_f32());
        assert_eq!(rows[0][0], 0.0);
        // size 4 has the same half width as size 5
        let mut rows = vec![vec![0.0f32; 5]; 5];
        make_filter(&mut rows, 2.0, 4);
        assert_eq!(rows[2][2], 1.0);
        assert_eq!(rows[0][2], (x(0.5f32) / 4.5f32).to_f32());
    }
}

/// The members of `SI_Camera` (0x13c bytes; `SI_image` copies the scene's camera to the
/// start of `SI_database`, 0x08790640) that set-up reads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Camera {
    /// +0x0a: the kind of camera; anything but 0 is refused.
    pub kind: i16,
    /// +0x10: position.
    pub position: Vector,
    /// +0x1c: interest (the point looked at).
    pub interest: Vector,
    /// +0x30: roll, radians.
    pub roll: f32,
    /// +0x34, +0x38: near and far distance.
    pub near: f32,
    pub far: f32,
    /// +0x40: field of view, radians (the whole vertical angle).
    pub fov: f32,
    /// +0x6c: depth of field mode, 0 for none.
    pub dof: i32,
    /// What `si_initDof` reads: +0x70 the focal length (millimetres) and +0x74 the focus
    /// distance (feet) of mode 1; +0x7c near, +0x84 far and +0x8c the out-of-focus distance
    /// of the other modes; +0x94 the aperture number; +0x98 the circle of confusion.
    pub dof_focal: f32,
    pub dof_focus: f32,
    pub dof_near: f32,
    pub dof_far: f32,
    pub dof_beyond: f32,
    pub dof_aperture: f32,
    pub dof_circle: f32,
    /// +0x108: an off-axis window is given, and +0x10c its five floats: distance, width,
    /// height, x offset, y offset.
    pub window: i16,
    pub window_v: [f32; 5],
}

/// 0x0869ce60 and 0x0869cea0: the two texts `SI_camera` hands to `si_fatal` (each with an
/// empty second text).
pub fn msg_camera_kind() -> &'static str {
    si_rt::text(0x0869_ce60)
}
pub fn msg_camera_direction() -> &'static str {
    si_rt::text(0x0869_cea0)
}

/// The ray directions and the camera's matrices [soft 0x08272450 SI_camera]: `SI_camera()`.
///
/// `si_fatal` does not return (it shows the text and leaves the render by `longjmp`); here
/// the text is the error, and what the routine had stored by then stays stored.
///
/// 1. A camera of a kind other than 0: fatal ([`MSG_CAMERA_KIND`]).
/// 2. Half the view plane's width w and height h at distance 1 (as the code uses them: the
///    plane is at z = −0.5): from the field of view, h = `tanf`(r32(fov × 0.5)) as the
///    library returns it and w = h × ((width / height) × pixel ratio), neither rounded; or
///    from the window, w = r32((width / distance) × 0.5), h = r32(0.5 × (height /
///    distance)), and the offsets ox = r32(x offset / distance), oy = r32(y offset /
///    distance) (0 without a window).
/// 3. Steps per sample: dx = w / ((x max + 1) × L), dy = h / (L × (1 + y max)), stored
///    rounded and used unrounded. First direction: x = r32((dx + (−w + ox)) × 0.5), y =
///    r32(0.5 × ((−h + oy) + dy)), z = −0.5. With field rendering y = r32(y ± dy × 0.25),
///    plus for field 0, on the already rounded y.
/// 4. The perspective matrix (0x087908ac): identity with [0][0] = r32(1 / w), [1][1] =
///    r32(1 / h), [2][2] = r32(−(far + near) × k), [2][3] = −1, [3][2] = r32(((far × −2) ×
///    near) × k), [3][3] = 0, k = r32(1 / (far − near)); with a window [2][0] = r32((x
///    offset + x offset) / width), [2][1] = r32((y offset + y offset) / height). It is then
///    multiplied by the viewport matrix: identity with [0][0] = sx = r32(((x max + 1) × L)
///    × 0.5), [1][1] = sy = r32((L × (1 + y max)) × 0.5), [3][0] = r32(sx − 0.5), [3][1] =
///    r32(sy − 0.5), the last ∓0.25 by field (minus for field 0), and with depth of field
///    both plus half the blur line count.
/// 5. The view matrix (0x0879082c): identity, `si_cameraRotate(3, −roll)`, then with d =
///    interest − position (rounded), len = |d| and lxz = r32(√(dz² + dx²)):
///    `si_cameraRotate(1, −dy / len, lxz / len)`; a len of 0 is fatal
///    ([`MSG_CAMERA_DIRECTION`]); if lxz > 0, `si_cameraRotate(2, dx / lxz, −dz / lxz)`;
///    `si_cameraTranslate(−position)`. Its inverse goes to 0x087907ec (left alone when the
///    matrix has none).
///
/// Class: libm (`tanf`, and `cosf`, `sinf` inside `si_cameraRotate(3, …)`).
pub fn camera(cam: &Camera, s: &Settings, v: &mut View, rc: &mut RenderCamera) -> Result<(), &'static str> {
    if cam.kind != 0 {
        return Err(msg_camera_kind());
    }
    let half = x(0.5f64);
    let aspect = (x(s.width as i32) / x(s.height as i32)) * s.pixel_ratio;
    let (w, h, ox, oy);
    if cam.window != 0 {
        let [dist, ww, wh, wx, wy] = cam.window_v;
        w = x(((x(ww) / dist) * half).to_f32());
        ox = (x(wx) / dist).to_f32();
        h = x((half * (x(wh) / dist)).to_f32());
        oy = (x(wy) / dist).to_f32();
    } else {
        h = trig::tanf((x(cam.fov) * half).to_f32());
        w = h * aspect;
        ox = 0.0;
        oy = 0.0;
    }
    let l = x(s.level as i32);
    let one = F80::ONE;
    let dx = w / ((x(v.picture[1]) + one) * l);
    let dy = h / (l * (one + v.picture[3]));
    v.dir[3] = dx.to_f32();
    v.dir[4] = dy.to_f32();
    v.dir[0] = ((dx + (-w + ox)) * half).to_f32();
    let y = (half * ((-h + oy) + dy)).to_f32();
    v.dir[1] = y;
    v.dir[2] = -0.5;
    if s.field != 0 {
        v.dir[1] = if s.field_no == 0 { (dy * 0.25f64 + y).to_f32() } else { (x(y) - dy * 0.25f64).to_f32() };
    }
    let k = (one / (x(cam.far) - cam.near)).to_f32();
    let mut p = ident();
    p[0][0] = (one / w).to_f32();
    p[1][1] = (one / h).to_f32();
    p[2][2] = (-(x(cam.far) + cam.near) * k).to_f32();
    p[2][3] = -1.0;
    p[3][3] = 0.0;
    p[3][2] = (((x(cam.far) * -2.0f64) * cam.near) * k).to_f32();
    if cam.window != 0 {
        let [_, ww, wh, wx, wy] = cam.window_v;
        p[2][0] = ((x(wx) + wx) / ww).to_f32();
        p[2][1] = ((x(wy) + wy) / wh).to_f32();
    }
    rc.prsp_mat = p;
    let mut vp = ident();
    let sx = (((x(v.picture[1]) + one) * l) * half).to_f32();
    let sy = ((l * (one + v.picture[3])) * half).to_f32();
    vp[0][0] = sx;
    vp[1][1] = sy;
    let mut ty = (x(sy) - half).to_f32();
    vp[3][0] = (x(sx) - half).to_f32();
    if s.field != 0 {
        ty = if s.field_no == 0 { (x(ty) - 0.25f64).to_f32() } else { (x(ty) + 0.25f64).to_f32() };
    }
    vp[3][1] = ty;
    if cam.dof != 0 {
        let n = x(v.dof_half);
        vp[3][0] = (x(vp[3][0]) + n).to_f32();
        vp[3][1] = (n + vp[3][1]).to_f32();
    }
    rc.prsp_mat = mmul(&rc.prsp_mat, &vp);
    let d = [(x(cam.interest.x) - cam.position.x).to_f32(), (x(cam.interest.y) - cam.position.y).to_f32(), (x(cam.interest.z) - cam.position.z).to_f32()];
    rc.raw_to_world_mat = ident();
    rc.camera_rotate(3, (-x(cam.roll)).to_f32(), 0.0);
    let (xx, yy, zz) = (x(d[0]) * d[0], x(d[1]) * d[1], x(d[2]) * d[2]);
    let len = ((yy + xx) + zz).sqrt();
    let lxz = (zz + xx).sqrt().to_f32();
    if x(0.0f32).eq(len) {
        return Err(msg_camera_direction());
    }
    rc.camera_rotate(1, (-x(d[1]) / len).to_f32(), (x(lxz) / len).to_f32());
    if x(0.0f32).lt(x(lxz)) {
        rc.camera_rotate(2, (x(d[0]) / lxz).to_f32(), (-x(d[2]) / lxz).to_f32());
    }
    rc.camera_translate((-x(cam.position.x)).to_f32(), (-x(cam.position.y)).to_f32(), (-x(cam.position.z)).to_f32());
    if let Some(inv) = invert(&rc.raw_to_world_mat) {
        rc.world_to_raw_mat = inv;
    }
    Ok(())
}
