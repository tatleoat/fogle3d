// SPDX-License-Identifier: MIT
//! The C library's streams, as far as this package's routines use them: the smallest
//! `FILE` layer that serves the reading of the layout file (`windowReadFile` and the
//! routines it calls) and `si_stringReading`.
//!
//! Nothing here reproduces a function of the program: the program takes these from the C
//! library. A `FILE *` is a handle of `si-mem` (the kind `File`, the emulation harness's
//! own numbering: `stdin` 0x6f000000, `stdout`, `stderr`, then one more every 0x100), the
//! content of a stream is bytes held here, and what a routine reads goes into the memory
//! image at the addresses it passes, as `fscanf` stores through its pointer arguments.
//!
//! What is covered: `fopen` for reading of a named file handed over beforehand
//! ([`Files::add`]; there is no access to the host's files), `fclose`, `fgetc`, `ungetc`,
//! `feof`, `fgets`, `access`, and `fscanf` with the conversions `d i o u x X` (lengths
//! `hh h l ll`), `e f g` (`float`, and `double` with `l`), `s`, `c`, `[set]`, `n`, `%`,
//! assignment suppression and field widths. Each behaves as the harness's stand-in of the
//! same name (`re/tools/emu/standins.py`, read for this), which is what the check runs the
//! original against: a number that runs into a dangling exponent letter ("1.5e") uses the
//! letter up, a failed match gives back one character, the end of the input before the
//! first conversion answers −1. Not covered: writing, seeking, `errno` beyond the one
//! value `access` and `fopen` leave (2, "no such file").

use crate::mem::Mem;
use si_mem::{Handles, Kind};
use std::collections::HashMap;

/// One open stream.
pub struct Stream {
    pub name: Vec<u8>,
    data: Vec<u8>,
    pos: usize,
    /// Characters pushed back, the last one first out.
    unget: Vec<u8>,
    pub eof: bool,
    pub closed: bool,
    readable: bool,
}

impl Stream {
    fn getc(&mut self) -> i32 {
        if let Some(c) = self.unget.pop() {
            return c as i32;
        }
        if !self.readable {
            return -1;
        }
        if self.pos >= self.data.len() {
            self.eof = true;
            return -1;
        }
        self.pos += 1;
        self.data[self.pos - 1] as i32
    }

    fn back(&mut self, c: i32) {
        if c >= 0 {
            self.unget.push(c as u8);
            self.eof = false;
        }
    }

    /// Where the next read takes place, counting what was pushed back.
    pub fn position(&self) -> u32 {
        (self.pos as u32).wrapping_sub(self.unget.len() as u32)
    }
}

/// The streams and the named files they are opened from.
pub struct Files {
    named: HashMap<Vec<u8>, Vec<u8>>,
    open: Handles<Stream>,
    /// What `*__errno_location()` holds.
    pub errno: i32,
}

impl Default for Files {
    fn default() -> Self {
        Files::new()
    }
}

const WS: &[u8] = b" \t\n\x0b\x0c\r";

impl Files {
    /// The three standard streams, in the C library's order.
    pub fn new() -> Files {
        let mut open = Handles::new(Kind::File);
        for (name, readable) in [(&b"stdin"[..], true), (b"stdout", false), (b"stderr", false)] {
            open.open(Stream { name: name.to_vec(), data: Vec::new(), pos: 0, unget: Vec::new(), eof: false, closed: false, readable });
        }
        Files { named: HashMap::new(), open, errno: 0 }
    }

    /// Hand over a file that `fopen` and `access` will find under this name.
    pub fn add(&mut self, path: &[u8], data: Vec<u8>) {
        self.named.insert(path.to_vec(), data);
    }

    /// Open a named file for reading without a call of the program's (a check's set-up for
    /// a routine that takes a `FILE *`); 0 when there is none of the name.
    pub fn open_named(&mut self, path: &[u8]) -> u32 {
        match self.named.get(path) {
            Some(data) => {
                let mut name = b"file:".to_vec();
                name.extend_from_slice(path);
                self.open.open(Stream { name, data: data.clone(), pos: 0, unget: Vec::new(), eof: false, closed: false, readable: true })
            }
            None => 0,
        }
    }

