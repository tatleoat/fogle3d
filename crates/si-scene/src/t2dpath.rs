// SPDX-License-Identifier: MIT
//! A 2D texture's picture looked for in the databases as the texture file is read
//! (`t2d_dial.c`; finding 0018, sections 19 and 23.10): `SI_texture2DVerifyPicturePath`
//! and `texture2DReplacePicPath`, on the texture record of the image.
//!
//! **Compared with the emulated original** (`re/tools/si-scene/emu_gen_edmat.py`, its
//! seventh part; `scene-emucheck re/out/si-scene/vec_edmat`): `texture2DReplacePicPath`,
//! `free` and `strdup` ran as they are; the database's routines, `SI_dbScnGetListNode`,
//! `SI_dbListNodeFree` and `SI_statusDisplayMessage` were recorded stand-ins
//! (`texture2DReplacePicPath` also on sample scenes' images, finding 0018, section 20).
//!
//! The port calls out by address ([`ByAddr`]). In the load it runs through an adapter
//! ([`verify_in_load`]): the list of databases is `si-db`'s value, not a record of the
//! image.

use crate::calls::{Channel, Message, Unported};
use crate::dbnode;
use crate::edtools::ByAddr;
use crate::globals::{SI_DB_WORK_FNAME, SI_DB_WORK_FNAME_EXT, SI_ED_SUMATRA_TO_3X};
use crate::load::Scene;
use crate::world::{list, Out};
use si_db::{DbInfo, FileSystem};
use si_dict::globals::SI_CID;
use si_mem::{Mem, Ptr32};
use si_types::dict::{Elem, ElemDict};

/// `SI_texturePath` (0x08679434) and `SI_resdir` (0x08781628): pointers to the two
/// directories the batch renderer's start-up sets (`standAloneRender`: `strdup(SI_cwd)`;
/// `si_runCreative`: the resource directory).
pub const SI_TEXTURE_PATH: u32 = 0x0867_9434;
pub const SI_RESDIR: u32 = 0x0878_1628;

/// The texture record's words the routines read (`__SI_LclMap`): the picture's name, the
/// name with its suffix, the type (16 bits: 0 a picture, 1 a sequence by frame number, 2 a
/// sequence named as it is).
const PIC_NAME: u32 = 0x14;
const PIC_NAME_EXT: u32 = 0x20;
const PIC_TYPE: u32 = 0x28;

/// A text of the image for `%s`: glibc prints `(null)` for a null pointer.
fn text(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 {
        b"(null)".to_vec()
    } else {
        m.cstr(p)
    }
}

fn strdup(m: &mut Mem, s: &[u8]) -> u32 {
    let p = m.malloc(s.len() as u32 + 1);
    m.put_cstr(p, s);
    p
}

/// [soft 0x082b1484 texture2DReplacePicPath] (held against the emulated original on sample
/// scenes' images, finding 0018, section 20): the texture's picture name (+0x14) and its
/// name with the suffix (+0x20) are freed and replaced by copies (`strdup`) of the two work
/// buffers, `SI_dBWorkFname` and `SI_dBWorkFnameExt`; for a PICTURES element given, the
/// string +0x08 of its name record is replaced by a copy of the first.
pub fn texture2d_replace_pic_path(m: &mut Mem, map: u32, e: u32) {
    for at in [map + PIC_NAME, map + PIC_NAME_EXT] {
        let p = m.u32(at);
        if p != 0 {
            m.free(p);
            m.set_u32(at, 0);
        }
    }
    let a = m.cstr(SI_DB_WORK_FNAME.addr());
    let b = m.cstr(SI_DB_WORK_FNAME_EXT.addr());
    let pa = strdup(m, &a);
    m.set_u32(map + PIC_NAME, pa);
    let pb = strdup(m, &b);
    m.set_u32(map + PIC_NAME_EXT, pb);
    if e != 0 {
        let node = m.u32(e);
        let old = m.u32(node + 8);
        if old != 0 {
            m.free(old);
            m.set_u32(node + 8, 0);
        }
        let p = strdup(m, &a);
        m.set_u32(node + 8, p);
    }
}

