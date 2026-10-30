// SPDX-License-Identifier: MIT
//! Everest pictures (finding 0014, "Everest"): the export and the import.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `everest.c` (0x085c1b30–0x085c304d). Ported: the
//! export (picture file to Everest file, what soft2everest calls), the import (everest2soft)
//! and what they use. Also ported: the in-memory pair the dialogs use, `SI_pictureReadEverest` (0x085c1b68),
//! `SI_pictureWriteEverest` (0x085c1f44) and their wrappers `readEverestPicture` (0x085c1b30)
//! and `writeEverestPicture` (0x085c1b4c); checked in the emulator, the writers on every word
//! the original defines (their 992 header bytes are stack leftovers).
//!
//! The file: a header of 1,024 bytes of which the first 32 are sixteen 16-bit words and the
//! rest what the writer's stack held (undefined here), then the rows in the picture file's
//! order, each as runs of one colour: a 32-bit big-endian word `count << 24 | red << 16 |
//! green << 8 | blue`, a count of 1 to 255. No alpha.

use crate::softpic;
use crate::tiff::{malloc, Blk, Fault};
use si_picture::Picture;

/// The header's sixteen words as the export fills them (machine order before the writer
/// turns them): 0x4949, 4, 0x200, 0, 0x100, 0, 0, the width and the height **with their bytes
/// exchanged** (so they come out little-endian), 0, 0, 0xffff, 0xffff, 0, 0, 0.
fn header_words(width: i16, height: i16) -> [u16; 16] {
    let sw = |v: i16| (v as u16).swap_bytes();
    [0x4949, 4, 0x200, 0, 0x100, 0, 0, sw(width), sw(height), 0, 0, 0xffff, 0xffff, 0, 0, 0]
}

/// The header to a file [soft 0x085c2cac si_EverestHeader_fwrite]: the record's sixteen
/// words turned to big-endian, the whole 1,024-byte record written, the words turned back.
/// Only the 32 bytes of words are known here; the other 992 are the caller's (`None`).
fn header_fwrite(words: &[u16; 16], out: &mut Vec<Option<u8>>) {
    for w in words {
        out.extend(w.to_be_bytes().map(Some));
    }
    out.extend(std::iter::repeat(None).take(0x400 - 32));
}

/// One row as runs of one colour, a 32-bit big-endian word `count << 24 | red << 16 |
/// green << 8 | blue` each: a run ends where the colour changes or at 255 pixels. The export and
/// the in-memory writer have this loop in common; a width of 0 writes nothing.
fn write_runs(out: &mut Vec<Option<u8>>, row: [&[u8]; 3]) {
    let word = |out: &mut Vec<Option<u8>>, v: u32| out.extend(v.to_be_bytes().map(Some));
    let mut count = 0u32;
    let mut cur = 0u32;
    for x in 0..row[0].len() {
        let rgb = (row[0][x] as u32) << 16 | (row[1][x] as u32) << 8 | row[2][x] as u32;
        let prev = count;
        count += 1;
        if count == 1 {
            cur = rgb;
        } else {
            if rgb != cur {
                word(out, cur | prev << 24);
                count = 1;
                cur = rgb;
            }
            if count == 0xff {
                cur |= 0xff << 24;
                word(out, cur);
                count = 0;
            }
        }
    }
    if count != 0 {
        word(out, cur | count << 24);
    }
}

