// SPDX-License-Identifier: MIT
//! `elemdict.c`, the rest: the global array of dictionaries, marks over a whole
//! dictionary, the clean-ups a scene load ends with, copies between dictionaries, and the
//! small look-ups built on the relation walks.

use crate::elemdict::{self, list_of};
use crate::elemname;
use crate::elemrel;
use crate::globals::*;
use crate::outside::{self, Outside};
use crate::reldict;
use crate::rodata::{CHAP_INFO, S_LEGACY_2};
use si_mem::{record, Mem, Opaque, Ptr32};
use si_types::dict::{ChapIds, ChapInfo, DbListNode, Elem, ElemDict, ElemIdentity, ElemList, RelDict};

record! {
    /// The start of a user-data record (`__SI_UDF`), as far as the dictionaries read it:
    /// the element the record belongs to.
    pub struct Udf = "__SI_UDF", size 0x0c {
        0x00 head: u32, "not read by the dictionaries";
        0x04 chapid: i32, "chapter of the owning element";
        0x08 elemid: i32, "its index; negative: none";
    }
}

/// [soft 0x08136c48 SI_elemdictSetNewFlag]: every slot of every chapter whose new-flag is
/// `from` (any slot when `from` is negative) gets the new-flag `to`.
pub fn set_new_flag(m: &mut Mem, ed: Ptr32<ElemDict>, from: i32, to: i32) {
    let mut c = 0;
    while c < m[ed].nb_lists {
        let l = list_of(m, ed, c);
        let mut i = 0;
        while i < m[l].nb_elems {
            let e = m[l].elems.add(i);
            if from < 0 || m[e].new_flag as i32 == from {
                m[e].new_flag = to as i16;
            }
            i += 1;
        }
        c += 1;
    }
}

/// [soft 0x08136cb0 SI_elemdictSetSelection]: sets the selected mark of every slot.
pub fn set_selection(m: &mut Mem, ed: Ptr32<ElemDict>, selected: i16) {
    let mut c = 0;
    while c < m[ed].nb_lists {
        let l = list_of(m, ed, c);
        let mut i = 0;
        while i < m[l].nb_elems {
            let e = m[l].elems.add(i);
            m[e].selected = selected;
            i += 1;
        }
        c += 1;
    }
}

/// [soft 0x08136d10 SI_elemdictIsElemUsed]: whether anything depends on an element: any
/// entry in its six backward lists. With `ignore_hierarchy`, entries of relations 110
/// and 207 in list 1 do not count.
pub fn is_elem_used(m: &Mem, e: Ptr32<Elem>, ignore_hierarchy: i16) -> bool {
    let mut g = 0;
    let mut used = false;
    loop {
        let l = m[e].backward.groups.add(g);
        if g == 1 && ignore_hierarchy != 0 {
            let mut k = 0;
            g = 2;
            while k < m[l].nb && !used {
                let relid = m[m[l].rels.add(k)].relid;
                if relid != 0x6e && relid != 0xcf {
                    used = true;
                }
                k += 1;
            }
        } else {
            used = 0 < m[l].nb;
            g += 1;
        }
        if !(g < 6 && !used) {
            return used;
        }
    }
}

/// [soft 0x08136db0 SI_elemdictCheckEmptyDescription]: the default scene makes room: when
/// the SCENES chapter has exactly one slot, the chapters MODELS, ANIMATION, TEXTURES2D,
/// TEXTURES3D, WAVES and MATERIALS have no element in use, and that one scene's new-flag
/// is 2, the SCENES chapter is disposed of.
pub fn check_empty_description(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>) {
    let cid = m.read(SI_CID);
    let scenes = list_of(m, ed, cid.dsc);
    if m[scenes].nb_elems == 1 {
        let mut total = 0;
        for chap in [cid.model, cid.ani, cid.t2d, cid.t3d, cid.wav, cid.mat] {
            total += elemdict::elemlist_count_valid_elems(m, list_of(m, ed, chap));
        }
        if total < 1 && m[m[scenes].elems].new_flag == 2 {
            elemdict::elemlist_dispose(m, o, scenes, 0);
        }
    }
}

