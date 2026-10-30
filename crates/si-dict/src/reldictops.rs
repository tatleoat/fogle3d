// SPDX-License-Identifier: MIT
//! `reldict.c`, the second half: the custom operators' view of the relation dictionary
//! (how many results and arguments an operator rule has, their pick messages and
//! conditions, their names, chapters, groups and scopes), and the look-ups that keep a
//! resource file's names apart (`lookForExisting*`, `searchAttdictByFcvName`).
//!
//! The rules of a custom effect (relation id 20000) sit one after another in the
//! dictionary's array of 0x50-byte rules: the first rules are the operator's results, the
//! next its arguments, each of them again for every instance. What tells two rules apart
//! is read as words of the rule at the offsets of [`si_types::dict::RelRule`] (+0x04 the
//! operator's name, +0x20 its effect name, +0x2c and +0x30 the name of a result or an
//! argument, +0x0c and +0x0e the groups). Every index is range-tested with `<=` or `<`
//! the way the original does, which for an index equal to the count reads the rule
//! behind the last one; this module reads the image where the original reads memory and
//! does not make up an answer.
//!
//! Strings are compared with the C library's `strcmp` on the image, in the order the
//! original compares them (a null name faults in the original; here the image reads it
//! as an empty string).

use crate::libc::strcmp;
use si_mem::{Mem, Ptr32};
use si_types::dict::RelDict;

/// The relation id of a custom effect's rule.
const CUSTOM: i32 = 20000;

/// Address of rule `index` (an `int` times 0x50, wrapping like the 32-bit arithmetic).
fn rule(m: &Mem, rd: u32, index: i32) -> u32 {
    m.u32(rd.wrapping_add(8)).wrapping_add((index.wrapping_mul(0x50)) as u32)
}

fn relid(m: &Mem, r: u32) -> i32 {
    m.i32(r)
}

fn nb(m: &Mem, rd: u32) -> i32 {
    m.i32(rd)
}

/// A rule's word at `off`.
fn word(m: &Mem, r: u32, off: u32) -> u32 {
    m.u32(r.wrapping_add(off))
}

/// `strcmp(rule's string at off, s2) == 0`.
fn same(m: &Mem, r: u32, off: u32, s2: u32) -> bool {
    strcmp(m, word(m, r, off), s2) == 0
}

/// [soft 0x0813eb7c SI_reldictGetNbRes]: for the custom-effect rule at `index`, how many
/// rules from there on belong to the same result: the same group (+0x0e) and the same
/// three names (+0x04, +0x20, +0x30). 0 for a null dictionary, an index past the count
/// (`index == count` is allowed and reads the rule behind the last) or a rule of another
/// relation.
pub fn get_nb_res(m: &mut Mem, rd: Ptr32<RelDict>, index: i32) -> i32 {
    let rd = rd.addr();
    if rd == 0 || index > nb(m, rd) {
        return 0;
    }
    let first = rule(m, rd, index);
    if relid(m, first) != CUSTOM {
        return 0;
    }
    let (s_op, s_fx, s_nm) = (word(m, first, 4), word(m, first, 0x20), word(m, first, 0x30));
    let group = m.i16(first.wrapping_add(0xe));
    let mut j = index.wrapping_add(1);
    if j < nb(m, rd) {
        let mut r = rule(m, rd, j);
        if relid(m, r) == CUSTOM && group == m.i16(r.wrapping_add(0xe)) {
            while same(m, r, 4, s_op) && same(m, r, 0x20, s_fx) && same(m, r, 0x30, s_nm) {
                r = r.wrapping_add(0x50);
                j = j.wrapping_add(1);
                if nb(m, rd) <= j || relid(m, r) != CUSTOM || group != m.i16(r.wrapping_add(0xe)) {
                    break;
                }
            }
        }
    }
    j.wrapping_sub(index)
}

