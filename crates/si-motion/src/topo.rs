// SPDX-License-Identifier: MIT
//! The update list and the topological sort that fills it: `updlist.c` and the part of
//! `toposort.c` the update at a time stands on (finding 0017, "The order").
//!
//! An element's six **forward** relation lists say what each aspect of the element is made
//! from: list 0 its parameters, 1 its transformation, 2 its geometry, 3 its rendering
//! links, 4 custom effects, 5 the rest. An entry names the element it draws on and the list
//! of that element which holds the other end; a function curve is such an element (chapter
//! ANIMATION) and so is a father, a path, a constraining object.
//!
//! Sorting a list is a depth-first walk: for each entry in list order (which is rank
//! order), first the list of the other element that the entry points at is sorted, then,
//! at the end of each run of entries with the same relation id and serial number, one
//! update node is appended for the run. So everything a step reads has been brought up to
//! date before the step, and the steps of one list run in rank order.

use crate::ed;
use crate::glob::{self, cid};
use si_mem::{Addr, Mem};
use crate::off::{arel, dict, elem, list, rellist, updelem, updlist, updnode};

crate::host_trait! {
    /// What the sort calls outside this package.
    pub trait TopoHost {
        /// `SI_reldictIsRelActive(reldict, relation, 1)` (reldict.c): the rule's "active for
        /// dependencies" flag.
        fn reldict_is_rel_active(reldict: Addr, relid: i32, dep: i32) -> i16;
        /// `si_reldictGetRelUpdlattr(reldict, relation, &pur, &pre)` (reldict.c): the two
        /// "update" columns of the relation's `ARG` line in `Relations.rsrc`.
        fn reldict_get_rel_updlattr(reldict: Addr, relid: i32, pur: Addr, pre: Addr);
        /// `SI_elemdictGetRelation(dict, elem, relation, chapter, -1)` (elemdict.c).
        fn elemdict_get_relation(dict: Addr, elem: Addr, relid: i32, chap: i32, nth: i32) -> Addr;
        /// `SI_dbCheckExtraUnToldRelation(dict, on, forward, 0)` (edtools.c): adds, or takes
        /// away again, the relations the scene implies without storing them.
        fn db_check_extra_un_told_relation(dict: Addr, on: i16, forward: i16, c: i16);
        /// `SI_cusResortUpdList(list)` (custom_dso.c): custom effects reorder the list.
        fn cus_resort_upd_list(list: Addr);
        /// `printf("TopoSort: help! level > 100 …")`.
        fn printf_toposort_too_deep();
    }
}

/// [soft 0x08141468 SI_updlistInit]: empty and dirty.
pub fn updlist_init(m: &mut Mem, l: Addr) {
    m.set_i32(l + updlist::NB, 0);
    m.set_ptr(l + updlist::NODES, 0);
    m.set_i32(l + updlist::DIRTY, 1);
}

/// Appends one argument to an argument array and returns the array
/// [soft 0x081413e0 si_updlistAddArgument]: the other element's chapter, index and list
/// from the relation entry, the element's object, the entry's activation flag, and the
/// relation's activation curve.
pub fn updlist_add_argument(m: &mut Mem, args: Addr, count: &mut i32, rel: Addr, data: Addr, act_fcv: Addr) -> Addr {
    let n = *count;
    let a = if args == 0 { m.si_malloc(updelem::SIZE) } else { m.si_realloc(args, (n as u32 * 5 + 5) * 4) };
    let p = a + n as u32 * updelem::SIZE;
    m.set_i16(p + updelem::CHAPID, m.i16(rel + arel::CHAPID));
    m.set_i32(p + updelem::ELEMID, m.i32(rel + arel::ELEMID));
    m.set_i16(p + updelem::GROUP, m.i16(rel + arel::GROUP));
    m.set_ptr(p + updelem::DATA, data);
    m.set_i16(p + updelem::ACTIVE, m.i16(rel + arel::ACTIVE));
    m.set_ptr(p + updelem::ACT_FCV, act_fcv);
    *count += 1;
    a
}

