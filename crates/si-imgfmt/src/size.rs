// SPDX-License-Identifier: MIT
//! Resizing a picture in memory: whole-number averaging and repetition, the bilinear zoom,
//! the area-weighted scale, the horizontal scale that makes pixels square, and the three
//! "integer" scales of the packed records (finding 0014, "Resizing").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `size.c` (0x085d9040–0x085dd281, 24 functions). The
//! file-to-file resizers of the same source file are in [`crate::zoom`].
//!
//! Conventions (those of [`crate::effect`]):
//! - A function returns `Ok(true)` where the original returns 1 and `Ok(false)` where it
//!   returns 0; in the second case the picture is left exactly as the original leaves it
//!   (planes already replaced stay replaced, a plane already freed is `None`).
//! - A [`Fault`] is returned where the original would use memory that is not its own (it reads
//!   the planes without testing their size) or trap on a division.
//! - The floating-point steps are computed on [`F80`], the x87 register value, with a rounding
//!   to `f32` exactly where the original stores to a float in memory. Which values stay in a
//!   register and which are stored was read from the instructions and is stated per function.
//! - New planes come from the program's allocator: a request for no bytes gives no plane (and
//!   the routines then return 0), and bytes the routine does not write keep the allocator's
//!   two marks (see [`crate::softpic`]).

use crate::effect::Fault;
use crate::softpic::si_malloc;
use si_picture::{Picture, PictureRgba, MAX_ALLOC};
use si_x87::{fmov, Round, F80};

/// The allocator, with this crate's limit on a single request in front of it.
pub(crate) fn alloc(n: i32) -> Result<Option<Vec<u8>>, Fault> {
    if n > 0 && n as usize > MAX_ALLOC {
        return Err(Fault::BadSize);
    }
    Ok(si_malloc(n))
}

fn half() -> F80 {
    F80::from_f64(0.5)
}

/// Reduces the picture by whole factors, each new pixel the mean of a `fx × fy` block
/// [soft 0x085d9040 SI_pictureAverage].
///
/// - New width = width / `fx`, new height = height / `fy` (integer division; a remainder of
///   columns or rows is dropped).
/// - The sums are kept in **16-bit** words and divided by `fx × fy` as a 16-bit product, both
///   signed: a block whose sum passes 32,767 (more than 128 full-white pixels) wraps and comes
///   out wrong. Reproduced.
/// - Each plane present is replaced in turn (red, green, blue, alpha). The size is set at the
///   end. If the new width is not positive the routine returns 0 at once; if the new height
///   is not positive it returns 0 at the first plane present.
pub fn average(p: &mut Picture, fx: i16, fy: i16) -> Result<bool, Fault> {
    if fx == 0 || fy == 0 {
        return Err(Fault::DivideError);
    }
    let nw = (p.width as i32 / fx as i32) as i16;
    let nh = (p.height as i32 / fy as i32) as i16;
    if alloc(2 * nw as i32)?.is_none() {
        return Ok(false);
    }
    let div = fx.wrapping_mul(fy) as i32;
    let mut sums = vec![0i16; nw as usize];
    for c in 0..4 {
        let Some(src) = p.planes[c].as_ref() else { continue };
        let Some(mut out) = alloc(nw as i32 * nh as i32)? else { return Ok(false) };
        let mut row = 0isize;
        let mut o = 0;
        for _ in 0..nh {
            sums.fill(0);
            for _ in 0..fy.max(0) {
                let mut at = row;
                for s in sums.iter_mut() {
                    for _ in 0..fx.max(0) {
                        let v = *usize::try_from(at).ok().and_then(|i| src.get(i)).ok_or(Fault::ShortPlane)?;
                        *s = s.wrapping_add(v as i16);
                        at += 1;
                    }
                }
                row += p.width as isize;
            }
            for &s in &sums {
                if div == 0 {
                    return Err(Fault::DivideError);
                }
                out[o] = (s as i32 / div) as u8;
                o += 1;
            }
        }
        p.planes[c] = Some(out);
    }
    p.width = nw;
    p.height = nh;
    Ok(true)
}

/// Enlarges the picture by whole factors, each pixel repeated `fx` times and each row `fy`
/// times [soft 0x085d9b5c SI_pictureEnlarge].
///
/// The new width and height are 16-bit products (they wrap). Each plane present is replaced
/// in turn; a plane that cannot be allocated (a size of zero or below) ends the routine with
/// 0, the earlier planes already replaced and the size not yet set.
pub fn enlarge(p: &mut Picture, fx: i16, fy: i16) -> Result<bool, Fault> {
    let nw = fx.wrapping_mul(p.width);
    let nh = fy.wrapping_mul(p.height);
    for c in 0..4 {
        let Some(src) = p.planes[c].as_ref() else { continue };
        let Some(mut out) = alloc(nw as i32 * nh as i32)? else { return Ok(false) };
        let mut o = 0usize;
        let mut row = 0usize;
        for _ in 0..p.height.max(0) {
            let mut next = row;
            for _ in 0..fy.max(0) {
                let mut at = row;
                for _ in 0..p.width.max(0) {
                    let v = *src.get(at).ok_or(Fault::ShortPlane)?;
                    for _ in 0..fx.max(0) {
                        *out.get_mut(o).ok_or(Fault::BadSize)? = v;
                        o += 1;
                    }
                    at += 1;
                }
                next = at;
            }
            row = next;
        }
        p.planes[c] = Some(out);
    }
    p.width = nw;
    p.height = nh;
    Ok(true)
}

