// SPDX-License-Identifier: MIT
//! `custom_util.c` / `custom_dialog.c` pieces left after the first continuations (unit shdb5, finding 0041, 2026-10-29): the
//! selection tags of an effect's icon, the callback removers, the relations of a dso effect and `cusStatusDeleteIcon`.

use crate::custom_dso::si_cus_resolve_symbol;
use crate::custom_io::SI_CUS_SET_CURRENT_DSO_EFFECT;
use crate::custom_misc::{si_cus_dec_nb_post_def_fx, SI_ELEMDICT_EXISTS, SI_ELEMDICT_GET_REL_SERIAL_NUMBER, SI_GRAF_LIST_EXISTS};
use crate::custom_scene::{elem_get_user_data_list, si_cus_user_data_get_callback_nodes};
use crate::custom_util::*;
use crate::shdb_init::SI_STATUS_DISPLAY_MESSAGE;
use si_mem::{Mem, Outside, Routine};

pub const SI_BYTESWAP_FREAD: Routine = Routine::new("SI_byteswap_fread", 0x083f_9ba0);
pub const SI_ELEMDICT_GET_ELEM_R: Routine = Routine::new("SI_elemdictGetElem", 0x0813_2b50);
pub const SI_ELEMDICT_FIND_RELATED_ELEM_R: Routine = Routine::new("SI_elemdictFindRelatedElem", 0x0813_508c);
pub const SI_ELEMDICT_GET_CUR_RELATION: Routine = Routine::new("SI_elemdictGetCurRelation", 0x0813_4db0);
pub const SI_ELEMDICT_REMOVE_SERIAL_REL_ID: Routine = Routine::new("SI_elemdictRemoveSerialRelId", 0x0813_3a6c);
pub const SI_ELEMDICT_ADD_SERIAL_RELATION: Routine = Routine::new("SI_elemdictAddSerialRelation", 0x0813_3184);
pub const SI_IS_MODEL_DEFORMED: Routine = Routine::new("SI_isModelDeformed", 0x0847_2d48);
pub const SI_MODEL_DELETE_DEFINITION_R: Routine = Routine::new("SI_modelDeleteDefinition", 0x0846_78a8);
pub const SI_RELDICT_GET_CHAP_R: Routine = Routine::new("SI_reldictGetChap", 0x0813_f7a0);
pub const SI_RELDICT_GET_GRP_R: Routine = Routine::new("SI_reldictGetGrp", 0x0813_f7e8);
pub const SI_ED_GET_CAM_FROM_CAM_INT: Routine = Routine::new("SI_edGetCamFromCamInt", 0x080f_d040);
pub const SI_ED_GET_LIG_FROM_LIG_INT: Routine = Routine::new("SI_edGetLigFromLigInt", 0x080f_d150);
pub const SI_FCV_SCROLL_BUILD_R: Routine = Routine::new("SI_fcvScrollBuild", 0x0839_78d8);
const SI_RD: u32 = 0x0878_15f0;
const SI_CID_MODELS: u32 = 0x0878_1680;
const SI_CID_CAMERAS: u32 = 0x0878_1664;
const SI_CID_LIGHTS: u32 = 0x0878_1678;
const SI_ELEM_DICTS_G: u32 = 0x0867_931c;
const SI_VAL_NUM_CALLBACK: u32 = 0x086e_49e8;
const PREVIEW_CDG: u32 = 0x086d_c300;
const PREVIEW_UDF: u32 = 0x086d_c2fc;
/// The pointers to the five status records `si_customValueCallbackRemoveByEffect` walks.
const STATUS_HEADS: [u32; 5] = [0x0882_2c4c, 0x0882_2dac, 0x0882_298c, 0x0882_2aec, 0x0882_2f0c];

fn dict_of(m: &Mem) -> u32 {
    let pd = m.u32(SI_PROC_DATA);
    let i = m.u32(pd);
    i.wrapping_shl(3).wrapping_sub(i).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS_G))
}