/// [soft 0x0813ec80 SI_reldictGetNbArgs]: how many arguments the operator at `index`
/// has: the rules after its results come in runs of one result's length, counted while
/// the relation id, the operator's name (+0x04) and the effect name (+0x20) stay the same.
pub fn get_nb_args(m: &mut Mem, rd: Ptr32<RelDict>, index: i32) -> i32 {
    let rdv = rd.addr();
    if rdv == 0 || nb(m, rdv) < index || relid(m, rule(m, rdv, index)) != CUSTOM {
        return 0;
    }
    let step = get_nb_res(m, rd, index);
    let first = rule(m, rdv, index);
    let mut k = index.wrapping_add(step);
    let mut p = k.wrapping_mul(5); // in units of 0x10 bytes, as the original counts
    let id = relid(m, first);
    let (s_op, s_fx) = (word(m, first, 4), word(m, first, 0x20));
    let mut count = 1;
    while k < nb(m, rdv) {
        let r = m.u32(rdv.wrapping_add(8)).wrapping_add(p.wrapping_mul(0x10) as u32);
        if relid(m, r) != id || !same(m, r, 4, s_op) || !same(m, r, 0x20, s_fx) {
            return count;
        }
        p = p.wrapping_add(step.wrapping_mul(5));
        count += 1;
        k = k.wrapping_add(step);
    }
    count
}

/// [soft 0x0813ed60 SI_reldictGetNbDistinctRes]: how many differently named results
/// (+0x2c changes) the run of rules in one group holds, from the rule at `index`.
pub fn get_nb_distinct_res(m: &mut Mem, rd: Ptr32<RelDict>, index: i32) -> i32 {
    let rd = rd.addr();
    if rd == 0 || index > nb(m, rd) {
        return 0;
    }
    let first = rule(m, rd, index);
    if relid(m, first) != CUSTOM {
        return 0;
    }
    let (s_op, s_fx, s_nm) = (word(m, first, 4), word(m, first, 0x20), word(m, first, 0x30));
    let mut last = word(m, first, 0x2c);
    let group = m.i16(first.wrapping_add(0xe));
    let mut count = 1;
    let mut j = index.wrapping_add(1);
    if nb(m, rd) <= j {
        return 1;
    }
    let mut r = rule(m, rd, j);
    if relid(m, r) != CUSTOM || group != m.i16(r.wrapping_add(0xe)) {
        return 1;
    }
    loop {
        if !same(m, r, 4, s_op) || !same(m, r, 0x20, s_fx) || !same(m, r, 0x30, s_nm) {
            return count;
        }
        if !same(m, r, 0x2c, last) {
            count += 1;
            last = word(m, r, 0x2c);
        }
        r = r.wrapping_add(0x50);
        j = j.wrapping_add(1);
        if nb(m, rd) <= j {
            break;
        }
        if relid(m, r) != CUSTOM || group != m.i16(r.wrapping_add(0xe)) {
            return count;
        }
    }
    count
}

/// [soft 0x0813eeac SI_reldictGetNbDistinctArgs]: how many differently named arguments
/// (+0x30 changes) follow the results of the operator at `index`, stepping one result's
/// length at a time.
pub fn get_nb_distinct_args(m: &mut Mem, rd: Ptr32<RelDict>, index: i32) -> i32 {
    let rdv = rd.addr();
    if rdv == 0 || index > nb(m, rdv) {
        return 0;
    }
    let first = rule(m, rdv, index);
    if relid(m, first) != CUSTOM {
        return 0;
    }
    let (s_op, s_fx) = (word(m, first, 4), word(m, first, 0x20));
    let mut count = 1;
    let mut last = word(m, first, 0x30);
    let step = get_nb_res(m, rd, index);
    let mut k = step.wrapping_add(index);
    let mut p = k.wrapping_mul(5);
    loop {
        if nb(m, rdv) <= k {
            return count;
        }
        let r = m.u32(rdv.wrapping_add(8)).wrapping_add(p.wrapping_mul(0x10) as u32);
        if relid(m, r) != CUSTOM || !same(m, r, 4, s_op) || !same(m, r, 0x20, s_fx) {
            return count;
        }
        if !same(m, r, 0x30, last) {
            count += 1;
            last = word(m, r, 0x30);
        }
        p = p.wrapping_add(step.wrapping_mul(5));
        k = k.wrapping_add(step);
    }
}

