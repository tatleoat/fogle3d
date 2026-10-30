// SPDX-License-Identifier: MIT
//! The element files of a save (`dictout.c`, with the writer wrappers of `physicalio.c`).
//!
//! [`save_elements`] goes through the chapters of the dictionary, MODELS first, and writes
//! one file per root element that has something to save; then the animation files, which
//! are put together from the relations between elements and curves. The record writers are
//! `si-hrc`'s and `si-elemio`'s; here is which writer gets which object under which name.
//!
//! Files are written whole: the bytes are made first and handed to the file system in one
//! call, where the original opens the file and writes as it goes. A file that cannot be
//! opened behaves the same (nothing is written, error code 1); a write that fails half-way
//! (the original's error code 2, which leaves a truncated file) cannot arise here.

use crate::calls::{Arg, Ask, Unported};
use crate::dbvsfcv;
use crate::dict::ElemId;
use crate::elemdict;
use crate::elemrel;
use crate::load::Scene;
use crate::object::Object;
use crate::scnrel;
use si_db::{DbInfo, FileSystemMut, ListNode, Log, Session};
use si_elemio::anim::{self, HierAnim, ItemData};
use si_elemio::io::Writer;
use si_types::dict::{ELEM_DELETED, ELEM_VALID};
use crate::edtools::{ByAddr, SI_ELEMDICT_FIND_ELEM_BY_DATA, SI_ELEMDICT_FIND_NEXT_RELATED_ELEM, SI_ELEMDICT_FIND_RELATED_ELEM, SI_ELEMDICT_GET_ELEM};
use crate::globals::SI_SAVE_OPT_NO_MAT_TXT;
use si_dict::globals::SI_CID;
use si_mem::Mem;
use si_types::Ptr32;

/// The program's state around a save: the preferences that steer it and what the element
/// writers leave behind.
#[derive(Clone, Debug)]
pub struct SaveState {
    /// `SI_pipedFiles` (0x086ceba8): every file is written into one stream, `SI_pipe`,
    /// instead of the database.
    pub piped: bool,
    /// What was written to `SI_pipe`.
    pub pipe: Vec<u8>,
    /// `si_saveOptNoOldVer` (0x08684060; the setup's `SAVE_NOT_RM_OLD_VERSIONS`): 0 as the
    /// program starts. While it is 0, a save removes the oldest versions of an element
    /// beyond the chapter's `VERSIONS` number.
    pub no_old_ver: i16,
    /// `si_saveOptNoMatTxt` (0x08684062; `SAVE_MAT_TXT_NOT_SEPARATELY`): 0 as the program
    /// starts; a setup file the load reads sets it (`crate::load`'s
    /// `save_options_from_setup`). When set, materials and textures used by nothing else
    /// stay inside their model's file ([`elem_check_restricted_save`]).
    pub no_mat_txt: i16,
    /// `SI_saveElemErrorCode` (0x086cebb0): why the last element writer failed. 1 the file
    /// could not be opened, 2 writing failed, 3 a shader could not be reached, 4 no cluster
    /// ("Main parameter is NULL."), 5, 6 no model.
    pub error_code: i32,
    /// What `SI_emergencySaving()` (0x08392bf4) answers: the 16-bit global at 0x088108b0,
    /// set while the program saves on a fatal signal.
    pub emergency_saving: bool,
    /// `SI_versionOutput` (0x086793f4): 4.0.
    pub version_output: f32,
    /// `SI_dBWorkFname` and `SI_dBWorkFnameExt` (0x08788140, 0x08788540): the path last
    /// built, without and with its suffix. The failure dialogue shows the second.
    pub work_fname: Vec<u8>,
    pub work_fname_ext: Vec<u8>,
    /// Not in the original, for the checks: [`find_save_name`] leaves every name as it is,
    /// so that each file is written under the name it was read from.
    pub keep_names: bool,
    /// Not in the original, for the checks: every file written, in order, with the element
    /// it was written for.
    pub written: Vec<Written>,
    /// Not in the original, for the checks: files the port could not write because the
    /// writer or the object belongs to a package that is not ported, with the writer's name.
    pub not_written: Vec<(ElemId, &'static str)>,
}

impl Default for SaveState {
    fn default() -> Self {
        SaveState {
            piped: false,
            pipe: Vec::new(),
            no_old_ver: 0,
            no_mat_txt: 0,
            error_code: 0,
            emergency_saving: false,
            version_output: 4.0,
            work_fname: Vec::new(),
            work_fname_ext: Vec::new(),
            keep_names: false,
            written: Vec::new(),
            not_written: Vec::new(),
        }
    }
}

/// One file a save wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    pub elem: ElemId,
    pub path: Vec<u8>,
}

/// The messages of `saveElemErrorMsg` (0x08683640), by `SI_saveElemErrorCode`.
pub const SAVE_ELEM_ERROR_MSG: [&str; 6] =
    ["Parameter NN is NULL.", "Error in opening file.", "Error writing in the file.", "Error in accessing shader.", "Main parameter is NULL.", "Filename does not exist."];

/// What a save works with besides the scene: the file system, the parts of the program that
/// are not ported, and whoever answers the dialogues.
pub struct Host<'a> {
    pub fs: &'a mut dyn FileSystemMut,
    pub un: &'a mut dyn Unported,
    pub ui: &'a mut dyn Ask,
}

fn or_null(s: &Option<Vec<u8>>) -> &[u8] {
    s.as_deref().unwrap_or(si_db::NULL_TEXT)
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn exists(h: &Host, path: &[u8]) -> bool {
    si_db::check_file_existence(&*h.fs, path)
}

/// `fopen(path, "wb")`, the bytes, `fclose`; or, with piped files, the bytes appended to
/// the pipe. False when the file cannot be opened.
fn put_file(sc: &mut Scene, h: &mut Host, elem: ElemId, path: &[u8], data: &[u8]) -> bool {
    if sc.saving.piped {
        sc.saving.pipe.extend_from_slice(data);
        return true;
    }
    if !h.fs.write(path, data, false) {
        return false;
    }
    sc.saving.written.push(Written { elem, path: path.to_vec() });
    true
}

/// The two work names of an element's file: `<chapter directory><sub-directory><full
/// name>` and the same with a dot and the chapter's suffix. If the sub-directory does not
/// exist in the chapter the name record's sub-directory is emptied first.
fn work_names(sc: &mut Scene, h: &Host, info: &DbInfo, id: ElemId) {
    let dir = or_null(&info.chapter_path).to_vec();
    let sub = sc.dict.node_of(id.chap, id.index).map(|n| or_null(&n.sub_path).to_vec()).unwrap_or_else(|| si_db::NULL_TEXT.to_vec());
    if !exists(h, &cat(&[&dir, &sub])) {
        if let Some(n) = sc.dict.node_of_mut(id.chap, id.index) {
            n.sub_path = Some(Vec::new());
        }
    }
    let n = sc.dict.node_of(id.chap, id.index);
    let sub = n.map(|n| or_null(&n.sub_path).to_vec()).unwrap_or_else(|| si_db::NULL_TEXT.to_vec());
    let full = n.map(|n| or_null(&n.full).to_vec()).unwrap_or_else(|| si_db::NULL_TEXT.to_vec());
    sc.saving.work_fname = cat(&[&dir, &sub, &full]);
    let suffix = si_types::store::get_suffix_from_chapter(&sc.program.cid, id.chap);
    sc.saving.work_fname_ext = cat(&[&sc.saving.work_fname, b".", suffix]);
}

/// `SI_relGetAnimFRelFromChapter (0x0810f540)`, `dbvsfcv.c`: (chapter); its port is
/// [`dbvsfcv::rel_get_anim_f_rel_from_chapter`].
pub const SI_REL_GET_ANIM_F_REL_FROM_CHAPTER: u32 = 0x0810_f540;
/// `SI_elemdictFindSerialRelation (0x0813335c)`, `elemdict.c`: (dictionary, element,
/// relation, &list, chapter, forward, first).
pub const SI_ELEMDICT_FIND_SERIAL_RELATION: u32 = 0x0813_335c;
/// `SI_elemdictGetCurRelationIndex (0x08138224)`, `elemdict.c`: (element, forward, list).
pub const SI_ELEMDICT_GET_CUR_RELATION_INDEX: u32 = 0x0813_8224;
/// `SI_elemdictSetCurRelationIndex (0x0813825c)`, `elemdict.c`: (element, forward, list,
/// index).
pub const SI_ELEMDICT_SET_CUR_RELATION_INDEX: u32 = 0x0813_825c;

/// Whether a spreadsheet setup is one of the predefined ones [soft 0x085a4e18
/// SI_spdIsPredefined]: the 16-bit word at +0x6e of the element's object, 0 for an element
/// without one.
pub fn spd_is_predefined(m: &Mem, elem: u32) -> i32 {
    let data = m.u32(elem + 4);
    if data == 0 {
        0
    } else {
        m.i16(data + 0x6e) as i32
    }
}

/// `SI_elemdictFindRelatedElem` (`next` false) or `…FindNextRelatedElem` (`next` true) with
/// the two locals of [`elem_check_restricted_save`] for the chapter and the index found.
#[allow(clippy::too_many_arguments)]
fn related<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32, locals: u32, next: bool, e: u32, forward: u32, list: u32, relid: u32, chap: i32) -> u32 {
    let f = if next { SI_ELEMDICT_FIND_NEXT_RELATED_ELEM } else { SI_ELEMDICT_FIND_RELATED_ELEM };
    o.call_addr(m, f, &[dict, e, forward, list, relid, chap as u32, locals, locals + 4])
}

/// The three tests [`elem_check_restricted_save`] makes on each element `x` its walks meet:
/// true when `x` is animated (a relation `anim` to ANIMATION in its forward list 0), drives
/// an expression (`SI_elemdictFindSerialRelation(dictionary, x, 20010, &local, -1, 1, 1)`)
/// or is driven by one (relation 20010 in its backward list 0); between the last two the
/// cursor of the asked element's backward list 0 is read, and put back when the last test
/// finds nothing.
#[allow(clippy::too_many_arguments)]
fn tied<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32, locals: u32, elem: u32, x: u32, anim: u32, ani: i32) -> bool {
    if related(o, m, dict, locals, false, x, 1, 0, anim, ani) != 0 {
        return true;
    }
    if o.call_addr(m, SI_ELEMDICT_FIND_SERIAL_RELATION, &[dict, x, 0x4e2a, locals, 0xffff_ffff, 1, 1]) != 0 {
        return true;
    }
    let cur = o.call_addr(m, SI_ELEMDICT_GET_CUR_RELATION_INDEX, &[elem, 0, 0]);
    if related(o, m, dict, locals, false, x, 0, 0, 0x4e2a, -1) != 0 {
        return true;
    }
    o.call_addr(m, SI_ELEMDICT_SET_CUR_RELATION_INDEX, &[elem, 0, 0, cur]);
    false
}

