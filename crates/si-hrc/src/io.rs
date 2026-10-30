// SPDX-License-Identifier: MIT
//! The read and write primitives of the binary readers (source files `byteswap.c`,
//! `utilio.c`, `filetoken.c`).
//!
//! The original reads every field with `fread` straight into the structure and swaps the
//! bytes afterwards, and only when the read was complete. A field cut by the end of the file
//! therefore keeps its old value except for the leading bytes that did arrive, unswapped.
//! The helpers here take the field's current value and reproduce exactly that.

use si_file::Stream;

pub struct Rd<'a> {
    pub s: Stream<'a>,
}

impl<'a> Rd<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Rd { s: Stream::new(data) }
    }

    pub fn left(&self) -> usize {
        self.s.len() - self.s.tell()
    }

    /// `fread(buf, size, buf.len() / size, f)`: the number of complete items.
    fn fread(&mut self, buf: &mut [u8], size: usize) -> usize {
        let n = self.s.read(buf);
        if size == 0 { 0 } else { n / size }
    }

    /// [soft 0x083f9ba0 SI_byteswap_fread] with a size of 1: bytes as they are.
    pub fn bytes(&mut self, buf: &mut [u8]) -> usize {
        self.s.read(buf)
    }

    /// [soft 0x083f9ba0 SI_byteswap_fread] for one 2-byte item [soft 0x083f9330 SI_swap2byte].
    pub fn i16(&mut self, v: &mut i16) -> bool {
        let mut b = v.to_le_bytes();
        let ok = self.fread(&mut b, 2) == 1;
        if ok {
            b.reverse();
        }
        *v = i16::from_le_bytes(b);
        ok
    }

    pub fn u16(&mut self, v: &mut u16) -> bool {
        let mut t = *v as i16;
        let ok = self.i16(&mut t);
        *v = t as u16;
        ok
    }

    /// [soft 0x083f9ba0 SI_byteswap_fread] for one 4-byte item [soft 0x083f9350 SI_swap4byte].
    pub fn i32(&mut self, v: &mut i32) -> bool {
        let mut b = v.to_le_bytes();
        let ok = self.fread(&mut b, 4) == 1;
        if ok {
            b.reverse();
        }
        *v = i32::from_le_bytes(b);
        ok
    }

    pub fn u32(&mut self, v: &mut u32) -> bool {
        let mut t = *v as i32;
        let ok = self.i32(&mut t);
        *v = t as u32;
        ok
    }

    /// A float read as a 4-byte item, and [soft 0x083f9a7c SI_float_fread], which does the
    /// same and then calls a validation hook that is empty in this build
    /// [soft 0x083f9320 validate_float].
    pub fn f32(&mut self, v: &mut f32) -> bool {
        let mut t = v.to_bits() as i32;
        let ok = self.i32(&mut t);
        *v = f32::from_bits(t as u32);
        ok
    }

    /// One 8-byte item [soft 0x083f9ad0 SI_double_fread] [soft 0x083f9388 SI_swap8byte].
    pub fn f64(&mut self, v: &mut f64) -> bool {
        let mut b = v.to_le_bytes();
        let ok = self.fread(&mut b, 8) == 1;
        if ok {
            b.reverse();
        }
        *v = f64::from_le_bytes(b);
        ok
    }

    /// `N` floats read as one item of `4 × N` bytes: all are swapped, or none
    /// [soft 0x083f9d54 SI_Vector_fread] [soft 0x083fa35c SI_RGB_fread]
    /// [soft 0x083fa184 SI_RGBA_fread].
    pub fn floats<const N: usize>(&mut self, v: &mut [f32; N]) -> bool {
        let mut b = vec![0u8; 4 * N];
        for (i, x) in v.iter().enumerate() {
            b[4 * i..4 * i + 4].copy_from_slice(&x.to_bits().to_le_bytes());
        }
        let ok = self.fread(&mut b, 4 * N) == 1;
        for (i, x) in v.iter_mut().enumerate() {
            let mut w = [b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]];
            if ok {
                w.reverse();
            }
            *x = f32::from_bits(u32::from_le_bytes(w));
        }
        ok
    }

    /// Sixteen floats [soft 0x083fa4b4 SI_Matrix_fread]. Unlike the other helpers this one
    /// swaps whatever is in the buffer, complete read or not.
    pub fn matrix(&mut self, v: &mut [f32; 16]) {
        let mut b = [0u8; 64];
        for (i, x) in v.iter().enumerate() {
            b[4 * i..4 * i + 4].copy_from_slice(&x.to_bits().to_le_bytes());
        }
        self.s.read(&mut b);
        for (i, x) in v.iter_mut().enumerate() {
            *x = f32::from_bits(u32::from_be_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]));
        }
    }

    /// Three doubles [soft 0x083f9ed4 SI_3doubles_fread]: swapped whether or not the read
    /// was complete.
    pub fn doubles3(&mut self, v: &mut [f64; 3]) {
        let mut b = [0u8; 24];
        for (i, x) in v.iter().enumerate() {
            b[8 * i..8 * i + 8].copy_from_slice(&x.to_le_bytes());
        }
        self.s.read(&mut b);
        for (i, x) in v.iter_mut().enumerate() {
            let mut w = [0u8; 8];
            w.copy_from_slice(&b[8 * i..8 * i + 8]);
            *x = f64::from_be_bytes(w);
        }
    }

    /// A zero-terminated string [soft 0x084040a0 SI_stringBinaryRead]: bytes up to a zero
    /// byte or the end of the file.
    ///
    /// The original collects them in a 256-byte buffer on the stack with no bound, so a
    /// string of 256 bytes or more overruns it there. Here it is simply read.
    pub fn string(&mut self) -> Vec<u8> {
        let mut v = Vec::new();
        while let Some(c) = self.s.getc() {
            if c == 0 {
                break;
            }
            v.push(c);
        }
        v
    }

    /// `n` raw bytes into a new buffer of `n` zeros (the original's `calloc` then `fread`).
    /// A length the rest of the file cannot hold gives a buffer of what is there: the original
    /// would allocate the full length, or fail to and write through a null pointer.
    pub fn blob(&mut self, n: i32) -> Vec<u8> {
        let n = (n.max(0) as usize).min(self.left());
        let mut v = vec![0u8; n];
        self.s.read(&mut v);
        v
    }

    /// Skips a token the caller does not know [soft 0x083ff174 si_fileSkipField].
    ///
    /// The token's high byte says how: 0, nothing can be skipped and the result is false
    /// (the callers ignore it and carry on with the next token); 1 to 254, that many bytes;
    /// 255, a 4-byte length follows and then that many bytes.
    ///
    /// The original reads the skipped bytes into a 1,024-byte buffer; a negative length
    /// becomes one huge `fread` into it. Here a negative length reads to the end of the file.
    pub fn skip_field(&mut self, token: i16) -> bool {
        let hi = (token as u16 >> 8) as usize;
        let n = match hi {
            0 => return false,
            0xff => {
                let mut len = 0xff; // the length shares its variable with the high byte
                self.i32(&mut len);
                if len < 0 { usize::MAX } else { len as usize }
            }
            n => n,
        };
        if n <= self.left() {
            let at = self.s.tell();
            self.s.seek(at + n);
        } else {
            self.s.seek_end();
            self.s.getc(); // a short read: sets the end-of-file flag as fread does
        }
        true
    }
}

