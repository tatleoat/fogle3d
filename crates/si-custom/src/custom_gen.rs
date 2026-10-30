// SPDX-License-Identifier: MIT
//! `custom_shader.c`, the mental ray text writers: the `declare` statement of a custom shader
//! and the helpers it uses. The text goes through `SI_mrpipeFPRINTF` (here [`mr_printf`]).

use crate::cfmt;
use crate::custom_shader::*;
use crate::strlist::si_str_list_append;
use crate::{EXIT, STDOUT};
use si_mem::{clib, Mem, Outside, Routine};

const MRPIPE_ID: u32 = 0x086d_2c00;
const BUFFER: u32 = 0x086d_2c08;
const BUFF_NDX: u32 = 0x086d_2c0c;
/// The `FILE *` the pipe's text goes to while no pipe is open (`out`).
const OUT: u32 = 0x0882_9f08;
const STANDALONE_RENDERER: u32 = 0x0867_9400;

pub const SI_MRPIPE_FLUSH: Routine = Routine::new("SI_mrpipeFlush", 0x0842_c5d8);
pub const SI_MRTHREAD_EXIT: Routine = Routine::new("SI_mrthreadExit", 0x0843_02cc);
pub const FPUTS: Routine = Routine::new("fputs", 0x080c_4408);
pub const SI_SPDL_USE_NAMESRV_SHD_DECL: Routine = Routine::new("SI_spdlUseNamesrvShdDecl", 0x0858_0da4);
pub const SI_MENTAL_PRINT_DEFAULT_DECLARE: Routine = Routine::new("SI_mentalPrintDefaultDeclare", 0x0859_5420);

/// What `SI_mrpipeFPRINTF` (0x0842c68c) does, the text being formatted as `vsprintf` would: with no
/// pipe open (`mrpipeID` is -1) the text goes to `fputs(out)` when `out` is set, otherwise it is
/// appended to the pipe buffer, which is flushed first when it would pass 0x3fff bytes (a failed
/// flush ends the mr thread, or the program when it is the standalone renderer).
/// Not cited: the function belongs to mr_util, outside this package.
pub fn mr_printf(m: &mut Mem, o: &mut dyn Outside, fmt: u32, args: &[u32]) {
    let text = cfmt::format(m, fmt, args);
    if m.i32(MRPIPE_ID) == -1 {
        let out = m.u32(OUT);
        if out != 0 {
            if out == m.u32(STDOUT) {
                o.print(1, &text);
            } else {
                let s = m.local(text.len() as u32 + 1);
                m.put_cstr(s, &text);
                o.call(m, FPUTS, &[s, out]);
                m.release(text.len() as u32 + 1);
            }
        }
        return;
    }
    let n = text.len() as u32;
    if n.wrapping_add(m.u32(BUFF_NDX)) as i32 > 0x3fff {
        let r = o.call(m, SI_MRPIPE_FLUSH, &[]);
        if r.eax as u16 == 0 {
            if m.u16(STANDALONE_RENDERER) != 0 {
                o.call(m, EXIT, &[0]);
            }
            o.call(m, SI_MRTHREAD_EXIT, &[0]);
        }
    }
    let dest = m.u32(m.u32(BUFFER)).wrapping_add(m.u32(BUFF_NDX));
    let s = m.local(n + 1);
    m.put_cstr(s, &text);
    clib::memmove(m, dest, s, n);
    m.release(n + 1);
    let b = m.u32(BUFF_NDX).wrapping_add(n);
    m.set_u32(BUFF_NDX, b);
}

/// [soft 0x08544080 getArrayArgumentIndex]: the token of the array-argument name in the 9-entry table
/// (name pointer at +4, token at +0 of each 8 bytes from 0x086e2860); -1 when it is not there.
pub fn get_array_argument_index(m: &Mem, name: u32) -> i32 {
    if name == 0 {
        return -1;
    }
    for i in 0..9u32 {
        let e = 0x086e_2860 + i * 8;
        if clib::strcmp(m, name, m.u32(e + 4)) == 0 {
            return m.i32(e);
        }
    }
    -1
}

fn mi2(m: &mut Mem, o: &mut dyn Outside) -> u16 {
    o.call(m, SI_GETMI2_OUTPUT, &[]).eax as u16
}

