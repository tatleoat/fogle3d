// SPDX-License-Identifier: MIT
//! `renu_subregion.c` on the memory image: the routines that work on the two rectangles the
//! sub-region dialogue keeps at 0x0879eb00 (the picture's box) and 0x0879eb08 (the box it
//! may be dragged in), and the dragging loops (finding 0018, section "Render set-up: the
//! display side"). The store-based arithmetic of the older `crate::subregion` module is the
//! same logic for the controls' fractions.
//!
//! **Compared with the emulated original**: the routines named in the doc comments as
//! checked were run against the original stepped through in the emulation harness
//! (`re/tools/si-rensetup/emu_gen_pass6.py`, `rensetup-emucheck`).

use super::globals::*;
use super::rec::ctl;
use si_mem::Mem;
use si_x87::x87::{Round, F80};

/// The picture's box (four 16-bit words, x0 x1 y0 y1) and the box it is clipped to.
pub const IMAGE_BOUNDS: u32 = 0x0879eb00;
pub const CLIP_BOUNDS: u32 = 0x0879eb08;

/// Holds the box at `r` inside the bounds `b` (two words: x0 x1, y0 y1 as 16-bit pairs)
/// [soft 0x082cb468 clipBox]: each x is raised to the bounds' left then lowered to their
/// right, each y likewise. `b` is passed by value in the original (the eight bytes as two
/// words).
pub fn clip_box(m: &mut Mem, r: u32, b: [u32; 2]) {
    let bx0 = b[0] as u16 as i16;
    let bx1 = (b[0] >> 16) as u16 as i16;
    let by0 = b[1] as u16 as i16;
    let by1 = (b[1] >> 16) as u16 as i16;
    for off in [0, 2] {
        if m.i16(r + off) < bx0 {
            m.set_i16(r + off, bx0);
        }
    }
    for off in [0, 2] {
        if bx1 < m.i16(r + off) {
            m.set_i16(r + off, bx1);
        }
    }
    for off in [4, 6] {
        if m.i16(r + off) < by0 {
            m.set_i16(r + off, by0);
        }
    }
    for off in [4, 6] {
        if by1 < m.i16(r + off) {
            m.set_i16(r + off, by1);
        }
    }
}

/// Sets the two rectangles from a window's grafport [soft 0x082ca448 computeBounds].
///
/// Both start as the grafport's box (its 16-bit words at +0x1c, +0x1e, +0x20, +0x22). When
/// the picture has a height, the "height from ratio" word (`SI_defaultControl` +0xfe) is
/// 0 and the picture's width over its height differs from the camera's ratio (the float at
/// the grafport's camera, +0x64), the picture's box is narrowed to that ratio about the
/// middle of the window: with s = (x0 + x1) × 0.5 held as a float and d = ((w / h) × (y1 −
/// y0)) × 0.5, the new left is s − d + 0.5 and the new right 0.5 + (d + s), each cut
/// towards zero to 32 bits and kept as 16; the picture's box is then clipped to the
/// window's box by `clipBox`, the result going to the second rectangle.
pub fn compute_bounds(m: &mut Mem, gp: u32) {
    let cam = m.u32(gp + 0x2c);
    let dc = SI_DEFAULT_CONTROL.0;
    let height = m.u16(dc + ctl::HEIGHT);
    for (i, off) in [0x1c, 0x1e, 0x20, 0x22].into_iter().enumerate() {
        let v = m.u16(gp + off);
        m.set_u16(CLIP_BOUNDS + 2 * i as u32, v);
        m.set_u16(IMAGE_BOUNDS + 2 * i as u32, v);
    }
    if height == 0 || m.u16(dc + ctl::HEIGHT_FROM_RATIO) != 0 {
        return;
    }
    let width = m.u16(dc + ctl::WIDTH);
    let ratio = F80::from_i32(width as i16 as i32) / F80::from_i32(height as i16 as i32);
    let camera = F80::from_f32(m.f32(cam + 0x64));
    if ratio.compare(camera) == Some(core::cmp::Ordering::Equal) {
        return;
    }
    let sum = m.i16(gp + 0x1c) as i32 + m.i16(gp + 0x1e) as i32;
    let s = (F80::from_i32(sum) * 0.5f64).to_f32();
    let dy = m.i16(gp + 0x22) as i32 - m.i16(gp + 0x20) as i32;
    let sx = F80::from_f32(s);
    let left = (sx - (ratio * F80::from_i32(dy)) * 0.5f64) + 0.5f64;
    m.set_u16(IMAGE_BOUNDS, left.to_i32(Round::Zero) as u16);
    let right = F80::from_f64(0.5) + ((ratio * F80::from_i32(dy)) * 0.5f64 + sx);
    m.set_u16(IMAGE_BOUNDS + 2, right.to_i32(Round::Zero) as u16);
    let local = m.local(8);
    let (a, b) = (m.u32(IMAGE_BOUNDS), m.u32(IMAGE_BOUNDS + 4));
    m.set_u32(local, a);
    m.set_u32(local + 4, b);
    let (c0, c1) = (m.u32(CLIP_BOUNDS), m.u32(CLIP_BOUNDS + 4));
    clip_box(m, local, [c0, c1]);
    let (a, b) = (m.u32(local), m.u32(local + 4));
    m.set_u32(CLIP_BOUNDS, a);
    m.set_u32(CLIP_BOUNDS + 4, b);
    m.release(8);
}

