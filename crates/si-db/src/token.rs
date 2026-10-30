// SPDX-License-Identifier: MIT
//! The token reader of the text resource files (`filetoken.c`).

use crate::cstr;
use si_file::Stream;

/// A text file being read token by token. The original keeps the current token in one
/// global buffer of 1024 bytes (`SI_dbInToken`, 0x08780f40) and never checks its length;
/// here the token is a vector, so a token longer than the buffer is simply kept whole.
pub struct TokenReader<'a> {
    s: Stream<'a>,
    token: Vec<u8>,
}

impl<'a> TokenReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        TokenReader { s: Stream::new(data), token: Vec::new() }
    }

    /// The current token as the original's string comparisons see it.
    pub fn token(&self) -> &[u8] {
        cstr(&self.token)
    }

    pub fn tell(&self) -> usize {
        self.s.tell()
    }

    /// Skips blanks, tabs, carriage returns and newlines and returns the first other byte
    /// as a signed character [soft 0x08404158 SI_fileSkipBlanks]. The end of the file gives
    /// −1, and so does a byte 0xff: the return type cannot tell the two apart.
    fn skip_blanks(&mut self) -> i32 {
        loop {
            match self.s.getc() {
                None => return -1,
                Some(b' ' | b'\t' | b'\r' | b'\n') => {}
                Some(c) => return c as i8 as i32,
            }
        }
    }

    /// Reads the next token; `sep` is a character that ends a token and stays in the file
    /// for the next read [soft 0x08404194 fileTokenGet].
    ///
    /// The token starts at the first byte that is not white space, whatever it is (a
    /// separator or a zero byte included), and runs until a blank, a tab, a newline, the
    /// separator, a byte outside 1..=0xfe or the end of the file. A carriage return before
    /// the ending newline is dropped. The result is false when there was nothing but white
    /// space, and also when the token was ended by the end of the file or by a byte 0xff:
    /// **the last token of a file with nothing after it is read into the buffer but
    /// reported as a failure.**
    pub fn get(&mut self, sep: u8) -> bool {
        let sep = sep as i8 as i32;
        self.token.clear();
        let first = self.skip_blanks();
        if first == -1 {
            return false;
        }
        self.token.push(first as u8);
        let c = loop {
            let c = self.s.getc().map_or(-1, |b| b as i32);
            if c == -1 || c == 0x20 || c == 9 || c == 10 {
                break c;
            }
            if c == sep {
                break c;
            }
            if c <= 0 || c > 0xfe {
                break c;
            }
            self.token.push(c as u8);
        };
        if c == sep && c != -1 {
            // ungetc: one byte back, end-of-file flag cleared
            let at = self.s.tell();
            self.s.seek(at - 1);
        }
        // (ungetc(EOF) does nothing, so a separator of 0xff, which compares equal to the end of
        // the file, needs no step back)
        if c == 10 && self.token.last() == Some(&b'\r') {
            self.token.pop();
        }
        (c as u32) <= 0xfe
    }

    /// The next token, ended by `;` as well as by white space
    /// [soft 0x08404298 SI_fileTokenRead]: the same code as [`TokenReader::get`] with the
    /// separator fixed.
    pub fn read(&mut self) -> bool {
        self.get(b';')
    }

    /// Reads tokens until one equals `word` [soft 0x08404350 SI_fileTokenReach]. False if a
    /// read fails first, which includes `word` being the very last thing in the file.
    pub fn reach(&mut self, word: &[u8], sep: u8) -> bool {
        loop {
            if !self.get(sep) {
                return false;
            }
            if self.token() == word {
                return true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_separators() {
        let mut t = TokenReader::new(b"SUFFIX hrc \nVERSIONS 4 ");
        let mut seen = Vec::new();
        while t.read() {
            seen.push(t.token().to_vec());
        }
        assert_eq!(seen, [b"SUFFIX".to_vec(), b"hrc".to_vec(), b"VERSIONS".to_vec(), b"4".to_vec()]);
        assert_eq!(t.token(), b""); // a failed read leaves an empty token
    }

    #[test]
    fn last_token_without_a_blank_fails_but_is_kept() {
        let mut t = TokenReader::new(b"VERSIONS 4");
        assert!(t.read());
        assert!(!t.read());
        assert_eq!(t.token(), b"4");
    }

    #[test]
    fn semicolon_ends_a_token_and_starts_the_next() {
        let mut t = TokenReader::new(b"a;b c\r\nd\r e");
        assert!(t.read());
        assert_eq!(t.token(), b"a");
        assert!(t.read());
        assert_eq!(t.token(), b";b");
        assert!(t.read());
        assert_eq!(t.token(), b"c"); // CR before LF dropped
        assert!(t.read());
        assert_eq!(t.token(), b"d\r"); // a CR not followed by LF stays in the token
        assert!(!t.read());
        assert_eq!(t.token(), b"e");
    }

    #[test]
    fn odd_bytes() {
        // a zero byte ends a token with success; 0xff ends it with failure
        let mut t = TokenReader::new(b"ab\0cd\xffef ");
        assert!(t.read());
        assert_eq!(t.token(), b"ab");
        assert!(!t.read());
        assert_eq!(t.token(), b"cd");
        assert!(t.read());
        assert_eq!(t.token(), b"ef");
        // 0xff as the first byte after white space looks like the end of the file
        let mut t = TokenReader::new(b"  \xffabc ");
        assert!(!t.read());
        assert!(t.read());
        assert_eq!(t.token(), b"abc");
    }

    #[test]
    fn reach() {
        let mut t = TokenReader::new(b"# junk here\nDATABASE x\nDATABASE");
        assert!(t.reach(b"DATABASE", b';'));
        assert!(t.read());
        assert_eq!(t.token(), b"x");
        assert!(!t.reach(b"DATABASE", b';')); // found, but as the file's last bytes
        assert_eq!(t.token(), b"DATABASE");
    }
}
