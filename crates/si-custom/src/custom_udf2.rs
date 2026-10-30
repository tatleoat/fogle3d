// SPDX-License-Identifier: MIT
//! `custom_udf.c`, the rest (unit shdb2): the picture lookup of user textures, the texture declarations of
//! the mental ray output, the shader flag getters and the per-process udf list storage; finding 0041,
//! "2026-10-29 (shdb2)".

use crate::cfmt;
use crate::custom_dyn::SI_ERROR;
use crate::custom_gen::{mr_printf, si_shader_get_symbol_type, SI_MRFILE_CONVERT_TO_UNIX};
use crate::custom_udf::{si_udf_is_enabled, SI_EMPTY_STRING, SI_IS_IGLOO, SI_STATUS_DISPLAY_MESSAGE_TO_FILE_ONLY, SI_UDF_PARSE_RAW_DATA};
use crate::custom_util::SI_PROC_DATA;
use crate::shdb_init::SI_STATUS_DISPLAY_MESSAGE;
use crate::shdb_translator::SI_MI2_IS_INCREMENTAL;
use si_mem::{clib, Mem, Outside, Routine};

const SI_DB_CHECK_FILE_EXISTENCE: Routine = Routine::new("SI_dbCheckFileExistence", 0x0812_0f4c);
const SI_DBFILENAME_REMOVE_PATH: Routine = Routine::new("SI_dbfilenameRemovePath", 0x0812_1d84);
const SI_DB_SET_CHAPTER: Routine = Routine::new("SI_dbSetChapter", 0x0812_2654);
const SI_DBFILEPATH_BUILD: Routine = Routine::new("SI_dbfilepathBuild", 0x0812_26cc);
const SI_DB_CHAPTER_GET_SUB_DIRS: Routine = Routine::new("SI_dbChapterGetSubDirs", 0x0812_508c);
const SI_FILENAME_STRIP_INDEX: Routine = Routine::new("SI_filenameStripIndex", 0x083c_fa30);
const SI_SHADER_GENERATE_LDFLAGS: Routine = Routine::new("SI_shaderGenerateLDflags", 0x0854_2f9c);
const SI_GETMI2_OUTPUT: Routine = Routine::new("SI_getmi2Output", 0x0858_0cfc);

const SI_PDBIIN: u32 = 0x0878_2058;
const SI_CID_PICTURES: u32 = 0x0878_1670;
const SI_CID_OUTPUT_SHADERS: u32 = 0x0878_16ac;
/// `SI_dbdir`: the count (+0) and the array of 0x28-byte database records (+4).
const SI_DBDIR: u32 = 0x0878_1390;
const SI_RESDIR: u32 = 0x0878_1628;
/// `SI_dBWorkFnameExt`, the file name work buffer.
const WORK_FNAME: u32 = 0x0878_8540;
const SI_ELEM_DICTS: u32 = 0x0867_931c;
const SI_NB_ELEM_DICTS: u32 = 0x0867_9318;
const UDF_LIST_POINTER: u32 = 0x086e_38ac;
const SI_UDF_LIST: u32 = 0x086e_49d4;
const SI_NUM_UDF: u32 = 0x086e_49d8;

fn sprintf(m: &mut Mem, dst: u32, fmt: u32, args: &[u32]) {
    let t = cfmt::format(m, fmt, args);
    m.put_cstr(dst, &t);
}

/// The element dictionary of the current process.
fn cur_dict(m: &Mem) -> u32 {
    let pd = m.u32(SI_PROC_DATA);
    m.u32(pd).wrapping_shl(3).wrapping_sub(m.u32(pd)).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS))
}

fn show(m: &mut Mem, o: &mut dyn Outside, msg: u32) {
    if o.call(m, SI_IS_IGLOO, &[]).eax as u16 != 0 {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE_TO_FILE_ONLY, &[msg, 2]);
    } else {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[msg, 2]);
    }
}

