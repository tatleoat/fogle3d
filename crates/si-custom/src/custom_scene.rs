// SPDX-License-Identifier: MIT
//! Scene callbacks and shader f-curve housekeeping of the custom effects (finding 0041, unit shdb3):
//! the render-time walk over the scene callbacks (`SI_cusSceneCallbackCallAll`, `SI_cusCheckRenderAbort`),
//! the chapter id copy, and three small `udf_fct.c` functions.

use crate::custom_io::SI_CUS_SET_CURRENT_DSO_EFFECT;
use crate::custom_udf::{si_udf_list_pointer_update, SI_UDF_DISPOSE};
use crate::custom_util::{si_cus_context_init, SI_PROC_DATA};
use si_mem::{clib, Mem, Outside, Routine};

pub const SI_STATUS_DISPLAY_MESSAGE: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);
pub const LONGJMP: Routine = Routine::new("plt:longjmp", 0x080c_4458);
pub const SI_FCV_SCNGL_B_DELETE: Routine = Routine::new("SI_fcvScnglBDelete", 0x0823_c8dc);
pub const SI_FCV_SCROLL_BUILD: Routine = Routine::new("SI_fcvScrollBuild", 0x0839_78d8);
/// `sceneCallbackLists` (per scene: the head of a list of 0x14-byte callback records) and `renderAbort` (per scene: a short).
pub const SCENE_CALLBACK_LISTS: u32 = 0x086d_f32c;
pub const RENDER_ABORT: u32 = 0x086d_f330;
pub const SI_STANDALONE_RENDERER: u32 = 0x0867_9400;
/// The chapter id record the callbacks get a copy of (0x84 bytes at this address).
pub const CHAP_ID: u32 = 0x0878_1660;
/// Text shown when a callback reports a fatal error (the effect's name goes in front of it).
const S_FATAL_SUFFIX: u32 = 0x086e_01c0;
const S_PERCENT_SS: u32 = 0x086e_0201;
/// `SI_renderJmp`: the jump buffer `SI_cusCheckRenderAbort` returns to.
pub const RENDER_JMP: u32 = 0x0879_0be0;

/// [soft 0x0854bd00 SI_cusChapIdInit]: copies the 0x84 bytes at `src` to `dst` (nothing when either is null).
pub fn si_cus_chap_id_init(m: &mut Mem, src: u32, dst: u32) {
    if src == 0 || dst == 0 {
        return;
    }
    for i in 0..0x21u32 {
        let w = m.u32(src.wrapping_add(i * 4));
        m.set_u32(dst.wrapping_add(i * 4), w);
    }
}

