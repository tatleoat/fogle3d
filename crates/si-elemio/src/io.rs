// SPDX-License-Identifier: MIT
//! What the element readers and writers share: the token stream, the variables the original
//! keeps between reads, and the small field readers and writers (finding 0005).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `utilio.c`, `bsceneio.c`, `byteswap.c`, `binary.c`,
//! `binary_b.c`, `b_matio.c`.
//!
//! An element file is, after its header, a stream of 16-bit *tokens*, each followed by the
//! field it announces; token 0 ends a record. The high byte of a token is the size of its
//! field in bytes (0 = not stated, 0xff = a 32-bit size follows), which is how a reader steps
//! over a token it does not know ([`Reader::skip_field`]).
//!
//! The original reads every token and most fields into global variables and never looks at
//! what `fread` returns. A read that fails leaves the variable as it was, so on a file that
//! ends early the old value is used again. [`Statics`] holds those variables so that the same
//! thing happens here.

use si_file::Stream;
use std::collections::BTreeMap;

/// The variables the original readers and writers keep between calls, and between files.
/// `token` exists once per source file (each is a `static short`); the decompiler prints them
/// all under one name, the addresses tell them apart.
#[derive(Clone, Debug, PartialEq)]
pub struct Statics {
    /// `token` of `b_structio.c` (0x08817f24): the camera, light and lattice writers and the
    /// converter's readers.
    pub token_bstructio: u16,
    /// `token` of `binary.c` (0x08817f2c): the cluster reader.
    pub token_binary: u16,
    /// `token` of `binary_b.c` (0x08817f30): the cluster writer.
    pub token_binary_b: u16,
    /// `token` of `bsceneio.c` (0x08817f34): the program's camera, light and lattice readers.
    pub token_bsceneio: u16,
    /// `token` of `fcurveio.c` (0x08817f3c): function curves, keys and static values.
    pub token_fcurveio: u16,
    /// `token` of `utilio.c` (0x08818160): values, positions, key and curve writers, actions.
    pub token_utilio: u16,
    /// `token` of the element dictionary's file (0x08785ba4): the animation item header.
    pub token_elemdict: u16,
    /// `token` of the hierarchy animation's file (0x08788120).
    pub token_hieranim: u16,
    /// `si_svalue` of the hierarchy animation's file (0x08788122), a static of its own.
    pub svalue_hieranim: i16,
    /// `si_svalue` (0x08817f48)
    pub svalue: i16,
    /// `si_fvalue` (0x08817f40)
    pub fvalue: f32,
    /// `si_ivalue` (0x08817f44)
    pub ivalue: i32,
    /// `si_lvalue` (0x08817f4c)
    pub lvalue: i32,
    /// `SI_versionInput` (0x086793f0): the version of the last header read. Starts at 4.0.
    pub version_input: f32,
    /// `fileVersion` of `binary.c` (0x08817f28): set by the model, shape and cluster readers
    /// only, yet consulted when user data is read from any file.
    pub file_version: f32,
    /// `SI_frameRate` (0x0878204c): the scene's frame rate; the action lists store their
    /// times shifted by half a frame. Zero until a scene sets it.
    pub frame_rate: f32,
}

impl Default for Statics {
    /// The state of a freshly started program: everything zero (`.bss`), `SI_versionInput` 4.0.
    fn default() -> Self {
        Statics {
            token_bstructio: 0,
            token_binary: 0,
            token_binary_b: 0,
            token_bsceneio: 0,
            token_fcurveio: 0,
            token_utilio: 0,
            token_elemdict: 0,
            token_hieranim: 0,
            svalue_hieranim: 0,
            svalue: 0,
            fvalue: 0.0,
            ivalue: 0,
            lvalue: 0,
            version_input: 4.0,
            file_version: 0.0,
            frame_rate: 0.0,
        }
    }
}

/// How a token loop ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// The zero token was read: the original returns.
    Zero,
    /// The file ended first. The original never notices: its token variable keeps the last
    /// token, so it runs that token's handler again and again without end. Here the loop
    /// stops at the first token read that fails with a non-zero token left in the variable.
    Eof,
}

// --- reads through a variable, as `fread` on the little-endian builds leaves them -------------
//
// `SI_byteswap_fread(&v, size, 1, f)` [soft 0x083f9ba0 SI_byteswap_fread]: `fread` into the
// variable, then swap its bytes if a whole item arrived [soft 0x083f9330 SI_swap2byte]
// [soft 0x083f9350 SI_swap4byte]. A short read copies the bytes that were there into the start
// of the variable and swaps nothing. The variable's memory image is little-endian (the Linux
// and NT builds); on IRIX the image is big-endian and nothing is swapped.

