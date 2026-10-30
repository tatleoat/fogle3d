// SPDX-License-Identifier: MIT
//! `dbmanager.c`: the database manager's element lists (the dialogs that move, delete and
//! list scene files and elements). Finding 0007, 2026-10-28 section.
//!
//! The text lists are 0x14-byte nodes (selection short at +0, element index word at +8,
//! flag word at +0x10) in a list of count (short at +0) and node pointer (+4); the elements
//! are 0x84-byte records in an element list (+8).

use crate::calls::DbCalls;
use crate::imglobals::{cid, DB_ERR_FLAG, DB_WORK_FNAME, SI_CID, STDIN, DBM_WORK_STR};
use si_mem::{Mem, Outside};

const NODE: u32 = 0x14;
const ELEM: u32 = 0x84;

/// A `%s` argument as the C library prints it: the text, or `(null)` for a null pointer.
fn arg(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 { b"(null)".to_vec() } else { m.cstr(p) }
}

fn put(m: &mut Mem, dst: u32, parts: &[&[u8]]) {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(p);
    }
    m.put_cstr(dst, &v);
}

/// [soft 0x080de08c si_dbMElemBuildName]: the full path of an element into `out1` and its file
/// name (with suffix) into `out2`, by the database kind (info +0x24), the list's kind (+0x18)
/// and the element's own suffix (+0x80).
#[allow(clippy::too_many_arguments)]
pub fn elem_build_name(m: &mut Mem, o: &mut dyn Outside, info: u32, list: u32, chap: i32, elem: u32, out1: u32, out2: u32) {
    let node = m.u32(elem);
    let dir = arg(m, m.u32(info + 0x14));
    let name = arg(m, m.u32(node + 8));
    let leaf = arg(m, m.u32(node + 0x24));
    if m.i32(info + 0x24) == 2 {
        put(m, out1, &[&dir, &name, &leaf]);
        let sfx = o.get_custom_suffix(m, chap);
        let sfx = arg(m, sfx);
        let work = m.cstr(DB_WORK_FNAME.0);
        put(m, out2, &[&work, b".", &sfx]);
    } else if m.i16(list + 0x18) == 2 {
        let base = arg(m, m.u32(list + 0x28));
        put(m, out1, &[&base, b"/", &leaf]);
        let s = m.cstr(out1);
        put(m, out2, &[&s]);
    } else {
        let mut sfx = m.u32(elem + 0x80);
        if sfx == 0 {
            sfx = o.ed_get_suffix_from_chapter(m, chap);
        }
        put(m, out1, &[&dir, &name, &leaf]);
        let s = m.cstr(out1);
        let sfx = arg(m, sfx);
        put(m, out2, &[&s, b".", &sfx]);
    }
}

/// [soft 0x080de20c dbMtextlistResetFlags]: clears the selection, the second short and the flag
/// word of every node.
pub fn textlist_reset_flags(m: &mut Mem, tl: u32) {
    let n = m.i16(tl) as i32;
    let nodes = m.u32(tl + 4);
    for i in 0..n {
        let p = nodes.wrapping_add((i as u32).wrapping_mul(NODE));
        m.set_u32(p + 0x10, 0);
        m.set_u16(p, 0);
        m.set_u16(p + 2, 0);
    }
}

/// [soft 0x080de24c dbMtextlistDelValidate]: counts the selected nodes (`*selected`) and those of
/// them whose element's name record has a non-negative mode byte (+0x2c) (`*deletable`);
/// with `clear` set it also unselects the latter.
pub fn textlist_del_validate(m: &mut Mem, tl: u32, list: u32, selected: u32, deletable: u32, clear: i16) {
    m.set_u32(deletable, 0);
    m.set_u32(selected, 0);
    let n = m.i16(tl) as i32;
    let nodes = m.u32(tl + 4);
    for i in 0..n {
        let node = nodes.wrapping_add((i as u32).wrapping_mul(NODE));
        let idx = m.i32(node + 8);
        if idx < 0 {
            continue;
        }
        let e = m.u32(list + 8).wrapping_add((idx as u32).wrapping_mul(ELEM));
        let sel = m.i16(node);
        if sel != 0 {
            let c = m.u32(selected);
            m.set_u32(selected, c.wrapping_add(1));
        }
        let rec = m.u32(e);
        if e != 0 && rec != 0 && sel != 0 && (m.u8(rec + 0x2c) as i8) >= 0 {
            if clear != 0 {
                m.set_u16(node, 0);
            }
            let c = m.u32(deletable);
            m.set_u32(deletable, c.wrapping_add(1));
        }
    }
}

