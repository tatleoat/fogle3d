// SPDX-License-Identifier: MIT
//! `meta-checkkit <directory of vector files>`: `metaball.c` on `si-checkkit` against the
//! emulated original (vectors of `re/tools/si-meta/gen_meta.py`; finding 0038 and 0011,
//! "A check binary with si-checkkit").

use si_checkkit::Kit;
mod files;

use si_meta::adopt::*;
use si_meta::corp::*;
use si_meta::metaball::*;
use si_meta::animation::*;
use si_meta::surface::*;
use si_meta::traverse::*;

/// Where the callback stand-in keeps the points it is handed (gen_meta.py has the same `ACC`).
const ACC: u32 = 0x0881_82a0;

fn main() {
    Kit::new(&[
        ("SI_readMetaCorp", |m, o, a| {
            files::begin(a);
            si_read_meta_corp(m, o, a.p::<()>("name").addr());
            ().into()
        }),
        ("SI_addMetaball", |m, o, _a| {
            si_add_metaball(m, o);
            ().into()
        }),
        ("SI_getActiveMetaSystem", |m, o, a| si_get_active_meta_system(m, o, a.i("create") as i16).into()),
        ("SI_addMetaSystem", |m, o, _a| {
            si_add_meta_system(m, o);
            ().into()
        }),
        ("SI_metaballAddInSystem", |m, o, a| {
            si_metaball_add_in_system(m, o, a.p::<()>("node").addr(), a.p::<()>("sys").addr());
            ().into()
        }),
        ("metaGroupRemovePrimitive", |m, _o, a| {
            meta_group_remove_primitive(m, a.p::<()>("group").addr(), a.p::<()>("ball").addr());
            ().into()
        }),
        ("SI_metaballDispose", |m, _o, a| {
            metaball_dispose(m, a.p::<()>("pball").addr());
            ().into()
        }),
        ("SI_metaballCopy", |m, _o, a| metaball_copy(m, a.p::<()>("ball").addr(), a.p::<()>("model").addr()).into()),
        ("deleteUnBlendList", |m, _o, a| {
            delete_unblend_list(m, a.p::<()>("first").addr());
            ().into()
        }),
        ("deleteUnBlendNode", |m, _o, a| delete_unblend_node(m, a.p::<()>("sys").addr(), a.p::<()>("pair").addr()).into()),
        ("deletePrimLink", |m, _o, a| delete_prim_link(m, a.p::<()>("group").addr(), a.p::<()>("link").addr()).into()),
        ("getUnblendGrpPt", |m, _o, a| {
            get_unblend_grp_pt(m, a.p::<()>("dst").addr(), a.p::<()>("src").addr(), a.p::<()>("target").addr()).into()
        }),
        ("metaUnblendCopy", |m, _o, a| {
            meta_unblend_copy(m, a.p::<()>("dst").addr(), a.p::<()>("src").addr());
            ().into()
        }),
        ("metaGroupElementsCopy", |m, _o, a| {
            meta_group_elements_copy(m, a.p::<()>("sys").addr(), a.p::<()>("src").addr());
            ().into()
        }),
        ("addMetaPrimtLnk", |m, _o, a| {
            add_meta_primt_lnk(m, a.p::<()>("sys").addr());
            ().into()
        }),
        ("metaGroupAddPrimitive", |m, _o, a| {
            meta_group_add_primitive(m, a.p::<()>("group").addr(), a.p::<()>("ball").addr());
            ().into()
        }),
        ("metaSystemCopyAll", |m, _o, a| meta_system_copy_all(m, a.p::<()>("sys").addr()).into()),
        ("metaSystemRemoveGroup", |m, _o, a| {
            meta_system_remove_group(m, a.p::<()>("sys").addr(), a.p::<()>("group").addr());
            ().into()
        }),
        ("SI_metaSystemDispose", |m, _o, a| {
            meta_system_dispose(m, a.p::<()>("psys").addr());
            ().into()
        }),
        ("si_metaSystemCopy", |m, _o, a| si_meta_system_copy(m, a.p::<()>("sys").addr(), a.p::<()>("model").addr()).into()),
        ("metaGroupChangeName", |m, _o, a| {
            meta_group_change_name(m, a.p::<()>("group").addr(), a.p::<()>("name").addr());
            ().into()
        }),
        ("metaGroupToggleBlend", |m, _o, a| {
            meta_group_toggle_blend(m, a.p::<()>("sys").addr(), a.p::<()>("a").addr(), a.p::<()>("b").addr());
            ().into()
        }),
        ("SI_metaSystemValidateGroups", |m, _o, a| {
            meta_system_validate_groups(m, a.p::<()>("sys").addr());
            ().into()
        }),
        ("metaSysGroupErrDetect", |m, _o, a| meta_sys_group_err_detect(m, a.p::<()>("sys").addr()).into()),
        ("SI_metaSystemResolveGroupName", |m, o, a| {
            meta_system_resolve_group_name(m, o, a.p::<()>("sys").addr());
            ().into()
        }),
        ("getGroupsList", |m, _o, a| get_groups_list(m, a.p::<()>("first").addr()).into()),
        ("SI_metaballTouch", |m, o, a| {
            metaball_touch(m, o, a.p::<()>("ball").addr(), a.p::<()>("sys").addr());
            ().into()
        }),
        ("SI_metaSysGetSurface", |m, o, a| {
            si_meta_sys_get_surface(m, o, a.p::<()>("sys").addr(), a.i("mode") as i16, a.i("render") as i16);
            ().into()
        }),
        ("SI_metaGroupCopy", |m, _o, a| {
            si_meta_group_copy(m, a.p::<()>("dst").addr(), a.p::<()>("src").addr());
            ().into()
        }),
        ("metaballSetWeight", |m, o, a| {
            metaball_set_weight(m, o, a.p::<()>("ball").addr(), a.f("w"));
            ().into()
        }),
        ("SI_metaSystemVolumeSurface", |m, o, a| {
            si_meta_system_volume_surface(m, o, a.p::<()>("sys").addr(), a.p::<()>("vol").addr(), a.p::<()>("area").addr());
            ().into()
        }),
        ("si_metaballContourTraversePnts", |m, o, a| {
            si_metaball_contour_traverse_pnts(m, o, a.p::<()>("sys").addr(), a.p::<()>("matrix").addr(), a.v("checkLow"), a.v("data"));
            ().into()
        }),
        ("SI_metaballTraversePnts", |m, o, a| {
            si_metaball_traverse_pnts(m, o, a.p::<()>("pball").addr(), a.p::<()>("matrix").addr(), a.s("flag"), a.v("checkLow"), a.v("data"));
            ().into()
        }),
        ("SI_metaballSaveInternalParameters", |m, o, a| {
            si_metaball_save_internal_parameters(m, o, a.p::<()>("pball").addr(), a.i("frame"));
            ().into()
        }),
        ("SI_metaSystemSaveShape", |m, o, _a| {
            si_meta_system_save_shape(m, o);
            ().into()
        }),
        ("metaSystemCreateAssociationList", |m, _o, a| meta_system_create_association_list(m, a.p::<()>("a").addr(), a.p::<()>("b").addr()).into()),
        ("metaSystemFillAssociationList", |m, _o, a| {
            // the C library's drand48 from its start state, as the harness's stand-in has it
            let mut state: u64 = 0x1234_abcd_330e;
            let mut rand = move || {
                state = state.wrapping_mul(0x5_deec_e66d).wrapping_add(0xb) & 0xffff_ffff_ffff;
                state as f64 / (1u64 << 48) as f64
            };
            meta_system_fill_association_list(m, a.p::<()>("rec").addr(), a.i("mode"), &mut rand);
            ().into()
        }),
        ("metaAdoptShapeDispose", |m, _o, a| {
            meta_adopt_shape_dispose(m, a.p::<()>("prec").addr());
            ().into()
        }),
        ("metaSystemAdoptShape", |m, o, a| {
            meta_system_adopt_shape(m, o, a.p::<()>("rec").addr());
            ().into()
        }),
        ("metaCircleInit", |m, _o, _a| {
            meta_circle_init(m);
            ().into()
        }),
        ("SI_metaSystemPreserveVolume", |m, o, _a| {
            si_meta_system_preserve_volume(m, o);
            ().into()
        }),
    ])
    // the library's stand-ins that store through a pointer (gen_meta.py has the same bodies)
    .stand_in("checkLow", |m, a| {
        let n = m.u32(ACC);
        for k in 0..3 {
            let w = m.u32(a[0] + 4 * k);
            m.set_u32(ACC + 4 + 16 * n + 4 * k, w);
        }
        m.set_u32(ACC + 4 + 16 * n + 12, a[1]);
        m.set_u32(ACC, n + 1);
        si_mem::Ret::eax(0)
    })
    .stand_in("VCCreatePrim", |m, a| {
        m.set_u32(a[2], ((a[1] >> 3) & 0xfff) | 1);
        si_mem::Ret::eax(0)
    })
    .stand_in("VCInfoPrim", |m, a| {
        m.set_u32(a[2], a[0].wrapping_mul(7).wrapping_add(3) & 0xff);
        si_mem::Ret::eax(0)
    })
    .stand_in("VCGetPrimParam", |m, a| {
        m.set_f64(a[2], 0.25 + (a[0] & 0xff) as f64 * 0.125 + a[1] as f64);
        si_mem::Ret::eax(0)
    })
    .stand_in("VCGenPolyData", |m, a| {
        let sys = a[4].wrapping_sub(0x4c);
        m.set_u32(sys + 0x4c, a[1]);
        for k in 0..12 {
            let w = m.u32(a[2] + 4 * k);
            m.set_u32(sys + 0x68 + 4 * k, w);
        }
        si_mem::Ret::eax(0)
    })
    // the three setters keep their vector in the node (corp.rs reads nothing back; gen_meta.py has the same bodies)
    .stand_in("SI_modelSetScaling", |m, a| set_vec(m, a, 0x400))
    .stand_in("SI_modelSetRotation", |m, a| set_vec(m, a, 0x40c))
    .stand_in("SI_modelSetTranslation", |m, a| set_vec(m, a, 0x418))
    .stand_in("SI_modelAddScaling", |m, a| {
        for k in 0..3 {
            let w = m.u32(a[1] + 4 * k);
            m.set_u32(a[0] + 0x400 + 4 * k, w);
        }
        si_mem::Ret::eax(0)
    })
    .setup(|m| {
        // the strings of the program's .rodata that corp.rs passes by address
        for (a, t) in [
            (0x086d_5bd5u32, "%s.med"),
            (0x086d_5bdc, "r"),
            (0x086d_5bde, "meta"),
            (0x086d_5be3, "%*s %*c %f %f %f"),
            (0x086d_5bf4, "%*s %*c %f %f %f"),
            (0x086d_5c05, "%*s %*c %f %f %f"),
            (0x086d_5c16, "%*s %*c %f"),
            (0x086d_5c21, "ball"),
            (0x086d_5cb1, "ball"),
        ] {
            for (k, b) in t.bytes().chain([0u8]).enumerate() {
                m.set_u8(a + k as u32, b);
            }
        }
    })
    .route(files::route)
    .run()
}

fn set_vec(m: &mut si_mem::Mem, a: &[u32], off: u32) -> si_mem::Ret {
    for k in 0..3 {
        let w = m.u32(a[1] + 4 * k);
        m.set_u32(a[0] + off + 4 * k, w);
    }
    si_mem::Ret::eax(0)
}
