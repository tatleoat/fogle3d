// SPDX-License-Identifier: MIT
//! The order of a chapter's elements (`elemdict.c`): the comparison the browser, the
//! duplicate-name check and the scene description writer sort by, and the sort itself;
//! with the two routines of `text.c` the comparison of pictures calls.
//!
//! The sorted order is an array of element indices the chapter owns (`SI_ElemList`
//! +0x0c), made with the C library's `calloc` and sorted with its `qsort`; the comparison
//! reads what it sorts by from four globals (`glbEL`, `glbSorttype`, `glbSortUp`,
//! `glbHierSort`).

use crate::elemdict::compare_elem_name;
use crate::globals::{GLB_EL, GLB_HIER_SORT, GLB_SORTTYPE, GLB_SORT_UP, SI_CID, STR1, STR2};
use crate::libc::{qsort, strcmp, strrchr};
use crate::outside::Outside;
use si_mem::clib::strtol;
use si_mem::{CStr32, Mem, Ptr32};
use si_types::dict::{DbListNode, Elem, ElemList};
use si_x87::{Round, F80};

/// Sort keys (`SI_ElmPart`, the third argument of `SI_elemlistSortChapterElements`): which
/// part of an element the order goes by.
pub mod key {
    /// The string at +0x08 of the name record (a sub-directory).
    pub const SUB_PATH: i32 = 1;
    /// The string at +0x0c of the name record (an alias).
    pub const ALIAS: i32 = 2;
    /// The element name (+0x14).
    pub const NAME: i32 = 4;
    /// The version, as a number.
    pub const VERSION: i32 = 5;
    /// The time (+0x20).
    pub const TIME: i32 = 6;
    /// The full name (+0x24): what the description writer and the duplicate-name check use.
    pub const FULL_NAME: i32 = 9;
    /// Prefix and name joined by a dash.
    pub const PREFIX_NAME: i32 = 10;
    /// Directories after everything else.
    pub const DIRECTORY: i32 = 11;
    /// Pictures: suffix, the full name without its frame number, the frame number.
    pub const PICTURE: i32 = 12;
    /// The word at +0x30 of the name record (a size).
    pub const SIZE: i32 = 13;
    /// The schematic box: height, then width.
    pub const BOX_HEIGHT: i32 = 14;
    /// The schematic box: width, then height.
    pub const BOX_WIDTH: i32 = 15;
    /// Schematic position x.
    pub const SCHEM_X: i32 = 16;
    /// Schematic position y.
    pub const SCHEM_Y: i32 = 17;
}

/// [soft 0x083cf0d4 SI_strGetSeqNum]: the number after a name's last dot, as `strtol`
/// reads it (blanks skipped, a sign allowed, whatever follows the digits ignored); -1
/// when there is no dot, nothing after it, or no number there.
pub fn str_get_seq_num(m: &mut Mem, s: CStr32) -> i32 {
    let dot = strrchr(m, s.addr(), b'.');
    if dot == 0 {
        return -1;
    }
    let start = dot + 1;
    if m.u8(start) == 0 {
        return -1;
    }
    let end = m.local(4);
    let value = strtol(m, start, end, 10);
    let converted = m.u32(end) != start;
    m.release(4);
    if converted {
        value
    } else {
        -1
    }
}

/// [soft 0x083cf154 SI_strStripDigit]: cuts a frame number off a name, in place: when
/// everything after the last dot is digits (a dash is allowed as the first character),
/// the dot becomes the end of the string. A name that ends in a dot is cut there too.
/// Answers the string.
pub fn str_strip_digit(m: &mut Mem, s: CStr32) -> CStr32 {
    let dot = strrchr(m, s.addr(), b'.');
    if dot != 0 {
        let mut other = false;
        let first = m.u8(dot + 1);
        if first != 0 {
            let mut i = 1;
            loop {
                // (the C library's digit class; a byte above 0x7f is no digit)
                if !m.u8(dot + i).is_ascii_digit() && !(i == 1 && first == b'-') {
                    other = true;
                }
                i += 1;
                if m.u8(dot + i) == 0 || other {
                    break;
                }
            }
        }
        if !other {
            m.set_u8(dot, 0);
        }
    }
    s
}

