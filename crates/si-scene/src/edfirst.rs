// SPDX-License-Identifier: MIT
//! `edtools.c`, the first half by address (0x080fcaf0 to 0x08101420): the element add and
//! delete flags, the graf-node look-ups, the links between models, the fitting, light
//! selection and wave relations, the renames. Written beside [`crate::edtools`] (same
//! outside world, [`ByAddr`]); checked by `scene-emucheck` on the vectors of
//! `re/tools/edtools1/emu_gen_edfirst.py` (finding 0018, section "edtools.c, first half").
//!
//! Every callee that is not in this file is called by its address through [`ByAddr`], the
//! way the original calls it, whether another crate has a port of it or not.

use crate::edtools::*;
use crate::globals::SI_ADD_IN_DICTIONARY;
use crate::world::{current_dict, list};
use si_dict::globals::{SI_CID, SI_RD};
use si_mem::{Mem, Ptr32};

/// `SI_elemdictFindElemByDelData (0x081345e8)`: (dictionary, chapter, object).
pub const SI_ELEMDICT_FIND_ELEM_BY_DEL_DATA: u32 = 0x0813_45e8;
/// `SI_elemdictFindElemByDelAuxData (0x0813462c)`: (dictionary, chapter, graf node).
pub const SI_ELEMDICT_FIND_ELEM_BY_DEL_AUX_DATA: u32 = 0x0813_462c;
/// `SI_elemdictRemoveRelationId (0x08133998)`: (dictionary, relation dictionary, element, relation).
pub const SI_ELEMDICT_REMOVE_RELATION_ID: u32 = 0x0813_3998;
/// `SI_elemdictChangeName (0x08136450)`: (dictionary, chapter, element, new name, name).
pub const SI_ELEMDICT_CHANGE_NAME: u32 = 0x0813_6450;
/// `SI_dbSortTblSetDirty (0x0813425c)`: (chapter's list).
pub const SI_DB_SORT_TBL_SET_DIRTY: u32 = 0x0813_425c;
/// `SI_elemdictCheckDuplElemName (0x08134804)`: (dictionary, chapter, element).
pub const SI_ELEMDICT_CHECK_DUPL_ELEM_NAME: u32 = 0x0813_4804;
/// `SI_edSoftElemChangeName (0x080fe96c)`: (element, chapter, name).
pub const SI_ED_SOFT_ELEM_CHANGE_NAME: u32 = 0x080f_e96c;
/// `SI_dbListNodeFree (0x08122450)`: (name record).
pub const SI_DB_LIST_NODE_FREE: u32 = 0x0812_2450;
/// `SI_dbIconFree (0x08135a8c)`: (element).
pub const SI_DB_ICON_FREE: u32 = 0x0813_5a8c;
/// `SI_relRemoveRelations (0x081396d8)`: (dictionary, element, chapter, index, backward).
pub const SI_REL_REMOVE_RELATIONS: u32 = 0x0813_96d8;
/// `SI_elemdictSortAnimElements (0x08137980)`: (chapter's list).
pub const SI_ELEMDICT_SORT_ANIM_ELEMENTS: u32 = 0x0813_7980;
/// `SI_elemdictUpdateModelHierarchy (0x080fd698)`: (dictionary).
pub const SI_ELEMDICT_UPDATE_MODEL_HIERARCHY: u32 = 0x080f_d698;
/// `SI_edDisposeUDF (0x08103ca8)`: (shader, chapter).
pub const SI_ED_DISPOSE_UDF: u32 = 0x0810_3ca8;
/// `SI_edAddUDF (0x08103b5c)`: (owner, chapter, shader).
pub const SI_ED_ADD_UDF: u32 = 0x0810_3b5c;
/// `edModelRemoveMatTxt (0x08102ec4)`: (element).
pub const ED_MODEL_REMOVE_MAT_TXT: u32 = 0x0810_2ec4;
/// `SI_findGlData (0x0832ee68)`: (graf list, object, kind).
pub const SI_FIND_GL_DATA: u32 = 0x0832_ee68;
/// `SI_dbConvertModelInternals2DB (0x081094b0)`: (model, flag).
pub const SI_DB_CONVERT_MODEL_INTERNALS2DB: u32 = 0x0810_94b0;
/// `SI_modelRoot (0x0847838c)`: (model).
pub const SI_MODEL_ROOT: u32 = 0x0847_838c;
/// `SI_elemdictPropagatePrfxAndShare (0x08136350)`: (dictionary, chapter, index, prefix, share).
pub const SI_ELEMDICT_PROPAGATE_PRFX_AND_SHARE: u32 = 0x0813_6350;
/// `si_dbCheckChapterDuplicateNames (0x081096f4)`: (chapter's list, chapter).
pub const SI_DB_CHECK_CHAPTER_DUPLICATE_NAMES: u32 = 0x0810_96f4;
/// `si_edMergeAnimFiles (0x080ef508)`: (chapter, father's index, son's index, flag).
pub const SI_ED_MERGE_ANIM_FILES: u32 = 0x080e_f508;
/// `si_edSeparateAnimFiles (0x080ef8cc)`: (chapter, root's index, son's index, graf node, flag).
pub const SI_ED_SEPARATE_ANIM_FILES: u32 = 0x080e_f8cc;
/// `SI_relGetAnimFRelFromChapter (0x0810f540)`: (chapter).
pub const SI_REL_GET_ANIM_F_REL_FROM_CHAPTER: u32 = 0x0810_f540;
/// `SI_actionsTmpActivateAll (0x081dfa68)`: (dictionary, actions, flag).
pub const SI_ACTIONS_TMP_ACTIVATE_ALL: u32 = 0x081d_fa68;
/// `SI_actionsAddMasterKeysToHA (0x081e73b4)`: (dictionary, element, flag).
pub const SI_ACTIONS_ADD_MASTER_KEYS_TO_HA: u32 = 0x081e_73b4;
/// `SI_patchZipClearIndex (0x081a40b8)`: (model).
pub const SI_PATCH_ZIP_CLEAR_INDEX: u32 = 0x081a_40b8;
/// `SI_patchZipSetFlag (0x081a42b8)`: (model).
pub const SI_PATCH_ZIP_SET_FLAG: u32 = 0x081a_42b8;
/// `SI_elemdictRelationsCount (0x08135700)`.
pub const SI_ELEMDICT_RELATIONS_COUNT_: u32 = 0x0813_5700;
/// `SI_ikValidConstraint (0x084adca4)`: (item, kind, item, kind).
pub const SI_IK_VALID_CONSTRAINT: u32 = 0x084a_dca4;
/// `SI_isModelDeformed (0x08472d48)`: (model).
pub const SI_IS_MODEL_DEFORMED: u32 = 0x0847_2d48;
/// `SI_modelDeleteDefinition (0x084678a8)`: (model, flag).
pub const SI_MODEL_DELETE_DEFINITION: u32 = 0x0846_78a8;
/// `SI_edCheckAnimFileDispose (0x080ef100)`: (element, chapter).
pub const SI_ED_CHECK_ANIM_FILE_DISPOSE: u32 = 0x080e_f100;

