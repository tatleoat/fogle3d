// SPDX-License-Identifier: MIT
//! Picture effects: mirror, quarter turns, negative, one channel shown as grey, contrast about
//! a pivot, a constant added per channel, and the file-to-file rotation (finding 0014).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `pic_effect.c` (0x085cad40–0x085cc724, 15 functions).
//!
//! Everything here is integer code except [`intensity_modify`] (one product and one sum in
//! the x87 register, then a truncating conversion) and two divisions and two `ceil`s in the
//! strip arithmetic of [`rotate_file`].
//!
//! Conventions of this module, shared by the other image-operation modules:
//! - Pictures are [`Picture`] records: planes red, green, blue, alpha, first row first.
//! - The original never tests the red, green and blue plane pointers, and it trusts the
//!   planes to hold `width × height` bytes. Where it would read or write through a null
//!   pointer or past a plane's end, these functions return a [`Fault`] and leave the picture
//!   untouched (they check before they write). Alpha is optional wherever the original tests
//!   it, and only there.
//! - Operations with a destination and a source record (`dst`, `src`) reproduce the original's
//!   two-pointer form. The original may be handed the same record twice; for all of them that
//!   gives the result of calling the function here with a copy of the record as `src`.
//! - Memory the original takes from `SI_malloc` and does not fill is zero here.

use si_picture::{encode_rows, Picture};
use si_x87::{fmov, Round, F80};

/// Why an operation stops here although the original would go on: what the original would do
/// next is outside the language (a null plane pointer, a plane too short for the picture's
/// size, a negative byte count, an integer division trap).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// A plane the original reads or writes without testing its pointer is absent.
    NullPlane,
    /// A plane holds fewer bytes than the picture's width × height.
    ShortPlane,
    /// A width, height or byte count is negative (or too large) where the original uses it as
    /// an unsigned size.
    BadSize,
    /// An integer division by zero (the processor's divide error).
    DivideError,
    /// The original uses a variable it never set (a pointer it frees, a count it loops on).
    Uninitialised,
    /// The original ends the program (`exit`).
    Exit,
}

/// `width × height` as the original computes it: a signed 32-bit product of the two 16-bit
/// fields.
pub(crate) fn area(p: &Picture) -> i32 {
    p.width as i32 * p.height as i32
}

/// Plane `c` of `p` for an operation over `n` bytes.
pub(crate) fn plane(p: &Picture, c: usize, n: usize) -> Result<&[u8], Fault> {
    match &p.planes[c] {
        None => Err(Fault::NullPlane),
        Some(v) if v.len() < n => Err(Fault::ShortPlane),
        Some(v) => Ok(&v[..n]),
    }
}

/// Checks that the planes an in-place operation touches are there: red, green and blue
/// always, alpha when present.
fn check_in_place(p: &Picture, n: usize) -> Result<(), Fault> {
    for c in 0..3 {
        plane(p, c, n)?;
    }
    if p.planes[3].is_some() {
        plane(p, 3, n)?;
    }
    Ok(())
}

/// The planes an in-place operation touches, after [`check_in_place`].
fn planes_mut(p: &mut Picture, n: usize) -> impl Iterator<Item = &mut [u8]> {
    p.planes.iter_mut().flatten().map(move |v| &mut v[..n])
}

/// Mirrors the picture left to right, in place [soft 0x085cad58 SI_pictureFlipX]
/// (`flipPictureX` [soft 0x085cad40 flipPictureX] only calls it).
///
/// In each row the first `width >> 1` pixels are exchanged with the last ones; the middle
/// column of an odd width stays. Alpha is mirrored when present. Size, ratio and `fields`
/// are untouched. A width below 2 or a height below 1 changes nothing.
pub fn flip_x(p: &mut Picture) -> Result<(), Fault> {
    if p.height <= 0 || p.width >> 1 <= 0 {
        return Ok(());
    }
    let n = area(p) as usize;
    check_in_place(p, n)?;
    let w = p.width as usize;
    for pl in planes_mut(p, n) {
        for row in pl.chunks_exact_mut(w) {
            row.reverse();
        }
    }
    Ok(())
}

/// Turns the picture upside down, in place [soft 0x085cae7c SI_pictureFlipY]
/// (`flipPictureY` [soft 0x085cae64 flipPictureY] only calls it).
///
/// Row `y` is exchanged with row `height − 1 − y` for the first `height >> 1` rows; the
/// middle row of an odd height stays. Alpha too when present.
pub fn flip_y(p: &mut Picture) -> Result<(), Fault> {
    if p.height >> 1 <= 0 || p.width <= 0 {
        return Ok(());
    }
    let n = area(p) as usize;
    check_in_place(p, n)?;
    let (w, h) = (p.width as usize, p.height as usize);
    for pl in planes_mut(p, n) {
        for y in 0..h / 2 {
            let (top, bottom) = pl.split_at_mut((h - 1 - y) * w);
            top[y * w..(y + 1) * w].swap_with_slice(&mut bottom[..w]);
        }
    }
    Ok(())
}

/// A quarter turn into fresh planes, shared by the two directions. `from(i, j)` is the index
/// in the old plane of the pixel that becomes column `j` of new row `i`.
fn quarter_turn(p: &mut Picture, from: impl Fn(usize, usize) -> usize) -> Result<(), Fault> {
    let (w, h) = (p.width as i32, p.height as i32);
    let n = w * h;
    let alpha = p.planes[3].is_some();
    let size = n.max(0) as usize;
    let mut fresh: Vec<Vec<u8>> = (0..if alpha { 4 } else { 3 }).map(|_| vec![0u8; size]).collect();
    if w > 0 && h > 0 {
        check_in_place(p, size)?;
        let (w, h) = (w as usize, h as usize);
        for (c, dst) in fresh.iter_mut().enumerate() {
            let src = p.planes[c].as_ref().unwrap();
            let mut k = 0;
            for i in 0..w {
                for j in 0..h {
                    dst[k] = src[from(i, j)];
                    k += 1;
                }
            }
        }
    }
    // the three colour planes are replaced whether or not there were any; alpha only if there was one
    for (c, v) in fresh.into_iter().enumerate() {
        p.planes[c] = Some(v);
    }
    std::mem::swap(&mut p.width, &mut p.height);
    Ok(())
}

