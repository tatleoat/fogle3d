// SPDX-License-Identifier: MIT
//! The channel setup file (`.chn`, tag `SCHN`): reader and writer (finding 0006).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `chn_file.c`, with the pieces of `chn_channel.c`,
//! `custom_dialog.c` and `in_out.c` the reader goes through.
//!
//! Unlike the scene and setup readers this one does not use the program's token reader. After
//! the ASCII header it reads the stream with C's `fscanf`, `fgetc` and `feof`, so the parsing
//! rules are those of the C library ([`scan`]), and several of its loops only end at a newline:
//! a file cut short inside such a line makes the original read the end of the file for ever.
//! Here that is [`ChnError::EndlessLoop`].
//!
//! What is **not** reproduced, because it is state of the running program and not of the file:
//!
//! * the channel driver itself. A driver is an executable in the driver directory which the
//!   program starts and which sends back its description (`si_chnDrvGetInfo`, 0x085731d0);
//!   this port has none, so wherever the original asks for one the answer is "does not exist";
//! * the driver's custom dialog. The lines between `_DATA` and `_END` are read here exactly as
//!   the original reads them, but the original then stores them into the dialog of the installed
//!   driver (`SI_cusParseDataString`, 0x0852487c) and writes them back from that dialog
//!   (`SI_cusEncodeDataString`, 0x08525b88). Here the lines are kept as text;
//! * expressions. A connection is an expression which the original parses after the read
//!   (`SI_chnUpdateItems`, 0x08568b58) and prints from the parsed form when it writes. Here the
//!   text is kept. Connections of files older than 3.0 are converted to expression text by the
//!   original with names from the attribute dictionary; here their fields are kept
//!   ([`Connection::Legacy`]) and such a setup cannot be written.

use crate::cnum;
use crate::token::read_asc_header;

/// C's stdio as the two setup readers use it: `fgetc`, `ungetc`, `feof`, `ftell`, `fseek` and
/// `fscanf`, and the fixed-point half of `printf`.
///
/// These are glibc functions, not Softimage ones (the binary needs `GLIBC_2.2`), so nothing here
/// carries a citation; the readers that call them do. `fscanf` follows glibc 2.2's `vfscanf`
/// directive by directive, from the glibc sources as remembered, not from the binary (libc is
/// linked dynamically). What the files depend on is plain: `%s` skips white space and stops
/// before the next white space, `%d` and `%f` take the longest text that can start a number, a
/// bracket set takes at least one character, and the result is the number of values stored.
pub mod scan {
    use crate::cnum;
    use crate::token::AsciiFile;

    /// C's `EOF`.
    pub const EOF: i32 = -1;

    /// `isspace` in the C locale, for a value `fgetc` returned.
    pub fn is_space(c: i32) -> bool {
        matches!(c, 0x20 | 9 | 10 | 11 | 12 | 13)
    }

    fn is_digit(c: i32) -> bool {
        (b'0' as i32..=b'9' as i32).contains(&c)
    }

    fn lower(c: i32) -> i32 {
        if (b'A' as i32..=b'Z' as i32).contains(&c) {
            c + 32
        } else {
            c
        }
    }

