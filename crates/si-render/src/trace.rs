// SPDX-License-Identifier: MIT
//! Ray tracing through the BSP tree: the original's `trace.c` (finding 0012, "Ray tracing").
//!
//! Ported here: the cell search `si_search`, the four mailbox functions, the step from one
//! cell to the next (`push`), the secondary-ray walk `si_trace` and the shadow-ray walk
//! `si_sTrace`. `shadowMap` (the textures of a transparent occluder) is described in the
//! finding and comes in as an argument.

use crate::render::intersect;
use crate::shade::{set_material_from_vertex_color, ShadeState};
use crate::types::{RVertex, Ray, Tn, Vector};
use si_x87::{fmov, fx, x, Round, F80};

/// `tree`: one node of the BSP tree over the scene's triangles, 0x24 bytes. An inner node
/// and a leaf use the words from +0x08 differently; both readings are members here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    /// +0x00: 0 a leaf (a cell); 1, 2, 3 a node split across x, y, z.
    pub kind: i16,
    /// +0x02: the node's depth in the tree (`push` prefers the shallowest of the nodes it
    /// could continue from).
    pub level: i16,
    /// +0x04: the mailbox, compared with the current ray number by `si_trace`.
    pub mailbox: i32,
    /// +0x08 of an inner node: where it is split.
    pub split: f32,
    /// +0x0c, +0x10 of an inner node: the children on the side of smaller and of greater
    /// coordinates.
    pub low: Option<usize>,
    pub high: Option<usize>,
    /// +0x08 of a leaf: head of the cell's list of [`Link`] records.
    pub list: Option<usize>,
    /// +0x0c to +0x20 of a leaf: the inner nodes whose split planes are the cell's six
    /// walls, in the order x low, y low, z low, x high, y high, z high.
    pub walls: [Option<usize>; 6],
}

/// `link`: one entry of a cell's triangle list, 0x20 bytes: the triangle's bounding box
/// (x min, x max, y min, y max, z min, z max), the triangle at +0x18, the next entry at
/// +0x1c. The same record the geometry stage's `traceList` makes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Link {
    pub bound: [f32; 6],
    pub tn: usize,
    pub next: Option<usize>,
}

/// The tree and what hangs off it.
#[derive(Clone, Debug, Default)]
pub struct Bsp {
    pub nodes: Vec<Tree>,
    pub links: Vec<Link>,
    /// 0x08790928: the root.
    pub root: usize,
    /// 0x0879092c: the node that stands for everything outside the scene's box; reaching it
    /// ends a ray.
    pub outside: Option<usize>,
}

/// The leaf a point lies in [soft 0x0826df20 si_search]: `si_search(point, root)`. At a node
/// split across an axis the point goes to the low child when its coordinate is below the
/// split, to the high child otherwise (on the split plane, or a NaN: high). A node whose
/// kind is none of 0..3 ends the search with a null result, here `None`; so does a missing
/// child, which the original would dereference. Class: copy (comparisons only).
pub fn search(nodes: &[Tree], root: usize, p: Vector) -> Option<usize> {
    let mut cur = root;
    loop {
        let node = &nodes[cur];
        let c = match node.kind {
            0 => return Some(cur),
            1 => p.x,
            2 => p.y,
            3 => p.z,
            _ => return None,
        };
        cur = if x(c).lt(x(node.split)) { node.low? } else { node.high? };
    }
}

/// 0x08654408 (and its negative at 0x08654404), float: how far past a wall the point handed
/// to the next search is pushed. 0x0865440c, float: the distance to a wall the ray runs
/// parallel to. 0x08654410, double: added to the wall's distance to give the ray's limit.
const NUDGE: f32 = f32::from_bits(0x38d1_b717);
const NO_WALL: f32 = 100000.0;
const PAST_WALL: f64 = 1e-4;
/// 0x08654400, float: the least distance of a hit on a secondary ray.
const TRACE_TMIN: f32 = f32::from_bits(0x38d1_b717);

