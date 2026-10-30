// SPDX-License-Identifier: MIT
//! Selection state and display type of the nodes: the counts and the routines behind the
//! display menus.

use super::list::is_model;
use crate::globals::*;
use crate::outside::{self, Outside};
use crate::records::flag;
use si_mem::{Mem, Ptr32};
use si_types::dict::Elem;
use si_types::model::Model;
use si_types::scene::{GrafList, GrafNode};

/// Whether a node is selected as a node or as a branch
/// [soft 0x0832db44 SI_grafNodeIsSelected]: its selection word is 1 or 3. 0 for null.
pub fn graf_node_is_selected(m: &Mem, node: Ptr32<GrafNode>) -> i32 {
    if node.is_null() {
        return 0;
    }
    let s = m[node].selection;
    (s == 1 || s == 3) as i32
}

/// The same for a node that is not editable
/// [soft 0x0832db6c SI_grafNodeIsWrathSelected]: flag 0x2000 or 0x4000. 0 for null.
pub fn graf_node_is_wrath_selected(m: &Mem, node: Ptr32<GrafNode>) -> i32 {
    if node.is_null() {
        return 0;
    }
    (m[node].flags & (flag::SEL_NODE | flag::SEL_BRANCH) != 0) as i32
}

/// How many nodes of a list are selected [soft 0x0832db90 SI_graflistCountSelected], and
/// through `first` (when not null) the first of them, or null. With `wrath` a node that
/// is not editable counts by its flags.
pub fn graflist_count_selected(m: &mut Mem, list: Ptr32<GrafList>, first: Ptr32<Ptr32<GrafNode>>, wrath: i16) -> i32 {
    let mut count = 0;
    let mut node = m[list].first;
    if !first.is_null() {
        m[first] = Ptr32::NULL;
    }
    while !node.is_null() {
        let selected = graf_node_is_selected(m, node) as i16 != 0
            || (wrath != 0 && m[node].flags & flag::UNEDITABLE != 0 && graf_node_is_wrath_selected(m, node) as i16 != 0);
        if selected {
            count += 1;
            if count == 1 && !first.is_null() {
                m[first] = node;
            }
        }
        node = m[node].next;
    }
    count
}

/// How many hierarchies of a list have something selected
/// [soft 0x0832dc0c SI_graflistCountSelHierarchies]. The list is taken as a row of
/// hierarchies: a node and the `nb_sub` nodes after it. A hierarchy counts when one of
/// its nodes has a selection word other than 0 or, with `wrath`, is not editable and has
/// one of the three selection flags. The chain is followed for as many nodes as the count
/// says without a test for its end (a count larger than what is left reads through null:
/// a recorded fault here).
pub fn graflist_count_sel_hierarchies(m: &Mem, list: Ptr32<GrafList>, wrath: i16) -> i32 {
    let mut count = 0;
    let mut node = m[list].first;
    while !node.is_null() {
        let mut found = false;
        let mut left = m[node].nb_sub as u16 as i32 + 1;
        while left > 0 {
            let f = m[node].flags;
            if m[node].selection != 0 || (wrath != 0 && f & flag::UNEDITABLE != 0 && f & flag::SEL_ANY != 0) {
                found = true;
            }
            node = m[node].next;
            left -= 1;
            if m.fault().is_some() {
                // the original is dead by now
                return count;
            }
        }
        if found {
            count += 1;
        }
    }
    count
}

