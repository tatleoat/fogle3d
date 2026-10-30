// SPDX-License-Identifier: MIT
//! The inverse-kinematics record's life, goals, sub-chains and the top of the solver: the
//! original's `invkin.c` (finding 0022, "From an effector to a solved chain").
//!
//! [`model_solve_inv_kin`] is what the scene update and the tools call for an effector and
//! a goal. A chain may be cut into *sub-chains* at **break joints**: joints whose position
//! something else decides (a position constraint, a path, an expression on their
//! translation). The stretch from the last break joint above the effector is what gets
//! solved.

use si_math::matrix_gen_dbl::{general_matrix_allocate_dbl, general_matrix_copy_dbl};
use si_model::hierarchy::next_son;
use si_model::node::{Models, NodeId};
use si_types::geom::Vector;
use si_types::Ptr32;
use si_x87::{fmov, x, F80};

use crate::iktools::{dist, limb_length, son};
use crate::jacobian::{check_branch_pullout, get_insert_point, solve_partial_inv_kin};
use crate::rec::{push_tx, IkPtr, IkStore, InvKin, CHAIN_CPX_2D, CHAIN_CPX_3D, CHAIN_DBL_2D, CHAIN_SGL_2D, GOAL_NONE, GOAL_SET, GOAL_SOLVED};
use crate::rotlim::{constrained_jnt, jnt_rotlim_set_flag, rotlim_constrained_branch};
use crate::skeleton::{is_chain_node, is_effector, is_root, ske_chain_root};
use crate::{fault, Globals, Host};

/// The double 0.05 (0x08660cd0) and its float forms (0x08660cd8, 0x08660cdc): the smallest
/// preferred bend.
const MIN_BEND: f64 = 0.05;
const MIN_BEND_F32: f32 = 0.050_000_000_745_058_06;
/// The double 0.001 (0x08660ce0): a scaling factor below it counts as zero.
const NO_SCALING: f64 = 0.001;
/// The double 0.0008 (0x08660cf0): the tolerance of the re-solve loop as a fraction of the
/// limb's length; the double 1e-5 (0x08660cf8): the smallest progress worth another pass.
const TOLERANCE: f64 = 0.0008;
const PROGRESS: f64 = 1e-5;
/// The relations whose presence on a joint breaks a chain there, in the order they are
/// asked for: 111 `CnsPos`, 806, 112 `Path`, 113, 126, 160 (the names of the first and
/// third are the vendor's, finding 0006).
pub const BREAK_RELATIONS: [i32; 6] = [0x6f, 0x326, 0x70, 0x71, 0x7e, 0xa0];
/// The expression relation (20010) the second break test walks.
pub const EXPRESSION_RELATION: i32 = 0x4e2a;

/// A new record [soft 0x084ad8f0 SI_invKinAllocate]: see [`InvKin::allocate`].
pub fn inv_kin_allocate(ik: &mut IkStore) -> IkPtr {
    ik.add(InvKin::allocate())
}

/// Makes a record a chain root's [soft 0x084ad9d4 SI_invKinRootAllocate]: the joint count,
/// the chain type and the solver's two matrices.
///
/// Planar (`spatial` 0): type 10 for one joint, 20 for two (neither needs matrices), 30
/// for more with a 2 × n Jacobian and an n × 2 inverse; a count below 1 leaves the type
/// alone. Spatial: type 40 for any count, a 3 × 3n Jacobian and a 3n × 3 inverse. `loaded`
/// is cleared. Matrices a record already has are not freed (the original leaks them too).
pub fn inv_kin_root_allocate(ik: &mut IkStore, rec: IkPtr, n: i32, spatial: i16) {
    let Some(r) = ik.get_mut(rec) else { return };
    r.nb_joints = n;
    r.loaded = 0;
    let (rows, cols) = if spatial == 0 {
        match n {
            1 => {
                r.kind = CHAIN_SGL_2D;
                return;
            }
            2 => {
                r.kind = CHAIN_DBL_2D;
                return;
            }
            _ if n < 3 => return,
            _ => {
                r.kind = CHAIN_CPX_2D;
                (2, n)
            }
        }
    } else {
        r.kind = CHAIN_CPX_3D;
        (3, n.wrapping_mul(3))
    };
    let j = ik.add_matrix(general_matrix_allocate_dbl(rows, cols));
    let inv = ik.add_matrix(general_matrix_allocate_dbl(cols, rows));
    let r = ik.get_mut(rec).unwrap();
    r.jacobian = j;
    r.inverse = inv;
}