/// [soft 0x08136e98 SI_elemdictAddSelElemsFromDb]: moves the selected elements of the
/// database dictionary `db` into `ed`, chapter by chapter (the same index in both). Each
/// gets a new name record copied from the old one; an element of OUTSIDE_DB_FILES keeps
/// its full name as it was; the first scene copied may first empty the SCENES chapter of
/// `ed` ([`check_empty_description`]). The new element takes the selected and shared
/// marks, new-flag 1 and the icon (which the old element gives up). Each chapter of `db`
/// that had any slot is then disposed of. `_unused` is the third argument.
pub fn add_sel_elems_from_db(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, db: Ptr32<ElemDict>, _unused: i16) {
    let mut c = 0;
    while c < m[db].nb_lists {
        let from = list_of(m, db, c);
        let to = list_of(m, ed, c);
        m[to].unk_1c = m[from].unk_1c;
        if m[from].kind == 2 && !m[from].subdir.is_null() {
            if !m[to].subdir.is_null() {
                let p = m[to].subdir.addr();
                m.free(p);
                m[to].subdir = Ptr32::NULL;
            }
            m[to].subdir = Ptr32::new(m.strdup(m[from].subdir.addr()));
        }
        let mut i = 0;
        while i < m[from].nb_elems {
            let old = m[from].elems.add(i);
            if m[old].selected != 0 {
                let node: Ptr32<DbListNode> = outside::si_db_list_node_allocate(o, m);
                let old_node = m[old].node;
                elemname::db_list_node_copy_info(m, o, ed, old_node, node);
                if c == m[SI_CID].dsc {
                    check_empty_description(m, o, ed);
                } else if c == m[SI_CID].out {
                    if !m[node].fullname.is_null() {
                        let p = m[node].fullname.addr();
                        m.free(p);
                        m[node].fullname = Ptr32::NULL;
                    }
                    let full = m[m[old].node].fullname.addr();
                    m[node].fullname = Ptr32::new(m.strdup(full));
                }
                let index = elemdict::elemlist_add_elem(m, o, to, node, Ptr32::NULL, Ptr32::NULL, 0, 0, 1, 1);
                let new = m[to].elems.add(index);
                m[new].selected = m[old].selected;
                m[new].shared = m[old].shared;
                m[new].new_flag = 1;
                m[new].icon = m[old].icon;
                m[old].icon = Ptr32::NULL;
            }
            i += 1;
        }
        if 0 < m[from].nb_elems {
            elemdict::elemlist_dispose(m, o, from, 0);
        }
        c += 1;
    }
    m[ed].upd_list.dirty = 1;
    m[ed].topo_list.dirty = 1;
    m[ed].hier_dirty = 1;
}

/// [soft 0x08137064 SI_elemdictMoveElemsFromOutsideChapter]: each selected element of the
/// OUTSIDE_DB_FILES chapter whose full name has a suffix that
/// `SI_edGetChapterFromSuffix (0x08120a7c)` knows is copied into that chapter (name
/// record copied, the full name kept, selected, new-flag 1, the icon handed over; the
/// outside chapter's sub-directory becomes the record's +8). The outside chapter is then
/// disposed of.
pub fn move_elems_from_outside_chapter(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>) {
    let outside_list = list_of(m, ed, m[SI_CID].out);
    let mut i = 0;
    while i < m[outside_list].nb_elems {
        let old = m[outside_list].elems.add(i);
        if m[old].selected != 0 && !m[m[old].node].fullname.is_null() {
            let dot = crate::libc::strrchr(m, m[m[old].node].fullname.addr(), b'.');
            if dot != 0 {
                let chap = outside::si_ed_get_chapter_from_suffix(o, m, ed, dot + 1);
                if -1 < chap {
                    let to = list_of(m, ed, chap);
                    let node: Ptr32<DbListNode> = outside::si_db_list_node_allocate(o, m);
                    let old_node = m[old].node;
                    elemname::db_list_node_copy_info(m, o, ed, old_node, node);
                    if !m[node].fullname.is_null() {
                        let p = m[node].fullname.addr();
                        m.free(p);
                        m[node].fullname = Ptr32::NULL;
                    }
                    let full = m[m[old].node].fullname.addr();
                    m[node].fullname = Ptr32::new(m.strdup(full));
                    let index = elemdict::elemlist_add_elem(m, o, to, node, Ptr32::NULL, Ptr32::NULL, 0, 0, 1, 1);
                    let new = m[to].elems.add(index);
                    m[new].selected = 1;
                    m[new].new_flag = 1;
                    m[new].icon = m[old].icon;
                    m[old].icon = Ptr32::NULL;
                    if !m[outside_list].subdir.is_null() {
                        let new_node = m[new].node;
                        if !m[new_node].str_08.is_null() {
                            let p = m[new_node].str_08.addr();
                            m.free(p);
                            m[new_node].str_08 = Ptr32::NULL;
                        }
                        m[new_node].str_08 = Ptr32::new(m.strdup(m[outside_list].subdir.addr()));
                    }
                }
            }
        }
        i += 1;
    }
    if 0 < m[outside_list].nb_elems {
        elemdict::elemlist_dispose(m, o, outside_list, 0);
    }
    m[ed].upd_list.dirty = 1;
    m[ed].topo_list.dirty = 1;
    m[ed].hier_dirty = 1;
}