/// Whether every 2D texture of a material record's list (+0x6ec, linked by +0x16c) is left
/// out: each looked up in TEXTURES2D by its record (`SI_elemdictFindElemByData`, the answer
/// kept in the first local; `SI_elemdictGetElem` when it is not negative, else no element)
/// and asked about without `deep`.
fn textures_left_out<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32, locals: u32, material: u32) -> bool {
    let t2d = m[SI_CID].t2d;
    let mut t = m.u32(material + 0x6ec);
    while t != 0 {
        let index = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, t2d as u32, t]) as i32;
        m.set_i32(locals, index);
        let e = if index < 0 { 0 } else { o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict, t2d as u32, index as u32]) };
        if elem_check_restricted_save(o, m, dict, t2d, e, 0) == 0 {
            return false;
        }
        t = m.u32(t + 0x16c);
    }
    true
}

/// A material record with one of its four shaders (+0xdb0 to +0xdbc).
fn has_shader(m: &Mem, material: u32) -> bool {
    (0..4).any(|k| m.u32(material + 0xdb0 + 4 * k) != 0)
}

/// Whether an element is left out of a save [soft 0x0812e110 si_elemCheckRestrictedSave]:
/// 1 left out, 0 saved; on the dictionary `dict` of the image.
///
/// - No element: left out.
/// - SPREADSHEET_SETUP: left out when it is one of the predefined setups
///   ([`spd_is_predefined`]).
/// - With `si_saveOptNoMatTxt` clear (the setup's `SAVE_MAT_TXT_NOT_SEPARATELY`, off as the
///   program starts), nothing else is left out; nor is anything but a material, a 2D
///   texture or a 3D texture.
/// - Else such an element is left out (it stays inside the model files that use it) unless
///   something keeps it a file of its own. The chapter's animation relation is asked first
///   (`SI_relGetAnimFRelFromChapter`); then:
///   - a texture asked about with `deep`: the material it belongs to (401 or 501, backward
///     list 0) must itself be left out (asked without `deep`; no material is "left out");
///   - a 2D texture whose record has a shader (+0x198), or a 3D texture with one (+0x14c),
///     is kept;
///   - a 2D texture is kept when a 2D texture of a model it is global to (400), or of a
///     material it belongs to (401), is tied ([`tied`]); a 3D texture likewise through the
///     models (500), and through its materials (501) the original asks for the first 3D
///     texture of **no element** (a null element, which the look-up answers with none), so
///     nothing is found there;
///   - a material is kept when, for each model that uses it (300, backward list 0), some
///     material of that model (forward list 3) is tied, has no record or one with a shader,
///     has a 2D texture that is kept, or a 3D texture (501, forward list 3) that is kept.
///     When the models run out the walk ends with the material itself: left out when it
///     has a record without shaders, none of its 2D textures is kept and it has no 3D
///     texture or one that is left out; else kept.
///
/// The look-ups write a chapter (or a list) and an index into two locals of the frame.
pub fn elem_check_restricted_save<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32, chap: i32, elem: u32, deep: i16) -> i32 {
    if elem == 0 {
        return 1;
    }
    let cid = m[SI_CID];
    if chap == cid.spd {
        return spd_is_predefined(m, elem);
    }
    if m[SI_SAVE_OPT_NO_MAT_TXT] == 0 {
        return 0;
    }
    if !(chap == cid.mat || chap == cid.t2d || chap == cid.t3d) {
        return 0;
    }
    let anim = o.call_addr(m, SI_REL_GET_ANIM_F_REL_FROM_CHAPTER, &[chap as u32]);
    let locals = m.local(8);
    let r = restricted_matter(o, m, dict, locals, chap, elem, deep, anim);
    m.release(8);
    r
}

/// [`elem_check_restricted_save`] for a material or a texture, once the animation relation
/// is known.
#[allow(clippy::too_many_arguments)]
fn restricted_matter<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32, locals: u32, chap: i32, elem: u32, deep: i16, anim: u32) -> i32 {
    let cid = m[SI_CID];
    if deep != 0 && (chap == cid.t2d || chap == cid.t3d) {
        let relid = if chap == cid.t2d { 0x191 } else { 0x1f5 };
        let owner = related(o, m, dict, locals, false, elem, 0, 0, relid, cid.mat);
        if elem_check_restricted_save(o, m, dict, cid.mat, owner, 0) == 0 {
            return 0;
        }
    }
    if chap == cid.t2d {
        let data = m.u32(elem + 4);
        if data != 0 && m.u32(data + 0x198) != 0 {
            return 0;
        }
        for (relid, owners) in [(400, cid.model), (0x191, cid.mat)] {
            let mut owner = related(o, m, dict, locals, false, elem, 0, 0, relid, owners);
            while owner != 0 {
                let mut x = related(o, m, dict, locals, false, owner, 1, 3, relid, cid.t2d);
                while x != 0 {
                    if tied(o, m, dict, locals, elem, x, anim, cid.ani) {
                        return 0;
                    }
                    x = related(o, m, dict, locals, true, owner, 1, 3, relid, cid.t2d);
                }
                owner = related(o, m, dict, locals, true, elem, 0, 0, relid, owners);
            }
        }
        return 1;
    }
    if chap == cid.t3d {
        let data = m.u32(elem + 4);
        if data != 0 && m.u32(data + 0x14c) != 0 {
            return 0;
        }
        // the models, then the materials; for the materials the original asks about the
        // texture of a null element
        for (relid, owners) in [(500, cid.model), (0x1f5, cid.mat)] {
            let mut owner = related(o, m, dict, locals, false, elem, 0, 0, relid, owners);
            while owner != 0 {
                let of = if relid == 500 { owner } else { 0 };
                let x = related(o, m, dict, locals, false, of, 1, 3, relid, cid.t3d);
                if x != 0 && tied(o, m, dict, locals, elem, x, anim, cid.ani) {
                    return 0;
                }
                owner = related(o, m, dict, locals, true, elem, 0, 0, relid, owners);
            }
        }
        return 1;
    }
    // a material
    let mut model = related(o, m, dict, locals, false, elem, 0, 0, 300, cid.model);
    loop {
        if model == 0 {
            let data = m.u32(elem + 4);
            if data != 0 && !has_shader(m, data) {
                if !textures_left_out(o, m, dict, locals, data) {
                    return 0;
                }
                let t3 = related(o, m, dict, locals, false, elem, 1, 3, 0x1f5, cid.t3d);
                if t3 == 0 || elem_check_restricted_save(o, m, dict, cid.t3d, t3, 0) != 0 {
                    return 1;
                }
            }
            return 0;
        }
        let mut x = related(o, m, dict, locals, false, model, 1, 3, 300, cid.mat);
        while x != 0 {
            if tied(o, m, dict, locals, elem, x, anim, cid.ani) {
                return 0;
            }
            let data = m.u32(x + 4);
            if data == 0 || has_shader(m, data) {
                return 0;
            }
            if !textures_left_out(o, m, dict, locals, data) {
                return 0;
            }
            let t3 = related(o, m, dict, locals, false, x, 1, 3, 0x1f5, cid.t3d);
            if t3 != 0 && elem_check_restricted_save(o, m, dict, cid.t3d, t3, 0) == 0 {
                return 0;
            }
            x = related(o, m, dict, locals, true, model, 1, 3, 300, cid.mat);
        }
        model = related(o, m, dict, locals, true, elem, 0, 0, 300, cid.model);
    }
}

// adapter: not in place
/// The save's question on the scene's image: [`elem_check_restricted_save`] on the image's
/// current dictionary, whose elements the stores number alike, with `si_saveOptNoMatTxt`
/// set from [`SaveState::no_mat_txt`]; true when the element is left out. `None` is no
/// element. An object that is not a record of the image (its reader on the image failed:
/// the element holds a handle) reads as zeros there, a recorded fault.
pub fn check_restricted_save(sc: &mut Scene, chap: i32, elem: Option<ElemId>, deep: bool, un: &mut dyn Unported) -> bool {
    let mut m = std::mem::take(&mut sc.mem);
    m[SI_SAVE_OPT_NO_MAT_TXT] = sc.saving.no_mat_txt;
    let ed = crate::world::current_dict(&m);
    let e = elem.and_then(|id| crate::world::slot(&m, ed, id)).map_or(0, |e| e.addr());
    let r = {
        let mut o = crate::image::Prog { sc: &mut *sc, un, fs: None, db: None };
        elem_check_restricted_save(&mut o, &mut m, ed.addr(), chap, e, deep as i16)
    };
    sc.mem = m;
    r != 0
}

/// The version number an element's next file gets, and the removal of its oldest versions:
/// `si_dbfileCheckExistence` (0x0812580c) and `si_dbfileCheckExistence2` (0x08125a78), of
/// the database layer. Not cited: the test "does some scene still name this version"
/// (`SI_dbfileCheckElement`, which reads every description of the database) is not ported;
/// it is put to `ui` as a question per version.
///
/// The number is the highest version on disk plus one, among the files of the element
/// (`prefixed`, its prefix and name) that carry its substitution number; with no such file
/// it is -9998. `listing` is the chapter's files as listed once for the whole chapter (the
/// second routine); without it the chapter is scanned for this element (the first).
///
/// With `purge`, when the chapter holds at least as many versions of the element as its
/// `VERSIONS` number says, the files are gone through from the lowest version up and the
/// first `count - VERSIONS + 1` are deleted (`SI_dbfileDelete`: the one file is unlinked),
/// each unless a scene still refers to it.
pub fn file_check_existence(sc: &mut Scene, h: &mut Host, info: &mut DbInfo, prefixed: &[u8], subst: Option<&[u8]>, chapter: &[u8], purge: bool, listing: Option<&[ListNode]>) -> i32 {
    let mut log = Log::default();
    let mut versions: Vec<(i32, ListNode)> = Vec::new();
    // one variable receives every conversion; the original leaves it unset before the first
    let mut v = 0;
    match listing {
        Some(nodes) => {
            for n in nodes {
                if si_db::file_compare_prefix(or_null(&n.full), prefixed, subst) {
                    if let Some((x, _)) = si_db::scan_int(or_null(&n.version)) {
                        v = x;
                    }
                    versions.push((v, n.clone()));
                }
            }
        }
        None => {
            let s = Session { fs: &*h.fs, now: sc.program.now, loading: sc.program.loading };
            let mut lst = None;
            for f in si_db::chapter_list_elements(&s, info, None, true, true, None, &mut lst, &mut log) {
                if si_db::file_compare_prefix(or_null(&f.node.full), prefixed, subst) {
                    if let Some((x, _)) = si_db::scan_int(or_null(&f.node.version)) {
                        v = x;
                    }
                    versions.push((v, f.node));
                }
            }
        }
    }
    let latest = match listing {
        Some(_) => versions.iter().map(|(x, _)| *x).fold(-9999, i32::max).wrapping_add(1),
        // the first routine scans the chapter itself with the database layer's own test
        // (file names with their suffix, icon files left out): `si-db` has that scan
        None => {
            let s = Session { fs: &*h.fs, now: sc.program.now, loading: sc.program.loading };
            si_db::next_version_number(&s, info, prefixed, subst, &mut log)
        }
    };
    if purge && !versions.is_empty() {
        let keep = si_db::db_info_get_nb_versions(info, or_null(&info.chapter).to_vec().as_slice(), None);
        if keep <= versions.len() as i32 {
            // by version, ascending (the element sort's key 5; equal versions keep the
            // listing's order here)
            let mut order: Vec<usize> = (0..versions.len()).collect();
            order.sort_by_key(|&i| versions[i].0);
            let excess = versions.len() as i32 - keep + 1;
            for &i in order.iter().take(excess.max(0) as usize) {
                let node = &versions[i].1;
                let referred = h.ui.ask("SI_dbfileCheckElement: a scene of the database names this version", &[Arg::Str(chapter.to_vec()), Arg::Str(or_null(&node.full).to_vec())]) != 0;
                if !referred {
                    si_db::file_delete(h.fs, info, chapter, 1, None, node, true, false);
                }
            }
        }
    }
    latest
}