/// [soft 0x0813efac SI_reldictGetNextDistinctRes]: the index of the rule after the
/// instances of the result at `index`; −1 for a null dictionary, an index past the count
/// or a rule that is not a custom effect's.
pub fn get_next_distinct_res(m: &mut Mem, rd: Ptr32<RelDict>, index: i32) -> i32 {
    let rdv = rd.addr();
    if rdv == 0 || nb(m, rdv) < index || relid(m, rule(m, rdv, index)) != CUSTOM {
        return -1;
    }
    get_nb_res_instances(m, rd, index).wrapping_add(index)
}

/// [soft 0x0813eff4 SI_reldictGetNextDistinctArg]: the index of the rule after the
/// instances of the argument at `index`: one result's length times the instance count
/// from there; −1 as above.
pub fn get_next_distinct_arg(m: &mut Mem, rd: Ptr32<RelDict>, index: i32) -> i32 {
    let rdv = rd.addr();
    if rdv == 0 || nb(m, rdv) < index || relid(m, rule(m, rdv, index)) != CUSTOM {
        return -1;
    }
    let step = get_nb_res(m, rd, index);
    let inst = get_nb_arg_instances(m, rd, index);
    step.wrapping_mul(inst).wrapping_add(index)
}

/// [soft 0x0813f04c SI_reldictGetNbResInstances]: like [`get_nb_res`] but a rule also has
/// to carry the same +0x2c name as the first, so the count is of one named result's rules.
pub fn get_nb_res_instances(m: &mut Mem, rd: Ptr32<RelDict>, index: i32) -> i32 {
    let rd = rd.addr();
    if rd == 0 || index > nb(m, rd) {
        return 0;
    }
    let first = rule(m, rd, index);
    if relid(m, first) != CUSTOM {
        return 0;
    }
    let (s_op, s_fx, s_res, s_nm) = (word(m, first, 4), word(m, first, 0x20), word(m, first, 0x2c), word(m, first, 0x30));
    let group = m.i16(first.wrapping_add(0xe));
    let mut j = index.wrapping_add(1);
    if j < nb(m, rd) {
        let mut r = rule(m, rd, j);
        if relid(m, r) == CUSTOM && group == m.i16(r.wrapping_add(0xe)) {
            while same(m, r, 4, s_op) && same(m, r, 0x20, s_fx) && same(m, r, 0x2c, s_res) && same(m, r, 0x30, s_nm) {
                r = r.wrapping_add(0x50);
                j = j.wrapping_add(1);
                if nb(m, rd) <= j || relid(m, r) != CUSTOM || group != m.i16(r.wrapping_add(0xe)) {
                    break;
                }
            }
        }
    }
    j.wrapping_sub(index)
}

/// [soft 0x0813f17c SI_reldictGetNbArgInstances]: how many times the argument at `index`
/// repeats, one result's length apart, while relation id, group (+0x0c) and the four
/// names (+0x30, +0x04, +0x20, +0x2c) stay the same.
pub fn get_nb_arg_instances(m: &mut Mem, rd: Ptr32<RelDict>, index: i32) -> i32 {
    let rdv = rd.addr();
    if rdv == 0 || nb(m, rdv) < index || relid(m, rule(m, rdv, index)) != CUSTOM {
        return 0;
    }
    let step = get_nb_res(m, rd, index);
    let first = rule(m, rdv, index);
    let mut k = index.wrapping_add(step);
    let id = relid(m, first);
    let (s_op, s_fx, s_res, s_nm) = (word(m, first, 4), word(m, first, 0x20), word(m, first, 0x2c), word(m, first, 0x30));
    let group = m.i16(first.wrapping_add(0xc));
    let mut count = 1;
    if k < nb(m, rdv) {
        let mut p = k.wrapping_mul(5);
        let at = |m: &Mem, p: i32| m.u32(rdv.wrapping_add(8)).wrapping_add(p.wrapping_mul(0x10) as u32);
        if relid(m, at(m, p)) == id && group == m.i16(at(m, p).wrapping_add(0xc)) {
            loop {
                let r = at(m, p);
                if !same(m, r, 0x30, s_nm) || !same(m, r, 4, s_op) || !same(m, r, 0x20, s_fx) || !same(m, r, 0x2c, s_res) {
                    break;
                }
                p = p.wrapping_add(step.wrapping_mul(5));
                k = k.wrapping_add(step);
                count += 1;
                if nb(m, rdv) <= k {
                    return count;
                }
                let r = at(m, p);
                if relid(m, r) != id || group != m.i16(r.wrapping_add(0xc)) {
                    return count;
                }
            }
        }
    }
    count
}

