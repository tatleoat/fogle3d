// SPDX-License-Identifier: MIT
//! The scanner's run-time of `exp_lex.c` on the image: the buffer routines every flex 2.5
//! scanner carries (finding 0013, section 13).
//!
//! The generated scanner [soft 0x08256a20 EXP_yylex] has its own port in value form
//! ([`crate::lex`]); this is the library part around it, written on the image because its state
//! (the buffer record, `yy_c_buf_p`, `EXP_yytext`, ...) is global data of the original. The
//! buffer record (0x28 bytes): +0 file, +4 character buffer, +8 position, +0xc size, +0x10
//! characters, +0x14 owns the buffer, +0x18 interactive, +0x1c at line start, +0x20 fill
//! buffer, +0x24 status.
//!
//! The character source is not a file: `yy_get_next_buffer` takes one byte per call from
//! `parse_string` (the string given to `si_setParsedString`). `yy_fatal_error` ends the
//! program (`exit(2)`); here the caller carries on after the exit routine answers, which the
//! original never does (a check skips such a case: the original stopped).

use si_mem::{Mem, Outside, Routine};

pub const YY_CURRENT_BUFFER: u32 = 0x0869_ba40;
pub const YY_C_BUF_P: u32 = 0x0869_ba44;
pub const YY_START: u32 = 0x0869_ba4c;
pub const EXP_YYIN: u32 = 0x0869_ba50;
pub const YY_START_STATE_LIST: u32 = 0x0869_ba58;
pub const YY_HOLD_CHAR: u32 = 0x0878_fea4;
pub const YY_N_CHARS: u32 = 0x0878_fea8;
pub const YY_DID_BUFFER_SWITCH_ON_EOF: u32 = 0x0878_feac;
pub const EXP_YYTEXT: u32 = 0x0878_feb0;
pub const YY_LAST_ACCEPTING_STATE: u32 = 0x0878_feb4;
pub const YY_LAST_ACCEPTING_CPOS: u32 = 0x0878_feb8;
pub const PARSE_STRING: u32 = 0x0878_febc;

/// `exit`, the C library's.
pub const EXIT: Routine = Routine::new("exit", 0x080c_4b58);

/// The messages of the scanner's fatal errors, at their addresses (data of the binary).
pub const MESSAGES: &[(u32, &[u8])] = &[
    (0x0869_bac0, b"fatal flex scanner internal error--end of buffer missed"),
    (0x0869_bb00, b"fatal error - scanner input buffer overflow"),
    (0x0869_bb40, b"flex scanner push-back overflow"),
    (0x0869_bb60, b"out of dynamic memory in yy_create_buffer()"),
    (0x0869_bba0, b"out of dynamic memory in yy_create_buffer()"),
    (0x0869_bbe0, b"out of dynamic memory in yy_scan_buffer()"),
    (0x0869_bc20, b"out of dynamic memory in yy_scan_bytes()"),
    (0x0869_bc49, b"bad buffer in yy_scan_bytes()"),
];
const S_EOB_MISSED: u32 = 0x0869_bac0;
const S_BUFFER_OVERFLOW: u32 = 0x0869_bb00;
const S_PUSH_BACK: u32 = 0x0869_bb40;
const S_CREATE_1: u32 = 0x0869_bb60;
const S_CREATE_2: u32 = 0x0869_bba0;
const S_SCAN_BUFFER: u32 = 0x0869_bbe0;
const S_SCAN_BYTES: u32 = 0x0869_bc20;
const S_BAD_SCAN_BYTES: u32 = 0x0869_bc49;

/// Puts the messages into the image (read-only data).
pub fn store_messages(m: &mut Mem) {
    for (a, s) in MESSAGES {
        m.put_rom_cstr(*a, s);
    }
}

const B_FILE: u32 = 0;
const B_BUF: u32 = 4;
const B_POS: u32 = 8;
const B_SIZE: u32 = 0xc;
const B_NCHARS: u32 = 0x10;
const B_OWN: u32 = 0x14;
const B_INTERACTIVE: u32 = 0x18;
const B_AT_BOL: u32 = 0x1c;
const B_FILL: u32 = 0x20;
const B_STATUS: u32 = 0x24;

