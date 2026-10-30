// SPDX-License-Identifier: MIT
//! `elemdict.c`: dictionaries, chapters and elements, on the image.
//!
//! - A dictionary (`SI_ElemDict`, 0x70 bytes) owns an array of chapters (`SI_ElemList`,
//!   0x40 bytes each), grown by one chapter per addition: `SI_malloc(0x40)`, then
//!   `SI_realloc(lists, (n + 1) * 0x40)`.
//! - A chapter owns an array of elements (`__SI_Elem`, 0x84 bytes), grown by one element
//!   when no deleted slot is reused: `SI_malloc(0x84)`, then
//!   `SI_realloc(elems, (n + 1) * 0x84)`. No spare room is kept anywhere.
//! - With `SI_LOAD_OPTIM` a chapter also has a sort table (`SI_calloc(1, 12)`) whose index
//!   array grows beside the elements, one word each, the new word −1.
//! - The global array of dictionaries (`SI_elemDicts`) and its flags (`elemDictExists`)
//!   grow the same way.
//!
//! The relation routines of the file are in [`crate::elemrel`], the names in
//! [`crate::elemname`], the rest in [`crate::elemmisc`]. The element sort
//! (`elemdictCompareElems (0x08131d7c)`, `SI_elemlistSortChapterElements (0x08132330)`) and
//! the two dumps are not on the image yet (finding 0026, section 8).

use crate::globals::*;
use crate::libc::{strcasecmp, strcmp};
use crate::outside::{self, Outside};
use crate::relations;
use crate::rodata::CHAP_INFO;
use crate::updlist;
use si_mem::{field, CStr32, FnPtr, Mem, Opaque, Ptr32};
use si_types::dict::{DbListNode, Elem, ElemDict, ElemList, Relations, SortTbl};

/// Chapter `chap` of a dictionary: `ed->lists + chap`.
pub(crate) fn list_of(m: &Mem, ed: Ptr32<ElemDict>, chap: i32) -> Ptr32<ElemList> {
    m[ed].lists.add(chap)
}

/// What every routine that adds or removes a relation stores in the dictionary: both
/// update lists dirty, the relation flag set.
pub(crate) fn mark_relations_changed(m: &mut Mem, ed: Ptr32<ElemDict>) {
    m[ed].upd_list.dirty = 1;
    m[ed].rel_dirty = 1;
    m[ed].topo_list.dirty = 1;
}

/// The 16-bit word at +0x36 of `SI_glbManip`.
pub(crate) fn glb_manip_36(m: &Mem) -> i16 {
    m.i16(SI_GLB_MANIP.addr() + GLB_MANIP_36)
}

/// [soft 0x08131980 filenameCheckSuffix]: the position of the last `.` of a name that no
/// `-` follows, looking back from the end and not at the first character; −1 without one.
pub fn filename_check_suffix(m: &Mem, name: CStr32) -> i32 {
    let mut i = m.strlen(name.addr()) as i32;
    while 0 < i && m.u8(name.addr() + i as u32) != b'-' {
        if m.u8(name.addr() + i as u32) == b'.' {
            return i;
        }
        i -= 1;
    }
    -1
}

/// The C library's `isxdigit` in the C locale: the class bit (0x1000 of the 16-bit entry
/// of `__ctype_b`) that `strcmp_underscore` tests.
fn class_bit_0x1000(c: u8) -> bool {
    c.is_ascii_hexdigit()
}

/// [soft 0x08131ce4 strcmp_underscore]: a string comparison in which, at the first
/// difference, an underscore facing a character of the class above sorts first. Equal
/// strings, and a first string that is longer than the second with the second used up,
/// follow the code as it stands: 0 when the second ends where the strings stop agreeing,
/// −1 when only the first ends.
pub fn strcmp_underscore(m: &Mem, a: CStr32, b: CStr32) -> i32 {
    let (mut a, mut b) = (a.addr(), b.addr());
    let mut differ = false;
    while m.u8(a) != 0 {
        let c = m.u8(b);
        if c == 0 || m.u8(a) != c {
            differ = true;
            break;
        }
        a += 1;
        b += 1;
    }
    let c = m.u8(b);
    if !differ {
        if c == 0 {
            return 0;
        }
        if m.u8(a) == 0 {
            return -1;
        }
    }
    let c1 = m.u8(a);
    if c != 0 {
        if c1 == b'_' && class_bit_0x1000(c) {
            return -1;
        }
        if c != b'_' || !class_bit_0x1000(c1) {
            return c1 as i8 as i32 - c as i8 as i32;
        }
    }
    1
}

/// [soft 0x08131d60 SI_compareElemName]
pub fn compare_elem_name(m: &Mem, a: CStr32, b: CStr32) -> i32 {
    strcmp_underscore(m, a, b)
}

/// [soft 0x08132564 elemdictCompareChapNames]: the comparison of
/// [`sort_chap_names`]: two chapter indices by the names of the chapters of `glbED`.
pub fn compare_chap_names(m: &Mem, a: Ptr32<i32>, b: Ptr32<i32>) -> i32 {
    let ed = m[GLB_ED];
    strcmp(m, m[list_of(m, ed, m[a])].name.addr(), m[list_of(m, ed, m[b])].name.addr())
}