/// The shape both tag routines share: for an icon's kind (1, 4, 6, 9, 10) the number of flagged items of the record at +0x48
/// and where each item's flag byte lives (bit 0). Kinds 2, 3, 5, 7, 8 and anything else have none. The counts are compared
/// as signed words, as the original does.
fn tag_shape(m: &Mem, kind: u32, s: u32) -> Option<(i32, Box<dyn Fn(&Mem, i32) -> u32>)> {
    match kind {
        6 => Some((m.u16(s.wrapping_add(0x10)) as i32, Box::new(move |m: &Mem, i| m.u32(s.wrapping_add(0x14)).wrapping_add((i as u32).wrapping_mul(0x30)).wrapping_add(0x2c)))),
        1 => {
            let n = (m.u16(s.wrapping_add(0xc)) as u32).wrapping_mul(m.u16(s.wrapping_add(0xe)) as u32) as i32;
            Some((n, Box::new(move |m: &Mem, i| m.u32(s.wrapping_add(0x1a8)).wrapping_add((i as u32).wrapping_mul(2)))))
        }
        4 => Some((m.u32(s.wrapping_add(0x14)) as i32, Box::new(move |m: &Mem, i| m.u32(s).wrapping_add((i as u32).wrapping_mul(0x14)).wrapping_add(0x12)))),
        9 => Some((m.u32(s.wrapping_add(4)) as i32, Box::new(move |m: &Mem, i| m.u32(s.wrapping_add(0x2c)).wrapping_add((i as u32).wrapping_mul(0x24)).wrapping_add(0x20)))),
        10 => Some((m.u32(s.wrapping_add(0x48)) as i32, Box::new(move |m: &Mem, i| m.u32(s.wrapping_add(0x54)).wrapping_add((i as u32).wrapping_mul(0x24)).wrapping_add(0x20)))),
        _ => None,
    }
}

/// [soft 0x0854a43c si_cusComputeTag]: for the icon's kind (+0x30 of the record two levels down, +0x28 then +0x1c) one byte per
/// item (1 when its flag bit 0 is set, else 0) in a fresh block; the count goes to +0x20 of the icon and the block to +0x24.
/// Kinds without items store 0 and 0.
pub fn si_cus_compute_tag(m: &mut Mem, icon: u32) {
    let rec = m.u32(m.u32(icon.wrapping_add(0x28)).wrapping_add(0x1c));
    let kind = m.u32(rec.wrapping_add(0x30));
    let s = m.u32(rec.wrapping_add(0x48));
    let (mut count, mut buf) = (0i32, 0u32);
    if let Some((n, at)) = tag_shape(m, kind, s) {
        count = n;
        if n > 0 {
            buf = m.malloc(n as u32);
        }
        let mut i = 0i32;
        while i < n {
            let on = m.u8(at(m, i)) & 1 != 0;
            m.set_u8(buf.wrapping_add(i as u32), on as u8);
            i += 1;
        }
    }
    m.set_u32(icon.wrapping_add(0x20), count as u32);
    m.set_u32(icon.wrapping_add(0x24), buf);
}

/// [soft 0x0854a5d4 si_cusReadTag]: reads a byte-swapped count and that many bytes from the file; with an icon, sets or clears
/// flag bit 0 of the icon's items (as many as both the item count and the bytes read hold) from them (a byte of 1 sets, anything
/// else clears). The block is freed; a zero count ends the routine at once.
pub fn si_cus_read_tag(m: &mut Mem, o: &mut dyn Outside, fp: u32, icon: u32) {
    let nloc = m.local(4);
    o.call(m, SI_BYTESWAP_FREAD, &[nloc, 4, 1, fp]);
    let n = m.u32(nloc) as i32;
    if n == 0 {
        m.release(4);
        return;
    }
    let buf = m.malloc(n as u32);
    o.call(m, SI_BYTESWAP_FREAD, &[buf, 1, n as u32, fp]);
    if icon != 0 {
        let kind = m.u32(icon.wrapping_add(0x30));
        let s = m.u32(icon.wrapping_add(0x48));
        if let Some((count, at)) = tag_shape(m, kind, s) {
            let mut i = 0i32;
            while i < count && i < n {
                let a = at(m, i);
                let v = m.u8(a);
                if m.u8(buf.wrapping_add(i as u32)) == 1 {
                    m.set_u8(a, v | 1);
                } else {
                    m.set_u8(a, v & 0xfe);
                }
                i += 1;
            }
        }
    }
    m.free(buf);
    m.release(4);
}

