// SPDX-License-Identifier: MIT
//! `custom_shader.c`, the loader: `shaderLoad` reads a shader's definition file section by
//! section (`_FILETYPE`, `_DESCRIPTION`, `_DIALOG_TEMPLATE`, `_SHADER_PREVIEWER`,
//! `_SHADER_LINKS`, `_SHADER_DECLARE`, `_SHADER_OPTIONS`, `_SHADER_SPDL`) and hands each to a
//! `shaderRead*` routine (finding 0041, "fifth continuation").
//!
//! Every reader takes the open file and the shader record, reads lines with `fgets(buf, 0xc8)`
//! into a 0xcc-byte local (byte 0xc8 is cleared), splits them with `strtok` on the token
//! `tokWHITESPACES`, skips empty and `#` lines and compares the first token, as a prefix of the
//! table's keyword (`strncmp` over the keyword's length). Files and the C library go through
//! the outside table; `strtok`'s own position is a local (`saved`).

use crate::cfmt;
use crate::custom_gen::{ACCESS, FERROR, SI_CUS_TRACE, SI_DB_DIR_SEARCH_DATABASE_FROM_NAME};
use crate::custom_shader::*;
use crate::shdb_init::SI_STATUS_DISPLAY_MESSAGE;
use crate::strlist::si_str_append;
use si_mem::{clib, Mem, Outside, Routine};

pub const FOPEN: Routine = Routine::new("fopen", 0x080c_4e18);
pub const FCLOSE: Routine = Routine::new("fclose", 0x080c_4958);
pub const FGETS: Routine = Routine::new("fgets", 0x080c_4388);
pub const SI_DBFILE_READ_ASC_HEADER: Routine = Routine::new("SI_dbfileReadAscHeader", 0x083f_b788);
pub const SI_CUS_READ_DESCRIPTION: Routine = Routine::new("si_cusReadDescription", 0x0853_7428);
pub const SI_CUS_DIALOG_READ: Routine = Routine::new("si_cusDialogRead", 0x0851_fdd0);
pub const SI_CUS_READ_DATA_SECTION: Routine = Routine::new("SI_cusReadDataSection", 0x0852_40f0);
pub const SI_CUS_PARSE_DATA_STRING: Routine = Routine::new("SI_cusParseDataString", 0x0852_487c);
pub const SI_STR_GUID_FROM_STRING: Routine = Routine::new("SI_strGuidFromString", 0x083c_faec);

/// The database directory handle (`SI_dbdir`).
const DB_DIR: u32 = 0x0878_1390;
/// `si_bIsDebug` (a short).
const IS_DEBUG: u32 = 0x086e_3820;
/// The keyword variables (pointers to the keyword strings) of the section readers.
const TOK_WHITESPACES: u32 = 0x086e_2208;
const TOK_FILETYPE: u32 = 0x086e_2218;
const TOK_DESCRIPTION: u32 = 0x086e_222c;
const TOK_DIALOGTMPL: u32 = 0x086e_2244;
const TOK_SHDLINKS: u32 = 0x086e_2258;
const TOK_SHDDECLARE: u32 = 0x086e_226c;
const TOK_SHDOPTIONS: u32 = 0x086e_2280;
const TOK_SHDBPARMS: u32 = 0x086e_2298;
const TOK_SHDSPDL: u32 = 0x086e_22ac;
const TOK_END: u32 = 0x086e_22b8;
const TOK_VERSION: u32 = 0x086e_22c4;
const TOK_MR19: u32 = 0x086e_22cc;
const TOK_MR20: u32 = 0x086e_22d4;
const TOK_NAME: u32 = 0x086e_22e0;
const TOK_DEFAULT: u32 = 0x086e_22ec;
const TOK_DEBUG: u32 = 0x086e_22f8;
const TOK_ON: u32 = 0x086e_2300;
const TOK_OFF: u32 = 0x086e_2308;
const TOK_NORMAL: u32 = 0x086e_2314;
const TOK_DEPTH: u32 = 0x086e_2320;
const TOK_RGBA16: u32 = 0x086e_232c;
const TOK_RGBAFP: u32 = 0x086e_2338;
const TOK_LABEL: u32 = 0x086e_2344;
const TOK_SCANLINE: u32 = 0x086e_2354;
const TOK_GUID: u32 = 0x086e_2360;
/// The keyword table `shaderToken`: 0x1d entries of 0x20 bytes (+0 kind, +4 keyword, +0x10 flag
/// word: 0 for the link statements, 1 for the declare statements).
const SHADER_TOKEN: u32 = 0x086e_2480;

