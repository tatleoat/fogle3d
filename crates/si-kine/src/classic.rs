// SPDX-License-Identifier: MIT
//! `classic.c` on the shared memory image: the manipulation mode in which a node's scaling is
//! kept apart from the angles that solve a chain (finding 0022, "Fifth pass: classic.c").
//!
//! The file's functions fall in four groups. The chain tests (`chainNotScaled`,
//! `chainUniformScaled`, `chainScaled`, `checkChainScaleAt`, `checkHierarchUnifScaled`) say
//! whether a chain's joints are scaled and how. The solvers (`SI_classicSolvePartialInvKin`
//! and its two forms) give the one-pass solver of `jacobian.c` a goal measured in the chain's
//! unscaled frame. The joint mover (`si_classicSkeMoveJointBranch`) does the same for the
//! skeleton tools. The compensation records (`computeCompensateAngle`,
//! `SI_modelCompensateAngle`, the key lists and `SI_modelCompensateLocalCoord`) keep the
//! angles that make up for a scaled father.
//!
//! Float routines run on the x87: where the original keeps a value in a register the port
//! computes on [`F80`] and rounds where the original stores (`to_f32`).
//!
//! Callees outside this file go through [`Outside`]: the node copies, `SI_modelSymmetry`,
//! `SI_modelFreezeScaling`, `si_skeMoveJointBranch`. The allocators run on the image.
//! Three more functions of this file are ported elsewhere: `SI_modelUpdateCScalingInfo`,
//! `compensateInfoListAllocate`, `SI_classicMdlCompInfoDispose`,
//! `SI_classicModelCompInfoCopy` (`si-rensetup`).

use si_math::algebra as alg;
use si_math::types::Matrix;
use si_mem::{Mem, Ptr32};
use si_types::geom::Vector;
use si_x87::trig::fpatan;
use si_x87::{x, F80};

use crate::image::jacobian::solve_partial_inv_kin;
use crate::image::tools::volume_projection;
use crate::image::{down, Node};
use crate::onimage::ske_chain_root;
use crate::outside as out;
use crate::outside::Outside;

/// `SI_glbManip` + 0x38: a 16-bit switch (name unknown) that makes the chain tests look at the
/// fathers too.
const MANIP_FATHERS: u32 = 0x0878_1598;
/// `SI_glbManip` + 4 (`PREF_UNIFORM_SCALING`).
const UNIFORM_SCALING: u32 = 0x0878_1564;
const FRAME_CURRENT: u32 = 0x0878_2040;
const DISPLAY_KEY_F: u32 = 0x0869_5e90;
const PLAY_BACK_RUN: u32 = 0x0869_5e92;
const REVERSE_PLAY_F: u32 = 0x0869_5e94;

/// The 1e-5 of the scaling tests (a double at 0x0864c630 and its neighbours).
const EPS: f64 = 1e-5;

fn call(m: &mut Mem, o: &mut dyn Outside, r: &'static out::Routine, args: &[u32]) -> u32 {
    o.call(m, r, args).eax
}

fn eps() -> F80 {
    x(EPS)
}

/// The two differences of the three scales are both under 1e-5, formed in the register.
fn uniform(c: Vector) -> bool {
    (x(c.x) - c.y).abs().lt(eps()) && (x(c.x) - c.z).abs().lt(eps())
}

/// The scale is exactly (1, 1, 1): three ordered equalities (a NaN is not 1).
fn unit(s: Vector) -> bool {
    s.x == 1.0 && s.y == 1.0 && s.z == 1.0
}

/// A null of algorithm 3 or 4 (an effector).
fn effector(m: &Mem, n: Node) -> bool {
    m[n].kind == 0 && (m[n].algo_type.wrapping_sub(3) as u32) <= 1
}

fn vec_at(m: &Mem, a: u32) -> Vector {
    Vector { x: m.f32(a), y: m.f32(a + 4), z: m.f32(a + 8) }
}

fn set_vec(m: &mut Mem, a: u32, v: Vector) {
    m.set_f32(a, v.x);
    m.set_f32(a + 4, v.y);
    m.set_f32(a + 8, v.z);
}

fn read_matrix(m: &Mem, a: u32) -> Matrix {
    let mut r = [[0.0f32; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, e) in row.iter_mut().enumerate() {
            *e = m.f32(a + 16 * i as u32 + 4 * j as u32);
        }
    }
    r
}

fn write_matrix(m: &mut Mem, a: u32, r: &Matrix) {
    for (i, row) in r.iter().enumerate() {
        for (j, e) in row.iter().enumerate() {
            m.set_f32(a + 16 * i as u32 + 4 * j as u32, *e);
        }
    }
}

/// The first son the original takes without a test; a null one is a fault on the image and
/// the walk stops.
fn son_of(m: &Mem, n: Node) -> Option<Node> {
    let s = m[n].son;
    if s.is_null() {
        crate::fault::note();
        m.record_fault(n.addr() + 0x2c4, 4, si_mem::Access::Read);
        None
    } else {
        Some(s)
    }
}