/// Two bytes into a 16-bit variable. True if both arrived.
pub fn fread_u16(s: &mut Stream, v: &mut u16) -> bool {
    let mut b = v.to_le_bytes();
    let ok = s.read_record(&mut b);
    if ok {
        b.reverse();
    }
    *v = u16::from_le_bytes(b);
    ok
}

pub fn fread_i16(s: &mut Stream, v: &mut i16) -> bool {
    let mut u = *v as u16;
    let ok = fread_u16(s, &mut u);
    *v = u as i16;
    ok
}

/// Four bytes into a 32-bit variable. True if all four arrived.
pub fn fread_u32(s: &mut Stream, v: &mut u32) -> bool {
    let mut b = v.to_le_bytes();
    let ok = s.read_record(&mut b);
    if ok {
        b.reverse();
    }
    *v = u32::from_le_bytes(b);
    ok
}

pub fn fread_i32(s: &mut Stream, v: &mut i32) -> bool {
    let mut u = *v as u32;
    let ok = fread_u32(s, &mut u);
    *v = u as i32;
    ok
}

/// Four bytes into a float variable. Also [soft 0x083f9a7c SI_float_fread], which swaps and
/// then calls [soft 0x083f9320 validate_float], an empty function in this build.
pub fn fread_f32(s: &mut Stream, v: &mut f32) -> bool {
    let mut u = v.to_bits();
    let ok = fread_u32(s, &mut u);
    *v = f32::from_bits(u);
    ok
}

/// A position that is not stored but taken from a model: what the old position reader files
/// on the program's constraint list (`si_cnstrLst`, 0x086ceb9c) for the scene loader to
/// resolve once all models exist.
#[derive(Clone, Debug, PartialEq)]
pub struct Constraint {
    /// The kind of the owner's handle (`SI_GrafType`): 4 camera position, 5 camera interest,
    /// 6 light position, 7 light interest.
    pub graf_type: i32,
    /// The 16-bit number stored before the name (its meaning is not read yet).
    pub index: i32,
    /// The model's name.
    pub name: Vec<u8>,
}

/// One node of a user-data list (`__SI_UserNode`, 20 bytes): data a plug-in attached to an
/// element under a name.
#[derive(Clone, Debug, PartialEq)]
pub struct UserNode {
    /// +0x00
    pub name: Vec<u8>,
    /// +0x04 (meaning not read; stored as is)
    pub kind: i16,
    /// +0x08 size and +0x0c bytes
    pub data: Vec<u8>,
}

/// A reference to a custom shader (`__SI_UDF`, 60 bytes), as the element files store it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Udf {
    /// +0x00 the shader's name
    pub name: Vec<u8>,
    /// +0x0c, stored as a 32-bit integer
    pub w0c: i16,
    /// +0x10, stored as a 32-bit integer
    pub w10: i16,
    /// +0x14
    pub d14: i32,
}

/// A file being read, with the original's variables.
pub struct Reader<'a> {
    pub s: Stream<'a>,
    pub st: Statics,
    /// Set when a size read from the file was larger than what the file holds and the read
    /// was cut to the bytes that exist (the original would overrun a buffer or run out of
    /// memory there).
    pub clamped: bool,
    /// If set, every token a reader dispatches on is counted here under the reader's name:
    /// the census of tokens the check binary prints. Not part of the original.
    pub census: Option<BTreeMap<(&'static str, u16), usize>>,
}

