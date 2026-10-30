// SPDX-License-Identifier: MIT
//! `dictlist.c`: the browser's list handling (element lists and text lists of the file dialogs,
//! path helpers, the search and the strings handed back to the caller). Finding 0007,
//! 2026-10-28 (third pass).
//!
//! A text list is a count (short at +0) and an array of 0x14-byte nodes (+4): selection short
//! at +0, a second short at +2, text pointer at +4, element index word at +8. An element list
//! has its element count at +4 and its 0x84-byte elements at +8 (field offsets in the
//! functions below). Routines of other source files are typed calls ([`DbCalls`]).

use crate::calls::DbCalls;
use crate::imglobals::{DB_WORK_FNAME, SI_ELEM_DICTS, SI_PROC_DATA};
use si_mem::clib;
use si_mem::{Mem, Outside};

const NODE: u32 = 0x14;
const ELEM: u32 = 0x84;
/// `SI_dBWorkFnameExt` (dictlist.c / dbexternal.c): the second scratch file-name buffer.
pub const DB_WORK_FNAME_EXT: u32 = 0x08788540;
/// `workStr2`: the browser's second scratch text buffer.
pub const WORK_STR2: u32 = 0x08784840;
/// Two words the list-title routine compares its chapter index with (the lights and models
/// chapter indices of the dictionary; unnamed in the symbol table).
pub const CHAP_LIGHTS: u32 = 0x08781718;
pub const CHAP_MODELS: u32 = 0x08781720;

/// The table of old file suffixes (`oldsuffixes`, 12 pointers, the last null) and its strings.
pub const OLD_SUFFIXES: u32 = 0x0867fba0;
pub mod strings {
    pub const SUFFIX_NAMES: [(u32, &str); 11] = [
        (0x0867fb68, "scn"), (0x0867fb6c, "hrc"), (0x0867fb70, "mtr"), (0x0867fb74, "txt"), (0x0867fb78, "t3D"),
        (0x0867fb7c, "shp"), (0x0867fb80, "pic"), (0x0867fb84, "lin"), (0x0867fb88, "pal"), (0x0867fb8c, "col"),
        (0x0867fb90, "rsrc"),
    ];
    pub const LIGHTS_LIST: u32 = 0x0868091f;
    pub const MODELS_LIST: u32 = 0x0868092b;
}

/// Writes the string literals and tables of this file into the image.
pub fn store_strings(m: &mut Mem) {
    for (i, (a, s)) in strings::SUFFIX_NAMES.iter().enumerate() {
        m.put_cstr(*a, s.as_bytes());
        m.set_u32(OLD_SUFFIXES + 4 * i as u32, *a);
    }
    m.set_u32(OLD_SUFFIXES + 44, 0);
    m.put_cstr(strings::LIGHTS_LIST, b"Lights List");
    m.put_cstr(strings::MODELS_LIST, b"Models List");
}

/// A `%s` argument as the C library prints it.
fn arg(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 { b"(null)".to_vec() } else { m.cstr(p) }
}

fn put(m: &mut Mem, dst: u32, parts: &[&[u8]]) {
    let v: Vec<u8> = parts.iter().flat_map(|p| p.iter().copied()).collect();
    m.put_cstr(dst, &v);
}

fn node(tl: u32, m: &Mem, i: u32) -> u32 {
    m.u32(tl + 4).wrapping_add(i.wrapping_mul(NODE))
}

fn elem_at(m: &Mem, list: u32, idx: u32) -> u32 {
    m.u32(list + 8).wrapping_add(idx.wrapping_mul(ELEM))
}

/// The element dictionary of the current process data (0x70 bytes per index).
fn current_dict(m: &Mem) -> u32 {
    let idx = m.u32(m.u32(SI_PROC_DATA.0));
    m.u32(SI_ELEM_DICTS.0).wrapping_add(idx.wrapping_mul(0x70))
}

