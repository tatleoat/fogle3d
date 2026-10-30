// SPDX-License-Identifier: MIT
//! `renu_dict.c`: material, texture and shader relations copied, attached, removed and
//! replaced in a render dictionary.
//!
//! All of them work on one dictionary with the relations of `Relations.rsrc`: a model has
//! its materials by 300, its global 2D textures by 400 and its global 3D texture by 500; a
//! material its local 2D textures by 401 and its local 3D texture by 501; a material or
//! texture its shaders by 550. Each is a forward entry in list 3 of the owner.
//!
//! An original routine that takes `__SI_Elem *` takes an [`ElemId`] here; where the original
//! goes on with a null element (a look-up that found nothing) the port goes on with `None`
//! and gets the same -1 from [`elem_id_of`].

use crate::edlocal::get_elem_from_data;
use crate::outside::Outside;
use crate::world::{data_of, get_elem, mat, rel, t2d, t3d, Env, Preview};
use si_scene::dict::{Dict, ElemId};
use si_scene::elemdict as ed;
use si_scene::elemrel;
use si_types::dict::ELEM_VALID;
use si_types::Ptr32;

/// `SI_elemdictGetElemId(dictionary, chapter, element)` with the element as a look-up left
/// it: -1 for none, as the original's pointer arithmetic gives for a null pointer.
pub fn elem_id_of(d: &Dict, chap: i32, e: Option<ElemId>) -> i32 {
    e.map_or(-1, |e| ed::get_elem_id(d, chap, e))
}

fn add(d: &mut Dict, env: &mut Env, src_chap: i32, dst_chap: i32, src: i32, dst: i32, relid: i32) {
    elemrel::add_uncomplete_relation(d, &mut env.p, src_chap, dst_chap, src, dst, relid, 0, 0, Ptr32::NULL, 1);
}

fn remove(d: &mut Dict, env: &mut Env, src_chap: i32, dst_chap: i32, src: i32, dst: i32, relid: i32) {
    ed::remove_uncomplete_rel_pair(d, &mut env.p, src_chap, dst_chap, src, dst, relid, 0, -1);
}

fn status(d: &Dict, e: ElemId) -> i16 {
    d.slot(e).map_or(-1, |s| s.rec.status)
}

/// Gives `to` the materials of `from` [soft 0x082c8670 SI_dictCopyMaterialRel]: when
/// `from` has a relation 300 to MATERIALS on its forward side, each material of its list 3
/// is related to `to` too. The walk stops at the first material element without an object.
pub fn copy_material_rel(d: &mut Dict, env: &mut Env, from: ElemId, to: ElemId) {
    let (cm, cmat) = (env.p.cid.model, env.p.cid.mat);
    if !ed::relation_check(d, from, true, rel::MAT, cmat) {
        return;
    }
    let to_id = ed::get_elem_id(d, cm, to);
    let mut r = ed::get_first_related_elem(d, from, true, 3, rel::MAT, cmat);
    while !r.data.is_null() {
        let id = elem_id_of(d, cmat, r.elem);
        add(d, env, cm, cmat, to_id, id, rel::MAT);
        r = ed::get_next_related_elem(d, from, true, 3, rel::MAT, cmat, Some(r.cursor));
    }
}

/// The same for the global 2D textures [soft 0x082c8750 SI_dictCopyTexture2dRel].
pub fn copy_texture2d_rel(d: &mut Dict, env: &mut Env, from: ElemId, to: ElemId) {
    let (cm, ct) = (env.p.cid.model, env.p.cid.t2d);
    if !ed::relation_check(d, from, true, rel::T2D_GLB, ct) {
        return;
    }
    let to_id = ed::get_elem_id(d, cm, to);
    let mut r = ed::get_first_related_elem(d, from, true, 3, rel::T2D_GLB, ct);
    while !r.data.is_null() {
        let id = elem_id_of(d, ct, r.elem);
        add(d, env, cm, ct, to_id, id, rel::T2D_GLB);
        r = ed::get_next_related_elem(d, from, true, 3, rel::T2D_GLB, ct, Some(r.cursor));
    }
}

