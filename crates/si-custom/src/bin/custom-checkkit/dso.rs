// SPDX-License-Identifier: MIT
//! The ports of unit custom12 (custom_dso.c run-time functions, cusStatusCreateRelations and the SI_cusCmd pieces),
//! kept apart from `main.rs` so the lists merge cleanly.

use crate::files;
use si_checkkit::Port;
use si_custom::custom_dso::*;

/// The case's `FILE *`: the argument `fp` when the case gives one (a null or made-up pointer), else the case's stream.
fn fp(a: &si_checkkit::Args) -> u32 {
    if a.has("fp") { a.v("fp") } else { files::begin(a); files::handle() }
}

/// `base` followed by this unit's ports.
pub fn with(base: &[(&'static str, Port)]) -> Vec<(&'static str, Port)> {
    let mut v: Vec<(&'static str, Port)> = base.to_vec();
    v.extend_from_slice(&[
        ("cusReadLine", |m, o, a| { let f = fp(a); cus_read_line(m, o, f, a.v("size"), a.v("out")).into() }),
        ("SI_cusParseOperatorOptions", |m, o, a| { files::begin(a); si_cus_parse_operator_options(m, o, files::handle(), a.v("eff"), a.v("rank")).into() }),
        ("SI_cusRelDataCreate", |m, o, a| {
            super::udf3::ED_INDEX_FROM_MODEL.with(|c| c.set(true));
            let r = si_cus_rel_data_create(m, o, a.v("model"), a.v("a"), a.v("b"), a.v("c"), a.v("d"), a.v("e"), a.v("f"), a.v("n1"), a.v("n2"));
            super::udf3::ED_INDEX_FROM_MODEL.with(|c| c.set(false));
            r.into()
        }),
        ("SI_cusResolveSymbol", |m, o, a| si_cus_resolve_symbol(m, o, a.v("link"), a.v("ty"), a.v("out")).into()),
        ("SI_cusExit", |m, o, a| { si_cus_exit(m, o, a.v("state")); ().into() }),
        ("cusEffectListProcessExit", |m, o, a| { cus_effect_list_process_exit(m, o, a.v("list"), a.v("state")); ().into() }),
        ("cusEffectListProcessCustomFunctions", |m, o, a| { cus_effect_list_process_custom_functions(m, o, a.v("list"), a.v("kind"), a.v("state")); ().into() }),
        ("SI_cusCallCustomFunctions", |m, o, a| { si_cus_call_custom_functions(m, o, a.v("kind"), a.v("state")); ().into() }),
        ("SI_cusParamAdd", |m, _o, a| si_cus_param_add(m, a.v("src"), a.v("idx") as i32, a.v("counts"), a.v("total"), a.v("arr")).into()),
        ("si_cusDsoAllocArgsRes", |m, o, a| si_cus_dso_alloc_args_res(m, o, a.v("rec"), a.v("id"), a.v("nargs"), a.v("nres"), a.v("args_out"), a.v("res_out")).into()),
        ("si_cusDsoAllocOccurrenceArrays", |m, o, a| si_cus_dso_alloc_occurrence_arrays(m, o, a.v("rec"), a.v("id"), a.v("nargs"), a.v("nres"), a.v("args_out"), a.v("res_out")).into()),
        ("cusVerifyParams", |m, o, a| cus_verify_params(m, o, a.v("n") as i32, a.v("counts"), a.v("arr"), a.v("id"), a.v("flag")).into()),
        ("cusVerifyInstancePicked", |m, o, a| cus_verify_instance_picked(m, o, a.v("rel"), a.v("dict"), a.v("inst"), a.v("cls"), a.v("id"), a.v("res")).into()),
        ("cusGenPickMsg", |m, o, a| cus_gen_pick_msg(m, o, a.v("cls"), a.v("vals"), a.v("name"), a.v("res"), a.v("num")).into()),
        ("branchDefGeomUpdateNode", |m, _o, a| branch_def_geom_update_node(m, a.v("node")).into()),
        ("SI_cusResortUpdList", |m, o, a| si_cus_resort_upd_list(m, o, a.v("list")).into()),
        ("SI_cusUpdateEffect", |m, o, a| { si_cus_update_effect(m, o, a.v("node"), a.f("frame")); ().into() }),
        ("cusResetGeometryDefinition", |m, o, a| { cus_reset_geometry_definition(m, o, a.v("node")); ().into() }),
        ("cusUpdSubListResort", |m, o, a| (cus_upd_sub_list_resort(m, o, a.v("list"), a.v("n") as i32) as u32).into()),
        ("processBranchDef", |m, o, a| process_branch_def(m, o, a.v("rec"), a.v("cur"), a.v("list"), a.v("npd") as i32, a.v("post")).into()),
        ("cusUpdListResortForBranchDef", |m, o, a| { cus_upd_list_resort_for_branch_def(m, o, a.v("list")); ().into() }),
        ("SI_cusSetCurrentDsoEffect", |m, _o, a| { si_cus_set_current_dso_effect(m, a.v("eff")); ().into() }),
    ]);
    v
}