/// [soft 0x0813259c SI_elemdictSortChapNames]: a new array (`calloc(n, 4)`) of the
/// chapter indices in the order of the chapters' names. The array the dictionary had is
/// not freed; a dictionary without chapters keeps what it has.
pub fn sort_chap_names(m: &mut Mem, ed: Ptr32<ElemDict>) {
    let n = m[ed].nb_lists;
    if n != 0 {
        let p: Ptr32<i32> = Ptr32::new(m.calloc(n as u32, 4));
        m[ed].chap_sort = p;
        let mut i = 0;
        while i < m[ed].nb_lists {
            let slot = m[ed].chap_sort.add(i);
            m[slot] = i;
            i += 1;
        }
        if 1 < m[ed].nb_lists {
            m[GLB_ED] = ed;
            let base = m[ed].chap_sort.addr();
            let n = m[ed].nb_lists as u32;
            crate::libc::qsort(m, base, n, 4, &mut |m, a, b| compare_chap_names(m, Ptr32::new(a), Ptr32::new(b)));
        }
    }
}

/// [soft 0x081325f4 SI_elemdictFindListByName]: the index of the chapter called `name`,
/// or −1. The names PICTURES and RENDER_PICTURES (the chapter table's rows 4 and 11) are
/// also found whatever their letter case.
pub fn find_list_by_name(m: &Mem, ed: Ptr32<ElemDict>, name: CStr32) -> i32 {
    let mut i = 0;
    while i < m[ed].nb_lists {
        let chapter = m[list_of(m, ed, i)].name.addr();
        if strcmp(m, chapter, name.addr()) == 0 {
            return i;
        }
        let row: Ptr32<si_types::dict::ChapInfo> = SI_CHAP_INFO.cast();
        if strcasecmp(m, m[row.add(4)].name.addr(), name.addr()) == 0 && strcasecmp(m, chapter, name.addr()) == 0 {
            return i;
        }
        if strcasecmp(m, m[row.add(11)].name.addr(), name.addr()) == 0 && strcasecmp(m, chapter, name.addr()) == 0 {
            return i;
        }
        i += 1;
    }
    -1
}

/// The stores [`init_elem`] and [`elemlist_add_elem`] share: schematic position, colour,
/// display, marks. `SI_grafnodeDefColor (0x0832bff4)` is asked for the wire colour.
fn default_elem_fields(m: &mut Mem, o: &mut dyn Outside, e: Ptr32<Elem>) {
    m[e].schem_x = 0.0;
    m[e].schem_y = 0.0;
    m[e].schem_width = 0.0;
    m[e].schem_collapse = 0;
    let color = outside::si_grafnode_def_color(o, m);
    m[e].schem_bbox = [0.0; 4];
    m[e].wire_color = color;
    m[e].wire_type = 2;
    m[e].selection = 0;
    m[e].dope_state = 0;
    m[e].schem_flags = 0;
    m[e].pad_2b = 0;
    m[e].root_srt = Ptr32::NULL;
    m[e].dirty = 1;
    m[e].selected = 0;
    m[e].new_flag = 2;
    m[e].status = 0;
    m[e].shared = 0;
}

/// [soft 0x081326b0 SI_elemdictInitElem]: every field of an element at its default; the
/// relation lists are null (not allocated).
pub fn init_elem(m: &mut Mem, o: &mut dyn Outside, e: Ptr32<Elem>) {
    m[e].node = Ptr32::NULL;
    m[e].data = Ptr32::NULL;
    m[e].aux_data = Ptr32::NULL;
    default_elem_fields(m, o, e);
    m[e].dsc_id = 0;
    m[e].root_id = 0;
    m[e].hier_level = 0;
    m[e].work_flag = 0;
    m[e].work_id = 0;
    m[e].subst = 0;
    m[e].icon = Ptr32::NULL;
    m[e].seq_first = 0;
    m[e].seq_last = 0;
    m[e].seq_unknown = 0;
    m[e].seq_braces = 0;
    m[e].suffix = Ptr32::NULL;
    m[e].backward.nb_groups = 0;
    m[e].forward.nb_groups = 0;
    m[e].backward.groups = Ptr32::NULL;
    m[e].forward.groups = Ptr32::NULL;
}

