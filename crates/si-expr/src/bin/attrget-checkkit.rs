// SPDX-License-Identifier: MIT
//! `attrget-checkkit <directory of vector files>`: the generic static read (`SI_edElementStaticValueGet`, `elem.rs`) and the
//! attribute-group readers of `attrget.rs` on `si-checkkit` against the emulated original (finding 0013, section 21).
//! Vectors: `re/tools/params3/gen_static.py`.
use si_checkkit::Kit;
use si_expr::image::attrget::*;
use si_expr::image::elem::ed_element_static_value_get;
use si_mem::Ptr32;

macro_rules! group {
    ($f:ident) => {
        |m, o, a| $f(m, o, Ptr32::new(a.v("ident")), Ptr32::new(a.v("index")), Ptr32::new(a.v("value"))).into()
    };
}

fn main() {
    // the parser's tables are read from the user's install (docs/design/runtime-data.md)
    si_rt::check_or_exit("attrget-checkkit");
    Kit::new(&[
        ("SI_edElementStaticValueGet", |m, o, a| {
            ed_element_static_value_get(m, o, Ptr32::new(a.v("e")), a.i("chap"), a.i("fcv_type"), Ptr32::new(a.v("access")), Ptr32::new(a.v("out")), a.v("custom")).into()
        }),
        ("SI_materialsAttrGetValue", group!(materials_attr_get_value)),
        ("SI_texture2DAttrGetValue", group!(texture2d_attr_get_value)),
        ("SI_texture3DAttrGetValue", group!(texture3d_attr_get_value)),
        ("SI_camerasAttrGetValue", group!(cameras_attr_get_value)),
        ("SI_lightsAttrGetValue", group!(lights_attr_get_value)),
        ("SI_spotAttrGetValue", group!(spot_attr_get_value)),
        ("SI_sunAttrGetValue", group!(sun_attr_get_value)),
        ("SI_scenesAttrGetValue", group!(scenes_attr_get_value)),
        ("SI_wavesAttrGetValue", group!(waves_attr_get_value)),
        ("SI_dummyAttrGetValue", group!(dummy_attr_get_value)),
    ])
    .run()
}
