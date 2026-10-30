// SPDX-License-Identifier: MIT
//! `wave_mn.c` of Softimage|3D 4.0 (0x084c67a0 …): the wave menu items that need no dialog (finding 0045). The items that
//! open the wave dialog (`si_dialogWave`, a hook) or wait for picks are there too. The current
//! selection is the item of the graph node at +8 of the current graph list.

use crate::particle_outside::ParticleCalls;
use crate::wave_fcrv::{si_wave_init_decay, si_wave_init_spread};
use crate::wave_init::{si_wave_allocate, si_wave_amp_create, si_wave_init_horiz_prof};
use crate::wave_sys::si_wave_dispose;
use crate::wave_util::{si_wave_detach_mdl, si_wave_find_from_selection, si_wave_freeze_wave, si_wave_get_mdl, si_wave_get_wave, si_wave_resize_icon};
use si_mem::{Mem, Outside};
use si_x87::x;

const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_GRAF_LISTS: u32 = 0x0867_9498;

/// Freezes the selected wave [soft 0x084c6cc4 SI_waveFreeze]: no selection gives "No currently selected wave." (level 3),
/// a branch wave (+0x70 is 1) a message (level 3); else the wave is frozen, everything is redrawn and "Wave's effect
/// frozen." is shown.
pub fn si_wave_freeze(m: &mut Mem, o: &mut dyn Outside) {
    let w = si_wave_find_from_selection(m);
    if w == 0 {
        o.si_status_display_message(m, 0x086d_9558, 3);
    } else if m.i32(w + 0x70) == 1 {
        o.si_status_display_message(m, 0x086d_9580, 3);
    } else {
        si_wave_freeze_wave(m, o, w);
        o.si_refresh_all(m, 1);
        o.si_status_display_message(m, 0x086d_95b2, 0);
    }
}

/// Detaches the selected wave from its model [soft 0x084c6e88 SI_waveDetachItem]: messages at level 3 for no selection or
/// no model; else `SI_waveDetachMdl(wave, model, 1)`, a redraw and "Wave detached.".
pub fn si_wave_detach_item(m: &mut Mem, o: &mut dyn Outside) {
    let w = si_wave_find_from_selection(m);
    if w == 0 {
        o.si_status_display_message(m, 0x086d_9698, 3);
    } else if m.u32(w + 0x6c) == 0 {
        o.si_status_display_message(m, 0x086d_96c0, 3);
    } else {
        let model = m.u32(w + 0x6c);
        si_wave_detach_mdl(m, o, w, model, 1);
        o.si_refresh_all(m, 1);
        o.si_status_display_message(m, 0x086d_96e3, 0);
    }
}

