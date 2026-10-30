// SPDX-License-Identifier: MIT
//! Three routines of `db2scn.c`: how the elements of a loaded scene enter the graph list.
//!
//! `db2scn.c` is the scene loader's source file and not this package's; these three are
//! here because they are the list's builder and call nothing of the loader but the
//! routines named below. Written from the instruction listings.

use crate::globals::*;
use crate::graflist::{find_gl_data, graf_list_create, graflist_add_data, scene_list};
use crate::outside::{self, Outside};
use si_mem::{clib, Mem, Ptr32};
use si_types::dict::{ChapIds, Elem, ElemDict, ElemList, RelDict};
use si_types::model::Model;
use si_types::scene::GrafNode;

/// Offsets in the objects the builder writes an element's index into. Read from the
/// builder's own stores; the records belong to other packages.
mod object {
    /// A light: the 16-bit type word (1, 2, 3, 4; 2 is the light with an interest).
    pub const LIGHT_TYPE: u32 = 0x1e;
    /// A light: the index of its element in the LIGHTS chapter.
    pub const LIGHT_ELEM: u32 = 0x170;
    /// A material: the index of its element, and a 16-bit word set to 1 with it.
    pub const MATERIAL_ELEM: u32 = 0xda8;
    pub const MATERIAL_ELEM_SET: u32 = 0xdac;
    /// A 2D texture: its name, the index of its element, and a 16-bit word set to 7.
    pub const TEXTURE_NAME: u32 = 0x18;
    pub const TEXTURE_ELEM: u32 = 0x190;
    pub const TEXTURE_ELEM_SET: u32 = 0x194;
}

/// Ties a graph node and its element together [soft 0x08109d88 grafnodeElemAssoc]:
/// nothing for a null node; else the element's second object (+8) becomes the node, and
/// when `copy` is set the node takes the element's display state: its wire colour (the low
/// 16 bits of the word at +0x1c), its selection (+0x24) and its display type (+0x20).
pub fn grafnode_elem_assoc(m: &mut Mem, node: Ptr32<GrafNode>, elem: Ptr32<Elem>, copy: i16) {
    if node.is_null() {
        return;
    }
    m[elem].aux_data = node.cast();
    if copy != 0 {
        m[node].wire_color = m[elem].wire_color as i16;
        m[node].selection = m[elem].selection;
        m[node].wire_type = m[elem].wire_type;
    }
}

/// Gives the nodes of a hierarchy that has just entered the list their elements
/// [soft 0x08109cb0 elemdictCompleteHierarchy]: `node` is the root's graph node, `index`
/// its element's index in the MODELS chapter.
///
/// The root's count plus one nodes are walked along the chain; each whose model has a
/// father goes to `SI_edAddModel (0x08101ad4)` (model, node, 0). Then, on the current
/// dictionary and the chapter `SI_cid` names for the models: `SI_elemdictUpdateHierInfo
/// (0x08134cc8)` (dictionary, chapter, 1, 0x6e); `SI_elemdictPropagatePrfxAndShare
/// (0x08136350)` (dictionary, chapter, `index`, the prefix of the element's name record,
/// its 16-bit "shared" word), the element being read from the dictionary only after the
/// call before; and `SI_dbConvertModelInternals2DB (0x081094b0)` (the root's model, 0).
pub fn elemdict_complete_hierarchy(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<GrafNode>, index: i32) {
    let mut left = m[node].nb_sub as u16 as i32 + 1;
    let mut at = node;
    while left > 0 {
        let model: Ptr32<Model> = m[at].data.cast();
        if !m[model].father.is_null() {
            o.call(m, &outside::SI_ED_ADD_MODEL, &[model.addr(), at.addr(), 0]);
        }
        at = m[at].next;
        left -= 1;
        if m.fault().is_some() {
            // the original is dead by now
            return;
        }
    }
    let models = m[SI_CID][cid::MODELS];
    let ed = current_dict(m);
    o.call(m, &outside::SI_ELEMDICT_UPDATE_HIER_INFO, &[ed, models as u32, 1, 0x6e]);
    let models = m[SI_CID][cid::MODELS];
    let ed: Ptr32<ElemDict> = Ptr32::new(current_dict(m));
    let chapter = m[ed].lists.addr().wrapping_add((models as u32) << 6);
    let elems = m.u32(chapter.wrapping_add(8));
    let elem: Ptr32<Elem> = Ptr32::new(elems.wrapping_add((index as u32).wrapping_mul(0x84)));
    let shared = m[elem].shared;
    let name = m[elem].node;
    let prefix = m[name].prefix;
    let ed = current_dict(m);
    o.call(m, &outside::SI_ELEMDICT_PROPAGATE_PRFX_AND_SHARE, &[ed, models as u32, index as u32, prefix.addr(), shared as u16 as u32]);
    let model = m[node].data.addr();
    o.call(m, &outside::SI_DB_CONVERT_MODEL_INTERNALS_2_DB, &[model, 0]);
}

