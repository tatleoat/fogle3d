// SPDX-License-Identifier: MIT
//! Abekas YUV frames (`.yuv`): 720 pixels a row, two bytes a pixel, no header (finding 0014,
//! "YUV").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `yuv.c` (0x085e5950–0x085e62a3).
//!
//! A frame is 486 rows (NTSC, 525 lines) or 576 rows (PAL, 625 lines) of 720 pixels; each
//! pair of pixels is four bytes: U, Y of the first, V, Y of the second. Which of the two a
//! file is cannot be told from the file: the reader goes by the environment variable
//! `ABEKAS` (`PAL` or `NTSC`, NTSC when unset).

use crate::effect::Fault;
use si_picture::Picture;
use si_x87::F80;

/// Pixels a row.
pub const WIDTH: usize = 720;
/// Rows of an NTSC and of a PAL frame.
pub const ROWS_NTSC: i32 = 486;
pub const ROWS_PAL: i32 = 576;
/// The `lines` argument that goes with each.
pub const LINES_NTSC: i32 = 525;
pub const LINES_PAL: i32 = 625;

/// What the environment variable `ABEKAS` says: `PAL` gives 576 rows, `NTSC` (or anything
/// else, or nothing) leaves `default`. The value is compared whole.
fn rows_by_environment(abekas: Option<&[u8]>, default: i32) -> i32 {
    match abekas {
        Some(b"PAL") => ROWS_PAL,
        Some(b"NTSC") => ROWS_NTSC,
        _ => default,
    }
}

/// Lays a picture into a YUV frame buffer [soft 0x085e5950 SI_pictureSoftToYuv]. Returns
/// false, with nothing done, for a picture without any plane.
///
/// - `lines` is 525 or 625 and gives the frame's rows (486 or 576); with any other value the
///   original uses a row offset it never set ([`Fault::Uninitialised`]).
/// - The picture is centred: column offset (720 − width) / 2 made even, row offset
///   (rows − height) / 2. What is wider or higher than the frame is cut off at the right and
///   at the bottom; an odd last column is dropped.
/// - **A picture wider than 720 is read wrong**: its rows are taken to be 720 bytes apart.
/// - Colour, in 16.16 fixed point: Y = 0.2568 R + 0.5041 G + 0.0979 B (the factors 0x41bd,
///   0x810f, 0x1910), plus 16 at the end; the fraction left over by one pixel is carried
///   into the next (and on over the rows). U and V are filtered over three pixels with the
///   weights ¼, ½, ¼: the pixel before the pair (the second of the pair before), the first
///   and the second of the pair; factors for the middle pixel −0x12f5, −0x253a, 0x382f (U)
///   and 0x3832, −0x2f0e, −0x924 (V), for the outer ones −0x97a, −0x129d, 0x1c17 and 0x1c19,
///   −0x1787, −0x492 (not exactly half). 128 is added. Their fractions are carried too.
/// - With `key` set, a pixel whose alpha is 0 gets its Y **without the 16**: below black, so
///   that a keyer can cut it out. Without an alpha plane every pixel counts as covered.
///
/// The red, green and blue planes are read without a test.
pub fn soft_to_yuv(p: &Picture, buf: &mut [u8], lines: i32, key: bool) -> Result<bool, Fault> {
    if p.planes.iter().all(|x| x.is_none()) {
        return Ok(false);
    }
    let w = (p.width as i32).min(WIDTH as i32);
    let mut h = p.height as i32;
    let mut xoff = (WIDTH as i32 - w) / 2;
    if xoff & 1 != 0 {
        xoff -= 1;
    }
    if lines == LINES_NTSC && h > ROWS_NTSC {
        h = ROWS_NTSC;
    } else if lines == LINES_PAL && h > ROWS_PAL {
        h = ROWS_PAL;
    }
    let rows = match lines {
        LINES_NTSC => ROWS_NTSC,
        LINES_PAL => ROWS_PAL,
        _ => return if h > 0 && w / 2 > 0 { Err(Fault::Uninitialised) } else { Ok(true) },
    };
    let yoff = (rows - h) / 2;
    let pairs = w / 2;
    if h <= 0 || pairs <= 0 {
        return Ok(true);
    }
    let plane = |c: usize| p.planes[c].as_deref().ok_or(Fault::NullPlane);
    let (r, g, b) = (plane(0)?, plane(1)?, plane(2)?);
    let a = p.planes[3].as_deref();
    let (mut acc_y, mut acc_u, mut acc_v, mut prev_u, mut prev_v) = (0u32, 0u32, 0u32, 0u32, 0u32);
    let (mut a0, mut a1) = (0xffu8, 0xffu8);
    let m = |v: u8, k: i32| (v as u32).wrapping_mul(k as u32);
    for y in 0..h as usize {
        let mut at = y * w as usize;
        let mut out = ((yoff as usize + y) * WIDTH + xoff as usize) * 2;
        for _ in 0..pairs {
            let px = |plane: &[u8], k: usize| plane.get(k).copied().ok_or(Fault::ShortPlane);
            let (i0, i1) = (at, at + 1);
            if let Some(a) = a {
                a0 = px(a, i0)?;
                a1 = px(a, i1)?;
            }
            let (r0, g0, b0, r1, g1, b1) = (px(r, i0)?, px(g, i0)?, px(b, i0)?, px(r, i1)?, px(g, i1)?, px(b, i1)?);
            let y0 = m(r0, 0x41bd).wrapping_add(m(g0, 0x810f)).wrapping_add(m(b0, 0x1910)).wrapping_add(acc_y & 0xffff);
            acc_y = m(r1, 0x41bd).wrapping_add(m(g1, 0x810f)).wrapping_add(m(b1, 0x1910)).wrapping_add(y0 & 0xffff);
            let u1 = m(r1, -0x97a).wrapping_add(m(g1, -0x129d)).wrapping_add(m(b1, 0x1c17));
            acc_u = prev_u.wrapping_add(m(r0, -0x12f5)).wrapping_add(m(g0, -0x253a)).wrapping_add(m(b0, 0x382f)).wrapping_add(u1).wrapping_add(acc_u & 0xffff);
            let v1 = m(r1, 0x1c19).wrapping_add(m(g1, -0x1787)).wrapping_add(m(b1, -0x492));
            acc_v = prev_v.wrapping_add(m(r0, 0x3832)).wrapping_add(m(g0, -0x2f0e)).wrapping_add(m(b0, -0x924)).wrapping_add(v1).wrapping_add(acc_v & 0xffff);
            let lift = |covered: u8| if !key || covered != 0 { 16u8 } else { 0 };
            let four = [((acc_u >> 16) as u8).wrapping_sub(0x80), ((y0 >> 16) as u8).wrapping_add(lift(a0)), ((acc_v >> 16) as u8).wrapping_sub(0x80), ((acc_y >> 16) as u8).wrapping_add(lift(a1))];
            buf.get_mut(out..out + 4).ok_or(Fault::ShortPlane)?.copy_from_slice(&four);
            out += 4;
            at += 2;
            prev_u = u1;
            prev_v = v1;
        }
    }
    Ok(true)
}

