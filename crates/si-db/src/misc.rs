// SPDX-License-Identifier: MIT
//! The remaining small routines of `dbexternal.c` that only look at bytes.

use crate::{Bytes, FileSystem, TokenReader};
use si_file::Stream;

/// `sscanf(s, "%f", &v)` for decimal numbers: leading white space, an optional sign, digits
/// with an optional fraction, an optional exponent. `None` when nothing converts.
/// (The C library also takes `inf`, `nan` and hexadecimal floats; those are not handled.)
pub fn scan_float(s: &[u8]) -> Option<f32> {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let start = i;
    if matches!(s.get(i), Some(b'+' | b'-')) {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let from = *i;
        while *i < s.len() && s[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - from
    };
    let mut n = digits(&mut i);
    if s.get(i) == Some(&b'.') {
        let mut j = i + 1;
        let frac = digits(&mut j);
        if n + frac > 0 {
            i = j;
            n += frac;
        }
    }
    if n == 0 {
        return None;
    }
    if matches!(s.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(s.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        if digits(&mut j) > 0 {
            i = j;
        }
    }
    std::str::from_utf8(&s[start..i]).ok()?.trim_start_matches('+').parse().ok()
}

/// What [`file_read_magic`] finds at the start of a file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileMagic {
    /// The first four bytes as a big-endian number, or −1 for a file that is not recognised.
    pub magic: i32,
    /// The file is a text file (its magic is a four-letter tag).
    pub ascii: bool,
    /// The file's version; −1.0 when it has none or was not recognised.
    pub version: f32,
}

/// Reads the magic number and the version of any database file, binary or text
/// [soft 0x08126994 SI_dbFileReadMagic].
///
/// - A binary magic (0x5380f634 to 0x5380f646): the version is the next four bytes, a
///   big-endian float.
/// - The tag `SSCN` (a scene from before version 2.6): text, and no version is read.
/// - One of 23 other tags (every text tag of the program's table of file types except
///   `SREL`): text; the version is the next token, read as a number.
/// - Anything else: the file is rewound and the result is −1.
///
/// Where this differs: on a file shorter than four bytes the original tests a variable it
/// never set; here such a file is not recognised. A binary file that ends inside the
/// version keeps −1.0 here.
pub fn file_read_magic(s: &mut Stream) -> FileMagic {
    const TAGS: [&[u8; 4]; 23] = [
        b"CLUS", b"SCHN", b"SDAD", b"SDAM", b"SDAT", b"SDIG", b"SDRV", b"SDSC", b"SDT2", b"SDT3", b"SDUT", b"SDVL", b"SFLK",
        b"SHAD", b"SHAM", b"SHAT", b"SHIG", b"SHT2", b"SHT3", b"SHUT", b"SHVL", b"SSET", b"SWAV",
    ];
    let mut out = FileMagic { magic: -1, ascii: false, version: -1.0 };
    let Some(magic) = s.be_u32() else {
        s.rewind();
        return out;
    };
    if magic.wrapping_sub(si_file::MAGIC_GENERAL) < 0x13 {
        out.magic = magic as i32;
        if let Some(v) = s.be_f32() {
            out.version = v;
        }
        return out;
    }
    let tag = magic.to_be_bytes();
    if &tag == b"SSCN" {
        return FileMagic { magic: magic as i32, ascii: true, version: -1.0 };
    }
    if !TAGS.iter().any(|t| **t == tag) {
        s.rewind();
        return out;
    }
    // the version is the rest of the first token: "SDSC3.71" then white space
    let mut t = TokenReader::new(s.rest());
    t.read();
    let consumed = t.tell();
    if let Some(v) = scan_float(t.token()) {
        out.version = v;
    }
    s.seek(s.tell() + consumed);
    FileMagic { magic: magic as i32, ascii: true, version: out.version }
}

/// The path stored in a file [soft 0x08127654 SI_dbfileSeekPath]: the bytes from the file's
/// first slash up to the next zero byte or the end of the file. `None` when the file
/// cannot be opened or holds no slash. (The original copies into a fixed work buffer,
/// unchecked.)
pub fn file_seek_path(fs: &dyn FileSystem, file: &[u8]) -> Option<Bytes> {
    let data = fs.read(file)?;
    let start = data.iter().position(|&c| c == b'/')?;
    let end = data[start..].iter().position(|&c| c == 0).map_or(data.len(), |k| start + k);
    Some(data[start..end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemFs;

    #[test]
    fn floats() {
        assert_eq!(scan_float(b"3.71"), Some(3.71));
        assert_eq!(scan_float(b" 2.6x"), Some(2.6));
        assert_eq!(scan_float(b"4"), Some(4.0));
        assert_eq!(scan_float(b".5e1"), Some(5.0));
        assert_eq!(scan_float(b"+1e"), Some(1.0));
        assert_eq!(scan_float(b"-0.25"), Some(-0.25));
        assert_eq!(scan_float(b"."), None);
        assert_eq!(scan_float(b"abc"), None);
        assert_eq!(scan_float(b""), None);
    }

    #[test]
    fn magics() {
        let mut bin = Vec::new();
        si_file::write_bin_header(&mut bin, si_file::MAGIC_MATERIAL, 3.5, None);
        let mut s = Stream::new(&bin);
        assert_eq!(file_read_magic(&mut s), FileMagic { magic: 0x5380_f640, ascii: false, version: 3.5 });
        assert_eq!(s.tell(), 8);
        let mut s = Stream::new(b"SDSC3.71\nscene");
        assert_eq!(file_read_magic(&mut s), FileMagic { magic: i32::from_be_bytes(*b"SDSC"), ascii: true, version: 3.71 });
        assert_eq!(s.tell(), 9);
        let mut s = Stream::new(b"SSCN2.5\n");
        assert_eq!(file_read_magic(&mut s), FileMagic { magic: i32::from_be_bytes(*b"SSCN"), ascii: true, version: -1.0 });
        assert_eq!(s.tell(), 4);
        for other in [&b"SREL1.0 "[..], b"SEXP1 ", b"PICT", b"ab"] {
            let mut s = Stream::new(other);
            assert_eq!(file_read_magic(&mut s), FileMagic { magic: -1, ascii: false, version: -1.0 });
            assert_eq!(s.tell(), 0);
        }
        let mut s = Stream::new(b"SSETx");
        assert_eq!(file_read_magic(&mut s).version, -1.0);
    }

    #[test]
    fn stored_path() {
        let mut fs = MemFs::new();
        fs.add_file(b"/a", b"\x01\x02xx/usr/db/MODELS/a.1-0\0rest");
        fs.add_file(b"/b", b"no slash here");
        fs.add_file(b"/c", b"x/to/the/end");
        assert_eq!(file_seek_path(&fs, b"/a").unwrap(), b"/usr/db/MODELS/a.1-0");
        assert_eq!(file_seek_path(&fs, b"/b"), None);
        assert_eq!(file_seek_path(&fs, b"/c").unwrap(), b"/to/the/end");
        assert_eq!(file_seek_path(&fs, b"/missing"), None);
    }
}