/// [soft 0x081327c4 SI_elemlistAddElem]: puts an element into a chapter and answers its
/// index. With `reuse` the first deleted slot (status 2) is taken; otherwise, or without
/// one, the array grows by one element. `SI_getLoadOptim (0x080d94b0)` decides whether
/// the chapter keeps a sort table: it is made on first use, marked stale on every call,
/// and its index array grows with the elements.
///
/// A new slot gets two sets of six relation lists when `with_rels` is set; a reused slot
/// with `with_rels` keeps the lists it has; in every other case the four relation words
/// are cleared. The three sequence fields (+0x74 to +0x7f) are not written.
#[allow(clippy::too_many_arguments)]
pub fn elemlist_add_elem(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<ElemList>, node: Ptr32<DbListNode>, data: Ptr32<Opaque>, aux: Ptr32<Opaque>, dsc_id: i32, work_id: i32, with_rels: i16, reuse: i16) -> i32 {
    let mut index = m[list].nb_elems;
    // The original keeps the table in a register it sets only on this path; without the
    // optimisation it is never read.
    let mut tbl: Ptr32<SortTbl> = Ptr32::NULL;
    if outside::si_get_load_optim(o, m) != 0 {
        tbl = m[list].sort_tbl;
        if tbl.is_null() {
            tbl = Ptr32::new(m.si_calloc(1, 0xc));
            m[list].sort_tbl = tbl;
            m[tbl].table = Ptr32::NULL;
            m[tbl].valid = 0;
        } else {
            m[tbl].valid = 0;
        }
    }
    let mut reused = false;
    if reuse != 0 {
        let mut i = 0;
        while i < m[list].nb_elems {
            if m[m[list].elems.add(i)].status == 2 {
                reused = true;
                index = i;
                break;
            }
            i += 1;
        }
    }
    if !reused {
        if m[list].elems.is_null() {
            m[list].elems = Ptr32::new(m.si_malloc(0x84));
            if outside::si_get_load_optim(o, m) != 0 {
                let p: Ptr32<i32> = Ptr32::new(m.si_malloc(4));
                m[tbl].table = p;
                m[p] = -1;
            }
        } else {
            let old = m[list].elems.addr();
            m[list].elems = Ptr32::new(m.si_realloc(old, ((index + 1) * 0x84) as u32));
            if outside::si_get_load_optim(o, m) != 0 {
                let old = m[tbl].table.addr();
                let p: Ptr32<i32> = Ptr32::new(m.si_realloc(old, ((index + 1) * 4) as u32));
                m[tbl].table = p;
                m[p.add(index)] = -1;
            }
        }
    }
    let e = m[list].elems.add(index);
    m[e].node = node;
    m[e].data = data;
    m[e].aux_data = aux;
    default_elem_fields(m, o, e);
    m[e].dsc_id = dsc_id;
    m[e].root_id = index;
    m[e].hier_level = 0;
    m[e].work_flag = 0;
    m[e].work_id = work_id;
    m[e].subst = 0;
    m[e].icon = Ptr32::NULL;
    m[e].suffix = Ptr32::NULL;
    if !reused && with_rels != 0 {
        relations::rel_allocate(m, field!(e, Elem, forward));
        relations::rel_allocate(m, field!(e, Elem, backward));
    } else if !(reused && with_rels != 0) {
        m[e].backward.nb_groups = 0;
        m[e].forward.nb_groups = 0;
        m[e].backward.groups = Ptr32::NULL;
        m[e].forward.groups = Ptr32::NULL;
    }
    if !reused {
        m[list].nb_elems += 1;
    }
    index
}

/// [soft 0x08132a5c SI_elemdictAddElem]: adds an element to chapter `chap` with relation
/// lists and slot reuse; both update lists become dirty. For an element of the MODELS
/// chapter that has an object, while the hierarchy flag is clean and the word at +0x36 of
/// `SI_glbManip` is set, the model relations are saved
/// (`SI_edAllocMdlRelBackUp (0x08105cb0)`, `SI_edMdlRelBackUp (0x08105d7c)`); in every
/// other case the hierarchy flag is set.
#[allow(clippy::too_many_arguments)]
pub fn add_elem(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, node: Ptr32<DbListNode>, data: Ptr32<Opaque>, aux: Ptr32<Opaque>, dsc_id: i32, work_id: i32) -> i32 {
    m[ed].upd_list.dirty = 1;
    m[ed].topo_list.dirty = 1;
    let l = list_of(m, ed, chap);
    let index = elemlist_add_elem(m, o, l, node, data, aux, dsc_id, work_id, 1, 1);
    if chap == m[SI_CID].model && m[ed].hier_dirty == 0 && glb_manip_36(m) != 0 {
        let e = get_elem(m, ed, chap, index);
        if !e.is_null() && !m[e].data.is_null() {
            outside::si_ed_alloc_mdl_rel_back_up(o, m, ed);
            outside::si_ed_mdl_rel_back_up(o, m, ed, e, index);
            return index;
        }
    }
    m[ed].hier_dirty = 1;
    index
}

/// [soft 0x08132b04 SI_elemdictGetChapName]
pub fn get_chap_name(m: &Mem, ed: Ptr32<ElemDict>, chap: i32) -> CStr32 {
    m[list_of(m, ed, chap)].name
}

/// [soft 0x08132b1c SI_elemlistGetElem]: element `index` of a chapter; null when the
/// index is outside the chapter or the slot is deleted.
pub fn elemlist_get_elem(m: &Mem, list: Ptr32<ElemList>, index: i32) -> Ptr32<Elem> {
    if index < 0 || m[list].nb_elems <= index {
        return Ptr32::NULL;
    }
    let e = m[list].elems.add(index);
    if m[e].status != 2 {
        e
    } else {
        Ptr32::NULL
    }
}

/// [soft 0x08132b50 SI_elemdictGetElem]: the same by chapter index (which is not checked).
pub fn get_elem(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, index: i32) -> Ptr32<Elem> {
    if -1 < index {
        let l = list_of(m, ed, chap);
        if index < m[l].nb_elems {
            let e = m[l].elems.add(index);
            if m[e].status == 2 {
                return Ptr32::NULL;
            }
            return e;
        }
    }
    Ptr32::NULL
}

/// [soft 0x08132b94 SI_elemdictGetElemId]: the index of an element given its address, or
/// −1 when the address is outside the chapter's array (or a pointer is null, or the
/// chapter index is not below the count). The quotient is the compiler's exact division
/// by 0x84 (a multiplication by the inverse of 33 and a shift), so an address between two
/// elements gives the product's low bits, not a rounded index.
pub fn get_elem_id(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, e: Ptr32<Elem>) -> i32 {
    if !ed.is_null() && !e.is_null() && chap < m[ed].nb_lists {
        let l = list_of(m, ed, chap);
        let first = m[l].elems.addr();
        let last = first.wrapping_add((m[l].nb_elems as u32).wrapping_mul(0x84)).wrapping_sub(0x84);
        if first <= e.addr() && e.addr() <= last {
            return (e.addr().wrapping_sub(first) as i32).wrapping_mul(0x3e0f_83e1) >> 2;
        }
    }
    -1
}

/// [soft 0x08132be8 SI_elemdictSetDirty]
pub fn set_dirty(m: &mut Mem, e: Ptr32<Elem>, dirty: i32) {
    m[e].dirty = dirty as i16;
}