/// [soft 0x085440e0 writeShaderDeclareArrayMembers]: writes the members of a dotted list of array-argument
/// names (`"min.max"`), each as a declaration of its kind, separated by commas; the string is cut at
/// each dot while its part is handled and put back. A null list writes `string "name"`.
pub fn write_shader_declare_array_members(m: &mut Mem, o: &mut dyn Outside, list: u32) {
    let mut first = true;
    if list == 0 {
        mr_printf(m, o, 0x086e_374a, &[]);
        return;
    }
    let mut p = list;
    while m.u8(p) != 0 {
        let mut q = p;
        while m.u8(q) != 0 && m.u8(q) != b'.' {
            q += 1;
        }
        let c = m.u8(q);
        m.set_u8(q, 0);
        let idx = get_array_argument_index(m, p);
        if idx >= 0 {
            if !first {
                mr_printf(m, o, 0x086e_375a, &[]);
            }
            mr_printf(m, o, 0x086e_375d, &[]);
            if mi2(m, o) != 0 {
                mr_printf(m, o, 0x086e_3760, &[]);
            }
            let fmt = match idx {
                0 => Some(0x086e_3762),
                1 => Some(0x086e_376f),
                2 => Some(0x086e_377c),
                3 => {
                    mr_printf(m, o, 0x086e_3789, &[p]);
                    mr_printf(m, o, 0x086e_379d, &[]);
                    if mi2(m, o) != 0 {
                        mr_printf(m, o, 0x086e_37a2, &[p]);
                    }
                    Some(0x086e_37a4)
                }
                4 => Some(0x086e_37b8),
                5 => Some(0x086e_37c8),
                6 => Some(0x086e_37d8),
                7 => Some(0x086e_37e8),
                8 => {
                    if mi2(m, o) != 0 { Some(0x086e_37f6) } else { None }
                }
                _ => None,
            };
            if let Some(f) = fmt {
                mr_printf(m, o, f, &[p]);
            }
            if mi2(m, o) != 0 {
                first = false;
            } else if mi2(m, o) == 0 && idx != 8 {
                first = false;
            }
        }
        m.set_u8(q, c);
        if c == 0 {
            return;
        }
        p = q + 1;
    }
}

/// [soft 0x08543d50 shaderGenerateDeclareDefaults]: for the shader's kind, the default declaration
/// (`SI_mentalPrintDefaultDeclare(code, file)`): kinds 2 to 7 and 15 have one. 1 when written, 0 if not.
pub fn shader_generate_declare_defaults(m: &mut Mem, o: &mut dyn Outside, sh: u32, file: u32) -> i32 {
    let kind = m.u32(m.u32(sh).wrapping_add(0xc));
    let code = match kind {
        1 | 9 | 0xe => 0,
        2 => 3,
        3 => 5,
        4 => 6,
        5 | 0xf => 1,
        6 => 2,
        7 => 4,
        _ => return 0,
    };
    o.call(m, SI_MENTAL_PRINT_DEFAULT_DECLARE, &[code, file]);
    1
}

/// [soft 0x08543dc8 shaderGenerateInclude]: nothing to write; answers 1.
pub fn shader_generate_include() -> i32 {
    1
}

/// [soft 0x0854429c shaderTokenInCustomShaderList]: the token is one of the shader's link tokens.
pub fn shader_token_in_custom_shader_list(m: &Mem, tok: u32, sh: u32) -> i32 {
    if tok != 0 && sh != 0 {
        let arr = m.u32(sh + 0x2c);
        if arr != 0 {
            let mut i = 0u32;
            while m.u32(arr + i * 4) != 0 {
                if tok == m.u32(arr + i * 4) {
                    return 1;
                }
                i += 1;
            }
        }
    }
    0
}

/// [soft 0x0854271c SI_shaderGenerateDeclares]: writes the banner, then the `declare` of every active
/// table shader while the previous ones went well. Answers the last result (1 if none were written).
pub fn si_shader_generate_declares(m: &mut Mem, o: &mut dyn Outside, file: u32) -> i32 {
    let mut ok = true;
    mr_printf(m, o, 0x086e_3140, &[]);
    let mut i = 0i32;
    let mut prev = ok;
    if 0 < m.i32(NUM_SHADER) {
        loop {
            let s = m.u32(m.u32(SHADER_LIST).wrapping_add((i as u32) << 2));
            ok = prev;
            if s != 0 && 0 < m.i32(s + 0x14) {
                ok = false;
                if prev {
                    ok = si_shader_generate_declare(m, o, s, file) as u16 != 0;
                }
            }
            i += 1;
            prev = ok;
            if i >= m.i32(NUM_SHADER) {
                break;
            }
        }
    }
    ok as i32
}

/// (kind) -> the six strings of a declaration: before (when the last one ended a member), the
/// array-bound text, the mi2 text, the name used when the token has none, the name format and the
/// declaration format.
struct Decl {
    pre: u32,
    lp: u32,
    mi2: u32,
    none: u32,
    sfmt: u32,
    fin: u32,
}

