// SPDX-License-Identifier: MIT
//! Giving the polygons of a mesh one consistent direction: the original's `order.c`
//! (finding 0015).
//!
//! [`order_mesh`] answers a **new mesh** with the same polygons, turned round where needed
//! so that two polygons sharing an edge run along it in opposite directions, and with
//! vertices duplicated where the surface is not a manifold: at an edge with more than two
//! polygons (or with two that cannot be paired) and at a vertex where two fans of polygons
//! meet only in that point.
//!
//! The original works on file-scope tables (`polTable`, `edgTable`, `vrtTable`,
//! `toDoList`, `polStack` with their counts); here they are the fields of [`Order`], and
//! every routine of the file is a method named after it. Polygons and edges are numbered
//! from 1; **a polygon's number in an edge's list, and an edge's number in a polygon's
//! corner, carry a sign**: positive when the polygon runs along the edge from its first
//! vertex to its second.
//!
//! Everything here is integer work; positions are only copied. Class: integer.

use crate::mesh::{Mesh, PolyNode, Polygon, Vertex};

/// A polygon of `polTable` (0x24 bytes): the material, the corner count, and six arrays of
/// that length (vertex, signed edge, u, v, colour, further texture coordinates), then the
/// "visited" byte.
#[derive(Clone, Debug, Default)]
struct Pol {
    material: i32,
    verts: Vec<i32>,
    edges: Vec<i32>,
    uv: Vec<[f32; 2]>,
    color: Vec<u32>,
    extra: Vec<Vec<[f32; 2]>>,
    done: bool,
}

/// An edge of `edgTable` (16 bytes): two vertices and the list of the polygons along it.
#[derive(Clone, Debug, Default)]
struct Edg {
    v0: i32,
    v1: i32,
    pols: Vec<i32>,
}

/// A vertex of `vrtTable` (24 bytes): position, flag, and a list of polygon pairs.
#[derive(Clone, Debug, Default)]
struct Vrt {
    pos: [f32; 3],
    flag: i16,
    pairs: Vec<[i32; 2]>,
}

/// The tables of `order.c`.
#[derive(Default)]
pub struct Order {
    vrt: Vec<Vrt>,
    /// Record 0 exists and is no edge: the original starts its numbering at 1 and uses
    /// record 0 as a scratch list.
    edg: Vec<Edg>,
    /// Record 0 is unused.
    pol: Vec<Pol>,
    todo: Vec<[i32; 4]>,
    stack: Vec<i32>,
}

impl Order {
    fn edge(&mut self, e: i32) -> &mut Edg {
        let i = e.unsigned_abs() as usize;
        // (an edge number that names no record reads foreign memory in the original)
        if i >= self.edg.len() {
            self.edg.resize(i + 1, Edg { v0: -1, v1: -1, pols: Vec::new() });
        }
        &mut self.edg[i]
    }

    /// A new vertex, its number answered [soft 0x084789c0 createVertex]. (The table grows
    /// 200 records at a time.)
    fn create_vertex(&mut self, pos: [f32; 3], flag: i16) -> i32 {
        self.vrt.push(Vrt { pos, flag, pairs: Vec::new() });
        self.vrt.len() as i32 - 1
    }

    /// A copy of vertex `v` (position and flag) as a new vertex.
    fn copy_vertex(&mut self, v: i32) -> i32 {
        let (pos, flag) = self.vrt.get(v as usize).map_or(([0.0; 3], 0), |t| (t.pos, t.flag));
        self.create_vertex(pos, flag)
    }

    /// A new edge between two vertices, its number answered [soft 0x08478aa4 createEdge].
    /// The first edge is number 1. (The table grows 100 records at a time.)
    fn create_edge(&mut self, v0: i32, v1: i32) -> i32 {
        if self.edg.is_empty() {
            self.edg.push(Edg { v0: -1, v1: -1, pols: Vec::new() });
        }
        self.edg.push(Edg { v0, v1, pols: Vec::new() });
        self.edg.len() as i32 - 1
    }

    /// Appends polygon `p` to the list of edge |`e`| with the sign of `e`
    /// [soft 0x08478b8c addPolygonToEdge]: `p` as it is for a positive `e`, negated
    /// otherwise, **also for edge 0**.
    fn add_polygon_to_edge(&mut self, e: i32, p: i32) {
        let signed = if e > 0 { p } else { p.wrapping_neg() };
        self.edge(e).pols.push(signed);
    }

