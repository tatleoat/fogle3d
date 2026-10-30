// SPDX-License-Identifier: MIT
//! `elemdict.c`, the relations of elements: adding and removing them at both ends, and the
//! look-ups that walk an element's lists.
//!
//! A relation is stored twice: an entry in a forward list of the source and one in a
//! backward list of the destination, each naming the other end (chapter, element index,
//! and the list there that holds the matching entry). Only the source's entry carries the
//! data.
//!
//! The look-ups keep a place per list: `SI_RelList` +8 is the position after the entry
//! the last look-up answered (0: none), and that entry carries the mark at +0x16 of
//! `SI_ARelation`. A "next" look-up clears the mark of the entry before the cursor and
//! goes on from the cursor; every look-up that finds nothing clears the cursor.
//!
//! Several of the "next" routines read the status of the element an entry names without
//! testing whether there is one: an entry that names a deleted or missing slot makes the
//! original read address 0x48. On the image that is a recorded fault.
//!
//! The product-tier branch (`SI_lightLicense`, which calls `checkNSEStatus`) is not
//! taken: the full product's path.

use crate::elemdict::{get_elem, glb_manip_36, list_of, mark_relations_changed};
use crate::globals::*;
use crate::outside::Outside;
use crate::records::CustomRelData;
use crate::relations::{rel_add_relation, rel_remove_a_relation, rel_remove_a_relation_id};
use crate::reldict;
use si_mem::{Mem, Opaque, Ptr32};
use si_types::dict::{ARelation, Elem, ElemDict, RelDict, RelList, Relations};

/// The forward lists of an element when `forward` is set, else the backward ones.
fn side(m: &Mem, e: Ptr32<Elem>, forward: i16) -> Relations {
    if forward != 0 {
        m[e].forward
    } else {
        m[e].backward
    }
}

/// The status of the element at `e`, read as the original reads it: without a test for
/// null.
fn status_unchecked(m: &Mem, e: Ptr32<Elem>) -> i16 {
    m.i16(e.addr().wrapping_add(0x48))
}

/// What the three "add at both ends" routines store when the entries are in: the lists
/// and the relation flag dirty, and the hierarchy flag when a model is involved or the
/// word at +0x36 of `SI_glbManip` is clear.
fn mark_added(m: &mut Mem, ed: Ptr32<ElemDict>, src_chap: i32, dst_chap: i32) {
    mark_relations_changed(m, ed);
    let model = m[SI_CID].model;
    if dst_chap == model || src_chap == model || glb_manip_36(m) == 0 {
        m[ed].hier_dirty = 1;
    }
}

/// [soft 0x08132cf0 SI_elemdictCusGetRelData]: the data of a relation given either of its
/// two entries. An entry without data is the destination's; the source's entry is looked
/// for in the *current* dictionary (`SI_elemDicts[*SI_procData]`): in the forward list
/// the entry names, the first entry of the same relation id and serial number.
pub fn cus_get_rel_data(m: &Mem, rel: Ptr32<ARelation>) -> Ptr32<Opaque> {
    if rel.is_null() {
        return Ptr32::NULL;
    }
    let r = m.read(rel);
    let mut data = r.data;
    if data.is_null() {
        let ed = m[SI_ELEM_DICTS].add(m[m[SI_PROC_DATA]]);
        let e = get_elem(m, ed, r.chapid as i32, r.elemid);
        if !e.is_null() && m[e].status == 0 && !m[e].forward.groups.is_null() {
            let l = m[e].forward.groups.add(r.group as i32);
            let n = m[l].nb;
            let rels = m[l].rels;
            let mut k = 0;
            while k < n {
                let q = m.read(rels.add(k));
                if r.relid == q.relid && r.serial == q.serial {
                    data = q.data;
                    break;
                }
                k += 1;
            }
        }
    }
    data
}

/// The elements at both ends when they exist and are in use.
fn both_ends(m: &Mem, ed: Ptr32<ElemDict>, src_chap: i32, dst_chap: i32, src: i32, dst: i32) -> Option<(Ptr32<Elem>, Ptr32<Elem>)> {
    if src_chap < m[ed].nb_lists && dst_chap < m[ed].nb_lists {
        let (sl, dl) = (list_of(m, ed, src_chap), list_of(m, ed, dst_chap));
        if -1 < src && src < m[sl].nb_elems && -1 < dst && dst < m[dl].nb_elems {
            let (s, d) = (m[sl].elems.add(src), m[dl].elems.add(dst));
            if m[s].status == 0 && m[d].status == 0 {
                return Some((s, d));
            }
        }
    }
    None
}