/// [soft 0x0813720c SI_elemdictRemoveAllSubScenes]: after a load, deletes every slot but
/// the first of SCENES, SETUP_SOFT, SETUP_CHANNELS and SETUP_FLOCK (whatever its status),
/// and in EXPRESSIONS every root in use (depth below 1) after the first.
pub fn remove_all_sub_scenes(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>) {
    for which in 0..4 {
        let cid = m.read(SI_CID);
        let chap = [cid.dsc, cid.sts, cid.stc, cid.sfk][which];
        let l = list_of(m, ed, chap);
        let mut i = 1;
        while i < m[l].nb_elems {
            let e = m[l].elems.add(i);
            elemdict::set_delete_elem(m, o, ed, chap, e, i, -1, 0, 1);
            i += 1;
        }
    }
    let chap = m[SI_CID].exp;
    let l = list_of(m, ed, chap);
    let mut seen = false;
    let mut i = 0;
    while i < m[l].nb_elems {
        let e = m[l].elems.add(i);
        if m[e].status == 0 && m[e].hier_level < 1 {
            if seen {
                let chap = m[SI_CID].exp;
                elemdict::set_delete_elem(m, o, ed, chap, e, i, -1, 0, 1);
            } else {
                seen = true;
            }
        }
        i += 1;
    }
}

/// [soft 0x081373d0 SI_elemdictRemoveAllInvalidElems]: deletes the elements of this load
/// (new-flag 1, in use) of MODELS, CAMERAS, LIGHTS and MATERIALS that have no object, in
/// chapters of kind 1.
pub fn remove_all_invalid_elems(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>) {
    let mut c = 0;
    while c < m[ed].nb_lists {
        let l = list_of(m, ed, c);
        let cid = m.read(SI_CID);
        if m[l].kind == 1 && (c == cid.model || c == cid.cam || c == cid.lig || c == cid.mat) {
            let mut i = 0;
            while i < m[l].nb_elems {
                let e = m[l].elems.add(i);
                if m[e].new_flag == 1 && m[e].status == 0 && m[e].data.is_null() {
                    elemdict::set_delete_elem(m, o, ed, c, e, i, -1, 0, 1);
                }
                i += 1;
            }
        }
        c += 1;
    }
}

/// [soft 0x08137490 SI_elemdictRemoveAllEmptyAnim]: deletes the roots of a chapter
/// (animation files) that have no element below them: the work flag of every slot is
/// cleared, each element in use below a root counts into its root's work flag, and every
/// root in use left at 0 is deleted. The hierarchical order of the ANIMATION chapter of
/// the scene's `SI_cid` is then made anew.
pub fn remove_all_empty_anim(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32) {
    let l = list_of(m, ed, chap);
    let mut i = 0;
    while i < m[l].nb_elems {
        let e = m[l].elems.add(i);
        m[e].work_flag = 0;
        i += 1;
    }
    let mut i = 0;
    while i < m[l].nb_elems {
        let e = m.read(m[l].elems.add(i));
        if e.status == 0 && 0 < e.hier_level && e.root_id != i {
            let root = m[l].elems.add(e.root_id);
            m[root].work_flag += 1;
        }
        i += 1;
    }
    let mut i = 0;
    while i < m[l].nb_elems {
        let e = m[l].elems.add(i);
        if m[e].status == 0 && m[e].hier_level == 0 && m[e].work_flag == 0 {
            elemdict::set_delete_elem(m, o, ed, chap, e, i, -1, 0, 1);
        }
        i += 1;
    }
    let ani = list_of(m, ed, m[SI_CID].ani);
    sort_anim_elements(m, o, ani);
}

/// [soft 0x08137980 SI_elemdictSortAnimElements]: the hierarchical order of a chapter
/// (finding 0018, section 8.3): two new arrays of one word per slot (`SI_calloc`). Going
/// through the slots in order: a slot not in use is put down as it comes; a root in use
/// (depth 0) is preceded by the elements in use whose root index it is, in index order,
/// and the count array holds how many those were at the root's position; an element in
/// use below a root is put down with its root only.
///
/// Nothing happens with `SI_XSI_LEGACYLOADER` in the environment or for a chapter without
/// slots; a chapter with one slot gets the two arrays, zeroed, and nothing more. The old
/// count array is freed only when there was an old order array. When the elements under a
/// root would run past the arrays, a message is printed and the routine stops there.
pub fn sort_anim_elements(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<ElemList>) {
    let name = m.cstr(S_LEGACY_2.addr());
    if o.getenv(m, &name) != 0 || m[list].nb_elems == 0 {
        return;
    }
    if !m[list].hier_order.is_null() {
        let p = m[list].hier_order.addr();
        m.free(p);
        m[list].hier_order = Ptr32::NULL;
        if !m[list].hier_count.is_null() {
            let p = m[list].hier_count.addr();
            m.free(p);
            m[list].hier_count = Ptr32::NULL;
        }
    }
    let n = m[list].nb_elems as u32;
    m[list].hier_order = Ptr32::new(m.si_calloc(n, 4));
    m[list].hier_count = Ptr32::new(m.si_calloc(n, 4));
    let mut i = 0;
    while i < m[list].nb_elems {
        let slot = m[list].hier_count.add(i);
        m[slot] = 0;
        i += 1;
    }
    if 1 < m[list].nb_elems {
        let mut at = 0;
        let mut i = 0;
        while i < m[list].nb_elems {
            let e = m[list].elems.add(i);
            if m[e].status == 0 {
                if m[e].hier_level == 0 {
                    let mut count = 0;
                    let mut j = 0;
                    while j < m[list].nb_elems {
                        let x = m[list].elems.add(j);
                        if m[x].status == 0 && x != e && m[x].root_id == i {
                            let slot = m[list].hier_order.add(at);
                            m[slot] = j;
                            at += 1;
                            count += 1;
                        }
                        j += 1;
                    }
                    let n = m[list].nb_elems;
                    if n <= at {
                        o.stdout(format!(" ANIM HierSort: #{i} ind > nbelems ({n})\n").as_bytes());
                        return;
                    }
                    let slot = m[list].hier_order.add(at);
                    m[slot] = i;
                    let slot = m[list].hier_count.add(at);
                    m[slot] = count;
                    at += 1;
                }
            } else {
                let slot = m[list].hier_order.add(at);
                m[slot] = i;
                at += 1;
            }
            i += 1;
        }
    }
}

