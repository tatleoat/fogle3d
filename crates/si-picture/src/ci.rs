// SPDX-License-Identifier: MIT
//! The colour-index picture record and the three operations `picture.c` has on it.
//!
//! Sources are functions of the Linux `soft` binary, cited as `[soft 0xADDRESS name]`.
//! Source file: `picture.c`. The record itself (the original's `DG_Picture`, 0x1c bytes) is
//! made and filled by `dgt_picture.c`.

use crate::rgba::{clamp_axis, enlarge_gap, nearest};
use crate::Irect;

/// A picture of colour indices: the parts of the original's 0x1c-byte `DG_Picture` these
/// functions touch. Width and height are **unsigned** 16-bit here, unlike the other records.
///
/// | Offset | Field |
/// |---|---|
/// | 0, 2 | width, height |
/// | 4 | `size`: a 32-bit count the allocator sets to 2 × width × height |
/// | 8 | pointer to the palette (not held here) |
/// | 0xc | `data`: one 16-bit index per pixel |
/// | 0x10 | a second buffer of one byte per pixel (not held here) |
/// | 0x14… | four 16-bit words the allocator sets to 0, height − 1, width − 1, 0 |
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PictureCi {
    pub width: u16,
    pub height: u16,
    pub size: u32,
    pub data: Vec<u16>,
}

// [soft 0x085d4ccc SI_pictureDisposeCI] frees `data` and the record; in Rust that is the
// value going out of scope. The same holds for [soft 0x085d45bc SI_pictureDispose],
// [soft 0x085d4c88 SI_pictureDisposeRGBA] and the wrapper [soft 0x085d2954 nDisposePicture]
// on the other two records.

/// Cuts a rectangle out of a colour-index picture [soft 0x085d79dc SI_cropPicCI]: the rules
/// of [`crate::crop`]. Every field of the result but the size and the pixels is zero.
pub fn crop_ci(p: &PictureCi, r: Irect) -> Option<PictureCi> {
    let (y0, y1) = clamp_axis(r.y0, r.y1, p.height as i32);
    let (x0, x1) = clamp_axis(r.x0, r.x1, p.width as i32);
    let (w, h) = (x1 as i32 - x0 as i32 + 1, y1 as i32 - y0 as i32 + 1);
    if w <= 0 || h <= 0 {
        return None; // not tested in the original, as in `crop`
    }
    let (w, h, sw) = (w as usize, h as usize, p.width as usize);
    let mut data = vec![0u16; w * h];
    for i in 0..h {
        let from = (y0 as usize + i) * sw + x0 as usize;
        data[i * w..(i + 1) * w].copy_from_slice(p.data.get(from..from + w)?);
    }
    Some(PictureCi { width: w as u16, height: h as u16, size: 0, data })
}

/// Resamples a colour-index picture by nearest index [soft 0x085d7464 SI_resizePicCI]: the
/// rules of [`crate::resize`] (flip for a negative height, mirror for a width below 1, copy
/// when the size is unchanged, the same arithmetic and the same wrap for a size of 1, here
/// with a factor of 2 bytes per pixel).
pub fn resize_ci(p: &PictureCi, width: i32, height: i32) -> Option<PictureCi> {
    let (w, h) = (width.unsigned_abs().max(1) as usize, height.unsigned_abs().max(1) as usize);
    if w > u16::MAX as usize || h > u16::MAX as usize {
        return None; // the original stores the size in 16 bits and allocates the full product
    }
    let mut out = PictureCi { width: w as u16, height: h as u16, size: 0, data: vec![0u16; w * h] };
    if p.width as i32 == width && p.height as i32 == height {
        out.data.copy_from_slice(p.data.get(..w * h)?);
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
            let from = ((index as u32).wrapping_mul(2) / 2) as usize;
            if let Some(&v) = p.data.get(from) {
                out.data[y * w + if width < 1 { w - 1 - x } else { x }] = v;
            }
        }
    }
    Some(out)
}

/// Doubles a colour-index picture across with a gap [soft 0x085d78bc SI_picEnlargeCI]: the
/// horizontal case of [`crate::enlarge`] only (the map's axis word is not consulted). The
/// result's `size` is its width × height.
pub fn enlarge_ci(p: &PictureCi, blend: f32) -> Option<PictureCi> {
    let (w, h) = (p.width as usize, p.height as usize);
    let gap = enlarge_gap(blend, p.width as i32);
    let nw = (gap.wrapping_add(2 * p.width as i32)) as u32 & 0xffff;
    let second = usize::try_from(p.width as i32 + gap).ok()?;
    if nw == 0 || h == 0 || second + w > nw as usize {
        return None; // a gap that wraps the 16-bit width overruns the buffer in the original
    }
    let nw = nw as usize;
    let mut out = PictureCi { width: nw as u16, height: p.height, size: (nw * h) as u32, data: vec![0u16; nw * h] };
    for y in 0..h {
        let row = p.data.get(w * y..w * (y + 1))?;
        out.data[nw * y..nw * y + w].copy_from_slice(row);
        out.data[nw * y + second..nw * y + second + w].copy_from_slice(row);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(w: u16, h: u16) -> PictureCi {
        PictureCi { width: w, height: h, size: 2 * w as u32 * h as u32, data: (0..w * h).collect() }
    }

    #[test]
    fn crop_resize_enlarge_follow_the_packed_versions() {
        let p = ramp(5, 4);
        let c = crop_ci(&p, Irect { x0: 1, x1: 3, y0: 2, y1: 9 }).unwrap();
        assert_eq!((c.width, c.height, c.size), (3, 2, 0));
        assert_eq!(c.data, [11, 12, 13, 16, 17, 18]);
        let r = resize_ci(&ramp(5, 1), 9, 1).unwrap();
        assert_eq!(r.data, [0, 0, 1, 1, 2, 2, 3, 3, 4]);
        assert_eq!(resize_ci(&ramp(5, 1), -3, 1).unwrap().data, [4, 2, 0]);
        assert_eq!(resize_ci(&p, 1, 1).unwrap().data, [0]);
        assert_eq!(resize_ci(&p, 5, 4).unwrap().data, p.data);
        let e = enlarge_ci(&ramp(4, 1), 0.5).unwrap();
        assert_eq!((e.width, e.size), (10, 10));
        assert_eq!(e.data, [0, 1, 2, 3, 0, 0, 0, 1, 2, 3]);
    }
}