/// [soft 0x08546080 SI_udfGenerateLDflags]: `SI_shaderGenerateLDflags(a, b)` (a short, sign-extended).
pub fn si_udf_generate_ldflags(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32) -> i32 {
    o.call(m, SI_SHADER_GENERATE_LDFLAGS, &[a, b]).eax as u16 as i16 as i32
}

/// [soft 0x085466dc SI_udfGetShaderFlags]: the flag word (+0x10) of the record's shader; 0 for a null or a
/// disabled record or one without a shader.
pub fn si_udf_get_shader_flags(m: &mut Mem, o: &mut dyn Outside, u: u32) -> u32 {
    if u != 0 && si_udf_is_enabled(m, o, u) as u16 != 0 {
        let sh = m.u32(u.wrapping_add(0x24));
        if sh != 0 {
            return m.u32(sh.wrapping_add(0x10));
        }
    }
    0
}

/// [soft 0x08546710 SI_udfGetOutputBufferFlags]: as the shader flags, but only for a record of the output
/// shaders class (`SI_cid.OUTPUT_SHADERS`).
pub fn si_udf_get_output_buffer_flags(m: &mut Mem, o: &mut dyn Outside, u: u32) -> u32 {
    let cls = m.u32(SI_CID_OUTPUT_SHADERS);
    if u != 0 && si_udf_is_enabled(m, o, u) as u16 != 0 && m.u32(u.wrapping_add(4)) == cls {
        let sh = m.u32(u.wrapping_add(0x24));
        if sh != 0 {
            return m.u32(sh.wrapping_add(0x10));
        }
    }
    0
}

/// [soft 0x0854794c SI_udfListAdd]: grows the table of per-process udf lists to one 8-byte entry per
/// process (a fresh table of one entry when there is one process), clears the new last entry and answers the
/// process count; -1 (after "Insufficient memory for udf List storage.") when the allocation fails, the
/// table pointer then being null.
pub fn si_udf_list_add(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let n = m.u32(SI_NB_ELEM_DICTS);
    let p = if n == 1 { m.si_calloc(1, 8) } else { m.si_realloc(m.u32(UDF_LIST_POINTER), n.wrapping_shl(3)) };
    m.set_u32(UDF_LIST_POINTER, p);
    if p == 0 {
        o.call(m, SI_ERROR, &[0x46, 0x086e_4540]);
        return -1;
    }
    let last = p.wrapping_add(n.wrapping_mul(8)).wrapping_sub(8);
    m.set_u32(last, 0);
    m.set_u32(last.wrapping_add(4), 0);
    n as i32
}

/// [soft 0x085479d0 SI_udfListUpdate]: when the table exists, makes the current process's list and its
/// count the current `si_udfList` and `si_numUdf`.
pub fn si_udf_list_update(m: &mut Mem) {
    let lp = m.u32(UDF_LIST_POINTER);
    if lp != 0 {
        let idx = m.u32(m.u32(SI_PROC_DATA));
        let at = lp.wrapping_add(idx.wrapping_mul(8));
        let (l, c) = (m.u32(at), m.u32(at.wrapping_add(4)));
        m.set_u32(SI_UDF_LIST, l);
        m.set_u32(SI_NUM_UDF, c);
    }
}

