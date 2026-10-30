// SPDX-License-Identifier: MIT
//! `renu_display.c`: the window the Render dialogue's command renders into (all three
//! functions of the file; finding 0023, section 20).
//!
//! **Compared with the emulated original**: each routine here was run against the
//! original stepped through in the emulation harness, on made-up controls
//! (`re/tools/si-rensetup/emu_gen_pass5.py`, `rensetup-emucheck`).

use super::globals::*;
use super::matter;
use super::outside::*;
use super::rec::ctl;
use si_mem::Mem;

/// [soft 0x082c9c00 SI_resetRenderWinOpen]: `renderWinOpen` = 0.
pub fn reset_render_win_open(m: &mut Mem) {
    m[RENDER_WIN_OPEN] = 0;
}

/// Opens the render window for a set of render controls [soft 0x082c9c14
/// SI_openRenderWindow]; answers 0, or 0x7e when nothing is shown.
///
/// 1. The crop switch is the **32-bit** word at +0x224 (the command has cleared its low
///    half, so it is the sub-region switch +0x226). With it, the sub-region box is turned
///    into pixels (`SI_updateSubregionValues`, [`matter::update_subregion_values`]) and the
///    window's size is the box's (+0x23c, +0x23e); without, the picture's (+0x59a, +0x59c).
/// 2. With the display switch (+0x178, 16 bits) set and the size wider than
///    `SI_xmaxscreen` + 1 or taller than `SI_ymaxscreen` + 1: the answer becomes 0x7e,
///    the display switch is cleared and +0x1ec set to 1; without a picture file (+0x17a)
///    the routine ends there with `SI_displayStatusError("Image too large for
///    screen...")`, else the status line says `Image too large for screen. Display
///    disabled.`
/// 3. With the display and picture-file switches both clear (the 32-bit word at +0x178):
///    an empty status line and 0x7e.
/// 4. Otherwise the window's place (+0x174, +0x176: half the screen less the size, C's
///    division), the picture's box (+0x142 = 0, +0x144 = width − 1, +0x146 = 0, +0x148 =
///    height − 1, all of the picture, not the crop) and the sequence word +0x9e = 2; with the display on:
///    `SI_rgbwindow()`, `RGBcolor(100, 100, 100)`, a box one pixel round the window (two
///    on the far sides when cropped, `sboxi`), and `renderWinOpen` = 1.
pub fn open_render_window<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, c: u32) -> u32 {
    let mut status = 0;
    let crop = m.u32(c + ctl::CROP) != 0;
    let (w, h) = if crop {
        matter::update_subregion_values(o, m, c);
        (m.i16(c + ctl::CROP_WIDTH) as i32, m.i16(c + ctl::CROP_HEIGHT) as i32)
    } else {
        (m.i16(c + ctl::WIDTH) as i32, m.i16(c + ctl::HEIGHT) as i32)
    };
    if m.i16(c + ctl::DISPLAY) != 0 && (w > m[SI_XMAXSCREEN] as i32 + 1 || h > m[SI_YMAXSCREEN] as i32 + 1) {
        status = 0x7e;
        m.set_i16(c + ctl::DISPLAY, 0);
        m.set_i16(c + ctl::LINE_STATISTICS, 1);
        if m.i16(c + ctl::PICTURE_FILE) == 0 {
            o.call(m, SI_DISPLAY_STATUS_ERROR, &[TOO_LARGE_FOR_SCREEN]);
            return 0x7e;
        }
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[DISPLAY_DISABLED, 2]);
    }
    if m.u32(c + ctl::DISPLAY) == 0 {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[EMPTY_86A6AEC, 2]);
        return 0x7e;
    }
    let x = (m[SI_XMAXSCREEN] as i32 - w) / 2;
    m.set_i16(c + ctl::WINDOW_X, x as i16);
    let y = (m[SI_YMAXSCREEN] as i32 - h) / 2;
    m.set_i16(c + ctl::WINDOW_Y, y as i16);
    let width = m.u16(c + ctl::WIDTH).wrapping_sub(1);
    m.set_u16(c + ctl::BOX_X1, width);
    let height = m.u16(c + ctl::HEIGHT).wrapping_sub(1);
    m.set_u16(c + ctl::BOX_X0, 0);
    m.set_u16(c + ctl::BOX_Y0, 0);
    m.set_u16(c + ctl::BOX_Y1, height);
    m.set_u16(c + ctl::SEQUENCE, 2);
    if m.i16(c + ctl::DISPLAY) != 0 {
        o.call(m, SI_RGBWINDOW, &[]);
        o.call(m, RGBCOLOR, &[100, 100, 100]);
        let (x, y) = (m.i16(c + ctl::WINDOW_X) as i32, m.i16(c + ctl::WINDOW_Y) as i32);
        let far = if crop { 2 } else { 1 };
        o.call(m, SBOXI, &[(x - 1) as u32, (y - 1) as u32, (w + far + x) as u32, (h + far + y) as u32]);
        m[RENDER_WIN_OPEN] = 1;
    }
    status
}

/// [soft 0x082c9e18 SI_closeRenderWindow]: with the window open and the display switch
/// (+0x178, 16 bits) set, `SI_menuwindow(null)`; then `renderWinOpen` = 0.
pub fn close_render_window<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, c: u32) {
    if m[RENDER_WIN_OPEN] != 0 && m.i16(c + ctl::DISPLAY) != 0 {
        o.call(m, SI_MENUWINDOW, &[0]);
    }
    m[RENDER_WIN_OPEN] = 0;
}
