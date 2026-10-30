// SPDX-License-Identifier: MIT
//! `rens_instance.c`: instances made real elements of a render dictionary, kept up to date
//! from frame to frame, and taken away again.
//!
//! An instance is a model node whose algorithm (+0x2ec) is 15: it has no geometry of its own
//! and stands for a copy of another hierarchy, its master, which its element names by
//! relation 20002 (list 4). For a render each instance is replaced, in the render dictionary
//! only, by a copy of the master's hierarchy "without geometry": new nodes that point at the
//! master's geometry, materials and textures. The copy's root takes the instance's place in
//! the instance's element; the nodes under it get new elements at the end of the MODELS
//! chapter. So a render element is "of an instance" when the element number its node
//! carries (+0x354) names an instance in the scene's dictionary: [`instance_elem`].
//!
//! The copying of the hierarchy itself is `instance.c`'s (`SI_modelExpandAllWithoutGeometryAbsolute`)
//! and is asked of [`Outside`].

use crate::edlocal::{add_model, instance_material_share, instance_texture_share, model_master_from_instance, replace_model, Backups, NewName};
use crate::outside::Outside;
use crate::propagate::propagate_transform;
use crate::rdict;
use crate::world::{algo, data_of, get_elem, mat, rel, Env};
use si_scene::dict::{Dict, ElemId};
use si_scene::elemdict as ed;
use si_scene::elemrel;
use si_types::dict::ELEM_VALID;
use si_types::Ptr32;

/// What the instance routines keep between calls besides the dictionaries.
#[derive(Clone, Copy, Debug, Default)]
pub struct InstState {
    /// The statics of the name maker.
    pub names: NewName,
    /// Requests to save the model relations, which `SI_elemdictAddElem` leaves to `edtools.c`.
    pub backups: Backups,
    /// Instances expanded by the last [`expand_instances`].
    pub expanded: usize,
    /// Elements added for the nodes under expanded roots.
    pub added: usize,
}

/// The scene element of the instance a render element belongs to
/// [soft 0x082c79d4 SI_instanceElem]: render element `index` of MODELS must be in use and
/// have a node; the scene element whose number that node carries (+0x354) must be in use and
/// have a node whose algorithm is 15. Null otherwise.
pub fn instance_elem(index: i32, scene: &Dict, ren: &Dict, env: &Env) -> Option<ElemId> {
    let cm = env.p.cid.model;
    let e = get_elem(ren, cm, index)?;
    let rec = ren.slot(e)?.rec;
    if rec.status != ELEM_VALID || rec.data.is_null() {
        return None;
    }
    let s = get_elem(scene, cm, env.heap.model(rec.data.0)?.elem_id)?;
    let srec = scene.slot(s)?.rec;
    if srec.status != ELEM_VALID || srec.data.is_null() {
        return None;
    }
    (env.heap.model(srec.data.0)?.algo_type == algo::INSTANCE).then_some(s)
}

/// Expands every instance of a render dictionary
/// [soft 0x082c70b0 SI_expandInstances]: each root model element in use, of those the
/// chapter holds when the routine starts, is gone through with its hierarchy; then the
/// depths and roots of MODELS are worked out again, the material arrays of the copies are
/// put right, and the copies' materials and textures become relations.
pub fn expand_instances(scene: &mut Dict, ren: &mut Dict, env: &mut Env, out: &mut dyn Outside, st: &mut InstState) {
    let cm = env.p.cid.model;
    let n = ren.ed.lists[cm as usize].elems.len();
    for i in 0..n {
        let rec = ren.ed.lists[cm as usize].elems[i].rec;
        if rec.status == ELEM_VALID && rec.hier_level == 0 {
            expand_instance_tree(scene, ren, env, out, st, ElemId::new(cm, i as i32), 0, 0);
        }
    }
    ren.ed.update_hier_info(cm, 1, rel::PARENT);
    adjust_inst_material_array(scene, ren, env);
    add_instance_mat_tex_to_dictionary(scene, ren, env);
}

