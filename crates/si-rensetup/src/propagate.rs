// SPDX-License-Identifier: MIT
//! `rens_propagate.c`: what a render dictionary's model elements inherit from their fathers:
//! the hidden and select states, the transformation, and materials and textures; and the
//! default material.
//!
//! The hierarchy walked is the dictionary's own: the sons of an element are the elements
//! related to it by 110 in list 1 of its backward side (`SI_elemdictFindSon`). Every walk
//! moves that list's cursor, as the original's does.

use crate::edlocal::{get_new_name, NewName};
use crate::outside::Outside;
use crate::rdict;
use crate::world::{data_of, get_elem, mat, rel, t2d, Env};
use si_db::ListNode;
use si_scene::dict::{Dict, ElemId};
use si_scene::elemdict as ed;
use si_scene::elemrel;
use si_types::dict::ELEM_VALID;
use si_types::model::{MFACE, MMETA, MNSRF, MPTCH, MSMSH};
use si_types::Ptr32;

/// The deepest hierarchy a walk follows. The original recurses without a limit, and a cycle
/// of parent relations takes its stack; the port stops there instead.
fn too_deep(d: &Dict, depth: usize) -> bool {
    depth > d.ed.lists.iter().map(|l| l.elems.len()).sum::<usize>()
}

/// Gives every element under `elem` its display type (+0x20: 0 hidden, 1 ghosted, 2
/// visible) [soft 0x082c67f0 SI_propagateHiddenFlag].
pub fn propagate_hidden_flag(d: &mut Dict, env: &Env, elem: ElemId) {
    hidden(d, env, elem, 0);
}

fn hidden(d: &mut Dict, env: &Env, elem: ElemId, depth: usize) {
    if too_deep(d, depth) {
        return;
    }
    let mut son = ed::find_son(d, &env.p, elem);
    while let Some(s) = son {
        let v = d.slot(elem).unwrap().rec.wire_type;
        d.slot_mut(s).unwrap().rec.wire_type = v;
        hidden(d, env, s, depth + 1);
        son = ed::find_next_son(d, &env.p, elem);
    }
}

/// Gives every element under `elem` its selection state (+0x24)
/// [soft 0x082c6844 SI_propagateSelectFlag].
pub fn propagate_select_flag(d: &mut Dict, env: &Env, elem: ElemId) {
    select(d, env, elem, 0);
}

fn select(d: &mut Dict, env: &Env, elem: ElemId, depth: usize) {
    if too_deep(d, depth) {
        return;
    }
    let mut son = ed::find_son(d, &env.p, elem);
    while let Some(s) = son {
        let v = d.slot(elem).unwrap().rec.selection;
        d.slot_mut(s).unwrap().rec.selection = v;
        select(d, env, s, depth + 1);
        son = ed::find_next_son(d, &env.p, elem);
    }
}

/// Rebuilds the matrices of an element's node and of every node under it, each from its
/// father's [soft 0x082c689c SI_propagateTransform]. The father is the object of the
/// element this one is related to by 110 (list 1, forward side), null for none. While the
/// nodes under a node are done its flag bit 0x8000 (+0x08) is set; it is cleared after.
/// An element without an object ends the walk on its branch.
pub fn propagate_transform(d: &mut Dict, env: &mut Env, out: &mut dyn Outside, elem: ElemId) {
    transform(d, env, out, elem, 0);
}

fn transform(d: &mut Dict, env: &mut Env, out: &mut dyn Outside, elem: ElemId, depth: usize) {
    if too_deep(d, depth) {
        return;
    }
    let father = elemrel::find_related_elem(d, elem, true, 1, rel::PARENT, env.p.cid.model);
    let model = data_of(d, elem);
    if model == 0 {
        return;
    }
    let father_model = father.map_or(0, |f| data_of(d, f));
    out.model_update_son_state(&mut env.heap, father_model, model, 0);
    if let Some(m) = env.heap.model_mut(model) {
        m.flags_08 |= 0x8000;
    }
    let mut son = ed::find_son(d, &env.p, elem);
    while let Some(s) = son {
        transform(d, env, out, s, depth + 1);
        son = ed::find_next_son(d, &env.p, elem);
    }
    if let Some(m) = env.heap.model_mut(model) {
        m.flags_08 &= 0x7fff;
    }
}

