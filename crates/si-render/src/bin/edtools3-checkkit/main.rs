// SPDX-License-Identifier: MIT
//! `edtools3-checkkit <directory of vector files>`: `edtools3.rs` (the twelve routines of
//! `edtools.c` that `edtools2` left open) on `si-checkkit` against the emulated original (vectors
//! from `re/tools/edtools3/gen_ed3.py`, finding 0012, "edtools3").

use si_checkkit::Kit;
use si_render::edtools3::*;

fn main() {
    Kit::new(&[
        ("SI_edAddCamera", |m, o, a| {
            ed_add_camera(m, o, a.v("node"), a.v("inode"), a.s("rename"));
            ().into()
        }),
        ("SI_edAddLight", |m, o, a| {
            ed_add_light(m, o, a.v("node"), a.v("inode"), a.s("rename"));
            ().into()
        }),
        ("SI_edAddCluster", |m, o, a| {
            ed_add_cluster(m, o, a.v("model"), a.v("cluster"));
            ().into()
        }),
        ("SI_edAddShape", |m, o, a| {
            ed_add_shape(m, o, a.v("model"));
            ().into()
        }),
        ("SI_edAddLattice", |m, o, a| {
            ed_add_lattice(m, o, a.v("model"), a.s("which"));
            ().into()
        }),
        ("SI_edReplaceUDF", |m, o, a| {
            ed_replace_udf(m, o, a.v("old"), a.v("new"), a.i("chapter"));
            ().into()
        }),
        ("SI_getRelationList", |m, o, a| get_relation_list(m, o, a.v("dict"), a.v("rec"), a.i("index"), a.v("pass")).into()),
        ("edCnsSetJntMdlExtraRel", |m, o, a| {
            ed_cns_set_jnt_mdl_extra_rel(m, o, a.v("dict"), a.v("model"), a.v("from"), a.v("list"), a.v("rel"), a.s("on"));
            ().into()
        }),
        ("si_elemdictRemoveExtraCam", |m, o, a| {
            elemdict_remove_extra_cam(m, o, a.v("dict"), a.v("seen"));
            ().into()
        }),
        ("SI_edSetUneditable", |m, o, a| {
            ed_set_uneditable(m, o, a.v("dict"), a.v("elem"));
            ().into()
        }),
        ("SI_edSetEditable", |m, o, a| ed_set_editable(m, o, a.v("dict"), a.v("elem"), a.i("chapter")).into()),
        ("SI_edCheckForUneditable", |m, o, a| ed_check_for_uneditable(m, o, a.v("elem"), a.i("chapter")).into()),
    ])
    .allow_global_write("GW:SI_secureLicAlloc+0x0")
    .normalise_stack_args()
    .given_lib_column()
    .run()
}