/// Frees a record with its matrices and clears the pointer
/// [soft 0x084ada60 SI_invKinDispose].
pub fn inv_kin_dispose(ik: &mut IkStore, rec: &mut IkPtr) {
    let Some(r) = ik.get(*rec).copied() else { return };
    for m in [r.jacobian, r.inverse] {
        if let Some(slot) = ik.mats.get_mut((m.0 as usize).wrapping_sub(1)) {
            *slot = None;
        }
    }
    ik.recs[rec.0 as usize - 1] = None;
    *rec = Ptr32::NULL;
}

/// A copy of a record [soft 0x084adab0 SI_invKinCopy]; null for null.
///
/// Copied: the type, the joint count, the preferred angles, both matrices (as new
/// matrices), `loaded`, `use_pref`, the saved angles, the plane constraint and the two
/// plane vectors. **Not copied**: the goal, the last step and the goal's state, which are
/// those of a new record.
pub fn inv_kin_copy(ik: &mut IkStore, rec: IkPtr) -> IkPtr {
    let Some(src) = ik.get(rec).copied() else { return Ptr32::NULL };
    let mut r = InvKin::allocate();
    r.kind = src.kind;
    r.nb_joints = src.nb_joints;
    r.prot = src.prot;
    r.jacobian = { let m = general_matrix_copy_dbl(ik.matrix(src.jacobian)); ik.add_matrix(m) };
    r.inverse = { let m = general_matrix_copy_dbl(ik.matrix(src.inverse)); ik.add_matrix(m) };
    r.loaded = src.loaded;
    r.use_pref = src.use_pref;
    r.saved_rot = src.saved_rot;
    r.cns_type = src.cns_type;
    r.cns_pos = src.cns_pos;
    r.plane_roll = src.plane_roll;
    r.plane_up = src.plane_up;
    ik.add(r)
}

/// A record from the three values a model file holds for it (token 20): the routine
/// `SI_invKinReadBinaryChunk` (0x084adb78), whose reading of the file is `si-hrc`'s. The
/// type and the preferred angles are stored; with a positive joint count the record
/// becomes a root's ([`inv_kin_root_allocate`], spatial when the stored type is 40);
/// `loaded` ends as 1.
pub fn inv_kin_from_file(ik: &mut IkStore, kind: i32, nb_joints: i32, rotation: [f32; 3]) -> IkPtr {
    let mut r = InvKin::allocate();
    r.kind = kind;
    r.prot = Vector { x: rotation[0], y: rotation[1], z: rotation[2] };
    let p = ik.add(r);
    if nb_joints > 0 {
        inv_kin_root_allocate(ik, p, nb_joints, (kind == CHAIN_CPX_3D) as i16);
    }
    ik.get_mut(p).unwrap().loaded = 1;
    p
}

/// `%g` of the C library for the values these records hold is not ported; the dump prints
/// with Rust's shortest form instead, which is the same text for the usual short decimals.
fn g(v: f32) -> String {
    format!("{}", v as f64)
}

/// The text the original's `invKinDump` (0x084adbe8) prints for a record: five lines, the
/// type, the joint count, the goal, the last step and the preferred angles; nothing for
/// null. Not cited as a port: the original prints the three vectors with `%g` and this does
/// not (see `g` above), and the routine was not run against the emulated original.
pub fn inv_kin_dump(rec: Option<&InvKin>) -> String {
    let Some(r) = rec else { return String::new() };
    format!(
        "chain type: {}\nnb joints: {}\ngoal: {} {} {}\ndrot: {} {} {}\nprot: {} {} {}\n",
        r.kind,
        r.nb_joints,
        g(r.goal.x),
        g(r.goal.y),
        g(r.goal.z),
        g(r.drot.x),
        g(r.drot.y),
        g(r.drot.z),
        g(r.prot.x),
        g(r.prot.y),
        g(r.prot.z)
    )
}

