// SPDX-License-Identifier: MIT
//! `mod_global.c`: the scaling method switch.

use super::{SI_REFRESH_ALL, SI_GRAF_LISTS, SI_PROC_DATA};
use super::ModelS;
use si_mem::{Mem, Outside, Ptr32};
use si_model::model_image::state;
use si_types::model::Model;

/// `SI_glbManip`, the manipulation mode word.
const SI_GLB_MANIP: u32 = 0x0878_1560;

/// What an x87 load and store of a `float` does to its bits: the same, but a signalling NaN
/// comes out quiet.
pub(crate) fn fld_fstp(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// Copies the scale of every model of the current display list (the nodes of kind 2) to the
/// saved scale at +0x3a0 when `SI_glbManip` is 1, else back from it, and updates the state of the
/// node and its sons; with a non-zero `redraw` (a 16-bit value) the views are refreshed
/// [soft 0x0817d4fc SI_updateScalingMethod].
pub fn update_scaling_method(m: &mut Mem, o: &mut dyn Outside, redraw: i16) {
    let idx = m.i32(m.u32(SI_PROC_DATA) + 4);
    let mut gn = m.u32(m.u32(SI_GRAF_LISTS).wrapping_add((idx.wrapping_mul(9) as u32).wrapping_mul(4)).wrapping_add(4));
    while gn != 0 {
        if m.i32(gn) == 2 {
            let node = m.u32(gn + 4);
            for k in 0..3u32 {
                let (from, to) = if m.i32(SI_GLB_MANIP) == 1 { (0x16c + 4 * k, 0x3a0 + 4 * k) } else { (0x3a0 + 4 * k, 0x16c + 4 * k) };
                let v = fld_fstp(m.u32(node + from));
                m.set_u32(node + to, v);
            }
            state::update_son_state(m, &mut ModelS(o), Ptr32::<Model>::NULL, Ptr32::new(node), 1);
        }
        gn = m.u32(gn + 0x1c);
    }
    if redraw != 0 {
        o.call(m, SI_REFRESH_ALL, &[1]);
    }
}
