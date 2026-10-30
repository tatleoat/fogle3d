// SPDX-License-Identifier: MIT
//! Wavefront pictures (`.rla`, `.rlb`; finding 0014, "Wavefront", sixth pass): the reader
//! (`wave2soft`), the writer (`soft2wave`), the run-length coding of a channel, and the two
//! headers.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `wavefront.c` (0x085e4520–0x085e5941), all 15.
//!
//! The file: a header of 740 bytes (big-endian; for `.rlb` a few more fields are set), a table
//! of 32-bit offsets, one a row of the active area, **bottom row first**, then each row's
//! channels (red, green, blue, and the matte when there is one), each a 16-bit byte count and
//! run-length data: a count byte `c` of 0 or more repeats the next byte `c + 1` times; a
//! negative one copies the next `−c` bytes.
//!
//! The writer's header is a record on its stack of which only some fields are set: the
//! others are whatever the stack held (`None` here). What the clock and the environment
//! answer (the date, `getlogin`) are arguments.

use crate::tiff::Fault;
use si_picture::Picture;

/// The header's size.
pub const HEADER: usize = 0x2e4;

/// Which of the two files: `.rla` (1) or `.rlb` (2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rla = 1,
    Rlb = 2,
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// The header's 16-bit fields that the byte-order routines turn (14 words from 0) and the
/// 32-bit one at 0x8c; for `.rlb` also the words at 0x264, 0x266, 0x278 … 0x27e and the
/// 32-bit numbers at 0x268 … 0x274. [soft 0x085e56a8 swaprlaHeader]
/// [soft 0x085e5804 swaprlbHeader]
fn swapped_fields(kind: Kind) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = (0..14).map(|i| (2 * i, 2)).collect();
    v.push((0x8c, 4));
    if kind == Kind::Rlb {
        v.extend([(0x264, 2), (0x266, 2), (0x268, 4), (0x26c, 4), (0x270, 4), (0x274, 4), (0x278, 2), (0x27a, 2), (0x27c, 2), (0x27e, 2)]);
    }
    v
}

/// [soft 0x085e57b8 si_rlaHeader_fwrite] and [soft 0x085e58f8 si_rlbHeader_fwrite]: the
/// record (in this machine's order) with its numeric fields turned, as written. `rec` holds
/// the record's bytes, `None` where unset.
pub fn header_fwrite(rec: &[Option<u8>], kind: Kind, out: &mut Vec<Option<u8>>) {
    let mut b = rec.to_vec();
    for (at, n) in swapped_fields(kind) {
        b[at..at + n].reverse();
    }
    out.extend_from_slice(&b);
}

/// [soft 0x085e5784 si_rlaHeader_fread] and [soft 0x085e58c4 si_rlbHeader_fread]: 740 bytes
/// (a short read leaves the rest as the record held: `None`), the numeric fields turned.
/// The answer of `fread` is not looked at by the reader.
pub fn header_fread(data: &[u8], kind: Kind) -> Vec<Option<u8>> {
    let mut b: Vec<Option<u8>> = (0..HEADER).map(|i| data.get(i).copied()).collect();
    if data.len() < HEADER {
        // fread of one record of 740 bytes: a partial record's bytes are copied all the same
    }
    for (at, n) in swapped_fields(kind) {
        b[at..at + n].reverse();
    }
    b
}

fn put_str(rec: &mut [Option<u8>], at: usize, s: &[u8]) -> Result<(), Fault> {
    if at + s.len() + 1 > rec.len() {
        return Err(Fault("a text copied past the header record: the original overruns its stack"));
    }
    for (i, &c) in s.iter().enumerate() {
        rec[at + i] = Some(c);
    }
    rec[at + s.len()] = Some(0);
    Ok(())
}

fn put_le(rec: &mut [Option<u8>], at: usize, v: &[u8]) {
    for (i, &c) in v.iter().enumerate() {
        rec[at + i] = Some(c);
    }
}

