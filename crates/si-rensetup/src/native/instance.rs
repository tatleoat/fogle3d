// SPDX-License-Identifier: MIT
//! `instance.c`, whole: instances and their masters, on the shared image.
//!
//! An instance is a model node whose algorithm is 15. It has no geometry; its element is
//! related to its master's by relation 20002 (list 4). This file has the look-ups of that
//! relation, the copy of a master's hierarchy "without geometry" that the renderers are
//! handed in place of an instance, the command that turns an instance into real models,
//! the command that gives a master's instances another master, and the list of material
//! arrays to free after a frame.
//!
//! The dictionary, the model library, the scene graph's list and everything interactive
//! are other packages: they are called through [`Outside`]. `SI_calloc (0x080d0f50)` and
//! `SI_malloc (0x080d0ee8)` are `si-mem`'s; the matrix routines are `si-math`'s; the
//! envelope and compensation copies are this package's ([`super::envtools`],
//! [`super::classic`]).

use super::classic::{classic_mdl_comp_info_dispose, classic_model_comp_info_copy};
use super::envtools::envelop_copy;
use super::globals::*;
use super::outside::*;
use super::rec::{event, flag, MemEntry, ALGO_INSTANCE, REL_INSTANCE};
use super::{current_dict, scene_graf_list};
use si_math::algebra::{matrix_extract_xyz_rotation, matrix_inverse, matrix_multiply_sub3, matrix_transpose};
use si_mem::{field, Mem, Opaque, Ptr32};
use si_types::dict::{Elem, ElemDict};
use si_types::model::Model;
use si_types::scene::GrafNode;
// SI_findGlData (0x0832ee68), `graflist.c`: `si-view`'s port, on the same image
use si_view::graflist::find_gl_data;
use si_x87::x87::F80;

type Node = Ptr32<Model>;

/// What one level of a recursive walk takes from the image's stack. The original's frames
/// are smaller (0x30 to 0x40 bytes) and its stack larger; a hierarchy that is its own
/// descendant ends it with a stack overflow after some hundred thousand levels. Here the
/// walk ends when the image's stack is used up (about 3,900 levels), with the fault
/// `Mem::local` records, so that the host's own stack is never at stake.
const FRAME: u32 = 0x200;

/// Takes a level's frame; false when the stack is used up.
fn enter(m: &mut Mem) -> bool {
    m.local(FRAME) != 0
}

fn leave(m: &mut Mem) {
    m.release(FRAME);
}

/// Three floats at an address as `si-math`'s vector, and back.
fn vector_at(m: &Mem, a: u32) -> si_math::types::Vector {
    si_math::types::Vector { x: m.f32(a), y: m.f32(a + 4), z: m.f32(a + 8) }
}

fn set_vector_at(m: &mut Mem, a: u32, v: si_math::types::Vector) {
    m.set_f32(a, v.x);
    m.set_f32(a + 4, v.y);
    m.set_f32(a + 8, v.z);
}

/// `SI_matrixExtractXyzRotation(matrix, &angles)` on the image: the angles at `angles` are
/// read as the previous ones and replaced (finding 0008). SI_matrixExtractXyzRotation
/// (0x085172b0) is `si-math`'s.
fn extract_xyz_rotation(m: &mut Mem, matrix: &si_math::types::Matrix, angles: u32) {
    let previous = vector_at(m, angles);
    let keep_turns = m[SI_GLB_MANIP_KEEP_TURNS] != 0;
    let v = matrix_extract_xyz_rotation(matrix, &previous, keep_turns);
    set_vector_at(m, angles, v);
}

// ---------------------------------------------------------------- the list of blocks

/// Enters a block in `memList` [soft 0x081a7310 addMemEntry]: nothing for a null block or
/// when the 8-byte entry cannot be had; else the entry goes to the head of the list.
pub fn add_mem_entry(m: &mut Mem, block: Ptr32<Opaque>) {
    if block.is_null() {
        return;
    }
    let entry: Ptr32<MemEntry> = Ptr32::new(m.si_calloc(1, 8));
    if entry.is_null() {
        return;
    }
    let head = m[MEM_LIST];
    m[entry].block = block;
    // the original stores null for an empty list and the head otherwise: the same word
    m[entry].next = head;
    m[MEM_LIST] = entry;
}

/// Frees every block of `memList` and the list itself, and switches the registration off
/// [soft 0x081a7354 SI_deleteMemReferences]. An empty list leaves the switch alone.
pub fn delete_mem_references(m: &mut Mem) {
    let mut entry = m[MEM_LIST];
    if entry.is_null() {
        return;
    }
    loop {
        let next = m[entry].next;
        let block = m[entry].block;
        if !block.is_null() {
            m.free(block.0);
            m[entry].block = Ptr32::NULL;
        }
        m.free(entry.0);
        entry = next;
        if entry.is_null() {
            break;
        }
    }
    m[MEM_LIST] = Ptr32::NULL;
    set_register_inst_mat_arrays_bool(m, 0);
}

/// The registration switch, sign-extended [soft 0x081a73b8 registerInstanceMatArrays].
pub fn register_instance_mat_arrays(m: &Mem) -> i32 {
    m[REG_INST_MAT_ARRAYS] as i32
}

/// Sets the registration switch [soft 0x081a73c8 SI_setRegisterInstMatArraysBool].
pub fn set_register_inst_mat_arrays_bool(m: &mut Mem, on: i16) {
    m[REG_INST_MAT_ARRAYS] = on;
}

// ---------------------------------------------------------------- the relation

/// Relates an instance to a master [soft 0x081a73d8 SI_instSetRelation].
///
/// Both models' elements are asked for, and whether the master has instances already,
/// before anything is tested. With both elements: relation 20002 from the instance's
/// element to the master's, and, when the master had no instance before, the schematic's
/// hierarchy operation 0x14 on the instance's element.
pub fn inst_set_relation<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, instance: Node, master: Node) {
    let sp = m.sp();
    let from_index = m.local(4);
    let to_index = m.local(4);
    let from = o.call(m, SI_ED_GET_ELEM_FROM_MODEL, &[instance.0, from_index]);
    let to = o.call(m, SI_ED_GET_ELEM_FROM_MODEL, &[master.0, to_index]);
    let was_master = model_is_master(o, m, master);
    if from != 0 && to != 0 {
        let dict = current_dict(m);
        let (a, b) = (m.u32(from_index), m.u32(to_index));
        o.call(m, SI_ELEMDICT_ADD_RELATION, &[dict.0, SI_RD.0, a, b, REL_INSTANCE, 1, 0]);
        if was_master as i16 == 0 {
            let dict = current_dict(m);
            let chapter = m[SI_CID].model as u32;
            o.call(m, SI_SCHEMATIC_HIER_OPERATION, &[0, dict.0, from, chapter, 0x14]);
        }
    }
    m.set_sp(sp);
}

/// The master of an instance in the current dictionary, or null
/// [soft 0x081a7490 SI_modelMasterFromInstance].
pub fn model_master_from_instance<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, model: Node) -> Node {
    let dict = current_dict(m);
    elemdict_model_master_from_instance(o, m, dict, model)
}