/// `SI_dbCheckFileExistence (0x08120f4c)`, `dbexternal.c`: (path); `si-db`'s
/// `check_file_existence`. Its answer is tested in 16 bits.
pub const SI_DB_CHECK_FILE_EXISTENCE: u32 = 0x0812_0f4c;
/// `SI_dbfilenameRemovePath (0x08121d84)`, `dbexternal.c`: (path), the C library's
/// `__xpg_basename`; `si-db`'s `filename_remove_path` (value form).
pub const SI_DBFILENAME_REMOVE_PATH: u32 = 0x0812_1d84;
/// `SI_dbScnGetListNode (0x081319f0)`: (name, 0, 1); `si-dict`'s
/// `elemname::db_scn_get_list_node`.
pub const SI_DB_SCN_GET_LIST_NODE: u32 = 0x0813_19f0;
/// `SI_dbListNodeFree (0x08122450)`: (name record); [`crate::dbnode::list_node_free`].
pub const SI_DB_LIST_NODE_FREE: u32 = 0x0812_2450;
/// `SI_dbSetChapter (0x08122654)`, `SI_dbfilepathBuild (0x081226cc)` and
/// `SI_dbChapterGetSubDirs (0x0812508c)`: (database, chapter's name), (database) and
/// (database, list, 1); `si-db`'s, on its `DbInfo` value.
pub const SI_DB_SET_CHAPTER: u32 = 0x0812_2654;
pub const SI_DBFILEPATH_BUILD: u32 = 0x0812_26cc;
pub const SI_DB_CHAPTER_GET_SUB_DIRS: u32 = 0x0812_508c;
/// `SI_statusDisplayMessage (0x083cca94)`: (text, 2).
pub use crate::custom::SI_STATUS_DISPLAY_MESSAGE;
/// `texture2DReplacePicPath (0x082b1484)`, `t2d_dial.c`: (texture, element);
/// [`texture2d_replace_pic_path`]. [`verify_picture_path`] calls it by address so that the
/// load's adapter ([`verify_in_load`]) sees which element's path it replaces; every caller
/// routes it to the port.
pub const TEXTURE2D_REPLACE_PIC_PATH: u32 = 0x082b_1484;

/// `%s` of `sprintf`: the text at `p`, `(null)` for a null pointer.
fn s(m: &Mem, p: u32) -> Vec<u8> {
    text(m, p)
}

/// `sprintf` into a work buffer of `dbexternal.c` (`SI_dBWorkFname` or `SI_dBWorkFnameExt`,
/// 0x400 bytes each, one after the other): written whole, as the original writes it.
fn put(m: &mut Mem, at: u32, parts: &[&[u8]]) {
    m.put_cstr(at, &parts.concat());
}

/// One look-up of [`verify_picture_path`]: `SI_dbCheckFileExistence(SI_dBWorkFnameExt)`,
/// tested in 16 bits.
fn exists<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, path: u32) -> bool {
    o.call_addr(m, SI_DB_CHECK_FILE_EXISTENCE, &[path]) as u16 != 0
}

/// The status line of a picture found: `sprintf(message, "%s: will take picture [%s] in
/// database [%s]", who, base, database's name)`, then `SI_statusDisplayMessage(message, 2)`.
/// The message is the first 0x100 bytes of the frame, the base name the next 0x100; a
/// longer message runs over the base name as in the original (cut at the frame's end,
/// where the original would overwrite its saved registers).
fn took<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, frame: u32, who: &[u8], db: u32) {
    let base = m.cstr(frame + 0x100);
    let name = s(m, m.u32(db + 8));
    let text = [who, b": will take picture [", &base, b"] in database [", &name, b"]"].concat();
    m.put_cstr(frame, &text[..text.len().min(0x1ff)]);
    o.call_addr(m, SI_STATUS_DISPLAY_MESSAGE, &[frame, 2]);
}

/// The databases of the list at `dir` (`SI_DbDir`: a count, then an array of 40-byte
/// `SI_DbInfo`) that [`verify_picture_path`] searches: valid (+0x18) and either a system
/// one (+0x1a) or `db` itself; answers the record of the `k`-th, if it is one.
fn searched(m: &Mem, dir: u32, db: u32, k: i32) -> Option<u32> {
    let d = m.u32(dir + 4).wrapping_add(k as u32 * 0x28);
    (m.i16(d + 0x18) != 0 && (m.i16(d + 0x1a) != 0 || d == db)).then_some(d)
}

/// The three places of a database [`verify_picture_path`] tries, in its order, each as
/// `SI_dBWorkFname` and with `.pic` as `SI_dBWorkFnameExt`: `<path>/PICTURES/<base>`,
/// `<path>/PICTURES/<base>.1-0`, then, after `SI_dbSetChapter(database, the list's name)`,
/// `SI_dbfilepathBuild(database)` and `SI_dbChapterGetSubDirs(database, list, 1)`, the same
/// two in each sub-directory of the list (+0x2c, +0x30: the count read again at each
/// step). Answers which one exists: 0 the first, 1 the second, 2 one of the
/// sub-directories'.
fn places<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, d: u32, l: u32, base: &[u8]) -> Option<u8> {
    let (fname, ext) = (SI_DB_WORK_FNAME.addr(), SI_DB_WORK_FNAME_EXT.addr());
    let path = s(m, m.u32(d + 0xc));
    for (k, tail) in [(0, &b""[..]), (1, &b".1-0"[..])] {
        put(m, fname, &[&path, b"/PICTURES/", base, tail]);
        put(m, ext, &[&m.cstr(fname), b".pic"]);
        if exists(o, m, ext) {
            return Some(k);
        }
    }
    let chapter = m.u32(l);
    o.call_addr(m, SI_DB_SET_CHAPTER, &[d, chapter]);
    o.call_addr(m, SI_DBFILEPATH_BUILD, &[d]);
    o.call_addr(m, SI_DB_CHAPTER_GET_SUB_DIRS, &[d, l, 1]);
    let mut j = 0i32;
    while j < m.i32(l + 0x30) {
        for tail in [&b""[..], &b".1-0"[..]] {
            let path = s(m, m.u32(d + 0xc));
            let sub = s(m, m.u32(m.u32(l + 0x2c).wrapping_add(4 * j as u32)));
            put(m, fname, &[&path, b"/PICTURES/", &sub, base, tail]);
            put(m, ext, &[&m.cstr(fname), b".pic"]);
            if exists(o, m, ext) {
                return Some(2);
            }
        }
        j += 1;
    }
    None
}

