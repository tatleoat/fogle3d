// SPDX-License-Identifier: MIT
//! `toposort.c`: the order in which a scene is brought up to date, on the image.
//!
//! The sort turns the relations of a dictionary into an update list
//! ([`crate::updlist`]): one node per run of relation entries, each element's inputs
//! before the element. It works in place on the relation lists: `SI_RelList` +0x10 is a
//! list's state (0 untouched, 1 being sorted, 2 done) and +8 its cursor (the first entry
//! not handled yet), both cleared by [`rel_topo_sort_init`]; a list takes part when its
//! word at +0x14 is set. A list met again while it is being sorted is a cycle.
//!
//! The same file holds the hierarchical order of a chapter
//! ([`sort_hier_elements`]), which uses the same state word.
//!
//! Not ported, with the project's standing reason: the term
//! `SI_lightLicense − SI_lightLicense2` that the original subtracts from a list's cursor
//! at the start of [`topo_sort`] (both words are 0 in the binary's data: the full
//! product's path).

use crate::elemdict::{get_elem, list_of};
use crate::elemrel;
use crate::globals::*;
use crate::outside::{self, Outside};
use crate::records::{UpdArg, UpdNode};
use crate::reldict;
use crate::updlist;
use si_mem::{field, record, Mem, Opaque, Ptr32};
use si_types::dict::{ARelation, Elem, ElemDict, ElemList, RelDict, RelList, Relations, UpdList};

record! {
    /// One chapter of the list `SI_elemdictTopoSortElemByFcv` is handed: a chapter and
    /// the elements of it to start from.
    pub struct ListChapUpdate = "SI_listChapUpdate", size 0x0c {
        0x00 chapid: i32, "the chapter";
        0x04 first: Ptr32<ListChapItem>, "its elements, a linked list";
        0x08 next: Ptr32<ListChapUpdate>, "the next chapter";
    }
}

record! {
    /// One element of that list.
    pub struct ListChapItem = "(item of SI_listChapUpdate)", size 0x08 {
        0x00 elemid: i32, "element index";
        0x04 next: Ptr32<ListChapItem>, "the next element";
    }
}

/// [soft 0x0813feb0 elemdictFindFather]: the index of an element's father: the element
/// the first entry of relation 110 in forward list 1 names, or −1. (An element without
/// relation lists is not tested for.)
pub fn find_father(m: &Mem, e: Ptr32<Elem>) -> i32 {
    let l = m[e].forward.groups.add(1);
    let n = m[l].nb;
    let rels = m[l].rels;
    let mut i = 0;
    while i < n {
        if m[rels.add(i)].relid == 0x6e {
            return m[rels.add(i)].elemid;
        }
        i += 1;
    }
    -1
}

/// [soft 0x0813ff44 relTopoSortInitChapter]: clears state and cursor of every list
/// (backward, or with `forward` forward) of every element in use of a chapter.
pub fn rel_topo_sort_init_chapter(m: &mut Mem, list: Ptr32<ElemList>, forward: i16) {
    let mut i = 0;
    while i < m[list].nb_elems {
        let e = m[list].elems.add(i);
        if m[e].status == 0 {
            let rels: Ptr32<Relations> = if forward != 0 { field!(e, Elem, forward) } else { field!(e, Elem, backward) };
            let mut g = 0;
            while g < m[rels].nb_groups {
                let l = m[rels].groups.add(g);
                m[l].unk_10 = 0;
                m[l].cur = 0;
                g += 1;
            }
        }
        i += 1;
    }
}

/// [soft 0x0813ffbc SI_relTopoSortInit]: the same for every chapter.
pub fn rel_topo_sort_init(m: &mut Mem, ed: Ptr32<ElemDict>, forward: i16) {
    let mut c = 0;
    while c < m[ed].nb_lists {
        let l = list_of(m, ed, c);
        rel_topo_sort_init_chapter(m, l, forward);
        c += 1;
    }
}