/// [soft 0x08535ac4 SI_cusSceneCallbackCallAll]: calls the scene callbacks of the current scene (a record:
/// +0 the effect, +4 the function, +8 its user word, +0xc stop-on-abort flag, +0x10 the next record).
/// Answers 0x10 when `arg` is not 0, 6 when a record lacks its function (or, outside the standalone renderer,
/// its effect), else 0. A callback answering 0x5f reports a fatal error (message with the effect's name) and
/// 0x7c or 0x5f disable the effect (+0x100 cleared, +0x118 set to 1). The walk stops at a callback that answers 0
/// when the record asks for it and the scene's abort flag is 1.
pub fn si_cus_scene_callback_call_all(m: &mut Mem, o: &mut dyn Outside, arg: u32) -> u32 {
    let pd = m.u32(SI_PROC_DATA);
    let scene = m.u32(pd);
    let lists = m.u32(SCENE_CALLBACK_LISTS);
    if m.u32(lists.wrapping_add(scene * 4)) == 0 {
        return 0;
    }
    if arg != 0 {
        return 0x10;
    }
    let loc = m.local(0xe0);
    let (status, info, ctx, chap) = (loc, loc + 0x24, loc + 4, loc + 0x50);
    m.set_u32(status, 0);
    m.set_u32(info, scene);
    let frame = m.u32(pd.wrapping_add(4));
    m.set_u32(info + 4, frame);
    m.set_u32(info + 8, frame.wrapping_add(1));
    si_cus_chap_id_init(m, CHAP_ID, chap);
    let pd = m.u32(SI_PROC_DATA);
    let mut n = m.u32(m.u32(SCENE_CALLBACK_LISTS).wrapping_add(m.u32(pd) * 4));
    while n != 0 {
        let eff = m.u32(n);
        let f = m.u32(n.wrapping_add(4));
        let standalone = m.u16(SI_STANDALONE_RENDERER) != 0;
        let mut skip = false;
        let mut run = true;
        if f == 0 || (!standalone && eff == 0) {
            m.set_u32(status, 6);
            skip = true;
        } else if eff != 0 && m.u16(eff.wrapping_add(0x118)) == 1 {
            skip = true;
        }
        if !skip {
            si_cus_context_init(m, ctx, eff, 0xf, 0x13, 0xffff_ffff);
            o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[eff]);
            let user = m.u32(n.wrapping_add(8));
            let r = o.call(m, Routine::at(f), &[ctx, info, user]).eax;
            o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[0]);
            let eff = m.u32(n);
            if eff != 0 {
                if r == 0x5f || r == 0x7c {
                    if r == 0x5f {
                        let name = m.u32(eff.wrapping_add(4));
                        let len = clib::strlen(m, S_FATAL_SUFFIX).wrapping_add(clib::strlen(m, name)).wrapping_add(1);
                        let buf = m.si_malloc(len);
                        if buf != 0 {
                            crate::cfmt::sprintf(m, buf, S_PERCENT_SS, &[name, S_FATAL_SUFFIX]);
                            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[buf, 3]);
                            m.free(buf);
                        }
                    }
                    let eff = m.u32(n);
                    m.set_u32(eff.wrapping_add(0x100), 0);
                    m.set_u16(eff.wrapping_add(0x118), 1);
                }
            }
            if r == 0 && m.u32(n.wrapping_add(0xc)) == 0 {
                let pd = m.u32(SI_PROC_DATA);
                let ab = m.u32(RENDER_ABORT);
                if m.i16(ab.wrapping_add(m.u32(pd) * 2)) == 1 {
                    run = false;
                }
            }
        }
        if !run {
            break;
        }
        n = m.u32(n.wrapping_add(0x10));
    }
    let r = m.u32(status);
    m.release(0xe0);
    r
}

/// [soft 0x08535d30 SI_cusCheckRenderAbort]: clears the scene's abort flag, runs the scene callbacks and, when one
/// set the flag to 1, jumps back to the renderer (`longjmp`).
pub fn si_cus_check_render_abort(m: &mut Mem, o: &mut dyn Outside) {
    let pd = m.u32(SI_PROC_DATA);
    let ab = m.u32(RENDER_ABORT);
    m.set_u16(ab.wrapping_add(m.u32(pd) * 2), 0);
    si_cus_scene_callback_call_all(m, o, 0);
    let pd = m.u32(SI_PROC_DATA);
    let ab = m.u32(RENDER_ABORT);
    if m.u16(ab.wrapping_add(m.u32(pd) * 2)) == 1 {
        o.call(m, LONGJMP, &[RENDER_JMP, 0]);
    }
}

/// [soft 0x0854c524 shaderFcurveRemoveFromEditor]: deletes the four f-curve scroll items of every key in the
/// shader's key chain (+0x2c, next +0x44) and rebuilds the scroll list. Does nothing for a null shader.
pub fn shader_fcurve_remove_from_editor(m: &mut Mem, o: &mut dyn Outside, shader: u32) {
    if shader == 0 {
        return;
    }
    let mut key = m.u32(shader.wrapping_add(0x2c));
    while key != 0 {
        for k in 0..4u32 {
            let item = m.u32(key.wrapping_add(0x2c + k * 4));
            if item != 0 {
                o.call(m, SI_FCV_SCNGL_B_DELETE, &[item, 0]);
            }
        }
        key = m.u32(key.wrapping_add(0x44));
    }
    o.call(m, SI_FCV_SCROLL_BUILD, &[]);
}

