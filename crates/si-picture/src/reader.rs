// SPDX-License-Identifier: MIT
//! A picture file opened for reading row by row, and the readers that build the packed
//! picture record from it.
//!
//! Sources are functions of the Linux `soft` binary, cited as `[soft 0xADDRESS name]`.
//! Source file: `picture.c`.

use crate::{may_allocate, open, open_with, read_mixed, read_raw, Opened, Packet, Picture, PictureError, PictureRgba, PLANE_BITS, TYPE_MIXED, TYPE_RAW};
use si_file::Stream;

/// A picture file after its header and channel packets
/// [soft 0x085d2970 SI_pictureOpenFile] [soft 0x085d2cc4 openPictureFile]: the stream stands
/// at the first scanline.
///
/// `openPictureFile` is the same call returning 1 whatever happened.
pub struct OpenPicture<'a> {
    pub stream: Stream<'a>,
    /// Width, height, ratio, fields and the spare word; no planes.
    pub picture: Picture,
    pub version: f32,
    pub comment: Vec<u8>,
    pub packets: Vec<Packet>,
}

impl<'a> OpenPicture<'a> {
    pub fn open(data: &'a [u8]) -> Result<Self, PictureError> {
        let mut stream = Stream::new(data);
        let Opened { picture, version, comment, packets } = open(&mut stream)?;
        Ok(OpenPicture { stream, picture, version, comment, packets })
    }

    /// As [`OpenPicture::open`], but with the original's size test and no more: only a zero
    /// width or height is refused, a negative one comes through. For callers that read rows
    /// into buffers of their own and deal with such a size themselves.
    pub fn open_signed(data: &'a [u8]) -> Result<Self, PictureError> {
        let mut stream = Stream::new(data);
        let Opened { picture, version, comment, packets } = open_with(&mut stream, true)?;
        Ok(OpenPicture { stream, picture, version, comment, packets })
    }

    /// Reads one row into row buffers [soft 0x085d2cfc SI_scanlineRead], as the original does
    /// when the packets carry no buffers of their own: each packet's planes are looked up
    /// afresh in `rows` (red, green, blue, alpha), so every call writes at the buffers' start.
    ///
    /// Returns what the last packet's reader returned, or false as soon as the end-of-file
    /// flag is set after a packet. Packets of a type other than raw and mixed read nothing.
    ///
    /// A packet naming a plane with no buffer makes the original write through a null
    /// pointer. Here the plane's bytes are read and dropped.
    pub fn scanline_read(&mut self, rows: &mut [Option<Vec<u8>>; 4]) -> bool {
        let width = self.picture.width.max(0) as usize;
        let mut result = false;
        let mut scratch: [Vec<u8>; 4] = Default::default();
        for p in &self.packets {
            let mut planes: Vec<&mut [u8]> = Vec::with_capacity(4);
            for ((row, spare), bit) in rows.iter_mut().zip(scratch.iter_mut()).zip(PLANE_BITS) {
                if p.channel & bit == 0 {
                    continue;
                }
                match row {
                    Some(r) => planes.push(&mut r[..]),
                    None => {
                        spare.resize(width, 0);
                        planes.push(&mut spare[..]);
                    }
                }
            }
            match p.kind & 0xf {
                TYPE_RAW => result = read_raw(&mut self.stream, width, &mut planes, 0),
                TYPE_MIXED => result = read_mixed(&mut self.stream, width, &mut planes, 0, true),
                _ => {}
            }
            if self.stream.feof() {
                return false;
            }
        }
        result
    }

    /// One packet's planes of one row through the mixed decoder alone, written at `at` in
    /// the caller's buffers (red, green, blue, alpha): [soft 0x085d2f8c si_channelReadMixed]
    /// as `SI_pictureReduce` of `size.c` calls it, walking the packets itself and reading
    /// no other coding. The buffers must hold `at + width` bytes for the packet's planes; a
    /// plane with no buffer is read and dropped. Returns the decoder's result.
    pub fn packet_read_mixed(&mut self, packet: usize, bufs: &mut [Option<Vec<u8>>; 4], at: usize) -> bool {
        let width = self.picture.width.max(0) as usize;
        let Some(p) = self.packets.get(packet).copied() else { return false };
        let mut scratch: [Vec<u8>; 4] = Default::default();
        let mut planes: Vec<&mut [u8]> = Vec::with_capacity(4);
        let mut base = Vec::with_capacity(4);
        for ((row, spare), bit) in bufs.iter_mut().zip(scratch.iter_mut()).zip(PLANE_BITS) {
            if p.channel & bit == 0 {
                continue;
            }
            match row {
                Some(r) if r.len() >= at + width => {
                    planes.push(&mut r[..]);
                    base.push(at);
                }
                _ => {
                    spare.resize(width, 0);
                    planes.push(&mut spare[..]);
                    base.push(0);
                }
            }
        }
        // the decoder takes one offset for all planes: dropped planes are padded to it
        if base.iter().any(|&b| b != at) {
            let mut padded: Vec<Vec<u8>> = planes.iter().map(|_| vec![0u8; at + width]).collect();
            let mut refs: Vec<&mut [u8]> = padded.iter_mut().map(|v| &mut v[..]).collect();
            let r = read_mixed(&mut self.stream, width, &mut refs, at, true);
            for ((dst, src), b) in planes.iter_mut().zip(&padded).zip(&base) {
                if *b == at {
                    dst[at..at + width].copy_from_slice(&src[at..at + width]);
                }
            }
            return r;
        }
        read_mixed(&mut self.stream, width, &mut planes, at, true)
    }

