// SPDX-License-Identifier: MIT
//! Animation files and their elements (`motionUtils.c`; finding 0017, section 9f): tying
//! an animation-file element to a hierarchy and untying it, the two small tests in front of
//! the animation commands, and the PostScript preview's file names.
//!
//! An **animation-file element** is an element of the ANIMATION chapter whose object is not
//! a curve but a hierarchy-animation record (`SI_HierAnim`: which element carries which
//! curves, and the actions). An element that uses one has a forward relation to it in list
//! 0, whose id depends on the element's chapter (`SI_relGetAnimFRelFromChapter`: one of
//! 15000 to 15010).
//!
//! Written in the fifth pass, measured in the sixth: each of the nine routines was held
//! against its instruction listing and run against the original in the emulation harness on
//! 2,000 made-up cases (finding 0017, sections 9f and 12). The walks agreed as written; the
//! one change is in `si_postscriptDumpFrame`, whose name is now cut at the end of its block
//! where the original writes one byte behind it.

use crate::ed;
use crate::glob::{self, cid};
use crate::fcvoper::OperHost;
use crate::key::KeyHost;
use crate::off::{arel, elem, grafnode, list, rellist};
use crate::path::PathHost;
use crate::play;
use crate::reset;
use crate::utils;
use si_mem::{Addr, Mem};

/// `si_psPrevFile` (0x0878fb2c): the PostScript preview's file name, a C string from
/// `strdup`, or null.
pub const SI_PS_PREV_FILE: Addr = 0x0878_fb2c;

/// The texts these routines put on the status line, at their addresses in the binary.
pub mod text {
    use si_mem::Addr;
    /// "You must select a root item first.."
    pub const SELECT_ROOT: Addr = 0x0869_7a20;
    /// "You must have only one hierarchy selected"
    pub const ONE_HIERARCHY: Addr = 0x0869_7a60;
    /// "You must pick an animation item.."
    pub const PICK_ANIMATION: Addr = 0x0869_7aa0;
    /// "The animation file element seems invalid"
    pub const INVALID: Addr = 0x0869_7ae0;
    /// "Sorry, Anim file elements cannot be shared for now"
    pub const SHARED: Addr = 0x0869_7b20;
    /// "Can't associate to lattice, shape or cluster animation."
    pub const NOT_TO_PARTS: Addr = 0x0869_7b60;
    /// "Anim file not compatible with selected element."
    pub const INCOMPATIBLE: Addr = 0x0869_7ba0;
    /// "Item already has some animation."
    pub const HAS_ANIMATION: Addr = 0x0869_7c00;
    /// "No match between items and animation names."
    pub const NO_MATCH: Addr = 0x0869_7c40;
    /// "Animation file not associated to anything"
    pub const NOT_ASSOCIATED: Addr = 0x0869_7c80;
    /// "Can't disassociate lattice, shape or cluster animation for now"
    pub const NOT_FROM_PARTS: Addr = 0x0869_7cc0;
    /// "Model currently has no animation.."
    pub const NO_ANIMATION: Addr = 0x0869_7d00;
    /// "Animation file not used by selected element"
    pub const NOT_USED: Addr = 0x0869_7d40;
    /// ".ps" and "frame": the suffix and the default name `SI_postscriptPreview` asks with.
    pub const PS_SUFFIX: Addr = 0x0869_7bd6;
    pub const PS_DEFAULT: Addr = 0x0869_7bd0;
}

/// Fields read here beyond [`crate::off`].
mod f {
    /// The record an element's first word points at: +0x10 and +0x14 are two names (C
    /// strings), +0x24 a word handed to `SI_hieranimAddElem`.
    pub const NODE_NAME_10: u32 = 0x10;
    pub const NODE_NAME_14: u32 = 0x14;
    pub const NODE_24: u32 = 0x24;
    /// `SI_HierAnim`: +8 the chapter the record is for; +0xc, +0x10 its actions (`SI_Actions`
    /// lies at +0xc: a count and an array).
    pub const HA_CHAPTER: u32 = 0x08;
    pub const HA_ACTIONS: u32 = 0x0c;
    pub const HA_ACTIONS_ARRAY: u32 = 0x10;
    /// `SI_Event` + 0xc: the window the event happened in.
    pub const EVENT_PORT: u32 = 0x0c;
}