    /// The edge between two vertices [soft 0x08479154 findEdges]: its number when it runs
    /// from `a` to `b`, the negated number when it runs from `b` to `a`, 0 when there is
    /// none. The first match from edge 1 on.
    fn find_edges(&self, a: i32, b: i32) -> i32 {
        for (i, e) in self.edg.iter().enumerate().skip(1) {
            if e.v0 == a && e.v1 == b {
                return i as i32;
            }
            if e.v0 == b && e.v1 == a {
                return -(i as i32);
            }
        }
        0
    }

    /// Builds the edges of every polygon [soft 0x084791b4 createEdgTable]: per corner the
    /// edge to the next corner of its contour (the contour's first after its last), found
    /// or created, the polygon added to it and its signed number stored in the corner.
    fn create_edg_table(&mut self) {
        for p in 1..self.pol.len() {
            let mut first = 0usize;
            for i in 0..self.pol[p].verts.len() {
                let v = self.pol[p].verts[i];
                if v == -1 {
                    first = i + 1;
                    continue;
                }
                let n = self.pol[p].verts.len();
                let next = if i == n - 1 || self.pol[p].verts[i + 1] == -1 { self.pol[p].verts[first] } else { self.pol[p].verts[i + 1] };
                let mut e = self.find_edges(v, next);
                if e == 0 {
                    e = self.create_edge(v, next);
                }
                self.add_polygon_to_edge(e, p as i32);
                self.pol[p].edges[i] = e;
            }
        }
    }

    /// Turns polygon `p` round [soft 0x084785e0 inversePolygon]: its corner and edge
    /// arrays are reversed contour by contour, every edge number is negated, and the
    /// polygon's entry in each of those edges' lists is negated too.
    ///
    /// As the original has it: a contour that starts at index s and ends before index e is
    /// reversed by swapping k with e − 1 − k for k from s on, which is a reversal only for
    /// the first contour (s = 0); for a hole it swaps into the contours before it. And the
    /// edge array is reversed like the corner array, which leaves each edge one corner
    /// away from the corner it leaves. The second edge table, built after all turning, is
    /// what the result rests on.
    fn inverse_polygon(&mut self, p: i32) {
        let pi = p as usize;
        let n = self.pol[pi].verts.len();
        let mut start = 0usize;
        for i in 0..=n {
            if i == n || self.pol[pi].verts[i] == -1 {
                let mut k = start;
                while k < (i - start) / 2 + start {
                    // (i − k − 1 is below zero only where the original swaps with the word
                    // before its array)
                    if let Some(j) = (i - k).checked_sub(1) {
                        self.pol[pi].verts.swap(j, k);
                        self.pol[pi].edges.swap(j, k);
                    }
                    k += 1;
                }
                if i != n {
                    start = i + 1;
                }
            }
        }
        for i in 0..n {
            if self.pol[pi].verts[i] == -1 {
                continue;
            }
            let e = self.pol[pi].edges[i].wrapping_neg();
            self.pol[pi].edges[i] = e;
            if let Some(x) = self.edge(e).pols.iter_mut().find(|x| x.unsigned_abs() == p as u32) {
                *x = x.wrapping_neg();
            }
        }
    }

    /// Notes an edge whose end must move from one vertex to another
    /// [soft 0x084787cc addToDoList]: (edge, old vertex, new vertex, the polygon already
    /// corrected). (The list grows ten entries at a time.)
    fn add_to_do_list(&mut self, e: i32, old: i32, new: i32, p: i32) {
        self.todo.push([e, old, new, p]);
    }

    /// Moves an end of edge |`e`| from vertex `old` to `new` and, in every polygon along
    /// the edge other than |`except`|, the first corner at `old` likewise
    /// [soft 0x08478720 fixEdge]. Nothing happens when the edge has no end at `old`.
    fn fix_edge(&mut self, e: i32, old: i32, new: i32, except: i32) {
        let ed = self.edge(e);
        if ed.v0 == old {
            ed.v0 = new;
        } else if ed.v1 == old {
            ed.v1 = new;
        } else {
            return;
        }
        let pols = ed.pols.clone();
        for x in pols {
            if x.unsigned_abs() == except.unsigned_abs() {
                continue;
            }
            if let Some(pol) = self.pol.get_mut(x.unsigned_abs() as usize) {
                if let Some(c) = pol.verts.iter_mut().find(|c| **c == old) {
                    *c = new;
                }
            }
        }
    }