/// [soft 0x081c9eb0 checkHierarchUnifScaled]: 1 when the node and every son and brother
/// below it is scaled alike on its three axes, else 0.
pub fn check_hierarch_unif_scaled(m: &mut Mem, n: Node) -> i32 {
    if !uniform(m[n].cscl) {
        return 0;
    }
    let s = m[n].son;
    if !s.is_null() && check_hierarch_unif_scaled(m, s) == 0 {
        return 0;
    }
    let b = m[n].brother;
    if !b.is_null() && check_hierarch_unif_scaled(m, b) == 0 {
        return 0;
    }
    1
}

/// [soft 0x081c9f44 SI_classicModelSymmetry]: the mirror of a node about a plane. A node
/// scaled alike on its axes (and, with `mode`, its sons) is mirrored as it is, with the
/// uniform-scaling preference on; any other is copied with its fathers, the copy is mirrored
/// from the top and the nodes of the copy that were the node's fathers are made plain nulls.
pub fn classic_model_symmetry(m: &mut Mem, o: &mut dyn Outside, pp: Ptr32<Node>, plane: u32, mode: i16) {
    let node = m[pp];
    let ok = uniform(m[node].cscl);
    let direct = if mode == 0 {
        ok
    } else {
        ok && (m[node].son.is_null() || check_hierarch_unif_scaled(m, m[node].son) as i16 != 0)
    };
    if direct {
        let saved = m.i32(UNIFORM_SCALING);
        m.set_i32(UNIFORM_SCALING, 1);
        call(m, o, &out::SI_MODEL_SYMMETRY, &[node.addr(), plane, mode as i32 as u32]);
        m.set_i32(UNIFORM_SCALING, saved);
        return;
    }
    let mut ebx = node;
    let mut esi: Node = Ptr32::new(call(m, o, &out::SI_MODEL_COPY_NODE, &[ebx.addr(), 1]));
    if mode != 0 && !m[ebx].son.is_null() {
        let r = call(m, o, &out::SI_MODEL_COPY, &[m[ebx].son.addr(), esi.addr()]);
        m[esi].son = Ptr32::new(r);
    }
    call(m, o, &out::SI_MODEL_COPY_FROM_SETUP, &[ebx.addr(), esi.addr()]);
    let mut count = 0;
    while !m[ebx].father.is_null() {
        let fa = m[ebx].father;
        let f: Node = Ptr32::new(call(m, o, &out::SI_MODEL_COPY_NODE, &[fa.addr(), 1]));
        m[esi].father = f;
        call(m, o, &out::SI_MODEL_COPY_FROM_SETUP, &[fa.addr(), f.addr()]);
        m[f].son = esi;
        esi = f;
        ebx = fa;
        count += 1;
    }
    let mut r: Node = Ptr32::new(call(m, o, &out::SI_MODEL_SYMMETRY, &[esi.addr(), plane, 1]));
    for _ in 0..count {
        if r.is_null() {
            // the original goes on through the null pointer; the walk stops here
            crate::fault::note();
            break;
        }
        m[r].kind = 0;
        r = m[r].son;
    }
    m[pp] = esi;
}

/// [soft 0x081ca430 si_classicFreezeRootScaling]: `SI_modelFreezeScaling` on a node and,
/// with `rec`, on every brother and son after it.
pub fn classic_freeze_root_scaling(m: &mut Mem, o: &mut dyn Outside, n: Node, rec: i16) {
    call(m, o, &out::SI_MODEL_FREEZE_SCALING, &[n.addr()]);
    if rec != 0 {
        let b = m[n].brother;
        if !b.is_null() {
            classic_freeze_root_scaling(m, o, b, 1);
        }
        let s = m[n].son;
        if !s.is_null() {
            classic_freeze_root_scaling(m, o, s, 1);
        }
    }
}

/// [soft 0x081ca488 si_envAdjustBeforeSet].
pub fn env_adjust_before_set(m: &mut Mem, o: &mut dyn Outside, a: Node, b: Node) {
    classic_freeze_root_scaling(m, o, b, 1);
    call(m, o, &out::SI_MODEL_FREEZE_SCALING, &[a.addr()]);
}

/// [soft 0x081ca4b0 chainNotScaled]: 0 when no joint from the node to the effector is scaled
/// (-1 for the first that is). With the 16-bit switch on, the fathers must be unscaled too.
pub fn chain_not_scaled(m: &mut Mem, node: Node) -> i32 {
    let mut r = 0;
    let mut p = node;
    loop {
        if effector(m, p) {
            break;
        }
        if !unit(m[p].scal) {
            r = -1;
            break;
        }
        match son_of(m, p) {
            Some(s) => p = s,
            None => break,
        }
    }
    if m.i16(MANIP_FATHERS) != 0 && r == 0 {
        let mut q = m[node].father;
        while !q.is_null() {
            if !unit(m[q].scal) {
                r = -1;
            }
            q = m[q].father;
        }
    }
    r
}