/// The scan of the two result routines below: the first non-zero word at `field` among
/// the rules of the same operator, group and names, from `index` on; 0 when none.
fn first_res_word(m: &Mem, rd: u32, index: i32, field: u32) -> u32 {
    if rd == 0 || index > nb(m, rd) {
        return 0;
    }
    let mut off = index.wrapping_mul(0x50);
    let mut index = index;
    let first = rule(m, rd, index);
    if relid(m, first) != CUSTOM {
        return 0;
    }
    let (s_op, s_fx, s_res, s_nm) = (word(m, first, 4), word(m, first, 0x20), word(m, first, 0x2c), word(m, first, 0x30));
    let group = m.i16(first.wrapping_add(0xe));
    loop {
        let rules = m.u32(rd.wrapping_add(8));
        let v = m.u32(rules.wrapping_add(off as u32).wrapping_add(field));
        if v != 0 {
            return v;
        }
        off = off.wrapping_add(0x50);
        index = index.wrapping_add(1);
        let r = rules.wrapping_add(off as u32);
        let more = index < nb(m, rd)
            && relid(m, r) == CUSTOM
            && m.i16(r.wrapping_add(0xe)) == group
            && same(m, r, 4, s_op)
            && same(m, r, 0x20, s_fx)
            && same(m, r, 0x2c, s_res)
            && same(m, r, 0x30, s_nm);
        if !more {
            return 0;
        }
    }
}

/// The scan of the two argument routines below: as [`first_res_word`] but stepping one
/// result's length, comparing the argument group (+0x0c), and the names in the order
/// +0x04, +0x20, +0x30, +0x2c.
fn first_arg_word(m: &mut Mem, rd: Ptr32<RelDict>, index: i32, field: u32) -> u32 {
    let rdv = rd.addr();
    if rdv == 0 || index > nb(m, rdv) {
        return 0;
    }
    let mut p6 = index.wrapping_mul(5);
    let first = rule(m, rdv, index);
    if relid(m, first) != CUSTOM {
        return 0;
    }
    let step = get_nb_res(m, rd, index);
    let id = relid(m, first);
    let (s_op, s_fx, s_res, s_nm) = (word(m, first, 4), word(m, first, 0x20), word(m, first, 0x2c), word(m, first, 0x30));
    let group = m.i16(first.wrapping_add(0xc));
    let mut k = index;
    let mut lp = p6;
    loop {
        let rules = m.u32(rdv.wrapping_add(8));
        let v = m.u32(rules.wrapping_add(lp.wrapping_mul(0x10) as u32).wrapping_add(field));
        if v != 0 {
            return v;
        }
        lp = lp.wrapping_add(step.wrapping_mul(5));
        k = k.wrapping_add(step);
        p6 = p6.wrapping_add(step.wrapping_mul(5));
        let r = rules.wrapping_add(p6.wrapping_mul(0x10) as u32);
        let more = k < nb(m, rdv)
            && relid(m, r) == id
            && m.i16(r.wrapping_add(0xc)) == group
            && same(m, r, 4, s_op)
            && same(m, r, 0x20, s_fx)
            && same(m, r, 0x30, s_nm)
            && same(m, r, 0x2c, s_res);
        if !more {
            return 0;
        }
    }
}

