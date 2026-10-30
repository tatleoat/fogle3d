// SPDX-License-Identifier: MIT
//! The one routine of the original's `polygon.c` the renderer calls (finding 0012, section
//! 13): the box a 2D map is projected from.

use crate::shade::ShadeMap;
use crate::softtriangle::{model_ctr_bbox, model_geo_bbox};
use crate::types::Vector;
use si_math::algebra::{matrix_inverse, matrix_scal_rot_trans};
use si_x87::x;

/// What `SI_modelComputeRotBounding` answers: the box of the node's points in the map's
/// rotated frame and, unless bit 0x20 of `SI_viewerUpdOpt` is set, the two radii.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RotBounding {
    pub least: Vector,
    pub greatest: Vector,
    pub radii: Option<(f32, f32)>,
}

/// The box a 2D map is projected from [soft 0x0829b584 SI_txtProjectionInfo]:
/// `SI_txtProjectionInfo(model, map, &bound, &size, &centre)`; `bound` is x min, x max, y
/// min, y max, z min, z max.
///
/// 1. The map's matrix (+0xb8) = the inverse of `SI_matrixScalRotTrans((1, 1, 1), rotation
///    (+0x94), (0, 0, 0))`; left as it was when there is none.
/// 2. When the map's box was made for this node (+0x1bc equals the node's number, +0x354):
///    the box kept at +0x1a4 … +0x1b8 is the result; size = greatest − least, centre = 0.5 ×
///    (least + greatest); the map's centre (+0x88) = `SI_modelCtrBbox` and its half extents
///    (+0x7c) = `SI_modelGeoBbox` of the kept box and the kept radii (+0x1c0, +0x1c4).
/// 3. Otherwise `SI_modelComputeRotBounding(model, rotation)` gives a box and two radii;
///    +0x88 and +0x7c from them as above; then **the box is scaled about its centre by the
///    map's scaling** (+0xac): least = centre − half × scale, greatest = half × scale +
///    centre per axis; size and centre follow from that (size.x from the two unrounded
///    values, the others from the stored floats); the box, the radii and the node's number
///    are kept in the map.
///
/// `bounding` is called in case 3 only; a result without radii (bit 0x20 of
/// `SI_viewerUpdOpt`) leaves the two locals unset in the original, which then go into the
/// half extents and the map: `unset` here. Class: libm (the rotation); the rest single.
pub fn txt_projection_info(m: &mut ShadeMap, elem_id: i32, bounding: &mut dyn FnMut(&Vector) -> RotBounding, unset: f32, bound: &mut [f32; 6], size: &mut Vector, centre: &mut Vector) {
    let rot = Vector::new(m.rotation[0], m.rotation[1], m.rotation[2]);
    let frame = matrix_scal_rot_trans(&Vector::new(1.0, 1.0, 1.0), &rot, &Vector::new(0.0, 0.0, 0.0));
    if let Some(inverse) = matrix_inverse(&frame) {
        m.tmat = inverse;
    }
    let half = x(0.5f64);
    if m.box_node == elem_id {
        let (lo, hi) = (m.box_min, m.box_max);
        *size = Vector::new((x(hi[0]) - lo[0]).to_f32(), (x(hi[1]) - lo[1]).to_f32(), (x(hi[2]) - lo[2]).to_f32());
        *centre = Vector::new((half * (x(lo[0]) + hi[0])).to_f32(), (half * (x(lo[1]) + hi[1])).to_f32(), (half * (x(lo[2]) + hi[2])).to_f32());
        *bound = [lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]];
        let (least, greatest) = (Vector::new(lo[0], lo[1], lo[2]), Vector::new(hi[0], hi[1], hi[2]));
        let c = model_ctr_bbox(least, greatest);
        m.centre = [c.x, c.y, c.z];
        let h = model_geo_bbox(m.method, least, greatest, m.box_r5, m.box_r6);
        m.v7c = [h.x, h.y, h.z];
        return;
    }
    let b = bounding(&rot);
    let (r5, r6) = b.radii.unwrap_or((unset, unset));
    let c = model_ctr_bbox(b.least, b.greatest);
    m.centre = [c.x, c.y, c.z];
    let h = model_geo_bbox(m.method, b.least, b.greatest, r5, r6);
    m.v7c = [h.x, h.y, h.z];
    let lo_x = x(m.centre[0]) - (x(m.v7c[0]) * m.scaling[0]);
    let hi_x = (x(m.v7c[0]) * m.scaling[0]) + m.centre[0];
    bound[0] = lo_x.to_f32();
    bound[1] = hi_x.to_f32();
    bound[2] = (x(m.centre[1]) - (x(m.v7c[1]) * m.scaling[1])).to_f32();
    bound[3] = ((x(m.v7c[1]) * m.scaling[1]) + m.centre[1]).to_f32();
    bound[4] = (x(m.centre[2]) - (x(m.v7c[2]) * m.scaling[2])).to_f32();
    bound[5] = ((x(m.v7c[2]) * m.scaling[2]) + m.centre[2]).to_f32();
    *size = Vector::new((hi_x - lo_x).to_f32(), (x(bound[3]) - bound[2]).to_f32(), (x(bound[5]) - bound[4]).to_f32());
    *centre = Vector::new((half * (x(bound[0]) + bound[1])).to_f32(), (half * (x(bound[2]) + bound[3])).to_f32(), (half * (x(bound[4]) + bound[5])).to_f32());
    m.box_min = [bound[0], bound[2], bound[4]];
    m.box_max = [bound[1], bound[3], bound[5]];
    m.box_r5 = r5;
    m.box_r6 = r6;
    m.box_node = elem_id;
}