/// Where a ray leaves its present cell [soft 0x0826dd00 push]: `push(ray, &point)`.
///
/// Per axis, by the sign of the direction: the wall ahead (the cell's low wall for a
/// negative component, its high wall for a positive one), its distance (wall − origin) ×
/// the stored reciprocal of the direction, the node beyond it (the wall node's child on the
/// far side) and a nudge of ∓0.0001; a zero component gives the distance 100000 and no
/// node. The least of the three distances t is chosen (`tx` if below both others, else `ty`
/// if below `tz`, else `tz`; ties go to the later axis). The point is origin + t ×
/// direction; on each axis whose distance equals t the nudge is added, and of the nodes
/// beyond those walls the one with the smallest level is returned (the first on ties). The
/// ray's distance becomes t + 0.0001, so that the intersection tests that follow accept
/// only hits inside the cell.
///
/// When the chosen distance is not the x distance and no later axis matches, or a matching
/// axis has a zero component, the original uses a register that holds nothing meaningful;
/// here that is a panic. Class: model.
pub fn push(nodes: &[Tree], ray: &mut Ray) -> (usize, Vector) {
    let cell = &nodes[ray.w38 as usize];
    let (o, d, inv) = (ray.origin, ray.dir, ray.v18);
    let wall = |i: usize| &nodes[cell.walls[i].expect("a cell has six walls")];
    // (distance, node beyond, nudge)
    let axis = |dc: f32, oc: f32, ic: f32, lo: usize, hi: usize| -> (F80, Option<usize>, f32) {
        if x(dc).lt(x(0.0f32)) {
            let s = wall(lo).split;
            (fx!((mul (sub (f s) (f oc)) (f ic))), wall(lo).low, -NUDGE)
        } else if x(dc).gt(x(0.0f32)) {
            let s = wall(hi).split;
            (fx!((mul (sub (f s) (f oc)) (f ic))), wall(hi).high, NUDGE)
        } else {
            (x(NO_WALL), None, 0.0)
        }
    };
    let (tx, nx, ex) = axis(d.x, o.x, inv.x, 0, 3);
    let (ty, ny, ey) = axis(d.y, o.y, inv.y, 1, 4);
    let (tz, nz, ez) = axis(d.z, o.z, inv.z, 2, 5);
    let t = if tx.lt(ty) {
        if tz.gt(tx) { tx } else { tz }
    } else if tz.gt(ty) {
        ty
    } else {
        tz
    };
    let px = fx!((add (mul (x t) (f d.x)) (f o.x)));
    let mut p = Vector::new(px.to_f32(), fx!(f32: (add (mul (x t) (f d.y)) (f o.y))), fx!(f32: (add (mul (x t) (f d.z)) (f o.z))));
    let mut node: Option<usize> = None;
    let mut level = 10000i16;
    if t.eq(tx) {
        p.x = fx!(f32: (add (f ex) (x px)));
        let n = nx.expect("push: no node beyond the x wall");
        node = Some(n);
        level = nodes[n].level;
    }
    if ty.eq(t) {
        p.y = ey + p.y;
        let n = ny.expect("push: no node beyond the y wall");
        if nodes[n].level < level {
            node = Some(n);
            level = nodes[n].level;
        }
    }
    if t.eq(tz) {
        p.z = ez + p.z;
        let n = nz.expect("push: no node beyond the z wall");
        if nodes[n].level < level {
            node = Some(n);
        }
    }
    ray.t = fx!(f32: (add (d PAST_WALL) (x t)));
    (node.expect("push: no wall was reached"), p)
}

