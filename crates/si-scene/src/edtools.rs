// SPDX-License-Identifier: MIT
//! `edtools.c`, `model.c` and `mat_global.c`: the small routines between the scene graph's
//! list, the dictionary and the model nodes that the list's builder and the render commands
//! call on a loaded scene (finding 0023, section 18.5; moved here from `si-rensetup` in the
//! pass of finding 0018, section 16, because the loader now builds the list and needs them:
//! `si-rensetup` calls them through wrappers of its own outside world).
//!
//! The first nine were compared with the emulated original (`rensetup-emucheck`, the
//! vectors of `re/tools/si-rensetup/emu_gen_pass4.py`: 6,800 cases, 0 different), the tenth
//! (`SI_edGetElementsGrafNodeInfo`, finding 0018, section 18) by `scene-emucheck` on the
//! vectors of `re/tools/si-scene/emu_gen_grafinfo.py`.
//!
//! The dictionary's own routines are `si-dict`'s, the name records' this crate's
//! ([`crate::dbnode`]), the model's `si-model`'s: all behind [`ByAddr`], the original's own
//! way of calling (a code address and the argument words).

use crate::globals::SI_ADD_IN_DICTIONARY;
use crate::world::current_dict;
use si_dict::globals::{SI_CID, SI_DBCID, SI_RD};
use si_mem::{Mem, Ptr32};
use si_types::dict::{ChapIds, DbListNode, Elem, ElemDict};
use si_types::model::Model;
use si_types::scene::GrafNode;

/// The rest of the program as these routines call it: the routine at the address `f` with
/// the stack words `args`; the answer is what it leaves in `eax`.
pub trait ByAddr {
    fn call_addr(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32;
}

/// `SI_elemdictFindElemByData (0x08134364)`, `elemdict.c`: (dictionary, chapter, object).
pub const SI_ELEMDICT_FIND_ELEM_BY_DATA: u32 = 0x0813_4364;
/// `SI_elemdictGetElem (0x08132b50)`, `elemdict.c`: (dictionary, chapter, index).
pub const SI_ELEMDICT_GET_ELEM: u32 = 0x0813_2b50;
/// `SI_elemdictFindSon (0x08137ff4)`, `elemdict.c`: (dictionary, element).
pub const SI_ELEMDICT_FIND_SON: u32 = 0x0813_7ff4;
/// `SI_elemdictFindNextSon (0x08138030)`, `elemdict.c`: (dictionary, element).
pub const SI_ELEMDICT_FIND_NEXT_SON: u32 = 0x0813_8030;
/// `SI_elemdictGetElemFromData (0x08137f94)`, `elemdict.c`: (dictionary, chapter, object, &index).
pub const SI_ELEMDICT_GET_ELEM_FROM_DATA: u32 = 0x0813_7f94;
/// `SI_elemdictUpdateHierInfo (0x08134cc8)`, `elemdict.c`: (dictionary, chapter, list, relation).
pub const SI_ELEMDICT_UPDATE_HIER_INFO: u32 = 0x0813_4cc8;
/// `SI_elemdictSortHierElements (0x08140f30)`, `toposort.c`: (dictionary, chapter).
pub const SI_ELEMDICT_SORT_HIER_ELEMENTS: u32 = 0x0814_0f30;
/// `SI_dbListNodeAllocate (0x0812250c)`, `dbexternal.c`.
pub const SI_DB_LIST_NODE_ALLOCATE: u32 = 0x0812_250c;
/// `SI_elemdictGetNewName (0x0813697c)`, `elemdict.c`: (dictionary, chapter, name).
pub const SI_ELEMDICT_GET_NEW_NAME: u32 = 0x0813_697c;
/// `SI_modelChangeName (0x0846db14)`, `names.c`: (node, name).
pub const SI_MODEL_CHANGE_NAME: u32 = 0x0846_db14;
/// `SI_dbFormatFullName (0x081223dc)`, `dbexternal.c`: (name record).
pub const SI_DB_FORMAT_FULL_NAME: u32 = 0x0812_23dc;
/// `SI_elemdictAddElem (0x08132a5c)`, `elemdict.c`: (dictionary, chapter, name record, object, graf node, description, number).
pub const SI_ELEMDICT_ADD_ELEM: u32 = 0x0813_2a5c;
/// `SI_elemdictAddRelation (0x08132f78)`, `elemdict.c`: (dictionary, relation dictionary, from, to, relation, list, data).
pub const SI_ELEMDICT_ADD_RELATION: u32 = 0x0813_2f78;

/// The element of a model node in the current dictionary
/// [soft 0x080fcf20 SI_edGetElemFromModel]. A node whose element number (+0x354) is
/// negative is looked up first (`SI_elemdictFindElemByData`, MODELS chapter) and the
/// answer stored in the node. The number goes to `*index` when `index` is not null. Null
/// for a node that still has none; else the element (`SI_elemdictGetElem`).
pub fn ed_get_elem_from_model<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: Ptr32<Model>, index: Ptr32<i32>) -> Ptr32<Elem> {
    if m[model].elem_id < 0 {
        let (chapter, dict) = (m[SI_CID].model as u32, current_dict(m));
        m[model].elem_id = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict.0, chapter, model.0]) as i32;
    }
    if !index.is_null() {
        m[index] = m[model].elem_id;
    }
    let number = m[model].elem_id;
    if number < 0 {
        return Ptr32::NULL;
    }
    let (chapter, dict) = (m[SI_CID].model as u32, current_dict(m));
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict.0, chapter, number as u32]))
}

/// The first son of an element in the current dictionary [soft 0x080fcec0 SI_edFindSon]:
/// `SI_elemdictFindSon` with the current dictionary.
pub fn ed_find_son<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, elem: Ptr32<Elem>) -> Ptr32<Elem> {
    let dict = current_dict(m);
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_FIND_SON, &[dict.0, elem.0]))
}

/// The next son [soft 0x080fcef0 SI_edFindNextSon]: `SI_elemdictFindNextSon` with the
/// current dictionary.
pub fn ed_find_next_son<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, elem: Ptr32<Elem>) -> Ptr32<Elem> {
    let dict = current_dict(m);
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_FIND_NEXT_SON, &[dict.0, elem.0]))
}

/// The element of a 2D texture in the current dictionary
/// [soft 0x080fd3e0 SI_edGetElemFromTexture2D]: `SI_elemdictGetElemFromData` with the
/// current dictionary and the TEXTURES2D chapter.
pub fn ed_get_elem_from_texture2d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, texture: u32, index: Ptr32<i32>) -> Ptr32<Elem> {
    let (chapter, dict) = (m[SI_CID].t2d as u32, current_dict(m));
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict.0, chapter, texture, index.0]))
}

/// Brings the MODELS chapter's hierarchy up to date
/// [soft 0x080fd698 SI_elemdictUpdateModelHierarchy]: `SI_elemdictUpdateHierInfo(dictionary,
/// MODELS, 1, 0x6e)`, then `SI_elemdictSortHierElements(dictionary, MODELS)`.
pub fn elemdict_update_model_hierarchy<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: Ptr32<ElemDict>) {
    let chapter = m[SI_CID].model as u32;
    o.call_addr(m, SI_ELEMDICT_UPDATE_HIER_INFO, &[dict.0, chapter, 1, 0x6e]);
    let chapter = m[SI_CID].model as u32;
    o.call_addr(m, SI_ELEMDICT_SORT_HIER_ELEMENTS, &[dict.0, chapter]);
}

/// Gives a model node its element in a dictionary [soft 0x081017f4 SI_elemdictAddModel].
///
/// While `SI_addInDictionary` is 0 (a scene is being loaded: the elements are there
/// already) the node only learns its element's number: `SI_elemdictFindElemByData`,
/// stored at +0x354.
///
/// Otherwise a name record is made (`SI_dbListNodeAllocate`); with `unique` a new name is
/// asked for (`SI_elemdictGetNewName`), stored in the record and given to the node
/// (`SI_modelChangeName`); then the record's name becomes a `strdup` of the node's name
/// **whatever was stored before** (the pointer `SI_elemdictGetNewName` answered is
/// overwritten, not freed), its full name is formed (`SI_dbFormatFullName`), and the
/// element is added (`SI_elemdictAddElem`, MODELS, the record, the node, the graf node, 0,
/// 0). Its number goes to +0x354. A node with a father is related to the father's element
/// (`SI_elemdictFindElemByData`, then `SI_elemdictAddRelation` with relation 0x6e).
///
/// `SI_lightLicense` is a product-tier flag: the full product's path is taken (the test
/// of the element with `SI_edCheckForUneditable` is not made).
pub fn elemdict_add_model<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: Ptr32<ElemDict>, model: Ptr32<Model>, gn: Ptr32<GrafNode>, unique: i16) {
    if m[SI_ADD_IN_DICTIONARY] == 0 {
        let chapter = m[SI_CID].model as u32;
        m[model].elem_id = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict.0, chapter, model.0]) as i32;
        return;
    }
    let record: Ptr32<DbListNode> = Ptr32::new(o.call_addr(m, SI_DB_LIST_NODE_ALLOCATE, &[]));
    if unique != 0 {
        let (name, chapter) = (m[model].name, m[SI_CID].model as u32);
        let new = o.call_addr(m, SI_ELEMDICT_GET_NEW_NAME, &[dict.0, chapter, name.0]);
        m[record].name = Ptr32::new(new);
        o.call_addr(m, SI_MODEL_CHANGE_NAME, &[model.0, new]);
    }
    let name = m[model].name;
    m[record].name = Ptr32::new(m.strdup(name.0));
    let full = o.call_addr(m, SI_DB_FORMAT_FULL_NAME, &[record.0]);
    m[record].fullname = Ptr32::new(full);
    let chapter = m[SI_CID].model as u32;
    let number = o.call_addr(m, SI_ELEMDICT_ADD_ELEM, &[dict.0, chapter, record.0, model.0, gn.0, 0, 0]);
    let father = m[model].father;
    m[model].elem_id = number as i32;
    if !father.is_null() {
        let chapter = m[SI_CID].model as u32;
        let above = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict.0, chapter, father.0]);
        o.call_addr(m, SI_ELEMDICT_ADD_RELATION, &[dict.0, SI_RD.0, number, above, 0x6e, 1, 0]);
    }
}

/// Gives a model node its element in the current dictionary
/// [soft 0x08101ad4 SI_edAddModel]: `SI_elemdictAddModel` with the current dictionary;
/// `unique` is passed on sign-extended.
pub fn ed_add_model<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: Ptr32<Model>, gn: Ptr32<GrafNode>, unique: i16) {
    let dict = current_dict(m);
    elemdict_add_model(o, m, dict, model, gn, unique);
}

/// How many nodes lie below a node [soft 0x0846d5ec SI_modelNbSubModels] (`model.c`): 0
/// for a node without a son; else the nodes are walked, sons before brothers, climbing by
/// the father links to the nearest ancestor below `node` that has a brother, until the
/// walk is back at `node`. The count is answered as a signed 16-bit number.
///
/// The climb follows the fathers without a test for null: a node that is not its own
/// descendants' ancestor is read through a null pointer (here the recorded fault ends the
/// walk; the original is dead by then).
pub fn model_nb_sub_models(m: &Mem, node: Ptr32<Model>) -> i32 {
    if m[node].son.is_null() {
        return 0;
    }
    let mut count: i32 = 0;
    let mut at = node;
    loop {
        let mut next = m[at].son;
        if next.is_null() {
            next = m[at].brother;
        }
        if next.is_null() {
            let mut up = m[at].father;
            loop {
                if up == node {
                    let brother = m[at].brother;
                    next = if brother.is_null() { up } else { brother };
                    break;
                }
                at = up;
                let brother = m[at].brother;
                if !brother.is_null() {
                    next = brother;
                    break;
                }
                up = m[at].father;
                if m.fault().is_some() {
                    return (count as i16) as i32;
                }
            }
        }
        at = next;
        count = count.wrapping_add(1);
        if at == node {
            break;
        }
    }
    (count.wrapping_sub(1) as i16) as i32
}

