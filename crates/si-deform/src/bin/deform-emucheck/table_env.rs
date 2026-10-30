// SPDX-License-Identifier: MIT
//! From a vector file's routine name to the port, for the envelope files (`envelop.c`,
//! `envelopset.c`, `envtools.c`): one line per function.

use crate::{Args, Ret};
use si_deform::env_records::DefCrv;
use si_deform::envelop as e;
use si_deform::envelopset as s;
use si_deform::envtools as t;
use si_deform::outside::Outside;
use si_mem::{Mem, Ptr32};
use si_x87::vectors::Word;
use si_x87::x;

/// The words of an argument passed by value as a structure, pointers resolved.
fn words(a: &Args, name: &str) -> Vec<u32> {
    let f = a.case.input(&format!("v_{name}"));
    f.words()
        .iter()
        .map(|w| match w {
            Word::Value { value, .. } => *value as u32,
            Word::Ref { name, offset } => a.names.iter().find(|b| b.0 == name).map(|b| b.1).unwrap_or(0).wrapping_add(*offset),
            Word::Unwritten { .. } => 0,
        })
        .collect()
}

/// An `SI_EnvDefCrvPrm` passed by value: six words.
fn def_crv(a: &Args, name: &str) -> DefCrv {
    let w = words(a, name);
    DefCrv { kind: w[0] as i32, table: Ptr32::new(w[1]), lo: f32::from_bits(w[2]), hi: f32::from_bits(w[3]), amplitude: f32::from_bits(w[4]), scale: f32::from_bits(w[5]) }
}