/// [soft 0x08132dc4 SI_elemdictAddCompleteRelation]: relates element `src` to element
/// `dst` under rule number `rule_no` of relation `relid`: an entry carrying `data` in the
/// source's forward list the rule names, and, when that one was accepted, its counterpart
/// in the destination's backward list. Answers 1 once both elements were found in use,
/// whether or not the entry was accepted (the lists are marked dirty all the same); 0
/// when the relation is unknown or an element is missing or not in use.
#[allow(clippy::too_many_arguments)]
pub fn add_complete_relation(m: &mut Mem, ed: Ptr32<ElemDict>, rd: Ptr32<RelDict>, src: i32, dst: i32, relid: i32, rule_no: i32, position: i32, data: Ptr32<Opaque>, flag: i16) -> i32 {
    let Some([src_chap, dst_chap, src_group, dst_group, rank, fanout]) = reldict::rel_attributes(m, rd, relid, rule_no) else {
        return 0;
    };
    let Some((s, d)) = both_ends(m, ed, src_chap, dst_chap, src, dst) else {
        return 0;
    };
    let l = m[s].forward.groups.add(src_group);
    if rel_add_relation(m, rd, l, relid, dst_chap, dst, dst_group, rank, fanout, position, -1, 1, data, flag) != 0 {
        let l = m[d].backward.groups.add(dst_group);
        rel_add_relation(m, rd, l, relid, src_chap, src, src_group, rank, fanout, position, -1, 0, Ptr32::NULL, flag);
    }
    mark_added(m, ed, src_chap, dst_chap);
    1
}

/// [soft 0x08132f78 SI_elemdictAddRelation]: the same without data, the entries active.
#[allow(clippy::too_many_arguments)]
pub fn add_relation(m: &mut Mem, ed: Ptr32<ElemDict>, rd: Ptr32<RelDict>, src: i32, dst: i32, relid: i32, rule_no: i32, position: i32) -> i32 {
    add_complete_relation(m, ed, rd, src, dst, relid, rule_no, position, Ptr32::NULL, 1) as i16 as i32
}

/// [soft 0x08132fb4 SI_elemdictAddUnCompleteRelation]: as
/// [`add_complete_relation`] for a rule that leaves a chapter open (−10): the caller's
/// chapter stands in for it.
#[allow(clippy::too_many_arguments)]
pub fn add_uncomplete_relation(m: &mut Mem, ed: Ptr32<ElemDict>, rd: Ptr32<RelDict>, src_chap: i32, dst_chap: i32, src: i32, dst: i32, relid: i32, rule_no: i32, position: i32, data: Ptr32<Opaque>, flag: i16) -> i32 {
    let Some([rule_src, rule_dst, src_group, dst_group, rank, fanout]) = reldict::rel_attributes(m, rd, relid, rule_no) else {
        return 0;
    };
    let src_chap = if rule_src == -10 { src_chap } else { rule_src };
    let dst_chap = if rule_dst == -10 { dst_chap } else { rule_dst };
    let Some((s, d)) = both_ends(m, ed, src_chap, dst_chap, src, dst) else {
        return 0;
    };
    let l = m[s].forward.groups.add(src_group);
    if rel_add_relation(m, rd, l, relid, dst_chap, dst, dst_group, rank, fanout, position, -1, 1, data, flag) != 0 {
        let l = m[d].backward.groups.add(dst_group);
        rel_add_relation(m, rd, l, relid, src_chap, src, src_group, rank, fanout, position, -1, 0, Ptr32::NULL, flag);
    }
    mark_added(m, ed, src_chap, dst_chap);
    1
}

/// [soft 0x08133184 SI_elemdictAddSerialRelation]: relates two elements with the
/// chapters and lists the caller names and a serial number. Only rank and fan-out come
/// from the dictionary: for relation 20000 (a custom effect) from the rule the data
/// names, else from rule number `rule_no`.
#[allow(clippy::too_many_arguments)]
pub fn add_serial_relation(
    m: &mut Mem,
    ed: Ptr32<ElemDict>,
    rd: Ptr32<RelDict>,
    src_chap: i32,
    dst_chap: i32,
    src_group: i32,
    dst_group: i32,
    src: i32,
    dst: i32,
    relid: i32,
    serial: i32,
    rule_no: i32,
    position: i32,
    data: Ptr32<Opaque>,
    flag: i16,
    is_source: i16,
) -> i32 {
    let attrs = if relid == 20000 {
        reldict::cus_rel_attributes(m, rd, data.cast::<CustomRelData>(), rule_no)
    } else {
        reldict::rel_attributes(m, rd, relid, rule_no).map(|a| [a[4], a[5]])
    };
    let Some([rank, fanout]) = attrs else {
        return 0;
    };
    let Some((s, d)) = both_ends(m, ed, src_chap, dst_chap, src, dst) else {
        return 0;
    };
    let l = m[s].forward.groups.add(src_group);
    if rel_add_relation(m, rd, l, relid, dst_chap, dst, dst_group, rank, fanout, position, serial, is_source, data, flag) != 0 {
        let l = m[d].backward.groups.add(dst_group);
        rel_add_relation(m, rd, l, relid, src_chap, src, src_group, rank, fanout, position, serial, 0, Ptr32::NULL, flag);
    }
    mark_added(m, ed, src_chap, dst_chap);
    1
}