crate::host_trait! {
    /// What these routines call outside this package.
    pub trait AssocHost: OperHost {
        /// `SI_dbMRequest2(message number, a, b, c)` (0x080dd938): a question from the
        /// message file; the answer is the button's number.
        fn db_m_request2(message: i32, a: i32, b: i32, c: i32) -> i32;
        /// `SI_graflistCountSelHierarchies(graf list, flag)` (0x0832dc0c).
        fn graflist_count_sel_hierarchies(gl: Addr, flag: i32) -> i32;
        /// `SI_gLSelect(window)` (0x0832ef0c): the graf node under the pointer.
        fn gl_select(port: Addr) -> Addr;
        /// `SI_elemdictFindElemByAuxData(dict, chapter, graf node)` (0x081345a0): the index
        /// of the element shown by that node, negative if none.
        fn elemdict_find_elem_by_aux_data(dict: Addr, chap: i32, gn: Addr) -> i32;
        /// `SI_foolDisplay(graf list, a, colour, graf node)` (0x08323a70), `swapbuffers()`
        /// (the graphics library), `usleep(microseconds)` (the C library): the two nodes
        /// flash for half a second.
        fn fool_display(gl: Addr, a: i32, b: i32, gn: Addr);
        fn swapbuffers();
        fn usleep(microseconds: i32);
        /// `SI_relGetAnimFRelFromChapter(chapter)` (0x0810f540): the id of the relation from
        /// an element of that chapter to its animation file.
        fn rel_get_anim_f_rel_from_chapter(chap: i32) -> i32;
        /// `SI_graflistTypeCount(graf list, 0, 0, 0, 0)` (0x0832dc74).
        fn graflist_type_count(gl: Addr, a: i32, b: i32, c: i32, d: i32) -> i32;
        /// `SI_elemdictSetAnimRelation(dict, &SI_RD, animation file element, −1, element,
        /// button, 0, single, 0)` (0x08128c3c): the file's curves are tied to the hierarchy
        /// by name; 0 when nothing matched.
        fn elemdict_set_anim_relation(dict: Addr, reldict: Addr, anim: Addr, a: i32, e: Addr, button: i32, b: i32, single: i32, c: i32) -> i32;
        /// `SI_edChapterAnimElemChangeName(element, chapter, name, name 2, name)` (0x080eea0c).
        fn ed_chapter_anim_elem_change_name(e: Addr, chap: i32, a: Addr, b: Addr, c: Addr);
        /// `SI_actionsUnselectAll(actions)` (0x081e27cc).
        fn actions_unselect_all(actions: Addr);
        /// `SI_hieranimAllocate(chapter)` (0x08138b28), `SI_hieranimDispose(record)`
        /// (0x08138b5c), `SI_hieranimAddElem(record, word, parent)` (0x08138cc4: the new
        /// entry's number), `SI_hieranimAddItem(record, entry, 0, −1, −1, 1, curve)` (0x08138e0c).
        fn hieranim_allocate(chap: i32) -> Addr;
        fn hieranim_dispose(ha: Addr);
        fn hieranim_add_elem(ha: Addr, word: i32, parent: i32) -> i32;
        fn hieranim_add_item(ha: Addr, entry: i32, a: i32, b: i32, c: i32, d: i32, curve: Addr);
        /// `SI_elemdictRemoveRelationPair(dict, &SI_RD, from, to, relation, 1, 0)` (0x0813362c).
        fn elemdict_remove_relation_pair(dict: Addr, reldict: Addr, from: i32, to: i32, relid: i32, a: i32, b: i32);
        /// `SI_getFilename(suffix, default name)` (0x08382704): asks for a file name; a C
        /// string, or null when the user gave none.
        fn get_filename(suffix: Addr, default: Addr) -> Addr;
        /// `SI_windowGetHilite(&window)` (0x083d63e0): the highlighted window.
        fn window_get_hilite(port: Addr);
        /// `SI_postscriptRefresh(window, file name)` (0x0834801c): the window is written as
        /// PostScript.
        fn postscript_refresh(port: Addr, name: Addr);
    }
}

