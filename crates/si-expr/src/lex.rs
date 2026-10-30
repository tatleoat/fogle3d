// SPDX-License-Identifier: MIT
//! The scanner of the expression language (`exp_lex.c`, finding 0013).
//!
//! The original is a generated scanner (flex, "fast" tables) [soft 0x08256a20 EXP_yylex]. Its
//! rules were recovered from the tables and are proved equal to them by
//! `re/tools/si-expr/lex_check.py`, which also writes [`crate::lex_tables`]. Its input is not
//! a file: the buffer filler hands over one byte at a time from the string being parsed
//! [soft 0x08257140 yy_get_next_buffer], so the scanner behaves like a reader of a byte
//! stream, and what it has taken from the string but not yet turned into a token stays in
//! its buffer when a parse is given up. The next parse starts with those bytes.

use crate::env::Dict;
use crate::lex_tables::{ACCEPT, CLASS, NEXT};
use crate::token;
use si_ascii::cnum;

/// Token numbers the scanner returns (the grammar's names are ours; the binary has none).
pub mod tok {
    pub const INT: i32 = 257;
    pub const RELNAME: i32 = 258;
    pub const FS: i32 = 259;
    pub const FE: i32 = 260;
    pub const FC: i32 = 261;
    pub const T: i32 = 262;
    pub const RAND: i32 = 263;
    pub const RANDBOOL: i32 = 264;
    pub const RAND01: i32 = 265;
    pub const EQ: i32 = 282;
    pub const LIKE: i32 = 283;
    pub const NOT: i32 = 284;
    pub const NE: i32 = 285;
    pub const ADDASSIGN: i32 = 286;
    pub const SUBASSIGN: i32 = 287;
    pub const MULASSIGN: i32 = 288;
    pub const DIVASSIGN: i32 = 289;
    pub const LT: i32 = 290;
    pub const LE: i32 = 291;
    pub const GT: i32 = 292;
    pub const GE: i32 = 293;
    pub const AND: i32 = 294;
    pub const OR: i32 = 295;
    pub const FLOAT: i32 = 296;
    pub const IDENT: i32 = 297;
    pub const STRING: i32 = 298;
    pub const ELEMPARAM: i32 = 300;
    pub const ENUMVAL: i32 = 301;
}

/// One 4-byte word of a semantic value: the original's `yylval` is a union of an integer, a
/// float and pointers, and the parser copies it around without knowing which it is.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Sem {
    /// Zero: a null pointer, or a word nothing has written yet.
    #[default]
    Null,
    Int(i32),
    Float(f32),
    /// Text the scanner duplicated (`strdup`); whoever holds this owns it.
    Text(Vec<u8>),
    /// A leaf record.
    Nop(Box<crate::tree::NopNode>),
    /// A tree node.
    Node(Box<crate::tree::ExprNode>),
    /// The parser's static record for a function call while only the syntax is checked: its
    /// kind (10) and the number of arguments.
    Count(i32),
    /// A copy of a pointer that another slot owns, or that has been freed: the original
    /// still holds the old bits here.
    Stale,
}

impl Sem {
    pub fn is_null(&self) -> bool {
        matches!(self, Sem::Null | Sem::Int(0))
    }

    /// The copy the parser pushes when it shifts a token: numbers are copied, text moves to
    /// the stack and the variable keeps the stale pointer.
    fn shift_copy(&mut self) -> Sem {
        match self {
            Sem::Text(_) | Sem::Nop(_) | Sem::Node(_) => std::mem::replace(self, Sem::Stale),
            other => other.clone(),
        }
    }
}

/// The three parts of the original's semantic value (12 bytes): the value itself at
/// 0x0878fe90, a second word at 0x0878fe94 (parameter name of a reference, value id of an
/// enumeration code) and a byte at 0x0878fe98 (`D`, `G` or `L` of a reference).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lval {
    pub a: Sem,
    pub b: Sem,
    pub gl: u8,
}

impl Lval {
    pub(crate) fn shift_copy(&mut self) -> Lval {
        Lval { a: self.a.shift_copy(), b: self.b.shift_copy(), gl: self.gl }
    }
}

/// Where the original scanner has no defined behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexUndefined {
    /// A byte above 127 at the start of a token. The generated scanner is a 7-bit one: no
    /// state has a transition for such a byte, the start state accepts nothing, and the
    /// scanner "backs up" to the position and state the previous token left behind.
    EightBitAtTokenStart(u8),
}