/// The 16-bit global at 0x08781596 (tested at the end of the delete-flag routines).
pub const POST_DEF_FLAG: u32 = 0x0878_1596;

fn dict(m: &Mem) -> u32 {
    current_dict(m).0
}
fn find_by_data<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, chap: i32, obj: u32) -> i32 {
    let d = dict(m);
    o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[d, chap as u32, obj]) as i32
}
fn get_elem<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, chap: i32, index: i32) -> u32 {
    let d = dict(m);
    o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[d, chap as u32, index as u32])
}
fn add_rel<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, from: i32, to: i32, relid: i32, rule: u32) {
    let d = dict(m);
    o.call_addr(m, SI_ELEMDICT_ADD_RELATION, &[d, SI_RD.addr(), from as u32, to as u32, relid as u32, rule, 0]);
}
fn find_related<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, elem: u32, side: u32, list: u32, relid: i32, chap: i32, c: u32, i: u32) -> u32 {
    let d = dict(m);
    o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[d, elem, side, list, relid as u32, chap as u32, c, i])
}
fn remove_pair<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, from: i32, to: i32, relid: i32) {
    let d = dict(m);
    o.call_addr(m, SI_ELEMDICT_REMOVE_RELATION_PAIR, &[d, SI_RD.addr(), from as u32, to as u32, relid as u32, 1, 0]);
}
fn remove_id<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, elem: i32, relid: i32) {
    let d = dict(m);
    o.call_addr(m, SI_ELEMDICT_REMOVE_RELATION_ID, &[d, SI_RD.addr(), elem as u32, relid as u32]);
}
fn update_model_hierarchy<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem) {
    let d = dict(m);
    o.call_addr(m, SI_ELEMDICT_UPDATE_MODEL_HIERARCHY, &[d]);
}
fn sort_anim<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem) {
    let (d, ani) = (current_dict(m), m[SI_CID].ani);
    let l = list(m, d, ani).0;
    o.call_addr(m, SI_ELEMDICT_SORT_ANIM_ELEMENTS, &[l]);
}

/// The chapter a kind of graf node lives in [soft 0x080fcaf0 SI_dbGrafType2Chapter]: the
/// models' for the kinds not listed (2, 3, 9, 10, 13, 0x15 and up, and out of range).
pub fn db_graf_type2chapter(m: &Mem, kind: i32) -> i32 {
    let c = &m[SI_CID];
    match kind {
        4 | 5 => c.cam,
        6 | 7 | 8 | 0x13 | 0x14 => c.lig,
        0xb => c.wav,
        0xc => c.nsl,
        0xe => c.mat,
        0xf => c.t2d,
        0x10 => c.t3d,
        0x11 => c.img,
        0x12 => c.ani,
        _ => c.model,
    }
}

/// The relation that constrains an item of a kind of graf node
/// [soft 0x080fcba0 SI_relGetConsRelFromGrafType]: −1 for the other kinds.
pub fn rel_get_cons_rel_from_graf_type(kind: i32) -> i32 {
    match kind {
        2 | 3 => 0x6f,
        4 => 0x457,
        5 => 0x45c,
        6 | 8 | 0x13 => 0x83f,
        7 => 0x844,
        _ => -1,
    }
}

/// [soft 0x081003e8 relGetFittingRelId]: 0xe6 / 0xe7 for a first argument of 0 (second 0 / not 0),
/// 0xdc / 0xdd otherwise.
pub fn rel_get_fitting_rel_id(a: i16, b: i16) -> i32 {
    match (a != 0, b != 0) {
        (false, false) => 0xe6,
        (false, true) => 0xe7,
        (true, false) => 0xdc,
        (true, true) => 0xdd,
    }
}

/// [soft 0x080fd5b8 SI_edGetElemFromNSelection]: `SI_elemdictGetElemFromData` with the current
/// dictionary and the NSELECTIONS chapter.
pub fn ed_get_elem_from_nselection<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, nsel: u32, index: Ptr32<i32>) -> Ptr32<si_types::dict::Elem> {
    let (chapter, d) = (m[SI_CID].nsl as u32, dict(m));
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[d, chapter, nsel, index.0]))
}

/// [soft 0x080fd5ec SI_edGetElemFromAnimFile]: the same with `SI_cid`'s first word (ANIMATION).
pub fn ed_get_elem_from_anim_file<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, file: u32, index: Ptr32<i32>) -> Ptr32<si_types::dict::Elem> {
    let (chapter, d) = (m[SI_CID].ani as u32, dict(m));
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[d, chapter, file, index.0]))
}

/// [soft 0x080fe894 SI_edModelGetNewName]: `SI_elemdictGetNewName(current dictionary, MODELS, name)`.
pub fn ed_model_get_new_name<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, name: u32) -> u32 {
    let (chapter, d) = (m[SI_CID].model as u32, dict(m));
    o.call_addr(m, SI_ELEMDICT_GET_NEW_NAME, &[d, chapter, name])
}

/// [soft 0x080fce80 edFindParent]: `SI_elemdictFindRelatedElem(current dictionary, element, 1, 1,
/// 0x6e, MODELS, &scratch, &scratch)`; its answer is the routine's (the decompiler lost it, the
/// callers use it).
pub fn ed_find_parent<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, elem: u32) -> u32 {
    let at = m.local(8);
    let chapter = m[SI_CID].model;
    let r = find_related(o, m, elem, 1, 1, 0x6e, chapter, at + 4, at);
    m.release(8);
    r
}

/// [soft 0x080fd544 SI_edGetClusterFromModel]: the cluster element `index` of CLUSTERS when it is
/// in use (+0x48 is 0) and its object is on the chain of the model's clusters (the model of the
/// element `elem`, chain head +0x348, links +0x1c); that object, else 0.
pub fn ed_get_cluster_from_model<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, elem: u32, index: i32) -> u32 {
    let model = m.u32(elem.wrapping_add(4));
    let chapter = m[SI_CID].cls;
    let cluster = get_elem(o, m, chapter, index);
    if cluster != 0 && m.i16(cluster.wrapping_add(0x48)) == 0 && model != 0 {
        let mut link = m.u32(model.wrapping_add(0x348));
        while link != 0 {
            if link == m.u32(cluster.wrapping_add(4)) {
                return link;
            }
            link = m.u32(link.wrapping_add(0x1c));
        }
    }
    0
}

