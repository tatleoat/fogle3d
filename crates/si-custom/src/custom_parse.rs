// SPDX-License-Identifier: MIT
//! The reader of the custom data text (`custom_dialog.c`): the `ANIMATION` and `DATA` sections of a custom
//! effect's saved user data are cut into lines and parsed into the dialog's value list (finding 0041,
//! twelfth continuation, unit custom11). The text parsed here is what `custom_enc` writes.
//!
//! The C library's `strtok` keeps its position in a hidden variable; the chain below (`SI_cusParseDataString`
//! -> `cusParseAnimationString` -> `cusParseFcvkeyString`) shares it, so a cell `saved` is threaded through.
//! The constant strings the code compares with or splits by are the program's (`tokWHITESPACES`, `tokEND`,
//! `tokDATA`, `tokANIMATION` are pointer variables; the delimiter "\r\n" and the keywords are literals).

use crate::cfmt;
use crate::custom_gen::SI_CUS_TRACE;
use crate::custom_io::SI_FKEY_INITIALIZE;
use crate::custom_util::{SI_FCURVE_ADD_KEY, SI_FCURVE_ALLOCATE};
use crate::custom_val::cusval_get_next_keyable_value;
use crate::shdb_init::SI_STATUS_DISPLAY_MESSAGE;
use si_mem::{clib, Mem, Outside, Routine};

pub const SI_FKEY_CREATE: Routine = Routine::new("SI_fkeyCreate", 0x0820_b248);
pub const SI_CUS_DIALOG_SET: Routine = Routine::new("SI_cusDialogSet", 0x0852_3e50);
pub const SI_CUS_PARSE_DATA_STRING_P: Routine = Routine::new("SI_cusParseDataString", 0x0852_487c);
pub const SI_CUS_READ_DATA_SECTION_P: Routine = Routine::new("SI_cusReadDataSection", 0x0852_40f0);
/// `tokWHITESPACES`, `tokDATA`, `tokANIMATION`, `tokEND` (pointer variables), `SI_frameCurrent` (float).
const TOK_WHITESPACES: u32 = 0x086d_c2c8;
const TOK_DATA: u32 = 0x086d_c2d4;
const TOK_ANIMATION: u32 = 0x086d_c2e4;
const TOK_END: u32 = 0x086d_c2f0;
const SI_FRAME_CURRENT: u32 = 0x0878_2040;

/// The literal strings at the program's addresses that the trace calls pass on (the image holds none of the
/// program's data: the address is what the call carries).
const FMT_PARSING: u32 = 0x086d_c7d4;
const NULL_STR: u32 = 0x086d_c7cd;
const FMT_NKEYS: u32 = 0x086d_c800;
const FMT_NLINES: u32 = 0x086d_c820;
const FMT_KEYTIME_LINE: u32 = 0x086d_c840;
const FMT_ANALYZING: u32 = 0x086d_c881;
const FMT_FOUND_END: u32 = 0x086d_c897;
const FMT_SEARCHING: u32 = 0x086d_c8c0;
const FMT_FOUND_SYMBOL: u32 = 0x086d_c8e7;
const FMT_NOARG_STR: u32 = 0x086d_c900;
const NULL_STR2: u32 = 0x086d_c926;
const FMT_STRING_VALUE: u32 = 0x086d_c92d;
const FMT_NOARG_INT: u32 = 0x086d_c960;
const FMT_INTEGER_VALUE: u32 = 0x086d_c986;
const FMT_NOARG_STATE: u32 = 0x086d_c9a0;
const FMT_STATE_VALUE: u32 = 0x086d_c9c6;
const FMT_NOARG_FLOAT: u32 = 0x086d_c9e0;
const FMT_FLOAT0: u32 = 0x086d_ca06;
const NULL_STR3: u32 = 0x086d_ca18;
const FMT_KEYTIME_SYMBOL1: u32 = 0x086d_ca20;
const FMT_FLOATN: u32 = 0x086d_ca71;
const FMT_NOARG_COLOR: u32 = 0x086d_caa0;
const FMT_COLOR0: u32 = 0x086d_caca;
const NULL_STR4: u32 = 0x086d_cae7;
const FMT_KEYTIME_SYMBOL2: u32 = 0x086d_cb00;
const FMT_COLORN: u32 = 0x086d_cb51;
const NULL_STR5: u32 = 0x086d_cb6f;
const FMT_FOUND_CONST: u32 = 0x086d_cb76;
const NULL_STR6: u32 = 0x086d_cb94;
const FMT_CONST_VALUE: u32 = 0x086d_cb9b;
/// The text of `FMT_KEYTIME_SYMBOL1` and `...2` (the same words twice in the program's data).
const KEYTIME_SYMBOL_TEXT: &[u8] = b"Parsing shader parms file _DATA section:Cannot read key time [%d] of symbol[%s]\n";
const KEYTIME_LINE_TEXT: &[u8] = b"Parsing shader parms file _DATA line: Cannot read key time [%d]\n";

/// `strtok(s, delim)` with a delimiter held by the program's pointer variable.
fn tok_ws(m: &mut Mem, s: u32, saved: &mut u32) -> u32 {
    let d = m.u32(TOK_WHITESPACES);
    clib::strtok(m, s, d, saved)
}

/// `strtok(s, "\r\n")`: the delimiter is a literal of the program, put on the stack for the call.
fn tok_crlf(m: &mut Mem, s: u32, saved: &mut u32) -> u32 {
    let d = m.local(4);
    m.set_bytes(d, b"\r\n\0");
    let r = clib::strtok(m, s, d, saved);
    m.release(4);
    r
}

/// `strtol(s, 0, 10)`.
fn atoi(m: &Mem, s: u32) -> i32 {
    clib::strtol10(m, s)
}

/// `(float)strtod(s, 0)`: the double in the x87 register stored as a float.
fn atof32(m: &Mem, s: u32) -> f32 {
    cfmt::strtod(m, s) as f32
}


fn trace(m: &mut Mem, o: &mut dyn Outside, args: &[u32]) {
    o.call(m, SI_CUS_TRACE, args);
}

