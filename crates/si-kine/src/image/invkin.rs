// SPDX-License-Identifier: MIT
//! `invkin.c` on the image: the record's allocation, goals, preferred angles, sub-chains,
//! and the top of the solver (finding 0022, "From an effector to a solved chain"; the
//! value forms are [`crate::invkin`]).

use si_mem::{Mem, Ptr32};
use si_types::geom::Vector;
use si_x87::{fmov, x, F80};

use super::jacobian::{check_branch_pullout, get_insert_point, solve_partial_inv_kin};
use super::rotlim::{constrained_jnt, jnt_rotlim_set_flag, rotlim_constrained_branch};
use super::tools::{dist, limb_length};
use super::{elem_from_model, ik, is_chain_node, is_effector, is_root, models_chapter, null_walk, scene_dict, IkRec, Manip, Node};
use crate::globals::{FCV_INFO_263, FCV_262, NB_FCV_261, SI_INTERACTIVE};
use crate::onimage::ske_chain_root;
use crate::outside as out;
use crate::outside::Outside;

/// The double 0.05 (0x08660cd0) and its float forms (0x08660cd8, 0x08660cdc).
const MIN_BEND: f64 = 0.05;
const MIN_BEND_F32: f32 = 0.050_000_000_745_058_06;
/// The double 0.001 (0x08660ce0).
const NO_SCALING: f64 = 0.001;
/// The float 1000 (0x08660ce8), the doubles 0.0008 (0x08660cf0) and 1e-5 (0x08660cf8).
const FAR: f32 = 1000.0;
const TOLERANCE: f64 = 0.0008;
const PROGRESS: f64 = 1e-5;

pub use crate::invkin::{BREAK_RELATIONS, EXPRESSION_RELATION};

/// [soft 0x084ad8f0 SI_invKinAllocate]: `SI_malloc(0x74)`; every word cleared but the two
/// pad halves at +0x36 and +0x3e (they keep what the heap held), `use_pref` 1.
pub fn inv_kin_allocate(m: &mut Mem) -> Ptr32<IkRec> {
    let p = m.si_malloc(IkRec::SIZE);
    if p != 0 {
        for w in (2..=10).chain(0x10..=0x12).chain(0x14..=0x1c).chain([0, 1, 0xb, 0xc, 0xe, 0x13]) {
            m.set_u32(p + 4 * w, 0);
        }
        m.set_u16(p + 0x34, 0);
        m.set_u16(p + 0x3c, 1);
    }
    Ptr32::new(p)
}

/// [soft 0x084ae884 SI_invKinSetGoal]: an effector's goal, state 1; a record is made when
/// the node has none (nothing more if that fails).
pub fn inv_kin_set_goal(m: &mut Mem, node: Node, goal: Vector) {
    if m[node].inv_kin.is_null() {
        let r = inv_kin_allocate(m);
        m[node].inv_kin = Ptr32::new(r.addr());
        if r.is_null() {
            return;
        }
    }
    let r = ik(m, node);
    m[r].state = 1;
    m[r].goal = goal;
}

/// [soft 0x084adcec SI_invKinResetPrefAngle]: a joint's preferred angles by group (0 all,
/// 1 x, 2 y, 3 z; compared unsigned); y only in a spatial chain (else 0), z kept away from
/// 0 by ±0.05. 0 for a node that is not a joint with a record. The root's record is read
/// without a test.
pub fn inv_kin_reset_pref_angle(m: &mut Mem, node: Node, group: u32, angles: Vector) -> i16 {
    if node.is_null() || m[node].kind != 5 || m[node].inv_kin.is_null() {
        return 0;
    }
    if group < 2 {
        let r = ik(m, node);
        m[r].prot.x = fmov(angles.x);
    }
    if group == 0 || group == 2 {
        let root = ske_chain_root(m, node);
        let spatial = m.i32(ik(m, root).addr()) == 0x28;
        let r = ik(m, node);
        m[r].prot.y = if spatial { fmov(angles.y) } else { 0.0 };
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
        let r = ik(m, node);
        m[r].prot.z = z;
    }
    1
}

