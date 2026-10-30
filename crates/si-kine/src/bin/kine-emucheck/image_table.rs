// SPDX-License-Identifier: MIT
//! The image-form routines `kine-emucheck` knows: for each, how the port is called with a
//! case's arguments. The comparison itself is `image::check`.

use si_kine::{cnsdialog, cnsedit, norconstr, onimage, pivot, skeimage, tanconstr, threepts};
use si_x87::vectors::{Case, Check, Verdict};

use crate::image::check;

fn nor_constr_mdl_at_vertex(c: &Case) -> Verdict {
    check(c, |m, _o, a| norconstr::nor_constr_mdl_at_vertex(m, a.p("mesh"), a.vec("p"), a.p("out")).into())
}

fn point_in_concave_polygon(c: &Case) -> Verdict {
    check(c, |m, _o, a| norconstr::point_in_concave_polygon(m, a.vec("p"), a.p("poly"), a.p("mesh")).into())
}

fn nor_constr_find_close_poly(c: &Case) -> Verdict {
    check(c, |m, _o, a| norconstr::nor_constr_find_close_poly(m, a.vec("p"), a.i("index"), a.p("mesh"), a.p("dist"), a.p("flag")).into())
}

fn nor_vector_from_mesh(c: &Case) -> Verdict {
    check(c, |m, _o, a| norconstr::nor_vector_from_mesh(m, a.p("mesh"), a.vec("p"), a.p("out")).into())
}

fn nor_constr_compute_normal_vec(c: &Case) -> Verdict {
    check(c, |m, o, a| norconstr::nor_constr_compute_normal_vec(m, o, a.p("node"), a.p("surface"), a.p("out")).into())
}

fn tan_constr_find_close_point(c: &Case) -> Verdict {
    check(c, |m, o, a| tanconstr::tan_constr_find_close_point(m, o, a.p("spline"), a.i("step"), a.vec("p"), a.p("len"), a.p("pos")).into())
}

fn tan_constr_compute_tan_vector(c: &Case) -> Verdict {
    check(c, |m, o, a| tanconstr::tan_constr_compute_tan_vector(m, o, a.p("node"), a.p("path"), a.p("out")).into())
}

fn tan_vector_from_path(c: &Case) -> Verdict {
    check(c, |m, o, a| tanconstr::tan_vector_from_path(m, o, a.p("data"), a.p("path"), a.p("out")).into())
}

fn constr_3pnts_form_face(c: &Case) -> Verdict {
    check(c, |m, o, _a| threepts::constr_3pnts_form_face(m, o).into())
}

fn check_low(c: &Case) -> Verdict {
    check(c, |m, _o, a| pivot::check_low(m, a.p("point"), a.i("index"), a.p("data")).into())
}

fn checklowest_rec(c: &Case) -> Verdict {
    check(c, |m, o, a| pivot::checklowest_geometry_vertex_rec(m, o, a.p("node"), a.p("data")).into())
}

fn checklowest(c: &Case) -> Verdict {
    check(c, |m, o, a| pivot::checklowest_geometry_vertex(m, o, a.p("node"), a.p("plane"), a.p("lowest"), a.p("point"), a.p("index"), a.p("model")).into())
}

fn pivot_constr_init(c: &Case) -> Verdict {
    check(c, |m, o, a| pivot::anim_pivot_constr_init(m, o, a.p("upd")).into())
}

fn scene_constraint_init(c: &Case) -> Verdict {
    check(c, |m, o, a| pivot::scene_constraint_init(m, o, a.p("list")).into())
}

fn pivot_check_transition(c: &Case) -> Verdict {
    check(c, |m, _o, a| pivot::anim_pivot_check_transition(m, a.p("upd")).into())
}

fn pivot_time(c: &Case) -> Verdict {
    check(c, |m, o, a| pivot::anim_pivot_time(m, o, a.p("upd"), a.f("time")).into())
}

fn pivot_plane_update(c: &Case) -> Verdict {
    check(c, |m, o, a| pivot::anim_model_pivot_plane_update(m, o, a.p("node"), a.p("plane"), a.i("unused"), a.p("upd")).into())
}

fn bound_plane_update(c: &Case) -> Verdict {
    check(c, |m, o, a| pivot::anim_model_bound_plane_update(m, o, a.p("node"), a.p("plane"), a.f("height"), a.i("select"), a.s("unused")).into())
}

