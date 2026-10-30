// SPDX-License-Identifier: MIT
//! `custom_mn.c` (the custom-effect menu and scene-update sources) and `custom_util.c`'s dialog data allocator: the
//! dirty-marking subtree of the scene transfer, the pre-scene-transfer and scene-update walkers over the five effect
//! status records, `cusTmpDir`, and `si_cusDialogDsoDataCreate` (unit cus1, finding 0041, 2026-10-29).
//!
//! The menus, dialogs, freeze/activate/edit-parameter commands and the signal handlers of the same source are UI and
//! not ported.

use crate::custom_util::{
    CUS_ACTOR, CUS_MATTER, CUS_MODEL, CUS_MOTION, CUS_TOOLS, SI_ATD, SI_ATTDICT_ATTRIBUTE_IS_ANIMATABLE, SI_ATTDICT_ATT_GRP_GET_ATTRIBUTE,
    SI_ATTDICT_ATT_GRP_GET_NB_ATTRIBUTES, SI_ATTDICT_GET_ATT_GRP_BY_PATH, SI_CUS_FCURVE_DIRTY, SI_PROC_DATA,
};
use si_mem::{clib, Mem, Outside, Routine};

pub const SI_ELEMDICT_FIND_RELATED_ELEM_M: Routine = Routine::new("SI_elemdictFindRelatedElem", 0x0813_508c);
pub const SI_ELEMDICT_FIND_NEXT_RELATED_ELEM: Routine = Routine::new("SI_elemdictFindNextRelatedElem", 0x0813_5154);
pub const SI_ELEMDICT_SET_REL_TO_SERIALID: Routine = Routine::new("SI_elemdictSetRelToSerialid", 0x0813_354c);
pub const SI_ELEMDICT_SET_CUR_RELATION_INDEX: Routine = Routine::new("SI_elemdictSetCurRelationIndex", 0x0813_825c);
pub const SI_ELEMDICT_GET_CUR_RELATION_INDEX: Routine = Routine::new("SI_elemdictGetCurRelationIndex", 0x0813_8224);
pub const SI_ELEMDICT_GET_CUR_RELATION_M: Routine = Routine::new("SI_elemdictGetCurRelation", 0x0813_4db0);
pub const SI_ELEMDICT_FIND_ELEM_BY_DATA_M: Routine = Routine::new("SI_elemdictFindElemByData", 0x0813_4364);
pub const SI_ATTDICT_GET_INFO_FROM_FCURVE: Routine = Routine::new("SI_attdictGetInfoFromFcurve", 0x0812_0264);
pub const SI_CUS_FETCH_CUSTOM_VALUE_M: Routine = Routine::new("SI_cusFetchCustomValue", 0x0853_0e08);
pub const SI_CUSTOM_VALUE_CALLBACK_CALL_ALL_M: Routine = Routine::new("SI_customValueCallbackCallAll", 0x0852_980c);
pub const SI_CUS_TRANSFER_TO_DEFINITION_M: Routine = Routine::new("SI_cusTransferToDefinition", 0x0854_9328);
pub const SI_CUS_UPDATE_EFFECT_M: Routine = Routine::new("si_cusUpdateEffect", 0x0852_df3c);
pub const SI_CUS_PROCESS_BASED_EFFECT: Routine = Routine::new("si_cusProcessBasedEffect", 0x0853_b11c);
pub const SI_TEMPORARY_DIRECTORY: Routine = Routine::new("SI_temporaryDirectory", 0x0841_d480);

/// `SI_fcurveDirty`, `SI_cusValDirty`, `currentEffect`, the static `depth` of `cusExpressSetDirty`, `SI_cid.MODELS`,
/// `SI_cid.ANIMATION`, `SI_elemDicts`.
pub const SI_FCURVE_DIRTY: u32 = 0x086e_49dc;
pub const SI_CUS_VAL_DIRTY: u32 = 0x086e_49e4;
pub const SI_CUS_VAL_NUM_CALLBACK_M: u32 = 0x086e_49e8;
pub const CURRENT_EFFECT: u32 = 0x086e_0aa0;
pub const EXPRESS_DEPTH: u32 = 0x086e_1d00;
pub const SI_CID_MODELS_M: u32 = 0x0878_1680;
pub const SI_CID_ANIMATION: u32 = 0x0878_1660;
pub const SI_ELEM_DICTS_M: u32 = 0x0867_931c;
/// The string `cusTmpDir` answers for a name that holds a directory separator (data, not read here).
pub const CUS_TMP_DIR_TEXT: u32 = 0x086e_21c1;

/// The dictionary of the current process: `SI_elemDicts + SI_procData[0] * 0x70`.
fn proc_dict(m: &Mem) -> u32 {
    let pd = m.u32(SI_PROC_DATA);
    m.u32(pd).wrapping_mul(8).wrapping_sub(m.u32(pd)).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS_M))
}

/// [soft 0x085400a8 cusTmpDir]: a name that holds a `/` or `\` before its end (it must not start with one) is answered
/// by the fixed text at 0x086e21c1; anything else (null, empty, no separator) by `SI_temporaryDirectory()`.
/// The original's scan stops at the first NUL, `/` or `\`; a hit that is not a NUL gives the text.
pub fn cus_tmp_dir(m: &mut Mem, o: &mut dyn Outside, name: u32) -> u32 {
    let mut p = name;
    while p != 0 {
        let c = m.u8(p);
        if c == 0 || c == b'/' || c == b'\\' {
            if c != 0 {
                return CUS_TMP_DIR_TEXT;
            }
            break;
        }
        p = p.wrapping_add(1);
    }
    o.call(m, SI_TEMPORARY_DIRECTORY, &[]).eax
}

/// [soft 0x0853e6ec cusEffectNotifyForCusValChanges]: for each effect of the chain (+0xfc) and each of its values (list at
/// +0x64, next +0x44) whose flag word (+0x48) has bit 0: the word is cleared to 0 (whole short), the value-callback walker
/// is called for it with 0xd; an answer of 0x5f or 0x7c ends that effect's values.
pub fn cus_effect_notify_for_cus_val_changes(m: &mut Mem, o: &mut dyn Outside, first: u32) {
    let mut e = first;
    while e != 0 {
        let mut v = m.u32(e.wrapping_add(0x64));
        while v != 0 {
            if m.u8(v.wrapping_add(0x48)) & 1 != 0 {
                m.set_u16(v.wrapping_add(0x48), 0);
                let r = o.call(m, SI_CUSTOM_VALUE_CALLBACK_CALL_ALL_M, &[e, v, 0xd]).eax;
                if r == 0x5f || r == 0x7c {
                    break;
                }
            }
            v = m.u32(v.wrapping_add(0x44));
        }
        e = m.u32(e.wrapping_add(0xfc));
    }
}

