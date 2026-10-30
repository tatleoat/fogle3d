// SPDX-License-Identifier: MIT
//! `user-checkkit <directory of vector files>`: `user.c`'s VC* wrappers on `si-checkkit` against the emulated original
//! (generator `re/tools/subreg1/gen_user.py`).

use si_checkkit::Kit;
use si_render::user::*;

fn main() {
    Kit::new(&[
        ("VCInit", |m, _o, _a| vc_init(m).into()),
        ("VCReInit", |m, _o, _a| vc_re_init(m).into()),
        ("VCRmAllTri", |m, _o, _a| vc_rm_all_tri(m).into()),
        ("VCCreatePrim", |m, _o, a| vc_create_prim(m, a.i("kind"), a.v("matrix"), a.v("out")).into()),
        ("VCSetPrimParam", |m, _o, a| vc_set_prim_param(m, a.i("handle"), a.i("which"), a.v("p")).into()),
        ("VCGetPrimParam", |m, _o, a| vc_get_prim_param(m, a.i("handle"), a.i("which"), a.v("out")).into()),
        ("VCDeletePrim", |m, _o, a| vc_delete_prim(m, a.i("handle")).into()),
        ("VCAddToGroup", |m, _o, a| vc_add_to_group(m, a.i("handle"), a.i("group")).into()),
        ("VCUnBlendGroups", |m, _o, a| vc_unblend_groups(m, a.i("a"), a.i("b")).into()),
        ("VCInfoPrim", |m, _o, a| vc_info_prim(m, a.i("handle"), a.v("a"), a.v("b")).into()),
        ("VCGenPolyData", |m, _o, a| vc_gen_poly_data(m, a.i("mode"), a.i("level"), a.v("bounds"), a.v("box"), a.v("nv"), a.v("vtx"), a.v("np"), a.v("poly")).into()),
    ])
    .setup(|m| {
        si_render::io::init_rodata(m);
        // the eight corners of the cube, each a triple of plus/minus one (gen_vol.py's table)
        for c in 0..8u32 {
            for k in 0..3u32 {
                m.set_f64(si_render::volume::CORNERS + 24 * c + 8 * k, if c >> k & 1 == 1 { 1.0 } else { -1.0 });
            }
        }
    })
    .allow_global_write("GW:vcG_voxList+0x0")
    .allow_global_write("GW:vcG_voxList+0x3")
    .allow_global_write("GW:update_flag.249+0x0")
    .reset_globals(&[(0x0882_99d0, 0), (0x086e_e4a4, 0)])
    .normalise_stack_args()
    .given_lib_column()
    .run()
}
