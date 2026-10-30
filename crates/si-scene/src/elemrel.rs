// SPDX-License-Identifier: MIT
//! Relations between elements in their store form: adding them at both ends, following
//! them, and removing them with the element they belong to (`elemdict.c`, `relations.c`):
//! the nine that code not on the image still calls.
//!
//! The dictionary is in the memory image and these routines are `si-dict`'s there; the
//! scene loader calls those. What is left here works on the stores of [`crate::dict`],
//! which are a copy of the image (`crate::world::to_stores`): the two walks for this
//! crate's saver and for `si-rensetup`, the three that add a relation for `si-rensetup`,
//! and the four removal routines those reach (removing an entry that owns an animation
//! element deletes that element). Each goes when its last caller is on the image.
// adapter: not in place
//!
//! A relation is stored twice (finding 0010): an entry in a *forward* list of its source
//! and one in a *backward* list of its destination. The routines here keep the two in step.
//!
//! The original takes `__SI_Elem *`; here an element is an [`ElemId`]. Where the original
//! indexes a relation list or an element array without a bound check, the Rust refuses the
//! operation and says so.

use crate::dict::{Dict, ElemId, Program};
use si_types::dict::{ARelation, ELEM_DELETED, ELEM_VALID};
use si_types::store::{rel_add_relation, NewRelation, RelListStore};
use si_types::{Opaque, Ptr32};

/// Relation ids the code names.
pub const REL_PARENT: i32 = 0x6e; // 110
pub const REL_CAM_INT: i32 = 0x456; // 1110
pub const REL_LIG_INT: i32 = 0x83e; // 2110
pub const REL_CUSTOM: i32 = 20000;
pub const REL_EXP: i32 = 0x4e2a; // 20010

/// The two fields of a custom effect's relation data that the dictionary code reads
/// (`SI_CustomRelData` +0x20 and +0x24): the relation's name and the effect's name.
#[derive(Clone, Copy, Debug, Default)]
pub struct CusNames<'a> {
    pub rel_name: Option<&'a [u8]>,
    pub effect_name: Option<&'a [u8]>,
}

fn list_mut(d: &mut Dict, id: ElemId, forward: bool, group: i32) -> Option<&mut RelListStore> {
    let e = d.slot_mut(id)?;
    let lists = if forward { &mut e.forward } else { &mut e.backward };
    lists.get_mut(usize::try_from(group).ok()?)
}

fn list(d: &Dict, id: ElemId, forward: bool, group: i32) -> Option<&RelListStore> {
    let e = d.slot(id)?;
    let lists = if forward { &e.forward } else { &e.backward };
    lists.get(usize::try_from(group).ok()?)
}

/// What the three adding routines share once the rule is known: both elements must be
/// slots in use, the entry goes into the source's forward list and, if that accepted it,
/// into the destination's backward list; then the dictionary is marked changed.
#[allow(clippy::too_many_arguments)]
fn add_both_ends(
    d: &mut Dict, p: &mut Program, src: ElemId, dst: ElemId, src_group: i32, dst_group: i32, relid: i32, rank: i32,
    fanout: i32, limit: i32, serial: i32, check_fanout: bool, data: Ptr32<Opaque>, flag: i16,
) -> bool {
    let nb_lists = d.ed.rec.nb_lists;
    // the original tests the chapters against the number of lists from above only
    if src.chap >= nb_lists || dst.chap >= nb_lists || src.chap < 0 || dst.chap < 0 {
        return false;
    }
    let in_use = |d: &Dict, id: ElemId| d.slot(id).is_some_and(|e| e.rec.status == ELEM_VALID);
    if src.index < 0 || dst.index < 0 || !in_use(d, src) || !in_use(d, dst) {
        return false;
    }
    // The original indexes the six lists with the rule's group unchecked (-1 for a group
    // word the resource reader did not know). Here such a relation is not recorded, but
    // the dictionary is still marked changed, as it would be.
    let added = match list_mut(d, src, true, src_group) {
        Some(l) => rel_add_relation(
            &mut p.rd,
            l,
            &NewRelation { relid, chapid: dst.chap, elemid: dst.index, group: dst_group, rank, fanout, limit, serial, check_fanout, data, flag },
        ),
        None => false,
    };
    if added {
        if let Some(l) = list_mut(d, dst, false, dst_group) {
            rel_add_relation(
                &mut p.rd,
                l,
                &NewRelation {
                    relid,
                    chapid: src.chap,
                    elemid: src.index,
                    group: src_group,
                    rank,
                    fanout,
                    limit,
                    serial,
                    check_fanout: false,
                    data: Ptr32::NULL,
                    flag,
                },
            );
        }
    }
    // `SI_lightLicense` is a product-tier flag: the full product does not call
    // checkNSEStatus here, and neither does this.
    mark_changed(d);
    if dst.chap == p.cid.model || src.chap == p.cid.model || !p.backup_enabled {
        d.ed.rec.hier_dirty = 1;
    }
    true
}

