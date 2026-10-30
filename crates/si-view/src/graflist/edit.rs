// SPDX-License-Identifier: MIT
//! Taking a node's object out of the scene: its animation, expressions, constraints and
//! curves, and the disposal of what it stands for. Everything here is a sequence of calls
//! into other parts of the program; what is the graph list's own is which calls a node of
//! each kind gets, with which arguments, in which order.

use super::list::{find_gl_data, graflist_insert_grafnode, graflist_remove_grafnode, graflist_set_active, grafnode_allocate, is_model};
use super::select::{select_grafnode, unselect_grafnode};
use crate::globals::*;
use crate::outside::{self, Outside};
use si_mem::{Mem, Opaque, Ptr32};
use si_types::dict::Elem;
use si_types::model::Model;
use si_types::scene::{GrafList, GrafNode};
use si_x87::F80;

/// A path's own deletion for a node that is one
/// [soft 0x0832ca20 pathRelaxFromData]: a model node whose model is of kind 6 or 9 goes
/// to `SI_pathDelete (0x0821f284)`. Nothing for null.
pub fn path_relax_from_data(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<GrafNode>) {
    if node.is_null() || !is_model(m[node].kind) {
        return;
    }
    let model: Ptr32<Model> = m[node].data.cast();
    let k = m[model].kind;
    if k == 6 || k == 9 {
        o.call(m, &outside::SI_PATH_DELETE, &[model.addr()]);
    }
}

/// Disposes of what a node stands for, through the caller's pointer to the node
/// [soft 0x0832ca54 SI_grafnodeDispose]: first `SI_edGrafNodeSetDeleteFlag (0x080fdcf4)`
/// with (node, 1, null, 1); then by kind: a model node `SI_modelDispose (0x08461920)` with
/// the address of the node's object word and 0; a curve (9) `SI_fcurveRemove (0x081ff0a0)`
/// and a wave (0xb) `SI_waveDispose (0x084c6f68)` with that address; a light (6, 7, 8,
/// 0x13, 0x14) `SI_lightDispose (0x082818f0)` with the object. Other kinds: nothing. The
/// node itself is not freed. Answers 0.
pub fn grafnode_dispose(m: &mut Mem, o: &mut dyn Outside, pnode: Ptr32<Ptr32<GrafNode>>) -> i32 {
    let node = m[pnode];
    o.call(m, &outside::SI_ED_GRAF_NODE_SET_DELETE_FLAG, &[node.addr(), 1, 0, 1]);
    let node = m[pnode];
    let data_at = node.addr().wrapping_add(4);
    match m[node].kind {
        2 | 3 => {
            o.call(m, &outside::SI_MODEL_DISPOSE, &[data_at, 0]);
        }
        9 => {
            o.call(m, &outside::SI_FCURVE_REMOVE, &[data_at]);
        }
        0xb => {
            o.call(m, &outside::SI_WAVE_DISPOSE, &[data_at]);
        }
        6 | 7 | 8 | 0x13 | 0x14 => {
            let light = m[node].data.addr();
            o.call(m, &outside::SI_LIGHT_DISPOSE, &[light]);
        }
        _ => {}
    }
    0
}

/// Deletes the curves of a node's object from the scene's curve lists
/// [soft 0x0832cacc sceneGraflistRemoveFcurves]. A model node: `SI_modelAFunction
/// (0x0822cc48)` with `SI_fcvScnglBDelete (0x0823c8dc)`, the model and the float 0. A light
/// (6, 8, 0x13, 0x14; not the node in front of one): its element from
/// `SI_edGetElemFromLight (0x080fd0bc)`, then `SI_elemdictActiveFcvOper (0x0822c8a4)` on
/// the current dictionary with that element, the lights' chapter, the element's index,
/// 0x18, the same routine and seven zero words. Other kinds: nothing.
pub fn scene_graflist_remove_fcurves(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<GrafNode>) {
    match m[node].kind {
        2 | 3 => {
            let model = m[node].data.addr();
            o.call(m, &outside::SI_MODEL_A_FUNCTION, &[outside::SI_FCV_SCNGL_B_DELETE.addr, model, 0]);
        }
        6 | 8 | 0x13 | 0x14 => {
            let light = m[node].data.addr();
            let index = m.local(4);
            let elem = o.call(m, &outside::SI_ED_GET_ELEM_FROM_LIGHT, &[light, index]);
            let chap = m[SI_CID][cid::LIGHTS] as u32;
            let ed = current_dict(m);
            let i = m.u32(index);
            o.call(m, &outside::SI_ELEMDICT_ACTIVE_FCV_OPER, &[ed, elem, chap, i, 0x18, outside::SI_FCV_SCNGL_B_DELETE.addr, 0, 0, 0, 0, 0, 0, 0]);
            m.release(4);
        }
        _ => {}
    }
}

/// Removes the animation of a node's element
/// [soft 0x0832cb58 grafnodeDeleteAnimation]: not for kind 0x12; the element from
/// `SI_elemdictFindElemFromGrafNode (0x080fcbec)`, and when there is one
/// `SI_edRemoveElemAnim (0x08221190)` with the current dictionary, the chapter, the
/// element, its index and 1.
pub fn grafnode_delete_animation(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<GrafNode>) {
    if m[node].kind == 0x12 {
        return;
    }
    let index = m.local(4);
    let chap = m.local(4);
    let elem = o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]);
    if elem != 0 {
        let ed = current_dict(m);
        let (c, i) = (m.u32(chap), m.u32(index));
        o.call(m, &outside::SI_ED_REMOVE_ELEM_ANIM, &[ed, c, elem, i, 1]);
    }
    m.release(8);
}

/// Removes the expressions on and through a node's element
/// [soft 0x0832cbb0 grafnodeDeleteExpressions]: not for kind 0x12; with the element,
/// `SI_edRemoveExpressionsFromArg (0x08255ac0)` (dictionary, element) and then
/// `SI_edRemoveElemExpressions (0x082559d0)` (dictionary, element, chapter, index).
pub fn grafnode_delete_expressions(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<GrafNode>) {
    if m[node].kind == 0x12 {
        return;
    }
    let index = m.local(4);
    let chap = m.local(4);
    let elem = o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]);
    if elem != 0 {
        let ed = current_dict(m);
        o.call(m, &outside::SI_ED_REMOVE_EXPRESSIONS_FROM_ARG, &[ed, elem]);
        let (c, i) = (m.u32(chap), m.u32(index));
        let ed = current_dict(m);
        o.call(m, &outside::SI_ED_REMOVE_ELEM_EXPRESSIONS, &[ed, elem, c, i]);
    }
    m.release(8);
}