/// One element and the elements under it [soft 0x082c7768 expandInstanceTree].
///
/// When the element's node is an instance and its master can be copied: the copy's father
/// becomes `parent` (the node the walk came from); the copy takes the instance's place in
/// the element; the nodes under the copy become new elements; the element and every element
/// under it get the element's prefix; depths and roots are worked out again; and the walk
/// goes on from the copy.
///
/// Two things are kept as they are. The element the sons are asked of is found again **by
/// the number its node carried** (+0x354), not by its position: for an element made by an
/// expansion that number is the master's element, so the first son is the element's own and
/// the following ones are the next sons of the master's element. And the prefix is handed
/// on after the name record's own copy of it has been freed: the original reads freed
/// memory there; the port hands on the string as it was, which is what an allocator that
/// leaves freed blocks alone gives.
#[allow(clippy::too_many_arguments)]
fn expand_instance_tree(scene: &mut Dict, ren: &mut Dict, env: &mut Env, out: &mut dyn Outside, st: &mut InstState, elem: ElemId, parent: u32, depth: usize) {
    let cm = env.p.cid.model;
    if depth > ren.ed.lists[cm as usize].elems.len() {
        return; // a cycle of parent relations: the original recurses until its stack is gone
    }
    let Some(rec) = ren.slot(elem).map(|s| s.rec) else { return };
    let mut model = rec.data.0;
    if model == 0 || rec.status != ELEM_VALID {
        return;
    }
    let Some(m) = env.heap.model(model) else { return };
    let (id, is_instance) = (m.elem_id, m.algo_type == algo::INSTANCE);
    let mut elem = elem;
    if is_instance {
        let copy = expand_instance_hierarchy(scene, ren, env, out, model);
        if copy != 0 {
            if let Some(c) = env.heap.model_mut(copy) {
                c.father = Ptr32::new(parent);
            }
            assign_instance_hierarchy_root(ren, env, st, elem, copy);
            add_instance_hierarchy_elems(ren, env, st, copy, 0);
            elem = ElemId::new(cm, id);
            let prefix = ren.node_of(cm, id).and_then(|n| n.prefix.clone());
            assign_prefix_expanded_hierarchy(ren, env, elem, prefix.as_deref(), 0);
            ren.ed.update_hier_info(cm, 1, rel::PARENT);
            model = copy;
            st.expanded += 1;
        }
    }
    let mut son = ed::find_son(ren, &env.p, elem);
    while let Some(s) = son {
        expand_instance_tree(scene, ren, env, out, st, s, model, depth + 1);
        son = ed::find_next_son(ren, &env.p, ElemId::new(cm, id));
    }
}

/// [soft 0x082c78a4 assignInstanceHierarchyRoot]: the element whose number the element's
/// node carries (+0x354) is made to stand for the copy, under a new name.
fn assign_instance_hierarchy_root(ren: &mut Dict, env: &mut Env, st: &mut InstState, elem: ElemId, copy: u32) {
    if copy == 0 {
        return;
    }
    let index = env.heap.model(data_of(ren, elem)).map_or(-1, |m| m.elem_id);
    replace_model(ren, env, &mut st.names, index, copy, true);
}

/// Makes elements for the nodes under a copy's root
/// [soft 0x082c78d8 addInstanceHierarchyElems]. The root itself (depth 0) is not added and
/// its brothers are not looked at. A node below it is added under a new name, with the
/// element number it carried put back afterwards (the number of the master's node it was
/// copied from); then its brothers, then its sons.
fn add_instance_hierarchy_elems(ren: &mut Dict, env: &mut Env, st: &mut InstState, model: u32, depth: i32) {
    if model == 0 || depth > 4096 {
        return;
    }
    if depth > 0 {
        let saved = env.heap.model(model).map_or(-1, |m| m.elem_id);
        add_model(ren, env, &mut st.names, model, Ptr32::NULL, true, &mut st.backups);
        st.added += 1;
        let Some(m) = env.heap.model_mut(model) else { return };
        m.elem_id = saved;
        let brother = m.brother.0;
        if brother != 0 {
            add_instance_hierarchy_elems(ren, env, st, brother, depth);
        }
    }
    let son = env.heap.model(model).map_or(0, |m| m.son.0);
    if son != 0 {
        add_instance_hierarchy_elems(ren, env, st, son, depth + 1);
    }
}