/// The plane's byte at `i` (the coder reads up to one byte past the active window; past the
/// plane's end that is heap memory nobody set: `past`, if the caller knows it).
fn byte(plane: &[u8], i: i64, past: Option<u8>) -> Result<u8, Fault> {
    if i < 0 {
        return Err(Fault("a pixel in front of the plane"));
    }
    match plane.get(i as usize) {
        Some(&b) => Ok(b),
        None if i as usize == plane.len() => past.ok_or(Fault("the run coder compares a byte past the plane's end (heap memory nobody set)")),
        None => Err(Fault("the run coder reads well past the plane's end")),
    }
}

/// [soft 0x085e5360 channelWriteWavefront]: `count + 1` bytes of the plane from `at`, coded:
/// a placeholder for the length, then literal stretches (count byte −n, n bytes; at most 128)
/// and runs (count byte n, one byte: n + 1 repeats; at most 128), a run being started by
/// three equal bytes (two at the end), then the length patched in (16 bits, the bytes after
/// it). The comparisons read up to two bytes beyond the one in hand: at the end of the window
/// that is the byte after it (the next pixel, the first of the next row, or past the plane).
///
/// `past` is the heap byte after the plane, which the comparisons reach on the plane's last
/// row when the window ends at the picture's right edge (`None`: unknown; a `Fault` if it
/// decides the coding).
pub fn channel_write(plane: &[u8], at: i64, count: i32, past: Option<u8>, out: &mut Vec<Option<u8>>) -> Result<(), Fault> {
    let p = |k: i64| byte(plane, at + k, past);
    let len_at = out.len();
    out.extend([Some(0), Some(0)]);
    let mut len: i16 = 0;
    let mut step: i64 = 2;
    let mut c: i8 = 0;
    let mut i: i64 = 0;
    let mut ptr: i64 = 0;
    let n = count as i64;
    let write = |out: &mut Vec<Option<u8>>, c: i8, from: i64, k: i64| -> Result<(), Fault> {
        out.push(Some(c as u8));
        for j in 0..k {
            out.push(Some(p(from + j)?));
        }
        Ok(())
    };
    if n > 0 {
        loop {
            let mut u = p(i)?;
            while u != p(i + 1)? || u != p(step + i)? {
                c = c.wrapping_sub(1);
                if c == -128 {
                    let k = -(c as i64);
                    write(out, c, ptr, k)?;
                    ptr += k;
                    len = len.wrapping_add(1).wrapping_sub(c as i16);
                    c = 0;
                }
                i += 1;
                if i == n - 1 {
                    step = 1;
                } else if i == n {
                    c = c.wrapping_sub(1);
                    let k = -(c as i64);
                    write(out, c, ptr, k)?;
                    ptr += k;
                    len = len.wrapping_add(1).wrapping_sub(c as i16);
                    c = 0;
                }
                if n <= i {
                    break;
                }
                u = p(i)?;
            }
            if c != 0 {
                let k = -(c as i64);
                write(out, c, ptr, k)?;
                ptr += k;
                len = len.wrapping_add(1).wrapping_sub(c as i16);
                c = 0;
            }
            if i < n && p(i)? == p(i + 1)? {
                let mut j = i;
                loop {
                    let full = c == 126;
                    c = c.wrapping_add(1);
                    i = j + 1;
                    if full {
                        write(out, c, ptr, 1)?;
                        ptr += c as i64 + 1;
                        len = len.wrapping_add(2);
                        c = 0;
                        i = j + 2;
                    }
                    if i == n {
                        let adv;
                        if c == 0 {
                            c = -1;
                            write(out, c, ptr, 1)?;
                            adv = -(c as i64);
                            len = len.wrapping_add(1).wrapping_sub(c as i16);
                        } else {
                            write(out, c, ptr, 1)?;
                            adv = c as i64 + 1;
                            len = len.wrapping_add(2);
                        }
                        ptr += adv;
                        c = 0;
                    }
                    if !(i < n) {
                        break;
                    }
                    j = i;
                    if p(i)? != p(i + 1)? {
                        break;
                    }
                }
            }
            if c != 0 {
                write(out, c, ptr, 1)?;
                ptr += c as i64 + 1;
                len = len.wrapping_add(2);
                c = 0;
                i += 1;
                if i == n {
                    c = -1;
                    write(out, c, ptr, 1)?;
                    ptr += -(c as i64);
                    len = len.wrapping_add(1).wrapping_sub(c as i16);
                    c = 0;
                }
            }
            if !(i < n) {
                break;
            }
        }
    }
    let _ = (c, ptr);
    let b = len.to_be_bytes();
    out[len_at] = Some(b[0]);
    out[len_at + 1] = Some(b[1]);
    Ok(())
}