/// Whether an animation command has something to work on [soft 0x0822d994 si_animOpCheck]:
/// true when a selected node of the current graf list is of a kind other than 3, 0xc, 0xd
/// and 0x12 (the animation items themselves); without one the status line says "You must
/// select a root item first..".
pub fn anim_op_check<H: KeyHost + ?Sized>(m: &mut Mem, h: &mut H) -> bool {
    let mut n = 0;
    let mut gn = m.ptr(glob::cur_graf_list(m) + 4);
    while gn != 0 {
        if m.i32(gn + grafnode::SELECTION) != 0 {
            let kind = m.i32(gn + grafnode::KIND);
            let counts = if kind < 0xe { kind < 0xc && kind != 3 } else { kind != 0x12 };
            if counts {
                n += 1;
            }
        }
        gn = m.ptr(gn + grafnode::NEXT);
    }
    if n == 0 {
        KeyHost::status_display_message(h, m, text::SELECT_ROOT, 1);
    }
    n > 0
}

/// The "plotting required" question [soft 0x0822e000 SI_warningIfPlottingRequired]: asked
/// when a live MODELS element has a relation 239 (quick stretch) to a model in list 2 but
/// neither a linear-velocity curve (relation 90 in list 1) nor a linear-acceleration curve
/// (91). The question is message 0x8c (`SI_dbMRequest2(0x8c, 0, 0, 1)`); the result is 0
/// when it is answered with button 1, and 1 otherwise or when it was not asked.
pub fn warning_if_plotting_required<H: AssocHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr) -> i16 {
    let models = glob::chap(m, cid::MODELS);
    let l = m.ptr(d + 4).wrapping_add((models as u32).wrapping_mul(list::SIZE));
    let out = m.local(12);
    let mut ask = false;
    let mut i = 0;
    while i < m.i32(l + list::NB_ELEMS) {
        let e = m.ptr(l + list::ELEMS).wrapping_add((i as u32).wrapping_mul(elem::SIZE));
        if m.i16(e + elem::STATUS) == 0 {
            let models = glob::chap(m, cid::MODELS);
            if ed::find_related_elem(m, d, e, true, 2, 0xef, models).elem != 0 {
                let none = |m: &Mem| m.ptr(out) == 0 && m.ptr(out + 4) == 0 && m.ptr(out + 8) == 0;
                utils::get_fcv_triplet(m, h, e, out, out + 4, out + 8, 1, 0x5a);
                if none(m) {
                    utils::get_fcv_triplet(m, h, e, out, out + 4, out + 8, 1, 0x5b);
                    if none(m) {
                        ask = true;
                        break;
                    }
                }
            }
        }
        i += 1;
    }
    m.release(12);
    if ask && h.db_m_request2(m, 0x8c, 0, 0, 1) == 1 {
        return 0;
    }
    1
}

/// Files an element's curves, and its descendants', in a hierarchy-animation record
/// [soft 0x0822e930 elemdictStoreAnimInHastruct]: the element gets an entry
/// (`SI_hieranimAddElem(record, node + 0x24, parent entry)`), and every forward relation of
/// it to the ANIMATION chapter (the animation-file links are **not** left out here) whose
/// element exists and has its word +0x54 set gets an item for that element's object
/// (`SI_hieranimAddItem(record, entry, 0, −1, −1, 1, object)`); `*count` goes up by one per
/// item. For a record of the MODELS chapter (record + 8) the sons follow (`SI_edFindSon`,
/// `SI_edFindNextSon`), with the element's entry as their parent.
///
/// The ANIMATION element is taken by index without a range test, as in the original.
pub fn elemdict_store_anim_in_hastruct<H: AssocHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, ha: Addr, e: Addr, parent: i32, count: Addr) {
    let word = m.i32(m.ptr(e + elem::NODE).wrapping_add(f::NODE_24));
    let entry = h.hieranim_add_elem(m, ha, word, parent);
    let mut g = 0;
    while g < m.i32(e + elem::FORWARD) {
        let l = m.ptr(e + elem::FORWARD + 4).wrapping_add((g as u32).wrapping_mul(rellist::SIZE));
        let mut i = 0;
        while i < m.i32(l + rellist::NB) {
            let r = m.ptr(l + rellist::RELS).wrapping_add((i as u32).wrapping_mul(arel::SIZE));
            let rc = m.i16(r + arel::CHAPID) as i32;
            if rc == glob::chap(m, cid::ANIMATION) {
                let cl = m.ptr(d + 4).wrapping_add((rc as u32).wrapping_mul(list::SIZE));
                let ce = m.ptr(cl + list::ELEMS).wrapping_add((m.i32(r + arel::ELEMID) as u32).wrapping_mul(elem::SIZE));
                if ce != 0 && m.i32(ce + elem::HIER_LEVEL) != 0 {
                    let obj = m.ptr(ce + elem::DATA);
                    h.hieranim_add_item(m, ha, entry, 0, -1, -1, 1, obj);
                    let n = m.i32(count).wrapping_add(1);
                    m.set_i32(count, n);
                }
            }
            i += 1;
        }
        g += 1;
    }
    if m.i32(ha + f::HA_CHAPTER) == glob::chap(m, cid::MODELS) {
        let mut s = ed::find_son(m, h, e);
        while s != 0 {
            elemdict_store_anim_in_hastruct(m, h, d, ha, s, entry, count);
            s = ed::find_next_son(m, h, e);
        }
    }
}