/// Cuts everything that hangs on a node's object before it goes
/// [soft 0x0832c92c SI_grafnodeCutConnections], in this order: the element is looked up
/// (`SI_elemdictFindElemFromGrafNode`); animation; expressions; `SI_chnGrafNodeDelete
/// (0x0856136c)`; `SI_constraintRelaxFromData (0x081f55f4)` with the object and the kind;
/// the path; the curves; and last `SI_cusDeleteElement (0x08549998)` with the element, its
/// chapter and its index as the first look-up answered them.
pub fn grafnode_cut_connections(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<GrafNode>) {
    let index = m.local(4);
    let chap = m.local(4);
    let elem = o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]);
    grafnode_delete_animation(m, o, node);
    grafnode_delete_expressions(m, o, node);
    o.call(m, &outside::SI_CHN_GRAF_NODE_DELETE, &[node.addr()]);
    let (data, kind) = (m[node].data.addr(), m[node].kind as u32);
    o.call(m, &outside::SI_CONSTRAINT_RELAX_FROM_DATA, &[data, kind]);
    path_relax_from_data(m, o, node);
    scene_graflist_remove_fcurves(m, o, node);
    let (c, i) = (m.u32(chap), m.u32(index));
    o.call(m, &outside::SI_CUS_DELETE_ELEMENT, &[elem, c, i]);
    m.release(8);
}

/// Takes a model out of the light lists of every light that names it
/// [soft 0x0833959c modelUnlinkAllLight]: the model's element and index
/// (`SI_edGetElemFromModel (0x080fcf20)`); then for every element related to it by
/// relation 0x898 in list 3 of the backward side, in the lights' chapter
/// (`SI_elemdictFindRelatedElem (0x0813508c)`, then `SI_elemdictFindNextRelatedElem
/// (0x08135154)`): in that element's object, a light, the word at the model's index in the
/// array at +0x168 is cleared. Neither the array nor the index is tested.
pub fn model_unlink_all_light(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) {
    let index = m.local(4);
    let found_index = m.local(4);
    let found_chap = m.local(4);
    let elem = o.call(m, &outside::SI_ED_GET_ELEM_FROM_MODEL, &[model.addr(), index]);
    let chap = m[SI_CID][cid::LIGHTS] as u32;
    let ed = current_dict(m);
    let mut rel = o.call(m, &outside::SI_ELEMDICT_FIND_RELATED_ELEM, &[ed, elem, 0, 3, 0x898, chap, found_chap, found_index]);
    while rel != 0 {
        let light = m.u32(rel.wrapping_add(4));
        let slots = m.u32(light.wrapping_add(0x168));
        let i = m.u32(index);
        m.set_u32(slots.wrapping_add(i.wrapping_mul(4)), 0);
        if m.fault().is_some() {
            // the original is dead by now
            break;
        }
        let chap = m[SI_CID][cid::LIGHTS] as u32;
        let ed = current_dict(m);
        rel = o.call(m, &outside::SI_ELEMDICT_FIND_NEXT_RELATED_ELEM, &[ed, elem, 0, 3, 0x898, chap, found_chap, found_index]);
    }
    m.release(12);
}

// ---------------------------------------------------------------- adding an object

/// The list's active node, whichever of the two places holds it, is unselected (when
/// there is one) and `node` made the active node: what `SI_graflistAddData` does for a new
/// node that comes selected into a list in single selection.
fn take_over_selection(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, node: Ptr32<GrafNode>) {
    let mut a = m[list].active;
    if a.is_null() {
        a = m[list].active2;
    }
    if !a.is_null() {
        unselect_grafnode(m, o, list, a);
    }
    graflist_set_active(m, list, node);
}