/// [soft 0x0813fff8 relCreateArguments]: the arguments of one update step: one for each
/// entry of the run that starts at entry `start` of a list (the entries that follow with
/// the same relation id and serial number). Each argument carries the object of the
/// element the entry names (read without a test for a missing element) and the curve
/// `SI_edGetFcvFromRelation (0x080ee6f0)` answers for the entry. `*count` is their number.
pub fn rel_create_arguments(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, l: Ptr32<RelList>, start: i32, count: Ptr32<i32>) -> Ptr32<UpdArg> {
    let mut i = start;
    let mut r = m[l].rels.add(i);
    m[count] = 0;
    let mut args: Ptr32<UpdArg> = Ptr32::NULL;
    loop {
        let e = get_elem(m, ed, m[r].chapid as i32, m[r].elemid);
        let fcv = outside::si_ed_get_fcv_from_relation(o, m, r);
        let data: Ptr32<Opaque> = Ptr32::new(m.u32(e.addr().wrapping_add(4)));
        args = updlist::add_argument(m, args, count, r, data, Ptr32::new(fcv));
        if m[l].nb - 1 <= i {
            return args;
        }
        let next = m.read(m[l].rels.add(i + 1));
        if next.relid != m[r].relid || next.serial != m[r].serial {
            return args;
        }
        i += 1;
        r = m[l].rels.add(i);
    }
}

/// The four relations whose entries are "pure dependencies": what lies below one is
/// sorted without a cycle being called (−102, −103, −104, −110).
fn is_pure_dependency(relid: i32) -> bool {
    matches!(relid, -0x66 | -0x67 | -0x68 | -0x6e)
}

