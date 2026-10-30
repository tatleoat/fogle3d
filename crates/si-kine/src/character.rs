// SPDX-License-Identifier: MIT
//! `character.c` on the shared memory image: the data side of the dope sheet's character
//! mode, which relations and function-curve tracks show, key saving on a character model
//! with its mirror (finding 0022, "Actor").
//!
//! **Not ported**: `SI_constraintMenu` (0x081dca78, a menu), `SI_relaxConstraintByMouse`
//! (0x081dcc6c, a mouse event loop). Callees outside the file go through [`Outside`].

use si_mem::Mem;
use si_x87::x;

use crate::actout as ao;
use crate::actout::{call, call_eax};
use crate::outside::{si_status_display_message, Outside};

/// `SI_RD` (the relation dictionary, 0x087815f0): rule count at +0, the rules at the
/// pointer at 0x087815f8, one 0x50 bytes each, the active flag at +0x1a.
/// `SI_cid.MODELS` (SI_cid is at 0x08781660), the chapter number of the models.
pub const SI_CID_MODELS: u32 = 0x0878_1680;
pub const SI_RD: u32 = 0x0878_15f0;
const SI_RD_RULES: u32 = 0x0878_15f8;
const RULE_SIZE: u32 = 0x50;
const RULE_ACTIVE: u32 = 0x1a;
/// The dope sheet's show flags (16 bits each).
pub const DOPE_SHOW_CONSTRAINT: u32 = 0x0869_43c0;
pub const DOPE_SHOW_ROTATION: u32 = 0x0869_43c2;
pub const DOPE_SHOW_SCALING: u32 = 0x0869_43c4;
pub const DOPE_SHOW_SYMMETRY: u32 = 0x0869_43c6;
pub const DOPE_SHOW_TRANSLATION: u32 = 0x0869_43c8;
/// `SI_dopeFcvTable`: 0x2a8 16-bit entries, one per function-curve track.
pub const SI_DOPE_FCV_TABLE: u32 = 0x0878_95c0;
pub const FCV_TRACKS: u32 = 0x2a8;
/// `SI_ATD`, the attribute dictionary, passed by address.
pub const SI_ATD: u32 = 0x0878_1438;
/// 16-bit words of the dope sheet: its mode flag and its frame.
pub const DOPE_MODE: u32 = 0x0878_faa8;
pub const DOPE_FRAME: u32 = 0x0878_faac;
pub const SI_KEY_TYPE_PROPAG_MODE: u32 = 0x0869_5e8e;
pub const SI_FRAME_CURRENT: u32 = 0x0878_2040;
pub const SI_FRAME_RATE: u32 = 0x0878_204c;
/// 180/pi, as the double at 0x0864cab0.
pub const RAD_TO_DEG: f64 = 57.29577951308232;
/// The relations `SI_setRelDictFromDopeMode` switches on, per show flag.
const REL_TRANSLATION: &[i32] = &[0x71, 0xa0];
const REL_ROTATION: &[i32] = &[0x65];
const REL_SCALING: &[i32] = &[0x64];
const REL_CONSTRAINT: &[i32] = &[0x66, 0x67, 0x6f, 0x72, 0x73, 0x74, 0x75, 0x76, 0x80];
const REL_SYMMETRY: &[i32] = &[0x96];

/// Activates the relations whose show flag is set, deactivates every other
/// [soft 0x081dc660 SI_setRelDictFromDopeMode].
pub fn si_set_rel_dict_from_dope_mode(m: &mut Mem, o: &mut dyn Outside) {
    let nb = m.i32(SI_RD);
    for i in 0..nb {
        let rules = m.u32(SI_RD_RULES);
        m.set_i16(rules.wrapping_add((i as u32).wrapping_mul(RULE_SIZE)).wrapping_add(RULE_ACTIVE), 0);
    }
    let groups: [(u32, &[i32]); 5] = [
        (DOPE_SHOW_TRANSLATION, REL_TRANSLATION),
        (DOPE_SHOW_ROTATION, REL_ROTATION),
        (DOPE_SHOW_SCALING, REL_SCALING),
        (DOPE_SHOW_CONSTRAINT, REL_CONSTRAINT),
        (DOPE_SHOW_SYMMETRY, REL_SYMMETRY),
    ];
    for (flag, ids) in groups {
        if m.i16(flag) == 0 {
            continue;
        }
        for id in ids {
            let idx = call_eax(o, m, &ao::SI_RELDICT_GET_REL_INDEX_FROM_ID, &[SI_RD, *id as u32]) as i32;
            if idx >= 0 {
                let rules = m.u32(SI_RD_RULES);
                m.set_i16(rules.wrapping_add((idx as u32).wrapping_mul(RULE_SIZE)).wrapping_add(RULE_ACTIVE), 1);
            }
        }
    }
}

