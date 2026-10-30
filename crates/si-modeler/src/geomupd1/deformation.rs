// SPDX-License-Identifier: MIT
//! `deformation.c` (and the one `edtools.c` routine it needs): removing a lattice from a model, and the surface point
//! and tangent. Finding 0034, section "geomupd1".

use crate::extrusion::ModelSide;
use si_mem::{Mem, Outside, Ptr32, Routine};
use si_types::model::Model;

/// `SI_elemdictFindElemByData (0x08134364)`, `elemdict.c` (ported in `si-dict` on its own world).
pub const SI_ELEMDICT_FIND_ELEM_BY_DATA: Routine = Routine::new("SI_elemdictFindElemByData", 0x0813_4364);
/// `SI_elemdictGetElem (0x08132b50)`.
pub const SI_ELEMDICT_GET_ELEM: Routine = Routine::new("SI_elemdictGetElem", 0x0813_2b50);
/// `SI_dbSortTblSetDirty (0x0813425c)`.
pub const SI_DB_SORT_TBL_SET_DIRTY: Routine = Routine::new("SI_dbSortTblSetDirty", 0x0813_425c);
/// `SI_dbListNodeFree (0x08122450)`.
pub const SI_DB_LIST_NODE_FREE: Routine = Routine::new("SI_dbListNodeFree", 0x0812_2450);
/// `SI_dbIconFree (0x08135a8c)`.
pub const SI_DB_ICON_FREE: Routine = Routine::new("SI_dbIconFree", 0x0813_5a8c);
/// `SI_relRemoveRelations (0x081396d8)`.
pub const SI_REL_REMOVE_RELATIONS: Routine = Routine::new("SI_relRemoveRelations", 0x0813_96d8);
/// `SI_elemdictCheckDuplElemName (0x08134804)`.
pub const SI_ELEMDICT_CHECK_DUPL_ELEM_NAME: Routine = Routine::new("SI_elemdictCheckDuplElemName", 0x0813_4804);
/// `SI_elemdictGetNewName (0x0813697c)`.
pub const SI_ELEMDICT_GET_NEW_NAME: Routine = Routine::new("SI_elemdictGetNewName", 0x0813_697c);
/// `SI_dbFormatFullName (0x081223dc)`.
pub const SI_DB_FORMAT_FULL_NAME: Routine = Routine::new("SI_dbFormatFullName", 0x0812_23dc);
/// `SI_edSoftElemChangeName (0x080fe96c)`.
pub const SI_ED_SOFT_ELEM_CHANGE_NAME: Routine = Routine::new("SI_edSoftElemChangeName", 0x080f_e96c);
/// `SI_elemdictSortAnimElements (0x08137980)`.
pub const SI_ELEMDICT_SORT_ANIM_ELEMENTS: Routine = Routine::new("SI_elemdictSortAnimElements", 0x0813_7980);
/// `SI_elemdictUpdateModelHierarchy (0x080fd698)`.
pub const SI_ELEMDICT_UPDATE_MODEL_HIERARCHY: Routine = Routine::new("SI_elemdictUpdateModelHierarchy", 0x080f_d698);
/// `SI_modelDeleteLatticeAnimation (0x0843ffa8)` (ported in `si-deform` behind its own world).
pub const SI_MODEL_DELETE_LATTICE_ANIMATION: Routine = Routine::new("SI_modelDeleteLatticeAnimation", 0x0843_ffa8);
/// `SI_latticeDispose (0x0843dff0)` (ported in `si-deform`).
pub const SI_LATTICE_DISPOSE: Routine = Routine::new("SI_latticeDispose", 0x0843_dff0);
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_ELEM_DICTS: u32 = 0x0867_931c;
const SI_ADD_IN_DICTIONARY: u32 = 0x0867_92b0;
/// `SI_cid.ANIMATION`, `SI_cid.MODELS`, `SI_cid.LATTICES` (the chapter numbers).
const CID_ANIMATION: u32 = 0x0878_1660;
const CID_MODELS: u32 = 0x0878_1680;
const CID_LATTICES: u32 = 0x0878_1674;
/// A short word of the data segment (0x08781596) `SI_edElementSetDeleteFlag` tests last.
const ED_FLAG: u32 = 0x0878_1596;