/// [soft 0x081ca588 chainUniformScaled]: 1 when every joint is scaled alike on its three
/// axes (an unscaled one is), else -1; the fathers too with the 16-bit switch on.
pub fn chain_uniform_scaled(m: &mut Mem, node: Node) -> i32 {
    let mut r = 1;
    let mut p = node;
    loop {
        if effector(m, p) {
            break;
        }
        if !unit(m[p].scal) && !uniform(m[p].scal) {
            r = -1;
            break;
        }
        match son_of(m, p) {
            Some(s) => p = s,
            None => break,
        }
    }
    if m.i16(MANIP_FATHERS) != 0 && r == 1 {
        let mut q = m[node].father;
        while !q.is_null() {
            if !unit(m[q].scal) && !uniform(m[q].scal) {
                r = -1;
            }
            q = m[q].father;
        }
    }
    r
}

/// [soft 0x081ca70c chainScaled]: 2 when the first joint is scaled, 3 when a later one is
/// (3 wins), else -1; the switch turns -1 into 2.
pub fn chain_scaled(m: &mut Mem, node: Node) -> i32 {
    let mut first = false;
    let mut later = false;
    let mut r = -1;
    if !unit(m[node].scal) {
        first = true;
    }
    let mut p = node;
    loop {
        match son_of(m, p) {
            Some(s) => p = s,
            None => break,
        }
        if effector(m, p) {
            break;
        }
        if !unit(m[p].scal) {
            later = true;
        }
    }
    if first {
        r = 2;
    }
    if later {
        r = 3;
    }
    if m.i16(MANIP_FATHERS) != 0 && r == -1 {
        r = 2;
    }
    r
}

/// [soft 0x081ca7f0 checkChainScaleAt]: the first of the three tests that does not say -1.
pub fn check_chain_scale_at(m: &mut Mem, node: Node) -> i32 {
    let mut r = chain_not_scaled(m, node);
    if r == -1 {
        r = chain_uniform_scaled(m, node);
        if r == -1 {
            r = chain_scaled(m, node);
        }
    }
    r
}

/// [soft 0x081ca82c classicSolvePartialInvKinR]: the goal brought into the frame of the
/// joint `n` sons below `first` (divided by its scale, moved by its offset), then the solver.
#[allow(clippy::too_many_arguments)]
pub fn solve_partial_inv_kin_r(m: &mut Mem, o: &mut dyn Outside, root: Node, first: Node, n: i32, goal: Vector, fp: i16, stop: &mut i16) {
    let mut p = first;
    for _ in 0..n.max(0) {
        match son_of(m, p) {
            Some(s) => p = s,
            None => return,
        }
    }
    let g = m[p].gtrs;
    let c = m[p].cscl;
    let u = m[p].unk_3ac;
    let v = Vector {
        x: ((x(goal.x) - g.x) / c.x + u.x).to_f32(),
        y: ((x(goal.y) - g.y) / c.y + u.y).to_f32(),
        z: ((x(goal.z) - g.z) / c.z + u.z).to_f32(),
    };
    solve_partial_inv_kin(m, o, root, first, n, v, fp, stop);
}

fn len3(dx: F80, dy: F80, dz: F80) -> f32 {
    ((dx * dx + dy * dy) + dz * dz).sqrt().to_f32()
}

/// [soft 0x081ca8ec classicGoalUpdate]: a goal pulled to where `a` and `b` sit in their
/// unscaled frame: the offset of `b` plus the goal's offset from `b` scaled by the ratio of
/// the two lengths (the smaller over the larger, halved; a half when one is nearly zero).
pub fn classic_goal_update(m: &Mem, a: Node, b: Node, goal: Vector) -> Vector {
    let ua = m[a].unk_3ac;
    let ub = m[b].unk_3ac;
    let len_a = len3(x(ua.x) - ub.x, x(ua.y) - ub.y, x(ua.z) - ub.z);
    let ga = m[a].gtrs;
    let gb = m[b].gtrs;
    let len_g = len3(x(ga.x) - gb.x, x(ga.y) - gb.y, x(ga.z) - gb.z);
    let f: f32 = if x(EPS).gt(x(len_g)) || x(len_a).lt(eps()) {
        0.5
    } else if x(len_g).gt(x(len_a)) {
        ((x(len_a) * 0.5f64) / len_g).to_f32()
    } else {
        ((x(len_g) * 0.5f64) / len_a).to_f32()
    };
    let t1 = x(goal.x) - gb.x;
    let t2 = x(goal.y) - gb.y;
    let t3 = x(goal.z) - gb.z;
    Vector {
        x: (t1 * f + ub.x).to_f32(),
        y: (t2 * f + ub.y).to_f32(),
        z: (x(f) * t3 + ub.z).to_f32(),
    }
}