/// [soft 0x084addc8 checkScaleBeforeIK]: with more than one joint and the limb's last node
/// `last` without global scaling (all three below 0.001 in size), the first of the joints
/// 1 to `n` − 1 below `first` without local scaling.
pub fn check_scale_before_ik(m: &Mem, first: Node, last: Node, n: i32) -> Option<Node> {
    if n <= 1 {
        return None;
    }
    let none = |s: Vector| x(s.x).abs().lt(x(NO_SCALING)) && x(s.y).abs().lt(x(NO_SCALING)) && x(s.z).abs().lt(x(NO_SCALING));
    if !none(m[last].gscl) {
        return None;
    }
    let mut p = m[first].son;
    for _ in 1..n {
        if p.is_null() {
            null_walk(m, 0x100);
            return None;
        }
        if none(m[p].scal) {
            return Some(p);
        }
        p = m[p].son;
    }
    None
}

/// [soft 0x084adea8 findSubGoalBeforeIK]: the point on the line from `first` to the goal
/// as far short of the goal as the effector is from `cut`.
pub fn find_sub_goal_before_ik(m: &Mem, first: Node, cut: Node, effector: Node, goal: Vector) -> Vector {
    let p = m[first].gtrs;
    let whole = dist(goal, p).to_f32();
    let rest = dist(m[effector].gtrs, m[cut].gtrs);
    get_insert_point((x(whole) - rest).to_f32(), whole, p, goal)
}

/// One pass: `SI_classicSolvePartialInvKin` under classical scaling (`SI_glbManip` word 0
/// is 1; `classic.c`, outside the package, the stop flag in a local), else
/// [`solve_partial_inv_kin`].
#[allow(clippy::too_many_arguments)]
fn pass(m: &mut Mem, o: &mut dyn Outside, root: Node, first: Node, n: i32, goal: Vector, first_pass: i16, stop: &mut i16) {
    let g = Manip::read(m);
    if g.classical {
        let sp = m.sp();
        let at = m.local(4);
        m.set_i16(at, *stop);
        o.call(m, &out::SI_CLASSIC_SOLVE_PARTIAL_INV_KIN, &[root.addr(), first.addr(), n as u32, goal.x.to_bits(), goal.y.to_bits(), goal.z.to_bits(), first_pass as i32 as u32, at]);
        *stop = m.i16(at);
        m.set_sp(sp);
    } else {
        solve_partial_inv_kin(m, o, root, first, n, goal, first_pass, stop);
    }
}

/// [soft 0x084adf7c modelSolveIK]: a limb solved and solved again while that helps (up to
/// ten more passes for a limb with a rotation limit on its first joint or a spatial chain;
/// [`crate::invkin::model_solve_ik`]).
#[allow(clippy::too_many_arguments)]
pub fn model_solve_ik(m: &mut Mem, o: &mut dyn Outside, root: Node, first: Node, effector: Node, goal: Vector, n: i32) {
    let mut stop = 0i16;
    jnt_rotlim_set_flag(m, o, first, n);
    pass(m, o, root, first, n, goal, 1, &mut stop);
    let limited = rotlim_constrained_branch(m, o, first, n);
    if !limited && m.i32(ik(m, root).addr()) != 0x28 {
        return;
    }
    let mut off = FAR;
    let mut before = 0.0f32;
    let mut tolerance = 0.0f32;
    let mut passes = 0;
    let mut pulled = false;
    let mut clamped = constrained_jnt(m, o, first, n);
    if clamped < n {
        off = dist(goal, m[effector].gtrs).to_f32();
        let (len, _, _) = limb_length(m, first, n);
        tolerance = (len * TOLERANCE).to_f32();
        pulled = check_branch_pullout(m, o, first, goal, n);
    }
    if stop != 0 {
        return;
    }
    while clamped < n && x(off).gt(x(tolerance)) && (x(off) - before).abs().gt(x(PROGRESS)) && !pulled {
        pass(m, o, root, first, n, goal, 0, &mut stop);
        passes += 1;
        clamped = constrained_jnt(m, o, first, n);
        if clamped < n {
            before = fmov(off);
            off = dist(goal, m[effector].gtrs).to_f32();
            pulled = check_branch_pullout(m, o, first, goal, n);
        }
        if stop != 0 || passes > 9 {
            return;
        }
    }
}