/// [soft 0x0813335c SI_elemdictFindSerialRelation]: goes through the *runs* of a
/// relation in an element's forward lists, a run being the consecutive entries of one
/// serial number: answers the first entry of the next run whose element exists
/// (optionally only entries naming chapter `chap`), forwards or backwards through the
/// lists and entries. `first` starts at the first list (or the last); otherwise the
/// search goes on from list `*list_index` and that list's cursor. On a hit the cursor and
/// the mark are set and `*list_index` is the list; a list that is used up has its cursor
/// cleared.
#[allow(clippy::too_many_arguments)]
pub fn find_serial_relation(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, relid: i32, list_index: Ptr32<i32>, chap: i32, forward: i16, first: i16) -> Ptr32<ARelation> {
    if e.is_null() {
        return Ptr32::NULL;
    }
    let mut g = if first == 0 {
        m[list_index]
    } else if forward == 0 {
        m[e].forward.nb_groups - 1
    } else {
        0
    };
    while g < m[e].forward.nb_groups && -1 < g {
        let l = m[e].forward.groups.add(g);
        let before = m[l].cur - 1;
        let mut prev: Ptr32<ARelation> = Ptr32::NULL;
        if !(before < 0 || m[l].nb <= before) {
            prev = m[l].rels.add(before);
            m[prev].unk_16 = 0;
        }
        let mut i;
        if first != 0 {
            prev = Ptr32::NULL;
            i = if forward == 0 { m[l].nb - 1 } else { 0 };
        } else if forward == 0 {
            i = if m[l].cur == 0 { m[l].nb - 1 } else { m[l].cur - 2 };
        } else {
            i = m[l].cur;
        }
        if i < m[l].nb {
            loop {
                if i < 0 {
                    break;
                }
                let rels = m[l].rels;
                let mut r = rels.add(i);
                let x = m.read(r);
                if x.relid == relid && (prev.is_null() || x.serial != m[prev].serial) && (chap < 0 || x.chapid as i32 == chap) {
                    if forward == 0 {
                        // back to the first entry of this run
                        let mut k = i;
                        loop {
                            i = k;
                            k = i - 1;
                            if k < 0 {
                                break;
                            }
                            let q = m.read(rels.add(k));
                            if q.relid != relid || q.serial != x.serial {
                                break;
                            }
                            if !(chap < 0 || q.chapid as i32 == chap) {
                                break;
                            }
                        }
                        r = rels.add(i);
                    }
                    let other = get_elem(m, ed, m[r].chapid as i32, m[r].elemid);
                    if status_unchecked(m, other) != 2 {
                        m[l].cur = i + 1;
                        m[r].unk_16 = 1;
                        m[list_index] = g;
                        return r;
                    }
                }
                if forward == 0 {
                    i -= 1;
                } else {
                    i += 1;
                }
                if !(i < m[l].nb) {
                    break;
                }
            }
        }
        m[l].cur = 0;
        if forward == 0 {
            g -= 1;
        } else {
            g += 1;
        }
    }
    Ptr32::NULL
}

/// [soft 0x0813354c SI_elemdictSetRelToSerialid]: makes the first entry of relation
/// `relid` with serial number `serial` the current entry of its list (backward lists, or
/// with `forward` the forward ones) and answers it; lists gone through without a hit have
/// their cursor cleared.
pub fn set_rel_to_serialid(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, relid: i32, serial: i32, forward: i16) -> Ptr32<ARelation> {
    if e.is_null() {
        return Ptr32::NULL;
    }
    let mut g = 0;
    while g < side(m, e, forward).nb_groups {
        let l = side(m, e, forward).groups.add(g);
        clear_mark_before_cursor(m, l);
        let mut i = 0;
        while i < m[l].nb {
            let r = m[l].rels.add(i);
            let x = m.read(r);
            if x.relid == relid && x.serial == serial {
                let other = get_elem(m, ed, x.chapid as i32, x.elemid);
                if status_unchecked(m, other) != 2 {
                    m[l].cur = i + 1;
                    m[r].unk_16 = 1;
                    return r;
                }
            }
            i += 1;
        }
        m[l].cur = 0;
        g += 1;
    }
    Ptr32::NULL
}