/// [soft 0x080e1290 dbGetPathFromFullname]: the directory part of a path (up to the last
/// slash, a lone `/` for a path whose only slash is the first character) in a new block, and
/// when `rest` is given the name after the slash (only when it is not empty) in another one
/// stored through it. A previous block behind `rest` is freed first. Null when there is no
/// slash (or the path is null).
pub fn get_path_from_fullname(m: &mut Mem, full: u32, rest: u32) -> u32 {
    if full == 0 {
        return 0;
    }
    if rest != 0 && m.u32(rest) != 0 {
        let old = m.u32(rest);
        m.free(old);
        m.set_u32(rest, 0);
    }
    let len = m.strlen(full) as i32;
    if len <= 0 {
        return 0;
    }
    let mut n = len;
    let (mut at, mut after);
    loop {
        after = n;
        n = after - 1;
        at = n;
        if n < 0 {
            return 0;
        }
        if m.u8(full + n as u32) == b'/' {
            break;
        }
    }
    let size = if at == 0 { 2 } else { after };
    let res = m.si_malloc(size as u32);
    if res != 0 {
        if at == 0 {
            clib::strncpy(m, res, full, after as u32);
            m.set_u8(res + 1, 0);
        } else {
            clib::strncpy(m, res, full, at as u32);
            m.set_u8(res + at as u32, 0);
        }
    }
    if rest != 0 && len - at > 1 {
        let d = m.si_malloc((len - at) as u32);
        m.set_u32(rest, d);
        if d != 0 {
            clib::strcpy(m, d, full + after as u32);
        }
    }
    res
}

/// [soft 0x080e13d4 dbConcatPathAndFilename]: `dir` and `name` into `out`: `dir/name`, or
/// `dirname` when `dir` already ends in a slash, or just `name` when `dir` is null or empty.
pub fn concat_path_and_filename(m: &mut Mem, dir: u32, name: u32, out: u32) {
    if out == 0 {
        return;
    }
    let nm = arg(m, name);
    if dir != 0 {
        let len = m.strlen(dir);
        if len != 0 {
            let d = m.cstr(dir);
            if m.u8(dir + len - 1) != b'/' {
                put(m, out, &[&d, b"/", &nm]);
            } else {
                put(m, out, &[&d, &nm]);
            }
            return;
        }
    }
    put(m, out, &[&nm]);
}

/// [soft 0x080e158c dbListCollapse]: for an element of the list (by index) that is not marked
/// in its +0x54 word, and which is a head in the list's group arrays (+0x10 indices, +0x14
/// counts, +4 entries), toggles the open/closed short (+0x2a) of the node of it and of the
/// following members; 1 when it has no group arrays, else whether it found the group.
pub fn list_collapse(m: &mut Mem, o: &mut dyn Outside, list: u32, idx: i32) -> i32 {
    let e = o.elemlist_get_elem(m, list, idx);
    if m.u32(e + 0x54) != 0 {
        return 0;
    }
    let heads = m.u32(list + 0x10);
    if heads == 0 {
        return 1;
    }
    let count = m.i32(list + 4);
    let mut pos: Option<i32> = None;
    let mut i = 0i32;
    while i < count {
        if m.i32(heads.wrapping_add((i as u32).wrapping_mul(4))) == idx {
            pos = Some(i);
            break;
        }
        i += 1;
    }
    let Some(pos) = pos else { return 0 };
    let mut n = m.i32(m.u32(list + 0x14).wrapping_add((pos as u32).wrapping_mul(4)));
    if n < 1 {
        return 0;
    }
    let mut p = pos;
    loop {
        let hs = m.u32(list + 0x10);
        let ei = m.u32(hs.wrapping_add((p as u32).wrapping_mul(4)));
        let nd = m.u32(elem_at(m, list, ei));
        p -= 1;
        let cur = m.i16(nd + 0x2a);
        m.set_u16(nd + 0x2a, (cur == 0) as u16);
        n -= 1;
        if n < 0 {
            break;
        }
    }
    1
}

/// [soft 0x080e1638 dbListCollapseAll]: sets the open/closed short (+0x2a) of every element
/// node (of the elements whose +0x48 short is zero) to the opposite of the dictionary's
/// own flag (+0x1e), and flips that flag.
pub fn list_collapse_all(m: &mut Mem, dict: u32, list: u32) {
    let flag = (m.i16(dict + 0x1e) == 0) as u16;
    let n = m.i32(list + 4);
    for i in 0..n {
        let e = elem_at(m, list, i as u32);
        if m.i16(e + 0x48) == 0 {
            let nd = m.u32(e);
            if nd != 0 {
                m.set_u16(nd + 0x2a, flag);
            }
        }
    }
    m.set_u16(dict + 0x1e, flag);
}

