// SPDX-License-Identifier: MIT
//! The ports of unit cus1 (finding 0041, 2026-10-29): the dirty-marking subtree of the custom-effect scene transfer, the scene update,
//! cusTmpDir and si_cusDialogDsoDataCreate. Chained from `shdb7::with`; chains `last2::with`.

use si_checkkit::Port;
use si_custom::custom_mn::*;
use si_matter::matter_copy::*;

/// `base` followed by this unit's ports.
pub fn with(base: &[(&'static str, Port)]) -> Vec<(&'static str, Port)> {
    let mut v: Vec<(&'static str, Port)> = base.to_vec();
    v.extend_from_slice(&[
        ("SI_currentCustomEffectSet", |m, _o, a| { si_current_custom_effect_set(m, a.v("effect")); ().into() }),
        ("setReadingAnimModel", |m, _o, a| { set_reading_anim_model(m, a.v("model")); ().into() }),
        ("si_cusProcessBasedEffect", |m, o, a| si_cus_process_based_effect(m, o, a.v("a8"), a.v("rec"), a.v("a10"), a.v("quiet"), a.v("all"), a.v("a1c")).into()),
        ("cusCompositeScanLine", |m, _o, a| { cus_composite_scan_line(m, a.v("d1"), a.v("d2"), a.v("d3"), a.v("da"), a.v("zd"), a.v("s1"), a.v("s2"), a.v("s3"), a.v("sa"), a.v("zs"), a.v("n") as i32, a.v("mode") as i32); ().into() }),
        ("cusCompositeCmd", |m, o, a| cus_composite_cmd(m, o, a.v("cmd")).into()),
        ("cusTmpDir", |m, o, a| cus_tmp_dir(m, o, a.v("name")).into()),
        ("si_cusDialogDsoDataCreate", |m, _o, a| si_cus_dialog_dso_data_create(m, a.v("dlg")).into()),
        ("cusEffectNotifyForCusValChanges", |m, o, a| { cus_effect_notify_for_cus_val_changes(m, o, a.v("list")); ().into() }),
        ("cusStatusPreSceneTransfer", |m, o, a| { cus_status_pre_scene_transfer(m, o, a.v("status"), a.v("notify") as u16); ().into() }),
        ("cusExpressSetDirty", |m, o, a| cus_express_set_dirty(m, o, a.v("dict"), a.v("chap"), a.v("idx"), a.v("elem"), a.v("ftype"), a.v("name")).into()),
        ("cusFcurveCusValSetDirty", |m, o, a| cus_fcurve_cus_val_set_dirty(m, o, a.v("head")).into()),
        ("cusValueMarkAsDirty", |m, o, _a| cus_value_mark_as_dirty(m, o).into()),
        ("SI_cusPreSceneTransfer", |m, o, _a| { si_cus_pre_scene_transfer(m, o); ().into() }),
        ("cusSceneUpdateStatus", |m, o, a| { cus_scene_update_status(m, o, a.v("status"), a.v("frame")); ().into() }),
        ("SI_cusSceneUpdate", |m, o, a| { si_cus_scene_update(m, o, a.v("frame")); ().into() }),
        ("textureCopyDisableUVOnMesh", |m, _o, a| { texture_copy_disable_uv_on_mesh(m, a.v("model"), a.v("tex")); ().into() }),
        ("matterCopySchemInfo", |m, o, a| { matter_copy_schem_info(m, o, a.v("src_item"), a.v("dst_item"), a.v("item_cid"), a.v("src_tex"), a.v("dst_tex"), a.v("tex_cid")); ().into() }),
        ("materialAddDefault", |m, o, a| { material_add_default(m, o, a.v("model")); ().into() }),
        ("txtCopyFromSetupLcl", |m, o, a| { txt_copy_from_setup_lcl(m, o, a.v("model"), a.v("src_mat"), a.v("dst_mat"), a.v("all") as i16, a.v("uv") as i16); ().into() }),
        ("SI_txtCopyFromSetup", |m, o, a| { si_txt_copy_from_setup(m, o, a.v("src"), a.v("dst"), a.v("all") as i16, a.v("uv") as i16); ().into() }),
        ("SI_materialCopyShareSetup", |m, o, a| { si_material_copy_share_setup(m, o, a.v("src"), a.v("dst")); ().into() }),
        ("incrustInit", |m, o, _a| { incrust_init(m, o); ().into() }),
        ("SI_matterInit", |m, o, _a| { si_matter_init(m, o); ().into() }),
    ]);
    crate::last2::with(&v)
}
