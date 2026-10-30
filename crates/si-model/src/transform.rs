// SPDX-License-Identifier: MIT
//! A node's transformation and how it is carried down the hierarchy: the transformation
//! part of the original's `model.c` (finding 0015).
//!
//! A record holds three sets of values (offsets are the record's):
//!
//! - **local**: `scal` (+0x100), `vrot` (+0x10c, angles in radians), `mrot` (+0x118, the
//!   rotation matrix of those angles), `trans` (+0x158);
//! - **father's**, a copy taken at each update: `hscl` (+0xa8), `hrot` (+0xb4), `htrs` (+0xf4);
//! - **global**: `gscl` (+0x16c), `grot` (+0x178), `gtrs` (+0x1b8) and the matrix `gmat`
//!   (+0x1c4).
//!
//! [`update_son_state`] derives the global set from the local one and the father's global
//! set, by one of two methods chosen by a program-wide setting ([`Globals::classical_scaling`]).
//! Conventions are those of finding 0008: row vectors, `v' = v × M`, translation in row 3,
//! rotation by three angles is X then Y then Z.
//!
//! Exactness classes as in `si-math`. Rotation matrices come from the angles through the
//! C library's sine and cosine (class libm); everything else here is copy, single or model.

use si_math::algebra::{matrix_extract_xyz_rotation, matrix_inverse, matrix_multiply, matrix_multiply_sub3, matrix_xyz_rotation};
use si_math::types::{Matrix, IDENTITY};
use si_types::geom::Vector;
use si_types::scene::LclMap;
use si_types::Ptr32;
use si_x87::x;

use crate::env::Env;
use crate::hierarchy::{next_def_get, DefSlot};
use crate::node::{mv, tv, Current, Definition, Models, NodeId, MBALL, MJNT, MNILL};
use crate::obj_global::mesh_transform;

/// Owned data of a node that the transformation routines need beyond [`crate::node::NodeData`].
#[derive(Clone, Debug, Default)]
pub struct NodeExt {
    /// The first word of the record `inv_kin` (+0x318) points to, when the pointer is not
    /// null: the kind of inverse kinematics of a chain root (model file token 20).
    pub inv_kin_kind: Option<i32>,
    /// The three floats at +0x08 of that record: the goal of the inverse kinematics
    /// (finding 0017), where a move of the effector is parked.
    pub inv_kin_goal: [f32; 3],
    /// The word at +0x38 of that record: 0 none, 1 goal set, 2 solved.
    pub inv_kin_state: i32,
}

/// Bits of the record's two update flag words (+0x0a and +0x3fa; set together, 0x3e at
/// creation).
pub const UPD_NAME: u16 = 0x01;
pub const UPD_SCALING: u16 = 0x02;
pub const UPD_ROTATION: u16 = 0x04;
pub const UPD_TRANSLATION: u16 = 0x08;
/// Set in +0x3fa only, when an update changes the sign of the product of the three global
/// scaling factors (the node has become mirrored, or stopped being so).
pub const UPD_MIRROR: u16 = 0x10;

/// Bit 15 of `flags_08` (+0x08): the node is inside [`update_son_state`].
pub const BUSY: u16 = 0x8000;

/// A scaling factor smaller than this (the double 1e-5, 0x0865fd30) is replaced by the
/// float nearest ±1e-5 (0x0865fd38, 0x0865fd3c).
const SCALING_MIN: f64 = 1e-5;
const SCALING_FLOOR: f32 = 9.999_999_747_378_752e-6;

/// The program-wide settings these routines read.
#[derive(Clone, Copy, Debug, Default)]
pub struct Globals {
    /// The first word of `SI_glbManip` (0x08781560) is 1: the *classical* scaling method,
    /// where a node's global matrix is its local matrix times its father's global matrix.
    /// 0, the value at start-up, is the method that keeps scaling apart from rotation.
    pub classical_scaling: bool,
    /// `SI_viewerUpdOpt` (0x08679678), 0 at start-up. Bit 0x10: `def_matrix` is not
    /// maintained. Bit 0x02: an update goes down to the sons only, not on to the brothers.
    pub viewer_upd_opt: u32,
    /// The 16-bit word at 0x08781486, which is `SI_glbAnim` + 0x06, the animation
    /// evaluation's "in update" mark (finding 0017): non-zero keeps an enveloped node
    /// (envelope type 5, 6 or 8) from having its definition readjusted.
    pub envelope_hold: i16,
    /// The short at `SI_glbManip` + 0x20: angles are kept within a turn of the previous
    /// ones when a rotation is taken out of a matrix (finding 0008).
    pub keep_turns: bool,
    /// `SI_initPoseCompensate` (0x087817a0) is not zero: freezing also resets the node's
    /// initial transformation (`init_srt`).
    pub init_pose_compensate: bool,
    /// The Duplicate command's settings (`SI_glbDuplic`, 0x087814c0).
    pub duplic: crate::node_copy::Duplic,
    /// `SI_edSumatraTo3x` is not zero: an export to the successor product is running.
    pub sumatra_to_3x: bool,
    /// `SI_glbManip` + 0x08 (setup keyword `PREF_ROTATION`): how an added rotation is meant.
    /// 1 about the world axes, 2 added to the angles, 3 about the axes of the reference
    /// object, 4 about the line of sight; any other value: composed with the local rotation.
    pub rot_mode: i32,
    /// `SI_glbManip` + 0x0c (`PREF_TRANSLATION`): how an added translation is meant. 0 along
    /// the node's own axes, 3 along the reference object's, 1 and 4 along the world axes,
    /// 2 added to the local translation; any other value: taken as the new local translation.
    pub trans_mode: i32,
    /// The model `SI_glbManip` + 0x18 leads to (the pointer there, then the pointer at +4 of
    /// what it points to): the reference object of the modes 3.
    pub ref_model: Option<NodeId>,
    /// The short at `SI_glbManip` + 0x1e (`PREF_HIERARCHY_MANIPULATION`).
    pub hierarchy_manipulation: bool,
    /// `SI_updateTime` (0x0869bca4) while `SI_updateTimeSet` (0x0869bca2) is not zero: the
    /// time deformations are evaluated at. `None`: the current frame over the frame rate.
    pub update_time: Option<f32>,
    /// `SI_frameCurrent` (0x08782040).
    pub frame_current: i32,
    /// `SI_frameRate` (0x0878204c).
    pub frame_rate: f32,
    /// `SI_interactive` (0x0867953e) is not zero.
    pub interactive: bool,
}