/// [soft 0x08257890 yy_fatal_error]: `fprintf(stderr, "%s\n", msg)` and `exit(2)`.
pub fn yy_fatal_error(m: &mut Mem, o: &mut dyn Outside, msg: u32) {
    let mut text = m.cstr(msg);
    text.push(b'\n');
    o.print(2, &text);
    o.call(m, EXIT, &[2]);
}

/// [soft 0x082578b8 yy_flex_alloc]: `malloc`.
pub fn yy_flex_alloc(m: &mut Mem, size: u32) -> u32 {
    m.malloc(size)
}

/// [soft 0x082578d0 yy_flex_realloc]: `realloc`.
pub fn yy_flex_realloc(m: &mut Mem, p: u32, size: u32) -> u32 {
    m.realloc(p, size)
}

/// [soft 0x082578ec yy_flex_free]: `free`.
pub fn yy_flex_free(m: &mut Mem, p: u32) {
    m.free(p)
}

/// [soft 0x082575d4 EXP_yy_load_buffer_state].
pub fn exp_yy_load_buffer_state(m: &mut Mem) {
    let b = m.u32(YY_CURRENT_BUFFER);
    let n = m.u32(b + B_NCHARS);
    let pos = m.u32(b + B_POS);
    m.set_u32(YY_N_CHARS, n);
    let file = m.u32(b + B_FILE);
    m.set_u32(YY_C_BUF_P, pos);
    m.set_u32(EXP_YYTEXT, pos);
    m.set_u32(EXP_YYIN, file);
    let c = m.u8(pos);
    m.set_u8(YY_HOLD_CHAR, c);
}

/// [soft 0x082576f8 EXP_yy_flush_buffer].
pub fn exp_yy_flush_buffer(m: &mut Mem, b: u32) {
    if b == 0 {
        return;
    }
    let buf = m.u32(b + B_BUF);
    m.set_u32(b + B_NCHARS, 0);
    m.set_u8(buf, 0);
    let buf = m.u32(b + B_BUF);
    m.set_u8(buf + 1, 0);
    let buf = m.u32(b + B_BUF);
    m.set_u32(b + B_POS, buf);
    m.set_u32(b + B_AT_BOL, 1);
    m.set_u32(b + B_STATUS, 0);
    if b == m.u32(YY_CURRENT_BUFFER) {
        exp_yy_load_buffer_state(m);
    }
}

/// [soft 0x082576c8 EXP_yy_init_buffer].
pub fn exp_yy_init_buffer(m: &mut Mem, b: u32, file: u32) {
    exp_yy_flush_buffer(m, b);
    m.set_u32(b + B_FILE, file);
    m.set_u32(b + B_FILL, 1);
    m.set_u32(b + B_INTERACTIVE, 0);
}

/// [soft 0x08257608 EXP_yy_create_buffer].
pub fn exp_yy_create_buffer(m: &mut Mem, o: &mut dyn Outside, file: u32, size: i32) -> u32 {
    let b = yy_flex_alloc(m, 0x28);
    if b == 0 {
        yy_fatal_error(m, o, S_CREATE_1);
    }
    m.set_u32(b + B_SIZE, size as u32);
    let buf = yy_flex_alloc(m, (size as u32).wrapping_add(2));
    m.set_u32(b + B_BUF, buf);
    if buf == 0 {
        yy_fatal_error(m, o, S_CREATE_2);
    }
    m.set_u32(b + B_OWN, 1);
    exp_yy_init_buffer(m, b, file);
    b
}

/// [soft 0x08257680 EXP_yy_delete_buffer].
pub fn exp_yy_delete_buffer(m: &mut Mem, b: u32) {
    if b == 0 {
        return;
    }
    if b == m.u32(YY_CURRENT_BUFFER) {
        m.set_u32(YY_CURRENT_BUFFER, 0);
    }
    if m.u32(b + B_OWN) != 0 {
        let buf = m.u32(b + B_BUF);
        yy_flex_free(m, buf);
    }
    yy_flex_free(m, b);
}