/// [soft 0x0853de64 cusStatusPreSceneTransfer]: over the effects of the status record's list (record +0x14c, list head at
/// +0xfc, chain +0xfc): with a non-zero `notify` first the value-change notifier on the list. Then for each effect that has
/// an icon (+0xc) and whose state (+0x100) is 1: the class path of the icon's effect class, the attribute group of that
/// path and, for each animatable attribute, the value of the effect (list +0x64) of the same name has its kind short (+0xc)
/// set to 0; the path is freed. Then for each entry of the effect's record list at +0xf8 (chain +0x30) whose kind short is
/// 0x18, 0x19, 0x37, 0x42 or 0x46 the model the entry's target (+0x28; its model at +0x1c) is transferred to its definition,
/// with 0 when the target's kind short is 0x20 and 1 otherwise.
pub fn cus_status_pre_scene_transfer(m: &mut Mem, o: &mut dyn Outside, status: u32, notify: u16) {
    let mut e = m.u32(m.u32(status.wrapping_add(0x14c)).wrapping_add(0xfc));
    if notify != 0 {
        cus_effect_notify_for_cus_val_changes(m, o, e);
    }
    while e != 0 {
        if m.u32(e.wrapping_add(0xc)) != 0 && m.u32(e.wrapping_add(0x100)) == 1 {
            let cell = m.local(4);
            m.set_u32(cell, 0);
            crate::custom_fcv::si_cus_effect_class_get_pathname(m, o, m.u32(e.wrapping_add(0xc)), cell);
            let path = m.u32(cell);
            let grp = o.call(m, SI_ATTDICT_GET_ATT_GRP_BY_PATH, &[SI_ATD, path]).eax;
            if grp != 0 {
                let mut i: i32 = 0;
                while i < o.call(m, SI_ATTDICT_ATT_GRP_GET_NB_ATTRIBUTES, &[grp]).eax as i32 {
                    let att = o.call(m, SI_ATTDICT_ATT_GRP_GET_ATTRIBUTE, &[SI_ATD, grp, i as u32]).eax;
                    let animatable = o.call(m, SI_ATTDICT_ATTRIBUTE_IS_ANIMATABLE, &[att]).eax as u16;
                    i += 1;
                    if animatable != 0 {
                        let mut v = m.u32(e.wrapping_add(0x64));
                        while v != 0 {
                            let (a, b) = (m.u32(v), m.u32(att.wrapping_add(0x20)));
                            if a != 0 && b != 0 && clib::strcmp(m, a, b) == 0 {
                                m.set_u16(v.wrapping_add(0xc), 0);
                            }
                            v = m.u32(v.wrapping_add(0x44));
                        }
                    }
                }
            }
            let path = m.u32(cell);
            if path != 0 {
                m.free(path);
                m.set_u32(cell, 0);
            }
            m.release(4);
        }
        let mut r = m.u32(e.wrapping_add(0xf8));
        while r != 0 {
            let kind = (m.u16(r) as i32).wrapping_sub(24) as i16 as i32 as u32;
            if kind <= 0x2e && matches!(m.u16(r), 24 | 25 | 55 | 66 | 70) {
                let target = m.u32(r.wrapping_add(0x28));
                let model = m.u32(target.wrapping_add(0x1c));
                let flag = (m.u16(target) != 0x20) as u32;
                o.call(m, SI_CUS_TRANSFER_TO_DEFINITION_M, &[model, flag]);
            }
            r = m.u32(r.wrapping_add(0x30));
        }
        e = m.u32(e.wrapping_add(0xfc));
    }
}