/// [soft 0x08132bfc SI_elemdictSchemSetPosX]
pub fn schem_set_pos_x(m: &mut Mem, e: Ptr32<Elem>, x: f32) {
    m[e].schem_x = x;
}

/// [soft 0x08132c0c SI_elemdictSchemSetPosY]
pub fn schem_set_pos_y(m: &mut Mem, e: Ptr32<Elem>, y: f32) {
    m[e].schem_y = y;
}

/// [soft 0x08132c1c si_elemdictSetFlag]
pub fn set_flag(m: &mut Mem, e: Ptr32<Elem>, flag: i16) {
    m[e].work_flag = flag as i32;
}

/// [soft 0x08132c30 SI_elemlistClearWorkFlag]: clears the work flag of every slot of a
/// chapter.
pub fn elemlist_clear_work_flag(m: &mut Mem, ed: Ptr32<ElemDict>, chap: i32) {
    if !ed.is_null() && chap < m[ed].nb_lists {
        let l = list_of(m, ed, chap);
        let n = m[l].nb_elems;
        let mut i = 0;
        while i < n {
            let e = m[l].elems.add(i);
            m[e].work_flag = 0;
            i += 1;
        }
    }
}

/// [soft 0x08132c70 SI_elemlistClearLowByteWorkFlag]: clears the byte at +0x5b of every
/// slot: the low byte of the work flag on the big-endian machines the code was written
/// for, its high byte in this build.
pub fn elemlist_clear_low_byte_work_flag(m: &mut Mem, ed: Ptr32<ElemDict>, chap: i32) {
    if !ed.is_null() && chap < m[ed].nb_lists {
        let l = list_of(m, ed, chap);
        let n = m[l].nb_elems;
        let mut i = 0;
        while i < n {
            let e = m[l].elems.add(i);
            m.set_u8(e.addr() + 0x5b, 0);
            i += 1;
        }
    }
}

/// [soft 0x08132cac SI_elemlistLowByteWorkFlagAND]: ands the same byte of every slot with
/// the low byte of `mask`.
pub fn elemlist_low_byte_work_flag_and(m: &mut Mem, ed: Ptr32<ElemDict>, chap: i32, mask: i16) {
    if !ed.is_null() && chap < m[ed].nb_lists {
        let l = list_of(m, ed, chap);
        let n = m[l].nb_elems;
        let mut i = 0;
        while i < n {
            let e = m[l].elems.add(i);
            m.and_u8(e.addr() + 0x5b, mask as u8);
            i += 1;
        }
    }
}

/// [soft 0x08133bc8 SI_elemlistInit]: an empty chapter of kind 1. The two bytes at +0x1a
/// are not written.
pub fn elemlist_init(m: &mut Mem, list: Ptr32<ElemList>) {
    m[list].name = Ptr32::NULL;
    m[list].nb_elems = 0;
    m[list].unk_20 = 0;
    m[list].elems = Ptr32::NULL;
    m[list].sort = Ptr32::NULL;
    m[list].hier_order = Ptr32::NULL;
    m[list].hier_count = Ptr32::NULL;
    m[list].kind = 1;
    m[list].unk_1c = 0;
    m[list].unk_24 = 0;
    m[list].subdir = Ptr32::NULL;
    m[list].subdirs = Ptr32::NULL;
    m[list].subdir_aux = Ptr32::NULL;
    m[list].nb_subdirs = 0;
    m[list].suffix = Ptr32::NULL;
    m[list].sort_tbl = Ptr32::NULL;
}

/// [soft 0x08133c40 SI_elemdictAddList]: the index of the chapter called `name`, added at
/// the end when the dictionary has none (the name is stored as given, not copied).
pub fn add_list(m: &mut Mem, ed: Ptr32<ElemDict>, name: CStr32) -> i32 {
    let mut index = find_list_by_name(m, ed, name);
    if index == -1 {
        index = m[ed].nb_lists;
        let old = m[ed].lists;
        let p = if old.is_null() { m.si_malloc(0x40) } else { m.si_realloc(old.addr(), ((index + 1) * 0x40) as u32) };
        m[ed].lists = Ptr32::new(p);
        let l = list_of(m, ed, index);
        elemlist_init(m, l);
        m[l].name = name;
        m[ed].nb_lists += 1;
    }
    index
}

/// [soft 0x08133cc0 SI_elemlistCountValidElems]: how many slots are in use (status 0).
pub fn elemlist_count_valid_elems(m: &Mem, list: Ptr32<ElemList>) -> i32 {
    let mut count = 0;
    let n = m[list].nb_elems;
    let mut i = 0;
    while i < n {
        if m[m[list].elems.add(i)].status == 0 {
            count += 1;
        }
        i += 1;
    }
    count
}

/// [soft 0x081341d4 SI_elemdictFindElemByName]: the first slot whose name record has the
/// full name `fullname` and, when `alias` is given, that alias; −1 without one. Deleted
/// slots have no name record and are passed over; flagged ones are not.
pub fn find_elem_by_name(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, fullname: CStr32, alias: CStr32) -> i32 {
    let l = list_of(m, ed, chap);
    let mut i = 0;
    while i < m[l].nb_elems {
        let node = m[m[l].elems.add(i)].node;
        if !node.is_null() && strcmp(m, m[node].fullname.addr(), fullname.addr()) == 0 {
            if alias.is_null() {
                return i;
            }
            if !m[node].str_0c.is_null() && strcmp(m, m[node].str_0c.addr(), alias.addr()) == 0 {
                return i;
            }
        }
        i += 1;
    }
    -1
}