/// [soft 0x080de2e8 dbMtextlistFlagSelected]: copies each node's selection into its element's
/// work flag (+0x58) and selected short (+0x42).
pub fn textlist_flag_selected(m: &mut Mem, tl: u32, list: u32) {
    let n = m.i16(tl) as i32;
    let nodes = m.u32(tl + 4);
    for i in 0..n {
        let node = nodes.wrapping_add((i as u32).wrapping_mul(NODE));
        let idx = m.i32(node + 8);
        if idx >= 0 {
            let e = m.u32(list + 8).wrapping_add((idx as u32).wrapping_mul(ELEM));
            let sel = m.i16(node);
            m.set_u32(e + 0x58, sel as i32 as u32);
            m.set_u16(e + 0x42, sel as u16);
        }
    }
}

/// [soft 0x080de340 dbMtextlistUnSelFlagged]: counts the nodes with an element (`*with_elem`), those
/// of them selected with a name record (`*selected`), and unselects the selected ones whose
/// element's work flag (+0x58) is clear (`*unselected`).
pub fn textlist_unsel_flagged(m: &mut Mem, tl: u32, list: u32, with_elem: u32, selected: u32, unselected: u32) {
    m.set_u32(with_elem, 0);
    m.set_u32(unselected, 0);
    m.set_u32(selected, 0);
    let n = m.i16(tl) as i32;
    let nodes = m.u32(tl + 4);
    for i in 0..n {
        let node = nodes.wrapping_add((i as u32).wrapping_mul(NODE));
        let idx = m.i32(node + 8);
        if idx < 0 {
            continue;
        }
        let c = m.u32(with_elem);
        m.set_u32(with_elem, c.wrapping_add(1));
        let e = m.u32(list + 8).wrapping_add((idx as u32).wrapping_mul(ELEM));
        if e != 0 && m.u32(e) != 0 && m.i16(node) != 0 {
            let c = m.u32(selected);
            m.set_u32(selected, c.wrapping_add(1));
            if m.u32(e + 0x58) == 0 {
                m.set_u16(node, 0);
                let c = m.u32(unselected);
                m.set_u32(unselected, c.wrapping_add(1));
            }
        }
    }
}

/// [soft 0x080e1268 si_dbGetError]: the browser's last error code.
pub fn get_error(m: &Mem) -> i32 {
    m.i32(DB_ERR_FLAG.0)
}

/// [soft 0x080e1274 si_dbSetError]: records an error code.
pub fn set_error(m: &mut Mem, code: i32) {
    m.set_u32(DB_ERR_FLAG.0, code as u32);
}

/// The string literals of the functions below that are handed on as pointers (a check's image
/// holds none of the binary's data).
pub mod strings {
    pub const HRC_CONVERT: u32 = 0x0867fae8;
    pub const HRC_CONVERT_OUT: u32 = 0x0867faf3;
    pub const BIN2ASCII: u32 = 0x0867fb03;
    pub const BIN2ASCII_OUT: u32 = 0x0867fb0d;
    pub const EMPTY: u32 = 0x0867f83c;
    pub const WHO_USES: u32 = 0x0867f83d;
    pub const DELETE: u32 = 0x0867fae1;
    pub const ALL: &[(u32, &str)] = &[
        (HRC_CONVERT, "hrcConvert"),
        (HRC_CONVERT_OUT, "hrcConvert"),
        (BIN2ASCII, "bin2ascii"),
        (BIN2ASCII_OUT, "bin2ascii"),
        (EMPTY, ""),
        (WHO_USES, "Who Uses"),
        (DELETE, "Delete"),
    ];
}

/// Writes the string literals of this file into the image.
pub fn store_strings(m: &mut Mem) {
    for (a, s) in strings::ALL {
        m.put_cstr(*a, s.as_bytes());
    }
}

/// The size of the element list record the callers keep on their stack.
const ELEMLIST: u32 = 0x40;

