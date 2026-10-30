// SPDX-License-Identifier: MIT
//! Alias pictures (`.als`). Original source file `alias.c`.
//!
//! Layout as this program reads and writes it:
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 2 | width |
//! | 2 | 2 | height |
//! | 4 | 2 | written as 0, never read |
//! | 6 | 2 | written as height − 1, never read |
//! | 8 | 2 | written as 24, never read |
//! | 10 | 4 × n | runs: count, blue, green, red |
//!
//! All fields big-endian 16-bit. A run is `count` (1 to 255) equal pixels; rows run first row
//! first. The writers end a run at the end of every row; the readers do not need that.
//! There is no alpha and no test of any kind on the header: any ten bytes are a header.
//! (Outside this program the three unread words are known as x offset, y offset and bits per
//! pixel of the Alias "pix" format; here they are only the constants above.)
//!
//! The file has four routines: a reader and a writer on pictures in memory, and an import and
//! an export that convert to and from a SOFTIMAGE picture file row by row without holding the
//! picture. They differ in more than memory use; see each.

use si_file::Stream;
use si_picture::Picture;

pub(crate) use crate::softpic::{self, si_malloc};

/// The extension every routine here hands to the opener (modes `rb` and `wb`).
pub const EXTENSION: &str = ".als";

const HEADER_SIZE: usize = 10;

/// How many pixels beyond what the rest of a file can possibly describe a reader of this
/// crate still allocates for. The originals allocate `width × height` whatever the file
/// holds and fill what the file lacks; that is reproduced up to this allowance and refused
/// beyond it, so that a few bytes of header cannot claim a gigabyte.
pub(crate) const SLACK_PIXELS: usize = 1 << 20;

/// Whether a reader may allocate `pixels` when the bytes left can describe `from_file` at most.
pub(crate) fn fits(pixels: usize, from_file: usize) -> bool {
    pixels <= from_file.saturating_add(SLACK_PIXELS)
}

/// The cases in which a routine of this file gives up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AliasError {
    /// [`read_alias`]: fewer than ten bytes. Nothing is shown.
    ShortHeader,
    /// [`import_alias`]: fewer than ten bytes.
    CannotReadInput,
    /// [`export_alias`]: the SOFTIMAGE picture file was refused.
    Picture(softpic::PicError),
    /// Not a case of the original: `width × height` is more than [`SLACK_PIXELS`] beyond
    /// what the file's bytes can describe. The original allocates it all the same.
    TooLarge,
    /// Not a case of the original: a size its loops cannot survive (a negative width or
    /// height in a writer, which counts down through the whole 16-bit range reading past
    /// the planes; two negative sizes in the import; a negative width in the export).
    BadSize,
    /// Not a case of the original: a writer was given a picture without a red, green or blue
    /// plane, or with one shorter than `width × height`. The original reads through the
    /// pointer regardless.
    MissingPlane,
    /// Not a case of the original: the import met a run that reaches more than one whole row
    /// past the end of the row it started in, and the file goes on. The original has by then
    /// written past its row buffers and next asks `memset` for a negative length.
    RunPastRow,
}

impl AliasError {
    /// The text the original shows. `name` is what the routine was given, except for
    /// [`AliasError::CannotReadInput`], where the original prints the name the opener
    /// reports: directory removed, extension added if the opener added it.
    pub fn message(&self, name: &str) -> String {
        match self {
            AliasError::CannotReadInput => format!("Cannot read input file {name}\n"),
            AliasError::Picture(e) => e.message(name),
            _ => String::new(),
        }
    }
}

/// The ten-byte header. The names of the last three fields are not from this program.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AliasHeader {
    pub width: i16,
    pub height: i16,
    pub x_offset: i16,
    pub y_offset: i16,
    pub bits: i16,
}

impl AliasHeader {
    /// What every writer here puts in the header for a picture of this size.
    pub fn for_size(width: i16, height: i16) -> Self {
        AliasHeader { width, height, x_offset: 0, y_offset: height.wrapping_sub(1), bits: 24 }
    }
}

