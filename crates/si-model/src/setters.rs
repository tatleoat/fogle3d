// SPDX-License-Identifier: MIT
//! The setters of a node's transformation beyond the plain three of [`crate::transform`]:
//! in global terms, as an addition, in the "safe" forms the tools call, from a matrix; and
//! what a change of a node does to its sons and its textures (compensation). From the
//! original's `model.c` (finding 0015, "Setting a transformation").
//!
//! Every setter ends the same way: it stores local values, sets the update flag bits,
//! calls [`compensate_matrix`] with the caller's two arguments and then
//! [`update_son_state`] on the node, its following brothers and its subtree.
//!
//! The two arguments every setter passes on: `mode` (the demangled `SI_SelectMode`; these
//! routines only compare it with 0 and 1) and `geom` (a 16-bit flag). In
//! [`compensate_matrix`] mode 0 means "the sons stay where they are" and a non-zero `geom`
//! "the geometry stays where it is".
//!
//! Exactness classes as in `si-math`; rotations go through the C library (class libm).

use si_math::algebra::{
    matrix_axis_rotation, matrix_extract_scaling, matrix_extract_transformations, matrix_extract_transformations2, matrix_extract_xyz_rotation,
    matrix_inverse, matrix_multiply, matrix_multiply_sub3, matrix_scal_rot_trans, matrix_scaling, matrix_transpose, matrix_xyz_rotation,
};
use si_math::types::{Matrix, Vector as MVector, IDENTITY};
use si_types::geom::Vector;
use si_types::Ptr32;
use si_x87::x;

use crate::env::Env;
use crate::node::{mv, tv, Models, NodeId, MBALL, MJNT};
use crate::transform::{floor_scaling, is_chain_node, mul, point_through, set_rotation, set_scaling, set_translation, update_son_state, Globals};
use crate::transform::{UPD_ROTATION, UPD_SCALING, UPD_TRANSLATION};

const ZERO: Matrix = [[0.0; 4]; 4];

fn div(a: f32, b: f32) -> f32 {
    (x(a) / b).to_f32()
}

fn add(a: f32, b: f32) -> f32 {
    (x(a) + b).to_f32()
}

fn vadd(a: Vector, b: Vector) -> Vector {
    Vector { x: add(a.x, b.x), y: add(a.y, b.y), z: add(a.z, b.z) }
}

fn flag(models: &mut Models, node: NodeId, bits: u16) {
    let m = models.m_mut(node);
    m.update_flags |= bits;
    m.update_flags2 |= bits;
}

/// A vector divided, component by component, by scaling factors that are floored first at
/// ±1e-5, as the update floors them ([`floor_scaling`]).
fn over_scaling(v: Vector, s: Vector) -> Vector {
    Vector { x: div(v.x, floor_scaling(s.x)), y: div(v.y, floor_scaling(s.y)), z: div(v.z, floor_scaling(s.z)) }
}

/// The compensation routines floor ten times higher: a divisor smaller than the double
/// 0.0001 (0x0865fd10, 0x0865fd20) is taken as the float nearest ±0.0001 (0x0865fd18,
/// 0x0865fd1c), with its sign (zero counts as positive).
pub(crate) fn floor_compensation(c: f32) -> f32 {
    const FLOOR: f32 = 9.999_999_747_378_752e-5;
    if x(c).abs().lt(x(0.0001f64)) {
        if c >= 0.0 { FLOOR } else { -FLOOR }
    } else {
        c
    }
}

fn over_compensation(v: Vector, s: Vector) -> Vector {
    Vector { x: div(v.x, floor_compensation(s.x)), y: div(v.y, floor_compensation(s.y)), z: div(v.z, floor_compensation(s.z)) }
}

/// `SI_matrixInverse` writes nothing for a matrix without an inverse, and these routines go
/// on with whatever their stack slot held. Here that is `stale`.
fn inverse_or(m: &Matrix, stale: Matrix) -> Matrix {
    matrix_inverse(m).unwrap_or_else(|| {
        crate::env::stale::note();
        stale
    })
}

fn finish(models: &mut Models, node: NodeId, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    compensate_matrix(models, node, mode, geom, g, env);
    let father = models.father(node);
    update_son_state(models, father, node, true, g, env);
}

// ------------------------------------------------------------------ in global terms

/// Puts a node at a position given in global terms
/// [soft 0x0846412c SI_modelSetGlobalTranslation]: the local translation becomes the
/// position through the inverse of the father's global matrix (the position itself for a
/// root); flag bit 0x8. Class: model.
pub fn set_global_translation(models: &mut Models, node: NodeId, pos: Vector, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    let t = match models.father(node) {
        None => pos,
        Some(f) => point_through(pos, &inverse_or(&models.m(f).gmat, ZERO)),
    };
    models.m_mut(node).trans = t;
    flag(models, node, UPD_TRANSLATION);
    finish(models, node, mode, geom, g, env);
}

/// Gives a node an orientation given in global terms
/// [soft 0x08464224 SI_modelSetGlobalRotation]: the local angles are those of
/// R(angles) × (father's `grot`)ᵀ, taken out with the node's present angles as the previous
/// solution (the angles themselves for a root); flag bit 0x4. The product is the full
/// 4 × 4 one. Unlike [`set_rotation`] this compensates a joint too. Class: libm.
pub fn set_global_rotation(models: &mut Models, node: NodeId, rot: Vector, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    match models.father(node) {
        None => models.m_mut(node).vrot = rot,
        Some(f) => {
            let t = matrix_transpose(&models.m(f).grot);
            let p = matrix_multiply(&matrix_xyz_rotation(&mv(rot)), &t);
            let prev = mv(models.m(node).vrot);
            models.m_mut(node).vrot = tv(matrix_extract_xyz_rotation(&p, &prev, g.keep_turns));
        }
    }
    flag(models, node, UPD_ROTATION);
    finish(models, node, mode, geom, g, env);
}

