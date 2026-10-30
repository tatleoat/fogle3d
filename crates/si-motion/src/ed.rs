// SPDX-License-Identifier: MIT
//! The element dictionary look-ups the animation code uses (`elemdict.c`, `edanim.c`,
//! `edtools.c`): an element by index, a related element by relation id, the function curve
//! of a parameter. Layouts in finding 0010.
//!
//! How a curve hangs on an element (read from these functions and from `toposort.c`): a
//! function curve is itself an element, of the ANIMATION chapter, whose object is the
//! `fcurve` record. The animated element has a **forward** relation to it, in the relation
//! list ("group") of the parameter and with the parameter's relation id: the ids of the
//! attribute lines of `Relations.rsrc` (100 Scaling, 101 Rotation, 113 ExplTrans, 1131
//! CamFov …, finding 0006 section 7). Which component a curve is (x, y or z) is told by the
//! curve's own type (`fcurve` + 0), not by the relation.

use crate::glob;
use si_mem::{Addr, Mem};
use crate::off::{arel, elem, list, rellist};

crate::host_trait! {
    /// What these look-ups call that is not ported.
    pub trait EdHost {
        /// `SI_elemdictFindElemByData(dict, chapter, object)` (elemdict.c): index of the
        /// element whose object is `data`, −1 if none. (A linear search, or a bisection in
        /// a table sorted by pointer when `SI_LOAD_OPTIM` is set; finding 0010.)
        fn elemdict_find_elem_by_data(dict: Addr, chap: i32, data: Addr) -> i32;
        /// `SI_elemdictFindSon(dict, elem)` (elemdict.c): the first son of a model element.
        fn elemdict_find_son(dict: Addr, elem: Addr) -> Addr;
        /// `SI_elemdictFindNextSon(dict, elem)`: the next son, null at the end.
        fn elemdict_find_next_son(dict: Addr, elem: Addr) -> Addr;
    }
}

/// The element `index` of chapter `chap`, null when the index is out of range or the slot
/// is deleted [soft 0x08132b50 SI_elemdictGetElem].
pub fn get_elem(m: &Mem, dict: Addr, chap: i32, index: i32) -> Addr {
    if index < 0 {
        return 0;
    }
    let l = m.ptr(dict + 4).wrapping_add((chap as u32).wrapping_mul(list::SIZE));
    if index < m.i32(l + list::NB_ELEMS) {
        let e = m.ptr(l + list::ELEMS).wrapping_add((index as u32).wrapping_mul(elem::SIZE));
        if m.i16(e + elem::STATUS) == 2 {
            return 0;
        }
        return e;
    }
    0
}

/// The relation list `group` of an element, forward or backward.
pub fn rel_list(m: &Mem, e: Addr, forward: bool, group: i32) -> Addr {
    let rels = if forward { e + elem::FORWARD } else { e + elem::BACKWARD };
    m.ptr(rels + 4).wrapping_add((group as u32).wrapping_mul(rellist::SIZE))
}

/// The relation entry the last look-up in a list stopped on
/// [soft 0x08134db0 SI_elemdictGetCurRelation]: entry `cursor − 1`, entry 0 when the cursor
/// is 0. Null only for a null element.
pub fn get_cur_relation(m: &Mem, e: Addr, forward: bool, group: i32) -> Addr {
    if e == 0 {
        return 0;
    }
    let l = rel_list(m, e, forward, group);
    let mut i = m.i32(l + rellist::CUR) - 1;
    if i < 0 {
        i = 0;
    }
    m.ptr(l + rellist::RELS).wrapping_add((i as u32).wrapping_mul(arel::SIZE))
}

/// What a look-up found: the element, and the two values the original writes through its
/// last two pointer arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Related {
    pub elem: Addr,
    /// The chapter of the element found (the plain look-ups) or the relation id (the range
    /// look-ups). Unchanged from the caller's variable when nothing is found; here 0.
    pub first: i32,
    /// The element's index in its chapter.
    pub index: i32,
}