/// [soft 0x0853e3e8 cusExpressSetDirty]: marks as dirty every custom value that depends, through the relations of kind
/// 0x4e2a, on the animated item `elem` (its chapter `chap` and index `idx` in the dictionary `dict`), for the f-curve
/// kind `ftype` (-1: any) and, for kind 0x236, the value `name`. Recurses (to depth 17) through the dependents; a value of a
/// model of kind 0xd gets flag bits 3 (+0x48) when bit 1 was clear. Answers 1 when any value was newly marked, else 0.
/// The original reads through a null relation record (+0x18 of the serial relation) when a dependent's relation has no
/// record and it recurses; the Rust reads the image there as well.
pub fn cus_express_set_dirty(m: &mut Mem, o: &mut dyn Outside, dict: u32, chap: u32, idx: u32, elem: u32, ftype: u32, name: u32) -> u16 {
    let mut result: u16 = 0;
    if dict == 0 || elem == 0 || (ftype == 0x236 && name == 0) || (m.u32(EXPRESS_DEPTH) as i32) > 0x10 {
        return 0;
    }
    m.set_u32(EXPRESS_DEPTH, m.u32(EXPRESS_DEPTH).wrapping_add(1));
    let out = m.local(8); // chapter, index
    let (out_chap, out_idx) = (out, out.wrapping_add(4));
    let local_fc = m.local(4);
    let mut kind: i32 = 0;
    loop {
        let mut last: u32 = 0xffff_ffff;
        let mut count: i32 = 0;
        let next_kind = kind + 1;
        if kind != 4 {
            let mut found = o.call(m, SI_ELEMDICT_FIND_RELATED_ELEM_M, &[proc_dict(m), elem, 0, kind as u32, 0x4e2a, last, out_chap, out_idx]).eax;
            while found != 0 {
                let mut hit = false;
                let cur_idx = o.call(m, SI_ELEMDICT_GET_CUR_RELATION_INDEX, &[elem, 0, kind as u32]).eax;
                let tab = m.u32(m.u32(elem.wrapping_add(0x6c)).wrapping_add((kind as u32).wrapping_mul(0x18)).wrapping_add(0xc));
                let group = m.u16(cur_idx.wrapping_shl(5).wrapping_add(tab).wrapping_sub(0xe)) as i16 as i32;
                let rel = o.call(m, SI_ELEMDICT_GET_CUR_RELATION_M, &[dict, elem, 0, kind as u32]).eax;
                let serial = m.u32(rel.wrapping_add(0xc));
                let sr = o.call(m, SI_ELEMDICT_SET_REL_TO_SERIALID, &[dict, found, 0x4e2a, serial, 1]).eax;
                let rec = m.u32(sr.wrapping_add(0x18));
                if last == serial {
                    count += 1;
                } else {
                    last = serial;
                    count = 1;
                }
                if ftype == 0xffff_ffff {
                    hit = true;
                } else {
                    let ci = o.call(m, SI_ELEMDICT_GET_CUR_RELATION_INDEX, &[found, 1, group as u32]).eax;
                    let mut n = 0i32;
                    let ftab = m.u32(m.u32(found.wrapping_add(100)).wrapping_add((group as u32).wrapping_mul(0x18)).wrapping_add(0xc));
                    let mut p = ci.wrapping_mul(0x20).wrapping_add(ftab);
                    while p != 0 && m.u32(p.wrapping_add(0xc)) == serial && n < count {
                        if m.u16(p.wrapping_add(0x10)) as i16 as i32 == chap as i32 && m.u32(p.wrapping_add(4)) == idx && m.u16(p.wrapping_add(0x12)) as i16 as i32 == kind {
                            n += 1;
                        }
                        p = p.wrapping_add(0x20);
                    }
                    if n == count && p != 0 {
                        let r2 = m.u32(p.wrapping_sub(8));
                        if r2 != 0 && m.u32(r2) == ftype && (ftype != 0x236 || clib::strcmp(m, m.u32(r2.wrapping_add(0x10)), name) == 0) {
                            hit = true;
                        }
                    }
                }
                if hit {
                    let mut skip = false;
                    let found_chap = m.u32(out_chap);
                    if found_chap == m.u32(SI_CID_MODELS_M) {
                        let mdl = m.u32(found.wrapping_add(4));
                        if mdl != 0 && m.u32(mdl.wrapping_add(0x2ec)) == 0xd && rec != 0 && m.u32(rec) == 0x236 {
                            let v = o.call(m, SI_CUS_FETCH_CUSTOM_VALUE_M, &[mdl, m.u32(rec.wrapping_add(0x10)), local_fc]).eax;
                            if v != 0 {
                                let fl = m.u16(v.wrapping_add(0x48));
                                if fl & 2 == 0 {
                                    m.set_u16(v.wrapping_add(0x48), fl | 3);
                                    result = 1;
                                } else {
                                    skip = true;
                                }
                            }
                        }
                    }
                    if (elem != found || ftype != 0xffff_ffff) && !skip {
                        let (c2, i2) = (m.u32(out_chap), m.u32(out_idx));
                        let (t2, n2) = (m.u32(rec), m.u32(rec.wrapping_add(0x10)));
                        if cus_express_set_dirty(m, o, dict, c2, i2, found, t2, n2) != 0 {
                            result = 1;
                        }
                    }
                }
                o.call(m, SI_ELEMDICT_SET_CUR_RELATION_INDEX, &[elem, 0, kind as u32, cur_idx]);
                found = o.call(m, SI_ELEMDICT_FIND_NEXT_RELATED_ELEM, &[proc_dict(m), elem, 0, kind as u32, 0x4e2a, 0xffff_ffff, out_chap, out_idx]).eax;
            }
        }
        kind = next_kind;
        if kind > 5 {
            break;
        }
    }
    m.release(12);
    m.set_u32(EXPRESS_DEPTH, m.u32(EXPRESS_DEPTH).wrapping_sub(1));
    result
}

/// [soft 0x0853e248 cusFcurveCusValSetDirty]: for each effect of the chain starting at `head` (+0xfc) while the f-curve
/// dirty count (`SI_cusFcurveDirty`) or the value dirty count (`SI_cusValDirty`) is above 0, each of its values (list +0x64,
/// next +0x44) that is flagged (bit 2 of the word at +0x48) or whose f-curve (+0x2c) has bit 0 of the byte at +0x62: with
/// bit 1 of the flag clear the word gets bits 3, the effect's model entry is looked up in the dictionary by data (+0xc) and
/// `cusExpressSetDirty` is run for it with the value's name (answer 1). Then a flagged f-curve is un-flagged and counted off
/// the f-curve count, a value with bit 2 off the value count, and the f-curve's word at +0x62 cleared. Answers 1 when any
/// dependency was marked.
pub fn cus_fcurve_cus_val_set_dirty(m: &mut Mem, o: &mut dyn Outside, head: u32) -> u16 {
    let mut result: u16 = 0;
    let mut eff = head;
    while eff != 0 {
        if !((m.u32(SI_CUS_FCURVE_DIRTY) as i32) > 0 || (m.u32(SI_CUS_VAL_DIRTY) as i32) > 0) {
            break;
        }
        let mut v = m.u32(eff.wrapping_add(0x64));
        while v != 0 {
            let fc = m.u32(v.wrapping_add(0x2c));
            let flags = m.u16(v.wrapping_add(0x48));
            let process = flags & 4 != 0 || (fc != 0 && m.u8(fc.wrapping_add(0x62)) & 1 != 0);
            if process {
                if flags & 2 == 0 {
                    m.set_u16(v.wrapping_add(0x48), flags | 3);
                    result = 1;
                    let dict = proc_dict(m);
                    let cid = m.u32(SI_CID_MODELS_M);
                    let ei = o.call(m, SI_ELEMDICT_FIND_ELEM_BY_DATA_M, &[dict, cid, m.u32(eff.wrapping_add(0xc))]).eax;
                    let pd = m.u32(SI_PROC_DATA);
                    let kd = m.u32(m.u32(SI_ELEM_DICTS_M).wrapping_add(m.u32(pd).wrapping_mul(8).wrapping_sub(m.u32(pd)).wrapping_shl(4)).wrapping_add(4));
                    let elems = m.u32(kd.wrapping_add(cid.wrapping_shl(6)).wrapping_add(8));
                    let elem = elems.wrapping_add(ei.wrapping_mul(0x84));
                    let nm = m.u32(v);
                    // the original ignores the answer here (the result is already 1)
                    let _ = cus_express_set_dirty(m, o, proc_dict(m), cid, ei, elem, 0x236, nm);
                }
                if fc != 0 && m.u8(fc.wrapping_add(0x62)) & 1 != 0 {
                    m.set_u32(SI_CUS_FCURVE_DIRTY, m.u32(SI_CUS_FCURVE_DIRTY).wrapping_sub(1));
                }
                if m.u8(v.wrapping_add(0x48)) & 4 != 0 {
                    m.set_u32(SI_CUS_VAL_DIRTY, m.u32(SI_CUS_VAL_DIRTY).wrapping_sub(1));
                }
                if fc != 0 {
                    m.set_u16(fc.wrapping_add(0x62), 0);
                }
            }
            v = m.u32(v.wrapping_add(0x44));
            if v == 0 {
                break;
            }
            if (m.u32(SI_CUS_FCURVE_DIRTY) as i32) > 0 || (m.u32(SI_CUS_VAL_DIRTY) as i32) > 0 {
                continue;
            }
            break;
        }
        eff = m.u32(eff.wrapping_add(0xfc));
    }
    result
}