/// Whether a constraint between two objects is allowed as far as skeletons go
/// [soft 0x084adca4 SI_ikValidConstraint]: 0 (refused) when both are models (graf types 2
/// or 3), the first is a skeleton node (algorithm 1 to 4) and the second is the root of
/// its own chain; 1 otherwise.
pub fn ik_valid_constraint(models: &Models, a: NodeId, a_type: i32, b: NodeId, b_type: i32) -> i32 {
    let model = |t: i32| t == 2 || t == 3;
    if model(b_type) && model(a_type) && (1..=4).contains(&models.m(a).algo_type) && ske_chain_root(models, Some(a)) == Some(b) {
        return 0;
    }
    1
}

/// Sets a joint's preferred angles [soft 0x084adcec SI_invKinResetPrefAngle]; false for a
/// node that is not a joint with a record. Class: copy.
///
/// `group` chooses the angles: 0 all three, 1 x, 2 y, 3 z. The roll x is stored as given.
/// y is stored only in a spatial chain (root type 40) and is 0 in any other. z is stored
/// as given unless it is below 0.05 in size: then it becomes +0.05, or −0.05 for a
/// negative value or a NaN, so a joint always has a side to bend to.
pub fn inv_kin_reset_pref_angle(models: &Models, ik: &mut IkStore, node: Option<NodeId>, group: u32, angles: Vector) -> bool {
    let Some(node) = node else { return false };
    if models.m(node).kind != 5 || ik.of(models, node).is_none() {
        return false;
    }
    if group < 2 {
        ik.of_mut(models, node).unwrap().prot.x = fmov(angles.x);
    }
    if group == 0 || group == 2 {
        let spatial = match ske_chain_root(models, Some(node)).and_then(|r| ik.of(models, r)) {
            Some(r) => r.kind == CHAIN_CPX_3D,
            None => {
                fault::note();
                false
            }
        };
        ik.of_mut(models, node).unwrap().prot.y = if spatial { fmov(angles.y) } else { 0.0 };
    }
    if group == 0 || group == 3 {
        let z = if x(angles.z).abs().lt(x(MIN_BEND)) {
            match x(angles.z).compare(F80::ZERO) {
                Some(core::cmp::Ordering::Less) | None => -MIN_BEND_F32,
                _ => MIN_BEND_F32,
            }
        } else {
            fmov(angles.z)
        };
        ik.of_mut(models, node).unwrap().prot.z = z;
    }
    true
}

/// Looks for a joint without scaling in a limb whose end has none
/// [soft 0x084addc8 checkScaleBeforeIK]. Class: model.
///
/// With more than one joint (`n` > 1) and `last` (the effector's father) having all three
/// global scaling factors below 0.001 in size: the first of the joints 1 to `n` − 1 below
/// `first` whose **local** factors are all below 0.001 is returned. Such a chain is solved
/// in two parts ([`model_solve_inv_kin`]).
pub fn check_scale_before_ik(models: &Models, first: NodeId, last: NodeId, n: i32) -> Option<NodeId> {
    if n <= 1 {
        return None;
    }
    let none = |s: Vector| x(s.x).abs().lt(x(NO_SCALING)) && x(s.y).abs().lt(x(NO_SCALING)) && x(s.z).abs().lt(x(NO_SCALING));
    if !none(models.m(last).gscl) {
        return None;
    }
    let mut p = son(models, first)?;
    for i in 1..n {
        if none(models.m(p).scal) {
            return Some(p);
        }
        if i + 1 < n {
            p = son(models, p)?;
        }
    }
    None
}

/// The goal of the upper part of a chain cut at a joint without scaling
/// [soft 0x084adea8 findSubGoalBeforeIK]: the point on the line from `first` to the goal
/// that lies as far short of the goal as the effector is from `cut`
/// ([`get_insert_point`] with the two distances rounded to float). Class: model.
pub fn find_sub_goal_before_ik(models: &Models, first: NodeId, cut: NodeId, effector: NodeId, goal: Vector) -> Vector {
    let p = models.m(first).gtrs;
    let whole = dist(goal, p).to_f32();
    let rest = dist(models.m(effector).gtrs, models.m(cut).gtrs);
    get_insert_point((x(whole) - rest).to_f32(), whole, p, goal)
}

