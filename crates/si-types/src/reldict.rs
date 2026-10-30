// SPDX-License-Identifier: MIT
//! The dictionary of relation kinds (`reldict.c`): its rules and the look-ups on them.
//!
//! A relation kind has an id and one or more *rules*; a rule says which chapter the source
//! and the destination belong to, in which of the element's relation lists each end is
//! kept, and the relation's rank and fan-out. The rules of one id are consecutive. The
//! dictionary is filled from the resource file `Relations.rsrc` at start-up; the reader is
//! not part of this module.

use crate::dict::{RelDict, RelRule, SortTbl};
use crate::{Ptr32, Zero};

/// The custom-effect part of a rule (`SI_cusRelInfo`), as `SI_reldictAddRelation` copies it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CusRelInfo {
    pub name: Option<Vec<u8>>,
    pub info: [i32; 6],
    pub s_3c: i16,
    pub max_fanout_dst: i16,
    pub max_fanout_src: i16,
    pub s_42: i16,
    pub s_44: i16,
}

/// A sorted index (`SI_SortTbl`) with the table it points to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SortTable {
    pub count: i32,
    pub table: Vec<i32>,
    pub valid: i32,
}

impl SortTable {
    pub fn record(&self) -> SortTbl {
        SortTbl { count: self.count, table: Ptr32::NULL, valid: self.valid }
    }
}

/// The relation dictionary with the arrays its record points to.
pub struct RelDictStore {
    /// The original's record; `nb` always equals `rules.len()`.
    pub rec: RelDict,
    pub rules: Vec<RelRule>,
    /// Name of each rule's relation (the string `RelRule::name` points to).
    pub names: Vec<Option<Vec<u8>>>,
    /// Custom-effect name of each rule (the string `RelRule::cus_name` points to).
    pub cus_names: Vec<Option<Vec<u8>>>,
    pub sort: Option<SortTable>,
    /// The original keeps the last look-up in three statics shared by all dictionaries
    /// (dictionary, id, index). There is one dictionary in the program; the cache is kept
    /// here, per store, and gives the same answers.
    last_relid: Option<i32>,
    last_index: i32,
}

/// Outputs of [`RelDictStore::get_rel_attributes`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RelAttributes {
    pub src_chap: i32,
    pub dst_chap: i32,
    pub src_group: i32,
    pub dst_group: i32,
    pub rank: i32,
    pub fanout: i32,
}

impl Default for RelDictStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RelDict {
    /// [soft 0x08139db0 SI_reldictInit]
    pub fn init(&mut self) {
        self.nb = 0;
        self.unk_04 = -1;
        self.rules = Ptr32::NULL;
        self.sort = Ptr32::NULL;
    }
}

impl RelDictStore {
    pub fn new() -> Self {
        let mut rec = RelDict::zero();
        rec.init();
        RelDictStore { rec, rules: Vec::new(), names: Vec::new(), cus_names: Vec::new(), sort: None, last_relid: None, last_index: -1 }
    }

    /// Adds a rule and returns its index [soft 0x08139b80 SI_reldictAddRelation].
    ///
    /// The original takes a flag saying whether to duplicate `name` or to keep the caller's
    /// pointer; here the name is always copied. Padding the original leaves uninitialised
    /// (`pad_1e`, `pad_46`, `pad_4a`) is zero.
    #[allow(clippy::too_many_arguments)]
    pub fn add_relation(
        &mut self, name: &[u8], relid: i32, src_chap: i32, dst_chap: i32, src_group: i32, dst_group: i32,
        rank: i32, fanout: i32, dope_disp: i16, upd_attr0: i32, upd_attr1: i32, cus: Option<&CusRelInfo>,
        render: i16, incmode: i32,
    ) -> i32 {
        // the sorted index is created on the first call and marked stale on every call
        let sort = self.sort.get_or_insert_with(SortTable::default);
        sort.valid = 0;
        let index = self.rec.nb;
        sort.table.push(-1);
        let mut r = RelRule::zero();
        r.relid = relid;
        r.src_chap = src_chap as i16;
        r.dst_chap = dst_chap as i16;
        r.src_group = src_group as i16;
        r.dst_group = dst_group as i16;
        r.rank = rank as i16;
        r.fanout = fanout as i16;
        r.dope_disp = dope_disp;
        r.upd_attr0 = upd_attr0 as i16;
        r.upd_attr1 = upd_attr1 as i16;
        r.active = 1;
        r.active_dep = 1;
        r.render = render;
        r.incmode = incmode;
        match cus {
            None => {
                r.max_fanout_src = 1;
                r.max_fanout_dst = 1;
                self.cus_names.push(None);
            }
            Some(c) => {
                r.cus_info = c.info;
                r.cus_3c = c.s_3c;
                r.cus_42 = c.s_42;
                r.cus_44 = c.s_44;
                r.max_fanout_src = c.max_fanout_src;
                r.max_fanout_dst = c.max_fanout_dst;
                self.cus_names.push(c.name.clone());
            }
        }
        self.rules.push(r);
        self.names.push(Some(name.to_vec()));
        self.rec.nb += 1;
        index
    }

