// SPDX-License-Identifier: MIT
//! A node's transformation on the image: the update that derives the global values from
//! the local ones, and the routines of `model.c` around it.
//!
//! The arithmetic is that of [`crate::transform`] and [`crate::setters`] (order of the
//! operations and where a value is rounded were read from the instruction listing and are
//! said there); what differs is that the record is the original's, in the image.

use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::x;

use super::{at, callee, g, mx, off, Outside};
use crate::transform::{floor_scaling, mul};

/// Bit 15 of the flag word at +0x08: the node is inside [`update_son_state`].
const BUSY: u8 = 0x80;

/// `((y·m[1][j] + x·m[0][j]) + z·m[2][j]) + m[3][j]` for j = 0, 1, 2, each formed in the
/// register and rounded once on the store: the point at `p` through the matrix at `mat`,
/// stored at `out`. The point's three components are loaded before the first store, as in
/// the original, so `out` may be `p`; the matrix elements are read as each sum is formed.
pub(crate) fn point_through(m: &mut Mem, p: u32, mat: u32, out: u32) {
    let (px, py, pz) = (m.f32(p), m.f32(p + 4), m.f32(p + 8));
    for j in 0..3u32 {
        let e = |m: &Mem, i: u32| m.f32(mat + 16 * i + 4 * j);
        let v = ((x(py) * e(m, 1) + x(px) * e(m, 0)) + x(pz) * e(m, 2)) + e(m, 3);
        m.set_f32(out + 4 * j, v.to_f32());
    }
}

/// Rows 0 to 2 of the matrix at `rot`, each times one factor of the vector at `s`, into
/// the first three columns of the matrix at `out` (which may be `rot`): one product per
/// element, rounded on its store.
fn scaled_rows(m: &mut Mem, rot: u32, s: u32, out: u32) {
    for i in 0..3u32 {
        let f = m.f32(s + 4 * i);
        for j in 0..2u32 {
            let v = mul(m.f32(rot + 16 * i + 4 * j), f);
            m.set_f32(out + 16 * i + 4 * j, v);
        }
        // the third column has the factor as the first operand
        let v = mul(f, m.f32(rot + 16 * i + 8));
        m.set_f32(out + 16 * i + 8, v);
    }
}

/// The node is an effector (algorithm 3) under a chain root whose inverse kinematics are
/// of kind 1. The original reads through the node's `father` link and that node's
/// `inv_kin` pointer without testing either; so does this (a null one is a recorded fault).
fn effector_keeps_orientation(m: &Mem, node: Ptr32<Model>) -> bool {
    if m[node].algo_type != 3 {
        return false;
    }
    let ik = m[m[node].father].inv_kin;
    m.i32(ik.addr() + off::INV_KIN_KIND) == 1
}

/// A node of a skeleton chain as the update tests it: a null (kind 0) with algorithm 1 to
/// 4, or a joint (kind 5).
pub(crate) fn chain_node(m: &Mem, n: Ptr32<Model>) -> bool {
    let kind = m[n].kind;
    let algo = m[n].algo_type;
    (kind == 0 && (algo.wrapping_sub(1) as u32) <= 1) || kind == 5 || (kind == 0 && (algo.wrapping_sub(3) as u32) <= 1)
}

/// The three scaling factors of a node, each one smaller than 1e-5 replaced by ±1e-5.
fn clamp_scaling(m: &mut Mem, node: Ptr32<Model>) {
    for a in [at!(node, scal), at!(node, scal) + 4, at!(node, scal) + 8] {
        let c = m.f32(a);
        let f = floor_scaling(c);
        if f.to_bits() != c.to_bits() {
            m.set_f32(a, f);
        }
    }
}

/// A node's local scaling, rotation angles and translation copied out
/// [soft 0x08463774 SI_modelState]: `SI_modelState(node, scaling, angles, translation)`
/// copies +0x100..+0x108 to `scaling`, +0x10c..+0x114 to `angles` and +0x158..+0x160 to
/// `translation`, word by word in that order (integer moves: every bit pattern is kept, and
/// an output may overlap the node, as `si_waveComputeVectors`' angles do).
pub fn model_state(m: &mut Mem, node: u32, scaling: u32, angles: u32, translation: u32) {
    for (from, to) in [(0x100u32, scaling), (0x10c, angles), (0x158, translation)] {
        for k in 0..3u32 {
            let v = m.u32(node.wrapping_add(from + 4 * k));
            m.set_u32(to.wrapping_add(4 * k), v);
        }
    }
}