/// The master of an instance in a dictionary [soft 0x081a74c0 SI_elemdictModelMasterFromInstance]:
/// the object of the first element the model's element is related to by 20002 on the
/// forward side of list 4; null when the model has no element or no such relation.
pub fn elemdict_model_master_from_instance<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, dict: Ptr32<ElemDict>, model: Node) -> Node {
    let sp = m.sp();
    let index = m.local(4);
    let chapter = m[SI_CID].model as u32;
    let elem = o.call(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict.0, chapter, model.0, index]);
    let mut master = Node::NULL;
    if elem != 0 {
        let chapter = m[SI_CID].model as u32;
        // both output arguments are the same local
        let related: Ptr32<Elem> = Ptr32::new(o.call(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict.0, elem, 1, 4, REL_INSTANCE, chapter, index, index]));
        if !related.is_null() {
            master = m[related].data.cast();
        }
    }
    m.set_sp(sp);
    master
}

/// 1 when some instance names the model as its master, else 0
/// [soft 0x081a751c SI_modelIsMaster]: the model's element has a relation 20002 on the
/// backward side of list 4.
pub fn model_is_master<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, model: Node) -> i32 {
    let sp = m.sp();
    let index = m.local(4);
    let elem = o.call(m, SI_ED_GET_ELEM_FROM_MODEL, &[model.0, index]);
    let mut answer = 0;
    if elem != 0 {
        let chapter = m[SI_CID].model as u32;
        let dict = current_dict(m);
        let related = o.call(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict.0, elem, 0, 4, REL_INSTANCE, chapter, index, index]);
        answer = (related != 0) as i32;
    }
    m.set_sp(sp);
    answer
}

/// Gives a copied hierarchy the instance relations of the hierarchy it was copied from
/// [soft 0x081a7580 SI_copyInstanceRelation].
///
/// `from` and `to` are walked together. With `up` not null (below the top) and `to` having
/// a brother: when `from`'s brother is an instance with a master, `to`'s brother is related
/// to that master; then the brothers, with the same `up`. With `to` having a son: the same
/// for the sons, with `from` as `up`. At the top (`up` null), when `from` itself is an
/// instance with a master: `to` is related to it and becomes an instance (algorithm 15).
///
/// The source's brother or son is read wherever the copy has one; a source that lacks it
/// is read through a null pointer.
pub fn copy_instance_relation<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, from: Node, to: Node, up: Node) {
    if !enter(m) {
        return;
    }
    if !up.is_null() && !m[to].brother.is_null() {
        let b = m[from].brother;
        if m[b].algo_type == ALGO_INSTANCE {
            let master = model_master_from_instance(o, m, b);
            if !master.is_null() {
                let tb = m[to].brother;
                inst_set_relation(o, m, tb, master);
            }
        }
        let (fb, tb) = (m[from].brother, m[to].brother);
        copy_instance_relation(o, m, fb, tb, up);
    }
    if !m[to].son.is_null() {
        let s = m[from].son;
        if m[s].algo_type == ALGO_INSTANCE {
            let master = model_master_from_instance(o, m, s);
            if !master.is_null() {
                let ts = m[to].son;
                inst_set_relation(o, m, ts, master);
            }
        }
        let (fs, ts) = (m[from].son, m[to].son);
        copy_instance_relation(o, m, fs, ts, from);
    }
    if up.is_null() && m[from].algo_type == ALGO_INSTANCE {
        let master = model_master_from_instance(o, m, from);
        if !master.is_null() {
            inst_set_relation(o, m, to, master);
            m[to].algo_type = ALGO_INSTANCE;
        }
    }
    leave(m);
}

// ---------------------------------------------------------------- expanding an instance

/// A graf node made on the spot for a model that has none (only while a scene of the
/// newer program is converted): 0x24 bytes from `SI_malloc`, kind 2, the model, no
/// sub-models, no function, no neighbours. Selection, display type and colour are left as
/// the allocator handed them out. Part of `connectExpandFromInstance`, which has these
/// stores three times.
fn stand_in_graf_node(m: &mut Mem, model: Node) -> Ptr32<GrafNode> {
    let node: Ptr32<GrafNode> = Ptr32::new(m.si_malloc(0x24));
    if !node.is_null() {
        m[node].data = model.cast();
        m[node].kind = 2;
        m[node].nb_sub = 0;
        m[node].func = Ptr32::NULL;
        m[node].next = Ptr32::NULL;
        m[node].prev = Ptr32::NULL;
    }
    node
}