// ---------------------------------------------------------------------------------------------
// The question itself: `SI_doSubregion`. The window system's side (the IRIS GL calls, the status
// line, the cursor, the event queue) and the three dragging loops behind `subregionEventHandler`
// are not ported: they are reached by address through the host ([`Outside`]) and answered
// there, each in the order the original makes the calls.

use super::outside::Outside;
use si_mem::FnPtr;

/// The globals of `renu_subregion.c` and the ones it reads.
pub const PERS_WIN: u32 = 0x086a6b08;
pub const SI_MDL_PERS: u32 = 0x088177bc;
pub const XO: u32 = 0x0879e6e0;
pub const YO: u32 = 0x0879e6e4;
pub const SUBREGION_MOVED: u32 = 0x0879e6e8;
pub const SUBREGION_XRESIZED: u32 = 0x0879e6ea;
pub const SUBREGION_YRESIZED: u32 = 0x0879e6ec;
const SI_TABLET_ACTIVE: u32 = 0x086793f8;
const SI_DIALOG_LIGHT: u32 = 0x08781420;
const SI_XMAXSCREEN_AT: u32 = 0x08781794;
const SI_YMAXSCREEN_AT: u32 = 0x08781796;
/// The three messages of the status line while the box is dragged (move/resize, accept, cancel) and the
/// three empty ones that take them away.
const STATUS_DRAG: [u32; 3] = [0x086a6b2e, 0x086a6b1d, 0x086a6b0c];
const STATUS_CLEAR: [u32; 3] = [0x086a6b46, 0x086a6b45, 0x086a6b44];

