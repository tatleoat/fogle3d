// SPDX-License-Identifier: MIT
//! Building the BSP tree over the scene's triangles: the original's `bsptree.c` (finding
//! 0012, section 7a). All five functions are here.
//!
//! The original takes every record from the renderer's chunk memory; here a node is an entry
//! of [`Bsp::nodes`] and a list entry one of [`Bsp::links`], pushed in the order the original
//! allocates them, so that the n-th node made is the n-th of the vector. An allocation
//! failure, which the original does not test for, is not modelled.

use crate::trace::{Bsp, Link, Tree};
use crate::types::{RVertex, Tn};
use si_x87::{fmov, x, F80};

/// The settings the build reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BspParams {
    /// 0x08790dba, short: a cell with fewer triangles than this is not split.
    pub min_count: i16,
    /// 0x08790dbc, short: a cell deeper than this is not split.
    pub max_level: i16,
    /// `cellVolumn`, 0x0869ce20, float (0.01 in the file): a cell of smaller volume is not
    /// split.
    pub cell_volume: f32,
    /// 0x08790dc0, float (`initData` stores 0.15): the fraction of a cell's extent that
    /// must be free of triangles on one side before that slab is cut off as an empty cell.
    pub empty_fraction: f32,
    /// 0x08790ba0, six floats: the box a cell's content is measured from (`initData`
    /// stores 100000, −100000 three times: least values start high, greatest low).
    pub empty_box: [f32; 6],
}

impl Default for BspParams {
    fn default() -> Self {
        BspParams {
            min_count: 0,
            max_level: 0,
            cell_volume: f32::from_bits(0x3c23_d70a),
            empty_fraction: f32::from_bits(0x3e19_999a),
            empty_box: [100000.0, -100000.0, 100000.0, -100000.0, 100000.0, -100000.0],
        }
    }
}

/// The three counters of `SI_stats` the build adds to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BspStats {
    /// 0x08791204 (`SI_stats` +0x24): list entries made (`traceList` counts in the same
    /// word).
    pub links: i32,
    /// 0x08791208 (+0x28): inner nodes made, the seven of the ancestry included.
    pub nodes: i32,
    /// 0x0879120c (+0x2c): cells made.
    pub leaves: i32,
}

/// 0x08654430, 0x08654440, 0x08654450, 0x08654460: the double 0.0001. Added to the content's
/// extent when an empty slab is cut off; below it an edge counts as parallel to the split
/// plane.
const EPS: f64 = 1e-4;
/// 0x08654438, 0x08654448, 0x08654458: the double 0.5.
const HALF: f64 = 0.5;
/// The kind (+0x00) of the seven nodes of the ancestry.
pub const KIND_ANCESTRY: i16 = 4;

/// The nodes around the scene's box [soft 0x0826f7b0 si_initAncestry]: `si_initAncestry()`
/// returns an `SI_Ancestors`, six node pointers in the order x low, y low, z low, x high, y
/// high, z high (0x18 bytes of chunk memory; here the array returned).
///
/// Seven nodes of 0x14 bytes and kind 4 are made: the "outside" node (level 0, everything
/// else zero), whose address goes to 0x0879092c, and one wall per side of the scene's box
/// (`scene` = the six floats at 0x087908f4: x min, x max, y min, y max, z min, z max) in the
/// order x low, x high, y low, y high, z low, z high: level 1, the split position that
/// side's coordinate, both children the outside node, mailbox 0. The node counter goes up by
/// 7. Class: copy.
pub fn init_ancestry(bsp: &mut Bsp, scene: &[f32; 6], stats: &mut BspStats) -> [Option<usize>; 6] {
    stats.nodes += 7;
    let outside = bsp.nodes.len();
    bsp.nodes.push(Tree { kind: KIND_ANCESTRY, level: 0, ..Tree::default() });
    bsp.outside = Some(outside);
    let mut anc = [None; 6];
    // the box's members in order, and the place of each wall in the record
    for (i, slot) in [0usize, 3, 1, 4, 2, 5].into_iter().enumerate() {
        anc[slot] = Some(bsp.nodes.len());
        bsp.nodes.push(Tree {
            kind: KIND_ANCESTRY,
            level: 1,
            split: fmov(scene[i]),
            low: Some(outside),
            high: Some(outside),
            ..Tree::default()
        });
    }
    anc
}

