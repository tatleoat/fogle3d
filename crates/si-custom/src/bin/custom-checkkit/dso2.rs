// SPDX-License-Identifier: MIT
//! The ports of unit custom13 (custom_dso.c second part, custom_io cusStatusCreateRelations), kept apart from `main.rs`.

use si_checkkit::Port;
use si_custom::custom_dso2::*;

/// `base` followed by this unit's ports.
pub fn with(base: &[(&'static str, Port)]) -> Vec<(&'static str, Port)> {
    let mut v: Vec<(&'static str, Port)> = base.to_vec();
    v.extend_from_slice(&[
        ("si_cusUpdateEffect", |m, o, a| { si_cus_update_effect_f(m, o, a.v("eff"), a.f("frame"), a.v("node")); ().into() }),
        ("cusPickObject", |m, o, a| cus_pick_object(m, o, a.v("p1"), a.v("res") as u16, a.v("mouse"), a.v("prompt"), a.v("cls"), a.v("p6"), a.v("cond"), a.v("mode") as i32, a.v("maxn") as i32, a.v("idx"), a.v("p11"), a.v("counts"), a.v("total"), a.v("arr")).into()),
        ("cusPickParameters", |m, o, a| cus_pick_parameters(m, o, a.v("eff"), a.v("name"), a.v("id"), a.v("res") as u16, a.v("mode") as u16).into()),
        ("cusSetResultsFromArgs", |m, o, a| cus_set_results_from_args(m, o, a.v("id") as i32, a.v("acounts"), a.v("x"), a.v("rcounts"), a.v("args"), a.v("rtotalp"), a.v("rarrp")).into()),
        ("setArgsAndResults", |m, o, a| set_args_and_results(m, o, a.v("eff"), a.v("id") as i32, a.v("mode") as u16).into()),
        ("dynamicSecLinkLoad", |m, _o, a| (dynamic_sec_link_load(m, a.v("data"), a.v("dialog")) as u32).into()),
        ("si_cusEffectLoadDso", |m, o, a| si_cus_effect_load_dso(m, o, a.v("eff")).into()),
        ("si_cusParseDynamicLink", |m, o, a| si_cus_parse_dynamic_link(m, o, a.v("fp"), a.v("link")).into()),
        ("si_cusReadSecondaryDialog", |m, o, a| si_cus_read_secondary_dialog(m, o, a.v("fp"), a.v("eff")).into()),
        ("cusInstantiatePersistentDsoEffect", |m, o, a| (cus_instantiate_persistent_dso_effect(m, o, a.v("status"), a.v("eff"), a.v("quiet") as u16, a.v("SI_cusParamAdd"), a.v("f5"), a.v("SI_dynamicSymbolListRemoveAll"), a.v("f7"), a.v("id") as i32) as u32).into()),
        ("si_cusDsoEffect", |m, o, a| (si_cus_dso_effect(m, o, a.v("status"), a.v("eff"), a.f("frame"), a.v("quiet") as u16 as i16, a.v("dialog") as u16 as i16) as u32).into()),
        ("si_cusParseSecondaryDynamicLink", |m, o, a| si_cus_parse_secondary_dynamic_link(m, o, a.v("fp"), a.v("link")).into()),
    ]);
    v
}
