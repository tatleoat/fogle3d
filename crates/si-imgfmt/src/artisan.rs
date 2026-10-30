// SPDX-License-Identifier: MIT
//! Artisan pictures (`.art`; finding 0014, "Artisan", sixth pass): the export (picture file to
//! Artisan file, what `soft2art` calls), the import (`art2soft`), the in-memory reader and
//! writer the dialogs use, and their headers.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `artisan.c` (0x085bf850–0x085c1092), all 11.
//!
//! The file: a first header of three big-endian 32-bit words (the magic 0xdeb88bed, the kind:
//! 1 colour, 3 colour with alpha, 2 colour-mapped, and a count of bytes to skip), a second of
//! two (width, height); then for kinds 1 and 3 the planes one after another, each `width ×
//! height` bytes, first row first: red, green, blue, and alpha for kind 3; for kind 2 three
//! colour maps of 256 bytes (red, green, blue) and one index byte a pixel. The names of the
//! header routines are Alias's (`si_AliasHeader1_fread` and so on); they are this file's.

use crate::softpic::{self, PicError, PicIn};
use crate::tiff::Fault;
use si_picture::Picture;

/// The magic of an Artisan file.
pub const MAGIC: u32 = 0xdeb8_8bed;

/// The first header (`si_ArtHeader1`, 12 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header1 {
    pub magic: u32,
    pub kind: i32,
    pub skip: i32,
}

/// [soft 0x085c0e2c si_AliasHeader1_fread] for one header: `None` if fewer than 12 bytes are
/// left (they are consumed all the same).
pub fn header1_fread(data: &[u8], pos: &mut usize) -> Option<Header1> {
    let b = take(data, pos, 12)?;
    let w = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    Some(Header1 { magic: w(0), kind: w(4) as i32, skip: w(8) as i32 })
}

/// [soft 0x085c0eac si_AliasHeader1_fwrite]: the words big-endian (the record is turned for
/// the write and turned back).
pub fn header1_fwrite(h: &Header1, out: &mut Vec<u8>) {
    out.extend_from_slice(&h.magic.to_be_bytes());
    out.extend_from_slice(&h.kind.to_be_bytes());
    out.extend_from_slice(&h.skip.to_be_bytes());
}

/// [soft 0x085c0f8c si_AliasHeader2_fread]: width and height.
pub fn header2_fread(data: &[u8], pos: &mut usize) -> Option<(i32, i32)> {
    let b = take(data, pos, 8)?;
    Some((i32::from_be_bytes([b[0], b[1], b[2], b[3]]), i32::from_be_bytes([b[4], b[5], b[6], b[7]])))
}

/// [soft 0x085c0ff4 si_AliasHeader2_fwrite]
pub fn header2_fwrite(w: i32, h: i32, out: &mut Vec<u8>) {
    out.extend_from_slice(&w.to_be_bytes());
    out.extend_from_slice(&h.to_be_bytes());
}

/// `fread` of one record of `n` bytes: all or nothing in the answer, a partial record
/// consumed.
fn take<'a>(data: &'a [u8], pos: &mut usize, n: usize) -> Option<&'a [u8]> {
    let start = (*pos).min(data.len());
    let end = (start + n).min(data.len());
    *pos = end;
    (end - start == n).then(|| &data[start..end])
}

/// `fread(buf, 1, n, f)`: how many bytes arrived (they are copied).
fn fread_into(data: &[u8], pos: &mut usize, buf: &mut [u8]) -> usize {
    let start = (*pos).min(data.len());
    let n = buf.len().min(data.len() - start);
    buf[..n].copy_from_slice(&data[start..start + n]);
    *pos = start + n;
    n
}

/// `getc`: −1 at the end.
fn getc(data: &[u8], pos: &mut usize) -> i32 {
    match data.get(*pos) {
        Some(&b) => {
            *pos += 1;
            b as i32
        }
        None => -1,
    }
}