/// The kind of graf node a light enters the scene graph's list with
/// [soft 0x082800fc SI_lightGetGrafType] (`mat_global.c`), by the light's 16-bit type at
/// +0x1e: 8 for 1 (point), 0x13 for 3 (infinite), 0x14 for 4 (sun), and 6 for 2 (spot) and
/// for every other value.
pub fn light_get_graf_type(m: &Mem, light: u32) -> i32 {
    match m.i16(light + 0x1e) {
        1 => 8,
        3 => 0x13,
        4 => 0x14,
        _ => 6,
    }
}

/// The elements take back from their nodes in the scene graph what the user may have
/// changed there, on the image [soft 0x08104acc SI_edGetElementsGrafNodeInfo] (read from
/// the listing; compared with the emulated original on the 2,000 cases of
/// `re/tools/si-scene/emu_gen_grafinfo.py`, finding 0018, section 18; the store form is
/// [`crate::savescene::get_elements_graf_node_info`]).
/// For every chapter of `dict` but the one `SI_cid` names at word 33 (AUDIO), for each
/// element whose 16-bit status (+0x48) is 0 and that has a node (+0x08):
///
/// - selection (+0x24): the node's (+0x08, 32 bits), unless bit 0x1000 of the node's 16-bit
///   flag word (+0x16) is set; then 1 for bit 0x2000, else 3 for bit 0x4000, else 2 for bit
///   0x8000, else 0;
/// - display type (+0x20): the node's (+0x0c, 32 bits);
/// - wire colour (+0x1c): the node's 16-bit colour (+0x12), sign-extended.
///
/// Then, in the MODELS chapter (`SI_cid` word 8), every element with an object (+0x04)
/// whose 32-bit depth (+0x54) is 0, whatever its status, gets nine floats behind +0x3c
/// (`SI_malloc(0x24)` when the pointer is null; the result is not tested): the node's
/// scaling (+0x100), rotation (+0x10c) and translation (+0x158), each loaded into the unit
/// and stored (`flds`, `fstps`). Both loops read their count again at every step.
///
/// The original counts the allocation in `SI_secureLicAlloc`, a counter of the licence
/// scheme, which is not ported.
pub fn ed_get_elements_graf_node_info(m: &mut Mem, dict: Ptr32<ElemDict>) {
    let d = dict.addr();
    let audio = m.i32(SI_CID.addr() + 0x84);
    let models = m.i32(SI_CID.addr() + 0x20);
    let mut chap = 0i32;
    while chap < m.i32(d) {
        if chap != audio {
            let l = m.u32(d + 4).wrapping_add((chap as u32).wrapping_mul(0x40));
            let mut i = 0i32;
            while i < m.i32(l + 4) {
                let e = m.u32(l + 8).wrapping_add((i as u32).wrapping_mul(0x84));
                let node = m.u32(e + 8);
                if m.i16(e + 0x48) == 0 && node != 0 {
                    let flags = m.u16(node + 0x16);
                    let sel = if flags & 0x1000 == 0 {
                        m.u32(node + 8)
                    } else if flags & 0x2000 != 0 {
                        1
                    } else if flags & 0x4000 != 0 {
                        3
                    } else {
                        ((flags as i16) < 0) as u32 * 2
                    };
                    m.set_u32(e + 0x24, sel);
                    let wire = m.u32(node + 0x0c);
                    m.set_u32(e + 0x20, wire);
                    let colour = m.i16(node + 0x12) as i32;
                    m.set_i32(e + 0x1c, colour);
                }
                let model = m.u32(e + 4);
                if chap == models && model != 0 && m.i32(e + 0x54) == 0 {
                    if m.u32(e + 0x3c) == 0 {
                        let block = m.si_malloc(0x24);
                        m.set_u32(e + 0x3c, block);
                    }
                    for (k, from) in [0x100u32, 0x104, 0x108, 0x10c, 0x110, 0x114, 0x158, 0x15c, 0x160].iter().enumerate() {
                        let v = si_x87::F80::from_f32(m.f32(model + from)).to_f32();
                        let block = m.u32(e + 0x3c);
                        m.set_f32(block.wrapping_add(4 * k as u32), v);
                    }
                }
                i += 1;
            }
        }
        chap += 1;
    }
}

// ---------------------------------------------------------------- the look-ups of the update
//
// Eight small routines of `edtools.c` that the scene's update (`si-motion`) calls out to;
// the dictionary's routines they call are `si-dict`'s, reached by address through
// [`ByAddr`] like those above, and so is `SI_activationFcvCheckTime` (`si-motion`'s).

/// `SI_elemdictFindRelatedElem (0x0813508c)`, `elemdict.c`: (dictionary, element, forward,
/// list, relation, chapter, &chapter, &index).
pub const SI_ELEMDICT_FIND_RELATED_ELEM: u32 = 0x0813_508c;
/// `SI_elemdictFindNextRelatedElem (0x08135154)`, `elemdict.c`: the same arguments.
pub const SI_ELEMDICT_FIND_NEXT_RELATED_ELEM: u32 = 0x0813_5154;
/// `SI_elemdictGetCurRelation (0x08134db0)`, `elemdict.c`: (dictionary, element, forward, list).
pub const SI_ELEMDICT_GET_CUR_RELATION: u32 = 0x0813_4db0;
/// `SI_activationFcvCheckTime (0x0822aa64)`, `motionUpdate.c`: (curve, time, &time out, 1);
/// answers a short.
pub const SI_ACTIVATION_FCV_CHECK_TIME: u32 = 0x0822_aa64;

/// The kind of an element's node in the scene graph [soft 0x080fcb5c SI_dbChapter2GrafType]:
/// `SI_elemdictGetElem(current dictionary, chapter, index)`; 0 for no element or an element
/// without node, else the node's first word.
pub fn db_chapter2graf_type<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, chap: i32, index: i32) -> i32 {
    let dict = current_dict(m);
    let e = o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict.0, chap as u32, index as u32]);
    if e == 0 {
        return 0;
    }
    let node = m.u32(e + 8);
    if node == 0 {
        return 0;
    }
    m.i32(node)
}

/// The element of a camera or a light, or of its interest's owner: `SI_elemdictGetElemFromData`
/// in the chapter; null when there is none; else the element it points at by `relid` in its
/// forward list 1 (`SI_elemdictFindRelatedElem`), whose index then goes to `*index`
/// **without a test of the pointer**, or the element itself when there is none.
fn elem_from_cam_or_light<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, chap: i32, relid: u32, object: u32, index: Ptr32<i32>) -> Ptr32<Elem> {
    let dict = current_dict(m);
    let e = o.call_addr(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict.0, chap as u32, object, index.0]);
    if e == 0 {
        return Ptr32::NULL;
    }
    // the two locals the look-up writes: the chapter (-0x8) and the index (-0x4)
    let locals = m.local(8);
    let dict = current_dict(m);
    let chap = m[SI_CID].cam;
    let chap = if relid == 0x456 { chap } else { m[SI_CID].lig };
    let r = o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict.0, e, 1, 1, relid, chap as u32, locals, locals + 4]);
    let found = m.i32(locals + 4);
    m.release(8);
    if r != 0 {
        m.set_i32(index.0, found);
        return Ptr32::new(r);
    }
    Ptr32::new(e)
}

/// [soft 0x080fcfb0 SI_edGetElemFromCamera]: the element of a camera record in the current
/// dictionary (CAMERAS chapter), or, when it has one, the element it points at by relation
/// 1110 (0x456) in its forward list 1 (an interest's camera), whose index goes to `*index`.
pub fn ed_get_elem_from_camera<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, camera: u32, index: Ptr32<i32>) -> Ptr32<Elem> {
    let chap = m[SI_CID].cam;
    elem_from_cam_or_light(o, m, chap, 0x456, camera, index)
}

/// [soft 0x080fd0bc SI_edGetElemFromLight]: the same for a light record (LIGHTS chapter,
/// relation 2110, 0x83e).
pub fn ed_get_elem_from_light<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, light: u32, index: Ptr32<i32>) -> Ptr32<Elem> {
    let chap = m[SI_CID].lig;
    elem_from_cam_or_light(o, m, chap, 0x83e, light, index)
}

/// The index of the element that points at element `index` of `chap` by `relid` in its
/// forward list 1 (looked up from the backward side of `index`'s element), else `index`.
fn owner_of_interest<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, chap: i32, relid: u32, index: i32) -> i32 {
    let dict = current_dict(m);
    let e = o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict.0, chap as u32, index as u32]);
    let locals = m.local(8);
    let dict = current_dict(m);
    let r = o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict.0, e, 0, 1, relid, chap as u32, locals, locals + 4]);
    let found = m.i32(locals + 4);
    m.release(8);
    if r != 0 {
        found
    } else {
        index
    }
}

/// [soft 0x080fd040 SI_edGetCamFromCamInt]: the camera an interest belongs to, by index in
/// CAMERAS: `SI_elemdictGetElem` of the interest (not tested for null), then
/// `SI_elemdictFindRelatedElem` on its backward list 1 with relation 1110; the index found,
/// or the interest's own when none.
pub fn ed_get_cam_from_cam_int<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, index: i32) -> i32 {
    let chap = m[SI_CID].cam;
    owner_of_interest(o, m, chap, 0x456, index)
}

/// [soft 0x080fd150 SI_edGetLigFromLigInt]: the same for a light's interest (LIGHTS,
/// relation 2110).
pub fn ed_get_lig_from_lig_int<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, index: i32) -> i32 {
    let chap = m[SI_CID].lig;
    owner_of_interest(o, m, chap, 0x83e, index)
}

/// [soft 0x080fd448 SI_edGetElemFromWave]: `SI_elemdictGetElemFromData` with the current
/// dictionary and the WAVES chapter (word 28 of `SI_cid`); the element it answers is left in
/// `eax` and the wave update reads it there (the source declares the routine `void`).
/// Held against the emulated original (`re/tools/si-scene/emu_gen_wave.py`).
pub fn ed_get_elem_from_wave<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, wave: u32, index: Ptr32<i32>) -> Ptr32<Elem> {
    let (chapter, dict) = (m[SI_CID].wav as u32, current_dict(m));
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict.0, chapter, wave, index.0]))
}

/// [soft 0x080fd4dc SI_edGetElemFromShape]: `SI_elemdictGetElemFromData` with the current
/// dictionary and the SHAPES chapter.
pub fn ed_get_elem_from_shape<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, shape: u32, index: Ptr32<i32>) -> Ptr32<Elem> {
    let (chapter, dict) = (m[SI_CID].shp as u32, current_dict(m));
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict.0, chapter, shape, index.0]))
}