    /// Moves polygon `p` from the edge (`a`, `b`) to its copy (`na`, `nb`)
    /// [soft 0x084788a4 fixPolygon]: the corner at `a` becomes `na` with `e` as its edge,
    /// the corner at `b` becomes `nb`, and the two neighbouring edges of the polygon (the
    /// one arriving at `a`, the one leaving `b`) are put on the to-do list so that their
    /// ends follow. It stops after two entries.
    ///
    /// The edge arriving at `a` is the one of the corner before it; when `a` is the first
    /// corner of its contour it is the contour's last, noted when the contour's end (a
    /// separator, or the last corner of the polygon) is reached.
    fn fix_polygon(&mut self, p: i32, e: i32, a: i32, b: i32, na: i32, nb: i32) {
        let pi = p as usize;
        if pi >= self.pol.len() {
            return;
        }
        let mut added = 0;
        let mut before = -1i32;
        let mut wrap = false;
        let n = self.pol[pi].verts.len();
        for i in 0..n {
            let v = self.pol[pi].verts[i];
            let entry: Option<[i32; 3]>;
            if v == -1 {
                if wrap {
                    entry = Some([before, a, na]);
                } else {
                    before = -1;
                    entry = None;
                }
            } else if v == a {
                self.pol[pi].verts[i] = na;
                self.pol[pi].edges[i] = e;
                if before == -1 {
                    wrap = true;
                    entry = None;
                } else {
                    entry = Some([before, a, na]);
                }
            } else if v == b {
                self.pol[pi].verts[i] = nb;
                entry = Some([self.pol[pi].edges[i], b, nb]);
            } else {
                if wrap && i == n - 1 {
                    let last = self.pol[pi].edges[i];
                    self.add_to_do_list(last, a, na, p);
                } else {
                    before = self.pol[pi].edges[i];
                }
                entry = None;
            }
            if let Some([edge, old, new]) = entry {
                self.add_to_do_list(edge, old, new, p);
                added += 1;
                if added == 2 {
                    return;
                }
            }
        }
    }

