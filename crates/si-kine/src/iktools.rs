// SPDX-License-Identifier: MIT
//! The tools of the solver: the original's `iktools.c` (finding 0022, "Limbs and the
//! reachable volume", "Two bones in a plane", "The chain's own update").
//!
//! A *limb* is a run of `n` joints starting at a node: the node, its first son, that node's
//! first son and so on; the node after the last joint (the effector, or the next joint of a
//! longer chain) is its end. A bone's length is the local x translation of the next node
//! times the joint's global x scaling.
//!
//! Exactness classes as in `si-math`.

use si_math::algebra::{matrix_copy, matrix_multiply_sub3, matrix_xyz_rotation};
use si_math::types::{Matrix, IDENTITY};
use si_model::node::{mv, Models, NodeId};
use si_model::transform::update_son_state;
use si_types::geom::Vector;
use si_x87::trig;
use si_x87::{fmov, x, F80};

use crate::rec::IkStore;
use crate::{fault, Globals, Host};

/// The double 0.0001 the volume tests compare with (0x08660ca8).
const EPS: f64 = 1e-4;
/// The float nearest 0.0001 (0x08660cb0): the margin of a two-joint limb's inner sphere.
const EPS_F32: f32 = 9.999_999_747_378_752e-5;
/// The double 0.001 (0x08660cb8): the same margin for other limbs, as a fraction of the
/// limb's length.
const INNER_MARGIN: f64 = 0.001;
/// π as a float (0x08660cc8).
const PI_F32: f32 = 3.141_592_741_012_573_2;

/// `SI_volumeProjection`'s result: where the goal lies with respect to what the limb can
/// reach.
pub const VOL_INSIDE: i32 = 0;
/// Beyond the limb's full length: the goal was pulled in onto the outer sphere.
pub const VOL_TOO_FAR: i32 = 2;
/// Inside the sphere the limb cannot fold into: pushed out onto it.
pub const VOL_TOO_NEAR: i32 = 3;
/// At the limb's root (closer than 0.0001): nothing to aim at.
pub const VOL_AT_ROOT: i32 = 4;

/// A node's position as the solver measures it: `gtrs`, or under classical scaling without
/// uniform scaling the position of the frame without scaling (+0x3ac).
pub(crate) fn pos(models: &Models, node: NodeId, g: &Globals) -> Vector {
    let m = models.m(node);
    if g.classic_frame() { m.unk_3ac } else { m.gtrs }
}

/// The first son, where the original follows the `son` pointer without testing it. A null
/// pointer (the original faults) ends the walk here and is counted.
pub(crate) fn son(models: &Models, node: NodeId) -> Option<NodeId> {
    let s = models.son(node);
    if s.is_none() {
        fault::note();
    }
    s
}

/// The node `n` sons below `node` (the walk every routine here makes to a limb's end).
pub(crate) fn down(models: &Models, node: NodeId, n: i32) -> NodeId {
    let mut p = node;
    for _ in 0..n.max(0) {
        match son(models, p) {
            Some(s) => p = s,
            None => break,
        }
    }
    p
}

/// √((x² + y²) + z²) of three register values, unrounded.
pub(crate) fn len3(dx: F80, dy: F80, dz: F80) -> F80 {
    ((dx * dx + dy * dy) + dz * dz).sqrt()
}

/// The distance of two points as these files compute it everywhere: differences and squares
/// in the register, summed x, y, z, the root unrounded.
pub fn dist(a: Vector, b: Vector) -> F80 {
    len3(x(a.x) - b.x, x(a.y) - b.y, x(a.z) - b.z)
}

/// The length of a limb of `n` joints from `node` [soft 0x084ace40 si_limbLength]: the sum
/// of its bone lengths **unrounded**, the longest bone and the shortest. Class: model.
///
/// A bone's length is `r32(|joint's gscl.x| × next node's trans.x)`. Under classical
/// scaling it is `r32(|joint's cscl.x| × trans.x)` with uniform scaling and the bare
/// `trans.x` without. The lengths are added in the register, so the sum carries more than a
/// float; a negative or NaN length is added as it is. The longest starts at 0 and the
/// shortest at the largest float: with `n` ≤ 0 those two come back.
///
/// (The original returns the two through pointers, the shortest written first. Two of its
/// callers pass the same address for both and use neither.)
pub fn limb_length(models: &Models, node: NodeId, n: i32, g: &Globals) -> (F80, f32, f32) {
    let mut min = f32::MAX;
    let mut max = 0.0f32;
    let mut sum = F80::ZERO;
    let mut p = node;
    for _ in 0..n.max(0) {
        let Some(s) = son(models, p) else { break };
        let tx = models.m(s).trans.x;
        let len = if g.model.classical_scaling {
            if g.uniform_scaling == 1 { (x(models.m(p).cscl.x).abs() * tx).to_f32() } else { tx }
        } else {
            (x(models.m(p).gscl.x).abs() * tx).to_f32()
        };
        // the shortest keeps its value only when it is below the new length; the longest
        // only when it is above
        if !x(min).lt(x(len)) {
            min = fmov(len);
        } else {
            min = fmov(min);
        }
        if !x(max).gt(x(len)) {
            max = fmov(len);
        } else {
            max = fmov(max);
        }
        sum = sum + len;
        p = s;
    }
    (sum, max, min)
}

