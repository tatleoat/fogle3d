// SPDX-License-Identifier: MIT
//! A node's geometry marked as changed, put through a matrix, and drawn: small routines of
//! `model.c` on the image (finding 0015, sixth pass). Their store forms are
//! [`crate::node_copy::touch_definition`], [`crate::node_copy::touch_geometry`],
//! [`crate::hierarchy::next_def_get`] and [`crate::transform::multiply_definition`].
//!
//! The image forms of the fourth pass that call these two (`defs`, `setup`) still call them
//! through [`Outside`], as their vectors have them stood in; whoever wires the program
//! answers those calls with these functions.

use si_mem::{Mem, Ptr32};
use si_types::model::Model;

use super::{at, bounds, callee, defs, mx, Outside};

/// Marks a node's working definition as changed [soft 0x0846d530 SI_modelTouchDefinition]:
/// bit 0x10 in the low bytes of both update flag words, `SI_removeTriangle(node, 0)`, then
/// by kind: a spline is touched (`SI_splineTouch`), a mesh too (`si_meshTouch`); a NURBS
/// curve gets bits 0x60 in its byte at +0x3b and `SI_nurbsCrvAnimUpdate`; a NURBS surface
/// bits 0x60 and then 0x20 in its byte at +0x5b (two `or`s, as the original writes them).
/// The kind is read again before each test, the definition pointer at each use.
pub fn touch_definition(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>) {
    m.or_u8(at!(node, update_flags), 0x10);
    m.or_u8(at!(node, update_flags2), 0x10);
    o.call(m, &callee::SI_REMOVE_TRIANGLE, &[node.addr(), 0]);
    if m[node].kind == 6 {
        let d = m[node].definition.addr();
        o.call(m, &callee::SI_SPLINE_TOUCH, &[d]);
    }
    if m[node].kind == 4 {
        let d = m[node].definition.addr();
        o.call(m, &callee::SI_MESH_TOUCH, &[d]);
    }
    if m[node].kind == 9 {
        let d = m[node].definition.addr();
        m.or_u8(d.wrapping_add(0x3b), 0x60);
        let d = m[node].definition.addr();
        o.call(m, &callee::SI_NURBS_CRV_ANIM_UPDATE, &[d]);
    }
    if m[node].kind == 10 {
        let d = m[node].definition.addr();
        m.or_u8(d.wrapping_add(0x5b), 0x60);
        let d = m[node].definition.addr();
        m.or_u8(d.wrapping_add(0x5b), 0x20);
    }
}

/// Marks a node's geometry as changed and brings its working definition up to date
/// [soft 0x0846d48c SI_modelTouchGeometry]: bit 0x10 in both update flag words; then, if
/// none of an active lattice ([`defs::lattice`]), spline fitting, patch fitting
/// (`defs::spline_fitting`, `defs::patch_fitting`, each handed one 16-bit local), a wave
/// on the node or on its branch (`SI_modelNodeWave`, `SI_modelBranchWave`) deforms it and
/// it depends on itself (`latdep`), the definition is touched ([`touch_definition`]);
/// otherwise it is readjusted (`SI_modelReadjustDefinition`). The questions stop at the
/// first that answers yes. Last, a node whose 16-bit word at +0x0c (`bbox_valid`) is set
/// has its bounding box recomputed ([`bounds::compute_bounding`]).
pub fn touch_geometry(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>) {
    let set = m.local(4);
    m.or_u8(at!(node, update_flags), 0x10);
    m.or_u8(at!(node, update_flags2), 0x10);
    let deformed = defs::lattice(m, node, set) != 0
        || defs::spline_fitting(m, node, set) != 0
        || defs::patch_fitting(m, node, set) != 0
        || o.call(m, &callee::SI_MODEL_NODE_WAVE, &[node.addr()]) != 0
        || o.call(m, &callee::SI_MODEL_BRANCH_WAVE, &[node.addr()]) != 0;
    if !deformed && m[node].latdep == node {
        touch_definition(m, o, node);
    } else {
        o.call(m, &callee::SI_MODEL_READJUST_DEFINITION, &[node.addr()]);
    }
    if m[node].bbox_valid != 0 {
        bounds::compute_bounding(m, node);
    }
    m.release(4);
}