/// Removes the animation of an element and of its descendants
/// [soft 0x0822ea50 animRemoveFromHierarchy]: `SI_edRemoveElemChapAnim(dict, chapter,
/// element, index, delete, 0)` for the element, then the same for each element of that
/// chapter that names it as father (backward entries of relation 110 in list 1), recursively.
pub fn anim_remove_from_hierarchy<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, chap: i32, e: Addr, index: i32, del: i16) {
    reset::ed_remove_elem_chap_anim(m, h, d, chap, e, index, del, 0);
    let mut s = ed::find_related_elem(m, d, e, false, 1, 0x6e, chap);
    while s.elem != 0 {
        anim_remove_from_hierarchy(m, h, d, chap, s.elem, s.index, del);
        s = ed::find_next_related_elem(m, d, e, false, 1, 0x6e, chap);
    }
}

/// Ties an animation file to an element [soft 0x0822ebe0 animationAssociate]; returns 1 when
/// it did. Nothing without the file's element, its record or the element.
///
/// An element that already has an animation file (a forward relation of its chapter's
/// animation-file id to the ANIMATION chapter, in list 0) is refused: "Item already has
/// some animation.". Otherwise the relation is added (`SI_elemdictAddRelation(dict, &SI_RD,
/// index, file index, id, 1, 0)`), and the file's curves are tied by name
/// (`SI_elemdictSetAnimRelation(dict, &SI_RD, file element, −1, element, getbutton(0x66), 0,
/// SI_graflistTypeCount(list, 0, 0, 0, 0) == 1, 0)`). If that matched nothing: "No match
/// between items and animation names." (the relation stays). Else the file's actions are
/// restored (`SI_actionsTmpActivateAll(dict, record + 0xc, 1)`) and the element's animation
/// name is set from its own (`SI_edChapterAnimElemChangeName`).
pub fn animation_associate<H: AssocHost + ?Sized>(m: &mut Mem, h: &mut H, chap: i32, e: Addr, index: i32, ae: Addr, ai: i32) -> i32 {
    if ae == 0 || m.ptr(ae + elem::DATA) == 0 || e == 0 {
        return 0;
    }
    let relid = h.rel_get_anim_f_rel_from_chapter(m, chap);
    let (d, ani) = (glob::cur_dict(m), glob::chap(m, cid::ANIMATION));
    let message;
    if ed::find_related_elem(m, d, e, true, 0, relid, ani).elem == 0 {
        let d = glob::cur_dict(m);
        h.elemdict_add_relation(m, d, glob::SI_RD, index, ai, relid, 1, 0);
        let button = h.getbutton(m, 0x66) as i16;
        let gl = glob::cur_graf_list(m);
        let single = h.graflist_type_count(m, gl, 0, 0, 0, 0) == 1;
        let d = glob::cur_dict(m);
        if h.elemdict_set_anim_relation(m, d, glob::SI_RD, ae, -1, e, button as i32, 0, single as i32, 0) != 0 {
            let ha = m.ptr(ae + elem::DATA);
            if ha != 0 {
                let d = glob::cur_dict(m);
                h.actions_tmp_activate_all(m, d, ha + f::HA_ACTIONS, 1);
            }
            let node = m.ptr(e + elem::NODE);
            let (a, b) = (m.ptr(node.wrapping_add(f::NODE_NAME_14)), m.ptr(node.wrapping_add(f::NODE_NAME_10)));
            h.ed_chapter_anim_elem_change_name(m, e, chap, a, b, a);
            return 1;
        }
        message = text::NO_MATCH;
    } else {
        message = text::HAS_ANIMATION;
    }
    KeyHost::status_display_message(h, m, message, 2);
    0
}

