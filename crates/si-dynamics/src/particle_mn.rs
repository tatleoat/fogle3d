// SPDX-License-Identifier: MIT
//! `particle_mn.c` of Softimage|3D 4.0 (0x084c05d0 …): the particle menu item and the key writer for the hide curve
//! (finding 0045). The dialog the item opens (`si_dialogParticle`) is user interface and stays a hook.

use crate::particle_fcrv::{si_particle_add_post_anim_hide_keys, si_particle_add_pre_anim_hide_keys, si_particle_make_periodic};
use crate::outside::DynaCalls;
use crate::particle_outside::ParticleCalls;
use si_mem::{Mem, Outside};
use si_x87::{x, F80};

const SI_FRAME_CURRENT: u32 = 0x0878_2040;
const SI_FRAME_RATE: u32 = 0x0878_204c;
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_GRAF_LISTS: u32 = 0x0867_9498;
const STDERR: u32 = 0x0878_0c88;
const STATUS: u32 = 0x0882_2040;
/// The words of the status record the item reads outside the record's own fields: the three nodes it was given
/// (`DAT_088220f4`, `f8`, `fc`).
const NODE_A: u32 = 0x0882_20f4;
const NODE_B: u32 = 0x0882_20f8;
const NODE_C: u32 = 0x0882_20fc;

fn key(m: &mut Mem, o: &mut dyn Outside, model: u32, value: f32) {
    o.si_savekey_model(m, model, 0, 10, 0xaf, 0, value, 0, 0);
}

fn bump(m: &mut Mem, v: i32) {
    m.set_i32(SI_FRAME_CURRENT, m.i32(SI_FRAME_CURRENT).wrapping_add(v));
}

/// Keys the hide curve (id 0xaf) of a particle model for repetition `rep` [soft 0x084c0ac4 particleHandleHideKeys].
/// With the pre-animation flag (+0x50): at frame `rep * (+0x44) + (+0x38) - 1` a key 0, the pre-animation keys, and at the
/// next frame a key 1.0. With a positive life length (+0x4c): without the pre flag a key 1.0 at `rep * (+0x44) + (+0x38)`;
/// a key 1.0 at `rep * (+0x44) + (+0x3c)`, a key 0 one frame later and a key 0 at `last + length - 1`, then the
/// post-animation keys. Without the cycle flag (+0x48) the curve is made periodic. `SI_frameCurrent` is left at the last
/// frame keyed.
pub fn particle_handle_hide_keys(m: &mut Mem, o: &mut dyn Outside, status: u32, model: u32, rep: i32) {
    let base = rep.wrapping_mul(m.i32(status + 0x44));
    if m.i32(status + 0x50) != 0 {
        m.set_i32(SI_FRAME_CURRENT, base.wrapping_add(m.i32(status + 0x38)).wrapping_sub(1));
        key(m, o, model, 0.0);
        si_particle_add_pre_anim_hide_keys(m, o, status, model);
        bump(m, 1);
        key(m, o, model, 1.0);
    }
    if m.i32(status + 0x4c) > 0 {
        if m.i32(status + 0x50) == 0 {
            m.set_i32(SI_FRAME_CURRENT, base.wrapping_add(m.i32(status + 0x38)));
            key(m, o, model, 1.0);
        }
        m.set_i32(SI_FRAME_CURRENT, base.wrapping_add(m.i32(status + 0x3c)));
        key(m, o, model, 1.0);
        bump(m, 1);
        key(m, o, model, 0.0);
        bump(m, m.i32(status + 0x4c).wrapping_sub(1));
        key(m, o, model, 0.0);
        si_particle_add_post_anim_hide_keys(m, o, status, model);
    }
    if m.i32(status + 0x48) == 0 {
        si_particle_make_periodic(m, o, status, model);
    }
}

/// The graph node list of the current process (`SI_grafLists` + 0x24 times the process index).
fn graf_list(m: &Mem) -> u32 {
    m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA) + 4).wrapping_mul(0x24))
}