    /// The polygon to go on with across edge `e` of polygon `p`, signed as the edge's list
    /// has it, or 0 [soft 0x08478c00 associatePolygon].
    ///
    /// An edge with fewer than two polygons has none. Otherwise the edge's list is read:
    /// the first polygon that runs **against** `p` along the edge is taken at once; failing
    /// that, the last polygon running the same way that has not been visited.
    ///
    /// When the edge has more than two polygons, or no partner was found, **the edge is
    /// split**: its two vertices are copied, a new edge joins the copies, and every
    /// polygon of the edge other than `p` and the partner moves to the new edge
    /// ([`Order::fix_polygon`]). Each neighbouring edge noted on the way that the partner
    /// also lies on is itself doubled, so that the partner keeps the old one; then the
    /// noted edges' ends are moved ([`Order::fix_edge`]). The old edge keeps `p` and the
    /// partner.
    ///
    /// (Polygons passed through the scratch list of record 0 come back with their sign
    /// turned, as [`Order::add_polygon_to_edge`] negates for edge 0.)
    fn associate_polygon(&mut self, e: i32, p: i32) -> i32 {
        let ei = e.unsigned_abs() as i32;
        if self.edge(e).pols.len() < 2 {
            return 0;
        }
        let p = if e < 1 { p.wrapping_neg() } else { p };
        let mut other = 0i32;
        for &x in &self.edge(e).pols.clone() {
            if x == p {
                continue;
            }
            if (x > 0) != (e > 0) {
                other = x;
                break;
            }
            if !self.pol.get(x.unsigned_abs() as usize).is_some_and(|q| q.done) {
                other = x;
            }
        }
        if self.edge(e).pols.len() <= 2 && other != 0 {
            return other;
        }
        let (v0, v1) = (self.edge(e).v0, self.edge(e).v1);
        let (n0, n1) = (self.copy_vertex(v0), self.copy_vertex(v1));
        let ne = self.create_edge(n0, n1);
        self.todo.clear();
        let mut i = 0;
        while i < self.edge(e).pols.len() {
            let x = self.edge(e).pols[i];
            if x != p && x != other {
                let se = if x < 1 { -ne } else { ne };
                self.add_polygon_to_edge(ne, x);
                let (a, b) = (self.edge(e).v0, self.edge(e).v1);
                let x = self.edge(e).pols[i].unsigned_abs() as i32;
                if se < 1 {
                    self.fix_polygon(x, se, b, a, n1, n0);
                } else {
                    self.fix_polygon(x, se, a, b, n0, n1);
                }
            }
            i += 1;
        }
        for t in 0..self.todo.len() {
            let te = self.todo[t][0];
            let Some(j) = self.edge(te).pols.iter().position(|x| x.unsigned_abs() == other.unsigned_abs()) else { continue };
            let (a, b) = (self.edge(te).v0, self.edge(te).v1);
            let copy = self.create_edge(a, b);
            let x = self.edge(te).pols[j];
            self.add_polygon_to_edge(copy, x);
            if let Some(pol) = self.pol.get_mut(x.unsigned_abs() as usize) {
                for c in 0..pol.verts.len() {
                    if pol.verts[c] != -1 && pol.edges[c].unsigned_abs() == te.unsigned_abs() {
                        pol.edges[c] = if x < 1 { -copy } else { copy };
                    }
                }
            }
            // the others go through the scratch list of record 0 and back
            for x in self.edge(te).pols.clone() {
                if x.unsigned_abs() != other.unsigned_abs() {
                    self.add_polygon_to_edge(0, x);
                }
            }
            self.edge(te).pols.clear();
            for x in self.edge(0).pols.clone() {
                self.add_polygon_to_edge(te.unsigned_abs() as i32, x);
            }
            self.edge(0).pols.clear();
        }
        for t in core::mem::take(&mut self.todo) {
            self.fix_edge(t[0], t[1], t[2], t[3]);
        }
        self.edge(e).pols.clear();
        self.add_polygon_to_edge(ei, p);
        if other != 0 {
            self.add_polygon_to_edge(ei, other);
        }
        other
    }

    /// Visits polygon `p` and, through each of its edges, every polygon reached from it
    /// [soft 0x084790ac processPolygon]: the partner across an edge
    /// ([`Order::associate_polygon`]) that has not been visited is turned round when it
    /// runs the same way along the edge, and visited in turn, depth first.
    ///
    /// The original recurses once per polygon; this walk keeps its own stack.
    fn process_polygon(&mut self, p: i32) {
        let mut stack: Vec<(i32, usize)> = Vec::new();
        self.pol[p as usize].done = true;
        stack.push((p, 0));
        while let Some(&(q, i)) = stack.last() {
            let qi = q as usize;
            if i >= self.pol[qi].verts.len() {
                stack.pop();
                continue;
            }
            stack.last_mut().unwrap().1 += 1;
            if self.pol[qi].verts[i] == -1 {
                continue;
            }
            let e = self.pol[qi].edges[i];
            let other = self.associate_polygon(e, q);
            if other == 0 {
                continue;
            }
            let o = other.unsigned_abs() as i32;
            if self.pol.get(o as usize).is_none_or(|x| x.done) {
                continue;
            }
            // the corner's edge is read again: the splitting may have renumbered it
            if (other > 0) != (self.pol[qi].edges[i] < 1) {
                self.inverse_polygon(o);
            }
            self.pol[o as usize].done = true;
            stack.push((o, 0));
        }
    }

    /// Puts a polygon on the stack [soft 0x084792d4 addPolygonToStack].
    fn add_polygon_to_stack(&mut self, p: i32) {
        self.stack.push(p);
    }

    /// Empties the stack [soft 0x0847936c clearPolygonStack].
    fn clear_polygon_stack(&mut self) {
        self.stack.clear();
    }