/// The address of the current element dictionary: `SI_elemDicts + 0x38 * SI_procData[0]` (`(n * 8 - n) * 0x10`).
fn current_dict(m: &Mem) -> u32 {
    let n = m.u32(m.u32(SI_PROC_DATA));
    m.u32(SI_ELEM_DICTS).wrapping_add(n.wrapping_mul(8).wrapping_sub(n).wrapping_mul(0x10))
}

/// [soft 0x080fd910 SI_edElementSetDeleteFlag] `(elem, chapter, index, delete, name_a, name_b, hard)`: the editor's
/// deletion mark of a dictionary element; nothing when `elem` is null or `SI_addInDictionary` is 0. The chapter's
/// sort table is marked stale first. `delete` set: with `hard` a live element (status +0x48 = 1) is freed whole
/// (status 2, its name node and icon freed, +0, +4, +8 and +0x70 cleared); with `hard` clear an element in use
/// (status 0) has its relations removed (both directions), takes the status 1 (deleted) with the index at +0x50 and 0
/// at +0x54. `delete` clear: a deleted element (status 1) is brought back with the two words `name_a`, `name_b` at +4
/// and +8; when its name now duplicates another (`SI_elemdictCheckDuplElemName`) it is given a new one
/// (`SI_elemdictGetNewName`, status 2 meanwhile, the old name and full name freed, a `strdup` of the new one,
/// `SI_dbFormatFullName`, `SI_edSoftElemChangeName`) and its status is 0. After either, the models chapter has its
/// hierarchy renewed, the animation chapter its elements re-sorted, and the dictionary's word at +0x6e set. At the
/// end (also when the status test failed) the dictionary's words +0x14 and +0x5a are set to 1, and +0x6e when the
/// word at 0x08781596 is 0. The dictionary routines it calls are `si-dict`'s and answer through `o`.
pub fn ed_element_set_delete_flag(m: &mut Mem, o: &mut dyn Outside, elem: u32, chapter: u32, index: u32, delete: i16, name_a: u32, name_b: u32, hard: i16) {
    if elem == 0 || m.i16(SI_ADD_IN_DICTIONARY) == 0 {
        return;
    }
    let lists = |m: &Mem| m.u32(current_dict(m) + 4).wrapping_add(chapter.wrapping_shl(6));
    let l = lists(m);
    o.call(m, SI_DB_SORT_TBL_SET_DIRTY, &[l]);
    if delete == 0 {
        if m.i16(elem + 0x48) == 1 {
            m.set_u32(elem + 4, name_a);
            m.set_u32(elem + 8, name_b);
            let d = current_dict(m);
            let dup = o.call(m, SI_ELEMDICT_CHECK_DUPL_ELEM_NAME, &[d, chapter, elem]).eax;
            if dup != u32::MAX {
                m.set_i16(elem + 0x48, 2);
                let d = current_dict(m);
                let node = m.u32(elem);
                let new_name = o.call(m, SI_ELEMDICT_GET_NEW_NAME, &[d, chapter, m.u32(node + 0x14)]).eax;
                m.set_i16(elem + 0x48, 0);
                let node = m.u32(elem);
                if m.u32(node + 0x14) != 0 {
                    m.free(m.u32(node + 0x14));
                    m.set_u32(m.u32(elem) + 0x14, 0);
                }
                let node = m.u32(elem);
                if m.u32(node + 0x24) != 0 {
                    m.free(m.u32(node + 0x24));
                    m.set_u32(m.u32(elem) + 0x24, 0);
                }
                let copy = m.strdup(new_name);
                m.set_u32(m.u32(elem) + 0x14, copy);
                let full = o.call(m, SI_DB_FORMAT_FULL_NAME, &[m.u32(elem)]).eax;
                m.set_u32(m.u32(elem) + 0x24, full);
                o.call(m, SI_ED_SOFT_ELEM_CHANGE_NAME, &[elem, chapter, new_name]);
            }
            m.set_i16(elem + 0x48, 0);
            finish_chapter(m, o, chapter);
        }
    } else {
        if hard != 0 && m.i16(elem + 0x48) == 1 {
            m.set_i16(elem + 0x48, 2);
            o.call(m, SI_DB_LIST_NODE_FREE, &[m.u32(elem)]);
            o.call(m, SI_DB_ICON_FREE, &[elem]);
            m.set_u32(elem + 4, 0);
            m.set_u32(elem + 8, 0);
            m.set_u32(elem, 0);
            m.set_u32(elem + 0x70, 0);
        } else if hard == 0 && m.i16(elem + 0x48) == 0 {
            let d = current_dict(m);
            o.call(m, SI_REL_REMOVE_RELATIONS, &[d, elem, chapter, index, 1]);
            let d = current_dict(m);
            o.call(m, SI_REL_REMOVE_RELATIONS, &[d, elem, chapter, index, 0]);
            m.set_u32(elem + 0x50, index);
            m.set_i16(elem + 0x48, 1);
            m.set_u32(elem + 0x54, 0);
            finish_chapter(m, o, chapter);
        }
    }
    let d = current_dict(m);
    m.set_u32(d + 0x14, 1);
    m.set_i16(d + 0x5a, 1);
    if m.i16(ED_FLAG) == 0 {
        m.set_i16(d + 0x6e, 1);
    }
}