fn decl_of(kind: i32) -> Option<Decl> {
    let d = |pre, lp, mi2, none, sfmt, fin| Some(Decl { pre, lp, mi2, none, sfmt, fin });
    match kind {
        2 => d(0x34fb, 0x34fe, 0x3500, 0x3502, 0x3503, 0x3506),
        3 => d(0x35cd, 0x35d2, 0x35d0, 0x35d4, 0x35d5, 0x35d8),
        4 => d(0x3513, 0x3516, 0x3518, 0x351a, 0x351b, 0x351e),
        5 => d(0x35e5, 0x35e8, 0x35ea, 0x35ec, 0x35ed, 0x35f0),
        6 => d(0x359c, 0x359f, 0x35a1, 0x35a3, 0x35a4, 0x35a7),
        7 => d(0x352a, 0x352d, 0x352f, 0x3531, 0x3532, 0x3535),
        8 => d(0x35b3, 0x35b6, 0x35b8, 0x35ba, 0x35bb, 0x35be),
        9 => d(0x3540, 0x3543, 0x3545, 0x3547, 0x3548, 0x354b),
        10 => d(0x355f, 0x3562, 0x3564, 0x3566, 0x3567, 0x356a),
        0xb | 0x14 | 0x15 => d(0x357e, 0x3581, 0x3583, 0x3585, 0x3586, 0x3589),
        0xc => d(0x3637, 0x363a, 0x363c, 0x363e, 0x363f, 0x3642),
        0x1d => d(0x35fc, 0x35ff, 0x3601, 0x3603, 0x3604, 0x3607),
        _ => None,
    }
    .map(|x| Decl { pre: 0x086e_0000 + x.pre, lp: 0x086e_0000 + x.lp, mi2: 0x086e_0000 + x.mi2, none: 0x086e_0000 + x.none, sfmt: 0x086e_0000 + x.sfmt, fin: 0x086e_0000 + x.fin })
}

/// `bound` copies of the array-bound text `lp`: one more than the token's count word (+0x10) when that
/// is not negative.
fn bound_text(m: &mut Mem, o: &mut dyn Outside, tok: u32, lp: u32) {
    if m.u32(tok + 0x10) < 0x8000_0000 {
        let mut i = 0i32;
        loop {
            mr_printf(m, o, lp, &[]);
            i = i.wrapping_add(1);
            if i > m.i32(tok + 0x10) {
                break;
            }
        }
    }
}

