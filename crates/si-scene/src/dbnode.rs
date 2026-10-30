// SPDX-License-Identifier: MIT
//! Name records (`SI_DbListNode`, 0x34 bytes) on the image: the routines of
//! `dbexternal.c` that the dictionaries call to make, free, clean and format one, and the
//! table look-ups by chapter.
//!
//! `si-db` has the same routines on values (it is the database layer, a library of values
//! that stays as it is); here they are on the record in the image, each string a block of
//! its own, with the original's allocation calls. `si-dict` reaches them through its
//! `Outside` trait; [`crate::world`] hands those calls to the functions of this file.
//!
//! Each of the ten is held against the emulated original by `scene-emucheck` on the
//! vectors of `re/tools/si-scene/emu_gen_node.py` (14,760 cases: finding 0018, section 14).
//!
//! The conversions at the end ([`to_value`], [`set_string`], [`from_value`]) are the seam
//! to `si-db`: its file search and directory scans take and return `si_db::ListNode`
//! values.

use crate::globals::{LOADING, S_NODE_EMPTY, S_NODE_ONE, S_NODE_ZERO, S_NO_SUFFIX, WORK_STR};
use si_dict::globals::{SI_CHAP_INFO, SI_CID};
use si_dict::Outside;
use si_mem::{CStr32, Mem, Ptr32};
use si_types::dict::{ChapInfo, DbListNode, ElemDict};

/// A new name record [soft 0x0812250c SI_dbListNodeAllocate]. Sub-directory an empty
/// string, version `1`, substitution `0` (three copies of literals), the time what
/// `time(NULL)` answers, everything else null or zero. The two bytes at +0x2e and the
/// word at +0x30 are not written.
pub fn list_node_allocate(m: &mut Mem, o: &mut dyn Outside) -> Ptr32<DbListNode> {
    let node: Ptr32<DbListNode> = Ptr32::new(m.si_malloc(0x34));
    m[node].unk_00 = 0;
    m[node].unk_04 = Ptr32::NULL;
    m[node].str_08 = Ptr32::new(m.strdup(S_NODE_EMPTY));
    m[node].str_0c = Ptr32::NULL;
    m[node].prefix = Ptr32::NULL;
    m[node].name = Ptr32::NULL;
    m[node].version = Ptr32::new(m.strdup(S_NODE_ONE));
    m[node].subst = Ptr32::new(m.strdup(S_NODE_ZERO));
    m[node].time = si_dict::outside::time(o, m);
    m[node].mode = 0;
    m[node].fullname = Ptr32::NULL;
    m[node].unk_28 = 0;
    m[node].unk_2a = 0;
    node
}

/// Frees a name record [soft 0x08122450 SI_dbListNodeFree]: the eight strings that are not
/// null, in the order +0x0c, +0x04, +0x08, +0x10, +0x14, +0x18, +0x1c, +0x24, then the
/// record. Nothing for a null record.
pub fn list_node_free(m: &mut Mem, node: Ptr32<DbListNode>) {
    if node.is_null() {
        return;
    }
    for off in [0x0c, 0x04, 0x08, 0x10, 0x14, 0x18, 0x1c, 0x24] {
        let s = m.u32(node.addr() + off);
        if s != 0 {
            m.free(s);
        }
    }
    m.free(node.addr());
}