/// [soft 0x0813362c SI_elemdictRemoveRelationPair]: removes the relation between `src`
/// and `dst` under rule `rule_no` of `relid` at both ends.
#[allow(clippy::too_many_arguments)]
pub fn remove_relation_pair(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, rd: Ptr32<RelDict>, src: i32, dst: i32, relid: i32, rule_no: i32, _unused: i32) -> i32 {
    let Some([src_chap, dst_chap, src_group, dst_group, _, _]) = reldict::rel_attributes(m, rd, relid, rule_no) else {
        return 0;
    };
    if src_chap < m[ed].nb_lists && dst_chap < m[ed].nb_lists {
        let (sl, dl) = (list_of(m, ed, src_chap), list_of(m, ed, dst_chap));
        if -1 < src && src < m[sl].nb_elems && -1 < dst && dst < m[dl].nb_elems {
            let s = m[sl].elems.add(src);
            let l = m[s].forward.groups.add(src_group);
            rel_remove_a_relation(m, o, ed, s, l, relid, src_chap, dst_chap, dst, -1);
            let d = m[dl].elems.add(dst);
            let l = m[d].backward.groups.add(dst_group);
            rel_remove_a_relation(m, o, ed, d, l, relid, dst_chap, src_chap, src, -1);
            mark_added(m, ed, src_chap, dst_chap);
            return 1;
        }
    }
    0
}

/// [soft 0x08133780 SI_elemdictDisconnectANDConnectRel]: moves the destination end of a
/// relation whose source is element `src`: the backward entry is taken from `from` and
/// one is added to `to` (through the global dictionary `SI_RD`, whatever `rd` is). The
/// source's own entry is not touched, and a relation the dictionary does not know is not
/// refused: the original then uses six words of its stack that nobody set.
pub fn disconnect_and_connect_rel(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, rd: Ptr32<RelDict>, src: i32, from: Ptr32<Elem>, to: Ptr32<Elem>, relid: i32) -> i32 {
    // Where the Rust differs: for an unknown relation the six values are 0 here.
    let [src_chap, dst_chap, src_group, dst_group, rank, fanout] = reldict::rel_attributes(m, rd, relid, 1).unwrap_or([0; 6]);
    let l = m[from].backward.groups.add(dst_group);
    rel_remove_a_relation(m, o, ed, from, l, relid, dst_chap, src_chap, src, -1);
    let l = m[to].backward.groups.add(dst_group);
    rel_add_relation(m, SI_RD, l, relid, src_chap, src, src_group, rank, fanout, 0, -1, 0, Ptr32::NULL, 1);
    1
}

/// [soft 0x0813382c SI_elemdictRemoveUncompleteRelPair]: as
/// [`remove_relation_pair`] for a rule with an open chapter, and only entries of serial
/// number `serial` (when it is not negative).
#[allow(clippy::too_many_arguments)]
pub fn remove_uncomplete_rel_pair(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, rd: Ptr32<RelDict>, src_chap: i32, dst_chap: i32, src: i32, dst: i32, relid: i32, rule_no: i32, _unused: i32, serial: i32) -> i32 {
    let Some([rule_src, rule_dst, src_group, dst_group, _, _]) = reldict::rel_attributes(m, rd, relid, rule_no) else {
        return 0;
    };
    let src_chap = if rule_src == -10 { src_chap } else { rule_src };
    let dst_chap = if rule_dst == -10 { dst_chap } else { rule_dst };
    if src_chap < m[ed].nb_lists && dst_chap < m[ed].nb_lists {
        let (sl, dl) = (list_of(m, ed, src_chap), list_of(m, ed, dst_chap));
        if -1 < src && src < m[sl].nb_elems && -1 < dst && dst < m[dl].nb_elems {
            let s = m[sl].elems.add(src);
            let l = m[s].forward.groups.add(src_group);
            rel_remove_a_relation(m, o, ed, s, l, relid, src_chap, dst_chap, dst, serial);
            let d = m[dl].elems.add(dst);
            let l = m[d].backward.groups.add(dst_group);
            rel_remove_a_relation(m, o, ed, d, l, relid, dst_chap, src_chap, src, serial);
            mark_added(m, ed, src_chap, dst_chap);
            return 1;
        }
    }
    0
}

/// [soft 0x08133998 SI_elemdictRemoveRelationId]: removes every relation `relid` of which
/// element `src` is the source (the first rule's chapter and list), at both ends.
pub fn remove_relation_id(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, rd: Ptr32<RelDict>, src: i32, relid: i32) -> i32 {
    let Some([src_chap, _, src_group, _, _, _]) = reldict::rel_attributes(m, rd, relid, 1) else {
        return 0;
    };
    if src_chap < m[ed].nb_lists {
        let sl = list_of(m, ed, src_chap);
        if -1 < src && src < m[sl].nb_elems {
            let s = m[sl].elems.add(src);
            let l = m[s].forward.groups.add(src_group);
            rel_remove_a_relation_id(m, o, ed, s, l, relid, src_chap, src, -1, 1, 0);
            mark_relations_changed(m, ed);
            m[ed].hier_dirty = 1;
            return 1;
        }
    }
    0
}

/// [soft 0x08133a6c SI_elemdictRemoveSerialRelId]: removes from the six forward lists of
/// an element every relation `relid` (of serial number `serial` when it is not negative),
/// at both ends.
pub fn remove_serial_rel_id(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, _rd: Ptr32<RelDict>, chap: i32, index: i32, relid: i32, serial: i32) -> i32 {
    let sl = list_of(m, ed, chap);
    if index < 0 || m[sl].nb_elems <= index {
        return 0;
    }
    for g in 0..6 {
        let s = m[sl].elems.add(index);
        let l = m[s].forward.groups.add(g);
        rel_remove_a_relation_id(m, o, ed, s, l, relid, chap, index, serial, 1, 0);
    }
    mark_relations_changed(m, ed);
    m[ed].hier_dirty = 1;
    1
}