/// [soft 0x080fd510 SI_edGetElemFromCluster]: `SI_elemdictGetElemFromData` with the current
/// dictionary and the CLUSTERS chapter (word 2 of `SI_cid`). Held against the emulated
/// original on sample scenes' images (finding 0018, section 20).
pub fn ed_get_elem_from_cluster<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, cluster: u32, index: Ptr32<i32>) -> Ptr32<Elem> {
    let (chapter, dict) = (m[SI_CID].cls as u32, current_dict(m));
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict.0, chapter, cluster, index.0]))
}

/// [soft 0x080fd47c SI_edGetElemFromLattice]: the element of a node's first lattice in the
/// LATTICES chapter (`SI_elemdictGetElemFromData`). With `branch` (16 bits) the list of
/// branch lattices (+0x58), else of node lattices (+0x50); a list's first word is the
/// lattice. Null when the list is null or its first word is; with `branch` set and no
/// branch list the node list is not looked at.
pub fn ed_get_elem_from_lattice<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, branch: i16, index: Ptr32<i32>) -> Ptr32<Elem> {
    let list = m.u32(model + if branch != 0 { 0x58 } else { 0x50 });
    let lattice = if list != 0 { m.u32(list) } else { 0 };
    if lattice == 0 {
        return Ptr32::NULL;
    }
    let (chapter, dict) = (m[SI_CID].lat as u32, current_dict(m));
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict.0, chapter, lattice, index.0]))
}

/// Whether an object is constrained by `relid` at `time` [soft 0x08106bbc SI_edIsItemConstrained].
///
/// The object's element is found by its chapter: a model node ([`ed_get_elem_from_model`]),
/// a camera ([`ed_get_elem_from_camera`]) or a light ([`ed_get_elem_from_light`]); any
/// other chapter answers 0. Then every entry of `relid` in the element's forward list 1,
/// any chapter (`SI_elemdictFindRelatedElem`, `…FindNextRelatedElem` in `dict`): with
/// `flag` (16 bits) 0 the first answers 1. Else the entry the look-up stands on
/// (`SI_elemdictGetCurRelation`) answers 1 when its 16-bit word +0x14 is set and either its
/// activation curve's element (ANIMATION, index +0x08) is not there or
/// `SI_activationFcvCheckTime(curve, time, &t, 1)` answers non-zero. 0 when no entry does.
/// The look-ups write the chapter into the routine's own `chap` argument and the index into
/// a local (here two locals of the image).
pub fn ed_is_item_constrained<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32, object: u32, chap: i32, relid: i32, flag: i16, time: f32) -> i16 {
    let locals = m.local(16);
    // -0x4: the index; the slot of `chap`; -0x8: the activation's time
    let (index, slot, t) = (locals, locals + 4, locals + 8);
    m.set_i32(slot, chap);
    let cid = m.read(SI_CID);
    let e = if chap == cid.model {
        ed_get_elem_from_model(o, m, Ptr32::new(object), Ptr32::new(index)).0
    } else if chap == cid.cam {
        ed_get_elem_from_camera(o, m, object, Ptr32::new(index)).0
    } else if chap == cid.lig {
        ed_get_elem_from_light(o, m, object, Ptr32::new(index)).0
    } else {
        m.release(16);
        return 0;
    };
    let mut answer = 0;
    let mut r = o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, e, 1, 1, relid as u32, 0xffff_ffff, slot, index]);
    while r != 0 {
        if flag == 0 {
            answer = 1;
            break;
        }
        let rel = o.call_addr(m, SI_ELEMDICT_GET_CUR_RELATION, &[dict, e, 1, 1]);
        if rel != 0 && m.i16(rel + 0x14) != 0 {
            let ani = m[SI_CID].ani as u32;
            let ae = o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict, ani, m.u32(rel + 8)]);
            if ae == 0 {
                answer = 1;
                break;
            }
            let curve = m.u32(ae + 4);
            if o.call_addr(m, SI_ACTIVATION_FCV_CHECK_TIME, &[curve, time.to_bits(), t, 1]) as u16 != 0 {
                answer = 1;
                break;
            }
        }
        r = o.call_addr(m, SI_ELEMDICT_FIND_NEXT_RELATED_ELEM, &[dict, e, 1, 1, relid as u32, 0xffff_ffff, slot, index]);
    }
    m.release(16);
    answer
}

// ---------------------------------------------------------------- a node's material and textures
//
// The relations between a model node and its material (300), its global 2D texture (400)
// and its 3D texture (500) in the current dictionary, made and taken away: the helpers the
// material conversion of the load (`SI_dbConvertModelMatTxt2DB`) calls. Compared with the
// emulated original on made-up dictionaries (vectors of `re/tools/si-scene/emu_gen_edmat.py`,
// `scene-emucheck`): the dictionary's routines ran as they are, `SI_schematicHierOperation`
// was a recorded stand-in.

/// `SI_elemdictRemoveRelationPair (0x0813362c)`, `elemdict.c`: (dictionary, relation
/// dictionary, from, to, relation, rule, unused).
pub use crate::untold::SI_ELEMDICT_REMOVE_RELATION_PAIR;
/// `SI_schematicHierOperation (0x0833c5dc)`, `graphsort.c`: (grafport, dictionary, element,
/// chapter, operation).
pub const SI_SCHEMATIC_HIER_OPERATION: u32 = 0x0833_c5dc;

/// The element of a material in the current dictionary
/// [soft 0x080fd288 SI_edGetElemFromMaterial]: `SI_elemdictGetElemFromData` with the current
/// dictionary and the MATERIALS chapter.
pub fn ed_get_elem_from_material<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, material: u32, index: Ptr32<i32>) -> Ptr32<Elem> {
    let (chapter, dict) = (m[SI_CID].mat as u32, current_dict(m));
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict.0, chapter, material, index.0]))
}

/// The element of a 3D texture in the current dictionary
/// [soft 0x080fd414 SI_edGetElemFromTexture3D]: `SI_elemdictGetElemFromData` with the
/// current dictionary and the TEXTURES3D chapter.
pub fn ed_get_elem_from_texture3d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, texture: u32, index: Ptr32<i32>) -> Ptr32<Elem> {
    let (chapter, dict) = (m[SI_CID].t3d as u32, current_dict(m));
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict.0, chapter, texture, index.0]))
}

/// A look-up from an object to its element and index, as the three `SI_edGetElemFrom…`
/// above take them.
type FindElem<O> = fn(&mut O, &mut Mem, u32, Ptr32<i32>) -> Ptr32<Elem>;

/// What the three `SI_edModelSet…` routines share: two locals at −1 (the node's index at
/// −0x4, the other's at −0x8), the node's element ([`ed_get_elem_from_model`]) and the
/// other's (`find`), whose answer is not used; then `SI_elemdictAddRelation(current
/// dictionary, SI_RD, node's index, other's index, relid, 1, 0)` whatever the indices are,
/// and `SI_schematicHierOperation(null, current dictionary, node's element, MODELS, 0x14)`.
fn model_set_relation<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: Ptr32<Model>, other: u32, relid: u32, find: FindElem<O>) {
    let locals = m.local(8);
    let (mine, theirs) = (locals + 4, locals);
    m.set_i32(mine, -1);
    m.set_i32(theirs, -1);
    let elem = ed_get_elem_from_model(o, m, model, Ptr32::new(mine));
    find(o, m, other, Ptr32::new(theirs));
    let (from, to) = (m.u32(mine), m.u32(theirs));
    let dict = current_dict(m);
    o.call_addr(m, SI_ELEMDICT_ADD_RELATION, &[dict.0, SI_RD.addr(), from, to, relid, 1, 0]);
    let (chapter, dict) = (m[SI_CID].model as u32, current_dict(m));
    o.call_addr(m, SI_SCHEMATIC_HIER_OPERATION, &[0, dict.0, elem.0, chapter, 0x14]);
    m.release(8);
}

/// What the three `SI_edModelRelax…` routines share: the node's index
/// (`SI_elemdictFindElemByData`, MODELS), then the other's (the same in the chapter `chap`
/// picks from `SI_cid`, read after the first look-up), then
/// `SI_elemdictRemoveRelationPair(current dictionary, SI_RD, node's, other's, relid, 1, 0)`
/// whatever the indices are.
fn model_relax_relation<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, other: u32, relid: u32, chap: fn(&ChapIds) -> i32) {
    let (chapter, dict) = (m[SI_CID].model as u32, current_dict(m));
    let from = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict.0, chapter, model]);
    let (chapter, dict) = (chap(&m[SI_CID]) as u32, current_dict(m));
    let to = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict.0, chapter, other]);
    let dict = current_dict(m);
    o.call_addr(m, SI_ELEMDICT_REMOVE_RELATION_PAIR, &[dict.0, SI_RD.addr(), from, to, relid, 1, 0]);
}

/// Relates a node to its material [soft 0x08100c34 SI_edModelSetMaterial]: relation 300
/// (`SI_edGetElemFromMaterial` for the material's index), then the schematic's operation
/// 0x14 on the node's element (see `model_set_relation`).
pub fn ed_model_set_material<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: Ptr32<Model>, material: u32) {
    model_set_relation(o, m, model, material, 300, ed_get_elem_from_material::<O>);
}

/// Takes a node's relation 300 to a material away [soft 0x08100cd8 SI_edModelRelaxMaterial]
/// (the material looked up in MATERIALS; see `model_relax_relation`).
pub fn ed_model_relax_material<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, material: u32) {
    model_relax_relation(o, m, model, material, 300, |c| c.mat);
}

/// Relates a node to a global 2D texture [soft 0x08101130 SI_edModelSetGTexture2D]:
/// relation 400 ([`ed_get_elem_from_texture2d`] for the texture's index), then the
/// schematic's operation 0x14 on the node's element.
pub fn ed_model_set_g_texture2d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: Ptr32<Model>, texture: u32) {
    model_set_relation(o, m, model, texture, 400, ed_get_elem_from_texture2d::<O>);
}

/// Takes a node's relation 400 to a 2D texture away
/// [soft 0x081011d4 SI_edModelRelaxGTexture2D] (the texture looked up in TEXTURES2D).
pub fn ed_model_relax_g_texture2d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, texture: u32) {
    model_relax_relation(o, m, model, texture, 400, |c| c.t2d);
}

/// Relates a node to a 3D texture [soft 0x08100e4c SI_edModelSetGTexture3D]: relation 500
/// ([`ed_get_elem_from_texture3d`] for the texture's index), then the schematic's
/// operation 0x14 on the node's element.
pub fn ed_model_set_g_texture3d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: Ptr32<Model>, texture: u32) {
    model_set_relation(o, m, model, texture, 500, ed_get_elem_from_texture3d::<O>);
}

/// Takes a node's relation 500 to a 3D texture away
/// [soft 0x08100ef0 SI_edModelRelaxGTexture3D] (the texture looked up in TEXTURES3D).
pub fn ed_model_relax_g_texture3d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, texture: u32) {
    model_relax_relation(o, m, model, texture, 500, |c| c.t3d);
}

// The second half: a material's element and its local textures, and the material curves'
// flags (vectors of `re/tools/si-scene/emu_gen_edmat.py` as well: the name records'
// routines and the dictionary's ran as they are; `SI_schematicHierOperation` and
// `SI_attdictSetFcurveFlag` were recorded stand-ins).

/// `SI_attdictSetFcurveFlag (0x0811ff18)`, `attdict.c`: (attribute dictionary, curve id, flag).
pub const SI_ATTDICT_SET_FCURVE_FLAG: u32 = 0x0811_ff18;

