// SPDX-License-Identifier: MIT
//! `shdb_lex.c`: the scanner (flex 2.5 with a table of 4-byte transitions), its buffer
//! routines, the custom character source that reads the text in `parsestring`, and the error
//! collection (`SHDB_yyerror`, `si_shdbErrorToken`).
//!
//! The buffer record (0x28 bytes): +0 file, +4 character buffer, +8 position, +0xc size,
//! +0x10 characters, +0x14 owns the buffer, +0x18 interactive, +0x1c at line start, +0x20
//! fill buffer, +0x24 status. `yy_fatal_error` ends the program (`exit(2)`); in the Rust the
//! callers carry on after the exit routine answers, which the original never does.

use crate::cfmt::{self, strtod};
use crate::{fprintf, fwrite_one, EXIT, STDERR, STDIN, STDOUT};
use si_mem::{clib, Mem, Outside};

pub const YY_CURRENT_BUFFER: u32 = 0x086e_5b00;
pub const YY_C_BUF_P: u32 = 0x086e_5b04;
pub const YY_INIT: u32 = 0x086e_5b08;
pub const YY_START: u32 = 0x086e_5b0c;
pub const SHDB_YYIN: u32 = 0x086e_5b10;
pub const SHDB_YYOUT: u32 = 0x086e_5b14;
pub const YY_START_STATE_LIST: u32 = 0x086e_5b18;
pub const BERROR: u32 = 0x086e_5b24;
pub const STR_ERROR_V: u32 = 0x086e_5b28;
pub const STR_ERROR_C: u32 = 0x086e_5b2c;
pub const PARSESTRING: u32 = 0x086e_5b30;
pub const CNT: u32 = 0x086e_5b34;
pub const STR_UNKNOWN_ERR: u32 = 0x086e_5b50;
pub const SHDB_YYLENG: u32 = 0x0882_487c;
pub const YY_HOLD_CHAR: u32 = 0x0882_4880;
pub const YY_N_CHARS: u32 = 0x0882_4884;
pub const YY_DID_BUFFER_SWITCH_ON_EOF: u32 = 0x0882_4888;
pub const SHDB_YYTEXT: u32 = 0x0882_488c;
pub const YY_LAST_ACCEPTING_STATE: u32 = 0x0882_4890;
pub const YY_LAST_ACCEPTING_CPOS: u32 = 0x0882_4894;
pub const LINENO: u32 = 0x0882_4898;
pub const SHDB_YYLVAL: u32 = 0x0882_4868;

const S_FATAL_FORMAT: u32 = 0x086e_5d47;
const S_INTERNAL_ERROR: u32 = 0x086e_5b60;
const S_EOB_MISSED: u32 = 0x086e_5ba0;
const S_BUFFER_OVERFLOW: u32 = 0x086e_5be0;
const S_PUSH_BACK: u32 = 0x086e_5c20;
const S_NO_MEMORY_CREATE: u32 = 0x086e_5c40;
const S_NO_MEMORY_CREATE_BUF: u32 = 0x086e_5c80;
const S_NO_MEMORY_SCAN_BUFFER: u32 = 0x086e_5cc0;
const S_NO_MEMORY_SCAN_BYTES: u32 = 0x086e_5d00;
const S_BAD_SCAN_BYTES: u32 = 0x086e_5d29;
const S_ERROR_AT_LINE: u32 = 0x086e_5d60;
const S_UNRECOGNIZED: u32 = 0x086e_5d8b;

/// `fwrite`.

// the buffer record
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

/// yy_fatal_error (0x085517f0): the message and `exit(2)`.
pub fn yy_fatal_error(m: &mut Mem, o: &mut dyn Outside, msg: u32) {
    let err = m.u32(STDERR);
    fprintf(m, o, err, S_FATAL_FORMAT, &[msg]);
    o.call(m, EXIT, &[2]);
}

/// [soft 0x08551818 yy_flex_alloc]: `malloc`.
pub fn yy_flex_alloc(m: &mut Mem, size: u32) -> u32 {
    m.malloc(size)
}

/// [soft 0x08551830 yy_flex_realloc]: `realloc`.
pub fn yy_flex_realloc(m: &mut Mem, p: u32, size: u32) -> u32 {
    m.realloc(p, size)
}

