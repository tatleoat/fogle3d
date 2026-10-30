// SPDX-License-Identifier: MIT
//! `dict-checkkit <directory of vector files>`: the record-taking routines of the dictionary
//! (`reldictops.rs`, `hieranim.rs`, `SI_elemdictRestoreActions`) on `si-checkkit`, beside
//! `dict-emucheck` (finding 0011, "A check binary with si-checkkit"). Only the entries the
//! vectors of `re/out/si-dict/vec_rdops` need; the old driver keeps the whole table.

use si_checkkit::Kit;
use si_dict::outside::{Outside, Routine};
use si_dict::*;
use si_mem::Mem;

/// The dictionary crate's own `Outside` over the kit's: a call is handed on by name and address.
struct Bridge<'a>(&'a mut dyn si_mem::Outside);

impl Outside for Bridge<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        self.0.call(m, si_mem::Routine::new(routine.name, routine.addr), args).eax
    }
    fn stdout(&mut self, text: &[u8]) {
        self.0.print(1, text);
    }
    fn stderr(&mut self, text: &[u8]) {
        self.0.print(2, text);
    }
}

fn main() {
    Kit::new(&[
        ("SI_elemdictRestoreActions", |m, o, a| dictin::restore_actions(m, &mut Bridge(o), a.p("ed")).into()),
        ("SI_hieranimDispose", |m, o, a| hieranim::dispose(m, &mut Bridge(o), a.p::<()>("anim").addr()).into()),
        ("SI_hieranimGetName", |m, _o, a| hieranim::get_name(m, a.p::<()>("anim").addr(), a.i("index")).into()),
        ("SI_hieranimGetType", |m, _o, a| hieranim::get_type(m, a.p::<()>("elem").addr(), a.i("index"), 0).into()),
        ("SI_reldictGetArgCond", |m, _o, a| reldictops::get_arg_cond(m, a.p("rd"), a.i("index")).into()),
        ("SI_reldictGetArgPickMessage", |m, _o, a| reldictops::get_arg_pick_message(m, a.p("rd"), a.i("index")).into()),
        ("SI_reldictGetChap", |m, _o, a| reldictops::get_chap(m, a.p("rd"), a.i("index"), a.s("which")).into()),
        ("SI_reldictGetCond", |m, _o, a| reldictops::get_cond(m, a.p("rd"), a.i("index"), a.s("which")).into()),
        ("SI_reldictGetGrp", |m, _o, a| reldictops::get_grp(m, a.p("rd"), a.i("index"), a.s("which")).into()),
        ("SI_reldictGetName", |m, _o, a| reldictops::get_name(m, a.p("rd"), a.i("index"), a.s("which")).into()),
        ("SI_reldictGetNbArgInstances", |m, _o, a| reldictops::get_nb_arg_instances(m, a.p("rd"), a.i("index")).into()),
        ("SI_reldictGetNbArgs", |m, _o, a| reldictops::get_nb_args(m, a.p("rd"), a.i("index")).into()),
        ("SI_reldictGetNbDistinctArgs", |m, _o, a| reldictops::get_nb_distinct_args(m, a.p("rd"), a.i("index")).into()),
        ("SI_reldictGetNbDistinctRes", |m, _o, a| reldictops::get_nb_distinct_res(m, a.p("rd"), a.i("index")).into()),
        ("SI_reldictGetNbRes", |m, _o, a| reldictops::get_nb_res(m, a.p("rd"), a.i("index")).into()),
        ("SI_reldictGetNbResInstances", |m, _o, a| reldictops::get_nb_res_instances(m, a.p("rd"), a.i("index")).into()),
        ("SI_reldictGetNextDistinctArg", |m, _o, a| reldictops::get_next_distinct_arg(m, a.p("rd"), a.i("index")).into()),
        ("SI_reldictGetNextDistinctRes", |m, _o, a| reldictops::get_next_distinct_res(m, a.p("rd"), a.i("index")).into()),
        ("SI_reldictGetResCond", |m, _o, a| reldictops::get_res_cond(m, a.p("rd"), a.i("index")).into()),
        ("SI_reldictGetResPickMessage", |m, _o, a| reldictops::get_res_pick_message(m, a.p("rd"), a.i("index")).into()),
        ("SI_reldictGetScope", |m, _o, a| reldictops::get_scope(m, a.p("rd"), a.i("index"), a.s("which")).into()),
        ("SI_reldictOpParamInstanceGetGrp", |m, _o, a| reldictops::op_param_instance_get_grp(m, a.p("rd"), a.i("index"), a.i("n"), a.s("which")).into()),
        ("lookForExistingArgName", |m, _o, a| reldictops::look_for_existing_arg_name(m, a.p::<()>("def").addr(), a.p::<()>("name").addr(), a.i("ty"), a.i("ty2")).into()),
        ("lookForExistingFcurveName", |m, _o, a| reldictops::look_for_existing_fcurve_name(m, a.p::<()>("atd").addr(), a.p::<()>("group").addr(), a.p::<()>("name").addr()).into()),
        ("lookForExistingOatName", |m, _o, a| reldictops::look_for_existing_oat_name(m, a.p::<()>("atd").addr(), a.p::<()>("group").addr(), a.p::<()>("name").addr()).into()),
        ("lookForExistingResName", |m, _o, a| reldictops::look_for_existing_res_name(m, a.p::<()>("def").addr(), a.p::<()>("name").addr(), a.i("ty"), a.i("ty2"), a.s("group"), a.s("all")).into()),
        ("reldictOpArgInstanceGetGrp", |m, _o, a| reldictops::op_arg_instance_get_grp(m, a.p("rd"), a.i("index"), a.i("n")).into()),
        ("reldictOpResInstanceGetGrp", |m, _o, a| reldictops::op_res_instance_get_grp(m, a.p("rd"), a.i("index"), a.i("n")).into()),
        ("searchAttdictByFcvName", |m, _o, a| reldictops::search_attdict_by_fcv_name(m, a.p::<()>("atd").addr(), a.p::<()>("group").addr(), a.p::<()>("name").addr()).into()),
    ])
    .setup(|m| {
        si_mem::init_globals(m, si_dict::globals::ALL);
        si_dict::rodata::store(m);
    })
    .allow_global_write("GW:SI_secureLicAlloc+0x0")
    .given_lib_column()
    .run()
}