/// [soft 0x08257538 EXP_yyrestart].
pub fn exp_yyrestart(m: &mut Mem, o: &mut dyn Outside, file: u32) {
    if m.u32(YY_CURRENT_BUFFER) == 0 {
        let yyin = m.u32(EXP_YYIN);
        let b = exp_yy_create_buffer(m, o, yyin, 0x4000);
        m.set_u32(YY_CURRENT_BUFFER, b);
    }
    let cur = m.u32(YY_CURRENT_BUFFER);
    exp_yy_init_buffer(m, cur, file);
    exp_yy_load_buffer_state(m);
}

/// [soft 0x08257580 EXP_yy_switch_to_buffer].
pub fn exp_yy_switch_to_buffer(m: &mut Mem, new: u32) {
    let cur = m.u32(YY_CURRENT_BUFFER);
    if cur == new {
        return;
    }
    if cur != 0 {
        let p = m.u32(YY_C_BUF_P);
        let h = m.u8(YY_HOLD_CHAR);
        m.set_u8(p, h);
        let cur = m.u32(YY_CURRENT_BUFFER);
        let p = m.u32(YY_C_BUF_P);
        m.set_u32(cur + B_POS, p);
        let n = m.u32(YY_N_CHARS);
        m.set_u32(cur + B_NCHARS, n);
    }
    m.set_u32(YY_CURRENT_BUFFER, new);
    exp_yy_load_buffer_state(m);
    m.set_u32(YY_DID_BUFFER_SWITCH_ON_EOF, 1);
}

/// [soft 0x08257740 EXP_yy_scan_buffer]: a buffer over `base[0..size]` (two NULs at the end).
pub fn exp_yy_scan_buffer(m: &mut Mem, o: &mut dyn Outside, base: u32, size: u32) -> u32 {
    if size < 2 || m.u8(base.wrapping_add(size).wrapping_sub(2)) != 0 || m.u8(base.wrapping_add(size).wrapping_sub(1)) != 0 {
        return 0;
    }
    let b = yy_flex_alloc(m, 0x28);
    if b == 0 {
        yy_fatal_error(m, o, S_SCAN_BUFFER);
    }
    m.set_u32(b + B_SIZE, size.wrapping_sub(2));
    m.set_u32(b + B_BUF, base);
    m.set_u32(b + B_POS, base);
    m.set_u32(b + B_OWN, 0);
    m.set_u32(b + B_FILE, 0);
    m.set_u32(b + B_NCHARS, size.wrapping_sub(2));
    m.set_u32(b + B_INTERACTIVE, 0);
    m.set_u32(b + B_AT_BOL, 1);
    m.set_u32(b + B_FILL, 0);
    m.set_u32(b + B_STATUS, 0);
    exp_yy_switch_to_buffer(m, b);
    b
}

/// [soft 0x08257804 EXP_yy_scan_bytes].
pub fn exp_yy_scan_bytes(m: &mut Mem, o: &mut dyn Outside, bytes: u32, len: i32) -> u32 {
    let n = (len as u32).wrapping_add(2);
    let buf = yy_flex_alloc(m, n);
    if buf == 0 {
        yy_fatal_error(m, o, S_SCAN_BYTES);
    }
    for i in 0..len.max(0) as u32 {
        let c = m.u8(bytes.wrapping_add(i));
        m.set_u8(buf.wrapping_add(i), c);
    }
    m.set_u8(buf.wrapping_add(len as u32).wrapping_add(1), 0);
    m.set_u8(buf.wrapping_add(len as u32), 0);
    let b = exp_yy_scan_buffer(m, o, buf, n);
    if b == 0 {
        yy_fatal_error(m, o, S_BAD_SCAN_BYTES);
    }
    m.set_u32(b + B_OWN, 1);
    b
}

/// [soft 0x082577d8 EXP_yy_scan_string]: the buffer of `exp_yy_scan_bytes` over the string
/// (the original's wrapper leaves that function's answer in the register).
pub fn exp_yy_scan_string(m: &mut Mem, o: &mut dyn Outside, s: u32) -> u32 {
    let n = si_mem::clib::strlen(m, s);
    exp_yy_scan_bytes(m, o, s, n as i32)
}