/// What the clock and the environment answer the writer: `localtime`'s month (0 … 11), day
/// and year, and `getlogin`'s name (`None`: a null pointer, which `sprintf` prints
/// `(null)`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Environment {
    pub month: usize,
    pub day: i32,
    pub year: i32,
    pub login: Option<Vec<u8>>,
}

/// The active window `-w` asks for (in the file's coordinates: x from the left, y from the
/// bottom): left, right, bottom, top.
fn active_window(pic: &Picture) -> Result<(i32, i32, i32, i32), Fault> {
    let (w, h) = (pic.width as i32, pic.height as i32);
    let planes: Vec<&Vec<u8>> = (0..3).map(|c| pic.planes[c].as_ref().ok_or(Fault("a colour plane missing: the window scan reads a null pointer"))).collect::<Result<_, _>>()?;
    let lit = |x: i32, y: i32| -> Result<bool, Fault> {
        let i = (x + y * w) as usize;
        let mut any = false;
        for p in &planes {
            if *p.get(i).ok_or(Fault("a plane shorter than the picture"))? != 0 {
                any = true;
            }
        }
        Ok(any)
    };
    // the topmost row (in memory, from row 1 down) with a pixel not black, else 0
    let mut top_row = 0;
    let mut r = h - 1;
    while 0 < r {
        for x in 0..w {
            if lit(x, r)? {
                top_row = r;
                break;
            }
        }
        r -= 1;
    }
    // the lowest such row, else the last
    let mut bottom_row = h - 1;
    for r in 0..h {
        for x in 0..w {
            if lit(x, r)? {
                bottom_row = r;
                break;
            }
        }
    }
    let (mut left, mut right) = (w, 0);
    let mut r = top_row;
    while r <= bottom_row {
        for x in 0..w {
            if lit(x, r)? {
                left = left.min(x);
                right = right.max(x);
            }
        }
        r += 1;
    }
    let mut top = h - top_row + 10;
    if h <= top {
        top = h - 1;
    }
    let bottom = (h - bottom_row - 10).max(0);
    let left = (left - 10).max(0);
    let mut right = right + 10;
    if w <= right {
        right = w - 1;
    }
    Ok((left, right, bottom, top))
}

