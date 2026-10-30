// SPDX-License-Identifier: MIT
//! `kine-checkkit <directory of vector files>`: the check of `si-kine`'s image-form `classic`
//! module on `si-checkkit`.
use si_checkkit::Kit;
use si_kine::classic as c;
use si_kine::outside as out;
use si_types::geom::Vector;

/// The crate's own callee trait, over the check's one: a hook is a call by name and address.
struct Adapt<'a>(&'a mut dyn si_mem::Outside);

impl out::Outside for Adapt<'_> {
    fn call(&mut self, m: &mut si_mem::Mem, r: &'static out::Routine, args: &[u32]) -> out::Ret {
        out::Ret::int(self.0.call(m, si_mem::Routine::new(r.name, r.addr), args).eax)
    }
}

fn vec3(a: &si_checkkit::Args, x: &str, y: &str, z: &str) -> Vector {
    Vector { x: a.f(x), y: a.f(y), z: a.f(z) }
}

fn main() {
    Kit::new(&[
        ("checkHierarchUnifScaled", |m, _o, a| c::check_hierarch_unif_scaled(m, a.p("node")).into()),
        ("SI_classicModelSymmetry", |m, o, a| {
            c::classic_model_symmetry(m, &mut Adapt(o), a.p("pp"), a.v("plane"), a.s("mode"));
            ().into()
        }),
        ("si_classicFreezeRootScaling", |m, o, a| {
            c::classic_freeze_root_scaling(m, &mut Adapt(o), a.p("node"), a.s("rec"));
            ().into()
        }),
        ("si_envAdjustBeforeSet", |m, o, a| {
            c::env_adjust_before_set(m, &mut Adapt(o), a.p("a"), a.p("b"));
            ().into()
        }),
        ("chainNotScaled", |m, _o, a| c::chain_not_scaled(m, a.p("node")).into()),
        ("chainUniformScaled", |m, _o, a| c::chain_uniform_scaled(m, a.p("node")).into()),
        ("chainScaled", |m, _o, a| c::chain_scaled(m, a.p("node")).into()),
        ("checkChainScaleAt", |m, _o, a| c::check_chain_scale_at(m, a.p("node")).into()),
        ("classicSolvePartialInvKinR", |m, o, a| {
            let mut s = m.i16(a.v("stop"));
            c::solve_partial_inv_kin_r(m, &mut Adapt(o), a.p("root"), a.p("first"), a.i("n"), vec3(a, "gx", "gy", "gz"), a.s("fp"), &mut s);
            m.set_i16(a.v("stop"), s);
            ().into()
        }),
        ("classicGoalUpdate", |m, _o, a| {
            let v = c::classic_goal_update(m, a.p("a"), a.p("b"), vec3(a, "gx", "gy", "gz"));
            let out = a.v("out");
            m.set_f32(out, v.x);
            m.set_f32(out + 4, v.y);
            m.set_f32(out + 8, v.z);
            ().into()
        }),
        ("classicSolvePartialInvKinJ", |m, o, a| {
            let mut s = m.i16(a.v("stop"));
            c::solve_partial_inv_kin_j(m, &mut Adapt(o), a.p("root"), a.p("first"), a.i("n"), vec3(a, "gx", "gy", "gz"), a.s("fp"), &mut s);
            m.set_i16(a.v("stop"), s);
            ().into()
        }),
        ("SI_classicSolvePartialInvKin", |m, o, a| {
            let mut s = m.i16(a.v("stop"));
            c::classic_solve_partial_inv_kin(m, &mut Adapt(o), a.p("root"), a.p("first"), a.i("n"), vec3(a, "gx", "gy", "gz"), a.s("fp"), &mut s);
            m.set_i16(a.v("stop"), s);
            ().into()
        }),
        ("classicLimbPosition", |m, _o, a| c::classic_limb_position(m, a.v("out"), a.p("node"), a.i("n")).into()),
        ("compensateInfoInit", |m, _o, a| {
            c::compensate_info_init(m, a.v("list"));
            ().into()
        }),
        ("vectorComputeAngle", |m, _o, a| {
            c::vector_compute_angle(m, vec3(a, "x", "y", "z"), a.v("out"));
            ().into()
        }),
        ("computeCompensateAngle", |m, _o, a| {
            c::compute_compensate_angle(m, a.i("idx"), vec3(a, "x1", "y1", "z1"), vec3(a, "x2", "y2", "z2"), a.p("node"));
            ().into()
        }),
        ("SI_modelCompensateAngle", |m, _o, a| {
            c::model_compensate_angle(m, a.v("mat"), a.p("node"));
            ().into()
        }),
        ("keyCompensateInfoUpdate", |m, _o, a| {
            c::key_compensate_info_update(m, a.p("node"), a.v("fc"));
            ().into()
        }),
        ("SI_savekeyCompensateInfo", |m, _o, a| {
            c::savekey_compensate_info(m, a.p("node"), a.v("fc"));
            ().into()
        }),
        ("compensateFindKF", |m, _o, a| c::compensate_find_kf(m, a.i("frame"), a.p("node")).into()),
        ("angleInterplate", |m, _o, a| {
            c::angle_interplate(m, a.i("which"), a.i("f0"), a.i("f1"), a.p("node"));
            ().into()
        }),
        ("angleAddition", |m, _o, a| {
            c::angle_addition(m, a.i("which"), a.p("node"));
            ().into()
        }),
        ("compensateAngleUpdate", |m, _o, a| {
            c::compensate_angle_update(m, a.i("f0"), a.i("f1"), a.i("idx3"), a.v("mat"), a.p("node"));
            ().into()
        }),
        ("compensateInfoUpdate", |m, _o, a| {
            c::compensate_info_update(m, a.v("mat"), a.p("node"));
            ().into()
        }),
        ("vectorRotation", |m, _o, a| {
            c::vector_rotation(m, a.f("ax"), a.f("ay"), a.f("az"), a.v("v"));
            ().into()
        }),
        ("vectorCompensation", |m, _o, a| {
            c::vector_compensation(m, a.i("idx"), a.v("v"), a.p("node"));
            ().into()
        }),
        ("SI_modelCompensateLocalCoord", |m, _o, a| {
            c::model_compensate_local_coord(m, a.p("p1"), a.p("p2"));
            ().into()
        }),
        ("classicJointPosUpdate", |m, _o, a| {
            let v = c::classic_joint_pos_update(m, a.p("node"), vec3(a, "gx", "gy", "gz"));
            let out = a.v("out");
            m.set_f32(out, v.x);
            m.set_f32(out + 4, v.y);
            m.set_f32(out + 8, v.z);
            ().into()
        }),
        ("classicMoveJointBranch", |m, o, a| {
            c::classic_move_joint_branch(m, &mut Adapt(o), a.p("node"), vec3(a, "gx", "gy", "gz"), a.s("a"), a.s("b"));
            ().into()
        }),
        ("si_classicSkeMoveJointBranch", |m, o, a| {
            c::classic_ske_move_joint_branch(m, &mut Adapt(o), a.p("node"), a.v("v"), a.s("a"), a.s("b"));
            ().into()
        }),
        ("norConstrDividPatchShell", |m, _o, a| {
            si_kine::norconstr::nor_constr_divid_patch_shell(m, a.p("pat"));
            ().into()
        }),
        ("norConstrExtractPatchShell", |m, _o, a| si_kine::norconstr::nor_constr_extract_patch_shell(m, a.p("src"), a.i("row"), a.i("col")).into()),
        ("norConstrFindCloseArea", |m, _o, a| {
            si_kine::norconstr::nor_constr_find_close_area(m, a.p("pat"), vec3(a, "x", "y", "z"), a.p("row"), a.p("col"), a.p("out"));
            ().into()
        }),
        ("norConstrSamplePatchShell", |m, _o, a| {
            si_kine::norconstr::nor_constr_sample_patch_shell(m, a.v("pp"), vec3(a, "x", "y", "z"), a.i("depth"), a.p("lo"), a.p("hi"), a.p("out"));
            ().into()
        }),
    ])
    .given_lib_column()
    .run()
}