/// Gives an element the name its hierarchy's objects carry
/// [soft 0x0812c200 si_elemdictSetNamesFromHierarchy]: the element's object is renamed to
/// the element's name ([`soft_elem_change_name`]), and for a model the same for every
/// element that names it as parent (relation 110, backward side, list 1), recursively.
pub fn set_names_from_hierarchy(sc: &mut Scene, id: ElemId, un: &mut dyn Unported) {
    let name = sc.dict.node_of(id.chap, id.index).and_then(|n| n.name.clone());
    soft_elem_change_name(sc, id, name.as_deref(), un);
    if id.chap == sc.program.cid.model {
        for son in elemrel::related_elems(&mut sc.dict, id, false, 1, 0x6e, id.chap) {
            set_names_from_hierarchy(sc, son, un);
        }
    }
}

/// `SI_edSoftElemChangeName` for an element of the stores (the saver's entry;
/// [`crate::world::soft_elem_change_name`] is the one for an element of the image): what
/// the routine reads from the dictionary is taken from the stores and handed to
/// [`rename_object`].
// adapter: not in place
pub fn soft_elem_change_name(sc: &mut Scene, id: ElemId, name: Option<&[u8]>, un: &mut dyn Unported) {
    let cid = sc.program.cid;
    let Some(e) = sc.dict.slot(id).map(|e| e.rec) else { return };
    if e.data.is_null() {
        return;
    }
    let Some(name) = name else { return };
    let owner = if id.chap == cid.cls {
        elemrel::find_related_elem(&mut sc.dict, id, false, 0, 700, cid.model).map(|m| sc.dict.node_of(m.chap, m.index).and_then(|n| n.name.clone()).unwrap_or_default())
    } else {
        None
    };
    rename_object(sc, &cid, id, e.data.0, e.aux_data.0, name, owner, un);
}

/// Renames the object of an element [soft 0x080fe96c SI_edSoftElemChangeName]. Nothing for
/// a missing element, an element without object, or a null name (the two entries test
/// that). The objects are not in the image: `data` and `aux` are the handles of the
/// element's object and of its node of the scene graph. By chapter:
///
/// | Chapter | The name goes to |
/// |---|---|
/// | MODELS | the node (→ `SI_modelChangeName`, which also marks the node changed) |
/// | WAVES, NSELECTIONS | the string at +4 of the object |
/// | LIGHTS | the 30 bytes at the start of the light, cut to 29 characters; not when the element's node in the scene graph is of kind 7 (the node in front of a spot light) |
/// | MATERIALS | the 30 bytes at the start of the material, cut to 29 characters |
/// | TEXTURES2D | the string at +0x18 |
/// | TEXTURES3D | the string at +0 |
/// | the eleven shader chapters | → `SI_udfSetName` |
/// | CLUSTERS | the string at +0 of the cluster: the name without the model's name in front, when it starts with it and is longer (`owner`: the name of the model the cluster is related to by relation 700, when there is one) |
#[allow(clippy::too_many_arguments)]
pub fn rename_object(sc: &mut Scene, cid: &si_types::dict::ChapIds, id: ElemId, data: u32, aux: u32, name: &[u8], owner: Option<Vec<u8>>, un: &mut dyn Unported) {
    let chap = id.chap;
    let cut29 = |n: &[u8]| n[..n.len().min(29)].to_vec();
    let shaders = [cid.shd_mat, cid.shd_mat_photon, cid.shd_t2d, cid.shd_t3d, cid.shd_vol, cid.shd_cam, cid.shd_out, cid.shd_sha, cid.shd_env, cid.shd_lig, cid.shd_lig_photon];
    if chap == cid.model {
        un.call("SI_modelChangeName", &[Arg::Elem(id), Arg::Str(name.to_vec())]);
        if let Some(Object::ModelNode { tree, node }) = sc.objects.get(data).cloned() {
            if let Some(t) = sc.models.get_mut(tree) {
                if t.names[node] != name {
                    t.names[node] = name.to_vec();
                }
            }
        }
    } else if chap == cid.wav || chap == cid.nsl {
        match sc.objects.get_mut(data) {
            Some(Object::NSelection { name: n }) => *n = name.to_vec(),
            _ => un.call("the name of a wave", &[Arg::Elem(id), Arg::Str(name.to_vec())]),
        }
    } else if chap == cid.lig {
        if sc.graf.get(aux).is_some_and(|n| n.kind == 7) {
            return;
        }
        if let Some(Object::Light(l)) = sc.objects.get_mut(data) {
            // strncpy of 30 bytes, then a terminator at the thirtieth
            l.set_name(name);
            l.name[29] = 0;
        }
    } else if chap == cid.mat {
        if let Some(Object::Material(m)) = sc.objects.get_mut(data) {
            m.name = cut29(name);
        }
    } else if chap == cid.t2d {
        // +0x18 is the texture's own name (file token 50). `si-hrc` calls that field
        // `filename` and the picture's path at +0x14 (token 1) `name`: the two labels are
        // the wrong way round there (the samples have the picture's path under token 1).
        if let Some(Object::Texture2D(t)) = sc.objects.get_mut(data) {
            t.filename = name.to_vec();
        }
    } else if chap == cid.t3d {
        if let Some(Object::Texture3D(t)) = sc.objects.get_mut(data) {
            t.name = name.to_vec();
        }
    } else if shaders.contains(&chap) {
        un.call("SI_udfSetName", &[Arg::Elem(id), Arg::Str(name.to_vec())]);
    } else if chap == cid.cls {
        let mut name = name;
        if let Some(model_name) = &owner {
            if model_name.len() < name.len() && name.starts_with(model_name) {
                name = &name[model_name.len()..];
            }
        }
        if let Some(Object::Cluster { model, cluster }) = sc.objects.get(data).cloned() {
            if let Some(p) = scnrel::clusters_of_model(sc, model) {
                if let Some(k) = sc.clusters[p].klusters.get_mut(cluster) {
                    k.name = name.to_vec();
                }
            }
        }
    }
}

/// The routines the savers call one level down, as a seam: [`Real`] calls the ports; the
/// emulation checks put recorded stand-ins here, exactly where the harness stubs the
/// original's callees, so that one routine at a time is compared.
pub trait Steps {
    /// `si_elemCheckRestrictedSave`.
    fn restricted(&mut self, sc: &mut Scene, chap: i32, id: ElemId, un: &mut dyn Unported) -> bool {
        check_restricted_save(sc, chap, Some(id), true, un)
    }
    /// `si_dbfileCheckExistence` and `si_dbfileCheckExistence2`.
    #[allow(clippy::too_many_arguments)]
    fn check_existence(&mut self, sc: &mut Scene, h: &mut Host, info: &mut DbInfo, prefixed: &[u8], subst: Option<&[u8]>, chapter: &[u8], purge: bool, listing: Option<&[ListNode]>) -> i32 {
        file_check_existence(sc, h, info, prefixed, subst, chapter, purge, listing)
    }
    /// `si_elemdictSetNamesFromHierarchy`.
    fn set_names(&mut self, sc: &mut Scene, id: ElemId, un: &mut dyn Unported) {
        set_names_from_hierarchy(sc, id, un)
    }
    /// `SI_dbFindSaveName`.
    #[allow(clippy::too_many_arguments)]
    fn find_save_name(&mut self, sc: &mut Scene, h: &mut Host, info: &mut DbInfo, chap: i32, id: ElemId, ask: &mut i16, rename: bool, listing: Option<&[ListNode]>) -> i32 {
        find_save_name_with(&mut Real, sc, h, info, chap, id, ask, rename, listing)
    }
    /// `elemdictSaveAnElement`.
    fn save_an_element(&mut self, sc: &mut Scene, h: &mut Host, info: &mut DbInfo, chap: i32, id: ElemId) -> bool {
        save_an_element(sc, h, info, chap, id)
    }
    /// `elemdictSaveAnimFile`.
    #[allow(clippy::too_many_arguments)]
    fn save_anim_file(&mut self, sc: &mut Scene, h: &mut Host, info: &mut DbInfo, anim_elem: ElemId, target: Option<ElemId>, chap: i32, selected_only: bool, emergency: bool, listing: Option<&[ListNode]>, ask: &mut i16) -> bool {
        save_anim_file_with(&mut Real, sc, h, info, anim_elem, target, chap, selected_only, emergency, listing, ask)
    }
    /// `elemdictSaveHierAnimation`.
    fn save_hier_animation(&mut self, sc: &mut Scene, h: &mut Host, info: &DbInfo, anim_elem: ElemId, target: Option<ElemId>, chap: i32) {
        save_hier_animation(sc, h, info, anim_elem, target, chap)
    }
    /// `elemdictSaveChapterElements`.
    #[allow(clippy::too_many_arguments)]
    fn save_chapter(&mut self, sc: &mut Scene, h: &mut Host, info: &mut DbInfo, selected_only: bool, emergency: bool, only_chap: i32, chap: i32, ask: &mut i16) -> bool {
        save_chapter_elements_with(&mut Real, sc, h, info, selected_only, emergency, only_chap, chap, ask)
    }
    /// `elemdictSaveAllAnimFiles`.
    #[allow(clippy::too_many_arguments)]
    fn save_all_anim(&mut self, sc: &mut Scene, h: &mut Host, info: &mut DbInfo, selected_only: bool, emergency: bool, listing: Option<&[ListNode]>, ask: &mut i16) -> bool {
        save_all_anim_files_with(&mut Real, sc, h, info, selected_only, emergency, listing, ask)
    }
}