/// Appends a node [soft 0x08141488 si_updlistAddNode]. A model element without an object
/// gets no node (and the argument array handed in is then lost, as in the original). The
/// node is active unless the relation entry carries data, in which case it takes the
/// entry's activation flag and data pointer. The words at +0x0c and +0x10 are not set.
#[allow(clippy::too_many_arguments)]
pub fn updlist_add_node(m: &mut Mem, l: Addr, chap: i32, elemid: i32, e: Addr, relid: i32, rel: Addr, group: i32, nb_args: i32,
                        args: Addr) {
    if m.ptr(e + elem::DATA) == 0 && chap == glob::chap(m, cid::MODELS) {
        return;
    }
    let n = m.i32(l + updlist::NB);
    let nodes = m.ptr(l + updlist::NODES);
    let nodes = if nodes == 0 { m.si_malloc(updnode::SIZE) } else { m.si_realloc(nodes, (n as u32 + 1) * updnode::SIZE) };
    m.set_ptr(l + updlist::NODES, nodes);
    let p = nodes + n as u32 * updnode::SIZE;
    m.set_i16(p + updnode::CHAPID, chap as i16);
    m.set_i16(p + updnode::GROUP, group as i16);
    m.set_i32(p + updnode::ELEMID, elemid);
    m.set_ptr(p + updnode::DATA, m.ptr(e + elem::DATA));
    m.set_i32(p + updnode::RELID, relid);
    m.set_i16(p + updnode::ACTIVE, 1);
    m.set_ptr(p + updnode::REL_DATA, 0);
    if rel != 0 && m.ptr(rel + arel::DATA) != 0 {
        m.set_ptr(p + updnode::REL_DATA, m.ptr(rel + arel::DATA));
        m.set_i16(p + updnode::ACTIVE, m.i16(rel + arel::ACTIVE));
    }
    m.set_ptr(p + updnode::OWNED_28, 0);
    m.set_i16(p + updnode::NB_ARGS, nb_args as i16);
    m.set_ptr(p + updnode::ARGS, args);
    m.set_i32(l + updlist::NB, n + 1);
}

/// Frees a list's nodes with their argument arrays and owned blocks, and leaves it empty
/// and dirty [soft 0x08141554 SI_updlistDispose].
pub fn updlist_dispose(m: &mut Mem, l: Addr) {
    let mut i = 0;
    while i < m.i32(l + updlist::NB) {
        let p = m.ptr(l + updlist::NODES) + i as u32 * updnode::SIZE;
        for at in [updnode::ARGS, updnode::OWNED_28] {
            let b = m.ptr(p + at);
            if b != 0 {
                m.free(b);
                m.set_ptr(p + at, 0);
            }
        }
        i += 1;
    }
    let nodes = m.ptr(l + updlist::NODES);
    if nodes != 0 {
        m.free(nodes);
        m.set_ptr(l + updlist::NODES, 0);
    }
    updlist_init(m, l);
}

/// Clears the sort's marks in one chapter [soft 0x0813ff44 relTopoSortInitChapter]: state
/// and cursor of every relation list (forward or backward) of every live element.
pub fn rel_topo_sort_init_chapter(m: &mut Mem, l: Addr, forward: bool) {
    let mut i = 0;
    while i < m.i32(l + list::NB_ELEMS) {
        let e = m.ptr(l + list::ELEMS) + i as u32 * elem::SIZE;
        i += 1;
        if m.i16(e + elem::STATUS) == 0 {
            let rels = if forward { e + elem::FORWARD } else { e + elem::BACKWARD };
            let mut g = 0;
            while g < m.i32(rels) {
                let rl = m.ptr(rels + 4) + g as u32 * rellist::SIZE;
                m.set_i32(rl + rellist::TOPO_STATE, 0);
                m.set_i32(rl + rellist::CUR, 0);
                g += 1;
            }
        }
    }
}

/// [soft 0x0813ffbc SI_relTopoSortInit]: every chapter of the dictionary.
pub fn rel_topo_sort_init(m: &mut Mem, d: Addr, forward: bool) {
    let mut c = 0;
    while c < m.i32(d + dict::NB_LISTS) {
        let l = m.ptr(d + dict::LISTS) + c as u32 * list::SIZE;
        rel_topo_sort_init_chapter(m, l, forward);
        c += 1;
    }
}