/// Looks for a 2D texture's picture in the databases [soft 0x082b1528
/// SI_texture2DVerifyPicturePath]`(databases, database, dictionary, element, again, copy)`:
/// 1 when the picture is there (or for a null element or one without a texture), 0
/// otherwise. `again` and `copy` are 16 bits; `who` below is `Pic Copy` with `copy`, else
/// `T2D`.
///
/// - **A sequence** (+0x28 = 2, 16 bits): `SI_dbCheckFileExistence(the name +0x14)`.
/// - **Otherwise** `<name>.pic` (in `SI_dBWorkFnameExt`), then, for a name not starting
///   with `/`, `<SI_texturePath>/<name>` and its `.pic`: found there, the texture's names
///   are replaced ([`texture2d_replace_pic_path`], no element).
/// - **The base name**: `SI_dbfilenameRemovePath(name)`, copied (`strncpy`, 0x100 bytes,
///   the last one cleared) into the frame; `SI_dbScnGetListNode(base, 0, 1)` makes a name
///   record. Each element of PICTURES (`SI_cid`'s, read once) in use (+0x48) and new to
///   this load (+0x46) whose record's name (+0x14) is the record's (`strcmp`) is "matched":
///   each database searched ([`searched`], [`places`]); found, the names and the element's
///   path are replaced and the status line says so. Then the element's own path (+0x08 of
///   its record) and its `.pic`: found, the names are replaced, no message. Only the first
///   place of a database and the element's own path free the name record before answering
///   1; the other places leave it (a leak of the original).
/// - **No element matched**: the record is freed, and each database searched the same way
///   with no element; while `SI_edSumatraTo3x` is set (16 bits), a database whose path has
///   `Shader_Lib` and a name that has it first try `<path><what follows Shader_Lib in the
///   name>`.
/// - **Once more** (`again` 0): a name with a `.` is cut there in place
///   (`strrchr`), whatever the answer, and looked for again with `again` 1. A name without
///   one, of type 1, becomes `<name>.<SI_frameCurrent>` (old one freed, a `strdup` of
///   `SI_dBWorkFname`) and is looked for again. Found, 1.
/// - **Last**, for type 0, no second look and no `copy`: `<who>: will replace unfound
///   picture [<name>] by default` on the status line, the names become
///   `<SI_resdir>/noIcon` and its `.pic`, and +0x2e (16 bits) is cleared. 0.
#[allow(clippy::too_many_arguments)]
pub fn verify_picture_path<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dir: u32, db: u32, ed: u32, e: u32, again: i16, copy: i16) -> i32 {
    if e == 0 {
        return 1;
    }
    let map = m.u32(e + 4);
    if map == 0 {
        return 1;
    }
    let who: &[u8] = if copy != 0 { b"Pic Copy" } else { b"T2D" };
    let (fname, ext) = (SI_DB_WORK_FNAME.addr(), SI_DB_WORK_FNAME_EXT.addr());
    if m.i16(map + PIC_TYPE) == 2 {
        let name = m.u32(map + PIC_NAME);
        return exists(o, m, name) as i32;
    }
    put(m, ext, &[&s(m, m.u32(map + PIC_NAME)), b".pic"]);
    if exists(o, m, ext) {
        return 1;
    }
    let name = m.u32(map + PIC_NAME);
    if m.u8(name) != b'/' {
        put(m, fname, &[&s(m, m.u32(SI_TEXTURE_PATH)), b"/", &s(m, name)]);
        put(m, ext, &[&m.cstr(fname), b".pic"]);
        if exists(o, m, ext) {
            o.call_addr(m, TEXTURE2D_REPLACE_PIC_PATH, &[map, 0]);
            return 1;
        }
    }
    // the frame: the message (0x100 bytes), then the base name (0x100)
    let frame = m.local(0x200);
    let base_at = frame + 0x100;
    let name = m.u32(map + PIC_NAME);
    let base = o.call_addr(m, SI_DBFILENAME_REMOVE_PATH, &[name]);
    si_mem::clib::strncpy(m, base_at, base, 0x100);
    m.set_u8(base_at + 0xff, 0);
    let record = o.call_addr(m, SI_DB_SCN_GET_LIST_NODE, &[base_at, 0, 1]);
    let base = m.cstr(base_at);
    let mut matched = false;
    let l = (m[SI_CID].img as u32).wrapping_mul(0x40).wrapping_add(m.u32(ed + 4));
    let mut i = 0i32;
    while i < m.i32(l + 4) {
        let pe = m.u32(l + 8).wrapping_add(i as u32 * 0x84);
        if m.i16(pe + 0x48) == 0 && m.i16(pe + 0x46) != 0 {
            let node = m.u32(pe);
            if si_mem::clib::strcmp(m, m.u32(node + 0x14), m.u32(record + 0x14)) == 0 {
                matched = true;
                let mut k = 0i32;
                while k < m.i32(dir) {
                    if let Some(d) = searched(m, dir, db, k) {
                        if let Some(at) = places(o, m, d, l, &base) {
                            o.call_addr(m, TEXTURE2D_REPLACE_PIC_PATH, &[map, pe]);
                            took(o, m, frame, who, d);
                            if at == 0 {
                                o.call_addr(m, SI_DB_LIST_NODE_FREE, &[record]);
                            }
                            m.release(0x200);
                            return 1;
                        }
                    }
                    k += 1;
                }
                let own = s(m, m.u32(node + 8));
                put(m, fname, &[&own]);
                put(m, ext, &[&own, b".pic"]);
                if exists(o, m, ext) {
                    o.call_addr(m, TEXTURE2D_REPLACE_PIC_PATH, &[map, pe]);
                    o.call_addr(m, SI_DB_LIST_NODE_FREE, &[record]);
                    m.release(0x200);
                    return 1;
                }
            }
        }
        i += 1;
    }
    o.call_addr(m, SI_DB_LIST_NODE_FREE, &[record]);
    if !matched {
        let mut k = 0i32;
        while k < m.i32(dir) {
            if let Some(d) = searched(m, dir, db, k) {
                if m[SI_ED_SUMATRA_TO_3X] != 0 {
                    let lib = |m: &Mem, p: u32| {
                        let t = m.cstr(p);
                        t.windows(10).position(|w| w == b"Shader_Lib").map(|i| p + i as u32)
                    };
                    let (in_path, in_name) = (lib(m, m.u32(d + 0xc)), lib(m, m.u32(map + PIC_NAME)));
                    if let (Some(_), Some(at)) = (in_path, in_name) {
                        put(m, fname, &[&s(m, m.u32(d + 0xc)), &m.cstr(at + 10)]);
                        put(m, ext, &[&m.cstr(fname), b".pic"]);
                        if exists(o, m, ext) {
                            o.call_addr(m, TEXTURE2D_REPLACE_PIC_PATH, &[map, 0]);
                            took(o, m, frame, who, d);
                            m.release(0x200);
                            return 1;
                        }
                    }
                }
                if places(o, m, d, l, &base).is_some() {
                    o.call_addr(m, TEXTURE2D_REPLACE_PIC_PATH, &[map, 0]);
                    took(o, m, frame, who, d);
                    m.release(0x200);
                    return 1;
                }
            }
            k += 1;
        }
    }
    let mut retried = false;
    let name = m.u32(map + PIC_NAME);
    let dot = si_mem::clib::strrchr(m, name, b'.');
    if dot != 0 && again == 0 {
        m.set_u8(dot, 0);
        if verify_picture_path(o, m, dir, db, ed, e, 1, copy) != 0 {
            m.release(0x200);
            return 1;
        }
        retried = true;
    } else if dot == 0 && m.i16(map + PIC_TYPE) == 1 && again == 0 {
        let frame_no = m[crate::globals::SI_FRAME_CURRENT];
        put(m, fname, &[&s(m, m.u32(map + PIC_NAME)), b".", frame_no.to_string().as_bytes()]);
        let old = m.u32(map + PIC_NAME);
        if old != 0 {
            m.free(old);
            m.set_u32(map + PIC_NAME, 0);
        }
        let copy_ = m.strdup(fname);
        m.set_u32(map + PIC_NAME, copy_);
        if verify_picture_path(o, m, dir, db, ed, e, 1, copy) != 0 {
            m.release(0x200);
            return 1;
        }
        retried = true;
    }
    if m.i16(map + PIC_TYPE) == 0 && !retried && copy == 0 {
        let shown = s(m, m.u32(map + PIC_NAME));
        let text = [who, b": will replace unfound picture [", &shown, b"] by default"].concat();
        m.put_cstr(frame, &text[..text.len().min(0x1ff)]);
        o.call_addr(m, SI_STATUS_DISPLAY_MESSAGE, &[frame, 2]);
        put(m, fname, &[&s(m, m.u32(SI_RESDIR)), b"/noIcon"]);
        put(m, ext, &[&m.cstr(fname), b".pic"]);
        o.call_addr(m, TEXTURE2D_REPLACE_PIC_PATH, &[map, 0]);
        m.set_i16(map + 0x2e, 0);
    }
    m.release(0x200);
    0
}