    /// Where a conversion stores its value. A conversion that fails leaves it untouched, as C
    /// leaves the caller's variable.
    pub enum Arg<'b> {
        /// `char *`: receives the bytes up to the first NUL (what C string functions would see
        /// in the caller's buffer). The original's buffers have fixed sizes; here they grow.
        Str(&'b mut Vec<u8>),
        /// `int *`
        Int(&'b mut i32),
        /// `float *`
        Float(&'b mut f32),
        /// `char *` of a `%c`
        Char(&'b mut u8),
        /// `short *` or `unsigned short *` of a `%hd` or `%hu`: the low 16 bits of the number.
        Half(&'b mut u16),
    }

    /// A `FILE *` open for reading.
    pub struct CFile<'a> {
        pub f: AsciiFile<'a>,
        /// For the checks: how many times the caller wanted input and the file had none left
        /// (a `fscanf` that stopped at the end of the file before a value, a keyword or a line
        /// asked for with the end-of-file flag already set). The end of the file right after
        /// a value is not counted.
        pub starved: usize,
    }

    impl<'a> CFile<'a> {
        pub fn new(data: &'a [u8]) -> Self {
            CFile { f: AsciiFile::new(data), starved: 0 }
        }

        /// `fgetc`
        pub fn getc(&mut self) -> i32 {
            self.f.getc()
        }

        /// `ungetc(c, f)` for the byte just read: it goes back and the end-of-file flag is
        /// cleared. `ungetc(EOF)` does nothing.
        pub fn ungetc(&mut self, c: i32) {
            if c != EOF {
                let pos = self.f.tell();
                self.f.seek(pos - 1);
            }
        }

        pub fn feof(&self) -> bool {
            self.f.feof()
        }

        /// `ftell`
        pub fn tell(&self) -> usize {
            self.f.tell()
        }

        /// `fseek(f, pos, SEEK_SET)`
        pub fn seek(&mut self, pos: usize) {
            self.f.seek(pos);
        }

        /// `fscanf(f, fmt, …)`. Supports what the readers' formats use: white space and
        /// literal characters, `%s`, `%d`, `%u`, `%f`/`%g`/`%e`, `%c`, `%[…]`, a field width,
        /// `*` and the length `h` (the raw function-curve reader's `%hu`).
        /// The result is C's: the number of values stored, or `EOF` when the input ended before
        /// the first one.
        pub fn scanf(&mut self, fmt: &str, args: &mut [Arg]) -> i32 {
            let fmt = fmt.as_bytes();
            let mut done: i32 = 0;
            let mut c: i32 = 0; // the character last read; once EOF, no further read is made
            let mut skip_space = false;
            let mut arg = 0usize;
            let mut i = 0usize;
            macro_rules! inchar {
                () => {{
                    if c != EOF {
                        c = self.getc();
                    }
                    c
                }};
            }
            // an input failure reports EOF only if nothing was stored yet
            macro_rules! input_error {
                () => {{
                    self.starved += 1;
                    return if done != 0 { done } else { EOF };
                }};
            }
            macro_rules! conv_error {
                () => {
                    return done
                };
            }
            while i < fmt.len() {
                let fc = fmt[i] as i32;
                i += 1;
                if fc != b'%' as i32 {
                    if is_space(fc) {
                        skip_space = true;
                        continue;
                    }
                    // a literal character must match the next one of the input
                    if inchar!() == EOF {
                        input_error!();
                    }
                    if skip_space {
                        while is_space(c) {
                            inchar!();
                        }
                        skip_space = false;
                    }
                    if c != fc {
                        self.ungetc(c);
                        conv_error!();
                    }
                    continue;
                }
                let mut suppress = false;
                let mut width: i32 = -1;
                if i < fmt.len() && fmt[i] == b'*' {
                    suppress = true;
                    i += 1;
                }
                if i < fmt.len() && fmt[i].is_ascii_digit() {
                    let mut w: i32 = 0;
                    while i < fmt.len() && fmt[i].is_ascii_digit() {
                        w = w * 10 + (fmt[i] - b'0') as i32;
                        i += 1;
                    }
                    width = if w == 0 { -1 } else { w };
                }
                // the length `h`: the value is stored in a short
                let half = i < fmt.len() && fmt[i] == b'h';
                if half {
                    i += 1;
                }
                if i >= fmt.len() {
                    conv_error!();
                }
                let conv = fmt[i];
                i += 1;
                if skip_space || !matches!(conv, b'[' | b'c') {
                    loop {
                        inchar!();
                        if !is_space(c) {
                            break;
                        }
                    }
                    self.ungetc(c);
                    skip_space = false;
                }
                match conv {
                    b's' => {
                        if inchar!() == EOF {
                            input_error!();
                        }
                        let mut buf: Vec<u8> = Vec::new();
                        loop {
                            if is_space(c) {
                                self.ungetc(c);
                                break;
                            }
                            buf.push(c as u8);
                            if width > 0 {
                                width -= 1;
                                if width == 0 {
                                    break;
                                }
                            }
                            if inchar!() == EOF {
                                break;
                            }
                        }
                        if !suppress {
                            store_str(args, &mut arg, buf);
                            done += 1;
                        }
                    }
                    b'c' => {
                        if inchar!() == EOF {
                            input_error!();
                        }
                        // only a width of one is used; a wider `%c` would store several bytes
                        if !suppress {
                            if let Some(Arg::Char(p)) = args.get_mut(arg) {
                                **p = c as u8;
                            }
                            arg += 1;
                            done += 1;
                        }
                    }
                    b'd' | b'u' => {
                        if inchar!() == EOF {
                            input_error!();
                        }
                        let mut wp: Vec<u8> = Vec::new();
                        if c == b'-' as i32 || c == b'+' as i32 {
                            wp.push(c as u8);
                            if width > 0 {
                                width -= 1;
                            }
                            inchar!();
                        }
                        while c != EOF && width != 0 {
                            if !is_digit(c) {
                                break;
                            }
                            wp.push(c as u8);
                            if width > 0 {
                                width -= 1;
                            }
                            inchar!();
                        }
                        // the character that ended the number goes back; a sign with no digit
                        // after it stays consumed
                        self.ungetc(c);
                        if !wp.iter().any(|b| b.is_ascii_digit()) {
                            conv_error!();
                        }
                        if !suppress {
                            // strtol for `%d`, strtoul for `%u`, base 10
                            let v = if conv == b'u' { cnum::strtoul(&wp) } else { cnum::atoi(&wp) as u32 };
                            match args.get_mut(arg) {
                                Some(Arg::Int(p)) if !half => **p = v as i32,
                                Some(Arg::Half(p)) if half => **p = v as u16,
                                _ => {}
                            }
                            arg += 1;
                            done += 1;
                        }
                    }
                    b'f' | b'g' | b'e' | b'E' | b'G' => {
                        if inchar!() == EOF {
                            input_error!();
                        }
                        let mut negative = false;
                        if c == b'-' as i32 || c == b'+' as i32 {
                            negative = c == b'-' as i32;
                            if width == 0 || inchar!() == EOF {
                                conv_error!();
                            }
                            if !is_digit(c) && c != b'.' as i32 {
                                // no number after the sign: the sign stays consumed
                                self.ungetc(c);
                                input_error!();
                            }
                            if width > 0 {
                                width -= 1;
                            }
                        }
                        let mut wp: Vec<u8> = Vec::new();
                        let mut special = false;
                        if lower(c) == b'n' as i32 {
                            // "nan"
                            wp.push(c as u8);
                            for want in [b'a', b'n'] {
                                if width == 0 || inchar!() == EOF || lower(c) != want as i32 {
                                    input_error!();
                                }
                                if width > 0 {
                                    width -= 1;
                                }
                                wp.push(c as u8);
                            }
                            special = true;
                        } else if lower(c) == b'i' as i32 {
                            // "inf" or "infinity"
                            wp.push(c as u8);
                            for want in [b'n', b'f'] {
                                if width == 0 || inchar!() == EOF || lower(c) != want as i32 {
                                    input_error!();
                                }
                                if width > 0 {
                                    width -= 1;
                                }
                                wp.push(c as u8);
                            }
                            if width != 0 && inchar!() != EOF {
                                if lower(c) == b'i' as i32 {
                                    if width > 0 {
                                        width -= 1;
                                    }
                                    wp.push(c as u8);
                                    for want in [b'n', b'i', b't', b'y'] {
                                        if width == 0 || inchar!() == EOF || lower(c) != want as i32 {
                                            input_error!();
                                        }
                                        if width > 0 {
                                            width -= 1;
                                        }
                                        wp.push(c as u8);
                                    }
                                } else {
                                    self.ungetc(c);
                                }
                            }
                            special = true;
                        }
                        if !special {
                            let mut is_hexa = false;
                            let mut exp_char = b'e';
                            if width != 0 && c == b'0' as i32 {
                                wp.push(b'0');
                                inchar!();
                                if width > 0 {
                                    width -= 1;
                                }
                                if width != 0 && lower(c) == b'x' as i32 {
                                    wp.push(c as u8);
                                    is_hexa = true;
                                    exp_char = b'p';
                                    inchar!();
                                    if width > 0 {
                                        width -= 1;
                                    }
                                }
                            }
                            let (mut got_dot, mut got_e) = (false, false);
                            loop {
                                // a digit, a hexadecimal digit of a hexadecimal mantissa, or the
                                // sign right after the exponent letter
                                if is_digit(c)
                                    || (!got_e && is_hexa && c != EOF && (c as u8).is_ascii_hexdigit())
                                    || (got_e && wp.last() == Some(&exp_char) && (c == b'-' as i32 || c == b'+' as i32))
                                {
                                    wp.push(c as u8);
                                } else if !wp.is_empty() && !got_e && lower(c) == exp_char as i32 {
                                    wp.push(exp_char);
                                    got_e = true;
                                    got_dot = true;
                                } else if c == b'.' as i32 && !got_dot {
                                    wp.push(b'.');
                                    got_dot = true;
                                } else {
                                    // not part of the number any more
                                    self.ungetc(c);
                                    break;
                                }
                                if width > 0 {
                                    width -= 1;
                                }
                                if width == 0 || inchar!() == EOF {
                                    break;
                                }
                            }
                            if wp.is_empty() || (is_hexa && (wp.len() == 2 || !got_e)) {
                                conv_error!();
                            }
                        }
                        // strtof on what was gathered: the text rounded once to a single. Text
                        // such as "1e" or "." is consumed whole even though only "1", or
                        // nothing, converts.
                        let v = match cnum::scan_f(&wp) {
                            Some(v) => v,
                            None => conv_error!(),
                        };
                        if !suppress {
                            if let Some(Arg::Float(p)) = args.get_mut(arg) {
                                **p = if negative { -v } else { v };
                            }
                            arg += 1;
                            done += 1;
                        }
                    }
                    b'[' => {
                        let mut not_in = false;
                        if i < fmt.len() && fmt[i] == b'^' {
                            not_in = true;
                            i += 1;
                        }
                        if width < 0 {
                            width = i32::MAX;
                        }
                        let mut set = [false; 256];
                        if i < fmt.len() && (fmt[i] == b']' || fmt[i] == b'-') {
                            set[fmt[i] as usize] = true;
                            i += 1;
                        }
                        loop {
                            if i >= fmt.len() {
                                conv_error!(); // no closing bracket in the format
                            }
                            let ch = fmt[i];
                            i += 1;
                            if ch == b']' {
                                break;
                            }
                            if ch == b'-' && i < fmt.len() && fmt[i] != b']' && fmt[i - 2] <= fmt[i] {
                                for x in fmt[i - 2]..fmt[i] {
                                    set[x as usize] = true;
                                }
                            } else {
                                set[ch as usize] = true;
                            }
                        }
                        if inchar!() == EOF {
                            input_error!();
                        }
                        let mut buf: Vec<u8> = Vec::new();
                        loop {
                            if set[c as usize] == not_in {
                                self.ungetc(c);
                                break;
                            }
                            buf.push(c as u8);
                            width -= 1;
                            if width <= 0 || inchar!() == EOF {
                                break;
                            }
                        }
                        if buf.is_empty() {
                            conv_error!();
                        }
                        if !suppress {
                            store_str(args, &mut arg, buf);
                            done += 1;
                        }
                    }
                    _ => conv_error!(),
                }
            }
            if skip_space {
                loop {
                    inchar!();
                    if !is_space(c) {
                        break;
                    }
                }
                self.ungetc(c);
            }
            done
        }
    }

    fn store_str(args: &mut [Arg], arg: &mut usize, mut buf: Vec<u8>) {
        if let Some(n) = buf.iter().position(|&b| b == 0) {
            buf.truncate(n);
        }
        if let Some(Arg::Str(p)) = args.get_mut(*arg) {
            **p = buf;
        }
        *arg += 1;
    }

    /// `sscanf(s, "%s", out)` on a C string: the first run of characters that are not white
    /// space, or `None` when there is none (the caller's buffer is then left as it was).
    pub fn first_word(s: &[u8]) -> Option<&[u8]> {
        let s = c_str(s);
        let start = s.iter().position(|&b| !is_space(b as i32))?;
        let len = s[start..].iter().position(|&b| is_space(b as i32)).unwrap_or(s.len() - start);
        Some(&s[start..start + len])
    }

    /// The bytes a C string function sees: up to the first NUL.
    pub fn c_str(s: &[u8]) -> &[u8] {
        &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]
    }

    /// `printf("%*.{precision}f", width, v)`: fixed notation, right-aligned in `width`.
    pub fn fmt_f(v: f64, width: usize, precision: usize) -> String {
        let s = if v.is_nan() {
            if v.is_sign_negative() { "-nan".to_string() } else { "nan".to_string() }
        } else if v.is_infinite() {
            if v < 0.0 { "-inf".to_string() } else { "inf".to_string() }
        } else {
            format!("{:.*}", precision, v)
        };
        format!("{:>1$}", s, width)
    }

    /// `printf("%s", p)` for a pointer that may be null: glibc prints `(null)`.
    pub fn fmt_s(p: &Option<Vec<u8>>) -> &[u8] {
        match p {
            Some(s) => c_str(s),
            None => b"(null)",
        }
    }
}

use scan::{c_str, first_word, fmt_f, fmt_s, is_space, Arg, CFile, EOF};

/// The keywords of the file and of the driver configuration file, with the numbers the readers
/// switch on [soft 0x08571a80 tokenType] (the table is the global `si_tokLook`, 0x086eca00).
/// `RUNMODE` and its older spelling `FREERUN` share a number.
pub const KEYWORDS: [(&str, i32); 44] = [
    ("CHANNELS", 2),
    ("#", 1),
    ("ON", 3),
    ("OFF", 4),
    ("DRIVER", 5),
    ("DRIVER_SYNC", 6),
    ("DRIVER_DEBUG", 7),
    ("CHANNEL", 8),
    ("ITEM", 9),
    ("MODEL", 10),
    ("WAVE", 11),
    ("CAMERA", 12),
    ("LIGHT", 13),
    ("LIGHT_POINT", 14),
    ("LIGHT_SPOT", 15),
    ("LIGHT_INFINITE", 16),
    ("LIGHT_SUN", 17),
    ("MATERIAL", 18),
    ("TEXTURE_2D", 19),
    ("TEXTURE_3D", 20),
    ("Softimage", 21),
    ("FRAMERATE", 22),
    ("TIMESTAMP", 23),
    ("REFRESH", 24),
    ("FAST_INPUT", 25),
    ("CALIBRATE", 26),
    ("CHANNEL_DRIVER", 27),
    ("DOF_NUM", 28),
    ("DOF_NAME", 29),
    ("DOF_TYPE", 30),
    ("PORT_NUM", 31),
    ("PORT", 32),
    ("ERROR_NUM", 33),
    ("ERROR", 34),
    ("RUNMODE", 35),
    ("FREERUN", 35),
    ("SYNCED", 36),
    ("FREE", 37),
    ("FREQUENCY", 38),
    ("REMOTE", 39),
    ("CAPTURE", 40),
    ("CONTROL", 41),
    ("TAKE_REC", 42),
    ("_END", 43),
];

const T_COMMENT: i32 = 1;
const T_CHANNELS: i32 = 2;
const T_ON: i32 = 3;
const T_DRIVER: i32 = 5;
const T_DRIVER_SYNC: i32 = 6;
const T_DRIVER_DEBUG: i32 = 7;
const T_CHANNEL: i32 = 8;
const T_ITEM: i32 = 9;
const T_SOFTIMAGE: i32 = 21;
const T_FRAMERATE: i32 = 22;
const T_TIMESTAMP: i32 = 23;
const T_REFRESH: i32 = 24;
const T_FAST_INPUT: i32 = 25;
const T_CALIBRATE: i32 = 26;
const T_CHANNEL_DRIVER: i32 = 27;
const T_DOF_NUM: i32 = 28;
const T_DOF_NAME: i32 = 29;
const T_DOF_TYPE: i32 = 30;
const T_ERROR_NUM: i32 = 33;
const T_ERROR: i32 = 34;
const T_END: i32 = 43;
const T_RUNMODE: i32 = 35;
const T_SYNCED: i32 = 36;
const T_FREE: i32 = 37;
const T_FREQUENCY: i32 = 38;
const T_REMOTE: i32 = 39;
const T_CAPTURE: i32 = 40;
const T_CONTROL: i32 = 41;
const T_TAKE_REC: i32 = 42;

/// The version this build writes after `CHANNELS` (a single, 3.51, printed with `%6.2f`), and
/// the version from which `REMOTE`, `RUNMODE` and `TAKE_REC` are read.
pub const CHANNELS_VERSION: f32 = 3.51;
/// The version the writer puts in the ASCII header: a constant of its own (0x0867217c), equal
/// to `si_file::VERSION_OUTPUT`.
pub const HEADER_VERSION: f32 = 4.0;
/// From this version a `DRIVER` statement carries the remote number, host and location.
const VERSION_REMOTE: f32 = 3.5;
/// From this version a connection is an expression in quotes.
const VERSION_EXPRESSION: f32 = 3.0;
/// Above this version (a double) a `DRIVER` statement is followed by a data section.
const VERSION_DATA_SECTION: f64 = 2.63;
/// Above this version (a double) a `CHANNEL` statement carries type and name. The channels
/// version 2.65 is a single, 2.6500001, and so is *not* above it.
const VERSION_CHANNEL_NAME: f64 = 2.6501;
/// Above this version (a double) a `DRIVER` statement carries its state and channel count.
const VERSION_DRIVER_COUNT: f64 = 2.0;
/// What stands for an empty host or location in a `DRIVER` statement.
const EMPTY_STRING: &[u8] = b"DRIVER_EMPTY_STRING";
/// What stands for the program's own driver directory as the location.
const DEFAULT_LOCATION: &[u8] = b"DEFAULT_DRIVER_LOCATION";
/// The most channels all drivers together may have: a driver that would pass it is dropped.
pub const MAX_CHANNELS: i32 = 1000;
/// The frequency a new driver has, and the one written as `DEFAULT`.
pub const DEFAULT_FREQUENCY: f32 = 10.0;
/// The driver directory the original falls back to when the environment variable
/// `SI_CHNLDRIVER` names none (`si_chnDrvDirectory`, 0x0855d0d0; if this one cannot be read
/// either it uses the current directory). Which one applies is a fact of the machine.
pub const DEFAULT_DRIVER_DIRECTORY: &[u8] = b"/usr/softimage/chnlDriver/bin";

/// Driver names of versions up to 2.0 and what replaces them (`oldDrvs` 0x086ec860, `newDrvs`
/// 0x086ec780, `oldDofNums` 0x086ec7c0): old name, new name, number of channels.
const OLD_DRIVERS: [(&str, &str, i32); 9] = [
    ("mouse_driver", "mouse", 2),
    ("dials_driver", "dialbox", 8),
    ("sonic_driver", "sonic", 24),
    ("space_driver", "spaceball", 12),
    ("polhemus_driver", "polhemus", 6),
    ("polhemus3_driver", "polhemus", 3),
    ("polhemus_6", "polhemus", 6),
    ("polhemus_24", "polhemus", 24),
    ("polhemus_48", "polhemus", 48),
];
/// The current driver names with their numbers of channels (`newDrvs`, `newDofNums`
/// 0x086ec6e0). The search stops at the first match, so `polhemus` is 24.
const NEW_DRIVERS: [(&str, i32); 12] = [
    ("mouse", 2),
    ("dialbox", 8),
    ("sonic", 24),
    ("spaceball", 12),
    ("polhemus", 24),
    ("polhemus", 24),
    ("polhemus", 24),
    ("polhemus", 24),
    ("polhemus", 24),
    ("ascension", 24),
    ("micro_vol", 1),
    ("micro_freqs", 8),
];

/// One channel of a driver: a "degree of freedom", the original's 32-byte record.
#[derive(Clone, Debug, PartialEq)]
pub struct Dof {
    /// +0x00. Null until the step after the last `DRIVER` names it `DOF<n>` or a `CHANNEL`
    /// statement names it.
    pub name: Option<Vec<u8>>,
    /// +0x04: 1 input (`I`), 2 output (`O`), 0 neither (`-`).
    pub kind: i32,
    /// +0x08: `ON` or `OFF`.
    pub active: i16,
    /// +0x0c: first number of a `CHANNEL` statement, 0 by default. Its meaning is not read from
    /// code; with `high` it looks like the range the raw value is mapped to.
    pub low: f32,
    /// +0x10: second number, 1 by default.
    pub high: f32,
    /// +0x1c: third number, −9999 by default.
    pub min: f32,
    /// +0x18: fourth number, 9999 by default; never below `min` after a read.
    pub max: f32,
}

/// A channel driver as the setup file describes it: the fields of the original's 1352-byte
/// record that the reader and the writer touch.
#[derive(Clone, Debug, PartialEq)]
pub struct Driver {
    /// +0x00
    pub name: Option<Vec<u8>>,
    /// +0x04: the driver directory at the time of the read.
    pub directory: Option<Vec<u8>>,
    /// +0x0c: number of channels, as read (it may be negative; `dofs` is then empty).
    pub ndof: i32,
    /// +0x10
    pub dofs: Vec<Dof>,
    /// +0x14 count, +0x18 pointers: the driver's error messages by number. Not in the setup
    /// file: they come from the driver's description ([`read_driver_description`]).
    pub errors: Vec<Option<Vec<u8>>>,
    /// +0x1c: `ON` or `OFF`.
    pub active: i16,
    /// +0x24: index of the driver's first channel among all channels.
    pub first_channel: i32,
    /// +0x2c: −2 from a `DRIVER` statement until the step after the last one, then −1.
    pub info_state: i32,
    /// +0x34: the number after `REMOTE`.
    pub remote: i32,
    /// +0x38 (512 bytes): host name, empty for `DRIVER_EMPTY_STRING`.
    pub host: Vec<u8>,
    /// +0x238: where the driver is; the driver directory for `DEFAULT_DRIVER_LOCATION`.
    pub location: Vec<u8>,
    /// +0x53c: 0 `SYNCED`, 1 `FREE`, 2 `FREQUENCY`.
    pub run_mode: i32,
    /// +0x540
    pub frequency: f32,
    /// The data section that followed the statement, as the original gathers it into one
    /// string ([`read_data_section`]). `None`: nothing was read (version up to 2.63) or the
    /// section was not found. The original hands the string to the driver's custom dialog
    /// (+0x30) if the driver is installed and otherwise drops it.
    pub data_section: Option<Vec<u8>>,
}

/// What a connection holds at +0x0c after the read.
#[derive(Clone, Debug, PartialEq)]
pub enum Connection {
    /// Version 3.0 and later: the expression between the quotes.
    Expression(Vec<u8>),
    /// Before 3.0: the fields of the statement. The original turns them into an expression
    /// right away, `<type><element>.<curve> = <scale> * CHN <driver>.<channel> + <offset>` for
    /// an input channel and the reverse for an output one (numbers with `%.12g`), taking the
    /// type prefix and the curve name from the attribute dictionary. That text is not built
    /// here.
    Legacy {
        /// `MODEL`, `CAMERA`, …: read and not used by the original.
        element_type: Vec<u8>,
        element: Vec<u8>,
        /// Third word (`no` in the samples): part of the curve name for two curve types.
        qualifier: Vec<u8>,
        /// Function curve type; the element type is derived from it.
        curve_type: i32,
        curve_index: i32,
        channel: i32,
        offset: f32,
        scale: f32,
        /// Driver and channel within it, found from `channel`.
        driver: i32,
        dof: i32,
        /// The channel was an input (`kind` 1) when the statement was read.
        input: bool,
    },
}

/// A connection between a channel and an element: the fields of the original's 152-byte record
/// that the reader and the writer touch.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// +0x02: set to 1 by the reader; recomputed by the original before each write.
    pub valid: i16,
    /// +0x04: `ON` in the `ITEM` statement, then the first word of its `CALIBRATE` line.
    pub enabled: i16,
    /// +0x0c
    pub connection: Connection,
    /// +0x74: second word of the `CALIBRATE` line.
    pub calibrated: i16,
    /// +0x78, +0x7c, +0x8c: the three numbers of the `CALIBRATE` line. Meaning not read from
    /// code.
    pub calibration: [f32; 3],
}

/// The channel setup: the fields of the original's 976-byte record that the file holds.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelSetup {
    /// +0x08: 2 for `DRIVER_SYNC ON`, 1 for `OFF`. Read, never written.
    pub driver_sync: i32,
    /// +0x0c: `DRIVER_DEBUG`. Read, never written.
    pub driver_debug: i16,
    /// +0x1c: `REFRESH`, or the opposite of `FAST_INPUT`.
    pub refresh: i16,
    /// +0x1e: `FRAMERATE`.
    pub framerate: i16,
    /// +0x20: `TIMESTAMP`. Read, never written.
    pub timestamp: i16,
    /// +0x24: `CAPTURE`.
    pub capture: i16,
    /// +0x26: `CONTROL`.
    pub control: i16,
    /// +0x34 (256 bytes): first word after `TAKE_REC`, the directory takes are recorded in.
    pub take_directory: Vec<u8>,
    /// +0x134 (256 bytes): second word, the name of the take file.
    pub take_name: Vec<u8>,
    /// +0x234: the first free slot in the take file, when the file exists.
    pub take_slot: i32,
    /// +0x238: the slot before it, or −1.
    pub take_last: i32,
    /// +0x37a: set when a connection was added.
    pub items_changed: i16,
    /// +0x00 count, +0x10 records.
    pub drivers: Vec<Driver>,
    /// +0x18: a linked list in the original.
    pub items: Vec<Item>,
}

/// A line the original shows on the status line, or prints.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    /// The second argument of `SI_statusDisplayMessage`; −1 for a line printed to `stderr`.
    pub severity: i32,
    pub text: Vec<u8>,
}

/// What the reader needs from the running program and the machine.
#[derive(Clone, Copy)]
pub struct Context<'c> {
    /// The file's name, for the messages.
    pub file_name: &'c [u8],
    /// What `si_chnDrvDirectory` returns.
    pub driver_directory: &'c [u8],
    /// The reader's third argument: ask each driver for its description after the last
    /// `DRIVER` statement. Loading a scene passes 1; the menu's "get setup" passes 0 when it
    /// reads into a fresh setup to compare with the current one, else 1. With no driver
    /// executables every driver asked is switched off.
    pub driver_lookup: bool,
    /// `SI_chnFirstFreeTakeSlot(path, 1)` (0x08574968) for the take file `TAKE_REC` names:
    /// −1 when the file does not exist.
    pub first_free_take_slot: fn(&[u8]) -> i32,
}

fn no_take_file(_path: &[u8]) -> i32 {
    -1
}

impl Default for Context<'_> {
    fn default() -> Self {
        Context { file_name: b"", driver_directory: DEFAULT_DRIVER_DIRECTORY, driver_lookup: false, first_free_take_slot: no_take_file }
    }
}

