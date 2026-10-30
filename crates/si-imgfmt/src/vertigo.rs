// SPDX-License-Identifier: MIT
//! Vertigo pictures (`.pic` of Cubicomp's Vertigo) (finding 0014, "Vertigo").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `vertigo.c` (0x085e35a0–0x085e4511), all six of
//! its functions.
//!
//! The file: a header of 232 bytes written from three records on the stack, in the machine's
//! byte order: magic 0x1ca, the text `@(#).pic     1986/06/01 1.01`, magic 0x1ca again at
//! 0x60, then a "ratio given" byte, a channel mask (8 red, 4 green, 2 blue, 1 alpha), the
//! four channel depths, a field byte (0 frame, 1 odd, 2 even, 3 and 4 mixed) and a YUV byte;
//! then width and height; then the coded window x0, x1, y0, y1. The rest of the three
//! records is what the stack held (undefined here: 153 of the 232 bytes). Then a table of
//! one big-endian offset and length per row, and the rows, last row of the picture first,
//! each channel coded on its own: a count byte `c` below 0x80 repeats the next byte `c + 1`
//! times, one of 0x80 and up copies the next `256 − c` bytes.

use crate::effect::{flip_y, Fault as EFault};
use crate::tiff::{malloc, Blk, Fault, Mem};
use si_picture::Picture;
use si_x87::F80;

/// The coder's run-length state machine, shared by the length and the writer: calls `out`
/// with each byte `channelWriteVertigo` puts.
fn code(row: &[u8], mut out: impl FnMut(u8)) {
    let mut count = 0usize; // bytes in the pending unit
    let mut same = 0usize; // equal bytes at its end
    let mut start = 0usize; // where the pending unit starts
    let mut prev = 0u8;
    for (i, &cur) in row.iter().enumerate() {
        let last = prev;
        prev = cur;
        if count == 0 {
            start = i;
            count = 1;
            same = 1;
            continue;
        }
        count += 1;
        if cur == last {
            same += 1;
            if same == 3 && count != 3 {
                // a literal stretch ends where a run of three begins
                out((0x103 - count) as u8);
                for k in 0..count - 3 {
                    out(row[start + k]);
                }
                start += count - 3;
                count = 3;
            }
            if count == 0x80 {
                if same == 0x80 {
                    out(0x7f);
                    out(row[start]);
                } else {
                    out(0x80);
                    out_bytes(&row[start..start + 0x80], &mut out);
                }
                count = 0;
            }
        } else {
            if same > 2 {
                out((count - 2) as u8);
                out(row[start]);
                start = i;
                count = 1;
            }
            if count == 0x80 {
                out(0x80);
                out_bytes(&row[start..start + 0x80], &mut out);
                count = 0;
            }
            same = 1;
        }
    }
    if count != 0 {
        if same > 2 {
            out((count - 1) as u8);
            out(row[start]);
        } else {
            out((0x100 - count) as u8);
            out_bytes(&row[start..start + count], &mut out);
        }
    }
}

fn out_bytes(b: &[u8], out: &mut impl FnMut(u8)) {
    for &x in b {
        out(x);
    }
}

/// The coded length of a row of one channel [soft 0x085e4458 channelLengthVertigo]: the
/// same state machine as [`channel_write`], counting (checked path by path against it).
pub fn channel_length(row: &[u8]) -> i32 {
    let mut n = 0;
    code(row, |_| n += 1);
    n
}

/// One channel of a row, run-length coded [soft 0x085e4264 channelWriteVertigo]: runs of
/// three equal bytes or more (up to 128) as a count byte `run − 1` and the byte; the bytes
/// between as a count byte `256 − n` and the `n` bytes (up to 128). A literal stretch that
/// reaches 128 bytes is written at once, even when its last bytes begin a run.
pub fn channel_write(row: &[u8], out: &mut Vec<Option<u8>>) {
    code(row, |b| out.push(Some(b)));
}