/// Follows a secondary ray through the cells until it hits a triangle
/// [soft 0x0826dbc0 si_trace]: `si_trace(ray, from)`, 1 for a hit and 0 when the ray leaves
/// the scene. The ray must carry its origin, direction and starting cell (+0x38).
///
/// A wrapped ray counter resets every mailbox. The reciprocals of the direction go into the
/// ray (+0x18), the ray takes a new number, and then, per cell: [`push`] finds the exit and
/// the node beyond; while that node's mailbox holds the ray's number the search moves on
/// through it without testing anything (nothing in this function stores a number in a
/// mailbox); every triangle of the cell's list that is neither void (flag 0x8000) nor of a
/// shadow-object material (flag 0x2000) is tested with `SI_Intersect(ray, tn, from, 0.0001)`,
/// all of them (the original tests for a return value of 1 to stop early, which
/// `SI_Intersect` never gives); if the ray's distance came down, that is the hit; else if the
/// node beyond is the "outside" node the ray is lost; else the cell containing the exit
/// point is searched from that node and the loop repeats. Class: model.
pub fn trace(bsp: &mut Bsp, mailbox: &mut Mailbox, ray: &mut Ray, tns: &[Tn], verts: &[RVertex], from: usize) -> bool {
    if mailbox.check_for_overflow() {
        let root = bsp.root;
        mailbox.reset(&mut bsp.nodes, root);
    }
    let bsp = &*bsp;
    ray.v18 = Vector::new(1.0 / ray.dir.x, 1.0 / ray.dir.y, 1.0 / ray.dir.z);
    mailbox.get_new_id();
    let descend = |node: usize, p: Vector| search(&bsp.nodes, node, p).expect("si_search found no cell") as u32;
    loop {
        let (mut node, mut exit) = push(&bsp.nodes, ray);
        while bsp.nodes[node].mailbox == mailbox.current {
            ray.w38 = descend(node, exit);
            (node, exit) = push(&bsp.nodes, ray);
        }
        let limit = ray.t;
        let mut cur = bsp.nodes[ray.w38 as usize].list;
        while let Some(l) = cur {
            let link = &bsp.links[l];
            cur = link.next;
            let flags = tns[link.tn].flags;
            if flags & 0x2000 == 0 && (flags as i16) >= 0 {
                intersect(ray, tns, verts, link.tn, Some(from), TRACE_TMIN);
            }
        }
        if x(limit).gt(x(ray.t)) {
            return true;
        }
        if Some(node) == bsp.outside {
            return false;
        }
        ray.w38 = descend(node, exit);
    }
}

/// 0x08654418, float: the least distance of a hit on a shadow ray leaving its surface.
const SHADOW_TMIN: f32 = f32::from_bits(0x38d1_b717);
/// 0x08654428, double: added to a transparent occluder's distance to give the least distance
/// of the next hit.
const SHADOW_PAST: f64 = 1e-4;
/// 0x08654420, double: the transparency above which a transparent occluder lets part of the
/// light through uncoloured.
const SHADOW_HALF: f64 = 0.5;
/// Flag bit 0x0800 of a triangle: met by the present shadow ray in the present cell.
pub const TN_MET: u16 = 0x0800;

