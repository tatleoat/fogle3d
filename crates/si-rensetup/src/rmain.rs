// SPDX-License-Identifier: MIT
//! `rens_main.c`: the render dictionary made and deleted, and what is set up around it.
//!
//! A render dictionary is a second element dictionary with the scene's 36 chapters. Its
//! elements are copies of the scene's, slot for slot, that point at **the same objects**;
//! of the relations only those marked `RENDERING` in `Relations.rsrc` are copied. On that
//! copy the program then does what a renderer wants and the scene must not see: instances
//! become real hierarchies, a model without material gets one from its father or the
//! default one, an editor's preview material or texture is put in place.

use crate::edlocal::{copy_elements, copy_render_relations, get_elem_from_data, NewName};
use crate::instance::{expand_instances, instance_element_cleanup, InstState};
use crate::outside::Outside;
use crate::propagate::{propagate_material, render_add_default_material};
use crate::rdict;
use crate::update::render_update_instance_hidden_and_select;
use crate::util::set_scene_transmit_flags;
use crate::world::{data_of, lig, t2d, Env, Image, Obj, Preview};
use si_scene::dict::{Dict, ElemId};
use si_types::dict::{ChapIds, ELEM_VALID};
use si_types::{Ptr32, Zero};

/// `SI_renderExecuteParms` (0x674 bytes): what the mental ray front end carries through a
/// render. Offsets of the fields this file sets, by name.
pub mod parms {
    pub const SIZE: usize = 0x674;
    /// The render dictionary (a block of 0x70 bytes).
    pub const REN_DICT: usize = 0x000;
    /// The scene's dictionary.
    pub const SCENE_DICT: usize = 0x004;
    /// A copy of `SI_defaultControl`, 0x600 bytes.
    pub const CONTROL: usize = 0x00c;
    pub const CONTROL_SIZE: usize = 0x600;
    /// 16 bits, 1.
    pub const W60C: usize = 0x60c;
    /// 16 bits: the caller's "live" switch: no questions, no pauses.
    pub const LIVE: usize = 0x60e;
    /// An array of camera pointers and, at +0x614, its length (1).
    pub const CAMERAS: usize = 0x610;
    pub const NB_CAMERAS: usize = 0x614;
    /// The scene's full name, a copy.
    pub const SCENE_NAME: usize = 0x61c;
}

/// Counts of one [`create_render_dictionary`], for the checks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub elements_copied: usize,
    pub relations_copied: usize,
    pub instances_expanded: usize,
    pub instance_elements_added: usize,
    pub default_material_given: usize,
}

/// What a render keeps between making and deleting its dictionary.
#[derive(Clone, Copy, Debug)]
pub struct RenState {
    /// `si_renChaps` (0x0879e640): the chapter indices of the render dictionary.
    pub ren_chaps: ChapIds,
    pub inst: InstState,
    pub counts: Counts,
}

impl Default for RenState {
    fn default() -> Self {
        RenState { ren_chaps: ChapIds::zero(), inst: InstState::default(), counts: Counts::default() }
    }
}

/// [soft 0x082c55e0 SI_initializePreviewStruct]: the 0x18 bytes are cleared.
pub fn initialize_preview_struct(prev: &mut Preview) {
    *prev = Preview::default();
}

