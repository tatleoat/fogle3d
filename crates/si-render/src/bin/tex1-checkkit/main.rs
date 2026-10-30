// SPDX-License-Identifier: MIT
//! `tex1-checkkit <directory of vector files>`: the leftovers of the texture and material code
//! (`tex_leftovers.rs`) on `si-checkkit` against the emulated original (vectors from
//! `re/tools/tex1/gen_tex1.py`, finding 0012, section 35).

mod files;

use si_checkkit::Kit;
use si_render::tex_leftovers::*;

fn main() {
    Kit::new(&[
        ("SI_texture2DSetDirty", |m, o, a| { texture2d_set_dirty(m, o, a.p::<()>("tex").addr()); ().into() }),
        ("SI_texture3DSetDirty", |m, o, a| { texture3d_set_dirty(m, o, a.p::<()>("tex").addr()); ().into() }),
        ("SI_materialSetDirty", |m, o, a| { material_set_dirty(m, o, a.p::<()>("mat").addr(), a.i("flags")); ().into() }),
        ("si_mdlTxtSetDirtyHW", |m, _o, a| { mdl_txt_set_dirty_hw(m, a.p::<()>("model").addr(), a.i("flag")); ().into() }),
        ("SI_updTxt2dEffects", |m, _o, a| { upd_txt2d_effects(m, a.p::<()>("map").addr()); ().into() }),
        ("SI_setAreaRect", |m, _o, a| { set_area_rect(m, a.p::<()>("light").addr()); ().into() }),
        ("SI_setAreaDisk", |m, _o, a| { set_area_disk(m, a.p::<()>("light").addr()); ().into() }),
        ("isBlank", |_m, _o, a| is_blank(a.i("c")).into()),
        ("SI_stripPicExtension", |m, _o, a| strip_pic_extension(m, a.p::<()>("text").addr()).into()),
        ("buildDefaultPicName__FP11__SI_LclMapi", |m, o, a| {
            files::begin(a);
            build_default_pic_name(m, o, a.p::<()>("map").addr(), a.i("frame"));
            ().into()
        }),
        ("SI_setFileName", |m, o, a| {
            files::begin(a);
            set_file_name(m, o, a.p::<()>("map").addr(), a.i("frame"));
            files::emit_err(o);
            ().into()
        }),
        ("SI_camAspSet", |m, _o, a| cam_asp_set(m, a.p::<()>("cam").addr(), a.i("idx")).into()),
        ("SI_changeCameraAspect", |m, o, a| { change_camera_aspect(m, o, a.p::<()>("cam").addr(), a.i("idx")); ().into() }),
    ])
    .setup(|m| {
        // the texts the routines hand to the C library (the binary's .rodata, as the cases need them)
        use si_render::tex_leftovers::text::*;
        for (at, s) in [
            (DOT_D, &b".%d"[..]),
            (S_DOT_D, b"%s.%d"),
            (MODE_R, b"r"),
            (UNKNOWN_ANIMATION, b"Unknown texture animation."),
            (T2D_DIAL_PATH, b"/home/data/si3d/OfficialBuilds/v4_0_2_0/Creative/final.v4_0_2_0-Linux24-flexlm/matter/t2d_dial.c"),
            (UNEXPECTED_DEFAULT, b"Unexpected default in file '%s' at line %d:\n   %s\n"),
            (S_NOICON, b"%s/noIcon"),
            (NO_FRAME, b"Cannot find frame %d in script '%s'. Using default picture."),
        ] {
            m.set_bytes(at, s);
            m.set_u8(at + s.len() as u32, 0);
        }
    })
    .route(files::route)
    .run()
}
