// SPDX-License-Identifier: MIT
//! `mi2control.c`: the control interface of the mental ray 2 translator, driven by the command parser behind the
//! `-mi2` pipe (package 63, finding 0042). It opens a scene, connects the translator to a file or pipe, answers the
//! `query` commands on the output stream and closes again. Everything else (the database, the render set-up, the
//! live connection) is the callees' work and goes through [`Outside`].

use crate::mi_mr_main::{self as mm, ext, si_malloc_blk, sprintf_lit};
use si_mem::{clib, Mem, Outside, Routine};

pub const SI_TRANSLATE_IN_LIVE_CONNECTION: Routine = Routine::new("SI_translateInLiveConnection", 0x0858_5c74);
pub const SI_CLOSE_LIVE_CONNECTION: Routine = Routine::new("SI_closeLiveConnection", 0x0858_5ba0);
pub const WRITE: Routine = Routine::new("write", 0x080c_3a88);
pub const XSTAT: Routine = Routine::new("__xstat", 0x080c_4238);
pub const SI_CUS_CALL_CUSTOM_FUNCTIONS: Routine = Routine::new("SI_cusCallCustomFunctions", 0x0852_ea24);
pub const SI_DB_LOAD_SCENE: Routine = Routine::new("SI_dbLoadScene", 0x080d_a548);
pub const SI_DELETE_ALL: Routine = Routine::new("SI_deleteAll", 0x0841_e004);
pub const SI_INIT_STAND_ALONE_RENDER: Routine = Routine::new("SI_initStandAloneRender", 0x0842_f730);
pub const MI2_YYPARSE: Routine = Routine::new("mi2_yyparse", 0x0842_e1d0);

/// Globals of the image.
pub mod g {
    pub const SI_STD_INPUT: u32 = 0x0881_a700;
    pub const SI_STD_OUTPUT: u32 = 0x0881_a704;
    pub const RENDER_PARMS: u32 = 0x0881_a720;
    pub const PLM_MAT_DEFAULT: u32 = 0x0881_ad94;
    pub const MAXCOC: u32 = 0x0881_ad98;
    pub const SI_LIVE_CONNECTION: u32 = 0x086d_2e00; // 16 bits
    pub const MI2_PIPE_NAME: u32 = 0x086d_2e04;
    pub const SI_CUS_UPDATE_STATE: u32 = 0x086e_49ec;
    pub const SI_PROC_DATA: u32 = 0x0867_9314;
    pub const SI_ELEM_DICTS: u32 = 0x0867_931c;
    pub const SI_RESDIR: u32 = 0x0878_1628;
    pub const SI_DBDIR: u32 = 0x0878_1390;
    pub const SI_DBDIR_TABLE: u32 = 0x0878_1394;
    pub const SI_CID_SCENES: u32 = 0x0878_166c;
    pub const SI_P_DBIIN: u32 = 0x0878_2058;
    pub const SI_FRAME_CURRENT: u32 = 0x0878_2040;
    pub const SI_FRAME_END: u32 = 0x0878_2044;
    pub const SI_FRAME_START: u32 = 0x0878_2050;
    pub const STDERR: u32 = 0x0878_0c88;
    pub const W_878193C: u32 = 0x0878_193c; // 16 bits: the half-frame flag of the range query
    pub const W_87819F8: u32 = 0x0878_19f8; // 16 bits: the frame step
    pub const OUTPUT_NAME: u32 = 0x0878_197c;
    pub const PTR_8781980: u32 = 0x0878_1980;
    pub const PTR_8781984: u32 = 0x0878_1984;
    pub const W_87818FA: u32 = 0x0878_18fa;
    pub const W_8781BD8: u32 = 0x0878_1bd8;
    pub const W_8781BDA: u32 = 0x0878_1bda;
    pub const PTR_8781C94: u32 = 0x0878_1c94;
}

/// [soft 0x0842d570 writeBuffer]: `write(si_stdOutput, text, strlen(text))`; 0 for a null text.
pub fn write_buffer(m: &mut Mem, o: &mut dyn Outside, text: u32) -> u32 {
    if text == 0 {
        return 0;
    }
    let n = clib::strlen(m, text);
    let fd = m.u32(g::SI_STD_OUTPUT);
    o.call(m, WRITE, &[fd, text, n]).eax
}

/// [soft 0x0842d5d8 forwardSlashes]: every backslash of the string becomes a slash.
pub fn forward_slashes(m: &mut Mem, s: u32) {
    if s == 0 {
        return;
    }
    let mut p = s;
    while m.u8(p) != 0 {
        if m.u8(p) == b'\\' {
            m.set_u8(p, b'/');
        }
        p += 1;
    }
}