/// Makes the render dictionary of a scene [soft 0x082c5600 SI_createRenderDictionary].
/// `ren` is a block the caller allocated; whatever it held is overwritten.
///
/// 1. The scene's elements take the state of the scene graph (`SI_edGetElementsGrafNodeInfo`).
/// 2. `ren` is initialised and given the 36 chapters (`si_renChaps` gets their indices).
/// 3. Every slot is copied ([`copy_elements`]) and the render relations with them
///    ([`copy_render_relations`]).
/// 4. With `expand`, instances become hierarchies ([`expand_instances`]).
/// 5. `SI_edGetElementsGrafNodeInfo` on `ren` (its elements have no nodes, so nothing).
/// 6. The native renderer's root model pointer (0x087907e8) is cleared.
/// 7. The editors' preview is put in place ([`add_preview_relations`], from `SI_prevStruct`).
/// 8. With `default_material`, the default material is made and materials and textures
///    are handed down the hierarchies ([`propagate_material`]).
/// 9. Hidden and select states are handed down expanded instances.
/// 10. The transmit flags are worked out ([`set_scene_transmit_flags`]).
///
/// The answer is the default material's element (null without step 8), which the caller
/// hands back to [`delete_render_dictionary`].
pub fn create_render_dictionary(
    scene: &mut Dict, ren: &mut Dict, env: &mut Env, out: &mut dyn Outside, st: &mut RenState, expand: bool, default_material: bool,
) -> Option<ElemId> {
    out.ed_get_elements_graf_node_info(scene, false);
    *ren = Dict::new();
    let scene_cid = env.p.cid;
    ren.ed.load_chapters(&mut st.ren_chaps, 0, Some(&scene_cid));
    copy_elements(scene, ren, &env.p);
    st.counts.elements_copied = scene.ed.lists.iter().map(|l| l.elems.len()).sum();
    st.counts.relations_copied = copy_render_relations(scene, ren, &mut env.p);
    if expand {
        st.inst.expanded = 0;
        st.inst.added = 0;
        expand_instances(scene, ren, env, out, &mut st.inst);
        st.counts.instances_expanded = st.inst.expanded;
        st.counts.instance_elements_added = st.inst.added;
    }
    out.ed_get_elements_graf_node_info(ren, true);
    env.g.database_root = 0;
    let prev = env.g.prev;
    add_preview_relations(ren, env, out, &prev);
    let mut default_elem = None;
    if default_material {
        let (e, index) = render_add_default_material(ren, env, &mut st.inst.names, out);
        default_elem = e;
        st.counts.default_material_given = propagate_material(ren, env, out, index);
    }
    render_update_instance_hidden_and_select(scene, ren, env);
    set_scene_transmit_flags(ren, scene, env, out);
    default_elem
}

/// Deletes a render dictionary [soft 0x082c56e0 SI_deleteRenderDictionary]: with `expand`
/// what the instance expansion allocated is freed ([`instance_element_cleanup`]); the
/// picture pointer of every 2D texture is cleared; the default material, the object of
/// `default_elem`, is freed; the chapter names are freed and the dictionary disposed of.
/// Everything else its elements pointed at is the scene's and stays.
pub fn delete_render_dictionary(scene: &Dict, ren: &mut Dict, env: &mut Env, default_elem: Option<ElemId>, expand: bool) {
    if expand {
        instance_element_cleanup(scene, ren, env);
    }
    reset_texture_elem_maps(ren, env);
    if let Some(e) = default_elem {
        let data = data_of(ren, e);
        if data != 0 {
            env.heap.free(data);
            ren.slot_mut(e).unwrap().rec.data = Ptr32::NULL;
        }
    }
    free_dict_chapter_names(ren);
    let freed = ren.ed.dispose(false);
    for n in freed.nodes {
        ren.nodes.free(n.0);
    }
}

/// [soft 0x082c574c freeDictChapterNames]: every chapter's name is freed and its pointer
/// cleared (`SI_elemdictDispose` leaves the names alone).
pub fn free_dict_chapter_names(d: &mut Dict) {
    for l in d.ed.lists.iter_mut().take(d.ed.rec.nb_lists.max(0) as usize) {
        l.name.clear();
        l.rec.name = Ptr32::NULL;
    }
}

/// [soft 0x082c5794 resetTextureElemMaps]: the pointer at +0x24 of the record of every 2D
/// texture element in use is cleared. The records are the scene's.
pub fn reset_texture_elem_maps(d: &Dict, env: &mut Env) {
    for e in &d.ed.lists[env.p.cid.t2d as usize].elems {
        if e.rec.status == ELEM_VALID {
            if let Some(im) = env.heap.image_mut(e.rec.data.0) {
                im.set_u32(t2d::MAP, 0);
            }
        }
    }
}