/// Makes a prefix or an element name fit for a file name, in place, and answers it
/// [soft 0x08121bdc SI_dbnameCheckValidity]; an empty string is freed and null answered;
/// null stays null.
///
/// A string of 100 characters or more gets a terminator at position 99, and the loop
/// still walks the old length: the characters behind the cut are rewritten like the
/// others. A blank or a slash becomes an underscore. The punctuation characters become
/// underscores unless a scene is being loaded ([`LOADING`]). A dash counts as punctuation
/// in a prefix, and in a name while the name's first character is a dash (read at that
/// moment: once a leading dash has become an underscore, later dashes stay). A dot counts
/// as a dash in a prefix when it is the last character.
pub fn name_check_validity(m: &mut Mem, name: CStr32, is_prefix: i16) -> CStr32 {
    if name.is_null() {
        return Ptr32::NULL;
    }
    let len = m.strlen(name.addr()) as i32;
    if len > 99 {
        m.set_u8(name.addr() + 99, 0);
    }
    if len == 0 {
        m.free(name.addr());
        return Ptr32::NULL;
    }
    for i in 0..len {
        let a = name.addr() + i as u32;
        let c = m.u8(a);
        let dash = |m: &Mem| is_prefix != 0 || m.u8(name.addr()) == b'-';
        let special = match c {
            b' ' | b'/' => {
                m.set_u8(a, b'_');
                false
            }
            b'!'..=b',' | b':'..=b'@' | b'['..=b'^' | b'`' | b'{'..=b'~' => true,
            b'-' => dash(m),
            b'.' => is_prefix != 0 && len - 1 <= i && dash(m),
            _ => false,
        };
        if special && m[LOADING] == 0 {
            m.set_u8(a, b'_');
        }
    }
    name
}

/// A string argument of `sprintf`'s `%s`: the text, or `(null)` for a null pointer, as
/// the C library prints one.
fn percent_s(m: &Mem, p: CStr32) -> Vec<u8> {
    if p.is_null() {
        si_db::NULL_TEXT.to_vec()
    } else {
        m.cstr(p.addr())
    }
}

/// Prints `prefix-name` into the caller's buffer, or the name alone when the prefix is
/// null or empty [soft 0x08121ca8 SI_dbFormatPrefixName]. The buffer's length is not
/// looked at.
pub fn format_prefix_name(m: &mut Mem, out: CStr32, prefix: CStr32, name: CStr32) {
    let mut text = Vec::new();
    if !prefix.is_null() && m.strlen(prefix.addr()) != 0 {
        text = m.cstr(prefix.addr());
        text.push(b'-');
    }
    text.extend_from_slice(&percent_s(m, name));
    m.put_cstr(out.addr(), &text);
}

/// Cleans the record's prefix and name with [`name_check_validity`], stores what comes
/// back (null for a string that was empty) and prints the two joined into the caller's
/// buffer [soft 0x08121d38 SI_dbfilenameSetPrefix].
pub fn filename_set_prefix(m: &mut Mem, out: CStr32, node: Ptr32<DbListNode>) {
    let p = m[node].prefix;
    m[node].prefix = name_check_validity(m, p, 1);
    let n = m[node].name;
    let n = name_check_validity(m, n, 0);
    m[node].name = n;
    let p = m[node].prefix;
    format_prefix_name(m, out, p, n);
}

/// The record's file name without suffix, `prefix-name.version-subst`, in a new block
/// [soft 0x081223dc SI_dbFormatFullName]. The record's stored full name is freed and
/// cleared first, prefix and name are cleaned ([`filename_set_prefix`], into a local
/// buffer of 0x400 bytes); storing the answer in the record is the caller's business. The
/// name is printed into the source file's work buffer (0x400 bytes, not checked) and
/// copied from there.
pub fn format_full_name(m: &mut Mem, node: Ptr32<DbListNode>) -> CStr32 {
    let old = m[node].fullname;
    if !old.is_null() {
        m.free(old.addr());
    }
    m[node].fullname = Ptr32::NULL;
    let local: CStr32 = Ptr32::new(m.local(0x400));
    filename_set_prefix(m, local, node);
    let mut text = m.cstr(local.addr());
    m.release(0x400);
    text.push(b'.');
    text.extend_from_slice(&percent_s(m, m[node].version));
    text.push(b'-');
    text.extend_from_slice(&percent_s(m, m[node].subst));
    // Where Rust must differ: a name longer than the buffers overruns them in the original
    // (the local one on the stack first); here it is cut to the work buffer (no name of a
    // database comes near 1023 characters).
    text.truncate(0x3ff);
    m.put_cstr(WORK_STR.addr(), &text);
    Ptr32::new(m.strdup(WORK_STR.addr()))
}