const LINE: u32 = 0xc8;

fn trace(m: &mut Mem, o: &mut dyn Outside, args: &[u32]) {
    o.call(m, SI_CUS_TRACE, args);
}

fn sprintf(m: &mut Mem, buf: u32, fmt: u32, args: &[u32]) {
    let t = cfmt::format(m, fmt, args);
    m.put_cstr(buf, &t);
}

/// `strncmp(tok, <keyword variable>, strlen(keyword)) == 0`.
fn is_tok(m: &Mem, tok: u32, var: u32) -> bool {
    let k = m.u32(var);
    clib::strncmp(m, tok, k, clib::strlen(m, k)) == 0
}

/// The guard every reader starts with: a file that is not null, without an error, and a shader.
fn guard(m: &mut Mem, o: &mut dyn Outside, f: u32, sh: u32) -> bool {
    sh != 0 && f != 0 && o.call(m, FERROR, &[f]).eax == 0
}

/// Reads the next line into `buf`; false at the end of the file.
fn next_line(m: &mut Mem, o: &mut dyn Outside, f: u32, buf: u32) -> bool {
    if o.call(m, FGETS, &[buf, LINE, f]).eax == 0 {
        return false;
    }
    m.set_u8(buf + LINE, 0);
    true
}

/// The next significant token of a line (null for an empty line or a `#` comment).
fn first_token(m: &mut Mem, buf: u32, saved: &mut u32) -> u32 {
    let ws = m.u32(TOK_WHITESPACES);
    let t = clib::strtok(m, buf, ws, saved);
    if t == 0 || m.u8(t) == b'#' {
        0
    } else {
        t
    }
}

/// [soft 0x08540898 shaderLoad]: opens `<database directory>/<name>` (the shader's name at +0x1c,
/// the directory of its second name at +0x20 found in the database directory) and reads its
/// sections. Answers the result of the last section read (a short); 0 on any error. At the end
/// of the file the shader's flag short at +4 is set when the last section read answered
/// non-zero, otherwise the "EOF reached without section" trace is made.
pub fn si_shader_load(m: &mut Mem, o: &mut dyn Outside, sh: u32) -> i32 {
    let mut ok: i16 = 0;
    if sh == 0 || m.u32(sh + 0x1c) == 0 || m.u32(sh) == 0 {
        return 0;
    }
    let name2 = m.u32(sh + 0x20);
    if name2 == 0 {
        trace(m, o, &[0x086e_2980]);
        return 0;
    }
    let dbp = m.local(8);
    let msg = m.local(0x400);
    let path = m.local(0x800);
    let buf = m.local(0xcc);
    m.set_u32(dbp, 0);
    let r = shader_load_file(m, o, sh, name2, dbp, msg, path, buf, &mut ok);
    m.release(0xcc);
    m.release(0x800);
    m.release(0x400);
    m.release(8);
    r.unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn shader_load_file(m: &mut Mem, o: &mut dyn Outside, sh: u32, name2: u32, dbp: u32, msg: u32, path: u32, buf: u32, ok: &mut i16) -> Option<i32> {
    trace(m, o, &[0x086e_28d0, name2]);
    let r = o.call(m, SI_DB_DIR_SEARCH_DATABASE_FROM_NAME, &[DB_DIR, name2, dbp, dbp + 4]).eax;
    let d = m.u32(dbp);
    if r != 0x64 || d == 0 || m.u32(d + 0x24) != 2 {
        trace(m, o, &[0x086e_2900]);
        return Some(0);
    }
    if m.u32(d + 0xc) == 0 {
        trace(m, o, &[0x086e_2940]);
        return Some(0);
    }
    sprintf(m, path, 0x086e_296c, &[m.u32(d + 0xc), m.u32(sh + 0x1c)]);
    if o.call(m, ACCESS, &[path, 4]).eax != 0 {
        sprintf(m, msg, 0x086e_29c0, &[path]);
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[msg, 3]);
        trace(m, o, &[msg]);
        return Some(0);
    }
    let f = o.call(m, FOPEN, &[path, 0x086e_29e7]).eax;
    if f == 0 {
        sprintf(m, msg, 0x086e_2a00, &[path]);
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[msg, 3]);
        trace(m, o, &[msg]);
        return Some(0);
    }
    let hdr = m.local(4);
    o.call(m, SI_DBFILE_READ_ASC_HEADER, &[f, hdr, 0]);
    m.release(4);
    m.set_u16(IS_DEBUG, 0);
    loop {
        if o.call(m, FGETS, &[buf, LINE, f]).eax == 0 {
            if *ok == 0 {
                trace(m, o, &[0x086e_2a40]);
            } else {
                m.set_u16(sh + 4, 1);
            }
            o.call(m, FCLOSE, &[f]);
            return Some(*ok as i32);
        }
        let res = if is_tok(m, buf, TOK_FILETYPE) {
            shader_read_header(m, o, f, sh)
        } else if is_tok(m, buf, TOK_DESCRIPTION) {
            o.call(m, SI_CUS_READ_DESCRIPTION, &[f, sh + 0x24]).eax as i16 as i32
        } else if is_tok(m, buf, TOK_DIALOGTMPL) {
            shader_read_dialog(m, o, f, sh)
        } else if is_tok(m, buf, TOK_SHDBPARMS) {
            shader_read_previewer(m, o, f, sh)
        } else if is_tok(m, buf, TOK_SHDLINKS) {
            shader_read_links(m, o, f, sh)
        } else if is_tok(m, buf, TOK_SHDDECLARE) {
            shader_read_declare(m, o, f, sh)
        } else if is_tok(m, buf, TOK_SHDOPTIONS) {
            shader_read_options(m, o, f, sh)
        } else if is_tok(m, buf, TOK_SHDSPDL) {
            shader_read_spdl(m, o, f, sh)
        } else {
            continue;
        };
        *ok = res as i16;
    }
}

