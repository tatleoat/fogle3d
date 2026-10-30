// SPDX-License-Identifier: MIT
//! `reldict.c`: the relation dictionary (`SI_RD`), on the image.
//!
//! A dictionary (`SI_RelDict`, 16 bytes) is an array of 80-byte rules, grown by one record
//! per addition (`SI_malloc(0x50)` for the first, `SI_realloc(rules, (n + 1) * 0x50)`
//! after), and a sort table (`SI_SortTbl`: `SI_calloc(1, 12)`, with an index array of one
//! word per rule grown beside the rules). The rules of one relation id are consecutive;
//! the table holds one entry per id, the index of its first rule, ordered by id, and is
//! rebuilt by the first look-up after an addition.
//!
//! The look-up keeps its last answer in three statics (`last_prd`, `last_relid`,
//! `last_index`). They are part of the image, so a second look-up of the same id in the
//! same dictionary answers from them even when the dictionary changed in between, as the
//! original does.

use crate::globals::{LAST_INDEX, LAST_PRD, LAST_RELID, SI_ELEM_DICTS, SI_PROC_DATA, SI_RD};
use crate::libc::strcmp;
use crate::records::{CusRelInfo, CustomRelData};
use crate::rodata::S_UNKNOWN;
use si_mem::{CStr32, Mem, Ptr32};
use si_types::dict::{ChapIds, RelDict, RelRule, SortTbl};

/// [soft 0x08139b80 SI_reldictAddRelation]: appends a rule and answers its index. The
/// sort table is made on the first call and marked stale on every one; its index array
/// grows with the rules, the new slot holding −1. `dup_name` makes the rule own a copy of
/// the name. Without custom-effect information the block at +0x20 is zero and the two
/// fan-out limits are 1.
#[allow(clippy::too_many_arguments)]
pub fn add_relation(
    m: &mut Mem,
    rd: Ptr32<RelDict>,
    name: CStr32,
    relid: i32,
    src_chap: i32,
    dst_chap: i32,
    src_group: i32,
    dst_group: i32,
    rank: i32,
    fanout: i32,
    dope_disp: i16,
    upd_attr0: i32,
    upd_attr1: i32,
    dup_name: i16,
    cus: Ptr32<CusRelInfo>,
    render: i16,
    incmode: i32,
) -> i32 {
    let mut tbl = m[rd].sort;
    if tbl.is_null() {
        tbl = Ptr32::new(m.si_calloc(1, 0xc));
        m[rd].sort = tbl;
        m[tbl].table = Ptr32::NULL;
        m[tbl].valid = 0;
    } else {
        m[tbl].valid = 0;
    }
    let n = m[rd].nb;
    if m[rd].rules.is_null() {
        m[rd].rules = Ptr32::new(m.si_malloc(0x50));
        let p: Ptr32<i32> = Ptr32::new(m.si_malloc(4));
        m[tbl].table = p;
        m[p] = -1;
    } else {
        let old = m[rd].rules.addr();
        m[rd].rules = Ptr32::new(m.si_realloc(old, ((n * 5 + 5) * 0x10) as u32));
        let old = m[tbl].table.addr();
        let p: Ptr32<i32> = Ptr32::new(m.si_realloc(old, (n * 4 + 4) as u32));
        m[tbl].table = p;
        m[p.add(n)] = -1;
    }
    let mut name = name;
    if dup_name != 0 {
        name = Ptr32::new(m.strdup(name.addr()));
    }
    let r = m[rd].rules.add(n);
    m[r].name = name;
    m[r].relid = relid;
    m[r].src_chap = src_chap as i16;
    m[r].dst_chap = dst_chap as i16;
    m[r].src_group = src_group as i16;
    m[r].dst_group = dst_group as i16;
    m[r].rank = rank as i16;
    m[r].fanout = fanout as i16;
    m[r].dope_disp = dope_disp;
    m[r].upd_attr0 = upd_attr0 as i16;
    m[r].upd_attr1 = upd_attr1 as i16;
    m[r].active = 1;
    m[r].active_dep = 1;
    m[r].render = render;
    m[r].incmode = incmode;
    if cus.is_null() {
        m[r].cus_name = Ptr32::NULL;
        m[r].cus_info = [0; 6];
        m[r].cus_3c = 0;
        m[r].cus_42 = 0;
        m[r].cus_44 = 0;
        m[r].max_fanout_src = 1;
        m[r].max_fanout_dst = 1;
    } else {
        let c = m.read(cus);
        m[r].cus_name = c.name;
        m[r].cus_info = c.info;
        m[r].cus_3c = c.new_src;
        m[r].cus_42 = c.w22;
        m[r].cus_44 = c.w24;
        m[r].max_fanout_src = c.max_fanout_src;
        m[r].max_fanout_dst = c.max_fanout_dst;
    }
    m[rd].nb += 1;
    n
}