/// The tail the two live paths share: the models chapter renews its hierarchy, the animation chapter re-sorts, and
/// the dictionary's word at +0x6e is set.
fn finish_chapter(m: &mut Mem, o: &mut dyn Outside, chapter: u32) {
    if chapter == m.u32(CID_MODELS) {
        let d = current_dict(m);
        o.call(m, SI_ELEMDICT_UPDATE_MODEL_HIERARCHY, &[d]);
    } else if chapter == m.u32(CID_ANIMATION) {
        let l = m.u32(current_dict(m) + 4).wrapping_add(chapter.wrapping_shl(6));
        o.call(m, SI_ELEMDICT_SORT_ANIM_ELEMENTS, &[l]);
    }
    let d = current_dict(m);
    m.set_i16(d + 0x6e, 1);
}

/// [soft 0x08103974 SI_edDisposeLattice] `(model, which)`: marks the dictionary element of the model's first lattice
/// (the set `which`, 0 the lattices at +0x50, otherwise at +0x58) deleted, in both senses the editor knows
/// ([`ed_element_set_delete_flag`] with `hard` 0, then 1): the element is found by its object, the first lattice of
/// the set (`SI_elemdictFindElemByData`, `SI_elemdictGetElem`), in the lattices chapter (`SI_cid.LATTICES`).
pub fn ed_dispose_lattice(m: &mut Mem, o: &mut dyn Outside, model: u32, which: i16) {
    let set = if which == 0 { m.u32(model + 0x50) } else { m.u32(model + 0x58) };
    let first = m.u32(set);
    let chapter = m.u32(CID_LATTICES);
    let d = current_dict(m);
    let index = o.call(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[d, chapter, first]).eax;
    let d = current_dict(m);
    let chapter = m.u32(CID_LATTICES);
    let elem = o.call(m, SI_ELEMDICT_GET_ELEM, &[d, chapter, index]).eax;
    let chapter = m.u32(CID_LATTICES);
    ed_element_set_delete_flag(m, o, elem, chapter, index, 1, 0, 0, 0);
    let chapter = m.u32(CID_LATTICES);
    ed_element_set_delete_flag(m, o, elem, chapter, index, 1, 0, 0, 1);
}

