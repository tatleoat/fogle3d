// SPDX-License-Identifier: MIT
//! `iktools.c` on the image: limb lengths, the reachable volume, the chain's own update
//! (finding 0022, "Limbs and the reachable volume", "The working frame"). The arithmetic
//! is that of [`crate::iktools`], which has the longer descriptions.

use si_math::algebra::{matrix_copy, matrix_multiply_sub3, matrix_xyz_rotation};
use si_math::types::IDENTITY;
use si_mem::Mem;
use si_types::geom::Vector;
use si_x87::{fmov, x, F80};

use super::{ik, is_effector, mv, son, update_son_state, Manip, Node};
use crate::outside::Outside;

/// The double 0.0001 the volume tests compare with (0x08660ca8).
const EPS: f64 = 1e-4;
/// The float nearest 0.0001 (0x08660cb0).
const EPS_F32: f32 = 9.999_999_747_378_752e-5;
/// The double 0.001 (0x08660cb8).
const INNER_MARGIN: f64 = 0.001;

pub use crate::iktools::{VOL_AT_ROOT, VOL_INSIDE, VOL_TOO_FAR, VOL_TOO_NEAR};

/// A node's position as the solver measures it: `gtrs`, or +0x3ac under classical scaling
/// without uniform scaling.
pub(crate) fn pos(m: &Mem, node: Node, g: &Manip) -> Vector {
    if g.classic_frame() { m[node].unk_3ac } else { m[node].gtrs }
}

/// √((x² + y²) + z²) of three register values, unrounded.
pub(crate) fn len3(dx: F80, dy: F80, dz: F80) -> F80 {
    ((dx * dx + dy * dy) + dz * dz).sqrt()
}

/// The distance of two points, differences and squares in the register, unrounded.
pub(crate) fn dist(a: Vector, b: Vector) -> F80 {
    len3(x(a.x) - b.x, x(a.y) - b.y, x(a.z) - b.z)
}

/// [soft 0x084ace40 si_limbLength]: the sum of the `n` bone lengths from `node`
/// (unrounded), the longest and the shortest bone ([`crate::iktools::limb_length`]).
pub fn limb_length(m: &Mem, node: Node, n: i32) -> (F80, f32, f32) {
    let g = &Manip::read(m);
    let mut min = f32::MAX;
    let mut max = 0.0f32;
    let mut sum = F80::ZERO;
    let mut p = node;
    for _ in 0..n.max(0) {
        let Some(s) = son(m, p) else { break };
        let tx = m[s].trans.x;
        let len = if g.classical {
            if g.uniform == 1 { (x(m[p].cscl.x).abs() * tx).to_f32() } else { tx }
        } else {
            (x(m[p].gscl.x).abs() * tx).to_f32()
        };
        min = if !x(min).lt(x(len)) { fmov(len) } else { fmov(min) };
        max = if !x(max).gt(x(len)) { fmov(len) } else { fmov(max) };
        sum = sum + len;
        p = s;
    }
    (sum, max, min)
}

/// [soft 0x084ad134 si_limbPosition]: `gtrs` of the node `n` sons below `node`.
pub fn limb_position(m: &Mem, node: Node, n: i32) -> Vector {
    m[super::down(m, node, n)].gtrs
}