/// Gives a node a scaling given in global terms [soft 0x084642fc SI_modelSetGlobalScaling];
/// flag bit 0x2. For a root the factors are stored as they are. Under a father:
///
/// - usual method: each factor divided by the node's `hscl` (the father's global scaling as
///   copied at the last update), a divisor smaller than 1e-5 taken as ±1e-5;
/// - classical method: the row lengths of S(factors) × (father's `gmat`)⁻¹, which are
///   always positive.
///
/// Class: single (usual), model (classical).
pub fn set_global_scaling(models: &mut Models, node: NodeId, scal: Vector, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    let s = match models.father(node) {
        None => scal,
        Some(f) if g.classical_scaling => {
            let inv = inverse_or(&models.m(f).gmat, ZERO);
            tv(matrix_extract_scaling(&matrix_multiply(&matrix_scaling(&mv(scal)), &inv)))
        }
        Some(_) => over_scaling(scal, models.m(node).hscl),
    };
    models.m_mut(node).scal = s;
    flag(models, node, UPD_SCALING);
    finish(models, node, mode, geom, g, env);
}

// ------------------------------------------------------------------ additions

/// Multiplies the local scaling by factors [soft 0x084644ec SI_modelAddScaling]; flag bit
/// 0x2. Class: single.
pub fn add_scaling(models: &mut Models, node: NodeId, factors: Vector, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    let m = models.m_mut(node);
    m.scal = Vector { x: mul(factors.x, m.scal.x), y: mul(factors.y, m.scal.y), z: mul(factors.z, m.scal.z) };
    flag(models, node, UPD_SCALING);
    finish(models, node, mode, geom, g, env);
}

/// The line of sight of a view, as [`matrix_rotation_refer`] reads it through its first
/// argument (`__SI_Grafport` + 0x2c, the view's camera): the three floats at +0x10 and the
/// three at +0x1c of that record (the camera's position and its interest; named, finding
/// 0005).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub position: Vector,
    pub interest: Vector,
}

/// The rotation a triple of angles means in the current rotation mode
/// [soft 0x08463ad4 SI_matrixRotationRefer]: three rotations about three axes, multiplied
/// in the order first × second × third ([`matrix_multiply_sub3`]).
///
/// - mode 1: about the world X, Y and Z axes (`SI_xAxis`, `SI_yAxis`, `SI_zAxis`);
/// - mode 3: about the three rows of the reference object's `grot`;
/// - mode 4: all three about the line of sight, position − interest of the view's camera,
///   divided by its length when that exceeds 0.0001. The difference is kept in the register:
///   the length (√((y² + x²) + z²), rounded to float) and the quotients are formed from the
///   unrounded components.
///
/// In any other mode the original multiplies three matrices it never set (stack memory);
/// here they are identities. The same holds for mode 3 without a reference object and mode
/// 4 without a view, where the original reads through a null pointer. Class: libm.
pub fn matrix_rotation_refer(models: &Models, view: Option<&View>, angles: Vector, g: &Globals) -> Matrix {
    let axes: Option<[MVector; 3]> = match g.rot_mode {
        1 => Some([MVector::new(1.0, 0.0, 0.0), MVector::new(0.0, 1.0, 0.0), MVector::new(0.0, 0.0, 1.0)]),
        3 => g.ref_model.map(|r| {
            let rot = &models.m(r).grot;
            [MVector::new(rot[0][0], rot[0][1], rot[0][2]), MVector::new(rot[1][0], rot[1][1], rot[1][2]), MVector::new(rot[2][0], rot[2][1], rot[2][2])]
        }),
        4 => view.map(|v| {
            let (dx, dy, dz) = (x(v.position.x) - v.interest.x, x(v.position.y) - v.interest.y, x(v.position.z) - v.interest.z);
            let len = ((dy * dy + dx * dx) + dz * dz).sqrt().to_f32();
            let axis = if x(len).abs().gt(x(0.0001f64)) {
                MVector::new((dx / len).to_f32(), (dy / len).to_f32(), (dz / len).to_f32())
            } else {
                MVector::new(dx.to_f32(), dy.to_f32(), dz.to_f32())
            };
            [axis, axis, axis]
        }),
        _ => None,
    };
    let (a, b, c) = match axes {
        Some(ax) => (matrix_axis_rotation(angles.x, &ax[0]), matrix_axis_rotation(angles.y, &ax[1]), matrix_axis_rotation(angles.z, &ax[2])),
        None => (IDENTITY, IDENTITY, IDENTITY),
    };
    matrix_multiply_sub3(&matrix_multiply_sub3(&a, &b), &c)
}

/// Turns a node by three angles, meant as the rotation mode says
/// [soft 0x0846455c SI_modelAddRotation].
///
/// - modes 1, 3 and 4 (world, reference object, view): the new local rotation is
///   `grot` × [`matrix_rotation_refer`] × (father's `grot`)ᵀ, and the angles are taken out of
///   it with the present angles as the previous solution. **No update flag bit is set** on
///   this path.
/// - mode 2: the angles are added to the local angles; flag bit 0x4.
/// - any other mode: R(angles) × `mrot`, angles taken out likewise; flag bit 0x4.
///
/// Then the compensation unless the node is a joint, and the update. Class: libm.
pub fn add_rotation(models: &mut Models, view: Option<&View>, node: NodeId, angles: Vector, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    let new: Option<Matrix> = if matches!(g.rot_mode, 1 | 3 | 4) {
        let mut r = matrix_rotation_refer(models, view, angles, g);
        r = matrix_multiply_sub3(&models.m(node).grot, &r);
        if let Some(f) = models.father(node) {
            r = matrix_multiply_sub3(&r, &matrix_transpose(&models.m(f).grot));
        }
        Some(r)
    } else {
        flag(models, node, UPD_ROTATION);
        if g.rot_mode == 2 {
            let m = models.m_mut(node);
            m.vrot = vadd(angles, m.vrot);
            None
        } else {
            Some(matrix_multiply_sub3(&matrix_xyz_rotation(&mv(angles)), &models.m(node).mrot))
        }
    };
    if let Some(r) = new {
        let prev = mv(models.m(node).vrot);
        models.m_mut(node).vrot = tv(matrix_extract_xyz_rotation(&r, &prev, g.keep_turns));
    }
    if models.m(node).kind != MJNT {
        compensate_matrix(models, node, mode, geom, g, env);
    }
    let father = models.father(node);
    update_son_state(models, father, node, true, g, env);
}