/// What is left of a light along a shadow ray [soft 0x0826dfc4 si_sTrace]:
/// `si_sTrace(ray, from, distance, light)` returns four floats: a factor for each of the
/// light's three colour components and a fourth float that is never written (`unset`).
///
/// The walk is `si_trace`'s (reciprocals, a new ray number, [`push`] from cell to cell,
/// cells whose exit node carries the ray's number passed over), with these differences:
///
/// - every triangle of a cell that is not void (0x8000) and not marked "met" (0x0800) is
///   tested, **shadow objects included**, with `SI_Intersect(ray, tn, from, tmin)`; `tmin`
///   starts at the float 0.0001. On entering a cell the mark is cleared in all its
///   triangles.
/// - a hit counts when it is nearer than both the cell's exit (as `push` left it in the
///   ray) and `distance`, the distance to the light (limit > t, or unordered).
/// - the triangle hit is marked. If the light is selective and does not light the
///   triangle's model, the rest of the cell is not looked at: the walk goes on with the
///   next cell (the ray's distance still that of the hit).
/// - an opaque triangle (none of the flags 0x340) ends the walk: all three factors are the
///   light's umbra (+0x90), **whatever transparent triangles were passed before**.
/// - a transparent one is passed. Its material's working block is adjusted at the hit
///   point: by `si_setMaterialFromVertexColor` unless both vertex colour modes are 1, and
///   by `shadowMap` with flag 0x0400. With static blur on the material the blur factor is
///   read from its table at |d̂ · N| (N the corner normals weighted by the hit's weights,
///   which are scaled by the triangle's `inv_n` and stored back in the ray, or corner 0's
///   normal without flag 2; both vectors normalised unless of length 0); **the factor is a
///   local that starts at 1 and keeps the last surface's value** for a later surface
///   without static blur. Then, with flag 0x100 or 0x200 or static blur:
///   k = float(1 − factor × opacity), held at 0 from below; below 0.5 each component is
///   multiplied by float(2k) × diffuse; else by (1 − w) × diffuse + w with w = float(2(k −
///   0.5)). The same cell is then searched again from the hit (`from` = that triangle,
///   `tmin` = its distance + 0.0001, the ray's distance back at the cell's exit).
/// - leaving a cell without a countable hit: the walk ends when the node beyond is the
///   "outside" node or `distance` is below the ray's distance; else the next cell.
/// - at the end, if a transparent triangle was passed, each factor f becomes float(float(1
///   − umbra) × f + umbra).
///
/// A hit the original takes with an unordered comparison (a NaN distance) reads the ray's
/// triangle member, which may be stale or null; the Rust panics on a null. Class: model.
#[allow(clippy::too_many_arguments)]
pub fn s_trace(
    bsp: &mut Bsp,
    mailbox: &mut Mailbox,
    st: &mut ShadeState,
    ray: &mut Ray,
    from: usize,
    distance: f32,
    light: usize,
    shadow_map: &mut dyn FnMut(&mut ShadeState, &mut Ray, usize, Vector),
) -> [f32; 4] {
    let mut tmin = SHADOW_TMIN;
    let mut from = from;
    let mut passed = false;
    let umbra = fmov(st.lights[light].umbra);
    let mut blur = 1.0f32;
    let mut left = [1.0f32; 3];
    if mailbox.check_for_overflow() {
        let root = bsp.root;
        mailbox.reset(&mut bsp.nodes, root);
    }
    let bsp = &*bsp;
    ray.v18 = Vector::new(fx!(f32: (div (x F80::ONE) (f ray.dir.x))), fx!(f32: (div (x F80::ONE) (f ray.dir.y))), fx!(f32: (div (x F80::ONE) (f ray.dir.z))));
    mailbox.get_new_id();
    let descend = |node: usize, p: Vector| search(&bsp.nodes, node, p).expect("si_search found no cell") as u32;
    loop {
        let (mut node, mut exit) = push(&bsp.nodes, ray);
        while bsp.nodes[node].mailbox == mailbox.current {
            ray.w38 = descend(node, exit);
            (node, exit) = push(&bsp.nodes, ray);
        }
        let cell_limit = fmov(ray.t);
        let head = bsp.nodes[ray.w38 as usize].list;
        let mut cur = head;
        while let Some(l) = cur {
            st.tns[bsp.links[l].tn].flags &= !TN_MET;
            cur = bsp.links[l].next;
        }
        loop {
            ray.t = cell_limit;
            let limit = if x(distance).gt(x(cell_limit)) { cell_limit } else { distance };
            let mut cur = head;
            while let Some(l) = cur {
                let link = &bsp.links[l];
                cur = link.next;
                if st.tns[link.tn].flags & (0x8000 | TN_MET) == 0 {
                    intersect(ray, &st.tns, &st.verts, link.tn, Some(from), tmin);
                }
            }
            if x(limit).lt(x(ray.t)) || x(limit).eq(x(ray.t)) {
                break;
            }
            let hit = ray.tn.expect("si_sTrace: the ray names no triangle");
            st.tns[hit].flags |= TN_MET;
            let flags = st.tns[hit].flags;
            let lt = &st.lights[light];
            if lt.selective != 0 && lt.select_table[st.tns[hit].w58 as usize] != lt.select_id {
                break;
            }
            if flags & 0x340 == 0 {
                return [umbra, umbra, umbra, st.glb.unset];
            }
            passed = true;
            let (t, d, o) = (ray.t, ray.dir, ray.origin);
            let p = Vector::new(fx!(f32: (add (mul (f t) (f d.x)) (f o.x))), fx!(f32: (add (mul (f t) (f d.y)) (f o.y))), fx!(f32: (add (mul (f t) (f d.z)) (f o.z))));
            let mat = st.tns[hit].material as usize;
            if !(st.materials[mat].vcol_rgb == 1 && st.materials[mat].vcol_alpha == 1) {
                set_material_from_vertex_color(st, ray, p);
            }
            if flags & 0x400 != 0 {
                shadow_map(st, ray, mat, p);
            }
            if st.materials[mat].static_blur != 0 {
                let tn = st.tns[hit].clone();
                let b = [x(ray.hit.x) * tn.inv_n, x(ray.hit.y) * tn.inv_n, x(ray.hit.z) * tn.inv_n];
                ray.hit = Vector::new(b[0].to_f32(), b[1].to_f32(), b[2].to_f32());
                // a triangle that was hit has its three corners
                let nv = tn.v.map(|i| st.verts[i.expect("a hit triangle has three corners")].normal);
                let n = if tn.flags & 2 != 0 {
                    let nx = (b[0] * nv[0].x + b[1] * nv[1].x) + b[2] * nv[2].x;
                    let ny = (b[0] * nv[0].y + b[1] * nv[1].y) + b[2] * nv[2].y;
                    let nz = b[2] * nv[2].z + (b[1] * nv[1].z + b[0] * nv[0].z);
                    let len = ((ny * ny + nx * nx) + nz * nz).sqrt();
                    if x(0.0f32).eq(len) {
                        [nx.to_f32(), ny.to_f32(), nz.to_f32()]
                    } else {
                        [(nx / len).to_f32(), (ny / len).to_f32(), (nz / len).to_f32()]
                    }
                } else {
                    [nv[0].x, nv[0].y, nv[0].z]
                };
                let len = ((x(d.y) * d.y + x(d.x) * d.x) + x(d.z) * d.z).sqrt();
                let dn = if x(0.0f32).eq(len) { [d.x, d.y, d.z] } else { [(x(d.x) / len).to_f32(), (x(d.y) / len).to_f32(), (x(d.z) / len).to_f32()] };
                let mut c = (x(dn[0]) * n[0] + x(dn[1]) * n[1]) + x(dn[2]) * n[2];
                if !(x(0.0f32).lt(c) || x(0.0f32).eq(c)) {
                    c = -c;
                }
                blur = st.materials[mat].blur_table.lookup(c).to_f32();
            }
            let m = &st.materials[mat];
            if flags & 0x300 != 0 || m.static_blur != 0 {
                let k = (F80::ONE - x(blur) * m.work.opacity).to_f32();
                let k = if x(k).lt(x(0.0f32)) { x(0.0f32) } else { x(k) };
                if x(SHADOW_HALF).gt(k) {
                    let k2 = (k + k).to_f32();
                    for i in 0..3 {
                        left[i] = ((x(k2) * m.work.diffuse[i]) * left[i]).to_f32();
                    }
                } else {
                    let w = ((k - SHADOW_HALF) + (k - SHADOW_HALF)).to_f32();
                    let rest = F80::ONE - x(w);
                    for i in 0..3 {
                        left[i] = ((rest * m.work.diffuse[i] + w) * left[i]).to_f32();
                    }
                }
            }
            tmin = (x(ray.t) + SHADOW_PAST).to_f32();
            from = hit;
            // the triangle is transparent (tested above), so the cell is searched again
        }
        if Some(node) == bsp.outside || x(distance).lt(x(ray.t)) {
            break;
        }
        ray.w38 = descend(node, exit);
    }
    if passed {
        let a = (F80::ONE - x(umbra)).to_f32();
        for f in &mut left {
            *f = (x(a) * *f + umbra).to_f32();
        }
    }
    [left[0], left[1], left[2], st.glb.unset]
}