/// [soft 0x08257330 yy_get_previous_state]: the state after the text from `yytext` to
/// `yy_c_buf_p`, the characters taken as unsigned (the scanner proper takes them signed).
pub fn yy_get_previous_state(m: &mut Mem) -> u32 {
    let start = m.u32(YY_START);
    let mut state = m.u32(YY_START_STATE_LIST.wrapping_add(start.wrapping_mul(4)));
    let end = m.u32(YY_C_BUF_P);
    let mut cp = m.u32(EXP_YYTEXT);
    while cp < end {
        let c = m.u8(cp);
        let entry = if c != 0 { state.wrapping_add(c as u32 * 4) } else { state.wrapping_add(0x200) };
        let nxt = m.i16(entry.wrapping_add(2)) as i32;
        state = state.wrapping_add((nxt as u32).wrapping_mul(4));
        if m.u16(state.wrapping_sub(2)) != 0 {
            m.set_u32(YY_LAST_ACCEPTING_STATE, state);
            m.set_u32(YY_LAST_ACCEPTING_CPOS, cp);
        }
        cp = cp.wrapping_add(1);
    }
    state
}

/// [soft 0x0825738c yy_try_NUL_trans]: the state after a NUL when the state's NUL entry
/// is marked 0x80 (it is then also recorded as the last accepting state if it accepts),
/// else 0.
pub fn yy_try_nul_trans(m: &mut Mem, state: u32) -> u32 {
    let cp = m.u32(YY_C_BUF_P);
    let marked = m.u16(state.wrapping_add(0x200)) == 0x80;
    let nxt = m.i16(state.wrapping_add(0x202)) as i32;
    let next = state.wrapping_add((nxt as u32).wrapping_mul(4));
    if marked && m.u16(next.wrapping_sub(2)) != 0 {
        m.set_u32(YY_LAST_ACCEPTING_STATE, next);
        m.set_u32(YY_LAST_ACCEPTING_CPOS, cp);
    }
    if marked {
        next
    } else {
        0
    }
}

/// [soft 0x082573d8 yyunput]: puts character `c` back in front of `yy_c_buf_p`; `text` is
/// the new `yytext` (moved with the buffer's contents when they are shifted up).
pub fn yyunput(m: &mut Mem, o: &mut dyn Outside, c: i32, mut text: u32) {
    let mut cp = m.u32(YY_C_BUF_P);
    let h = m.u8(YY_HOLD_CHAR);
    m.set_u8(cp, h);
    let cb = m.u32(YY_CURRENT_BUFFER);
    let buf = m.u32(cb + B_BUF);
    if cp < buf.wrapping_add(2) {
        let mut dst = buf.wrapping_add(m.u32(cb + B_SIZE)).wrapping_add(2);
        let mut src = buf.wrapping_add(m.u32(YY_N_CHARS)).wrapping_add(2);
        if buf < src {
            loop {
                dst = dst.wrapping_sub(1);
                src = src.wrapping_sub(1);
                let v = m.u8(src);
                m.set_u8(dst, v);
                // the loop test reads the buffer's start again
                if m.u32(m.u32(YY_CURRENT_BUFFER) + B_BUF) >= src {
                    break;
                }
            }
        }
        let d = dst.wrapping_sub(src);
        cp = cp.wrapping_add(d);
        text = text.wrapping_add(d);
        let cb = m.u32(YY_CURRENT_BUFFER);
        let size = m.u32(cb + B_SIZE);
        m.set_u32(YY_N_CHARS, size);
        m.set_u32(cb + B_NCHARS, size);
        if cp < m.u32(cb + B_BUF).wrapping_add(2) {
            yy_fatal_error(m, o, S_PUSH_BACK);
        }
    }
    cp = cp.wrapping_sub(1);
    m.set_u32(YY_C_BUF_P, cp);
    m.set_u8(cp, c as u8);
    m.set_u32(EXP_YYTEXT, text);
    let h = m.u8(cp);
    m.set_u8(YY_HOLD_CHAR, h);
}