    /// The streams opened beyond the three standard ones: (handle, stream).
    pub fn opened(&self) -> impl Iterator<Item = (u32, &Stream)> {
        self.open.iter().skip(3)
    }

    /// A stream by its handle. Anything that is not an open stream stops the routine, as
    /// the check's stand-ins stop the original there (in the C library it is undefined):
    /// by a read of address 0, which the image answers with its fault.
    fn stream(&mut self, m: &Mem, handle: u32) -> &mut Stream {
        let usable = matches!(self.open.get(handle), Some(s) if !s.closed);
        if !usable {
            m.u8(0);
        }
        self.open.get_mut(handle).expect("the fault above stops a check; outside one there is no stream to go on with")
    }

    /// `fopen(path, mode)`: only reading; null and `errno` 2 when the file does not exist.
    pub fn fopen(&mut self, m: &mut Mem, path: u32, mode: u32) -> u32 {
        let (path, mode) = (m.cstr(path), m.cstr(mode));
        if mode.contains(&b'r') {
            let handle = self.open_named(&path);
            if handle == 0 {
                self.errno = 2;
            }
            return handle;
        }
        // writing is not part of this layer
        m.u8(0);
        0
    }

    /// `fclose(stream)`.
    pub fn fclose(&mut self, m: &mut Mem, stream: u32) -> i32 {
        self.stream(m, stream).closed = true;
        0
    }

    /// `fgetc(stream)`: the next character, −1 at the end (which sets the end mark).
    pub fn fgetc(&mut self, m: &mut Mem, stream: u32) -> i32 {
        self.stream(m, stream).getc()
    }

    /// `ungetc(c, stream)`: −1 is refused; anything else is pushed back as a character and
    /// clears the end mark.
    pub fn ungetc(&mut self, m: &mut Mem, c: i32, stream: u32) -> i32 {
        if c == -1 {
            return -1;
        }
        let s = self.stream(m, stream);
        s.unget.push(c as u8);
        s.eof = false;
        c & 0xff
    }

    /// `feof(stream)`.
    pub fn feof(&mut self, m: &mut Mem, stream: u32) -> i32 {
        self.stream(m, stream).eof as i32
    }

    /// `fgets(buffer, n, stream)`: up to n − 1 characters through a line feed, with a
    /// terminating zero; null (and nothing stored) when there was nothing to read.
    pub fn fgets(&mut self, m: &mut Mem, buffer: u32, n: i32, stream: u32) -> u32 {
        let s = self.stream(m, stream);
        let mut out = Vec::new();
        while (out.len() as i64) < n as i64 - 1 {
            let c = s.getc();
            if c < 0 {
                break;
            }
            out.push(c as u8);
            if c == 10 {
                break;
            }
        }
        if out.is_empty() {
            return 0;
        }
        out.push(0);
        m.set_bytes(buffer, &out);
        buffer
    }

    /// `access(path, mode)`: 0 for a file that exists, else −1 and `errno` 2.
    pub fn access(&mut self, m: &mut Mem, path: u32, _mode: i32) -> i32 {
        let path = m.cstr(path);
        if self.named.contains_key(&path) {
            0
        } else {
            self.errno = 2;
            -1
        }
    }