/// [soft 0x0854c588 SI_udfDisposeAll]: disposes every user shader in the list, then empties it.
pub fn si_udf_dispose_all(m: &mut Mem, o: &mut dyn Outside) {
    let mut i = 0i32;
    while i < m.u32(0x086e_49d8) as i32 {
        let list = m.u32(0x086e_49d4);
        let u = m.u32(list.wrapping_add(i as u32 * 4));
        if u != 0 {
            o.call(m, SI_UDF_DISPOSE, &[u]);
        }
        i += 1;
    }
    m.set_u32(0x086e_49d4, 0);
    m.set_u32(0x086e_49d8, 0);
    si_udf_list_pointer_update(m);
}

/// [soft 0x0854ca48 shaderFcurveUpdateOwners]: points the owner word (+4) of each f-curve scroll item of the shader's
/// key chain at the shader. Answers 0 for a null shader, else 1.
pub fn shader_fcurve_update_owners(m: &mut Mem, shader: u32) -> u32 {
    if shader == 0 {
        return 0;
    }
    let mut key = m.u32(shader.wrapping_add(0x2c));
    while key != 0 {
        for k in 0..4u32 {
            let item = m.u32(key.wrapping_add(0x2c + k * 4));
            if item != 0 {
                m.set_u32(item.wrapping_add(4), shader);
            }
        }
        key = m.u32(key.wrapping_add(0x44));
    }
    1
}

/// [soft 0x0854b8cc elemGetUserDataList]: the user data list of an element for a kind of element (2, 6, 7, 8, 17
/// and 18 have one, at offsets 0x138, 0x220, 0xdc4, 0x34c, 0x1d0 and 0x154 of the record the element points at
/// with +4); 0 for the other kinds, a null element or a null record.
pub fn elem_get_user_data_list(m: &mut Mem, elem: u32, kind: u32) -> u32 {
    if elem == 0 {
        return 0;
    }
    let rec = m.u32(elem.wrapping_add(4));
    if rec == 0 {
        return 0;
    }
    let off = match kind {
        2 => 0x138,
        6 => 0x220,
        7 => 0xdc4,
        8 => 0x34c,
        17 => 0x1d0,
        18 => 0x154,
        _ => return 0,
    };
    m.u32(rec.wrapping_add(off))
}

/// [soft 0x0854b928 SI_cusUserDataGetCallbackNodes]: with both outputs given, clears them and walks the list
/// (next at +0x10) for the nodes named "CUSELEMCB" (into `out_cb`) and "CUSNEXTELEMCB" (into `out_next`), the
/// last one of each name winning until both are set; the walk ends at the first node that leaves both set.
pub fn si_cus_user_data_get_callback_nodes(m: &mut Mem, list: u32, out_cb: u32, out_next: u32) {
    if out_cb == 0 || out_next == 0 {
        return;
    }
    m.set_u32(out_cb, 0);
    m.set_u32(out_next, 0);
    let mut n = list;
    while n != 0 {
        let name = m.u32(n);
        let s = m.cstr(name);
        if s == b"CUSELEMCB" {
            m.set_u32(out_cb, n);
            if m.u32(out_next) != 0 {
                return;
            }
        } else if s == b"CUSNEXTELEMCB" {
            m.set_u32(out_next, n);
            if m.u32(out_cb) != 0 {
                return;
            }
        }
        n = m.u32(n.wrapping_add(0x10));
    }
}

/// Texts of `SI_cusElemCallbackCallAll`: the suffix of the fatal error message and its format ("%s%s").
const S_ELEM_FATAL_SUFFIX: u32 = 0x086e_4640;
const S_ELEM_PERCENT_SS: u32 = 0x086e_4683;

