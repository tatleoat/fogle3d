// SPDX-License-Identifier: MIT
//! Aurora pictures (`.sim`). Original source file `aurora.c`.
//!
//! Layout as this program reads and writes it:
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 4 | written as zeros; read and thrown away |
//! | 4 | 2 | width, **little-endian** |
//! | 6 | 2 | height, little-endian |
//! | 8 | 4 × width × height | pixels: red, green, blue, alpha; **last row first** |
//!
//! No compression, no magic, no test of anything: every file is an Aurora file. (The size
//! words are the one little-endian field among the foreign formats of this group: the reader
//! reads them through the big-endian helper and then swaps them back, the writer copies the
//! picture record's bytes as they lie in memory.)
//!
//! None of the routines here is called from the `soft` program itself; they are the code its
//! converter programs share.

use crate::alias::{fits, si_malloc, softpic};
use si_file::Stream;
use si_picture::{write_mixed, Picture};

/// The extension every routine here hands to the opener (modes `rb` and `wb`).
pub const EXTENSION: &str = ".sim";

const HEADER_SIZE: usize = 8;

/// The cases in which a routine of this file gives up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AuroraError {
    /// [`import_aurora`]: a row could not be read whole. The picture file exists by then and
    /// holds its header and nothing else (the rows are fetched last row of the file first,
    /// so a file that is too short fails on the first one).
    UnexpectedEnd,
    /// [`export_aurora`]: the SOFTIMAGE picture file was refused.
    Picture(softpic::PicError),
    /// Not a case of the original: `width × height` is more than the allowance beyond what
    /// the file's bytes can hold. The original allocates it all the same.
    TooLarge,
    /// Not a case of the original: a negative width with a positive height in the import or
    /// the export. The original then has null row buffers and hands the C library a
    /// negative length.
    BadSize,
    /// Not a case of the original: a picture without a red, green or blue plane (or with one
    /// shorter than `width × height`) where the original reads through the pointer.
    MissingPlane,
}

impl AuroraError {
    /// The text the original shows. For [`AuroraError::UnexpectedEnd`] `name` is the name
    /// the opener reports (directory removed, extension added if the opener added it).
    pub fn message(&self, name: &str) -> String {
        match self {
            AuroraError::UnexpectedEnd => format!("Unexpected end of file in input file {name}\n"),
            AuroraError::Picture(e) => e.message(name),
            _ => String::new(),
        }
    }
}

/// The header as both readers take it apart (the same four calls inlined in each): two
/// 2-byte reads whose result is overwritten, then width and height. Neither the reads nor
/// their results are tested.
///
/// A file shorter than eight bytes therefore still yields a size: a word that could not be
/// read whole keeps what the first pair of reads put there (bytes 0 to 3 of the file, which
/// then come out big-endian), with a single stray byte copied over its first byte. Words no
/// read reached are stale stack in the original and 0 here.
fn read_size(s: &mut Stream) -> (i16, i16) {
    let mut w = [0u8; 2];
    let mut h = [0u8; 2];
    s.read(&mut w);
    s.read(&mut h);
    // the big-endian helper swaps a word only if it arrived whole …
    if s.read(&mut w) == 2 {
        w.swap(0, 1);
    }
    if s.read(&mut h) == 2 {
        h.swap(0, 1);
    }
    // … and the caller swaps both back unconditionally
    (i16::from_be_bytes(w), i16::from_be_bytes(h))
}

fn write_size(out: &mut Vec<u8>, width: i16, height: i16) {
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
}