/// [soft 0x0813f2f8 SI_reldictGetResPickMessage]: the pick message (+0x34) of the first
/// rule of the result's run that has one, 0 for none.
pub fn get_res_pick_message(m: &mut Mem, rd: Ptr32<RelDict>, index: i32) -> u32 {
    first_res_word(m, rd.addr(), index, 0x34)
}

/// [soft 0x0813f404 SI_reldictGetArgPickMessage]: the pick message (+0x38) of the first
/// instance of the argument that has one, 0 for none.
pub fn get_arg_pick_message(m: &mut Mem, rd: Ptr32<RelDict>, index: i32) -> u32 {
    first_arg_word(m, rd, index, 0x38)
}

/// [soft 0x0813f550 SI_reldictGetResCond]: the condition (+0x24) of the first rule of the
/// result's run that has one.
pub fn get_res_cond(m: &mut Mem, rd: Ptr32<RelDict>, index: i32) -> u32 {
    first_res_word(m, rd.addr(), index, 0x24)
}

/// [soft 0x0813f654 SI_reldictGetArgCond]: the condition (+0x28) of the first instance of
/// the argument that has one.
pub fn get_arg_cond(m: &mut Mem, rd: Ptr32<RelDict>, index: i32) -> u32 {
    first_arg_word(m, rd, index, 0x28)
}

/// The shared head of the simple accessors: −1 or 0 (`none`) for a null dictionary or an
/// index past the count, else the word of the rule `index`.
fn simple(m: &Mem, rd: Ptr32<RelDict>, index: i32, none: i32, which: i16, off_zero: u32, off_other: u32, short: bool) -> i32 {
    let rdv = rd.addr();
    if rdv == 0 || nb(m, rdv) < index {
        return none;
    }
    let r = rule(m, rdv, index);
    let off = if which == 0 { off_zero } else { off_other };
    if short {
        m.i16(r.wrapping_add(off)) as i32
    } else {
        m.i32(r.wrapping_add(off))
    }
}

/// [soft 0x0813f7a0 SI_reldictGetChap]: the chapter at the destination end (`which` 0,
/// +0x0a) or the source (+0x08) of rule `index`; −1 for none.
pub fn get_chap(m: &mut Mem, rd: Ptr32<RelDict>, index: i32, which: i16) -> i32 {
    simple(m, rd, index, -1, which, 0xa, 0x8, true)
}

/// [soft 0x0813f7e8 SI_reldictGetGrp]: the group at the destination end (+0x0e) or the
/// source (+0x0c); −1 for none.
pub fn get_grp(m: &mut Mem, rd: Ptr32<RelDict>, index: i32, which: i16) -> i32 {
    simple(m, rd, index, -1, which, 0xe, 0xc, true)
}

/// [soft 0x0813f82c reldictOpArgInstanceGetGrp]: the group (+0x0e) of instance `n` of the
/// argument at `index`, `n` result-lengths on; −1 past the count.
pub fn op_arg_instance_get_grp(m: &mut Mem, rd: Ptr32<RelDict>, index: i32, n: i32) -> i32 {
    let rdv = rd.addr();
    if rdv != 0 && index <= nb(m, rdv) {
        let step = get_nb_res(m, rd, index);
        let at = n.wrapping_mul(step).wrapping_add(index);
        if at <= nb(m, rdv) {
            return m.i16(rule(m, rdv, at).wrapping_add(0xe)) as i32;
        }
    }
    -1
}

/// [soft 0x0813f880 reldictOpResInstanceGetGrp]: the group (+0x0c) of the rule `n` after
/// `index`; −1 past the count.
pub fn op_res_instance_get_grp(m: &mut Mem, rd: Ptr32<RelDict>, index: i32, n: i32) -> i32 {
    let rdv = rd.addr();
    if rdv == 0 || nb(m, rdv) < index || nb(m, rdv) < index.wrapping_add(n) {
        return -1;
    }
    m.i16(rule(m, rdv, index.wrapping_add(n)).wrapping_add(0xc)) as i32
}

