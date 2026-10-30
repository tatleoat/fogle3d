// SPDX-License-Identifier: MIT
//! The picture on the screen while it is rendered: the original's `fbuffer.c` (0x08273560
//! to 0x0827390a, two functions; finding 0012, section 16). Ported in place, on the memory
//! image; the GL layer's calls and `SI_chunkMalloc (0x0857cb64)` go through [`Outside`].

use crate::img::{f, g, Outside};
use si_mem::Mem;

/// The picture's width in pixels, or the crop's when the crop word (0x08790e04, tested as
/// 32 bits here) is set.
fn line_width(m: &Mem) -> i16 {
    if m.i32(g::CROP) != 0 {
        m.i16(g::CROP_WIDTH)
    } else {
        m.i16(g::WIDTH)
    }
}

/// The three background lines of the display [soft 0x08273560 si_fbInit]: `si_fbInit()`.
///
/// The word at 0x08791218 becomes three times the line's width (the crop's width with a
/// crop, else the picture's). Three lines of that width are taken from the renderer's chunk
/// memory and their addresses stored in `redNull`, `greenNull`, `blueNull`; every byte of
/// them is set to the background's red, green and blue byte (0x08790d14 … 0x08790d16). No
/// routine of the files read uses the three lines afterwards. Class: copy.
pub fn fb_init(m: &mut Mem, env: &mut dyn Outside) {
    let width = line_width(m) as i32;
    m.set_i32(g::FB_WIDTH3, width.wrapping_mul(3));
    for line in [g::RED_NULL, g::GREEN_NULL, g::BLUE_NULL] {
        let chunk = m.u32(g::SI_REN_CHUNK);
        let width = line_width(m) as i32 as u32;
        let p = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, width]);
        m.set_u32(line, p);
    }
    let (red, green, blue) = (m.u32(g::RED_NULL), m.u32(g::GREEN_NULL), m.u32(g::BLUE_NULL));
    let mut i: i32 = 0;
    while i < line_width(m) as i32 {
        let k = i as u32;
        let (r, gr, b) = (m.u8(g::BACKGROUND_BYTES), m.u8(g::BACKGROUND_BYTES + 1), m.u8(g::BACKGROUND_BYTES + 2));
        m.set_u8(red.wrapping_add(k), r);
        m.set_u8(green.wrapping_add(k), gr);
        m.set_u8(blue.wrapping_add(k), b);
        i += 1;
    }
}

/// One line of the picture to the screen [soft 0x082736d0 SI_fbWrite]: `SI_fbWrite(y)`.
///
/// `y` is doubled with field rendering. The outgoing red, green and blue byte lines
/// (0x08790b54 …) are packed into a block of 4 × width bytes from `SI_malloc` (red, green,
/// blue, 0xff per pixel; the width is the crop's with a crop). `getorigin (0x08619ba4)`
/// gives the window's origin; the line is drawn with `lrectwrite (0x08611a50)` from
/// (window x − origin x, window y − origin y + y) to (… + width − 1, the same line), all in
/// sixteen bits, and the block freed. For a line above the second, the two lines below it
/// are painted black: `RGBcolor (0x086111a4)`(0, 0, 0) and `sboxs (0x08612158)`(window x,
/// window y + y − 2, window x + width − 1, window y + y − 1). Class: copy.
pub fn fb_write(m: &mut Mem, env: &mut dyn Outside, y: u32) {
    let mut y = y;
    if m.i16(g::FIELDS) != 0 {
        y = y.wrapping_add(y);
    }
    let width = line_width(m);
    let block = m.si_malloc((width as i32 as u32) << 2);
    let mut i: i32 = 0;
    while i < line_width(m) as i32 {
        let k = i as u32;
        let (r, gr, b) = (m.u8(m.u32(g::OUT_RED).wrapping_add(k)), m.u8(m.u32(g::OUT_GREEN).wrapping_add(k)), m.u8(m.u32(g::OUT_BLUE).wrapping_add(k)));
        let at = block.wrapping_add(4 * k);
        m.set_u8(at, r);
        m.set_u8(at.wrapping_add(1), gr);
        m.set_u8(at.wrapping_add(2), b);
        m.set_u8(at.wrapping_add(3), 0xff);
        i += 1;
    }
    let sp = m.sp();
    let oy = m.local(4);
    let ox = m.local(4);
    env.call(m, f::GETORIGIN, &[ox, oy]);
    // the origin's words are read whole; only sixteen bits of each sum are kept
    let (ox, oy) = (m.u32(ox), m.u32(oy));
    m.set_sp(sp);
    let line = (m.u16(g::WINDOW_Y) as u32).wrapping_sub(oy).wrapping_add(y) as i16 as i32 as u32;
    let left = (m.u16(g::WINDOW_X) as u32).wrapping_sub(ox);
    let right = (left as u16).wrapping_add(line_width(m) as u16).wrapping_sub(1) as i16 as i32 as u32;
    let left = left as i16 as i32 as u32;
    env.call(m, f::LRECTWRITE, &[left, line, right, line, block]);
    if block != 0 {
        m.free(block);
    }
    if y as i16 > 2 {
        env.call(m, f::RGBCOLOR, &[0, 0, 0]);
        let base = (m.u16(g::WINDOW_Y) as u32).wrapping_add(y);
        let top = base.wrapping_sub(1) as i16 as i32 as u32;
        let right = (line_width(m) as u16).wrapping_add(m.u16(g::WINDOW_X)).wrapping_sub(1) as i16 as i32 as u32;
        let bottom = base.wrapping_sub(2) as i16 as i32 as u32;
        let left = m.i16(g::WINDOW_X) as i32 as u32;
        env.call(m, f::SBOXS, &[left, bottom, right, top]);
    }
}