/// The scanner with its buffer and the string being parsed.
#[derive(Clone, Debug, Default)]
pub struct Scanner {
    /// The string being parsed with the two blanks the original appends
    /// (`parse_string_original`).
    src: Vec<u8>,
    /// How much of it the scanner has taken (`parse_string`).
    pos: usize,
    /// Bytes taken from a string but not yet matched: the scanner's buffer between tokens.
    pending: Vec<u8>,
    /// `error_ptr`: an offset into `src`, or null.
    error_ptr: Option<usize>,
    /// `EXP_yylval` and its two neighbours.
    pub lval: Lval,
    /// Text of the token just matched (`EXP_yytext`).
    pub text: Vec<u8>,
}

impl Scanner {
    pub fn new() -> Scanner {
        Scanner::default()
    }

    /// Makes `s` the string to parse [soft 0x08257904 si_setParsedString]: a copy with two
    /// blanks appended. The scanner's buffer is not touched: bytes left over from a parse
    /// that was given up are scanned first.
    pub fn set_parsed_string(&mut self, s: &[u8]) {
        self.src = s.to_vec();
        self.src.extend_from_slice(b"  ");
        self.pos = 0;
    }

    /// Bytes still waiting in the scanner's buffer.
    pub fn pending(&self) -> &[u8] {
        &self.pending
    }

    /// `error_ptr = 0` [soft 0x0825798c si_initErrorPtr].
    pub fn init_error_ptr(&mut self) {
        self.error_ptr = None;
    }

    /// Points the error position at the last place, at or before the scanner's position in
    /// the string, where `text` occurs; null if it occurs nowhere
    /// [soft 0x082579a0 si_setParsedStringErrorPtr]. An empty `text` occurs at the position
    /// itself.
    pub fn set_error_ptr(&mut self, text: &[u8]) {
        let mut p = self.pos as isize;
        loop {
            if p < 0 {
                self.error_ptr = None;
                return;
            }
            // `strncmp` over the length of `text`; the string's terminator ends a comparison
            if self.src[(p as usize).min(self.src.len())..].starts_with(text) {
                self.error_ptr = Some(p as usize);
                return;
            }
            p -= 1;
        }
    }

    /// What the parser's error routine does with the position: it asks for the current one
    /// [soft 0x08257a4c si_getParsedStringErrorPtr] and sets it again with the rest of the
    /// string from there as the text to find. The search starts at the scanner's position
    /// and can only match at that very place, so an existing position is kept and a null
    /// one becomes the scanner's position.
    pub fn keep_error_ptr(&mut self) {
        if self.error_ptr.is_none() {
            self.error_ptr = Some(self.pos);
        }
    }

    /// The error position, or the scanner's position if there is none
    /// [soft 0x08257a4c si_getParsedStringErrorPtr].
    pub fn error_ptr(&self) -> usize {
        self.error_ptr.unwrap_or(self.pos)
    }

    /// The byte at an offset of the string; 0 at and after its end.
    pub fn byte_at(&self, at: usize) -> u8 {
        self.src.get(at).copied().unwrap_or(0)
    }

    /// One more byte from the string into the buffer; false at the end of the string.
    fn fill(&mut self) -> bool {
        match self.src.get(self.pos) {
            Some(&c) => {
                self.pos += 1;
                self.pending.push(c);
                true
            }
            None => false,
        }
    }

    /// The next token [soft 0x08256a20 EXP_yylex]: its number (0 at the end of the input),
    /// with [`Scanner::lval`] set as the rule's action sets it.
    pub fn yylex(&mut self, dict: &dyn Dict) -> Result<i32, LexUndefined> {
        loop {
            // longest match over the buffer, taking bytes from the string as needed
            let (mut state, mut i) = (0usize, 0usize);
            let mut last: Option<(usize, usize)> = None;
            loop {
                if i == self.pending.len() && !self.fill() {
                    break;
                }
                let c = self.pending[i];
                if c >= 128 || c == 0 {
                    break;
                }
                let next = NEXT[state][CLASS[c as usize] as usize];
                if next < 0 {
                    break;
                }
                state = next as usize;
                i += 1;
                if ACCEPT[state] != 0 {
                    last = Some((state, i));
                }
            }
            if i == 0 {
                if self.pending.is_empty() {
                    // end of the string: the scanner restarts its buffer and returns 0
                    return Ok(0);
                }
                // Every 7-bit byte leaves the start state, so this is a byte above 127
                // (a NUL cannot be inside a C string).
                return Err(LexUndefined::EightBitAtTokenStart(self.pending[0]));
            }
            // every state entered from the start state accepts something, so `last` is set
            let (state, len) = last.expect("a state after the start state accepts");
            self.text = self.pending.drain(..len).collect();
            match ACCEPT[state] {
                1 => {
                    self.lval.a = Sem::Int(-1);
                    self.lval.b = Sem::Int(-1);
                    return Ok(tok::ENUMVAL);
                }
                2 => return Ok(tok::LIKE),
                3 => return Ok(self.number_or_reference()),
                4 => return Ok(self.word(dict)),
                5 => {
                    // the quotes are dropped
                    let inner = self.text[1..self.text.len() - 1].to_vec();
                    self.lval.a = Sem::Text(inner);
                    return Ok(tok::STRING);
                }
                6 => {
                    // `atof`, stored to a float
                    self.lval.a = Sem::Float(cnum::atof(&self.text) as f32);
                    return Ok(tok::FLOAT);
                }
                7 => {
                    self.lval.a = Sem::Int(cnum::atoi(&self.text));
                    return Ok(tok::INT);
                }
                8 => return Ok(tok::EQ),
                9 => return Ok(tok::NE),
                10 => return Ok(tok::LT),
                11 => return Ok(tok::LE),
                12 => return Ok(tok::GT),
                13 => return Ok(tok::GE),
                14 => return Ok(tok::AND),
                15 => return Ok(tok::OR),
                16 => return Ok(tok::NOT),
                17 => return Ok(tok::ADDASSIGN),
                18 => return Ok(tok::SUBASSIGN),
                19 => return Ok(tok::MULASSIGN),
                20 => return Ok(tok::DIVASSIGN),
                23 => continue,
                // rules 21, 22, 24, 25: the character itself
                _ => return Ok(self.text[0] as i32),
            }
        }
    }