/// The copy of an instance's master [soft 0x082c7950 expandInstanceHierarchy]: the master
/// is the one the *render* dictionary names; the copy takes bit 8 of the instance's flag
/// byte (+0x08) if set, the element number the instance carries, an array of the instance's
/// materials, the instance's textures, and the instance's transformation. Null when the
/// instance has no master or the copy fails.
fn expand_instance_hierarchy(scene: &mut Dict, ren: &mut Dict, env: &mut Env, out: &mut dyn Outside, model: u32) -> u32 {
    if model == 0 {
        return 0;
    }
    let master = model_master_from_instance(ren, &env.p, model);
    if master == 0 {
        return 0;
    }
    let copy = out.model_expand_all_without_geometry_absolute(&mut env.heap, scene, master);
    if copy == 0 {
        return 0;
    }
    let Some(inst) = env.heap.model(model).copied() else { return 0 };
    if let Some(c) = env.heap.model_mut(copy) {
        if inst.flags_08 & 8 != 0 {
            c.flags_08 |= 8;
        }
        c.elem_id = inst.elem_id;
    }
    instance_material_share(env, model, copy);
    instance_texture_share(&mut env.heap, model, copy);
    out.expand_update_transfo(&mut env.heap, scene, master, model, copy);
    copy
}

/// Gives an element and every element under it a prefix (null for none)
/// [soft 0x082c7b48 assignPrefixExpandedHierarchy]. The full name is not rebuilt.
fn assign_prefix_expanded_hierarchy(ren: &mut Dict, env: &Env, elem: ElemId, prefix: Option<&[u8]>, depth: usize) {
    if depth > ren.ed.lists[elem.chap as usize].elems.len() {
        return;
    }
    if let Some(n) = ren.node_of_mut(elem.chap, elem.index) {
        n.prefix = prefix.map(|p| p.to_vec());
    }
    let mut son = ed::find_son(ren, &env.p, elem);
    while let Some(s) = son {
        assign_prefix_expanded_hierarchy(ren, env, s, prefix, depth + 1);
        son = ed::find_next_son(ren, &env.p, elem);
    }
}

/// Puts right the material array of each copy that stands in an instance's element
/// [soft 0x082c7a4c adjustInstMaterialArray]: where the copy has materials and the
/// instance's master (as the *scene* dictionary names it) has more than one, the copy's
/// array is freed and replaced by a new array of the master's materials whose first entry is
/// the copy's old first material. (The original reads that first entry before it tests the
/// array for null, and the master without testing it; the port reads nothing there.)
pub fn adjust_inst_material_array(scene: &mut Dict, ren: &mut Dict, env: &mut Env) {
    let cm = env.p.cid.model as usize;
    let mut i = 0;
    while i < ren.ed.lists[cm].elems.len() {
        let index = i as i32;
        i += 1;
        let Some(inst) = instance_elem(index, scene, ren, env) else { continue };
        let rec = ren.ed.lists[cm].elems[index as usize].rec;
        let inst_model = data_of(scene, inst);
        if rec.status != ELEM_VALID || rec.data.is_null() || inst_model == 0 {
            continue;
        }
        let copy = rec.data.0;
        let master = model_master_from_instance(scene, &env.p, inst_model);
        let (Some(c), Some(m)) = (env.heap.model(copy), env.heap.model(master)) else { continue };
        if c.nb_materials > 0 && m.nb_materials > 1 {
            let old = c.materials.0;
            let Some(first) = env.heap.words(old).and_then(|a| a.first().copied()) else { continue };
            env.heap.free(old);
            let c = env.heap.model_mut(copy).unwrap();
            c.materials = Ptr32::NULL;
            c.nb_materials = 0;
            instance_material_share(env, master, copy);
            let arr = env.heap.model(copy).unwrap().materials.0;
            if let Some(slot) = env.heap.words_mut(arr).and_then(|a| a.first_mut()) {
                *slot = first;
            }
        }
    }
}

/// [soft 0x082c7204 addInstanceMatTexToDictionary]: every render element in use that
/// belongs to an instance has its materials and textures made relations.
pub fn add_instance_mat_tex_to_dictionary(scene: &mut Dict, ren: &mut Dict, env: &mut Env) {
    let cm = env.p.cid.model as usize;
    let mut i = 0;
    while i < ren.ed.lists[cm].elems.len() {
        let index = i as i32;
        i += 1;
        if instance_elem(index, scene, ren, env).is_some() && ren.ed.lists[cm].elems[index as usize].rec.status == ELEM_VALID {
            attach_inst_mat_tex_relations(ren, env, index, 0);
        }
    }
}

