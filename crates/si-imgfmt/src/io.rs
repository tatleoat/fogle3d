// SPDX-License-Identifier: MIT
//! Small file-layer pieces the format modules share: a file being written, and
//! little-endian fields on a file being read.
//!
//! The original uses C stdio throughout. Reading is [`si_file::Stream`]; this adds what the
//! foreign formats need on top of it. Nothing here reproduces an original function.

use si_file::Stream;

/// A file being written, with the part of C stdio's behaviour the original writers depend on:
/// a position that can be moved back to patch a header or an offset table, writes that
/// overwrite in place and extend the file at its end, and a seek past the end that leaves a
/// zero-filled gap once something is written there.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Out {
    data: Vec<u8>,
    pos: usize,
}

impl Out {
    pub fn new() -> Self {
        Out::default()
    }

    /// `fwrite(buf, 1, buf.len(), f)`
    pub fn write(&mut self, buf: &[u8]) {
        let end = self.pos + buf.len();
        if self.data.len() < end {
            self.data.resize(end, 0);
        }
        self.data[self.pos..end].copy_from_slice(buf);
        self.pos = end;
    }

    /// `putc`
    pub fn putc(&mut self, b: u8) {
        self.write(&[b]);
    }

    pub fn be_u16(&mut self, v: u16) {
        self.write(&v.to_be_bytes());
    }

    pub fn be_u32(&mut self, v: u32) {
        self.write(&v.to_be_bytes());
    }

    pub fn le_u16(&mut self, v: u16) {
        self.write(&v.to_le_bytes());
    }

    pub fn le_u32(&mut self, v: u32) {
        self.write(&v.to_le_bytes());
    }

    /// `ftell`
    pub fn tell(&self) -> usize {
        self.pos
    }

    /// `fseek(f, pos, SEEK_SET)`
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// `fseek(f, 0, SEEK_END)`
    pub fn seek_end(&mut self) {
        self.pos = self.data.len();
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }
}

/// Little-endian fields and relative seeks on a file being read.
pub trait StreamExt {
    fn le_u16(&mut self) -> Option<u16>;
    fn le_i16(&mut self) -> Option<i16>;
    fn le_u32(&mut self) -> Option<u32>;
    fn le_i32(&mut self) -> Option<i32>;
    /// `fseek(f, n, SEEK_CUR)` for a forward or backward step; clears the end-of-file flag.
    fn skip(&mut self, n: i64);
    /// Bytes from the position to the end of the file: what a count read from the file must
    /// be checked against before anything is allocated for it.
    fn left(&self) -> usize;
}

impl StreamExt for Stream<'_> {
    fn le_u16(&mut self) -> Option<u16> {
        let mut b = [0u8; 2];
        self.read_record(&mut b).then(|| u16::from_le_bytes(b))
    }

    fn le_i16(&mut self) -> Option<i16> {
        self.le_u16().map(|v| v as i16)
    }

    fn le_u32(&mut self) -> Option<u32> {
        let mut b = [0u8; 4];
        self.read_record(&mut b).then(|| u32::from_le_bytes(b))
    }

    fn le_i32(&mut self) -> Option<i32> {
        self.le_u32().map(|v| v as i32)
    }

    fn skip(&mut self, n: i64) {
        let to = (self.tell() as i64 + n).max(0) as usize;
        self.seek(to);
    }

    fn left(&self) -> usize {
        self.len() - self.tell()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_overwrites_and_extends() {
        let mut o = Out::new();
        o.be_u32(0);
        o.write(b"abc");
        o.seek(0);
        o.be_u32(7);
        assert_eq!(o.tell(), 4);
        o.seek(9);
        o.putc(1);
        assert_eq!(o.bytes(), &[0, 0, 0, 7, b'a', b'b', b'c', 0, 0, 1]);
    }

    #[test]
    fn little_endian_and_skip() {
        let d = [1u8, 2, 3, 4, 5, 6];
        let mut s = Stream::new(&d);
        assert_eq!(s.le_u16(), Some(0x0201));
        s.skip(2);
        assert_eq!(s.left(), 2);
        assert_eq!(s.le_u32(), None);
        assert!(s.feof());
        s.skip(-6);
        assert_eq!(s.le_u32(), Some(0x0403_0201));
    }
}