const NONE: Related = Related { elem: 0, first: 0, index: 0 };

/// The first element related to `e` by relation `relid` in list `group`, in chapter `chap`
/// or in any chapter when `chap` is negative [soft 0x0813508c SI_elemdictFindRelatedElem].
/// The list's cursor is left behind the entry found (0 when none), and the entry's word at
/// +0x16 is set to 1.
pub fn find_related_elem(m: &mut Mem, dict: Addr, e: Addr, forward: bool, group: i32, relid: i32, chap: i32) -> Related {
    if e == 0 {
        return NONE;
    }
    let rels = if forward { e + elem::FORWARD } else { e + elem::BACKWARD };
    if m.ptr(rels + 4) == 0 {
        return NONE;
    }
    let l = rel_list(m, e, forward, group);
    let mut i = 0;
    while i < m.i32(l + rellist::NB) {
        let r = m.ptr(l + rellist::RELS).wrapping_add((i as u32).wrapping_mul(arel::SIZE));
        let rc = m.i16(r + arel::CHAPID) as i32;
        if m.i32(r + arel::RELID) == relid && (rc == chap || chap < 0) {
            let found = get_elem(m, dict, rc, m.i32(r + arel::ELEMID));
            if found != 0 && m.i16(found + elem::STATUS) != 2 {
                m.set_i32(l + rellist::CUR, i + 1);
                m.set_i16(r + 0x16, 1);
                return Related { elem: found, first: rc, index: m.i32(r + arel::ELEMID) };
            }
        }
        i += 1;
    }
    m.set_i32(l + rellist::CUR, 0);
    NONE
}

/// The next one after the list's cursor [soft 0x08135154 SI_elemdictFindNextRelatedElem].
/// The entry before the cursor loses its mark at +0x16 first.
///
/// Unlike the first look-up this one does not test the element for null before reading its
/// status: a relation to a deleted or missing element makes the original read near address
/// 0x48. Here that read gives 0 (and a fault in the image) and a null element is returned
/// with the cursor advanced.
pub fn find_next_related_elem(m: &mut Mem, dict: Addr, e: Addr, forward: bool, group: i32, relid: i32, chap: i32) -> Related {
    if e == 0 {
        return NONE;
    }
    let l = rel_list(m, e, forward, group);
    let prev = m.i32(l + rellist::CUR) - 1;
    if prev > -1 && prev < m.i32(l + rellist::NB) {
        let r = m.ptr(l + rellist::RELS).wrapping_add((prev as u32).wrapping_mul(arel::SIZE));
        m.set_i16(r + 0x16, 0);
    }
    let mut i = m.i32(l + rellist::CUR);
    while i < m.i32(l + rellist::NB) {
        let r = m.ptr(l + rellist::RELS).wrapping_add((i as u32).wrapping_mul(arel::SIZE));
        let rc = m.i16(r + arel::CHAPID) as i32;
        if m.i32(r + arel::RELID) == relid && (rc == chap || chap < 0) {
            let found = get_elem(m, dict, rc, m.i32(r + arel::ELEMID));
            if m.i16(found.wrapping_add(elem::STATUS)) != 2 {
                m.set_i32(l + rellist::CUR, i + 1);
                m.set_i16(r + 0x16, 1);
                return Related { elem: found, first: rc, index: m.i32(r + arel::ELEMID) };
            }
        }
        i += 1;
    }
    m.set_i32(l + rellist::CUR, 0);
    NONE
}

/// The first element related by a relation whose id lies in `lo ..= hi`
/// [soft 0x081353c4 SI_elemdictFindRangeRelatedElem]. `first` of the result is the relation
/// id found. No null test on the element (see [`find_next_related_elem`]).
#[allow(clippy::too_many_arguments)]
pub fn find_range_related_elem(m: &mut Mem, dict: Addr, e: Addr, forward: bool, group: i32, lo: i32, hi: i32, chap: i32)
                               -> Related {
    if e == 0 {
        return NONE;
    }
    let rels = if forward { e + elem::FORWARD } else { e + elem::BACKWARD };
    if m.ptr(rels + 4) == 0 {
        return NONE;
    }
    let l = rel_list(m, e, forward, group);
    range_scan(m, dict, l, 0, lo, hi, chap)
}