/// `sscanf(text, "%d", out)`: blanks, an optional sign, digits; the number, held to 32
/// bits, is stored in `*out`. Nothing is stored when there is no digit.
fn scan_int(m: &mut Mem, text: CStr32, out: Ptr32<i32>) {
    let t = m.cstr(text.addr());
    let mut i = 0;
    while t.get(i).is_some_and(|c| b" \t\n\x0b\x0c\r".contains(c)) {
        i += 1;
    }
    let negative = t.get(i) == Some(&b'-');
    if matches!(t.get(i), Some(b'+' | b'-')) {
        i += 1;
    }
    let start = i;
    let mut v: i64 = 0;
    while let Some(c) = t.get(i).filter(|c| c.is_ascii_digit()) {
        v = (v * 10 + (c - b'0') as i64).min(1 << 40);
        i += 1;
    }
    if i == start {
        return;
    }
    let v = if negative { -v } else { v };
    m[out] = v.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
}

/// A `%s` argument as the C library prints it.
fn text(m: &Mem, s: CStr32) -> Vec<u8> {
    if s.is_null() {
        b"(null)".to_vec()
    } else {
        m.cstr(s.addr())
    }
}

/// `sprintf(buffer, "%s%c%s", prefix, '-', name)`, or `sprintf(buffer, "%s", name)` for a
/// record without a prefix.
fn print_prefix_name(m: &mut Mem, buffer: u32, node: Ptr32<DbListNode>) {
    let mut out = Vec::new();
    if !m[node].prefix.is_null() {
        out.extend(text(m, m[node].prefix));
        out.push(b'-');
    }
    out.extend(text(m, m[node].name));
    m.put_cstr(buffer, &out);
}

fn sign(a: i32, b: i32) -> i32 {
    if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    }
}

/// `(int)` of a value of the x87 unit: cut toward zero; 0x80000000 for a NaN or a value
/// that does not fit.
fn to_int(v: F80) -> i32 {
    v.to_i32(Round::Zero)
}