/// Puts an object into a list [soft 0x0832d240 SI_graflistAddData]: `kind` says what
/// `data` is, `select` whether it comes selected. Answers the object's first node, or
/// null when it was refused.
///
/// **Order.** The new node goes to the **head** of the list. What follows depends on the
/// kind:
///
/// - a model's root (2): the nodes of the hierarchy follow it at once, one per model node
///   in the order `SI_modelNextNode (0x08478340)` hands them out, each chained behind the
///   one before; they have kind 3. Each node's `nb_sub` is `SI_modelNbSubModels
///   (0x0846d5ec)` of its model, each gets its element from `SI_edAddModel (0x08101ad4)`
///   (model, node, the unique-name switch). With `select` the root gets selection 1, or 3
///   when it has nodes below it, and those get 2. Last `SI_elemdictUpdateModelHierarchy
///   (0x080fd698)` on the current dictionary.
/// - a camera (4): the head node becomes kind 5 with `nb_sub` 1 and a second node of kind
///   4 for the same object follows it; `SI_edAddCamera (0x08101b08)` (camera node, head
///   node, switch). The schematic view is not told.
/// - a light (6): the same with kinds 7 and 6 and `SI_edAddLight (0x08101d70)`; with
///   `select` the light's own node gets selection 1 and the head node 1 only in multiple
///   selection, and in single selection the light's node takes over as the active node.
/// - the lights of kinds 8, 0x13, 0x14: one node, `SI_edAddLight` (node, null, switch).
/// - a material, a 2D or a 3D texture (0xe, 0xf, 0x10): one node; when `SI_edAddMaterial
///   (0x0810203c)`, `SI_edAddTexture2D (0x08102314)` or `SI_edAddTexture3D (0x081027e0)`
///   (node, switch, object) answers 0 the node is taken out again and freed, and null
///   answered **without the three calls that end the routine**.
/// - a wave (0xb): one node, `SI_edAddWave (0x081033b4)` (node, null).
/// - any other kind: one node and nothing else.
///
/// Every node gets the default wire colour (`SI_select7WM` + `SI_color1`, 16 bits) and,
/// when the schematic view's word at +0x3e is set and the one at +0x3c is not,
/// `SI_schemSetDefaultColor (0x083516d0)`. Unless `schemNoRefresh` is set (and not for a
/// camera) `SI_schematicAdjustNewNode (0x0834fe20)` places the node. A selected node in a
/// list in single selection has the list's active node unselected and becomes it. The
/// routine ends with `SI_adjustGnFunc (0x08176700)` on the node, `SI_dopeFlagsSet
/// (0x081fa5d0)` (1, 1, 0, 0) and `SI_sprdUpdateSelect (0x085aaa7c)`; these three are also
/// made, with a null node, when the first node could not be allocated.
///
/// When a node of the hierarchy cannot be allocated the nodes made so far are taken out
/// of the list (not freed) and null is answered at once. The second node of a camera or
/// a light is written without a test for null.
pub fn graflist_add_data(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, data: Ptr32<Opaque>, kind: i32, select: i16) -> Ptr32<GrafNode> {
    let mut select = select;
    let node = grafnode_allocate(m);
    if !node.is_null() {
        o.call(m, &outside::SI_FAST_SET_DIRTY, &[0, 0xffff_ffff, 1]);
        graflist_insert_grafnode(m, list, Ptr32::NULL, node);
        m[node].wire_color = m[SI_SELECT7WM].wrapping_add(m[SI_COLOR1]) as i16;
        m[node].data = data;
        m[node].kind = kind;
        if m.i16(SI_GLB_SCHEM.addr() + GLB_SCHEM_3E) != 0 && m.i16(SI_GLB_SCHEM.addr() + GLB_SCHEM_3C) == 0 {
            o.call(m, &outside::SI_SCHEM_SET_DEFAULT_COLOR, &[node.addr()]);
        }
        if select != 0 {
            m[node].selection = 1;
        }
        // whether the schematic view is told at the end (not for a camera or a light,
        // which is told on the way)
        let mut adjust = true;
        match kind {
            2 => {
                let mut model = data.addr();
                let unique = m[SI_UNIQUE_NAME_FLAG] as u16 as u32;
                o.call(m, &outside::SI_ED_ADD_MODEL, &[model, node.addr(), unique]);
                let n = o.call(m, &outside::SI_MODEL_NB_SUB_MODELS, &[model]) as i16;
                m[node].nb_sub = n;
                if select != 0 && n != 0 {
                    m[node].selection = 3;
                }
                let mut last = node;
                loop {
                    model = o.call(m, &outside::SI_MODEL_NEXT_NODE, &[model]);
                    if model == 0 {
                        break;
                    }
                    let sub = grafnode_allocate(m);
                    if sub.is_null() {
                        while last != node {
                            let before = m[last].prev;
                            graflist_remove_grafnode(m, o, list, last);
                            last = before;
                        }
                        graflist_remove_grafnode(m, o, list, last);
                        return Ptr32::NULL;
                    }
                    m[sub].wire_color = m[node].wire_color;
                    graflist_insert_grafnode(m, list, last, sub);
                    let unique = m[SI_UNIQUE_NAME_FLAG] as u16 as u32;
                    o.call(m, &outside::SI_ED_ADD_MODEL, &[model, sub.addr(), unique]);
                    m[sub].kind = 3;
                    last = sub;
                    m[sub].data = Ptr32::new(model);
                    let n = o.call(m, &outside::SI_MODEL_NB_SUB_MODELS, &[model]) as i16;
                    m[sub].nb_sub = n;
                    if select != 0 {
                        m[sub].selection = 2;
                    }
                }
                let ed = current_dict(m);
                o.call(m, &outside::SI_ELEMDICT_UPDATE_MODEL_HIERARCHY, &[ed]);
            }
            4 | 6 => {
                m[node].kind = kind + 1;
                m[node].nb_sub = 1;
                let sub = grafnode_allocate(m);
                graflist_insert_grafnode(m, list, node, sub);
                m[sub].data = data;
                m[sub].wire_color = m[node].wire_color;
                m[sub].kind = kind;
                let unique = m[SI_UNIQUE_NAME_FLAG] as u16 as u32;
                adjust = false;
                if kind == 4 {
                    o.call(m, &outside::SI_ED_ADD_CAMERA, &[sub.addr(), node.addr(), unique]);
                } else {
                    o.call(m, &outside::SI_ED_ADD_LIGHT, &[sub.addr(), node.addr(), unique]);
                    if m[SCHEM_NO_REFRESH] == 0 {
                        o.call(m, &outside::SI_SCHEMATIC_ADJUST_NEW_NODE, &[list.addr(), node.addr()]);
                    }
                    if select != 0 {
                        m[node].selection = if m[list].multi == 0 { 0 } else { 1 };
                        m[sub].selection = 1;
                        if m[list].multi == 0 {
                            take_over_selection(m, o, list, sub);
                        }
                        select = 0;
                    }
                }
            }
            8 | 0x13 | 0x14 => {
                let unique = m[SI_UNIQUE_NAME_FLAG] as u16 as u32;
                o.call(m, &outside::SI_ED_ADD_LIGHT, &[node.addr(), 0, unique]);
            }
            0xe | 0xf | 0x10 => {
                let routine = match kind {
                    0xe => &outside::SI_ED_ADD_MATERIAL,
                    0xf => &outside::SI_ED_ADD_TEXTURE_2D,
                    _ => &outside::SI_ED_ADD_TEXTURE_3D,
                };
                let object = m[node].data.addr();
                let unique = m[SI_UNIQUE_NAME_FLAG] as u16 as u32;
                if o.call(m, routine, &[node.addr(), unique, object]) as i16 == 0 {
                    graflist_remove_grafnode(m, o, list, node);
                    m.free(node.addr());
                    return Ptr32::NULL;
                }
            }
            0xb => {
                o.call(m, &outside::SI_ED_ADD_WAVE, &[node.addr(), 0]);
            }
            _ => {}
        }
        if adjust && m[SCHEM_NO_REFRESH] == 0 {
            o.call(m, &outside::SI_SCHEMATIC_ADJUST_NEW_NODE, &[list.addr(), node.addr()]);
        }
        if m[list].multi == 0 && select != 0 {
            take_over_selection(m, o, list, node);
        }
    }
    o.call(m, &outside::SI_ADJUST_GN_FUNC, &[node.addr()]);
    o.call(m, &outside::SI_DOPE_FLAGS_SET, &[1, 1, 0, 0]);
    o.call(m, &outside::SI_SPRD_UPDATE_SELECT, &[]);
    node
}

// ---------------------------------------------------------------- hierarchy changes