/// [soft 0x0853dfec cusValueMarkAsDirty]: the dirty-flag pass before a scene transfer. With no value callback registered
/// (`SI_cusValNumCallback` 0) the three counters (`SI_fcurveDirty`, `SI_cusFcurveDirty`, `SI_cusValDirty`) are cleared and
/// the answer is 0; with all three 0 the answer is 0. Otherwise each animation element of the current dictionary (chapter
/// `SI_cid.ANIMATION`) that has a curve with bit 0 at +0x62 (and an f-curve record at +0x54, state short +0x48 0) is looked up
/// by its curve's attribute info, `cusExpressSetDirty` is run for what depends on it, the curve's flag word is cleared and
/// `SI_fcurveDirty` counted down, while that count stays above 0. Then `cusFcurveCusValSetDirty` runs on the model, motion,
/// actor, matter and tools effect lists; the counters are cleared. 1 when anything was marked.
pub fn cus_value_mark_as_dirty(m: &mut Mem, o: &mut dyn Outside) -> u16 {
    let mut result: u16 = 0;
    if m.u32(SI_CUS_VAL_NUM_CALLBACK_M) == 0 {
        m.set_u32(SI_FCURVE_DIRTY, 0);
        m.set_u32(SI_CUS_FCURVE_DIRTY, 0);
        m.set_u32(SI_CUS_VAL_DIRTY, 0);
        return 0;
    }
    let fd0 = m.u32(SI_FCURVE_DIRTY) as i32;
    if fd0 == 0 && m.u32(SI_CUS_FCURVE_DIRTY) == 0 && m.u32(SI_CUS_VAL_DIRTY) == 0 {
        return 0;
    }
    let pd = m.u32(SI_PROC_DATA);
    let kd = m.u32(m.u32(SI_ELEM_DICTS_M).wrapping_add(m.u32(pd).wrapping_mul(8).wrapping_sub(m.u32(pd)).wrapping_shl(4)).wrapping_add(4))
        .wrapping_add(m.u32(SI_CID_ANIMATION).wrapping_shl(6));
    let count = m.u32(kd.wrapping_add(4)) as i32;
    let mut e = m.u32(kd.wrapping_add(8));
    let outs = m.local(16);
    let (o1, o2, o3, o4) = (outs, outs.wrapping_add(4), outs.wrapping_add(8), outs.wrapping_add(12));
    let found = m.local(8);
    let (f_chap, f_idx) = (found, found.wrapping_add(4));
    let mut i = 0i32;
    let mut cur = fd0;
    while i < count && cur > 0 {
        let fc = m.u32(e.wrapping_add(4));
        if m.u16(e.wrapping_add(0x48)) == 0 && m.u32(e.wrapping_add(0x54)) != 0 && fc != 0 && m.u8(fc.wrapping_add(0x62)) & 1 != 0 {
            o.call(m, SI_ATTDICT_GET_INFO_FROM_FCURVE, &[SI_ATD, m.u32(fc), o1, o2, o3, o4]);
            let rel = m.u32(o1);
            let r = o.call(m, SI_ELEMDICT_FIND_RELATED_ELEM_M, &[proc_dict(m), e, 0, 0, rel, 0xffff_ffff, f_chap, f_idx]).eax;
            let (c, ix) = (m.u32(f_chap), m.u32(f_idx));
            let ft = m.u32(m.u32(e.wrapping_add(4)));
            if cus_express_set_dirty(m, o, proc_dict(m), c, ix, r, ft, 0) != 0 {
                result = 1;
            }
            m.set_u16(fc.wrapping_add(0x62), 0);
            m.set_u32(SI_FCURVE_DIRTY, m.u32(SI_FCURVE_DIRTY).wrapping_sub(1));
        }
        i += 1;
        e = e.wrapping_add(0x84);
        cur = m.u32(SI_FCURVE_DIRTY) as i32;
    }
    m.release(24);
    for head in [0x0882_2c4cu32, 0x0882_2dac, 0x0882_298c, 0x0882_2aec, 0x0882_2f0c] {
        let list = m.u32(m.u32(head).wrapping_add(0xfc));
        if cus_fcurve_cus_val_set_dirty(m, o, list) != 0 {
            result = 1;
        }
    }
    m.set_u32(SI_FCURVE_DIRTY, 0);
    m.set_u32(SI_CUS_FCURVE_DIRTY, 0);
    m.set_u32(SI_CUS_VAL_DIRTY, 0);
    result
}

/// [soft 0x0853de00 SI_cusPreSceneTransfer]: `cusValueMarkAsDirty`, then `cusStatusPreSceneTransfer` for the motion, model,
/// matter, tools and actor records with its answer (as a short).
pub fn si_cus_pre_scene_transfer(m: &mut Mem, o: &mut dyn Outside) {
    let dirty = cus_value_mark_as_dirty(m, o);
    for s in [CUS_MOTION, CUS_MODEL, CUS_MATTER, CUS_TOOLS, CUS_ACTOR] {
        cus_status_pre_scene_transfer(m, o, s, dirty);
    }
}