/// Moves a node by a vector, meant as the translation mode says
/// [soft 0x084646a4 SI_modelAddTranslation]; flag bit 0x8.
///
/// - modes 0 and 3: the vector goes through `grot` of the node (0) or of the reference
///   object (3), row 3 of that matrix included; then as mode 1;
/// - modes 1 and 4: row 3 of the node's `gmat` (its global position as of the last update)
///   is added, and under a father the sum goes through the inverse of the father's `gmat`;
/// - mode 2: the vector is added to the local translation;
/// - any other mode: the vector becomes the local translation.
///
/// Class: model.
pub fn add_translation(models: &mut Models, node: NodeId, delta: Vector, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    let m = *models.m(node);
    let mut v = delta;
    match g.trans_mode {
        0 | 1 | 3 | 4 => {
            if g.trans_mode == 0 {
                v = point_through(v, &m.grot);
            } else if g.trans_mode == 3 {
                // (the original reads through the reference pointers without testing them)
                if let Some(r) = g.ref_model {
                    v = point_through(v, &models.m(r).grot);
                }
            }
            v = vadd(v, Vector { x: m.gmat[3][0], y: m.gmat[3][1], z: m.gmat[3][2] });
            if let Some(f) = models.father(node) {
                v = point_through(v, &inverse_or(&models.m(f).gmat, ZERO));
            }
        }
        2 => v = vadd(v, m.trans),
        _ => {}
    }
    models.m_mut(node).trans = v;
    flag(models, node, UPD_TRANSLATION);
    finish(models, node, mode, geom, g, env);
}

// ------------------------------------------------------------------ the safe forms

fn after_set(models: &mut Models, node: NodeId, mode: i32, env: &mut dyn Env) {
    if mode == 1 {
        env.inv_kin_goal_invalidate(models, node);
    }
    if models.m(node).kind == MBALL {
        env.metaball_touch_ball(models, node);
    }
}

/// Sets a node's scaling, local or (with `local` false) global, for a tool
/// [soft 0x08465058 SI_modelSafeSetScaling]. Nothing without a node or a value. A node of
/// a skeleton chain (root, joint, effector) never has its geometry compensated: `geom` is
/// taken as 0. Afterwards the inverse-kinematics goal is invalidated when `mode` is 1, and
/// a metaball is touched.
pub fn safe_set_scaling(models: &mut Models, node: Option<NodeId>, scal: Option<Vector>, local: bool, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    let (Some(node), Some(scal)) = (node, scal) else { return };
    let geom = if is_chain_node(models, node) { 0 } else { geom };
    if local {
        set_scaling(models, node, scal, mode, geom, g, env);
    } else {
        set_global_scaling(models, node, scal, mode, geom, g, env);
    }
    after_set(models, node, mode, env);
}

/// Multiplies a node's local scaling by factors [soft 0x08465100 SI_modelSafeAddScaling]:
/// [`safe_set_scaling`] of the products, as a local scaling. Class: single.
pub fn safe_add_scaling(models: &mut Models, node: Option<NodeId>, factors: Option<Vector>, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    let (Some(node), Some(f)) = (node, factors) else { return };
    let s = models.m(node).scal;
    let product = Vector { x: mul(f.x, s.x), y: mul(f.y, s.y), z: mul(f.z, s.z) };
    safe_set_scaling(models, Some(node), Some(product), true, mode, geom, g, env);
}

/// Sets a node's rotation, local or global, for a tool
/// [soft 0x08465154 SI_modelSafeSetRotation]. A joint never has its geometry compensated;
/// afterwards its preferred angles are reset to its new local angles. In the global form
/// the given angles are first stored as the local ones, which makes them the "previous
/// solution" of the extraction in [`set_global_rotation`].
pub fn safe_set_rotation(models: &mut Models, node: Option<NodeId>, rot: Option<Vector>, local: bool, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    let (Some(node), Some(rot)) = (node, rot) else { return };
    let joint = models.m(node).kind == MJNT;
    let geom = if joint { 0 } else { geom };
    if local {
        set_rotation(models, node, rot, mode, geom, g, env);
    } else {
        models.m_mut(node).vrot = rot;
        set_global_rotation(models, node, rot, mode, geom, g, env);
    }
    if models.m(node).kind == MJNT {
        let angles = models.m(node).vrot;
        env.inv_kin_reset_pref_angle(models, node, angles);
    }
    after_set(models, node, mode, env);
}