/// [soft 0x08527b34 si_cusValueInDialog]: 1 when both pointers are set and the dialog's id (a short at +0) is the value's (+0x4c).
pub fn si_cus_value_in_dialog(m: &Mem, dialog: u32, value: u32) -> u32 {
    (dialog != 0 && value != 0 && m.u16(dialog) as i16 as i32 as u32 == m.u32(value.wrapping_add(0x4c))) as u32
}

/// [soft 0x0852728c setCustomPreviewUDF]: remembers the preview's dialog group and udf.
pub fn set_custom_preview_udf(m: &mut Mem, cdg: u32, udf: u32) {
    m.set_u32(PREVIEW_CDG, cdg);
    m.set_u32(PREVIEW_UDF, udf);
}

/// [soft 0x085272a4 unsetCustomPreviewUDF]: forgets them.
pub fn unset_custom_preview_udf(m: &mut Mem) {
    m.set_u32(PREVIEW_CDG, 0);
    m.set_u32(PREVIEW_UDF, 0);
}

/// [soft 0x085299e0 SI_customValueCallbackAdd]: appends a 0x18-byte record (elem dict, graf list, two words, a user word, next 0) to the
/// status' callback list (+0x3c head, +0x40 first-to-run). 0x10 for a null status or word, 0x46 when out of memory.
pub fn si_custom_value_callback_add(m: &mut Mem, o: &mut dyn Outside, status: u32, dict: u32, graf: u32, w1: u32, w2: u32, user: u32) -> u32 {
    if status == 0 || w2 == 0 || w1 == 0 {
        return 0x10;
    }
    if o.call(m, SI_ELEMDICT_EXISTS, &[dict]).eax as u16 == 0 || o.call(m, SI_GRAF_LIST_EXISTS, &[graf]).eax as u16 == 0 {
        return 0x10;
    }
    let mut last = m.u32(status.wrapping_add(0x3c));
    while last != 0 && m.u32(last.wrapping_add(0x14)) != 0 {
        last = m.u32(last.wrapping_add(0x14));
    }
    let n = m.si_malloc(0x18);
    if n == 0 {
        return 0x46;
    }
    m.set_u32(n.wrapping_add(4), graf);
    m.set_u32(n.wrapping_add(8), w1);
    m.set_u32(n, dict);
    m.set_u32(n.wrapping_add(0xc), w2);
    m.set_u32(n.wrapping_add(0x10), user);
    m.set_u32(n.wrapping_add(0x14), 0);
    if last == 0 {
        m.set_u32(status.wrapping_add(0x3c), n);
    } else {
        m.set_u32(last.wrapping_add(0x14), n);
    }
    if m.u32(status.wrapping_add(0x40)) == 0 {
        m.set_u32(status.wrapping_add(0x40), n);
    }
    let c = m.u32(SI_VAL_NUM_CALLBACK).wrapping_add(1);
    m.set_u32(SI_VAL_NUM_CALLBACK, c);
    0
}

/// [soft 0x08529ab0 SI_customValueCallbackRemove]: unlinks the record that has the given three words and frees it. 0x10 when an argument
/// is null, a dict or graf list is unknown, or no record matches.
pub fn si_custom_value_callback_remove(m: &mut Mem, o: &mut dyn Outside, status: u32, dict: u32, graf: u32, w1: u32, w2: u32, user: u32) -> u32 {
    if status == 0 || w2 == 0 || w1 == 0 {
        return 0x10;
    }
    if o.call(m, SI_ELEMDICT_EXISTS, &[dict]).eax as u16 == 0 || o.call(m, SI_GRAF_LIST_EXISTS, &[graf]).eax as u16 == 0 {
        return 0x10;
    }
    let mut prev = 0u32;
    let mut e = m.u32(status.wrapping_add(0x3c));
    loop {
        if e == 0 {
            return 0x10;
        }
        if m.u32(e.wrapping_add(0xc)) == w2 && m.u32(e.wrapping_add(0x10)) == user && m.u32(e.wrapping_add(8)) == w1 {
            break;
        }
        prev = e;
        e = m.u32(e.wrapping_add(0x14));
    }
    let next = m.u32(e.wrapping_add(0x14));
    if prev == 0 {
        m.set_u32(status.wrapping_add(0x3c), next);
    } else {
        m.set_u32(prev.wrapping_add(0x14), next);
    }
    if m.u32(status.wrapping_add(0x40)) == e {
        m.set_u32(status.wrapping_add(0x40), next);
    }
    m.free(e);
    let c = m.u32(SI_VAL_NUM_CALLBACK).wrapping_sub(1);
    m.set_u32(SI_VAL_NUM_CALLBACK, c);
    0
}