/// A quarter turn counter-clockwise, in place [soft 0x085cbd10 SI_pictureRotate90]
/// (`rotatePicture90` [soft 0x085cbcf8 rotatePicture90] only calls it).
///
/// The new picture is `height` wide and `width` high. Its row `i` is the old column
/// `width − 1 − i` read from the top: the right-hand column becomes the first row. Width and
/// height are exchanged; **the ratio is left as it was** (the file version, [`rotate_file`],
/// inverts it). New planes replace the old; alpha is turned when present.
pub fn rotate_90(p: &mut Picture) -> Result<(), Fault> {
    let (w, _h) = (p.width.max(0) as usize, p.height.max(0) as usize);
    quarter_turn(p, |i, j| j * w + (w - 1 - i))
}

/// A quarter turn clockwise, in place [soft 0x085cbfc0 SI_pictureRotate270]
/// (`rotatePicture270` [soft 0x085cbfa8 rotatePicture270] only calls it).
///
/// The new row `i` is the old column `i` read from the bottom: the left-hand column becomes
/// the first row. Otherwise as [`rotate_90`].
pub fn rotate_270(p: &mut Picture) -> Result<(), Fault> {
    let (w, h) = (p.width.max(0) as usize, p.height.max(0) as usize);
    quarter_turn(p, |i, j| (h - 1 - j) * w + i)
}

/// A half turn, in place [soft 0x085cbee8 SI_pictureRotate180]
/// (`rotatePicture180` [soft 0x085cbed0 rotatePicture180] only calls it).
///
/// Each plane is reversed end to end: `(width × height) >> 1` exchanges.
pub fn rotate_180(p: &mut Picture) -> Result<(), Fault> {
    let n = area(p);
    if n >> 1 <= 0 {
        return Ok(());
    }
    check_in_place(p, n as usize)?;
    for pl in planes_mut(p, n as usize) {
        pl.reverse();
    }
    Ok(())
}

/// What the three "new picture from an old one" operations do to a destination whose red
/// plane is absent: it gets the source's width, height, ratio and `fields` and new planes
/// for red, green and blue, and for alpha only if the source has alpha. (The spare word and
/// an alpha plane the record already had are left.)
fn prepare(dst: &mut Picture, src: &Picture, n: usize) {
    dst.width = src.width;
    dst.height = src.height;
    dst.ratio = fmov(src.ratio);
    dst.fields = src.fields;
    // from the program's allocator: no plane at all for an empty picture, and otherwise a
    // buffer whose first and last bytes carry its marks until something is written there
    for c in 0..3 {
        dst.planes[c] = crate::softpic::si_malloc(n as i32);
    }
    if src.planes[3].is_some() {
        dst.planes[3] = crate::softpic::si_malloc(n as i32);
    }
}

/// The source's pixel count for the two-record operations: negative is a [`Fault::BadSize`]
/// where the original would allocate or copy with it.
fn count(src: &Picture) -> Result<usize, Fault> {
    let n = area(src);
    if n < 0 {
        return Err(Fault::BadSize);
    }
    Ok(n as usize)
}

/// The negative of `src` written into the planes `dst` already has
/// [soft 0x085cc188 SI_pictureNegate].
///
/// Every byte of red, green and blue is complemented (`255 − v`), and of alpha if **the
/// source** has alpha. Nothing else of `dst` is set: not its size, not its planes. The count
/// is the source's width × height; a count of zero or less does nothing.
/// No function of the binary calls this one.
pub fn negate(dst: &mut Picture, src: &Picture) -> Result<(), Fault> {
    let n = area(src);
    if n <= 0 {
        return Ok(());
    }
    let n = n as usize;
    let last = if src.planes[3].is_some() { 4 } else { 3 };
    for c in 0..last {
        plane(src, c, n)?;
        plane(dst, c, n)?;
    }
    for c in 0..last {
        let s = src.planes[c].as_ref().unwrap();
        let d = dst.planes[c].as_mut().unwrap();
        for i in 0..n {
            d[i] = !s[i];
        }
    }
    Ok(())
}