/// How many selected nodes of one or two kinds a list has
/// [soft 0x0832dc74 SI_graflistTypeCount], and through `last` (when not null) the **last**
/// of them, or null. Selected: a selection word other than 0 or, with `wrath`, not
/// editable with one of the three selection flags. Two kinds of 0 accept every kind.
pub fn graflist_type_count(m: &mut Mem, list: Ptr32<GrafList>, kind1: i32, kind2: i32, last: Ptr32<Ptr32<GrafNode>>, wrath: i16) -> i32 {
    let any = kind1 == 0 && kind2 == 0;
    let mut count = 0;
    if !last.is_null() {
        m[last] = Ptr32::NULL;
    }
    let mut node = m[list].first;
    while !node.is_null() {
        let f = m[node].flags;
        let selected = m[node].selection != 0 || (wrath != 0 && f & flag::UNEDITABLE != 0 && f & flag::SEL_ANY != 0);
        if selected {
            let k = m[node].kind;
            if any || k == kind1 || k == kind2 {
                count += 1;
                if !last.is_null() {
                    m[last] = node;
                }
            }
        }
        node = m[node].next;
    }
    count
}

/// Whether a node counts as selected for the display menus: a selection word other than
/// 0 or any of the three selection flags (the flags are looked at whether or not the node
/// is editable).
#[inline]
fn menu_selected(m: &Mem, node: Ptr32<GrafNode>) -> bool {
    m[node].selection != 0 || m[node].flags & flag::SEL_ANY != 0
}

/// Every node visible [soft 0x0832dcf8 graflistAllVisible]: display type 2. Answers 0.
/// No caller was found in the binary.
pub fn graflist_all_visible(m: &mut Mem, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        m[node].wire_type = 2;
        node = m[node].next;
    }
    0
}

/// Every ghosted node visible [soft 0x0832ddf4 SI_graflistUnghostAll]: display type 1
/// becomes 2. Answers 0.
pub fn graflist_unghost_all(m: &mut Mem, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        if m[node].wire_type == 1 {
            m[node].wire_type = 2;
        }
        node = m[node].next;
    }
    0
}

/// Ghosts the visible selected nodes and shows the ghosted ones
/// [soft 0x0832de20 SI_graflistToggleGhostSelection]: a selected node whose display type
/// is not 0 gets 3 minus the low 16 bits of it (taken with sign). Answers 0.
pub fn graflist_toggle_ghost_selection(m: &mut Mem, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        if menu_selected(m, node) && m[node].wire_type != 0 {
            m[node].wire_type = 3i32.wrapping_sub(m[node].wire_type as i16 as i32);
        }
        node = m[node].next;
    }
    0
}

/// Ghosts every visible node that is not selected
/// [soft 0x0832de64 SI_graflistGhostUnselected]: display type 2 becomes 1. Answers 0.
pub fn graflist_ghost_unselected(m: &mut Mem, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        if !menu_selected(m, node) && m[node].wire_type == 2 {
            m[node].wire_type = 1;
        }
        node = m[node].next;
    }
    0
}

/// Shows every hidden node as a ghost [soft 0x0832de9c SI_graflistGhostHidden]: display
/// type 0 becomes 1. Answers 0.
pub fn graflist_ghost_hidden(m: &mut Mem, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        if m[node].wire_type == 0 {
            m[node].wire_type = 1;
        }
        node = m[node].next;
    }
    0
}

/// Every model node drawn in wire frame whatever the view's mode
/// [soft 0x0832e248 SI_graflistSetForceWireframeAll]: the model's word at +0x420 becomes 1
/// where it was 0. Answers 0.
pub fn graflist_set_force_wireframe_all(m: &mut Mem, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        if is_model(m[node].kind) {
            let model = super::list::model_of(m, node);
            if m[model].wf_display == 0 {
                m[model].wf_display = 1;
            }
        }
        node = m[node].next;
    }
    0
}

/// The opposite [soft 0x0832e288 SI_graflistResetForceWireframeAll]: the word becomes 0
/// where it was not. Answers 0.
pub fn graflist_reset_force_wireframe_all(m: &mut Mem, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        if is_model(m[node].kind) {
            let model = super::list::model_of(m, node);
            if m[model].wf_display != 0 {
                m[model].wf_display = 0;
            }
        }
        node = m[node].next;
    }
    0
}