/// Turns a node by three angles, in its own frame (`local`) or in the global one
/// [soft 0x08465220 SI_modelSafeAddRotation]: the new local rotation is R(angles) × `mrot`,
/// or `grot` × R(angles) × (father's `grot`)ᵀ; its angles, taken out with the present ones
/// as the previous solution, go to [`safe_set_rotation`] as a local rotation. Class: libm.
pub fn safe_add_rotation(models: &mut Models, node: Option<NodeId>, angles: Option<Vector>, local: bool, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    let (Some(node), Some(angles)) = (node, angles) else { return };
    let mut r = matrix_xyz_rotation(&mv(angles));
    if local {
        r = matrix_multiply_sub3(&r, &models.m(node).mrot);
    } else {
        r = matrix_multiply_sub3(&models.m(node).grot, &r);
        if let Some(f) = models.father(node) {
            r = matrix_multiply_sub3(&r, &matrix_transpose(&models.m(f).grot));
        }
    }
    let new = tv(matrix_extract_xyz_rotation(&r, &mv(models.m(node).vrot), g.keep_turns));
    safe_set_rotation(models, Some(node), Some(new), true, mode, geom, g, env);
}

/// Sets a node's translation, local or global, for a tool
/// [soft 0x08465318 SI_modelSafeSetTranslation]. An effector (algorithm 3) is not moved:
/// the value becomes the goal of its inverse kinematics (state 1) and the goal is
/// invalidated; an effector without that record is left alone.
pub fn safe_set_translation(models: &mut Models, node: Option<NodeId>, pos: Option<Vector>, local: bool, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    let (Some(node), Some(pos)) = (node, pos) else { return };
    let mut invalidate = false;
    if models.m(node).algo_type == 3 {
        let tx = &mut models.data[node].tx;
        if tx.inv_kin_kind.is_some() {
            tx.inv_kin_goal = [pos.x, pos.y, pos.z];
            tx.inv_kin_state = 1;
            invalidate = true;
        }
    } else {
        if local {
            set_translation(models, node, pos, mode, geom, g, env);
        } else {
            set_global_translation(models, node, pos, mode, geom, g, env);
        }
        invalidate = mode == 1;
    }
    if invalidate {
        env.inv_kin_goal_invalidate(models, node);
    }
    if models.m(node).kind == MBALL {
        env.metaball_touch_ball(models, node);
    }
}

/// Moves a node by a vector given in its own frame (`local`: it goes through `grot`, row 3
/// included) or in the global one [soft 0x084653cc SI_modelSafeAddTranslation]: the vector
/// plus the node's global position `gtrs` goes to [`safe_set_translation`] as a global
/// position. Class: model.
pub fn safe_add_translation(models: &mut Models, node: Option<NodeId>, delta: Option<Vector>, local: bool, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    let (Some(node), Some(delta)) = (node, delta) else { return };
    let m = models.m(node);
    let v = if local { point_through(delta, &m.grot) } else { delta };
    let pos = vadd(v, m.gtrs);
    safe_set_translation(models, Some(node), Some(pos), false, mode, geom, g, env);
}

/// [`transformation_matrix`] for a tool [soft 0x084654d4 SI_modelSafeTransformationMatrix]:
/// no geometry compensation for a node of a skeleton chain; a joint's preferred angles are
/// reset afterwards; the inverse-kinematics goal is invalidated when `mode` is 1 or the
/// node is an effector with an inverse-kinematics record.
pub fn safe_transformation_matrix(models: &mut Models, node: Option<NodeId>, m: &Matrix, local: bool, mode: i32, geom: i16, relative: bool, g: &Globals, env: &mut dyn Env) {
    let Some(node) = node else { return };
    let geom = if is_chain_node(models, node) { 0 } else { geom };
    transformation_matrix(models, node, m, local, mode, geom, relative, g, env);
    if models.m(node).kind == MJNT {
        let angles = models.m(node).vrot;
        env.inv_kin_reset_pref_angle(models, node, angles);
    }
    if mode == 1 || (models.m(node).algo_type == 3 && models.data[node].tx.inv_kin_kind.is_some()) {
        env.inv_kin_goal_invalidate(models, node);
    }
}

// ------------------------------------------------------------------ whole states

/// Sets all three local values and updates the node alone, then its sons "by halves"
/// [soft 0x084639cc SI_modelHalfSetState]: flag bits 0xe; [`update_son_state`] without
/// recursion; [`half_update_son_state`] from the first son.
pub fn half_set_state(models: &mut Models, node: NodeId, scal: Vector, rot: Vector, trans: Vector, g: &Globals, env: &mut dyn Env) {
    flag(models, node, 0xe);
    let m = models.m_mut(node);
    m.scal = scal;
    m.vrot = rot;
    m.trans = trans;
    let father = models.father(node);
    update_son_state(models, father, node, false, g, env);
    if let Some(s) = models.son(node) {
        half_update_son_state(models, s, g, env);
    }
}

/// Updates a node, its following brothers and their subtrees, stopping at animated nodes
/// [soft 0x08466c14 modelHalfUpdateSonState]: the brothers first; then, unless the node's
/// element has a function curve of id 100 or 0x70 (in which case neither the node nor
/// anything below it is updated), the node without recursion and on from its first son.
pub fn half_update_son_state(models: &mut Models, node: NodeId, g: &Globals, env: &mut dyn Env) {
    if let Some(b) = models.brother(node) {
        half_update_son_state(models, b, g, env);
    }
    let elem = env.ed_get_elem_from_model(models, node);
    if elem != 0 && (env.ed_find_fcv(elem, 100) || env.ed_find_fcv(elem, 0x70)) {
        return;
    }
    let father = models.father(node);
    update_son_state(models, father, node, false, g, env);
    if let Some(s) = models.son(node) {
        half_update_son_state(models, s, g, env);
    }
}

/// Sets all three local values of a master node from an instance and updates it **as a
/// root**, whatever its father [soft 0x0846393c SI_modelSetMasterTransfoFromInstance]. For
/// the time of the update its algorithm is 0x1e, which exempts it from the readjusting of
/// its definition. No update flag bit is set.
pub fn set_master_transfo_from_instance(models: &mut Models, node: NodeId, scal: Vector, rot: Vector, trans: Vector, g: &Globals, env: &mut dyn Env) {
    let m = models.m_mut(node);
    m.scal = scal;
    m.vrot = rot;
    m.trans = trans;
    let algo = core::mem::replace(&mut m.algo_type, 0x1e);
    update_son_state(models, None, node, true, g, env);
    models.m_mut(node).algo_type = algo;
}