/// The same for the one global 3D texture [soft 0x082c8830 SI_dictCopyTexture3dRel]. The
/// relation is looked for in every forward list but the texture only in list 3: when it is
/// elsewhere the relation is added towards element -1, which the adder refuses.
pub fn copy_texture3d_rel(d: &mut Dict, env: &mut Env, from: ElemId, to: ElemId) {
    let (cm, ct) = (env.p.cid.model, env.p.cid.t3d);
    if !ed::relation_check(d, from, true, rel::T3D_GLB, ct) {
        return;
    }
    let to_id = ed::get_elem_id(d, cm, to);
    let r = ed::get_first_related_elem(d, from, true, 3, rel::T3D_GLB, ct);
    let id = elem_id_of(d, ct, r.elem);
    add(d, env, cm, ct, to_id, id, rel::T3D_GLB);
}

/// The index of the element of a chapter that stands for an object, made if there is none
/// [soft 0x082c88d8 SI_dictAddElemData]. A new element is a new slot at the end of the
/// chapter with relation lists and **no name record**; the dictionary's dirty marks are not
/// touched. A null object always makes a new element.
pub fn add_elem_data(d: &mut Dict, env: &Env, chap: i32, data: u32) -> i32 {
    let (_, found) = get_elem_from_data(d, chap, data);
    if found >= 0 {
        return found;
    }
    let l = &mut d.ed.lists[chap as usize];
    let i = l.add_elem(Ptr32::NULL, Ptr32::NULL, Ptr32::NULL, 0, 0, true, false, env.p.load_optim, env.p.def_color);
    l.elems[i as usize].rec.status = ELEM_VALID;
    l.elems[i as usize].rec.data = Ptr32::new(data);
    l.sort_tbl_set_dirty();
    i
}

/// Shows the preview's material on the selected models
/// [soft 0x082c8958 SI_dictAttachMaterialToSelectedModels].
///
/// The material becomes an element. Then, for every model element in use that has an
/// object and is selected (selection state 1, 2 or 3), with `n` its number of materials in
/// the dictionary and `cur` the number of the node's current material (+0x208):
///
/// - `n` is 0, or `cur` equals `n`: if the node is the preview's model, the material is
///   added to its list and the material's four shaders become its shader relations;
/// - `cur` above `n`: a message on standard output, and the routine ends there;
/// - otherwise, if the node's current material is the one the preview replaces, or the node
///   is the preview's model: the node's material relations are removed and added again in
///   the same order with the preview's material in place `cur`; each material's local
///   textures are related to it again, and each material's shaders are related **to the
///   preview material's element** (the original passes that index for every one of them).
///
/// Where the original would read through a null pointer (a node without material array, a
/// material element without object) the port reads nothing.
pub fn attach_material_to_selected_models(d: &mut Dict, env: &mut Env, out: &mut dyn Outside, prev: &Preview) {
    let (cm, cmat) = (env.p.cid.model, env.p.cid.mat);
    let mat_id = add_elem_data(d, env, cmat, prev.material);
    let mut i = 0;
    while (i as usize) < d.ed.lists[cm as usize].elems.len() {
        let e = ElemId::new(cm, i);
        i += 1;
        let rec = d.slot(e).unwrap().rec;
        if rec.status != ELEM_VALID || rec.data.is_null() || !matches!(rec.selection, 1..=3) {
            continue;
        }
        let n = ed::relations_count(d, e, true, 3, 0, rel::MAT, cmat);
        let model = rec.data.0;
        let eid = ed::get_elem_id(d, cm, e);
        let cur = env.heap.model(model).map_or(0, |m| m.cur_material as i32);
        if n == 0 || cur == n {
            if model == prev.model {
                add(d, env, cm, cmat, eid, mat_id, rel::MAT);
                let s = shaders_of(env, prev.material);
                add_material_shader_relations(d, env, mat_id, s);
            }
            continue;
        }
        if n < cur {
            out.print(
                format!(
                    "{}, {}: SI_dictAttachMaterialToSelectedModels(): Error: mdl->currMtrl > nbMat\n",
                    "/home/data/si3d/OfficialBuilds/v4_0_2_0/Creative/final.v4_0_2_0-Linux24-flexlm/renutil/renu_dict.c", 0x16e
                )
                .as_bytes(),
            );
            return;
        }
        let current = env.heap.model_materials(model).and_then(|a| a.get(usize::try_from(cur).ok()?)).copied().unwrap_or(0);
        if !((prev.replaced != 0 && prev.replaced == current) || model == prev.model) {
            continue;
        }
        let mut list = Vec::with_capacity(n as usize);
        let mut r = ed::get_first_related_elem(d, e, true, 3, rel::MAT, cmat);
        for _ in 0..n {
            list.push(r.data.0);
            r = ed::get_next_related_elem(d, e, true, 3, rel::MAT, cmat, Some(r.cursor));
        }
        for m in &list {
            let (_, id) = get_elem_from_data(d, cmat, *m);
            remove(d, env, cm, cmat, eid, id, rel::MAT);
        }
        if let Some(slot) = usize::try_from(cur).ok().and_then(|c| list.get_mut(c)) {
            *slot = prev.material;
        }
        for m in &list {
            let (_, id) = get_elem_from_data(d, cmat, *m);
            add(d, env, cm, cmat, eid, id, rel::MAT);
            let (tex, tex3) = env.heap.image(*m).map_or((0, 0), |im| (im.u32(mat::TEXTURES), im.u32(mat::TEXTURE3D)));
            if tex != 0 {
                add_t2d_lcl_relations(d, env, id, tex);
            }
            if tex3 != 0 {
                add_t3d_lcl_relation(d, env, id, tex3);
            }
            let s = shaders_of(env, *m);
            add_material_shader_relations(d, env, mat_id, s);
        }
    }
}