/// [soft 0x08540de0 shaderReadHeader]: the `_FILETYPE` section. `DEFAULT [OFF]` sets the short at
/// +0x18 (ON when no value), `DEBUG` sets `si_bIsDebug`, `VERSION` takes the version names
/// `1.9` (bit 0x20 of +0x10) and `2.X` (bit 0x40); `NAME` is recognised and ignored. The end line
/// sets 0x20 unless 0x40 is set. 1 at `_END`, 0 at the end of the file.
pub fn shader_read_header(m: &mut Mem, o: &mut dyn Outside, f: u32, sh: u32) -> i32 {
    if !guard(m, o, f, sh) {
        return 0;
    }
    trace(m, o, &[0x086e_2a66]);
    let b = m.u8(sh + 0x10) & 0x9f;
    m.set_u8(sh + 0x10, b);
    let buf = m.local(0xcc);
    let r = read_header_lines(m, o, f, sh, buf);
    m.release(0xcc);
    r
}

fn read_header_lines(m: &mut Mem, o: &mut dyn Outside, f: u32, sh: u32, buf: u32) -> i32 {
    let ws = m.u32(TOK_WHITESPACES);
    let mut saved = 0u32;
    loop {
        if !next_line(m, o, f, buf) {
            trace(m, o, &[0x086e_2a80]);
            return 0;
        }
        let tok = first_token(m, buf, &mut saved);
        if tok == 0 {
            continue;
        }
        trace(m, o, &[0x086e_2aa2, tok]);
        if is_tok(m, tok, TOK_END) {
            let w = m.u32(sh + 0x10);
            if w & 0x40 == 0 {
                m.set_u32(sh + 0x10, w | 0x20);
            }
            return 1;
        }
        if is_tok(m, tok, TOK_DEFAULT) {
            let t = clib::strtok(m, 0, ws, &mut saved);
            if t == 0 || m.u8(t) == b'#' {
                trace(m, o, &[0x086e_2ab1]);
                m.set_u16(sh + 0x18, 1);
            } else {
                m.set_u16(sh + 0x18, if is_tok(m, t, TOK_OFF) { 0 } else { 1 });
                trace(m, o, &[0x086e_2ac8, m.u16(sh + 0x18) as i16 as i32 as u32]);
            }
        } else if is_tok(m, tok, TOK_DEBUG) {
            m.set_u16(IS_DEBUG, 1);
            trace(m, o, &[0x086e_2ada]);
        } else if is_tok(m, tok, TOK_VERSION) {
            trace(m, o, &[0x086e_2aec]);
            let mut t = clib::strtok(m, 0, ws, &mut saved);
            while t != 0 {
                if is_tok(m, t, TOK_MR19) {
                    let b = m.u8(sh + 0x10) | 0x20;
                    m.set_u8(sh + 0x10, b);
                } else if is_tok(m, t, TOK_MR20) {
                    let b = m.u8(sh + 0x10) | 0x40;
                    m.set_u8(sh + 0x10, b);
                }
                t = clib::strtok(m, 0, ws, &mut saved);
            }
        } else {
            // `NAME`: compared, the answer unused (the original falls through either way)
            let _ = is_tok(m, tok, TOK_NAME);
        }
    }
}