/// Rebuilds the function-curve track table from the show flags and hands it to the
/// attribute dictionary [soft 0x081dc924 SI_setDopFcvTableFromDopeMode].
pub fn si_set_dop_fcv_table_from_dope_mode(m: &mut Mem, o: &mut dyn Outside) {
    call(o, m, &ao::SI_DOPE_FCV_TABLE_SET, &[0]);
    if m.i16(DOPE_SHOW_TRANSLATION) != 0 {
        for a in [0x0878_96d6u32, 0x0878_96d8, 0x0878_96da] {
            m.set_i16(a, 1);
        }
    }
    if m.i16(DOPE_SHOW_ROTATION) != 0 {
        for a in [0x0878_95c8u32, 0x0878_95ca, 0x0878_95cc] {
            m.set_i16(a, 1);
        }
    }
    if m.i16(DOPE_SHOW_SCALING) != 0 {
        for a in [0x0878_95c2u32, 0x0878_95c4, 0x0878_95c6] {
            m.set_i16(a, 1);
        }
    }
    for i in 0..FCV_TRACKS {
        let v = m.i16(SI_DOPE_FCV_TABLE + 2 * i) as i32;
        call(o, m, &ao::SI_ATTDICT_SET_FCURVE_FLAG, &[SI_ATD, i, v as u32]);
    }
}

/// Runs one operation on the active curves of every selected element that has a graf node
/// [soft 0x081dc9d8 dopePerformOperOnKeyInterval]. `a`, `b`, `c` are floats passed as bits.
pub fn dope_perform_oper_on_key_interval(m: &mut Mem, o: &mut dyn Outside, dict: u32, op: i32, a: u32, b: u32, c: u32) {
    let mut chap = 0u32;
    while chap < m.u32(dict.wrapping_add(0)) {
        let list = m.u32(dict.wrapping_add(4)).wrapping_add(chap.wrapping_mul(0x40));
        let mut k = 0u32;
        while k < m.u32(list.wrapping_add(4)) {
            let elem = m.u32(list.wrapping_add(8)).wrapping_add(k.wrapping_mul(0x84));
            if m.i16(elem.wrapping_add(0x48)) == 0 && m.u32(elem.wrapping_add(8)) != 0 && m.i16(elem.wrapping_add(0x42)) != 0 {
                call(
                    o,
                    m,
                    &ao::SI_ELEMDICT_ACTIVE_FCV_OPER,
                    &[dict, elem, chap, k, op as u32, 0, a, b, c, 0, 0, 0, 0],
                );
            }
            k += 1;
        }
        chap += 1;
    }
}