/// Rebuilds the material and texture relations of a model element from its node, and of the
/// elements under it [soft 0x082c7280 attachInstMatTexRelations].
///
/// The element's relations 300, 400 and 500 are removed. Each material of the node's array
/// becomes an element (if it is none yet) and is related to the model; its local 3D texture
/// relation is removed, and its local 2D textures (+0x6ec) and local 3D texture (+0x6fc)
/// are related to it. The node's global 2D textures (+0x21c) and global 3D texture (+0x224)
/// are related to the model.
///
/// One slip is kept: the material's local 2D texture relations are meant to be removed
/// first, but the element handed to the removal is looked up in the MODELS chapter with the
/// material's index, so the removal works on a model element, which has none, and a material
/// that was an element already keeps its old 401 relations beside the new ones.
fn attach_inst_mat_tex_relations(ren: &mut Dict, env: &mut Env, index: i32, depth: usize) {
    let c = env.p.cid;
    if depth > ren.ed.lists[c.model as usize].elems.len() {
        return;
    }
    let Some(e) = get_elem(ren, c.model, index) else { return };
    let model = data_of(ren, e);
    if model == 0 {
        return;
    }
    rdict::remove_mat_relations(ren, env, index);
    rdict::remove_t2d_global_relations(ren, env, Some(e));
    rdict::remove_t3d_glb_relation(ren, env, index);
    let Some(m) = env.heap.model(model).copied() else { return };
    if !m.materials.is_null() {
        for k in 0..m.nb_materials.max(0) as usize {
            let material = env.heap.words(m.materials.0).and_then(|a| a.get(k).copied()).unwrap_or(0);
            let mi = rdict::add_elem_data(ren, env, c.mat, material);
            elemrel::add_uncomplete_relation(ren, &mut env.p, c.model, c.mat, index, mi, rel::MAT, 0, 0, Ptr32::NULL, 1);
            let wrong = get_elem(ren, c.model, mi);
            rdict::remove_t2d_local_relations(ren, env, wrong);
            rdict::remove_t3d_lcl_relation(ren, env, mi);
            let (tex, tex3) = env.heap.image(material).map_or((0, 0), |im| (im.u32(mat::TEXTURES), im.u32(mat::TEXTURE3D)));
            if tex != 0 {
                rdict::add_t2d_lcl_relations(ren, env, mi, tex);
            }
            if tex3 != 0 {
                rdict::add_t3d_lcl_relation(ren, env, mi, tex3);
            }
        }
    }
    if !m.textures.is_null() {
        rdict::add_t2d_glb_relations(ren, env, index, m.textures.0);
    }
    if !m.texture3d.is_null() {
        rdict::add_t3d_glb_relation(ren, env, index, m.texture3d.0);
    }
    let mut son = ed::find_son(ren, &env.p, e);
    while let Some(s) = son {
        let si = ed::get_elem_id(ren, c.model, s);
        attach_inst_mat_tex_relations(ren, env, si, depth + 1);
        son = ed::find_next_son(ren, &env.p, e);
    }
}

/// Frees the material arrays the expansion made
/// [soft 0x082c7144 SI_deleteInstanceMaterialArrays]: of every render element in use that
/// belongs to an instance and whose node has more than one material, the array is freed and
/// the count and the current material's number are cleared.
pub fn delete_instance_material_arrays(scene: &Dict, ren: &Dict, env: &mut Env) {
    let cm = env.p.cid.model as usize;
    for i in 0..ren.ed.lists[cm].elems.len() {
        if instance_elem(i as i32, scene, ren, env).is_none() {
            continue;
        }
        let rec = ren.ed.lists[cm].elems[i].rec;
        if rec.status != ELEM_VALID {
            continue;
        }
        let Some(m) = env.heap.model(rec.data.0) else { continue };
        if m.nb_materials > 1 {
            let arr = m.materials.0;
            env.heap.free(arr);
            let m = env.heap.model_mut(rec.data.0).unwrap();
            m.materials = Ptr32::NULL;
            m.nb_materials = 0;
            m.cur_material = 0;
        }
    }
}

