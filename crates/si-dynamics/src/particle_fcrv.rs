// SPDX-License-Identifier: MIT
//! `particle_fcrv.c` of Softimage|3D 4.0 (0x084c3fe0 … 0x084c457f): the function-curve edits of the particle
//! animation (finding 0045). The animation-editor callees (`edtools.c`, `edanim.c`, `fkey_fct.c`, `fcurve.c`) are
//! reached through [`ParticleCalls`].
//!
//! The particle status record (our name): +0x4c the frames held after the animation, +0x50 a flag (non-zero: the
//! first-key behaviour is not set to 2), +0x60 a flag (the material's curves are edited too). A model's material list
//! is at +0x20c (the first pointer), its count a short at +0x206. A function curve: keys at +8 (0x24 bytes each,
//! the time a float first), key count +0xc, two shorts at +0x18 and +0x1a (the extrapolation modes).

use crate::particle_outside::ParticleCalls;
use si_mem::{Mem, Outside};
use si_x87::x;

const SI_FRAME_RATE: u32 = 0x0878_204c;

/// What the key's time (a float at its start) becomes before a curve's first key: `time - 1.0 / SI_frameRate`.
fn before(m: &Mem, key: u32) -> f32 {
    (x(m.f32(key)) - x(1.0f32) / x(m.f32(SI_FRAME_RATE))).to_f32()
}

/// What it becomes after the last key: `float(frames) / SI_frameRate + time`.
fn after(m: &Mem, status: u32, key: u32) -> f32 {
    (x(m.i32(status + 0x4c)) / x(m.f32(SI_FRAME_RATE)) + x(m.f32(key))).to_f32()
}

/// Adds a key one frame before the first on the hide curves of a particle model [soft 0x084c3fe0
/// si_particleAddPreAnimHideKeys]: on the shape's curves 0xc9 and 0xca (kind 0) a copy of the first key moved
/// back by `1 / SI_frameRate`; on the model's curves in the range 0..14999 (kind 1) and, when the status flag +0x60
/// is set and the model has a material, that material's curves (kind 0) the same, the first curve's moved time
/// being used for every following curve.
pub fn si_particle_add_pre_anim_hide_keys(m: &mut Mem, o: &mut dyn Outside, status: u32, model: u32) {
    let out = m.local(4);
    let elem = o.si_ed_get_elem_from_shape(m, model, out);
    if elem != 0 {
        for ch in [0xc9, 0xca] {
            let fcv = o.si_ed_find_fcv(m, elem, 0, ch);
            if fcv != 0 {
                let key = o.si_fkey_copy(m, m.u32(fcv + 8));
                let t = before(m, key);
                m.set_f32(key, t);
                o.si_fcurve_add_key(m, fcv, key);
            }
        }
    }
    let elem = o.si_ed_get_elem_from_model(m, model, out);
    if elem != 0 {
        range_pre(m, o, elem, 1);
    }
    if m.u32(status + 0x60) != 0 && m.i16(model + 0x206) > 0 {
        let first_material = m.u32(m.u32(model + 0x20c));
        let elem = o.si_ed_get_elem_from_material(m, first_material, out);
        if elem != 0 {
            range_pre(m, o, elem, 0);
        }
    }
    m.release(4);
}

fn range_pre(m: &mut Mem, o: &mut dyn Outside, elem: u32, kind: i32) {
    let mut first = true;
    let mut t = 0.0f32;
    let mut fcv = o.si_ed_find_range_fcv(m, elem, kind, 0, 14999);
    while fcv != 0 {
        let key = o.si_fkey_copy(m, m.u32(fcv + 8));
        if first {
            t = before(m, key);
        }
        first = false;
        m.set_f32(key, t);
        o.si_fcurve_add_key(m, fcv, key);
        fcv = o.si_ed_find_range_next_fcv(m, elem, kind, 0, 14999);
    }
}