/// Why the export writes nothing (the original answers 0 and shows the message).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExportError {
    /// The picture file was refused by `SI_pictureOpenFile` (its own message).
    Picture(PicError),
    /// "%s: The red channel is missing in %s" (program name, input name); likewise green and
    /// blue: no packet names the plane, or the picture is 0 pixels wide.
    MissingChannel(&'static str),
}

impl ExportError {
    pub fn message(&self, program: &str, name: &str) -> String {
        match self {
            ExportError::Picture(e) => e.message(name),
            ExportError::MissingChannel(c) => format!("{program}: The {c} channel is missing in {name}"),
        }
    }
}

/// Converts a picture file to an Artisan file [soft 0x085c0760 SI_pictureExportArtisan].
///
/// The picture file is opened as every reader of it does; a row buffer of `width` bytes
/// (`SI_malloc`, with its two marks) is made **for every plane bit of every packet** (so a
/// plane two packets name counts twice, the later buffer taking its place) and the planes are
/// counted that way. Red, green and blue must exist. Then the headers (kind 3 when there is
/// an alpha buffer, else 1; skip 0; the picture's width and height), and one pass over the
/// rows for each counted plane: red from where the header ended, then green, blue and alpha
/// each after a seek back to that place; the rows go through the scanline reader (whose
/// failure is ignored: a file cut short repeats what the buffers held) and the pass's plane
/// is written, `width` bytes a row. A fifth or later pass (more than four plane bits) seeks
/// nothing and writes the alpha buffer again, from wherever the reading stands.
pub fn export_artisan(pic_file: &[u8]) -> Result<Result<Vec<u8>, ExportError>, Fault> {
    let mut pic = match PicIn::open(pic_file) {
        Ok(p) => p,
        Err(e) => return Ok(Err(ExportError::Picture(e))),
    };
    let mut bufs: [Option<Vec<u8>>; 4] = Default::default();
    let mut planes = 0;
    for p in pic.packets.clone() {
        for (i, bit) in [0x80u8, 0x40, 0x20, 0x10].iter().enumerate() {
            if p.channel & bit != 0 {
                bufs[i] = softpic::si_malloc(pic.width as i32);
                planes += 1;
            }
        }
    }
    for (i, c) in ["red", "green", "blue"].iter().enumerate() {
        if bufs[i].is_none() {
            return Ok(Err(ExportError::MissingChannel(c)));
        }
    }
    let mut out = Vec::new();
    header1_fwrite(&Header1 { magic: MAGIC, kind: if bufs[3].is_some() { 3 } else { 1 }, skip: 0 }, &mut out);
    header2_fwrite(pic.width as i32, pic.height as i32, &mut out);
    let start = pic.tell();
    let mut plane = 0usize;
    for pass in 0..planes {
        if (1..4).contains(&pass) {
            pic.seek(start);
        }
        if pass < 4 {
            plane = pass;
        }
        for _ in 0..pic.height.max(0) {
            pic.scanline(&mut bufs);
            // the pass's buffer is null when a fourth pass comes without an alpha plane (a
            // plane counted twice): fwrite of a null pointer
            let b = bufs[plane].as_ref().ok_or(Fault("a pass over a plane without a buffer: fwrite from a null pointer"))?;
            out.extend_from_slice(b);
        }
    }
    Ok(Ok(out))
}

/// Why the import stops (the original answers 0 and shows the message, level 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// The file does not open (`SI_softOpen`, extension `.art`, its own message).
    CannotOpen,
    /// "Unexpected end of file encountered in the input file %s\n": no first header.
    NoHeader,
    /// "Cannot read input file %s\n": another magic, no second header, or a colour map
    /// short of 256 bytes.
    CannotRead,
    /// The picture file could not be created (`SI_pictureWriteHeader`'s own message).
    CannotCreate,
    /// "Unexpected end of file encountered in %s\n": a row short; the picture file is left
    /// as far as it was written.
    UnexpectedEnd,
}

/// What the import leaves: the picture file as far as it was written (`None`: not created)
/// and how it ended. Bytes 102 and 103 (the header's padding word, from the stack) are
/// `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub file: Option<Vec<Option<u8>>>,
    pub result: Result<(), ImportError>,
}

