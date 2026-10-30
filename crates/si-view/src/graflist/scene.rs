// SPDX-License-Identifier: MIT
//! Emptying a scene: the objects of the dictionary's chapters, the elements, the graph
//! list (which gets the camera back), the default description.

use super::edit::{graflist_add_data, graflist_delete_grafnode};
use super::events::scene_list;
use super::list::{is_model, schematic_no_refresh};
use crate::globals::*;
use crate::outside::{self, Outside};
use si_mem::{Mem, Ptr32};
use si_types::dict::{Elem, ElemDict, ElemList};
use si_types::scene::GrafList;

/// Offsets in the camera record `graflistDelete` reads and clears. Read from its own
/// accesses; the record belongs to the camera's package.
mod camera {
    /// A list of user data, disposed of when not null.
    pub const USER_DATA: u32 = 0x138;
    /// Two words cleared when the list is emptied.
    pub const CLEARED_A0: u32 = 0xa0;
    pub const CLEARED_A4: u32 = 0xa4;
}

/// Empties a list and puts the scene's camera back into it
/// [soft 0x0832c36c graflistDelete]. `schemNoRefresh` is 1 meanwhile and what it was
/// afterwards; the deletions are one history cluster (`SI_historyOpenCluster
/// (0x080c7270)`, `SI_historyCloseCluster (0x080c7298)`).
///
/// **Model nodes first**: the chain is walked from its head, a node of kind 2 or 3 is
/// deleted (`SI_graflistDeleteGrafnode`) and the walk starts again at the head. Then the
/// head is deleted until there is none. The list's word at +0x10, its head and its two
/// active nodes are cleared. `SI_fastPlayDeleteAll (0x0832a108)` (null),
/// `SI_globalMapListDelete (0x082b2164)`; the camera's user data (+0x138, when the
/// process record has a camera and the word is set) goes to `SI_userDataDispose
/// (0x084619e4)`. Then the camera enters the list again (`SI_graflistAddData`, kind 4,
/// not selected): **both its nodes get display type 0**, and the camera's words at +0xa0
/// and +0xa4 are cleared. Neither the node nor the camera is tested for null there.
pub fn graflist_delete(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>) {
    let saved = m[SCHEM_NO_REFRESH];
    schematic_no_refresh(m, 1);
    o.call(m, &outside::SI_HISTORY_OPEN_CLUSTER, &[]);
    let mut node = m[list].first;
    while !node.is_null() && m.fault().is_none() {
        if is_model(m[node].kind) {
            graflist_delete_grafnode(m, o, list, node);
            node = m[list].first;
        } else {
            node = m[node].next;
        }
    }
    while !m[list].first.is_null() && m.fault().is_none() {
        let head = m[list].first;
        graflist_delete_grafnode(m, o, list, head);
    }
    m[list].unk_10[0] = 0;
    m[list].first = Ptr32::NULL;
    m[list].active = Ptr32::NULL;
    m[list].active2 = Ptr32::NULL;
    o.call(m, &outside::SI_HISTORY_CLOSE_CLUSTER, &[]);
    schematic_no_refresh(m, saved);
    o.call(m, &outside::SI_FAST_PLAY_DELETE_ALL, &[0]);
    o.call(m, &outside::SI_GLOBAL_MAP_LIST_DELETE, &[]);
    let proc = m[SI_PROC_DATA];
    let cam = m[proc].camera.addr();
    if cam != 0 && m.u32(cam.wrapping_add(camera::USER_DATA)) != 0 {
        o.call(m, &outside::SI_USER_DATA_DISPOSE, &[cam.wrapping_add(camera::USER_DATA)]);
    }
    let proc = m[SI_PROC_DATA];
    let cam = m[proc].camera;
    let node = graflist_add_data(m, o, list, cam, 4, 0);
    let behind = m[node].next;
    m[node].wire_type = 0;
    let proc = m[SI_PROC_DATA];
    m[behind].wire_type = 0;
    let cam = m[proc].camera.addr();
    m.set_u32(cam.wrapping_add(camera::CLEARED_A0), 0);
    let cam = m[proc].camera.addr();
    m.set_u32(cam.wrapping_add(camera::CLEARED_A4), 0);
}