/// [soft 0x08133b18 SI_elemdictRemoveBackwardRelId]: the same for the six backward lists.
pub fn remove_backward_rel_id(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, index: i32, relid: i32, serial: i32) -> i32 {
    let sl = list_of(m, ed, chap);
    if index < 0 || m[sl].nb_elems <= index {
        return 0;
    }
    for g in 0..6 {
        let s = m[sl].elems.add(index);
        let l = m[s].backward.groups.add(g);
        rel_remove_a_relation_id(m, o, ed, s, l, relid, chap, index, serial, 1, 1);
    }
    mark_relations_changed(m, ed);
    m[ed].hier_dirty = 1;
    1
}

/// [soft 0x08134db0 SI_elemdictGetCurRelation]: the current entry of list `group`: the
/// one before the cursor, or the first when the cursor is clear. The address is formed
/// whether or not the list has entries.
pub fn get_cur_relation(m: &Mem, _ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32) -> Ptr32<ARelation> {
    if e.is_null() {
        return Ptr32::NULL;
    }
    let l = side(m, e, forward).groups.add(group);
    let i = (m[l].cur - 1).max(0);
    m[l].rels.add(i)
}

/// [soft 0x08134dec SI_elemdictGetRelation]: the `nth` (from 0) entry in a row of
/// relation `relid` (optionally naming chapter `chap`) in the forward list the relation's
/// first rule gives in `SI_RD`. The count starts again after every entry that is not of
/// the relation and chapter.
pub fn get_relation(m: &mut Mem, _ed: Ptr32<ElemDict>, e: Ptr32<Elem>, relid: i32, chap: i32, nth: i32) -> Ptr32<ARelation> {
    if e.is_null() {
        return Ptr32::NULL;
    }
    let group: Ptr32<i32> = Ptr32::new(m.local(4));
    reldict::get_rel_chap_grp(m, SI_RD, relid, group);
    let g = m[group];
    m.release(4);
    let mut run = 0;
    if -1 < g && g < m[e].forward.nb_groups {
        let l = m[e].forward.groups.add(g);
        let n = m[l].nb;
        let rels = m[l].rels;
        let mut i = 0;
        while i < n {
            let x = m.read(rels.add(i));
            if x.relid == relid && (chap < 0 || x.chapid as i32 == chap) {
                run += 1;
            } else {
                run = 0;
            }
            if x.relid == relid && run == nth + 1 {
                return rels.add(i);
            }
            i += 1;
        }
    }
    Ptr32::NULL
}

/// [soft 0x08134e9c SI_elemdictGetRelationFromIds]: the forward entry of relation `relid`
/// that names element `index` of chapter `chap` (and, when `serial` is not negative, has
/// that serial number). For relation 20010 (expressions) every forward list is searched,
/// otherwise the one the relation's first rule gives in `SI_RD`.
pub fn get_relation_from_ids(m: &mut Mem, _ed: Ptr32<ElemDict>, e: Ptr32<Elem>, relid: i32, chap: i32, index: i32, serial: i32) -> Ptr32<ARelation> {
    if e.is_null() {
        return Ptr32::NULL;
    }
    let (mut g, end);
    if relid == 0x4e2a {
        end = m[e].forward.nb_groups;
        g = 0;
    } else {
        let group: Ptr32<i32> = Ptr32::new(m.local(4));
        reldict::get_rel_chap_grp(m, SI_RD, relid, group);
        g = m[group];
        m.release(4);
        if g < 0 || m[e].forward.nb_groups <= g {
            return Ptr32::NULL;
        }
        end = g + 1;
    }
    while g < end {
        let l = m[e].forward.groups.add(g);
        let n = m[l].nb;
        let rels = m[l].rels;
        let mut i = 0;
        while i < n {
            let x = m.read(rels.add(i));
            if x.relid == relid && x.chapid as i32 == chap && x.elemid == index && (serial < 0 || x.serial == serial) {
                return rels.add(i);
            }
            i += 1;
        }
        g += 1;
    }
    Ptr32::NULL
}

/// [soft 0x08134f78 SI_elemdictResetCurrFlag]: clears the cursor of every forward list
/// and the mark of every entry of relation `relid`.
pub fn reset_curr_flag(m: &mut Mem, e: Ptr32<Elem>, relid: i32) {
    if e.is_null() {
        return;
    }
    let mut g = 0;
    while g < m[e].forward.nb_groups {
        let l = m[e].forward.groups.add(g);
        m[l].cur = 0;
        let mut i = 0;
        while i < m[l].nb {
            let r = m[l].rels.add(i);
            if m[r].relid == relid {
                m[r].unk_16 = 0;
            }
            i += 1;
        }
        g += 1;
    }
}