/// [soft 0x08101098 SI_edModelRelaxLTexture3D]: the material's index (MATERIALS), the texture's
/// (TEXTURES3D), `SI_elemdictRemoveRelationPair(…, 0x1f5, 1, 0)`.
pub fn ed_model_relax_l_texture3d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, material: u32, texture: u32) {
    let (mat, t3d) = (m[SI_CID].mat, m[SI_CID].t3d);
    let from = find_by_data(o, m, mat, material);
    let to = find_by_data(o, m, t3d, texture);
    remove_pair(o, m, from, to, 0x1f5);
}

/// [soft 0x0810137c SI_edModelRelaxLTexture2D]: as above for TEXTURES2D, relation 0x191, or 0x192
/// when the texture is the material's first local one (+0x6f4, read after the look-ups).
pub fn ed_model_relax_l_texture2d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, material: u32, texture: u32) {
    let (mat, t2d) = (m[SI_CID].mat, m[SI_CID].t2d);
    let from = find_by_data(o, m, mat, material);
    let to = find_by_data(o, m, t2d, texture);
    let relid = if m.u32(material.wrapping_add(0x6f4)) == texture { 0x192 } else { 0x191 };
    remove_pair(o, m, from, to, relid);
}

/// [soft 0x08101420 SI_edModelLinkWave]: relation 600 (0x259 when the wave's +0x70 is not 0). The
/// wave's element ([`ed_get_elem_from_wave`]); when nothing relates it to a model by that relation
/// (`SI_elemdictFindRelatedElem(…, element, 0, 0, relation, MODELS, &s, &s)`), the model's element
/// is related to the wave's, on lists 1 and 2.
pub fn ed_model_link_wave<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: Ptr32<si_types::model::Model>, wave: u32) {
    let relid = if m.u32(wave.wrapping_add(0x70)) != 0 { 0x259 } else { 600 };
    let at = m.local(12);
    let (l10, lc, l8) = (at + 8, at + 4, at);
    let elem = ed_get_elem_from_wave(o, m, wave, Ptr32::new(l8));
    let chapter = m[SI_CID].model;
    let found = find_related(o, m, elem.0, 0, 0, relid, chapter, lc, lc);
    if found == 0 {
        ed_get_elem_from_model(o, m, model, Ptr32::new(l10));
        let (from, to) = (m.i32(l10), m.i32(l8));
        add_rel(o, m, from, to, relid, 1);
        add_rel(o, m, from, to, relid, 2);
    }
    m.release(12);
}

/// [soft 0x0810041c SI_edModelSetFitting]: both models' indices (MODELS), then the fitting
/// relation `rel_get_fitting_rel_id(a, b)` on lists 1 and 2.
pub fn ed_model_set_fitting<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, other: u32, a: i16, b: i16) {
    let chapter = m[SI_CID].model;
    let from = find_by_data(o, m, chapter, model);
    let chapter = m[SI_CID].model;
    let to = find_by_data(o, m, chapter, other);
    let relid = rel_get_fitting_rel_id(a, b);
    add_rel(o, m, from, to, relid, 1);
    add_rel(o, m, from, to, relid, 2);
}

/// [soft 0x08100500 SI_edModelRelaxFitting]: `SI_elemdictRemoveRelationId` of the fitting relation
/// on the model's element.
pub fn ed_model_relax_fitting<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, a: i16, b: i16) {
    let chapter = m[SI_CID].model;
    let index = find_by_data(o, m, chapter, model);
    let relid = rel_get_fitting_rel_id(a, b);
    remove_id(o, m, index, relid);
}

/// [soft 0x08100d70 SI_edModelSetLigSelection]: relation 0x898 from the light's element to the
/// model's, list 1 (both indices start at −1).
pub fn ed_model_set_lig_selection<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, light: u32, model: Ptr32<si_types::model::Model>) {
    let at = m.local(8);
    let (lc, l8) = (at + 4, at);
    m.set_i32(lc, -1);
    m.set_i32(l8, -1);
    ed_get_elem_from_light(o, m, light, Ptr32::new(l8));
    ed_get_elem_from_model(o, m, model, Ptr32::new(lc));
    let (from, to) = (m.i32(l8), m.i32(lc));
    add_rel(o, m, from, to, 0x898, 1);
    m.release(8);
}

/// [soft 0x08100de4 SI_edModelRemoveLigSelection]: the same two indices (not preset), then
/// `SI_elemdictRemoveRelationPair(…, 0x898, 1, 0)`.
pub fn ed_model_remove_lig_selection<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, light: u32, model: Ptr32<si_types::model::Model>) {
    let at = m.local(8);
    let (lc, l8) = (at + 4, at);
    ed_get_elem_from_light(o, m, light, Ptr32::new(l8));
    ed_get_elem_from_model(o, m, model, Ptr32::new(lc));
    let (from, to) = (m.i32(l8), m.i32(lc));
    remove_pair(o, m, from, to, 0x898);
    m.release(8);
}

/// The element and chapter of the object a graf node carries [soft 0x080fcbec
/// SI_elemdictFindElemFromGrafNode]: null for a null node (nothing stored); else `*chap` and
/// `*index` start at −1 and the kinds 2, 3 (model), 4, 5 (camera), 6, 7, 8, 0x13, 0x14 (light),
/// 0xb (wave), 0xc (named selection), 0xd (scene), 0xe (material), 0xf, 0x10 (textures),
/// 0x12 (animation file) store their chapter and answer the element of the object at +4 (or the
/// scene's); other kinds answer 0.
pub fn elemdict_find_elem_from_graf_node<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, gn: u32, chap: u32, index: u32) -> u32 {
    if gn == 0 {
        return 0;
    }
    m.set_i32(chap, -1);
    m.set_i32(index, -1);
    let obj = m.u32(gn.wrapping_add(4));
    let kind = m.i32(gn);
    let c = m[SI_CID].clone();
    let at = Ptr32::new(index);
    match kind {
        2 | 3 => {
            m.set_i32(chap, c.model);
            ed_get_elem_from_model(o, m, Ptr32::new(obj), at).0
        }
        4 | 5 => {
            m.set_i32(chap, c.cam);
            ed_get_elem_from_camera(o, m, obj, at).0
        }
        6 | 7 | 8 | 0x13 | 0x14 => {
            m.set_i32(chap, c.lig);
            ed_get_elem_from_light(o, m, obj, at).0
        }
        0xb => {
            m.set_i32(chap, c.wav);
            ed_get_elem_from_wave(o, m, obj, at).0
        }
        0xc => {
            m.set_i32(chap, c.nsl);
            ed_get_elem_from_nselection(o, m, obj, at).0
        }
        0xd => {
            m.set_i32(chap, c.dsc);
            ed_get_elem_from_scene(o, m, at).0
        }
        0xe => {
            m.set_i32(chap, c.mat);
            ed_get_elem_from_material(o, m, obj, at).0
        }
        0xf => {
            m.set_i32(chap, c.t2d);
            ed_get_elem_from_texture2d(o, m, obj, at).0
        }
        0x10 => {
            m.set_i32(chap, c.t3d);
            ed_get_elem_from_texture3d(o, m, obj, at).0
        }
        0x12 => {
            m.set_i32(chap, c.ani);
            ed_get_elem_from_anim_file(o, m, obj, at).0
        }
        _ => 0,
    }
}