/// Flips the switch of the selected model nodes
/// [soft 0x0832e2c8 SI_graflistToggleForceWireframeSelected]: bit 0 of the byte at +0x420
/// of the model is inverted (the other routines write the whole 16-bit word). Answers 0.
pub fn graflist_toggle_force_wireframe_selected(m: &mut Mem, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        if menu_selected(m, node) && is_model(m[node].kind) {
            let at = m[node].data.addr().wrapping_add(0x420);
            let b = m.u8(at);
            m.set_u8(at, b ^ 1);
        }
        node = m[node].next;
    }
    0
}

// ---------------------------------------------------------------- with calls outside

/// Nothing selected [soft 0x0832dd20 SI_graflistUnselectAll]: every node loses its
/// selection word and its three selection flags, the list both active nodes; then
/// `SI_dopeFlagsSet (0x081fa5d0)` with (1, 1, 0, 0) and `SI_sprdUpdateSelect (0x085aaa7c)`.
/// Answers 0.
pub fn graflist_unselect_all(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        m[node].flags &= !flag::SEL_ANY;
        m[node].selection = 0;
        node = m[node].next;
    }
    m[list].active = Ptr32::NULL;
    m[list].active2 = Ptr32::NULL;
    o.call(m, &outside::SI_DOPE_FLAGS_SET, &[1, 1, 0, 0]);
    o.call(m, &outside::SI_SPRD_UPDATE_SELECT, &[]);
    0
}

/// Switches a list between one selected node and several
/// [soft 0x0832dd70 SI_graflistSetMultiSelection]. Nothing when the list is in that mode
/// already. Going to single selection: the selected nodes are counted along the chain
/// (a node that is not editable by its flags, any node by its selection word being 1 or
/// 3); the first becomes the active node; a second one unselects everything. Answers 0.
pub fn graflist_set_multi_selection(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, multi: i16) -> i32 {
    if m[list].multi == multi {
        return 0;
    }
    m[list].multi = multi;
    if multi != 0 {
        return 0;
    }
    let mut count = 0;
    let mut node = m[list].first;
    while !node.is_null() {
        let selected = (m[node].flags & flag::UNEDITABLE != 0 && graf_node_is_wrath_selected(m, node) as i16 != 0)
            || graf_node_is_selected(m, node) as i16 != 0;
        if selected {
            count += 1;
            if count > 1 {
                graflist_unselect_all(m, o, list);
                return 0;
            }
            super::list::graflist_set_active(m, list, node);
        }
        node = m[node].next;
    }
    0
}

/// No model node drawn as its bounding box [soft 0x0832ded0 SI_unboundGlAll]: where the
/// model's word at +0x0c is set it is cleared and `SI_modelTouchGeometry (0x0846d48c)`
/// called. Answers 0.
pub fn unbound_gl_all(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        if is_model(m[node].kind) {
            let model = super::list::model_of(m, node);
            if m[model].bbox_valid != 0 {
                m[model].bbox_valid = 0;
                o.call(m, &outside::SI_MODEL_TOUCH_GEOMETRY, &[model.addr()]);
            }
        }
        node = m[node].next;
    }
    0
}

/// Every model node drawn as its bounding box [soft 0x0832e014 SI_boundGlAll]: where the
/// word is 0 it becomes 1, with the same call. Answers 0.
pub fn bound_gl_all(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        if is_model(m[node].kind) {
            let model = super::list::model_of(m, node);
            if m[model].bbox_valid == 0 {
                m[model].bbox_valid = 1;
                o.call(m, &outside::SI_MODEL_TOUCH_GEOMETRY, &[model.addr()]);
            }
        }
        node = m[node].next;
    }
    0
}