/// Writes a picture as a Wavefront file [soft 0x085e4ab8 SI_pictureWriteWavefront] (`name`
/// as given, which the header keeps; the original opens it with `SI_softOpen` and the
/// extension `.rla` or `.rlb`, mode `wb`). `active`: only the window around the pixels that
/// are not black, ten pixels of margin (see the code for its exact bounds), instead of the
/// whole picture. A kind other than 1 and 2 writes nothing (`Ok(None)`).
///
/// The header: window (0, width − 1, 0, height − 1), active window, frame 1, storage 0 (bytes),
/// three channels, one matte when the picture has alpha, gamma `2.2`, the chromaticities
/// 0.67/0.33, 0.21/0.71, 0.14/0.08 and 0.31/0.316 printed `%7.4f %7.4f`, job 0, the name, the
/// description `Image converted from a Softimage format picture.`, program `Softimage v3.0`,
/// machine `Iris Workstation`, the login name, the date (`October 14, 2026`), aspect `user
/// defined`, channels `rgb`; for `.rlb` also the ratio printed `%3.2f` at 0x23c and zeros in
/// the fields from 0x264 to 0x27f. Every other header byte is the stack's (`None`). Then the
/// offset table (written first as whatever lies on the stack behind a zero word, then
/// patched) and the rows of the active window, bottom row first, each channel through
/// [`channel_write`] with the window's width less one as the count.
///
/// `past`: the heap byte after each plane (red, green, blue, alpha), which the coder may
/// compare on the last row (see [`channel_write`]); `None` where unknown.
pub fn write_wavefront(pic: &Picture, name: &[u8], active: bool, kind: i32, env: &Environment) -> Result<Option<Vec<Option<u8>>>, Fault> {
    write_wavefront_past(pic, name, active, kind, env, [None; 4])
}

/// [`write_wavefront`] with the bytes after the planes given.
pub fn write_wavefront_past(pic: &Picture, name: &[u8], active: bool, kind: i32, env: &Environment, past: [Option<u8>; 4]) -> Result<Option<Vec<Option<u8>>>, Fault> {
    let kind = match kind {
        1 => Kind::Rla,
        2 => Kind::Rlb,
        _ => return Ok(None),
    };
    let (w, h) = (pic.width as i32, pic.height as i32);
    let (mut left, mut right, mut bottom, mut top) = (0, w - 1, 0, h - 1);
    if active {
        (left, right, bottom, top) = active_window(pic)?;
    }
    let mut rec: Vec<Option<u8>> = vec![None; HEADER];
    let words: [i16; 14] = [0, (w - 1) as i16, 0, (h - 1) as i16, left as i16, right as i16, bottom as i16, top as i16, 1, 0, 3, pic.planes[3].is_some() as i16, 0, 0];
    for (k, v) in words.iter().enumerate() {
        put_le(&mut rec, 2 * k, &v.to_le_bytes());
    }
    put_str(&mut rec, 0x1c, b"2.2")?;
    for (at, (a, b)) in [(0x2c, (0.67f64, 0.33f64)), (0x44, (0.21, 0.71)), (0x5c, (0.14, 0.08)), (0x74, (0.31, 0.316))] {
        put_str(&mut rec, at, format!("{a:7.4} {b:7.4}").as_bytes())?;
    }
    put_le(&mut rec, 0x8c, &0u32.to_le_bytes());
    let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
    put_str(&mut rec, 0x90, name)?;
    put_str(&mut rec, 0x110, b"Image converted from a Softimage format picture.")?;
    put_str(&mut rec, 0x190, b"Softimage v3.0")?;
    put_str(&mut rec, 0x1d0, b"Iris Workstation")?;
    put_str(&mut rec, 0x1f0, env.login.as_deref().unwrap_or(b"(null)"))?;
    let month = MONTHS.get(env.month).ok_or(Fault("a month outside the table"))?;
    put_str(&mut rec, 0x210, format!("{month} {}, {}", env.day, env.year).as_bytes())?;
    put_str(&mut rec, 0x224, b"user defined")?;
    put_str(&mut rec, 0x244, b"rgb")?;
    let mut out = Vec::new();
    match kind {
        Kind::Rla => header_fwrite(&rec, kind, &mut out),
        Kind::Rlb => {
            put_str(&mut rec, 0x23c, format!("{:3.2}", pic.ratio as f64).as_bytes())?;
            for at in [0x264usize, 0x266, 0x278, 0x27a, 0x27c, 0x27e] {
                put_le(&mut rec, at, &[0, 0]);
            }
            for at in [0x268usize, 0x26c, 0x270, 0x274] {
                put_le(&mut rec, at, &[0, 0, 0, 0]);
            }
            header_fwrite(&rec, kind, &mut out);
        }
    }
    let table_at = out.len();
    let n = (top - bottom + 1).max(0) as usize;
    if n > 1 << 20 {
        return Err(Fault("an offset table larger than the allocator gives here"));
    }
    out.extend(std::iter::repeat(Some(0)).take(4 * n));
    let mut table = vec![0u32; n];
    let wid = right - left;
    let mut row = h - bottom;
    let mut k = 0usize;
    while h - top <= row {
        table[k] = out.len() as u32;
        k += 1;
        row -= 1;
        for c in 0..4 {
            let Some(plane) = pic.planes[c].as_ref() else {
                if c < 3 {
                    return Err(Fault("a colour plane missing: the coder reads a null pointer"));
                }
                continue;
            };
            channel_write(plane, w as i64 * row as i64 + left as i64, wid, past[c], &mut out)?;
        }
    }
    for (i, v) in table.iter().enumerate() {
        for (j, b) in v.to_be_bytes().into_iter().enumerate() {
            out[table_at + 4 * i + j] = Some(b);
        }
    }
    Ok(Some(out))
}

