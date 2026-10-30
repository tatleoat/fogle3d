// SPDX-License-Identifier: MIT
//! The tools' moves of skeleton parts: the original's `sketransform.c` (finding 0022,
//! "Moving skeleton parts").
//!
//! A joint is not translated like another node. Its place is its bone: the father's
//! orientation and the length along the father's X axis. So "move this joint" becomes
//! "turn the bone above it and change its length" ([`ske_move_joint_branch`]), and "move
//! the effector" becomes "set its goal" ([`ske_effector_trans`]).

use si_math::algebra::{matrix_extract_xyz_rotation, matrix_multiply, matrix_z_rotation};
use si_model::node::{mv, tv, Models, NodeId};
use si_model::setters::{add_rotation, add_scaling, View};
use si_model::transform::{set_rotation, set_scaling, update_son_state, UPD_TRANSLATION};
use si_types::geom::Vector;
use si_x87::trig;
use si_x87::{fmov, x, F80};

use crate::invkin::{inv_kin_reset_pref_angle, inv_kin_set_goal};
use crate::iktools::{matrix_ortho_normal_inverse, matrix_zyz_euler_rotation};
use crate::rec::IkStore;
use crate::skeleton::{is_chain_node, is_effector, ske_chain_root};
use crate::{fault, Globals, Host};

/// The double 0.01 (0x0864c8d0): the shortest bone a move may leave, times the father's
/// scaling; the double 0.0001 (0x0864c8d8); the double 1e-6 (0x0864c8e0).
const MIN_BONE: f64 = 0.01;
const EPS: f64 = 1e-4;
const TINY: f64 = 1e-6;

/// Scales a skeleton node [soft 0x081d2bd0 SI_skeJointScale]: `factors` multiply the local
/// scaling (`absolute` 0, `SI_modelAddScaling`) or replace it (`SI_modelSetScaling`), both
/// with mode 1 and without geometry compensation; then the scaling is shown
/// (`SI_displayModelScal`, a host method). False, and nothing done, for a node that is
/// not a root, a joint or an effector.
pub fn ske_joint_scale(models: &mut Models, node: NodeId, factors: Vector, absolute: i16, g: &Globals, host: &mut dyn Host) -> bool {
    if !is_chain_node(models, node) {
        return false;
    }
    if absolute == 0 {
        add_scaling(models, node, factors, 1, 0, &g.model, host);
    } else {
        set_scaling(models, node, factors, 1, 0, &g.model, host);
    }
    host.display_model_scal(models, node);
    true
}

