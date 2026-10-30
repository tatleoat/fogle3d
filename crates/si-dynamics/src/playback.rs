// SPDX-License-Identifier: MIT
//! `dyn_playback.c`: the simulation's keys on the function curves of a model (what the simulation recorded, taken back for
//! playback, and the list of transitions). The curves and the scene are reached through [`DynaCalls`].

use crate::outside::DynaCalls;
use crate::settings_rec::{si_dyna_algo_type_set, si_dynajntvar_set_default};
use si_mem::{Mem, Outside};
use si_x87::{fmov, x, F80};

const SI_ATD: u32 = 0x0878_1438;
const SI_CID_MODELS: u32 = 0x0878_1680;
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_ELEM_DICTS: u32 = 0x0867_931c;
const SI_FRAME_START: u32 = 0x0878_2050;
const SI_FRAME_CURRENT: u32 = 0x0878_2040;

/// `SI_frameToSec` (0x08201554, a frame over the frame rate `0x0878204c` in the register); it only reads a global, so it runs here.
fn frame_to_sec(m: &Mem, frame: i32) -> F80 {
    F80::from_i32(frame) / x(m.f32(0x0878_204c))
}

/// The element dictionary of the current process (`SI_elemDicts` + 0x70 times the process index).
fn elem_dict(m: &Mem) -> u32 {
    let pd = m.u32(SI_PROC_DATA);
    m.u32(pd).wrapping_mul(0x70).wrapping_add(m.u32(SI_ELEM_DICTS))
}

/// [soft 0x0849d3b0 SI_dynaSimulationCheckExistence]: sets the "has simulated keys" flag (`+0x12` of the simulation record
/// `+0x304`) of a model from whether any of the six curves (position of the model, rotation of its body or, for the
/// algorithm kinds 1 and 2 of a body with kind 0, of the first body of its chain) has raw keys.
pub fn si_dyna_simulation_check_existence(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let rec = m.u32(model + 0x304);
    if rec == 0 {
        return;
    }
    let mut other = model;
    if m.u32(model + 0x30) == 0 && m.u32(model + 0x2ec).wrapping_sub(1) < 2 {
        other = m.u32(model + 0x2c4);
    }
    let out = m.local(4);
    let e1 = o.si_ed_get_elem_from_model(m, model, out);
    let e2 = o.si_ed_get_elem_from_model(m, other, out);
    m.release(4);
    if e1 == 0 || e2 == 0 {
        return;
    }
    let c1 = o.si_ed_find_fcv(m, e1, 1, 0x71);
    let c2 = o.si_ed_find_next_fcv(m, e1, 1, 0x71);
    let c3 = o.si_ed_find_next_fcv(m, e1, 1, 0x71);
    let c4 = o.si_ed_find_fcv(m, e2, 1, 0x65);
    let c5 = o.si_ed_find_next_fcv(m, e2, 1, 0x65);
    let c6 = o.si_ed_find_next_fcv(m, e2, 1, 0x65);
    let rec = m.u32(model + 0x304);
    let mut any = 0u16;
    for c in [c1, c2, c3, c4, c5, c6] {
        if o.si_fcurve_check_for_raw_keys(m, c) as u16 != 0 {
            any = 1;
            break;
        }
    }
    m.set_u16(rec + 0x12, any);
}

