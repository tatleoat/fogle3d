// SPDX-License-Identifier: MIT
//! `nselections.c` (0x08346050 to 0x08347d13, 16 functions): named selections and tags.
//!
//! A named selection is a record of 12 bytes (`SI_NamedSelection`) behind a graph node of
//! kind 0xc and an element of the NSELECTIONS chapter. What it holds is in the element's
//! first forward relation list: one relation (id 0x2710; 0x2711 for a tag) to every
//! element that was selected when it was made. Selecting the node selects those.
//!
//! Ported here: the records' life, selecting the content, the tags on the models, and
//! the fix-up after a load. Not ported: the two dialogs with the list they show, the
//! debugging print-out and the two routines that read and write a relation's data in a
//! scene file (finding 0027 says why).

use crate::globals::*;
use crate::graflist::{
    find_gl_data, graflist_add_data, graflist_check_multi, graflist_count_selected, graflist_delete_grafnode, graflist_set_multi_selection, scene_list, select_grafnode, toggle_grafnode,
    unselect_grafnode,
};
use crate::outside::{self, Outside};
use crate::records::{NSelFcvData, NamedSelection};
use si_mem::{clib, CStr32, Mem, Opaque, Ptr32};
use si_types::dict::{ARelation, Elem, ElemDict, ElemList, RelList};
use si_types::model::Model;
use si_types::scene::GrafNode;

/// The relation from a named selection to each element it holds …
pub const REL_NSELECTION: u32 = 0x2710;
/// … and from a tag.
pub const REL_TAG: u32 = 0x2711;

/// The two messages of `nselectionReplace`, in the binary's `.data`. Held against the
/// binary by `re/tools/si-view/check_data.py`.
pub mod text {
    pub const NOT_CREATED: u32 = 0x086b_b540;
    pub const REMOVED: u32 = 0x086b_b580;
    pub const ALL: [(u32, &[u8]); 2] =
        [(NOT_CREATED, b"Named selection not created: no selected item found"), (REMOVED, b"Named selection removed: no selected item found")];
}

/// Writes the constant texts of this file into the image, where the binary has them.
pub fn init_nselection_text(m: &mut Mem) {
    for (addr, bytes) in text::ALL {
        m.put_cstr(addr, bytes);
    }
}

/// How many named selections the scene's list has [soft 0x08346050 nselectionCount]: the
/// nodes of kind 0xc, with `selected` only those whose selection word is not 0. The last
/// of them is stored through `last` (null when there is none).
pub fn nselection_count(m: &mut Mem, selected: i16, last: Ptr32<Ptr32<GrafNode>>) -> i32 {
    m[last] = Ptr32::NULL;
    let mut count = 0;
    let mut node = m[scene_list(m)].first;
    while !node.is_null() {
        if m[node].kind == 0xc && (selected == 0 || m[node].selection != 0) {
            count += 1;
            m[last] = node;
        }
        node = m[node].next;
    }
    count
}

/// A new named selection record [soft 0x083460a4 SI_nselectionAllocate]: 12 bytes from
/// `SI_malloc`, not tested for null. The name is the pointer handed in, not a copy; the
/// third word is 0; the first is the type when that is 0 or 1 and is **not written** for
/// any other type.
pub fn nselection_allocate(m: &mut Mem, name: CStr32, ty: i32) -> Ptr32<NamedSelection> {
    let p: Ptr32<NamedSelection> = Ptr32::new(m.si_malloc(0xc));
    m[p].name = name;
    m[p].unk_08 = 0;
    if ty == 0 {
        m[p].kind = 0;
    } else if ty == 1 {
        m[p].kind = 1;
    }
    p
}