    /// Frees everything and re-initialises the record [soft 0x08139dd8 SI_reldictDispose].
    pub fn dispose(&mut self) {
        self.rules.clear();
        self.names.clear();
        self.cus_names.clear();
        self.sort = None;
        self.rec.init();
        // the statics of the look-up are not touched by the original; a stale hit is not
        // possible afterwards because the next look-up finds no sorted index and resets them
    }

    /// Bisection over the sorted index for a relation id
    /// [soft 0x08139e88 findElemInRelSortTbl]: `Ok(position)` when found, else
    /// `Err(position where it belongs)`.
    fn find_in_sort(&self, key: i32, count: i32) -> Result<i32, i32> {
        let table = &self.sort.as_ref().expect("sorted index").table;
        let mut lo = 0;
        let mut mid = (count + 1) / 2;
        let mut hi = count + 1;
        let mut out = 0;
        if mid != 0 {
            loop {
                let id = self.rules[table[(mid - 1) as usize] as usize].relid;
                let mut new_hi = mid;
                if id <= key {
                    new_hi = hi;
                    lo = mid;
                    if key == id {
                        return Ok(mid - 1);
                    }
                }
                mid = (new_hi + lo) / 2;
                out = mid;
                hi = new_hi;
                if mid == lo {
                    break;
                }
            }
        }
        Err(out)
    }

    /// Inserts rule `index` under `key` unless the key is already there
    /// [soft 0x08139ef8 addElemInRelSortTbl]: the index keeps one entry per relation id,
    /// that of its first rule.
    fn add_in_sort(&mut self, index: i32, key: i32, count: &mut i32) {
        if let Err(pos) = self.find_in_sort(key, *count) {
            let table = &mut self.sort.as_mut().expect("sorted index").table;
            if pos != *count {
                table.copy_within(pos as usize..*count as usize, pos as usize + 1);
            }
            table[pos as usize] = index;
            *count += 1;
        }
    }

    /// Index of the first rule of a relation id, or -1
    /// [soft 0x08139f68 SI_reldictGetRelIndexFromId].
    pub fn get_rel_index_from_id(&mut self, relid: i32) -> i32 {
        let state = match &self.sort {
            None => 0,
            Some(s) if s.valid == 0 => 1,
            Some(_) => 2,
        };
        match state {
            0 => {
                // no sorted index (nothing was ever added): a linear search, not cached
                self.last_relid = None;
                self.last_index = self.rules.iter().position(|r| r.relid == relid).map_or(-1, |i| i as i32);
            }
            1 => {
                // the index is stale: rebuild it, noting the first rule of the id on the way
                self.last_relid = None;
                self.last_index = -1;
                let mut count = 0;
                for i in 0..self.rec.nb {
                    let id = self.rules[i as usize].relid;
                    self.add_in_sort(i, id, &mut count);
                    if self.last_relid.is_none() && id == relid {
                        self.last_relid = Some(relid);
                        self.last_index = i;
                    }
                }
                let s = self.sort.as_mut().unwrap();
                s.count = count;
                s.valid = 1;
            }
            _ => {
                if self.last_relid != Some(relid) {
                    let s = self.sort.as_ref().unwrap();
                    self.last_index = match self.find_in_sort(relid, s.count) {
                        Ok(pos) => s.table[pos as usize],
                        Err(_) => -1,
                    };
                    self.last_relid = Some(relid);
                }
            }
        }
        self.last_index
    }