/// [soft 0x084ae1fc SI_modelSolveInvKin]: the chain above an effector (or a joint) solved
/// for a goal: the limb from its sub-chain's first joint to its father, in two parts when
/// a joint without scaling cuts it; the first joint's plane constraint is cleared
/// afterwards ([`crate::invkin::model_solve_inv_kin`]). The father is read without a
/// test; a sub-chain without a first joint (the original reads through null) stops here,
/// recorded.
pub fn model_solve_inv_kin(m: &mut Mem, o: &mut dyn Outside, node: Node, goal: Vector, time: f32) {
    if node.is_null() || is_root(m, node) {
        return;
    }
    let a = m[node].algo_type;
    if !(a == 1 || a == 3) {
        return;
    }
    let last = m[node].father;
    if last.is_null() {
        null_walk(m, 0x30);
        return;
    }
    if is_root(m, last) {
        return;
    }
    let root = ske_chain_root(m, node);
    let (first, up) = inv_kin_get_sub_chain_first_jnt(m, o, last, 1, time);
    let n = up + 1;
    match check_scale_before_ik(m, first, last, up) {
        None => model_solve_ik(m, o, root, first, node, goal, n),
        Some(cut) => {
            let mut k = 0;
            let mut p = cut;
            loop {
                p = m[p].father;
                k += 1;
                if p == first || p.is_null() {
                    if p.is_null() {
                        null_walk(m, 0x2bc);
                    }
                    break;
                }
            }
            let sub = find_sub_goal_before_ik(m, first, cut, node, goal);
            model_solve_ik(m, o, root, first, cut, sub, k);
            model_solve_ik(m, o, root, cut, node, goal, n - k);
        }
    }
    if first.is_null() {
        null_walk(m, 0x318);
        return;
    }
    let r = ik(m, first);
    if !r.is_null() {
        m[r].cns_type = 0;
    }
}

/// [soft 0x084ae3ac SI_invKinIsBreakJnt]: whether a chain is cut at a joint: never for a
/// node that is not a joint, has no father or hangs on the chain's root; else when
/// `SI_edIsItemConstrained` says yes for one of the relations 111, 806, 112, 113, 126, 160
/// (asked in that order), or when an active expression (relation 20010) drives one of the
/// joint's translation curves (ids 139 to 141) and its activation curve, if any, is on at
/// `time`. The expressions' curve list is `SI_getExprFcurveUsed`'s, kept in the routine's
/// three statics as the original keeps it.
pub fn inv_kin_is_break_jnt(m: &mut Mem, o: &mut dyn Outside, node: Node, flag: i16, time: f32) -> bool {
    if node.is_null() || m[node].kind != 5 {
        return false;
    }
    let f = m[node].father;
    if f.is_null() || is_root(m, f) {
        return false;
    }
    let chapter = models_chapter(m);
    for rel in BREAK_RELATIONS {
        let d = scene_dict(m);
        let r = o.call(m, &out::SI_ED_IS_ITEM_CONSTRAINED, &[d, node.addr(), chapter as u32, rel as u32, flag as i32 as u32, time.to_bits()]).eax as i16;
        if r != 0 {
            return true;
        }
    }
    let elem = elem_from_model(m, o, node);
    let sp = m.sp();
    let index = m.local(4);
    let find = |m: &mut Mem, o: &mut dyn Outside, first: u32| {
        let d = scene_dict(m);
        o.call(m, &out::SI_ELEMDICT_FIND_SERIAL_RELATION, &[d, elem, EXPRESSION_RELATION as u32, index, 0xffff_ffff, 1, first]).eax
    };
    let mut rel = find(m, o, 1);
    let mut answer = false;
    if rel != 0 {
        let list = m.u32(FCV_262.addr());
        let got = o.call(m, &out::SI_GET_EXPR_FCURVE_USED, &[elem, NB_FCV_261.addr(), list, 0, FCV_INFO_263.addr(), 0xffff_ffff]).eax;
        m.set_u32(FCV_262.addr(), got);
        let mut i: i32 = 0;
        if 0 < m.i32(NB_FCV_261.addr()) {
            loop {
                let curve = m.i32(m.u32(FCV_262.addr()).wrapping_add(4 * i as u32));
                let hit = (curve.wrapping_sub(0x8b) as u32) <= 2 && m.i16(rel + 0x14) != 0;
                if hit {
                    let act = m.i32(rel + 8);
                    if act == -1 {
                        answer = true;
                        break;
                    }
                    let d = scene_dict(m);
                    // the first word of `SI_cid`: the chapter of the animation elements
                    let cid = m.i32(si_dict::globals::SI_CID.addr());
                    let e = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[d, cid as u32, act as u32]).eax;
                    let value = m.local(4);
                    let curve = m.u32(e.wrapping_add(4));
                    let on = o.call(m, &out::SI_ACTIVATION_FCV_CHECK_TIME, &[curve, time.to_bits(), value, 1]).eax as i16;
                    m.release(4);
                    if on != 0 {
                        answer = true;
                        break;
                    }
                }
                rel = find(m, o, 0);
                i += 1;
                if m.i32(NB_FCV_261.addr()) <= i || rel == 0 {
                    break;
                }
            }
        }
    }
    m.set_sp(sp);
    answer
}