/// Puts the copy of a master in an instance's place in the scene
/// [soft 0x081a767c connectExpandFromInstance].
///
/// `master` is the instance's master, `copy` a full copy of it (`SI_modelCopy`). In order:
/// the copy's envelopes are rebuilt; a metaball system's groups are copied; the copy takes
/// the instance's global scaling, rotation (as angles, taken from the matrix with previous
/// angles of 0) and translation; the copy gets a graf node of kind 2; the instance
/// relations inside the master are given to the copy; the set-up and the texture
/// coordinate sets are copied from the master. When the instance has materials: a copy
/// that has materials gets the instance's current one transferred into its current one,
/// and a copy without (unless it is a metaball) gets a copy of it as its only material;
/// then the instance's textures. A lattice on the instance replaces the copy's. The
/// instance's animation is copied. The copy's graf node and its sub-nodes take the
/// instance's wire colour. The copy is linked under the instance's father, the instance's
/// sons are cut and linked under the copy, and the instance is deleted.
///
/// While a scene of the newer program is converted (`SI_edSumatraTo3x`) a model without
/// graf node gets one made on the spot for the duration of each link, the links are also
/// made in the dictionary, and the instance is cut instead of deleted (only when it has a
/// father).
pub fn connect_expand_from_instance<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, master: Node, instance: Node, copy: Node) {
    let sp = m.sp();
    let mut made = false;
    o.call(m, SI_ENVELOP_REBUILD, &[copy.0, 0, 0]);
    if m[master].kind == 7 {
        o.call(m, SI_META_GROUP_COPY, &[copy.0, master.0]);
    }
    // the text buffer SI_modelCut fills, and the three angles
    let text = m.local(0x100);
    let angles = m.local(12);
    m.set_u32(angles, 0);
    m.set_u32(angles + 4, 0);
    m.set_u32(angles + 8, 0);
    let grot = m[instance].grot;
    extract_xyz_rotation(m, &grot, angles);
    let gscl = field!(instance, Model, gscl);
    let gtrs = field!(instance, Model, gtrs);
    o.call(m, SI_MODEL_SET_STATE, &[copy.0, gscl.0, angles, gtrs.0]);
    let list = scene_graf_list(m);
    let mut old_gn = find_gl_data(m, list, instance.cast(), 2);
    let list = scene_graf_list(m);
    let new_gn: Ptr32<GrafNode> = Ptr32::new(o.call(m, SI_GRAFLIST_ADD_DATA, &[list.0, copy.0, 2, 1]));
    copy_instance_relation(o, m, master, copy, Node::NULL);
    o.call(m, SI_INIT_COPY_MULTI_UV, &[]);
    o.call(m, SI_MODEL_COPY_FROM_SETUP, &[master.0, copy.0]);
    o.call(m, SI_COPY_MULT_UV_HIERARCHY, &[master.0, copy.0]);

    if !m[instance].materials.is_null() {
        let mats = m[copy].materials;
        if !mats.is_null() {
            let mine = m[mats.add(m[copy].cur_material as i32)];
            o.call(m, SI_DELETE_MATERIAL_SHADERS, &[mine.0]);
            let theirs = m[m[instance].materials.add(m[instance].cur_material as i32)];
            o.call(m, SI_MATERIAL_TRANSFER, &[theirs.0, mine.0, 1]);
            o.call(m, SI_CONNECT_MATERIAL_SHADERS, &[mine.0]);
        } else if m[copy].kind != 8 {
            m[copy].materials = Ptr32::new(m.si_malloc(4));
            let theirs = m[m[instance].materials.add(m[instance].cur_material as i32)];
            let new = Ptr32::new(o.call(m, SI_MATERIAL_COPY, &[theirs.0]));
            let slot = m[copy].materials;
            m[slot] = new;
            m[copy].nb_materials = 1;
            m[copy].cur_material = 0;
            let first = m[m[copy].materials];
            let list = scene_graf_list(m);
            o.call(m, SI_GRAFLIST_ADD_DATA, &[list.0, first.0, 0xe, 0]);
            let first = m[m[copy].materials];
            o.call(m, SI_ED_MODEL_SET_MATERIAL, &[copy.0, first.0]);
            let theirs = m[m[instance].materials.add(m[instance].cur_material as i32)];
            let first = m[m[copy].materials];
            o.call(m, SI_COPY_MATERIAL_SHADERS, &[first.0, theirs.0]);
        }
        o.call(m, SI_TXT_COPY_FROM_SETUP, &[instance.0, copy.0, 1, 0]);
    }

    if o.call(m, SI_MODEL_TREE_LATTICE, &[instance.0]) != 0 {
        o.call(m, SI_MODEL_REMOVE_LATTICE, &[copy.0, 1, 1]);
        let saved = m[SI_GLB_DUPLIC_20];
        m[SI_GLB_DUPLIC_20] = 1;
        o.call(m, SI_LATTICE_COPY_FROM_SETUP, &[instance.0, copy.0]);
        m[SI_GLB_DUPLIC_20] = saved;
    }
    o.call(m, SI_MODEL_TRANSFO_ANIM_COPY, &[instance.0, copy.0]);

    if !old_gn.is_null() && !new_gn.is_null() {
        m[new_gn].wire_color = m[old_gn].wire_color;
        let mut sub = m[new_gn].next;
        if m[new_gn].nb_sub != 0 {
            let mut i = 0i32;
            loop {
                i += 1;
                m[sub].wire_color = m[new_gn].wire_color;
                sub = m[sub].next;
                if i >= m[new_gn].nb_sub as u16 as i32 {
                    break;
                }
            }
        }
    }

    let father = m[instance].father;
    if !father.is_null() {
        let list = scene_graf_list(m);
        let mut father_gn = find_gl_data(m, list, father.cast(), 2);
        if m[SI_ED_SUMATRA_TO_3X] != 0 && father_gn.is_null() {
            let f = m[instance].father;
            father_gn = stand_in_graf_node(m, f);
            if !father_gn.is_null() {
                made = true;
            }
        }
        o.call(m, SI_MODEL_CREATE_PARENT, &[father_gn.0, new_gn.0, 0]);
        if m[SI_ED_SUMATRA_TO_3X] != 0 {
            o.call(m, SI_ED_LINK_MODEL, &[father_gn.0, new_gn.0, 0]);
            if made {
                if !father_gn.is_null() {
                    m.free(father_gn.0);
                }
                made = false;
            }
        }
    }

    // as long as the instance has a son: the first one is cut and linked under the copy
    while !m[instance].son.is_null() {
        let son = m[instance].son;
        let list = scene_graf_list(m);
        let mut son_gn = find_gl_data(m, list, son.cast(), 2);
        if m[SI_ED_SUMATRA_TO_3X] != 0 && son_gn.is_null() {
            son_gn = stand_in_graf_node(m, son);
            if !son_gn.is_null() {
                made = true;
            }
        }
        o.call(m, SI_MODEL_CUT, &[son_gn.0, text]);
        o.call(m, SI_MODEL_CREATE_PARENT, &[new_gn.0, son_gn.0, 0]);
        if m[SI_ED_SUMATRA_TO_3X] != 0 {
            o.call(m, SI_ED_LINK_MODEL, &[new_gn.0, son_gn.0, 0]);
            if made {
                if !son_gn.is_null() {
                    m.free(son_gn.0);
                }
                made = false;
            }
        }
    }

    o.call(m, SI_SCHEMATIC_NO_REFRESH, &[1]);
    if m[SI_ED_SUMATRA_TO_3X] == 0 {
        let list = scene_graf_list(m);
        o.call(m, SI_GRAFLIST_DELETE_SELECTION, &[list.0, old_gn.0]);
    } else if !m[instance].father.is_null() {
        old_gn = stand_in_graf_node(m, instance);
        o.call(m, SI_MODEL_CUT, &[old_gn.0, text]);
        let list = scene_graf_list(m);
        o.call(m, SI_GRAFLIST_DELETE_SELECTION, &[list.0, old_gn.0]);
        if !old_gn.is_null() {
            m.free(old_gn.0);
        }
    }
    o.call(m, SI_SCHEMATIC_NO_REFRESH, &[0]);
    if m[SI_ED_SUMATRA_TO_3X] == 0 {
        o.call(m, SI_DYNAMIC_CHAINS_UPDATE, &[]);
    }
    m.set_sp(sp);
}

/// The command "expand instance" [soft 0x081a7c14 SI_expandInstance]: every selected graf
/// node of kind 2 or 3 whose model is an instance is expanded (`deep`: instances inside
/// the copies too). The status line says `Expanding, please wait...` first; then either
/// everything is redrawn or, when no instance was selected, the status line says so.
pub fn expand_instance<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, deep: i16) {
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[EXPANDING, 0]);
    let mut found = false;
    let mut gn = m[scene_graf_list(m)].first;
    while !gn.is_null() {
        if m[gn].selection != 0 && (m[gn].kind as u32).wrapping_sub(2) <= 1 {
            let model: Node = m[gn].data.cast();
            if !model.is_null() && m[model].algo_type == ALGO_INSTANCE {
                found = true;
                model_expand_instance(o, m, model, deep);
            }
        }
        gn = m[gn].next;
    }
    if found {
        o.call(m, SI_REFRESH_ALL, &[1]);
    } else {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[NEED_INSTANCE, 2]);
    }
}

