// SPDX-License-Identifier: MIT
//! Reversal of curves (and, later, of surfaces): the editing routines of `nrb_b.c` that
//! change points and knots in place.

use crate::knots::dir_inverse;
use crate::own::Host;
use crate::{NurbsCrv, NurbsPoint};

/// [soft 0x084dca24 SI_nurbsCrvInverse]: reverses a curve in place: the control points
/// change places end for end (whole records, flags included), the knot vector is reversed
/// by [`dir_inverse`], and for a keyframed curve (+0x4c not 0) the animation package turns
/// the keys of its path curve round. Where `SI_gblIndex` is set, both indices of every
/// exchanged pair are recorded in it (the middle point of an odd count is not). Returns 1.
pub fn crv_inverse(host: &mut dyn Host, rec: &mut NurbsCrv, knots: &mut [f64], points: &mut [NurbsPoint]) -> Option<i32> {
    let mut i = 0i32;
    let mut j = rec.dir.nb_points.wrapping_sub(1);
    if 0 < j {
        loop {
            let (a, b) = (usize::try_from(i).ok()?, usize::try_from(j).ok()?);
            if b >= points.len() {
                return None;
            }
            points.swap(a, b);
            if let Some(t) = host.gbl_index() {
                *t.get_mut(b)? = i;
                *t.get_mut(a)? = j;
            }
            i += 1;
            j -= 1;
            if i >= j {
                break;
            }
        }
    }
    dir_inverse(&mut rec.dir, knots)?;
    if rec.keyframed != 0 {
        host.invert_path_keys();
    }
    Some(1)
}