/// What these routines call in parts of the program that are not ported. The defaults do
/// nothing.
pub trait Hooks {
    /// `SI_metaballGetParentSystem` and `SI_metaballTouch(ball, NULL)`: a metaball moved.
    fn metaball_moved(&mut self, _models: &mut Models, _node: NodeId) {}
    /// `SI_modelCompensateLocalCoord(father, node)` (`classic.c`, not ported).
    fn compensate_local_coord(&mut self, _models: &mut Models, _father: NodeId, _node: NodeId) {}
    /// `SI_modelCompensateAngle(m, son)` (`classic.c`, not ported): the classical method's
    /// bookkeeping of a son's angles when its father is transformed under it.
    fn compensate_angle(&mut self, _models: &mut Models, _m: &Matrix, _son: NodeId) {}
    /// `SI_clustersApplyTransformation(node, m)` (`klusters.c`).
    fn clusters_apply_transformation(&mut self, _models: &mut Models, _node: NodeId, _m: &Matrix) {}
    /// `SI_invKinGoalInvalidate(node)` (`invkin.c`).
    fn inv_kin_goal_invalidate(&mut self, _models: &mut Models, _node: NodeId) {}
    /// `SI_invKinResetPrefAngle(node, 0, angles)` (`invkin.c`): a joint's preferred angles.
    fn inv_kin_reset_pref_angle(&mut self, _models: &mut Models, _node: NodeId, _angles: Vector) {}
    /// `SI_metaballTouch(node.definition, NULL)`: a metaball was transformed.
    fn metaball_touch_ball(&mut self, _models: &mut Models, _node: NodeId) {}
    /// `SI_texture2DSetDirty(texture)`.
    fn texture2d_set_dirty(&mut self, _texture: Ptr32<LclMap>) {}
    /// `SI_edGetElemFromModel(node, …)`: the node's element in the scene dictionary, as a
    /// handle; 0 when it has none.
    fn ed_get_elem_from_model(&mut self, _models: &Models, _node: NodeId) -> u32 {
        0
    }
    /// `SI_edFindFcv(element, 1, id) != 0`: the element has a function curve of that id.
    fn ed_find_fcv(&mut self, _element: u32, _id: i32) -> bool {
        false
    }
}

impl Hooks for () {}

/// What the definition routines call in geometry code that is not ported (patches, faces,
/// splines, NURBS). The defaults do nothing.
pub trait GeometryHooks {
    /// `SI_patchTransform`, `SI_faceTransform`, `SI_splineTransform`,
    /// `SI_nurbsCrvApplyMatrix(…, 0)`, `SI_nurbsSrfApplyMatrix(…, 0)`: a definition of that
    /// kind through a matrix.
    fn transform(&mut self, _kind: i32, _definition: &mut Definition, _m: &Matrix) {}
    /// `SI_modelComputeBounding(node)` (not ported).
    fn compute_bounding(&mut self, _models: &mut Models, _node: NodeId) {}
}

impl GeometryHooks for () {}

pub(crate) fn row3(v: Vector) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// One product of two floats, formed in the register and stored: the bits of a float
/// multiplication except when both operands are NaNs, where the unit keeps the one with the
/// larger significand.
pub(crate) fn mul(a: f32, b: f32) -> f32 {
    (x(a) * b).to_f32()
}

/// Rows of a rotation times three factors, as the compiler laid the nine products out:
/// rotation element first in the first two columns, factor first in the third.
pub(crate) fn scaled_rows(rot: &Matrix, s: [f32; 3], out: &mut Matrix) {
    for i in 0..3 {
        out[i][0] = mul(rot[i][0], s[i]);
        out[i][1] = mul(rot[i][1], s[i]);
        out[i][2] = mul(s[i], rot[i][2]);
    }
}

/// Builds a node's matrix from its **local** values, in place of the global matrix
/// [soft 0x0846559c SI_modelUpdateMatrix]: row i of `mrot` times the i-th scaling factor in
/// the first three columns, the translation in row 3. The fourth column is not written.
/// Class: single.
///
/// The result is stored in `gmat`: a caller uses this for a node without a father, or
/// before multiplying by the father's matrix itself.
pub fn update_matrix(models: &mut Models, node: NodeId) {
    let m = models.m_mut(node);
    let (rot, s) = (m.mrot, row3(m.scal));
    scaled_rows(&rot, s, &mut m.gmat);
    let t = row3(m.trans);
    m.gmat[3][..3].copy_from_slice(&t);
}

/// A node's local scaling, rotation angles and translation [soft 0x08463774 SI_modelState].
/// Class: copy.
pub fn state(models: &Models, node: NodeId) -> (Vector, Vector, Vector) {
    let m = models.m(node);
    (m.scal, m.vrot, m.trans)
}

/// A node's global scaling, rotation angles and translation
/// [soft 0x084637dc SI_modelGlobalState]. Class: libm (through the angle extraction).
///
/// By the usual method: `gscl`; the angles of `grot`, taken out with the node's local angles
/// as the previous solution (`SI_matrixExtractXyzRotation`); `gtrs`. By the classical
/// method the answer is scaling 1, angles 0 and row 3 of `gmat`: there the global matrix
/// cannot be taken apart, and the routine does not try.
pub fn global_state(models: &Models, node: NodeId, g: &Globals) -> (Vector, Vector, Vector) {
    let m = models.m(node);
    if g.classical_scaling {
        let one = Vector { x: 1.0, y: 1.0, z: 1.0 };
        let zero = Vector { x: 0.0, y: 0.0, z: 0.0 };
        return (one, zero, Vector { x: m.gmat[3][0], y: m.gmat[3][1], z: m.gmat[3][2] });
    }
    let rot = matrix_extract_xyz_rotation(&m.grot, &mv(m.vrot), g.keep_turns);
    (m.gscl, tv(rot), m.gtrs)
}