/// [soft 0x08546324 shaderVerifyPicturePath]: looks for the picture `path` of a user texture. 0 for a null
/// path or one that exists as given. Otherwise the file name part is searched as `<database>/PICTURES/<name>`
/// (and with each sub directory name of the pictures chapter in front of it) in every database that is
/// loaded and either selected (+0x1a) or the input database; the first hit is announced
/// ("Shader will take picture [..] in database [..]") and the work buffer `SI_dBWorkFnameExt` holds its path:
/// answer 1. When none is found the replacement is announced and the work buffer holds `<resdir>/noIcon.pic`:
/// answer 2. `dict` is the element dictionary (its +4 the chapter table).
pub fn shader_verify_picture_path(m: &mut Mem, o: &mut dyn Outside, dict: u32, path: u32) -> i32 {
    let input_db = m.u32(SI_PDBIIN);
    if path == 0 || o.call(m, SI_DB_CHECK_FILE_EXISTENCE, &[path]).eax as u16 != 0 {
        return 0;
    }
    let msg = m.local(0x100);
    let name = m.local(0x100);
    let base = o.call(m, SI_DBFILENAME_REMOVE_PATH, &[path]).eax;
    clib::strncpy(m, name, base, 0x100);
    m.set_u8(name.wrapping_add(0xff), 0);
    let cls = m.u32(dict.wrapping_add(4)).wrapping_add(m.u32(SI_CID_PICTURES).wrapping_shl(6));
    let mut i = 0i32;
    let mut off = 0u32;
    let mut ret = None;
    while i < m.i32(SI_DBDIR) {
        let db = off.wrapping_add(m.u32(SI_DBDIR + 4));
        if m.u16(db.wrapping_add(0x18)) != 0 && (m.u16(db.wrapping_add(0x1a)) != 0 || db == input_db) {
            sprintf(m, WORK_FNAME, 0x086e_416c, &[m.u32(db.wrapping_add(0xc)), name]);
            if o.call(m, SI_DB_CHECK_FILE_EXISTENCE, &[WORK_FNAME]).eax as u16 != 0 {
                sprintf(m, msg, 0x086e_4180, &[name, m.u32(db.wrapping_add(8))]);
                show(m, o, msg);
                ret = Some(1);
                break;
            }
            o.call(m, SI_DB_SET_CHAPTER, &[db, m.u32(cls)]);
            o.call(m, SI_DBFILEPATH_BUILD, &[db]);
            o.call(m, SI_DB_CHAPTER_GET_SUB_DIRS, &[db, cls, 1]);
            let mut j = 0u32;
            while (j as i32) < m.i32(cls.wrapping_add(0x30)) {
                let sub = m.u32(m.u32(cls.wrapping_add(0x2c)).wrapping_add(j.wrapping_mul(4)));
                sprintf(m, WORK_FNAME, 0x086e_41af, &[m.u32(db.wrapping_add(0xc)), sub, name]);
                if o.call(m, SI_DB_CHECK_FILE_EXISTENCE, &[WORK_FNAME]).eax as u16 != 0 {
                    sprintf(m, msg, 0x086e_41c0, &[name, m.u32(db.wrapping_add(8))]);
                    show(m, o, msg);
                    ret = Some(1);
                    break;
                }
                j += 1;
            }
            if ret.is_some() {
                break;
            }
        }
        i += 1;
        off = off.wrapping_add(0x28);
    }
    let r = match ret {
        Some(r) => r,
        None => {
            sprintf(m, msg, 0x086e_4200, &[path]);
            show(m, o, msg);
            sprintf(m, WORK_FNAME, 0x086e_4234, &[m.u32(SI_RESDIR)]);
            2
        }
    };
    m.release(0x100);
    m.release(0x100);
    r
}

/// [soft 0x08547880 SI_sequenceGetImage]: the file of image `n` of the picture sequence `seq`, into `out`:
/// `<seq>.<n>.pic` looked up with `shaderVerifyPicturePath`; when that falls back to the default picture the
/// sequence name with its index stripped (SI_filenameStripIndex) is tried as `<name>.<n>.pic`; a found
/// picture's path (the work buffer) replaces `out`. Answers 1.
pub fn si_sequence_get_image(m: &mut Mem, o: &mut dyn Outside, seq: u32, n: u32, out: u32) -> i32 {
    sprintf(m, out, 0x086e_4509, &[seq, n]);
    let mut r = shader_verify_picture_path(m, o, cur_dict(m), out);
    if r == 2 {
        clib::strcpy(m, out, seq);
        if o.call(m, SI_FILENAME_STRIP_INDEX, &[out]).eax as u16 != 0 {
            sprintf(m, out, 0x086e_4513, &[out, n]);
            r = shader_verify_picture_path(m, o, cur_dict(m), out);
        }
    }
    if r != 0 {
        sprintf(m, out, 0x086e_451e, &[WORK_FNAME]);
    }
    1
}