/// Makes a named selection (type 0) or a tag [soft 0x083460f0 SI_nselectionCreate] and
/// answers its graph node: the record; its node in the scene's list (`SI_graflistAddData`,
/// kind 0xc, not selected); `nselcount` or, for any type but 0, `ntagcount` goes up by
/// one; a name record from `SI_dbListNodeAllocate (0x0812250c)` gets a copy of the name
/// (`strdup`) and its full name from `SI_dbFormatFullName (0x081223dc)`; the element is
/// added with `SI_elemdictAddElem (0x08132a5c)` (the current dictionary, the NSELECTIONS
/// chapter, name record, record, node, 0, 0); `SI_schematicAdjustNewNode (0x0834fe20)`
/// places the node.
pub fn nselection_create(m: &mut Mem, o: &mut dyn Outside, name: CStr32, ty: i32) -> Ptr32<GrafNode> {
    let nsel = nselection_allocate(m, name, ty);
    let list = scene_list(m);
    let node = graflist_add_data(m, o, list, nsel.cast(), 0xc, 0);
    if ty == 0 {
        m[NSELCOUNT] = m[NSELCOUNT].wrapping_add(1);
    } else {
        m[NTAGCOUNT] = m[NTAGCOUNT].wrapping_add(1);
    }
    let record = o.call(m, &outside::SI_DB_LIST_NODE_ALLOCATE, &[]);
    let copy = clib::strdup(m, name.addr());
    m.set_u32(record.wrapping_add(0x14), copy);
    let full = o.call(m, &outside::SI_DB_FORMAT_FULL_NAME, &[record]);
    m.set_u32(record.wrapping_add(0x24), full);
    let chap = m[SI_CID][cid::NSELECTIONS] as u32;
    let ed = current_dict(m);
    o.call(m, &outside::SI_ELEMDICT_ADD_ELEM, &[ed, chap, record, nsel.addr(), node.addr(), 0, 0]);
    let list = scene_list(m);
    o.call(m, &outside::SI_SCHEMATIC_ADJUST_NEW_NODE, &[list.addr(), node.addr()]);
    node
}

/// Disposes of a named selection through a pointer to its pointer
/// [soft 0x083461c4 nselectionDispose]: the relations of its element
/// (`SI_edGetElemFromNSelection (0x080fd5b8)`, then `SI_relRemoveRelations (0x081396d8)`
/// with the current dictionary, the element, the NSELECTIONS chapter, its index and 0);
/// its name is freed and cleared. Then **the pointer handed in is freed, not the record
/// it points to**. Its one caller hands in the address of a local.
pub fn nselection_dispose(m: &mut Mem, o: &mut dyn Outside, pp: Ptr32<Ptr32<NamedSelection>>) {
    let nsel = m[pp];
    let index = m.local(4);
    let elem = o.call(m, &outside::SI_ED_GET_ELEM_FROM_N_SELECTION, &[nsel.addr(), index]);
    if elem != 0 {
        let chap = m[SI_CID][cid::NSELECTIONS] as u32;
        let i = m.u32(index);
        let ed = current_dict(m);
        o.call(m, &outside::SI_REL_REMOVE_RELATIONS, &[ed, elem, chap, i, 0]);
    }
    m.release(4);
    let name = m[nsel].name;
    if !name.is_null() {
        m.free(name.addr());
        m[nsel].name = Ptr32::NULL;
    }
    if !pp.is_null() {
        m.free(pp.addr());
    }
}

/// One relation from a named selection to an element: `SI_elemdictAddUnCompleteRelation
/// (0x08132fb4)` with the dictionary, `SI_RD`, the NSELECTIONS chapter, the other
/// element's chapter, the selection's index, the other's index, the relation id and the
/// four words that follow.
#[allow(clippy::too_many_arguments)]
fn relate(m: &mut Mem, o: &mut dyn Outside, ed: u32, chap: u32, index: u32, other: u32, relid: u32, serial: u32) {
    let nsl = m[SI_CID][cid::NSELECTIONS] as u32;
    o.call(m, &outside::SI_ELEMDICT_ADD_UN_COMPLETE_RELATION, &[ed, SI_RD.addr(), nsl, chap, index, other, relid, 1, serial, 0, 1]);
}

