// SPDX-License-Identifier: MIT
//! One routine of `matter.c` on the memory image (finding 0012, section 20): the
//! sub-region box from fractions of the picture to pixels, which the render window's
//! routine (`SI_openRenderWindow (0x082c9c14)`, `renu_display.c`) calls on the working
//! controls when the crop switch is on, before the renderer reads them. Another package's
//! file (si-matter's); ported here because the renderer's picture depends on it.

use crate::data::text;
use crate::img::{f, Outside};
use si_mem::Mem;
use si_x87::{x, Round, F80};

/// 0x086551fc (float): 0.5.
const HALF: f32 = 0.5;

/// x rounded towards zero by the two `frndint`s of the listing: a value below 0 rounded up,
/// any other (a NaN included) down.
fn towards_zero(v: F80) -> F80 {
    if v.lt(F80::ZERO) {
        v.round_int(Round::Up)
    } else {
        v.round_int(Round::Down)
    }
}

/// The sub-region box in pixels [soft 0x08292608 SI_updateSubregionValues]:
/// `SI_updateSubregionValues(controls)`.
///
/// With W and H the picture's width and height (16-bit words at +0x59a, +0x59c), each of
/// the four fractions left (+0x22c), right (+0x230), top (+0x234) and bottom (+0x238)
/// becomes float(W × f + 0.5) (H for top and bottom), rounded to a whole number towards
/// zero in the register and stored as a float. Then the sub-region's width (+0x23c) =
/// trunc((W − right) − left) and height (+0x23e) = trunc((H − top) − bottom), each the
/// low 16 bits of the 32-bit integer. When either is not above 0 (as 16-bit numbers):
/// `SI_statusDisplayMessage(" Current image resolution is (%d x %d). Using (%d x %d).",
/// 2)` with the two and the two held to at least 1 (the text is `sprintf`'s, here made on
/// the image), and the held ones are stored. Class: model.
pub fn update_subregion_values(m: &mut Mem, env: &mut dyn Outside, c: u32) {
    let w = x(m.i16(c + 0x59a) as i32);
    let h = x(m.i16(c + 0x59c) as i32);
    for (off, size) in [(0x22c, w), (0x230, w), (0x234, h), (0x238, h)] {
        let v = towards_zero((size * m.f32(c + off)) + x(HALF)).to_f32();
        m.set_f32(c + off, v);
    }
    let width = ((w - m.f32(c + 0x230)) - m.f32(c + 0x22c)).to_i32(Round::Zero) as i16;
    m.set_i16(c + 0x23c, width);
    let height = ((h - m.f32(c + 0x234)) - m.f32(c + 0x238)).to_i32(Round::Zero) as i16;
    m.set_i16(c + 0x23e, height);
    if width <= 0 || height <= 0 {
        let (si, di) = (width.max(1), height.max(1));
        let line = format!(" Current image resolution is ({width} x {height}). Using ({si} x {di}).");
        let sp = m.sp();
        let buffer = m.local(0x50);
        let bytes: Vec<u8> = line.bytes().collect();
        m.put_cstr(buffer, &bytes);
        let _ = text::RESOLUTION_USED;
        env.call(m, f::SI_STATUS_DISPLAY_MESSAGE, &[buffer, 2]);
        m.set_sp(sp);
        m.set_i16(c + 0x23c, si);
        m.set_i16(c + 0x23e, di);
    }
}