/// [soft 0x081400cc topoSort]: sorts one relation list of one element, depth first.
///
/// `rels` is the element's lists in the direction of the sort (`forward`), `group` the
/// list, `level` the depth of the recursion (more than 100 fails with a message). The
/// list is marked "being sorted" and, in a backward sort, the element marked (+0x42).
/// For each entry from the cursor on whose relation is active in the dictionary
/// (`SI_reldictIsRelActive(…, 1)`) and whose other element is in use in a chapter of
/// kind 1:
///
/// - the list at the other end (same direction, the list the entry names) is looked at.
///   Being sorted, it counts as done when it is this very list, when the entry is
///   inactive, when its own current entry is of relation −102, or when the sort is below
///   a pure dependency (`pure`); otherwise it is a cycle and the sort fails. Untouched,
///   and the entry active, it is sorted first (below an entry of −102, −103, −104 or
///   −110, or into the CHANNEL_DRIVERS chapter, as a pure dependency).
/// - an active entry of relation 20000 that points back at its own list, in list 1 of a
///   model, first gets a transformation step (−100).
/// - at the last entry of a run (same relation id and serial number), unless
///   `check_only`, the relation's first update value is not 0, or the list is 3: a
///   transformation step first when the second update value is set; then, when any entry
///   of the run is active, one node for the run with its arguments
///   ([`rel_create_arguments`], from the cursor). The node carries the entry of the run
///   that has data (looking back from the last; an expression entry, 20010, with data
///   counts only when it names the EXPRESSIONS chapter), else the last entry.
/// - the cursor moves past the entry at the end of a run, and past an entry that was
///   passed over (but not past one whose other element is not in use or lies in a chapter
///   of another kind: that one is passed without the cursor moving).
///
/// In a forward sort restricted to marked elements (`restricted`), with the word at +0x2e
/// of `SI_glbManip` set, an entry of relation 110 instead climbs the hierarchy from the
/// other element to the first marked ancestor and sorts that one's list; an entry of
/// −110 in a forward sort is passed over.
///
/// After the entries, unless `check_only`: list 1 of a MODELS or WAVES element for which
/// a node was appended gets the closing step −100 (waves −101), list 2 the step −200.
/// The list is then "done" and the answer 1.
#[allow(clippy::too_many_arguments)]
pub fn topo_sort(
    m: &mut Mem,
    o: &mut dyn Outside,
    rd: Ptr32<RelDict>,
    ed: Ptr32<ElemDict>,
    upd: Ptr32<UpdList>,
    elem: Ptr32<Elem>,
    rels: Ptr32<Relations>,
    chap: i32,
    index: i32,
    group: i32,
    level: i32,
    check_only: i16,
    forward: i16,
    restricted: i16,
    pure: i16,
) -> i16 {
    let mut pure = pure;
    if level >= 0x65 {
        o.stdout(si_rt::message(0x0868_5160));
        return 0;
    }
    let l = m[rels].groups.add(group);
    m[l].unk_10 = 1;
    if forward == 0 {
        m[elem].selected = 1;
    }
    let mut appended = 0;
    let mut i = m[l].cur;
    while i < m[l].nb {
        let r = m[l].rels.add(i);
        let x = m.read(r);
        if reldict::is_rel_active(m, rd, x.relid, 1) as i16 == 0 {
            m[l].cur = i + 1;
            i += 1;
            continue;
        }
        let (mut oc, mut oi, mut og) = (x.chapid as i32, x.elemid, x.group as i32);
        let ol = list_of(m, ed, oc);
        // An entry whose other element is not in use, or lies in a chapter without files,
        // is passed without the cursor moving.
        let mut skip = false;
        if m[ol].kind == 1 && m[m[ol].elems.add(oi)].status == 0 {
            skip = true;
            let mut oe = m[ol].elems.add(oi);
            let mut orels: Ptr32<Relations> = if forward != 0 { field!(oe, Elem, forward) } else { field!(oe, Elem, backward) };
            if forward == 0 || x.relid != -0x6e {
                if m.i16(SI_GLB_MANIP.addr() + GLB_MANIP_2E) == 0 || forward == 0 || restricted == 0 || x.relid != 0x6e {
                    let olist = m[orels].groups.add(og);
                    let mut state = m[olist].unk_10;
                    if check_only == 0 && orels == rels && x.flag != 0 && x.relid == 20000 && chap == m[SI_CID].model && group == 1 && og == 1 {
                        updlist::add_node(m, upd, chap, index, elem, -100, Ptr32::NULL, 1, 0, Ptr32::NULL);
                    }
                    let same = orels == rels || m[r].flag == 0;
                    let mut harmless = same;
                    if state == 1 && !m[olist].rels.is_null() {
                        let current = m[olist].rels.add(m[olist].cur);
                        harmless = same || m[current].relid == -0x66;
                    }
                    if (harmless || pure != 0) && state == 1 {
                        state = 2;
                    }
                    if state == 0 && m[r].flag != 0 {
                        pure = (pure != 0 || is_pure_dependency(m[r].relid)) as i16;
                        if oc == m[SI_CID].chn_drv {
                            pure = 1;
                        }
                        if topo_sort(m, o, rd, ed, upd, oe, orels, oc, oi, og, level + 1, check_only, forward, restricted, pure) == 0 {
                            return 0;
                        }
                    } else if state == 1 {
                        return 0;
                    }
                    let relid = m[r].relid;
                    let serial = m[r].serial;
                    let last_of_run = m[l].nb - 1 <= i || {
                        let next = m.read(m[l].rels.add(i + 1));
                        relid != next.relid || serial != next.serial
                    };
                    if last_of_run {
                        if check_only == 0 {
                            // the relation is in the dictionary (it was found active above)
                            let [first, second] = reldict::rel_updlattr(m, rd, relid).unwrap_or([0, 0]);
                            if first == 0 && group != 3 {
                                if second != 0 {
                                    updlist::add_node(m, upd, chap, index, elem, -100, Ptr32::NULL, 1, 0, Ptr32::NULL);
                                }
                                let mut any_active: u16 = 0;
                                if -1 < i {
                                    let arr = m[l].rels;
                                    if m[arr.add(i)].relid == relid && m[arr.add(i)].serial == serial {
                                        let mut k = i;
                                        loop {
                                            any_active |= m[arr.add(k)].flag as u16;
                                            k -= 1;
                                            if k < 0 || m[arr.add(k)].relid != relid || m[arr.add(k)].serial != serial {
                                                break;
                                            }
                                        }
                                    }
                                }
                                let mut k = i;
                                if -1 < i {
                                    loop {
                                        let y = m.read(m[l].rels.add(k));
                                        if y.relid != relid || y.serial != serial || (!y.data.is_null() && (y.relid != 0x4e2a || y.chapid as i32 == m[SI_CID].exp)) {
                                            break;
                                        }
                                        k -= 1;
                                        if k < 0 {
                                            break;
                                        }
                                    }
                                }
                                if any_active != 0 {
                                    let mut carrier = r;
                                    if -1 < k && m[m[l].rels.add(k)].relid == relid {
                                        carrier = m[l].rels.add(k);
                                    }
                                    let count: Ptr32<i32> = Ptr32::new(m.local(4));
                                    let start = m[l].cur;
                                    let args = rel_create_arguments(m, o, ed, l, start, count);
                                    let n = m[count];
                                    m.release(4);
                                    updlist::add_node(m, upd, chap, index, elem, relid, carrier, group, n, args);
                                    appended += 1;
                                }
                            }
                        }
                        m[l].cur = i + 1;
                    }
                    skip = false;
                } else {
                    // up the hierarchy to the first marked ancestor
                    let mut entry_found = true;
                    let mut gone = false;
                    while m[oe].selected == 0 {
                        let model = m[SI_CID].model;
                        let up: Ptr32<ARelation> = elemrel::get_relation(m, ed, oe, 0x6e, model, -1);
                        if up.is_null() {
                            gone = true;
                            break;
                        }
                        oc = m[up].chapid as i32;
                        oi = m[up].elemid;
                        og = m[up].group as i32;
                        let ul = list_of(m, ed, oc);
                        if m[ul].kind != 1 {
                            entry_found = false;
                            break;
                        }
                        oe = m[ul].elems.add(oi);
                        if m[oe].status != 0 {
                            gone = true;
                            break;
                        }
                        orels = field!(oe, Elem, forward);
                    }
                    if !gone && entry_found && topo_sort(m, o, rd, ed, upd, oe, orels, oc, oi, og, level + 1, check_only, forward, restricted, pure) == 0 {
                        return 0;
                    }
                }
            }
        }
        if skip {
            m[l].cur = i + 1;
        }
        i += 1;
    }
    let cid = m.read(SI_CID);
    if check_only == 0 && 0 < m[l].nb && (chap == cid.model || chap == cid.wav) {
        let step = if chap == cid.wav { -0x65 } else { -100 };
        if group == 1 && 0 < appended {
            updlist::add_node(m, upd, chap, index, elem, step, Ptr32::NULL, 1, 0, Ptr32::NULL);
        } else if group == 2 && 0 < appended {
            updlist::add_node(m, upd, chap, index, elem, -200, Ptr32::NULL, 2, 0, Ptr32::NULL);
        }
    }
    let l = m[rels].groups.add(group);
    m[l].unk_10 = 2;
    1
}