/// [soft 0x08541260 shaderReadDialog]: the `_DIALOG_TEMPLATE` section: the dialog is read into
/// the record at +0x30 (a recorded callee), its short at +0x30 set to 6 and its word at +0x34
/// copied from +0x3c; the data section, if any, is parsed into +0xe8. Answers the dialog
/// reader's short.
pub fn shader_read_dialog(m: &mut Mem, o: &mut dyn Outside, f: u32, sh: u32) -> i32 {
    if !guard(m, o, f, sh) {
        return 0;
    }
    trace(m, o, &[0x086e_2b03]);
    let ax = o.call(m, SI_CUS_DIALOG_READ, &[f, sh + 0x30, 0xffff_ffff]).eax as i16;
    let w = m.u32(sh + 0x3c);
    m.set_u32(sh + 0x34, w);
    m.set_u16(sh + 0x30, 6);
    let data = o.call(m, SI_CUS_READ_DATA_SECTION, &[f]).eax;
    if data != 0 {
        let p = o.call(m, SI_CUS_PARSE_DATA_STRING, &[0, sh + 0x30, data, 1]).eax;
        m.set_u32(sh + 0xe8, p);
        m.free(data);
    }
    if ax != 0 {
        trace(m, o, &[0x086e_2b16]);
    } else {
        trace(m, o, &[0x086e_2b40]);
    }
    ax as i32
}

/// [soft 0x08541324 shaderReadPreviewer]: the `_SHADER_PREVIEWER` section: every line up to the
/// one whose first non-blank text starts with `_END` is appended (whole, with its blanks) to a
/// text, which becomes the shader's at +8. 1 at `_END`, 0 at the end of the file (the text is
/// then leaked).
pub fn shader_read_previewer(m: &mut Mem, o: &mut dyn Outside, f: u32, sh: u32) -> i32 {
    let text = m.local(4);
    m.set_u32(text, 0);
    let ok = sh != 0 && f != 0 && o.call(m, FERROR, &[f]).eax == 0;
    if !ok {
        m.release(4);
        return 0;
    }
    trace(m, o, &[0x086e_2b5f]);
    let buf = m.local(0xcc);
    let ws = m.u32(TOK_WHITESPACES);
    let r = loop {
        if o.call(m, FGETS, &[buf, LINE, f]).eax == 0 {
            trace(m, o, &[0x086e_2b80]);
            break 0;
        }
        m.set_u8(buf + LINE, 0);
        let mut p = buf;
        while m.u8(p) != 0 && clib::strchr(m, ws, m.u8(p)) != 0 {
            p += 1;
        }
        if is_tok(m, p, TOK_END) {
            let t = m.u32(text);
            m.set_u32(sh + 8, t);
            break 1;
        }
        si_str_append(m, text, buf);
    };
    m.release(0xcc);
    m.release(4);
    r
}

/// [soft 0x08541870 readBoolOption]: the value of one option line, `ON` sets the bit in the word
/// at +0x10 of the shader, `OFF` clears it; anything else is traced and ignored. `name` is the
/// option's text for the trace.
pub fn read_bool_option(m: &mut Mem, o: &mut dyn Outside, sh: u32, name: u32, bit: u32, saved: &mut u32) {
    let ws = m.u32(TOK_WHITESPACES);
    let t = clib::strtok(m, 0, ws, saved);
    if t == 0 || m.u8(t) == b'#' {
        trace(m, o, &[0x086e_2c65, name]);
    } else if is_tok(m, t, TOK_ON) {
        let w = m.u32(sh + 0x10) | bit;
        m.set_u32(sh + 0x10, w);
        trace(m, o, &[0x086e_2c82, name]);
    } else if is_tok(m, t, TOK_OFF) {
        let w = m.u32(sh + 0x10) & !bit;
        m.set_u32(sh + 0x10, w);
        trace(m, o, &[0x086e_2c92, name]);
    } else {
        trace(m, o, &[0x086e_2ca3, name]);
    }
}