/// The C library's `__xpg_basename(path)`, what `SI_dbfilenameRemovePath` is: the last
/// component, in the path itself. A path ending in `/` has its run of trailing slashes
/// cut **in place** (a terminator written over the first of them) and the component
/// before it answered; a path of slashes only answers itself. For a null or empty path,
/// `dot` (the C library's own static `"."`, which is not in the image).
fn xpg_basename(m: &mut Mem, path: u32, dot: u32) -> u32 {
    if path == 0 || m.u8(path) == 0 {
        return dot;
    }
    let text = m.cstr(path);
    let Some(last) = text.iter().rposition(|&c| c == b'/') else {
        return path;
    };
    if last + 1 < text.len() {
        return path + last as u32 + 1;
    }
    let mut p = last;
    while p > 0 && text[p - 1] == b'/' {
        p -= 1;
    }
    if p == 0 {
        return path;
    }
    m.set_u8(path + p as u32, 0);
    p -= 1;
    while p > 0 && text[p - 1] != b'/' {
        p -= 1;
    }
    path + p as u32
}

// adapter: not in place (the database list is si-db's value)
/// How [`verify_picture_path`] calls out in the load: the database's routines on `si-db`'s
/// `DbInfo` value and the load's file system, the name records' routines on the image
/// (`si-dict`'s and [`dbnode`]'s), the status line into the scene's messages. Each
/// PICTURES element whose path `texture2DReplacePicPath` replaces is noted with the path it
/// had (`pictures_replaced`, for [`file_paths_in_stores`]).
struct LoadSide<'a, 'b> {
    cx: &'a mut Ctx<'b>,
    /// The PICTURES list's elements, for the element's number.
    elems: u32,
    /// A `"."` in the frame of [`verify_in_load`].
    dot: u32,
}

