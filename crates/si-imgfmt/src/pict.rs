// SPDX-License-Identifier: MIT
//! `pict.c`: Macintosh PICT files, video fields and frame names (finding 0014, "Macintosh
//! PICT" and "Fields and frame sequences").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `pict.c` (0x085d0060–0x085d28fe), all 11 since the
//! sixth pass, which added the writer of line drawings ([`lin_write_pict`], on the line-file
//! reader of `linInOut.c` in [`crate::lin`]).
//!
//! **PICT.** The writer makes a version 2 picture of one `DirectBitsRect` (opcode 0x9a): 32
//! bits a pixel, the rows PackBits-coded with alpha (when the picture has it), red, green and
//! blue one after another in each row. The reader takes the opcodes such a file holds, and
//! `PackBitsRect` (0x98, 8-bit colour-table pictures); others are reported and skipped
//! without their data. What this build does of its own, all reproduced:
//!
//! - The picture's size word is the low 16 bits of the **whole file's** length, MacBinary
//!   header and 512-byte preamble included; the header opcode's rectangle is
//!   (0, 0, width, height) in 16.16, the second half the wrong way round.
//! - A colour-table pixel is an index through `signed char`: indices of 128 and up take
//!   their colours from the 768 bytes in front of the table (undefined here).
//! - Row counts run on from one pixel opcode to the next: a `PackBitsRect` continues where the
//!   previous one stopped, a `DirectBitsRect` starts again at row 0 and reads the rows of every
//!   earlier pixel opcode a second time.
//! - Fewer than 32 bits a pixel, and compressed QuickTime pictures, are refused with a message
//!   and the file left open; the planes already made are left to the caller.
//!
//! **Model.** Heap blocks are bytes that may be undefined (`None`), as in [`crate::tiff`]: a
//! plane the reader allocates and does not fill keeps the allocator's two marks and unknown
//! bytes. The stack words the reader reads into are undefined until the file fills them; a
//! branch, an index, a size or a count that would depend on one is a [`TFault`].
//!
//! What a field holds (an open point since finding 0003): splitting a full frame puts the
//! rows at even positions in memory (0, 2, 4 …) into the picture marked `fields` 2 ("EVEN
//! FIELD") and the odd ones into the picture marked 1 ("ODD FIELD"); a frame of odd height
//! gives both fields `(height + 1) / 2` rows, the odd field's last row being the frame's last
//! row again.

use crate::effect::Fault;
use crate::tiff::{malloc, Blk, Fault as TFault, Mem};
use si_picture::Picture;
use si_x87::{Round, F80};

// ---------------------------------------------------------------------------------------
// Macintosh PICT
// ---------------------------------------------------------------------------------------

/// A `FILE *` opened for reading on a file's bytes, with the end-of-file flag the reader
/// tests: set by a read that asks for more than is left, cleared by a successful seek.
#[derive(Clone, Debug)]
struct PFile<'a> {
    data: &'a [u8],
    pos: i64,
    eof: bool,
}

impl<'a> PFile<'a> {
    fn new(data: &'a [u8]) -> Self {
        PFile { data, pos: 0, eof: false }
    }

    fn read(&mut self, n: usize) -> &'a [u8] {
        let from = (self.pos.max(0) as usize).min(self.data.len());
        let to = from.saturating_add(n).min(self.data.len());
        if to - from < n {
            self.eof = true;
        }
        if to > from {
            self.pos = to as i64;
        }
        &self.data[from..to]
    }

    /// `fseek(f, off, SEEK_SET)`; a negative position fails and changes nothing.
    fn seek_set(&mut self, off: i64) {
        if off >= 0 {
            self.pos = off;
            self.eof = false;
        }
    }

    /// `fseek(f, d, SEEK_CUR)`
    fn seek_cur(&mut self, d: i64) {
        self.seek_set(self.pos + d);
    }

    fn tell(&self) -> i64 {
        self.pos
    }
}

/// A 16-bit word on the stack or in a global, as its two bytes in memory (low byte first);
/// `None` for a byte nobody set.
type Word = [Option<u8>; 2];

fn word(w: Word) -> Option<u16> {
    Some(u16::from_le_bytes([w[0]?, w[1]?]))
}

fn need_word(w: Word, why: &'static str) -> Result<u16, TFault> {
    word(w).ok_or(TFault(why))
}

/// `SI_byteswap_fread(dst, size, count, f)` (0x083f9ba0) into bytes that may be undefined:
/// `fread`, which puts whatever the file holds at the front of `dst` (a partial last element
/// included), then each whole element read turned from big-endian into the machine's order.
/// Answers the whole elements read.
fn bswap_fread(f: &mut PFile, dst: &mut [Option<u8>], size: usize, count: usize) -> usize {
    let got = f.read(size * count);
    for (k, &b) in got.iter().enumerate() {
        dst[k] = Some(b);
    }
    let whole = if size == 0 { 0 } else { got.len() / size };
    if size > 1 {
        for e in 0..whole {
            dst[e * size..(e + 1) * size].reverse();
        }
    }
    whole
}

fn bswap_word(f: &mut PFile, w: &mut Word) -> usize {
    bswap_fread(f, w, 2, 1)
}

/// A byte of a block at a signed index: undefined outside the block.
fn at(m: &[Option<u8>], i: i64) -> Option<u8> {
    if i < 0 || i as usize >= m.len() {
        None
    } else {
        m[i as usize]
    }
}

fn block(b: &Blk) -> Result<&Mem, TFault> {
    b.as_ref().ok_or(TFault("a read through a null pointer"))
}

fn block_mut(b: &mut Blk) -> Result<&mut Mem, TFault> {
    b.as_mut().ok_or(TFault("a write through a null pointer"))
}

fn put(m: &mut [Option<u8>], off: i64, v: Option<u8>) -> Result<(), TFault> {
    if off < 0 || off as usize >= m.len() {
        return Err(TFault("a write outside a heap block"));
    }
    m[off as usize] = v;
    Ok(())
}

/// `memcpy(dst + d, src + s, n)` between blocks; bytes read from outside `src` are undefined.
fn copy(dst: &mut Blk, d: i64, src: &Blk, s: i64, n: i64) -> Result<(), TFault> {
    if n == 0 {
        return Ok(());
    }
    if n < 0 {
        return Err(TFault("a copy of a negative length (a size_t of nearly 4 GB)"));
    }
    let bytes: Vec<Option<u8>> = {
        let s_blk = block(src)?;
        (0..n).map(|k| at(s_blk, s + k)).collect()
    };
    let d_blk = block_mut(dst)?;
    for (k, b) in bytes.into_iter().enumerate() {
        put(d_blk, d + k as i64, b)?;
    }
    Ok(())
}

