// SPDX-License-Identifier: MIT
//! `system.c` on the image: the routines of the editing commands that are not drawing.
//!
//! [soft 0x0841df84 cleanUpProjLists] frees each window's two projection blocks;
//! [soft 0x0841ea30 SI_updateDeformation] touches every model of the current graph list;
//! [soft 0x0841eb08 SI_pickGrafNode] reads events until one picks something;
//! [soft 0x0841e004 SI_deleteAll] clears the scene after the warning. Everything they call
//! that is another file's is a [`SystemCalls`] hook.

use super::calls::SystemCalls;
use super::g;
use si_mem::{Mem, Outside};
use si_x87::F80;

/// The window table of `SI_template`: the count (i16, +0x54) and the array (+0x60) of
/// 0x2b8-byte records.
const WINDOW_COUNT: u32 = 0x0878_20d4;
const WINDOWS: u32 = 0x0878_20e0;
const WINDOW_SIZE: u32 = 0x2b8;
/// `SI_audioContainer`: the head of the audio markers.
pub const AUDIO_CONTAINER: u32 = 0x087f_b9c0;

/// The text of the warning when the scene was changed, and the one for a clean scene.
pub const WARN_DIRTY: u32 = 0x086d_1e60;
pub const WARN_CLEAN: u32 = 0x086d_1e89;
pub const WARN_QUESTION: u32 = 0x086d_1e8b;
pub const WARN_LOSE: u32 = 0x086d_1ea0;

/// The current graph list's first node: `SI_grafLists[SI_procData->+4]`, 0x24 bytes each, +4.
fn graf_list_head(m: &Mem) -> u32 {
    let idx = m.i32(m[g::SI_PROC_DATA] + 4);
    m.u32(m[g::SI_GRAF_LISTS].wrapping_add((idx.wrapping_mul(9) as u32).wrapping_mul(4)).wrapping_add(4))
}

/// [soft 0x0841df84 cleanUpProjLists]: for every window (`SI_windowGetNext` fills the
/// local with the next one) unless the program is the batch renderer: the block at +0x40
/// loses the block at its +4, then is itself freed and the pointer cleared.
pub fn clean_up_proj_lists(m: &mut Mem, o: &mut dyn Outside) {
    let slot = m.local(4);
    m.set_u32(slot, 0);
    if m.i16(g::SI_STANDALONE_RENDERER.0) == 0 {
        while o.si_window_get_next(m, slot) != 0 {
            let w = m.u32(slot);
            let p = m.u32(w + 0x40);
            if p == 0 {
                continue;
            }
            let q = m.u32(p + 4);
            if q != 0 {
                m.free(q);
                let p = m.u32(m.u32(slot) + 0x40);
                m.set_u32(p + 4, 0);
            }
            let p = m.u32(m.u32(slot) + 0x40);
            if p != 0 {
                m.free(p);
                let w = m.u32(slot);
                m.set_u32(w + 0x40, 0);
            }
        }
    }
    m.release(4);
}

/// [soft 0x0841ea30 SI_updateDeformation]: for each node of the current graph list whose
/// kind is 2 or 3: if none of the spline fitting, lattice, patch fitting, node wave and
/// branch wave took it and the model's +0x44 is itself, nothing; otherwise the definition
/// is readjusted. A model of type 7 has its metaball touched. Then the counter is
/// refreshed. Answers 0.
pub fn update_deformation(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let s = m.local(4);
    let mut node = graf_list_head(m);
    while node != 0 {
        let kind = m.i32(node);
        if (2..=3).contains(&kind) {
            let model = m.u32(node + 4);
            let taken = o.si_model_spline_fitting(m, model, s) != 0
                || o.si_model_lattice(m, model, s) != 0
                || o.si_model_patch_fitting(m, model, s) != 0
                || o.si_model_node_wave(m, model) != 0
                || o.si_model_branch_wave(m, model) != 0;
            if taken || m.u32(model + 0x44) != model {
                o.si_model_readjust_definition(m, model);
            }
            if m.i32(model + 0x30) == 7 {
                let b = m.u32(model + 0x48);
                o.si_metaball_touch(m, 0, b);
            }
        }
        node = m.u32(node + 0x1c);
    }
    o.si_refresh_counter(m);
    m.release(4);
    0
}