/// Makes a named selection hold what is selected now
/// [soft 0x08346248 nselectionReplace]: `node` is its graph node; `items` and `clusters`
/// say what of the scene's list goes in, `curves` whether the selected curves of the
/// curve list (`SI_scngl`) do, `created` whether the selection has just been made (it
/// chooses the message and the answer).
///
/// The element's relations are removed first (`SI_edGetElemFromNSelection (0x080fd5b8)`,
/// `SI_relRemoveRelations (0x081396d8)`; the element is not tested for null). Then every
/// selected node of the scene's list that is no named selection, material, texture or
/// animation file (kinds 0xc, 0xe, 0xf, 0x10, 0x12), when `items` or `clusters` is set:
/// its element is looked up (`SI_elemdictFindElemFromGrafNode (0x080fcbec)`); for a
/// camera's own node (4) the index becomes `SI_edGetCamFromCamInt (0x080fd040)` of it and
/// for a light's own node (6) `SI_edGetLigFromLigInt (0x080fd150)`, each followed by a
/// `SI_elemdictGetElem (0x08132b50)` whose answer is not used; a relation 0x2710 (0x2711
/// when the record is a tag) is added. With `clusters`, a model node also gets a relation
/// 0x2710 to the element of its active cluster (`SI_klusterGetActive (0x0843b4b8)`,
/// `SI_elemdictFindElemByData (0x08134364)` in the CLUSTERS chapter).
///
/// With `curves`, every selected node of kind 9 of the curve list gets a relation 0x2710
/// (with 1 in its serial word) to its curve's element of the ANIMATION chapter.
///
/// When nothing was related, the node is deleted (`SI_graflistDeleteGrafnode`) and 1
/// answered, after `SI_statusDisplayMessage (0x083cca94)` with one of two texts and 2:
/// "not created" with `created`, and the record is **not** disposed of; "removed"
/// without it, followed by `nselectionDispose` **with the address of this routine's own
/// copy of the record pointer, which that routine then hands to `free`**: the harness
/// stops there and the image records the fault; the listing goes on, and so does the
/// port. Otherwise the answer is 1 when `created` is set and 0 when not.
pub fn nselection_replace(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<GrafNode>, items: i16, clusters: i16, curves: i16, created: i16) -> i32 {
    let index = m.local(4);
    let other = m.local(4);
    let chap = m.local(4);
    let nsel_at: Ptr32<Ptr32<NamedSelection>> = Ptr32::new(m.local(4));
    let nsel: Ptr32<NamedSelection> = m[node].data.cast();
    m[nsel_at] = nsel;
    let elem = o.call(m, &outside::SI_ED_GET_ELEM_FROM_N_SELECTION, &[nsel.addr(), index]);
    let nsl = m[SI_CID][cid::NSELECTIONS] as u32;
    let i = m.u32(index);
    let ed = current_dict(m);
    o.call(m, &outside::SI_REL_REMOVE_RELATIONS, &[ed, elem, nsl, i, 0]);
    let mut count = 0;
    let mut at = m[scene_list(m)].first;
    while !at.is_null() && m.fault().is_none() {
        if m[at].selection != 0 && !matches!(m[at].kind, 0xc | 0xe | 0xf | 0x10 | 0x12) && (items != 0 || clusters != 0) {
            o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[at.addr(), chap, other]);
            let kind = m[at].kind;
            count += 1;
            if kind == 4 || kind == 6 {
                let (routine, row) = if kind == 4 { (&outside::SI_ED_GET_CAM_FROM_CAM_INT, cid::CAMERAS) } else { (&outside::SI_ED_GET_LIG_FROM_LIG_INT, cid::LIGHTS) };
                let v = m.u32(other);
                let v = o.call(m, routine, &[v]);
                m.set_u32(other, v);
                let c = m[SI_CID][row] as u32;
                let ed = current_dict(m);
                o.call(m, &outside::SI_ELEMDICT_GET_ELEM, &[ed, c, v]);
            }
            let nsel = m[nsel_at];
            let relid = if m[nsel].kind == 1 { REL_TAG } else { REL_NSELECTION };
            let (c, i, v) = (m.u32(chap), m.u32(index), m.u32(other));
            let ed = current_dict(m);
            relate(m, o, ed, c, i, v, relid, 0);
            if clusters != 0 && (m[at].kind.wrapping_sub(2) as u32) <= 1 {
                let model = m[at].data.addr();
                let active = o.call(m, &outside::SI_KLUSTER_GET_ACTIVE, &[model]);
                let cls = m[SI_CID][cid::CLUSTERS] as u32;
                let ed = current_dict(m);
                let v = o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[ed, cls, active]);
                m.set_u32(other, v);
                let cls = m[SI_CID][cid::CLUSTERS] as u32;
                let i = m.u32(index);
                let ed = current_dict(m);
                relate(m, o, ed, cls, i, v, REL_NSELECTION, 0);
            }
        }
        at = m[at].next;
    }
    if curves != 0 {
        let scngl = m[SI_SCNGL];
        let mut at = m[scngl].first;
        while !at.is_null() && m.fault().is_none() {
            if m[at].selection != 0 && m[at].kind == 9 {
                let data = m[at].data.addr();
                let ani = m[SI_CID][cid::ANIMATION] as u32;
                let ed = current_dict(m);
                let v = o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[ed, ani, data]);
                m.set_u32(other, v);
                let ani = m[SI_CID][cid::ANIMATION] as u32;
                let i = m.u32(index);
                count += 1;
                let ed = current_dict(m);
                relate(m, o, ed, ani, i, v, REL_NSELECTION, 1);
            }
            at = m[at].next;
        }
    }
    let answer = if count == 0 {
        if created != 0 {
            o.call(m, &outside::SI_STATUS_DISPLAY_MESSAGE, &[text::NOT_CREATED, 2]);
        } else {
            o.call(m, &outside::SI_STATUS_DISPLAY_MESSAGE, &[text::REMOVED, 2]);
            nselection_dispose(m, o, nsel_at);
        }
        let list = scene_list(m);
        graflist_delete_grafnode(m, o, list, node);
        1
    } else {
        (created != 0) as i32
    };
    m.release(16);
    answer
}

