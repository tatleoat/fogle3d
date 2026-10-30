// SPDX-License-Identifier: MIT
//! Joint rotation limits as the solver uses them: six routines at the end of the
//! original's `constraint.c` (finding 0022, "Rotation limits").
//!
//! A joint's limits are the data of its element's `CnsRotLim` relation (id 103, 0x67).
//! The routines reach it through the scene dictionary in three calls
//! (`SI_edGetElemFromModel`, `SI_elemdictFindRelatedElem`, `SI_elemdictGetCurRelation`);
//! here those are the two [`Host`] methods `find_rotlim` and `cur_rotlim`, called as often
//! and in the order the original calls them.

use si_model::node::{Models, NodeId};
use si_x87::{fmov, x};

use crate::iktools::son;
use crate::rec::IkStore;
use crate::skeleton::ske_chain_root;
use crate::{fault, Host};

/// The double 0.05 (0x0864d0e8) and its float forms (0x0864d0f0, 0x0864d0f4): the smallest
/// bend a preferred angle is allowed to have.
const MIN_BEND: f64 = 0.05;
const MIN_BEND_F32: f32 = 0.050_000_000_745_058_06;

/// Marks which joints of a limb start from their preferred angles
/// [soft 0x081f9bc4 SI_jntRotlimSetFlag]: `use_pref` of each of the `n` joints from `node`
/// becomes 1, then 0 again when the joint has a rotation limit whose data has any of its
/// six "active" words set.
pub fn jnt_rotlim_set_flag(models: &mut Models, ik: &mut IkStore, node: NodeId, n: i32, host: &mut dyn Host) {
    let mut p = node;
    for i in 0..n.max(0) {
        ik.force(models, p).use_pref = 1;
        if host.find_rotlim(models, p) {
            if let Some(rel) = host.cur_rotlim(models, p) {
                if let Some(d) = rel.data {
                    if d.max_active[0] != 0 || d.min_active[0] != 0 || d.max_active[1] != 0 || d.min_active[1] != 0 || d.max_active[2] != 0 || d.min_active[2] != 0 {
                        ik.force(models, p).use_pref = 0;
                    }
                }
            }
        }
        if i + 1 < n {
            let Some(s) = son(models, p) else { return };
            p = s;
        }
    }
}

/// Whether a limb has a rotation limit [soft 0x081f9cb0 SI_rotlimConstrainedBranch].
///
/// As the original has it, the loop over the `n` joints never moves on from `node`: it asks
/// about the **first joint** `n` times, so a limit further down the limb is not seen.
pub fn rotlim_constrained_branch(models: &Models, node: NodeId, n: i32, host: &mut dyn Host) -> bool {
    for _ in 0..n.max(0) {
        if host.find_rotlim(models, node) {
            return true;
        }
    }
    false
}

/// Clamps a joint's angles to its rotation limits [soft 0x081f9d2c si_checkJntRotLim] and
/// returns how many of the three were clamped. Class: copy.
///
/// Nothing happens without the relation, without data or while the relation is inactive.
/// Per angle: an upper limit in force that is not above the angle replaces it; otherwise a
/// lower limit in force that is not below the angle replaces it. (An angle equal to its
/// limit counts as clamped. A NaN is never clamped.)
///
/// The original reads the entry `SI_elemdictGetCurRelation` returns without testing the
/// pointer; a null one is "no limits" here and counted as a fault.
pub fn check_jnt_rot_lim(models: &mut Models, node: NodeId, host: &mut dyn Host) -> i32 {
    if !host.find_rotlim(models, node) {
        return 0;
    }
    let Some(rel) = host.cur_rotlim(models, node) else {
        fault::note();
        return 0;
    };
    let Some(d) = rel.data else { return 0 };
    if rel.active == 0 {
        return 0;
    }
    let mut count = 0;
    let m = models.m_mut(node);
    let (max, min) = ([d.max.x, d.max.y, d.max.z], [d.min.x, d.min.y, d.min.z]);
    let angles = [m.vrot.x, m.vrot.y, m.vrot.z];
    let mut out = angles;
    for k in 0..3 {
        let limit = if d.max_active[k] != 0 && x(max[k]).le(x(angles[k])) {
            Some(max[k])
        } else if d.min_active[k] != 0 && x(min[k]).ge(x(angles[k])) {
            Some(min[k])
        } else {
            None
        };
        if let Some(l) = limit {
            out[k] = fmov(l);
            count += 1;
        }
    }
    m.vrot.x = out[0];
    m.vrot.y = out[1];
    m.vrot.z = out[2];
    count
}