/// [soft 0x080dd730 SI_dbMGetInputLine]: reads standard input up to a newline into `buf` (the
/// newline is dropped and the string ended). Only the low byte of each character is compared
/// with the newline, so an end of file (-1) never ends the loop; the original has no
/// bound on `buf` either.
pub fn get_input_line(m: &mut Mem, o: &mut dyn Outside, buf: u32) {
    let stream = m.u32(STDIN.0);
    let mut c = o.io_getc(m, stream) as u8;
    let mut i = 0u32;
    while c != 0x0a {
        m.set_u8(buf.wrapping_add(i), c);
        i = i.wrapping_add(1);
        let stream = m.u32(STDIN.0);
        c = o.io_getc(m, stream) as u8;
    }
    m.set_u8(buf.wrapping_add(i), 0);
}

/// [soft 0x080ddfd4 SI_dbMListElems]: when the refresh flag is set, formats the heading by the
/// kind (0xe: still referred to; 0x47: unused; 0x4c: scenes that refer to it; others leave
/// the work string as it was) into `workStr`, lets the list be filled and shows it in a
/// scrolling message box, then disposes of the list. `name` is the `%s` of the heading.
#[allow(clippy::too_many_arguments)]
pub fn list_elems(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, kind: i32, _unused: i32, name: u32, title: u32) {
    if (o.cus_dso_get_refresh_flag(m) as u8) & 1 == 0 {
        return;
    }
    let text: Option<&[u8]> = match kind {
        0x0e => Some(b"Not done! These scenes still refer to the selected "),
        0x47 => Some(b"The following "),
        0x4c => Some(b"The following scenes refer to the selected "),
        _ => None,
    };
    if let Some(head) = text {
        let n = arg(m, name);
        let mut v = head.to_vec();
        v.extend_from_slice(&n);
        if kind == 0x47 {
            v.extend_from_slice(b" are unused");
        }
        m.put_cstr(DBM_WORK_STR.0, &v);
    }
    let sp = m.sp();
    let list = m.local(4);
    m.set_u32(list, 0);
    o.db_list_prepare_elements_list(m, a, b, -1, 0, list, 1);
    let l = m.u32(list);
    o.scrolling_msg_box_id(m, l, 0x2714, DBM_WORK_STR.0, title);
    if m.u32(list) != 0 {
        o.text_list_dispose(m, list);
    }
    m.set_sp(sp);
}

/// [soft 0x080de178 SI_dbMElemHeadersGet]: makes chapter `idx` of the database record current,
/// builds its path, and lists its elements and sub-directories (the suffix is the custom
/// chapter's for a shader database, else the chapter record's own at +0x3c, else the
/// chapter table's).
pub fn elem_headers_get(m: &mut Mem, o: &mut dyn Outside, info: u32, list: u32, idx: i32, flag: i16) {
    let rec = (idx as u32).wrapping_shl(6).wrapping_add(m.u32(list + 4));
    let first = m.u32(rec);
    o.db_set_chapter(m, info, first);
    o.db_filepath_build(m, info);
    let sfx = if m.i32(info + 0x24) == 2 {
        o.get_custom_suffix(m, idx)
    } else {
        let own = m.u32(rec + 0x3c);
        if own == 0 { o.ed_get_suffix_from_chapter(m, idx) } else { own }
    };
    o.db_chapter_list_elements(m, info, list, sfx, idx, 1, flag as i32);
    o.db_chapter_get_sub_dirs(m, info, rec, flag as i32);
}

/// [soft 0x080de3d8 SI_dbMFlagUnused]: marks every element of the list (work flag +0x58 and
/// selected short +0x42 set), lets the scene check clear the marks of those that a scene
/// refers to, and returns the check's answer.
pub fn flag_unused(m: &mut Mem, o: &mut dyn Outside, info: u32, list: u32, flag: i16) -> i32 {
    let sp = m.sp();
    let l = m.local(ELEMLIST);
    o.elemlist_init(m, l);
    let n = m.i32(list + 4);
    let mut i = 0;
    while i < n {
        let e = m.u32(list + 8).wrapping_add((i as u32).wrapping_mul(0x84));
        m.set_u32(e + 0x58, 1);
        m.set_u16(e + 0x42, 1);
        i += 1;
    }
    let r = o.dbfile_check_elements(m, info, list, l, flag as i32);
    o.elemlist_dispose(m, l, 0);
    m.set_sp(sp);
    r
}