/// Unties an animation file from an element [soft 0x0822ed7c animationDisassociate]; returns
/// 1 when it did. Nothing without the file's element or the element.
///
/// Refusals, each a text on the status line (kind 2): the file's element has no backward
/// relation in list 0 ("Animation file not associated to anything"); the first of them is
/// 15002, 15009 or 15010, the links of lattices, shapes and clusters ("Can't disassociate
/// lattice, shape or cluster animation for now"); the element has no animation file ("Model
/// currently has no animation..") or another one ("Animation file not used by selected
/// element").
///
/// Otherwise the element's present animation is written into a new record
/// (`SI_hieranimAllocate(chapter)`), so that the file holds what the hierarchy had: the old
/// record's actions move over (opened and unselected first, the old record's two words
/// cleared), and the hierarchy's curves are filed ([`elemdict_store_anim_in_hastruct`]).
/// With at least one curve and an old record, the old record is disposed of and freed and
/// the new one takes its place; with no curve and no actions the new one is freed and the
/// old stays; in the remaining cases the new one replaces the old without the old being
/// freed. The picked graf node's object (+4) becomes the file's record, the hierarchy's
/// animation is removed ([`anim_remove_from_hierarchy`], without deleting), and the
/// relation pair to the file goes (`SI_elemdictRemoveRelationPair`).
///
/// The new record is read at +0xc even when the allocation gave null; the image records a
/// fault where the original would stop.
#[allow(clippy::too_many_arguments)]
pub fn animation_disassociate<H: AssocHost + ?Sized>(m: &mut Mem, h: &mut H, chap: i32, e: Addr, index: i32, ae: Addr, ai: i32, picked: Addr) -> i32 {
    if ae == 0 || e == 0 {
        return 0;
    }
    let back = m.ptr(ae + elem::BACKWARD + 4);
    let message;
    'refused: {
        if m.i32(back + rellist::NB) < 1 {
            message = text::NOT_ASSOCIATED;
            break 'refused;
        }
        let first = m.i32(m.ptr(back + rellist::RELS) + arel::RELID);
        if first == 0x3a9a || first == 0x3aa1 || first == 0x3aa2 {
            message = text::NOT_FROM_PARTS;
            break 'refused;
        }
        let relid = h.rel_get_anim_f_rel_from_chapter(m, chap);
        let (d, ani) = (glob::cur_dict(m), glob::chap(m, cid::ANIMATION));
        let used = ed::find_related_elem(m, d, e, true, 0, relid, ani).elem;
        if used == 0 {
            message = text::NO_ANIMATION;
            break 'refused;
        }
        if ae != used {
            message = text::NOT_USED;
            break 'refused;
        }
        let count = m.local(4);
        m.set_i32(count, 0);
        let ha = h.hieranim_allocate(m, chap);
        if ha != 0 && m.ptr(ae + elem::DATA) != 0 {
            let d = glob::cur_dict(m);
            let old = m.ptr(ae + elem::DATA);
            h.actions_tmp_activate_all(m, d, old + f::HA_ACTIONS, 0);
            let old = m.ptr(ae + elem::DATA);
            h.actions_unselect_all(m, old + f::HA_ACTIONS);
            let old = m.ptr(ae + elem::DATA);
            let v = m.u32(old + f::HA_ACTIONS_ARRAY);
            m.set_u32(ha + f::HA_ACTIONS_ARRAY, v);
            let v = m.u32(old + f::HA_ACTIONS);
            m.set_u32(ha + f::HA_ACTIONS, v);
            m.set_u32(old + f::HA_ACTIONS_ARRAY, 0);
            m.set_u32(old + f::HA_ACTIONS, 0);
        }
        let d = glob::cur_dict(m);
        elemdict_store_anim_in_hastruct(m, h, d, ha, e, -1, count);
        let n = m.i32(count);
        m.release(4);
        let mut replace = true;
        if n < 1 || m.ptr(ae + elem::DATA) == 0 {
            if n < 1 && m.i32(ha.wrapping_add(f::HA_ACTIONS)) < 1 {
                if ha != 0 {
                    m.free(ha);
                }
                replace = false;
            }
        } else {
            let old = m.ptr(ae + elem::DATA);
            h.hieranim_dispose(m, old);
            let old = m.ptr(ae + elem::DATA);
            if old != 0 {
                m.free(old);
                m.set_ptr(ae + elem::DATA, 0);
            }
        }
        if replace {
            m.set_ptr(ae + elem::DATA, ha);
        }
        let rec = m.ptr(ae + elem::DATA);
        m.set_ptr(picked + grafnode::DATA, rec);
        let d = glob::cur_dict(m);
        anim_remove_from_hierarchy(m, h, d, chap, e, index, 0);
        let d = glob::cur_dict(m);
        h.elemdict_remove_relation_pair(m, d, glob::SI_RD, index, ai, relid, 1, 0);
        return 1;
    }
    KeyHost::status_display_message(h, m, message, 2);
    0
}