/// [soft 0x08131d7c elemdictCompareElems]: `qsort`'s comparison of two entries of the
/// chapter `glbEL`: negative, zero or positive as the first goes before, with or after the
/// second. An entry is an element index, or with `glbHierSort` a position in the
/// chapter's hierarchical order (+0x10).
///
/// 1. An entry outside the chapter compares equal to anything.
/// 2. A freed slot (status 2) goes after every other; two freed slots are equal.
/// 3. An element without a name record compares equal to anything.
/// 4. Then the key `glbSorttype` ([`key`]). For a key that is a string the two strings are
///    compared with `SI_compareElemName`; a null string makes the two equal. Equal strings
///    are told apart by: status (in use first), new-flag (lower first), depth (the first
///    goes after when it is deeper), version (lower first). A key the routine has no case
///    for compares everything equal.
/// 5. For a descending sort (`glbSortUp` 0) the result is negated. The answers of 1 to 3
///    and of a null string are not.
///
/// The version is read with `sscanf("%d")` into a local that is not set first: a version
/// that is no number leaves there what the stack held (here too: a word of the image's
/// stack). The pictures key copies both full names (`strdup`), cuts their frame numbers
/// off, compares the suffixes (the element's own, else the chapter's), the cut names and
/// the frame numbers, and frees the copies. The prefix-name key prints into two statics
/// (`str1`, `str2`, 512 bytes each).
pub fn compare_elems(m: &mut Mem, a: Ptr32<i32>, b: Ptr32<i32>) -> i32 {
    let list = m[GLB_EL];
    let ia = m[a];
    if ia < 0 || m[list].nb_elems <= ia {
        return 0;
    }
    let ib = m[b];
    if ib < 0 || m[list].nb_elems <= ib {
        return 0;
    }
    let (ea, eb): (Ptr32<Elem>, Ptr32<Elem>) = if m[GLB_HIER_SORT] == 0 {
        (m[list].elems.add(ia), m[list].elems.add(ib))
    } else {
        let order = m[list].hier_order;
        (m[list].elems.add(m[order.add(ia)]), m[list].elems.add(m[order.add(ib)]))
    };
    if m[ea].status == 2 {
        return if m[eb].status == 2 { 0 } else { 1 };
    }
    if m[eb].status == 2 {
        return -1;
    }
    let (na, nb) = (m[ea].node, m[eb].node);
    if na.is_null() || nb.is_null() {
        return 0;
    }
    // the two locals a version is scanned into: not set first, so they keep what the
    // call before left
    let frame = crate::frame(m, 8);
    let (va, vb): (Ptr32<i32>, Ptr32<i32>) = (Ptr32::new(frame + 4), Ptr32::new(frame));
    let mut strings: Option<(CStr32, CStr32)> = None;
    let mut v = 0;
    match m[GLB_SORTTYPE] {
        key::SUB_PATH => strings = Some((m[na].str_08, m[nb].str_08)),
        key::ALIAS => strings = Some((m[na].str_0c, m[nb].str_0c)),
        key::NAME => strings = Some((m[na].name, m[nb].name)),
        key::FULL_NAME => strings = Some((m[na].fullname, m[nb].fullname)),
        key::PREFIX_NAME => {
            print_prefix_name(m, STR1.addr(), na);
            print_prefix_name(m, STR2.addr(), nb);
            strings = Some((STR1.cast(), STR2.cast()));
        }
        key::VERSION => {
            scan_int(m, m[na].version, va);
            scan_int(m, m[nb].version, vb);
            v = sign(m[va], m[vb]);
        }
        key::TIME => v = sign(m[na].time, m[nb].time),
        key::SIZE => v = sign(m[na].unk_30, m[nb].unk_30),
        key::DIRECTORY => {
            let (da, db) = (m[na].mode & 0xf000 == 0x4000, m[nb].mode & 0xf000 == 0x4000);
            v = if da { !db as i32 } else if db { -1 } else { 0 };
        }
        key::PICTURE => {
            let ca = m.strdup(m[na].fullname.addr());
            let cb = m.strdup(m[nb].fullname.addr());
            str_strip_digit(m, Ptr32::new(ca));
            str_strip_digit(m, Ptr32::new(cb));
            let mut sa = m[ea].suffix;
            if sa.is_null() {
                sa = m[list].suffix;
            }
            let mut sb = m[eb].suffix;
            if sb.is_null() {
                sb = m[list].suffix;
            }
            let fa = str_get_seq_num(m, m[na].fullname);
            let fb = str_get_seq_num(m, m[nb].fullname);
            if !sa.is_null() && !sb.is_null() {
                v = strcmp(m, sa.addr(), sb.addr());
            }
            if v == 0 {
                v = strcmp(m, ca, cb);
                if v == 0 {
                    v = sign(fa, fb);
                }
            }
            if ca != 0 {
                m.free(ca);
            }
            if cb != 0 {
                m.free(cb);
            }
        }
        k @ (key::BOX_HEIGHT | key::BOX_WIDTH) => {
            // differences of the box's corners, formed and compared in the x87 unit
            let (ba, bb) = (m[ea].schem_bbox, m[eb].schem_bbox);
            let d = |hi: f32, lo: f32| F80::from_f32(hi) - F80::from_f32(lo);
            let (wa, ha, wb, hb) = (d(ba[2], ba[0]), d(ba[3], ba[1]), d(bb[2], bb[0]), d(bb[3], bb[1]));
            let equal = |x: F80, y: F80| x.compare(y) == Some(core::cmp::Ordering::Equal);
            v = if k == key::BOX_HEIGHT {
                if equal(ha, hb) { to_int(wa - wb) } else { to_int(ha - hb) }
            } else if !equal(wa, wb) {
                to_int(wa - wb)
            } else {
                to_int(ha - hb)
            };
        }
        k @ (key::SCHEM_X | key::SCHEM_Y) => {
            let (xa, xb) = if k == key::SCHEM_X { (m[ea].schem_x, m[eb].schem_x) } else { (m[ea].schem_y, m[eb].schem_y) };
            v = match F80::from_f32(xa).compare(F80::from_f32(xb)) {
                Some(core::cmp::Ordering::Greater) => 1,
                Some(core::cmp::Ordering::Less) => -1,
                _ => 0,
            };
        }
        _ => strings = Some((Ptr32::NULL, Ptr32::NULL)),
    }
    if let Some((sa, sb)) = strings {
        if sa.is_null() || sb.is_null() {
            m.release(8);
            return 0;
        }
        v = compare_elem_name(m, sa, sb);
        if v == 0 {
            let (sta, stb) = (m[ea].status, m[eb].status);
            if sta != stb {
                v = if sta == 0 { -1 } else { 1 };
            } else {
                v = sign(m[ea].new_flag as i32, m[eb].new_flag as i32);
                if v == 0 {
                    v = (m[eb].hier_level < m[ea].hier_level) as i32;
                }
                if v == 0 {
                    scan_int(m, m[na].version, va);
                    scan_int(m, m[nb].version, vb);
                    v = sign(m[va], m[vb]);
                }
            }
        }
    }
    m.release(8);
    if m[GLB_SORT_UP] == 0 {
        v = v.wrapping_neg();
    }
    v
}