/// A plane with a border of one pixel all round, the edge pixels repeated
/// [soft 0x085d96cc channelExtend]: `(w + 2) × (h + 2)` bytes.
fn channel_extend(src: &[u8], w: i32, h: i32) -> Result<Option<Vec<u8>>, Fault> {
    let n = w + 2;
    let Some(mut dst) = alloc((h + 2).wrapping_mul(n))? else { return Ok(None) };
    if h < 0 || w < 0 || (h > 0 && w == 0) {
        // the original copies with a negative length or reads in front of the plane
        return Err(Fault::BadSize);
    }
    let (w, h, n) = (w as usize, h as usize, n as usize);
    if src.len() < w * h {
        return Err(Fault::ShortPlane);
    }
    for r in 0..h {
        let row = &src[r * w..(r + 1) * w];
        let d = &mut dst[(r + 1) * n..(r + 2) * n];
        d[0] = row[0];
        d[1..=w].copy_from_slice(row);
        d[w + 1] = row[w - 1];
    }
    dst.copy_within(n..2 * n, 0);
    dst.copy_within(h * n..(h + 1) * n, (h + 1) * n);
    Ok(Some(dst))
}

/// One plane through the bilinear zoom [soft 0x085d979c channelBilinearZoom].
fn channel_bilinear_zoom(width: i16, height: i16, plane: &mut Option<Vec<u8>>, zx: f32, zy: f32) -> Result<bool, Fault> {
    let (w, h) = (width as i32, height as i32);
    let Some(ext) = channel_extend(plane.as_deref().ok_or(Fault::NullPlane)?, w, h)? else { return Ok(false) };
    // the old plane is given back before the new one is asked for
    *plane = None;
    let nw = (F80::from_i32(w) * F80::from_f32(zx)).to_i32(Round::Zero);
    let nh = (F80::from_i32(h) * F80::from_f32(zy)).to_i32(Round::Zero);
    let dy = (F80::ONE / F80::from_f32(zy)).to_f32();
    let dx = (F80::ONE / F80::from_f32(zx)).to_f32();
    let Some(mut out) = alloc(nw.wrapping_mul(nh))? else { return Ok(false) };
    let stride = (w + 2) as i64;
    let mut y = ((F80::from_i32(h) - F80::from_i32(nh.wrapping_sub(1)) * F80::from_f32(dy)) * half()).to_f32();
    let mut o = 0usize;
    for _ in 0..nh.max(0) {
        let mut x = ((F80::from_i32(w) - F80::from_i32(nw.wrapping_sub(1)) * F80::from_f32(dx)) * half()).to_f32();
        let y_next = (F80::from_f32(y) + F80::from_f32(dy)).to_f32();
        if nw > 0 {
            let iy = F80::from_f32(y).to_i32(Round::Zero);
            let fy = F80::from_f32(y) - F80::from_i32(iy);
            let up = fy.gt(half());
            for _ in 0..nw {
                let xr = F80::from_f32(x);
                let ix = xr.to_i32(Round::Zero);
                let fx = xr - F80::from_i32(ix);
                let right = fx.gt(half());
                // the sample's own pixel in the bordered plane (the original's address
                // arithmetic is 32-bit; an index outside the plane is a fault here)
                let base = (iy as i64 + 1) * stride + ix as i64 + 1;
                let (a, c) = if right { (F80::from_f64(1.5) - fx, fx - half()) } else { (half() - fx, fx + half()) };
                let (b, d) = if up { (F80::from_f64(1.5) - fy, fy - half()) } else { (half() - fy, fy + half()) };
                // the four neighbours: this pixel's row or the one above/below, this column or
                // the one beside it, by which half of the pixel the sample falls in
                let col = if right { 0 } else { -1 };
                let top = if up { 0 } else { -stride };
                let at = [base + top + col, base + top + col + 1, base + top + stride + col, base + top + stride + col + 1];
                let weight = [(a * b).to_f32(), (b * c).to_f32(), (a * d).to_f32(), (c * d).to_f32()];
                let mut sum = F80::ZERO;
                for k in 0..4 {
                    let v = *usize::try_from(at[k]).ok().and_then(|i| ext.get(i)).ok_or(Fault::ShortPlane)?;
                    // the byte is read as a signed char
                    let term = F80::from_i32(v as i8 as i32) * F80::from_f32(weight[k]);
                    sum = if k == 0 { term } else { sum + term };
                }
                out[o] = sum.to_i32(Round::Zero) as u8;
                o += 1;
                x = (F80::from_f32(x) + F80::from_f32(dx)).to_f32();
            }
        }
        y = y_next;
    }
    *plane = Some(out);
    Ok(true)
}

