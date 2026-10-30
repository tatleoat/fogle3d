// SPDX-License-Identifier: MIT
//! The packed picture record: one 32-bit word per pixel (the original's `SI_PictureRGBA`).
//!
//! Sources are functions of the Linux `soft` binary, cited as `[soft 0xADDRESS name]`.
//! Source files: `picture.c`, `dgt_picture.c`.

use crate::Picture;
use si_x87::{x, Round, F80};

/// A picture as the display and texture code holds it: a 16-byte record (width, height,
/// ratio, fields, a spare word, one pointer) and `width × height` four-byte pixels.
///
/// - In memory a pixel is the bytes red, green, blue, alpha in that order; read as the
///   little-endian word the Linux build sees, red is the low byte.
/// - **Rows run bottom-up**: the first row in `data` is the last row of the picture file
///   (the display layer's convention), the opposite of [`Picture`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PictureRgba {
    pub width: i16,
    pub height: i16,
    pub ratio: f32,
    /// Offset 8 of the record. The plane converters below do not copy it.
    pub fields: i16,
    /// Offset 10 of the record.
    pub pad: i16,
    /// `4 × width × height` bytes: red, green, blue, alpha per pixel, bottom row first.
    pub data: Vec<u8>,
}

impl PictureRgba {
    /// Pixel `i` as the 32-bit word the Linux build reads (red in the low byte).
    pub fn word(&self, i: usize) -> u32 {
        u32::from_le_bytes([self.data[4 * i], self.data[4 * i + 1], self.data[4 * i + 2], self.data[4 * i + 3]])
    }