/// The globals of `pict.c` the reader uses: `PixMap` (0x0882a380, 46 bytes, as the last
/// pixel opcode left them; zero when the program starts), which a pixel opcode the file
/// cuts short leaves partly as it was; `buf0` and `buf1` (0x0882a300, 0x0882a304), the row
/// buffers, set to null once a header is accepted, freed at the end and left allocated by a
/// refusal after the header. `clrMap` (0x0882a3b0) is made again by every colour-table
/// opcode before it is used and is kept inside [`PictState::read`].
#[derive(Clone, Debug, PartialEq)]
pub struct PictState {
    pub pixmap: [Option<u8>; 46],
    pub buf0: Blk,
    pub buf1: Blk,
}

impl Default for PictState {
    fn default() -> Self {
        PictState { pixmap: [Some(0); 46], buf0: None, buf1: None }
    }
}

/// Offsets of the fields of `PixMap` the reader uses (16-bit words, machine order).
const PM_ROW_BYTES: usize = 0x00;
const PM_TOP: usize = 0x02;
const PM_LEFT: usize = 0x04;
const PM_BOTTOM: usize = 0x06;
const PM_RIGHT: usize = 0x08;
const PM_PIXEL_SIZE: usize = 0x1c;
const PM_CMP_COUNT: usize = 0x1e;

/// The `SI_Picture` record as the PICT reader fills it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PictPicture {
    pub width: i16,
    pub height: i16,
    pub ratio: f32,
    pub fields: i16,
    pub planes: [Blk; 4],
}

/// What [`PictState::read`] leaves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadPict {
    /// The answer: 1 read, 0 refused (or not opened).
    pub ret: i32,
    /// The record, once the header has been accepted (untouched before).
    pub picture: Option<PictPicture>,
    /// `SI_statusDisplayMessage(text, level)`, in order.
    pub messages: Vec<(Vec<u8>, i16)>,
    /// What went to standard error.
    pub stderr: Vec<u8>,
    /// Whether the file was closed (a refusal after the header leaves it open).
    pub closed: bool,
}

/// `buf0`'s size for a row of `row_bytes`: `row_bytes × 1.2`, in the x87 unit, truncated.
fn packed_room(row_bytes: i32) -> i32 {
    (F80::from_i32(row_bytes) * F80::from_f64(1.2)).to_i32(Round::Zero)
}

/// The colour table of a `PackBitsRect` [soft 0x085d196c readMap]: `n` entries of an index
/// word and three 16-bit colours, into a new block of `6 n` bytes (the old table is not
/// freed). An entry whose index is not its position stops the reading with three lines on
/// standard error ("Error while reading colour table!", "val = %d", "pos = %d", the index
/// read and the file's position) and −1; the entries not read keep what the allocator left.
/// Otherwise the answer is the file's position.
fn read_map(f: &mut PFile, n: i32, clr: &mut Blk, stderr: &mut Vec<u8>) -> Result<i64, TFault> {
    *clr = malloc((n.wrapping_mul(6)) as u32)?;
    let mut idx: Word = [None, None];
    for i in 0..n.max(0) {
        bswap_word(f, &mut idx);
        let v = need_word(idx, "a colour index the file did not fill (stale stack)")? as i16 as i32;
        if v != i {
            stderr.extend_from_slice(b"Error while reading colour table!\n");
            stderr.extend_from_slice(format!("val = {v}\n").as_bytes());
            stderr.extend_from_slice(format!("pos = {}\n", f.tell() as i32).as_bytes());
            return Ok(-1);
        }
        let m = block_mut(clr)?;
        let at6 = 6 * i as usize;
        let mut entry = [None; 6];
        entry.copy_from_slice(&m[at6..at6 + 6]);
        bswap_fread(f, &mut entry, 2, 3);
        m[at6..at6 + 6].copy_from_slice(&entry);
    }
    Ok(f.tell())
}

impl PictState {
    /// The fields of a `PackBitsRect` [soft 0x085d1a48 readop98]: the pixel map (46 bytes, 23
    /// words turned to the machine's order) into `PixMap`, the colour table's header (seed,
    /// flags, size) and the table itself ([`read_map`], size + 1 entries), the source and
    /// destination rectangles and the transfer mode, which are dropped.
    fn readop98(&mut self, f: &mut PFile, clr: &mut Blk, stderr: &mut Vec<u8>) -> Result<(), TFault> {
        bswap_fread(f, &mut self.pixmap, 2, 0x17);
        let mut ct = [None; 8];
        bswap_fread(f, &mut ct, 2, 4);
        let size = need_word([ct[6], ct[7]], "a colour table size the file did not fill (stale stack)")? as i16 as i32;
        read_map(f, size + 1, clr, stderr)?;
        let _rects = f.read(16);
        let mut mode: Word = [None, None];
        bswap_word(f, &mut mode);
        Ok(())
    }

    /// The fields of a `DirectBitsRect` [soft 0x085d1b20 readop9a]: the base address (4
    /// bytes, dropped), the pixel map into `PixMap` as in [`PictState::readop98`], the two
    /// rectangles and the mode, dropped.
    fn readop9a(&mut self, f: &mut PFile) {
        let mut base = [None; 4];
        bswap_fread(f, &mut base, 4, 1);
        bswap_fread(f, &mut self.pixmap, 2, 0x17);
        let _rects = f.read(16);
        let mut mode: Word = [None, None];
        bswap_word(f, &mut mode);
    }

    fn pm(&self, off: usize) -> Word {
        [self.pixmap[off], self.pixmap[off + 1]]
    }

    fn pm_i16(&self, off: usize, why: &'static str) -> Result<i16, TFault> {
        need_word(self.pm(off), why).map(|v| v as i16)
    }