/// Solves a limb and solves it again while that helps [soft 0x084adf7c modelSolveIK].
/// Class: libm.
///
/// 1. [`jnt_rotlim_set_flag`]: which joints start from their preferred angles.
/// 2. The first pass: [`solve_partial_inv_kin`] (under classical scaling the routine of
///    `classic.c`, a host method).
/// 3. Only for a limb with a rotation limit ([`rotlim_constrained_branch`], which looks at
///    the first joint alone) or a spatial chain (root type 40): up to ten more passes, each
///    with `first_pass` 0, while not every joint is fully clamped ([`constrained_jnt`]),
///    the effector is further from the goal than r32(0.0008 × the limb's length), the
///    distance changed by more than 1e-5 since the pass before, the limb is not pulled out
///    straight at an unreachable goal ([`check_branch_pullout`]) and no pass gave up.
#[allow(clippy::too_many_arguments)]
pub fn model_solve_ik(models: &mut Models, ik: &mut IkStore, root: NodeId, first: NodeId, effector: NodeId, goal: Vector, n: i32, g: &Globals, host: &mut dyn Host) {
    let mut stop = 0i16;
    jnt_rotlim_set_flag(models, ik, first, n, host);
    let pass = |models: &mut Models, ik: &mut IkStore, first_pass: i16, stop: &mut i16, host: &mut dyn Host| {
        if g.model.classical_scaling {
            host.classic_solve_partial_inv_kin(models, ik, root, first, n, goal, first_pass, stop);
        } else {
            solve_partial_inv_kin(models, ik, Some(root), Some(first), n, goal, first_pass, stop, g, host);
        }
    };
    pass(models, ik, 1, &mut stop, host);
    let limited = rotlim_constrained_branch(models, first, n, host);
    if !limited && ik.force(models, root).kind != CHAIN_CPX_3D {
        return;
    }
    let mut off = 1000.0f32;
    let mut before = 0.0f32;
    let mut tolerance = 0.0f32;
    let mut passes = 0;
    let mut pulled = false;
    let mut clamped = constrained_jnt(models, first, n, host);
    if clamped < n {
        off = dist(goal, models.m(effector).gtrs).to_f32();
        let (len, _, _) = limb_length(models, first, n, g);
        tolerance = (len * TOLERANCE).to_f32();
        pulled = check_branch_pullout(models, ik, first, goal, n, g, host);
    }
    if stop != 0 {
        return;
    }
    while clamped < n && x(off).gt(x(tolerance)) && (x(off) - before).abs().gt(x(PROGRESS)) && !pulled {
        pass(models, ik, 0, &mut stop, host);
        passes += 1;
        clamped = constrained_jnt(models, first, n, host);
        if clamped < n {
            before = fmov(off);
            off = dist(goal, models.m(effector).gtrs).to_f32();
            pulled = check_branch_pullout(models, ik, first, goal, n, g, host);
        }
        if stop != 0 || passes > 9 {
            return;
        }
    }
}

/// Solves the chain above an effector, or above a joint, for a goal
/// [soft 0x084ae1fc SI_modelSolveInvKin]. Class: libm.
///
/// `node` must have algorithm 1 or 3 (a joint or an effector of inverse kinematics), must
/// not be a chain root and must not hang directly on one. The limb is the stretch from the
/// first joint of `node`'s sub-chain ([`inv_kin_get_sub_chain_first_jnt`], counted from
/// `node`'s father upwards) to `node`'s father. If that limb is cut by a joint without
/// scaling ([`check_scale_before_ik`]) the upper part is solved for a point short of the
/// goal ([`find_sub_goal_before_ik`]) and then the lower part for the goal; otherwise the
/// whole limb is ([`model_solve_ik`]). Afterwards the first joint's plane constraint
/// (`cns_type`) is cleared: it holds for one solve.
pub fn model_solve_inv_kin(models: &mut Models, ik: &mut IkStore, node: Option<NodeId>, goal: Vector, time: f32, g: &Globals, host: &mut dyn Host) {
    let Some(node) = node else { return };
    let a = models.m(node).algo_type;
    if is_root(models, node) || !(a == 1 || a == 3) {
        return;
    }
    let Some(last) = models.father(node) else {
        fault::note();
        return;
    };
    if is_root(models, last) {
        return;
    }
    let Some(root) = ske_chain_root(models, Some(node)) else { return };
    let (first, up) = inv_kin_get_sub_chain_first_jnt(models, Some(last), 1, time, host);
    let Some(first) = first else {
        fault::note();
        return;
    };
    let n = up + 1;
    match check_scale_before_ik(models, first, last, up) {
        None => model_solve_ik(models, ik, root, first, node, goal, n, g, host),
        Some(cut) => {
            let mut k = 0;
            let mut p = cut;
            loop {
                match models.father(p) {
                    Some(f) => p = f,
                    None => {
                        fault::note();
                        break;
                    }
                }
                k += 1;
                if p == first {
                    break;
                }
            }
            let sub = find_sub_goal_before_ik(models, first, cut, node, goal);
            model_solve_ik(models, ik, root, first, cut, sub, k, g, host);
            model_solve_ik(models, ik, root, cut, node, goal, n - k, g, host);
        }
    }
    if let Some(r) = ik.of_mut(models, first) {
        r.cns_type = 0;
    }
}