/// The next one after the cursor [soft 0x08135484 SI_elemdictFindRangeNextRelatedElem].
#[allow(clippy::too_many_arguments)]
pub fn find_range_next_related_elem(m: &mut Mem, dict: Addr, e: Addr, forward: bool, group: i32, lo: i32, hi: i32,
                                    chap: i32)
                                    -> Related {
    if e == 0 {
        return NONE;
    }
    let l = rel_list(m, e, forward, group);
    let prev = m.i32(l + rellist::CUR) - 1;
    if prev > -1 && prev < m.i32(l + rellist::NB) {
        let r = m.ptr(l + rellist::RELS).wrapping_add((prev as u32).wrapping_mul(arel::SIZE));
        m.set_i16(r + 0x16, 0);
    }
    let from = m.i32(l + rellist::CUR);
    range_scan(m, dict, l, from, lo, hi, chap)
}

fn range_scan(m: &mut Mem, dict: Addr, l: Addr, from: i32, lo: i32, hi: i32, chap: i32) -> Related {
    let mut i = from;
    while i < m.i32(l + rellist::NB) {
        let r = m.ptr(l + rellist::RELS).wrapping_add((i as u32).wrapping_mul(arel::SIZE));
        let id = m.i32(r + arel::RELID);
        let rc = m.i16(r + arel::CHAPID) as i32;
        if lo <= id && id <= hi && (rc == chap || chap < 0) {
            let found = get_elem(m, dict, rc, m.i32(r + arel::ELEMID));
            if m.i16(found.wrapping_add(elem::STATUS)) != 2 {
                m.set_i32(l + rellist::CUR, i + 1);
                m.set_i16(r + 0x16, 1);
                return Related { elem: found, first: id, index: m.i32(r + arel::ELEMID) };
            }
        }
        i += 1;
    }
    m.set_i32(l + rellist::CUR, 0);
    NONE
}

fn data_of(m: &Mem, e: Addr) -> Addr {
    if e == 0 {
        0
    } else {
        m.ptr(e + elem::DATA)
    }
}

/// The curve of parameter `relid` in list `group` of element `e`, null if it has none
/// [soft 0x080edb20 SI_edFindFcv]: the object of the first ANIMATION element the element is
/// related to by that id.
pub fn find_fcv(m: &mut Mem, e: Addr, group: i32, relid: i32) -> Addr {
    let dict = glob::cur_dict(m);
    let ani = glob::chap(m, glob::cid::ANIMATION);
    let r = find_related_elem(m, dict, e, true, group, relid, ani);
    data_of(m, r.elem)
}

/// The next curve of the same parameter (its other components)
/// [soft 0x080edb74 SI_edFindNextFcv].
pub fn find_next_fcv(m: &mut Mem, e: Addr, group: i32, relid: i32) -> Addr {
    let dict = glob::cur_dict(m);
    let ani = glob::chap(m, glob::cid::ANIMATION);
    let r = find_next_related_elem(m, dict, e, true, group, relid, ani);
    data_of(m, r.elem)
}

/// True for the relation ids 15000 to 15010: the links from an element to its animation
/// *file* element (`Anim`, `WavAnim` … `ShpAnim`), which the range look-ups step over.
fn is_anim_file_relation(id: i32) -> bool {
    (id.wrapping_sub(15000) as u32) < 0xb
}