/// The same for the deleted [soft 0x080fcde0 elemdictFindElemFromDelGrafNode]: null node, 0;
/// else both outputs −1, `*chap` = [`db_graf_type2chapter`] of the node's kind, `*index` =
/// `SI_elemdictFindElemByDelAuxData(current dictionary, chapter, node)`; with an index not
/// negative the element (`SI_elemdictGetElem`), else 0.
pub fn elemdict_find_elem_from_del_graf_node<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, gn: u32, chap: u32, index: u32) -> u32 {
    if gn == 0 {
        return 0;
    }
    m.set_i32(chap, -1);
    m.set_i32(index, -1);
    let c = db_graf_type2chapter(m, m.i32(gn));
    m.set_i32(chap, c);
    let d = dict(m);
    let i = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DEL_AUX_DATA, &[d, c as u32, gn]) as i32;
    m.set_i32(index, i);
    if i >= 0 {
        let c = m.i32(chap);
        return get_elem(o, m, c, i);
    }
    0
}

/// The shared body of [soft 0x080fd6c8 SI_elemdictElementSetDeleteFlag] and [soft 0x080fd910
/// SI_edElementSetDeleteFlag] (the second takes the current dictionary).
///
/// Nothing for a null element or while `SI_addInDictionary` is 0. The chapter's sort table is
/// marked stale. With `flag` 0 (undelete): an element not deleted (+0x48 not 1) goes to the end;
/// else it gets `data` (+4) and `aux` (+8), and when `SI_elemdictCheckDuplElemName` finds another
/// element of its name (not −1) a new name is made (the status 2 meanwhile), the name record's
/// name and full name (+0x14, +0x24) are freed, replaced (a `strdup`, `SI_dbFormatFullName`) and
/// `SI_edSoftElemChangeName` told; the status becomes 0. With `flag` set (delete): `flag2` not 0
/// frees a deleted element for good (status 2, name record and icon freed, the pointers and +0x70
/// cleared); otherwise an element in use (status 0) loses its relations (forward then backward),
/// gets status 1, +0x54 0, +0x50 the index. MODELS then update the model hierarchy, ANIMATION
/// sorts its elements. The end marks the dictionary (+0x14 = 1, +0x5a = 1, and +0x6e = 1 while
/// the 16-bit global 0x08781596 is 0).
#[allow(clippy::too_many_arguments)]
fn element_set_delete_flag<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, ed: u32, e: u32, chap: i32, index: i32, flag: i16, data: u32, aux: u32, flag2: i16) {
    if e == 0 || m[SI_ADD_IN_DICTIONARY] == 0 {
        return;
    }
    let l = list(m, Ptr32::new(ed), chap).0;
    o.call_addr(m, SI_DB_SORT_TBL_SET_DIRTY, &[l]);
    let mut touched = true;
    if flag == 0 {
        if m.i16(e.wrapping_add(0x48)) != 1 {
            touched = false;
        } else {
            m.set_u32(e.wrapping_add(4), data);
            m.set_u32(e.wrapping_add(8), aux);
            let dup = o.call_addr(m, SI_ELEMDICT_CHECK_DUPL_ELEM_NAME, &[ed, chap as u32, e]) as i32;
            if dup != -1 {
                m.set_i16(e.wrapping_add(0x48), 2);
                let node = m.u32(e);
                let old = m.u32(node.wrapping_add(0x14));
                let new = o.call_addr(m, SI_ELEMDICT_GET_NEW_NAME, &[ed, chap as u32, old]);
                m.set_i16(e.wrapping_add(0x48), 0);
                let node = m.u32(e);
                for at in [0x14u32, 0x24] {
                    let p = m.u32(node.wrapping_add(at));
                    if p != 0 {
                        m.free(p);
                        m.set_u32(node.wrapping_add(at), 0);
                    }
                }
                let copy = m.strdup(new);
                m.set_u32(node.wrapping_add(0x14), copy);
                let full = o.call_addr(m, SI_DB_FORMAT_FULL_NAME, &[node]);
                m.set_u32(node.wrapping_add(0x24), full);
                o.call_addr(m, SI_ED_SOFT_ELEM_CHANGE_NAME, &[e, chap as u32, new]);
            }
            m.set_i16(e.wrapping_add(0x48), 0);
            after_flag(o, m, ed, chap);
        }
    } else {
        let mut skip = false;
        if flag2 != 0 {
            if m.i16(e.wrapping_add(0x48)) == 1 {
                m.set_i16(e.wrapping_add(0x48), 2);
                let node = m.u32(e);
                o.call_addr(m, SI_DB_LIST_NODE_FREE, &[node]);
                o.call_addr(m, SI_DB_ICON_FREE, &[e]);
                for at in [4u32, 8, 0, 0x70] {
                    m.set_u32(e.wrapping_add(at), 0);
                }
            }
            skip = true;
        }
        if skip || m.i16(e.wrapping_add(0x48)) != 0 {
            touched = false;
        } else {
            o.call_addr(m, SI_REL_REMOVE_RELATIONS, &[ed, e, chap as u32, index as u32, 1]);
            o.call_addr(m, SI_REL_REMOVE_RELATIONS, &[ed, e, chap as u32, index as u32, 0]);
            m.set_i16(e.wrapping_add(0x48), 1);
            m.set_u32(e.wrapping_add(0x54), 0);
            m.set_u32(e.wrapping_add(0x50), index as u32);
            after_flag(o, m, ed, chap);
        }
    }
    if touched {
        m.set_i16(ed.wrapping_add(0x6e), 1);
    }
    m.set_u32(ed.wrapping_add(0x14), 1);
    m.set_i16(ed.wrapping_add(0x5a), 1);
    if m.u16(POST_DEF_FLAG) == 0 {
        m.set_i16(ed.wrapping_add(0x6e), 1);
    }
}