/// [soft 0x08543250 SI_shaderGenerateDeclare]: writes the shader's `declare` statement in mental ray
/// text. With mi2 output and name-server declarations and a non-zero GUID only a `lookup` and
/// `declare` line; otherwise one member per token of the chain (by kind), the end of the parameter list, and
/// for mi2 the version, the `apply` line of the shader's kind and `end declare`. Answers 1.
pub fn si_shader_generate_declare(m: &mut Mem, o: &mut dyn Outside, sh: u32, file: u32) -> i32 {
    let mut prev: i32 = 0;
    let mut s3: i16 = 0;
    let big = 0x1000u32;
    if mi2(m, o) != 0 && o.call(m, SI_SPDL_USE_NAMESRV_SHD_DECL, &[]).eax as u16 != 0 {
        if clib::memcmp(m, sh + 0xec, NULL_GUID, 16) != 0 {
            let gs = m.local(44);
            m.set_u8(gs, 0);
            o.call(m, SI_STR_GUID_TO_STRING, &[gs, sh + 0xec]);
            mr_printf(m, o, 0x086e_34b1, &[gs]);
            let l = m.local(big);
            shader_get_func_name(m, o, sh, l, 0x1000);
            mr_printf(m, o, 0x086e_34bf, &[l]);
            m.release(big);
            m.release(44);
            return 1;
        }
    }
    let local = m.local(big);
    let mut tok = m.u32(sh + 0x28);
    while tok != 0 {
        let kind = m.i32(tok);
        let d = decl_of(kind);
        if let Some(d) = d {
            if s3 == 1 {
                mr_printf(m, o, d.pre, &[]);
            }
            if kind == 3 {
                s3 = mi2(m, o) as i16;
                if s3 != 0 {
                    mr_printf(m, o, d.mi2, &[]);
                }
                bound_text(m, o, tok, d.lp);
            } else {
                if kind != 0xc || prev != 0xd {
                    bound_text(m, o, tok, d.lp);
                }
                s3 = mi2(m, o) as i16;
                if s3 != 0 {
                    mr_printf(m, o, d.mi2, &[]);
                }
            }
            let name = match m.u32(tok + 4) {
                0 => d.none,
                n => n,
            };
            let t = cfmt::format(m, d.sfmt, &[name]);
            m.put_cstr(local, &t);
            mr_printf(m, o, d.fin, &[local]);
            s3 = 1;
        } else {
            match kind {
                1 => {
                    shader_get_func_name(m, o, sh, local, 0x1000);
                    s3 = mi2(m, o) as i16;
                    let f = if s3 == 0 { 0x086e_34ed } else { 0x086e_34d1 };
                    mr_printf(m, o, f, &[local]);
                    s3 = if m.i16(sh + 0x18) == 0 { 0 } else { shader_generate_declare_defaults(m, o, sh, file) as i16 };
                }
                0xd => {
                    if s3 == 1 {
                        mr_printf(m, o, 0x086e_364d, &[]);
                    }
                    bound_text(m, o, tok, 0x086e_3650);
                    s3 = mi2(m, o) as i16;
                    if s3 != 0 {
                        mr_printf(m, o, 0x086e_3652, &[]);
                    }
                    mr_printf(m, o, 0x086e_3654, &[]);
                    s3 = 0;
                }
                0xe => {
                    if s3 == 1 {
                        mr_printf(m, o, 0x086e_365b, &[]);
                    }
                    if prev != 0xd {
                        bound_text(m, o, tok, 0x086e_365e);
                    }
                    s3 = mi2(m, o) as i16;
                    if s3 != 0 {
                        mr_printf(m, o, 0x086e_3660, &[]);
                    }
                    let name = match m.u32(tok + 4) {
                        0 => 0x086e_3662,
                        n => n,
                    };
                    let t = cfmt::format(m, 0x086e_3663, &[name]);
                    m.put_cstr(local, &t);
                    mr_printf(m, o, 0x086e_3666, &[local]);
                    s3 = 0;
                }
                0xf => {
                    bound_text(m, o, tok, 0x086e_3675);
                    s3 = mi2(m, o) as i16;
                    if s3 != 0 {
                        mr_printf(m, o, 0x086e_3677, &[]);
                    }
                    mr_printf(m, o, 0x086e_3679, &[]);
                    s3 = 1;
                }
                0x1c => {
                    if s3 == 1 {
                        mr_printf(m, o, 0x086e_3612, &[]);
                    }
                    if prev != 0xd {
                        bound_text(m, o, tok, 0x086e_3615);
                    }
                    s3 = mi2(m, o) as i16;
                    if s3 != 0 {
                        mr_printf(m, o, 0x086e_3617, &[]);
                    }
                    let name = match m.u32(tok + 4) {
                        0 => 0x086e_3619,
                        n => n,
                    };
                    let t = cfmt::format(m, 0x086e_361a, &[name]);
                    m.put_cstr(local, &t);
                    mr_printf(m, o, 0x086e_361d, &[local]);
                    s3 = mi2(m, o) as i16;
                    if s3 != 0 {
                        mr_printf(m, o, 0x086e_362a, &[]);
                    }
                    mr_printf(m, o, 0x086e_362c, &[]);
                    write_shader_declare_array_members(m, o, m.u32(tok + 8));
                    mr_printf(m, o, 0x086e_3630, &[]);
                    s3 = mi2(m, o) as i16;
                    if s3 != 0 {
                        mr_printf(m, o, 0x086e_3632, &[]);
                    }
                    mr_printf(m, o, 0x086e_3634, &[]);
                    s3 = 1;
                }
                _ => {}
            }
        }
        prev = m.i32(tok);
        tok = m.u32(tok + 0x1c);
    }
    m.release(big);
    mr_printf(m, o, 0x086e_367b, &[]);
    if mi2(m, o) == 0 {
        mr_printf(m, o, 0x086e_3748, &[]);
        return 1;
    }
    if -1 < m.i32(sh + 0xfc) {
        mr_printf(m, o, 0x086e_3680, &[m.u32(sh + 0xfc)]);
    }
    if m.u32(sh) != 0 {
        let s = match m.i32(m.u32(sh) + 0xc) {
            1 => Some(0x086e_368d),
            2 => Some(0x086e_369e),
            3 | 4 => Some(0x086e_36ad),
            5 => Some(0x086e_36bd),
            6 => Some(0x086e_36cb),
            7 => Some(0x086e_36d8),
            8 => Some(0x086e_36ec),
            9 => Some(0x086e_36fd),
            10 => Some(0x086e_370c),
            0xe => Some(0x086e_371b),
            0xf => Some(0x086e_372a),
            _ => None,
        };
        if let Some(s) = s {
            mr_printf(m, o, s, &[]);
        }
    }
    mr_printf(m, o, 0x086e_373a, &[]);
    1
}