/// Whether a list is one the sort starts in: it takes part (+0x14), and is untouched or
/// has entries left behind its cursor.
fn to_sort(m: &Mem, l: Ptr32<RelList>) -> bool {
    m[l].flag != 0 && (m[l].unk_10 == 0 || m[l].cur < m[l].nb)
}

/// [soft 0x0814070c elemdictTopoSortChapter]: sorts the forward lists 0 to 5 of every
/// element in use of a chapter: with `roots_only` only of hierarchy roots (depth 0), with
/// `restricted` only of marked elements (+0x42). 0 as soon as a sort fails.
#[allow(clippy::too_many_arguments)]
pub fn topo_sort_chapter(m: &mut Mem, o: &mut dyn Outside, rd: Ptr32<RelDict>, ed: Ptr32<ElemDict>, upd: Ptr32<UpdList>, list: Ptr32<ElemList>, chap: i32, check_only: i16, roots_only: i16, restricted: i16) -> i16 {
    let mut i = 0;
    while i < m[list].nb_elems {
        let e = m[list].elems.add(i);
        if m[e].status == 0 && (roots_only == 0 || m[e].hier_level == 0) && (restricted == 0 || m[e].selected != 0) {
            let mut g = 0;
            while g < m[e].forward.nb_groups {
                let l = m[e].forward.groups.add(g);
                if to_sort(m, l) && topo_sort(m, o, rd, ed, upd, e, field!(e, Elem, forward), chap, i, g, 0, check_only, 1, restricted, 0) == 0 {
                    return 0;
                }
                g += 1;
            }
        }
        i += 1;
    }
    1
}

/// The end mark of an update list under the fast hierarchy update (bit 2 of
/// `SI_viewerUpdOpt`): room for one more node, whose chapter word is −1. The count is
/// not changed, and nothing else of the extra node is written.
fn end_mark(m: &mut Mem, upd: Ptr32<UpdList>) {
    let n = m[upd].nb;
    let old = m[upd].nodes.addr();
    let p = m.si_realloc(old, ((n + 1) * 0x2c) as u32);
    m[upd].nodes = Ptr32::new(p);
    let extra: Ptr32<UpdNode> = Ptr32::<UpdNode>::new(p).add(n);
    m[extra].chapid = -1;
}