/// The header of an element file as every element reader takes it
/// [soft 0x083fb6fc SI_dbfileReadBinHeader]: the version and comment of an accepted header;
/// for any other start of file the stream is rewound, the version reported is −1.0 and the
/// reader goes on from the first byte regardless (none of the callers looks at the result).
/// Only an accepted header's version reaches `SI_versionInput`.
pub fn read_header(r: &mut Reader) -> (f32, Vec<u8>) {
    match si_file::read_bin_header(&mut r.s) {
        Some(h) => {
            r.st.version_input = h.version;
            (h.version, h.comment_bytes().to_vec())
        }
        None => (-1.0, Vec::new()),
    }
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { s: Stream::new(data), st: Statics::default(), clamped: false, census: None }
    }

    pub fn with_statics(data: &'a [u8], st: Statics) -> Self {
        Reader { s: Stream::new(data), st, clamped: false, census: None }
    }

    /// Counts a token for the census, if one is being taken. Tokens whose read failed are
    /// not counted.
    pub fn note(&mut self, reader: &'static str, token: u16, read_ok: bool) {
        if let (Some(c), true) = (self.census.as_mut(), read_ok) {
            *c.entry((reader, token)).or_default() += 1;
        }
    }

    /// A short through `si_svalue`.
    pub fn svalue(&mut self) -> i16 {
        fread_i16(&mut self.s, &mut self.st.svalue);
        self.st.svalue
    }

    /// A 32-bit integer through `si_ivalue`.
    pub fn ivalue(&mut self) -> i32 {
        fread_i32(&mut self.s, &mut self.st.ivalue);
        self.st.ivalue
    }

    /// A 32-bit integer through `si_lvalue`.
    pub fn lvalue(&mut self) -> i32 {
        fread_i32(&mut self.s, &mut self.st.lvalue);
        self.st.lvalue
    }

    /// A float through `si_fvalue`.
    pub fn fvalue(&mut self) -> f32 {
        fread_f32(&mut self.s, &mut self.st.fvalue);
        self.st.fvalue
    }

    /// A zero-terminated string [soft 0x084040a0 SI_stringBinaryRead]: bytes up to a zero byte
    /// or the end of the file.
    ///
    /// The original collects them in a 256-byte buffer on its stack with no bound, so a
    /// string of 256 bytes or more overruns it. Here the string simply has its length.
    pub fn string(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some(b) = self.s.getc() {
            if b == 0 {
                break;
            }
            out.push(b);
        }
        out
    }

    /// Steps over the field of a token the reader does not know
    /// [soft 0x083ff174 si_fileSkipField]. The token's high byte is the field's size: 0 means
    /// nothing can be skipped (returns false, nothing read); 1 to 0xfe is that many bytes;
    /// 0xff means a 32-bit size follows, and that many bytes after it.
    ///
    /// The original reads the bytes into a 1024-byte buffer, 1024 at a time while more than
    /// that remain. A negative 32-bit size skips its loop and becomes one huge `fread` into
    /// that buffer: the rest of the file, written over the stack. Here that case consumes the
    /// rest of the file and sets `clamped`.
    pub fn skip_field(&mut self, token: u16) -> bool {
        let mut n = (token >> 8) as u32;
        if n == 0 {
            return false;
        }
        if n == 0xff {
            fread_u32(&mut self.s, &mut n);
            if (n as i32) < 0 {
                self.clamped = true;
            }
        }
        self.skip(n as usize);
        true
    }

    /// `fread` of `n` bytes that are thrown away.
    fn skip(&mut self, n: usize) {
        let left = self.s.len() - self.s.tell();
        if n > left {
            // fread comes back short and sets the end-of-file flag
            self.s.seek_end();
            self.s.getc();
        } else {
            self.s.seek(self.s.tell() + n);
        }
    }

    /// Three floats [soft 0x083fd350 si_vectorBinaryRead]. Each is read through `si_fvalue`,
    /// so a component that could not be read repeats the last float read. With no
    /// destination the floats are read and dropped.
    pub fn vector(&mut self, dst: Option<&mut [f32; 3]>) -> bool {
        let x = self.fvalue();
        let y = self.fvalue();
        let z = self.fvalue();
        match dst {
            Some(v) => {
                *v = [x, y, z];
                true
            }
            None => false,
        }
    }

    /// A value that may be animated [soft 0x083fd564 si_valueBinaryRead]: a token of
    /// `utilio.c`, then for token 1 a float. Token 2 announces a function curve in place of
    /// the float; this reader reads nothing for it (the writer side would have put a whole
    /// curve there, so such a file cannot be read by this build). Any other token is skipped
    /// by its size byte.
    pub fn value(&mut self, dst: Option<&mut f32>) {
        fread_u16(&mut self.s, &mut self.st.token_utilio);
        let t = self.st.token_utilio;
        if t == 1 {
            let v = self.fvalue();
            if let Some(d) = dst {
                *d = v;
            }
        } else if t != 2 {
            self.skip_field(t);
        }
    }

    /// Three values under one token [soft 0x083fd6a0 si_value3BinaryRead].
    pub fn value3(&mut self, dst: Option<&mut [f32; 3]>) {
        fread_u16(&mut self.s, &mut self.st.token_utilio);
        let t = self.st.token_utilio;
        if t == 1 {
            let a = self.fvalue();
            let b = self.fvalue();
            let c = self.fvalue();
            if let Some(d) = dst {
                *d = [a, b, c];
            }
        } else if t != 2 {
            self.skip_field(t);
        }
    }

    /// A position as the converter's readers take it [soft 0x083fdbe8 si_positionBinaryRead]:
    /// token 1 and a vector; anything else is skipped by its size byte.
    pub fn position(&mut self, dst: Option<&mut [f32; 3]>) {
        fread_u16(&mut self.s, &mut self.st.token_utilio);
        let t = self.st.token_utilio;
        if t == 1 {
            self.vector(dst);
        } else {
            self.skip_field(t);
        }
    }

    /// A position as the program's readers take it [soft 0x083f77b0 oldpositionBinaryRead]:
    /// a token of `bsceneio.c` (the caller's own token variable), then
    ///
    /// | token | field |
    /// |---|---|
    /// | 1 | a vector: the position itself |
    /// | 3 | a 16-bit number and a string: the position follows the model of that name |
    /// | 2, 4, 5 | nothing is read; the original prints "token … not supported" |
    ///
    /// For token 3 the original links a 28-byte record (owner, handle kind, number, name) at
    /// the head of `si_cnstrLst`; here it is returned.
    pub fn old_position(&mut self, dst: Option<&mut [f32; 3]>, graf_type: i32) -> Option<Constraint> {
        fread_u16(&mut self.s, &mut self.st.token_bsceneio);
        match self.st.token_bsceneio {
            1 => {
                self.vector(dst);
                None
            }
            3 => {
                let index = self.svalue() as i32;
                let name = self.string();
                Some(Constraint { graf_type, index, name })
            }
            _ => None,
        }
    }

    /// A user-data list [soft 0x083ec910 si_userDataRead]: a 32-bit count, then per node a
    /// 32-bit name length, the name, a 16-bit word, a 32-bit size and the data.
    ///
    /// The data is read with one `SI_byteswap_fread`. If `fileVersion` is below 3.82099 the
    /// call asks for one item of `size` bytes, and that routine reverses items of 2, 4 or 8
    /// bytes: so in a file older than that, a block of exactly 2, 4 or 8 bytes comes out
    /// byte-reversed on the little-endian builds. From 3.82099 on it asks for `size` items of
    /// one byte and nothing is reversed. `fileVersion` is `binary.c`'s own variable: the
    /// camera and light readers do not set it, so for them it is whatever the last model,
    /// shape or cluster file left there.
    ///
    /// The original allocates what the counts say. Here every count is checked against the
    /// bytes left, and the list ends at the end of the file (`clamped` is set).
    pub fn user_data(&mut self) -> Vec<UserNode> {
        let mut count = 0i32;
        fread_i32(&mut self.s, &mut count);
        let mut nodes = Vec::new();
        for _ in 0..count.max(0) {
            if self.s.feof() || self.s.tell() >= self.s.len() {
                self.clamped = true;
                break;
            }
            let mut len = 0u32;
            fread_u32(&mut self.s, &mut len);
            let name = self.bytes(len as usize);
            let mut kind = 0i16;
            fread_i16(&mut self.s, &mut kind);
            let mut size = 0u32;
            fread_u32(&mut self.s, &mut size);
            let mut data = self.bytes(size as usize);
            if (self.st.file_version as f64) < 3.82099 && matches!(size, 2 | 4 | 8) && data.len() == size as usize {
                data.reverse();
            }
            // the name is a C string in the original: it ends at its first zero byte
            let name = name.split(|&b| b == 0).next().unwrap_or(&[]).to_vec();
            nodes.push(UserNode { name, kind, data });
        }
        nodes
    }

    /// `fread(buf, 1, n, f)` into a zeroed buffer of `n` bytes, cut to the file's length.
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        let left = self.s.len() - self.s.tell();
        if n > left {
            self.clamped = true;
        }
        let mut buf = vec![0u8; n.min(left)];
        self.s.read(&mut buf);
        if n > left {
            self.s.getc(); // the short read sets the end-of-file flag
        }
        buf
    }

    /// A custom shader reference with no prototype list: [`Reader::udf_with`]`(None)`, which is
    /// how the light reader calls it (its prototype record is a local with no lists).
    pub fn udf(&mut self) -> Option<Udf> {
        self.udf_with(None)
    }

    /// A custom shader reference [soft 0x083e9154 si_udfRead]. `list` is the file's prototype
    /// for it (slot +0x60 of the prototype record, id 0x1c); without one the fields are taken
    /// in the order 1, 2, 3, 4, 5.
    ///
    /// | field | data |
    /// |---|---|
    /// | 1 | string: the shader's name. An empty name frees the record: the fields that follow are read and dropped and there is no shader |
    /// | 2 | 32-bit, kept as 16: +0x0c |
    /// | 3 | 32-bit, kept as 16: +0x10 |
    /// | 4 | 32-bit: +0x14 |
    /// | 5 | ends the record at once |
    /// | other | skipped by its size byte |
    ///
    /// A list that ends with its zero (rather than with field 5) has one more rule: a shader
    /// named `NULL` is dropped.
    ///
    /// For a file version the program calls old [soft 0x083fb934 si_dbfileIsOldUdfVersion]
    /// (up to 2.67906, or from 2.69 up to but not including 2.999019) the older layout is
    /// read and dropped instead [soft 0x083e9374 readOldUDF]: fields 1, 2, 3 are strings,
    /// 4 is 16 bits, 5 is 32 bits, others are skipped by size; the record ends at the list's
    /// zero, at a field number 6, or, without a list, after field 5.
    ///
    /// The original walks a list without a bound; here the end of the list ends the record.
    pub fn udf_with(&mut self, list: Option<&[i16]>) -> Option<Udf> {
        let mut next = {
            let mut i = 0usize;
            move |cur: i16| -> i16 {
                match list {
                    None => cur + 1,
                    Some(l) => {
                        let v = l.get(i).copied().unwrap_or(0);
                        i += 1;
                        v
                    }
                }
            }
        };
        if is_old_udf_version(self.st.version_input) {
            let mut f = if list.is_some() { next(0) } else { 1 };
            while f != 0 {
                match f {
                    1..=3 => {
                        self.string();
                    }
                    4 => {
                        let mut w = 0u16;
                        fread_u16(&mut self.s, &mut w);
                    }
                    5 => {
                        let mut d = 0u32;
                        fread_u32(&mut self.s, &mut d);
                    }
                    _ => {
                        self.skip_field(f as u16);
                    }
                }
                f = next(f);
                if f == 6 {
                    f = 0;
                }
            }
            return None;
        }
        // the record exists from the start (`SI_udfAllocate`); an empty name frees it
        let mut udf = Some(Udf::default());
        let mut f = if list.is_some() { next(0) } else { 1 };
        let mut v = 0i32;
        while f != 0 {
            match f {
                1 => {
                    let name = self.string();
                    if udf.is_none() || name.is_empty() {
                        udf = None;
                    } else if let Some(u) = udf.as_mut() {
                        u.name = name;
                    }
                }
                2 => {
                    fread_i32(&mut self.s, &mut v);
                    if let Some(u) = udf.as_mut() {
                        u.w0c = v as i16;
                    }
                }
                3 => {
                    fread_i32(&mut self.s, &mut v);
                    if let Some(u) = udf.as_mut() {
                        u.w10 = v as i16;
                    }
                }
                4 => {
                    fread_i32(&mut self.s, &mut v);
                    if let Some(u) = udf.as_mut() {
                        u.d14 = v;
                    }
                }
                5 => return udf,
                _ => {
                    self.skip_field(f as u16);
                }
            }
            f = next(f);
            if list.is_none() && f == 6 {
                f = 0;
            }
        }
        // only a list's own zero gets here
        if udf.as_ref().is_some_and(|u| u.name == b"NULL") {
            udf = None;
        }
        udf
    }
}