/// Adds a key `status[+0x4c]` frames after the last on the same curves [soft 0x084c41c0 si_particleAddPostAnimHideKeys]:
/// the shape's 0xca then 0xc9 curves get a copy of their last key moved by `float(frames) / SI_frameRate`. On the
/// model's range (and the material's) curves the first curve's key count and moved time are used for every curve
/// (the key copied is the one at `count - 1` of the first curve's count), and on the model's range one extra copy of
/// each curve's first key is made and dropped, as the original does.
pub fn si_particle_add_post_anim_hide_keys(m: &mut Mem, o: &mut dyn Outside, status: u32, model: u32) {
    let out = m.local(4);
    let elem = o.si_ed_get_elem_from_shape(m, model, out);
    if elem != 0 {
        for ch in [0xca, 0xc9] {
            let fcv = o.si_ed_find_fcv(m, elem, 0, ch);
            if fcv != 0 {
                let n = m.u32(fcv + 0xc);
                let last = m.u32(fcv + 8).wrapping_add(n.wrapping_mul(0x24)).wrapping_sub(0x24);
                let key = o.si_fkey_copy(m, last);
                let t = after(m, status, key);
                m.set_f32(key, t);
                o.si_fcurve_add_key(m, fcv, key);
            }
        }
    }
    let elem = o.si_ed_get_elem_from_model(m, model, out);
    if elem != 0 {
        range_post(m, o, status, elem, 1, true);
    }
    if m.u32(status + 0x60) != 0 && m.i16(model + 0x206) > 0 {
        let first_material = m.u32(m.u32(model + 0x20c));
        let elem = o.si_ed_get_elem_from_material(m, first_material, out);
        if elem != 0 {
            range_post(m, o, status, elem, 0, false);
        }
    }
    m.release(4);
}

fn range_post(m: &mut Mem, o: &mut dyn Outside, status: u32, elem: u32, kind: i32, extra_copy: bool) {
    let mut first = true;
    let mut n = 0u32;
    let mut t = 0.0f32;
    let mut fcv = o.si_ed_find_range_fcv(m, elem, kind, 0, 14999);
    while fcv != 0 {
        if extra_copy {
            // the copy of the first key is not used (and not freed)
            o.si_fkey_copy(m, m.u32(fcv + 8));
        }
        if first {
            n = m.u32(fcv + 0xc);
        }
        let last = m.u32(fcv + 8).wrapping_add(n.wrapping_mul(0x24)).wrapping_sub(0x24);
        let key = o.si_fkey_copy(m, last);
        if first {
            t = after(m, status, key);
        }
        first = false;
        m.set_f32(key, t);
        o.si_fcurve_add_key(m, fcv, key);
        fcv = o.si_ed_find_range_next_fcv(m, elem, kind, 0, 14999);
    }
}

/// Sets the extrapolation of the particle curves to cycle [soft 0x084c4404 si_particleMakePeriodic]: on the shape's
/// curves 0xc9 and 0xca, the model's curve 10 (only when `status[+0x4c] > 0`), the model's range curves (kind 1) and,
/// with the flag +0x60 and a material, the material's range curves (kind 0), the short at +0x1a becomes 2, and the
/// short at +0x18 too when `status[+0x50]` is 0.
pub fn si_particle_make_periodic(m: &mut Mem, o: &mut dyn Outside, status: u32, model: u32) {
    let set = |m: &mut Mem, fcv: u32| {
        m.set_i16(fcv + 0x1a, 2);
        if m.u32(status + 0x50) == 0 {
            m.set_i16(fcv + 0x18, 2);
        }
    };
    let out = m.local(4);
    let elem = o.si_ed_get_elem_from_shape(m, model, out);
    if elem != 0 {
        for ch in [0xc9, 0xca] {
            let fcv = o.si_ed_find_fcv(m, elem, 0, ch);
            if fcv != 0 {
                set(m, fcv);
            }
        }
    }
    let elem = o.si_ed_get_elem_from_model(m, model, out);
    if m.i32(status + 0x4c) > 0 && elem != 0 {
        let fcv = o.si_ed_find_fcv(m, elem, 0, 10);
        if fcv != 0 {
            set(m, fcv);
        }
    }
    let elem = o.si_ed_get_elem_from_model(m, model, out);
    if elem != 0 {
        let mut fcv = o.si_ed_find_range_fcv(m, elem, 1, 0, 14999);
        while fcv != 0 {
            set(m, fcv);
            fcv = o.si_ed_find_range_next_fcv(m, elem, 1, 0, 14999);
        }
    }
    if m.u32(status + 0x60) != 0 && m.i16(model + 0x206) > 0 {
        let first_material = m.u32(m.u32(model + 0x20c));
        let elem = o.si_ed_get_elem_from_material(m, first_material, out);
        if elem != 0 {
            let mut fcv = o.si_ed_find_range_fcv(m, elem, 0, 0, 14999);
            while fcv != 0 {
                set(m, fcv);
                fcv = o.si_ed_find_range_next_fcv(m, elem, 0, 0, 14999);
            }
        }
    }
    m.release(4);
}