/// [soft 0x081caa28 classicSolvePartialInvKinJ]: the solver run again (up to 50 times) with
/// the goal updated by [`classic_goal_update`] until the end joint is within 0.0001 of it.
#[allow(clippy::too_many_arguments)]
pub fn solve_partial_inv_kin_j(m: &mut Mem, o: &mut dyn Outside, root: Node, first: Node, n: i32, goal: Vector, fp: i16, stop: &mut i16) {
    let mut err: f32 = 10000.0;
    let mut i = 0;
    let Some(mut ebx) = son_of(m, first) else { return };
    for _ in 1..n.max(1) {
        match son_of(m, ebx) {
            Some(s) => ebx = s,
            None => return,
        }
    }
    let mut loc = classic_goal_update(m, first, ebx, goal);
    let (code, v) = volume_projection(m, first, n, loc);
    loc = v;
    if code != crate::iktools::VOL_AT_ROOT {
        while i <= 0x31 && x(err).gt(x(0.0001f64)) {
            solve_partial_inv_kin(m, o, root, first, n, loc, fp, stop);
            loc = classic_goal_update(m, first, ebx, goal);
            let g = m[ebx].gtrs;
            err = len3(x(goal.x) - g.x, x(goal.y) - g.y, x(goal.z) - g.z);
            i += 1;
        }
    }
}

/// [soft 0x081cab70 SI_classicSolvePartialInvKin]: the solver by what the chain's scaling is.
#[allow(clippy::too_many_arguments)]
pub fn classic_solve_partial_inv_kin(m: &mut Mem, o: &mut dyn Outside, root: Node, first: Node, n: i32, goal: Vector, fp: i16, stop: &mut i16) {
    match check_chain_scale_at(m, root) {
        -1 => {}
        0 => solve_partial_inv_kin(m, o, root, first, n, goal, fp, stop),
        1 => {
            let saved = m.i32(UNIFORM_SCALING);
            m.set_i32(UNIFORM_SCALING, 1);
            solve_partial_inv_kin(m, o, root, first, n, goal, fp, stop);
            m.set_i32(UNIFORM_SCALING, saved);
        }
        2 => solve_partial_inv_kin_r(m, o, root, first, n, goal, fp, stop),
        3 => solve_partial_inv_kin_j(m, o, root, first, n, goal, fp, stop),
        _ => {}
    }
}

/// [soft 0x081cac8c classicLimbPosition]: the three words at +0x3ac of the node `n` sons
/// below, copied to `out`.
pub fn classic_limb_position(m: &mut Mem, out: u32, node: Node, n: i32) -> u32 {
    let p = down(m, node, n);
    for k in 0..3 {
        let w = m.u32(p.addr() + 0x3ac + 4 * k);
        m.set_u32(out + 4 * k, w);
    }
    out
}

/// [soft 0x081caccc compensateInfoInit].
pub fn compensate_info_init(m: &mut Mem, list: u32) {
    m.set_i32(list, 0);
    let p = m.u32(list + 4);
    m.set_i32(p, 0);
    m.set_i32(p + 0xc, 0);
    m.set_i32(p + 8, 0);
    m.set_i32(p + 4, 0);
    m.set_i32(p + 0x18, 0);
    m.set_i32(p + 0x14, 0);
    m.set_i32(p + 0x10, 0);
}

/// [soft 0x081cad18 vectorComputeAngle]: the angles about z and y that turn the vector onto
/// the x axis, in `out[2]` and `out[1]` (`out[0]` is left); a zero projection leaves the
/// angle it would have set.
pub fn vector_compute_angle(m: &mut Mem, v: Vector, out: u32) {
    if !(v.y == 0.0 && v.x == 0.0) {
        m.set_f32(out + 8, fpatan(x(v.y), x(v.x)).to_f32());
    }
    let r = alg::matrix_z_rotation(m.f32(out + 8));
    let t2 = x(r[0][2]) * v.z + (x(r[0][1]) * v.y + x(r[0][0]) * v.x);
    let t1 = x(v.z) * r[2][2] + (x(v.y) * r[2][1] + x(v.x) * r[2][0]);
    if !(t1.compare(x(0.0f32)) == Some(core::cmp::Ordering::Equal) && t2.compare(x(0.0f32)) == Some(core::cmp::Ordering::Equal)) {
        m.set_f32(out + 4, fpatan(-t1, t2).to_f32());
    }
}