/// [soft 0x08137b00 SI_elemdictAdd]: one more dictionary at the end of the global array
/// (`SI_elemDicts`, with its flag in `elemDictExists`), initialised and marked existing;
/// answers its index, or −1 when the array could not be had.
pub fn add(m: &mut Mem) -> i32 {
    let index = m[SI_NB_ELEM_DICTS];
    if m[SI_ELEM_DICTS].is_null() {
        m[SI_ELEM_DICTS] = Ptr32::new(m.si_malloc(0x70));
        m[ELEM_DICT_EXISTS] = Ptr32::new(m.si_malloc(2));
    } else {
        let old = m[SI_ELEM_DICTS].addr();
        let n = (m[SI_NB_ELEM_DICTS] + 1) as u32;
        m[SI_ELEM_DICTS] = Ptr32::new(m.si_realloc(old, n * 0x70));
        let old = m[ELEM_DICT_EXISTS].addr();
        m[ELEM_DICT_EXISTS] = Ptr32::new(m.si_realloc(old, n * 2));
    }
    if m[SI_ELEM_DICTS].is_null() {
        return -1;
    }
    let ed = m[SI_ELEM_DICTS].add(m[SI_NB_ELEM_DICTS]);
    elemdict::init(m, ed);
    let flag = m[ELEM_DICT_EXISTS].add(m[SI_NB_ELEM_DICTS]);
    m[flag] = 1;
    m[SI_NB_ELEM_DICTS] += 1;
    index
}

/// [soft 0x08137bdc SI_elemdictRemove]: disposes of dictionary `index` and clears its
/// flag; its slot stays. Refused (0) for the current dictionary and for an index outside
/// the array; a slot that is already gone answers 1.
pub fn remove(m: &mut Mem, o: &mut dyn Outside, index: i32) -> i32 {
    if index == m[m[SI_PROC_DATA]] || index < 0 || m[SI_NB_ELEM_DICTS] <= index {
        return 0;
    }
    let flag = m[ELEM_DICT_EXISTS].add(index);
    if m[flag] == 1 {
        let ed = m[SI_ELEM_DICTS].add(index);
        elemdict::dispose(m, o, ed, 0);
        m[flag] = 0;
    }
    1
}

/// [soft 0x08137c40 SI_elemdictGetNb]: how many dictionaries exist.
pub fn get_nb(m: &Mem) -> i32 {
    let mut count = 0;
    let mut i = 0;
    while i < m[SI_NB_ELEM_DICTS] {
        if m[m[ELEM_DICT_EXISTS].add(i)] == 1 {
            count += 1;
        }
        i += 1;
    }
    count
}

/// [soft 0x08137c70 SI_elemdictExists]
pub fn exists(m: &Mem, index: i32) -> i32 {
    if index < 0 || m[SI_NB_ELEM_DICTS] <= index || m[m[ELEM_DICT_EXISTS].add(index)] == 0 {
        0
    } else {
        1
    }
}

/// [soft 0x08137c9c SI_elemdictCompareElemIdent]: whether two identities name the same
/// chapter and element (the two further words are not compared).
pub fn compare_elem_ident(m: &Mem, a: Ptr32<ElemIdentity>, b: Ptr32<ElemIdentity>) -> bool {
    if a.is_null() || b.is_null() {
        return false;
    }
    m[a].chapid == m[b].chapid && m[a].elemid == m[b].elemid
}