/// [soft 0x08529c1c customValueCallbackRemoveByEffect]: from the list of effects starting at `list` (linked by +0xfc), for each that has a
/// dso record with a link and an owner (+0x64), and for each owner on its chain (+0x44), unlinks and frees the callback records whose
/// word at +8 is `eff`. 0x10 for a null `eff`.
pub fn custom_value_callback_remove_by_effect(m: &mut Mem, eff: u32, list: u32) -> u32 {
    if eff == 0 {
        return 0x10;
    }
    let mut e = list;
    while e != 0 {
        let rec = m.u32(e.wrapping_add(0x104));
        if rec != 0 && m.u32(rec) != 0 {
            let mut owner = m.u32(e.wrapping_add(0x64));
            while owner != 0 {
                let mut prev = 0u32;
                let mut c = m.u32(owner.wrapping_add(0x3c));
                while c != 0 {
                    if m.u32(c.wrapping_add(8)) == eff {
                        let next = m.u32(c.wrapping_add(0x14));
                        if m.u32(owner.wrapping_add(0x40)) == c {
                            m.set_u32(owner.wrapping_add(0x40), next);
                        }
                        if prev == 0 {
                            m.set_u32(owner.wrapping_add(0x3c), next);
                        } else {
                            m.set_u32(prev.wrapping_add(0x14), next);
                        }
                        m.free(c);
                        let n = m.u32(SI_VAL_NUM_CALLBACK).wrapping_sub(1);
                        m.set_u32(SI_VAL_NUM_CALLBACK, n);
                        c = if prev == 0 { m.u32(owner.wrapping_add(0x3c)) } else { m.u32(prev.wrapping_add(0x14)) };
                    } else {
                        prev = c;
                        c = m.u32(c.wrapping_add(0x14));
                    }
                }
                owner = m.u32(owner.wrapping_add(0x44));
            }
        }
        e = m.u32(e.wrapping_add(0xfc));
    }
    0
}

/// [soft 0x08529b84 si_customValueCallbackRemoveByEffect]: the above for the effect lists of the five status records and then for the
/// effect itself as the list. 0x10 for a null effect.
pub fn si_custom_value_callback_remove_by_effect(m: &mut Mem, eff: u32) -> u32 {
    if eff == 0 {
        return 0x10;
    }
    for g in STATUS_HEADS {
        let head = m.u32(g);
        let list = m.u32(head.wrapping_add(0xfc));
        custom_value_callback_remove_by_effect(m, eff, list);
    }
    custom_value_callback_remove_by_effect(m, eff, eff);
    0
}

/// [soft 0x08529cf8 cusCheckFcurveChange]: does nothing for a null `first`, or when both flags (shorts) are already set. Otherwise walks the
/// values after `first` (chain +0x44) for the first one that belongs to the dialog (si_cusValueInDialog), has id `id` (+8) and an f-curve
/// record (+0x2c): it sets the second flag, and the first too unless the record's +0xc is 1.
pub fn cus_check_fcurve_change(m: &mut Mem, dialog: u32, first: u32, id: i16, f1: u32, f2: u32) {
    if first == 0 {
        return;
    }
    if m.u16(f1) != 0 && m.u16(f2) != 0 {
        return;
    }
    let mut v = m.u32(first.wrapping_add(0x44));
    while v != 0 {
        if si_cus_value_in_dialog(m, dialog, v) as u16 != 0 && m.u32(v.wrapping_add(8)) == id as i32 as u32 {
            let fc = m.u32(v.wrapping_add(0x2c));
            if fc != 0 {
                if m.u32(fc.wrapping_add(0xc)) != 1 {
                    m.set_u16(f1, 1);
                }
                m.set_u16(f2, 1);
                return;
            }
        }
        v = m.u32(v.wrapping_add(0x44));
    }
}

