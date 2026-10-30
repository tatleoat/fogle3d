// SPDX-License-Identifier: MIT
//! The solver: the original's `jacobian.c` (finding 0022, "The solver").
//!
//! [`solve_partial_inv_kin`] is the entry: it solves a *limb* (a run of `n` joints from a
//! first joint) for a goal and, when a joint ran into a rotation limit, solves again the
//! part of the limb below that joint. By the root's chain type and `n`:
//!
//! | Limb | Routine | Method |
//! |---|---|---|
//! | one joint | [`invkin0`] | the bone is pointed at the goal |
//! | two joints, planar chain | [`invkin1`] | closed form ([`crate::iktools::solve_planar_limb`]) in the resolution plane |
//! | three or more, planar chain | [`invkin2d`] | iteration on the pseudo-inverse of a 2 × n Jacobian, in the resolution plane |
//! | two or more, spatial chain | [`invkin3d`] | the same on a 3 × 3n Jacobian |
//!
//! All of them work in the frame of the limb's first joint ([`crate::iktools::chain_update`])
//! and end with the full node update of `si-model` from that joint down.
//!
//! Exactness: every routine is class model or libm. Angles come out of matrices through
//! `SI_matrixExtractXyzRotation` and of `fpatan`, both recorded and replayed by the
//! emulation check.

use core::cmp::Ordering;

use si_math::algebra::{
    matrix_align_axes, matrix_axis_rotation, matrix_copy, matrix_extract_xyz_rotation, matrix_multiply, matrix_multiply_sub3, matrix_transpose,
    matrix_x_rotation, matrix_xyz_rotation, matrix_y_rotation, matrix_z_rotation, vector_angle,
};
use si_math::matrix_gen_dbl::{general_matrix_allocate_dbl, general_matrix_greville_inverse_dbl, GenMatrixDbl};
use si_math::types::{Matrix, Vector as MVector, IDENTITY};
use si_model::node::{mv, tv, Models, NodeId};
use si_model::transform::update_son_state;
use si_types::geom::Vector;
use si_types::Ptr32;
use si_x87::trig;
use si_x87::{fmov, x, Round, F80};

use crate::iktools::{chain_update, dist, down, limb_length, limb_position, pos, solve_planar_limb, son, volume_projection, VOL_AT_ROOT, VOL_TOO_FAR};
use crate::rec::{IkStore, CHAIN_CPX_3D};
use crate::rotlim::check_rotation_limit;
use crate::{fault, Globals, Host};

/// The double 0.0001 of these routines (0x08660d28, …d30, …d38, …d40, …d60, …d70, …db0: one
/// copy per routine).
const EPS: f64 = 1e-4;
/// The double 1e-5 (0x08660d80, …dc0, …de0, …de8).
const EPS5: f64 = 1e-5;
/// 2π and π as doubles (0x08660d00, 0x08660d48; 0x08660d50) and as floats (0x08660d20;
/// 0x08660d10). `rotationFindCloser` divides by the float and takes the remainder by the
/// double of the same value (0x08660d18).
const TWO_PI: f64 = core::f64::consts::TAU;
const PI: f64 = core::f64::consts::PI;
const TWO_PI_F32: f32 = 6.283_185_482_025_146_5;
const PI_F32: f32 = 3.141_592_741_012_573_2;
/// The double 0.5 by which a step that overshoots is halved (0x08660d08, 0x08660da0).
const HALF: f64 = 0.5;
/// The double 0.0008 (0x08660d68, 0x08660da8): the solver's tolerance as a fraction of the
/// limb's length.
const TOLERANCE: f64 = 0.0008;
/// The double 0.05 (0x08660d90, 0x08660dd0) and its float forms: the smallest bend a joint
/// may start the iteration with.
const MIN_BEND: f64 = 0.05;
const MIN_BEND_F32: f32 = 0.050_000_000_745_058_06;
/// How deep a step may be halved (the test is `depth > 20`).
const MAX_DEPTH: i32 = 20;

/// How often the iteration's routines were entered, per thread: steps in the plane
/// (`computeJoints2d`), steps in space (`computeJoints3d`), pseudo-inverses made. The
/// emulation check compares these with the original's (finding 0022, "Checks").
pub mod rounds {
    use std::cell::Cell;

    thread_local! {
        static COUNT: Cell<[u32; 3]> = const { Cell::new([0; 3]) };
    }

    pub(crate) fn note(i: usize) {
        COUNT.with(|c| {
            let mut v = c.get();
            v[i] += 1;
            c.set(v);
        });
    }

    /// The counts since the last call, which are then cleared.
    pub fn take() -> [u32; 3] {
        COUNT.with(|c| c.replace([0; 3]))
    }
}

fn v(a: f32, b: f32, c: f32) -> Vector {
    Vector { x: a, y: b, z: c }
}

const ZERO: Vector = Vector { x: 0.0, y: 0.0, z: 0.0 };

/// The father, where the original follows the pointer without testing it; counted as a
/// fault when null.
fn father(models: &Models, node: NodeId) -> Option<NodeId> {
    let f = models.father(node);
    if f.is_none() {
        fault::note();
    }
    f
}

/// `grot` of the node's father (the identity, counted, where there is none).
fn father_grot(models: &Models, node: NodeId) -> Matrix {
    father(models, node).map_or(IDENTITY, |f| models.m(f).grot)
}

fn extract(m: &Matrix, previous: Vector, g: &Globals) -> Vector {
    tv(matrix_extract_xyz_rotation(m, &mv(previous), g.model.keep_turns))
}

/// On which side of the line from `a` to `b` the point `p` lies, in the XY plane
/// [soft 0x084aea70 pointSide2d]: −1 when (p.y − a.y)(b.x − a.x) − (p.x − a.x)(b.y − a.y)
/// is negative, else 1 (zero and NaN included). Class: model. Unlike `pointSide` of
/// `algebra.c` there is no tolerance and no zero result.
pub fn point_side_2d(p: &Vector, a: &Vector, b: &Vector) -> i32 {
    let t1 = x(p.x) - a.x;
    let t2 = x(p.y) - a.y;
    let t3 = x(b.x) - a.x;
    let t4 = x(b.y) - a.y;
    if (t2 * t3 - t1 * t4).lt(F80::ZERO) { -1 } else { 1 }
}

/// The angle at `b` between the directions to `a` and to `c`, signed by the side `c` lies
/// on [soft 0x084aeae0 pointsAngleSigned2d]: `SI_vectorAngle(a − b, c − b)` times
/// [`point_side_2d`]`(c, a, b)`, **returned unrounded**. Class: libm.
pub fn points_angle_signed_2d(a: &Vector, b: &Vector, c: &Vector) -> F80 {
    let v1 = MVector::new((x(a.x) - b.x).to_f32(), (x(a.y) - b.y).to_f32(), (x(a.z) - b.z).to_f32());
    let v2 = MVector::new((x(c.x) - b.x).to_f32(), (x(c.y) - b.y).to_f32(), (x(c.z) - b.z).to_f32());
    let angle = vector_angle(&v1, &v2);
    let side = point_side_2d(c, a, b);
    F80::from_i32(side) * angle
}

/// Brings the angles of a chain's joints into (−2π, 2π) [soft 0x084aeb58 jointAngleInRange]:
/// from `node` down to the first null with algorithm 3 or 4 (not included), each angle
/// becomes r32(r53(fmod(angle, 2π))), the remainder with the sign of the angle. Class: model.
///
/// A chain without an effector makes the original fault; the walk ends at its last node.
pub fn joint_angle_in_range(models: &mut Models, node: NodeId) {
    let mut p = node;
    loop {
        let m = models.m_mut(p);
        if m.kind == 0 && (m.algo_type == 3 || m.algo_type == 4) {
            return;
        }
        let wrap = |a: f32| x(x(a).fmod(x(TWO_PI)).to_f64()).to_f32();
        m.vrot = v(wrap(m.vrot.x), wrap(m.vrot.y), wrap(m.vrot.z));
        match son(models, p) {
            Some(s) => p = s,
            None => return,
        }
    }
}