/// [soft 0x08541470 shaderReadOptions]: the `_SHADER_OPTIONS` section: `NORMAL` (bit 4), `DEPTH` (1),
/// `RGBA16` (8), `RGBAFP` (0x10), `LABEL` (2) and `SCANLINE` (0x10000) of the word at +0x10, each
/// followed by `ON` or `OFF`. 1 at `_END`, 0 at the end of the file.
pub fn shader_read_options(m: &mut Mem, o: &mut dyn Outside, f: u32, sh: u32) -> i32 {
    if !guard(m, o, f, sh) {
        return 0;
    }
    trace(m, o, &[0x086e_2ba2]);
    let buf = m.local(0xcc);
    let mut saved = 0u32;
    let r = loop {
        if !next_line(m, o, f, buf) {
            trace(m, o, &[0x086e_2bc0]);
            break 0;
        }
        let tok = first_token(m, buf, &mut saved);
        if tok == 0 {
            continue;
        }
        trace(m, o, &[0x086e_2be2, tok]);
        if is_tok(m, tok, TOK_END) {
            break 1;
        }
        let (name, bit) = if is_tok(m, tok, TOK_NORMAL) {
            (0x086e_2bf1, 4)
        } else if is_tok(m, tok, TOK_DEPTH) {
            (0x086e_2c01, 1)
        } else if is_tok(m, tok, TOK_RGBA16) {
            (0x086e_2c07, 8)
        } else if is_tok(m, tok, TOK_RGBAFP) {
            (0x086e_2c0e, 0x10)
        } else if is_tok(m, tok, TOK_LABEL) {
            (0x086e_2c22, 2)
        } else if is_tok(m, tok, TOK_SCANLINE) {
            (0x086e_2c2e, 0x10000)
        } else {
            trace(m, o, &[0x086e_2c40, tok]);
            continue;
        };
        read_bool_option(m, o, sh, name, bit, &mut saved);
    };
    m.release(0xcc);
    r
}

/// [soft 0x085419c4 shaderReadSPDL]: the `_SHADER_SPDL` section: `GUID <text>` (a text longer than
/// 0x25 characters is converted into the GUID at +0xec; a shorter one is ignored) and
/// `VERSION <n>` (decimal, into +0xfc); other tokens are traced. 1 at `_END`, 0 at the end of the file.
pub fn shader_read_spdl(m: &mut Mem, o: &mut dyn Outside, f: u32, sh: u32) -> i32 {
    if sh == 0 || f == 0 || o.call(m, FERROR, &[f]).eax != 0 {
        return 0;
    }
    trace(m, o, &[0x086e_2cbe]);
    let buf = m.local(0xcc);
    let ws = m.u32(TOK_WHITESPACES);
    let mut saved = 0u32;
    let r = loop {
        if !next_line(m, o, f, buf) {
            trace(m, o, &[0x086e_2ce0]);
            break 0;
        }
        let tok = first_token(m, buf, &mut saved);
        if tok == 0 {
            continue;
        }
        trace(m, o, &[0x086e_2d02, tok]);
        if is_tok(m, tok, TOK_END) {
            break 1;
        }
        if is_tok(m, tok, TOK_GUID) {
            trace(m, o, &[0x086e_2d11]);
            let t = clib::strtok(m, 0, ws, &mut saved);
            if t == 0 || m.u8(t) == b'#' {
                continue;
            }
            if clib::strlen(m, t) > 0x25 {
                o.call(m, SI_STR_GUID_FROM_STRING, &[t, sh + 0xec]);
                trace(m, o, &[0x086e_2d28, t]);
            }
        } else if is_tok(m, tok, TOK_VERSION) {
            trace(m, o, &[0x086e_2d3a]);
            let t = clib::strtok(m, 0, ws, &mut saved);
            if t == 0 {
                continue;
            }
            let v = clib::strtol(m, t, 0, 10);
            m.set_i32(sh + 0xfc, v);
            trace(m, o, &[0x086e_2d54, v as u32]);
        } else {
            trace(m, o, &[0x086e_2d80, tok]);
        }
    };
    m.release(0xcc);
    r
}