/// What the delete flag does to a chapter after the change: MODELS update the hierarchy,
/// ANIMATION sorts its elements.
fn after_flag<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, ed: u32, chap: i32) {
    if chap == m[SI_CID].model {
        o.call_addr(m, SI_ELEMDICT_UPDATE_MODEL_HIERARCHY, &[ed]);
    } else if chap == m[SI_CID].ani {
        let l = list(m, Ptr32::new(ed), chap).0;
        o.call_addr(m, SI_ELEMDICT_SORT_ANIM_ELEMENTS, &[l]);
    }
}

/// [soft 0x080fd6c8 SI_elemdictElementSetDeleteFlag]: see `element_set_delete_flag`; the
/// dictionary is the first argument.
#[allow(clippy::too_many_arguments)]
pub fn elemdict_element_set_delete_flag<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, ed: u32, e: u32, chap: i32, index: i32, flag: i16, data: u32, aux: u32, flag2: i16) {
    element_set_delete_flag(o, m, ed, e, chap, index, flag, data, aux, flag2);
}

/// [soft 0x080fd910 SI_edElementSetDeleteFlag]: the same on the current dictionary.
#[allow(clippy::too_many_arguments)]
pub fn ed_element_set_delete_flag<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, e: u32, chap: i32, index: i32, flag: i16, data: u32, aux: u32, flag2: i16) {
    let ed = dict(m);
    element_set_delete_flag(o, m, ed, e, chap, index, flag, data, aux, flag2);
}

/// [soft 0x080fdc18 edModelDeleteFlag]: the model's element (`SI_elemdictFindElemByDelData`,
/// MODELS) is flagged (`SI_edElementSetDeleteFlag(element, MODELS, index, flag, null, null, flag2)`);
/// with `deep` the same for the model's son (+0x2c4) and its brother (+0x2c0) when not null (the
/// second test repeats `deep`).
pub fn ed_model_delete_flag<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, deep: i16, flag: i16, flag2: i16) {
    let (chapter, d) = (m[SI_CID].model, dict(m));
    let index = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DEL_DATA, &[d, chapter as u32, model]) as i32;
    let e = get_elem(o, m, chapter, index);
    ed_element_set_delete_flag(o, m, e, chapter, index, flag, 0, 0, flag2);
    if deep != 0 {
        let son = m.u32(model.wrapping_add(0x2c4));
        if son != 0 {
            ed_model_delete_flag(o, m, son, deep, flag, flag2);
        }
        let brother = m.u32(model.wrapping_add(0x2c0));
        if deep != 0 && brother != 0 {
            ed_model_delete_flag(o, m, brother, deep, flag, flag2);
        }
    }
}

/// [soft 0x080fe330 transferConstraintToRoot]: for each element related to `elem` by `relid`
/// (`p5`) on list `p4` backward side (`SI_elemdictFindRelatedElem(…, elem, 1, p4, p5, MODELS, …)`),
/// the same relation is added from `to` (list 1); then, if there was one, the relation `p5` is
/// removed from the element `from`.
pub fn transfer_constraint_to_root<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, elem: u32, from: i32, to: i32, p4: u32, relid: i32) {
    let at = m.local(8);
    let (lc, l8) = (at + 4, at);
    let chapter = m[SI_CID].model;
    let mut found = find_related(o, m, elem, 1, p4, relid, chapter, lc, l8);
    if found != 0 {
        loop {
            let other = m.i32(l8);
            add_rel(o, m, to, other, relid, 1);
            let (chapter, d) = (m[SI_CID].model, dict(m));
            found = o.call_addr(m, SI_ELEMDICT_FIND_NEXT_RELATED_ELEM, &[d, elem, 1, p4, relid as u32, chapter as u32, lc, l8]);
            if found == 0 {
                break;
            }
        }
        remove_id(o, m, from, relid);
    }
    m.release(8);
}

/// [soft 0x080fe8c8 SI_edModelChangeName]: the node's element (`SI_elemdictFindElemByData`,
/// MODELS); `SI_elemdictChangeName(current dictionary, MODELS, element, null, name)` (16-bit
/// answer); when it did not answer 0 the node's own name is changed (`SI_modelChangeName`).
pub fn ed_model_change_name<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, name: u32) {
    let chapter = m[SI_CID].model;
    let index = find_by_data(o, m, chapter, model);
    let e = get_elem(o, m, chapter, index);
    let d = dict(m);
    let r = o.call_addr(m, SI_ELEMDICT_CHANGE_NAME, &[d, chapter as u32, e, 0, name]) as u16;
    if r != 0 {
        o.call_addr(m, SI_MODEL_CHANGE_NAME, &[model, name]);
    }
}

/// [soft 0x080fe41c SI_edLinkModel]: `(father's graf node, son's graf node, flag)`. The son's
/// element is related to the father's by 0x6e unless it has a parent already ([`ed_find_parent`]);
/// the constraints 0x75 and 0x76 on the son's element move to the root of the father's tree
/// ([`transfer_constraint_to_root`], list 1); the hierarchy is updated; the root's prefix and share
/// flag (`*(*root_element + 0x10)`, 16 bits at +0x44) are propagated
/// (`SI_elemdictPropagatePrfxAndShare`), duplicate names checked, and the animation files merged
/// (`si_edMergeAnimFiles`; an answer not 0 sorts the ANIMATION elements).
pub fn ed_link_model<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, father_gn: u32, son_gn: u32, flag: i16) {
    let father = m.u32(father_gn.wrapping_add(4));
    let son = m.u32(son_gn.wrapping_add(4));
    let root = o.call_addr(m, SI_MODEL_ROOT, &[father]);
    let at = m.local(12);
    let (l10, lc, l8) = (at + 8, at + 4, at);
    ed_get_elem_from_model(o, m, Ptr32::new(father), Ptr32::new(l8));
    ed_get_elem_from_model(o, m, Ptr32::new(son), Ptr32::new(lc));
    let root_elem = ed_get_elem_from_model(o, m, Ptr32::new(root), Ptr32::new(l10)).0;
    let chapter = m[SI_CID].model;
    let son_elem = get_elem(o, m, chapter, m.i32(lc));
    if ed_find_parent(o, m, son_elem) == 0 {
        let (from, to) = (m.i32(lc), m.i32(l8));
        add_rel(o, m, from, to, 0x6e, 1);
    }
    let (from, to) = (m.i32(lc), m.i32(l10));
    transfer_constraint_to_root(o, m, son_elem, from, to, 1, 0x75);
    transfer_constraint_to_root(o, m, son_elem, from, to, 1, 0x76);
    update_model_hierarchy(o, m);
    let (chapter, d) = (m[SI_CID].model, dict(m));
    let node = m.u32(root_elem);
    let (prefix, share) = (m.u32(node.wrapping_add(0x10)), m.i16(root_elem.wrapping_add(0x44)) as u32);
    o.call_addr(m, SI_ELEMDICT_PROPAGATE_PRFX_AND_SHARE, &[d, chapter as u32, m.u32(l10), prefix, share]);
    let (d, chapter) = (current_dict(m), m[SI_CID].model);
    let l = list(m, d, chapter).0;
    o.call_addr(m, SI_DB_CHECK_CHAPTER_DUPLICATE_NAMES, &[l, chapter as u32]);
    let chapter = m[SI_CID].model;
    let merged = o.call_addr(m, SI_ED_MERGE_ANIM_FILES, &[chapter as u32, m.u32(l8), m.u32(lc), flag as i32 as u32]) as u16;
    if merged != 0 {
        sort_anim(o, m);
    }
    m.release(12);
}