/// The preferred angle's number of whole turns carried over to an angle
/// [soft 0x084aef50 rotationFindCloser]: `angle` + k × 2π (float), **unrounded**, where k is
/// `preferred` / 2π truncated, plus 1 when r32(r53(fmod(preferred, 2π))) is above π (float)
/// and minus 1 when it is below −π. Class: model.
pub fn rotation_find_closer(preferred: f32, angle: f32) -> F80 {
    let mut k = (x(preferred) / TWO_PI_F32).to_i32(Round::Zero);
    let r = x(x(preferred).fmod(x(TWO_PI_F32 as f64)).to_f64()).to_f32();
    if x(r).gt(x(PI_F32)) {
        k = k.wrapping_add(1);
    } else if x(r).lt(x(-PI_F32)) {
        k = k.wrapping_sub(1);
    }
    x(angle) + x(TWO_PI_F32) * F80::from_i32(k)
}

/// A global point in the frame the limb is solved in [soft 0x084aefe4 mapGoalToRoot]: the
/// point minus the first joint's position (each difference rounded to float), then through
/// the transpose of the `grot` of the joint's father, each component rounded once.
/// Class: model.
pub fn map_goal_to_root(models: &Models, node: NodeId, goal: Vector, g: &Globals) -> Vector {
    let p = pos(models, node, g);
    let o = v((x(goal.x) - p.x).to_f32(), (x(goal.y) - p.y).to_f32(), (x(goal.z) - p.z).to_f32());
    let r = father_grot(models, node);
    v(
        ((x(r[0][1]) * o.y + x(r[0][0]) * o.x) + x(r[0][2]) * o.z).to_f32(),
        ((x(r[1][0]) * o.x + x(r[1][1]) * o.y) + x(r[1][2]) * o.z).to_f32(),
        ((x(o.y) * r[2][1] + x(o.x) * r[2][0]) + x(o.z) * r[2][2]).to_f32(),
    )
}

/// A row vector through the upper three rows of a matrix **and its row 3**, as these
/// routines write it out: ((y·m₁ⱼ + x·m₀ⱼ) + z·m₂ⱼ) + m₃ⱼ.
fn through(p: Vector, m: &Matrix) -> Vector {
    let col = |j: usize| (((x(p.y) * m[1][j] + x(p.x) * m[0][j]) + x(p.z) * m[2][j]) + m[3][j]).to_f32();
    v(col(0), col(1), col(2))
}

/// The same as the compiler laid it out for a rotation by a roll: x and y as [`through`],
/// z as (z·m₂₂ + (y·m₁₂ + x·m₀₂)) + m₃₂.
fn through_roll(p: Vector, m: &Matrix) -> Vector {
    let col = |j: usize| (((x(p.y) * m[1][j] + x(p.x) * m[0][j]) + x(p.z) * m[2][j]) + m[3][j]).to_f32();
    let z = ((x(p.z) * m[2][2] + (x(p.y) * m[1][2] + x(p.x) * m[0][2])) + m[3][2]).to_f32();
    v(col(0), col(1), z)
}

/// A unit vector along `d`, or `None` when r32(|d|) is below 0.0001: each component times
/// the unrounded reciprocal of the float length. `len` is r32(√(…)) of the sum of squares
/// in the caller's order.
fn unit(d: [F80; 3], len: f32) -> Option<Vector> {
    match x(len).compare(x(EPS)) {
        Some(Ordering::Less) | None => None,
        _ => {
            let inv = F80::ONE / len;
            Some(v((d[0] * inv).to_f32(), (d[1] * inv).to_f32(), (d[2] * inv).to_f32()))
        }
    }
}

/// The resolution plane of a limb for a goal [soft 0x084af0cc findResolutionPlane]: the
/// orientation the limb's first joint has when the limb lies stretched towards the goal.
/// Class: libm. Returns the matrix; `goal` (global on entry) is replaced by the goal **in
/// the plane's frame**, where it lies along +X.
///
/// 1. The goal is brought into the frame of the joint's father ([`map_goal_to_root`]).
/// 2. The plane's second vector, by the joint's `cns_type`: for 2 the constraint point in
///    the same frame is taken as the direction the plane's Y axis leans to; otherwise u × goal
///    is, with u the constraint point for 1 and (0, 0, 0) for 0, and **u replaced by
///    (0, 0, 1) when each of its components is below 0.0001 in size**. So an unconstrained
///    limb bends in the plane that holds the goal and has the father's Z axis as its normal
///    side.
/// 3. The matrix is `SI_matrixAlignAxes(goal, second)`: rows X along the goal, Y, Z.
/// 4. With `with_roll` and a roll (the joint's `prot.x`) above 0.0001 in size the matrix is
///    multiplied by the rotation by the roll about the goal's direction (the newer way), or
///    the goal was first turned by the roll about X (the old way,
///    [`Globals::old_joint_roll`]).
///
/// It also leaves two vectors in the joint's record: `plane_up`, the second vector of step 2
/// in global terms (through the father's `grot`), and `plane_roll`, the same turned by the
/// roll; nothing in the files of this crate reads them back.
///
/// `chain_type` matters only in the old way: for 20 the roll of the **second** joint turns
/// `plane_roll` about the direction from the first joint to the global goal.
pub fn find_resolution_plane(models: &mut Models, ik: &mut IkStore, node: NodeId, chain_type: i32, with_roll: i16, goal: &mut Vector, g: &Globals) -> Matrix {
    let goal_in = *goal;
    let mut rolled = false;
    let mut rot: Matrix = [[0.0; 4]; 4];
    let mut axis = ZERO;
    let mut gl = map_goal_to_root(models, node, goal_in, g);
    let rec = *ik.force(models, node);
    let mut roll = rec.prot.x;
    if g.old_joint_roll && with_roll != 0 && x(roll).abs().gt(x(EPS)) {
        rolled = true;
        rot = matrix_x_rotation(roll);
        gl = v(
            ((x(rot[0][1]) * gl.y + x(rot[0][0]) * gl.x) + x(rot[0][2]) * gl.z).to_f32(),
            (x(rot[1][2]) * gl.z + (x(rot[1][1]) * gl.y + x(rot[1][0]) * gl.x)).to_f32(),
            (x(gl.z) * rot[2][2] + (x(gl.y) * rot[2][1] + x(gl.x) * rot[2][0])).to_f32(),
        );
    }
    let mut up;
    let second;
    if rec.cns_type == 2 {
        up = map_goal_to_root(models, node, rec.cns_pos, g);
        second = up;
    } else {
        up = ZERO;
        if rec.cns_type == 1 {
            up = map_goal_to_root(models, node, rec.cns_pos, g);
        }
        let small = |c: f32| x(EPS).gt((x(c) - 0.0f32).abs());
        if small(up.x) && small(up.y) && small(up.z) {
            up = v(0.0, 0.0, 1.0);
        }
        second = v((x(up.y) * gl.z - x(up.z) * gl.y).to_f32(), (x(up.z) * gl.x - x(gl.z) * up.x).to_f32(), (x(gl.y) * up.x - x(gl.x) * up.y).to_f32());
    }
    let (mut plane, _) = matrix_align_axes(&mv(gl), &mv(second));
    if !g.old_joint_roll && with_roll != 0 && x(EPS).lt(x(roll).abs()) {
        let len = ((x(gl.y) * gl.y + x(gl.x) * gl.x) + x(gl.z) * gl.z).sqrt().to_f32();
        if let Some(a) = unit([x(gl.x), x(gl.y), x(gl.z)], len) {
            axis = a;
            rolled = true;
            rot = matrix_axis_rotation(roll, &mv(axis));
        }
    }
    let in_plane = v(
        ((x(plane[0][1]) * gl.y + x(plane[0][0]) * gl.x) + x(plane[0][2]) * gl.z).to_f32(),
        ((x(plane[1][0]) * gl.x + x(plane[1][1]) * gl.y) + x(plane[1][2]) * gl.z).to_f32(),
        ((x(gl.y) * plane[2][1] + x(gl.x) * plane[2][0]) + x(gl.z) * plane[2][2]).to_f32(),
    );
    if rolled {
        plane = matrix_multiply(&plane, &rot);
    }
    if g.old_joint_roll && rolled {
        up = through_roll(up, &rot);
    }
    let fg = father_grot(models, node);
    let up_world = through(up, &fg);
    let mut up_rolled = up_world;
    if g.old_joint_roll {
        if chain_type == 20 {
            if let Some(s) = son(models, node) {
                roll = ik.force(models, s).prot.x;
                if x(EPS).lt(x(roll).abs()) {
                    let p = models.m(node).gtrs;
                    let d = [x(goal_in.x) - p.x, x(goal_in.y) - p.y, x(goal_in.z) - p.z];
                    let len = ((d[1] * d[1] + d[0] * d[0]) + d[2] * d[2]).sqrt().to_f32();
                    if let Some(a) = unit(d, len) {
                        rot = matrix_axis_rotation(roll, &mv(a));
                        up_rolled = through_roll(up_rolled, &rot);
                    }
                }
            }
        }
    } else if rolled {
        let a = through(axis, &fg);
        rot = matrix_axis_rotation(roll, &mv(a));
        up_rolled = through_roll(up_rolled, &rot);
    }
    let rec = ik.force(models, node);
    rec.plane_roll = up_rolled;
    rec.plane_up = up_world;
    *goal = in_plane;
    plane
}