/// Writes a picture as a Vertigo file [soft 0x085e3df4 SI_pictureWriteVertigo]
/// (`writeVertigoPicture` [soft 0x085e35a0 writeVertigoPicture] only calls it). The original
/// opens the name with `SI_softOpen` and the extension `.pic`; here the file's bytes are
/// returned, `None` for the stack bytes the header records carry unset (and for the field
/// byte of a picture whose `fields` is not 1, 2 or 3). The picture is turned upside down for
/// the writing and back (`SI_pictureFlipY` (0x085cae7c)), so the rows go out last first.
/// Row table: the offset of each row, starting after the table, and the sum of its channels'
/// coded lengths as a 16-bit word (offsets advance by that word, sign-extended).
pub fn write_vertigo(pic: &Picture) -> Result<Vec<Option<u8>>, Fault> {
    let (w, h) = (pic.width, pic.height);
    let mut head: Vec<Option<u8>> = vec![None; 0xe8];
    let mut set = |at: usize, b: &[u8]| {
        for (k, &x) in b.iter().enumerate() {
            head[at + k] = Some(x);
        }
    };
    set(0, &0x1cau32.to_le_bytes());
    set(4, b"@(#).pic     1986/06/01 1.01\0");
    set(0x28, &[0]);
    set(0x60, &0x1cau32.to_le_bytes());
    let alpha = pic.planes[3].is_some();
    set(0x64, &[0, if alpha { 0xf } else { 0xe }, 8, 8, 8, if alpha { 8 } else { 0 }, 0, 0, 0, 0]);
    match pic.fields {
        3 => set(0x6e, &[0]),
        1 => set(0x6e, &[1]),
        2 => set(0x6e, &[2]),
        _ => {}
    }
    set(0x70, &[0, 0, 0, 0, 0]);
    set(0x76, &(w as i32).to_le_bytes());
    set(0x7a, &(h as i32).to_le_bytes());
    set(0x7e, &[0]);
    for (k, v) in [0i32, w as i32 - 1, 0, h as i32 - 1].iter().enumerate() {
        set(0x80 + 4 * k, &v.to_le_bytes());
    }
    if w < 0 {
        return Err(Fault("a negative width: the coder counts down from it past zero"));
    }
    let mut p = pic.clone();
    flip_y(&mut p).map_err(efault)?;
    let wu = w as usize;
    let planes: Vec<&Vec<u8>> = {
        let mut v = Vec::new();
        for c in 0..4 {
            match &p.planes[c] {
                Some(q) => v.push(q),
                None if c < 3 && w > 0 && h > 0 => return Err(Fault("a colour plane the writer reads is absent")),
                None => {}
            }
        }
        v
    };
    let row = |q: &Vec<u8>, r: usize| -> Result<Vec<u8>, Fault> {
        q.get(r * wu..(r + 1) * wu).map(|s| s.to_vec()).ok_or(Fault("a plane shorter than the picture"))
    };
    let mut out = head;
    let mut off: i32 = (h as i32) * 6 + 0xe8;
    for r in 0..h.max(0) as usize {
        out.extend(off.to_be_bytes().map(Some));
        let mut len: i16 = 0;
        for q in &planes {
            len = len.wrapping_add(channel_length(&row(q, r)?) as i16);
        }
        out.extend(len.to_be_bytes().map(Some));
        off = off.wrapping_add(len as i32);
    }
    for r in 0..h.max(0) as usize {
        for q in &planes {
            channel_write(&row(q, r)?, &mut out);
        }
    }
    Ok(out)
}

fn efault(e: EFault) -> Fault {
    match e {
        EFault::NullPlane => Fault("a plane the flip reads is absent"),
        EFault::ShortPlane => Fault("a plane shorter than the picture"),
        _ => Fault("the flip of a picture the original cannot flip"),
    }
}

/// The `SI_Picture` record as the Vertigo reader leaves it: `None` for a field it does not
/// write (the caller's value stays).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VertigoPicture {
    pub width: i16,
    pub height: i16,
    pub ratio: f32,
    pub fields: Option<i16>,
    pub planes: [Option<Blk>; 4],
}

/// What [`read_vertigo`] leaves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadVertigo {
    pub ret: i32,
    pub picture: Option<VertigoPicture>,
    pub messages: Vec<(Vec<u8>, i16)>,
}