/// [soft 0x0853e7b0 cusSceneUpdateStatus]: for each effect of the status record's list (record +0x14c, head +0xfc, chain
/// +0xfc) that has an icon (+0xc) and state 1: without a dso record (+0x104) or with a record whose first word is 0, a
/// process-based effect (kind short 1, or kind 4 when it is `currentEffect`) is updated by `si_cusProcessBasedEffect(status,
/// effect, frame, 1, 1, 0)`; with a record: no occurrence entries (+0x1c is 0) means one `si_cusUpdateEffect(effect, frame,
/// 0)`, else when the record's short at +0x30 is not 0 one call per occurrence entry (table at +0x20, 0x14 bytes each) with
/// the entry's short at +4, word at +8 and short at +0xc copied to a node record. `frame` is the float's bits.
pub fn cus_scene_update_status(m: &mut Mem, o: &mut dyn Outside, status: u32, frame: u32) {
    let mut e = m.u32(m.u32(status.wrapping_add(0x14c)).wrapping_add(0xfc));
    while e != 0 {
        if m.u32(e.wrapping_add(0xc)) != 0 && m.u32(e.wrapping_add(0x100)) == 1 {
            let rec = m.u32(e.wrapping_add(0x104));
            if rec == 0 || m.u32(rec) == 0 {
                let kind = m.u16(e);
                if kind == 1 || (kind == 4 && m.u32(CURRENT_EFFECT) == e) {
                    o.call(m, SI_CUS_PROCESS_BASED_EFFECT, &[status, e, frame, 1, 1, 0]);
                }
            } else {
                let cnt = m.u32(rec.wrapping_add(0x1c)) as i32;
                if cnt == 0 {
                    o.call(m, SI_CUS_UPDATE_EFFECT_M, &[e, frame, 0]);
                } else if m.u16(rec.wrapping_add(0x30)) != 0 && cnt > 0 {
                    let node = m.local(8);
                    let mut i = 0i32;
                    let mut off = 0u32;
                    loop {
                        let tab = m.u32(m.u32(e.wrapping_add(0x104)).wrapping_add(0x20));
                        let a = m.u16(tab.wrapping_add(off).wrapping_add(4));
                        m.set_u16(node, a);
                        let tab = m.u32(m.u32(e.wrapping_add(0x104)).wrapping_add(0x20));
                        let b = m.u32(tab.wrapping_add(off).wrapping_add(8));
                        m.set_u32(node.wrapping_add(4), b);
                        let tab = m.u32(m.u32(e.wrapping_add(0x104)).wrapping_add(0x20));
                        let c = m.u16(tab.wrapping_add(off).wrapping_add(0xc));
                        m.set_u16(node.wrapping_add(2), c);
                        o.call(m, SI_CUS_UPDATE_EFFECT_M, &[e, frame, node]);
                        off = off.wrapping_add(0x14);
                        i += 1;
                        if i >= m.u32(m.u32(e.wrapping_add(0x104)).wrapping_add(0x1c)) as i32 {
                            break;
                        }
                    }
                    m.release(8);
                }
            }
        }
        e = m.u32(e.wrapping_add(0xfc));
    }
}

/// [soft 0x0853e750 SI_cusSceneUpdate]: `cusSceneUpdateStatus` for the model, motion, actor, matter and tools records.
pub fn si_cus_scene_update(m: &mut Mem, o: &mut dyn Outside, frame: u32) {
    for s in [CUS_MODEL, CUS_MOTION, CUS_ACTOR, CUS_MATTER, CUS_TOOLS] {
        cus_scene_update_status(m, o, s, frame);
    }
}

/// [soft 0x0854aaec si_cusDialogDsoDataCreate]: for a dialog (item count at +8, item table at +0xc, 0x3c bytes per item)
/// with at least one item, a table of 0x10 bytes per item (`SI_malloc`); each entry gets +8 = 0, +4 = -1, the short at
/// +0xc = 0 and a zeroed array of `4 * (short at item +0xc)` bytes (`SI_calloc(count, 4)`) at +0. When an array cannot be
/// made, the arrays made before it and the table are freed and the answer is 0. 0 for a null dialog or no items (the
/// original reads the count before testing the dialog).
pub fn si_cus_dialog_dso_data_create(m: &mut Mem, dialog: u32) -> u32 {
    let n = m.u32(dialog.wrapping_add(8)) as i32;
    if dialog == 0 || n < 1 {
        return 0;
    }
    let table = m.si_malloc((n as u32).wrapping_shl(4));
    if table == 0 {
        return 0;
    }
    let mut i = 0i32;
    let mut item_off = 0u32;
    while i < n {
        let e = table.wrapping_add((i as u32).wrapping_shl(4));
        m.set_u32(e.wrapping_add(8), 0);
        m.set_u32(e.wrapping_add(4), 0xffff_ffff);
        m.set_u16(e.wrapping_add(0xc), 0);
        let items = m.u32(dialog.wrapping_add(0xc));
        let cnt = m.u16(items.wrapping_add(item_off).wrapping_add(0xc)) as i16 as i32 as u32;
        let arr = m.si_calloc(cnt, 4);
        m.set_u32(e, arr);
        if arr == 0 {
            let mut k = i;
            let mut p = table;
            while k > 0 {
                let a = m.u32(p);
                if a != 0 {
                    m.free(a);
                    m.set_u32(p, 0);
                }
                p = p.wrapping_add(0x10);
                k -= 1;
            }
            m.free(table);
            return 0;
        }
        item_off = item_off.wrapping_add(0x3c);
        i += 1;
    }
    table
}

/// [soft 0x0853ac4c SI_currentCustomEffectSet]: the argument becomes the current custom effect (0x086e0aa0).
pub fn si_current_custom_effect_set(m: &mut Mem, effect: u32) {
    m.set_u32(0x086e_0aa0, effect);
}

/// [soft 0x0853b264 setReadingAnimModel]: the low 16 bits of the argument are stored at 0x086e0ca2.
pub fn set_reading_anim_model(m: &mut Mem, model: u32) {
    m.set_u16(0x086e_0ca2, model as u16);
}

pub const SI_CUS_CMD_M: Routine = Routine::new("SI_cusCmd", 0x0853_b284);
pub const SI_CHN_DRV_DIALOG_UPD: Routine = Routine::new("SI_chnDrvDialogUpd", 0x0856_8e74);
pub const SI_CHN_DRV_CUS_OK: Routine = Routine::new("SI_chnDrvCusOK", 0x0856_8ea4);
pub const SI_CUS_ADD_MOTION_EFFECT: Routine = Routine::new("si_cusAddMotionEffect", 0x0854_7c14);
pub const SI_CUS_CLOSE_PIPES: Routine = Routine::new("SI_cusClosePipes", 0x0854_7bb0);
pub const SI_REFRESH_ALL: Routine = Routine::new("SI_refreshAll", 0x0832_5dc8);
pub const SI_STATUS_DISPLAY_MESSAGE_M: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);
pub const PLT_SIGNAL: Routine = Routine::new("plt:signal", 0x080c_3bd8);
pub const PLT_SETJMP: Routine = Routine::new("plt:_setjmp", 0x080c_4e48);
/// The jump buffer `_setjmp` is given, and the handler `signal(13)` (SIGPIPE) installs; both are addresses only.
const CUS_JMP_BUF: u32 = 0x0882_2760;
const CUS_SIGPIPE_HANDLER: u32 = 0x0853_f288;