/// The classical method's bookkeeping for one node; with `recurse`, then for its following
/// brothers and its subtree [soft 0x081ca134 SI_modelUpdateCScalingInfo] (`classic.c`; the
/// update cannot do without it). What it computes is said at
/// [`crate::transform::update_cscaling_info`].
pub fn update_cscaling_info(m: &mut Mem, father: Ptr32<Model>, node: Ptr32<Model>, recurse: i16) {
    if father.is_null() {
        m[node].hscl = si_types::geom::Vector { x: 1.0, y: 1.0, z: 1.0 };
        mx::copy(m, g::SI_IDENTITE.addr(), at!(node, hrot));
        m[node].htrs.z = 0.0;
        m[node].htrs.y = 0.0;
        m[node].htrs.x = 0.0;
        let t = m[node].trans;
        m[node].unk_3ac = t;
    } else {
        let v = m[father].cscl;
        m[node].hscl = v;
        mx::copy(m, at!(father, grot), at!(node, hrot));
        let v = m[father].unk_3ac;
        m[node].htrs = v;
        point_through(m, at!(node, trans), at!(father, cscl_matrix), at!(node, unk_3ac));
    }
    mx::xyz_rotation(m, at!(node, vrot), at!(node, mrot));
    for i in 0..3u32 {
        let v = mul(m.f32(at!(node, hscl) + 4 * i), m.f32(at!(node, scal) + 4 * i));
        m.set_f32(at!(node, cscl) + 4 * i, v);
    }
    if effector_keeps_orientation(m, node) {
        mx::copy(m, at!(node, mrot), at!(node, grot));
    } else {
        mx::multiply_sub3(m, at!(node, mrot), at!(node, hrot), at!(node, grot));
    }
    for i in 0..3 {
        for j in 0..3 {
            let v = m[node].grot[i][j];
            m[node].cscl_matrix[i][j] = v;
        }
    }
    let p = m[node].unk_3ac;
    m[node].cscl_matrix[3][0] = p.x;
    m[node].cscl_matrix[3][1] = p.y;
    m[node].cscl_matrix[3][2] = p.z;
    if recurse != 0 {
        let brother = m[node].brother;
        if !brother.is_null() {
            update_cscaling_info(m, father, brother, 1);
        }
        let son = m[node].son;
        if !son.is_null() {
            update_cscaling_info(m, node, son, 1);
        }
    }
}

/// Makes an effector keep its own orientation under the classical scaling method
/// [soft 0x084661ec toggleEffRotConstraint]: `gscl` becomes the row lengths of `gmat`,
/// `grot` a copy of `mrot`, `gmat` a copy of `grot` with its rows times those lengths and
/// `gtrs` in row 3.
pub fn toggle_eff_rot_constraint(m: &mut Mem, node: Ptr32<Model>) {
    mx::extract_scaling(m, at!(node, gmat), at!(node, gscl));
    mx::copy(m, at!(node, mrot), at!(node, grot));
    mx::copy(m, at!(node, grot), at!(node, gmat));
    scaled_rows(m, at!(node, gmat), at!(node, gscl), at!(node, gmat));
    let t = m[node].gtrs;
    m[node].gmat[3][0] = t.x;
    m[node].gmat[3][1] = t.y;
    m[node].gmat[3][2] = t.z;
}