/// [soft 0x08139db0 SI_reldictInit]
pub fn init(m: &mut Mem, rd: Ptr32<RelDict>) {
    m[rd].nb = 0;
    m[rd].unk_04 = -1;
    m[rd].rules = Ptr32::NULL;
    m[rd].sort = Ptr32::NULL;
}

/// [soft 0x08139dd8 SI_reldictDispose]: frees every rule's name (whether or not the rule
/// owned a copy), the rules, the sort table's index array and the table.
pub fn dispose(m: &mut Mem, rd: Ptr32<RelDict>) {
    let mut i = 0;
    while i < m[rd].nb {
        let r = m[rd].rules.add(i);
        let name = m[r].name;
        if !name.is_null() {
            m.free(name.addr());
            m[r].name = Ptr32::NULL;
        }
        i += 1;
    }
    if !m[rd].rules.is_null() {
        let p = m[rd].rules.addr();
        m.free(p);
        m[rd].rules = Ptr32::NULL;
    }
    let tbl = m[rd].sort;
    if !tbl.is_null() {
        if !m[tbl].table.is_null() {
            let p = m[tbl].table.addr();
            m.free(p);
            m[tbl].table = Ptr32::NULL;
        }
        m.free(tbl.addr());
    }
    init(m, rd);
}

/// [soft 0x08139e88 findElemInRelSortTbl]: bisection in the first `count` entries of the
/// table for the rule with id `relid`. Found: `*out` is the entry's position, the answer
/// 1. Not found: `*out` is where it would go, the answer 0.
pub fn find_elem_in_rel_sort_tbl(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32, tbl: Ptr32<SortTbl>, count: i32, out: Ptr32<i32>) -> i16 {
    let mut lo = 0;
    let mut mid = (count + 1) / 2;
    let mut pos = 0;
    let mut hi = count + 1;
    if mid != 0 {
        loop {
            let index = m[m[tbl].table.add(mid - 1)];
            let key = m[m[rd].rules.add(index)].relid;
            let mut next_hi = mid;
            if key <= relid {
                next_hi = hi;
                lo = mid;
                if relid == key {
                    m[out] = mid - 1;
                    return 1;
                }
            }
            mid = (next_hi + lo) / 2;
            pos = mid;
            hi = next_hi;
            if mid == lo {
                break;
            }
        }
    }
    m[out] = pos;
    0
}

/// [soft 0x08139ef8 addElemInRelSortTbl]: puts rule `index` (whose id is `relid`) into
/// the table unless a rule of that id is there already; `*count` is the table's length.
pub fn add_elem_in_rel_sort_tbl(m: &mut Mem, index: i32, rd: Ptr32<RelDict>, relid: i32, tbl: Ptr32<SortTbl>, count: Ptr32<i32>) {
    let pos: Ptr32<i32> = Ptr32::new(m.local(4));
    let n = m[count];
    if find_elem_in_rel_sort_tbl(m, rd, relid, tbl, n, pos) == 0 {
        let at = m[pos];
        let table = m[tbl].table;
        if at != m[count] {
            let len = (m[count] - at) * 4;
            m.copy(table.add(at + 1).addr(), table.add(at).addr(), len as u32);
        }
        m[table.add(at)] = index;
        m[count] += 1;
    }
    m.release(4);
}