/// si_cusProcessBasedEffect (0x0853b11c): runs the command records of an effect (`rec + 0xf8`, chained through +0x30, up to the
/// record of kind 0x1f) through `SI_cusCmd`, a record of kind 1 only when `all` is set. A broken pipe (SIGPIPE, handler at
/// 0x0853f288) makes the original's `_setjmp` come back non-zero: it then shows "Unable to complete custom effect" and answers -1
/// with the pipe handler still installed; a command that answers below zero does the same without the message.
/// Afterwards the handler is removed; an effect record without a pipe gets its pipes closed, one with +0xc set and `quiet` clear
/// is first handed to si_cusAddMotionEffect; a dialog effect (kind 3) is closed with SI_chnDrvCusOK and a set refresh flag
/// refreshes everything. The answer is the last command's answer; with no command at all the original answers an uninitialised
/// stack word, here 0.
/// The Rust cannot jump out of a signal handler: the `_setjmp` answer decides the branch, as the emulation vectors give it.
pub fn si_cus_process_based_effect(m: &mut Mem, o: &mut dyn Outside, a8: u32, rec: u32, a10: u32, quiet: u32, all: u32, a1c: u32) -> u32 {
    let quiet = quiet as u16;
    let all = all as u16;
    let flag = m.local(4);
    m.set_u16(flag, 0);
    if m.u16(rec) == 3 {
        o.call(m, SI_CHN_DRV_DIALOG_UPD, &[a1c]);
    }
    o.call(m, PLT_SIGNAL, &[0xd, CUS_SIGPIPE_HANDLER]);
    let r = o.call(m, PLT_SETJMP, &[CUS_JMP_BUF]).eax;
    if r != 0 {
        let buf = m.local(0x40);
        m.put_cstr(buf, b"Unable to complete custom effect");
        o.call(m, SI_STATUS_DISPLAY_MESSAGE_M, &[buf, 3]);
        return 0xffff_ffff;
    }
    let mut ret = 0u32;
    let mut cmd = m.u32(rec.wrapping_add(0xf8));
    while cmd != 0 && m.u16(cmd) != 0x1f {
        if all != 0 || m.u16(cmd) != 1 {
            ret = o.call(m, SI_CUS_CMD_M, &[a8, rec, cmd, a10, quiet as i16 as i32 as u32, flag, a1c]).eax;
            if (ret as i32) < 0 {
                return 0xffff_ffff;
            }
        }
        cmd = m.u32(cmd.wrapping_add(0x30));
    }
    o.call(m, PLT_SIGNAL, &[0xd, 0]);
    if m.u32(rec.wrapping_add(0xc)) != 0 {
        if quiet == 0 {
            o.call(m, SI_CUS_ADD_MOTION_EFFECT, &[a8, rec, 0xffff_ffff]);
        }
    }
    if m.u32(rec.wrapping_add(0xc)) == 0 {
        o.call(m, SI_CUS_CLOSE_PIPES, &[rec]);
    }
    if m.u16(rec) == 3 {
        o.call(m, SI_CHN_DRV_CUS_OK, &[a1c]);
    }
    if m.u16(flag) != 0 {
        o.call(m, SI_REFRESH_ALL, &[1]);
    }
    ret
}

/// The blend factor of a byte: `(255 - a) / 255.0` divided in the 80-bit register and rounded to a float, as stored.
fn comp_factor(a: u8) -> f32 {
    (si_x87::x(!a as i32) / si_x87::x(255.0f64)).to_f32()
}

/// `0.5 + s + d * f` as the original sums it (the product and the sum in the 80-bit register, then rounded to a float), turned
/// into a byte: above 255.0 the byte is 255, else the float is truncated.
fn comp_mix(d: u8, f: f32, s: u8) -> u8 {
    use si_x87::{x, Round};
    let sum = x(d as i32) * f + x(s as i32) + x(0.5f64);
    let r = sum.to_f32();
    if x(r).gt(x(255.0f64)) {
        0xff
    } else {
        x(r).to_i32(Round::Zero) as u8
    }
}

/// cusCompositeScanLine (0x0853f864): blends `n` pixels of a scan line, the destination colour bytes `d1 d2 d3` (and alpha `da`
/// when not null) with the source `s1 s2 s3` (and alpha `sa`), by `mode`: 0x26 puts the source over the destination (the
/// source alone, alpha 0xff, when `sa` is null; else the destination weighted by the source alpha's complement plus the source),
/// 0x27 adds the source weighted by the destination alpha's complement (only when `da` is not null), 0x28 does the same by
/// depth: where the source depth `zs` is not greater than the buffer depth `zd` it is added like 0x27 (when `da` is not null),
/// else the depth is taken over and the source is put over (alpha 0 without `sa`). Any other mode, or n <= 0, does nothing.
/// A NaN depth counts as "not greater" like the original's flag test. Not a [soft ..] citation: the x87 rounding steps are kept,
/// the unused stack double the original loads in mode 0x28 is not.
#[allow(clippy::too_many_arguments)]
pub fn cus_composite_scan_line(m: &mut Mem, d1: u32, d2: u32, d3: u32, da: u32, zd: u32, s1: u32, s2: u32, s3: u32, sa: u32, zs: u32, n: i32, mode: i32) {
    if n <= 0 {
        return;
    }
    for i in 0..n as u32 {
        match mode {
            0x26 => {
                if sa == 0 {
                    let (a, b, c) = (m.u8(s1 + i), m.u8(s2 + i), m.u8(s3 + i));
                    m.set_u8(d1 + i, a);
                    m.set_u8(d2 + i, b);
                    m.set_u8(d3 + i, c);
                    if da != 0 {
                        m.set_u8(da + i, 0xff);
                    }
                } else {
                    comp_over(m, d1, d2, d3, da, s1, s2, s3, sa, i);
                }
            }
            0x27 => {
                if da != 0 {
                    comp_add(m, d1, d2, d3, da, s1, s2, s3, sa, i);
                }
            }
            0x28 => {
                let (zn, zo) = (m.f32(zs + i * 4), m.f32(zd + i * 4));
                if zn > zo {
                    m.set_f32(zd + i * 4, si_x87::x(zn).to_f32());
                    if sa == 0 {
                        let (a, b, c) = (m.u8(s1 + i), m.u8(s2 + i), m.u8(s3 + i));
                        m.set_u8(d1 + i, a);
                        m.set_u8(d2 + i, b);
                        m.set_u8(d3 + i, c);
                        if da != 0 {
                            m.set_u8(da + i, 0);
                        }
                    } else {
                        comp_over(m, d1, d2, d3, da, s1, s2, s3, sa, i);
                    }
                } else if da != 0 {
                    comp_add(m, d1, d2, d3, da, s1, s2, s3, sa, i);
                }
            }
            _ => return,
        }
    }
}