/// Both update lists dirty, relations changed: the three words every relation routine sets.
fn mark_changed(d: &mut Dict) {
    d.ed.rec.upd_list.dirty = 1;
    d.ed.rec.rel_dirty = 1;
    d.ed.rec.topo_list.dirty = 1;
}

/// Relates element `src` (an index in the rule's source chapter) to element `dst` (an index
/// in the rule's destination chapter) by rule number `rule` of relation `relid`
/// [soft 0x08132dc4 SI_elemdictAddCompleteRelation]. `limit` is handed to the list
/// insertion as its count limit; `data` goes to the source's entry; `flag` to both.
///
/// Returns false, as the original returns 0, when the relation id is unknown, a chapter of
/// the rule is not in the dictionary or an element is not a slot in use. It returns true
/// even when the list refused the entry (a duplicate, or the fan-out reached).
#[allow(clippy::too_many_arguments)]
pub fn add_complete_relation(
    d: &mut Dict, p: &mut Program, src: i32, dst: i32, relid: i32, rule: i32, limit: i32, data: Ptr32<Opaque>, flag: i16,
) -> bool {
    let Some(a) = p.rd.get_rel_attributes(relid, rule) else { return false };
    add_both_ends(
        d,
        p,
        ElemId::new(a.src_chap, src),
        ElemId::new(a.dst_chap, dst),
        a.src_group,
        a.dst_group,
        relid,
        a.rank,
        a.fanout,
        limit,
        -1,
        true,
        data,
        flag,
    )
}

/// [soft 0x08132f78 SI_elemdictAddRelation]: the same with no data and flag 1.
pub fn add_relation(d: &mut Dict, p: &mut Program, src: i32, dst: i32, relid: i32, rule: i32, limit: i32) -> bool {
    add_complete_relation(d, p, src, dst, relid, rule, limit, Ptr32::NULL, 1)
}

/// The same for a relation whose rule leaves a chapter open (−10 in the rule): the caller
/// names the chapters, and each is used where the rule has −10
/// [soft 0x08132fb4 SI_elemdictAddUnCompleteRelation].
#[allow(clippy::too_many_arguments)]
pub fn add_uncomplete_relation(
    d: &mut Dict, p: &mut Program, src_chap: i32, dst_chap: i32, src: i32, dst: i32, relid: i32, rule: i32, limit: i32,
    data: Ptr32<Opaque>, flag: i16,
) -> bool {
    let Some(a) = p.rd.get_rel_attributes(relid, rule) else { return false };
    let sc = if a.src_chap == -10 { src_chap } else { a.src_chap };
    let dc = if a.dst_chap == -10 { dst_chap } else { a.dst_chap };
    add_both_ends(d, p, ElemId::new(sc, src), ElemId::new(dc, dst), a.src_group, a.dst_group, relid, a.rank, a.fanout, limit, -1, true, data, flag)
}