fn slashes(m: &Mem, s: u32) -> i32 {
    m.cstr(s).iter().filter(|&&c| c == b'/').count() as i32
}

/// [soft 0x080e2138 dbDisplaySubDir]: whether a sub-directory path is shown in the list whose
/// current directory is at +0x28: with no current directory the paths of at most one slash; else
/// those with as many slashes as it or one more, that start with it and are not equal to it.
pub fn display_sub_dir(m: &mut Mem, list: u32, path: u32) -> i32 {
    let n = slashes(m, path);
    let cur = m.u32(list + 0x28);
    if cur == 0 {
        return (n < 2) as i32;
    }
    let k = slashes(m, cur);
    if n < k || k + 1 < n {
        return 0;
    }
    if clib::strcmp(m, path, cur) == 0 {
        return 0;
    }
    let len = m.strlen(cur);
    (clib::strncmp(m, path, cur, len) == 0) as i32
}

/// [soft 0x080e4788 txtlistTransferSelection2ElemList]: copies each node's selection short into
/// its element's selected short (+0x42) and counts the selected ones in the list's +0x1c
/// (from node 1 when `skip` is set).
pub fn txtlist_transfer_selection_to_elemlist(m: &mut Mem, tl: u32, list: u32, skip: i16) {
    m.set_u32(list + 0x1c, 0);
    let n = m.i16(tl) as i32;
    let mut i = (skip != 0) as i32;
    while i < n {
        let nd = node(tl, m, i as u32);
        let idx = m.i32(nd + 8);
        if idx >= 0 {
            let sel = m.i16(nd);
            let e = elem_at(m, list, idx as u32);
            m.set_i16(e + 0x42, sel);
            if sel != 0 {
                let c = m.u32(list + 0x1c);
                m.set_u32(list + 0x1c, c.wrapping_add(1));
            }
        }
        i += 1;
    }
}

/// [soft 0x080e480c elemlistTransferFlags2TextList]: copies each element's work flag (+0x58,
/// low short) into its node's selection short (from node 1 when `skip` is set).
pub fn elemlist_transfer_flags_to_textlist(m: &mut Mem, tl: u32, list: u32, skip: i16) {
    let n = m.i16(tl) as i32;
    let mut i = (skip != 0) as i32;
    while i < n {
        let nd = node(tl, m, i as u32);
        let idx = m.i32(nd + 8);
        if idx >= 0 {
            let e = elem_at(m, list, idx as u32);
            let v = m.i16(e + 0x58);
            m.set_i16(nd, v);
        }
        i += 1;
    }
}

/// [soft 0x080e4a8c dbSetPickable]: for each selected (selection 1) node with an element, makes
/// the element pickable or not (`flag`) in the dictionary's list named like the element list.
pub fn set_pickable(m: &mut Mem, o: &mut dyn Outside, tl: u32, list: u32, skip: i16, flag: i16) {
    let dict = current_dict(m);
    let name = m.u32(list);
    let found = o.elemdict_find_list_by_name(m, dict, name);
    let n = m.i16(tl) as i32;
    let mut i = (skip != 0) as i32;
    while i < n {
        let nd = node(tl, m, i as u32);
        let idx = m.i32(nd + 8);
        if idx >= 0 && m.i16(nd) == 1 {
            let e = elem_at(m, list, idx as u32);
            o.ed_set_pickable(m, e, found, flag as i32);
        }
        i += 1;
    }
}

/// [soft 0x080e4b40 dialogGetPicked]: the first node with selection 1: stores 1 in `*found`,
/// its second short in `*second` and returns its element index; else stores 0 and 0 and
/// returns -1.
pub fn dialog_get_picked(m: &mut Mem, tl: u32, found: u32, second: u32) -> i32 {
    m.set_u16(found, 1);
    m.set_u32(second, 0);
    let n = m.i16(tl) as i32;
    for i in 0..n {
        let nd = node(tl, m, i as u32);
        if m.i16(nd) == 1 {
            let v = m.i16(nd + 2) as i32;
            m.set_u32(second, v as u32);
            return m.i32(nd + 8);
        }
    }
    m.set_u16(found, 0);
    -1
}

/// [soft 0x080e4ba4 dialogGetPickedStr]: the text pointer of the first node with selection 1, or 0.
pub fn dialog_get_picked_str(m: &Mem, tl: u32) -> u32 {
    let n = m.i16(tl) as i32;
    for i in 0..n {
        let nd = node(tl, m, i as u32);
        if m.i16(nd) == 1 {
            return m.u32(nd + 4);
        }
    }
    0
}