/// Converts a SOFTIMAGE picture file to an Everest file row by row
/// [soft 0x085c266c SI_pictureExportEverest]. Returns the bytes of the file, `None` for the
/// header's 992 bytes of stack.
///
/// The picture file is opened as every reader of it does (`SI_pictureOpenFile`), a row buffer
/// of `width` bytes is made for each plane its packets name, and each row read by the
/// scanline reader (whose result is ignored: a file cut short repeats what the buffers hold,
/// as in the Alias export) is written as runs: a run ends where the colour changes or at 255
/// pixels. A width of 0 writes nothing for a row.
pub fn export_everest(pic_file: &[u8]) -> Result<Result<Vec<Option<u8>>, softpic::PicError>, Fault> {
    let mut pic = match softpic::PicIn::open(pic_file) {
        Ok(p) => p,
        Err(e) => return Ok(Err(e)),
    };
    let mut out = Vec::new();
    header_fwrite(&header_words(pic.width, pic.height), &mut out);
    if pic.height < 0 {
        return Err(Fault("a negative height: the row loop counts down past zero"));
    }
    if pic.width < 0 {
        return Err(Fault("a negative width: the pixel loop counts down past zero"));
    }
    let mut bufs = pic.row_buffers();
    let w = pic.width as usize;
    for _ in 0..pic.height {
        pic.scanline(&mut bufs);
        let [Some(r), Some(g), Some(b), _] = &bufs else { return Err(Fault("a colour plane the export reads has no row buffer")) };
        write_runs(&mut out, [&r[..w], &g[..w], &b[..w]]);
    }
    Ok(Ok(out))
}

/// The header from a file [soft 0x085c2ac8 si_EverestHeader_fread]: `fread` of the 1,024-byte
/// record, then its sixteen words turned from big-endian whatever was read. Answers the
/// words, or `None` when the file is shorter than the record (`fread` answers 0 and the
/// import stops on that count before it looks at the words).
fn header_fread(file: &[u8]) -> Option<[u16; 16]> {
    if file.len() < 0x400 {
        return None;
    }
    let mut w = [0u16; 16];
    for (k, v) in w.iter_mut().enumerate() {
        *v = u16::from_be_bytes([file[2 * k], file[2 * k + 1]]);
    }
    Some(w)
}

/// A `FILE *` read byte by byte with `fgetc` (−1 at the end).
struct Getc<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Getc<'_> {
    fn getc(&mut self) -> i32 {
        match self.data.get(self.pos) {
            Some(&b) => {
                self.pos += 1;
                b as i32
            }
            None => -1,
        }
    }
}