/// The last node of a node's branch: `nb_sub` steps along the chain (a 16-bit count).
fn branch_end(m: &Mem, node: Ptr32<GrafNode>) -> Ptr32<GrafNode> {
    let mut last = node;
    let mut n = m[node].nb_sub as u16;
    while n != 0 {
        last = m[last].next;
        n -= 1;
        if m.fault().is_some() {
            break;
        }
    }
    last
}

/// Makes a node of a hierarchy a root again in the list
/// [soft 0x0833562c SI_graflistCutModel]: `node` with the `nb_sub` nodes after it is
/// taken out of the chain and put at the **head** of the list, as kind 2. `father` is the
/// model it hung under. Answers 0.
///
/// The node before it and the list's head are written without a test for null (the node
/// is expected to be inside a hierarchy and the list not empty). Then `SI_edCutModel
/// (0x080fe5ec)` (father, node, the 16-bit `flag`); every model from `father` upwards
/// whose node is found (`SI_findGlData`) has its count lowered by one more than the cut
/// node's; and for the cut node's element and the father's (`SI_elemdictFindElemFromGrafNode
/// (0x080fcbec)`), where there is one, `SI_schematicHierOperation (0x0833c5dc)` with
/// (null, the current dictionary, the element, the chapter the second look-up stored,
/// 0x16).
///
/// The reduced product's call of `SI_edSetEditable (0x08107764)` for a node that is not
/// editable is behind `SI_lightLicense` and left out (the full product's path).
pub fn graflist_cut_model(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, node: Ptr32<GrafNode>, father: Ptr32<Model>, flag: i16) -> i32 {
    let index = m.local(4);
    let chap = m.local(4);
    let last = branch_end(m, node);
    let before = m[node].prev;
    m[before].next = m[last].next;
    let after = m[last].next;
    if !after.is_null() {
        m[after].prev = m[node].prev;
    }
    m[node].kind = 2;
    let head = m[list].first;
    m[head].prev = last;
    m[last].next = m[list].first;
    m[list].first = node;
    m[node].prev = Ptr32::NULL;
    o.call(m, &outside::SI_ED_CUT_MODEL, &[father.addr(), node.addr(), flag as u16 as u32]);
    let mut up = father;
    while !up.is_null() {
        let g = find_gl_data(m, list, up.cast(), 0);
        if !g.is_null() {
            m[g].nb_sub = (m[g].nb_sub as u16).wrapping_sub(1).wrapping_sub(m[node].nb_sub as u16) as i16;
        }
        up = m[up].father;
        if m.fault().is_some() {
            break;
        }
    }
    let own = o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]);
    let g = find_gl_data(m, list, father.cast(), 0);
    let fathers = o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[g.addr(), chap, index]);
    for elem in [own, fathers] {
        if elem != 0 {
            let ed = current_dict(m);
            let c = m.u32(chap);
            o.call(m, &outside::SI_SCHEMATIC_HIER_OPERATION, &[0, ed, elem, c, 0x16]);
        }
    }
    m.release(8);
    0
}

/// Hangs a root node under another node in the list
/// [soft 0x083353fc SI_graflistLinkModel]: `child` with the `nb_sub` nodes after it is
/// taken out of the chain and put **right behind `parent`**, as kind 3. Answers 0.
///
/// Before the chain is touched the two elements are looked up
/// (`SI_elemdictFindElemFromGrafNode (0x080fcbec)`: the child's, then the parent's), and
/// the element at the parent's root index (+0x50, when not negative) through
/// `SI_elemdictGetElem (0x08132b50)`. After it: `SI_edLinkModel (0x080fe41c)` (parent,
/// child, the 16-bit `flag` with its sign); every model from the parent's upwards has the
/// count of its node raised by one more than the child's (the node is not tested for
/// null); with `select` the parent gets selection 1 and goes to `SI_selectGrafnode` in
/// mode 1 (the full product's path: the reduced product leaves a parent that is not
/// editable alone).
///
/// Then, when the schematic view's word at +0x48 is set, the root element was found and
/// `schemNoRefresh` is not set: `si_schematicRightReadjustX (0x08350c50)` with the child's
/// element, the chapter, a distance and 0; bit 0 of the element's byte at +0x2a is
/// cleared; `SI_schematicHierOperation (0x0833c5dc)` with (null, dictionary, the root
/// element, the chapter, 0x14). The distance is minus the size of the element's box along
/// the view's layout direction (`SI_glbSchem`'s first word: 0 the floats at +0x34 and
/// +0x2c, 1 those at +0x38 and +0x30), stored as a float, less 0.5 or 1.5, stored as a
/// float again. For any other layout direction the original passes a float of its stack
/// that nobody set; the port passes 0.
///
/// Last `SI_dopeFlagsSet (0x081fa5d0)` with (1, 1, 1, 1).
pub fn graflist_link_model(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, parent: Ptr32<GrafNode>, child: Ptr32<GrafNode>, select: i16, flag: i16) -> i32 {
    let index = m.local(4);
    let chap = m.local(4);
    let child_elem: Ptr32<Elem> = Ptr32::new(o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[child.addr(), chap, index]));
    let parent_elem: Ptr32<Elem> = Ptr32::new(o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[parent.addr(), chap, index]));
    let mut root_elem = 0;
    if !parent_elem.is_null() {
        let root = m[parent_elem].root_id;
        if root >= 0 {
            let ed = current_dict(m);
            let c = m.u32(chap);
            root_elem = o.call(m, &outside::SI_ELEMDICT_GET_ELEM, &[ed, c, root as u32]);
        }
    }
    let before = m[child].prev;
    let last = branch_end(m, child);
    if before.is_null() {
        m[list].first = m[last].next;
    } else {
        m[before].next = m[last].next;
    }
    let after = m[last].next;
    if !after.is_null() {
        m[after].prev = m[child].prev;
    }
    let behind = m[parent].next;
    if !behind.is_null() {
        m[behind].prev = last;
    }
    m[last].next = m[parent].next;
    m[parent].next = child;
    m[child].prev = parent;
    m[child].kind = 3;
    o.call(m, &outside::SI_ED_LINK_MODEL, &[parent.addr(), child.addr(), flag as u16 as u32]);
    let mut up: Ptr32<Model> = m[parent].data.cast();
    while !up.is_null() {
        let g = find_gl_data(m, list, up.cast(), 0);
        m[g].nb_sub = (m[g].nb_sub as u16).wrapping_add(1).wrapping_add(m[child].nb_sub as u16) as i16;
        up = m[up].father;
        if m.fault().is_some() {
            break;
        }
    }
    if select != 0 {
        // the full product's path: also for a parent that is not editable
        m[parent].selection = 1;
        select_grafnode(m, o, list, parent, 1);
    }
    if m.i16(SI_GLB_SCHEM.addr() + GLB_SCHEM_48) != 0 && root_elem != 0 && m[SCHEM_NO_REFRESH] == 0 {
        let b = m[child_elem].schem_bbox;
        let distance = match m.i32(SI_GLB_SCHEM.addr()) {
            0 => Some((b[2], b[0], 0.5f64)),
            1 => Some((b[3], b[1], 1.5f64)),
            _ => None,
        };
        // on the x87 unit: the difference is rounded once, when it is stored as a float;
        // so is that float less the constant
        let bits = match distance {
            Some((hi, lo, less)) => {
                let size = (-(F80::from_f32(hi) - F80::from_f32(lo)).abs()).to_f32();
                (F80::from_f32(size) - F80::from_f64(less)).to_f32().to_bits()
            }
            None => 0,
        };
        let c = m.u32(chap);
        o.call(m, &outside::SI_SCHEMATIC_RIGHT_READJUST_X, &[child_elem.addr(), c, bits, 0]);
        m[child_elem].schem_flags &= 0xfe;
        let ed = current_dict(m);
        let c = m.u32(chap);
        o.call(m, &outside::SI_SCHEMATIC_HIER_OPERATION, &[0, ed, root_elem, c, 0x14]);
    }
    o.call(m, &outside::SI_DOPE_FLAGS_SET, &[1, 1, 1, 1]);
    m.release(8);
    0
}