/// Applies the joints' rolls to a solved planar limb, the newer way
/// [soft 0x084af8e4 chain2DRollApply]. Class: libm.
///
/// From the first joint below `node` (among the limb's joints 1 to `n` − 1) whose roll
/// (`prot.x`) is above 0.0001 in size, every joint's angles become those of
/// Rx(rolls so far, this joint's included) × Rz(the joint's z angle) × Rx(rolls before it)ᵀ,
/// the sum of rolls kept as a float. A joint without a roll of its own adds nothing to the
/// sum. Nothing changes when no joint has a roll.
pub fn chain_2d_roll_apply(models: &mut Models, ik: &mut IkStore, node: NodeId, n: i32, g: &Globals) {
    let mut sum = 0.0f32;
    let mut a: Matrix = matrix_copy(&IDENTITY);
    let mut i = 1;
    let Some(mut p) = son(models, node) else { return };
    loop {
        if i >= n {
            return;
        }
        let roll = ik.force(models, p).prot.x;
        match x(roll).abs().compare(x(EPS)) {
            Some(Ordering::Less | Ordering::Equal) => {}
            _ => break,
        }
        let Some(s) = son(models, p) else { return };
        p = s;
        i += 1;
    }
    for left in (1..=(n - i)).rev() {
        let mut b = matrix_transpose(&a);
        let z = matrix_z_rotation(models.m(p).vrot.z);
        b = matrix_multiply(&z, &b);
        let roll = ik.force(models, p).prot.x;
        if x(roll).abs().gt(x(EPS)) {
            sum = (x(roll) + sum).to_f32();
            a = matrix_x_rotation(sum);
        }
        b = matrix_multiply(&a, &b);
        let prev = models.m(p).vrot;
        models.m_mut(p).vrot = extract(&b, prev, g);
        if left > 1 {
            let Some(s) = son(models, p) else { return };
            p = s;
        }
    }
}

/// The preferred bend's sign after the angle has been brought into (−π, π]: what
/// [`invkin1`] hands to the closed form as "bend upwards".
fn bends_up(preferred: f32) -> bool {
    let mut r = x(x(preferred).fmod(x(TWO_PI)).to_f64()).to_f32();
    if x(r).gt(x(PI)) {
        r = (x(r) - TWO_PI).to_f32();
    } else if x(r).lt(x(-PI)) {
        r = (x(TWO_PI) + r).to_f32();
    }
    F80::ZERO.lt(x(r))
}

/// A limb of one joint [soft 0x084afa1c invkin0]: the joint's angles become those of the
/// resolution plane, so that the bone points at the goal. Class: libm.
///
/// Nothing happens when the bone is shorter than 0.0001 or the goal is at the joint
/// ([`VOL_AT_ROOT`]). The z angle is then moved by the preferred angle's whole turns
/// ([`rotation_find_closer`]), the angles are wrapped and the node is updated.
pub fn invkin0(models: &mut Models, ik: &mut IkStore, node: NodeId, goal: Vector, g: &Globals, host: &mut dyn Host) {
    let Some(s) = son(models, node) else { return };
    if dist(models.m(node).gtrs, models.m(s).gtrs).lt(x(EPS)) {
        return;
    }
    let (code, mut p) = volume_projection(models, node, 1, goal, g);
    if code == VOL_AT_ROOT {
        return;
    }
    let plane = find_resolution_plane(models, ik, node, 10, 1, &mut p, g);
    let prev = models.m(node).vrot;
    models.m_mut(node).vrot = extract(&plane, prev, g);
    finish_first_joint(models, ik, node, g);
    joint_angle_in_range(models, node);
    let f = models.father(node);
    update_son_state(models, f, node, true, &g.model, host);
}

/// `vrot.z` = r32([`rotation_find_closer`](the joint's preferred z, `vrot.z`)).
fn finish_first_joint(models: &mut Models, ik: &mut IkStore, node: NodeId, _g: &Globals) {
    let pref = ik.force(models, node).prot.z;
    let m = models.m_mut(node);
    m.vrot.z = rotation_find_closer(pref, m.vrot.z).to_f32();
}

/// A limb of two joints of a planar chain, in closed form [soft 0x084afb1c invkin1].
/// Class: libm.
///
/// Nothing happens when a bone (measured between the nodes' positions, rounded to float)
/// is shorter than 0.0001 or the goal is at the first joint. The goal is brought into
/// reach ([`volume_projection`]) and into the resolution plane; the two angles come from
/// [`solve_planar_limb`], bending to the side the **second joint's preferred z angle** has
/// (brought into (−π, π] first; zero and negative bend "down"). The first joint's angles
/// are those of Rz(first angle) × plane; the second joint gets (0, 0, bend). Then the rolls
/// ([`chain_2d_roll_apply`]), the preferred angles' whole turns on both z angles, wrapping
/// and the node update.
///
/// The old way of rolling ([`Globals::old_joint_roll`]) with a roll on the second joint:
/// the first joint turns by its angle about the Z axis of Rx(roll) instead, and the second
/// joint's angles are those of Rz(bend) × Rx(roll).
pub fn invkin1(models: &mut Models, ik: &mut IkStore, node: NodeId, goal: Vector, g: &Globals, host: &mut dyn Host) {
    let Some(s) = son(models, node) else { return };
    let Some(e) = son(models, s) else { return };
    let l1 = dist(pos(models, node, g), pos(models, s, g)).to_f32();
    let (ps, pe) = (pos(models, s, g), pos(models, e, g));
    let (dx, dy, dz) = (x(ps.x) - pe.x, x(ps.y) - pe.y, x(ps.z) - pe.z);
    let l2 = ((dx * dx + dy * dy) + dz * dz).sqrt().to_f32();
    if x(EPS).gt(x(l1)) || x(EPS).gt(x(l2)) {
        return;
    }
    let (code, mut p) = volume_projection(models, node, 2, goal, g);
    if code == VOL_AT_ROOT {
        return;
    }
    let plane = find_resolution_plane(models, ik, node, 20, 1, &mut p, g);
    let srec = *ik.force(models, s);
    let mut old_roll = None;
    if g.old_joint_roll && x(srec.prot.x).abs().gt(x(EPS)) {
        old_roll = Some(matrix_x_rotation(srec.prot.x));
    }
    let up = bends_up(srec.prot.z);
    let (a1, a2) = solve_planar_limb(l1, l2, p.x, p.y, up as i16);
    match old_roll {
        Some(rx) => {
            let axis = MVector::new(fmov(rx[2][0]), fmov(rx[2][1]), fmov(rx[2][2]));
            let m = matrix_multiply(&matrix_axis_rotation(a1, &axis), &plane);
            let prev = models.m(node).vrot;
            models.m_mut(node).vrot = extract(&m, prev, g);
            let m = matrix_multiply(&matrix_z_rotation(a2), &rx);
            let prev = models.m(s).vrot;
            models.m_mut(s).vrot = extract(&m, prev, g);
        }
        None => {
            let m = matrix_multiply(&matrix_z_rotation(a1), &plane);
            let prev = models.m(node).vrot;
            models.m_mut(node).vrot = extract(&m, prev, g);
            models.m_mut(s).vrot = v(0.0, 0.0, fmov(a2));
        }
    }
    if !g.old_joint_roll {
        chain_2d_roll_apply(models, ik, node, 2, g);
    }
    finish_first_joint(models, ik, node, g);
    finish_first_joint(models, ik, s, g);
    joint_angle_in_range(models, node);
    let f = models.father(node);
    update_son_state(models, f, node, true, &g.model, host);
}

