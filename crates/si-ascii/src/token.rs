// SPDX-License-Identifier: MIT
//! The token reader under every ASCII file, and the ASCII file header (finding 0006).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `filetoken.c` (all six of its functions), `headers.c`.

use crate::cnum;
use si_file::Stream;

/// An ASCII file being read: a C stream and the program's one global token buffer.
///
/// The original keeps the last token in the global `SI_dbInToken` (0x08780f40, 1024 bytes) and
/// every reader compares and converts that buffer directly, so the token belongs with the file
/// here. It holds what C string functions see: the bytes up to the first NUL.
pub struct AsciiFile<'a> {
    s: Stream<'a>,
    token: Vec<u8>,
    /// Tokens read so far, for the checks ("did the reader ask for a token after the end?").
    pub tokens_read: usize,
}

/// C's `EOF`.
const EOF: i32 = -1;

impl<'a> AsciiFile<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        AsciiFile { s: Stream::new(data), token: Vec::new(), tokens_read: 0 }
    }

    /// The token buffer as a C string.
    pub fn token(&self) -> &[u8] {
        &self.token
    }

    /// `strcmp(SI_dbInToken, word) == 0`
    pub fn is(&self, word: &str) -> bool {
        self.token == word.as_bytes()
    }

    /// `atoi(SI_dbInToken)`
    pub fn atoi(&self) -> i32 {
        cnum::atoi(&self.token)
    }

    /// `atof(SI_dbInToken)`
    pub fn atof(&self) -> f64 {
        cnum::atof(&self.token)
    }

    /// `(float)atof(SI_dbInToken)`: the readers store most numbers as singles, converting the
    /// double `strtod` returns (two roundings, decimal to double to single).
    pub fn atof32(&self) -> f32 {
        cnum::atof(&self.token) as f32
    }

    pub fn tell(&self) -> usize {
        self.s.tell()
    }

    pub fn len(&self) -> usize {
        self.s.len()
    }

    pub fn is_empty(&self) -> bool {
        self.s.is_empty()
    }

    pub fn feof(&self) -> bool {
        self.s.feof()
    }

    pub fn rest(&self) -> &'a [u8] {
        self.s.rest()
    }

    /// `fgetc`
    pub fn getc(&mut self) -> i32 {
        match self.s.getc() {
            Some(b) => b as i32,
            None => EOF,
        }
    }

    /// `ungetc(c, f)` for the one case the token reader has: `c` is the byte just read. The
    /// byte goes back and the end-of-file flag is cleared, as stdio does. `ungetc(EOF)` does
    /// nothing.
    fn ungetc(&mut self, c: i32) {
        if c != EOF {
            self.s.seek(self.s.tell() - 1);
        }
    }

    /// `rewind`
    pub fn rewind(&mut self) {
        self.s.rewind();
    }

    /// `fseek(f, pos, SEEK_SET)`
    pub fn seek(&mut self, pos: usize) {
        self.s.seek(pos);
    }

    /// `fread` of raw bytes (for the four-byte tag).
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        self.s.read(buf)
    }

    /// Skips spaces, tabs, carriage returns and newlines and returns the first other byte
    /// **as a signed char**, or −1 at the end of the file [soft 0x08404158 SI_fileSkipBlanks].
    /// Because of the sign extension a byte 0xff is indistinguishable from the end of the file
    /// for the callers.
    pub fn skip_blanks(&mut self) -> i32 {
        loop {
            let c = self.getc();
            if c == EOF || !(c == 0x20 || c == 9 || c == 0xd || c == 0xa) {
                return c as u8 as i8 as i32; // EOF (−1) stays −1
            }
        }
    }

    /// The shared body of the two token readers: a token ends at a space, a tab, a newline, the
    /// end of the file, a NUL or 0xff byte, or *before* `delim`, which is left unread.
    /// Returns the terminating character (`EOF`, or what `fgetc` gave), or `None` if the file
    /// ended (or held 0xff) before a token started.
    fn token_body(&mut self, delim: i32) -> Option<i32> {
        self.token.clear();
        self.tokens_read += 1;
        let first = self.skip_blanks();
        if first == -1 {
            return None;
        }
        // The first byte is taken whatever it is, the delimiter included: a `;` that starts a
        // token is the token (and `;abc` is one token). A NUL in first place gives an empty C
        // string: the original still stores the bytes that follow, behind the terminator,
        // where nothing reads them; here they are dropped.
        let mut buf: Vec<u8> = vec![first as u8];
        let c = loop {
            let c = self.getc();
            if c == EOF || c == 0x20 || c == 9 || c == 0xa {
                break c;
            }
            if c == delim {
                break c;
            }
            if c <= 0 || c > 0xfe {
                break c;
            }
            // The original writes into a 1024-byte global with no bound check, so a token of
            // 1024 bytes or more overruns into the globals after it. Here the token just grows.
            buf.push(c as u8);
        };
        if c == delim {
            self.ungetc(c);
        }
        // A carriage return is dropped only when it is the last byte before a newline; CR
        // followed by a space or a tab stays in the token.
        if c == 0xa && buf.last() == Some(&0xd) {
            buf.pop();
        }
        if buf[0] != 0 {
            self.token = buf;
        }
        Some(c)
    }

    /// Reads the next token into the token buffer [soft 0x08404298 SI_fileTokenRead].
    ///
    /// Returns the original's result: true if a token was read **and** something other than
    /// the end of the file (or a 0xff byte) ended it. A last token that runs up to the end of
    /// the file is in the buffer but reported as false; at the end of the file the buffer is
    /// empty and the result false.
    pub fn token_read(&mut self) -> bool {
        match self.token_body(b';' as i32) {
            Some(c) => (c as u32) <= 0xfe,
            None => false,
        }
    }

    /// The general form with a caller-chosen delimiter [soft 0x08404194 fileTokenGet].
    /// The delimiter is a `char` in the original and is sign-extended before it is compared
    /// with `fgetc`'s result, so a delimiter of 0xff matches the end of the file instead of
    /// the byte. The original can also hand back a copy of the token (`strdup`), or a null
    /// pointer when the result is false after a token started: callers here read
    /// [`AsciiFile::token`] instead.
    pub fn token_get(&mut self, delim: u8) -> bool {
        match self.token_body(delim as i8 as i32) {
            Some(c) => (c as u32) <= 0xfe,
            None => false,
        }
    }

    /// Reads tokens until one equals `word` [soft 0x08404350 SI_fileTokenReach]. False if the
    /// file ends first, and also if the matching token is the one that runs into the end of
    /// the file: the result of the read is tested before the comparison.
    pub fn token_reach(&mut self, word: &str, delim: u8) -> bool {
        loop {
            if !self.token_get(delim) {
                return false;
            }
            if self.token == word.as_bytes() {
                return true;
            }
        }
    }

    /// Reads a token and returns its integer value: the readers' `SI_fileTokenRead` + `atoi`.
    pub fn next_int(&mut self) -> i32 {
        self.token_read();
        self.atoi()
    }

    /// Reads a token and returns it as a single: `SI_fileTokenRead` + `(float)atof`.
    pub fn next_f32(&mut self) -> f32 {
        self.token_read();
        self.atof32()
    }

    /// Reads a token and returns a copy of it: `SI_fileTokenRead` + `strdup`.
    pub fn next_string(&mut self) -> Vec<u8> {
        self.token_read();
        self.token.clone()
    }

    /// True when nothing but blanks is left: every token of the file has been consumed.
    pub fn only_blanks_left(&self) -> bool {
        self.s.rest().iter().all(|&b| b == 0x20 || b == 9 || b == 0xd || b == 0xa)
    }
}