// ---------------------------------------------------------------- deleting an object

/// The relations a model that is being deleted is taken out of, one call of
/// `SI_edRelaxRelFromDelModel (0x080ff5f0)` each, in this order.
const RELAXED: [u32; 7] = [0xf5, 0xfa, 0xfb, 0xfc, 0xfd, 0x100, 0x101];

/// What every model node gets when it is deleted, the node asked for and the nodes below
/// it alike (the node asked for has two calls more, see `SI_graflistDeleteGrafnode`).
fn delete_model_part(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, model: Ptr32<Model>, fittings: bool) {
    for rel in RELAXED {
        o.call(m, &outside::SI_ED_RELAX_REL_FROM_DEL_MODEL, &[model.addr(), rel]);
    }
    if fittings {
        o.call(m, &outside::SI_MODEL_REMOVE_ALL_FITTINGS, &[model.addr()]);
    }
    delete_q_stretch_ctrls(m, o, list, model);
    if m[model].kind == 1 {
        o.call(m, &outside::SI_PATCH_REMOVE_FITTING, &[model.addr()]);
    }
}

/// The custom effects' part of it: `SI_cusDeleteModel (0x08549570)` when the model's
/// 16-bit word at +0x350 is above 0, `SI_cusDeleteIcon (0x0854a094)` for algorithm 0xd.
fn delete_model_custom(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) {
    if m[model].unk_350 > 0 {
        o.call(m, &outside::SI_CUS_DELETE_MODEL, &[model.addr()]);
    }
    if m[model].algo_type == 0xd {
        o.call(m, &outside::SI_CUS_DELETE_ICON, &[model.addr()]);
    }
}

/// The custom effects are told that a node's element goes: its element is looked up
/// (`SI_elemdictFindElemFromGrafNode (0x080fcbec)`), then `SI_cusElemCallbackCallAll
/// (0x0854b614)` (chapter, index, element, 0) and
/// `SI_cusElemCallbackUpdateEffectsForDeletion (0x0854bc44)` (chapter, element). Answers
/// the element and its chapter.
fn announce_deletion(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<GrafNode>, chap: u32, index: u32) -> (u32, u32) {
    let elem = o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]);
    let (c, i) = (m.u32(chap), m.u32(index));
    o.call(m, &outside::SI_CUS_ELEM_CALLBACK_CALL_ALL, &[c, i, elem, 0]);
    let c = m.u32(chap);
    o.call(m, &outside::SI_CUS_ELEM_CALLBACK_UPDATE_EFFECTS_FOR_DELETION, &[c, elem]);
    (elem, c)
}

/// Deletes the two control nodes of a quick-stretch set-up on a model
/// [soft 0x0832cc28 deleteQStretchCtrls]. The model's element (`SI_edGetElemFromModel
/// (0x080fcf20)`; nothing without one) is asked twice for the first element related to it
/// by relation 0xee in list 1 of the backward side, in the models' chapter
/// (`SI_elemdictFindRelatedElem (0x0813508c)`; the first deletion is what makes the second
/// question find the other one). Each time: the element's graph node (its word at +8),
/// when it is a node of kind 3 with a model, is cut out of its hierarchy
/// (`SI_modelCutFather (0x0846d790)`, then `SI_graflistCutModel` with its father and 0),
/// and then, **whatever it is, also null**, handed to `SI_graflistDeleteGrafnode`.
pub fn delete_q_stretch_ctrls(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, model: Ptr32<Model>) {
    let index = m.local(4);
    let chap = m.local(4);
    let elem = o.call(m, &outside::SI_ED_GET_ELEM_FROM_MODEL, &[model.addr(), index]);
    if elem != 0 {
        for _ in 0..2 {
            let models = m[SI_CID][cid::MODELS] as u32;
            let ed = current_dict(m);
            let related: Ptr32<Elem> = Ptr32::new(o.call(m, &outside::SI_ELEMDICT_FIND_RELATED_ELEM, &[ed, elem, 0, 1, 0xee, models, chap, index]));
            if related.is_null() {
                break;
            }
            let g: Ptr32<GrafNode> = m[related].aux_data.cast();
            if !g.is_null() && !m[g].data.is_null() && m[g].kind == 3 {
                let control: Ptr32<Model> = m[g].data.cast();
                let father = m[control].father;
                o.call(m, &outside::SI_MODEL_CUT_FATHER, &[control.addr()]);
                graflist_cut_model(m, o, list, g, father, 0);
            }
            graflist_delete_grafnode(m, o, list, g);
            if m.fault().is_some() {
                break;
            }
        }
    }
    m.release(8);
}