/// The point between `p` and `q` that lies `d1` from `p` when the two are `d2` apart
/// [soft 0x084aff6c si_getInsertPoint]. Class: model.
///
/// `p` when `d1` is zero; `q` when `d1` equals `d2`; otherwise, with t = d1 / (d2 − d1)
/// in the register, each component is r32((p + q × t) / (1 + t)), **or `q`'s own when the
/// two components differ by less than 0.0001**.
pub fn get_insert_point(d1: f32, d2: f32, p: Vector, q: Vector) -> Vector {
    if x(d1).eq(F80::ZERO) {
        return v(fmov(p.x), fmov(p.y), p.z);
    }
    if x(d2).eq(x(d1)) {
        return v(fmov(q.x), fmov(q.y), q.z);
    }
    let t = x(d1) / (x(d2) - d1);
    let one = F80::ONE + t;
    let c = |a: f32, b: f32| -> f32 {
        if (x(a) - b).abs().lt(x(EPS)) { fmov(b) } else { ((x(a) + x(b) * t) / one).to_f32() }
    };
    v(c(p.x, q.x), c(p.y, q.y), c(p.z, q.z))
}

/// The two matrices of a root for a limb of `n` joints: the stored ones cut to the limb's
/// size (the original shortens them for the time of the solve by writing the counts into
/// their headers). Missing or too small matrices, where the original reads through a null
/// pointer or past a block, are made here and counted as a fault.
struct Work {
    j: GenMatrixDbl,
    inv: GenMatrixDbl,
    rows: usize,
    cols: usize,
}

impl Work {
    fn take(models: &mut Models, ik: &mut IkStore, root: NodeId, rows: usize, cols: usize) -> Work {
        let rec = *ik.force(models, root);
        let mut w = Work { j: GenMatrixDbl { rows, cols, data: vec![0.0; rows * cols] }, inv: GenMatrixDbl { rows: cols, cols: rows, data: vec![0.0; rows * cols] }, rows, cols };
        match ik.matrix(rec.jacobian) {
            Some(m) if m.rows >= rows && m.cols >= cols => {
                for r in 0..rows {
                    for c in 0..cols {
                        w.j.data[r * cols + c] = m.data[r * m.cols + c];
                    }
                }
            }
            _ => fault::note(),
        }
        match ik.matrix(rec.inverse) {
            Some(m) if m.rows >= cols && m.cols >= rows => {
                for r in 0..cols {
                    for c in 0..rows {
                        w.inv.data[r * rows + c] = m.data[r * m.cols + c];
                    }
                }
            }
            _ => fault::note(),
        }
        w
    }

    fn put(self, models: &mut Models, ik: &mut IkStore, root: NodeId) {
        let (rows, cols) = (self.rows, self.cols);
        let rec = *ik.force(models, root);
        let fits = |m: Option<&GenMatrixDbl>, r: usize, c: usize| m.is_some_and(|m| m.rows >= r && m.cols >= c);
        if !fits(ik.matrix(rec.jacobian), rows, cols) {
            let p = ik.add_matrix(general_matrix_allocate_dbl(rows as i32, cols as i32));
            ik.force(models, root).jacobian = p;
        }
        if !fits(ik.matrix(rec.inverse), cols, rows) {
            let p = ik.add_matrix(general_matrix_allocate_dbl(cols as i32, rows as i32));
            ik.force(models, root).inverse = p;
        }
        let rec = *ik.force(models, root);
        if let Some(m) = ik.matrix_mut(rec.jacobian) {
            for r in 0..rows {
                for c in 0..cols {
                    m.data[r * m.cols + c] = self.j.data[r * cols + c];
                }
            }
        }
        if let Some(m) = ik.matrix_mut(rec.inverse) {
            for r in 0..cols {
                for c in 0..rows {
                    m.data[r * m.cols + c] = self.inv.data[r * rows + c];
                }
            }
        }
    }
}

/// The Jacobian of a planar limb [soft 0x084aebd8 computeJacobian2d]: column i is the
/// velocity of the limb's end for a turn of joint i about Z, (−(e.y − p.y), e.x − p.x) with
/// e the end's position and p the joint's. Class: model.
///
/// **The two rows are rounded differently**: row 0 is the negated difference straight from
/// the register to double, row 1 the difference rounded to float first.
pub fn compute_jacobian_2d(models: &Models, node: NodeId, j: &mut GenMatrixDbl, n: i32) {
    let e = limb_position(models, node, n);
    let mut p = node;
    for i in 0..n.max(1) as usize {
        let at = models.m(p).gtrs;
        let dx = (x(e.x) - at.x).to_f32();
        let dy = x(e.y) - at.y;
        j.set(0, i, (-dy).to_f64());
        j.set(1, i, x(dx).to_f64());
        if i + 1 < n.max(1) as usize {
            match son(models, p) {
                Some(s) => p = s,
                None => return,
            }
        }
    }
}

/// One step of a planar limb towards `target`, halved as long as it lands too far off
/// [soft 0x084aecc0 computeJoints2d]. Class: libm.
///
/// With e = target − the limb's end (floats): if `recompute`, the Jacobian and its
/// pseudo-inverse (Greville's method, `si-math`) are made anew; each joint's z angle grows
/// by r32(e.x × inverse[i][0] + e.y × inverse[i][1]) (the products with the **doubles** of
/// the matrix, summed in the register), the change kept in the joint's `drot.z`; the chain
/// is updated. If the end is then further than `tolerance` from the target, the step is
/// taken back and replaced by two: one to the point halfway (brought into reach by
/// [`volume_projection`]) with the same matrices and twice the tolerance, then one to the
/// target with new matrices. Nothing is done at depth 21.
#[allow(clippy::too_many_arguments)]
fn compute_joints_2d(models: &mut Models, ik: &mut IkStore, w: &mut Work, depth: i32, node: NodeId, n: i32, recompute: bool, tolerance: f32, target: Vector, g: &Globals, host: &mut dyn Host) {
    rounds::note(0);
    if depth > MAX_DEPTH {
        return;
    }
    let e0 = limb_position(models, node, n);
    let mut e = v((x(target.x) - e0.x).to_f32(), (x(target.y) - e0.y).to_f32(), (x(target.z) - e0.z).to_f32());
    if recompute {
        compute_jacobian_2d(models, node, &mut w.j, n);
        // SI_generalMatrixGrevilleInverseDbl always answers 1
        rounds::note(2);
        general_matrix_greville_inverse_dbl(&w.j, &mut w.inv);
    }
    let mut p = node;
    for i in 0..n.max(0) as usize {
        let d = (x(e.x) * w.inv.get(i, 0) + x(e.y) * w.inv.get(i, 1)).to_f32();
        ik.force(models, p).drot.z = d;
        let m = models.m_mut(p);
        m.vrot.z = (x(m.vrot.z) + d).to_f32();
        if i + 1 < n as usize {
            match son(models, p) {
                Some(s) => p = s,
                None => break,
            }
        }
    }
    chain_update(models, ik, node, 0, g, host);
    let e1 = limb_position(models, node, n);
    let off = dist(target, e1).to_f32();
    if !x(off).gt(x(tolerance)) {
        return;
    }
    e = v((x(HALF) * e.x).to_f32(), (x(HALF) * e.y).to_f32(), (x(HALF) * e.z).to_f32());
    let mid = v((x(e.x) + e0.x).to_f32(), (x(e.y) + e0.y).to_f32(), (x(e.z) + e0.z).to_f32());
    let (_, near) = volume_projection(models, node, n, mid, g);
    let mut p = node;
    for i in 0..n.max(0) {
        let d = ik.force(models, p).drot.z;
        let m = models.m_mut(p);
        m.vrot.z = (x(m.vrot.z) - d).to_f32();
        if i + 1 < n {
            match son(models, p) {
                Some(s) => p = s,
                None => break,
            }
        }
    }
    chain_update(models, ik, node, 0, g, host);
    let doubled = (x(tolerance) + tolerance).to_f32();
    compute_joints_2d(models, ik, w, depth + 1, node, n, false, doubled, near, g, host);
    compute_joints_2d(models, ik, w, depth + 1, node, n, true, tolerance, target, g, host);
}