/// Sets all three local values and updates the node and what hangs below it
/// [soft 0x084638a8 SI_modelSetState]. The update flag bits 0x2, 0x4 and 0x8 are set in
/// both words. Class: libm (through the update).
pub fn set_state(models: &mut Models, node: NodeId, scal: Vector, rot: Vector, trans: Vector, g: &Globals, hooks: &mut dyn Env) {
    let m = models.m_mut(node);
    m.update_flags |= 0xe;
    m.update_flags2 |= 0xe;
    m.scal = scal;
    m.vrot = rot;
    m.trans = trans;
    let father = models.father(node);
    update_son_state(models, father, node, true, g, hooks);
}

/// Sets the local scaling [soft 0x08463a74 SI_modelSetScaling]: the value, flag bit 0x2,
/// [`crate::setters::compensate_matrix`] with the caller's two arguments, then the update.
pub fn set_scaling(models: &mut Models, node: NodeId, scal: Vector, a: i32, b: i16, g: &Globals, hooks: &mut dyn Env) {
    let m = models.m_mut(node);
    m.scal = scal;
    m.update_flags |= UPD_SCALING;
    m.update_flags2 |= UPD_SCALING;
    crate::setters::compensate_matrix(models, node, a, b, g, hooks);
    let father = models.father(node);
    update_son_state(models, father, node, true, g, hooks);
}

/// Sets the local rotation angles [soft 0x08463f54 SI_modelSetRotation]: the angles, their
/// matrix in `mrot`, flag bit 0x4, the compensation unless the node is a joint (kind 5),
/// then the update. Class: libm.
pub fn set_rotation(models: &mut Models, node: NodeId, rot: Vector, a: i32, b: i16, g: &Globals, hooks: &mut dyn Env) {
    let m = models.m_mut(node);
    m.vrot = rot;
    m.mrot = matrix_xyz_rotation(&mv(rot));
    m.update_flags |= UPD_ROTATION;
    m.update_flags2 |= UPD_ROTATION;
    if m.kind != MJNT {
        crate::setters::compensate_matrix(models, node, a, b, g, hooks);
    }
    let father = models.father(node);
    update_son_state(models, father, node, true, g, hooks);
}

/// Sets the local translation [soft 0x084640cc SI_modelSetTranslation]: the value, flag bit
/// 0x8, the compensation, then the update.
pub fn set_translation(models: &mut Models, node: NodeId, trans: Vector, a: i32, b: i16, g: &Globals, hooks: &mut dyn Env) {
    let m = models.m_mut(node);
    m.trans = trans;
    m.update_flags |= UPD_TRANSLATION;
    m.update_flags2 |= UPD_TRANSLATION;
    crate::setters::compensate_matrix(models, node, a, b, g, hooks);
    let father = models.father(node);
    update_son_state(models, father, node, true, g, hooks);
}

/// A scaling factor as these routines divide by it or keep it: one smaller than 1e-5
/// replaced by the float nearest ±1e-5, with the factor's sign (zero of either sign counts
/// as positive).
pub(crate) fn floor_scaling(c: f32) -> f32 {
    if x(c).abs().lt(x(SCALING_MIN)) {
        if c >= 0.0 { SCALING_FLOOR } else { -SCALING_FLOOR }
    } else {
        c
    }
}

/// The scaling factors with the too-small ones replaced, written back to the record.
fn clamp_scaling(models: &mut Models, node: NodeId) {
    let m = models.m_mut(node);
    m.scal = Vector { x: floor_scaling(m.scal.x), y: floor_scaling(m.scal.y), z: floor_scaling(m.scal.z) };
}

/// A point through the upper three rows and row 3 of a matrix, as these routines write it
/// out: `((y·m[1][j] + x·m[0][j]) + z·m[2][j]) + m[3][j]`, formed in the register and
/// rounded once.
pub(crate) fn point_through(p: Vector, m: &Matrix) -> Vector {
    let c = |j: usize| (((x(p.y) * m[1][j] + x(p.x) * m[0][j]) + x(p.z) * m[2][j]) + m[3][j]).to_f32();
    Vector { x: c(0), y: c(1), z: c(2) }
}

/// True for the nodes of a skeleton chain: its root (a null with algorithm 1 or 2), a joint
/// (kind 5), its effector (a null with algorithm 3 or 4).
pub(crate) fn is_chain_node(models: &Models, n: NodeId) -> bool {
    let m = models.m(n);
    m.kind == MJNT || (m.kind == MNILL && (1..=4).contains(&m.algo_type))
}

/// The node is an effector (algorithm 3) and the chain root above it (its own `father`
/// link) has inverse kinematics of kind 1. The original reads through the father pointer
/// and its `inv_kin` pointer without testing either; a null one is "no" here.
fn effector_keeps_orientation(models: &Models, node: NodeId) -> bool {
    models.m(node).algo_type == 3
        && models.father(node).is_some_and(|f| models.data[f].tx.inv_kin_kind == Some(1))
}

