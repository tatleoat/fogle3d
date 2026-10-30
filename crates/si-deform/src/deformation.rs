// SPDX-License-Identifier: MIT
//! `deformation.c`: the deformation menus' commands (lattices and fittings created,
//! frozen, removed, deactivated), the touches that tell the update a deformation of a
//! node or of a branch changed, and the point and frame along a fitting's curve. Only what
//! the update reaches is ported so far.

use crate::outside::*;
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::fmov;

/// After the animation moved a deformation [soft 0x08153618 SI_postAnimTouch]: the
/// working definition of `model` is rebuilt (`SI_modelReadjustDefinition`), and for a
/// branch (`branch`, a `short`, not 0) that of every node of its subtree too, in the order
/// `SI_modelNextSon` walks it (depth first, from a null start). Answers nothing.
pub fn post_anim_touch(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, branch: i16) {
    o.call(m, &SI_MODEL_READJUST_DEFINITION, &[model.addr()]);
    if branch == 0 {
        return;
    }
    let mut son = o.call(m, &SI_MODEL_NEXT_SON, &[model.addr(), 0]);
    while son != 0 {
        o.call(m, &SI_MODEL_READJUST_DEFINITION, &[son]);
        son = o.call(m, &SI_MODEL_NEXT_SON, &[model.addr(), son]);
    }
}

/// The point and the frame at `distance` along the curve a spline fitting `fit` follows
/// [soft 0x081561dc SI_curveDistanceToValueFrenet]: by the kind (+0x30) of the followed
/// node (`fit` + 4), `SI_splineDistanceToValueFrenet` for a spline (6) or
/// `SI_nurbsDistanceToValueFrenet` for a NURBS curve (9), each with the node's definition
/// (+0x48), the distance (moved through the unit), `point`, `fit` + 8 (the fitting's
/// frames) and `out`; any other kind nothing. The frames the caller passes (`frames`) are
/// not read: the original always hands on `fit` + 8. Answers nothing.
pub fn curve_distance_to_value_frenet(m: &mut Mem, o: &mut dyn Outside, fit: u32, distance: f32, point: u32, frames: u32, out: u32) {
    let _ = frames;
    let node = m.u32(fit.wrapping_add(4));
    let routine = match m.i32(node.wrapping_add(0x30)) {
        6 => &SI_SPLINE_DISTANCE_TO_VALUE_FRENET,
        9 => &SI_NURBS_DISTANCE_TO_VALUE_FRENET,
        _ => return,
    };
    let def = m.u32(node.wrapping_add(0x48));
    o.call(m, routine, &[def, fmov(distance).to_bits(), point, fit.wrapping_add(8), out]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frenet_goes_to_the_followed_curve() {
        let mut m = Mem::new();
        let fit = m.si_malloc(0xa8);
        let node = m.si_malloc(0x430);
        m.set_u32(fit + 4, node);
        m.set_u32(node + 0x48, 0x1234_5678);
        for (kind, name) in [(6, Some("SI_splineDistanceToValueFrenet")), (9, Some("SI_nurbsDistanceToValueFrenet")), (4, None), (10, None)] {
            m.set_i32(node + 0x30, kind);
            let mut o = NoOutside::default();
            curve_distance_to_value_frenet(&mut m, &mut o, fit, 2.5, 0x100, 0x200, 0x300);
            match name {
                Some(n) => {
                    assert_eq!(o.calls.len(), 1);
                    assert_eq!(o.calls[0].0.name, n);
                    assert_eq!(o.calls[0].1, vec![0x1234_5678, 2.5f32.to_bits(), 0x100, fit + 8, 0x300]);
                }
                None => assert!(o.calls.is_empty()),
            }
        }
    }
}