/// [soft 0x081cae08 computeCompensateAngle]: the angles between two vectors (the first
/// angle stays 0 when they are within 1e-5 of parallel in cosine) stored for row `idx`
/// (1 or 2) of the node: the first vector's angles and the differences.
pub fn compute_compensate_angle(m: &mut Mem, idx: i32, v1: Vector, v2: Vector, node: Node) {
    let sp = m.sp();
    let a = m.local(12);
    let b = m.local(12);
    for k in 0..3 {
        m.set_f32(a + 4 * k, 0.0);
        m.set_f32(b + 4 * k, 0.0);
    }
    let n1 = ((x(v1.y) * v1.y + x(v1.x) * v1.x) + x(v1.z) * v1.z).sqrt().to_f32();
    let n2 = ((x(v2.y) * v2.y + x(v2.x) * v2.x) + x(v2.z) * v2.z).sqrt();
    let den = (x(n1) * n2).to_f32();
    let dot = x(v2.z) * v1.z + (x(v2.y) * v1.y + x(v1.x) * v2.x);
    let c = (dot / den - 1.0f32).abs();
    if c.compare(eps()) == Some(core::cmp::Ordering::Greater) {
        vector_compute_angle(m, v1, a);
        vector_compute_angle(m, v2, b);
    }
    let (ax, ay, az) = (m.f32(a), m.f32(a + 4), m.f32(a + 8));
    let (bx, by, bz) = (m.f32(b), m.f32(b + 4), m.f32(b + 8));
    let t1 = (x(ay) - by).to_f32();
    let t2 = (x(ax) - bx).to_f32();
    let t3 = (x(az) - bz).to_f32();
    let base = node.addr();
    if idx == 1 {
        m.set_f32(base + 0x3b8, ax);
        m.set_f32(base + 0x3bc, t1);
        m.set_f32(base + 0x3c0, az);
        m.set_f32(base + 0x3d0, t2);
        m.set_f32(base + 0x3d4, t1);
        m.set_f32(base + 0x3d8, t3);
    } else if idx == 2 {
        m.set_f32(base + 0x3c4, ax);
        m.set_f32(base + 0x3c8, t1);
        m.set_f32(base + 0x3cc, az);
        m.set_f32(base + 0x3dc, t2);
        m.set_f32(base + 0x3e0, t1);
        m.set_f32(base + 0x3e4, t3);
    }
    m.set_sp(sp);
}

/// [soft 0x081cafa0 SI_modelCompensateAngle]: rows 1 and 2 of `mat` against rows 1 and 2 of
/// the node's rotation matrix.
pub fn model_compensate_angle(m: &mut Mem, mat: u32, node: Node) {
    for i in 1..=2u32 {
        let v1 = vec_at(m, mat + 16 * i);
        let v2 = vec_at(m, node.addr() + 0x118 + 16 * i);
        compute_compensate_angle(m, i as i32, v1, v2, node);
    }
}

/// [soft 0x081cb064 keyCompensateInfoUpdate]: the compensation list of the node gets the
/// record for the key just added to `fc` (its +0xc is the number of keys): the frame and the
/// six deltas; then the same for the next brother.
pub fn key_compensate_info_update(m: &mut Mem, node: Node, fc: u32) {
    let mut n = node;
    loop {
        if m[n].comp_info.is_null() {
            let l = m.si_malloc(8);
            if l != 0 {
                m.set_i32(l, 0);
                m.set_u32(l + 4, 0);
            }
            m[n].comp_info = Ptr32::new(l);
        }
        let list = m[n].comp_info.addr();
        let count = m.i32(fc + 0xc);
        let buf = if count == 0 { m.si_malloc(0x1c) } else { m.si_realloc(m.u32(list + 4), (count as u32).wrapping_add(1).wrapping_mul(0x1c)) };
        m.set_u32(list + 4, buf);
        m.set_i32(list, count);
        let e = buf.wrapping_add((count as u32).wrapping_sub(1).wrapping_mul(0x1c));
        let frame = m.i32(FRAME_CURRENT);
        m.set_i32(e, frame);
        for k in 0..6u32 {
            let w = m.u32(n.addr() + 0x3d0 + 4 * k);
            m.set_u32(e + 4 + 4 * k, w);
        }
        let b = m[n].brother;
        if b.is_null() {
            break;
        }
        n = b;
    }
}

/// [soft 0x081cb1cc SI_savekeyCompensateInfo].
pub fn savekey_compensate_info(m: &mut Mem, node: Node, fc: u32) {
    let s = m[node].son;
    if !s.is_null() {
        key_compensate_info_update(m, s, fc);
    }
}

/// [soft 0x081cb1f0 compensateFindKF]: the index of the first record at `frame`. With none
/// the original returns whatever its caller left in `edi`; the port returns 0.
pub fn compensate_find_kf(m: &Mem, frame: i32, node: Node) -> i32 {
    let list = m[node].comp_info.addr();
    let n = m.i32(list);
    let recs = m.u32(list + 4);
    for k in 0..n.max(0) {
        if m.i32(recs.wrapping_add(0x1c * k as u32)) == frame {
            return k;
        }
    }
    0
}