/// Selects, unselects or toggles what a named selection holds
/// [soft 0x083465a4 SI_nselectionSelectContent]: `oper` 0 toggles, 1 selects, 2
/// unselects; any other value only does what comes before and after. Nothing for a null
/// node or one that is no named selection.
///
/// 1. The node's own selection word and its three selection flags are cleared; in single
///    selection the scene's list loses its active node (both places).
/// 2. `SI_dopeFlagsSet (0x081fa5d0)` (1, 1, 0, 0). The element is looked up
///    (`SI_elemdictFindElemFromGrafNode (0x080fcbec)`); without one the routine ends.
/// 3. Its first forward relation list is counted: relations to the ANIMATION chapter, and
///    relations to any chapter but that one and CLUSTERS. When either count is above 1,
///    the scene's list and the curve list (`SI_scngl`) go to multiple selection
///    (`SI_graflistSetMultiSelection`, each only when it is in single selection).
/// 4. With a relation to ANIMATION and `SI_clearSelectedFcv` set: `SI_fcvScnglClear
///    (0x0823c948)`.
/// 5. Every relation of the list, the element at its other end fetched with
///    `SI_elemdictGetElem (0x08132b50)` (not tested for null):
///    - to ANIMATION, an element above level 0: a curve. Its node in the curve list
///      (`SI_findGlData`, kind 9) is the one acted on; when it has none,
///      `SI_fcvScnglAdd (0x0823c7a8)` (curve, the prefix of the element's name record, 1)
///      makes one, and it is looked up again;
///    - to CLUSTERS: the element related to it by 0x2bc in the MODELS chapter
///      (`SI_elemdictFindRelatedElem (0x0813508c)`), when there is one, has the cluster
///      made active: `SI_klusterActivate (0x0843b4f0)` (that element's object, the
///      cluster, 1, 1);
///    - anything else: the element's graph node (+8) in the scene's list is acted on.
///
///    Acting is `SI_toggleGrafnode`, `SI_selectGrafnode` (both with mode 0) or
///    `SI_unselectGrafnode`.
/// 6. `SI_fcvScrollBuild (0x083978d8)` when a curve node was made.
/// 7. When the manipulation word at +0x30 is set: the selected nodes of both lists are
///    counted (`SI_graflistCountSelected`, the not-editable ones included) and, when
///    there is at most one in all, `SI_graflistCheckMulti` sets the selection mode.
///
/// The reduced product asks `SI_checkFcurveNSel` first and may end there; that is behind
/// `SI_lightLicense` and left out (the full product's path).
pub fn nselection_select_content(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<GrafNode>, oper: i32) {
    if node.is_null() || m[node].kind != 0xc {
        return;
    }
    m[node].selection = 0;
    m[node].flags &= 0x1fff;
    let list = scene_list(m);
    if m[list].multi == 0 {
        m[list].active = Ptr32::NULL;
        let list = scene_list(m);
        m[list].active2 = Ptr32::NULL;
    }
    o.call(m, &outside::SI_DOPE_FLAGS_SET, &[1, 1, 0, 0]);
    let index = m.local(4);
    let chap = m.local(4);
    let elem: Ptr32<Elem> = Ptr32::new(o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]));
    m.release(8);
    if elem.is_null() {
        return;
    }
    let rels: Ptr32<RelList> = m[elem].forward.groups;
    let (mut others, mut curves) = (0, 0);
    let n = m[rels].nb;
    if n > 0 {
        let (ani, cls) = (m[SI_CID][cid::ANIMATION], m[SI_CID][cid::CLUSTERS]);
        let first: Ptr32<ARelation> = m[rels].rels;
        for k in 0..n {
            let c = m[first.add(k)].chapid as i32;
            if c == ani {
                curves += 1;
            } else if c != cls {
                others += 1;
            }
            if m.fault().is_some() {
                return;
            }
        }
    }
    if others > 1 || curves > 1 {
        let list = scene_list(m);
        if m[list].multi == 0 {
            let list = scene_list(m);
            graflist_set_multi_selection(m, o, list, 1);
        }
        let scngl = m[SI_SCNGL];
        if m[scngl].multi == 0 {
            graflist_set_multi_selection(m, o, scngl, 1);
        }
    }
    if curves > 0 && m[SI_CLEAR_SELECTED_FCV] != 0 {
        o.call(m, &outside::SI_FCV_SCNGL_CLEAR, &[]);
    }
    let mut made = false;
    let mut k: i32 = 0;
    while k < m[rels].nb && m.fault().is_none() {
        let rel: Ptr32<ARelation> = m[rels].rels.add(k);
        let (c, e) = (m[rel].chapid as i32 as u32, m[rel].elemid as u32);
        let ed = current_dict(m);
        let other: Ptr32<Elem> = Ptr32::new(o.call(m, &outside::SI_ELEMDICT_GET_ELEM, &[ed, c, e]));
        if m[rel].chapid as i32 == m[SI_CID][cid::ANIMATION] && m[other].hier_level > 0 {
            let (scngl, data) = (m[SI_SCNGL], m[other].data);
            let mut g = find_gl_data(m, scngl, data, 9);
            if g.is_null() {
                let record = m[other].node;
                let prefix = m[record].prefix.addr();
                let data = m[other].data.addr();
                o.call(m, &outside::SI_FCV_SCNGL_ADD, &[data, prefix, 1]);
                let (scngl, data) = (m[SI_SCNGL], m[other].data);
                g = find_gl_data(m, scngl, data, 9);
                made = true;
            }
            if !g.is_null() {
                let scngl = m[SI_SCNGL];
                act(m, o, scngl, g, oper);
            }
        } else if m[rel].chapid as i32 == m[SI_CID][cid::CLUSTERS] {
            let found = m.local(4);
            let models = m[SI_CID][cid::MODELS] as u32;
            let ed = current_dict(m);
            let owner = o.call(m, &outside::SI_ELEMDICT_FIND_RELATED_ELEM, &[ed, other.addr(), 0, 0, 0x2bc, models, found, found]);
            m.release(4);
            if owner != 0 {
                let model = m.u32(owner.wrapping_add(4));
                let cluster = m[other].data.addr();
                o.call(m, &outside::SI_KLUSTER_ACTIVATE, &[model, cluster, 1, 1]);
            }
        } else {
            let g: Ptr32<GrafNode> = m[other].aux_data.cast();
            if !g.is_null() {
                let list = scene_list(m);
                act(m, o, list, g, oper);
            }
        }
        k += 1;
    }
    if curves > 0 && made {
        o.call(m, &outside::SI_FCV_SCROLL_BUILD, &[]);
    }
    if m.i16(SI_GLB_MANIP.addr() + GLB_MANIP_30) != 0 {
        let list = scene_list(m);
        let a = graflist_count_selected(m, list, Ptr32::NULL, 1);
        let scngl = m[SI_SCNGL];
        let b = graflist_count_selected(m, scngl, Ptr32::NULL, 1);
        if a.wrapping_add(b) <= 1 {
            graflist_check_multi(m, o);
        }
    }
}

