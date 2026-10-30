// SPDX-License-Identifier: MIT
//! Selecting, unselecting and toggling a node.
//!
//! A node's selection is its word at +0x08: 0 not selected, 1 selected as a node, 3
//! selected as a branch (with the nodes below it, which then carry 2, "inherited"). A node
//! that is not editable (flag 0x1000, the reduced product's marker) keeps the same three
//! states in its flags 0x2000, 0x4000 and 0x8000 instead.

use super::list::{find_gl_data, graflist_set_active};
use crate::globals::*;
use crate::outside::{self, Outside};
use crate::records::flag;
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_types::scene::{GrafList, GrafNode};

/// Clears the selection of a node and of the `count` nodes after it.
fn clear_run(m: &mut Mem, first: Ptr32<GrafNode>) {
    let mut left = m[first].nb_sub as u16 as u32;
    let mut node = m[first].next;
    m[first].flags &= !flag::SEL_ANY;
    m[first].selection = 0;
    while left != 0 {
        m[node].flags &= !flag::SEL_ANY;
        m[node].selection = 0;
        node = m[node].next;
        left -= 1;
        if m.fault().is_some() {
            // a count that runs past the chain's end: the original dies at the first
            // node that is not there
            break;
        }
    }
}

/// Unselects a node [soft 0x08333830 SI_unselectGrafnode].
///
/// A named selection (kind 0xc) that is selected hands the whole matter to
/// `SI_nselectionSelectContent (0x083465a4)` with 2, and nothing else happens. Otherwise:
///
/// - selected as a node (selection 1, or flags 0x1000 and 0x2000 together): its selection
///   word and selection flags are cleared;
/// - selected as a branch or by one (selection 2 or 3, or not editable with flag 0x4000 or
///   0x8000): the node and the `nb_sub` nodes after it are cleared. When its object, read
///   as a model, is of kind 0 with algorithm 3 or 4 (an effector), the clearing starts
///   instead at the node of the chain's root (`SI_skeChainRoot (0x081d20c0)`, looked up
///   with `SI_findGlData` as a root node and not tested for null);
/// - anything else: nothing is cleared.
///
/// Then a list in single selection loses its active node, and `SI_dopeFlagsSet
/// (0x081fa5d0)` with (1, 1, 0, 0) and `SI_sprdUpdateSelect (0x085aaa7c)` are called.
/// Answers 0.
pub fn unselect_grafnode(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, node: Ptr32<GrafNode>) -> i32 {
    if m[node].kind == 0xc && m[node].selection != 0 {
        o.call(m, &outside::SI_NSELECTION_SELECT_CONTENT, &[node.addr(), 2]);
        return 0;
    }
    let s = m[node].selection;
    let f = m[node].flags;
    if s == 1 || f & (flag::UNEDITABLE | flag::SEL_NODE) == (flag::UNEDITABLE | flag::SEL_NODE) {
        m[node].selection = 0;
        m[node].flags &= !flag::SEL_ANY;
    } else if (s.wrapping_sub(2) as u32) <= 1 || (f & flag::UNEDITABLE != 0 && f & (flag::SEL_BRANCH | flag::SEL_INHERITED) != 0) {
        let model: Ptr32<Model> = m[node].data.cast();
        if m[model].kind == 0 && (m[model].algo_type.wrapping_sub(3) as u32) <= 1 {
            let root = o.call(m, &outside::SI_SKE_CHAIN_ROOT, &[model.addr()]);
            let first = find_gl_data(m, list, Ptr32::new(root), 2);
            clear_run(m, first);
        } else {
            clear_run(m, node);
        }
    }
    if m[list].multi == 0 {
        graflist_set_active(m, list, Ptr32::NULL);
    }
    o.call(m, &outside::SI_DOPE_FLAGS_SET, &[1, 1, 0, 0]);
    o.call(m, &outside::SI_SPRD_UPDATE_SELECT, &[]);
    0
}