/// [soft 0x080e4d64 dbAddTypedElementToList]: adds a list node for a typed file name to the
/// element list and marks the new element selected (+0x42).
pub fn add_typed_element_to_list(m: &mut Mem, o: &mut dyn Outside, name: u32, list: u32, kind: i16) {
    let nd = o.db_scn_get_list_node(m, name, 0, kind as i32);
    let i = o.elemlist_add_elem(m, list, nd, 0, 0, 0, 0, 1, 1);
    let e = elem_at(m, list, i as u32);
    m.set_u16(e + 0x42, 1);
}

/// [soft 0x080e4dac dbPrepareWithAndWithoutSuffixStrings]: `out1` gets the name without its
/// suffix and `out2` the name; when the name had no suffix, or its suffix differs from the
/// list's (+0x3c), and the list has a suffix other than `all`, `out2` becomes `out1.suffix`.
pub fn prepare_with_and_without_suffix(m: &mut Mem, o: &mut dyn Outside, list: u32, name: u32, out1: u32, out2: u32) {
    if name == 0 {
        return;
    }
    let s1 = o.str_strip_suffix(m, name);
    if out1 != 0 {
        let t = arg(m, s1);
        put(m, out1, &[&t]);
    }
    if out2 != 0 {
        let t = arg(m, name);
        put(m, out2, &[&t]);
    }
    let same = clib::strcmp(m, s1, name) == 0;
    let mut suffix_ok = true;
    if !same {
        let len = m.strlen(s1);
        let sfx = m.u32(list + 0x3c);
        if sfx != 0 && clib::strcmp(m, name.wrapping_add(len).wrapping_add(1), sfx) != 0 {
            suffix_ok = false;
        }
    }
    if out2 != 0 && (same || !suffix_ok) {
        let sfx = m.u32(list + 0x3c);
        if sfx != 0 && m.cstr(sfx) != b"all" {
            let a = arg(m, out1);
            let b = arg(m, sfx);
            put(m, out2, &[&a, b".", &b]);
        }
    }
    if s1 != 0 {
        m.free(s1);
    }
}

/// [soft 0x080e4ef0 dbPrepareReturnStrings]: the directory (`*dir_out`, a new block, a trailing
/// slash dropped) and the suffix (`*sfx_out`) to hand back for a typed name or a chosen element,
/// with the work buffers `SI_dBWorkFname` and `SI_dBWorkFnameExt` set on the way.
#[allow(clippy::too_many_arguments)]
pub fn prepare_return_strings(m: &mut Mem, o: &mut dyn Outside, name: u32, info: u32, list: u32, idx: i32, dir_out: u32, sfx_out: u32) {
    let mut el = 0u32;
    if idx >= 0 && idx < m.i32(list + 4) {
        el = elem_at(m, list, idx as u32);
    }
    let fname = DB_WORK_FNAME.0;
    let mut trim = false;
    if name == 0 && el != 0 {
        let nd = m.u32(el);
        let a = arg(m, m.u32(info + 0x14));
        let b = arg(m, m.u32(nd + 8));
        put(m, fname, &[&a, &b]);
        if dir_out != 0 {
            let d = m.strdup(fname);
            m.set_u32(dir_out, d);
        }
        let leaf = m.u32(nd + 0x24);
        let cur = m.cstr(fname);
        let l = arg(m, leaf);
        put(m, fname, &[&cur, &l]);
        let esfx = m.u32(el + 0x80);
        if esfx == 0 {
            let lsfx = m.u32(list + 0x3c);
            if lsfx != 0 && m.cstr(lsfx) != b"all" {
                let f = m.cstr(fname);
                let s = m.cstr(lsfx);
                put(m, DB_WORK_FNAME_EXT, &[&f, b".", &s]);
            } else {
                let f = m.cstr(fname);
                put(m, DB_WORK_FNAME_EXT, &[&f]);
            }
        } else {
            let f = m.cstr(fname);
            let s = m.cstr(esfx);
            put(m, DB_WORK_FNAME_EXT, &[&f, b".", &s]);
        }
        trim = dir_out != 0;
    } else {
        prepare_with_and_without_suffix(m, o, list, name, fname, DB_WORK_FNAME_EXT);
        if dir_out != 0 {
            let mut p = m.u32(list + 0x28);
            if p == 0 {
                p = m.u32(info + 0x14);
            }
            let d = m.strdup(p);
            m.set_u32(dir_out, d);
            trim = true;
        }
    }
    if trim {
        let d = m.u32(dir_out);
        if d != 0 {
            let len = m.strlen(d);
            let last = d.wrapping_add(len).wrapping_sub(1);
            if m.u8(last) == b'/' {
                m.set_u8(last, 0);
            }
        }
    }
    if sfx_out == 0 {
        return;
    }
    let lsfx = m.u32(list + 0x3c);
    if lsfx != 0 && m.cstr(lsfx) != b"all" {
        let d = m.strdup(lsfx);
        m.set_u32(sfx_out, d);
        return;
    }
    let esfx = if el == 0 { 0 } else { m.u32(el + 0x80) };
    if esfx == 0 {
        let dot = clib::strrchr(m, DB_WORK_FNAME_EXT, b'.');
        if dot != 0 {
            let d = m.strdup(dot + 1);
            m.set_u32(sfx_out, d);
        } else {
            m.set_u32(sfx_out, 0);
        }
        return;
    }
    let d = m.strdup(esfx);
    m.set_u32(sfx_out, d);
}