/// [soft 0x085e4964 channelReadByte]: one channel's run-length data into `plane` from `*at`
/// (advanced by what is decoded). The data is a 16-bit count and that many bytes read into a
/// buffer (`SI_malloc`; bytes the file does not supply are the allocator's: `None`).
fn channel_read_byte(data: &[u8], pos: &mut usize, plane: &mut [u8], at: &mut i64) -> Result<(), Fault> {
    let n = match data.get(*pos..*pos + 2) {
        Some(b) => i16::from_be_bytes([b[0], b[1]]),
        None => return Err(Fault("a channel's count past the end of the file: a stack word nobody set")),
    };
    *pos += 2;
    let nn = n.max(0) as usize;
    let mut buf: Vec<Option<u8>> = (0..nn).map(|i| data.get(*pos + i).copied()).collect();
    if nn > 0 {
        // SI_malloc's marks where the file did not supply the bytes
        if buf[0].is_none() {
            buf[0] = Some(0x6f);
        }
        if buf[nn - 1].is_none() {
            buf[nn - 1] = Some(0x6b);
        }
    }
    *pos = (*pos + nn).min(data.len());
    let get = |i: usize| -> Result<u8, Fault> { buf.get(i).copied().flatten().ok_or(Fault("a coded byte the file did not supply (heap memory nobody set)")) };
    let mut put = |v: u8, at: &mut i64| -> Result<(), Fault> {
        let i = *at;
        if i < 0 || i as usize >= plane.len() {
            return Err(Fault("a decoded byte outside the plane: the original writes past it"));
        }
        plane[i as usize] = v;
        *at += 1;
        Ok(())
    };
    let mut left = n as i32;
    let mut q = 0usize;
    while 0 < left {
        let c = get(q)? as i8;
        if c < 0 {
            q += 1;
            left = left - 1 + c as i32;
            for _ in 0..-(c as i32) {
                let v = get(q)?;
                q += 1;
                put(v, at)?;
            }
        } else {
            left -= 2;
            let v = get(q + 1)?;
            for _ in 0..=c as i32 {
                put(v, at)?;
            }
            q += 2;
        }
    }
    Ok(())
}