    /// Whether the stack is empty [soft 0x084793e8 polygonStackEmpty].
    fn polygon_stack_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// In every polygon on the stack, the first corner at vertex `old` becomes `new`
    /// [soft 0x0847939c processPolygonStack].
    fn process_polygon_stack(&mut self, old: i32, new: i32) {
        for k in 0..self.stack.len() {
            let Some(pol) = self.pol.get_mut(self.stack[k] as usize) else { continue };
            if let Some(c) = pol.verts.iter_mut().find(|c| **c == old) {
                *c = new;
            }
        }
    }

    /// Notes at vertex `v` that polygon `a` is followed by polygon `b` round it
    /// [soft 0x084793fc addVrtPair]; 0 stands for "no polygon" (a border).
    fn add_vrt_pair(&mut self, v: i32, a: i32, b: i32) {
        if let Some(t) = self.vrt.get_mut(v as usize) {
            t.pairs.push([a, b]);
        }
    }

    /// Splits vertex `v` where more than one fan of polygons meets in it (the second half
    /// of `SI_orderMesh`). The pairs noted at the vertex are followed from one polygon to
    /// the next; the first chain keeps the vertex, every further chain gets a copy of it,
    /// written into the chain's polygons.
    fn split_fans(&mut self, v: usize) {
        let mut first = true;
        loop {
            let (mut start, mut cur) = (0i32, 0i32);
            for k in 0..self.vrt[v].pairs.len() {
                let [a, b] = self.vrt[v].pairs[k];
                if a != 0 {
                    cur = b;
                    self.vrt[v].pairs[k] = [0, 0];
                    start = a;
                    if !first && cur != 0 {
                        self.add_polygon_to_stack(cur);
                    }
                    break;
                }
            }
            if start == 0 || cur == 0 {
                break;
            }
            let mut k = 0usize;
            while k < self.vrt[v].pairs.len() {
                if self.vrt[v].pairs[k][0] == cur {
                    let next = self.vrt[v].pairs[k][1];
                    if !first && next != 0 {
                        self.add_polygon_to_stack(next);
                    }
                    cur = next;
                    self.vrt[v].pairs[k] = [0, 0];
                    if start == cur {
                        if !first {
                            let nv = self.copy_vertex(v as i32);
                            self.process_polygon_stack(v as i32, nv);
                            self.clear_polygon_stack();
                        }
                        break;
                    }
                    if cur == 0 {
                        break;
                    }
                    k = 0;
                    continue;
                }
                k += 1;
            }
            first = false;
            if !self.polygon_stack_empty() {
                let nv = self.copy_vertex(v as i32);
                self.add_polygon_to_stack(start);
                self.process_polygon_stack(v as i32, nv);
                self.clear_polygon_stack();
            }
        }
        self.vrt[v].pairs.clear();
        self.clear_polygon_stack();
    }
}

