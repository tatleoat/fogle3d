// SPDX-License-Identifier: MIT
//! Compositing one picture onto another, the cross-fade, and the helpers of the compositing
//! dialog that work on pictures alone (finding 0014, "Compositing").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `pic_composite.c` (0x085c6b70–0x085ca23e, 10
//! functions). The dialog itself (`SI_pictureCompositeNew`) and its screen refresh
//! (`resetBackBufferAndRefresh`) are not here.
//!
//! Conventions are those of [`crate::effect`]: planes red, green, blue, alpha, first row
//! first; a [`Fault`] where the original would read or write through a null pointer or past a
//! plane. The compositor reads and writes **all four planes of both pictures** without a test.
//! Arithmetic that the original does in the x87 register is done on [`F80`], with a rounding
//! to `f32` where the original stores a factor to a float.

use crate::effect::Fault;
use crate::softpic::si_malloc;
use si_file::Stream;
use si_picture::{Picture, MAX_ALLOC};
use si_x87::{Round, F80};

/// The three depth files of a depth-aware composite: rows of big-endian floats, one per
/// pixel, no header (the renderer's depth pictures). `back` goes with the destination,
/// `layer` with the source, and `out` receives the destination's new depth, one row per
/// picture row composited. In the original these are three global `FILE` pointers (`bfp`,
/// `lfp`, `newbzfp`) which the compositing dialog opens.
///
/// A larger value is nearer: a pixel whose depth cannot be read counts as −100000.
pub struct Depth<'a> {
    pub back: Stream<'a>,
    pub layer: Stream<'a>,
    pub out: Vec<u8>,
}

/// `count` floats read as [soft 0x083f9a7c SI_float_fread] reads them: the bytes go into the
/// buffer as they come and each whole value read is turned from big-endian. Returns the
/// number of whole values.
fn float_fread(buf: &mut [u8], count: usize, s: &mut Stream) -> usize {
    let n = s.read(&mut buf[..count * 4]) / 4;
    for v in buf[..n * 4].chunks_exact_mut(4) {
        v.reverse();
    }
    n
}

fn f32_at(buf: &[u8], i: usize) -> f32 {
    f32::from_le_bytes([buf[4 * i], buf[4 * i + 1], buf[4 * i + 2], buf[4 * i + 3]])
}