pub fn run(name: &str, m: &mut Mem, o: &mut dyn Outside, a: &Args) -> Option<Ret> {
    Some(match name {
        // envtools.c
        "SI_envelopLevel" => t::envelop_level(m, a.p("node")).into(),
        "SI_envGetRootFromEnvElem" => t::env_get_root_from_env_elem(m, a.p("node")).into(),
        "SI_envLocalGet" => t::env_local_get(m, a.p("node")).into(),
        "SI_skelRootGet" => t::skel_root_get(m, a.p("node")).into(),
        "SI_mainGlobEnv" => t::main_glob_env(m, a.p("node")).into(),
        "SI_envelopSkelRoot" => t::envelop_skel_root(m, a.p("node")).into(),
        "SI_envelopActive" => t::envelop_active(m, o, a.p("node")).into(),
        "SI_envelopVertAllocate" => t::envelop_vert_allocate(m).into(),
        "SI_envelopListAllocate" => t::envelop_list_allocate(m, a.p("owner")).into(),
        "SI_envelopInfoAllocate" => t::envelop_info_allocate(m).into(),
        "SI_envelopVertCopy" => t::envelop_vert_copy(m, a.p("from")).into(),
        "envelopInfoCopy" => t::envelop_info_copy(m, a.p("from"), a.s("lists")).into(),
        "si_envelopVertListDispose" => t::envelop_vert_list_dispose(m, a.p("slot")).into(),
        "envelopDispose" => t::envelop_dispose(m, a.p("list"), a.p("record")).into(),
        "SI_envelopInfoDispose" => t::envelop_info_dispose(m, a.p("slot")).into(),
        "envelopInfoTreeDispose" => t::envelop_info_tree_dispose(m, a.p("skin"), a.p("node"), a.i("level")).into(),
        "SI_envelopDispDef" => t::envelop_disp_def(m, o, a.p("node")).into(),
        "SI_envelopCopyDef" => t::envelop_copy_def(m, o, a.p("node"), a.s("flag")).into(),
        "SI_envelopNeedUpdate" => t::envelop_need_update(m, a.p("skin"), a.p("node"), a.i("level"), a.s("brothers")).into(),
        "SI_envSkelSetFlag" => t::env_skel_set_flag(m, a.p("skin"), a.p("node"), a.s("moved"), a.i("level"), a.s("brothers")).into(),
        "SI_envSkelResetFlag" => t::env_skel_reset_flag(m, a.v("dict")).into(),
        "SI_envelopCompensate" => t::envelop_compensate(m, a.p("node"), a.p("by")).into(),
        // envelop.c
        "editVertAllocate" => e::edit_vert_allocate(m).into(),
        "editVertDispose" => e::edit_vert_dispose(m).into(),
        "dampTabCreate" => e::damp_tab_create(m, a.p("node")).into(),
        "envelopMeshCompNormals" => e::envelop_mesh_comp_normals(m, o, a.v("mesh")).into(),
        "envUpdateJointState" => e::env_update_joint_state(m, a.p("node"), a.p("matrix")).into(),
        "envUpdateMatrix" => e::env_update_matrix(m, a.p("node"), a.p("matrix"), a.i("level")).into(),
        "envAutoSet" => e::env_auto_set(m, a.p("root"), a.s("set")).into(),
        "expFct" => Ret::St0(x(e::exp_fct(a.f("base"), a.f("exponent")))),
        "yDef" => Ret::St0(e::y_def(m, &def_crv(a, "prm"), a.f("pos"), a.f("t"))),
        "compDef" => Ret::St0(e::comp_def(m, a.p("node"), a.f("pos"), a.f("bend"), a.f("turn"), a.f("side"), a.f("along"))),
        "envelopSkelMdl" => e::envelop_skel_mdl(m, o, a.p("skin"), a.p("bone"), a.p("back"), a.s("edit")).into(),
        "envelopJoint" => e::envelop_joint(m, o, a.p("skin"), a.p("joint"), a.p("back"), a.s("edit")).into(),
        "unassVertCopy" => e::unass_vert_copy(m, a.p("skin")).into(),
        "envelopJntAxisMap" => e::envelop_jnt_axis_map(m, a.p("node"), a.p("bone")).into(),
        "envelopChain" => e::envelop_chain(m, o, a.p("skin"), a.p("node"), a.p("back"), a.s("edit"), a.i("level"), a.s("brothers")).into(),
        "envelopSkel" => e::envelop_skel(m, o, a.p("skin"), a.p("root"), a.s("edit")).into(),
        "SI_envelopSkelUpdate" => e::envelop_skel_update(m, o, a.p("node")).into(),
        "SI_envelopCompUpdate" => e::envelop_comp_update(m, o, a.p("skin")).into(),
        "SI_envelopHierUpdate" => e::envelop_hier_update(m, o, a.p("node")).into(),
        // envelopset.c
        "skelJntAllocate" => s::skel_jnt_allocate(m).into(),
        "skelJntDispose" => s::skel_jnt_dispose(m, a.p("slot")).into(),
        "vrtlstAllocate" => s::vrtlst_allocate(m).into(),
        "vrtlstDispose" => s::vrtlst_dispose(m, a.p("slot")).into(),
        "compRatio" => Ret::St0(s::comp_ratio(m, a.v3("point"), a.v3("start"), a.v3("end"), a.p("dist"))),
        "SI_envelopVertWeightBalance" => s::envelop_vert_weight_balance(m, a.f("weight"), a.p("list")).into(),
        "SI_envelopModifyWeight" => s::envelop_modify_weight(m, a.p("v"), a.p("changed"), a.p("bone"), a.i("mode"), a.p("amount"), a.p("list")).into(),
        // second pass (finding 0029, section 14)
        "vrtInJntBound" => s::vrt_in_jnt_bound(m, o, a.v3("point"), a.p("bone"), a.p("out")).into(),
        "envToJntAssign" => s::env_to_jnt_assign(m, o, a.p("skin"), a.i("index"), a.v3("point"), a.i("mode"), a.p("flag")).into(),
        "envAssign" => s::env_assign(m, o, a.p("skin"), a.i("mode")).into(),
        "envelopListBuild" => s::envelop_list_build(m, o, a.p("node"), a.i("word"), a.p("head")).into(),
        "envelopSkelBuild" => s::envelop_skel_build(m, a.p("skin"), a.p("node"), a.i("level"), a.s("brothers")).into(),
        "SI_envelopSkelSet" => s::envelop_skel_set(m, o, a.p("node"), a.v("colour") as u16, a.s("brothers")).into(),
        "SI_envelopSkelReset" => s::envelop_skel_reset(m, o, a.p("node"), a.s("brothers")).into(),
        "envelopAssignSkelSet" => s::envelop_assign_skel_set(m, o, a.p("node"), a.p("lists"), a.i("level"), a.i("limit"), a.v("colour") as u16, a.s("local"), a.s("brothers")).into(),
        "SI_envelopSkelAssign" => s::envelop_skel_assign(m, o, a.p("skin"), a.p("root"), a.i("limit"), a.i("mode")).into(),
        "oldenvelopJoint" => e::oldenvelop_joint(m, a.p("node")).into(),
        "oldenvelopChain" => e::oldenvelop_chain(m, a.p("chain"), a.i("level")).into(),
        "si_oldenvelopReset" => e::oldenvelop_reset(m, o, a.p("node")).into(),
        "envVrtResetEditFlag" => e::env_vrt_reset_edit_flag(m, a.p("node")).into(),
        "SI_envelopUpdateInitDef" => e::envelop_update_init_def(m, o, a.p("node")).into(),
        "envPrecInListFind" => s::env_prec_in_list_find(m, a.p("node"), a.p("target")).into(),
        "SI_envelopRenum" => t::envelop_renum(m, a.p("skin"), a.p("node"), a.p("map"), a.s("brothers")).into(),
        "SI_envelopNewVertAssign" => s::envelop_new_vert_assign(m, o, a.p("skin")).into(),
        "SI_envelopAddNewParts" => s::envelop_add_new_parts(m, o, a.p("node"), a.p("child")).into(),
        "SI_envelopAddNewSkelParts" => s::envelop_add_new_skel_parts(m, o, a.p("node"), a.p("child")).into(),
        "SI_envelopRelax" => t::envelop_relax(m, o, a.p("node"), a.p("root"), a.s("quiet"), a.s("sons")).into(),
        "SI_envelopCut" => t::envelop_cut(m, o, a.p("node")).into(),
        "SI_envelopVrtColGet" => (s::envelop_vrt_col_get(m, a.p("skin"), a.i("index")) as u32).into(),
        "SI_envelopVrtColSet" => s::envelop_vrt_col_set(m, a.p("skin"), a.i("index"), a.v("colour") as u16).into(),
        "SI_envelopReassign" => s::envelop_reassign(m, o, a.p("skin"), a.s("hier")).into(),
        "SI_matrixOrthoNormalInverse" => {
            let src: Ptr32<si_math::types::Matrix> = a.p("src");
            let r = t::matrix_ortho_normal_inverse(&m.read(src));
            m.write(a.p::<si_math::types::Matrix>("dst"), r);
            Ret::None
        }
        "SI_envelopSkelPosReset" => t::envelop_skel_pos_reset(m, o, a.p("node"), a.i("what"), a.s("auto"), a.s("brothers")).into(),
        "SI_envelopTmpReset" => t::envelop_tmp_reset(m, o, a.p("skin"), a.i("what")).into(),
        "SI_envelopWeightCreateReference" => s::envelop_weight_create_reference(m, a.p("bone"), a.i("index"), a.p("amount"), a.v3("point"), a.p("list")).into(),
        "vertWeightUpdate" => s::vert_weight_update(m, a.p("skin"), a.p("node"), a.p("target"), a.i("index"), a.v3("point"), a.i("mode"), a.p("amount"), a.p("list"), a.i("level"), a.s("brothers")).into(),
        "SI_envelopVertWeightUpdate" => s::envelop_vert_weight_update(m, o, a.p("skin"), a.p("node"), a.p("target"), a.i("index"), a.f("weight"), a.i("mode"), a.s("by_dist"), a.f("reach"), a.f("scale")).into(),
        "SI_envelopWeightUpdate" => s::envelop_weight_update(m, a.p("skin"), a.p("node"), a.p("target"), a.f("weight"), a.i("mode")).into(),
        "envelopCheckVrtInd" => t::envelop_check_vrt_ind(m, o, a.p("node")).into(),
        "envelopReassignPtr" => t::envelop_reassign_ptr(m, a.p("node"), a.i("word"), a.p("lists")).into(),
        "envelopRebuildList" => t::envelop_rebuild_list(m, o, a.p("skin"), a.p("node"), a.i("level")).into(),
        "SI_envelopVertDistMinMaxCompute" => s::envelop_vert_dist_min_max_compute(m, o, a.p("skin"), a.p("bone"), a.i("index"), a.p("min"), a.p("max")).into(),
        _ => return None,
    })
}