/// Recomputes a node's global values from its local ones and from the global values of
/// `father` (null: a root); with `recurse`, then for its following brothers and its
/// subtree [soft 0x084662f8 SI_modelUpdateSonState]. The two methods, the mirror flag,
/// `def_matrix` and the order of the walk are described at
/// [`crate::transform::update_son_state`]; this is the same routine on the image.
///
/// The 4 × 4 local of the original (the node's local matrix by the classical method, then
/// the inverse of the father's matrix) is a block on the image's stack. When the father's
/// matrix has no inverse, `SI_matrixInverse` stores nothing and the product is formed with
/// what the block holds: the local matrix by the classical method, as in the original;
/// by the usual method whatever the image's stack has there, which is not what the
/// original's stack has (the use is counted, [`crate::env::stale`]).
pub fn update_son_state(m: &mut Mem, o: &mut dyn Outside, father: Ptr32<Model>, node: Ptr32<Model>, recurse: i16) {
    let mut hold: i16 = 0;
    m.or_u8(node.addr() + 9, BUSY);
    if m[node].kind == 8 {
        let ball = m[node].definition.addr();
        let system = o.call(m, &callee::SI_METABALL_GET_PARENT_SYSTEM, &[ball]);
        if system != 0 && m.i16(system + off::META_SYSTEM_FLAGS) >= 0 {
            o.call(m, &callee::SI_METABALL_TOUCH, &[ball, 0]);
        }
    }
    let local = m.local(0x40);
    let old;
    let new;
    if m[g::SI_GLB_MANIP].scaling_method == 1 {
        old = m[node].cscl;
        update_cscaling_info(m, father, node, 0);
        new = m[node].cscl;
        m[node].hscl = si_types::geom::Vector { x: 1.0, y: 1.0, z: 1.0 };
        if father.is_null() {
            mx::copy(m, g::SI_IDENTITE.addr(), at!(node, hrot));
            m[node].htrs.z = 0.0;
            m[node].htrs.y = 0.0;
            m[node].htrs.x = 0.0;
        } else {
            mx::copy(m, at!(father, grot), at!(node, hrot));
            let v = m[father].gtrs;
            m[node].htrs = v;
        }
        mx::xyz_rotation(m, at!(node, vrot), at!(node, mrot));
        mx::multiply_sub3(m, at!(node, mrot), at!(node, hrot), at!(node, grot));
        clamp_scaling(m, node);
        // the local matrix: rows of mrot times the scaling factors, the translation, and a
        // last column of (0, 0, 0, 1)
        scaled_rows(m, at!(node, mrot), at!(node, scal), local);
        for i in 0..3u32 {
            m.set_u32(local + 16 * i + 12, 0);
        }
        let t = m[node].trans;
        m.set_f32(local + 0x30, t.x);
        m.set_f32(local + 0x34, t.y);
        m.set_f32(local + 0x38, t.z);
        m.set_f32(local + 0x3c, 1.0);
        if father.is_null() {
            mx::copy(m, local, at!(node, gmat));
        } else {
            mx::multiply(m, local, at!(father, gmat), at!(node, gmat));
        }
        let row = m[node].gmat[3];
        m[node].gtrs = si_types::geom::Vector { x: row[0], y: row[1], z: row[2] };
        if effector_keeps_orientation(m, node) {
            toggle_eff_rot_constraint(m, node);
        }
        if !father.is_null() && !chain_node(m, father) && !chain_node(m, node) && m.u8(node.addr() + 9) & 0x20 != 0 {
            o.call(m, &callee::SI_MODEL_COMPENSATE_LOCAL_COORD, &[father.addr(), node.addr()]);
        }
    } else {
        if father.is_null() {
            m[node].hscl = si_types::geom::Vector { x: 1.0, y: 1.0, z: 1.0 };
            mx::copy(m, g::SI_IDENTITE.addr(), at!(node, hrot));
            m[node].htrs.z = 0.0;
            m[node].htrs.y = 0.0;
            m[node].htrs.x = 0.0;
        } else {
            let v = m[father].gscl;
            m[node].hscl = v;
            mx::copy(m, at!(father, grot), at!(node, hrot));
            let v = m[father].gtrs;
            m[node].htrs = v;
        }
        mx::xyz_rotation(m, at!(node, vrot), at!(node, mrot));
        clamp_scaling(m, node);
        old = m[node].gscl;
        for i in 0..3u32 {
            let v = mul(m.f32(at!(node, hscl) + 4 * i), m.f32(at!(node, scal) + 4 * i));
            m.set_f32(at!(node, gscl) + 4 * i, v);
        }
        new = m[node].gscl;
        if effector_keeps_orientation(m, node) {
            mx::copy(m, at!(node, mrot), at!(node, grot));
        } else {
            mx::multiply_sub3(m, at!(node, mrot), at!(node, hrot), at!(node, grot));
        }
        if father.is_null() {
            let t = m[node].trans;
            m[node].gtrs = t;
        } else {
            point_through(m, at!(node, trans), at!(father, gmat), at!(node, gtrs));
        }
        scaled_rows(m, at!(node, grot), at!(node, gscl), at!(node, gmat));
        let t = m[node].gtrs;
        m[node].gmat[3][0] = t.x;
        m[node].gmat[3][1] = t.y;
        m[node].gmat[3][2] = t.z;
    }
    // mirrored since the last update: the six factors multiplied in the register
    let flags = m[node].update_flags2;
    if flags & 0x10 == 0 {
        let product = ((((x(old.x) * new.x) * old.y) * new.y) * old.z) * new.z;
        if product.lt(x(0.0f32)) {
            m[node].update_flags2 = flags | 0x10;
        }
    }
    if m.u8(g::SI_VIEWER_UPD_OPT.addr()) & 0x10 == 0 {
        if father.is_null() {
            mx::copy(m, at!(node, gmat), at!(node, def_matrix));
        } else {
            if !mx::inverse(m, at!(father, gmat), local) && m[g::SI_GLB_MANIP].scaling_method != 1 {
                crate::env::stale::note();
            }
            mx::multiply(m, at!(node, gmat), local, at!(node, def_matrix));
        }
    }
    if m[node].algo_type != 0x1e {
        let holder = m[node].latdep;
        if holder != node && m.i16(holder.addr() + 8) >= 0 {
            let envelope = m[node].envelop;
            if (envelope.wrapping_sub(5) as u32) <= 1 || envelope == 8 {
                hold = m[g::SI_GLB_ANIM][3] as i16;
            }
            if hold == 0 {
                o.call(m, &callee::SI_MODEL_READJUST_DEFINITION, &[node.addr()]);
            }
        }
    }
    if recurse != 0 {
        if m.u8(g::SI_VIEWER_UPD_OPT.addr()) & 2 != 0 {
            let mut son = m[node].son;
            while !son.is_null() {
                update_son_state(m, o, node, son, 1);
                son = m[son].brother;
            }
        } else {
            let brother = m[node].brother;
            if !brother.is_null() {
                update_son_state(m, o, father, brother, 1);
            }
            let son = m[node].son;
            if !son.is_null() {
                update_son_state(m, o, node, son, 1);
            }
        }
    }
    m.release(0x40);
    let f = m[node].flags_08;
    m[node].flags_08 = f & 0x7fff;
}