/// Moves a joint, or an effector, to a global point by turning and stretching the bone
/// above it [soft 0x081d2c50 si_skeMoveJointBranch]. Class: libm. Nothing for a node
/// without a father.
///
/// 1. d = the point − the father's position (each difference rounded to float). If d is
///    shorter than m = r32(0.01 × the father's global x scaling) **and m is below 0.01**,
///    d is stretched to the length m (to (m, 0, 0) when it is shorter than 0.0001).
/// 2. With the father's angles set to zero for the moment and the father updated (alone),
///    d is brought into the father's frame (through the transpose of its `grot`) and
///    becomes the node's local translation.
/// 3. That translation is turned by Rz(az), az = `fpatan`(y, x) (0 when both are below 1e-6
///    in size), which puts it in the XZ plane; el = `fpatan`(−z, x) of the result.
/// 4. With `turn_father`: the father's angles become those of the Z-Y-Z rotation
///    (az, el, 0) ([`matrix_zyz_euler_rotation`]): its X axis now points along d. Without:
///    the father gets its old angles back. The father is updated again.
/// 5. Unless `keep_angles`, or the node has no son, or it is an effector under a joint
///    whose record's first word is 1: the node's angles become those of its `grot` × the
///    inverse of the father's new `grot`, so that the node keeps its global orientation.
/// 6. The node's translation becomes (|translation| ÷ the father's global x scaling, 0, 0):
///    the bone's new length. Under classical scaling the division is by the father's
///    `cscl.x` with uniform scaling and left out without. The node is updated (alone).
pub fn ske_move_joint_branch(models: &mut Models, ik: &IkStore, node: NodeId, target: Vector, turn_father: i16, keep_angles: i16, g: &Globals, host: &mut dyn Host) {
    let Some(f) = models.father(node) else { return };
    let fm = *models.m(f);
    let (at, scale) = if g.classic_frame() { (fm.unk_3ac, fm.cscl.x) } else { (fm.gtrs, fm.gscl.x) };
    let mut d = [(x(target.x) - at.x).to_f32(), (x(target.y) - at.y).to_f32(), (x(target.z) - at.z).to_f32()];
    let min = (x(scale) * MIN_BONE).to_f32();
    let sum = (x(d[1]) * d[1] + x(d[0]) * d[0]) + x(d[2]) * d[2];
    let len = sum.sqrt().to_f32();
    if x(len).lt(x(min)) && x(min).lt(x(MIN_BONE)) {
        if x(EPS).gt(x(len)) {
            d = [fmov(min), 0.0, 0.0];
        } else {
            let len2 = sum.sqrt().to_f32();
            if x(len2).abs().gt(x(EPS)) {
                let k = x(min) / len2;
                d = [(x(d[0]) * k).to_f32(), (x(d[1]) * k).to_f32(), (k * d[2]).to_f32()];
            }
        }
    }
    let saved = fm.vrot;
    let saved = Vector { x: fmov(saved.x), y: fmov(saved.y), z: fmov(saved.z) };
    models.m_mut(f).vrot = Vector { x: 0.0, y: 0.0, z: 0.0 };
    let ff = models.father(f);
    update_son_state(models, ff, f, false, &g.model, host);
    let r = models.m(f).grot;
    let t2 = (x(r[0][1]) * d[1] + x(r[0][0]) * d[0]) + x(r[0][2]) * d[2];
    let t3 = (x(r[1][0]) * d[0] + x(r[1][1]) * d[1]) + x(r[1][2]) * d[2];
    let t = [t2.to_f32(), t3.to_f32(), (x(d[2]) * r[2][2] + (x(d[1]) * r[2][1] + x(d[0]) * r[2][0])).to_f32()];
    let az = if x(TINY).gt(t2.abs()) && t3.abs().lt(x(TINY)) { 0.0 } else { trig::fpatan(x(t[1]), x(t[0])).to_f32() };
    let z = matrix_z_rotation(az);
    let t4 = (x(z[0][1]) * t[1] + x(z[0][0]) * t[0]) + x(z[0][2]) * t[2];
    let ty = ((x(z[1][0]) * t[0] + x(z[1][1]) * t[1]) + x(z[1][2]) * t[2]).to_f32();
    let t5 = x(t[2]) * z[2][2] + (x(t[1]) * z[2][1] + x(t[0]) * z[2][0]);
    let t = [t4.to_f32(), ty, t5.to_f32()];
    models.m_mut(node).trans = Vector { x: t[0], y: t[1], z: t[2] };
    if turn_father != 0 {
        let el = trig::fpatan(-t5, t4).to_f32();
        let m = matrix_zyz_euler_rotation(az, el, 0.0);
        let prev = models.m(f).vrot;
        models.m_mut(f).vrot = tv(matrix_extract_xyz_rotation(&m, &mv(prev), g.model.keep_turns));
    } else {
        models.m_mut(f).vrot = saved;
    }
    update_son_state(models, ff, f, false, &g.model, host);
    let under_kept = models.m(node).algo_type == 3 && {
        match ik.of(models, f) {
            Some(r) => r.kind == 1,
            None => {
                fault::note();
                false
            }
        }
    };
    if models.son(node).is_some() && keep_angles == 0 && !under_kept {
        let inv = matrix_ortho_normal_inverse(&models.m(f).grot);
        let m = matrix_multiply(&models.m(node).grot, &inv);
        let prev = models.m(node).vrot;
        models.m_mut(node).vrot = tv(matrix_extract_xyz_rotation(&m, &mv(prev), g.model.keep_turns));
    }
    let length = |t: [f32; 3]| ((x(t[0]) * t[0] + x(t[1]) * t[1]) + x(t[2]) * t[2]).sqrt();
    let mut tx = t[0];
    if !g.model.classical_scaling {
        tx = (length(t) / models.m(f).gscl.x).to_f32();
    } else if g.uniform_scaling == 1 {
        tx = (length(t) / models.m(f).cscl.x).to_f32();
    }
    models.m_mut(node).trans = Vector { x: tx, y: 0.0, z: 0.0 };
    update_son_state(models, Some(f), node, false, &g.model, host);
}