/// The source over the destination, weights from the source alpha `sa[i]`; the destination alpha (when there is one) becomes
/// `0.5 + sa + da * f`.
#[allow(clippy::too_many_arguments)]
fn comp_over(m: &mut Mem, d1: u32, d2: u32, d3: u32, da: u32, s1: u32, s2: u32, s3: u32, sa: u32, i: u32) {
    let f = comp_factor(m.u8(sa + i));
    for (d, s) in [(d1, s1), (d2, s2), (d3, s3)] {
        let v = comp_mix(m.u8(d + i), f, m.u8(s + i));
        m.set_u8(d + i, v);
    }
    if da != 0 {
        let v = comp_mix(m.u8(da + i), f, m.u8(sa + i));
        m.set_u8(da + i, v);
    }
}

/// The source added to the destination, weights from the destination alpha `da[i]`; the destination alpha becomes
/// `0.5 + da + sa * f` when there is a source alpha.
#[allow(clippy::too_many_arguments)]
fn comp_add(m: &mut Mem, d1: u32, d2: u32, d3: u32, da: u32, s1: u32, s2: u32, s3: u32, sa: u32, i: u32) {
    let f = comp_factor(m.u8(da + i));
    for (d, s) in [(d1, s1), (d2, s2), (d3, s3)] {
        let v = comp_mix(m.u8(s + i), f, m.u8(d + i));
        m.set_u8(d + i, v);
    }
    if sa != 0 {
        let v = comp_mix(m.u8(sa + i), f, m.u8(da + i));
        m.set_u8(da + i, v);
    }
}

pub const SI_PICTURE_OPEN_FILE: Routine = Routine::new("SI_pictureOpenFile", 0x085d_2970);
pub const SI_FDPICTURE_OPEN_FILE: Routine = Routine::new("SI_fdpictureOpenFile", 0x085d_6350);
pub const SI_SCANLINE_READ: Routine = Routine::new("SI_scanlineRead", 0x085d_2cfc);
pub const SI_FLOAT_FREAD: Routine = Routine::new("SI_float_fread", 0x083f_9a7c);
pub const SI_PICTURE_DISPOSE: Routine = Routine::new("SI_pictureDispose", 0x085d_45bc);
pub const LRECTWRITE: Routine = Routine::new("lrectwrite", 0x0861_1a50);
pub const PLT_FOPEN: Routine = Routine::new("plt:fopen", 0x080c_4e18);
pub const PLT_FCLOSE: Routine = Routine::new("plt:fclose", 0x080c_4958);
pub const PLT_FEOF: Routine = Routine::new("plt:feof", 0x080c_3948);
pub const PLT_FPRINTF_M: Routine = Routine::new("plt:fprintf", 0x080c_3b88);
const STDERR_CELL: u32 = 0x0878_0c88;
const STDIN_CELL: u32 = 0x0878_0e90;
const STDOUT_CELL: u32 = 0x0878_0c84;
/// `cusPicture` (width and height of the picture the effect works on), the four plane buffers (red, green, blue, alpha) and the
/// depth buffer `cusZPicture`.
const CUS_PICTURE: u32 = 0x0882_27fc;
const CUS_PLANES: u32 = 0x0882_2808;
const CUS_Z_PICTURE: u32 = 0x0882_2818;
/// The draw-back flag (a short at 0x08790d58) and the rectangle origin (shorts at 0x08790d54 / 0x08790d56).
const DRAW_BACK: u32 = 0x0879_0d58;
const DRAW_X: u32 = 0x0879_0d54;
const DRAW_Y: u32 = 0x0879_0d56;
// text addresses in the original's read-only data; the Rust passes the addresses, the texts are given in the comment
const TXT_CANNOT_READ_FD: u32 = 0x086e_2100;
const TXT_CANNOT_READ: u32 = 0x086e_2140;
const TXT_BAD_RESOLUTION: u32 = 0x086e_2162;
const TXT_RB: u32 = 0x086e_2187;
const TXT_NO_Z: u32 = 0x086e_21a0;

fn c_string_or_null(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 { b"(null)".to_vec() } else { m.cstr(p) }
}