/// [soft 0x08137cd0 SI_elemdictCopyElements]: appends a copy of every slot of `src` to
/// the same chapter of `dst` (no slot reused, relation lists allocated and empty): a new
/// name record copied from the old one when there is one, the selected and shared marks,
/// the status, depth, root index and object, new-flag 1, and the thirteen words from
/// +0x0c to +0x3f (schematic position, colour, display, the pointer at +0x3c included).
pub fn copy_elements(m: &mut Mem, o: &mut dyn Outside, src: Ptr32<ElemDict>, dst: Ptr32<ElemDict>) {
    let mut c = 0;
    while c < m[src].nb_lists {
        let from = list_of(m, src, c);
        let to = list_of(m, dst, c);
        let mut i = 0;
        while i < m[from].nb_elems {
            let old = m[from].elems.add(i);
            let mut node: Ptr32<DbListNode> = Ptr32::NULL;
            if !m[old].node.is_null() {
                node = outside::si_db_list_node_allocate(o, m);
                let old_node = m[old].node;
                elemname::db_list_node_copy_info(m, o, src, old_node, node);
            }
            let index = elemdict::elemlist_add_elem(m, o, to, node, Ptr32::NULL, Ptr32::NULL, 0, 0, 1, 0);
            let new = m[to].elems.add(index);
            let e = m.read(old);
            m[new].selected = e.selected;
            m[new].shared = e.shared;
            m[new].status = e.status;
            m[new].hier_level = e.hier_level;
            m[new].root_id = e.root_id;
            m[new].data = e.data;
            m[new].new_flag = 1;
            m.copy(new.addr() + 0xc, old.addr() + 0xc, 13 * 4);
            i += 1;
        }
        c += 1;
    }
}

/// [soft 0x08137ddc SI_elemdictCopyRenderRelations]: for every forward entry of every
/// element in use of `src` whose relation has the render flag in `rd`, the same relation
/// is added between the elements of the same indices in `dst` (first rule, open chapters
/// taken from the entry, no data, active).
pub fn copy_render_relations(m: &mut Mem, src: Ptr32<ElemDict>, dst: Ptr32<ElemDict>, rd: Ptr32<RelDict>) {
    let mut c = 0;
    while c < m[src].nb_lists {
        let l = list_of(m, src, c);
        let mut i = 0;
        while i < m[l].nb_elems {
            let e = m[l].elems.add(i);
            if m[e].status == 0 {
                let mut g = 0;
                while g < m[e].forward.nb_groups {
                    let rl = m[e].forward.groups.add(g);
                    let mut k = 0;
                    while k < m[rl].nb {
                        let x = m.read(m[rl].rels.add(k));
                        if reldict::get_rel_render_flag(m, rd, x.relid) as i16 != 0 {
                            elemrel::add_uncomplete_relation(m, dst, rd, c, x.chapid as i32, i, x.elemid, x.relid, 0, 0, Ptr32::NULL, 1);
                        }
                        k += 1;
                    }
                    g += 1;
                }
            }
            i += 1;
        }
        c += 1;
    }
}

/// [soft 0x08137f0c SI_elemdictRelationCheck]: whether any list of an element (backward,
/// or with `forward` forward) holds an entry of relation `relid`, optionally naming
/// chapter `chap`.
pub fn relation_check(m: &Mem, _ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, relid: i32, chap: i32) -> i32 {
    if e.is_null() {
        return 0;
    }
    let side = if forward != 0 { m[e].forward } else { m[e].backward };
    if side.groups.is_null() {
        return 0;
    }
    let mut g = 0;
    while g < side.nb_groups {
        let l = side.groups.add(g);
        let rels = m[l].rels;
        let mut k = 0;
        while k < m[l].nb {
            let x = m.read(rels.add(k));
            if x.relid == relid && (x.chapid as i32 == chap || chap < 0) {
                return 1;
            }
            k += 1;
        }
        g += 1;
    }
    0
}

/// [soft 0x08137f94 SI_elemdictGetElemFromData]: the element in use whose object is
/// `data`, and its index in `*index` (−1 without one, or for a null dictionary or
/// object).
pub fn get_elem_from_data(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, data: Ptr32<Opaque>, index: Ptr32<i32>) -> Ptr32<Elem> {
    if ed.is_null() || data.is_null() {
        if !index.is_null() {
            m[index] = -1;
        }
        return Ptr32::NULL;
    }
    let i = elemdict::find_elem_by_data(m, o, ed, chap, data);
    if !index.is_null() {
        m[index] = i;
    }
    if -1 < i {
        return elemdict::get_elem(m, ed, chap, i);
    }
    Ptr32::NULL
}

/// The two words of stack the look-ups below hand on for the chapter and index they do
/// not want.
fn with_scratch<R>(m: &mut Mem, f: impl FnOnce(&mut Mem, Ptr32<i32>, Ptr32<i32>) -> R) -> R {
    let a: Ptr32<i32> = Ptr32::new(m.local(4));
    let b: Ptr32<i32> = Ptr32::new(m.local(4));
    let r = f(m, a, b);
    m.release(8);
    r
}

/// [soft 0x08137ff4 SI_elemdictFindSon]: the first son of an element: the first element
/// of the MODELS chapter related to it by relation 110 in backward list 1.
pub fn find_son(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>) -> Ptr32<Elem> {
    if ed.is_null() || e.is_null() {
        return Ptr32::NULL;
    }
    let model = m[SI_CID].model;
    with_scratch(m, |m, a, b| elemrel::find_related_elem(m, ed, e, 0, 1, 0x6e, model, a, b))
}

/// [soft 0x08138030 SI_elemdictFindNextSon]: the next son.
pub fn find_next_son(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>) -> Ptr32<Elem> {
    if ed.is_null() || e.is_null() {
        return Ptr32::NULL;
    }
    let model = m[SI_CID].model;
    with_scratch(m, |m, a, b| elemrel::find_next_related_elem(m, ed, e, 0, 1, 0x6e, model, a, b))
}