/// The ports themselves.
pub struct Real;

impl Steps for Real {}

/// What [`find_save_name`] answers.
pub const NAME_OK: i32 = 1;
pub const NAME_CANCEL_ALL: i32 = 3;
pub const NAME_SHARE_NOT_SAVED: i32 = 4;
pub const NAME_SHARE_SKIP_ALL: i32 = 5;

/// Decides under which name an element is saved [soft 0x0812eaf0 SI_dbFindSaveName].
/// `listing` is the chapter's files as listed by the caller (null when it did not list
/// them); `ask` is the caller's "ask before saving over" word, cleared by "Save All".
/// Nothing is done with piped files.
///
/// **An element that is not shared**
///
/// 1. The version number the next file gets is looked up ([`file_check_existence`], which
///    also removes the oldest versions unless the preference says to keep them).
/// 2. Only for an element made in this session (new-flag 2) whose name already has files,
///    with `ask` set and outside the batch renderer, a dialogue: `New <chapter> <full
///    name>` / `exists in database...` / `Change Name or Save New Version?` with the
///    buttons `Rename`, `Save`, `Save All`, `Cancel All` (answers 0 to 3). `Rename` opens
///    the save dialogue for the element (→ `SI_dialogSaveElement`) and asks again (now
///    `Sorry,` … `also exists in database...`); `Save` goes on; `Save All` clears `ask`;
///    `Cancel All` ends the save: the answer is 3.
/// 3. With `rename`, the element's objects take the element's name
///    ([`set_names_from_hierarchy`]).
/// 4. **The version**: with no file of that name on disk it becomes `1` (and the name is
///    rebuilt), unless it is `1` already; otherwise it becomes the number of step 1. In the
///    batch renderer, with no file on disk and outside an emergency save, the name is left
///    alone.
///
/// **A shared element** keeps its name and version. Its chapter's `SHARE` directory is
/// made if need be; if it could not be made or written to, or the element's file exists
/// and cannot be written while `ask` is set, a dialogue (→ `SI_dbMRequest2`, texts 0x9b
/// and 0x9a) answers 4 (this element is not saved), 5 (nor any other: `ask` is cleared) or
/// 3 (the save ends). Otherwise the existing file is first copied to `<name>.bak`: **the
/// original does that with a shell command** (`cp <file> <file>.bak`, `SI_dbFileMove`).
#[allow(clippy::too_many_arguments)]
pub fn find_save_name(sc: &mut Scene, h: &mut Host, info: &mut DbInfo, chap: i32, id: ElemId, ask: &mut i16, rename: bool, listing: Option<&[ListNode]>) -> i32 {
    find_save_name_with(&mut Real, sc, h, info, chap, id, ask, rename, listing)
}

/// [`find_save_name`] with its two callees given ([`Steps`]).
#[allow(clippy::too_many_arguments)]
pub fn find_save_name_with(steps: &mut dyn Steps, sc: &mut Scene, h: &mut Host, info: &mut DbInfo, chap: i32, id: ElemId, ask: &mut i16, rename: bool, listing: Option<&[ListNode]>) -> i32 {
    if sc.saving.piped {
        return NAME_OK;
    }
    let chapter = sc.dict.ed.lists[chap as usize].name.clone();
    let Some(e) = sc.dict.slot(id).map(|e| e.rec) else { return NAME_OK };
    let suffix = si_types::store::get_suffix_from_chapter(&sc.program.cid, chap).to_vec();
    if e.shared == 0 {
        let loading = sc.program.loading;
        let (mut intro, mut what): (&[u8], &[u8]) = (b"New", b"exists in database...");
        let mut next;
        loop {
            let (prefixed, subst) = match sc.dict.node_of_mut(id.chap, id.index) {
                Some(n) => (si_db::filename_set_prefix(n, loading), n.subst.clone()),
                None => return NAME_OK,
            };
            let purge = sc.saving.no_old_ver == 0;
            next = steps.check_existence(sc, h, info, &prefixed, subst.as_deref(), &chapter, purge, listing);
            if *ask == 0 || e.new_flag != 2 || next < 0 || sc.standalone_renderer {
                break;
            }
            let full = sc.dict.node_of(id.chap, id.index).map(|n| or_null(&n.full).to_vec()).unwrap_or_default();
            let line = cat(&[intro, b" ", &chapter, b" ", &full]);
            match h.ui.ask("SI_dialogRequest34", &[Arg::Str(line), Arg::Str(what.to_vec()), Arg::Str(b"Change Name or Save New Version?".to_vec())]) {
                1 => break,
                0 => {
                    h.un.call("SI_dialogSaveElement", &[Arg::Chap(chap), Arg::Elem(id), Arg::Int(0)]);
                    intro = b"Sorry,";
                    what = b"also exists in database...";
                }
                2 => {
                    *ask = 0;
                    break;
                }
                3 => return NAME_CANCEL_ALL,
                _ => {}
            }
        }
        if rename {
            steps.set_names(sc, id, h.un);
        }
        if sc.saving.keep_names {
            return NAME_OK;
        }
        let Some(n) = sc.dict.node_of_mut(id.chap, id.index) else { return NAME_OK };
        if next < 0 {
            if sc.standalone_renderer && !sc.saving.emergency_saving {
                return NAME_OK;
            }
            match &n.version {
                None => return NAME_OK,
                Some(v) if v == b"1" => return NAME_OK,
                _ => {}
            }
            n.version = Some(b"1".to_vec());
        } else {
            n.version = Some(next.to_string().into_bytes());
        }
        n.full = Some(si_db::format_full_name(n, loading));
        return NAME_OK;
    }
    // a shared element
    let n = sc.dict.node_of(id.chap, id.index);
    let sub = n.map(|n| or_null(&n.sub_path).to_vec()).unwrap_or_default();
    let full = n.map(|n| or_null(&n.full).to_vec()).unwrap_or_default();
    let share = si_db::check_create_share_subdir(h.fs, info, &chapter);
    let mut text = -1;
    if share == -1 || share == -2 {
        sc.saving.work_fname = cat(&[&chapter, b" ", &full, b".", &suffix]);
        text = 0x9b;
    } else {
        sc.saving.work_fname_ext = cat(&[or_null(&info.chapter_path), &sub, &full, b".", &suffix]);
        let ext = sc.saving.work_fname_ext.clone();
        if *ask != 0 && exists(h, &ext) && !h.fs.access(&ext, si_db::W_OK) {
            sc.saving.work_fname = cat(&[b"...", &chapter, b"/", &sub, &full, b".", &suffix]);
            text = 0x9a;
        }
    }
    if text > 0 {
        return match h.ui.ask("SI_dbMRequest2", &[Arg::Int(text as i64), Arg::Str(sc.saving.work_fname.clone())]) {
            0 => NAME_SHARE_NOT_SAVED,
            1 => {
                *ask = 0;
                NAME_SHARE_SKIP_ALL
            }
            _ => NAME_CANCEL_ALL,
        };
    }
    sc.saving.work_fname = cat(&[or_null(&info.chapter_path), &sub, &full, b".bak"]);
    let (from, to) = (sc.saving.work_fname_ext.clone(), sc.saving.work_fname.clone());
    // `cp <from> <to>` through the shell: refused on a real file system by `si-db`
    si_db::file_move(h.fs, &from, &to, true, false, false);
    NAME_OK
}

/// Whether an element is one the element saver writes: it has an object, is in use and is
/// a root; and, for a full save, it is dirty or new (new-flag not 0), or, for "selected
/// only", the root of its hierarchy is marked selected; an emergency save takes them all.
fn to_save(sc: &Scene, chap: i32, index: usize, selected_only: bool, emergency: bool) -> bool {
    let l = &sc.dict.ed.lists[chap as usize];
    let e = &l.elems[index].rec;
    if e.data.is_null() || e.status != ELEM_VALID || e.hier_level != 0 {
        return false;
    }
    changed(sc, chap, index, selected_only) || emergency
}

fn changed(sc: &Scene, chap: i32, index: usize, selected_only: bool) -> bool {
    let l = &sc.dict.ed.lists[chap as usize];
    let e = &l.elems[index].rec;
    if selected_only {
        // the original indexes the chapter with the root index unchecked
        l.elems.get(e.root_id as usize).is_some_and(|r| r.rec.selected != 0)
    } else {
        e.dirty != 0 || e.new_flag != 0
    }
}

/// Saves the elements of one chapter [soft 0x0812d7d0 elemdictSaveChapterElements]; false
/// when the save is to stop.
///
/// Nothing is done for SCENES, ANIMATION, PICTURES and RENDER_PICTURES, for a chapter that
/// is not kept in a database, or, when `only_chap` is not -1, for any other chapter.
///
/// 1. **Which elements.** Roots in use that have an object and are dirty, new, or (for
///    "selected only") have a selected root; all of them in an emergency save; each unless
///    [`check_restricted_save`] leaves it out. **An element that is none of those is saved
///    all the same when its file is not in this database** (not under its name in its
///    sub-directory) **or it was read from another database**: it is marked dirty. For this
///    test the element's sub-directory is emptied when the chapter does not have it.
/// 2. If there is none, nothing more is done.
/// 3. Unless files are piped: the chapter's files are listed once (all sub-directories),
///    and unless old versions are kept, every listed file is marked "unused" and the marks
///    of those some scene of the database still names are cleared (→ `SI_dbMFlagUnused`,
///    which reads every description of the database). Then every element gets its name
///    ([`find_save_name`]); "Cancel All" there ends the save: the progress window is
///    closed and the listing disposed of.
/// 4. Each element is written ([`save_an_element`]), with `Saving <chapter> <full name>`
///    in the progress window. **The first failure stops the chapter and the save**: the
///    progress window is closed and a dialogue says `<chapter>: <message of the error
///    code>` / `Cannot write file:` / `<path>.` / `Save aborted.`; the error code is
///    cleared. Files written before stay.
#[allow(clippy::too_many_arguments)]
pub fn save_chapter_elements(sc: &mut Scene, h: &mut Host, info: &mut DbInfo, selected_only: bool, emergency: bool, only_chap: i32, chap: i32, ask: &mut i16) -> bool {
    save_chapter_elements_with(&mut Real, sc, h, info, selected_only, emergency, only_chap, chap, ask)
}