/// [soft 0x084aecc0 computeJoints2d] on the root's stored matrices: see
/// [`compute_joints_2d`]'s description above. `root` is the node whose record holds the
/// matrices.
#[allow(clippy::too_many_arguments)]
pub fn compute_joints_2d_stored(models: &mut Models, ik: &mut IkStore, root: NodeId, depth: i32, node: NodeId, n: i32, recompute: bool, tolerance: f32, target: Vector, g: &Globals, host: &mut dyn Host) {
    let mut w = Work::take(models, ik, root, 2, n.max(1) as usize);
    compute_joints_2d(models, ik, &mut w, depth, node, n, recompute, tolerance, target, g, host);
    w.put(models, ik, root);
}

/// Walks a planar limb's end to the goal [soft 0x084b0070 iterComputeJoints2d]. Class: libm.
///
/// While the point reached (at first the limb's end) is further than `reach` from the goal:
/// a step ([`compute_joints_2d`], tolerance `tolerance`) to the point `fraction` of the way
/// to the goal ([`get_insert_point`]), **after which that point, not the limb's end, counts
/// as reached**. With `fraction` 1 that is one step. Afterwards one more step to the goal
/// itself unless the last point aimed at was exactly the goal.
///
/// When the loop does not run at all the original compares the goal with a local it never
/// set and so, but for a chance equality with stack bytes, takes the last step; so does
/// the port.
#[allow(clippy::too_many_arguments)]
fn iter_compute_joints_2d(models: &mut Models, ik: &mut IkStore, w: &mut Work, node: NodeId, n: i32, reach: f32, fraction: f32, tolerance: f32, goal: Vector, g: &Globals, host: &mut dyn Host) {
    let mut cur = limb_position(models, node, n);
    let mut aim: Option<Vector> = None;
    loop {
        let (dx, dy, dz) = (x(goal.x) - cur.x, x(goal.y) - cur.y, x(goal.z) - cur.z);
        let d = (dz * dz + (dy * dy + dx * dx)).sqrt().to_f32();
        if !x(d).gt(x(reach)) {
            break;
        }
        let a = get_insert_point((x(d) * fraction).to_f32(), d, cur, goal);
        compute_joints_2d(models, ik, w, 0, node, n, true, tolerance, a, g, host);
        cur = v(fmov(a.x), fmov(a.y), fmov(a.z));
        aim = Some(a);
    }
    let at_goal = aim.is_some_and(|a| (x(goal.x) - a.x).eq(F80::ZERO) && (x(goal.y) - a.y).eq(F80::ZERO) && (x(goal.z) - a.z).eq(F80::ZERO));
    if !at_goal {
        compute_joints_2d(models, ik, w, 0, node, n, true, tolerance, goal, g, host);
    }
}

/// [soft 0x084b0070 iterComputeJoints2d] on the root's stored matrices
/// ([`iter_compute_joints_2d`] above has the description).
#[allow(clippy::too_many_arguments)]
pub fn iter_compute_joints_2d_stored(models: &mut Models, ik: &mut IkStore, root: NodeId, node: NodeId, n: i32, reach: f32, fraction: f32, tolerance: f32, goal: Vector, g: &Globals, host: &mut dyn Host) {
    let mut w = Work::take(models, ik, root, 2, n.max(1) as usize);
    iter_compute_joints_2d(models, ik, &mut w, node, n, reach, fraction, tolerance, goal, g, host);
    w.put(models, ik, root);
}

/// What the two iterative solvers do first: each joint's angles are noted in its record
/// (`saved_rot`), and the limb is measured. Returns the limb's end node, the distance from
/// the first joint to the end (float), the limb's length (float), the tolerance
/// r32(0.0008 × length) and the shortest bone.
fn enter(models: &mut Models, ik: &mut IkStore, node: NodeId, n: i32, g: &Globals) -> (NodeId, f32, f32, f32, f32) {
    let mut p = node;
    for _ in 0..n.max(0) {
        let r = models.m(p).vrot;
        ik.force(models, p).saved_rot = v(fmov(r.x), fmov(r.y), fmov(r.z));
        match son(models, p) {
            Some(s) => p = s,
            None => break,
        }
    }
    let (a, b) = (pos(models, node, g), pos(models, p, g));
    let (dx, dy, dz) = (x(a.x) - b.x, x(a.y) - b.y, x(a.z) - b.z);
    let span = ((dx * dx + dy * dy) + dz * dz).sqrt().to_f32();
    let (len, _, shortest) = limb_length(models, node, n, g);
    let len = len.to_f32();
    let tolerance = (x(TOLERANCE) * len).to_f32();
    (p, span, len, tolerance, shortest)
}

/// Whether an iterative solver has anything to do: the goal is at least the tolerance away
/// from the limb's end (`gtrs`) and no bone is shorter than 0.0001, or the first joint has
/// a plane constraint.
fn worth_solving(models: &mut Models, ik: &mut IkStore, node: NodeId, end: NodeId, goal: Vector, tolerance: f32, shortest: f32) -> bool {
    let off = dist(goal, models.m(end).gtrs).to_f32();
    let far = !x(off).lt(x(tolerance)) && !x(shortest).lt(x(EPS));
    far || ik.force(models, node).cns_type != 0
}

/// A joint's z angle kept away from zero: below 0.05 in size it becomes +0.05 when
/// positive and −0.05 otherwise.
fn min_bend(z: f32) -> f32 {
    if x(z).abs().lt(x(MIN_BEND)) {
        if x(z).gt(F80::ZERO) { MIN_BEND_F32 } else { -MIN_BEND_F32 }
    } else {
        z
    }
}