/// [soft 0x0813425c SI_dbSortTblSetDirty]: marks a chapter's sort table stale, when the
/// chapter has one.
pub fn sort_tbl_set_dirty(m: &mut Mem, list: Ptr32<ElemList>) {
    let tbl = if list.is_null() { Ptr32::NULL } else { m[list].sort_tbl };
    if !tbl.is_null() {
        m[tbl].valid = 0;
    }
}

/// [soft 0x081342ec findElemInChapSortTbl]: bisection in the first `count` entries of a
/// chapter's sort table for the element whose object is `data` (addresses compared
/// unsigned). Found: `*out` is the entry's position, the answer 1. Not found: `*out` is
/// where it would go, the answer 0.
pub fn find_elem_in_chap_sort_tbl(m: &mut Mem, list: Ptr32<ElemList>, data: u32, tbl: Ptr32<SortTbl>, count: i32, out: Ptr32<i32>) -> i16 {
    let mut lo = 0;
    let mut mid = (count + 1) / 2;
    let mut pos = 0;
    let mut hi = count + 1;
    if mid != 0 {
        loop {
            let index = m[m[tbl].table.add(mid - 1)];
            let key = m[m[list].elems.add(index)].data.addr();
            let mut next_hi = mid;
            if key <= data {
                next_hi = hi;
                lo = mid;
                if data == key {
                    m[out] = mid - 1;
                    return 1;
                }
            }
            mid = (next_hi + lo) / 2;
            pos = mid;
            hi = next_hi;
            if mid == lo {
                break;
            }
        }
    }
    m[out] = pos;
    0
}

/// [soft 0x0813427c addElemInChapSortTbl]: puts element `index` (whose object is `data`)
/// into the table unless an element with that object is there already.
pub fn add_elem_in_chap_sort_tbl(m: &mut Mem, index: i32, list: Ptr32<ElemList>, data: u32, tbl: Ptr32<SortTbl>, count: Ptr32<i32>) {
    let pos: Ptr32<i32> = Ptr32::new(m.local(4));
    let n = m[count];
    if find_elem_in_chap_sort_tbl(m, list, data, tbl, n, pos) == 0 {
        let at = m[pos];
        let table = m[tbl].table;
        if at != m[count] {
            let len = (m[count] - at) * 4;
            m.copy(table.add(at + 1).addr(), table.add(at).addr(), len as u32);
        }
        m[table.add(at)] = index;
        m[count] += 1;
    }
    m.release(4);
}

/// [soft 0x08134364 SI_elemdictFindElemByData]: the index of the element in use whose
/// object is `data`, or −1.
///
/// Without a sort table (or with `SI_useElemListSortTbl` clear, or without
/// `SI_LOAD_OPTIM`) the chapter is gone through in order. With a stale table the table is
/// rebuilt from the elements in use; this path answers the index the rebuild met, which
/// is the *last* element with that object. With a valid one, a repeat of the last
/// question is answered from three statics and anything else by bisection.
pub fn find_elem_by_data(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, data: Ptr32<Opaque>) -> i32 {
    let l = list_of(m, ed, chap);
    let tbl = m[l].sort_tbl;
    if tbl.is_null() || m[SI_USE_ELEM_LIST_SORT_TBL] == 0 || outside::si_get_load_optim(o, m) == 0 {
        m[LAST_PEL] = Ptr32::NULL;
        m[LAST_DATA] = 0;
        m[LAST_INDEX_385] = -1;
        let mut i = 0;
        if 0 < m[l].nb_elems {
            let elems = m[l].elems;
            if m[elems].status == 0 && m[elems].data == data {
                m[LAST_INDEX_385] = 0;
            } else {
                loop {
                    i += 1;
                    if m[l].nb_elems <= i {
                        m[LAST_PEL] = Ptr32::NULL;
                        m[LAST_DATA] = 0;
                        m[LAST_INDEX_385] = -1;
                        return -1;
                    }
                    if m[elems.add(i)].status == 0 {
                        m[LAST_INDEX_385] = i;
                        if m[elems.add(i)].data == data {
                            break;
                        }
                    }
                }
            }
        }
    } else if m[tbl].valid == 0 {
        m[LAST_PEL] = Ptr32::NULL;
        m[LAST_DATA] = 0;
        m[LAST_INDEX_385] = -1;
        let count: Ptr32<i32> = Ptr32::new(m.local(4));
        m[count] = 0;
        let mut i = 0;
        while i < m[l].nb_elems {
            if m[m[l].elems.add(i)].status == 0 {
                let d = m[m[l].elems.add(i)].data.addr();
                add_elem_in_chap_sort_tbl(m, i, l, d, tbl, count);
                if m[m[l].elems.add(i)].data == data {
                    m[LAST_DATA] = data.addr();
                    m[LAST_PEL] = l;
                    m[LAST_INDEX_385] = i;
                }
            }
            i += 1;
        }
        m[tbl].count = m[count];
        m[tbl].valid = 1;
        m.release(4);
    } else if data.addr() != m[LAST_DATA] || l != m[LAST_PEL] {
        let n = m[tbl].count;
        if find_elem_in_chap_sort_tbl(m, l, data.addr(), tbl, n, LAST_INDEX_385) == 0 {
            m[LAST_INDEX_385] = -1;
        } else {
            let at = m[LAST_INDEX_385];
            m[LAST_INDEX_385] = m[m[tbl].table.add(at)];
        }
        m[LAST_DATA] = data.addr();
        m[LAST_PEL] = l;
    }
    m[LAST_INDEX_385]
}

