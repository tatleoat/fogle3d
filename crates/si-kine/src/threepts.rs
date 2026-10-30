// SPDX-License-Identifier: MIT
//! The helper object of the three-points constraint (`constraint.c`), on the shared
//! memory image (finding 0024). The orientation step is
//! `limits::constr_3pts_map_direction` (first pass, old form).

use si_mem::{Mem, Ptr32};
use si_types::geom::{Spline, SplineKey, Vector};
use si_types::model::Model;

use crate::globals::{SI_GRAF_LISTS, SI_PROC_DATA};
use crate::outside as out;
use crate::outside::Outside;
use crate::rodata::S_FACE;

/// Makes the triangle the three-points constraint shows: a face object named `face` whose
/// outline is a closed spline through (1, 0, 0), (−1, 0, −1) and (−1, 0, 1), added to the
/// current graf list [soft 0x081f3c64 si_constr3PntsFormFace]. Answers the new node.
///
/// The spline comes from `SI_splineAllocate` and its three keys from
/// `SI_splineKeyAllocate(3)`; of each key the position is stored first, then the two
/// handles, all three the same point. After `SI_splineUpdateMatrix` and `SI_splineTouch`
/// the spline is moved into a new face (`SI_splineTransfer` to the face's +4), the face
/// becomes definition 2 of a new node, the selection is saved (`SI_backSelected(0, 1)`),
/// the node goes into the graf list of the current process slot (`SI_grafLists` +
/// 0x24 × the second word of `SI_procData`) and both buffers and the counter are
/// refreshed. Nothing is tested on the way: a null from an allocator is stored through.
pub fn constr_3pnts_form_face(m: &mut Mem, o: &mut dyn Outside) -> Ptr32<Model> {
    let spline: Ptr32<Spline> = out::si_spline_allocate(o, m);
    m[spline].closed = 1;
    m[spline].nb_keys = 3;
    let keys: Ptr32<SplineKey> = out::si_spline_key_allocate(o, m, 3);
    m[spline].keys = keys;
    for (i, p) in [(1.0f32, 0.0f32, 0.0f32), (-1.0, 0.0, -1.0), (-1.0, 0.0, 1.0)].into_iter().enumerate() {
        let p = Vector { x: p.0, y: p.1, z: p.2 };
        let key = m[spline].keys.add(i as i32);
        m[key].pos = p;
        let key = m[spline].keys.add(i as i32);
        m[key].prev = p;
        let key = m[spline].keys.add(i as i32);
        m[key].next = p;
    }
    out::si_spline_update_matrix(o, m, spline);
    out::si_spline_touch(o, m, spline);
    let face = out::si_face_allocate(o, m);
    out::si_spline_transfer(o, m, spline, face.wrapping_add(4));
    let node: Ptr32<Model> = out::si_model_allocate(o, m, S_FACE.0);
    out::si_model_add_definition(o, m, node, 2, face);
    out::si_back_selected(o, m, 0, 1);
    let slot = m.i32(m[SI_PROC_DATA].addr().wrapping_add(4));
    let list = m[SI_GRAF_LISTS].wrapping_add((slot as u32).wrapping_mul(0x24));
    out::si_graflist_add_data(o, m, list, node.addr(), 2, 1);
    out::si_refresh_selected_both_buffers(o, m, 1);
    out::si_refresh_counter(o, m);
    node
}