/// The classical method's bookkeeping for one node, and with `recurse` for its following
/// brothers and its subtree [soft 0x081ca134 SI_modelUpdateCScalingInfo]. Class: libm.
///
/// It keeps beside the real transformation a frame without scaling: `cscl` (+0x3a0) is the
/// product of the scaling factors down the hierarchy, component by component; `unk_3ac`
/// (+0x3ac) is the node's position carried through its ancestors' rotations and
/// translations only; `cscl_matrix` (+0x360) has `grot` in its upper rows and that position
/// in row 3. It also refreshes `hscl`, `hrot`, `htrs`, `mrot` and `grot`.
pub fn update_cscaling_info(models: &mut Models, father: Option<NodeId>, node: NodeId, recurse: bool) {
    match father {
        None => {
            let m = models.m_mut(node);
            m.hscl = Vector { x: 1.0, y: 1.0, z: 1.0 };
            m.hrot = IDENTITY;
            m.htrs = Vector { x: 0.0, y: 0.0, z: 0.0 };
            m.unk_3ac = m.trans;
        }
        Some(f) => {
            let fm = *models.m(f);
            let m = models.m_mut(node);
            m.hscl = fm.cscl;
            m.hrot = fm.grot;
            m.htrs = fm.unk_3ac;
            m.unk_3ac = point_through(m.trans, &fm.cscl_matrix);
        }
    }
    let keeps = effector_keeps_orientation(models, node);
    let m = models.m_mut(node);
    m.mrot = matrix_xyz_rotation(&mv(m.vrot));
    m.cscl = Vector { x: mul(m.hscl.x, m.scal.x), y: mul(m.hscl.y, m.scal.y), z: mul(m.hscl.z, m.scal.z) };
    m.grot = if keeps { m.mrot } else { matrix_multiply_sub3(&m.mrot, &m.hrot) };
    for i in 0..3 {
        for j in 0..3 {
            m.cscl_matrix[i][j] = m.grot[i][j];
        }
    }
    m.cscl_matrix[3][0] = m.unk_3ac.x;
    m.cscl_matrix[3][1] = m.unk_3ac.y;
    m.cscl_matrix[3][2] = m.unk_3ac.z;
    if recurse {
        if let Some(b) = models.brother(node) {
            update_cscaling_info(models, father, b, true);
        }
        if let Some(s) = models.son(node) {
            update_cscaling_info(models, Some(node), s, true);
        }
    }
}

/// Recomputes a node's global values from its local ones and its father's global ones;
/// with `recurse`, then those of its **following brothers** and of its subtree
/// [soft 0x084662f8 SI_modelUpdateSonState]. Class: libm.
///
/// `father` is the node whose global values are used; the original takes it as an argument
/// and does not look at the node's own `father` link for this (`None`: a root).
///
/// **The usual method** (scaling kept apart from rotation):
///
/// ```text
///   hscl, hrot, htrs = father's gscl, grot, gtrs      (1, identity, 0 for a root)
///   mrot  = rotation matrix of vrot (X, then Y, then Z)
///   scal  = each factor smaller than 1e-5 replaced by ±1e-5
///   gscl  = hscl × scal, component by component
///   grot  = mrot × hrot                               (3 × 3 part; see below)
///   gtrs  = trans × father's gmat                     (trans itself for a root)
///   gmat  = rows of grot times the factors of gscl, gtrs in row 3
/// ```
///
/// So a node's global matrix is *not* the product of the local matrices above it: a
/// father's non-uniform scaling scales a son along the son's own rotated axes and never
/// shears it. Only the position goes through the father's matrix.
///
/// **The classical method**: `gmat` = local matrix × father's `gmat` (the plain product),
/// `gtrs` = its row 3, with `hscl` = 1 and `grot` = `mrot` × father's `grot`; the scaling
/// bookkeeping is [`update_cscaling_info`]'s.
///
/// An effector whose chain root has inverse kinematics of kind 1 keeps its own orientation:
/// `grot` = `mrot` (usual method), or [`crate::setters::toggle_eff_rot_constraint`]
/// (classical).
///
/// Then, by either method: if the sign of the product of the three global factors changed,
/// bit 0x10 of the second flag word is set (never cleared here); `def_matrix` (+0x68)
/// becomes `gmat` × (father's `gmat`)⁻¹, the node's matrix relative to its father (or
/// `gmat` for a root); and a node whose geometry lives in another node's space has it
/// readjusted ([`crate::node_copy::readjust_definition`]).
///
/// The fourth column of `gmat` is never written by the usual method.
///
/// **Order of the walk** with `recurse`: the node, then the call for its next brother (which
/// goes on to that brother's brothers and subtree), then the call for its first son. With
/// bit 0x02 of the viewer option instead: the node, then each of its sons in order, each
/// with its subtree, and no brothers.
pub fn update_son_state(models: &mut Models, father: Option<NodeId>, node: NodeId, recurse: bool, g: &Globals, hooks: &mut dyn Env) {
    models.m_mut(node).flags_08 |= BUSY;
    if models.m(node).kind == MBALL {
        hooks.metaball_moved(models, node);
    }
    let fm = father.map(|f| *models.m(f));
    let old;
    let new;
    // the local matrix of the classical method stays in the stack slot that the inverse of
    // the father's matrix is later written to
    let mut scratch: Matrix = [[0.0; 4]; 4];
    if g.classical_scaling {
        old = models.m(node).cscl;
        update_cscaling_info(models, father, node, false);
        new = models.m(node).cscl;
        {
            let m = models.m_mut(node);
            m.hscl = Vector { x: 1.0, y: 1.0, z: 1.0 };
            match &fm {
                None => {
                    m.hrot = IDENTITY;
                    m.htrs = Vector { x: 0.0, y: 0.0, z: 0.0 };
                }
                Some(f) => {
                    m.hrot = f.grot;
                    m.htrs = f.gtrs;
                }
            }
            m.mrot = matrix_xyz_rotation(&mv(m.vrot));
            m.grot = matrix_multiply_sub3(&m.mrot, &m.hrot);
        }
        clamp_scaling(models, node);
        let m = models.m_mut(node);
        let (rot, s) = (m.mrot, row3(m.scal));
        scaled_rows(&rot, s, &mut scratch);
        for row in scratch.iter_mut().take(3) {
            row[3] = 0.0;
        }
        scratch[3] = [m.trans.x, m.trans.y, m.trans.z, 1.0];
        m.gmat = match &fm {
            None => scratch,
            Some(f) => matrix_multiply(&scratch, &f.gmat),
        };
        m.gtrs = Vector { x: m.gmat[3][0], y: m.gmat[3][1], z: m.gmat[3][2] };
        if effector_keeps_orientation(models, node) {
            crate::setters::toggle_eff_rot_constraint(models, node);
        }
        if let Some(f) = father {
            if !is_chain_node(models, f) && !is_chain_node(models, node) && models.m(node).flags_08 & 0x2000 != 0 {
                hooks.compensate_local_coord(models, f, node);
            }
        }
    } else {
        {
            let m = models.m_mut(node);
            match &fm {
                None => {
                    m.hscl = Vector { x: 1.0, y: 1.0, z: 1.0 };
                    m.hrot = IDENTITY;
                    m.htrs = Vector { x: 0.0, y: 0.0, z: 0.0 };
                }
                Some(f) => {
                    m.hscl = f.gscl;
                    m.hrot = f.grot;
                    m.htrs = f.gtrs;
                }
            }
            m.mrot = matrix_xyz_rotation(&mv(m.vrot));
        }
        clamp_scaling(models, node);
        let keeps = effector_keeps_orientation(models, node);
        let m = models.m_mut(node);
        old = m.gscl;
        new = Vector { x: mul(m.hscl.x, m.scal.x), y: mul(m.hscl.y, m.scal.y), z: mul(m.hscl.z, m.scal.z) };
        m.gscl = new;
        m.grot = if keeps { m.mrot } else { matrix_multiply_sub3(&m.mrot, &m.hrot) };
        m.gtrs = match &fm {
            None => m.trans,
            Some(f) => point_through(m.trans, &f.gmat),
        };
        let (rot, s) = (m.grot, row3(m.gscl));
        scaled_rows(&rot, s, &mut m.gmat);
        m.gmat[3][0] = m.gtrs.x;
        m.gmat[3][1] = m.gtrs.y;
        m.gmat[3][2] = m.gtrs.z;
    }
    // mirrored since the last update? the six factors multiplied in the register
    let product = ((((x(old.x) * new.x) * old.y) * new.y) * old.z) * new.z;
    if models.m(node).update_flags2 & UPD_MIRROR == 0 && product.lt(x(0.0f32)) {
        models.m_mut(node).update_flags2 |= UPD_MIRROR;
    }
    if g.viewer_upd_opt & 0x10 == 0 {
        let gmat = models.m(node).gmat;
        models.m_mut(node).def_matrix = match &fm {
            None => gmat,
            Some(f) => {
                // SI_matrixInverse writes nothing when the father's matrix has no inverse:
                // the product then uses what the stack slot held, the node's local matrix
                // by the classical method and whatever was there by the usual one (zeros
                // here).
                let inverse = matrix_inverse(&f.gmat).unwrap_or_else(|| {
                    crate::env::stale::note();
                    scratch
                });
                matrix_multiply(&gmat, &inverse)
            }
        };
    }
    let m = models.m(node);
    if m.algo_type != 0x1e && m.latdep != Models::ptr(node) {
        let holder_busy = Models::id(m.latdep).is_some_and(|l| models.m(l).flags_08 & BUSY != 0);
        if !holder_busy {
            let hold = if matches!(m.envelop, 5 | 6 | 8) { g.envelope_hold } else { 0 };
            if hold == 0 {
                crate::node_copy::readjust_definition(models, node, g, hooks);
            }
        }
    }
    if recurse {
        if g.viewer_upd_opt & 2 == 0 {
            if let Some(b) = models.brother(node) {
                update_son_state(models, father, b, true, g, hooks);
            }
            if let Some(s) = models.son(node) {
                update_son_state(models, Some(node), s, true, g, hooks);
            }
        } else {
            let mut at = models.son(node);
            while let Some(s) = at {
                update_son_state(models, Some(node), s, true, g, hooks);
                at = models.brother(s);
            }
        }
    }
    models.m_mut(node).flags_08 &= !BUSY;
}

