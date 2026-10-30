// SPDX-License-Identifier: MIT
//! Relation queries of the element dictionary (`elemdict.c`) in their store form: the
//! fourteen that code not on the image still calls.
//!
//! The dictionary is in the memory image and these routines are `si-dict`'s there
//! (`si_dict::elemrel`, `si_dict::elemmisc`); the scene loader calls those. What is left
//! here works on the stores of [`crate::dict`], which are a copy of the image
//! (`crate::world::to_stores`), for two callers that are not in place: this crate's saver
//! (`get_cur_relation`, the two range walks, the two "son" walks, `find_serial_relation`)
//! and `si-rensetup` (the others). Each goes when its last caller is on the image.
// adapter: not in place

use crate::dict::{Dict, ElemId, Program};
use si_types::dict::ARelation;

use si_types::dict::{ELEM_DELETED, ELEM_VALID};
use si_types::store::RelListStore;

fn list_of(d: &Dict, id: ElemId, forward: bool, group: i32) -> Option<&RelListStore> {
    let e = d.slot(id)?;
    (if forward { &e.forward } else { &e.backward }).get(usize::try_from(group).ok()?)
}

fn list_of_mut(d: &mut Dict, id: ElemId, forward: bool, group: i32) -> Option<&mut RelListStore> {
    let e = d.slot_mut(id)?;
    (if forward { &mut e.forward } else { &mut e.backward }).get_mut(usize::try_from(group).ok()?)
}

/// The index of an element given the chapter it should be in
/// [soft 0x08132b94 SI_elemdictGetElemId]: the original subtracts pointers and answers -1
/// for a chapter the dictionary does not have and for an element outside that chapter's
/// array. Here the element is already (chapter, index): the answer is its index when the
/// chapter is the one given and the slot exists.
pub fn get_elem_id(d: &Dict, chap: i32, elem: ElemId) -> i32 {
    if chap >= d.ed.rec.nb_lists || elem.chap != chap || d.slot(elem).is_none() {
        return -1;
    }
    elem.index
}

/// The entry before a list's cursor (the one the last look-up returned), or the first when
/// the cursor is 0 [soft 0x08134db0 SI_elemdictGetCurRelation]. The original returns a
/// pointer without looking at the list's count: for an empty list it points at nothing.
pub fn get_cur_relation(d: &Dict, elem: ElemId, forward: bool, group: i32) -> Option<ARelation> {
    let l = list_of(d, elem, forward, group)?;
    l.rels.get((l.rec.cur - 1).max(0) as usize).copied()
}

/// What the "find related element" family shares: the first entry at or after `start` of
/// one list for which `fits` holds and whose other end is not a freed slot gets the cursor
/// and the mark; with none the cursor is cleared.
fn find_entry(d: &mut Dict, elem: ElemId, forward: bool, group: i32, start: usize, fits: &dyn Fn(&ARelation) -> bool) -> Option<ARelation> {
    let n = list_of(d, elem, forward, group)?.rels.len();
    for i in start..n {
        let r = list_of(d, elem, forward, group)?.rels[i];
        if fits(&r) && d.ed.get_elem(r.chapid as i32, r.elemid).is_some() {
            let l = list_of_mut(d, elem, forward, group)?;
            l.rec.cur = i as i32 + 1;
            l.rels[i].unk_16 = 1;
            return Some(r);
        }
    }
    list_of_mut(d, elem, forward, group)?.rec.cur = 0;
    None
}

/// Clears the mark of the entry before the cursor and returns where the search goes on.
fn step_cursor(d: &mut Dict, elem: ElemId, forward: bool, group: i32) -> Option<usize> {
    let l = list_of_mut(d, elem, forward, group)?;
    let prev = l.rec.cur - 1;
    if prev >= 0 && (prev as usize) < l.rels.len() {
        l.rels[prev as usize].unk_16 = 0;
    }
    Some(l.rec.cur.max(0) as usize)
}