/// The arguments of the run of relation entries that starts at `start`
/// [soft 0x0813fff8 relCreateArguments]: one argument per entry while the following entry
/// has the same relation id and serial number. Returns (array, count).
///
/// The other element is fetched with `SI_elemdictGetElem` and its object read without a
/// null test: an entry that points at a deleted element reads address 4 in the original.
pub fn rel_create_arguments(m: &mut Mem, d: Addr, rl: Addr, start: i32) -> (Addr, i32) {
    let mut count = 0;
    let mut args = 0;
    let mut i = start;
    loop {
        let r = m.ptr(rl + rellist::RELS).wrapping_add((i as u32).wrapping_mul(arel::SIZE));
        let e = ed::get_elem(m, d, m.i16(r + arel::CHAPID) as i32, m.i32(r + arel::ELEMID));
        let act = ed::get_fcv_from_relation(m, r);
        let data = m.ptr(e.wrapping_add(elem::DATA));
        args = updlist_add_argument(m, args, &mut count, r, data, act);
        if m.i32(rl + rellist::NB) - 1 <= i {
            return (args, count);
        }
        let next = r + arel::SIZE;
        if m.i32(next + arel::RELID) != m.i32(r + arel::RELID) || m.i32(next + arel::SERIAL) != m.i32(r + arel::SERIAL) {
            return (args, count);
        }
        i += 1;
    }
}

/// The global word at 0x0878158e (inside `SI_glbManip`): with it set, a forward sort asked
/// to do so (`roots` argument of [`topo_sort`]) climbs hierarchy relations to the first
/// already marked ancestor instead of sorting each father in turn.
pub const HIER_SHORTCUT: Addr = 0x0878_158e;

/// What a sort is asked to do, the four flag arguments of the original's `topoSort`.
#[derive(Clone, Copy, Debug)]
pub struct Mode {
    /// Only look for cycles: no nodes are appended.
    pub check_only: bool,
    /// Walk forward lists (what an element is made from). The whole-scene sort and the
    /// per-element sort do; the sorts "by selection" and "by curves" walk backward lists
    /// (what depends on an element) and mark every element they reach as selected.
    pub forward: bool,
    /// Allows the hierarchy short cut (see [`HIER_SHORTCUT`]).
    pub roots: bool,
}

