// SPDX-License-Identifier: MIT
//! Setting and relaxing constraints, and the update lists constraints are run from
//! (`constraint.c`), on the shared memory image (finding 0024).
//!
//! These routines are dictionary work: nearly every line is a call of the element
//! dictionary (`elemdict.c`, `relations.c`, `toposort.c`: on the image in `si-dict`), of
//! the scene's editing layer (`edtools.c`), of clusters or of the interface. All of those
//! are outside this package and are reached through [`Outside`] by name and address, with
//! the original's argument words; an implementation hands the dictionary's to `si-dict`.
//!
//! The scene's dictionary is `SI_elemDicts` + 0x70 × the first word of `SI_procData`,
//! worked out anew before every call, as the original does.
//!
//! `SI_lightLicense` (0x08679320) is a product-tier flag: the full product's path (0) is
//! taken wherever the original reads it, and the flag itself is not ported.

use si_dict::globals::{SI_CID, SI_ELEM_DICTS, SI_GLB_MANIP, SI_RD};
use si_dict::records::{UpdArg, UpdNode};
use si_mem::{Mem, Ptr32};
use si_types::dict::UpdList;
use si_types::model::Model;

use crate::globals::{SI_CUS_UPDATE_STATE, SI_GLB_ANIM, SI_GRAF_LISTS, SI_PROC_DATA};
use crate::outside as out;
use crate::outside::{Outside, Routine};
use crate::rodata;

fn call(o: &mut dyn Outside, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
    o.call(m, routine, args).eax
}

/// The scene's element dictionary.
fn scene(m: &Mem) -> u32 {
    m[SI_ELEM_DICTS].addr().wrapping_add((m[m[SI_PROC_DATA]] as u32).wrapping_mul(0x70))
}

/// The current graf list.
fn graf_list(m: &Mem) -> u32 {
    let slot = m.i32(m[SI_PROC_DATA].addr().wrapping_add(4));
    m[SI_GRAF_LISTS].wrapping_add((slot as u32).wrapping_mul(0x24))
}

/// The chapter of the models, of the clusters and of the cameras in the scene's
/// dictionary (`SI_cid` + 0x20, + 0x08, + 0x04).
fn chap_models(m: &Mem) -> u32 {
    m[SI_CID].model as u32
}

/// Ties an object to a cluster of another [soft 0x081f4d58 itemSetObjPntCns]: relation
/// 126 from the object's element, in rules 1 and 2 to the other object's element and in
/// rule 3 to the element of that object's active cluster.
///
/// Nothing happens unless both nodes have elements. Without an active cluster on the
/// second node's object the status line says so; when the first element already has a
/// relation 126 it says that; both with severity 3.
pub fn item_set_obj_pnt_cns(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, other: Ptr32<Model>) {
    let sp = m.sp();
    let src_index = m.local(4);
    let dst_index = m.local(4);
    let scratch = m.local(4);
    let cluster_index = m.local(4);
    let src = call(o, m, &out::SI_ED_GET_ELEM_FROM_MODEL, &[node.addr(), src_index]);
    let dst = call(o, m, &out::SI_ED_GET_ELEM_FROM_MODEL, &[other.addr(), dst_index]);
    if src != 0 && dst != 0 {
        let cluster = call(o, m, &out::SI_KLUSTER_GET_ACTIVE, &[m.u32(dst.wrapping_add(4))]);
        if cluster == 0 {
            out::si_status_display_message(o, m, rodata::S_NO_CLUSTER.0, 3);
        } else if call(o, m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[scene(m), src, 1, 1, 0x7e, u32::MAX, scratch, scratch]) != 0 {
            out::si_status_display_message(o, m, rodata::S_HAS_CLUSTER_CNS.0, 3);
        } else {
            call(o, m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[cluster, cluster_index]);
            for (rule, to) in [(1, dst_index), (2, dst_index), (3, cluster_index)] {
                call(o, m, &out::SI_ELEMDICT_ADD_COMPLETE_RELATION, &[scene(m), SI_RD.addr(), m.u32(src_index), m.u32(to), 0x7e, rule, 0, 0, 1]);
            }
        }
    }
    m.set_sp(sp);
}