/// Gives `to` the place in space `from` has [soft 0x08461f98 SI_modelCopyTransformation].
///
/// `from`'s global scaling, rotation and position are formed in locals: its local values
/// for a root; else scaling × the father's `gscl`, `mrot` × the father's `grot` (the
/// angles taken out of that matrix with zeros as the previous solution), and the
/// translation through the father's `gmat`. For a root `to` they become its local values
/// (and the rotation matrix its `mrot`); under a father: scaling ÷ the father's `gscl`,
/// the rotation × the inverse of the father's `grot` into `mrot` with the angles taken
/// out of it against `to`'s present ones, and the position through the inverse of the
/// father's `gmat`. Then the update of `to` and what hangs below it.
///
/// Both inverses go into one block of the stack that nothing fills first: when a matrix
/// has no inverse the original computes with what was there (counted, as in the update).
pub fn copy_transformation(m: &mut Mem, o: &mut dyn Outside, from: Ptr32<Model>, to: Ptr32<Model>) {
    let scl = m.local(12);
    let rot = m.local(12);
    let pos = m.local(12);
    let matrix = m.local(0x40);
    let inverse = m.local(0x40);
    let father = m[from].father;
    if father.is_null() {
        m.copy(scl, at!(from, scal), 12);
        m.copy(rot, at!(from, vrot), 12);
        mx::copy(m, at!(from, mrot), matrix);
        m.copy(pos, at!(from, trans), 12);
    } else {
        for i in 0..3u32 {
            let v = mul(m.f32(at!(from, scal) + 4 * i), m.f32(at!(father, gscl) + 4 * i));
            m.set_f32(scl + 4 * i, v);
        }
        mx::multiply_sub3(m, at!(from, mrot), at!(father, grot), matrix);
        m.set_u32(rot + 8, 0);
        m.set_u32(rot + 4, 0);
        m.set_u32(rot, 0);
        mx::extract_xyz_rotation(m, matrix, rot);
        point_through(m, at!(from, trans), at!(father, gmat), pos);
    }
    let father = m[to].father;
    if father.is_null() {
        m.copy(at!(to, scal), scl, 12);
        m.copy(at!(to, vrot), rot, 12);
        mx::copy(m, matrix, at!(to, mrot));
        m.copy(at!(to, trans), pos, 12);
    } else {
        for i in 0..3u32 {
            let v = (x(m.f32(scl + 4 * i)) / m.f32(at!(father, gscl) + 4 * i)).to_f32();
            m.set_f32(at!(to, scal) + 4 * i, v);
        }
        if !mx::inverse(m, at!(father, grot), inverse) {
            crate::env::stale::note();
        }
        mx::multiply_sub3(m, matrix, inverse, at!(to, mrot));
        mx::extract_xyz_rotation(m, at!(to, mrot), at!(to, vrot));
        if !mx::inverse(m, at!(father, gmat), inverse) {
            crate::env::stale::note();
        }
        point_through(m, pos, inverse, at!(to, trans));
    }
    let father = m[to].father;
    update_son_state(m, o, father, to, 1);
    m.release(12 * 3 + 0x80);
}