/// What is taken away again when a render dictionary is deleted
/// [soft 0x082c745c SI_instanceElementCleanup]: the material arrays above; then **the names**
/// of the nodes of the render elements that belong to an instance (among the first as many
/// as the scene has model elements) and of every node of the elements beyond those, the
/// ones the expansion added. The node records themselves, the copies, are not freed here or
/// by anything this file calls.
pub fn instance_element_cleanup(scene: &Dict, ren: &Dict, env: &mut Env) {
    delete_instance_material_arrays(scene, ren, env);
    let cm = env.p.cid.model as usize;
    let n = scene.ed.lists[cm].elems.len();
    let free_name = |env: &mut Env, i: usize| {
        let Some(e) = ren.ed.lists[cm].elems.get(i) else { return };
        if e.rec.status != ELEM_VALID {
            return;
        }
        let Some(m) = env.heap.model(e.rec.data.0) else { return };
        let name = m.name.0;
        if name != 0 {
            env.heap.free(name);
            env.heap.model_mut(e.rec.data.0).unwrap().name = Ptr32::NULL;
        }
    };
    for i in 0..n {
        if instance_elem(i as i32, scene, ren, env).is_some() {
            free_name(env, i);
        }
    }
    for i in n..ren.ed.lists[cm].elems.len() {
        free_name(env, i);
    }
}

/// Brings the nodes of a render dictionary up to date with the scene's
/// [soft 0x082c7574 SI_transferInstLocalTransforms]. For each render element in use with a
/// node, the scene element whose number the node carries gives its node's local scaling,
/// rotation angles, translation and rotation matrix. Then the geometry pointers (+0x38,
/// +0x3c, +0x48) and the number of definitions (+0x34) are taken: for an element that
/// belongs to an instance from the instance's master; for an element whose own index is
/// the number its node carries, from nobody; for the others (the nodes under an expanded
/// root) from the scene node itself.
pub fn transfer_inst_local_transforms(scene: &mut Dict, ren: &Dict, env: &mut Env) {
    let cm = env.p.cid.model;
    for i in 0..ren.ed.lists[cm as usize].elems.len() {
        let rec = ren.ed.lists[cm as usize].elems[i].rec;
        if rec.status != ELEM_VALID {
            continue;
        }
        let model = rec.data.0;
        let Some(m) = env.heap.model(model) else { continue };
        let Some(s) = get_elem(scene, cm, m.elem_id) else { continue };
        let elem_id = m.elem_id;
        let source = data_of(scene, s);
        let Some(src) = env.heap.model(source).copied() else { continue };
        let m = env.heap.model_mut(model).unwrap();
        m.scal = src.scal;
        m.vrot = src.vrot;
        m.trans = src.trans;
        m.mrot = si_math::algebra::matrix_copy(&src.mrot);
        let geometry = match instance_elem(i as i32, scene, ren, env) {
            None if i as i32 == elem_id => 0,
            None => source,
            Some(inst) => {
                let inst_model = data_of(scene, inst);
                model_master_from_instance(scene, &env.p, inst_model)
            }
        };
        if let Some(g) = env.heap.model(geometry).copied() {
            let m = env.heap.model_mut(model).unwrap();
            m.definition = g.definition;
            m.org_def = g.org_def;
            m.defs = g.defs;
            m.nb_defs = g.nb_defs;
        }
    }
}

/// [soft 0x082c76fc SI_updateInstTransforms]: the transfer above, then the matrices of every
/// hierarchy of the render dictionary are rebuilt from its root down.
pub fn update_inst_transforms(scene: &mut Dict, ren: &mut Dict, env: &mut Env, out: &mut dyn Outside) {
    transfer_inst_local_transforms(scene, ren, env);
    let cm = env.p.cid.model;
    let mut i = 0;
    while (i as usize) < ren.ed.lists[cm as usize].elems.len() {
        let rec = ren.ed.lists[cm as usize].elems[i as usize].rec;
        if rec.status == ELEM_VALID && rec.hier_level == 0 {
            propagate_transform(ren, env, out, ElemId::new(cm, i));
        }
        i += 1;
    }
}