/// The "associate" and "disassociate" commands [soft 0x0822da04 si_animDialAssociate]: the
/// user picks an animation item on screen, and it is tied to (or untied from) the selection.
///
/// To associate, at most one hierarchy may be selected ("You must have only one hierarchy
/// selected"). The picked node (`SI_gLSelect(event's window)`; nothing happens without one)
/// must be an animation item (graf kind 0x12: "You must pick an animation item..") that an
/// ANIMATION element shows ("The animation file element seems invalid"), and to associate,
/// that element must not be in use already (no backward relation in its list 0: "Sorry,
/// Anim file elements cannot be shared for now").
///
/// Then every selected node of the current graf list whose kind is not 3, 0xc, 0xd or 0x12
/// is treated: [`animation_disassociate`], or [`animation_associate`] when the file has no
/// record or a record for the node's chapter (a record for the CLUSTERS, SHAPES or LATTICES
/// chapter: "Can't associate to lattice, shape or cluster animation."; for another chapter:
/// "Anim file not compatible with selected element."). After each success the node and the
/// picked item flash (`SI_foolDisplay` in two colours, `swapbuffers`, half a second's
/// `usleep`, `SI_refreshSystem(0, 0, 0x25)`). At the end `SI_refreshSystem(0, 0, 0x27)`.
///
/// The two locals the element look-up writes through start as zero here.
pub fn anim_dial_associate<H: AssocHost + ?Sized>(m: &mut Mem, h: &mut H, event: Addr, associate: i16) {
    if associate != 0 {
        let gl = glob::cur_graf_list(m);
        if h.graflist_count_sel_hierarchies(m, gl, 0) >= 2 {
            KeyHost::status_display_message(h, m, text::ONE_HIERARCHY, 1);
            return;
        }
    }
    let picked = h.gl_select(m, m.ptr(event + f::EVENT_PORT));
    if picked == 0 {
        return;
    }
    if m.i32(picked + grafnode::KIND) != 0x12 {
        KeyHost::status_display_message(h, m, text::PICK_ANIMATION, 1);
        return;
    }
    let (d, ani) = (glob::cur_dict(m), glob::chap(m, cid::ANIMATION));
    let ai = h.elemdict_find_elem_by_aux_data(m, d, ani, picked);
    if ai < 0 {
        KeyHost::status_display_message(h, m, text::INVALID, 2);
        return;
    }
    let (d, ani) = (glob::cur_dict(m), glob::chap(m, cid::ANIMATION));
    let ae = ed::get_elem(m, d, ani, ai);
    if associate != 0 && m.i32(m.ptr(ae.wrapping_add(elem::BACKWARD + 4)) + rellist::NB) != 0 {
        KeyHost::status_display_message(h, m, text::SHARED, 2);
        return;
    }
    let ha = m.ptr(ae.wrapping_add(elem::DATA));
    let out = m.local(8);
    let mut done: i16 = 0;
    let mut gn = m.ptr(glob::cur_graf_list(m) + 4);
    while gn != 0 {
        let kind = m.i32(gn + grafnode::KIND);
        if kind != 3 && kind != 0x12 && kind != 0xd && kind != 0xc && m.i32(gn + grafnode::SELECTION) != 0 {
            let e = h.elemdict_find_elem_from_graf_node(m, gn, out, out + 4);
            let (chap, index) = (m.i32(out), m.i32(out + 4));
            if associate == 0 {
                done = animation_disassociate(m, h, chap, e, index, ae, ai, picked) as i16;
            } else if ha == 0 {
                done = animation_associate(m, h, chap, e, index, ae, ai) as i16;
            } else {
                let hc = m.i32(ha + f::HA_CHAPTER);
                if hc == glob::chap(m, cid::CLUSTERS) || hc == glob::chap(m, cid::SHAPES) || hc == glob::chap(m, cid::LATTICES) {
                    KeyHost::status_display_message(h, m, text::NOT_TO_PARTS, 2);
                } else if hc == chap {
                    done = animation_associate(m, h, chap, e, index, ae, ai) as i16;
                } else {
                    KeyHost::status_display_message(h, m, text::INCOMPATIBLE, 2);
                }
            }
            if done != 0 {
                let gl = glob::cur_graf_list(m);
                h.fool_display(m, gl, 0, 1, gn);
                let gl = glob::cur_graf_list(m);
                h.fool_display(m, gl, 0, 2, picked);
                h.swapbuffers(m);
                h.usleep(m, 500_000);
                h.refresh_system(m, 0, 0, 0x25);
                done = 0;
            }
        }
        gn = m.ptr(gn + grafnode::NEXT);
    }
    m.release(8);
    h.refresh_system(m, 0, 0, 0x27);
}