/// Resizes by real factors with bilinear interpolation [soft 0x085d95ec SI_pictureBilinearZoom]
/// (its two helpers are the bordered copy [soft 0x085d96cc channelExtend] and the per-plane
/// loop [soft 0x085d979c channelBilinearZoom]).
///
/// - New width = trunc(width × `zx`), new height = trunc(height × `zy`).
/// - Sample positions run over the source in steps of `1/zx` and `1/zy` (each rounded to
///   float), centred: the first is `(size − (new size − 1) × step) × 0.5`. Positions are kept
///   in floats and advance by repeated addition, rounded each time.
/// - A sample at integer part `i` and fraction `f` mixes pixel `i` with pixel `i − 1` (when
///   `f ≤ 0.5`) or `i + 1` (when `f > 0.5`), with weights `0.5 ± f` and `0.5 ∓ f` (or `1.5 − f`
///   and `f − 0.5`), each of the four products rounded to float. The plane is first copied
///   with its edge pixels repeated once all round.
/// - **Pixel bytes are read as signed**: on this build `char` is signed, so a value of 128 or
///   more enters the sum as value − 256. The sum is truncated towards zero and its low byte
///   stored, which makes the result right again whenever the four weights sum to exactly 1 and
///   wrong by one where the truncation falls on the other side.
/// - The planes are done in the order red, green, blue, alpha; one that fails (no pixels to
///   make) ends the routine with 0 **after its old plane was freed**, the size unchanged.
pub fn bilinear_zoom(p: &mut Picture, zx: f32, zy: f32) -> Result<bool, Fault> {
    let nw = (F80::from_i32(p.width as i32) * F80::from_f32(zx)).to_i32(Round::Zero) as i16;
    let nh = (F80::from_i32(p.height as i32) * F80::from_f32(zy)).to_i32(Round::Zero) as i16;
    for c in 0..4 {
        if p.planes[c].is_some() && !channel_bilinear_zoom(p.width, p.height, &mut p.planes[c], zx, zy)? {
            return Ok(false);
        }
    }
    p.width = nw;
    p.height = nh;
    Ok(true)
}

/// A byte of a plane for the area-weighted scale. The original reads one pixel past the end
/// of a row (and so, in the last row, past the end of the plane) whenever rounding leaves a
/// destination pixel a hair short of full; the weight it gives that byte is the shortfall,
/// some 1e-7. Rust cannot read there: the byte is taken as 0.
fn lenient(src: &[u8], at: isize) -> Result<u8, Fault> {
    match usize::try_from(at) {
        Ok(i) => Ok(src.get(i).copied().unwrap_or(0)),
        Err(_) => Err(Fault::ShortPlane),
    }
}