/// [soft 0x080de450 SI_dbMBuildTextlistFromName]: a text list of one node (selection 1, no
/// element) holding a copy of `name`; null for a null name. (The original counts both
/// allocations in a licence counter, not touched here.)
pub fn build_textlist_from_name(m: &mut Mem, name: u32) -> u32 {
    if name == 0 {
        return 0;
    }
    let tl = m.si_malloc(8);
    m.set_u32(tl + 4, 0);
    m.set_u16(tl, 1);
    let count = m.i16(tl) as i32 as u32;
    let node = m.si_calloc(count, NODE);
    m.set_u32(tl + 4, node);
    let copy = m.strdup(name);
    m.set_u32(node + 4, copy);
    m.set_u16(node, 1);
    m.set_u16(node + 2, 0);
    m.set_u32(node + 8, 0);
    tl
}

/// [soft 0x080de4cc dbMgetFileInfo]: `stat`s `dir` followed by `name`; when that fails and
/// `use_suffix` is non-zero, again with `.suffix` of the chapter added. 1 when a stat worked.
pub fn get_file_info(m: &mut Mem, o: &mut dyn Outside, dir: u32, name: u32, use_suffix: i16, chap: i32, st: u32) -> i32 {
    let (d, n) = (arg(m, dir), arg(m, name));
    put(m, DBM_WORK_STR.0, &[&d, &n]);
    let mut ok = (o.xstat(m, 3, DBM_WORK_STR.0, st) == 0) as i16;
    if use_suffix != 0 && ok == 0 {
        let sfx = o.ed_get_suffix_from_chapter(m, chap);
        let (w, sfx) = (m.cstr(DBM_WORK_STR.0), arg(m, sfx));
        put(m, DBM_WORK_STR.0, &[&w, b".", &sfx]);
        ok = (o.xstat(m, 3, DBM_WORK_STR.0, st) == 0) as i16;
    }
    ok as i32
}

/// [soft 0x080de5f4 SI_dbMBuildElemDictFromName]: empties the element list `out` and looks
/// up the file `name` in the database's directory (info +0x14). When the lookup worked (or
/// `use_suffix` is zero, whatever the lookup left in the stat buffer) and the mode is not a
/// directory's, adds a node for it: a list node by the name, modification time at +0x20,
/// mode at +0x2c, an empty second name at +8, and the element's short at +0x40 set. Returns
/// the file-info answer.
#[allow(clippy::too_many_arguments)]
pub fn build_elem_dict_from_name(m: &mut Mem, o: &mut dyn Outside, name: u32, info: u32, _unused: u32, out: u32, chap: i32, use_suffix: i16) -> i32 {
    let sp = m.sp();
    let st = m.local(0x58);
    o.elemlist_dispose(m, out, 0);
    let dir = m.u32(info + 0x14);
    let r = get_file_info(m, o, dir, name, use_suffix, chap, st) as i16;
    if r != 0 || use_suffix == 0 {
        let mode = m.u32(st + 0x10);
        if mode & 0xf000 != 0x4000 {
            let node = o.db_scn_get_list_node(m, name, 0, 1);
            m.set_u32(node + 0x20, m.u32(st + 0x40));
            m.set_u16(node + 0x2c, mode as u16);
            let copy = m.strdup(strings::EMPTY);
            m.set_u32(node + 8, copy);
            let idx = o.elemlist_add_elem(m, out, node, 0, 0, 0, 0, 0, 0);
            let e = o.elemlist_get_elem(m, out, idx);
            m.set_u16(e + 0x40, 1);
        }
    }
    m.set_sp(sp);
    r as i32
}