/// [soft 0x08134530 SI_elemdictFindElemByCB]: the first element in use for which the
/// caller's test `test(element, arg)` answers non-zero (16 bits), or −1. The test is a
/// code address, reached through [`Outside::call_addr`].
pub fn find_elem_by_cb(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, test: FnPtr, arg: u32) -> i32 {
    if !test.is_null() {
        let l = list_of(m, ed, chap);
        let mut i = 0;
        while i < m[l].nb_elems {
            let e = m[l].elems.add(i);
            if m[e].status == 0 && o.call_addr(m, test.addr(), &[e.addr(), arg]) as i16 != 0 {
                return i;
            }
            i += 1;
        }
    }
    -1
}

/// The scan the four plain look-ups by object share: the first slot that passes `pick`.
fn find_slot(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, pick: impl Fn(&Elem) -> bool) -> i32 {
    let l = list_of(m, ed, chap);
    let n = m[l].nb_elems;
    let elems = m[l].elems;
    let mut i = 0;
    while i < n {
        if pick(&m[elems.add(i)]) {
            return i;
        }
        i += 1;
    }
    -1
}

/// [soft 0x081345a0 SI_elemdictFindElemByAuxData]: the first element in use whose second
/// object is `aux`.
pub fn find_elem_by_aux_data(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, aux: Ptr32<Opaque>) -> i32 {
    find_slot(m, ed, chap, |e| e.status == 0 && e.aux_data == aux)
}

/// [soft 0x081345e8 SI_elemdictFindElemByDelData]: the first slot that is not deleted
/// (so in use, or flagged for deletion) whose object is `data`.
pub fn find_elem_by_del_data(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, data: Ptr32<Opaque>) -> i32 {
    find_slot(m, ed, chap, |e| e.status != 2 && e.data == data)
}

/// [soft 0x0813462c SI_elemdictFindElemByDelAuxData]: the same by second object.
pub fn find_elem_by_del_aux_data(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, aux: Ptr32<Opaque>) -> i32 {
    find_slot(m, ed, chap, |e| e.status != 2 && e.aux_data == aux)
}

/// How deep [`elem_update_hier_info`] follows a hierarchy before it gives up.
pub const HIER_DEPTH_LIMIT: i32 = 10_000;

/// [soft 0x08134c34 elemUpdateHierInfo]: writes depth and root index of element `index`
/// and goes down: for each entry of relation `relid` in list `group` of `rels` (the
/// element's backward lists), the element it names, one level deeper.
///
/// Where the Rust differs: a cycle of such relations makes the original recurse until its
/// stack is used up. Here the walk stops at [`HIER_DEPTH_LIMIT`] levels and records a
/// fault of the image.
#[allow(clippy::too_many_arguments)]
pub fn elem_update_hier_info(m: &mut Mem, list: Ptr32<ElemList>, rels: Ptr32<Relations>, group: i32, relid: i32, index: i32, root: i32, level: i32) {
    if level > HIER_DEPTH_LIMIT {
        m.record_fault(m.sp(), 0, si_mem::Access::Call);
        return;
    }
    let e = m[list].elems.add(index);
    m[e].hier_level = level;
    m[e].root_id = root;
    let l = m[rels].groups.add(group);
    let mut k = 0;
    while k < m[l].nb {
        let r = m[l].rels.add(k);
        if m[r].relid == relid {
            let son = m[r].elemid;
            let below: Ptr32<Relations> = field!(m[list].elems.add(son), Elem, backward);
            elem_update_hier_info(m, list, below, group, relid, son, root, level + 1);
        }
        k += 1;
    }
}

/// [soft 0x08134cc8 SI_elemdictUpdateHierInfo]: gives every slot of a chapter depth 0
/// and itself as root, then walks down from every root: an element in use that has
/// relation `relid` in list `group` on its backward side and not on its forward side.
pub fn update_hier_info(m: &mut Mem, ed: Ptr32<ElemDict>, chap: i32, group: i32, relid: i32) {
    let l = list_of(m, ed, chap);
    let mut i = 0;
    while i < m[l].nb_elems {
        let e = m[l].elems.add(i);
        m[e].hier_level = 0;
        m[e].root_id = i;
        i += 1;
    }
    let mut i = 0;
    while i < m[l].nb_elems {
        let e = m[l].elems.add(i);
        if m[e].status == 0 {
            let as_source = relations::rel_check(m, field!(e, Elem, forward), group, relid);
            let as_destination = relations::rel_check(m, field!(e, Elem, backward), group, relid);
            if -1 < as_destination && as_source < 0 {
                elem_update_hier_info(m, l, field!(e, Elem, backward), group, relid, i, i, 0);
            }
        }
        i += 1;
    }
}

/// [soft 0x081359bc SI_elemdictInit]: an empty scene dictionary: no chapters, both update
/// lists empty and dirty, the browser options at their defaults in both copies, the
/// relation and hierarchy flags set. The two bytes at +0x2e and +0x46 are whatever the
/// first copy of the options held there (they are copied, not set).
pub fn init(m: &mut Mem, ed: Ptr32<ElemDict>) {
    m[ed].nb_lists = 0;
    m[ed].lists = Ptr32::NULL;
    m[ed].chap_sort = Ptr32::NULL;
    m[ed].is_scene = 1;
    m[ed].dbinfo = Ptr32::NULL;
    m[ed].options.sort_type = 6;
    m[ed].options.unk_0a = 0;
    m[ed].options.unk_06 = 0;
    m[ed].options.unk_08 = 1;
    m[ed].options.unk_04 = 3;
    m[ed].options.unk_0c = -1;
    m[ed].options.unk_10 = 0;
    m[ed].options.unk_12 = 0;
    m[ed].options.unk_14 = 1;
    let from = field!(ed, ElemDict, options).cast::<u8>().addr();
    let to = field!(ed, ElemDict, options_db).cast::<u8>().addr();
    m.copy(to, from, 0x18);
    m[ed].upd_list.nb = 0;
    m[ed].upd_list.nodes = Ptr32::NULL;
    m[ed].upd_list.dirty = 1;
    m[ed].topo_list.nb = 0;
    m[ed].topo_list.nodes = Ptr32::NULL;
    m[ed].topo_list.dirty = 1;
    m[ed].rel_dirty = 1;
    m[ed].nb_fwd_backup = 0;
    m[ed].fwd_backup = Ptr32::NULL;
    m[ed].nb_back_backup = 0;
    m[ed].back_backup = Ptr32::NULL;
    m[ed].untold = 0;
    m[ed].hier_dirty = 1;
}

