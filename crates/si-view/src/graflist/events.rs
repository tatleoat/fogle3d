// SPDX-License-Identifier: MIT
//! The selection commands of the menus, which work on the scene's own list: the list the
//! process record names in the array `SI_grafLists`.

use super::flags::{graf_node_is_selected, graf_node_is_wrath_selected, graflist_count_selected, graflist_set_multi_selection};
use super::list::graflist_set_active;
use super::select::{select_grafnode, unselect_grafnode};
use crate::globals::*;
use crate::outside::{self, Outside};
use crate::records::flag;
use si_mem::{Mem, Opaque, Ptr32};
use si_types::scene::{GrafList, GrafNode};

/// The scene's list: `SI_grafLists` + 0x24 × the process record's list index. The
/// original works it out again at every use.
pub fn scene_list(m: &Mem) -> Ptr32<GrafList> {
    let proc = m[SI_PROC_DATA];
    m[SI_GRAF_LISTS].add(m[proc].list)
}

/// Whether a schematic view is among the visible windows: `SI_windowGetNextVisible
/// (0x083d6500)` is asked for one window after another (through a local that starts as
/// null) until it answers 0 or a window whose view record (the word at +4 of what the
/// window's +0x2c points to) has 0x807d7 in the bits 0x8ffff.
fn schematic_shown(m: &mut Mem, o: &mut dyn Outside) -> bool {
    let window = m.local(4);
    m.set_u32(window, 0);
    let mut shown = false;
    while o.call(m, &outside::SI_WINDOW_GET_NEXT_VISIBLE, &[window]) as i16 != 0 {
        let w = m.u32(window);
        let view = m.u32(w.wrapping_add(0x2c));
        if m.u32(view.wrapping_add(4)) & 0x8ffff == 0x807d7 {
            shown = true;
            break;
        }
        if m.fault().is_some() {
            break;
        }
    }
    m.release(4);
    shown
}

/// Whether the commands "select all" and "toggle" touch a node: not an animation file
/// (0x12), and a material or a texture (0xe, 0xf, 0x10) only while a schematic view shows.
#[inline]
fn command_kind(kind: i32, schematic: bool) -> bool {
    kind != 0x12 && (schematic || (kind.wrapping_sub(0xe) as u32) > 2)
}

/// "Select all" [soft 0x083376f0 SI_allSelect]: the scene's list goes to multiple
/// selection; then every node whose selection word is 0 and whose kind the command
/// touches is selected with `SI_selectGrafnode` in mode 2; then `SI_dopeFlagsSet
/// (0x081fa5d0)` with (1, 0, 0, 0) and `SI_sprdUpdateSelect (0x085aaa7c)`. The event it is
/// handed is not looked at. Answers 0.
pub fn all_select(m: &mut Mem, o: &mut dyn Outside, _event: Ptr32<Opaque>) -> i32 {
    let list = scene_list(m);
    graflist_set_multi_selection(m, o, list, 1);
    let schematic = schematic_shown(m, o);
    let mut node = m[scene_list(m)].first;
    while !node.is_null() {
        if m[node].selection == 0 && command_kind(m[node].kind, schematic) {
            let list = scene_list(m);
            select_grafnode(m, o, list, node, 2);
        }
        node = m[node].next;
    }
    o.call(m, &outside::SI_DOPE_FLAGS_SET, &[1, 0, 0, 0]);
    o.call(m, &outside::SI_SPRD_UPDATE_SELECT, &[]);
    0
}

/// "Clear selection" [soft 0x083377e4 SI_clearSelection]: every node of the scene's list
/// that has a selection word other than 0, or is not editable with one of the three
/// selection flags, goes to `SI_unselectGrafnode`; then `SI_dopeFlagsSet` with
/// (1, 1, 0, 0) and `SI_sprdUpdateSelect`. Answers 0.
pub fn clear_selection(m: &mut Mem, o: &mut dyn Outside, _event: Ptr32<Opaque>) -> i32 {
    let mut node = m[scene_list(m)].first;
    while !node.is_null() {
        let f = m[node].flags;
        if m[node].selection != 0 || (f & flag::UNEDITABLE != 0 && f & flag::SEL_ANY != 0) {
            let list = scene_list(m);
            unselect_grafnode(m, o, list, node);
        }
        node = m[node].next;
    }
    o.call(m, &outside::SI_DOPE_FLAGS_SET, &[1, 1, 0, 0]);
    o.call(m, &outside::SI_SPRD_UPDATE_SELECT, &[]);
    0
}

