// SPDX-License-Identifier: MIT
//! Tiling a packed picture for a repeated texture.
//!
//! Sources are functions of the Linux `soft` binary, cited as `[soft 0xADDRESS name]`.
//! Source file: `pic_tile.c`.

use si_picture::{resize, PictureRgba};

/// How neighbouring tiles are turned (the fourth parameter of `SI_generateTiledPic`; the
/// texture loader makes it from the map's two "alternate" flags).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alternate {
    /// 0: every tile the same.
    None,
    /// 1: every second column mirrored.
    Across,
    /// 2: every second row turned upside down.
    Up,
    /// 3: both.
    Both,
}

impl Alternate {
    pub fn from_mode(mode: i16) -> Option<Alternate> {
        Some(match mode {
            0 => Alternate::None,
            1 => Alternate::Across,
            2 => Alternate::Up,
            3 => Alternate::Both,
            _ => return None,
        })
    }
}

/// Lays four tiles of the same size out as `across × up` tiles [soft 0x085e6624 tilePic].
///
/// Tile rows are counted from the **end** of the buffer: the last tile row, the third from
/// the end, … take `a` in the even columns and `b` in the odd ones; the others take `c` and
/// `d`. The result is `a.width × across` by `a.height × up` (16-bit products); its ratio,
/// `fields` and spare word are zero. Nothing is returned for an empty result (the allocator
/// gives nothing for a count of zero).
///
/// The original copies `a`'s size from each tile without looking at theirs; a tile smaller
/// than `a` is read past its end there and is refused here.
pub fn tile_pic(a: &PictureRgba, b: &PictureRgba, c: &PictureRgba, d: &PictureRgba, across: i32, up: i32) -> Option<PictureRgba> {
    let (w, h) = (a.width as i32, a.height as i32);
    let (ow, oh) = (a.width.wrapping_mul(across as i16), a.height.wrapping_mul(up as i16));
    if ow as i32 * oh as i32 <= 0 || ow < 0 || w <= 0 || h <= 0 || ow as i32 != w * across || oh as i32 != h * up {
        return None; // also where the 16-bit products wrap: the original then writes outside its buffer
    }
    let (w, h, ow_u) = (w as usize, h as usize, ow as usize);
    let mut out = PictureRgba { width: ow, height: oh, ratio: 0.0, fields: 0, pad: 0, data: vec![0u8; 4 * ow_u * oh as usize] };
    for t in (1..=up).rev() {
        let row0 = h * (up - t) as usize;
        for j in 0..across {
            let src = match (t % 2 == 1, j % 2 == 0) {
                (true, true) => a,
                (true, false) => b,
                (false, true) => c,
                (false, false) => d,
            };
            let from = src.data.get(..4 * w * h)?;
            for r in 0..h {
                let at = 4 * ((row0 + r) * ow_u + j as usize * w);
                out.data[at..at + 4 * w].copy_from_slice(&from[4 * r * w..4 * (r + 1) * w]);
            }
        }
    }
    Some(out)
}

/// Shrinks a packed picture and repeats it `across × up` times in a picture of (nearly) the
/// same size [soft 0x085e62b0 SI_generateTiledPic].
///
/// One tile is the picture resampled to `width / across` by `height / up` (integer
/// divisions) with [`si_picture::resize`]; the alternating kinds add the mirrored, the
/// upside-down or both versions (a negative size to the same routine) and [`tile_pic`] lays
/// them out. With `Up` and `Both` it is the **last** tile row, and every second one from
/// there, that is upside down.
///
/// The original frees the picture it was given and puts the result in its place, also when
/// the result is nothing (any mode other than 0 to 3, or an empty result). A count of zero
/// divides by zero there; here it gives nothing.
pub fn generate_tiled_pic(p: &PictureRgba, across: i32, up: i32, mode: i16) -> Option<PictureRgba> {
    if across == 0 || up == 0 {
        return None;
    }
    let (tw, th) = (p.width as i32 / across, p.height as i32 / up);
    let plain = || resize(p, tw, th);
    match Alternate::from_mode(mode)? {
        Alternate::None => {
            let t = plain()?;
            tile_pic(&t, &t, &t, &t, across, up)
        }
        Alternate::Across => {
            let (a, b) = (plain()?, resize(p, -tw, th)?);
            tile_pic(&a, &b, &a, &b, across, up)
        }
        Alternate::Up => {
            let (a, c) = (resize(p, tw, -th)?, plain()?);
            tile_pic(&a, &a, &c, &c, across, up)
        }
        Alternate::Both => {
            let (a, b, c, d) = (resize(p, tw, -th)?, resize(p, -tw, -th)?, plain()?, resize(p, -tw, th)?);
            tile_pic(&a, &b, &c, &d, across, up)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pic(w: i16, h: i16, base: u8) -> PictureRgba {
        let n = w as usize * h as usize;
        PictureRgba { width: w, height: h, ratio: 1.5, fields: 3, pad: 1, data: (0..n).flat_map(|i| [base + i as u8, 0, 0, 9]).collect() }
    }

    fn reds(p: &PictureRgba) -> Vec<u8> {
        p.data.chunks(4).map(|c| c[0]).collect()
    }

    #[test]
    fn four_tiles_by_position() {
        let (a, b, c, d) = (pic(1, 1, 10), pic(1, 1, 20), pic(1, 1, 30), pic(1, 1, 40));
        let t = tile_pic(&a, &b, &c, &d, 3, 3).unwrap();
        assert_eq!((t.width, t.height, t.ratio, t.pad), (3, 3, 0.0, 0));
        // rows from the start of the buffer: a/b, c/d, a/b (counted from the end: 3rd, 2nd, 1st)
        assert_eq!(reds(&t), [10, 20, 10, 30, 40, 30, 10, 20, 10]);
        let t = tile_pic(&a, &b, &c, &d, 2, 2).unwrap();
        assert_eq!(reds(&t), [30, 40, 10, 20]);
        assert!(tile_pic(&a, &b, &c, &d, 0, 2).is_none());
    }

    #[test]
    fn tiling_shrinks_then_repeats() {
        let p = pic(4, 2, 0); // rows 0 1 2 3 / 4 5 6 7
        let t = generate_tiled_pic(&p, 2, 1, 0).unwrap();
        assert_eq!((t.width, t.height), (4, 2));
        assert_eq!(reds(&t), [0, 3, 0, 3, 4, 7, 4, 7]); // each tile: columns 0 and 3
        let t = generate_tiled_pic(&p, 2, 1, 1).unwrap();
        assert_eq!(reds(&t), [0, 3, 3, 0, 4, 7, 7, 4]); // second column mirrored
        let t = generate_tiled_pic(&p, 1, 2, 2).unwrap();
        // tiles are one row high (row 0 of the source both ways); two of them
        assert_eq!((t.width, t.height), (4, 2));
        assert!(generate_tiled_pic(&p, 2, 1, 4).is_none());
        assert!(generate_tiled_pic(&p, 0, 1, 0).is_none());
    }
}