/// Why a read gave nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum ChnError {
    /// The first keyword is not `CHANNELS`: "File '…' has an invalid id for a channel setup
    /// file." The original returns 0 with the setup reset.
    InvalidId,
    /// The original never returns: it is in a loop that reads characters until a newline (or,
    /// for the version, until a digit) and the file has ended. `at` is the file offset where
    /// the loop started.
    EndlessLoop { at: usize, what: &'static str },
    /// A driver with more than [`MAX_CHANNELS`] channels that the original would keep (the
    /// test is on the total, which earlier negative counts can lower), or a count in a driver
    /// configuration file that is larger than the file itself. The original allocates 52
    /// bytes per channel for it; here the read stops.
    HugeDriver { ndof: i32 },
    /// A number of error messages in a driver configuration file that is larger than the file
    /// itself. The original allocates a pointer for each; here the read stops.
    HugeErrorCount { count: i32 },
}

/// What a read reports besides the setup.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadInfo {
    /// The ASCII header's version; −1.0 when the file has no accepted header (the reader then
    /// starts over from the first byte).
    pub version: f32,
    /// The header's tag.
    pub header_tag: Option<u32>,
    /// The number after `CHANNELS`: the version the reader's branches test (the original keeps
    /// it in the global `si_version`, 0x08829820).
    pub channels_version: f32,
    /// No number followed `CHANNELS` before the first digit-like character: the original then
    /// uses an uninitialised single as the version. Here the version is 0.
    pub version_missing: bool,
    /// The reader's result: false if a `DRIVER`, `CHANNEL` or `ITEM` statement was bad. The
    /// rest of the file is read all the same.
    pub ok: bool,
    pub messages: Vec<Message>,
    /// Nothing but white space is left after the last byte the reader took.
    pub all_tokens_consumed: bool,
    /// A statement wanted more input than the file had (the main loop finding no further
    /// keyword is how the reader normally ends, and does not count).
    pub read_past_end: bool,
    /// Places where the original would have written past a fixed buffer (a word longer than
    /// the buffer `fscanf` stores it in). The strings are kept whole here.
    pub overruns: usize,
    /// File offset after the last byte the reader took.
    pub end: usize,
}

struct Reader<'a, 'c> {
    f: CFile<'a>,
    ctx: Context<'c>,
    messages: Vec<Message>,
    overruns: usize,
    /// How much of `f.starved` is the main loop finding no further keyword: the normal end.
    expected: usize,
}

impl Reader<'_, '_> {
    fn message(&mut self, severity: i32, text: Vec<u8>) {
        self.messages.push(Message { severity, text });
    }

    /// `sprintf(buf, "… '%s' …", file name)`
    fn file_message(&mut self, severity: i32, before: &str, after: &str) {
        let mut text = before.as_bytes().to_vec();
        text.extend_from_slice(c_str(self.ctx.file_name));
        text.extend_from_slice(after.as_bytes());
        self.message(severity, text);
    }

    /// Notes a string longer than the buffer the original reads it into (`cap` bytes, one of
    /// them for the terminator).
    fn fits(&mut self, s: &[u8], cap: usize) {
        if s.len() >= cap {
            self.overruns += 1;
        }
    }

    /// `do c = fgetc(f); while (c != '\n');`: the rest of the line. At the end of the file
    /// `fgetc` keeps returning `EOF` and the original's loop never ends.
    fn skip_line(&mut self, what: &'static str) -> Result<(), ChnError> {
        let at = self.f.tell();
        loop {
            let c = self.f.getc();
            if c == b'\n' as i32 {
                return Ok(());
            }
            if c == EOF {
                return Err(ChnError::EndlessLoop { at, what });
            }
        }
    }
}

/// The number of a keyword, 0 for any other word [soft 0x08571a80 tokenType].
pub fn token_type(word: &[u8]) -> i32 {
    KEYWORDS.iter().find(|(name, _)| name.as_bytes() == word).map_or(0, |&(_, t)| t)
}

impl Reader<'_, '_> {
    /// The next keyword [soft 0x08571af0 tokenReadNext]. Words that are not keywords are
    /// skipped without a message, and so is the rest of a line after a `#` that stands alone
    /// (`#abc` is just an unknown word, and the words after it are read). The result is 0 at
    /// the end of the file; it is also 0, with the word consumed, when an unknown word is the
    /// last thing in a file with no newline after it.
    ///
    /// The original reads each word with `%s` into 80 bytes of stack.
    fn token_read_next(&mut self) -> Result<i32, ChnError> {
        let mut tok = 0;
        if self.f.feof() {
            self.f.starved += 1;
        }
        while !self.f.feof() && tok == 0 {
            let mut word = Vec::new();
            if self.f.scanf("%s", &mut [Arg::Str(&mut word)]) != 1 {
                return Ok(0);
            }
            self.fits(&word, 80);
            tok = token_type(&word);
            if tok == T_COMMENT {
                self.skip_line("comment")?;
                tok = 0;
            }
        }
        Ok(tok)
    }

    /// [`Reader::token_read_next`] for the loop that reads one statement after the other: the
    /// end of the file here is how a read normally ends.
    fn next_statement(&mut self) -> Result<i32, ChnError> {
        let mark = self.f.starved;
        let tok = self.token_read_next()?;
        self.expected += self.f.starved - mark;
        Ok(tok)
    }

    /// 1 if the next keyword is `ON`, else 0 [soft 0x08571b70 tokenReadBoolean]. The keyword
    /// is consumed whatever it is: `FRAMERATE` followed directly by `DRIVER` loses the driver.
    fn token_read_boolean(&mut self) -> Result<i16, ChnError> {
        Ok((self.token_read_next()? == T_ON) as i16)
    }

    /// The run mode after `RUNMODE` [soft 0x08571b98 tokenReadFreeRun]: `FREE` 1, `FREQUENCY`
    /// 2, and `ON` (the argument of the older `FREERUN`) 2; `SYNCED` and anything else 0.
    fn token_read_free_run(&mut self) -> Result<i32, ChnError> {
        Ok(match self.token_read_next()? {
            T_FREE => 1,
            T_FREQUENCY | T_ON => 2,
            T_SYNCED => 0,
            _ => 0,
        })
    }

    /// Skips to the first digit, sign or point and scans a single there with `%f`
    /// [soft 0x083fba00 SI_floatRead]. `None`: nothing scanned and the caller's variable is
    /// left as it was. The original's result is always 1, so its callers' "bad version number"
    /// branches cannot be taken; at the end of the file the skipping loop never ends.
    fn float_read(&mut self) -> Result<Option<f32>, ChnError> {
        let at = self.f.tell();
        loop {
            let c = self.f.getc();
            if c == EOF {
                return Err(ChnError::EndlessLoop { at, what: "version number" });
            }
            // the original tests the byte as a signed char; 0xff is then the same as EOF and
            // is skipped like any other byte
            if (c as u8).is_ascii_digit() || c == b'+' as i32 || c == b'-' as i32 || c == b'.' as i32 {
                self.f.ungetc(c);
                break;
            }
        }
        let mut v = 0.0f32;
        Ok((self.f.scanf("%f", &mut [Arg::Float(&mut v)]) == 1).then_some(v))
    }
}

impl Dof {
    /// A channel as a new driver gets it `si_dofAlloc` (0x08561428; not counted as ported: only the fields
    /// kept here; the original also allocates a second, 20-byte record per channel).
    fn new() -> Dof {
        Dof { name: None, kind: 1, active: 1, low: 0.0, high: 1.0, min: -9999.0, max: 9999.0 }
    }
}

impl Driver {
    /// An empty driver `si_chnDrvNew` (0x0855bfe0; not counted as ported: only the fields kept here): all
    /// zero, except that its location is the driver directory and its frequency 10.
    pub fn new(driver_directory: &[u8]) -> Driver {
        Driver {
            name: None,
            directory: None,
            ndof: 0,
            dofs: Vec::new(),
            errors: Vec::new(),
            active: 0,
            first_channel: 0,
            info_state: -1,
            remote: 0,
            host: Vec::new(),
            location: c_str(driver_directory).to_vec(),
            run_mode: 0,
            frequency: DEFAULT_FREQUENCY,
            data_section: None,
        }
    }

    /// Gives the driver `ndof` channels with their defaults (`si_dofAlloc`, 0x08561428; see
    /// [`Dof::new`] for what is left out).
    /// A count of zero or less leaves it without channels, as the original's failed `calloc`
    /// and empty loop do. The caller has bounded the count.
    fn dof_alloc(&mut self, ndof: i32) {
        self.ndof = ndof;
        self.dofs = (0..ndof.max(0)).map(|_| Dof::new()).collect();
    }

    /// Names every channel that has no name `DOF<index>` [soft 0x085742dc chnDofNameDefaults].
    fn dof_name_defaults(&mut self) {
        if self.ndof <= 0 {
            return;
        }
        if self.dofs.is_empty() {
            // a driver of version 2.0 or older that got its count from the table of old names
            self.dof_alloc(self.ndof);
        }
        for (i, dof) in self.dofs.iter_mut().enumerate() {
            if dof.name.is_none() {
                dof.name = Some(format!("DOF{i}").into_bytes());
            }
        }
    }
}

impl Default for ChannelSetup {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelSetup {
    /// A new setup `si_chnAllocate` (0x08558f0c; not counted as ported: only the fields kept here). This is
    /// what the reader starts from when a scene is loaded and when the user gets a setup.
    pub fn new() -> ChannelSetup {
        ChannelSetup {
            driver_sync: 2,
            driver_debug: 0,
            refresh: 1,
            framerate: 0,
            timestamp: 1,
            capture: 1,
            control: 1,
            take_directory: b"/usr/tmp".to_vec(),
            take_name: b"default".to_vec(),
            take_slot: 1,
            take_last: -1,
            items_changed: 0,
            drivers: Vec::new(),
            items: Vec::new(),
        }
    }

    /// What the reader clears before it reads `si_chnGlobalReset` (0x085598a0; not counted as ported: only the fields kept here). `CAPTURE`, `CONTROL` and the take file are **not** among them: a
    /// file without those statements leaves what the setup held before.
    fn global_reset(&mut self) {
        self.drivers.clear();
        self.items.clear();
        self.driver_sync = 2;
        self.driver_debug = 0;
        self.timestamp = 1;
        self.framerate = 0;
        self.refresh = 1;
    }

    /// Number of channels of all drivers: the last driver's first channel plus its count, in
    /// the original's 32-bit arithmetic.
    pub fn channel_count(&self) -> i32 {
        self.drivers.last().map_or(0, |d| d.ndof.wrapping_add(d.first_channel))
    }

    /// Numbers the drivers' first channels [soft 0x0855e280 chnDrvMap].
    fn drv_map(&mut self) {
        let mut first: i32 = 0;
        for d in &mut self.drivers {
            d.first_channel = first;
            first = first.wrapping_add(d.ndof);
        }
    }

    /// Adds a driver at the end unless the total would pass 1000 channels
    /// [soft 0x0855cd04 si_chnDrvAppend]. The original copies the record
    /// (`si_chnDrvCopy`, 0x0855c4b0); the callers here ignore the result, as the original's do.
    fn drv_append(&mut self, drv: Driver) -> bool {
        if self.channel_count().wrapping_add(drv.ndof) > MAX_CHANNELS {
            return false;
        }
        self.drivers.push(drv);
        self.drv_map();
        true
    }

    /// The driver and the channel within it for a channel number
    /// [soft 0x0855cda4 si_chnGetDrvDof]: the first driver whose range the number is not
    /// beyond. `None` where the original returns a null pointer (−1, or a number not below the
    /// total), and also where it would return a pointer outside the driver's channels (a
    /// number below −1, which its callers exclude).
    pub fn get_drv_dof(&self, channel: i32) -> Option<(usize, usize)> {
        if channel == -1 {
            return None;
        }
        if self.drivers.is_empty() {
            return None; // for a number below −1 the original computes a pointer from garbage
        }
        if channel >= self.channel_count() {
            return None;
        }
        for (i, d) in self.drivers.iter().enumerate() {
            let rel = channel.wrapping_sub(d.first_channel);
            if rel < d.ndof {
                return (rel >= 0 && (rel as usize) < d.dofs.len()).then_some((i, rel as usize));
            }
        }
        None
    }
}

/// One line [soft 0x08526504 cusSafeReadLine]: the characters up to a newline, a carriage
/// return or the end of the file, without the terminator. `None` once the end of the file has
/// been met. A CR LF pair ends two lines, the second one empty.
fn safe_read_line(f: &mut CFile) -> Option<Vec<u8>> {
    if f.feof() {
        f.starved += 1;
        return None;
    }
    let mut line = Vec::new();
    loop {
        let c = f.getc();
        if c == EOF || c == b'\n' as i32 || c == b'\r' as i32 {
            return Some(line);
        }
        line.push(c as u8);
    }
}

/// C's `isspace` for a byte of a line (never a newline or a carriage return).
fn is_blank(b: u8) -> bool {
    is_space(b as i32)
}

/// A line as C string functions see it, and the same without its leading white space.
/// `None`: empty or all white space.
fn line_start(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let line = c_str(line);
    let n = line.iter().position(|&b| !is_blank(b))?;
    Some((line, &line[n..]))
}

/// The data section of a custom dialog, gathered into one string
/// [soft 0x085240f0 SI_cusReadDataSection]. It works on whole lines:
///
/// 1. lines are skipped until one starts (after white space) with `_ANIMATION` or `_DATA`;
/// 2. after `_ANIMATION`, lines are gathered up to one starting with `_END`, then lines are
///    skipped until one starts with `_DATA`;
/// 3. after `_DATA`, lines are gathered up to one starting with `_END`.
///
/// "Starts with" is a prefix test (`_ENDING` ends a section). Empty lines are dropped. The
/// string is the first line without its leading white space and a newline, then a newline and
/// the line for every further line: `_DATA`, an empty line, the values, `_END`, with no
/// newline at the end.
///
/// `None` with a message when the file ends first. Step 1 has no other way out, so a `DRIVER`
/// statement that should have a data section and has none swallows the file up to the next
/// driver's.
pub fn read_data_section(f: &mut CFile, messages: &mut Vec<Message>) -> Option<Vec<u8>> {
    let mut stderr = |text: &str| messages.push(Message { severity: -1, text: text.as_bytes().to_vec() });
    let start;
    loop {
        let Some(line) = safe_read_line(f) else {
            stderr(si_rt::text(0x086d_c5a0));
            return None;
        };
        if let Some((_, p)) = line_start(&line) {
            if p.starts_with(b"_ANIMATION") || p.starts_with(b"_DATA") {
                start = p.to_vec();
                break;
            }
        }
    }
    let mut out: Vec<u8>;
    let data_line;
    if start.starts_with(b"_ANIMATION") {
        out = start;
        out.push(b'\n');
        loop {
            let Some(line) = safe_read_line(f) else {
                stderr(si_rt::text(0x086d_c620));
                return None;
            };
            if let Some((whole, p)) = line_start(&line) {
                out.push(b'\n');
                out.extend_from_slice(whole);
                if p.starts_with(b"_END") {
                    break;
                }
            }
        }
        loop {
            let Some(line) = safe_read_line(f) else {
                stderr(si_rt::text(0x086d_c6a0));
                return None;
            };
            if let Some((_, p)) = line_start(&line) {
                if p.starts_with(b"_DATA") {
                    data_line = p.to_vec();
                    break;
                }
            }
        }
        out.push(b'\n');
        out.extend_from_slice(&data_line);
    } else {
        out = start;
        out.push(b'\n');
    }
    loop {
        let Some(line) = safe_read_line(f) else {
            stderr(si_rt::text(0x086d_c620));
            return None;
        };
        if let Some((whole, p)) = line_start(&line) {
            out.push(b'\n');
            out.extend_from_slice(whole);
            if p.starts_with(b"_END") {
                return Some(out);
            }
        }
    }
}

/// The lines of a gathered data section as a file holds them: each line followed by a newline.
///
/// **Not from the original.** The original writes a driver's data from the driver's custom
/// dialog, one `name value` line per parameter of the dialog; this stands in for a dialog that
/// has exactly the parameters the section named, with values that print as they were written.
pub fn data_section_lines(section: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in section.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        out.extend_from_slice(line);
        out.push(b'\n');
    }
    out
}