/// One plane through the area-weighted scale [soft 0x085da01c channelScale].
///
/// A source pixel is `zx` wide and `zy` high in destination pixels. Each destination pixel
/// collects source pixels until the widths collected reach 1, row by row until the heights
/// reach 1; a source pixel's remaining part carries over to the next destination pixel.
///
/// State and rounding, as the instructions have it:
/// - the weight in use and the part left over of the current source pixel are register values
///   inside one destination pixel and are rounded to float at the start of each destination
///   pixel (x) and of each destination row (y);
/// - a new part `1 − collected` is rounded to float when it is cut off a larger remainder;
/// - the tolerance 0.001 decides both "this source pixel is used up" (`left ≤ 0.001`) and
///   "the remainder is larger than what is still needed" (`needed + 0.001 < left`);
/// - the sum of a destination pixel is a float in memory: every term `value × wy × wx` is
///   added and rounded in turn; the pixel is the sum truncated.
fn channel_scale(width: i16, height: i16, plane: &mut Option<Vec<u8>>, zx: f32, zy: f32) -> Result<bool, Fault> {
    let nw = (F80::from_i32(width as i32) * F80::from_f32(zx)).to_i32(Round::Zero) as i16;
    let nh = (F80::from_i32(height as i32) * F80::from_f32(zy)).to_i32(Round::Zero) as i16;
    let Some(mut out) = alloc(nw as i32 * nh as i32)? else { return Ok(false) };
    if nw < 0 || nh < 0 {
        // the original counts both down as unsigned 16-bit numbers and writes past the plane
        return Err(Fault::BadSize);
    }
    let src = plane.as_deref().ok_or(Fault::NullPlane)?;
    let (one, eps) = (F80::ONE, F80::from_f64(0.001));
    let (zxr, zyr) = (F80::from_f32(zx), F80::from_f32(zy));
    // a factor above 1: the first part of a source pixel is 1 and the rest is left over
    let start = |z: F80| if one.lt(z) { (one, F80::from_f32((z - one).to_f32())) } else { (z, F80::ZERO) };
    let (mut yw, mut y_left) = if one.lt(zyr) { (1.0f32, F80::from_f32((zyr - one).to_f32())) } else { (fmov(zy), F80::ZERO) };
    let mut row_at = 0isize;
    let mut o = 0usize;
    for _ in 0..nh {
        let (mut xw, mut x_left) = start(zxr);
        let (yw_row, y_left_row) = (yw, y_left.to_f32());
        let mut pixel_at = row_at;
        let mut rows = 0i16;
        for _ in 0..nw {
            let mut sum = 0f32;
            yw = yw_row;
            let (xw_start, x_left_start) = (xw.to_f32(), x_left.to_f32());
            let mut y_rest = F80::from_f32(y_left_row);
            let mut y_got = F80::ZERO;
            let mut column = pixel_at;
            rows = 0;
            let mut used;
            loop {
                y_got = y_got + F80::from_f32(yw);
                let (mut w, mut rest) = (F80::from_f32(xw_start), F80::from_f32(x_left_start));
                let mut x_got = F80::ZERO;
                let mut at = column;
                used = 0i32;
                loop {
                    let v = lenient(src, at)?;
                    sum = (F80::from_i32(v as i32) * F80::from_f32(yw) * w + F80::from_f32(sum)).to_f32();
                    x_got = w + x_got;
                    let r = if eps.lt(rest) || eps.unordered(rest) {
                        rest
                    } else {
                        at += 1;
                        used += 1;
                        zxr
                    };
                    let need = one - x_got;
                    if (need + eps).lt(r) {
                        w = F80::from_f32(need.to_f32());
                        rest = r - w;
                    } else {
                        w = r;
                        rest = F80::ZERO;
                    }
                    if !x_got.lt(one) {
                        break;
                    }
                }
                xw = w;
                x_left = rest;
                let r = if eps.lt(y_rest) || eps.unordered(y_rest) {
                    y_rest
                } else {
                    column += width as isize;
                    rows = rows.wrapping_add(1);
                    zyr
                };
                let need = one - y_got;
                if (need + eps).lt(r) {
                    yw = need.to_f32();
                    y_left = r - F80::from_f32(yw);
                } else {
                    yw = r.to_f32();
                    y_left = F80::ZERO;
                }
                if !one.gt(y_got) {
                    break;
                }
                y_rest = y_left;
            }
            out[o] = F80::from_f32(sum).to_i32(Round::Zero) as u8;
            o += 1;
            pixel_at += used as i16 as isize;
        }
        row_at += rows as isize * width as isize;
    }
    *plane = Some(out);
    Ok(true)
}

/// Resizes by real factors, each new pixel the area-weighted mean of the source pixels it
/// covers [soft 0x085d9eec SI_pictureScale] (per plane: [soft 0x085da01c channelScale], whose
/// rules are in the text above it in this file).
///
/// The factors are first adjusted so that the new size is a whole number:
/// `zx = float(float(width × sx + 0.5) / width)`, likewise `zy`; new width =
/// trunc(width × zx). The new **height** written to the record is trunc(height × zy) with `zy`
/// before its rounding to float, while the planes are made with the rounded `zy`; the two
/// agree except when the product falls within a rounding of a whole number.
pub fn scale(p: &mut Picture, sx: f32, sy: f32) -> Result<bool, Fault> {
    let (w, h) = (F80::from_i32(p.width as i32), F80::from_i32(p.height as i32));
    let a = (w * F80::from_f32(sx) + half()).to_f32();
    let b = (half() + h * F80::from_f32(sy)).to_f32();
    let zx = (F80::from_f32(a) / w).to_f32();
    let zy_wide = F80::from_f32(b) / h;
    let nw = (w * F80::from_f32(zx)).to_i32(Round::Zero) as i16;
    let zy = zy_wide.to_f32();
    let nh = (h * zy_wide).to_i32(Round::Zero) as i16;
    for c in 0..4 {
        if p.planes[c].is_some() && !channel_scale(p.width, p.height, &mut p.planes[c], zx, zy)? {
            return Ok(false);
        }
    }
    p.width = nw;
    p.height = nh;
    Ok(true)
}