fn item_set_obj_pnt_cns(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsedit::item_set_obj_pnt_cns(m, o, a.p("node"), a.p("other")).into())
}

fn item_set_cluster_constraint(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsedit::item_set_cluster_constraint(m, o, a.p("node"), a.p("other"), a.i("relation"), a.p::<u8>("data").addr()).into())
}

fn model_has_constrained_clusters(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsedit::model_has_constrained_clusters(m, o, a.p("node")).into())
}

fn quick_stretch_active(c: &Case) -> Verdict {
    check(c, |m, o, a| si_kine::image::qstretch::quick_stretch_active(m, o, a.p::<u8>("model").addr()).into())
}

fn item_relax_constraint(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsedit::item_relax_constraint(m, o, a.p::<u8>("item").addr(), a.i("kind")).into())
}

fn any_visible_camera_constraints(c: &Case) -> Verdict {
    check(c, |m, _o, _a| cnsedit::any_visible_camera_constraints(m).into())
}

fn relax_cns_link_from_data(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsedit::relax_cns_link_from_data(m, o, a.p("node"), a.i("relation")).into())
}

fn constraint_relax_from_data(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsedit::constraint_relax_from_data(m, o, a.p("item"), a.i("kind")).into())
}

fn build_local_update_list(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsedit::build_local_update_list(m, o, a.i("group"), a.s("flag")).into())
}

fn get_constraint_update_list(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsedit::get_constraint_update_list(m, o, a.i("group"), a.s("camera")).into())
}

fn item_set_constraint(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsedit::item_set_constraint(m, o, a.p("item"), a.i("kind"), a.p("other"), a.i("other_kind"), a.s("relation")).into())
}

fn image_ske_chain_root(c: &Case) -> Verdict {
    check(c, |m, _o, a| onimage::ske_chain_root(m, a.p("node")).into())
}

fn image_ik_valid_constraint(c: &Case) -> Verdict {
    check(c, |m, _o, a| onimage::ik_valid_constraint(m, a.p("item"), a.i("kind"), a.p("other"), a.i("other_kind")).into())
}

fn image_tan_create(c: &Case) -> Verdict {
    check(c, |m, _o, _a| onimage::tan_constr_create_data(m).into())
}

fn image_rotlim_create(c: &Case) -> Verdict {
    check(c, |m, _o, _a| onimage::rotlim_create_data(m).into())
}

fn image_poslim_create(c: &Case) -> Verdict {
    check(c, |m, _o, _a| onimage::poslim_create_data(m).into())
}

fn image_pref_angle_update(c: &Case) -> Verdict {
    check(c, |m, o, a| onimage::inv_kin_pref_angle_update(m, o, a.p("node")).into())
}

fn scene_constr_update(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsedit::scene_constr_update(m, o, a.i("group"), a.f("time"), a.p::<u8>("vector").addr()).into())
}

fn image_joint_number(c: &Case) -> Verdict {
    check(c, |m, _o, a| skeimage::ske_joint_number(m, a.p("root")).into())
}

fn image_total_length(c: &Case) -> Verdict {
    check(c, |m, _o, a| skeimage::ske_chain_total_length(m, a.p("root")).into())
}

fn ske_joint_draw(c: &Case) -> Verdict {
    check(c, |m, o, a| skeimage::ske_joint_draw(m, o, a.p("node"), a.s("center")).into())
}

fn ske_spline_to_chain(c: &Case) -> Verdict {
    check(c, |m, o, a| skeimage::ske_spline_to_chain(m, o, a.p("spline")).into())
}

fn ske_tubing(c: &Case) -> Verdict {
    check(c, |m, o, a| skeimage::ske_tubing(m, o, a.p("root")).into())
}

fn ske_tubing_update(c: &Case) -> Verdict {
    check(c, |m, o, a| skeimage::ske_tubing_update(m, o, a.p("root")).into())
}

fn ske_auto_envelope(c: &Case) -> Verdict {
    check(c, |m, o, a| skeimage::ske_auto_envelope(m, o, a.p("root"), a.p("profile"), a.i("steps"), a.s("cap_end"), a.s("cap_start"), a.s("nurbs")).into())
}