impl ByAddr for LoadSide<'_, '_> {
    fn call_addr(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
        let a = |k: usize| args.get(k).copied().unwrap_or(0);
        match f {
            SI_DB_CHECK_FILE_EXISTENCE => self.cx.exists(&m.cstr(a(0))) as u32,
            SI_DBFILENAME_REMOVE_PATH => xpg_basename(m, a(0), self.dot),
            SI_DB_SCN_GET_LIST_NODE => {
                let out = &mut Out { sc: &mut *self.cx.sc, un: &mut *self.cx.un };
                si_dict::elemname::db_scn_get_list_node(m, out, Ptr32::new(a(0)), Ptr32::new(a(1)), a(2) as i16).addr()
            }
            SI_DB_LIST_NODE_FREE => {
                dbnode::list_node_free(m, Ptr32::new(a(0)));
                0
            }
            SI_DB_SET_CHAPTER => {
                self.cx.info.set_chapter(&m.cstr(a(1)));
                0
            }
            SI_DBFILEPATH_BUILD => {
                self.cx.info.filepath_build();
                0
            }
            SI_DB_CHAPTER_GET_SUB_DIRS => {
                crate::load::chapter_get_sub_dirs(m, self.cx.sc, self.cx.fs, self.cx.info, Ptr32::new(a(1)));
                0
            }
            SI_STATUS_DISPLAY_MESSAGE => {
                self.cx.status(m.cstr(a(0)));
                0
            }
            TEXTURE2D_REPLACE_PIC_PATH => {
                let pe = a(1);
                if pe != 0 {
                    // for the checks: what the element's path was
                    let node = m.u32(pe);
                    let before = (m.u32(node + 8) != 0).then(|| m.cstr(m.u32(node + 8)));
                    let i = (pe.wrapping_sub(self.elems) / 0x84) as i32;
                    self.cx.sc.contents.pictures_replaced.push((i, before));
                }
                texture2d_replace_pic_path(m, a(0), pe);
                0
            }
            _ => unreachable!("SI_texture2DVerifyPicturePath calls nothing else ({f:#x})"),
        }
    }
}

// adapter: not in place (the database list is si-db's value)
/// [`verify_picture_path`]`(&SI_dbdir, database, dictionary, element, 0, 0)` as the load
/// makes it, on the texture record of the image.
///
/// **The list of databases is `si-db`'s value**, not a record of the image: the scene's
/// database (`DbInfo`) stands for the whole list `SI_dbdir` (0x08781390), as the
/// original's batch runs had it (one database in `DatabaseDir.rsrc`, the one the scene is
/// rendered from: `re/tools/oracle/inside.sh`), and for the current one (`SI_pDbiin`). For
/// the call, a list of that one database is laid out in a frame of the image: its name
/// (+0x08), path (+0x0c), valid mark (+0x18) and system mark (+0x1a) are the value's; the
/// chapter the search sets (`SI_dbSetChapter`, `SI_dbfilepathBuild`) is set on the value.
pub fn verify_in_load(cx: &mut Ctx, m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>) -> i32 {
    let (name, path) = (cx.info.name.clone(), cx.info.path.clone());
    let len = |s: &Option<Vec<u8>>| s.as_ref().map_or(0, |s| s.len() as u32 + 1);
    let size = (8 + 0x28 + 4 + len(&name) + len(&path) + 3) & !3;
    let frame = m.local(size);
    let (dir, db, dot) = (frame, frame + 8, frame + 0x30);
    m.set_bytes(dir, &[0; 0x34]);
    m.put_cstr(dot, b".");
    m.set_u32(dir, 1);
    m.set_u32(dir + 4, db);
    let mut at = frame + 0x34;
    for (field, s) in [(8, &name), (0xc, &path)] {
        if let Some(s) = s {
            m.put_cstr(at, s);
            m.set_u32(db + field, at);
            at += s.len() as u32 + 1;
        }
    }
    m.set_i16(db + 0x18, cx.info.valid);
    m.set_i16(db + 0x1a, cx.info.system);
    let elems = m[list(m, ed, m[SI_CID].img)].elems.addr();
    let r = verify_picture_path(&mut LoadSide { cx, elems, dot }, m, dir, db, ed.addr(), e.addr(), 0, 0);
    m.release(size);
    r
}