/// [soft 0x080de6b8 SI_dbMWhoUses]: takes the first selected node of the text list that has an
/// element, asks the scene check which scenes use it and, if any, lists them. 2 when no
/// node qualifies, 1 when scenes were listed, else 0x4b.
pub fn who_uses(m: &mut Mem, o: &mut dyn Outside, info: u32, dialog: u32, list: u32, tl: u32, flag: i16) -> i32 {
    let sp = m.sp();
    let l = m.local(ELEMLIST);
    o.elemlist_init(m, l);
    let n = m.i16(tl) as i32;
    let mut elem = 0u32;
    let mut i = 0;
    while i < n {
        let node = m.u32(tl + 4).wrapping_add((i as u32).wrapping_mul(NODE));
        if m.i16(node) != 0 {
            let idx = m.i32(node + 8);
            if idx >= 0 {
                elem = m.u32(list + 8).wrapping_add((idx as u32).wrapping_mul(ELEM));
                break;
            }
        }
        i += 1;
    }
    if elem == 0 {
        // (the original returns here without disposing of the list Init made)
        m.set_sp(sp);
        return 2;
    }
    let name = m.u32(m.u32(elem) + 0x24);
    let found = o.dbfile_check_element(m, info, list, l, name, 1, 1);
    let mut r = 0x4b;
    if found > 0 {
        let list_name = m.u32(list);
        list_elems(m, o, dialog, l, 0x4c, flag as i32, list_name, strings::WHO_USES);
        r = 1;
    }
    o.elemlist_dispose(m, l, 0);
    m.set_sp(sp);
    r
}

/// [soft 0x080e1080 SI_dbMFileBinary2Ascii]: converts the file `file` (a model, `.hrc` cut off
/// the name first, or another binary file) with an external command (`hrcConvert ... -a`
/// or `bin2ascii ... -k`) into `/usr/tmp/SI_tmp.<pid>`. Text chapters (scenes, the three
/// setups, waves) are not converted: 0. Otherwise `file` becomes the temporary name (with
/// `.hrc` appended for a model, whose command's answer is also recorded with
/// `si_dbSetError`), `out1` receives it, `out2` (when given) the temporary name without the
/// suffix, `*cmd_out` (when given) the name of the command; 1.
pub fn file_binary2ascii(m: &mut Mem, o: &mut dyn Outside, chap: i32, file: u32, out1: u32, out2: u32, cmd_out: u32) -> i32 {
    let cid_at = |m: &Mem, off: u32| m.i32(SI_CID.0 + off);
    let models = chap == cid_at(m, cid::MODEL);
    let cmd;
    if models {
        cmd = strings::HRC_CONVERT;
        if cmd_out != 0 {
            m.set_u32(cmd_out, strings::HRC_CONVERT_OUT);
        }
        // the original cuts a trailing ".hrc" (reading four bytes before the end whatever the length)
        let len = m.strlen(file);
        let end = file.wrapping_add(len);
        if m.bytes(end.wrapping_sub(4), 4) == b".hrc" {
            m.set_u8(end.wrapping_sub(4), 0);
        }
    } else {
        let texts = [cid::DSC, cid::STS, cid::STC, cid::SFK, cid::WAV];
        if texts.iter().any(|&t| chap == cid_at(m, t)) {
            return 0;
        }
        cmd = strings::BIN2ASCII;
        if cmd_out != 0 {
            m.set_u32(cmd_out, strings::BIN2ASCII_OUT);
        }
    }
    let pid = o.getpid(m);
    let tmp = format!("/usr/tmp/SI_tmp.{}", pid).into_bytes();
    let f = arg(m, file);
    put(m, DBM_WORK_STR.0, &[&f, b" ", &tmp]);
    let w = m.cstr(DBM_WORK_STR.0);
    put(m, DBM_WORK_STR.0, &[&w, if models { b" -a" } else { b" -k" }]);
    let r = o.db_system_command(m, cmd, DBM_WORK_STR.0);
    if models {
        set_error(m, r);
    }
    put(m, file, &[&tmp]);
    if models {
        let f = m.cstr(file);
        put(m, file, &[&f, b".hrc"]);
    }
    let f = m.cstr(file);
    put(m, out1, &[&f]);
    if out2 != 0 {
        put(m, out2, &[&tmp]);
    }
    1
}

// ---- the message routines: a code selects the texts of a dialog (`dbmessages.rs` holds the
// tables, generated from the program's switch tables)

use crate::dbmessages::{message_choice, request2_choice, request_choice, Choice, Plan, Val};

const EMPTY: u32 = 0x0867_de07;