fn limit_freeze_set(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsdialog::limit_freeze_set(m, o, a.v("dialog"), a.s("toggle"), a.s("field"), a.s("state")).into())
}

fn limit_freeze_toggle(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsdialog::limit_freeze_toggle(m, o, a.v("dialog"), a.s("toggle"), a.s("field")).into())
}

fn limit_ghost_set(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsdialog::limit_ghost_set(m, o, a.v("dialog"), a.s("toggle"), a.s("field"), a.s("ghost")).into())
}

fn rotlim_default(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsdialog::rotlim_default(m, o, a.i("button"), a.v("dialog"), a.p::<u8>("data").addr(), a.p("node")).into())
}

fn rotlim_warning(c: &Case) -> Verdict {
    check(c, |m, o, a| cnsdialog::rotlim_warning(m, o, a.v("dialog"), a.s("was")).into())
}

fn image_inv_kin_dump(c: &Case) -> Verdict {
    check(c, |m, o, a| onimage::inv_kin_dump(m, o, a.p::<u8>("rec").addr()).into())
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "norConstrMdlAtVertex" => nor_constr_mdl_at_vertex,
        "pointInConcavePolygon" => point_in_concave_polygon,
        "norConstrFindClosePoly" => nor_constr_find_close_poly,
        "norVectorFromMesh" => nor_vector_from_mesh,
        "si_norConstrComputeNormalVec" => nor_constr_compute_normal_vec,
        "si_constr3PntsFormFace" => constr_3pnts_form_face,
        "checkLow" => check_low,
        "checklowestGeometryVertexRec" => checklowest_rec,
        "checklowestGeometryVertex" => checklowest,
        "si_animPivotConstrInit" => pivot_constr_init,
        "SI_sceneConstraintInit" => scene_constraint_init,
        "si_animPivotCheckTransition" => pivot_check_transition,
        "si_animPivotTime" => pivot_time,
        "si_animModelPivotPlaneUpdate" => pivot_plane_update,
        "si_animModelBoundPlaneUpdate" => bound_plane_update,
        "itemSetObjPntCns" => item_set_obj_pnt_cns,
        "SI_itemSetClusterConstraint" => item_set_cluster_constraint,
        "SI_modelHasConstrainedClusters" => model_has_constrained_clusters,
        "SI_quickStretchActive" => quick_stretch_active,
        "SI_itemRelaxConstraint" => item_relax_constraint,
        "SI_anyVisibleCameraConstraints" => any_visible_camera_constraints,
        "SI_relaxCnsLinkFromData" => relax_cns_link_from_data,
        "SI_constraintRelaxFromData" => constraint_relax_from_data,
        "SI_buildLocalUpdateList" => build_local_update_list,
        "SI_getConstraintUpdateList" => get_constraint_update_list,
        "SI_itemSetConstraint" => item_set_constraint,
        "SI_skeChainRoot#image" => image_ske_chain_root,
        "SI_ikValidConstraint#image" => image_ik_valid_constraint,
        "si_tanConstrCreateData#image" => image_tan_create,
        "rotlimCreateData#image" => image_rotlim_create,
        "poslimCreateData#image" => image_poslim_create,
        "invKinPrefAngleUpdate#image" => image_pref_angle_update,
        "SI_sceneConstrUpdate" => scene_constr_update,
        "si_skeJointNumber#image" => image_joint_number,
        "skeChainTotalLength#image" => image_total_length,
        "SI_skeJointDraw" => ske_joint_draw,
        "skeSplineToChain" => ske_spline_to_chain,
        "skeTubing" => ske_tubing,
        "skeTubingUpdate" => ske_tubing_update,
        "si_skeAutoEnvelope" => ske_auto_envelope,
        "limitFreezeSet" => limit_freeze_set,
        "limitFreezeToggle" => limit_freeze_toggle,
        "limitGhostSet" => limit_ghost_set,
        "rotlimDefault" => rotlim_default,
        "rotlimWarning" => rotlim_warning,
        "invKinDump" => image_inv_kin_dump,
        "tanConstrFindClosePoint" => tan_constr_find_close_point,
        "si_tanConstrComputeTanVector" => tan_constr_compute_tan_vector,
        "si_tanVectorFromPath" => tan_vector_from_path,
        _ => return None,
    })
}