/// [soft 0x08140828 SI_elemdictTopoSort]: fills an update list with the whole scene in
/// evaluation order; nothing happens (answer 1) while the list is not dirty.
///
/// The relations a scene implies are added for the duration
/// (`SI_dbCheckExtraUnToldRelation (0x08104fb8)`, `(ed, 1, 1, 0)` before and
/// `(ed, 0, 1, 0)` after), unless `restricted`. Every forward list's state and cursor are
/// cleared; unless `check_only` the list is emptied. Chapters of kind 1 in dictionary
/// order: first every hierarchy root, then every element. A failure (a cycle, or more
/// than 100 levels) answers 0 with the nodes appended so far and the list still dirty.
/// Otherwise: the end mark under the fast hierarchy update (when the list has nodes and
/// not `check_only`), the dirty word cleared (not `check_only`), and
/// `SI_cusResortUpdList (0x0852dd6c)`.
pub fn topo_sort_dict(m: &mut Mem, o: &mut dyn Outside, rd: Ptr32<RelDict>, ed: Ptr32<ElemDict>, upd: Ptr32<UpdList>, check_only: i16, restricted: i16) -> i32 {
    if m[upd].dirty == 0 {
        return 1;
    }
    if restricted == 0 {
        outside::si_db_check_extra_untold_relation(o, m, ed, 1, 1, 0);
    }
    rel_topo_sort_init(m, ed, 1);
    if check_only == 0 {
        updlist::dispose(m, upd);
    }
    let mut c = 0;
    while c < m[ed].nb_lists {
        let l = list_of(m, ed, c);
        if m[l].kind == 1 && (topo_sort_chapter(m, o, rd, ed, upd, l, c, check_only, 1, restricted) == 0 || topo_sort_chapter(m, o, rd, ed, upd, l, c, check_only, 0, restricted) == 0) {
            if restricted == 0 {
                outside::si_db_check_extra_untold_relation(o, m, ed, 0, 1, 0);
            }
            return 0;
        }
        c += 1;
    }
    if m[SI_VIEWER_UPD_OPT] & 4 != 0 && 0 < m[upd].nb && check_only == 0 {
        end_mark(m, upd);
    }
    if restricted == 0 {
        outside::si_db_check_extra_untold_relation(o, m, ed, 0, 1, 0);
    }
    if check_only == 0 {
        m[upd].dirty = 0;
    }
    outside::si_cus_resort_upd_list(o, m, upd);
    1
}

/// [soft 0x081409bc SI_elemdictTopoSortElement]: fills the dictionary's second update
/// list (+0x4c) with what one element is made from: all its forward lists that take part
/// (`group` negative), or one list. On success the list's dirty word is cleared and
/// `SI_cusResortUpdList (0x0852dd6c)` called. An element none of whose lists takes part
/// answers a word of the original's stack that nobody set; here that is 0.
pub fn topo_sort_element(m: &mut Mem, o: &mut dyn Outside, rd: Ptr32<RelDict>, ed: Ptr32<ElemDict>, chap: i32, index: i32, group: i32, check_only: i16) -> i32 {
    let e = m[list_of(m, ed, chap)].elems.add(index);
    let upd: Ptr32<UpdList> = field!(ed, ElemDict, topo_list);
    updlist::dispose(m, upd);
    outside::si_db_check_extra_untold_relation(o, m, ed, 1, 1, 0);
    let rels: Ptr32<Relations> = field!(e, Elem, forward);
    rel_topo_sort_init(m, ed, 1);
    // Where the Rust differs: the original's result variable is not initialised.
    let mut result: i16 = 0;
    if group < 0 {
        let mut g = 0;
        while g < m[rels].nb_groups {
            let l = m[rels].groups.add(g);
            if to_sort(m, l) {
                result = topo_sort(m, o, rd, ed, upd, e, rels, chap, index, g, 0, check_only, 1, 0, 0);
                if result == 0 {
                    outside::si_db_check_extra_untold_relation(o, m, ed, 0, 1, 0);
                    return 0;
                }
            }
            g += 1;
        }
    } else {
        result = topo_sort(m, o, rd, ed, upd, e, rels, chap, index, group, 0, check_only, 1, 0, 0);
    }
    outside::si_db_check_extra_untold_relation(o, m, ed, 0, 1, 0);
    if result != 0 {
        m[ed].topo_list.dirty = 0;
        outside::si_cus_resort_upd_list(o, m, upd);
    }
    result as i32
}