/// The bounding boxes of a hierarchy, sons before fathers
/// [soft 0x0832df1c instanceComputeBoundingBox]: for an instance (algorithm 0xf) with a
/// master (`SI_modelMasterFromInstance (0x081a7490)`) the master's hierarchy first; then,
/// when a father was given, the node's next brother with the same father; then the first
/// son with this node as father; last `SI_modelComputeBounding (0x08467e70)` of the node
/// itself. A hierarchy that contains itself never ends in the original (it runs out of
/// stack); here the walk is cut after 20,000 levels.
pub fn instance_compute_bounding_box(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, father: Ptr32<Model>) {
    bounding_box(m, o, model, father, 0);
}

fn bounding_box(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, father: Ptr32<Model>, depth: u32) {
    if depth > 20_000 || m.fault().is_some() {
        return;
    }
    if m[model].algo_type == 0xf {
        let master: Ptr32<Model> = Ptr32::new(o.call(m, &outside::SI_MODEL_MASTER_FROM_INSTANCE, &[model.addr()]));
        if !master.is_null() {
            bounding_box(m, o, master, Ptr32::NULL, depth + 1);
        }
    }
    if !father.is_null() {
        let brother = m[model].brother;
        if !brother.is_null() {
            bounding_box(m, o, brother, father, depth + 1);
        }
    }
    let son = m[model].son;
    if !son.is_null() {
        bounding_box(m, o, son, model, depth + 1);
    }
    o.call(m, &outside::SI_MODEL_COMPUTE_BOUNDING, &[model.addr()]);
}

/// Flips "drawn as its bounding box" for the selected model nodes
/// [soft 0x0832df98 SI_toggleBoundGlSelected]: bit 0 of the byte at +0x0c of the model is
/// inverted; then an instance has its master's hierarchy measured
/// (`instanceComputeBoundingBox`, nothing without a master) and any other model gets
/// `SI_modelTouchGeometry`. Answers 0.
pub fn toggle_bound_gl_selected(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>) -> i32 {
    let mut node = m[list].first;
    while !node.is_null() {
        if menu_selected(m, node) && is_model(m[node].kind) {
            let model = super::list::model_of(m, node);
            let at = model.addr().wrapping_add(0xc);
            let b = m.u8(at);
            m.set_u8(at, b ^ 1);
            if m[model].algo_type == 0xf {
                let master: Ptr32<Model> = Ptr32::new(o.call(m, &outside::SI_MODEL_MASTER_FROM_INSTANCE, &[model.addr()]));
                if !master.is_null() {
                    instance_compute_bounding_box(m, o, master, Ptr32::NULL);
                }
            } else {
                o.call(m, &outside::SI_MODEL_TOUCH_GEOMETRY, &[model.addr()]);
            }
        }
        node = m[node].next;
    }
    0
}

/// Shows every hidden node [soft 0x0832e060 SI_unhideGlAll]: for each node whose display
/// type is 0, its element (`SI_elemdictFindTrueElemFromGrafNode (0x080fcd34)`) is handed
/// to `SI_edSetLinetype (0x080ff174)` with its chapter, 2 and 0. Answers 0.
pub fn unhide_gl_all(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>) -> i32 {
    let index = m.local(4);
    let chap = m.local(4);
    let mut node = m[list].first;
    while !node.is_null() {
        if m[node].wire_type == 0 {
            let elem = o.call(m, &outside::SI_ELEMDICT_FIND_TRUE_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]);
            if elem != 0 {
                let c = m.u32(chap);
                o.call(m, &outside::SI_ED_SET_LINETYPE, &[elem, c, 2, 0]);
            }
        }
        node = m[node].next;
    }
    m.release(8);
    0
}

/// Hides every node that is not selected [soft 0x0832e308 SI_hideGlUnselected]: the same
/// two calls with display type 0. Answers 0.
pub fn hide_gl_unselected(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>) -> i32 {
    let index = m.local(4);
    let chap = m.local(4);
    let mut node = m[list].first;
    while !node.is_null() {
        if !menu_selected(m, node) {
            let elem = o.call(m, &outside::SI_ELEMDICT_FIND_TRUE_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]);
            if elem != 0 {
                let c = m.u32(chap);
                o.call(m, &outside::SI_ED_SET_LINETYPE, &[elem, c, 0, 0]);
            }
        }
        node = m[node].next;
    }
    m.release(8);
    0
}