/// [soft 0x0854609c SI_udfGenerateTextureDefs]: for every enabled udf (only `only` when given), every value
/// of its dialog list (else of its default list) whose shader symbol is a picture (kind 0x14) or a picture
/// sequence (0x15) with a name, writes the mental ray statement `color texture "_user_map_<udf>_<name>" "<file>"`
/// (preceded by `incremental ` for incremental mental ray 2 output); the file of a picture is
/// `<name>.pic` looked up with shaderVerifyPicturePath, of a sequence image `frame` of
/// SI_sequenceGetImage (an inaccessible one is reported on stderr); both are converted to the unix form unless
/// `only` is given. Ends with a blank line; answers 1.
/// Differs: with `only` given the picture case prints the file text from an uninitialised stack buffer in the
/// original; here it is empty.
pub fn si_udf_generate_texture_defs(m: &mut Mem, o: &mut dyn Outside, frame: u32, only: u32) -> i32 {
    let sym = m.local(4);
    let buf = m.local(0x1000);
    m.set_u8(buf, 0);
    let mut i = 0i32;
    while i < m.i32(SI_NUM_UDF) {
        let u = m.u32(m.u32(SI_UDF_LIST).wrapping_add((i as u32).wrapping_shl(2)));
        if only != 0 && u != only {
            i += 1;
            continue;
        }
        let en = si_udf_is_enabled(m, o, u) as u16;
        i += 1;
        if en == 0 {
            continue;
        }
        o.call(m, SI_UDF_PARSE_RAW_DATA, &[u]);
        let mut second = false;
        let mut v = m.u32(u.wrapping_add(0x2c));
        if v == 0 {
            second = true;
            v = m.u32(u.wrapping_add(0x30));
            if v == 0 {
                continue;
            }
        }
        loop {
            let name = m.u32(v);
            let kind = si_shader_get_symbol_type(m, m.u32(u.wrapping_add(0x24)), name, sym, 0);
            let s = m.u32(v.wrapping_add(0x18));
            if kind == 0x14 && o.call(m, SI_EMPTY_STRING, &[s]).eax as u16 == 0 {
                if only == 0 {
                    sprintf(m, buf, 0x086e_4076, &[s]);
                    if shader_verify_picture_path(m, o, cur_dict(m), buf) != 0 {
                        sprintf(m, buf, 0x086e_407e, &[WORK_FNAME]);
                    }
                    o.call(m, SI_MRFILE_CONVERT_TO_UNIX, &[buf, buf, 0x1000]);
                }
                incremental(m, o, 0x086e_4081);
                mr_printf(m, o, 0x086e_40a0, &[u, name, buf]);
            }
            if kind == 0x15 && o.call(m, SI_EMPTY_STRING, &[s]).eax as u16 == 0 {
                if si_sequence_get_image(m, o, s, frame, buf) as u16 == 0 {
                    crate::fprintf(m, o, m.u32(crate::STDERR), 0x086e_40e0, &[frame, s]);
                }
                if only == 0 {
                    o.call(m, SI_MRFILE_CONVERT_TO_UNIX, &[buf, buf, 0x1000]);
                }
                incremental(m, o, 0x086e_411d);
                mr_printf(m, o, 0x086e_4140, &[u, name, buf]);
            }
            v = m.u32(v.wrapping_add(0x44));
            if v == 0 {
                if !second {
                    v = m.u32(u.wrapping_add(0x30));
                    second = true;
                }
                if v == 0 {
                    break;
                }
            }
        }
    }
    mr_printf(m, o, 0x086e_4169, &[]);
    m.release(0x1000);
    m.release(4);
    1
}

fn incremental(m: &mut Mem, o: &mut dyn Outside, text: u32) {
    if o.call(m, SI_GETMI2_OUTPUT, &[]).eax as u16 != 0 && o.call(m, SI_MI2_IS_INCREMENTAL, &[]).eax as u16 != 0 {
        mr_printf(m, o, text, &[]);
    }
}