/// Changes the width by `ratio`, area-weighted, and divides the pixel ratio by the factor
/// used [soft 0x085da2dc SI_scalePictureX]: how a picture with oblong pixels is made square
/// for display (the packed readers call it with the picture's ratio).
///
/// - New width = trunc(width × ratio + 0.5); the factor used is `z = float(new / width)`.
/// - Along a row each source pixel is `z` wide; a destination pixel collects until it has 1.
///   Here the weight and the remainder stay in registers for the whole row (they start
///   afresh at each row), there is **no tolerance**, and a row never reads past its end.
/// - **Red and green are summed in registers, blue and alpha in floats in memory** (the
///   compiler ran out of registers), so the same bytes can come out one apart between red or
///   green and blue or alpha. A pixel is trunc(sum + 0.5).
/// - Every row writes at most the new width; rows are written one after the other.
/// - All new planes are allocated first; if one cannot be (nothing to make), the others are
///   freed and the routine returns 0 with the picture untouched.
/// - At the end the planes present are replaced, the width is set and the ratio becomes
///   `ratio field / z`. The height is not touched.
pub fn scale_picture_x(p: &mut Picture, ratio: f32) -> Result<bool, Fault> {
    let w = F80::from_i32(p.width as i32);
    let nw = (w * F80::from_f32(ratio) + half()).to_i32(Round::Zero) as i16;
    let z = (F80::from_i32(nw as i32) / w).to_f32();
    let size = p.height as i32 * nw as i32;
    let has = [0, 1, 2, 3].map(|c| p.planes[c].is_some());
    let mut out: [Option<Vec<u8>>; 4] = [None, None, None, None];
    for c in 0..4 {
        if has[c] {
            match alloc(size)? {
                Some(v) => out[c] = Some(v),
                // On a failure the original frees the buffers of the planes before this
                // one through variables it set only for planes that exist. All requests are
                // the same size, so the first plane present is the one that fails: with red
                // present nothing is freed; with red absent a pointer never set is.
                None => return if c == 0 { Ok(false) } else { Err(Fault::Uninitialised) },
            }
        }
    }
    let any = has.iter().any(|&h| h);
    if any && (p.height < 0 || nw < 0) {
        return Err(Fault::BadSize);
    }
    if any {
        let n = p.width.max(0) as usize * p.height.max(0) as usize;
        for c in 0..4 {
            if has[c] && p.planes[c].as_ref().unwrap().len() < n {
                return Err(Fault::ShortPlane);
            }
        }
        let (one, zr) = (F80::ONE, F80::from_f32(z));
        let (mut sp, mut op) = (0usize, 0usize);
        for _ in 0..p.height {
            let (mut rest, mut weight) = if one.lt(zr) { (F80::from_f32((zr - one).to_f32()), one) } else { (F80::ZERO, zr) };
            let mut made = 0i32;
            let mut left = p.width;
            while left > 0 {
                let mut wide = [F80::ZERO; 2];
                let mut narrow = [0f32; 2];
                let mut got = F80::ZERO;
                loop {
                    for c in 0..4 {
                        if has[c] {
                            let term = F80::from_i32(p.planes[c].as_ref().unwrap()[sp] as i32) * weight;
                            if c < 2 {
                                wide[c] = wide[c] + term;
                            } else {
                                narrow[c - 2] = (term + F80::from_f32(narrow[c - 2])).to_f32();
                            }
                        }
                    }
                    got = weight + got;
                    let r = if rest.gt(F80::ZERO) || rest.unordered(F80::ZERO) {
                        rest
                    } else {
                        sp += 1;
                        left -= 1;
                        zr
                    };
                    let need = one - got;
                    if need.lt(r) {
                        weight = F80::from_f32(need.to_f32());
                        rest = r - weight;
                    } else {
                        weight = r;
                        rest = F80::ZERO;
                    }
                    if !got.lt(one) || left <= 0 {
                        break;
                    }
                }
                if made < nw as i32 {
                    made += 1;
                    for c in 0..4 {
                        if let Some(o) = out[c].as_mut() {
                            let v = if c < 2 { wide[c] + half() } else { half() + F80::from_f32(narrow[c - 2]) };
                            o[op] = v.to_i32(Round::Zero) as u8;
                        }
                    }
                    op += 1;
                }
            }
        }
    }
    for c in 0..4 {
        if has[c] {
            p.planes[c] = out[c].take();
        }
    }
    p.width = nw;
    p.ratio = (F80::from_f32(p.ratio) / F80::from_f32(z)).to_f32();
    Ok(true)
}

/// What the three integer scales share: nothing is done for factors of exactly 1 and 1;
/// otherwise the new size is trunc(size × factor) and the source is read at whole steps of
/// trunc(1 / factor + 0.5) pixels and rows.
fn integer_steps(w: i16, h: i16, zx: f32, zy: f32) -> Option<(i32, i32, i32, i32)> {
    let (zxr, zyr, one) = (F80::from_f32(zx), F80::from_f32(zy), F80::ONE);
    if zxr.eq(one) && zyr.eq(one) {
        return None;
    }
    let nw = (F80::from_i32(w as i32) * zxr).to_i32(Round::Zero);
    let nh = (F80::from_i32(h as i32) * zyr).to_i32(Round::Zero);
    let xs = (one / zxr + half()).to_i32(Round::Zero);
    let ys = (one / zyr + half()).to_i32(Round::Zero);
    Some((nw, nh, xs, ys))
}