/// A picture from a YUV frame buffer [soft 0x085e5cb4 SI_pictureYuvToSoft]: `height` rows of
/// 360 big-endian 32-bit words (U, Y, V, Y). The picture is 720 wide, `fields` 3, ratio
/// float(height / 540); no alpha plane. Nothing for a height of 0; a negative height makes
/// the original ask for 4 GB and write through the null pointer it gets.
///
/// Per pair: Y − 16 (not below 0) × 0x12a16, then red = + (V − 128) × 0x198bb, green =
/// − (U − 128) × 0x645a − (V − 128) × 0xd02e, blue = + (U − 128) × 0x204b6, in 16.16 fixed
/// point. A result of 0x1000000 or more is 255, and one **below 0x10000 is 0**: a value that
/// would come out as a fraction below 1 is not rounded, it is cut, like the negative ones.
///
/// `words` holds the words in the machine's order, as `SI_byteswap_fread` leaves them.
pub fn yuv_to_soft(height: i16, words: &[u32]) -> Result<Option<Picture>, Fault> {
    let ratio = (F80::from_i32(height as i32) / F80::from_f32(540.0)).to_f32();
    if height == 0 {
        return Ok(None);
    }
    if height < 0 {
        return Err(Fault::BadSize);
    }
    let n = height as usize * WIDTH;
    let mut planes = [vec![0u8; n], vec![0u8; n], vec![0u8; n]];
    let clip = |v: i32| -> u8 {
        if v >= 0x1000000 {
            0xff
        } else if v < 0x10000 {
            0
        } else {
            (v >> 16) as u8
        }
    };
    for k in 0..height as usize * 360 {
        let word = *words.get(k).ok_or(Fault::ShortPlane)?;
        let u = (word >> 24) as i32 - 0x80;
        let v = (word >> 8 & 0xff) as i32 - 0x80;
        for (i, luma) in [(word >> 16 & 0xff) as i32, (word & 0xff) as i32].into_iter().enumerate() {
            let y = (luma - 16).max(0) * 0x12a16;
            planes[0][2 * k + i] = clip(v * 0x198bb + y);
            planes[1][2 * k + i] = clip(y + u * -0x645a + v * -0xd02e);
            planes[2][2 * k + i] = clip(y + u * 0x204b6);
        }
    }
    let [r, g, b] = planes;
    Ok(Some(Picture { width: WIDTH as i16, height, ratio, fields: 3, pad: 0, planes: [Some(r), Some(g), Some(b), None] }))
}