// ---------------------------------------------------------------------------------------------
// the 'INCLUDE', 'CODE', 'LINK', c_flags, ld_flags and c_compiler statements

pub const FERROR: Routine = Routine::new("ferror", 0x080c_3878);
pub const GETCWD: Routine = Routine::new("getcwd", 0x080c_4d08);
pub const ACCESS: Routine = Routine::new("access", 0x080c_3f18);
pub const GETENV: Routine = Routine::new("getenv", 0x080c_3bb8);
pub const SI_CUS_TRACE: Routine = Routine::new("si_cusTrace", 0x0854_42f0);
pub const SI_MRFILE_CONVERT_TO_UNIX: Routine = Routine::new("SI_mrfileConvertToUnix", 0x0842_faa8);
pub const SI_DB_DIR_SEARCH_DATABASE_FROM_NAME: Routine = Routine::new("SI_dbDirSearchDatabaseFromName", 0x0812_1744);
pub const SI_STR_LIST_MERGE: Routine = Routine::new("SI_strListMerge", 0x083c_f714);
pub const SI_STR_LIST_DESTROY: Routine = Routine::new("SI_strListDestroy", 0x083c_f7fc);

const SHADER_DB_ROOT: u32 = 0x0868_4064;
const DB_ROOT_HANDLE: u32 = 0x0878_1390;
const MI_SHD_TOK: u32 = 0x0882_2820;
const MI_SHD_TOK_INIT: u32 = 0x0882_2824;
const LINK_EXTENSIONS: u32 = 0x086e_28a8;

/// [soft 0x08542798 SI_shaderGenerateIncludes]: writes the banner of the 'INCLUDE' statements to
/// `file` (a null `file` is passed to `fprintf` as it is), then asks shaderGenerateInclude for every
/// table shader that is in use while the previous ones went well. Answers 1, or 0 for a null file
/// or a stream in error.
pub fn si_shader_generate_includes(m: &mut Mem, o: &mut dyn Outside, file: u32) -> i32 {
    crate::fprintf(m, o, file, 0x086e_3180, &[]);
    if file == 0 {
        return 0;
    }
    if o.call(m, FERROR, &[file]).eax != 0 {
        return 0;
    }
    let mut ok: u16 = 1;
    let mut i = 0i32;
    while i < m.i32(NUM_SHADER) {
        let sh = m.u32(m.u32(SHADER_LIST).wrapping_add((i as u32) << 2));
        if sh != 0 && m.i32(sh + 0x14) > 0 {
            let mut v: u16 = 0;
            if ok != 0 {
                v = (shader_generate_include() as u16 != 0) as u16;
            }
            ok = v;
        }
        i += 1;
    }
    ok as i16 as i32
}

fn is_empty(m: &Mem, s: u32) -> bool {
    m.u8(s) == 0
}

/// The start of the statements: the pipe's own "declaration" flag, the banner and the shader
/// database root (the current directory's name when none is set). `Err` when there is neither.
fn statement_root(m: &mut Mem, o: &mut dyn Outside, banner: u32) -> Result<(u16, u32, u32), ()> {
    let mut decl: u16 = 0;
    if mi2(m, o) != 0 {
        decl = (o.call(m, SI_SPDL_USE_NAMESRV_SHD_DECL, &[]).eax as u16 != 0) as u16;
    }
    mr_printf(m, o, banner, &[]);
    let mut root = m.u32(SHADER_DB_ROOT);
    let mut cwd = 0;
    if root == 0 {
        cwd = o.call(m, GETCWD, &[0, 0x400]).eax;
        root = cwd;
        if root == 0 {
            return Err(());
        }
    }
    Ok((decl, root, cwd))
}

/// sprintf of one format into the buffer.
fn sprintf(m: &mut Mem, buf: u32, fmt: u32, args: &[u32]) {
    let t = cfmt::format(m, fmt, args);
    m.put_cstr(buf, &t);
}

