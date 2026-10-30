// SPDX-License-Identifier: MIT
//! `relations.c`: the relation lists of one element, on the image.
//!
//! An element has two `SI_Relations` (forward at +0x60, backward at +0x68), each an array
//! of six `SI_RelList` (24 bytes) in one block of 0x90 bytes; a list's entries
//! (`SI_ARelation`, 32 bytes) are one block that grows by one entry: `SI_malloc(0x20)`
//! for the first, `SI_realloc(rels, (n + 1) * 0x20)` when the count has reached the
//! capacity. A list that loses its last entry frees the block.

use crate::elemdict;
use crate::globals::{REL_SERIAL_NUMBER, SI_CID};
use crate::outside::{self, Outside};
use crate::reldict;
use si_mem::{Mem, Opaque, Ptr32};
use si_types::dict::{ARelation, Elem, ElemDict, RelDict, RelList, Relations};

/// The six ids for which a relation with a single rule may join the same pair twice.
const REPEATABLE: [i32; 6] = [20000, 10000, 20010, 246, 247, 551];

/// [soft 0x081392a0 si_relAllocate]: six empty lists in a block of 0x90 bytes. The two
/// bytes at +0x16 of each list are not written.
pub fn rel_allocate(m: &mut Mem, rels: Ptr32<Relations>) {
    m[rels].nb_groups = 6;
    m[rels].groups = Ptr32::new(m.si_malloc(0x90));
    let mut i = 0;
    while i < m[rels].nb_groups {
        let l = m[rels].groups.add(i);
        reset_list(m, l);
        i += 1;
    }
}

/// The six stores every routine makes to empty a list.
pub(crate) fn reset_list(m: &mut Mem, l: Ptr32<RelList>) {
    m[l].nb = 0;
    m[l].capacity = 0;
    m[l].cur = 0;
    m[l].rels = Ptr32::NULL;
    m[l].unk_10 = 0;
    m[l].flag = 1;
}

/// What the two removal routines do with an entry they drop: its data is disposed of
/// (`SI_elemdictDisposeRelationData (0x08111b7c)`), and the animation element it owns
/// (+8, the curve of its active range) is deleted with its slot given up.
fn drop_entry(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, r: Ptr32<ARelation>, arg5: i32) {
    if !m[r].data.is_null() {
        outside::si_elemdict_dispose_relation_data(o, m, r);
    }
    let owned = m[r].active_elem;
    if -1 < owned {
        let ani = m[SI_CID].ani;
        let e = elemdict::get_elem(m, ed, ani, owned);
        if !e.is_null() {
            elemdict::set_delete_elem(m, o, ed, ani, e, owned, arg5, 1, 1);
        }
    }
}

/// What both removal routines do when the loop is over: a list with nothing left frees
/// its block and is reset; otherwise the count is stored.
fn close_list(m: &mut Mem, l: Ptr32<RelList>, n: i32) {
    if n < 1 {
        if !m[l].rels.is_null() {
            let p = m[l].rels.addr();
            m.free(p);
            m[l].rels = Ptr32::NULL;
        }
        m[l].capacity = 0;
        m[l].nb = 0;
        m[l].cur = 0;
        m[l].unk_10 = 0;
        m[l].flag = 1;
    } else {
        m[l].nb = n;
    }
}

/// [soft 0x0813930c si_relRemoveARelation]: drops from a list every entry of relation
/// `relid` that names chapter `chap` and, when they are not negative, element `elemid`
/// and serial number `serial`. The count is stored only at the end, so while an entry's
/// animation element is being deleted the list still shows its old count. `_elem` and
/// `_own_chap` serve only the light product's "editable" mark (`SI_lightLicense`), which
/// is not ported: the full product's path.
#[allow(clippy::too_many_arguments)]
pub fn rel_remove_a_relation(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, _elem: Ptr32<Elem>, l: Ptr32<RelList>, relid: i32, _own_chap: i32, chap: i32, elemid: i32, serial: i32) -> i32 {
    let mut i = 0;
    let mut n = m[l].nb;
    while i < n {
        let r = m[l].rels.add(i);
        let e = m.read(r);
        if e.relid == relid && e.chapid as i32 == chap && (elemid < 0 || e.elemid == elemid) && (serial < 0 || e.serial == serial) {
            drop_entry(m, o, ed, r, elemid);
            n -= 1;
            if i < n {
                let rels = m[l].rels;
                m.copy(rels.add(i).addr(), rels.add(i + 1).addr(), ((n - i) * 0x20) as u32);
            }
        } else {
            i += 1;
        }
    }
    close_list(m, l, n);
    1
}