/// Whether a chain is cut at a joint [soft 0x084ae3ac SI_invKinIsBreakJnt].
///
/// Never for a node that is not a joint, has no father or hangs directly on the chain's
/// root (the first joint). Otherwise:
///
/// - when the dictionary says the joint is constrained by one of [`BREAK_RELATIONS`] at
///   `time` (asked in that order, stopping at the first yes; `flag` is passed through);
/// - or when an expression that is switched on at `time` drives one of the joint's
///   translation curves (ids 139 to 141). The element's expression relations (20010) and
///   the list of curves its expressions drive are walked in step: an entry counts when its
///   curve is a translation curve and its activation flag is set; if it has an activation
///   curve that is off at `time` the walk goes on. The walk ends without a result at the
///   end of either list.
pub fn inv_kin_is_break_jnt(models: &Models, node: Option<NodeId>, flag: i16, time: f32, host: &mut dyn Host) -> bool {
    let Some(node) = node else { return false };
    if models.m(node).kind != 5 {
        return false;
    }
    let Some(f) = models.father(node) else { return false };
    if is_root(models, f) {
        return false;
    }
    for rel in BREAK_RELATIONS {
        if host.ed_is_item_constrained(models, node, rel, flag, time) {
            return true;
        }
    }
    let Some(mut rel) = host.find_serial_relation(models, node, true) else { return false };
    let curves = host.expr_fcurve_used(models, node);
    if curves.is_empty() {
        return false;
    }
    let mut i = 0;
    loop {
        let hit = (0x8b..=0x8d).contains(&curves[i]) && rel.active != 0;
        if hit && (rel.activation == -1 || host.activation_on(rel.activation, time)) {
            return true;
        }
        let next = host.find_serial_relation(models, node, false);
        i += 1;
        if curves.len() <= i {
            return false;
        }
        match next {
            Some(r) => rel = r,
            None => return false,
        }
    }
}

/// The first joint of the sub-chain a skeleton node belongs to, and how many steps up it
/// lies [soft 0x084ae68c SI_invKinGetSubChainFirstJnt].
///
/// For a node that is no skeleton node: `None` and 0. From a root the walk starts at its
/// first son with the count 1; from any other node at the node with 0. It goes up father
/// by father until the chain's first joint or a break joint ([`inv_kin_is_break_jnt`]).
pub fn inv_kin_get_sub_chain_first_jnt(models: &Models, node: Option<NodeId>, flag: i16, time: f32, host: &mut dyn Host) -> (Option<NodeId>, i32) {
    let Some(node) = node else { return (None, 0) };
    if !is_chain_node(models, node) {
        return (None, 0);
    }
    let Some(root) = ske_chain_root(models, Some(node)) else { return (None, 0) };
    let top = models.son(root);
    let (mut p, mut count) = if node == root { (top, 1) } else { (Some(node), 0) };
    if p != top && !inv_kin_is_break_jnt(models, p, flag, time, host) {
        loop {
            p = match p {
                Some(q) => models.father(q),
                None => {
                    fault::note();
                    None
                }
            };
            count += 1;
            if p.is_none() || p == top || inv_kin_is_break_jnt(models, p, flag, time, host) {
                break;
            }
        }
    }
    (p, count)
}