/// cusCompositeCmd (0x0853f2ac): composites the picture an effect wrote (a file named by the command at +8, or the pipe
/// record at +0x2c) over the working planes, row by row through `cusCompositeScanLine` (the mode is the command's kind at +0, 0x26
/// to 0x28; mode 0x28 also reads the picture's depth file "<tmp dir><name>.Zpic", missing depths -100000.0), and when the
/// draw-back flag is set sends each composited row to the screen with `lrectwrite`. Answers 1, or -1 after printing to stderr
/// "Cannot read custom picture file.", "Incompatable resolution.." or "Cannot open z file for reading."; a piped command
/// (+0x2c set) answers -1 right after the resolution check without closing or freeing anything, as the original does.
/// Texts: "Cannot read custom picture file.\n" (x2), "Incompatable resolution..\n", "%s%s.Zpic", "rb",
/// "Cannot open z file for reading.\n". Partly cited: not a [soft ..] citation because the file and picture calls are hooks.
pub fn cus_composite_cmd(m: &mut Mem, o: &mut dyn Outside, cmd: u32) -> u32 {
    let file_cell = m.local(4); // the open file
    let zf_cell = m.local(4); // the float the open call answers (unused afterwards)
    let chans_cell = m.local(4);
    let namebuf = m.local(0x100);
    let pic = m.local(0xc); // width, height (shorts), then the picture's other words
    let pipe = m.u32(cmd.wrapping_add(0x2c));
    let ok;
    if pipe != 0 {
        let f = m.u32(pipe.wrapping_add(0x14));
        m.set_u32(file_cell, f);
        ok = o.call(m, SI_FDPICTURE_OPEN_FILE, &[f, pic, zf_cell, namebuf, chans_cell]).eax as u16;
        if ok == 0 {
            let e = m.u32(STDERR_CELL);
            o.call(m, PLT_FPRINTF_M, &[e, TXT_CANNOT_READ_FD]);
            return 0xffff_ffff;
        }
    } else {
        let name = m.u32(cmd.wrapping_add(8));
        ok = o.call(m, SI_PICTURE_OPEN_FILE, &[name, file_cell, pic, zf_cell, namebuf, chans_cell]).eax as u16;
        if ok == 0 {
            let e = m.u32(STDERR_CELL);
            o.call(m, PLT_FPRINTF_M, &[e, TXT_CANNOT_READ]);
            return 0xffff_ffff;
        }
    }
    if m.u32(CUS_PICTURE) != m.u32(pic) {
        let e = m.u32(STDERR_CELL);
        o.call(m, PLT_FPRINTF_M, &[e, TXT_BAD_RESOLUTION]);
        return 0xffff_ffff;
    }
    let rows = m.u16(pic.wrapping_add(2)) as i16 as i32;
    let width = m.u16(pic) as i16 as i32;
    let wu = width as u32;
    let mut zfile = 0u32;
    let mut zbuf = 0u32;
    let mut zpix = m.u32(CUS_Z_PICTURE);
    if pipe != 0 {
        return 0xffff_ffff;
    }
    let mode = m.u16(cmd) as i16 as i32;
    if mode == 0x28 {
        let name = m.u32(cmd.wrapping_add(8));
        let tmp = cus_tmp_dir(m, o, name);
        let mut text = c_string_or_null(m, tmp);
        text.extend_from_slice(&c_string_or_null(m, name));
        text.extend_from_slice(b".Zpic");
        m.put_cstr(namebuf, &text);
        zfile = o.call(m, PLT_FOPEN, &[namebuf, TXT_RB]).eax;
        if zfile == 0 {
            let e = m.u32(STDERR_CELL);
            o.call(m, PLT_FPRINTF_M, &[e, TXT_NO_Z]);
            return 0xffff_ffff;
        }
        zbuf = m.malloc(wu.wrapping_mul(4));
    }
    let mut row_img = 0u32;
    if m.u16(DRAW_BACK) != 0 {
        row_img = m.malloc(wu.wrapping_mul(4));
        let mut i = 0i32;
        while i < width {
            m.set_u32(row_img.wrapping_add(i as u32 * 4), 0);
            i += 1;
        }
    }
    let file = m.u32(file_cell);
    let chans = m.u32(chans_cell);
    m.set_u16(pic.wrapping_add(2), 1);
    let mut plane = [0u32; 4]; // alpha (-0x10), blue.. as the flag bits 0x80, 0x40, 0x20, 0x10 give them
    let flag_bits = [0x80u8, 0x40, 0x20, 0x10];
    // first row: allocate a plane per flag and hand out the pointers
    let mut ch = chans;
    while ch != 0 {
        m.set_u16(ch.wrapping_add(0x14), 0);
        let f = m.u8(ch.wrapping_add(2));
        for (k, bit) in flag_bits.iter().enumerate() {
            if f & bit != 0 {
                let p = m.malloc(m.u16(pic) as i16 as i32 as u32);
                plane[k] = p;
                let n = m.u16(ch.wrapping_add(0x14)) as i16 as i32 as u32;
                m.set_u32(ch.wrapping_add(4).wrapping_add(n.wrapping_mul(4)), p);
                m.set_u16(ch.wrapping_add(0x14), (n as u16).wrapping_add(1));
            }
        }
        ch = m.u32(ch.wrapping_add(0x18));
    }
    let mut dst = [m.u32(CUS_PLANES), m.u32(CUS_PLANES + 4), m.u32(CUS_PLANES + 8), m.u32(CUS_PLANES + 12)];
    let mut row = 0i32;
    while row < rows {
        o.call(m, SI_SCANLINE_READ, &[file, chans, pic]);
        if zfile != 0 && zbuf != 0 {
            let got = o.call(m, SI_FLOAT_FREAD, &[zbuf, 4, wu, zfile]).eax;
            if got != wu {
                let mut i = 0i32;
                while i < width {
                    m.set_f32(zbuf.wrapping_add(i as u32 * 4), -100000.0);
                    i += 1;
                }
            }
        }
        // the planes d1 d2 d3 da, the depth zpix; the source planes (colour bits 0x80 0x40 0x20) and alpha (0x10); the depth read
        cus_composite_scan_line(m, dst[0], dst[1], dst[2], dst[3], zpix, plane[0], plane[1], plane[2], plane[3], zbuf, width, mode);
        if m.u16(DRAW_BACK) != 0 && row_img != 0 {
            let mut i = 0i32;
            while i < width {
                let a = i as u32;
                let v = (m.u8(dst[2].wrapping_add(a)) as u32) << 16 | (m.u8(dst[1].wrapping_add(a)) as u32) << 8 | m.u8(dst[0].wrapping_add(a)) as u32;
                m.set_u32(row_img.wrapping_add(a * 4), v);
                i += 1;
            }
            let y = (rows as i16).wrapping_add(m.u16(DRAW_Y) as i16).wrapping_sub(row as i16).wrapping_sub(1) as i32;
            let x2 = (width as i16).wrapping_add(m.u16(DRAW_X) as i16).wrapping_sub(1) as i32;
            o.call(m, LRECTWRITE, &[m.u16(DRAW_X) as i16 as i32 as u32, y as u32, x2 as u32, y as u32, row_img]);
        }
        if o.call(m, PLT_FEOF, &[file]).eax != 0 {
            break;
        }
        for d in dst.iter_mut() {
            if *d != 0 {
                *d = d.wrapping_add(wu);
            }
        }
        if zpix != 0 {
            zpix = zpix.wrapping_add(wu.wrapping_mul(4));
        }
        row += 1;
        // later rows: the same planes again
        let mut ch = chans;
        while ch != 0 {
            m.set_u16(ch.wrapping_add(0x14), 0);
            let f = m.u8(ch.wrapping_add(2));
            for (k, bit) in flag_bits.iter().enumerate() {
                if f & bit != 0 {
                    let n = m.u16(ch.wrapping_add(0x14)) as i16 as i32 as u32;
                    m.set_u32(ch.wrapping_add(4).wrapping_add(n.wrapping_mul(4)), plane[k]);
                    m.set_u16(ch.wrapping_add(0x14), (n as u16).wrapping_add(1));
                }
            }
            ch = m.u32(ch.wrapping_add(0x18));
        }
    }
    if file != m.u32(STDIN_CELL) && file != m.u32(STDOUT_CELL) {
        o.call(m, PLT_FCLOSE, &[file]);
    }
    let mut ch = chans;
    while ch != 0 {
        let next = m.u32(ch.wrapping_add(0x18));
        m.free(ch);
        ch = next;
    }
    o.call(m, SI_PICTURE_DISPOSE, &[pic]);
    if zbuf != 0 {
        m.free(zbuf);
    }
    if row_img != 0 {
        m.free(row_img);
    }
    1
}