/// [soft 0x083fb934 si_dbfileIsOldUdfVersion]: the float version against three double
/// constants (0x0865c5a0, 0x0865c5a8, 0x0865c5b0).
pub fn is_old_udf_version(version: f32) -> bool {
    let v = version as f64;
    v <= 2.67906 || (v >= 2.69 && v < 2.999019)
}

/// A file being written, with the original's variables (the writers assign each token and
/// each short, float and integer to the same globals before writing them).
pub struct Writer {
    pub out: Vec<u8>,
    pub st: Statics,
    /// `SI_versionOutput` (0x086793f4): 4.0 in this build. The writers leave out newer fields
    /// when it is set lower.
    pub version_output: f32,
}

impl Default for Writer {
    fn default() -> Self {
        Writer::new()
    }
}

impl Writer {
    pub fn new() -> Self {
        Writer { out: Vec::new(), st: Statics::default(), version_output: si_file::VERSION_OUTPUT }
    }

    pub fn with_version(version_output: f32) -> Self {
        Writer { out: Vec::new(), st: Statics::default(), version_output }
    }

    // `SI_byteswap_fwrite` [soft 0x083f9c40 SI_byteswap_fwrite] swaps the variable, writes it
    // and swaps it back: big-endian bytes in the file.
    pub fn put_u16(&mut self, v: u16) {
        self.out.extend_from_slice(&v.to_be_bytes());
    }