/// One row of an Everest file into the three row buffers [soft 0x085c24f0 scanlineReadEverest].
/// Kind 1: three bytes a pixel. Kind 2: runs, a count byte and red, green, blue; a count of 0
/// fills the rest of the row with the colour last decoded **in this call** (stack bytes
/// before the first run: undefined, `None`) after skipping three bytes; a run longer than the
/// rest of the row is cut there (the remainder is dropped, not carried into the next row).
/// Any other kind leaves the buffers as they are. At the end of the file a count of −1 makes
/// the row's length grow at every step: the routine never ends (a fault here).
fn scanline(f: &mut Getc, width: i16, kind: i16, bufs: &mut [Vec<Option<u8>>; 3]) -> Result<(), Fault> {
    let w = width as i32;
    match kind {
        1 => {
            for x in 0..w.max(0) as usize {
                for b in bufs.iter_mut() {
                    let v = Some(f.getc() as u8);
                    *b.get_mut(x).ok_or(Fault("a write outside a heap block"))? = v;
                }
            }
        }
        2 => {
            let mut left = w;
            let mut x = 0usize;
            let mut colour: [Option<u8>; 3] = [None; 3];
            while left != 0 {
                let mut count = f.getc();
                if count == 0 {
                    for _ in 0..3 {
                        f.getc();
                    }
                    count = left;
                } else {
                    for c in colour.iter_mut() {
                        *c = Some(f.getc() as u8);
                    }
                }
                if count > left {
                    count = left;
                }
                if count < 0 {
                    return Err(Fault("a count of -1 at the end of the file: the row never ends"));
                }
                left -= count;
                for _ in 0..count {
                    for (b, c) in bufs.iter_mut().zip(colour) {
                        *b.get_mut(x).ok_or(Fault("a write outside a heap block"))? = c;
                    }
                    x += 1;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// What [`import_everest`] answers.
#[derive(Clone, Debug, PartialEq)]
pub enum Imported {
    /// The picture file written (`None`: the padding word, stale stack).
    File(Vec<Option<u8>>),
    /// The header could not be read: "%s: Cannot read file %s" (level 3), 0.
    CannotRead,
}

/// Converts an Everest file to a SOFTIMAGE picture file row by row
/// [soft 0x085c21f4 SI_pictureImportEverest]. `comment` is what the caller passes for the
/// picture file's comment.
///
/// The header gives width and height (stored little-endian), the row coding (kind, word 2,
/// little-endian) and whether a table of row offsets is to be made (word 3 equal to 1: the
/// rows are then read once to note where each starts, and again from there, which reads the
/// same). The picture file has ratio 1.0, `fields` 3, three planes, and is written with
/// `SI_pictureWriteHeader` and a mixed row a row; the row buffers are not cleared between
/// rows. A pixel that depends on bytes nobody set, a width of 0 or less, and a row that does
/// not end are faults here.
pub fn import_everest(file: &[u8], comment: &[u8]) -> Result<Imported, Fault> {
    let Some(h) = header_fread(file) else { return Ok(Imported::CannotRead) };
    let le = |v: u16| v.swap_bytes() as i16;
    let (width, height, kind, table) = (le(h[7]), le(h[8]), le(h[2]), le(h[3]));
    if width <= 0 {
        return Err(Fault("a width of 0 or less (row buffers of SI_malloc(0) and below)"));
    }
    let mk = || {
        let mut v = vec![None; width as usize];
        v[0] = Some(0x6f);
        v[width as usize - 1] = Some(0x6b);
        v
    };
    let mut bufs = [mk(), mk(), mk()];
    let mut f = Getc { data: file, pos: 0x400 };
    let mut starts = Vec::new();
    if table == 1 {
        for _ in 0..height.max(0) {
            starts.push(f.pos);
            scanline(&mut f, width, kind, &mut bufs)?;
        }
    }
    let mut out = Vec::new();
    crate::softpic::write_header(&mut out, false, (width, height, 1.0, 3), [true, true, true, false], Some(comment));
    let mut file_out: Vec<Option<u8>> = out.into_iter().map(Some).collect();
    file_out[102] = None;
    file_out[103] = None;
    for r in 0..height.max(0) as usize {
        if table == 1 {
            f.pos = starts[r];
        }
        scanline(&mut f, width, kind, &mut bufs)?;
        let rows: Vec<Vec<u8>> = bufs
            .iter()
            .map(|b| b.iter().map(|v| v.ok_or(Fault("a pixel nobody set goes into the run-length coder"))).collect())
            .collect::<Result<_, _>>()?;
        let mut row = Vec::new();
        crate::softpic::write_row(&mut row, false, [Some(&rows[0]), Some(&rows[1]), Some(&rows[2]), None]);
        file_out.extend(row.into_iter().map(Some));
    }
    Ok(Imported::File(file_out))
}

/// The `SI_Picture` record as [`read_everest`] leaves it. The reader sets width, height,
/// ratio (1.0), `fields` (3) and the three colour planes and clears the alpha pointer; the
/// word after `fields` (`pad`) is the caller's and stays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EverestPicture {
    pub width: i16,
    pub height: i16,
    pub ratio: f32,
    pub fields: i16,
    /// Red, green, blue (`None` bytes: stack bytes or allocator contents nobody wrote).
    pub planes: [Blk; 3],
}

/// What [`read_everest`] leaves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadEverest {
    /// The answer: 1 read, 0 refused (or not opened).
    pub ret: i32,
    /// The record, once the header has been read (untouched before).
    pub picture: Option<EverestPicture>,
    /// `SI_statusDisplayMessage(text, level)`, in order.
    pub messages: Vec<(Vec<u8>, i16)>,
}

/// Reads an Everest file into a picture in memory [soft 0x085c1b68 SI_pictureReadEverest].
/// `file` is what `SI_softOpen` opened for the name and the extension `.ima` (`None`: nothing,
/// the answer is 0 and the record is untouched); `program` and `path` go into the message
/// "%s: Cannot read %s" (level 3), shown when the file is shorter than the 1,024-byte header.
///
/// - Width and height are the header's words 7 and 8, **little-endian** (`i16`; the
///   allocation takes `width × height` as the product of two signed 16-bit numbers: that many
///   bytes a plane, three planes), ratio 1.0, `fields` 3, alpha pointer 0.
/// - Word 2 (little-endian) is the coding: 1 three bytes a pixel, 2 runs, any other value
///   reads nothing and leaves the planes as the allocator made them. Word 3 equal to 1 puts
///   the rows in the plane **last row first** (the first row of the file goes to the last
///   row of the plane).
/// - Runs: a count byte, then red, green, blue. A count of 0 skips three more bytes and fills
///   the rest of the row with the colour last decoded (**in any earlier row, too**: the colour
///   is the function's, and before the first run it is stack bytes, `None` here). A run longer
///   than the rest of its row is cut there and **the remainder carries into the next row**
///   (the import's reader drops it). At the end of the file `fgetc` answers −1 for every
///   byte: a plane byte of 0xff, and a count of −1 which makes the row grow at each step (the
///   original loops some two thousand million times; a [`Fault`] here).
/// - Faults (what the original would do is a crash or a four-thousand-million-fold loop): a
///   negative width or height where a loop counts down to zero, a request for more than the
///   port's allocation limit, a write outside a plane.
pub fn read_everest(file: Option<&[u8]>, program: &[u8], path: &[u8]) -> Result<ReadEverest, Fault> {
    let mut out = ReadEverest::default();
    let Some(data) = file else { return Ok(out) };
    let Some(h) = header_fread(data) else {
        let mut m = program.to_vec();
        m.extend_from_slice(b": Cannot read ");
        m.extend_from_slice(path);
        out.messages.push((m, 3));
        return Ok(out);
    };
    let le = |v: u16| v.swap_bytes();
    let width = le(h[7]) as i16;
    let height = le(h[8]) as i16;
    let (kind, flip) = (le(h[2]) as i16, le(h[3]) as i16);
    // the size is the product of the two signed 16-bit numbers (`cwde` on the height)
    let size = (width as i32).wrapping_mul(height as i32);
    let mut planes = [malloc(size as u32)?, malloc(size as u32)?, malloc(size as u32)?];
    // the pointers advance one pixel at a time; with the flip they start at the last row and
    // step back two rows' worth after each row
    let mut at = if flip == 1 { size.wrapping_sub(width as i32) as i64 } else { 0 };
    let back = if flip == 1 { width as i64 * -2 } else { 0 };
    let mut f = Getc { data, pos: 0x400 };
    if kind == 1 || kind == 2 {
        if height < 0 || (width < 0 && height > 0) {
            return Err(Fault("a negative width or height: the loops count down past zero"));
        }
        let put = |planes: &mut [Blk; 3], at: i64, v: [Option<u8>; 3]| -> Result<(), Fault> {
            for (p, v) in planes.iter_mut().zip(v) {
                let slot = usize::try_from(at).ok().and_then(|i| p.as_mut()?.get_mut(i)).ok_or(Fault("a write outside a heap block"))?;
                *slot = v;
            }
            Ok(())
        };
        let mut carry = 0i32; // the rest of a run cut at the end of a row
        let mut colour: [Option<u8>; 3] = [None; 3]; // stack bytes before the first run
        for _ in 0..height {
            if kind == 1 {
                for _ in 0..width {
                    let px = [f.getc() as u8, f.getc() as u8, f.getc() as u8].map(Some);
                    put(&mut planes, at, px)?;
                    at += 1;
                }
            } else {
                let mut left = width as i32;
                while left != 0 {
                    let mut count;
                    if carry == 0 {
                        count = f.getc();
                        if count == 0 {
                            for _ in 0..3 {
                                f.getc();
                            }
                            count = left;
                        } else {
                            colour = [f.getc() as u8, f.getc() as u8, f.getc() as u8].map(Some);
                        }
                    } else {
                        count = carry;
                        carry = 0;
                    }
                    if count < 0 {
                        return Err(Fault("a count of -1 at the end of the file: the row never ends"));
                    }
                    if left < count {
                        carry = count - left;
                        count = left;
                    }
                    left -= count;
                    for _ in 0..count {
                        put(&mut planes, at, colour)?;
                        at += 1;
                    }
                }
            }
            at += back;
        }
    }
    out.picture = Some(EverestPicture { width, height, ratio: 1.0, fields: 3, planes });
    out.ret = 1;
    Ok(out)
}

/// The wrapper the picture-format table holds for the reader
/// [soft 0x085c1b30 readEverestPicture]: `SI_pictureReadEverest`'s answer as a 16-bit number
/// widened to `int`.
pub fn read_everest_picture(file: Option<&[u8]>, program: &[u8], path: &[u8]) -> Result<ReadEverest, Fault> {
    let mut r = read_everest(file, program, path)?;
    r.ret = r.ret as i16 as i32;
    Ok(r)
}

/// Writes a picture in memory as an Everest file [soft 0x085c1f44 SI_pictureWriteEverest]
/// (the file `SI_softOpen` made for the extension `.ima` is the caller's; a name that cannot
/// be opened answers 0 before this). The header is the export's (the 1,024-byte record, its
/// other 992 bytes the stack's: `None`), then the rows from the first, each as runs
/// ([`write_runs`]), read straight from the three colour planes, which the original does not
/// test: a missing or short plane, and a negative width or height (the loops count down past
/// zero), are faults here. Answers 1; a failed header write (a full disk) is not modelled.
pub fn write_everest(pic: &Picture) -> Result<Vec<Option<u8>>, Fault> {
    let mut out = Vec::new();
    header_fwrite(&header_words(pic.width, pic.height), &mut out);
    if pic.height < 0 || (pic.width < 0 && pic.height > 0) {
        return Err(Fault("a negative width or height: the loops count down past zero"));
    }
    if pic.width <= 0 || pic.height == 0 {
        return Ok(out);
    }
    let (w, h) = (pic.width as usize, pic.height as usize);
    let [Some(r), Some(g), Some(b), _] = &pic.planes else { return Err(Fault("a colour plane the writer reads is null")) };
    if r.len() < w * h || g.len() < w * h || b.len() < w * h {
        return Err(Fault("a plane shorter than the picture"));
    }
    for y in 0..h {
        let a = y * w..(y + 1) * w;
        write_runs(&mut out, [&r[a.clone()], &g[a.clone()], &b[a]]);
    }
    Ok(out)
}

/// The wrapper the picture-format table holds for the writer
/// [soft 0x085c1b4c writeEverestPicture]: `SI_pictureWriteEverest`'s answer widened from 16
/// bits (always 1 here).
pub fn write_everest_picture(pic: &Picture) -> Result<(i32, Vec<Option<u8>>), Fault> {
    Ok((1i16 as i32, write_everest(pic)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_then_import_gives_the_picture_back() {
        let p = crate::alias::testpic::picture(7, 300, 3, false);
        let file = p.write(Some(b"t"), false);
        let ev = export_everest(&file).unwrap().unwrap();
        // 1,024 header bytes, then runs: no run is longer than 255
        let bytes: Vec<u8> = ev.iter().map(|b| b.unwrap_or(0)).collect();
        assert_eq!(&bytes[..4], &[0x49, 0x49, 0, 4]);
        assert_eq!(&bytes[14..18], &[44, 1, 3, 0]);
        let Imported::File(back) = import_everest(&bytes, b"t").unwrap() else { panic!() };
        let back: Vec<u8> = back.iter().map(|b| b.unwrap_or(0)).collect();
        let q = Picture::read(&back).unwrap().picture;
        assert_eq!(q.planes[..3], p.planes[..3]);
    }
}