/// What a build works on: the tree being made, the triangles the list entries name and
/// their corners, the settings and the counters.
pub struct BspBuild<'a> {
    pub bsp: &'a mut Bsp,
    pub tns: &'a [Tn],
    pub verts: &'a [RVertex],
    pub params: BspParams,
    pub stats: &'a mut BspStats,
}

/// A list under construction: head and length. Entries are put at the head.
#[derive(Clone, Copy, Default)]
struct List {
    head: Option<usize>,
    count: i32,
}

impl BspBuild<'_> {
    fn put(&mut self, list: &mut List, link: usize) {
        list.count += 1;
        self.bsp.links[link].next = list.head;
        list.head = Some(link);
    }

    /// The tree of one cell [soft 0x0826f95c si_bsptree]:
    /// `si_bsptree(list, count, &bound, &ancestors, level)` returns the node.
    ///
    /// **A cell** (kind 0, level + 1, the list as given, the six ancestors as its walls) is
    /// made when the level is above 0x7ff8, or the count is below the least count, or the
    /// level is above the greatest level, or the volume, (dx × dy) × dz formed in the
    /// register from the unrounded differences of the bound, is below `cellVolumn`.
    ///
    /// Otherwise the box of the list's entries is measured (from [`BspParams::empty_box`];
    /// a least value is replaced when an entry's is below it, a greatest when an entry's is
    /// above it) and six fractions are formed, each in the register with the bound's extent
    /// rounded to float as divisor: the free space below the content and above it, per
    /// axis. The greatest of them that exceeds [`BspParams::empty_fraction`] is cut off as
    /// an empty cell: tested in the order x low, x high, y low, y high, z low, z high, each
    /// against the unrounded best so far and strictly, **except the last, which is tested
    /// rounded to float** (best < float(z high)); a low slab ends at the content's least
    /// value − 0.0001, a high slab starts at its greatest + 0.0001 (the double added in the
    /// register, the sum stored as float). The node (kind 1, 2, 3 by axis, level + 1) gets
    /// the empty side with no list and the other side with the whole list.
    ///
    /// When nothing is cut off the cell is halved across its longest side (the rounded
    /// extents: x if dy ≤ dx and dx ≥ dz, else y if dy ≥ dx and dy ≥ dz, else z), by
    /// [`BspBuild::split`]. Class: model.
    pub fn bsptree(&mut self, list: Option<usize>, count: i32, bound: &[f32; 6], anc: &[Option<usize>; 6], level: i32) -> usize {
        let p = self.params;
        let dx = x(bound[1]) - bound[0];
        let dy = x(bound[3]) - bound[2];
        let dz = x(bound[5]) - bound[4];
        let (dxf, dyf, dzf) = (dx.to_f32(), dy.to_f32(), dz.to_f32());
        let volume = dx * dy * dz;
        if level > 0x7ff8 || count < p.min_count as i32 || level > p.max_level as i32 || volume.lt(x(p.cell_volume)) {
            self.stats.leaves += 1;
            self.bsp.nodes.push(Tree { kind: 0, level: (level + 1) as i16, list, walls: *anc, ..Tree::default() });
            return self.bsp.nodes.len() - 1;
        }
        let mut content = p.empty_box;
        let mut cur = list;
        while let Some(l) = cur {
            let b = self.bsp.links[l].bound;
            for i in [0, 2, 4] {
                if x(b[i]).lt(x(content[i])) {
                    content[i] = b[i];
                }
            }
            for i in [1, 3, 5] {
                if x(b[i]).gt(x(content[i])) {
                    content[i] = b[i];
                }
            }
            cur = self.bsp.links[l].next;
        }
        let below = |axis: usize, d: f32| (x(content[2 * axis]) - bound[2 * axis]) / d;
        let above = |axis: usize, d: f32| (x(bound[2 * axis + 1]) - content[2 * axis + 1]) / d;
        let (x_lo, y_lo, z_lo) = (below(0, dxf), below(1, dyf), below(2, dzf));
        let (x_hi, y_hi, z_hi) = (above(0, dxf), above(1, dyf), above(2, dzf));
        let low_edge = |axis: usize| (x(content[2 * axis]) - EPS).to_f32();
        let high_edge = |axis: usize| (x(EPS) + content[2 * axis + 1]).to_f32();
        let all = List { head: list, count };
        let none = List::default();
        // (axis, split position, low side's list, high side's list)
        let mut cut: Option<(usize, f32, List, List)> = None;
        let mut best = x(p.empty_fraction);
        if best.lt(x_lo) {
            cut = Some((0, low_edge(0), none, all));
            best = x_lo;
        }
        if x_hi.gt(best) {
            cut = Some((0, high_edge(0), all, none));
            best = x_hi;
        }
        if y_lo.gt(best) {
            cut = Some((1, low_edge(1), none, all));
            best = y_lo;
        }
        if y_hi.gt(best) {
            cut = Some((1, high_edge(1), all, none));
            best = y_hi;
        }
        if z_lo.gt(best) {
            cut = Some((2, low_edge(2), none, all));
            best = z_lo;
        }
        if best.lt(x(z_hi.to_f32())) {
            cut = Some((2, high_edge(2), all, none));
        }
        match cut {
            Some((axis, at, low, high)) => self.node(axis, at, low, high, bound, anc, level),
            None => {
                let (fx, fy, fz) = (x(dxf), x(dyf), x(dzf));
                let axis = if fy.le(fx) && fx.ge(fz) {
                    0
                } else if fy.ge(fx) && fy.ge(fz) {
                    1
                } else {
                    2
                };
                self.split(axis, list, bound, anc, level)
            }
        }
    }

    /// The inner node and its two subtrees, as both `si_bsptree` and the three split
    /// routines make them: the node first (kind = axis + 1, level + 1, the split position),
    /// then the low side (the bound's greatest value on the axis replaced by the split
    /// position, the node as that side's high wall), then the high side (the least value
    /// replaced, the node as its low wall); mailbox 0.
    fn node(&mut self, axis: usize, at: f32, low: List, high: List, bound: &[f32; 6], anc: &[Option<usize>; 6], level: i32) -> usize {
        self.stats.nodes += 1;
        let node = self.bsp.nodes.len();
        self.bsp.nodes.push(Tree { kind: axis as i16 + 1, level: (level + 1) as i16, split: at, ..Tree::default() });
        let (mut bound_low, mut bound_high) = (*bound, *bound);
        bound_low[2 * axis + 1] = at;
        bound_high[2 * axis] = at;
        let (mut anc_low, mut anc_high) = (*anc, *anc);
        anc_low[axis + 3] = Some(node);
        anc_high[axis] = Some(node);
        let l = self.bsptree(low.head, low.count, &bound_low, &anc_low, level + 1);
        self.bsp.nodes[node].low = Some(l);
        let h = self.bsptree(high.head, high.count, &bound_high, &anc_high, level + 1);
        self.bsp.nodes[node].high = Some(h);
        node
    }

    /// Halving a cell across one axis [soft 0x08270090 xSplit] [soft 0x08270c70 ySplit]
    /// [soft 0x08271860 zSplit]: `xSplit(list, &bound, &ancestors, level)`; `axis` is 0, 1,
    /// 2 for the three routines, which are one source text compiled three times.
    ///
    /// The plane is at m = double(0.5 × (least + greatest)) of the bound on the axis u.
    /// Every entry of the list goes to the head of the low list (m above the entry's
    /// greatest u), of the high list (m below its least u), or is looked at closely
    /// ([`BspBuild::side`]): it then goes to one of the two, or to both, in which case the
    /// entry itself stays on the low side with its greatest u set to float(m) and a copy
    /// (a new record, counted in the list counter) goes to the high side with its least u
    /// set to float(m). Both lists come out in reverse order. Then the node, with split
    /// position float(m), and its subtrees ([`BspBuild::node`]).
    ///
    /// The three compilations differ in which intermediate values the compiler kept in
    /// the register and which it reloaded rounded to double; [`P_EXACT`] has the
    /// differences. Class: model.
    pub fn split(&mut self, axis: usize, list: Option<usize>, bound: &[f32; 6], anc: &[Option<usize>; 6], level: i32) -> usize {
        let (lo, hi) = (2 * axis, 2 * axis + 1);
        let mid = (x(HALF) * (x(bound[hi]) + bound[lo])).to_f64();
        let midf = x(mid).to_f32();
        let (mut low, mut high) = (List::default(), List::default());
        let mut cur = list;
        while let Some(l) = cur {
            cur = self.bsp.links[l].next;
            let b = self.bsp.links[l].bound;
            let side = if x(mid).gt(x(b[hi])) {
                Side::Low
            } else if x(mid).lt(x(b[lo])) {
                Side::High
            } else {
                self.side(axis, mid, l, bound)
            };
            match side {
                Side::Low => self.put(&mut low, l),
                Side::High => self.put(&mut high, l),
                Side::Both => {
                    self.put(&mut low, l);
                    self.stats.links += 1;
                    let mut copy = self.bsp.links[l];
                    copy.bound[lo] = midf;
                    self.bsp.links[l].bound[hi] = midf;
                    let n = self.bsp.links.len();
                    self.bsp.links.push(copy);
                    self.put(&mut high, n);
                }
            }
        }
        self.node(axis, midf, low, high, bound, anc, level)
    }

    /// Which side of the plane the part of a triangle inside the cell lies on (the long
    /// middle of the three split routines).
    ///
    /// With u the split axis and (v, w) the other two in the order the routine tests them
    /// (x: z, y; y: x, z; z: x, y), and a, b, c the triangle's corners: the corner alone on
    /// its side of the plane is o, the others f and s: (c; a, b) when a and b are on one
    /// side ("below m" against "not below m", so a corner on the plane or a NaN counts as
    /// above), else (a; b, c) when a and c are on different sides too, else (b; a, c).
    ///
    /// The two edges from o cross the plane at P (towards f) and Q (towards s):
    /// P = o + ((m − o.u) / (f.u − o.u)) × (f − o) in v and w, or o itself when |f.u − o.u| <
    /// 0.0001; Q likewise with s. For P the divisor is the unrounded difference; for Q it,
    /// and every other difference, is rounded to double first; each coordinate is rounded
    /// to double. ([`P_EXACT`]: where the compiler kept P's other difference unrounded.)
    ///
    /// Then, for v and after it w: if the cell's least value on that axis is above both P
    /// and Q there, or its greatest below both, the segment P–Q passes outside the cell and
    /// the triangle can reach into the cell on one side of the plane only: the edge that
    /// crosses that face of the cell is o–f when the face's coordinate separates o from f,
    /// else o–s (stated on s: the edge that is not o–f), and where it crosses, at
    /// X = ((face − e.k) / (e.k − o.k)) × (e.u − o.u) + e.u with e the edge's far corner,
    /// decides: low when m > double(X), else high. When no face separates, the triangle is
    /// on both sides.
    fn side(&self, axis: usize, mid: f64, link: usize, bound: &[f32; 6]) -> Side {
        let tn = &self.tns[self.bsp.links[link].tn];
        // a null corner is dereferenced by the original
        let corner = |k: usize| {
            let w = self.verts[tn.v[k].expect("a listed triangle has three corners")].world;
            [w.x, w.y, w.z]
        };
        let (a, b, c) = (corner(0), corner(1), corner(2));
        let u = axis;
        let others = [[2, 1], [0, 2], [0, 1]][axis];
        let m = x(mid);
        let a_not_below = !x(a[u]).lt(m);
        let (o, f, s, alone) = if a_not_below == x(b[u]).lt(m) {
            if a_not_below == x(c[u]).lt(m) {
                (a, b, c, 0)
            } else {
                (b, a, c, 1)
            }
        } else {
            (c, a, b, 2)
        };
        let r64 = |v: F80| x(v.to_f64());
        // P, towards f
        let du_f = x(f[u]) - o[u];
        let mut p = [x(0.0f32); 3];
        if du_f.abs().lt(x(EPS)) {
            for k in others {
                p[k] = x(o[k]);
            }
        } else {
            let t = (m - o[u]) / du_f;
            for k in others {
                let d = x(f[k]) - o[k];
                p[k] = if alone == 0 && P_EXACT[axis][k] { r64(d * t + o[k]) } else { r64(t * r64(d) + o[k]) };
            }
        }
        // Q, towards s
        let du_s = r64(x(s[u]) - o[u]);
        let mut q = [x(0.0f32); 3];
        if du_s.abs().lt(x(EPS)) {
            for k in others {
                q[k] = x(o[k]);
            }
        } else {
            let t = (m - o[u]) / du_s;
            for k in others {
                q[k] = r64(t * r64(x(s[k]) - o[k]) + o[k]);
            }
        }
        for k in others {
            for upper in [false, true] {
                let face = x(bound[2 * k + upper as usize]);
                let beyond = |v: F80| if upper { face.lt(v) } else { face.gt(v) };
                if !(beyond(p[k]) && beyond(q[k])) {
                    continue;
                }
                let e = if beyond(x(o[k])) != beyond(x(f[k])) { f } else { s };
                let at = r64(((face - e[k]) / r64(x(e[k]) - o[k])) * r64(x(e[u]) - o[u]) + e[u]);
                return if m.gt(at) { Side::Low } else { Side::High };
            }
        }
        Side::Both
    }
}