/// The four shader pointers of a material (+0xdb0 to +0xdbc); zeros where there is no record.
fn shaders_of(env: &Env, material: u32) -> [u32; 4] {
    env.heap.image(material).map_or([0; 4], |im| mat::SHADERS.map(|o| im.u32(o)))
}

/// Shows a list of 2D textures as the global textures of a model
/// [soft 0x082c8d18 SI_dictAttachT2dGlobal]: the model's 400 relations are removed, then
/// one is added for each texture of the list.
pub fn attach_t2d_global(d: &mut Dict, env: &mut Env, model: u32, tex: u32) {
    if model == 0 || tex == 0 {
        return;
    }
    let (Some(e), idx) = get_elem_from_data(d, env.p.cid.model, model) else { return };
    if ed::relations_count(d, e, true, 3, 0, rel::T2D_GLB, env.p.cid.t2d) > 0 {
        remove_t2d_global_relations(d, env, Some(e));
    }
    add_t2d_glb_relations(d, env, idx, tex);
}

/// Shows a list of 2D textures as the local textures of a model's current material
/// [soft 0x082c8d98 SI_dictAttachT2dLocal]. The material is the model's `cur`-th (+0x208)
/// in the dictionary; when there is none the textures are related from element -1, which
/// the adder refuses.
pub fn attach_t2d_local(d: &mut Dict, env: &mut Env, model: u32, tex: u32) {
    if model == 0 || tex == 0 {
        return;
    }
    let (cm, cmat, ct) = (env.p.cid.model, env.p.cid.mat, env.p.cid.t2d);
    let (Some(e), _) = get_elem_from_data(d, cm, model) else { return };
    let cur = env.heap.model(model).map_or(0, |m| m.cur_material as i32);
    let me = ed::get_nth_related_elem(d, e, cur, true, 3, rel::MAT, cmat).elem;
    let mid = elem_id_of(d, cmat, me);
    if me.is_some_and(|me| ed::relations_count(d, me, true, 3, 0, rel::T2D_LOC, ct) > 0) {
        remove_t2d_local_relations(d, env, me);
    }
    add_t2d_lcl_relations(d, env, mid, tex);
}

/// Relates a model to every texture of a list, each made an element if it is none yet, and
/// each texture to its shader [soft 0x082c8e60 SI_dictAddT2dGlbRelations].
pub fn add_t2d_glb_relations(d: &mut Dict, env: &mut Env, model_id: i32, tex: u32) {
    let (cm, ct) = (env.p.cid.model, env.p.cid.t2d);
    let mut t = tex;
    while t != 0 {
        let Some((next, shader)) = env.heap.image(t).map(|im| (im.u32(t2d::NEXT), im.u32(t2d::SHADER))) else { break };
        let i = add_elem_data(d, env, ct, t);
        add(d, env, cm, ct, model_id, i, rel::T2D_GLB);
        add_t2d_shader_relation(d, env, i, shader);
        t = next;
    }
}

/// The same from a material, by 401 [soft 0x082c8eec SI_dictAddT2dLclRelations].
pub fn add_t2d_lcl_relations(d: &mut Dict, env: &mut Env, mat_id: i32, tex: u32) {
    let (cmat, ct) = (env.p.cid.mat, env.p.cid.t2d);
    let mut t = tex;
    while t != 0 {
        let Some((next, shader)) = env.heap.image(t).map(|im| (im.u32(t2d::NEXT), im.u32(t2d::SHADER))) else { break };
        let i = add_elem_data(d, env, ct, t);
        add(d, env, cmat, ct, mat_id, i, rel::T2D_LOC);
        add_t2d_shader_relation(d, env, i, shader);
        t = next;
    }
}