/// [soft 0x081cb23c angleInterplate]: between the records at `f0` and `f1` the node's angle
/// row `which` is moved by the difference of the keys, linearly at the current frame.
pub fn angle_interplate(m: &mut Mem, which: i32, f0: i32, f1: i32, node: Node) {
    let list = m[node].comp_info.addr();
    let recs = m.u32(list + 4);
    let k0 = recs.wrapping_add(0x1c * compensate_find_kf(m, f0, node) as u32);
    let k1 = recs.wrapping_add(0x1c * compensate_find_kf(m, f1, node) as u32);
    let (oy, oz) = if which == 1 { (8, 0xc) } else { (0x14, 0x18) };
    let den = F80::from_i32(f1.wrapping_sub(f0));
    let num = F80::from_i32(m.i32(FRAME_CURRENT).wrapping_sub(f0));
    let (y0, y1) = (m.f32(k0 + oy), m.f32(k1 + oy));
    let (z0, z1) = (m.f32(k0 + oz), m.f32(k1 + oz));
    let t1 = ((x(y1) - y0) / den) * num + y0;
    let t2 = ((x(z1) - z0) / den) * num + z0;
    let zero = 0.0f32;
    let b = node.addr() + if which == 1 { 0x3b8 } else { 0x3c4 };
    m.set_f32(b, (x(m.f32(b)) + zero).to_f32());
    m.set_f32(b + 4, (t1 + zero).to_f32());
    m.set_f32(b + 8, (t2 + m.f32(b + 8)).to_f32());
}

/// [soft 0x081cb3e4 angleAddition]: the node's angle row `which` plus the deltas, the node's
/// own or those of the record at the current frame when keys are displayed.
pub fn angle_addition(m: &mut Mem, which: i32, node: Node) {
    let base = node.addr();
    let b = base + if which == 1 { 0x3b8 } else { 0x3c4 };
    let d = base + if which == 1 { 0x3d0 } else { 0x3dc };
    m.set_f32(b + 4, 0.0);
    let add = |m: &mut Mem, at: u32, from: u32| {
        let v = (x(m.f32(at)) + m.f32(from)).to_f32();
        m.set_f32(at, v);
    };
    if m.i16(DISPLAY_KEY_F) == 0 {
        for k in 0..3 {
            add(m, b + 4 * k, d + 4 * k);
        }
    } else {
        let k = compensate_find_kf(m, m.i32(FRAME_CURRENT), node);
        let list = m[node].comp_info.addr();
        let e = m.u32(list + 4).wrapping_add(0x1c * k as u32);
        let o = if which == 1 { 4 } else { 0x10 };
        for j in 0..3 {
            add(m, b + 4 * j, e + o + 4 * j);
        }
    }
}

/// [soft 0x081cb550 compensateAngleUpdate]: for rows 1 and 2 of `mat`, the angles of the
/// row are stored in the node and then either interpolated between the records at `f0` and
/// `f1` (playing, inside the span) or added to the deltas.
pub fn compensate_angle_update(m: &mut Mem, f0: i32, f1: i32, idx3: i32, mat: u32, node: Node) {
    let saved = m.i16(PLAY_BACK_RUN);
    if saved == 0 && m.i16(REVERSE_PLAY_F) != 0 {
        m.set_i16(PLAY_BACK_RUN, 1);
    }
    for i in 1..=2u32 {
        let sp = m.sp();
        let loc = m.local(12);
        for k in 0..3 {
            m.set_f32(loc + 4 * k, 0.0);
        }
        let row = vec_at(m, mat + 0x10 * i);
        vector_compute_angle(m, row, loc);
        let b = node.addr() + if i == 1 { 0x3b8 } else { 0x3c4 };
        for k in 0..3 {
            let w = m.u32(loc + 4 * k);
            m.set_u32(b + 4 * k, w);
        }
        if m.i16(PLAY_BACK_RUN) != 0 && idx3 != -1 && f0 != f1 && !(m.i32(FRAME_CURRENT) > f1) {
            angle_interplate(m, i as i32, f0, f1, node);
        } else {
            angle_addition(m, i as i32, node);
        }
        m.set_sp(sp);
    }
    m.set_i16(PLAY_BACK_RUN, saved);
}

/// [soft 0x081cb680 compensateInfoUpdate]: finds the pair of records around the current
/// frame and runs [`compensate_angle_update`] on it.
pub fn compensate_info_update(m: &mut Mem, mat: u32, node: Node) {
    let list = m[node].comp_info.addr();
    if list == 0 {
        return;
    }
    let n = m.i32(list);
    let recs = m.u32(list + 4);
    let frame = |m: &Mem, k: i32| m.i32(recs.wrapping_add(0x1c * k as u32));
    let cur = m.i32(FRAME_CURRENT);
    let (mut esi, mut edx, mut edi) = (0, 0, -1);
    if n > 0 {
        esi = frame(m, 0);
        edx = frame(m, n - 1);
        let mut ecx = 0;
        while ecx < n - 1 {
            esi = frame(m, ecx);
            edx = frame(m, ecx + 1);
            if cur >= esi && cur < edx {
                edi = ecx;
                break;
            }
            ecx += 1;
        }
        if edi == -1 && cur >= edx {
            edi = ecx;
        }
    }
    compensate_angle_update(m, esi, edx, edi, mat, node);
}