/// [soft 0x0842d5fc backSlashes]: every slash of the string becomes a backslash.
pub fn back_slashes(m: &mut Mem, s: u32) {
    if s == 0 {
        return;
    }
    let mut p = s;
    while m.u8(p) != 0 {
        if m.u8(p) == b'/' {
            m.set_u8(p, b'\\');
        }
        p += 1;
    }
}

/// [soft 0x0842d624 getFullPath]: `*out = strdup(path)`.
pub fn get_full_path(m: &mut Mem, path: u32, out: u32) {
    let p = clib::strdup(m, path);
    m.set_u32(out, p);
}

/// [soft 0x0842d644 databaseGetByPath]: finds the database directory entry (0x28 bytes each, path at +0xc) that names the same
/// file as `path` (same device and inode by `stat`); `*out` = the entry. 1 when found, else 0.
pub fn database_get_by_path(m: &mut Mem, o: &mut dyn Outside, path: u32, out: u32) -> u32 {
    if path == 0 || out == 0 {
        return 0;
    }
    let st = m.local(0x58);
    let st2 = m.local(0x58);
    let mut found = 0;
    if o.call(m, XSTAT, &[3, path, st]).eax == 0 {
        let (dev_lo, dev_hi, ino) = (m.u32(st), m.u32(st + 4), m.u32(st + 0xc));
        let mut i = 0i32;
        while i < m.i32(g::SI_DBDIR) {
            let table = m.u32(g::SI_DBDIR_TABLE);
            let ent = table.wrapping_add((i as u32).wrapping_mul(0x28));
            let name = m.u32(ent.wrapping_add(0xc));
            if o.call(m, XSTAT, &[3, name, st2]).eax == 0
                && ino == m.u32(st2 + 0xc)
                && dev_lo == m.u32(st2)
                && dev_hi == m.u32(st2 + 4)
            {
                m.set_u32(out, ent);
                found = 1;
                break;
            }
            i += 1;
        }
    }
    m.release(0x58);
    m.release(0x58);
    found
}

/// [soft 0x0842d7c0 extractPathSeparator]: the last `/` of the path, or null.
pub fn extract_path_separator(m: &mut Mem, path: u32) -> u32 {
    if path == 0 {
        0
    } else {
        clib::strrchr(m, path, b'/')
    }
}

/// [soft 0x0842d7e8 extractSceneAndDbName]: cuts a trailing `.dsc` off the path, then the part after the last `/` becomes
/// `*scene` (a new block; the path is cut there) and the path, cut again before a last component `SCENES` (any case), is copied to `*db`.
/// A path without `/` leaves `*scene` null.
pub fn extract_scene_and_db_name(m: &mut Mem, path: u32, scene: u32, db: u32) {
    m.set_u32(scene, 0);
    m.set_u32(db, 0);
    let dot = clib::strrchr(m, path, b'.');
    if dot != 0 && prefix_nocase(m, dot, b".dsc") {
        m.set_u8(dot, 0);
    }
    let sep = extract_path_separator(m, path);
    if sep != 0 {
        let src = sep + 1;
        let n = clib::strlen(m, src) + 1;
        let blk = si_malloc_blk(m, n);
        m.set_u32(scene, blk);
        clib::strncpy(m, blk, src, n);
        m.set_u8(sep, 0);
    }
    let sep = extract_path_separator(m, path);
    if sep != 0 && prefix_nocase(m, sep + 1, b"scenes") {
        m.set_u8(sep, 0);
    }
    let d = clib::strdup(m, path);
    m.set_u32(db, d);
}
/// `strncasecmp(p, lit, lit.len()) == 0` for a lower-case literal of the original's read-only data (the image has none).
fn prefix_nocase(m: &Mem, p: u32, lit: &[u8]) -> bool {
    lit.iter().enumerate().all(|(i, &c)| clib::tolower(m.u8(p + i as u32) as i32) as u8 == c)
}