/// [soft 0x080e4be4 dbSetSubDirectory]: takes the directory from the first selected node: a node
/// with an index below -1 names it (its text is copied to +0x28 of the list); index -1 means "up":
/// the last path component is cut off the current directory. 1 when it changed something.
pub fn set_sub_directory(m: &mut Mem, tl: u32, list: u32) -> i32 {
    let n = m.i16(tl) as i32;
    for i in 0..n {
        let nd = node(tl, m, i as u32);
        if m.i16(nd) == 0 {
            continue;
        }
        let idx = m.i32(nd + 8);
        if idx < -1 {
            let old = m.u32(list + 0x28);
            if old != 0 {
                m.free(old);
                m.set_u32(list + 0x28, 0);
            }
            let t = m.u32(nd + 4);
            let d = m.strdup(t);
            m.set_u32(list + 0x28, d);
            return 1;
        }
        let cur = m.u32(list + 0x28);
        if idx == -1 && cur != 0 {
            let len = m.strlen(cur);
            if len != 0 {
                m.set_u8(cur.wrapping_add(len).wrapping_sub(1), 0);
                let cur = m.u32(list + 0x28);
                let r = clib::strrchr(m, cur, b'/');
                if r == 0 {
                    let c = m.u32(list + 0x28);
                    if c != 0 {
                        m.free(c);
                    }
                    m.set_u32(list + 0x28, 0);
                } else {
                    m.set_u8(r + 1, 0);
                }
                return 1;
            }
        }
    }
    0
}

/// [soft 0x080e5ce0 txtlistSetSelection]: sets every node's selection short (from node 1 when
/// `skip` is set).
pub fn txtlist_set_selection(m: &mut Mem, tl: u32, skip: i16, value: i16) {
    let n = m.i16(tl) as i32;
    let mut i = (skip != 0) as i32;
    while i < n {
        let nd = node(tl, m, i as u32);
        m.set_i16(nd, value);
        i += 1;
    }
}

/// [soft 0x080ea5cc dialogGetSuffixFromId]: the old suffix text for an id below 12, else 0.
pub fn dialog_get_suffix_from_id(m: &Mem, id: i32) -> u32 {
    if (id as u32) < 12 { m.u32(OLD_SUFFIXES + 4 * id as u32) } else { 0 }
}

/// [soft 0x080ea5e8 dialogGetIdFromSuffix]: the id of an old suffix, 11 when it is none (or null).
pub fn dialog_get_id_from_suffix(m: &Mem, s: u32) -> i32 {
    if s != 0 {
        for i in 0..12u32 {
            let p = m.u32(OLD_SUFFIXES + 4 * i);
            if p != 0 && clib::strcmp(m, s, p) == 0 {
                return i as i32;
            }
        }
    }
    11
}

/// [soft 0x080eba84 copySelFieldsInTextList]: copies the selection shorts of nodes 1.. of `a`
/// to nodes 0.. of `b`.
pub fn copy_sel_fields_in_text_list(m: &mut Mem, a: u32, b: u32) {
    let n = m.i16(a) as i32;
    let mut i = 1;
    while i < n {
        let v = m.i16(node(a, m, i as u32));
        let dst = node(b, m, (i - 1) as u32);
        m.set_i16(dst, v);
        i += 1;
    }
}