/// Ties the active cluster of an object to another object
/// [soft 0x081f4ec4 SI_itemSetClusterConstraint]: two entries of relation `relation` under
/// one new serial number, both in list 2: rule 1 from the object's element to the other
/// object's (models to models, with `data`), rule 2 from the object's element to its
/// cluster's (models to clusters, without data).
///
/// Nothing happens when a node is null or has no element; without an active cluster on
/// the first node the status line says so (severity 3).
pub fn item_set_cluster_constraint(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, other: Ptr32<Model>, relation: i32, data: u32) {
    if node.is_null() || other.is_null() {
        return;
    }
    let sp = m.sp();
    let src_index = m.local(4);
    let dst_index = m.local(4);
    let cluster_index = m.local(4);
    let src = call(o, m, &out::SI_ED_GET_ELEM_FROM_MODEL, &[node.addr(), src_index]);
    let dst = call(o, m, &out::SI_ED_GET_ELEM_FROM_MODEL, &[other.addr(), dst_index]);
    if src != 0 && dst != 0 {
        let cluster = call(o, m, &out::SI_KLUSTER_GET_ACTIVE, &[node.addr()]);
        if cluster == 0 {
            out::si_status_display_message(o, m, rodata::S_NO_ACTIVE_CLUSTER.0, 3);
        } else {
            call(o, m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[cluster, cluster_index]);
            let serial = call(o, m, &out::SI_ELEMDICT_GET_REL_SERIAL_NUMBER, &[]);
            let models = chap_models(m);
            let args = [scene(m), SI_RD.addr(), models, models, 2, 1, m.u32(src_index), m.u32(dst_index), relation as u32, serial, 1, 0, data, 1, 1];
            call(o, m, &out::SI_ELEMDICT_ADD_SERIAL_RELATION, &args);
            let args = [scene(m), SI_RD.addr(), chap_models(m), m[SI_CID].cls as u32, 2, 0, m.u32(src_index), m.u32(cluster_index), relation as u32, serial, 2, 0, 0, 1, 1];
            call(o, m, &out::SI_ELEMDICT_ADD_SERIAL_RELATION, &args);
        }
    }
    m.set_sp(sp);
}

/// Whether a node has an active cluster constraint
/// [soft 0x081f5250 SI_modelHasConstrainedClusters]: an entry of relation 246 or 247 to a
/// model, in list 2 of its element, whose active word (`SI_ARelation` + 0x14) is not 0.
pub fn model_has_constrained_clusters(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>) -> i32 {
    if node.is_null() {
        return 0;
    }
    let sp = m.sp();
    let index = m.local(4);
    let found = m.local(4);
    let mut answer = 0;
    let elem = call(o, m, &out::SI_ED_GET_ELEM_FROM_MODEL, &[node.addr(), index]);
    if elem != 0 {
        let mut hit = call(o, m, &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM, &[scene(m), elem, 1, 2, 0xf6, 0xf7, chap_models(m), found, found]);
        while hit != 0 {
            let rel = call(o, m, &out::SI_ELEMDICT_GET_CUR_RELATION, &[scene(m), elem, 1, 2]);
            if rel != 0 && m.i16(rel.wrapping_add(0x14)) != 0 {
                answer = 1;
                break;
            }
            hit = call(o, m, &out::SI_ELEMDICT_FIND_RANGE_NEXT_RELATED_ELEM, &[scene(m), elem, 1, 2, 0xf6, 0xf7, chap_models(m), found, found]);
        }
    }
    m.set_sp(sp);
    answer
}

/// The relations a model's constraints are, in the order they are relaxed; any other kind
/// of item has the first of [`RELAXED_OTHER`] only.
const RELAXED_MODEL: [u32; 17] = [0x6f, 0x72, 0x74, 0x73, 0x75, 0x76, 0x66, 0x67, 0x7c, 0x7e, 0x7d, 0x7f, 0x77, 0xf6, 0x80, 0x81, 0xf7];
const RELAXED_OTHER: [u32; 2] = [0x6f, 0x80];