/// The rows of a file: kinds 1 and 3 read their planes through four streams placed `area`
/// bytes apart; kind 2 through the colour maps.
struct Rows<'a> {
    data: &'a [u8],
    pos: [usize; 4],
    maps: [[u8; 256]; 3],
    kind: i32,
}

/// [soft 0x085c0654 scanlineReadArtisan]: one row into the picture's row buffers. Kinds 1
/// and 3: `width` bytes of red, green, blue (and alpha) from their streams; a short one ends
/// it with 0. Kind 2: a byte a pixel through the three maps (the maps were passed by value;
/// at the end of the file `getc`'s −1 takes the byte in front of each map: the kind word's
/// top byte, 0, for red, red's last entry for green, green's for blue). Another kind: 1,
/// nothing read.
fn scanline_read(r: &mut Rows, width: usize, bufs: &mut [Option<Vec<u8>>; 4]) -> Result<bool, Fault> {
    match r.kind {
        2 => {
            for x in 0..width {
                let i = getc(r.data, &mut r.pos[0]);
                let at = |m: usize| -> u8 {
                    if i >= 0 {
                        r.maps[m][i as usize]
                    } else if m == 0 {
                        (r.kind >> 24) as u8
                    } else {
                        r.maps[m - 1][255]
                    }
                };
                let (cr, cg, cb) = (at(0), at(1), at(2));
                for (k, c) in [cr, cg, cb].into_iter().enumerate() {
                    let b = bufs[k].as_mut().ok_or(Fault("a row buffer of a width of 0: a write through a null pointer"))?;
                    b[x] = c;
                }
            }
            Ok(true)
        }
        1 | 3 => {
            let n = if r.kind == 3 { 4 } else { 3 };
            for k in 0..n {
                let b = bufs[k].as_mut().ok_or(Fault("a row buffer of a width of 0: fread into a null pointer"))?;
                if fread_into(r.data, &mut r.pos[k], &mut b[..width]) != width {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Ok(true),
    }
}

/// Converts an Artisan file to a picture file [soft 0x085bfe38 SI_pictureImportArtisan] with
/// the comment given (`art2soft` passes its own).
///
/// The headers are read (the skip count's bytes passed over with `getc`); the picture is the
/// second header's width and height as 16-bit numbers, ratio 1.0, fields 3, with row buffers
/// of `width` bytes for red, green and blue and, for kind 3, alpha (`SI_malloc`: none for a
/// width of 0). Kind 2 then reads its three colour maps. The picture file's header is written
/// for the planes that have buffers, and every row read (see [`scanline_read`]: for kinds 1
/// and 3 the green, blue and alpha planes are read through the file opened again and placed
/// `width × height` bytes (of the header's 32-bit numbers) further each) and written.
pub fn import_artisan(art: &[u8], comment: &[u8]) -> Result<Imported, Fault> {
    let mut pos = 0usize;
    let Some(h1) = header1_fread(art, &mut pos) else {
        return Ok(Imported { file: None, result: Err(ImportError::NoHeader) });
    };
    if h1.magic != MAGIC {
        return Ok(Imported { file: None, result: Err(ImportError::CannotRead) });
    }
    let mut k = h1.skip;
    while k != 0 {
        getc(art, &mut pos);
        k = k.wrapping_sub(1);
        if k < 0 && k < -(1 << 24) {
            return Err(Fault("a negative skip count: getc about four thousand million times"));
        }
    }
    let Some((w, h)) = header2_fread(art, &mut pos) else {
        return Ok(Imported { file: None, result: Err(ImportError::CannotRead) });
    };
    let area = w.wrapping_mul(h);
    let (width, height) = (w as i16, h as i16);
    let mut bufs: [Option<Vec<u8>>; 4] = [softpic::si_malloc(width as i32), softpic::si_malloc(width as i32), softpic::si_malloc(width as i32), None];
    let mut maps = [[0u8; 256]; 3];
    if h1.kind == 2 {
        for m in maps.iter_mut() {
            if fread_into(art, &mut pos, m) != 256 {
                return Ok(Imported { file: None, result: Err(ImportError::CannotRead) });
            }
        }
    } else if h1.kind == 3 {
        bufs[3] = softpic::si_malloc(width as i32);
    }
    let mut out = Vec::new();
    let has = [bufs[0].is_some(), bufs[1].is_some(), bufs[2].is_some(), bufs[3].is_some()];
    softpic::write_header(&mut out, false, (width, height, 1.0, 3), has, Some(comment));
    let mut file: Vec<Option<u8>> = out.into_iter().map(Some).collect();
    file[102] = None;
    file[103] = None;
    let base = pos as i64;
    let at = |n: i64| (base + n * area as i64).max(0) as usize;
    let mut rows = Rows { data: art, pos: [pos, at(1), at(2), at(3)], maps, kind: h1.kind };
    let wu = width.max(0) as usize;
    for _ in 0..height.max(0) {
        if !scanline_read(&mut rows, wu, &mut bufs)? {
            return Ok(Imported { file: Some(file), result: Err(ImportError::UnexpectedEnd) });
        }
        let mut row = Vec::new();
        softpic::write_row(&mut row, false, [bufs[0].as_deref(), bufs[1].as_deref(), bufs[2].as_deref(), bufs[3].as_deref()]);
        file.extend(row.into_iter().map(Some));
    }
    Ok(Imported { file: Some(file), result: Ok(()) })
}

/// [soft 0x085bf850 pictureReadArtisan]: an Artisan file into a picture in memory (`None`:
/// the original answers 0; the picture may then hold what was set before the failure, as
/// returned). The planes are `width × height` bytes of the header's numbers (`SI_malloc`);
/// kinds 1 and 3 read them whole (alpha for kind 3); kind 2 reads the three maps and an index
/// byte a pixel taken **as a signed byte**: an index of 128 or more reads in front of its map,
/// which for red is the end of the green map and for green the end of the blue map; for blue
/// it is the stack below the maps (a `Fault`). Another kind: the planes are left as allocated
/// and the answer is 1.
pub fn read_artisan(art: &[u8]) -> Result<(bool, Picture), Fault> {
    let mut pic = Picture::default();
    let mut pos = 0usize;
    let Some(h1) = header1_fread(art, &mut pos) else { return Ok((false, pic)) };
    if h1.magic != MAGIC {
        return Ok((false, pic));
    }
    let mut k = h1.skip;
    while k != 0 {
        getc(art, &mut pos);
        k = k.wrapping_sub(1);
        if k < -(1 << 24) {
            return Err(Fault("a negative skip count: getc about four thousand million times"));
        }
    }
    let Some((w, h)) = header2_fread(art, &mut pos) else { return Ok((false, pic)) };
    let n = w.wrapping_mul(h);
    if n > (1 << 26) {
        return Err(Fault("a plane larger than the allocator gives here"));
    }
    pic.width = w as i16;
    pic.height = h as i16;
    pic.ratio = 1.0;
    pic.fields = 3;
    pic.planes[0] = softpic::si_malloc(n);
    pic.planes[1] = softpic::si_malloc(n);
    pic.planes[2] = softpic::si_malloc(n);
    let nu = n.max(0) as usize;
    match h1.kind {
        1 | 3 => {
            for c in 0..3 {
                let p = pic.planes[c].get_or_insert_with(Vec::new);
                if p.len() < nu {
                    return Err(Fault("fread into a null plane"));
                }
                if fread_into(art, &mut pos, &mut p[..nu]) != nu {
                    return Ok((false, pic));
                }
            }
            if h1.kind == 3 {
                let mut a = softpic::si_malloc(n).unwrap_or_default();
                if a.len() < nu {
                    return Err(Fault("fread into a null plane"));
                }
                let got = fread_into(art, &mut pos, &mut a[..nu]);
                pic.planes[3] = Some(a);
                if got != nu {
                    return Ok((false, pic));
                }
            }
            Ok((true, pic))
        }
        2 => {
            let mut maps = [[0u8; 256]; 3];
            for m in maps.iter_mut() {
                if fread_into(art, &mut pos, m) != 256 {
                    return Ok((false, pic));
                }
            }
            for i in 0..nu {
                let c = getc(art, &mut pos) as u8 as i8 as i32;
                let look = |m: usize| -> Result<u8, Fault> {
                    if c >= 0 {
                        Ok(maps[m][c as usize])
                    } else if m < 2 {
                        Ok(maps[m + 1][(256 + c) as usize])
                    } else {
                        Err(Fault("a blue index of 128 or more: the original reads the stack below its maps"))
                    }
                };
                let v = [look(0)?, look(1)?, look(2)?];
                for (ch, x) in v.into_iter().enumerate() {
                    pic.planes[ch].as_mut().ok_or(Fault("a write through a null plane"))?[i] = x;
                }
            }
            Ok((true, pic))
        }
        _ => Ok((true, pic)),
    }
}

/// [soft 0x085bfc30 pictureWriteArtisan]: a picture in memory to an Artisan file (kind 3 with
/// an alpha plane, else 1; the planes whole, `width × height` bytes each). A `Fault` when a
/// colour plane is missing or short (the original's `fwrite` reads through a null pointer or
/// past the plane).
pub fn write_artisan(pic: &Picture) -> Result<Vec<u8>, Fault> {
    let mut out = Vec::new();
    header1_fwrite(&Header1 { magic: MAGIC, kind: if pic.planes[3].is_some() { 3 } else { 1 }, skip: 0 }, &mut out);
    header2_fwrite(pic.width as i32, pic.height as i32, &mut out);
    let n = (pic.height as i32 * pic.width as i32).max(0) as usize;
    for c in 0..4 {
        match &pic.planes[c] {
            Some(p) => out.extend_from_slice(p.get(..n).ok_or(Fault("a plane shorter than the picture: fwrite reads past it"))?),
            None if c == 3 => {}
            None if n == 0 => {}
            None => return Err(Fault("a colour plane missing: fwrite from a null pointer")),
        }
    }
    Ok(out)
}

/// [soft 0x085c0df4 readArtPicture]: [`read_artisan`]'s answer.
pub fn read_art_picture(art: &[u8]) -> Result<(bool, Picture), Fault> {
    read_artisan(art)
}

/// [soft 0x085c0e10 writeArtPicture]: [`write_artisan`]'s answer.
pub fn write_art_picture(pic: &Picture) -> Result<Vec<u8>, Fault> {
    write_artisan(pic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_then_import_and_read_give_the_picture_back() {
        let p = crate::alias::testpic::picture(5, 37, 9, true);
        let file = p.write(Some(b"t"), false);
        let art = export_artisan(&file).unwrap().unwrap();
        assert_eq!(&art[..8], &[0xde, 0xb8, 0x8b, 0xed, 0, 0, 0, 3]);
        assert_eq!(art.len(), 20 + 4 * 37 * 9);
        let back = import_artisan(&art, b"t").unwrap();
        assert_eq!(back.result, Ok(()));
        let bytes: Vec<u8> = back.file.unwrap().iter().map(|b| b.unwrap_or(0)).collect();
        let q = Picture::read(&bytes).unwrap().picture;
        assert_eq!(q.planes, p.planes);
        let (ok, r) = read_artisan(&art).unwrap();
        assert!(ok);
        assert_eq!(r.planes, p.planes);
        assert_eq!(write_artisan(&r).unwrap(), art);
    }

    #[test]
    fn a_short_file_stops_the_import() {
        let p = crate::alias::testpic::picture(2, 8, 4, false);
        let art = export_artisan(&p.write(Some(b"t"), false)).unwrap().unwrap();
        let back = import_artisan(&art[..art.len() - 3], b"t").unwrap();
        assert_eq!(back.result, Err(ImportError::UnexpectedEnd));
        assert_eq!(import_artisan(&art[..10], b"t").unwrap().result, Err(ImportError::NoHeader));
    }
}