/// The position of a limb's end [soft 0x084ad134 si_limbPosition]: `gtrs` of the node `n`
/// sons below `node`, copied as it is. Class: copy.
pub fn limb_position(models: &Models, node: NodeId, n: i32) -> Vector {
    models.m(down(models, node, n)).gtrs
}

/// Brings a goal into the volume a limb of `n` joints from `node` can reach
/// [soft 0x084aceec SI_volumeProjection]. Returns one of the `VOL_` values and the point
/// to solve for. Class: model.
///
/// With d the goal minus the limb's root and D = r32(|d|):
///
/// - D < 0.0001: [`VOL_AT_ROOT`], the goal itself;
/// - L + 0.0001 < D, L the limb's length (unrounded): [`VOL_TOO_FAR`], the point at
///   distance L along d;
/// - if the longest bone is longer than all the others together (longest > L − longest) and
///   D ≤ inner = (longest − (L − longest)) + margin: [`VOL_TOO_NEAR`], the point at distance
///   `inner` along d. The margin is the float nearest 0.0001 for a limb of two joints and
///   r32(L × 0.001) otherwise;
/// - otherwise [`VOL_INSIDE`], the goal itself.
///
/// The scaled point is r32(factor × d) with the factor in the register; the root's position
/// is then added to that float (two roundings), except under classical scaling without
/// uniform scaling, where each component is r32(factor × d + position) in one go.
pub fn volume_projection(models: &Models, node: NodeId, n: i32, goal: Vector, g: &Globals) -> (i32, Vector) {
    let root = pos(models, node, g);
    let dx = (x(goal.x) - root.x).to_f32();
    let dy = (x(goal.y) - root.y).to_f32();
    let dz = (x(goal.z) - root.z).to_f32();
    let d = ((x(dx) * dx + x(dy) * dy) + x(dz) * dz).sqrt().to_f32();
    if x(d).lt(x(EPS)) {
        return (VOL_AT_ROOT, goal);
    }
    let (len, longest, _) = limb_length(models, node, n, g);
    let scaled = |factor: F80| -> Vector {
        let (px, py, pz) = (factor * dx, factor * dy, factor * dz);
        if g.classic_frame() {
            Vector { x: (px + root.x).to_f32(), y: (py + root.y).to_f32(), z: (pz + root.z).to_f32() }
        } else {
            Vector { x: (x(px.to_f32()) + root.x).to_f32(), y: (x(py.to_f32()) + root.y).to_f32(), z: (x(pz.to_f32()) + root.z).to_f32() }
        }
    };
    if (len + EPS).lt(x(d)) {
        return (VOL_TOO_FAR, scaled(len / d));
    }
    let margin = if n == 2 { EPS_F32 } else { (len * INNER_MARGIN).to_f32() };
    let rest = len - longest;
    if !x(longest).gt(rest) {
        return (VOL_INSIDE, goal);
    }
    let inner = (x(longest) - rest) + margin;
    match x(d).compare(inner) {
        Some(core::cmp::Ordering::Greater) | None => (VOL_INSIDE, goal),
        _ => (VOL_TOO_NEAR, scaled(inner / d)),
    }
}

/// The inverse of a matrix whose upper 3 × 3 is orthonormal
/// [soft 0x084ad170 SI_matrixOrthoNormalInverse]: the transpose of the 3 × 3, row 3 =
/// −((t.x·m[i][0] + t.y·m[i][1]) + t.z·m[i][2]) for column i, last column (0, 0, 0, 1).
/// Class: model. The input is copied first, so the two may be the same matrix.
pub fn matrix_ortho_normal_inverse(m: &Matrix) -> Matrix {
    let mut out: Matrix = [[0.0; 4]; 4];
    out[3][3] = 1.0;
    let t = m[3];
    for i in 0..3 {
        out[0][i] = fmov(m[i][0]);
        out[1][i] = fmov(m[i][1]);
        out[2][i] = fmov(m[i][2]);
        out[3][i] = (-((x(t[0]) * m[i][0] + x(t[1]) * m[i][1]) + x(t[2]) * m[i][2])).to_f32();
    }
    out
}