/// The first curve whose relation id lies in `lo ..= hi`
/// [soft 0x080edbc8 SI_edFindRangeFcv]. One animation-file relation (15000 … 15010) is
/// stepped over; a second one in a row would be returned as if it were a curve.
pub fn find_range_fcv(m: &mut Mem, e: Addr, group: i32, lo: i32, hi: i32) -> Addr {
    let dict = glob::cur_dict(m);
    let ani = glob::chap(m, glob::cid::ANIMATION);
    let mut r = find_range_related_elem(m, dict, e, true, group, lo, hi, ani);
    if r.elem != 0 && is_anim_file_relation(r.first) {
        r = find_range_next_related_elem(m, dict, e, true, group, lo, hi, ani);
    }
    data_of(m, r.elem)
}

/// The next curve of the range [soft 0x080edc74 SI_edFindRangeNextFcv].
pub fn find_range_next_fcv(m: &mut Mem, e: Addr, group: i32, lo: i32, hi: i32) -> Addr {
    let dict = glob::cur_dict(m);
    let ani = glob::chap(m, glob::cid::ANIMATION);
    let mut r = find_range_next_related_elem(m, dict, e, true, group, lo, hi, ani);
    if r.elem != 0 && is_anim_file_relation(r.first) {
        r = find_range_next_related_elem(m, dict, e, true, group, lo, hi, ani);
    }
    data_of(m, r.elem)
}

/// Up to `max` curves of one parameter, in list order [soft 0x080edd20 SI_edFindFcvs]. The
/// original fills the caller's array and returns the count; with no curve it returns 0.
pub fn find_fcvs(m: &mut Mem, e: Addr, group: i32, relid: i32, max: i32) -> Vec<Addr> {
    let mut out = Vec::new();
    let mut f = find_fcv(m, e, group, relid);
    if f == 0 {
        return out;
    }
    let mut i = 0;
    while i < max {
        out.push(f);
        f = find_next_fcv(m, e, group, relid);
        if f == 0 {
            return out;
        }
        i += 1;
    }
    out
}

/// The activation curve of a relation entry, null if it has none
/// [soft 0x080ee6f0 SI_edGetFcvFromRelation]: the object of ANIMATION element
/// `entry.active_elem` (+8) when that index is not negative. The element array is indexed
/// directly: no range or deletion test.
pub fn get_fcv_from_relation(m: &Mem, rel: Addr) -> Addr {
    let i = m.i32(rel + arel::ACTIVE_ELEM);
    if i < 0 {
        return 0;
    }
    let dict = glob::cur_dict(m);
    let ani = glob::chap(m, glob::cid::ANIMATION);
    let l = m.ptr(dict + 4).wrapping_add((ani as u32).wrapping_mul(list::SIZE));
    m.ptr(m.ptr(l + list::ELEMS).wrapping_add((i as u32).wrapping_mul(elem::SIZE)) + elem::DATA)
}

/// The element of a model node and its index in the MODELS chapter
/// [soft 0x080fcf20 SI_edGetElemFromModel]. The node caches the index at +0x354; a negative
/// cache is refreshed by a search for the node among the chapter's objects.
pub fn get_elem_from_model<H: EdHost + ?Sized>(m: &mut Mem, h: &mut H, model: Addr) -> (Addr, i32) {
    let dict = glob::cur_dict(m);
    let models = glob::chap(m, glob::cid::MODELS);
    if m.i32(model + crate::off::model::ELEM_ID) < 0 {
        let i = h.elemdict_find_elem_by_data(m, dict, models, model);
        m.set_i32(model + crate::off::model::ELEM_ID, i);
    }
    let id = m.i32(model + crate::off::model::ELEM_ID);
    if id < 0 {
        (0, id)
    } else {
        (get_elem(m, dict, models, id), id)
    }
}

/// The first son of a model element [soft 0x080fcec0 SI_edFindSon].
pub fn find_son<H: EdHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr) -> Addr {
    let dict = glob::cur_dict(m);
    h.elemdict_find_son(m, dict, e)
}

/// The next son [soft 0x080fcef0 SI_edFindNextSon].
pub fn find_next_son<H: EdHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr) -> Addr {
    let dict = glob::cur_dict(m);
    h.elemdict_find_next_son(m, dict, e)
}