    /// Rule 3: text with a point in it. If `sscanf("%f%n")` converts it and uses all of it,
    /// it is a real number. Otherwise it is a reference: the part after the last point is
    /// the parameter, and if what is left ends in `.G`, `.g`, `.L` or `.l` that letter (in
    /// upper case) is the global/local mark and is cut off too; the rest is the element.
    fn number_or_reference(&mut self) -> i32 {
        if let Some((v, n)) = scan_float_n(&self.text) {
            if n == self.text.len() {
                self.lval.a = Sem::Float(v);
                return tok::FLOAT;
            }
        }
        let mut text = self.text.clone();
        let dot = text.iter().rposition(|&c| c == b'.').expect("rule 3 has a point");
        self.lval.b = Sem::Text(text[dot + 1..].to_vec());
        text.truncate(dot);
        self.lval.gl = b'D';
        if let Some(dot) = text.iter().rposition(|&c| c == b'.') {
            if text.len() == dot + 2 && matches!(text[dot + 1], b'G' | b'g' | b'L' | b'l') {
                self.lval.gl = text[dot + 1].to_ascii_uppercase();
                text.truncate(dot);
            }
        }
        self.lval.a = Sem::Text(text);
        tok::ELEMPARAM
    }

    /// Rule 4: a word. In this order: a predefined word; the code of a value of any
    /// enumeration; an element type word; the name of a relation; else an identifier.
    fn word(&mut self, dict: &dyn Dict) -> i32 {
        let t = token::get_predef_tok(&self.text);
        if t >= 0 {
            self.lval.a = Sem::Int(t);
            return t;
        }
        let mut enum_type = -1;
        if let Some(id) = dict.enum_val_by_any_code(&mut enum_type, &self.text) {
            self.lval.a = Sem::Int(enum_type);
            self.lval.b = Sem::Int(id);
            return tok::ENUMVAL;
        }
        let t = token::get_elem_type_tok(&self.text);
        if t >= 0 {
            self.lval.a = Sem::Int(t);
            return t;
        }
        let id = dict.rel_id_by_name(&self.text);
        if id != -1 {
            self.lval.a = Sem::Int(id);
            return tok::RELNAME;
        }
        self.lval.a = Sem::Text(self.text.clone());
        tok::IDENT
    }
}