/// [soft 0x0854b614 SI_cusElemCallbackCallAll]: runs the element callbacks of an element (`elem`, its user data
/// list selected by `kind`). Answers 0x10 for a null element, an element without its +4 record or a non-zero
/// `x`; 0 when the list has neither of the two callback nodes, 6 when it has only one. The "next" node's +0xc
/// points to a counter: it is set to 0, each pass takes the record of the "callback" node's table (12-byte
/// records from +0xc, +8 holds the table's size in bytes) the counter points at and adds 1 to it, and the walk ends
/// when the counter leaves 0..count (a callback can move it) and leaves -1 in it. A record without its effect or
/// function gives status 6; one whose effect has +0x118 equal 1 is skipped. Callbacks get (context, scene info,
/// element info, user word); the answers 0x5f (message with the effect's name) and 0x7c disable the effect.
pub fn si_cus_elem_callback_call_all(m: &mut Mem, o: &mut dyn Outside, kind: u32, p1: u32, elem: u32, x: u32) -> u32 {
    if elem == 0 || m.u32(elem.wrapping_add(4)) == 0 || x != 0 {
        return 0x10;
    }
    let loc = m.local(0xc4);
    let (ctx, info2, chap, info1) = (loc, loc + 0x20, loc + 0x2c, loc + 0xb0);
    let list = elem_get_user_data_list(m, elem, kind);
    let cbv = m.local(8);
    si_cus_user_data_get_callback_nodes(m, list, cbv, cbv + 4);
    let (cb, next) = (m.u32(cbv), m.u32(cbv + 4));
    m.release(8);
    if next == 0 {
        m.release(0xc4);
        return if cb == 0 { 0 } else { 6 };
    }
    if cb == 0 {
        m.release(0xc4);
        return 6;
    }
    let pd = m.u32(SI_PROC_DATA);
    let scene = m.u32(pd);
    m.set_u32(info1, scene);
    m.set_u32(info1 + 4, kind);
    m.set_u32(info1 + 8, p1);
    m.set_u32(info1 + 0xc, 5);
    m.set_u32(info1 + 0x10, 0);
    m.set_u32(info2, m.u32(pd));
    let frame = m.u32(pd.wrapping_add(4));
    m.set_u32(info2 + 4, frame);
    m.set_u32(info2 + 8, frame.wrapping_add(1));
    si_cus_chap_id_init(m, CHAP_ID, chap);
    let counter = m.u32(next.wrapping_add(0xc));
    m.set_u32(counter, 0);
    let mut status = 0u32;
    loop {
        let counter = m.u32(m.u32(next.wrapping_add(0xc)));
        let idx = counter as i32;
        if idx < 0 || idx >= (m.u32(cb.wrapping_add(8)) as i32) / 12 {
            break;
        }
        let rec = m.u32(cb.wrapping_add(0xc)).wrapping_add(counter.wrapping_mul(12));
        let cptr = m.u32(next.wrapping_add(0xc));
        m.set_u32(cptr, counter.wrapping_add(1));
        let eff = m.u32(rec);
        let f = m.u32(rec.wrapping_add(4));
        if f == 0 || eff == 0 {
            status = 6;
            continue;
        }
        if m.u16(eff.wrapping_add(0x118)) == 1 {
            continue;
        }
        si_cus_context_init(m, ctx, eff, 0xd, 0xf, 0xffff_ffff);
        let user = m.u32(rec.wrapping_add(8));
        let r = o.call(m, Routine::at(f), &[ctx, info2, info1, user]).eax;
        if r == 0x5f || r == 0x7c {
            if r == 0x5f {
                let eff = m.u32(rec);
                let name = m.u32(eff.wrapping_add(4));
                let len = clib::strlen(m, name).wrapping_add(0x43);
                let buf = m.si_malloc(len);
                if buf != 0 {
                    crate::cfmt::sprintf(m, buf, S_ELEM_PERCENT_SS, &[name, S_ELEM_FATAL_SUFFIX]);
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[buf, 3]);
                    m.free(buf);
                }
            }
            let eff = m.u32(rec);
            m.set_u32(eff.wrapping_add(0x100), 0);
            m.set_u16(eff.wrapping_add(0x118), 1);
        }
    }
    let cptr = m.u32(next.wrapping_add(0xc));
    m.set_u32(cptr, 0xffff_ffff);
    m.release(0xc4);
    status
}