/// Gives a material its element in the current dictionary [soft 0x0810203c SI_edAddMaterial].
///
/// 1 at once while `SI_addInDictionary` (16 bits) is 0. A material that already has an
/// element (`SI_edGetElemFromMaterial`, its index into an uninitialised local) answers 0.
/// Otherwise, with `unique`, the name `SI_elemdictGetNewName(current dictionary, MATERIALS,
/// material)` answers is copied over the material's own (its first 0x1d bytes, `strncpy`,
/// then a zero at +0x1d); a name record is made (`SI_dbListNodeAllocate`), its name a
/// `strdup` of the material's, its full name formed (`SI_dbFormatFullName`); the element is
/// added (`SI_elemdictAddElem(current dictionary, MATERIALS, record, material, graf node, 0,
/// 0)`) and its number stored at +0xda8; 1. The chapter is read once, before the name.
pub fn ed_add_material<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, gn: u32, unique: i16, material: u32) -> i32 {
    if m[SI_ADD_IN_DICTIONARY] == 0 {
        return 1;
    }
    let index = m.local(4);
    let found = ed_get_elem_from_material(o, m, material, Ptr32::new(index));
    m.release(4);
    if !found.is_null() {
        return 0;
    }
    let chapter = m[SI_CID].mat as u32;
    if unique != 0 {
        let dict = current_dict(m);
        let name = o.call_addr(m, SI_ELEMDICT_GET_NEW_NAME, &[dict.0, chapter, material]);
        si_mem::clib::strncpy(m, material, name, 0x1d);
        m.set_u8(material.wrapping_add(0x1d), 0);
    }
    let record: Ptr32<DbListNode> = Ptr32::new(o.call_addr(m, SI_DB_LIST_NODE_ALLOCATE, &[]));
    m[record].name = Ptr32::new(m.strdup(material));
    let full = o.call_addr(m, SI_DB_FORMAT_FULL_NAME, &[record.0]);
    m[record].fullname = Ptr32::new(full);
    let dict = current_dict(m);
    let number = o.call_addr(m, SI_ELEMDICT_ADD_ELEM, &[dict.0, chapter, record.0, material, gn, 0, 0]);
    m.set_u32(material.wrapping_add(0xda8), number);
    1
}

/// What the two `SI_edModelSetLTexture…` routines share: four locals (the material's index
/// at −0x4, the texture's at −0x8, both −1; the related element's index at −0xc, −1, and
/// its chapter at −0x10, not set); the material's element and the texture's (`find`, whose
/// answer is not used); `SI_elemdictAddRelation(current dictionary, SI_RD, material's
/// index, texture's index, relid, 1, 0)` whatever the indices are; then the node the
/// material belongs to, `SI_elemdictFindRelatedElem(current dictionary, material's element,
/// 0, 0, 300, MODELS, &chapter, &index)`: `SI_schematicHierOperation(null, current
/// dictionary, that node, MODELS, 0x14)`, or with no node the same on the material's
/// element and MATERIALS.
fn material_set_texture<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, material: u32, texture: u32, relid: u32, find: FindElem<O>) {
    let locals = m.local(16);
    let (mine, theirs, index, chap) = (locals + 12, locals + 8, locals + 4, locals);
    for at in [mine, theirs, index] {
        m.set_i32(at, -1);
    }
    let elem = ed_get_elem_from_material(o, m, material, Ptr32::new(mine));
    find(o, m, texture, Ptr32::new(theirs));
    let (from, to) = (m.u32(mine), m.u32(theirs));
    let dict = current_dict(m);
    o.call_addr(m, SI_ELEMDICT_ADD_RELATION, &[dict.0, SI_RD.addr(), from, to, relid, 1, 0]);
    let (chapter, dict) = (m[SI_CID].model as u32, current_dict(m));
    let node = o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict.0, elem.0, 0, 0, 300, chapter, chap, index]);
    let (what, chapter) = if node != 0 { (node, m[SI_CID].model) } else { (elem.0, m[SI_CID].mat) };
    let dict = current_dict(m);
    o.call_addr(m, SI_SCHEMATIC_HIER_OPERATION, &[0, dict.0, what, chapter as u32, 0x14]);
    m.release(16);
}

/// Relates a material to a local 2D texture [soft 0x0810126c SI_edModelSetLTexture2D]:
/// relation 401 ([`ed_get_elem_from_texture2d`] for the texture's index), then the
/// schematic's operation 0x14 on the material's node (see `material_set_texture`).
pub fn ed_model_set_l_texture2d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, material: u32, texture: u32) {
    material_set_texture(o, m, material, texture, 401, ed_get_elem_from_texture2d::<O>);
}

/// Relates a material to a local 3D texture [soft 0x08100f88 SI_edModelSetLTexture3D]:
/// relation 501 ([`ed_get_elem_from_texture3d`] for the texture's index), then the
/// schematic's operation 0x14 on the material's node.
pub fn ed_model_set_l_texture3d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, material: u32, texture: u32) {
    material_set_texture(o, m, material, texture, 501, ed_get_elem_from_texture3d::<O>);
}

/// The curve ids [`attdict_flag_all_material_fcurves`] flags, in its order.
pub const MATERIAL_FCURVES: [u32; 15] = [0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x39, 0x3b, 0x101, 0x102];

/// Flags the material's animatable parameters in the attribute dictionary
/// [soft 0x080fd2bc SI_attdictFlagAllMaterialFcurves]: `SI_attdictSetFcurveFlag(&SI_ATD,
/// id, 1)` for each id of [`MATERIAL_FCURVES`], the answers not looked at.
pub fn attdict_flag_all_material_fcurves<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem) {
    for id in MATERIAL_FCURVES {
        o.call_addr(m, SI_ATTDICT_SET_FCURVE_FLAG, &[si_dict::globals::SI_ATD.addr(), id, 1]);
    }
}

// A shader's element and its relation to its owner (vectors of
// `re/tools/si-scene/emu_gen_edmat.py`, the third part: the dictionary's and the name
// records' routines ran as they are; `SI_udfAttachArrayElements` was a recorded stand-in).

/// `SI_elemdictGetRelationFromIds (0x08134e9c)`, `elemdict.c`: (dictionary, element,
/// relation, chapter, index, serial).
pub const SI_ELEMDICT_GET_RELATION_FROM_IDS: u32 = 0x0813_4e9c;
/// `SI_elemdictAddUnCompleteRelation (0x08132fb4)`, `elemdict.c`: (dictionary, relation
/// dictionary, from chapter, to chapter, from, to, relation, rule, position, data, flag).
pub const SI_ELEMDICT_ADD_UN_COMPLETE_RELATION: u32 = 0x0813_2fb4;
/// `SI_elemdictSetDirty (0x08132be8)`, `elemdict.c`: (element, flag).
pub const SI_ELEMDICT_SET_DIRTY: u32 = 0x0813_2be8;
/// `SI_udfAttachArrayElements (0x08545584)`, `custom_udf.c`: (shader);
/// [`crate::custom::udf_attach_array_elements`].
pub const SI_UDF_ATTACH_ARRAY_ELEMENTS: u32 = 0x0854_5584;
/// The relation from an owner to its shader (550).
pub const UDF_RELATION: u32 = 0x226;

/// The scene's element [soft 0x080fd620 SI_edGetElemFromScene]: 0 to `*index`, then
/// `SI_elemdictGetElem(current dictionary, SCENES, 0)`.
pub fn ed_get_elem_from_scene<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, index: Ptr32<i32>) -> Ptr32<Elem> {
    m.set_i32(index.0, 0);
    let (chapter, dict) = (m[SI_CID].dsc as u32, current_dict(m));
    Ptr32::new(o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict.0, chapter, 0]))
}

/// Relates an owner to its shader [soft 0x08103a24 SI_edSetUDFRelation].
///
/// The owner's element and index (a local, not set beforehand): with a null owner and
/// `chapter` the database's SCENES (`SI_dbcid`), the scene's ([`ed_get_elem_from_scene`]);
/// else with `chapter` the scene's LIGHTS (`SI_cid`), the light's
/// ([`ed_get_elem_from_light`]); else `SI_elemdictFindElemByData(current dictionary,
/// chapter, owner)` and `SI_elemdictGetElem` with that index. Then the shader's index
/// (`SI_elemdictFindElemByData` in `udf_chapter`); when `SI_elemdictGetRelationFromIds(current
/// dictionary, owner's element, 550, udf_chapter, shader's index, −1)` finds no relation,
/// `SI_elemdictAddUnCompleteRelation(current dictionary, SI_RD, chapter, udf_chapter,
/// owner's index, shader's index, 550, 1, 0, null, 1)`, the indices whatever they are.
pub fn ed_set_udf_relation<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, owner: u32, chapter: i32, udf: u32, udf_chapter: i32) {
    let local = m.local(4);
    let elem = if owner == 0 && chapter == m[SI_DBCID].dsc {
        ed_get_elem_from_scene(o, m, Ptr32::new(local))
    } else if chapter == m[SI_CID].lig {
        ed_get_elem_from_light(o, m, owner, Ptr32::new(local))
    } else {
        let dict = current_dict(m);
        let index = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict.0, chapter as u32, owner]);
        m.set_u32(local, index);
        let dict = current_dict(m);
        Ptr32::new(o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict.0, chapter as u32, index]))
    };
    let dict = current_dict(m);
    let to = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict.0, udf_chapter as u32, udf]);
    let dict = current_dict(m);
    let found = o.call_addr(m, SI_ELEMDICT_GET_RELATION_FROM_IDS, &[dict.0, elem.0, UDF_RELATION, udf_chapter as u32, to, 0xffff_ffff]);
    if found == 0 {
        let from = m.u32(local);
        let dict = current_dict(m);
        o.call_addr(m, SI_ELEMDICT_ADD_UN_COMPLETE_RELATION, &[dict.0, SI_RD.addr(), chapter as u32, udf_chapter as u32, from, to, UDF_RELATION, 1, 0, 0, 1]);
    }
    m.release(4);
}