/// What `SI_modelExpandInstance` does for one instance: copy the master, turn the copy
/// inside out when the instance is (flag bit 8), connect it.
fn expand_one<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, instance: Node, master: Node) -> Node {
    let copy: Node = Ptr32::new(o.call(m, SI_MODEL_COPY, &[master.0, 0]));
    if m[instance].flags_08 & flag::INVERSE != 0 {
        o.call(m, SI_MODEL_INVERSE, &[copy.0]);
        m[copy].flags_08 &= !flag::INVERSE;
    }
    copy
}

/// The second half of the same: while a scene of the newer program is converted, the
/// instance is marked and its element's relations of both sides are removed.
fn forget_instance<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, instance: Node, index: u32) {
    if m[SI_ED_SUMATRA_TO_3X] != 0 {
        m[instance].flags_08 |= flag::VISITED;
        let elem = o.call(m, SI_ED_GET_ELEM_FROM_MODEL, &[instance.0, index]);
        for side in [1, 0] {
            let (i, chapter, dict) = (m.u32(index), m[SI_CID].model as u32, current_dict(m));
            o.call(m, SI_REL_REMOVE_RELATIONS, &[dict.0, elem, chapter, i, side]);
        }
    }
}

/// Replaces one instance by a copy of its master [soft 0x081a7cb8 SI_modelExpandInstance].
///
/// Nothing without a master. The master is copied (`SI_modelCopy`), turned inside out when
/// the instance is, and connected in the instance's place. With `deep`, every instance
/// found in the copy's hierarchy is then expanded the same way (the search starts again
/// from the top each time; when the top itself was the instance, its copy becomes the
/// top). At the end the first copy's dependencies are updated when it, or a node under
/// it, has a lattice.
pub fn model_expand_instance<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, instance: Node, deep: i16) {
    let master = model_master_from_instance(o, m, instance);
    if master.is_null() {
        return;
    }
    let sp = m.sp();
    let index_a = m.local(4);
    let index_b = m.local(4);
    let lattice = m.local(2);
    let first = expand_one(o, m, instance, master);
    // without `deep` the top is never read
    let mut top = first;
    connect_expand_from_instance(o, m, master, instance, first);
    forget_instance(o, m, instance, index_a);
    if deep != 0 {
        loop {
            let found: Node = find_instance_in_hierarchy(m, top, Node::NULL, 1);
            if found.is_null() {
                break;
            }
            let master = model_master_from_instance(o, m, found);
            let copy = expand_one(o, m, found, master);
            if top == found {
                top = copy;
            }
            connect_expand_from_instance(o, m, master, found, copy);
            forget_instance(o, m, found, index_b);
        }
    }
    let mut update = o.call(m, SI_MODEL_LATTICE, &[first.0, lattice]) != 0;
    if !update {
        let mut son = o.call(m, SI_MODEL_NEXT_SON, &[first.0, 0]);
        while son != 0 {
            if o.call(m, SI_MODEL_LATTICE, &[son, lattice]) != 0 {
                update = true;
                break;
            }
            son = o.call(m, SI_MODEL_NEXT_SON, &[first.0, son]);
        }
    }
    if update {
        o.call(m, SI_MODEL_UPDATE_DEPENDENCY, &[first.0, 0]);
    }
    m.set_sp(sp);
}

/// The first instance of a hierarchy, depth first [soft 0x081a7f1c SI_findInstanceInHierarchy].
///
/// A node that is an instance is the answer, unless `any` is 0 and the node is marked as
/// visited (flag bit 0x20). Otherwise the node's brothers are searched when `brothers` is
/// not null (the caller passes null for the top node; inside, the father is passed), and
/// then its sons.
pub fn find_instance_in_hierarchy(m: &mut Mem, node: Node, brothers: Node, any: i16) -> Node {
    if !enter(m) {
        return Node::NULL;
    }
    let mut found = Node::NULL;
    if m[node].algo_type == ALGO_INSTANCE && (any != 0 || m[node].flags_08 & flag::VISITED == 0) {
        found = node;
    } else {
        let brother = m[node].brother;
        if !brothers.is_null() && !brother.is_null() {
            found = find_instance_in_hierarchy(m, brother, brothers, any);
        }
        if found.is_null() {
            let son = m[node].son;
            if !son.is_null() {
                found = find_instance_in_hierarchy(m, son, node, any);
            }
        }
    }
    leave(m);
    found
}

/// Gives every instance of a master another master [soft 0x081a7f94 SI_edSwapMaster].
///
/// The master's element is asked how many relations 20002 point at it (backward side,
/// list 4). Two arrays of that many words are made (`SI_calloc`): the related elements and
/// the relations' numbers, filled by walking the relations; an element that repeats the
/// one before it is passed over. Then, for each entry until a null element or the count:
/// the relation is removed by its number and the element's model is related to the new
/// master. Both arrays are freed.
pub fn ed_swap_master<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, master: Node, new_master: Node) {
    let sp = m.sp();
    let index = m.local(4);
    let number = m.local(4);
    let other = m.local(4);
    let elem = o.call(m, SI_ED_GET_ELEM_FROM_MODEL, &[master.0, index]);
    let (chapter, dict) = (m[SI_CID].model as u32, current_dict(m));
    let count = o.call(m, SI_ELEMDICT_RELATIONS_COUNT, &[dict.0, elem, 0, 4, 4, REL_INSTANCE, chapter]) as i32;
    if count > 0 {
        let elems: Ptr32<Ptr32<Elem>> = Ptr32::new(m.si_calloc(count as u32, 4));
        let numbers: Ptr32<u32> = Ptr32::new(m.si_calloc(count as u32, 4));
        if !elems.is_null() && !numbers.is_null() {
            let (chapter, dict) = (m[SI_CID].model as u32, current_dict(m));
            let mut related: Ptr32<Elem> = Ptr32::new(o.call(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict.0, elem, 0, 4, REL_INSTANCE, chapter, other, number]));
            m[elems] = related;
            m[numbers] = m.u32(number);
            let mut n = 1;
            while !related.is_null() && n < count {
                if related != m[elems.add(n - 1)] {
                    m[elems.add(n)] = related;
                    m[numbers.add(n)] = m.u32(number);
                    n += 1;
                }
                let (chapter, dict) = (m[SI_CID].model as u32, current_dict(m));
                related = Ptr32::new(o.call(m, SI_ELEMDICT_FIND_NEXT_RELATED_ELEM, &[dict.0, elem, 0, 4, REL_INSTANCE, chapter, other, number]));
            }
            let mut i = 0;
            if !m[elems].is_null() && i < count {
                loop {
                    let e = m[elems.add(i)];
                    let num = m[numbers.add(i)];
                    m.set_u32(number, num);
                    let dict = current_dict(m);
                    o.call(m, SI_ELEMDICT_REMOVE_RELATION_ID, &[dict.0, SI_RD.0, num, REL_INSTANCE]);
                    let instance: Node = m[e].data.cast();
                    inst_set_relation(o, m, instance, new_master);
                    i += 1;
                    // the entry is read before the count is tested: one word past the
                    // array after the last entry, whose value then does not matter
                    if m[elems.add(i)].is_null() || i >= count {
                        break;
                    }
                }
            }
            m.free(elems.0);
            m.free(numbers.0);
        }
    }
    m.set_sp(sp);
}