/// Removes every constraint of an item [soft 0x081f533c SI_itemRelaxConstraint]: one
/// `SI_edElementRelaxConstraint` per relation, seventeen for a model (graf type 2 or 3),
/// two for anything else. A null item is left alone.
pub fn item_relax_constraint(m: &mut Mem, o: &mut dyn Outside, item: u32, kind: i32) {
    if item == 0 {
        return;
    }
    let list: &[u32] = if (kind as u32).wrapping_sub(2) < 2 { &RELAXED_MODEL } else { &RELAXED_OTHER };
    for &relation in list {
        call(o, m, &out::SI_ED_ELEMENT_RELAX_CONSTRAINT, &[item, kind as u32, relation]);
    }
}

/// Whether the scene's update list has a step of a camera constraint
/// [soft 0x081f5474 SI_anyVisibleCameraConstraints]: a node (0x2c bytes each) of the
/// dictionary's first update list whose relation is 1111, 1116 or 1117.
pub fn any_visible_camera_constraints(m: &Mem) -> i32 {
    let list: Ptr32<UpdList> = Ptr32::new(scene(m).wrapping_add(0x0c));
    let count = m[list].nb;
    let mut i = 0;
    while i < count {
        let relation = m.i32(m[list].nodes.addr().wrapping_add(0x14).wrapping_add((i as u32).wrapping_mul(0x2c)));
        if relation == 0x457 || relation == 0x45c || relation == 0x45d {
            return 1;
        }
        i += 1;
    }
    0
}

/// Removes one kind of link constraint from a node
/// [soft 0x081f54d8 SI_relaxCnsLinkFromData]: the backward entry of `relation` to a model
/// in list 1 of the node's element. An entry with a serial number above 0 goes with all
/// entries of that number (`SI_elemdictRemoveSerialRelId`), any other by relation
/// (`SI_elemdictRemoveRelationId`). For relation 247 the object at the other end loses a
/// definition and has its dependencies updated.
pub fn relax_cns_link_from_data(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, relation: i32) {
    let sp = m.sp();
    let index = m.local(4);
    let other_chap = m.local(4);
    let other_index = m.local(4);
    let elem = call(o, m, &out::SI_ED_GET_ELEM_FROM_MODEL, &[node.addr(), index]);
    let other = call(o, m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[scene(m), elem, 0, 1, relation as u32, chap_models(m), other_chap, other_index]);
    if other != 0 {
        let rel = call(o, m, &out::SI_ELEMDICT_GET_CUR_RELATION, &[scene(m), elem, 0, 1]);
        if rel != 0 && m.i32(rel.wrapping_add(0x0c)) > 0 {
            call(o, m, &out::SI_ELEMDICT_REMOVE_SERIAL_REL_ID, &[scene(m), SI_RD.addr(), chap_models(m), m.u32(other_index), relation as u32, m.u32(rel.wrapping_add(0x0c))]);
        } else {
            call(o, m, &out::SI_ELEMDICT_REMOVE_RELATION_ID, &[scene(m), SI_RD.addr(), m.u32(other_index), relation as u32]);
        }
        if relation == 0xf7 && m.u32(other.wrapping_add(4)) != 0 {
            call(o, m, &out::SI_MODEL_DELETE_DEFINITION, &[m.u32(other.wrapping_add(4)), 1]);
            call(o, m, &out::SI_MODEL_UPDATE_DEPENDENCY, &[m.u32(other.wrapping_add(4)), 0]);
        }
    }
    m.set_sp(sp);
}

/// Removes the constraints that hang on an item as their data
/// [soft 0x081f55f4 SI_constraintRelaxFromData]. For a model (graf type 2 or 3) the link
/// constraints 127, 119, 246 and 247 go ([`relax_cns_link_from_data`]), and every model
/// in the current graf list (nodes of type 2 or 3 with an object) is asked to drop its
/// dynamic constraints on the item; the answer is the last such call's, as 16 bits. A
/// null item answers 0.
pub fn constraint_relax_from_data(m: &mut Mem, o: &mut dyn Outside, item: Ptr32<Model>, kind: i32) -> i32 {
    let mut answer = 0i16;
    if item.is_null() {
        return 0;
    }
    let model = (kind as u32).wrapping_sub(2) <= 1;
    if model {
        for relation in [0x7f, 0x77, 0xf6, 0xf7] {
            relax_cns_link_from_data(m, o, item, relation);
        }
    }
    let mut node = m.u32(graf_list(m).wrapping_add(4));
    while node != 0 {
        let object = m.u32(node.wrapping_add(4));
        let node_kind = m.i32(node);
        if object != 0 && node_kind <= 3 && node_kind >= 2 && model {
            answer = call(o, m, &out::SI_DYNA_CONSTRAINT_RELAX_FROM_DATA, &[object, item.addr()]) as i16;
        }
        node = m.u32(node.wrapping_add(0x1c));
    }
    answer as i32
}