/// One channel of `src` spread over the other planes of `dst`
/// [soft 0x085cc1ec SI_channelExtract]. No function of the binary calls it.
///
/// - `dst` without a red plane is first made a picture of the source's size (see the other
///   two-record operations). Otherwise, **if the source has no alpha, `dst` gets a new alpha
///   plane** (an old one is dropped).
/// - `channel` 0 (red): the source's red goes to green, blue and alpha of `dst`.
///   1 (green): to red, blue and alpha. 2 (blue): to red, green and alpha.
///   **The plane of the chosen channel itself is not written**: the routine is made for
///   `dst` and `src` being the same record, where it is already right. In a fresh `dst` it
///   stays as allocated (zero here, arbitrary in the original).
/// - 3 (alpha): the source's alpha goes to red, green and blue; alpha of `dst` is not
///   written. But if a new alpha plane was just made because the source has none, all four
///   planes of `dst` are set to zero instead.
/// - Any other value: only the preparation above happens.
///
/// Faults where the original would copy to or from a null pointer: a fresh `dst` and a
/// source without alpha (channels 0 to 3), absent source planes.
pub fn channel_extract(dst: &mut Picture, src: &Picture, channel: i32) -> Result<(), Fault> {
    let n = count(src)?;
    // what the planes of dst will be when the copies start
    let fresh = dst.planes[0].is_none();
    let mut have = [0, 1, 2, 3].map(|c| dst.planes[c].as_ref().map(|v| v.len()));
    let mut both = true;
    if fresh {
        have[0] = Some(n);
        have[1] = Some(n);
        have[2] = Some(n);
        if src.planes[3].is_some() {
            have[3] = Some(n);
        }
    } else if src.planes[3].is_none() {
        have[3] = Some(n);
        both = false;
    }
    // (source plane, destination planes) of the three copies
    let copies: Option<(usize, [usize; 3])> = match channel {
        0 => Some((0, [1, 2, 3])),
        1 => Some((1, [0, 2, 3])),
        2 => Some((2, [0, 1, 3])),
        3 if both => Some((3, [0, 1, 2])),
        _ => None,
    };
    let zero_all = channel == 3 && !both;
    if n > 0 {
        let written: &[usize] = match &copies {
            Some((_, to)) => to,
            None if zero_all => &[0, 1, 2, 3],
            None => &[],
        };
        for &c in written {
            match have[c] {
                None => return Err(Fault::NullPlane),
                Some(len) if len < n => return Err(Fault::ShortPlane),
                _ => {}
            }
        }
        if let Some((from, _)) = copies {
            plane(src, from, n)?;
        }
    }
    if fresh {
        prepare(dst, src, n);
    } else if !both {
        dst.planes[3] = crate::softpic::si_malloc(n as i32);
    }
    if let Some((from, to)) = copies {
        if n > 0 {
            let s = &src.planes[from].as_ref().unwrap()[..n];
            for c in to {
                dst.planes[c].as_mut().unwrap()[..n].copy_from_slice(s);
            }
        }
    } else if zero_all {
        for c in 0..4 {
            dst.planes[c].as_mut().unwrap()[..n].fill(0);
        }
    }
    Ok(())
}

/// Contrast about a pivot, per channel [soft 0x085cc3cc SI_intensityModifyPicture].
/// No function of the binary calls it.
///
/// For each byte `v` of a channel with factor `f` (a float) and pivot `p` (an integer):
/// `t = trunc(f × (v − p) + 0.5) + p`, then 255 if `t > 254`, 0 if `t < 1`, else `t`.
/// The product and the sum are formed in the x87 register (the product of a 32-bit integer
/// and a float is exact there) and converted with truncation toward zero, so the rounding is
/// to nearest above the pivot and **not** below it: −14.5 becomes −14, not −15.
/// Alpha is processed if the source has alpha. A `dst` without a red plane is made a picture
/// of the source's size first.
///
/// `factor` and `pivot` are in plane order: red, green, blue, alpha. Class: model.
pub fn intensity_modify(dst: &mut Picture, src: &Picture, factor: [f32; 4], pivot: [i32; 4]) -> Result<(), Fault> {
    let n = count(src)?;
    let last = if src.planes[3].is_some() { 4 } else { 3 };
    check_two(dst, src, n, last, last)?;
    if dst.planes[0].is_none() {
        prepare(dst, src, n);
    }
    let half = F80::from_f64(0.5);
    for c in 0..last {
        let s = &src.planes[c].as_ref().unwrap()[..n];
        let d = &mut dst.planes[c].as_mut().unwrap()[..n];
        let f = F80::from_f32(factor[c]);
        for i in 0..n {
            let delta = (s[i] as i32).wrapping_sub(pivot[c]);
            let t = (F80::from_i32(delta) * f + half).to_i32(Round::Zero).wrapping_add(pivot[c]);
            d[i] = if t > 0xfe {
                0xff
            } else if t <= 0 {
                0
            } else {
                t as u8
            };
        }
    }
    Ok(())
}

/// A constant added to every byte of each channel, clamped to 0…255
/// [soft 0x085cc5ec pictureSaturate]. No function of the binary calls it.
///
/// `offset` is in plane order. **Alpha is processed without testing for it**, in the source
/// and in the destination: a picture without alpha is a fault here (a crash there) unless it
/// is empty. A `dst` without a red plane is made a picture of the source's size first (and
/// gets alpha only if the source has it).
pub fn saturate(dst: &mut Picture, src: &Picture, offset: [i32; 4]) -> Result<(), Fault> {
    let n = count(src)?;
    let alpha = if src.planes[3].is_some() { 4 } else { 3 };
    check_two(dst, src, n, 4, alpha)?;
    if dst.planes[0].is_none() {
        prepare(dst, src, n);
    }
    if n == 0 {
        return Ok(());
    }
    for c in 0..4 {
        let s = &src.planes[c].as_ref().unwrap()[..n];
        let d = &mut dst.planes[c].as_mut().unwrap()[..n];
        for i in 0..n {
            let t = (s[i] as i32).wrapping_add(offset[c]);
            d[i] = t.clamp(0, 0xff) as u8;
        }
    }
    Ok(())
}

/// Checks for a two-record operation over `n` bytes that touches planes `0..used` of both
/// records; a fresh `dst` (no red plane) will get planes `0..made` from [`prepare`].
fn check_two(dst: &Picture, src: &Picture, n: usize, used: usize, made: usize) -> Result<(), Fault> {
    if n == 0 {
        return Ok(());
    }
    let fresh = dst.planes[0].is_none();
    for c in 0..used {
        plane(src, c, n)?;
        if !(fresh && c < made.max(3)) {
            plane(dst, c, n)?;
        }
    }
    Ok(())
}