    pub fn set_word(&mut self, i: usize, v: u32) {
        self.data[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// Planes to packed [soft 0x085bbfc4 SI_convertSI_Pict2SI_PictRGBA].
    ///
    /// Copies width, height and ratio, sets the spare word to 0 and leaves `fields` as the
    /// caller's record had it (here: 0). Rows are turned upside down. **Alpha is not copied**:
    /// every pixel's fourth byte is 0, whether or not the picture has an alpha plane. The
    /// original reads the red, green and blue planes without testing them; a picture lacking
    /// one is refused here. An empty picture (no pixels) is refused by the original too.
    pub fn from_planes(p: &Picture) -> Option<PictureRgba> {
        let (w, h) = (p.width.max(0) as usize, p.height.max(0) as usize);
        if w * h == 0 {
            return None; // the allocator returns nothing for a count of zero
        }
        let (r, g, b) = (p.planes[0].as_ref()?, p.planes[1].as_ref()?, p.planes[2].as_ref()?);
        let mut data = vec![0u8; 4 * w * h];
        let mut src = 0usize;
        for y in (0..h).rev() {
            for x in 0..w {
                let at = 4 * (w * y + x);
                data[at] = r[src];
                data[at + 1] = g[src];
                data[at + 2] = b[src];
                src += 1;
            }
        }
        Some(PictureRgba { width: p.width, height: p.height, ratio: p.ratio, fields: 0, pad: 0, data })
    }

    /// Packed to planes [soft 0x085bc0c0 SI_convertSI_PictRGBA2SI_Pict].
    ///
    /// All four planes are made, alpha included; rows are turned upside down; the spare word
    /// of the result is 1 and `fields` is left as the caller's record had it (here: 0).
    /// An empty picture gives nothing (the allocator returns nothing for a count of zero).
    pub fn to_planes(&self) -> Option<Picture> {
        let (w, h) = (self.width.max(0) as usize, self.height.max(0) as usize);
        if w * h == 0 || self.data.len() < 4 * w * h {
            return None;
        }
        let mut planes: [Vec<u8>; 4] = [vec![0u8; w * h], vec![0u8; w * h], vec![0u8; w * h], vec![0u8; w * h]];
        let mut src = 0usize;
        for y in (0..h).rev() {
            for x in 0..w {
                for (c, plane) in planes.iter_mut().enumerate() {
                    plane[w * y + x] = self.data[4 * src + c];
                }
                src += 1;
            }
        }
        let [r, g, b, a] = planes;
        Some(Picture { width: self.width, height: self.height, ratio: self.ratio, fields: 0, pad: 1, planes: [Some(r), Some(g), Some(b), Some(a)] })
    }
}


/// A rectangle of pixel columns and rows, both ends included (the original's `SI_Irect`,
/// passed by value as four 16-bit words in this order).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Irect {
    pub x0: i16,
    pub x1: i16,
    pub y0: i16,
    pub y1: i16,
}

/// Blackens the outermost pixels [soft 0x085d6710 SI_picRgbaBorder]: the first and the last
/// row, and the first and last pixel of every row, all four bytes zero.
///
/// With no rows the original clears a row in front of the buffer; nothing is done here.
pub fn border(p: &mut PictureRgba) {
    let (w, h) = (p.width.max(0) as usize, p.height.max(0) as usize);
    if w == 0 || h == 0 || p.data.len() < 4 * w * h {
        return;
    }
    p.data[..4 * w].fill(0);
    p.data[4 * w * (h - 1)..4 * w * h].fill(0);
    for y in 0..h {
        p.data[4 * w * y..4 * w * y + 4].fill(0);
        p.data[4 * (w * y + w - 1)..4 * (w * y + w)].fill(0);
    }
}

/// Reverses the byte order of every pixel [soft 0x085d6fb4 SI_picRgbaSwapBits], with the
/// original's flaw: each byte is sign-extended before it is shifted into place and the four
/// are or-ed together, so a byte of 0x80 or more also sets every bit above it. Read as bytes
/// in memory the result is alpha, blue, green, red, where a blue of 0x80 or more turns green
/// and red into 0xff, and so on; **an opaque alpha (0xff) turns the whole pixel into
/// 0xffffffff**.
pub fn swap_bits_rgba(p: &mut PictureRgba) {
    let n = (p.width as i32 * p.height as i32).max(0) as usize;
    for i in 0..n.min(p.data.len() / 4) {
        let b = &p.data[4 * i..4 * i + 4];
        let s = |v: u8| v as i8 as i32;
        let word = (s(b[0]) << 24) | (s(b[1]) << 16) | (s(b[2]) << 8) | s(b[3]);
        p.data[4 * i..4 * i + 4].copy_from_slice(&(word as u32).to_le_bytes());
    }
}

/// Reverses each three-byte pixel [soft 0x085d7048 SI_picRgbSwapBits]: `data` must already
/// be three bytes per pixel (after [`alpha_remove`]); red and blue change places.
pub fn swap_bits_rgb(p: &mut PictureRgba) {
    let n = (p.width as i32 * p.height as i32).max(0) as usize;
    for i in 0..n.min(p.data.len() / 3) {
        p.data.swap(3 * i, 3 * i + 2);
    }
    p.data.truncate(3 * n);
}

/// Drops the fourth byte of every pixel [soft 0x085d76b8 picAlphaRemove]: afterwards `data`
/// is three bytes per pixel, red, green, blue.
pub fn alpha_remove(p: &mut PictureRgba) {
    let n = (p.width as i32 * p.height as i32).max(0) as usize;
    let mut out = Vec::with_capacity(3 * n);
    for i in 0..n.min(p.data.len() / 4) {
        out.extend_from_slice(&p.data[4 * i..4 * i + 3]);
    }
    out.resize(3 * n, 0);
    p.data = out;
}

/// The clamping `SI_cropPic` and `SI_cropPicCI` apply to one axis of the rectangle: the far
/// end is pulled inside the picture, a near end not below the far end becomes far − 1, and a
/// negative near end becomes 0. Returns (near, far).
pub(crate) fn clamp_axis(mut near: i16, mut far: i16, size: i32) -> (i16, i16) {
    if size <= far as i32 {
        far = (size - 1) as i16;
    }
    if far <= near {
        near = far.wrapping_sub(1);
    }
    if near < 0 {
        near = 0;
    }
    (near, far)
}

/// Cuts a rectangle out of a packed picture [soft 0x085d70cc SI_cropPic].
///
/// The rectangle is clamped per axis (see `clamp_axis`): note that a rectangle one pixel
/// wide comes out two wide, because a near end equal to the far end is moved one back. The
/// result has `x1 − x0 + 1` columns and `y1 − y0 + 1` rows, the source's ratio and `fields`
/// and a zero word at offset 10.
///
/// The original does not test the clamped size. A far end below 0 gives a zero or negative
/// size there (and a failed or wild allocation); here that is `None`.
pub fn crop(p: &PictureRgba, r: Irect) -> Option<PictureRgba> {
    let (y0, y1) = clamp_axis(r.y0, r.y1, p.height as i32);
    let (x0, x1) = clamp_axis(r.x0, r.x1, p.width as i32);
    let (w, h) = (x1 as i32 - x0 as i32 + 1, y1 as i32 - y0 as i32 + 1);
    if w <= 0 || h <= 0 || p.width <= 0 {
        return None;
    }
    let (w, h, sw) = (w as usize, h as usize, p.width as usize);
    let mut data = vec![0u8; 4 * w * h];
    for i in 0..h {
        let from = 4 * ((y0 as usize + i) * sw + x0 as usize);
        data[4 * i * w..4 * (i + 1) * w].copy_from_slice(p.data.get(from..from + 4 * w)?);
    }
    Some(PictureRgba { width: w as i16, height: h as i16, ratio: p.ratio, fields: p.fields, pad: 0, data })
}

/// The source index `resize` and `resize_ci` take for output position `i` of `n`, from a
/// source of `m`: `(m − 1) × (i / (n − 1))` truncated, computed in the x87 unit (class:
/// model). With `n` = 1 the quotient is 0/0 and the conversion gives the integer indefinite,
/// 0x80000000.
pub(crate) fn nearest(i: i32, n: i32, m: i32) -> i32 {
    (F80::from_i32(m - 1) * (F80::from_i32(i) / F80::from_i32(n - 1))).to_i32(Round::Zero)
}

/// Resamples a packed picture to `width × height` by taking the nearest pixel
/// [soft 0x085d7220 SI_resizePic].
///
/// - A negative `height` turns the picture upside down, a `width` below 1 mirrors it; the
///   size is the absolute value, at least 1.
/// - When both equal the source's size the pixels are copied.
/// - Otherwise output row `y` takes source row `(hs − 1) × (y / (h − 1))` and column `x`
///   source column `(x / (w − 1)) × (ws − 1)`, truncated: the first and last pixels map onto
///   the first and last, nothing is averaged.
/// - A size of 1 divides 0 by 0; the index becomes 0x80000000, which the 32-bit address
///   arithmetic (index × 4) wraps to row 0 or column 0. Reproduced by wrapping the same way.
/// - The result's ratio, `fields` and spare word are zero.
///
/// An index that lands outside the source (possible only through that wrapping with a
/// flipped size of 1) reads foreign memory in the original; here it gives a zero pixel.
pub fn resize(p: &PictureRgba, width: i32, height: i32) -> Option<PictureRgba> {
    let (w, h) = (width.unsigned_abs().max(1) as usize, height.unsigned_abs().max(1) as usize);
    if w > i16::MAX as usize || h > i16::MAX as usize {
        return None; // the original stores the size in 16 bits and allocates the full product
    }
    let mut out = PictureRgba { width: w as i16, height: h as i16, ratio: 0.0, fields: 0, pad: 0, data: vec![0u8; 4 * w * h] };
    if p.width as i32 == width && p.height as i32 == height {
        out.data.copy_from_slice(p.data.get(..4 * w * h)?);
        return Some(out);
    }
    for y in 0..h {
        let mut sy = nearest(y as i32, h as i32, p.height as i32);
        if height < 0 {
            sy = (p.height as i32).wrapping_sub(sy).wrapping_sub(1);
        }
        for x in 0..w {
            let sx = nearest(x as i32, w as i32, p.width as i32);
            let index = (p.width as i32).wrapping_mul(sy).wrapping_add(sx);
            let from = (index as u32).wrapping_mul(4) as usize;
            let to = 4 * (y * w + if width < 1 { w - 1 - x } else { x });
            if let Some(px) = p.data.get(from..from + 4) {
                out.data[to..to + 4].copy_from_slice(px);
            }
        }
    }
    Some(out)
}

/// The width of the transparent gap `enlarge` and `enlarge_ci` leave between the two copies:
/// `(1 − blend) × size`, truncated (x87, class: model).
pub(crate) fn enlarge_gap(blend: f32, size: i32) -> i32 {
    ((F80::ONE - x(blend)) * F80::from_i32(size)).to_i32(Round::Zero)
}

/// Doubles a packed picture with a gap between the copies [soft 0x085d772c SI_picEnlarge].
///
/// `blend` is the float at offset 0x58 of the texture-map record and `axis` its word at
/// offset 0xfc. The gap is `(1 − blend) × width` pixels (the width is used for both axes).
/// - `axis` = 0: each row becomes the row, the gap, the row again.
/// - `axis` = 1: the picture, `gap` empty rows, the picture again.
/// - The result's ratio, `fields` and spare word are zero.
///
/// Any other `axis` sizes the result as for 0 but copies as for 1, past the end of the
/// buffer; a `blend` above 1 gives a negative gap, and below −width a copy that starts in
/// front of the buffer. Those cases return `None` here.
pub fn enlarge(p: &PictureRgba, blend: f32, axis: i16) -> Option<PictureRgba> {
    let (w, h) = (p.width as i32, p.height as i32);
    let gap = enlarge_gap(blend, w);
    let (nw, nh) = if axis == 1 { (w, gap.wrapping_add(2 * h)) } else { (gap.wrapping_add(2 * w), h) };
    if w <= 0 || h <= 0 || nw <= 0 || nh <= 0 || nw > i16::MAX as i32 || nh > i16::MAX as i32 || !(0..=1).contains(&axis) {
        return None;
    }
    let (w, h, nw, nh) = (w as usize, h as usize, nw as usize, nh as usize);
    let src = p.data.get(..4 * w * h)?;
    let mut out = PictureRgba { width: nw as i16, height: nh as i16, ratio: 0.0, fields: 0, pad: 0, data: vec![0u8; 4 * nw * nh] };
    if axis == 1 {
        let second = 4 * w * usize::try_from(gap + h as i32).ok()?;
        out.data[..4 * w * h].copy_from_slice(src);
        out.data.get_mut(second..second + 4 * w * h)?.copy_from_slice(src);
    } else {
        let second = usize::try_from(w as i32 + gap).ok()?;
        for y in 0..nh {
            let row = &src[4 * w * y..4 * w * (y + 1)];
            out.data[4 * nw * y..4 * (nw * y + w)].copy_from_slice(row);
            out.data[4 * (nw * y + second)..4 * (nw * y + second + w)].copy_from_slice(row);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planes_to_packed_and_back() {
        let (w, h) = (3usize, 2usize);
        let plane = |k: u8| Some((0..(w * h) as u8).map(|i| i * 4 + k).collect::<Vec<u8>>());
        let p = Picture { width: 3, height: 2, ratio: 1.5, fields: 3, pad: 9, planes: [plane(0), plane(1), plane(2), plane(3)] };
        let q = PictureRgba::from_planes(&p).unwrap();
        // the file's first row is the packed picture's last; alpha is dropped
        assert_eq!(&q.data[12..16], &[0, 1, 2, 0]);
        assert_eq!(q.word(0), 0x000e_0d0c);
        assert_eq!((q.ratio, q.fields, q.pad), (1.5, 0, 0));
        let back = q.to_planes().unwrap();
        assert_eq!(back.planes[0], p.planes[0]);
        assert_eq!(back.planes[2], p.planes[2]);
        assert_eq!(back.planes[3], Some(vec![0u8; 6]));
        assert_eq!(back.pad, 1);
    }

    fn ramp(w: i16, h: i16) -> PictureRgba {
        let n = w as usize * h as usize;
        let mut data = Vec::new();
        for i in 0..n {
            data.extend_from_slice(&[i as u8, (i >> 8) as u8, 7, 9]);
        }
        PictureRgba { width: w, height: h, ratio: 1.25, fields: 3, pad: 1, data }
    }

    #[test]
    fn border_blackens_the_frame() {
        let mut p = ramp(4, 3);
        border(&mut p);
        let px = |x: usize, y: usize| p.word(y * 4 + x);
        assert_eq!(px(0, 1), 0);
        assert_eq!(px(3, 1), 0);
        assert_eq!(px(1, 0), 0);
        assert_eq!(px(2, 2), 0);
        assert_eq!(px(1, 1), 0x0907_0005);
    }

    #[test]
    fn swap_bits_sign_extends() {
        let mut p = PictureRgba { width: 3, height: 1, ratio: 1.0, fields: 0, pad: 0, data: vec![1, 2, 3, 4, 1, 2, 0x80, 4, 1, 2, 3, 0xff] };
        swap_bits_rgba(&mut p);
        assert_eq!(&p.data[..4], &[4, 3, 2, 1]);
        assert_eq!(&p.data[4..8], &[4, 0x80, 0xff, 0xff]); // a high blue floods green and red
        assert_eq!(&p.data[8..], &[0xff; 4]); // an opaque alpha floods the pixel
        let mut q = ramp(2, 1);
        alpha_remove(&mut q);
        assert_eq!(q.data, [0, 0, 7, 1, 0, 7]);
        swap_bits_rgb(&mut q);
        assert_eq!(q.data, [7, 0, 0, 7, 0, 1]);
    }

    #[test]
    fn crop_clamps_as_the_original() {
        let p = ramp(5, 4);
        let c = crop(&p, Irect { x0: 1, x1: 3, y0: 2, y1: 9 }).unwrap();
        assert_eq!((c.width, c.height, c.ratio, c.fields, c.pad), (3, 2, 1.25, 3, 0));
        assert_eq!(c.data[0], 11);
        assert_eq!(c.data[4 * 5], 18);
        // a one-pixel rectangle comes out two wide and two high
        let c = crop(&p, Irect { x0: 2, x1: 2, y0: 1, y1: 1 }).unwrap();
        assert_eq!((c.width, c.height, c.data[0]), (2, 2, 1));
        assert_eq!(crop(&p, Irect { x0: 0, x1: -3, y0: 0, y1: 2 }), None);
    }

    #[test]
    fn resize_takes_nearest_with_ends_fixed() {
        let p = ramp(5, 1);
        let r = resize(&p, 3, 1).unwrap();
        assert_eq!((r.data[0], r.data[4], r.data[8]), (0, 2, 4));
        assert_eq!((r.ratio, r.fields, r.pad), (0.0, 0, 0));
        // up: (x / 8) × 4 truncated
        let r = resize(&p, 9, 1).unwrap();
        let got: Vec<u8> = (0..9).map(|i| r.data[4 * i]).collect();
        assert_eq!(got, [0, 0, 1, 1, 2, 2, 3, 3, 4]);
        // mirrored
        let r = resize(&p, -5, 1).unwrap();
        let got: Vec<u8> = (0..5).map(|i| r.data[4 * i]).collect();
        assert_eq!(got, [4, 3, 2, 1, 0]);
        // same size: a copy
        assert_eq!(resize(&p, 5, 1).unwrap().data, p.data);
        // a size of 1 wraps to the first column and row
        let q = ramp(4, 3);
        let r = resize(&q, 1, 1).unwrap();
        assert_eq!(r.data[0], 0);
        let r = resize(&q, 1, -1).unwrap();
        assert_eq!(r.data[0], 8); // flipped: the last row
    }

    #[test]
    fn enlarge_doubles_with_a_gap() {
        let p = ramp(4, 2);
        let e = enlarge(&p, 0.5, 0).unwrap();
        assert_eq!((e.width, e.height), (10, 2));
        let row: Vec<u8> = (0..10).map(|i| e.data[4 * i]).collect();
        assert_eq!(row, [0, 1, 2, 3, 0, 0, 0, 1, 2, 3]);
        let e = enlarge(&p, 0.75, 1).unwrap();
        assert_eq!((e.width, e.height), (4, 5)); // gap = 0.25 × width = 1 row
        assert_eq!(e.data[4 * 4 * 3], 0);
        assert_eq!(e.data[4 * 4 * 2], 0);
        assert_eq!(e.data[4 * 4 * 2 + 3], 0);
        assert_eq!(e.data[4 * (4 * 3 + 1)], 1);
        assert_eq!(enlarge(&p, 0.5, 2), None);
    }
}