fn set_f32(buf: &mut [u8], i: usize, v: f32) {
    buf[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
}

fn cap(v: i32) -> u8 {
    if v > 0xff {
        0xff
    } else {
        v as u8
    }
}

fn b(v: u8) -> F80 {
    F80::from_i32(v as i32)
}

/// Composites `src` onto `dst` with its lower left corner at (`x`, `y`) of the destination,
/// **`y` counted from the bottom row** [soft 0x085c8244 pictureComposite]
/// (`compositePicture` [soft 0x085c820c compositePicture] is the same call without depth;
/// both return 0 whatever happened).
///
/// The part of the source that falls outside the destination is left out; if nothing
/// overlaps, nothing happens. Rows are done from the top one down. `mode`, with s and d a
/// source and a destination byte, sa and da the alphas, all four channels treated alike
/// unless stated:
///
/// | Mode | Result |
/// |---|---|
/// | 0 | s (copy) |
/// | 1 | where sa ≠ 0: trunc(d × k + s + 0.5), k = float((255 − sa) / 255): source over destination, colours already multiplied by alpha |
/// | 2 | where the source pixel is not all zero: trunc(t × f + (1 − f) × d), t = the result of mode 1 **before** its limit to 255 |
/// | 3 | the larger of s and d |
/// | 4 | where sa ≠ 0: the smaller of 255 − s × sa / 255 (integer division) and 255 − d; alpha: the same with s = sa |
/// | 5 | trunc(d × k + s × float(1 − k)), k as in mode 1: source over destination, colours not multiplied |
/// | 6 | where sa ≠ 0: trunc(d + s × f × float(sa / 255) + 0.5): added |
/// | 7 | where d's red + green + blue ≤ s's: the source pixel, alpha too |
/// | 8 | the absolute difference |
///
/// Results above 255 become 255; `f` is `factor`. Other modes do nothing.
///
/// With `depth` (the original's last argument non-zero), modes 1 and 5 read one row of each
/// depth file per picture row and choose per pixel: where the layer is **behind** the
/// destination (its depth is less) the roles are exchanged, the destination going over the
/// source with the destination's alpha; otherwise the mode's rule applies and the pixel's
/// depth becomes the layer's. Mode 1 under depth has no test of sa. The destination's depth
/// row is then written to the third file. A depth row that gives no value at all is taken
/// as −100000 throughout; mode 1 asks the layer file for the source's width in values, mode
/// 5 for the destination's.
pub fn picture_composite(dst: &mut Picture, src: &Picture, x: i16, y: i16, mode: i16, factor: f32, mut depth: Option<&mut Depth>) -> Result<(), Fault> {
    let (sw, sh, dw, dh) = (src.width as i32, src.height as i32, dst.width as i32, dst.height as i32);
    let (mut x0, mut y0) = (x as i32, y as i32);
    let mut x1 = sw - 1 + x0;
    let mut y1 = sh + y0 - 1;
    if x0 > dw - 1 || y0 > dh - 1 || x1 < 0 || y1 < 0 {
        return Ok(());
    }
    let mut skip = 0i32;
    if x0 < 0 {
        skip = -x0;
        x0 = 0;
    }
    if x1 > dw - 1 {
        x1 = dw - 1;
    }
    if y1 > dh - 1 {
        skip += (sh + y0 - dh) * sw;
        y1 = dh - 1;
    }
    if y0 < 0 {
        y0 = 0;
    }
    let count = x1 - x0 + 1;
    let mut d_at = (x0 + (dh - (y1 + 1)) * dw) as isize;
    let mut s_at = skip as isize;
    if dw > 0 && (dw as usize) * 4 > MAX_ALLOC {
        return Err(Fault::BadSize);
    }
    // the two depth rows, made whether or not they are used
    let mut back = si_malloc(dw << 2);
    let mut layer = si_malloc(dw << 2);
    if y1 < y0 {
        return Ok(());
    }
    if !(0..=8).contains(&mode) || count <= 0 {
        return Ok(());
    }
    // every mode goes through all eight planes
    let sp: Vec<&[u8]> = src.planes.iter().map(|p| p.as_deref().ok_or(Fault::NullPlane)).collect::<Result<_, _>>()?;
    if dst.planes.iter().any(|p| p.is_none()) {
        return Err(Fault::NullPlane);
    }
    let rows = (y1 - y0 + 1) as isize;
    let (s_last, d_last) = (s_at + (rows - 1) * sw as isize + count as isize, d_at + (rows - 1) * dw as isize + count as isize);
    if s_at < 0 || d_at < 0 || sp.iter().any(|p| (p.len() as isize) < s_last) || dst.planes.iter().any(|p| (p.as_ref().unwrap().len() as isize) < d_last) {
        return Err(Fault::ShortPlane);
    }
    let [dr, dg, db, da] = &mut dst.planes;
    let mut dp: [&mut [u8]; 4] = [dr.as_mut().unwrap(), dg.as_mut().unwrap(), db.as_mut().unwrap(), da.as_mut().unwrap()];
    let (half, one, full) = (F80::from_f64(0.5), F80::ONE, F80::from_f64(255.0));
    let f = F80::from_f32(factor);
    let far = f32::from_bits(0xc7c3_5000); // −100000
    let count = count as usize;
    for _ in 0..rows {
        let (s0, d0) = (s_at as usize, d_at as usize);
        let with_depth = depth.is_some() && (mode == 1 || mode == 5);
        if with_depth {
            let z = depth.as_mut().unwrap();
            let (bk, ly) = (back.as_mut().ok_or(Fault::NullPlane)?, layer.as_mut().ok_or(Fault::NullPlane)?);
            let got_back = float_fread(bk, dw as usize, &mut z.back);
            let asked = if mode == 1 { sw } else { dw };
            if asked < 0 || asked > dw {
                return Err(Fault::BadSize); // more values than the row buffer holds
            }
            let got_layer = float_fread(ly, asked as usize, &mut z.layer);
            if mode == 1 {
                for i in sw.max(0) as usize..count {
                    set_f32(ly, i, far);
                }
            }
            if got_back == 0 {
                (0..count).for_each(|i| set_f32(bk, i, far));
            }
            if got_layer == 0 {
                (0..count).for_each(|i| set_f32(ly, i, far));
            }
        }
        for i in 0..count {
            let s = [sp[0][s0 + i], sp[1][s0 + i], sp[2][s0 + i], sp[3][s0 + i]];
            let d = [dp[0][d0 + i], dp[1][d0 + i], dp[2][d0 + i], dp[3][d0 + i]];
            let mut put = |c: usize, v: u8| dp[c][d0 + i] = v;
            // float((255 − a) / 255)
            let through = |a: u8| F80::from_f32((F80::from_i32((!a) as i32) / full).to_f32());
            if with_depth {
                let (bk, ly) = (back.as_mut().unwrap(), layer.as_ref().unwrap());
                let behind = F80::from_f32(f32_at(ly, i)).lt(F80::from_f32(f32_at(bk, i)));
                let (under, over, k) = if behind { (s, d, through(d[3])) } else { (d, s, through(s[3])) };
                if mode == 1 {
                    for c in 0..4 {
                        put(c, cap((b(under[c]) * k + b(over[c]) + half).to_i32(Round::Zero)));
                    }
                } else {
                    let rest = F80::from_f32((one - k).to_f32());
                    for c in 0..4 {
                        put(c, cap((b(under[c]) * k + b(over[c]) * rest).to_i32(Round::Zero)));
                    }
                }
                if !behind {
                    let v = f32_at(ly, i);
                    set_f32(bk, i, si_x87::fmov(v));
                }
                continue;
            }
            match mode {
                0 => (0..4).for_each(|c| put(c, s[c])),
                1 => {
                    if s[3] != 0 {
                        let k = through(s[3]);
                        for c in 0..4 {
                            put(c, cap((b(d[c]) * k + b(s[c]) + half).to_i32(Round::Zero)));
                        }
                    }
                }
                2 => {
                    if s != [0, 0, 0, 0] {
                        let k = through(s[3]);
                        for c in 0..4 {
                            let t = (b(d[c]) * k + b(s[c]) + half).to_i32(Round::Zero);
                            put(c, cap((F80::from_i32(t) * f + (one - f) * b(d[c])).to_i32(Round::Zero)));
                        }
                    }
                }
                3 => (0..4).for_each(|c| put(c, s[c].max(d[c]))),
                4 => {
                    if s[3] != 0 {
                        for c in 0..4 {
                            let v = 255 - (s[c] as i32 * s[3] as i32) / 255;
                            put(c, v.min(255 - d[c] as i32) as u8);
                        }
                    }
                }
                5 => {
                    let k = through(s[3]);
                    let rest = F80::from_f32((one - k).to_f32());
                    for c in 0..4 {
                        put(c, cap((b(d[c]) * k + b(s[c]) * rest).to_i32(Round::Zero)));
                    }
                }
                6 => {
                    if s[3] != 0 {
                        let ka = F80::from_f32((b(s[3]) / full).to_f32());
                        for c in 0..4 {
                            put(c, cap((b(d[c]) + b(s[c]) * f * ka + half).to_i32(Round::Zero)));
                        }
                    }
                }
                7 => {
                    if d[0] as i32 + d[1] as i32 + d[2] as i32 <= s[0] as i32 + s[1] as i32 + s[2] as i32 {
                        (0..4).for_each(|c| put(c, s[c]));
                    }
                }
                _ => (0..4).for_each(|c| put(c, s[c].abs_diff(d[c]))),
            }
        }
        if with_depth {
            // the destination's depth row, all of it, big-endian
            let (z, bk) = (depth.as_mut().unwrap(), back.as_ref().unwrap());
            for v in bk.chunks_exact(4).take(dw as usize) {
                z.out.extend(v.iter().rev());
            }
        }
        s_at += sw as isize;
        d_at += dw as isize;
    }
    Ok(())
}

/// Mixes `a` towards `b` in place: each byte of `a` becomes trunc(b + (a − b) × t)
/// [soft 0x085c9760 pictureFade].
///
/// Red, green and blue always (read without a test); alpha when both pictures have it. The
/// pictures must have the same width and height: otherwise the original shows
/// [`FADE_MESSAGE`] and returns 0 (`Ok(false)` here). The low byte of the truncated value is
/// stored, with no limit: a `t` outside 0…1 wraps.
pub fn picture_fade(a: &mut Picture, b_pic: &Picture, t: f32) -> Result<bool, Fault> {
    if (a.width, a.height) != (b_pic.width, b_pic.height) {
        return Ok(false);
    }
    let n = a.width as i32 * a.height as i32;
    if n == 0 {
        return Ok(true);
    }
    if n < 0 {
        return Err(Fault::BadSize); // the original counts down from a negative number
    }
    let n = n as usize;
    let t = F80::from_f32(t);
    let both_alpha = a.planes[3].is_some() && b_pic.planes[3].is_some();
    for c in 0..if both_alpha { 4 } else { 3 } {
        let s = crate::effect::plane(b_pic, c, n)?;
        crate::effect::plane(a, c, n)?;
        let d = a.planes[c].as_mut().unwrap();
        for i in 0..n {
            d[i] = (b(s[i]) + (b(d[i]) - b(s[i])) * t).to_i32(Round::Zero) as u8;
        }
    }
    Ok(true)
}

/// [`picture_fade`] as the routine behind the effects menu calls it
/// [soft 0x085c990c fadePicture]: the same work and the same message, but the result is 1
/// whatever happened.
pub fn fade_picture(a: &mut Picture, b_pic: &Picture, t: f32) -> Result<bool, Fault> {
    picture_fade(a, b_pic, t)?;
    Ok(true)
}

/// What [`picture_fade`] shows (status line, level 3) for pictures of different sizes.
pub const FADE_MESSAGE: &str = "The pictures have different sizes!";

/// Whether (`x`, `y`) lies inside the picture [soft 0x085ca20c superpose].
pub fn superpose(x: i32, y: i32, p: &Picture) -> bool {
    !(x < 0 || p.width as i32 <= x || y < 0 || p.height as i32 <= y)
}

/// Gives `dst` the alpha of `src` placed with its lower left corner at (`x`, `y`) from the
/// destination's bottom left, and 0 everywhere else [soft 0x085ca160 setAlpha]. Both alpha
/// planes are used without a test.
pub fn set_alpha(dst: &mut Picture, src: &Picture, x: i32, y: i32) -> Result<(), Fault> {
    let (w, h) = (dst.width as i32, dst.height as i32);
    if h <= 0 || w <= 0 {
        return Ok(());
    }
    let from = src.planes[3].as_deref().ok_or(Fault::NullPlane)?;
    let to = dst.planes[3].as_mut().ok_or(Fault::NullPlane)?;
    if to.len() < (w * h) as usize {
        return Err(Fault::ShortPlane);
    }
    let shift = h - (src.height as i32 + y);
    for row in 0..h {
        for col in 0..w {
            let (sx, sy) = (col - x, row - shift);
            to[(row * w + col) as usize] = if superpose(sx, sy, src) { *from.get((sx + src.width as i32 * sy) as usize).ok_or(Fault::ShortPlane)? } else { 0 };
        }
    }
    Ok(())
}

/// Softens the edge of a cut-out layer in place [soft 0x085c9d34 layerBlur].
///
/// For every pixel whose alpha is not 0, its colour becomes a mean over the square of
/// neighbours within `radius − 1` in each direction (clipped to the picture):
/// - a neighbour with alpha 0 and some colour adds colour × weight to the sum and 1 to the
///   count, weight = float((radius − distance) / radius) inside the circle of that radius
///   and 0 outside (distance as a square root in the register);
/// - a neighbour with alpha not 0 adds 1 to the count only;
/// - a neighbour with alpha 0 and no colour adds nothing.
/// The sums are unsigned integers: each term is added in the register and **truncated at
/// once**. The colour is sum / count, or 0 for a count of 0.
pub fn layer_blur(p: &mut Picture, radius: i32) -> Result<(), Fault> {
    let (w, h) = (p.width as i32, p.height as i32);
    if radius > 0 && (radius as usize).saturating_mul(radius as usize) * 4 > MAX_ALLOC {
        return Err(Fault::BadSize);
    }
    let r = radius.max(0) as usize;
    let rr = (F80::from_i32(radius.wrapping_mul(radius))).to_f32();
    let rf = F80::from_i32(radius).to_f32();
    let mut kernel = vec![0f32; r * r];
    for i in 0..r {
        for j in 0..r {
            let d = F80::from_i32((j * j + i * i) as i32);
            if d.lt(F80::from_f32(rr)) {
                kernel[i * r + j] = ((F80::from_f32(rf) - d.sqrt()) / F80::from_f32(rf)).to_f32();
            }
        }
    }
    if w <= 0 || h <= 0 {
        return Ok(());
    }
    let n = (w * h) as usize;
    for c in 0..4 {
        crate::effect::plane(p, c, n)?;
    }
    let [pr, pg, pb, pa] = &mut p.planes;
    let (pr, pg, pb, pa) = (pr.as_mut().unwrap(), pg.as_mut().unwrap(), pb.as_mut().unwrap(), pa.as_ref().unwrap());
    for y in 0..h {
        for x in 0..w {
            let at = (y * w + x) as usize;
            if pa[at] == 0 {
                continue;
            }
            let (mut count, mut sum) = (0u32, [0u32; 3]);
            let mut dy = (1 - radius).max(-y);
            while dy < radius && y + dy < h {
                let mut dx = (1 - radius).max(-x);
                while dx < radius && x + dx < w {
                    let q = (dx + x + w * (y + dy)) as usize;
                    if pa[q] != 0 {
                        count += 1;
                    } else if pr[q] != 0 || pg[q] != 0 || pb[q] != 0 {
                        let k = F80::from_f32(kernel[dy.unsigned_abs() as usize * r + dx.unsigned_abs() as usize]);
                        for (s, v) in sum.iter_mut().zip([pr[q], pg[q], pb[q]]) {
                            *s = (b(v) * k + F80::from_i64(*s as i64)).to_i64(Round::Zero) as u32;
                        }
                        count += 1;
                    }
                    dx += 1;
                }
                dy += 1;
            }
            let v = if count == 0 { [0; 3] } else { sum.map(|s| (s / count) as u8) };
            pr[at] = v[0];
            pg[at] = v[1];
            pb[at] = v[2];
        }
    }
    Ok(())
}

/// Copies into `out` the colour of the pixels on the edge of a mask
/// [soft 0x085c9934 layerGetContour]: for the compositing dialog's edge blur.
///
/// `mask` has one byte per pixel of `src` (0 = outside). All pixels but the last column and
/// the last row are visited in order; each is compared with its right and lower neighbours
/// and, when `neighbours` is 4, with the lower right and (from the second row on) the upper
/// right ones. For a pair with one pixel in the mask and one outside, where the brightness
/// (red + green + blue) of the **outside** one exceeds the inside one's by more than
/// `threshold`, the outside pixel's colour is copied from `src` to `out`.
///
/// The walk's index is never set back at the end of a row: it advances by width − 1 per row,
/// so each row of the walk starts one pixel further left than the picture's row and the
/// last rows of the picture are never reached. Reproduced.
pub fn layer_get_contour(src: &Picture, out: &mut Picture, mask: &[u8], threshold: i32, neighbours: i32) -> Result<(), Fault> {
    let (w, h) = (src.width as i32, src.height as i32);
    if h - 1 <= 0 || w - 1 <= 0 {
        return Ok(());
    }
    let n = (w * h) as usize;
    for c in 0..3 {
        crate::effect::plane(src, c, n)?;
        crate::effect::plane(out, c, n)?;
    }
    if mask.len() < n {
        return Err(Fault::ShortPlane);
    }
    let s: Vec<&[u8]> = (0..3).map(|c| src.planes[c].as_deref().unwrap()).collect();
    let [o0, o1, o2, _] = &mut out.planes;
    let mut o: [&mut [u8]; 3] = [o0.as_mut().unwrap(), o1.as_mut().unwrap(), o2.as_mut().unwrap()];
    let light = |i: usize| s[0][i] as i32 + s[1][i] as i32 + s[2][i] as i32;
    let mut copy = |i: usize| {
        for c in 0..3 {
            o[c][i] = s[c][i];
        }
    };
    // the running indices are never set back at a row's end: pixel k of the walk is index k,
    // so the walk drifts one pixel to the left with every row
    let mut at = 0usize;
    let w = w as usize;
    for row in 0..h - 1 {
        for _ in 0..w - 1 {
            let (right, below) = (at + 1, at + w);
            if below >= n || right >= n {
                return Err(Fault::ShortPlane);
            }
            let (here, r_in, b_in) = (mask[at] != 0, mask[right] != 0, mask[below] != 0);
            let (l0, l1, l2) = (light(at), light(right), light(below));
            if !here {
                if r_in && threshold < l0 - l1 {
                    copy(at);
                }
                if b_in && threshold < l0 - l2 {
                    copy(at);
                }
            } else {
                if !r_in && threshold < l1 - l0 {
                    copy(right);
                }
                if !b_in && threshold < l2 - l0 {
                    copy(below);
                }
            }
            if neighbours == 4 {
                let diag = at + w + 1;
                if diag >= n {
                    return Err(Fault::ShortPlane);
                }
                let ld = light(diag);
                if !here {
                    if mask[diag] != 0 && threshold < l0 - ld {
                        copy(at);
                    }
                } else if mask[diag] == 0 && threshold < ld - l0 {
                    copy(diag);
                }
                if row > 0 {
                    let up = at + 1 - w;
                    let lu = light(up);
                    if !here {
                        if mask[up] != 0 && threshold < l0 - lu {
                            copy(at);
                        }
                    } else if mask[up] == 0 && threshold < lu - l0 {
                        copy(up);
                    }
                }
            }
            at += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pic(w: i16, h: i16, rgb: u8, a: u8) -> Picture {
        let n = w as usize * h as usize;
        Picture { width: w, height: h, ratio: 1.0, fields: 3, pad: 0, planes: [Some(vec![rgb; n]), Some(vec![rgb; n]), Some(vec![rgb; n]), Some(vec![a; n])] }
    }

    #[test]
    fn the_source_is_placed_from_the_bottom_left() {
        let mut dst = pic(3, 3, 0, 0);
        let src = pic(1, 1, 9, 255);
        picture_composite(&mut dst, &src, 0, 0, 0, 1.0, None).unwrap();
        // y = 0 is the last row in memory
        assert_eq!(dst.planes[0].as_deref(), Some(&[0u8, 0, 0, 0, 0, 0, 9, 0, 0][..]));
        // off the picture: nothing happens
        let before = dst.clone();
        picture_composite(&mut dst, &src, 3, 0, 0, 1.0, None).unwrap();
        assert_eq!(dst, before);
    }

    #[test]
    fn over_keeps_what_shows_through() {
        let mut dst = pic(1, 1, 100, 255);
        let src = pic(1, 1, 50, 128);
        picture_composite(&mut dst, &src, 0, 0, 1, 1.0, None).unwrap();
        // 100 × float(127 / 255) + 50 + 0.5 = 100.3
        assert_eq!(dst.planes[0].as_deref(), Some(&[100u8][..]));
        assert_eq!(dst.planes[3].as_deref(), Some(&[255u8][..]));
        // a source without alpha is not laid over at all
        let mut dst = pic(1, 1, 100, 255);
        picture_composite(&mut dst, &pic(1, 1, 50, 0), 0, 0, 1, 1.0, None).unwrap();
        assert_eq!(dst.planes[0].as_deref(), Some(&[100u8][..]));
    }

    #[test]
    fn maximum_difference_and_lighter() {
        let src = pic(1, 1, 60, 10);
        let mut d = pic(1, 1, 200, 5);
        picture_composite(&mut d, &src, 0, 0, 3, 1.0, None).unwrap();
        assert_eq!((d.planes[0].as_ref().unwrap()[0], d.planes[3].as_ref().unwrap()[0]), (200, 10));
        let mut d = pic(1, 1, 200, 5);
        picture_composite(&mut d, &src, 0, 0, 8, 1.0, None).unwrap();
        assert_eq!((d.planes[0].as_ref().unwrap()[0], d.planes[3].as_ref().unwrap()[0]), (140, 5));
        let mut d = pic(1, 1, 200, 5);
        picture_composite(&mut d, &src, 0, 0, 7, 1.0, None).unwrap();
        assert_eq!(d.planes[0].as_ref().unwrap()[0], 200); // the destination is the lighter one
    }

    #[test]
    fn depth_decides_who_is_in_front() {
        let src = pic(1, 1, 50, 255);
        let be = |v: f32| v.to_be_bytes().to_vec();
        // the layer is nearer (larger): it covers, and its depth is written out
        let (back, layer) = (be(-5.0), be(-1.0));
        let mut z = Depth { back: Stream::new(&back), layer: Stream::new(&layer), out: Vec::new() };
        let mut d = pic(1, 1, 100, 255);
        picture_composite(&mut d, &src, 0, 0, 1, 1.0, Some(&mut z)).unwrap();
        assert_eq!(d.planes[0].as_ref().unwrap()[0], 50);
        assert_eq!(z.out, be(-1.0));
        // the layer is behind an opaque destination: nothing shows, the depth stays
        let (back, layer) = (be(-1.0), be(-5.0));
        let mut z = Depth { back: Stream::new(&back), layer: Stream::new(&layer), out: Vec::new() };
        let mut d = pic(1, 1, 100, 255);
        picture_composite(&mut d, &src, 0, 0, 1, 1.0, Some(&mut z)).unwrap();
        assert_eq!(d.planes[0].as_ref().unwrap()[0], 100);
        assert_eq!(z.out, be(-1.0));
    }

    #[test]
    fn fade_and_its_wrapper() {
        let mut a = pic(2, 1, 200, 255);
        let b_pic = pic(2, 1, 100, 255);
        assert_eq!(picture_fade(&mut a, &b_pic, 0.25), Ok(true));
        assert_eq!(a.planes[0].as_deref(), Some(&[125u8, 125][..]));
        let mut other = pic(1, 1, 0, 0);
        assert_eq!(picture_fade(&mut other, &b_pic, 0.5), Ok(false));
        assert_eq!(fade_picture(&mut other, &b_pic, 0.5), Ok(true));
    }

    #[test]
    fn set_alpha_and_blur() {
        let mut dst = pic(3, 2, 0, 7);
        let src = pic(1, 1, 0, 200);
        set_alpha(&mut dst, &src, 1, 0).unwrap();
        assert_eq!(dst.planes[3].as_deref(), Some(&[0u8, 0, 0, 0, 200, 0][..]));
        assert!(superpose(0, 0, &src) && !superpose(1, 0, &src) && !superpose(0, -1, &src));
        // one covered pixel between two coloured, uncovered ones: radius 2 gives each a
        // weight of one half, and the pixel itself counts without colour
        let mut p = Picture { width: 3, height: 1, ratio: 1.0, fields: 3, pad: 0, planes: [Some(vec![100, 9, 200]), Some(vec![100, 9, 200]), Some(vec![100, 9, 200]), Some(vec![0, 255, 0])] };
        layer_blur(&mut p, 2).unwrap();
        assert_eq!(p.planes[0].as_deref(), Some(&[100u8, 50, 200][..])); // (50 + 100) / 3
    }
}
