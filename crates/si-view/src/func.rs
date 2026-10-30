// SPDX-License-Identifier: MIT
//! `func.c` (0x0816e290 to 0x08176734): the routine that draws a graph node, chosen by what the
//! node holds. Only the chooser [soft 0x08176618 SI_updateGnFunc] is here; the drawing
//! routines (`faceFunc` and the others, whose addresses are the values it stores) are not.

use crate::outside::{self, Outside};
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_types::scene::GrafNode;

// Stored, never called, here: `patchFunc (0x08172140)`, `faceFunc (0x0816e290)`,
// `meshFunc (0x08170628)`, `splineFunc (0x08172d14)`, `ballFunc (0x081738f8)`,
// `nurbsCrvFunc (0x08175734)`, `nurbsSrfFunc (0x08173af0)`, `SI_fcurveFunc (0x0820490c)`.
const PATCH_FUNC: u32 = 0x0817_2140;
const FACE_FUNC: u32 = 0x0816_e290;
const MESH_FUNC: u32 = 0x0817_0628;
const SPLINE_FUNC: u32 = 0x0817_2d14;
const BALL_FUNC: u32 = 0x0817_38f8;
const NURBS_CRV_FUNC: u32 = 0x0817_5734;
const NURBS_SRF_FUNC: u32 = 0x0817_3af0;
const FCURVE_FUNC: u32 = 0x0820_490c;

/// Sets a node's drawing routine from what it holds [soft 0x08176618 SI_updateGnFunc].
///
/// - A node of kind 2 or 3 holds a model; the model's kind picks the routine stored at
///   +0x18: 0 none (stored as null), 1 patch, 2 face, 4 mesh, 6 spline, 8 ball, 9 NURBS
///   curve, 10 NURBS surface; any other kind leaves the node's routine as it was. For a
///   patch model every one of its `nb_defs` definitions also goes through
///   `SI_patchUpdateSurface (0x0847cee4)`, in order, after the routine is stored (the
///   original counts with a 16-bit signed index, so a count above 0x8000 never ends;
///   reproduced).
/// - A node of kind 9 takes `SI_fcurveFunc`. Any other kind (below 2, 4 to 8, above 9, or
///   negative as a signed number) is left alone.
pub fn update_gn_func(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<GrafNode>) {
    let kind = m[node].kind;
    if kind > 1 {
        if kind < 4 {
            let model: Ptr32<Model> = m[node].data.cast();
            match m[model].kind {
                0 => m[node].func = Ptr32::NULL,
                1 => {
                    m[node].func = Ptr32::new(PATCH_FUNC);
                    let count = m[model].nb_defs as i32;
                    let defs = m[model].defs.addr();
                    let mut i: i16 = 0;
                    if count != 0 {
                        loop {
                            let patch = m.u32(defs.wrapping_add((i as i32 as u32).wrapping_mul(4)));
                            o.call(m, &outside::SI_PATCH_UPDATE_SURFACE, &[patch]);
                            i = i.wrapping_add(1);
                            if m.fault().is_some() || (i as i32) >= count {
                                break;
                            }
                        }
                    }
                }
                2 => m[node].func = Ptr32::new(FACE_FUNC),
                4 => m[node].func = Ptr32::new(MESH_FUNC),
                6 => m[node].func = Ptr32::new(SPLINE_FUNC),
                8 => m[node].func = Ptr32::new(BALL_FUNC),
                9 => m[node].func = Ptr32::new(NURBS_CRV_FUNC),
                10 => m[node].func = Ptr32::new(NURBS_SRF_FUNC),
                _ => {}
            }
        } else if kind == 9 {
            m[node].func = Ptr32::new(FCURVE_FUNC);
        }
    }
}