/// The context of one call: the scene, the file system, the database.
pub struct Ctx<'a> {
    pub sc: &'a mut Scene,
    pub fs: &'a dyn FileSystem,
    pub info: &'a mut DbInfo,
    pub un: &'a mut dyn Unported,
}

impl Ctx<'_> {
    fn exists(&self, path: &[u8]) -> bool {
        si_db::check_file_existence(self.fs, path)
    }

    fn status(&mut self, text: Vec<u8>) {
        self.sc.messages.push(Message { channel: Channel::Status(2), text });
    }
}

// adapter: not in place (kept for `SCENE_NO_MAT_CONVERSION=1`: the load without the conversion)
/// The picture search of the 2D textures a model node holds **without an element**, made
/// where the loader calls `SI_dbCheckConvertModelsMatTxt` (0x08109664, load step 16) when
/// `SCENE_NO_MAT_CONVERSION=1` leaves that call recorded; otherwise the load runs the port
/// ([`crate::matconv::db_check_convert_models_mat_txt`]) and this is not called. Answers
/// how many textures were searched for.
///
/// A reading, not a port: in the original the conversion `SI_dbConvertModelMatTxt2DB`
/// (0x08108490) makes each such texture an element (`SI_graflistAddData(…, 0xf, 0)` →
/// `SI_edAddTexture2D` 0x08102314), and `SI_edAddTexture2D` ends with
/// `SI_texture2DVerifyPicturePath(&SI_dbdir, SI_pDbiin, dictionary, new element, 0, 0)`
/// (decompile; `soft.calls.tsv`). Evidence that it runs in the batch renderer: the oracle's
/// CubicMap-House run logs `T2D: will take picture [Brick] in database [3D_Extreme_FX]`
/// although the scene has no TEXTURES_2D element (its texture is in a model node). Only the
/// search is made here ([`verify_in_load`], the port), on a stand-in element whose object is
/// the texture (as `SI_texture2DVerifyTexturePath` 0x082b2060 hands one:
/// `SI_elemdictInitElem`, then the object): no element is added, nothing else of the
/// conversion is done.
///
/// The walk is the conversion's with `SI_edSumatraTo3x` 0: each MODELS element in use (+0x48
/// 0) at depth 0 (+0x54), new flag (+0x46) 1, with an object: the node, then (recursively)
/// its first son (+0x2c4), then its next brother (+0x2c0); in each node each material's
/// (+0x20c, +0x206 of them) local 2D textures (+0x6ec, linked by +0x16c), then the node's
/// own 2D textures (+0x21c). A texture that is an element's object is left (`param_2` 0),
/// and so is one already searched for (it would be an element by then). Nothing while
/// `SI_addInDictionary` is 0 (`SI_edAddTexture2D` adds nothing then).
pub fn verify_node_textures(cx: &mut Ctx, m: &mut Mem, ed: Ptr32<ElemDict>) -> usize {
    use si_mem::handle::Kind;
    if m[crate::globals::SI_ADD_IN_DICTIONARY] == 0 {
        return 0;
    }
    let ours = |p: u32| p != 0 && Kind::of(p).is_none();
    let t2d = list(m, ed, m[SI_CID].t2d);
    let models = list(m, ed, m[SI_CID].model);
    let mut searched = std::collections::HashSet::new();
    let mut nodes_seen = std::collections::HashSet::new();
    let mut n = 0;
    for i in 0..m[models].nb_elems {
        let me = m[models].elems.add(i);
        if !(m[me].status == 0 && m[me].hier_level == 0 && m[me].new_flag == 1 && ours(m[me].data.addr())) {
            continue;
        }
        let mut stack = vec![m[me].data.addr()];
        while let Some(node) = stack.pop() {
            if !ours(node) || !nodes_seen.insert(node) {
                continue;
            }
            let mut maps = Vec::new();
            let materials = m.u32(node + 0x20c);
            if ours(materials) {
                for k in 0..m.i16(node + 0x206).max(0) as u32 {
                    let mat = m.u32(materials + 4 * k);
                    let mut map = if ours(mat) { m.u32(mat + 0x6ec) } else { 0 };
                    while ours(map) && !maps.contains(&map) {
                        maps.push(map);
                        map = m.u32(map + 0x16c);
                    }
                }
            }
            let mut map = m.u32(node + 0x21c);
            while ours(map) && !maps.contains(&map) {
                maps.push(map);
                map = m.u32(map + 0x16c);
            }
            for map in maps {
                let has_elem = (0..m[t2d].nb_elems).any(|j| {
                    let te = m[t2d].elems.add(j);
                    m[te].status == 0 && m[te].data.addr() == map
                });
                if has_elem || !searched.insert(map) {
                    continue;
                }
                let e: Ptr32<Elem> = Ptr32::new(m.local(0x84));
                si_dict::elemdict::init_elem(m, &mut Out { sc: &mut *cx.sc, un: &mut *cx.un }, e);
                m[e].data = Ptr32::new(map);
                verify_in_load(cx, m, ed, e);
                m.release(0x84);
                *cx.sc.contents.ported.entry("SI_texture2DVerifyPicturePath").or_default() += 1;
                n += 1;
            }
            // the brother after the son's whole branch
            for next in [m.u32(node + 0x2c0), m.u32(node + 0x2c4)] {
                if next != 0 {
                    stack.push(next);
                }
            }
        }
    }
    n
}