/// Where P's coordinate on axis k is formed as (f.k − o.k) × t + o.k with the difference
/// unrounded, instead of t × double(f.k − o.k) + o.k: only when corner a is the one alone,
/// and by routine (row: split axis) and coordinate (column) as read from each listing:
/// `xSplit` both (z, y), `ySplit` z only, `zSplit` both (x, y).
pub const P_EXACT: [[bool; 3]; 3] = [[false, true, true], [false, false, true], [true, true, false]];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    Low,
    High,
    Both,
}

/// The tree of a scene as `SI_update` builds it: the ancestry around `scene` (the box
/// `traceList` gathered), then `si_bsptree(list, count, &scene, &ancestors, 2)`; the root
/// goes to 0x08790928. `links` are the trace records in the order `traceList` made them;
/// the list handed on starts at the last (each record was put at the head).
pub fn build(bsp: &mut Bsp, links: &[Link], scene: &[f32; 6], tns: &[Tn], verts: &[RVertex], params: BspParams, stats: &mut BspStats) {
    let anc = init_ancestry(bsp, scene, stats);
    let base = bsp.links.len();
    for (i, l) in links.iter().enumerate() {
        bsp.links.push(Link { next: if i == 0 { None } else { Some(base + i - 1) }, ..*l });
    }
    let head = if links.is_empty() { None } else { Some(base + links.len() - 1) };
    let mut b = BspBuild { bsp, tns, verts, params, stats };
    let root = b.bsptree(head, links.len() as i32, scene, &anc, 2);
    bsp.root = root;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Vector;

    fn tri(verts: &mut Vec<RVertex>, tns: &mut Vec<Tn>, p: [[f32; 3]; 3]) -> Link {
        let base = verts.len();
        let mut b = [f32::MAX, f32::MIN, f32::MAX, f32::MIN, f32::MAX, f32::MIN];
        for c in p {
            verts.push(RVertex { world: Vector::new(c[0], c[1], c[2]), ..RVertex::default() });
            for k in 0..3 {
                b[2 * k] = b[2 * k].min(c[k]);
                b[2 * k + 1] = b[2 * k + 1].max(c[k]);
            }
        }
        tns.push(Tn { v: [Some(base), Some(base + 1), Some(base + 2)], ..Tn::default() });
        Link { bound: b, tn: tns.len() - 1, next: None }
    }

    #[test]
    fn ancestry_is_seven_nodes_around_the_box() {
        let mut bsp = Bsp::default();
        let mut stats = BspStats::default();
        let anc = init_ancestry(&mut bsp, &[-1.0, 2.0, -3.0, 4.0, -5.0, 6.0], &mut stats);
        assert_eq!(stats.nodes, 7);
        assert_eq!(bsp.outside, Some(0));
        // allocation order x low, x high, y low, y high, z low, z high
        assert_eq!(anc, [Some(1), Some(3), Some(5), Some(2), Some(4), Some(6)]);
        let splits: Vec<f32> = bsp.nodes[1..].iter().map(|n| n.split).collect();
        assert_eq!(splits, [-1.0, 2.0, -3.0, 4.0, -5.0, 6.0]);
        assert!(bsp.nodes[1..].iter().all(|n| n.kind == 4 && n.level == 1 && n.low == Some(0) && n.high == Some(0)));
    }

    #[test]
    fn a_small_list_is_one_cell() {
        let (mut verts, mut tns) = (Vec::new(), Vec::new());
        let links = [tri(&mut verts, &mut tns, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])];
        let mut bsp = Bsp::default();
        let mut stats = BspStats::default();
        let params = BspParams { min_count: 4, max_level: 20, ..BspParams::default() };
        build(&mut bsp, &links, &[0.0, 1.0, 0.0, 1.0, 0.0, 1.0], &tns, &verts, params, &mut stats);
        let root = &bsp.nodes[bsp.root];
        assert_eq!((root.kind, root.level, root.list), (0, 3, Some(0)));
        assert_eq!(stats, BspStats { links: 0, nodes: 7, leaves: 1 });
    }

    #[test]
    fn empty_space_is_cut_off_before_halving() {
        // two triangles in the top tenth of a tall box: the space below them goes first
        let (mut verts, mut tns) = (Vec::new(), Vec::new());
        let links = [
            tri(&mut verts, &mut tns, [[0.0, 9.0, 0.0], [1.0, 9.0, 0.0], [0.0, 10.0, 1.0]]),
            tri(&mut verts, &mut tns, [[0.0, 9.5, 0.0], [1.0, 9.5, 1.0], [1.0, 10.0, 1.0]]),
        ];
        let mut bsp = Bsp::default();
        let mut stats = BspStats::default();
        let params = BspParams { min_count: 2, max_level: 2, ..BspParams::default() };
        build(&mut bsp, &links, &[0.0, 1.0, 0.0, 10.0, 0.0, 1.0], &tns, &verts, params, &mut stats);
        let root = bsp.nodes[bsp.root].clone();
        assert_eq!((root.kind, root.level), (2, 3));
        assert_eq!(root.split, (9.0f64 - 1e-4) as f32);
        let (low, high) = (&bsp.nodes[root.low.unwrap()], &bsp.nodes[root.high.unwrap()]);
        assert_eq!((low.kind, low.list, low.walls[4]), (0, None, Some(bsp.root)));
        assert_eq!((high.kind, high.list, high.walls[1]), (0, Some(1), Some(bsp.root)));
    }

    #[test]
    fn a_triangle_across_the_plane_is_listed_on_both_sides() {
        let (mut verts, mut tns) = (Vec::new(), Vec::new());
        let links = [
            tri(&mut verts, &mut tns, [[0.5, 0.5, 0.5], [3.5, 0.6, 0.5], [0.5, 3.5, 0.7]]),
            tri(&mut verts, &mut tns, [[0.1, 0.2, 0.1], [0.4, 0.2, 0.1], [0.1, 0.4, 0.3]]),
            tri(&mut verts, &mut tns, [[3.1, 3.2, 3.1], [3.4, 3.2, 3.1], [3.1, 3.9, 3.9]]),
        ];
        let mut bsp = Bsp::default();
        let mut stats = BspStats::default();
        let params = BspParams { min_count: 3, max_level: 2, ..BspParams::default() };
        build(&mut bsp, &links, &[0.0, 4.0, 0.0, 4.0, 0.0, 4.0], &tns, &verts, params, &mut stats);
        let root = bsp.nodes[bsp.root].clone();
        assert_eq!((root.kind, root.split), (1, 2.0));
        assert_eq!(stats.links, 1);
        assert_eq!(bsp.links.len(), 4);
        // the first triangle: its own record below the plane, the copy above
        assert_eq!(bsp.links[0].bound[1], 2.0);
        assert_eq!(bsp.links[3].bound[0], 2.0);
        assert_eq!(bsp.links[3].tn, 0);
        let low = &bsp.nodes[root.low.unwrap()];
        let high = &bsp.nodes[root.high.unwrap()];
        // lists come out reversed: the trace list runs 2, 1, 0
        assert_eq!(low.list, Some(0));
        assert_eq!(bsp.links[0].next, Some(1));
        assert_eq!(high.list, Some(3));
        assert_eq!(bsp.links[3].next, Some(2));
    }

    #[test]
    fn a_triangle_whose_crossing_misses_the_cell_goes_to_one_side() {
        // the triangle's box straddles x = 2, but inside the cell (z ≤ 4) it stays below
        let (mut verts, mut tns) = (Vec::new(), Vec::new());
        let links = [tri(&mut verts, &mut tns, [[1.0, 1.0, 1.0], [1.5, 3.0, 5.0], [3.0, 2.0, 9.0]])];
        let mut bsp = Bsp::default();
        let mut stats = BspStats::default();
        let params = BspParams { min_count: 1, max_level: 2, empty_fraction: 2.0, ..BspParams::default() };
        let scene = [0.0, 4.0, 0.0, 4.0, 0.0, 4.0];
        let anc = init_ancestry(&mut bsp, &scene, &mut stats);
        bsp.links.push(links[0]);
        let mut b = BspBuild { bsp: &mut bsp, tns: &tns, verts: &verts, params, stats: &mut stats };
        let root = b.split(0, Some(0), &scene, &anc, 2);
        let n = bsp.nodes[root].clone();
        assert_eq!(bsp.links.len(), 1);
        assert_eq!(bsp.nodes[n.low.unwrap()].list, Some(0));
        assert_eq!(bsp.nodes[n.high.unwrap()].list, None);
    }
}