/// The first half of the two restricted sorts: from one element, everything that depends
/// on it is marked by a backward sort that appends no node. A failure matters only with
/// `untold`.
fn mark_dependents(m: &mut Mem, o: &mut dyn Outside, rd: Ptr32<RelDict>, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, chap: i32, index: i32, group: i32, untold: i16) -> bool {
    let upd: Ptr32<UpdList> = field!(ed, ElemDict, topo_list);
    let rels: Ptr32<Relations> = field!(e, Elem, backward);
    if group < 0 {
        let mut g = 0;
        while g < m[rels].nb_groups {
            let l = m[rels].groups.add(g);
            if to_sort(m, l) && topo_sort(m, o, rd, ed, upd, e, rels, chap, index, g, 0, 1, 0, 0, 0) == 0 && untold != 0 {
                outside::si_db_check_extra_untold_relation(o, m, ed, 0, 0, 0);
                return false;
            }
            g += 1;
        }
    } else {
        topo_sort(m, o, rd, ed, upd, e, rels, chap, index, group, 0, 1, 0, 0, 0);
    }
    true
}

/// The second half: the end mark when the list has nodes, the scene sort restricted to
/// marked elements, the dirty word cleared.
fn sort_marked(m: &mut Mem, o: &mut dyn Outside, rd: Ptr32<RelDict>, ed: Ptr32<ElemDict>, untold: i16, check_only: i16) -> i32 {
    let upd: Ptr32<UpdList> = field!(ed, ElemDict, topo_list);
    if m[SI_VIEWER_UPD_OPT] & 4 != 0 && 0 < m[upd].nb {
        end_mark(m, upd);
    }
    let result = topo_sort_dict(m, o, rd, ed, upd, check_only, 1) as i16;
    if untold != 0 {
        outside::si_db_check_extra_untold_relation(o, m, ed, 0, 0, 0);
    }
    m[ed].topo_list.dirty = 0;
    result as i32
}

/// [soft 0x08140b28 SI_elemdictTopoSortSelectedElements]: fills the second update list
/// with everything that depends on the marked elements (+0x42): each marked element in
/// use marks its dependents by a backward sort (all its lists that take part, or list
/// `group`), then the scene sort runs restricted to marked elements. With `untold` the
/// implied relations are added for the duration (`(ed, 1, 0, 0)`, `(ed, 0, 0, 0)`).
pub fn topo_sort_selected_elements(m: &mut Mem, o: &mut dyn Outside, rd: Ptr32<RelDict>, ed: Ptr32<ElemDict>, group: i32, untold: i16, check_only: i16) -> i32 {
    let upd: Ptr32<UpdList> = field!(ed, ElemDict, topo_list);
    updlist::dispose(m, upd);
    if untold != 0 {
        outside::si_db_check_extra_untold_relation(o, m, ed, 1, 0, 0);
    }
    rel_topo_sort_init(m, ed, 0);
    let mut c = 0;
    while c < m[ed].nb_lists {
        let l = list_of(m, ed, c);
        let mut i = 0;
        while i < m[l].nb_elems {
            let e = m[l].elems.add(i);
            if m[e].status == 0 && m[e].selected != 0 && !mark_dependents(m, o, rd, ed, e, c, i, group, untold) {
                return 0;
            }
            i += 1;
        }
        c += 1;
    }
    sort_marked(m, o, rd, ed, untold, check_only)
}

/// [soft 0x08140d80 SI_elemdictCheckCycle]: whether the scene's relations hold a cycle:
/// the scene sort into the first update list without appending anything. (A list that is
/// not dirty is not sorted, and then there is no cycle.)
pub fn check_cycle(m: &mut Mem, o: &mut dyn Outside, rd: Ptr32<RelDict>, ed: Ptr32<ElemDict>) -> bool {
    topo_sort_dict(m, o, rd, ed, field!(ed, ElemDict, upd_list), 1, 0) as i16 == 0
}