/// Gives a wave the profile of the selected spline [soft 0x084c6d2c SI_waveChangeProfile] (the original reads a register
/// the caller left when the selected node is not a model node of kind 2 or 3; here that case gives the message "No
/// currently selected spline." like a model that is not a NURBS curve). The selection must be a spline model (kind 6): none
/// gives "No currently selected spline.", a NURBS curve (kind 9) "You cannot create a wave with a NURBS curve." and
/// any other "No currently selected spline." (all level 3). Then "Select Wave." (level 0) and the pick; none gives "No
/// wave selected.". The reciprocals of the wave's two ranges (+0x3c - +0x38, +0x44 - +0x40) are kept, the amplitude graph at
/// +0x2c is rebuilt from the spline (failure: "Spline is not a valid graph."), the icon is resized by the new range times
/// the old reciprocal (z = x), an attached model has its definition readjusted, everything is redrawn and "Profile
/// changed." is shown.
pub fn si_wave_change_profile(m: &mut Mem, o: &mut dyn Outside) {
    let list = m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA) + 4).wrapping_mul(0x24));
    let node = m.u32(list + 8);
    if node == 0 {
        o.si_status_display_message(m, 0x086d_95c8, 3);
        return;
    }
    let model = if m.u32(node).wrapping_sub(2) < 2 { m.u32(node + 4) } else { 0 };
    if model == 0 || m.i32(model + 0x30) != 6 {
        if model != 0 && m.i32(model + 0x30) == 9 {
            o.si_status_display_message(m, 0x086d_9600, 3);
        } else {
            o.si_status_display_message(m, 0x086d_962d, 3);
        }
        return;
    }
    o.si_status_display_message(m, 0x086d_964b, 0);
    let w = si_wave_get_wave(m, o);
    if w == 0 {
        o.si_status_display_message(m, 0x086d_9658, 3);
        return;
    }
    let loc = m.local(12);
    let rx = (x(1.0f32) / (x(m.f32(w + 0x3c)) - x(m.f32(w + 0x38)))).to_f32();
    let ry = (x(1.0f32) / (x(m.f32(w + 0x44)) - x(m.f32(w + 0x40)))).to_f32();
    m.set_f32(loc, rx);
    m.set_f32(loc + 4, ry);
    if si_wave_amp_create(m, o, w + 0x2c, model) < 1 {
        m.release(12);
        o.si_status_display_message(m, 0x086d_966a, 3);
        return;
    }
    let nx = ((x(m.f32(w + 0x3c)) - x(m.f32(w + 0x38))) * x(m.f32(loc))).to_f32();
    m.set_f32(loc, nx);
    m.set_f32(loc + 8, nx);
    let ny = ((x(m.f32(w + 0x44)) - x(m.f32(w + 0x40))) * x(m.f32(loc + 4))).to_f32();
    m.set_f32(loc + 4, ny);
    si_wave_resize_icon(m, w, loc);
    m.release(12);
    let attached = m.u32(w + 0x6c);
    if attached != 0 {
        o.si_model_readjust_definition(m, attached);
    }
    o.si_refresh_all(m, 1);
    o.si_status_display_message(m, 0x086d_9687, 0);
}

const WAVE_STATUS: u32 = 0x0882_2120;
const SI_FRAME_RATE: u32 = 0x0878_204c;
const SI_RD: u32 = 0x0878_15f0;
const SI_ELEM_DICTS: u32 = 0x0867_931c;