/// What the list's active node is, whichever of the two places holds it.
#[inline]
fn active_of(m: &Mem, list: Ptr32<GrafList>) -> Ptr32<GrafNode> {
    let a = m[list].active;
    if a.is_null() {
        m[list].active2
    } else {
        a
    }
}

/// Whether a click selects plain nodes instead of branches: the manipulation global at
/// `SI_glbManip` + 0x1c is set and the list is in multiple selection.
#[inline]
fn nodes_only(m: &Mem, list: Ptr32<GrafList>) -> bool {
    m.i16(SI_GLB_MANIP.addr() + GLB_MANIP_1C) != 0 && m[list].multi != 0
}

/// Selects a node [soft 0x08333df8 SI_selectGrafnode]. `mode`: 0 the node alone, 1 the
/// node and, on a second call, its branch, 2 the whole model the node belongs to, 3 the
/// chain a joint belongs to. Answers 0.
///
/// The original reads the product-tier flag `SI_lightLicense` once and, where it is set,
/// keeps the selection of nodes that are not editable in their flags instead of their
/// selection word. **The port takes the full product's path**: the flag counts as 0 and
/// those branches are left out.
///
/// 1. Nothing for null and for kind 0xa. A named selection (0xc) that is selected goes to
///    `SI_nselectionSelectContent (0x083465a4)` with 1, and nothing else happens.
/// 2. A node that is no model node is selected with mode 0 whatever was asked. For a model
///    node in mode 1: an effector (model kind 0, algorithm 3 or 4) is remembered and its
///    father looked at instead; when the model looked at is a joint (kind 5) the mode
///    becomes 3.
/// 3. Mode 2: the node is replaced by the root node of its model (the chain is followed
///    backwards to kind 2 without a test for its head), then mode 1. Mode 3: the fathers
///    are climbed to the first model of kind 0, its node is looked up (`SI_findGlData`;
///    nothing more happens without one) and **given the selection word of the node that
///    was clicked**; the routine goes on with that node in mode 1.
/// 4. Mode 0: a node that is not selected gets 1; in single selection the list's active
///    node, if it is another, is unselected first and the node becomes the active one.
/// 5. Any other mode, by the node's selection word: 1 becomes 3 (a branch) and the
///    `nb_sub` nodes after it get 2, and in single selection the node becomes the active
///    one; 0 becomes 3 with the same for the nodes after it, and in single selection the
///    active node is unselected (only when none of the nodes below had been selected) and
///    the node, or the remembered effector, becomes the active one. When plain nodes are
///    wanted (see `nodes_only`) every node gets 1 instead and a node that had 1 keeps it.
///    A node with 2 or 3, or one that is not editable and not 1, is left as it is.
/// 6. If the node now counts as selected, `lastSelected`/`multipleSelected` note whether
///    this is the second selection since they were cleared. The "not editable" test of
///    this step reads the flags of **the node the walk of step 5 ended on** (the node
///    after the branch, null at the chain's end: a read through null in the original).
/// 7. `SI_dopeFlagsSet (0x081fa5d0)` (1, 1, 0, 0), `SI_sprdUpdateSelect (0x085aaa7c)`;
///    and in single selection, when the manipulation global at +0x32 is set and the active
///    node has flag 0x0800, `SI_setUItoPrefFlags (0x08424f98)` with its flags.
pub fn select_grafnode(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, node: Ptr32<GrafNode>, mode: i32) -> i32 {
    if node.is_null() {
        return 0;
    }
    let kind = m[node].kind;
    if kind == 0xa {
        return 0;
    }
    if kind == 0xc && m[node].selection != 0 {
        o.call(m, &outside::SI_NSELECTION_SELECT_CONTENT, &[node.addr(), 1]);
        return 0;
    }
    let mut mode = mode;
    let mut effector: Ptr32<GrafNode> = Ptr32::NULL;
    let mut model: Ptr32<Model> = Ptr32::NULL;
    if (kind.wrapping_sub(2) as u32) > 1 {
        mode = 0;
    } else {
        model = m[node].data.cast();
        if mode == 1 {
            if m[model].kind == 0 && (m[model].algo_type.wrapping_sub(3) as u32) <= 1 {
                effector = node;
                model = m[model].father;
            }
            if m[model].kind == 5 {
                mode = 3;
            }
        }
    }
    let mut cur = node;
    if mode == 2 {
        if kind != 2 {
            loop {
                cur = m[cur].prev;
                if m[cur].kind == 2 || m.fault().is_some() {
                    break;
                }
            }
        }
        mode = 1;
    } else if mode == 3 {
        while m[model].kind != 0 {
            model = m[model].father;
            if m.fault().is_some() {
                break;
            }
        }
        let clicked = m[node].selection;
        cur = find_gl_data(m, list, model.cast(), 3);
        if cur.is_null() {
            cur = find_gl_data(m, list, model.cast(), 2);
            if cur.is_null() {
                return 0;
            }
        }
        m[cur].selection = clicked;
        mode = 1;
    }
    let target = cur;
    if mode == 0 {
        if m[cur].selection == 0 {
            m[cur].selection = 1;
            if m[list].multi == 0 {
                let a = active_of(m, list);
                if !a.is_null() && a != cur {
                    unselect_grafnode(m, o, list, a);
                }
                graflist_set_active(m, list, cur);
            }
        }
    } else {
        let s = m[target].selection;
        if s == 1 {
            if !nodes_only(m, list) {
                m[cur].selection = 3;
                if m[list].multi == 0 {
                    graflist_set_active(m, list, cur);
                }
            }
            let mut left = m[cur].nb_sub as u16;
            cur = m[cur].next;
            while left != 0 {
                m[cur].selection = if nodes_only(m, list) { 1 } else { 2 };
                cur = m[cur].next;
                left -= 1;
                if m.fault().is_some() {
                    break;
                }
            }
        } else if m[cur].flags & flag::UNEDITABLE == 0 && s == 0 {
            let mut alone = true;
            m[cur].selection = if nodes_only(m, list) { 1 } else { 3 };
            let mut left = m[cur].nb_sub as u16;
            let top = cur;
            cur = m[cur].next;
            while left != 0 {
                alone = alone && m[cur].selection == 0;
                m[cur].selection = if nodes_only(m, list) { 1 } else { 2 };
                cur = m[cur].next;
                left -= 1;
                if m.fault().is_some() {
                    break;
                }
            }
            if m[list].multi == 0 {
                if alone {
                    let a = active_of(m, list);
                    if !a.is_null() && a != top {
                        unselect_grafnode(m, o, list, a);
                    }
                }
                graflist_set_active(m, list, if effector.is_null() { top } else { effector });
            }
        }
    }
    // the flags read here are those of the node the walk ended on (see step 6)
    let selected = (m[cur].flags & flag::UNEDITABLE != 0 && super::flags::graf_node_is_wrath_selected(m, target) as i16 != 0)
        || super::flags::graf_node_is_selected(m, target) as i16 != 0;
    if selected && m[MULTIPLE_SELECTED] == 0 {
        if !m[LAST_SELECTED].is_null() {
            m[MULTIPLE_SELECTED] = 1;
            m[LAST_SELECTED] = Ptr32::NULL;
        } else {
            m[LAST_SELECTED] = target;
        }
    }
    o.call(m, &outside::SI_DOPE_FLAGS_SET, &[1, 1, 0, 0]);
    o.call(m, &outside::SI_SPRD_UPDATE_SELECT, &[]);
    let a = active_of(m, list);
    if m[list].multi == 0 && !a.is_null() && m.i16(SI_GLB_MANIP.addr() + GLB_MANIP_32) != 0 && m[a].flags & flag::PREF != 0 {
        let f = m[a].flags as u32;
        o.call(m, &outside::SI_SET_UI_TO_PREF_FLAGS, &[f]);
    }
    0
}