/// [soft 0x08139f68 SI_reldictGetRelIndexFromId]: the index of the first rule of relation
/// `relid`, or −1. Without a sort table the rules are gone through in order. With a stale
/// one the table is rebuilt first; this path answers the last cached index, which the
/// rebuild sets only when it meets `relid` (so a stale table and an id that is not there
/// answer −1). With a valid one the three statics answer a repeat of the last question,
/// anything else is found by bisection.
pub fn get_rel_index_from_id(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32) -> i32 {
    let tbl = m[rd].sort;
    if tbl.is_null() {
        m[LAST_PRD] = Ptr32::NULL;
        m[LAST_RELID] = 0x7fff_ffff;
        m[LAST_INDEX] = -1;
        let mut i = 0;
        if 0 < m[rd].nb {
            if m[m[rd].rules].relid == relid {
                m[LAST_INDEX] = 0;
            } else {
                loop {
                    i += 1;
                    if m[rd].nb <= i {
                        m[LAST_PRD] = Ptr32::NULL;
                        m[LAST_RELID] = 0x7fff_ffff;
                        m[LAST_INDEX] = -1;
                        return -1;
                    }
                    m[LAST_INDEX] = i;
                    if m[m[rd].rules.add(i)].relid == relid {
                        break;
                    }
                }
            }
        }
    } else if m[tbl].valid == 0 {
        m[LAST_PRD] = Ptr32::NULL;
        m[LAST_RELID] = 0x7fff_ffff;
        m[LAST_INDEX] = -1;
        let count: Ptr32<i32> = Ptr32::new(m.local(4));
        m[count] = 0;
        let mut i = 0;
        while i < m[rd].nb {
            let id = m[m[rd].rules.add(i)].relid;
            add_elem_in_rel_sort_tbl(m, i, rd, id, tbl, count);
            if m[LAST_PRD].is_null() && m[m[rd].rules.add(i)].relid == relid {
                m[LAST_PRD] = rd;
                m[LAST_RELID] = relid;
                m[LAST_INDEX] = i;
            }
            i += 1;
        }
        m[tbl].count = m[count];
        m[tbl].valid = 1;
        m.release(4);
    } else if relid != m[LAST_RELID] || rd != m[LAST_PRD] {
        let n = m[tbl].count;
        if find_elem_in_rel_sort_tbl(m, rd, relid, tbl, n, LAST_INDEX) == 0 {
            m[LAST_INDEX] = -1;
        } else {
            let at = m[LAST_INDEX];
            m[LAST_INDEX] = m[m[tbl].table.add(at)];
        }
        m[LAST_PRD] = rd;
        m[LAST_RELID] = relid;
    }
    m[LAST_INDEX]
}

/// The rule at `index` of the dictionary.
fn rule(m: &Mem, rd: Ptr32<RelDict>, index: i32) -> Ptr32<RelRule> {
    m[rd].rules.add(index)
}

/// [soft 0x0813a0f4 SI_reldictGetRelName]: the name of the relation's first rule, or the
/// literal `unknown`.
pub fn get_rel_name(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32) -> CStr32 {
    let i = get_rel_index_from_id(m, rd, relid);
    if i < 0 {
        S_UNKNOWN
    } else {
        m[rule(m, rd, i)].name
    }
}

/// [soft 0x0813a12c si_reldictGetRelSub]: among the rules of `relid`, the number (from 1)
/// of the one with these chapters and lists; its fan-out goes to `*fanout`. Without a
/// match the answer is 1 and `*fanout` is left alone. An id that is not in the dictionary
/// starts the walk at index −1, one rule before the array, as the original does.
#[allow(clippy::too_many_arguments)]
pub fn get_rel_sub(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32, src_chap: i32, dst_chap: i32, src_group: i32, dst_group: i32, fanout: Ptr32<i32>) -> i32 {
    let mut i = get_rel_index_from_id(m, rd, relid);
    let mut number = 1;
    while i < m[rd].nb {
        let r = m.read(rule(m, rd, i));
        if r.relid != relid {
            return 1;
        }
        if r.src_chap as i32 == src_chap && r.dst_chap as i32 == dst_chap && r.src_group as i32 == src_group && r.dst_group as i32 == dst_group {
            m[fanout] = r.fanout as i32;
            return number;
        }
        number += 1;
        i += 1;
    }
    1
}

/// [soft 0x0813a1ac SI_reldictGetRelNbRules]: how many consecutive rules carry `relid`.
/// (For an id that is not in the dictionary the first rule looked at is the one before
/// the array.)
pub fn get_rel_nb_rules(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32) -> i32 {
    let mut i = get_rel_index_from_id(m, rd, relid);
    let mut n = 0;
    if i < m[rd].nb && m[rule(m, rd, i)].relid == relid {
        loop {
            n += 1;
            i += 1;
            if m[rd].nb <= i {
                return n;
            }
            if m[rule(m, rd, i)].relid != relid {
                break;
            }
        }
    }
    n
}