/// Makes a node's local values those that keep it where it is under a new father
/// [soft 0x08473574 SI_modelCompensateParentingMatrix] (the node is not linked yet).
///
/// By the usual method: scaling ÷ the father's `gscl`, `mrot` × the inverse of the
/// father's `grot`. By the classical method the father's global matrix is taken apart
/// (`SI_matrixExtractTransformations`, with the father's local angles as the previous
/// solution): scaling ÷ that scaling, `mrot` × the inverse of the rotation by those
/// angles. Either way the angles are then taken out of the new `mrot` and the
/// translation goes through the inverse of the father's `gmat`. With
/// `SI_initPoseCompensate` set, the initial poses are prepared before
/// (`computeInit_srtParenting(node)`) and compensated after
/// (`compensateInit_srt(father, node)`).
///
/// The inverses share one block of the stack; one that does not exist leaves it as it
/// was (counted, as in the update).
pub fn compensate_parenting_matrix(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, father: Ptr32<Model>) {
    let scl = m.local(12);
    let rot = m.local(12);
    let trs = m.local(12);
    let matrix = m.local(0x40);
    let inverse = m.local(0x40);
    if m[g::SI_INIT_POSE_COMPENSATE] != 0 {
        o.call(m, &callee::COMPUTE_INIT_SRT_PARENTING, &[node.addr()]);
    }
    if m[g::SI_GLB_MANIP].scaling_method == 1 {
        m.copy(rot, at!(father, vrot), 12);
        mx::extract_transformations(m, at!(father, gmat), scl, rot, trs);
        for i in 0..3u32 {
            let v = (x(m.f32(at!(node, scal) + 4 * i)) / m.f32(scl + 4 * i)).to_f32();
            m.set_f32(at!(node, scal) + 4 * i, v);
        }
        mx::xyz_rotation(m, rot, matrix);
        if !mx::inverse(m, matrix, inverse) {
            crate::env::stale::note();
        }
    } else {
        for i in 0..3u32 {
            let v = (x(m.f32(at!(node, scal) + 4 * i)) / m.f32(at!(father, gscl) + 4 * i)).to_f32();
            m.set_f32(at!(node, scal) + 4 * i, v);
        }
        if !mx::inverse(m, at!(father, grot), inverse) {
            crate::env::stale::note();
        }
    }
    mx::multiply_sub3(m, at!(node, mrot), inverse, at!(node, mrot));
    mx::extract_xyz_rotation(m, at!(node, mrot), at!(node, vrot));
    if !mx::inverse(m, at!(father, gmat), inverse) {
        crate::env::stale::note();
    }
    point_through(m, at!(node, trans), inverse, at!(node, trans));
    if m[g::SI_INIT_POSE_COMPENSATE] != 0 {
        o.call(m, &callee::COMPENSATE_INIT_SRT, &[father.addr(), node.addr()]);
    }
    m.release(12 * 3 + 0x80);
}