/// [soft 0x085e4a0c channelReadShort]: the same for 16-bit data (storage type not 0): the
/// count and the data are big-endian 16-bit words, a sample keeps its low byte.
fn channel_read_short(data: &[u8], pos: &mut usize, plane: &mut [u8], at: &mut i64) -> Result<(), Fault> {
    let n = match data.get(*pos..*pos + 2) {
        Some(b) => i16::from_be_bytes([b[0], b[1]]),
        None => return Err(Fault("a channel's count past the end of the file: a stack word nobody set")),
    };
    *pos += 2;
    let nn = n.max(0) as usize;
    let words: Vec<Option<i16>> = (0..nn).map(|i| data.get(*pos + 2 * i..*pos + 2 * i + 2).map(|b| i16::from_be_bytes([b[0], b[1]]))).collect();
    *pos = (*pos + 2 * nn).min(data.len());
    let get = |i: usize| -> Result<i16, Fault> { words.get(i).copied().flatten().ok_or(Fault("a coded word the file did not supply (heap memory nobody set)")) };
    let mut put = |v: u8, at: &mut i64| -> Result<(), Fault> {
        let i = *at;
        if i < 0 || i as usize >= plane.len() {
            return Err(Fault("a decoded byte outside the plane: the original writes past it"));
        }
        plane[i as usize] = v;
        *at += 1;
        Ok(())
    };
    let mut left = n as i32;
    let mut q = 0usize;
    while 0 < left {
        let c = get(q)?;
        if c < 0 {
            left = left - 1 + c as i32;
            for _ in 0..-(c as i32) {
                q += 1;
                put(get(q)? as u8, at)?;
            }
            q += 1;
        } else {
            left -= 2;
            let v = get(q + 1)? as u8;
            for _ in 0..c as i32 + 1 {
                put(v, at)?;
            }
            q += 2;
        }
    }
    Ok(())
}

/// Reads a Wavefront file [soft 0x085e45b0 SI_pictureReadWavefront] (the original opens the
/// name with `SI_softOpen`, extension `.rla` for kind 1, else `.rlb`, mode `rb`; a failure to
/// open answers −1). The header is read (its kind's fields turned; nothing checked); the
/// picture is the whole window (`active` false: the active area's rows placed inside it, the
/// rest black) or only the active area, ratio 1.0, fields 3, planes of `SI_calloc` (alpha
/// only with a matte). The rows are read from the offset table's **last** entry to its first
/// (top row first), each channel decoded at the current place (a row that decodes to another
/// width than the window's shifts everything after it), with the window's margins skipped
/// before and after.
pub fn read_wavefront(file: &[u8], active: bool, kind: i32) -> Result<Picture, Fault> {
    let kind = if kind == 1 { Kind::Rla } else { Kind::Rlb };
    let hd = header_fread(file, kind);
    let word = |at: usize| -> Result<i16, Fault> {
        match (hd[at], hd[at + 1]) {
            (Some(a), Some(b)) => Ok(i16::from_le_bytes([a, b])),
            _ => Err(Fault("a header field past the end of the file: the record's stack bytes")),
        }
    };
    let (wl, wr, wb, wt) = (word(0)?, word(2)?, word(4)?, word(6)?);
    let (al, ar, ab, at_) = (word(8)?, word(10)?, word(12)?, word(14)?);
    let storage = word(18)?;
    let matte = word(22)?;
    let mut pic = Picture::default();
    let (pre, post, start);
    if !active {
        pic.width = wr.wrapping_add(1).wrapping_sub(wl);
        pic.height = wt.wrapping_add(1).wrapping_sub(wb);
        pre = al as i64 - wl as i64;
        post = wr.wrapping_add(1) as i64 - ar.wrapping_add(1) as i64;
        start = pic.width as i64 * (wt.wrapping_add(1) as i64 - at_.wrapping_add(1) as i64);
    } else {
        pic.width = ar.wrapping_add(1).wrapping_sub(al);
        pic.height = at_.wrapping_add(1).wrapping_sub(ab);
        pre = 0;
        post = 0;
        start = 0;
    }
    pic.ratio = 1.0;
    pic.fields = 3;
    let area = pic.width as i32 * pic.height as i32;
    let rows = at_.wrapping_add(1) as i32 - ab as i32;
    if rows > 1 << 20 || area > 1 << 26 {
        return Err(Fault("a size beyond what the allocator gives here"));
    }
    let mut pos = HEADER;
    let mut table = Vec::new();
    for i in 0..rows.max(0) as usize {
        table.push(file.get(pos + 4 * i..pos + 4 * i + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])));
    }
    let calloc = |n: i32| (n > 0).then(|| vec![0u8; n as usize]);
    pic.planes[0] = calloc(area);
    pic.planes[1] = calloc(area);
    pic.planes[2] = calloc(area);
    pic.planes[3] = if matte != 0 { calloc(area) } else { None };
    let nch = if matte != 0 { 4 } else { 3 };
    let mut at = [start; 4];
    let mut r = rows;
    loop {
        r -= 1;
        if r < 0 {
            break;
        }
        for a in at.iter_mut() {
            *a += pre;
        }
        pos = table[r as usize].ok_or(Fault("an offset past the end of the file: SI_malloc's bytes"))? as usize;
        for c in 0..nch {
            let plane = pic.planes[c].as_mut().ok_or(Fault("a plane of no pixels: the decoder writes through a null pointer"))?;
            let mut a = at[c];
            if storage != 0 {
                channel_read_short(file, &mut pos, plane, &mut a)?;
            } else {
                channel_read_byte(file, &mut pos, plane, &mut a)?;
            }
            at[c] = a;
        }
        for a in at.iter_mut() {
            *a += post;
        }
    }
    Ok(pic)
}

