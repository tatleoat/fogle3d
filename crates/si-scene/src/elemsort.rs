// SPDX-License-Identifier: MIT
//! The order of the elements of a chapter (`elemdict.c`): the comparison the browser and the
//! scene description writer sort by, and the hierarchical order of the ANIMATION chapter.
//!
//! The writer of a scene description sorts each chapter by full name before it numbers the
//! elements (finding 0018), so this order decides the numbers in a `.dsc` file.
//!
//! This is the store form, kept for that writer ([`crate::save`]), which is not on the
//! image, and for its two extra name orders. The form on the image, held against the
//! emulated original, is `si-dict`'s (`si_dict::elemsort`); the loader's and the saver's
//! duplicate-name check use that one.

use crate::dict::Dict;
use si_ascii::dsc::{compare_elem_name, compare_elem_name_nt};
use si_db::{scan_int, ListNode};
use si_types::dict::{ELEM_DELETED, ELEM_VALID};
use si_types::store::{ElemListStore, ElemStore};
use std::cmp::Ordering;

/// Sort keys (`SI_ElmPart`, the third argument of `SI_elemlistSortChapterElements`): which
/// part of an element the order goes by. The numbers are the original's.
pub mod key {
    /// The sub-directory (name record +0x08).
    pub const SUB_PATH: i32 = 1;
    /// The string at +0x0c of the name record (an alias).
    pub const ALIAS: i32 = 2;
    /// The element name (+0x14).
    pub const NAME: i32 = 4;
    /// The version number.
    pub const VERSION: i32 = 5;
    /// The time (+0x20).
    pub const TIME: i32 = 6;
    /// The full name (+0x24): what the description writer uses.
    pub const FULL_NAME: i32 = 9;
    /// Prefix and name joined by a dash.
    pub const PREFIX_NAME: i32 = 10;
    /// Directories after everything else.
    pub const DIRECTORY: i32 = 11;
    /// Pictures: suffix, the name without its frame number, the frame number.
    pub const PICTURE: i32 = 12;
    /// The size (+0x30).
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

/// Which build's name comparison to use. The same source compiles to two different orders
/// (finding 0006): the Linux build compares bytes, the NT build compares them after
/// `toupper`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameOrder {
    Linux,
    Nt,
    /// Plain `strcmp`, without the underscore rule. Not in this binary: it is the order the
    /// sample files of versions 2.61 to 2.65 are in (a guess from those files; finding 0018).
    Plain,
}

/// The globals of the comparison (`glbEL`, `glbSorttype`, `glbSortUp`, `glbHierSort`).
#[derive(Clone, Copy, Debug)]
pub struct SortSpec {
    pub key: i32,
    /// False reverses the result.
    pub up: bool,
    /// The entries being sorted are positions in the chapter's hierarchical order
    /// (`ElemList` +0x10) instead of element indices.
    pub hier: bool,
    pub names: NameOrder,
}

/// Cuts a frame number off a name: if everything after the last dot is digits (a dash is
/// allowed as the first character), the name ends at that dot [soft 0x083cf154
/// SI_strStripDigit]. A name ending in a dot is cut there too.
pub fn str_strip_digit(name: &[u8]) -> &[u8] {
    let Some(dot) = name.iter().rposition(|&c| c == b'.') else { return name };
    let tail = &name[dot + 1..];
    let all_digits = tail.iter().enumerate().all(|(i, &c)| c.is_ascii_digit() || (i == 0 && c == b'-'));
    if all_digits {
        &name[..dot]
    } else {
        name
    }
}

/// The number after a name's last dot [soft 0x083cf0d4 SI_strGetSeqNum]: -1 when there is
/// no dot, nothing after it, or no number there (`strtol` converts nothing: `walk.pic`).
/// `walk.12` gives 12; `walk.12x` gives 12 too; blanks before the number are skipped as
/// `strtol` skips them.
pub fn str_get_seq_num(name: &[u8]) -> i32 {
    match name.iter().rposition(|&c| c == b'.') {
        Some(dot) if dot + 1 < name.len() => scan_int(&name[dot + 1..]).map_or(-1, |(v, _)| v),
        _ => -1,
    }
}

/// A C cast of a floating-point value to `int` as this compiler does it: the x87 unit is
/// switched to truncation for the store, so the value is cut towards zero; a NaN or a value
/// that does not fit gives 0x80000000.
pub fn c_cast_to_int(v: f64) -> i32 {
    let r = v.trunc();
    if r.is_nan() || r < i32::MIN as f64 || r > i32::MAX as f64 {
        i32::MIN
    } else {
        r as i32
    }
}

