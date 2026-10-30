// SPDX-License-Identifier: MIT
//! The ports of unit shdb5 (finding 0041, 2026-10-29): tags, callback removers, dso relations, cusStatusDeleteIcon.
//! Chained from `shdb4::with`.

use si_checkkit::Port;
use si_custom::custom_util2::*;

/// `base` followed by this unit's ports.
pub fn with(base: &[(&'static str, Port)]) -> Vec<(&'static str, Port)> {
    let mut v: Vec<(&'static str, Port)> = base.to_vec();
    v.extend_from_slice(&[
        ("si_cusComputeTag", |m, _o, a| { si_cus_compute_tag(m, a.v("obj")); ().into() }),
        ("si_cusReadTag", |m, o, a| { si_cus_read_tag(m, o, a.v("fp"), a.v("obj")); ().into() }),
        ("si_cusValueInDialog", |m, _o, a| si_cus_value_in_dialog(m, a.v("dlg"), a.v("val")).into()),
        ("setCustomPreviewUDF", |m, _o, a| { set_custom_preview_udf(m, a.v("cdg"), a.v("udf")); ().into() }),
        ("unsetCustomPreviewUDF", |m, _o, _a| { unset_custom_preview_udf(m); ().into() }),
        ("SI_customValueCallbackAdd", |m, o, a| si_custom_value_callback_add(m, o, a.v("status"), a.v("dict"), a.v("graf"), a.v("w1"), a.v("w2"), a.v("user")).into()),
        ("SI_customValueCallbackRemove", |m, o, a| si_custom_value_callback_remove(m, o, a.v("status"), a.v("dict"), a.v("graf"), a.v("w1"), a.v("w2"), a.v("user")).into()),
        ("customValueCallbackRemoveByEffect", |m, _o, a| custom_value_callback_remove_by_effect(m, a.v("eff"), a.v("list")).into()),
        ("si_customValueCallbackRemoveByEffect", |m, _o, a| si_custom_value_callback_remove_by_effect(m, a.v("eff")).into()),
        ("cusCheckFcurveChange", |m, _o, a| { cus_check_fcurve_change(m, a.v("dlg"), a.v("vals"), a.v("id") as u16 as i16, a.v("f1"), a.v("f2")); ().into() }),
        ("cusElemCallbackRemoveByEffect", |m, _o, a| { cus_elem_callback_remove_by_effect(m, a.v("eff")); ().into() }),
        ("SI_cusDsoEffectDestroyRelations", |m, o, a| si_cus_dso_effect_destroy_relations(m, o, a.v("eff")).into()),
        ("SI_cusDsoEffectCreateRelations", |m, o, a| si_cus_dso_effect_create_relations(m, o, a.v("eff"), a.v("cls") as i32).into()),
        ("cusStatusDeleteIcon", |m, o, a| (cus_status_delete_icon(m, o, a.v("status"), a.v("icon")) as u32).into()),
    ]);
    crate::shdb7::with(&v)
}

/// The tape of the `SI_byteswap_fread` stand-in of the `si_cusReadTag` cases: a word (the read position) then the bytes; each call copies
/// size*count bytes from the tape to its first argument and advances (the generator's answer function does the same in the emulator).
const TAPE: u32 = 0x0878_8b00;

pub fn effect(m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32], answer: u32) -> Result<(), String> {
    if r.addr == 0x083f_9ba0 && a.len() == 4 {
        let n = a[1].wrapping_mul(a[2]);
        let pos = m.u32(TAPE);
        for i in 0..n {
            let b = m.u8(TAPE + 4 + pos.wrapping_add(i));
            m.set_u8(a[0].wrapping_add(i), b);
        }
        m.set_u32(TAPE, pos.wrapping_add(n));
    }
    crate::shdb3::effect(m, r, a, answer)
}