/// [soft 0x080fe5ec SI_edCutModel]: `(model, its graf node's father … the son's graf node, flag)`:
/// with `flag` 0 the son's animation (element related by `SI_relGetAnimFRelFromChapter(MODELS)`,
/// side 1, to ANIMATION) has its actions activated and its master keys put to the history;
/// the 0x6e relation between the two models is removed; a model of kind 5 (not a null-less
/// son of kind 5, or kind 0 with a special sub-kind) also loses its 0x79 relation with the
/// first kind-0 model with sub-kind other than 3 and 4 down its chain; the hierarchy is updated;
/// the animation files are separated (`si_edSeparateAnimFiles`; an answer not 0 sorts ANIMATION).
pub fn ed_cut_model<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, gn: u32, flag: i16) {
    let son = m.u32(gn.wrapping_add(4));
    let chapter = m[SI_CID].model;
    let i2 = find_by_data(o, m, chapter, model);
    let chapter = m[SI_CID].model;
    let i3 = find_by_data(o, m, chapter, son);
    let at = m.local(8);
    let (lc, l8) = (at + 4, at);
    if flag == 0 {
        let chapter = m[SI_CID].model;
        let rel = o.call_addr(m, SI_REL_GET_ANIM_F_REL_FROM_CHAPTER, &[chapter as u32]);
        let chapter = m[SI_CID].model;
        let elem = get_elem(o, m, chapter, i2);
        let ani = m[SI_CID].ani;
        let p6 = find_related(o, m, elem, 1, 0, rel as i32, ani, l8, l8);
        if p6 != 0 && m.u32(p6.wrapping_add(4)) != 0 {
            let (d, actions) = (dict(m), m.u32(p6.wrapping_add(4)).wrapping_add(0xc));
            o.call_addr(m, SI_ACTIONS_TMP_ACTIVATE_ALL, &[d, actions, 0]);
            let d = dict(m);
            o.call_addr(m, SI_ACTIONS_ADD_MASTER_KEYS_TO_HA, &[d, p6, 1]);
        }
    }
    remove_pair(o, m, i3, i2, 0x6e);
    let kind = m.u32(son.wrapping_add(0x30));
    if kind != 5 && (kind != 0 || 1 < m.u32(son.wrapping_add(0x2ec)).wrapping_sub(3)) && m.u32(model.wrapping_add(0x30)) == 5 {
        let mut p = model;
        loop {
            loop {
                p = m.u32(p.wrapping_add(0x2c4));
                if m.u32(p.wrapping_add(0x30)) == 0 {
                    break;
                }
            }
            if 1 >= m.u32(p.wrapping_add(0x2ec)).wrapping_sub(3) {
                break;
            }
        }
        ed_get_elem_from_model(o, m, Ptr32::new(p), Ptr32::new(lc));
        let to = m.i32(lc);
        remove_pair(o, m, i3, to, 0x79);
    }
    update_model_hierarchy(o, m);
    let root = o.call_addr(m, SI_MODEL_ROOT, &[model]);
    let chapter = m[SI_CID].model;
    let ri = find_by_data(o, m, chapter, root);
    let chapter = m[SI_CID].model;
    let sep = o.call_addr(m, SI_ED_SEPARATE_ANIM_FILES, &[chapter as u32, ri as u32, i3 as u32, gn, flag as i32 as u32]) as u16;
    if sep != 0 {
        sort_anim(o, m);
    }
    m.release(8);
}

/// [soft 0x080ff5f0 SI_edRelaxRelFromDelModel]: removes the relation `relid` that leaves the
/// deleted model's element on side 0 list 2, one element after the other. For relation 0xff the
/// zip index of the model is cleared first (`SI_patchZipClearIndex`); for others, an element that
/// also has an 0xff relation (side 1 or 0, list 2) gets its zip flag set. For relation 0x101 and a
/// model of kind 9, an element with more than two such relations on side 1, lists 2 and 2
/// (`SI_elemdictRelationsCount`) ends the loop: its relation is removed as a pair with the
/// model's own element, once, and the routine returns.
pub fn ed_relax_rel_from_del_model<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, relid: i32) {
    let at = m.local(12);
    let (l10, lc, l8) = (at + 8, at + 4, at);
    let mut stop = false;
    let elem = ed_get_elem_from_model(o, m, Ptr32::new(model), Ptr32::new(l8)).0;
    let chapter = m[SI_CID].model;
    let mut other = find_related(o, m, elem, 0, 2, relid, chapter, l10, lc);
    if relid == 0xff {
        o.call_addr(m, SI_PATCH_ZIP_CLEAR_INDEX, &[model]);
    }
    while other != 0 {
        if relid != 0xff {
            let chapter = m[SI_CID].model;
            let mut r = find_related(o, m, other, 1, 2, 0xff, chapter, l10, l10);
            if r == 0 {
                let chapter = m[SI_CID].model;
                r = find_related(o, m, other, 0, 2, 0xff, chapter, l10, l10);
            }
            if r != 0 {
                let obj = m.u32(other.wrapping_add(4));
                o.call_addr(m, SI_PATCH_ZIP_SET_FLAG, &[obj]);
            }
        }
        if relid == 0x101 && m.i32(model.wrapping_add(0x30)) == 9 {
            let (d, chapter) = (dict(m), m[SI_CID].model);
            let n = o.call_addr(m, SI_ELEMDICT_RELATIONS_COUNT_, &[d, other, 1, 2, 2, 0x101, chapter as u32]) as i32;
            if 2 < n {
                stop = true;
            }
        }
        if stop {
            break;
        }
        let to = m.i32(lc);
        remove_id(o, m, to, relid);
        let chapter = m[SI_CID].model;
        other = find_related(o, m, elem, 0, 2, relid, chapter, l10, lc);
    }
    if stop {
        let (from, to) = (m.i32(lc), m.i32(l8));
        remove_pair(o, m, from, to, relid);
    }
    m.release(12);
}

