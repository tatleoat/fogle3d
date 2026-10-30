// SPDX-License-Identifier: MIT
//! `mi2control_lex.c`: the flex run-time around the scene-file scanner of mental ray's input (package 62, finding 0042). It is the skeleton every
//! flex 2.5 scanner carries (finding 0013 has the same code for `exp_lex.c`); the buffer record (0x28 bytes): +0 file, +4 character buffer, +8 position,
//! +0xc size, +0x10 characters, +0x14 owns the buffer, +0x18 interactive, +0x1c at line start, +0x20 fill buffer, +0x24 status.
//! The character source is `read(si_stdInput, buf, 1)`, one byte a call; a NUL byte counts as end of input. `yy_fatal_error` ends the program
//! (`exit(2)`); here the caller carries on after the exit routine answers, which the original never does (a check skips such a case).

use si_mem::{clib, Mem, Outside, Routine};

pub const YY_CURRENT_BUFFER: u32 = 0x086d_31c0;
pub const YY_C_BUF_P: u32 = 0x086d_31c4;
pub const YY_START: u32 = 0x086d_31cc;
pub const MI2_YYIN: u32 = 0x086d_31d0;
pub const YY_START_STATE_LIST: u32 = 0x086d_31d8;
pub const YY_HOLD_CHAR: u32 = 0x0881_adb0;
pub const YY_N_CHARS: u32 = 0x0881_adb4;
pub const YY_DID_BUFFER_SWITCH_ON_EOF: u32 = 0x0881_adb8;
pub const MI2_YYTEXT: u32 = 0x0881_adbc;
pub const YY_LAST_ACCEPTING_STATE: u32 = 0x0881_adc0;
pub const YY_LAST_ACCEPTING_CPOS: u32 = 0x0881_adc4;
/// The file descriptor the scanner reads from (`si_stdInput`), one byte per `read`.
pub const INPUT_FD: u32 = 0x0881_a700;
/// The check's source pointer (its stand-in for `read`), in the place of `si_stdOutput`.
pub const SRC_PTR: u32 = 0x0881_a704;
/// `read`, the C library's.
pub const READ: Routine = Routine::new("read", 0x080c_50e8);

/// `exit`, the C library's.
pub const EXIT: Routine = Routine::new("exit", 0x080c_4b58);

/// The messages of the scanner's fatal errors, at their addresses (data of the binary).
pub const MESSAGES: &[(u32, &[u8])] = &[
    (0x086d_3200, b"fatal flex scanner internal error--no action found"),
    (0x086d_3240, b"fatal flex scanner internal error--end of buffer missed"),
    (0x086d_3280, b"fatal error - scanner input buffer overflow"),
    (0x086d_32c0, b"flex scanner push-back overflow"),
    (0x086d_32e0, b"out of dynamic memory in yy_create_buffer()"),
    (0x086d_3320, b"out of dynamic memory in yy_create_buffer()"),
    (0x086d_3360, b"out of dynamic memory in yy_scan_buffer()"),
    (0x086d_33a0, b"out of dynamic memory in yy_scan_bytes()"),
    (0x086d_33c9, b"bad buffer in yy_scan_bytes()"),
    (0x086d_33e7, b"%s
"),
];
pub const S_NO_ACTION: u32 = 0x086d_3200;
const S_EOB_MISSED: u32 = 0x086d_3240;
const S_BUFFER_OVERFLOW: u32 = 0x086d_3280;
const S_PUSH_BACK: u32 = 0x086d_32c0;
const S_CREATE_1: u32 = 0x086d_32e0;
const S_CREATE_2: u32 = 0x086d_3320;
const S_SCAN_BUFFER: u32 = 0x086d_3360;
const S_SCAN_BYTES: u32 = 0x086d_33a0;
const S_BAD_SCAN_BYTES: u32 = 0x086d_33c9;

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

/// [soft 0x0842f6b0 yy_fatal_error]: `fprintf(stderr, "%s\n", msg)` and `exit(2)`.
pub fn yy_fatal_error(m: &mut Mem, o: &mut dyn Outside, msg: u32) {
    let mut text = m.cstr(msg);
    text.push(b'\n');
    o.print(2, &text);
    o.call(m, EXIT, &[2]);
}

/// [soft 0x0842f6d8 yy_flex_alloc]: `malloc`.
pub fn yy_flex_alloc(m: &mut Mem, size: u32) -> u32 {
    m.malloc(size)
}

/// [soft 0x0842f6f0 yy_flex_realloc]: `realloc`.
pub fn yy_flex_realloc(m: &mut Mem, p: u32, size: u32) -> u32 {
    m.realloc(p, size)
}

/// [soft 0x0842f70c yy_flex_free]: `free`.
pub fn yy_flex_free(m: &mut Mem, p: u32) {
    m.free(p)
}

/// [soft 0x0842f3f4 mi2_yy_load_buffer_state].
pub fn mi2_yy_load_buffer_state(m: &mut Mem) {
    let b = m.u32(YY_CURRENT_BUFFER);
    let n = m.u32(b + B_NCHARS);
    let pos = m.u32(b + B_POS);
    m.set_u32(YY_N_CHARS, n);
    let file = m.u32(b + B_FILE);
    m.set_u32(YY_C_BUF_P, pos);
    m.set_u32(MI2_YYTEXT, pos);
    m.set_u32(MI2_YYIN, file);
    let c = m.u8(pos);
    m.set_u8(YY_HOLD_CHAR, c);
}

/// [soft 0x0842f518 mi2_yy_flush_buffer].
pub fn mi2_yy_flush_buffer(m: &mut Mem, b: u32) {
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
        mi2_yy_load_buffer_state(m);
    }
}