/// Reads an Aurora picture into memory [soft 0x085c10a0 SI_pictureReadAurora] (wrapped
/// unchanged by [soft 0x085c126c readAuroraPicture]).
///
/// Ratio 1.0, `fields` 3, all four planes always. The file's rows fill the picture from its
/// last row up to its first, four `getc` calls per pixel with no test for the end of the
/// file: whatever the file lacks is 0xff, and the reader always succeeds.
///
/// A zero size gives no planes. If only one of the sizes is negative likewise (the allocator
/// refuses the negative product). If both are negative the planes are allocated and nothing
/// is read into them: they hold the allocator's marks (first byte 0x6f, last 0x6b) around
/// heap contents, which are 0 here. `pad` is not touched by the original.
pub fn read_aurora(data: &[u8]) -> Result<Picture, AuroraError> {
    let mut s = Stream::new(data);
    let (width, height) = read_size(&mut s);
    let mut pic = Picture { width, height, ratio: 1.0, fields: 3, pad: 0, planes: Default::default() };
    let total = width as i32 * height as i32;
    if total > 0 && !fits(total as usize, data.len().saturating_sub(HEADER_SIZE) / 4 + 1) {
        return Err(AuroraError::TooLarge);
    }
    for p in pic.planes.iter_mut() {
        *p = si_malloc(total);
    }
    if width > 0 && height > 0 {
        let w = width as usize;
        for y in (0..height as usize).rev() {
            for x in 0..w {
                for p in pic.planes.iter_mut() {
                    p.as_mut().unwrap()[y * w + x] = s.getc().unwrap_or(0xff);
                }
            }
        }
    }
    Ok(pic)
}

/// Writes a picture in memory as an Aurora file [soft 0x085c1288 SI_pictureWriteAurora]
/// (wrapped unchanged by [soft 0x085c1458 writeAuroraPicture]).
///
/// Four zero bytes, the first four bytes of the picture record as they lie in memory (width
/// and height, little-endian on this build), then the rows from the last to the first, four
/// bytes per pixel. **A picture without an alpha plane gets 0xff as every fourth byte.**
/// A width or height of zero or less writes the header alone.
pub fn write_aurora(pic: &Picture) -> Result<Vec<u8>, AuroraError> {
    let mut out = Vec::new();
    write_size(&mut out, pic.width, pic.height);
    if pic.width <= 0 || pic.height <= 0 {
        return Ok(out);
    }
    let (w, h) = (pic.width as usize, pic.height as usize);
    let [Some(r), Some(g), Some(b), a] = &pic.planes else { return Err(AuroraError::MissingPlane) };
    if [Some(r), Some(g), Some(b), a.as_ref()].iter().flatten().any(|p| p.len() < w * h) {
        return Err(AuroraError::MissingPlane);
    }
    out.reserve(4 * w * h);
    for y in (0..h).rev() {
        for i in y * w..(y + 1) * w {
            out.extend_from_slice(&[r[i], g[i], b[i], a.as_ref().map_or(0xff, |a| a[i])]);
        }
    }
    Ok(out)
}

/// Converts an Aurora file to a SOFTIMAGE picture file row by row
/// [soft 0x085c1474 SI_pictureImportAurora]. Returns the bytes of the picture file.
///
/// The picture header is written (ratio 1.0, `fields` 3, colour and alpha), then for each
/// row from the file's last to its first the routine seeks to it, reads it whole and writes
/// it as two mixed sequences, colour and alpha.
///
/// What distinguishes it from [`read_aurora`] followed by the picture writer:
/// - **A file that is too short is refused** (`UnexpectedEnd`, shown to the user) where the
///   reader in memory fills with 0xff and succeeds. The picture file has been created by
///   then and is left with its header only.
/// - The rows are always written in the mixed coding, also when the output is standard
///   output (`to_stdout`: the name starts with `-`), although the header written for
///   standard output declares raw packets. Such output cannot be read back.
/// - A width of 0 gives a header that declares colour only (there are no row buffers, so
///   the header writer sees no alpha) and no rows.
pub fn import_aurora(data: &[u8], comment: Option<&[u8]>, to_stdout: bool) -> Result<Vec<u8>, AuroraError> {
    let mut s = Stream::new(data);
    let (width, height) = read_size(&mut s);
    if width < 0 && height > 0 {
        return Err(AuroraError::BadSize);
    }
    let w = width.max(0) as usize;
    let mut out = Vec::new();
    // the four row buffers exist (and so count as planes for the header) when width > 0
    softpic::write_header(&mut out, to_stdout, (width, height, 1.0, 3), [w > 0; 4], comment);
    let start = s.tell();
    let mut rows: [Vec<u8>; 4] = std::array::from_fn(|_| vec![0u8; w]);
    let mut buf = vec![0u8; 4 * w];
    for y in (0..height.max(0) as usize).rev() {
        s.seek(4 * w * y + start);
        if s.read(&mut buf) != 4 * w {
            return Err(AuroraError::UnexpectedEnd);
        }
        for x in 0..w {
            for (c, row) in rows.iter_mut().enumerate() {
                row[x] = buf[4 * x + c];
            }
        }
        // the mixed coder is called directly, twice; for a width of 0 it writes nothing
        if w > 0 {
            write_mixed(&mut out, &[&rows[0], &rows[1], &rows[2]]);
            write_mixed(&mut out, &[&rows[3]]);
        }
    }
    Ok(out)
}