/// The three curves of kind `kind`, their raw keys removed, the attribute flags (`first`, `+1`, `+2`) set to whether each is
/// now empty, and the element's animation reset.
fn remove_elem_curves(m: &mut Mem, o: &mut dyn Outside, elem: u32, loc: u32, kind: u32, first: u32) {
    let c1 = o.si_ed_find_fcv(m, elem, 1, kind as i32);
    let c2 = o.si_ed_find_next_fcv(m, elem, 1, kind as i32);
    let c3 = o.si_ed_find_next_fcv(m, elem, 1, kind as i32);
    o.si_fcurve_remove_raw_keys(m, c1);
    o.si_fcurve_remove_raw_keys(m, c2);
    o.si_fcurve_remove_raw_keys(m, c3);
    o.si_fcv_table_set(m, 0);
    for (i, c) in [c1, c2, c3].into_iter().enumerate() {
        let empty = if c != 0 { (m.i32(c + 0xc) < 1) as i32 } else { 0 };
        o.si_attdict_set_fcurve_flag(m, SI_ATD, first + i as u32, empty);
    }
    let d = elem_dict(m);
    let cid = m.u32(SI_CID_MODELS);
    o.si_elemdict_reset_elem_anim(m, d, elem, cid, loc, 1);
}

/// [soft 0x0849d514 simulationRemoveElemFrames]: takes the raw keys off the curves of the model's element, the position
/// curves (`pos`, kind 0x71, attributes 0x8b..) and the rotation curves (`rot`, kind 0x65, attributes 4..).
pub fn simulation_remove_elem_frames(m: &mut Mem, o: &mut dyn Outside, model: u32, pos: i16, rot: i16) {
    let loc = m.local(4);
    let elem = o.si_ed_get_elem_from_model(m, model, loc);
    if elem != 0 {
        let l = m.u32(loc);
        if pos != 0 {
            remove_elem_curves(m, o, elem, l, 0x71, 0x8b);
        }
        if rot != 0 {
            remove_elem_curves(m, o, elem, l, 0x65, 4);
        }
    }
    m.release(4);
}

/// [soft 0x0849d760 SI_dynaSimulationDispose]: drops what the simulation recorded on a model: with `flag` the scene goes
/// to the later of the first key and the start frame and the algorithm is reset; the raw keys of the chain are removed and
/// the kinematic and joint variables of every body set back to their defaults.
pub fn si_dyna_simulation_dispose(m: &mut Mem, o: &mut dyn Outside, model: u32, flag: i16) {
    let loc = m.local(4);
    let elem = o.si_ed_get_elem_from_model(m, model, loc);
    m.release(4);
    let rec = m.u32(model + 0x304);
    if elem == 0 || rec == 0 || m.u16(rec + 0x12) == 0 {
        return;
    }
    if flag != 0 {
        let fcv = o.si_ed_find_fcv(m, elem, 0, 0xb);
        if fcv != 0 {
            let keys = m.u32(fcv + 8);
            let start = m.u32(SI_FRAME_START) as i32;
            let a = frame_to_sec(m, start);
            let t = if a.lt(x(m.f32(keys))) { m.f32(m.u32(fcv + 8)) } else { frame_to_sec(m, start).to_f32() };
            let f = o.si_sec_to_frame(m, t);
            m.set_u32(SI_FRAME_CURRENT, f as u32);
            o.si_scene_update(m, t);
        }
        si_dyna_algo_type_set(m, model, 0);
    }
    if m.u32(model + 0x30) == 0 {
        simulation_remove_elem_frames(m, o, model, 1, 0);
        let mut p = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
        while p != 0 {
            simulation_remove_elem_frames(m, o, p, 0, 1);
            p = m.u32(p + 0x30c);
        }
    } else {
        simulation_remove_elem_frames(m, o, model, 1, 1);
    }
    let rec = m.u32(model + 0x304);
    m.set_u32(rec + 0x14, 0);
    let rec = m.u32(model + 0x304);
    m.set_u16(rec + 0x12, 0);
    let rec = m.u32(model + 0x304);
    m.set_u16(rec + 6, 0);
    let mut p = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
    while p != 0 {
        let k = m.u32(p + 0x2f0);
        si_kine::image::qstretch::kine_state_set_default(m, k);
        let v = m.u32(p + 0x300);
        si_dynajntvar_set_default(m, v);
        p = m.u32(p + 0x30c);
    }
}