/// Makes an effector keep its own orientation under the classical scaling method
/// [soft 0x084661ec toggleEffRotConstraint]: `gscl` becomes the row lengths of `gmat`,
/// `grot` a copy of `mrot`, and `gmat` is rebuilt as the rows of `grot` times those lengths
/// with `gtrs` in row 3 (its fourth column is `grot`'s). Class: model.
pub fn toggle_eff_rot_constraint(models: &mut Models, node: NodeId) {
    let m = models.m_mut(node);
    m.gscl = tv(matrix_extract_scaling(&m.gmat));
    m.grot = m.mrot;
    m.gmat = m.grot;
    let s = [m.gscl.x, m.gscl.y, m.gscl.z];
    for i in 0..3 {
        for j in 0..3 {
            m.gmat[i][j] = mul(m.gmat[i][j], s[i]);
        }
    }
    m.gmat[3][0] = m.gtrs.x;
    m.gmat[3][1] = m.gtrs.y;
    m.gmat[3][2] = m.gtrs.z;
}

// ------------------------------------------------------------------ compensation

/// One entry of the original's scratch list `glbInit_srt` (0x58 bytes): a node's global
/// scaling, global rotation matrix and global position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InitSrtEntry {
    pub gscl: Vector,
    pub grot: Matrix,
    pub gtrs: Vector,
}

fn entry_of(models: &Models, n: NodeId) -> InitSrtEntry {
    let m = models.m(n);
    InitSrtEntry { gscl: m.gscl, grot: m.grot, gtrs: m.gtrs }
}

fn brothers_from(models: &Models, first: Option<NodeId>) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut at = first;
    while let Some(n) = at {
        out.push(n);
        at = models.brother(n);
    }
    out
}

/// Notes where the initial poses of a node's sons are in global terms
/// [soft 0x08465c10 computeInit_srtGlobalTransfo]: for each of `first` and its following
/// brothers that has an initial transformation, the son is put into that pose (local
/// values and `mrot`), updated under `father` without recursion, its global scaling,
/// rotation and position are appended to the scratch list ([`Models::glb_init_srt`]), and
/// it is put back and updated again. Without such a son the list is left as it was.
pub fn compute_init_srt_global_transfo(models: &mut Models, father: NodeId, first: Option<NodeId>, g: &Globals, env: &mut dyn Env) {
    let sons = brothers_from(models, first);
    if !sons.iter().any(|&s| models.data[s].init_srt.is_some()) {
        return;
    }
    models.glb_init_srt = Some(Vec::new());
    for s in sons {
        let Some(init) = models.data[s].init_srt else { continue };
        let saved = *models.m(s);
        {
            let m = models.m_mut(s);
            m.scal = init[0];
            m.vrot = init[1];
            m.mrot = matrix_xyz_rotation(&mv(init[1]));
            m.trans = init[2];
        }
        update_son_state(models, Some(father), s, false, g, env);
        let e = entry_of(models, s);
        if let Some(list) = models.glb_init_srt.as_mut() {
            list.push(e);
        }
        {
            let m = models.m_mut(s);
            m.scal = saved.scal;
            m.vrot = saved.vrot;
            m.mrot = saved.mrot;
            m.trans = saved.trans;
        }
        update_son_state(models, Some(father), s, false, g, env);
    }
}

/// Notes the global scaling, rotation and position of a node and its following brothers as
/// they are [soft 0x0846613c computeInit_srtParenting], if any of them has an initial
/// transformation.
///
/// The original sizes the list for the nodes that have one but writes an entry for
/// **every** node, past the end of its block when some have none; and
/// [`compensate_init_srt`] then reads the entries in order for the nodes that have one, so
/// that after a node without one the entries no longer belong to their nodes. The entries
/// are the same here; nothing is overrun.
pub fn compute_init_srt_parenting(models: &mut Models, first: Option<NodeId>) {
    let nodes = brothers_from(models, first);
    if !nodes.iter().any(|&s| models.data[s].init_srt.is_some()) {
        return;
    }
    models.glb_init_srt = Some(nodes.iter().map(|&n| entry_of(models, n)).collect());
}

/// Rewrites the initial transformations of a node's sons so that their noted global poses
/// ([`compute_init_srt_global_transfo`]) hold under the node as it is now
/// [soft 0x08465e3c compensateInit_srt], and frees the scratch list.
///
/// - usual method: initial scaling = noted global scaling ÷ the node's `gscl` (floored at
///   ±0.0001, ten times the update's floor);
///   initial rotation = the angles of noted rotation × (`grot`)⁻¹, with the present initial
///   angles as the previous solution; initial translation = noted position through
///   (`gmat`)⁻¹;
/// - classical method: the matrix S·R·T of the noted values times (`gmat`)⁻¹, taken apart
///   again (scaling positive). The angles of the noted rotation are first taken out with a
///   previous solution the original never initialises (a stack slot; from the second son on
///   it holds the angles of the son before): zeros here for the first son.
///
/// A son with an initial transformation but no entry left (the list is gone or too short:
/// the original reads on from a null or stale pointer) is skipped.
pub fn compensate_init_srt(models: &mut Models, node: NodeId, first: Option<NodeId>, g: &Globals) {
    let m = *models.m(node);
    let inv_rot = inverse_or(&m.grot, ZERO);
    let inv = inverse_or(&m.gmat, ZERO);
    let list = models.glb_init_srt.take().unwrap_or_default();
    let mut entries = list.iter();
    let mut angles = MVector::new(0.0, 0.0, 0.0);
    let mut first_son = true;
    for s in brothers_from(models, first) {
        let Some(mut init) = models.data[s].init_srt else { continue };
        let Some(e) = entries.next() else { continue };
        if g.classical_scaling {
            if first_son {
                // the previous solution is a stack slot nobody wrote
                crate::env::stale::note();
                first_son = false;
            }
            angles = matrix_extract_xyz_rotation(&e.grot, &angles, g.keep_turns);
            let p = matrix_multiply(&matrix_scal_rot_trans(&mv(e.gscl), &angles, &mv(e.gtrs)), &inv);
            let (sc, ro, tr) = matrix_extract_transformations(&p, &mv(init[1]), g.keep_turns);
            init = [tv(sc), tv(ro), tv(tr)];
        } else {
            init[0] = over_compensation(e.gscl, m.gscl);
            let r = matrix_multiply_sub3(&e.grot, &inv_rot);
            init[1] = tv(matrix_extract_xyz_rotation(&r, &mv(init[1]), g.keep_turns));
            init[2] = point_through(e.gtrs, &inv);
        }
        models.data[s].init_srt = Some(init);
    }
}