    /// Reads a Macintosh PICT file [soft 0x085d0060 SI_pictureReadPict]. `file` is what
    /// `SI_softOpen` (0x085e3164) opened for the name and the extension `.pict` (`None`:
    /// nothing, and 0); `mac` is 0x80 for a file behind a MacBinary header.
    ///
    /// From offset `mac + 512`: the size word (dropped) and the frame; zero words skipped; the
    /// version opcode 0x11 ("This is not a PICT version 2 file.", level 3, 0) and 0x2ff
    /// ("This is no a PICT version 2 file."). The picture is then `right − left` by
    /// `bottom − top` (16 bits), ratio 1.0, `fields` 3, three planes of `SI_malloc`, no alpha.
    /// Opcodes, each followed by a skip to an even position: 0 and 0x1e nothing; 1 a region,
    /// skipped by its size; 0xc00 the header (24 bytes, dropped); 0x98 and 0x9a the pixels
    /// (an alpha plane is made when the pixel map has four components); 0x8200 refused
    /// ("Sorry, cannot handle compressed files", level 3, 0); any other "Opcode 0x%x is not
    /// supported!" (level 2) and the next word is taken as the next opcode. 0xff, or a word
    /// that cannot be read, ends the picture: 1.
    ///
    /// The rows of a pixel opcode: the pixel map's `bottom − top` rows, each a byte count (a
    /// word when the row bytes are above 250) and that many bytes read into `buf0`
    /// (`row bytes × 1.2` bytes), unpacked into `buf1` (`row bytes`): for 0x98 with
    /// `si_unPackBit` (0x085e2d50), each byte an index (signed) into the colour table, whose
    /// first byte of each colour goes to red, green and blue; for 0x9a with
    /// [`un_pack_bit_9a`], then alpha (with four components) from the start of the row and
    /// red, green and blue from the next three stretches of `right − left` bytes. Rows
    /// outside the picture are read and dropped.
    pub fn read(&mut self, file: Option<&[u8]>, mac: i32) -> Result<ReadPict, TFault> {
        let mut out = ReadPict::default();
        let Some(data) = file else { return Ok(out) };
        let mut f = PFile::new(data);
        f.seek_set(mac.wrapping_add(0x200) as i64);
        let mut size_word: Word = [None, None];
        bswap_word(&mut f, &mut size_word);
        let mut frame = [None; 8];
        for (k, &b) in f.read(8).iter().enumerate() {
            frame[k] = Some(b);
        }
        for k in 0..4 {
            frame.swap(2 * k, 2 * k + 1);
        }
        let mut v: Word = [None, None];
        bswap_word(&mut f, &mut v);
        loop {
            let x = need_word(v, "a version word the file did not fill (stale stack)")?;
            if x != 0 || f.eof {
                break;
            }
            bswap_word(&mut f, &mut v);
        }
        let refuse = |out: &mut ReadPict, text: &[u8]| {
            out.messages.push((text.to_vec(), 3));
            out.ret = 0;
        };
        if need_word(v, "")? != 0x11 || f.eof {
            refuse(&mut out, b"This is not a PICT version 2 file.\n");
            return Ok(out);
        }
        let mut ver: Word = [None, None];
        bswap_word(&mut f, &mut ver);
        if need_word(ver, "a version number the file did not fill (stale stack)")? != 0x2ff {
            refuse(&mut out, b"This is no a PICT version 2 file.\n");
            return Ok(out);
        }
        let fw = |k: usize| need_word([frame[2 * k], frame[2 * k + 1]], "a frame the file did not fill").map(|v| v as i16);
        let (top, left, bottom, right) = (fw(0)?, fw(1)?, fw(2)?, fw(3)?);
        let width = right.wrapping_sub(left);
        let height = bottom.wrapping_sub(top);
        let size = height as i32 * width as i32;
        let mut pic = PictPicture { width, height, ratio: 1.0, fields: 3, planes: [None, None, None, None] };
        for c in 0..3 {
            pic.planes[c] = malloc(size as u32)?;
        }
        self.buf0 = None;
        self.buf1 = None;
        let mut clr: Blk = None;
        let mut row: i32 = 0;
        let mut total: i32 = 0;
        let mut count: Word = [None, None];
        let mut count_byte: Option<u8> = None;
        let mut region: Word = [None, None];
        let w = width as i64;
        let mut op: Word = [None, None];
        bswap_word(&mut f, &mut op);
        loop {
            let o = need_word(op, "an opcode the file did not fill (stale stack)")?;
            if o == 0xff {
                break;
            }
            match o {
                0x98 | 0x9a => {
                    if o == 0x98 {
                        self.readop98(&mut f, &mut clr, &mut out.stderr)?;
                    } else {
                        self.readop9a(&mut f);
                    }
                    if let Some(b) = self.pixmap[PM_ROW_BYTES + 1].as_mut() {
                        *b &= 0x7f;
                    }
                    let cmp = self.pm_i16(PM_CMP_COUNT, "a component count the file did not fill")?;
                    if cmp == 4 && pic.planes[3].is_none() {
                        pic.planes[3] = malloc(size as u32)?;
                    }
                    if o == 0x9a {
                        let ps = self.pm_i16(PM_PIXEL_SIZE, "a pixel size the file did not fill")?;
                        if ps / 8 <= 3 {
                            out.picture = Some(pic);
                            refuse(&mut out, b"Sorry, cannot handle less than 32 Bits per pixel");
                            return Ok(out);
                        }
                    }
                    let span = if o == 0x9a {
                        let r = self.pm_i16(PM_RIGHT, "a pixel map the file did not fill")? as i32;
                        let l = self.pm_i16(PM_LEFT, "a pixel map the file did not fill")? as i32;
                        r - l
                    } else {
                        0
                    };
                    let rb = self.pm_i16(PM_ROW_BYTES, "a row size the file did not fill")? as i32;
                    if self.buf0.is_none() {
                        self.buf0 = malloc(packed_room(rb) as u32)?;
                        self.buf1 = malloc(rb as u32)?;
                    }
                    let b = self.pm_i16(PM_BOTTOM, "a pixel map the file did not fill")? as i32;
                    let t = self.pm_i16(PM_TOP, "a pixel map the file did not fill")? as i32;
                    total = total.wrapping_add(b - t);
                    if o == 0x9a {
                        row = 0;
                    }
                    while row < total {
                        if rb > 0xfa {
                            bswap_word(&mut f, &mut count);
                        } else {
                            let mut one = [count_byte];
                            bswap_fread(&mut f, &mut one, 1, 1);
                            count_byte = one[0];
                            count = [count_byte, Some(0)];
                        }
                        let n = need_word(count, "a row's byte count the file did not fill (stale stack)")? as i64;
                        let got = f.read(n as usize);
                        if !got.is_empty() {
                            let b0 = block_mut(&mut self.buf0)?;
                            if got.len() > b0.len() {
                                return Err(TFault("a row's bytes read past the end of buf0"));
                            }
                            for (k, &x) in got.iter().enumerate() {
                                b0[k] = Some(x);
                            }
                        }
                        if o == 0x98 {
                            un_pack_bit_mem(&mut self.buf1, &self.buf0, n)?;
                            let mut x = 0i64;
                            while x < w && row >= 0 && row < height as i32 {
                                let ib = at(block(&self.buf1)?, x).ok_or(TFault("a colour index nobody set"))?;
                                let idx = ib as i8 as i64;
                                let cm = block(&clr)?;
                                let vals: Vec<Option<u8>> = (0..3).map(|c| at(cm, idx * 6 + 1 + 2 * c)).collect();
                                for (c, val) in vals.into_iter().enumerate() {
                                    put(block_mut(&mut pic.planes[c])?, w * row as i64 + x, val)?;
                                }
                                x += 1;
                            }
                        } else {
                            un_pack_bit_9a(&mut self.buf1, &self.buf0, n)?;
                            let mut first = 0i64;
                            let inside = row >= 0 && row < height as i32;
                            if cmp == 4 && row >= 0 && inside {
                                copy(&mut pic.planes[3], row as i64 * w, &self.buf1, 0, w)?;
                                first = 1;
                            }
                            if inside {
                                for c in 0..3 {
                                    copy(&mut pic.planes[c], row as i64 * w, &self.buf1, (first + c as i64) * span as i64, w)?;
                                }
                            }
                        }
                        row += 1;
                    }
                }
                1 => {
                    bswap_word(&mut f, &mut region);
                    let n = need_word(region, "a region size the file did not fill (stale stack)")? as i16 as i64;
                    f.seek_cur(n - 2);
                }
                0 | 0x1e => {}
                0xc00 => {
                    let mut b4 = [None; 4];
                    bswap_fread(&mut f, &mut b4, 4, 1);
                    let _ = f.read(16);
                    bswap_fread(&mut f, &mut b4, 4, 1);
                }
                0x8200 => {
                    out.picture = Some(pic);
                    refuse(&mut out, b"Sorry, cannot handle compressed files");
                    return Ok(out);
                }
                _ => out.messages.push((format!("Opcode 0x{o:x} is not supported!\n").into_bytes(), 2)),
            }
            if f.tell() & 1 != 0 {
                let _ = f.read(1);
            }
            if bswap_word(&mut f, &mut op) == 0 {
                op = [Some(0xff), Some(0)];
            }
        }
        self.buf1 = None;
        self.buf0 = None;
        out.picture = Some(pic);
        out.ret = 1;
        out.closed = true;
        Ok(out)
    }
}