/// The kinds `SI_setSelectability` deals with: model nodes, cameras, lights, waves and
/// named selections.
#[inline]
fn selectable_kind(k: i32) -> bool {
    matches!(k, 2 | 3 | 4 | 5 | 6 | 7 | 8 | 0xb | 0xc | 0x13 | 0x14)
}

/// The "selectable" switch of the nodes, flag bit 0 (set: not selectable)
/// [soft 0x0832e410 SI_setSelectability], by `mode`:
///
/// - 0: flipped for the selected nodes of those kinds whose element
///   (`SI_elemdictFindTrueElemFromGrafNode`) is in use. An element of the chapter of
///   groups does not flip its own node: every entry of its first forward relation list
///   names an element (`SI_elemdictGetElem (0x08132b50)`), and the graph node of each,
///   when it is not selected, has the bit flipped.
/// - 1: set for every node of those kinds that is not selected;
/// - 3: cleared for every node of those kinds;
/// - anything else: set for every node of those kinds.
pub fn set_selectability(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, mode: i16) {
    if mode == 0 {
        let index = m.local(4);
        let chap = m.local(4);
        let mut node = m[list].first;
        while !node.is_null() {
            if menu_selected(m, node) && selectable_kind(m[node].kind) {
                let elem: Ptr32<Elem> = Ptr32::new(o.call(m, &outside::SI_ELEMDICT_FIND_TRUE_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]));
                if !elem.is_null() && m[elem].status == 0 {
                    if m.i32(chap) == m[SI_CID][cid::GROUPS] {
                        let rels = m[elem].forward.groups;
                        let mut i = 0;
                        while i < m[rels].nb {
                            let entry = m[rels].rels.add(i);
                            let (c, e) = (m[entry].chapid as i32 as u32, m[entry].elemid as u32);
                            let ed = current_dict(m);
                            let other: Ptr32<Elem> = Ptr32::new(o.call(m, &outside::SI_ELEMDICT_GET_ELEM, &[ed, c, e]));
                            if !other.is_null() {
                                let g: Ptr32<GrafNode> = m[other].aux_data.cast();
                                if !g.is_null() && m[g].selection == 0 && m[g].flags & flag::SEL_ANY == 0 {
                                    m[g].flags ^= flag::UNSELECTABLE;
                                }
                            }
                            i += 1;
                            if m.fault().is_some() {
                                break;
                            }
                        }
                    } else {
                        m[node].flags ^= flag::UNSELECTABLE;
                    }
                }
            }
            node = m[node].next;
        }
        m.release(8);
    } else if mode == 1 {
        let mut node = m[list].first;
        while !node.is_null() {
            if !menu_selected(m, node) && selectable_kind(m[node].kind) {
                m[node].flags |= flag::UNSELECTABLE;
            }
            node = m[node].next;
        }
    } else {
        let mut node = m[list].first;
        while !node.is_null() {
            if selectable_kind(m[node].kind) {
                m[node].flags &= !flag::UNSELECTABLE;
                if mode != 3 {
                    m[node].flags |= flag::UNSELECTABLE;
                }
            }
            node = m[node].next;
        }
    }
}

/// Whether a node counts as selected for the routines that unselect what they hid: by its
/// flags when it is not editable, else (or also) by its selection word being 1 or 3.
#[inline]
fn selected_either(m: &Mem, node: Ptr32<GrafNode>) -> bool {
    (m[node].flags & flag::UNEDITABLE != 0 && graf_node_is_wrath_selected(m, node) as i16 != 0) || graf_node_is_selected(m, node) as i16 != 0
}