/// A limb of three or more joints of a planar chain [soft 0x084b0254 invkin2d].
/// Class: libm. `root` holds the matrices; `first_pass` is the caller's (1 on the first
/// solve of a goal, 0 on a re-solve).
///
/// 1. The joints' angles are saved. Nothing more happens when the goal is within the
///    tolerance (0.0008 × the limb's length) of the limb's end or a bone is shorter than
///    0.0001, unless the first joint has a plane constraint; nor when the goal is at the
///    first joint.
/// 2. The goal is brought into reach and into the resolution plane.
/// 3. Out of reach ([`VOL_TOO_FAR`]): all angles zero, the limb stretched.
/// 4. Otherwise the start pose: the first joint's angles are zero; on a first pass with the
///    first joint's `use_pref` set every other joint gets (0, 0, its preferred z); else
///    every other joint keeps its z, moved to ±0.05 where it is smaller, with x and y zero.
///    The chain is updated and the first joint turned about Z by the signed angle at it
///    between the second joint and the limb's end, which puts the end on the X axis. Then
///    the iteration ([`iter_compute_joints_2d`]) with reach 0.0005 and fraction 1, or, when
///    the limb is stretched to within 1e-5 of its length, reach 0.0001 (float) and
///    fraction 0.718.
/// 5. The first joint's angles become those of Rz(its z) × plane, taken out with the saved
///    angles as the previous solution; then rolls, the preferred angle's whole turns,
///    wrapping and the node update, as in [`invkin1`].
#[allow(clippy::too_many_arguments)]
pub fn invkin2d(models: &mut Models, ik: &mut IkStore, root: NodeId, node: NodeId, n: i32, goal: Vector, first_pass: i16, g: &Globals, host: &mut dyn Host) {
    let (end, span, len, tolerance, shortest) = enter(models, ik, node, n, g);
    if !worth_solving(models, ik, node, end, goal, tolerance, shortest) {
        return;
    }
    let (code, mut p) = volume_projection(models, node, n, goal, g);
    if code == VOL_AT_ROOT {
        return;
    }
    let plane = find_resolution_plane(models, ik, node, 30, 1, &mut p, g);
    if code == VOL_TOO_FAR {
        let mut q = node;
        for i in 0..n {
            models.m_mut(q).vrot = ZERO;
            if i + 1 < n {
                match son(models, q) {
                    Some(s) => q = s,
                    None => break,
                }
            }
        }
    } else {
        let (mut reach, mut fraction) = (0.000_500_000_023_748_725_7f32, 1.0f32);
        if (x(len) - span).abs().lt(x(EPS5)) {
            reach = 9.999_999_747_378_752e-5;
            fraction = 0.717_999_994_754_791_3;
        }
        let preferred = first_pass != 0 && ik.force(models, node).use_pref != 0;
        models.m_mut(node).vrot = ZERO;
        let mut q = node;
        for _ in 1..n {
            let Some(s) = son(models, q) else { break };
            q = s;
            let z = if preferred { fmov(ik.force(models, q).prot.z) } else { min_bend(models.m(q).vrot.z) };
            models.m_mut(q).vrot = v(0.0, 0.0, z);
        }
        chain_update(models, ik, node, 0, g, host);
        let e = models.m(down(models, node, n)).gtrs;
        let second = son(models, node).map_or(ZERO, |s| models.m(s).gtrs);
        let first = models.m(node).gtrs;
        models.m_mut(node).vrot.z = points_angle_signed_2d(&second, &first, &e).to_f32();
        chain_update(models, ik, node, 0, g, host);
        let mut w = Work::take(models, ik, root, 2, n as usize);
        iter_compute_joints_2d(models, ik, &mut w, node, n, reach, fraction, tolerance, p, g, host);
        w.put(models, ik, root);
    }
    let m = matrix_multiply(&matrix_z_rotation(models.m(node).vrot.z), &plane);
    let saved = ik.force(models, node).saved_rot;
    let saved = v(fmov(saved.x), fmov(saved.y), fmov(saved.z));
    models.m_mut(node).vrot = extract(&m, saved, g);
    if !g.old_joint_roll {
        chain_2d_roll_apply(models, ik, node, n, g);
    }
    finish_first_joint(models, ik, node, g);
    joint_angle_in_range(models, node);
    let f = models.father(node);
    update_son_state(models, f, node, true, &g.model, host);
}

/// The Jacobian of a spatial limb [soft 0x084b07f4 computeJacobian3d]: three columns per
/// joint, column 3i + k the velocity of the limb's end for a turn of joint i about row k of
/// its `grot`: axis × (e − p). Class: model.
///
/// **Row 0 goes from the register to double; rows 1 and 2 are rounded to float first.**
/// The difference e − p is rounded to float before the products.
pub fn compute_jacobian_3d(models: &Models, node: NodeId, j: &mut GenMatrixDbl, n: i32) {
    let e = limb_position(models, node, n);
    let mut p = node;
    let mut col = 0;
    for i in 0..n.max(0) {
        let m = models.m(p);
        let dx = (x(e.x) - m.gtrs.x).to_f32();
        let dy = (x(e.y) - m.gtrs.y).to_f32();
        let dz = (x(e.z) - m.gtrs.z).to_f32();
        for k in 0..3 {
            let (a0, a1, a2) = (m.grot[k][0], m.grot[k][1], m.grot[k][2]);
            j.set(0, col, (x(a1) * dz - x(a2) * dy).to_f64());
            j.set(1, col, x((x(a2) * dx - x(dz) * a0).to_f32()).to_f64());
            j.set(2, col, x((x(dy) * a0 - x(dx) * a1).to_f32()).to_f64());
            col += 1;
        }
        if i + 1 < n {
            match son(models, p) {
                Some(s) => p = s,
                None => return,
            }
        }
    }
}

/// One step of a spatial limb towards `target` [soft 0x084b0920 computeJoints3d].
/// Class: libm.
///
/// As [`compute_joints_2d`], with three small angles per joint: δ = inverse rows 3i, 3i + 1,
/// 3i + 2 times e, each r32((q₀·e.x + q₁·e.y) + q₂·e.z) with the matrix's doubles; the
/// joint's new angles are those of R(δ) × `mrot` (taken out with the present angles as the
/// previous solution), and `drot` the change. A step that lands further than `tolerance`
/// from the target is taken back and split as in the plane, **but the first half keeps the
/// same tolerance**. At depth 21 `*stop` is set; a set `*stop` keeps the second half from
/// running.
#[allow(clippy::too_many_arguments)]
fn compute_joints_3d(models: &mut Models, ik: &mut IkStore, w: &mut Work, depth: i32, node: NodeId, n: i32, recompute: bool, tolerance: f32, target: Vector, stop: &mut i16, g: &Globals, host: &mut dyn Host) {
    rounds::note(1);
    if depth > MAX_DEPTH {
        *stop = 1;
        return;
    }
    let e0 = limb_position(models, node, n);
    let mut e = v((x(target.x) - e0.x).to_f32(), (x(target.y) - e0.y).to_f32(), (x(target.z) - e0.z).to_f32());
    if recompute {
        compute_jacobian_3d(models, node, &mut w.j, n);
        rounds::note(2);
        general_matrix_greville_inverse_dbl(&w.j, &mut w.inv);
    }
    let mut p = node;
    for i in 0..n.max(0) as usize {
        let row = |r: usize| ((x(w.inv.get(r, 0)) * e.x + x(w.inv.get(r, 1)) * e.y) + x(w.inv.get(r, 2)) * e.z).to_f32();
        let delta = MVector::new(row(3 * i), row(3 * i + 1), row(3 * i + 2));
        let turned = matrix_multiply_sub3(&matrix_xyz_rotation(&delta), &models.m(p).mrot);
        let old = models.m(p).vrot;
        let new = extract(&turned, old, g);
        ik.force(models, p).drot = v((x(new.x) - old.x).to_f32(), (x(new.y) - old.y).to_f32(), (x(new.z) - old.z).to_f32());
        models.m_mut(p).vrot = new;
        if i + 1 < n as usize {
            match son(models, p) {
                Some(s) => p = s,
                None => break,
            }
        }
    }
    chain_update(models, ik, node, 0, g, host);
    let e1 = limb_position(models, node, n);
    let off = dist(target, e1).to_f32();
    if !x(off).gt(x(tolerance)) {
        return;
    }
    e = v((x(HALF) * e.x).to_f32(), (x(HALF) * e.y).to_f32(), (x(HALF) * e.z).to_f32());
    let mid = v((x(e.x) + e0.x).to_f32(), (x(e.y) + e0.y).to_f32(), (x(e.z) + e0.z).to_f32());
    let (_, near) = volume_projection(models, node, n, mid, g);
    let mut p = node;
    for i in 0..n.max(0) {
        let d = ik.force(models, p).drot;
        let m = models.m_mut(p);
        m.vrot = v((x(m.vrot.x) - d.x).to_f32(), (x(m.vrot.y) - d.y).to_f32(), (x(m.vrot.z) - d.z).to_f32());
        if i + 1 < n {
            match son(models, p) {
                Some(s) => p = s,
                None => break,
            }
        }
    }
    chain_update(models, ik, node, 0, g, host);
    compute_joints_3d(models, ik, w, depth + 1, node, n, false, tolerance, near, stop, g, host);
    if *stop == 0 {
        compute_joints_3d(models, ik, w, depth + 1, node, n, true, tolerance, target, stop, g, host);
    }
}

/// [soft 0x084b0920 computeJoints3d] on the root's stored matrices
/// ([`compute_joints_3d`] above has the description).
#[allow(clippy::too_many_arguments)]
pub fn compute_joints_3d_stored(models: &mut Models, ik: &mut IkStore, root: NodeId, depth: i32, node: NodeId, n: i32, recompute: bool, tolerance: f32, target: Vector, stop: &mut i16, g: &Globals, host: &mut dyn Host) {
    let mut w = Work::take(models, ik, root, 3, 3 * n.max(1) as usize);
    compute_joints_3d(models, ik, &mut w, depth, node, n, recompute, tolerance, target, stop, g, host);
    w.put(models, ik, root);
}