/// [soft 0x08153714 SI_modelRemoveLattice] `(model, which, update)`: takes every lattice of the set `which` (0 the
/// lattices at +0x50 with their count at +0x4c, otherwise the set at +0x58 with the count at +0x54) off the model.
/// Answers 0 when the model is null or the set is empty. The set's animation is deleted
/// (`SI_modelDeleteLatticeAnimation`), its dictionary elements marked deleted ([`ed_dispose_lattice`]), each lattice
/// disposed (`SI_latticeDispose`), the array freed and its pointer and count cleared. With `update` set the
/// dependencies of the model's root are renewed (`SI_modelUpdateDependency(root, 0)`). Answers 1.
pub fn model_remove_lattice(m: &mut Mem, o: &mut dyn Outside, model: u32, which: i16, update: i16) -> u32 {
    if model == 0 {
        return 0;
    }
    let (count_at, array_at) = if which == 0 { (0x4c, 0x50) } else { (0x54, 0x58) };
    if m.i16(model + count_at) == 0 {
        return 0;
    }
    o.call(m, SI_MODEL_DELETE_LATTICE_ANIMATION, &[model, which as i32 as u32]);
    ed_dispose_lattice(m, o, model, which);
    let mut i: i16 = 0;
    if m.i16(model + count_at) != 0 {
        loop {
            let slot = m.u32(model + array_at).wrapping_add((i as i32 as u32).wrapping_mul(4));
            o.call(m, SI_LATTICE_DISPOSE, &[slot]);
            i = i.wrapping_add(1);
            if (i as i32) >= m.u16(model + count_at) as i32 {
                break;
            }
        }
    }
    if m.u32(model + array_at) != 0 {
        m.free(m.u32(model + array_at));
        m.set_u32(model + array_at, 0);
    }
    m.set_u32(model + array_at, 0);
    m.set_i16(model + count_at, 0);
    if update != 0 {
        let root = si_model::model_image::tree::root(m, Ptr32::<Model>::new(model)).addr();
        si_model::model_image::readjust::update_dependency(m, &mut ModelSide(o), Ptr32::<Model>::new(root), 0);
    }
    1
}

/// `SI_nurbsCrv` (0x08688c60), the curve `SI_svtAllocOptimBegin` made.
pub use si_model::model_image::patch_fit::NURBS_CRV;

/// [soft 0x0815638c SI_svtAllocOptimGetCrv]: the global `nurbsCrv`, the curve `SI_svtAllocOptimBegin` made for the
/// evaluator of a followed NURBS surface to reuse (0 outside a begin / end pair).
pub fn svt_alloc_optim_get_crv(m: &Mem) -> u32 {
    m.u32(NURBS_CRV)
}

/// [soft 0x081563bc SI_surfaceValueAndTangent] `(kind, surface, u, v, p, tu, tv)`: the point and the two tangents
/// (three doubles each) of a patch (kind 1; the parameters narrowed to floats) or a NURBS surface (kind 10; the
/// parameters brought into the surface's ranges, wrapped for a closed direction, held at the end with the excess
/// added back along the tangent for an open one; the one-span curves come from `SI_nurbsSrfExtractCurveSeg`, into
/// the curve [`svt_alloc_optim_get_crv`] answers when there is one, otherwise into curves of their own that are
/// disposed of at the end). Any other kind writes nothing. The evaluation is `si-model`'s
/// (`patch_fit::surface_value_and_tangent`), which reproduces the listing.
pub fn surface_value_and_tangent(m: &mut Mem, kind: i32, surface: u32, u: f64, v: f64, p: u32, tu: u32, tv: u32) {
    si_model::model_image::patch_fit::surface_value_and_tangent(m, kind, surface, u, v, p, tu, tv);
}