/// Turns the PostScript preview on or off [soft 0x0822dcbc SI_postscriptPreview]: the flag
/// `si_psPrevFlag` takes the argument; turning it on (exactly 1) asks for a file name
/// (`SI_getFilename(".ps", "frame")`), and a name given replaces `si_psPrevFile` (the old
/// one freed, the new a `strdup`).
pub fn postscript_preview<H: AssocHost + ?Sized>(m: &mut Mem, h: &mut H, on: i16) {
    m.set_i16(play::g::SI_PS_PREV_FLAG, on);
    if on == 1 {
        let name = h.get_filename(m, text::PS_SUFFIX, text::PS_DEFAULT);
        if name != 0 {
            let old = m.ptr(SI_PS_PREV_FILE);
            if old != 0 {
                m.free(old);
                m.set_ptr(SI_PS_PREV_FILE, 0);
            }
            let copy = m.strdup(name);
            m.set_ptr(SI_PS_PREV_FILE, copy);
        }
    }
}

/// Writes the highlighted window as the PostScript file of a frame
/// [soft 0x0822dd24 si_postscriptDumpFrame]: nothing without a highlighted window; else the
/// name is `si_psPrevFile` + "." + the frame in decimal + ".ps", built in two blocks from
/// `SI_calloc` (the name's length + 15 bytes; 10 bytes for the number) that are never freed,
/// and handed to `SI_postscriptRefresh(window, name)`.
///
/// The number is printed into the 10-byte block whatever its length: "." and a frame of
/// nine characters or more (a frame beyond 99,999,999, or below −9,999,999) run past it
/// into the allocator's padding. Here the block takes its first ten bytes and the name is
/// built from the whole text, which is what the original's `strcat` reads back.
///
/// The name's block has room for a number of ten characters. With eleven (a frame of
/// −1,000,000,000 or below) the original's terminating zero lies one byte behind the block:
/// in the allocator's padding, or on the first byte of the next block's header. Here the
/// name is cut at the end of its block, so that it has no terminating zero inside it; what
/// `SI_postscriptRefresh` is handed then differs from the original's by that byte.
///
/// A null `si_psPrevFile` is measured for its length by the original (it stops there); the
/// image records the fault.
pub fn postscript_dump_frame<H: AssocHost + ?Sized>(m: &mut Mem, h: &mut H, frame: i32) {
    let slot = m.local(4);
    m.set_ptr(slot, 0);
    h.window_get_hilite(m, slot);
    let port = m.ptr(slot);
    m.release(4);
    if port == 0 {
        return;
    }
    let base = m.cstr(m.ptr(SI_PS_PREV_FILE));
    let size = base.len() + 0xf;
    let name = m.si_calloc(size as u32, 1);
    m.put_cstr(name, &base);
    let number = m.si_calloc(10, 1);
    let mut text = format!(".{frame}").into_bytes();
    text.push(0);
    m.set_bytes(number, &text[..text.len().min(10)]);
    let mut whole = base;
    whole.extend_from_slice(&text[..text.len() - 1]);
    whole.extend_from_slice(b".ps\0");
    m.set_bytes(name, &whole[..whole.len().min(size)]);
    h.postscript_refresh(m, port, name);
}