/// Makes a node's local values its global ones, for a node about to lose its father
/// [soft 0x084737a0 SI_modelCompensateCuttingMatrix]. Usual method: scaling × the father's
/// `gscl`, `mrot` × the father's `grot` with the angles taken out of it, the translation
/// through the father's `gmat`. Classical method: scaling, angles and translation taken
/// out of the node's own `gmat` (the present angles as the previous solution), and
/// `mrot` rebuilt from the angles.
pub fn compensate_cutting_matrix(m: &mut Mem, node: Ptr32<Model>, father: Ptr32<Model>) {
    if m[g::SI_GLB_MANIP].scaling_method == 1 {
        mx::extract_transformations(m, at!(node, gmat), at!(node, scal), at!(node, vrot), at!(node, trans));
        mx::xyz_rotation(m, at!(node, vrot), at!(node, mrot));
    } else {
        for i in 0..3u32 {
            let v = mul(m.f32(at!(node, scal) + 4 * i), m.f32(at!(father, gscl) + 4 * i));
            m.set_f32(at!(node, scal) + 4 * i, v);
        }
        mx::multiply_sub3(m, at!(node, mrot), at!(father, grot), at!(node, mrot));
        mx::extract_xyz_rotation(m, at!(node, mrot), at!(node, vrot));
        point_through(m, at!(node, trans), at!(father, gmat), at!(node, trans));
    }
}

/// Sets the translation of a rotation matrix so that it turns about a point
/// [soft 0x08463e20 SI_correctTransOfRefMatrix]: row 3 becomes p − p × R, written out per
/// component as `((−p.x·m[0][j] − p.y·m[1][j]) − p.z·m[2][j]) + p[j]`, formed in the
/// register and rounded once. The point is passed by value.
pub fn correct_trans_of_ref_matrix(m: &mut Mem, p: [f32; 3], matrix: u32) {
    let e = |m: &Mem, i: u32, j: u32| m.f32(matrix + 16 * i + 4 * j);
    let nx = -x(p[0]);
    let v = x(p[0]) + ((nx * e(m, 0, 0) - x(p[1]) * e(m, 1, 0)) - x(p[2]) * e(m, 2, 0));
    m.set_f32(matrix + 0x30, v.to_f32());
    let v = ((nx * e(m, 0, 1) - x(p[1]) * e(m, 1, 1)) - x(p[2]) * e(m, 2, 1)) + p[1];
    m.set_f32(matrix + 0x34, v.to_f32());
    let v = x(p[2]) + ((nx * e(m, 0, 2) - x(p[1]) * e(m, 1, 2)) - x(p[2]) * e(m, 2, 2));
    m.set_f32(matrix + 0x38, v.to_f32());
}