impl Reader<'_, '_> {
    /// `si_chnDrvGetInfo` (0x085731d0) for a machine with no driver executables: the function
    /// up to its first test, whether `path` exists. It replaces the record's name by the last
    /// part of the path and its directory by the rest, reports the missing file and fails.
    /// The rest of the function (starting the executable, or reading an old driver
    /// configuration file) is not ported.
    fn drv_get_info_absent(&mut self, path: &[u8], drv: &mut Driver) -> bool {
        match path.iter().rposition(|&b| b == b'/') {
            Some(n) => {
                drv.name = Some(path[n + 1..].to_vec());
                drv.directory = Some(path[..n].to_vec());
            }
            None => {
                drv.name = Some(path.to_vec());
                drv.directory = Some(b".".to_vec());
            }
        }
        let mut text = b"Channel driver:\n".to_vec();
        text.extend_from_slice(path);
        text.extend_from_slice(b"\ndoes not exist");
        self.message(3, text);
        false
    }

    /// `sprintf(path, "%s/%s", si_chnDrvDirectory(), name)`
    fn driver_path(&self, name: &[u8]) -> Vec<u8> {
        let mut path = c_str(self.ctx.driver_directory).to_vec();
        path.push(b'/');
        path.extend_from_slice(name);
        path
    }

    /// A `DRIVER` statement [soft 0x08573a14 chnDrvRead]. What follows the keyword depends on
    /// the version after `CHANNELS`:
    ///
    /// * up to 1.0: the name;
    /// * up to 2.0: `ON`/`OFF` and the name;
    /// * above 2.0: `ON`/`OFF`, the name, the number of channels;
    /// * from 3.5: then a number, the host and the location (three words), which from 3.51
    ///   must be introduced by the keyword `REMOTE`;
    /// * from 3.51: then `RUNMODE` (or `FREERUN`), a mode keyword and a frequency (`DEFAULT`
    ///   for 10);
    /// * above 2.63: then the data section, read line by line.
    ///
    /// The result is false for a bad statement ("Bad format for DRIVER token in channel setup
    /// file."), in which case no driver is added, and also for every driver of a version up to
    /// 2.0, which is added all the same.
    ///
    /// Not reproduced: the lookup of the driver among the installed ones, into whose dialog
    /// the data section goes, and the driver description for versions up to 2.0 (see
    /// [`Reader::drv_get_info_absent`]).
    fn drv_read(&mut self, g: &mut ChannelSetup, ver: f32) -> Result<bool, ChnError> {
        let mut ok = true;
        let mut run_mode = 0;
        let mut frequency = 0.0f32;
        let (mut state, mut name) = (Vec::new(), Vec::new());
        let mut ndof = 0i32;
        let mut remote = 0i32;
        let (mut host, mut location) = (Vec::new(), Vec::new());
        let active: i16;
        if (ver as f64) > VERSION_DRIVER_COUNT {
            if self.f.scanf("%s %s %d", &mut [Arg::Str(&mut state), Arg::Str(&mut name), Arg::Int(&mut ndof)]) != 3 {
                return Ok(self.bad_driver());
            }
            self.fits(&state, 0x100);
            self.fits(&name, 0x100);
            active = (state == b"ON") as i16;
            if ver >= VERSION_REMOTE {
                if ver >= CHANNELS_VERSION {
                    ok = self.token_read_next()? == T_REMOTE;
                    if !ok {
                        self.file_message(2, "File '", "' has an invalid remote entry in file");
                    }
                }
                ok = ok
                    && self.f.scanf("%d", &mut [Arg::Int(&mut remote)]) == 1
                    && self.f.scanf("%s", &mut [Arg::Str(&mut host)]) == 1
                    && self.f.scanf("%s", &mut [Arg::Str(&mut location)]) == 1;
                if !ok {
                    return Ok(self.bad_driver());
                }
                self.fits(&host, 0x200);
                self.fits(&location, 0x200);
                if ver >= CHANNELS_VERSION {
                    ok = self.token_read_next()? == T_RUNMODE;
                    if !ok {
                        self.file_message(2, "File '", "' has an invalid freerun entry in file");
                    } else {
                        run_mode = self.token_read_free_run()?;
                        let mut word = Vec::new();
                        ok = self.f.scanf("%s", &mut [Arg::Str(&mut word)]) == 1;
                        self.fits(&word, 76);
                        // if the file ended here the original compares an unset buffer; the
                        // statement is refused just below either way
                        frequency = if word == b"DEFAULT" { DEFAULT_FREQUENCY } else { cnum::atof(&word) as f32 };
                    }
                }
            }
        } else if 1.0 < ver {
            if self.f.scanf("%s %s", &mut [Arg::Str(&mut state), Arg::Str(&mut name)]) != 2 {
                return Ok(self.bad_driver());
            }
            active = (state == b"ON") as i16;
        } else {
            if self.f.scanf("%s", &mut [Arg::Str(&mut name)]) != 1 {
                return Ok(self.bad_driver());
            }
            active = 1;
        }
        if !ok {
            return Ok(self.bad_driver());
        }

        let directory = c_str(self.ctx.driver_directory).to_vec();
        let mut drv = Driver::new(&directory);
        drv.name = Some(name.clone());
        drv.directory = Some(directory.clone());
        drv.active = active;
        if ver >= VERSION_REMOTE {
            drv.remote = remote;
            if host != EMPTY_STRING {
                drv.host = host;
            }
            if location == DEFAULT_LOCATION {
                drv.location = directory;
            } else if location != EMPTY_STRING {
                drv.location = location;
            }
            // an empty location leaves what a new driver has, which is the driver directory
            // again: the two spellings read the same
            if ver >= CHANNELS_VERSION {
                drv.run_mode = run_mode;
                drv.frequency = frequency;
            }
        }
        if (ver as f64) > VERSION_DATA_SECTION {
            // `SI_cusDialogReadData` (0x085240ac) reads the section whether or not the driver
            // is installed; its second half, storing the values into the dialog, is left out
            drv.data_section = read_data_section(&mut self.f, &mut self.messages);
        }
        if (ver as f64) > VERSION_DRIVER_COUNT {
            drv.info_state = -2;
            if ndof > MAX_CHANNELS {
                // The original allocates the channels before it decides whether to keep the
                // driver. A driver this large is normally dropped, and is dropped here without
                // the allocation.
                if g.channel_count().wrapping_add(ndof) <= MAX_CHANNELS {
                    return Err(ChnError::HugeDriver { ndof });
                }
                drv.ndof = ndof;
            } else {
                drv.dof_alloc(ndof);
            }
        } else {
            let path = self.driver_path(&name);
            if !self.drv_get_info_absent(&path, &mut drv) {
                if let Some(&(old, new, count)) = OLD_DRIVERS.iter().find(|(old, _, _)| old.as_bytes() == name) {
                    drv.ndof = count;
                    drv.name = Some(new.as_bytes().to_vec());
                    drv.info_state = -2;
                    self.message(2, format!("Channel driver '{old}' replaced by '{new}'.").into_bytes());
                } else if let Some(&(_, count)) = NEW_DRIVERS.iter().find(|(new, _)| new.as_bytes() == name) {
                    drv.ndof = count;
                } else {
                    drv.ndof = 0;
                }
                ok = false;
            }
        }
        g.drv_append(drv);
        Ok(ok)
    }

    fn bad_driver(&mut self) -> bool {
        self.message(2, si_rt::message(0x086e_d640).to_vec());
        false
    }

    /// A `CHANNEL` statement [soft 0x08571c58 globalChnRead]: the channel number, four
    /// numbers and `ON`/`OFF` (three characters at most), scanned in one call; above version
    /// 2.6501 then one character for the type (`I`, `O`, anything else for neither), a word
    /// that is ignored (`NO` in every file), and the name: the rest of the line after its
    /// leading spaces, which may contain spaces. The newlines after the name are consumed.
    ///
    /// A number outside the drivers' channels is skipped without a message and with the rest
    /// of the line left unread. False only when the scan fails.
    fn global_chn_read(&mut self, g: &mut ChannelSetup, ver: f32) -> bool {
        let mut index = 0i32;
        let (mut low, mut high, mut min, mut max) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut state = Vec::new();
        let n = self.f.scanf(
            "%d %f %f %f %f %3s",
            &mut [Arg::Int(&mut index), Arg::Float(&mut low), Arg::Float(&mut high), Arg::Float(&mut min), Arg::Float(&mut max), Arg::Str(&mut state)],
        );
        if n != 6 {
            return false;
        }
        if index < 0 || index >= g.channel_count() {
            return true;
        }
        let Some((d, k)) = g.get_drv_dof(index) else {
            // only with channel counts below zero: the original writes through a pointer
            // outside the driver's channels
            self.overruns += 1;
            return true;
        };
        if min > max {
            max = min;
        }
        {
            let dof = &mut g.drivers[d].dofs[k];
            dof.active = (state == b"ON") as i16;
            dof.low = low;
            dof.high = high;
            dof.min = min;
            dof.max = max;
        }
        let named = (ver as f64) > VERSION_CHANNEL_NAME;
        if !named {
            return true;
        }
        let (mut kind, mut word) = (Vec::new(), Vec::new());
        if self.f.scanf("%1s %3s", &mut [Arg::Str(&mut kind), Arg::Str(&mut word)]) != 2 {
            return false;
        }
        g.drivers[d].dofs[k].kind = match kind.first() {
            Some(b'I') => 1,
            Some(b'O') => 2,
            _ => 0,
        };
        let mut name = Vec::new();
        if self.f.scanf("%*[ ]%[^\n]%*[\n]", &mut [Arg::Str(&mut name)]) == 1 {
            self.fits(&name, 0x200);
            g.drivers[d].dofs[k].name = Some(name);
        }
        true
    }

    /// An `ITEM` statement [soft 0x08571e30 itemChnRead].
    ///
    /// From version 3.0: `ON` (anything else is off) and the expression in double quotes. The
    /// original measures the rest of the line to size the buffer, goes back and scans
    /// `%s "%[^"]"`: the expression runs to the next quote wherever that is, so a missing
    /// closing quote takes the following lines too (and overruns the buffer).
    ///
    /// Before 3.0: three words, three integers, two numbers and `ON`/`OFF`
    /// ([`Connection::Legacy`]). The channel must be one of the drivers'; a curve type of 0 is
    /// read and dropped.
    ///
    /// False ("Bad connection in channel setup file '…'.") when the scan fails or the channel
    /// is out of range; reading goes on wherever the scan stopped.
    ///
    /// Not reproduced: the expression text of the older form (see [`Connection::Legacy`]).
    fn item_chn_read(&mut self, g: &mut ChannelSetup, ver: f32) -> bool {
        let (mut channel, mut curve_type, mut curve_index) = (0i32, 0i32, 0i32);
        let (mut element_type, mut element, mut qualifier) = (Vec::new(), Vec::new(), Vec::new());
        let (mut offset, mut scale) = (0.0f32, 0.0f32);
        let mut state = Vec::new();
        let mut expression: Option<Vec<u8>> = None;
        let mut ok;
        let quoted = ver >= VERSION_EXPRESSION;
        if quoted {
            let pos = self.f.tell();
            loop {
                let c = self.f.getc();
                if c == b'\n' as i32 || self.f.feof() {
                    break;
                }
            }
            let cap = self.f.tell() - pos + 5;
            self.f.seek(pos);
            let mut text = Vec::new();
            ok = self.f.scanf("%s \"%[^\"]\"", &mut [Arg::Str(&mut state), Arg::Str(&mut text)]) == 2;
            self.fits(&text, cap);
            self.fits(&state, 8);
            expression = Some(text);
        } else {
            ok = self.f.scanf("%s", &mut [Arg::Str(&mut element_type)]) == 1
                && self.f.scanf("%s", &mut [Arg::Str(&mut element)]) == 1
                && self.f.scanf("%s", &mut [Arg::Str(&mut qualifier)]) == 1
                && self.f.scanf("%d", &mut [Arg::Int(&mut curve_type)]) == 1
                && self.f.scanf("%d", &mut [Arg::Int(&mut curve_index)]) == 1
                && self.f.scanf("%d", &mut [Arg::Int(&mut channel)]) == 1
                && self.f.scanf("%f %f %s", &mut [Arg::Float(&mut offset), Arg::Float(&mut scale), Arg::Str(&mut state)]) == 3;
            self.fits(&element_type, 0x100);
            self.fits(&element, 0x100);
            self.fits(&qualifier, 0x100);
            // the state word is scanned into 8 bytes just below the qualifier's buffer: a
            // longer one overwrites the qualifier, which goes into the curve name
            self.fits(&state, 8);
            if ok && (channel < 0 || g.drivers.is_empty() || channel >= g.channel_count()) {
                ok = false;
            }
        }
        if !ok {
            self.file_message(3, "Bad connection in channel setup file '", "'.");
            return false;
        }
        let connection = match expression {
            Some(text) => Connection::Expression(text),
            None => {
                let Some((d, k)) = g.get_drv_dof(channel) else {
                    // only with channel counts below zero: the original reads through a
                    // pointer outside the driver's channels
                    self.overruns += 1;
                    return true;
                };
                Connection::Legacy {
                    element_type,
                    element,
                    qualifier,
                    curve_type,
                    curve_index,
                    channel,
                    offset,
                    scale,
                    driver: d as i32,
                    dof: k as i32,
                    input: g.drivers[d].dofs[k].kind == 1,
                }
            }
        };
        if !quoted && curve_type == 0 {
            return true;
        }
        if channel > -1 && channel <= 999 {
            // `si_chnItemInit` (0x0855a108; not counted as ported: only the fields kept here), then the end
            // of the list [soft 0x08559fd0 si_chnItemAddEnd]
            g.items.push(Item { valid: 1, enabled: (state == b"ON") as i16, connection, calibrated: 0, calibration: [0.0; 3] });
            g.items_changed = 1;
        }
        true
    }

    /// The step after the last `DRIVER` statement, part of [soft 0x08572214 si_chnAsciiRead].
    fn after_drivers(&mut self, g: &mut ChannelSetup) {
        for i in 0..g.drivers.len() {
            if self.ctx.driver_lookup && g.drivers[i].info_state == -2 {
                // The original asks the driver executable for its description and, if the
                // number of channels agrees, takes the channel names and types from it
                // (`si_chnDrvChange`, 0x0855c944). Without the executable the driver is
                // switched off.
                let mut probe = g.drivers[i].clone();
                let path = self.driver_path(g.drivers[i].name.as_deref().unwrap_or(b""));
                if !self.drv_get_info_absent(&path, &mut probe) {
                    g.drivers[i].active = 0;
                }
            }
            g.drivers[i].info_state = -1;
            g.drivers[i].dof_name_defaults();
        }
    }
}

/// What reading a driver configuration file reports.
#[derive(Clone, Debug, PartialEq)]
pub struct DescriptionInfo {
    /// The number after `CHANNEL_DRIVER`. The reader does not use it.
    pub version: f32,
    /// No number followed the keyword (see [`ReadInfo::version_missing`]).
    pub version_missing: bool,
    /// The reader stopped at a statement it could not scan (or at `_END`) and not at the end
    /// of the file.
    pub stopped: bool,
    pub all_tokens_consumed: bool,
    pub read_past_end: bool,
    pub overruns: usize,
    pub end: usize,
}