// adapter: not in place (for the checks of the writer)
/// Puts back, **in the stores only** (`Scene::dict`, not the image), the PICTURES elements'
/// paths the load's search replaced ([`verify_in_load`]): the original's save
/// writes the path where the picture was found, the sample files have the path of the
/// machine that saved them; a check of the writer against those files compares with what
/// the file's machine had. Answers how many paths were put back.
pub fn file_paths_in_stores(sc: &mut Scene) -> usize {
    let img = sc.program.cid.img;
    let mut n = 0;
    // the first path an element had is the file's: put back in reverse order
    for (i, before) in std::mem::take(&mut sc.contents.pictures_replaced).into_iter().rev() {
        let Some(slot) = sc.dict.slot(crate::dict::ElemId::new(img, i)) else { continue };
        let h = slot.rec.node.0;
        if let Some(node) = sc.dict.nodes.get_mut(h) {
            node.sub_path = before;
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The files that exist; every call logged. `SI_dbfilenameRemovePath` answers what
    /// follows the last `/`, `SI_dbScnGetListNode` a record whose name (+0x14) is the text
    /// it was given.
    struct Disk(Vec<&'static [u8]>, Vec<(u32, Vec<u8>)>);
    impl ByAddr for Disk {
        fn call_addr(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
            let texts = [SI_DB_CHECK_FILE_EXISTENCE, SI_DBFILENAME_REMOVE_PATH, SI_DB_SCN_GET_LIST_NODE, SI_STATUS_DISPLAY_MESSAGE];
            let text = if texts.contains(&f) && args[0] != 0 { m.cstr(args[0]) } else { Vec::new() };
            self.1.push((f, text.clone()));
            match f {
                TEXTURE2D_REPLACE_PIC_PATH => {
                    texture2d_replace_pic_path(m, args[0], args[1]);
                    0
                }
                SI_DB_CHECK_FILE_EXISTENCE => self.0.iter().any(|p| *p == text.as_slice()) as u32,
                SI_DBFILENAME_REMOVE_PATH => args[0] + text.iter().rposition(|&c| c == b'/').map_or(0, |i| i as u32 + 1),
                SI_DB_SCN_GET_LIST_NODE => {
                    let record = m.calloc(1, 0x34);
                    let name = m.strdup(args[0]);
                    m.set_u32(record + 0x14, name);
                    record
                }
                _ => 0,
            }
        }
    }

    fn string(m: &mut Mem, text: &[u8]) -> u32 {
        let p = m.malloc(text.len() as u32 + 1);
        m.put_cstr(p, text);
        p
    }

    /// A dictionary whose PICTURES (chapter 1) has one element in use, new to the load,
    /// named `Brick`, at `own`; one database, valid, at `/db`; the texture `name` of type
    /// `kind` in an element. Answers (image, databases, database, dictionary, element,
    /// texture, the PICTURES element's name record).
    fn world(name: &[u8], kind: i16) -> (Mem, u32, u32, u32, u32, u32, u32) {
        let mut m = Mem::new();
        si_mem::init_globals(&mut m, si_dict::globals::ALL);
        crate::globals::store(&mut m);
        let mut cid = m.read(SI_CID);
        cid.img = 1;
        m.write(SI_CID, cid);
        let node = m.calloc(1, 0x34);
        let (own, brick) = (string(&mut m, b"/own/Brick"), string(&mut m, b"Brick"));
        m.set_u32(node + 8, own);
        m.set_u32(node + 0x14, brick);
        let elems = m.calloc(1, 0x84);
        m.set_u32(elems, node);
        m.set_i16(elems + 0x46, 1);
        let lists = m.calloc(2, 0x40);
        let pictures = string(&mut m, b"PICTURES");
        m.set_u32(lists + 0x40, pictures);
        m.set_u32(lists + 0x44, 1);
        m.set_u32(lists + 0x48, elems);
        let ed = m.calloc(1, 0x70);
        m.set_u32(ed + 4, lists);
        let db = m.calloc(1, 0x28);
        let (db_name, db_path) = (string(&mut m, b"3D_Extreme_FX"), string(&mut m, b"/db"));
        m.set_u32(db + 8, db_name);
        m.set_u32(db + 0xc, db_path);
        m.set_i16(db + 0x18, 1);
        let dir = m.calloc(1, 8);
        m.set_u32(dir, 1);
        m.set_u32(dir + 4, db);
        let texture = m.calloc(1, 0x1d8);
        let picture = string(&mut m, name);
        m.set_u32(texture + 0x14, picture);
        m.set_i16(texture + 0x28, kind);
        m.set_i16(texture + 0x2e, 9);
        let e = m.calloc(1, 0x84);
        m.set_u32(e + 4, texture);
        let res = string(&mut m, b"/res");
        m.set_u32(SI_RESDIR, res);
        let tp = string(&mut m, b"/cwd");
        m.set_u32(SI_TEXTURE_PATH, tp);
        (m, dir, db, ed, e, texture, node)
    }

    fn called(o: &Disk, f: u32) -> Vec<Vec<u8>> {
        o.1.iter().filter(|c| c.0 == f).map(|c| c.1.clone()).collect()
    }

    #[test]
    fn a_picture_found_in_the_database_takes_the_element_s_path_and_leaves_the_record() {
        let (mut m, dir, db, ed, e, texture, node) = world(b"maps/Brick", 0);
        let mut o = Disk(vec![b"/db/PICTURES/Brick.1-0.pic"], Vec::new());
        assert_eq!(verify_picture_path(&mut o, &mut m, dir, db, ed, e, 0, 0), 1);
        assert_eq!(
            called(&o, SI_DB_CHECK_FILE_EXISTENCE),
            [&b"maps/Brick.pic"[..], b"/cwd/maps/Brick.pic", b"/db/PICTURES/Brick.pic", b"/db/PICTURES/Brick.1-0.pic"]
        );
        assert_eq!(called(&o, SI_DB_SCN_GET_LIST_NODE), [b"Brick".to_vec()]);
        assert_eq!(called(&o, SI_STATUS_DISPLAY_MESSAGE), [b"T2D: will take picture [Brick] in database [3D_Extreme_FX]".to_vec()]);
        // the second place found: the name record is not freed
        assert!(called(&o, SI_DB_LIST_NODE_FREE).is_empty());
        let (a, b, c) = (m.cstr(m.u32(texture + 0x14)), m.cstr(m.u32(texture + 0x20)), m.cstr(m.u32(node + 8)));
        assert_eq!((a.as_slice(), b.as_slice(), c.as_slice()), (&b"/db/PICTURES/Brick.1-0"[..], &b"/db/PICTURES/Brick.1-0.pic"[..], &b"/db/PICTURES/Brick.1-0"[..]));
    }

    #[test]
    fn an_unfound_name_is_cut_at_its_dot_and_replaced_by_the_default() {
        let (mut m, dir, db, ed, e, texture, node) = world(b"wood.x", 0);
        let mut o = Disk(Vec::new(), Vec::new());
        assert_eq!(verify_picture_path(&mut o, &mut m, dir, db, ed, e, 0, 1), 0);
        // with `copy`: no message, the names left (cut in place)
        assert!(called(&o, SI_STATUS_DISPLAY_MESSAGE).is_empty());
        assert_eq!(m.cstr(m.u32(texture + 0x14)), b"wood");
        assert_eq!(called(&o, SI_DB_LIST_NODE_FREE).len(), 2);
        let (mut m, dir, db, ed, e, texture, _) = world(b"wood.x", 0);
        let mut o = Disk(Vec::new(), Vec::new());
        assert_eq!(verify_picture_path(&mut o, &mut m, dir, db, ed, e, 0, 0), 0);
        assert_eq!(called(&o, SI_STATUS_DISPLAY_MESSAGE), [b"T2D: will replace unfound picture [wood] by default".to_vec()]);
        assert_eq!((m.cstr(m.u32(texture + 0x14)), m.cstr(m.u32(texture + 0x20)), m.i16(texture + 0x2e)), (b"/res/noIcon".to_vec(), b"/res/noIcon.pic".to_vec(), 0));
        assert_eq!(m.cstr(m.u32(node + 8)), b"/own/Brick");
    }

    #[test]
    fn a_sequence_takes_the_frame_number_and_its_own_path_frees_the_record() {
        let (mut m, dir, db, ed, e, texture, node) = world(b"Brick", 1);
        m[crate::globals::SI_FRAME_CURRENT] = 25;
        let mut o = Disk(vec![b"/own/Brick.pic"], Vec::new());
        // the element's own path is found on the first look: the record freed, no message
        assert_eq!(verify_picture_path(&mut o, &mut m, dir, db, ed, e, 0, 0), 1);
        assert_eq!(called(&o, SI_DB_LIST_NODE_FREE).len(), 1);
        assert!(called(&o, SI_STATUS_DISPLAY_MESSAGE).is_empty());
        assert_eq!(m.cstr(m.u32(node + 8)), b"/own/Brick");
        // nothing found: `Brick.25`, looked for again, and no default for type 1
        let (mut m, dir, db, ed, e, texture2, _) = world(b"Brick", 1);
        m[crate::globals::SI_FRAME_CURRENT] = 25;
        let mut o = Disk(Vec::new(), Vec::new());
        assert_eq!(verify_picture_path(&mut o, &mut m, dir, db, ed, e, 0, 0), 0);
        assert_eq!(m.cstr(m.u32(texture2 + 0x14)), b"Brick.25");
        assert_eq!(called(&o, SI_DBFILENAME_REMOVE_PATH), [b"Brick".to_vec(), b"Brick.25".to_vec()]);
        assert!(called(&o, SI_STATUS_DISPLAY_MESSAGE).is_empty());
        // a sequence (type 2): the name as it is
        let (mut m, dir, db, ed, e, _, _) = world(b"seq", 2);
        let mut o = Disk(vec![b"seq"], Vec::new());
        assert_eq!(verify_picture_path(&mut o, &mut m, dir, db, ed, e, 0, 0), 1);
        assert_eq!(o.1.len(), 1);
        let _ = texture;
    }
}