/// Sorts one relation list of one element into `ul` [soft 0x081400cc topoSort]. Returns
/// false for a cycle (a list met again while it is being sorted) and for more than 100
/// levels of recursion.
///
/// For each entry from the list's cursor on, in order:
///
/// 1. A relation the dictionary has switched off for dependencies is passed over.
/// 2. The other end must be a live element of a chapter with files; otherwise the entry is
///    passed over **without** advancing the cursor.
/// 3. The other element's list (the same direction, the list index stored in the entry) is
///    looked at: not yet sorted and the entry active → it is sorted first, recursively;
///    being sorted → a cycle, unless the entry is inactive or points back at this very
///    list, or the "pure dependency" mode is on (below), or the other list's current entry
///    is a `HierLeafDep` (−102).
/// 4. At the last entry of a run (same relation id and serial number): unless the relation
///    is a pure dependency (`pur` column non-zero) or the list is 3 (rendering), and if
///    any entry of the run is active, one node is appended for the run with the run's
///    entries as arguments. A relation with the `pre` column set first gets a
///    transformation step (−100) of this element, so that a constraint sees the node where
///    its curves put it. The node carries the data and activation of the first entry of
///    the run that has data (expressions: of the first whose other end is not an
///    EXPRESSIONS element).
/// 5. A custom effect (20000) of a model on itself in list 1 appends a transformation step
///    before anything else.
///
/// After the entries: a MODELS or WAVES element for which at least one node was appended
/// gets its closing step: list 1 the transformation update (−100, waves −101), list 2 the
/// geometry update (−200).
///
/// "Pure dependency" mode (the last argument of the original) is switched on for the walk
/// below an entry of relation −102, −103, −104 or −110, or into the CHANNEL_DRIVERS
/// chapter, and makes lists that are being sorted count as done.
///
/// The original starts at `cursor − (SI_lightLicense − SI_lightLicense2)`: a product-tier
/// term, zero in the full product, which is the path taken here.
#[allow(clippy::too_many_arguments)]
pub fn topo_sort<H: TopoHost + ?Sized>(m: &mut Mem, h: &mut H, rd: Addr, d: Addr, ul: Addr, e: Addr, rels: Addr, chap: i32,
                                       elemid: i32, group: i32, depth: i32, mode: Mode, pure_in: bool)
                                       -> bool {
    let mut pure = pure_in;
    if depth >= 0x65 {
        h.printf_toposort_too_deep(m);
        return false;
    }
    let models = glob::chap(m, cid::MODELS);
    let rl = m.ptr(rels + 4) + group as u32 * rellist::SIZE;
    m.set_i32(rl + rellist::TOPO_STATE, 1);
    if !mode.forward {
        m.set_i16(e + elem::SELECTED, 1);
    }
    let mut appended = 0;
    let mut i = m.i32(rl + rellist::CUR);
    while i < m.i32(rl + rellist::NB) {
        let r = m.ptr(rl + rellist::RELS) + i as u32 * arel::SIZE;
        let id = m.i32(r + arel::RELID);
        if h.reldict_is_rel_active(m, rd, id, 1) == 0 {
            m.set_i32(rl + rellist::CUR, i + 1);
            i += 1;
            continue;
        }
        let mut oc = m.i16(r + arel::CHAPID) as i32;
        let mut oi = m.i32(r + arel::ELEMID);
        let mut og = m.i16(r + arel::GROUP) as i32;
        let ol = m.ptr(d + dict::LISTS).wrapping_add((oc as u32).wrapping_mul(list::SIZE));
        if m.i16(ol + list::KIND) != 1 {
            i += 1;
            continue;
        }
        let mut oe = m.ptr(ol + list::ELEMS).wrapping_add((oi as u32).wrapping_mul(elem::SIZE));
        if m.i16(oe + elem::STATUS) != 0 {
            i += 1;
            continue;
        }
        let mut orels = if mode.forward { oe + elem::FORWARD } else { oe + elem::BACKWARD };
        if mode.forward && id == -0x6e {
            // a ChainUpdate entry in a forward walk is passed over
            m.set_i32(rl + rellist::CUR, i + 1);
            i += 1;
            continue;
        }
        if m.i16(HIER_SHORTCUT) != 0 && mode.forward && mode.roots && id == 0x6e {
            // climb to the first marked ancestor and sort from there
            let mut climb = true;
            while m.i16(oe + elem::SELECTED) == 0 {
                let up = h.elemdict_get_relation(m, d, oe, 0x6e, models, -1);
                if up == 0 {
                    climb = false;
                    break;
                }
                oc = m.i16(up + arel::CHAPID) as i32;
                oi = m.i32(up + arel::ELEMID);
                og = m.i16(up + arel::GROUP) as i32;
                let l2 = m.ptr(d + dict::LISTS).wrapping_add((oc as u32).wrapping_mul(list::SIZE));
                if m.i16(l2 + list::KIND) != 1 {
                    climb = false;
                    break;
                }
                oe = m.ptr(l2 + list::ELEMS).wrapping_add((oi as u32).wrapping_mul(elem::SIZE));
                if m.i16(oe + elem::STATUS) != 0 {
                    climb = false;
                    break;
                }
                orels = oe + elem::FORWARD;
            }
            if climb && !topo_sort(m, h, rd, d, ul, oe, orels, oc, oi, og, depth + 1, mode, pure) {
                return false;
            }
            m.set_i32(rl + rellist::CUR, i + 1);
            i += 1;
            continue;
        }
        let other = m.ptr(orels + 4).wrapping_add((og as u32).wrapping_mul(rellist::SIZE));
        let mut state = m.i32(other + rellist::TOPO_STATE);
        let active = m.i16(r + arel::ACTIVE) != 0;
        if !mode.check_only && orels == rels && active && id == 20000 && chap == models && group == 1 && og == 1 {
            updlist_add_node(m, ul, chap, elemid, e, -100, 0, 1, 0, 0);
        }
        let harmless = orels == rels || !active;
        let mut done = harmless;
        if state == 1 && m.ptr(other + rellist::RELS) != 0 {
            let cur = m.ptr(other + rellist::RELS).wrapping_add((m.i32(other + rellist::CUR) as u32).wrapping_mul(arel::SIZE));
            done = harmless || m.i32(cur + arel::RELID) == -0x66;
        }
        if (done || pure) && state == 1 {
            state = 2;
        }
        if state == 0 && active {
            pure = pure || matches!(id, -0x66 | -0x67 | -0x68 | -0x6e);
            if oc == glob::chap(m, cid::CHANNEL_DRIVERS) {
                pure = true;
            }
            if !topo_sort(m, h, rd, d, ul, oe, orels, oc, oi, og, depth + 1, mode, pure) {
                return false;
            }
        } else if state == 1 {
            return false;
        }
        // the end of a run of entries with one relation id and serial number
        let nb = m.i32(rl + rellist::NB);
        let serial = m.i32(r + arel::SERIAL);
        let run_ends = nb - 1 <= i || {
            let next = r + arel::SIZE;
            id != m.i32(next + arel::RELID) || serial != m.i32(next + arel::SERIAL)
        };
        if run_ends {
            if !mode.check_only {
                let out = m.local(8);
                h.reldict_get_rel_updlattr(m, rd, id, out, out + 4);
                let (pur, pre) = (m.i32(out), m.i32(out + 4));
                m.release(8);
                if pur == 0 && group != 3 {
                    if pre != 0 {
                        updlist_add_node(m, ul, chap, elemid, e, -100, 0, 1, 0, 0);
                    }
                    let arr = m.ptr(rl + rellist::RELS);
                    let same = |m: &Mem, k: i32| {
                        let p = arr + k as u32 * arel::SIZE;
                        m.i32(p + arel::RELID) == id && m.i32(p + arel::SERIAL) == serial
                    };
                    // any entry of the run active
                    let mut any: u16 = 0;
                    let mut k = i;
                    while k >= 0 && same(m, k) {
                        any |= m.u16(arr + k as u32 * arel::SIZE + arel::ACTIVE);
                        k -= 1;
                    }
                    // back to the first entry of the run that carries data
                    let expressions = glob::chap(m, cid::EXPRESSIONS);
                    let mut k = i;
                    while k >= 0 {
                        let p = arr + k as u32 * arel::SIZE;
                        if !same(m, k)
                           || (m.ptr(p + arel::DATA) != 0
                               && (m.i32(p + arel::RELID) != 0x4e2a || m.i16(p + arel::CHAPID) as i32 == expressions))
                        {
                            break;
                        }
                        k -= 1;
                    }
                    if any != 0 {
                        let mut head = r;
                        if k >= 0 && m.i32(arr + k as u32 * arel::SIZE + arel::RELID) == id {
                            head = arr + k as u32 * arel::SIZE;
                        }
                        let start = m.i32(rl + rellist::CUR);
                        let (args, n) = rel_create_arguments(m, d, rl, start);
                        updlist_add_node(m, ul, chap, elemid, e, id, head, group, n, args);
                        appended += 1;
                    }
                }
            }
            m.set_i32(rl + rellist::CUR, i + 1);
        }
        i += 1;
    }
    let waves = glob::chap(m, cid::WAVES);
    if !mode.check_only && m.i32(rl + rellist::NB) > 0 && (chap == models || chap == waves) && appended > 0 {
        if group == 1 {
            updlist_add_node(m, ul, chap, elemid, e, if chap == waves { -0x65 } else { -100 }, 0, 1, 0, 0);
        } else if group == 2 {
            updlist_add_node(m, ul, chap, elemid, e, -200, 0, 2, 0, 0);
        }
    }
    m.set_i32(rl + rellist::TOPO_STATE, 2);
    true
}