    /// Whether the readers below make an alpha buffer: only when the first or the second
    /// packet names alpha. Alpha in a third packet is not seen.
    fn alpha_in_first_two(&self) -> bool {
        self.packets.iter().take(2).any(|p| p.channel & crate::ALPHA != 0)
    }

    /// A lower bound on the bytes one row takes in the file.
    fn min_row(&self) -> usize {
        self.packets.iter().filter(|p| matches!(p.kind & 0xf, TYPE_RAW | TYPE_MIXED)).count().max(1)
    }
}

/// The horizontal rescaling the scaled readers apply when the pixel ratio is not 1:
/// `SI_scalePictureX` of `size.c` (crate `si-imgfmt`, `size::scale_picture_x`), which replaces
/// the picture's planes and width.
pub type ScaleX<'f> = &'f dyn Fn(&mut Picture, f32);

/// What the three readers share up to the point where the output is built.
struct Loaded<'a> {
    file: OpenPicture<'a>,
    /// Row buffers (`width` bytes) without scaling, whole planes with it.
    planes: [Option<Vec<u8>>; 4],
    has_alpha: bool,
}

/// Opens the file and makes the buffers; with `scale`, reads the whole picture bottom row
/// first and rescales it.
///
/// The original asks `SI_swapSpaceAvailable` for `4 × width × (height + 1)` bytes (plus
/// `4 × width × height` with scaling) and shows "image too big, memory insufficient" when
/// refused; here that refusal is [`may_allocate`] and comes back as `TooBig`.
fn load<'a>(data: &'a [u8], scale: Option<ScaleX>) -> Result<Loaded<'a>, PictureError> {
    let mut file = OpenPicture::open(data)?;
    let (w, h) = (file.picture.width as usize, file.picture.height as usize);
    let want = 4 * w * (h + 1) + if scale.is_some() { 4 * w * h } else { 0 };
    if !may_allocate(want, h, file.min_row(), file.stream.len() - file.stream.tell()) {
        return Err(PictureError::TooBig);
    }
    let has_alpha = file.alpha_in_first_two();
    let n = if scale.is_some() { w * h } else { w };
    let mut planes: [Option<Vec<u8>>; 4] = [Some(vec![0; n]), Some(vec![0; n]), Some(vec![0; n]), has_alpha.then(|| vec![0; n])];
    if let Some(scale_x) = scale {
        // each row is read at the start of the planes and then copied to its place, the
        // file's first row last
        for row in (0..h).rev() {
            if !file.scanline_read(&mut planes) {
                break;
            }
            for (i, plane) in planes.iter_mut().enumerate() {
                // alpha is copied when the file has a second packet, whatever it carries: a
                // file with all four planes in one packet keeps only the last row read, at
                // row 0. (With two packets and no alpha buffer the original copies from a
                // null pointer; skipped here.)
                if i == 3 && file.packets.len() < 2 {
                    continue;
                }
                if let Some(p) = plane {
                    p.copy_within(0..w, row * w);
                }
            }
        }
        // anything but exactly 1.0, a NaN included
        if file.picture.ratio != 1.0 {
            let mut pic = Picture { planes, ..file.picture.clone() };
            let ratio = pic.ratio;
            scale_x(&mut pic, ratio);
            planes = std::mem::take(&mut pic.planes);
            file.picture = pic;
        }
    }
    Ok(Loaded { file, planes, has_alpha })
}

/// [soft 0x085d4d10 SI_pictureReadRGBA]: [`read_rgba_scale`] without scaling.
pub fn read_rgba(data: &[u8]) -> Result<PictureRgba, PictureError> {
    read_rgba_scale(data, None)
}