/// Sets the scene's list to single or multiple selection by what is selected
/// [soft 0x08337860 SI_graflistCheckMulti] (after a load): the selected nodes are counted
/// (selection 1 or 3, or the flags of a node that is not editable). More than one: the
/// list's mode word is 1, and so is the first word of the list `SI_scngl` points to;
/// `SI_buttonOff (0x0837fa74)` with 0xa and `SI_buttonKeepHilite (0x0837fa1c)` with 0x14;
/// both lists lose their active node (their word at +8). Otherwise the mode words are 0,
/// the two buttons the other way round, the last selected node becomes the scene list's
/// active node, and `SI_scngl`'s is recounted (`SI_graflistCountSelected` storing the
/// first selected node at its +8) and moved to the second place when it is not editable.
/// Answers 0.
pub fn graflist_check_multi(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let mut last: Ptr32<GrafNode> = Ptr32::NULL;
    let mut count = 0;
    let mut node = m[scene_list(m)].first;
    while !node.is_null() {
        if (m[node].flags & flag::UNEDITABLE != 0 && graf_node_is_wrath_selected(m, node) as i16 != 0) || graf_node_is_selected(m, node) as i16 != 0 {
            last = node;
            count += 1;
        }
        node = m[node].next;
    }
    let list = scene_list(m);
    m[list].multi = (count > 1) as i16;
    let scngl = m[SI_SCNGL];
    m[scngl].multi = m[scene_list(m)].multi;
    if m[scene_list(m)].multi != 0 {
        o.call(m, &outside::SI_BUTTON_OFF, &[0xa]);
        o.call(m, &outside::SI_BUTTON_KEEP_HILITE, &[0x14]);
        let list = scene_list(m);
        m[list].active = Ptr32::NULL;
        let scngl = m[SI_SCNGL];
        m[scngl].active = Ptr32::NULL;
    } else {
        o.call(m, &outside::SI_BUTTON_OFF, &[0x14]);
        o.call(m, &outside::SI_BUTTON_KEEP_HILITE, &[0xa]);
        let list = scene_list(m);
        graflist_set_active(m, list, last);
        let scngl = m[SI_SCNGL];
        graflist_count_selected(m, scngl, si_mem::field!(scngl, GrafList, active), 0);
        let scngl = m[SI_SCNGL];
        let a = m[scngl].active;
        if !a.is_null() && m[a].flags & flag::UNEDITABLE != 0 {
            m[scngl].active2 = a;
            m[scngl].active = Ptr32::NULL;
        }
    }
    0
}

/// "Toggle selection" [soft 0x083379ac SI_toggleSelect]: the scene's list goes to
/// multiple selection; then every node that is selected in any way (selection 1, 2 or 3;
/// for a node that is not editable, its flags say which) loses its selection word and its
/// three flags, and every node that is not, of a kind the command touches, gets selection
/// 1. (The full product's path: for a node that is not editable the reduced product sets
/// flag 0x2000 instead; `SI_lightLicense` is read as 0.) A selection word below 0 or above
/// 3 is left alone. Then `SI_dopeFlagsSet` with (1, 1, 0, 0) and `SI_sprdUpdateSelect`.
/// Answers 0.
pub fn toggle_select(m: &mut Mem, o: &mut dyn Outside, _event: Ptr32<Opaque>) -> i32 {
    let list = scene_list(m);
    graflist_set_multi_selection(m, o, list, 1);
    let schematic = schematic_shown(m, o);
    let mut node = m[scene_list(m)].first;
    while !node.is_null() {
        let ok = command_kind(m[node].kind, schematic);
        let f = m[node].flags;
        let state = if f & flag::UNEDITABLE != 0 && f & flag::SEL_ANY != 0 {
            if f & flag::SEL_NODE != 0 {
                1
            } else if f & flag::SEL_BRANCH != 0 {
                3
            } else {
                ((f as i16) < 0) as i32 * 2
            }
        } else {
            m[node].selection
        };
        if state == 0 {
            if ok {
                m[node].selection = 1;
            }
        } else if (1..=3).contains(&state) {
            m[node].flags = f & !flag::SEL_ANY;
            m[node].selection = 0;
        }
        node = m[node].next;
    }
    o.call(m, &outside::SI_DOPE_FLAGS_SET, &[1, 1, 0, 0]);
    o.call(m, &outside::SI_SPRD_UPDATE_SELECT, &[]);
    0
}

/// Clears the selection planes of every view [soft 0x08335040 SI_myclearSelect]; what is
/// left of it is the walk: nothing in the stand-alone renderer; else for every window
/// `SI_windowGetNextActive (0x083d1ca4)` hands out (through a local that starts as null)
/// whose view record's 16-bit word at +4 is not 0x7d4 and whose word at +0x64 is 0:
/// `SI_windowSet (0x083d6588)` with 2 and the window, `si_softSetGrafport (0x08369490)`,
/// `SI_clearSelect` (which does nothing), `SI_unsetGrafport (0x0836a2d8)`,
/// `SI_windowUnset (0x083d65f4)`.
pub fn my_clear_select(m: &mut Mem, o: &mut dyn Outside) {
    if m[SI_STANDALONE_RENDERER] != 0 {
        return;
    }
    let window = m.local(4);
    m.set_u32(window, 0);
    while o.call(m, &outside::SI_WINDOW_GET_NEXT_ACTIVE, &[window]) as i16 != 0 {
        let w = m.u32(window);
        let view = m.u32(w.wrapping_add(0x2c));
        if m.i16(view.wrapping_add(4)) != 0x7d4 && m.u32(w.wrapping_add(0x64)) == 0 {
            o.call(m, &outside::SI_WINDOW_SET, &[2, w]);
            let w = m.u32(window);
            o.call(m, &outside::SI_SOFT_SET_GRAFPORT, &[w]);
            super::list::clear_select();
            o.call(m, &outside::SI_UNSET_GRAFPORT, &[]);
            o.call(m, &outside::SI_WINDOW_UNSET, &[]);
        }
        if m.fault().is_some() {
            break;
        }
    }
    m.release(4);
}

/// Deletes a selected node [soft 0x0832db14 SI_graflistDeleteSelection]: `SI_graflistDeleteNode
/// (0x0832d75c)` (of this source file; not ported yet, so behind the trait) with the node
/// and 1 when it is selected as a branch (selection 3, or flag 0x4000), else 0. Answers 0.
pub fn graflist_delete_selection(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, node: Ptr32<GrafNode>) -> i32 {
    let branch = m[node].selection == 3 || m[node].flags & flag::SEL_BRANCH != 0;
    o.call(m, &outside::SI_GRAFLIST_DELETE_NODE, &[list.addr(), node.addr(), branch as u32]);
    0
}
