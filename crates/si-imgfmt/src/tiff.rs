// SPDX-License-Identifier: MIT
//! TIFF files (`.tif`) (finding 0014, "TIFF").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `tiff.c` (0x085dec70–0x085e2b14).
//!
//! The reader is a small TIFF reader of the program's own (not libtiff): one image file
//! directory a call, strips, no compression or PackBits; a fax (CCITT run-length) decoder
//! built from two text tables, and an LZW decoder the reader refuses to call. The writer
//! writes big-endian files of three 8-bit channels, chunky or one plane after another, raw or
//! PackBits. What this build does of its own, all reproduced (finding 0014 has the details):
//!
//! - **Little-endian files are refused.** After a value has been read and turned from
//!   big-endian into the machine's order, the reader turns it once more by hand for a file
//!   not marked `MM`, wrongly: element `i` is taken at byte offset `i`, and the last byte it
//!   moves is the one behind its scratch block. The magic number of an `II` file comes out as
//!   its first byte times 256 plus that stray byte, never 42 unless the first byte is 0.
//! - Samples are read through `signed char`: a byte of 0x80 or more spreads ones into every
//!   bit above it, and when a sample ends on a byte boundary the top bit of the **next** byte
//!   decides whether the sample is all ones. For an 8-bit sample the pixel is 0xff whenever
//!   the following byte is 0x80 or more.
//! - LZW (compression 5) is refused after the directory is read and the planes allocated,
//!   before any strip is read; the file is left open.
//! - The fax decoder never ends: its bit mask is a `signed char` shifted right, so after its
//!   first call it never comes back to zero and the end of a strip is never seen.
//! - A compression the reader does not know (anything but 1, 2, 5 and 0x8005) is reported and
//!   then decoded from a null buffer: the program dies.
//!
//! **Model.** The original keeps its whole state in globals of the file, and some of it
//! survives from one call to the next. [`Tiff`] holds them; [`Tiff::default`] is their state
//! when the program starts. Heap blocks are kept as bytes that may be undefined (`None`):
//! what the allocator returned and nobody wrote. A read outside a block gives an undefined
//! byte; a write outside one, a read through a null pointer, a branch on an undefined value,
//! or a routine that never returns gives a [`Fault`] (with the reason) instead of a guess.
//! Undefined values that only flow into the picture make undefined pixels, not a fault.

use si_x87::{Round, F80};

/// A case in which the original's result is not defined, or in which it never returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault(pub &'static str);

/// Bytes of a heap block; `None` for a byte nobody wrote.
pub type Mem = Vec<Option<u8>>;

/// A pointer: null, or a block.
pub type Blk = Option<Mem>;

/// The largest block the port hands out. The original asks `malloc` for whatever a file says;
/// above this the answer depends on the machine, and the port refuses.
pub const ALLOC_LIMIT: u32 = 1 << 22;

/// `SI_malloc` (0x080d0ee8) on a block whose content is followed byte by byte: 0 gives a null
/// pointer, otherwise the first byte is 0x6f and the last 0x6b, the rest unknown.
pub fn malloc(n: u32) -> Result<Blk, Fault> {
    if n == 0 {
        return Ok(None);
    }
    if n > ALLOC_LIMIT {
        return Err(Fault("a block larger than the port hands out"));
    }
    let mut v = vec![None; n as usize];
    v[0] = Some(0x6f);
    v[n as usize - 1] = Some(0x6b);
    Ok(Some(v))
}

/// `SI_realloc` (0x080d0f28): a size of 0 changes nothing; otherwise `realloc`, which keeps
/// the first bytes and grows the block with unknown ones (from a null pointer: a plain
/// `malloc`, without the allocator's two marks).
pub fn realloc(b: Blk, n: u32) -> Result<Blk, Fault> {
    if n == 0 {
        return Ok(b);
    }
    if n > ALLOC_LIMIT {
        return Err(Fault("a block larger than the port hands out"));
    }
    let mut v = b.unwrap_or_default();
    v.resize(n as usize, None);
    Ok(Some(v))
}

/// A byte of a block at a signed index: undefined outside the block.
fn at(m: &[Option<u8>], i: i64) -> Option<u8> {
    if i < 0 || i as usize >= m.len() {
        None
    } else {
        m[i as usize]
    }
}

fn need<T>(v: Option<T>, why: &'static str) -> Result<T, Fault> {
    v.ok_or(Fault(why))
}

fn le<const N: usize>(m: &[Option<u8>], off: i64) -> Option<[u8; N]> {
    let mut out = [0u8; N];
    for (k, o) in out.iter_mut().enumerate() {
        *o = at(m, off + k as i64)?;
    }
    Some(out)
}

fn i16_at(m: &[Option<u8>], i: i64) -> Option<i16> {
    le::<2>(m, 2 * i).map(i16::from_le_bytes)
}

fn i32_at(m: &[Option<u8>], i: i64) -> Option<i32> {
    le::<4>(m, 4 * i).map(i32::from_le_bytes)
}

fn put(m: &mut [Option<u8>], off: i64, b: &[u8]) -> Result<(), Fault> {
    if off < 0 || off as usize + b.len() > m.len() {
        return Err(Fault("a write outside a heap block"));
    }
    for (k, &v) in b.iter().enumerate() {
        m[off as usize + k] = Some(v);
    }
    Ok(())
}

fn put_opt(m: &mut [Option<u8>], off: i64, v: Option<u8>) -> Result<(), Fault> {
    if off < 0 || off as usize >= m.len() {
        return Err(Fault("a write outside a heap block"));
    }
    m[off as usize] = v;
    Ok(())
}

fn block<'a>(m: &'a Blk) -> Result<&'a Mem, Fault> {
    m.as_ref().ok_or(Fault("a read through a null pointer"))
}

fn block_mut<'a>(m: &'a mut Blk) -> Result<&'a mut Mem, Fault> {
    m.as_mut().ok_or(Fault("a write through a null pointer"))
}

/// Known bytes as a block.
pub fn known(b: &[u8]) -> Mem {
    b.iter().map(|&v| Some(v)).collect()
}

/// A `FILE *` on a file's bytes, as the C library's stream behaves for these routines:
/// `fseek` may go past the end (a negative position fails and changes nothing), `fread`
/// gives what is there, `ftell` the position.
#[derive(Clone, Debug)]
pub struct CFile<'a> {
    pub data: &'a [u8],
    pub pos: i64,
}

impl<'a> CFile<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        CFile { data, pos: 0 }
    }

    /// `fseek(f, off, SEEK_SET)`
    pub fn seek(&mut self, off: i32) {
        if off >= 0 {
            self.pos = off as i64;
        }
    }

    pub fn tell(&self) -> i32 {
        self.pos as i32
    }

    /// `fread` of up to `n` bytes: the bytes there are.
    pub fn read(&mut self, n: u64) -> &'a [u8] {
        let from = (self.pos as usize).min(self.data.len());
        let to = (from as u64).saturating_add(n).min(self.data.len() as u64) as usize;
        self.pos = self.pos.max(from as i64) + (to - from) as i64;
        &self.data[from..to]
    }
}

/// `fread(dst, size, count, f)`: the bytes there are go to the front of `dst` (a partial
/// last element included); answers the whole elements read.
fn fread(f: &mut CFile, dst: &mut [Option<u8>], size: u32, count: u32) -> Result<u32, Fault> {
    let want = size as u64 * count as u64;
    let got = f.read(want);
    if got.len() > dst.len() {
        return Err(Fault("fread writes past its buffer"));
    }
    for (k, &b) in got.iter().enumerate() {
        dst[k] = Some(b);
    }
    Ok(if size == 0 { 0 } else { (got.len() as u64 / size as u64) as u32 })
}

/// What a routine showed or printed, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Log {
    /// `SI_statusDisplayMessage(text, level)`.
    pub status: Vec<(Vec<u8>, i16)>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Offsets in [`Tiff::area`], the block of globals from `mask` (0x0882f4e0) to the end of
/// `realWidth` (0x0882f780), kept as the bytes they are because messages run over them.
pub const A_MASK: usize = 0x00;
pub const A_MSG: usize = 0x20;
pub const A_NB_ENTRY: usize = 0x70;
pub const A_OUTBUF: usize = 0x80;
pub const OUTBUF_SIZE: usize = 0x208;
pub const A_PHOTO: usize = 0x288;
pub const A_POS: usize = 0x28c;
pub const A_REAL_WIDTH: usize = 0x290;
pub const AREA: usize = 0x2a0;
/// Address of the area's first byte.
pub const AREA_ADDR: u32 = 0x0882_f4e0;

/// The globals of `tiff.c` the reader and the writer use. [`Default`] is their state when
/// the program starts (initialised data 0x086fbee0…, the rest zero).
#[derive(Clone, Debug, PartialEq)]
pub struct Tiff {
    pub bits_per_sample: Blk,
    /// The strip being decoded by the fax and LZW decoders; the writer's plane pointer.
    pub cinput: Blk,
    pub color_map: Blk,
    pub compression: u16,
    pub coutput: Blk,
    pub plan_config: i16,
    pub rows_per_strip: i32,
    pub samples_per_pix: i16,
    pub strip_byte_cnt: Blk,
    pub strip_offset: Blk,
    pub cinput_pos: i32,
    /// `IFD_offset`: where the next directory is; undefined when the file ended before it.
    pub ifd_offset: Option<i32>,
    pub file_pos: i32,
    pub bit_pos: i32,
    pub cur_byte: i32,
    pub lmask: u8,
    pub coutput_pos: i32,
    pub first_bit: i32,
    pub last_bit: i32,
    pub i_269: i32,
    pub first_byte: i32,
    pub last_byte: i32,
    /// The last sample `extract` made (bits that depend on undefined bytes are unknown).
    pub ret_val: Bits,
    pub byte_order: i32,
    pub cinput_len: i32,
    pub color_len: i32,
    pub coutput_len: i32,
    pub img_length: i32,
    pub img_width: i32,
    pub area: Vec<u8>,
    pub strips_per_img: i32,
    pub width_adj: i32,
    /// `xRes`, `yRes`: two words each, as bytes (a short read changes only some).
    pub x_res: [Option<u8>; 8],
    pub y_res: [Option<u8>; 8],
    /// `codeTable` (0x0882a480): 208 entries of 20 bytes, a run length (after loading: the
    /// code's length) and the code as text.
    pub code_table: Mem,
    /// `whiteTree` (0x08833788), `blackTree` (0x0882a450): node 0 is the static root.
    pub white_tree: CTree,
    pub black_tree: CTree,
    /// The LZW decoder's state: `stringTbl` (0x0882f780) and `lengthTbl` (0x0882b4e0), 4096
    /// entries each; `stringTree`, an index into `lzw_nodes` (`None`: null); `codeLen`,
    /// `code`, `word1`, `word2`, `old_code`, `out_str` and its size `str_len`, `out_len`,
    /// the loop counter `i_260`. `nbEntry` and `mask` are in [`Tiff::area`].
    pub string_tbl: Vec<Blk>,
    pub length_tbl: Vec<u32>,
    pub string_tree: Option<usize>,
    pub lzw_nodes: Vec<LNode>,
    pub code_len: i32,
    pub code: u16,
    pub word1: u16,
    pub word2: u16,
    pub old_code: u16,
    pub out_str: Blk,
    pub str_len: u32,
    pub out_len: u32,
    pub i_260: u32,
    pub log: Log,
}

/// A node of the LZW string tree (`lzw_node`, 0x14 bytes: the entry number, 0x1000 for none,
/// and four children, one for each two-bit group of a byte).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LNode {
    pub value: u32,
    pub kids: [Option<usize>; 4],
    pub freed: bool,
}

/// Entries of the LZW tables.
pub const LZW_TABLE: usize = 0x1000;

/// Bytes of [`Tiff::code_table`].
pub const CODE_TABLE: usize = 0xd0 * 20;

/// A node of a fax code tree (`ccitt_node`, 12 bytes: a run length and the two children, for
/// the bits 0 and 1); a node made by the loader starts with the value 0x80000000.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CNode {
    pub value: i32,
    pub kids: [Option<usize>; 2],
}