impl Reader<'_, '_> {
    /// A `DOF_NAME` statement [soft 0x08574124 chnDofNameRead]: channel number and a name of
    /// one word. A number the driver does not have is ignored.
    fn dof_name_read(&mut self, drv: &mut Driver) -> bool {
        let mut index = 0i32;
        let mut name = Vec::new();
        if self.f.scanf("%d %s", &mut [Arg::Int(&mut index), Arg::Str(&mut name)]) != 2 {
            return false;
        }
        self.fits(&name, 0x100);
        if index >= 0 && index < drv.ndof {
            match drv.dofs.get_mut(index as usize) {
                Some(dof) => dof.name = Some(name),
                None => self.overruns += 1,
            }
        }
        true
    }

    /// A `DOF_TYPE` statement [soft 0x085741c0 chnDofTypeRead]: channel number, `I` or `O`,
    /// `ON` or `OFF`. Anything else in the second or third place fails, but only for a channel
    /// the driver has: a number out of range is accepted with whatever follows it.
    fn dof_type_read(&mut self, drv: &mut Driver) -> bool {
        let mut index = 0i32;
        let mut kind = 0u8;
        let mut state = Vec::new();
        if self.f.scanf("%d %c %3s", &mut [Arg::Int(&mut index), Arg::Char(&mut kind), Arg::Str(&mut state)]) != 3 {
            return false;
        }
        if index < 0 || index >= drv.ndof {
            return true;
        }
        if (kind != b'I' && kind != b'O') || (state != b"ON" && state != b"OFF") {
            return false;
        }
        match drv.dofs.get_mut(index as usize) {
            Some(dof) => {
                dof.kind = if kind == b'I' { 1 } else { 2 };
                dof.active = (state == b"ON") as i16;
            }
            None => self.overruns += 1,
        }
        true
    }

    /// An `ERROR` statement [soft 0x0857436c chnErrorRead]: the message's number and its
    /// text, at most 255 bytes. For a driver without a custom dialog the text is what stands
    /// between the next two double quotes, wherever they are; for one with a dialog it is the
    /// rest of the line after the next space. A number the driver does not have is ignored.
    fn error_read(&mut self, drv: &mut Driver, has_dialog: bool) -> bool {
        let mut index = 0i32;
        if self.f.scanf("%d", &mut [Arg::Int(&mut index)]) != 1 {
            return false;
        }
        let (open, close) = if has_dialog { (b' ', b'\n') } else { (b'"', b'"') };
        // both loops compare the low byte of what getc returned and are ended by the
        // end-of-file flag
        loop {
            let c = self.f.getc();
            if c as u8 == open || self.f.feof() {
                break;
            }
        }
        let mut text = Vec::new();
        loop {
            let c = self.f.getc();
            if c as u8 == close || self.f.feof() || text.len() > 0xfe {
                break;
            }
            text.push(c as u8);
        }
        if index >= 0 && (index as usize) < drv.errors.len() {
            drv.errors[index as usize] = Some(c_str(&text).to_vec());
        }
        true
    }

    /// The statements of a driver configuration file [soft 0x08573790 si_chnDrvReadInfo]:
    ///
    /// | Keyword | Arguments |
    /// |---|---|
    /// | `DOF_NUM` | number of channels, above zero; the channels start over with their defaults |
    /// | `DOF_NAME` | channel number, name |
    /// | `DOF_TYPE` | channel number, `I`/`O`, `ON`/`OFF` |
    /// | `ERROR_NUM` | number of error messages, above zero |
    /// | `ERROR` | message number, text in double quotes |
    /// | `_END` | stops the read |
    /// | any other keyword | the rest of the line is skipped |
    ///
    /// The driver's channels and messages are dropped first. The read stops without a message
    /// at the first statement that does not scan. Afterwards every channel without a name is
    /// named `DOF<n>` and every message without a text `error number <n>`. `size` is the
    /// length of the file, against which the two counts are checked here.
    fn drv_read_info(&mut self, drv: &mut Driver, has_dialog: bool, size: usize) -> Result<bool, ChnError> {
        drv.ndof = 0;
        drv.dofs.clear();
        drv.errors.clear();
        let mut go_on = true;
        while !self.f.feof() && go_on {
            match self.next_statement()? {
                0 | T_END => go_on = false,
                T_DOF_NUM => {
                    let mut n = 0i32;
                    if self.f.scanf("%d", &mut [Arg::Int(&mut n)]) != 1 || n <= 0 {
                        go_on = false;
                    } else if n as usize > size {
                        return Err(ChnError::HugeDriver { ndof: n });
                    } else {
                        drv.dof_alloc(n);
                    }
                }
                T_DOF_NAME => go_on = self.dof_name_read(drv),
                T_DOF_TYPE => go_on = self.dof_type_read(drv),
                T_ERROR_NUM => {
                    let mut n = 0i32;
                    if self.f.scanf("%d", &mut [Arg::Int(&mut n)]) != 1 || n <= 0 {
                        go_on = false;
                    } else if n as usize > size {
                        return Err(ChnError::HugeErrorCount { count: n });
                    } else {
                        drv.errors = vec![None; n as usize];
                    }
                }
                T_ERROR => go_on = self.error_read(drv, has_dialog),
                _ => self.skip_line("line of a keyword with no meaning here")?,
            }
        }
        drv.dof_name_defaults();
        for (i, e) in drv.errors.iter_mut().enumerate() {
            if e.is_none() {
                *e = Some(format!("error number {i}").into_bytes());
            }
        }
        Ok(!go_on)
    }
}

/// Reads a driver configuration file (`<driver>.drv` in the driver directory) into a driver
/// record: the part of `si_chnDrvGetInfo` (0x085731d0) that handles a driver which is not a
/// custom effect, from the point where the file is open. The first keyword must be
/// `CHANNEL_DRIVER` ([`ChnError::InvalidId`]: "Channel driver configuration file: … has an
/// invalid ID"), a version number follows, then the statements of
/// [soft 0x08573790 si_chnDrvReadInfo].
///
/// The file has no ASCII header: the tag `SDRV` ("Ascii Driver Descr." in the program's table
/// of file types) is known to the header reader, but nothing in this build reads or writes a
/// file with it. A header before `CHANNEL_DRIVER` would be skipped as an unknown word.
///
/// `has_dialog`: the record already carries a custom dialog, which changes how `ERROR` texts
/// are delimited.
///
/// Not reproduced: the rest of `si_chnDrvGetInfo` (finding the files, starting a driver
/// executable) and what the caller does with the description (`si_chnDrvChange`, 0x0855c944).
pub fn read_driver_description(drv: &mut Driver, data: &[u8], has_dialog: bool) -> Result<DescriptionInfo, ChnError> {
    let mut r = Reader { f: CFile::new(data), ctx: Context::default(), messages: Vec::new(), overruns: 0, expected: 0 };
    if r.next_statement()? != T_CHANNEL_DRIVER {
        return Err(ChnError::InvalidId);
    }
    let scanned = r.float_read()?;
    let stopped = r.drv_read_info(drv, has_dialog, data.len())?;
    Ok(DescriptionInfo {
        version: scanned.unwrap_or(0.0),
        version_missing: scanned.is_none(),
        stopped: stopped && !r.f.f.only_blanks_left(),
        all_tokens_consumed: r.f.f.only_blanks_left(),
        read_past_end: r.f.starved > r.expected,
        overruns: r.overruns,
        end: r.f.tell(),
    })
}

/// Reads a channel setup file into a new setup: [`read_into`] with the default context.
pub fn read(data: &[u8]) -> Result<(ChannelSetup, ReadInfo), ChnError> {
    let mut g = ChannelSetup::new();
    let info = read_into(&mut g, data, &Context::default())?;
    Ok((g, info))
}

/// Reads a channel setup file [soft 0x08572214 si_chnAsciiRead].
///
/// The setup is reset, the ASCII header is read (its result is ignored: without a header the
/// file is read from its first byte), the first keyword must be `CHANNELS` and the number
/// after it is the version every later branch tests. Then keywords are read until the end of
/// the file:
///
/// | Keyword | Arguments |
/// |---|---|
/// | `DRIVER` | see [`Reader::drv_read`] |
/// | `CHANNEL` | see [`Reader::global_chn_read`] |
/// | `ITEM` | see [`Reader::item_chn_read`] |
/// | `CALIBRATE` | one line per connection read so far: two `ON`/`OFF` and three numbers |
/// | `FAST_INPUT`, `REFRESH`, `FRAMERATE`, `TIMESTAMP`, `CAPTURE`, `CONTROL`, `DRIVER_DEBUG`, `DRIVER_SYNC` | `ON` or anything else |
/// | `TAKE_REC` | two words, from version 3.51 on (before that the keyword does nothing and its two words are unknown words to the loop) |
/// | `Softimage` and every other keyword | the rest of the line is skipped |
///
/// The first keyword that is not `DRIVER` after a `DRIVER` triggers the step that names the
/// channels. At the end, if neither `CAPTURE` nor `CONTROL` is on, `CAPTURE` is switched on.
pub fn read_into(g: &mut ChannelSetup, data: &[u8], ctx: &Context) -> Result<ReadInfo, ChnError> {
    let mut r = Reader { f: CFile::new(data), ctx: *ctx, messages: Vec::new(), overruns: 0, expected: 0 };
    let mut ok = true;
    g.global_reset();
    let header = read_asc_header(&mut r.f.f);
    let (header_tag, version) = match header {
        Some((tag, v)) => (Some(tag), v),
        None => (None, -1.0),
    };
    if r.next_statement()? != T_CHANNELS {
        r.file_message(2, "File '", "' has an invalid id for a channel setup file.");
        return Err(ChnError::InvalidId);
    }
    let scanned = r.float_read()?;
    // without a number the original's version is whatever its stack held
    let ver = scanned.unwrap_or(0.0);

    let mut previous = 0;
    let mut go_on = true;
    while !r.f.feof() && go_on {
        let tok = r.next_statement()?;
        if previous == T_DRIVER && tok != T_DRIVER {
            r.after_drivers(g);
        }
        previous = tok;
        match tok {
            0 => go_on = false,
            T_DRIVER => {
                if !r.drv_read(g, ver)? {
                    ok = false;
                }
            }
            T_DRIVER_SYNC => g.driver_sync = if r.token_read_boolean()? != 0 { 2 } else { 1 },
            T_DRIVER_DEBUG => g.driver_debug = r.token_read_boolean()?,
            T_CHANNEL => {
                if !r.global_chn_read(g, ver) {
                    ok = false;
                    r.file_message(2, "Bad format for channel in channel setup file '", "'.");
                }
            }
            T_ITEM => {
                if !r.item_chn_read(g, ver) {
                    ok = false;
                }
            }
            T_SOFTIMAGE => r.skip_line("Softimage line")?,
            T_FRAMERATE => g.framerate = r.token_read_boolean()?,
            T_TIMESTAMP => g.timestamp = r.token_read_boolean()?,
            T_REFRESH => g.refresh = r.token_read_boolean()?,
            T_FAST_INPUT => g.refresh = (r.token_read_boolean()? == 0) as i16,
            T_CALIBRATE => {
                for i in 0..g.items.len() {
                    g.items[i].enabled = r.token_read_boolean()?;
                    g.items[i].calibrated = r.token_read_boolean()?;
                    let [mut a, mut b, mut c] = g.items[i].calibration;
                    r.f.scanf("%g %g %g", &mut [Arg::Float(&mut a), Arg::Float(&mut b), Arg::Float(&mut c)]);
                    g.items[i].calibration = [a, b, c];
                    r.skip_line("CALIBRATE line")?;
                }
            }
            T_CAPTURE => g.capture = r.token_read_boolean()?,
            T_CONTROL => g.control = r.token_read_boolean()?,
            T_TAKE_REC => {
                if ver >= CHANNELS_VERSION {
                    r.f.scanf("%s %s", &mut [Arg::Str(&mut g.take_directory), Arg::Str(&mut g.take_name)]);
                    let (dir_len, name_len) = (g.take_directory.len(), g.take_name.len());
                    if dir_len >= 0x100 || name_len >= 0x100 {
                        r.overruns += 1;
                    }
                    // sprintf(path, "%s/%s.CHD", directory, name): the take file
                    let mut path = g.take_directory.clone();
                    path.push(b'/');
                    path.extend_from_slice(&g.take_name);
                    path.extend_from_slice(b".CHD");
                    let slot = (r.ctx.first_free_take_slot)(&path);
                    if slot >= 0 {
                        g.take_slot = slot;
                    }
                    g.take_last = if slot > 0 { slot - 1 } else { -1 };
                }
            }
            _ => r.skip_line("line of a keyword with no meaning here")?,
        }
    }
    if g.capture == 0 && g.control == 0 {
        g.capture = 1;
    }
    Ok(ReadInfo {
        version,
        header_tag,
        channels_version: ver,
        version_missing: scanned.is_none(),
        ok,
        messages: r.messages,
        all_tokens_consumed: r.f.f.only_blanks_left(),
        read_past_end: r.f.starved > r.expected,
        overruns: r.overruns,
        end: r.f.tell(),
    })
}

/// How the writer prints what the program keeps outside the setup.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DataSections {
    /// Every driver as the original writes one that is not installed (no custom dialog): an
    /// empty section, `_DATA` and `_END`.
    Empty,
    /// The lines that were read, through [`data_section_lines`]: a stand-in for the dialog of
    /// an installed driver. Not from the original.
    AsRead,
}

/// What the writer needs from the running program.
#[derive(Clone, Copy)]
pub struct WriteContext<'c> {
    /// The version in the ASCII header: [`HEADER_VERSION`] in the original.
    pub header_version: f32,
    /// What `si_chnDrvDirectory` returns: a location equal to it is written as
    /// `DEFAULT_DRIVER_LOCATION`.
    pub driver_directory: &'c [u8],
    pub data_sections: DataSections,
}

impl Default for WriteContext<'_> {
    fn default() -> Self {
        WriteContext { header_version: HEADER_VERSION, driver_directory: DEFAULT_DRIVER_DIRECTORY, data_sections: DataSections::Empty }
    }
}

/// Why a setup cannot be written here.
#[derive(Clone, Debug, PartialEq)]
pub enum WriteError {
    /// Connection number `item` is of the form older than 3.0, whose expression text the
    /// original builds with the attribute dictionary.
    LegacyConnection { item: usize },
    /// Driver number `driver` has a channel count without channels (the original would
    /// dereference a null pointer).
    MissingChannels { driver: usize },
}

fn on_off(v: i16) -> &'static str {
    if v != 0 { "ON" } else { "OFF" }
}