/// [soft 0x0813a1fc SI_reldictCheckRelAnim]: whether a rule of `relid` has the chapter in
/// the first word of `cid` (ANIMATION) as its destination; `*fanout` is that rule's
/// fan-out, 1 otherwise. The walk tests a rule's destination before it tests the rule's
/// id, so the first rule after the relation's own is looked at as well.
pub fn check_rel_anim(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32, fanout: Ptr32<i32>, cid: Ptr32<ChapIds>) -> i16 {
    m[fanout] = 1;
    let mut i = get_rel_index_from_id(m, rd, relid);
    let mut found = 0;
    while i < m[rd].nb {
        let r = m.read(rule(m, rd, i));
        let ani = m[cid].ani;
        if found == 0 && r.dst_chap as i32 == ani {
            m[fanout] = r.fanout as i32;
        }
        if found != 0 || r.dst_chap as i32 == ani {
            found = 1;
        }
        if r.relid != relid {
            return found;
        }
        i += 1;
    }
    found
}

/// The six numbers [`get_rel_attributes`] hands out, for callers in this crate whose
/// out-parameters are their own locals: source chapter, destination chapter, source
/// list, destination list, rank, fan-out of rule number `rule_no` (from 1; below 1 is 1)
/// of the relation. The rule is taken by position after the relation's first: a number
/// beyond the relation's rules reads the rule of the next relation, as the original does.
pub(crate) fn rel_attributes(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32, rule_no: i32) -> Option<[i32; 6]> {
    let rule_no = rule_no.max(1);
    let i = get_rel_index_from_id(m, rd, relid);
    if i < 0 {
        return None;
    }
    let r = m.read(rule(m, rd, i + rule_no - 1));
    Some([r.src_chap as i32, r.dst_chap as i32, r.src_group as i32, r.dst_group as i32, r.rank as i32, r.fanout as i32])
}

/// [soft 0x0813a28c SI_reldictGetRelAttributes]: see [`rel_attributes`]; the fourth
/// argument is not used. Nothing is stored when the relation is not in the dictionary.
#[allow(clippy::too_many_arguments)]
pub fn get_rel_attributes(
    m: &mut Mem,
    rd: Ptr32<RelDict>,
    relid: i32,
    rule_no: i32,
    _unused: i32,
    src_chap: Ptr32<i32>,
    dst_chap: Ptr32<i32>,
    src_group: Ptr32<i32>,
    dst_group: Ptr32<i32>,
    rank: Ptr32<i32>,
    fanout: Ptr32<i32>,
) -> bool {
    match rel_attributes(m, rd, relid, rule_no) {
        Some(a) => {
            for (p, v) in [src_chap, dst_chap, src_group, dst_group, rank, fanout].into_iter().zip(a) {
                m[p] = v;
            }
            true
        }
        None => false,
    }
}

/// The two numbers of [`cus_get_rel_attributes`] (rank, fan-out), for callers in this
/// crate.
pub(crate) fn cus_rel_attributes(m: &mut Mem, rd: Ptr32<RelDict>, data: Ptr32<CustomRelData>, rule_no: i32) -> Option<[i32; 2]> {
    if rd.is_null() || data.is_null() {
        return None;
    }
    let name = m[data].name;
    let effect = m[data].effect;
    if name.is_null() || effect.is_null() {
        return Some([0x32, 0x7fff]);
    }
    let i = get_rel_index_from_name(m, rd, effect, name);
    if -1 < i {
        let rule_no = rule_no.max(1);
        if rule_no + i - 1 < m[rd].nb {
            let r = m.read(rule(m, rd, i + rule_no - 1));
            if rule_no < 2
                || (r.relid == 20000
                    && !r.name.is_null()
                    && strcmp(m, r.name.addr(), name.addr()) == 0
                    && !r.cus_name.is_null()
                    && strcmp(m, r.cus_name.addr(), effect.addr()) == 0)
            {
                return Some([r.rank as i32, r.fanout as i32]);
            }
        }
    }
    None
}