/// The first element related to `elem` by relation `relid` in list `group`, on the forward
/// side (`forward`) or the backward side, in chapter `chap` (any chapter when negative)
/// [soft 0x0813508c SI_elemdictFindRelatedElem].
///
/// The entry found is marked (its word at +0x16 becomes 1) and the list's cursor is left
/// just after it, for [`find_next_related_elem`]; when nothing is found the cursor is 0.
/// An entry whose other end is out of range or a freed slot is passed over.
pub fn find_related_elem(d: &mut Dict, elem: ElemId, forward: bool, group: i32, relid: i32, chap: i32) -> Option<ElemId> {
    // an element without relation lists (a null array in the original) finds nothing and
    // has no cursor to reset
    let n = list(d, elem, forward, group)?.rels.len();
    for i in 0..n {
        let r = list(d, elem, forward, group)?.rels[i];
        if r.relid == relid && (r.chapid as i32 == chap || chap < 0) {
            let other = ElemId::new(r.chapid as i32, r.elemid);
            if d.ed.get_elem(other.chap, other.index).is_some() {
                let l = list_mut(d, elem, forward, group)?;
                l.rec.cur = i as i32 + 1;
                l.rels[i].unk_16 = 1;
                return Some(other);
            }
        }
    }
    list_mut(d, elem, forward, group)?.rec.cur = 0;
    None
}

/// The next one after the list's cursor [soft 0x08135154 SI_elemdictFindNextRelatedElem].
/// The mark of the entry before the cursor is cleared first.
///
/// The original does not test what `SI_elemdictGetElem` returns here before reading the
/// status through it: an entry whose other end is gone makes it read through a null
/// pointer. Here such an entry is passed over.
pub fn find_next_related_elem(d: &mut Dict, elem: ElemId, forward: bool, group: i32, relid: i32, chap: i32) -> Option<ElemId> {
    let l = list_mut(d, elem, forward, group)?;
    let prev = l.rec.cur - 1;
    if prev >= 0 && (prev as usize) < l.rels.len() {
        l.rels[prev as usize].unk_16 = 0;
    }
    let start = l.rec.cur.max(0) as usize;
    let n = l.rels.len();
    for i in start..n {
        let r = list(d, elem, forward, group)?.rels[i];
        if r.relid == relid && (r.chapid as i32 == chap || chap < 0) {
            let other = ElemId::new(r.chapid as i32, r.elemid);
            if d.ed.get_elem(other.chap, other.index).is_some() {
                let l = list_mut(d, elem, forward, group)?;
                l.rec.cur = i as i32 + 1;
                l.rels[i].unk_16 = 1;
                return Some(other);
            }
        }
    }
    list_mut(d, elem, forward, group)?.rec.cur = 0;
    None
}

/// Every element related to `elem` that way, in list order: [`find_related_elem`] followed
/// by [`find_next_related_elem`] until it finds nothing (the loop the original writes out
/// at each use).
pub fn related_elems(d: &mut Dict, elem: ElemId, forward: bool, group: i32, relid: i32, chap: i32) -> Vec<ElemId> {
    let mut out = Vec::new();
    let mut next = find_related_elem(d, elem, forward, group, relid, chap);
    while let Some(e) = next {
        out.push(e);
        next = find_next_related_elem(d, elem, forward, group, relid, chap);
    }
    out
}

/// A list as `si_relRemoveARelation` leaves it once its last entry is gone.
fn reset_list(l: &mut RelListStore) {
    l.rels.clear();
    l.rec.nb = 0;
    l.rec.capacity = 0;
    l.rec.cur = 0;
    l.rec.unk_10 = 0;
    l.rec.flag = 1;
}

/// What goes with a relation entry when the entry is removed: its data is disposed of
/// (`SI_elemdictDisposeRelationData`, recorded in [`Dict::disposed`]) and the element of
/// the ANIMATION chapter it owns, if any, is deleted unless something else still uses it.
fn drop_entry_extras(d: &mut Dict, p: &mut Program, r: &ARelation, owner_index: i32) {
    if !r.data.is_null() {
        d.disposed.rel_data.push((r.relid, r.data));
    }
    if r.active_elem > -1 && d.ed.get_elem(p.cid.ani, r.active_elem).is_some() {
        set_delete_elem(d, p, ElemId::new(p.cid.ani, r.active_elem), owner_index, true, true);
    }
}