/// [soft 0x08542830 SI_shaderGenerateCodes]: one `code "path"` line for every active code token
/// (kind 0x17) of the links list that the shader uses (any, when `sh` is null). The path is
/// `<database directory>/src/<name>`, the directory being that of the token's database name in
/// the database directory (else the root). Answers 1; 0 when there is no root directory.
pub fn si_shader_generate_codes(m: &mut Mem, o: &mut dyn Outside, sh: u32) -> i32 {
    let (decl, root, cwd) = match statement_root(m, o, 0x086e_31c0) {
        Ok(v) => v,
        Err(()) => return 0,
    };
    let buf = m.local(0x1000);
    let db = m.local(8);
    let mut t = m.u32(LINKS_LIST);
    while t != 0 {
        if sh == 0 || m.u32(sh + 0x2c) == 0 || shader_token_in_custom_shader_list(m, t, sh) as u16 != 0 {
            let mut dir = 0u32;
            let name = m.u32(t + 4);
            if m.i32(t) == 0x17 && name != 0 && !is_empty(m, name) && m.i32(t + 0x18) > 0 {
                let name2 = m.u32(t + 0xc);
                if name2 != 0 {
                    let r = o.call(m, SI_DB_DIR_SEARCH_DATABASE_FROM_NAME, &[DB_ROOT_HANDLE, name2, db, db + 4]).eax;
                    let d = m.u32(db);
                    // a null record with answer 0x64 is read at +0xc as it is (the original's check
                    // differs from SI_shaderGenerateLinks here); the Rust reads the image's page zero
                    if r == 0x64 && (d == 0 || m.u32(d + 0x24) == 2) {
                        dir = m.u32(d + 0xc);
                    } else {
                        o.call(m, SI_CUS_TRACE, &[0x086e_3200]);
                    }
                }
                if dir == 0 {
                    dir = root;
                }
                sprintf(m, buf, 0x086e_3231, &[dir, name]);
                if sh == 0 {
                    o.call(m, SI_MRFILE_CONVERT_TO_UNIX, &[buf, buf, 0x1000]);
                }
                if decl != 0 {
                    mr_printf(m, o, 0x086e_3240, &[]);
                }
                mr_printf(m, o, 0x086e_3264, &[buf]);
            }
        }
        t = m.u32(t + 0x1c);
    }
    if cwd != 0 {
        m.free(cwd);
    }
    m.release(0x1008);
    1
}

/// [soft 0x08542a44 shaderLinksUseRegistry]: whether the environment names a shader token
/// (`SI_MI_SHADER_TOKEN`); asked once and kept.
pub fn shader_links_use_registry(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    if m.u32(MI_SHD_TOK_INIT) == 0 {
        let r = o.call(m, GETENV, &[0x086e_326f]).eax;
        m.set_u16(MI_SHD_TOK, (r != 0) as u16);
        m.set_u32(MI_SHD_TOK_INIT, 1);
    }
    m.u16(MI_SHD_TOK) as i16 as i32
}

/// [soft 0x08543dd4 shaderCorrectLinkPath]: a library name ending in one of the three extensions of
/// the table at 0x086e28a8 (ten bytes an entry: the extension, then its replacement) gets the
/// replacement, whatever the case of the extension, from the last dot on.
pub fn shader_correct_link_path(m: &mut Mem, path: u32) {
    let p = clib::strrchr(m, path, b'.');
    if p != 0 {
        for i in 0..3u32 {
            let e = LINK_EXTENSIONS + 10 * i;
            let n = clib::strlen(m, e);
            if clib::strncasecmp(m, e, p, n) == 0 {
                clib::strcpy(m, p, e + 5);
                break;
            }
        }
    }
}

/// [soft 0x08542a88 SI_shaderGenerateLinks]: one `link "path"` line for every active link token
/// (kind 0x16) the shader uses. The path comes from the token's registry entry
/// (`{NAME2}/lib20/name`) when the mental ray output asks for it, else
/// `<directory>/obj/name`, falling back to `lib20` or `lib` when the file cannot be read (then
/// the line is not written and the message goes to stderr). Answers 1; 0 without a root.
pub fn si_shader_generate_links(m: &mut Mem, o: &mut dyn Outside, sh: u32) -> i32 {
    let (decl, root, cwd) = match statement_root(m, o, 0x086e_32a0) {
        Ok(v) => v,
        Err(()) => return 0,
    };
    let buf = m.local(0x1000);
    let upper = m.local(0x200);
    let db = m.local(8);
    let mut t = m.u32(LINKS_LIST);
    while t != 0 {
        if sh == 0 || m.u32(sh + 0x2c) == 0 || shader_token_in_custom_shader_list(m, t, sh) as u16 != 0 {
            link_token(m, o, sh, t, decl, root, buf, upper, db);
        }
        t = m.u32(t + 0x1c);
    }
    if cwd != 0 {
        m.free(cwd);
    }
    m.release(0x1208);
    1
}

