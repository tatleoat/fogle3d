// SPDX-License-Identifier: MIT
//! `modelparam-checkkit <directory of vector files>`: `SI_modelGetParam` and `SI_modelSetParam` (`si_expr::image::modelparam`) on
//! `si-checkkit` against the emulated original (finding 0013, section 20). Vectors: `re/tools/params2/gen_model.py`.
use si_checkkit::Kit;
use si_expr::image::modelparam::*;

/// The three global setters of `model.c` are stand-ins that keep their vector in the node (`gen_model.py` has the same bodies).
fn set_vec(m: &mut si_mem::Mem, a: &[u32], off: u32) -> si_mem::Ret {
    for k in 0..3 {
        let w = m.u32(a[1] + 4 * k);
        m.set_u32(a[0] + off + 4 * k, w);
    }
    si_mem::Ret::eax(0)
}

fn main() {
    // the parser's tables are read from the user's install (docs/design/runtime-data.md)
    si_rt::check_or_exit("modelparam-checkkit");
    Kit::new(&[
        ("SI_modelGetParam", |m, o, a| model_get_param(m, o, a.i("t"), a.v("mdl"), a.v("fcv"), si_mem::Ptr32::new(a.v("out")), a.i("coord")).into()),
        ("SI_modelSetParam", |m, o, a| model_set_param(m, o, a.i("t"), a.v("mdl"), a.v("fcv"), si_mem::Ptr32::new(a.v("value")), a.i("coord")).into()),
        ("SI_modelAttrGetValue", |m, o, a| {
            model_attr_get_value(m, o, si_mem::Ptr32::new(a.v("ident")), si_mem::Ptr32::new(a.v("index")), si_mem::Ptr32::new(a.v("value"))).into()
        }),
    ])
    .stand_in("SI_modelSetGlobalScaling", |m, a| set_vec(m, a, 0x16c))
    .stand_in("SI_modelSetGlobalRotation", |m, a| set_vec(m, a, 0x178))
    .stand_in("SI_modelSetGlobalTranslation", |m, a| set_vec(m, a, 0x1b8))
    .run()
}