/// Draws a node and what hangs below it [soft 0x0846db60 SI_modelDraw]: `pushmatrix`,
/// `multmatrix(gmat)`, the stored definition numbered `cur_def` by its kind's routine
/// (patch, face, mesh, spline, NURBS curve; a NURBS surface with a second argument 1;
/// kinds 3, 5, 7, 8 and anything outside 1 ..= 10 draw nothing), the first son with its
/// brothers (this routine with 1), `popmatrix`; then, when asked (`brothers` not 0), the
/// next brother with its later brothers. The GL calls and the kinds' drawing routines are
/// the layer below (`si-gl`, the geometry packages): all through [`Outside`].
pub fn draw(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, brothers: i16) {
    o.call(m, &callee::PUSHMATRIX, &[]);
    o.call(m, &callee::MULTMATRIX, &[at!(node, gmat)]);
    let kind = m[node].kind;
    let routine = match kind {
        1 => Some(&callee::SI_PATCH_DRAW),
        2 => Some(&callee::SI_FACE_DRAW),
        4 => Some(&callee::SI_MESH_DRAW),
        6 => Some(&callee::SI_SPLINE_DRAW),
        9 => Some(&callee::SI_NURBS_CRV_DRAW),
        10 => Some(&callee::SI_NURBS_SRF_DRAW),
        _ => None,
    };
    if let Some(routine) = routine {
        let array = m[node].defs.addr();
        let def = m.u32(array.wrapping_add(4 * m[node].cur_def as u16 as u32));
        if kind == 10 {
            o.call(m, routine, &[def, 1]);
        } else {
            o.call(m, routine, &[def]);
        }
    }
    let son = m[node].son;
    if !son.is_null() {
        draw(m, o, son, 1);
    }
    o.call(m, &callee::POPMATRIX, &[]);
    if brothers != 0 {
        let brother = m[node].brother;
        if !brother.is_null() {
            draw(m, o, brother, 1);
        }
    }
}

/// The next of a node's geometries in the order stored ones, working one, original one
/// [soft 0x08466e64 SI_modelNextDefGet]; `cursor` (an `int` the caller keeps, from 0) is
/// moved on. With `n` stored definitions: for a cursor below `n` the stored one at it; at
/// `n` the working definition if it is not the stored one numbered `cur_def` (the cursor
/// becomes `n` + 1), else (and at `n` + 1) the original definition (+0x3c), the cursor going
/// one further; null, the cursor left alone, without stored definitions, for a negative
/// cursor or one beyond `n` + 1.
pub fn next_def_get(m: &mut Mem, node: Ptr32<Model>, cursor: u32) -> u32 {
    let n = m[node].nb_defs as u16 as i32;
    let c = m.i32(cursor);
    if n == 0 || c < 0 || n + 1 < c {
        return 0;
    }
    if c < n {
        let d = m.u32(m[node].defs.addr().wrapping_add(4 * c as u32));
        m.set_i32(cursor, c + 1);
        return d;
    }
    if c == n {
        m.set_i32(cursor, n + 1);
        let working = m[node].definition.addr();
        let current = m.u32(m[node].defs.addr().wrapping_add(4 * m[node].cur_def as u16 as u32));
        if working != current {
            return working;
        }
    }
    let c = m.i32(cursor);
    m.set_i32(cursor, c + 1);
    m[node].org_def.addr()
}

/// Puts every geometry of a node through a matrix [soft 0x0846d3a4 SI_modelMultiplyDefinition]:
/// the transpose of the matrix's inverse is formed first (`SI_matrixTransposeInverse`, the
/// identity when there is none), then for each geometry [`next_def_get`] answers, by the
/// node's kind read anew each time: a patch, a face, a spline, a NURBS curve or surface
/// through its own routine (the last two with a third argument 0), a mesh through
/// `SI_meshTransform` with the matrix and that transpose, a metaball (kind 8) by
/// multiplying its own matrix (+0x0c) by the matrix; other kinds are passed over. Then a
/// node with `bbox_valid` set has its box recomputed and the geometry is touched
/// ([`touch_geometry`]).
pub fn multiply_definition(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, matrix: u32) {
    let normals = m.local(0x40);
    let cursor = m.local(4);
    m.set_i32(cursor, 0);
    let (t, _) = si_math::algebra::matrix_transpose_inverse(&super::mat(m, matrix));
    super::put_mat(m, normals, &t);
    loop {
        let d = next_def_get(m, node, cursor);
        if d == 0 {
            break;
        }
        match m[node].kind {
            1 => {
                o.call(m, &callee::SI_PATCH_TRANSFORM, &[d, matrix]);
            }
            2 => {
                o.call(m, &callee::SI_FACE_TRANSFORM, &[d, matrix]);
            }
            4 => {
                o.call(m, &callee::SI_MESH_TRANSFORM, &[d, matrix, normals]);
            }
            6 => {
                o.call(m, &callee::SI_SPLINE_TRANSFORM, &[d, matrix]);
            }
            8 => mx::multiply(m, d.wrapping_add(0xc), matrix, d.wrapping_add(0xc)),
            9 => {
                o.call(m, &callee::SI_NURBS_CRV_APPLY_MATRIX, &[d, matrix, 0]);
            }
            10 => {
                o.call(m, &callee::SI_NURBS_SRF_APPLY_MATRIX, &[d, matrix, 0]);
            }
            _ => {}
        }
    }
    if m[node].bbox_valid != 0 {
        bounds::compute_bounding(m, node);
    }
    touch_geometry(m, o, node);
    m.release(4);
    m.release(0x40);
}