    pub fn put_i32(&mut self, v: i32) {
        self.out.extend_from_slice(&v.to_be_bytes());
    }

    pub fn put_f32(&mut self, v: f32) {
        self.out.extend_from_slice(&v.to_bits().to_be_bytes());
    }

    /// A short through `si_svalue`.
    pub fn svalue(&mut self, v: i16) {
        self.st.svalue = v;
        self.put_u16(v as u16);
    }

    /// A float through `si_fvalue`.
    pub fn fvalue(&mut self, v: f32) {
        self.st.fvalue = v;
        self.put_f32(v);
    }

    /// A 32-bit integer through `si_ivalue`.
    pub fn ivalue(&mut self, v: i32) {
        self.st.ivalue = v;
        self.put_i32(v);
    }

    /// A 32-bit integer through `si_lvalue`.
    pub fn lvalue(&mut self, v: i32) {
        self.st.lvalue = v;
        self.put_i32(v);
    }

    /// A token of `utilio.c`.
    pub fn token_utilio(&mut self, t: u16) {
        self.st.token_utilio = t;
        self.put_u16(t);
    }

    /// A zero-terminated string [soft 0x08404104 SI_stringBinaryWrite]; a null pointer is
    /// written as an empty string. The bytes stop at the first zero, as a C string does.
    pub fn string(&mut self, s: Option<&[u8]>) {
        if let Some(s) = s {
            let n = s.iter().position(|&b| b == 0).unwrap_or(s.len());
            self.out.extend_from_slice(&s[..n]);
        }
        self.out.push(0);
    }

