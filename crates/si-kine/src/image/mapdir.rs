// SPDX-License-Identifier: MIT
//! The orientation step of the three-point, normal and tangency constraints on the image
//! (`constraint.c`; finding 0022, "Orientation from a direction"; the value forms are in
//! [`crate::limits`]).

use si_math::algebra::{matrix_align_axes, matrix_align_y_axes, matrix_extract_xyz_rotation, matrix_inverse, matrix_multiply, matrix_x_rotation, matrix_y_rotation};
use si_math::types::Matrix;
use si_mem::Mem;
use si_types::geom::Vector;
use si_x87::x;

use super::{elem_from_model, find_related, models_chapter, mv, set_rotation, tv, Manip, Node};
use crate::outside::Outside;

/// A global point in the frame of a node's father: through the inverse of the father's
/// `gmat`, row 3 included, in the original's order of sums. A node without a father keeps
/// the point. When the father's matrix has no inverse the original computes with its stack
/// slot's bytes; the point is kept here and a fault counted ([`crate::fault`]).
fn into_father(m: &Mem, node: Node, p: Vector) -> Vector {
    let f = m[node].father;
    if f.is_null() {
        return p;
    }
    let Some(i) = matrix_inverse(&m[f].gmat) else {
        crate::fault::note();
        return p;
    };
    Vector {
        x: (((x(p.y) * i[1][0] + x(p.x) * i[0][0]) + x(p.z) * i[2][0]) + i[3][0]).to_f32(),
        y: ((x(p.z) * i[2][1] + (x(p.y) * i[1][1] + x(p.x) * i[0][1])) + i[3][1]).to_f32(),
        z: ((x(p.z) * i[2][2] + (x(p.y) * i[1][2] + x(p.x) * i[0][2])) + i[3][2]).to_f32(),
    }
}

/// The angles of a frame (zeros as the previous solution) set as the node's rotation,
/// mode 1, no geometry compensation.
fn orient(m: &mut Mem, o: &mut dyn Outside, node: Node, frame: &Matrix) {
    let g = Manip::read(m);
    let zero = Vector { x: 0.0, y: 0.0, z: 0.0 };
    let angles = tv(matrix_extract_xyz_rotation(frame, &mv(zero), g.keep_turns));
    set_rotation(m, o, node, angles, 1, 0);
}

/// [soft 0x081f3e48 si_constr3PtsMapDirection]: the node turned so that its Y axis lies
/// along `second` and its X axis as near `first` as can be (`SI_matrixAlignYAxes`, both
/// brought into the father's frame first). False, and nothing set, when no frame can be
/// built ([`crate::limits::constr_3pts_map_direction`]).
pub fn constr_3pts_map_direction(m: &mut Mem, o: &mut dyn Outside, node: Node, first: Vector, second: Vector) -> bool {
    let (a, b) = (into_father(m, node, first), into_father(m, node, second));
    match matrix_align_y_axes(&mv(a), &mv(b)) {
        Some(frame) => {
            orient(m, o, node, &frame);
            true
        }
        None => false,
    }
}

/// [soft 0x081f3ad0 si_norConstrMapDirection]: as [`constr_3pts_map_direction`], the frame
/// turned first by Ry(`__SI_Model` + 0x168), the kept copy of the node's y angle.
pub fn nor_constr_map_direction(m: &mut Mem, o: &mut dyn Outside, node: Node, first: Vector, second: Vector) -> bool {
    let (a, b) = (into_father(m, node, first), into_father(m, node, second));
    match matrix_align_y_axes(&mv(a), &mv(b)) {
        Some(frame) => {
            let turn = matrix_y_rotation(m[node].unk_164[1]);
            orient(m, o, node, &matrix_multiply(&turn, &frame));
            true
        }
        None => false,
    }
}

/// [soft 0x081f4998 si_tanConstrMapDirection]: X along the tangent, Y towards (0, 1, 0) or,
/// when the node's element has an up-vector relation (128, found with
/// `SI_elemdictFindRelatedElem`), towards that relation's object (the model at the found
/// element's +4); the frame turned first by Rx(`__SI_Model` + 0x164). False, and nothing
/// set, when the tangent is zero ([`crate::limits::tan_constr_map_direction`]).
pub fn tan_constr_map_direction(m: &mut Mem, o: &mut dyn Outside, node: Node, tangent: Vector) -> bool {
    let mut up = Vector { x: 0.0, y: 1.0, z: 0.0 };
    let elem = elem_from_model(m, o, node);
    let chapter = models_chapter(m);
    let found = find_related(m, o, elem, 0x80, chapter);
    if found != 0 {
        let other: Node = si_mem::Ptr32::new(m.u32(found + 4));
        let (a, b) = (m[other].gtrs, m[node].gtrs);
        up = Vector { x: (x(a.x) - b.x).to_f32(), y: (x(a.y) - b.y).to_f32(), z: (x(a.z) - b.z).to_f32() };
    }
    let (a, b) = (into_father(m, node, tangent), into_father(m, node, up));
    let (frame, ok) = matrix_align_axes(&mv(a), &mv(b));
    if ok {
        let turn = matrix_x_rotation(m[node].unk_164[0]);
        orient(m, o, node, &matrix_multiply(&turn, &frame));
    }
    ok
}