/// Makes the default material of a render dictionary
/// [soft 0x082c6930 si_renderAddDefaultMaterial]: a new material record, added to MATERIALS
/// as an element named `DEFAULT`. The record's own name (its first thirty bytes) is a name
/// no material element has yet, `DEFAULT` with a number, worked out before the element is
/// added; the record learns its element's index (+0xda8). Returns the element and its index.
pub fn render_add_default_material(d: &mut Dict, env: &mut Env, names: &mut NewName, out: &mut dyn Outside) -> (Option<ElemId>, i32) {
    let cmat = env.p.cid.mat;
    let material = out.material_allocate(&mut env.heap);
    let name = get_new_name(d, names, cmat, Some(b"DEFAULT"));
    let mut node = ListNode::allocate(env.p.now);
    node.name = Some(b"DEFAULT".to_vec());
    node.full = Some(si_db::format_full_name(&mut node, env.p.loading));
    let node = d.nodes.alloc(node);
    let index = d
        .ed
        .add_elem(cmat, Ptr32::new(node), Ptr32::new(material), Ptr32::NULL, 0, 0, env.p.cid.model, env.p.backup_enabled, env.p.load_optim, env.p.def_color)
        .map_or(-1, |a| a.index);
    if let Some(im) = env.heap.image_mut(material) {
        // strncpy(material, name, 0x1e): the rest of the thirty bytes is cleared
        for k in 0..mat::NAME_LEN {
            im.bytes[mat::NAME + k] = name.get(k).copied().unwrap_or(0);
        }
        im.set_i32(mat::ELEM_ID, index);
    }
    (get_elem(d, cmat, index), index)
}

/// [soft 0x082c69d8 testUvPropagation]: the texture's mapping method (+0x2a) is 4, UV.
fn test_uv_propagation(env: &Env, tex: u32) -> bool {
    env.heap.image(tex).is_some_and(|im| im.i16(t2d::METHOD) == 4)
}

/// The byte at +0x5b of an element: the top byte of its work flag, which this file uses as
/// "the textures of this model's material source have been marked".
fn mark(d: &Dict, e: ElemId) -> u8 {
    d.slot(e).map_or(0, |s| (s.rec.work_flag as u32 >> 24) as u8)
}

fn set_mark(d: &mut Dict, e: ElemId) {
    if let Some(s) = d.slot_mut(e) {
        s.rec.work_flag = (s.rec.work_flag as u32 & 0x00ff_ffff | 0x0100_0000) as i32;
    }
}

/// The node's kind (+0x30); 0, a null node's kind, when the element has no object, where
/// the original reads through a null pointer.
fn kind_of(d: &Dict, env: &Env, e: ElemId) -> i32 {
    env.heap.model(data_of(d, e)).map_or(0, |m| m.kind)
}

fn is_surface(kind: i32) -> bool {
    matches!(kind, MNSRF | MPTCH | MFACE | MSMSH | MMETA)
}

/// Kinds for which an inherited UV texture is no cause for a warning: null, patch, mesh,
/// joint, spline, NURBS curve and NURBS surface. (What is left: faces, metaball systems,
/// metaballs.)
fn uv_is_fine(kind: i32) -> bool {
    matches!(kind, 0 | 1 | 4 | 5 | 6 | 9 | 10)
}

/// Marks the 2D and 3D texture elements a model element uses, its own and its materials'
/// [soft 0x082c6e2c flagAllTextures]: the byte at +0x5b of each becomes 1.
fn flag_all_textures(d: &mut Dict, env: &Env, e: ElemId) {
    let c = env.p.cid;
    let mut r = ed::get_first_related_elem(d, e, true, 3, rel::T2D_GLB, c.t2d);
    while let Some(t) = r.elem {
        set_mark(d, t);
        r = ed::get_next_related_elem(d, e, true, 3, rel::T2D_GLB, c.t2d, Some(r.cursor));
    }
    if let Some(t) = ed::get_first_related_elem(d, e, true, 3, rel::T3D_GLB, c.t3d).elem {
        set_mark(d, t);
    }
    let mut m = ed::get_first_related_elem(d, e, true, 3, rel::MAT, c.mat);
    while let Some(me) = m.elem {
        let mut r = ed::get_first_related_elem(d, me, true, 3, rel::T2D_LOC, c.t2d);
        while let Some(t) = r.elem {
            set_mark(d, t);
            r = ed::get_next_related_elem(d, me, true, 3, rel::T2D_LOC, c.t2d, Some(r.cursor));
        }
        if let Some(t) = ed::get_first_related_elem(d, me, true, 3, rel::T3D_LOC, c.t3d).elem {
            set_mark(d, t);
        }
        m = ed::get_next_related_elem(d, e, true, 3, rel::MAT, c.mat, Some(m.cursor));
    }
}