/// Sorts the forward lists of the elements of one chapter
/// [soft 0x0814070c elemdictTopoSortChapter]: elements in index order (only hierarchy roots
/// with `roots_only`, only marked ones with `selected_only`), each element's lists 0 to 5
/// in order, a list being taken when it takes part (its word +0x14) and is either not
/// sorted yet or not finished (cursor short of its count).
#[allow(clippy::too_many_arguments)]
pub fn elemdict_topo_sort_chapter<H: TopoHost + ?Sized>(m: &mut Mem, h: &mut H, rd: Addr, d: Addr, ul: Addr, l: Addr,
                                                        chap: i32, check_only: bool, roots_only: bool, selected_only: bool)
                                                        -> bool {
    let mode = Mode { check_only, forward: true, roots: selected_only };
    let mut i = 0;
    while i < m.i32(l + list::NB_ELEMS) {
        let e = m.ptr(l + list::ELEMS) + i as u32 * elem::SIZE;
        if m.i16(e + elem::STATUS) == 0
           && (!roots_only || m.i32(e + elem::HIER_LEVEL) == 0)
           && (!selected_only || m.i16(e + elem::SELECTED) != 0)
        {
            let mut g = 0;
            while g < m.i32(e + elem::FORWARD) {
                let rl = m.ptr(e + elem::FORWARD + 4) + g as u32 * rellist::SIZE;
                if m.i16(rl + rellist::FLAG) != 0
                   && (m.i32(rl + rellist::TOPO_STATE) == 0 || m.i32(rl + rellist::CUR) < m.i32(rl + rellist::NB))
                   && !topo_sort(m, h, rd, d, ul, e, e + elem::FORWARD, chap, i, g, 0, mode, false)
                {
                    return false;
                }
                g += 1;
            }
        }
        i += 1;
    }
    true
}