/// [soft 0x0813a30c SI_reldictCusGetRelAttributes]: rank and fan-out of rule number
/// `rule_no` of the custom-effect relation the data names (two strings at +0x20 and +0x24
/// of it). Data without both strings answers rank 50 and fan-out 0x7fff.
pub fn cus_get_rel_attributes(m: &mut Mem, rd: Ptr32<RelDict>, data: Ptr32<CustomRelData>, rule_no: i32, rank: Ptr32<i32>, fanout: Ptr32<i32>) -> i32 {
    match cus_rel_attributes(m, rd, data, rule_no) {
        Some([r, f]) => {
            m[rank] = r;
            m[fanout] = f;
            1
        }
        None => 0,
    }
}

/// [soft 0x0813a3f4 SI_reldictGetRelRenderFlag]
pub fn get_rel_render_flag(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32) -> i32 {
    let i = get_rel_index_from_id(m, rd, relid);
    if i < 0 {
        0
    } else {
        m[rule(m, rd, i)].render as i32
    }
}

/// [soft 0x0813a428 SI_reldictGetRelIncmodeFlag]
pub fn get_rel_incmode_flag(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32) -> i32 {
    let i = get_rel_index_from_id(m, rd, relid);
    if i < 0 {
        -1
    } else {
        m[rule(m, rd, i)].incmode
    }
}

/// [soft 0x0813a464 SI_reldictGetRelChapGrp]: source chapter of the first rule (−1 when
/// the relation is unknown), its source list in `*group` (0 then).
pub fn get_rel_chap_grp(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32, group: Ptr32<i32>) -> i32 {
    let i = get_rel_index_from_id(m, rd, relid);
    if i < 0 {
        m[group] = 0;
        -1
    } else {
        m[group] = m[rule(m, rd, i)].src_group as i32;
        m[rule(m, rd, i)].src_chap as i32
    }
}

/// [soft 0x0813a4b4 reldictGetRelDopeDisp]
pub fn get_rel_dope_disp(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32) -> i32 {
    let i = get_rel_index_from_id(m, rd, relid);
    if i < 0 {
        0
    } else {
        m[rule(m, rd, i)].dope_disp as i32
    }
}

/// [soft 0x0813a4e8 SI_reldictGetRelRank]
pub fn get_rel_rank(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32) -> i32 {
    let i = get_rel_index_from_id(m, rd, relid);
    if i < 0 {
        0
    } else {
        m[rule(m, rd, i)].rank as i32
    }
}

/// The two update values of the first rule, for callers in this crate.
pub(crate) fn rel_updlattr(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32) -> Option<[i32; 2]> {
    let i = get_rel_index_from_id(m, rd, relid);
    (-1 < i).then(|| [m[rule(m, rd, i)].upd_attr0 as i32, m[rule(m, rd, i)].upd_attr1 as i32])
}

/// [soft 0x0813a51c si_reldictGetRelUpdlattr]
pub fn get_rel_updlattr(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32, a: Ptr32<i32>, b: Ptr32<i32>) -> bool {
    match rel_updlattr(m, rd, relid) {
        Some([x, y]) => {
            m[a] = x;
            m[b] = y;
            true
        }
        None => false,
    }
}

/// [soft 0x0813a56c SI_reldictIsRelActive]: the first rule's active flag, or with
/// `dep` its "active dependency" flag; 0 for an unknown relation.
pub fn is_rel_active(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32, dep: i16) -> i32 {
    let i = get_rel_index_from_id(m, rd, relid);
    if i < 0 {
        0
    } else if dep == 0 {
        m[rule(m, rd, i)].active as i32
    } else {
        m[rule(m, rd, i)].active_dep as i32
    }
}

/// [soft 0x0813a5bc SI_reldictGetRelIdByName]: the id of the first rule called `name`,
/// or −1.
pub fn get_rel_id_by_name(m: &Mem, rd: Ptr32<RelDict>, name: CStr32) -> i32 {
    let mut i = 0;
    while i < m[rd].nb {
        let r = rule(m, rd, i);
        if strcmp(m, name.addr(), m[r].name.addr()) == 0 {
            return m[r].relid;
        }
        i += 1;
    }
    -1
}