/// Reads a picture file into the packed record [soft 0x085d4d30 SI_pictureReadRGBAScale].
///
/// - Rows are stored bottom-up. A pixel's fourth byte is the file's alpha, or 0xff when the
///   file has none; the record's word at offset 10 says which (1 or 0).
/// - Without `scale`, rows are read one at a time; a row that fails to read ends the picture
///   (it and the rows after it stay zero) and the call still succeeds.
/// - With `scale` (the caller wants square pixels), the whole picture is read first and, if
///   its ratio is not exactly 1, handed to `SI_scalePictureX`; the record then has the new
///   width. The ratio stored is the file's.
pub fn read_rgba_scale(data: &[u8], scale: Option<ScaleX>) -> Result<PictureRgba, PictureError> {
    let Loaded { mut file, mut planes, has_alpha } = load(data, scale)?;
    let (w, h) = (file.picture.width.max(0) as usize, file.picture.height.max(0) as usize);
    let mut out = header(&file.picture, has_alpha);
    out.data = vec![0u8; 4 * w * h];
    let mut alpha = 0xffu8;
    for row in (0..h).rev() {
        if scale.is_none() && !file.scanline_read(&mut planes) {
            break;
        }
        let at = if scale.is_some() { row * w } else { 0 };
        for x in 0..w {
            if let Some(a) = &planes[3] {
                alpha = a[at + x];
            }
            let px = [plane(&planes, 0, at + x), plane(&planes, 1, at + x), plane(&planes, 2, at + x), alpha];
            out.data[4 * (row * w + x)..4 * (row * w + x) + 4].copy_from_slice(&px);
        }
    }
    Ok(out)
}

/// Reads a picture file as grey levels [soft 0x085d5294 SI_pictureReadBWScale]: as
/// [`read_rgba_scale`], but the record's `data` holds **one byte per pixel**, the integer
/// mean (red + green + blue) / 3, bottom row first. Alpha is read and not used.
pub fn read_bw_scale(data: &[u8], scale: Option<ScaleX>) -> Result<PictureRgba, PictureError> {
    let Loaded { mut file, mut planes, has_alpha } = load(data, scale)?;
    let (w, h) = (file.picture.width.max(0) as usize, file.picture.height.max(0) as usize);
    let mut out = header(&file.picture, has_alpha);
    out.data = vec![0u8; w * h];
    for row in (0..h).rev() {
        if scale.is_none() && !file.scanline_read(&mut planes) {
            break;
        }
        let at = if scale.is_some() { row * w } else { 0 };
        for x in 0..w {
            let sum = plane(&planes, 0, at + x) as u32 + plane(&planes, 1, at + x) as u32 + plane(&planes, 2, at + x) as u32;
            out.data[row * w + x] = (sum / 3) as u8;
        }
    }
    Ok(out)
}

/// Reads a picture file at half size [soft 0x085d57c8 SI_pictureReadScaledRGBAScale].
///
/// The record keeps the **full** width and height while `data` holds
/// `(width / 2) × (height / 2)` pixels, every second pixel of a row, fourth byte 0.
/// - With `scale` the whole picture is read first (and rescaled as in [`read_rgba_scale`]),
///   then every second row is taken.
/// - Without it only `height / 2` rows are read from the file and none is skipped: the
///   result is the **top half** of the picture at half width, not the picture at half size.
/// - A picture one pixel wide or high has nothing to allocate and is refused without a
///   message.
pub fn read_scaled_rgba_scale(data: &[u8], scale: Option<ScaleX>) -> Result<PictureRgba, PictureError> {
    let Loaded { mut file, mut planes, has_alpha } = load(data, scale)?;
    let w = file.picture.width.max(0) as usize;
    let (hw, hh) = (w / 2, file.picture.height.max(0) as usize / 2);
    let mut out = header(&file.picture, has_alpha);
    if hw * hh == 0 {
        // the allocator returns nothing for a count of zero and the reader gives up, silently
        return Err(PictureError::NothingAllocated);
    }
    out.data = vec![0u8; 4 * hw * hh];
    for row in (0..hh).rev() {
        if scale.is_none() && !file.scanline_read(&mut planes) {
            break;
        }
        let at = if scale.is_some() { 2 * w * row } else { 0 };
        for x in 0..hw {
            let px = [plane(&planes, 0, at + 2 * x), plane(&planes, 1, at + 2 * x), plane(&planes, 2, at + 2 * x), 0];
            out.data[4 * (row * hw + x)..4 * (row * hw + x) + 4].copy_from_slice(&px);
        }
    }
    Ok(out)
}