/// [soft 0x08135a8c SI_dbIconFree]: frees an element's icon record and the block at +8
/// of it.
pub fn icon_free(m: &mut Mem, e: Ptr32<Elem>) {
    let icon = m[e].icon.addr();
    if icon != 0 {
        let pixels = m.u32(icon + 8);
        if pixels != 0 {
            m.free(pixels);
            m.set_u32(icon + 8, 0);
        }
        m.free(icon);
    }
    m[e].icon = Ptr32::NULL;
}

/// [soft 0x08135ad4 SI_elemdictSetDeleteElem]: removes an element's relations and, with
/// `give_up_slot`, deletes it.
///
/// With `only_towards` set and more than one entry in the element's backward lists, only
/// the backward entries that name element `towards` are removed (with their counterparts)
/// and nothing else happens: an element that others still point at survives the removal
/// of one of them. Otherwise every relation goes, in both directions, and with
/// `give_up_slot` the chapter's sort table is marked stale, the name record and icon are
/// freed (`SI_dbListNodeFree (0x08122450)`), the pointers cleared, the status becomes 2,
/// the new-flag 0, the depth 0 and the root index the element's own. The dictionary's
/// lists and flags are marked.
#[allow(clippy::too_many_arguments)]
pub fn set_delete_elem(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, e: Ptr32<Elem>, index: i32, towards: i32, only_towards: i16, give_up_slot: i16) -> i32 {
    if only_towards != 0 {
        let mut total = 0;
        let mut g = 0;
        let n = m[e].backward.nb_groups;
        while g < n {
            total += m[m[e].backward.groups.add(g)].nb;
            g += 1;
        }
        if 1 < total {
            let mut g = 0;
            while g < m[e].backward.nb_groups {
                let l = m[e].backward.groups.add(g);
                relations::rel_remove_a_relation_id(m, o, ed, e, l, towards, chap, index, -1, 0, 1);
                g += 1;
            }
            return 1;
        }
    }
    relations::rel_remove_relations(m, o, ed, e, chap, index, 1);
    relations::rel_remove_relations(m, o, ed, e, chap, index, 0);
    if give_up_slot != 0 {
        let l = list_of(m, ed, chap);
        sort_tbl_set_dirty(m, l);
        m[e].status = 2;
        let node = m[e].node;
        outside::si_db_list_node_free(o, m, node);
        icon_free(m, e);
        m[e].data = Ptr32::NULL;
        m[e].aux_data = Ptr32::NULL;
        m[e].node = Ptr32::NULL;
        m[e].icon = Ptr32::NULL;
        m[e].hier_level = 0;
        m[e].root_id = index;
        m[e].new_flag = 0;
    }
    mark_relations_changed(m, ed);
    m[ed].hier_dirty = 1;
    1
}

/// [soft 0x08135c2c SI_elemdictDeleteChapterElems]: deletes every element in use of a
/// chapter, or with `root` not −1 those whose root index is `root`.
pub fn delete_chapter_elems(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, root: i32) {
    let l = list_of(m, ed, chap);
    let mut i = 0;
    while i < m[l].nb_elems {
        let e = m[l].elems.add(i);
        if m[e].status == 0 && (root == -1 || m[e].root_id == root) {
            set_delete_elem(m, o, ed, chap, e, i, -1, 0, 1);
        }
        i += 1;
    }
}

/// Frees the entry arrays of one direction's lists, then the lists.
fn free_relations(m: &mut Mem, rels: Ptr32<Relations>) {
    let mut g = 0;
    while g < m[rels].nb_groups {
        let l = m[rels].groups.add(g);
        if !m[l].rels.is_null() {
            let p = m[l].rels.addr();
            m.free(p);
            m[l].rels = Ptr32::NULL;
        }
        g += 1;
    }
    if !m[rels].groups.is_null() {
        let p = m[rels].groups.addr();
        m.free(p);
        m[rels].groups = Ptr32::NULL;
    }
}

/// What both disposal routines do with a chapter's sort table.
fn free_sort_tbl(m: &mut Mem, list: Ptr32<ElemList>) {
    let tbl = m[list].sort_tbl;
    if !tbl.is_null() {
        if !m[tbl].table.is_null() {
            let p = m[tbl].table.addr();
            m.free(p);
            m[tbl].table = Ptr32::NULL;
        }
        if !m[list].sort_tbl.is_null() {
            let p = m[list].sort_tbl.addr();
            m.free(p);
            m[list].sort_tbl = Ptr32::NULL;
        }
        m[list].sort_tbl = Ptr32::NULL;
    }
}