/// [soft 0x0813f8bc SI_reldictOpParamInstanceGetGrp]: the argument's group for `which` 0,
/// else the result's.
pub fn op_param_instance_get_grp(m: &mut Mem, rd: Ptr32<RelDict>, index: i32, n: i32, which: i16) -> i32 {
    let rdv = rd.addr();
    if rdv == 0 || nb(m, rdv) < index {
        return -1;
    }
    if which == 0 {
        op_arg_instance_get_grp(m, rd, index, n)
    } else {
        op_res_instance_get_grp(m, rd, index, n)
    }
}

/// [soft 0x0813f904 SI_reldictGetCond]: the argument condition (+0x28, `which` 0) or the
/// result condition (+0x24) of rule `index`; 0 for none.
pub fn get_cond(m: &mut Mem, rd: Ptr32<RelDict>, index: i32, which: i16) -> u32 {
    simple(m, rd, index, 0, which, 0x28, 0x24, false) as u32
}

/// [soft 0x0813f948 SI_reldictGetScope]: the argument scope (+0x44, `which` 0) or the
/// result scope (+0x42); −1 for none.
pub fn get_scope(m: &mut Mem, rd: Ptr32<RelDict>, index: i32, which: i16) -> i32 {
    simple(m, rd, index, -1, which, 0x44, 0x42, true)
}

/// [soft 0x0813f98c SI_reldictGetName]: the argument name (+0x30, `which` 0) or the result
/// name (+0x2c); null for none.
pub fn get_name(m: &mut Mem, rd: Ptr32<RelDict>, index: i32, which: i16) -> u32 {
    simple(m, rd, index, 0, which, 0x30, 0x2c, false) as u32
}

/// SI_reldictCusOpDump (0x0813eae0): an empty function in this build (not in the emulator check, so not cited).
pub fn cus_op_dump(_m: &mut Mem, _rd: Ptr32<RelDict>) {}

/// [soft 0x0813fa74 lookForExistingResName]: whether an operator definition (`SI_OperDef`:
/// +0x18 its results, 0x20 bytes each, +0x1c their count as a short) already has a result
/// of that name. A result of the same name only counts when its group (+0x18 of the
/// result) differs from `group` or its type (+0x08) from `ty`, or, when `all` is 0,
/// its other type (+0x0a) equals `ty2`; `all` 0 also leaves the last result out of the scan.
pub fn look_for_existing_res_name(m: &Mem, def: u32, name: u32, ty: i32, ty2: i32, group: i16, all: i16) -> i16 {
    if def == 0 || m.u32(def.wrapping_add(0x18)) == 0 || name == 0 {
        return 0;
    }
    let count = m.i16(def.wrapping_add(0x1c)) as i32;
    let n = if all == 0 { count - 1 } else { count };
    let mut k = 0i32;
    while k < n {
        let o = (k.wrapping_mul(0x20)) as u32;
        let arr = m.u32(def.wrapping_add(0x18));
        let s2 = m.u32(arr.wrapping_add(4).wrapping_add(o));
        if s2 != 0 && strcmp(m, name, s2) == 0 {
            let arr = m.u32(def.wrapping_add(0x18));
            if group != m.i16(arr.wrapping_add(0x18).wrapping_add(o))
                || m.i16(arr.wrapping_add(8).wrapping_add(o)) as i32 != ty
                || (all == 0 && m.i16(arr.wrapping_add(10).wrapping_add(o)) as i32 == ty2)
            {
                return 1;
            }
        }
        k += 1;
    }
    0
}

/// [soft 0x0813fb48 lookForExistingArgName]: as above for the arguments (+0x20, 0x28
/// bytes each, +0x24 their count as a short, the last left out): a same-named argument
/// counts when its +0x08 differs from `ty` or its +0x0a equals `ty2`.
pub fn look_for_existing_arg_name(m: &Mem, def: u32, name: u32, ty: i32, ty2: i32) -> i16 {
    if def == 0 || m.u32(def.wrapping_add(0x20)) == 0 || name == 0 || (m.i16(def.wrapping_add(0x24)) as i32) - 1 <= 0 {
        return 0;
    }
    let mut o = 0u32;
    let mut k = 0i32;
    loop {
        let arr = m.u32(def.wrapping_add(0x20));
        let s2 = m.u32(arr.wrapping_add(4).wrapping_add(o));
        if s2 != 0
            && strcmp(m, name, s2) == 0
            && (m.i16(arr.wrapping_add(8).wrapping_add(o)) as i32 != ty || m.i16(arr.wrapping_add(10).wrapping_add(o)) as i32 == ty2)
        {
            return 1;
        }
        o = o.wrapping_add(0x28);
        k += 1;
        if k >= (m.i16(def.wrapping_add(0x24)) as i32) - 1 {
            return 0;
        }
    }
}