/// One element of the material propagation [soft 0x082c69f0 propagateMaterials], then the
/// elements under it.
///
/// The element's work id (+0x5c) is first set to -1; it ends as the index of the model
/// element its materials come from, or stays -1 when they are its own.
///
/// With a father, when the element has no material of its own (no 300 to MATERIALS) and
/// the father has one:
/// - unless the element has a global texture of its own (400 or 500), its work id becomes
///   the father's work id, or the father's index when that is -1; and for a surface (patch,
///   face, mesh, metaball system, NURBS surface) the textures of that source element are
///   marked, once;
/// - for the kinds that cannot carry texture coordinates, a warning is given for each local
///   2D texture of the father's materials that is mapped by UV;
/// - the father's materials become the element's;
/// - the father's global 2D textures become the element's when it has none (with the same
///   warning), and so does the father's global 3D texture.
///
/// An element that kept its own materials and is a surface has its textures marked.
fn propagate_materials(d: &mut Dict, env: &mut Env, out: &mut dyn Outside, elem: ElemId, father: Option<ElemId>, depth: usize) {
    if too_deep(d, depth) {
        return;
    }
    let c = env.p.cid;
    d.slot_mut(elem).unwrap().rec.work_id = -1;
    let kind = kind_of(d, env, elem);
    'inherit: {
        let Some(father) = father else { break 'inherit };
        if ed::relation_check(d, elem, true, rel::MAT, c.mat) || !ed::relation_check(d, father, true, rel::MAT, c.mat) {
            break 'inherit;
        }
        let own_texture = ed::relation_check(d, elem, true, rel::T2D_GLB, c.t2d) || ed::relation_check(d, elem, true, rel::T3D_GLB, c.t3d);
        if !own_texture {
            let mut source = d.slot(father).unwrap().rec.work_id;
            if source == -1 {
                source = ed::get_elem_id(d, c.model, father);
            }
            d.slot_mut(elem).unwrap().rec.work_id = source;
            if is_surface(kind) {
                if let Some(s) = get_elem(d, c.model, source) {
                    if mark(d, s) == 0 {
                        flag_all_textures(d, env, s);
                        set_mark(d, s);
                    }
                }
            }
        }
        let name = d.node_of(elem.chap, elem.index).and_then(|n| n.name.clone());
        if !uv_is_fine(kind) {
            let mut m = ed::get_first_related_elem(d, father, true, 3, rel::MAT, c.mat);
            while let Some(me) = m.elem {
                let mut t = ed::get_first_related_elem(d, me, true, 3, rel::T2D_LOC, c.t2d);
                while !t.data.is_null() {
                    if test_uv_propagation(env, t.data.0) {
                        out.print_warn_uv(name.as_deref());
                    }
                    t = ed::get_next_related_elem(d, me, true, 3, rel::T2D_LOC, c.t2d, Some(t.cursor));
                }
                m = ed::get_next_related_elem(d, father, true, 3, rel::MAT, c.mat, Some(m.cursor));
            }
        }
        rdict::copy_material_rel(d, env, father, elem);
        if !ed::relation_check(d, elem, true, rel::T2D_GLB, c.t2d) && ed::relation_check(d, father, true, rel::T2D_GLB, c.t2d) {
            if !uv_is_fine(kind) {
                let mut t = ed::get_first_related_elem(d, father, true, 3, rel::T2D_GLB, c.t2d);
                while !t.data.is_null() {
                    if test_uv_propagation(env, t.data.0) {
                        out.print_warn_uv(name.as_deref());
                    }
                    t = ed::get_next_related_elem(d, father, true, 3, rel::T2D_GLB, c.t2d, Some(t.cursor));
                }
            }
            rdict::copy_texture2d_rel(d, env, father, elem);
        }
        if !ed::relation_check(d, elem, true, rel::T3D_GLB, c.t3d) && ed::relation_check(d, father, true, rel::T3D_GLB, c.t3d) {
            rdict::copy_texture3d_rel(d, env, father, elem);
        }
    }
    if d.slot(elem).unwrap().rec.work_id == -1 && is_surface(kind) {
        flag_all_textures(d, env, elem);
        set_mark(d, elem);
    }
    let mut son = ed::find_son(d, &env.p, elem);
    while let Some(s) = son {
        if d.slot(s).unwrap().rec.status == ELEM_VALID {
            propagate_materials(d, env, out, s, Some(elem), depth + 1);
        }
        son = ed::find_next_son(d, &env.p, elem);
    }
}

/// Gives every model element of a dictionary materials and textures
/// [soft 0x082c6fb0 SI_propagateMaterial]: the marks (+0x5b) of the chapters TEXTURES2D,
/// TEXTURES3D and MODELS are cleared; then every root model element in use (depth below 1)
/// that has no material is related to the default material, element `default_material` of
/// MATERIALS, and its hierarchy inherits downwards. Returns how many roots took the default.
pub fn propagate_material(d: &mut Dict, env: &mut Env, out: &mut dyn Outside, default_material: i32) -> usize {
    let c = env.p.cid;
    d.ed.clear_low_byte_work_flag(c.t2d);
    d.ed.clear_low_byte_work_flag(c.t3d);
    d.ed.clear_low_byte_work_flag(c.model);
    let n = d.ed.lists[c.model as usize].elems.len();
    let mut given = 0;
    for i in 0..n {
        let e = ElemId::new(c.model, i as i32);
        let rec = d.slot(e).unwrap().rec;
        if rec.hier_level < 1 && rec.status == ELEM_VALID {
            if !ed::relation_check(d, e, true, rel::MAT, c.mat) {
                elemrel::add_uncomplete_relation(d, &mut env.p, c.model, c.mat, i as i32, default_material, rel::MAT, 0, 0, Ptr32::NULL, 1);
                given += 1;
            }
            propagate_materials(d, env, out, e, None, 1);
        }
    }
    given
}