/// Selects the node of an object [soft 0x0832d714 SI_graflistSelectData]: the first node
/// that stands for `data`, whatever its kind, goes to `SI_selectGrafnode` with `mode`.
/// Answers whether there is such a node.
pub fn graflist_select_data(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, data: Ptr32<si_mem::Opaque>, mode: i32) -> i32 {
    let node = find_gl_data(m, list, data, 0);
    if node.is_null() {
        return 0;
    }
    select_grafnode(m, o, list, node, mode);
    1
}

/// Selects a node that is not selected and unselects one that is
/// [soft 0x083343b4 SI_toggleGrafnode]. Answers 0.
///
/// A named selection (0xc) that is selected goes to `SI_nselectionSelectContent
/// (0x083465a4)` with 0. Only model nodes, cameras and lights (kinds 2 to 7) keep the mode
/// asked for; a light of kind 8 keeps mode 1 when its word at +0x182 is set; everything
/// else is toggled in mode 0. Mode 2 goes back along the chain to the node that heads the
/// group (kind 2, 5 or 7) and becomes mode 1.
///
/// Then by the node's selection word (the full product's path: a node that is not editable
/// would have its state read from its flags, and that reading is made here too, but the
/// one write that depends on `SI_lightLicense` is left out):
///
/// - 0: `SI_selectGrafnode` with the mode;
/// - 1, mode 1, a light (kind 6 or 8): with the word at +0x182 set, its word at +0x1dc is
///   flipped between 0 and 1; then `SI_selectGrafnode` in mode 1;
/// - 1, mode 0: `SI_unselectGrafnode`;
/// - 1, otherwise: when plain nodes are wanted (the manipulation global at +0x1c and
///   multiple selection) the node is given 3 and unselected; else `SI_selectGrafnode` in
///   mode 1;
/// - 3 in mode 1: `SI_unselectGrafnode`;
/// - anything else: nothing.
pub fn toggle_grafnode(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<GrafList>, node: Ptr32<GrafNode>, mode: i32) -> i32 {
    if m[node].kind == 0xc && m[node].selection != 0 {
        o.call(m, &outside::SI_NSELECTION_SELECT_CONTENT, &[node.addr(), 0]);
        return 0;
    }
    let mut node = node;
    let mut mode = mode;
    let kind = m[node].kind;
    if !matches!(kind, 2..=7) {
        let keep = kind == 8 && m.i16(m[node].data.addr().wrapping_add(0x182)) != 0 && mode == 1;
        if !keep {
            mode = 0;
        }
    }
    if mode == 2 {
        if !matches!(kind, 2 | 5 | 7) {
            loop {
                node = m[node].prev;
                if matches!(m[node].kind, 2 | 5 | 7) || m.fault().is_some() {
                    break;
                }
            }
        }
        mode = 1;
    }
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
    match state {
        0 => {
            select_grafnode(m, o, list, node, mode);
        }
        1 => {
            if mode == 1 && matches!(m[node].kind, 6 | 8) {
                let light = m[node].data.addr();
                if m.i16(light.wrapping_add(0x182)) != 0 {
                    let on = m.i16(light.wrapping_add(0x1dc)) != 0;
                    m.set_i16(light.wrapping_add(0x1dc), if on { 0 } else { 1 });
                }
                select_grafnode(m, o, list, node, 1);
            } else if mode == 0 {
                unselect_grafnode(m, o, list, node);
            } else if nodes_only(m, list) {
                // the full product's path: the selection word, not the flags
                m[node].selection = 3;
                unselect_grafnode(m, o, list, node);
            } else {
                select_grafnode(m, o, list, node, 1);
            }
        }
        3 => {
            if mode == 1 {
                unselect_grafnode(m, o, list, node);
            }
        }
        _ => {}
    }
    0
}