/// [soft 0x085bf654 si_AliasHeader_fread]: one record of five 16-bit fields, byte-swapped
/// after reading. `None` if the file has fewer than ten bytes left (what was there is
/// consumed).
fn read_header(s: &mut Stream) -> Option<AliasHeader> {
    let mut b = [0u8; HEADER_SIZE];
    if !s.read_record(&mut b) {
        return None;
    }
    let f = |i: usize| i16::from_be_bytes([b[i], b[i + 1]]);
    Some(AliasHeader { width: f(0), height: f(2), x_offset: f(4), y_offset: f(6), bits: f(8) })
}

/// [soft 0x085bf708 si_AliasHeader_fwrite]: the record byte-swapped, written, swapped back.
fn write_header(out: &mut Vec<u8>, h: &AliasHeader) {
    for v in [h.width, h.height, h.x_offset, h.y_offset, h.bits] {
        out.extend_from_slice(&v.to_be_bytes());
    }
}

/// Reads an Alias picture into memory [soft 0x085be980 SI_pictureReadAlias] (wrapped
/// unchanged by [soft 0x085bf638 readAliasPicture]).
///
/// Width and height come from the header, untested; ratio 1.0, `fields` 3, no alpha. Then
/// runs are read until `width × height` pixels are covered: four `getc` calls per run, with
/// no test for the end of the file. Past the end each call gives −1, so a file that stops
/// early continues as runs of 255 pixels of 0xff, 0xff, 0xff and the reader still succeeds.
/// A count of 0 is a run of no pixels. Runs may cross rows.
///
/// A size whose product is zero or negative gives a picture without planes (the allocator
/// returns nothing for it) and reads no runs. `pad` is not touched by the original.
pub fn read_alias(data: &[u8]) -> Result<Picture, AliasError> {
    let mut s = Stream::new(data);
    let h = read_header(&mut s).ok_or(AliasError::ShortHeader)?;
    let mut pic = Picture { width: h.width, height: h.height, ratio: 1.0, fields: 3, pad: 0, planes: Default::default() };
    let total = h.height as i32 * h.width as i32;
    if total <= 0 {
        return Ok(pic);
    }
    let n = total as usize;
    // each four bytes left can give 255 pixels at most; the original does not look
    if !fits(n, (data.len() - HEADER_SIZE).div_ceil(4).saturating_mul(255)) {
        return Err(AliasError::TooLarge);
    }
    let (mut red, mut green, mut blue) = (vec![0u8; n], vec![0u8; n], vec![0u8; n]);
    let mut at = 0usize;
    while at < n {
        let count = s.getc().unwrap_or(0xff) as usize;
        let b = s.getc().unwrap_or(0xff);
        let g = s.getc().unwrap_or(0xff);
        let r = s.getc().unwrap_or(0xff);
        // the last run is not cut to what is left: the original writes all of it, up to 254
        // bytes past the end of each plane. Here it stops at the end.
        let end = (at + count).min(n);
        red[at..end].fill(r);
        green[at..end].fill(g);
        blue[at..end].fill(b);
        at += count;
    }
    pic.planes = [Some(red), Some(green), Some(blue), None];
    Ok(pic)
}

/// One row as runs: the coding both writers use (inlined in the picture writer,
/// [soft 0x085bf4e8 scanlineWriteAlias] in the export).
///
/// A run ends when the pixel changes, when it reaches 255 pixels, and at the end of the row.
fn write_row(out: &mut Vec<u8>, r: &[u8], g: &[u8], b: &[u8]) {
    let mut count = 0u32;
    let mut cur = (0u8, 0u8, 0u8);
    let put = |out: &mut Vec<u8>, count: u32, cur: (u8, u8, u8)| out.extend_from_slice(&[count as u8, cur.2, cur.1, cur.0]);
    for x in 0..r.len() {
        let px = (r[x], g[x], b[x]);
        count += 1;
        if count == 1 {
            cur = px;
        } else {
            if px != cur {
                put(out, count - 1, cur);
                cur = px;
                count = 1;
            }
            if count == 0xff {
                put(out, 0xff, cur);
                count = 0;
            }
        }
    }
    if count != 0 {
        put(out, count, cur);
    }
}