/// [soft 0x0842d940 initRendering]: switches the translator to mental ray 2 output, resets the output name and the render set-up and
/// starts the sequence; 6 when the render set-up refuses (0x7f), 2 when the sequence could not start, else -1.
pub fn init_rendering(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    mm::si_set_mi2_output(m, 1);
    mm::si_set_mi2_pipe_connection(m);
    for slot in [g::PTR_8781980, g::PTR_8781984] {
        let p = m.u32(slot);
        if p != 0 {
            m.free(p);
            m.set_u32(slot, 0);
        }
    }
    m.set_u16(g::W_87818FA, 10);
    m.set_u16(g::W_8781BD8, 1);
    m.set_u16(g::W_8781BDA, 0);
    let name = m.u32(g::MI2_PIPE_NAME);
    let cur = m.u32(g::PTR_8781C94);
    if name != 0 {
        if cur != 0 {
            m.free(cur);
            m.set_u32(g::PTR_8781C94, 0);
        }
        let d = clib::strdup(m, name);
        m.set_u32(g::PTR_8781C94, d);
    } else if cur == 0 {
        let d = mm::strdup_lit(m, b"out.mi");
        m.set_u32(g::PTR_8781C94, d);
    }
    o.call(m, SI_CUS_CALL_CUSTOM_FUNCTIONS, &[0xe, 0x10]);
    m.set_u32(g::SI_CUS_UPDATE_STATE, 0xe);
    let r = o.call(m, ext::SI_PREPARE_RENDERING, &[g::RENDER_PARMS, g::PLM_MAT_DEFAULT, g::MAXCOC, 1]).eax;
    if r == 0x7f {
        return 6;
    }
    si_init_mental_pipe_parms(m, o, g::RENDER_PARMS);
    if o.call(m, ext::INIT_SEQUENCE_MENTAL, &[g::RENDER_PARMS, 1]).eax as u16 == 0 {
        2
    } else {
        u32::MAX
    }
}
pub use crate::mr_main::si_init_mental_pipe_parms;

/// [soft 0x0842da74 si_mi2InitLexYacc]: the parser reads stdin and writes stdout; always 1.
pub fn si_mi2_init_lex_yacc(m: &mut Mem) -> u32 {
    m.set_u32(g::SI_STD_INPUT, 0);
    m.set_u32(g::SI_STD_OUTPUT, 1);
    1
}

/// [soft 0x0842da94 SI_startCmdParser]: remembers whether the connection is live, sets up the streams and runs the parser; 1 when it ran.
pub fn si_start_cmd_parser(m: &mut Mem, o: &mut dyn Outside, live: u32, _a: u32, _b: u32) -> u32 {
    m.set_u16(g::SI_LIVE_CONNECTION, live as u16);
    if si_mi2_init_lex_yacc(m) as u16 != 0 {
        o.call(m, MI2_YYPARSE, &[]);
        1
    } else {
        0
    }
}

/// [soft 0x0842dad0 si_mi2SceneRender]: translates the frames `first..=last` in steps of `step` (field mode 0 to 3); 0x46 for bad numbers, else -1.
pub fn si_mi2_scene_render(m: &mut Mem, o: &mut dyn Outside, first: i32, last: i32, step: i32, field: i32) -> u32 {
    if last < first || step < 1 || !(0..=3).contains(&field) {
        return 0x46;
    }
    o.call(m, SI_TRANSLATE_IN_LIVE_CONNECTION, &[g::RENDER_PARMS, first as u32, last as u32, step as u32, field as u32]);
    u32::MAX
}

/// [soft 0x0842db20 si_mi2InitTranslator]: accepts only the version `"1.0"` (1 otherwise); finds the resource directory (`SI_resdir`, else
/// `$SI_LOCATION/3D/crsrc`) and starts the stand-alone render set-up. -1 when all went well, 0 without a resource directory, 6 when the set-up fails.
pub fn si_mi2_init_translator(m: &mut Mem, o: &mut dyn Outside, version: u32) -> u32 {
    if m.cstr(version) != b"\"1.0\"" {
        return 1;
    }
    if m.u16(g::SI_LIVE_CONNECTION) != 0 {
        return u32::MAX;
    }
    let mut dir = m.u32(g::SI_RESDIR);
    if dir == 0 {
        let loc = o.call(m, ext::GETENV, &[0x086d_2e21]).eax;
        if loc == 0 {
            o.print(2, b"SI_LOCATION path not set!\n");
            o.print(2, b"No way to determine rsrc path\n");
            return 0;
        }
        let n = clib::strlen(m, loc) + 9;
        dir = si_malloc_blk(m, n);
        sprintf_lit(m, dir, b"%s%c3D%crsrc", &[loc, 0x2f, 0x2f]);
        if o.call(m, ext::ACCESS, &[dir, 0]).eax != 0 {
            o.print(2, b"No way to determine rsrc path\n");
            m.free(dir);
            return 0;
        }
    }
    m.set_u32(g::SI_RESDIR, dir);
    if o.call(m, SI_INIT_STAND_ALONE_RENDER, &[dir]).eax as u16 == 1 {
        return u32::MAX;
    }
    o.print(2, b"Initialization failed. Check if Database.rsrc\nfile exists in the path specified by the SI_DBDIR \nenvironment variable.\n");
    6
}