/// [soft 0x0849d8f8 SI_dynaTransitionCrvDispose]: frees the list of transitions of a model (the simulation record's `+0x18`),
/// flags the transition curve's attribute (0xb2) as empty, resets the element's animation, and lets the dynamics of the
/// chain's root go before the algorithm is reset.
pub fn si_dyna_transition_crv_dispose(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let loc = m.local(4);
    let elem = o.si_ed_get_elem_from_model(m, model, loc);
    if elem != 0 && o.si_ed_find_fcv(m, elem, 0, 0xb) != 0 {
        loop {
            let rec = m.u32(model + 0x304);
            let head = m.u32(rec + 0x18);
            if head == 0 {
                break;
            }
            let next = m.u32(head + 8);
            m.set_u32(rec + 0x18, next);
            m.free(head);
        }
        o.si_fcv_table_set(m, 0);
        o.si_attdict_set_fcurve_flag(m, SI_ATD, 0xb2, 1);
        let d = elem_dict(m);
        let cid = m.u32(SI_CID_MODELS);
        let l = m.u32(loc);
        o.si_elemdict_reset_elem_anim(m, d, elem, cid, l, 1);
        let mut root = model;
        if m.u32(model + 0x30) == 0 {
            let mut k = m.u32(model + 0x2ec);
            if k.wrapping_sub(1) <= 1 {
                while k.wrapping_sub(3) > 1 {
                    loop {
                        root = m.u32(root + 0x2c4);
                        if m.u32(root + 0x30) == 0 {
                            break;
                        }
                    }
                    k = m.u32(root + 0x2ec);
                }
            }
        }
        o.si_ed_element_relax_dynamics(m, model, root);
        si_dyna_algo_type_set(m, model, 0);
    }
    m.release(4);
}

/// [soft 0x0849da08 SI_simseginfoRecordTransition]: sets the state (a short at `+4`) of the transition of the model's list whose
/// frame is `frame`.
pub fn si_simseginfo_record_transition(m: &mut Mem, model: u32, frame: i32, state: i16) {
    let rec = m.u32(model + 0x304);
    if rec == 0 {
        return;
    }
    let mut p = m.u32(rec + 0x18);
    while p != 0 {
        if m.i32(p) == frame {
            m.set_u16(p + 4, state as u16);
            return;
        }
        p = m.u32(p + 8);
    }
}

/// [soft 0x0849da48 SI_simseginfoAddTransition]: appends a transition (frame, state 1) to the model's list (`+0x18`, tail
/// `+0x1c`). The original links the new record even when the allocation failed.
pub fn si_simseginfo_add_transition(m: &mut Mem, model: u32, frame: i32) {
    let rec = m.u32(model + 0x304);
    if rec == 0 {
        return;
    }
    let n = m.si_malloc(0xc);
    if n != 0 {
        m.set_u16(n + 4, 1);
        m.set_u32(n, frame as u32);
        m.set_u32(n + 8, 0);
    }
    if m.u32(rec + 0x18) == 0 {
        m.set_u32(rec + 0x18, n);
    } else {
        let tail = m.u32(rec + 0x1c);
        m.set_u32(tail + 8, n);
    }
    m.set_u32(rec + 0x1c, n);
}

/// [soft 0x0849daa0 fcvFindKeyFrame]: the index of the key of `fcv` whose time is within 0.0001 of `t` (the last key looked
/// at when none is), through `out`; whether one was found and its flags (short at `+0x1c`) say 3 in the low nibble.
pub fn fcv_find_key_frame(m: &mut Mem, fcv: u32, t: f32, out: u32) -> u32 {
    let n = m.i32(fcv + 0xc);
    let mut found = false;
    let mut i = 0i32;
    if n > 0 {
        let keys = m.u32(fcv + 8);
        loop {
            let d = (m.f32(keys.wrapping_add((i as u32).wrapping_mul(0x24))) as f64 - t as f64).abs();
            i += 1;
            found = d < 0.0001;
            if i >= n || found {
                break;
            }
        }
    }
    m.set_u32(out, (i - 1) as u32);
    if found && m.u16(m.u32(fcv + 8).wrapping_add((i as u32).wrapping_mul(0x24)).wrapping_sub(8)) & 0xf == 3 {
        1
    } else {
        0
    }
}