/// The rotation by three angles about Z, Y and Z again
/// [soft 0x084ad214 SI_matrixZyzeulerRotation]. Class: libm.
///
/// Sines and cosines are rounded to float (asked of the library in the order sin a, cos a,
/// sin b, cos b, sin c, cos c); each element is rounded once.
pub fn matrix_zyz_euler_rotation(a: f32, b: f32, c: f32) -> Matrix {
    let sa = trig::sin(a as f64).to_f32();
    let ca = trig::cos(a as f64).to_f32();
    let sb = trig::sin(b as f64).to_f32();
    let cb = trig::cos(b as f64).to_f32();
    let sc = trig::sin(c as f64).to_f32();
    let cc = trig::cos(c as f64).to_f32();
    let mut m: Matrix = [[0.0; 4]; 4];
    m[0][0] = ((x(ca) * cb) * cc - x(sa) * sc).to_f32();
    m[1][0] = ((-x(ca) * cb) * sc - x(sa) * cc).to_f32();
    m[2][0] = (x(ca) * sb).to_f32();
    m[0][1] = (x(ca) * sc + (x(sa) * cb) * cc).to_f32();
    m[1][1] = (x(ca) * cc + (-x(sa) * cb) * sc).to_f32();
    m[2][1] = (x(sa) * sb).to_f32();
    m[0][2] = (x(cc) * -x(sb)).to_f32();
    m[1][2] = (x(sc) * sb).to_f32();
    m[2][2] = fmov(cb);
    m[3][3] = 1.0;
    m
}

/// The two angles of a pair of bones of lengths `l1` and `l2` whose end is to reach the
/// point (`px`, `py`) of their plane [soft 0x084ad360 si_solvePlanarLimb]: the first bone's
/// angle from the x axis and the bend between the bones. `up` chooses the side of the
/// bend. Class: libm (`fpatan`).
///
/// With D = r32(√(px² + py²)):
///
/// - D + 0.0001 > l1 + l2 (out of reach): the first angle is r32(atan2(py, px)), the bend 0;
/// - |l1 − l2| > D − 0.0001 (too close to fold): the same first angle, the bend π (float);
/// - otherwise c = r32(((px² + py² − l1²) − l2²) / ((l1 + l1) × l2)), the bend's cosine, and
///   s = ±√(r32(1 − c²)), positive with `up`: the bend is r32(atan2(s, c)) and the first
///   angle r32(r32(atan2(py, px)) − r32(atan2(s × l2, c × l2 + l1))).
///
/// A comparison with a NaN falls through to the third case.
pub fn solve_planar_limb(l1: f32, l2: f32, px: f32, py: f32, up: i16) -> (f32, f32) {
    let (a, b) = (x(l1), x(l2));
    let t1 = x(px) * px + x(py) * py;
    let r = x(t1.sqrt().to_f32());
    if (r + EPS).gt(a + b) {
        return (trig::fpatan(x(py), x(px)).to_f32(), 0.0);
    }
    if (a - b).abs().gt(r - EPS) {
        return (trig::fpatan(x(py), x(px)).to_f32(), PI_F32);
    }
    let c = x((((t1 - a * a) - b * b) / ((a + a) * b)).to_f32());
    let s = x((F80::ONE - c * c).to_f32()).sqrt();
    let s = if up != 0 { s } else { -s };
    let bend = trig::fpatan(s, c);
    let (sb, cb) = (s * b, c * b);
    let bend = bend.to_f32();
    let phi = trig::fpatan(x(py), x(px)).to_f32();
    let psi = trig::fpatan(sb, cb + a).to_f32();
    ((x(phi) - psi).to_f32(), bend)
}