/// The start of an ASCII file [soft 0x083fb788 SI_dbfileReadAscHeader].
///
/// Four bytes are read as a big-endian tag. If it is one of the seven accepted tags
/// (`si_file::is_ascii_header_tag`) the next token is read and scanned with `sscanf("%f")`;
/// the result is `(tag, version)`. The version starts as −1.0 and stays −1.0 when the token
/// does not scan (`SDSC` followed by a space, say: the version is then the *next* word, which
/// is not a number). The original also copies the version into the global `SI_versionInput`
/// (0x086793f0); that is the caller's business here.
///
/// For any other tag the file is rewound and the result is `None` (−1 in the original), with
/// the version −1.0.
///
/// A file shorter than four bytes: the original compares four bytes of which some are stale
/// stack. Here that is "not accepted".
pub fn read_asc_header(f: &mut AsciiFile) -> Option<(u32, f32)> {
    let mut version = -1.0f32;
    let mut b = [0u8; 4];
    let tag = if f.read(&mut b) == 4 { Some(u32::from_be_bytes(b)) } else { None };
    match tag {
        Some(t) if si_file::is_ascii_header_tag(t) => {
            f.token_read();
            if let Some(v) = cnum::scan_f(f.token()) {
                version = v;
            }
            Some((t, version))
        }
        _ => {
            f.rewind();
            None
        }
    }
}