/// [soft 0x081cb71c vectorRotation]: the vector turned about y by the second angle and then
/// about z by the third (the first is not used).
pub fn vector_rotation(m: &mut Mem, _ax: f32, ay: f32, az: f32, v: u32) {
    for r in [alg::matrix_y_rotation(ay), alg::matrix_z_rotation(az)] {
        let (vx, vy, vz) = (m.f32(v), m.f32(v + 4), m.f32(v + 8));
        let nx = ((x(vy) * r[1][0] + x(vx) * r[0][0]) + x(vz) * r[2][0]).to_f32();
        let ny = ((x(vy) * r[1][1] + x(vx) * r[0][1]) + x(vz) * r[2][1]).to_f32();
        let nz = ((x(vy) * r[1][2] + x(vx) * r[0][2]) + x(vz) * r[2][2]).to_f32();
        m.set_f32(v, nx);
        m.set_f32(v + 4, ny);
        m.set_f32(v + 8, nz);
    }
}

/// [soft 0x081cb808 vectorCompensation]: the vector brought through the angles stored for
/// row `idx` of the node (nothing when they are all zero).
pub fn vector_compensation(m: &mut Mem, idx: i32, v: u32, node: Node) {
    let sp = m.sp();
    let loc = m.local(12);
    let (vx, vy, vz) = (m.f32(v), m.f32(v + 4), m.f32(v + 8));
    let a = if vy == 0.0 && vx == 0.0 { 0.0 } else { fpatan(x(vy), x(vx)).to_f32() };
    let r = alg::matrix_z_rotation(a);
    m.set_f32(loc, ((x(r[0][1]) * vy + x(r[0][0]) * vx) + x(r[0][2]) * vz).to_f32());
    m.set_f32(loc + 4, (x(r[1][2]) * vz + (x(r[1][1]) * vy + x(r[1][0]) * vx)).to_f32());
    m.set_f32(loc + 8, (x(vz) * r[2][2] + (x(vy) * r[2][1] + x(vx) * r[2][0])).to_f32());
    let base = node.addr();
    let row = |m: &Mem, at: u32| (m.f32(at), m.f32(at + 4), m.f32(at + 8));
    let nonzero = |t: (f32, f32, f32)| !(t.0 == 0.0 && t.1 == 0.0 && t.2 == 0.0);
    let ang = if idx == 1 && nonzero(row(m, base + 0x3b8)) {
        Some(row(m, base + 0x3b8))
    } else if idx == 2 && nonzero(row(m, base + 0x3c4)) {
        Some(row(m, base + 0x3c4))
    } else {
        None
    };
    if let Some((ax, ay, az)) = ang {
        vector_rotation(m, ax, ay, az, loc);
        for k in 0..3 {
            let w = m.u32(loc + 4 * k);
            m.set_u32(v + 4 * k, w);
        }
    }
    m.set_sp(sp);
}

/// [soft 0x081cb9cc SI_modelCompensateLocalCoord]: the local matrix of `p2` under `p1`
/// (`p2`'s global times the inverse of `p1`'s), its rows 1 and 2 brought through the
/// compensation angles, and `p2`'s global matrix rebuilt from it. The angles are put back.
pub fn model_compensate_local_coord(m: &mut Mem, p1: Node, p2: Node) {
    let sp = m.sp();
    let inv_at = m.local(0x40);
    let l_at = m.local(0x40);
    let g1 = read_matrix(m, p1.addr() + 0x1c4);
    // the original leaves the destination untouched when the matrix cannot be inverted
    if let Some(inv) = alg::matrix_inverse(&g1) {
        write_matrix(m, inv_at, &inv);
    }
    let g2 = read_matrix(m, p2.addr() + 0x1c4);
    let inv = read_matrix(m, inv_at);
    write_matrix(m, l_at, &alg::matrix_multiply(&g2, &inv));
    let mut saved = [0u32; 6];
    for (k, s) in saved.iter_mut().enumerate() {
        *s = m.u32(p2.addr() + 0x3b8 + 4 * k as u32);
    }
    compensate_info_update(m, l_at, p2);
    for i in 1..=2u32 {
        vector_compensation(m, i as i32, l_at + 16 * i, p2);
    }
    for (k, s) in saved.iter().enumerate() {
        m.set_u32(p2.addr() + 0x3b8 + 4 * k as u32, *s);
    }
    let l = read_matrix(m, l_at);
    let g1 = read_matrix(m, p1.addr() + 0x1c4);
    write_matrix(m, p2.addr() + 0x1c4, &alg::matrix_multiply(&l, &g1));
    m.set_sp(sp);
}