/// [soft 0x080fffac SI_edElementSetConstraint]: `(item, kind, target, kind, relation, data)`;
/// 0 for a null item or when `SI_ikValidConstraint` answers (16 bits) 0. The relation is the given
/// one (models, kinds 2 and 3; negative gives 0), 0x45d for a camera kind (4, 5) given 0x80,
/// else [`rel_get_cons_rel_from_graf_type`] (negative gives 0). The two elements are those of the
/// items (models and other kinds by `SI_elemdictFindElemByData` in the kind's chapter, cameras
/// and lights by their own look-ups; a kind 4 or 6 target is first looked up through its
/// interest relation 0x456 or 0x83e, and that element, when there is one, replaces it), then
/// `SI_elemdictAddUnCompleteRelation(…, chapter, chapter, item, target, relation, 1, 0, data, 1)`
/// and for relation 0x7c the same on list 2 with no data. 1.
pub fn ed_element_set_constraint<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, item: u32, kind: i32, target: u32, tkind: i32, rel: i16, data: u32) -> u32 {
    if item == 0 {
        return 0;
    }
    if o.call_addr(m, SI_IK_VALID_CONSTRAINT, &[item, kind as u32, target, tkind as u32]) as u16 == 0 {
        return 0;
    }
    let c2 = db_graf_type2chapter(m, kind);
    let c3 = db_graf_type2chapter(m, tkind);
    let relid = match constraint_relation(m, kind, rel) {
        Some(r) => r,
        None => return 0,
    };
    let at = m.local(16);
    let (l14, l10, lc, l8) = (at + 12, at + 8, at + 4, at);
    if kind < 4 {
        let i = find_by_data(o, m, c2, item);
        m.set_i32(l8, i);
    } else if kind < 6 {
        ed_get_elem_from_camera(o, m, item, Ptr32::new(l8));
    } else if 7 < kind {
        let i = find_by_data(o, m, c2, item);
        m.set_i32(l8, i);
    } else {
        ed_get_elem_from_light(o, m, item, Ptr32::new(l8));
    }
    // (the target)
    let direct = |o: &mut O, m: &mut Mem| {
        let i = find_by_data(o, m, c3, target);
        m.set_i32(lc, i);
    };
    if tkind == 5 {
        ed_get_elem_from_camera(o, m, target, Ptr32::new(lc));
    } else if tkind < 6 {
        if tkind != 4 {
            direct(o, m);
        } else {
            let e = ed_get_elem_from_camera(o, m, target, Ptr32::new(lc)).0;
            let cam = m[SI_CID].cam;
            if find_related(o, m, e, 0, 1, 0x456, cam, l14, l10) != 0 {
                let v = m.i32(l10);
                m.set_i32(lc, v);
            }
        }
    } else if tkind != 6 {
        if tkind == 7 {
            ed_get_elem_from_light(o, m, target, Ptr32::new(lc));
        } else {
            direct(o, m);
        }
    } else {
        let e = ed_get_elem_from_light(o, m, target, Ptr32::new(lc)).0;
        let lig = m[SI_CID].lig;
        if find_related(o, m, e, 0, 1, 0x83e, lig, l14, l10) != 0 {
            let v = m.i32(l10);
            m.set_i32(lc, v);
        }
    }
    let (d, from, to) = (dict(m), m.u32(l8), m.u32(lc));
    o.call_addr(m, SI_ELEMDICT_ADD_UN_COMPLETE_RELATION, &[d, SI_RD.addr(), c2 as u32, c3 as u32, from, to, relid as u32, 1, 0, data, 1]);
    if relid == 0x7c {
        let d = dict(m);
        o.call_addr(m, SI_ELEMDICT_ADD_UN_COMPLETE_RELATION, &[d, SI_RD.addr(), c2 as u32, c3 as u32, from, to, 0x7c, 2, 0, 0, 1]);
    }
    m.release(16);
    1
}

/// The relation the two constraint routines use: the caller's for a model kind (2, 3), 0x45d for
/// a camera kind (4, 5) given 0x80, else the kind's own; `None` when negative.
fn constraint_relation(m: &Mem, kind: i32, rel: i16) -> Option<i32> {
    let _ = m;
    let relid = if (kind.wrapping_sub(2) as u32) < 2 {
        rel as i32
    } else if rel != 0x80 || 1 < (kind.wrapping_sub(4) as u32) {
        rel_get_cons_rel_from_graf_type(kind)
    } else {
        0x45d
    };
    (relid >= 0).then_some(relid)
}

/// [soft 0x08100238 SI_edElementRelaxConstraint]: `(item, kind, relation)`; the relation as in
/// [`ed_element_set_constraint`] (0 when negative). The item's element index: cameras and lights
/// by their look-ups, the others by `SI_elemdictFindElemByData` and `SI_elemdictGetElem`. For
/// relation 0xf7 the element's 0xf7 relation on side 1, list 2 is looked for. The relation is
/// removed from the index (`SI_elemdictRemoveRelationId`); if there was one and the model is not
/// deformed (`SI_isModelDeformed`, 16 bits) its definition goes (`SI_modelDeleteDefinition(model,
/// 1)`); a non-null element is offered to `SI_edCheckAnimFileDispose`. 1.
pub fn ed_element_relax_constraint<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, item: u32, kind: i32, rel: i16) -> u32 {
    let mut had = false;
    let c2 = db_graf_type2chapter(m, kind);
    let relid = match constraint_relation(m, kind, rel) {
        Some(r) => r,
        None => return 0,
    };
    let at = m.local(8);
    let (lc, l8) = (at + 4, at);
    let elem;
    if 3 < kind && kind < 6 {
        elem = ed_get_elem_from_camera(o, m, item, Ptr32::new(l8)).0;
    } else if 3 < kind && kind < 8 {
        elem = ed_get_elem_from_light(o, m, item, Ptr32::new(l8)).0;
    } else {
        let i = find_by_data(o, m, c2, item);
        m.set_i32(l8, i);
        elem = get_elem(o, m, c2, i);
    }
    if relid == 0xf7 {
        let chapter = m[SI_CID].model;
        had = find_related(o, m, elem, 1, 2, 0xf7, chapter, lc, lc) != 0;
    }
    let index = m.i32(l8);
    remove_id(o, m, index, relid);
    if had && o.call_addr(m, SI_IS_MODEL_DEFORMED, &[item]) as u16 == 0 {
        o.call_addr(m, SI_MODEL_DELETE_DEFINITION, &[item, 1]);
    }
    if elem != 0 {
        o.call_addr(m, SI_ED_CHECK_ANIM_FILE_DISPOSE, &[elem, c2 as u32]);
    }
    m.release(8);
    1
}