    /// [soft 0x0813a0f4 SI_reldictGetRelName]: the name, or `unknown`.
    pub fn get_rel_name(&mut self, relid: i32) -> Vec<u8> {
        let i = self.get_rel_index_from_id(relid);
        if i < 0 {
            b"unknown".to_vec()
        } else {
            self.names[i as usize].clone().unwrap_or_default()
        }
    }

    /// Which rule of `relid` joins these chapters and groups: its number counted from 1,
    /// and its fan-out [soft 0x0813a12c si_reldictGetRelSub]. When none matches the
    /// original returns 1 and leaves the fan-out untouched: `(1, None)`.
    ///
    /// For an unknown id the original starts reading one rule before the array; here that
    /// case answers `(1, None)`.
    pub fn get_rel_sub(&mut self, relid: i32, src_chap: i32, dst_chap: i32, src_group: i32, dst_group: i32) -> (i32, Option<i32>) {
        let first = self.get_rel_index_from_id(relid);
        if first < 0 {
            return (1, None);
        }
        let mut number = 1;
        for r in &self.rules[first as usize..] {
            if r.relid != relid {
                return (1, None);
            }
            if r.src_chap as i32 == src_chap && r.dst_chap as i32 == dst_chap && r.src_group as i32 == src_group
                && r.dst_group as i32 == dst_group
            {
                return (number, Some(r.fanout as i32));
            }
            number += 1;
        }
        (1, None)
    }

    /// Number of rules of a relation id [soft 0x0813a1ac SI_reldictGetRelNbRules].
    /// For an unknown id the original reads one rule before the array; here that is 0.
    pub fn get_rel_nb_rules(&mut self, relid: i32) -> i32 {
        let first = self.get_rel_index_from_id(relid);
        if first < 0 {
            return 0;
        }
        self.rules[first as usize..].iter().take_while(|r| r.relid == relid).count() as i32
    }

    /// The chapters, groups, rank and fan-out of rule number `rule` (from 1; anything
    /// below 1 means 1) of a relation id [soft 0x0813a28c SI_reldictGetRelAttributes].
    /// `None` when the id is unknown. The original does not check that the rule number
    /// stays inside the array; here a number past the end is `None` too.
    pub fn get_rel_attributes(&mut self, relid: i32, rule: i32) -> Option<RelAttributes> {
        let rule = rule.max(1);
        let first = self.get_rel_index_from_id(relid);
        if first < 0 {
            return None;
        }
        let r = self.rules.get((first + rule - 1) as usize)?;
        Some(RelAttributes {
            src_chap: r.src_chap as i32,
            dst_chap: r.dst_chap as i32,
            src_group: r.src_group as i32,
            dst_group: r.dst_group as i32,
            rank: r.rank as i32,
            fanout: r.fanout as i32,
        })
    }

    fn first_rule(&mut self, relid: i32) -> Option<RelRule> {
        let i = self.get_rel_index_from_id(relid);
        if i < 0 { None } else { Some(self.rules[i as usize]) }
    }

    /// [soft 0x0813a3f4 SI_reldictGetRelRenderFlag]
    pub fn get_rel_render_flag(&mut self, relid: i32) -> i32 {
        self.first_rule(relid).map_or(0, |r| r.render as i32)
    }

    /// [soft 0x0813a428 SI_reldictGetRelIncmodeFlag]
    pub fn get_rel_incmode_flag(&mut self, relid: i32) -> i32 {
        self.first_rule(relid).map_or(-1, |r| r.incmode)
    }

    /// Source chapter and source group of the first rule
    /// [soft 0x0813a464 SI_reldictGetRelChapGrp]; `(-1, 0)` for an unknown id.
    pub fn get_rel_chap_grp(&mut self, relid: i32) -> (i32, i32) {
        self.first_rule(relid).map_or((-1, 0), |r| (r.src_chap as i32, r.src_group as i32))
    }