/// [soft 0x0842dca8 si_mi2ExitTranslator]: always -1.
pub fn si_mi2_exit_translator() -> u32 {
    u32::MAX
}

/// The quoted-name step of the commands: skip a leading `"` and cut a trailing one in place (the byte before the string when it is empty).
fn unquote(m: &mut Mem, s: u32) -> u32 {
    let s = if m.u8(s) == b'"' { s + 1 } else { s };
    let last = s.wrapping_add(clib::strlen(m, s)).wrapping_sub(1);
    if m.u8(last) == b'"' {
        m.set_u8(last, 0);
    }
    s
}

/// [soft 0x0842dcb4 si_mi2OpenScene]: opens the scene named `path` (`database/SCENES/name.dsc`), loads it and starts rendering.
/// 0x30 when the file does not exist, 0x31 when it cannot be read, 0x32 for a database or scene that cannot be found, else what `initRendering` answers.
pub fn si_mi2_open_scene(m: &mut Mem, o: &mut dyn Outside, path: u32) -> u32 {
    let info = m.local(4);
    let names = m.local(8); // [scene, db]
    m.set_u32(info, 0);
    let path = unquote(m, path);
    let file = clib::strdup(m, path);
    let r = (|| {
        if o.call(m, ext::ACCESS, &[file, 0]).eax != 0 {
            o.print(2, format!("\nCannot load scene from {} \n", cs(m, file)).as_bytes());
            m.free(file);
            return 0x30;
        }
        if o.call(m, ext::ACCESS, &[file, 4]).eax != 0 {
            o.print(2, format!("\nNo permission for scene {} \n", cs(m, file)).as_bytes());
            m.free(file);
            return 0x31;
        }
        extract_scene_and_db_name(m, file, names, names + 4);
        m.free(file);
        let db = m.u32(names + 4);
        let scene = m.u32(names);
        let ok = database_get_by_path(m, o, db, info) as u16 != 0 && m.u16(m.u32(info) + 0x18) != 0;
        let text = if !ok {
            format!("ERROR: input database [{}] cannot be accessed\n", cs(m, db))
        } else {
            let pdb = m.u32(info);
            m.set_u32(g::SI_P_DBIIN, pdb);
            let procdata = m.u32(g::SI_PROC_DATA);
            let idx = m.u32(procdata);
            let dict = idx.wrapping_mul(7).wrapping_mul(16).wrapping_add(m.u32(g::SI_ELEM_DICTS));
            if o.call(m, SI_DB_LOAD_SCENE, &[dict, pdb, scene]).eax as u16 != 0 {
                if scene != 0 {
                    m.free(scene);
                    m.set_u32(names, 0);
                }
                m.free(m.u32(names + 4));
                return init_rendering(m, o);
            }
            let dbname = m.u32(m.u32(g::SI_P_DBIIN) + 8);
            format!("ERROR: Scene [{}] could not be found in database [{}]\n", cs(m, scene), cs(m, dbname))
        };
        o.print(2, text.as_bytes());
        for slot in [names, names + 4] {
            let p = m.u32(slot);
            if p != 0 {
                m.free(p);
                m.set_u32(slot, 0);
            }
        }
        0x32
    })();
    m.release(8);
    m.release(4);
    r
}

/// A C string for `%s` of glibc (a null pointer prints as `(null)`), as text.
fn cs(m: &Mem, p: u32) -> String {
    if p == 0 {
        "(null)".into()
    } else {
        m.cstr(p).iter().map(|&b| b as char).collect()
    }
}

/// [soft 0x0842deb4 si_mi2CloseScene]: closes the live connection and deletes the scene; -1.
pub fn si_mi2_close_scene(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let f = m.u32(g::MAXCOC);
    let a = m.u32(g::PLM_MAT_DEFAULT);
    o.call(m, SI_CLOSE_LIVE_CONNECTION, &[g::RENDER_PARMS, a, f]);
    o.call(m, SI_DELETE_ALL, &[]);
    u32::MAX
}