/// [`save_chapter_elements`] with its callees given ([`Steps`]).
#[allow(clippy::too_many_arguments)]
pub fn save_chapter_elements_with(steps: &mut dyn Steps, sc: &mut Scene, h: &mut Host, info: &mut DbInfo, selected_only: bool, emergency: bool, only_chap: i32, chap: i32, ask: &mut i16) -> bool {
    let cid = sc.program.cid;
    if chap == cid.dsc || chap == cid.ani || chap == cid.img || chap == cid.rim || !(only_chap == -1 || chap == only_chap) {
        return true;
    }
    if sc.dict.ed.lists[chap as usize].rec.kind != 1 {
        return true;
    }
    let chapter = sc.dict.ed.lists[chap as usize].name.clone();
    info.set_chapter(&chapter);
    info.filepath_build();
    let suffix = si_types::store::get_suffix_from_chapter(&cid, chap).to_vec();
    si_db::db_info_get_nb_versions(info, &chapter, Some(&*h.fs));
    let nb = sc.dict.ed.lists[chap as usize].elems.len();
    let mut count = 0;
    for i in 0..nb {
        let e = sc.dict.ed.lists[chap as usize].elems[i].rec;
        let id = ElemId::new(chap, i as i32);
        if to_save(sc, chap, i, selected_only, emergency) {
            if !steps.restricted(sc, chap, id, h.un) {
                count += 1;
            }
            continue;
        }
        if !e.data.is_null() && e.status != ELEM_VALID {
            continue;
        }
        if e.status == ELEM_VALID && e.hier_level == 0 && !e.data.is_null() {
            work_names(sc, h, info, id);
            let ext = sc.saving.work_fname_ext.clone();
            let from_elsewhere = sc.dict.node_of(chap, i as i32).is_some_and(|n| n.w00 != 0 && n.w00 != sc_db_mark(sc, info));
            if !exists(h, &ext) || from_elsewhere {
                sc.dict.ed.lists[chap as usize].elems[i].rec.dirty = 1;
                count += 1;
            }
        }
    }
    if count == 0 {
        return true;
    }
    if !sc.saving.piped {
        let text: Vec<u8> = if sc.saving.no_old_ver == 0 { cat(&[b"Checking scenes for old ", &chapter]) } else { b"Checking latest versions".to_vec() };
        h.un.call("SI_dialogInProgressSetTexts", &[Arg::Str(text), Arg::Str(Vec::new())]);
        h.un.call("SI_dialogInProgressSet", &[Arg::Str(b"Please wait...".to_vec()), Arg::Int(0)]);
        let s = Session { fs: &*h.fs, now: sc.program.now, loading: sc.program.loading };
        let mut lst = None;
        let mut log = Log::default();
        let found = si_db::chapter_list_elements(&s, info, Some(&suffix), true, true, None, &mut lst, &mut log);
        let listing: Option<Vec<ListNode>> = Some(found.into_iter().map(|f| f.node).collect());
        if sc.saving.no_old_ver == 0 {
            h.un.call("SI_dbMFlagUnused", &[Arg::Str(chapter.clone()), Arg::Int(1)]);
        }
        count = 0;
        for i in 0..nb {
            if !to_save(sc, chap, i, selected_only, emergency) {
                continue;
            }
            let id = ElemId::new(chap, i as i32);
            if steps.restricted(sc, chap, id, h.un) {
                continue;
            }
            let r = steps.find_save_name(sc, h, info, chap, id, ask, true, listing.as_deref());
            if r == NAME_CANCEL_ALL {
                h.un.call("SI_dialogInProgressClose", &[]);
                return false;
            }
            h.un.call("SI_dialogInProgressRedraw", &[]);
            count += 1;
        }
    }
    h.un.call("SI_dialogInProgressSetTexts", &[Arg::Str(b"Saving Elements".to_vec()), Arg::Str(Vec::new())]);
    let mut done = 1;
    let mut ok = true;
    for i in 0..nb {
        if !to_save(sc, chap, i, selected_only, emergency) {
            continue;
        }
        let id = ElemId::new(chap, i as i32);
        if steps.restricted(sc, chap, id, h.un) {
            continue;
        }
        if count != 0 {
            let full = sc.dict.node_of(chap, i as i32).map(|n| or_null(&n.full).to_vec()).unwrap_or_default();
            // the fraction shown: done * 100.0 / count, as a float
            h.un.call("SI_dialogInProgressSet", &[Arg::Str(cat(&[b"Saving ", &chapter, b" ", &full])), Arg::Int(done), Arg::Int(count)]);
            done += 1;
        }
        ok = steps.save_an_element(sc, h, info, chap, id);
        if !ok {
            break;
        }
    }
    if !ok {
        h.un.call("SI_dialogInProgressClose", &[]);
        let message = SAVE_ELEM_ERROR_MSG.get(sc.saving.error_code as usize).copied().unwrap_or("");
        let args = [
            Arg::Str(cat(&[&chapter, b": ", message.as_bytes()])),
            Arg::Str(b"Cannot write file:".to_vec()),
            Arg::Str(cat(&[&sc.saving.work_fname_ext, b"."])),
            Arg::Str(b"Save aborted.".to_vec()),
        ];
        h.ui.ask("SI_dialogRequestLong", &args);
        sc.saving.error_code = 0;
        return false;
    }
    true
}

/// The mark of the database being saved to, without adding it to the list of databases: 0
/// when no element was ever read from or saved to it.
fn sc_db_mark(sc: &Scene, info: &DbInfo) -> i32 {
    let path = info.path.clone().unwrap_or_default();
    sc.db_paths.iter().position(|p| *p == path).map_or(0, |i| i as i32 + 1)
}

/// The sub-tree of a hierarchy that starts at a node, without the node's brothers.
fn subtree(root: &si_hrc::Node, index: usize) -> Option<Box<si_hrc::Node>> {
    let mut found = None;
    let mut i = 0usize;
    root.walk(0, &mut |_, n| {
        if i == index {
            found = Some(n);
        }
        i += 1;
    });
    let mut n = Box::new(found?.clone());
    n.brother = None;
    Some(n)
}