/// [soft 0x08134fdc SI_elemdictSetCurrRelPointer]: clears every forward list's cursor,
/// then puts the cursor of the first list that holds a marked entry of relation `relid`
/// behind that entry; `*list_index` is that list, or −1.
pub fn set_curr_rel_pointer(m: &mut Mem, _ed: Ptr32<ElemDict>, e: Ptr32<Elem>, relid: i32, list_index: Ptr32<i32>) -> i32 {
    m[list_index] = -1;
    if e.is_null() {
        return 0;
    }
    let mut g = 0;
    while g < m[e].forward.nb_groups {
        let l = m[e].forward.groups.add(g);
        m[l].cur = 0;
        g += 1;
    }
    let n = m[e].forward.nb_groups;
    let groups = m[e].forward.groups;
    let mut g = 0;
    while g < n {
        let l = groups.add(g);
        let rels = m[l].rels;
        let mut i = 0;
        while i < m[l].nb {
            let x = m.read(rels.add(i));
            if x.relid == relid && x.unk_16 != 0 {
                m[l].cur = i + 1;
                m[list_index] = g;
                return 1;
            }
            i += 1;
        }
        g += 1;
    }
    0
}

/// The mark of the entry before a list's cursor is cleared: how every "next" look-up
/// begins.
fn clear_mark_before_cursor(m: &mut Mem, l: Ptr32<RelList>) {
    let before = m[l].cur - 1;
    if -1 < before && before < m[l].nb {
        let r = m[l].rels.add(before);
        m[r].unk_16 = 0;
    }
}

/// The walk every look-up for a related element makes: from entry `start` of list `l`,
/// the first entry `pick` accepts whose element can be had. On a hit the cursor is put
/// behind the entry and the entry marked; without one the cursor is cleared. With
/// `tested` the element is tested for null before its status is read; without, the
/// status is read at whatever `SI_elemdictGetElem` answered.
fn walk(m: &mut Mem, ed: Ptr32<ElemDict>, l: Ptr32<RelList>, start: i32, tested: bool, pick: impl Fn(&ARelation) -> bool) -> Option<(ARelation, Ptr32<Elem>)> {
    let mut i = start;
    while i < m[l].nb {
        let r = m[l].rels.add(i);
        let x = m.read(r);
        if pick(&x) {
            let other = get_elem(m, ed, x.chapid as i32, x.elemid);
            if (!tested || !other.is_null()) && status_unchecked(m, other) != 2 {
                m[l].cur = i + 1;
                m[r].unk_16 = 1;
                return Some((x, other));
            }
        }
        i += 1;
    }
    m[l].cur = 0;
    None
}

/// The list a "first" look-up starts in: none for a null element or one without lists.
fn first_list(m: &Mem, e: Ptr32<Elem>, forward: i16, group: i32) -> Option<Ptr32<RelList>> {
    if e.is_null() || side(m, e, forward).groups.is_null() {
        return None;
    }
    Some(side(m, e, forward).groups.add(group))
}

/// The list a "next" look-up goes on in, the mark before its cursor cleared.
fn next_list(m: &mut Mem, e: Ptr32<Elem>, forward: i16, group: i32) -> Option<(Ptr32<RelList>, i32)> {
    if e.is_null() {
        return None;
    }
    let l = side(m, e, forward).groups.add(group);
    clear_mark_before_cursor(m, l);
    Some((l, m[l].cur))
}

/// [soft 0x0813508c SI_elemdictFindRelatedElem]: the first element related to `e` by
/// relation `relid` in list `group` (forward or backward), optionally only in chapter
/// `chap`; its chapter and index go to `*out_chap` and `*out_index`.
#[allow(clippy::too_many_arguments)]
pub fn find_related_elem(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, relid: i32, chap: i32, out_chap: Ptr32<i32>, out_index: Ptr32<i32>) -> Ptr32<Elem> {
    let Some(l) = first_list(m, e, forward, group) else {
        return Ptr32::NULL;
    };
    match walk(m, ed, l, 0, true, |x| x.relid == relid && (x.chapid as i32 == chap || chap < 0)) {
        Some((x, other)) => {
            m[out_chap] = x.chapid as i32;
            m[out_index] = x.elemid;
            other
        }
        None => Ptr32::NULL,
    }
}

/// [soft 0x08135154 SI_elemdictFindNextRelatedElem]: the next one after the cursor.
#[allow(clippy::too_many_arguments)]
pub fn find_next_related_elem(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, relid: i32, chap: i32, out_chap: Ptr32<i32>, out_index: Ptr32<i32>) -> Ptr32<Elem> {
    let Some((l, start)) = next_list(m, e, forward, group) else {
        return Ptr32::NULL;
    };
    match walk(m, ed, l, start, false, |x| x.relid == relid && (x.chapid as i32 == chap || chap < 0)) {
        Some((x, other)) => {
            m[out_chap] = x.chapid as i32;
            m[out_index] = x.elemid;
            other
        }
        None => Ptr32::NULL,
    }
}