/// A picture as a `.yuv` file [soft 0x085e5f58 SI_pictureWriteYuv]: the bytes written, or
/// nothing when the picture has no plane at all (the original then leaves an empty file and
/// reports success all the same).
///
/// - A picture 486 or 576 rows high makes a frame of that many rows. Any other height makes
///   an NTSC frame, or a PAL frame when `ABEKAS` is `PAL`; **in that case the picture is
///   still centred and cut as for NTSC**, in the top 486 rows.
/// - The frame starts as black with no colour and no lift: U and V 0x7f, Y 0.
pub fn write_yuv(p: &Picture, key: bool, abekas: Option<&[u8]>) -> Result<Option<Vec<u8>>, Fault> {
    let mut lines = LINES_NTSC;
    let rows = if p.height as i32 == ROWS_NTSC || p.height as i32 == ROWS_PAL {
        if p.height as i32 == ROWS_PAL {
            lines = LINES_PAL;
        }
        p.height as i32
    } else {
        rows_by_environment(abekas, ROWS_NTSC)
    };
    let mut buf = vec![0u8; rows as usize * WIDTH * 2];
    for b in buf.iter_mut().step_by(2) {
        *b = 0x7f;
    }
    Ok(soft_to_yuv(p, &mut buf, lines, key)?.then_some(buf))
}

/// A `.yuv` file as a picture [soft 0x085e6124 SI_pictureReadYuv]: 486 rows, or 576 when
/// `ABEKAS` is `PAL`. The second value is the number of pixel pairs the file really held:
/// past them the original converts memory nobody set (the buffer is not cleared), and the
/// port converts zeros.
pub fn read_yuv(data: &[u8], abekas: Option<&[u8]>) -> Result<(Picture, usize), Fault> {
    let height = rows_by_environment(abekas, ROWS_NTSC) as i16;
    let want = height as usize * 360;
    let have = (data.len() / 4).min(want);
    let mut words = vec![0u32; want];
    for (w, c) in words.iter_mut().zip(data.chunks_exact(4)) {
        *w = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
    }
    // a last, incomplete word is read but not brought into the machine's order
    if have < want && data.len() % 4 != 0 {
        let rest = &data[4 * have..];
        let mut b = [0u8; 4];
        b[..rest.len()].copy_from_slice(rest);
        words[have] = u32::from_le_bytes(b);
    }
    Ok((yuv_to_soft(height, &words)?.ok_or(Fault::BadSize)?, have))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(w: i16, h: i16, v: u8, alpha: Option<u8>) -> Picture {
        let n = w as usize * h as usize;
        Picture { width: w, height: h, ratio: 1.0, fields: 3, pad: 0, planes: [Some(vec![v; n]), Some(vec![v; n]), Some(vec![v; n]), alpha.map(|a| vec![a; n])] }
    }

    #[test]
    fn a_small_picture_sits_in_the_middle_of_a_blank_frame() {
        let file = write_yuv(&grey(4, 2, 255, None), false, None).unwrap().unwrap();
        assert_eq!(file.len(), 486 * 1440);
        assert_eq!(&file[..4], &[0x7f, 0, 0x7f, 0]);
        let at = ((486 - 2) / 2 * 720 + 358) * 2;
        // white: Y 219 + 16, no colour
        assert_eq!((file[at + 1], file[at + 3]), (235, 235));
        assert_eq!(file[at], 0x80);
        assert_eq!(write_yuv(&grey(4, 2, 0, None), false, Some(b"PAL")).unwrap().unwrap().len(), 576 * 1440);
        // keyed out: no lift where alpha is 0
        let keyed = write_yuv(&grey(4, 2, 0, Some(0)), true, None).unwrap().unwrap();
        assert_eq!(keyed[at + 1], 0);
        let none = Picture { width: 4, height: 2, ..Picture::default() };
        assert_eq!(write_yuv(&none, false, None).unwrap(), None);
    }

    #[test]
    fn reading_gives_720_columns_and_cuts_small_values_to_zero() {
        let mut file = vec![0u8; 486 * 1440];
        for c in file.chunks_exact_mut(4) {
            c.copy_from_slice(&[128, 235, 128, 16]);
        }
        let (p, pairs) = read_yuv(&file, None).unwrap();
        assert_eq!((p.width, p.height, p.fields, pairs), (720, 486, 3, 486 * 360));
        assert_eq!(p.ratio, 0.9);
        let r = p.planes[0].as_ref().unwrap();
        assert_eq!((r[0], r[1]), (255, 0));
        assert_eq!(read_yuv(&file, Some(b"PAL")).unwrap().0.height, 576);
        assert_eq!(read_yuv(&file[..10], None).unwrap().1, 2);
    }
}