/// Makes a wave from the selected spline [soft 0x084c67a0 SI_waveCreateItem]: the selection must be a spline model (kind
/// 6) as for the profile change (messages at level 3: none or other "No currently selected spline.", a NURBS curve "You
/// cannot create a wave with a NURBS curve."); its amplitude graph is built into a zeroed 0x20-byte record ("Spline is
/// not a valid graph.") and a wave is allocated ("Unable to allocate item."), the record is copied to +0x2c, the wave dialog
/// is run (flag 1), the horizontal profile and the icon model are made ("Cannot create model for wave.": the wave is
/// left allocated) and the icon is resized by (w, h, w) of the extent. A dialog result below 1 disposes the wave; else it
/// is put in the current graph list (kind 0xb, after `SI_backSelected(0, 1)`), its decay and spread are set and systems
/// 0x25 and 5 and the counter are refreshed.
pub fn si_wave_create_item(m: &mut Mem, o: &mut dyn Outside) {
    let list = m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA) + 4).wrapping_mul(0x24));
    let node = m.u32(list + 8);
    if node == 0 {
        o.si_status_display_message(m, 0x086d_9320, 3);
        return;
    }
    let mut model = 0u32;
    let mut bad = true;
    if m.u32(node).wrapping_sub(2) < 2 {
        model = m.u32(node + 4);
        if m.i32(model + 0x30) == 6 {
            bad = false;
        }
    }
    if bad {
        if model == 0 || m.i32(model + 0x30) != 9 {
            o.si_status_display_message(m, 0x086d_936d, 3);
        } else {
            o.si_status_display_message(m, 0x086d_9340, 3);
        }
        return;
    }
    // the frame: the wave slot (a local the dispose takes by address), the graph record and the resize vector
    let loc = m.local(0x30);
    let (slot, graph, v) = (loc, loc + 4, loc + 0x24);
    for k in 0..8 {
        m.set_u32(graph + 4 * k, 0);
    }
    if (si_wave_amp_create(m, o, graph, model) as i32) < 1 {
        o.si_status_display_message(m, 0x086d_938b, 3);
        m.release(0x30);
        return;
    }
    let wave = si_wave_allocate(m, o, WAVE_STATUS);
    m.set_u32(slot, wave);
    if wave == 0 {
        o.si_status_display_message(m, 0x086d_93a8, 3);
        m.release(0x30);
        return;
    }
    for k in 0..8 {
        let w = m.u32(graph + 4 * k);
        m.set_u32(wave + 0x2c + 4 * k, w);
    }
    let accepted = o.si_dialog_wave(m, wave, 1);
    si_wave_init_horiz_prof(m, wave);
    if o.si_wave_create_icon_mdl(m, wave) == 0 {
        o.si_status_display_message(m, 0x086d_93c1, 3);
        m.release(0x30);
        return;
    }
    let w = (x(m.f32(wave + 0x3c)) - x(m.f32(wave + 0x38))).to_f32();
    let h = (x(m.f32(wave + 0x44)) - x(m.f32(wave + 0x40))).to_f32();
    m.set_f32(v, w);
    m.set_f32(v + 4, h);
    m.set_f32(v + 8, w);
    si_wave_resize_icon(m, wave, v);
    if accepted < 1 {
        si_wave_dispose(m, o, slot);
    } else {
        o.si_back_selected(m, 0, 1);
        let list = m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA) + 4).wrapping_mul(0x24));
        o.si_graflist_add_data(m, list, wave, 0xb, 1);
        si_wave_init_decay(m, o, wave);
        si_wave_init_spread(m, o, wave);
        o.si_refresh_system(m, 0, 0, 0x25);
        o.si_refresh_system(m, 0, 0, 5);
        o.si_refresh_counter(m);
    }
    m.release(0x30);
}

/// Edits the selected wave [soft 0x084c69a8 SI_waveEditItem]: none gives "No currently selected wave." (level 3). The
/// periodic flag (+0x28) and the frame range (+0x10, +0x14) are kept, the dialog is run (flag 0; a result below 1 ends)
/// and the wave's animation element is found (none ends); of its two curves (channels 0x266 and 0x267) the +0x1a word
/// becomes 2 when the periodic flag went from 0 to non-0 and 0 when it went the other way; a changed frame range scales
/// both curves by `new length / old length` about `old start / SI_frameRate` and offsets them by `(new start - old
/// start) / SI_frameRate` (the ratio and times in 80 bits, rounded to f32 for each call). An attached model has its
/// definition readjusted and everything is redrawn.
pub fn si_wave_edit_item(m: &mut Mem, o: &mut dyn Outside) {
    let wave = si_wave_find_from_selection(m);
    if wave == 0 {
        o.si_status_display_message(m, 0x086d_93e0, 3);
        return;
    }
    let old_periodic = m.i32(wave + 0x28);
    let old_start = m.i32(wave + 0x10);
    let old_end = m.i32(wave + 0x14);
    if o.si_dialog_wave(m, wave, 0) < 1 {
        return;
    }
    let out = m.local(8);
    let elem = o.si_ed_get_elem_from_wave(m, wave, out);
    m.release(8);
    if elem == 0 {
        return;
    }
    let f1 = o.si_ed_find_fcv(m, elem, 0, 0x266);
    let f2 = o.si_ed_find_fcv(m, elem, 0, 0x267);
    let periodic = m.i32(wave + 0x28);
    if periodic != 0 {
        if old_periodic == 0 {
            m.set_u16(f1 + 0x1a, 2);
            m.set_u16(f2 + 0x1a, 2);
        }
    } else if old_periodic != 0 {
        m.set_u16(f1 + 0x1a, 0);
        m.set_u16(f2 + 0x1a, 0);
    }
    let (start, end) = (m.i32(wave + 0x10), m.i32(wave + 0x14));
    if start != old_start || end != old_end {
        let ratio = (x(end.wrapping_sub(start)) / x(old_end.wrapping_sub(old_start))).to_f32();
        let origin = (x(old_start) / x(m.f32(SI_FRAME_RATE))).to_f32();
        o.si_fcurve_scale_origin(m, f1, ratio, origin);
        o.si_fcurve_scale_origin(m, f2, ratio, origin);
        let off = (x(start.wrapping_sub(old_start)) / x(m.f32(SI_FRAME_RATE))).to_f32();
        o.si_fcurve_offset(m, f1, off);
        o.si_fcurve_offset(m, f2, off);
    }
    let attached = m.u32(wave + 0x6c);
    if attached != 0 {
        o.si_model_readjust_definition(m, attached);
        o.si_refresh_all(m, 1);
    }
}