/// Splits a file name into prefix, element name, version, substitution and suffix,
/// written into five buffers of the caller (none of them checked for length)
/// [soft 0x081228c0 dbfilenameExpand]. Nothing at all for a null name.
///
/// The name is copied (`strdup`) and the copy cut up: the text after the last dot is
/// tried as a version (`<int>-<int>`, exactly as `%d-%d` prints it:
/// `SI_dbCheckVersionString (0x0812283c)`, which `si-db` has on values); if it is none,
/// the text after the dot before it is tried, and what was set aside is then the suffix;
/// without a version the answer is version `1`, substitution `0`, no suffix, the name cut
/// at the second dot from the end if there is one. What is left is split at its first
/// dash, unless that dash is the first character or follows a dot. The copy is freed.
pub fn filename_expand(m: &mut Mem, name: CStr32, parts: [u32; 5]) {
    if name.is_null() {
        return;
    }
    let [proj, nam, vers, prec, suff] = parts;
    let copy = m.strdup(name.addr());
    let mut s = m.cstr(copy);
    let number = |m: &mut Mem, at: u32, v: i32| m.put_cstr(at, v.to_string().as_bytes());
    match s.iter().rposition(|&c| c == b'.') {
        None => {
            number(m, vers, 1);
            number(m, prec, 0);
            m.set_u8(suff, 0);
        }
        Some(dot) => match si_db::check_version_string(&s[dot + 1..]) {
            None => {
                let tail = s[dot + 1..].to_vec();
                s.truncate(dot);
                m.set_u8(copy + dot as u32, 0);
                let q = s.iter().rposition(|&c| c == b'.');
                match q.and_then(|q| si_db::check_version_string(&s[q + 1..]).map(|vr| (q, vr))) {
                    Some((q, (v, r))) => {
                        number(m, vers, v);
                        number(m, prec, r);
                        s.truncate(q);
                        m.set_u8(copy + q as u32, 0);
                        m.put_cstr(suff, &tail);
                    }
                    None => {
                        number(m, vers, 1);
                        number(m, prec, 0);
                        m.set_u8(suff, 0);
                        if let Some(q) = q {
                            s.truncate(q);
                            m.set_u8(copy + q as u32, 0);
                        }
                    }
                }
            }
            Some((v, r)) => {
                number(m, vers, v);
                number(m, prec, r);
                m.set_u8(suff, 0);
                s.truncate(dot);
                m.set_u8(copy + dot as u32, 0);
            }
        },
    }
    match s.iter().position(|&c| c == b'-') {
        Some(d) if d > 0 && s[d - 1] != b'.' => {
            m.set_u8(copy + d as u32, 0);
            m.put_cstr(proj, &s[..d]);
            m.put_cstr(nam, &s[d + 1..]);
        }
        _ => {
            m.set_u8(proj, 0);
            m.put_cstr(nam, &s);
        }
    }
    m.free(copy);
}

/// The 35 tests of `SI_chapInfoOffsetFromChapId`, in its order: the offset
/// of a field of `SI_cid` and the row of the chapter table a chapter with that index has.
/// (The two picture chapters share row 11; no test names row 4 or 9.)
const CHAP_ROWS: [(u32, i32); 35] = [
    (32, 8),
    (0, 1),
    (4, 2),
    (84, 15),
    (8, 3),
    (24, 6),
    (20, 5),
    (28, 7),
    (104, 17),
    (108, 18),
    (12, 0),
    (40, 10),
    (112, 19),
    (116, 20),
    (88, 12),
    (92, 13),
    (96, 14),
    (100, 16),
    (16, 11),
    (44, 11),
    (120, 30),
    (124, 31),
    (128, 32),
    (132, 33),
    (48, 21),
    (136, 34),
    (64, 22),
    (60, 23),
    (52, 24),
    (56, 25),
    (68, 26),
    (140, 35),
    (72, 27),
    (76, 28),
    (80, 29),
];