/// Gives a shader (`__SI_UDF`) its element and relates it to its owner
/// [soft 0x08103b5c SI_edAddUDF].
///
/// Nothing while `SI_addInDictionary` (16 bits) is 0, for a null shader, or for a null
/// owner unless `chapter` is the database's SCENES (`SI_dbcid`). The shader's chapter is
/// its +4, read again at each use. A shader that has no element
/// (`SI_elemdictFindElemByData` negative) gets the name `SI_elemdictGetNewName(current
/// dictionary, its chapter, its name)` answers (`SI_udfSetName`), a name record
/// (`SI_dbListNodeAllocate`, the name a `strdup` of the shader's, the full name
/// `SI_dbFormatFullName`'s) and an element (`SI_elemdictAddElem(current dictionary, its
/// chapter, record, shader, 0, 0, 0)`), whose number goes to its +8. Then
/// [`ed_set_udf_relation`], `SI_udfAttachArrayElements(shader)`, and
/// `SI_elemdictSetDirty(SI_elemdictGetElem(current dictionary, its chapter, index), its
/// 16-bit +0xe)`; the element is not tested (a null one is written at 0x40, as in the
/// original).
pub fn ed_add_udf<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, owner: u32, chapter: i32, udf: u32) {
    if m[SI_ADD_IN_DICTIONARY] == 0 || udf == 0 || (owner == 0 && chapter != m[SI_DBCID].dsc) {
        return;
    }
    let dict = current_dict(m);
    let mut index = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict.0, m.u32(udf + 4), udf]);
    if (index as i32) < 0 {
        let (name, chap, dict) = (m.u32(udf), m.u32(udf + 4), current_dict(m));
        let name = o.call_addr(m, SI_ELEMDICT_GET_NEW_NAME, &[dict.0, chap, name]);
        si_hrc::image::udf::udf_set_name(m, Ptr32::new(udf), name);
        let record: Ptr32<DbListNode> = Ptr32::new(o.call_addr(m, SI_DB_LIST_NODE_ALLOCATE, &[]));
        let copy = m.strdup(m.u32(udf));
        m[record].name = Ptr32::new(copy);
        let full = o.call_addr(m, SI_DB_FORMAT_FULL_NAME, &[record.0]);
        m[record].fullname = Ptr32::new(full);
        let (chap, dict) = (m.u32(udf + 4), current_dict(m));
        index = o.call_addr(m, SI_ELEMDICT_ADD_ELEM, &[dict.0, chap, record.0, udf, 0, 0, 0]);
        m.set_u32(udf + 8, index);
    }
    let udf_chapter = m.i32(udf + 4);
    ed_set_udf_relation(o, m, owner, chapter, udf, udf_chapter);
    o.call_addr(m, SI_UDF_ATTACH_ARRAY_ELEMENTS, &[udf]);
    let (chap, dict) = (m.u32(udf + 4), current_dict(m));
    let elem = o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict.0, chap, index]);
    let dirty = m.i16(udf + 0xe) as i32 as u32;
    o.call_addr(m, SI_ELEMDICT_SET_DIRTY, &[elem, dirty]);
}

// The textures' elements and a shared material or 3D texture made a node's own (vectors of
// `re/tools/si-scene/emu_gen_edmat.py`, the sixth part: the dictionary's and the name
// records' routines and this file's ran as they are; the callees below were recorded
// stand-ins).

/// `SI_materialCopy (0x08284888)`, `mat_dial.c`: (material); [`crate::matconv::material_copy`].
pub const SI_MATERIAL_COPY: u32 = 0x0828_4888;
/// `SI_txt2dCopy (0x082adfbc)`, `t2d_dial.c`: (texture); [`crate::matconv::txt2d_copy`].
pub const SI_TXT2D_COPY: u32 = 0x082a_dfbc;
/// `SI_txt3dCopy (0x082b4008)`, `t3d_dial.c`: (texture); [`crate::matconv::txt3d_copy`].
pub const SI_TXT3D_COPY: u32 = 0x082b_4008;
/// `SI_txt2dSetWrap (0x082ad9f8)`, `t2d_dial.c`: (node, texture).
pub const SI_TXT2D_SET_WRAP: u32 = 0x082a_d9f8;
/// `SI_copyT2DShader (0x082a6864)`, `shd_fct.c`: (texture, shader);
/// [`crate::matconv::copy_t2d_shader`].
pub const SI_COPY_T2D_SHADER: u32 = 0x082a_6864;
/// `SI_copyT3DShader (0x082a68cc)`, `shd_fct.c`: (texture, shader);
/// [`crate::matconv::copy_t3d_shader`].
pub const SI_COPY_T3D_SHADER: u32 = 0x082a_68cc;
/// `SI_texture2DLoadFcurveAll (0x0822d890)`: (texture, the texture whose curves it takes);
/// `si-motion`'s `animcopy::texture2d_load_fcurve_all`.
pub const SI_TEXTURE2D_LOAD_FCURVE_ALL: u32 = 0x0822_d890;
/// `SI_texture3DLoadFcurveAll (0x0822d914)`: the same for a 3D texture; `si-motion`'s
/// `animcopy::texture3d_load_fcurve_all`.
pub const SI_TEXTURE3D_LOAD_FCURVE_ALL: u32 = 0x0822_d914;
/// `SI_elemdictDisconnectANDConnectRel (0x08133780)`, `elemrel.c`: (dictionary, relation
/// dictionary, source, from, to, relation); `si-dict`'s `elemrel::disconnect_and_connect_rel`.
pub const SI_ELEMDICT_DISCONNECT_AND_CONNECT_REL: u32 = 0x0813_3780;
/// `SI_elemdictCopyElemAnim (0x0822bd8c)`: (dictionary, from, to, chapter, from's index,
/// to's index); `si-motion`'s `animcopy`.
pub const SI_ELEMDICT_COPY_ELEM_ANIM: u32 = 0x0822_bd8c;
/// `SI_texture2DVerifyPicturePath (0x082b1528)`, `t2d_dial.c`: (database directory, database
/// information, dictionary, element, 0, 0); [`crate::t2dpath::verify_picture_path`].
pub const SI_TEXTURE2D_VERIFY_PICTURE_PATH: u32 = 0x082b_1528;
/// `SI_dbdir` (0x08781390), the database directory record.
pub const SI_DBDIR: u32 = 0x0878_1390;
/// `SI_pDbiin` (0x08782058), the input database's information (a pointer).
pub const SI_P_DBIIN: u32 = 0x0878_2058;
/// The scene graph lists' kinds of a 2D and a 3D texture (`SI_graflistAddData`).
const GRAF_T2D: u32 = 0xf;
const GRAF_T3D: u32 = 0x10;

/// `&ed->lists[chapter].elems[index]`, as the original computes it: no test.
fn elem_at(m: &Mem, ed: u32, chapter: i32, index: i32) -> u32 {
    let lists = m.u32(ed.wrapping_add(4));
    let elems = m.u32(lists.wrapping_add((chapter as u32).wrapping_mul(0x40)).wrapping_add(8));
    elems.wrapping_add((index as u32).wrapping_mul(0x84))
}

/// Gives a 3D texture its element in the current dictionary
/// [soft 0x081027e0 SI_edAddTexture3D].
///
/// 1 at once while `SI_addInDictionary` (16 bits) is 0. A texture that already has an
/// element (`SI_edGetElemFromTexture3D`, its index into an uninitialised local) answers 0.
/// Otherwise, with `unique`, the texture's name (+0) becomes a `strdup` of
/// `SI_elemdictGetNewName(current dictionary, TEXTURES3D, its name)`, the old one freed
/// first when it is not null. A name record is made (its name a `strdup` of the texture's,
/// its full name `SI_dbFormatFullName`'s) and the element added
/// (`SI_elemdictAddElem(current dictionary, TEXTURES3D, record, texture, graf node, 0, 0)`);
/// its number is not kept. 1. The chapter is read once, before the name.
pub fn ed_add_texture3d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, gn: u32, unique: i16, texture: u32) -> i32 {
    if m[SI_ADD_IN_DICTIONARY] == 0 {
        return 1;
    }
    let index = m.local(4);
    let found = ed_get_elem_from_texture3d(o, m, texture, Ptr32::new(index));
    m.release(4);
    if !found.is_null() {
        return 0;
    }
    let chapter = m[SI_CID].t3d as u32;
    if unique != 0 {
        let dict = current_dict(m);
        let name = o.call_addr(m, SI_ELEMDICT_GET_NEW_NAME, &[dict.0, chapter, m.u32(texture)]);
        let old = m.u32(texture);
        if old != 0 {
            m.free(old);
            m.set_u32(texture, 0);
        }
        let copy = m.strdup(name);
        m.set_u32(texture, copy);
    }
    let record: Ptr32<DbListNode> = Ptr32::new(o.call_addr(m, SI_DB_LIST_NODE_ALLOCATE, &[]));
    m[record].name = Ptr32::new(m.strdup(m.u32(texture)));
    let full = o.call_addr(m, SI_DB_FORMAT_FULL_NAME, &[record.0]);
    m[record].fullname = Ptr32::new(full);
    let dict = current_dict(m);
    o.call_addr(m, SI_ELEMDICT_ADD_ELEM, &[dict.0, chapter, record.0, texture, gn, 0, 0]);
    1
}

/// Gives a 2D texture (`__SI_LclMap`) its element in the current dictionary
/// [soft 0x08102314 SI_edAddTexture2D].
///
/// 1 at once while `SI_addInDictionary` (16 bits) is 0; 0 for a texture that already has an
/// element ([`ed_get_elem_from_texture2d`]). Otherwise the chapter is read once and a name
/// record made (`SI_dbListNodeAllocate`) before the name is looked at:
/// - a texture with no name (+0x18) gets a `strdup` of `SI_elemdictGetNewName(current
///   dictionary, TEXTURES2D, "t2d")`;
/// - with `unique`, the name becomes a `strdup` of `SI_elemdictGetNewName` of it, the old
///   one not freed;
/// - the record's name is a `strdup` of the last name (the look-up's answer with `unique`),
///   its full name `SI_dbFormatFullName`'s.
///
/// The element is added (`SI_elemdictAddElem(current dictionary, TEXTURES2D, record,
/// texture, graf node, 0, 0)`), its number stored in the local and at +0x190. Last comes
/// `SI_texture2DVerifyPicturePath(&SI_dbdir, SI_pDbiin, current dictionary,
/// &lists[TEXTURES2D].elems[number], 0, 0)`, the dictionary read again and the number not
/// tested; 1.
pub fn ed_add_texture2d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, gn: u32, unique: i16, texture: u32) -> i32 {
    if m[SI_ADD_IN_DICTIONARY] == 0 {
        return 1;
    }
    let index = m.local(4);
    let found = ed_get_elem_from_texture2d(o, m, texture, Ptr32::new(index));
    if !found.is_null() {
        m.release(4);
        return 0;
    }
    let chapter = m[SI_CID].t2d;
    let record: Ptr32<DbListNode> = Ptr32::new(o.call_addr(m, SI_DB_LIST_NODE_ALLOCATE, &[]));
    if m.u32(texture + 0x18) == 0 {
        let dict = current_dict(m);
        let name = o.call_addr(m, SI_ELEMDICT_GET_NEW_NAME, &[dict.0, chapter as u32, crate::globals::S_T2D_ED]);
        let copy = m.strdup(name);
        m.set_u32(texture + 0x18, copy);
    }
    let mut name = m.u32(texture + 0x18);
    if unique != 0 {
        let dict = current_dict(m);
        name = o.call_addr(m, SI_ELEMDICT_GET_NEW_NAME, &[dict.0, chapter as u32, name]);
        let copy = m.strdup(name);
        m.set_u32(texture + 0x18, copy);
    }
    m[record].name = Ptr32::new(m.strdup(name));
    let full = o.call_addr(m, SI_DB_FORMAT_FULL_NAME, &[record.0]);
    m[record].fullname = Ptr32::new(full);
    let dict = current_dict(m);
    let number = o.call_addr(m, SI_ELEMDICT_ADD_ELEM, &[dict.0, chapter as u32, record.0, texture, gn, 0, 0]);
    m.set_u32(index, number);
    m.set_u32(texture + 0x190, number);
    let dict = current_dict(m);
    let elem = elem_at(m, dict.0, chapter, number as i32);
    let dbiin = m.u32(SI_P_DBIIN);
    o.call_addr(m, SI_TEXTURE2D_VERIFY_PICTURE_PATH, &[SI_DBDIR, dbiin, dict.0, elem, 0, 0]);
    m.release(4);
    1
}