/// [soft 0x0842f4e8 mi2_yy_init_buffer].
pub fn mi2_yy_init_buffer(m: &mut Mem, b: u32, file: u32) {
    mi2_yy_flush_buffer(m, b);
    m.set_u32(b + B_FILE, file);
    m.set_u32(b + B_FILL, 1);
    m.set_u32(b + B_INTERACTIVE, 0);
}

/// [soft 0x0842f428 mi2_yy_create_buffer].
pub fn mi2_yy_create_buffer(m: &mut Mem, o: &mut dyn Outside, file: u32, size: i32) -> u32 {
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
    mi2_yy_init_buffer(m, b, file);
    b
}

/// [soft 0x0842f4a0 mi2_yy_delete_buffer].
pub fn mi2_yy_delete_buffer(m: &mut Mem, b: u32) {
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

/// [soft 0x0842f358 mi2_yyrestart].
pub fn mi2_yyrestart(m: &mut Mem, o: &mut dyn Outside, file: u32) {
    if m.u32(YY_CURRENT_BUFFER) == 0 {
        let yyin = m.u32(MI2_YYIN);
        let b = mi2_yy_create_buffer(m, o, yyin, 0x4000);
        m.set_u32(YY_CURRENT_BUFFER, b);
    }
    let cur = m.u32(YY_CURRENT_BUFFER);
    mi2_yy_init_buffer(m, cur, file);
    mi2_yy_load_buffer_state(m);
}

/// [soft 0x0842f3a0 mi2_yy_switch_to_buffer].
pub fn mi2_yy_switch_to_buffer(m: &mut Mem, new: u32) {
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
    mi2_yy_load_buffer_state(m);
    m.set_u32(YY_DID_BUFFER_SWITCH_ON_EOF, 1);
}

/// [soft 0x0842f560 mi2_yy_scan_buffer]: a buffer over `base[0..size]` (two NULs at the end).
pub fn mi2_yy_scan_buffer(m: &mut Mem, o: &mut dyn Outside, base: u32, size: u32) -> u32 {
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
    mi2_yy_switch_to_buffer(m, b);
    b
}

/// [soft 0x0842f624 mi2_yy_scan_bytes].
pub fn mi2_yy_scan_bytes(m: &mut Mem, o: &mut dyn Outside, bytes: u32, len: i32) -> u32 {
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
    let b = mi2_yy_scan_buffer(m, o, buf, n);
    if b == 0 {
        yy_fatal_error(m, o, S_BAD_SCAN_BYTES);
    }
    m.set_u32(b + B_OWN, 1);
    b
}

/// [soft 0x0842f5f8 mi2_yy_scan_string]: the buffer of `mi2_yy_scan_bytes` over the string
/// (the original's wrapper leaves that function's answer in the register).
pub fn mi2_yy_scan_string(m: &mut Mem, o: &mut dyn Outside, s: u32) -> u32 {
    let n = si_mem::clib::strlen(m, s);
    mi2_yy_scan_bytes(m, o, s, n as i32)
}

/// [soft 0x0842f14c yy_get_previous_state]: the state after the text from `yytext` to
/// `yy_c_buf_p`, the characters taken as unsigned (the scanner proper takes them signed).
pub fn yy_get_previous_state(m: &mut Mem) -> u32 {
    let start = m.u32(YY_START);
    let mut state = m.u32(YY_START_STATE_LIST.wrapping_add(start.wrapping_mul(4)));
    let end = m.u32(YY_C_BUF_P);
    let mut cp = m.u32(MI2_YYTEXT);
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

/// [soft 0x0842f1b0 yy_try_NUL_trans]: the state after a NUL when the state's NUL entry
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

/// [soft 0x0842f1fc yyunput]: puts character `c` back in front of `yy_c_buf_p`; `text` is
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
    m.set_u32(MI2_YYTEXT, text);
    let h = m.u8(cp);
    m.set_u8(YY_HOLD_CHAR, h);
}

/// [soft 0x0842ef50 yy_get_next_buffer]: refills the buffer, one byte per `read` of `si_stdInput`. Answers 0 (continue), 1 (end of input), 2 (last match).
pub fn yy_get_next_buffer(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let mut source = m.u32(MI2_YYTEXT);
    let cb = m.u32(YY_CURRENT_BUFFER);
    let mut dest = m.u32(cb + B_BUF);
    let cp = m.u32(YY_C_BUF_P);
    if m.u32(YY_N_CHARS).wrapping_add(1).wrapping_add(dest) < cp {
        yy_fatal_error(m, o, S_EOB_MISSED);
    }
    if m.u32(m.u32(YY_CURRENT_BUFFER) + B_FILL) == 0 {
        let yytext = m.u32(MI2_YYTEXT);
        return if yytext == m.u32(YY_C_BUF_P).wrapping_sub(1) { 1 } else { 2 };
    }
    let to_move = m.u32(YY_C_BUF_P).wrapping_sub(m.u32(MI2_YYTEXT)).wrapping_sub(1) as i32;
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
        // read(si_stdInput, buf + to_move, 1); a NUL byte in the buffer afterwards counts as nothing read
        let fd = m.u32(INPUT_FD);
        let cb = m.u32(YY_CURRENT_BUFFER);
        let at = m.u32(cb + B_BUF).wrapping_add(to_move as u32);
        let r = o.call(m, READ, &[fd, at, 1]).eax;
        m.set_u32(YY_N_CHARS, r);
        let cb = m.u32(YY_CURRENT_BUFFER);
        if m.u8(m.u32(cb + B_BUF).wrapping_add(to_move as u32)) == 0 {
            m.set_u32(YY_N_CHARS, 0);
        }
        let got = m.u32(YY_N_CHARS);
        let cb = m.u32(YY_CURRENT_BUFFER);
        m.set_u32(cb + B_NCHARS, got);
    }
    let ret;
    if m.u32(YY_N_CHARS) == 0 {
        if to_move == 0 {
            ret = 1;
            let yyin = m.u32(MI2_YYIN);
            mi2_yyrestart(m, o, yyin);
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
    m.set_u32(MI2_YYTEXT, buf);
    ret
}

/// [soft 0x0842f2a8 yyinput]: the next character (-1 at the end).
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
            let off = cp.wrapping_sub(m.u32(MI2_YYTEXT));
            m.set_u32(YY_C_BUF_P, cp.wrapping_add(1));
            match yy_get_next_buffer(m, o) {
                1 => return -1,
                0 => m.set_u32(YY_C_BUF_P, off.wrapping_add(m.u32(MI2_YYTEXT))),
                2 => {
                    let yyin = m.u32(MI2_YYIN);
                    mi2_yyrestart(m, o, yyin);
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


/// `write`, the C library's.
pub const WRITE: Routine = Routine::new("write", 0x080c_3a88);
/// `fprintf`, the C library's.
pub const FPRINTF: Routine = Routine::new("fprintf", 0x080c_3b88);
/// `si_stdOutput` (the descriptor the answers go to) and the `stderr` pointer.
pub const STD_OUTPUT: u32 = 0x0881_a704;
const STDERR: u32 = 0x0878_0c88;
/// The format `"%s\n"` of `mi2_yyerror` and the answer texts `"ok\n"`, `"error %d"`.
pub const FMT_ERR: u32 = 0x086d_319a;
pub const TEXT_OK: u32 = 0x086d_319e;
pub const FMT_ERROR: u32 = 0x086d_31a2;

/// [soft 0x0842e908 mi2_yyerror]: `fprintf(stderr, "%s\n", msg)`.
pub fn mi2_yyerror(m: &mut Mem, o: &mut dyn Outside, msg: u32) {
    let err = m.u32(STDERR);
    o.call(m, FPRINTF, &[err, FMT_ERR, msg]);
}

/// [soft 0x0842e92c output_ok]: `write(si_stdOutput, "ok\n", 3)` (the text is copied to a local first).
pub fn output_ok(m: &mut Mem, o: &mut dyn Outside) {
    let buf = m.local(0x24);
    let word = m.u32(TEXT_OK);
    m.set_u32(buf, word);
    let len = m.cstr(buf).len() as u32;
    let fd = m.u32(STD_OUTPUT);
    o.call(m, WRITE, &[fd, buf, len]);
    m.release(0x24);
}

/// [soft 0x0842e978 output_error]: `sprintf(buf, "error %d", code)` and `write(si_stdOutput, buf, strlen(buf))`.
pub fn output_error(m: &mut Mem, o: &mut dyn Outside, code: i32) {
    let buf = m.local(0x20);
    let text = format!("error {code}");
    m.put_cstr(buf, text.as_bytes());
    let fd = m.u32(STD_OUTPUT);
    o.call(m, WRITE, &[fd, buf, text.len() as u32]);
    m.release(0x20);
}

/// `mi2_yylval` (the value of a token: a float, an integer or a string), `mi2_yyleng` and the standard streams' pointers.
pub const MI2_YYLVAL: u32 = 0x0881_ada4;
pub const MI2_YYLENG: u32 = 0x0881_adac;
pub const YY_INIT: u32 = 0x086d_31c8;
pub const MI2_YYOUT: u32 = 0x086d_31d4;
const STDIN: u32 = 0x0878_0e90;
const STDOUT: u32 = 0x0878_0c84;
/// `fwrite`, the C library's.
pub const FWRITE: Routine = Routine::new("fwrite", 0x080c_4f78);

/// `strtod(text, 0)` for the decimal text the scanner hands it.
fn strtod(m: &Mem, s: u32) -> f64 {
    let b = m.cstr(s);
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let start = i;
    if i < b.len() && matches!(b[i], b'+' | b'-') {
        i += 1;
    }
    let digits_at = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut ndig = i - digits_at;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        ndig += i - f;
    }
    if ndig == 0 {
        return 0.0;
    }
    if i < b.len() && matches!(b[i], b'e' | b'E') {
        let mut j = i + 1;
        if j < b.len() && matches!(b[j], b'+' | b'-') {
            j += 1;
        }
        let e = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > e {
            i = j;
        }
    }
    let text = std::str::from_utf8(&b[start..i]).unwrap_or("0");
    text.strip_prefix('+').unwrap_or(text).parse::<f64>().unwrap_or(0.0)
}

#[derive(Clone, Copy)]
enum Step {
    /// start a new token: `yy_c_buf_p`, the start state
    Again,
    /// walk the transition table from `cp`
    Walk,
    /// look up the action of `state` and cut the text
    Find,
    /// carry out action number n
    Act(u32),
}

/// [soft 0x0842e9e0 mi2_yylex]: the generated scanner of the scene language. The tables are the image's: `yy_start_state_list[yy_start]`
/// points into an array of (verify: i16, next: i16) pairs indexed by the (signed) character, the accepting rule at `state - 2`; rule 0 backs
/// up to the last accepting position, rule 1 skips, rules 2..=24 are the tokens 0x101..=0x117, 25 and 26 a float (token 0x119, a single
/// stored at `mi2_yylval`), 27 an integer (0x118), 28 a string copy (0x11a), 29 any other character as itself, 30 echoes the text to
/// `mi2_yyout`, 31 is the end of the buffer (refill, 0x20 + (yy_start - 1) / 2 at the end of the input) and 32 the end of the input (0).
/// A scanner error (`no action found`) ends the program in the original; here the answer is 0.
pub fn mi2_yylex(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    if m.u32(YY_INIT) != 0 {
        m.set_u32(YY_INIT, 0);
        if m.u32(YY_START) == 0 {
            m.set_u32(YY_START, 1);
        }
        if m.u32(MI2_YYIN) == 0 {
            let v = m.u32(STDIN);
            m.set_u32(MI2_YYIN, v);
        }
        if m.u32(MI2_YYOUT) == 0 {
            let v = m.u32(STDOUT);
            m.set_u32(MI2_YYOUT, v);
        }
        if m.u32(YY_CURRENT_BUFFER) == 0 {
            let yyin = m.u32(MI2_YYIN);
            let b = mi2_yy_create_buffer(m, o, yyin, 0x4000);
            m.set_u32(YY_CURRENT_BUFFER, b);
        }
        mi2_yy_load_buffer_state(m);
    }
    let (mut cp, mut bp, mut state) = (0u32, 0u32, 0u32);
    let mut step = Step::Again;
    loop {
        match step {
            Step::Again => {
                cp = m.u32(YY_C_BUF_P);
                let h = m.u8(YY_HOLD_CHAR);
                m.set_u8(cp, h);
                let start = m.u32(YY_START);
                state = m.u32(YY_START_STATE_LIST.wrapping_add(start.wrapping_mul(4)));
                bp = cp;
                step = Step::Walk;
            }
            Step::Walk => {
                loop {
                    let c = m.u8(cp) as i8 as i32;
                    let entry = state.wrapping_add((c as u32).wrapping_mul(4));
                    if m.i16(entry) != c as i16 {
                        break;
                    }
                    let next = m.i16(entry.wrapping_add(2)) as i32;
                    state = state.wrapping_add((next as u32).wrapping_mul(4));
                    if m.u16(state.wrapping_sub(2)) != 0 {
                        m.set_u32(YY_LAST_ACCEPTING_STATE, state);
                        m.set_u32(YY_LAST_ACCEPTING_CPOS, cp);
                    }
                    cp = cp.wrapping_add(1);
                }
                step = Step::Find;
            }
            Step::Find => {
                let act = m.i16(state.wrapping_sub(2)) as i32 as u32;
                m.set_u32(MI2_YYTEXT, bp);
                m.set_u32(MI2_YYLENG, cp.wrapping_sub(bp));
                let h = m.u8(cp);
                m.set_u8(YY_HOLD_CHAR, h);
                m.set_u8(cp, 0);
                m.set_u32(YY_C_BUF_P, cp);
                step = Step::Act(act);
            }
            Step::Act(act) => {
                if act > 0x20 {
                    yy_fatal_error(m, o, S_NO_ACTION);
                    return 0;
                }
                match act {
                    0 => {
                        let h = m.u8(YY_HOLD_CHAR);
                        m.set_u8(cp, h);
                        cp = m.u32(YY_LAST_ACCEPTING_CPOS).wrapping_add(1);
                        state = m.u32(YY_LAST_ACCEPTING_STATE);
                        step = Step::Find;
                    }
                    1 => step = Step::Again,
                    2..=24 => return 0x100 + act as i32 - 1,
                    25 | 26 => {
                        let text = m.u32(MI2_YYTEXT);
                        let v = strtod(m, text) as f32;
                        m.set_u32(MI2_YYLVAL, v.to_bits());
                        return 0x119;
                    }
                    27 => {
                        let text = m.u32(MI2_YYTEXT);
                        let v = clib::strtol(m, text, 0, 10);
                        m.set_i32(MI2_YYLVAL, v);
                        return 0x118;
                    }
                    28 => {
                        let text = m.u32(MI2_YYTEXT);
                        let v = m.strdup(text);
                        m.set_u32(MI2_YYLVAL, v);
                        return 0x11a;
                    }
                    29 => {
                        let text = m.u32(MI2_YYTEXT);
                        return m.u8(text) as i8 as i32;
                    }
                    30 => {
                        let (text, len, out) = (m.u32(MI2_YYTEXT), m.u32(MI2_YYLENG), m.u32(MI2_YYOUT));
                        o.call(m, FWRITE, &[text, len, 1, out]);
                        step = Step::Again;
                    }
                    31 => {
                        let amount = cp.wrapping_sub(m.u32(MI2_YYTEXT)).wrapping_sub(1);
                        let h = m.u8(YY_HOLD_CHAR);
                        m.set_u8(cp, h);
                        let cb = m.u32(YY_CURRENT_BUFFER);
                        if m.u32(cb + B_STATUS) == 0 {
                            let n = m.u32(cb + B_NCHARS);
                            m.set_u32(YY_N_CHARS, n);
                            let yyin = m.u32(MI2_YYIN);
                            m.set_u32(cb + B_FILE, yyin);
                            m.set_u32(cb + B_STATUS, 1);
                        }
                        let cb = m.u32(YY_CURRENT_BUFFER);
                        let limit = m.u32(YY_N_CHARS).wrapping_add(m.u32(cb + B_BUF));
                        if m.u32(YY_C_BUF_P) <= limit {
                            // a NUL in the text: the state after the text so far, then the transition on NUL
                            let p = amount.wrapping_add(m.u32(MI2_YYTEXT));
                            m.set_u32(YY_C_BUF_P, p);
                            state = yy_get_previous_state(m);
                            let next = yy_try_nul_trans(m, state);
                            bp = m.u32(MI2_YYTEXT);
                            if next != 0 {
                                let p = m.u32(YY_C_BUF_P).wrapping_add(1);
                                m.set_u32(YY_C_BUF_P, p);
                                cp = p;
                                state = next;
                                step = Step::Walk;
                            } else {
                                cp = m.u32(YY_C_BUF_P);
                                step = Step::Find;
                            }
                        } else {
                            match yy_get_next_buffer(m, o) {
                                1 => {
                                    m.set_u32(YY_DID_BUFFER_SWITCH_ON_EOF, 0);
                                    let t = m.u32(MI2_YYTEXT);
                                    m.set_u32(YY_C_BUF_P, t);
                                    let s = (m.u32(YY_START) as i32).wrapping_sub(1);
                                    step = Step::Act((0x20i32.wrapping_add(s / 2)) as u32);
                                }
                                0 => {
                                    let p = amount.wrapping_add(m.u32(MI2_YYTEXT));
                                    m.set_u32(YY_C_BUF_P, p);
                                    state = yy_get_previous_state(m);
                                    cp = m.u32(YY_C_BUF_P);
                                    bp = m.u32(MI2_YYTEXT);
                                    step = Step::Walk;
                                }
                                2 => {
                                    let cb = m.u32(YY_CURRENT_BUFFER);
                                    let p = m.u32(YY_N_CHARS).wrapping_add(m.u32(cb + B_BUF));
                                    m.set_u32(YY_C_BUF_P, p);
                                    state = yy_get_previous_state(m);
                                    cp = m.u32(YY_C_BUF_P);
                                    bp = m.u32(MI2_YYTEXT);
                                    step = Step::Find;
                                }
                                _ => step = Step::Again,
                            }
                        }
                    }
                    _ => return 0,
                }
            }
        }
    }
}