fn int_cmp(a: i32, b: i32) -> i32 {
    match a.cmp(&b) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// `sscanf(text, "%d", &v)` where `v` starts with whatever the stack held: 0 here when the
/// text is not a number or the pointer is null.
fn version_of(n: &ListNode) -> i32 {
    n.version.as_deref().and_then(scan_int).map_or(0, |(v, _)| v)
}

fn c_strcmp(a: &[u8], b: &[u8]) -> i32 {
    match a.cmp(b) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// Compares two entries of a chapter [soft 0x08131d7c elemdictCompareElems]: negative, zero
/// or positive as the first goes before, with or after the second.
///
/// `a` and `b` are what the sorted array holds: element indices, or with `spec.hier`
/// positions in the chapter's hierarchical order.
///
/// 1. An entry outside the chapter compares equal to anything.
/// 2. A freed slot (status 2) goes after every other; two freed slots are equal. These two
///    answers are **not** reversed for a descending sort.
/// 3. An element without a name record compares equal to anything.
/// 4. Then the key. For the keys that are a string (sub-directory, alias, name, full name,
///    prefix-name) the strings are compared with the element name comparison
///    (`SI_compareElemName`: [`compare_elem_name`], or the NT build's); a missing string
///    makes the two equal (and not reversed). Equal strings are told apart by: status
///    (in use first), new-flag (lower first), depth (roots first), version (lower first).
/// 5. A descending sort negates the result.
///
/// The pictures key compares the suffix (the element's own, else the chapter's), then the
/// full name without its frame number, then the frame number.
pub fn compare_elems(d: &Dict, list: &ElemListStore, hier_order: Option<&[i32]>, spec: &SortSpec, a: i32, b: i32) -> i32 {
    let n = list.elems.len() as i32;
    if a < 0 || a >= n || b < 0 || b >= n {
        return 0;
    }
    let pick = |i: i32| -> Option<&ElemStore> {
        if spec.hier {
            // the original reads the hierarchical order unchecked
            list.elems.get(usize::try_from(*hier_order?.get(i as usize)?).ok()?)
        } else {
            list.elems.get(i as usize)
        }
    };
    let (Some(ea), Some(eb)) = (pick(a), pick(b)) else { return 0 };
    if ea.rec.status == ELEM_DELETED {
        return if eb.rec.status == ELEM_DELETED { 0 } else { 1 };
    }
    if eb.rec.status == ELEM_DELETED {
        return -1;
    }
    let (Some(na), Some(nb)) = (d.nodes.get(ea.rec.node.0), d.nodes.get(eb.rec.node.0)) else { return 0 };
    let finish = |v: i32| if spec.up { v } else { v.wrapping_neg() };
    let strings: (Option<Vec<u8>>, Option<Vec<u8>>) = match spec.key {
        key::SUB_PATH => (na.sub_path.clone(), nb.sub_path.clone()),
        key::ALIAS => (na.s0c.clone(), nb.s0c.clone()),
        key::NAME => (na.name.clone(), nb.name.clone()),
        key::FULL_NAME => (na.full.clone(), nb.full.clone()),
        key::PREFIX_NAME => {
            // `%s%c%s` with a dash, or the name alone when the prefix pointer is null; a
            // null name prints as glibc prints one
            let join = |n: &ListNode| {
                let name = n.name.clone().unwrap_or_else(|| si_db::NULL_TEXT.to_vec());
                match &n.prefix {
                    None => name,
                    Some(p) => [p.as_slice(), b"-", name.as_slice()].concat(),
                }
            };
            (Some(join(na)), Some(join(nb)))
        }
        key::VERSION => return finish(int_cmp(version_of(na), version_of(nb))),
        key::TIME => return finish(int_cmp(na.time, nb.time)),
        key::SIZE => return finish(int_cmp(na.size, nb.size)),
        key::DIRECTORY => {
            let dir = |n: &ListNode| n.mode & 0xf000 == 0x4000;
            return finish(match (dir(na), dir(nb)) {
                (true, false) => 1,
                (false, true) => -1,
                _ => 0,
            });
        }
        key::PICTURE => {
            let (fa, fb) = (na.full.clone().unwrap_or_default(), nb.full.clone().unwrap_or_default());
            let mut v = 0;
            // the element's own suffix string is not kept in this crate: the chapter's is used
            if let (Some(sa), Some(sb)) = (list.suffix.as_deref(), list.suffix.as_deref()) {
                v = c_strcmp(sa, sb);
            }
            if v == 0 {
                v = c_strcmp(str_strip_digit(&fa), str_strip_digit(&fb));
                if v == 0 {
                    v = int_cmp(str_get_seq_num(&fa), str_get_seq_num(&fb));
                }
            }
            return finish(v);
        }
        key::BOX_HEIGHT | key::BOX_WIDTH => {
            // the differences are formed in the x87 registers from the stored floats; in
            // double precision they are exact for any values a schematic view holds
            let ext = |e: &ElemStore| {
                let b = e.rec.schem_bbox;
                (b[2] as f64 - b[0] as f64, b[3] as f64 - b[1] as f64)
            };
            let ((wa, ha), (wb, hb)) = (ext(ea), ext(eb));
            // the result is a difference cut to an integer; which difference comes first
            // depends on the key, and the other is used when the first two are equal
            let v = if spec.key == key::BOX_HEIGHT {
                if ha == hb { wa - wb } else { ha - hb }
            } else if wa != wb {
                wa - wb
            } else {
                ha - hb
            };
            return finish(c_cast_to_int(v));
        }
        key::SCHEM_X | key::SCHEM_Y => {
            let (va, vb) = if spec.key == key::SCHEM_X { (ea.rec.schem_x, eb.rec.schem_x) } else { (ea.rec.schem_y, eb.rec.schem_y) };
            return finish(if vb < va {
                1
            } else if va < vb {
                -1
            } else {
                0
            });
        }
        _ => (None, None),
    };
    let (Some(sa), Some(sb)) = strings else { return 0 };
    let mut v = match spec.names {
        NameOrder::Linux => compare_elem_name(&sa, &sb),
        NameOrder::Nt => compare_elem_name_nt(&sa, &sb),
        NameOrder::Plain => c_strcmp(&sa, &sb),
    };
    if v == 0 {
        if ea.rec.status != eb.rec.status {
            v = if ea.rec.status == ELEM_VALID { -1 } else { 1 };
        } else {
            v = int_cmp(ea.rec.new_flag as i32, eb.rec.new_flag as i32);
            if v == 0 {
                v = (eb.rec.hier_level < ea.rec.hier_level) as i32;
            }
            if v == 0 {
                v = int_cmp(version_of(na), version_of(nb));
            }
        }
    }
    finish(v)
}

/// Builds the sorted order of a chapter: the element indices in the order of `key`
/// [soft 0x08132330 SI_elemlistSortChapterElements]. The original stores the array in the
/// chapter (`ElemList` +0x0c); here it is returned, `None` when the chapter has no slots.
///
/// - For the two picture chapters the keys "name" and "full name" become the pictures key.
/// - A chapter with fewer than two slots is not sorted.
/// - With `hier` and a hierarchical order in the chapter (`SI_elemdictSortAnimElements`, `si-dict`'s on the image), only
///   the roots are sorted and each root is followed by the entries before it in the
///   hierarchical order, as many as its count says.
///
/// The sort is the C library's `qsort` ([`crate::csort::msort`]). The comparison is not
/// symmetric for equal names at different depths, so the order of such a pair depends on
/// the order in which the sort presents them.
pub fn sort_chapter_elements(
    d: &Dict, chap: i32, key_in: i32, up: bool, hier: bool, names: NameOrder, picture_chapters: (i32, i32),
    hier_tables: Option<(&[i32], &[i32])>,
) -> Option<Vec<i32>> {
    let list = d.ed.lists.get(usize::try_from(chap).ok()?)?;
    let n = list.elems.len();
    if n == 0 {
        return None;
    }
    let mut key = key_in;
    if (key == key::NAME || key == key::FULL_NAME) && (chap == picture_chapters.0 || chap == picture_chapters.1) {
        key = key::PICTURE;
    }
    let hier = hier && hier_tables.is_some();
    let spec = SortSpec { key, up, hier, names };
    let order = hier_tables.map(|t| t.0);
    let mut base: Vec<i32> = if hier {
        let order = order.unwrap();
        (0..n as i32).filter(|&i| order.get(i as usize).and_then(|&e| list.elems.get(e as usize)).is_some_and(|e| e.rec.hier_level == 0)).collect()
    } else {
        (0..n as i32).collect()
    };
    if n < 2 {
        return Some(base);
    }
    crate::csort::msort(&mut base, &mut |&a, &b| compare_elems(d, list, order, &spec, a, b));
    if !hier {
        return Some(base);
    }
    let (order, counts) = hier_tables.unwrap();
    let mut out = vec![0i32; n];
    let mut at = 0usize;
    for &pos in &base {
        let mut i = pos;
        let mut left = counts.get(pos as usize).copied().unwrap_or(0);
        while left > -1 {
            // the original prints "ERROR in elemlistSortChapterElements" and writes past
            // the array; here the entry is dropped
            if at < n {
                out[at] = order.get(usize::try_from(i).unwrap_or(usize::MAX)).copied().unwrap_or(0);
            }
            at += 1;
            i -= 1;
            left -= 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init;

    fn names(d: &Dict, chap: i32, order: &[i32]) -> Vec<String> {
        order.iter().map(|&i| String::from_utf8_lossy(d.node_of(chap, i).and_then(|n| n.full.as_deref()).unwrap_or(b"-")).into_owned()).collect()
    }

    #[test]
    fn frame_numbers() {
        assert_eq!(str_strip_digit(b"walk.12"), b"walk");
        assert_eq!(str_strip_digit(b"walk.-12"), b"walk");
        assert_eq!(str_strip_digit(b"walk.1-2"), b"walk.1-2");
        assert_eq!(str_strip_digit(b"walk.pic"), b"walk.pic");
        assert_eq!(str_strip_digit(b"walk."), b"walk");
        assert_eq!(str_strip_digit(b"walk"), b"walk");
        assert_eq!(str_get_seq_num(b"walk.12"), 12);
        assert_eq!(str_get_seq_num(b"walk.pic"), -1);
        assert_eq!(str_get_seq_num(b"walk.12x"), 12);
        assert_eq!(str_get_seq_num(b"walk.-3"), -3);
        assert_eq!(str_get_seq_num(b"walk."), -1);
        assert_eq!(str_get_seq_num(b"walk"), -1);
    }

    #[test]
    fn full_name_order_with_its_tie_breaks() {
        let (mut d, p) = init::empty_scene();
        let m = p.cid.model;
        for n in ["s-b.1-0", "s-a_1.1-0", "s-a1.1-0", "s-B.1-0", "s-a.2-0", "s-a.2-0", "s-gone.1-0"] {
            init::add_named(&mut d, &p, m, n.as_bytes());
        }
        // a freed slot goes last; equal names: roots first
        d.ed.lists[m as usize].elems[6].rec.status = ELEM_DELETED;
        d.ed.lists[m as usize].elems[4].rec.hier_level = 1;
        let o = sort_chapter_elements(&d, m, key::FULL_NAME, true, false, NameOrder::Linux, (p.cid.rim, p.cid.img), None).unwrap();
        assert_eq!(o, [3, 5, 4, 1, 2, 0, 6]);
        assert_eq!(names(&d, m, &o)[..4], ["s-B.1-0", "s-a.2-0", "s-a.2-0", "s-a_1.1-0"]);
        // the NT build ignores case: B goes with b
        let o = sort_chapter_elements(&d, m, key::FULL_NAME, true, false, NameOrder::Nt, (p.cid.rim, p.cid.img), None).unwrap();
        assert_eq!(o, [5, 4, 1, 2, 0, 3, 6]);
        // descending: the freed slot still goes last
        let o = sort_chapter_elements(&d, m, key::FULL_NAME, false, false, NameOrder::Linux, (p.cid.rim, p.cid.img), None).unwrap();
        assert_eq!(o, [0, 2, 1, 4, 5, 3, 6]);
    }

    #[test]
    fn pictures_sort_by_name_then_frame() {
        let (mut d, p) = init::empty_scene();
        let c = p.cid.img;
        for n in ["walk.10", "walk.9", "run.2", "walk"] {
            init::add_named(&mut d, &p, c, n.as_bytes());
        }
        let o = sort_chapter_elements(&d, c, key::FULL_NAME, true, false, NameOrder::Linux, (p.cid.rim, p.cid.img), None).unwrap();
        assert_eq!(names(&d, c, &o), ["run.2", "walk", "walk.9", "walk.10"]);
    }

}