/// The command "swap master" [soft 0x081a8160 SI_swapMaster].
///
/// In multi-selection mode a notice says the command needs single selection. Otherwise
/// the active graf node must be a model (kind 2 or 3) that is a master (else the status
/// line says so for a second). The mouse line asks for the new master and events are read
/// until one settles it: a button with code 0x65 cancels; 0x66 or 0x67 picks the model
/// under the pointer, which must not lead to a cycle (a dialogue says so), must be a root
/// and must not be an instance (status line, one second); an event that is neither a
/// button nor "nothing" sets `SI_gotcha` and ends the command at once. After a swap, or
/// when there was no active model, everything is redrawn.
pub fn swap_master<O: Outside + ?Sized>(o: &mut O, m: &mut Mem) {
    let list = scene_graf_list(m);
    if m[list].multi != 0 {
        o.call(m, SI_SYSTEM_NOTICE, &[SWAP_SINGLE_1, SWAP_SINGLE_2, EMPTY_868E3FF]);
        return;
    }
    let active = m[list].active;
    if !active.is_null() && (m[active].kind as u32).wrapping_sub(2) <= 1 {
        let master: Node = m[active].data.cast();
        // a refusal: the status line for a second, then the module's own messages again
        let refuse = |o: &mut O, m: &mut Mem, text: u32| {
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[text, 2]);
            o.call(m, SLEEP, &[1]);
            o.call(m, SI_MDL_STATUS_MESSAGES, &[]);
        };
        if model_is_master(o, m, master) as i16 == 0 {
            refuse(o, m, NEED_MASTER);
            return;
        }
        o.call(m, SI_DISPLAY_MOUSE, &[PICK_MASTER_1, PICK_MASTER_2, CANCEL]);
        let mut picked = Node::NULL;
        loop {
            let ev = o.call(m, SI_GET_EVENT, &[]);
            let kind = m.i32(ev + event::KIND);
            if kind == 5 {
                let window = m.u32(ev + event::WINDOW);
                let button = m.i16(ev + event::BUTTON);
                if button == 0x65 {
                    o.call(m, SI_MDL_STATUS_MESSAGES, &[]);
                    return;
                }
                if button > 0x65 && button <= 0x67 {
                    let gn: Ptr32<GrafNode> = Ptr32::new(o.call(m, SI_GL_SELECT, &[window]));
                    if !gn.is_null() && (m[gn].kind as u32).wrapping_sub(2) <= 1 {
                        picked = m[gn].data.cast();
                        if instance_check_cycle(o, m, master, picked, Node::NULL) as i16 != 0 {
                            o.call(m, SI_DIALOG_REQUEST, &[CYCLE_1, CYCLE_2, CYCLE_3, OK]);
                            o.call(m, SI_MDL_STATUS_MESSAGES, &[]);
                            return;
                        }
                        if !m[picked].father.is_null() {
                            refuse(o, m, NEED_ROOT);
                            return;
                        }
                        if m[picked].algo_type == ALGO_INSTANCE {
                            refuse(o, m, NOT_AN_INSTANCE);
                            return;
                        }
                        ed_swap_master(o, m, master, picked);
                    }
                }
            } else if kind != 0 {
                m[SI_GOTCHA] = 1;
                return;
            }
            if !picked.is_null() {
                break;
            }
        }
    }
    o.call(m, SI_REFRESH_ALL, &[1]);
    o.call(m, SI_REFRESH_COUNTER, &[]);
    o.call(m, SI_MDL_STATUS_MESSAGES, &[]);
}

// ---------------------------------------------------------------- what a copy shares

/// Gives a node an array of its own with another node's material pointers
/// [soft 0x081a8310 SI_instanceMaterialShare].
///
/// Nothing when the source has no material. The count and the current material number are
/// copied; the array is `SI_calloc(count, 4)` (the count sign-extended) and is entered in
/// `memList` while the registration switch is on; the pointers are copied when the count
/// is positive. The materials themselves are shared.
pub fn instance_material_share(m: &mut Mem, from: Node, to: Node) {
    if m[from].nb_materials == 0 {
        return;
    }
    m[to].nb_materials = m[from].nb_materials;
    m[to].cur_material = m[from].cur_material;
    let count = m[from].nb_materials as i32 as u32;
    m[to].materials = Ptr32::new(m.si_calloc(count, 4));
    if register_instance_mat_arrays(m) as i16 != 0 {
        let array = m[to].materials;
        add_mem_entry(m, array.cast());
    }
    let mut i = 0;
    if m[from].nb_materials > 0 {
        loop {
            let material = m[m[from].materials.add(i)];
            let slot = m[to].materials.add(i);
            m[slot] = material;
            i += 1;
            if i >= m[from].nb_materials as i32 {
                break;
            }
        }
    }
}

/// Gives a node another node's textures [soft 0x081a83a8 SI_instanceTextureShare]: when
/// the source has a 2D texture list or a 3D texture, the list's head (+0x21c), the pointer
/// beside it (+0x218) and the count (+0x216) are copied, and the 3D texture when the
/// source has one (the destination's is kept otherwise).
pub fn instance_texture_share(m: &mut Mem, from: Node, to: Node) {
    let textures = m[from].textures;
    if textures.is_null() && m[from].texture3d.is_null() {
        return;
    }
    m[to].textures = textures;
    m[to].texture_218 = m[from].texture_218;
    m[to].unk_216 = m[from].unk_216;
    let t3 = m[from].texture3d;
    if !t3.is_null() {
        m[to].texture3d = t3;
    }
}

/// Makes a model, and the nodes under it, depend on another model's lattice
/// [soft 0x081a840c SI_instanceSetMasterDependency]; answers the model the node depended
/// on before, or null when either argument is null. Nothing is changed when the
/// dependency is the same already; a node under the model that depends on itself keeps
/// doing so.
pub fn instance_set_master_dependency<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, model: Node, dependency: Node) -> Node {
    if model.is_null() || dependency.is_null() {
        return Node::NULL;
    }
    let old = m[model].latdep;
    if dependency != old {
        m[model].latdep = dependency;
        let mut son: Node = Ptr32::new(o.call(m, SI_MODEL_NEXT_SON, &[model.0, 0]));
        while !son.is_null() {
            if m[son].latdep != son {
                m[son].latdep = dependency;
            }
            son = Ptr32::new(o.call(m, SI_MODEL_NEXT_SON, &[model.0, son.0]));
        }
    }
    old
}

// ---------------------------------------------------------------- the copy without geometry