/// Removes from one list every entry of relation `relid` whose other end is in chapter
/// `chap` and, unless negative, is element `index` and has serial number `serial`
/// [soft 0x0813930c si_relRemoveARelation]. `list_of`, `forward`, `group` name the list.
///
/// The entries that remain close up. A list left empty is reset (no array, capacity 0,
/// cursor 0, flag 1); otherwise only the count changes: capacity and cursor stay.
/// (The original's last step concerns the product-tier flag and is not ported.)
#[allow(clippy::too_many_arguments)]
pub fn rel_remove_a_relation(
    d: &mut Dict, p: &mut Program, list_of: ElemId, forward: bool, group: i32, relid: i32, chap: i32, index: i32, serial: i32,
) -> bool {
    let mut i = 0usize;
    loop {
        let Some(l) = list(d, list_of, forward, group) else { return true };
        if i >= l.rels.len() {
            break;
        }
        let r = l.rels[i];
        if r.relid == relid && r.chapid as i32 == chap && (index < 0 || r.elemid == index) && (serial < 0 || r.serial == serial) {
            drop_entry_extras(d, p, &r, index);
            // the deletion above may itself have removed entries of this list
            if let Some(l) = list_mut(d, list_of, forward, group) {
                if i < l.rels.len() {
                    l.rels.remove(i);
                    l.rec.nb = l.rels.len() as i32;
                }
            }
        } else {
            i += 1;
        }
    }
    if let Some(l) = list_mut(d, list_of, forward, group) {
        if l.rels.is_empty() {
            reset_list(l);
        } else {
            l.rec.nb = l.rels.len() as i32;
        }
    }
    true
}

/// Removes entries from one list of `elem` together with their counterparts at the other
/// end [soft 0x081394b8 si_relRemoveARelationId].
///
/// Which entries: with `by_relid` those of relation `key` (and, when `serial` is not
/// negative, of that serial number); without it those whose other end has element index
/// `key`, whatever the relation. For each, the matching entries in the other element's
/// list are removed with [`rel_remove_a_relation`], looking for (`chap`, `index`): the
/// element this list belongs to. `forward` says which side `elem`'s list is on.
#[allow(clippy::too_many_arguments)]
pub fn rel_remove_a_relation_id(
    d: &mut Dict, p: &mut Program, elem: ElemId, forward: bool, group: i32, key: i32, chap: i32, index: i32, serial: i32,
    by_relid: bool,
) -> bool {
    let mut i = 0usize;
    loop {
        let Some(l) = list(d, elem, forward, group) else { return true };
        if i >= l.rels.len() {
            break;
        }
        let r = l.rels[i];
        let hit = if !by_relid {
            r.elemid == key
        } else if serial < 0 {
            r.relid == key
        } else {
            r.relid == key && r.serial == serial
        };
        if hit {
            let other = ElemId::new(r.chapid as i32, r.elemid);
            rel_remove_a_relation(d, p, other, !forward, r.group as i32, r.relid, chap, index, serial);
            drop_entry_extras(d, p, &r, index);
            if let Some(l) = list_mut(d, elem, forward, group) {
                if i < l.rels.len() {
                    l.rels.remove(i);
                    l.rec.nb = l.rels.len() as i32;
                }
            }
        } else {
            i += 1;
        }
    }
    if let Some(l) = list_mut(d, elem, forward, group) {
        if l.rels.is_empty() {
            reset_list(l);
        } else {
            l.rec.nb = l.rels.len() as i32;
        }
    }
    true
}

