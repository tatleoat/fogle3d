// SPDX-License-Identifier: MIT
//! The ports of unit shdb3 (shdb_hooks, the node-tree helpers, the flex buffer routines), kept apart from `main.rs`
//! so the lists merge cleanly.

use si_checkkit::Port;
use si_custom::custom_scene::*;
use si_custom::shdb_hooks::*;
use si_custom::shdb_lex::*;
use si_custom::shdb_support::*;

/// `base` followed by this unit's ports.
pub fn with(base: &[(&'static str, Port)]) -> Vec<(&'static str, Port)> {
    let mut v: Vec<(&'static str, Port)> = base.to_vec();
    v.extend_from_slice(&[
        ("SI_cusSceneCallbackCallAll", |m, o, a| si_cus_scene_callback_call_all(m, o, a.v("arg")).into()),
        ("SI_cusCheckRenderAbort", |m, o, _a| { si_cus_check_render_abort(m, o); ().into() }),
        ("elemGetUserDataList", |m, _o, a| elem_get_user_data_list(m, a.v("elem"), a.v("kind")).into()),
        ("SI_cusUserDataGetCallbackNodes", |m, _o, a| { si_cus_user_data_get_callback_nodes(m, a.v("list"), a.v("outcb"), a.v("outnext")); ().into() }),
        ("SI_cusElemCallbackCallAll", |m, o, a| si_cus_elem_callback_call_all(m, o, a.v("kind"), a.v("p1"), a.v("elem"), a.v("x")).into()),
        ("SI_cusChapIdInit", |m, _o, a| { si_cus_chap_id_init(m, a.v("src"), a.v("dst")); ().into() }),
        ("shaderFcurveRemoveFromEditor", |m, o, a| { shader_fcurve_remove_from_editor(m, o, a.v("sh")); ().into() }),
        ("shaderFcurveUpdateOwners", |m, _o, a| shader_fcurve_update_owners(m, a.v("sh")).into()),
        ("SI_udfDisposeAll", |m, o, _a| { si_udf_dispose_all(m, o); ().into() }),
        ("si_interrupt_checkflag", |m, _o, a| (si_interrupt_checkflag(m, a.v("mode") as i32) as u32).into()),
        ("si_print_errormessage", |m, o, a| { si_print_errormessage(m, o, a.v("text")); ().into() }),
        ("si_begin_gl", |_m, _o, _a| { si_begin_gl(); ().into() }),
        ("si_end_gl", |_m, _o, _a| { si_end_gl(); ().into() }),
        ("si_shdbFreeNode", |m, _o, a| { si_shdb_free_node(m, a.v("node")); ().into() }),
        ("si_shdbFreeTree", |m, _o, a| { si_shdb_free_tree(m, a.v("pp"), a.v("flags") as i32); ().into() }),
        ("si_shdbFreeAllNodes", |m, _o, _a| { si_shdb_free_all_nodes(m); ().into() }),
        ("si_shdbAddNode", |m, _o, a| (si_shdb_add_node(m, a.v("pp"), a.v("node"), a.v("mode") as i32) as u32).into()),
        ("si_shdbPrintTree", |m, o, a| { si_shdb_print_tree(m, o, a.v("fp"), a.v("node"), a.v("depth") as i32); ().into() }),
        ("SHDB_yy_flush_buffer", |m, _o, a| { shdb_yy_flush_buffer(m, a.v("b")); ().into() }),
        ("SHDB_yy_switch_to_buffer", |m, _o, a| { shdb_yy_switch_to_buffer(m, a.v("new")); ().into() }),
        ("SHDB_yy_delete_buffer", |m, _o, a| { shdb_yy_delete_buffer(m, a.v("b")); ().into() }),
        ("SHDB_yyrestart", |m, o, a| { shdb_yyrestart(m, o, a.v("file")); ().into() }),
        ("SHDB_yy_scan_buffer", |m, o, a| shdb_yy_scan_buffer(m, o, a.v("base"), a.v("size")).into()),
        ("SHDB_yy_scan_bytes", |m, o, a| shdb_yy_scan_bytes(m, o, a.v("bytes"), a.v("len") as i32).into()),
        ("SHDB_yy_scan_string", |m, o, a| shdb_yy_scan_string(m, o, a.v("s")).into()),
        ("0855133c", |m, o, a| { yyunput(m, o, a.v("c") as i32, a.v("text")); ().into() }),
        ("085513e8", |m, o, _a| (yyinput(m, o) as u32).into()),
        ("085517f0", |m, o, a| { yy_fatal_error(m, o, a.v("msg")); ().into() }),
        ("08551830", |m, _o, a| yy_flex_realloc(m, a.v("p"), a.v("size")).into()),
        ("0855184c", |m, _o, a| { yy_flex_free(m, a.v("p")); ().into() }),
    ]);
    crate::shdb4::with(&v)
}

/// The callbacks' arguments (ctx 8 words, info 3 words, user) appended to the capture global, and the abort flag
/// set by an answer of 0x33 (the generator's `cb_answer` does the same in the emulator); everything else goes to unit shdb2's hook.
const CAP2: u32 = 0x0878_8d00;
const CB_ADDR: u32 = 0x0854_a3d0;
const CB2_ADDR: u32 = 0x0854_a43c;

pub fn effect(m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32], answer: u32) -> Result<(), String> {
    if r.addr == CB_ADDR && a.len() == 3 {
        let n = m.u32(CAP2);
        let mut at = CAP2 + 4 + n;
        for i in 0..8u32 {
            let w = m.u32(a[0].wrapping_add(4 * i));
            m.set_u32(at, w);
            at += 4;
        }
        for i in 0..3u32 {
            let w = m.u32(a[1].wrapping_add(4 * i));
            m.set_u32(at, w);
            at += 4;
        }
        m.set_u32(at, a[2]);
        m.set_u32(CAP2, n + 48);
        if answer == 0x33 {
            let pd = m.u32(si_custom::custom_util::SI_PROC_DATA);
            let ab = m.u32(RENDER_ABORT);
            m.set_u16(ab.wrapping_add(m.u32(pd) * 2), 1);
        }
    }
    if r.addr == CB2_ADDR && a.len() == 4 {
        let n = m.u32(CAP2);
        let mut at = CAP2 + 4 + n;
        for (p, cnt) in [(a[0], 8u32), (a[1], 3), (a[2], 5)] {
            for i in 0..cnt {
                let w = m.u32(p.wrapping_add(4 * i));
                m.set_u32(at, w);
                at += 4;
            }
        }
        m.set_u32(at, a[3]);
        m.set_u32(CAP2, n + 68);
    }
    crate::udf3::effect(m, r, a, answer)
}