/// [soft 0x08138068 SI_elemdictGetElemFromScene]: slot 0 of the SCENES chapter; `*index`
/// becomes 0.
pub fn get_elem_from_scene(m: &mut Mem, ed: Ptr32<ElemDict>, index: Ptr32<i32>) -> Ptr32<Elem> {
    if !index.is_null() {
        m[index] = 0;
    }
    let dsc = m[SI_CID].dsc;
    elemdict::get_elem(m, ed, dsc, 0)
}

/// [soft 0x08138224 SI_elemdictGetCurRelationIndex]: the cursor of list `group` (backward,
/// or with `forward` forward); −1 for a null element or one without lists.
pub fn get_cur_relation_index(m: &Mem, e: Ptr32<Elem>, forward: i16, group: i32) -> i32 {
    if !e.is_null() {
        let side = if forward != 0 { m[e].forward } else { m[e].backward };
        if !side.groups.is_null() {
            return m[side.groups.add(group)].cur;
        }
    }
    -1
}

/// [soft 0x0813825c SI_elemdictSetCurRelationIndex]: sets that cursor.
pub fn set_cur_relation_index(m: &mut Mem, e: Ptr32<Elem>, forward: i16, group: i32, cur: i32) {
    if !e.is_null() {
        let side = if forward != 0 { m[e].forward } else { m[e].backward };
        if !side.groups.is_null() {
            let l = side.groups.add(group);
            m[l].cur = cur;
        }
    }
}

/// [soft 0x08138094 SI_elemdictGetFirstRelatedElem]: the object of the first element
/// related by `relid` in list `group`; the element goes to `*elem`, the list's cursor to
/// `*cursor` (each when given).
#[allow(clippy::too_many_arguments)]
pub fn get_first_related_elem(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, relid: i32, chap: i32, elem: Ptr32<Ptr32<Elem>>, cursor: Ptr32<i32>) -> Ptr32<Opaque> {
    let found = with_scratch(m, |m, a, b| elemrel::find_related_elem(m, ed, e, forward, group, relid, chap, a, b));
    if !cursor.is_null() {
        m[cursor] = get_cur_relation_index(m, e, forward, group);
    }
    if !elem.is_null() {
        m[elem] = found;
    }
    if found.is_null() {
        Ptr32::NULL
    } else {
        m[found].data
    }
}

/// [soft 0x08138104 SI_elemdictGetNextRelatedElem]: the next one, going on from
/// `*cursor` (which is put into the list first, when given).
#[allow(clippy::too_many_arguments)]
pub fn get_next_related_elem(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, relid: i32, chap: i32, elem: Ptr32<Ptr32<Elem>>, cursor: Ptr32<i32>) -> Ptr32<Opaque> {
    if !cursor.is_null() {
        let cur = m[cursor];
        set_cur_relation_index(m, e, forward, group, cur);
    }
    let found = with_scratch(m, |m, a, b| elemrel::find_next_related_elem(m, ed, e, forward, group, relid, chap, a, b));
    if !cursor.is_null() {
        m[cursor] = get_cur_relation_index(m, e, forward, group);
    }
    if !elem.is_null() {
        m[elem] = found;
    }
    if found.is_null() {
        Ptr32::NULL
    } else {
        m[found].data
    }
}

/// [soft 0x08138190 SI_elemdictGetNthRelatedElem]: the object of the `nth` (from 0)
/// related element, the element in `*elem` (when given).
#[allow(clippy::too_many_arguments)]
pub fn get_nth_related_elem(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, nth: i32, forward: i16, group: i32, relid: i32, chap: i32, elem: Ptr32<Ptr32<Elem>>) -> Ptr32<Opaque> {
    let found = with_scratch(m, |m, a, b| {
        let mut found = elemrel::find_related_elem(m, ed, e, forward, group, relid, chap, a, b);
        let mut k = 1;
        while k <= nth {
            found = elemrel::find_next_related_elem(m, ed, e, forward, group, relid, chap, a, b);
            if found.is_null() {
                break;
            }
            k += 1;
        }
        found
    });
    if !elem.is_null() {
        m[elem] = found;
    }
    if found.is_null() {
        Ptr32::NULL
    } else {
        m[found].data
    }
}

/// [soft 0x08138290 SI_elemdictAddUDFElemRelation]: relates the element a user-data
/// record belongs to (the source) to element `e` of chapter `chap` (found by its object)
/// by relation 551 with serial number `serial`, unless that relation is there already.
/// The lists come from the first rule of 551 in `SI_RD`.
pub fn add_udf_elem_relation(m: &mut Mem, o: &mut dyn Outside, udf: Ptr32<Udf>, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, chap: i32, serial: i32) {
    if udf.is_null() || e.is_null() || m[udf].elemid < 0 {
        return;
    }
    let data = m[e].data;
    let index = elemdict::find_elem_by_data(m, o, ed, chap, data);
    if -1 < index {
        let owner = elemdict::get_elem(m, ed, m[udf].chapid, m[udf].elemid);
        if !owner.is_null() && elemrel::get_relation_from_ids(m, ed, owner, 0x227, chap, index, serial).is_null() {
            if let Some([_, _, src_group, dst_group, _, _]) = reldict::rel_attributes(m, SI_RD, 0x227, 1) {
                let (uc, ui) = (m[udf].chapid, m[udf].elemid);
                elemrel::add_serial_relation(m, ed, SI_RD, uc, chap, src_group, dst_group, ui, index, 0x227, serial, 1, 0, Ptr32::NULL, 1, 0);
            }
        }
    }
}