/// Moves a skeleton node by a vector, taking its bone along
/// [soft 0x081d3174 si_skeJointBranchTranslate]. Class: libm.
///
/// The node's translation update flag is set. The target is the node's position plus the
/// vector (floats); [`ske_move_joint_branch`] moves the node there with the caller's two
/// flags, and then, for a node that is not an effector and without `keep_angles`, **moves
/// the node's first son back to where it was** (turning the node's own bone), so that only
/// the one joint moves. Under classical scaling the routine of `classic.c` does both (a
/// host method). Then the chain is updated in full from its root's first son, and the
/// preferred angles of the joints whose angles changed are set to their angles: for a
/// joint (algorithm 1) its father's, its own and, without `keep_angles`, its son's; for
/// an effector its father's.
///
/// Returns the target, which the original writes back into the caller's vector.
pub fn ske_joint_branch_translate(models: &mut Models, ik: &mut IkStore, node: NodeId, by: Vector, turn_father: i16, keep_angles: i16, g: &Globals, host: &mut dyn Host) -> Vector {
    models.m_mut(node).update_flags |= UPD_TRANSLATION;
    let mut target = by;
    if g.model.classical_scaling {
        host.classic_ske_move_joint_branch(models, ik, node, by, turn_father, keep_angles);
    } else {
        let at = models.m(node).gtrs;
        target = Vector { x: (x(at.x) + by.x).to_f32(), y: (x(at.y) + by.y).to_f32(), z: (x(at.z) + by.z).to_f32() };
        ske_move_joint_branch(models, ik, node, target, turn_father, keep_angles, g, host);
        if !is_effector(models, node) && keep_angles == 0 {
            match models.son(node) {
                Some(s) => {
                    let at = models.m(s).gtrs;
                    ske_move_joint_branch(models, ik, s, at, 1, 0, g, host);
                }
                None => fault::note(),
            }
        }
    }
    match ske_chain_root(models, Some(node)) {
        Some(root) => match models.son(root) {
            Some(s) => update_son_state(models, Some(root), s, true, &g.model, host),
            None => fault::note(),
        },
        None => fault::note(),
    }
    let reset = |models: &Models, ik: &mut IkStore, n: Option<NodeId>| match n {
        Some(n) => {
            inv_kin_reset_pref_angle(models, ik, Some(n), 0, models.m(n).vrot);
        }
        None => fault::note(),
    };
    match models.m(node).algo_type {
        1 => {
            reset(models, ik, models.father(node));
            reset(models, ik, Some(node));
            if keep_angles == 0 {
                reset(models, ik, models.son(node));
            }
        }
        3 => reset(models, ik, models.father(node)),
        _ => {}
    }
    target
}

/// Turns a skeleton node and keeps a joint's preferred angles in step
/// [soft 0x081d3320 SI_skeJointRotate].
///
/// The rotation: added (`absolute` 0, `SI_modelAddRotation`); or set, through
/// `SI_modelRotationRefMatrixSet` (a host method, `model.c`, not ported) when the rotation
/// mode (`SI_glbManip` + 0x08) is 1, 3 or 4, else through `SI_modelSetRotation`; all with
/// mode 0 and without geometry compensation. Then, for a joint with a record, its preferred
/// angles are set ([`inv_kin_reset_pref_angle`], all three): to the node's new angles when
/// the word `SI_glbManip` + 0x24 ([`Globals::pref_follows_input`]; 1 at start-up) is 0,
/// else to the angles given, plus the old preferred angles when the rotation was added.
pub fn ske_joint_rotate(models: &mut Models, ik: &mut IkStore, view: Option<&View>, node: NodeId, angles: Vector, absolute: i16, g: &Globals, host: &mut dyn Host) {
    if absolute == 0 {
        add_rotation(models, view, node, angles, 0, 0, &g.model, host);
    } else if matches!(g.model.rot_mode, 1 | 3 | 4) {
        host.model_rotation_ref_matrix_set(models, view, node, angles);
    } else {
        set_rotation(models, node, angles, 0, 0, &g.model, host);
    }
    if models.m(node).kind != 5 {
        return;
    }
    let Some(rec) = ik.of(models, node).copied() else { return };
    let a = if !g.pref_follows_input {
        let r = models.m(node).vrot;
        Vector { x: fmov(r.x), y: fmov(r.y), z: fmov(r.z) }
    } else if absolute == 0 {
        Vector { x: (x(angles.x) + rec.prot.x).to_f32(), y: (x(angles.y) + rec.prot.y).to_f32(), z: (x(angles.z) + rec.prot.z).to_f32() }
    } else {
        Vector { x: fmov(angles.x), y: fmov(angles.y), z: fmov(angles.z) }
    };
    inv_kin_reset_pref_angle(models, ik, Some(node), 0, a);
}

/// Moves an effector: sets its goal [soft 0x081d3428 SI_skeEffectorTrans]. Class: single.
///
/// `absolute`: the goal is the point. Otherwise the vector is added to the effector's
/// present goal if it has one (a record whose state is not 0) and `from_position` is 0,
/// else to its position (`gtrs`). The solve happens at the next update
/// ([`crate::invkin::inv_kin_set_goal`] sets the state to 1).
pub fn ske_effector_trans(models: &mut Models, ik: &mut IkStore, node: NodeId, by: Vector, absolute: i16, from_position: i16) {
    let goal = if absolute == 0 {
        let base = match ik.of(models, node) {
            Some(r) if from_position == 0 && r.state != 0 => r.goal,
            _ => models.m(node).gtrs,
        };
        Vector { x: (x(base.x) + by.x).to_f32(), y: (x(base.y) + by.y).to_f32(), z: (x(base.z) + by.z).to_f32() }
    } else {
        Vector { x: fmov(by.x), y: fmov(by.y), z: by.z }
    };
    let _ = F80::ZERO;
    inv_kin_set_goal(models, ik, node, goal);
}