/// A fax code tree: node 0 is the root, which lives in the original's static data; the
/// others came from the heap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CTree {
    pub nodes: Vec<CNode>,
}

impl Default for CTree {
    fn default() -> Self {
        CTree { nodes: vec![CNode { value: 0, kids: [None, None] }] }
    }
}

impl CTree {
    /// The nodes depth first, kid 0 before kid 1: (value, has kid 0, has kid 1), the form the
    /// checks compare.
    pub fn preorder(&self) -> Vec<(i32, bool, bool)> {
        let mut out = Vec::new();
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let nd = &self.nodes[n];
            out.push((nd.value, nd.kids[0].is_some(), nd.kids[1].is_some()));
            if let Some(k) = nd.kids[1] {
                stack.push(k);
            }
            if let Some(k) = nd.kids[0] {
                stack.push(k);
            }
        }
        out
    }

    /// The order in which `treeFreeCcitt` [soft 0x085e2ad0 treeFreeCcitt] frees the nodes from
    /// `n` down: kid 0's subtree, kid 1's subtree, the node.
    pub fn free_order(&self, n: usize) -> Vec<usize> {
        let mut out = Vec::new();
        for k in self.nodes[n].kids.into_iter().flatten() {
            out.extend(self.free_order(k));
        }
        out.push(n);
        out
    }
}

impl Default for Tiff {
    fn default() -> Self {
        Tiff {
            bits_per_sample: None,
            cinput: None,
            color_map: None,
            compression: 1,
            coutput: None,
            plan_config: 2,
            rows_per_strip: -1,
            samples_per_pix: 1,
            strip_byte_cnt: None,
            strip_offset: None,
            cinput_pos: 2,
            ifd_offset: Some(0),
            file_pos: 0,
            bit_pos: 0,
            cur_byte: 0,
            lmask: 0,
            coutput_pos: 0,
            first_bit: 0,
            last_bit: 0,
            i_269: 0,
            first_byte: 0,
            last_byte: 0,
            ret_val: Bits::known(0),
            byte_order: 0,
            cinput_len: 0,
            color_len: 0,
            coutput_len: 0,
            img_length: 0,
            img_width: 0,
            area: vec![0; AREA],
            strips_per_img: 0,
            width_adj: 0,
            x_res: [Some(0); 8],
            y_res: [Some(0); 8],
            code_table: vec![Some(0); CODE_TABLE],
            white_tree: CTree::default(),
            black_tree: CTree::default(),
            string_tbl: vec![None; LZW_TABLE],
            length_tbl: vec![0; LZW_TABLE],
            string_tree: None,
            lzw_nodes: Vec::new(),
            code_len: 0,
            code: 0,
            word1: 0,
            word2: 0,
            old_code: 0,
            out_str: None,
            str_len: 0,
            out_len: 0,
            i_260: 0,
            log: Log::default(),
        }
    }
}

/// A 32-bit value some of whose bits are unknown (`u`: the unknown bits).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bits {
    pub v: u32,
    pub u: u32,
}

impl Bits {
    pub fn known(v: i32) -> Bits {
        Bits { v: v as u32, u: 0 }
    }

    /// A `signed char` loaded with `movsx`.
    fn sx(b: Option<u8>) -> Bits {
        match b {
            Some(b) => Bits::known(b as i8 as i32),
            None => Bits { v: 0, u: u32::MAX },
        }
    }

    pub fn value(self) -> Option<i32> {
        (self.u == 0).then_some(self.v as i32)
    }

    fn or(self, o: Bits) -> Bits {
        let one = (self.v & !self.u) | (o.v & !o.u);
        let u = (self.u | o.u) & !one;
        Bits { v: one, u }
    }

    fn shl(self, n: u32) -> Bits {
        let n = n & 31;
        Bits { v: self.v << n, u: self.u << n }
    }

    fn sar(self, n: u32) -> Bits {
        let n = n & 31;
        Bits { v: ((self.v as i32) >> n) as u32, u: ((self.u as i32) >> n) as u32 }
    }
}

/// `power(base, exponent)` [soft 0x085e0384 power]: `base` multiplied into 1, `exponent`
/// times (none for an exponent of 0 or below), in 32-bit arithmetic.
pub fn power(base: i32, exponent: i32) -> i32 {
    let mut r = 1i32;
    let mut e = exponent;
    while e > 0 {
        r = r.wrapping_mul(base);
        e -= 1;
    }
    r
}

/// The four plane pointers of an `SI_Picture` the reader fills, and the fields it writes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TiffPicture {
    pub width: i16,
    pub height: i16,
    /// The ratio's bits.
    pub ratio: u32,
    pub fields: i16,
    pub planes: [Blk; 4],
}

/// What [`Tiff::read`] leaves: the answer and, when it got that far, the picture record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadTiff {
    pub ret: Option<i32>,
    pub picture: Option<TiffPicture>,
}

impl Tiff {
    fn area_u32(&self, off: usize) -> u32 {
        u32::from_le_bytes(self.area[off..off + 4].try_into().unwrap())
    }