    /// `fscanf(stream, format, …)`: `args` are the pointer arguments in order, addresses of
    /// the image. The number of assignments, or −1 at the end of the input before the
    /// first one.
    #[allow(unused_assignments)] // the count of characters is only read by %n
    pub fn fscanf(&mut self, m: &mut Mem, stream: u32, format: &[u8], args: &[u32]) -> i32 {
        let s = self.stream(m, stream);
        let mut args = args.iter().copied();
        let mut next_arg = move || args.next().expect("fscanf: a pointer argument for every conversion");
        let (mut done, mut count) = (0i32, 0u32);
        let mut pos = 0usize;
        macro_rules! next {
            () => {{
                let c = s.getc();
                if c >= 0 {
                    count += 1;
                }
                c
            }};
        }
        macro_rules! back {
            ($c:expr) => {{
                let c = $c;
                if c >= 0 {
                    s.back(c);
                    count = count.wrapping_sub(1);
                }
            }};
        }
        macro_rules! skip_ws {
            () => {{
                let mut c = next!();
                while c >= 0 && WS.contains(&(c as u8)) {
                    c = next!();
                }
                c
            }};
        }
        let is_in = |c: i32, set: &[u8]| c >= 0 && set.contains(&(c as u8));
        while pos < format.len() {
            let ch = format[pos];
            if WS.contains(&ch) {
                back!(skip_ws!());
                pos += 1;
                continue;
            }
            if ch != b'%' {
                let c = next!();
                if c != ch as i32 {
                    back!(c);
                    return if done != 0 || c >= 0 { done } else { -1 };
                }
                pos += 1;
                continue;
            }
            // %[*][width][hh|h|ll|l|L|q]conversion
            pos += 1;
            let skip = format.get(pos) == Some(&b'*');
            if skip {
                pos += 1;
            }
            let mut width: Option<usize> = None;
            while let Some(d) = format.get(pos).filter(|d| d.is_ascii_digit()) {
                width = Some(width.unwrap_or(0) * 10 + (d - b'0') as usize);
                pos += 1;
            }
            let mut length: &[u8] = b"";
            for l in [&b"hh"[..], b"h", b"ll", b"l", b"L", b"q"] {
                if format[pos..].starts_with(l) {
                    length = l;
                    pos += l.len();
                    break;
                }
            }
            let conv = *format.get(pos).expect("fscanf: a conversion after %");
            let mut set: &[u8] = b"";
            if conv == b'[' {
                // the set runs to the first ']' that is not its first member
                let start = pos + 1;
                let mut end = start;
                if format.get(end) == Some(&b'^') {
                    end += 1;
                }
                if format.get(end) == Some(&b']') {
                    end += 1;
                }
                while format.get(end).is_some_and(|c| *c != b']') {
                    end += 1;
                }
                set = &format[start..end];
                pos = end + 1;
            } else {
                pos += 1;
            }
            match conv {
                b'%' => {
                    let c = skip_ws!();
                    if c != b'%' as i32 {
                        back!(c);
                        return if done != 0 || c >= 0 { done } else { -1 };
                    }
                }
                b'n' => {
                    if !skip {
                        m.set_u32(next_arg(), count);
                    }
                }
                b'c' => {
                    let n = width.unwrap_or(1).max(1);
                    let mut buf = Vec::new();
                    while buf.len() < n {
                        let c = next!();
                        if c < 0 {
                            break;
                        }
                        buf.push(c as u8);
                    }
                    if buf.len() < n {
                        return if done != 0 { done } else { -1 };
                    }
                    if !skip {
                        m.set_bytes(next_arg(), &buf);
                        done += 1;
                    }
                }
                b'[' => {
                    let (negated, body) = match set.strip_prefix(b"^") {
                        Some(rest) => (true, rest),
                        None => (false, set),
                    };
                    let mut allowed = [false; 256];
                    let mut i = 0;
                    while i < body.len() {
                        if i + 2 < body.len() && body[i + 1] == b'-' {
                            for c in body[i]..=body[i + 2] {
                                allowed[c as usize] = true;
                            }
                            i += 3;
                        } else {
                            allowed[body[i] as usize] = true;
                            i += 1;
                        }
                    }
                    let mut buf = Vec::new();
                    let mut last = 0i32;
                    while width.is_none_or(|w| buf.len() < w) {
                        let c = next!();
                        last = c;
                        if c < 0 || allowed[c as usize] == negated {
                            back!(c);
                            break;
                        }
                        buf.push(c as u8);
                    }
                    if buf.is_empty() {
                        return if done != 0 || last >= 0 { done } else { -1 };
                    }
                    if !skip {
                        buf.push(0);
                        m.set_bytes(next_arg(), &buf);
                        done += 1;
                    }
                }
                b's' | b'd' | b'i' | b'o' | b'u' | b'x' | b'X' | b'e' | b'E' | b'f' | b'g' | b'G' => {
                    let mut c = skip_ws!();
                    if c < 0 {
                        return if done != 0 { done } else { -1 };
                    }
                    let limit = width.unwrap_or(1 << 30);
                    let mut buf: Vec<u8> = Vec::new();
                    if conv == b's' {
                        while c >= 0 && !WS.contains(&(c as u8)) && buf.len() < limit {
                            buf.push(c as u8);
                            c = next!();
                        }
                        back!(c);
                        if !skip {
                            buf.push(0);
                            m.set_bytes(next_arg(), &buf);
                            done += 1;
                        }
                    } else if b"diouxX".contains(&conv) {
                        let mut base: u32 = match conv {
                            b'o' => 8,
                            b'x' | b'X' => 16,
                            b'i' => 0,
                            _ => 10,
                        };
                        let mut negative = false;
                        if is_in(c, b"+-") && buf.len() < limit {
                            negative = c == b'-' as i32;
                            buf.push(c as u8);
                            c = next!();
                        }
                        let mut digits: Vec<u8> = Vec::new();
                        if c == b'0' as i32 && (base == 0 || base == 16) && buf.len() < limit {
                            buf.push(b'0');
                            digits.push(b'0');
                            c = next!();
                            if is_in(c, b"xX") && buf.len() < limit {
                                buf.push(c as u8);
                                base = 16;
                                digits.clear();
                                c = next!();
                            } else if base == 0 {
                                base = 8;
                            }
                        }
                        if base == 0 {
                            base = 10;
                        }
                        while c >= 0 && (c as u8 as char).to_digit(base).is_some() && buf.len() < limit {
                            buf.push(c as u8);
                            digits.push(c as u8);
                            c = next!();
                        }
                        back!(c);
                        if digits.is_empty() {
                            return done;
                        }
                        // the value, as large as the digits make it (the stand-in computes
                        // in unbounded integers); clamped below for signed 32 and 64 bits
                        let mut v: i128 = 0;
                        for d in &digits {
                            v = v.saturating_mul(base as i128).saturating_add((*d as char).to_digit(base).unwrap() as i128);
                        }
                        if negative {
                            v = -v;
                        }
                        if !skip {
                            let size: u32 = match length {
                                b"ll" | b"q" => 8,
                                b"h" => 2,
                                b"hh" => 1,
                                _ => 4,
                            };
                            if (conv == b'd' || conv == b'i') && size >= 4 {
                                let top = (1i128 << (8 * size - 1)) - 1;
                                v = v.clamp(-top - 1, top);
                            }
                            let bytes = (v as u128 as u64).to_le_bytes();
                            m.set_bytes(next_arg(), &bytes[..size as usize]);
                            done += 1;
                        }
                    } else {
                        // e f g: sign, digits, one decimal point, digits, then an exponent
                        // letter with an optional sign and digits
                        if is_in(c, b"+-") && buf.len() < limit {
                            buf.push(c as u8);
                            c = next!();
                        }
                        let mut digits = 0;
                        while is_in(c, b"0123456789") && buf.len() < limit {
                            buf.push(c as u8);
                            digits += 1;
                            c = next!();
                        }
                        if c == b'.' as i32 && buf.len() < limit {
                            buf.push(b'.');
                            c = next!();
                            while is_in(c, b"0123456789") && buf.len() < limit {
                                buf.push(c as u8);
                                digits += 1;
                                c = next!();
                            }
                        }
                        if digits == 0 {
                            back!(c);
                            return done;
                        }
                        let mut number = buf.clone();
                        if is_in(c, b"eE") && buf.len() < limit {
                            // the letter and a sign after it are used up even when no
                            // exponent digits follow
                            c = next!();
                            let mut sign = Vec::new();
                            if is_in(c, b"+-") && buf.len() + 1 < limit {
                                sign.push(c as u8);
                                c = next!();
                            }
                            let mut exponent = Vec::new();
                            while is_in(c, b"0123456789") && buf.len() + 1 + sign.len() + exponent.len() < limit {
                                exponent.push(c as u8);
                                c = next!();
                            }
                            if !exponent.is_empty() {
                                number.push(b'e');
                                number.extend_from_slice(&sign);
                                number.extend_from_slice(&exponent);
                            }
                        }
                        back!(c);
                        if !skip {
                            // a decimal string rounded once to the type: the host's parser
                            // does that, with a number that ends in "." or begins with one
                            let text = String::from_utf8_lossy(&number).into_owned();
                            let text = if text.ends_with('.') { format!("{text}0") } else { text };
                            if length == b"l" {
                                m.set_f64(next_arg(), text.parse::<f64>().unwrap_or(0.0));
                            } else if length.is_empty() || length == b"h" || length == b"hh" {
                                m.set_f32(next_arg(), text.parse::<f32>().unwrap_or(0.0));
                            } else {
                                // long double: not in this layer
                                m.u8(0);
                            }
                            done += 1;
                        }
                    }
                }
                _ => {
                    // a conversion this layer does not have
                    m.u8(0);
                }
            }
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(data: &[u8]) -> (Files, Mem, u32) {
        let mut f = Files::new();
        f.add(b"x", data.to_vec());
        let h = f.open_named(b"x");
        (f, crate::mem::image(), h)
    }

    #[test]
    fn handles_are_the_harness_numbers() {
        let (_, _, h) = with(b"");
        assert_eq!(h, 0x6f00_0300);
    }

    #[test]
    fn numbers_words_and_the_end() {
        let (mut f, mut m, h) = with(b"WIND_ID: 1002 -7 1.5e Front\n 0x1f 12");
        let a = crate::mem::ARENA.0;
        assert_eq!(f.fscanf(&mut m, h, b"%*s %hd", &[a]), 1);
        assert_eq!(m.i16(a), 1002);
        assert_eq!(f.fscanf(&mut m, h, b"%d %f %s", &[a + 4, a + 8, a + 0x10]), 3);
        assert_eq!(m.i32(a + 4), -7);
        assert_eq!(m.f32(a + 8), 1.5); // the dangling "e" is used up
        assert_eq!(m.cstr(a + 0x10), b"Front");
        assert_eq!(f.fgetc(&mut m, h), 10);
        assert_eq!(f.fscanf(&mut m, h, b"%i%2d", &[a + 0x20, a + 0x24]), 2);
        assert_eq!((m.i32(a + 0x20), m.i32(a + 0x24)), (0x1f, 12));
        // the number ran to the end of the file: the look at the next character set the mark
        assert_eq!(f.feof(&mut m, h), 1);
        assert_eq!(f.fscanf(&mut m, h, b"%d", &[a]), -1);
        assert_eq!(f.feof(&mut m, h), 1);
        assert_eq!(f.ungetc(&mut m, b'#' as i32, h), b'#' as i32);
        assert_eq!(f.feof(&mut m, h), 0);
    }

    #[test]
    fn lines() {
        let (mut f, mut m, h) = with(b"\nSecond line\nlast");
        let a = crate::mem::ARENA.0;
        assert_eq!(f.fgets(&mut m, a, 0x50, h), a);
        assert_eq!(m.cstr(a), b"\n");
        assert_eq!(f.fgets(&mut m, a, 8, h), a);
        assert_eq!(m.cstr(a), b"Second ");
        assert_eq!(f.fgets(&mut m, a, 0x50, h), a);
        assert_eq!(f.fgets(&mut m, a, 0x50, h), a);
        assert_eq!(m.cstr(a), b"last");
        assert_eq!(f.fgets(&mut m, a, 0x50, h), 0);
        assert_eq!(f.access(&mut m, a, 4), -1);
        assert_eq!(f.errno, 2);
    }
}