/// Builds the scene's second update list for the selected elements
/// [soft 0x081f5b30 SI_buildLocalUpdateList] and starts its pivot constraints. Answers
/// whether the list has a node.
///
/// The list (`SI_ElemDict` + 0x4c) is marked dirty, the sort's marks are cleared
/// (`SI_relTopoSortInit`), the selected elements are sorted into it
/// (`SI_elemdictTopoSortSelectedElements` with `group` and `flag`), and
/// [`crate::pivot::scene_constraint_init`] runs over it.
pub fn build_local_update_list(m: &mut Mem, o: &mut dyn Outside, group: i32, flag: i16) -> i32 {
    let ed = scene(m);
    m.set_i32(ed.wrapping_add(0x54), 1);
    call(o, m, &out::SI_REL_TOPO_SORT_INIT, &[ed, 0]);
    call(o, m, &out::SI_ELEMDICT_TOPO_SORT_SELECTED_ELEMENTS, &[SI_RD.addr(), scene(m), group as u32, flag as i32 as u32, 0]);
    crate::pivot::scene_constraint_init(m, o, Ptr32::new(scene(m).wrapping_add(0x4c)));
    (m.i32(scene(m).wrapping_add(0x4c)) > 0) as i32
}

/// The update list for constraints [soft 0x081f5bf0 SI_getConstraintUpdateList].
///
/// With `camera` 0 it is built for the selection: nothing (0) when the current graf list
/// has nothing selected (`SI_graflistCountSelected(list, 0, 1)` below 1), else the
/// selected elements are marked (`SI_edGetSelectedElements`) and the list is built with
/// flag 1. Otherwise for the current camera: every element is unselected, the camera's
/// element and its interest's are marked selected (+0x42), and the list is built with
/// what `SI_elemdictIsElemUsed` says of the interest's element. The answer is
/// [`build_local_update_list`]'s, as 16 bits.
///
/// Product tier: the original hands `SI_lightLicense` to `SI_edGetSelectedElements` and,
/// when it is set, ends with `SI_edUnselectedNseInDict`; the full product's value 0 is
/// passed and that call is not made.
pub fn get_constraint_update_list(m: &mut Mem, o: &mut dyn Outside, group: i32, camera: i16) -> i32 {
    let mut flag = 1i16;
    if camera != 0 {
        let index = m.local(4);
        call(o, m, &out::SI_ELEMDICT_SET_SELECTION, &[scene(m), 0]);
        let elem = call(o, m, &out::SI_ED_GET_ELEM_FROM_CAMERA, &[m.u32(m[SI_PROC_DATA].addr().wrapping_add(8)), index]);
        if elem != 0 {
            m.set_i16(elem.wrapping_add(0x42), 1);
        }
        let interest = call(o, m, &out::SI_ED_GET_CAM_FROM_CAM_INT, &[m.u32(index)]);
        m.release(4);
        let elem = call(o, m, &out::SI_ELEMDICT_GET_ELEM, &[scene(m), m[SI_CID].cam as u32, interest]);
        if elem != 0 {
            m.set_i16(elem.wrapping_add(0x42), 1);
        }
        flag = call(o, m, &out::SI_ELEMDICT_IS_ELEM_USED, &[elem, 0]) as i16;
    } else {
        if (call(o, m, &out::SI_GRAFLIST_COUNT_SELECTED, &[graf_list(m), 0, 1]) as i32) < 1 {
            return 0;
        }
        call(o, m, &out::SI_ED_GET_SELECTED_ELEMENTS, &[scene(m), 0]);
    }
    build_local_update_list(m, o, group, flag) as i16 as i32
}