/// A new node that stands for a master's node without owning any geometry
/// [soft 0x081a8460 instanceCopyNodeFromMaster].
///
/// The node comes from `SI_modelAllocate` with the master's name and gets flag bit 0x40.
/// Copied: kind, the bounding box and its "valid" word, the number of definitions, the
/// element number, local scaling, rotation angles and translation, the visibility and
/// motion blur words, the caustic and global illumination words, the wire-frame display
/// word; in "classic" manipulation mode the compensation records; **the pointers** to the
/// definitions, the current definition and the original definition (shared, not copied);
/// the algorithm (`SI_modelAlgoTypeCopy`); the envelope word, unless it is 0 or 3, or, for
/// the values 5 and 6 only, the master's graf node is selected as a node (1) or `keep` is
/// set; a copy of the inverse kinematics record; an array of its own with the master's
/// materials; the master's textures. Null when no node can be had.
pub fn instance_copy_node_from_master<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, master: Node, keep: i16) -> Node {
    let mut gn: Ptr32<GrafNode> = Ptr32::NULL;
    let name = m[master].name;
    let node: Node = Ptr32::new(o.call(m, SI_MODEL_ALLOCATE, &[name.0]));
    if node.is_null() {
        return node;
    }
    m[node].flags_08 |= flag::COPY;
    m[node].kind = m[master].kind;
    m[node].bbox_valid = m[master].bbox_valid;
    m[node].nb_defs = m[master].nb_defs;
    m[node].elem_id = m[master].elem_id;
    m[node].bbox_min = m[master].bbox_min;
    m[node].bbox_max = m[master].bbox_max;
    m[node].scal = m[master].scal;
    m[node].vrot = m[master].vrot;
    m[node].trans = m[master].trans;
    m[node].visibility = m[master].visibility;
    m[node].motion_blur = m[master].motion_blur;
    m[node].caustic = m[master].caustic;
    m[node].global_illum = m[master].global_illum;
    m[node].wf_display = m[master].wf_display;
    if m[SI_GLB_MANIP] == 1 {
        classic_model_comp_info_copy(m, master, node);
    }
    if (m[master].envelop as u32).wrapping_sub(5) <= 1 {
        let list = scene_graf_list(m);
        gn = find_gl_data(m, list, master.cast(), 0);
    }
    m[node].defs = m[master].defs;
    m[node].definition = m[master].definition;
    m[node].org_def = m[master].org_def;
    o.call(m, SI_MODEL_ALGO_TYPE_COPY, &[master.0, node.0]);
    let envelop = m[master].envelop;
    if envelop != 0 && envelop != 3 && (gn.is_null() || (m[gn].selection != 1 && keep == 0)) {
        m[node].envelop = envelop;
    }
    m[node].unk_320 = Ptr32::NULL;
    let ik = m[master].inv_kin;
    m[node].inv_kin = Ptr32::new(o.call(m, SI_INV_KIN_COPY, &[ik.0]));
    instance_material_share(m, master, node);
    instance_texture_share(m, master, node);
    node
}

/// Whether the hierarchy copy handed to the native renderer is asked to compute `visible`.
#[derive(Clone, Copy)]
enum Flavour {
    /// `SI_modelExpandAllWithoutGeometry`: the copy's `visible` word, its envelope.
    Native { visible: i16 },
    /// `SI_modelExpandAllWithoutGeometryAbsolute`: no `visible`, envelope words cleared.
    Absolute,
}

/// The `visible` argument for the copy of the master of the instance `node`: the caller's
/// own, cut to 0 or 1, when the instance's graf node is not hidden, else 0. A node without
/// graf node is read through a null pointer.
fn instance_visible(m: &Mem, node: Node, visible: i16) -> i16 {
    let gn = find_gl_data(m, scene_graf_list(m), node.cast(), 0);
    if m[gn].wire_type != 0 { (visible != 0) as i16 } else { 0 }
}

/// The copy of the master of the instance `node` and what it takes from the instance: the
/// instance's element number, material array and textures, the father `father`, and the
/// instance's transformation. Three places of each hierarchy copy have these steps.
fn copy_for_instance<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, fl: Flavour, node: Node, master: Node, deep: i16, father: impl FnOnce(&Mem) -> Node) -> Node {
    let e = match fl {
        Flavour::Native { visible } => {
            let v = instance_visible(m, node, visible);
            expand_all(o, m, Flavour::Native { visible: v }, master, Node::NULL, deep)
        }
        Flavour::Absolute => expand_all(o, m, fl, master, Node::NULL, deep),
    };
    m[e].elem_id = m[node].elem_id;
    instance_material_share(m, node, e);
    instance_texture_share(m, node, e);
    m[e].father = father(m);
    instance_copy_transformation(o, m, node, e);
    e
}

/// Hands the sons `kids` of a node that is about to be disposed of to `e`: they go behind
/// `e`'s own sons, the old father's son link is cleared and each takes `e` as father.
fn take_sons(m: &mut Mem, e: Node, kids: Node) {
    let mut last = m[e].son;
    if last.is_null() {
        m[e].son = kids;
    } else {
        while !m[last].brother.is_null() {
            last = m[last].brother;
        }
        m[last].brother = kids;
    }
    if !kids.is_null() {
        let old_father = m[kids].father;
        m[old_father].son = Node::NULL;
        let mut k = kids;
        loop {
            m[k].father = e;
            k = m[k].brother;
            if k.is_null() {
                break;
            }
        }
    }
}