/// [soft 0x080e6b54 reverseText]: reverses the order of the nodes of a text list.
pub fn reverse_text(m: &mut Mem, tl: u32) {
    let n = m.i16(tl) as i32;
    let half = (n as f64 * 0.5) as i32; // truncated (the original switches the x87 rounding to truncate)
    for t in 0..half.max(0) {
        let a = node(tl, m, t as u32);
        let b = node(tl, m, (n - 1 - t) as u32);
        let mut tmp = [0u8; NODE as usize];
        for k in 0..NODE {
            tmp[k as usize] = m.u8(a + k);
        }
        for k in 0..NODE {
            let v = m.u8(b + k);
            m.set_u8(a + k, v);
        }
        for k in 0..NODE {
            m.set_u8(b + k, tmp[k as usize]);
        }
    }
}

/// [soft 0x080ed2fc setSelectionDialog]: titles the dialog "Lights List" or "Models List" by the
/// chapter index and sets the +0x14 short of its item data record (+0x60 of the dialog's item
/// array, +0x14).
pub fn set_selection_dialog(m: &mut Mem, o: &mut dyn Outside, dlg: u32, chap: i32) {
    if chap == m.i32(CHAP_LIGHTS) {
        o.set_dialog_title(m, dlg, strings::LIGHTS_LIST);
    } else if chap == m.i32(CHAP_MODELS) {
        o.set_dialog_title(m, dlg, strings::MODELS_LIST);
    }
    let items = m.u32(dlg + 0x14);
    let rec = m.u32(items + 0x60);
    m.set_u16(rec + 0x14, 1);
}

/// [soft 0x080e7a54 dbSearchString]: looks for `text` in the names of the list's elements, from
/// the node after the first selected one (`from_sel`) on. `part` says which name: 0 the
/// name record's +0x10, 1 its +0x14, 2 `+0x10` and `+0x14` joined by the prefix formatter;
/// `exact` and `fold` choose the comparison (case-insensitive equal, or substring; the strcmp
/// the original calls in one combination has its result dropped). `clear` unselects all first;
/// `all` goes on after the first hit; `mark` selects the hits. Returns whether there was a hit.
#[allow(clippy::too_many_arguments)]
pub fn search_string(m: &mut Mem, o: &mut dyn Outside, list: u32, tl: u32, text: u32, p4: i16, p5: i16, part: i32, from_sel: i16, all: i16, mark: i16) -> i16 {
    let n = m.i16(tl) as i32;
    let mut start = 0i32;
    let mut found_sel = false;
    for i in 0..n {
        let nd = node(tl, m, i as u32);
        if from_sel != 0 && !found_sel && m.i16(nd) != 0 {
            found_sel = true;
            start = i + 1;
        }
        if mark != 0 {
            m.set_u16(nd, 0);
        }
    }
    let mut res = 0i16;
    let mut name = 0u32; // (the original's register keeps the previous element's name when `part` is outside 0..2)
    let mut i = start;
    while i < m.i16(tl) as i32 {
        let nd = node(tl, m, i as u32);
        let idx = m.i32(nd + 8);
        if idx >= 0 {
            let e = elem_at(m, list, idx as u32);
            let rec = m.u32(e);
            match part {
                1 => name = m.u32(rec + 0x14),
                0 => name = m.u32(rec + 0x10),
                2 => {
                    let a = m.u32(rec + 0x10);
                    let b = m.u32(rec + 0x14);
                    o.db_format_prefix_name(m, WORK_STR2, a, b);
                    name = WORK_STR2;
                }
                _ => {}
            }
            if name != 0 {
                let miss = if p4 == 0 {
                    if p5 == 0 { clib::strstr(m, name, text) == 0 } else { clib::strcasecmp(m, text, name) != 0 }
                } else {
                    // (p5 != 0: the original calls strcmp here and drops the result)
                    clib::strstr(m, name, text) == 0
                };
                if !miss {
                    res = 1;
                    if mark != 0 {
                        m.set_u16(nd, 1);
                    }
                    if all == 0 {
                        return res;
                    }
                }
            }
        }
        i += 1;
    }
    res
}