/// [soft 0x08135220 SI_elemdictFindRelatedElem2]: as [`find_related_elem`], and the entry
/// must name list `other_group` at the other end.
#[allow(clippy::too_many_arguments)]
pub fn find_related_elem2(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, other_group: i32, relid: i32, chap: i32, out_chap: Ptr32<i32>, out_index: Ptr32<i32>) -> Ptr32<Elem> {
    let Some(l) = first_list(m, e, forward, group) else {
        return Ptr32::NULL;
    };
    match walk(m, ed, l, 0, true, |x| x.relid == relid && x.group as i32 == other_group && (x.chapid as i32 == chap || chap < 0)) {
        Some((x, other)) => {
            m[out_chap] = x.chapid as i32;
            m[out_index] = x.elemid;
            other
        }
        None => Ptr32::NULL,
    }
}

/// [soft 0x081352ec SI_elemdictFindRangeNextRelatedElem2]: the next related element whose
/// relation id lies in `lo..=hi` and whose entry names list `other_group`; the relation
/// id and the index go out.
#[allow(clippy::too_many_arguments)]
pub fn find_range_next_related_elem2(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, other_group: i32, lo: i32, hi: i32, chap: i32, out_relid: Ptr32<i32>, out_index: Ptr32<i32>) -> Ptr32<Elem> {
    let Some((l, start)) = next_list(m, e, forward, group) else {
        return Ptr32::NULL;
    };
    match walk(m, ed, l, start, false, |x| lo <= x.relid && x.relid <= hi && x.group as i32 == other_group && (x.chapid as i32 == chap || chap < 0)) {
        Some((x, other)) => {
            m[out_relid] = x.relid;
            m[out_index] = x.elemid;
            other
        }
        None => Ptr32::NULL,
    }
}

/// [soft 0x081353c4 SI_elemdictFindRangeRelatedElem]: the first related element whose
/// relation id lies in `lo..=hi`.
#[allow(clippy::too_many_arguments)]
pub fn find_range_related_elem(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, lo: i32, hi: i32, chap: i32, out_relid: Ptr32<i32>, out_index: Ptr32<i32>) -> Ptr32<Elem> {
    let Some(l) = first_list(m, e, forward, group) else {
        return Ptr32::NULL;
    };
    match walk(m, ed, l, 0, false, |x| lo <= x.relid && x.relid <= hi && (x.chapid as i32 == chap || chap < 0)) {
        Some((x, other)) => {
            m[out_relid] = x.relid;
            m[out_index] = x.elemid;
            other
        }
        None => Ptr32::NULL,
    }
}

/// [soft 0x08135484 SI_elemdictFindRangeNextRelatedElem]: the next one.
#[allow(clippy::too_many_arguments)]
pub fn find_range_next_related_elem(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, lo: i32, hi: i32, chap: i32, out_relid: Ptr32<i32>, out_index: Ptr32<i32>) -> Ptr32<Elem> {
    let Some((l, start)) = next_list(m, e, forward, group) else {
        return Ptr32::NULL;
    };
    match walk(m, ed, l, start, false, |x| lo <= x.relid && x.relid <= hi && (x.chapid as i32 == chap || chap < 0)) {
        Some((x, other)) => {
            m[out_relid] = x.relid;
            m[out_index] = x.elemid;
            other
        }
        None => Ptr32::NULL,
    }
}

/// [soft 0x08135554 SI_elemdictFindRelatedElemSerial]: as [`find_related_elem`], and with
/// `serial` not negative only entries of that serial number.
#[allow(clippy::too_many_arguments)]
pub fn find_related_elem_serial(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, relid: i32, chap: i32, out_chap: Ptr32<i32>, out_index: Ptr32<i32>, serial: i32) -> Ptr32<Elem> {
    let Some(l) = first_list(m, e, forward, group) else {
        return Ptr32::NULL;
    };
    match walk(m, ed, l, 0, true, |x| x.relid == relid && (x.chapid as i32 == chap || chap < 0) && (serial < 0 || x.serial == serial)) {
        Some((x, other)) => {
            m[out_chap] = x.chapid as i32;
            m[out_index] = x.elemid;
            other
        }
        None => Ptr32::NULL,
    }
}

/// [soft 0x08135620 SI_elemdictFindNextRelatedElemSerial]: the next one.
#[allow(clippy::too_many_arguments)]
pub fn find_next_related_elem_serial(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, relid: i32, chap: i32, out_chap: Ptr32<i32>, out_index: Ptr32<i32>, serial: i32) -> Ptr32<Elem> {
    let Some((l, start)) = next_list(m, e, forward, group) else {
        return Ptr32::NULL;
    };
    match walk(m, ed, l, start, false, |x| x.relid == relid && (x.chapid as i32 == chap || chap < 0) && (serial < 0 || x.serial == serial)) {
        Some((x, other)) => {
            m[out_chap] = x.chapid as i32;
            m[out_index] = x.elemid;
            other
        }
        None => Ptr32::NULL,
    }
}