/// Converts a SOFTIMAGE picture file to an Aurora file
/// [soft 0x085c1818 SI_pictureExportAurora]. Returns the bytes of the Aurora file.
///
/// The picture file's rows are coded with runs and cannot be reached backwards, so the
/// routine reads through all of them once, noting where each starts, and then seeks to them
/// from the last to the first, decodes each again and writes it.
///
/// What distinguishes it from the picture reader followed by [`write_aurora`]:
/// - `default_alpha` (the low byte of the routine's third argument) is the fourth byte of
///   every pixel when the picture file has no alpha plane; the writer in memory uses 0xff.
/// - The scanline reader's result is ignored and the row buffers are never cleared: rows the
///   picture file lacks come out as whatever the buffers held last (after the first pass
///   that is the last row that could be read, or part of it).
/// - A negative height writes the header alone.
pub fn export_aurora(pic_file: &[u8], default_alpha: u8) -> Result<Vec<u8>, AuroraError> {
    let mut pic = softpic::PicIn::open(pic_file).map_err(AuroraError::Picture)?;
    let mut out = Vec::new();
    write_size(&mut out, pic.width, pic.height);
    if pic.height <= 0 {
        return Ok(out);
    }
    if pic.width < 0 {
        return Err(AuroraError::BadSize);
    }
    let mut bufs = pic.row_buffers();
    let mut offsets = Vec::with_capacity(pic.height as usize);
    for _ in 0..pic.height {
        offsets.push(pic.tell());
        pic.scanline(&mut bufs);
    }
    for &at in offsets.iter().rev() {
        pic.seek(at);
        pic.scanline(&mut bufs);
        let [Some(r), Some(g), Some(b), a] = &bufs else { return Err(AuroraError::MissingPlane) };
        for x in 0..pic.width as usize {
            out.extend_from_slice(&[r[x], g[x], b[x], a.as_ref().map_or(default_alpha, |a| a[x])]);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alias::testpic::picture;

    #[test]
    fn round_trip() {
        for (seed, w, h) in [(1, 1, 1), (2, 7, 5), (3, 300, 4), (4, 2, 9)] {
            let p = picture(seed, w, h, true);
            let bytes = write_aurora(&p).unwrap();
            assert_eq!(bytes.len(), 8 + 4 * w * h);
            assert_eq!(&bytes[..8], &[0, 0, 0, 0, w as u8, (w >> 8) as u8, h as u8, 0]);
            assert_eq!(read_aurora(&bytes).unwrap(), p);
            // without alpha the file carries 0xff, which comes back as a plane
            let mut q = p.clone();
            q.planes[3] = None;
            let bytes = write_aurora(&q).unwrap();
            let got = read_aurora(&bytes).unwrap();
            assert_eq!(got.planes[3], Some(vec![0xff; w * h]));
            assert_eq!(got.planes[..3], q.planes[..3]);
        }
    }

    #[test]
    fn rows_are_stored_last_first() {
        let p = Picture { width: 2, height: 2, ratio: 1.0, fields: 3, pad: 0, planes: [Some(vec![1, 2, 3, 4]), Some(vec![5, 6, 7, 8]), Some(vec![9, 10, 11, 12]), Some(vec![13, 14, 15, 16])] };
        let bytes = write_aurora(&p).unwrap();
        assert_eq!(&bytes[8..], &[3, 7, 11, 15, 4, 8, 12, 16, 1, 5, 9, 13, 2, 6, 10, 14]);
    }

    #[test]
    fn truncation_at_every_byte() {
        let p = picture(5, 6, 4, true);
        let bytes = write_aurora(&p).unwrap();
        for cut in 8..bytes.len() {
            let got = read_aurora(&bytes[..cut]).unwrap();
            // the file's k-th pixel byte belongs to row 3 − k / 24
            for k in 0..96 {
                let (row, x, c) = (3 - k / 24, k % 24 / 4, k % 4);
                let v = got.planes[c].as_ref().unwrap()[row * 6 + x];
                assert_eq!(v, if 8 + k < cut { bytes[8 + k] } else { 0xff }, "cut {cut} byte {k}");
            }
            assert_eq!(import_aurora(&bytes[..cut], None, false).unwrap_err(), AuroraError::UnexpectedEnd);
        }
        assert_eq!(AuroraError::UnexpectedEnd.message("a.sim"), "Unexpected end of file in input file a.sim\n");
    }

    #[test]
    fn short_headers_still_give_a_size() {
        // eight bytes: little-endian words at 4 and 6, the first four bytes ignored
        let size = |f: &[u8]| {
            let p = read_aurora(f).unwrap();
            (p.width, p.height)
        };
        assert_eq!(size(&[9, 9, 9, 9, 2, 0, 3, 0]), (2, 3));
        // seven bytes: the height word is not whole: byte 6 lands on the first byte of the
        // word read from bytes 2 and 3, unswapped, and the pair is then read big-endian
        assert_eq!(size(&[9, 9, 0, 1, 2, 0, 0]), (2, 1));
        assert_eq!(size(&[9, 9, 5, 1, 2, 0, 0]), (2, 1));
        // six bytes: height is bytes 2 and 3, big-endian
        assert_eq!(size(&[9, 9, 0, 3, 2, 0]), (2, 3));
        // four bytes: both words from the first four bytes, big-endian
        assert_eq!(size(&[0, 2, 0, 3]), (2, 3));
        // nothing: stale stack in the original, 0 here
        assert_eq!(size(&[]), (0, 0));
        // all of these succeed; the pixels are 0xff
        assert_eq!(read_aurora(&[0, 2, 0, 3]).unwrap().planes[0], Some(vec![0xff; 6]));
    }

    #[test]
    fn odd_sizes() {
        let hdr = |w: i16, h: i16| {
            let mut f = Vec::new();
            write_size(&mut f, w, h);
            f
        };
        for (w, h) in [(0i16, 4i16), (4, 0), (-2, 3), (2, -3)] {
            assert_eq!(read_aurora(&hdr(w, h)).unwrap().planes, [None, None, None, None]);
        }
        // both negative: planes of |w × h| bytes that nothing is read into
        let p = read_aurora(&hdr(-2, -3)).unwrap();
        assert_eq!(p.planes[0], Some(vec![0x6f, 0, 0, 0, 0, 0x6b]));
        assert_eq!(read_aurora(&hdr(32767, 32767)).unwrap_err(), AuroraError::TooLarge);
        assert_eq!(read_aurora(&hdr(-32768, -32768)).unwrap_err(), AuroraError::TooLarge);
        // writer: nothing but the header for sizes of zero or less, planes not looked at
        let q = Picture { width: -2, height: 3, ..Default::default() };
        assert_eq!(write_aurora(&q).unwrap(), [0, 0, 0, 0, 0xfe, 0xff, 3, 0]);
        let q = Picture { width: 2, height: 3, ..Default::default() };
        assert_eq!(write_aurora(&q).unwrap_err(), AuroraError::MissingPlane);
    }

    #[test]
    fn import_equals_read_then_write() {
        for (seed, w, h) in [(11, 1, 1), (12, 7, 5), (13, 300, 4)] {
            let p = picture(seed, w, h, true);
            let sim = write_aurora(&p).unwrap();
            assert_eq!(import_aurora(&sim, Some(b"c"), false).unwrap(), p.write(Some(b"c"), false), "seed {seed}");
            // to standard output the header declares raw packets and the rows are mixed all the same
            let out = import_aurora(&sim, None, true).unwrap();
            let file = p.write(None, false);
            assert_eq!(&out[..104], &p.write(None, true)[..104]);
            assert_eq!(&out[104..120], &[1, 8, 0, 0x80, 1, 8, 0, 0x40, 1, 8, 0, 0x20, 0, 8, 0, 0x10]);
            assert_eq!(&out[120..], &file[112..]);
        }
        // extra bytes after the last row are ignored
        let p = picture(14, 3, 2, true);
        let mut sim = write_aurora(&p).unwrap();
        sim.extend_from_slice(&[1, 2, 3]);
        assert_eq!(import_aurora(&sim, None, false).unwrap(), p.write(None, false));
    }

    #[test]
    fn import_odd_sizes() {
        let hdr = |w: i16, h: i16| {
            let mut f = Vec::new();
            write_size(&mut f, w, h);
            f
        };
        // width 0: colour packet only, no rows
        let out = import_aurora(&hdr(0, 5), None, false).unwrap();
        assert_eq!((out.len(), &out[104..108]), (108, &[0u8, 8, 2, 0xe0][..]));
        // height 0 or less: both packets, no rows
        for h in [0i16, -5] {
            let out = import_aurora(&hdr(5, h), None, false).unwrap();
            assert_eq!((out.len(), &out[104..112]), (112, &[1u8, 8, 2, 0xe0, 0, 8, 2, 0x10][..]));
        }
        assert_eq!(import_aurora(&hdr(-5, 5), None, false).unwrap_err(), AuroraError::BadSize);
        assert_eq!(import_aurora(&hdr(-5, -5), None, false).unwrap().len(), 108);
    }

    #[test]
    fn export_equals_read_then_write_for_whole_files() {
        for (seed, w, h, alpha) in [(21, 1, 1, true), (22, 7, 5, true), (23, 300, 4, true), (24, 9, 3, false)] {
            let p = picture(seed, w, h, alpha);
            for to_stdout in [false, true] {
                let file = p.write(None, to_stdout);
                assert_eq!(export_aurora(&file, 0xff).unwrap(), write_aurora(&p).unwrap(), "seed {seed}");
            }
        }
        // the default alpha is the caller's
        let p = picture(25, 4, 2, false);
        let sim = export_aurora(&p.write(None, false), 0x12).unwrap();
        assert!(sim[8..].chunks(4).all(|px| px[3] == 0x12));
    }

    #[test]
    fn export_of_a_cut_picture() {
        let p = picture(31, 6, 4, false);
        let file = p.write(None, true); // raw rows of 18 bytes after 116 bytes of header
        // rows 0 and 1 whole; row 2 red only
        let cut = &file[..116 + 2 * 18 + 6];
        let sim = export_aurora(cut, 0).unwrap();
        let got = read_aurora(&sim).unwrap();
        let (r, g) = (p.planes[0].as_ref().unwrap(), p.planes[1].as_ref().unwrap());
        let (gr, gg) = (got.planes[0].as_ref().unwrap(), got.planes[1].as_ref().unwrap());
        // rows 0 and 1 are right
        assert_eq!((&gr[..12], &gg[..12]), (&r[..12], &g[..12]));
        // row 3 is written first, from the buffers as the first pass left them: red of row 2
        // (the last bytes that could be read), green of row 1
        assert_eq!((&gr[18..], &gg[18..]), (&r[12..18], &g[6..12]));
        // row 2 is read again and gives the same
        assert_eq!((&gr[12..18], &gg[12..18]), (&r[12..18], &g[6..12]));
        assert_eq!(export_aurora(&file[..60], 0).unwrap_err(), AuroraError::Picture(softpic::PicError::CannotRead));
    }

    #[test]
    fn export_sizes() {
        let p = picture(41, 5, 3, false);
        let file = p.write(None, false);
        let mut f = file.clone();
        f[94..96].copy_from_slice(&(-3i16).to_be_bytes());
        assert_eq!(export_aurora(&f, 0).unwrap(), [0, 0, 0, 0, 5, 0, 0xfd, 0xff]);
        let mut f = file.clone();
        f[92..94].copy_from_slice(&(-5i16).to_be_bytes());
        assert_eq!(export_aurora(&f, 0).unwrap_err(), AuroraError::BadSize);
        let mut f = file.clone();
        f[107] = 0x10;
        assert_eq!(export_aurora(&f, 0).unwrap_err(), AuroraError::MissingPlane);
    }
}