/// [soft 0x0854221c shaderReadLinks]: the `_SHADER_LINKS` section: lines naming a link statement
/// (the `shaderToken` entries whose flag word is 0: `CODE`, `LINK`, `INCLUDE`, `CFLAGS`,
/// `LDFLAGS`, `CCOMP`) and its parameter. A link record (0x20 bytes: +0 kind, +4 copy of the
/// parameter, +0xc the shader's second name) is found or made, counted in the links list and
/// appended to the shader's null-ended array at +0x2c (grown by realloc). Every table entry whose
/// keyword is a prefix of the token takes part (no early end). 1 at a line containing `_END`
/// (anywhere in the token), 0 at the end of the file or when out of memory.
pub fn shader_read_links(m: &mut Mem, o: &mut dyn Outside, f: u32, sh: u32) -> i32 {
    if f == 0 || o.call(m, FERROR, &[f]).eax != 0 || sh == 0 {
        return 0;
    }
    trace(m, o, &[0x086e_30bf]);
    let buf = m.local(0xcc);
    let tmp = m.local(0x20);
    let r = read_links_lines(m, o, f, sh, buf, tmp);
    m.release(0x20);
    m.release(0xcc);
    r
}

fn read_links_lines(m: &mut Mem, o: &mut dyn Outside, f: u32, sh: u32, buf: u32, tmp: u32) -> i32 {
    let ws = m.u32(TOK_WHITESPACES);
    let mut saved = 0u32;
    loop {
        if o.call(m, FGETS, &[buf, LINE, f]).eax == 0 {
            return 0;
        }
        m.set_u8(buf + LINE, 0);
        trace(m, o, &[0x086e_30d1, buf]);
        let tok = clib::strtok(m, buf, ws, &mut saved);
        if tok == 0 || m.u8(tok) == b'#' {
            continue;
        }
        if clib::strstr(m, tok, m.u32(TOK_END)) != 0 {
            return 1;
        }
        for i in 0..0x1du32 {
            let e = SHADER_TOKEN + 0x20 * i;
            if m.u32(e + 0x10) != 0 {
                continue;
            }
            let (kind, kw) = (m.u32(e), m.u32(e + 4));
            if clib::strncmp(m, tok, kw, clib::strlen(m, kw)) != 0 {
                continue;
            }
            let parm = clib::strtok(m, 0, ws, &mut saved);
            let name2 = m.u32(sh + 0x20);
            let mut rec = links_list_find(m, parm, name2);
            let shown = if parm == 0 { 0x086e_30e0 } else { parm };
            if rec == 0 {
                rec = m.si_calloc(1, 0x20);
                if rec == 0 {
                    return 0;
                }
                m.set_u32(rec, kind);
                let p = if parm != 0 { clib::strdup(m, parm) } else { 0 };
                m.set_u32(rec + 4, p);
                m.set_u32(rec + 0xc, name2);
                links_list_add(m, rec);
                trace(m, o, &[0x086e_30e5, kind, shown]);
            } else {
                // the original adds a stack copy that carries only the kind, parameter and second name
                m.set_u32(tmp, kind);
                m.set_u32(tmp + 4, parm);
                m.set_u32(tmp + 0xc, name2);
                links_list_add(m, tmp);
                trace(m, o, &[0x086e_3107, kind, shown]);
            }
            let arr = m.u32(sh + 0x2c);
            if arr == 0 {
                let a = m.si_malloc(8);
                m.set_u32(sh + 0x2c, a);
                if a == 0 {
                    return 0;
                }
                m.set_u32(a, rec);
                m.set_u32(a + 4, 0);
            } else {
                let mut n = 0u32;
                if m.u32(arr) != 0 {
                    n = 1;
                    while m.u32(arr + 4 * n) != 0 {
                        n += 1;
                    }
                }
                n += 2;
                let a = m.si_realloc(arr, n << 4);
                m.set_u32(sh + 0x2c, a);
                if a == 0 {
                    return 0;
                }
                m.set_u32(a + 4 * n - 8, rec);
                m.set_u32(a + 4 * n - 4, 0);
            }
        }
    }
}