/// Keeps what hangs on a node in place while the node itself is changed
/// [soft 0x08465660 SI_modelCompensateMatrix]. Called by every setter **before** its
/// update, with the new local values already stored. Nothing unless `mode` is 0 or `geom`
/// is set. In this order:
///
/// 1. with `SI_initPoseCompensate`: [`compute_init_srt_global_transfo`] for the sons;
/// 2. the node's old `gmat` is kept, the node is updated without recursion, and the
///    inverse of its new `gmat` is taken;
/// 3. with `geom`: the geometry is multiplied by old × new⁻¹
///    ([`crate::transform::multiply_definition`]), so do its clusters (a hook), and the
///    bounding box is recomputed (a hook);
/// 4. with `mode` 0, for every son: its flag word +0x08 is **set to** 0x2000 (all other
///    bits cleared), then bit 0x2000 is cleared again in every node a son follows as its
///    path. Then each son gets new local values that reproduce its old global ones under
///    the node's new matrix. Under hierarchy manipulation (`SI_glbManip` + 0x1e) a son that
///    is selected on screen is instead compensated in turn (this routine, mode 0), and the
///    local translations are left alone. For the others, those that still have bit 0x2000:
///    - usual method: scaling = the son's `gscl` ÷ the node's `gscl` (a divisor smaller
///      than 0.0001 taken as ±0.0001: ten times the floor the update itself uses); `mrot` =
///      the son's `grot` × (the node's `grot`)⁻¹ and the angles taken out of it; translation
///      = the son's `gtrs` through the node's new inverse;
///    - classical method: the son's `gmat` × new⁻¹ taken apart into scaling, angles and
///      translation; and unless the node or the son belongs to a skeleton chain,
///      `SI_modelCompensateAngle` (a hook) with that product;
///    - an effector whose chain root has inverse kinematics of kind other than 0 keeps its
///      angles in both methods.
/// 5. with `SI_initPoseCompensate`: [`compensate_init_srt`].
///
/// Where the original goes on after an inverse that does not exist, with what its stack
/// held: zeros here, except that in the usual method the inverse of step 2 is then the
/// matrix it was the last time (none: zeros). Class: libm.
pub fn compensate_matrix(models: &mut Models, node: NodeId, mode: i32, geom: i16, g: &Globals, env: &mut dyn Env) {
    if mode != 0 && geom == 0 {
        return;
    }
    if g.init_pose_compensate {
        let first = models.son(node);
        compute_init_srt_global_transfo(models, node, first, g, env);
    }
    let old = models.m(node).gmat;
    let father = models.father(node);
    update_son_state(models, father, node, false, g, env);
    let inv = inverse_or(&models.m(node).gmat, ZERO);
    if geom != 0 {
        let m = matrix_multiply(&old, &inv);
        crate::transform::multiply_definition(models, node, &m, g, env);
        env.clusters_apply_transformation(models, node, &m);
        env.compute_bounding(models, node);
    }
    if mode == 0 {
        let sons = brothers_from(models, models.son(node));
        for &s in &sons {
            models.m_mut(s).flags_08 = 0x2000;
        }
        for &s in &sons {
            if let Some(path) = Models::id(Ptr32::new(models.m(s).path.0)) {
                models.m_mut(path).flags_08 &= 0xdfff;
            }
        }
        let node_in_chain = is_chain_node(models, node);
        // the classical method takes the inverse a second time, into a slot of its own
        let inv2 = if g.classical_scaling { inverse_or(&models.m(node).gmat, ZERO) } else { ZERO };
        let inv_rot = if g.classical_scaling { ZERO } else { inverse_or(&models.m(node).grot, ZERO) };
        // the product of the classical method stays in its slot from one son to the next
        let mut product = ZERO;
        for &s in &sons {
            if g.hierarchy_manipulation && env.graf_selection(models, s, 3) == Some(1) {
                compensate_matrix(models, s, 0, geom, g, env);
                continue;
            }
            let marked = models.m(s).flags_08 & 0x2000 != 0;
            // an effector under a chain root with inverse kinematics of a kind other than 0
            // keeps its angles (the original reads the root's record without testing the
            // pointer; none is "kind 0" here)
            let keeps = models.m(s).algo_type == 3 && models.data[node].tx.inv_kin_kind.unwrap_or(0) != 0;
            if g.classical_scaling {
                if marked {
                    product = matrix_multiply(&models.m(s).gmat, &inv2);
                    let (sc, ro, tr) = matrix_extract_transformations(&product, &mv(models.m(s).vrot), g.keep_turns);
                    let m = models.m_mut(s);
                    m.scal = tv(sc);
                    if !keeps {
                        m.vrot = tv(ro);
                        m.mrot = matrix_xyz_rotation(&ro);
                    }
                    if !g.hierarchy_manipulation {
                        m.trans = tv(tr);
                    }
                }
                if !node_in_chain && !is_chain_node(models, s) {
                    env.compensate_angle(models, &product, s);
                }
            } else if marked {
                let gscl = models.m(node).gscl;
                let m = models.m_mut(s);
                m.scal = over_compensation(m.gscl, gscl);
                if !keeps {
                    m.mrot = matrix_multiply_sub3(&m.grot, &inv_rot);
                    m.vrot = tv(matrix_extract_xyz_rotation(&m.mrot, &mv(m.vrot), g.keep_turns));
                }
                if !g.hierarchy_manipulation {
                    m.trans = point_through(m.gtrs, &inv);
                }
            }
        }
    }
    if g.init_pose_compensate {
        let first = models.son(node);
        compensate_init_srt(models, node, first, g);
    }
}