fn header(p: &Picture, has_alpha: bool) -> PictureRgba {
    PictureRgba { width: p.width, height: p.height, ratio: p.ratio, fields: p.fields, pad: has_alpha as i16, data: Vec::new() }
}

/// A byte of a plane; 0 where the rescaler left the plane shorter than the record says.
fn plane(planes: &[Option<Vec<u8>>; 4], i: usize, at: usize) -> u8 {
    planes[i].as_ref().and_then(|p| p.get(at).copied()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(alpha: bool) -> Picture {
        let (w, h) = (6usize, 4usize);
        let plane = |k: usize| Some((0..w * h).map(|i| (i * 7 + k * 50) as u8).collect::<Vec<u8>>());
        Picture { width: 6, height: 4, ratio: 1.0, fields: 3, pad: 0, planes: [plane(0), plane(1), plane(2), if alpha { plane(3) } else { None }] }
    }

    #[test]
    fn packed_read_is_bottom_up_with_alpha_flag() {
        for alpha in [false, true] {
            let p = sample(alpha);
            let q = read_rgba(&p.write(None, false)).unwrap();
            assert_eq!((q.width, q.height, q.fields, q.pad), (6, 4, 3, alpha as i16));
            // packed row 3 is the file's first row
            let px = &q.data[4 * 18..4 * 18 + 4];
            assert_eq!(px[..3], [0, 50, 100]);
            assert_eq!(px[3], if alpha { 150 } else { 0xff });
            // and packed row 0 the file's last
            assert_eq!(q.data[0], (18 * 7) as u8);
        }
    }

    #[test]
    fn a_short_file_leaves_the_rest_zero() {
        let p = sample(true);
        let bytes = p.write(None, false);
        let q = read_rgba(&bytes[..bytes.len() - 3]).unwrap();
        assert!(q.data[..24].iter().all(|&b| b == 0)); // the last row was not completed
        assert_eq!(q.data[4 * 6], (12 * 7) as u8);
    }

    #[test]
    fn grey_is_the_integer_mean() {
        let p = sample(false);
        let q = read_bw_scale(&p.write(None, false), None).unwrap();
        assert_eq!(q.data.len(), 24);
        assert_eq!(q.data[18], ((0 + 50 + 100) / 3) as u8);
        assert_eq!(q.data[19], ((7 + 57 + 107) / 3) as u8);
    }

    #[test]
    fn half_size_without_scaling_takes_the_top_half() {
        let p = sample(false);
        let q = read_scaled_rgba_scale(&p.write(None, false), None).unwrap();
        assert_eq!((q.width, q.height, q.data.len()), (6, 4, 4 * 3 * 2));
        // packed row 1 = file row 0, packed row 0 = file row 1 (not row 2)
        assert_eq!(q.data[4 * 3], 0);
        assert_eq!(q.data[4 * 3 + 4], 14);
        assert_eq!(q.data[0], 42);
        assert_eq!(q.data[3], 0);
    }

    #[test]
    fn scaling_reads_all_rows_first_and_calls_the_rescaler_only_off_ratio_one() {
        let mut p = sample(true);
        let called = std::cell::Cell::new(0);
        let f = |pic: &mut Picture, r: f32| {
            called.set(called.get() + 1);
            assert_eq!(r, 2.0);
            // the planes arrive bottom row first
            assert_eq!(pic.planes[0].as_ref().unwrap()[18], 0);
        };
        let q = read_rgba_scale(&p.write(None, false), Some(&f)).unwrap();
        assert_eq!(called.get(), 0);
        assert_eq!(q.data[4 * 18 + 3], 150);
        p.ratio = 2.0;
        let q = read_scaled_rgba_scale(&p.write(None, false), Some(&f)).unwrap();
        assert_eq!(called.get(), 1);
        // every second row of the bottom-up planes: packed row 1 = planes row 2 = file row 1
        assert_eq!(q.data[4 * 3], 42);
        assert_eq!(q.data[0], (18 * 7) as u8);
    }

    #[test]
    fn huge_header_on_a_small_file_is_refused() {
        let mut bytes = sample(false).write(None, false);
        bytes[92..96].copy_from_slice(&[0x7f, 0xff, 0x7f, 0xff]);
        assert_eq!(read_rgba(&bytes).unwrap_err(), PictureError::TooBig);
        assert_eq!(Picture::read(&bytes).unwrap_err(), PictureError::InvalidSize);
    }
}
