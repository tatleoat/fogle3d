// SPDX-License-Identifier: MIT
//! `matval-checkkit <directory of vector files>`: the 22 validators of `mat_validation.c`
//! (`mat_validation.rs`) on `si-checkkit` against the emulated original (vectors from
//! `re/tools/si-render/matval/gen_matval.py`, finding 0012, section 33).

use si_checkkit::Kit;
use si_render::mat_validation::*;

fn main() {
    Kit::new(&[
        ("SI_validMaterialBlurDecay", |m, _o, a| valid_material_blur_decay(m, a.p::<()>("mat").addr(), a.f("value")).into()),
        ("SI_validMaterialBlurWidth", |m, _o, a| valid_material_blur_width(m, a.p::<()>("mat").addr(), a.f("value")).into()),
        ("SI_validTxt2dMethod", |m, o, a| valid_txt2d_method(m, o, a.p::<()>("t").addr(), a.i("method")).into()),
        ("SI_validTxt2dURepeat", |m, o, a| valid_txt2d_u_repeat(m, o, a.p::<()>("t").addr(), a.i("count")).into()),
        ("SI_validTxt2dVRepeat", |m, o, a| valid_txt2d_v_repeat(m, o, a.p::<()>("t").addr(), a.i("count")).into()),
        ("SI_validTxt2dCropU", |m, o, a| valid_txt2d_crop_u(m, o, a.p::<()>("t").addr()).into()),
        ("SI_validTxt2dCropV", |m, o, a| valid_txt2d_crop_v(m, o, a.p::<()>("t").addr(), a.i("value"), a.i("high")).into()),
        ("SI_validTxt2dWindowScal", |m, o, a| {
            valid_txt2d_window_scal(m, o, a.p::<()>("t").addr(), a.p::<()>("slot").addr(), a.f("value")).into()
        }),
        ("SI_validTxt2dWindowTrans", |m, o, a| {
            valid_txt2d_window_trans(m, o, a.p::<()>("t").addr(), a.p::<()>("slot").addr(), a.f("value")).into()
        }),
        ("SI_validTxt3dNbIter", |m, _o, a| valid_txt3d_nb_iter(m, a.p::<()>("t").addr(), a.i("count")).into()),
        ("SI_validTxt3dFct1", |m, _o, a| valid_txt3d_fct1(m, a.p::<()>("t").addr(), a.f("value")).into()),
        ("SI_validTxt3dFct2", |m, _o, a| valid_txt3d_fct2(m, a.p::<()>("t").addr(), a.f("value")).into()),
        ("SI_validTxt3dFct3", |m, _o, a| valid_txt3d_fct3(m, a.p::<()>("t").addr(), a.f("value")).into()),
        ("SI_validLightShadowType", |m, _o, a| valid_light_shadow_type(m, a.p::<()>("light").addr(), a.i("type")).into()),
        ("SI_validLightMapRes", |m, _o, a| valid_light_map_res(m, a.p::<()>("light").addr(), a.i("value")).into()),
        ("SI_validLightFilterStep", |m, _o, a| valid_light_filter_step(m, a.p::<()>("light").addr(), a.i("value")).into()),
        ("SI_lightValidateSelective", |m, o, a| light_validate_selective(m, o, a.p::<()>("light").addr()).into()),
        ("SI_lightValidateAreaLight", |m, _o, a| light_validate_area_light(m, a.p::<()>("light").addr()).into()),
        ("SI_lightValidateAreaType", |m, _o, a| light_validate_area_type(m, a.p::<()>("light").addr()).into()),
        ("SI_lightValidateAreaRotation", |m, o, a| light_validate_area_rotation(m, o, a.p::<()>("light").addr()).into()),
        ("SI_lightValidateAreaScaling", |m, o, a| light_validate_area_scaling(m, o, a.p::<()>("light").addr()).into()),
        ("SI_lightSunValidateDateTime", |m, o, a| light_sun_validate_date_time(m, o, a.p::<()>("light").addr()).into()),
    ])
    .run()
}
