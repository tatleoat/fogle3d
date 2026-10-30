// SPDX-License-Identifier: MIT
//! `metaball.c`, keying the shape of a metaball (finding 0038): the three exponents and the
//! weight of every ball of the active system become keys of the f-curves of their parameters.
//! The f-curve routines (`motionKey.c`, `fkey.c`, `fcurve.c`) are outside this crate and
//! reached through [`MetaCalls`](crate::metaball::MetaCalls).

use crate::metaball::{next_son, si_get_active_meta_system, MetaCalls};
use si_hrc::image::meta::{ball, system};
use si_mem::{Mem, Outside};
use si_x87::x;

/// `SI_frameCurrent (0x08782040)`, an int.
const SI_FRAME_CURRENT: u32 = 0x0878_2040;

/// `curve->interpolation` read as the key's last argument: the short at +0x1e, sign-extended.
fn interp(m: &Mem, curve: u32) -> i32 {
    m.i16(curve + 0x1e) as i32
}

/// A key of `value` (a double of the ball, rounded to a float on its way through memory) at
/// `frame`, added to `curve`: slope 0, slope type 0, the curve's own interpolation.
fn add_key(m: &mut Mem, o: &mut dyn Outside, curve: u32, frame: i32, value: f64) {
    let v = x(value).to_f32();
    let key = o.si_fkey_frame_create(m, frame, v.to_bits(), 0, 0, interp(m, curve));
    o.si_fcurve_add_key(m, curve, key);
}

/// [soft 0x0845e430 SI_metaballSaveInternalParameters]: the ball's node gives its element
/// (nothing happens without one); the weight (+0xd4) is keyed at `frame` on the curve of
/// parameter 0xf1 and the three exponents (+0xdc, +0xe4, +0xec) on the three curves of 0xf2.
/// Each value is rounded to a float, then handed to `SI_fkeyFrameCreate`.
pub fn si_metaball_save_internal_parameters(m: &mut Mem, o: &mut dyn Outside, pball: u32, frame: i32) {
    if pball == 0 {
        return;
    }
    // locals of the original, in the order of its frame: the curves of 0xf2, the one of 0xf1, the id
    let curves = m.local(0x10);
    let id = curves + 0x0c;
    let c1 = curves + 0x08;
    let (c3a, c3b) = (curves, curves + 4);
    let elem = o.si_ed_get_elem_from_model(m, m.u32(pball + ball::MODEL), id);
    if elem != 0 {
        o.si_fcurve_check_add1(m, elem, m.u32(id), 0xf1, c1);
        add_key(m, o, m.u32(c1), frame, m.f64(pball + ball::WEIGHT));
        o.si_fcurve_check_add3(m, elem, m.u32(id), 0xf2, c1, c3a, c3b);
        add_key(m, o, m.u32(c1), frame, m.f64(pball + ball::EXPONENT));
        add_key(m, o, m.u32(c3a), frame, m.f64(pball + 0xe4));
        add_key(m, o, m.u32(c3b), frame, m.f64(pball + 0xec));
    }
    m.release(0x10);
}

/// [soft 0x0845e584 SI_metaSystemSaveShape]: for every son of the active system's node that is
/// a metaball (kind 8), its parameters are keyed at the current frame and the node's keys
/// saved (`SI_savekeyObjectAll`). Nothing happens without an active system.
pub fn si_meta_system_save_shape(m: &mut Mem, o: &mut dyn Outside) {
    let sys = si_get_active_meta_system(m, o, 0);
    if sys == 0 {
        return;
    }
    let top = m.u32(sys + system::MODEL);
    let mut node = 0;
    loop {
        node = next_son(m, top, node);
        if node == 0 {
            break;
        }
        if m.i32(node + 0x30) == 8 {
            let def = m.u32(node + 0x48);
            si_metaball_save_internal_parameters(m, o, def, m.i32(SI_FRAME_CURRENT));
            o.si_savekey_object_all(m, node);
        }
    }
}