// ------------------------------------------------------------------ geometry through a matrix

pub(crate) fn key_through(p: [f32; 3], m: &Matrix) -> [f32; 3] {
    let v = point_through(Vector { x: p[0], y: p[1], z: p[2] }, m);
    [v.x, v.y, v.z]
}

fn spline_through(s: &mut si_hrc::Spline, m: &Matrix) {
    for k in &mut s.keys {
        if s.kind == 1 {
            k.prev = key_through(k.prev, m);
            k.pos = key_through(k.pos, m);
            k.next = key_through(k.next, m);
        } else {
            k.pos = key_through(k.pos, m);
        }
    }
}

/// One definition of the given kind through a matrix [soft 0x0846e918 SI_modelMultiplyDef].
/// Class: model.
///
/// - **mesh** (4): [`mesh_transform`] with the matrix and the transpose of its inverse
///   (`SI_matrixTransposeInverse`, computed first whatever the kind).
/// - **spline** (6), and the outline and holes of a spline **face** (2): every key position
///   as `((y·m₁ⱼ + x·m₀ⱼ) + z·m₂ⱼ) + m₃ⱼ`; of a spline of type 1 also the two tangent
///   points of each key, of any other type the positions alone.
/// - a **face** of NURBS curves: each control point's x, y, z are rounded from double to
///   float, sent through the matrix (x as `((x·m₀₀ + y·m₁₀) + z·m₂₀) + m₃₀`, y and z in
///   the order above) and stored as doubles; the weight is left.
/// - **patch** (1): the points through `SI_hvectorMultiplyMatrix` (their fourth component
///   too, which `si-hrc`'s patch does not keep: it is taken as 1 and dropped), and the two
///   evaluated surface arrays, which are not kept here.
/// - **NURBS curve and surface** (9, 10): their own routines (a hook).
/// - any other kind: nothing.
pub fn multiply_def(m: &Matrix, kind: i32, def: &mut Definition, hooks: &mut dyn GeometryHooks) {
    use si_math::algebra::{hvector_multiply_matrix, matrix_transpose_inverse};
    use si_math::types::Hvector;
    let (normals, _) = matrix_transpose_inverse(m);
    match (kind, def) {
        (1, Definition::Patch(p)) => {
            for q in &mut p.points {
                let h = hvector_multiply_matrix(&Hvector::new(q[0], q[1], q[2], 1.0), m);
                *q = [h.x, h.y, h.z];
            }
        }
        (2, Definition::Face(f)) => {
            if f.is_spline == 0 {
                for c in &mut f.nurbs {
                    for p in &mut c.points {
                        let v = [p.xyzw[0] as f32, p.xyzw[1] as f32, p.xyzw[2] as f32];
                        p.xyzw[0] = (((x(v[0]) * m[0][0] + x(v[1]) * m[1][0]) + x(v[2]) * m[2][0]) + m[3][0]).to_f64();
                        p.xyzw[1] = (((x(v[1]) * m[1][1] + x(v[0]) * m[0][1]) + x(v[2]) * m[2][1]) + m[3][1]).to_f64();
                        p.xyzw[2] = (((x(v[1]) * m[1][2] + x(v[0]) * m[0][2]) + x(v[2]) * m[2][2]) + m[3][2]).to_f64();
                    }
                }
            } else {
                spline_through(&mut f.exterior, m);
                for h in &mut f.holes {
                    spline_through(h, m);
                }
            }
        }
        (4, Definition::Mesh(mesh)) => mesh_transform(mesh, m, &normals),
        (6, Definition::Spline(sp)) => spline_through(sp, m),
        (9 | 10, d) => hooks.transform(kind, d, m),
        _ => {}
    }
}