/// [soft 0x0813a60c SI_reldictActiveSet]: sets the active flag (or with `dep` the
/// dependency flag) of every rule of the global dictionary `SI_RD` (`relid` below 0) or
/// of the rules of one relation. With `dep` the current element dictionary's update list
/// and hierarchy flag are marked dirty.
pub fn active_set(m: &mut Mem, relid: i32, value: i16, dep: i16) {
    let rd = SI_RD;
    if relid < 0 {
        let mut i = 0;
        while i < m[rd].nb {
            let r = rule(m, rd, i);
            if dep == 0 {
                m[r].active = value;
            } else {
                m[r].active_dep = value;
            }
            i += 1;
        }
    } else {
        let mut i = get_rel_index_from_id(m, rd, relid);
        if -1 < i {
            loop {
                let r = rule(m, rd, i);
                if dep == 0 {
                    m[r].active = value;
                } else {
                    m[r].active_dep = value;
                }
                i += 1;
                if !(m[rule(m, rd, i)].relid == relid && i < m[rd].nb) {
                    break;
                }
            }
        }
    }
    if dep != 0 {
        let cur = m[m[SI_PROC_DATA]];
        let ed = m[SI_ELEM_DICTS].add(cur);
        m[ed].upd_list.dirty = 1;
        m[ed].hier_dirty = 1;
    }
}

/// [soft 0x0813a714 SI_reldictNbRelForDeactCount]: the sum of the fan-outs of the
/// relation's rules whose dope-sheet display value is not 0.
pub fn nb_rel_for_deact_count(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32, _unused: i32) -> i32 {
    let mut total = 0;
    let mut i = get_rel_index_from_id(m, rd, relid);
    if i < m[rd].nb {
        while m[rule(m, rd, i)].relid == relid {
            if m[rule(m, rd, i)].dope_disp != 0 {
                total += m[rule(m, rd, i)].fanout as i32;
            }
            i += 1;
            if m[rd].nb <= i {
                return total;
            }
        }
    }
    total
}

/// [soft 0x0813a770 SI_reldictCheckUnCompleteRelation]: whether the relation's first
/// rule leaves a chapter open (−10).
pub fn check_uncomplete_relation(m: &mut Mem, rd: Ptr32<RelDict>, relid: i32) -> i32 {
    let i = get_rel_index_from_id(m, rd, relid);
    if i < 0 {
        return 0;
    }
    let r = rule(m, rd, i);
    (m[r].dst_chap == -10 || m[r].src_chap == -10) as i32
}

/// [soft 0x0813eae8 SI_reldictGetRelIndexFromName]: the index of the first rule whose
/// name is `name` and whose custom-effect name is `effect`, or −1.
pub fn get_rel_index_from_name(m: &Mem, rd: Ptr32<RelDict>, effect: CStr32, name: CStr32) -> i32 {
    if !rd.is_null() && !effect.is_null() && !name.is_null() {
        let mut i = 0;
        while i < m[rd].nb {
            let r = m.read(rule(m, rd, i));
            if !r.name.is_null() && strcmp(m, r.name.addr(), name.addr()) == 0 && !r.cus_name.is_null() && strcmp(m, r.cus_name.addr(), effect.addr()) == 0 {
                return i;
            }
            i += 1;
        }
    }
    -1
}

/// [soft 0x0813f9cc SI_reldictGetMaxFanout]: a rule's fan-out limit on the source side,
/// or with `dst` on the destination side; −1 for a null dictionary or an index outside
/// it. An index equal to the count passes the test and reads one rule past the array.
pub fn get_max_fanout(m: &Mem, rd: Ptr32<RelDict>, index: i32, dst: i16) -> i32 {
    if rd.is_null() || m[rd].nb < index || index < 0 {
        -1
    } else if dst == 0 {
        m[rule(m, rd, index)].max_fanout_src as i32
    } else {
        m[rule(m, rd, index)].max_fanout_dst as i32
    }
}

/// [soft 0x0813fa18 SI_reldictIsNewSrc]: the 16-bit value at +0x3c of a rule (from the
/// custom-effect information); a negative index is not refused.
pub fn is_new_src(m: &Mem, rd: Ptr32<RelDict>, index: i32) -> i32 {
    if rd.is_null() || m[rd].nb < index {
        -1
    } else {
        m[rule(m, rd, index)].cus_3c as i32
    }
}

/// [soft 0x0813fa44 SI_reldictGetRelRankByIndex]
pub fn get_rel_rank_by_index(m: &Mem, rd: Ptr32<RelDict>, index: i32) -> i32 {
    if rd.is_null() || m[rd].nb < index {
        -1
    } else {
        m[rule(m, rd, index)].rank as i32
    }
}