/// [soft 0x08138384 SI_elemdictRemoveUDFElemRelation]: removes that relation.
pub fn remove_udf_elem_relation(m: &mut Mem, o: &mut dyn Outside, udf: Ptr32<Udf>, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, chap: i32, serial: i32) {
    if udf.is_null() || e.is_null() || m[udf].elemid < 0 {
        return;
    }
    let owner = elemdict::get_elem(m, ed, m[udf].chapid, m[udf].elemid);
    let data = m[e].data;
    let index = elemdict::find_elem_by_data(m, o, ed, chap, data);
    if -1 < index && !elemrel::get_relation_from_ids(m, ed, owner, 0x227, chap, index, serial).is_null() {
        let (uc, ui) = (m[udf].chapid, m[udf].elemid);
        elemrel::remove_uncomplete_rel_pair(m, o, ed, SI_RD, uc, chap, ui, index, 0x227, 1, 0, serial);
    }
}

/// For each row of the chapter table, the word of `SI_ChapIds` that receives the
/// chapter's index (finding 0010's table, read again from the 36 stores of
/// `SI_elemdictLoadChapters`).
pub const CHAP_ID_WORD: [usize; 36] = [
    3, 0, 1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 0x16, 0x17, 0x18, 0x15, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0xc, 0x10, 0xf, 0xd, 0xe, 0x11, 0x12, 0x13, 0x14, 0x1e, 0x1f, 0x20,
    0x21, 0x22, 0x23,
];

/// [soft 0x08133cf0 SI_elemdictLoadChapters]: gives a dictionary the 36 chapters of the
/// chapter table (`SI_chapInfo`), in table order, each under a copy of its name, and
/// writes each chapter's index into its word of `cid`. NSELECTIONS and CHANNEL_DRIVERS
/// become kind 0, OUTSIDE_DB_FILES kind 2. `is_scene` is stored; a scene dictionary gets
/// sort type 9 and the word at +0x22 set in its first copy of the browser options, a
/// database dictionary sort type 9 and +0x3a in the second copy, and +0x20 cleared. Last,
/// every chapter gets a copy of the suffix `SI_edGetSuffixFromChapter (0x08120a48)`
/// answers for its index.
///
/// A chapter the dictionary already has is found by name, not added again, and the copy
/// of the name made for it is lost.
pub fn load_chapters(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, cid: Ptr32<ChapIds>, is_scene: i16) {
    let words: Ptr32<i32> = cid.cast();
    for row in 0..CHAP_INFO.len() {
        let info: Ptr32<ChapInfo> = SI_CHAP_INFO.cast::<ChapInfo>().add(row as i32);
        let name = m.strdup(m[info].name.addr());
        let index = elemdict::add_list(m, ed, Ptr32::new(name));
        m[words.add(CHAP_ID_WORD[row] as i32)] = index;
    }
    let l = list_of(m, ed, m[cid].nsl);
    m[l].kind = 0;
    let l = list_of(m, ed, m[cid].chn_drv);
    m[l].kind = 0;
    let l = list_of(m, ed, m[cid].out);
    m[l].kind = 2;
    m[ed].is_scene = is_scene;
    if is_scene == 0 {
        m[ed].options.unk_08 = 0;
        m[ed].options_db.sort_type = 9;
        m[ed].options_db.unk_0a = 1;
    } else {
        m[ed].options.sort_type = 9;
        m[ed].options.unk_0a = 1;
    }
    let mut i = 0;
    while i < m[ed].nb_lists {
        let lists = m[ed].lists;
        let suffix = outside::si_ed_get_suffix_from_chapter(o, m, i);
        let copy = m.strdup(suffix.addr());
        m[lists.add(i)].suffix = Ptr32::new(copy);
        i += 1;
    }
}