/// [soft 0x081394b8 si_relRemoveARelationId]: drops from a list every entry that matches
/// `key` (with `by_relid` 0: its element index is `key`; otherwise its relation id is
/// `key` and, when `serial` is not negative, its serial number is `serial`), and for each
/// first removes the matching entries from the list at the other end (the backward lists
/// there, or with `other_forward` the forward ones).
#[allow(clippy::too_many_arguments)]
pub fn rel_remove_a_relation_id(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, _elem: Ptr32<Elem>, l: Ptr32<RelList>, key: i32, arg5: i32, arg6: i32, serial: i32, by_relid: i16, other_forward: i16) -> i32 {
    let mut i = 0;
    let mut n = m[l].nb;
    while i < n {
        let r = m[l].rels.add(i);
        let e = m.read(r);
        let hit = if by_relid == 0 {
            e.elemid == key
        } else if serial < 0 {
            e.relid == key
        } else {
            e.relid == key && e.serial == serial
        };
        if hit {
            let other = m[m[ed].lists.add(e.chapid as i32)].elems.add(e.elemid);
            let groups = if other_forward == 0 { m[other].backward.groups } else { m[other].forward.groups };
            rel_remove_a_relation(m, o, ed, other, groups.add(e.group as i32), e.relid, e.chapid as i32, arg5, arg6, serial);
            drop_entry(m, o, ed, r, arg6);
            n -= 1;
            if i < n {
                let rels = m[l].rels;
                m.copy(rels.add(i).addr(), rels.add(i + 1).addr(), ((n - i) * 0x20) as u32);
            }
        } else {
            i += 1;
        }
    }
    close_list(m, l, n);
    1
}

/// [soft 0x081396d8 SI_relRemoveRelations]: empties every forward list of an element (or
/// with `backward` every backward list), after removing each entry's counterpart at the
/// other end.
pub fn rel_remove_relations(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, arg3: i32, arg4: i32, backward: i16) -> i32 {
    let rels: Ptr32<Relations> = if backward != 0 { si_mem::field!(elem, Elem, backward) } else { si_mem::field!(elem, Elem, forward) };
    let mut g = 0;
    while g < m[rels].nb_groups {
        let l = m[rels].groups.add(g);
        let mut k = 0;
        while k < m[l].nb {
            let r = m[l].rels.add(k);
            let e = m.read(r);
            let other = m[m[ed].lists.add(e.chapid as i32)].elems.add(e.elemid);
            let groups = if backward == 0 { m[other].backward.groups } else { m[other].forward.groups };
            rel_remove_a_relation(m, o, ed, other, groups.add(e.group as i32), e.relid, e.chapid as i32, arg3, arg4, -1);
            drop_entry(m, o, ed, r, arg4);
            k += 1;
        }
        g += 1;
    }
    let mut g = 0;
    while g < m[rels].nb_groups {
        let l = m[rels].groups.add(g);
        if !m[l].rels.is_null() {
            let p = m[l].rels.addr();
            m.free(p);
            m[l].rels = Ptr32::NULL;
        }
        reset_list(m, l);
        g += 1;
    }
    1
}