/// Deletes a node with what it stands for, and the nodes below it
/// [soft 0x0832cd50 SI_graflistDeleteGrafnode]. Answers 0. In this order:
///
/// 1. The custom effects are told (`SI_cusElemCallbackCallAll (0x0854b614)`,
///    `SI_cusElemCallbackUpdateEffectsForDeletion (0x0854bc44)`). When a schematic view is
///    laid out and the node has an element, what lies to its right is moved up by the
///    element's box size along the layout direction plus 0.5 or 1.5
///    (`si_schematicRightReadjustX (0x08350c50)` with the element, its chapter, minus that
///    distance as a float, and whether `schemNoRefresh` is clear). For a layout direction
///    other than 0 and 1 the original passes a float of its stack nobody set; the port
///    passes minus zero.
/// 2. `SI_grafnodeCutConnections`.
/// 3. By kind. **A model node**: `SI_fastSetDirty (0x08329898)` unless `schemNoRefresh`;
///    `SI_modelUnlinkAllWaves (0x084c7018)`; `modelUnlinkAllLight` unless
///    `SI_edSumatraTo3x` is set; seven relations relaxed (`SI_edRelaxRelFromDelModel
///    (0x080ff5f0)`); `SI_modelRemoveAllFittings (0x08476d0c)`; `deleteQStretchCtrls`;
///    `SI_patchRemoveFitting (0x0843a580)` for a patch; the custom effects' two calls; and
///    when the node is the one the manipulation globals refer to (+0x18), that reference
///    is cleared, the mode words at +8 (3 becomes 2) and +0xc (3 becomes 4, and +0x14
///    with it) are stepped back and `SI_modeBoxRefresh (0x083a6380)` called. **A light**
///    (6, 7, 8, 0x13, 0x14): its array of slots at +0x168 is cleared over the count at
///    +0x164, then `SI_edDisposeLight (0x08101f18)`. **A wave**: `SI_waveDispose
///    (0x084c6f68)` through the node's object word. **A material, a 2D or a 3D texture**:
///    `SI_edDisposeMaterial (0x08102114)` (with 1), `SI_edDisposeTexture2D (0x0810253c)`,
///    `SI_edDisposeTexture3D (0x081028d0)`, and the object word cleared. **An animation
///    file** (0x12), unless `schemNoRefresh`: `SI_edDisposeAnimFile (0x080efc64)` with the
///    node, and the object word cleared. **A named selection** (0xc): the object word
///    cleared. Other kinds: nothing.
/// 4. While the node's count is not 0: the node behind it has the custom effects told,
///    its connections cut, and, when it is a model node, the same calls as in 3 without
///    `SI_fastSetDirty`, without the fittings, without the test of `SI_edSumatraTo3x` and
///    without the manipulation globals (a node of kind 6 has its object word cleared);
///    then it is taken out of the chain (`SI_graflistRemoveGrafnode`) and the count
///    lowered by one. The count is a 16-bit word that is decremented until it was 1.
/// 5. The node itself is taken out of the chain; nothing is freed here. A named selection
///    has `SI_fcvScrollBuild (0x083978d8)` called.
/// 6. Unless a history operation is running (`SI_historyActive`) or `SI_edSumatraTo3x` is
///    set, `SI_grafnodeSaveHistoryDelete (0x080c973c)` keeps the node for undo when it is a
///    model node whose model's algorithm was not 0xd, or a light of kind 7, 8, 0x13 or
///    0x14.
/// 7. `SI_dopeFlagsSet (0x081fa5d0)` (1, 1, 1, 1) and `SI_schemComputeHierarchiesBBox
///    (0x0833bd74)` (the current dictionary, 1, 0).
pub fn graflist_delete_grafnode(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, node: Ptr32<GrafNode>) -> i32 {
    let sub_index = m.local(4);
    let sub_chap = m.local(4);
    let index = m.local(4);
    let chap = m.local(4);
    let (elem, c) = announce_deletion(m, o, node, chap, index);
    let elem: Ptr32<Elem> = Ptr32::new(elem);
    if m.i16(SI_GLB_SCHEM.addr() + GLB_SCHEM_48) != 0 && !elem.is_null() {
        let b = m[elem].schem_bbox;
        let size = match m.i32(SI_GLB_SCHEM.addr()) {
            0 => Some((b[2], b[0], 0.5f64)),
            1 => Some((b[3], b[1], 1.5f64)),
            _ => None,
        };
        // on the x87 unit: the difference is stored as a float, then the constant is
        // added and the sum stored as a float again
        let bits = match size {
            Some((hi, lo, more)) => {
                let d = (F80::from_f32(hi) - F80::from_f32(lo)).to_f32();
                (-F80::from_f32((F80::from_f64(more) + F80::from_f32(d)).to_f32())).to_f32().to_bits()
            }
            None => 0x8000_0000,
        };
        let refresh = (m[SCHEM_NO_REFRESH] == 0) as u32;
        o.call(m, &outside::SI_SCHEMATIC_RIGHT_READJUST_X, &[elem.addr(), c, bits, refresh]);
    }
    grafnode_cut_connections(m, o, node);
    let mut algo = 0;
    match m[node].kind {
        2 | 3 => {
            if m[SCHEM_NO_REFRESH] == 0 {
                o.call(m, &outside::SI_FAST_SET_DIRTY, &[0, 0xffff_ffff, 1]);
            }
            let model: Ptr32<Model> = m[node].data.cast();
            algo = m[model].algo_type;
            o.call(m, &outside::SI_MODEL_UNLINK_ALL_WAVES, &[model.addr()]);
            if m.i16(SI_ED_SUMATRA_TO_3X.addr()) == 0 {
                model_unlink_all_light(m, o, model);
            }
            delete_model_part(m, o, list, model, true);
            delete_model_custom(m, o, model);
            if m.u32(SI_GLB_MANIP.addr() + GLB_MANIP_18) == node.addr() {
                let manip = SI_GLB_MANIP.addr();
                m.set_u32(manip + GLB_MANIP_18, 0);
                if m.u32(manip + GLB_MANIP_08) == 3 {
                    m.set_u32(manip + GLB_MANIP_08, 2);
                }
                if m.u32(manip + GLB_MANIP_0C) == 3 {
                    m.set_u32(manip + GLB_MANIP_0C, 4);
                    m.set_u32(manip + GLB_MANIP_14, 4);
                }
                o.call(m, &outside::SI_MODE_BOX_REFRESH, &[]);
            }
        }
        6 | 7 | 8 | 0x13 | 0x14 => {
            let light = m[node].data.addr();
            if m.u32(light.wrapping_add(0x168)) != 0 {
                let mut i: i32 = 0;
                while i < m.i32(light.wrapping_add(0x164)) {
                    let slots = m.u32(light.wrapping_add(0x168));
                    m.set_u32(slots.wrapping_add((i as u32).wrapping_mul(4)), 0);
                    i += 1;
                    if m.fault().is_some() {
                        break;
                    }
                }
            }
            o.call(m, &outside::SI_ED_DISPOSE_LIGHT, &[light]);
        }
        0xb => {
            o.call(m, &outside::SI_WAVE_DISPOSE, &[node.addr().wrapping_add(4)]);
        }
        0xc => m[node].data = Ptr32::NULL,
        0xe => {
            let object = m[node].data.addr();
            o.call(m, &outside::SI_ED_DISPOSE_MATERIAL, &[object, 1]);
            m[node].data = Ptr32::NULL;
        }
        0xf => {
            let object = m[node].data.addr();
            o.call(m, &outside::SI_ED_DISPOSE_TEXTURE_2D, &[object]);
            m[node].data = Ptr32::NULL;
        }
        0x10 => {
            let object = m[node].data.addr();
            o.call(m, &outside::SI_ED_DISPOSE_TEXTURE_3D, &[object]);
            m[node].data = Ptr32::NULL;
        }
        0x12 => {
            if m[SCHEM_NO_REFRESH] == 0 {
                o.call(m, &outside::SI_ED_DISPOSE_ANIM_FILE, &[node.addr()]);
                m[node].data = Ptr32::NULL;
            }
        }
        _ => {}
    }
    if m[node].nb_sub != 0 {
        loop {
            let sub = m[node].next;
            announce_deletion(m, o, sub, sub_chap, sub_index);
            let sub = m[node].next;
            grafnode_cut_connections(m, o, sub);
            let sub = m[node].next;
            let k = m[sub].kind;
            if k == 2 || k == 3 {
                let model: Ptr32<Model> = m[sub].data.cast();
                o.call(m, &outside::SI_MODEL_UNLINK_ALL_WAVES, &[model.addr()]);
                model_unlink_all_light(m, o, model);
                delete_model_part(m, o, list, model, false);
                let model: Ptr32<Model> = m[m[node].next].data.cast();
                delete_model_custom(m, o, model);
            } else if k == 6 {
                m[sub].data = Ptr32::NULL;
            }
            let sub = m[node].next;
            graflist_remove_grafnode(m, o, list, sub);
            let n = m[node].nb_sub;
            m[node].nb_sub = n.wrapping_sub(1);
            if n == 1 || m.fault().is_some() {
                break;
            }
        }
    }
    graflist_remove_grafnode(m, o, list, node);
    if m[node].kind == 0xc {
        o.call(m, &outside::SI_FCV_SCROLL_BUILD, &[]);
    }
    if m[SI_HISTORY_ACTIVE] == 0 && m.i16(SI_ED_SUMATRA_TO_3X.addr()) == 0 {
        let k = m[node].kind;
        if ((k.wrapping_sub(2) as u32) <= 1 && algo != 0xd) || matches!(k, 7 | 8 | 0x13 | 0x14) {
            o.call(m, &outside::SI_GRAFNODE_SAVE_HISTORY_DELETE, &[node.addr()]);
        }
    }
    o.call(m, &outside::SI_DOPE_FLAGS_SET, &[1, 1, 1, 1]);
    let ed = current_dict(m);
    o.call(m, &outside::SI_SCHEM_COMPUTE_HIERARCHIES_BBOX, &[ed, 1, 0]);
    m.release(16);
    0
}