// ------------------------------------------------------------------ from a matrix

/// Gives a node the transformation a matrix holds
/// [soft 0x084738f0 SI_modelTransformationMatrix].
///
/// The matrix is taken apart into scaling (row lengths), a rotation matrix and a
/// translation (`SI_matrixExtractTransformations2`). With `relative` it is applied on top of
/// the node's present global transformation: scaling × the node's local scaling × `hscl`;
/// rotation `grot` × R (or R × `grot` with `local`, in which case the translation also goes
/// through that product); translation + `gtrs`; and the result is then global whatever
/// `local` said.
///
/// A global result under a father is brought into the father's terms: scaling ÷ the
/// father's `gscl` (floored) by the usual method, by the classical one the row lengths of
/// S × (father's `gmat`)⁻¹ (or, with `relative`, the scaling as it is); rotation × (father's
/// `grot`)ᵀ; translation through (father's `gmat`)⁻¹. A local result, and that of a root,
/// is stored as it is. The angles are always taken out with zeros as the previous solution.
///
/// An effector (algorithm 3) keeps its translation; if it has an inverse-kinematics record
/// the translation (plus `htrs` when it is local) becomes the goal instead (state 1).
///
/// Each update flag bit (0x2, 0x4, 0x8) is set unless all three components of that value
/// moved by less than 0.0001. Then the compensation unless the node is a joint, and the
/// update. Class: libm.
pub fn transformation_matrix(models: &mut Models, node: NodeId, matrix: &Matrix, local: bool, mode: i32, geom: i16, relative: bool, g: &Globals, env: &mut dyn Env) {
    let before = *models.m(node);
    let (scl, mut rot, trs) = matrix_extract_transformations2(matrix);
    let (mut scl, mut trs) = (tv(scl), tv(trs));
    let mut local = local;
    if relative {
        let m = &before;
        let both = |a: f32, b: f32, c: f32| ((x(a) * b) * c).to_f32();
        scl = Vector { x: both(scl.x, m.scal.x, m.hscl.x), y: both(scl.y, m.scal.y, m.hscl.y), z: both(scl.z, m.scal.z, m.hscl.z) };
        rot = if local { matrix_multiply_sub3(&rot, &m.grot) } else { matrix_multiply_sub3(&m.grot, &rot) };
        if local {
            trs = point_through(trs, &rot);
        }
        trs = vadd(trs, m.gtrs);
        local = false;
    }
    let father = models.father(node);
    let zero = MVector::new(0.0, 0.0, 0.0);
    let effector = before.algo_type == 3;
    match father {
        Some(f) if !local => {
            let fm = *models.m(f);
            let inv = inverse_or(&fm.gmat, ZERO);
            let s = if g.classical_scaling {
                if relative { scl } else { tv(matrix_extract_scaling(&matrix_multiply(&matrix_scaling(&mv(scl)), &inv))) }
            } else {
                over_scaling(scl, fm.gscl)
            };
            rot = matrix_multiply_sub3(&rot, &matrix_transpose(&fm.grot));
            let m = models.m_mut(node);
            m.scal = s;
            m.vrot = tv(matrix_extract_xyz_rotation(&rot, &zero, g.keep_turns));
            if !effector {
                m.trans = point_through(trs, &inv);
            }
        }
        _ => {
            let m = models.m_mut(node);
            m.scal = scl;
            m.vrot = tv(matrix_extract_xyz_rotation(&rot, &zero, g.keep_turns));
            if !effector {
                m.trans = trs;
            }
        }
    }
    if effector && models.data[node].tx.inv_kin_kind.is_some() {
        if local {
            trs = vadd(trs, before.htrs);
        }
        let tx = &mut models.data[node].tx;
        tx.inv_kin_goal = [trs.x, trs.y, trs.z];
        tx.inv_kin_state = 1;
    }
    let moved = |new: Vector, old: Vector| {
        let near = |a: f32, b: f32| (x(a) - b).abs().lt(x(0.0001f64));
        !(near(new.x, old.x) && near(new.y, old.y) && near(new.z, old.z))
    };
    let m = *models.m(node);
    if moved(m.scal, before.scal) {
        flag(models, node, UPD_SCALING);
    }
    if moved(m.vrot, before.vrot) {
        flag(models, node, UPD_ROTATION);
    }
    if moved(m.trans, before.trans) {
        flag(models, node, UPD_TRANSLATION);
    }
    if m.kind != MJNT {
        compensate_matrix(models, node, mode, geom, g, env);
    }
    update_son_state(models, father, node, true, g, env);
}

// ------------------------------------------------------------------ textures