/// [soft 0x08140dac SI_elemdictUpdListSetDirty]
pub fn upd_list_set_dirty(m: &mut Mem, ed: Ptr32<ElemDict>) {
    m[ed].upd_list.dirty = 1;
    m[ed].hier_dirty = 1;
}

/// [soft 0x08140dc4 SI_elemdictFindRoot]: the root that follows element `index` in a
/// chapter's hierarchical order (where a root comes after its elements): from the
/// position of `index`, the first position whose element has depth 0. Its index goes to
/// `*root`. Null without the order or when the search runs to the end. (The original
/// reads the order one word past its end before it tests for the end.)
pub fn find_root(m: &mut Mem, list: Ptr32<ElemList>, index: i32, root: Ptr32<i32>) -> Ptr32<Elem> {
    let order = m[list].hier_order;
    if order.is_null() {
        return Ptr32::NULL;
    }
    let n = m[list].nb_elems;
    let mut k = 0;
    if 0 < n {
        let mut v = m[order];
        while v != index {
            k += 1;
            if !(k < n) {
                break;
            }
            v = m[order.add(k)];
        }
    }
    let mut v;
    loop {
        v = m[order.add(k)];
        if !(0 < m[m[list].elems.add(v)].hier_level && k < n) {
            break;
        }
        k += 1;
    }
    if k != n {
        m[root] = v;
        return m[list].elems.add(v);
    }
    Ptr32::NULL
}

/// [soft 0x08140e54 elemdictSortHier]: puts an element behind its sons in a chapter's
/// hierarchical order and answers the size of its subtree. For an element in use, list
/// `group` of `rels` (its backward lists) is marked and every son (entry of relation
/// 110) in use whose list is untouched is put down first, recursively. Then the
/// element's index goes to `order[*pos]`, the number of elements below it to
/// `count[*pos]`, and `*pos` is counted up. `_level` is the depth, which nothing reads.
#[allow(clippy::too_many_arguments)]
pub fn sort_hier(m: &mut Mem, list: Ptr32<ElemList>, e: Ptr32<Elem>, rels: Ptr32<Relations>, index: i32, group: i32, _level: i32, order: Ptr32<i32>, count: Ptr32<i32>, pos: Ptr32<i32>) -> i32 {
    let mut size = 1;
    if m[e].status == 0 {
        let l = m[rels].groups.add(group);
        m[l].unk_10 = 1;
        let mut k = 0;
        while k < m[l].nb {
            let x = m.read(m[l].rels.add(k));
            if x.relid == 0x6e {
                let son = m[list].elems.add(x.elemid);
                if m[son].status == 0 && m[m[son].backward.groups.add(group)].unk_10 == 0 {
                    size += sort_hier(m, list, son, field!(son, Elem, backward), x.elemid, group, _level + 1, order, count, pos);
                }
            }
            k += 1;
        }
    }
    let at = m[pos];
    m[order.add(at)] = index;
    m[count.add(at)] = size - 1;
    m[pos] += 1;
    size
}

/// [soft 0x08140f30 SI_elemdictSortHierElements]: the hierarchical order of a chapter
/// from its relation 110 in list 1: two new arrays of one word per slot (`SI_calloc`),
/// first the identity and zeros; then, for a chapter with more than one slot, every slot
/// that is not in use and every element without a father starts a subtree
/// ([`sort_hier`]). The states and cursors of the chapter's backward lists are cleared
/// first. The old count array is freed only when there was an old order array.
pub fn sort_hier_elements(m: &mut Mem, ed: Ptr32<ElemDict>, chap: i32) {
    let l = list_of(m, ed, chap);
    if m[l].nb_elems == 0 {
        return;
    }
    rel_topo_sort_init_chapter(m, l, 0);
    if !m[l].hier_order.is_null() {
        let p = m[l].hier_order.addr();
        m.free(p);
        m[l].hier_order = Ptr32::NULL;
        if !m[l].hier_count.is_null() {
            let p = m[l].hier_count.addr();
            m.free(p);
            m[l].hier_count = Ptr32::NULL;
        }
    }
    let n = m[l].nb_elems as u32;
    m[l].hier_order = Ptr32::new(m.si_calloc(n, 4));
    m[l].hier_count = Ptr32::new(m.si_calloc(n, 4));
    let mut i = 0;
    while i < m[l].nb_elems {
        let slot = m[l].hier_order.add(i);
        m[slot] = i;
        let slot = m[l].hier_count.add(i);
        m[slot] = 0;
        i += 1;
    }
    if 1 < m[l].nb_elems {
        let pos: Ptr32<i32> = Ptr32::new(m.local(4));
        m[pos] = 0;
        let mut i = 0;
        while i < m[l].nb_elems {
            let e = m[l].elems.add(i);
            if m[e].status != 0 || find_father(m, e) == -1 {
                let (order, count) = (m[l].hier_order, m[l].hier_count);
                sort_hier(m, l, e, field!(e, Elem, backward), i, 1, 0, order, count, pos);
            }
            i += 1;
        }
        m.release(4);
    }
}