/// The count both counting routines make over one list.
fn count_entries(m: &Mem, e: Ptr32<Elem>, forward: i16, group: i32, pick: impl Fn(&ARelation) -> bool) -> i32 {
    if e.is_null() || side(m, e, forward).groups.is_null() {
        return 0;
    }
    let l = side(m, e, forward).groups.add(group);
    let n = m[l].nb;
    let rels = m[l].rels;
    let mut count = 0;
    let mut i = 0;
    while i < n {
        if pick(&m[rels.add(i)]) {
            count += 1;
        }
        i += 1;
    }
    count
}

/// [soft 0x08135700 SI_elemdictRelationsCount]: how many entries of list `group` are of
/// relation `relid`, name list `other_group` at the other end and (optionally) chapter
/// `chap`.
#[allow(clippy::too_many_arguments)]
pub fn relations_count(m: &Mem, _ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, other_group: i32, relid: i32, chap: i32) -> i32 {
    count_entries(m, e, forward, group, |x| x.relid == relid && x.group as i32 == other_group && (x.chapid as i32 == chap || chap < 0))
}

/// [soft 0x08135770 SI_elemdictRangeRelationsCount]: how many entries of list `group`
/// have a relation id in `lo..=hi` and (optionally) name chapter `chap`.
#[allow(clippy::too_many_arguments)]
pub fn range_relations_count(m: &Mem, _ed: Ptr32<ElemDict>, e: Ptr32<Elem>, forward: i16, group: i32, lo: i32, hi: i32, chap: i32) -> i32 {
    count_entries(m, e, forward, group, |x| lo <= x.relid && x.relid <= hi && (x.chapid as i32 == chap || chap < 0))
}

/// [soft 0x081357dc SI_elemdictFindSourceRelation]: the forward entry, at one of the
/// elements `e` depends on, that points back at `e` (optionally only entries naming
/// chapter `chap`): for each backward entry of `e`, the source's forward list is gone
/// through with the look-ups above (which moves that list's cursor and marks) until the
/// element found is `e`; the answer is then that list's current entry.
///
/// The original hands both look-ups the same two words of its stack for the chapter and
/// index they report; nothing reads them.
pub fn find_source_relation(m: &mut Mem, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, chap: i32) -> Ptr32<ARelation> {
    if e.is_null() || m[e].backward.groups.is_null() {
        return Ptr32::NULL;
    }
    let scratch: Ptr32<i32> = Ptr32::new(m.local(4));
    let mut found = Ptr32::NULL;
    let mut g = 0;
    'lists: while g < m[e].backward.nb_groups {
        let l = m[e].backward.groups.add(g);
        let mut k = 0;
        while k < m[l].nb {
            let x = m.read(m[l].rels.add(k));
            let source = m[list_of(m, ed, x.chapid as i32)].elems.add(x.elemid);
            let mut other = find_related_elem(m, ed, source, 1, x.group as i32, x.relid, chap, scratch, scratch);
            if !other.is_null() && other != e {
                loop {
                    other = find_next_related_elem(m, ed, source, 1, x.group as i32, x.relid, chap, scratch, scratch);
                    if other.is_null() || other == e {
                        break;
                    }
                }
            }
            if !other.is_null() && other == e {
                found = get_cur_relation(m, ed, source, 1, x.group as i32);
                break 'lists;
            }
            k += 1;
        }
        g += 1;
    }
    m.release(4);
    found
}

/// [soft 0x0813590c SI_elemdictSerialRelCount]: how many runs of relation `relid` an
/// element's forward lists hold (a run ends where the serial number changes; the
/// comparison carries over from one list to the next). With `which` and `position` the
/// number of the run that `which` starts goes to `*position`, 0 when it starts none.
pub fn serial_rel_count(m: &mut Mem, _ed: Ptr32<ElemDict>, e: Ptr32<Elem>, relid: i32, which: Ptr32<ARelation>, position: Ptr32<i32>) -> i32 {
    let mut count = 0;
    if e.is_null() || m[e].forward.groups.is_null() {
        return 0;
    }
    let mut last: Ptr32<ARelation> = Ptr32::NULL;
    if !position.is_null() {
        m[position] = 0;
    }
    let mut g = 0;
    while g < m[e].forward.nb_groups {
        let l = m[e].forward.groups.add(g);
        let mut i = 0;
        while i < m[l].nb {
            let r = m[l].rels.add(i);
            if m[r].relid == relid && (last.is_null() || m[r].serial != m[last].serial) {
                count += 1;
                last = r;
                if !which.is_null() && which == r && !position.is_null() {
                    m[position] = count;
                }
            }
            i += 1;
        }
        g += 1;
    }
    count
}