#[allow(clippy::too_many_arguments)]
fn link_token(m: &mut Mem, o: &mut dyn Outside, sh: u32, t: u32, decl: u16, root: u32, buf: u32, upper: u32, db: u32) {
    let mut dir = 0u32;
    let name = m.u32(t + 4);
    if m.i32(t) != 0x16 || name == 0 || is_empty(m, name) || m.i32(t + 0x18) <= 0 {
        return;
    }
    let name2 = m.u32(t + 0xc);
    if name2 != 0 {
        if mi2(m, o) != 0 && shader_links_use_registry(m, o) as u16 != 0 {
            sprintf(m, upper, 0x086e_32c9, &[name2]);
            let n = clib::strlen(m, upper) as i32;
            let mut i = n - 1;
            while i >= 0 {
                let c = m.u8(upper + i as u32);
                m.set_u8(upper + i as u32, clib::toupper(c as i8 as i32) as u8);
                i -= 1;
            }
            sprintf(m, buf, 0x086e_32ce, &[upper, name]);
        } else {
            m.set_u32(db, 0);
            let r = o.call(m, SI_DB_DIR_SEARCH_DATABASE_FROM_NAME, &[DB_ROOT_HANDLE, name2, db, db + 4]).eax;
            let d = m.u32(db);
            if r == 0x64 && d != 0 && m.u32(d + 0x24) == 2 {
                dir = m.u32(d + 0xc);
            } else {
                o.call(m, SI_CUS_TRACE, &[0x086e_32e0]);
                dir = 0;
            }
        }
    }
    let registry = mi2(m, o) != 0 && shader_links_use_registry(m, o) as u16 != 0 && m.u32(t + 0xc) != 0;
    if !registry {
        if dir == 0 {
            dir = root;
        }
        sprintf(m, buf, 0x086e_3311, &[dir, name]);
        if o.call(m, ACCESS, &[buf, 4]).eax != 0 {
            if mi2(m, o) != 0 {
                sprintf(m, buf, 0x086e_331b, &[dir, name]);
            } else {
                sprintf(m, buf, 0x086e_3327, &[dir, name]);
            }
            if o.call(m, ACCESS, &[buf, 4]).eax != 0 {
                crate::fprintf(m, o, m.u32(crate::STDERR), 0x086e_3340, &[buf]);
                return;
            }
        }
    }
    if sh == 0 {
        o.call(m, SI_MRFILE_CONVERT_TO_UNIX, &[buf, buf, 0x1000]);
        shader_correct_link_path(m, buf);
    }
    if decl != 0 {
        mr_printf(m, o, 0x086e_3380, &[]);
    }
    mr_printf(m, o, 0x086e_33a4, &[buf]);
}

/// The common walk of the three flag writers: a string list is started with the option `tag`,
/// the name of every active token of `kind` (non-empty) is appended, and the list is merged into
/// the caller's two arguments when one was found. 0 when out of memory (the message goes to stderr).
fn flag_list(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, tag: u32, kind: i32, err: u32) -> i32 {
    let l = m.local(8);
    m.set_u32(l, 0);
    m.set_u32(l + 4, 0);
    si_str_list_append(m, tag, l, l + 4);
    let mut found = false;
    let mut t = m.u32(LINKS_LIST);
    while t != 0 {
        let name = m.u32(t + 4);
        if m.i32(t) == kind && name != 0 && !is_empty(m, name) && m.i32(t + 0x18) > 0 {
            found = true;
            if si_str_list_append(m, name, l, l + 4) == 0 {
                crate::fprintf(m, o, m.u32(crate::STDERR), err, &[]);
                o.call(m, SI_STR_LIST_DESTROY, &[m.u32(l), m.u32(l + 4)]);
                m.release(8);
                return 0;
            }
        }
        t = m.u32(t + 0x1c);
    }
    if found {
        o.call(m, SI_STR_LIST_MERGE, &[a, b, m.u32(l), m.u32(l + 4)]);
    }
    o.call(m, SI_STR_LIST_DESTROY, &[m.u32(l), m.u32(l + 4)]);
    m.release(8);
    1
}

/// [soft 0x08542e64 SI_shaderGenerateCflags]: the names of the active kind-0x19 tokens as a list
/// `-c_flags name ...` merged into the caller's arguments `a`, `b` (SI_strListMerge) when any exist.
/// Answers 1; 0 out of memory.
pub fn si_shader_generate_cflags(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32) -> i32 {
    flag_list(m, o, a, b, 0x086e_33af, 0x19, 0x086e_33c0)
}

/// [soft 0x08542f9c SI_shaderGenerateLDflags]: as Cflags for kind 0x1a and `-ld_flags`.
pub fn si_shader_generate_ldflags(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32) -> i32 {
    flag_list(m, o, a, b, 0x086e_33e8, 0x1a, 0x086e_3400)
}