/// Writes a channel setup file [soft 0x08572814 si_chnAsciiWrite]. The layout is always the
/// current one (`CHANNELS   3.51`), with `#` comment lines between its five parts: the setup's
/// switches, the drivers, the channels, the connections and their calibration.
///
/// Not written at all: `DRIVER_SYNC`, `DRIVER_DEBUG`, `TIMESTAMP` (and `REFRESH`, which comes
/// out as the opposite `FAST_INPUT`). A connection that is off is written `ITEM FALSE`, which
/// reads back as off. A null name prints as `(null)`, as glibc does.
///
/// Not reproduced: the drivers' data sections come from their dialogs ([`DataSections`]) and
/// the expressions are printed from their parsed form (the text is written as it is held);
/// before the connections the original recomputes each one's `valid` flag
/// (`si_chnItemListValidate`, 0x0855a16c), which is not written.
pub fn write(g: &ChannelSetup, ctx: &WriteContext) -> Result<Vec<u8>, WriteError> {
    let mut out = Vec::new();
    let put = |out: &mut Vec<u8>, s: &str| out.extend_from_slice(s.as_bytes());
    si_file::write_asc_header(&mut out, ctx.header_version, u32::from_be_bytes(*b"SCHN"));
    put(&mut out, &format!("CHANNELS {}\n", fmt_f(CHANNELS_VERSION as f64, 6, 2)));
    put(&mut out, "Softimage Creative Environment\n");
    put(&mut out, "#\n# CHANNEL SETUP INFORMATION\n#\n");
    put(&mut out, &format!("FAST_INPUT {}\n", if g.refresh != 0 { "OFF" } else { "ON" }));
    put(&mut out, &format!("CAPTURE {}\n", on_off(g.capture)));
    put(&mut out, &format!("CONTROL {}\n", on_off(g.control)));
    put(&mut out, "TAKE_REC ");
    out.extend_from_slice(c_str(&g.take_directory));
    out.push(b' ');
    out.extend_from_slice(c_str(&g.take_name));
    out.push(b'\n');
    put(&mut out, &format!("FRAMERATE {}\n\n", on_off(g.framerate)));

    put(&mut out, &format!("#\n# DRIVER SETUP INFORMATION ({} drivers)\n#\n", g.drivers.len()));
    let directory = c_str(ctx.driver_directory);
    for (i, d) in g.drivers.iter().enumerate() {
        put(&mut out, &format!("\n# DRIVER {i} CONFIG INFORMATION\n"));
        put(&mut out, &format!("DRIVER {} ", on_off(d.active)));
        out.extend_from_slice(fmt_s(&d.name));
        put(&mut out, &format!(" {}\n", d.ndof));
        put(&mut out, &format!("REMOTE {} ", d.remote));
        // host and location go through sscanf("%s"): only their first word is written
        out.extend_from_slice(first_word(&d.host).unwrap_or(EMPTY_STRING));
        put(&mut out, " \n\t");
        let location = match first_word(&d.location) {
            None => EMPTY_STRING,
            Some(_) if c_str(&d.location) == directory => DEFAULT_LOCATION,
            Some(word) => word,
        };
        out.extend_from_slice(location);
        put(&mut out, " \n");
        let frequency = if d.frequency as f64 == DEFAULT_FREQUENCY as f64 { "DEFAULT".to_string() } else { fmt_f(d.frequency as f64, 0, 4) };
        // any other run mode: no line
        if let Some(mode) = ["SYNCED", "FREE", "FREQUENCY"].get(d.run_mode as u32 as usize) {
            put(&mut out, &format!("RUNMODE {mode} {frequency}\n"));
        }
        // `SI_cusDialogWriteData` (0x08524054) writes the string of the dialog's encoder,
        // which for a driver without a dialog is the two lines below
        match (ctx.data_sections, &d.data_section) {
            (DataSections::AsRead, Some(section)) => out.extend_from_slice(&data_section_lines(section)),
            _ => put(&mut out, "_DATA\n_END\n"),
        }
    }
    put(&mut out, "\n");

    put(&mut out, &format!("#\n# CHANNELS INFORMATION ({} channels used)\n#\n", g.channel_count()));
    for (i, d) in g.drivers.iter().enumerate() {
        for k in 0..d.ndof.max(0) {
            let dof = d.dofs.get(k as usize).ok_or(WriteError::MissingChannels { driver: i })?;
            put(
                &mut out,
                &format!(
                    "CHANNEL {} {} {} {} {} {}",
                    k.wrapping_add(d.first_channel),
                    cnum::fmt_g(dof.low as f64, 6),
                    cnum::fmt_g(dof.high as f64, 6),
                    cnum::fmt_g(dof.min as f64, 6),
                    cnum::fmt_g(dof.max as f64, 6),
                    on_off(dof.active)
                ),
            );
            put(&mut out, match dof.kind {
                1 => " I",
                2 => " O",
                _ => " -",
            });
            put(&mut out, " NO ");
            out.extend_from_slice(fmt_s(&dof.name));
            out.push(b'\n');
        }
    }
    put(&mut out, "\n");

    put(&mut out, "#\n# CHANNEL CONNECTIONS INFORMATION\n#\n");
    for (i, item) in g.items.iter().enumerate() {
        let Connection::Expression(text) = &item.connection else {
            return Err(WriteError::LegacyConnection { item: i });
        };
        put(&mut out, &format!("ITEM {} \"", if item.enabled != 0 { "ON" } else { "FALSE" }));
        out.extend_from_slice(c_str(text));
        put(&mut out, "\"\n");
    }
    put(&mut out, "\n");

    put(&mut out, "#\n# CHANNEL CALIBRATION INFORMATION\n#\n");
    put(&mut out, "CALIBRATE\n");
    for item in &g.items {
        let [a, b, c] = item.calibration;
        put(
            &mut out,
            &format!(
                "   {} {} {} {} {}\n",
                on_off(item.enabled),
                on_off(item.calibrated),
                cnum::fmt_g(a as f64, 6),
                cnum::fmt_g(b as f64, 6),
                cnum::fmt_g(c as f64, 6)
            ),
        );
    }
    put(&mut out, "\n");
    put(&mut out, "#\n# END OF CHANNEL SETUP FILE\n#\n");
    Ok(out)
}

/// What the corpus check finds for one file.
#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    /// The read's error, if it gave nothing.
    pub error: Option<ChnError>,
    pub info: Option<ReadInfo>,
    /// Drivers, channels and connections read.
    pub counts: (usize, i32, usize),
    /// The setup can be written (no connection of the older form).
    pub writable: bool,
    /// Written with the data sections as read and read again, the setup is the same.
    pub same_after_rewrite: bool,
    /// Written with the file's header version and the data sections as read, the bytes are
    /// the file's.
    pub identical: bool,
    /// The same with empty data sections, which is what the original writes for drivers that
    /// are not installed.
    pub identical_without_data: bool,
    /// Connections of the form older than 3.0, which cannot be written here.
    pub legacy_connections: usize,
    /// For a setup with such connections: written without them and read again, the rest of
    /// the setup (switches, drivers, channels, data sections) is the same.
    pub same_without_legacy: bool,
    /// The rewrite that was compared (for a setup with legacy connections, the one without
    /// them).
    pub rewritten: Vec<u8>,
    /// For a file that does not come back byte for byte: what differs, one word per cause.
    pub causes: Vec<&'static str>,
}

/// Reads a file, writes it and reads it again: the corpus check for one file.
pub fn check(data: &[u8]) -> Check {
    let mut c = Check {
        error: None,
        info: None,
        counts: (0, 0, 0),
        writable: false,
        same_after_rewrite: false,
        identical: false,
        identical_without_data: false,
        legacy_connections: 0,
        same_without_legacy: false,
        rewritten: Vec::new(),
        causes: Vec::new(),
    };
    let (g, info) = match read(data) {
        Ok(x) => x,
        Err(e) => {
            c.error = Some(e);
            return c;
        }
    };
    c.counts = (g.drivers.len(), g.channel_count(), g.items.len());
    let ctx = WriteContext { header_version: info.version, data_sections: DataSections::AsRead, ..WriteContext::default() };
    if let Ok(out) = write(&g, &ctx) {
        c.writable = true;
        c.identical = out == data;
        if let Ok((g2, _)) = read(&out) {
            c.same_after_rewrite = g2 == g;
        }
        if let Ok(bare) = write(&g, &WriteContext { data_sections: DataSections::Empty, ..ctx }) {
            c.identical_without_data = bare == data;
        }
        if !c.identical {
            c.causes = difference_causes(data, &out, &info);
        }
        c.rewritten = out;
    } else {
        c.causes.push("connections of the form before 3.0");
        if info.channels_version < CHANNELS_VERSION {
            c.causes.push("older layout");
        }
        let mut rest = g.clone();
        rest.items.retain(|item| matches!(item.connection, Connection::Expression(_)));
        c.legacy_connections = g.items.len() - rest.items.len();
        if let Ok(out) = write(&rest, &ctx) {
            if let Ok((g2, _)) = read(&out) {
                // a setup without connections reads back with the flag for "a connection was
                // added" clear
                rest.items_changed = g2.items_changed;
                c.same_without_legacy = g2 == rest;
            }
            if !c.same_without_legacy {
                if let Ok((mut g2, _)) = read(&out) {
                    g2.drivers.iter_mut().for_each(|d| d.data_section = None);
                    rest.drivers.iter_mut().for_each(|d| d.data_section = None);
                    if g2 == rest {
                        c.causes.push("the writer adds a data section the file did not have");
                    }
                }
            }
            c.rewritten = out;
        }
    }
    c.info = Some(info);
    c
}

/// What the corpus check finds for one driver configuration file.
#[derive(Clone, Debug, PartialEq)]
pub struct DescriptionCheck {
    pub error: Option<ChnError>,
    pub info: Option<DescriptionInfo>,
    /// Channels, channels with a name from the file, error messages, messages with a text
    /// from the file.
    pub counts: (i32, usize, usize, usize),
    /// The same four numbers counted from the file's lines without the reader: the number
    /// after the last `DOF_NUM`, the distinct in-range numbers after `DOF_NAME`, the number
    /// after `ERROR_NUM`, the distinct in-range numbers after `ERROR`.
    pub expected: (i32, usize, usize, usize),
}

/// Reads a driver configuration file and counts what it gave: the corpus check for one file.
/// There is no writer for these files in the original, so there is nothing to compare bytes
/// with; the counts are compared with a plain count of the file's lines.
pub fn check_description(data: &[u8]) -> DescriptionCheck {
    let mut drv = Driver::new(DEFAULT_DRIVER_DIRECTORY);
    let mut c = DescriptionCheck { error: None, info: None, counts: (0, 0, 0, 0), expected: (0, 0, 0, 0) };
    match read_driver_description(&mut drv, data, false) {
        Ok(info) => c.info = Some(info),
        Err(e) => c.error = Some(e),
    }
    let named = drv.dofs.iter().enumerate().filter(|(i, d)| d.name.as_deref() != Some(format!("DOF{i}").as_bytes())).count();
    let texts = drv.errors.iter().enumerate().filter(|(i, e)| e.as_deref() != Some(format!("error number {i}").as_bytes())).count();
    c.counts = (drv.ndof, named, drv.errors.len(), texts);
    let number = |w: Option<&[u8]>| w.and_then(|w| std::str::from_utf8(w).ok()).and_then(|w| w.parse::<i32>().ok());
    let (mut ndof, mut nerr) = (0i32, 0i32);
    let (mut names, mut errors) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
    for line in data.split(|&b| b == b'\n') {
        let mut words = line.split(|b| b.is_ascii_whitespace()).filter(|w| !w.is_empty());
        match (words.next(), number(words.next())) {
            (Some(b"DOF_NUM"), Some(n)) => ndof = n,
            (Some(b"ERROR_NUM"), Some(n)) => nerr = n,
            (Some(b"DOF_NAME"), Some(n)) if n >= 0 && n < ndof => {
                names.insert(n);
            }
            (Some(b"ERROR"), Some(n)) if n >= 0 && n < nerr => {
                errors.insert(n);
            }
            _ => {}
        }
    }
    c.expected = (ndof, names.len(), nerr as usize, errors.len());
    c
}

/// How the reader ends on a file cut short: the outcomes of reading each prefix of `data`
/// whose length is a multiple of `step`, as counts of prefixes that
/// 0. read with nothing left over and no statement cut short,
/// 1. read, with a statement that wanted more input or with words left unread,
/// 2. are refused (no `CHANNELS` keyword),
/// 3. make the original loop for ever,
/// 4. are stopped here for a driver too large.
pub fn truncation_outcomes(data: &[u8], step: usize) -> [usize; 5] {
    let mut n = [0usize; 5];
    for len in (0..data.len()).step_by(step.max(1)) {
        let k = match read(&data[..len]) {
            Ok((_, info)) if info.all_tokens_consumed && !info.read_past_end => 0,
            Ok(_) => 1,
            Err(ChnError::InvalidId) => 2,
            Err(ChnError::EndlessLoop { .. }) => 3,
            Err(ChnError::HugeDriver { .. } | ChnError::HugeErrorCount { .. }) => 4,
        };
        n[k] += 1;
    }
    n
}