/// Walks a spatial limb's end to the goal [soft 0x084b0d20 iterComputeJoints3d].
/// Class: libm.
///
/// As [`iter_compute_joints_2d`], with two more ways out of the loop: `*stop` set by a
/// step, and **no progress**: when the distance left (a float) is the same as before the
/// step 22 times running. The last step to the goal itself is taken under the same
/// condition as in the plane, whatever `*stop` says.
#[allow(clippy::too_many_arguments)]
fn iter_compute_joints_3d(models: &mut Models, ik: &mut IkStore, w: &mut Work, node: NodeId, n: i32, reach: f32, fraction: f32, tolerance: f32, goal: Vector, stop: &mut i16, g: &Globals, host: &mut dyn Host) {
    let mut cur = limb_position(models, node, n);
    let mut aim: Option<Vector> = None;
    let mut same = 0;
    let measure = |cur: Vector| {
        let (dx, dy, dz) = (x(goal.x) - cur.x, x(goal.y) - cur.y, x(goal.z) - cur.z);
        (dz * dz + (dy * dy + dx * dx)).sqrt().to_f32()
    };
    let mut d = measure(cur);
    while x(d).gt(x(reach)) && *stop == 0 {
        let a = get_insert_point((x(d) * fraction).to_f32(), d, cur, goal);
        compute_joints_3d(models, ik, w, 0, node, n, true, tolerance, a, stop, g, host);
        cur = v(fmov(a.x), fmov(a.y), fmov(a.z));
        aim = Some(a);
        let next = measure(cur);
        let unchanged = x(d).eq(x(next));
        d = next;
        if unchanged {
            if same > 20 {
                break;
            }
            same += 1;
        } else {
            same = 0;
        }
    }
    let at_goal = aim.is_some_and(|a| (x(goal.x) - a.x).eq(F80::ZERO) && (x(goal.y) - a.y).eq(F80::ZERO) && (x(goal.z) - a.z).eq(F80::ZERO));
    if !at_goal {
        compute_joints_3d(models, ik, w, 0, node, n, true, tolerance, goal, stop, g, host);
    }
}

/// [soft 0x084b0d20 iterComputeJoints3d] on the root's stored matrices
/// ([`iter_compute_joints_3d`] above has the description).
#[allow(clippy::too_many_arguments)]
pub fn iter_compute_joints_3d_stored(models: &mut Models, ik: &mut IkStore, root: NodeId, node: NodeId, n: i32, reach: f32, fraction: f32, tolerance: f32, goal: Vector, stop: &mut i16, g: &Globals, host: &mut dyn Host) {
    let mut w = Work::take(models, ik, root, 3, 3 * n.max(1) as usize);
    iter_compute_joints_3d(models, ik, &mut w, node, n, reach, fraction, tolerance, goal, stop, g, host);
    w.put(models, ik, root);
}

/// A point through the upper 3 × 3 of a rotation about Z as [`invkin3d`] and
/// [`check_branch_orient`] write it out; returns x and z **unrounded** beside the rounded
/// vector. The two routines sum x in different orders (`x_first`).
fn through_z(z: &Matrix, p: Vector, x_first: bool) -> (Vector, F80, F80) {
    let px = if x_first { (x(z[0][1]) * p.y + x(z[0][0]) * p.x) + x(z[0][2]) * p.z } else { x(z[0][2]) * p.z + (x(z[0][1]) * p.y + x(z[0][0]) * p.x) };
    let py = if x_first { (x(z[1][0]) * p.x + x(z[1][1]) * p.y) + x(z[1][2]) * p.z } else { x(z[1][2]) * p.z + (x(z[1][1]) * p.y + x(z[1][0]) * p.x) };
    let pz = x(p.z) * z[2][2] + (x(p.y) * z[2][1] + x(p.x) * z[2][0]);
    (v(px.to_f32(), py.to_f32(), pz.to_f32()), px, pz)
}

/// A limb of two or more joints of a spatial chain [soft 0x084b0f48 invkin3d]. Class: libm.
///
/// Steps 1 and 2 as [`invkin2d`]. Out of reach: the first joint's angles become those of
/// the resolution plane (with the roll) and every other joint gets (its preferred x, 0, 0).
/// Otherwise, with the plane made **without** the roll:
///
/// - the start pose: on a first pass with the first joint's `use_pref` set every joint
///   (the first included) gets its preferred angles; else every joint keeps its angles,
///   z moved to ±0.05 where smaller;
/// - the chain is updated and the first joint turned so that the limb's end lies on the
///   X axis: by −azimuth about Z and −elevation about Y, composed after its own rotation
///   (R(angles) × (Rz(−az) × Ry(−el))). The end node's `gtrs` is overwritten on the way
///   with the end turned by Rz(az); the update that follows puts it right;
/// - the iteration ([`iter_compute_joints_3d`]) with reach 0.001 and fraction 1, or, for a
///   limb stretched to within 1e-5, reach 0.0005 and fraction 0.618;
/// - if the iteration gave up (`*stop`), every joint gets its saved angles back; else the
///   first joint's angles become those of R(its angles) × plane, taken out with the saved
///   angles as the previous solution.
///
/// Then wrapping and the node update. No roll is applied afterwards and the preferred
/// angle's turns are not carried over.
#[allow(clippy::too_many_arguments)]
pub fn invkin3d(models: &mut Models, ik: &mut IkStore, root: NodeId, node: NodeId, n: i32, goal: Vector, first_pass: i16, stop: &mut i16, g: &Globals, host: &mut dyn Host) {
    let (end, span, len, tolerance, shortest) = enter(models, ik, node, n, g);
    if !worth_solving(models, ik, node, end, goal, tolerance, shortest) {
        return;
    }
    let (code, mut p) = volume_projection(models, node, n, goal, g);
    if code == VOL_AT_ROOT {
        return;
    }
    if code == VOL_TOO_FAR {
        let plane = find_resolution_plane(models, ik, node, 40, 1, &mut p, g);
        let prev = models.m(node).vrot;
        models.m_mut(node).vrot = extract(&plane, prev, g);
        let mut q = node;
        for _ in 1..n {
            let Some(s) = son(models, q) else { break };
            q = s;
            let roll = fmov(ik.force(models, q).prot.x);
            models.m_mut(q).vrot = v(roll, 0.0, 0.0);
        }
    } else {
        let plane = find_resolution_plane(models, ik, node, 40, 0, &mut p, g);
        let (mut reach, mut fraction) = (0.001_000_000_047_497_451_3f32, 1.0f32);
        if (x(len) - span).abs().lt(x(EPS5)) {
            reach = 0.000_500_000_023_748_725_7;
            fraction = 0.617_999_970_912_933_3;
        }
        let preferred = first_pass != 0 && ik.force(models, node).use_pref != 0;
        let mut q = node;
        for i in 0..n {
            if preferred {
                let pr = ik.force(models, q).prot;
                models.m_mut(q).vrot = v(fmov(pr.x), fmov(pr.y), fmov(pr.z));
            } else {
                let m = models.m_mut(q);
                m.vrot.z = min_bend(m.vrot.z);
            }
            if i + 1 < n {
                match son(models, q) {
                    Some(s) => q = s,
                    None => break,
                }
            }
        }
        chain_update(models, ik, node, 0, g, host);
        let e = models.m(end).gtrs;
        let az = if x(e.x).eq(F80::ZERO) && x(e.y).eq(F80::ZERO) { 0.0 } else { trig::fpatan(x(e.y), x(e.x)).to_f32() };
        let z = matrix_z_rotation(az);
        let (turned, ex, ez) = through_z(&z, e, true);
        models.m_mut(end).gtrs = turned;
        let el = trig::fpatan(-ez, ex).to_f32();
        let y = matrix_y_rotation((-x(el)).to_f32());
        let back = matrix_z_rotation((-x(az)).to_f32());
        let align = matrix_multiply(&back, &y);
        let own = matrix_xyz_rotation(&mv(models.m(node).vrot));
        let m = matrix_multiply(&own, &align);
        let prev = models.m(node).vrot;
        models.m_mut(node).vrot = extract(&m, prev, g);
        chain_update(models, ik, node, 0, g, host);
        let mut w = Work::take(models, ik, root, 3, 3 * n as usize);
        iter_compute_joints_3d(models, ik, &mut w, node, n, reach, fraction, tolerance, p, stop, g, host);
        w.put(models, ik, root);
        if *stop == 0 {
            let m = matrix_multiply(&matrix_xyz_rotation(&mv(models.m(node).vrot)), &plane);
            let saved = ik.force(models, node).saved_rot;
            let saved = v(fmov(saved.x), fmov(saved.y), fmov(saved.z));
            models.m_mut(node).vrot = extract(&m, saved, g);
        } else {
            let mut q = node;
            for i in 0..n {
                let s = ik.force(models, q).saved_rot;
                models.m_mut(q).vrot = v(fmov(s.x), fmov(s.y), fmov(s.z));
                if i + 1 < n {
                    match son(models, q) {
                        Some(s) => q = s,
                        None => break,
                    }
                }
            }
        }
    }
    joint_angle_in_range(models, node);
    let f = models.father(node);
    update_son_state(models, f, node, true, &g.model, host);
}