/// Rows a file-to-file operation has written, with the record its file header is made from.
///
/// The original opens the output, writes the header from a record on its stack and then
/// writes rows one at a time, so a run that stops early leaves a header that promises more
/// rows than follow.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    /// Width, height, ratio and `fields` as in the header. The planes hold the rows written:
    /// `rows × width` bytes each, for the planes the output file has.
    pub picture: Picture,
    /// Number of rows written; equal to `picture.height` when the operation ran to its end.
    pub rows: usize,
}

impl Written {
    /// The bytes of the output file: header, picture chunk and channel packets as the
    /// library's header writer makes them (`Picture::write` in `si-picture`), then the rows
    /// in the mixed run coding.
    ///
    /// With `to_stdout` (an output name starting with `-`) the header declares one **raw**
    /// packet per plane, yet the rows of these operations are still written run-coded: such
    /// output cannot be read back. That is what the original does, and what this returns.
    pub fn file(&self, comment: &[u8], to_stdout: bool) -> Vec<u8> {
        // the header alone: written for a picture of no rows, with the height patched in
        let mut head = self.picture.clone();
        head.height = 0;
        for v in head.planes.iter_mut().flatten() {
            v.clear();
        }
        let mut out = head.write(Some(comment), to_stdout);
        out[94..96].copy_from_slice(&self.picture.height.to_be_bytes());
        // the rows, at most 32,767 at a time (the record's height is 16 bits)
        let w = self.picture.width.max(0) as usize;
        let mut y = 0;
        while y < self.rows {
            let k = (self.rows - y).min(i16::MAX as usize);
            let mut part = Picture { height: k as i16, ..head.clone() };
            for c in 0..4 {
                part.planes[c] = self.picture.planes[c].as_ref().map(|v| v[y * w..(y + k) * w].to_vec());
            }
            out.extend_from_slice(&encode_rows(&part));
            y += k;
        }
        out
    }

    /// The picture a reader gets back from the file, when all rows were written.
    pub fn into_picture(self) -> Option<Picture> {
        (self.rows == self.picture.height.max(0) as usize).then_some(self.picture)
    }
}

/// The header comment of a file written by [`rotate_file`].
pub const ROTATE_COMMENT: &[u8] = b"Rotated picture";

/// Bytes the file rotation keeps out of its working memory: it plans with the machine's
/// physical memory less this much.
pub const ROTATE_RESERVE: f64 = 3_000_000.0;

/// The cases in which [`rotate_file`] gives up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotateError {
    /// The angle is not 90, −90, 180 or 270. By the time the original notices, it has
    /// created the output file and written the header of a quarter turn
    /// ([`rotated_header`]); the file is closed with nothing after it.
    InvalidAngle(i32),
    Fault(Fault),
}

impl RotateError {
    /// The text the original shows (status line, level 3). The file name is not part of it.
    pub fn message(&self, _name: &str) -> String {
        match self {
            RotateError::InvalidAngle(a) => format!("{a} is an invalid rotation angle.\n"),
            RotateError::Fault(_) => String::new(),
        }
    }
}

impl From<Fault> for RotateError {
    fn from(f: Fault) -> Self {
        RotateError::Fault(f)
    }
}

/// The record `SI_pictureRotate` writes its header from: for 180 the input's width, height
/// and ratio; for **every other angle, valid or not**, width and height exchanged and the
/// ratio replaced by `1 / ratio` (one division, rounded to float). `fields` is copied; the
/// spare word is whatever was on the stack (0 here). No planes.
pub fn rotated_header(src: &Picture, angle: i32) -> Picture {
    let mut out = Picture { fields: src.fields, ..Picture::default() };
    if angle == 180 {
        out.width = src.width;
        out.height = src.height;
        out.ratio = fmov(src.ratio);
    } else {
        out.width = src.height;
        out.height = src.width;
        out.ratio = (F80::ONE / F80::from_f32(src.ratio)).to_f32();
    }
    out
}

/// How a quarter turn of a `width × height` picture is cut into vertical strips for a
/// machine with `physmem` bytes of memory (part of `SI_pictureRotate`): number of strips,
/// columns per strip, columns of the last strip.
///
/// - memory = trunc(`physmem` − 3,000,000), as a signed 32-bit integer (a value above 2³¹
///   becomes −2³¹, the unit's "integer indefinite");
/// - columns affordable = trunc((memory / height) × 0.25), the division an integer one;
/// - strips = ceil(width / columns), columns per strip = ceil(width / strips), both through
///   a float; last = width − (strips − 1) × columns per strip.
///
/// With no memory to spare the strip count comes out zero or negative; [`rotate_file`]
/// says what happens then. Class: model (each value is an integer, exactly represented).
pub fn rotate_strips(physmem: u32, width: i16, height: i16) -> Result<(i32, i16, i32), Fault> {
    let mem = (F80::from_i64(physmem as i64) - F80::from_f64(ROTATE_RESERVE)).to_i32(Round::Zero);
    if height == 0 || (mem == i32::MIN && height == -1) {
        return Err(Fault::DivideError);
    }
    let per_row = mem / height as i32;
    let columns = (F80::from_i32(per_row) * F80::from_f64(0.25)).to_i32(Round::Zero);
    let ceil_div = |a: i32, b: i32| -> i32 {
        let q = (F80::from_i32(a) / F80::from_i32(b)).round_int(Round::Up).to_f32();
        F80::from_f32(q).to_i32(Round::Zero)
    };
    let strips = ceil_div(width as i32, columns);
    let strip_width = ceil_div(width as i32, strips) as i16;
    let last = (width as i32).wrapping_sub(strips.wrapping_sub(1).wrapping_mul(strip_width as i32));
    Ok((strips, strip_width, last))
}