/// The attribute `idx` of the dictionary `atd` (100 bytes each), as an address.
fn attribute(m: &Mem, atd: u32, idx: i32) -> u32 {
    m.u32(atd.wrapping_add(8)).wrapping_add(idx.wrapping_mul(100) as u32)
}

/// [soft 0x0813fbe0 lookForExistingOatName]: whether an attribute of the group already
/// has this name (+0x20 of the attribute, compared in the group's order).
pub fn look_for_existing_oat_name(m: &Mem, atd: u32, group: u32, name: u32) -> i16 {
    if atd == 0 || group == 0 || name == 0 {
        return 0;
    }
    let mut k = 0i32;
    while k < m.i32(group.wrapping_add(0xc)) {
        let idx = m.i32(m.u32(group.wrapping_add(0x10)).wrapping_add(k.wrapping_mul(4) as u32));
        let s2 = m.u32(attribute(m, atd, idx).wrapping_add(0x20));
        if s2 != 0 && strcmp(m, name, s2) == 0 {
            return 1;
        }
        k += 1;
    }
    0
}

/// [soft 0x0813fc58 lookForExistingFcurveName]: whether an attribute of the group is named
/// like this, or (the answer is then still 1 at the end) one of the attributes' component
/// names (+0x5c, `nb_values` of them at +0x56) is.
pub fn look_for_existing_fcurve_name(m: &Mem, atd: u32, group: u32, name: u32) -> i16 {
    if atd == 0 || group == 0 || name == 0 {
        return 0;
    }
    let mut found = 0i16;
    let mut k = 0i32;
    while k < m.i32(group.wrapping_add(0xc)) {
        let idx = m.i32(m.u32(group.wrapping_add(0x10)).wrapping_add(k.wrapping_mul(4) as u32));
        let a = attribute(m, atd, idx);
        let s2 = m.u32(a.wrapping_add(0x20));
        if s2 != 0 && strcmp(m, name, s2) == 0 {
            return 1;
        }
        if found == 0 {
            let mut j = 0i32;
            loop {
                let a = attribute(m, atd, m.i32(m.u32(group.wrapping_add(0x10)).wrapping_add(k.wrapping_mul(4) as u32)));
                if (m.i16(a.wrapping_add(0x56)) as i32) <= j {
                    break;
                }
                let comp = m.u32(m.u32(a.wrapping_add(0x5c)).wrapping_add(j.wrapping_mul(4) as u32));
                if strcmp(m, name, comp) == 0 {
                    found = 1;
                    break;
                }
                j += 1;
            }
        }
        k += 1;
    }
    found
}

/// [soft 0x0813fd20 searchAttdictByFcvName]: [`look_for_existing_fcurve_name`] on the
/// group, else on each of its sub-groups (0x28 bytes each, +0x20 their count, +0x24 the
/// array) in turn, down the tree, until one answers.
pub fn search_attdict_by_fcv_name(m: &Mem, atd: u32, group: u32, name: u32) -> i32 {
    let mut r = look_for_existing_fcurve_name(m, atd, group, name);
    if r == 0 {
        let mut p = 0u32;
        let mut k = 0i32;
        while k < m.i32(group.wrapping_add(0x20)) {
            r = search_attdict_by_fcv_name(m, atd, m.u32(group.wrapping_add(0x24)).wrapping_add(p), name) as i16;
            if r != 0 {
                break;
            }
            p = p.wrapping_add(0x28);
            k += 1;
        }
    }
    r as i32
}