/// The row of the chapter table for a chapter index of the scene dictionary, found by
/// comparing the index with the fields of `SI_cid` one after the other; -1 when none
/// holds it [soft 0x08120794 SI_chapInfoOffsetFromChapId].
pub fn chap_info_offset_from_chap_id(m: &Mem, chap: i32) -> i32 {
    for (off, row) in CHAP_ROWS {
        if m.i32(SI_CID.addr() + off) == chap {
            return row;
        }
    }
    -1
}

/// The file suffix of a chapter, a string of the binary's chapter table; `???` for a
/// chapter without a row [soft 0x08120a48 SI_edGetSuffixFromChapter].
pub fn ed_get_suffix_from_chapter(m: &Mem, chap: i32) -> CStr32 {
    match chap_info_offset_from_chap_id(m, chap) {
        -1 => Ptr32::new(S_NO_SUFFIX),
        row => m[SI_CHAP_INFO.cast::<ChapInfo>().add(row)].suffix,
    }
}

/// The first chapter of a dictionary whose suffix is this text, or -1 (also for a null
/// suffix) [soft 0x08120a7c SI_edGetChapterFromSuffix].
pub fn ed_get_chapter_from_suffix(m: &Mem, ed: Ptr32<ElemDict>, suffix: CStr32) -> i32 {
    for chap in 0..m[ed].nb_lists {
        let s = ed_get_suffix_from_chapter(m, chap);
        if !s.is_null() && !suffix.is_null() && m.cstr(s.addr()) == m.cstr(suffix.addr()) {
            return chap;
        }
    }
    -1
}

/// A string field of a record as a value; `None` for null.
pub fn text(m: &Mem, p: CStr32) -> Option<Vec<u8>> {
    (!p.is_null()).then(|| m.cstr(p.addr()))
}

// adapter: not in place
/// A name record as the value `si-db` works on (its file search, its directory scans and
/// its name routines take and return `si_db::ListNode`). The word at +0x00 is the address
/// the record holds.
pub fn to_value(m: &Mem, node: Ptr32<DbListNode>) -> Option<si_db::ListNode> {
    if node.is_null() {
        return None;
    }
    let n = m.read(node);
    Some(si_db::ListNode {
        w00: n.unk_00,
        s04: text(m, n.unk_04.cast()),
        sub_path: text(m, n.str_08),
        s0c: text(m, n.str_0c),
        prefix: text(m, n.prefix),
        name: text(m, n.name),
        version: text(m, n.version),
        subst: text(m, n.subst),
        time: n.time,
        full: text(m, n.fullname),
        w28: n.unk_28,
        w2a: n.unk_2a,
        mode: n.mode,
        size: n.unk_30,
    })
}

// adapter: not in place
/// Replaces one string of a record by a value that `si-db` computed: the old block is
/// freed, the new text is a new block (`None`: null). The database layer's own
/// allocation calls for the change are not reproduced.
pub fn set_string(m: &mut Mem, slot: Ptr32<CStr32>, value: Option<&[u8]>) {
    let old = m[slot];
    if !old.is_null() {
        m.free(old.addr());
    }
    m[slot] = match value {
        None => Ptr32::NULL,
        Some(v) => new_string(m, v),
    };
}

/// A copy of a text in a new block, as `strdup` makes one.
pub fn new_string(m: &mut Mem, s: &[u8]) -> CStr32 {
    let p = m.malloc(s.len() as u32 + 1);
    m.put_cstr(p, s);
    Ptr32::new(p)
}