/// Rotates a picture file into another picture file [soft 0x085caf90 SI_pictureRotate].
///
/// `src` is the input file's picture as `Picture::read` returns it (the planes its channel
/// packets name), `angle` is in degrees and `physmem` is what `SI_getphysmem` reports (the
/// system's total memory). The result holds the header record and the rows written; its
/// [`Written::file`] with [`ROTATE_COMMENT`] is the output file.
///
/// - 90: the same turn as [`rotate_90`] (counter-clockwise). −90 and 270: as [`rotate_270`].
///   Both write a header with width and height exchanged and the **ratio inverted**.
///   The work is done strip by strip ([`rotate_strips`]): for each strip the whole input is
///   read again and the strip's columns are kept, then written as rows. The result does not
///   depend on the strip count; with a count of zero or less (no memory to plan with) the
///   picture is turned as one strip (see the comment in the code: observed, not read).
/// - 180: row `y` of the output is input row `height − 1 − y` reversed; the ratio stays.
/// - Anything else: [`RotateError::InvalidAngle`], after the output file has been created.
///
/// Not reproduced here, because the row-by-row reader belongs to `si-picture`: the original
/// reads the input a scanline at a time and stops when a scanline cannot be read ("Unexpected
/// End Of File in the input file!", to the status line for −90 and 270, to standard error and
/// without closing either file for 90 and 180), leaving a short output file. Here the input
/// is a decoded picture and is complete by construction.
pub fn rotate_file(src: &Picture, angle: i32, physmem: u32) -> Result<Written, RotateError> {
    let mut out = rotated_header(src, angle);
    if !matches!(angle, 90 | -90 | 180 | 270) {
        return Err(RotateError::InvalidAngle(angle));
    }
    let (w, h) = (src.width as i32, src.height as i32);
    let alpha = src.planes[3].is_some();
    let used = if alpha { 4 } else { 3 };
    // the output file gets the planes the input file's packets name
    for c in 0..4 {
        out.planes[c] = src.planes[c].as_ref().map(|_| Vec::new());
    }
    let mut rows = 0usize;
    if angle == 180 {
        // one strip per row, each a single reversed row
        if h > 0 && w > 0 {
            let n = (w * h) as usize;
            for c in 0..used {
                plane(src, c, n)?;
            }
            let w = w as usize;
            for s in 0..h as usize {
                let y = h as usize - s - 1;
                for c in 0..used {
                    let row = &src.planes[c].as_ref().unwrap()[y * w..(y + 1) * w];
                    out.planes[c].as_mut().unwrap().extend(row.iter().rev());
                }
                rows += 1;
            }
        } else if h > 0 {
            rows = h as usize; // rows of no pixels
        }
        return Ok(Written { picture: out, rows });
    }
    let (mut strips, mut strip_width, mut last) = rotate_strips(physmem, src.width, src.height)?;
    if strips <= 0 {
        // No memory to plan with (less than the reserve, or more than fits a signed 32-bit
        // integer): the strip count comes out zero or negative. Run in the emulator, the
        // original then still writes the whole turned picture (540 made-up cases over five
        // memory sizes, all equal to the single-strip result). Its buffer sizes for this
        // case were not read from the listing, so whether a real run stays inside them is
        // not established; the result here is the one observed.
        (strips, strip_width, last) = (1, src.width, w);
    }
    let sw = strip_width as i32;
    {
        if w < 0 || h < 0 {
            return Err(Fault::BadSize.into());
        }
        let n = (w * h) as usize;
        if n > 0 {
            for c in 0..used {
                plane(src, c, n)?;
            }
        }
    }
    let (wu, hu) = (w.max(0) as usize, h.max(0) as usize);
    for s in 0..strips {
        let count = if s < strips - 1 { sw } else { last };
        if count < 0 {
            return Err(Fault::BadSize.into()); // a negative length handed to memcpy
        }
        let offset = if angle == 90 { (w - sw * (s + 1)).max(0) } else { sw * s };
        if offset < 0 || offset + count > w || count > sw {
            return Err(Fault::ShortPlane.into()); // past the end of the input row or of the strip
        }
        let (offset, count) = (offset as usize, count as usize);
        // the strip's columns become rows: from the right for 90, from the left otherwise
        for k in 0..count {
            let column = if angle == 90 { offset + count - 1 - k } else { offset + k };
            for c in 0..used {
                let from = src.planes[c].as_ref().unwrap();
                let to = out.planes[c].as_mut().unwrap();
                if angle == 90 {
                    to.extend((0..hu).map(|j| from[j * wu + column]));
                } else {
                    to.extend((0..hu).rev().map(|j| from[j * wu + column]));
                }
            }
            rows += 1;
        }
    }
    Ok(Written { picture: out, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture whose planes are the given rows for red, +50 for green, +100 for blue and,
    /// with `alpha`, +150.
    pub(crate) fn pic(w: i16, h: i16, red: &[u8], alpha: bool) -> Picture {
        assert_eq!(red.len(), w as usize * h as usize);
        let shifted = |d: u8| Some(red.iter().map(|v| v.wrapping_add(d)).collect::<Vec<u8>>());
        Picture { width: w, height: h, ratio: 1.5, fields: 3, pad: 7, planes: [shifted(0), shifted(50), shifted(100), if alpha { shifted(150) } else { None }] }
    }

    fn red(p: &Picture) -> &[u8] {
        p.planes[0].as_ref().unwrap()
    }

    #[test]
    fn flips_by_hand() {
        // 1 2 3
        // 4 5 6
        let mut p = pic(3, 2, &[1, 2, 3, 4, 5, 6], true);
        flip_x(&mut p).unwrap();
        assert_eq!(red(&p), [3, 2, 1, 6, 5, 4]);
        assert_eq!(p.planes[3].as_ref().unwrap(), &[153, 152, 151, 156, 155, 154]);
        flip_y(&mut p).unwrap();
        assert_eq!(red(&p), [6, 5, 4, 3, 2, 1]);
        assert_eq!((p.width, p.height, p.ratio, p.fields, p.pad), (3, 2, 1.5, 3, 7));
        // odd height: the middle row stays; even width: every column moves
        let mut p = pic(2, 3, &[1, 2, 3, 4, 5, 6], false);
        flip_y(&mut p).unwrap();
        assert_eq!(red(&p), [5, 6, 3, 4, 1, 2]);
        flip_x(&mut p).unwrap();
        assert_eq!(red(&p), [6, 5, 4, 3, 2, 1]);
        assert_eq!(p.planes[3], None);
    }

    #[test]
    fn flips_of_degenerate_sizes_do_nothing() {
        // one column has nothing to mirror, one row nothing to turn over
        let mut p = pic(1, 4, &[1, 2, 3, 4], false);
        flip_x(&mut p).unwrap();
        assert_eq!(red(&p), [1, 2, 3, 4]);
        let mut p = pic(4, 1, &[1, 2, 3, 4], false);
        flip_y(&mut p).unwrap();
        assert_eq!(red(&p), [1, 2, 3, 4]);
        // empty and negative sizes are never read, so they need no planes
        for (w, h) in [(0i16, 0i16), (0, 5), (5, 0), (-3, 2), (2, -3)] {
            let mut p = Picture { width: w, height: h, ..Picture::default() };
            let before = p.clone();
            flip_x(&mut p).unwrap();
            flip_y(&mut p).unwrap();
            rotate_180(&mut p).unwrap();
            assert_eq!(p, before);
        }
    }

    #[test]
    fn quarter_turns_by_hand() {
        let mut p = pic(3, 2, &[1, 2, 3, 4, 5, 6], true);
        rotate_90(&mut p).unwrap();
        // the right-hand column is the first row
        assert_eq!((p.width, p.height), (2, 3));
        assert_eq!(red(&p), [3, 6, 2, 5, 1, 4]);
        assert_eq!(p.planes[3].as_ref().unwrap(), &[153, 156, 152, 155, 151, 154]);
        assert_eq!(p.ratio, 1.5); // not inverted
        let mut p = pic(3, 2, &[1, 2, 3, 4, 5, 6], false);
        rotate_270(&mut p).unwrap();
        assert_eq!((p.width, p.height), (2, 3));
        assert_eq!(red(&p), [4, 1, 5, 2, 6, 3]);
        assert_eq!(p.planes[3], None);
        let mut p = pic(3, 2, &[1, 2, 3, 4, 5, 6], true);
        rotate_180(&mut p).unwrap();
        assert_eq!(red(&p), [6, 5, 4, 3, 2, 1]);
        assert_eq!((p.width, p.height), (3, 2));
    }

    #[test]
    fn turn_identities() {
        let data: Vec<u8> = (0..35).map(|i| (i * 7 % 251) as u8).collect();
        let start = pic(5, 7, &data, true);
        let mut p = start.clone();
        for _ in 0..4 {
            rotate_90(&mut p).unwrap();
        }
        assert_eq!(p, start);
        rotate_90(&mut p).unwrap();
        rotate_270(&mut p).unwrap();
        assert_eq!(p, start);
        // two quarter turns are the half turn, which is both flips
        let mut a = start.clone();
        rotate_90(&mut a).unwrap();
        rotate_90(&mut a).unwrap();
        let mut b = start.clone();
        rotate_180(&mut b).unwrap();
        let mut c = start.clone();
        flip_x(&mut c).unwrap();
        flip_y(&mut c).unwrap();
        assert_eq!(a, b);
        assert_eq!(b, c);
        // three counter-clockwise are one clockwise
        let mut a = start.clone();
        for _ in 0..3 {
            rotate_90(&mut a).unwrap();
        }
        let mut b = start.clone();
        rotate_270(&mut b).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn quarter_turn_of_an_empty_picture_makes_planes() {
        let mut p = Picture { width: 0, height: 5, ..Picture::default() };
        rotate_90(&mut p).unwrap();
        assert_eq!((p.width, p.height), (5, 0));
        assert_eq!(p.planes, [Some(vec![]), Some(vec![]), Some(vec![]), None]);
        // a real size without planes is where the original reads through a null pointer
        let mut p = Picture { width: 2, height: 2, ..Picture::default() };
        assert_eq!(rotate_90(&mut p), Err(Fault::NullPlane));
        assert_eq!(rotate_180(&mut p), Err(Fault::NullPlane));
        assert_eq!(flip_x(&mut p), Err(Fault::NullPlane));
        p.planes = [Some(vec![0; 4]), Some(vec![0; 4]), Some(vec![0; 3]), None];
        assert_eq!(flip_y(&mut p), Err(Fault::ShortPlane));
    }

    #[test]
    fn negate_complements_into_existing_planes() {
        let src = pic(2, 2, &[0, 1, 254, 255], true);
        let mut dst = pic(2, 2, &[9, 9, 9, 9], true);
        dst.ratio = 2.0;
        negate(&mut dst, &src).unwrap();
        assert_eq!(red(&dst), [255, 254, 1, 0]);
        assert_eq!(dst.planes[1].as_ref().unwrap(), &[205, 204, 207, 206]); // green is 50 51 48 49
        assert_eq!(dst.planes[3].as_ref().unwrap(), &[105, 104, 107, 106]); // alpha is 150 151 148 149
        assert_eq!(dst.ratio, 2.0); // nothing but the planes is touched
        // alpha follows the source: none there, none written
        let src = pic(2, 2, &[0, 1, 254, 255], false);
        let mut dst = pic(2, 2, &[9, 9, 9, 9], true);
        negate(&mut dst, &src).unwrap();
        assert_eq!(dst.planes[3].as_ref().unwrap(), &[159; 4]);
        // a destination without planes is a null pointer
        assert_eq!(negate(&mut Picture::default(), &src), Err(Fault::NullPlane));
        // twice is the identity
        let mut a = src.clone();
        negate(&mut a, &src).unwrap();
        let b = a.clone();
        negate(&mut a, &b).unwrap();
        assert_eq!(a, src);
    }

    #[test]
    fn channel_extract_variants() {
        let src = pic(2, 1, &[10, 20], true); // g = 60 70, b = 110 120, a = 160 170
        // same record for both: the chosen channel ends up everywhere
        for (ch, want) in [(0, [10u8, 20]), (1, [60, 70]), (2, [110, 120]), (3, [160, 170])] {
            let mut dst = src.clone();
            channel_extract(&mut dst, &src, ch).unwrap();
            for c in 0..4 {
                let expect: &[u8] = if c == 3 && ch == 3 { &[160, 170] } else { &want };
                assert_eq!(dst.planes[c].as_deref(), Some(expect), "channel {ch} plane {c}");
            }
        }
        // a fresh destination: the chosen channel's own plane is left as allocated
        let mut dst = Picture::default();
        channel_extract(&mut dst, &src, 1).unwrap();
        assert_eq!((dst.width, dst.height, dst.ratio, dst.fields, dst.pad), (2, 1, 1.5, 3, 0));
        // (it carries the allocator's two marks, 0x6f and 0x6b)
        assert_eq!(dst.planes, [Some(vec![60, 70]), Some(vec![0x6f, 0x6b]), Some(vec![60, 70]), Some(vec![60, 70])]);
        // no alpha in the source, same record: a new alpha plane, which gets the channel too
        let rgb = pic(2, 1, &[10, 20], false);
        let mut dst = rgb.clone();
        channel_extract(&mut dst, &rgb, 2).unwrap();
        assert_eq!(dst.planes, [Some(vec![110, 120]), Some(vec![110, 120]), Some(vec![110, 120]), Some(vec![110, 120])]);
        // … and asking for the alpha it does not have blanks all four planes
        let mut dst = rgb.clone();
        channel_extract(&mut dst, &rgb, 3).unwrap();
        assert_eq!(dst.planes, [Some(vec![0, 0]), Some(vec![0, 0]), Some(vec![0, 0]), Some(vec![0, 0])]);
        // fresh destination and no alpha: the copy into the alpha plane has nowhere to go
        assert_eq!(channel_extract(&mut Picture::default(), &rgb, 0), Err(Fault::NullPlane));
        assert_eq!(channel_extract(&mut Picture::default(), &rgb, 3), Err(Fault::NullPlane));
        // an unknown channel only prepares the destination
        let mut dst = Picture::default();
        channel_extract(&mut dst, &rgb, 7).unwrap();
        let m = Some(vec![0x6f, 0x6b]); // as the allocator leaves them
        assert_eq!(dst.planes, [m.clone(), m.clone(), m, None]);
        let mut dst = rgb.clone();
        channel_extract(&mut dst, &rgb, -1).unwrap();
        assert_eq!(dst.planes[3], Some(vec![0x6f, 0x6b]));
        assert_eq!(dst.planes[..3], rgb.planes[..3]);
    }

    #[test]
    fn intensity_rounds_toward_the_pivot_side() {
        let src = pic(4, 1, &[110, 90, 100, 255], false);
        let mut dst = Picture::default();
        intensity_modify(&mut dst, &src, [1.5, 1.0, 1.0, 1.0], [100, 0, 0, 0]).unwrap();
        // +10 → 15.5 → 15; −10 → −14.5 → −14 (truncation, not floor); 155 → 233
        assert_eq!(red(&dst), [115, 86, 100, 255]);
        assert_eq!(dst.planes[1].as_ref().unwrap(), &[160, 140, 150, 49]); // factor 1: unchanged (305 wraps to 49 in the test data)
        assert_eq!(dst.planes[3], None);
        assert_eq!((dst.width, dst.height, dst.fields), (4, 1, 3));
        // the clamp: above 254 gives 255, below 1 gives 0
        let src = pic(4, 1, &[127, 128, 1, 0], true);
        let mut dst = src.clone();
        intensity_modify(&mut dst, &src, [2.0, 0.0, -1.0, 0.5], [0, 7, 0, 150]).unwrap();
        assert_eq!(red(&dst), [254, 255, 2, 0]);
        assert_eq!(dst.planes[1].as_ref().unwrap(), &[7; 4]); // factor 0: the pivot
        assert_eq!(dst.planes[2].as_ref().unwrap(), &[0; 4]); // negative values clamp to 0
        // alpha 21 22 151 150 about 150 at one half: −64.5+.5 → −64, −64+.5 → −63, 0.5+.5 → 1, 0.5 → 0
        assert_eq!(dst.planes[3].as_ref().unwrap(), &[86, 87, 151, 150]);
    }

    #[test]
    fn intensity_is_computed_in_the_register() {
        // 0.7 as a float is 0.699999988…; × 5 = 3.49999994… exactly in the register, and
        // adding one half stays below 4. In float arithmetic the product rounds to 3.5 and
        // the answer would be 4.
        let src = pic(1, 1, &[5], false);
        let mut dst = Picture::default();
        intensity_modify(&mut dst, &src, [0.7, 0.7, 0.7, 0.7], [0; 4]).unwrap();
        assert_eq!(red(&dst), [3]);
        assert_eq!((0.7f32 * 5.0 + 0.5) as i32, 4);
    }

    #[test]
    fn saturate_adds_and_clamps_and_needs_alpha() {
        let src = pic(3, 1, &[0, 100, 250], true); // g 50 150 44, b 100 200 94, a 150 250 144
        let mut dst = Picture::default();
        saturate(&mut dst, &src, [10, -60, 100, 0]).unwrap();
        assert_eq!(dst.planes, [Some(vec![10, 110, 255]), Some(vec![0, 90, 0]), Some(vec![200, 255, 194]), Some(vec![150, 250, 144])]);
        let rgb = pic(3, 1, &[0, 100, 250], false);
        assert_eq!(saturate(&mut Picture::default(), &rgb, [0; 4]), Err(Fault::NullPlane));
        assert_eq!(saturate(&mut rgb.clone(), &rgb, [0; 4]), Err(Fault::NullPlane));
        // an empty picture is never read
        let empty = Picture { width: 0, height: 3, ..Picture::default() };
        let mut dst = Picture::default();
        saturate(&mut dst, &empty, [1; 4]).unwrap();
        assert_eq!(dst.planes, [None, None, None, None]); // the allocator gives nothing for no pixels
    }

    #[test]
    fn strips_follow_the_memory() {
        // plenty of memory: one strip
        assert_eq!(rotate_strips(64 << 20, 640, 480), Ok((1, 640, 640)));
        // 3,000,000 + 4 × 480 × 100 bytes: 100 columns at a time
        assert_eq!(rotate_strips(3_192_000, 640, 480), Ok((7, 92, 88)));
        assert_eq!(rotate_strips(3_192_000, 100, 480), Ok((1, 100, 100)));
        assert_eq!(rotate_strips(3_192_000, 101, 480), Ok((2, 51, 50)));
        // nothing to spare: the division by zero columns gives the integer indefinite
        assert_eq!(rotate_strips(3_000_000, 640, 480), Ok((i32::MIN, 0, 640)));
        // less than the reserve: a negative column count, and ceil(640 / −1041) is (minus) zero
        assert_eq!(rotate_strips(1_000_000, 640, 480), Ok((0, 0, 640)));
        // more than 2 GiB + 3,000,000 does not fit the conversion
        assert_eq!(rotate_strips(0xf000_0000, 640, 480).unwrap().0, 0);
        assert_eq!(rotate_strips(64 << 20, 640, 0), Err(Fault::DivideError));
    }

    #[test]
    fn file_rotation_matches_the_in_memory_turns() {
        let data: Vec<u8> = (0..77).map(|i| (i * 13 % 251) as u8).collect();
        for alpha in [false, true] {
            let src = pic(11, 7, &data, alpha);
            for physmem in [64u32 << 20, 3_000_000 + 4 * 7 * 3, 3_000_000 + 4 * 7, 3_000_000 + 4 * 7 * 5] {
                for (angle, turn) in [(90, rotate_90 as fn(&mut Picture) -> Result<(), Fault>), (-90, rotate_270), (270, rotate_270), (180, rotate_180)] {
                    let got = rotate_file(&src, angle, physmem).unwrap();
                    assert_eq!(got.rows, got.picture.height as usize);
                    let mut want = src.clone();
                    turn(&mut want).unwrap();
                    want.pad = 0;
                    if angle != 180 {
                        want.ratio = 1.0 / 1.5;
                    }
                    assert_eq!(got.picture, want, "angle {angle} physmem {physmem}");
                    // the file reads back as that picture
                    let bytes = got.file(ROTATE_COMMENT, false);
                    let back = Picture::read(&bytes).unwrap();
                    assert_eq!(back.picture, want);
                    assert_eq!(back.comment, ROTATE_COMMENT);
                    assert_eq!(back.end, bytes.len());
                    assert_eq!(bytes, want.write(Some(ROTATE_COMMENT), false));
                }
            }
        }
    }

    #[test]
    fn file_rotation_odd_cases() {
        let src = pic(3, 2, &[1, 2, 3, 4, 5, 6], false);
        // an invalid angle: the message, and the header a quarter turn would get
        let e = rotate_file(&src, 45, 64 << 20).unwrap_err();
        assert_eq!(e, RotateError::InvalidAngle(45));
        assert_eq!(e.message("x"), "45 is an invalid rotation angle.\n");
        let head = rotated_header(&src, 45);
        assert_eq!((head.width, head.height, head.ratio, head.fields), (2, 3, 1.0 / 1.5, 3));
        // no memory to plan with: the picture is turned all the same (as observed in the
        // emulator), in one strip
        let got = rotate_file(&src, 90, 2_000_000).unwrap();
        assert_eq!((got.rows, got.picture.width, got.picture.height), (3, 2, 3));
        assert_eq!(got.file(ROTATE_COMMENT, false), rotate_file(&src, 90, 64 << 20).unwrap().file(ROTATE_COMMENT, false));
        // … but the half turn does not use the strip arithmetic
        assert_eq!(rotate_file(&src, 180, 0).unwrap().rows, 2);
        // to standard output the header promises raw rows and the rows are run-coded
        let got = rotate_file(&src, 180, 64 << 20).unwrap();
        let bytes = got.file(ROTATE_COMMENT, true);
        assert_eq!(&bytes[104..116], &[1, 8, 0, 0x80, 1, 8, 0, 0x40, 0, 8, 0, 0x20]);
        assert_eq!(&bytes[116..], &got.file(ROTATE_COMMENT, false)[108..]);
        // a picture lacking a colour plane is a null pointer in the original
        let mut bad = src.clone();
        bad.planes[1] = None;
        assert_eq!(rotate_file(&bad, 270, 64 << 20), Err(RotateError::Fault(Fault::NullPlane)));
    }
}