/// The body both hierarchy copies share (the two original routines differ only in what
/// [`Flavour`] says).
///
/// `src` is copied as a node ([`instance_copy_node_from_master`]) with `father` as father;
/// a node with a father whose master node does not depend on itself takes the father's
/// lattice dependency. With a father, the brothers are copied (same father); without one
/// they are not. Then the sons, with the copy as father. A brother or son marked
/// "expanding" (flag bit 0x200) ends the walk in that direction: for a brother the sons
/// are still done, for a son the copy is returned as it is.
///
/// With `deep`, a brother or son that is an instance is marked "expanding" while its
/// master's hierarchy is copied (as a root, recursively); that copy takes the instance's
/// element number, materials, textures and transformation, the place of the node copied
/// for the instance, and that node's sons; the node is disposed of. The same is done for
/// `src` itself when it is an instance and has no father to copy under, in which case the
/// master's copy is what is returned.
fn expand_all<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, fl: Flavour, src: Node, father: Node, deep: i16) -> Node {
    if !enter(m) {
        return Node::NULL;
    }
    // the copy is kept in a local because its address is passed to the disposal at the end
    let slot: Ptr32<Node> = m.local_record(Node::NULL);
    let copy = instance_copy_node_from_master(o, m, src, 0);
    m[slot] = copy;
    let recurse = |o: &mut O, m: &mut Mem, node: Node, father: Node| expand_all(o, m, fl, node, father, deep);
    'walk: {
        if copy.is_null() {
            break 'walk;
        }
        m[copy].father = father;
        if !father.is_null() && src != m[src].latdep {
            m[copy].latdep = m[father].latdep;
        }
        match fl {
            Flavour::Native { visible } => {
                let gn = find_gl_data(m, scene_graf_list(m), src.cast(), 0);
                // without `visible` the graf node is not read
                let v = if visible != 0 { (m[gn].wire_type != 0) as i16 } else { 0 };
                m[copy].visible = v;
                envelop_copy(m, src, father, copy);
            }
            Flavour::Absolute => {
                m[copy].envelop = 0;
                m[copy].env_skel_jnt = Ptr32::NULL;
            }
        }
        let brother = m[src].brother;
        if !father.is_null() && !brother.is_null() && m[brother].flags_08 & flag::EXPANDING == 0 {
            m[copy].brother = recurse(o, m, brother, father);
            let brother = m[src].brother;
            if deep != 0 && m[brother].algo_type == ALGO_INSTANCE {
                m[brother].flags_08 |= flag::EXPANDING;
                let brother = m[src].brother;
                let master = model_master_from_instance(o, m, brother);
                if !master.is_null() {
                    let brother = m[src].brother;
                    let e = copy_for_instance(o, m, fl, brother, master, deep, |m| m[copy].father);
                    let stand_in = m[copy].brother;
                    m[e].brother = m[stand_in].brother;
                    let kids = m[m[copy].brother].son;
                    take_sons(m, e, kids);
                    instance_dispose_node_without_geometry(o, m, field!(copy, Model, brother));
                    m[copy].brother = e;
                }
                let brother = m[src].brother;
                m[brother].flags_08 &= !flag::EXPANDING;
            }
        }
        let son = m[src].son;
        if son.is_null() {
            break 'walk;
        }
        if m[son].flags_08 & flag::EXPANDING != 0 {
            leave_frame(m, slot);
            return copy;
        }
        m[copy].son = recurse(o, m, son, copy);
        if deep == 0 {
            leave_frame(m, slot);
            return copy;
        }
        let son = m[src].son;
        if m[son].algo_type == ALGO_INSTANCE {
            m[son].flags_08 |= flag::EXPANDING;
            let son = m[src].son;
            let master = model_master_from_instance(o, m, son);
            if !master.is_null() {
                let son = m[src].son;
                let e = copy_for_instance(o, m, fl, son, master, deep, |_| copy);
                let stand_in = m[copy].son;
                m[e].brother = m[stand_in].brother;
                let kids = m[m[copy].son].son;
                take_sons(m, e, kids);
                instance_dispose_node_without_geometry(o, m, field!(copy, Model, son));
                m[copy].son = e;
            }
            let son = m[src].son;
            m[son].flags_08 &= !flag::EXPANDING;
        }
    }
    // the node itself: an instance copied as a root is replaced by its master's copy
    if deep != 0 && father.is_null() && m[src].algo_type == ALGO_INSTANCE {
        let master = model_master_from_instance(o, m, src);
        if !master.is_null() {
            // a node that could not be copied is read through a null pointer here
            let e = copy_for_instance(o, m, fl, src, master, deep, |m| m[copy].father);
            m[e].brother = m[copy].brother;
            let kids = m[copy].son;
            take_sons(m, e, kids);
            instance_dispose_node_without_geometry(o, m, slot);
            m[slot] = e;
        }
    }
    let answer = m[slot];
    leave_frame(m, slot);
    answer
}

/// Gives back the local of [`expand_all`] and its level's frame.
fn leave_frame(m: &mut Mem, _slot: Ptr32<Node>) {
    m.release(4);
    leave(m);
}

/// The copy of a master's hierarchy that the native renderer is handed in place of an
/// instance [soft 0x081a865c SI_modelExpandAllWithoutGeometry]: see [`expand_all`]. Each
/// copied node's `visible` word is 0 unless `visible` is set, in which case it is whether
/// the master node's graf node is not hidden; each node's envelope is copied
/// (`SI_envelopCopy`). With `deep`, instances inside the master are expanded too, each
/// with `visible` cut by its own graf node's display type.
pub fn model_expand_all_without_geometry<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, master: Node, father: Node, visible: i16, deep: i16) -> Node {
    expand_all(o, m, Flavour::Native { visible }, master, father, deep)
}

/// The same copy for the render dictionary
/// [soft 0x081a8be0 SI_modelExpandAllWithoutGeometryAbsolute]: no `visible` word is set,
/// the envelope word (+0x31c) and the envelope record's pointer (+0x324) of every copied
/// node are cleared, and no graf node is asked for.
pub fn model_expand_all_without_geometry_absolute<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, master: Node, father: Node, deep: i16) -> Node {
    expand_all(o, m, Flavour::Absolute, master, father, deep)
}

/// Disposes of a copied hierarchy [soft 0x081a9080 SI_instanceDispose]: the node `*node`
/// with, when `brothers` is set, its brothers, and always its sons (with their brothers).
/// The node is taken out of its father's list of sons first: a first son clears the
/// father's son link (the brothers were disposed of just before or are left without
/// father's link), any other is unlinked from the brother before it. `*node` is cleared.
pub fn instance_dispose<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, node: Ptr32<Node>, brothers: i16) {
    if !enter(m) {
        return;
    }
    let slot: Ptr32<Node> = m.local_record(m[node]);
    let n = m[slot];
    if !n.is_null() {
        if brothers != 0 && !m[n].brother.is_null() {
            instance_dispose(o, m, field!(n, Model, brother), 1);
        }
        if !m[n].son.is_null() {
            instance_dispose(o, m, field!(n, Model, son), 1);
        }
        let father = m[n].father;
        if !father.is_null() {
            let first = m[father].son;
            if n == first {
                m[father].son = Node::NULL;
            } else {
                let mut before = first;
                loop {
                    let next = m[before].brother;
                    if next == n {
                        m[before].brother = m[n].brother;
                        break;
                    }
                    if before.is_null() {
                        // the node is not among its father's sons: the original reads on
                        // through the null pointer and dies; here that read is the
                        // recorded fault and the walk ends
                        break;
                    }
                    before = next;
                }
            }
        }
        instance_dispose_node_without_geometry(o, m, slot);
        m[node] = Node::NULL;
    }
    m.release(4);
    leave(m);
}

/// Frees one copied node and what it alone owns
/// [soft 0x081a9148 instanceDisposeNodeWithoutGeometry]: its name, the block at +0x00, the
/// extra texture coordinates (+0x330), the render vertices (+0x22c), the block at +0x42c,
/// the render triangles (+0x234), each pointer cleared after its block is freed; the
/// compensation records; the inverse kinematics record when there is one; the node, and
/// the caller's pointer to it is cleared. Not freed: the material array (that is what
/// `memList` is for) and the geometry, which is the master's.
pub fn instance_dispose_node_without_geometry<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, node: Ptr32<Node>) {
    let n = m[node];
    if n.is_null() {
        return;
    }
    macro_rules! release {
        ($field:ident) => {
            let block = m[n].$field;
            if !block.is_null() {
                m.free(block.0);
                m[n].$field = Ptr32::NULL;
            }
        };
    }
    release!(name);
    release!(block_00);
    release!(extra_tcoords);
    release!(ren_vertices);
    release!(unk_42c);
    release!(ren_triangles);
    classic_mdl_comp_info_dispose(m, field!(n, Model, comp_info).cast());
    if !m[n].inv_kin.is_null() {
        o.call(m, SI_INV_KIN_DISPOSE, &[field!(n, Model, inv_kin).0]);
    }
    m.free(n.0);
    m[node] = Node::NULL;
}