/// Keeps a node's 2D textures oriented when its rotation is reset
/// [soft 0x0846cf80 textureFreeze]. For every local 2D texture of every material of the
/// node, then every 2D texture of the node itself: with `rot`, the texture's rotation
/// angles (+0x94) become those of (R(angles)⁻¹ × the node's `grot`)⁻¹, taken out with the
/// old angles as the previous solution; and the texture is marked dirty (a hook) in every
/// case. `trans` and `scal`, the routine's first two flags, are not looked at.
///
/// (Two inverses are taken per texture into stack slots; where one does not exist the slot
/// keeps what the texture before left there, zeros for the first.) Class: libm.
pub fn texture_freeze(models: &mut Models, node: NodeId, _trans: bool, _scal: bool, rot: bool, g: &Globals, env: &mut dyn Env) {
    let grot = models.m(node).grot;
    let count = models.m(node).nb_materials.max(0) as usize;
    let cx = &mut models.data[node].cx;
    let (mut first, mut second) = (ZERO, ZERO);
    let per_material = cx.material_textures.iter_mut().take(count).flatten();
    for t in per_material.chain(cx.textures.iter_mut()) {
        if rot {
            first = inverse_or(&matrix_xyz_rotation(&mv(t.rot)), first);
            second = inverse_or(&matrix_multiply(&first, &grot), second);
            t.rot = tv(matrix_extract_xyz_rotation(&second, &mv(t.rot), g.keep_turns));
        }
        env.texture2d_set_dirty(t.ptr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::set_state;

    fn v(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    /// A father with one son, both updated.
    fn pair() -> (Models, NodeId, NodeId, Globals) {
        let mut m = Models::default();
        let (a, b) = (m.allocate(None), m.allocate(None));
        m.arena.create_parentship(a, b);
        let g = Globals::default();
        set_state(&mut m, a, v(2.0, 2.0, 2.0), v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), &g, &mut ());
        set_state(&mut m, b, v(1.0, 1.0, 1.0), v(0.0, 0.0, 0.0), v(1.0, 2.0, 3.0), &g, &mut ());
        (m, a, b, g)
    }

    #[test]
    fn mode_0_keeps_the_sons_in_place_and_mode_1_takes_them_along() {
        let (mut m, a, b, g) = pair();
        assert_eq!(m.m(b).gtrs, v(12.0, 4.0, 6.0));
        // the father moves, sons compensated: the son's global position holds
        set_global_translation(&mut m, a, v(0.0, 5.0, 0.0), 0, 0, &g, &mut ());
        assert_eq!(m.m(a).gtrs, v(0.0, 5.0, 0.0));
        assert_eq!(m.m(b).gtrs, v(12.0, 4.0, 6.0));
        assert_eq!(m.m(b).trans, v(6.0, -0.5, 3.0));
        // not compensated: the son follows
        set_global_translation(&mut m, a, v(1.0, 5.0, 0.0), 1, 0, &g, &mut ());
        assert_eq!(m.m(b).gtrs, v(13.0, 4.0, 6.0));
    }

    #[test]
    fn a_global_value_is_brought_into_the_fathers_terms() {
        let (mut m, _, b, g) = pair();
        set_global_scaling(&mut m, b, v(4.0, 2.0, 1.0), 1, 0, &g, &mut ());
        assert_eq!((m.m(b).scal, m.m(b).gscl), (v(2.0, 1.0, 0.5), v(4.0, 2.0, 1.0)));
        set_global_translation(&mut m, b, v(10.0, 0.0, 2.0), 1, 0, &g, &mut ());
        assert_eq!(m.m(b).trans, v(0.0, 0.0, 1.0));
        assert_eq!(m.m(b).update_flags & 0xa, 0xa);
    }

    #[test]
    fn the_compensation_floors_at_one_ten_thousandth() {
        let (mut m, a, b, g) = pair();
        // the father's scaling collapses on x; its global factor is floored at 1e-5 by the
        // update, but the son's new local factor divides by 1e-4
        set_scaling(&mut m, a, v(0.0, 2.0, 2.0), 0, 0, &g, &mut ());
        assert_eq!(m.m(a).gscl.x, 9.999_999_747_378_752e-6);
        assert_eq!(m.m(b).scal.x, (2.0f32 / 9.999_999_747_378_752e-5f32));
    }

    #[test]
    fn an_effector_is_not_moved_but_given_a_goal() {
        let (mut m, _, b, g) = pair();
        m.m_mut(b).algo_type = 3;
        m.data[b].tx.inv_kin_kind = Some(0);
        safe_set_translation(&mut m, Some(b), Some(v(7.0, 8.0, 9.0)), true, 0, 0, &g, &mut ());
        assert_eq!(m.m(b).trans, v(1.0, 2.0, 3.0));
        assert_eq!((m.data[b].tx.inv_kin_goal, m.data[b].tx.inv_kin_state), ([7.0, 8.0, 9.0], 1));
    }

    #[test]
    fn added_translation_by_mode() {
        let (mut m, _, b, mut g) = pair();
        g.trans_mode = 2; // added to the local translation
        add_translation(&mut m, b, v(1.0, 0.0, 0.0), 1, 0, &g, &mut ());
        assert_eq!(m.m(b).trans, v(2.0, 2.0, 3.0));
        g.trans_mode = 1; // along the world axes: one unit there is half a unit under a father scaled by 2
        add_translation(&mut m, b, v(2.0, 0.0, 0.0), 1, 0, &g, &mut ());
        assert_eq!(m.m(b).trans, v(3.0, 2.0, 3.0));
        g.trans_mode = 7; // any other mode: the vector itself
        add_translation(&mut m, b, v(5.0, 5.0, 5.0), 1, 0, &g, &mut ());
        assert_eq!(m.m(b).trans, v(5.0, 5.0, 5.0));
    }
}