/// Every geometry of a node through a matrix [soft 0x0846d3a4 SI_modelMultiplyDefinition]:
/// the stored definitions, the working one and the original one, in the order of
/// `SI_modelNextDefGet` (in which a node that works on a stored definition other than the
/// one numbered `cur_def` meets that definition twice, and transforms it twice). A mesh goes through [`mesh_transform`]; a metaball (kind 8) has its
/// own matrix multiplied by `m` on the right; patch, face, spline and NURBS go through
/// their own routines (hooks; note that this routine does **not** use `SI_modelMultiplyDef`
/// for them). Afterwards the bounding box is recomputed if the node has a valid one (a
/// hook), and the geometry is marked as touched ([`crate::node_copy::touch_geometry`]).
/// Class: model.
pub fn multiply_definition(models: &mut Models, node: NodeId, m: &Matrix, g: &Globals, hooks: &mut dyn Env) {
    let (normals, _) = si_math::algebra::matrix_transpose_inverse(m);
    let kind = models.m(node).kind;
    let mut cursor = 0;
    while let Some(slot) = next_def_get(models, node, &mut cursor) {
        let d = &mut models.data[node];
        let def = match slot {
            DefSlot::Stored(i) => d.defs.get_mut(i),
            // the working definition comes up in the walk whenever it is not the stored one
            // numbered `cur_def`: a node working on another stored definition has that one
            // transformed a second time
            DefSlot::Current => match &mut d.current {
                Current::Own(b) => Some(&mut **b),
                Current::Def(i) => d.defs.get_mut(*i),
                Current::None => None,
            },
            DefSlot::Original => d.org_def.as_deref_mut(),
        };
        let Some(def) = def else { continue };
        match (kind, def) {
            (4, Definition::Mesh(mesh)) => mesh_transform(mesh, m, &normals),
            (8, Definition::Metaball(b)) => {
                let mut own = [[0.0f32; 4]; 4];
                for i in 0..16 {
                    own[i / 4][i % 4] = b.matrix[i];
                }
                let out = matrix_multiply(&own, m);
                for i in 0..16 {
                    b.matrix[i] = out[i / 4][i % 4];
                }
            }
            (1 | 2 | 6 | 9 | 10, def) => hooks.transform(kind, def, m),
            _ => {}
        }
    }
    if models.m(node).bbox_valid != 0 {
        hooks.compute_bounding(models, node);
    }
    // (the touch readjusts a deformed node, which needs the program-wide settings; the
    // original reads them from globals)
    crate::node_copy::touch_geometry(models, node, g, hooks);
}

// ------------------------------------------------------------------ freezing

fn is_chain_null(models: &Models, n: NodeId, up_to: i32) -> bool {
    let m = models.m(n);
    m.kind == MNILL && (1..=up_to).contains(&m.algo_type)
}

fn reset_init(models: &mut Models, node: NodeId, g: &Globals, which: usize, value: f32) {
    if g.init_pose_compensate {
        if let Some(srt) = models.data[node].init_srt.as_mut() {
            srt[which] = Vector { x: value, y: value, z: value };
        }
    }
}

fn freeze_end(models: &mut Models, node: NodeId, recurse: bool, g: &Globals, env: &mut dyn Env) {
    crate::setters::compensate_matrix(models, node, 0, 1, g, env);
    let father = models.father(node);
    update_son_state(models, father, node, recurse, g, env);
}

/// Resets a node's local transformation [soft 0x0846ce38 SI_modelFreeze]: scaling 1 always;
/// rotation 0 (angles and matrix) and translation 0 unless the node is a joint or a chain
/// null (algorithm 1 to 4). With `SI_initPoseCompensate` the same parts of the initial
/// transformation are reset. Then the textures ([`crate::setters::texture_freeze`]), the
/// compensation ([`crate::setters::compensate_matrix`] with mode 0 and the geometry flag)
/// and the update. The update flag words are not touched.
///
/// "Freeze" here resets the values; the geometry is multiplied by the matrix the node had
/// inside the compensation, which compares the global matrix before and after its own
/// update.
pub fn freeze(models: &mut Models, node: NodeId, g: &Globals, env: &mut dyn Env) {
    models.m_mut(node).scal = Vector { x: 1.0, y: 1.0, z: 1.0 };
    reset_init(models, node, g, 0, 1.0);
    if !is_chain_null(models, node, 4) && models.m(node).kind != MJNT {
        let m = models.m_mut(node);
        m.vrot = Vector { x: 0.0, y: 0.0, z: 0.0 };
        m.mrot = IDENTITY;
        m.trans = Vector { x: 0.0, y: 0.0, z: 0.0 };
        reset_init(models, node, g, 1, 0.0);
        reset_init(models, node, g, 2, 0.0);
    }
    crate::setters::texture_freeze(models, node, true, true, true, g, env);
    freeze_end(models, node, true, g, env);
}