/// [soft 0x080fdcf4 SI_edGrafNodeSetDeleteFlag]: `(graf node, delete, replacing graf node, flag2)`.
///
/// The node's element ([`elemdict_find_elem_from_del_graf_node`]); nothing for none or for a
/// camera. When deleting a light's element that is not new (+0x54 0) its two shaders are disposed
/// (`SI_edDisposeUDF` at +0x178 with LIGHT_SHADERS, +0x17c with LIGHT_PHOTON_SHADERS); a model's
/// materials and textures go (`edModelRemoveMatTxt`). The element is flagged, or for a model with
/// `flag2` the whole subtree ([`ed_model_delete_flag`]). When undeleting, with a replacing graf
/// node: its models' (kinds 2, 3) chain of sons (count at +0x10, links +0x1c) are undeleted with
/// their parents relinked ([`ed_link_model`] to the node found in the graf list); a light
/// (kinds 7, 8, 0x13, 0x14) re-adds its shaders (`SI_edAddUDF`), kind 7 also undeletes the light
/// next to it (+0x1c) with its 0x83e relation and hierarchy; a model without a database record
/// (+700 is 0) is converted (`SI_dbConvertModelInternals2DB(model, 1)`).
pub fn ed_graf_node_set_delete_flag<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, gn: u32, del: i16, gn2: u32, flag2: i16) {
    let at = m.local(8);
    let (lc, l8) = (at + 4, at);
    let e = elemdict_find_elem_from_del_graf_node(o, m, gn, lc, l8);
    let c = m.i32(lc);
    if e != 0 && c != m[SI_CID].cam {
        let (lig, model) = (m[SI_CID].lig, m[SI_CID].model);
        if del != 0 {
            if c == lig && m.u32(e.wrapping_add(0x54)) == 0 {
                let obj = m.u32(e.wrapping_add(4));
                if obj != 0 {
                    let (a, b) = (m.u32(obj.wrapping_add(0x178)), m.u32(obj.wrapping_add(0x17c)));
                    o.call_addr(m, SI_ED_DISPOSE_UDF, &[a, m[SI_CID].shd_lig as u32]);
                    o.call_addr(m, SI_ED_DISPOSE_UDF, &[b, m[SI_CID].shd_lig_photon as u32]);
                }
            }
            if del != 0 && c == model {
                o.call_addr(m, ED_MODEL_REMOVE_MAT_TXT, &[e]);
            }
        }
        let data = if gn2 != 0 { m.u32(gn2.wrapping_add(4)) } else { 0 };
        if flag2 == 0 || c != model {
            let i = m.i32(l8);
            ed_element_set_delete_flag(o, m, e, c, i, del, data, gn2, flag2);
        } else {
            let obj = m.u32(gn.wrapping_add(4));
            ed_model_delete_flag(o, m, obj, 1, del, flag2);
        }
        if del == 0 {
            if gn2 != 0 {
                match m.i32(gn2) {
                    2 | 3 => {
                        let mut g = gn2;
                        if m.u16(gn2.wrapping_add(0x10)) != 0 {
                            let mut n = 0u32;
                            loop {
                                g = m.u32(g.wrapping_add(0x1c));
                                let obj = if g != 0 { m.u32(g.wrapping_add(4)) } else { 0 };
                                if g != 0 && obj != 0 {
                                    let (chapter, d) = (m[SI_CID].model, dict(m));
                                    let i = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DEL_DATA, &[d, chapter as u32, obj]) as i32;
                                    m.set_i32(l8, i);
                                    let e = get_elem(o, m, chapter, i);
                                    ed_element_set_delete_flag(o, m, e, chapter, i, 0, obj, g, flag2);
                                    let father = m.u32(obj.wrapping_add(700));
                                    if father != 0 {
                                        let lists = crate::dispose::graf_list(m);
                                        let found = o.call_addr(m, SI_FIND_GL_DATA, &[lists, father, 2]);
                                        if found != 0 {
                                            ed_link_model(o, m, found, g, 0);
                                        }
                                    }
                                }
                                n += 1;
                                if n >= m.u16(gn2.wrapping_add(0x10)) as u32 {
                                    break;
                                }
                            }
                        }
                    }
                    7 => {
                        let obj = m.u32(gn2.wrapping_add(4));
                        if obj != 0 {
                            readd_shaders(o, m, obj);
                        }
                        let (saved, chapter) = (m.i32(l8), m[SI_CID].lig);
                        let next = m.u32(gn2.wrapping_add(0x1c));
                        let nobj = if next != 0 { m.u32(next.wrapping_add(4)) } else { 0 };
                        if next != 0 && nobj != 0 {
                            let d = dict(m);
                            let i = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DEL_DATA, &[d, chapter as u32, nobj]) as i32;
                            let e = get_elem(o, m, chapter, i);
                            ed_element_set_delete_flag(o, m, e, chapter, i, 0, nobj, next, flag2);
                            add_rel(o, m, i, saved, 0x83e, 1);
                            let d = dict(m);
                            o.call_addr(m, SI_ELEMDICT_UPDATE_HIER_INFO, &[d, chapter as u32, 1, 0x83e]);
                        }
                    }
                    8 | 0x13 | 0x14 => {
                        let obj = m.u32(gn2.wrapping_add(4));
                        if obj != 0 {
                            readd_shaders(o, m, obj);
                        }
                    }
                    _ => {}
                }
            }
            if gn2 != 0 && (m.u32(gn2).wrapping_sub(2)) < 2 {
                let model = m.u32(gn2.wrapping_add(4));
                if model != 0 && m.u32(model.wrapping_add(700)) == 0 {
                    o.call_addr(m, SI_DB_CONVERT_MODEL_INTERNALS2DB, &[model, 1]);
                }
            }
        }
    }
    m.release(8);
}

/// A light's two shaders (+0x178, +0x17c) added back to the dictionary when not null
/// (`SI_edAddUDF(light, LIGHTS, shader)`).
fn readd_shaders<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, light: u32) {
    for at in [0x178u32, 0x17c] {
        let shader = m.u32(light.wrapping_add(at));
        if shader != 0 {
            let lig = m[SI_CID].lig;
            o.call_addr(m, SI_ED_ADD_UDF, &[light, lig as u32, shader]);
        }
    }
}