// adapter: not in place
/// A whole record from a value of `si-db` (a directory scan's entry): a new record whose
/// strings are copies of the value's.
pub fn from_value(m: &mut Mem, n: &si_db::ListNode) -> Ptr32<DbListNode> {
    let node: Ptr32<DbListNode> = Ptr32::new(m.si_malloc(0x34));
    let put = |m: &mut Mem, s: &Option<Vec<u8>>| match s {
        None => Ptr32::NULL,
        Some(s) => new_string(m, s),
    };
    m[node].unk_00 = n.w00;
    m[node].unk_04 = put(m, &n.s04).cast();
    m[node].str_08 = put(m, &n.sub_path);
    m[node].str_0c = put(m, &n.s0c);
    m[node].prefix = put(m, &n.prefix);
    m[node].name = put(m, &n.name);
    m[node].version = put(m, &n.version);
    m[node].subst = put(m, &n.subst);
    m[node].time = n.time;
    m[node].fullname = put(m, &n.full);
    m[node].unk_28 = n.w28;
    m[node].unk_2a = n.w2a;
    m[node].mode = n.mode;
    m[node].unk_30 = n.size;
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_dict::globals::SI_CID;

    /// An outside world that knows the time and nothing else.
    struct Clock(u32);
    impl Outside for Clock {
        fn call(&mut self, _m: &mut Mem, routine: &'static si_dict::Routine, _args: &[u32]) -> u32 {
            assert_eq!(routine.name, "time");
            self.0
        }
    }

    /// An image with the constants and globals these routines read, and the chapter
    /// indices of a scene dictionary with the 36 standard chapters.
    fn image(loading: bool) -> Mem {
        let mut m = Mem::new();
        si_mem::init_globals(&mut m, si_dict::globals::ALL);
        si_dict::rodata::store(&mut m);
        crate::globals::store(&mut m);
        let (_, p) = crate::init::empty_scene();
        m.write(SI_CID, p.cid);
        m[LOADING] = loading as i16;
        m
    }

    const NAMES: [&str; 22] = [
        "",
        "cube",
        "100%blue",
        "a b/c",
        "-lead",
        "-lead-more",
        "mid-dash",
        "dot.",
        "dot.mid",
        "x!y\"z#$%&'()*+,",
        "a:b;c<d=e>f?g@h",
        "[\\]^`{|}~",
        "Bike-A_bike.3-0",
        "Bike-A_bike.3-0.hrc",
        "walk.12.pic",
        "walk.pic",
        "a.b.c.d",
        "-x.1-0",
        "p.-n.2-1",
        "no_dash.7-12.dsc",
        "x.01-0",
        "x.1-0-0.y",
    ];

    #[test]
    fn a_new_record_and_its_disposal() {
        let mut m = image(false);
        let before = m.heap_blocks().len();
        let n = list_node_allocate(&mut m, &mut Clock(77));
        assert_eq!(m.heap_blocks().len() - before, 4);
        assert_eq!(m.block_size(n.addr()), Some(0x34));
        let v = to_value(&m, n).unwrap();
        assert_eq!(v, si_db::ListNode { size: v.size, ..si_db::ListNode::allocate(77) });
        list_node_free(&mut m, n);
        assert_eq!(m.freed().len(), 4);
        assert_eq!(*m.freed().last().unwrap(), n.addr());
        list_node_free(&mut m, Ptr32::NULL);
        assert!(m.fault().is_none());
    }

    #[test]
    fn names_are_cleaned_as_the_value_form_cleans_them() {
        let long = "x-".repeat(60);
        let mut names: Vec<&str> = NAMES.to_vec();
        names.push(&long);
        for loading in [false, true] {
            for is_prefix in [false, true] {
                for name in &names {
                    let mut m = image(loading);
                    let p = new_string(&mut m, name.as_bytes());
                    let got = name_check_validity(&mut m, p, is_prefix as i16);
                    let want = si_db::name_check_validity(Some(name.as_bytes().to_vec()), is_prefix, loading);
                    assert_eq!(text(&m, got), want, "{name:?} prefix {is_prefix} loading {loading}");
                    // in place, or freed when empty
                    assert_eq!(got.is_null(), name.is_empty());
                    assert_eq!(m.is_freed(p.addr()), name.is_empty());
                    assert!(m.fault().is_none());
                }
            }
        }
        let mut m = image(false);
        assert!(name_check_validity(&mut m, Ptr32::NULL, 1).is_null());
    }

    #[test]
    fn a_long_name_is_rewritten_behind_its_cut() {
        let mut m = image(false);
        let mut name = vec![b'a'; 110];
        name[104] = b'%';
        let p = new_string(&mut m, &name);
        name_check_validity(&mut m, p, 0);
        assert_eq!(m.strlen(p.addr()), 99);
        assert_eq!(m.u8(p.addr() + 104), b'_');
        assert_eq!(m.u8(p.addr() + 105), b'a');
    }

    #[test]
    fn file_names_are_split_as_the_value_form_splits_them() {
        for name in NAMES {
            let mut m = image(false);
            let p = new_string(&mut m, name.as_bytes());
            let bufs: Vec<u32> = (0..5).map(|_| m.place(0x200, 4)).collect();
            let blocks = m.heap_blocks().len();
            filename_expand(&mut m, p, [bufs[0], bufs[1], bufs[2], bufs[3], bufs[4]]);
            let want = si_db::filename_expand(name.as_bytes());
            let got: Vec<Vec<u8>> = bufs.iter().map(|&b| m.cstr(b)).collect();
            assert_eq!(got[0], want.prefix, "{name:?}");
            assert_eq!(got[1], want.name, "{name:?}");
            assert_eq!(got[2], want.version.to_string().into_bytes(), "{name:?}");
            assert_eq!(got[3], want.subst.to_string().into_bytes(), "{name:?}");
            assert_eq!(got[4], want.suffix, "{name:?}");
            // one copy made, and freed; the name itself untouched
            assert_eq!(m.heap_blocks().len(), blocks + 1);
            assert!(m.is_freed(m.heap_blocks()[blocks].0));
            assert_eq!(m.cstr(p.addr()), name.as_bytes());
            assert!(m.fault().is_none());
        }
        let mut m = image(false);
        let blocks = m.heap_blocks().len();
        filename_expand(&mut m, Ptr32::NULL, [0; 5]);
        assert_eq!(m.heap_blocks().len(), blocks);
        assert!(m.fault().is_none());
    }

    #[test]
    fn full_names_are_formatted_as_the_value_form_formats_them() {
        for loading in [false, true] {
            for name in NAMES {
                let mut value = si_db::scn_get_list_node(name.as_bytes(), Some(b"al"), false, 5);
                let mut m = image(loading);
                let node = from_value(&mut m, &value);
                let old = m[node].fullname;
                let got = format_full_name(&mut m, node);
                let want = si_db::format_full_name(&mut value, loading);
                assert_eq!(m.cstr(got.addr()), want, "{name:?} loading {loading}");
                assert!(m[node].fullname.is_null());
                assert!(m.is_freed(old.addr()));
                assert_eq!(to_value(&m, node).unwrap(), value, "{name:?} loading {loading}");
                assert_eq!(m.cstr(WORK_STR.addr()), want);
                assert!(m.fault().is_none());
            }
        }
    }

    #[test]
    fn suffixes_by_chapter_as_the_value_form_has_them() {
        let mut m = image(false);
        for chap in -2..40 {
            let s = ed_get_suffix_from_chapter(&m, chap);
            assert_eq!(m.cstr(s.addr()), si_db::ed_get_suffix_from_chapter(chap).as_bytes(), "chapter {chap}");
        }
        let ed: Ptr32<ElemDict> = Ptr32::new(m.calloc(1, 0x70));
        m[ed].nb_lists = 36;
        for suffix in ["hrc", "dsc", "pic", "shd", "", "???", "t3D", "nope"] {
            let p = new_string(&mut m, suffix.as_bytes());
            assert_eq!(ed_get_chapter_from_suffix(&m, ed, p), si_db::ed_get_chapter_from_suffix(36, Some(suffix.as_bytes())), "suffix {suffix:?}");
        }
        assert_eq!(ed_get_chapter_from_suffix(&m, ed, Ptr32::NULL), -1);
        assert!(m.fault().is_none());
    }
}