/// [soft 0x081cbb64 classicJointPosUpdate]: where a joint is aimed: its offset plus the
/// vector to the goal, shortened to 0.6 of the unit when longer (0.6 of itself when shorter).
pub fn classic_joint_pos_update(m: &Mem, node: Node, goal: Vector) -> Vector {
    let g = m[node].gtrs;
    let u = m[node].unk_3ac;
    let t1 = x(goal.x) - g.x;
    let t2 = x(goal.y) - g.y;
    let t3 = x(goal.z) - g.z;
    let len = len3(t1, t2, t3);
    let f = if x(1.0f32).lt(x(len)) { x(0.6f64) / len } else { x(len) * 0.6f64 }.to_f32();
    // the original stores the three differences into the result and multiplies from there
    // (rounded to float), unlike classicGoalUpdate, which keeps them in the register
    let (r1, r2, r3) = (t1.to_f32(), t2.to_f32(), t3.to_f32());
    Vector {
        x: (x(f) * r1 + u.x).to_f32(),
        y: (x(f) * r2 + u.y).to_f32(),
        z: (x(f) * r3 + u.z).to_f32(),
    }
}

/// [soft 0x081cbc10 classicMoveJointBranch]: `si_skeMoveJointBranch` repeated (up to 20
/// times) with the aim of [`classic_joint_pos_update`] until the joint is within 1e-5.
pub fn classic_move_joint_branch(m: &mut Mem, o: &mut dyn Outside, node: Node, goal: Vector, a: i16, b: i16) {
    let sp = m.sp();
    // the original's frame puts the vector 4 bytes lower than a 12-byte local would
    let loc = m.local(16);
    set_vec(m, loc, classic_joint_pos_update(m, node, goal));
    let mut i = 0;
    loop {
        call(m, o, &out::SI_SKE_MOVE_JOINT_BRANCH, &[node.addr(), loc, a as i32 as u32, b as i32 as u32]);
        let fa = m[node].father;
        call(m, o, &out::SI_MODEL_UPDATE_SON_STATE, &[fa.addr(), node.addr(), 0]);
        set_vec(m, loc, classic_joint_pos_update(m, node, goal));
        let g = m[node].gtrs;
        let err = len3(x(goal.x) - g.x, x(goal.y) - g.y, x(goal.z) - g.z);
        i += 1;
        if i > 0x13 || !x(err).gt(x(EPS)) {
            break;
        }
    }
    m.set_sp(sp);
}

/// [soft 0x081cbcf4 si_classicSkeMoveJointBranch]: a joint moved to a place given in the
/// frame of the chain's scaling: by the chain's kind (checkChainScaleAt) the vector is
/// turned into the joint's unscaled frame, and the joint (and its son, when `b` is 0) moved.
pub fn classic_ske_move_joint_branch(m: &mut Mem, o: &mut dyn Outside, node: Node, v: u32, a: i16, b: i16) {
    let root = ske_chain_root(m, node);
    let c = check_chain_scale_at(m, root);
    if c == -1 {
        return;
    }
    let has_son = !effector(m, node);
    let son = m[node].son;
    let (a32, b32) = (a as i32 as u32, b as i32 as u32);
    let gt = m[node].gtrs;
    if c == 3 {
        // the original's frame holds 0x60 bytes below the saved vector; a hook that is given
        // the address of classicMoveJointBranch's local sees the same address
        let sp = m.sp();
        m.local(0x60);
        let saved = if has_son { Some(m[son].gtrs) } else { None };
        let cur = vec_at(m, v);
        let nv = Vector { x: (x(cur.x) + gt.x).to_f32(), y: (x(cur.y) + gt.y).to_f32(), z: (x(cur.z) + gt.z).to_f32() };
        set_vec(m, v, nv);
        classic_move_joint_branch(m, o, node, nv, a, b);
        if has_son && b == 0 {
            classic_move_joint_branch(m, o, son, saved.unwrap_or(nv), 1, 0);
        }
        m.set_sp(sp);
    } else if c == 2 {
        let cur = vec_at(m, v);
        let cs = m[node].cscl;
        let u = m[node].unk_3ac;
        set_vec(m, v, Vector {
            x: (x(cur.x) / cs.x + u.x).to_f32(),
            y: (x(cur.y) / cs.y + u.y).to_f32(),
            z: (x(cur.z) / cs.z + u.z).to_f32(),
        });
        call(m, o, &out::SI_SKE_MOVE_JOINT_BRANCH, &[node.addr(), v, a32, b32]);
        if has_son && b == 0 {
            call(m, o, &out::SI_SKE_MOVE_JOINT_BRANCH, &[son.addr(), son.addr() + 0x3ac, 1, 0]);
        }
    } else {
        let saved = m.i32(UNIFORM_SCALING);
        m.set_i32(UNIFORM_SCALING, 1);
        let cur = vec_at(m, v);
        set_vec(m, v, Vector { x: (x(cur.x) + gt.x).to_f32(), y: (x(cur.y) + gt.y).to_f32(), z: (x(cur.z) + gt.z).to_f32() });
        call(m, o, &out::SI_SKE_MOVE_JOINT_BRANCH, &[node.addr(), v, a32, b32]);
        if has_son && b == 0 {
            call(m, o, &out::SI_SKE_MOVE_JOINT_BRANCH, &[son.addr(), son.addr() + 0x1b8, 1, 0]);
        }
        m.set_i32(UNIFORM_SCALING, saved);
    }
}