/// [soft 0x08257140 yy_get_next_buffer]: refills the buffer, one byte of `parse_string` per
/// call. Answers 0 (continue), 1 (end of input), 2 (last match).
pub fn yy_get_next_buffer(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let mut source = m.u32(EXP_YYTEXT);
    let cb = m.u32(YY_CURRENT_BUFFER);
    let mut dest = m.u32(cb + B_BUF);
    let cp = m.u32(YY_C_BUF_P);
    if m.u32(YY_N_CHARS).wrapping_add(1).wrapping_add(dest) < cp {
        yy_fatal_error(m, o, S_EOB_MISSED);
    }
    if m.u32(m.u32(YY_CURRENT_BUFFER) + B_FILL) == 0 {
        let yytext = m.u32(EXP_YYTEXT);
        return if yytext == m.u32(YY_C_BUF_P).wrapping_sub(1) { 1 } else { 2 };
    }
    let to_move = m.u32(YY_C_BUF_P).wrapping_sub(m.u32(EXP_YYTEXT)).wrapping_sub(1) as i32;
    for _ in 0..to_move.max(0) {
        let c = m.u8(source);
        m.set_u8(dest, c);
        source = source.wrapping_add(1);
        dest = dest.wrapping_add(1);
    }
    let cb = m.u32(YY_CURRENT_BUFFER);
    if m.u32(cb + B_STATUS) == 2 {
        m.set_u32(YY_N_CHARS, 0);
        let cb = m.u32(YY_CURRENT_BUFFER);
        m.set_u32(cb + B_NCHARS, 0);
    } else {
        loop {
            let cb = m.u32(YY_CURRENT_BUFFER);
            let room = (m.u32(cb + B_SIZE) as i32).wrapping_sub(to_move);
            if room > 1 {
                break;
            }
            let cp = m.u32(YY_C_BUF_P);
            let old = m.u32(cb + B_BUF);
            if m.u32(cb + B_OWN) == 0 {
                m.set_u32(cb + B_BUF, 0);
            } else {
                let size = m.u32(cb + B_SIZE);
                let grown = if (size.wrapping_mul(2) as i32) < 1 { size.wrapping_add(size >> 3) } else { size.wrapping_mul(2) };
                m.set_u32(cb + B_SIZE, grown);
                let nb = yy_flex_realloc(m, m.u32(cb + B_BUF), m.u32(cb + B_SIZE).wrapping_add(2));
                m.set_u32(cb + B_BUF, nb);
            }
            let nb = m.u32(cb + B_BUF);
            if nb == 0 {
                yy_fatal_error(m, o, S_BUFFER_OVERFLOW);
            }
            m.set_u32(YY_C_BUF_P, cp.wrapping_add(nb.wrapping_sub(old)));
        }
        // the custom input: one byte of parse_string
        let text = m.u32(PARSE_STRING);
        let c = m.u8(text);
        if c != 0 {
            m.set_u32(PARSE_STRING, text.wrapping_add(1));
            let cb = m.u32(YY_CURRENT_BUFFER);
            let buf = m.u32(cb + B_BUF);
            m.set_u8(buf.wrapping_add(to_move as u32), c);
        }
        let got = (c != 0) as u32;
        m.set_u32(YY_N_CHARS, got);
        let cb = m.u32(YY_CURRENT_BUFFER);
        m.set_u32(cb + B_NCHARS, got);
    }
    let ret;
    if m.u32(YY_N_CHARS) == 0 {
        if to_move == 0 {
            ret = 1;
            let yyin = m.u32(EXP_YYIN);
            exp_yyrestart(m, o, yyin);
        } else {
            ret = 2;
            let cb = m.u32(YY_CURRENT_BUFFER);
            m.set_u32(cb + B_STATUS, 2);
        }
    } else {
        ret = 0;
    }
    let n = (to_move as u32).wrapping_add(m.u32(YY_N_CHARS));
    m.set_u32(YY_N_CHARS, n);
    let buf = m.u32(m.u32(YY_CURRENT_BUFFER) + B_BUF);
    m.set_u8(buf.wrapping_add(n), 0);
    m.set_u8(buf.wrapping_add(n).wrapping_add(1), 0);
    m.set_u32(EXP_YYTEXT, buf);
    ret
}