/// [soft 0x0854b9ac cusElemCallbackRemoveByEffect]: for every element of every element dictionary kind (while the effect's callback count,
/// a short at +0x3c of its dso record, stays above 0) that has the callback nodes in its user data list, removes the 12-byte callback
/// records (+0xc of the "callback" node, size at +8) whose first word is the effect, rebuilding the table in a fresh block (or
/// emptying it when it held only that record); each removal takes one off the effect's count, stored back at the end.
pub fn cus_elem_callback_remove_by_effect(m: &mut Mem, eff: u32) {
    if eff == 0 {
        return;
    }
    let rec = m.u32(eff.wrapping_add(0x104));
    if rec == 0 {
        return;
    }
    let mut left = m.u16(rec.wrapping_add(0x3c)) as i16 as i32;
    let loc = m.local(8);
    let (l_cb, l_next) = (loc, loc.wrapping_add(4));
    let mut k = 0i32;
    'kinds: while left > 0 && k < m.u32(dict_of(m)) as i32 {
        let kd = m.u32(dict_of(m).wrapping_add(4)).wrapping_add((k as u32).wrapping_shl(6));
        let mut j = 0i32;
        while left > 0 && j < m.u32(kd.wrapping_add(4)) as i32 {
            let elem = m.u32(kd.wrapping_add(8)).wrapping_add((j as u32).wrapping_mul(0x84));
            j += 1;
            m.set_u32(l_cb, 0);
            m.set_u32(l_next, 0);
            if m.u32(elem.wrapping_add(4)) == 0 || m.u16(elem.wrapping_add(0x48)) != 0 {
                continue;
            }
            let list = elem_get_user_data_list(m, elem, k as u32);
            si_cus_user_data_get_callback_nodes(m, list, l_cb, l_next);
            if m.u32(l_next) == 0 || m.u32(l_cb) == 0 {
                continue;
            }
            let cb = m.u32(l_cb);
            let mut idx = 0i32;
            let mut off = 0u32;
            while idx < (m.u32(cb.wrapping_add(8)) as i32) / 12 {
                let table = m.u32(cb.wrapping_add(0xc));
                if m.u32(table.wrapping_add(off)) != eff {
                    off = off.wrapping_add(12);
                    idx += 1;
                    continue;
                }
                let size = m.u32(cb.wrapping_add(8));
                if size == 12 {
                    m.set_u32(cb.wrapping_add(8), 0);
                    let p = m.u32(cb.wrapping_add(0xc));
                    if p != 0 {
                        m.free(p);
                        m.set_u32(cb.wrapping_add(0xc), 0);
                    }
                    m.set_u32(cb.wrapping_add(0xc), 0);
                    left -= 1;
                } else {
                    let nb = m.si_malloc(size.wrapping_sub(12));
                    left -= 1;
                    if nb != 0 {
                        m.set_u32(cb.wrapping_add(0xc), nb);
                        let ns = m.u32(cb.wrapping_add(8)).wrapping_sub(12);
                        m.set_u32(cb.wrapping_add(8), ns);
                        let n = (ns as i32) / 12;
                        for r in 0..idx.max(0) as u32 {
                            for w in 0..3 {
                                let v = m.u32(table.wrapping_add(r * 12 + w * 4));
                                m.set_u32(nb.wrapping_add(r * 12 + w * 4), v);
                            }
                        }
                        let mut r = idx;
                        while r < n {
                            for w in 0..3 {
                                let v = m.u32(table.wrapping_add((r as u32) * 12 + 12 + w * 4));
                                m.set_u32(nb.wrapping_add((r as u32) * 12 + w * 4), v);
                            }
                            r += 1;
                        }
                        if table != 0 {
                            m.free(table);
                        }
                    }
                }
            }
        }
        k += 1;
        if left <= 0 {
            break 'kinds;
        }
    }
    m.release(8);
    m.set_u16(m.u32(eff.wrapping_add(0x104)).wrapping_add(0x3c), left as u16);
}