    /// Three floats [soft 0x083fd2e0 si_vectorBinaryWrite].
    pub fn vector(&mut self, v: &[f32; 3]) {
        for &c in v {
            self.fvalue(c);
        }
    }

    /// A value with no function curve [soft 0x083fd4f0 si_valueBinaryWrite]: token 1 and the
    /// float. (Given a curve the original writes token 2 and the curve; every caller in the
    /// element writers passes none.)
    pub fn value(&mut self, v: f32) {
        self.token_utilio(1);
        self.fvalue(v);
    }

    /// Three values with no function curves [soft 0x083fd5dc si_value3BinaryWrite]: token 1
    /// and three floats.
    pub fn value3(&mut self, v: &[f32; 3]) {
        self.token_utilio(1);
        for &c in v {
            self.fvalue(c);
        }
    }

    /// A position [soft 0x083fd76c si_positionBinaryWrite]: token 1 and a vector. The writer
    /// has no form for a position that follows a model.
    pub fn position(&mut self, v: &[f32; 3]) {
        self.token_utilio(1);
        self.vector(v);
    }

    /// A custom shader reference [soft 0x083e9068 si_udfWrite]: the name (an absent shader
    /// or an empty name as an empty string) and three 32-bit integers, zero for an absent
    /// shader.
    pub fn udf(&mut self, udf: Option<&Udf>) {
        let name = udf.map(|u| u.name.as_slice()).filter(|n| !n.is_empty() && n[0] != 0);
        self.string(name);
        self.put_i32(udf.map_or(0, |u| u.w0c as i32));
        self.put_i32(udf.map_or(0, |u| u.w10 as i32));
        self.put_i32(udf.map_or(0, |u| u.d14));
    }

    /// A token of `b_structio.c`.
    pub fn token_bstructio(&mut self, t: u16) {
        self.st.token_bstructio = t;
        self.put_u16(t);
    }

    /// A token of `fcurveio.c`.
    pub fn token_fcurveio(&mut self, t: u16) {
        self.st.token_fcurveio = t;
        self.put_u16(t);
    }

    /// The sizes and nodes of a user-data list as the camera and light writers put them:
    /// [`Writer::user_data_mode`] with mode 0.
    pub fn user_data(&mut self, nodes: &[UserNode]) {
        self.user_data_mode(nodes, 0);
    }