/// The three operations of `SI_nselectionSelectContent` on one node of a list.
fn act(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<si_types::scene::GrafList>, g: Ptr32<GrafNode>, oper: i32) {
    match oper {
        0 => {
            toggle_grafnode(m, o, list, g, 0);
        }
        1 => {
            select_grafnode(m, o, list, g, 0);
        }
        2 => {
            unselect_grafnode(m, o, list, g);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- tags

/// Takes the tag numbers off every model of the scene's list
/// [soft 0x08347290 SI_tagMdlRm]: a model node whose model has an array at +0x35c has it
/// freed, and the count at +0x358 and the pointer cleared.
pub fn tag_mdl_rm(m: &mut Mem) {
    let mut node = m[scene_list(m)].first;
    while !node.is_null() && m.fault().is_none() {
        if (m[node].kind.wrapping_sub(2) as u32) <= 1 {
            let model: Ptr32<Model> = m[node].data.cast();
            let tags = m[model].unk_358[1];
            if tags != 0 {
                m.free(tags);
                m[model].unk_358[0] = 0;
                m[model].unk_358[1] = 0;
            }
        }
        node = m[node].next;
    }
}

/// The chapter `row` of `SI_cid` names in the current dictionary.
fn chapter(m: &Mem, row: usize) -> Ptr32<ElemList> {
    let ed: Ptr32<ElemDict> = Ptr32::new(current_dict(m));
    Ptr32::new(m[ed].lists.addr().wrapping_add((m[SI_CID][row] as u32) << 6))
}

/// The number of a tag among the tags [soft 0x0834746c tagValid]: how many elements in
/// use of the NSELECTIONS chapter before the one at `index` are tags (their record's
/// first word is 1). 0 when `index` is not reached, which includes an element at `index`
/// that is deleted.
pub fn tag_valid(m: &Mem, index: i32) -> i32 {
    let chapter = chapter(m, cid::NSELECTIONS);
    let n = m[chapter].nb_elems;
    let mut tags = 0;
    let mut elem = m[chapter].elems;
    let mut k: i32 = 0;
    while k < n {
        if m[elem].status == 0 {
            if k == index {
                return tags;
            }
            let record: Ptr32<NamedSelection> = m[elem].data.cast();
            if m[record].kind == 1 {
                tags += 1;
            }
        }
        elem = elem.add(1);
        k += 1;
        if m.fault().is_some() {
            break;
        }
    }
    0
}

/// Gives every model the numbers of the tags it carries
/// [soft 0x083472f8 SI_tagMdlUpdate] and answers how many tags the scene has.
///
/// For every element in use of the MODELS chapter: the relations 0x2711 of its first
/// backward list are counted; when there are any, the model gets the count at +0x358 and
/// an array of that many words from `SI_malloc` at +0x35c (not tested for null), filled
/// with `tagValid` of each such relation's other end, in the list's order. The answer is
/// the number of elements in use of the NSELECTIONS chapter whose record is a tag.
pub fn tag_mdl_update(m: &mut Mem) -> i32 {
    let models = chapter(m, cid::MODELS);
    let mut k: i32 = 0;
    while k < m[models].nb_elems && m.fault().is_none() {
        let elem: Ptr32<Elem> = m[models].elems.add(k);
        k += 1;
        if m[elem].status != 0 {
            continue;
        }
        let rels: Ptr32<RelList> = m[elem].backward.groups;
        let n = m[rels].nb;
        let first: Ptr32<ARelation> = m[rels].rels;
        let mut tags = 0;
        for j in 0..n.max(0) {
            if m[first.add(j)].relid as u32 == REL_TAG {
                tags += 1;
            }
        }
        let mut model: Ptr32<Model> = Ptr32::NULL;
        if tags != 0 {
            model = m[elem].data.cast();
            m[model].unk_358[0] = tags as u32;
            let array = m.si_malloc((tags as u32).wrapping_mul(4));
            m[model].unk_358[1] = array;
        }
        let mut filled: u32 = 0;
        let mut j: i32 = 0;
        while j < m[rels].nb && m.fault().is_none() {
            let rel: Ptr32<ARelation> = m[rels].rels.add(j);
            if m[rel].relid as u32 == REL_TAG {
                let number = tag_valid(m, m[rel].elemid);
                let array = m[model].unk_358[1];
                m.set_i32(array.wrapping_add(filled.wrapping_mul(4)), number);
                filled += 1;
            }
            j += 1;
        }
    }
    let nsl = chapter(m, cid::NSELECTIONS);
    let n = m[nsl].nb_elems;
    let mut tags = 0;
    if n > 0 {
        let mut elem = m[nsl].elems;
        let mut left = n;
        loop {
            if m[elem].status == 0 {
                let record: Ptr32<NamedSelection> = m[elem].data.cast();
                if m[record].kind == 1 {
                    tags += 1;
                }
            }
            elem = elem.add(1);
            left -= 1;
            if left == 0 || m.fault().is_some() {
                break;
            }
        }
    }
    tags
}

// ---------------------------------------------------------------- after a load

/// One step of the walk over the elements a named selection is related to by 0x2710 in
/// the ANIMATION chapter: the first (`SI_elemdictFindRelatedElem (0x0813508c)`) or the
/// next (`SI_elemdictFindNextRelatedElem (0x08135154)`).
fn related_curve(m: &mut Mem, o: &mut dyn Outside, next: bool, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, chap_out: u32, index_out: u32) -> Ptr32<Elem> {
    let routine = if next { &outside::SI_ELEMDICT_FIND_NEXT_RELATED_ELEM } else { &outside::SI_ELEMDICT_FIND_RELATED_ELEM };
    let ani = m[SI_CID][cid::ANIMATION] as u32;
    Ptr32::new(o.call(m, routine, &[ed.addr(), elem.addr(), 1, 0, REL_NSELECTION, ani, chap_out, index_out]))
}

/// Puts the named selections of a dictionary that has just been loaded in order
/// [soft 0x0834757c SI_nselectionUpdate]. Answers 0 for a null dictionary, else 1.
///
/// First walk, every new element in use (status 0, "new" word 1) of the NSELECTIONS
/// chapter:
///
/// - one that has a relation 0x2711 (`SI_elemdictFindRelatedElem (0x0813508c)` in any
///   chapter) has its record made a tag;
/// - its relations 0x2710 to the ANIMATION chapter are walked. One that leads to an
///   element at level 0 (the animation, not a curve of it) may carry, on the relation the
///   walk stands at (`SI_elemdictGetCurRelation (0x08134db0)`), a record of four words
///   read from the scene file: the chapter and index of an element, a curve type and a
///   number. The record is taken off the relation. Among the curves of that element
///   (`SI_edFindFcv (0x080edb20)`, `SI_edFindNextFcv (0x080edb74)`, in the group
///   `SI_fcvGetRelGrpFromType (0x0810f624)` gives for the type and with the first word
///   `SI_fcvGetRelInfo (0x0810f698)` stores) the first of that type is looked for and
///   then the one as many places further as the number says, as long as the type stays
///   the same; when it is found, a relation 0x2710 from the selection to that curve's
///   element (`SI_elemdictFindElemByData (0x08134364)`) is added
///   (`SI_elemdictAddUnCompleteRelation (0x08132fb4)`). The record is freed, and the
///   walk **starts again from the first** related element. Without a record, or at
///   another level, the walk goes on to the next.
///
///   When the curves run out in the second search (the number is larger than the curves
///   there are), the original compares the type through the null it was just answered:
///   the image records the fault.
///
/// Second walk, the same elements: every relation 0x2710 to an element of the ANIMATION
/// chapter at level 0 is removed (`SI_elemdictRemoveBackwardRelId (0x08133b18)` with the
/// dictionary, the chapter, the index found, 0x2710 and −1) and the walk starts again
/// from the first; an element met twice in a row is passed over.
///
/// Last `SI_fcvScrollBuild (0x083978d8)`.
pub fn nselection_update(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>) -> i32 {
    if ed.is_null() {
        return 0;
    }
    let chapter: Ptr32<ElemList> = Ptr32::new(m[ed].lists.addr().wrapping_add((m[SI_CID][cid::NSELECTIONS] as u32) << 6));
    let index = m.local(4);
    let chap = m.local(4);
    let index2 = m.local(4);
    let info = m.local(12);
    let mut i: i32 = 0;
    while i < m[chapter].nb_elems && m.fault().is_none() {
        let elem: Ptr32<Elem> = m[chapter].elems.add(i);
        if m[elem].status == 0 && m[elem].new_flag == 1 {
            let tagged = o.call(m, &outside::SI_ELEMDICT_FIND_RELATED_ELEM, &[ed.addr(), elem.addr(), 1, 0, REL_TAG, 0xffff_ffff, chap, index]);
            if tagged != 0 {
                let record: Ptr32<NamedSelection> = m[elem].data.cast();
                m[record].kind = 1;
            }
            let mut related = related_curve(m, o, false, ed, elem, chap, index2);
            while !related.is_null() && m.fault().is_none() {
                if m[related].hier_level != 0 {
                    related = related_curve(m, o, true, ed, elem, chap, index);
                    continue;
                }
                let rel: Ptr32<ARelation> = Ptr32::new(o.call(m, &outside::SI_ELEMDICT_GET_CUR_RELATION, &[ed.addr(), elem.addr(), 1, 0]));
                let data: Ptr32<NSelFcvData> = if rel.is_null() { Ptr32::NULL } else { m[rel].data.cast() };
                if data.is_null() {
                    related = related_curve(m, o, true, ed, elem, chap, index);
                    continue;
                }
                m[rel].data = Ptr32::NULL;
                let (c, e) = (m[data].chap as u32, m[data].elem as u32);
                let owner = o.call(m, &outside::SI_ELEMDICT_GET_ELEM, &[ed.addr(), c, e]);
                let ty = m[data].fcv_type as u32;
                o.call(m, &outside::SI_FCV_GET_REL_INFO, &[ty, info + 8, info + 4, info]);
                let ty = m[data].fcv_type as u32;
                let group = o.call(m, &outside::SI_FCV_GET_REL_GRP_FROM_TYPE, &[ty]);
                let key = m.u32(info + 8);
                let mut curve = o.call(m, &outside::SI_ED_FIND_FCV, &[owner, group, key]);
                // the first curve of the type …
                while curve != 0 && m.i32(curve) != m[data].fcv_type {
                    let key = m.u32(info + 8);
                    curve = o.call(m, &outside::SI_ED_FIND_NEXT_FCV, &[owner, group, key]);
                }
                // … and the one `nth` places further, while the type stays
                if curve != 0 && m[data].nth != 0 {
                    let mut step = 0;
                    let wanted = m[data].nth;
                    let mut same = true;
                    while step != wanted {
                        let key = m.u32(info + 8);
                        curve = o.call(m, &outside::SI_ED_FIND_NEXT_FCV, &[owner, group, key]);
                        step += 1;
                        if curve == 0 {
                            break;
                        }
                        if m.i32(curve) != m[data].fcv_type {
                            same = false;
                            break;
                        }
                        if step == m[data].nth {
                            break;
                        }
                    }
                    // (through null when the curves ran out: the original's read)
                    if !same || m.i32(curve) != m[data].fcv_type {
                        curve = 0;
                    }
                }
                if curve != 0 {
                    let ani = m[SI_CID][cid::ANIMATION] as u32;
                    let found = o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[ed.addr(), ani, curve]);
                    let ani = m[SI_CID][cid::ANIMATION] as u32;
                    relate(m, o, ed.addr(), ani, i as u32, found, REL_NSELECTION, 0);
                }
                m.free(data.addr());
                related = related_curve(m, o, false, ed, elem, chap, index);
            }
        }
        i += 1;
    }
    let mut i: i32 = 0;
    while i < m[chapter].nb_elems && m.fault().is_none() {
        let elem: Ptr32<Elem> = m[chapter].elems.add(i);
        if m[elem].status == 0 && m[elem].new_flag == 1 {
            let mut last: Ptr32<Elem> = Ptr32::NULL;
            let mut related = related_curve(m, o, false, ed, elem, chap, index2);
            while !related.is_null() && m.fault().is_none() {
                if m[related].hier_level == 0 && related != last {
                    let ani = m[SI_CID][cid::ANIMATION] as u32;
                    let found = m.u32(index2);
                    o.call(m, &outside::SI_ELEMDICT_REMOVE_BACKWARD_REL_ID, &[ed.addr(), ani, found, REL_NSELECTION, 0xffff_ffff]);
                    last = related;
                    related = related_curve(m, o, false, ed, elem, chap, index2);
                } else {
                    related = related_curve(m, o, true, ed, elem, chap, index2);
                }
            }
        }
        i += 1;
    }
    m.release(24);
    o.call(m, &outside::SI_FCV_SCROLL_BUILD, &[]);
    1
}

/// A named selection's object word as the record it is.
#[allow(dead_code)]
fn record_of(m: &Mem, node: Ptr32<GrafNode>) -> Ptr32<NamedSelection> {
    let data: Ptr32<Opaque> = m[node].data;
    data.cast()
}
