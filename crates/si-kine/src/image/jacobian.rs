// SPDX-License-Identifier: MIT
//! `jacobian.c` on the image: the solver (finding 0022, "The resolution plane", "One
//! joint, two joints", "The iteration"). The arithmetic and its order are those of
//! [`crate::jacobian`], whose comments have the longer descriptions; what changes is that
//! the nodes, their records and the root's two matrices are the original's records in the
//! image, the matrices' sizes are written into their headers for the time of a solve as
//! the original does, and the node update goes out through [`Outside`].

use core::cmp::Ordering;

use si_math::algebra::{
    matrix_align_axes, matrix_axis_rotation, matrix_copy, matrix_extract_xyz_rotation, matrix_multiply, matrix_multiply_sub3, matrix_transpose,
    matrix_x_rotation, matrix_xyz_rotation, matrix_y_rotation, matrix_z_rotation,
};
use si_math::types::{Matrix, Vector as MVector, IDENTITY};
use si_mem::{Mem, Ptr32};
use si_types::geom::Vector;
use si_x87::trig;
use si_x87::{fmov, x, F80};

use super::greville::greville_inverse_dbl;
use super::rotlim::check_rotation_limit;
use super::tools::{chain_update, dist, limb_length, limb_position, pos, volume_projection, VOL_AT_ROOT, VOL_TOO_FAR};
use super::{down, father, ik, mat_at, mv, son, tv, update_son_state, v, GenMat, Manip, Node, ZERO};
use crate::jacobian::rounds;
use crate::outside::Outside;

pub use crate::jacobian::{get_insert_point, point_side_2d, points_angle_signed_2d, rotation_find_closer};

/// The double 0.0001 of these routines (one copy per routine in the binary).
const EPS: f64 = 1e-4;
/// The double 1e-5.
const EPS5: f64 = 1e-5;
const TWO_PI: f64 = core::f64::consts::TAU;
const PI: f64 = core::f64::consts::PI;
/// The double 0.5 a step that overshoots is halved by.
const HALF: f64 = 0.5;
/// The double 0.0008: the tolerance as a fraction of the limb's length.
const TOLERANCE: f64 = 0.0008;
/// The double 0.05 and its float forms: the smallest starting bend.
const MIN_BEND: f64 = 0.05;
const MIN_BEND_F32: f32 = 0.050_000_000_745_058_06;
/// How deep a step may be halved (the test is `depth > 20`).
const MAX_DEPTH: i32 = 20;

/// `grot` of the node's father: the original reads it without a test (the identity, and
/// a recorded fault, where there is none).
fn father_grot(m: &Mem, node: Node) -> Matrix {
    father(m, node).map_or(IDENTITY, |f| m[f].grot)
}

fn extract(mat: &Matrix, previous: Vector, g: &Manip) -> Vector {
    tv(matrix_extract_xyz_rotation(mat, &mv(previous), g.keep_turns))
}

/// [soft 0x084aeb58 jointAngleInRange]: each angle of each node from `node` down to the
/// first null of algorithm 3 or 4 (not included) becomes r32(r53(fmod(angle, 2π))).
pub fn joint_angle_in_range(m: &mut Mem, node: Node) {
    let mut p = node;
    loop {
        if super::is_effector(m, p) {
            return;
        }
        let wrap = |a: f32| x(x(a).fmod(x(TWO_PI)).to_f64()).to_f32();
        let r = m[p].vrot;
        m[p].vrot = v(wrap(r.x), wrap(r.y), wrap(r.z));
        match son(m, p) {
            Some(s) => p = s,
            None => return,
        }
    }
}

/// [soft 0x084aefe4 mapGoalToRoot]: a global point in the frame a limb is solved in (minus
/// the first joint's position, each difference a float, then through the transpose of the
/// father's `grot`).
pub fn map_goal_to_root(m: &Mem, node: Node, goal: Vector) -> Vector {
    let g = &Manip::read(m);
    let p = pos(m, node, g);
    let o = v((x(goal.x) - p.x).to_f32(), (x(goal.y) - p.y).to_f32(), (x(goal.z) - p.z).to_f32());
    let r = father_grot(m, node);
    v(
        ((x(r[0][1]) * o.y + x(r[0][0]) * o.x) + x(r[0][2]) * o.z).to_f32(),
        ((x(r[1][0]) * o.x + x(r[1][1]) * o.y) + x(r[1][2]) * o.z).to_f32(),
        ((x(o.y) * r[2][1] + x(o.x) * r[2][0]) + x(o.z) * r[2][2]).to_f32(),
    )
}

fn through(p: Vector, mat: &Matrix) -> Vector {
    let col = |j: usize| (((x(p.y) * mat[1][j] + x(p.x) * mat[0][j]) + x(p.z) * mat[2][j]) + mat[3][j]).to_f32();
    v(col(0), col(1), col(2))
}