/// [`PictState::read`] from the program's starting state.
pub fn read_pict(file: Option<&[u8]>, mac: i32) -> Result<ReadPict, TFault> {
    PictState::default().read(file, mac)
}

/// PackBits decoding into a block [soft 0x085e2d50 si_unPackBit], for bytes that may be
/// undefined (the rules of `si_picture::un_pack_bit`): a count byte below 0x80 copies the
/// next `count + 1` bytes, one above repeats the next byte `1 − count` times, and 0x80 is
/// copied as a data byte without being counted; the loop runs while the length left is above
/// 0. The destination's size is not looked at by the original: a write past it is a fault
/// here. Bytes read past the source block are undefined; a count byte among them is a fault.
fn un_pack_bit_mem(dst: &mut Blk, src: &Blk, length: i64) -> Result<(), TFault> {
    let mut left = length;
    let mut s = 0i64;
    let mut d = 0i64;
    while left > 0 {
        let c = at(block(src)?, s).ok_or(TFault("a PackBits count byte nobody set"))?;
        if c == 0x80 {
            put(block_mut(dst)?, d, Some(0x80))?;
            s += 1;
            d += 1;
        } else if c > 0x80 {
            let n = 1 - (c as i8 as i64);
            let v = at(block(src)?, s + 1);
            let m = block_mut(dst)?;
            for k in 0..n {
                put(m, d + k, v)?;
            }
            s += 2;
            d += n;
            left -= 2;
        } else {
            let n = c as i64 + 1;
            copy(dst, d, src, s + 1, n)?;
            d += n;
            s += 1 + n;
            left -= 1 + n;
        }
    }
    Ok(())
}

/// PackBits decoding of a `DirectBitsRect` row [soft 0x085d1bec unPackBit9a]: like
/// `si_unPackBit` except that a count byte of 0x80 is a run of 129 (every byte with the top
/// bit set repeats the next byte `1 − count` times). No destination size: a write past
/// `dst` is a fault here.
pub fn un_pack_bit_9a(dst: &mut Blk, src: &Blk, length: i64) -> Result<(), TFault> {
    let mut left = length;
    let mut s = 0i64;
    let mut d = 0i64;
    while left > 0 {
        let c = at(block(src)?, s).ok_or(TFault("a PackBits count byte nobody set"))? as i8;
        if c < 0 {
            let n = 1 - c as i64;
            let v = at(block(src)?, s + 1);
            let m = block_mut(dst)?;
            for k in 0..n {
                put(m, d + k, v)?;
            }
            s += 2;
            d += n;
            left -= 2;
        } else {
            let n = c as i64 + 1;
            copy(dst, d, src, s + 1, n)?;
            s += 1 + n;
            d += n;
            left -= 1 + n;
        }
    }
    Ok(())
}

/// A rectangle as four big-endian words [soft 0x085d1cac macRectWrite] (the original turns
/// the record's words, writes them and turns them back).
fn mac_rect_write(out: &mut Vec<u8>, r: [i16; 4]) {
    for v in r {
        out.extend_from_slice(&v.to_be_bytes());
    }
}

/// `fwrite` at a position of a file being written: overwrites, and extends at the end.
fn write_at(out: &mut Vec<u8>, pos: usize, b: &[u8]) {
    if out.len() < pos + b.len() {
        out.resize(pos + b.len(), 0);
    }
    out[pos..pos + b.len()].copy_from_slice(b);
}