/// What [`ed_disconnect_shared_material_or3d_txt`] does for each 2D texture of the shared
/// material: `SI_txt2dCopy(texture)`, method 4 (+0x2a, 16 bits) made 6 when the node's +0x30
/// is 4; then `SI_txt2dSetWrap(node, copy)`, `SI_graflistAddData(scene graph list, copy,
/// 0xf, 0)`, `SI_copyT2DShader(copy, texture's shader +0x198)`,
/// [`ed_model_set_l_texture2d`]`(material, copy)` and `SI_texture2DLoadFcurveAll(copy,
/// texture)`. The copy is stored at `at` and read back from there for each call.
fn copy_local_texture2d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, material: u32, model: u32, texture: u32, at: u32) {
    let copy = o.call_addr(m, SI_TXT2D_COPY, &[texture]);
    m.set_u32(at, copy);
    if m.u32(model.wrapping_add(0x30)) == 4 && m.u16(copy.wrapping_add(0x2a)) == 4 {
        m.set_u16(copy.wrapping_add(0x2a), 6);
    }
    o.call_addr(m, SI_TXT2D_SET_WRAP, &[model, m.u32(at)]);
    let list = crate::dispose::graf_list(m);
    o.call_addr(m, crate::wave::SI_GRAFLIST_ADD_DATA, &[list, m.u32(at), GRAF_T2D, 0]);
    let shader = m.u32(texture.wrapping_add(0x198));
    o.call_addr(m, SI_COPY_T2D_SHADER, &[m.u32(at), shader]);
    let copy = m.u32(at);
    ed_model_set_l_texture2d(o, m, material, copy);
    o.call_addr(m, SI_TEXTURE2D_LOAD_FCURVE_ALL, &[m.u32(at), texture]);
}