/// Clears the marks "visited" and "expanding" (flag bits 0x20 and 0x200) of every instance
/// the scene's graf list holds as a model [soft 0x081a9270 SI_instanceInitializeVisit].
pub fn instance_initialize_visit(m: &mut Mem) {
    let mut gn = m[scene_graf_list(m)].first;
    while !gn.is_null() {
        if (m[gn].kind as u32).wrapping_sub(2) <= 1 {
            let model: Node = m[gn].data.cast();
            if m[model].algo_type == ALGO_INSTANCE {
                m[model].flags_08 &= !(flag::VISITED | flag::EXPANDING);
            }
        }
        gn = m[gn].next;
    }
}

/// Gives the copy of a master the place in space of the instance it stands for
/// [soft 0x081a92bc SI_instanceCopyTransformation].
///
/// Without a father the copy's local transformation is the instance's global one: the
/// scaling and translation as they are, the angles taken from the global rotation matrix
/// (with previous angles of 0). With a father it is the instance's global transformation
/// in the father's space: each scaling divided by the father's global scaling; the
/// rotation matrix = the instance's global rotation × the transpose of the father's, and
/// the angles taken from it; the translation = the instance's global translation through
/// the inverse of the father's global matrix. Then the node's matrices are rebuilt
/// (`SI_modelUpdateSonState(father, copy, 0)`).
///
/// Every quotient and sum is formed in the 80-bit register and rounded once, on the store
/// to the float. When the father's matrix has no inverse the matrix used is what the
/// local buffer still holds: the transpose of the father's rotation.
pub fn instance_copy_transformation<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, instance: Node, copy: Node) {
    let father = m[copy].father;
    if father.is_null() {
        m[copy].scal.x = m[instance].gscl.x;
        m[copy].scal.y = m[instance].gscl.y;
        let z = m[instance].gscl.z;
        let vrot = field!(copy, Model, vrot).0;
        m.set_u32(vrot + 8, 0);
        m.set_u32(vrot + 4, 0);
        m.set_u32(vrot, 0);
        m[copy].scal.z = z;
        let grot = m[instance].grot;
        extract_xyz_rotation(m, &grot, vrot);
        m[copy].trans = m[instance].gtrs;
    } else {
        let (s, f) = (m[instance].gscl, m[father].gscl);
        m[copy].scal.x = (F80::from_f32(s.x) / f.x).to_f32();
        m[copy].scal.y = (F80::from_f32(s.y) / f.y).to_f32();
        m[copy].scal.z = (F80::from_f32(s.z) / f.z).to_f32();
        let mut t = matrix_transpose(&m[father].grot);
        let mrot = matrix_multiply_sub3(&m[instance].grot, &t);
        m[copy].mrot = mrot;
        let vrot = field!(copy, Model, vrot).0;
        m.set_u32(vrot + 8, 0);
        m.set_u32(vrot + 4, 0);
        m.set_u32(vrot, 0);
        let mrot = m[copy].mrot;
        extract_xyz_rotation(m, &mrot, vrot);
        // the inverse goes to the buffer that holds the transpose; a failure leaves it
        if let Some(inverse) = matrix_inverse(&m[father].gmat) {
            t = inverse;
        }
        let g = m[instance].gtrs;
        let (x, y, z) = (F80::from_f32(g.x), F80::from_f32(g.y), F80::from_f32(g.z));
        let column = |j: usize| (((x * t[0][j] + y * t[1][j]) + z * t[2][j]) + t[3][j]).to_f32();
        m[copy].trans.x = column(0);
        m[copy].trans.y = column(1);
        m[copy].trans.z = column(2);
    }
    o.call(m, SI_MODEL_UPDATE_SON_STATE, &[father.0, copy.0, 0]);
}

/// What the root of a master's copy takes from the instance when the native renderer's
/// callers make it [soft 0x081a94a8 SI_expandCopyInfoFromInstance]: the element number,
/// an array of its own with the instance's materials, the instance's textures, and as
/// local transformation the instance's global one (scaling, the angles of the global
/// rotation matrix with previous angles of 0, translation: each float through the unit);
/// then the node's matrices are rebuilt as a root's (`SI_modelUpdateSonState(null, copy, 1)`).
pub fn expand_copy_info_from_instance<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, instance: Node, copy: Node) {
    m[copy].elem_id = m[instance].elem_id;
    instance_material_share(m, instance, copy);
    instance_texture_share(m, instance, copy);
    let sp = m.sp();
    let angles = m.local(12);
    m.set_u32(angles + 8, 0);
    m.set_u32(angles + 4, 0);
    m.set_u32(angles, 0);
    let grot = m[instance].grot;
    extract_xyz_rotation(m, &grot, angles);
    // a float loaded into the unit and stored again: the same bits unless it is a
    // signalling NaN, which comes back quiet
    let through = |v: f32| F80::from_f32(v).to_f32();
    let s = m[instance].gscl;
    m[copy].scal.x = through(s.x);
    m[copy].scal.y = through(s.y);
    m[copy].scal.z = through(s.z);
    m[copy].vrot.x = through(m.f32(angles));
    m[copy].vrot.y = through(m.f32(angles + 4));
    m[copy].vrot.z = through(m.f32(angles + 8));
    let t = m[instance].gtrs;
    m[copy].trans.x = through(t.x);
    m[copy].trans.y = through(t.y);
    m[copy].trans.z = through(t.z);
    o.call(m, SI_MODEL_UPDATE_SON_STATE, &[0, copy.0, 1]);
    m.set_sp(sp);
}

/// 1 when making `candidate`'s hierarchy the master of `master`'s instances would close a
/// cycle, else 0 [soft 0x081a957c SI_instanceCheckCycle].
///
/// A node of the candidate's hierarchy that is an instance leads to its own master: if
/// that master is `master` or one of its ancestors the answer is 1, and so it is if the
/// test, started again from that master (as a top node), says 1. Otherwise the brothers
/// are tried when `up` is not null (null for the top node, the father inside), then the
/// sons.
pub fn instance_check_cycle<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, master: Node, candidate: Node, up: Node) -> i32 {
    if !enter(m) {
        return 0;
    }
    let answer = 'test: {
        if m[candidate].algo_type == ALGO_INSTANCE {
            let its_master = model_master_from_instance(o, m, candidate);
            if !its_master.is_null() {
                let mut a = master;
                while !a.is_null() {
                    if a == its_master {
                        break 'test 1;
                    }
                    a = m[a].father;
                }
                if instance_check_cycle(o, m, master, its_master, Node::NULL) as i16 != 0 {
                    break 'test 1;
                }
            }
        }
        if !up.is_null() {
            let brother = m[candidate].brother;
            if !brother.is_null() && instance_check_cycle(o, m, master, brother, up) as i16 != 0 {
                break 'test 1;
            }
        }
        let son = m[candidate].son;
        if !son.is_null() && instance_check_cycle(o, m, master, son, candidate) as i16 != 0 {
            break 'test 1;
        }
        0
    };
    leave(m);
    answer
}