/// Relates a model to a 3D texture by 500 and the texture to its shader
/// [soft 0x082c8f70 SI_dictAddT3dGlbRelation].
pub fn add_t3d_glb_relation(d: &mut Dict, env: &mut Env, model_id: i32, tex: u32) {
    if tex == 0 {
        return;
    }
    let (cm, ct) = (env.p.cid.model, env.p.cid.t3d);
    let i = add_elem_data(d, env, ct, tex);
    add(d, env, cm, ct, model_id, i, rel::T3D_GLB);
    let shader = env.heap.image(tex).map_or(0, |im| im.u32(t3d::SHADER));
    add_t3d_shader_relation(d, env, i, shader);
}

/// The same from a material, by 501 [soft 0x082c8fe4 SI_dictAddT3dLclRelation].
pub fn add_t3d_lcl_relation(d: &mut Dict, env: &mut Env, mat_id: i32, tex: u32) {
    if tex == 0 {
        return;
    }
    let (cmat, ct) = (env.p.cid.mat, env.p.cid.t3d);
    let i = add_elem_data(d, env, ct, tex);
    add(d, env, cmat, ct, mat_id, i, rel::T3D_LOC);
    let shader = env.heap.image(tex).map_or(0, |im| im.u32(t3d::SHADER));
    add_t3d_shader_relation(d, env, i, shader);
}

/// Removes a model's material relations, last first
/// [soft 0x082c9058 SI_dictRemoveMatRelations].
pub fn remove_mat_relations(d: &mut Dict, env: &mut Env, model_id: i32) {
    let (cm, cmat) = (env.p.cid.model, env.p.cid.mat);
    let Some(e) = get_elem(d, cm, model_id) else { return };
    let mut n = ed::relations_count(d, e, true, 3, 0, rel::MAT, cmat);
    while n > 0 {
        n -= 1;
        let m = ed::get_nth_related_elem(d, e, n, true, 3, rel::MAT, cmat).elem;
        let id = elem_id_of(d, cmat, m);
        remove(d, env, cm, cmat, model_id, id, rel::MAT);
    }
}

/// Removes a model element's global 2D texture relations, last first
/// [soft 0x082c911c SI_dictRemoveT2dGlobalRelations].
pub fn remove_t2d_global_relations(d: &mut Dict, env: &mut Env, e: Option<ElemId>) {
    let Some(e) = e else { return };
    let (cm, ct) = (env.p.cid.model, env.p.cid.t2d);
    let mut n = ed::relations_count(d, e, true, 3, 0, rel::T2D_GLB, ct);
    let mid = ed::get_elem_id(d, cm, e);
    while n > 0 {
        n -= 1;
        let t = ed::get_nth_related_elem(d, e, n, true, 3, rel::T2D_GLB, ct).elem;
        let id = elem_id_of(d, ct, t);
        remove(d, env, cm, ct, mid, id, rel::T2D_GLB);
    }
}

/// Removes a material element's local 2D texture relations, last first
/// [soft 0x082c91e4 SI_dictRemoveT2dLocalRelations]. Unlike the two routines above it finds
/// each texture's element again by the texture's object, so a texture element without an
/// object keeps its relation.
pub fn remove_t2d_local_relations(d: &mut Dict, env: &mut Env, e: Option<ElemId>) {
    let Some(e) = e else { return };
    let (cmat, ct) = (env.p.cid.mat, env.p.cid.t2d);
    let mut n = ed::relations_count(d, e, true, 3, 0, rel::T2D_LOC, ct);
    let mid = ed::get_elem_id(d, cmat, e);
    while n > 0 {
        n -= 1;
        let data = ed::get_nth_related_elem(d, e, n, true, 3, rel::T2D_LOC, ct).data;
        let (_, id) = get_elem_from_data(d, ct, data.0);
        remove(d, env, cmat, ct, mid, id, rel::T2D_LOC);
    }
}