/// Writes the file of one element [soft 0x0812d3c8 elemdictSaveAnElement]; false when its
/// writer failed.
///
/// The file is `<chapter directory><sub-directory><full name>.<suffix>` (the sub-directory
/// emptied first when the chapter does not have it). A shared element whose file exists and
/// cannot be written is passed over, with success. Then, by chapter:
///
/// | Chapter | Writer and what it is given |
/// |---|---|
/// | MODELS | [soft 0x0811045c SI_elemdictWriteModel]: the hierarchy under the element's node (`SI_omodelBinaryWrite`: header, prototypes, `HRCH`, the node with its sons) |
/// | SHAPES | `SI_elemdictWriteShape`: the shapes of the model the element points at |
/// | CLUSTERS | `SI_elemdictWriteClusters`: the cluster the element points at, of the model related by 700; nothing is written without one |
/// | LIGHTS, CAMERAS | `SI_elemdictWriteLight`, `SI_elemdictWriteCamera`: the element's object |
/// | LATTICES | `SI_elemdictWriteLattice`: all the node lattices of the model related by 210, or else all the branch lattices of the model related by 215; nothing is written without either |
/// | MATERIALS | [soft 0x08110654 SI_elemdictWriteMaterial] (without its textures) |
/// | TEXTURES2D, TEXTURES3D | [soft 0x0811067c SI_elemdictWriteTexture2D], [soft 0x081106a4 SI_elemdictWriteTexture3D] |
/// | the eleven shader chapters | [soft 0x081106cc SI_elemdictWriteShader] (→ `SI_udfWrite`) |
/// | WAVES | [soft 0x081106f4 SI_elemdictWriteWave] (→ `SI_dbwaveAsciiWrite`) |
/// | EXPRESSIONS | `SI_elemdictWriteExpressions`: every expression of the dictionary into this one file |
/// | SETUP_SOFT, SETUP_CHANNELS, SETUP_FLOCK | [soft 0x08111ae4 SI_elemdictWriteSetup], [soft 0x08111b18 SI_elemdictWriteChannelSetup], [soft 0x08111b4c SI_elemdictWriteParticleSetup]: the program's setups, not anything of the element |
/// | SPREADSHEET_SETUP | → `SI_sprdSetupWrite` |
///
/// For EXPRESSIONS and the three setups the element's object (the placeholder
/// [`crate::savescene::check_setup_elem`] gave it) is freed afterwards and the pointer
/// cleared. In every case the name record's time becomes the current time.
///
/// The answer for a chapter without a writer, a cluster without model and a lattice without
/// model is not set by the original: it returns whatever its caller had in a register,
/// which is the address of the chapter's record cut to 16 bits; taken as "success" here (it
/// is zero only when that address happens to end in sixteen zero bits).
pub fn save_an_element(sc: &mut Scene, h: &mut Host, info: &mut DbInfo, chap: i32, id: ElemId) -> bool {
    let cid = sc.program.cid;
    work_names(sc, h, info, id);
    let fname = sc.saving.work_fname.clone();
    let ext = sc.saving.work_fname_ext.clone();
    let Some(e) = sc.dict.slot(id).map(|e| e.rec) else { return true };
    if e.shared != 0 && exists(h, &ext) && !h.fs.access(&ext, si_db::W_OK) {
        return true;
    }
    let data = e.data.0;
    let version = sc.saving.version_output;
    let shaders = [cid.shd_mat, cid.shd_mat_photon, cid.shd_t2d, cid.shd_t3d, cid.shd_vol, cid.shd_cam, cid.shd_out, cid.shd_sha, cid.shd_env, cid.shd_lig, cid.shd_lig_photon];
    let mut result = true;
    let mut free_object = false;
    let writer = |sc: &mut Scene, h: &mut Host, path: Vec<u8>, bytes: Option<Vec<u8>>, name: &'static str| -> bool {
        match bytes {
            Some(b) => {
                if put_file(sc, h, id, &path, &b) {
                    true
                } else {
                    sc.saving.error_code = 1;
                    false
                }
            }
            None => {
                // the writer or the object is another package's: nothing is written, and
                // the writer's answer is asked for
                sc.saving.not_written.push((id, name));
                h.ui.ask(name, &[Arg::Str(path), Arg::Elem(id)]) != 0
            }
        }
    };
    if chap == cid.model {
        let bytes = match sc.objects.get(data) {
            Some(Object::ModelNode { tree, node }) => scnrel::assemble_model(sc, *tree).and_then(|(root, _)| subtree(&root, *node)).map(|n| si_hrc::write_model(&n)),
            _ => None,
        };
        result = writer(sc, h, cat(&[&fname, b".hrc"]), bytes, "SI_omodelBinaryWrite");
    } else if chap == cid.shp {
        // SI_shapeBinaryWrite: the model's definitions; `si-elemio` keeps them per shape
        // file read (the last one read onto this model stands for the model's)
        let bytes = sc.shapes.iter().rev().find(|s| s.model == data).map(|s| {
            let mut w = Writer::with_version(version);
            w.st = sc.statics.clone();
            si_elemio::shape::write_file(&mut w, &s.shapes);
            w.out
        });
        result = writer(sc, h, cat(&[&fname, b".shp"]), bytes, "SI_shapeBinaryWrite");
    } else if chap == cid.cls {
        if let Some(m) = elemrel::find_related_elem(&mut sc.dict, id, false, 0, 700, cid.model) {
            let mdata = sc.dict.slot(m).map_or(0, |x| x.rec.data.0);
            // SI_clusterAnimBinaryWrite(name, model, cluster): error 4 without a cluster,
            // 6 without a model, before any file is opened
            let (kind, uv) = match sc.objects.get(mdata) {
                Some(Object::ModelNode { tree, node }) => sc.models.get(*tree).and_then(|t| t.node(*node)).map_or((0, 1), |n| (n.kind, n.uv_sets)),
                _ => (0, 1),
            };
            match sc.objects.get(data).cloned() {
                _ if mdata == 0 => {
                    sc.saving.error_code = 6;
                    result = false;
                }
                Some(Object::Cluster { model, cluster }) => {
                    let bytes = sc.clusters.iter().find(|c| c.model == model).and_then(|c| {
                        let k = c.klusters.get(cluster)?;
                        let mut w = Writer::with_version(version);
                        w.st = sc.statics.clone();
                        si_elemio::cluster::write_file(&mut w, c.current.as_ref(), kind, k, c.anim.as_ref(), uv);
                        Some(w.out)
                    });
                    result = writer(sc, h, cat(&[&fname, b".cls"]), bytes, "SI_clusterAnimBinaryWrite");
                }
                _ => result = writer(sc, h, cat(&[&fname, b".cls"]), None, "SI_clusterAnimBinaryWrite"),
            }
        }
    } else if chap == cid.lig {
        let bytes = match sc.objects.get(data) {
            Some(Object::Light(l)) => {
                let mut w = Writer::with_version(version);
                w.st = sc.statics.clone();
                si_elemio::light::write_file(&mut w, l);
                Some(w.out)
            }
            _ => None,
        };
        result = writer(sc, h, ext.clone(), bytes, "SI_elemdictWriteLight");
    } else if chap == cid.cam {
        let bytes = match sc.objects.get(data) {
            Some(Object::Camera(c)) => {
                let mut w = Writer::with_version(version);
                w.st = sc.statics.clone();
                si_elemio::camera::write_file(&mut w, c);
                Some(w.out)
            }
            _ => None,
        };
        result = writer(sc, h, ext.clone(), bytes, "SI_elemdictWriteCamera");
    } else if chap == cid.lat {
        let mut branch = false;
        let mut m = elemrel::find_related_elem(&mut sc.dict, id, false, 0, 0xd2, cid.model);
        if m.is_none() {
            branch = true;
            m = elemrel::find_related_elem(&mut sc.dict, id, false, 0, 0xd7, cid.model);
        }
        if let Some(m) = m {
            let mdata = sc.dict.slot(m).map_or(0, |x| x.rec.data.0);
            // the model's list of node lattices (+0x50) or branch lattices (+0x58): what
            // the lattice files put there
            let bytes = sc.lattices.iter().rev().find(|l| l.model == mdata && l.branch == branch).map(|l| {
                let mut w = Writer::with_version(version);
                w.st = sc.statics.clone();
                let _ = si_elemio::lattice::write_file(&mut w, &l.list);
                w.out
            });
            result = writer(sc, h, ext.clone(), bytes, "SI_elemdictWriteLattice");
        }
    } else if chap == cid.mat {
        let bytes = match sc.objects.get(data) {
            Some(Object::Material(m)) => Some(si_hrc::write_material(m, version)),
            _ => None,
        };
        result = writer(sc, h, cat(&[&fname, b".mtr"]), bytes, "SI_materialWriteBinaryFile");
    } else if shaders.contains(&chap) {
        result = writer(sc, h, ext.clone(), None, "SI_udfWrite");
    } else if chap == cid.t2d {
        let bytes = match sc.objects.get(data) {
            Some(Object::Texture2D(t)) => Some(si_hrc::write_texture(t, version)),
            _ => None,
        };
        result = writer(sc, h, cat(&[&fname, b".txt"]), bytes, "SI_textureWriteBinaryFile");
    } else if chap == cid.t3d {
        let bytes = match sc.objects.get(data) {
            Some(Object::Texture3D(t)) => Some(si_hrc::write_texture3d(t, version)),
            _ => None,
        };
        result = writer(sc, h, cat(&[&fname, b".t3D"]), bytes, "SI_texture3DWriteBinaryFile");
    } else if chap == cid.wav {
        result = writer(sc, h, ext.clone(), None, "SI_dbwaveAsciiWrite");
    } else if chap == cid.exp {
        // SI_elemdictWriteExpressions (`si-expr` has it; not wired to this dictionary)
        result = writer(sc, h, ext.clone(), None, "SI_elemdictWriteExpressions");
        free_object = true;
    } else if chap == cid.sts {
        // SI_setupFileSavePath: the program's setup, written with `si-ascii`'s writer from
        // what the last setup file read left
        let bytes = sc.setup.as_ref().map(|s| si_ascii::setup::write(s, &si_ascii::setup::WriteOptions::default()));
        writer(sc, h, ext.clone(), bytes, "SI_setupFileSavePath");
        // SI_elemdictWriteSetup answers 1 whatever happened
        result = true;
        free_object = true;
    } else if chap == cid.spd {
        result = writer(sc, h, ext.clone(), None, "SI_sprdSetupWrite");
    } else if chap == cid.stc {
        let bytes = sc.channel_setup.as_ref().and_then(|s| si_ascii::chn::write(s, &si_ascii::chn::WriteContext::default()).ok());
        writer(sc, h, ext.clone(), bytes, "SI_chnSetupSave");
        // SI_elemdictWriteChannelSetup answers 1 whatever happened
        result = true;
        free_object = true;
    } else if chap == cid.sfk {
        let bytes = sc.flock_setup.as_ref().map(|s| si_ascii::flk::write(s, version));
        result = writer(sc, h, ext.clone(), bytes, "SI_particleSetupSave");
        free_object = true;
    }
    if free_object {
        if let Some(e) = sc.dict.slot_mut(id) {
            if !e.rec.data.is_null() {
                sc.objects.free(e.rec.data.0);
            }
            e.rec.data = Ptr32::NULL;
        }
    }
    let now = sc.program.now;
    if let Some(n) = sc.dict.node_of_mut(id.chap, id.index) {
        n.time = now;
    }
    result
}

/// Adds the descendants of a model element to a hierarchy animation
/// [soft 0x0812c288 SI_elemdictCreateHierAnim]: each element that names `elem` as parent
/// (relation 110, backward side, list 1), in list order, with `elem`'s position in the
/// animation as its father, each followed by its own descendants. The name entered is the
/// full name (`full`) or the name part.
pub fn create_hier_anim(sc: &mut Scene, elem: ElemId, ha: &mut HierAnim, full: bool) {
    let own = sc.dict.node_of(elem.chap, elem.index).map(|n| or_null(&n.full).to_vec()).unwrap_or_default();
    let father = hieranim_find_elem_index(ha, &own);
    for son in elemrel::related_elems(&mut sc.dict, elem, false, 1, 0x6e, sc.program.cid.model) {
        let name = sc.dict.node_of(son.chap, son.index).map(|n| if full { or_null(&n.full).to_vec() } else { or_null(&n.name).to_vec() }).unwrap_or_default();
        ha.add_elem(&name, father);
        create_hier_anim(sc, son, ha, full);
    }
}

/// The position of a name in a hierarchy animation's element list, -1 when it is not there
/// [soft 0x08138d70 SI_hieranimFindElemIndex].
pub fn hieranim_find_elem_index(ha: &HierAnim, name: &[u8]) -> i32 {
    ha.elems.iter().position(|e| e.name == name).map_or(-1, |i| i as i32)
}

/// Writes the curves of an element and of its descendants as items of an animation file
/// [soft 0x0812c34c elemdictSaveElementAnimation]. The items go under the element's
/// position in the hierarchy animation (its full name; -1 when it is not there).
///
/// 1. The first three forward lists in turn: every relation with an id from 0 to 14999 that
///    leads to an element of ANIMATION is a curve; it is written as an item of type 0
///    without action numbers, with the relation entry's active word as the item's flag.
///    (In the first list the ids 15000 to 15010 and 11000 are left out, which the range
///    already does.)
/// 2. Every forward list: the curve a relation entry owns (+8: the curve of the relation's
///    active range) is written with the relation's id as its action relation and the
///    entry's position in its run as occurrence number: the count of entries since the last
///    change of relation id, where for an expression (20010) entries of one serial number
///    count as one. An expression's curve is left out when the relation has no data or its
///    parameter is the custom effects' (0x236).
/// 3. The same for every element that names this one as parent.
///
/// Static values are not written: an item of type 1 read from an animation file was set on
/// its element by the load and is not a relation.
pub fn save_element_animation(sc: &mut Scene, w: &mut Writer, elem: ElemId, ha: &HierAnim) {
    let mut items = Vec::new();
    element_animation_items(sc, elem, ha, &mut |it| items.push(it));
    for it in items {
        // the curve behind the pointer; a pointer that is null or not a curve of this
        // crate gets its header and nothing after it (the original hands the pointer to
        // the curve writer as it is)
        let data = match sc.objects.get(it.curve) {
            Some(Object::Curve { anim, elem, item }) => match sc.objects.get(*anim) {
                Some(Object::Animation(ha)) => ha.elems.get(*elem).and_then(|e| e.items.get(*item)).map(|i| i.data.clone()),
                _ => None,
            },
            _ => None,
        };
        anim::write_item(w, &data.unwrap_or(ItemData::None), it.owner, 0, it.act_rel_id, it.act_occ_id, it.flag);
    }
}

/// One call of `SI_elemdictWriteAnimation` by [`save_element_animation`]: the curve (the
/// object of an ANIMATION element, as the element holds it), the owner's position in the
/// hierarchy animation, the two action numbers and the flag. The item type is always 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimItem {
    pub curve: u32,
    pub owner: i32,
    pub act_rel_id: i32,
    pub act_occ_id: i32,
    pub flag: i16,
}