/// Puts the elements of a dictionary into the scene's graph list
/// [soft 0x08109ec8 SI_graflistBuildFromElemDict]: `ed` the dictionary, `rd` the relation
/// dictionary (only handed on), `cids` which chapter is which, `loaded` whether the
/// elements carry a display state to give to their nodes (a scene read from a file) or
/// are to be completed (0).
///
/// When there is no array of lists yet, `SI_grafListCreate` makes it. Then every chapter
/// in index order, and in it every element in index order **whose "new" word (+0x46) is
/// 1**; by the chapter:
///
/// - **MODELS**: an element whose object is a model without a father. The model enters the
///   list as a root (`SI_graflistAddData`, kind 2, selected or not as
///   `SI_cusReadingAnimModel (0x0853b274)` answers in 16 bits), which brings its whole
///   hierarchy behind it; node and element are tied; `SI_modelUpdateDependency
///   (0x0846f200)` (model, null). When `loaded` is 0, `SI_addInDictionary` is 1 while
///   `elemdictCompleteHierarchy` runs.
/// - **SCENES**: a node of kind 0xd without an object, tied.
/// - **NSELECTIONS** (0xc), **TEXTURES3D** (0x10), **ANIMATION** (0x12): the element's
///   object enters with that kind, tied.
/// - **MATERIALS** (0xe): the same, and the material gets the element's index at +0xda8
///   and 1 in the 16-bit word at +0xdac (the object is not tested for null).
/// - **TEXTURES2D** (0xf): the same; a texture without a name (+0x18) gets a copy
///   (`strdup`) of the element's name; the index goes to +0x190 and 7 to the 16-bit word
///   at +0x194.
/// - **CAMERAS**: nothing is added. The node of the process record's camera is looked up
///   (`SI_findGlData`), as kind 4 for an element below a root (its level at +0x54 is not
///   0) and as kind 5 for a root; an element without an object gets that camera; an
///   element without a node is tied to the one found. For a root element when `loaded` is
///   0: `SI_edCreateObjectFromInterest (0x08109db8)` (dictionary, relation dictionary,
///   chapter, index, element, the camera).
/// - **LIGHTS**: an element with an object. The light gets the element's index at +0x170.
///   By the light's 16-bit type at +0x1e the node is looked up as kind 8 (type 1), 0x13
///   (3), 0x14 (4), and for type 2 as 6 (an element below a root) or 7 (a root). For kind
///   6 when `loaded` is set, the element's name is copied over the light's own, 0x1e
///   bytes (`strncpy`). Tied; a node found as kind 7 when `loaded` is 0 goes to
///   `SI_edCreateObjectFromInterest` with the light.
///   **For any other type the kind is not set**: the look-up uses what the last camera,
///   light or root model left in the routine's local, and before the first of those a
///   word of the stack nobody wrote. The port keeps that local on the image's stack and
///   does not set it either, so it reads what the image holds there.
/// - **WAVES**: an element with an object is tied to the node found for it as kind 0xb.
/// - Any other chapter: nothing.
///
/// The chapter's address is worked out again after `elemdictCompleteHierarchy` and
/// `SI_edCreateObjectFromInterest` (the array of chapters may have moved), and the
/// element's from the chapter's array at every step.
///
/// Last, a second walk over the MODELS chapter: every new element whose model **has** a
/// father is tied to the node found for the model (any kind), always with the display
/// state copied.
pub fn graflist_build_from_elem_dict(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, rd: Ptr32<RelDict>, cids: Ptr32<ChapIds>, loaded: i16) {
    // the original's local at -0x14(%ebp): see "for any other type" above
    let kind_at = m.local(4);
    build(m, o, ed, rd, cids, loaded, kind_at);
    m.release(4);
}