/// [soft 0x08257488 yyinput]: the next character (-1 at the end).
pub fn yyinput(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let cp = m.u32(YY_C_BUF_P);
    let h = m.u8(YY_HOLD_CHAR);
    m.set_u8(cp, h);
    let cp = m.u32(YY_C_BUF_P);
    if m.u8(cp) == 0 {
        let cb = m.u32(YY_CURRENT_BUFFER);
        let limit = m.u32(YY_N_CHARS).wrapping_add(m.u32(cb + B_BUF));
        if cp < limit {
            m.set_u8(cp, 0);
        } else {
            let off = cp.wrapping_sub(m.u32(EXP_YYTEXT));
            m.set_u32(YY_C_BUF_P, cp.wrapping_add(1));
            match yy_get_next_buffer(m, o) {
                1 => return -1,
                0 => m.set_u32(YY_C_BUF_P, off.wrapping_add(m.u32(EXP_YYTEXT))),
                2 => {
                    let yyin = m.u32(EXP_YYIN);
                    exp_yyrestart(m, o, yyin);
                    return -1;
                }
                _ => {}
            }
        }
    }
    let p = m.u32(YY_C_BUF_P);
    let c = m.u8(p);
    m.set_u8(p, 0);
    let p = m.u32(YY_C_BUF_P);
    m.set_u32(YY_C_BUF_P, p.wrapping_add(1));
    let h = m.u8(p.wrapping_add(1));
    m.set_u8(YY_HOLD_CHAR, h);
    c as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_mem::Mem;

    struct Quiet(Vec<u8>);
    impl Outside for Quiet {
        fn call(&mut self, _m: &mut Mem, _r: Routine, _a: &[u32]) -> si_mem::Ret {
            si_mem::Ret::NONE
        }
        fn print(&mut self, _s: u32, t: &[u8]) {
            self.0.extend_from_slice(t);
        }
    }

    /// A scan of "ab" through the buffer routines: the scanner takes the bytes one by one
    /// through `yy_get_next_buffer`, and the end of the input answers 1.
    #[test]
    fn scan_string_and_refill() {
        let mut m = Mem::program();
        let mut o = Quiet(Vec::new());
        let s = m.place_cstr(b"ab");
        let b = exp_yy_scan_string(&mut m, &mut o, s);
        assert_ne!(b, 0);
        assert_eq!(m.u32(YY_CURRENT_BUFFER), b);
        // the buffer is not ours to refill (scan_buffer sets fill 0): end of input at once
        assert_eq!(m.u32(b + 0x20), 0);
        assert_eq!(yyinput(&mut m, &mut o), b'a' as i32);
        assert_eq!(yyinput(&mut m, &mut o), b'b' as i32);
        assert_eq!(yyinput(&mut m, &mut o), -1);
        // refilling from parse_string: a buffer made by create_buffer
        let nb = exp_yy_create_buffer(&mut m, &mut o, 0, 4);
        exp_yy_switch_to_buffer(&mut m, nb);
        let ps = m.place_cstr(b"xyz");
        m.set_u32(PARSE_STRING, ps);
        assert_eq!(yyinput(&mut m, &mut o), b'x' as i32);
        assert_eq!(m.u32(PARSE_STRING), ps + 1);
    }

    #[test]
    fn try_nul_trans_needs_the_mark() {
        let mut m = Mem::program();
        let t = m.place(0x300, 4);
        m.set_u16(t + 0x200, 0x80);
        m.set_i16(t + 0x202, 4);
        m.set_u16(t + 16 - 2, 7);
        m.set_u32(YY_C_BUF_P, 0x1234);
        assert_eq!(yy_try_nul_trans(&mut m, t), t + 16);
        assert_eq!(m.u32(YY_LAST_ACCEPTING_STATE), t + 16);
        assert_eq!(m.u32(YY_LAST_ACCEPTING_CPOS), 0x1234);
        m.set_u16(t + 0x200, 0);
        assert_eq!(yy_try_nul_trans(&mut m, t), 0);
    }
}