/// The walk of [`save_element_animation`]: every item it writes, in order, handed to `out`.
pub fn element_animation_items(sc: &mut Scene, elem: ElemId, ha: &HierAnim, out: &mut dyn FnMut(AnimItem)) {
    let cid = sc.program.cid;
    let full = sc.dict.node_of(elem.chap, elem.index).map(|n| or_null(&n.full).to_vec()).unwrap_or_default();
    let owner = hieranim_find_elem_index(ha, &full);
    let curve_of = |sc: &Scene, index: i32| sc.dict.ed.lists[cid.ani as usize].elems.get(index as usize).map_or(0, |e| e.rec.data.0);
    for group in 0..3 {
        let mut next = elemdict::find_range_related_elem(&mut sc.dict, elem, true, group, 0, 14999, cid.ani);
        while let Some((curve, relid)) = next {
            let flag = elemdict::get_cur_relation(&sc.dict, elem, true, group).map_or(1, |r| r.flag);
            let skip = group == 0 && ((15000..=15010).contains(&relid) || relid == 11000);
            if !skip {
                out(AnimItem { curve: curve_of(sc, curve.index), owner, act_rel_id: -1, act_occ_id: -1, flag });
            }
            next = elemdict::find_range_next_related_elem(&mut sc.dict, elem, true, group, 0, 14999, cid.ani);
        }
    }
    let (mut last_serial, mut last_rel, mut occurrence) = (-1, -1, 0);
    let lists = sc.dict.slot(elem).map_or(0, |e| e.forward.len());
    for g in 0..lists {
        let rels = sc.dict.slot(elem).map(|e| e.forward[g].rels.clone()).unwrap_or_default();
        for r in &rels {
            if last_rel == r.relid {
                if r.relid == 0x4e2a && r.serial != -1 && last_serial == r.serial {
                    continue;
                }
                occurrence += 1;
            } else {
                occurrence = 0;
            }
            // SI_edGetFcvFromRelation (0x080ee6f0): the object of the ANIMATION element the
            // entry owns, null when it owns none
            let curve = if r.active_elem >= 0 { curve_of(sc, r.active_elem) } else { 0 };
            if curve != 0 {
                let mut occ = occurrence;
                if r.relid == 0x4e2a {
                    // the occurrence of an expression's curve is the expression's parameter;
                    // without data, or for the custom effects' parameter, nothing is written
                    // and the entry does not count as the last one seen
                    match sc.rel_data.get(r.data.0) {
                        Some(si_ascii::reldata::RelData::Expression(x)) if x.fcurve != 0x236 => occ = x.fcurve,
                        _ => continue,
                    }
                }
                out(AnimItem { curve, owner, act_rel_id: r.relid, act_occ_id: occ, flag: 1 });
            }
            last_rel = r.relid;
            last_serial = r.serial;
        }
    }
    for son in elemrel::related_elems(&mut sc.dict, elem, false, 1, 0x6e, cid.model) {
        element_animation_items(sc, son, ha, out);
    }
}

/// Fills in the names an action's path record carries
/// [soft 0x0812c7c8 actionsPropagatePathName]: for each action with a path record whose
/// first number names a model element, the record's two strings (+0x0c, +0x10) become that
/// element's name and prefix; if its second number names a model element too, the other two
/// strings (+0x14, +0x18) become that one's name and prefix. A missing prefix is the one a
/// split of the full name gives. The record's curve becomes the second element's curve of
/// relation 112 in its second list (→ `SI_edFindFcv`, asked even when there is no second
/// element; the curve read from the file is kept here).
pub fn actions_propagate_path_name(sc: &Scene, actions: &mut [anim::ActionElem], un: &mut dyn Unported) {
    let model = sc.program.cid.model;
    let names = |index: i32| -> Option<(Vec<u8>, Vec<u8>)> {
        let e = sc.dict.slot(ElemId::new(model, index)).filter(|e| e.rec.status != ELEM_DELETED)?;
        let n = sc.dict.nodes.get(e.rec.node.0)?;
        let prefix = match &n.prefix {
            Some(p) => p.clone(),
            None => si_db::filename_expand(or_null(&n.full)).prefix,
        };
        Some((or_null(&n.name).to_vec(), prefix))
    };
    for a in actions.iter_mut() {
        let Some(p) = a.path.as_mut() else { continue };
        let Some((name, prefix)) = names(p.d_00) else { continue };
        p.s_0c = Some(name);
        p.s_10 = Some(prefix);
        if let Some((name, prefix)) = names(p.d_04) {
            p.s_14 = Some(name);
            p.s_18 = Some(prefix);
        }
        un.call("SI_edFindFcv", &[Arg::Elem(ElemId::new(model, p.d_04)), Arg::Int(1), Arg::Int(0x70)]);
    }
}

/// Writes one animation file [soft 0x0812ca2c elemdictSaveHierAnimation]:
/// `<chapter directory><sub-directory><full name>.ani` (with piped files, into the pipe,
/// followed by a 16-bit token 99). If the file cannot be opened nothing is said.
///
/// - Without an animated element the hierarchy animation the animation's element points at
///   is written as it is: its element list under the name of its own chapter, every item
///   with no action numbers and flag 1, the two action lists.
/// - With one, a new hierarchy animation is made for its chapter: the element under its
///   full name, and for a model all its descendants ([`create_hier_anim`]). Its element
///   list is written, then the curves found through the relations
///   ([`save_element_animation`]), then the two action lists of the animation's own object,
///   after their path names have been brought up to date
///   ([`actions_propagate_path_name`]). Around the curves, every action is switched on for
///   the time of the writing and the master keys are put into the animation
///   (→ `SI_actionsTmpActivateAll`, `SI_actionsAddMasterKeysToHA`).
pub fn save_hier_animation(sc: &mut Scene, h: &mut Host, info: &DbInfo, anim_elem: ElemId, target: Option<ElemId>, chap: i32) {
    let n = sc.dict.node_of(anim_elem.chap, anim_elem.index);
    let sub = n.map(|n| or_null(&n.sub_path).to_vec()).unwrap_or_default();
    let full = n.map(|n| or_null(&n.full).to_vec()).unwrap_or_default();
    sc.saving.work_fname = cat(&[or_null(&info.chapter_path), &sub, &full]);
    sc.saving.work_fname_ext = cat(&[&sc.saving.work_fname, b".ani"]);
    let path = sc.saving.work_fname_ext.clone();
    let data = sc.dict.slot(anim_elem).map_or(0, |e| e.rec.data.0);
    let mut w = Writer::with_version(sc.saving.version_output);
    w.st = sc.statics.clone();
    si_file::write_db_header(&mut w.out, w.version_output, si_file::MAGIC_ANIMATION);
    match target {
        None => {
            let Some(Object::Animation(ha)) = sc.objects.get(data) else {
                // the original reads the hierarchy animation through the element's pointer
                // whatever it is; without one nothing can be written here
                h.un.call("si_hieranimBinaryWrite (the animation's object is not a hierarchy animation)", &[Arg::Elem(anim_elem)]);
                sc.saving.not_written.push((anim_elem, "elemdictSaveHierAnimation"));
                return;
            };
            let chapter = ha.chapter.as_deref().map(|n| n.to_vec()).or_else(|| sc.dict.ed.lists.get(sc.program.cid.model as usize).map(|l| l.name.clone()));
            anim::write_ani_body(&mut w, chapter.as_deref(), ha, anim::ItemHeaders::Program);
        }
        Some(t) => {
            let mut ha = HierAnim::new();
            let full = sc.dict.node_of(t.chap, t.index).map(|n| or_null(&n.full).to_vec()).unwrap_or_default();
            ha.add_elem(&full, -1);
            if chap == sc.program.cid.model {
                create_hier_anim(sc, t, &mut ha, true);
            }
            let chapter = sc.dict.ed.lists[chap as usize].name.clone();
            anim::write_hieranim(&mut w, Some(&chapter), &ha);
            h.un.call("SI_actionsTmpActivateAll", &[Arg::Elem(anim_elem), Arg::Int(0)]);
            h.un.call("SI_actionsAddMasterKeysToHA", &[Arg::Elem(anim_elem), Arg::Int(1)]);
            save_element_animation(sc, &mut w, t, &ha);
            h.un.call("SI_actionsAddMasterKeysToHA", &[Arg::Elem(anim_elem), Arg::Int(0)]);
            h.un.call("SI_actionsTmpActivateAll", &[Arg::Elem(anim_elem), Arg::Int(1)]);
            let (mut actions, sources) = match sc.objects.get(data) {
                Some(Object::Animation(a)) => (a.actions.clone(), a.sources.clone()),
                _ => (Vec::new(), Vec::new()),
            };
            actions_propagate_path_name(sc, &mut actions, h.un);
            anim::write_actions(&mut w, &actions, false);
            anim::write_actions(&mut w, &sources, true);
            if let Some(Object::Animation(a)) = sc.objects.get_mut(data) {
                a.actions = actions;
            }
        }
    }
    if sc.saving.piped {
        w.st.token_elemdict = 99;
        w.put_u16(99);
    }
    sc.statics = w.st.clone();
    put_file(sc, h, anim_elem, &path, &w.out);
}

/// Saves the animation file of one animation element [soft 0x0812d0b4 elemdictSaveAnimFile];
/// false when the save is to stop ("Cancel All").
///
/// `target` is the element the animation is related to and `chap` its chapter. With a
/// target, the file is written only when the target is in use and either has an object, is
/// a root and (for "selected only") is selected or the save is an emergency one, or else is
/// the scene's own element. Then, unless files are piped, the animation's element gets its
/// name ([`find_save_name`]: a shared animation that cannot be written is passed over), and
/// the file is written ([`save_hier_animation`]).
#[allow(clippy::too_many_arguments)]
pub fn save_anim_file(sc: &mut Scene, h: &mut Host, info: &mut DbInfo, anim_elem: ElemId, target: Option<ElemId>, chap: i32, selected_only: bool, emergency: bool, listing: Option<&[ListNode]>, ask: &mut i16) -> bool {
    save_anim_file_with(&mut Real, sc, h, info, anim_elem, target, chap, selected_only, emergency, listing, ask)
}

/// [`save_anim_file`] with its callees given ([`Steps`]).
#[allow(clippy::too_many_arguments)]
pub fn save_anim_file_with(steps: &mut dyn Steps, sc: &mut Scene, h: &mut Host, info: &mut DbInfo, anim_elem: ElemId, target: Option<ElemId>, chap: i32, selected_only: bool, emergency: bool, listing: Option<&[ListNode]>, ask: &mut i16) -> bool {
    let cid = sc.program.cid;
    if let Some(t) = target {
        let Some(e) = sc.dict.slot(t).map(|e| e.rec) else { return true };
        let direct = !e.data.is_null() && e.status == ELEM_VALID && e.hier_level == 0 && (!selected_only || e.selected != 0 || emergency);
        if !direct {
            if e.status != ELEM_VALID {
                return true;
            }
            if chap != cid.dsc {
                return true;
            }
        }
    }
    if !sc.saving.piped {
        match steps.find_save_name(sc, h, info, cid.ani, anim_elem, ask, true, listing) {
            NAME_CANCEL_ALL => {
                h.un.call("SI_dialogInProgressClose", &[]);
                return false;
            }
            NAME_SHARE_NOT_SAVED | NAME_SHARE_SKIP_ALL => {
                h.un.call("SI_dialogInProgressRedraw", &[]);
                return true;
            }
            _ => h.un.call("SI_dialogInProgressRedraw", &[]),
        }
    }
    steps.save_hier_animation(sc, h, info, anim_elem, target, chap);
    true
}