/// Resets the scaling [soft 0x0846d190 SI_modelFreezeScaling]. Under the classical scaling
/// method the update that follows does not go on to brothers and sons.
pub fn freeze_scaling(models: &mut Models, node: NodeId, g: &Globals, env: &mut dyn Env) {
    models.m_mut(node).scal = Vector { x: 1.0, y: 1.0, z: 1.0 };
    reset_init(models, node, g, 0, 1.0);
    crate::setters::texture_freeze(models, node, false, true, false, g, env);
    freeze_end(models, node, !g.classical_scaling, g, env);
}

/// Resets the rotation [soft 0x0846d244 SI_modelFreezeRotation]; nothing at all for a joint
/// or a chain root (a null with algorithm 1 or 2).
pub fn freeze_rotation(models: &mut Models, node: NodeId, g: &Globals, env: &mut dyn Env) {
    if is_chain_null(models, node, 2) || models.m(node).kind == MJNT {
        return;
    }
    let m = models.m_mut(node);
    m.vrot = Vector { x: 0.0, y: 0.0, z: 0.0 };
    m.mrot = IDENTITY;
    reset_init(models, node, g, 1, 0.0);
    crate::setters::texture_freeze(models, node, false, false, true, g, env);
    freeze_end(models, node, true, g, env);
}