/// [soft 0x0849db20 SI_simulatedValuesGet]: reads the keys of the model's curves at a frame back into the kinematic state
/// (`+0x2f0`): the values at `+0x30..` (position) and `+0x48..` (rotation) with the slopes at `+0x3c..` and `+0x54..`. 1 when
/// any curve had a key there.
pub fn si_simulated_values_get(m: &mut Mem, o: &mut dyn Outside, model: u32, frame: i32) -> u16 {
    let k = m.u32(model + 0x2f0);
    if k == 0 {
        return 0;
    }
    let loc = m.local(4);
    let elem = o.si_ed_get_elem_from_model(m, model, loc);
    m.release(4);
    if elem == 0 {
        return 0;
    }
    let t = frame_to_sec(m, frame).to_f32();
    for off in (0x30u32..0x60).step_by(4) {
        m.set_u32(k + off, 0);
    }
    let mut res = 0u16;
    let idx = m.local(4);
    for (kind, vbase) in [(0x71i32, 0x30u32), (0x65, 0x48)] {
        let (a, b, c) = (m.local(4), m.local(4), m.local(4));
        o.si_get_fcv_triplet(m, elem, a, b, c, 1, kind);
        for (n, p) in [a, b, c].into_iter().enumerate() {
            let f = m.u32(p);
            if f != 0 && fcv_find_key_frame(m, f, t, idx) as u16 != 0 {
                let key = m.u32(f + 8).wrapping_add(m.u32(idx).wrapping_mul(0x24));
                let v = m.u32(key + 0x18);
                m.set_u32(k + vbase + 4 * n as u32, v);
                res = 1;
                let sl = m.u32(key + 8);
                m.set_u32(k + vbase + 0xc + 4 * n as u32, sl);
            }
        }
        m.release(4);
        m.release(4);
        m.release(4);
    }
    m.release(4);
    res
}

/// The key the simulation keeps to write curves from (`pbworkFkey`): one function-curve key, 0x24 bytes.
const PBWORK_FKEY: u32 = 0x086d_7144;
/// The factor from radians to degrees (the double at 0x08660ae0).
const DEGREES: f64 = 57.29577951308232;

/// One key of the work key's value, copy and insertion: the work key's time (+4) is `time`, its value (+0x18) and slope
/// (+8) are copied, and a copy of the key is added after the others of `curve`.
fn add_work_key(m: &mut Mem, o: &mut dyn Outside, curve: u32, time: f32, value: u32, slope: u32) {
    let key = m.u32(PBWORK_FKEY);
    m.set_f32(key + 4, time);
    m.set_f32(key + 0x18, fmov(m.f32(value)));
    m.set_f32(key + 8, fmov(m.f32(slope)));
    let copy = crate::particle_outside::ParticleCalls::si_fkey_copy(o, m, key);
    o.si_fcurve_add_key_after(m, curve, copy, 0, 0, 1);
}