/// [soft 0x0841eb08 SI_pickGrafNode]: `*out` is the event `SI_getEvent` answers. A key
/// press (kind 5) that `SI_checkSupra` takes becomes kind 0; kind 0x1a goes to
/// `SI_fastKey` and becomes 0. A press or kind 0 whose detail is 0x67, 0x66 or 0x65
/// with the matching bit 4, 2, 1 of `flags` set answers 0, as does any other kind. A press that `SI_gLSelect` (of
/// its +0x0c) resolves answers what that answered; otherwise the loop reads the next event.
pub fn pick_graf_node(m: &mut Mem, o: &mut dyn Outside, out: u32, flags: i32) -> u32 {
    loop {
        let ev = o.si_get_event(m);
        m.set_u32(out, ev);
        if m.fault().is_some() {
            // (the original would be killed by the fault; the loop would never end here)
            return 0;
        }
        if m.i32(ev) == 5 && o.si_check_supra(m, ev, 1) != 0 {
            let ev = m.u32(out);
            m.set_i32(ev, 0);
        }
        let ev = m.u32(out);
        if m.i32(ev) == 0x1a {
            o.si_fast_key(m, ev);
            let ev = m.u32(out);
            m.set_i32(ev, 0);
        }
        let ev = m.u32(out);
        let kind = m.i32(ev);
        if kind != 5 && kind != 0 {
            return 0;
        }
        let detail = m.u16(ev + 6);
        if (detail == 0x67 && flags & 4 != 0) || (detail == 0x66 && flags & 2 != 0) || (detail == 0x65 && flags & 1 != 0) {
            return 0;
        }
        if m.i32(ev) == 5 {
            let win = m.u32(ev + 0xc);
            let r = o.si_gl_select(m, win);
            if r != 0 {
                return r;
            }
        }
    }
}

/// [soft 0x0841e004 SI_deleteAll]: nothing to do when the current graph list is empty and
/// there are no audio markers. Otherwise, if the scene was changed (or not) the warning
/// ([`WARN_DIRTY`] or a blank) is shown through `SI_systemWarning` (not in the batch
/// renderer) and a refusal ends the call. Then edit-normals mode ends, the audio markers
/// are deleted one by one, the camera's two constraint kinds (4 and 5) are relaxed, the
/// scene file name, matter fade, scene objects and projection lists are cleared; outside
/// the batch renderer the screen is refreshed. Every window loses its block at +0x70 and
/// the camera slot it names loses its +0x24.
pub fn delete_all(m: &mut Mem, o: &mut dyn Outside) {
    let sp = m.sp();
    if graf_list_head(m) == 0 && o.si_audio_mng_at_count(m, AUDIO_CONTAINER) <= 0 {
        return;
    }
    let buf = m.local(0x200);
    if o.si_scene_dirty(m) != 0 {
        for i in 0..41 {
            let b = m.u8(WARN_DIRTY + i);
            m.set_u8(buf + i, b);
        }
    } else {
        m.set_u8(buf, b' ');
        m.set_u8(buf + 1, 0);
    }
    if m.i16(g::SI_STANDALONE_RENDERER.0) != 1 && o.si_system_warning(m, WARN_LOSE, buf, WARN_QUESTION) == 0 {
        m.set_sp(sp);
        return;
    }
    o.si_edit_normals_end(m);
    let out = m.local(4);
    loop {
        let c = m.u32(AUDIO_CONTAINER);
        if c == 0 {
            break;
        }
        let next = m.u32(c + 0xc);
        o.si_audio_mng_at_delete(m, AUDIO_CONTAINER, next, 0, out);
    }
    let cam = m.u32(m[g::SI_PROC_DATA] + 8);
    o.si_item_relax_constraint(m, cam, 4);
    let cam = m.u32(m[g::SI_PROC_DATA] + 8);
    o.si_item_relax_constraint(m, cam, 5);
    o.si_scene_file_reset(m);
    o.si_matter_fade_init(m);
    o.si_scene_delete_all_not_in_history(m);
    clean_up_proj_lists(m, o);
    o.si_fcv_scngl_clear(m);
    if m.i16(g::SI_STANDALONE_RENDERER.0) == 0 {
        o.si_refresh_all(m, 0);
        o.si_refresh_counter(m);
    }
    let count = m.i16(WINDOW_COUNT) as i32;
    let mut i = 0;
    if count > 0 {
        loop {
            let w = m.u32(WINDOWS).wrapping_add(i * WINDOW_SIZE);
            let p = m.u32(w + 0x70);
            if p != 0 {
                m.free(p);
                m.set_u32(w + 0x70, 0);
            }
            let c = m.u32(w + 0x2c);
            m.set_u32(w + 0x70, 0);
            let id = o.si_cam_id_convert(m, m.i16(c + 4) as i32);
            let slot = m.u32(w + 0x30).wrapping_add((id as u32).wrapping_mul(0x8c)).wrapping_add(0x24);
            m.set_u32(slot, 0);
            i += 1;
            if (i as i32) >= m.i16(WINDOW_COUNT) as i32 {
                break;
            }
        }
    }
    m.set_sp(sp);
}

/// The `f32` at 0x865d9a4 (read-only data; [`super::RODATA`]): the limit `mdlZoom` clamps
/// the corners it gets back to.
pub const ZOOM_LIMIT: f32 = f32::from_bits(0x7f4c_cccc);

/// The clamp of one corner coordinate: beyond the limit it becomes the limit (`1 - limit`
/// and `1 + limit` are worked out in the unit's registers, rounded to a double, which the
/// C library's `copysign` gives the sign of the coordinate; the result is stored as a float).
fn clamp_coordinate(v: f32, plus: bool) -> f32 {
    let limit = ZOOM_LIMIT;
    if !(F80::from_f32(v).abs().gt(F80::from_f32(limit))) {
        return v;
    }
    let one = F80::ONE;
    let mag = if plus { one + limit as f64 } else { F80::from_f32(limit) - one };
    let r = mag.to_f64().copysign(F80::from_f32(v).to_f64());
    F80::from_f64(r).to_f32()
}