/// Formats the plan's `sprintf` into `workStr` and resolves its values. `args` are the stack
/// arguments from offset 8 on.
fn run_plan(m: &mut Mem, plan: &Plan, args: &[i32]) -> Vec<u32> {
    let at = |v: &Val| -> u32 {
        match *v {
            Val::Imm(a) => a,
            Val::Arg(off) => args.get(((off - 8) / 4) as usize).copied().unwrap_or(0) as u32,
            Val::Undef => 0,
        }
    };
    if let Some((fmt, vals)) = plan.printf {
        // (the formats use only %d and %s)
        let mut out = Vec::new();
        let mut k = 0;
        let f = fmt.as_bytes();
        let mut i = 0;
        while i < f.len() {
            if f[i] == b'%' && i + 1 < f.len() {
                let v = at(&vals[k]);
                k += 1;
                if f[i + 1] == b'd' {
                    out.extend_from_slice((v as i32).to_string().as_bytes());
                } else {
                    out.extend_from_slice(&arg(m, v));
                }
                i += 2;
            } else {
                out.push(f[i]);
                i += 1;
            }
        }
        m.put_cstr(DBM_WORK_STR.0, &out);
    }
    plan.regs.iter().map(at).collect()
}

fn pick(c: Choice, args: &[i32]) -> (&'static Plan, bool) {
    match c {
        Choice::One(p) => (p, false),
        Choice::IfArgGt1(a, b) => (if args.first().copied().unwrap_or(0) > 1 { a } else { b }, true),
    }
}

/// [soft 0x080dd778 si_dbMRequest]: the one-button request for a message code (`n1`, `n2`
/// are numbers some texts print, `path` a name some print); returns the dialog's answer.
pub fn db_request(m: &mut Mem, o: &mut dyn Outside, code: i32, n1: i32, n2: i32, path: u32) -> i32 {
    let args = [code, n1, n2, path as i32];
    let (l1, l2, l3) = match request_choice(code) {
        Some(c) => {
            let (p, _) = pick(c, &args[1..]);
            let r = run_plan(m, p, &args);
            (r[0], r[1], r[2])
        }
        None => (EMPTY, EMPTY, EMPTY),
    };
    o.dialog_request(m, l1, l2, l3, 0x0867_de04)
}

/// [soft 0x080dd938 SI_dbMRequest2]: the two- or three-button request for a message code;
/// the answer is the dialog's low 16 bits, sign-extended. (Codes outside 20..=155 leave the
/// lines undefined in the original; here they are empty.)
pub fn db_request2(m: &mut Mem, o: &mut dyn Outside, code: i32, n1: i32, n2: i32, path: u32) -> i32 {
    let args = [code, n1, n2, path as i32];
    let (r, local) = match request2_choice(code) {
        Some(c) => {
            let (p, _) = pick(c, &args[1..]);
            let r = run_plan(m, p, &args);
            let local = match p.local {
                Val::Imm(a) => a,
                Val::Arg(off) => args[((off - 8) / 4) as usize] as u32,
                Val::Undef => 0,
            };
            (r, local)
        }
        None => (vec![0, 0, 0, 0x0867_e4ad, 0x0867_e4b4], 0),
    };
    let a = if local == 0 {
        o.dialog_request2(m, r[0], r[1], r[2], r[3], r[4])
    } else {
        o.dialog_request3(m, r[0], r[1], r[2], r[3], r[4], local)
    };
    a as i16 as i32
}

/// [soft 0x080ddc44 SI_dbMMessage]: shows the text of a message code, as the title of item
/// `id` of the dialog `dlg`, or in the status line when there is no dialog. Code 1 shows
/// nothing.
pub fn db_message(m: &mut Mem, o: &mut dyn Outside, code: i32, id: i16, dlg: u32, a: u32, b: u32) {
    if code == 1 {
        return;
    }
    let args = [code, id as i32, dlg as i32, a as i32, b as i32];
    let text = match message_choice(code) {
        Some(c) => {
            let (p, _) = pick(c, &args[1..]);
            run_plan(m, p, &args)[0]
        }
        None => 0x0867_f762,
    };
    if dlg != 0 {
        o.set_item_title(m, dlg, id as i32, text);
    } else {
        o.status_display_message(m, text, 1);
    }
}

/// `SI_dbtStandalone` (a short): set when the program runs without its dialogs.
const DBT_STANDALONE: u32 = 0x0868_0e4c;
/// Three words of `SI_cid` the file operations test the chapter against (pictures, scenes, render pictures).
const CID_PICTURES: u32 = 0x0878_1670;
const CID_SCENES: u32 = 0x0878_166c;
const CID_RENDER_PICTURES: u32 = 0x0878_168c;