/// `sscanf(text, "%f%n", &value, &count)` for a text without blanks: the value and the
/// count if the conversion happened.
///
/// This is the C library's doing (the binary imports `sscanf`), so it is not read from the
/// binary: it follows the float scanning of glibc 2.2 as its sources describe it. The
/// scanner collects an optional sign, then digits with at most one point and at most one
/// exponent letter (which must follow something, and may be followed by one sign), or `nan`,
/// `inf`, `infinity`, or `0x` with hexadecimal digits and a `p` exponent; it stops at the
/// first byte that does not fit, without giving back more than that one byte. The collected
/// text then goes through `strtof`, which may use less of it (`1.5e` converts as 1.5), and
/// the count is what the scanner collected.
pub fn scan_float_n(text: &[u8]) -> Option<(f32, usize)> {
    let mut i = 0;
    let mut neg = false;
    let at = |i: usize| text.get(i).copied();
    if matches!(at(i), Some(b'-' | b'+')) {
        neg = text[i] == b'-';
        i += 1;
        at(i)?;
    }
    let start = i;
    let lower = |i: usize| at(i).map(|c| c.to_ascii_lowercase());
    if lower(i) == Some(b'n') {
        if lower(i + 1) == Some(b'a') && lower(i + 2) == Some(b'n') {
            let v = f32::NAN;
            return Some((if neg { -v } else { v }, i + 3));
        }
        return None;
    }
    if lower(i) == Some(b'i') {
        if lower(i + 1) == Some(b'n') && lower(i + 2) == Some(b'f') {
            let mut n = i + 3;
            if lower(n) == Some(b'i') {
                if text.len() >= n + 5 && text[n..n + 5].eq_ignore_ascii_case(b"inity") {
                    n += 5;
                } else {
                    return None;
                }
            }
            let v = f32::INFINITY;
            return Some((if neg { -v } else { v }, n));
        }
        return None;
    }
    let mut hexa = false;
    let mut exp_char = b'e';
    let mut wp: Vec<u8> = Vec::new();
    if at(i) == Some(b'0') {
        wp.push(b'0');
        i += 1;
        if lower(i) == Some(b'x') {
            wp.push(b'x');
            hexa = true;
            exp_char = b'p';
            i += 1;
        }
    }
    let (mut got_dot, mut got_e) = (false, false);
    while let Some(c) = at(i) {
        if c.is_ascii_digit() {
            wp.push(c);
        } else if !got_e && hexa && c.is_ascii_hexdigit() {
            wp.push(c);
        } else if got_e && wp.last() == Some(&exp_char) && (c == b'-' || c == b'+') {
            wp.push(c);
        } else if !wp.is_empty() && !got_e && c.to_ascii_lowercase() == exp_char {
            wp.push(exp_char);
            got_e = true;
            got_dot = true;
        } else if c == b'.' && !got_dot {
            wp.push(c);
            got_dot = true;
        } else {
            break;
        }
        i += 1;
    }
    if wp.is_empty() || (hexa && (wp.len() == 2 || !got_e)) {
        return None;
    }
    let _ = start;
    let v = strtof_prefix(&wp, hexa)?;
    Some((if neg { -v } else { v }, i))
}

/// `strtof` on the text the scanner collected: the longest prefix that is a number, rounded
/// once to a float; `None` if no prefix is one.
fn strtof_prefix(wp: &[u8], hexa: bool) -> Option<f32> {
    if hexa {
        // 0x<hex digits>[.<hex digits>]p[sign]<decimal digits>; the scanner made sure of
        // the exponent letter, but digits may be missing on either side
        let body = &wp[2..];
        let p = body.iter().position(|&c| c == b'p')?;
        let (mant, exp) = (&body[..p], &body[p + 1..]);
        let mut value = 0f64;
        let mut scale = 0i32;
        let mut seen_dot = false;
        let mut digits = 0;
        for &c in mant {
            if c == b'.' {
                seen_dot = true;
                continue;
            }
            // more than 13 hexadecimal digits would need exact arithmetic to round once;
            // such a spelling is not expected in an expression
            value = value * 16.0 + (c as char).to_digit(16)? as f64;
            digits += 1;
            if seen_dot {
                scale -= 4;
            }
        }
        if digits == 0 {
            return None;
        }
        let e: i32 = std::str::from_utf8(exp).ok().and_then(|s| s.parse().ok()).unwrap_or(0);
        return Some((value * 2f64.powi(scale.saturating_add(e))) as f32);
    }
    // decimal: try the whole text, then without a dangling exponent
    let mut end = wp.len();
    while end > 0 {
        if let Ok(s) = std::str::from_utf8(&wp[..end]) {
            let digits = s.bytes().any(|c| c.is_ascii_digit());
            let tail_ok = !matches!(s.as_bytes()[end - 1], b'e' | b'+' | b'-');
            if digits && tail_ok {
                if let Ok(v) = s.parse::<f32>() {
                    return Some(v);
                }
            }
        }
        end -= 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_float_counts() {
        assert_eq!(scan_float_n(b"1.5"), Some((1.5, 3)));
        assert_eq!(scan_float_n(b"1."), Some((1.0, 2)));
        assert_eq!(scan_float_n(b"1.e2"), Some((100.0, 4)));
        // a dangling exponent letter is collected and counted, but not converted
        assert_eq!(scan_float_n(b"1.5e"), Some((1.5, 4)));
        // a second point ends the number
        assert_eq!(scan_float_n(b"1.5.x"), Some((1.5, 3)));
        assert_eq!(scan_float_n(b"arg1.etrnx"), None);
        assert_eq!(scan_float_n(b"inf.x").map(|r| r.1), Some(3));
        assert_eq!(scan_float_n(b"."), None);
        assert_eq!(scan_float_n(b"0x1.8"), None);
        assert_eq!(scan_float_n(b"0x1.8p1"), Some((3.0, 7)));
    }
}