/// Sorts the lines that differ between a file and its rewrite by the statement they belong to.
fn difference_causes(file: &[u8], written: &[u8], info: &ReadInfo) -> Vec<&'static str> {
    let mut causes: Vec<&'static str> = Vec::new();
    if info.channels_version < CHANNELS_VERSION {
        causes.push("older layout");
        return causes;
    }
    let (a, b): (Vec<&[u8]>, Vec<&[u8]>) = (file.split(|&x| x == b'\n').collect(), written.split(|&x| x == b'\n').collect());
    if a.len() != b.len() {
        causes.push("number of lines");
        return causes;
    }
    let mut in_data = false;
    for (x, y) in a.iter().zip(&b) {
        if x.starts_with(b"_DATA") {
            in_data = true;
        }
        if x != y {
            let cause = if in_data {
                "data section line"
            } else if x.starts_with(b"CHANNEL ") {
                "CHANNEL line"
            } else if x.starts_with(b"ITEM ") {
                "ITEM line"
            } else if x.starts_with(b"REMOTE") || x.starts_with(b"\t") {
                "host or location"
            } else if x.starts_with(b"   ") {
                "CALIBRATE line"
            } else {
                "other line"
            };
            if !causes.contains(&cause) {
                causes.push(cause);
            }
        }
        if x.starts_with(b"_END") {
            in_data = false;
        }
    }
    causes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(data: &[u8], fmt: &str) -> (i32, Vec<u8>, i32, f32, usize, bool) {
        let mut f = CFile::new(data);
        let (mut s, mut d, mut x) = (b"unset".to_vec(), -77, -7.5f32);
        let n = f.scanf(fmt, &mut [Arg::Str(&mut s), Arg::Int(&mut d), Arg::Float(&mut x)]);
        (n, s, d, x, f.tell(), f.feof())
    }

    #[test]
    fn scanf_words_and_numbers() {
        assert_eq!(scan_all(b"  ab\t 12 3.5x", "%s %d %f"), (3, b"ab".to_vec(), 12, 3.5, 12, false));
        // the end of the file inside the last value sets the flag; before the first value the
        // result is EOF, after it the count
        assert_eq!(scan_all(b"ab 12 3.5", "%s %d %f"), (3, b"ab".to_vec(), 12, 3.5, 9, true));
        assert_eq!(scan_all(b"  \n", "%s %d %f"), (EOF, b"unset".to_vec(), -77, -7.5, 3, true));
        assert_eq!(scan_all(b"ab ", "%s %d %f"), (1, b"ab".to_vec(), -77, -7.5, 3, true));
        // a word where a number should be: nothing consumed but the white space
        assert_eq!(scan_all(b"ab cd", "%s %d %f"), (1, b"ab".to_vec(), -77, -7.5, 3, false));
        // a sign with no digit is consumed and the scan stops
        assert_eq!(scan_all(b"ab -x", "%s %d %f"), (1, b"ab".to_vec(), -77, -7.5, 4, false));
        assert_eq!(scan_all(b"ab 5 -x", "%s %d %f"), (2, b"ab".to_vec(), 5, -7.5, 6, false));
        // digits stop a %d at the point; %f then takes ".5e" whole and converts ".5"
        assert_eq!(scan_all(b"ab 1.5e x", "%s %d %f"), (3, b"ab".to_vec(), 1, 0.5, 7, false));
        assert_eq!(scan_all(b"ab -12 -.25", "%s %d %f"), (3, b"ab".to_vec(), -12, -0.25, 11, true));
        assert_eq!(scan_all(b"ab 99999999999 1e2", "%s %d %f"), (3, b"ab".to_vec(), i32::MAX, 100.0, 18, true));
        assert_eq!(scan_all(b"ab 1 . 5", "%s %d %f"), (2, b"ab".to_vec(), 1, -7.5, 6, false));
        assert_eq!(scan_all(b"ab 1 INF", "%s %d %f").3, f32::INFINITY);
    }

    #[test]
    fn scanf_widths_sets_and_literals() {
        let mut f = CFile::new(b"ONE two\n");
        let (mut a, mut b) = (Vec::new(), Vec::new());
        assert_eq!(f.scanf("%1s %3s", &mut [Arg::Str(&mut a), Arg::Str(&mut b)]), 2);
        assert_eq!((a.as_slice(), b.as_slice(), f.tell()), (&b"O"[..], &b"NE"[..], 3));
        // a quoted string: the quotes must be there, the text may hold anything else
        let mut f = CFile::new(b" ON  \"a b = 2 * c\" tail");
        let (mut a, mut b) = (Vec::new(), Vec::new());
        assert_eq!(f.scanf("%s \"%[^\"]\"", &mut [Arg::Str(&mut a), Arg::Str(&mut b)]), 2);
        assert_eq!((a.as_slice(), b.as_slice(), f.tell()), (&b"ON"[..], &b"a b = 2 * c"[..], 18));
        // no opening quote: one value, the offending character left unread
        let mut f = CFile::new(b"ON x");
        assert_eq!(f.scanf("%s \"%[^\"]\"", &mut [Arg::Str(&mut a), Arg::Str(&mut b)]), 1);
        assert_eq!(f.tell(), 3);
        // an empty pair of quotes fails the set, which needs one character
        let mut f = CFile::new(b"ON \"\"");
        assert_eq!(f.scanf("%s \"%[^\"]\"", &mut [Arg::Str(&mut a), Arg::Str(&mut b)]), 1);
        assert_eq!(f.tell(), 4);
        // the name of a channel: spaces, the rest of the line, then every newline that follows
        let mut f = CFile::new(b"  left hand\n\n\nnext");
        let mut name = Vec::new();
        assert_eq!(f.scanf("%*[ ]%[^\n]%*[\n]", &mut [Arg::Str(&mut name)]), 1);
        assert_eq!((name.as_slice(), f.tell()), (&b"left hand"[..], 14));
        // without a leading space the first directive fails and nothing is stored
        let mut f = CFile::new(b"x\n");
        let mut name = b"old".to_vec();
        assert_eq!(f.scanf("%*[ ]%[^\n]%*[\n]", &mut [Arg::Str(&mut name)]), 0);
        assert_eq!((name.as_slice(), f.tell()), (&b"old"[..], 0));
        // a tab is not in the set of the first directive
        let mut f = CFile::new(b" \tx\n");
        assert_eq!(f.scanf("%*[ ]%[^\n]%*[\n]", &mut [Arg::Str(&mut name)]), 1);
        assert_eq!(name, b"\tx");
        // ranges in a set
        let mut f = CFile::new(b"abc123");
        assert_eq!(f.scanf("%[a-c]", &mut [Arg::Str(&mut name)]), 1);
        assert_eq!(name, b"abc");
    }

    #[test]
    fn printf_fixed() {
        assert_eq!(fmt_f(CHANNELS_VERSION as f64, 6, 2), "  3.51");
        assert_eq!(fmt_f(12.5, 0, 4), "12.5000");
        assert_eq!(fmt_f(0.125, 0, 2), "0.12"); // a tie goes to the even digit, as in glibc
        assert_eq!(fmt_f(0.375, 0, 2), "0.38");
        assert_eq!(fmt_f(-0.05f32 as f64, 0, 6), "-0.050000");
        assert_eq!(fmt_f(f64::INFINITY, 0, 6), "inf");
        assert_eq!(first_word(b"  ab cd"), Some(&b"ab"[..]));
        assert_eq!(first_word(b" \t"), None);
    }

    #[test]
    fn keywords() {
        assert_eq!(token_type(b"CHANNELS"), 2);
        assert_eq!(token_type(b"RUNMODE"), token_type(b"FREERUN"));
        assert_eq!(token_type(b"_END"), 43);
        assert_eq!(token_type(b"_DATA"), 0);
        assert_eq!(token_type(b""), 0); // the table's last entry, the empty string, ends the search
        assert_eq!(token_type(b"channels"), 0);
        assert_eq!(KEYWORDS.iter().map(|k| k.1).max(), Some(43));
    }

    /// A made-up setup in the current layout.
    const SAMPLE: &[u8] = b"SCHN3.71\n\
CHANNELS   3.51\n\
Softimage Creative Environment\n\
#\n\
# CHANNEL SETUP INFORMATION\n\
#\n\
FAST_INPUT OFF\n\
CAPTURE ON\n\
CONTROL OFF\n\
TAKE_REC /var/takes walk\n\
FRAMERATE ON\n\
\n\
#\n\
# DRIVER SETUP INFORMATION (2 drivers)\n\
#\n\
\n\
# DRIVER 0 CONFIG INFORMATION\n\
DRIVER ON wheel 2\n\
REMOTE 0 DRIVER_EMPTY_STRING \n\
\tDEFAULT_DRIVER_LOCATION \n\
RUNMODE SYNCED DEFAULT\n\
_DATA\n\
_END\n\
\n\
# DRIVER 1 CONFIG INFORMATION\n\
DRIVER OFF glove 1\n\
REMOTE 3 farhost \n\
\t/opt/drivers \n\
RUNMODE FREQUENCY 12.5000\n\
_DATA\n\
SPEED 4\n\
GAIN 0.250000\n\
_END\n\
\n\
#\n\
# CHANNELS INFORMATION (3 channels used)\n\
#\n\
CHANNEL 0 0 1 -9999 9999 ON I NO turn\n\
CHANNEL 1 0.5 2 -10 10 OFF O NO push hard\n\
CHANNEL 2 0 1 -9999 9999 ON - NO bend\n\
\n\
#\n\
# CHANNEL CONNECTIONS INFORMATION\n\
#\n\
ITEM ON \"box.rotx = 30 * CHN wheel.turn\"\n\
ITEM FALSE \"CHN wheel.push hard = box.posy + 1\"\n\
\n\
#\n\
# CHANNEL CALIBRATION INFORMATION\n\
#\n\
CALIBRATE\n\
\x20  ON OFF 0 0 0\n\
\x20  OFF ON 1.5 -2 0.25\n\
\n\
#\n\
# END OF CHANNEL SETUP FILE\n\
#\n";

    #[test]
    fn reads_the_current_layout() {
        let (g, info) = read(SAMPLE).unwrap();
        assert_eq!((info.version, info.channels_version, info.ok), (3.71, 3.51, true));
        assert!(info.all_tokens_consumed && !info.read_past_end && info.messages.is_empty());
        assert_eq!((info.overruns, info.end), (0, SAMPLE.len()));
        assert_eq!((g.refresh, g.capture, g.control, g.framerate), (1, 1, 0, 1));
        assert_eq!((g.take_directory.as_slice(), g.take_name.as_slice()), (&b"/var/takes"[..], &b"walk"[..]));
        assert_eq!((g.take_slot, g.take_last), (1, -1));
        assert_eq!(g.drivers.len(), 2);
        let d = &g.drivers[0];
        assert_eq!((d.name.as_deref(), d.ndof, d.active, d.first_channel, d.info_state), (Some(&b"wheel"[..]), 2, 1, 0, -1));
        assert_eq!((d.remote, d.host.as_slice(), d.location.as_slice()), (0, &b""[..], DEFAULT_DRIVER_DIRECTORY));
        assert_eq!((d.run_mode, d.frequency), (0, 10.0));
        assert_eq!(d.data_section.as_deref(), Some(&b"_DATA\n\n_END"[..]));
        let d = &g.drivers[1];
        assert_eq!((d.name.as_deref(), d.ndof, d.active, d.first_channel), (Some(&b"glove"[..]), 1, 0, 2));
        assert_eq!((d.remote, d.host.as_slice(), d.location.as_slice()), (3, &b"farhost"[..], &b"/opt/drivers"[..]));
        assert_eq!((d.run_mode, d.frequency), (2, 12.5));
        assert_eq!(d.data_section.as_deref(), Some(&b"_DATA\n\nSPEED 4\nGAIN 0.250000\n_END"[..]));
        let dof = &g.drivers[0].dofs[1];
        assert_eq!((dof.name.as_deref(), dof.kind, dof.active), (Some(&b"push hard"[..]), 2, 0));
        assert_eq!((dof.low, dof.high, dof.min, dof.max), (0.5, 2.0, -10.0, 10.0));
        assert_eq!(g.drivers[1].dofs[0].kind, 0);
        assert_eq!(g.channel_count(), 3);
        assert_eq!(g.items.len(), 2);
        assert_eq!(g.items[0].connection, Connection::Expression(b"box.rotx = 30 * CHN wheel.turn".to_vec()));
        assert_eq!((g.items[0].enabled, g.items[0].calibrated, g.items[0].calibration), (1, 0, [0.0; 3]));
        assert_eq!((g.items[1].enabled, g.items[1].calibrated, g.items[1].calibration), (0, 1, [1.5, -2.0, 0.25]));
        assert_eq!(g.items_changed, 1);
    }

    #[test]
    fn writes_the_current_layout_back() {
        let (g, info) = read(SAMPLE).unwrap();
        let ctx = WriteContext { header_version: info.version, data_sections: DataSections::AsRead, ..WriteContext::default() };
        assert_eq!(String::from_utf8(write(&g, &ctx).unwrap()).unwrap(), String::from_utf8(SAMPLE.to_vec()).unwrap());
        let c = check(SAMPLE);
        assert!(c.writable && c.identical && c.same_after_rewrite && !c.identical_without_data);
        // the original's own header version, and what it writes for drivers that are not
        // installed
        let out = write(&g, &WriteContext::default()).unwrap();
        assert!(out.starts_with(b"SCHN4\nCHANNELS   3.51\n"));
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("RUNMODE FREQUENCY 12.5000\n_DATA\n_END\n\n#\n"));
        assert!(!text.contains("SPEED"));
    }

    #[test]
    fn writer_special_cases() {
        let mut g = ChannelSetup::new();
        let mut d = Driver::new(b"/drv");
        d.dof_alloc(1);
        d.host = b" two words".to_vec();
        d.location = b"/drv".to_vec();
        d.run_mode = 7;
        g.drv_append(d);
        let mut d = Driver::new(b"/drv");
        d.name = Some(b"b".to_vec());
        d.location = b" ".to_vec();
        d.run_mode = 1;
        d.frequency = 0.5;
        g.drv_append(d);
        g.refresh = 0;
        let out = write(&g, &WriteContext { driver_directory: b"/drv", ..WriteContext::default() }).unwrap();
        let text = String::from_utf8(out).unwrap();
        // a null name, the first word of the host, the directory as the default location, no
        // RUNMODE line for a mode out of range
        assert!(text.contains("DRIVER OFF (null) 1\nREMOTE 0 two \n\tDEFAULT_DRIVER_LOCATION \n_DATA\n_END\n"));
        assert!(text.contains("DRIVER OFF b 0\nREMOTE 0 DRIVER_EMPTY_STRING \n\tDRIVER_EMPTY_STRING \nRUNMODE FREE 0.5000\n"));
        assert!(text.contains("FAST_INPUT ON\nCAPTURE ON\nCONTROL ON\nTAKE_REC /usr/tmp default\nFRAMERATE OFF\n"));
        assert!(text.contains("CHANNEL 0 0 1 -9999 9999 ON I NO (null)\n"));
        // a legacy connection cannot be written
        g.items.push(Item {
            valid: 1,
            enabled: 1,
            connection: Connection::Legacy {
                element_type: vec![],
                element: vec![],
                qualifier: vec![],
                curve_type: 1,
                curve_index: 0,
                channel: 0,
                offset: 0.0,
                scale: 1.0,
                driver: 0,
                dof: 0,
                input: true,
            },
            calibrated: 0,
            calibration: [0.0; 3],
        });
        assert_eq!(write(&g, &WriteContext::default()), Err(WriteError::LegacyConnection { item: 0 }));
    }

    #[test]
    fn older_layouts() {
        // 2.65 as a single is not above 2.6501: channels carry no type and no name, and are
        // named by the step after the last DRIVER; connections are of the older form
        let data = b"SCHN2.64933\nCHANNELS   2.65\nSoftimage Creative Environment\n\nFAST_INPUT ON\nFRAMERATE OFF\n\
DRIVER ON wand 3\n_DATA\n SPEED  4\n_END\n\nCHANNEL 0  0 1 -9999 9999  ON\nCHANNEL 2  0 1 5 -5  OFF\nCHANNEL 3  0 1 0 0  OFF\n\n\
ITEM MODEL ball no 139 -1 2  0.5 2  ON\nITEM MODEL ball no 0 -1 1  0 1  ON\nITEM MODEL ball no 4 -1 7  0 1  ON\n\nCALIBRATE\n   ON ON 1 2 3\n";
        let (g, info) = read(data).unwrap();
        assert_eq!((info.version, info.channels_version), (2.64933, 2.65));
        // the third ITEM names a channel no driver has
        assert!(!info.ok && info.all_tokens_consumed && !info.read_past_end);
        assert_eq!(info.messages, vec![Message { severity: 3, text: b"Bad connection in channel setup file ''.".to_vec() }]);
        assert_eq!((g.refresh, g.capture, g.control), (0, 1, 1)); // CAPTURE and CONTROL as a new setup has them
        let d = &g.drivers[0];
        assert_eq!(d.data_section.as_deref(), Some(&b"_DATA\n\n SPEED  4\n_END"[..]));
        assert_eq!((d.remote, d.run_mode, d.frequency, d.location.as_slice()), (0, 0, 10.0, DEFAULT_DRIVER_DIRECTORY));
        assert_eq!(d.dofs.iter().map(|x| x.name.clone().unwrap()).collect::<Vec<_>>(), vec![b"DOF0".to_vec(), b"DOF1".to_vec(), b"DOF2".to_vec()]);
        // the upper bound is raised to the lower one; channel 3 does not exist and is skipped
        assert_eq!((d.dofs[2].min, d.dofs[2].max, d.dofs[2].active, d.dofs[2].kind), (5.0, 5.0, 0, 1));
        // curve type 0 is dropped; the calibration line goes to the one connection kept
        assert_eq!(g.items.len(), 1);
        assert_eq!(
            g.items[0].connection,
            Connection::Legacy {
                element_type: b"MODEL".to_vec(),
                element: b"ball".to_vec(),
                qualifier: b"no".to_vec(),
                curve_type: 139,
                curve_index: -1,
                channel: 2,
                offset: 0.5,
                scale: 2.0,
                driver: 0,
                dof: 2,
                input: true
            }
        );
        assert_eq!((g.items[0].enabled, g.items[0].calibrated, g.items[0].calibration), (1, 1, [1.0, 2.0, 3.0]));
        assert!(!check(data).writable);

        // 2.60: no data section is read, so _DATA is just an unknown word
        let (g, info) = read(b"CHANNELS 2.60\nDRIVER OFF tape 2\nCHANNEL 1 0 1 -1 1 ON\n").unwrap();
        assert_eq!((info.version, info.header_tag, info.ok), (-1.0, None, true));
        assert_eq!((g.drivers[0].data_section.clone(), g.drivers[0].active, g.drivers[0].dofs[1].max), (None, 0, 1.0));

        // 3.5: host and location without the REMOTE keyword, and no RUNMODE
        let (g, info) = read(b"CHANNELS 3.5\nDRIVER ON tape 1\n4 box /there\n_DATA\n_END\nFRAMERATE ON\n").unwrap();
        assert!(info.ok && info.all_tokens_consumed);
        assert_eq!((g.drivers[0].remote, g.drivers[0].host.as_slice(), g.drivers[0].location.as_slice(), g.framerate), (4, &b"box"[..], &b"/there"[..], 1));

        // up to 2.0 the description comes from the driver itself, of which there is none: the
        // old names are replaced from the table and the read reports failure
        let (g, info) = read(b"CHANNELS 2.0\nDRIVER ON dials_driver\nDRIVER OFF micro_freqs\nDRIVER ON other\nFRAMERATE ON\n").unwrap();
        assert!(!info.ok);
        assert_eq!(g.drivers.iter().map(|d| (d.name.clone().unwrap(), d.ndof, d.active, d.first_channel)).collect::<Vec<_>>(),
                   vec![(b"dialbox".to_vec(), 8, 1, 0), (b"micro_freqs".to_vec(), 8, 0, 8), (b"other".to_vec(), 0, 1, 16)]);
        assert_eq!(g.drivers[0].dofs[7].name.as_deref(), Some(&b"DOF7"[..]));
        assert!(info.messages.iter().any(|m| m.text == b"Channel driver 'dials_driver' replaced by 'dialbox'."));
        let (g, _) = read(b"CHANNELS 1.0\nDRIVER tape\nFRAMERATE ON\n").unwrap();
        assert_eq!((g.drivers[0].name.as_deref(), g.drivers[0].active), (Some(&b"tape"[..]), 1));
    }

    #[test]
    fn switches_and_unknown_words() {
        // unknown words are skipped silently, one at a time; a switch takes the next keyword
        // whatever it is
        let (g, info) = read(b"CHANNELS 3.51\nbanana FRAMERATE maybe ON\nTIMESTAMP OFF\nDRIVER_SYNC OFF DRIVER_DEBUG ON\nREFRESH OFF\nCAPTURE OFF\nCONTROL OFF\n").unwrap();
        assert!(info.ok && info.all_tokens_consumed);
        assert_eq!((g.framerate, g.timestamp, g.driver_sync, g.driver_debug, g.refresh), (1, 0, 1, 1, 0));
        // neither CAPTURE nor CONTROL: CAPTURE comes back on
        assert_eq!((g.capture, g.control), (1, 0));
        // a keyword with no meaning at this level loses its line
        let (g, _) = read(b"CHANNELS 3.51\nMODEL FRAMERATE ON\nSYNCED\n").unwrap();
        assert_eq!(g.framerate, 0);
        // "#word" is not a comment: the words after it are read
        let (g, _) = read(b"CHANNELS 3.51\n#no FRAMERATE ON\n# FAST_INPUT ON\n").unwrap();
        assert_eq!((g.framerate, g.refresh), (1, 1));
        // TAKE_REC before 3.51 is not read: its words are unknown words to the main loop
        let (g, _) = read(b"CHANNELS 3.5\nTAKE_REC /x y\n").unwrap();
        assert_eq!(g.take_directory, b"/usr/tmp");
        // a take file that exists
        let ctx = Context { first_free_take_slot: |path| if path == b"/x/y.CHD" { 4 } else { -1 }, ..Context::default() };
        let mut g = ChannelSetup::new();
        read_into(&mut g, b"CHANNELS 3.51\nTAKE_REC /x y\n", &ctx).unwrap();
        assert_eq!((g.take_slot, g.take_last), (4, 3));
        // the version is the first thing that looks like a number, whatever comes before
        let (_, info) = read(b"CHANNELS of version 3.51\n").unwrap();
        assert_eq!((info.channels_version, info.version_missing), (3.51, false));
        let (_, info) = read(b"CHANNELS -x\n").unwrap();
        assert!(info.version_missing);
    }

    #[test]
    fn bad_statements_do_not_stop_the_read() {
        si_rt::skip_without_install!();
        // no REMOTE keyword: two messages, no driver, and the read goes on
        let (g, info) = read(b"CHANNELS 3.51\nDRIVER ON a 1\nRUNMODE SYNCED DEFAULT\nFRAMERATE ON\n").unwrap();
        assert!(!info.ok && g.drivers.is_empty() && g.framerate == 1);
        assert_eq!(info.messages.iter().map(|m| m.text.clone()).collect::<Vec<_>>(),
                   vec![b"File '' has an invalid remote entry in file".to_vec(), si_rt::message(0x086e_d640).to_vec()]);
        // a channel statement cut short
        let (_, info) = read(b"CHANNELS 3.51\nDRIVER ON a 1\nREMOTE 0 h l\nRUNMODE FREE 5\n_DATA\n_END\nCHANNEL 0 0 1 x\n").unwrap();
        assert!(!info.ok);
        assert_eq!(info.messages[0].text, b"Bad format for channel in channel setup file ''.".to_vec());
        // an ITEM without quotes
        let ctx = Context { file_name: b"a.chn", ..Context::default() };
        let mut g = ChannelSetup::new();
        let info = read_into(&mut g, b"CHANNELS 3.51\nITEM ON box.rotx\nFRAMERATE ON\n", &ctx).unwrap();
        assert!(!info.ok && g.items.is_empty() && g.framerate == 1);
        assert_eq!(info.messages[0], Message { severity: 3, text: b"Bad connection in channel setup file 'a.chn'.".to_vec() });
        // a driver that would pass 1000 channels is dropped
        let (g, info) = read(b"CHANNELS 2.5\nDRIVER ON a 600\nDRIVER ON b 401\nDRIVER ON c 400\nFRAMERATE ON\n").unwrap();
        assert!(info.ok);
        assert_eq!(g.drivers.iter().map(|d| (d.ndof, d.first_channel)).collect::<Vec<_>>(), vec![(600, 0), (400, 600)]);
        let (g, _) = read(b"CHANNELS 2.5\nDRIVER ON a 2000000000\nFRAMERATE ON\n").unwrap();
        assert!(g.drivers.is_empty());
        assert_eq!(read(b"CHANNELS 2.5\nDRIVER ON a -5\nDRIVER ON b 1004\n").unwrap_err(), ChnError::HugeDriver { ndof: 1004 });
    }

    #[test]
    fn driver_lookup_switches_missing_drivers_off() {
        let data = b"CHANNELS 2.5\nDRIVER ON a 1\nDRIVER ON b 1\nFRAMERATE ON\n";
        let ctx = Context { driver_lookup: true, driver_directory: b"/d", ..Context::default() };
        let mut g = ChannelSetup::new();
        let info = read_into(&mut g, data, &ctx).unwrap();
        assert!(info.ok);
        assert_eq!(g.drivers.iter().map(|d| (d.active, d.info_state)).collect::<Vec<_>>(), vec![(0, -1), (0, -1)]);
        assert_eq!(info.messages[1], Message { severity: 3, text: b"Channel driver:\n/d/b\ndoes not exist".to_vec() });
        assert_eq!(g.drivers[0].location, b"/d");
    }

    #[test]
    fn data_sections() {
        si_rt::skip_without_install!();
        let section = |data: &[u8]| {
            let mut f = CFile::new(data);
            let mut m = Vec::new();
            let s = read_data_section(&mut f, &mut m);
            (s, f.tell(), m.len())
        };
        // lines before the section are skipped, empty lines dropped, leading white space kept
        // on all lines but the first, and a prefix is enough
        assert_eq!(section(b"junk\n\n  _DATAx\n\n A 1\r\n\t_ENDING\nrest\n"), (Some(b"_DATAx\n\n A 1\n\t_ENDING".to_vec()), 31, 0));
        assert_eq!(section(b"_ANIMATION\nK 1\n_END\nskipped\n_DATA\nV 2\n_END\n"), (Some(b"_ANIMATION\n\nK 1\n_END\n_DATA\nV 2\n_END".to_vec()), 43, 0));
        assert_eq!(section(b"A 1\n_END\n"), (None, 9, 1));
        assert_eq!(section(b"_DATA\nA 1\n"), (None, 10, 1));
        assert_eq!(section(b"_ANIMATION\n_END\nA\n"), (None, 18, 1));
        assert_eq!(data_section_lines(b"_DATA\n\n A 1\n_END"), b"_DATA\n A 1\n_END\n");
        // a driver without its section takes the file up to the next section
        let (g, info) = read(b"CHANNELS 3.51\nDRIVER ON a 1\nREMOTE 0 h l\nRUNMODE ON 5\nFRAMERATE ON\nDRIVER ON b 1\nREMOTE 0 h l\nRUNMODE SYNCED 5\n_DATA\n_END\n").unwrap();
        assert_eq!((g.drivers.len(), g.framerate, g.drivers[0].run_mode, g.drivers[0].frequency), (1, 0, 2, 5.0));
        assert!(info.ok && info.all_tokens_consumed);
    }

    #[test]
    fn truncated_files() {
        si_rt::skip_without_install!();
        // no CHANNELS keyword
        assert_eq!(read(b"SCHN3.71\nFRAMERATE ON\n").unwrap_err(), ChnError::InvalidId);
        assert_eq!(read(b"").unwrap_err(), ChnError::InvalidId);
        // loops that only a newline ends
        assert_eq!(read(b"CHANNELS").unwrap_err(), ChnError::EndlessLoop { at: 8, what: "version number" });
        assert_eq!(read(b"CHANNELS 3.51\n# cut").unwrap_err(), ChnError::EndlessLoop { at: 15, what: "comment" });
        assert_eq!(read(b"CHANNELS 3.51\nSoftimage").unwrap_err(), ChnError::EndlessLoop { at: 23, what: "Softimage line" });
        assert!(matches!(read(b"CHANNELS 3.51\n_END"), Err(ChnError::EndlessLoop { .. })));
        assert!(matches!(read(b"CHANNELS 3.51\nITEM ON \"a\"\nCALIBRATE\n ON ON 0 0 0"), Err(ChnError::EndlessLoop { what: "CALIBRATE line", .. })));
        // a file that ends inside a statement but not inside such a loop is read, and says so
        let (g, info) = read(b"CHANNELS 3.51\nFRAMERATE ON\nDRIVER ON a").unwrap();
        assert!(!info.ok && info.read_past_end && info.all_tokens_consumed && g.framerate == 1);
        let (g, info) = read(b"CHANNELS 3.51\nFRAMERATE").unwrap();
        assert!(info.ok && info.read_past_end && g.framerate == 0);
        // the last word of a file with no newline after it ends the main loop
        let (g, info) = read(b"CHANNELS 3.51\nFRAMERATE ON").unwrap();
        assert!(info.ok && !info.read_past_end && g.framerate == 1);
        // a missing closing quote takes the following lines into the expression
        let (g, info) = read(b"CHANNELS 3.51\nITEM ON \"a = b\nFRAMERATE ON\n").unwrap();
        assert!(info.ok && info.read_past_end);
        assert_eq!(info.overruns, 1);
        assert_eq!(g.items[0].connection, Connection::Expression(b"a = b\nFRAMERATE ON\n".to_vec()));
    }

    #[test]
    fn driver_descriptions() {
        let data = b"CHANNEL_DRIVER   3.0\n\n# a comment\nNOTE word\nDOF_NUM 3\n\nDOF_NAME 0 turn\t# why\nDOF_NAME 2 bend\nDOF_NAME 7 none\n\
DOF_TYPE 1 O OFF\nDOF_TYPE 9 X maybe\nPORT_NUM 2 ignored\nERROR_NUM 3\n\nERROR 0 \"no licence\"\nERROR 2 \"two\nlines\" ERROR 5 \"x\"\n";
        let mut d = Driver::new(b"/drv");
        d.dof_alloc(1);
        let info = read_driver_description(&mut d, data, false).unwrap();
        assert_eq!((info.version, info.stopped, info.all_tokens_consumed, info.read_past_end, info.overruns), (3.0, false, true, false, 0));
        assert_eq!(d.ndof, 3);
        assert_eq!(d.dofs.iter().map(|x| (x.name.clone().unwrap(), x.kind, x.active)).collect::<Vec<_>>(),
                   vec![(b"turn".to_vec(), 1, 1), (b"DOF1".to_vec(), 2, 0), (b"bend".to_vec(), 1, 1)]);
        assert_eq!(d.errors, vec![Some(b"no licence".to_vec()), Some(b"error number 1".to_vec()), Some(b"two\nlines".to_vec())]);
        // a statement that does not scan ends the read without a message; so does _END
        let mut d = Driver::new(b"");
        let info = read_driver_description(&mut d, b"CHANNEL_DRIVER 3.0\nDOF_NUM 2\nDOF_TYPE 0 X ON\nDOF_NAME 0 a\n", false).unwrap();
        assert!(info.stopped && !info.all_tokens_consumed);
        assert_eq!(d.dofs[0].name.as_deref(), Some(&b"DOF0"[..]));
        let info = read_driver_description(&mut d, b"CHANNEL_DRIVER 3.0\nDOF_NUM 1\n_END\nDOF_NUM 5\n", false).unwrap();
        assert!(info.stopped && d.ndof == 1);
        let info = read_driver_description(&mut d, b"CHANNEL_DRIVER 3.0\nDOF_NUM 0\n", false).unwrap();
        assert!(!info.stopped && d.ndof == 0 && d.dofs.is_empty());
        // with a custom dialog the text of an error is the rest of the line
        let mut d = Driver::new(b"");
        read_driver_description(&mut d, b"CHANNEL_DRIVER 3.0\nERROR_NUM 1\nERROR 0 \"quoted\" text\nDOF_NUM 1\n", true).unwrap();
        assert_eq!((d.errors[0].as_deref(), d.ndof), (Some(&b"\"quoted\" text"[..]), 1));
        // a text of more than 255 bytes is cut; the rest of it is read as words
        let mut long = b"CHANNEL_DRIVER 3.0\nERROR_NUM 1\nERROR 0 \"".to_vec();
        long.extend_from_slice(&[b'x'; 300]);
        long.extend_from_slice(b"\"\n");
        let info = read_driver_description(&mut d, &long, false).unwrap();
        assert_eq!((d.errors[0].as_ref().unwrap().len(), info.all_tokens_consumed), (255, true));
        // refusals and loops
        assert_eq!(read_driver_description(&mut d, b"DOF_NUM 2\n", false).unwrap_err(), ChnError::InvalidId);
        assert!(matches!(read_driver_description(&mut d, b"CHANNEL_DRIVER 3.0\nPORT 1", false), Err(ChnError::EndlessLoop { .. })));
        assert_eq!(read_driver_description(&mut d, b"CHANNEL_DRIVER 3.0\nDOF_NUM 5000\n", false).unwrap_err(), ChnError::HugeDriver { ndof: 5000 });
        assert_eq!(read_driver_description(&mut d, b"CHANNEL_DRIVER 3.0\nERROR_NUM 5000\n", false).unwrap_err(), ChnError::HugeErrorCount { count: 5000 });
    }

    #[test]
    fn channel_lookup() {
        let mut g = ChannelSetup::new();
        assert_eq!(g.get_drv_dof(0), None);
        for n in [2, -3, 4] {
            let mut d = Driver::new(b"");
            d.dof_alloc(n);
            g.drv_append(d);
        }
        assert_eq!(g.drivers.iter().map(|d| d.first_channel).collect::<Vec<_>>(), vec![0, 2, -1]);
        assert_eq!(g.channel_count(), 3);
        assert_eq!((g.get_drv_dof(0), g.get_drv_dof(1), g.get_drv_dof(2), g.get_drv_dof(3), g.get_drv_dof(-1)), (Some((0, 0)), Some((0, 1)), Some((2, 3)), None, None));
    }
}