    /// [soft 0x0813a4b4 reldictGetRelDopeDisp]
    pub fn get_rel_dope_disp(&mut self, relid: i32) -> i32 {
        self.first_rule(relid).map_or(0, |r| r.dope_disp as i32)
    }

    /// [soft 0x0813a4e8 SI_reldictGetRelRank]
    pub fn get_rel_rank(&mut self, relid: i32) -> i32 {
        self.first_rule(relid).map_or(0, |r| r.rank as i32)
    }

    /// [soft 0x0813a51c si_reldictGetRelUpdlattr]
    pub fn get_rel_updlattr(&mut self, relid: i32) -> Option<(i32, i32)> {
        self.first_rule(relid).map(|r| (r.upd_attr0 as i32, r.upd_attr1 as i32))
    }

    /// [soft 0x0813a56c SI_reldictIsRelActive]: the `active` flag, or with `dependent`
    /// the `active_dep` flag, of the first rule.
    pub fn is_rel_active(&mut self, relid: i32, dependent: bool) -> i32 {
        self.first_rule(relid).map_or(0, |r| if dependent { r.active_dep as i32 } else { r.active as i32 })
    }

    /// [soft 0x0813a5bc SI_reldictGetRelIdByName]: -1 when no rule has the name.
    pub fn get_rel_id_by_name(&self, name: &[u8]) -> i32 {
        self.names.iter().position(|n| n.as_deref() == Some(name)).map_or(-1, |i| self.rules[i].relid)
    }

    /// True when the first rule of the id has -10 as either chapter
    /// [soft 0x0813a770 SI_reldictCheckUnCompleteRelation].
    pub fn check_uncomplete_relation(&mut self, relid: i32) -> bool {
        self.first_rule(relid).is_some_and(|r| r.dst_chap == -10 || r.src_chap == -10)
    }

    /// The maximum fan-out of rule `index` on the source side, or with `dst` on the
    /// destination side [soft 0x0813f9cc SI_reldictGetMaxFanout]; -1 for a bad index. The
    /// original accepts `index == nb` and reads one rule past the array; here that is -1.
    pub fn get_max_fanout(&self, index: i32, dst: bool) -> i32 {
        if index < 0 || index >= self.rec.nb {
            return -1;
        }
        let r = &self.rules[index as usize];
        (if dst { r.max_fanout_dst } else { r.max_fanout_src }) as i32
    }