/// [soft 0x084aceec SI_volumeProjection]: the goal brought into what a limb of `n` joints
/// can reach, and where it was ([`crate::iktools::volume_projection`]).
pub fn volume_projection(m: &Mem, node: Node, n: i32, goal: Vector) -> (i32, Vector) {
    let g = &Manip::read(m);
    let root = pos(m, node, g);
    let dx = (x(goal.x) - root.x).to_f32();
    let dy = (x(goal.y) - root.y).to_f32();
    let dz = (x(goal.z) - root.z).to_f32();
    let d = ((x(dx) * dx + x(dy) * dy) + x(dz) * dz).sqrt().to_f32();
    if x(d).lt(x(EPS)) {
        return (VOL_AT_ROOT, goal);
    }
    let (len, longest, _) = limb_length(m, node, n);
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

/// [soft 0x084ad480 SI_modelSpecialUpdateSonState]: one node of a chain derived from its
/// father's global values, or, without a father, made the origin of the solver's frame
/// ([`crate::iktools::model_special_update_son_state`]). An effector (algorithm 3) under a
/// joint whose record's first word is 1 keeps `grot` = `mrot`; the original reads the
/// father and its record without a test (a null one is a fault on the image).
pub fn model_special_update_son_state(m: &mut Mem, father: Node, node: Node) {
    let g = &Manip::read(m);
    if father.is_null() {
        let f = m[node].father;
        if !f.is_null() {
            let s = m[f].gscl;
            m[node].hscl = s;
        }
        m[node].hrot = matrix_copy(&IDENTITY);
        m[node].htrs = Vector { x: 0.0, y: 0.0, z: 0.0 };
        m[node].gtrs = Vector { x: 0.0, y: 0.0, z: 0.0 };
    } else {
        let (gscl, grot, gtrs, gmat) = (m[father].gscl, m[father].grot, m[father].gtrs, m[father].gmat);
        m[node].hscl = gscl;
        m[node].hrot = matrix_copy(&grot);
        m[node].htrs = gtrs;
        let t = m[node].trans;
        let col = |j: usize| (((x(t.y) * gmat[1][j] + x(t.x) * gmat[0][j]) + x(t.z) * gmat[2][j]) + gmat[3][j]).to_f32();
        m[node].gtrs = Vector { x: col(0), y: col(1), z: col(2) };
    }
    let n = &mut m[node];
    n.gscl = Vector { x: (x(n.hscl.x) * n.scal.x).to_f32(), y: (x(n.hscl.y) * n.scal.y).to_f32(), z: (x(n.hscl.z) * n.scal.z).to_f32() };
    n.mrot = matrix_xyz_rotation(&mv(n.vrot));
    let keeps = m[node].algo_type == 3 && {
        let f = m[node].father;
        let r = ik(m, f);
        m.i32(r.addr()) == 1
    };
    let n = &mut m[node];
    n.grot = if keeps { matrix_copy(&n.mrot) } else { matrix_multiply_sub3(&n.mrot, &n.hrot) };
    n.gmat = matrix_copy(&n.grot);
    n.gmat[3][0] = fmov(n.gtrs.x);
    n.gmat[3][1] = fmov(n.gtrs.y);
    n.gmat[3][2] = fmov(n.gtrs.z);
    let scale = if !g.classical {
        Some(n.gscl)
    } else if g.uniform == 1 {
        Some(n.cscl)
    } else {
        None
    };
    if let Some(s) = scale {
        let s = [s.x, s.y, s.z];
        for i in 0..3 {
            n.gmat[i][0] = (x(n.gmat[i][0]) * s[i]).to_f32();
            n.gmat[i][1] = (x(n.gmat[i][1]) * s[i]).to_f32();
            n.gmat[i][2] = (x(s[i]) * n.gmat[i][2]).to_f32();
        }
    }
    if g.classical {
        n.unk_3ac = Vector { x: fmov(n.gtrs.x), y: fmov(n.gtrs.y), z: fmov(n.gtrs.z) };
    }
}

/// [soft 0x084ad86c SI_chainUpdate]: a chain brought up to date in the frame of `node`:
/// `node` as the origin, then each first son from its father down to and including the
/// first null of algorithm 3 or 4; with `sons`, that effector's first son gets the full
/// `SI_modelUpdateSonState`. A chain that runs out of sons first (the original faults)
/// stops there, recorded.
pub fn chain_update(m: &mut Mem, o: &mut dyn Outside, node: Node, sons: i16) {
    let mut father = Node::NULL;
    let mut p = node;
    loop {
        model_special_update_son_state(m, father, p);
        let Some(s) = son(m, p) else { return };
        p = s;
        if is_effector(m, p) {
            break;
        }
        father = m[p].father;
    }
    let f = m[p].father;
    model_special_update_son_state(m, f, p);
    if sons != 0 {
        let s = m[p].son;
        if !s.is_null() {
            update_son_state(m, o, p, s, 1);
        }
    }
}