/// Writes a picture in memory as an Alias file [soft 0x085beb48 pictureWriteAlias].
///
/// Header: width, height, 0, height − 1, 24. Then every row as runs (see the module text).
/// Alpha is not written: the format as written here has no place for it.
pub fn write_alias(pic: &Picture) -> Result<Vec<u8>, AliasError> {
    if pic.width < 0 || pic.height < 0 {
        return Err(AliasError::BadSize);
    }
    let (w, h) = (pic.width as usize, pic.height as usize);
    let mut out = Vec::new();
    write_header(&mut out, &AliasHeader::for_size(pic.width, pic.height));
    if w * h == 0 {
        return Ok(out); // no pixel is read, so the planes may be absent
    }
    let [Some(r), Some(g), Some(b), _] = &pic.planes else { return Err(AliasError::MissingPlane) };
    if r.len().min(g.len()).min(b.len()) < w * h {
        return Err(AliasError::MissingPlane);
    }
    for y in 0..h {
        let row = y * w..(y + 1) * w;
        write_row(&mut out, &r[row.clone()], &g[row.clone()], &b[row]);
    }
    Ok(out)
}

/// Converts an Alias file to a SOFTIMAGE picture file without holding the picture
/// [soft 0x085bedb4 SI_pictureImportAlias]. Returns the bytes of the picture file.
///
/// The header is written first (ratio 1.0, `fields` 3, colour only), then runs are read into
/// one row's buffers and each completed row is written in the picture format's mixed coding.
/// `to_stdout`: the output name starts with `-` (see [`softpic::write_header`]).
///
/// What distinguishes it from [`read_alias`] followed by the picture writer:
/// - A header shorter than ten bytes is reported to the user (`CannotReadInput`).
/// - A run is carried over **one** row boundary only. A run ending exactly one row further
///   on is still handled (that row is written when the next run arrives); a run that reaches
///   further overruns the row buffers, and unless it is the file's last run the routine then
///   crashes. So Alias files from writers that let runs cross rows convert correctly only
///   while every run is shorter than a row, which fails for pictures under 255 pixels wide.
/// - Rows are written only when a later run completes them, and nothing is flushed at the
///   end, so if the runs stop short the picture file simply has fewer rows than its header
///   says. A file cut short still "completes": the missing runs read as 255 × 0xff.
/// - The picture chunk's padding word is stale stack in the original (0 here).
pub fn import_alias(data: &[u8], comment: Option<&[u8]>, to_stdout: bool) -> Result<Vec<u8>, AliasError> {
    let mut s = Stream::new(data);
    let h = read_header(&mut s).ok_or(AliasError::CannotReadInput)?;
    let w = h.width as i32;
    let total = h.height as i32 * w;
    if total > 0 && w < 0 {
        return Err(AliasError::BadSize); // two negative sizes: no row buffers, and a negative memset at once
    }
    let mut out = Vec::new();
    // the original calls the header writer before it has set the three colour pointers of
    // its picture record; a file gets the same packet whatever they hold, standard output
    // gets one raw packet per non-null pointer. We take all three as set.
    softpic::write_header(&mut out, to_stdout, (h.width, h.height, 1.0, 3), [true, true, true, false], comment);
    if total <= 0 {
        return Ok(out);
    }
    let w = w as usize;
    let mut rows: [Vec<u8>; 3] = std::array::from_fn(|_| si_malloc(w as i32).unwrap());
    let mut x = 0usize; // pixels held in the row buffers
    let mut done = 0i32;
    loop {
        let count = s.getc().unwrap_or(0xff) as usize;
        let b = s.getc().unwrap_or(0xff);
        let g = s.getc().unwrap_or(0xff);
        let r = s.getc().unwrap_or(0xff);
        let px = [r, g, b];
        if x > w {
            // the previous run left more than a row behind: negative length in the original
            return Err(AliasError::RunPastRow);
        }
        if x + count < w {
            for (row, v) in rows.iter_mut().zip(px) {
                row[x..x + count].fill(v);
            }
            x += count;
        } else {
            for (row, v) in rows.iter_mut().zip(px) {
                row[x..w].fill(v);
            }
            x = x + count - w;
            softpic::write_row(&mut out, to_stdout, [Some(&rows[0]), Some(&rows[1]), Some(&rows[2]), None]);
            // what is left of the run starts the next row; beyond one row the original
            // writes past the buffers, here the fill stops at their end
            for (row, v) in rows.iter_mut().zip(px) {
                row[..x.min(w)].fill(v);
            }
        }
        done += count as i32;
        if done >= total {
            break;
        }
    }
    Ok(out)
}