/// The ray counter behind the mailboxes, the file-scope `mailboxCurrent`: each traced ray
/// takes a new number, and a triangle list already looked at by the ray carrying the current
/// number is not looked at again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mailbox {
    pub current: i32,
}

impl Mailbox {
    /// `mailboxCurrent += 1` [soft 0x0826db30 mailboxGetNewID]. The original's addition
    /// wraps; so does this.
    pub fn get_new_id(&mut self) {
        self.current = self.current.wrapping_add(1);
    }

    /// Clears the mailbox of a node and of everything below it
    /// [soft 0x0826db44 mailboxResetTree]: the node first, then its low child's subtree, then
    /// its high child's.
    ///
    /// The original does not look at the node's kind: it follows the words at +0x0c and
    /// +0x10 of every node. In a leaf those are the cell's x-low and y-low walls, which are
    /// ancestors of the leaf, so on a real tree the original's recursion would never end. It
    /// is reached only when the ray counter has wrapped past 2³¹. The Rust stops at a leaf.
    pub fn reset_tree(nodes: &mut [Tree], node: usize) {
        nodes[node].mailbox = 0;
        if let Some(low) = nodes[node].low {
            Mailbox::reset_tree(nodes, low);
        }
        if let Some(high) = nodes[node].high {
            Mailbox::reset_tree(nodes, high);
        }
    }