/// One node of a chain derived from its father's global values, the solver's own short
/// form of the node update [soft 0x084ad480 SI_modelSpecialUpdateSonState]. Class: libm
/// (the rotation matrix).
///
/// With a `father`: `hscl`, `hrot`, `htrs` are copied from its `gscl`, `grot`, `gtrs`, and
/// `gtrs` = `trans` × father's `gmat`, formed as ((y·m₁ⱼ + x·m₀ⱼ) + z·m₂ⱼ) + m₃ⱼ and rounded
/// once. Without one the node is **the origin of the frame the solver works in**: `hrot`
/// is the identity, `htrs` and `gtrs` are zero, and `hscl` is still the `gscl` of the node's
/// real father (kept as it is when there is none).
///
/// Then `gscl` = `hscl` × `scal` (no floor on small factors, unlike the full update),
/// `mrot` from the angles, `grot` = `mrot` × `hrot` (or `mrot` alone for an effector under a
/// joint whose record's first word is 1), and `gmat` = the rows of `grot` times the factors
/// of `gscl` with `gtrs` in row 3. Under classical scaling the rows are scaled by `cscl`
/// with uniform scaling and not at all without, and +0x3ac takes `gtrs`.
pub fn model_special_update_son_state(models: &mut Models, ik: &IkStore, father: Option<NodeId>, node: NodeId, g: &Globals) {
    match father {
        Some(f) => {
            let fm = *models.m(f);
            let m = models.m_mut(node);
            m.hscl = fm.gscl;
            m.hrot = matrix_copy(&fm.grot);
            m.htrs = fm.gtrs;
            let t = m.trans;
            let col = |j: usize| (((x(t.y) * fm.gmat[1][j] + x(t.x) * fm.gmat[0][j]) + x(t.z) * fm.gmat[2][j]) + fm.gmat[3][j]).to_f32();
            m.gtrs = Vector { x: col(0), y: col(1), z: col(2) };
        }
        None => {
            if let Some(f) = models.father(node) {
                let s = models.m(f).gscl;
                models.m_mut(node).hscl = s;
            }
            let m = models.m_mut(node);
            m.hrot = matrix_copy(&IDENTITY);
            m.htrs = Vector { x: 0.0, y: 0.0, z: 0.0 };
            m.gtrs = Vector { x: 0.0, y: 0.0, z: 0.0 };
        }
    }
    let keeps = models.m(node).algo_type == 3 && {
        // the original reads the father's record through two pointers it does not test
        match models.father(node) {
            Some(f) => match ik.of(models, f) {
                Some(r) => r.kind == 1,
                None => {
                    fault::note();
                    false
                }
            },
            None => {
                fault::note();
                false
            }
        }
    };
    let m = models.m_mut(node);
    m.gscl = Vector { x: (x(m.hscl.x) * m.scal.x).to_f32(), y: (x(m.hscl.y) * m.scal.y).to_f32(), z: (x(m.hscl.z) * m.scal.z).to_f32() };
    m.mrot = matrix_xyz_rotation(&mv(m.vrot));
    m.grot = if keeps { matrix_copy(&m.mrot) } else { matrix_multiply_sub3(&m.mrot, &m.hrot) };
    m.gmat = matrix_copy(&m.grot);
    m.gmat[3][0] = fmov(m.gtrs.x);
    m.gmat[3][1] = fmov(m.gtrs.y);
    m.gmat[3][2] = fmov(m.gtrs.z);
    let scale = if !g.model.classical_scaling {
        Some(m.gscl)
    } else if g.uniform_scaling == 1 {
        Some(m.cscl)
    } else {
        None
    };
    if let Some(s) = scale {
        let s = [s.x, s.y, s.z];
        for i in 0..3 {
            m.gmat[i][0] = (x(m.gmat[i][0]) * s[i]).to_f32();
            m.gmat[i][1] = (x(m.gmat[i][1]) * s[i]).to_f32();
            m.gmat[i][2] = (x(s[i]) * m.gmat[i][2]).to_f32();
        }
    }
    if g.model.classical_scaling {
        m.unk_3ac = Vector { x: fmov(m.gtrs.x), y: fmov(m.gtrs.y), z: fmov(m.gtrs.z) };
    }
}

/// A chain brought up to date from its joints' angles, **in the frame of its first node**
/// [soft 0x084ad86c SI_chainUpdate]. Class: libm.
///
/// `node` is updated as the origin ([`model_special_update_son_state`] without a father),
/// then each first son from its father, down to and including the first null with
/// algorithm 3 or 4 (the effector). So after this call the positions of the chain's nodes
/// are relative to `node` and their rotations do not include what lies above it: the
/// solver's working frame. With `sons` the effector's first son is then given the full
/// update of `si-model`, which runs on through that son's brothers and subtree.
///
/// A chain that ends without an effector makes the original fault; the walk ends there.
pub fn chain_update(models: &mut Models, ik: &IkStore, node: NodeId, sons: i16, g: &Globals, host: &mut dyn Host) {
    let mut father = None;
    let mut p = node;
    loop {
        model_special_update_son_state(models, ik, father, p, g);
        let Some(s) = son(models, p) else { return };
        p = s;
        let m = models.m(p);
        if m.kind == 0 && (m.algo_type == 3 || m.algo_type == 4) {
            break;
        }
        father = models.father(p);
    }
    model_special_update_son_state(models, ik, models.father(p), p, g);
    if sons != 0 {
        if let Some(s) = models.son(p) {
            update_son_state(models, Some(p), s, true, &g.model, host);
        }
    }
}