/// A NUL-terminated string of a binary file [soft 0x084040a0 SI_stringBinaryRead]: the bytes
/// up to the next NUL or the end of the file. (The same source file as the token reader, and
/// used by the binary readers, not the ASCII ones.)
///
/// The original collects into a 256-byte buffer on its stack with no bound check: a string of
/// 256 bytes or more overruns it. Here the string is returned whole.
pub fn string_binary_read(s: &mut Stream) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(b) = s.getc() {
        if b == 0 {
            break;
        }
        out.push(b);
    }
    out
}

/// Writes a string with its terminating NUL [soft 0x08404104 SI_stringBinaryWrite]. A null
/// pointer (`None`) is written as a lone NUL, and is the one case with a result of false. A
/// string is cut at its first NUL, as a C string is.
pub fn string_binary_write(out: &mut Vec<u8>, s: Option<&[u8]>) -> bool {
    match s {
        None => {
            out.push(0);
            false
        }
        Some(s) => {
            let n = s.iter().position(|&b| b == 0).unwrap_or(s.len());
            out.extend_from_slice(&s[..n]);
            out.push(0);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_strings() {
        let mut out = Vec::new();
        assert!(string_binary_write(&mut out, Some(b"abc")));
        assert!(!string_binary_write(&mut out, None));
        assert!(string_binary_write(&mut out, Some(b"")));
        out.extend_from_slice(b"tail");
        assert_eq!(out, b"abc\0\0\0tail");
        let mut s = Stream::new(&out);
        assert_eq!(string_binary_read(&mut s), b"abc");
        assert_eq!(string_binary_read(&mut s), b"");
        assert_eq!(string_binary_read(&mut s), b"");
        assert_eq!(string_binary_read(&mut s), b"tail"); // ended by the end of the file
        assert_eq!(string_binary_read(&mut s), b"");
    }

    fn tokens(data: &[u8]) -> Vec<(Vec<u8>, bool)> {
        let mut f = AsciiFile::new(data);
        let mut out = Vec::new();
        for _ in 0..50 {
            let ok = f.token_read();
            out.push((f.token().to_vec(), ok));
            if !ok && f.token().is_empty() {
                break;
            }
        }
        out
    }

    fn t(s: &str, ok: bool) -> (Vec<u8>, bool) {
        (s.as_bytes().to_vec(), ok)
    }

    #[test]
    fn blanks_and_terminators() {
        assert_eq!(tokens(b"  ab\tcd\r\nef\n"), vec![t("ab", true), t("cd", true), t("ef", true), t("", false)]);
        // the last token of a file with no newline after it is read but reported as failed
        assert_eq!(tokens(b"ab cd"), vec![t("ab", true), t("cd", false), t("", false)]);
    }

    #[test]
    fn semicolon_is_left_unread_then_starts_a_token() {
        assert_eq!(tokens(b"abc; def;\n"), vec![t("abc", true), t(";", true), t("def", true), t(";", true), t("", false)]);
        // a token that starts with the delimiter takes it, and whatever follows it
        assert_eq!(tokens(b"a;b c\n"), vec![t("a", true), t(";b", true), t("c", true), t("", false)]);
        assert_eq!(tokens(b";;\n"), vec![t(";", true), t(";", true), t("", false)]);
    }

    #[test]
    fn carriage_return_is_dropped_only_before_a_newline() {
        assert_eq!(tokens(b"ab\r\ncd\r ef\n"), vec![t("ab", true), t("cd\r", true), t("ef", true), t("", false)]);
        assert_eq!(tokens(b"ab\r\r\n"), vec![t("ab\r", true), t("", false)]);
    }

    #[test]
    fn nul_and_ff_bytes() {
        // a NUL ends a token and is consumed; the read still succeeds
        assert_eq!(tokens(b"ab\0cd\n"), vec![t("ab", true), t("cd", true), t("", false)]);
        // 0xff ends a token like the end of the file does: token kept, result false
        let got = tokens(b"ab\xffcd\n");
        assert_eq!(got[0], t("ab", false));
        assert_eq!(got[1], t("cd", true));
        // 0xff where a token would start reads as the end of the file, and is consumed
        let mut f = AsciiFile::new(b" \xffab\n");
        assert!(!f.token_read());
        assert_eq!(f.token(), b"");
        assert!(f.token_read());
        assert_eq!(f.token(), b"ab");
        // a NUL in first place: an empty C string, the read succeeds
        let mut f = AsciiFile::new(b"\0ab cd\n");
        assert!(f.token_read());
        assert_eq!(f.token(), b"");
        assert!(f.token_read());
        assert_eq!(f.token(), b"cd");
    }

    #[test]
    fn reach_tests_the_read_before_the_comparison() {
        let mut f = AsciiFile::new(b"a b ELEMENTS c\n");
        assert!(f.token_reach("ELEMENTS", b';'));
        assert!(f.token_read());
        assert_eq!(f.token(), b"c");
        // the word is there, but it is the token that hits the end of the file
        let mut f = AsciiFile::new(b"a b ELEMENTS");
        assert!(!f.token_reach("ELEMENTS", b';'));
        assert_eq!(f.token(), b"ELEMENTS");
        let mut f = AsciiFile::new(b"a b\n");
        assert!(!f.token_reach("ELEMENTS", b';'));
    }

    #[test]
    fn header() {
        let mut f = AsciiFile::new(b"SDSC3.71\nELEMENTS \n");
        assert_eq!(read_asc_header(&mut f), Some((u32::from_be_bytes(*b"SDSC"), 3.71)));
        assert!(f.token_read());
        assert_eq!(f.token(), b"ELEMENTS");
        // not one of the seven tags: rewound
        let mut f = AsciiFile::new(b"SEXP3.71\n");
        assert_eq!(read_asc_header(&mut f), None);
        assert_eq!(f.tell(), 0);
        // the version is the next token, wherever it is; one that is no number leaves −1
        let mut f = AsciiFile::new(b"SSET  2.5 x\n");
        assert_eq!(read_asc_header(&mut f), Some((u32::from_be_bytes(*b"SSET"), 2.5)));
        let mut f = AsciiFile::new(b"SSET\nELEMENTS\n");
        assert_eq!(read_asc_header(&mut f), Some((u32::from_be_bytes(*b"SSET"), -1.0)));
        let mut f = AsciiFile::new(b"SD");
        assert_eq!(read_asc_header(&mut f), None);
    }
}