/// The chapters whose elements own a user function record (`SI_udfDispose`): the shader
/// chapters but the shadow shaders', in the order the original compares them.
const SHADERS: [usize; 9] = [
    cid::MATERIAL_SHADERS,
    cid::TEXTURE2D_SHADERS,
    cid::TEXTURE3D_SHADERS,
    cid::VOLUME_SHADERS,
    cid::CAMERA_SHADERS,
    cid::OUTPUT_SHADERS,
    cid::MATERIAL_PHOTON_SHADERS,
    cid::LIGHT_PHOTON_SHADERS,
    cid::LIGHT_SHADERS,
];

/// Disposes of the objects of one chapter's elements
/// [soft 0x0832c4c0 removeChapterData]: `chapter` is the chapter, `chap` its index. Every
/// element in use (status 0) that has an object, by the chapter `SI_cid` says `chap` is:
///
/// - ANIMATION: an element whose level (+0x54) is above 0 has its object removed through
///   the element's own object word (`SI_fcurveRemove (0x081ff0a0)` with that word's
///   address; **the word is not cleared here**); one at level 0 or below goes to
///   `SI_hieranimDispose (0x08138b5c)` and its object word is cleared;
/// - EXPRESSIONS: `SI_expressDataDispose (0x0824e5b0)`, word cleared;
/// - the shader chapters (material, 2D and 3D texture, volume, camera, output, the two
///   photon chapters, light; not shadow): `SI_udfDispose (0x0854c3c0)`, word cleared;
/// - STRUCT_KEYS: `SI_hieranimDispose`, word cleared;
/// - any other chapter: nothing.
///
/// An element at address 0 is skipped (the first of a chapter without an array).
pub fn remove_chapter_data(m: &mut Mem, o: &mut dyn Outside, chapter: Ptr32<ElemList>, chap: i32) {
    let mut index: i32 = 0;
    let mut offset: u32 = 0;
    if index >= m[chapter].nb_elems {
        return;
    }
    loop {
        let elem: Ptr32<Elem> = Ptr32::new(m[chapter].elems.addr().wrapping_add(offset));
        if !elem.is_null() && m[elem].status == 0 {
            let data = m[elem].data.addr();
            if data != 0 {
                let ids = m[SI_CID];
                let cleared = if chap == ids[cid::ANIMATION] {
                    if m[elem].hier_level <= 0 {
                        o.call(m, &outside::SI_HIERANIM_DISPOSE, &[data]);
                        true
                    } else {
                        o.call(m, &outside::SI_FCURVE_REMOVE, &[elem.addr().wrapping_add(4)]);
                        false
                    }
                } else if chap == ids[cid::EXPRESSIONS] {
                    o.call(m, &outside::SI_EXPRESS_DATA_DISPOSE, &[data]);
                    true
                } else if SHADERS.iter().any(|&k| chap == ids[k]) {
                    o.call(m, &outside::SI_UDF_DISPOSE, &[data]);
                    true
                } else if chap == ids[cid::STRUCT_KEYS] {
                    o.call(m, &outside::SI_HIERANIM_DISPOSE, &[data]);
                    true
                } else {
                    false
                };
                if cleared {
                    m[elem].data = Ptr32::NULL;
                }
            }
        }
        offset = offset.wrapping_add(0x84);
        index += 1;
        if index >= m[chapter].nb_elems || m.fault().is_some() {
            break;
        }
    }
}

/// The chapters emptying a scene leaves to the graph list: their elements go when their
/// nodes are deleted.
const BY_NODE: [usize; 6] = [cid::MODELS, cid::MATERIALS, cid::TEXTURES2D, cid::TEXTURES3D, cid::WAVES, cid::LIGHTS];

fn left_to_the_list(m: &Mem, chap: i32) -> bool {
    let ids = m[SI_CID];
    BY_NODE.iter().any(|&k| chap == ids[k])
}

/// The current dictionary's record.
fn dict(m: &Mem) -> Ptr32<ElemDict> {
    Ptr32::new(current_dict(m))
}

