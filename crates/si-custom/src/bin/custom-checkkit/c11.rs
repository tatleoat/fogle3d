// SPDX-License-Identifier: MIT
//! The ports of unit custom11 (the data-string parser chain and the rest of the run-time files), kept apart
//! from `main.rs` and `cv.rs` so the lists merge cleanly.

use si_checkkit::Port;
use si_custom::custom_fcv::*;
use si_custom::custom_parse::*;

/// `base` followed by this unit's ports.
pub fn with(base: &[(&'static str, Port)]) -> Vec<(&'static str, Port)> {
    let mut v: Vec<(&'static str, Port)> = base.to_vec();
    v.extend_from_slice(&[
        ("cusParseFcvkeyString", |m, _o, a| { let mut sv = 0; (cus_parse_fcvkey_string(m, a.v("key"), a.v("lines"), a.v("count") as i32, &mut sv) as u32).into() }),
        ("cusParseAnimationString", |m, o, a| { let mut sv = 0; (cus_parse_animation_string(m, o, a.v("udf"), a.v("dlg"), a.v("lines"), a.v("count") as i32, &mut sv) as u32).into() }),
        ("SI_cusParseDataString", |m, o, a| si_cus_parse_data_string(m, o, a.v("udf"), a.v("dlg"), a.v("s"), a.v("keep")).into()),
        ("SI_cusDialogReadData", |m, o, a| (si_cus_dialog_read_data(m, o, a.v("dlg"), a.v("fp")) as u32).into()),
        ("SI_cusDialogItemCallbackAdd", |m, _o, a| (si_cus_dialog_item_callback_add(m, a.v("eff"), a.v("item") as i32, a.v("func"), a.v("data")) as u32).into()),
        ("SI_cusDialogItemCallbackRemove", |m, _o, a| (si_cus_dialog_item_callback_remove(m, a.v("eff"), a.v("item") as i32, a.v("func"), a.v("data")) as u32).into()),
        ("SI_cusDialogItemCallbackRemoveAll", |m, _o, a| (si_cus_dialog_item_callback_remove_all(m, a.v("eff"), a.v("item") as i32) as u32).into()),
        ("findDiffValue", |m, o, a| find_diff_value(m, o, a.v("val"), a.v("want"), a.v("nkeys") as i32, a.v("frame") as i32, a.v("comp")).into()),
        ("areValuesSame", |m, o, a| are_values_same(m, o, a.v("a"), a.v("dlg"), a.v("frame") as i32).into()),
        ("SI_customValueCallbackCallAll", |m, o, a| (si_custom_value_callback_call_all(m, o, a.v("eff"), a.v("val"), a.v("flag")) as u32).into()),
        ("SI_shaderFcurveOffsetAndScaleAll", |m, o, a| { si_shader_fcurve_offset_and_scale_all(m, o, fl(a)); ().into() }),
        ("SI_shaderFcurveOffsetAndScaleSelected", |m, o, a| { si_shader_fcurve_offset_and_scale_selected(m, o, fl(a)); ().into() }),
        ("matAndLocTxtShaderOffsetAndScale", |m, o, a| { mat_and_loc_txt_shader_offset_and_scale(m, o, a.v("rec"), fl(a)); ().into() }),
        ("SI_customFcurveOffsetAndScaleSelected", |m, o, a| { si_custom_fcurve_offset_and_scale_selected(m, o, fl(a)); ().into() }),
        ("SI_customFcurveOffsetAndScaleAll", |m, o, a| { si_custom_fcurve_offset_and_scale_all(m, o, fl(a)); ().into() }),
        ("SI_customInitDataSet", |m, _o, a| { si_custom_init_data_set(m, a.v("v")); ().into() }),
        ("SI_customInitDataGet", |m, _o, _a| si_custom_init_data_get(m).into()),
        ("SI_cusDsoSetRefreshFlag", |m, _o, a| { si_cus_dso_set_refresh_flag(m, a.v("v")); ().into() }),
        ("SI_cusDsoGetRefreshFlag", |m, _o, _a| si_cus_dso_get_refresh_flag(m).into()),
        ("SI_cusGetCurrentDsoEffect", |m, _o, _a| si_cus_get_current_dso_effect(m).into()),
        ("SI_cusRelDataSetDscId", |m, _o, a| (si_cus_rel_data_set_dsc_id(m, a.v("rd"), a.v("id") as i32) as u32).into()),
        ("cusInitCustomElemArray", |m, _o, a| (cus_init_custom_elem_array(m, a.v("n") as i32, a.v("arr")) as u32).into()),
        ("symbolAlreadyRead", |m, _o, a| (symbol_already_read(m, a.v("n") as i32, a.v("arr"), a.v("key")) as u32).into()),
        ("cusUpdNodeGetEffect", |m, o, a| cus_upd_node_get_effect(m, o, a.v("node")).into()),
        ("SI_cusDialogGetDirtyFlag", |m, _o, a| (si_cus_dialog_get_dirty_flag(m, a.v("eff")) as u32).into()),
        ("SI_cusDialogSetDirtyFlag", |m, _o, a| { si_cus_dialog_set_dirty_flag(m, a.v("eff"), a.v("v")); 0u32.into() }),
        ("si_cusDialogItemCallbackCallAll", |m, o, a| (si_cus_dialog_item_callback_call_all(m, o, a.v("eff"), a.v("item") as i32, a.v("flag")) as u32).into()),
    ]);
    v
}

/// `SI_fcurveFrameValue(fc, frame)` as the generator's stand-in answers it: the float in the word at +0x14 of the curve.
pub fn fcurve_frame_value(m: &mut si_mem::Mem, a: &[u32]) -> si_mem::Ret {
    let w = m.u32(a[0].wrapping_add(0x14));
    let (sign, e, fr) = ((w >> 31) as u16, (w >> 23) & 0xff, (w & 0x7f_ffff) as u64);
    let (mant, se): (u64, u16) = if e == 0 && fr == 0 { (0, sign << 15) } else { ((1u64 << 63) | (fr << 40), (sign << 15) | (e as u16 + 16383 - 127)) };
    let mut r = [0u8; 10];
    r[..8].copy_from_slice(&mant.to_le_bytes());
    r[8..].copy_from_slice(&se.to_le_bytes());
    si_mem::Ret { eax: 0, edx: 0, st0: Some(r) }
}

fn fl(a: &si_checkkit::Args) -> [u32; 5] {
    [a.v("f0"), a.v("f1"), a.v("f2"), a.v("f3"), a.v("f4")]
}