/// [soft 0x0855184c yy_flex_free]: `free`.
pub fn yy_flex_free(m: &mut Mem, p: u32) {
    m.free(p)
}

/// [soft 0x08551534 SHDB_yy_load_buffer_state].
pub fn shdb_yy_load_buffer_state(m: &mut Mem) {
    let b = m.u32(YY_CURRENT_BUFFER);
    let n = m.u32(b + B_NCHARS);
    let pos = m.u32(b + B_POS);
    m.set_u32(YY_N_CHARS, n);
    let file = m.u32(b + B_FILE);
    m.set_u32(YY_C_BUF_P, pos);
    m.set_u32(SHDB_YYTEXT, pos);
    m.set_u32(SHDB_YYIN, file);
    let c = m.u8(pos);
    m.set_u8(YY_HOLD_CHAR, c);
}

/// [soft 0x08551658 SHDB_yy_flush_buffer].
pub fn shdb_yy_flush_buffer(m: &mut Mem, b: u32) {
    if b == 0 {
        return;
    }
    let buf = m.u32(b + B_BUF);
    m.set_u32(b + B_NCHARS, 0);
    m.set_u8(buf, 0);
    m.set_u8(buf + 1, 0);
    m.set_u32(b + B_POS, buf);
    m.set_u32(b + B_AT_BOL, 1);
    m.set_u32(b + B_STATUS, 0);
    if b == m.u32(YY_CURRENT_BUFFER) {
        shdb_yy_load_buffer_state(m);
    }
}

/// [soft 0x08551628 SHDB_yy_init_buffer].
pub fn shdb_yy_init_buffer(m: &mut Mem, b: u32, file: u32) {
    shdb_yy_flush_buffer(m, b);
    m.set_u32(b + B_FILE, file);
    m.set_u32(b + B_FILL, 1);
    m.set_u32(b + B_INTERACTIVE, 0);
}

/// [soft 0x08551568 SHDB_yy_create_buffer].
pub fn shdb_yy_create_buffer(m: &mut Mem, o: &mut dyn Outside, file: u32, size: i32) -> u32 {
    let b = yy_flex_alloc(m, 0x28);
    if b == 0 {
        yy_fatal_error(m, o, S_NO_MEMORY_CREATE);
    }
    m.set_u32(b + B_SIZE, size as u32);
    let buf = yy_flex_alloc(m, (size as u32).wrapping_add(2));
    m.set_u32(b + B_BUF, buf);
    if buf == 0 {
        yy_fatal_error(m, o, S_NO_MEMORY_CREATE_BUF);
    }
    m.set_u32(b + B_OWN, 1);
    shdb_yy_init_buffer(m, b, file);
    b
}

