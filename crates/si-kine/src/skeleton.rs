// SPDX-License-Identifier: MIT
//! Skeleton chains as objects: the original's `skeleton.c` (finding 0022, "Chains as
//! objects").
//!
//! A chain is a null with algorithm 1 or 2 (its *root*), below it joints (kind 5), each the
//! first son of the one before, and after the last joint a null with algorithm 3 or 4 (the
//! *effector*; the manuals' "leaf").

use si_model::node::{Models, NodeId};
use si_model::transform::update_son_state;
use si_types::geom::Vector;
use si_x87::{x, F80};

use crate::iktools::son;
use crate::{fault, Globals, Host};

/// A null with algorithm 1 or 2.
pub fn is_root(models: &Models, n: NodeId) -> bool {
    let m = models.m(n);
    m.kind == 0 && (m.algo_type == 1 || m.algo_type == 2)
}

/// A null with algorithm 3 or 4.
pub fn is_effector(models: &Models, n: NodeId) -> bool {
    let m = models.m(n);
    m.kind == 0 && (m.algo_type == 3 || m.algo_type == 4)
}

/// A root, a joint or an effector.
pub fn is_chain_node(models: &Models, n: NodeId) -> bool {
    is_root(models, n) || models.m(n).kind == 5 || is_effector(models, n)
}

/// The root of the chain a joint or an effector belongs to [soft 0x081d20c0 SI_skeChainRoot]:
/// the nearest null above it that has algorithm 1 or 2. **Any other node, a root included,
/// is answered with itself.**
///
/// The walk follows father pointers without testing them; where the original would fault
/// (a joint with no root above it) the answer is `None`, counted as a fault.
pub fn ske_chain_root(models: &Models, node: Option<NodeId>) -> Option<NodeId> {
    let mut p = node?;
    if is_root(models, p) || !(models.m(p).kind == 5 || is_effector(models, p)) {
        return Some(p);
    }
    let mut up = models.m(p).kind != 0;
    loop {
        if up {
            loop {
                match models.father(p) {
                    Some(f) => p = f,
                    None => {
                        fault::note();
                        return None;
                    }
                }
                if models.m(p).kind == 0 {
                    break;
                }
            }
        }
        let a = models.m(p).algo_type;
        if a == 1 || a == 2 {
            return Some(p);
        }
        up = true;
    }
}

/// The effector of the chain a root or a joint belongs to [soft 0x081d2120 skeChainLeaf]:
/// the nearest null below it (by first sons) with algorithm 3 or 4. Any other node, an
/// effector included, is answered with itself. `None` (counted) where the original would
/// fault on a chain without an effector.
pub fn ske_chain_leaf(models: &Models, node: Option<NodeId>) -> Option<NodeId> {
    let mut p = node?;
    if is_effector(models, p) || !(models.m(p).kind == 5 || is_root(models, p)) {
        return Some(p);
    }
    let mut step = models.m(p).kind != 0;
    loop {
        if step {
            loop {
                match models.son(p) {
                    Some(s) => p = s,
                    None => {
                        fault::note();
                        return None;
                    }
                }
                if models.m(p).kind == 0 {
                    break;
                }
            }
        }
        let a = models.m(p).algo_type;
        if a == 3 || a == 4 {
            return Some(p);
        }
        step = true;
    }
}

/// The length of a chain from its root [soft 0x081d2180 skeChainTotalLength]: over the
/// joints, the distance from each to the next node (`gtrs`), the sum **rounded to float
/// after every bone**. 0 for a node that is not a root. Class: model.
///
/// Under classical scaling a bone counts |joint's cscl.x| × the next node's trans.x instead
/// (whatever the uniform-scaling word says, unlike [`crate::iktools::limb_length`]).
pub fn ske_chain_total_length(models: &Models, root: Option<NodeId>, g: &Globals) -> F80 {
    let mut sum = F80::ZERO;
    let Some(root) = root else { return sum };
    if !is_root(models, root) {
        return sum;
    }
    let Some(mut p) = son(models, root) else { return sum };
    while !is_effector(models, p) {
        let Some(s) = son(models, p) else { return sum };
        let len = if g.model.classical_scaling {
            x(models.m(p).cscl.x).abs() * models.m(s).trans.x
        } else {
            crate::iktools::dist(models.m(p).gtrs, models.m(s).gtrs)
        };
        sum = x((len + sum).to_f32());
        p = s;
    }
    sum
}

/// The number of joints of a chain [soft 0x081d2234 si_skeJointNumber]: the nodes below
/// the root, by first sons, before the first null with algorithm 3 or 4. 0 for a node that
/// is not a root.
pub fn ske_joint_number(models: &Models, root: Option<NodeId>) -> i32 {
    let Some(root) = root else { return 0 };
    if !is_root(models, root) {
        return 0;
    }
    let mut n = 0;
    let Some(mut p) = son(models, root) else { return 0 };
    while !is_effector(models, p) {
        n += 1;
        match son(models, p) {
            Some(s) => p = s,
            None => break,
        }
    }
    n
}

/// Straightens a chain [soft 0x081d2284 si_skeChainReset]: every joint's angles become
/// zero, then the root's first son is updated in full (with its brothers and subtree).
/// The update is made for any node, a root or not.
pub fn ske_chain_reset(models: &mut Models, root: NodeId, g: &Globals, host: &mut dyn Host) {
    if is_root(models, root) {
        if let Some(mut p) = son(models, root) {
            while !is_effector(models, p) {
                models.m_mut(p).vrot = Vector { x: 0.0, y: 0.0, z: 0.0 };
                match son(models, p) {
                    Some(s) => p = s,
                    None => break,
                }
            }
        }
    }
    match models.son(root) {
        Some(s) => update_son_state(models, Some(root), s, true, &g.model, host),
        None => fault::note(),
    }
}

/// Whether Duplicate and Delete must refuse a node [soft 0x081d2300 SI_skeValidateCopyDelete]:
/// 0 (allowed as a skeleton part) for a chain node selected as a node (`state` 1), and for
/// a joint or an effector selected as a branch (`state` 3); 1 otherwise.
pub fn ske_validate_copy_delete(models: &Models, node: NodeId, state: i32) -> i32 {
    let ok = is_chain_node(models, node) && (state == 1 || (state == 3 && (is_effector(models, node) || models.m(node).kind == 5)));
    if ok { 0 } else { 1 }
}