/// Test support: a small scene dictionary in the image.
pub mod build {
    use super::*;
    use crate::off::dict;

    /// Maps the globals ([`glob::init_defaults`]) and one dictionary with the 36 standard
    /// chapters, all empty; returns the dictionary's address.
    pub fn scene(m: &mut Mem) -> Addr {
        glob::init_defaults(m);
        let d = m.si_calloc(1, dict::SIZE);
        let lists = m.si_calloc(36, list::SIZE);
        m.set_i32(d + dict::NB_LISTS, 36);
        m.set_ptr(d + dict::LISTS, lists);
        for c in 0..36 {
            m.set_i16(lists + c * list::SIZE + list::KIND, 1);
        }
        m.set_i32(d + dict::UPD_LIST + 8, 1);
        m.set_i32(d + dict::TOPO_LIST + 8, 1);
        m.set_ptr(glob::SI_ELEM_DICTS, d);
        m.set_i32(glob::SI_NB_ELEM_DICTS, 1);
        d
    }

    /// Adds an element with object `data` to chapter `chap` (a chapter index), with six
    /// empty relation lists in each direction; returns (element address, index). Element
    /// addresses of the chapter change, as they do in the original when the array grows.
    pub fn add_elem(m: &mut Mem, d: Addr, chap: i32, data: Addr) -> (Addr, i32) {
        let l = m.ptr(d + dict::LISTS) + chap as u32 * list::SIZE;
        let n = m.i32(l + list::NB_ELEMS);
        let old = m.ptr(l + list::ELEMS);
        let arr = if old == 0 { m.si_calloc(1, elem::SIZE) } else { m.si_realloc(old, (n as u32 + 1) * elem::SIZE) };
        m.set_ptr(l + list::ELEMS, arr);
        m.set_i32(l + list::NB_ELEMS, n + 1);
        let e = arr + n as u32 * elem::SIZE;
        m.set_bytes(e, &[0; elem::SIZE as usize]);
        m.set_ptr(e + elem::DATA, data);
        m.set_i32(e + elem::ROOT_ID, n);
        for dir in [elem::FORWARD, elem::BACKWARD] {
            let lists = m.si_calloc(6, rellist::SIZE);
            for g in 0..6 {
                m.set_i16(lists + g * rellist::SIZE + rellist::FLAG, 1);
            }
            m.set_i32(e + dir, 6);
            m.set_ptr(e + dir + 4, lists);
        }
        (e, n)
    }

    fn push(m: &mut Mem, l: Addr, relid: i32, chap: i32, index: i32, group: i32, rank: i32, data: Addr) -> Addr {
        let n = m.i32(l + rellist::NB);
        let old = m.ptr(l + rellist::RELS);
        // kept in rank order, a new entry after the entries of the same or lower rank
        let arr = if old == 0 { m.si_calloc(1, arel::SIZE) } else { m.si_realloc(old, (n as u32 + 1) * arel::SIZE) };
        let mut at = n;
        while at > 0 && m.i32(arr + (at as u32 - 1) * arel::SIZE + arel::RANK) > rank {
            let b = m.bytes(arr + (at as u32 - 1) * arel::SIZE, arel::SIZE).to_vec();
            m.set_bytes(arr + at as u32 * arel::SIZE, &b);
            at -= 1;
        }
        let r = arr + at as u32 * arel::SIZE;
        m.set_bytes(r, &[0; arel::SIZE as usize]);
        m.set_i32(r + arel::RELID, relid);
        m.set_i32(r + arel::ELEMID, index);
        m.set_i32(r + arel::ACTIVE_ELEM, -1);
        m.set_i32(r + arel::SERIAL, -1);
        m.set_i16(r + arel::CHAPID, chap as i16);
        m.set_i16(r + arel::GROUP, group as i16);
        m.set_i16(r + arel::ACTIVE, 1);
        m.set_ptr(r + arel::DATA, data);
        m.set_i32(r + arel::RANK, rank);
        m.set_ptr(l + rellist::RELS, arr);
        m.set_i32(l + rellist::NB, n + 1);
        m.set_i32(l + rellist::CAPACITY, n + 1);
        r
    }