/// [soft 0x08549bc8 SI_cusDsoEffectDestroyRelations]: takes the effect's last relation record (+0x10 array of 20-byte records, +0xc
/// count) and, as long as the element it names has related elements of that relation kind, deletes each one's definition when it is a
/// model that is not deformed and removes the serial relation. Answers 0.
pub fn si_cus_dso_effect_destroy_relations(m: &mut Mem, o: &mut dyn Outside, eff: u32) -> u32 {
    if eff == 0 {
        return 0;
    }
    let rec = m.u32(eff.wrapping_add(0x104));
    if (m.u32(rec.wrapping_add(0x1c)) as i32) <= 0 {
        return 0;
    }
    let r = m.u32(rec.wrapping_add(0x10)).wrapping_add(m.u32(rec.wrapping_add(0xc)).wrapping_mul(20)).wrapping_sub(0x14);
    let (dict, rel) = (dict_of(m), m.u32(r.wrapping_add(0xc)));
    let elem = o.call(m, SI_ELEMDICT_GET_ELEM_R, &[dict, m.u32(r.wrapping_add(4)), m.u32(r.wrapping_add(8))]).eax;
    let loc = m.local(8);
    let (o8, o4) = (loc, loc.wrapping_add(4)); // ebp-8, ebp-4
    let find = |m: &mut Mem, o: &mut dyn Outside| o.call(m, SI_ELEMDICT_FIND_RELATED_ELEM_R, &[dict_of(m), elem, 0, rel, 0x4e20, 0xffff_ffff, o8, o4]).eax;
    let mut found = find(m, o);
    while found != 0 {
        let cur = o.call(m, SI_ELEMDICT_GET_CUR_RELATION, &[dict_of(m), elem, 0, rel]).eax;
        if m.u16(cur.wrapping_add(0x10)) as i16 as i32 == m.u32(SI_CID_MODELS) as i32 {
            let mdl = m.u32(found.wrapping_add(4));
            if o.call(m, SI_IS_MODEL_DEFORMED, &[mdl]).eax as u16 == 0 {
                o.call(m, SI_MODEL_DELETE_DEFINITION_R, &[mdl, 1]);
            }
        }
        let (a, b) = (m.u32(o8), m.u32(o4));
        o.call(m, SI_ELEMDICT_REMOVE_SERIAL_REL_ID, &[dict_of(m), SI_RD, a, b, 0x4e20, m.u32(cur.wrapping_add(0xc))]);
        found = find(m, o);
    }
    m.release(8);
    0
}