macro_rules! host {
    ( $( $(#[$meta:meta])* $id:ident = $addr:literal, $name:literal; )* ) => {
        $( $(#[$meta])* pub const $id: FnPtr = FnPtr::new($addr); )*
        /// Address and name of every routine this module reaches through the host.
        pub const CALLEES: &[(u32, &str)] = &[ $( ($addr, $name) ),* ];
    };
}

host! {
    /// getdrawmode (0x086116b0), IRIS GL: answers the current draw mode.
    GETDRAWMODE = 0x086116b0, "getdrawmode";
    /// SI_refreshSystem (0x08323d44), `draw.c`: (port, list, what).
    SI_REFRESH_SYSTEM = 0x08323d44, "SI_refreshSystem";
    /// SI_windowSetPerspectiveActive (0x083d906c), `window.c`.
    SI_WINDOW_SET_PERSPECTIVE_ACTIVE = 0x083d906c, "SI_windowSetPerspectiveActive";
    /// SI_windowGetNextVisible (0x083d6500): (where the window goes).
    SI_WINDOW_GET_NEXT_VISIBLE = 0x083d6500, "SI_windowGetNextVisible";
    /// SI_unZoomWindow (0x083d4e08): (window).
    SI_UN_ZOOM_WINDOW = 0x083d4e08, "SI_unZoomWindow";
    /// winget (0x08619918): the current GL window.
    WINGET = 0x08619918, "winget";
    /// winset (0x0861993c): (GL window).
    WINSET = 0x0861993c, "winset";
    /// getorigin (0x08619ba4): (where x goes, where y goes).
    GETORIGIN = 0x08619ba4, "getorigin";
    /// getmmode (0x08610350): the matrix mode.
    GETMMODE = 0x08610350, "getmmode";
    /// mmode (0x0861021c): (mode).
    MMODE = 0x0861021c, "mmode";
    /// getmatrix (0x086103f4): (where the 16 floats go).
    GETMATRIX = 0x086103f4, "getmatrix";
    /// loadmatrix (0x0861037c): (the 16 floats).
    LOADMATRIX = 0x0861037c, "loadmatrix";
    /// SI_cursorChange (0x083d1910): (cursor).
    SI_CURSOR_CHANGE = 0x083d1910, "SI_cursorChange";
    /// SI_statusDisplayMouse (0x083cc650), `status.c`: (middle, left, right button texts).
    SI_STATUS_DISPLAY_MOUSE = 0x083cc650, "SI_statusDisplayMouse";
    /// pushviewport (0x0860fdcc), popviewport (0x0860fde4), pushmatrix (0x0861052c), popmatrix (0x0861053c).
    PUSHVIEWPORT = 0x0860fdcc, "pushviewport";
    POPVIEWPORT = 0x0860fde4, "popviewport";
    PUSHMATRIX = 0x0861052c, "pushmatrix";
    POPMATRIX = 0x0861053c, "popmatrix";
    /// drawmode (0x08611608): (mode).
    DRAWMODE = 0x08611608, "drawmode";
    /// ortho2 (0x086100c8): (left, right, bottom, top) as floats.
    ORTHO2 = 0x086100c8, "ortho2";
    /// viewport (0x0860fe90): (left, right, bottom, top).
    VIEWPORT = 0x0860fe90, "viewport";
    /// getmcolor (0x0861107c): (index, where r, g, b go as 16 bits).
    GETMCOLOR = 0x0861107c, "getmcolor";
    /// mapcolor (0x08610fb8): (index, r, g, b).
    MAPCOLOR = 0x08610fb8, "mapcolor";
    /// GLOB_initSubregion (0x08614c2c), GLOB_exitSubregion (0x08614c50).
    GLOB_INIT_SUBREGION = 0x08614c2c, "GLOB_initSubregion";
    GLOB_EXIT_SUBREGION = 0x08614c50, "GLOB_exitSubregion";
    /// subregionEventHandler (0x082cad1c): (region, where the accepted word goes); answers a 16-bit "go on".
    SUBREGION_EVENT_HANDLER = 0x082cad1c, "subregionEventHandler";
    /// SI_tabletGetData (0x083cec18).
    SI_TABLET_GET_DATA = 0x083cec18, "SI_tabletGetData";
    /// qtest (0x0860e188), qread (0x0860e1bc): (where the 16-bit value goes), qreset (0x0860e12c).
    QTEST_AT = 0x0860e188, "qtest";
    QREAD_AT = 0x0860e1bc, "qread";
    QRESET = 0x0860e12c, "qreset";
}

/// The name of a routine this module reaches through the host, by its address.
pub fn name_of(f: FnPtr) -> Option<&'static str> {
    CALLEES.iter().find(|c| c.0 == f.0).map(|c| c.1)
}

/// The sub-region box of a window on the image [soft 0x082ca5b0 regionMake]: `image` and `bounds` are the addresses of the
/// two rectangles (four 16-bit words each: x0 x1 y0 y1); the fractions are those at +0x22c, +0x230, +0x234 and +0x238 of
/// `SI_defaultControl`. With w and h the picture's width and height in pixels: left = x0 + w × f(+0x22c); right = x1 −
/// w × f(+0x230); bottom = y0 + h × f(+0x238); top = y1 − h × f(+0x234), each plus 0.5, cut towards zero and kept as 16
/// bits. A box that then lies partly outside the bounds sets `subregionXresized` and is clipped. Answers a new 16-byte
/// `SI_Region` (`SI_calloc(1, 16)`): the box and the bounds.
pub fn region_make(m: &mut Mem, image: u32, bounds: u32) -> u32 {
    let dc = SI_DEFAULT_CONTROL.0;
    let w = F80::from_i32(m.i16(image + 2) as i32 - m.i16(image) as i32);
    let h = F80::from_i32(m.i16(image + 6) as i32 - m.i16(image + 4) as i32);
    let r = m.calloc(1, 0x10);
    let half = F80::from_f32(0.5);
    let f = |m: &Mem, off: u32| F80::from_f32(m.f32(dc + off));
    let cut = |v: F80| v.to_i32(Round::Zero) as i16;
    let x0 = cut(F80::from_i32(m.i16(image) as i32) + f(m, ctl::CROP_LEFT) * w + half);
    m.set_i16(r, x0);
    let x1 = cut(F80::from_i32(m.i16(image + 2) as i32) - w * f(m, ctl::CROP_RIGHT) + half);
    m.set_i16(r + 2, x1);
    let y0 = cut(F80::from_i32(m.i16(image + 4) as i32) + f(m, ctl::CROP_BOTTOM) * h + half);
    m.set_i16(r + 4, y0);
    let y1 = cut(F80::from_i32(m.i16(image + 6) as i32) - h * f(m, ctl::CROP_TOP) + half);
    m.set_i16(r + 6, y1);
    for k in 0..4 {
        let v = m.u16(bounds + 2 * k);
        m.set_u16(r + 8 + 2 * k, v);
    }
    let (a0, a1) = (m.u32(r), m.u32(r + 4));
    let (b0, b1) = (m.u32(r + 8), m.u32(r + 12));
    if !clipped_box_words(a0, a1, b0, b1) {
        m.set_i16(SUBREGION_XRESIZED, 1);
    }
    clip_box(m, r, [b0, b1]);
    r
}

/// `clippedBox` [soft 0x082cb4d0 clippedBox] on the eight bytes of each rectangle as two words.
fn clipped_box_words(a0: u32, a1: u32, b0: u32, b1: u32) -> bool {
    let s = |w: u32, k: u32| (w >> (16 * k)) as u16 as i16;
    let (ax0, ax1, ay0, ay1) = (s(a0, 0), s(a0, 1), s(a1, 0), s(a1, 1));
    let (bx0, bx1, by0, by1) = (s(b0, 0), s(b0, 1), s(b1, 0), s(b1, 1));
    bx0 <= ax0 && bx0 <= ax1 && ax0 <= bx1 && ax1 <= bx1 && by0 <= ay0 && by0 <= ay1 && ay0 <= by1 && ay1 <= by1
}

/// Lets the user drag the render's sub-region box in the perspective window
/// [soft 0x082c9e50 SI_doSubregion]; answers the 16-bit accepted word (0: cancelled).
///
/// The windows and the screen are set up for drawing over the picture (the perspective window made active and un-zoomed
/// when it is not, its GL window made current, its origin read, the status line told the three button texts, the draw
/// mode, matrix and viewport pushed, the screen made the ortho box, the dialogue colour turned red); the picture's
/// rectangles are computed ([`compute_bounds`]) and the box made from the fractions ([`region_make`]); the event
/// handler is called until it answers 0. If the box was accepted, the fractions in `SI_defaultControl`
/// (+0x22c left, +0x230 right, +0x234 top, +0x238 bottom, floats, and the pixel sizes at +0x23c and +0x23e) are made from where
/// it ended: a moved or x-resized box takes its left from the box's distance to the picture's left over the picture's
/// width and its right from the width left (resized: the distance to the picture's right over the width); the same
/// for y; the pixel sizes are what the fractions leave of the picture's width and height, plus a half, cut towards zero,
/// when the box was resized in that direction, else the values taken from the fractions before the drag. Everything is computed in the
/// 80-bit register; the quotient that is stored as a float is also what the second fraction takes. The state is then
/// put back and the event queue is read until a release of the left button (device 0x65 or 0x66 with value 0).
pub fn do_subregion<O: Outside + ?Sized>(o: &mut O, m: &mut Mem) -> i32 {
    let dc = SI_DEFAULT_CONTROL.0;
    let draw_mode = o.call(m, GETDRAWMODE, &[]);
    let accepted = m.local(4);
    m.set_i32(accepted, 1);
    let mut mm = 0u32;
    o.call(m, SI_REFRESH_SYSTEM, &[0, 0, 0x20]);
    let pers = m.u32(SI_MDL_PERS);
    m.set_u32(PERS_WIN, pers);
    let cam = m.u32(pers + 0x2c);
    if m.i16(cam + 4) != 0x7d3 || m.i16(pers + 6) == 0 {
        o.call(m, SI_WINDOW_SET_PERSPECTIVE_ACTIVE, &[]);
        o.call(m, SI_REFRESH_SYSTEM, &[0, 0, 0x3f]);
    }
    let pers = m.u32(PERS_WIN);
    m.set_u32(YO, 0);
    m.set_u32(XO, 0);
    if m.i16(pers + 6) == 0 {
        let win = m.local(4);
        m.set_u32(win, 0);
        o.call(m, SI_WINDOW_GET_NEXT_VISIBLE, &[win]);
        let w = m.u32(win);
        o.call(m, SI_UN_ZOOM_WINDOW, &[w]);
        o.call(m, SI_REFRESH_SYSTEM, &[0, 0, 0x1f]);
        m.release(4);
    }
    let matrix = m.local(64);
    let saved_win;
    let pers = m.u32(PERS_WIN);
    if m.u32(pers + 0x50) != u32::MAX {
        saved_win = o.call(m, WINGET, &[]);
        let gl = m.u32(pers + 0x50);
        o.call(m, WINSET, &[gl]);
        o.call(m, GETORIGIN, &[XO, YO]);
        mm = o.call(m, GETMMODE, &[]);
        if mm != 0 {
            o.call(m, MMODE, &[0]);
            o.call(m, GETMATRIX, &[matrix]);
        }
        o.call(m, SI_CURSOR_CHANGE, &[0]);
    } else {
        saved_win = 0;
    }
    o.call(m, SI_STATUS_DISPLAY_MOUSE, &STATUS_DRAG);
    o.call(m, PUSHVIEWPORT, &[]);
    o.call(m, PUSHMATRIX, &[]);
    o.call(m, DRAWMODE, &[0x20]);
    let xmax = m.i16(SI_XMAXSCREEN_AT) as i32;
    let ymax = m.i16(SI_YMAXSCREEN_AT) as i32;
    let half = F80::from_f64(0.5);
    let top = (F80::from_i32(ymax) + half).to_f32();
    let right = (F80::from_i32(xmax) + half).to_f32();
    o.call(m, ORTHO2, &[(-0.5f32).to_bits(), right.to_bits(), (-0.5f32).to_bits(), top.to_bits()]);
    o.call(m, VIEWPORT, &[0, xmax as u32, 0, ymax as u32]);
    let light = m.u16(SI_DIALOG_LIGHT) as u32;
    let rgb = m.local(6);
    o.call(m, GETMCOLOR, &[light, rgb, rgb + 2, rgb + 4]);
    o.call(m, MAPCOLOR, &[light, 0xe6, 10, 10]);
    m.set_i16(SUBREGION_MOVED, 0);
    m.set_i16(SUBREGION_XRESIZED, 0);
    m.set_i16(SUBREGION_YRESIZED, 0);
    let one = F80::ONE;
    let fl = |m: &Mem, off: u32| F80::from_f32(m.f32(dc + off));
    let width = F80::from_i32(m.i16(dc + ctl::WIDTH) as i32);
    let height = F80::from_i32(m.i16(dc + ctl::HEIGHT) as i32);
    // the part of the picture the box leaves, as floats, and the box's pixel sizes before the drag
    let f_x = ((one - fl(m, ctl::CROP_RIGHT)) - fl(m, ctl::CROP_LEFT)).to_f32();
    let before_x = (width * F80::from_f32(f_x) + half).to_i32(Round::Zero);
    let f_y = ((one - fl(m, ctl::CROP_TOP)) - fl(m, ctl::CROP_BOTTOM)).to_f32();
    let before_y = (height * F80::from_f32(f_y) + half).to_i32(Round::Zero);
    compute_bounds(m, m.u32(PERS_WIN));
    let img_w = m.i16(IMAGE_BOUNDS + 2) as i32 - m.i16(IMAGE_BOUNDS) as i32 + 1;
    let img_h = m.i16(IMAGE_BOUNDS + 6) as i32 - m.i16(IMAGE_BOUNDS + 4) as i32 + 1;
    let region = region_make(m, IMAGE_BOUNDS, CLIP_BOUNDS);
    let pers = m.u32(PERS_WIN);
    let (px0, py0) = (m.i16(pers + 0x1c) as i32, m.i16(pers + 0x20) as i32);
    let pw = m.i16(pers + 0x1e) as i32 - px0 + 1;
    let ph = m.i16(pers + 0x22) as i32 - py0 + 1;
    if m.u32(pers + 0x50) == u32::MAX {
        o.call(m, GLOB_INIT_SUBREGION, &[px0 as u32, py0 as u32, pw as u32, ph as u32]);
    } else {
        o.call(m, GLOB_INIT_SUBREGION, &[0, 0, pw as u32, ph as u32]);
    }
    while o.call(m, SUBREGION_EVENT_HANDLER, &[region, accepted]) as u16 != 0 {}
    o.call(m, GLOB_EXIT_SUBREGION, &[]);
    if m.i32(accepted) != 0 {
        let (moved, xres, yres) = (m.i16(SUBREGION_MOVED) != 0, m.i16(SUBREGION_XRESIZED) != 0, m.i16(SUBREGION_YRESIZED) != 0);
        if moved || xres {
            let q = F80::from_i32(m.i16(region) as i32 - m.i16(IMAGE_BOUNDS) as i32) / F80::from_i32(img_w);
            m.set_f32(dc + ctl::CROP_LEFT, q.to_f32());
            let right = if xres {
                F80::from_i32(m.i16(IMAGE_BOUNDS + 2) as i32 - m.i16(region + 2) as i32) / F80::from_i32(img_w)
            } else {
                (one - F80::from_f32(f_x)) - q
            };
            m.set_f32(dc + ctl::CROP_RIGHT, right.to_f32());
        }
        if moved || yres {
            let q = F80::from_i32(m.i16(region + 4) as i32 - m.i16(IMAGE_BOUNDS + 4) as i32) / F80::from_i32(img_h);
            m.set_f32(dc + ctl::CROP_BOTTOM, q.to_f32());
            let top = if yres {
                F80::from_i32(m.i16(IMAGE_BOUNDS + 6) as i32 - m.i16(region + 6) as i32) / F80::from_i32(img_h)
            } else {
                (one - F80::from_f32(f_y)) - q
            };
            m.set_f32(dc + ctl::CROP_TOP, top.to_f32());
        }
        let w16 = F80::from_i32(m.i16(dc + ctl::WIDTH) as i32);
        let h16 = F80::from_i32(m.i16(dc + ctl::HEIGHT) as i32);
        let pix_w = if xres { ((w16 * ((one - fl(m, ctl::CROP_RIGHT)) - fl(m, ctl::CROP_LEFT))) + half).to_i32(Round::Zero) } else { before_x };
        m.set_i16(dc + ctl::CROP_WIDTH, pix_w as i16);
        let pix_h = if yres { ((h16 * ((one - fl(m, ctl::CROP_TOP)) - fl(m, ctl::CROP_BOTTOM))) + half).to_i32(Round::Zero) } else { before_y };
        m.set_i16(dc + ctl::CROP_HEIGHT, pix_h as i16);
    }
    o.call(m, POPVIEWPORT, &[]);
    o.call(m, POPMATRIX, &[]);
    let (r, g, b) = (m.i16(rgb) as i32 as u32, m.i16(rgb + 2) as i32 as u32, m.i16(rgb + 4) as i32 as u32);
    o.call(m, MAPCOLOR, &[light, r, g, b]);
    o.call(m, DRAWMODE, &[draw_mode]);
    if region != 0 {
        m.free(region);
    }
    if mm != 0 {
        o.call(m, MMODE, &[mm as i16 as i32 as u32]);
        o.call(m, LOADMATRIX, &[matrix]);
    }
    if m.u32(m.u32(PERS_WIN) + 0x50) != u32::MAX {
        o.call(m, WINSET, &[saved_win]);
    }
    o.call(m, SI_STATUS_DISPLAY_MOUSE, &STATUS_CLEAR);
    let value = m.local(2);
    loop {
        if m.i32(SI_TABLET_ACTIVE) != 0 {
            while o.call(m, QTEST_AT, &[]) == 0 {
                o.call(m, SI_TABLET_GET_DATA, &[]);
            }
        }
        let device = o.call(m, QREAD_AT, &[value]);
        if m.i16(value) == 0 && device.wrapping_sub(0x65) <= 1 {
            break;
        }
    }
    o.call(m, QRESET, &[]);
    let answer = m.i16(accepted) as i32 as u32;
    m.release(4 + 64 + 8 + 4);
    answer as i32
}