    fn set_area_u32(&mut self, off: usize, v: u32) {
        self.area[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    pub fn photo_inter(&self) -> i16 {
        i16::from_le_bytes([self.area[A_PHOTO], self.area[A_PHOTO + 1]])
    }

    pub fn set_photo_inter(&mut self, v: i16) {
        self.area[A_PHOTO..A_PHOTO + 2].copy_from_slice(&v.to_le_bytes());
    }

    pub fn pos(&self) -> u32 {
        self.area_u32(A_POS)
    }

    pub fn set_pos(&mut self, v: u32) {
        self.set_area_u32(A_POS, v)
    }

    pub fn real_width(&self) -> i32 {
        self.area_u32(A_REAL_WIDTH) as i32
    }

    pub fn set_real_width(&mut self, v: i32) {
        self.set_area_u32(A_REAL_WIDTH, v as u32)
    }

    /// `sprintf(msg, …)` and `SI_statusDisplayMessage(msg, level)` (0x083cca94): the text and
    /// its NUL go into the area from `msg` on, over whatever lies behind the 80 bytes of
    /// `msg` when it is longer.
    fn show(&mut self, text: &[u8], level: i16) -> Result<(), Fault> {
        self.sprintf_msg(text)?;
        self.log.status.push((text.to_vec(), level));
        Ok(())
    }

    fn sprintf_msg(&mut self, text: &[u8]) -> Result<(), Fault> {
        if A_MSG + text.len() + 1 > AREA {
            return Err(Fault("a message that runs past the variables behind msg"));
        }
        self.area[A_MSG..A_MSG + text.len()].copy_from_slice(text);
        self.area[A_MSG + text.len()] = 0;
        Ok(())
    }

    /// Reads `count` elements of `size` bytes into `dst` [soft 0x085e12a8 myRead]: at the
    /// file offset `off` (plus 0x80 with a MacBinary header) and back to where the file was,
    /// or, for an offset of 0, where the file is. The whole elements read are turned from
    /// big-endian into the machine's order (`SI_byteswap_fread` (0x083f9ba0)); for a file
    /// not marked `MM` the routine then turns `count` elements once more, by hand and
    /// wrongly: element `i` is taken at byte offset `i` (not `i × size`) into a scratch
    /// block of `size` bytes, and written back from the scratch block's bytes `size` down to
    /// 1, of which byte `size` is the one behind the block (unknown). Answers the whole
    /// elements read.
    pub fn my_read(&mut self, f: &mut CFile, dst: &mut [Option<u8>], size: i32, count: i32, off: i32, mac: i32) -> Result<i32, Fault> {
        let mut off = off;
        let mut back = None;
        if off != 0 {
            if mac != 0 {
                off = off.wrapping_add(0x80);
            }
            if off != 0 {
                back = Some(f.tell());
                f.seek(off);
            }
        }
        let n = fread(f, dst, size as u32, count as u32)?;
        if matches!(size, 2 | 4 | 8) {
            for k in 0..n as usize {
                dst[k * size as usize..(k + 1) * size as usize].reverse();
            }
        }
        if self.byte_order != 0 && size > 1 {
            if size as u32 > ALLOC_LIMIT {
                return Err(Fault("a block larger than the port hands out"));
            }
            let s = size as usize;
            for i in 0..count.max(0) as usize {
                if i + s > dst.len() {
                    return Err(Fault("myRead's second swap reaches past its buffer"));
                }
                let mut tmp: Vec<Option<u8>> = dst[i..i + s].to_vec();
                tmp.push(None); // the byte behind the scratch block
                for k in 0..s {
                    dst[i + k] = tmp[s - k];
                }
            }
        }
        if let Some(p) = back {
            f.seek(p);
        }
        Ok(n as i32)
    }

    /// Reads one image file directory [soft 0x085e0974 ifdread]: a 16-bit count of entries,
    /// each a tag, a type, a count and a value (2, 2, 4, 4 bytes) handed to
    /// [`Tiff::entry_exec`], then the offset of the next directory, which is returned (plus
    /// 0x80 with a MacBinary header, when it is not 0). A field the file ends before keeps
    /// what the entry before it put there (undefined in the first entry); the offset of the
    /// next directory is then undefined (`None`).
    pub fn ifd_read(&mut self, f: &mut CFile, mac: i32) -> Result<Option<i32>, Fault> {
        let mut n = [None; 2];
        self.my_read(f, &mut n, 2, 1, 0, mac)?;
        let n = need(le::<2>(&n, 0), "the directory's entry count is a value nobody set")?;
        let n = i16::from_le_bytes(n);
        let (mut tag, mut ty, mut count, mut value) = ([None; 2], [None; 2], [None; 4], [None; 4]);
        let mut i = 0;
        while i < n as i32 {
            self.my_read(f, &mut tag, 2, 1, 0, mac)?;
            self.my_read(f, &mut ty, 2, 1, 0, mac)?;
            self.my_read(f, &mut count, 4, 1, 0, mac)?;
            self.my_read(f, &mut value, 4, 1, 0, mac)?;
            let t = i16::from_le_bytes(need(le::<2>(&tag, 0), "a tag nobody set")?);
            self.entry_exec(f, t, Arg(ty), Arg32(count), Arg32(value), mac)?;
            i += 1;
        }
        let mut next = [None; 4];
        self.my_read(f, &mut next, 4, 1, 0, mac)?;
        Ok(le::<4>(&next, 0).map(|b| {
            let v = i32::from_le_bytes(b);
            if v != 0 && mac != 0 {
                v.wrapping_add(0x80)
            } else {
                v
            }
        }))
    }
}

/// A 16-bit argument whose bytes may be unknown; read only where the routine uses it.
#[derive(Clone, Copy, Debug)]
pub struct Arg(pub [Option<u8>; 2]);

/// A 32-bit argument whose bytes may be unknown.
#[derive(Clone, Copy, Debug)]
pub struct Arg32(pub [Option<u8>; 4]);

impl Arg {
    pub fn known(v: i16) -> Self {
        Arg(v.to_le_bytes().map(Some))
    }
    fn get(self) -> Result<i16, Fault> {
        Ok(i16::from_le_bytes(need(le::<2>(&self.0, 0), "an entry's type nobody set")?))
    }
}

impl Arg32 {
    pub fn known(v: i32) -> Self {
        Arg32(v.to_le_bytes().map(Some))
    }
    fn get(self) -> Result<i32, Fault> {
        Ok(i32::from_le_bytes(need(le::<4>(&self.0, 0), "an entry's count or value nobody set")?))
    }
}

impl Tiff {
    /// Executes one directory entry [soft 0x085e0a6c entryExec]. Before anything, the
    /// bits-per-sample array is made (one entry, 1) when there is none. Then, by tag:
    ///
    /// - 0x100, 0x101, 0x116 (width, length, rows a strip): a SHORT takes the value's high
    ///   half, sign-extended; a LONG the value; another type prints two lines on standard
    ///   output and closes the file, which the reader then goes on reading (a [`Fault`]);
    /// - 0x103, 0x106, 0x115, 0x11c (compression, photometric interpretation, samples a
    ///   pixel, planar configuration): the high half, whatever the type;
    /// - 0x102 (bits a sample): with a count of 1 the first entry takes the high half;
    ///   otherwise the array is grown to `count` entries and read as 16-bit values;
    /// - 0x111, 0x117 (strip offsets and byte counts): an array of `count` words; one comes
    ///   from the value (for 0x111 as SHORT: `(value + mac) >> 16`, the MacBinary offset
    ///   added before the shift), more are read as SHORT (sign-extended) or LONG values; the
    ///   offsets get the MacBinary offset added; another type prints two lines (0x111 into
    ///   the TIFF file itself, 0x117 on standard error) and closes the file;
    /// - 0x140 (colour map): `count` 16-bit entries, read, or one set from the high half;
    /// - 0x11a, 0x11b (resolutions): two words read into the global pair;
    /// - 0x10d, 0x10e, 0x10f, 0x110, 0x11d, 0x131, 0x132, 0x13b, 0x13c (texts), 0x11e,
    ///   0x11f (positions), 0x123, 0x12d: read into a scratch block and dropped. Every
    ///   value is taken as an offset, even where the data would fit in it; a read with an
    ///   offset of 0 takes the bytes at the file's position, inside the directory.
    /// - 0xff, 0x107 … 0x10a, 0x112, 0x118, 0x119, 0x120, 0x121: "Tag %d is not supported!"
    ///   at level 2;
    /// - anything else: nothing.
    ///
    /// Returns 1 (0 after closing the file, which is a [`Fault`] here).
    pub fn entry_exec(&mut self, f: &mut CFile, tag: i16, ty: Arg, count: Arg32, value: Arg32, mac: i32) -> Result<i32, Fault> {
        if self.bits_per_sample.is_none() {
            let mut b = malloc(2)?.unwrap();
            put(&mut b, 0, &1u16.to_le_bytes())?;
            self.bits_per_sample = Some(b);
        }
        let t = (tag as u16).wrapping_sub(0xfe) as i16 as i32;
        if !(0..=0x42).contains(&t) {
            return Ok(1);
        }
        let high = |v: Arg32| -> Result<i32, Fault> { Ok(v.get()? >> 16) };
        match tag as u16 {
            0xff | 0x107 | 0x108 | 0x109 | 0x10a | 0x112 | 0x118 | 0x119 | 0x120 | 0x121 => {
                let text = format!("Tag {} is not supported!\n", tag);
                self.show(text.as_bytes(), 2)?;
            }
            0x100 | 0x101 | 0x116 => {
                let v = match ty.get()? {
                    3 => high(value)?,
                    4 => value.get()?,
                    _ => {
                        let what = match tag {
                            0x100 => "img_width",
                            0x101 => "img_length",
                            _ => "rows_per_strip",
                        };
                        self.log.stdout.extend_from_slice(b"Case error in IFDexec(): ");
                        self.log.stdout.extend_from_slice(format!("unexpected data type for {what}!\n").as_bytes());
                        return Err(Fault("entryExec closes the file the reader goes on reading"));
                    }
                };
                match tag {
                    0x100 => self.img_width = v,
                    0x101 => self.img_length = v,
                    _ => self.rows_per_strip = v,
                }
            }
            0x102 => {
                let n = count.get()?;
                if n == 1 {
                    let v = high(value)? as u16;
                    put(block_mut(&mut self.bits_per_sample)?, 0, &v.to_le_bytes())?;
                } else {
                    let b = realloc(self.bits_per_sample.take(), (n as u32).wrapping_mul(2))?;
                    self.bits_per_sample = b;
                    let mut b = self.bits_per_sample.take();
                    let r = self.read_into(f, &mut b, 2, n, value, mac);
                    self.bits_per_sample = b;
                    r?;
                }
            }
            0x103 => self.compression = high(value)? as u16,
            0x106 => self.set_photo_inter(high(value)? as i16),
            0x115 => self.samples_per_pix = high(value)? as i16,
            0x11c => self.plan_config = high(value)? as i16,
            0x140 | 0x123 | 0x12d => {
                let n = count.get()?;
                let mut b = malloc((n as u32).wrapping_mul(2))?;
                if n == 1 {
                    let v = high(value)? as u16;
                    put(block_mut(&mut b)?, 0, &v.to_le_bytes())?;
                } else {
                    self.read_into(f, &mut b, 2, n, value, mac)?;
                }
                if tag == 0x140 {
                    self.color_map = b;
                }
            }
            0x10d | 0x10e | 0x10f | 0x110 | 0x11d | 0x131 | 0x132 | 0x13b | 0x13c => {
                let n = count.get()?;
                let mut b = malloc(n as u32)?;
                self.read_into(f, &mut b, 1, n, value, mac)?;
            }
            0x11a | 0x11b => {
                let mut b: Mem = if tag == 0x11a { self.x_res.to_vec() } else { self.y_res.to_vec() };
                self.my_read(f, &mut b, 4, 2, value.get()?, mac)?;
                let b: [Option<u8>; 8] = b.try_into().unwrap();
                if tag == 0x11a {
                    self.x_res = b;
                } else {
                    self.y_res = b;
                }
            }
            0x11e | 0x11f => {
                let mut b = vec![None; 8];
                self.my_read(f, &mut b, 4, 2, value.get()?, mac)?;
            }
            0x111 | 0x117 => {
                let ty = ty.get()?;
                if ty != 3 && ty != 4 {
                    if tag == 0x117 {
                        self.log.stderr.extend_from_slice(b"Case error in IFDexec(): ");
                        self.log.stderr.extend_from_slice(b"unexpected data type for strip_byte_cnt!\n");
                    }
                    // 0x111 prints its two lines into the TIFF file, opened for reading
                    return Err(Fault("entryExec closes the file the reader goes on reading"));
                }
                let n = count.get()?;
                let mut arr = malloc((n as u32).wrapping_mul(4))?;
                let add = if tag == 0x111 { mac } else { 0 };
                if n == 1 {
                    let v = if ty == 3 {
                        if tag == 0x111 {
                            mac.wrapping_add(value.get()?) >> 16
                        } else {
                            high(value)?
                        }
                    } else {
                        value.get()?.wrapping_add(add)
                    };
                    put(block_mut(&mut arr)?, 0, &v.to_le_bytes())?;
                } else {
                    let size = if ty == 3 { 2 } else { 4 };
                    let mut tmp = malloc((n as u32).wrapping_mul(size as u32))?;
                    self.read_into(f, &mut tmp, size, n, value, mac)?;
                    for i in 0..n.max(0) as i64 {
                        let t = block(&tmp)?;
                        let v = if size == 2 { i16_at(t, i).map(|v| v as i32) } else { i32_at(t, i) };
                        let a = block_mut(&mut arr)?;
                        match v {
                            Some(v) => put(a, 4 * i, &v.wrapping_add(add).to_le_bytes())?,
                            None => {
                                for k in 0..4 {
                                    put_opt(a, 4 * i + k, None)?;
                                }
                            }
                        }
                    }
                }
                if tag == 0x111 {
                    self.strip_offset = arr;
                } else {
                    self.strip_byte_cnt = arr;
                }
            }
            _ => {}
        }
        Ok(1)
    }

    /// `myRead(block, size, count, f, value, mac)` on a block that may be null.
    fn read_into(&mut self, f: &mut CFile, b: &mut Blk, size: i32, count: i32, off: Arg32, mac: i32) -> Result<i32, Fault> {
        let off = off.get()?;
        let mut empty: Mem = Vec::new();
        let dst = match b.as_mut() {
            Some(v) => v.as_mut_slice(),
            None => empty.as_mut_slice(),
        };
        self.my_read(f, dst, size, count, off, mac)
    }

    /// Takes the next `len` bits of `buf` as one sample [soft 0x085e22bc extract], from bit 0
    /// when `reset`. The bytes are loaded as `signed char`: the first byte's sign spreads
    /// over every bit above it (the mask meant to cut it is `-1 >> first_bit`, which is all
    /// ones), and the byte after the sample is shifted in by `8 − last_bit` places, a full 8
    /// when the sample ends on a byte boundary, which leaves 0 or −1 to be ORed in. After the
    /// position moves, it jumps by `widthAdj` when `position + widthAdj` is a multiple of
    /// `realWidth` (meant for rows of 1-bit samples padded to whole bytes; it is in bits for
    /// every depth). A length above 32 prints "len = %d is too big in extract()!" on standard
    /// error and gives −1.
    pub fn extract(&mut self, buf: &[Option<u8>], len: i32, reset: bool) -> Result<Bits, Fault> {
        if len > 0x20 {
            self.log.stderr.extend_from_slice(format!("len = {len} is too big in extract()!\n\n").as_bytes());
            return Ok(Bits::known(-1));
        }
        if reset {
            self.bit_pos = 0;
        }
        let bp = self.bit_pos;
        self.first_byte = bp >> 3;
        self.first_bit = bp % 8;
        let bp = len.wrapping_add(bp);
        self.bit_pos = bp;
        self.last_byte = bp >> 3;
        self.last_bit = bp % 8;
        let probe = bp.wrapping_add(self.width_adj);
        let rw = self.real_width();
        if rw == 0 || (probe == i32::MIN && rw == -1) {
            return Err(Fault("an integer division by zero or overflow (the processor's divide error)"));
        }
        if probe % rw == 0 {
            self.bit_pos = probe;
        }
        let byte = |i: i32| Bits::sx(at(buf, i as i64));
        // the mask: -1 shifted right arithmetically, which is -1
        let mut r = byte(self.first_byte);
        self.i_269 = self.first_byte.wrapping_add(1);
        while self.i_269 < self.last_byte {
            r = r.shl(8).or(byte(self.i_269));
            self.i_269 += 1;
        }
        let lb = self.last_bit;
        if lb != 0 && self.last_byte == self.first_byte {
            r = r.sar((8 - lb) as u32);
        } else {
            r = r.shl(lb as u32).or(byte(self.last_byte).sar((8 - lb) as u32));
        }
        self.ret_val = r;
        Ok(r)
    }
}

/// `fscanf(f, "%ld %s", &value, code)` on a text table, for well-formed lines: −1 at the end
/// of the text before the number, 1 when it ends before the code, else 2.
fn scan_entry(text: &[u8], at: &mut usize) -> Result<(i32, Option<i32>, Option<Vec<u8>>), Fault> {
    let space = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    while *at < text.len() && space(text[*at]) {
        *at += 1;
    }
    if *at >= text.len() {
        return Ok((-1, None, None));
    }
    let start = *at;
    if matches!(text[*at], b'+' | b'-') {
        *at += 1;
    }
    let digits = *at;
    while *at < text.len() && text[*at].is_ascii_digit() {
        *at += 1;
    }
    if *at == digits || *at - digits > 9 {
        return Err(Fault("a table line that does not start with a number the port reads"));
    }
    let v: i32 = std::str::from_utf8(&text[start..*at]).unwrap().parse().unwrap();
    while *at < text.len() && space(text[*at]) {
        *at += 1;
    }
    if *at >= text.len() {
        return Ok((1, Some(v), None));
    }
    let s = *at;
    while *at < text.len() && !space(text[*at]) {
        *at += 1;
    }
    Ok((2, Some(v), Some(text[s..*at].to_vec())))
}

impl Tiff {
    /// Builds the two fax code trees from the text tables [soft 0x085e25bc codesReadCcitt]:
    /// `/usr/softimage/rsrc/white_rl.tiff`, then `black_rl.tiff` (absolute paths), each read
    /// as 104 lines `%ld %s` (a run length and a code of `0` and `1`) into the entries of the
    /// code table. Each code is walked from the root (`0`: kid 0, anything else: kid 1),
    /// making nodes as needed, and its run length put in the last node (black ones negated, a
    /// black run of 0 stored as 0x80000000); the entry keeps the code's length. A line the
    /// file ends before leaves its entry as it was (and is walked all the same). Without the
    /// white file: "Cannot open the code file white.rl!" (level 1) and 0; without the black
    /// one the message is only written into `msg`, not shown, and 0. Else 1.
    pub fn codes_read_ccitt(&mut self, white: Option<&[u8]>, black: Option<&[u8]>) -> Result<i32, Fault> {
        let Some(w) = white else {
            self.show(b"Cannot open the code file white.rl!\n", 1)?;
            return Ok(0);
        };
        self.white_tree = CTree { nodes: vec![CNode { value: i32::MIN, kids: [None, None] }] };
        self.load_codes(w, 0, 0x68, false)?;
        let Some(b) = black else {
            self.sprintf_msg(b"Cannot open the code file black_rl!\n")?;
            return Ok(0);
        };
        self.black_tree = CTree { nodes: vec![CNode { value: i32::MIN, kids: [None, None] }] };
        self.load_codes(b, 0x68, 0xd0, true)?;
        Ok(1)
    }

    fn load_codes(&mut self, text: &[u8], from: usize, to: usize, black: bool) -> Result<(), Fault> {
        let mut at = 0usize;
        for k in from..to {
            let e = 20 * k;
            let (_, v, code) = scan_entry(text, &mut at)?;
            if let Some(v) = v {
                self.code_table[e..e + 4].copy_from_slice(&known(&v.to_le_bytes()));
            }
            if let Some(code) = code {
                if e + 4 + code.len() + 1 > CODE_TABLE {
                    return Err(Fault("a code that runs past the code table"));
                }
                for (j, &c) in code.iter().enumerate() {
                    self.code_table[e + 4 + j] = Some(c);
                }
                self.code_table[e + 4 + code.len()] = Some(0);
            }
            let mut value = need(i32_at(&self.code_table, k as i64 * 5), "a run length nobody set")?;
            if black {
                value = value.wrapping_neg();
                self.code_table[e..e + 4].copy_from_slice(&known(&value.to_le_bytes()));
            }
            let mut len = 0usize;
            while need(at_(&self.code_table, e + 4 + len), "a code that runs past the code table")? != 0 {
                len += 1;
            }
            let tree = if black { &mut self.black_tree } else { &mut self.white_tree };
            let mut node = 0usize;
            for j in 0..len {
                let dir = (self.code_table[e + 4 + j] != Some(b'0')) as usize;
                node = match tree.nodes[node].kids[dir] {
                    Some(n) => n,
                    None => {
                        tree.nodes.push(CNode { value: i32::MIN, kids: [None, None] });
                        let n = tree.nodes.len() - 1;
                        tree.nodes[node].kids[dir] = Some(n);
                        n
                    }
                };
            }
            tree.nodes[node].value = if black && value == 0 { i32::MIN } else { value };
            self.code_table[e..e + 4].copy_from_slice(&known(&(len as i32).to_le_bytes()));
        }
        Ok(())
    }

    /// The next bit of a fax-coded strip [soft 0x085e28d8 bitGet]: the byte at `cur_byte`
    /// under the mask `lmask`, which then shifts right **as a signed char**. When the mask is
    /// 0 it becomes 0x80 and, unless `cur_byte` is 0, the strip moves on a byte, and −1 says
    /// the strip's byte count is reached. From 0x80 the mask goes 0xc0, 0xe0 … 0xff and stays
    /// there: after its first call the routine never moves on, never ends a strip, and from
    /// the eighth bit answers 1 for a byte that is not 0.
    pub fn bit_get(&mut self, buf: &Blk, strip: i32) -> Result<i32, Fault> {
        if self.lmask == 0 {
            self.lmask = 0x80;
            if self.cur_byte != 0 {
                self.cur_byte = self.cur_byte.wrapping_add(1);
                let cnt = need(i32_at(block(&self.strip_byte_cnt)?, strip as i64), "a strip byte count nobody set")?;
                if self.cur_byte == cnt {
                    return Ok(-1);
                }
            }
        }
        let b = need(at(block(buf)?, self.cur_byte as i64), "a strip byte nobody set")?;
        let m = self.lmask;
        self.lmask = ((m as i8) >> 1) as u8;
        Ok((m & b != 0) as i32)
    }

    fn outbuf(&self, i: i64) -> Result<usize, Fault> {
        let a = A_OUTBUF as i64 + i;
        if a < 0 || a as usize >= AREA {
            return Err(Fault("a write past the globals the port keeps (outBuf runs on)"));
        }
        Ok(a as usize)
    }

    /// Appends `n` bits given as the characters `0` and `1` to the output of the fax decoder
    /// [soft 0x085e2940 codeWriteCcitt]. Bits gather in `outBuf` at the bit position `pos`;
    /// past 512 full bytes the first 512 go to `coutput` (grown by 2 KB whenever fewer than
    /// 512 bytes are left) and the rest moves down, and the routine then clears 512 bytes from
    /// there, which runs over `photoInter` and `pos` when more than 12 bytes moved. The bit
    /// mask is a `signed char` shifted right: started at 0x80 it never comes back to zero and
    /// every bit after the first lands in the same byte; started lower it moves on once. With
    /// `flush` the bytes gathered go to `coutput` and the position starts again. Answers how
    /// far `coutput` is filled.
    pub fn code_write_ccitt(&mut self, text: &[u8], n: i32, flush: i32) -> Result<i32, Fault> {
        if self.coutput_pos.wrapping_add(0x200) >= self.coutput_len {
            self.coutput_len = self.coutput_len.wrapping_add(0x800);
            self.coutput = realloc(self.coutput.take(), self.coutput_len as u32)?;
        }
        let pos = self.pos();
        let bit = pos & 7;
        let mut idx = (pos >> 3) as i64;
        if idx > 0x1ff && flush == 0 {
            let chunk: Mem = self.area[A_OUTBUF..A_OUTBUF + 0x200].iter().map(|&b| Some(b)).collect();
            let cp = self.coutput_pos as i64;
            put_block(&mut self.coutput, cp, &chunk)?;
            self.coutput_pos = self.coutput_pos.wrapping_add(0x200);
            self.set_pos(self.pos().wrapping_sub(0x1000));
            let k = idx - 0x1ff;
            if A_OUTBUF as i64 + 0x200 + k > AREA as i64 || A_OUTBUF as i64 + k + 0x200 > AREA as i64 {
                return Err(Fault("a copy past the globals the port keeps (outBuf runs on)"));
            }
            for j in 0..k as usize {
                self.area[A_OUTBUF + j] = self.area[A_OUTBUF + 0x200 + j];
            }
            for j in 0..0x200 {
                self.area[A_OUTBUF + k as usize + j] = 0;
            }
            idx = k - 1;
        }
        let mut mask = (0x80u32 >> bit) as u8;
        for i in 0..n.max(0) as usize {
            if text.get(i) == Some(&b'1') {
                let a = self.outbuf(idx)?;
                self.area[a] |= mask;
            }
            mask = ((mask as i8) >> 1) as u8;
            if mask == 0 {
                mask = 0x80;
                idx += 1;
                let a = self.outbuf(idx)?;
                self.area[a] = 0;
            }
        }
        if flush == 0 {
            self.set_pos(self.pos().wrapping_add(n as u32));
            return Ok(self.coutput_pos);
        }
        let len = if n != 0 || bit != 0 { idx + 1 } else { idx };
        if len != 0 {
            let a = self.outbuf(len - 1)?;
            let chunk: Mem = self.area[A_OUTBUF..=a].iter().map(|&b| Some(b)).collect();
            let cp = self.coutput_pos as i64;
            put_block(&mut self.coutput, cp, &chunk)?;
            self.coutput_pos = self.coutput_pos.wrapping_add(len as i32);
        }
        let r = self.coutput_pos;
        self.coutput_pos = 0;
        self.set_pos(0);
        Ok(r)
    }

    /// Decodes one fax-coded strip into `coutput` [soft 0x085e2424 ccittDecompress]: the
    /// bits walk the white tree, then the black one, alternately; at a node without the kid
    /// the bit asks for, the node's run is written (white: that many `1` bits when above 0;
    /// black: the negated value in `0` bits, for values below 0 but 0x80000000), and the walk
    /// starts again at the root with the same bit, of the black tree after a white run up to
    /// 63, of the white tree after a black run of −63 or more. It ends when [`Tiff::bit_get`]
    /// answers −1, and writes what is gathered. As [`Tiff::bit_get`] never answers −1 once its
    /// mask has been set from 0, in the program the routine never returns: that, a walk that
    /// goes from root to root without reading a bit, and a run longer than the routine's
    /// 520-byte buffer are a [`Fault`] here.
    pub fn ccitt_decompress(&mut self, buf: &Blk, strip: i32) -> Result<(), Fault> {
        let cnt = need(i32_at(block(&self.strip_byte_cnt)?, strip as i64), "a strip byte count nobody set")?;
        self.coutput_len = cnt.wrapping_mul(2);
        self.coutput = malloc(self.coutput_len as u32)?;
        let doomed = |t: &Tiff| t.lmask & 0x80 != 0;
        let mut bit = self.bit_get(buf, strip)?;
        if bit >= 0 && doomed(self) {
            return Err(Fault("the fax decoder never ends (its bit mask never comes back to zero)"));
        }
        let (mut black_tree, mut node, mut white) = (false, 0usize, true);
        let mut handled = 0;
        while bit >= 0 {
            let tree = if black_tree { &self.black_tree } else { &self.white_tree };
            let kid = if bit <= 1 { tree.nodes[node].kids[bit as usize] } else { Some(node) };
            match kid {
                Some(k) => {
                    node = k;
                    handled = 0;
                    bit = self.bit_get(buf, strip)?;
                    if bit >= 0 && doomed(self) {
                        return Err(Fault("the fax decoder never ends (its bit mask never comes back to zero)"));
                    }
                }
                None => {
                    handled += 1;
                    if handled == 4 {
                        return Err(Fault("the fax decoder goes from root to root for ever"));
                    }
                    let v = tree.nodes[node].value;
                    if white {
                        if v > 0 {
                            if v > 0x208 {
                                return Err(Fault("a run longer than the decoder's buffer on the stack"));
                            }
                            self.code_write_ccitt(&vec![b'1'; v as usize], v, 0)?;
                        }
                        if v > 0x3f {
                            (black_tree, node) = (false, 0);
                        } else {
                            (black_tree, node, white) = (true, 0, false);
                        }
                    } else {
                        if (v as u32) > 0x8000_0000 && v < 0 {
                            if -v > 0x208 {
                                return Err(Fault("a run longer than the decoder's buffer on the stack"));
                            }
                            self.code_write_ccitt(&vec![b'0'; (-v) as usize], -v, 0)?;
                        }
                        if v < -0x3f {
                            (black_tree, node) = (true, 0);
                        } else {
                            (black_tree, node, white) = (false, 0, true);
                        }
                    }
                }
            }
        }
        self.code_write_ccitt(&[], 0, 1)?;
        Ok(())
    }
}

impl Tiff {
    pub fn nb_entry(&self) -> u32 {
        self.area_u32(A_NB_ENTRY)
    }

    pub fn set_nb_entry(&mut self, v: u32) {
        self.set_area_u32(A_NB_ENTRY, v)
    }

    pub fn mask(&self) -> u16 {
        u16::from_le_bytes([self.area[A_MASK], self.area[A_MASK + 1]])
    }

    pub fn set_mask(&mut self, v: u16) {
        self.area[A_MASK..A_MASK + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn new_lnode(&mut self) -> usize {
        self.lzw_nodes.push(LNode { value: 0x1000, kids: [None; 4], freed: false });
        self.lzw_nodes.len() - 1
    }

    /// Adds a string to the LZW tables [soft 0x085e0014 tableAddEntry]: a copy in a new block
    /// at entry `nbEntry` of `stringTbl`, its length in `lengthTbl`, and a path in the string
    /// tree, two bits of each byte at a time from the low end, the byte shifted **as a signed
    /// char** (a byte of 0x80 or more continues with groups of ones), nodes made as needed
    /// (entry 0x1000); the last node gets the entry's number. `nbEntry` goes up; at 0x200,
    /// 0x400 and 0x800 entries the code length becomes 10, 11 and 12 bits. Answers `nbEntry`.
    pub fn table_add_entry(&mut self, bytes: &[Option<u8>], len: u32) -> Result<u32, Fault> {
        let nb = self.nb_entry();
        if nb as usize >= LZW_TABLE {
            return Err(Fault("an LZW table entry past the table"));
        }
        let mut copy = malloc(len)?;
        if len != 0 {
            if len as usize > bytes.len() {
                return Err(Fault("a read outside a heap block"));
            }
            let m = block_mut(&mut copy)?;
            m.copy_from_slice(&bytes[..len as usize]);
        }
        self.string_tbl[nb as usize] = copy;
        self.length_tbl[nb as usize] = len;
        let mut node = self.string_tree.ok_or(Fault("a read through a null pointer"))?;
        for u in 0..len as usize {
            let mut b = need(bytes[u], "a string byte nobody set")?;
            for _ in 0..4 {
                let d = (b & 3) as usize;
                node = match self.lzw_nodes[node].kids[d] {
                    Some(k) => k,
                    None => {
                        let k = self.new_lnode();
                        self.lzw_nodes[node].kids[d] = Some(k);
                        k
                    }
                };
                b = ((b as i8) >> 2) as u8;
            }
        }
        self.lzw_nodes[node].value = nb;
        let nb = nb.wrapping_add(1);
        self.set_nb_entry(nb);
        match nb {
            0x400 => {
                self.code_len = 11;
                self.set_mask(0x7ff);
            }
            0x200 => {
                self.code_len = 10;
                self.set_mask(0x3ff);
            }
            0x800 => {
                self.code_len = 12;
                self.set_mask(0xfff);
            }
            _ => {}
        }
        Ok(nb)
    }

    /// The next code of an LZW strip [soft 0x085e0284 codeReadLzw]: `codeLen` bits after the
    /// bit position `pos` of the 32 bits `word1`, `word2`; when `pos` passes 15 the next two
    /// bytes of the strip (one when it ends) are loaded into `word2` at `cinput_pos`, the
    /// second **sign-extended**, so that a byte of 0x80 or more fills the high byte with ones.
    pub fn code_read_lzw(&mut self, strip: i32) -> Result<u16, Fault> {
        let pos = self.pos().wrapping_add(self.code_len as u32);
        self.set_pos(pos);
        let mask = self.mask() as u32;
        if pos <= 0xf {
            self.code = (mask & ((self.word1 as u32) >> ((0x10 - pos) & 31))) as u16;
            return Ok(self.code);
        }
        let p = pos.wrapping_sub(0x10);
        self.set_pos(p);
        let cin = block(&self.cinput)?;
        let b0 = need(at(cin, self.cinput_pos as i64), "an LZW byte nobody set")?;
        let mut w2 = (b0 as u16) << 8;
        self.cinput_pos = self.cinput_pos.wrapping_add(1);
        let cnt = need(i32_at(block(&self.strip_byte_cnt)?, strip as i64), "a strip byte count nobody set")?;
        if self.cinput_pos < cnt {
            let b1 = need(at(block(&self.cinput)?, self.cinput_pos as i64), "an LZW byte nobody set")?;
            w2 |= b1 as i8 as i16 as u16;
            self.cinput_pos = self.cinput_pos.wrapping_add(1);
        }
        self.word2 = w2;
        let sh = p & 31;
        let hi = ((self.word1 as u32) & (mask >> sh)) << sh;
        let lo = ((w2 as u32) >> ((0x10 - p) & 31)) & (mask >> ((self.code_len as u32).wrapping_sub(p) & 31));
        self.code = (lo | hi) as u16;
        self.word1 = w2;
        Ok(self.code)
    }

    /// Frees the LZW string tree from `n` down [soft 0x085e2250 treeFreeLzw]: the four
    /// children's subtrees in order, then the node. Answers the order of the nodes freed.
    pub fn tree_free_lzw(&mut self, n: Option<usize>) -> Result<Vec<usize>, Fault> {
        let mut out = Vec::new();
        if let Some(n) = n {
            if self.lzw_nodes[n].freed {
                return Err(Fault("a node freed twice"));
            }
            for k in self.lzw_nodes[n].kids {
                if k.is_some() {
                    out.extend(self.tree_free_lzw(k)?);
                }
            }
            self.lzw_nodes[n].freed = true;
            out.push(n);
        }
        Ok(out)
    }

    fn grow_coutput(&mut self, need_: u32) -> Result<(), Fault> {
        if self.coutput_len as u32 <= need_ {
            while self.coutput_len as u32 <= need_ {
                self.coutput_len = self.coutput_len.wrapping_add(0x800);
            }
            self.coutput = realloc(self.coutput.take(), self.coutput_len as u32)?;
        }
        Ok(())
    }

    fn string(&self, code: u16) -> Result<(Mem, u32), Fault> {
        let len = self.length_tbl[code as usize];
        let s = match &self.string_tbl[code as usize] {
            Some(m) => {
                if len as usize > m.len() {
                    return Err(Fault("a read outside a heap block"));
                }
                m[..len as usize].to_vec()
            }
            None if len == 0 => Vec::new(),
            None => return Err(Fault("a read through a null pointer")),
        };
        Ok((s, len))
    }

    /// Decodes one LZW strip into `coutput` [soft 0x085e1d70 lzwDecompress]. `coutput` starts
    /// at twice the strip's byte count and grows by 2 KB as needed; codes start 9 bits wide,
    /// the first two bytes of the strip are loaded as the reader of [`Tiff::code_read_lzw`]
    /// loads them. Code 0x101 ends the strip. Code 0x100 frees the table's strings and the
    /// tree, makes 256 one-byte entries and the two empty ones, and copies the string of the
    /// code after it. Any other code appends its string (a code not yet in the table: the
    /// previous string and its own first byte) and adds the previous string plus the first
    /// byte of this one. The tables, `nbEntry` and `cinput_pos` are not reset: a strip
    /// starts from what the one before left. Not reached from the reader, which refuses LZW.
    pub fn lzw_decompress(&mut self, strip: i32) -> Result<(), Fault> {
        let mut filled: u32 = 0;
        let cnt = need(i32_at(block(&self.strip_byte_cnt)?, strip as i64), "a strip byte count nobody set")?;
        self.coutput_len = cnt.wrapping_mul(2);
        self.coutput = malloc(self.coutput_len as u32)?;
        self.str_len = 0x50;
        self.out_str = malloc(0x50)?;
        self.code_len = 9;
        self.set_mask(0x1ff);
        let cin = block(&self.cinput)?;
        let b0 = need(at(cin, 0), "an LZW byte nobody set")?;
        let mut w1 = (b0 as u16) << 8;
        if cnt > 1 {
            let b1 = need(at(cin, 1), "an LZW byte nobody set")?;
            w1 |= b1 as i8 as i16 as u16;
        }
        self.word1 = w1;
        let mut steps = 0u64;
        loop {
            steps += 1;
            if steps > 1 << 20 {
                return Err(Fault("an LZW strip the port gives up on (more than a million codes)"));
            }
            let mut code = self.code_read_lzw(strip)?;
            if code == 0x101 {
                break;
            }
            let n;
            if code == 0x100 {
                self.i_260 = 0;
                let nb = self.nb_entry();
                if nb != 0 {
                    while self.i_260 < self.nb_entry() {
                        if (self.i_260 as usize) >= LZW_TABLE {
                            return Err(Fault("an LZW table entry past the table"));
                        }
                        self.string_tbl[self.i_260 as usize] = None;
                        self.i_260 += 1;
                    }
                }
                let nb = self.nb_entry() as usize;
                if nb > LZW_TABLE {
                    return Err(Fault("an LZW table entry past the table"));
                }
                for k in 0..nb {
                    self.string_tbl[k] = None;
                    self.length_tbl[k] = 0;
                }
                let root = self.string_tree;
                self.tree_free_lzw(root)?;
                self.code_len = 9;
                self.set_mask(0x1ff);
                let r = self.new_lnode();
                self.string_tree = Some(r);
                self.set_nb_entry(0);
                for b in 0..=255u8 {
                    self.table_add_entry(&[Some(b)], 1)?;
                }
                let nb = self.nb_entry() as usize;
                self.string_tbl[nb] = None;
                self.string_tbl[nb + 1] = None;
                self.set_nb_entry(nb as u32 + 2);
                code = self.code_read_lzw(strip)?;
                if code == 0x101 {
                    break;
                }
                let (s, len) = self.string(code)?;
                self.grow_coutput(filled.wrapping_add(len))?;
                put_block(&mut self.coutput, filled as i64, &s)?;
                n = len;
            } else {
                let oc = self.old_code;
                self.out_len = self.length_tbl[oc as usize].wrapping_add(1);
                if self.str_len < self.out_len {
                    self.str_len = self.out_len.wrapping_add(10);
                    self.out_str = malloc(self.str_len)?;
                }
                let (old, olen) = self.string(oc)?;
                put_block(&mut self.out_str, 0, &old)?;
                if (code as u32) < self.nb_entry() {
                    let (s, len) = self.string(code)?;
                    self.grow_coutput(filled.wrapping_add(len))?;
                    put_block(&mut self.coutput, filled as i64, &s)?;
                    n = len;
                    let first = match &self.string_tbl[code as usize] {
                        Some(m) => at(m, 0),
                        None => return Err(Fault("a read through a null pointer")),
                    };
                    put_block(&mut self.out_str, olen as i64, &[first])?;
                } else {
                    while self.coutput_len as u32 <= filled.wrapping_add(self.out_len) {
                        self.coutput_len = self.coutput_len.wrapping_add(0x800);
                        self.coutput = realloc(self.coutput.take(), self.coutput_len as u32)?;
                    }
                    let first = match &self.string_tbl[oc as usize] {
                        Some(m) => at(m, 0),
                        None => return Err(Fault("a read through a null pointer")),
                    };
                    put_block(&mut self.out_str, olen as i64, &[first])?;
                    let s = block(&self.out_str)?;
                    if self.out_len as usize > s.len() {
                        return Err(Fault("a read outside a heap block"));
                    }
                    let s = s[..self.out_len as usize].to_vec();
                    put_block(&mut self.coutput, filled as i64, &s)?;
                    n = self.out_len;
                }
                let s = block(&self.out_str)?.clone();
                let ol = self.out_len;
                self.table_add_entry(&s, ol)?;
            }
            filled = filled.wrapping_add(n);
            self.old_code = code;
        }
        self.out_str = None;
        Ok(())
    }
}

fn at_(m: &[Option<u8>], i: usize) -> Option<u8> {
    m.get(i).copied().flatten()
}

/// Bytes into a block at an offset; a write outside the block (or through null) faults.
fn put_block(b: &mut Blk, off: i64, bytes: &[Option<u8>]) -> Result<(), Fault> {
    let m = block_mut(b)?;
    if off < 0 || off as usize + bytes.len() > m.len() {
        return Err(Fault("a write outside a heap block"));
    }
    m[off as usize..off as usize + bytes.len()].copy_from_slice(bytes);
    Ok(())
}

/// `fild` of an int, `fdivr` of the double 255.0 by it, `fstp` to a float: the scale from a
/// sample of `2^bits − 1` steps to a byte.
fn scale(steps: i32) -> F80 {
    F80::from_f32((F80::from_f64(255.0) / F80::from_i32(steps)).to_f32())
}

/// `fild` of the sample, `fmul` by the scale, then (`minus`) `fsubr` from the float 255.0,
/// `fistp` under truncation; the byte stored is the low byte of the integer.
fn to_byte(v: Bits, s: Option<F80>, minus: bool) -> Option<u8> {
    let p = F80::from_i32(v.value()?) * s?;
    let p = if minus { F80::from_f32(255.0) - p } else { p };
    Some(p.to_i32(Round::Zero) as u8)
}

/// PackBits decoding of `n` bytes of `src` into `dst` [the routine of `util.c`,
/// `si_unPackBit` (0x085e2d50); its port on plain bytes is `si_picture::un_pack_bit`]: here
/// on blocks whose bytes may be undefined and whose ends are known. A count byte of 0x80 is
/// copied as data and not counted.
fn unpack_bits(dst: &mut Blk, src: &Blk, n: i32) -> Result<(), Fault> {
    let mut left = n;
    let mut s: i64 = 0;
    let mut d: i64 = 0;
    let rd = |i: i64| -> Option<u8> { src.as_ref().and_then(|m| at(m, i)) };
    while left > 0 {
        if src.is_none() {
            return Err(Fault("a read through a null pointer"));
        }
        let c = need(rd(s), "a PackBits count byte nobody set")?;
        if c == 0x80 {
            put(block_mut(dst)?, d, &[0x80])?;
            s += 1;
            d += 1;
        } else if c & 0x80 != 0 {
            let k = 1 - (c as i8 as i64);
            let v = rd(s + 1);
            let o = block_mut(dst)?;
            for j in 0..k {
                put_opt(o, d + j, v)?;
            }
            d += k;
            s += 2;
            left -= 2;
        } else {
            let k = c as i64 + 1;
            let o = block_mut(dst)?;
            for j in 0..k {
                put_opt(o, d + j, rd(s + 1 + j))?;
            }
            d += k;
            s += 1 + k;
            left -= 1 + k as i32;
        }
    }
    Ok(())
}

impl Tiff {
    /// Decodes one strip into the picture [soft 0x085e13b0 stripStoreInPict]: `sample` is
    /// the plane (planar files) and `strip` the strip within it.
    ///
    /// The rows go to `strip × rowsPerStrip × imgWidth` in the planes. Where they go: red,
    /// green and blue alike, except for photometric interpretation 2 of a planar file (the
    /// plane of the sample, 0 to 2) and 4 (the alpha plane; a mask that is not one bit and one
    /// sample prints three lines on standard error and has no plane). The strip is first
    /// brought into `coutput`: copied for compression 1 (a block of the byte count of strip
    /// `strip` **of the first plane**), unpacked for 0x8005 (into a block of `rowsPerStrip ×
    /// imgWidth` made once), decoded by the fax decoder for 2 (which never returns) or the LZW
    /// decoder for 5; another compression is reported ("Compression = %d is invalid!", level
    /// 3) and decoding goes on from whatever `coutput` is. Then, for each pixel of the strip
    /// (the first one whatever the count), by photometric interpretation: 0, white is zero:
    /// `255 − v × 255/(2^b − 1)`; 1: `v × 255/(2^b − 1)`; 2: each of three samples (a planar
    /// file: the one) with its own scale; 3: the high bytes of the colour map's red, green and
    /// blue entries `v`; 4: `NOT (255 × v)`; another prints "Error: photo_inter = %d!".
    /// Products are truncated; the byte stored is the low byte of the integer.
    pub fn strip_store(&mut self, pic: &mut TiffPicture, buf: &Blk, sample: i32, strip: i32) -> Result<(), Fault> {
        let off = strip.wrapping_mul(self.rows_per_strip).wrapping_mul(self.img_width) as i64;
        let index = if self.plan_config == 1 { strip } else { sample.wrapping_mul(self.strips_per_img).wrapping_add(strip) };
        let photo = self.photo_inter();
        let bps = |t: &Tiff, i: i64| -> Result<Option<i16>, Fault> { Ok(i16_at(block(&t.bits_per_sample)?, i)) };
        // which planes: three (r, g, b), or one (`single`), or one that is never set
        let mut three = false;
        let mut single: Option<usize> = None;
        let mut single_scale: Option<F80> = None;
        if photo == 2 && self.plan_config == 2 {
            let b = need(bps(self, sample as i64)?, "a bits-per-sample entry nobody set")?;
            single_scale = Some(scale(power(2, b as i32).wrapping_sub(1)));
            single = match sample {
                0 | 1 | 2 => Some(sample as usize),
                _ => None,
            };
        } else if photo == 4 {
            let b0 = need(bps(self, 0)?, "a bits-per-sample entry nobody set")?;
            if b0 == 1 && self.samples_per_pix == 1 {
                single = Some(3);
            } else {
                self.log.stderr.extend_from_slice(b"Error: photo_inter = 4\n");
                self.log.stderr.extend_from_slice(format!("       bits_per_sample[0] = {b0}\n").as_bytes());
                self.log.stderr.extend_from_slice(format!("       samples_per_pix = {}\n\n", self.samples_per_pix).as_bytes());
            }
        } else {
            three = true;
        }
        match self.compression {
            2 => {
                self.cinput = buf.clone();
                self.set_pos(0);
                self.area[A_OUTBUF] = 0;
                self.ccitt_decompress(buf, index)?;
            }
            1 => {
                let cnt = need(i32_at(block(&self.strip_byte_cnt)?, strip as i64), "a strip byte count nobody set")?;
                self.coutput = if self.coutput.is_none() { malloc(cnt as u32)? } else { realloc(self.coutput.take(), cnt as u32)? };
                if cnt != 0 {
                    let src = block(buf)?.clone();
                    let dst = block_mut(&mut self.coutput)?;
                    if cnt as u32 as usize > dst.len() {
                        return Err(Fault("a write outside a heap block"));
                    }
                    for k in 0..cnt as u32 as i64 {
                        dst[k as usize] = at(&src, k);
                    }
                }
            }
            5 => {
                self.cinput = buf.clone();
                self.set_pos(0);
                for b in &mut self.area[A_OUTBUF..A_OUTBUF + OUTBUF_SIZE] {
                    *b = 0;
                }
                self.lzw_decompress(index)?;
                let root = self.string_tree;
                self.tree_free_lzw(root)?;
            }
            0x8005 => {
                if self.coutput.is_none() {
                    self.coutput_len = self.rows_per_strip.wrapping_mul(self.img_width);
                    self.coutput = malloc(self.coutput_len as u32)?;
                }
                let i = sample.wrapping_mul(self.strips_per_img).wrapping_add(strip);
                let cnt = need(i32_at(block(&self.strip_byte_cnt)?, i as i64), "a strip byte count nobody set")?;
                let mut out = self.coutput.take();
                let r = unpack_bits(&mut out, buf, cnt);
                self.coutput = out;
                r?;
            }
            c => {
                let text = format!("Compression = {} is invalid!\n", c as u32);
                self.show(text.as_bytes(), 3)?;
            }
        }
        let rows = if strip == self.strips_per_img.wrapping_sub(1) {
            self.img_length.wrapping_sub(strip.wrapping_mul(self.rows_per_strip))
        } else {
            self.rows_per_strip
        };
        let count = rows.wrapping_mul(self.img_width);
        let cout = self.coutput.clone();
        let src: &[Option<u8>] = match &cout {
            Some(m) => m,
            None => &[],
        };
        let null_src = cout.is_none();
        let ex = |t: &mut Tiff, len: Option<i16>, reset: bool| -> Result<Bits, Fault> {
            let len = need(len, "a bits-per-sample entry nobody set")?;
            if null_src && len <= 0x20 {
                return Err(Fault("a read through a null pointer"));
            }
            t.extract(src, len as i32, reset)
        };
        // a byte into plane `p` at pixel k of the strip
        fn store(pic: &mut TiffPicture, p: Option<usize>, at: i64, v: Option<u8>) -> Result<(), Fault> {
            let p = p.ok_or(Fault("a plane pointer the routine never set"))?;
            put_opt(block_mut(&mut pic.planes[p])?, at, v)
        }
        let each = count.max(1) as i64;
        match photo {
            0 | 1 => {
                let b0 = bps(self, 0)?;
                let s = b0.map(|b| scale(power(2, b as i32).wrapping_sub(1)));
                for k in 0..each {
                    let v = ex(self, b0, k == 0)?;
                    let byte = to_byte(v, s, photo == 0);
                    if !three {
                        return Err(Fault("a plane pointer the routine never set"));
                    }
                    for p in [2, 1, 0] {
                        store(pic, Some(p), off + k, byte)?;
                    }
                }
            }
            2 => {
                if self.plan_config == 1 {
                    let b: Vec<Option<i16>> = (0..3).map(|i| bps(self, i)).collect::<Result<_, _>>()?;
                    let s: Vec<Option<F80>> = b.iter().map(|b| b.map(|b| scale(power(2, b as i32).wrapping_sub(1)))).collect();
                    for k in 0..each {
                        for c in 0..3 {
                            let v = ex(self, b[c], k == 0 && c == 0)?;
                            store(pic, Some(c), off + k, to_byte(v, s[c], false))?;
                        }
                    }
                } else if self.plan_config == 2 {
                    let b = bps(self, sample as i64)?;
                    for k in 0..each {
                        let v = ex(self, b, k == 0)?;
                        store(pic, single, off + k, to_byte(v, single_scale, false))?;
                    }
                } else {
                    self.log.stderr.extend_from_slice(format!("Error: plan_config = {}!\n\n", self.plan_config).as_bytes());
                }
            }
            3 => {
                let b0 = bps(self, 0)?;
                for k in 0..each {
                    let v = ex(self, b0, k == 0)?.value();
                    let cm = block(&self.color_map)?;
                    let cl = self.color_len;
                    let pick = |e: Option<i32>| e.and_then(|e| at(cm, 2 * e as i64 + 1));
                    let r = pick(v);
                    let g = pick(v.map(|v| v.wrapping_add(cl)));
                    let b = pick(v.map(|v| cl.wrapping_mul(2).wrapping_add(v)));
                    store(pic, Some(0), off + k, r)?;
                    store(pic, Some(1), off + k, g)?;
                    store(pic, Some(2), off + k, b)?;
                }
            }
            4 => {
                let b0 = bps(self, 0)?;
                for k in 0..each {
                    let v = ex(self, b0, k == 0)?.value();
                    store(pic, single, off + k, v.map(|v| !(v.wrapping_mul(255) as u8)))?;
                }
            }
            _ => {
                self.log.stderr.extend_from_slice(format!("Error: photo_inter = {photo}!\n\n").as_bytes());
            }
        }
        Ok(())
    }

    /// Reads one picture from a TIFF file [soft 0x085e03a4 SI_pictureReadTiff]. `file` is
    /// what `SI_softOpen` (0x085e3164) opened for the name and the extension `.tif` (`None`:
    /// nothing, and −1); `mac` is 0x80 for a file behind a MacBinary header (any value not 0
    /// counts as one; the offsets in the file are from the TIFF header on).
    ///
    /// The globals the routine starts from are set again (no strip tables, one sample of one
    /// bit, compression 1, planar configuration 2, rows a strip unknown), the byte order is
    /// taken from the first two bytes (`MM`: big-endian; anything else: the other), and the
    /// magic number 42 read. A file that does not have it is refused with "%s is not a TIFF
    /// file!" or, when the two bytes at 0x80 are `MM`, "Not a TIFF file or file has a
    /// MacBinary header" (level 3), −1. Otherwise the resolutions start at 1/1, the
    /// directory is read, and the picture gets the width and height (16 bits of the 32),
    /// ratio `x resolution / y resolution` (the x87 quotient of the two cross products,
    /// stored as a float), `fields` 3, three planes of `width × height` and an alpha plane for
    /// a one-bit one-sample mask (photometric interpretation 4). The fax decoder's tables are
    /// read for compression 2 (−1 if that fails); LZW is refused ("LZW decompression is not
    /// supported.", level 2, −1). Then each strip of each plane is read into a block of its
    /// byte count and decoded. A fax-coded picture of photometric interpretation 1 would be
    /// turned upside down at the end; the tables' roots are static, so their release kills the
    /// program before that. The answer is 1 when the directory names a next one, else 0;
    /// `file_pos` keeps where the file stood.
    pub fn read(&mut self, file: Option<&[u8]>, mac: i32, name: &[u8], white: Option<&[u8]>, black: Option<&[u8]>) -> Result<ReadTiff, Fault> {
        let mut out = ReadTiff::default();
        let Some(data) = file else {
            out.ret = Some(-1);
            return Ok(out);
        };
        let mut f = CFile::new(data);
        self.cinput = None;
        self.coutput = None;
        self.rows_per_strip = -1;
        self.strip_byte_cnt = None;
        self.bits_per_sample = None;
        self.samples_per_pix = 1;
        self.color_map = None;
        self.strip_offset = None;
        self.compression = 1;
        self.plan_config = 2;
        if mac != 0 {
            f.seek(0x80);
        }
        // the two bytes of the byte order, into an 8-byte local nobody set
        let mut mark = [None; 8];
        fread(&mut f, &mut mark, 1, 2)?;
        let is_mm = strncmp2(&mark, b"MM")?;
        self.byte_order = (!is_mm) as i32;
        let mut magic = [None; 2];
        self.my_read(&mut f, &mut magic, 2, 1, 0, mac)?;
        // 42 is 0x2a, 0x00 in memory: a known byte that differs settles it
        let is_42 = match (magic[0], magic[1]) {
            (Some(a), Some(b)) => a == 0x2a && b == 0,
            (Some(a), None) if a != 0x2a => false,
            (None, Some(b)) if b != 0 => false,
            _ => return Err(Fault("the magic number is a value nobody set")),
        };
        if !is_42 {
            f.seek(0x80);
            fread(&mut f, &mut mark, 1, 2)?;
            let text = if strncmp2(&mark, b"MM")? {
                b"Not a TIFF file or file has a MacBinary header\n".to_vec()
            } else {
                let mut t = name.to_vec();
                t.extend_from_slice(b" is not a TIFF file!\n");
                t
            };
            self.show(&text, 3)?;
            out.ret = Some(-1);
            return Ok(out);
        }
        self.x_res = [1, 0, 0, 0, 1, 0, 0, 0].map(Some);
        self.y_res = [1, 0, 0, 0, 1, 0, 0, 0].map(Some);
        let mut off = [None; 4];
        let old = self.ifd_offset;
        if let Some(v) = old {
            off = v.to_le_bytes().map(Some);
        }
        self.my_read(&mut f, &mut off, 4, 1, 0, mac)?;
        let mut ifd = le::<4>(&off, 0).map(i32::from_le_bytes);
        if mac != 0 {
            ifd = ifd.map(|v| v.wrapping_add(0x80));
        }
        self.ifd_offset = ifd;
        f.seek(need(ifd, "the directory's offset is a value nobody set")?);
        self.ifd_offset = self.ifd_read(&mut f, mac)?;
        let w = self.img_width;
        let rw = (if w.wrapping_add(7) < 0 { w.wrapping_add(14) } else { w.wrapping_add(7) }) & !7;
        self.set_real_width(rw);
        self.width_adj = rw.wrapping_sub(w);
        let mut pic = TiffPicture { width: w as i16, height: self.img_length as i16, fields: 3, ..Default::default() };
        let word = |b: &[Option<u8>; 8], i: usize| le::<4>(b, 4 * i as i64).map(i32::from_le_bytes);
        let (x0, x1, y0, y1) = (word(&self.x_res, 0), word(&self.x_res, 1), word(&self.y_res, 0), word(&self.y_res, 1));
        let ratio = match (x0, x1, y0, y1) {
            (Some(x0), Some(x1), Some(y0), Some(y1)) => {
                let a = y1.wrapping_mul(x0);
                let b = x1.wrapping_mul(y0);
                (F80::from_i32(a) / F80::from_i32(b)).to_f32().to_bits()
            }
            _ => return Err(Fault("a resolution nobody set")),
        };
        pic.ratio = ratio;
        let size = (pic.width as i32).wrapping_mul(pic.height as i32) as u32;
        for p in 0..3 {
            pic.planes[p] = malloc(size)?;
        }
        if self.photo_inter() == 4 {
            let b0 = need(i16_at(block(&self.bits_per_sample)?, 0), "a bits-per-sample entry nobody set")?;
            if b0 == 1 && self.samples_per_pix == 1 {
                pic.planes[3] = malloc(size)?;
            }
        }
        out.picture = Some(pic.clone());
        if self.rows_per_strip == -1 || self.img_length < self.rows_per_strip {
            self.rows_per_strip = self.img_length;
            self.strips_per_img = 1;
        } else {
            let rps = self.rows_per_strip;
            let n = rps.wrapping_add(self.img_length).wrapping_sub(1);
            if rps == 0 || (n == i32::MIN && rps == -1) {
                return Err(Fault("an integer division by zero or overflow (the processor's divide error)"));
            }
            self.strips_per_img = n / rps;
        }
        if self.compression == 2 && self.codes_read_ccitt(white, black)? == 0 {
            out.ret = Some(-1);
            return Ok(out);
        }
        if self.compression == 5 {
            self.log.status.push((b"LZW decompression is not supported.".to_vec(), 2));
            out.ret = Some(-1);
            return Ok(out);
        }
        let planes = if self.plan_config == 2 { self.samples_per_pix as i32 } else { 1 };
        if self.photo_inter() == 3 {
            let b0 = need(i16_at(block(&self.bits_per_sample)?, 0), "a bits-per-sample entry nobody set")?;
            self.color_len = power(2, b0 as i32);
        }
        let mut i = 0;
        while i < planes {
            let mut j = 0;
            while j < self.strips_per_img {
                let k = i.wrapping_mul(self.strips_per_img).wrapping_add(j) as i64;
                let cnt = need(i32_at(block(&self.strip_byte_cnt)?, k), "a strip byte count nobody set")?;
                let mut buf = malloc(cnt as u32)?;
                let at_ = need(i32_at(block(&self.strip_offset)?, k), "a strip offset nobody set")?;
                f.seek(at_);
                match buf.as_mut() {
                    Some(b) => {
                        fread(&mut f, b, 1, cnt as u32)?;
                    }
                    None => {
                        if !f.read(cnt as u32 as u64).is_empty() {
                            return Err(Fault("fread into a null pointer"));
                        }
                    }
                }
                self.strip_store(&mut pic, &buf, i, j)?;
                j += 1;
            }
            i += 1;
        }
        self.coutput = None;
        if self.compression == 2 {
            // treeFreeCcitt (0x085e2ad0) on the two trees frees their static roots
            return Err(Fault("the fax trees' roots are static data, which free() is handed"));
        }
        self.strip_byte_cnt = None;
        self.color_map = None;
        self.bits_per_sample = None;
        self.strip_offset = None;
        self.file_pos = f.tell();
        out.picture = Some(pic);
        out.ret = self.ifd_offset.map(|v| (v != 0) as i32);
        Ok(out)
    }
}

/// A file being written, byte by byte; a byte the writer took from memory nobody set is
/// `None`. A write past the end fills the gap with zeros, as the C library does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WFile {
    pub data: Mem,
    pub pos: usize,
}

impl WFile {
    fn write(&mut self, b: &[Option<u8>]) {
        if self.pos > self.data.len() {
            self.data.resize(self.pos, Some(0));
        }
        let end = self.pos + b.len();
        if self.data.len() < end {
            self.data.resize(end, Some(0));
        }
        self.data[self.pos..end].copy_from_slice(b);
        self.pos = end;
    }

    fn known(&mut self, b: &[u8]) {
        self.write(&known(b));
    }

    /// `SI_byteswap_fwrite(p, 4, n, f)` (0x083f9c40): each word big-endian.
    fn words(&mut self, m: &[Option<u8>], n: i32) -> Result<(), Fault> {
        if n <= 0 {
            return Ok(());
        }
        if (n as usize) * 4 > m.len() {
            return Err(Fault("a read outside a heap block"));
        }
        for e in 0..n as usize {
            let mut w = m[4 * e..4 * e + 4].to_vec();
            w.reverse();
            self.write(&w);
        }
        Ok(())
    }

    fn be32(&mut self, v: i32) {
        self.known(&v.to_be_bytes());
    }

    fn tell(&self) -> i32 {
        self.pos as i32
    }

    fn seek(&mut self, to: i32) {
        if to >= 0 {
            self.pos = to as usize;
        }
    }
}

/// What [`Tiff::write`] leaves: the answer and the file's bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WriteTiff {
    pub ret: i32,
    pub file: Mem,
    /// A PackBits row came out longer than the 1.1 × width bytes of `coutput` and the
    /// original wrote on past the block (see [`Tiff::write`]).
    pub overrun: bool,
}

/// A byte of a plane at a signed index: undefined outside the plane.
fn plane_at(p: &Option<Vec<u8>>, i: i64) -> Result<Option<u8>, Fault> {
    let p = p.as_ref().ok_or(Fault("a plane read without a test is absent"))?;
    Ok(if i < 0 || i as usize >= p.len() { None } else { Some(p[i as usize]) })
}

impl Tiff {
    /// Writes a picture as a TIFF file [soft 0x085dec70 SI_pictureWriteTiff]. `planar` is
    /// the planar configuration written (1: chunky, which is always uncompressed; anything
    /// else: the three planes one after another), `raw` not 0 for no compression (else
    /// PackBits, 0x8005, row by row); `mac` is 0x80 for a MacBinary header (the value is used
    /// as the header's size in the offsets). `date` is the block of 20 bytes `strftime` filled
    /// with the time as `%Y:%m:%d %H:%M:%S` (the original asks the clock).
    ///
    /// The file: `MM`, 42, the directory's offset (written last); the three bits-per-sample
    /// words (8); room for the strip offsets and (when there is more than one strip) the byte
    /// counts, filled at the end; resolutions `ratio × 10^6 / 10^6` and 1/1, positions 0/0 and
    /// 0/0; the description "Image coming from a Softimage picture file." in an 80-byte block
    /// of which the last 36 bytes are what the allocator left; "Softimage v2.1", the date,
    /// "Iris"; the directory of 20 entries; the strips (`8192 / width + 1` rows each). What
    /// this build writes wrongly, reproduced: every rational has a count of 2; with one strip
    /// (a chunky picture of at most that many rows) the strip offset and byte count entries
    /// hold what lay in the allocator's block and on the stack, while the real offset goes to
    /// the room after the bits-per-sample words, which no entry points to; the offsets of the
    /// entries after it are then 4 bytes short.
    ///
    /// With a MacBinary header the file starts with 128 zero bytes, overwritten at the end by
    /// a header naming the file (the base name, at most 63 bytes before it runs on into the
    /// type `TIFF`, creator `SOFT`, flags 4) with the data length `file length − 128`, and the
    /// file is padded with zeros to a multiple of 128 bytes past the header.
    ///
    /// A PackBits row is coded into a block of `1.1 × width` bytes, made at the first row;
    /// a row that codes longer (this coder can make 4 bytes of 3) runs on past the block.
    /// The file is still what the coder made (each row is written at once from the block);
    /// the answer says so in `overrun`, and what the overrun does to the heap is not modelled.
    pub fn write(&mut self, pic: &si_picture::Picture, name: &[u8], mac: i32, planar: i16, raw: i16, date: &[Option<u8>]) -> Result<WriteTiff, Fault> {
        let raw = if planar == 1 { 1 } else { raw };
        self.cinput = None;
        self.coutput = None;
        let mut f = WFile::default();
        let mut overrun = false;
        let header_room = vec![Some(0u8); 0x80];
        if mac != 0 {
            f.write(&header_room);
        }
        f.known(b"MM");
        f.known(&42u16.to_be_bytes());
        f.be32(0);
        let mut bps = malloc(6)?.unwrap();
        put(&mut bps, 0, &[8, 0, 8, 0, 8, 0])?;
        self.bits_per_sample = Some(bps.clone());
        self.compression = if raw != 0 { 1 } else { 0x8005 };
        let w = pic.width as i32;
        let h = pic.height as i32;
        self.img_width = w;
        if w == 0 {
            return Err(Fault("an integer division by zero (the processor's divide error)"));
        }
        let q = 0x2000 / w;
        let rps = q + 1;
        if rps == 0 {
            return Err(Fault("an integer division by zero (the processor's divide error)"));
        }
        let strips = q.wrapping_add(h) / rps;
        self.img_length = h;
        self.set_photo_inter(2);
        self.rows_per_strip = rps;
        self.samples_per_pix = 3;
        let spp = self.samples_per_pix as i32;
        let n = if planar == 1 { strips } else { spp.wrapping_mul(strips) };
        self.strip_byte_cnt = malloc((strips as u32).wrapping_mul(if planar == 1 { 4 } else { 12 }))?;
        self.strip_offset = malloc((n as u32).wrapping_mul(4))?;
        let xres = (F80::from_f32(pic.ratio) * F80::from_f64(1_000_000.0)).to_i32(Round::Zero);
        let mut desc = malloc(0x50)?.unwrap();
        put(&mut desc, 0, b"Image coming from a Softimage picture file.\0")?;
        let software = known(b"Softimage v2.1\0");
        let make = known(b"Iris\0");
        if date.len() != 0x14 {
            return Err(Fault("the date block is 20 bytes"));
        }
        // the values of the entries with a count of 1, as little-endian words; a4[10] is
        // never set
        let mut a4: Vec<[Option<u8>; 4]> = vec![[None; 4]; 20];
        let word = |v: i32| v.to_le_bytes().map(Some);
        a4[0] = word(w << 16);
        a4[1] = word(h << 16);
        f.known(&[0, 8, 0, 8, 0, 8]);
        a4[2] = word((self.compression as i32) << 16);
        a4[3] = word(2 << 16);
        a4[4] = word(spp << 16);
        a4[5] = word(rps << 16);
        a4[6] = word((planar as u16 as i32) << 16);
        a4[7] = word(0x10000);
        a4[8] = word(0x10000);
        let tables = f.tell();
        let mut counts_at = None;
        f.words(self.strip_offset.as_deref().unwrap_or(&[]), n)?;
        if n == 1 {
            let b = block(&self.strip_byte_cnt)?;
            a4[9] = [at(b, 0), at(b, 1), at(b, 2), at(b, 3)];
        } else {
            counts_at = Some(f.tell());
            f.words(self.strip_byte_cnt.as_deref().unwrap_or(&[]), n)?;
        }
        f.be32(xres);
        f.be32(1_000_000);
        for v in [1, 1, 0, 0, 0, 0] {
            f.be32(v);
        }
        f.write(&desc);
        f.write(&software);
        f.write(date);
        f.write(&make);
        let ifd = f.tell().wrapping_sub(mac);
        f.seek(mac.wrapping_add(4));
        f.be32(ifd);
        f.seek(mac.wrapping_add(ifd));
        let tags: [u16; 20] = [0x100, 0x101, 0x102, 0x103, 0x106, 0x115, 0x116, 0x11c, 0x128, 0x13d, 0x111, 0x117, 0x11a, 0x11b, 0x11e, 0x11f, 0x10e, 0x131, 0x132, 0x13c];
        let counts: [i32; 20] = [1, 1, spp, 1, 1, 1, 1, 1, 1, 1, n, n, 2, 2, 2, 2, 0x50, 0xf, 0x14, 5];
        f.known(&0x14u16.to_be_bytes());
        let mut off = 8i32;
        let mut k = 0usize;
        for i in 0..20 {
            let (ty, unit) = match i {
                0..=9 => (3u16, 2),
                10 | 11 => (4, 4),
                12..=15 => (5, 4),
                _ => (2, 1),
            };
            f.known(&tags[i].to_be_bytes());
            f.known(&ty.to_be_bytes());
            f.be32(counts[i]);
            if counts[i] == 1 && i < 12 {
                let mut b = a4[k].to_vec();
                b.reverse();
                f.write(&b);
                k += 1;
            } else {
                f.be32(off);
                off = off.wrapping_add(counts[i].wrapping_mul(unit));
            }
        }
        f.be32(0);
        self.set_pos(0);
        self.cinput_len = w;
        let all = h.wrapping_mul(w);
        if planar == 1 {
            self.cinput_len = rps.wrapping_mul(self.cinput_len);
            self.coutput = malloc((self.cinput_len as u32).wrapping_mul(3))?;
            let mut s = 0;
            while s < strips {
                if s == strips - 1 {
                    self.cinput_len = all.wrapping_sub(s.wrapping_mul(rps).wrapping_mul(w));
                }
                let len = self.cinput_len;
                put(block_mut(&mut self.strip_byte_cnt)?, 4 * s as i64, &len.wrapping_mul(3).to_le_bytes())?;
                let base = s.wrapping_mul(rps).wrapping_mul(w) as i64;
                for j in 0..len.max(0) as i64 {
                    let px = [plane_at(&pic.planes[0], base + j)?, plane_at(&pic.planes[1], base + j)?, plane_at(&pic.planes[2], base + j)?];
                    let out = block_mut(&mut self.coutput)?;
                    for (c, v) in px.iter().enumerate() {
                        put_opt(out, 3 * j + c as i64, *v)?;
                    }
                }
                let here = f.tell().wrapping_sub(mac);
                put(block_mut(&mut self.strip_offset)?, 4 * s as i64, &here.to_le_bytes())?;
                let cnt = len.wrapping_mul(3);
                if cnt != 0 {
                    let out = block(&self.coutput)?;
                    if cnt < 0 || cnt as usize > out.len() {
                        return Err(Fault("a read outside a heap block"));
                    }
                    let b = out[..cnt as usize].to_vec();
                    f.write(&b);
                }
                s += 1;
            }
        } else {
            if raw != 0 {
                self.coutput = malloc(self.cinput_len as u32)?;
            }
            let len = self.cinput_len;
            for plane in 0..3 {
                let mut cin: i64 = 0;
                let mut rows = rps;
                let mut s = 0;
                while s < strips {
                    let e = (plane as i32).wrapping_mul(strips).wrapping_add(s) as i64;
                    let here = f.tell().wrapping_sub(mac);
                    put(block_mut(&mut self.strip_offset)?, 4 * e, &here.to_le_bytes())?;
                    let mut total = 0i32;
                    put(block_mut(&mut self.strip_byte_cnt)?, 4 * e, &total.to_le_bytes())?;
                    if s == strips - 1 {
                        rows = h.wrapping_sub(s.wrapping_mul(rps));
                    }
                    let mut r = rows;
                    if r > 0 {
                        loop {
                            let row: Vec<Option<u8>> = (0..len.max(0) as i64).map(|j| plane_at(&pic.planes[plane], cin + j)).collect::<Result<_, _>>()?;
                            if raw == 0 {
                                let bytes: Vec<u8> = row.iter().map(|b| b.ok_or(Fault("PackBits compares a byte nobody set"))).collect::<Result<_, _>>()?;
                                if bytes.len() < 2 {
                                    return Err(Fault("PackBits of fewer than two bytes compares bytes in front of its input"));
                                }
                                if self.coutput.is_none() {
                                    let size = (F80::from_i32(len) * F80::from_f64(1.1)).to_i32(Round::Zero);
                                    self.coutput = malloc(size as u32)?;
                                }
                                let packed = si_picture::pack_bit(&bytes);
                                let out = block_mut(&mut self.coutput)?;
                                // `si_packBit` has no limit: a row that codes to more than
                                // the block's 1.1 × width bytes runs on into the heap behind
                                // it, and the row goes to the file from the buffer all the
                                // same. The file is therefore what the coder made; what the
                                // overrun does to the heap is not modelled (the real
                                // converter sometimes dies after writing, soft2tiff on p034
                                // of the oracle's samples). The block keeps what fits.
                                if packed.len() > out.len() {
                                    overrun = true;
                                    let room = out.len();
                                    put(out, 0, &packed[..room])?;
                                } else {
                                    put(out, 0, &packed)?;
                                }
                                total = total.wrapping_add(packed.len() as i32);
                                f.known(&packed);
                            } else {
                                total = total.wrapping_add(len);
                                f.write(&row);
                            }
                            put(block_mut(&mut self.strip_byte_cnt)?, 4 * e, &total.to_le_bytes())?;
                            cin += len as i64;
                            r -= 1;
                            if r == 0 {
                                break;
                            }
                        }
                    }
                    s += 1;
                }
                self.cinput = if pic.planes[plane].is_some() { Some(Vec::new()) } else { None };
            }
        }
        f.seek(tables);
        f.words(self.strip_offset.as_deref().unwrap_or(&[]), n)?;
        if n != 1 {
            f.seek(counts_at.unwrap_or(0));
            f.words(self.strip_byte_cnt.as_deref().unwrap_or(&[]), n)?;
        }
        if mac != 0 {
            let base = match name.iter().rposition(|&b| b == b'/') {
                Some(i) => &name[i + 1..],
                None => name,
            };
            // strcpy into the header from byte 2: past byte 0x7f it runs over the stack
            if 2 + base.len() + 1 > 0x80 {
                return Err(Fault("a name that runs past the MacBinary header on the stack"));
            }
            let mut head = vec![0u8; 0x80];
            head[1] = base.len() as u8;
            head[2..2 + base.len()].copy_from_slice(base);
            head[2 + base.len()] = 0;
            head[0x41..0x45].copy_from_slice(b"TIFF");
            head[0x45..0x49].copy_from_slice(b"SOFT");
            head[0x49] = 4;
            let end = f.data.len() as i32;
            let data = end.wrapping_sub(0x80) as u32;
            head[0x53..0x57].copy_from_slice(&data.to_be_bytes());
            f.seek(0);
            f.known(&head);
            let pad = 0x80 - (data & 0x7f) as i32;
            if pad < 0x80 {
                f.pos = f.data.len();
                f.known(&vec![0u8; pad as usize]);
            }
        }
        self.coutput = None;
        self.bits_per_sample = None;
        self.strip_offset = None;
        self.strip_byte_cnt = None;
        Ok(WriteTiff { ret: 1, file: f.data, overrun })
    }
}

/// `strncmp(buf, s, 2) == 0` on bytes that may be undefined: the comparison stops at the
/// first difference or at a NUL of both.
fn strncmp2(buf: &[Option<u8>], s: &[u8; 2]) -> Result<bool, Fault> {
    for k in 0..2 {
        let b = need(buf[k], "a comparison with a byte nobody set")?;
        if b != s[k] {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be_entry(tag: u16, ty: u16, count: u32, value: u32) -> Vec<u8> {
        let mut v = tag.to_be_bytes().to_vec();
        v.extend_from_slice(&ty.to_be_bytes());
        v.extend_from_slice(&count.to_be_bytes());
        v.extend_from_slice(&value.to_be_bytes());
        v
    }

    #[test]
    fn power_is_repeated_product() {
        assert_eq!(power(2, 8), 256);
        assert_eq!(power(2, 0), 1);
        assert_eq!(power(2, -3), 1);
        assert_eq!(power(2, 32), 0);
    }

    #[test]
    fn extract_spreads_the_sign_and_takes_in_the_next_byte() {
        let mut t = Tiff::default();
        t.set_real_width(8);
        let b = known(&[0x12, 0x34, 0x90, 0x01]);
        assert_eq!(t.extract(&b, 8, true).unwrap().value(), Some(0x12));
        // the byte after 0x34 is 0x90: its top bit makes the sample all ones
        assert_eq!(t.extract(&b, 8, false).unwrap().value(), Some(-1));
        assert_eq!(t.extract(&b, 8, false).unwrap().value(), Some(0x90u8 as i8 as i32));
        // the byte after the last one is not there: the sample is not known
        assert_eq!(t.extract(&b, 8, false).unwrap().value(), None);
    }

    #[test]
    fn little_endian_magic_is_refused() {
        let mut file = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        file.extend_from_slice(&[0; 8]);
        let mut t = Tiff::default();
        let r = t.read(Some(&file), 0, b"/t/a.tif", None, None).unwrap();
        assert_eq!(r.ret, Some(-1));
        assert_eq!(t.log.status[0].0, b"/t/a.tif is not a TIFF file!\n");
    }

    #[test]
    fn a_small_grey_file() {
        // 2 x 1, 8 bits, photometric 1, one strip of 3 bytes (one spare)
        let mut f = b"MM\x00\x2a\x00\x00\x00\x08".to_vec();
        let entries = [
            be_entry(0x100, 3, 1, 2 << 16),
            be_entry(0x101, 3, 1, 1 << 16),
            be_entry(0x102, 3, 1, 8 << 16),
            be_entry(0x106, 3, 1, 1 << 16),
            be_entry(0x111, 4, 1, 8 + 2 + 12 * 6 + 4),
            be_entry(0x117, 4, 1, 3),
        ];
        f.extend_from_slice(&(entries.len() as u16).to_be_bytes());
        for e in &entries {
            f.extend_from_slice(e);
        }
        f.extend_from_slice(&[0; 4]);
        f.extend_from_slice(&[0x10, 0x20, 0x00]);
        let mut t = Tiff::default();
        let r = t.read(Some(&f), 0, b"/t/a.tif", None, None).unwrap();
        assert_eq!(r.ret, Some(0));
        let p = r.picture.unwrap();
        assert_eq!((p.width, p.height, p.fields), (2, 1, 3));
        assert_eq!(p.planes[0].as_ref().unwrap(), &vec![Some(0x10), Some(0x20)]);
        assert!(p.planes[3].is_none());
    }
}