/// [soft 0x084ae68c SI_invKinGetSubChainFirstJnt]: the first joint of the sub-chain a
/// skeleton node belongs to and how many steps up it lies (from a root: its son, counted
/// 1); up father by father to the chain's first joint or a break joint. Null and 0 for a
/// node that is no skeleton node.
pub fn inv_kin_get_sub_chain_first_jnt(m: &mut Mem, o: &mut dyn Outside, node: Node, flag: i16, time: f32) -> (Node, i32) {
    if node.is_null() || !is_chain_node(m, node) {
        return (Node::NULL, 0);
    }
    let root = ske_chain_root(m, node);
    let top = m[root].son;
    let (mut p, mut count) = if node == root { (m[node].son, 1) } else { (node, 0) };
    if p != top && !inv_kin_is_break_jnt(m, o, p, flag, time) {
        loop {
            p = m[p].father;
            count += 1;
            if p.is_null() || p == top || inv_kin_is_break_jnt(m, o, p, flag, time) {
                break;
            }
        }
    }
    (p, count)
}

/// [soft 0x084ae784 SI_invKinGetSubChainEffector]: the end of the sub-chain a skeleton node
/// belongs to (the node itself when it is an effector or a break joint, else the first
/// effector or break joint below it by first sons) and how many steps down; null when the
/// sons run out (the count then includes the step off the end), and for a node that is no
/// skeleton node.
pub fn inv_kin_get_sub_chain_effector(m: &mut Mem, o: &mut dyn Outside, node: Node, flag: i16, time: f32) -> (Node, i32) {
    if node.is_null() || !is_chain_node(m, node) {
        return (Node::NULL, 0);
    }
    let mut count = 0;
    let mut p = node;
    if is_effector(m, p) || inv_kin_is_break_jnt(m, o, p, flag, time) {
        return (p, count);
    }
    loop {
        p = m[p].son;
        count += 1;
        if p.is_null() {
            return (p, count);
        }
        if is_effector(m, p) || inv_kin_is_break_jnt(m, o, p, flag, time) {
            return (p, count);
        }
    }
}

/// [soft 0x084ae8d0 SI_invKinSetCnsPosition]: the plane constraint of a sub-chain's first
/// joint; the sub-chain's effector (a record made if it has none) is made to solve again:
/// state 2 becomes 1, state 0 with the program not interactive takes the effector's
/// position as its goal. The joint's record is written without a test.
pub fn inv_kin_set_cns_position(m: &mut Mem, o: &mut dyn Outside, node: Node, cns_type: i32, position: Vector, time: f32) {
    let (first, _) = inv_kin_get_sub_chain_first_jnt(m, o, node, 1, time);
    if m[node].kind != 5 || first != node {
        return;
    }
    let s = m[node].son;
    let (e, _) = inv_kin_get_sub_chain_effector(m, o, s, 1, time);
    if !e.is_null() {
        let mut ok = true;
        if m[e].inv_kin.is_null() {
            let r = inv_kin_allocate(m);
            m[e].inv_kin = Ptr32::new(r.addr());
            ok = !r.is_null();
        }
        if ok {
            let r = ik(m, e);
            match m[r].state {
                0 if m[SI_INTERACTIVE] == 0 => {
                    let at = m[e].gtrs;
                    m[r].goal = at;
                    m[r].state = 1;
                }
                2 => m[r].state = 1,
                _ => {}
            }
        }
    }
    let r = ik(m, node);
    m[r].cns_type = cns_type;
    m[r].cns_pos = position;
}