/// A copy of a mesh with its polygons turned one consistent way and its vertices doubled
/// where the surface is not a manifold [soft 0x08479494 SI_orderMesh].
///
/// 1. Every polygon is taken into a table (corners, texture coordinates, colours, the
///    further coordinate sets, the material) and every vertex with its flag; the edges
///    are built ([`Order::create_edg_table`]).
/// 2. From each polygon not yet visited, in order, the polygons reached through shared
///    edges are visited depth first and turned round where they run the same way along
///    the shared edge as the polygon they were reached from; edges with more than two
///    polygons are split on the way ([`Order::process_polygon`]). **The first polygon of
///    each connected piece decides the direction of the piece.**
/// 3. The edges are thrown away and built again from the corners as they now are.
/// 4. For every edge with one polygon, or with two that run against each other, each of
///    its two vertices notes which polygon follows which round it; an edge whose two
///    polygons still run the same way notes nothing.
/// 5. Every vertex is split into one vertex per chain of polygons round it
///    ([`Order::split_fans`]); vertices made on the way are looked at too.
/// 6. The new mesh: the vertices (position and flag) and the polygons with, per corner,
///    the vertex, the first texture coordinates, the colour and the further coordinate
///    sets. **Normals, edges, stars, polygon flags are not carried and nothing is
///    rebuilt**: the caller does that. The mesh's own settings are a new mesh's.
pub fn order_mesh(mesh: &Mesh) -> Mesh {
    let mut o = Order::default();
    o.pol.push(Pol::default());
    for p in &mesh.polygons {
        o.pol.push(Pol {
            material: p.material as i32,
            verts: p.nodes.iter().map(|c| c.vertex).collect(),
            edges: vec![0; p.nodes.len()],
            uv: p.nodes.iter().map(|c| c.uv).collect(),
            color: p.nodes.iter().map(|c| c.color).collect(),
            extra: p.nodes.iter().map(|c| c.extra_uv.clone()).collect(),
            done: false,
        });
    }
    for v in &mesh.vertices {
        o.create_vertex(v.pos, v.flag);
    }
    o.create_edg_table();
    for p in 1..o.pol.len() {
        if !o.pol[p].done {
            o.process_polygon(p as i32);
        }
    }
    o.edg.clear();
    o.create_edg_table();
    for e in 1..o.edg.len() {
        let (v0, v1) = (o.edg[e].v0, o.edg[e].v1);
        let pols = o.edg[e].pols.clone();
        match pols[..] {
            [a, b] if a >= 1 && b < 0 => {
                o.add_vrt_pair(v0, a, -b);
                o.add_vrt_pair(v1, -b, a);
            }
            [a, b] if b >= 1 && a < 0 => {
                o.add_vrt_pair(v0, b, -a);
                o.add_vrt_pair(v1, -a, b);
            }
            [a] if a < 1 => {
                o.add_vrt_pair(v0, 0, -a);
                o.add_vrt_pair(v1, -a, 0);
            }
            [a] => {
                o.add_vrt_pair(v0, a, 0);
                o.add_vrt_pair(v1, 0, a);
            }
            _ => {}
        }
    }
    let mut v = 0;
    while v < o.vrt.len() {
        o.split_fans(v);
        v += 1;
    }
    let mut out = Mesh::default();
    out.vertices = o.vrt.iter().map(|t| Vertex { pos: t.pos, star: Vec::new(), flag: t.flag }).collect();
    for p in o.pol.iter().skip(1) {
        let nodes = (0..p.verts.len())
            .map(|i| PolyNode { vertex: p.verts[i], edge: 0, normal: [0.0; 3], uv: p.uv[i], color: p.color[i], extra_uv: p.extra[i].clone() })
            .collect();
        out.polygons.push(Polygon { normal: [0.0; 3], nodes, material: p.material as i16, flag: 0 });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_triangles_wound_against_each_other_are_made_to_agree() {
        let mut m = Mesh::with_vertices(&[[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
        m.push_polygon(&[0, 1, 2]);
        m.push_polygon(&[0, 2, 3]); // runs 0 → 2 where the first runs 2 → 0: they agree
        let o = order_mesh(&m);
        let corners = |m: &Mesh| m.polygons.iter().map(|p| p.nodes.iter().map(|c| c.vertex).collect::<Vec<_>>()).collect::<Vec<_>>();
        assert_eq!(corners(&o), [[0, 1, 2], [0, 2, 3]]);
        assert_eq!(o.nb_vertices(), 4);
        // the second one the other way round: it is turned, the first decides
        let mut m2 = Mesh::with_vertices(&[[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
        m2.push_polygon(&[0, 1, 2]);
        m2.push_polygon(&[0, 3, 2]);
        let o2 = order_mesh(&m2);
        assert_eq!(corners(&o2), [[0, 1, 2], [2, 3, 0]]);
    }

    #[test]
    fn a_third_polygon_on_an_edge_gets_vertices_of_its_own() {
        // three triangles on the edge 0-1
        let mut m = Mesh::with_vertices(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]]);
        m.push_polygon(&[0, 1, 2]);
        m.push_polygon(&[1, 0, 3]);
        m.push_polygon(&[1, 0, 4]);
        let o = order_mesh(&m);
        assert_eq!(o.nb_vertices(), 7);
        assert_eq!((o.vertices[5].pos, o.vertices[6].pos), ([0.0; 3], [1.0, 0.0, 0.0]));
        // two lone triangles that share one vertex are left on it: a chain that starts
        // at a border is not followed
        let mut b = Mesh::with_vertices(&[[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [-1.0, -1.0, 0.0]]);
        b.push_polygon(&[0, 1, 2]);
        b.push_polygon(&[0, 3, 4]);
        assert_eq!(order_mesh(&b).nb_vertices(), 5);
    }
}