/// Converts a SOFTIMAGE picture file to an Alias file row by row
/// [soft 0x085bf208 SI_pictureExportAlias]. Returns the bytes of the Alias file.
///
/// The picture file is opened as every reader of it does (`Picture` errors are that
/// routine's), the header is written from its width and height, and then for each row the
/// picture's scanline reader fills one row buffer per plane and the row is written as runs.
///
/// What distinguishes it from the picture reader followed by [`write_alias`]:
/// - The scanline reader's result is ignored and the row buffers are never cleared, so when
///   the picture file is cut short or badly coded every remaining row is written from
///   whatever the buffers hold: the last rows read, or for a file with no row data at all
///   the allocator's fill (here: 0x6f, zeros, 0x6b). The reader in memory leaves such rows 0.
/// - A negative height is not refused: the file is a header and nothing else.
pub fn export_alias(pic_file: &[u8]) -> Result<Vec<u8>, AliasError> {
    let mut pic = softpic::PicIn::open(pic_file).map_err(AliasError::Picture)?;
    let mut out = Vec::new();
    write_header(&mut out, &AliasHeader::for_size(pic.width, pic.height));
    if pic.height <= 0 {
        return Ok(out);
    }
    if pic.width < 0 {
        return Err(AliasError::BadSize);
    }
    let mut bufs = pic.row_buffers();
    for _ in 0..pic.height {
        pic.scanline(&mut bufs);
        let [Some(r), Some(g), Some(b), _] = &bufs else { return Err(AliasError::MissingPlane) };
        write_row(&mut out, r, g, b);
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod testpic {
    use si_picture::Picture;

    /// A made-up picture with runs of many lengths (seeded, deterministic).
    pub fn picture(seed: u32, width: usize, height: usize, alpha: bool) -> Picture {
        let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(12345);
        let mut next = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state >> 16
        };
        let n = width * height;
        let mut planes: [Vec<u8>; 4] = Default::default();
        let mut made = 0usize;
        while made < n {
            let run = match next() % 5 {
                0 => 1,
                1 => 2,
                2 => 1 + next() as usize % 7,
                3 => 250 + next() as usize % 12,
                _ => 1 + next() as usize % 40,
            }
            .min(n - made);
            let px = [next() as u8, (next() % 3) as u8, (next() % 2 * 255) as u8, next() as u8];
            for (p, v) in planes.iter_mut().zip(px) {
                p.extend(std::iter::repeat(v).take(run));
            }
            made += run;
        }
        let [r, g, b, a] = planes;
        Picture { width: width as i16, height: height as i16, ratio: 1.0, fields: 3, pad: 0, planes: [Some(r), Some(g), Some(b), alpha.then_some(a)] }
    }
}

#[cfg(test)]
mod tests {
    use super::testpic::picture;
    use super::*;

    fn rgb(p: &Picture) -> Picture {
        let mut q = p.clone();
        q.planes[3] = None;
        q
    }

    #[test]
    fn round_trip() {
        for (seed, w, h) in [(1, 1, 1), (2, 7, 5), (3, 300, 4), (4, 255, 3), (5, 256, 2), (6, 1, 40), (7, 513, 9)] {
            for alpha in [false, true] {
                let p = picture(seed, w, h, alpha);
                let bytes = write_alias(&p).unwrap();
                assert_eq!(&bytes[..10], &[(w >> 8) as u8, w as u8, 0, h as u8, 0, 0, 0, (h - 1) as u8, 0, 24]);
                assert_eq!((bytes.len() - 10) % 4, 0);
                // alpha is lost, everything else comes back
                assert_eq!(read_alias(&bytes).unwrap(), rgb(&p), "seed {seed}");
            }
        }
    }

    #[test]
    fn writer_run_forms() {
        let enc = |row: &[u8]| {
            let p = Picture { width: row.len() as i16, height: 1, ratio: 1.0, fields: 3, pad: 0, planes: [Some(row.to_vec()), Some(vec![2; row.len()]), Some(vec![3; row.len()]), None] };
            write_alias(&p).unwrap()[10..].to_vec()
        };
        assert_eq!(enc(&[9]), [1, 3, 2, 9]); // count, blue, green, red
        assert_eq!(enc(&[9, 9, 8]), [2, 3, 2, 9, 1, 3, 2, 8]);
        assert_eq!(enc(&[9; 255]), [255, 3, 2, 9]);
        assert_eq!(enc(&[9; 256]), [255, 3, 2, 9, 1, 3, 2, 9]);
        assert_eq!(enc(&[9; 510]), [255, 3, 2, 9, 255, 3, 2, 9]);
        // runs end with the row
        let p = Picture { width: 2, height: 2, ratio: 1.0, fields: 3, pad: 0, planes: [Some(vec![5; 4]), Some(vec![5; 4]), Some(vec![5; 4]), None] };
        assert_eq!(&write_alias(&p).unwrap()[10..], &[2, 5, 5, 5, 2, 5, 5, 5]);
    }

    #[test]
    fn header_is_not_tested_and_odd_sizes() {
        // any ten bytes are a header; the three unread words do not matter
        let mut f = vec![0, 2, 0, 1, 0xde, 0xad, 0xbe, 0xef, 0, 1];
        f.extend_from_slice(&[2, 30, 20, 10]);
        let p = read_alias(&f).unwrap();
        assert_eq!((p.width, p.height, p.ratio, p.fields), (2, 1, 1.0, 3));
        assert_eq!(p.planes, [Some(vec![10, 10]), Some(vec![20, 20]), Some(vec![30, 30]), None]);
        // zero or negative product: no planes, success
        for (w, h) in [(0i16, 5i16), (5, 0), (-3, 4), (3, -4)] {
            let mut f = Vec::new();
            write_header(&mut f, &AliasHeader::for_size(w, h));
            let p = read_alias(&f).unwrap();
            assert_eq!((p.width, p.height), (w, h));
            assert_eq!(p.planes, [None, None, None, None]);
        }
        // two negative sizes give a positive count
        let mut f = Vec::new();
        write_header(&mut f, &AliasHeader::for_size(-2, -3));
        f.extend_from_slice(&[6, 1, 2, 3]);
        assert_eq!(read_alias(&f).unwrap().planes[0], Some(vec![3; 6]));
        assert_eq!(write_alias(&read_alias(&f).unwrap()).unwrap_err(), AliasError::BadSize);
        assert_eq!(read_alias(&[0; 9]).unwrap_err(), AliasError::ShortHeader);
        assert_eq!(AliasError::ShortHeader.message("x"), "");
    }

    #[test]
    fn reader_runs_cross_rows_zero_counts_and_overrun() {
        let mut f = Vec::new();
        write_header(&mut f, &AliasHeader::for_size(3, 2));
        f.extend_from_slice(&[0, 9, 9, 9, 4, 1, 2, 3, 0, 9, 9, 9, 200, 4, 5, 6]);
        let p = read_alias(&f).unwrap();
        assert_eq!(p.planes[0], Some(vec![3, 3, 3, 3, 6, 6])); // the last run is cut at the plane's end
        assert_eq!(p.planes[2], Some(vec![1, 1, 1, 1, 4, 4]));
    }

    #[test]
    fn truncation_at_every_byte() {
        let p = picture(11, 9, 6, false);
        let bytes = write_alias(&p).unwrap();
        for cut in 0..bytes.len() {
            let got = read_alias(&bytes[..cut]);
            if cut < 10 {
                assert_eq!(got.unwrap_err(), AliasError::ShortHeader);
                continue;
            }
            let got = got.unwrap();
            // whole runs before the cut are intact; from the cut on, the missing bytes read
            // as 0xff: the cut run takes 0xff for each missing byte, later runs are 255 × ff
            let whole = (cut - 10) / 4;
            let mut expect = [Vec::new(), Vec::new(), Vec::new()];
            let mut k = 10;
            let mut runs = 0;
            while expect[0].len() < 54 {
                let mut run = [0xffu8; 4];
                for (i, v) in run.iter_mut().enumerate() {
                    if k + i < cut {
                        *v = bytes[k + i];
                    }
                }
                k += 4;
                for (e, v) in expect.iter_mut().zip([run[3], run[2], run[1]]) {
                    e.extend(std::iter::repeat(v).take(run[0] as usize));
                    e.truncate(54);
                }
                runs += 1;
            }
            assert!(runs >= whole.min(1));
            for i in 0..3 {
                assert_eq!(got.planes[i].as_ref().unwrap(), &expect[i], "cut {cut}");
            }
        }
        // header only: all 0xff
        assert_eq!(read_alias(&bytes[..10]).unwrap().planes[1], Some(vec![0xff; 54]));
    }

    #[test]
    fn oversized_header_is_refused_here() {
        let mut f = Vec::new();
        write_header(&mut f, &AliasHeader::for_size(32767, 32767));
        assert_eq!(read_alias(&f).unwrap_err(), AliasError::TooLarge);
        // within the allowance the original's fill is reproduced
        let mut f = Vec::new();
        write_header(&mut f, &AliasHeader::for_size(1024, 1024));
        assert_eq!(read_alias(&f).unwrap().planes[0].as_ref().unwrap().len(), 1 << 20);
    }

    #[test]
    fn softpic_writes_what_the_picture_writer_writes() {
        for alpha in [false, true] {
            let p = picture(21, 40, 7, alpha);
            for to_stdout in [false, true] {
                let mut out = Vec::new();
                softpic::write_header(&mut out, to_stdout, (40, 7, 1.0, 3), [true, true, true, alpha], Some(b"c"));
                for y in 0..7 {
                    let row = |i: usize| p.planes[i].as_ref().map(|pl| &pl[y * 40..(y + 1) * 40]);
                    softpic::write_row(&mut out, to_stdout, [row(0), row(1), row(2), row(3)]);
                }
                assert_eq!(out, p.write(Some(b"c"), to_stdout));
            }
        }
    }

    #[test]
    fn softpic_reads_rows_as_the_picture_reader_does() {
        for alpha in [false, true] {
            for to_stdout in [false, true] {
                let p = picture(22, 33, 6, alpha);
                let file = p.write(None, to_stdout);
                let mut pin = softpic::PicIn::open(&file).unwrap();
                assert_eq!((pin.width, pin.height, pin.ratio, pin.fields), (33, 6, 1.0, 3));
                let mut bufs = pin.row_buffers();
                assert_eq!(bufs[3].is_some(), alpha);
                assert_eq!((bufs[0].as_ref().unwrap()[0], bufs[0].as_ref().unwrap()[32]), (0x6f, 0x6b));
                for y in 0..6 {
                    assert!(pin.scanline(&mut bufs));
                    for i in 0..4 {
                        assert_eq!(bufs[i].as_deref(), p.planes[i].as_ref().map(|pl| &pl[y * 33..(y + 1) * 33]));
                    }
                }
                assert!(!pin.scanline(&mut bufs)); // past the last row: end of file
            }
        }
    }

    #[test]
    fn softpic_refusals() {
        let file = picture(23, 4, 4, false).write(None, false);
        let err = |f: &[u8]| softpic::PicIn::open(f).err().unwrap();
        assert_eq!(err(&file[..87]), softpic::PicError::CannotRead);
        assert_eq!(err(&file[..90]), softpic::PicError::CannotRead);
        assert_eq!(err(&file[..103]), softpic::PicError::CannotRead);
        let mut f = file.clone();
        f[3] = 0x41;
        assert_eq!(err(&f), softpic::PicError::NotSoftimage);
        let mut f = file.clone();
        f[91] = b'X';
        assert_eq!(err(&f), softpic::PicError::NotPicture);
        let mut f = file.clone();
        f[94..96].fill(0);
        assert_eq!(err(&f), softpic::PicError::InvalidSize(4, 0));
        assert_eq!(err(&f).message("a.pic"), "Picture a.pic has invalid size 4 x 0\n");
        assert_eq!(softpic::PicError::NotSoftimage.message("a"), "The file a is not a SOFTIMAGE file.\n");
        assert_eq!(softpic::PicError::NotPicture.message("a"), "File a is not a SOFTIMAGE Picture file.\n");
        assert_eq!(AliasError::Picture(softpic::PicError::CannotRead).message("a"), "Cannot read input file a\n");
        // the packet list ends with the file: header only is one empty packet
        assert_eq!(softpic::PicIn::open(&file[..104]).unwrap().packets.len(), 1);
    }

    #[test]
    fn import_equals_read_then_write_for_this_writer_s_files() {
        for (seed, w, h) in [(31, 1, 1), (32, 7, 5), (33, 300, 4), (34, 255, 3), (35, 256, 2), (36, 2, 9)] {
            let p = rgb(&picture(seed, w, h, false));
            let als = write_alias(&p).unwrap();
            for to_stdout in [false, true] {
                assert_eq!(import_alias(&als, Some(b"x"), to_stdout).unwrap(), p.write(Some(b"x"), to_stdout), "seed {seed}");
            }
        }
        assert_eq!(import_alias(&[1, 2, 3], None, false).unwrap_err(), AliasError::CannotReadInput);
        assert_eq!(AliasError::CannotReadInput.message("a.als"), "Cannot read input file a.als\n");
    }

    fn rows_of(pic_file: &[u8]) -> (usize, Picture) {
        // how many rows a picture file really holds, and the picture
        let f = Picture::read(pic_file).unwrap();
        let w = f.picture.width as usize;
        let mut rows = 0;
        let mut pin = softpic::PicIn::open(pic_file).unwrap();
        let mut bufs = pin.row_buffers();
        while pin.scanline(&mut bufs) {
            rows += 1;
        }
        let _ = w;
        (rows, f.picture)
    }

    #[test]
    fn import_runs_across_rows() {
        let hdr = |w: i16, h: i16| {
            let mut f = Vec::new();
            write_header(&mut f, &AliasHeader::for_size(w, h));
            f
        };
        // a run that crosses one row boundary is split correctly; the last row here is
        // complete in the buffers when the pixel count is reached and is never written
        let mut f = hdr(4, 3);
        f.extend_from_slice(&[6, 1, 2, 3, 6, 4, 5, 6]);
        let out = import_alias(&f, None, false).unwrap();
        let (rows, p) = rows_of(&out);
        assert_eq!(rows, 2);
        assert_eq!(&p.planes[0].as_ref().unwrap()[..8], &[3, 3, 3, 3, 3, 3, 6, 6]);
        assert_eq!(read_alias(&f).unwrap().planes[0], Some(vec![3, 3, 3, 3, 3, 3, 6, 6, 6, 6, 6, 6]));
        // a run ending exactly one row further on: the full row waits for the next run
        let mut f = hdr(4, 3);
        f.extend_from_slice(&[8, 1, 2, 3, 4, 4, 5, 6]);
        let (rows, p) = rows_of(&import_alias(&f, None, false).unwrap());
        assert_eq!(rows, 2); // the last row is complete in the buffers and never written
        assert_eq!(&p.planes[0].as_ref().unwrap()[..8], &[3; 8]);
        // one run for the whole picture, more than two rows long: one row reaches the file
        let mut f = hdr(4, 3);
        f.extend_from_slice(&[12, 1, 2, 3]);
        let (rows, _) = rows_of(&import_alias(&f, None, false).unwrap());
        assert_eq!(rows, 1);
        assert_eq!(read_alias(&f).unwrap().planes[0], Some(vec![3; 12]));
        // the same run with more of the file to come: the original cannot go on
        let mut f = hdr(4, 4);
        f.extend_from_slice(&[12, 1, 2, 3, 4, 1, 2, 3]);
        assert_eq!(import_alias(&f, None, false).unwrap_err(), AliasError::RunPastRow);
        // sizes with nothing to read: the header alone
        for (w, h) in [(0i16, 4i16), (4, 0), (-4, 4), (4, -4)] {
            assert_eq!(import_alias(&hdr(w, h), None, false).unwrap().len(), 108);
        }
        assert_eq!(import_alias(&hdr(-4, -4), None, false).unwrap_err(), AliasError::BadSize);
    }

    #[test]
    fn import_of_a_cut_file_never_fails() {
        let p = rgb(&picture(41, 300, 5, false));
        let als = write_alias(&p).unwrap();
        for cut in 10..als.len() {
            // every missing run is 255 pixels of 0xff; 300 > 255, so no run passes a row
            let out = import_alias(&als[..cut], None, false).unwrap();
            let (rows, got) = rows_of(&out);
            assert!(rows == 5 || rows == 4, "cut {cut}: {rows}");
            let want = read_alias(&als[..cut]).unwrap();
            let n = rows * 300;
            assert_eq!(&got.planes[0].as_ref().unwrap()[..n], &want.planes[0].as_ref().unwrap()[..n], "cut {cut}");
        }
    }

    #[test]
    fn export_equals_read_then_write_for_whole_files() {
        for (seed, w, h, alpha) in [(51, 1, 1, false), (52, 7, 5, true), (53, 300, 4, false), (54, 255, 3, true), (55, 256, 2, false)] {
            let p = picture(seed, w, h, alpha);
            for to_stdout in [false, true] {
                let file = p.write(None, to_stdout);
                assert_eq!(export_alias(&file).unwrap(), write_alias(&p).unwrap(), "seed {seed}");
            }
        }
    }

    #[test]
    fn export_of_a_cut_picture_repeats_the_row_buffers() {
        let p = picture(61, 6, 4, false);
        let file = p.write(None, true); // raw rows: 18 bytes each after 116 bytes of header
        assert_eq!(file.len(), 116 + 4 * 18);
        // rows 0 and 1 whole, row 2 has red only: its green and blue stay those of row 1,
        // and row 3 is written from the same buffers
        let cut = &file[..116 + 2 * 18 + 6];
        let als = export_alias(cut).unwrap();
        let got = read_alias(&als).unwrap();
        let (r, g) = (p.planes[0].as_ref().unwrap(), p.planes[1].as_ref().unwrap());
        let (gr, gg) = (got.planes[0].as_ref().unwrap(), got.planes[1].as_ref().unwrap());
        assert_eq!(&gr[..18], &r[..18]);
        assert_eq!(&gr[18..], &r[12..18].repeat(1));
        assert_eq!(&gg[12..18], &g[6..12]);
        assert_eq!(&gg[18..], &g[6..12]);
        // no row data at all: the allocator's marks, as far as they can be known
        let als = export_alias(&file[..116]).unwrap();
        let got = read_alias(&als).unwrap();
        assert_eq!(&got.planes[0].as_ref().unwrap()[..6], &[0x6f, 0, 0, 0, 0, 0x6b]);
        // the reader in memory leaves such rows zero
        assert_eq!(Picture::read(&file[..116]).unwrap().picture.planes[0], Some(vec![0; 24]));
    }

    #[test]
    fn export_refusals_and_sizes() {
        let p = picture(71, 5, 3, false);
        let file = p.write(None, false);
        assert_eq!(export_alias(&file[..50]).unwrap_err(), AliasError::Picture(softpic::PicError::CannotRead));
        // a negative height: header only, with the height as it is and height − 1 after it
        let mut f = file.clone();
        f[94..96].copy_from_slice(&(-3i16).to_be_bytes());
        assert_eq!(export_alias(&f).unwrap(), [0, 5, 0xff, 0xfd, 0, 0, 0xff, 0xfc, 0, 24]);
        let mut f = file.clone();
        f[92..94].copy_from_slice(&(-5i16).to_be_bytes());
        assert_eq!(export_alias(&f).unwrap_err(), AliasError::BadSize);
        // a picture file with an alpha packet only has no colour buffers to write from
        let mut f = file.clone();
        f[107] = 0x10;
        assert_eq!(export_alias(&f).unwrap_err(), AliasError::MissingPlane);
    }
}