    /// Relates element (`chap`, `index`) as source to (`chap2`, `index2`) as destination:
    /// an entry in the source's forward list `group` and one in the destination's backward
    /// list `group2`, as `SI_elemdictAddCompleteRelation` leaves them.
    #[allow(clippy::too_many_arguments)]
    pub fn relate(m: &mut Mem, d: Addr, chap: i32, index: i32, group: i32, relid: i32, rank: i32, chap2: i32, index2: i32,
                  group2: i32, data: Addr) {
        let src = get_elem(m, d, chap, index);
        let dst = get_elem(m, d, chap2, index2);
        let fl = rel_list(m, src, true, group);
        push(m, fl, relid, chap2, index2, group2, rank, data);
        let bl = rel_list(m, dst, false, group2);
        push(m, bl, relid, chap, index, group, rank, 0);
    }

    /// Adds a curve as an element of the ANIMATION chapter and relates element (`chap`,
    /// `index`) to it by `relid` in list `group`; returns the curve element's index.
    pub fn attach_curve(m: &mut Mem, d: Addr, chap: i32, index: i32, group: i32, relid: i32, rank: i32, curve: Addr) -> i32 {
        let ani = glob::chap(m, glob::cid::ANIMATION);
        let (_, ci) = add_elem(m, d, ani, curve);
        relate(m, d, chap, index, group, relid, rank, ani, ci, 0, 0);
        ci
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcv;

    #[test]
    fn curves_of_an_element() {
        let mut m = Mem::new();
        let d = build::scene(&mut m);
        let models = glob::chap(&m, glob::cid::MODELS);
        let (_, mi) = build::add_elem(&mut m, d, models, 0x5000);
        let cx = fcv::make(&mut m, 1, 1, &[(0.0, 1.0)]);
        let cy = fcv::make(&mut m, 2, 1, &[(0.0, 2.0)]);
        let cr = fcv::make(&mut m, 4, 1, &[(0.0, 3.0)]);
        build::attach_curve(&mut m, d, models, mi, 1, 100, 16, cx);
        build::attach_curve(&mut m, d, models, mi, 1, 100, 16, cy);
        build::attach_curve(&mut m, d, models, mi, 1, 101, 17, cr);
        let e = get_elem(&m, d, models, mi);
        assert_eq!(find_fcv(&mut m, e, 1, 100), cx);
        assert_eq!(find_next_fcv(&mut m, e, 1, 100), cy);
        assert_eq!(find_next_fcv(&mut m, e, 1, 100), 0);
        assert_eq!(find_fcv(&mut m, e, 1, 101), cr);
        assert_eq!(find_fcv(&mut m, e, 0, 100), 0);
        assert_eq!(find_fcvs(&mut m, e, 1, 100, 3), vec![cx, cy]);
        assert_eq!(find_range_fcv(&mut m, e, 1, 0, 14999), cx);
        assert_eq!(find_range_next_fcv(&mut m, e, 1, 0, 14999), cy);
        assert_eq!(find_range_next_fcv(&mut m, e, 1, 0, 14999), cr);
        assert_eq!(find_range_next_fcv(&mut m, e, 1, 0, 14999), 0);
        assert!(m.fault().is_none());
    }

    #[test]
    fn deleted_elements_are_not_found() {
        let mut m = Mem::new();
        let d = build::scene(&mut m);
        let models = glob::chap(&m, glob::cid::MODELS);
        let (e, i) = build::add_elem(&mut m, d, models, 0x5000);
        assert_eq!(get_elem(&m, d, models, i), e);
        m.set_i16(e + elem::STATUS, 2);
        assert_eq!(get_elem(&m, d, models, i), 0);
        assert_eq!(get_elem(&m, d, models, 1), 0);
        assert_eq!(get_elem(&m, d, models, -1), 0);
    }
}