/// The particle menu item [soft 0x084c05d0 SI_particleAnimation]. The dialog (`si_dialogParticle`, status record at
/// 0x08822040) is a hook; below 0 it ends the item, then `si_particleGetMeshes` below 0, a repetition count (+0x40) or a
/// particle count (+0x24) not above 0 end it too. Then: "Creating particle animation..." (level 0); with the
/// flag +0x28 the count is taken from the element behind the first node of the status record (+0xb4: the word at +0x14
/// of its +0x48) and the flag +0x2c is cleared when the second node's (+0xb8) differs; with +0x28 clear and +0x2c set it is
/// taken from the second node. "Computing flock animation..." and the mouse help "Cancel". One repetition: the animation
/// model from `si_particleCreateAnimation(status, 1)` (none ends the item). Several: a model "null" (`SI_modelAllocate`)
/// added to the graph list as kind 2, and for each repetition the message "Computing repetition # n..." and the animation
/// model, made the son of the "null" node; from the second one on a pressed button 0x65 sets the halted flag, saves the
/// repetition count and shortens it to the repetitions done. A null model prints "NULL parent returned!\n" on stderr, else the sons' state is
/// updated, the selection is put back and the hide keys are written (flag +0x34 clear: one model for one repetition, or the
/// sons of the "null" one by one, repetition = index; set: for each of the first +0x24 sons, or of each repetition model's first
/// +0x24 sons), `SI_frameCurrent` is restored, the time is the frame over the rate, a metaball (type 8 of the third node) with a parent
/// system makes the animation model the son of that system, the models are updated to the time, the views and the counter
/// are redrawn and "Done." (level 1) is shown. A halted run gives the saved count back and says "Computation halted!" (level 2).
pub fn si_particle_animation(m: &mut Mem, o: &mut dyn Outside) {
    if o.si_dialog_particle(m, STATUS) < 0 {
        return;
    }
    if o.si_particle_get_meshes(m, STATUS) < 0 {
        return;
    }
    if m.i32(STATUS + 0x40) <= 0 || m.i32(STATUS + 0x24) <= 0 {
        return;
    }
    o.si_status_display_message(m, 0x086d_7a60, 0);
    if m.i32(STATUS + 0x28) == 0 {
        if m.i32(STATUS + 0x2c) != 0 {
            let n = m.i32(m.u32(m.u32(NODE_B) + 0x48) + 0x14);
            m.set_i32(STATUS + 0x24, n);
        }
    } else {
        let n = m.i32(m.u32(m.u32(NODE_A) + 0x48) + 0x14);
        m.set_i32(STATUS + 0x24, n);
        if m.i32(STATUS + 0x2c) != 0 && n != m.i32(m.u32(m.u32(NODE_B) + 0x48) + 0x14) {
            m.set_i32(STATUS + 0x2c, 0);
        }
    }
    o.si_status_display_message(m, 0x086d_7a80, 1);
    o.si_status_display_mouse(m, 0x086d_7aa5, 0x086d_7aa4, 0x086d_7a9d);
    // the item's frame: the buffer of the repetition message is at ebp-0x50, the frame pointer 4 bytes under the stack top
    let frame_top = m.local(4);
    let mut halted = false;
    // the original keeps the saved count in a stack slot that is read only when `halted` is set
    let mut saved: i32 = 0;
    let model;
    if m.i32(STATUS + 0x40) == 1 {
        model = o.si_particle_create_animation(m, STATUS, 1);
        if model == 0 {
            m.release(4);
            return;
        }
    } else {
        model = o.si_model_allocate(m, 0x086d_7aa6);
        let list = graf_list(m);
        let node = o.si_graflist_add_data(m, list, model, 2, 1);
        let mut done: i32 = 0;
        if 0 < m.i32(STATUS + 0x40) {
            loop {
                let rep = done + 1;
                let text = m.local(80);
                let mut line = format!("Computing repetition # {}...", rep).into_bytes();
                line.push(0);
                m.set_bytes(text, &line);
                o.si_status_display_message(m, text, 1);
                m.release(80);
                let made = o.si_particle_create_animation(m, STATUS, rep);
                if made == 0 {
                    m.release(4);
                    return;
                }
                let list = graf_list(m);
                let son = o.si_find_gl_data(m, list, made, 2);
                o.si_model_create_parent(m, node, son, 1);
                if 0 < done && o.getbutton(m, 0x65) != 0 {
                    halted = true;
                    saved = m.i32(STATUS + 0x40);
                    m.set_i32(STATUS + 0x40, rep);
                }
                done = rep;
                if done >= m.i32(STATUS + 0x40) {
                    break;
                }
            }
        }
    }
    if model == 0 {
        let stream = m.u32(STDERR);
        let text = m.cstr(0x086d_7ace);
        o.print(stream, &text);
    } else {
        o.si_model_update_son_state(m, 0, model, 1);
        o.si_back_selected(m, 0, 1);
        let frame = m.i32(SI_FRAME_CURRENT);
        let reps = m.i32(STATUS + 0x40);
        let by_index = m.i32(STATUS + 0x34) == 0;
        if reps == 1 && by_index {
            o.particle_handle_hide_keys(m, STATUS, model, 0);
        }
        if 1 < reps && by_index {
            let mut son = m.u32(model + 0x2c4);
            let mut i = 0;
            while i < m.i32(STATUS + 0x40) {
                o.particle_handle_hide_keys(m, STATUS, son, i);
                son = m.u32(son + 0x2c0);
                i += 1;
            }
        }
        if m.i32(STATUS + 0x40) == 1 && m.i32(STATUS + 0x34) == 1 {
            let mut son = m.u32(model + 0x2c4);
            let mut i = 0;
            while son != 0 && i < m.i32(STATUS + 0x24) {
                o.particle_handle_hide_keys(m, STATUS, son, 0);
                son = m.u32(son + 0x2c0);
                i += 1;
            }
        }
        if 1 < m.i32(STATUS + 0x40) && m.i32(STATUS + 0x34) == 1 {
            let mut rep_model = m.u32(model + 0x2c4);
            let mut r = 0;
            while r < m.i32(STATUS + 0x40) {
                let mut son = m.u32(rep_model + 0x2c4);
                let mut i = 0;
                while son != 0 && i < m.i32(STATUS + 0x24) {
                    o.particle_handle_hide_keys(m, STATUS, son, r);
                    son = m.u32(son + 0x2c0);
                    i += 1;
                }
                rep_model = m.u32(rep_model + 0x2c0);
                r += 1;
            }
        }
        m.set_i32(SI_FRAME_CURRENT, frame);
        // SI_frameToSec (0x08201554): the frame over the frame rate, rounded to f32 as it is stored
        let time = (F80::from_i32(frame) / x(m.f32(SI_FRAME_RATE))).to_f32();
        let node_c = m.u32(NODE_C);
        if m.i32(node_c + 0x30) == 8 {
            let system = o.si_metaball_get_parent_system(m, m.u32(node_c + 0x48));
            if system != 0 {
                let list = graf_list(m);
                let child = o.si_find_gl_data(m, list, model, 2);
                let list = graf_list(m);
                let parent = o.si_find_gl_data(m, list, system, 2);
                o.si_model_create_parent(m, parent, child, 1);
            }
        }
        let list = graf_list(m);
        let node = o.si_find_gl_data(m, list, model, 2);
        o.si_model_update(m, model, time, node);
        let mut son = m.u32(model + 0x2c4);
        while son != 0 {
            let list = graf_list(m);
            let node = o.si_find_gl_data(m, list, son, 2);
            o.si_model_update(m, son, time, node);
            son = m.u32(son + 0x2c0);
        }
        o.si_refresh_system(m, 0, 0, 0x27);
        o.si_refresh_counter(m);
        o.si_status_display_message(m, 0x086d_7ac8, 1);
    }
    m.release(4);
    let _ = frame_top;
    if halted {
        m.set_i32(STATUS + 0x40, saved);
        o.si_status_display_message(m, 0x086d_7ae5, 2);
    }
}