// ---------------------------------------------------------------- the Delete command

/// `((y × b + x × a) + z × c) + t` on the x87 unit, rounded once when it is stored as a
/// float: one coordinate of a point times a matrix, as `SI_graflistDeleteNode` has it
/// inline (the products of the first and second coordinate are added first, second
/// operand first).
fn row_times_column(x: f32, y: f32, z: f32, a: f32, b: f32, c: f32, t: f32) -> f32 {
    let xa = F80::from_f32(x) * F80::from_f32(a);
    let yb = F80::from_f32(y) * F80::from_f32(b);
    let zc = F80::from_f32(z) * F80::from_f32(c);
    (((yb + xa) + zc) + F80::from_f32(t)).to_f32()
}

/// The Delete command for one node [soft 0x0832d75c SI_graflistDeleteNode]: `branch` says
/// whether the nodes below it go with it. Answers 0. `schemNoRefresh` is saved at entry
/// and set again, through `SI_schematicNoRefresh`, as the last thing.
///
/// First `SI_schematicCheckDeleteExpand (0x08350414)` (node, `branch`). Then by kind:
///
/// - **A model node without `branch` that has a son: only its geometry goes.** Nothing
///   for a model of kind 7. Else `SI_modelResetGeomRelatedAnim (0x082216b4)` (the current
///   dictionary, model), `SI_modelTouchGeometry (0x0846d48c)`, `SI_modelSaveHistoryGeometry
///   (0x080c8898)` (node, 1), `SI_modelDynaInfoDispose (0x084a1248)` (model, 1),
///   `SI_modelDeleteDefinition (0x084678a8)` (model, 0); the node's selection becomes 0 and
///   the three selection bits of its flags are cleared; `SI_updateGnFunc (0x08176618)`;
///   `SI_schemComputeHierarchiesBBox (0x0833bd74)` (dictionary, 1, 0).
/// - **Any other model node** is deleted with what is below it. A node of kind 3 is first
///   cut out of its hierarchy:
///   - `SI_modelSaveHistoryParenting (0x080ca4d4)` (node, the node of the model's father,
///     1) unless the node is not editable;
///   - the root model (`SI_modelRoot (0x0847838c)`) and each of its sons
///     (`SI_modelNextSon (0x0846d70c)`) get bit 0x20 in the byte at +0x0a;
///   - when `SI_hrchCpst` is set, **the node's local transformation is composed with its
///     father's global one**: each scaling factor is multiplied by the father's global
///     scaling; the rotation matrix becomes `SI_matrixMultiplySub3 (0x0851650c)` of
///     itself and the father's global rotation, and the angles are taken from it again
///     (`SI_matrixExtractXyzRotation (0x085172b0)`); the translation becomes the point it
///     is, times the father's global matrix (rows 0 to 2 by the point, plus row 3). Every
///     product and sum is made on the x87 unit and rounded once, when stored; the three
///     new coordinates are all computed from the old ones;
///   - the node's element (`SI_elemdictFindElemFromGrafNode (0x080fcbec)`; **not tested
///     for null**) names its root's index, whose element is fetched (`SI_elemdictGetElem
///     (0x08132b50)`); `SI_modelCutFather (0x0846d790)`; `SI_graflistCutModel` (node, the
///     father, 0); the model's node is looked up again and is the one deleted; with a
///     root element, `SI_schematicHierOperation (0x0833c5dc)` (null, dictionary, the root
///     element, the chapter, 0x14).
///
///   Then `SI_graflistDeleteGrafnode` between `SI_schematicNoRefresh(1)` and `(0)`, and
///   `SI_schemComputeHierarchiesBBox` (dictionary, 1, 0).
/// - **A light's own node** (6): the node before it, the one in front of the light, is
///   deleted.
/// - **Any other node** is deleted as it is.
pub fn graflist_delete_node(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, node: Ptr32<GrafNode>, branch: i16) -> i32 {
    let mut node = node;
    let saved = m[SCHEM_NO_REFRESH];
    o.call(m, &outside::SI_SCHEMATIC_CHECK_DELETE_EXPAND, &[node.addr(), branch as u16 as u32]);
    let kind = m[node].kind;
    if kind == 2 || kind == 3 {
        let model: Ptr32<Model> = m[node].data.cast();
        if branch == 0 && !m[model].son.is_null() {
            if m[model].kind != 7 {
                let ed = current_dict(m);
                o.call(m, &outside::SI_MODEL_RESET_GEOM_RELATED_ANIM, &[ed, model.addr()]);
                o.call(m, &outside::SI_MODEL_TOUCH_GEOMETRY, &[model.addr()]);
                o.call(m, &outside::SI_MODEL_SAVE_HISTORY_GEOMETRY, &[node.addr(), 1]);
                o.call(m, &outside::SI_MODEL_DYNA_INFO_DISPOSE, &[model.addr(), 1]);
                let data = m[node].data.addr();
                o.call(m, &outside::SI_MODEL_DELETE_DEFINITION, &[data, 0]);
                m[node].selection = 0;
                m[node].flags &= 0x1fff;
                o.call(m, &outside::SI_UPDATE_GN_FUNC, &[node.addr()]);
                let ed = current_dict(m);
                o.call(m, &outside::SI_SCHEM_COMPUTE_HIERARCHIES_BBOX, &[ed, 1, 0]);
            }
        } else {
            if kind == 3 {
                let above = m[model].father;
                let g = find_gl_data(m, list, above.cast(), 0);
                if m[node].flags & crate::records::flag::UNEDITABLE == 0 {
                    o.call(m, &outside::SI_MODEL_SAVE_HISTORY_PARENTING, &[node.addr(), g.addr(), 1]);
                }
                let root = o.call(m, &outside::SI_MODEL_ROOT, &[model.addr()]);
                let b = m.u8(root.wrapping_add(0xa));
                m.set_u8(root.wrapping_add(0xa), b | 0x20);
                let mut son = o.call(m, &outside::SI_MODEL_NEXT_SON, &[root, 0]);
                let index = m.local(4);
                let chap = m.local(4);
                while son != 0 && m.fault().is_none() {
                    let b = m.u8(son.wrapping_add(0xa));
                    m.set_u8(son.wrapping_add(0xa), b | 0x20);
                    son = o.call(m, &outside::SI_MODEL_NEXT_SON, &[root, son]);
                }
                let father = m[model].father;
                if m[SI_HRCH_CPST] != 0 {
                    let (s, gs) = (m[model].scal, m[father].gscl);
                    m[model].scal.x = (F80::from_f32(s.x) * F80::from_f32(gs.x)).to_f32();
                    m[model].scal.y = (F80::from_f32(s.y) * F80::from_f32(gs.y)).to_f32();
                    m[model].scal.z = (F80::from_f32(s.z) * F80::from_f32(gs.z)).to_f32();
                    let mrot = model.addr().wrapping_add(0x118);
                    o.call(m, &outside::SI_MATRIX_MULTIPLY_SUB3, &[mrot, father.addr().wrapping_add(0x178), mrot]);
                    o.call(m, &outside::SI_MATRIX_EXTRACT_XYZ_ROTATION, &[mrot, model.addr().wrapping_add(0x10c)]);
                    let p = m[model].trans;
                    let g = m[father].gmat;
                    m[model].trans.x = row_times_column(p.x, p.y, p.z, g[0][0], g[1][0], g[2][0], g[3][0]);
                    let g = m[father].gmat;
                    m[model].trans.y = row_times_column(p.x, p.y, p.z, g[0][1], g[1][1], g[2][1], g[3][1]);
                    let g = m[father].gmat;
                    m[model].trans.z = row_times_column(p.x, p.y, p.z, g[0][2], g[1][2], g[2][2], g[3][2]);
                }
                let elem = o.call(m, &outside::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]);
                let root_id = m.u32(elem.wrapping_add(0x50));
                let ed = current_dict(m);
                let c = m.u32(chap);
                let root_elem = o.call(m, &outside::SI_ELEMDICT_GET_ELEM, &[ed, c, root_id]);
                o.call(m, &outside::SI_MODEL_CUT_FATHER, &[model.addr()]);
                graflist_cut_model(m, o, list, node, father, 0);
                node = find_gl_data(m, list, model.cast(), 0);
                if root_elem != 0 {
                    let ed = current_dict(m);
                    let c = m.u32(chap);
                    o.call(m, &outside::SI_SCHEMATIC_HIER_OPERATION, &[0, ed, root_elem, c, 0x14]);
                }
                m.release(8);
            }
            super::list::schematic_no_refresh(m, 1);
            graflist_delete_grafnode(m, o, list, node);
            super::list::schematic_no_refresh(m, 0);
            let ed = current_dict(m);
            o.call(m, &outside::SI_SCHEM_COMPUTE_HIERARCHIES_BBOX, &[ed, 1, 0]);
        }
    } else if kind == 6 {
        let front = m[node].prev;
        graflist_delete_grafnode(m, o, list, front);
    } else {
        graflist_delete_grafnode(m, o, list, node);
    }
    super::list::schematic_no_refresh(m, saved);
    0
}