/// [soft 0x0849ddb8 simulationRawFcrvSaveElemFrame]: writes the state the simulation reached (`model` +0x2f0: position at
/// +0x30 with slopes at +0x3c, rotation at +0x48 with slopes at +0x54) as one key on the three position curves (`pos`; kind 0x71)
/// and/or the three rotation curves (`rot`; kind 0x65) of the model's element, at the work key's time. When the element
/// has a trajectory relation (0xa0, models chapter) the trajectory is plotted instead of the position keys. The key times
/// are the model's own: the position curves' at +0x158.. and the rotation curves' at +0x10c.. (times the degree factor).
pub fn simulation_raw_fcrv_save_elem_frame(m: &mut Mem, o: &mut dyn Outside, model: u32, pos: i16, rot: i16, time: f32) {
    let loc = m.local(4);
    let elem = o.si_ed_get_elem_from_model(m, model, loc);
    if elem != 0 {
        let kine = m.u32(model + 0x2f0);
        if pos != 0 {
            let (out_a, out_b) = (m.local(4), m.local(4));
            let (cid, d) = (m.u32(SI_CID_MODELS), elem_dict(m));
            let rel = o.si_elemdict_find_related_elem(m, d, elem, 1, 1, 0xa0, cid, out_b, out_a);
            if rel != 0 {
                let l = m.u32(loc);
                o.si_model_anim_plot_trajectory(m, elem, model, l, time);
            } else {
                let (c1, c2, c3) = (m.local(4), m.local(4), m.local(4));
                let l = m.u32(loc);
                crate::particle_outside::ParticleCalls::si_fcurve_check_add3(o, m, elem, l as i32, 0x71, c3, c2, c1);
                for (n, c) in [c3, c2, c1].into_iter().enumerate() {
                    let n = 4 * n as u32;
                    let t = fmov(m.f32(model + 0x158 + n));
                    let curve = m.u32(c);
                    add_work_key(m, o, curve, t, kine + 0x30 + n, kine + 0x3c + n);
                }
                for _ in 0..3 {
                    m.release(4);
                }
            }
            m.release(4);
            m.release(4);
        }
        if rot != 0 {
            let (c1, c2, c3) = (m.local(4), m.local(4), m.local(4));
            let l = m.u32(loc);
            crate::particle_outside::ParticleCalls::si_fcurve_check_add3(o, m, elem, l as i32, 0x65, c3, c2, c1);
            for (n, c) in [c3, c2, c1].into_iter().enumerate() {
                let n = 4 * n as u32;
                // the degree factor times the model's angle, rounded once to single precision
                let t = (F80::from_f64(DEGREES) * x(m.f32(model + 0x10c + n))).to_f32();
                let curve = m.u32(c);
                add_work_key(m, o, curve, t, kine + 0x48 + n, kine + 0x54 + n);
            }
            for _ in 0..3 {
                m.release(4);
            }
        }
    }
    m.release(4);
}

/// [soft 0x0849e030 SI_simulationRawFcrvSaveKeys]: records the simulation's state at `time` on the model's curves: makes
/// the work key when there is none (flags `+0x1c` and `+0xc` 3, the cubic slope kind), sets its first word to `time`, and saves
/// the frame: both position and rotation for a body of kind 12; otherwise the position of the model, then the rotation
/// of the model and of the bodies of its chain (the first body of the chain, for a kind 0 model, then each `+0x30c`
/// brother). The model's simulation record is marked as holding keys (`+0x12`).
pub fn si_simulation_raw_fcrv_save_keys(m: &mut Mem, o: &mut dyn Outside, model: u32, time: f32) {
    if m.u32(PBWORK_FKEY) == 0 {
        let k = o.si_fkey_create(m, 0, 0, 0, 0, 1);
        m.set_u32(PBWORK_FKEY, k);
        m.set_u16(k + 0x1c, 3);
        m.set_u16(k + 0xc, 3);
    }
    let k = m.u32(PBWORK_FKEY);
    m.set_f32(k, fmov(time));
    if m.i32(model + 0x2ec) == 0xc {
        simulation_raw_fcrv_save_elem_frame(m, o, model, 1, 1, time);
        let rec = m.u32(model + 0x304);
        m.set_u16(rec + 0x12, 1);
        return;
    }
    simulation_raw_fcrv_save_elem_frame(m, o, model, 1, 0, time);
    let rec = m.u32(model + 0x304);
    m.set_u16(rec + 0x12, 1);
    let mut p = model;
    if m.u32(model + 0x30) == 0 {
        p = m.u32(model + 0x2c4);
    }
    while p != 0 {
        simulation_raw_fcrv_save_elem_frame(m, o, p, 0, 1, time);
        p = m.u32(p + 0x30c);
    }
}