/// [soft 0x085515e0 SHDB_yy_delete_buffer].
pub fn shdb_yy_delete_buffer(m: &mut Mem, b: u32) {
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

/// [soft 0x08551498 SHDB_yyrestart].
pub fn shdb_yyrestart(m: &mut Mem, o: &mut dyn Outside, file: u32) {
    if m.u32(YY_CURRENT_BUFFER) == 0 {
        let yyin = m.u32(SHDB_YYIN);
        let b = shdb_yy_create_buffer(m, o, yyin, 0x4000);
        m.set_u32(YY_CURRENT_BUFFER, b);
    }
    let cur = m.u32(YY_CURRENT_BUFFER);
    shdb_yy_init_buffer(m, cur, file);
    shdb_yy_load_buffer_state(m);
}

/// [soft 0x085514e0 SHDB_yy_switch_to_buffer].
pub fn shdb_yy_switch_to_buffer(m: &mut Mem, new: u32) {
    let cur = m.u32(YY_CURRENT_BUFFER);
    if cur == new {
        return;
    }
    if cur != 0 {
        let p = m.u32(YY_C_BUF_P);
        let h = m.u8(YY_HOLD_CHAR);
        m.set_u8(p, h);
        let p = m.u32(YY_C_BUF_P);
        m.set_u32(cur + B_POS, p);
        let n = m.u32(YY_N_CHARS);
        m.set_u32(cur + B_NCHARS, n);
    }
    m.set_u32(YY_CURRENT_BUFFER, new);
    shdb_yy_load_buffer_state(m);
    m.set_u32(YY_DID_BUFFER_SWITCH_ON_EOF, 1);
}

/// [soft 0x085516a0 SHDB_yy_scan_buffer]: a buffer over `base[0..size]` (two NULs at the end).
pub fn shdb_yy_scan_buffer(m: &mut Mem, o: &mut dyn Outside, base: u32, size: u32) -> u32 {
    if size > 1 && (m.u8(base + size - 2) != 0 || m.u8(base + size - 1) != 0) || size <= 1 {
        return 0;
    }
    let b = yy_flex_alloc(m, 0x28);
    if b == 0 {
        yy_fatal_error(m, o, S_NO_MEMORY_SCAN_BUFFER);
    }
    m.set_u32(b + B_SIZE, size - 2);
    m.set_u32(b + B_BUF, base);
    m.set_u32(b + B_POS, base);
    m.set_u32(b + B_OWN, 0);
    m.set_u32(b + B_FILE, 0);
    m.set_u32(b + B_NCHARS, size - 2);
    m.set_u32(b + B_INTERACTIVE, 0);
    m.set_u32(b + B_AT_BOL, 1);
    m.set_u32(b + B_FILL, 0);
    m.set_u32(b + B_STATUS, 0);
    shdb_yy_switch_to_buffer(m, b);
    b
}

/// [soft 0x08551764 SHDB_yy_scan_bytes].
pub fn shdb_yy_scan_bytes(m: &mut Mem, o: &mut dyn Outside, bytes: u32, len: i32) -> u32 {
    let n = len.wrapping_add(2) as u32;
    let buf = yy_flex_alloc(m, n);
    if buf == 0 {
        yy_fatal_error(m, o, S_NO_MEMORY_SCAN_BYTES);
    }
    for i in 0..len.max(0) as u32 {
        let c = m.u8(bytes + i);
        m.set_u8(buf + i, c);
    }
    m.set_u8(buf.wrapping_add(len as u32).wrapping_add(1), 0);
    m.set_u8(buf.wrapping_add(len as u32), 0);
    let b = shdb_yy_scan_buffer(m, o, buf, n);
    if b == 0 {
        yy_fatal_error(m, o, S_BAD_SCAN_BYTES);
    }
    m.set_u32(b + B_OWN, 1);
    b
}

/// [soft 0x08551738 SHDB_yy_scan_string].
pub fn shdb_yy_scan_string(m: &mut Mem, o: &mut dyn Outside, s: u32) -> u32 {
    let n = clib::strlen(m, s);
    shdb_yy_scan_bytes(m, o, s, n as i32)
}

/// [soft 0x08551290 yy_get_previous_state]: the state after the text from `yytext` to
/// `yy_c_buf_p`, the characters taken as unsigned (the scanner proper takes them signed).
pub fn yy_get_previous_state(m: &mut Mem) -> u32 {
    let start = m.u32(YY_START);
    let mut state = m.u32(YY_START_STATE_LIST.wrapping_add(start.wrapping_mul(4)));
    let end = m.u32(YY_C_BUF_P);
    let mut cp = m.u32(SHDB_YYTEXT);
    while cp < end {
        let c = m.u8(cp);
        let entry = if c != 0 { state.wrapping_add(c as u32 * 4) } else { state.wrapping_add(0x200) };
        let nxt = m.i16(entry.wrapping_add(2)) as i32;
        state = state.wrapping_add((nxt as u32).wrapping_mul(4));
        if m.u16(state.wrapping_sub(2)) != 0 {
            m.set_u32(YY_LAST_ACCEPTING_STATE, state);
            m.set_u32(YY_LAST_ACCEPTING_CPOS, cp);
        }
        cp += 1;
    }
    state
}

/// [soft 0x085512f0 yy_try_NUL_trans]: the state after a NUL, 0 when the scanner jams.
pub fn yy_try_nul_trans(m: &mut Mem, state: u32) -> u32 {
    let cp = m.u32(YY_C_BUF_P);
    let jam = m.u16(state.wrapping_add(0x200)) == 0x80;
    let nxt = m.i16(state.wrapping_add(0x202)) as i32;
    let next = state.wrapping_add((nxt as u32).wrapping_mul(4));
    if !jam && m.u16(next.wrapping_sub(2)) != 0 {
        m.set_u32(YY_LAST_ACCEPTING_STATE, next);
        m.set_u32(YY_LAST_ACCEPTING_CPOS, cp);
    }
    if jam {
        0
    } else {
        next
    }
}

/// [soft 0x0855133c yyunput]: puts character `c` back in front of `yy_c_buf_p`.
pub fn yyunput(m: &mut Mem, o: &mut dyn Outside, c: i32, mut text: u32) {
    let mut cp = m.u32(YY_C_BUF_P);
    let h = m.u8(YY_HOLD_CHAR);
    m.set_u8(cp, h);
    let cb = m.u32(YY_CURRENT_BUFFER);
    let buf = m.u32(cb + B_BUF);
    if cp < buf.wrapping_add(2) {
        let mut dst = buf.wrapping_add(m.u32(cb + B_SIZE)).wrapping_add(2);
        let mut src = buf.wrapping_add(m.u32(YY_N_CHARS)).wrapping_add(2);
        while buf < src {
            dst = dst.wrapping_sub(1);
            src = src.wrapping_sub(1);
            let v = m.u8(src);
            m.set_u8(dst, v);
            // the loop test reads the buffer's start again
            if m.u32(m.u32(YY_CURRENT_BUFFER) + B_BUF) >= src {
                break;
            }
        }
        let d = dst.wrapping_sub(src);
        cp = cp.wrapping_add(d);
        text = text.wrapping_add(d);
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
    m.set_u32(SHDB_YYTEXT, text);
    let h = m.u8(cp);
    m.set_u8(YY_HOLD_CHAR, h);
}

/// [soft 0x08551080 yy_get_next_buffer]: refills the buffer (the custom input: one character
/// of `parsestring` per call). Answers 0 (continue), 1 (end of file), 2 (last match).
pub fn yy_get_next_buffer(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let mut source = m.u32(SHDB_YYTEXT);
    let cb = m.u32(YY_CURRENT_BUFFER);
    let mut dest = m.u32(cb + B_BUF);
    let cp = m.u32(YY_C_BUF_P);
    if m.u32(YY_N_CHARS).wrapping_add(1).wrapping_add(dest) < cp {
        yy_fatal_error(m, o, S_EOB_MISSED);
    }
    if m.u32(m.u32(YY_CURRENT_BUFFER) + B_FILL) == 0 {
        let yytext = m.u32(SHDB_YYTEXT);
        return if yytext == m.u32(YY_C_BUF_P).wrapping_sub(1) { 1 } else { 2 };
    }
    let to_move = m.u32(YY_C_BUF_P).wrapping_sub(m.u32(SHDB_YYTEXT)).wrapping_sub(1) as i32;
    for _ in 0..to_move.max(0) {
        let c = m.u8(source);
        m.set_u8(dest, c);
        source += 1;
        dest += 1;
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
                let grown = if (size.wrapping_mul(2) as i32) < 1 { size + (size >> 3) } else { size * 2 };
                m.set_u32(cb + B_SIZE, grown);
                let nb = yy_flex_realloc(m, old, grown.wrapping_add(2));
                m.set_u32(cb + B_BUF, nb);
            }
            let nb = m.u32(cb + B_BUF);
            if nb == 0 {
                yy_fatal_error(m, o, S_BUFFER_OVERFLOW);
            }
            m.set_u32(YY_C_BUF_P, cp.wrapping_add(nb.wrapping_sub(old)));
        }
        // the custom input: one character of parsestring
        let text = m.u32(PARSESTRING);
        let cnt = m.u32(CNT);
        let c = m.u8(text.wrapping_add(cnt));
        if c == b'\n' {
            let l = m.u32(LINENO).wrapping_add(1);
            m.set_u32(LINENO, l);
        }
        let valid = c != 0 && c != 0xff;
        if valid {
            m.set_u32(CNT, cnt.wrapping_add(1));
            let cb = m.u32(YY_CURRENT_BUFFER);
            let buf = m.u32(cb + B_BUF);
            m.set_u8(buf.wrapping_add(to_move as u32), c);
        }
        m.set_u32(YY_N_CHARS, valid as u32);
        let cb = m.u32(YY_CURRENT_BUFFER);
        m.set_u32(cb + B_NCHARS, valid as u32);
    }
    let ret;
    if m.u32(YY_N_CHARS) == 0 {
        if to_move == 0 {
            ret = 1;
            let yyin = m.u32(SHDB_YYIN);
            shdb_yyrestart(m, o, yyin);
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
    m.set_u32(SHDB_YYTEXT, buf);
    ret
}

/// [soft 0x085513e8 yyinput]: the next character (-1 at the end).
pub fn yyinput(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let cp = m.u32(YY_C_BUF_P);
    let h = m.u8(YY_HOLD_CHAR);
    m.set_u8(cp, h);
    let text = m.u32(SHDB_YYTEXT);
    if m.u8(cp) == 0 {
        let cb = m.u32(YY_CURRENT_BUFFER);
        if cp < m.u32(YY_N_CHARS).wrapping_add(m.u32(cb + B_BUF)) {
            m.set_u8(cp, 0);
        } else {
            m.set_u32(YY_C_BUF_P, cp + 1);
            match yy_get_next_buffer(m, o) {
                1 => return -1,
                0 => {
                    let t = m.u32(SHDB_YYTEXT);
                    m.set_u32(YY_C_BUF_P, cp.wrapping_add(t.wrapping_sub(text)));
                }
                2 => {
                    let yyin = m.u32(SHDB_YYIN);
                    shdb_yyrestart(m, o, yyin);
                    return -1;
                }
                _ => {}
            }
        }
    }
    let cp = m.u32(YY_C_BUF_P);
    let c = m.u8(cp);
    m.set_u8(cp, 0);
    let next = m.u8(cp + 1);
    m.set_u8(YY_HOLD_CHAR, next);
    m.set_u32(YY_C_BUF_P, cp + 1);
    c as i32
}

/// [soft 0x085519c8 skipcommnts]: skips the rest of the line in `parsestring`.
pub fn skipcommnts(m: &mut Mem) {
    loop {
        let text = m.u32(PARSESTRING);
        let cnt = m.u32(CNT);
        let c = m.u8(text.wrapping_add(cnt));
        m.set_u32(CNT, cnt.wrapping_add(1));
        if c == b'\n' {
            break;
        }
    }
    let l = m.u32(LINENO).wrapping_add(1);
    m.set_u32(LINENO, l);
}

/// [soft 0x08551950 si_shdbInitLEXYACC]: points the input at `text` and clears the errors.
pub fn si_shdb_init_lexyacc(m: &mut Mem, text: u32) {
    m.set_u32(PARSESTRING, text);
    m.set_u32(CNT, 0);
    m.set_u32(LINENO, 1);
    m.set_u16(BERROR, 0);
    m.set_u32(STR_ERROR_V, 0);
    m.set_u32(STR_ERROR_C, 0);
}

/// [soft 0x08551990 si_shdbGetErrorStatus]: the status, the count and the list, each through
/// its pointer when that is not null.
pub fn si_shdb_get_error_status(m: &mut Mem, status: u32, count: u32, list: u32) {
    if count != 0 {
        let c = m.u32(STR_ERROR_C);
        m.set_u32(count, c);
    }
    if list != 0 {
        let v = m.u32(STR_ERROR_V);
        m.set_u32(list, v);
    }
    if status != 0 {
        let b = m.u16(BERROR);
        m.set_u16(status, b);
    }
}

/// [soft 0x08551864 SHDB_yyerror]: records "message at line N of _SHADER_PREVIEWER section"
/// ([`crate::strlist::si_str_list_append`]) and sets `bError`. Answers 1.
///
/// The original formats into a 512-byte local
/// without a bound; the Rust gives the text the room it needs.
pub fn shdb_yyerror(m: &mut Mem, _o: &mut dyn Outside, msg: u32) -> i32 {
    m.set_u16(BERROR, 1);
    let msg = if msg == 0 || clib::strlen(m, msg) == 0 { m.u32(STR_UNKNOWN_ERR) } else { msg };
    let line = m.u32(LINENO);
    let len = cfmt::format(m, S_ERROR_AT_LINE, &[msg, line]).len() as u32 + 1;
    let buf = m.local(len.max(0x200));
    cfmt::sprintf(m, buf, S_ERROR_AT_LINE, &[msg, line]);
    crate::strlist::si_str_list_append(m, buf, STR_ERROR_V, STR_ERROR_C);
    m.release(len.max(0x200));
    1
}

/// [soft 0x08551908 si_shdbErrorToken]: "Unrecognized token <s>" through `SHDB_yyerror`, then
/// frees `s`.
pub fn si_shdb_error_token(m: &mut Mem, o: &mut dyn Outside, s: u32) {
    let len = cfmt::format(m, S_UNRECOGNIZED, &[s]).len() as u32 + 1;
    let buf = m.local(len.max(0x400));
    cfmt::sprintf(m, buf, S_UNRECOGNIZED, &[s]);
    shdb_yyerror(m, o, buf);
    m.release(len.max(0x400));
    if s != 0 {
        m.free(s);
    }
}

/// The scanner's character as `char` (signed), used to index the transition table.
fn sc(m: &Mem, p: u32) -> i32 {
    m.u8(p) as i8 as i32
}

/// [soft 0x08550a60 SHDB_yylex]: the next token of the text in `parsestring`; 0 at the end.
/// Numbers set `SHDB_yylval` (a float for 0x126, an int for 0x125), names a `strdup` (0x101).
pub fn shdb_yylex(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    if m.u32(YY_INIT) != 0 {
        m.set_u32(YY_INIT, 0);
        if m.u32(YY_START) == 0 {
            m.set_u32(YY_START, 1);
        }
        if m.u32(SHDB_YYIN) == 0 {
            let v = m.u32(STDIN);
            m.set_u32(SHDB_YYIN, v);
        }
        if m.u32(SHDB_YYOUT) == 0 {
            let v = m.u32(STDOUT);
            m.set_u32(SHDB_YYOUT, v);
        }
        if m.u32(YY_CURRENT_BUFFER) == 0 {
            let yyin = m.u32(SHDB_YYIN);
            let b = shdb_yy_create_buffer(m, o, yyin, 0x4000);
            m.set_u32(YY_CURRENT_BUFFER, b);
        }
        shdb_yy_load_buffer_state(m);
    }
    'rescan: loop {
        let cp0 = m.u32(YY_C_BUF_P);
        let h = m.u8(YY_HOLD_CHAR);
        m.set_u8(cp0, h);
        let start = m.u32(YY_START);
        let mut state = m.u32(YY_START_STATE_LIST.wrapping_add(start.wrapping_mul(4)));
        let mut bp = m.u32(YY_C_BUF_P);
        let mut cp = bp;
        // true: scan on from `cp` in `state`; false: find the action of `state`
        let mut matching = true;
        loop {
            if matching {
                loop {
                    let c = sc(m, cp);
                    let entry = state.wrapping_add((c as u32).wrapping_mul(4));
                    if m.i16(entry) as i32 != (c as i16) as i32 {
                        break;
                    }
                    let nxt = m.i16(entry + 2) as i32;
                    state = state.wrapping_add((nxt as u32).wrapping_mul(4));
                    if m.u16(state.wrapping_sub(2)) != 0 {
                        m.set_u32(YY_LAST_ACCEPTING_STATE, state);
                        m.set_u32(YY_LAST_ACCEPTING_CPOS, cp);
                    }
                    cp += 1;
                }
            }
            matching = false;
            // find the action
            let mut action = m.i16(state.wrapping_sub(2)) as i32;
            m.set_u32(SHDB_YYLENG, cp.wrapping_sub(bp));
            let h = m.u8(cp);
            m.set_u8(YY_HOLD_CHAR, h);
            m.set_u32(SHDB_YYTEXT, bp);
            m.set_u8(cp, 0);
            m.set_u32(YY_C_BUF_P, cp);
            loop {
                match action {
                    0 => {
                        let h = m.u8(YY_HOLD_CHAR);
                        m.set_u8(cp, h);
                        cp = m.u32(YY_LAST_ACCEPTING_CPOS) + 1;
                        state = m.u32(YY_LAST_ACCEPTING_STATE);
                        break;
                    }
                    1 => continue 'rescan,
                    2 => {
                        skipcommnts(m);
                        continue 'rescan;
                    }
                    3..=35 => {
                        const TOKENS: [i32; 33] = [
                            0x102, 0x103, 0x104, 0x105, 0x106, 0x107, 0x108, 0x109, 0x10a, 0x10b, 0x10c, 0x10d, 0x10f, 0x10e, 0x110,
                            0x111, 0x112, 0x113, 0x114, 0x115, 0x116, 0x119, 0x11b, 0x11c, 0x11d, 0x11e, 0x11f, 0x11a, 0x120, 0x121,
                            0x122, 0x123, 0x124,
                        ];
                        return TOKENS[(action - 3) as usize];
                    }
                    36..=39 => {
                        let text = m.u32(SHDB_YYTEXT);
                        let v = strtod(m, text) as f32;
                        m.set_f32(SHDB_YYLVAL, v);
                        return 0x126;
                    }
                    40 => {
                        let text = m.u32(SHDB_YYTEXT);
                        let v = clib::strtol10(m, text);
                        m.set_i32(SHDB_YYLVAL, v);
                        return 0x125;
                    }
                    41 => {
                        let text = m.u32(SHDB_YYTEXT);
                        let d = m.strdup(text);
                        m.set_u32(SHDB_YYLVAL, d);
                        return 0x101;
                    }
                    42 => {
                        let (t, n, out) = (m.u32(SHDB_YYTEXT), m.u32(SHDB_YYLENG), m.u32(SHDB_YYOUT));
                        fwrite_one(m, o, t, n, out);
                        continue 'rescan;
                    }
                    43 => {
                        // end of buffer
                        let amount = cp.wrapping_sub(m.u32(SHDB_YYTEXT)).wrapping_sub(1);
                        let h = m.u8(YY_HOLD_CHAR);
                        m.set_u8(cp, h);
                        let cb = m.u32(YY_CURRENT_BUFFER);
                        if m.u32(cb + B_STATUS) == 0 {
                            let n = m.u32(cb + B_NCHARS);
                            m.set_u32(YY_N_CHARS, n);
                            let yyin = m.u32(SHDB_YYIN);
                            m.set_u32(cb + B_FILE, yyin);
                            m.set_u32(cb + B_STATUS, 1);
                        }
                        let cb = m.u32(YY_CURRENT_BUFFER);
                        if m.u32(YY_C_BUF_P) <= m.u32(YY_N_CHARS).wrapping_add(m.u32(cb + B_BUF)) {
                            let text = m.u32(SHDB_YYTEXT);
                            m.set_u32(YY_C_BUF_P, text.wrapping_add(amount));
                            let prev = yy_get_previous_state(m);
                            let next = yy_try_nul_trans(m, prev);
                            cp = m.u32(YY_C_BUF_P);
                            bp = m.u32(SHDB_YYTEXT);
                            if next == 0 {
                                state = prev;
                                break;
                            }
                            cp += 1;
                            m.set_u32(YY_C_BUF_P, cp);
                            state = next;
                            matching = true;
                            break;
                        }
                        match yy_get_next_buffer(m, o) {
                            1 => {
                                m.set_u32(YY_DID_BUFFER_SWITCH_ON_EOF, 0);
                                let text = m.u32(SHDB_YYTEXT);
                                m.set_u32(YY_C_BUF_P, text);
                                let st = m.u32(YY_START) as i32;
                                action = (st - 1) / 2 + 0x2c;
                                continue;
                            }
                            2 => {
                                let cb = m.u32(YY_CURRENT_BUFFER);
                                let end = m.u32(YY_N_CHARS).wrapping_add(m.u32(cb + B_BUF));
                                m.set_u32(YY_C_BUF_P, end);
                                state = yy_get_previous_state(m);
                                cp = m.u32(YY_C_BUF_P);
                                bp = m.u32(SHDB_YYTEXT);
                                break;
                            }
                            0 => {
                                let text = m.u32(SHDB_YYTEXT);
                                m.set_u32(YY_C_BUF_P, text.wrapping_add(amount));
                                state = yy_get_previous_state(m);
                                cp = m.u32(YY_C_BUF_P);
                                bp = m.u32(SHDB_YYTEXT);
                                matching = true;
                                break;
                            }
                            _ => continue 'rescan,
                        }
                    }
                    44 => return 0,
                    _ => {
                        yy_fatal_error(m, o, S_INTERNAL_ERROR);
                        continue 'rescan;
                    }
                }
            }
        }
    }
}