/// Copies every `step`-th unit of `per` bytes: `rows × cols` units from `src`, whose rows
/// are `src_row` units apart.
fn pick(src: &[u8], per: usize, cols: i32, rows: i32, xs: i32, row_step: i32) -> Result<Option<Vec<u8>>, Fault> {
    let bytes = cols.wrapping_mul(rows).wrapping_mul(per as i32);
    let Some(mut out) = alloc(bytes)? else {
        // a byte count that wrapped to nothing while there are pixels to write: the original
        // writes through a null pointer
        return if cols > 0 && rows > 0 { Err(Fault::BadSize) } else { Ok(None) };
    };
    let mut o = 0usize;
    for y in 0..rows.max(0) {
        let mut at = y as i64 * row_step as i64;
        for _ in 0..cols.max(0) {
            let i = usize::try_from(at).map_err(|_| Fault::ShortPlane)? * per;
            let unit = src.get(i..i + per).ok_or(Fault::ShortPlane)?;
            out.get_mut(o..o + per).ok_or(Fault::BadSize)?.copy_from_slice(unit);
            o += per;
            at += xs as i64;
        }
    }
    Ok(Some(out))
}

/// Reduces a packed picture by taking every n-th pixel of every m-th row
/// [soft 0x085dcde0 SI_pictureRGBAIntegerScale]: n = trunc(1 / `zx` + 0.5), m likewise.
///
/// `dst` gets the new width, height and pixels; its other fields are left. Factors of
/// exactly 1 and 1 do nothing at all. Nothing is averaged. For a factor above 1 the step is
/// 1 (or 0 from a factor of 2 up) while the new size grows, so the routine reads past each
/// row and past the buffer: only reductions work, and anything else is a [`Fault`] here.
/// A new size of nothing leaves `dst` without pixels.
pub fn rgba_integer_scale(dst: &mut PictureRgba, src: &PictureRgba, zx: f32, zy: f32) -> Result<(), Fault> {
    let Some((nw, nh, xs, ys)) = integer_steps(src.width, src.height, zx, zy) else { return Ok(()) };
    let data = pick(&src.data, 4, nw, nh, xs, (src.width as i32).wrapping_mul(ys))?;
    dst.width = nw as i16;
    dst.height = nh as i16;
    dst.data = data.unwrap_or_default();
    Ok(())
}

/// The same for a record with one byte a pixel [soft 0x085dcf64 SI_pictureBWIntegerScale].
pub fn bw_integer_scale(dst: &mut PictureRgba, src: &PictureRgba, zx: f32, zy: f32) -> Result<(), Fault> {
    let Some((nw, nh, xs, ys)) = integer_steps(src.width, src.height, zx, zy) else { return Ok(()) };
    let data = pick(&src.data, 1, nw, nh, xs, (src.width as i32).wrapping_mul(ys))?;
    dst.width = nw as i16;
    dst.height = nh as i16;
    dst.data = data.unwrap_or_default();
    Ok(())
}

/// The same for a "scaled" record, whose buffer holds half the width by half the height the
/// record states [soft 0x085dd0d0 SI_pictureScaledRGBAIntegerScale].
///
/// `dst` gets the full new width and height, trunc(size × factor); its buffer holds
/// (new width / 2) × (new height / 2) pixels (16-bit divisions towards zero), read from the
/// source buffer whose rows are width / 2 pixels long.
pub fn scaled_rgba_integer_scale(dst: &mut PictureRgba, src: &PictureRgba, zx: f32, zy: f32) -> Result<(), Fault> {
    let Some((nw, nh, xs, ys)) = integer_steps(src.width, src.height, zx, zy) else { return Ok(()) };
    let half_w = (src.width / 2) as i32;
    let (cols, rows) = ((nw as i16 / 2) as i32, (nh as i16 / 2) as i32);
    let data = pick(&src.data, 4, cols, rows, xs, half_w.wrapping_mul(ys))?;
    dst.width = nw as i16;
    dst.height = nh as i16;
    dst.data = data.unwrap_or_default();
    Ok(())
}