/// Sets a constraint of relation `relation` from `item` on `other`
/// [soft 0x081f4fec SI_itemSetConstraint]. Answers 0 when `item` is null, when
/// `SI_ikValidConstraint` refuses the pair, or when a path constraint was added and
/// closed a cycle of relations; 1 otherwise, also when nothing was added.
///
/// By relation:
/// - 246: [`item_set_cluster_constraint`] without data; 247: the same with 0x40 bytes from
///   `SI_malloc` that `SI_klusterCnsSetData` fills; 126: [`item_set_obj_pnt_cns`];
/// - 124 tangency, 103 rotation limit, 102 position limit: the relation is added
///   (`SI_edElementSetConstraint`) with a new record of its kind; a joint (kind 5) gets no
///   position limit, only a message; after a rotation limit on a joint that is a model
///   its preferred angles are brought inside the limits;
/// - 116: two bytes from `SI_malloc`, cleared when the block is not null;
/// - 111, the path constraint: when the item is on a path already (the word at +0x2c8 of
///   a model, +0xa0, +0xa4, +0x144 or +0x148 of the other kinds of item; any item of a
///   kind without such a word counts as on one) the user is asked and "Cancel" (any
///   answer but 0) ends it; waves (kind 11) are refused with a message. Then the relation
///   is added without data and the dictionary checked for a cycle;
/// - any other: added without data.
pub fn item_set_constraint(m: &mut Mem, o: &mut dyn Outside, item: Ptr32<Model>, kind: i32, other: Ptr32<Model>, other_kind: i32, relation: i16) -> i32 {
    if item.is_null() || crate::onimage::ik_valid_constraint(m, item, kind, other, other_kind) as i16 == 0 {
        return 0;
    }
    let data = match relation {
        0x7c => crate::onimage::tan_constr_create_data(m).addr(),
        0xf6 => {
            item_set_cluster_constraint(m, o, item, other, 0xf6, 0);
            return 1;
        }
        0x7e => {
            item_set_obj_pnt_cns(m, o, item, other);
            return 1;
        }
        0xf7 => {
            let data = m.si_malloc(0x40);
            call(o, m, &out::SI_KLUSTER_CNS_SET_DATA, &[item.addr(), other.addr(), data]);
            item_set_cluster_constraint(m, o, item, other, 0xf7, data);
            return 1;
        }
        0x67 => crate::onimage::rotlim_create_data(m),
        0x66 => {
            if m[item].kind == 5 {
                out::si_status_display_message(o, m, rodata::S_NO_POSLIM_FOR_JOINT.0, 3);
                return 1;
            }
            crate::onimage::poslim_create_data(m)
        }
        0x74 => {
            let p = m.si_malloc(2);
            if p != 0 {
                m.set_i16(p, 0);
            }
            p
        }
        _ => 0,
    };
    if relation != 0x6f {
        call(o, m, &out::SI_ED_ELEMENT_SET_CONSTRAINT, &[item.addr(), kind as u32, other.addr(), other_kind as u32, relation as i32 as u32, data]);
        if relation == 0x67 && (kind.wrapping_sub(2) as u32) <= 1 && m[item].kind == 5 {
            crate::onimage::inv_kin_pref_angle_update(m, o, item);
        }
        return 1;
    }
    let on_path = match kind {
        2 | 3 => m.u32(item.addr().wrapping_add(0x2c8)) != 0,
        4 => m.u32(item.addr().wrapping_add(0xa0)) != 0,
        5 => m.u32(item.addr().wrapping_add(0xa4)) != 0,
        6 | 8 | 19 => m.u32(item.addr().wrapping_add(0x144)) != 0,
        7 => m.u32(item.addr().wrapping_add(0x148)) != 0,
        11 => {
            out::si_status_display_message(o, m, rodata::S_WAVES.0, 3);
            return 1;
        }
        _ => true,
    };
    if on_path {
        let q = rodata::S_ON_PATH;
        if call(o, m, &out::SI_DIALOG_REQUEST2, &[q[0].0, q[1].0, q[2].0, q[3].0, q[4].0]) as i16 != 0 {
            return 1;
        }
    }
    call(o, m, &out::SI_ED_ELEMENT_SET_CONSTRAINT, &[item.addr(), kind as u32, other.addr(), other_kind as u32, relation as i32 as u32, 0]);
    (call(o, m, &out::SI_ELEMDICT_CHECK_CYCLE, &[SI_RD.addr(), scene(m)]) as i16 == 0) as i32
}