fn le32(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// Writes `n` zeros at the plane's pointer (a loop that counts down to zero: a negative
/// count would run four thousand million times).
fn zeros(p: &mut Mem, at: &mut usize, n: i32) -> Result<(), Fault> {
    if n < 0 {
        return Err(Fault("a fill of a negative count (four thousand million bytes)"));
    }
    for _ in 0..n {
        *p.get_mut(*at).ok_or(Fault("a write outside a heap block"))? = Some(0);
        *at += 1;
    }
    Ok(())
}

/// Reads a Vertigo file [soft 0x085e35dc SI_pictureReadVertigo] (`readVertigoPicture`
/// [soft 0x085e35bc readVertigoPicture] only calls it). `file` is what `SI_softOpen` opened
/// for the name and the extension `.pic` (`None`: nothing, 0); `window` not 0 makes the
/// picture the coded window alone (vertigo2soft's `-w`), else the whole frame with zeros
/// around the window; `program` and `path` go into the messages ("%s: Cannot read %s",
/// "%s: %s has a wrong magic number!", "%s: Mixed fields are not supported!", "%s: Cannot
/// convert Vertigo YUV file!", all level 3, 0).
///
/// Width and height are the header's (the window's with `window`); ratio 1.0 when the
/// "ratio given" byte is set, else `height / (width × 0.75)` in the x87 unit; `fields` from
/// the field byte (0: 3, 1: 1, 2: 2; any other value but 3 and 4 leaves the record's field
/// as it was). A plane of `width × height` bytes for each channel of the mask (alpha cleared
/// when absent; a colour plane not in the mask is left as it was). The row table is skipped
/// by reading it, the rows read one after another, each channel's row filled from the left
/// edge of the window and decoded until the window's width is reached or passed, nothing
/// checked; the picture is turned upside down at the end (`SI_pictureFlipY`).
pub fn read_vertigo(file: Option<&[u8]>, window: i16, program: &[u8], path: &[u8]) -> Result<ReadVertigo, Fault> {
    let mut out = ReadVertigo::default();
    let Some(data) = file else { return Ok(out) };
    let msg = |text: &str| -> (Vec<u8>, i16) {
        let mut m = program.to_vec();
        m.extend_from_slice(b": ");
        let t = text.replace("{}", &String::from_utf8_lossy(path));
        m.extend_from_slice(t.as_bytes());
        (m, 3)
    };
    if data.len() < 0xe8 {
        out.messages.push(msg("Cannot read {}"));
        return Ok(out);
    }
    let a = &data[..0x76];
    let b = &data[0x76..0x80];
    let c = &data[0x80..0xe8];
    if le32(a, 0) != 0x1ca || le32(a, 0x60) != 0x1ca {
        out.messages.push(msg("{} has a wrong magic number!"));
        return Ok(out);
    }
    let (mut w, mut h) = (le32(b, 0), le32(b, 4));
    let (x0, x1, y0, y1) = (le32(c, 0), le32(c, 4), le32(c, 8), le32(c, 12));
    if window != 0 {
        w = x1.wrapping_sub(x0).wrapping_add(1);
        h = y1.wrapping_sub(y0).wrapping_add(1);
    }
    let size = w.wrapping_mul(h);
    let mut pic = VertigoPicture { width: w as i16, height: h as i16, ..Default::default() };
    pic.ratio = if a[0x64] == 0 {
        let q = F80::from_i32(pic.height as i32) / (F80::from_i32(pic.width as i32) * F80::from_f64(0.75));
        q.to_f32()
    } else {
        1.0
    };
    match a[0x6e] as i8 {
        0 => pic.fields = Some(3),
        1 => pic.fields = Some(1),
        2 => pic.fields = Some(2),
        3 | 4 => {
            out.messages.push((format!("{}: Mixed fields are not supported!", String::from_utf8_lossy(program)).into_bytes(), 3));
            out.picture = Some(pic);
            return Ok(out);
        }
        _ => {}
    }
    if b[8] == 1 {
        out.messages.push((format!("{}: Cannot convert Vertigo YUV file!", String::from_utf8_lossy(program)).into_bytes(), 3));
        out.picture = Some(pic);
        return Ok(out);
    }
    let mask = a[0x65];
    let mut order: Vec<usize> = Vec::new();
    for (bit, c) in [(8u8, 0usize), (4, 1), (2, 2), (1, 3)] {
        if mask & bit != 0 {
            pic.planes[c] = Some(malloc(size as u32)?);
            order.push(c);
        }
    }
    if mask & 1 == 0 {
        pic.planes[3] = Some(None);
    }
    let (rows, top, left, span, right, bottom): (i16, i32, i16, i16, i16, i32) = if window == 0 {
        let rows = y1.wrapping_sub(y0).wrapping_add(1) as i16;
        let top = h.wrapping_sub(y1.wrapping_add(1)).wrapping_mul(w);
        let left = x0 as i16;
        let span = (x1 as i16).wrapping_sub(left).wrapping_add(1);
        let right = (w as i16).wrapping_sub((x1 as i16).wrapping_add(1));
        (rows, top, left, span, right, w.wrapping_mul(y0))
    } else {
        let rows = y1.wrapping_sub(y0).wrapping_add(1) as i16;
        let span = (x1 as i16).wrapping_sub(x0 as i16).wrapping_add(1);
        (rows, 0, 0, span, 0, 0)
    };
    if rows < 0 {
        return Err(Fault("a row table skipped four thousand million times"));
    }
    let mut pos = 0xe8usize + 6 * rows as usize;
    let mut getc = || -> i32 {
        let v = data.get(pos).map_or(-1, |&x| x as i32);
        if pos < data.len() {
            pos += 1;
        }
        v
    };
    let mut at = vec![0usize; order.len()];
    let mut blocks: Vec<Mem> = order.iter().map(|&c| pic.planes[c].clone().unwrap().unwrap_or_default()).collect();
    for k in 0..order.len() {
        zeros(&mut blocks[k], &mut at[k], top)?;
    }
    for _ in 0..rows {
        for k in 0..order.len() {
            zeros(&mut blocks[k], &mut at[k], left as i32)?;
            let mut left_to_do = span as i32;
            while left_to_do > 0 {
                let mut cb = getc() as u16 as i16;
                if cb > 0x7f {
                    cb = cb.wrapping_sub(0x100);
                }
                if cb < 0 {
                    for _ in 0..-(cb as i32) {
                        let v = getc() as u8;
                        *blocks[k].get_mut(at[k]).ok_or(Fault("a write outside a heap block"))? = Some(v);
                        at[k] += 1;
                    }
                    left_to_do += cb as i32;
                } else {
                    let v = getc() as u8;
                    for _ in 0..cb as i32 + 1 {
                        *blocks[k].get_mut(at[k]).ok_or(Fault("a write outside a heap block"))? = Some(v);
                        at[k] += 1;
                    }
                    left_to_do = left_to_do - 1 - cb as i32;
                }
            }
            zeros(&mut blocks[k], &mut at[k], right as i32)?;
        }
    }
    for k in 0..order.len() {
        zeros(&mut blocks[k], &mut at[k], bottom)?;
    }
    for (k, &c) in order.iter().enumerate() {
        if pic.planes[c] != Some(None) {
            pic.planes[c] = Some(Some(std::mem::take(&mut blocks[k])));
        }
    }
    flip_mem(&mut pic)?;
    out.ret = 1;
    out.picture = Some(pic);
    Ok(out)
}

/// `SI_pictureFlipY` (0x085cae7c) on planes of bytes that may be undefined (the rule of
/// [`flip_y`]: rows `y` and `height − 1 − y` exchanged for the first `height >> 1` rows,
/// alpha too; nothing for a height below 2 or a width below 1).
fn flip_mem(p: &mut VertigoPicture) -> Result<(), Fault> {
    if p.height >> 1 <= 0 || p.width <= 0 {
        return Ok(());
    }
    let (w, h) = (p.width as usize, p.height as usize);
    for c in 0..4 {
        match &mut p.planes[c] {
            Some(Some(q)) => {
                if q.len() < w * h {
                    return Err(Fault("a plane shorter than the picture"));
                }
                for y in 0..h / 2 {
                    for x in 0..w {
                        q.swap(y * w + x, (h - 1 - y) * w + x);
                    }
                }
            }
            _ if c < 3 => return Err(Fault("a colour plane the flip reads is not the reader's")),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coder_units() {
        let mut v = Vec::new();
        channel_write(&[1, 2, 3, 3, 3, 3, 4], &mut v);
        let b: Vec<u8> = v.into_iter().map(|x| x.unwrap()).collect();
        assert_eq!(b, vec![0xfe, 1, 2, 3, 3, 0xff, 4]);
        assert_eq!(channel_length(&[1, 2, 3, 3, 3, 3, 4]), 7);
        assert_eq!(channel_length(&[]), 0);
    }
}