/// [soft 0x08541d3c shaderReadDeclare]: the `_SHADER_DECLARE` section: one statement a line, the
/// keyword (a `shaderToken` entry with flag 1) and up to two words. The chain of declared tokens
/// (`cusAllocateToken`, nesting depth as the word at +0x10) is appended at +0x28. A stack of 0x14
/// block kinds checks `STRUCT_BEGIN/END`, `ABSSTR_BEGIN/END` and `DEFAULT_BEGIN/END` nesting:
/// ABSSTR, DEFAULT and IMAGE_MAP/IMAGE_SEQ are accepted at level 0 only, ends need their begin;
/// a refused line is traced and skipped. `_END` is looked for at the start of the whole line.
/// 1 at `_END` (with a trace when STRUCTs are still open), 0 at the end of the file. The second
/// word of a line without a first is the one of the line before (a local kept across lines).
/// Not reproduced: reading the stack one word before its start when a block end empties it (the
/// original reads the allocator's header word there; it cannot equal a kind); the array-of-X error
/// block is dead code in the original (a flag folded to zero).
pub fn shader_read_declare(m: &mut Mem, o: &mut dyn Outside, f: u32, sh: u32) -> i32 {
    if sh == 0 || f == 0 || o.call(m, FERROR, &[f]).eax != 0 {
        return 0;
    }
    trace(m, o, &[0x086e_2da5]);
    let stk = m.si_calloc(0x14, 4);
    if stk == 0 {
        return 0;
    }
    let buf = m.local(0xcc);
    let r = read_declare_lines(m, o, f, sh, buf, stk);
    m.release(0xcc);
    r
}

fn read_declare_lines(m: &mut Mem, o: &mut dyn Outside, f: u32, sh: u32, buf: u32, stk: u32) -> i32 {
    let ws = m.u32(TOK_WHITESPACES);
    let mut saved = 0u32;
    let head = sh + 0x28;
    let mut nest: i32 = 0;
    let mut top: i32 = -1;
    let mut last = 0u32;
    let mut parm2 = 0u32;
    let at = |i: i32| stk.wrapping_add((i as u32) << 2);
    loop {
        if o.call(m, FGETS, &[buf, LINE, f]).eax == 0 {
            trace(m, o, &[0x086e_2dc0]);
            m.free(stk);
            return 0;
        }
        m.set_u8(buf + LINE, 0);
        trace(m, o, &[0x086e_2de2, buf]);
        if is_tok(m, buf, TOK_END) {
            m.free(stk);
            if nest > 0 {
                trace(m, o, &[0x086e_30a2, nest as u32]);
            }
            return 1;
        }
        let tok = clib::strtok(m, buf, ws, &mut saved);
        if tok == 0 || m.u8(tok) == b'#' {
            continue;
        }
        let mut kind = 0u32;
        let mut idx = 0u32;
        while idx <= 0x1c {
            let e = SHADER_TOKEN + 0x20 * idx;
            let kw = m.u32(e + 4);
            if clib::strncmp(m, tok, kw, clib::strlen(m, kw)) == 0 && m.u32(e + 0x10) == 1 {
                kind = m.u32(e);
                break;
            }
            idx += 1;
        }
        if kind == 0 {
            trace(m, o, &[0x086e_2df1]);
            continue;
        }
        trace(m, o, &[0x086e_2e08, m.u32(SHADER_TOKEN + 0x20 * idx + 4)]);
        if top >= 0 {
            if kind == 0x12 || kind == 0x10 {
                trace(m, o, &[0x086e_2e20]);
                continue;
            }
            if kind.wrapping_sub(0x14) <= 1 {
                trace(m, o, &[0x086e_2e60]);
                continue;
            }
            if m.u32(at(top)) == 0x12 && kind == 0x13 {
                top -= 1;
            }
            if top >= 0 && m.u32(at(top)) == 0x10 && kind == 0x11 {
                top -= 1;
            }
            if top >= 0 && m.u32(at(top)) == 0xe && kind == 0xf {
                nest -= 1;
                top -= 1;
            }
        } else {
            if kind == 0x11 || kind == 0x13 || kind == 0xf {
                trace(m, o, &[0x086e_2ea0]);
                continue;
            }
            if kind == 0x10 || kind == 0x12 {
                top += 1;
                m.set_u32(at(top), kind);
            }
        }
        if kind == 0xe {
            top += 1;
            nest += 1;
            m.set_u32(at(top), 0xe);
        }
        let p1 = clib::strtok(m, 0, ws, &mut saved);
        if p1 != 0 {
            parm2 = clib::strtok(m, 0, ws, &mut saved);
            trace(m, o, &[0x086e_308c, p1]);
        }
        let t = cus_allocate_token(m, kind, p1, parm2, nest as u32, 0);
        if t != 0 {
            if last == 0 {
                m.set_u32(head, t);
            } else {
                m.set_u32(last + 0x1c, t);
            }
            m.set_u32(t + 0x1c, 0);
            last = t;
        }
    }
}