/// The element `index` of chapter `chap` of the scene's dictionary, as the update loop
/// works it out itself: `lists[chap].elems` + 0x84 × index, nothing tested.
fn elem_at(m: &Mem, chap: i32, index: i32) -> u32 {
    let lists = m.u32(scene(m).wrapping_add(4));
    let elems = m.u32(lists.wrapping_add((chap as u32).wrapping_mul(0x40)).wrapping_add(8));
    elems.wrapping_add((index as u32).wrapping_mul(0x84))
}

/// Offset of an element's selection mark (`__SI_Elem.selected`).
const SELECTED: u32 = 0x42;

/// Runs the constraints of the scene's second update list while an object is moved by
/// hand [soft 0x081f56b4 SI_sceneConstrUpdate]. `group` is the kind of manipulation,
/// `time` the time, `vector` what the plane constraints get.
///
/// 1. The selected elements are marked (`SI_edGetSelectedElements`; the product-tier flag
///    is passed as 0). With buttons 0x8f and 3 both down the list is dumped.
/// 2. Nothing more happens when the word at `SI_glbManip` + 0x24 is 0. Otherwise
///    `SI_cusUpdateState` is 3 for the duration and 2 afterwards.
/// 3. For every node of relation 250 or 251 whose element is not marked and whose object
///    is a null: the element is marked when the element of one of the object's sons is.
/// 4. With the word at `SI_glbAnim` + 6 raised and the list made current, every node is
///    looked at. A node is filtered by its arguments when the word at `SI_glbManip` +
///    0x26 is 0, or its element is marked 1 or 3, or its object is a joint (kind 5), or
///    its chapter is the one at `SI_cid` + 0x18 or the cameras': it is skipped when one of
///    its arguments is an animation element (the scan stops at relation 239). What
///    remains is skipped when the element is marked and the manipulation is 3 to 5 on a
///    node of list 2, unless the relation is 247, −200 or 20000; when the element is
///    marked and the manipulation is 6 to 8 on relation 239; and relation 101 always.
///    Under manipulation 6 to 8 a node of relation 247 with data and an argument gets its
///    cluster data refreshed (`SI_klusterCnsSetData`) instead of an update. Everything
///    else is updated: relations 102 and 103 by `si_animModelConstraintUpdate`, 117 and
///    118 by `si_animModelPlaneConstrUpdate`, 150 by `si_animModelSymConstrUpdate`, any
///    other by `si_updNodeUpdate`.
/// 5. The envelope flags are reset when the word at `SI_glbAnim` + 0x38 is set.
///
/// Product tier: the original ends with `SI_edUnselectedNseInDict` when
/// `SI_lightLicense` is set; the full product does not make that call.
pub fn scene_constr_update(m: &mut Mem, o: &mut dyn Outside, group: i32, time: f32, vector: u32) {
    let post_info = m.local(4);
    m.set_u32(post_info, 0);
    let list: Ptr32<UpdList> = Ptr32::new(scene(m).wrapping_add(0x4c));
    call(o, m, &out::SI_ED_GET_SELECTED_ELEMENTS, &[scene(m), 0]);
    if call(o, m, &out::GETBUTTON, &[0x8f]) != 0 && call(o, m, &out::GETBUTTON, &[3]) != 0 {
        call(o, m, &out::SI_UPDLIST_DUMP, &[scene(m), SI_RD.addr(), scene(m).wrapping_add(0x4c), 1]);
    }
    if m.i16(SI_GLB_MANIP.addr() + 0x24) == 0 {
        m.release(4);
        return;
    }
    m[SI_CUS_UPDATE_STATE] = 3;
    let index = m.local(4);
    let mut i = 0i32;
    while i < m[list].nb {
        let node: Ptr32<UpdNode> = m[list].nodes.cast::<UpdNode>().add(i);
        i += 1;
        if (m[node].relid.wrapping_sub(0xfa) as u32) > 1 {
            continue;
        }
        m.set_i32(index, m[node].elemid);
        let elem = call(o, m, &out::SI_ELEMDICT_GET_ELEM, &[scene(m), m[node].chapid as i32 as u32, m[node].elemid as u32]);
        if elem == 0 || m.i16(elem + SELECTED) != 0 {
            continue;
        }
        let object: Ptr32<Model> = m[node].data.cast();
        if m[object].kind != 0 {
            continue;
        }
        let mut son = m[object].son;
        while !son.is_null() {
            let son_elem = call(o, m, &out::SI_ED_GET_ELEM_FROM_MODEL, &[son.addr(), index]);
            if m.i16(son_elem.wrapping_add(SELECTED)) == 1 {
                m.set_i16(elem + SELECTED, 1);
                break;
            }
            son = m[son].brother;
        }
    }
    m[SI_GLB_ANIM][3] = 1;
    call(o, m, &out::SI_UPD_LIST_SET_CURRENT, &[list.addr()]);
    let mut i = 0i32;
    while i < m[list].nb {
        let node: Ptr32<UpdNode> = m[list].nodes.cast::<UpdNode>().add(i);
        i += 1;
        let chap = m[node].chapid as i32;
        m.set_i32(index, m[node].elemid);
        let elem = elem_at(m, chap, m[node].elemid);
        let mut joint = false;
        if chap == m[SI_CID].model {
            joint = m.i32(m.u32(elem.wrapping_add(4)).wrapping_add(0x30)) == 5;
        }
        let mark = m.i16(elem.wrapping_add(SELECTED));
        let by_arguments =
            m.i16(SI_GLB_MANIP.addr() + 0x26) == 0 || mark == 1 || mark == 3 || joint || chap == m.i32(SI_CID.addr() + 0x18) || chap == m[SI_CID].cam;
        if by_arguments {
            let relation = m[node].relid;
            let count = m[node].nb_args as i32;
            let mut animated = false;
            let mut k = 0i32;
            while k < count && relation != 0xef {
                let arg: Ptr32<UpdArg> = m[node].args.add(k);
                if m[arg].chapid as i32 == m[SI_CID].ani {
                    animated = true;
                    break;
                }
                k += 1;
            }
            if animated {
                continue;
            }
        }
        let relation = m[node].relid;
        let mark = m.i16(elem.wrapping_add(SELECTED));
        if mark != 0 {
            if (group.wrapping_sub(3) as u32) <= 2 && m[node].group == 2 && relation != 0xf7 && relation != -200 && relation != 20000 {
                continue;
            }
            if (group.wrapping_sub(6) as u32) <= 2 && relation == 0xef {
                continue;
            }
        }
        if (group.wrapping_sub(6) as u32) <= 2 && relation == 0xf7 && !m[node].rel_data.is_null() && m[node].nb_args > 0 {
            let arg: Ptr32<UpdArg> = m[node].args;
            let other = elem_at(m, m[arg].chapid as i32, m[arg].elemid);
            call(o, m, &out::SI_KLUSTER_CNS_SET_DATA, &[m.u32(elem.wrapping_add(4)), m.u32(other.wrapping_add(4)), m[node].rel_data.addr()]);
            continue;
        }
        match relation {
            0x65 => {}
            0x66 | 0x67 => {
                call(o, m, &out::SI_ANIM_MODEL_CONSTRAINT_UPDATE, &[node.addr(), time.to_bits(), group as u32]);
            }
            0x75 | 0x76 => {
                call(o, m, &out::SI_ANIM_MODEL_PLANE_CONSTR_UPDATE, &[node.addr(), time.to_bits(), group as u32, vector]);
            }
            0x96 => {
                call(o, m, &out::SI_ANIM_MODEL_SYM_CONSTR_UPDATE, &[node.addr(), time.to_bits(), group as u32]);
            }
            _ => {
                call(o, m, &out::SI_UPD_NODE_UPDATE, &[node.addr(), time.to_bits(), post_info]);
            }
        }
    }
    if m[SI_GLB_ANIM][0x1c] != 0 {
        call(o, m, &out::SI_ENV_SKEL_RESET_FLAG, &[scene(m)]);
    }
    m[SI_GLB_ANIM][3] = 0;
    m[SI_CUS_UPDATE_STATE] = 2;
    m.release(8);
}