/// A packed picture with a border of one pixel all round, the edge pixels repeated
/// [soft 0x085dccf0 pictureExtent]: `(w + 2) × (h + 2)` pixels of four bytes. The same as
/// [`channel_extend`] with a pixel for a byte.
pub fn picture_extent(src: &[u8], w: i32, h: i32) -> Result<Option<Vec<u8>>, Fault> {
    let n = w.wrapping_add(2);
    let Some(mut dst) = alloc(h.wrapping_add(2).wrapping_mul(n).wrapping_mul(4))? else { return Ok(None) };
    if h <= 0 || w <= 0 {
        return Err(Fault::BadSize);
    }
    let (w, h, n) = (w as usize, h as usize, n as usize);
    if src.len() < w * h * 4 {
        return Err(Fault::ShortPlane);
    }
    for r in 0..h {
        let row = &src[r * w * 4..(r + 1) * w * 4];
        let d = &mut dst[(r + 1) * n * 4..(r + 2) * n * 4];
        d[..4].copy_from_slice(&row[..4]);
        d[4..4 + w * 4].copy_from_slice(row);
        d[(w + 1) * 4..].copy_from_slice(&row[(w - 1) * 4..]);
    }
    dst.copy_within(n * 4..2 * n * 4, 0);
    dst.copy_within(h * n * 4..(h + 1) * n * 4, (h + 1) * n * 4);
    Ok(Some(dst))
}