    /// Index of the rule with this relation name and custom-effect name, or -1
    /// [soft 0x0813eae8 SI_reldictGetRelIndexFromName].
    pub fn get_rel_index_from_name(&self, cus_name: &[u8], name: &[u8]) -> i32 {
        (0..self.rules.len())
            .position(|i| self.names[i].as_deref() == Some(name) && self.cus_names[i].as_deref() == Some(cus_name))
            .map_or(-1, |i| i as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> RelDictStore {
        let mut d = RelDictStore::new();
        // ids deliberately out of order; id 30 has two rules
        for (name, id, sc, dc, sg, dg, rank, fo) in [
            (&b"B"[..], 30, 8, 8, 0, 0, 5, 1),
            (b"B", 30, 8, 2, 1, 0, 6, 4),
            (b"A", 10, 7, 8, 2, 3, 1, 2),
            (b"C", 20, -10, 8, 0, 0, 9, 1),
        ] {
            d.add_relation(name, id, sc, dc, sg, dg, rank, fo, 3, 11, 12, None, 1, 7);
        }
        d
    }

    #[test]
    fn a_new_dictionary_is_as_the_original_initialises_it() {
        let d = RelDictStore::new();
        assert_eq!((d.rec.nb, d.rec.unk_04), (0, -1));
        assert!(d.rec.rules.is_null() && d.rec.sort.is_null());
    }

    #[test]
    fn index_by_id_uses_the_first_rule_and_survives_additions() {
        let mut d = dict();
        assert_eq!(d.get_rel_index_from_id(30), 0); // rebuild path
        assert_eq!(d.sort.as_ref().unwrap().table[..3], [2, 3, 0]); // ids 10, 20, 30: one entry each
        assert_eq!(d.sort.as_ref().unwrap().count, 3);
        assert_eq!(d.get_rel_index_from_id(10), 2); // bisection path
        assert_eq!(d.get_rel_index_from_id(20), 3);
        assert_eq!(d.get_rel_index_from_id(30), 0);
        assert_eq!(d.get_rel_index_from_id(25), -1);
        assert_eq!(d.get_rel_index_from_id(25), -1); // cached miss
        let i = d.add_relation(b"D", 5, 1, 1, 0, 0, 0, 1, 0, 0, 0, None, 0, 0);
        assert_eq!(i, 4);
        assert_eq!(d.sort.as_ref().unwrap().valid, 0);
        assert_eq!(d.get_rel_index_from_id(5), 4);
        assert_eq!(d.get_rel_index_from_id(30), 0);
    }

    #[test]
    fn rule_look_ups() {
        let mut d = dict();
        assert_eq!(d.get_rel_nb_rules(30), 2);
        assert_eq!(d.get_rel_nb_rules(10), 1);
        assert_eq!(d.get_rel_nb_rules(99), 0);
        assert_eq!(d.get_rel_sub(30, 8, 2, 1, 0), (2, Some(4)));
        assert_eq!(d.get_rel_sub(30, 1, 1, 1, 1), (1, None));
        let a = d.get_rel_attributes(30, 2).unwrap();
        assert_eq!((a.src_chap, a.dst_chap, a.src_group, a.dst_group, a.rank, a.fanout), (8, 2, 1, 0, 6, 4));
        assert_eq!(d.get_rel_attributes(30, 0), d.get_rel_attributes(30, 1));
        assert_eq!(d.get_rel_attributes(99, 1), None);
        assert_eq!(d.get_rel_name(10), b"A");
        assert_eq!(d.get_rel_name(99), b"unknown");
        assert_eq!((d.get_rel_rank(10), d.get_rel_dope_disp(10), d.get_rel_render_flag(10), d.get_rel_incmode_flag(10)), (1, 3, 1, 7));
        assert_eq!((d.get_rel_rank(99), d.get_rel_incmode_flag(99)), (0, -1));
        assert_eq!(d.get_rel_chap_grp(10), (7, 2));
        assert_eq!(d.get_rel_chap_grp(99), (-1, 0));
        assert_eq!(d.get_rel_updlattr(10), Some((11, 12)));
        assert_eq!((d.is_rel_active(10, false), d.is_rel_active(10, true), d.is_rel_active(99, true)), (1, 1, 0));
        assert_eq!((d.get_rel_id_by_name(b"C"), d.get_rel_id_by_name(b"Z")), (20, -1));
        assert!(d.check_uncomplete_relation(20) && !d.check_uncomplete_relation(10));
        assert_eq!((d.get_max_fanout(0, false), d.get_max_fanout(0, true), d.get_max_fanout(9, false)), (1, 1, -1));
    }

    #[test]
    fn custom_information_is_copied_and_found_by_name() {
        let mut d = dict();
        let cus = CusRelInfo { name: Some(b"fx".to_vec()), info: [1, 2, 3, 4, 5, 6], s_3c: 7, max_fanout_dst: 8, max_fanout_src: 9, s_42: 10, s_44: 11 };
        let i = d.add_relation(b"E", 40, 8, 8, 0, 0, 0, 1, 0, 0, 0, Some(&cus), 0, 0);
        let r = d.rules[i as usize];
        assert_eq!((r.cus_info, r.cus_3c, r.max_fanout_dst, r.max_fanout_src, r.cus_42, r.cus_44), ([1, 2, 3, 4, 5, 6], 7, 8, 9, 10, 11));
        assert_eq!(d.get_rel_index_from_name(b"fx", b"E"), i);
        assert_eq!(d.get_rel_index_from_name(b"fx", b"A"), -1);
        assert_eq!((d.get_max_fanout(i, false), d.get_max_fanout(i, true)), (9, 8));
        d.dispose();
        assert_eq!((d.rec.nb, d.rec.unk_04, d.rules.len()), (0, -1, 0));
        assert_eq!(d.get_rel_index_from_id(40), -1);
    }
}