/// Clamps the joints of a limb until one is found at a limit
/// [soft 0x081f9e94 SI_checkRotationLimit].
///
/// The joints from `*node` are clamped in turn ([`check_jnt_rot_lim`]). At the first one
/// that had an angle clamped, `*node` becomes the node below it and `*n` the number of
/// joints left below; the remaining joints are clamped too, and the count of that first
/// joint's clamped angles is returned. Without any, `*node` and `*n` stay and the result
/// is 0.
///
/// With `*n` ≤ 0 the original returns a local it never set; 0 here.
pub fn check_rotation_limit(models: &mut Models, node: &mut NodeId, n: &mut i32, host: &mut dyn Host) -> i32 {
    let total = *n;
    let mut p = *node;
    let mut i = 0;
    while i < total {
        let lim = check_jnt_rot_lim(models, p, host);
        if lim != 0 {
            *n = (*n - i) - 1;
            // the original stores the son pointer whatever it is; a null one (a limb that
            // ends at the clamped joint) leaves `*node` at that joint here, with `*n` 0 or
            // less, so that no caller goes on
            let Some(next) = models.son(p) else {
                if *n > 0 {
                    fault::note();
                    *n = 0;
                }
                return lim;
            };
            *node = next;
            p = next;
            for k in (i + 1)..total {
                check_jnt_rot_lim(models, p, host);
                if k + 1 < total {
                    let Some(s) = son(models, p) else { break };
                    p = s;
                }
            }
            return lim;
        }
        i += 1;
        if i < total {
            let Some(next) = son(models, p) else { return 0 };
            p = next;
        }
    }
    0
}

/// The number of joints of a limb that have all three angles at a limit
/// [soft 0x081f9f28 SI_constrainedJnt]. It clamps as it counts ([`check_jnt_rot_lim`]).
pub fn constrained_jnt(models: &mut Models, node: NodeId, n: i32, host: &mut dyn Host) -> i32 {
    let mut count = 0;
    let mut p = node;
    for i in 0..n.max(0) {
        if check_jnt_rot_lim(models, p, host) == 3 {
            count += 1;
        }
        if i + 1 < n {
            match son(models, p) {
                Some(s) => p = s,
                None => break,
            }
        }
    }
    count
}

/// Brings a joint's preferred angles inside its rotation limits
/// [soft 0x081f9f6c invKinPrefAngleUpdate]. Class: copy.
///
/// Needs the joint's record, the relation and its data. A preferred angle above an active
/// upper bound becomes that bound, else one below an active lower bound becomes that: x
/// always, y only in a spatial chain (root type 40), z always. For z a bound smaller than
/// 0.05 in size is not taken as it is: the preferred bend becomes +0.05 for a positive
/// bound and −0.05 otherwise, so that the chain keeps a side to bend to.
pub fn inv_kin_pref_angle_update(models: &mut Models, ik: &mut IkStore, node: Option<NodeId>, host: &mut dyn Host) {
    let Some(node) = node else { return };
    if ik.of(models, node).is_none() {
        return;
    }
    if !host.find_rotlim(models, node) {
        return;
    }
    let Some(rel) = host.cur_rotlim(models, node) else {
        fault::note();
        return;
    };
    let pref = ik.of(models, node).unwrap().prot;
    let Some(d) = rel.data else { return };
    let spatial = match ske_chain_root(models, Some(node)).and_then(|r| ik.of(models, r)) {
        Some(r) => r.kind == crate::rec::CHAIN_CPX_3D,
        None => {
            fault::note();
            false
        }
    };
    let rec = ik.of_mut(models, node).unwrap();
    // x, and in a spatial chain y: the limit itself
    if d.max_active[0] != 0 && x(d.max.x).lt(x(pref.x)) {
        rec.prot.x = fmov(d.max.x);
    } else if d.min_active[0] != 0 && x(pref.x).lt(x(d.min.x)) {
        rec.prot.x = fmov(d.min.x);
    }
    if spatial {
        if d.max_active[1] != 0 && x(d.max.y).lt(x(pref.y)) {
            rec.prot.y = fmov(d.max.y);
        } else if d.min_active[1] != 0 && x(pref.y).lt(x(d.min.y)) {
            rec.prot.y = fmov(d.min.y);
        }
    }
    // z: the limit, or ±0.05 when the limit is smaller than that
    let floored = |limit: f32| -> f32 {
        if x(limit).abs().lt(x(MIN_BEND)) {
            if x(limit).gt(x(0.0f32)) { MIN_BEND_F32 } else { -MIN_BEND_F32 }
        } else {
            limit
        }
    };
    if d.max_active[2] != 0 && x(d.max.z).lt(x(pref.z)) {
        rec.prot.z = floored(d.max.z);
    } else if d.min_active[2] != 0 && !x(d.min.z).le(x(pref.z)) {
        rec.prot.z = floored(d.min.z);
    }
}
