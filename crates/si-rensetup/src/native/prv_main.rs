// SPDX-License-Identifier: MIT
//! `prv_main.c`: the picture a render leaves on the screen, saved as a picture file (finding
//! 0018, section "Render set-up: the display side").
//!
//! Ported: `screenToRle (0x0830a44c)`. Not ported, they drive the display: `SI_renderSgi`
//! (0x08309b10), `renderBlackScreenOpen` (0x0830a204), `renderBlackScreenClose`
//! (0x0830a2cc), `renderScreenOpen` (0x0830a324): windows, buffers and the GL state of the
//! render window.

use super::globals::*;
use super::outside::*;
use si_mem::Mem;
use si_picture::{write_header, write_mixed, Picture};

/// The four scan-line buffers `screenToRle` unpacks a row into (statics of `prv_main.c`,
/// 0x640 bytes each, in the order red, green, blue, alpha).
pub const SCAN_LINES: [u32; 4] = [0x087b_8f40, 0x087b_9580, 0x087b_9bc0, 0x087b_a200];
/// The texts of the five modes (a table of pointers: render, ghost-less default ...), read by index.
pub const RND_MSG: u32 = 0x086b_912c;
/// The picture's pixel ratio (a float, 0x08781954), copied into the header.
pub const PIXEL_RATIO: u32 = 0x0878_1954;

/// Saves a rectangle of the screen as a picture file, a row at a time from the top
/// [soft 0x0830a44c screenToRle]: (`w`, `h`, `mode`, `x0`, `y0`) and the file's name, which
/// is the caller's; the file's bytes are the answer.
///
/// `None` is the original's 0: the rectangle is wider than the screen (`SI_xmaxscreen` + 1)
/// or taller (`SI_ymaxscreen` + 1), and no file is made. (The original also answers 0 when
/// `SI_pictureWriteHeader` cannot open the file; a file here always opens.) `Some(bytes)`
/// is 1 with the file as written: the header [`write_header`] makes with the width and
/// height (16 bits), the ratio, 3 fields, an alpha plane and the comment of the mode
/// (the word of `rndMsg` at index 0 for mode 1, 3 for modes 2 and 3, 1 for mode 4, 2 for
/// mode 5, 4 for any other), then, for each row from `y0 + h - 1` down to `y0`, the row read with
/// `readdisplay(x0, y - 1, x0 + w, y, buffer, 3)` (all 16-bit; `w + 1` pixels of two rows
/// into a local buffer of 3200 words, the first `w` pixels of its first row used), its bytes
/// split into the red, green, blue and alpha lines and written as a mixed sequence of red,
/// green, blue and one of alpha. Before each row `SI_abortion` is asked; a stop leaves the
/// file closed with the rows so far. The picture record's spare chunk word is stale stack in
/// the original (`pad` here is 0). The plane pointers of the record the original passes are
/// stale stack too except alpha's (32, non-zero): a file's layout depends on the alpha one only.
pub fn screen_to_rle<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, w: i32, h: i32, mode: i32, x0: i32, y0: i32) -> Option<Vec<u8>> {
    if w > m[SI_XMAXSCREEN] as i32 + 1 || h > m[SI_YMAXSCREEN] as i32 + 1 {
        return None;
    }
    let slot = match mode {
        1 => 0,
        2 | 3 => 3,
        4 => 1,
        5 => 2,
        _ => 4,
    };
    let text = m.u32(RND_MSG + 4 * slot);
    let comment = if text == 0 { None } else { Some(m.cstr(text)) };
    let width = w as i16;
    let pic = Picture { width, height: h as i16, ratio: m.f32(PIXEL_RATIO), fields: 3, pad: 0, planes: Default::default() };
    let mut out = write_header(&pic, [true; 4], comment.as_deref(), false);
    let sp = m.sp();
    let buf = m.local(0x3200);
    let mut y = h.wrapping_add(y0);
    while y > y0 && abortion_free(o, m) {
        o.call(m, READDISPLAY, &[x0 as i16 as u32, (y - 1) as i16 as u32, x0.wrapping_add(w) as i16 as u32, y as i16 as u32, buf, 3]);
        y -= 1;
        for i in 0..w.max(0) as u32 {
            let px = m.u32(buf + 4 * i);
            for (k, line) in SCAN_LINES.iter().enumerate() {
                m.set_u8(line + i, (px >> (8 * k)) as u8);
            }
        }
        let n = width.max(0) as u32;
        let lines: Vec<Vec<u8>> = SCAN_LINES.iter().map(|&a| (0..n).map(|i| m.u8(a + i)).collect()).collect();
        write_mixed(&mut out, &[&lines[0], &lines[1], &lines[2]]);
        write_mixed(&mut out, &[&lines[3]]);
    }
    m.set_sp(sp);
    Some(out)
}

/// `SI_abortion` answered 0 (the loop goes on).
fn abortion_free<O: Outside + ?Sized>(o: &mut O, m: &mut Mem) -> bool {
    super::pre_render::abortion(o, m) as u16 == 0
}