/// [soft 0x0842dee8 si_mi2ConnectToFile]: remembers the (unquoted) file or pipe name; starts rendering at once when the connection is live. -1 or what `initRendering` answers.
pub fn si_mi2_connect_to_file(m: &mut Mem, o: &mut dyn Outside, name: u32) -> u32 {
    let name = unquote(m, name);
    let d = clib::strdup(m, name);
    m.set_u32(g::MI2_PIPE_NAME, d);
    if m.u16(g::SI_LIVE_CONNECTION) == 0 {
        u32::MAX
    } else {
        init_rendering(m, o)
    }
}

/// [soft 0x0842df70 si_mi2Disconnect]: closes the live connection and forgets the file name; -1.
pub fn si_mi2_disconnect(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let f = m.u32(g::MAXCOC);
    let a = m.u32(g::PLM_MAT_DEFAULT);
    o.call(m, SI_CLOSE_LIVE_CONNECTION, &[g::RENDER_PARMS, a, f]);
    let p = m.u32(g::MI2_PIPE_NAME);
    if p != 0 {
        m.free(p);
        m.set_u32(g::MI2_PIPE_NAME, 0);
    }
    u32::MAX
}

/// Writes `text` (NUL-terminated copy in a local) with `writeBuffer`; -1 when something was written, else 0.
fn query_out(m: &mut Mem, o: &mut dyn Outside, buf: u32) -> u32 {
    if write_buffer(m, o, buf) != 0 {
        u32::MAX
    } else {
        0
    }
}

/// [soft 0x0842dfc0 si_mi2QueryRange]: `"first last step"` of the frame range (`first last.5 step` when the half-frame flag is set).
pub fn si_mi2_query_range(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let buf = m.local(0x80);
    let fmt: &[u8] = if m.u16(g::W_878193C) != 0 { b"%d %d.5 %d\n" } else { b"%d %d %d\n" };
    let step = m.i16(g::W_87819F8) as i32 as u32;
    sprintf_lit(m, buf, fmt, &[m.u32(g::SI_FRAME_START), m.u32(g::SI_FRAME_END), step]);
    let r = query_out(m, o, buf);
    m.release(0x80);
    r
}

/// [soft 0x0842e03c si_mi2QueryFrame]: the current frame number and a newline.
pub fn si_mi2_query_frame(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let buf = m.local(0x80);
    sprintf_lit(m, buf, b"%d\n", &[m.u32(g::SI_FRAME_CURRENT)]);
    let r = query_out(m, o, buf);
    m.release(0x80);
    r
}

/// [soft 0x0842e080 si_mi2QueryTranslator]: the translator's name and version in quotes.
pub fn si_mi2_query_translator(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let buf = m.local(0x80);
    let t = m.local(0x40);
    m.put_cstr(t, b"softimage 3D translator, version 1.0");
    sprintf_lit(m, buf, b"\"%s\"\n", &[t]);
    let r = query_out(m, o, buf);
    m.release(0x40);
    m.release(0x80);
    r
}

/// [soft 0x0842e0c0 si_mi2QueryName]: the name of the scene element 0 of the scene class, in quotes.
pub fn si_mi2_query_name(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let buf = m.local(0x200);
    let idx = m.u32(m.u32(g::SI_PROC_DATA));
    let dict = idx.wrapping_mul(7).wrapping_mul(16).wrapping_add(m.u32(g::SI_ELEM_DICTS));
    let elems = m.u32(dict + 4);
    let cid = m.u32(g::SI_CID_SCENES);
    let rec = m.u32(elems.wrapping_add(cid.wrapping_shl(6)).wrapping_add(8));
    let obj = m.u32(rec);
    let name = m.u32(obj.wrapping_add(0x24));
    // `%s` of a null pointer prints "(null)" in glibc, as the original does
    let t = format!("\"{}\"\n", cs(m, name));
    m.put_cstr(buf, t.as_bytes());
    let r = query_out(m, o, buf);
    m.release(0x200);
    r
}

/// [soft 0x0842e12c si_mi2QueryOutput]: the output file name in quotes; 0 when there is none.
pub fn si_mi2_query_output(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let name = m.u32(g::OUTPUT_NAME);
    if name == 0 {
        return 0;
    }
    let buf = m.local(0x200);
    sprintf_lit(m, buf, b"\"%s\"\n", &[name]);
    let r = query_out(m, o, buf);
    m.release(0x200);
    r
}

/// [soft 0x0842e174 si_mi2QueryCaps]: `"conn-p" "raccess" "open"`.
pub fn si_mi2_query_caps(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let buf = m.local(0x80);
    m.put_cstr(buf, b"\"conn-p\" \"raccess\" \"open\"\n");
    let r = query_out(m, o, buf);
    m.release(0x80);
    r
}