/// Sorts the whole dictionary into `ul` [soft 0x08140828 SI_elemdictTopoSort]. Nothing
/// happens unless the list is dirty (its third word; every change to the dictionary's
/// elements or relations sets it).
///
/// 1. Unless `selected_only`, the implied relations are added
///    (`SI_dbCheckExtraUnToldRelation(dict, 1, 1, 0)`).
/// 2. The marks are cleared; unless `check_only`, the list is emptied.
/// 3. Chapters in dictionary order (finding 0010: SCENES, ANIMATION, CAMERAS, CLUSTERS, …
///    LIGHTS, MATERIALS, MODELS, …), those with files only; in each, first the hierarchy
///    roots, then all elements.
/// 4. With the fast hierarchy update on (bit 2 of `SI_viewerUpdOpt`) and a list that is not
///    empty, a record with chapter −1 is appended behind the last node (not counted).
/// 5. The implied relations are taken away again, the list is marked clean, and custom
///    effects may reorder it (`SI_cusResortUpdList`).
///
/// Returns false for a cycle; the list then holds the nodes appended so far and stays
/// dirty. (The original also counts a licence-related global at each allocation; not
/// ported.)
pub fn elemdict_topo_sort<H: TopoHost + ?Sized>(m: &mut Mem, h: &mut H, rd: Addr, d: Addr, ul: Addr, check_only: bool,
                                                selected_only: bool)
                                                -> bool {
    if m.i32(ul + updlist::DIRTY) == 0 {
        return true;
    }
    if !selected_only {
        h.db_check_extra_un_told_relation(m, d, 1, 1, 0);
    }
    rel_topo_sort_init(m, d, true);
    if !check_only {
        updlist_dispose(m, ul);
    }
    let mut c = 0;
    while c < m.i32(d + dict::NB_LISTS) {
        let l = m.ptr(d + dict::LISTS) + c as u32 * list::SIZE;
        if m.i16(l + list::KIND) == 1
           && (!elemdict_topo_sort_chapter(m, h, rd, d, ul, l, c, check_only, true, selected_only)
               || !elemdict_topo_sort_chapter(m, h, rd, d, ul, l, c, check_only, false, selected_only))
        {
            if !selected_only {
                h.db_check_extra_un_told_relation(m, d, 0, 1, 0);
            }
            return false;
        }
        c += 1;
    }
    if m.u8(glob::SI_VIEWER_UPD_OPT) & 4 != 0 && m.i32(ul + updlist::NB) > 0 && !check_only {
        let n = m.i32(ul + updlist::NB) as u32;
        let nodes = m.si_realloc(m.ptr(ul + updlist::NODES), (n + 1) * updnode::SIZE);
        m.set_ptr(ul + updlist::NODES, nodes);
        m.set_i16(nodes + n * updnode::SIZE, -1);
    }
    if !selected_only {
        h.db_check_extra_un_told_relation(m, d, 0, 1, 0);
    }
    if !check_only {
        m.set_i32(ul + updlist::DIRTY, 0);
    }
    h.cus_resort_upd_list(m, ul);
    true
}

/// Sorts one element into the dictionary's second list (+0x4c)
/// [soft 0x081409bc SI_elemdictTopoSortElement]: all its forward lists (`group` negative)
/// or one. What comes out is everything the element is made from, in order, ending with
/// the element's own steps. On success the list is marked clean and handed to
/// `SI_cusResortUpdList`.
///
/// With `group` negative and no list to sort the original returns an uninitialised word;
/// here false.
pub fn elemdict_topo_sort_element<H: TopoHost + ?Sized>(m: &mut Mem, h: &mut H, rd: Addr, d: Addr, chap: i32, elemid: i32,
                                                        group: i32, check_only: bool)
                                                        -> bool {
    let l = m.ptr(d + dict::LISTS).wrapping_add((chap as u32).wrapping_mul(list::SIZE));
    let e = m.ptr(l + list::ELEMS).wrapping_add((elemid as u32).wrapping_mul(elem::SIZE));
    let ul = d + dict::TOPO_LIST;
    updlist_dispose(m, ul);
    h.db_check_extra_un_told_relation(m, d, 1, 1, 0);
    rel_topo_sort_init(m, d, true);
    let mode = Mode { check_only, forward: true, roots: false };
    let mut ok = false;
    if group < 0 {
        let mut g = 0;
        while g < m.i32(e + elem::FORWARD) {
            let rl = m.ptr(e + elem::FORWARD + 4) + g as u32 * rellist::SIZE;
            if m.i16(rl + rellist::FLAG) != 0
               && (m.i32(rl + rellist::TOPO_STATE) == 0 || m.i32(rl + rellist::CUR) < m.i32(rl + rellist::NB))
            {
                ok = topo_sort(m, h, rd, d, ul, e, e + elem::FORWARD, chap, elemid, g, 0, mode, false);
                if !ok {
                    h.db_check_extra_un_told_relation(m, d, 0, 1, 0);
                    return false;
                }
            }
            g += 1;
        }
    } else {
        ok = topo_sort(m, h, rd, d, ul, e, e + elem::FORWARD, chap, elemid, group, 0, mode, false);
    }
    h.db_check_extra_un_told_relation(m, d, 0, 1, 0);
    if ok {
        m.set_i32(ul + updlist::DIRTY, 0);
        h.cus_resort_upd_list(m, ul);
    }
    ok
}