/// [soft 0x085430cc SI_shaderGenerateCcomp]: as Cflags for kind 0x1b and `-c_compiler`, but only the
/// first token's name is listed: a later token with another name is reported on stderr
/// (`Two compiler references given`) and one with the same name is left out. Answers 1; 0 out of memory.
pub fn si_shader_generate_ccomp(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32) -> i32 {
    let l = m.local(8);
    m.set_u32(l, 0);
    m.set_u32(l + 4, 0);
    si_str_list_append(m, 0x086e_3428, l, l + 4);
    let mut found = false;
    let mut first = 0u32;
    let mut t = m.u32(LINKS_LIST);
    while t != 0 {
        let name = m.u32(t + 4);
        if m.i32(t) == 0x1b && name != 0 && !is_empty(m, name) && m.i32(t + 0x18) > 0 {
            found = true;
            if first == 0 {
                first = t;
                if si_str_list_append(m, name, l, l + 4) == 0 {
                    crate::fprintf(m, o, m.u32(crate::STDERR), 0x086e_3440, &[]);
                    o.call(m, SI_STR_LIST_DESTROY, &[m.u32(l), m.u32(l + 4)]);
                    m.release(8);
                    return 0;
                }
            } else if clib::strcmp(m, name, m.u32(first + 4)) != 0 {
                let f = m.u32(first + 4);
                crate::fprintf(m, o, m.u32(crate::STDERR), 0x086e_3480, &[name, f]);
            }
        }
        t = m.u32(t + 0x1c);
    }
    if found {
        o.call(m, SI_STR_LIST_MERGE, &[a, b, m.u32(l), m.u32(l + 4)]);
    }
    o.call(m, SI_STR_LIST_DESTROY, &[m.u32(l), m.u32(l + 4)]);
    m.release(8);
    1
}

/// [soft 0x0854253c SI_shaderGetSymbolType]: finds the shader's declared token that `name` (its
/// text up to the first dot compared as a prefix of the token's name, case ignored) refers to.
/// Walks the token chain: kinds 0x10/0x11 switch a filter on and off (while on only kind-0x10
/// tokens count), 0x12 starts skipping tokens and 0x13 ends it; only tokens without a +0x10
/// word are looked at. For the token found `*kind_out` is set from the letter after the first
/// dot of `name` (r x -> 1, g y -> 2, b z -> 3, a -> 4, otherwise 0), `*text_out` (when given) is a
/// copy of the token's string, and its kind is answered. 0 when none, or no shader or name.
pub fn si_shader_get_symbol_type(m: &mut Mem, sh: u32, name: u32, kind_out: u32, text_out: u32) -> i32 {
    let mut filter = false;
    let mut skip = false;
    if text_out != 0 {
        m.set_u32(text_out, 0);
    }
    if sh == 0 || name == 0 {
        return 0;
    }
    let mut tok = m.u32(sh + 0x28);
    while tok != 0 {
        let kind = m.i32(tok);
        let mut next = false;
        match kind {
            0x11 => filter = false,
            0x10 => filter = true,
            0x12 => {
                skip = true;
                next = true;
            }
            0x13 => skip = false,
            _ => {}
        }
        if !next && !skip && !(filter && kind != 0x10) && m.u32(tok + 0x10) == 0 {
            let tn = m.u32(tok + 4);
            if tn != 0 {
                let n = clib::strlen(m, tn);
                if clib::strncasecmp(m, name, tn, n) == 0 {
                    if text_out != 0 && m.u32(tok + 8) != 0 {
                        let c = clib::strdup(m, m.u32(tok + 8));
                        m.set_u32(text_out, c);
                    }
                    m.set_u32(kind_out, 0);
                    let p = clib::strchr(m, name, b'.');
                    if p != 0 && clib::strlen(m, p) > 1 {
                        let v = m.u8(p + 1).wrapping_sub(0x41) as i8 as i32 as u32;
                        if v <= 0x39 {
                            let k = match m.u8(p + 1) {
                                b'R' | b'X' | b'r' | b'x' => 1,
                                b'G' | b'Y' | b'g' | b'y' => 2,
                                b'B' | b'Z' | b'b' | b'z' => 3,
                                b'A' | b'a' => 4,
                                _ => 0,
                            };
                            if k != 0 {
                                m.set_u32(kind_out, k);
                            }
                        }
                    }
                    return kind;
                }
            }
        }
        tok = m.u32(tok + 0x1c);
    }
    0
}