fn build(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, rd: Ptr32<RelDict>, cids: Ptr32<ChapIds>, loaded: i16, kind_at: u32) {
    if m[SI_GRAF_LISTS].is_null() {
        graf_list_create(m);
    }
    let mut models = m[cids].model;
    let mut chap: i32 = 0;
    while chap < m[ed].nb_lists {
        let chapter_of = |m: &Mem| -> Ptr32<ElemList> { Ptr32::new(m[ed].lists.addr().wrapping_add((chap as u32) << 6)) };
        let mut chapter = chapter_of(m);
        let mut index: i32 = 0;
        if index < m[chapter].nb_elems {
            let mut offset: u32 = 0;
            loop {
                let elem: Ptr32<Elem> = Ptr32::new(m[chapter].elems.addr().wrapping_add(offset));
                if m[elem].new_flag == 1 {
                    if chap == m[cids].model {
                        let model: Ptr32<Model> = m[elem].data.cast();
                        if !model.is_null() && m[model].father.is_null() {
                            m.set_i32(kind_at, 2);
                            let select = o.call(m, &outside::SI_CUS_READING_ANIM_MODEL, &[]) as i16;
                            let list = scene_list(m);
                            let node = graflist_add_data(m, o, list, model.cast(), 2, select);
                            grafnode_elem_assoc(m, node, elem, loaded);
                            o.call(m, &outside::SI_MODEL_UPDATE_DEPENDENCY, &[model.addr(), 0]);
                            if loaded == 0 {
                                m[SI_ADD_IN_DICTIONARY] = 1;
                                elemdict_complete_hierarchy(m, o, node, index);
                                m[SI_ADD_IN_DICTIONARY] = 0;
                                chapter = chapter_of(m);
                            }
                        }
                    } else if chap == m[cids].dsc {
                        let list = scene_list(m);
                        let node = graflist_add_data(m, o, list, Ptr32::NULL, 0xd, 0);
                        grafnode_elem_assoc(m, node, elem, loaded);
                    } else if chap == m[cids].nsl {
                        let (list, data) = (scene_list(m), m[elem].data);
                        let node = graflist_add_data(m, o, list, data, 0xc, 0);
                        grafnode_elem_assoc(m, node, elem, loaded);
                    } else if chap == m[cids].mat {
                        let (list, data) = (scene_list(m), m[elem].data);
                        let node = graflist_add_data(m, o, list, data, 0xe, 0);
                        let material = m[elem].data.addr();
                        m.set_i32(material.wrapping_add(object::MATERIAL_ELEM), index);
                        m.set_i16(material.wrapping_add(object::MATERIAL_ELEM_SET), 1);
                        grafnode_elem_assoc(m, node, elem, loaded);
                    } else if chap == m[cids].t2d {
                        let (list, data) = (scene_list(m), m[elem].data);
                        let node = graflist_add_data(m, o, list, data, 0xf, 0);
                        let texture = m[elem].data.addr();
                        if m.u32(texture.wrapping_add(object::TEXTURE_NAME)) == 0 {
                            let name = m[elem].node;
                            let text = m[name].name.addr();
                            let copy = clib::strdup(m, text);
                            m.set_u32(texture.wrapping_add(object::TEXTURE_NAME), copy);
                        }
                        m.set_i32(texture.wrapping_add(object::TEXTURE_ELEM), index);
                        m.set_i16(texture.wrapping_add(object::TEXTURE_ELEM_SET), 7);
                        grafnode_elem_assoc(m, node, elem, loaded);
                    } else if chap == m[cids].t3d {
                        let (list, data) = (scene_list(m), m[elem].data);
                        let node = graflist_add_data(m, o, list, data, 0x10, 0);
                        grafnode_elem_assoc(m, node, elem, loaded);
                    } else if chap == m[cids].ani {
                        let (list, data) = (scene_list(m), m[elem].data);
                        let node = graflist_add_data(m, o, list, data, 0x12, 0);
                        grafnode_elem_assoc(m, node, elem, loaded);
                    } else if chap == m[cids].cam {
                        let kind = if m[elem].hier_level == 0 { 5 } else { 4 };
                        m.set_i32(kind_at, kind);
                        let proc = m[SI_PROC_DATA];
                        let camera = m[proc].camera;
                        let list = scene_list(m);
                        let node = find_gl_data(m, list, camera, kind);
                        if m[elem].data.is_null() {
                            let proc = m[SI_PROC_DATA];
                            m[elem].data = m[proc].camera;
                        }
                        if m[elem].aux_data.is_null() {
                            grafnode_elem_assoc(m, node, elem, loaded);
                        }
                        if m[elem].hier_level == 0 && loaded == 0 {
                            let proc = m[SI_PROC_DATA];
                            let camera = m[proc].camera.addr();
                            o.call(m, &outside::SI_ED_CREATE_OBJECT_FROM_INTEREST, &[ed.addr(), rd.addr(), chap as u32, index as u32, elem.addr(), camera]);
                            chapter = chapter_of(m);
                        }
                    } else if chap == m[cids].lig {
                        let light = m[elem].data.addr();
                        if light != 0 {
                            let ty = m.i16(light.wrapping_add(object::LIGHT_TYPE));
                            m.set_i32(light.wrapping_add(object::LIGHT_ELEM), index);
                            match ty {
                                1 => m.set_i32(kind_at, 8),
                                3 => m.set_i32(kind_at, 0x13),
                                4 => m.set_i32(kind_at, 0x14),
                                2 => {
                                    let kind = if m[elem].hier_level == 0 { 7 } else { 6 };
                                    m.set_i32(kind_at, kind);
                                    if kind == 6 && loaded != 0 {
                                        let name = m[elem].node;
                                        let text = m[name].name.addr();
                                        let own = m[elem].data.addr();
                                        clib::strncpy(m, own, text, 0x1e);
                                    }
                                }
                                _ => {}
                            }
                            let kind = m.i32(kind_at);
                            let (list, data) = (scene_list(m), m[elem].data);
                            let node = find_gl_data(m, list, data, kind);
                            grafnode_elem_assoc(m, node, elem, loaded);
                            if !node.is_null() && m.i32(kind_at) == 7 && loaded == 0 {
                                o.call(m, &outside::SI_ED_CREATE_OBJECT_FROM_INTEREST, &[ed.addr(), rd.addr(), chap as u32, index as u32, elem.addr(), light]);
                                chapter = chapter_of(m);
                            }
                        }
                    } else if chap == m[cids].wav && !m[elem].data.is_null() {
                        let (list, data) = (scene_list(m), m[elem].data);
                        let node = find_gl_data(m, list, data, 0xb);
                        grafnode_elem_assoc(m, node, elem, loaded);
                    }
                }
                index += 1;
                offset = offset.wrapping_add(0x84);
                models = m[cids].model;
                if index >= m[chapter].nb_elems || m.fault().is_some() {
                    break;
                }
            }
        }
        chap += 1;
        if m.fault().is_some() {
            // the original is dead by now
            return;
        }
    }
    // the hierarchies' other nodes: every model that has a father
    let chapter: Ptr32<ElemList> = Ptr32::new(m[ed].lists.addr().wrapping_add((models as u32) << 6));
    let mut index: i32 = 0;
    let mut offset: u32 = 0;
    while index < m[chapter].nb_elems {
        let elem: Ptr32<Elem> = Ptr32::new(m[chapter].elems.addr().wrapping_add(offset));
        if m[elem].new_flag == 1 {
            let model: Ptr32<Model> = m[elem].data.cast();
            if !model.is_null() && !m[model].father.is_null() {
                let list = scene_list(m);
                let node = find_gl_data(m, list, model.cast(), 0);
                grafnode_elem_assoc(m, node, elem, 1);
            }
        }
        index += 1;
        offset = offset.wrapping_add(0x84);
        if m.fault().is_some() {
            return;
        }
    }
}