/// The writing side: every multi-byte value is swapped to big-endian around the `fwrite`
/// [soft 0x083f9c40 SI_byteswap_fwrite].
#[derive(Default)]
pub struct Wr {
    pub out: Vec<u8>,
}

impl Wr {
    pub fn i16(&mut self, v: i16) {
        self.out.extend_from_slice(&v.to_be_bytes());
    }
    pub fn u16(&mut self, v: u16) {
        self.out.extend_from_slice(&v.to_be_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.out.extend_from_slice(&v.to_be_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_be_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.out.extend_from_slice(&v.to_bits().to_be_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.out.extend_from_slice(&v.to_bits().to_be_bytes());
    }
    /// [soft 0x083f9df8 SI_Vector_fwrite] [soft 0x083fa3d4 SI_RGB_fwrite]
    /// [soft 0x083fa24c SI_RGBA_fwrite] [soft 0x083fa524 SI_Matrix_fwrite]
    pub fn floats(&mut self, v: &[f32]) {
        for x in v {
            self.f32(*x);
        }
    }
    /// [soft 0x083f9f5c SI_3doubles_fwrite]
    pub fn doubles(&mut self, v: &[f64]) {
        for x in v {
            self.f64(*x);
        }
    }
    pub fn bytes(&mut self, v: &[u8]) {
        self.out.extend_from_slice(v);
    }
    /// [soft 0x08404104 SI_stringBinaryWrite]: the bytes up to the first zero, then a zero.
    /// A null pointer is written as a lone zero byte.
    pub fn string(&mut self, v: Option<&[u8]>) {
        if let Some(v) = v {
            let n = v.iter().position(|&b| b == 0).unwrap_or(v.len());
            self.out.extend_from_slice(&v[..n]);
        }
        self.out.push(0);
    }
}

/// `strlen`: the length up to the first zero byte.
pub fn c_len(v: &[u8]) -> usize {
    v.iter().position(|&b| b == 0).unwrap_or(v.len())
}

/// The part of a buffer that C sees as a string.
pub fn c_str(v: &[u8]) -> &[u8] {
    &v[..c_len(v)]
}

/// Drops a read's result: the original ignores them all.
pub(crate) fn ok(_: bool) {}