/// The end of the sub-chain a skeleton node belongs to, and how many steps down it lies
/// [soft 0x084ae784 SI_invKinGetSubChainEffector]: the node itself when it is an effector
/// or a break joint, else the first effector or break joint below it by first sons;
/// `None` when the sons run out first (the count then includes the step off the end), and
/// for a node that is no skeleton node.
pub fn inv_kin_get_sub_chain_effector(models: &Models, node: Option<NodeId>, flag: i16, time: f32, host: &mut dyn Host) -> (Option<NodeId>, i32) {
    let Some(node) = node else { return (None, 0) };
    if !is_chain_node(models, node) {
        return (None, 0);
    }
    let mut count = 0;
    let mut p = Some(node);
    if is_effector(models, node) || inv_kin_is_break_jnt(models, p, flag, time, host) {
        return (p, count);
    }
    loop {
        p = p.and_then(|q| models.son(q));
        count += 1;
        match p {
            None => return (None, count),
            Some(q) => {
                if is_effector(models, q) || inv_kin_is_break_jnt(models, p, flag, time, host) {
                    return (p, count);
                }
            }
        }
    }
}

/// Gives an effector a goal [soft 0x084ae884 SI_invKinSetGoal]: a record is made if the
/// node has none, its state becomes 1 and its goal the point.
pub fn inv_kin_set_goal(models: &mut Models, ik: &mut IkStore, node: NodeId, goal: Vector) {
    if ik.of(models, node).is_none() {
        let p = inv_kin_allocate(ik);
        models.m_mut(node).inv_kin = p;
    }
    let r = ik.of_mut(models, node).unwrap();
    r.state = GOAL_SET;
    r.goal = goal;
    push_tx(models, ik, node);
}

/// Sets the plane constraint of a sub-chain's first joint
/// [soft 0x084ae8d0 SI_invKinSetCnsPosition] (the up-vector and preferred-axis
/// constraints, relations 128 and 129, end here).
///
/// Only for a joint that is the first of its sub-chain. The sub-chain's effector, if there
/// is one, gets a record if it has none and is made to solve again: a solved goal (state
/// 2) becomes a set one; with no goal at all (state 0) and the program not interactive,
/// the effector's present position becomes its goal. Then the joint's `cns_type` and
/// `cns_pos` are stored; they hold until the next solve
/// ([`model_solve_inv_kin`] clears the type).
pub fn inv_kin_set_cns_position(models: &mut Models, ik: &mut IkStore, node: NodeId, cns_type: i32, position: Vector, time: f32, g: &Globals, host: &mut dyn Host) {
    let (first, _) = inv_kin_get_sub_chain_first_jnt(models, Some(node), 1, time, host);
    if models.m(node).kind != 5 || first != Some(node) {
        return;
    }
    let (eff, _) = inv_kin_get_sub_chain_effector(models, models.son(node), 1, time, host);
    if let Some(e) = eff {
        if ik.of(models, e).is_none() {
            let p = inv_kin_allocate(ik);
            models.m_mut(e).inv_kin = p;
        }
        let at = models.m(e).gtrs;
        let r = ik.of_mut(models, e).unwrap();
        match r.state {
            GOAL_NONE if !g.interactive => {
                r.goal = at;
                r.state = GOAL_SET;
            }
            GOAL_SOLVED => r.state = GOAL_SET,
            _ => {}
        }
        push_tx(models, ik, e);
    }
    let r = ik.force(models, node);
    r.cns_type = cns_type;
    r.cns_pos = position;
}

/// Makes the effectors below a node solve again [soft 0x084ae9b8 SI_invKinGoalInvalidate]:
/// in the subtree of `node` (the node itself not included, `SI_modelNextSon`'s walk) every
/// null with algorithm 3 or 4 whose goal is solved (state 2) has it set back to none (0).
pub fn inv_kin_goal_invalidate(models: &mut Models, ik: &mut IkStore, node: Option<NodeId>) {
    let Some(node) = node else { return };
    let mut at = None;
    while let Some(p) = next_son(models, node, at) {
        at = Some(p);
        if is_effector(models, p) {
            if let Some(r) = ik.of_mut(models, p) {
                if r.state == GOAL_SOLVED {
                    r.state = GOAL_NONE;
                    push_tx(models, ik, p);
                }
            }
        }
    }
}