/// [soft 0x085e4520 readRlaPicture]: [`read_wavefront`].
pub fn read_rla_picture(file: &[u8], active: bool, kind: i32) -> Result<Picture, Fault> {
    read_wavefront(file, active, kind)
}

/// [soft 0x085e4568 pictureReadRla]: [`read_wavefront`].
pub fn picture_read_rla(file: &[u8], active: bool, kind: i32) -> Result<Picture, Fault> {
    read_wavefront(file, active, kind)
}

/// [soft 0x085e4544 writeRlaPicture]: [`write_wavefront`].
pub fn write_rla_picture(pic: &Picture, name: &[u8], active: bool, kind: i32, env: &Environment) -> Result<Option<Vec<Option<u8>>>, Fault> {
    write_wavefront(pic, name, active, kind, env)
}

/// [soft 0x085e458c pictureWriteRla]: [`write_wavefront`].
pub fn picture_write_rla(pic: &Picture, name: &[u8], active: bool, kind: i32, env: &Environment) -> Result<Option<Vec<Option<u8>>>, Fault> {
    write_wavefront(pic, name, active, kind, env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Environment {
        Environment { month: 9, day: 14, year: 2026, login: None }
    }

    #[test]
    fn written_and_read_back() {
        for (alpha, kind) in [(false, 1), (true, 2)] {
            let p = crate::alias::testpic::picture(3, 41, 7, alpha);
            let f = write_wavefront(&p, b"/out/x", false, kind, &env()).unwrap().unwrap();
            let bytes: Vec<u8> = f.iter().map(|b| b.unwrap_or(0)).collect();
            assert_eq!(&bytes[0x210..0x220], b"October 14, 2026");
            assert_eq!(&bytes[0x1f0..0x1f7], b"(null)\0");
            let q = read_wavefront(&bytes, false, kind).unwrap();
            assert_eq!((q.width, q.height), (41, 7));
            assert_eq!(q.planes, p.planes);
        }
    }

    #[test]
    fn runs_and_literals() {
        // 1 2 3 then six 9s then 4 5: count 10 → 11 bytes coded
        let row = [1u8, 2, 3, 9, 9, 9, 9, 9, 9, 4, 5];
        let mut out = Vec::new();
        channel_write(&row, 0, 10, None, &mut out).unwrap();
        let b: Vec<u8> = out.iter().map(|v| v.unwrap()).collect();
        assert_eq!(b, [0, 9, 0xfd, 1, 2, 3, 5, 9, 0xfe, 4, 5]);
        let mut plane = vec![0u8; 11];
        let mut at = 0;
        let mut pos = 0;
        channel_read_byte(&b, &mut pos, &mut plane, &mut at).unwrap();
        assert_eq!(plane, row);
    }
}