fn dbl(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

fn dbl32(v: f32) -> [u32; 2] {
    dbl(v as f64)
}

/// [soft 0x0852911c cusParseFcvkeyString]: the lines of one fcurve key (`TIME`, `VALUE`, `LEFT_SLOPE`,
/// `RIGHT_SLOPE`, `INTERPOLATION`, `FLAG`, `SPLINE_INDEX`, then `KEYTYPE` which ends the key) into the
/// 0x24-byte key `key`: floats at +0, +4, +8, +0x18, shorts at +0xc (flag), +0xe (interpolation), +0x1c
/// (key type), +0x1e (spline index). Returns the index of the line it stopped at (the `KEYTYPE` line, or
/// the number of lines).
pub fn cus_parse_fcvkey_string(m: &mut Mem, key: u32, lines: u32, count: i32, saved: &mut u32) -> i32 {
    let mut i: i32 = 0;
    while i < count {
        let line = m.u32(lines.wrapping_add((i as u32).wrapping_mul(4)));
        let tok = tok_ws(m, line, saved);
        if tok != 0 {
            let t = m.cstr(tok);
            match &t[..] {
                b"TIME" => {
                    let v = tok_ws(m, 0, saved);
                    if v != 0 {
                        m.set_f32(key, atof32(m, v));
                    }
                }
                b"VALUE" => {
                    let v = tok_ws(m, 0, saved);
                    if v != 0 {
                        m.set_f32(key.wrapping_add(4), atof32(m, v));
                    }
                }
                b"LEFT_SLOPE" => {
                    let v = tok_ws(m, 0, saved);
                    if v != 0 {
                        m.set_f32(key.wrapping_add(0x18), atof32(m, v));
                    }
                }
                b"RIGHT_SLOPE" => {
                    let v = tok_ws(m, 0, saved);
                    if v != 0 {
                        m.set_f32(key.wrapping_add(8), atof32(m, v));
                    }
                }
                b"INTERPOLATION" => {
                    let v = tok_ws(m, 0, saved);
                    if v != 0 {
                        let k = match &m.cstr(v)[..] {
                            b"CONSTANT" => Some(0u16),
                            b"LINEAR" => Some(1),
                            b"SPLINE" => Some(2),
                            b"SLOPE0" => Some(3),
                            b"PLATEAU" => Some(4),
                            _ => None,
                        };
                        if let Some(k) = k {
                            m.set_u16(key.wrapping_add(0xe), k);
                        }
                    }
                }
                b"FLAG" => {
                    let v = tok_ws(m, 0, saved);
                    if v != 0 {
                        let n = atoi(m, v);
                        m.set_u16(key.wrapping_add(0xc), n as u16);
                    }
                }
                b"SPLINE_INDEX" => {
                    let v = tok_ws(m, 0, saved);
                    if v != 0 {
                        let n = atoi(m, v);
                        m.set_u16(key.wrapping_add(0x1e), n as u16);
                    }
                }
                b"KEYTYPE" => {
                    // the end of the key: the line is not counted, whatever follows
                    let v = tok_ws(m, 0, saved);
                    if v != 0 {
                        let k = match &m.cstr(v)[..] {
                            b"FREEFORM" => Some(0u16),
                            b"KEYFRAME" => Some(1),
                            b"KEYPATH" => Some(2),
                            b"RAWDATA" => Some(3),
                            _ => None,
                        };
                        if let Some(k) = k {
                            m.set_u16(key.wrapping_add(0x1c), k);
                        }
                    }
                    return i;
                }
                _ => {}
            }
        }
        i += 1;
    }
    i
}

/// [soft 0x085289fc cusParseAnimationString]: the lines between the `ANIMATION` header and the `END` token.
/// `CURVE_TYPE <name> <k>` selects fcurve `k` (0..3) of the keyable value called `name` in the dialog's
/// list (creating it, interpolation spline, when the value has none yet); `PRE_EXTRAPOLATION`,
/// `POST_EXTRAPOLATION`, `FLAGS`, `INTERPOLATION` set fields of that curve; `KEYS n` allocates `n` keys
/// (rounded up to a multiple of 8) and reads them with [`cus_parse_fcvkey_string`]. Returns the index of
/// the `END` line (or the number of lines).
pub fn cus_parse_animation_string(m: &mut Mem, o: &mut dyn Outside, udf: u32, dlg: u32, lines: u32, count: i32, saved: &mut u32) -> i32 {
    let mut i: i32 = 0;
    let mut cur: u32 = 0; // the curve being filled (the original's local at ebp-4)
    while i < count {
        let line = m.u32(lines.wrapping_add((i as u32).wrapping_mul(4)));
        let tok = tok_ws(m, line, saved);
        if tok != 0 {
            let end = m.u32(TOK_END);
            if clib::strcmp(m, tok, end) == 0 {
                return i;
            }
            let t = m.cstr(tok);
            if t == b"CURVE_TYPE" {
                let name = tok_ws(m, 0, saved);
                if name != 0 {
                    let mut v = dlg.wrapping_add(0x10);
                    let mut found = true;
                    loop {
                        v = cusval_get_next_keyable_value(m, v, 0);
                        if v == 0 {
                            found = false;
                            break;
                        }
                        let vn = m.u32(v);
                        if clib::strcmp(m, name, vn) == 0 {
                            break;
                        }
                    }
                    if found {
                        let num = tok_ws(m, 0, saved);
                        if num != 0 {
                            let k = atoi(m, num) as u32;
                            if k <= 3 {
                                let slot = v.wrapping_add(0x2c).wrapping_add(k.wrapping_mul(4));
                                if m.u32(slot) == 0 {
                                    let fc = o.call(m, SI_FCURVE_ALLOCATE, &[0x23a, udf]).eax;
                                    m.set_u32(slot, fc);
                                    if fc != 0 {
                                        m.set_u16(fc.wrapping_add(0x1e), 2);
                                        cur = fc;
                                    }
                                }
                            }
                        }
                    }
                }
            } else if cur != 0 {
                match &t[..] {
                    b"PRE_EXTRAPOLATION" | b"POST_EXTRAPOLATION" | b"FLAGS" => {
                        let v = tok_ws(m, 0, saved);
                        if v != 0 {
                            let n = atoi(m, v);
                            let at = match &t[..] {
                                b"PRE_EXTRAPOLATION" => 0x18,
                                b"POST_EXTRAPOLATION" => 0x1a,
                                _ => 0x1c,
                            };
                            m.set_u16(cur.wrapping_add(at), n as u16);
                        }
                    }
                    b"INTERPOLATION" => {
                        let v = tok_ws(m, 0, saved);
                        if v != 0 {
                            let k = match &m.cstr(v)[..] {
                                b"CONSTANT" => Some(0u16),
                                b"LINEAR" => Some(1),
                                b"SPLINE" => Some(2),
                                b"SLOPE0" => Some(3),
                                b"PLATEAU" => Some(4),
                                _ => None,
                            };
                            if let Some(k) = k {
                                m.set_u16(cur.wrapping_add(0x1e), k);
                            }
                        }
                    }
                    b"KEYS" => {
                        let v = tok_ws(m, 0, saved);
                        if v != 0 {
                            let n = atoi(m, v);
                            m.set_u32(cur.wrapping_add(0xc), n as u32);
                            if n < 0 {
                                m.set_u32(cur.wrapping_add(0xc), 0);
                            } else if n > 0 {
                                // n rounded up to a multiple of 8
                                let size = (n as u32).wrapping_sub(((n as u32).wrapping_sub(1) & 7).wrapping_sub(7));
                                let keys = m.si_calloc(size, 0x24);
                                m.set_u32(cur.wrapping_add(8), keys);
                                if keys == 0 {
                                    m.set_u32(cur.wrapping_add(0xc), 0);
                                } else {
                                    let mut j: i32 = 0;
                                    let mut off: u32 = 0;
                                    while j < m.u32(cur.wrapping_add(0xc)) as i32 {
                                        let key = off.wrapping_add(m.u32(cur.wrapping_add(8)));
                                        o.call(m, SI_FKEY_INITIALIZE, &[key]);
                                        i += 1;
                                        if i < count {
                                            let p = lines.wrapping_add((i as u32).wrapping_mul(4));
                                            i += cus_parse_fcvkey_string(m, key, p, count - i, saved);
                                        }
                                        j += 1;
                                        let base = m.u32(cur.wrapping_add(8));
                                        m.set_u32(base.wrapping_add(off).wrapping_add(0x10), cur);
                                        let base = m.u32(cur.wrapping_add(8));
                                        m.set_u16(base.wrapping_add(off).wrapping_add(0x14), 0);
                                        off = off.wrapping_add(0x24);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        i += 1;
    }
    i
}

/// Formats the error text of a key time that cannot be read into a local buffer, shows it as a status
/// message (kind 3) and traces it (the original passes the text itself as the trace format).
fn key_time_error(m: &mut Mem, o: &mut dyn Outside, fmt: u32, text: &[u8], k: u32, name: u32) {
    let buf = m.local(0x200);
    let a = m.local(text.len() as u32 + 1);
    m.set_bytes(a, text);
    m.set_u8(a.wrapping_add(text.len() as u32), 0);
    // `fmt` is the program's copy of this text; the image formats the same words
    let _ = fmt;
    cfmt::sprintf(m, buf, a, &[k, name]);
    m.release(text.len() as u32 + 1);
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[buf, 3]);
    trace(m, o, &[buf]);
    m.release(0x200);
}

/// The strdup'ed copies of the lines are freed with the line table.
fn release_lines(m: &mut Mem, lines: u32, n: i32) {
    for i in 0..n {
        let a = lines.wrapping_add((i as u32).wrapping_mul(4));
        let l = m.u32(a);
        if l != 0 {
            m.free(l);
            m.set_u32(a, 0);
        }
    }
    if lines != 0 {
        m.free(lines);
    }
}

/// [soft 0x0852487c SI_cusParseDataString]: parses the text `s` (lines split at CR and LF) into dialog `dlg`
/// (a dialog of kind 6 is an effect's: an `ANIMATION` section may come first, the `DATA` line holds the key
/// count and key times). Each following line `NAME args` finds the dialog's values called NAME and stores:
/// text (type 0, a copy of the rest), integer (2), state (3, short), float (1, +0x10) with one key per key time
/// when the line is not animated, colour (4: four floats at +0x1c, one fcurve each). A line whose name is
/// not in the dialog, when `keep_const` is 1, becomes a 0x50-byte constant node (name, text at +0x18, type
/// 0, id -1) of the list returned. A key time or key value that cannot be read ends the parse (0). For a dialog
/// of another kind the first line is skipped and the dialog's values are refreshed at the end
/// (`SI_cusDialogSet`). Frees the lines and the key times. Returns the constants list (or 0).
pub fn si_cus_parse_data_string(m: &mut Mem, o: &mut dyn Outside, udf: u32, dlg: u32, s: u32, keep_const: u32) -> u32 {
    let keep_const = keep_const as u16;
    let mut consts: u32 = 0;
    let mut nkeys: i32 = 0;
    let mut times: u32 = 0;
    let mut idx: i32 = 0;
    let mut animated = false;
    let mut saved = 0u32;
    trace(m, o, &[FMT_PARSING, if s == 0 { NULL_STR } else { s }]);
    if s == 0 || dlg == 0 {
        return 0;
    }
    // cut the text into lines
    let dup = clib::strdup(m, s);
    let mut nlines: i32 = 0;
    let mut lines: u32 = 0;
    let mut tok = tok_crlf(m, dup, &mut saved);
    while tok != 0 {
        if clib::strlen(m, tok) != 0 {
            let n1 = (nlines + 1) as u32;
            // the original reallocates first and stores the copy after: a failed realloc ends the loop
            lines = m.si_realloc(lines, n1.wrapping_mul(4));
            if lines == 0 {
                break;
            }
            let copy = clib::strdup(m, tok);
            m.set_u32(lines.wrapping_add((nlines as u32).wrapping_mul(4)), copy);
            nlines += 1;
        }
        tok = tok_crlf(m, 0, &mut saved);
    }
    if dup != 0 {
        m.free(dup);
    }
    let kind6 = |m: &Mem| m.u16(dlg) == 6;
    if kind6(m) {
        idx = 0;
        let anim = m.u32(TOK_ANIMATION);
        let alen = clib::strlen(m, anim);
        let first = m.u32(lines);
        if clib::strncmp(m, first, anim, alen) == 0 {
            animated = true;
            idx = 1;
            if idx < nlines {
                let mut sv = 0u32;
                idx = cus_parse_animation_string(m, o, udf, dlg, lines.wrapping_add(4), nlines - 1, &mut sv) + 1;
            }
            idx += 1;
        }
    }
    if kind6(m) {
        // the DATA line: `_DATA n t0 t1 ..` (the keyword is skipped by its length, not compared)
        let dtok = m.u32(TOK_DATA);
        let dlen = clib::strlen(m, dtok);
        let line = m.u32(lines.wrapping_add((idx as u32).wrapping_mul(4)));
        let t = tok_ws(m, line.wrapping_add(dlen), &mut saved);
        if t == 0 {
            nkeys = 0;
        } else {
            nkeys = atoi(m, t);
            if nkeys < 0 {
                nkeys = 0;
            }
        }
    }
    trace(m, o, &[FMT_NKEYS, nkeys as u32]);
    trace(m, o, &[FMT_NLINES, nlines as u32]);
    if nkeys > 0 {
        times = m.si_calloc(nkeys as u32, 4);
        for k in 0..nkeys {
            let t = tok_ws(m, 0, &mut saved);
            if t == 0 {
                // the key times cannot all be read
                let buf = m.local(0x200);
                let a = m.local(KEYTIME_LINE_TEXT.len() as u32 + 1);
                m.set_bytes(a, KEYTIME_LINE_TEXT);
                m.set_u8(a.wrapping_add(KEYTIME_LINE_TEXT.len() as u32), 0);
                cfmt::sprintf(m, buf, a, &[k as u32]);
                m.release(KEYTIME_LINE_TEXT.len() as u32 + 1);
                o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[buf, 3]);
                trace(m, o, &[buf]);
                m.release(0x200);
                let _ = FMT_KEYTIME_LINE;
                return 0;
            }
            let v = atof32(m, t);
            m.set_f32(times.wrapping_add((k as u32).wrapping_mul(4)), v);
        }
    }
    // the lines after the header
    idx += 1;
    'lines: while idx < nlines {
        let line = m.u32(lines.wrapping_add((idx as u32).wrapping_mul(4)));
        let tok = tok_ws(m, line, &mut saved);
        if tok == 0 || m.u8(tok) == 0x23 {
            idx += 1;
            continue;
        }
        trace(m, o, &[FMT_ANALYZING, tok]);
        let end = m.u32(TOK_END);
        let elen = clib::strlen(m, end);
        if clib::strncmp(m, tok, end, elen) == 0 {
            trace(m, o, &[FMT_FOUND_END]);
            break;
        }
        trace(m, o, &[FMT_SEARCHING, tok, idx as u32]);
        let mut found = false;
        let mut cur = m.u32(dlg.wrapping_add(0x54));
        let mut exhausted = true; // the list ran out (the tail of the loop of the original)
        while cur != 0 {
            let name = m.u32(cur);
            if clib::strcmp(m, tok, name) == 0 {
                found = true;
                trace(m, o, &[FMT_FOUND_SYMBOL, tok]);
                let ty = m.u16(cur.wrapping_add(4));
                let mut stop = false;
                match ty {
                    0 => {
                        let v = tok_crlf(m, 0, &mut saved);
                        if v != 0 {
                            let d = clib::strdup(m, v);
                            m.set_u32(cur.wrapping_add(0x18), d);
                        } else {
                            trace(m, o, &[FMT_NOARG_STR, tok]);
                            m.set_u32(cur.wrapping_add(0x18), 0);
                        }
                        trace(m, o, &[FMT_STRING_VALUE, if v == 0 { NULL_STR2 } else { v }]);
                    }
                    2 => {
                        let v = tok_ws(m, 0, &mut saved);
                        if v != 0 {
                            let n = atoi(m, v);
                            m.set_u32(cur.wrapping_add(0x14), n as u32);
                        } else {
                            trace(m, o, &[FMT_NOARG_INT, tok]);
                            m.set_u32(cur.wrapping_add(0x14), 0);
                        }
                        let n = m.u32(cur.wrapping_add(0x14));
                        trace(m, o, &[FMT_INTEGER_VALUE, n]);
                    }
                    3 => {
                        let v = tok_ws(m, 0, &mut saved);
                        if v != 0 {
                            let n = atoi(m, v);
                            m.set_u16(cur.wrapping_add(0xc), n as u16);
                        } else {
                            trace(m, o, &[FMT_NOARG_STATE, tok]);
                            m.set_u16(cur.wrapping_add(0xc), 0);
                        }
                        let n = m.i16(cur.wrapping_add(0xc)) as i32 as u32;
                        trace(m, o, &[FMT_STATE_VALUE, n]);
                    }
                    1 => {
                        let v = tok_ws(m, 0, &mut saved);
                        if v != 0 {
                            let f = atof32(m, v);
                            m.set_f32(cur.wrapping_add(0x10), f);
                        } else {
                            trace(m, o, &[FMT_NOARG_FLOAT, tok]);
                            m.set_u32(cur.wrapping_add(0x10), 0);
                        }
                        let f = m.f32(cur.wrapping_add(0x10));
                        let d = dbl32(f);
                        trace(m, o, &[FMT_FLOAT0, d[0], d[1]]);
                        if nkeys > 0 && !animated {
                            let fc = if kind6(m) {
                                o.call(m, SI_FCURVE_ALLOCATE, &[0x23a, udf]).eax
                            } else {
                                o.call(m, SI_FCURVE_ALLOCATE, &[0, cur]).eax
                            };
                            m.set_u32(cur.wrapping_add(0x2c), fc);
                            let fc = m.u32(cur.wrapping_add(0x2c));
                            m.set_u16(fc.wrapping_add(0x1e), 2);
                            let t0 = m.u32(times);
                            let val = m.u32(cur.wrapping_add(0x10));
                            let key = o.call(m, SI_FKEY_CREATE, &[t0, val, 0, 0, 2]).eax;
                            let fc = m.u32(cur.wrapping_add(0x2c));
                            o.call(m, SI_FCURVE_ADD_KEY, &[fc, key]);
                        }
                        let mut k: i32 = 1;
                        while k < nkeys {
                            let v = tok_ws(m, 0, &mut saved);
                            if v == 0 {
                                let nm = m.u32(cur);
                                key_time_error(m, o, FMT_KEYTIME_SYMBOL1, KEYTIME_SYMBOL_TEXT, k as u32, if nm == 0 { NULL_STR3 } else { nm });
                                return 0;
                            }
                            if !animated {
                                let f = atof32(m, v);
                                let tk = m.u32(times.wrapping_add((k as u32).wrapping_mul(4)));
                                let key = o.call(m, SI_FKEY_CREATE, &[tk, f.to_bits(), 0, 0, 2]).eax;
                                let fc = m.u32(cur.wrapping_add(0x2c));
                                o.call(m, SI_FCURVE_ADD_KEY, &[fc, key]);
                            }
                            let d = dbl(cfmt::strtod(m, v));
                            trace(m, o, &[FMT_FLOATN, k as u32, d[0], d[1]]);
                            k += 1;
                        }
                        stop = true;
                    }
                    4 => {
                        let base = cur.wrapping_add(0x1c);
                        for c in 0..4u32 {
                            let v = tok_ws(m, 0, &mut saved);
                            let at = base.wrapping_add(c * 4);
                            if v != 0 {
                                let f = atof32(m, v);
                                m.set_f32(at, f);
                            } else {
                                trace(m, o, &[FMT_NOARG_COLOR, tok, c]);
                                m.set_u32(at, 0);
                            }
                        }
                        let mut a = vec![FMT_COLOR0];
                        for c in 0..4u32 {
                            let f = m.f32(base.wrapping_add(c * 4));
                            a.extend_from_slice(&dbl32(f));
                        }
                        trace(m, o, &a);
                        for c in 0..4u32 {
                            if nkeys > 0 && !animated {
                                let slot = cur.wrapping_add(0x2c).wrapping_add(c * 4);
                                let fc = if kind6(m) {
                                    o.call(m, SI_FCURVE_ALLOCATE, &[0x23a, udf]).eax
                                } else {
                                    o.call(m, SI_FCURVE_ALLOCATE, &[0, cur]).eax
                                };
                                m.set_u32(slot, fc);
                                let fc = m.u32(slot);
                                m.set_u16(fc.wrapping_add(0x1e), 2);
                                let t0 = m.u32(times);
                                let val = m.u32(base.wrapping_add(c * 4));
                                let key = o.call(m, SI_FKEY_CREATE, &[t0, val, 0, 0, 2]).eax;
                                let fc = m.u32(slot);
                                o.call(m, SI_FCURVE_ADD_KEY, &[fc, key]);
                            }
                        }
                        let mut k: i32 = 1;
                        while k < nkeys {
                            for c in 0..4u32 {
                                let at = base.wrapping_add(c * 4);
                                let v = tok_ws(m, 0, &mut saved);
                                if v == 0 {
                                    let nm = m.u32(cur);
                                    key_time_error(m, o, FMT_KEYTIME_SYMBOL2, KEYTIME_SYMBOL_TEXT, k as u32, if nm == 0 { NULL_STR4 } else { nm });
                                    return 0;
                                }
                                let f = atof32(m, v);
                                m.set_f32(at, f);
                                if !animated {
                                    let tk = m.u32(times.wrapping_add((k as u32).wrapping_mul(4)));
                                    let val = m.u32(at);
                                    let key = o.call(m, SI_FKEY_CREATE, &[tk, val, 0, 0, 2]).eax;
                                    let slot = cur.wrapping_add(0x2c).wrapping_add(c * 4);
                                    let fc = m.u32(slot);
                                    o.call(m, SI_FCURVE_ADD_KEY, &[fc, key]);
                                }
                            }
                            let mut a = vec![FMT_COLORN, k as u32];
                            for c in 0..4u32 {
                                let f = m.f32(base.wrapping_add(c * 4));
                                a.extend_from_slice(&dbl32(f));
                            }
                            trace(m, o, &a);
                            k += 1;
                        }
                        stop = true;
                    }
                    _ => {}
                }
                if stop {
                    exhausted = false;
                    break;
                }
            }
            cur = m.u32(cur.wrapping_add(0x44));
        }
        if exhausted && cur == 0 && keep_const == 1 && !found {
            // a name the dialog does not know: kept as a constant
            trace(m, o, &[FMT_FOUND_CONST, tok]);
            let node = m.si_calloc(1, 0x50);
            if consts != 0 {
                m.set_u32(node.wrapping_add(0x44), consts);
            }
            consts = node;
            let d = clib::strdup(m, tok);
            m.set_u32(node, d);
            let v = tok_crlf(m, 0, &mut saved);
            trace(m, o, &[FMT_CONST_VALUE, if v == 0 { NULL_STR6 } else { v }]);
            if v != 0 {
                let d = clib::strdup(m, v);
                m.set_u32(node.wrapping_add(0x18), d);
            }
            m.set_u16(node.wrapping_add(4), 0);
            m.set_u32(node.wrapping_add(8), 0xffff_ffff);
        }
        let _ = (FMT_FOUND_CONST, NULL_STR5);
        idx += 1;
        continue 'lines;
    }
    if !kind6(m) {
        let id = m.u32(dlg.wrapping_add(4));
        let fr = m.u32(SI_FRAME_CURRENT);
        o.call(m, SI_CUS_DIALOG_SET, &[id, dlg.wrapping_add(0x10), fr]);
    }
    if times != 0 {
        m.free(times);
    }
    release_lines(m, lines, nlines);
    consts
}

/// [soft 0x085240ac SI_cusDialogReadData]: reads the `DATA` section from the file `fp` (SI_cusReadDataSection
/// answers the text) and parses it into `dlg` (no effect, no constants kept); always 1.
pub fn si_cus_dialog_read_data(m: &mut Mem, o: &mut dyn Outside, dlg: u32, fp: u32) -> i32 {
    let text = o.call(m, SI_CUS_READ_DATA_SECTION_P, &[fp]).eax;
    if text != 0 {
        o.call(m, SI_CUS_PARSE_DATA_STRING_P, &[0, dlg, text, 0]);
        m.free(text);
    }
    1
}

// ---------------------------------------------------------------------------------------------------------
// The dialog item callback lists (`custom_dialog.c`): an effect's record keeps, at +0xb0, a table whose word 0
// points to one list head per dialog item; +4 is the item being called and +8 the node being called.
// A node is 12 bytes: function, user data, next.

pub const SI_CUS_CONTEXT_INIT_P: Routine = Routine::new("SI_cusContextInit", 0x0854_b1e0);
pub const SI_CUS_SET_CURRENT_DSO_EFFECT_P: Routine = Routine::new("SI_cusSetCurrentDsoEffect", 0x0852_e84c);
/// The program's `updateMode` (a short).
pub const UPDATE_MODE: u32 = 0x086d_c2f4;

/// The table, the list-head array and the item count the four list functions need, or None for the error 0x10.
fn cb_lists(m: &Mem, eff: u32, item: i32) -> Option<(u32, u32)> {
    let dlg = m.u32(eff.wrapping_add(4));
    if dlg == 0 {
        return None;
    }
    let tab = m.u32(eff.wrapping_add(0xb0));
    if tab == 0 {
        return None;
    }
    let heads = m.u32(tab);
    if heads == 0 || item <= 0 || item > m.u16(dlg.wrapping_add(0xc)) as i16 as i32 {
        return None;
    }
    Some((tab, heads))
}

/// [soft 0x08527650 SI_cusDialogItemCallbackAdd]: appends (`func`, `data`) to the callback list of dialog item
/// `item` (1-based) of effect `eff`; 0x10 for a bad effect or item or a null function, 0x46 when the node cannot be
/// allocated, else 0. The node becomes the one to call when it is the first of the item being called.
pub fn si_cus_dialog_item_callback_add(m: &mut Mem, eff: u32, item: i32, func: u32, data: u32) -> i32 {
    let (tab, heads) = match cb_lists(m, eff, item) {
        Some(t) if func != 0 => t,
        _ => return 0x10,
    };
    let slot = heads.wrapping_add((item as u32).wrapping_mul(4)).wrapping_sub(4);
    let mut tail = m.u32(slot);
    while tail != 0 && m.u32(tail.wrapping_add(8)) != 0 {
        tail = m.u32(tail.wrapping_add(8));
    }
    let node = m.si_malloc(0xc);
    if node == 0 {
        return 0x46;
    }
    m.set_u32(node, func);
    m.set_u32(node.wrapping_add(4), data);
    m.set_u32(node.wrapping_add(8), 0);
    if tail == 0 {
        m.set_u32(slot, node);
    } else {
        m.set_u32(tail.wrapping_add(8), node);
    }
    if m.u32(tab.wrapping_add(4)) == item as u32 && m.u32(tab.wrapping_add(8)) == 0 {
        m.set_u32(tab.wrapping_add(8), node);
    }
    0
}

/// [soft 0x08527700 SI_cusDialogItemCallbackRemove]: unlinks and frees the first node of the item's list with this
/// function and data (no node, no change); 0x10 for a bad effect or item or a null function, else 0.
pub fn si_cus_dialog_item_callback_remove(m: &mut Mem, eff: u32, item: i32, func: u32, data: u32) -> i32 {
    let (tab, heads) = match cb_lists(m, eff, item) {
        Some(t) if func != 0 => t,
        _ => return 0x10,
    };
    let slot = heads.wrapping_add((item as u32).wrapping_mul(4)).wrapping_sub(4);
    let mut prev = 0u32;
    let mut node = m.u32(slot);
    while node != 0 && !(m.u32(node) == func && m.u32(node.wrapping_add(4)) == data) {
        prev = node;
        node = m.u32(node.wrapping_add(8));
    }
    if node == 0 {
        return 0;
    }
    let next = m.u32(node.wrapping_add(8));
    if prev == 0 {
        m.set_u32(slot, next);
    } else {
        m.set_u32(prev.wrapping_add(8), next);
    }
    if m.u32(tab.wrapping_add(4)) == item as u32 && m.u32(tab.wrapping_add(8)) == node {
        m.set_u32(tab.wrapping_add(8), next);
    }
    m.free(node);
    0
}

/// [soft 0x085277b4 SI_cusDialogItemCallbackRemoveAll]: frees every node of the item's list; 0x10 for a bad effect
/// or item, else 0.
pub fn si_cus_dialog_item_callback_remove_all(m: &mut Mem, eff: u32, item: i32) -> i32 {
    let (tab, heads) = match cb_lists(m, eff, item) {
        Some(t) => t,
        None => return 0x10,
    };
    let slot = heads.wrapping_add((item as u32).wrapping_mul(4)).wrapping_sub(4);
    let mut node = m.u32(slot);
    while node != 0 {
        let next = m.u32(node.wrapping_add(8));
        m.free(node);
        node = next;
    }
    m.set_u32(slot, 0);
    if m.u32(tab.wrapping_add(4)) == item as u32 {
        m.set_u32(tab.wrapping_add(8), 0);
    }
    0
}

/// [soft 0x08527838 SI_cusDialogGetDirtyFlag]: the short at +0xc of the effect's callback table, 0 without one.
pub fn si_cus_dialog_get_dirty_flag(m: &Mem, eff: u32) -> i32 {
    if eff == 0 {
        return 0;
    }
    let tab = m.u32(eff.wrapping_add(0xb0));
    if tab == 0 { 0 } else { m.u16(tab.wrapping_add(0xc)) as i16 as i32 }
}

/// [soft 0x08527858 SI_cusDialogSetDirtyFlag]: stores the low half of `v` there (nothing without a table).
pub fn si_cus_dialog_set_dirty_flag(m: &mut Mem, eff: u32, v: u32) {
    if eff != 0 {
        let tab = m.u32(eff.wrapping_add(0xb0));
        if tab != 0 {
            m.set_u16(tab.wrapping_add(0xc), v as u16);
        }
    }
}

/// [soft 0x085274fc si_cusDialogItemCallbackCallAll]: calls the callbacks of dialog item `item` of effect `eff`
/// (with a context made by `SI_cusContextInit`, the effect set as the current DSO effect meanwhile) until the list
/// ends or one answers 0x5f or 0x7c, which is the result (6 for a node without function; 0x10 for a bad effect
/// or item). `updateMode` is restored; the list position is reset on the way out.
pub fn si_cus_dialog_item_callback_call_all(m: &mut Mem, o: &mut dyn Outside, eff: u32, item: i32, flag: u32) -> i32 {
    let flag = flag as u16;
    let mut ret: i32 = 0;
    let saved_mode = m.u16(UPDATE_MODE);
    let t104 = if eff != 0 { m.u32(eff.wrapping_add(0x104)) } else { 0 };
    let ok = eff != 0 && t104 != 0 && m.u32(t104) != 0 && m.u32(eff.wrapping_add(0x14)) != 0 && m.u32(eff.wrapping_add(0xc0)) != 0;
    let (tab, heads) = if ok {
        let tab = m.u32(eff.wrapping_add(0xc0));
        (tab, m.u32(tab))
    } else {
        (0, 0)
    };
    let dlg = if ok { m.u32(eff.wrapping_add(0x14)) } else { 0 };
    if !ok || heads == 0 || item <= 0 || item > m.u16(dlg.wrapping_add(0xc)) as i16 as i32 {
        return 0x10;
    }
    let first = m.u32(heads.wrapping_add((item as u32).wrapping_mul(4)).wrapping_sub(4));
    m.set_u32(tab.wrapping_add(8), first);
    m.set_u32(tab.wrapping_add(4), item as u32);
    if first != 0 {
        let ctx = m.local(0x20);
        loop {
            let node = m.u32(tab.wrapping_add(8));
            if m.u32(node) == 0 {
                ret = 6;
            } else {
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT_P, &[eff]);
                crate::custom_util::si_cus_context_init(m, ctx, eff, if flag != 0 { 7 } else { 6 }, if flag != 0 { 0xc } else { 0xb }, 0xffff_ffff);
                m.set_u32(tab.wrapping_add(8), m.u32(node.wrapping_add(8)));
                let r = o.call(m, Routine::at(m.u32(node)), &[ctx, item as u32, m.u32(node.wrapping_add(4))]).eax as i32;
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT_P, &[0]);
                if r == 0x5f || r == 0x7c {
                    ret = r;
                }
            }
            let next = m.u32(tab.wrapping_add(8));
            if next == 0 || ret == 0x5f || ret == 0x7c {
                break;
            }
        }
        m.release(0x20);
    }
    m.set_u16(UPDATE_MODE, saved_mode);
    m.set_u32(tab.wrapping_add(8), 0);
    m.set_u32(tab.wrapping_add(4), 0xffff_ffff);
    ret
}

// ---------------------------------------------------------------------------------------------------------
// The comparison of dialog values with their animation (`custom_dialog.c`).

const SI_FRAME_RATE_ADDR: u32 = 0x0878_204c;
const FMT_F_1: u32 = 0x086d_cd6b;
const FMT_F_2: u32 = 0x086d_cd6e;
/// `SI_fcurveFrameValue` (a float in st0).
const SI_FCURVE_FRAME_VALUE_P: Routine = Routine::new("SI_fcurveFrameValue", 0x0820_0258);

/// [soft 0x08527c44 findDiffValue]: whether the value of the fcurve `val[0x2c + comp*4]` at `frame` prints
/// differently (`%f`) from the float `want`: 1 if it differs, else 0. `nkeys` keys of the curve are first scanned
/// for the one at or after `frame` (rounded half away from zero), which changes nothing (its reads can fault).
pub fn find_diff_value(m: &mut Mem, o: &mut dyn Outside, val: u32, want: u32, nkeys: i32, frame: i32, comp: u32) -> u32 {
    let curve = m.u32(val.wrapping_add(0x2c).wrapping_add(comp.wrapping_mul(4)));
    if nkeys > 0 {
        // x87: the product and the +-0.5 are formed in 80 bits, then truncated by `fistp` (RC = 11)
        let rate = si_x87::F80::from_f32(f32::from_bits(m.u32(SI_FRAME_RATE_ADDR)));
        for i in 0..nkeys as u32 {
            let keys = m.u32(curve.wrapping_add(8));
            let t = f32::from_bits(m.u32(keys.wrapping_add(i.wrapping_mul(0x24))));
            let tt = si_x87::F80::from_f32(t);
            let half = if si_x87::F80::ZERO.lt(tt) { 0.5f64 } else { -0.5f64 };
            let sum = rate * tt + half;
            let r = sum.to_i32(si_x87::Round::Zero);
            if r >= frame {
                break;
            }
        }
    }
    let r = o.call(m, SI_FCURVE_FRAME_VALUE_P, &[curve, frame as u32]);
    let got = r.st0.map(crate::custom_enc::st0_f32).unwrap_or(f32::NAN);
    let b1 = (f32::from_bits(want) as f64).to_bits();
    let b2 = (got as f64).to_bits();
    let buf = m.local(0x50);
    cfmt::sprintf(m, buf, FMT_F_1, &[b1 as u32, (b1 >> 32) as u32]);
    cfmt::sprintf(m, buf.wrapping_add(0x28), FMT_F_2, &[b2 as u32, (b2 >> 32) as u32]);
    let same = clib::strcmp(m, buf, buf.wrapping_add(0x28)) == 0;
    m.release(0x50);
    if same { 0 } else { 1 }
}

/// [soft 0x08527b5c areValuesSame]: whether every keyable value of the list from `a` that is in dialog `dlg` and
/// has an fcurve prints (`%f`) the same as its curve at `frame`: 0 at the first difference, else 1. (The colour
/// components compared are those of `a`, the first value, whatever value is being looked at.)
pub fn are_values_same(m: &mut Mem, o: &mut dyn Outside, a: u32, dlg: u32, frame: i32) -> u32 {
    let mut v = a;
    loop {
        v = cusval_get_next_keyable_value(m, v, m.u32(dlg.wrapping_add(0x14)));
        if v == 0 {
            return 1;
        }
        let inside = o.call(m, crate::custom_init::SI_CUS_VALUE_IN_DIALOG, &[dlg, v]).eax as u16;
        if inside == 0 {
            continue;
        }
        let ty = m.u16(v.wrapping_add(4));
        if ty == 1 {
            let fc = m.u32(v.wrapping_add(0x2c));
            if fc != 0 {
                let n = m.u32(fc.wrapping_add(0xc)) as i32;
                if find_diff_value(m, o, v, m.u32(v.wrapping_add(0x10)), n, frame, 0) as u16 == 1 {
                    return 0;
                }
            }
        } else if ty == 4 {
            for k in 0..4u32 {
                let fc = m.u32(v.wrapping_add(0x2c).wrapping_add(k * 4));
                if fc != 0 {
                    let n = m.u32(fc.wrapping_add(0xc)) as i32;
                    if find_diff_value(m, o, v, m.u32(a.wrapping_add(0x1c).wrapping_add(k * 4)), n, frame, k) as u16 == 1 {
                        return 0;
                    }
                }
            }
        }
    }
}

const CB_FATAL_TEXT: &[u8] = b": Custom Value callback reported fatal error.  Disabling effect.";

/// [soft 0x0852980c SI_customValueCallbackCallAll]: calls the callbacks of custom value `val` of effect `eff`
/// (each with a context made by `SI_cusContextInit`, kind 8, `flag`, and the callback's own name and data in
/// it) until the list ends or a node is unusable (its effect missing: result 6). A callback answering 0x5f is
/// reported with a status message and, like one answering 0x7c, disables its effect (+0x100 = 0, short +0x118
/// = 1); an effect already disabled is skipped. 0x10 for a bad effect or value; 0 at once for a disabled `eff`.
pub fn si_custom_value_callback_call_all(m: &mut Mem, o: &mut dyn Outside, eff: u32, val: u32, flag: u32) -> i32 {
    let t104 = if eff != 0 { m.u32(eff.wrapping_add(0x104)) } else { 0 };
    if eff == 0 || t104 == 0 || m.u32(t104) == 0 || val == 0 {
        return 0x10;
    }
    if m.u16(eff.wrapping_add(0x118)) == 1 {
        return 0;
    }
    let first = m.u32(val.wrapping_add(0x3c));
    m.set_u32(val.wrapping_add(0x40), first);
    let mut ret: i32 = 0;
    if first != 0 {
        let ctx = m.local(0x20);
        loop {
            let node = m.u32(val.wrapping_add(0x40));
            let owner = m.u32(node.wrapping_add(8));
            if m.u32(node.wrapping_add(0xc)) == 0 || owner == 0 {
                ret = 6;
            } else if m.u16(owner.wrapping_add(0x118)) != 1 {
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT_P, &[eff]);
                crate::custom_util::si_cus_context_init(m, ctx, owner, 8, flag, 0xffff_ffff);
                m.set_u32(ctx.wrapping_add(0xc), m.u32(node));
                m.set_u32(ctx.wrapping_add(0x10), m.u32(node.wrapping_add(4)));
                m.set_u32(val.wrapping_add(0x40), m.u32(node.wrapping_add(0x14)));
                let r = o.call(m, Routine::at(m.u32(node.wrapping_add(0xc))), &[ctx, m.u32(val), m.u32(node.wrapping_add(0x10))]).eax;
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT_P, &[0]);
                if r == 0x5f || r == 0x7c {
                    if r == 0x5f {
                        let name = m.u32(eff.wrapping_add(4));
                        let len = clib::strlen(m, name);
                        let buf = m.si_malloc(len.wrapping_add(0x41));
                        if buf != 0 {
                            let mut text = m.cstr(name).to_vec();
                            text.extend_from_slice(CB_FATAL_TEXT);
                            m.put_cstr(buf, &text);
                            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[buf, 3]);
                            m.free(buf);
                        }
                    }
                    let owner = m.u32(node.wrapping_add(8));
                    m.set_u32(owner.wrapping_add(0x100), 0);
                    m.set_u16(owner.wrapping_add(0x118), 1);
                }
            }
            if m.u32(val.wrapping_add(0x40)) == 0 || ret != 0 {
                break;
            }
        }
        m.release(0x20);
    }
    m.set_u32(val.wrapping_add(0x40), 0);
    ret
}

// ---------------------------------------------------------------------------------------------------------
// The small run-time pieces of `custom_dso.c`.

const CUSTOM_INIT_DATA: u32 = 0x086d_d0a0;
const CUS_DSO_REFRESH_FLAG: u32 = 0x086d_d0a4;
const CURRENT_DSO_EFFECT: u32 = 0x086d_d0a8;

/// [soft 0x0852e808 SI_customInitDataSet]: stores the word.
pub fn si_custom_init_data_set(m: &mut Mem, v: u32) {
    m.set_u32(CUSTOM_INIT_DATA, v);
}
/// [soft 0x0852e818 SI_customInitDataGet]: the stored word.
pub fn si_custom_init_data_get(m: &Mem) -> u32 {
    m.u32(CUSTOM_INIT_DATA)
}
/// [soft 0x0852e830 SI_cusDsoSetRefreshFlag]: stores the word.
pub fn si_cus_dso_set_refresh_flag(m: &mut Mem, v: u32) {
    m.set_u32(CUS_DSO_REFRESH_FLAG, v);
}
/// [soft 0x0852e824 SI_cusDsoGetRefreshFlag]: the stored word.
pub fn si_cus_dso_get_refresh_flag(m: &Mem) -> u32 {
    m.u32(CUS_DSO_REFRESH_FLAG)
}
/// [soft 0x0852e840 SI_cusGetCurrentDsoEffect]: the effect `SI_cusSetCurrentDsoEffect` stored.
pub fn si_cus_get_current_dso_effect(m: &Mem) -> u32 {
    m.u32(CURRENT_DSO_EFFECT)
}

/// [soft 0x0852e3c0 SI_cusRelDataSetDscId]: replaces the text at +4 of the relation record by a new 8-byte one
/// holding `id` in decimal; 1, or 0 for no record or no memory (the old text is gone then).
pub fn si_cus_rel_data_set_dsc_id(m: &mut Mem, rd: u32, id: i32) -> i32 {
    if rd == 0 {
        return 0;
    }
    let old = m.u32(rd.wrapping_add(4));
    if old != 0 {
        m.free(old);
        m.set_u32(rd.wrapping_add(4), 0);
    }
    let p = m.si_malloc(8);
    m.set_u32(rd.wrapping_add(4), p);
    if p == 0 {
        return 0;
    }
    m.put_cstr(p, id.to_string().as_bytes());
    1
}

/// [soft 0x0852e420 cusInitCustomElemArray]: sets `n` records of 0x14 bytes to (-1, -1, -1, -1, 0); 0 without
/// array or for a negative count, else 1.
pub fn cus_init_custom_elem_array(m: &mut Mem, n: i32, arr: u32) -> i32 {
    if arr == 0 || n < 0 {
        return 0;
    }
    let mut p = arr;
    for _ in 0..n {
        for k in 0..4u32 {
            m.set_u32(p.wrapping_add(k * 4), 0xffff_ffff);
        }
        m.set_u32(p.wrapping_add(0x10), 0);
        p = p.wrapping_add(0x14);
    }
    1
}

/// [soft 0x0852a674 symbolAlreadyRead]: whether the first of the `n` 0x108-byte records of `arr` whose first
/// word is `key` has its short at +0x104 equal to 1 (0 when there is none).
pub fn symbol_already_read(m: &Mem, n: i32, arr: u32, key: u32) -> i32 {
    if arr == 0 {
        return 0;
    }
    for i in 0..n.max(0) as u32 {
        let rec = arr.wrapping_add(i.wrapping_mul(0x108));
        if m.u32(rec) == key {
            return (m.u16(rec.wrapping_add(0x104)) == 1) as i32;
        }
    }
    0
}

/// [soft 0x0852de40 cusUpdNodeGetEffect]: the custom effect of the object that the last entry (index +0x18, array
/// +0x1c, 20-byte entries) of an update node names: 0 unless the object is a custom effect's icon (+0x2ec = 0xd).
pub fn cus_upd_node_get_effect(m: &mut Mem, o: &mut dyn Outside, node: u32) -> u32 {
    let idx = m.u16(node.wrapping_add(0x18)) as i16 as i32 as u32;
    let ent = m.u32(node.wrapping_add(0x1c)).wrapping_add(idx.wrapping_mul(0x14));
    let (a, b) = (m.u16(ent.wrapping_sub(0x14)) as i16 as i32 as u32, m.u32(ent.wrapping_sub(0x10)));
    let dict = crate::custom_fcv::elem_dicts(m);
    let el = o.call(m, crate::custom_fcv::SI_ELEMDICT_GET_ELEM, &[dict, a, b]).eax;
    let ob = m.u32(el.wrapping_add(4));
    if ob != 0 && m.u32(ob.wrapping_add(0x2ec)) == 0xd {
        let out = m.local(4);
        let r = o.call(m, crate::custom_io::SI_CUS_GET_EFFECT_BY_ICON, &[ob, out]).eax;
        m.release(4);
        r
    } else {
        0
    }
}