/// Writes a picture as a Macintosh PICT file [soft 0x085d08ac SI_pictureWritePict]. `name` is
/// the name as given (the MacBinary header takes its last component); `mac` is 0x80 for a
/// MacBinary header (its value is the size the preamble grows by). The original opens the
/// name with `SI_softOpen` and the extension `.pict`; here the file's bytes are returned.
///
/// The file: `mac + 512` zero bytes; the size word (patched at the end with the low 16 bits
/// of the file's length) and the frame (0, 0, height, width); the version opcodes 0x11 and
/// 0x2ff; the header opcode 0xc00 with version −1, the 16.16 rectangle (0, 0, width, height)
/// and a reserved 0; a clip region (opcode 1, size 10, the frame); then opcode 0x9a with base
/// address 0xff and a pixel map of row bytes `4 × width` (top bit set), the frame, version 0,
/// pack type 4, 72 dpi, pixel type 16, 32 bits a pixel, 3 components (4 with alpha) of 8
/// bits, the frame twice and mode 0x40; then each row, first to last: alpha (when the
/// picture has it), red, green and blue, PackBits-coded (`si_packBit` (0x085e2bb0), as
/// `si_picture::pack_bit`), behind a byte count of one byte when the row bytes are at most
/// 250, else two; a zero byte to an even length; the end opcode 0xff.
///
/// With a MacBinary header (`mac` not 0) the first 128 bytes are then a header: the length
/// of the name's last component and the component itself from byte 2 (a component of more
/// than 63 bytes runs on into the fields behind it, which are written after it), type `PICT`
/// and creator `SOFT` at 0x41, flags 4 at 0x49, the length of the data (the file's length
/// less 128) big-endian at 0x53; and the file is padded with zeros to a whole number of
/// 128-byte blocks after the header.
///
/// Faults where the original's result is not defined: a preamble outside 0 … 640 bytes (the
/// stack buffer it is cleared in), a width of 0 with rows to write (the PackBits coder reads
/// in front of a null input), a negative width, a plane missing or too short, a coded row
/// longer than 32,767 bytes, and a name component that runs past the header's buffer and
/// the locals behind it.
pub fn write_pict(pic: &Picture, name: &[u8], mac: i32) -> Result<Vec<u8>, TFault> {
    let head = mac.wrapping_add(0x200);
    if !(0..=0x280).contains(&head) {
        return Err(TFault("a preamble cleared past its stack buffer"));
    }
    let w = pic.width;
    let h = pic.height;
    let frame = [0i16, 0, h, w];
    let mut out = vec![0u8; head as usize];
    out.extend_from_slice(&0u16.to_be_bytes());
    mac_rect_write(&mut out, frame);
    for v in [0x11u16, 0x2ff, 0xc00] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&0xffff_ffffu32.to_be_bytes());
    for v in [0i32, 0, (w as i32).wrapping_shl(16), (h as i32).wrapping_shl(16)] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&10u16.to_be_bytes());
    mac_rect_write(&mut out, frame);
    for v in [0x9au16, 0, 0xff] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let row_bytes = ((w as u16) << 2) | 0x8000;
    out.extend_from_slice(&row_bytes.to_be_bytes());
    mac_rect_write(&mut out, frame);
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0x0048_0000u32.to_be_bytes());
    out.extend_from_slice(&0x0048_0000u32.to_be_bytes());
    let alpha = pic.planes[3].is_some();
    for v in [0x10u16, 0x20, if alpha { 4 } else { 3 }, 8] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    for _ in 0..3 {
        out.extend_from_slice(&0u32.to_be_bytes());
    }
    mac_rect_write(&mut out, frame);
    mac_rect_write(&mut out, frame);
    out.extend_from_slice(&0x40u16.to_be_bytes());
    let n = if alpha { (w as i32) << 2 } else { w as i32 * 3 };
    if n < 0 {
        return Err(TFault("a negative row size (SI_malloc of nearly 4 GB)"));
    }
    let wide = (row_bytes & 0x7fff) as i16 > 0xfa;
    let wu = w.max(0) as usize;
    let order: &[usize] = if alpha { &[3, 0, 1, 2] } else { &[0, 1, 2] };
    let mut buf = Vec::with_capacity(n as usize);
    for r in 0..h.max(0) as usize {
        if n == 0 {
            return Err(TFault("a row of no bytes: si_packBit reads in front of a null input"));
        }
        buf.clear();
        for &c in order {
            let p = pic.planes[c].as_ref().ok_or(TFault("a plane the writer copies from is absent"))?;
            buf.extend_from_slice(p.get(r * wu..(r + 1) * wu).ok_or(TFault("a plane shorter than the picture"))?);
        }
        let packed = si_picture::pack_bit(&buf);
        if packed.len() > 0x7fff {
            return Err(TFault("a coded row longer than 32,767 bytes (fwrite of a negative count)"));
        }
        if wide {
            out.extend_from_slice(&(packed.len() as u16).to_be_bytes());
        } else {
            out.push(packed.len() as u8);
        }
        out.extend_from_slice(&packed);
    }
    if out.len() & 1 != 0 {
        out.push(0);
    }
    out.extend_from_slice(&0xffu16.to_be_bytes());
    let pic_size = out.len() as u16;
    write_at(&mut out, head as usize, &pic_size.to_be_bytes());
    if mac != 0 {
        let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
        let base = match name.iter().rposition(|&b| b == b'/') {
            Some(i) => &name[i + 1..],
            None => name,
        };
        // the header's buffer is 128 bytes, then 16 bytes of locals no longer used
        if 2 + base.len() + 1 > 0x90 {
            return Err(TFault("a name copied past the header's buffer and the locals behind it"));
        }
        let mut hdr = vec![0u8; 0x90];
        hdr[1] = base.len() as u8;
        hdr[2..2 + base.len()].copy_from_slice(base);
        hdr[2 + base.len()] = 0;
        hdr[0x41..0x45].copy_from_slice(b"PICT");
        hdr[0x45] = 0;
        hdr[0x45..0x49].copy_from_slice(b"SOFT");
        hdr[0x49] = 4;
        let data = (out.len() as u32).wrapping_sub(0x80);
        hdr[0x53..0x57].copy_from_slice(&data.to_be_bytes());
        write_at(&mut out, 0, &hdr[..0x80]);
        let pad = 0x80 - (data & 0x7f);
        if pad <= 0x7f {
            out.extend(std::iter::repeat(0).take(pad as usize));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------
// Fields and frame sequences
// ---------------------------------------------------------------------------------------

/// `SI_calloc(n, 1)` (0x080d0e94): no block for 0.
fn calloc(n: i32) -> Result<Option<Vec<u8>>, Fault> {
    if n == 0 {
        return Ok(None);
    }
    if n < 0 || n > 1 << 24 {
        return Err(Fault::BadSize);
    }
    Ok(Some(vec![0; n as usize]))
}

fn row<'a>(p: &'a Option<Vec<u8>>, at: usize, n: usize) -> Result<&'a [u8], Fault> {
    let p = p.as_ref().ok_or(Fault::NullPlane)?;
    p.get(at..at + n).ok_or(Fault::ShortPlane)
}