/// Resizes a packed picture by real factors with bilinear interpolation, in place
/// [soft 0x085dc868 pictureRGBABilinearZoom]: the arithmetic of [`bilinear_zoom`] on the
/// three colour bytes of each pixel, the bordered copy being [soft 0x085dccf0 pictureExtent].
///
/// - Both factors exactly 1: nothing happens (a NaN does not count as 1).
/// - The first byte of each new pixel (the one before red) is not written: it keeps what the
///   allocator made it. The new width and height are stored as 16-bit numbers; the loops use
///   the 32-bit ones.
/// - The old buffer is given back before the new one is asked for, so a failure leaves the
///   record without pixels.
pub fn rgba_bilinear_zoom(p: &mut PictureRgba, zx: f32, zy: f32) -> Result<(), Fault> {
    if zx == 1.0 && zy == 1.0 {
        return Ok(());
    }
    let (w, h) = (p.width as i32, p.height as i32);
    let ext = picture_extent(&p.data, w, h);
    p.data = Vec::new();
    let Some(ext) = ext? else { return Ok(()) };
    let nw = (F80::from_i32(w) * F80::from_f32(zx)).to_i32(Round::Zero);
    let nh = (F80::from_i32(h) * F80::from_f32(zy)).to_i32(Round::Zero);
    p.width = nw as i16;
    p.height = nh as i16;
    let dx = (F80::ONE / F80::from_f32(zx)).to_f32();
    let dy = (F80::ONE / F80::from_f32(zy)).to_f32();
    let Some(mut out) = alloc(nw.wrapping_mul(nh).wrapping_mul(4))? else { return Ok(()) };
    let stride = (w + 2) as i64;
    let mut y = ((F80::from_i32(h) - F80::from_i32(nh.wrapping_sub(1)) * F80::from_f32(dy)) * half()).to_f32();
    let mut o = 0usize;
    for _ in 0..nh.max(0) {
        let mut x = ((F80::from_i32(w) - F80::from_i32(nw.wrapping_sub(1)) * F80::from_f32(dx)) * half()).to_f32();
        let y_next = (F80::from_f32(y) + F80::from_f32(dy)).to_f32();
        let iy = F80::from_f32(y).to_i32(Round::Zero);
        let fy = F80::from_f32(y) - F80::from_i32(iy);
        let up = fy.gt(half());
        for _ in 0..nw.max(0) {
            let xr = F80::from_f32(x);
            let ix = xr.to_i32(Round::Zero);
            let fx = xr - F80::from_i32(ix);
            let right = fx.gt(half());
            let base = (iy as i64 + 1) * stride + ix as i64 + 1;
            let (a, c) = if right { (F80::from_f64(1.5) - fx, fx - half()) } else { (half() - fx, fx + half()) };
            let (b, d) = if up { (F80::from_f64(1.5) - fy, fy - half()) } else { (half() - fy, fy + half()) };
            let col = if right { 0 } else { -1 };
            let top = if up { 0 } else { -stride };
            let at = [base + top + col, base + top + col + 1, base + top + stride + col, base + top + stride + col + 1];
            let weight = [(a * b).to_f32(), (b * c).to_f32(), (a * d).to_f32(), (c * d).to_f32()];
            for k in 0..3 {
                let mut sum = F80::ZERO;
                for j in 0..4 {
                    let i = usize::try_from(at[j]).ok().map(|i| i * 4 + 1 + k);
                    let v = *i.and_then(|i| ext.get(i)).ok_or(Fault::ShortPlane)?;
                    let term = F80::from_i32(v as i8 as i32) * F80::from_f32(weight[j]);
                    sum = if j == 0 { term } else { sum + term };
                }
                out[o + 1 + k] = sum.to_i32(Round::Zero) as u8;
            }
            o += 4;
            x = (F80::from_f32(x) + F80::from_f32(dx)).to_f32();
        }
        y = y_next;
    }
    p.data = out;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pic(w: i16, h: i16, v: &[u8]) -> Picture {
        Picture { width: w, height: h, ratio: 1.0, fields: 3, pad: 0, planes: [Some(v.to_vec()), None, None, None] }
    }

    #[test]
    fn average_is_a_truncated_mean_and_drops_the_remainder() {
        let mut p = pic(5, 2, &[1, 2, 10, 20, 99, 3, 5, 30, 41, 99]);
        assert_eq!(average(&mut p, 2, 2), Ok(true));
        assert_eq!((p.width, p.height), (2, 1));
        assert_eq!(p.planes[0].as_deref(), Some(&[2u8, 25][..])); // 11 / 4, 101 / 4
    }

    #[test]
    fn average_wraps_its_16_bit_sums() {
        // 144 pixels of 255 sum to 36720, which is −28816 in 16 bits: −28816 / 144 = −200 = 56
        let mut p = pic(12, 12, &[255; 144]);
        assert_eq!(average(&mut p, 12, 12), Ok(true));
        assert_eq!(p.planes[0].as_deref(), Some(&[56u8][..]));
    }

    #[test]
    fn enlarge_repeats_pixels_and_rows() {
        let mut p = pic(2, 1, &[7, 9]);
        assert_eq!(enlarge(&mut p, 2, 2), Ok(true));
        assert_eq!(p.planes[0].as_deref(), Some(&[7u8, 7, 9, 9, 7, 7, 9, 9][..]));
        assert_eq!(enlarge(&mut p, 0, 1), Ok(false)); // nothing to allocate
    }

    #[test]
    fn bilinear_zoom_by_one_is_the_identity_even_for_signed_bytes() {
        let v = [0u8, 1, 127, 128, 200, 255];
        let mut p = pic(3, 2, &v);
        assert_eq!(bilinear_zoom(&mut p, 1.0, 1.0), Ok(true));
        assert_eq!(p.planes[0].as_deref(), Some(&v[..]));
    }

    #[test]
    fn bilinear_zoom_doubles_a_step() {
        // samples at 0.25 and 0.75 of each pixel: three quarters of one and a quarter of its neighbour
        let mut p = pic(2, 1, &[0, 100]);
        assert_eq!(bilinear_zoom(&mut p, 2.0, 1.0), Ok(true));
        assert_eq!(p.planes[0].as_deref(), Some(&[0u8, 25, 75, 100][..]));
    }

    #[test]
    fn scale_halves_by_area() {
        let mut p = pic(4, 2, &[10, 20, 30, 40, 50, 60, 70, 80]);
        assert_eq!(scale(&mut p, 0.5, 0.5), Ok(true));
        assert_eq!((p.width, p.height), (2, 1));
        // the factor becomes (4 × 0.5 + 0.5) / 4 = 0.625, so a destination pixel takes 1.6
        // source pixels each way; the sums are truncated
        let v = p.planes[0].clone().unwrap();
        assert_eq!(v.len(), 2);
        assert!(v[0] < v[1]);
    }

    #[test]
    fn scale_picture_x_by_one_is_the_identity() {
        let v = [0u8, 1, 127, 128, 200, 255];
        let mut p = pic(3, 2, &v);
        p.ratio = 1.5;
        assert_eq!(scale_picture_x(&mut p, 1.0), Ok(true));
        assert_eq!(p.planes[0].as_deref(), Some(&v[..]));
        assert_eq!(p.ratio, 1.5);
    }

    #[test]
    fn scale_picture_x_divides_the_ratio_by_the_factor_used() {
        let mut p = pic(4, 1, &[0, 0, 200, 200]);
        p.ratio = 2.0;
        assert_eq!(scale_picture_x(&mut p, 2.0), Ok(true));
        assert_eq!(p.width, 8);
        assert_eq!(p.ratio, 1.0);
        assert_eq!(p.planes[0].as_deref(), Some(&[0u8, 0, 0, 0, 200, 200, 200, 200][..]));
    }

    #[test]
    fn integer_scale_takes_every_second_pixel() {
        let src = PictureRgba { width: 4, height: 2, ratio: 1.0, fields: 3, pad: 0, data: (0..32).collect() };
        let mut dst = PictureRgba::default();
        assert_eq!(rgba_integer_scale(&mut dst, &src, 0.5, 0.5), Ok(()));
        assert_eq!((dst.width, dst.height), (2, 1));
        assert_eq!(dst.data, vec![0, 1, 2, 3, 8, 9, 10, 11]);
        // factors of exactly one change nothing, and an enlargement reads outside the picture
        let mut same = PictureRgba { width: 9, ..PictureRgba::default() };
        assert_eq!(rgba_integer_scale(&mut same, &src, 1.0, 1.0), Ok(()));
        assert_eq!(same.width, 9);
        assert_eq!(rgba_integer_scale(&mut dst, &src, 1.5, 1.5), Err(Fault::ShortPlane));
    }
}