/// [soft 0x08549d58 SI_cusDsoEffectCreateRelations]: for each relation record group of the effect's dso record (+0x14 groups, each with
/// +0x18 argument counts, +4 result kinds, +8 result counts) creates a relation data record (`SI_cusRelDataCreate`) and adds a serial
/// relation for every argument/result pair; for results of the camera or light class it also adds the relation for the camera or
/// light behind the interface element. Answers 0.
pub fn si_cus_dso_effect_create_relations(m: &mut Mem, o: &mut dyn Outside, eff: u32, cls: i32) -> u32 {
    if eff == 0 || cls < 0 || cls > m.u32(SI_RD) as i32 {
        return 0;
    }
    let rec = m.u32(eff.wrapping_add(0x104));
    let status = m.u32(eff.wrapping_add(0x114));
    if rec == 0 || status == 0 {
        return 0;
    }
    let name2 = status.wrapping_add(2);
    let model = m.u32(eff.wrapping_add(0xc));
    let name1 = m.u32(eff.wrapping_add(4));
    let mut acc = 0u32;
    let mut grp_cls = cls as u32;
    let mut i = 0i32;
    while i < m.u32(rec.wrapping_add(0x14)) as i32 {
        let chap_a = o.call(m, SI_RELDICT_GET_CHAP_R, &[SI_RD, grp_cls, 1]).eax;
        let grp_a = o.call(m, SI_RELDICT_GET_GRP_R, &[SI_RD, grp_cls, 1]).eax;
        let nj = m.u32(m.u32(rec.wrapping_add(0x18)).wrapping_add((i as u32).wrapping_mul(4)));
        let mut j = 0i32;
        while j < nj as i32 {
            let serial = o.call(m, SI_ELEMDICT_GET_REL_SERIAL_NUMBER, &[]).eax;
            let elem_id = m.u32(m.u32(rec.wrapping_add(0x20)).wrapping_add(acc.wrapping_add(j as u32).wrapping_mul(20)).wrapping_add(8));
            let mut xcls = cls as u32;
            let mut y = 0u32;
            let mut k = 0i32;
            while k < m.u32(rec.wrapping_add(4)) as i32 {
                let chap_b = o.call(m, SI_RELDICT_GET_CHAP_R, &[SI_RD, xcls, 0]).eax;
                let grp_b = o.call(m, SI_RELDICT_GET_GRP_R, &[SI_RD, xcls, 0]).eax;
                let nl = m.u32(m.u32(rec.wrapping_add(8)).wrapping_add((k as u32).wrapping_mul(4)));
                let mut yoff = y.wrapping_mul(20);
                let mut l = 0i32;
                while l < nl as i32 {
                    let item = m.u32(m.u32(rec.wrapping_add(0x10)).wrapping_add(yoff).wrapping_add(8));
                    let rd = o.call(m, SI_CUS_REL_DATA_CREATE_R, &[model, (i + 1) as u32, (k + 1) as u32, j as u32, l as u32, grp_b, grp_a, name1, name2]).eax;
                    let pair = (k as u32).wrapping_mul(m.u32(rec.wrapping_add(0x14))).wrapping_add(i as u32).wrapping_add(1);
                    o.call(m, SI_ELEMDICT_ADD_SERIAL_RELATION, &[dict_of(m), SI_RD, chap_a, chap_b, grp_a, grp_b, elem_id, item, 0x4e20, serial, pair, 0, rd, 1, 1]);
                    let alt = if chap_b == m.u32(SI_CID_CAMERAS) {
                        Some(o.call(m, SI_ED_GET_CAM_FROM_CAM_INT, &[item]).eax)
                    } else if chap_b == m.u32(SI_CID_LIGHTS) {
                        Some(o.call(m, SI_ED_GET_LIG_FROM_LIG_INT, &[item]).eax)
                    } else {
                        None
                    };
                    if let Some(alt) = alt {
                        if alt != item {
                            let rd = o.call(m, SI_CUS_REL_DATA_CREATE_R, &[model, (i + 1) as u32, (k + 1) as u32, j as u32, l as u32, grp_b, grp_a, name1, name2]).eax;
                            o.call(m, SI_ELEMDICT_ADD_SERIAL_RELATION, &[dict_of(m), SI_RD, chap_a, chap_b, grp_a, grp_b, elem_id, alt, 0x4e20, serial, pair, 0, rd, 1, 1]);
                        }
                    }
                    yoff = yoff.wrapping_add(20);
                    l += 1;
                }
                y = y.wrapping_add(nl);
                xcls = xcls.wrapping_add(m.u32(rec.wrapping_add(0x14)));
                k += 1;
            }
            j += 1;
        }
        acc = acc.wrapping_add(nj);
        grp_cls = grp_cls.wrapping_add(1);
        i += 1;
    }
    0
}

pub const SI_CUS_DELETE_EFFECT_R: Routine = Routine::new("SI_cusDeleteEffect", 0x0854_a348);
pub const SI_CUS_DSO_EFFECT_DESTROY_RELATIONS_R: Routine = Routine::new("SI_cusDsoEffectDestroyRelations", 0x0854_9bc8);
pub const SI_CUSTOM_VALUE_CALLBACK_REMOVE_BY_EFFECT_R: Routine = Routine::new("si_customValueCallbackRemoveByEffect", 0x0852_9b84);
pub const CUS_ELEM_CALLBACK_REMOVE_BY_EFFECT_R: Routine = Routine::new("cusElemCallbackRemoveByEffect", 0x0854_b9ac);
pub const SI_CUS_REL_DATA_CREATE_R: Routine = Routine::new("SI_cusRelDataCreate", 0x0852_e170);
pub const SI_FREEZE_FLAG_GET: Routine = Routine::new("si_freezeFlagGet", 0x0853_ec84);