/// Resets the translation [soft 0x0846d304 SI_modelFreezeTranslation]; nothing at all for a
/// joint or a chain null (algorithm 1 to 4).
pub fn freeze_translation(models: &mut Models, node: NodeId, g: &Globals, env: &mut dyn Env) {
    if is_chain_null(models, node, 4) || models.m(node).kind == MJNT {
        return;
    }
    models.m_mut(node).trans = Vector { x: 0.0, y: 0.0, z: 0.0 };
    reset_init(models, node, g, 2, 0.0);
    crate::setters::texture_freeze(models, node, true, false, false, g, env);
    freeze_end(models, node, true, g, env);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    fn two_levels() -> (Models, NodeId, NodeId) {
        let mut m = Models::default();
        let (a, b) = (m.allocate(None), m.allocate(None));
        m.arena.create_parentship(a, b);
        (m, a, b)
    }

    #[test]
    fn local_matrix_is_scaled_rotation_rows_and_translation() {
        let (mut m, a, _) = two_levels();
        let r = m.m_mut(a);
        r.scal = v(2.0, 3.0, 4.0);
        r.trans = v(5.0, 6.0, 7.0);
        r.mrot = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        update_matrix(&mut m, a);
        assert_eq!(m.m(a).gmat, [[0.0, 2.0, 0.0, 0.0], [-3.0, 0.0, 0.0, 0.0], [0.0, 0.0, 4.0, 0.0], [5.0, 6.0, 7.0, 1.0]]);
        assert_eq!(state(&m, a), (v(2.0, 3.0, 4.0), v(0.0, 0.0, 0.0), v(5.0, 6.0, 7.0)));
    }

    #[test]
    fn scaling_goes_down_by_components_and_position_through_the_fathers_matrix() {
        let (mut m, a, b) = two_levels();
        let g = Globals::default();
        set_state(&mut m, b, v(0.5, 1.0, 1.0), v(0.0, 0.0, 0.0), v(1.0, 1.0, 1.0), &g, &mut ());
        set_state(&mut m, a, v(2.0, 3.0, 4.0), v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), &g, &mut ());
        // the father
        assert_eq!(m.m(a).gscl, v(2.0, 3.0, 4.0));
        assert_eq!(m.m(a).gmat[3], [10.0, 0.0, 0.0, 1.0]);
        assert_eq!(m.m(a).def_matrix, m.m(a).gmat);
        // the son: gscl = father's gscl × own, position through the father's matrix
        assert_eq!(m.m(b).hscl, v(2.0, 3.0, 4.0));
        assert_eq!(m.m(b).gscl, v(1.0, 3.0, 4.0));
        assert_eq!(m.m(b).gtrs, v(12.0, 3.0, 4.0));
        assert_eq!(m.m(b).gmat, [[1.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 4.0, 0.0], [12.0, 3.0, 4.0, 1.0]]);
        // relative to its father: its local matrix again
        assert_eq!(m.m(b).def_matrix, [[0.5, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 1.0, 1.0, 1.0]]);
        assert_eq!(global_state(&m, b, &g), (v(1.0, 3.0, 4.0), v(0.0, 0.0, 0.0), v(12.0, 3.0, 4.0)));
        assert_eq!(m.m(b).flags_08 & BUSY, 0);
    }

    #[test]
    fn a_fathers_uneven_scaling_does_not_shear_a_rotated_son() {
        let (mut m, a, b) = two_levels();
        let quarter = core::f32::consts::FRAC_PI_2;
        let g = Globals::default();
        set_state(&mut m, a, v(2.0, 1.0, 1.0), v(0.0, 0.0, 0.0), v(0.0, 0.0, 0.0), &g, &mut ());
        set_state(&mut m, b, v(1.0, 1.0, 1.0), v(0.0, 0.0, quarter), v(0.0, 0.0, 0.0), &g, &mut ());
        // usual method: the son's rows are its rotation's rows times (2, 1, 1): still
        // perpendicular
        let gm = m.m(b).gmat;
        let dot = gm[0][0] * gm[1][0] + gm[0][1] * gm[1][1] + gm[0][2] * gm[1][2];
        assert!(dot.abs() < 1e-6);
        assert!((gm[0][1] - 2.0).abs() < 1e-6 && (gm[1][0] + 1.0).abs() < 1e-6);
        // classical method: the plain product, which scales the world x axis instead
        let c = Globals { classical_scaling: true, ..Globals::default() };
        update_son_state(&mut m, None, a, true, &c, &mut ());
        let gm = m.m(b).gmat;
        assert!((gm[0][1] - 1.0).abs() < 1e-6 && (gm[1][0] + 2.0).abs() < 1e-6);
        assert_eq!(m.m(b).cscl, v(2.0, 1.0, 1.0));
        assert_eq!(global_state(&m, b, &c).0, v(1.0, 1.0, 1.0));
    }

    #[test]
    fn tiny_scaling_is_floored_and_a_mirror_is_flagged() {
        let (mut m, a, _) = two_levels();
        let g = Globals::default();
        m.m_mut(a).update_flags2 = 0;
        set_state(&mut m, a, v(0.0, -1e-7, 2.0), v(0.0, 0.0, 0.0), v(0.0, 0.0, 0.0), &g, &mut ());
        assert_eq!(m.m(a).scal, v(SCALING_FLOOR, -SCALING_FLOOR, 2.0));
        // gscl went from (1, 1, 1) to one negative factor
        assert_eq!(m.m(a).update_flags2 & UPD_MIRROR, UPD_MIRROR);
        assert_eq!(m.m(a).update_flags & 0xe, 0xe);
        // 1e-5 itself is not below the limit as a double, but the float nearest to it is
        let (mut m, a, _) = two_levels();
        set_scaling(&mut m, a, v(1e-5, 1.0, 1.0), 0, 0, &g, &mut ());
        assert_eq!(m.m(a).scal.x, SCALING_FLOOR);
    }

    #[test]
    fn freezing_resets_values_and_spares_skeleton_nodes() {
        let (mut m, a, b) = two_levels();
        let g = Globals { init_pose_compensate: true, ..Globals::default() };
        set_state(&mut m, a, v(2.0, 2.0, 2.0), v(0.5, 0.0, 0.0), v(1.0, 2.0, 3.0), &g, &mut ());
        m.data[a].init_srt = Some([v(4.0, 4.0, 4.0), v(1.0, 1.0, 1.0), v(7.0, 7.0, 7.0)]);
        let flags = m.m(a).update_flags;
        freeze(&mut m, a, &g, &mut ());
        assert_eq!(state(&m, a), (v(1.0, 1.0, 1.0), v(0.0, 0.0, 0.0), v(0.0, 0.0, 0.0)));
        assert_eq!(m.m(a).gmat, IDENTITY);
        assert_eq!(m.data[a].init_srt, Some([v(1.0, 1.0, 1.0), v(0.0, 0.0, 0.0), v(0.0, 0.0, 0.0)]));
        assert_eq!(m.m(a).update_flags, flags);
        // a chain root keeps rotation and translation; only its scaling is reset
        set_state(&mut m, b, v(3.0, 3.0, 3.0), v(0.0, 0.25, 0.0), v(1.0, 0.0, 0.0), &g, &mut ());
        m.m_mut(b).algo_type = 1;
        freeze(&mut m, b, &g, &mut ());
        assert_eq!(state(&m, b), (v(1.0, 1.0, 1.0), v(0.0, 0.25, 0.0), v(1.0, 0.0, 0.0)));
        freeze_rotation(&mut m, b, &g, &mut ());
        assert_eq!(m.m(b).vrot, v(0.0, 0.25, 0.0));
        m.m_mut(b).algo_type = 3; // an effector: rotation may be frozen, translation not
        freeze_translation(&mut m, b, &g, &mut ());
        assert_eq!(m.m(b).trans, v(1.0, 0.0, 0.0));
    }

    #[test]
    fn a_spline_goes_through_a_matrix_by_its_type() {
        let key = si_hrc::SplineKey { prev: [0.0, 0.0, 0.0], pos: [1.0, 2.0, 3.0], next: [2.0, 2.0, 2.0], ..si_hrc::SplineKey::default() };
        let shift = [[2.0, 0.0, 0.0, 0.0], [0.0, 2.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0], [1.0, 1.0, 1.0, 1.0]];
        for (kind, prev, next) in [(1, [1.0, 1.0, 1.0], [5.0, 5.0, 5.0]), (0, [0.0, 0.0, 0.0], [2.0, 2.0, 2.0])] {
            let mut d = Definition::Spline(si_hrc::Spline { kind, keys: vec![key], ..si_hrc::Spline::default() });
            multiply_def(&shift, 6, &mut d, &mut ());
            let Definition::Spline(s) = d else { unreachable!() };
            assert_eq!((s.keys[0].prev, s.keys[0].pos, s.keys[0].next), (prev, [3.0, 5.0, 7.0], next));
        }
    }

    /// Notes the order in which nodes are readjusted: the routine asks for each node's
    /// element.
    struct Order(Vec<NodeId>);
    impl Hooks for Order {
        fn ed_get_elem_from_model(&mut self, _models: &Models, node: NodeId) -> u32 {
            self.0.push(node);
            0
        }
    }
    impl GeometryHooks for Order {}
    impl crate::hierarchy::Hooks for Order {}
    impl crate::node_copy::Hooks for Order {}

    #[test]
    fn an_update_runs_on_through_the_following_brothers() {
        // root(0): a(1) [c(3)], b(2)
        let mut m = Models::default();
        for _ in 0..5 {
            m.allocate(None);
        }
        for (f, s) in [(0, 1), (0, 2), (1, 3)] {
            m.arena.create_parentship(f, s);
        }
        // make every node report through the readjust hook: its geometry "lives" in node 4
        for n in 0..4 {
            m.m_mut(n).latdep = Models::ptr(4);
        }
        let mut order = Order(Vec::new());
        let g = Globals::default();
        update_son_state(&mut m, Some(0), 1, true, &g, &mut order);
        assert_eq!(order.0, [1, 2, 3]); // the node, its brother, then its son
        let mut order = Order(Vec::new());
        let sons_only = Globals { viewer_upd_opt: 2, ..g };
        update_son_state(&mut m, Some(0), 1, true, &sons_only, &mut order);
        assert_eq!(order.0, [1, 3]);
        let mut order = Order(Vec::new());
        update_son_state(&mut m, None, 0, false, &g, &mut order);
        assert_eq!(order.0, [0]);
    }
}