/// Whether a limb, as it is, points at the goal [soft 0x084b1578 checkBranchOrient].
/// Class: libm.
///
/// The goal's direction from the first joint, in the frame of that joint's father, is
/// taken as azimuth (`fpatan` of y and x) and elevation (after a turn by the azimuth about
/// Z, `fpatan` of −z and x); the limb's end, with the chain updated in its working frame,
/// likewise. True when both angles agree within 1e-5. The routine ends with the full node
/// update from the first joint, so it leaves the limb's nodes in global terms.
pub fn check_branch_orient(models: &mut Models, ik: &mut IkStore, node: NodeId, goal: Vector, n: i32, g: &Globals, host: &mut dyn Host) -> bool {
    let p = pos(models, node, g);
    let d = v((x(goal.x) - p.x).to_f32(), (x(goal.y) - p.y).to_f32(), (x(goal.z) - p.z).to_f32());
    let r = father_grot(models, node);
    let t1 = (x(r[0][1]) * d.y + x(r[0][0]) * d.x) + x(r[0][2]) * d.z;
    let t2 = (x(r[1][0]) * d.x + x(r[1][1]) * d.y) + x(r[1][2]) * d.z;
    let t3 = x(d.z) * r[2][2] + (x(d.y) * r[2][1] + x(d.x) * r[2][0]);
    let local = v(t1.to_f32(), t2.to_f32(), t3.to_f32());
    let az1 = trig::fpatan(t2, t1).to_f32();
    let (_, tx, tz) = through_z(&matrix_z_rotation(az1), local, false);
    let el1 = trig::fpatan(-tz, tx).to_f32();
    chain_update(models, ik, node, 0, g, host);
    let e = models.m(down(models, node, n)).gtrs;
    let e = v(fmov(e.x), fmov(e.y), fmov(e.z));
    let f = models.father(node);
    update_son_state(models, f, node, true, &g.model, host);
    let az2 = trig::fpatan(x(e.y), x(e.x)).to_f32();
    let (_, tx, tz) = through_z(&matrix_z_rotation(az2), e, false);
    let el2 = trig::fpatan(-tz, tx).to_f32();
    x(EPS5).gt((x(az1) - az2).abs()) && (x(el1) - el2).abs().lt(x(EPS5))
}

/// Whether some part of a limb is stretched straight at a goal it cannot reach
/// [soft 0x084b1810 si_checkBranchPullout]. Class: libm.
///
/// For the limb and each shorter limb that starts one joint further down (all ending at
/// the same node): if the distance from its first joint to the end is within 1e-5 of its
/// length, the goal is projected for it and its orientation tested
/// ([`check_branch_orient`], which is called whatever the projection said); true at the
/// first one whose goal is out of reach and which points at it.
pub fn check_branch_pullout(models: &mut Models, ik: &mut IkStore, node: NodeId, goal: Vector, n: i32, g: &Globals, host: &mut dyn Host) -> bool {
    let end = models.m(down(models, node, n)).gtrs;
    let end = v(fmov(end.x), fmov(end.y), fmov(end.z));
    let mut p = node;
    let mut left = n;
    for i in 0..n {
        let span = dist(models.m(p).gtrs, end).to_f32();
        let (len, _, _) = limb_length(models, p, left, g);
        if (len - span).abs().lt(x(EPS5)) {
            let (code, _) = volume_projection(models, p, left, goal, g);
            let aimed = check_branch_orient(models, ik, p, goal, left, g, host);
            if code == VOL_TOO_FAR && aimed {
                return true;
            }
        }
        if i + 1 < n {
            match son(models, p) {
                Some(s) => p = s,
                None => break,
            }
        }
        left -= 1;
    }
    false
}

/// Solves a limb for a goal [soft 0x084b1940 SI_solvePartialInvKin]. Class: libm.
///
/// `root` must be a chain root (a null with algorithm 1 or 2) with a record, `first` a
/// joint with algorithm 1 that is not a null; else nothing happens. By the root's chain
/// type and `n`: one joint [`invkin0`]; a spatial chain (type 40) with more [`invkin3d`];
/// any other chain with two [`invkin1`], with more [`invkin2d`]. For the two iterative
/// solvers the root's joint count is `n` for the time of the solve.
///
/// Then, unless the iteration gave up (`*stop`): the limb's joints are clamped to their
/// rotation limits ([`check_rotation_limit`]) and the chain is updated in full from the
/// father of the node that routine stopped at. If a joint was clamped, joints remain below
/// it and that rest is not stretched at a goal out of its reach
/// ([`check_branch_pullout`]), **the rest is solved for the same goal as a limb of its
/// own**, by this routine again.
#[allow(clippy::too_many_arguments)]
pub fn solve_partial_inv_kin(models: &mut Models, ik: &mut IkStore, root: Option<NodeId>, first: Option<NodeId>, n: i32, goal: Vector, first_pass: i16, stop: &mut i16, g: &Globals, host: &mut dyn Host) {
    let (Some(root), Some(first)) = (root, first) else { return };
    let rm = models.m(root);
    if rm.kind != 0 || !(rm.algo_type == 1 || rm.algo_type == 2) {
        return;
    }
    let fm = models.m(first);
    if fm.algo_type != 1 || fm.kind == 0 {
        return;
    }
    let spatial = ik.force(models, root).kind == CHAIN_CPX_3D;
    if n == 1 {
        invkin0(models, ik, first, goal, g, host);
    } else if spatial {
        if n > 1 {
            let saved = ik.force(models, root).nb_joints;
            ik.force(models, root).nb_joints = n;
            invkin3d(models, ik, root, first, n, goal, first_pass, stop, g, host);
            ik.force(models, root).nb_joints = saved;
        }
    } else if n == 2 {
        invkin1(models, ik, first, goal, g, host);
    } else if n > 2 {
        let saved = ik.force(models, root).nb_joints;
        ik.force(models, root).nb_joints = n;
        invkin2d(models, ik, root, first, n, goal, first_pass, g, host);
        ik.force(models, root).nb_joints = saved;
    }
    if *stop != 0 {
        return;
    }
    let (mut cur, mut left) = (first, n);
    let clamped = check_rotation_limit(models, &mut cur, &mut left, host);
    if let Some(f) = father(models, cur) {
        let ff = models.father(f);
        update_son_state(models, ff, f, true, &g.model, host);
    }
    if clamped != 0 && left > 0 && !check_branch_pullout(models, ik, cur, goal, left, g, host) {
        solve_partial_inv_kin(models, ik, Some(root), Some(cur), left, goal, first_pass, stop, g, host);
    }
}

/// The null pointer of a matrix field, for callers that build records by hand.
pub const NO_MATRIX: Ptr32<GenMatrixDbl> = Ptr32::NULL;