/// Saves every animation file [soft 0x0812d1ac elemdictSaveAllAnimFiles]: for each root in
/// use of the ANIMATION chapter, in index order, the element it animates is the first one
/// related to it by one of the animation relations 15000 to 15010 (backward side, list 0);
/// without one the animation is saved as it is, as an animation of the ANIMATION chapter.
/// False when there is no such root or the save was cancelled. The progress window says
/// `Saving ANIMATION for <full name>`.
pub fn save_all_anim_files(sc: &mut Scene, h: &mut Host, info: &mut DbInfo, selected_only: bool, emergency: bool, listing: Option<&[ListNode]>, ask: &mut i16) -> bool {
    save_all_anim_files_with(&mut Real, sc, h, info, selected_only, emergency, listing, ask)
}

/// [`save_all_anim_files`] with its callee given ([`Steps`]).
#[allow(clippy::too_many_arguments)]
pub fn save_all_anim_files_with(steps: &mut dyn Steps, sc: &mut Scene, h: &mut Host, info: &mut DbInfo, selected_only: bool, emergency: bool, listing: Option<&[ListNode]>, ask: &mut i16) -> bool {
    let ani = sc.program.cid.ani;
    let roots = sc.dict.ed.lists[ani as usize].elems.iter().filter(|e| e.rec.status == ELEM_VALID && e.rec.hier_level == 0).count() as i64;
    if roots == 0 {
        return false;
    }
    h.un.call("SI_dialogInProgressSetTexts", &[Arg::Str(b"Save Elements".to_vec()), Arg::Str(Vec::new())]);
    let mut done = 1;
    let mut i = 0;
    while (i as usize) < sc.dict.ed.lists[ani as usize].elems.len() {
        let e = sc.dict.ed.lists[ani as usize].elems[i as usize].rec;
        if e.status == ELEM_VALID && e.hier_level == 0 {
            let id = ElemId::new(ani, i);
            let relid = elemdict::find_range_related_elem(&mut sc.dict, id, false, 0, dbvsfcv::REL_ANIM_FIRST, dbvsfcv::REL_ANIM_LAST, -1).map(|(_, r)| r);
            // without a relation in the range the original looks for the relation whose id
            // is whatever its variable held; nothing is found
            let target = relid.and_then(|r| elemrel::find_related_elem(&mut sc.dict, id, false, 0, r, -1));
            let chap = target.map_or(ani, |t| t.chap);
            if !steps.save_anim_file(sc, h, info, id, target, chap, selected_only, emergency, listing, ask) {
                return false;
            }
            let full = sc.dict.node_of(ani, i).map(|n| or_null(&n.full).to_vec()).unwrap_or_default();
            h.un.call("SI_dialogInProgressSet", &[Arg::Str(cat(&[b"Saving ANIMATION for ", &full])), Arg::Int(done), Arg::Int(roots)]);
            done += 1;
        }
        i += 1;
    }
    true
}

/// Saves the element files of a dictionary [soft 0x0812de10 SI_elemdictSaveElements]; false
/// when a chapter stopped the save.
///
/// 1. A second, empty dictionary is made for the listings of the database's chapters, and
///    the progress window `Save Elements` opens.
/// 2. **MODELS**, then every other chapter in dictionary order ([`save_chapter_elements`]).
///    A chapter that fails ends everything here: what was written stays, the marks of the
///    elements are not cleared, the progress window stays open (the failing chapter closed
///    it and showed its dialogue).
/// 3. **The animation files**, if the ANIMATION chapter has a slot: its files are listed
///    (and, unless old versions are kept, the unused ones flagged) and
///    [`save_all_anim_files`] writes one per root.
/// 4. Unless this is an emergency save, the marks are cleared in every chapter (or only
///    `only_chap`): for a full save "dirty" and the new-flag of every slot that had either;
///    for "selected only" of every slot whose own selected word is set. And every name
///    record of a slot not freed learns the database (`si_elemdictSetOriginDB`).
#[allow(clippy::too_many_arguments)]
pub fn save_elements(sc: &mut Scene, h: &mut Host, info: &mut DbInfo, selected_only: bool, emergency: bool, only_chap: i32, ask: &mut i16) -> bool {
    save_elements_with(&mut Real, sc, h, info, selected_only, emergency, only_chap, ask)
}

/// [`save_elements`] with its two callees given ([`Steps`]).
#[allow(clippy::too_many_arguments)]
pub fn save_elements_with(steps: &mut dyn Steps, sc: &mut Scene, h: &mut Host, info: &mut DbInfo, selected_only: bool, emergency: bool, only_chap: i32, ask: &mut i16) -> bool {
    let cid = sc.program.cid;
    h.un.call("SI_dialogInProgressOpen", &[Arg::Str(b"Save Elements".to_vec())]);
    if !steps.save_chapter(sc, h, info, selected_only, emergency, only_chap, cid.model, ask) {
        return false;
    }
    let nb = sc.dict.ed.lists.len() as i32;
    for chap in 0..nb {
        if chap != cid.model && !steps.save_chapter(sc, h, info, selected_only, emergency, only_chap, chap, ask) {
            return false;
        }
    }
    let chapter = sc.dict.ed.lists[cid.ani as usize].name.clone();
    info.set_chapter(&chapter);
    info.filepath_build();
    si_db::db_info_get_nb_versions(info, &chapter, Some(&*h.fs));
    if sc.dict.ed.lists[cid.ani as usize].rec.nb_elems > 0 {
        let mut listing: Option<Vec<ListNode>> = None;
        if !sc.saving.piped {
            let text: Vec<u8> = if sc.saving.no_old_ver == 0 { cat(&[b"Checking scenes for old ", &chapter]) } else { b"Checking latest versions".to_vec() };
            h.un.call("SI_dialogInProgressSetTexts", &[Arg::Str(text), Arg::Str(Vec::new())]);
            h.un.call("SI_dialogInProgressSet", &[Arg::Str(b"Please wait...".to_vec()), Arg::Int(0)]);
            let suffix = si_types::store::get_suffix_from_chapter(&cid, cid.ani).to_vec();
            let s = Session { fs: &*h.fs, now: sc.program.now, loading: sc.program.loading };
            let mut lst = None;
            let mut log = Log::default();
            let found = si_db::chapter_list_elements(&s, info, Some(&suffix), true, true, None, &mut lst, &mut log);
            listing = Some(found.into_iter().map(|f| f.node).collect());
            if sc.saving.no_old_ver == 0 {
                h.un.call("SI_dbMFlagUnused", &[Arg::Str(chapter.clone()), Arg::Int(1)]);
            }
        }
        steps.save_all_anim(sc, h, info, selected_only, emergency, listing.as_deref(), ask);
    }
    if !emergency {
        for chap in 0..nb {
            if only_chap == -1 || chap == only_chap {
                for e in sc.dict.ed.lists[chap as usize].elems.iter_mut() {
                    let hit = if selected_only { e.rec.selected != 0 } else { e.rec.dirty != 0 || e.rec.new_flag != 0 };
                    if hit {
                        e.rec.dirty = 0;
                        e.rec.new_flag = 0;
                    }
                }
            }
        }
        set_origin_db(sc, info);
    }
    h.un.call("SI_dialogInProgressClose", &[]);
    true
}

/// Every name record of a slot that is not freed learns the database it now belongs to
/// [soft 0x08137918 si_elemdictSetOriginDB]: its first word becomes the pointer to the
/// database's path.
pub fn set_origin_db(sc: &mut Scene, info: &DbInfo) {
    let mark = sc.db_mark(info);
    let d = &mut sc.dict;
    for l in d.ed.lists.iter() {
        for e in l.elems.iter() {
            if e.rec.status != ELEM_DELETED {
                if let Some(n) = d.nodes.get_mut(e.rec.node.0) {
                    n.w00 = mark;
                }
            }
        }
    }
}

/// Writes a struct key file for an element and its descendants
/// [soft 0x0812ce68 SI_elemdictSaveHierStructKey] [soft 0x0812cce0 elemdictSaveElementStructKey]:
/// header with the struct key magic, the hierarchy's element list (the element under its
/// full name, a model's descendants after it), and per element one static value for each
/// curve of each attribute of the element's chapter that has curves (the attribute ids 0 to
/// 3999 are tried in order; an attribute belongs to a chapter by its relation's source
/// chapter), as items of type 1.
///
/// The value of an attribute's curve is read from the element by another package
/// (→ `SI_edElementStaticValueGet`): `value` answers it, `None` for "has none". The file is
/// written to `path` (with piped files, into the pipe).
pub fn save_hier_struct_key(sc: &mut Scene, h: &mut Host, path: &[u8], chap: i32, elem: ElemId, value: &mut dyn FnMut(ElemId, i32) -> Option<f32>) {
    let mut w = Writer::with_version(sc.saving.version_output);
    w.st = sc.statics.clone();
    si_file::write_db_header(&mut w.out, w.version_output, si_file::MAGIC_STRUCT_KEYS);
    let mut ha = HierAnim::new();
    let full = sc.dict.node_of(elem.chap, elem.index).map(|n| or_null(&n.full).to_vec()).unwrap_or_default();
    ha.add_elem(&full, -1);
    if chap == sc.program.cid.model {
        create_hier_anim(sc, elem, &mut ha, true);
    }
    let chapter = sc.dict.ed.lists[chap as usize].name.clone();
    anim::write_hieranim(&mut w, Some(&chapter), &ha);
    fn element(sc: &mut Scene, w: &mut Writer, elem: ElemId, chap: i32, ha: &HierAnim, value: &mut dyn FnMut(ElemId, i32) -> Option<f32>) {
        let full = sc.dict.node_of(elem.chap, elem.index).map(|n| or_null(&n.full).to_vec()).unwrap_or_default();
        let owner = hieranim_find_elem_index(ha, &full);
        for att in 0..4000 {
            let first = si_dict::attdict::get_fcurve_from_att_id(&sc.mem, si_dict::globals::SI_ATD, att);
            if first < 0 {
                continue;
            }
            let nb = si_dict::attdict::get_nb_fcurves_for_att_id(&sc.mem, si_dict::globals::SI_ATD, att);
            if nb <= 0 {
                continue;
            }
            let (src, _) = sc.program.rd.get_rel_chap_grp(att);
            if src < 0 || src != chap {
                continue;
            }
            for id in first..first + nb {
                if let Some(v) = value(elem, id) {
                    let sv = si_elemio::fcurve::StaticVal { curve_type: id, value: v };
                    anim::write_item(w, &ItemData::Static(sv), owner, 1, -1, -1, 1);
                }
            }
        }
        let model = sc.program.cid.model;
        for son in elemrel::related_elems(&mut sc.dict, elem, false, 1, 0x6e, model) {
            element(sc, w, son, chap, ha, value);
        }
    }
    element(sc, &mut w, elem, chap, &ha, value);
    sc.statics = w.st.clone();
    put_file(sc, h, elem, path, &w.out);
}