/// The first related element whose relation id lies in `lo..=hi`
/// [soft 0x081353c4 SI_elemdictFindRangeRelatedElem]. The answer is the element and **the
/// relation id** (the original's two out-words are the id and the element's index, not the
/// chapter). An entry whose other end is gone makes the original read through a null
/// pointer; here it is passed over.
pub fn find_range_related_elem(d: &mut Dict, elem: ElemId, forward: bool, group: i32, lo: i32, hi: i32, chap: i32) -> Option<(ElemId, i32)> {
    find_entry(d, elem, forward, group, 0, &|r| lo <= r.relid && r.relid <= hi && (r.chapid as i32 == chap || chap < 0)).map(|r| (ElemId::new(r.chapid as i32, r.elemid), r.relid))
}

/// How many entries of one list have relation `relid`, their counterpart in list
/// `other_group`, and their other end in chapter `chap` (any when negative)
/// [soft 0x08135700 SI_elemdictRelationsCount].
pub fn relations_count(d: &Dict, elem: ElemId, forward: bool, group: i32, other_group: i32, relid: i32, chap: i32) -> i32 {
    list_of(d, elem, forward, group).map_or(0, |l| l.rels.iter().filter(|r| r.relid == relid && r.group as i32 == other_group && (r.chapid as i32 == chap || chap < 0)).count() as i32)
}

/// The status values of an element, for readers of this module: 0 in use, 1 flagged for
/// deletion (relations removed, slot kept), 2 freed.
pub const STATUS: [i16; 3] = [ELEM_VALID, 1, ELEM_DELETED];

use crate::elemrel;
use si_types::{Opaque, Ptr32};

fn mark_relations_changed(d: &mut Dict) {
    d.ed.rec.upd_list.dirty = 1;
    d.ed.rec.rel_dirty = 1;
    d.ed.rec.topo_list.dirty = 1;
}

/// The same for a relation whose rule leaves chapters open, and for one serial number
/// (any when negative) [soft 0x0813382c SI_elemdictRemoveUncompleteRelPair].
#[allow(clippy::too_many_arguments)]
pub fn remove_uncomplete_rel_pair(d: &mut Dict, p: &mut Program, src_chap: i32, dst_chap: i32, src: i32, dst: i32, relid: i32, rule: i32, serial: i32) -> bool {
    let Some(a) = p.rd.get_rel_attributes(relid, rule) else { return false };
    let sc = if a.src_chap == -10 { src_chap } else { a.src_chap };
    let dc = if a.dst_chap == -10 { dst_chap } else { a.dst_chap };
    remove_pair(d, p, ElemId::new(sc, src), ElemId::new(dc, dst), a.src_group, a.dst_group, relid, serial)
}

#[allow(clippy::too_many_arguments)]
fn remove_pair(d: &mut Dict, p: &mut Program, src: ElemId, dst: ElemId, src_group: i32, dst_group: i32, relid: i32, serial: i32) -> bool {
    let n = d.ed.rec.nb_lists;
    if src.chap >= n || dst.chap >= n || src.chap < 0 || dst.chap < 0 || src.index < 0 || dst.index < 0 || d.slot(src).is_none() || d.slot(dst).is_none() {
        return false;
    }
    elemrel::rel_remove_a_relation(d, p, src, true, src_group, relid, dst.chap, dst.index, serial);
    elemrel::rel_remove_a_relation(d, p, dst, false, dst_group, relid, src.chap, src.index, serial);
    mark_relations_changed(d);
    if dst.chap == p.cid.model || src.chap == p.cid.model || !p.backup_enabled {
        d.ed.rec.hier_dirty = 1;
    }
    true
}

/// The first son of a model element: the first element related by 110 in list 1 of the
/// backward side [soft 0x08137ff4 SI_elemdictFindSon].
pub fn find_son(d: &mut Dict, p: &Program, elem: ElemId) -> Option<ElemId> {
    elemrel::find_related_elem(d, elem, false, 1, elemrel::REL_PARENT, p.cid.model)
}