/// [soft 0x080e0d54 SI_dbMFilesDelete]: deletes the selected elements of a chapter. Returns a
/// message code: 0xb database not writable, 0xc external directory not writable, 2 nothing selected,
/// 0xd none of the selected may be deleted, 0xf cancelled or still referred to, 0x11/0x13/0x1b
/// done (pictures, other chapters, external directory), 0x10 a delete failed. Scene chapters go
/// to `SI_dbMSceneFilesMoveDel` (0x080de788, not ported: called through [`DbCalls`]).
pub fn files_delete(m: &mut Mem, o: &mut dyn Outside, info: u32, dict: u32, list: u32, chap: i32, tl: u32, flag: i16) -> i32 {
    let ext = m.i16(list + 0x18) == 2;
    let pic = chap == m.i32(CID_PICTURES) || chap == m.i32(CID_RENDER_PICTURES);
    let mut strip = false;
    if ext {
        let path = m.u32(list + 0x28);
        if o.access(m, path, 2) != 0 {
            return 0xc;
        }
    } else {
        if pic {
            strip = m.i16(dict + 0x2c) != 0;
        }
        if o.db_database_check_access(m, info, 2) as i16 == 0 {
            return 0xb;
        }
        if chap == m.i32(CID_SCENES) {
            let r = o.db_m_scene_files_move_del(m, info, 0, dict, 0, list, 0, tl, 0, 1, 0, flag as i32, 0, 0, 0, 0, 0);
            return r as i16 as i32;
        }
    }
    let sp = m.sp();
    let l = m.local(ELEMLIST);
    o.elemlist_init(m, l);
    let vars = m.local(0x10);
    // (the original keeps these four counts in its frame: selected, deletable, and the two of UnSelFlagged)
    let (sel, del, with, unsel) = (vars, vars + 4, vars + 8, vars + 12);
    textlist_del_validate(m, tl, list, sel, del, 1);
    let nsel = m.i32(sel);
    if nsel == 0 {
        m.set_sp(sp);
        return 2;
    }
    if m.i32(del) == nsel {
        m.set_sp(sp);
        return 0xd;
    }
    let standalone = m.i16(DBT_STANDALONE) != 0;
    let mut proceed = true;
    if !ext {
        if !pic {
            textlist_flag_selected(m, tl, list);
            o.dbfile_check_elements(m, info, list, l, 1);
            textlist_unsel_flagged(m, tl, list, with, sel, unsel);
            if m.i32(unsel) >= m.i32(sel) {
                let name = m.u32(list);
                list_elems(m, o, dict, l, 0x0e, 0, name, strings::DELETE);
                o.elemlist_dispose(m, l, 0);
                m.set_sp(sp);
                return 0xf;
            }
        }
        if pic {
            if !standalone {
                proceed = db_request2(m, o, 0x14, 0, 0, 0) as i16 != 0;
            }
        } else if !standalone {
            proceed = db_request2(m, o, 0x15, 0, 0, 0) == 0;
        }
    } else if !standalone {
        proceed = db_request2(m, o, 0x1a, 0, 0, 0) as i16 != 0;
    }
    let mut code = if ext { 0x1b } else if pic { 0x11 } else { 0x13 };
    if !proceed {
        code = 0xf;
    } else {
        let mut count = m.i16(tl) as i32;
        let mut i = 0;
        let mut at = 0u32;
        while i < count {
            let nd = m.u32(tl + 4).wrapping_add(at);
            let idx = m.i32(nd + 8);
            if m.i16(nd) != 0 && idx >= 0 {
                let e = m.u32(list + 8).wrapping_add((idx as u32).wrapping_mul(ELEM));
                let mut st = 0i32;
                if strip {
                    let leaf = m.u32(m.u32(e) + 0x24);
                    st = o.str_check_and_strip_digit_only(m, leaf) as i16 as i32;
                }
                let r = o.db_file_delete(m, info, list, e, 1, st);
                if r == -1 {
                    code = 0xd;
                } else if r == 0 {
                    code = 0x10;
                }
                count = m.i16(tl) as i32;
            }
            at = at.wrapping_add(NODE);
            i += 1;
        }
    }
    o.elemlist_dispose(m, l, 0);
    m.set_sp(sp);
    code
}