fn put_row(p: &mut Option<Vec<u8>>, at: usize, src: &[u8]) -> Result<(), Fault> {
    let p = p.as_mut().ok_or(Fault::NullPlane)?;
    p.get_mut(at..at + src.len()).ok_or(Fault::ShortPlane)?.copy_from_slice(src);
    Ok(())
}

/// What the field routines answer: the pictures they filled, or the message they showed
/// (level 3) and 0.
#[derive(Clone, Debug, PartialEq)]
pub enum Fields<T> {
    Done(T),
    Refused(&'static str),
}

/// Splits a full frame into its two fields [soft 0x085d258c SI_pictureFull2Field]: answers
/// (even, odd) as the routine fills its second and third arguments. A picture whose `fields`
/// is not 3 is refused with "Input picture should be full frame !". Both fields get the
/// frame's width, ratio and spare word, `(height + 1) / 2` rows, `fields` 2 and 1, and a
/// zeroed plane for each plane the frame has.
pub fn full_to_field(full: &Picture) -> Result<Fields<(Picture, Picture)>, Fault> {
    if full.fields != 3 {
        return Ok(Fields::Refused("Input picture should be full frame !\n"));
    }
    let n = full.width as i32;
    let h = full.height as i32;
    let fh = (h + 1) / 2;
    let size = n.wrapping_mul(fh);
    let blank = |fields: i16| Picture { width: full.width, height: fh as i16, ratio: full.ratio, fields, pad: full.pad, planes: [None, None, None, None] };
    let (mut even, mut odd) = (blank(2), blank(1));
    for c in 0..4 {
        if full.planes[c].is_some() {
            even.planes[c] = calloc(size)?;
            odd.planes[c] = calloc(size)?;
        }
    }
    let n = n.max(0) as usize;
    let mut k = 0i32;
    while k < h {
        let src = k as usize * n;
        let dst = (k / 2) as usize * n;
        for c in 0..4 {
            if full.planes[c].is_some() {
                let r = row(&full.planes[c], src, n)?.to_vec();
                let to = if k & 1 == 0 { &mut even } else { &mut odd };
                put_row(&mut to.planes[c], dst, &r)?;
            }
        }
        k += 1;
        if k == h && h % 2 == 1 {
            for c in 0..4 {
                if full.planes[c].is_some() {
                    let r = row(&full.planes[c], src, n)?.to_vec();
                    put_row(&mut odd.planes[c], dst, &r)?;
                }
            }
        }
    }
    Ok(Fields::Done((even, odd)))
}

/// Joins fields into a full frame [soft 0x085d2110 SI_pictureField2Full].
///
/// - Without a second picture (`b` `None`) the field `a` is doubled: each row twice (a full
///   frame is refused, "Input picture is a full-frame. Should be odd or even !").
/// - With two: they must have the same width and height ("Input pictures do not have the same
///   dimension !"), `fields` 1, 2 or 3 each ("Input pictures should be odd, even or full
///   frames !"), and be both full frames or one odd and one even ("Input pictures should be
///   both full or one odd and one even !").
/// - Two fields: the frame has twice the height; row `2r` is row `r` of the first field and
///   row `2r + 1` row `r` of the second, where `second_first` 0 puts the odd field first and
///   anything else the even one. Two full frames: the frame keeps the height and takes its
///   even rows from one frame and its odd rows from the other (`second_first` 0: even rows
///   from `b`).
///
/// The frame has `a`'s width, ratio and spare word, `fields` 3, and a zeroed plane for each
/// plane `a` has; the rows of a plane are taken from the same plane of both pictures.
pub fn field_to_full(a: &Picture, b: Option<&Picture>, second_first: i16) -> Result<Fields<Picture>, Fault> {
    let b = match b {
        None => {
            if a.fields == 3 {
                return Ok(Fields::Refused("Input picture is a full-frame. Should be odd or even !\n"));
            }
            a
        }
        Some(b) => {
            if a.width != b.width || a.height != b.height {
                return Ok(Fields::Refused("Input pictures do not have the same dimension !\n"));
            }
            let ok = |f: i16| matches!(f, 1..=3);
            if !ok(a.fields) || !ok(b.fields) {
                return Ok(Fields::Refused("Input pictures should be odd, even or full frames !\n"));
            }
            let (fa, fb) = (a.fields, b.fields);
            let fine = if fa == fb { fa == 3 } else { fa != 3 && fb != 3 };
            if !fine {
                return Ok(Fields::Refused("Input pictures should be both full or one odd and one even !\n"));
            }
            b
        }
    };
    let full = a.fields == 3 && b.fields == 3;
    let height = if full { a.height } else { a.height.wrapping_mul(2) };
    let mut out = Picture { width: a.width, height, ratio: a.ratio, fields: 3, pad: a.pad, planes: [None, None, None, None] };
    let n = out.width as i32;
    let size = (height as i32).wrapping_mul(n);
    for c in 0..4 {
        if a.planes[c].is_some() {
            out.planes[c] = calloc(size)?;
        }
    }
    let (first, second) = if full {
        if second_first != 0 {
            (a, b)
        } else {
            (b, a)
        }
    } else {
        let lead = if second_first == 0 { 1 } else { 2 };
        if a.fields == lead {
            (a, b)
        } else {
            (b, a)
        }
    };
    let n = n.max(0) as usize;
    for r in 0..(a.height as i32).max(0) as usize {
        let off = r * n;
        let d = if full { off } else { 2 * off };
        let gap = if full { 0 } else { n };
        for c in 0..4 {
            if a.planes[c].is_none() {
                continue;
            }
            if !full || r % 2 == 0 {
                let src = row(&first.planes[c], off, n)?.to_vec();
                put_row(&mut out.planes[c], d, &src)?;
            }
            if !full || r % 2 == 1 {
                let src = row(&second.planes[c], off, n)?.to_vec();
                put_row(&mut out.planes[c], d + gap, &src)?;
            }
        }
    }
    Ok(Fields::Done(out))
}

/// The state of [`next_frame`]: the statics of `SI_getNextFrameInSequence` (0x0882a2e0 …
/// and the name at 0x086fafb8).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sequence {
    pub first: i32,
    pub last: i32,
    pub step: i32,
    pub current: i32,
    pub kind: i32,
    pub cycle_pos: i32,
    pub cycle: i32,
    pub stereo: i16,
    pub just_primed: i16,
    pub name: Option<Vec<u8>>,
}

