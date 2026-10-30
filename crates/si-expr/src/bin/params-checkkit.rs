// SPDX-License-Identifier: MIT
//! `params-checkkit <directory of vector files>`: the parameter readers and writers of `edparams.c` for the render's chapters
//! (`si_expr::image::params`) on `si-checkkit` against the emulated original (finding 0013). Vectors:
//! `re/tools/params1/gen_params.py`.
use si_checkkit::Kit;
use si_expr::image::params::*;

fn main() {
    // the parser's tables are read from the user's install (docs/design/runtime-data.md)
    si_rt::check_or_exit("params-checkkit");
    Kit::new(&[
        ("SI_getAttrValInBoolean", |m, _o, a| get_attr_val_in_boolean(m, a.v("v")).into()),
        ("SI_modelPropagateMatteParam", |m, _o, a| model_propagate_matte_param(m, a.v("mdl"), a.f("v")).into()),
        ("SI_waveGetParam", |m, _o, a| wave_get_param(m, a.i("fcv_type"), a.v("wave"), a.v("fcv"), a.v("out")).into()),
        ("SI_waveSetParam", |m, o, a| wave_set_param(m, o, a.i("fcv_type"), a.v("wave"), a.v("fcv"), a.v("value")).into()),
        ("SI_camGetParam", |m, _o, a| cam_get_param(m, a.i("fcv_type"), a.v("cam"), a.v("fcv"), a.v("out")).into()),
        ("SI_camSetParam", |m, o, a| cam_set_param(m, o, a.i("fcv_type"), a.v("cam"), a.v("fcv"), a.v("value")).into()),
        ("SI_materialGetParam", |m, _o, a| material_get_param(m, a.i("fcv_type"), a.v("mat"), a.v("out")).into()),
        ("SI_text3dGetParam", |m, _o, a| text3d_get_param(m, a.i("fcv_type"), a.v("t"), a.v("out")).into()),
        ("SI_text3dSetParam", |m, o, a| text3d_set_param(m, o, a.i("fcv_type"), a.v("t"), a.v("value")).into()),
        ("SI_text2dGetParam", |m, o, a| text2d_get_param(m, o, a.i("fcv_type"), a.v("t"), a.v("out")).into()),
        ("SI_text2dSetParam", |m, o, a| text2d_set_param(m, o, a.i("fcv_type"), a.v("t"), a.v("value")).into()),
        ("SI_lightGetParam", |m, o, a| light_get_param(m, o, a.i("fcv_type"), a.v("light"), a.v("fcv"), a.v("out")).into()),
        ("SI_lightSetParam", |m, o, a| light_set_param(m, o, a.i("fcv_type"), a.v("light"), a.v("fcv"), a.v("value")).into()),
        ("SI_sceneGetParam", |m, o, a| scene_get_param(m, o, a.i("fcv_type"), a.v("ctrl"), a.v("out")).into()),
    ])
    .run()
}