    /// The sizes and nodes of a user-data list, as its writers put them after their token:
    /// the byte size of what follows the size field [soft 0x083f3780 si_userDataSize], the
    /// node count, then the nodes [soft 0x083f3940 si_userDataWrite]: per node a 32-bit name
    /// length, the name, a 16-bit word, a 32-bit size and the data.
    ///
    /// Nodes named `CUSELEMCB` and `CUSNEXTELEMCB` (the plug-in call-back hooks) are neither
    /// counted nor written. With `mode` 1 the nodes named `CUSA`, `CUSB`, `CUSC` and `CUSD`
    /// are left out as well; the element writers of this crate use mode 0.
    ///
    /// The callers write the token and call this only for a list that exists; a list of
    /// which nothing is kept is written with a count of zero.
    pub fn user_data_mode(&mut self, nodes: &[UserNode], mode: i16) {
        let kept: Vec<&UserNode> = nodes.iter().filter(|n| !is_callback_node(&n.name) && !(mode == 1 && is_cus_abcd(&n.name))).collect();
        let cstr = |n: &UserNode| n.name.iter().position(|&b| b == 0).unwrap_or(n.name.len());
        let size: usize = 4 + kept.iter().map(|n| cstr(n) + n.data.len() + 10).sum::<usize>();
        self.put_i32(size as i32);
        self.put_i32(kept.len() as i32);
        for n in kept {
            let len = cstr(n);
            self.put_i32(len as i32);
            self.out.extend_from_slice(&n.name[..len]);
            self.put_u16(n.kind as u16);
            self.put_i32(n.data.len() as i32);
            self.out.extend_from_slice(&n.data);
        }
    }
}

fn is_cus_abcd(name: &[u8]) -> bool {
    let n = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    matches!(&name[..n], b"CUSA" | b"CUSB" | b"CUSC" | b"CUSD")
}