/// What [`next_frame`] answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// A null pointer: nothing more, or the sequence was set up.
    Null,
    /// One of the routine's own texts ("first > last").
    Text(&'static str),
    /// The caller's buffer, filled with the next name.
    Name(Vec<u8>),
}

/// The names of a sequence of frames, one call at a time
/// [soft 0x085d1d6c SI_getNextFrameInSequence].
///
/// Called with a name (`Some`), it sets the sequence up: frames `first` to `last` by `step`,
/// `kind` 3 for whole frames (any other value: fields, two a frame; 2 puts field 1 first,
/// anything else field 2), `stereo` not 0 for a left and a right picture each; a cycle is 1,
/// 2 or 4 names. It answers null, or "first > last". Called without a name and with `first`
/// −1 it starts the sequence again; otherwise it answers the next name in `out`, or null
/// after the last frame. Names: the base name, `_L` or `_R` for a stereo pair (a stereo
/// sequence of whole frames has a plain left name: `name.N` and `name_R.N`), `.N` the frame
/// number when the sequence starts above 0, and `.1` or `.2` for a field. Two of the
/// formats without a frame number take a field number the routine does not pass (the third
/// and fourth names of a stereo field sequence from frame 0): the name then holds whatever
/// the stack held, a [`Fault`] here, as is the "Coding error!" exit for a cycle position
/// above 3.
pub fn next_frame(s: &mut Sequence, name: Option<&[u8]>, first: i32, last: i32, step: i32, kind: i32, stereo: i16) -> Result<Frame, Fault> {
    if let Some(nm) = name {
        s.current = first;
        s.first = first;
        s.last = last;
        s.step = step;
        s.stereo = stereo;
        s.kind = kind;
        s.just_primed = 1;
        s.cycle = if stereo != 0 { 2 } else { 1 };
        if kind != 3 {
            s.cycle *= 2;
        }
        s.cycle_pos = 0;
        s.name = Some(nm.to_vec());
        if s.first > s.last {
            return Ok(Frame::Text("first > last"));
        }
        return Ok(Frame::Null);
    }
    if first == -1 {
        s.current = s.first;
        s.cycle_pos = 0;
        s.just_primed = 1;
        return Ok(Frame::Null);
    }
    if s.just_primed == 0 {
        let p = s.cycle_pos.wrapping_add(1);
        if s.cycle == 0 || (p == i32::MIN && s.cycle == -1) {
            return Err(Fault::DivideError);
        }
        s.cycle_pos = p % s.cycle;
        if s.cycle_pos == 0 {
            s.current = s.current.wrapping_add(s.step);
        }
    } else {
        s.just_primed = 0;
    }
    if s.current > s.last {
        return Ok(Frame::Null);
    }
    let base = s.name.clone().ok_or(Fault::Uninitialised)?;
    let (one, two): (&[u8], &[u8]) = if s.kind == 2 { (b"1", b"2") } else { (b"2", b"1") };
    let numbered = s.first > 0;
    let n = s.current.to_string().into_bytes();
    let mut out = base.clone();
    let mut add = |parts: &[&[u8]]| {
        for p in parts {
            out.extend_from_slice(p);
        }
    };
    let three = s.kind == 3;
    match s.cycle_pos {
        0 => {
            let side: &[u8] = if s.stereo != 0 { b"_L" } else { b"" };
            match (three, numbered) {
                (true, true) => add(&[side, b".", &n]),
                (true, false) => add(&[side]),
                (false, true) => add(&[side, b".", &n, b".", one]),
                (false, false) => add(&[side, b".", one]),
            }
        }
        1 => {
            if three {
                if numbered {
                    add(&[b"_R.", &n]);
                } else {
                    add(&[b"_R"]);
                }
            } else {
                let (side, f): (&[u8], &[u8]) = if s.stereo != 0 { (b"_R", one) } else { (b"", two) };
                if numbered {
                    add(&[side, b".", &n, b".", f]);
                } else {
                    add(&[side, b".", f]);
                }
            }
        }
        2 | 3 => {
            if !numbered {
                return Err(Fault::Uninitialised);
            }
            let side: &[u8] = if s.cycle_pos == 2 { b"_L" } else { b"_R" };
            add(&[side, b".", &n, b".", two]);
        }
        _ => return Err(Fault::Exit),
    }
    Ok(Frame::Name(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pic(w: i16, h: i16, fields: i16) -> Picture {
        let n = (w as usize) * (h as usize);
        Picture { width: w, height: h, ratio: 1.0, fields, pad: 0, planes: [Some((0..n as u8).collect()), None, None, None] }
    }

    #[test]
    fn split_and_join() {
        let f = pic(2, 3, 3);
        let Fields::Done((even, odd)) = full_to_field(&f).unwrap() else { panic!() };
        assert_eq!(even.planes[0].as_deref(), Some(&[0, 1, 4, 5][..]));
        assert_eq!(odd.planes[0].as_deref(), Some(&[2, 3, 4, 5][..]));
        assert_eq!((even.fields, odd.fields, even.height), (2, 1, 2));
        // odd field first when the flag is 0
        let Fields::Done(j) = field_to_full(&odd, Some(&even), 0).unwrap() else { panic!() };
        assert_eq!(j.planes[0].as_deref(), Some(&[2, 3, 0, 1, 4, 5, 4, 5][..]));
    }

    #[test]
    fn frame_names() {
        let mut s = Sequence::default();
        assert_eq!(next_frame(&mut s, Some(b"pic"), 1, 2, 1, 2, 0).unwrap(), Frame::Null);
        let names: Vec<Frame> = (0..5).map(|_| next_frame(&mut s, None, 0, 0, 0, 0, 0).unwrap()).collect();
        assert_eq!(names[0], Frame::Name(b"pic.1.1".to_vec()));
        assert_eq!(names[1], Frame::Name(b"pic.1.2".to_vec()));
        assert_eq!(names[2], Frame::Name(b"pic.2.1".to_vec()));
        assert_eq!(names[4], Frame::Null);
    }
}

// ---------------------------------------------------------------------------------------
// Line drawings (sixth pass)
// ---------------------------------------------------------------------------------------

/// Why [`lin_write_pict`] writes nothing (the original answers 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinPictError {
    /// The line file's header was refused (the message is on the status line).
    Header(crate::lin::LinError),
}

/// Turns a line drawing into a Macintosh PICT file [soft 0x085d1088 SI_linWritePict]
/// (`lin2pict`'s work). `lin` is the line file's bytes (the original opens the name with
/// `SI_softOpen`, extension `.lin`), `name` the output's name as given (the MacBinary header
/// takes its last component; the original opens it with the extension `.pict`, mode `wb`),
/// `width` the picture's width in pixels, `mac` 0x80 for a MacBinary header (0 for none).
///
/// The scales, in the x87 unit: with the bounds' widths `dx` (greatest x less least x) and
/// `dy`, `sx = width / dx` and the height `H = (int)(width / (dx / dy))`, `sy = H / dy`, both
/// scales stored as floats. The file: `mac + 512` zero bytes; the size word and the frame
/// (0, 0, `(int)(dy × sy + 0.5)`, `(int)(dx × sx + 0.5)`); the version opcodes 0x11 and
/// 0x2ff; the header opcode 0xc00 with version −1, the 16.16 rectangle (0, 0, width, H) and
/// a reserved 0; a clip region (opcode 1, size 10, the frame); then for each record of `n`
/// points (`n + 1` points read): opcode 0x20 (line) from point 0 to point 1, and opcode 0x21
/// (line from the pen) to each of points 2 … `n`, every point as `(int)((y − least y) × sy)`,
/// `(int)((x − least x) × sx)`, their low 16 bits; a record whose count byte is negative
/// reads no points and writes nothing (the next byte is taken as the next count); then a
/// zero byte to an even length, the end opcode 0xff, and the size word: the low 16 bits of
/// the **whole** file's length (as the picture writer). The MacBinary header as
/// [`write_pict`]'s.
///
/// Faults where the original's result is not defined: a file that ends without a count of 0
/// (the last count is read again and again: the loop never ends), a point taken from the
/// stack buffer before any record filled it, a preamble outside 0 … 640 bytes, a name
/// component that runs past the header's buffer and the locals behind it.
pub fn lin_write_pict(lin: &[u8], name: &[u8], width: i32, mac: i32) -> Result<Result<Vec<u8>, LinPictError>, TFault> {
    let mut s = si_file::Stream::new(lin);
    let hdr = match crate::lin::read_header(&mut s) {
        Ok(h) => h,
        Err(e) => return Ok(Err(LinPictError::Header(e))),
    };
    let head = mac.wrapping_add(0x200);
    if !(0..=0x280).contains(&head) {
        return Err(TFault("a preamble cleared past its stack buffer"));
    }
    let [x0, x1, y0, y1] = hdr.bounds;
    let a = F80::from(x1) - x0;
    let b = F80::from(y1) - y0;
    let q = a / b;
    let w = F80::from(width);
    let sx = w / a;
    let tall = (w / q).to_i32(Round::Zero);
    let sy = F80::from(tall) / b;
    let (sx, sy) = (sx.to_f32(), sy.to_f32());
    let bottom = (b * sy + 0.5f64).to_i32(Round::Zero) as i16;
    let right = (a * sx + 0.5f64).to_i32(Round::Zero) as i16;
    let frame = [0i16, 0, bottom, right];
    let mut out = vec![0u8; head as usize];
    out.extend_from_slice(&0u16.to_be_bytes());
    mac_rect_write(&mut out, frame);
    for v in [0x11u16, 0x2ff, 0xc00] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&0xffff_ffffu32.to_be_bytes());
    for v in [0i32, 0, width.wrapping_shl(16), tall.wrapping_shl(16)] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&10u16.to_be_bytes());
    mac_rect_write(&mut out, frame);
    // the points' stack buffer (2 + 512 floats), kept from record to record
    let mut pts: Vec<Option<f32>> = vec![None; 514];
    let point = |pts: &[Option<f32>], i: usize| -> Result<[i16; 2], TFault> {
        let (Some(x), Some(y)) = (pts[2 * i], pts[2 * i + 1]) else {
            return Err(TFault("a point the stack buffer held before any record filled it"));
        };
        let v = ((F80::from(y) - y0) * sy).to_i32(Round::Zero) as i16;
        let h = ((F80::from(x) - x0) * sx).to_i32(Round::Zero) as i16;
        Ok([v, h])
    };
    let mut count: i8 = {
        let mut c = [0u8; 1];
        if s.read(&mut c) == 0 {
            return Err(TFault("a line file without records: the count byte is a stack byte nobody set"));
        }
        c[0] as i8
    };
    while count != 0 {
        if count > 0 {
            crate::lin::fcoor2d_fread(&mut s, &mut pts, count as usize + 1);
            out.extend_from_slice(&0x20u16.to_be_bytes());
            for v in point(&pts, 0)?.into_iter().chain(point(&pts, 1)?) {
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
        let mut i = 2;
        while i <= count as i32 {
            out.extend_from_slice(&0x21u16.to_be_bytes());
            for v in point(&pts, i as usize)? {
                out.extend_from_slice(&v.to_be_bytes());
            }
            i += 1;
        }
        let mut c = [0u8; 1];
        if s.read(&mut c) == 0 {
            return Err(TFault("a line file without a closing count of 0: the last count is read again forever"));
        }
        count = c[0] as i8;
    }
    if out.len() & 1 != 0 {
        out.push(0);
    }
    out.extend_from_slice(&0xffu16.to_be_bytes());
    let pic_size = out.len() as u16;
    write_at(&mut out, head as usize, &pic_size.to_be_bytes());
    if mac != 0 {
        let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
        let base = match name.iter().rposition(|&b| b == b'/') {
            Some(i) => &name[i + 1..],
            None => name,
        };
        // the header's 128 bytes, then locals no longer used up to the saved registers
        if 2 + base.len() + 1 > 0x908 {
            return Err(TFault("a name copied past the header's buffer and the locals behind it"));
        }
        let mut hdr = vec![0u8; 0x908];
        hdr[1] = base.len() as u8;
        hdr[2..2 + base.len()].copy_from_slice(base);
        hdr[0x41..0x45].copy_from_slice(b"PICT");
        hdr[0x45..0x49].copy_from_slice(b"SOFT");
        hdr[0x49] = 4;
        let data = (out.len() as u32).wrapping_sub(0x80);
        hdr[0x53..0x57].copy_from_slice(&data.to_be_bytes());
        write_at(&mut out, 0, &hdr[..0x80]);
        let pad = 0x80 - (data & 0x7f);
        if pad <= 0x7f {
            out.extend(std::iter::repeat(0).take(pad as usize));
        }
    }
    Ok(Ok(out))
}