    /// Counter back to 1 and every mailbox of the tree at 0x08790928 cleared
    /// [soft 0x0826db80 mailboxReset]. The original does not test the root for null.
    pub fn reset(&mut self, nodes: &mut [Tree], root: usize) {
        self.current = 1;
        Mailbox::reset_tree(nodes, root);
    }

    /// True once the counter has wrapped to a negative number
    /// [soft 0x0826dba4 mailboxCheckForOverflow].
    pub fn check_for_overflow(&self) -> bool {
        self.current < 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Vec<Tree> {
        vec![
            Tree { kind: 1, split: 0.0, low: Some(1), high: Some(2), mailbox: 3, ..Tree::default() },
            Tree { kind: 0, mailbox: 4, ..Tree::default() },
            Tree { kind: 3, split: -5.0, low: Some(3), high: Some(4), mailbox: 5, ..Tree::default() },
            Tree { kind: 0, mailbox: 6, ..Tree::default() },
            Tree { kind: 7, mailbox: 7, ..Tree::default() },
        ]
    }

    #[test]
    fn search_descends_by_strict_less() {
        let t = tree();
        assert_eq!(search(&t, 0, Vector::new(-1.0, 0.0, 0.0)), Some(1));
        // on the split plane: the high side
        assert_eq!(search(&t, 0, Vector::new(0.0, 0.0, -6.0)), Some(3));
        // a node of unknown kind: null
        assert_eq!(search(&t, 0, Vector::new(0.0, 0.0, -5.0)), None);
        // a NaN goes high
        assert_eq!(search(&t, 0, Vector::new(f32::NAN, 0.0, -9.0)), Some(3));
        // a leaf root is its own answer
        assert_eq!(search(&t, 1, Vector::default()), Some(1));
    }

    /// Two cells side by side along x, split at x = 0, inside the box −10..10; node 0 is the
    /// root, 1 and 2 the cells, 3 the outside node; the outer walls are nodes 4 (low) and 5
    /// (high) whose far children are the outside node.
    fn two_cells() -> Bsp {
        let wall = |split: f32| Tree { kind: 1, level: 0, split, low: Some(3), high: Some(3), ..Tree::default() };
        let cell = |list: Option<usize>, xlo: usize, xhi: usize| Tree { kind: 0, level: 1, list, walls: [Some(xlo), Some(4), Some(4), Some(xhi), Some(5), Some(5)], ..Tree::default() };
        Bsp {
            nodes: vec![
                Tree { kind: 1, level: 0, split: 0.0, low: Some(1), high: Some(2), ..Tree::default() },
                cell(None, 4, 0),
                cell(Some(0), 0, 5),
                Tree { kind: 0, level: 0, ..Tree::default() },
                wall(-10.0),
                wall(10.0),
            ],
            links: vec![Link { tn: 0, ..Link::default() }],
            root: 0,
            outside: Some(3),
        }
    }

    #[test]
    fn push_finds_the_wall_ahead() {
        let bsp = two_cells();
        let mut ray = Ray { origin: Vector::new(-4.0, 1.0, 2.0), dir: Vector::new(1.0, 0.0, 0.0), w38: 1, ..Ray::default() };
        ray.v18 = Vector::new(1.0, f32::INFINITY, f32::INFINITY);
        let (node, p) = push(&bsp.nodes, &mut ray);
        // the wall at x = 0 is 4 away; beyond it the root's high child; the point is nudged
        assert_eq!(node, 2);
        assert_eq!(p, Vector::new(NUDGE, 1.0, 2.0));
        assert_eq!(ray.t, (x(1e-4f64) + x(4.0f32)).to_f32());
        // the other way: the low outer wall, beyond it the outside node
        ray.dir = Vector::new(-1.0, 0.0, 0.0);
        ray.v18.x = -1.0;
        let (node, p) = push(&bsp.nodes, &mut ray);
        assert_eq!(node, 3);
        assert_eq!(p.x, -NUDGE + -10.0);
    }

    #[test]
    fn trace_walks_to_the_hit_or_out() {
        let mut bsp = two_cells();
        // one triangle in the plane x = 5 around (5, 0, 0), dominant axis x
        let verts: Vec<RVertex> = [(5.0, -4.0, -4.0), (5.0, 4.0, -4.0), (5.0, 0.0, 4.0)].iter().map(|&(a, b, c)| RVertex { world: Vector::new(a, b, c), ..RVertex::default() }).collect();
        let tn = Tn {
            v: [Some(0), Some(1), Some(2)],
            n: Vector::new(64.0, 0.0, 0.0),
            d: 320.0,
            axis: 1,
            edge: [Vector::new(0.0, -4.0, 8.0), Vector::new(0.0, -4.0, -8.0), Vector::new(0.0, 8.0, 0.0)],
            ..Tn::default()
        };
        let mut tns = vec![tn, Tn::default()];
        let mut mailbox = Mailbox { current: 1 };
        let mut ray = Ray { origin: Vector::new(-4.0, 0.0, 0.0), dir: Vector::new(1.0, 0.0, 0.0), w38: 1, ..Ray::default() };
        assert!(trace(&mut bsp, &mut mailbox, &mut ray, &tns, &verts, 1));
        assert_eq!((ray.tn, ray.t, ray.w38, mailbox.current), (Some(0), 9.0, 2, 2));
        assert_eq!(ray.v18.x, 1.0);
        // the triangle the ray starts from is not hit; the ray leaves the scene
        let mut ray = Ray { origin: Vector::new(-4.0, 0.0, 0.0), dir: Vector::new(1.0, 0.0, 0.0), w38: 1, ..Ray::default() };
        assert!(!trace(&mut bsp, &mut mailbox, &mut ray, &tns, &verts, 0));
        // a shadow-object triangle (flag 0x2000) is not seen either
        tns[0].flags = 0x2000;
        let mut ray = Ray { origin: Vector::new(-4.0, 0.0, 0.0), dir: Vector::new(1.0, 0.0, 0.0), w38: 1, ..Ray::default() };
        assert!(!trace(&mut bsp, &mut mailbox, &mut ray, &tns, &verts, 1));
    }

    #[test]
    fn mailboxes() {
        let mut t = tree();
        let mut m = Mailbox { current: 41 };
        m.get_new_id();
        assert_eq!(m.current, 42);
        m.reset(&mut t, 0);
        assert_eq!(m.current, 1);
        assert!(t.iter().all(|n| n.mailbox == 0));
        assert!(!m.check_for_overflow());
        m.current = i32::MAX;
        m.get_new_id();
        assert!(m.check_for_overflow());
    }
}

// ---------------------------------------------------------------------------------- shadowMap

/// The texture stage for a surface a shadow ray passes [soft 0x0826e534 shadowMap]:
/// `shadowMap(ray, material, &point)`, called by `si_sTrace` for a transparent triangle
/// whose flag word has 0x400. The ray's triangle is the one hit.
///
/// 1. The working diffuse colour (+0x720) is the material's again (+0x44, four floats) and
///    the working opacity 1 − transparency.
/// 2. The hit's three weights are multiplied by the triangle's `inv_n` and stored back
///    (rounded); inv = 1 / ((w₁ + w₀) + w₂) from the unrounded products, rounded to double.
/// 3. The material's 3D texture (+0x6fc), if any: `SI_renderTxt3d(material, point, null)`.
/// 4. Every 2D map of the material's list, then of the model's own (the model through the
///    dictionary by the triangle's element number), **that drives the diffuse colour
///    (+0x114) or the transparency (+0x11a)**; the pair counter goes up for every map,
///    used or not. u and v are interpolated from the corners' pairs with the stored
///    weights, times inv; then the coordinate rules of `SI_shade` step 7 (methods, the
///    region, the seam), `si_BlookUp(u, v, map, map +0x2c, &sample, 0, …)` and
///    `SI_renderTxt2d(material, map, sample)`. A reflection map is not passed over here as
///    `SI_shade` does, and nothing perturbs a normal.
/// 5. The model's own 3D texture (+0x224), if any, **whether the material had one or not**:
///    it is put in the material's place (+0x6fc) for the call and taken out again.
///
/// The global switches for textures are not looked at. Class: model.
pub fn shadow_map<E: crate::shade::ShadeEnv>(st: &mut ShadeState, env: &mut E, ray: &mut Ray, mat: usize, p: Vector) {
    {
        let m = &mut st.materials[mat];
        m.work.diffuse = m.diffuse;
        m.work.opacity = (F80::ONE - m.transparency).to_f32();
    }
    let tn = st.tns[ray.tn.expect("shadowMap: the ray has hit a triangle")].clone();
    let b0 = x(ray.hit.x) * tn.inv_n;
    let b1 = x(ray.hit.y) * tn.inv_n;
    let b2 = x(ray.hit.z) * tn.inv_n;
    ray.hit = Vector::new(b0.to_f32(), b1.to_f32(), b2.to_f32());
    let inv = (F80::ONE / ((b1 + b0) + b2)).to_f64();
    if st.materials[mat].tex3d != 0 {
        env.render_txt3d(st, mat, p, None);
    }
    let v = tn.v.map(|i| st.verts[i.expect("a hit triangle has three corners")]);
    let b = [ray.hit.x, ray.hit.y, ray.hit.z];
    let model = st.models[tn.w58 as usize].clone();
    let mut pair = 0usize;
    for head in [st.materials[mat].maps, model.maps] {
        let mut cur = head;
        while let Some(map) = cur {
            cur = st.maps[map].next;
            let this = pair;
            pair += 1;
            let m = st.maps[map].clone();
            if m.fx_diffuse == 0 && m.fx_transp == 0 {
                continue;
            }
            let uv = [0, 1, 2].map(|k| st.uvs[v[k].uv as usize + this]);
            let uf = (((x(b[2]) * uv[2][0]) + ((x(b[1]) * uv[1][0]) + (x(b[0]) * uv[0][0]))) * inv).to_f32();
            let vf = ((((x(b[1]) * uv[1][1]) + (x(b[0]) * uv[0][1])) + (x(b[2]) * uv[2][1])) * inv).to_f32();
            let (mut u, mut vv) = (x(uf), x(vf));
            let tiled = m.method == 4 && m.repeat != 0;
            if !(1..=3).contains(&m.method) {
                if m.method == 4 {
                    if m.repeat != 0 {
                        u = x((x(uf) - x(x(uf).round_int(Round::Down).to_f64())).to_f32());
                        vv = x((x(vf) - x(x(vf).round_int(Round::Down).to_f64())).to_f32());
                    } else {
                        if m.wrap_u != 0 {
                            u = x(uf) - x(x(uf).to_i32(Round::Zero));
                        }
                        if m.wrap_v != 0 {
                            vv = x(vf) - x(x(vf).to_i32(Round::Zero));
                        }
                    }
                } else {
                    u = x(uf) - x(x(uf).to_i32(Round::Zero));
                    vv = x(vf) - x(x(vf).to_i32(Round::Zero));
                }
            }
            if !tiled {
                let inside = |c: F80, lo: f32, hi: f32, wrap: i16, band: i16, lo2: f32, hi2: f32| {
                    (c.ge(x(lo)) && c.le(x(hi))) || (wrap != 0 && (c.ge(x(lo2)) || c.le(x(hi2)))) || (band != 0 && c.ge(x(lo2)) && c.le(x(hi2)))
                };
                if !inside(u, m.umin, m.umax, m.wrap_u, m.band_u, m.u2min, m.u2max) || !inside(vv, m.vmin, m.vmax, m.wrap_v, m.band_v, m.v2min, m.v2max) {
                    continue;
                }
            }
            if m.wrap_u != 0 && u.lt(x(m.u2max)) {
                u = u + 1.0f32;
            }
            if m.wrap_v != 0 && vv.lt(x(m.v2max)) {
                vv = vv + 1.0f32;
            }
            let sample = env.blookup(st, u.to_f32(), vv.to_f32(), map, false)[0];
            env.render_txt2d(st, mat, map, sample);
        }
    }
    if model.tex3d != 0 {
        let own = st.materials[mat].tex3d;
        st.materials[mat].tex3d = model.tex3d;
        env.render_txt3d(st, mat, p, None);
        st.materials[mat].tex3d = own;
    }
}
