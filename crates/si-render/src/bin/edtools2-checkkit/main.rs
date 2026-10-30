// SPDX-License-Identifier: MIT
//! `edtools2-checkkit <directory of vector files>`: `edtools2.rs` (the second half of
//! `edtools.c`) on `si-checkkit` against the emulated original (vectors from
//! `re/tools/edtools2/gen_ed2.py`, finding 0012, "edtools2").

use si_checkkit::Kit;
use si_render::edtools2::*;

fn main() {
    Kit::new(&[
        ("si_relGetDynConsType", |_m, _o, a| rel_get_dyn_cons_type(a.i("id")).into()),
        ("relGetDynConsRelid", |_m, _o, a| rel_get_dyn_cons_relid(a.i("id")).into()),
        ("SI_edElementSetDynamics", |m, o, a| {
            ed_element_set_dynamics(m, o, a.v("a"), a.v("b"));
            ().into()
        }),
        ("SI_edElementRelaxDynamics", |m, o, a| {
            ed_element_relax_dynamics(m, o, a.v("a"), a.v("b"));
            ().into()
        }),
        ("SI_edSelectDynaConstraint", |m, o, a| {
            ed_select_dyna_constraint(m, o, a.v("a"), a.v("b"), a.i("kind"));
            ().into()
        }),
        ("SI_edRelaxDynaConstraint", |m, o, a| {
            ed_relax_dyna_constraint(m, o, a.v("a"), a.v("b"), a.i("kind"));
            ().into()
        }),
        ("SI_edModelUnlinkWave", |m, o, a| {
            ed_model_unlink_wave(m, o, a.v("model"), a.v("wave"));
            ().into()
        }),
        ("SI_edCheckGroupRelations", |m, _o, a| ed_check_group_relations(m, a.v("elem"), a.i("i"), a.i("j")).into()),
        ("SI_wrathUnsupportedMdlType", |m, _o, a| wrath_unsupported_mdl_type(m, a.v("model")).into()),
        ("SI_wrathUnsupportedMdlType2", |m, _o, a| wrath_unsupported_mdl_type2(m, a.i("kind"), a.v("data")).into()),
        ("SI_edModelRemoveShader", |m, o, a| {
            ed_model_remove_shader(m, o, a.v("model"));
            ().into()
        }),
        ("SI_edMaterialRemoveShader", |m, o, a| {
            ed_material_remove_shader(m, o, a.v("material"));
            ().into()
        }),
        ("SI_edTexture2DRemoveShader", |m, o, a| {
            ed_texture2d_remove_shader(m, o, a.v("texture"));
            ().into()
        }),
        ("SI_edTexture3DRemoveShader", |m, o, a| {
            ed_texture3d_remove_shader(m, o, a.v("texture"));
            ().into()
        }),
        ("SI_edLightRemoveShader", |m, o, a| {
            ed_light_remove_shader(m, o, a.v("light"));
            ().into()
        }),
        ("SI_edCameraRemoveShader", |m, o, a| {
            ed_camera_remove_shader(m, o, a.v("dict"));
            ().into()
        }),
        ("SI_edOutputRemoveShader", |m, o, a| {
            ed_output_remove_shader(m, o, a.v("dict"));
            ().into()
        }),
        ("SI_edAtmosphereRemoveShader", |m, o, a| {
            ed_atmosphere_remove_shader(m, o, a.v("dict"));
            ().into()
        }),
        ("edRemoveMultipleShader", |m, o, a| {
            ed_remove_multiple_shader(m, o, a.v("dict"), a.v("chapter"));
            ().into()
        }),
        ("edModelRemoveMatTxt", |m, o, a| {
            ed_model_remove_mat_txt(m, o, a.v("elem"));
            ().into()
        }),
        ("edTexture2DRemove", |m, o, a| {
            ed_texture2d_remove(m, o, a.v("texture"));
            ().into()
        }),
        ("edTexture3DRemove", |m, o, a| {
            ed_texture3d_remove(m, o, a.v("texture"));
            ().into()
        }),
        ("edModelMaterialRemove", |m, o, a| ed_model_material_remove(m, o, a.v("model"), a.v("material")).into()),
        ("SI_edDisposeLattice", |m, o, a| {
            ed_dispose_lattice(m, o, a.v("model"), a.s("which"));
            ().into()
        }),
        ("SI_edDisposeWave", |m, o, a| {
            ed_dispose_wave(m, o, a.v("wave"));
            ().into()
        }),
        ("SI_edDisposeCluster", |m, o, a| {
            ed_dispose_cluster(m, o, a.v("model"), a.v("cluster"));
            ().into()
        }),
        ("SI_edDisposeShape", |m, o, a| {
            ed_dispose_shape(m, o, a.v("model"));
            ().into()
        }),
        ("SI_edDisposeUDF", |m, o, a| {
            ed_dispose_udf(m, o, a.v("udf"), a.v("cid"));
            ().into()
        }),
        ("SI_edDisposeLight", |m, o, a| ed_dispose_light(m, o, a.v("light")).into()),
        ("si_updateUVDomainInfo", |m, o, a| {
            update_uv_domain_info(m, o, a.v("model"), a.i("number"));
            ().into()
        }),
        ("SI_edDisposeTexture3D", |m, o, a| ed_dispose_texture3d(m, o, a.v("texture")).into()),
        ("SI_edDisposeTexture2D", |m, o, a| ed_dispose_texture2d(m, o, a.v("texture")).into()),
        ("SI_edDisposeMaterial", |m, o, a| ed_dispose_material(m, o, a.v("material"), a.s("dispose")).into()),
        ("SI_edUnselectedNseInDict", |m, _o, a| {
            ed_unselected_nse_in_dict(m, a.v("dict"));
            ().into()
        }),
        ("SI_edIsNseByDependence", |m, o, a| ed_is_nse_by_dependence(m, o, a.v("dict"), a.v("elem")).into()),
        ("SI_edAddWave", |m, o, a| {
            ed_add_wave(m, o, a.v("node"), a.v("wave"));
            ().into()
        }),
    ])
    .normalise_stack_args()
    .given_lib_column()
    .run()
}