/// [soft 0x08132330 SI_elemlistSortChapterElements]: builds the sorted order of a chapter
/// (+0x0c): the element indices in the order of `key`, ascending when `up` is not 0.
/// Nothing for a null chapter or one without slots.
///
/// - For the two picture chapters (`SI_cid`'s RENDER_PICTURES and PICTURES) the keys
///   "name" and "full name" become the pictures key.
/// - The old order is freed; the new one comes from `calloc`.
/// - A chapter with fewer than two slots is not sorted.
/// - With `hier` and a hierarchical order in the chapter (+0x10, +0x14) only the roots
///   (depth 0) are sorted, as positions in that order; then each root is followed by the
///   entries before it in the hierarchical order, as many as its count says, in an array
///   of the chapter's size. An entry beyond that size prints `ERROR in
///   elemlistSortChapterElements. Please report` and is written all the same, past the
///   array, as the original writes it.
pub fn sort_chapter_elements(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<ElemList>, chap: i32, key_in: i32, up: i16, hier: i16) {
    if list.is_null() || m[list].nb_elems == 0 {
        return;
    }
    let mut key = key_in;
    if (key == key::NAME || key == key::FULL_NAME) && (chap == m[SI_CID].rim || chap == m[SI_CID].img) {
        key = key::PICTURE;
    }
    if !m[list].sort.is_null() {
        let old = m[list].sort.addr();
        m.free(old);
        m[list].sort = Ptr32::NULL;
    }
    m[GLB_EL] = list;
    m[GLB_SORT_UP] = up;
    m[GLB_HIER_SORT] = 0;
    if hier != 0 {
        m[GLB_HIER_SORT] = !m[list].hier_order.is_null() as i16;
    }
    let root = |m: &Mem, i: i32| {
        let which = m[m[list].hier_order.add(i)];
        m[m[list].elems.add(which)].hier_level == 0
    };
    let mut n = m[list].nb_elems;
    if m[GLB_HIER_SORT] != 0 {
        n = 0;
        let mut i = 0;
        while i < m[list].nb_elems {
            if root(m, i) {
                n += 1;
            }
            i += 1;
        }
    }
    m[GLB_SORTTYPE] = key;
    let base: Ptr32<i32> = Ptr32::new(m.calloc(n as u32, 4));
    if m[GLB_HIER_SORT] == 0 {
        let mut i = 0;
        while i < m[list].nb_elems {
            m[base.add(i)] = i;
            i += 1;
        }
    } else {
        n = 0;
        let mut i = 0;
        while i < m[list].nb_elems {
            if root(m, i) {
                m[base.add(n)] = i;
                n += 1;
            }
            i += 1;
        }
    }
    if m[list].nb_elems < 2 {
        m[list].sort = base;
        return;
    }
    qsort(m, base.addr(), n as u32, 4, &mut |m, a, b| compare_elems(m, Ptr32::new(a), Ptr32::new(b)));
    if m[GLB_HIER_SORT] == 0 {
        m[list].sort = base;
        return;
    }
    let size = m[list].nb_elems;
    m[list].sort = Ptr32::new(m.calloc(size as u32, 4));
    let mut at = 0;
    let mut k = 0;
    while k < n {
        let mut i = m[base.add(k)];
        k += 1;
        let mut left = m[m[list].hier_count.add(i)];
        while -1 < left {
            if m[list].nb_elems <= at {
                o.stdout(si_rt::message(0x0868_3bc0));
            }
            let entry = m[m[list].hier_order.add(i)];
            let slot = m[list].sort.add(at);
            m[slot] = entry;
            at += 1;
            i -= 1;
            left -= 1;
        }
    }
    if !base.is_null() {
        m.free(base.addr());
    }
}