/// [soft 0x08135ca0 SI_elemlistDisposeElements]: frees everything the elements of a
/// chapter own (icon, suffix string, name record, relation lists; never the object) and
/// the chapter's arrays: elements, sub-directory strings, the three sort arrays, the sort
/// table. With `keep_selected` the name record of an element whose selected mark is set
/// is not freed (its pointer is cleared all the same).
pub fn elemlist_dispose_elements(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<ElemList>, keep_selected: i16) {
    let mut i = 0;
    while i < m[list].nb_elems {
        let e = m[list].elems.add(i);
        if !m[e].icon.is_null() {
            icon_free(m, e);
            m[e].icon = Ptr32::NULL;
        }
        if !m[e].suffix.is_null() {
            let p = m[e].suffix.addr();
            m.free(p);
            m[e].suffix = Ptr32::NULL;
        }
        if (keep_selected == 0 || m[e].selected == 0) && !m[e].node.is_null() {
            let node = m[e].node;
            outside::si_db_list_node_free(o, m, node);
            m[e].node = Ptr32::NULL;
        }
        free_relations(m, field!(e, Elem, forward));
        free_relations(m, field!(e, Elem, backward));
        m[e].forward.nb_groups = 0;
        m[e].backward.nb_groups = 0;
        m[e].node = Ptr32::NULL;
        i += 1;
    }
    if !m[list].elems.is_null() {
        let p = m[list].elems.addr();
        m.free(p);
        m[list].elems = Ptr32::NULL;
    }
    if !m[list].subdirs.is_null() {
        let mut k = 0;
        while k < m[list].nb_subdirs {
            let s = m[list].subdirs.add(k);
            if !m[s].is_null() {
                let p = m[s].addr();
                m.free(p);
                m[s] = Ptr32::NULL;
            }
            let aux = m[m[list].subdir_aux.add(k)];
            if !aux.is_null() {
                m.free(aux.addr());
            }
            k += 1;
        }
        if !m[list].subdirs.is_null() {
            let p = m[list].subdirs.addr();
            m.free(p);
            m[list].subdirs = Ptr32::NULL;
        }
        if !m[list].subdir_aux.is_null() {
            let p = m[list].subdir_aux.addr();
            m.free(p);
            m[list].subdir_aux = Ptr32::NULL;
        }
    }
    if !m[list].sort.is_null() {
        let p = m[list].sort.addr();
        m.free(p);
        m[list].sort = Ptr32::NULL;
    }
    if !m[list].hier_order.is_null() {
        let p = m[list].hier_order.addr();
        m.free(p);
        m[list].hier_order = Ptr32::NULL;
    }
    if !m[list].hier_count.is_null() {
        let p = m[list].hier_count.addr();
        m.free(p);
        m[list].hier_count = Ptr32::NULL;
    }
    free_sort_tbl(m, list);
    m[list].nb_elems = 0;
    m[list].elems = Ptr32::NULL;
    m[list].sort = Ptr32::NULL;
    m[list].hier_order = Ptr32::NULL;
    m[list].hier_count = Ptr32::NULL;
    m[list].subdirs = Ptr32::NULL;
    m[list].nb_subdirs = 0;
}

/// [soft 0x08135fd4 SI_elemlistDispose]: the elements, then the chapter's sub-directory
/// and suffix strings and its sort table. The chapter's name is not freed.
pub fn elemlist_dispose(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<ElemList>, keep_selected: i16) {
    elemlist_dispose_elements(m, o, list, keep_selected);
    if !m[list].subdir.is_null() {
        let p = m[list].subdir.addr();
        m.free(p);
        m[list].subdir = Ptr32::NULL;
    }
    if !m[list].suffix.is_null() {
        let p = m[list].suffix.addr();
        m.free(p);
        m[list].suffix = Ptr32::NULL;
    }
    free_sort_tbl(m, list);
}

/// [soft 0x08136128 relBackUpDispose]: frees an array of saved relations (`*count`
/// records of 8 bytes at `*array`) with each record's lists and their entry arrays.
pub fn rel_back_up_dispose(m: &mut Mem, array: Ptr32<Ptr32<Relations>>, count: Ptr32<i32>) {
    let mut i = 0;
    while i < m[count] {
        let rels = m[array].add(i);
        free_relations(m, rels);
        i += 1;
    }
    if !m[array].is_null() {
        let p = m[array].addr();
        m.free(p);
        m[array] = Ptr32::NULL;
    }
    m[count] = 0;
}

/// [soft 0x0813606c SI_elemdictDispose]: every chapter, the chapter array, the sorted
/// chapter indices, both update lists, both sets of saved relations; then the dictionary
/// is initialised again.
pub fn dispose(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, keep_selected: i16) {
    let mut i = 0;
    while i < m[ed].nb_lists {
        let l = list_of(m, ed, i);
        elemlist_dispose(m, o, l, keep_selected);
        i += 1;
    }
    if !m[ed].lists.is_null() {
        let p = m[ed].lists.addr();
        m.free(p);
        m[ed].lists = Ptr32::NULL;
    }
    if !m[ed].chap_sort.is_null() {
        let p = m[ed].chap_sort.addr();
        m.free(p);
        m[ed].chap_sort = Ptr32::NULL;
    }
    updlist::dispose(m, field!(ed, ElemDict, upd_list));
    updlist::dispose(m, field!(ed, ElemDict, topo_list));
    rel_back_up_dispose(m, field!(ed, ElemDict, fwd_backup), field!(ed, ElemDict, nb_fwd_backup));
    rel_back_up_dispose(m, field!(ed, ElemDict, back_backup), field!(ed, ElemDict, nb_back_backup));
    init(m, ed);
}

/// The chapter table's row count, for the loaders.
pub const NB_CHAPTERS: usize = CHAP_INFO.len();