/// [soft 0x08141060 relGetRelationList]: appends to an update list one node per run of a
/// relation list, with the run's arguments.
pub fn rel_get_relation_list(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, l: Ptr32<RelList>, chap: i32, index: i32, group: i32, upd: Ptr32<UpdList>) {
    let elems = m[list_of(m, ed, chap)].elems;
    let count: Ptr32<i32> = Ptr32::new(m.local(4));
    let mut i = 0;
    while i < m[l].nb {
        let rels = m[l].rels;
        let args = rel_create_arguments(m, o, ed, l, i, count);
        let n = m[count];
        let relid = m[rels.add(i)].relid;
        updlist::add_node(m, upd, chap, index, elems.add(index), relid, Ptr32::NULL, group, n, args);
        i += n;
    }
    m.release(4);
}

/// [soft 0x081410e4 SI_elemdictGetAffectedByRelations]: empties an update list and fills
/// it from forward list `group` of an element: what the element is made from.
pub fn get_affected_by_relations(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, index: i32, group: i32, upd: Ptr32<UpdList>) {
    updlist::dispose(m, upd);
    let e = m[list_of(m, ed, chap)].elems.add(index);
    let l = m[e].forward.groups.add(group);
    rel_get_relation_list(m, o, ed, l, chap, index, group, upd);
}

/// [soft 0x08141140 SI_elemdictGetAffectsRelations]: the same from backward list `group`:
/// what the element takes part in.
pub fn get_affects_relations(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, index: i32, group: i32, upd: Ptr32<UpdList>) {
    updlist::dispose(m, upd);
    let e = m[list_of(m, ed, chap)].elems.add(index);
    let l = m[e].backward.groups.add(group);
    rel_get_relation_list(m, o, ed, l, chap, index, group, upd);
}

/// [soft 0x0814119c SI_elemdictTopoSortElemByFcv]: as
/// [`topo_sort_selected_elements`], starting from the elements of a list of chapters
/// (those in use) instead of from the marked ones.
pub fn topo_sort_elem_by_fcv(m: &mut Mem, o: &mut dyn Outside, rd: Ptr32<RelDict>, ed: Ptr32<ElemDict>, group: i32, untold: i16, check_only: i16, chapters: Ptr32<ListChapUpdate>) -> i32 {
    let upd: Ptr32<UpdList> = field!(ed, ElemDict, topo_list);
    updlist::dispose(m, upd);
    if untold != 0 {
        outside::si_db_check_extra_untold_relation(o, m, ed, 1, 0, 0);
    }
    rel_topo_sort_init(m, ed, 0);
    let mut chapter = chapters;
    while !chapter.is_null() {
        let chap = m[chapter].chapid;
        let lists = m[ed].lists;
        let mut item = m[chapter].first;
        while !item.is_null() {
            let index = m[item].elemid;
            let e = m[lists.add(chap)].elems.add(index);
            if m[e].status == 0 {
                let (c, i) = (m[chapter].chapid, m[item].elemid);
                if !mark_dependents(m, o, rd, ed, e, c, i, group, untold) {
                    return 0;
                }
            }
            item = m[item].next;
        }
        chapter = m[chapter].next;
    }
    sort_marked(m, o, rd, ed, untold, check_only)
}