fn through_roll(p: Vector, mat: &Matrix) -> Vector {
    let col = |j: usize| (((x(p.y) * mat[1][j] + x(p.x) * mat[0][j]) + x(p.z) * mat[2][j]) + mat[3][j]).to_f32();
    let z = ((x(p.z) * mat[2][2] + (x(p.y) * mat[1][2] + x(p.x) * mat[0][2])) + mat[3][2]).to_f32();
    v(col(0), col(1), z)
}

fn unit(d: [F80; 3], len: f32) -> Option<Vector> {
    match x(len).compare(x(EPS)) {
        Some(Ordering::Less) | None => None,
        _ => {
            let inv = F80::ONE / len;
            Some(v((d[0] * inv).to_f32(), (d[1] * inv).to_f32(), (d[2] * inv).to_f32()))
        }
    }
}

/// [soft 0x084af0cc findResolutionPlane]: the orientation the first joint `node` has when
/// the limb lies stretched towards the goal; `goal` (global on entry) becomes the goal in
/// the plane's frame. The joint's record gets `plane_up` and `plane_roll`
/// ([`crate::jacobian::find_resolution_plane`]). The record of `node` (and, under the old
/// roll with a chain type of 20, of its son) is read without a test.
pub fn find_resolution_plane(m: &mut Mem, node: Node, chain_type: i32, with_roll: i16, goal: &mut Vector) -> Matrix {
    let g = &Manip::read(m);
    let goal_in = *goal;
    let mut rolled = false;
    let mut rot: Matrix = [[0.0; 4]; 4];
    let mut axis = ZERO;
    let mut gl = map_goal_to_root(m, node, goal_in);
    let rec = ik(m, node);
    let mut roll = m[rec].prot.x;
    let (cns_type, cns_pos) = (m[rec].cns_type, m[rec].cns_pos);
    if g.old_roll && with_roll != 0 && x(roll).abs().gt(x(EPS)) {
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
    if cns_type == 2 {
        up = map_goal_to_root(m, node, cns_pos);
        second = up;
    } else {
        up = ZERO;
        if cns_type == 1 {
            up = map_goal_to_root(m, node, cns_pos);
        }
        let small = |c: f32| x(EPS).gt((x(c) - 0.0f32).abs());
        if small(up.x) && small(up.y) && small(up.z) {
            up = v(0.0, 0.0, 1.0);
        }
        second = v((x(up.y) * gl.z - x(up.z) * gl.y).to_f32(), (x(up.z) * gl.x - x(gl.z) * up.x).to_f32(), (x(gl.y) * up.x - x(gl.x) * up.y).to_f32());
    }
    let (mut plane, _) = matrix_align_axes(&mv(gl), &mv(second));
    if !g.old_roll && with_roll != 0 && x(EPS).lt(x(roll).abs()) {
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
    if g.old_roll && rolled {
        up = through_roll(up, &rot);
    }
    let fg = father_grot(m, node);
    let up_world = through(up, &fg);
    let mut up_rolled = up_world;
    if g.old_roll {
        if chain_type == 20 {
            if let Some(s) = son(m, node) {
                roll = m[ik(m, s)].prot.x;
                if x(EPS).lt(x(roll).abs()) {
                    let p = m[node].gtrs;
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
    let rec = ik(m, node);
    m[rec].plane_roll = up_rolled;
    m[rec].plane_up = up_world;
    *goal = in_plane;
    plane
}

/// [soft 0x084af8e4 chain2DRollApply]: the joints' rolls applied to a solved planar limb,
/// from the first joint below `node` whose roll is above 0.0001 in size
/// ([`crate::jacobian::chain_2d_roll_apply`]).
pub fn chain_2d_roll_apply(m: &mut Mem, node: Node, n: i32) {
    let g = &Manip::read(m);
    let mut sum = 0.0f32;
    let mut a: Matrix = matrix_copy(&IDENTITY);
    let mut i = 1;
    let Some(mut p) = son(m, node) else { return };
    loop {
        if i >= n {
            return;
        }
        let roll = m[ik(m, p)].prot.x;
        match x(roll).abs().compare(x(EPS)) {
            Some(Ordering::Less | Ordering::Equal) => {}
            _ => break,
        }
        let Some(s) = son(m, p) else { return };
        p = s;
        i += 1;
    }
    for left in (1..=(n - i)).rev() {
        let mut b = matrix_transpose(&a);
        let z = matrix_z_rotation(m[p].vrot.z);
        b = matrix_multiply(&z, &b);
        let roll = m[ik(m, p)].prot.x;
        if x(roll).abs().gt(x(EPS)) {
            sum = (x(roll) + sum).to_f32();
            a = matrix_x_rotation(sum);
        }
        b = matrix_multiply(&a, &b);
        let prev = m[p].vrot;
        m[p].vrot = extract(&b, prev, g);
        if left > 1 {
            let Some(s) = son(m, p) else { return };
            p = s;
        }
    }
}

fn bends_up(preferred: f32) -> bool {
    let mut r = x(x(preferred).fmod(x(TWO_PI)).to_f64()).to_f32();
    if x(r).gt(x(PI)) {
        r = (x(r) - TWO_PI).to_f32();
    } else if x(r).lt(x(-PI)) {
        r = (x(TWO_PI) + r).to_f32();
    }
    F80::ZERO.lt(x(r))
}

/// `vrot.z` = r32(rotationFindCloser(the joint's preferred z, `vrot.z`)).
fn finish_first_joint(m: &mut Mem, node: Node) {
    let pref = m[ik(m, node)].prot.z;
    let z = m[node].vrot.z;
    m[node].vrot.z = rotation_find_closer(pref, z).to_f32();
}

/// [soft 0x084afa1c invkin0]: a limb of one joint, the bone pointed at the goal
/// ([`crate::jacobian::invkin0`]).
pub fn invkin0(m: &mut Mem, o: &mut dyn Outside, node: Node, goal: Vector) {
    let g = &Manip::read(m);
    let Some(s) = son(m, node) else { return };
    if dist(m[node].gtrs, m[s].gtrs).lt(x(EPS)) {
        return;
    }
    let (code, mut p) = volume_projection(m, node, 1, goal);
    if code == VOL_AT_ROOT {
        return;
    }
    let plane = find_resolution_plane(m, node, 10, 1, &mut p);
    let prev = m[node].vrot;
    m[node].vrot = extract(&plane, prev, g);
    finish_first_joint(m, node);
    joint_angle_in_range(m, node);
    let f = m[node].father;
    update_son_state(m, o, f, node, 1);
}

/// [soft 0x084afb1c invkin1]: a limb of two joints of a planar chain, in closed form
/// ([`crate::jacobian::invkin1`]).
pub fn invkin1(m: &mut Mem, o: &mut dyn Outside, node: Node, goal: Vector) {
    let g = &Manip::read(m);
    let Some(s) = son(m, node) else { return };
    let Some(e) = son(m, s) else { return };
    let l1 = dist(pos(m, node, g), pos(m, s, g)).to_f32();
    let (ps, pe) = (pos(m, s, g), pos(m, e, g));
    let (dx, dy, dz) = (x(ps.x) - pe.x, x(ps.y) - pe.y, x(ps.z) - pe.z);
    let l2 = ((dx * dx + dy * dy) + dz * dz).sqrt().to_f32();
    if x(EPS).gt(x(l1)) || x(EPS).gt(x(l2)) {
        return;
    }
    let (code, mut p) = volume_projection(m, node, 2, goal);
    if code == VOL_AT_ROOT {
        return;
    }
    let plane = find_resolution_plane(m, node, 20, 1, &mut p);
    let sprot = m[ik(m, s)].prot;
    let mut old_roll = None;
    if g.old_roll && x(sprot.x).abs().gt(x(EPS)) {
        old_roll = Some(matrix_x_rotation(sprot.x));
    }
    let up = bends_up(sprot.z);
    let (a1, a2) = crate::iktools::solve_planar_limb(l1, l2, p.x, p.y, up as i16);
    match old_roll {
        Some(rx) => {
            let axis = MVector::new(fmov(rx[2][0]), fmov(rx[2][1]), fmov(rx[2][2]));
            let mat = matrix_multiply(&matrix_axis_rotation(a1, &axis), &plane);
            let prev = m[node].vrot;
            m[node].vrot = extract(&mat, prev, g);
            let mat = matrix_multiply(&matrix_z_rotation(a2), &rx);
            let prev = m[s].vrot;
            m[s].vrot = extract(&mat, prev, g);
        }
        None => {
            let mat = matrix_multiply(&matrix_z_rotation(a1), &plane);
            let prev = m[node].vrot;
            m[node].vrot = extract(&mat, prev, g);
            m[s].vrot = v(0.0, 0.0, fmov(a2));
        }
    }
    if !g.old_roll {
        chain_2d_roll_apply(m, node, 2);
    }
    finish_first_joint(m, node);
    finish_first_joint(m, s);
    joint_angle_in_range(m, node);
    let f = m[node].father;
    update_son_state(m, o, f, node, 1);
}

/// [soft 0x084aebd8 computeJacobian2d]: column i of the planar Jacobian `j`,
/// (−(e.y − p.y), e.x − p.x); row 0 from the register to double, row 1 through float.
pub fn compute_jacobian_2d(m: &mut Mem, node: Node, j: Ptr32<GenMat>, n: i32) {
    let e = limb_position(m, node, n);
    let mut p = node;
    for i in 0..n.max(1) as u32 {
        let at = m[p].gtrs;
        let dx = (x(e.x) - at.x).to_f32();
        let dy = x(e.y) - at.y;
        let a0 = mat_at(m, j, 0, i);
        m.set_f64(a0, (-dy).to_f64());
        let a1 = mat_at(m, j, 1, i);
        m.set_f64(a1, x(dx).to_f64());
        if (i as i32) + 1 < n.max(1) {
            match son(m, p) {
                Some(s) => p = s,
                None => return,
            }
        }
    }
}

/// [soft 0x084aecc0 computeJoints2d]: one step of a planar limb towards `target` on the
/// matrices `j` and `inv`, halved while it lands further than `tolerance` off; nothing at
/// depth 21 ([`crate::jacobian`]'s `compute_joints_2d`).
#[allow(clippy::too_many_arguments)]
pub fn compute_joints_2d(m: &mut Mem, o: &mut dyn Outside, depth: i32, node: Node, n: i32, j: Ptr32<GenMat>, inv: Ptr32<GenMat>, recompute: bool, tolerance: f32, target: Vector) {
    rounds::note(0);
    if depth > MAX_DEPTH {
        return;
    }
    let e0 = limb_position(m, node, n);
    let mut e = v((x(target.x) - e0.x).to_f32(), (x(target.y) - e0.y).to_f32(), (x(target.z) - e0.z).to_f32());
    if recompute {
        compute_jacobian_2d(m, node, j, n);
        rounds::note(2);
        greville_inverse_dbl(m, j, inv);
    }
    let mut p = node;
    for i in 0..n.max(0) as u32 {
        let (q0, q1) = (m.f64(mat_at(m, inv, i, 0)), m.f64(mat_at(m, inv, i, 1)));
        let d = (x(e.x) * q0 + x(e.y) * q1).to_f32();
        let r = ik(m, p);
        m[r].drot.z = d;
        let z = m[p].vrot.z;
        m[p].vrot.z = (x(z) + d).to_f32();
        if (i as i32) + 1 < n {
            match son(m, p) {
                Some(s) => p = s,
                None => break,
            }
        }
    }
    chain_update(m, o, node, 0);
    let e1 = limb_position(m, node, n);
    let off = dist(target, e1).to_f32();
    if !x(off).gt(x(tolerance)) {
        return;
    }
    e = v((x(HALF) * e.x).to_f32(), (x(HALF) * e.y).to_f32(), (x(HALF) * e.z).to_f32());
    let mid = v((x(e.x) + e0.x).to_f32(), (x(e.y) + e0.y).to_f32(), (x(e.z) + e0.z).to_f32());
    let (_, near) = volume_projection(m, node, n, mid);
    let mut p = node;
    for i in 0..n.max(0) {
        let d = m[ik(m, p)].drot.z;
        let z = m[p].vrot.z;
        m[p].vrot.z = (x(z) - d).to_f32();
        if i + 1 < n {
            match son(m, p) {
                Some(s) => p = s,
                None => break,
            }
        }
    }
    chain_update(m, o, node, 0);
    let doubled = (x(tolerance) + tolerance).to_f32();
    compute_joints_2d(m, o, depth + 1, node, n, j, inv, false, doubled, near);
    compute_joints_2d(m, o, depth + 1, node, n, j, inv, true, tolerance, target);
}

/// The root's two matrices, as the walks read them from its record at each step.
fn matrices(m: &Mem, root: Node) -> (Ptr32<GenMat>, Ptr32<GenMat>) {
    let r = ik(m, root);
    (m[r].jacobian, m[r].inverse)
}

/// [soft 0x084b0070 iterComputeJoints2d]: a planar limb's end walked to the goal
/// ([`crate::jacobian`]'s `iter_compute_joints_2d`; the uninitialised local of the
/// original is taken as "not the goal", as there).
#[allow(clippy::too_many_arguments)]
pub fn iter_compute_joints_2d(m: &mut Mem, o: &mut dyn Outside, root: Node, node: Node, n: i32, reach: f32, fraction: f32, tolerance: f32, goal: Vector) {
    let mut cur = limb_position(m, node, n);
    let mut aim: Option<Vector> = None;
    loop {
        let (dx, dy, dz) = (x(goal.x) - cur.x, x(goal.y) - cur.y, x(goal.z) - cur.z);
        let d = (dz * dz + (dy * dy + dx * dx)).sqrt().to_f32();
        if !x(d).gt(x(reach)) {
            break;
        }
        let a = get_insert_point((x(d) * fraction).to_f32(), d, cur, goal);
        let (j, inv) = matrices(m, root);
        compute_joints_2d(m, o, 0, node, n, j, inv, true, tolerance, a);
        cur = v(fmov(a.x), fmov(a.y), fmov(a.z));
        aim = Some(a);
    }
    let at_goal = aim.is_some_and(|a| (x(goal.x) - a.x).eq(F80::ZERO) && (x(goal.y) - a.y).eq(F80::ZERO) && (x(goal.z) - a.z).eq(F80::ZERO));
    if !at_goal {
        let (j, inv) = matrices(m, root);
        compute_joints_2d(m, o, 0, node, n, j, inv, true, tolerance, goal);
    }
}

/// The start of both iterative solvers: each joint's angles into its record's
/// `saved_rot`, and the limb measured (end node, span, length, tolerance, shortest bone).
fn enter(m: &mut Mem, node: Node, n: i32, g: &Manip) -> (Node, f32, f32, f32, f32) {
    let mut p = node;
    for _ in 0..n.max(0) {
        let r = m[p].vrot;
        let rec = ik(m, p);
        m[rec].saved_rot = v(fmov(r.x), fmov(r.y), fmov(r.z));
        match son(m, p) {
            Some(s) => p = s,
            None => break,
        }
    }
    let (a, b) = (pos(m, node, g), pos(m, p, g));
    let (dx, dy, dz) = (x(a.x) - b.x, x(a.y) - b.y, x(a.z) - b.z);
    let span = ((dx * dx + dy * dy) + dz * dz).sqrt().to_f32();
    let (len, _, shortest) = limb_length(m, node, n);
    let len = len.to_f32();
    let tolerance = (x(TOLERANCE) * len).to_f32();
    (p, span, len, tolerance, shortest)
}

fn worth_solving(m: &Mem, node: Node, end: Node, goal: Vector, tolerance: f32, shortest: f32) -> bool {
    let off = dist(goal, m[end].gtrs).to_f32();
    let far = !x(off).lt(x(tolerance)) && !x(shortest).lt(x(EPS));
    far || m[ik(m, node)].cns_type != 0
}

fn min_bend(z: f32) -> f32 {
    if x(z).abs().lt(x(MIN_BEND)) {
        if x(z).gt(F80::ZERO) { MIN_BEND_F32 } else { -MIN_BEND_F32 }
    } else {
        z
    }
}

/// [soft 0x084b0254 invkin2d]: a limb of three or more joints of a planar chain, the
/// iteration in the resolution plane ([`crate::jacobian::invkin2d`]).
#[allow(clippy::too_many_arguments)]
pub fn invkin2d(m: &mut Mem, o: &mut dyn Outside, root: Node, node: Node, n: i32, goal: Vector, first_pass: i16) {
    let g = &Manip::read(m);
    let (end, span, len, tolerance, shortest) = enter(m, node, n, g);
    if !worth_solving(m, node, end, goal, tolerance, shortest) {
        return;
    }
    let (code, mut p) = volume_projection(m, node, n, goal);
    if code == VOL_AT_ROOT {
        return;
    }
    let plane = find_resolution_plane(m, node, 30, 1, &mut p);
    if code == VOL_TOO_FAR {
        let mut q = node;
        for i in 0..n {
            m[q].vrot = ZERO;
            if i + 1 < n {
                match son(m, q) {
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
        let preferred = first_pass != 0 && m[ik(m, node)].use_pref != 0;
        m[node].vrot = ZERO;
        let mut q = node;
        for _ in 1..n {
            let Some(s) = son(m, q) else { break };
            q = s;
            let z = if preferred { fmov(m[ik(m, q)].prot.z) } else { min_bend(m[q].vrot.z) };
            m[q].vrot = v(0.0, 0.0, z);
        }
        chain_update(m, o, node, 0);
        let e = m[down(m, node, n)].gtrs;
        let second = son(m, node).map_or(ZERO, |s| m[s].gtrs);
        let first = m[node].gtrs;
        m[node].vrot.z = points_angle_signed_2d(&second, &first, &e).to_f32();
        chain_update(m, o, node, 0);
        iter_compute_joints_2d(m, o, root, node, n, reach, fraction, tolerance, p);
    }
    let mat = matrix_multiply(&matrix_z_rotation(m[node].vrot.z), &plane);
    let saved = m[ik(m, node)].saved_rot;
    let saved = v(fmov(saved.x), fmov(saved.y), fmov(saved.z));
    m[node].vrot = extract(&mat, saved, g);
    if !g.old_roll {
        chain_2d_roll_apply(m, node, n);
    }
    finish_first_joint(m, node);
    joint_angle_in_range(m, node);
    let f = m[node].father;
    update_son_state(m, o, f, node, 1);
}

/// [soft 0x084b07f4 computeJacobian3d]: three columns per joint, axis × (e − p) for each
/// row of the joint's `grot`; row 0 from the register to double, rows 1 and 2 through
/// float.
pub fn compute_jacobian_3d(m: &mut Mem, node: Node, j: Ptr32<GenMat>, n: i32) {
    let e = limb_position(m, node, n);
    let mut p = node;
    let mut col = 0u32;
    for i in 0..n.max(0) {
        let (gtrs, grot) = (m[p].gtrs, m[p].grot);
        let dx = (x(e.x) - gtrs.x).to_f32();
        let dy = (x(e.y) - gtrs.y).to_f32();
        let dz = (x(e.z) - gtrs.z).to_f32();
        for k in 0..3 {
            let (a0, a1, a2) = (grot[k][0], grot[k][1], grot[k][2]);
            let r0 = (x(a1) * dz - x(a2) * dy).to_f64();
            let r1 = x((x(a2) * dx - x(dz) * a0).to_f32()).to_f64();
            let r2 = x((x(dy) * a0 - x(dx) * a1).to_f32()).to_f64();
            let at = mat_at(m, j, 0, col);
            m.set_f64(at, r0);
            let at = mat_at(m, j, 1, col);
            m.set_f64(at, r1);
            let at = mat_at(m, j, 2, col);
            m.set_f64(at, r2);
            col += 1;
        }
        if i + 1 < n {
            match son(m, p) {
                Some(s) => p = s,
                None => return,
            }
        }
    }
}

/// [soft 0x084b0920 computeJoints3d]: one step of a spatial limb towards `target`; at depth
/// 21 `*stop` is set, and a set `*stop` keeps the second half of a split from running
/// ([`crate::jacobian`]'s `compute_joints_3d`).
#[allow(clippy::too_many_arguments)]
pub fn compute_joints_3d(m: &mut Mem, o: &mut dyn Outside, depth: i32, node: Node, n: i32, j: Ptr32<GenMat>, inv: Ptr32<GenMat>, recompute: bool, tolerance: f32, target: Vector, stop: &mut i16) {
    let g = &Manip::read(m);
    rounds::note(1);
    if depth > MAX_DEPTH {
        *stop = 1;
        return;
    }
    let e0 = limb_position(m, node, n);
    let mut e = v((x(target.x) - e0.x).to_f32(), (x(target.y) - e0.y).to_f32(), (x(target.z) - e0.z).to_f32());
    if recompute {
        compute_jacobian_3d(m, node, j, n);
        rounds::note(2);
        greville_inverse_dbl(m, j, inv);
    }
    let mut p = node;
    for i in 0..n.max(0) as u32 {
        let row = |m: &Mem, r: u32| {
            let q = |c: u32| m.f64(mat_at(m, inv, r, c));
            ((x(q(0)) * e.x + x(q(1)) * e.y) + x(q(2)) * e.z).to_f32()
        };
        let delta = MVector::new(row(m, 3 * i), row(m, 3 * i + 1), row(m, 3 * i + 2));
        let turned = matrix_multiply_sub3(&matrix_xyz_rotation(&delta), &m[p].mrot);
        let old = m[p].vrot;
        let new = extract(&turned, old, g);
        let r = ik(m, p);
        m[r].drot = v((x(new.x) - old.x).to_f32(), (x(new.y) - old.y).to_f32(), (x(new.z) - old.z).to_f32());
        m[p].vrot = new;
        if (i as i32) + 1 < n {
            match son(m, p) {
                Some(s) => p = s,
                None => break,
            }
        }
    }
    chain_update(m, o, node, 0);
    let e1 = limb_position(m, node, n);
    let off = dist(target, e1).to_f32();
    if !x(off).gt(x(tolerance)) {
        return;
    }
    e = v((x(HALF) * e.x).to_f32(), (x(HALF) * e.y).to_f32(), (x(HALF) * e.z).to_f32());
    let mid = v((x(e.x) + e0.x).to_f32(), (x(e.y) + e0.y).to_f32(), (x(e.z) + e0.z).to_f32());
    let (_, near) = volume_projection(m, node, n, mid);
    let mut p = node;
    for i in 0..n.max(0) {
        let d = m[ik(m, p)].drot;
        let r = m[p].vrot;
        m[p].vrot = v((x(r.x) - d.x).to_f32(), (x(r.y) - d.y).to_f32(), (x(r.z) - d.z).to_f32());
        if i + 1 < n {
            match son(m, p) {
                Some(s) => p = s,
                None => break,
            }
        }
    }
    chain_update(m, o, node, 0);
    compute_joints_3d(m, o, depth + 1, node, n, j, inv, false, tolerance, near, stop);
    if *stop == 0 {
        compute_joints_3d(m, o, depth + 1, node, n, j, inv, true, tolerance, target, stop);
    }
}

/// [soft 0x084b0d20 iterComputeJoints3d]: a spatial limb's end walked to the goal; the
/// loop also ends on `*stop` and after 22 unchanged distances
/// ([`crate::jacobian`]'s `iter_compute_joints_3d`).
#[allow(clippy::too_many_arguments)]
pub fn iter_compute_joints_3d(m: &mut Mem, o: &mut dyn Outside, root: Node, node: Node, n: i32, reach: f32, fraction: f32, tolerance: f32, goal: Vector, stop: &mut i16) {
    let mut cur = limb_position(m, node, n);
    let mut aim: Option<Vector> = None;
    let mut same = 0;
    let measure = |cur: Vector| {
        let (dx, dy, dz) = (x(goal.x) - cur.x, x(goal.y) - cur.y, x(goal.z) - cur.z);
        (dz * dz + (dy * dy + dx * dx)).sqrt().to_f32()
    };
    let mut d = measure(cur);
    while x(d).gt(x(reach)) && *stop == 0 {
        let a = get_insert_point((x(d) * fraction).to_f32(), d, cur, goal);
        let (j, inv) = matrices(m, root);
        compute_joints_3d(m, o, 0, node, n, j, inv, true, tolerance, a, stop);
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
        let (j, inv) = matrices(m, root);
        compute_joints_3d(m, o, 0, node, n, j, inv, true, tolerance, goal, stop);
    }
}

fn through_z(z: &Matrix, p: Vector, x_first: bool) -> (Vector, F80, F80) {
    let px = if x_first { (x(z[0][1]) * p.y + x(z[0][0]) * p.x) + x(z[0][2]) * p.z } else { x(z[0][2]) * p.z + (x(z[0][1]) * p.y + x(z[0][0]) * p.x) };
    let py = if x_first { (x(z[1][0]) * p.x + x(z[1][1]) * p.y) + x(z[1][2]) * p.z } else { x(z[1][2]) * p.z + (x(z[1][1]) * p.y + x(z[1][0]) * p.x) };
    let pz = x(p.z) * z[2][2] + (x(p.y) * z[2][1] + x(p.x) * z[2][0]);
    (v(px.to_f32(), py.to_f32(), pz.to_f32()), px, pz)
}

/// [soft 0x084b0f48 invkin3d]: a limb of two or more joints of a spatial chain
/// ([`crate::jacobian::invkin3d`]).
#[allow(clippy::too_many_arguments)]
pub fn invkin3d(m: &mut Mem, o: &mut dyn Outside, root: Node, node: Node, n: i32, goal: Vector, first_pass: i16, stop: &mut i16) {
    let g = &Manip::read(m);
    let (end, span, len, tolerance, shortest) = enter(m, node, n, g);
    if !worth_solving(m, node, end, goal, tolerance, shortest) {
        return;
    }
    let (code, mut p) = volume_projection(m, node, n, goal);
    if code == VOL_AT_ROOT {
        return;
    }
    if code == VOL_TOO_FAR {
        let plane = find_resolution_plane(m, node, 40, 1, &mut p);
        let prev = m[node].vrot;
        m[node].vrot = extract(&plane, prev, g);
        let mut q = node;
        for _ in 1..n {
            let Some(s) = son(m, q) else { break };
            q = s;
            let roll = fmov(m[ik(m, q)].prot.x);
            m[q].vrot = v(roll, 0.0, 0.0);
        }
    } else {
        let plane = find_resolution_plane(m, node, 40, 0, &mut p);
        let (mut reach, mut fraction) = (0.001_000_000_047_497_451_3f32, 1.0f32);
        if (x(len) - span).abs().lt(x(EPS5)) {
            reach = 0.000_500_000_023_748_725_7;
            fraction = 0.617_999_970_912_933_3;
        }
        let preferred = first_pass != 0 && m[ik(m, node)].use_pref != 0;
        let mut q = node;
        for i in 0..n {
            if preferred {
                let pr = m[ik(m, q)].prot;
                m[q].vrot = v(fmov(pr.x), fmov(pr.y), fmov(pr.z));
            } else {
                let z = m[q].vrot.z;
                m[q].vrot.z = min_bend(z);
            }
            if i + 1 < n {
                match son(m, q) {
                    Some(s) => q = s,
                    None => break,
                }
            }
        }
        chain_update(m, o, node, 0);
        let e = m[end].gtrs;
        let az = if x(e.x).eq(F80::ZERO) && x(e.y).eq(F80::ZERO) { 0.0 } else { trig::fpatan(x(e.y), x(e.x)).to_f32() };
        let z = matrix_z_rotation(az);
        let (turned, ex, ez) = through_z(&z, e, true);
        m[end].gtrs = turned;
        let el = trig::fpatan(-ez, ex).to_f32();
        let y = matrix_y_rotation((-x(el)).to_f32());
        let back = matrix_z_rotation((-x(az)).to_f32());
        let align = matrix_multiply(&back, &y);
        let own = matrix_xyz_rotation(&mv(m[node].vrot));
        let mat = matrix_multiply(&own, &align);
        let prev = m[node].vrot;
        m[node].vrot = extract(&mat, prev, g);
        chain_update(m, o, node, 0);
        iter_compute_joints_3d(m, o, root, node, n, reach, fraction, tolerance, p, stop);
        if *stop == 0 {
            let mat = matrix_multiply(&matrix_xyz_rotation(&mv(m[node].vrot)), &plane);
            let saved = m[ik(m, node)].saved_rot;
            let saved = v(fmov(saved.x), fmov(saved.y), fmov(saved.z));
            m[node].vrot = extract(&mat, saved, g);
        } else {
            let mut q = node;
            for i in 0..n {
                let s = m[ik(m, q)].saved_rot;
                m[q].vrot = v(fmov(s.x), fmov(s.y), fmov(s.z));
                if i + 1 < n {
                    match son(m, q) {
                        Some(s) => q = s,
                        None => break,
                    }
                }
            }
        }
    }
    joint_angle_in_range(m, node);
    let f = m[node].father;
    update_son_state(m, o, f, node, 1);
}

/// [soft 0x084b1578 checkBranchOrient]: whether a limb, as it is, points at the goal
/// (azimuth and elevation within 1e-5); ends with the full node update from `node`
/// ([`crate::jacobian::check_branch_orient`]).
pub fn check_branch_orient(m: &mut Mem, o: &mut dyn Outside, node: Node, goal: Vector, n: i32) -> bool {
    let g = &Manip::read(m);
    let p = pos(m, node, g);
    let d = v((x(goal.x) - p.x).to_f32(), (x(goal.y) - p.y).to_f32(), (x(goal.z) - p.z).to_f32());
    let r = father_grot(m, node);
    let t1 = (x(r[0][1]) * d.y + x(r[0][0]) * d.x) + x(r[0][2]) * d.z;
    let t2 = (x(r[1][0]) * d.x + x(r[1][1]) * d.y) + x(r[1][2]) * d.z;
    let t3 = x(d.z) * r[2][2] + (x(d.y) * r[2][1] + x(d.x) * r[2][0]);
    let local = v(t1.to_f32(), t2.to_f32(), t3.to_f32());
    let az1 = trig::fpatan(t2, t1).to_f32();
    let (_, tx, tz) = through_z(&matrix_z_rotation(az1), local, false);
    let el1 = trig::fpatan(-tz, tx).to_f32();
    chain_update(m, o, node, 0);
    let e = m[down(m, node, n)].gtrs;
    let e = v(fmov(e.x), fmov(e.y), fmov(e.z));
    let f = m[node].father;
    update_son_state(m, o, f, node, 1);
    let az2 = trig::fpatan(x(e.y), x(e.x)).to_f32();
    let (_, tx, tz) = through_z(&matrix_z_rotation(az2), e, false);
    let el2 = trig::fpatan(-tz, tx).to_f32();
    x(EPS5).gt((x(az1) - az2).abs()) && (x(el1) - el2).abs().lt(x(EPS5))
}

/// [soft 0x084b1810 si_checkBranchPullout]: whether some part of a limb is stretched
/// straight at a goal it cannot reach ([`crate::jacobian::check_branch_pullout`]).
pub fn check_branch_pullout(m: &mut Mem, o: &mut dyn Outside, node: Node, goal: Vector, n: i32) -> bool {
    let end = m[down(m, node, n)].gtrs;
    let end = v(fmov(end.x), fmov(end.y), fmov(end.z));
    let mut p = node;
    let mut left = n;
    for i in 0..n {
        let span = dist(m[p].gtrs, end).to_f32();
        let (len, _, _) = limb_length(m, p, left);
        if (len - span).abs().lt(x(EPS5)) {
            let (code, _) = volume_projection(m, p, left, goal);
            let aimed = check_branch_orient(m, o, p, goal, left);
            if code == VOL_TOO_FAR && aimed {
                return true;
            }
        }
        if i + 1 < n {
            match son(m, p) {
                Some(s) => p = s,
                None => break,
            }
        }
        left -= 1;
    }
    false
}

/// [soft 0x084b1940 SI_solvePartialInvKin]: a limb solved for a goal by the method its
/// chain type and joint count choose; for the iterative two the root's joint count and the
/// matrices' sizes (the Jacobian's columns, the inverse's rows) are `n` (spatial: 3n) for
/// the time of the solve and the record's count (× 3) afterwards. Then, unless `*stop`,
/// the rotation limits are applied and what lies below a clamped joint is solved again
/// ([`crate::jacobian::solve_partial_inv_kin`]).
#[allow(clippy::too_many_arguments)]
pub fn solve_partial_inv_kin(m: &mut Mem, o: &mut dyn Outside, root: Node, first: Node, n: i32, goal: Vector, first_pass: i16, stop: &mut i16) {
    if root.is_null() || first.is_null() {
        return;
    }
    if m[root].kind != 0 || (m[root].algo_type.wrapping_sub(1) as u32) > 1 {
        return;
    }
    if m[first].algo_type != 1 || m[first].kind == 0 {
        return;
    }
    let rec = ik(m, root);
    // the root's count, the Jacobian's columns and the inverse's rows, each read again
    // through the root's record as the original does
    let resize = |m: &mut Mem, count: i32, k: i32| {
        let rec = ik(m, root);
        m[rec].nb_joints = count;
        let j = m[ik(m, root)].jacobian;
        m[j].cols = count.wrapping_mul(k);
        let inv = m[ik(m, root)].inverse;
        m[inv].rows = count.wrapping_mul(k);
    };
    if m[rec].kind == 0x28 {
        if n != 1 {
            if n > 1 {
                let saved = m[rec].nb_joints;
                resize(m, n, 3);
                invkin3d(m, o, root, first, n, goal, first_pass, stop);
                resize(m, saved, 3);
            }
        } else {
            invkin0(m, o, first, goal);
        }
    } else if n != 1 {
        if n == 2 {
            invkin1(m, o, first, goal);
        } else if n > 2 {
            let saved = m[rec].nb_joints;
            resize(m, n, 1);
            invkin2d(m, o, root, first, n, goal, first_pass);
            resize(m, saved, 1);
        }
    } else {
        invkin0(m, o, first, goal);
    }
    if *stop != 0 {
        return;
    }
    let (mut cur, mut left) = (first, n);
    let clamped = check_rotation_limit(m, o, &mut cur, &mut left);
    // the original updates the father of the node it stopped at from that father's father,
    // reading both without a test
    let Some(f) = father(m, cur) else { return };
    let ff = m[f].father;
    update_son_state(m, o, ff, f, 1);
    if clamped != 0 && left > 0 && !check_branch_pullout(m, o, cur, goal, left) {
        solve_partial_inv_kin(m, o, root, cur, left, goal, first_pass, stop);
    }
}