/// [soft 0x0813987c si_relAddRelation]: inserts an entry into a list kept in ascending
/// rank; answers 0 when it is refused.
///
/// Room is made first (one more entry when the list is full), and the capacity counted,
/// whether or not the entry is then refused. Going through the list in order, the place
/// is before the first entry of higher rank; among entries of the same rank an entry
/// goes behind those of its own kind (same relation, chapter and list at the other end),
/// or with `position` at least 1 and a fan-out of 2 or more before the `position`-th of
/// them; with a serial number, behind the entries of that serial number. Refused: the
/// same element again under a relation with a single rule (but for six ids), and on the
/// source side (`is_source`) an entry beyond the rule's fan-out.
#[allow(clippy::too_many_arguments)]
pub fn rel_add_relation(m: &mut Mem, rd: Ptr32<RelDict>, l: Ptr32<RelList>, relid: i32, chap: i32, elemid: i32, group: i32, rank: i32, fanout: i32, position: i32, serial: i32, is_source: i16, data: Ptr32<Opaque>, flag: i16) -> i32 {
    let mut seen_serial = false;
    let mut seen_kind = false;
    let n = m[l].nb;
    if m[l].rels.is_null() {
        m[l].rels = Ptr32::new(m.si_malloc(0x20));
        m[l].capacity += 1;
    } else if n == m[l].capacity {
        let old = m[l].rels.addr();
        m[l].rels = Ptr32::new(m.si_realloc(old, ((n + 1) * 0x20) as u32));
        m[l].capacity += 1;
    }
    let mut i = 0;
    if n >= 1 {
        let mut count = 0;
        let nb_rules = if relid == 20000 { 1 } else { reldict::get_rel_nb_rules(m, rd, relid) };
        let total = m[l].nb;
        let mut p = m.read(m[l].rels);
        if total >= 1 {
            loop {
                if rank < p.rank {
                    break;
                }
                let mut look_at_serial = serial != -1;
                if rank == p.rank {
                    if relid == p.relid && chap == p.chapid as i32 && group == p.group as i32 {
                        count += 1;
                        seen_kind = true;
                        if nb_rules == 1 && elemid == p.elemid && !REPEATABLE.contains(&relid) {
                            return 0;
                        }
                        if !(fanout < 2 || position < 1 || count < position) {
                            break;
                        }
                    } else if serial == -1 {
                        if relid != p.relid {
                            if seen_kind {
                                break;
                            }
                        } else {
                            seen_kind = true;
                            if chap < p.chapid as i32 {
                                break;
                            }
                        }
                        look_at_serial = false;
                    }
                }
                if look_at_serial {
                    if p.serial == serial {
                        seen_serial = true;
                    } else if seen_serial || (p.unk_16 != 0 && rank == p.rank) {
                        break;
                    }
                }
                i += 1;
                if total <= i {
                    break;
                }
                p = m.read(m[l].rels.add(i));
            }
        }
        if is_source != 0 && 0 < i && 0 < n {
            let q = m.read(m[l].rels.add(i - 1));
            if rank == q.rank && relid == q.relid && chap == q.chapid as i32 && group == q.group as i32 && fanout <= count {
                return 0;
            }
        }
        if i < total {
            let rels = m[l].rels;
            m.copy(rels.add(i + 1).addr(), rels.add(i).addr(), ((total - i) * 0x20) as u32);
        }
    }
    let r = m[l].rels.add(i);
    m[r].relid = relid;
    m[r].chapid = chap as i16;
    m[r].elemid = elemid;
    m[r].group = group as i16;
    m[r].flag = flag;
    m[r].data = data;
    m[r].active_elem = -1;
    m[r].serial = serial;
    m[r].unk_16 = 0;
    m[r].rank = rank;
    m[l].nb += 1;
    1
}

/// [soft 0x08139b24 si_relCheck]: the position of the first entry of relation `relid` in
/// list `group`, or −1.
pub fn rel_check(m: &Mem, rels: Ptr32<Relations>, group: i32, relid: i32) -> i32 {
    let l = m[rels].groups.add(group);
    let n = m[l].nb;
    let mut i = 0;
    while i < n {
        if m[m[l].rels.add(i)].relid == relid {
            return i;
        }
        i += 1;
    }
    -1
}

/// [soft 0x08139b68 SI_elemdictGetRelSerialNumber]: the next serial number.
pub fn get_rel_serial_number(m: &mut Mem) -> i32 {
    m[REL_SERIAL_NUMBER] += 1;
    m[REL_SERIAL_NUMBER]
}