/// The next son [soft 0x08138030 SI_elemdictFindNextSon].
pub fn find_next_son(d: &mut Dict, p: &Program, elem: ElemId) -> Option<ElemId> {
    elemrel::find_next_related_elem(d, elem, false, 1, elemrel::REL_PARENT, p.cid.model)
}

/// What the three "get related element" routines answer: the element found, its object (the
/// original's result: null when there is none), and the list's cursor afterwards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Related {
    pub elem: Option<ElemId>,
    pub data: Ptr32<Opaque>,
    pub cursor: i32,
}

fn related(d: &Dict, elem: ElemId, forward: bool, group: i32, found: Option<ElemId>) -> Related {
    let data = found.and_then(|f| d.slot(f)).map_or(Ptr32::NULL, |e| e.rec.data);
    let cursor = d.slot(elem).map_or(-1, |e| e.get_cur_relation_index(forward, group));
    Related { elem: found, data, cursor }
}

/// The object of the first related element, with the cursor to go on from
/// [soft 0x08138094 SI_elemdictGetFirstRelatedElem].
pub fn get_first_related_elem(d: &mut Dict, elem: ElemId, forward: bool, group: i32, relid: i32, chap: i32) -> Related {
    let found = elemrel::find_related_elem(d, elem, forward, group, relid, chap);
    related(d, elem, forward, group, found)
}

/// The object of the next one; `cursor`, when given, is first put back into the list (so
/// that several walks of one list can be interleaved)
/// [soft 0x08138104 SI_elemdictGetNextRelatedElem].
pub fn get_next_related_elem(d: &mut Dict, elem: ElemId, forward: bool, group: i32, relid: i32, chap: i32, cursor: Option<i32>) -> Related {
    if let (Some(c), Some(e)) = (cursor, d.slot_mut(elem)) {
        e.set_cur_relation_index(forward, group, c);
    }
    let found = elemrel::find_next_related_elem(d, elem, forward, group, relid, chap);
    related(d, elem, forward, group, found)
}

/// The object of the `n`-th related element, counted from 0
/// [soft 0x08138190 SI_elemdictGetNthRelatedElem]: the first, then `n` times the next.
pub fn get_nth_related_elem(d: &mut Dict, elem: ElemId, n: i32, forward: bool, group: i32, relid: i32, chap: i32) -> Related {
    let mut found = elemrel::find_related_elem(d, elem, forward, group, relid, chap);
    let mut k = 1;
    while k <= n {
        found = elemrel::find_next_related_elem(d, elem, forward, group, relid, chap);
        if found.is_none() {
            break;
        }
        k += 1;
    }
    related(d, elem, forward, group, found)
}

/// The next related element whose relation id lies in `lo..=hi`, after the cursor
/// [soft 0x08135484 SI_elemdictFindRangeNextRelatedElem]; the answer as in
/// [`find_range_related_elem`].
pub fn find_range_next_related_elem(d: &mut Dict, elem: ElemId, forward: bool, group: i32, lo: i32, hi: i32, chap: i32) -> Option<(ElemId, i32)> {
    let start = step_cursor(d, elem, forward, group)?;
    find_entry(d, elem, forward, group, start, &|r| lo <= r.relid && r.relid <= hi && (r.chapid as i32 == chap || chap < 0)).map(|r| (ElemId::new(r.chapid as i32, r.elemid), r.relid))
}

/// Whether an element has, on one side, any relation `relid` to chapter `chap` (any chapter
/// when negative) [soft 0x08137f0c SI_elemdictRelationCheck].
pub fn relation_check(d: &Dict, elem: ElemId, forward: bool, relid: i32, chap: i32) -> bool {
    d.slot(elem).is_some_and(|e| {
        (if forward { &e.forward } else { &e.backward }).iter().flat_map(|l| l.rels.iter()).any(|r| r.relid == relid && (r.chapid as i32 == chap || chap < 0))
    })
}