/// [soft 0x0854a108 cusStatusDeleteIcon]: takes the icon's effect out of the status (`SI_cusRemoveEffect`); with a dso record that has a
/// link: removes the effect's value and element callbacks, undoes the post-definition count for kind 0x73, resolves the cleanup symbols
/// (a function of type 0xb called with a fresh context, else one of type 3 called with the six values of the record), unloads the
/// link, destroys the relations, and deletes the effect unless a cleanup answered non-zero. Rebuilds the f-curve scroll list. Answers 1
/// when the effect was deleted, -1 otherwise (none found, or a cleanup failed).
pub fn cus_status_delete_icon(m: &mut Mem, o: &mut dyn Outside, status: u32, icon: u32) -> i32 {
    let mut edi = 0u32;
    let esi = si_cus_remove_effect(m, status, icon);
    if esi == 0 {
        edi = 0x24;
    }
    if edi == 0 {
        let rec = if esi != 0 { m.u32(esi.wrapping_add(0x104)) } else { 0 };
        if rec != 0 && m.u32(rec) != 0 {
            let loc = m.local(0x20);
            let (l24, l28) = (loc, loc.wrapping_add(4));
            m.set_u32(l24, 0);
            m.set_u32(l28, 0);
            o.call(m, SI_CUSTOM_VALUE_CALLBACK_REMOVE_BY_EFFECT_R, &[esi]);
            o.call(m, CUS_ELEM_CALLBACK_REMOVE_BY_EFFECT_R, &[esi]);
            if m.u16(m.u32(esi.wrapping_add(0x104)).wrapping_add(0x38)) == 0x73 {
                si_cus_dec_nb_post_def_fx(m);
            }
            let link = m.u32(m.u32(esi.wrapping_add(0x104)));
            edi = si_cus_resolve_symbol(m, o, link, 0xb, l24);
            if edi != 0 {
                o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086e_45a0, 3]);
            }
            let mut go = true;
            if m.u32(l24) == 0 {
                if edi != 0 {
                    go = false;
                } else {
                    let link = m.u32(m.u32(esi.wrapping_add(0x104)));
                    edi = si_cus_resolve_symbol(m, o, link, 3, l28);
                    if edi != 0 {
                        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086e_45e0, 3]);
                        go = false;
                    }
                }
            }
            if go {
                let flag = m.local(4);
                o.call(m, SI_FREEZE_FLAG_GET, &[flag]);
                let rec = m.u32(esi.wrapping_add(0x104));
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[esi]);
                if m.u32(l24) != 0 {
                    let ctx = m.local(0x20);
                    let state = if m.u16(flag) != 0 { 0x12 } else { 6 };
                    si_cus_context_init(m, ctx, esi, 0xb, state, 0xffff_ffff);
                    edi = o.call(m, Routine::at(m.u32(l24)), &[ctx]).eax;
                    m.release(0x20);
                } else if m.u32(l28) != 0 {
                    let mut count = m.u32(rec.wrapping_add(0xc));
                    if m.u32(esi.wrapping_add(0xc)) != 0 {
                        count = count.wrapping_sub(1);
                    }
                    let args = [m.u32(esi.wrapping_add(0x64)), count, m.u32(rec.wrapping_add(0x10)), m.u32(rec.wrapping_add(0x1c)), m.u32(rec.wrapping_add(0x20)), rec.wrapping_add(0x24)];
                    edi = o.call(m, Routine::at(m.u32(l28)), &args).eax;
                }
                m.release(4);
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[0]);
            }
            let link = m.u32(m.u32(esi.wrapping_add(0x104)));
            crate::custom_dyn::si_dynamic_link_unload(m, o, link);
            o.call(m, SI_CUS_DSO_EFFECT_DESTROY_RELATIONS_R, &[esi]);
            m.release(0x20);
        }
        if edi == 0 {
            o.call(m, SI_CUS_DELETE_EFFECT_R, &[esi]);
        }
    }
    o.call(m, SI_FCV_SCROLL_BUILD_R, &[]);
    if edi == 0 {
        1
    } else {
        -1
    }
}