/// [soft 0x0813757c SI_elemdictRecursiveSelect]: marks an element and everything it is
/// made from as selected (the selected mark at +0x42; the work flag at +0x58 says "done").
///
/// At depth 0 the hierarchy information of MODELS, CAMERAS and LIGHTS is brought up to
/// date (relations 110, 1110 and 2110 in list 1), every mark and work flag is cleared and
/// the element of chapter `chap` with the full name `fullname` is marked; without one the
/// answer is 0. Each further pass goes through every chapter: for an element in use that
/// is marked and not done, a root marks every element in use of its chapter that has it
/// as root, and every element its forward entries name is marked (with `nb_chaps` above
/// 0, only entries into one of the chapters `chaps`); the pass counts the elements newly
/// marked through entries and repeats while there are any. An element is flagged done
/// once it has been gone through, but only if it has relation lists; and so is every
/// element the pass met that it did not have to go through.
pub fn recursive_select(m: &mut Mem, ed: Ptr32<ElemDict>, chap: i32, fullname: si_mem::CStr32, depth: i32, nb_chaps: i32, chaps: Ptr32<i32>) -> i32 {
    let mut count = 0;
    if depth == 0 {
        let cid = m.read(SI_CID);
        elemdict::update_hier_info(m, ed, cid.model, 1, 0x6e);
        elemdict::update_hier_info(m, ed, cid.cam, 1, 0x456);
        elemdict::update_hier_info(m, ed, cid.lig, 1, 0x83e);
        let mut c: u32 = 0;
        while c < m[ed].nb_lists as u32 {
            let l = list_of(m, ed, c as i32);
            let mut i: u32 = 0;
            while i < m[l].nb_elems as u32 {
                let e = m[l].elems.add(i as i32);
                m[e].selected = 0;
                m[e].work_flag = 0;
                i += 1;
            }
            c += 1;
        }
        let i = elemname::find_elem_by_full_name(m, ed, chap, Ptr32::NULL, fullname, 0);
        if i < 0 {
            return 0;
        }
        let e = m[list_of(m, ed, chap)].elems.add(i);
        m[e].selected = 1;
    } else {
        let mut c: u32 = 0;
        while c < m[ed].nb_lists as u32 {
            let l = list_of(m, ed, c as i32);
            let mut j: u32 = 0;
            while j < m[l].nb_elems as u32 {
                let e = m[l].elems.add(j as i32);
                let active = m[e].status == 0 && m[e].selected != 0 && m[e].work_flag == 0;
                let mut done = !active;
                if active {
                    if m[e].hier_level == 0 {
                        let mut k: u32 = 0;
                        while k < m[l].nb_elems as u32 {
                            let x = m[l].elems.add(k as i32);
                            if m[x].status == 0 && m[x].root_id as u32 == j {
                                m[x].selected = 1;
                            }
                            k += 1;
                        }
                    }
                    if !m[e].forward.groups.is_null() {
                        let mut g: u32 = 0;
                        while g < m[e].forward.nb_groups as u32 {
                            let rl = m[e].forward.groups.add(g as i32);
                            let mut q: u32 = 0;
                            while q < m[rl].nb as u32 {
                                let r = m.read(m[rl].rels.add(q as i32));
                                let mut wanted = false;
                                if 0 < nb_chaps && !chaps.is_null() {
                                    let mut t = 0;
                                    while t < nb_chaps {
                                        if wanted || r.chapid as i32 == m[chaps.add(t)] {
                                            wanted = true;
                                        }
                                        t += 1;
                                    }
                                }
                                if wanted || nb_chaps < 1 {
                                    let x = m[list_of(m, ed, r.chapid as i32)].elems.add(r.elemid);
                                    if m[x].selected == 0 {
                                        m[x].selected = 1;
                                        count += 1;
                                    }
                                }
                                q += 1;
                            }
                            g += 1;
                        }
                        done = true;
                    }
                }
                if done {
                    m[e].work_flag = 1;
                }
                j += 1;
            }
            c += 1;
        }
        if count == 0 {
            return 1;
        }
    }
    recursive_select(m, ed, 0, Ptr32::NULL, depth + 1, nb_chaps, chaps);
    1
}

/// [soft 0x08138434 checkNSEStatus]: the light product's bookkeeping of what may be
/// edited, called by the routines that add a relation when `SI_lightLicense` is set (a
/// call the port does not make: the full product's path). Unless the dictionary's word at
/// +0x6c is set, and when both elements have a node (+8): a source whose node has bit
/// 0x1000 of its flag word makes the destination uneditable
/// (`SI_edSetUneditable (0x081074c0)`), but for relations 238 and 800 to 807; and for
/// relation 20002 a destination with that bit, when
/// `SI_edCheckForUneditable (0x08107a44)` says so, makes the source uneditable.
pub fn check_nse_status(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, src: Ptr32<Elem>, dst: Ptr32<Elem>, relid: i32, chap: i32) {
    if src.is_null() || dst.is_null() || m[ed].untold != 0 {
        return;
    }
    let (src_node, dst_node) = (m[src].aux_data.addr(), m[dst].aux_data.addr());
    if src_node == 0 || dst_node == 0 {
        return;
    }
    if m.u8(src_node + 0x17) & 0x10 != 0 && relid != 0xee && !(800..=0x327).contains(&relid) {
        outside::si_ed_set_uneditable(o, m, ed, dst);
    }
    if relid == 0x4e22 && m.u8(dst_node + 0x17) & 0x10 != 0 && outside::si_ed_check_for_uneditable(o, m, dst, chap) != 0 {
        outside::si_ed_set_uneditable(o, m, ed, src);
    }
}