/// Removes a model's global 3D texture relation
/// [soft 0x082c92ac SI_dictRemoveT3dGlbRelation].
pub fn remove_t3d_glb_relation(d: &mut Dict, env: &mut Env, model_id: i32) {
    let (cm, ct) = (env.p.cid.model, env.p.cid.t3d);
    let Some(e) = get_elem(d, cm, model_id) else { return };
    let data = ed::get_first_related_elem(d, e, true, 3, rel::T3D_GLB, ct).data;
    if !data.is_null() {
        let (_, id) = get_elem_from_data(d, ct, data.0);
        remove(d, env, cm, ct, model_id, id, rel::T3D_GLB);
    }
}

/// Removes a material's local 3D texture relation
/// [soft 0x082c9340 SI_dictRemoveT3dLclRelation].
pub fn remove_t3d_lcl_relation(d: &mut Dict, env: &mut Env, mat_id: i32) {
    let (cmat, ct) = (env.p.cid.mat, env.p.cid.t3d);
    let Some(e) = get_elem(d, cmat, mat_id) else { return };
    let data = ed::get_first_related_elem(d, e, true, 3, rel::T3D_LOC, ct).data;
    if !data.is_null() {
        let (_, id) = get_elem_from_data(d, ct, data.0);
        remove(d, env, cmat, ct, mat_id, id, rel::T3D_LOC);
    }
}

fn nb(d: &Dict, chap: i32) -> i32 {
    d.ed.lists.get(chap as usize).map_or(0, |l| l.rec.nb_elems)
}

/// The shader of a 3D texture element in use that has an object, for the two routines below.
fn t3d_shader_of(d: &Dict, env: &Env, tex_id: i32) -> Option<u32> {
    let te = get_elem(d, env.p.cid.t3d, tex_id)?;
    let rec = d.slot(te)?.rec;
    if rec.status != ELEM_VALID || rec.data.is_null() {
        return None;
    }
    Some(env.heap.image(rec.data.0).map_or(0, |im| im.u32(t3d::SHADER)))
}

/// Makes a 3D texture element the global 3D texture of a model element
/// [soft 0x082c93d4 SI_dictReplaceT3dGlb]; both by index, each inside its chapter.
pub fn replace_t3d_glb(d: &mut Dict, env: &mut Env, model_id: i32, tex_id: i32) {
    let (cm, ct) = (env.p.cid.model, env.p.cid.t3d);
    if model_id < 0 || tex_id < 0 || model_id >= nb(d, cm) || tex_id >= nb(d, ct) || get_elem(d, cm, model_id).is_none() {
        return;
    }
    remove_t3d_glb_relation(d, env, model_id);
    add(d, env, cm, ct, model_id, tex_id, rel::T3D_GLB);
    if let Some(shader) = t3d_shader_of(d, env, tex_id) {
        add_t3d_shader_relation(d, env, tex_id, shader);
    }
}

/// Makes a 3D texture element the local 3D texture of a model's current material
/// [soft 0x082c94ac SI_dictReplaceT3dLcl].
pub fn replace_t3d_lcl(d: &mut Dict, env: &mut Env, model_id: i32, tex_id: i32) {
    let (cm, cmat, ct) = (env.p.cid.model, env.p.cid.mat, env.p.cid.t3d);
    if model_id < 0 || tex_id < 0 || model_id >= nb(d, cm) || tex_id >= nb(d, ct) {
        return;
    }
    let Some(e) = get_elem(d, cm, model_id) else { return };
    let model = data_of(d, e);
    if model == 0 {
        return;
    }
    let cur = env.heap.model(model).map_or(0, |m| m.cur_material as i32);
    let me = ed::get_nth_related_elem(d, e, cur, true, 3, rel::MAT, cmat).elem;
    let mid = elem_id_of(d, cmat, me);
    remove_t3d_lcl_relation(d, env, mid);
    add(d, env, cmat, ct, mid, tex_id, rel::T3D_LOC);
    if let Some(shader) = t3d_shader_of(d, env, tex_id) {
        add_t3d_shader_relation(d, env, tex_id, shader);
    }
}

/// Clears the selection state of every element in use of a chapter
/// [soft 0x082c95d8 SI_dictUnselectAllChapterElems].
pub fn unselect_all_chapter_elems(d: &mut Dict, chap: i32) {
    if chap < 0 || chap >= d.ed.rec.nb_lists {
        return;
    }
    for e in &mut d.ed.lists[chap as usize].elems {
        if e.rec.status == ELEM_VALID {
            e.rec.selection = 0;
        }
    }
}