/// The rotation three angles mean in the current rotation mode, as a matrix that acts in
/// the space of `frame` [soft 0x08463e9c SI_matrixRotationReferTag]: `frame` × R ×
/// `frame`⁻¹ at `out`, R being `SI_matrixRotationRefer(view, angles)`, turned about the
/// reference object's global position in mode 3 and about the view's interest (three
/// floats at +0x1c of the record at +0x2c of the `__SI_Grafport`: named offsets) in mode 4
/// ([`correct_trans_of_ref_matrix`]). The inverse is taken first, into a block of the
/// stack nothing fills: a `frame` without an inverse is counted as in the update.
pub fn matrix_rotation_refer_tag(m: &mut Mem, o: &mut dyn Outside, view: u32, angles: u32, frame: u32, out: u32) {
    let inverse = m.local(0x40);
    if !mx::inverse(m, frame, inverse) {
        crate::env::stale::note();
    }
    o.call(m, &callee::SI_MATRIX_ROTATION_REFER, &[view, angles, out]);
    let mode = m[g::SI_GLB_MANIP].rot_mode;
    if mode == 3 || mode == 4 {
        let p = if mode == 3 {
            let model = m.u32(m[g::SI_GLB_MANIP].reference.addr() + 4);
            model + 0x1b8
        } else {
            m.u32(view + 0x2c) + 0x1c
        };
        let p = [m.f32(p), m.f32(p + 4), m.f32(p + 8)];
        correct_trans_of_ref_matrix(m, p, out);
    }
    mx::multiply(m, frame, out, out);
    mx::multiply(m, out, inverse, out);
    m.release(0x40);
}

/// A translation as the current translation mode means it, for the points of a node
/// [soft 0x08463fd4 SI_matrixTranslationReferTag]; the answer is stored at `out` (the
/// original returns the vector through that hidden argument and answers its address).
///
/// - mode 0: the vector as it is;
/// - modes 1 and 4: the vector through `frame` (`SI_vectorMultiplyMatrix`);
/// - mode 2: as mode 0 without a node; with one, through the node's `gmat` × `frame`,
///   the node's position left out of `gmat` unless `keep_position`;
/// - mode 3: the same with the reference object in the node's place;
/// - any other mode: what the stack held (counted; zeros of the image's stack here).
pub fn matrix_translation_refer_tag(m: &mut Mem, out: u32, node: Ptr32<Model>, vector: [f32; 3], frame: u32, keep_position: i16) -> u32 {
    let v = m.local(12);
    let result = m.local(12);
    let work = m.local(0x40);
    let global = m.local(0x40);
    for i in 0..3 {
        m.set_f32(v + 4 * i as u32, vector[i]);
    }
    mx::copy(m, frame, work);
    let mode = m[g::SI_GLB_MANIP].trans_mode;
    let through: Option<Ptr32<Model>> = match mode {
        2 if !node.is_null() => Some(node),
        3 => Some(Ptr32::new(m.u32(m[g::SI_GLB_MANIP].reference.addr() + 4))),
        _ => None,
    };
    if let Some(model) = through {
        mx::copy(m, at!(model, gmat), global);
        if keep_position == 0 {
            m.set_u32(global + 0x38, 0);
            m.set_u32(global + 0x34, 0);
            m.set_u32(global + 0x30, 0);
        }
        mx::multiply(m, global, work, work);
        mx::vector_multiply_matrix(m, v, work, result);
    } else {
        match mode {
            0 | 2 => m.copy(result, v, 12),
            1 | 4 => mx::vector_multiply_matrix(m, v, work, result),
            _ => crate::env::stale::note(),
        }
    }
    m.copy(out, result, 12);
    m.release(24 + 0x80);
    out
}

/// Turns a node by three angles meant in the current rotation mode, keeping its global
/// scaling and position [soft 0x08463d5c SI_modelRotationRefMatrixSet]: the rotation
/// (`SI_matrixRotationRefer(view, angles)`) with its rows times the node's `gscl` and the
/// node's `gtrs` in row 3 is handed to
/// `SI_modelTransformationMatrix(node, matrix, 0, select_mode, flag, 0)`.
pub fn rotation_ref_matrix_set(m: &mut Mem, o: &mut dyn Outside, view: u32, node: Ptr32<Model>, angles: u32, select_mode: u32, flag: i16) {
    let matrix = m.local(0x40);
    o.call(m, &callee::SI_MATRIX_ROTATION_REFER, &[view, angles, matrix]);
    scaled_rows(m, matrix, at!(node, gscl), matrix);
    m.copy(matrix + 0x30, at!(node, gtrs), 12);
    o.call(m, &callee::SI_MODEL_TRANSFORMATION_MATRIX, &[node.addr(), matrix, 0, select_mode, flag as i32 as u32, 0]);
    m.release(0x40);
}