/// Hides the selected nodes that are shown and shows those that are hidden
/// [soft 0x0832e0b8 SI_toggleHideGlSelected]. For each selected node whose element
/// (`SI_elemdictFindTrueElemFromGrafNode (0x080fcd34)`) is in use: `SI_edSetLinetype
/// (0x080ff174)` with the element, its chapter, 0 when the node's display type is not 0
/// and 2 when it is, and 0. An element of the chapter of groups is not changed itself:
/// each entry of its first forward relation list names an element (`SI_elemdictGetElem
/// (0x08132b50)`), and those whose graph node is not selected are switched the same way,
/// by their own node's display type. Then, unless `keep` is set, every selected node that
/// is now hidden is unselected. Answers 0.
pub fn toggle_hide_gl_selected(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, keep: i16) -> i32 {
    let index = m.local(4);
    let chap = m.local(4);
    let mut node = m[list].first;
    while !node.is_null() {
        if menu_selected(m, node) {
            let elem: Ptr32<Elem> = Ptr32::new(o.call(m, &outside::SI_ELEMDICT_FIND_TRUE_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]));
            if !elem.is_null() && m[elem].status == 0 {
                let c = m.i32(chap);
                if c == m[SI_CID][cid::GROUPS] {
                    let rels = m[elem].forward.groups;
                    let mut i = 0;
                    while i < m[rels].nb {
                        let entry = m[rels].rels.add(i);
                        let (ec, e) = (m[entry].chapid as i32 as u32, m[entry].elemid as u32);
                        let ed = current_dict(m);
                        let other: Ptr32<Elem> = Ptr32::new(o.call(m, &outside::SI_ELEMDICT_GET_ELEM, &[ed, ec, e]));
                        if !other.is_null() {
                            let g: Ptr32<GrafNode> = m[other].aux_data.cast();
                            if !g.is_null() && m[g].selection == 0 && m[g].flags & flag::SEL_ANY == 0 {
                                let to = if m[g].wire_type != 0 { 0 } else { 2 };
                                let ec = m[entry].chapid as i32 as u32;
                                o.call(m, &outside::SI_ED_SET_LINETYPE, &[other.addr(), ec, to, 0]);
                            }
                        }
                        i += 1;
                        if m.fault().is_some() {
                            break;
                        }
                    }
                } else {
                    let to = if m[node].wire_type != 0 { 0 } else { 2 };
                    o.call(m, &outside::SI_ED_SET_LINETYPE, &[elem.addr(), c as u32, to, 0]);
                }
            }
        }
        node = m[node].next;
    }
    if keep == 0 {
        let mut node = m[list].first;
        while !node.is_null() {
            if selected_either(m, node) && m[node].wire_type == 0 {
                super::select::unselect_grafnode(m, o, list, node);
            }
            node = m[node].next;
        }
    }
    m.release(8);
    0
}

/// Hides every ghosted node [soft 0x0832e36c SI_hideGlGhosted]: each node of display type
/// 1 whose element is found goes to `SI_edSetLinetype` with its chapter, 0 and 0; then
/// **every** selected node of the list is unselected, hidden or not. Answers 0.
pub fn hide_gl_ghosted(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>) -> i32 {
    let index = m.local(4);
    let chap = m.local(4);
    let mut node = m[list].first;
    while !node.is_null() {
        if m[node].wire_type == 1 {
            let elem = o.call(m, &outside::SI_ELEMDICT_FIND_TRUE_ELEM_FROM_GRAF_NODE, &[node.addr(), chap, index]);
            if elem != 0 {
                let c = m.u32(chap);
                o.call(m, &outside::SI_ED_SET_LINETYPE, &[elem, c, 0, 0]);
            }
        }
        node = m[node].next;
    }
    let mut node = m[list].first;
    while !node.is_null() {
        if selected_either(m, node) {
            super::select::unselect_grafnode(m, o, list, node);
        }
        node = m[node].next;
    }
    m.release(8);
    0
}