/// Removes every relation of one side of an element, with the counterparts at the other
/// ends [soft 0x081396d8 SI_relRemoveRelations]: the backward side with `backward`, else
/// the forward side. All lists of that side end up empty and reset.
pub fn rel_remove_relations(d: &mut Dict, p: &mut Program, elem: ElemId, backward: bool) -> bool {
    let forward = !backward;
    let nb_groups = d.slot(elem).map_or(0, |e| if forward { e.forward.len() } else { e.backward.len() });
    for g in 0..nb_groups as i32 {
        let mut i = 0usize;
        // the count is read again at each step, as the original does: removing an owned
        // animation element can shorten this very list
        while let Some(r) = list(d, elem, forward, g).and_then(|l| l.rels.get(i).copied()) {
            let other = ElemId::new(r.chapid as i32, r.elemid);
            rel_remove_a_relation(d, p, other, !forward, r.group as i32, r.relid, elem.chap, elem.index, -1);
            drop_entry_extras(d, p, &r, elem.index);
            i += 1;
        }
    }
    for g in 0..nb_groups as i32 {
        if let Some(l) = list_mut(d, elem, forward, g) {
            reset_list(l);
        }
    }
    true
}

/// Deletes an element [soft 0x08135ad4 SI_elemdictSetDeleteElem].
///
/// - With `if_unused_by_others` (the original's sixth argument; `from` its fifth): when
///   more than one backward relation points at the element, only those coming from the
///   element with index `from` are removed, and the element stays.
/// - Otherwise all its relations are removed, backward side first, and with `free_slot`
///   the slot is given up: status 2, the name record and the icon freed, `node`, `data`,
///   `aux_data`, `icon` cleared, depth 0, root index its own, new-flag 0. The slot keeps
///   its place: indices stored in relations elsewhere stay valid.
///
/// The dictionary is marked changed in both cases of the second branch; the early return
/// of the first branch marks nothing.
pub fn set_delete_elem(d: &mut Dict, p: &mut Program, elem: ElemId, from: i32, if_unused_by_others: bool, free_slot: bool) -> bool {
    if d.slot(elem).is_none() {
        return true; // the original has a pointer; a slot that does not exist cannot be passed
    }
    if if_unused_by_others {
        let (groups, total) = {
            let e = d.slot(elem).unwrap();
            (e.backward.len(), e.backward.iter().map(|l| l.rec.nb).sum::<i32>())
        };
        if total > 1 {
            for g in 0..groups as i32 {
                rel_remove_a_relation_id(d, p, elem, false, g, from, elem.chap, elem.index, -1, false);
            }
            return true;
        }
    }
    rel_remove_relations(d, p, elem, true);
    rel_remove_relations(d, p, elem, false);
    if free_slot {
        if let Some(l) = d.ed.lists.get_mut(elem.chap as usize) {
            l.sort_tbl_set_dirty();
        }
        let e = d.slot_mut(elem).unwrap();
        e.rec.status = ELEM_DELETED;
        let (node, icon) = (e.rec.node, e.rec.icon);
        e.rec.data = Ptr32::NULL;
        e.rec.aux_data = Ptr32::NULL;
        e.rec.node = Ptr32::NULL;
        e.rec.icon = Ptr32::NULL;
        e.rec.hier_level = 0;
        e.rec.root_id = elem.index;
        e.rec.new_flag = 0;
        d.nodes.free(node.0); // SI_dbListNodeFree
        if !icon.is_null() {
            d.disposed.icons.push(icon); // SI_dbIconFree
        }
    }
    mark_changed(d);
    d.ed.rec.hier_dirty = 1;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init;

    /// A dictionary with the 36 chapters and a made-up relation dictionary: 110 between
    /// models (source list 1, destination list 1), 300 from models to materials, 111 with
    /// open chapters, 15000 from models to animation.
    pub(crate) fn scene() -> (Dict, Program) {
        let (mut d, mut p) = init::empty_scene();
        let m = p.cid.model;
        p.rd.add_relation(b"Parent", 110, m, m, 1, 1, 10, 1, 0, 0, 0, None, 0, 3);
        p.rd.add_relation(b"Mat", 300, m, p.cid.mat, 3, 0, 20, 0x7fff, 0, 0, 0, None, 1, 3);
        p.rd.add_relation(b"CnsPos", 111, m, -10, 1, 1, 30, 1, 0, 0, 0, None, 0, 3);
        p.rd.add_relation(b"Anim", 15000, m, p.cid.ani, 0, 0, 5, 0x7fff, 0, 0, 0, None, 0, 3);
        for _ in 0..4 {
            init::add_named(&mut d, &p, m, b"s-m.1-0");
        }
        for _ in 0..2 {
            init::add_named(&mut d, &p, p.cid.mat, b"s-mat.1-0");
        }
        init::add_named(&mut d, &p, p.cid.ani, b"s-a.1-0");
        (d, p)
    }

    fn fwd(d: &Dict, chap: i32, i: i32, g: usize) -> Vec<(i32, i32, i32)> {
        d.ed.lists[chap as usize].elems[i as usize].forward[g].rels.iter().map(|r| (r.relid, r.chapid as i32, r.elemid)).collect()
    }
    fn back(d: &Dict, chap: i32, i: i32, g: usize) -> Vec<(i32, i32, i32)> {
        d.ed.lists[chap as usize].elems[i as usize].backward[g].rels.iter().map(|r| (r.relid, r.chapid as i32, r.elemid)).collect()
    }

    #[test]
    fn a_relation_is_stored_at_both_ends() {
        let (mut d, mut p) = scene();
        let m = p.cid.model;
        d.ed.rec.hier_dirty = 0;
        d.ed.rec.rel_dirty = 0;
        assert!(add_relation(&mut d, &mut p, 1, 0, 110, 1, 0));
        assert!(add_relation(&mut d, &mut p, 2, 0, 110, 1, 0));
        assert_eq!(fwd(&d, m, 1, 1), [(110, m, 0)]);
        assert_eq!(back(&d, m, 0, 1), [(110, m, 1), (110, m, 2)]);
        assert_eq!((d.ed.rec.hier_dirty, d.ed.rec.rel_dirty, d.ed.rec.upd_list.dirty, d.ed.rec.topo_list.dirty), (1, 1, 1, 1));
        // unknown relation, element out of range, deleted element
        assert!(!add_relation(&mut d, &mut p, 1, 0, 999, 1, 0));
        assert!(!add_relation(&mut d, &mut p, 9, 0, 110, 1, 0));
        assert!(!add_relation(&mut d, &mut p, -1, 0, 110, 1, 0));
        d.ed.lists[m as usize].elems[3].rec.status = ELEM_DELETED;
        assert!(!add_relation(&mut d, &mut p, 3, 0, 110, 1, 0));
        // a second father is refused by the list (fan-out 1) but the call still answers 1
        assert!(add_relation(&mut d, &mut p, 1, 2, 110, 1, 0));
        assert_eq!(fwd(&d, m, 1, 1), [(110, m, 0)]);
        assert_eq!(back(&d, m, 2, 1), []);
    }

    #[test]
    fn open_chapters_come_from_the_caller() {
        let (mut d, mut p) = scene();
        let (m, mat) = (p.cid.model, p.cid.mat);
        assert!(add_uncomplete_relation(&mut d, &mut p, 99, mat, 0, 1, 111, 1, 0, Ptr32::new(7), 1));
        assert_eq!(fwd(&d, m, 0, 1), [(111, mat, 1)]);
        assert_eq!(back(&d, mat, 1, 1), [(111, m, 0)]);
        assert_eq!(d.ed.lists[m as usize].elems[0].forward[1].rels[0].data, Ptr32::new(7));
        assert!(d.ed.lists[mat as usize].elems[1].backward[1].rels[0].data.is_null());
    }

    #[test]
    fn following_relations_moves_the_cursor_and_marks_the_entry() {
        let (mut d, mut p) = scene();
        let m = p.cid.model;
        for son in [1, 2, 3] {
            add_relation(&mut d, &mut p, son, 0, 110, 1, 0);
        }
        let father = ElemId::new(m, 0);
        assert_eq!(find_related_elem(&mut d, father, false, 1, 110, m), Some(ElemId::new(m, 1)));
        assert_eq!(d.ed.lists[m as usize].elems[0].backward[1].rec.cur, 1);
        assert_eq!(d.ed.lists[m as usize].elems[0].backward[1].rels[0].unk_16, 1);
        assert_eq!(find_next_related_elem(&mut d, father, false, 1, 110, m), Some(ElemId::new(m, 2)));
        assert_eq!(d.ed.lists[m as usize].elems[0].backward[1].rels[0].unk_16, 0);
        assert_eq!(find_next_related_elem(&mut d, father, false, 1, 110, -1), Some(ElemId::new(m, 3)));
        assert_eq!(find_next_related_elem(&mut d, father, false, 1, 110, m), None);
        assert_eq!(d.ed.lists[m as usize].elems[0].backward[1].rec.cur, 0);
        // another chapter wanted, another relation wanted
        assert_eq!(find_related_elem(&mut d, father, false, 1, 110, p.cid.mat), None);
        assert_eq!(find_related_elem(&mut d, father, false, 1, 300, m), None);
        assert_eq!(related_elems(&mut d, father, false, 1, 110, m).len(), 3);
        // a freed slot at the other end is passed over
        d.ed.lists[m as usize].elems[2].rec.status = ELEM_DELETED;
        assert_eq!(related_elems(&mut d, father, false, 1, 110, m), [ElemId::new(m, 1), ElemId::new(m, 3)]);
    }

    #[test]
    fn deleting_an_element_removes_its_relations_at_both_ends() {
        let (mut d, mut p) = scene();
        let (m, mat) = (p.cid.model, p.cid.mat);
        add_relation(&mut d, &mut p, 1, 0, 110, 1, 0);
        add_relation(&mut d, &mut p, 2, 0, 110, 1, 0);
        add_complete_relation(&mut d, &mut p, 0, 0, 300, 1, 0, Ptr32::new(9), 1);
        add_relation(&mut d, &mut p, 1, 0, 300, 1, 0);
        let node = d.ed.lists[m as usize].elems[0].rec.node;
        assert!(set_delete_elem(&mut d, &mut p, ElemId::new(m, 0), -1, false, true));
        let e = &d.ed.lists[m as usize].elems[0];
        assert_eq!((e.rec.status, e.rec.new_flag, e.rec.root_id), (ELEM_DELETED, 0, 0));
        assert!(e.rec.node.is_null() && d.nodes.get(node.0).is_none());
        assert!(e.forward.iter().chain(e.backward.iter()).all(|l| l.rels.is_empty() && l.rec.flag == 1));
        assert_eq!(fwd(&d, m, 1, 1), []);
        assert_eq!(fwd(&d, m, 2, 1), []);
        // the material keeps the relation of the other model only
        assert_eq!(back(&d, mat, 0, 0), [(300, m, 1)]);
        assert_eq!(d.disposed.rel_data, [(300, Ptr32::new(9))]);
        // the slot is reused by the next element
        assert_eq!(init::add_named(&mut d, &p, m, b"s-new.1-0"), 0);
    }

    #[test]
    fn an_element_used_by_others_is_only_detached() {
        let (mut d, mut p) = scene();
        let (m, ani) = (p.cid.model, p.cid.ani);
        add_relation(&mut d, &mut p, 0, 0, 15000, 1, 0);
        add_relation(&mut d, &mut p, 1, 0, 15000, 1, 0);
        // model 1 lets go of the curve: model 0 still uses it
        assert!(set_delete_elem(&mut d, &mut p, ElemId::new(ani, 0), 1, true, true));
        assert_eq!(d.ed.lists[ani as usize].elems[0].rec.status, ELEM_VALID);
        assert_eq!(back(&d, ani, 0, 0), [(15000, m, 0)]);
        assert_eq!(fwd(&d, m, 1, 0), []);
        // now the last user lets go: the curve is deleted
        assert!(set_delete_elem(&mut d, &mut p, ElemId::new(ani, 0), 0, true, true));
        assert_eq!(d.ed.lists[ani as usize].elems[0].rec.status, ELEM_DELETED);
        assert_eq!(fwd(&d, m, 0, 0), []);
    }

}