/// Saves the keys of one element of a character at a frame: every active relation's
/// channels (rotation in degrees), or removes the keys at that time when `mode` is not
/// zero [soft 0x081dd174 savekeyCharacter].
pub fn savekey_character(m: &mut Mem, o: &mut dyn Outside, elem: u32, idx: i32, frame: i32, mode: i16) {
    let saved = m.i32(SI_FRAME_CURRENT);
    m.set_i32(SI_FRAME_CURRENT, frame);
    let model = m.u32(elem.wrapping_add(4));
    let t = (x(frame as f32) / x(m.f32(SI_FRAME_RATE))).to_f32();
    let mut count = 0;
    let nb = m.i32(SI_RD);
    for i in 0..nb {
        let rules = m.u32(SI_RD_RULES);
        let rule = rules.wrapping_add((i as u32).wrapping_mul(RULE_SIZE));
        if m.i16(rule.wrapping_add(RULE_ACTIVE)) == 0 {
            continue;
        }
        let relid = m.i32(rule);
        if m.i16(DOPE_MODE) != 0 {
            let r = call_eax(o, m, &ao::SI_DOPE_IS_REL_ACTIVE, &[relid as u32]) as u16;
            if r == 0 {
                continue;
            }
        }
        count += 1;
        if mode == 0 {
            let key = |m: &mut Mem, o: &mut dyn Outside, comp: u32, off: u32, deg: bool| {
                let raw = m.f32(model.wrapping_add(off));
                let v = if deg { (x(RAD_TO_DEG) * x(raw)).to_f32() } else { raw };
                call(o, m, &ao::SI_SAVEKEY_MODEL, &[model, 1, relid as u32, comp, 1, v.to_bits(), 0, 0]);
            };
            match relid {
                0x65 => {
                    key(m, o, 4, 0x10c, true);
                    key(m, o, 5, 0x110, true);
                    key(m, o, 6, 0x114, true);
                }
                0x64 => {
                    key(m, o, 1, 0x100, false);
                    key(m, o, 2, 0x104, false);
                    key(m, o, 3, 0x108, false);
                }
                0x71 => {
                    let base = if m.i32(model.wrapping_add(0x2ec)) == 3 { 0x1b8 } else { 0x158 };
                    key(m, o, 0x8b, base, false);
                    key(m, o, 0x8c, base + 4, false);
                    key(m, o, 0x8d, base + 8, false);
                }
                _ => {}
            }
        } else if relid >= 100 && (relid <= 0x65 || relid == 0x71) {
            call(o, m, &ao::SI_FCURVE_CHECK_REMOVE_KEY3, &[elem, idx as u32, relid as u32, t.to_bits()]);
        }
    }
    if count == 0 {
        si_status_display_message(o, m, ao::S_NO_ACTIVE_RELATION, 1);
    }
    m.set_i32(SI_FRAME_CURRENT, saved);
}

/// Defines a key on a character model and its mirror: with a symmetry pair the frames of
/// the pair's sampling interval alternate between the two sides
/// [soft 0x081dd47c SI_mdlCharDefineKey].
pub fn si_mdl_char_define_key(m: &mut Mem, o: &mut dyn Outside, model: u32, frame: i32, mode: i16) {
    let slots = m.local(12);
    let (idx_a, idx_b, data_p) = (slots, slots + 4, slots + 8);
    let mut e1 = call_eax(o, m, &ao::SI_ED_GET_ELEM_FROM_MODEL, &[model, idx_a]);
    let mut i1 = m.i32(idx_a);
    if m.i16(DOPE_MODE) != 0 {
        let r = call_eax(o, m, &ao::SI_DOPE_IS_ELEM_ACTIVE, &[m.u32(SI_CID_MODELS), i1 as u32]) as u16;
        if r == 0 {
            m.release(12);
            return;
        }
    }
    let mut e2 = call_eax(o, m, &ao::SI_SYMMETRY_GET_ELEM, &[model, idx_b, data_p, 1, 0]);
    let mut i2 = m.i32(idx_b);
    let mut flag = false;
    let mut ebx = 0i32;
    let mut cur = (0u32, 0i32);
    let mut step = 0i32;
    let mut start = 0i32;
    if e2 != 0 {
        let d = m.u32(data_p);
        step = m.i16(d.wrapping_add(2)) as i32;
        start = m.i16(d.wrapping_add(4)) as i32;
        let n = m.i16(d.wrapping_add(6)) as i32;
        if frame >= start && frame <= (n + 1) * step + start {
            if step == 0 {
                crate::fault::note();
                m.release(12);
                return;
            }
            let q = (frame - start) / step;
            let r = (frame - start) % step;
            flag = true;
            if q & 1 == 0 {
                std::mem::swap(&mut e1, &mut e2);
                std::mem::swap(&mut i1, &mut i2);
            }
            cur = (e2, i2);
            ebx = r + start;
        }
    }
    if flag {
        loop {
            let d = m.u32(data_p);
            let n = m.i16(d.wrapping_add(6)) as i32;
            if ebx >= start + step * (n + 1) {
                break;
            }
            m.set_i16(SI_KEY_TYPE_PROPAG_MODE, (ebx != frame) as i16);
            savekey_character(m, o, cur.0, cur.1, ebx, mode);
            cur = if cur.0 == e1 { (e2, i2) } else { (e1, i1) };
            ebx += step;
        }
    } else {
        savekey_character(m, o, e1, i1, frame, mode);
    }
    m.release(12);
}