/// Empties the scene [soft 0x0832c5d0 SI_sceneDeleteAllNotInHistory], on the current
/// dictionary:
///
/// 1. Every chapter but those of the models, materials, 2D and 3D textures, waves and
///    lights: for the chapters of the animation, the structure keys, the expressions and
///    the shaders (not shadow) `removeChapterData` disposes of the objects; then every
///    element in use is deleted with `SI_elemdictSetDeleteElem (0x08135ad4)` (dictionary,
///    chapter, element, index, −1, 0, 1).
/// 2. The same chapters again: `SI_elemlistDispose (0x08135fd4)` (chapter, 0).
/// 3. Of `SI_defaultControl`: the blocks at +0x3ec and +0x3f0 are freed and the words
///    cleared; the words at +0x3e4, +0x3e8 and +0x44c are cleared.
/// 4. `SI_udfDisposeAll (0x0854c588)`, `SI_dialogChnReset (0x085710a8)` (0),
///    `SI_particleSetupReset (0x084c0438)`.
/// 5. `graflistDelete` on the scene's list: the models, lights, materials, textures and
///    waves go with their nodes, and the camera comes back.
/// 6. `SI_elemdictCreateDefaultDsc (0x081369cc)` (dictionary), `SI_sprdAllContentDispose
///    (0x085aace8)`.
///
/// The number of chapters is read again after every chapter.
pub fn scene_delete_all_not_in_history(m: &mut Mem, o: &mut dyn Outside) {
    let mut chap: i32 = 0;
    while chap < m[dict(m)].nb_lists {
        if !left_to_the_list(m, chap) {
            let chapter: Ptr32<ElemList> = Ptr32::new(m[dict(m)].lists.addr().wrapping_add((chap as u32) << 6));
            let ids = m[SI_CID];
            if chap == ids[cid::ANIMATION] || chap == ids[cid::STRUCT_KEYS] || chap == ids[cid::EXPRESSIONS] || SHADERS.iter().any(|&k| chap == ids[k]) {
                remove_chapter_data(m, o, chapter, chap);
            }
            let mut index: i32 = 0;
            let mut offset: u32 = 0;
            while index < m[chapter].nb_elems {
                let elem: Ptr32<Elem> = Ptr32::new(m[chapter].elems.addr().wrapping_add(offset));
                if !elem.is_null() && m[elem].status == 0 {
                    let ed = current_dict(m);
                    o.call(m, &outside::SI_ELEMDICT_SET_DELETE_ELEM, &[ed, chap as u32, elem.addr(), index as u32, 0xffff_ffff, 0, 1]);
                }
                offset = offset.wrapping_add(0x84);
                index += 1;
                if m.fault().is_some() {
                    return;
                }
            }
        }
        chap += 1;
    }
    let mut chap: i32 = 0;
    while chap < m[dict(m)].nb_lists {
        if !left_to_the_list(m, chap) {
            let chapter = m[dict(m)].lists.addr().wrapping_add((chap as u32) << 6);
            o.call(m, &outside::SI_ELEMLIST_DISPOSE, &[chapter, 0]);
        }
        chap += 1;
        if m.fault().is_some() {
            return;
        }
    }
    let control = SI_DEFAULT_CONTROL.addr();
    for at in [DEFAULT_CONTROL_3EC, DEFAULT_CONTROL_3F0] {
        let block = m.u32(control + at);
        if block != 0 {
            m.free(block);
            m.set_u32(control + at, 0);
        }
    }
    m.set_u32(control + DEFAULT_CONTROL_3E4, 0);
    m.set_u32(control + DEFAULT_CONTROL_3E8, 0);
    m.set_u32(control + DEFAULT_CONTROL_44C, 0);
    o.call(m, &outside::SI_UDF_DISPOSE_ALL, &[]);
    o.call(m, &outside::SI_DIALOG_CHN_RESET, &[0]);
    o.call(m, &outside::SI_PARTICLE_SETUP_RESET, &[]);
    let list = scene_list(m);
    graflist_delete(m, o, list);
    let ed = current_dict(m);
    o.call(m, &outside::SI_ELEMDICT_CREATE_DEFAULT_DSC, &[ed]);
    o.call(m, &outside::SI_SPRD_ALL_CONTENT_DISPOSE, &[]);
}
