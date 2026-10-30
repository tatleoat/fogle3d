// SPDX-License-Identifier: MIT
//! `pre_render.c`: the frame a rendering is drawn into and the test for the user's abort
//! (all three functions of the file; finding 0018, section "Render set-up: the display side").
//!
//! **Compared with the emulated original**: each routine here was run against the
//! original stepped through in the emulation harness, on made-up screen sizes, controls and
//! button states (`re/tools/si-rensetup/emu_gen_pass6.py`, `rensetup-emucheck`).

use super::globals::*;
use super::outside::*;
use super::rec::ctl;
use si_mem::Mem;

/// The box the render window is drawn in, centred on the screen [soft 0x0830a620 si_getRndBox]: writes `box` (four 16-bit words: left, right, bottom, top). The window's
/// place (`SI_ren` +0x174, +0x176) is half of what the screen has over the picture's size
/// (`SI_defaultControl` +0x59a, +0x59c), C's division of 32-bit signed numbers, kept as 16
/// bits; the box is the place and the place plus the size, added as 16-bit numbers.
pub fn get_rnd_box(m: &mut Mem, r#box: u32) {
    let (dc, ren) = (SI_DEFAULT_CONTROL.0, SI_REN.0);
    let x = (m[SI_XMAXSCREEN] as i32 - m.i16(dc + ctl::WIDTH) as i32) / 2;
    m.set_i16(ren + ctl::WINDOW_X, x as i16);
    let y = (m[SI_YMAXSCREEN] as i32 - m.i16(dc + ctl::HEIGHT) as i32) / 2;
    m.set_i16(ren + ctl::WINDOW_Y, y as i16);
    m.set_i16(r#box, m.i16(ren + ctl::WINDOW_X));
    let right = m.u16(dc + ctl::WIDTH).wrapping_add(m.u16(ren + ctl::WINDOW_X));
    m.set_u16(r#box + 2, right);
    m.set_u16(r#box + 4, m.u16(ren + ctl::WINDOW_Y));
    let top = m.u16(dc + ctl::HEIGHT).wrapping_add(m.u16(ren + ctl::WINDOW_Y));
    m.set_u16(r#box + 6, top);
}

/// Draws the grey frame round the picture [soft 0x0830a6a0 si_drawRndBox]: colour 0x646464
/// (`cpack`), then `sboxi` (left − 1, bottom − 1, right, top) of the box (four 16-bit
/// words).
pub fn draw_rnd_box<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, r#box: u32) {
    o.call(m, CPACK, &[0x0064_6464]);
    let (x0, x1, y0, y1) = (m.i16(r#box) as i32, m.i16(r#box + 2) as i32, m.i16(r#box + 4) as i32, m.i16(r#box + 6) as i32);
    o.call(m, SBOXI, &[(x0 - 1) as u32, (y0 - 1) as u32, x1 as u32, y1 as u32]);
}

/// Whether the user is stopping the render [soft 0x0830a6d8 SI_abortion]: 1 when button 7
/// is down, or button 0x66 is down or the tablet's last button was 0x66, or all three of
/// the buttons 0x65, 0x66, 0x67 are each down or the tablet's last button; else 0. The
/// tests are made in that order and stop at the first that decides (each `getbutton` is a
/// call).
pub fn abortion<O: Outside + ?Sized>(o: &mut O, m: &mut Mem) -> u32 {
    let down = |o: &mut O, m: &mut Mem, b: u32| o.call(m, GETBUTTON, &[b]) != 0;
    if down(o, m, 7) || down(o, m, 0x66) || m[SI_TABLET_BUTTON] == 0x66 {
        return 1;
    }
    for b in [0x65, 0x66, 0x67] {
        if !(down(o, m, b) || m[SI_TABLET_BUTTON] == b as i32) {
            return 0;
        }
    }
    1
}