/// The scene's element: slot 0 of the SCENES chapter, unless it is freed
/// [soft 0x08138068 SI_elemdictGetElemFromScene].
pub fn get_elem_from_scene(d: &Dict, p: &Program) -> Option<ElemId> {
    d.ed.get_elem(p.cid.dsc, 0).map(|_| ElemId::new(p.cid.dsc, 0))
}

/// Steps through the *relations* of one id on an element's forward side, where a relation
/// is a run of entries with one serial number [soft 0x0813335c SI_elemdictFindSerialRelation].
/// Returns the list and position of the first entry of the relation found, sets that list's
/// cursor just after it and marks it; `None` when there is no further one.
///
/// - `restart`: begin at the first list (`ahead`) or the last one, at its first or last
///   entry; otherwise continue in list `group` from its cursor: after the cursor going
///   ahead, two before it going back (from the list's end when the cursor is 0).
/// - Going on from a cursor, an entry is taken only if its serial number differs from that
///   of the entry before the cursor: the rest of the relation just visited is passed over.
/// - Going back, the answer is moved to the first entry of the run it found.
/// - `chap`: when not negative, only entries towards that chapter.
///
/// An entry whose other end is a freed slot is not taken (the original reads the status
/// through the result of `SI_elemdictGetElem` without testing it for null).
pub fn find_serial_relation(d: &mut Dict, elem: ElemId, relid: i32, group: i32, chap: i32, ahead: bool, restart: bool) -> Option<(i32, usize)> {
    let nb_groups = d.slot(elem)?.forward.len() as i32;
    let mut g = if !restart {
        group
    } else if ahead {
        0
    } else {
        nb_groups - 1
    };
    while g < nb_groups && g > -1 {
        let (cur, nb) = {
            let l = &d.slot(elem)?.forward[g as usize];
            (l.rec.cur, l.rels.len() as i32)
        };
        let before = cur - 1;
        let mut prev_serial: Option<i32> = None;
        if before >= 0 && before < nb {
            let l = &mut d.slot_mut(elem)?.forward[g as usize];
            l.rels[before as usize].unk_16 = 0;
            if !restart {
                prev_serial = Some(l.rels[before as usize].serial);
            }
        }
        let mut i = if restart {
            if ahead { 0 } else { nb - 1 }
        } else if ahead {
            cur
        } else if cur == 0 {
            nb - 1
        } else {
            cur - 2
        };
        if i < nb {
            loop {
                if i < 0 {
                    break;
                }
                let rels = d.slot(elem)?.forward[g as usize].rels.clone();
                let r = rels[i as usize];
                let fits = |x: &ARelation| x.relid == relid && (chap < 0 || x.chapid as i32 == chap);
                if fits(&r) && prev_serial != Some(r.serial) {
                    let mut at = i;
                    if !ahead {
                        // back to the first entry of this relation
                        while at > 0 {
                            let x = &rels[at as usize - 1];
                            if x.relid != relid || x.serial != r.serial || !(chap < 0 || x.chapid as i32 == chap) {
                                break;
                            }
                            at -= 1;
                        }
                    }
                    let hit = rels[at as usize];
                    if d.ed.get_elem(hit.chapid as i32, hit.elemid).is_some() {
                        let l = &mut d.slot_mut(elem)?.forward[g as usize];
                        l.rec.cur = at + 1;
                        l.rels[at as usize].unk_16 = 1;
                        return Some((g, at as usize));
                    }
                    i = at;
                }
                i += if ahead { 1 } else { -1 };
                if i >= nb {
                    break;
                }
            }
        }
        d.slot_mut(elem)?.forward[g as usize].rec.cur = 0;
        g += if ahead { 1 } else { -1 };
    }
    None
}