/// Gives an element a material or a 3D texture of its own in place of the shared one
/// [soft 0x0810080c SI_edDisconnectSharedMaterialOr3DTxt]: `(dictionary, element, source,
/// shared element, index, material, slot, global)`; `material` and `global` are 16 bits.
///
/// **The copy.** `SI_materialCopy` or `SI_txt3dCopy` of the shared element's object (+4).
/// `SI_addInDictionary` is 1 while the copy gets its element:
/// - **A material** (MATERIALS, relation 300): [`ed_add_material`]`(null, 1, copy)`, the
///   number its +0xda8. The shared material is the object of `elems[index]` of the
///   dictionary passed, the node the element's object. Each of its local 2D textures (from
///   +0x6ec, linked through +0x16c) is copied (`copy_local_texture2d`). The first goes to the
///   copy's +0x6ec, and +0x6f0, with 1 at +0x6e8 (16 bits), when the copy has none there yet;
///   every other one is linked behind the last (+0x16c), +0x6e8 counting up. A local 3D
///   texture (+0x6fc) gets `SI_txt3dCopy`, `SI_graflistAddData(…, 0x10, 0)`,
///   `SI_copyT3DShader(copy, its shader +0x14c)`, [`ed_model_set_l_texture3d`] and
///   `SI_texture3DLoadFcurveAll`.
/// - **A 3D texture** (TEXTURES3D, relation 500 when `global`, else 501):
///   [`ed_add_texture3d`]`(null, 1, copy)`, the number [`ed_get_elem_from_texture3d`]'s.
///
/// **The relation moved.** With a number not negative, `SI_elemdictGetRelationFromIds(
/// dictionary, element, relation, chapter, index, -1)`; when there is one,
/// `SI_elemdictDisconnectANDConnectRel(dictionary, SI_RD, source, &elems[index],
/// &elems[number], relation)` and the relation's +4 becomes the number. The copy goes to
/// the element's object: the material at `slot` of the array at +0x20c, the 3D texture at
/// +0x224 when `global`, else +0x6fc. The new element's +0x5c becomes `index`, then
/// `SI_elemdictCopyElemAnim(dictionary, &elems[index], &elems[number], chapter, index,
/// number)`. The elements' array is read once, after the copy got its element.
#[allow(clippy::too_many_arguments)]
pub fn ed_disconnect_shared_material_or3d_txt<O: ByAddr + ?Sized>(
    o: &mut O,
    m: &mut Mem,
    ed: u32,
    elem: u32,
    source: i32,
    shared: u32,
    index: i32,
    material: i16,
    slot: i32,
    global: i16,
) {
    let number = m.local(4);
    let (mut copy, mut t3d) = (0, 0);
    let object = m.u32(shared.wrapping_add(4));
    if material != 0 {
        copy = o.call_addr(m, SI_MATERIAL_COPY, &[object]);
    } else {
        t3d = o.call_addr(m, SI_TXT3D_COPY, &[object]);
    }
    m[SI_ADD_IN_DICTIONARY] = 1;
    let (chapter, relid);
    if material != 0 {
        chapter = m[SI_CID].mat;
        relid = 300;
        ed_add_material(o, m, 0, 1, copy);
        m.set_u32(number, m.u32(copy.wrapping_add(0xda8)));
        let shared_mat = m.u32(elem_at(m, ed, chapter, index).wrapping_add(4));
        let model = m.u32(elem.wrapping_add(4));
        let mut last = m.u32(copy.wrapping_add(0x6ec));
        let mut texture = m.u32(shared_mat.wrapping_add(0x6ec));
        while texture != 0 {
            if m.u32(copy.wrapping_add(0x6ec)) == 0 {
                m.set_u16(copy.wrapping_add(0x6e8), 1);
                copy_local_texture2d(o, m, copy, model, texture, copy.wrapping_add(0x6ec));
                let first = m.u32(copy.wrapping_add(0x6ec));
                m.set_u32(copy.wrapping_add(0x6f0), first);
                last = first;
            } else {
                let count = m.u16(copy.wrapping_add(0x6e8));
                m.set_u16(copy.wrapping_add(0x6e8), count.wrapping_add(1));
                copy_local_texture2d(o, m, copy, model, texture, last.wrapping_add(0x16c));
                last = m.u32(last.wrapping_add(0x16c));
            }
            texture = m.u32(texture.wrapping_add(0x16c));
        }
        let shared3d = m.u32(shared_mat.wrapping_add(0x6fc));
        if shared3d != 0 {
            let c = o.call_addr(m, SI_TXT3D_COPY, &[shared3d]);
            m.set_u32(copy.wrapping_add(0x6fc), c);
            let list = crate::dispose::graf_list(m);
            o.call_addr(m, crate::wave::SI_GRAFLIST_ADD_DATA, &[list, m.u32(copy.wrapping_add(0x6fc)), GRAF_T3D, 0]);
            let shader = m.u32(m.u32(shared_mat.wrapping_add(0x6fc)).wrapping_add(0x14c));
            o.call_addr(m, SI_COPY_T3D_SHADER, &[m.u32(copy.wrapping_add(0x6fc)), shader]);
            let c = m.u32(copy.wrapping_add(0x6fc));
            ed_model_set_l_texture3d(o, m, copy, c);
            let from = m.u32(shared_mat.wrapping_add(0x6fc));
            o.call_addr(m, SI_TEXTURE3D_LOAD_FCURVE_ALL, &[m.u32(copy.wrapping_add(0x6fc)), from]);
        }
    } else {
        chapter = m[SI_CID].t3d;
        relid = if global != 0 { 500 } else { 501 };
        ed_add_texture3d(o, m, 0, 1, t3d);
        ed_get_elem_from_texture3d(o, m, t3d, Ptr32::new(number));
    }
    m[SI_ADD_IN_DICTIONARY] = 0;
    let n = m.i32(number);
    if n >= 0 {
        let to = elem_at(m, ed, chapter, n);
        let from = elem_at(m, ed, chapter, index);
        let rel = o.call_addr(m, SI_ELEMDICT_GET_RELATION_FROM_IDS, &[ed, elem, relid, chapter as u32, index as u32, 0xffff_ffff]);
        if rel != 0 {
            o.call_addr(m, SI_ELEMDICT_DISCONNECT_AND_CONNECT_REL, &[ed, SI_RD.addr(), source as u32, from, to, relid]);
            m.set_u32(rel.wrapping_add(4), m.u32(number));
            let object = m.u32(elem.wrapping_add(4));
            if material != 0 {
                let array = m.u32(object.wrapping_add(0x20c));
                m.set_u32(array.wrapping_add((slot as u32).wrapping_mul(4)), copy);
            } else if global != 0 {
                m.set_u32(object.wrapping_add(0x224), t3d);
            } else {
                m.set_u32(object.wrapping_add(0x6fc), t3d);
            }
            m.set_u32(to.wrapping_add(0x5c), index as u32);
            let n = m.u32(number);
            o.call_addr(m, SI_ELEMDICT_COPY_ELEM_ANIM, &[ed, from, to, chapter as u32, index as u32, n]);
        }
    }
    m.release(4);
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_dict::globals::SI_ELEM_DICTS;

    /// Every call logged; `SI_elemdictGetElemFromData` stores 7 through the index pointer
    /// and answers 0x5000, `SI_elemdictFindElemByData` answers 3, the rest 0.
    #[derive(Default)]
    struct Log(Vec<(u32, Vec<u32>)>);
    impl ByAddr for Log {
        fn call_addr(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
            self.0.push((f, args.to_vec()));
            match f {
                SI_ELEMDICT_GET_ELEM_FROM_DATA => {
                    m.set_i32(args[3], 7);
                    0x5000
                }
                SI_ELEMDICT_FIND_ELEM_BY_DATA => 3,
                SI_ELEMDICT_GET_ELEM => 0x6000,
                _ => 0,
            }
        }
    }

    /// An image with the dictionary array allocated and made-up chapter numbers.
    fn image() -> (Mem, u32) {
        let mut m = Mem::new();
        si_mem::init_globals(&mut m, si_dict::globals::ALL);
        let dicts = m.malloc(0x70);
        m[SI_ELEM_DICTS] = Ptr32::new(dicts);
        let mut cid = m.read(SI_CID);
        (cid.mat, cid.model, cid.t2d, cid.t3d) = (7, 8, 26, 27);
        m.write(SI_CID, cid);
        (m, dicts)
    }

    #[test]
    fn a_node_is_related_to_its_material_whatever_the_indices() {
        let (mut m, dict) = image();
        let model = Ptr32::<Model>::new(m.malloc(core::mem::size_of::<Model>() as u32));
        m[model].elem_id = 2;
        let mut o = Log::default();
        ed_model_set_material(&mut o, &mut m, model, 0x4444);
        let calls: Vec<u32> = o.0.iter().map(|c| c.0).collect();
        assert_eq!(calls, [SI_ELEMDICT_GET_ELEM, SI_ELEMDICT_GET_ELEM_FROM_DATA, SI_ELEMDICT_ADD_RELATION, SI_SCHEMATIC_HIER_OPERATION]);
        assert_eq!(o.0[1].1[..3], [dict, 7, 0x4444]);
        assert_eq!(o.0[2].1, [dict, SI_RD.addr(), 2, 7, 300, 1, 0]);
        assert_eq!(o.0[3].1, [0, dict, 0x6000, 8, 0x14]);

        // a node without element: the relation is asked for with −1
        m[model].elem_id = -1;
        let mut o = Log::default();
        ed_model_set_g_texture3d(&mut o, &mut m, model, 0x4444);
        assert_eq!(o.0[0].0, SI_ELEMDICT_FIND_ELEM_BY_DATA);
        assert_eq!(o.0[2].1[..3], [dict, 27, 0x4444]);
        assert_eq!(o.0[3].1, [dict, SI_RD.addr(), 3, 7, 500, 1, 0]);
        assert_eq!(m[model].elem_id, 3);
    }

    #[test]
    fn a_relation_is_taken_away_in_the_texture_s_chapter() {
        let (mut m, dict) = image();
        for (relid, chap, relax) in [(300, 7, ed_model_relax_material::<Log> as fn(&mut Log, &mut Mem, u32, u32)), (400, 26, ed_model_relax_g_texture2d::<Log>), (500, 27, ed_model_relax_g_texture3d::<Log>)] {
            let mut o = Log::default();
            relax(&mut o, &mut m, 0x1111, 0x2222);
            assert_eq!(o.0[0], (SI_ELEMDICT_FIND_ELEM_BY_DATA, vec![dict, 8, 0x1111]));
            assert_eq!(o.0[1], (SI_ELEMDICT_FIND_ELEM_BY_DATA, vec![dict, chap, 0x2222]));
            assert_eq!(o.0[2], (SI_ELEMDICT_REMOVE_RELATION_PAIR, vec![dict, SI_RD.addr(), 3, 3, relid, 1, 0]));
        }
    }

    /// The calls of `SI_edAddMaterial`: no element for the material, a new name of 40
    /// characters, a name record from the heap, element number 5.
    struct Adding(Vec<(u32, Vec<u32>)>, u32);
    impl ByAddr for Adding {
        fn call_addr(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
            self.0.push((f, args.to_vec()));
            match f {
                SI_ELEMDICT_GET_NEW_NAME => self.1,
                SI_DB_LIST_NODE_ALLOCATE => m.calloc(1, 0x34),
                SI_DB_FORMAT_FULL_NAME => 0x7777,
                SI_ELEMDICT_ADD_ELEM => 5,
                _ => 0,
            }
        }
    }

    #[test]
    fn a_material_gets_a_truncated_new_name_and_its_element() {
        let (mut m, dict) = image();
        let material = m.calloc(1, 0xdcc);
        m.put_cstr(material, b"mat");
        let name = m.malloc(41);
        m.put_cstr(name, &[b'n'; 40]);
        m[SI_ADD_IN_DICTIONARY] = 0;
        let mut o = Adding(Vec::new(), name);
        assert_eq!(ed_add_material(&mut o, &mut m, 0x9999, 1, material), 1);
        assert!(o.0.is_empty());
        m[SI_ADD_IN_DICTIONARY] = 1;
        assert_eq!(ed_add_material(&mut o, &mut m, 0x9999, 1, material), 1);
        let calls: Vec<u32> = o.0.iter().map(|c| c.0).collect();
        assert_eq!(calls, [SI_ELEMDICT_GET_ELEM_FROM_DATA, SI_ELEMDICT_GET_NEW_NAME, SI_DB_LIST_NODE_ALLOCATE, SI_DB_FORMAT_FULL_NAME, SI_ELEMDICT_ADD_ELEM]);
        assert_eq!(o.0[1].1, [dict, 7, material]);
        assert_eq!(m.cstr(material), [b'n'; 0x1d]);
        let record: Ptr32<DbListNode> = Ptr32::new(o.0[3].1[0]);
        assert_eq!(m.cstr(m[record].name.0), [b'n'; 0x1d]);
        assert_eq!(m[record].fullname.0, 0x7777);
        assert_eq!(o.0[4].1, [dict, 7, record.0, material, 0x9999, 0, 0]);
        assert_eq!(m.u32(material + 0xda8), 5);

        // a material that has an element: 0, and nothing made
        let mut o = Log::default();
        assert_eq!(ed_add_material(&mut o, &mut m, 0x9999, 1, material), 0);
        assert_eq!(o.0.len(), 1);
    }

    /// `Log`, but `SI_elemdictFindRelatedElem` answers `self.1`.
    struct Related(Log, u32);
    impl ByAddr for Related {
        fn call_addr(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
            let answer = self.0.call_addr(m, f, args);
            if f == SI_ELEMDICT_FIND_RELATED_ELEM {
                assert_eq!(m.i32(args[7]), -1);
                return self.1;
            }
            answer
        }
    }

    #[test]
    fn a_local_texture_marks_the_material_s_node_or_the_material() {
        let (mut m, dict) = image();
        for (relid, node) in [(401, 0x6100), (401, 0), (501, 0x6100), (501, 0)] {
            let mut o = Related(Log::default(), node);
            if relid == 401 {
                ed_model_set_l_texture2d(&mut o, &mut m, 0x1111, 0x2222);
            } else {
                ed_model_set_l_texture3d(&mut o, &mut m, 0x1111, 0x2222);
            }
            let calls = &o.0 .0;
            assert_eq!(calls[0].1[..3], [dict, 7, 0x1111]);
            assert_eq!(calls[1].1[..3], [dict, if relid == 401 { 26 } else { 27 }, 0x2222]);
            assert_eq!(calls[2], (SI_ELEMDICT_ADD_RELATION, vec![dict, SI_RD.addr(), 7, 7, relid, 1, 0]));
            assert_eq!(calls[3].1[..6], [dict, 0x5000, 0, 0, 300, 8]);
            let want = if node != 0 { vec![0, dict, node, 8, 0x14] } else { vec![0, dict, 0x5000, 7, 0x14] };
            assert_eq!(calls[4], (SI_SCHEMATIC_HIER_OPERATION, want));
        }
    }

    /// Another double, with the answers of some routines replaced.
    struct Answering<B>(B, Vec<(u32, u32)>);
    impl<B: ByAddr> ByAddr for Answering<B> {
        fn call_addr(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
            let answer = self.0.call_addr(m, f, args);
            self.1.iter().find(|x| x.0 == f).map_or(answer, |x| x.1)
        }
    }

    /// SCENES 3 and LIGHTS 6 in `SI_cid`, SCENES 12 in `SI_dbcid`.
    fn scene_and_lights(m: &mut Mem) {
        let mut cid = m.read(SI_CID);
        (cid.dsc, cid.lig) = (3, 6);
        m.write(SI_CID, cid);
        let mut db = m.read(SI_DBCID);
        db.dsc = 12;
        m.write(SI_DBCID, db);
    }

    #[test]
    fn a_shader_is_related_to_the_scene_a_light_or_its_owner() {
        let (mut m, dict) = image();
        scene_and_lights(&mut m);
        // the scene's element, index 0
        let mut o = Log::default();
        ed_set_udf_relation(&mut o, &mut m, 0, 12, 0x7000, 30);
        assert_eq!(o.0[0], (SI_ELEMDICT_GET_ELEM, vec![dict, 3, 0]));
        assert_eq!(o.0[1], (SI_ELEMDICT_FIND_ELEM_BY_DATA, vec![dict, 30, 0x7000]));
        assert_eq!(o.0[2], (SI_ELEMDICT_GET_RELATION_FROM_IDS, vec![dict, 0x6000, 550, 30, 3, 0xffff_ffff]));
        assert_eq!(o.0[3], (SI_ELEMDICT_ADD_UN_COMPLETE_RELATION, vec![dict, SI_RD.addr(), 12, 30, 0, 3, 550, 1, 0, 0, 1]));
        assert_eq!(o.0.len(), 4);
        // a light's: the look-up of a light and its interest (index 7)
        let mut o = Log::default();
        ed_set_udf_relation(&mut o, &mut m, 0x4444, 6, 0x7000, 30);
        let calls: Vec<u32> = o.0.iter().map(|c| c.0).collect();
        assert_eq!(calls[..2], [SI_ELEMDICT_GET_ELEM_FROM_DATA, SI_ELEMDICT_FIND_RELATED_ELEM]);
        assert_eq!(o.0[0].1[..3], [dict, 6, 0x4444]);
        assert_eq!(o.0[3].1[1], 0x5000);
        assert_eq!(o.0[4].1[2..6], [6, 30, 7, 3]);
        // a null owner outside the database's SCENES goes the general way; a relation that
        // is there already is not added
        let mut o = Answering(Log::default(), vec![(SI_ELEMDICT_GET_RELATION_FROM_IDS, 0x8000)]);
        ed_set_udf_relation(&mut o, &mut m, 0, 3, 0x7000, 30);
        let calls: Vec<u32> = o.0 .0.iter().map(|c| c.0).collect();
        assert_eq!(calls, [SI_ELEMDICT_FIND_ELEM_BY_DATA, SI_ELEMDICT_GET_ELEM, SI_ELEMDICT_FIND_ELEM_BY_DATA, SI_ELEMDICT_GET_RELATION_FROM_IDS]);
        assert_eq!((o.0 .0[0].1.clone(), o.0 .0[1].1.clone()), (vec![dict, 3, 0], vec![dict, 3, 3]));
        // the scene's element on its own
        let index = m.malloc(4);
        m.set_i32(index, -1);
        let mut o = Log::default();
        assert_eq!(ed_get_elem_from_scene(&mut o, &mut m, Ptr32::new(index)).0, 0x6000);
        assert_eq!(m.i32(index), 0);
    }

    #[test]
    fn a_shader_gets_a_new_name_an_element_and_its_relation() {
        let (mut m, dict) = image();
        scene_and_lights(&mut m);
        let udf = m.calloc(1, 0x3c);
        let old = m.malloc(4);
        m.put_cstr(old, b"old");
        m.set_u32(udf, old);
        m.set_u32(udf + 4, 30);
        let name = m.malloc(8);
        m.put_cstr(name, b"phong1");
        let answers = vec![(SI_ELEMDICT_FIND_ELEM_BY_DATA, 0xffff_ffff), (SI_ELEMDICT_GET_ELEM, 0x6000)];
        let mut o = Answering(Adding(Vec::new(), name), answers.clone());
        // nothing for a null owner outside the database's SCENES, a null shader, or while
        // nothing is added to the dictionary
        ed_add_udf(&mut o, &mut m, 0, 3, udf);
        ed_add_udf(&mut o, &mut m, 0x4444, 7, 0);
        m[SI_ADD_IN_DICTIONARY] = 0;
        ed_add_udf(&mut o, &mut m, 0x4444, 7, udf);
        assert!(o.0 .0.is_empty());
        m[SI_ADD_IN_DICTIONARY] = 1;
        ed_add_udf(&mut o, &mut m, 0x4444, 7, udf);
        let log = &o.0 .0;
        let calls: Vec<u32> = log.iter().map(|c| c.0).collect();
        #[rustfmt::skip]
        assert_eq!(calls, [
            SI_ELEMDICT_FIND_ELEM_BY_DATA, SI_ELEMDICT_GET_NEW_NAME, SI_DB_LIST_NODE_ALLOCATE, SI_DB_FORMAT_FULL_NAME,
            SI_ELEMDICT_ADD_ELEM, SI_ELEMDICT_FIND_ELEM_BY_DATA, SI_ELEMDICT_GET_ELEM, SI_ELEMDICT_FIND_ELEM_BY_DATA,
            SI_ELEMDICT_GET_RELATION_FROM_IDS, SI_ELEMDICT_ADD_UN_COMPLETE_RELATION, SI_UDF_ATTACH_ARRAY_ELEMENTS,
            SI_ELEMDICT_GET_ELEM, SI_ELEMDICT_SET_DIRTY,
        ]);
        assert_eq!(log[0].1, [dict, 30, udf]);
        assert_eq!(log[1].1, [dict, 30, old]);
        // the new name is a copy in the shader, a second copy in the name record
        let record: Ptr32<DbListNode> = Ptr32::new(log[3].1[0]);
        let mine = m.u32(udf);
        assert!(mine != name && m[record].name.0 != mine);
        assert_eq!((m.cstr(mine), m.cstr(m[record].name.0)), (b"phong1".to_vec(), b"phong1".to_vec()));
        assert_eq!(m[record].fullname.0, 0x7777);
        assert_eq!(log[4].1, [dict, 30, record.0, udf, 0, 0, 0]);
        assert_eq!(m.u32(udf + 8), 5);
        assert_eq!(log[9].1, [dict, SI_RD.addr(), 7, 30, 0xffff_ffff, 0xffff_ffff, 550, 1, 0, 0, 1]);
        assert_eq!((log[10].1.clone(), log[11].1.clone(), log[12].1.clone()), (vec![udf], vec![dict, 30, 5], vec![0x6000, 1]));

        // a shader that has its element keeps its name; its index is the one found
        let mut o = Answering(Adding(Vec::new(), name), vec![(SI_ELEMDICT_FIND_ELEM_BY_DATA, 2), (SI_ELEMDICT_GET_ELEM, 0x6000)]);
        m.set_u16(udf + 0xe, 0xffff);
        ed_add_udf(&mut o, &mut m, 0x4444, 7, udf);
        let log = &o.0 .0;
        assert_eq!(log[0].0, SI_ELEMDICT_FIND_ELEM_BY_DATA);
        assert_eq!(log[1].0, SI_ELEMDICT_FIND_ELEM_BY_DATA);
        assert_eq!(m.u32(udf), mine);
        let n = log.len();
        assert_eq!((log[n - 2].1.clone(), log[n - 1].1.clone()), (vec![dict, 30, 2], vec![0x6000, 0xffff_ffff]));
    }

    #[test]
    fn fifteen_material_curves_are_flagged() {
        let (mut m, _) = image();
        let mut o = Log::default();
        attdict_flag_all_material_fcurves(&mut o, &mut m);
        let ids: Vec<u32> = o.0.iter().inspect(|c| assert_eq!((c.0, c.1[0], c.1[2]), (SI_ATTDICT_SET_FCURVE_FLAG, si_dict::globals::SI_ATD.addr(), 1))).map(|c| c.1[1]).collect();
        assert_eq!(ids, [0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x39, 0x3b, 0x101, 0x102]);
    }

    /// The calls of the textures' elements and the disconnection: the first look-up of an
    /// object finds nothing, the later ones find element 1; the copies are cleared blocks
    /// of the heap (a 3D texture's named, a 2D texture's method 4); name records from the
    /// heap, element number 1, the relation `rel`.
    struct Disc {
        calls: Vec<(u32, Vec<u32>)>,
        looked: u32,
        name: u32,
        rel: u32,
    }
    impl ByAddr for Disc {
        fn call_addr(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
            self.calls.push((f, args.to_vec()));
            match f {
                SI_ELEMDICT_GET_ELEM_FROM_DATA => {
                    self.looked += 1;
                    if self.looked == 1 {
                        return 0;
                    }
                    m.set_i32(args[3], 1);
                    0x5000
                }
                SI_ELEMDICT_GET_NEW_NAME => self.name,
                SI_DB_LIST_NODE_ALLOCATE => m.calloc(1, 0x34),
                SI_DB_FORMAT_FULL_NAME => 0x7777,
                SI_ELEMDICT_ADD_ELEM => 1,
                SI_ELEMDICT_GET_RELATION_FROM_IDS => self.rel,
                SI_MATERIAL_COPY => m.calloc(1, 0xdcc),
                SI_TXT3D_COPY => {
                    let t = m.calloc(1, 0x158);
                    let name = m.strdup(self.name);
                    m.set_u32(t, name);
                    t
                }
                SI_TXT2D_COPY => {
                    let t = m.calloc(1, 0x1d8);
                    m.set_u16(t + 0x2a, 4);
                    t
                }
                _ => 0,
            }
        }
    }

    /// `image()` with lists in the current dictionary and a dictionary `ed` whose chapters
    /// MATERIALS and TEXTURES3D have two elements each; answers (image, current dictionary,
    /// ed, its MATERIALS elements, its TEXTURES3D elements, a `Disc`).
    fn disc_image() -> (Mem, u32, u32, u32, u32, Disc) {
        let (mut m, dict) = image();
        let lists = m.calloc(28, 0x40);
        m.set_u32(dict + 4, lists);
        let ed = m.calloc(1, 0x70);
        let lists = m.calloc(28, 0x40);
        m.set_u32(ed + 4, lists);
        let (mats, t3ds) = (m.calloc(2, 0x84), m.calloc(2, 0x84));
        m.set_u32(lists + 7 * 0x40 + 8, mats);
        m.set_u32(lists + 27 * 0x40 + 8, t3ds);
        let name = m.malloc(5);
        m.put_cstr(name, b"new1");
        m[SI_ADD_IN_DICTIONARY] = 1;
        let o = Disc { calls: Vec::new(), looked: 0, name, rel: m.calloc(1, 0x20) };
        (m, dict, ed, mats, t3ds, o)
    }

    fn called(o: &Disc, f: u32) -> Vec<Vec<u32>> {
        o.calls.iter().filter(|c| c.0 == f).map(|c| c.1.clone()).collect()
    }

    #[test]
    fn a_3d_texture_gets_a_new_name_in_place_of_its_own() {
        let (mut m, dict, _, _, _, mut o) = disc_image();
        let texture = m.calloc(1, 0x158);
        let old = m.malloc(4);
        m.put_cstr(old, b"t3d");
        m.set_u32(texture, old);
        assert_eq!(ed_add_texture3d(&mut o, &mut m, 0x9999, 1, texture), 1);
        assert_eq!(called(&o, SI_ELEMDICT_GET_NEW_NAME), [vec![dict, 27, old]]);
        assert!(m.freed().contains(&old));
        assert_eq!(m.cstr(m.u32(texture)), b"new1");
        let record = called(&o, SI_DB_FORMAT_FULL_NAME)[0][0];
        assert_eq!(called(&o, SI_ELEMDICT_ADD_ELEM), [vec![dict, 27, record, texture, 0x9999, 0, 0]]);
        // one that has an element: 0
        assert_eq!(ed_add_texture3d(&mut o, &mut m, 0x9999, 1, texture), 0);
        m[SI_ADD_IN_DICTIONARY] = 0;
        assert_eq!(ed_add_texture3d(&mut o, &mut m, 0x9999, 1, texture), 1);
    }

    #[test]
    fn a_2d_texture_without_a_name_is_named_t2d_and_its_picture_looked_for() {
        let (mut m, dict, _, _, _, mut o) = disc_image();
        let texture = m.calloc(1, 0x1d8);
        assert_eq!(ed_add_texture2d(&mut o, &mut m, 0, 0, texture), 1);
        assert_eq!(called(&o, SI_ELEMDICT_GET_NEW_NAME), [vec![dict, 26, crate::globals::S_T2D_ED]]);
        assert_eq!(m.cstr(m.u32(texture + 0x18)), b"new1");
        assert_eq!(m.u32(texture + 0x190), 1);
        let elems = m.u32(m.u32(dict + 4) + 26 * 0x40 + 8);
        assert_eq!(called(&o, SI_TEXTURE2D_VERIFY_PICTURE_PATH), [vec![SI_DBDIR, 0, dict, elems + 0x84, 0, 0]]);
    }

    #[test]
    fn a_shared_material_is_copied_with_its_2d_textures_and_the_relation_moved() {
        let (mut m, _, ed, mats, _, mut o) = disc_image();
        let shared = m.calloc(1, 0xdcc);
        let (a, b) = (m.calloc(1, 0x1d8), m.calloc(1, 0x1d8));
        m.set_u32(shared + 0x6ec, a);
        m.set_u32(a + 0x16c, b);
        m.set_u32(a + 0x198, 0x8888);
        m.set_u32(mats + 4, shared);
        let (elem, model, array) = (m.calloc(1, 0x84), m.calloc(1, 0x360), m.calloc(4, 4));
        m.set_u32(elem + 4, model);
        m.set_u32(model + 0x30, 4);
        m.set_u32(model + 0x20c, array);
        ed_disconnect_shared_material_or3d_txt(&mut o, &mut m, ed, elem, 2, mats, 0, 1, 3, 0);
        let copy = m.u32(array + 12);
        assert_eq!(called(&o, SI_MATERIAL_COPY), [vec![shared]]);
        let first = m.u32(copy + 0x6ec);
        let second = m.u32(first + 0x16c);
        assert_eq!((m.u16(copy + 0x6e8), m.u32(copy + 0x6f0), m.u16(first + 0x2a), m.u16(second + 0x2a)), (2, first, 6, 6));
        assert_eq!(called(&o, SI_TXT2D_COPY), [vec![a], vec![b]]);
        assert_eq!(called(&o, SI_COPY_T2D_SHADER), [vec![first, 0x8888], vec![second, 0]]);
        assert_eq!(called(&o, SI_TEXTURE2D_LOAD_FCURVE_ALL), [vec![first, a], vec![second, b]]);
        assert!(called(&o, SI_TXT3D_COPY).is_empty());
        assert_eq!(called(&o, SI_ELEMDICT_GET_RELATION_FROM_IDS), [vec![ed, elem, 300, 7, 0, 0xffff_ffff]]);
        assert_eq!(called(&o, SI_ELEMDICT_DISCONNECT_AND_CONNECT_REL), [vec![ed, SI_RD.addr(), 2, mats, mats + 0x84, 300]]);
        assert_eq!((m.u32(o.rel + 4), m.u32(mats + 0x84 + 0x5c), m[SI_ADD_IN_DICTIONARY]), (1, 0, 0));
        assert_eq!(called(&o, SI_ELEMDICT_COPY_ELEM_ANIM), [vec![ed, mats, mats + 0x84, 7, 0, 1]]);
    }

    #[test]
    fn a_shared_global_3d_texture_is_copied_and_the_node_takes_it() {
        let (mut m, _, ed, _, t3ds, mut o) = disc_image();
        m.set_u32(t3ds + 0x84 + 4, 0x4321);
        let (elem, model) = (m.calloc(1, 0x84), m.calloc(1, 0x360));
        m.set_u32(elem + 4, model);
        ed_disconnect_shared_material_or3d_txt(&mut o, &mut m, ed, elem, 0, t3ds + 0x84, 0, 0, 0, 1);
        let copy = m.u32(model + 0x224);
        assert_eq!(called(&o, SI_TXT3D_COPY), [vec![0x4321]]);
        assert_eq!(m.cstr(m.u32(copy)), b"new1");
        assert_eq!(called(&o, SI_ELEMDICT_DISCONNECT_AND_CONNECT_REL), [vec![ed, SI_RD.addr(), 0, t3ds, t3ds + 0x84, 500]]);
        // no relation: nothing moved, nothing stored
        let (mut m, _, ed, _, t3ds, mut o) = disc_image();
        o.rel = 0;
        let (elem, material) = (m.calloc(1, 0x84), m.calloc(1, 0xdcc));
        m.set_u32(elem + 4, material);
        ed_disconnect_shared_material_or3d_txt(&mut o, &mut m, ed, elem, 0, t3ds, 0, 0, 0, 0);
        assert_eq!(called(&o, SI_ELEMDICT_GET_RELATION_FROM_IDS), [vec![ed, elem, 501, 27, 0, 0xffff_ffff]]);
        assert!(called(&o, SI_ELEMDICT_DISCONNECT_AND_CONNECT_REL).is_empty());
        assert_eq!(m.u32(material + 0x6fc), 0);
    }
}