fn is_callback_node(name: &[u8]) -> bool {
    let n = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    &name[..n] == b"CUSELEMCB" || &name[..n] == b"CUSNEXTELEMCB"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_read_keeps_the_old_value() {
        let mut r = Reader::new(&[0x12, 0x34, 0x56]);
        assert_eq!(r.svalue(), 0x1234);
        // one byte left: fread copies it into the variable's first byte (the low one on the
        // little-endian builds) and nothing is swapped
        assert_eq!(r.svalue(), 0x1256);
        assert!(r.s.feof());
        assert_eq!(r.svalue(), 0x1256);
        let mut r = Reader::new(&[0x3f, 0x80, 0, 0]);
        assert_eq!(r.fvalue(), 1.0);
        assert_eq!(r.fvalue(), 1.0);
        let mut v = [9.0f32; 3];
        assert!(r.vector(Some(&mut v)));
        assert_eq!(v, [1.0; 3]);
    }

    #[test]
    fn skip_field_follows_the_size_byte() {
        let data: Vec<u8> = (0..40).collect();
        let mut r = Reader::new(&data);
        assert!(!r.skip_field(0x0005));
        assert_eq!(r.s.tell(), 0);
        assert!(r.skip_field(0x0305));
        assert_eq!(r.s.tell(), 3);
        let mut data = vec![0, 0, 0, 6];
        data.extend_from_slice(&[7; 10]);
        let mut r = Reader::new(&data);
        assert!(r.skip_field(0xff15));
        assert_eq!(r.s.tell(), 10);
        // a size beyond the file: the read comes back short
        let mut r = Reader::new(&[0, 0, 1, 0, 9, 9]);
        r.skip_field(0xff01);
        assert!(r.s.feof());
        assert!(!r.clamped);
        let mut r = Reader::new(&[0xff, 0xff, 0xff, 0xff, 9, 9]);
        r.skip_field(0xff01);
        assert!(r.clamped);
    }

    #[test]
    fn values_and_positions() {
        let mut w = Writer::new();
        w.value(2.5);
        w.value3(&[1.0, 2.0, 3.0]);
        w.position(&[4.0, 5.0, 6.0]);
        assert_eq!(w.out.len(), 6 + 14 + 14);
        assert_eq!(&w.out[..6], &[0, 1, 0x40, 0x20, 0, 0]);
        let mut r = Reader::new(&w.out);
        let mut f = 0.0;
        r.value(Some(&mut f));
        let mut v = [0.0; 3];
        r.value3(Some(&mut v));
        let mut p = [0.0; 3];
        assert_eq!(r.old_position(Some(&mut p), 4), None);
        assert_eq!((f, v, p), (2.5, [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]));
        // a position that follows a model
        let mut r = Reader::new(&[0, 3, 0, 2, b'b', b'o', b'x', 0, 0xaa]);
        let c = r.old_position(Some(&mut p), 5).unwrap();
        assert_eq!((c.graf_type, c.index, c.name.as_slice()), (5, 2, &b"box"[..]));
        assert_eq!(r.s.tell(), 8);
        // token 2 in a value reads nothing more
        let mut r = Reader::new(&[0, 2, 1, 2, 3, 4]);
        r.value(Some(&mut f));
        assert_eq!(r.s.tell(), 2);
        // an unknown token with a size byte is skipped
        let mut r = Reader::new(&[4, 9, 1, 2, 3, 4, 5]);
        r.value(Some(&mut f));
        assert_eq!(r.s.tell(), 6);
    }

    #[test]
    fn user_data_round_trip_and_the_old_swap() {
        let nodes = vec![
            UserNode { name: b"abc".to_vec(), kind: 3, data: vec![1, 2, 3, 4] },
            UserNode { name: b"CUSELEMCB".to_vec(), kind: 0, data: vec![9] },
            UserNode { name: b"d".to_vec(), kind: -1, data: vec![5, 6, 7] },
        ];
        let mut w = Writer::new();
        w.user_data(&nodes);
        // size, count, then two nodes
        let size = u32::from_be_bytes([w.out[0], w.out[1], w.out[2], w.out[3]]) as usize;
        assert_eq!(size, w.out.len() - 4);
        assert_eq!(size, 4 + (3 + 4 + 10) + (1 + 3 + 10));
        let mut r = Reader::new(&w.out[4..]);
        r.st.file_version = 4.0;
        let got = r.user_data();
        assert_eq!(got, vec![nodes[0].clone(), nodes[2].clone()]);
        assert_eq!(r.s.tell(), size);
        // below 3.82099 a four-byte block is reversed
        let mut r = Reader::new(&w.out[4..]);
        r.st.file_version = 3.8;
        let got = r.user_data();
        assert_eq!(got[0].data, vec![4, 3, 2, 1]);
        assert_eq!(got[1].data, vec![5, 6, 7]);
        // mode 1 leaves out the four CUSx nodes too
        let mut w = Writer::new();
        w.user_data_mode(&[UserNode { name: b"CUSB".to_vec(), kind: 0, data: vec![1] }, UserNode { name: b"CUSE".to_vec(), kind: 0, data: vec![] }], 1);
        assert_eq!(&w.out[4..8], &[0, 0, 0, 1]);
        // a count with nothing behind it does not allocate
        let mut r = Reader::new(&[0x7f, 0xff, 0xff, 0xff]);
        assert!(r.user_data().is_empty());
        assert!(r.clamped);
    }

    #[test]
    fn udf_forms() {
        let u = Udf { name: b"glow".to_vec(), w0c: 2, w10: -3, d14: 70000 };
        let mut w = Writer::new();
        w.udf(Some(&u));
        w.udf(None);
        assert_eq!(w.out.len(), 5 + 12 + 1 + 12);
        let mut r = Reader::new(&w.out);
        assert_eq!(r.udf(), Some(u));
        assert_eq!(r.udf(), None);
        assert_eq!(r.s.tell(), w.out.len());
        // with a prototype list: field 3 first, an unknown sized field, then the name; the
        // list ends with its zero, so the name NULL means no shader
        let b = [0, 0, 0, 7, 9, 9, b'x', 0];
        let mut r = Reader::new(&b);
        assert_eq!(r.udf_with(Some(&[3, 0x0263, 1, 0])), Some(Udf { name: b"x".to_vec(), w0c: 0, w10: 7, d14: 0 }));
        assert_eq!(r.s.tell(), b.len());
        assert_eq!(Reader::new(b"NULL\0").udf_with(Some(&[1, 0])), None);
        assert!(Reader::new(b"NULL\0").udf_with(Some(&[1, 5])).is_some());
        // the old layout: three strings, a short, a long
        let b = [b'a', 0, 0, b'c', 0, 1, 2, 3, 4, 5, 6, 0xee];
        let mut r = Reader::new(&b);
        r.st.version_input = 2.65;
        assert_eq!(r.udf(), None);
        assert_eq!(r.s.tell(), 11);
        assert!(is_old_udf_version(2.65));
        assert!(!is_old_udf_version(2.68));
        assert!(is_old_udf_version(2.9));
        assert!(!is_old_udf_version(2.99908)); // a version found in the sample files
        assert!(!is_old_udf_version(3.0));
        // the float nearest 2.69 is above the double 2.69, so a file marked 2.69 is "old"
        assert!((2.69f32 as f64) > 2.69);
        assert!(is_old_udf_version(2.69));
    }
}