/// Removes every picture of the PICTURES chapter that is in use and has an object
/// [soft 0x082c9628 SI_deleteImageMapElems], through `SI_elemdictPictureElemRemove`.
pub fn delete_image_map_elems(d: &mut Dict, env: &Env, out: &mut dyn Outside) {
    let c = env.p.cid.img as usize;
    let n = d.ed.lists[c].elems.len();
    for i in 0..n {
        let Some(e) = d.ed.lists[c].elems.get(i) else { break };
        if !e.rec.data.is_null() && e.rec.status == ELEM_VALID {
            let data = e.rec.data.0;
            out.picture_elem_remove(d, data);
        }
    }
}

/// Replaces the shader relations of a material element
/// [soft 0x082c968c SI_dictAddMaterialShaderRelations]: its first relation 550 towards each
/// of the chapters MATERIAL_SHADERS, VOLUME_SHADERS, SHADOW_SHADERS and
/// MATERIAL_PHOTON_SHADERS is removed, then each of the four shader records that is not
/// null becomes an element of its chapter and is related to the material.
pub fn add_material_shader_relations(d: &mut Dict, env: &mut Env, mat_id: i32, shaders: [u32; 4]) {
    let cmat = env.p.cid.mat;
    let Some(e) = get_elem(d, cmat, mat_id) else { return };
    if status(d, e) != ELEM_VALID {
        return;
    }
    let chaps = [env.p.cid.shd_mat, env.p.cid.shd_vol, env.p.cid.shd_sha, env.p.cid.shd_mat_photon];
    for c in chaps {
        let s = ed::get_first_related_elem(d, e, true, 3, rel::SHADER, c).elem;
        let id = elem_id_of(d, c, s);
        remove(d, env, cmat, c, mat_id, id, rel::SHADER);
    }
    for (c, s) in chaps.into_iter().zip(shaders) {
        if s != 0 {
            let i = add_elem_data(d, env, c, s);
            add(d, env, cmat, c, mat_id, i, rel::SHADER);
        }
    }
}

/// Replaces the shader relation of a 2D texture element
/// [soft 0x082c9968 SI_dictAddT2dShaderRelation]: its first 550 towards ENVIRON_SHADERS and
/// towards TEXTURE2D_SHADERS are removed; a shader record becomes an element of
/// ENVIRON_SHADERS when the texture's mapping method (+0x2a) is 7 or 8, else of
/// TEXTURE2D_SHADERS, and is related to the texture. Nothing is added for a texture element
/// without an object.
pub fn add_t2d_shader_relation(d: &mut Dict, env: &mut Env, tex_id: i32, shader: u32) {
    let ct = env.p.cid.t2d;
    let Some(e) = get_elem(d, ct, tex_id) else { return };
    if status(d, e) != ELEM_VALID {
        return;
    }
    for c in [env.p.cid.shd_env, env.p.cid.shd_t2d] {
        let s = ed::get_first_related_elem(d, e, true, 3, rel::SHADER, c).elem;
        let id = elem_id_of(d, c, s);
        remove(d, env, ct, c, tex_id, id, rel::SHADER);
    }
    let data = data_of(d, e);
    if shader != 0 && data != 0 {
        let method = env.heap.image(data).map_or(0, |im| im.u16(t2d::METHOD));
        let c = if method.wrapping_sub(7) < 2 { env.p.cid.shd_env } else { env.p.cid.shd_t2d };
        let i = add_elem_data(d, env, c, shader);
        add(d, env, ct, c, tex_id, i, rel::SHADER);
    }
}

/// Replaces the shader relation of a 3D texture element
/// [soft 0x082c9b10 SI_dictAddT3dShaderRelation].
pub fn add_t3d_shader_relation(d: &mut Dict, env: &mut Env, tex_id: i32, shader: u32) {
    let (ct, cs) = (env.p.cid.t3d, env.p.cid.shd_t3d);
    let Some(e) = get_elem(d, ct, tex_id) else { return };
    if status(d, e) != ELEM_VALID {
        return;
    }
    let s = ed::get_first_related_elem(d, e, true, 3, rel::SHADER, cs).elem;
    let id = elem_id_of(d, cs, s);
    remove(d, env, ct, cs, tex_id, id, rel::SHADER);
    if shader != 0 && data_of(d, e) != 0 {
        let i = add_elem_data(d, env, cs, shader);
        add(d, env, ct, cs, tex_id, i, rel::SHADER);
    }
}