/// Attaches the selected wave to a picked model [soft 0x084c6b34 SI_waveAttachItem] (`branch`: the short argument, 0 for a
/// node object). No selection gives "No currently selected wave." and a wave that already has a model "Sorry, only one
/// object per wave allowed." (level 3). Else "Pick Branch Object." / "Pick Node Object." is shown and the pick awaited
/// ("No Object Chosen." at level 3 for none); a metaclay (kinds 7 and 8) gives a message at level 2, a 1.5 s sleep and
/// an empty message; a face model (kind 2) the message "Wave has no effect on face." at level 2 and, for a node object,
/// a 1.5 s sleep and "Wave not attached.". Else the wave is attached and the model dictionary of the current scene
/// checked for a cycle: none gives "Wave attached." and a redraw; a cycle a request dialog and a detach.
pub fn si_wave_attach_item(m: &mut Mem, o: &mut dyn Outside, branch: i16) {
    let wave = si_wave_find_from_selection(m);
    if wave == 0 {
        o.si_status_display_message(m, 0x086d_9400, 3);
        return;
    }
    if m.u32(wave + 0x6c) != 0 {
        o.si_status_display_message(m, 0x086d_9420, 3);
        return;
    }
    o.si_status_display_message(m, if branch == 0 { 0x086d_945d } else { 0x086d_9449 }, 0);
    let model = si_wave_get_mdl(m, o);
    if model == 0 {
        o.si_status_display_message(m, 0x086d_946f, 3);
        return;
    }
    let kind = m.u32(model + 0x30);
    if kind.wrapping_sub(7) < 2 {
        o.si_status_display_message(m, 0x086d_94a0, 2);
        o.usleep(m, 1_500_000);
        o.si_status_display_message(m, 0x086d_94c5, 0);
        return;
    }
    if kind == 2 {
        o.si_status_display_message(m, 0x086d_94c6, 2);
        if branch == 0 {
            o.usleep(m, 1_500_000);
            o.si_status_display_message(m, 0x086d_94e2, 0);
            return;
        }
    }
    o.si_wave_add_mdl(m, wave, model, branch as i32, 0);
    let idx = m.u32(m.u32(SI_PROC_DATA));
    let dict = m.u32(SI_ELEM_DICTS).wrapping_add(idx.wrapping_mul(7).wrapping_mul(0x10));
    if o.si_elemdict_check_cycle(m, SI_RD, dict) == 0 {
        o.si_status_display_message(m, 0x086d_9549, 0);
        o.si_refresh_all(m, 1);
    } else {
        o.si_dialog_request(m, 0x086d_9520, 0x086d_94f9, 0x086d_94f8, 0x086d_94f5);
        si_wave_detach_mdl(m, o, wave, model, 1);
    }
}