/// [soft 0x0841db40 mdlZoom]: the camera follows a zoom rectangle drawn in a window. `win`
/// is the window (+0x2c its camera, +0x64 its view kind), `v*` the rectangle the view
/// covered, `r*` the one drawn, `button` 0x65, 0x66 or 0x67. Nothing changes (answer 0)
/// when the drawn rectangle has no width or no height or a corner cannot be
/// resolved. The camera position is saved first unless its +0x44 is set; a camera with +0x0c
/// set (cropped view) goes to `SI_dovCrop` alone. In a view of kind 2 or 3 the crop and the
/// orthographic setting are done by `SI_dovCrop` and `SI_dovSetCamOrtho`. Otherwise the
/// corners are turned into world positions with `SI_screenToWorld`: with button 0x65
/// from the rectangle scaled by the ratio of the old rectangle to the drawn one (whole numbers,
/// truncated, and kept to 16 bits), with 0x66 and 0x67 from the drawn rectangle as it is
/// (the camera of kind 0x7d4 gets `SI_setCamView`, the others `SI_setCamOrthoView`);
/// huge coordinates are clamped to the limit. Answers 1 once it has asked a callee.
#[allow(clippy::too_many_arguments)]
pub fn mdl_zoom(m: &mut Mem, o: &mut dyn Outside, win: u32, vx0: i32, vx1: i32, vy0: i32, vy1: i32, rx0: i32, rx1: i32, ry0: i32, ry1: i32, button: i32) -> i32 {
    let cam = m.u32(win + 0x2c);
    if rx0 == rx1 || ry0 == ry1 {
        return 0;
    }
    if m.i16(cam + 0x44) == 0 {
        o.si_do_save_cam_pos(m, win);
    }
    if m.i16(cam + 0xc) != 0 {
        o.si_dov_crop(m, win, rx0, rx1, ry0, ry1, button);
        return 1;
    }
    if !(0x65..=0x67).contains(&button) {
        return 0;
    }
    let kind = m.i32(win + 0x64);
    if (2..=3).contains(&kind) {
        o.si_dov_crop(m, win, rx0, rx1, ry0, ry1, button);
        let flag = (m.i16(cam + 0x44) == 0) as i32;
        o.si_dov_set_cam_ortho(m, win, flag, button);
        return 1;
    }
    let sp = m.sp();
    // two corners of three floats each, zeroed: c1 below c0 as on the original's stack
    let c1 = m.local(24);
    let c0 = c1 + 12;
    for i in 0..6 {
        m.set_f32(c1 + 4 * i, 0.0);
    }
    let (p0, p1);
    if button == 0x65 {
        // the ratios of the old rectangle to the drawn one, whole numbers kept to 16 bits
        let q = F80::from_i32(vx1.wrapping_sub(vx0)) / F80::from_i32(rx1.wrapping_sub(rx0));
        let s1 = q.to_i32(si_x87::Round::Zero) as i16 as i32;
        let q = F80::from_i32(vy1.wrapping_sub(vy0)) / F80::from_i32(ry1.wrapping_sub(ry0));
        let s2 = q.to_i32(si_x87::Round::Zero) as i16 as i32;
        let x0 = vx0.wrapping_sub(rx0).wrapping_mul(s1).wrapping_add(vx0) as i16 as i32;
        let x1 = vx1.wrapping_add(vx1.wrapping_sub(rx1).wrapping_mul(s1)) as i16 as i32;
        let y0 = vy0.wrapping_sub(ry0).wrapping_mul(s2).wrapping_add(vy0) as i16 as i32;
        let y1 = vy1.wrapping_add(vy1.wrapping_sub(ry1).wrapping_mul(s2)) as i16 as i32;
        p0 = (x0, y0);
        p1 = (x1, y1);
    } else {
        p0 = (rx0, ry0);
        p1 = (rx1, ry1);
    }
    if o.si_screen_to_world(m, win, p0.0, p0.1, c0, 0, 0) != 0 || o.si_screen_to_world(m, win, p1.0, p1.1, c1, 0, 0) != 0 {
        m.set_sp(sp);
        return 0;
    }
    if button == 0x65 {
        for i in 0..3 {
            let v = clamp_coordinate(m.f32(c0 + 4 * i), false);
            m.set_f32(c0 + 4 * i, v);
        }
        for i in 0..3 {
            let v = clamp_coordinate(m.f32(c1 + 4 * i), true);
            m.set_f32(c1 + 4 * i, v);
        }
        o.si_set_cam_ortho_view(m, cam, c0, c1, 0, 0);
    } else if m.i16(m.u32(win + 0x2c) + 4) == 0x7d4 {
        o.si_set_cam_view(m, cam, c0, c1, 1);
    } else {
        o.si_set_cam_ortho_view(m, cam, c0, c1, 0, 0);
    }
    m.set_sp(sp);
    1
}
