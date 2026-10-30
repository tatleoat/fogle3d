// SPDX-License-Identifier: MIT
//! `kine-actorcheck <directory of vector files>`: the check of `si-kine`'s `actor`, `character` and
//! `cha_libkeys` modules on `si-checkkit` (vectors from `re/tools/actor2/mkspecs.py`).
//!
//! Nothing here reproduces a function of the original.
use si_checkkit::Kit;
use si_kine::actor as ac;
use si_kine::cha_libkeys as cl;
use si_kine::character as ch;
use si_kine::outside as out;

/// The crate's own callee trait, over the check's one.
struct Adapt<'a>(&'a mut dyn si_mem::Outside);

impl out::Outside for Adapt<'_> {
    fn call(&mut self, m: &mut si_mem::Mem, r: &'static out::Routine, args: &[u32]) -> out::Ret {
        out::Ret::int(self.0.call(m, si_mem::Routine::new(r.name, r.addr), args).eax)
    }
}

fn main() {
    Kit::new(&[
        ("actorGetButton", |m, _o, a| {
            let (b, f) = ac::actor_get_button(a.s("mode"));
            m.set_i16(a.v("flag"), f);
            (b as i32).into()
        }),
        ("SI_actorGetMode", |m, _o, _a| ac::si_actor_get_mode(m).into()),
        ("actorSetMode", |m, o, a| {
            ac::actor_set_mode(m, &mut Adapt(o), a.i("mode"), a.s("s"));
            ().into()
        }),
        ("SI_actSetEditKeyMode", |m, o, _a| {
            ac::si_act_set_edit_key_mode(m, &mut Adapt(o));
            ().into()
        }),
        ("SI_actSetCutPasteKeyMode", |m, o, _a| {
            ac::si_act_set_cut_paste_key_mode(m, &mut Adapt(o));
            ().into()
        }),
        ("SI_actSetSlopeManagementMode", |m, o, _a| {
            ac::si_act_set_slope_management_mode(m, &mut Adapt(o));
            ().into()
        }),
        ("SI_actSetSetKeyMode", |m, o, _a| {
            ac::si_act_set_set_key_mode(m, &mut Adapt(o));
            ().into()
        }),
        ("SI_actSetEditFcvMode", |m, o, _a| {
            ac::si_act_set_edit_fcv_mode(m, &mut Adapt(o));
            ().into()
        }),
        ("SI_actSetEditTagMode", |m, o, _a| {
            ac::si_act_set_edit_tag_mode(m, &mut Adapt(o));
            ().into()
        }),
        ("SI_actSetInterpManagementMode", |m, o, _a| {
            ac::si_act_set_interp_management_mode(m, &mut Adapt(o));
            ().into()
        }),
        ("SI_skeletonFlagSet", |m, o, a| {
            ac::si_skeleton_flag_set(m, &mut Adapt(o), a.i("a"), a.i("b"));
            ().into()
        }),
        ("SI_actorDirectionFlagSet", |m, o, a| {
            ac::si_actor_direction_flag_set(m, &mut Adapt(o), a.i("v"));
            ().into()
        }),
        ("actorMultiRefresh", |m, o, _a| {
            ac::actor_multi_refresh(m, &mut Adapt(o));
            ().into()
        }),
        ("refreshActor", |m, o, _a| {
            ac::refresh_actor(m, &mut Adapt(o));
            ().into()
        }),
        ("initActor", |m, o, _a| {
            ac::init_actor(m, &mut Adapt(o));
            ().into()
        }),
        ("actorButton", |m, o, a| {
            ac::actor_button(m, &mut Adapt(o), a.v("ev"));
            ().into()
        }),
        ("SI_setRelDictFromDopeMode", |m, o, _a| {
            ch::si_set_rel_dict_from_dope_mode(m, &mut Adapt(o));
            ().into()
        }),
        ("SI_setDopFcvTableFromDopeMode", |m, o, _a| {
            ch::si_set_dop_fcv_table_from_dope_mode(m, &mut Adapt(o));
            ().into()
        }),
        ("dbGetNewlyReadElement", |m, _o, a| cl::db_get_newly_read_element(m, a.v("dict"), a.i("chap"), a.v("out")).into()),
        ("hieranimAssociateElems", |m, o, a| {
            cl::hieranim_associate_elems(m, &mut Adapt(o), a.v("dict"), a.v("elem"), a.i("chapIdx"), a.s("flag"), a.v("out")).into()
        }),
        ("filterApplyOnHierAnim", |m, o, a| {
            cl::filter_apply_on_hier_anim(m, &mut Adapt(o), a.v("elem"));
            ().into()
        }),
        ("SI_dbLoadHierAnimFromDict", |m, o, a| cl::si_db_load_hier_anim_from_dict(m, &mut Adapt(o), a.v("dict"), a.i("cid"), a.s("s")).into()),
        ("SI_dbLoadActionsFromDict", |m, o, a| {
            (cl::si_db_load_actions_from_dict(m, &mut Adapt(o), a.v("dict"), a.s("filter"), a.s("masters"), a.s("arg")) as i32).into()
        }),
        ("savekeyCharacter", |m, o, a| {
            ch::savekey_character(m, &mut Adapt(o), a.v("elem"), a.i("idx"), a.i("frame"), a.s("mode"));
            ().into()
        }),
        ("SI_mdlCharDefineKey", |m, o, a| {
            ch::si_mdl_char_define_key(m, &mut Adapt(o), a.v("model"), a.i("frame"), a.s("mode"));
            ().into()
        }),
        ("dopePerformOperOnKeyInterval", |m, o, a| {
            ch::dope_perform_oper_on_key_interval(m, &mut Adapt(o), a.v("dict"), a.i("op"), a.v("a"), a.v("b"), a.v("c"));
            ().into()
        }),
        ("SI_setFilterTrackTable", |m, _o, a| {
            cl::si_set_filter_track_table(m, a.s("val"), a.i("idx"));
            ().into()
        }),
    ])
    .setup(|m| si_kine::actout::store(m))
    .allow_global_write("the modules keep their state in globals (actmode, the dope flags, the filter table); the vectors list them")
    .given_lib_column()
    .run()
}