/// [soft 0x08140d80 SI_elemdictCheckCycle]: true when the scene's relations form a cycle
/// (a sort of the scene's list that appends nothing).
pub fn elemdict_check_cycle<H: TopoHost + ?Sized>(m: &mut Mem, h: &mut H, rd: Addr, d: Addr) -> bool {
    !elemdict_topo_sort(m, h, rd, d, d + dict::UPD_LIST, true, false)
}

/// [soft 0x08140dac SI_elemdictUpdListSetDirty]: the scene's list must be sorted again, and
/// so must the hierarchy information.
pub fn elemdict_upd_list_set_dirty(m: &mut Mem, d: Addr) {
    m.set_i32(d + dict::UPD_LIST + updlist::DIRTY, 1);
    m.set_i16(d + dict::HIER_DIRTY, 1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ed::build as edb;
    use crate::fcv;
    use crate::host::{Arg, Ret, Trace};

    /// Answers the relation dictionary's questions from a table of (id, pur, pre).
    fn host(rules: &'static [(i32, i32, i32)]) -> Trace {
        Trace::with(move |m, name, args| match name {
            "reldict_is_rel_active" => Ret::int(1),
            "reldict_get_rel_updlattr" => {
                let (Arg::I(id), Arg::I(a), Arg::I(b)) = (args[1], args[2], args[3]) else { panic!() };
                let (_, pur, pre) = rules.iter().copied().find(|r| r.0 == id as i32).unwrap_or((0, 0, 0));
                m.set_i32(a, pur);
                m.set_i32(b, pre);
                Ret::default()
            }
            _ => Ret::default(),
        })
    }

    fn nodes(m: &Mem, ul: Addr) -> Vec<(i32, i32, i32, i32, i32)> {
        (0..m.i32(ul + updlist::NB))
            .map(|i| {
                let p = m.ptr(ul + updlist::NODES) + i as u32 * updnode::SIZE;
                (m.i16(p) as i32, m.i32(p + 4), m.i16(p + 2) as i32, m.i32(p + updnode::RELID), m.i16(p + updnode::NB_ARGS) as i32)
            })
            .collect()
    }

    #[test]
    fn father_before_son_curves_before_matrices() {
        let mut m = Mem::new();
        let d = edb::scene(&mut m);
        let models = glob::chap(&m, cid::MODELS);
        // element 0 is the son, element 1 the father: index order must not decide
        let (son, father) = (m.si_calloc(1, 0x430), m.si_calloc(1, 0x430));
        edb::add_elem(&mut m, d, models, son);
        edb::add_elem(&mut m, d, models, father);
        let e0 = ed::get_elem(&m, d, models, 0);
        m.set_i32(e0 + elem::HIER_LEVEL, 1);
        // son: Parent (110, rank 1), scaling (100, rank 3) with two curves, translation (113, rank 6)
        edb::relate(&mut m, d, models, 0, 1, 110, 1, models, 1, 1, 0);
        let (sx, sy, tx) = (fcv::make(&mut m, 1, 1, &[(0.0, 1.0)]), fcv::make(&mut m, 2, 1, &[(0.0, 1.0)]),
                            fcv::make(&mut m, 139, 1, &[(0.0, 1.0)]));
        edb::attach_curve(&mut m, d, models, 0, 1, 113, 6, tx);
        edb::attach_curve(&mut m, d, models, 0, 1, 100, 3, sx);
        edb::attach_curve(&mut m, d, models, 0, 1, 100, 3, sy);
        // father: rotation (101, rank 4)
        let ry = fcv::make(&mut m, 5, 1, &[(0.0, 1.0)]);
        edb::attach_curve(&mut m, d, models, 1, 1, 101, 4, ry);
        let mut t = host(&[(110, 1, 0)]);
        let ul = d + dict::UPD_LIST;
        assert!(elemdict_topo_sort(&mut m, &mut t, glob::SI_RD, d, ul, false, false));
        // (chapter, element, list, relation, arguments)
        assert_eq!(nodes(&m, ul),
                   [(models, 1, 1, 101, 1), (models, 1, 1, -100, 0), (models, 0, 1, 100, 2), (models, 0, 1, 113, 1),
                    (models, 0, 1, -100, 0)]);
        assert_eq!(m.i32(ul + updlist::DIRTY), 0);
        // the arguments of the scaling node are the two curves, in list order
        let p = m.ptr(ul + updlist::NODES) + 2 * updnode::SIZE;
        let a = m.ptr(p + updnode::ARGS);
        assert_eq!((m.ptr(a + updelem::DATA), m.ptr(a + updelem::SIZE + updelem::DATA)), (sx, sy));
        assert_eq!(m.ptr(p + updnode::DATA), son);
        let names = t.names();
        assert_eq!((names[0], *names.last().unwrap()), ("db_check_extra_un_told_relation", "cus_resort_upd_list"));
        // clean: a second call does nothing
        t.calls.clear();
        assert!(elemdict_topo_sort(&mut m, &mut t, glob::SI_RD, d, ul, false, false));
        assert!(t.calls.is_empty());
        assert!(m.fault().is_none());
    }

    #[test]
    fn a_constraint_gets_a_transformation_step_first_and_cycles_are_found() {
        let mut m = Mem::new();
        let d = edb::scene(&mut m);
        let models = glob::chap(&m, cid::MODELS);
        let (a, b) = (m.si_calloc(1, 0x430), m.si_calloc(1, 0x430));
        edb::add_elem(&mut m, d, models, a);
        edb::add_elem(&mut m, d, models, b);
        // a's direction is constrained to b (116: pre set)
        edb::relate(&mut m, d, models, 0, 1, 116, 8, models, 1, 1, 0);
        let mut t = host(&[(116, 0, 1)]);
        let ul = d + dict::UPD_LIST;
        assert!(elemdict_topo_sort(&mut m, &mut t, glob::SI_RD, d, ul, false, false));
        assert_eq!(nodes(&m, ul), [(models, 0, 1, -100, 0), (models, 0, 1, 116, 1), (models, 0, 1, -100, 0)]);
        // and b to a: a cycle
        edb::relate(&mut m, d, models, 1, 1, 116, 8, models, 0, 1, 0);
        elemdict_upd_list_set_dirty(&mut m, d);
        assert!(!elemdict_topo_sort(&mut m, &mut t, glob::SI_RD, d, ul, false, false));
        assert_eq!(m.i32(ul + updlist::DIRTY), 1);
        assert!(elemdict_check_cycle(&mut m, &mut t, glob::SI_RD, d));
        assert!(m.fault().is_none());
    }

    #[test]
    fn one_element_and_the_end_mark() {
        let mut m = Mem::new();
        let d = edb::scene(&mut m);
        let cams = glob::chap(&m, cid::CAMERAS);
        let cam = m.si_calloc(1, 0x13c);
        edb::add_elem(&mut m, d, cams, cam);
        let fov = fcv::make(&mut m, 12, 1, &[(0.0, 45.0)]);
        let near = fcv::make(&mut m, 13, 1, &[(0.0, 1.0)]);
        edb::attach_curve(&mut m, d, cams, 0, 0, 1132, 9, near);
        edb::attach_curve(&mut m, d, cams, 0, 0, 1131, 9, fov);
        let mut t = host(&[]);
        assert!(elemdict_topo_sort_element(&mut m, &mut t, glob::SI_RD, d, cams, 0, -1, false));
        let ul = d + dict::TOPO_LIST;
        // equal ranks: the order the relations were added in
        assert_eq!(nodes(&m, ul), [(cams, 0, 0, 1132, 1), (cams, 0, 0, 1131, 1)]);
        m.set_u8(glob::SI_VIEWER_UPD_OPT, 4);
        elemdict_upd_list_set_dirty(&mut m, d);
        let sl = d + dict::UPD_LIST;
        assert!(elemdict_topo_sort(&mut m, &mut t, glob::SI_RD, d, sl, false, false));
        assert_eq!(m.i32(sl + updlist::NB), 2);
        assert_eq!(m.i16(m.ptr(sl + updlist::NODES) + 2 * updnode::SIZE), -1);
        assert!(m.fault().is_none());
    }
}