/// [soft 0x082c65d8 initialiseRenderParms]: the 0x674 bytes are cleared (the controls at
/// +0x0c a second time, the word at +8 a third) and the 16-bit words at +0x60c and +0x60e
/// set to 1 and 0.
pub fn initialise_render_parms() -> Image {
    let mut p = Image::new(parms::SIZE);
    p.set_u16(parms::W60C, 1);
    p.set_u16(parms::LIVE, 0);
    p
}

/// Sizes the per-model table of every selective light
/// [soft 0x082c6628 updateSelectiveLights]: for each light element in use whose record is
/// selective (+0x15c), when the table's length (+0x164) is not the number of slots of the
/// MODELS chapter, the table (+0x168) is reallocated to that many words and one more, the
/// words from the old length on are cleared, and the length is updated. A null table with
/// length 0 becomes a cleared table.
pub fn update_selective_lights(d: &Dict, env: &mut Env) {
    let n = d.ed.lists[env.p.cid.model as usize].rec.nb_elems;
    for e in &d.ed.lists[env.p.cid.lig as usize].elems {
        if e.rec.status != ELEM_VALID {
            continue;
        }
        let light = e.rec.data.0;
        let Some(im) = env.heap.image(light) else { continue };
        if im.i32(lig::SELECTIVE) == 0 {
            continue;
        }
        let (old, table) = (im.i32(lig::TABLE_LEN), im.u32(lig::TABLE));
        if old != n {
            let size = (n + 1).max(0) as usize;
            let table = match env.heap.words_mut(table) {
                Some(w) => {
                    w.resize(size, 0);
                    table
                }
                None => env.heap.alloc(Obj::Words(vec![0; size])),
            };
            // the words from the old length on; what lies below it is kept
            if let Some(w) = env.heap.words_mut(table) {
                for k in old.max(0) as usize..size {
                    w[k] = 0;
                }
            }
            env.heap.image_mut(light).unwrap().set_u32(lig::TABLE, table);
        }
        env.heap.image_mut(light).unwrap().set_i32(lig::TABLE_LEN, n);
    }
}

/// Puts an editor's preview in place in a render dictionary
/// [soft 0x082c66f8 addPreviewRelations]. Nothing unless the preview names a model that is
/// an element in use of the dictionary. Then by kind: 0, the material on the selected
/// models; 1 and 3, the 2D texture list as the model's global textures or as the local
/// textures of its current material; 2 and 4, the 3D texture, made an element, as the
/// model's global 3D texture or as the local one of its current material.
pub fn add_preview_relations(ren: &mut Dict, env: &mut Env, out: &mut dyn Outside, prev: &Preview) {
    if prev.model == 0 {
        return;
    }
    let (_, index) = get_elem_from_data(ren, env.p.cid.model, prev.model);
    if index == -1 {
        return;
    }
    match prev.kind {
        0 => rdict::attach_material_to_selected_models(ren, env, out, prev),
        1 if prev.tex2d != 0 => rdict::attach_t2d_global(ren, env, prev.model, prev.tex2d),
        3 if prev.tex2d != 0 => rdict::attach_t2d_local(ren, env, prev.model, prev.tex2d),
        2 | 4 if prev.tex3d != 0 => {
            let t = rdict::add_elem_data(ren, env, env.p.cid.t3d, prev.tex3d);
            if prev.kind == 2 {
                rdict::replace_t3d_glb(ren, env, index, t);
            } else {
                rdict::replace_t3d_lcl(ren, env, index, t);
            }
        }
        _ => {}
    }
}

/// The names statics, for callers that make a render dictionary outside [`RenState`].
pub fn new_names() -> NewName {
    NewName::default()
}
