// SPDX-License-Identifier: MIT
//! The mesh-editing support of `meshEditFct.c` on the image (finding 0015, 2026-10-28 pass,
//! package 18): the array growers, the corner arithmetic, the edge flags, removing a
//! polygon, adding a vertex on an edge. Everything works on the mesh record at its address
//! (the layout of [`super::off`]) and makes the original's calls to `SI_calloc`,
//! `SI_realloc` and `free`, in its order. Float code of the x87 unit: see each routine.

use si_math::algebra::vector_cos_angle;
use si_math::types::Vector;
use si_mem::Mem;
use si_x87::{trig, x, Round};

use super::obj::{nodes_copy, nodes_dispose};
use super::off as mo;
use super::rebuild;
use crate::model_image::Outside;

fn edge_rec(m: &Mem, mesh: u32, e: i32) -> u32 {
    super::edge_record(m, mesh, e)
}

fn vtx(m: &Mem, mesh: u32, v: i32) -> u32 {
    m.u32(mesh + mo::VERTICES).wrapping_add((v as u32).wrapping_mul(0x14))
}

fn node_at(m: &Mem, poly: u32, k: i32) -> u32 {
    m.u32(poly + mo::POLYGON_NODES).wrapping_add((k as u32).wrapping_mul(0x24))
}

/// Round a count up to the next multiple of `mask + 1` the way the original does:
/// `n - (((n - 1) & mask) - mask)`.
fn round_up(n: i32, mask: i32) -> i32 {
    n.wrapping_sub((n.wrapping_sub(1) & mask).wrapping_sub(mask))
}

/// [soft 0x0844c640 compareIndices]: the comparison `qsort` is given for index arrays:
/// −1 when the second integer is below the first, else 1 (descending order, never 0).
pub fn compare_indices(m: &Mem, a: u32, b: u32) -> i32 {
    if m.i32(b) < m.i32(a) {
        -1
    } else {
        1
    }
}

/// [soft 0x0844c660 setEdgeFlag]: for every edge 1..=count, flag 0x20 (border) when it has
/// no polygon on one side (and 0x10 cleared); otherwise 0x20 cleared and 0x10
/// (discontinuity) set when the cosine of the angle between the two polygons' normals is
/// at most the mesh's cosine (ordered compare: a NaN clears it), cleared otherwise.
pub fn set_edge_flag(m: &mut Mem, mesh: u32) {
    let mut e = 1i32;
    if 0 < m.i32(mesh + mo::NB_EDGES) {
        loop {
            let r = edge_rec(m, mesh, e);
            let fl = r + mo::EDGE_FLAG;
            if m.i32(r + 8) == -1 || m.i32(r + 0xc) == -1 {
                m.set_u16(fl, (m.u16(fl) & 0xffef) | 0x20);
            } else {
                m.set_u8(fl, m.u8(fl) & 0xdf);
                let pa = super::polygon_at(m, mesh, m.i32(r + 8));
                let pb = super::polygon_at(m, mesh, m.i32(r + 0xc));
                let v = |a: u32| Vector { x: m.f32(a), y: m.f32(a + 4), z: m.f32(a + 8) };
                let c = vector_cos_angle(&v(pa), &v(pb));
                let ang = m.f32(mesh + mo::ANGLE_COS);
                if c <= ang {
                    m.set_u8(fl, m.u8(fl) | 0x10);
                } else {
                    m.set_u8(fl, m.u8(fl) & 0xef);
                }
            }
            e += 1;
            if e > m.i32(mesh + mo::NB_EDGES) {
                break;
            }
        }
    }
}

/// [soft 0x0844c6fc si_vertexReallocate]: room for `n` more vertices. When the array is
/// full to a multiple of 32 (`-count & 31 <= n`): a null array is made with
/// `SI_calloc(n` rounded up to 32`, 0x14)` and the count set to 0; otherwise
/// `SI_realloc` to `count + n` rounded up to 32 records.
pub fn vertex_reallocate(m: &mut Mem, mesh: u32, n: i16) {
    let n = n as i32;
    let count = m.i32(mesh + mo::NB_VERTICES);
    if (count.wrapping_neg() & 0x1f) <= n {
        if m.u32(mesh) == 0 {
            let p = m.si_calloc(round_up(n, 0x1f) as u32, 0x14);
            m.set_u32(mesh, p);
            m.set_i32(mesh + mo::NB_VERTICES, 0);
        } else {
            let t = n.wrapping_add(count);
            let p = m.si_realloc(m.u32(mesh), (round_up(t, 0x1f) as u32).wrapping_mul(0x14));
            m.set_u32(mesh, p);
        }
    }
}

/// [soft 0x0844c76c edgeReallocate]: room for `n` more edges: while the table of 1024-record
/// blocks does not reach edge `count + n` (record 0 is the unused one), the table grows by a
/// pointer (`SI_calloc(1, 4)` when null, else `SI_realloc`) and a block of
/// `SI_calloc(1024, 0x14)` is made for it.
pub fn edge_reallocate(m: &mut Mem, mesh: u32, n: i16) {
    let n = n as i32;
    let u = m.i32(mesh + mo::NB_EDGES);
    let mut k = u.wrapping_sub(u | -0x400i32);
    if k < u.wrapping_add(n).wrapping_add(1) {
        loop {
            if m.u32(mesh + mo::EDGES) == 0 {
                let t = m.si_calloc(1, 4);
                m.set_u32(mesh + mo::EDGES, t);
            } else {
                let t = m.si_realloc(m.u32(mesh + mo::EDGES), ((k >> 10) as u32).wrapping_mul(4).wrapping_add(4));
                m.set_u32(mesh + mo::EDGES, t);
            }
            let b = m.si_calloc(0x400, 0x14);
            m.set_u32(m.u32(mesh + mo::EDGES).wrapping_add(((k >> 10) as u32).wrapping_mul(4)), b);
            k = k.wrapping_add(0x400);
            if !(k < n.wrapping_add(1).wrapping_add(m.i32(mesh + mo::NB_EDGES))) {
                break;
            }
        }
    }
}

/// A fresh corner record as `SI_polygonNodeAllocate` leaves it, with the further-pairs
/// pointer (+0x20) `extra`.
fn init_node(m: &mut Mem, at: u32, extra: u32) {
    m.set_u32(at, 0xffff_ffff);
    m.set_u32(at + 4, 0);
    m.set_u32(at + 0x18, 0);
    m.set_u32(at + 0x14, 0);
    m.set_u32(at + 0x1c, 0xffb2_b2b2);
    m.set_u32(at + 0x10, 0);
    m.set_u32(at + 0xc, 0);
    m.set_u32(at + 8, 0);
    m.set_u32(at + 0x20, extra);
}

/// [soft 0x0844c808 nodeReallocate]: room for `n` more corners in a polygon. When the array
/// is full to a multiple of 4 (`-count & 3 <= n`): a null array is `SI_polygonNodeAllocate(n)`;
/// otherwise `SI_realloc` to `count + n` rounded up to 4 corners, and the new corners (from
/// the old count to the new capacity) filled as `SI_polygonNodeAllocate` fills them.
pub fn node_reallocate(m: &mut Mem, poly: u32, n: i16) {
    let n = n as i32;
    let count = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
    let cap = round_up(n.wrapping_add(count), 3);
    if (count.wrapping_neg() & 3) <= n {
        if m.u32(poly + mo::POLYGON_NODES) == 0 {
            let p = si_hrc::image::geom::polygon_node_allocate(m, n);
            m.set_u32(poly + mo::POLYGON_NODES, p);
        } else {
            let p = m.si_realloc(m.u32(poly + mo::POLYGON_NODES), (cap as u32).wrapping_mul(0x24));
            m.set_u32(poly + mo::POLYGON_NODES, p);
            let have = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
            if have < cap {
                let mut off = (have as u32).wrapping_mul(0x24);
                let mut left = cap - have;
                loop {
                    let at = m.u32(poly + mo::POLYGON_NODES).wrapping_add(off);
                    init_node(m, at, 0);
                    off = off.wrapping_add(0x24);
                    left -= 1;
                    if left == 0 {
                        break;
                    }
                }
            }
        }
    }
}

/// [soft 0x0844c8e8 neighborReallocate]: room for `n` more star entries (12 bytes) at a
/// vertex: when `-count & 3 <= n`, `SI_calloc` (null star) or `SI_realloc` (to `count + n`
/// rounded up to 4 entries); the result is stored (even when the call failed).
pub fn neighbor_reallocate(m: &mut Mem, v: u32, n: i16) {
    let n = n as i32;
    let count = m.u16(v + 0x10) as i32;
    if (count.wrapping_neg() & 3) <= n {
        let p = if m.u32(v + 0xc) == 0 {
            m.si_calloc(round_up(n, 3) as u32, 0xc)
        } else {
            let t = n.wrapping_add(count);
            m.si_realloc(m.u32(v + 0xc), (round_up(t, 3) as u32).wrapping_mul(0xc))
        };
        m.set_u32(v + 0xc, p);
    }
}

/// [soft 0x08453538 SI_nodeSetUV]: texture pair `d` of a corner: the pair at +0x14 for
/// `d < 1` or a corner without further pairs, else entry `d − 1` of the array at +0x20.
/// Answers 0 for a null corner, else 1.
pub fn node_set_uv(m: &mut Mem, d: i32, u: f32, v: f32, node: u32) -> i32 {
    if node == 0 {
        return 0;
    }
    if d < 1 || m.u32(node + 0x20) == 0 {
        m.set_f32(node + 0x14, u);
        m.set_f32(node + 0x18, v);
    } else {
        let a = m.u32(node + 0x20).wrapping_add((d as u32).wrapping_mul(8)).wrapping_sub(8);
        m.set_f32(a, u);
        m.set_f32(a.wrapping_add(4), v);
    }
    1
}

/// The pair `d` of a corner as `SI_nodeGetUV` reads it (ported in `si-render`; the same two
/// reads on the image).
fn node_get_uv(m: &Mem, node: u32, d: i32) -> (f32, f32) {
    if d < 1 || m.u32(node + 0x20) == 0 {
        (m.f32(node + 0x14), m.f32(node + 0x18))
    } else {
        let a = m.u32(node + 0x20).wrapping_add((d as u32).wrapping_mul(8)).wrapping_sub(8);
        (m.f32(a), m.f32(a.wrapping_add(4)))
    }
}

/// [soft 0x0844c954 computeNodeAttributes]: corner `dst` of the array `nodes` as the point
/// `t` of the way from corner `a` to corner `b`: texture pair `(b − a) t + a` (in the
/// register, rounded when stored), and, when `b` has further pairs, the same for pairs 1..7
/// (`SI_calloc(14, 4)` for `dst` first when it has none); the four colour bytes
/// `a + (b − a) t` truncated; the normal: x as `(b − a) t + a` (the product kept in the
/// register), y and z with the product rounded to `f32` first, then added to `a`.
pub fn compute_node_attributes(m: &mut Mem, t: f32, nodes: u32, dst: i32, a: i32, b: i32) {
    let at = |k: i32| nodes.wrapping_add((k as u32).wrapping_mul(0x24));
    let (nd, na, nb) = (at(dst), at(a), at(b));
    let tt = x(t);
    let lerp = |p: f32, q: f32| ((x(q) - x(p)) * tt + x(p)).to_f32();
    m.set_f32(nd + 0x14, lerp(m.f32(na + 0x14), m.f32(nb + 0x14)));
    m.set_f32(nd + 0x18, lerp(m.f32(na + 0x18), m.f32(nb + 0x18)));
    if m.u32(nb + 0x20) != 0 {
        if m.u32(nd + 0x20) == 0 {
            let p = m.si_calloc(0xe, 4);
            m.set_u32(nd + 0x20, p);
        }
        for d in 1..8 {
            let (u0, v0) = node_get_uv(m, na, d);
            let (u1, v1) = node_get_uv(m, nb, d);
            node_set_uv(m, d, lerp(u0, u1), lerp(v0, v1), nd);
        }
    }
    let mut ca = m.u32(na + 0x1c) as i32;
    let mut cb = m.u32(nb + 0x1c) as i32;
    let mut out = 0u32;
    for k in 0..4 {
        let (ub, ua) = (cb & 0xff, ca & 0xff);
        cb >>= 8;
        ca >>= 8;
        let s = x(ub - ua) * tt + x(ua);
        out |= ((s.to_i32(Round::Zero) as u32) & 0xff) << (k * 8);
    }
    m.set_u32(nd + 0x1c, out);
    let d8 = x(m.f32(nb + 8)) - x(m.f32(na + 8));
    let d12 = x(m.f32(nb + 0xc)) - x(m.f32(na + 0xc));
    let d16 = x(m.f32(nb + 0x10)) - x(m.f32(na + 0x10));
    let (p8, p12, p16) = (d8 * tt, d12 * tt, d16 * tt);
    let (r12, r16) = (p12.to_f32(), p16.to_f32());
    m.set_f32(nd + 8, (p8 + x(m.f32(na + 8))).to_f32());
    m.set_f32(nd + 0xc, (x(m.f32(na + 0xc)) + x(r12)).to_f32());
    m.set_f32(nd + 0x10, (x(m.f32(na + 0x10)) + x(r16)).to_f32());
}

/// [soft 0x0844cb84 findContourSize]: the smallest and the largest vertex coordinates of
/// `count` corners starting at `node`, into `min` and `max` (three floats each). The
/// minimum takes a coordinate that is below the kept one,
/// the maximum one that is above (both strict and ordered: a NaN never replaces).
pub fn find_contour_size(m: &mut Mem, mesh: u32, count: i32, node: u32, min: u32, max: u32) {
    let p = |m: &Mem, node: u32| vtx(m, mesh, m.i32(node));
    let v0 = p(m, node);
    for k in 0..3 {
        let w = m.u32(v0 + 4 * k);
        m.set_u32(min + 4 * k, w);
    }
    let v0 = p(m, node);
    for k in 0..3 {
        let w = m.u32(v0 + 4 * k);
        m.set_u32(max + 4 * k, w);
    }
    if 1 < count {
        let mut node = node;
        let mut left = count - 1;
        loop {
            node = node.wrapping_add(0x24);
            let v = p(m, node);
            let c = [m.f32(v), m.f32(v + 4), m.f32(v + 8)];
            for k in 0..3u32 {
                let cur = m.f32(min + 4 * k);
                if c[k as usize] < cur {
                    m.set_f32(min + 4 * k, c[k as usize]);
                }
            }
            for k in 0..3u32 {
                if m.f32(max + 4 * k) < c[k as usize] {
                    m.set_f32(max + 4 * k, c[k as usize]);
                }
            }
            left -= 1;
            if left == 0 {
                break;
            }
        }
    }
}

/// [soft 0x0844cca4 locateEdge]: in the polygon's corner list (contours separated by corners
/// whose vertex is −1) finds the corner whose edge number (sign dropped) is that of `edge`:
/// `*a` = its index (−1 if none), `*b` = the index after the last separator seen before it
/// (0 if none; written at each separator), `*c` = the index of the last corner of its
/// contour (written only when found).
pub fn locate_edge(m: &mut Mem, poly: u32, edge: i32, a: u32, b: u32, c: u32) {
    let edge = if edge < 0 { edge.wrapping_neg() } else { edge };
    m.set_i32(a, -1);
    m.set_i32(b, 0);
    let nb = |m: &Mem| m.u16(poly + mo::POLYGON_NB_NODES) as i32;
    let mut i = 0i32;
    if nb(m) == 0 {
        return;
    }
    loop {
        let n = node_at(m, poly, i);
        if m.i32(n) == -1 {
            m.set_i32(b, i + 1);
        } else {
            let mut e = m.i32(n + 4);
            if e < 0 {
                e = e.wrapping_neg();
            }
            let j = i + 1;
            if e == edge {
                m.set_i32(a, i);
                m.set_i32(c, nb(m) - 1);
                if nb(m) <= j {
                    return;
                }
                if m.i32(node_at(m, poly, j)) == -1 {
                    m.set_i32(c, i);
                    return;
                }
                let mut j = j;
                let mut last;
                loop {
                    last = j;
                    j = last + 1;
                    if nb(m) <= j {
                        return;
                    }
                    if m.i32(node_at(m, poly, j)) == -1 {
                        break;
                    }
                }
                m.set_i32(c, last);
                return;
            }
        }
        i += 1;
        if nb(m) <= i {
            return;
        }
    }
}

/// [soft 0x0844cd68 removeNodesDuplicate]: for a polygon of more than two corners, packs
/// the corner list so that no two neighbouring corners have the same vertex (the first
/// corner not a separator moves to the front; a separator is kept like any vertex), drops a
/// trailing separator, and, when the count changed, moves the corners into a new
/// `SI_polygonNodeAllocate` block and disposes the old one with `SI_nodesDispose`. (A count
/// of 0 after packing disposes through the word before the array, as the original reads it.)
pub fn remove_nodes_duplicate(m: &mut Mem, poly: u32) {
    let old = m.u16(poly + mo::POLYGON_NB_NODES);
    if 2 < old {
        let mut skip = 0i32;
        let first = m.u32(poly + mo::POLYGON_NODES);
        let mut last = m.i32(first);
        let mut p = first;
        while last == -1 {
            p = p.wrapping_add(0x24);
            skip += 1;
            last = m.i32(p);
        }
        let mut kept: u16 = 1;
        nodes_copy(m, 0, 0, first, first.wrapping_add((skip as u32).wrapping_mul(0x24)));
        skip += 1;
        if skip < m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
            let mut at = (skip as u32).wrapping_mul(0x24);
            let mut put = 0x24u32;
            kept = 1;
            loop {
                let nodes = m.u32(poly + mo::POLYGON_NODES);
                if m.i32(nodes.wrapping_add(at)) != last {
                    let d = put.wrapping_add(nodes);
                    put = put.wrapping_add(0x24);
                    kept = kept.wrapping_add(1);
                    nodes_copy(m, 0, 0, d, at.wrapping_add(nodes));
                    last = m.i32(m.u32(poly + mo::POLYGON_NODES).wrapping_add(at));
                }
                at = at.wrapping_add(0x24);
                skip += 1;
                if !(skip < m.u16(poly + mo::POLYGON_NB_NODES) as i32) {
                    break;
                }
            }
        }
        m.set_u16(poly + mo::POLYGON_NB_NODES, kept);
        let n = m.u16(poly + mo::POLYGON_NB_NODES) as u32;
        if m.i32(n.wrapping_mul(0x24).wrapping_add(m.u32(poly + mo::POLYGON_NODES)).wrapping_sub(0x24)) == -1 {
            m.set_u16(poly + mo::POLYGON_NB_NODES, kept.wrapping_sub(1));
        }
        let n = m.u16(poly + mo::POLYGON_NB_NODES);
        if old != n {
            if n == 0 {
                let w = m.i32(m.u32(poly + mo::POLYGON_NODES).wrapping_sub(0x24)) == -1;
                nodes_dispose(m, poly + mo::POLYGON_NODES, -1, w as i32);
            } else {
                let fresh = si_hrc::image::geom::polygon_node_allocate(m, n as i32);
                let mut i = 0i32;
                let mut src = 0u32;
                let mut dst = fresh;
                while i < m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
                    let s = src.wrapping_add(m.u32(poly + mo::POLYGON_NODES));
                    nodes_copy(m, 0, 0, dst, s);
                    dst = dst.wrapping_add(0x24);
                    src = src.wrapping_add(0x24);
                    i += 1;
                }
                let mut count = m.u16(poly + mo::POLYGON_NB_NODES) as u32;
                if m.i32(count.wrapping_mul(0x24).wrapping_add(m.u32(poly + mo::POLYGON_NODES)).wrapping_sub(0x24)) == -1 {
                    count += 1;
                }
                nodes_dispose(m, poly + mo::POLYGON_NODES, -1, count as i32);
                m.set_u32(poly + mo::POLYGON_NODES, fresh);
            }
        }
    }
}

/// Removes from the star of the vertex record `v` every entry whose edge (sign dropped) is
/// `e`: entries behind it move up by one, the count drops, and the scan goes on from the
/// index after the one that matched (so an entry that moved into its place is not looked at).
fn star_drop_edge(m: &mut Mem, v: u32, e: u32) {
    let mut i = 0i32;
    if m.u16(v + 0x10) != 0 {
        loop {
            let mut k = i;
            let at = |k: i32| m.u32(v + 0xc).wrapping_add((k as u32).wrapping_mul(12));
            let mut w = m.i32(at(i) + 4);
            if w < 0 {
                w = w.wrapping_neg();
            }
            let next = i + 1;
            if w as u32 == e {
                if k < m.u16(v + 0x10) as i32 - 1 {
                    loop {
                        let s = m.u32(v + 0xc).wrapping_add((k as u32).wrapping_mul(12));
                        for j in 0..3 {
                            let w = m.u32(s + 12 + 4 * j);
                            m.set_u32(s + 4 * j, w);
                        }
                        k += 1;
                        if !(k < m.u16(v + 0x10) as i32 - 1) {
                            break;
                        }
                    }
                }
                m.set_u16(v + 0x10, m.u16(v + 0x10).wrapping_sub(1));
            }
            i = next;
            if !(next < m.u16(v + 0x10) as i32) {
                break;
            }
        }
    }
}

/// One side of [`remove_polygon_from_mesh`]: in the star of vertex record `v`, the entry
/// whose neighbour is `nb` and whose edge is `e` gets its polygon column set to −1.
fn star_clear_polygon(m: &mut Mem, v: u32, nb: i32, e: u32) {
    let mut i = 0i32;
    while i < m.u16(v + 0x10) as i32 {
        let s = m.u32(v + 0xc).wrapping_add((i as u32).wrapping_mul(12));
        if m.i32(s) == nb {
            let mut w = m.i32(s + 4);
            if w < 0 {
                w = w.wrapping_neg();
            }
            if w as u32 == e {
                m.set_i32(s + 8, -1);
                break;
            }
        }
        i += 1;
    }
}

/// [soft 0x0844cf1c removePolygonFromMesh]: for every real corner of polygon `n`, the edge
/// it names loses this polygon: on the side the sign says, the matching entry of the near
/// vertex' star gets polygon −1 and the edge's polygon column is set to −3; when the other
/// side has no polygon either (negative), the edge's entry is dropped from both vertices'
/// stars. The corner block is then disposed (`SI_nodesDispose`) and the count set to 0.
pub fn remove_polygon_from_mesh(m: &mut Mem, mesh: u32, n: i32) {
    let poly = super::polygon_at(m, mesh, n);
    let mut c = 0i32;
    if m.u16(poly + mo::POLYGON_NB_NODES) != 0 {
        loop {
            let node = node_at(m, poly, c);
            c += 1;
            if m.i32(node) != -1 {
                let raw = m.i32(node + 4);
                let e = if raw < 0 { raw.wrapping_neg() } else { raw } as u32;
                let rec = edge_rec(m, mesh, e as i32);
                let other;
                if raw < 0 {
                    let v = vtx(m, mesh, m.i32(rec + 4));
                    star_clear_polygon(m, v, m.i32(rec), e);
                    m.set_i32(rec + 0xc, -3);
                    other = m.i32(rec + 8);
                } else {
                    let v = vtx(m, mesh, m.i32(rec));
                    star_clear_polygon(m, v, m.i32(rec + 4), e);
                    m.set_i32(rec + 8, -3);
                    other = m.i32(rec + 0xc);
                }
                if other < 0 {
                    let v = vtx(m, mesh, m.i32(rec));
                    star_drop_edge(m, v, e);
                    let v = vtx(m, mesh, m.i32(rec + 4));
                    star_drop_edge(m, v, e);
                }
            }
            if !(c < m.u16(poly + mo::POLYGON_NB_NODES) as i32) {
                break;
            }
        }
    }
    if m.u32(poly + mo::POLYGON_NODES) != 0 {
        nodes_dispose(m, poly + mo::POLYGON_NODES, -1, m.u16(poly + mo::POLYGON_NB_NODES) as i32);
    }
    m.set_u16(poly + mo::POLYGON_NB_NODES, 0);
}

/// [soft 0x0844d1d8 nodeShiftRight]: corners `at`..=count move up one place (the last
/// first, by `SI_nodesCopy`), leaving a copy of corner `at − 1`... in place at `at`; the
/// count is not changed.
pub fn node_shift_right(m: &mut Mem, poly: u32, at: i32) {
    let mut n = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
    if at <= n {
        let mut hi = (n as u32).wrapping_mul(0x24);
        let mut lo = hi;
        loop {
            lo = lo.wrapping_sub(0x24);
            let nodes = m.u32(poly + mo::POLYGON_NODES);
            nodes_copy(m, 0, 0, nodes.wrapping_add(hi), lo.wrapping_add(nodes));
            hi = hi.wrapping_sub(0x24);
            n -= 1;
            if !(at <= n) {
                break;
            }
        }
    }
}

/// [soft 0x08453b9c SI_nodesAllocate]: room for corner `n` in the array `*pp` of `count`
/// corners of a polygon of `model`: answers 0 when `n < -count & 3`; else a null array is
/// `SI_polygonNodeAllocate(n)`, any other `SI_realloc` to `n` rounded up to 4 corners, the
/// new corners filled as the allocator fills them but with a block of
/// `SI_calloc(2 × nb_uv − 2, 4)` for the further pairs when the model has two or more
/// texture domains (+0x424). Answers 1.
pub fn nodes_allocate(m: &mut Mem, model: u32, pp: u32, count: i32, n: i32) -> i32 {
    if n < (count.wrapping_neg() & 3) {
        return 0;
    }
    if m.u32(pp) == 0 {
        let p = si_hrc::image::geom::polygon_node_allocate(m, n);
        m.set_u32(pp, p);
    } else {
        let cap = round_up(n, 3);
        let p = m.si_realloc(m.u32(pp), (cap as u32).wrapping_mul(0x24));
        m.set_u32(pp, p);
        if count < cap {
            let mut off = (count as u32).wrapping_mul(0x24);
            let mut left = cap - count;
            loop {
                let at = m.u32(pp).wrapping_add(off);
                init_node(m, at, 0);
                if model == 0 || m.i32(model + 0x424) < 2 {
                    m.set_u32(m.u32(pp).wrapping_add(off + 0x20), 0);
                } else {
                    let b = m.si_calloc(m.i32(model + 0x424).wrapping_mul(2).wrapping_sub(2) as u32, 4);
                    m.set_u32(m.u32(pp).wrapping_add(off + 0x20), b);
                }
                off = off.wrapping_add(0x24);
                left -= 1;
                if left == 0 {
                    break;
                }
            }
        }
    }
    1
}

/// [soft 0x0844d228 SI_addVertexToMesh]: a vertex `(x, y, z)` appended (arrays grown with
/// `SI_vertexAllocate` / [`vertex_reallocate`]); the third vertex of a mesh makes the first
/// polygon (0, 1, 2) and updates the mesh. For any later vertex and an edge `e` (not −1) the
/// vertex splits that edge: the new edge `count + 1` takes the old edge's far end, polygons
/// and flags; the new vertex gets the two star entries; the far vertex' entry is renamed;
/// in each of the edge's polygons the new vertex is inserted between the edge's two
/// corners ([`node_shift_right`], [`compute_node_attributes`] with the ratio of the
/// distances `|a − new| / |a − b|`, taken in `f64` and rounded to `f32` for the call); the
/// edge flags are set again ([`set_edge_flag`]).
pub fn add_vertex_to_mesh(m: &mut Mem, o: &mut dyn Outside, mesh: u32, px: f32, py: f32, pz: f32, e: u32) {
    let nv = m.i32(mesh + mo::NB_VERTICES);
    if nv == 0 {
        let p = si_hrc::image::geom::vertex_allocate(m, 1);
        m.set_u32(mesh, p);
    } else {
        vertex_reallocate(m, mesh, 1);
    }
    m.set_i32(mesh + mo::NB_VERTICES, m.i32(mesh + mo::NB_VERTICES) + 1);
    let v = vtx(m, mesh, nv);
    m.set_f32(v, px);
    m.set_f32(v + 4, py);
    m.set_f32(v + 8, pz);
    m.set_u16(v + 0x12, 0);
    m.set_u16(v + 0x10, 0);
    m.set_u32(v + 0xc, 0);
    if m.i32(mesh + mo::NB_VERTICES) == 3 {
        m.set_i32(mesh + mo::NB_POLYGONS, 1);
        let p = si_hrc::image::geom::polygon_allocate(m, 1);
        m.set_u32(mesh + mo::POLYGONS, p);
        rebuild::polygon_initialize(m, p, &[0, 1, 2]);
        rebuild::update(m, o, mesh);
    } else if e != 0xffff_ffff {
        neighbor_reallocate(m, v, 2);
        m.set_u16(v + 0x10, 2);
        edge_reallocate(m, mesh, 1);
        let ne = m.i32(mesh + mo::NB_EDGES).wrapping_add(1);
        m.set_i32(mesh + mo::NB_EDGES, ne);
        let e3 = edge_rec(m, mesh, e as i32);
        let e4 = edge_rec(m, mesh, ne);
        let a = m.i32(e3);
        let b = m.i32(e3 + 4);
        m.set_i32(e4, nv);
        m.set_i32(e4 + 4, b);
        m.set_u32(e4 + 8, m.u32(e3 + 8));
        m.set_u32(e4 + 0xc, m.u32(e3 + 0xc));
        m.set_u16(e4 + 0x10, 0);
        m.set_u16(e4 + 0x12, m.u16(e3 + 0x12));
        m.set_i32(e3 + 4, nv);
        let star = |m: &Mem, k: u32| m.u32(v + 0xc).wrapping_add(12 * k);
        m.set_i32(star(m, 0), a);
        m.set_u32(star(m, 0) + 4, e.wrapping_neg());
        m.set_u32(star(m, 0) + 8, m.u32(e3 + 0xc));
        m.set_i32(star(m, 1), b);
        m.set_u32(star(m, 1) + 4, ne as u32);
        m.set_u32(star(m, 1) + 8, m.u32(e3 + 8));
        let va = vtx(m, mesh, a);
        let mut i = 0i32;
        while i < m.u16(va + 0x10) as i32 {
            let s = m.u32(va + 0xc).wrapping_add((i as u32).wrapping_mul(12));
            if m.i32(s) == b {
                m.set_i32(s, nv);
                m.set_u32(s + 4, e);
                break;
            }
            i += 1;
        }
        let vb = vtx(m, mesh, b);
        let mut i = 0i32;
        while i < m.u16(vb + 0x10) as i32 {
            let s = m.u32(vb + 0xc).wrapping_add((i as u32).wrapping_mul(12));
            if m.i32(s) == a {
                m.set_i32(s, nv);
                m.set_u32(s + 4, (ne as u32).wrapping_neg());
                break;
            }
            i += 1;
        }
        // the ratio |a - new| / |a - b|: the first length squared sum is stored as f64, its
        // root stored as f64 again; the second root stays in the register; the quotient is
        // stored as f64 and then as f32 for the call
        let (pa, pn, pb) = (vtx(m, mesh, a), vtx(m, mesh, nv), vtx(m, mesh, b));
        let sq = |m: &Mem, p: u32, q: u32| {
            let d = |k: u32| x(m.f32(p + k)) - x(m.f32(q + k));
            let (dx, dy, dz) = (d(0), d(4), d(8));
            (dx * dx + dy * dy) + dz * dz
        };
        let s1 = x(sq(m, pa, pn).to_f64()).sqrt().to_f64();
        let s2 = sq(m, pa, pb).sqrt();
        let ratio = (x(s1) / s2).to_f64() as f32;
        if m.i32(e3 + 8) != -1 {
            let poly = super::polygon_at(m, mesh, m.i32(e3 + 8));
            node_reallocate(m, poly, 1);
            let (mut pos_a, mut pos_b) = (-1i32, -1i32);
            let mut i = 0i32;
            while i < m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
                let w = m.i32(node_at(m, poly, i));
                if w == a {
                    let stop = pos_b != -1;
                    pos_a = i;
                    if stop {
                        break;
                    }
                } else if w == b {
                    let stop = pos_a != -1;
                    pos_b = i;
                    if stop {
                        break;
                    }
                }
                i += 1;
            }
            if pos_a != -1 && pos_b != -1 {
                let (mut hi, lo) = if pos_a < pos_b { (pos_b, pos_a) } else { (pos_a, pos_b) };
                if lo + 1 != hi {
                    hi += 1;
                }
                node_shift_right(m, poly, hi);
                if pos_a < pos_b {
                    if pos_a + 1 == pos_b {
                        pos_b = pos_a + 2;
                    }
                } else if pos_b + 1 == pos_a {
                    pos_a = pos_b + 2;
                }
                let n = node_at(m, poly, hi);
                m.set_i32(n, nv);
                m.set_u32(n + 4, ne as u32);
                let nodes = m.u32(poly + mo::POLYGON_NODES);
                compute_node_attributes(m, ratio, nodes, hi, pos_a, pos_b);
                m.set_u16(poly + mo::POLYGON_NB_NODES, m.u16(poly + mo::POLYGON_NB_NODES).wrapping_add(1));
            }
        }
        if m.i32(e3 + 0xc) != -1 {
            let poly = super::polygon_at(m, mesh, m.i32(e3 + 0xc));
            node_reallocate(m, poly, 1);
            let (mut pos_a, mut pos_b) = (-1i32, -1i32);
            let mut i = 0i32;
            while i < m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
                let w = m.i32(node_at(m, poly, i));
                if w == a {
                    let stop = pos_b != -1;
                    pos_a = i;
                    if stop {
                        break;
                    }
                } else if w == b {
                    let stop = pos_a != -1;
                    pos_b = i;
                    if stop {
                        break;
                    }
                }
                i += 1;
            }
            if pos_a != -1 && pos_b != -1 {
                let (mut hi, lo) = if pos_a < pos_b { (pos_b, pos_a) } else { (pos_a, pos_b) };
                if lo + 1 != hi {
                    hi += 1;
                }
                node_shift_right(m, poly, hi);
                if pos_a < pos_b {
                    if pos_a + 1 == pos_b {
                        pos_b = pos_a + 2;
                    }
                } else if pos_b + 1 == pos_a {
                    pos_a = pos_b + 2;
                }
                let nb = node_at(m, poly, pos_b);
                m.set_u32(nb + 4, (ne as u32).wrapping_neg());
                let n = node_at(m, poly, hi);
                m.set_i32(n, nv);
                m.set_u32(n + 4, e.wrapping_neg());
                let nodes = m.u32(poly + mo::POLYGON_NODES);
                compute_node_attributes(m, ratio, nodes, hi, pos_a, pos_b);
                m.set_u16(poly + mo::POLYGON_NB_NODES, m.u16(poly + mo::POLYGON_NB_NODES).wrapping_add(1));
            }
        }
        set_edge_flag(m, mesh);
    }
}

/// [soft 0x0844d724 insertOutside]: puts the outer contour of `poly` (its corners up to the
/// first separator, or all of them) in front of the `*count` corners at `nodes`: the old
/// corners move up by that length plus one (from the last, `SI_nodesCopy` with the model
/// twice), the outer contour's vertices (word 0 only) are copied to the front, a separator
/// follows them, and `*count` grows by the length plus one.
pub fn insert_outside(m: &mut Mem, model: u32, poly: u32, nodes: u32, count: u32) {
    let mut outer = 0i32;
    if m.u16(poly + mo::POLYGON_NB_NODES) != 0 {
        let mut p = m.u32(poly + mo::POLYGON_NODES);
        let mut w = m.i32(p);
        while w != -1 {
            p = p.wrapping_add(0x24);
            outer += 1;
            if m.u16(poly + mo::POLYGON_NB_NODES) as i32 <= outer {
                break;
            }
            w = m.i32(p);
        }
    }
    let mut k = m.i32(count).wrapping_sub(1);
    if -1 < k {
        let mut dst = nodes.wrapping_add((k as u32).wrapping_mul(0x24)).wrapping_add((outer as u32).wrapping_mul(0x24)).wrapping_add(0x24);
        let mut src = nodes.wrapping_add((k as u32).wrapping_mul(0x24));
        loop {
            nodes_copy(m, model, model, dst, src);
            dst = dst.wrapping_sub(0x24);
            src = src.wrapping_sub(0x24);
            k -= 1;
            if !(-1 < k) {
                break;
            }
        }
    }
    if 0 < outer {
        let mut off = 0u32;
        for _ in 0..outer {
            let w = m.u32(m.u32(poly + mo::POLYGON_NODES).wrapping_add(off));
            m.set_u32(nodes.wrapping_add(off), w);
            off = off.wrapping_add(0x24);
        }
    }
    m.set_u32(nodes.wrapping_add((outer as u32).wrapping_mul(0x24)), 0xffff_ffff);
    m.set_i32(count, m.i32(count).wrapping_add(1).wrapping_add(outer));
}

/// [soft 0x0844d7ec contourIsCCW]: whether the first contour of `nodes` (up to the first
/// separator, `n` corners at most) runs counter-clockwise in the XY plane: the sum over the
/// edges, from the last corner round, of `(y_prev + y) (x_prev - x)`, each product formed in
/// the register and added to the `f32` sum, which is rounded when stored; true when > 0.
/// (A first corner that is a separator makes the original read the word before the array.)
pub fn contour_is_ccw(m: &mut Mem, verts: u32, nodes: u32, n: i32) -> bool {
    let mut len = 0i32;
    if 0 < n {
        let mut p = nodes;
        let mut w = m.i32(p);
        while w != -1 {
            p = p.wrapping_add(0x24);
            len += 1;
            if n <= len {
                break;
            }
            w = m.i32(p);
        }
    }
    let at = |m: &Mem, k: i32| verts.wrapping_add((m.i32(nodes.wrapping_add((k as u32).wrapping_mul(0x24))) as u32).wrapping_mul(12));
    let last = at(m, len.wrapping_sub(1));
    let (mut px, mut py) = (m.f32(last), m.f32(last + 4));
    let mut area = 0f32;
    let mut k = 0;
    while k < len {
        let a = at(m, k);
        let (cx, cy) = (m.f32(a), m.f32(a + 4));
        area = ((x(px) - x(cx)) * (x(py) + x(cy)) + x(area)).to_f32();
        px = cx;
        py = cy;
        k += 1;
    }
    0.0 < area
}

/// [soft 0x08453288 pointInsideContour]: whether the point `(px, py, pz)` is inside the
/// contours of `nodes` (`count` corners, separators −1; vertex coordinates at `verts`,
/// 12 bytes each), by the parity of crossings of a ray. The angles (`fpatan`, stored as
/// `f64` then `f32` in a block of `SI_calloc(count, 4)`) of the corners round the point are
/// sorted; the ray leaves at the middle of the widest gap between neighbouring angles (the
/// wrap-around gap is `first + 2π − last`), with the length 4 × √(largest squared distance);
/// each contour's edges (the closing one first) are tested with `SI_segmentIntersect`
/// ([`crate::triangulate::segment_intersect`], which leaves its last product in `mD0`).
/// The C library's `cos` and `sin` are asked through [`si_x87::trig`].
pub fn point_inside_contour(m: &mut Mem, px: f32, py: f32, pz: f32, verts: u32, count: i32, nodes: u32) -> u32 {
    let arr = m.si_calloc(count as u32, 4);
    let mut max = x(0.0f32);
    let mut n = 0i32;
    let mut k = 0;
    while 0 < count && k < count {
        let node = nodes.wrapping_add((k as u32).wrapping_mul(0x24));
        if m.i32(node) != -1 {
            let v = verts.wrapping_add((m.u32(node)).wrapping_mul(12));
            let dx = x(m.f32(v)) - x(px);
            let dy = x(m.f32(v + 4)) - x(py);
            let dz = x(m.f32(v + 8)) - x(pz);
            let sum = (dy * dy + dx * dx) + dz * dz;
            if sum.gt(max) {
                max = sum;
            }
            let a = trig::fpatan(dy, dx).to_f64();
            m.set_f32(arr.wrapping_add(4 * n as u32), a as f32);
            n += 1;
        }
        k += 1;
    }
    let mut i = 0;
    while i < n - 1 {
        let mut j = i + 1;
        while j < n {
            let (ai, aj) = (m.f32(arr + 4 * i as u32), m.f32(arr + 4 * j as u32));
            if ai > aj {
                m.set_f32(arr + 4 * j as u32, ai);
                m.set_f32(arr + 4 * i as u32, aj);
            }
            j += 1;
        }
        i += 1;
    }
    let tau = 6.283185307179586f64;
    let gap = (x(m.f32(arr)) + x(tau) - x(m.f32(arr.wrapping_add(4 * n as u32).wrapping_sub(4)))).to_f32();
    let mut gap = x(gap);
    let mut best = 0i32;
    let mut j = 1;
    while j < n {
        let d = x(m.f32(arr + 4 * j as u32)) - x(m.f32(arr + 4 * (j - 1) as u32));
        if d.gt(gap) {
            gap = d;
            best = j;
        }
        j += 1;
    }
    let r = (max.sqrt() * x(4.0f64)).to_f32();
    let theta = (x(m.f32(arr.wrapping_add(4 * best as u32))) - gap * x(0.5f64)).to_f32();
    if arr != 0 {
        m.free(arr);
    }
    let rr = x(r);
    let c = trig::cos(theta as f64);
    let ex = (c * rr + x(px as f64)).to_f32();
    let s = trig::sin(theta as f64);
    let ey = (s * rr + x(py as f64)).to_f32();
    let mut crossings = 0i32;
    let mut start = 0i32;
    let mut k = 0i32;
    crate::triangulate::take_md0();
    let node_v = |m: &Mem, k: i32| m.i32(nodes.wrapping_add((k as u32).wrapping_mul(0x24)));
    let pt = |m: &Mem, v: i32| {
        let a = verts.wrapping_add((v as u32).wrapping_mul(12));
        [m.f32(a), m.f32(a + 4), m.f32(a + 8)]
    };
    while k <= count {
        let mut next = start;
        if k == count || node_v(m, k) == -1 {
            next = k + 1;
            let mut j = start;
            if j <= k - 1 {
                let mut prev = pt(m, node_v(m, k - 1));
                while j <= k - 1 {
                    let cur = pt(m, node_v(m, j));
                    let hit = crate::triangulate::segment_intersect([px, py, pz], [ex, ey, 0.0], prev, cur);
                    crossings += hit as i16 as i32;
                    prev = cur;
                    j += 1;
                }
            }
        }
        start = next;
        k += 1;
    }
    if let Some(v) = crate::triangulate::take_md0() {
        m[super::g::MD0] = v;
    }
    (crossings as u32) & 1
}

/// [soft 0x0844d8bc dispatchHoles]: splits the contours of `poly` after its first into two
/// lists, `nodes_a` (with `*count_a` corners) when the contour's first vertex is inside the
/// contours already in `nodes_a` ([`point_inside_contour`]), else `nodes_b`; each contour is
/// appended with a separator (−1) in front, its vertex word only. The contour starting at
/// corner `skip` is left out.
pub fn dispatch_holes(m: &mut Mem, poly: u32, verts: u32, nodes_a: u32, count_a: u32, nodes_b: u32, count_b: u32, skip: i32) {
    let mut start = 0i32;
    let mut k = 0i32;
    let node = |m: &Mem, k: i32| m.u32(poly + mo::POLYGON_NODES).wrapping_add((k as u32).wrapping_mul(0x24));
    loop {
        let nb = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
        let mut next = start;
        if k == nb || m.i32(node(m, k)) == -1 {
            next = k + 1;
            if start != 0 && start != skip {
                let vi = m.u32(node(m, start));
                let p = verts.wrapping_add(vi.wrapping_mul(12));
                let (a, b, c) = (m.f32(p), m.f32(p + 4), m.f32(p + 8));
                let inside = point_inside_contour(m, a, b, c, verts, m.i32(count_a), nodes_a) as u16 as i16;
                let (dst, cnt) = if inside == 0 { (nodes_b, count_b) } else { (nodes_a, count_a) };
                m.set_u32(dst.wrapping_add((m.u32(cnt)).wrapping_mul(0x24)), 0xffff_ffff);
                m.set_i32(cnt, m.i32(cnt).wrapping_add(1));
                let mut j = start;
                while j <= k - 1 {
                    let w = m.u32(node(m, j));
                    m.set_u32(dst.wrapping_add(m.u32(cnt).wrapping_mul(0x24)), w);
                    m.set_i32(cnt, m.i32(cnt).wrapping_add(1));
                    j += 1;
                }
            }
        }
        start = next;
        k += 1;
        let nb = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
        if !(k <= nb) {
            break;
        }
    }
}

/// [soft 0x0844da30 attachHoles]: appends to `nodes` (`*count` corners) every contour of
/// `poly` after the first except those starting at corner `skip_a` or `skip_b`: a separator
/// (−1) and the contour's vertex words.
pub fn attach_holes(m: &mut Mem, poly: u32, nodes: u32, count: u32, skip_a: i32, skip_b: i32) {
    let mut start = 0i32;
    let mut k = 0i32;
    let node = |m: &Mem, k: i32| m.u32(poly + mo::POLYGON_NODES).wrapping_add((k as u32).wrapping_mul(0x24));
    while k <= m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
        let mut next = start;
        if k == m.u16(poly + mo::POLYGON_NB_NODES) as i32 || m.i32(node(m, k)) == -1 {
            next = k + 1;
            if start != 0 && start != skip_a && start != skip_b {
                m.set_u32(nodes.wrapping_add(m.u32(count).wrapping_mul(0x24)), 0xffff_ffff);
                m.set_i32(count, m.i32(count).wrapping_add(1));
                let mut j = start;
                while j <= k - 1 {
                    let w = m.u32(node(m, j));
                    m.set_u32(nodes.wrapping_add(m.u32(count).wrapping_mul(0x24)), w);
                    m.set_i32(count, m.i32(count).wrapping_add(1));
                    j += 1;
                }
            }
        }
        start = next;
        k += 1;
    }
}

/// The edge record of a signed edge number (the sign is the direction): record `|e| & 0x3ff`
/// of block `|e| >> 10`.
fn edge_abs(m: &Mem, mesh: u32, e: i32) -> u32 {
    edge_rec(m, mesh, if e < 0 { e.wrapping_neg() } else { e })
}

/// The corner after `k` in the contour `[start, end]`, round the ring.
fn ring_next(k: i32, start: i32, end: i32) -> i32 {
    if k == end {
        start
    } else {
        k + 1
    }
}

/// [soft 0x0844dae0 SI_addEdgeToMesh]: a new edge between vertices `a` and `b` of one polygon
/// (the polygon is split in two, or a hole is joined to the outline). Answers 0, or the
/// error: −1 the vertices are already joined, −2 they share no polygon, −3 the new edge
/// would cross one of the polygon's edges (tested in the plane the polygon's normal is
/// aligned to the Z axis by `SI_matrixZAlign`, the vertices stored as `f32` after
/// `((y m1 + x m0) + z m2) + m3`, each sum kept at 80 bits), −5 no corner pair found.
///
/// Corner pairs: in each contour of the polygon the first corner at `a` (and the first at
/// `b`) such that the direction to the other vertex lies in the wedge between the
/// directions to the corner's previous and next vertices (angles by `fpatan`, stored as
/// `f64` then `f32`; 2π added at 80 bits to the smaller ones; a contour whose previous and
/// next vertices are the same always counts). The corners are copied into one or two new
/// corner lists (walked round the contour from one vertex to the other, or both contours
/// joined into one when they differ), the holes dispatched or attached again, the polygon
/// taken out of the mesh ([`super::edit::mesh_remove_ipolygon`]), given the first list and
/// put back; when both corners lay in one contour a new polygon (same material and flags)
/// made of the second list is appended; hard edges and vertex normals follow.
pub fn add_edge_to_mesh(m: &mut Mem, o: &mut dyn Outside, model: u32, mesh: u32, a: i16, b: i16, track: i16) -> u32 {
    let (pa, pb) = (a as i32, b as i32);
    let sp = m.sp();
    let va = vtx(m, mesh, pa);
    for i in 0..m.u16(va + mo::VERTEX_NB_STAR) as u32 {
        if m.i32(m.u32(va + 0xc).wrapping_add(12 * i)) == pb {
            return 0xffff_ffff;
        }
    }
    let vb = vtx(m, mesh, pb);
    for i in 0..m.u16(vb + mo::VERTEX_NB_STAR) as u32 {
        if m.i32(m.u32(vb + 0xc).wrapping_add(12 * i)) == pa {
            return 0xffff_ffff;
        }
    }
    // the polygon both vertices lie in
    let mut shared = -1i32;
    'outer: for i in 0..m.u16(va + mo::VERTEX_NB_STAR) as u32 {
        let ea = edge_abs(m, mesh, m.i32(m.u32(va + 0xc).wrapping_add(12 * i + 4)));
        for j in 0..m.u16(vb + mo::VERTEX_NB_STAR) as u32 {
            let eb = edge_abs(m, mesh, m.i32(m.u32(vb + 0xc).wrapping_add(12 * j + 4)));
            let (a1, a2) = (m.i32(ea + 8), m.i32(ea + 0xc));
            let (b1, b2) = (m.i32(eb + 8), m.i32(eb + 0xc));
            if a1 != -1 && (a1 == b1 || a1 == b2) {
                shared = a1;
                break 'outer;
            }
            if a2 != -1 && (a2 == b1 || a2 == b2) {
                shared = a2;
                break 'outer;
            }
        }
    }
    if shared == -1 {
        return 0xffff_fffe;
    }
    let poly = super::polygon_at(m, mesh, shared);
    let normal = Vector { x: m.f32(poly), y: m.f32(poly + 4), z: m.f32(poly + 8) };
    let mt = si_math::algebra::matrix_z_align(&normal);
    let nvert = m.i32(mesh + mo::NB_VERTICES);
    let buf = m.si_malloc((nvert as u32).wrapping_mul(12));
    let node = |m: &Mem, k: i32| node_at(m, poly, k);
    let nnodes = |m: &Mem| m.u16(poly + mo::POLYGON_NB_NODES) as i32;
    for k in 0..nnodes(m) {
        let w = m.i32(node(m, k));
        if w != -1 {
            let v = vtx(m, mesh, w);
            let (px, py, pz) = (m.f32(v), m.f32(v + 4), m.f32(v + 8));
            let t = |c: usize| (((x(py) * mt[1][c]) + (x(px) * mt[0][c])) + (x(pz) * mt[2][c]) + x(mt[3][c])).to_f32();
            let o4 = buf.wrapping_add((w as u32).wrapping_mul(12));
            m.set_f32(o4, t(0));
            m.set_f32(o4 + 4, t(1));
            m.set_f32(o4 + 8, t(2));
        }
    }
    let pt = |m: &Mem, v: i32| {
        let q = buf.wrapping_add((v as u32).wrapping_mul(12));
        [m.f32(q), m.f32(q + 4), m.f32(q + 8)]
    };
    // a crossing with an edge of the polygon
    crate::triangulate::take_md0();
    for k in 0..nnodes(m) {
        if m.i32(node(m, k)) != -1 {
            let e = edge_abs(m, mesh, m.i32(node(m, k) + 4));
            let hit = crate::triangulate::segment_intersect(pt(m, pa), pt(m, pb), pt(m, m.i32(e)), pt(m, m.i32(e + 4)));
            if hit {
                if buf != 0 {
                    m.free(buf);
                }
                if let Some(v) = crate::triangulate::take_md0() {
                    m[super::g::MD0] = v;
                }
                return 0xffff_fffd;
            }
        }
    }
    if let Some(v) = crate::triangulate::take_md0() {
        m[super::g::MD0] = v;
    }
    // the corner at each vertex, per contour: (index, start, end)
    let two_pi = x(6.2831854820251465f64);
    let (mut ca, mut cb) = ((-1i32, 0i32, 0i32), (-1i32, 0i32, 0i32));
    let mut start = 0i32;
    let mut u = 0i32;
    let mut total = nnodes(m);
    loop {
        if u == total || m.i32(node(m, u)) == -1 {
            let end = u - 1;
            let mut k = start;
            while k <= end {
                for (which, at, other) in [(0, pa, pb), (1, pb, pa)] {
                    let slot = if which == 0 { ca } else { cb };
                    if m.i32(node(m, k)) == at && slot.0 == -1 {
                        let from = pt(m, at);
                        let ang = |m: &Mem, to: i32| {
                            let t = pt(m, to);
                            trig::fpatan(x(t[1]) - x(from[1]), x(t[0]) - x(from[0])).to_f64() as f32
                        };
                        let a1 = ang(m, other);
                        let prev = if k == start { m.i32(node(m, end)) } else { m.i32(node(m, k - 1)) };
                        let next = if k == end { m.i32(node(m, start)) } else { m.i32(node(m, k + 1)) };
                        let mut hit = false;
                        if prev == next {
                            hit = true;
                        } else {
                            let b1 = ang(m, prev);
                            let c1 = ang(m, next);
                            let b2 = if x(b1).lt(x(c1)) { x(b1) + two_pi } else { x(b1) };
                            let a2 = if x(a1).lt(x(c1)) { x(a1) + two_pi } else { x(a1) };
                            if x(c1).le(a2) && a2.le(b2) {
                                hit = true;
                            }
                        }
                        if hit {
                            let r = (k, start, end);
                            if which == 0 {
                                ca = r;
                            } else {
                                cb = r;
                            }
                        }
                    }
                }
                k += 1;
            }
            start = u + 1;
        }
        if ca.0 != -1 && cb.0 != -1 {
            break;
        }
        u += 1;
        total = nnodes(m);
        if u > total {
            break;
        }
    }
    if ca.0 == -1 || cb.0 == -1 {
        if buf != 0 {
            m.free(buf);
        }
        return 0xffff_fffb;
    }
    if cb.0 < ca.0 {
        std::mem::swap(&mut ca, &mut cb);
    }
    let (s8c, s94, s9c) = ca;
    let (mut s88, s90, s98) = cb;
    let nb = nnodes(m);
    let l68 = m.local(4);
    let l6c = m.local(4);
    let l74 = m.local(4);
    let l70 = m.local(4);
    m.set_u32(l68, 0);
    m.set_u32(l6c, 0);
    m.set_i32(l74, 0);
    m.set_i32(l70, 0);
    nodes_allocate(m, model, l68, 0, nb + 1);
    nodes_allocate(m, model, l6c, 0, nb + 1);
    let push = |m: &mut Mem, list: u32, cnt: u32, k: i32| {
        let n = m.i32(cnt);
        m.set_i32(cnt, n + 1);
        let d = m.u32(list).wrapping_add((n as u32).wrapping_mul(0x24));
        let s = node_at(m, poly, k);
        nodes_copy(m, model, model, d, s);
    };
    if s94 != s90 {
        push(m, l68, l74, s8c);
        let mut k = if s8c == s9c { s94 } else { s8c + 1 };
        while k != s8c {
            push(m, l68, l74, k);
            k = ring_next(k, s94, s9c);
        }
        push(m, l68, l74, s8c);
        push(m, l68, l74, s88);
        let mut k = if s88 == s98 { s90 } else { s88 + 1 };
        while k != s88 {
            push(m, l68, l74, k);
            k = ring_next(k, s90, s98);
        }
        push(m, l68, l74, s88);
    } else {
        let mut k = s8c;
        while k != s88 {
            push(m, l68, l74, k);
            k = ring_next(k, s94, s9c);
        }
        push(m, l68, l74, s88);
        while s88 != s8c {
            push(m, l6c, l70, s88);
            s88 = ring_next(s88, s94, s9c);
        }
        push(m, l6c, l70, s8c);
    }
    if s94 == 0 {
        if s90 != 0 {
            attach_holes(m, poly, m.u32(l68), l74, s90, 0);
        } else {
            let (p68, p6c) = (m.u32(l68), m.u32(l6c));
            dispatch_holes(m, poly, buf, p68, l74, p6c, l70, 0);
        }
    } else if s94 != s90 {
        insert_outside(m, model, poly, m.u32(l68), l74);
        attach_holes(m, poly, m.u32(l68), l74, s94, s90);
    } else {
        if !contour_is_ccw(m, buf, m.u32(l68), m.i32(l74)) {
            insert_outside(m, model, poly, m.u32(l68), l74);
        } else {
            insert_outside(m, model, poly, m.u32(l6c), l70);
        }
        let (p68, p6c) = (m.u32(l68), m.u32(l6c));
        dispatch_holes(m, poly, buf, p68, l74, p6c, l70, s94);
    }
    if buf != 0 {
        m.free(buf);
    }
    super::edit::mesh_remove_ipolygon(m, mesh, shared, 0, track);
    let old = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
    nodes_allocate(m, model, poly + mo::POLYGON_NODES, old, m.i32(l74));
    for k in 0..m.i32(l74) {
        let d = m.u32(poly + mo::POLYGON_NODES).wrapping_add((k as u32).wrapping_mul(0x24));
        let s = m.u32(l68).wrapping_add((k as u32).wrapping_mul(0x24));
        nodes_copy(m, model, model, d, s);
    }
    m.set_u16(poly + mo::POLYGON_NB_NODES, m.i32(l74) as u16);
    rebuild::insert_ipolygon(m, o, mesh, shared);
    rebuild::computes_polygon_normal(m, mesh, shared);
    if s94 == s90 {
        let np = m.u32(mesh + mo::NB_POLYGONS);
        let bytes = np.wrapping_sub(np | 0xffff_ffe0).wrapping_mul(0x18);
        let t = m.si_realloc(m.u32(mesh + mo::POLYGONS), bytes);
        m.set_u32(mesh + mo::POLYGONS, t);
        let fresh = t.wrapping_add(np.wrapping_mul(0x18));
        m.set_u32(mesh + mo::NB_POLYGONS, np.wrapping_add(1));
        let orig = super::polygon_at(m, mesh, shared);
        m.set_u16(fresh + 0x14, m.u16(orig + 0x14));
        m.set_u16(fresh + 0x12, m.u16(orig + 0x12));
        m.set_u32(fresh + 0xc, 0);
        m.set_u16(fresh + 0x10, 0);
        nodes_allocate(m, model, fresh + 0xc, m.u16(fresh + 0x10) as i32, m.i32(l70));
        for k in 0..m.i32(l70) {
            let d = m.u32(fresh + 0xc).wrapping_add((k as u32).wrapping_mul(0x24));
            let s = m.u32(l6c).wrapping_add((k as u32).wrapping_mul(0x24));
            nodes_copy(m, model, model, d, s);
        }
        m.set_u16(fresh + 0x10, m.i32(l70) as u16);
        let last = m.i32(mesh + mo::NB_POLYGONS) - 1;
        rebuild::insert_ipolygon(m, o, mesh, last);
        rebuild::computes_polygon_normal(m, mesh, last);
    }
    rebuild::compute_hard_edge(m, mesh, -1);
    rebuild::computes_vertex_normal(m, mesh, -1);
    nodes_dispose(m, l68, -1, m.i32(l74));
    nodes_dispose(m, l6c, -1, m.i32(l70));
    m.set_sp(sp);
    0
}

/// [soft 0x0844eac8 si_addPolygonToMesh]: a polygon through the `n` vertices (16-bit numbers
/// at `verts`) appended to the mesh, with the model's current material (`u16` at +0x208) and
/// no flags. Answers 0, or −2 when the vertices already used by an existing polygon lie in
/// more than one of its contours, −3 when one of them is met twice in a contour at a place
/// that does not follow the first (the test compares the second match's index, less the
/// contour's length, less one, with the previous match's index). The polygon is filed
/// ([`rebuild::insert_ipolygon`]), its normal and the hard edges computed; with the mesh's
/// flag 8 clear the corners take the polygon's normal (the normal computed a second time),
/// else the vertex normals are computed.
pub fn add_polygon_to_mesh(m: &mut Mem, o: &mut dyn Outside, model: u32, mesh: u32, n: i32, verts: u32) -> u32 {
    let vert = |m: &Mem, j: i32| m.i16(verts.wrapping_add((j as u32).wrapping_mul(2))) as i32;
    let mut p = 0i32;
    let mut poly = m.u32(mesh + mo::POLYGONS);
    let mut last = 0i32; // local_1c: read before it is set only where its value is not used
    if 0 < m.i32(mesh + mo::NB_POLYGONS) {
        loop {
            let mut start = 0i32;
            let mut first = -1i32;
            let mut u = 0i32;
            loop {
                if u == m.u16(poly + mo::POLYGON_NB_NODES) as i32 || m.i32(node_at(m, poly, u)) == -1 {
                    let end = u - 1;
                    let mut prev = -1i32; // local_20
                    let mut j = 0i32;
                    if 0 < n {
                        loop {
                            if start <= end {
                                let mut c = start;
                                loop {
                                    let mut keep = prev;
                                    let mut keep_last = last;
                                    if vert(m, j) == m.i32(node_at(m, poly, c)) {
                                        let mut t = start;
                                        if first != -1 {
                                            t = first;
                                            if start != first {
                                                return 0xffff_fffe;
                                            }
                                        }
                                        first = t;
                                        keep = c;
                                        keep_last = c;
                                        if prev != -1 {
                                            if prev < c {
                                                keep_last = c - (end - start) - 1;
                                            }
                                            keep = prev;
                                            if last <= keep_last {
                                                return 0xffff_fffd;
                                            }
                                        }
                                    }
                                    last = keep_last;
                                    prev = keep;
                                    c += 1;
                                    if c > end {
                                        break;
                                    }
                                }
                            }
                            j += 1;
                            if j >= n {
                                break;
                            }
                        }
                    }
                    start = u + 1;
                }
                u += 1;
                if u > m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
                    break;
                }
            }
            p += 1;
            poly = poly.wrapping_add(0x18);
            if p >= m.i32(mesh + mo::NB_POLYGONS) {
                break;
            }
        }
    }
    let np = m.u32(mesh + mo::NB_POLYGONS);
    let bytes = np.wrapping_sub(np | 0xffff_ffe0).wrapping_mul(0x18);
    let t = m.si_realloc(m.u32(mesh + mo::POLYGONS), bytes);
    m.set_u32(mesh + mo::POLYGONS, t);
    let rec = t.wrapping_add(np.wrapping_mul(0x18));
    m.set_u32(mesh + mo::NB_POLYGONS, np.wrapping_add(1));
    m.set_u16(rec + 0x14, 0);
    m.set_u16(rec + 0x12, m.u16(model + 0x208));
    m.set_u32(rec + 0xc, 0);
    nodes_allocate(m, model, rec + 0xc, 0, n);
    for j in 0..n {
        let d = m.u32(rec + 0xc).wrapping_add((j as u32).wrapping_mul(0x24));
        m.set_i32(d, vert(m, j));
    }
    m.set_u16(rec + 0x10, n as u16);
    let last_poly = m.i32(mesh + mo::NB_POLYGONS) - 1;
    rebuild::insert_ipolygon(m, o, mesh, last_poly);
    rebuild::computes_polygon_normal(m, mesh, last_poly);
    rebuild::compute_hard_edge(m, mesh, -1);
    if m.u8(mesh + mo::FLAGS) & 8 == 0 {
        rebuild::computes_polygon_normal(m, mesh, last_poly);
        for k in 0..m.u16(rec + 0x10) as i32 {
            let c = m.u32(rec + 0xc).wrapping_add((k as u32).wrapping_mul(0x24));
            if m.i32(c) != -1 {
                m.set_u32(c + 8, m.u32(rec));
                m.set_u32(c + 0xc, m.u32(rec + 4));
                m.set_u32(c + 0x10, m.u32(rec + 8));
            }
        }
    } else {
        rebuild::computes_vertex_normal(m, mesh, -1);
    }
    0
}

/// [soft 0x0844ed7c si_removeVertexFromMesh]: vertex `v` taken out of every polygon that
/// uses it. A copy of the mesh is kept first. While the vertex has polygons: the last of its
/// star (the last entry with a polygon) is taken out of the mesh ([`super::edit::mesh_remove_ipolygon`],
/// no compression, `track` passed on), each contour of its corners rewritten without the
/// vertex and without repeats of the corner before (a closing corner equal to the first is
/// dropped; a contour left with fewer than 2 corners is dropped, the outer one with fewer than
/// 3 drops the polygon), separators (−1, edge 0) between the contours; a polygon that is kept
/// is filed again, one that is not has its place in `SI_gblIndex2` fixed up (when it is installed
/// and `track` is 1) and is compressed away ([`super::edit::mesh_compress_polygons_array`]),
/// with the vertex normals recomputed when the mesh's flag 8 is set. When a removal answers 0
/// the mesh is emptied and replaced by the copy and the message "corrupted mesh, can't
/// delete vertex" shown (an uncited hook, `SI_statusDisplayMessage (0x083cca94)`); else the
/// hard edges are computed, vertex `v` compressed away ([`super::edit::mesh_compress_vertices_array`])
/// and the copy disposed. (A vertex without polygons makes the original use an unset local.)
pub fn remove_vertex_from_mesh(m: &mut Mem, o: &mut dyn Outside, model: u32, mesh: u32, v: i32, track: i32) {
    const SI_GBL_INDEX2: u32 = 0x08788970;
    let sp = m.sp();
    let mut removed = 0i32;
    let mut ok = 1i16;
    let backup = m.local(4);
    let copy = super::obj::mesh_copy(m, o, mesh);
    m.set_u32(backup, copy);
    let vr = vtx(m, mesh, v);
    let mut last_poly = 0i32; // local_10, unset in the original until a star entry has a polygon
    if m.u16(vr + mo::VERTEX_NB_STAR) != 0 {
        loop {
            let mut left = m.u16(vr + mo::VERTEX_NB_STAR) as u32;
            if left != 0 {
                let mut p = m.u32(vr + 0xc).wrapping_add(8);
                loop {
                    if m.i32(p) != -1 {
                        last_poly = m.i32(p);
                    }
                    p = p.wrapping_add(12);
                    left -= 1;
                    if left == 0 {
                        break;
                    }
                }
            }
            let poly = super::polygon_at(m, mesh, last_poly);
            let mut keep = true;
            ok = super::edit::mesh_remove_ipolygon(m, mesh, last_poly, 0, track as i16) as i16;
            if ok != 0 {
                let mut start = 0i32;
                let mut out = 0i32;
                let mut u = 0i32;
                let mut count = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
                loop {
                    if u == count || m.i32(node_at(m, poly, u)) == -1 {
                        if start != 0 {
                            let c = node_at(m, poly, out);
                            out += 1;
                            m.set_u32(c, 0xffff_ffff);
                            m.set_u32(c + 4, 0);
                        }
                        let begin = out;
                        let mut k = start;
                        while k <= u - 1 {
                            let w = m.i32(node_at(m, poly, k));
                            if w != v && (out == begin || w != m.i32(node_at(m, poly, out - 1))) {
                                let (d, s) = (node_at(m, poly, out), node_at(m, poly, k));
                                nodes_copy(m, model, model, d, s);
                                out += 1;
                            }
                            k += 1;
                        }
                        if m.i32(node_at(m, poly, out - 1)) == m.i32(node_at(m, poly, begin)) {
                            out -= 1;
                        }
                        count = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
                        if start == 0 {
                            if out - begin < 3 {
                                keep = false;
                                u = count;
                            }
                        } else if out - begin < 2 {
                            out = begin - 1;
                        }
                        start = u + 1;
                    }
                    u += 1;
                    if u > count {
                        break;
                    }
                }
                m.set_u16(poly + mo::POLYGON_NB_NODES, out as u16);
                if keep {
                    rebuild::insert_ipolygon(m, o, mesh, last_poly);
                } else {
                    let g2 = m.u32(SI_GBL_INDEX2);
                    if g2 != 0 && track == 1 {
                        let at = m.i32(mesh + mo::NB_POLYGONS).wrapping_sub(removed).wrapping_sub(1);
                        let slot = |i: i32| g2.wrapping_add((i as u32).wrapping_mul(4));
                        if last_poly == at {
                            m.set_i32(slot(last_poly), -1);
                        } else {
                            if last_poly < at {
                                m.set_i32(slot(at), last_poly);
                            } else {
                                let mut i = 0;
                                while i < last_poly {
                                    let t = m.i32(slot(i));
                                    if m.i32(slot(last_poly)) < t {
                                        m.set_i32(slot(i), t - 1);
                                    }
                                    i += 1;
                                }
                            }
                            m.set_i32(slot(last_poly), -1);
                        }
                        removed += 1;
                    }
                    super::edit::mesh_compress_polygons_array(m, mesh, last_poly);
                    if m.u8(mesh + mo::FLAGS) & 8 != 0 {
                        rebuild::computes_vertex_normal(m, mesh, -1);
                    }
                }
            }
            if !(m.u16(vr + mo::VERTEX_NB_STAR) != 0 && ok != 0) {
                break;
            }
        }
    }
    if ok == 0 {
        super::obj::mesh_delete(m, mesh);
        let b = m.u32(backup);
        m.copy(mesh, b, 0x54);
        if b != 0 {
            m.free(b);
        }
        m.set_u32(backup, 0);
        o.call(m, &super::callee::SI_STATUS_DISPLAY_MESSAGE, &[0x086d4ec0, 3]);
    } else {
        rebuild::compute_hard_edge(m, mesh, -1);
        super::edit::mesh_compress_vertices_array(m, mesh, v);
        super::obj::mesh_dispose(m, backup);
    }
    m.set_sp(sp);
}

/// The corner colour halved from two corner colours, channel by channel (each byte of the two
/// added and halved, the sums staying below 512).
fn colour_mean(a: u32, b: u32) -> u32 {
    let ch = |s: u32| ((a >> s) & 0xff).wrapping_add((b >> s) & 0xff) / 2;
    ch(0) | ch(8) << 8 | ch(16) << 16 | ch(24) << 24
}

/// Corner `dst` of a corner list taking vertex, normal-side words (+8, +0xc, +0x10) and
/// texture pairs from `src`.
fn corner_take(m: &mut Mem, model: u32, dst: u32, src: u32) {
    m.set_u32(dst, m.u32(src));
    for off in [8u32, 0xc, 0x10] {
        m.set_u32(dst + off, m.u32(src + off));
    }
    super::obj::nodes_copy_uv(m, model, model, dst, src);
}

/// [soft 0x0844f0f4 si_collapseVertexInMesh]: vertex `v` merged away. A copy of the mesh is
/// kept. While the vertex has polygons: the last of its star is taken out of the mesh
/// ([`super::edit::mesh_remove_ipolygon`], no compression, no tracking), the number of
/// removed polygons per material counted, and each contour rewritten without the vertex and
/// without repeats of the corner before; where the vertex stood, the corner before it and
/// the corner after it (with the mean of their colour and the removed corner's) are set
/// aside as one pair in list A. A polygon left with enough corners is filed again, one that
/// is not is compressed away (its place in `SI_gblIndex2` fixed up when installed and `track`
/// is 1). Then the pairs are chained into one outline (list B, the colours averaged along the
/// chain), the polygon of that outline (the material removed most often) appended and filed
/// when it has more than two corners, the normals and hard edges computed, vertex `v`
/// compressed away and the copy disposed. When anything is refused the mesh is emptied and
/// replaced by the copy and "initial or resulting mesh is corrupted" shown (an uncited hook,
/// `SI_statusDisplayMessage (0x083cca94)`); the two lists are then not freed, as in the
/// original. (A vertex without polygons makes the original read an empty list.)
pub fn collapse_vertex_in_mesh(m: &mut Mem, o: &mut dyn Outside, model: u32, mesh: u32, v: i32, track: i32) {
    const SI_GBL_INDEX2: u32 = 0x08788970;
    let sp = m.sp();
    let mut removed = 0i32;
    let mut ok = 1i16;
    let backup = m.local(4);
    let la = m.local(4); // list A (pairs)
    let lb = m.local(4); // list B (outline)
    let copy = super::obj::mesh_copy(m, o, mesh);
    m.set_u32(backup, copy);
    let mut na = 0i32;
    m.set_u32(la, 0);
    let mut nb = 0i32;
    m.set_u32(lb, 0);
    let mut counts = vec![0i16; 0x7fff];
    let vr = vtx(m, mesh, v);
    let mut last_poly = 0i32;
    let mut failed = false;
    let mut first_pass = true;
    // do { if (sVar8 == 0) break; ... sVar8 = ok; } while (nstar != 0)
    let mut gate = m.u16(vr + mo::VERTEX_NB_STAR) as i16;
    loop {
        if gate == 0 {
            break;
        }
        let _ = first_pass;
        first_pass = false;
        let mut left = m.u16(vr + mo::VERTEX_NB_STAR) as u32;
        if left != 0 {
            let mut p = m.u32(vr + 0xc).wrapping_add(8);
            loop {
                if m.i32(p) != -1 {
                    last_poly = m.i32(p);
                }
                p = p.wrapping_add(12);
                left -= 1;
                if left == 0 {
                    break;
                }
            }
        }
        let poly = super::polygon_at(m, mesh, last_poly);
        let mut keep = true;
        let mat = m.i16(poly + 0x12) as i32;
        if (0..0x7fff).contains(&mat) {
            counts[mat as usize] = counts[mat as usize].wrapping_add(1);
        }
        ok = super::edit::mesh_remove_ipolygon(m, mesh, last_poly, 0, 0) as i16;
        if ok != 0 {
            let mut start = 0i32;
            let mut out = 0i32;
            let mut u = 0i32;
            let mut count = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
            loop {
                if u == count || m.i32(node_at(m, poly, u)) == -1 {
                    let end = u - 1;
                    if start != 0 {
                        let c = node_at(m, poly, out);
                        out += 1;
                        m.set_u32(c, 0xffff_ffff);
                        m.set_u32(c + 4, 0);
                    }
                    let begin = out;
                    let mut k = start;
                    while k <= end {
                        let w = m.i32(node_at(m, poly, k));
                        if w == v {
                            let next = if k == end { start } else { k + 1 };
                            let prev = if k == start { end } else { k - 1 };
                            nodes_allocate(m, model, la, na, na + 1);
                            let (pn, cn, nn) = (node_at(m, poly, prev), node_at(m, poly, k), node_at(m, poly, next));
                            let d = m.u32(la).wrapping_add((na as u32).wrapping_mul(0x24));
                            corner_take(m, model, d, pn);
                            m.set_u32(d + 0x1c, colour_mean(m.u32(pn + 0x1c), m.u32(cn + 0x1c)));
                            let n1 = na + 1;
                            na += 2;
                            nodes_allocate(m, model, la, n1, na);
                            let d = m.u32(la).wrapping_add((n1 as u32).wrapping_mul(0x24));
                            corner_take(m, model, d, nn);
                            m.set_u32(d + 0x1c, colour_mean(m.u32(nn + 0x1c), m.u32(cn + 0x1c)));
                        } else if out == begin || w != m.i32(node_at(m, poly, out - 1)) {
                            let (d, s) = (node_at(m, poly, out), node_at(m, poly, k));
                            m.set_u32(d, w as u32);
                            m.set_u32(d + 4, 0);
                            for off in [8u32, 0xc, 0x10, 0x1c] {
                                m.set_u32(d + off, m.u32(s + off));
                            }
                            super::obj::nodes_copy_uv(m, model, model, d, s);
                            out += 1;
                        }
                        k += 1;
                    }
                    if m.i32(node_at(m, poly, out - 1)) == m.i32(node_at(m, poly, begin)) {
                        out -= 1;
                    }
                    count = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
                    if start == 0 {
                        if out - begin < 3 {
                            keep = false;
                            u = count;
                        }
                    } else if out - begin < 2 {
                        out = begin - 1;
                    }
                    start = u + 1;
                }
                u += 1;
                if u > count {
                    break;
                }
            }
            m.set_u16(poly + mo::POLYGON_NB_NODES, out as u16);
            if keep {
                ok = rebuild::insert_ipolygon(m, o, mesh, last_poly) as i16;
                if ok == 0 {
                    failed = true;
                    break;
                }
            } else {
                let g2 = m.u32(SI_GBL_INDEX2);
                if g2 != 0 && track == 1 {
                    let at = m.i32(mesh + mo::NB_POLYGONS).wrapping_sub(removed).wrapping_sub(1);
                    let slot = |i: i32| g2.wrapping_add((i as u32).wrapping_mul(4));
                    if last_poly == at {
                        m.set_i32(slot(last_poly), -1);
                    } else {
                        if last_poly < at {
                            m.set_i32(slot(at), last_poly);
                        } else {
                            let mut i = 0;
                            while i < last_poly {
                                let t = m.i32(slot(i));
                                if m.i32(slot(last_poly)) < t {
                                    m.set_i32(slot(i), t - 1);
                                }
                                i += 1;
                            }
                        }
                        m.set_i32(slot(last_poly), -1);
                    }
                    removed += 1;
                }
                super::edit::mesh_compress_polygons_array(m, mesh, last_poly);
            }
        }
        gate = ok;
        if m.u16(vr + mo::VERTEX_NB_STAR) == 0 {
            break;
        }
    }
    let mut nbc = 0i32; // iVar17: corners of list B
    if !failed && ok != 0 {
        let half = na / 2;
        let at = |m: &Mem, l: u32, k: i32| m.u32(l).wrapping_add((k as u32).wrapping_mul(0x24));
        let mut first = 0i32;
        let mut lastc = 0i32;
        for u in 0..half {
            let (mut b2, mut b3) = (true, true);
            for k in 0..half {
                if m.i32(at(m, la, u * 2)) == m.i32(at(m, la, k * 2 + 1)) {
                    b2 = false;
                }
                if m.i32(at(m, la, u * 2 + 1)) == m.i32(at(m, la, k * 2)) {
                    b3 = false;
                }
            }
            if b2 {
                first = u * 2;
            }
            if b3 {
                lastc = u * 2 + 1;
            }
        }
        if first != 0 || lastc != 0 {
            let n1 = na + 1;
            nodes_allocate(m, model, la, na, n1);
            let (d, s) = (at(m, la, na), at(m, la, lastc));
            corner_take(m, model, d, s);
            m.set_u32(d + 0x1c, m.u32(s + 0x1c));
            na += 2;
            nodes_allocate(m, model, la, n1, na);
            let (d, s) = (at(m, la, n1), at(m, la, first));
            corner_take(m, model, d, s);
            m.set_u32(d + 0x1c, m.u32(s + 0x1c));
        }
        let half = na / 2; // (the pairs added above count)
        let mut cur = 1i32;
        let mut col = m.u32(at(m, la, 1) + 0x1c);
        let mut off = 0i32;
        loop {
            let a = at(m, la, cur);
            let n1 = nb + 1;
            let cc = m.u32(a + 0x1c);
            nodes_allocate(m, model, lb, nb, n1);
            let d = at(m, lb, off);
            corner_take(m, model, d, a);
            m.set_u32(d + 0x1c, colour_mean(cc, col));
            off += 1;
            col = m.u32(at(m, la, cur - 1) + 0x1c);
            let want = m.i32(at(m, la, cur - 1));
            for k in 0..half {
                if m.i32(at(m, la, k * 2 + 1)) == want {
                    cur = k * 2 + 1;
                    break;
                }
            }
            nb = n1;
            if cur == 1 || nb > 100_000 {
                // (a chain that never closes: the original grows the list until memory is gone)
                break;
            }
        }
        nbc = nb;
        let c1 = m.u32(at(m, la, 1) + 0x1c);
        m.set_u32(at(m, lb, 0) + 0x1c, colour_mean(c1, col));
        let (mut best_n, mut best) = (0i16, 0i32);
        for (i, &c) in counts.iter().enumerate() {
            if best_n < c {
                best = i as i32;
                best_n = c;
            }
        }
        if 2 < nbc {
            let np = m.u32(mesh + mo::NB_POLYGONS);
            let bytes = np.wrapping_sub(np | 0xffff_ffe0).wrapping_mul(0x18);
            let t = m.si_realloc(m.u32(mesh + mo::POLYGONS), bytes);
            m.set_u32(mesh + mo::POLYGONS, t);
            let rec = t.wrapping_add(np.wrapping_mul(0x18));
            m.set_u32(mesh + mo::NB_POLYGONS, np.wrapping_add(1));
            m.set_u16(rec + 0x14, 0);
            m.set_u16(rec + 0x12, best as u16);
            m.set_u32(rec + 0xc, 0);
            nodes_allocate(m, model, rec + 0xc, 0, nbc);
            for k in 0..nbc {
                let d = at(m, rec + 0xc, k);
                let s = at(m, lb, k);
                m.set_u32(d, m.u32(s));
                m.set_u32(d + 4, 0);
                nodes_copy(m, model, model, d, s);
                m.set_u32(d + 0x1c, m.u32(s + 0x1c));
            }
            m.set_u16(rec + 0x10, nbc as u16);
            ok = rebuild::insert_ipolygon(m, o, mesh, m.i32(mesh + mo::NB_POLYGONS) - 1) as i16;
        }
    } else {
        failed = true;
    }
    if !failed && ok != 0 {
        rebuild::computes_polygon_normal(m, mesh, -1);
        rebuild::compute_hard_edge(m, mesh, -1);
        rebuild::computes_vertex_normal(m, mesh, -1);
        super::edit::mesh_compress_vertices_array(m, mesh, v);
        nodes_dispose(m, la, -1, na);
        nodes_dispose(m, lb, -1, nbc);
        super::obj::mesh_dispose(m, backup);
        m.set_sp(sp);
        return;
    }
    super::obj::mesh_delete(m, mesh);
    let b = m.u32(backup);
    m.copy(mesh, b, 0x54);
    if b != 0 {
        m.free(b);
    }
    m.set_u32(backup, 0);
    o.call(m, &super::callee::SI_STATUS_DISPLAY_MESSAGE, &[0x086d4f00, 3]);
    m.set_sp(sp);
}

/// [soft 0x08450090 SI_canRemoveEdgeFromMesh]: 1 when edge `e` may be taken out of the mesh,
/// 0 when the polygon made of the two polygons of the edge would repeat a vertex or when a
/// vertex of it already has an edge joining the same two polygons (in either order). An edge
/// that has fewer than two different polygons answers 1. The merged outline is built in a
/// block `SI_polygonNodeAllocate(n₁ − 2 + n₀)`, corner copies (nine words each) in the order
/// of the original: the first polygon's corners up to the edge's corner, the second polygon's
/// after the edge's up to the end of its contour, from its contour start up to the edge's
/// corner, the first polygon's rest, then a separator (−1, −1) and the second polygon's
/// first contours when the second polygon's edge is not in the first contour, and the rest.
pub fn can_remove_edge_from_mesh(m: &mut Mem, mesh: u32, e: i32) -> u32 {
    let sp = m.sp();
    let ed = edge_rec(m, mesh, e);
    let (pa, pb) = (m.i32(ed + 8), m.i32(ed + 0xc));
    if pa == -1 || pb == -1 || pa == pb {
        return 1;
    }
    let polys = [super::polygon_at(m, mesh, pa), super::polygon_at(m, mesh, pb)];
    let l = m.local(24);
    let li = |m: &Mem, i: usize| m.i32(l + 4 * i as u32);
    locate_edge(m, polys[0], e, l, l + 8, l + 16);
    locate_edge(m, polys[1], e, l + 4, l + 12, l + 20);
    let t8 = (li(m, 2) == 0) as usize;
    let t3c = (li(m, 2) != 0) as usize;
    let n = (m.u16(polys[1] + mo::POLYGON_NB_NODES) as i32 - 2).wrapping_add(m.u16(polys[0] + mo::POLYGON_NB_NODES) as i32);
    let buf = si_hrc::image::geom::polygon_node_allocate(m, n);
    let mut nn = 0i32;
    let take = |m: &mut Mem, nn: &mut i32, poly: u32, i: i32| {
        let s = node_at(m, poly, i);
        let d = buf.wrapping_add((*nn as u32).wrapping_mul(0x24));
        for k in 0..9 {
            m.set_u32(d + 4 * k, m.u32(s + 4 * k));
        }
        *nn += 1;
    };
    let cnt = |m: &Mem, p: u32| m.u16(p + mo::POLYGON_NB_NODES) as i32;
    let mut i = 0;
    while i < li(m, t8) {
        take(m, &mut nn, polys[t8], i);
        i += 1;
    }
    i = li(m, t3c) + 1;
    while i <= li(m, t3c + 4) {
        take(m, &mut nn, polys[t3c], i);
        i += 1;
    }
    i = li(m, t3c + 2);
    while i < li(m, t3c) {
        take(m, &mut nn, polys[t3c], i);
        i += 1;
    }
    i = li(m, t8) + 1;
    while i < cnt(m, polys[t8]) {
        take(m, &mut nn, polys[t8], i);
        i += 1;
    }
    let b2 = li(m, t3c + 2);
    if b2 != 1 && -1 < b2 - 1 {
        let d = buf.wrapping_add((nn as u32).wrapping_mul(0x24));
        m.set_u32(d, 0xffff_ffff);
        m.set_u32(d + 4, 0xffff_ffff);
        nn += 1;
        i = 0;
        while i < li(m, t3c) - 1 {
            take(m, &mut nn, polys[t3c], i);
            i += 1;
        }
    }
    i = li(m, t3c + 4) + 1;
    while i < cnt(m, polys[t3c]) {
        take(m, &mut nn, polys[t3c], i);
        i += 1;
    }
    let bv = |m: &Mem, k: i32| m.i32(buf.wrapping_add((k as u32).wrapping_mul(0x24)));
    let mut result = 1u32;
    'out: {
        if 0 < nn - 1 {
            let mut i = 0;
            loop {
                let a = bv(m, i);
                i += 1;
                if a != -1 && i < nn {
                    for j in i..nn {
                        if a == bv(m, j) {
                            result = 0;
                            break 'out;
                        }
                    }
                }
                if i >= nn - 1 {
                    break;
                }
            }
        }
        for i in 0..nn {
            let w = bv(m, i);
            if w == -1 {
                continue;
            }
            let vr = vtx(m, mesh, w);
            for k in 0..m.u16(vr + mo::VERTEX_NB_STAR) as u32 {
                let e2 = edge_abs(m, mesh, m.i32(m.u32(vr + 0xc).wrapping_add(12 * k + 4)));
                let (a, b) = (m.i32(e2 + 8), m.i32(e2 + 0xc));
                if (a == pb && b == pa) || (b == pb && a == pa) {
                    result = 0;
                    break 'out;
                }
            }
        }
    }
    if buf != 0 {
        m.free(buf);
    }
    m.set_sp(sp);
    result
}

/// Corners `from..to` (not including `to`) of `poly`'s list appended to the merged list `buf`
/// at index `*nn` by [`nodes_copy`] without models; `*nn` follows.
fn append_corners(m: &mut Mem, buf: u32, nn: &mut i32, poly: u32, from: i32, to: i32) {
    let mut i = from;
    while i < to {
        let d = buf.wrapping_add((*nn as u32).wrapping_mul(0x24));
        let s = node_at(m, poly, i);
        nodes_copy(m, 0, 0, d, s);
        *nn += 1;
        i += 1;
    }
}

/// [soft 0x084505f8 SI_removeEdgeFromMesh]: edge `e` taken out of the mesh; answers 0, or 1
/// when a polygon would be left with a repeated vertex or the same polygon on both sides of
/// an edge. An edge with one polygon removes that polygon ([`super::edit::mesh_remove_ipolygon`]
/// with compression, `track` passed on; `SI_gblIndex2` fixed up first), then the hard edges
/// (and, with the mesh's flag 8, the vertex normals). An edge with two: the edge's two
/// vertices lose each other from their stars (an entry that follows a removed one is skipped
/// in the scan); with the same polygon on both sides (a bridge to a hole) the corners between
/// its two corners are taken out as a contour of their own and, with the rest, put into a
/// new corner block (the contour inside the first of the others whose bounding box holds
/// it, else first), duplicates removed, the polygon dropped when it has under 3 corners;
/// with two polygons their outlines are merged into the first (the second emptied, its
/// place in `SI_gblIndex2` closed up) and the vertices' stars and the edges renamed. The edge
/// is then marked removed (polygons −2) and the edge, polygon, star and corner numbers
/// packed down through two renumbering tables (`SI_calloc`, `SI_malloc`), edge flags set again
/// ([`set_edge_flag`]).
pub fn remove_edge_from_mesh(m: &mut Mem, mesh: u32, e: i32, track: i16) -> i32 {
    const SI_GBL_INDEX2: u32 = 0x08788970;
    const SI_GBL_INDEX3: u32 = 0x08788974;
    let sp = m.sp();
    let mut ret = 0i16;
    let abs_e = if e < 0 { e.wrapping_neg() } else { e };
    let ed = edge_rec(m, mesh, e);
    let (mut pa, pb) = (m.i32(ed + 8), m.i32(ed + 0xc));
    let np = |m: &Mem| m.i32(mesh + mo::NB_POLYGONS);
    if pa == -1 {
        pa = pb;
    } else if pb != -1 {
        let g3 = m.u32(SI_GBL_INDEX3);
        if g3 != 0 && track != 0 {
            m.set_i32(g3.wrapping_add((e as u32).wrapping_mul(4)), -1);
        }
        // the two vertices lose each other from their stars
        for (this, other) in [(m.i32(ed), m.i32(ed + 4)), (m.i32(ed + 4), m.i32(ed))] {
            let vr = vtx(m, mesh, this);
            let mut n = m.u16(vr + mo::VERTEX_NB_STAR) as i32;
            let mut i = 0i32;
            if n != 0 {
                loop {
                    let i1 = i + 1;
                    let s = m.u32(vr + 0xc).wrapping_add((i as u32).wrapping_mul(12));
                    if m.i32(s) == other {
                        if i < n - 1 {
                            loop {
                                let s = m.u32(vr + 0xc).wrapping_add((i as u32).wrapping_mul(12));
                                for k in 0..3 {
                                    m.set_u32(s + 4 * k, m.u32(s + 12 + 4 * k));
                                }
                                i += 1;
                                if i >= m.u16(vr + mo::VERTEX_NB_STAR) as i32 - 1 {
                                    break;
                                }
                            }
                        }
                        m.set_u16(vr + mo::VERTEX_NB_STAR, m.u16(vr + mo::VERTEX_NB_STAR).wrapping_sub(1));
                    }
                    n = m.u16(vr + mo::VERTEX_NB_STAR) as i32;
                    i = i1;
                    if i1 >= n {
                        break;
                    }
                }
            }
        }
        let (pa0, pb0) = (m.i32(ed + 8), m.i32(ed + 0xc));
        if pa0 != -1 && pb0 != -1 {
            if pa0 == pb0 {
                let poly = super::polygon_at(m, mesh, pa0);
                let list = m.local(4);
                m.set_u32(list, 0);
                let (mut first, mut second) = (-1i32, -1i32);
                let mut i = 0i32;
                while i < m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
                    let c = node_at(m, poly, i);
                    if m.i32(c) != -1 {
                        let ce = m.i32(c + 4);
                        let ce = if ce < 0 { ce.wrapping_neg() } else { ce };
                        if ce == abs_e {
                            if first != -1 {
                                second = i;
                                break;
                            }
                            first = i;
                        }
                    }
                    i += 1;
                }
                if first != -1 && second != -1 {
                    let n = second - first - 1;
                    let after = second + 1;
                    if 0 < n {
                        let l = si_hrc::image::geom::polygon_node_allocate(m, n);
                        m.set_u32(list, l);
                        let c = first + 1;
                        let mut k = 0;
                        while c + k < second {
                            let d = l.wrapping_add((k as u32).wrapping_mul(0x24));
                            let s = node_at(m, poly, c + k);
                            nodes_copy(m, 0, 0, d, s);
                            k += 1;
                        }
                    }
                    let mut k = 0;
                    while after + k < m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
                        let d = node_at(m, poly, first + k);
                        let s = node_at(m, poly, after + k);
                        nodes_copy(m, 0, 0, d, s);
                        k += 1;
                    }
                    let newc = (m.u16(poly + mo::POLYGON_NB_NODES) as i16).wrapping_sub((n as i16).wrapping_add(2));
                    m.set_u16(poly + mo::POLYGON_NB_NODES, newc as u16);
                    if 0 < n {
                        let mut best = -1i32; // local_8c
                        let mut size = n; // local_90
                        let mut alloc = n;
                        let bb = m.local(48);
                        let (mn, mx, cmn, cmx) = (bb, bb + 12, bb + 24, bb + 36);
                        if newc != 0 {
                            find_contour_size(m, mesh, n, m.u32(list), mn, mx);
                            let mut cnt = 0i32;
                            let mut start = 0i32;
                            let mut u = 0i32;
                            let mut total = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
                            loop {
                                if u == total || m.i32(node_at(m, poly, u)) == -1 {
                                    find_contour_size(m, mesh, cnt, node_at(m, poly, start), cmn, cmx);
                                    let f = |m: &Mem, a: u32| f32::from_bits(m.u32(a));
                                    let inside = (0..3u32).all(|k| f(m, cmn + 4 * k) <= f(m, mn + 4 * k))
                                        && (0..3u32).all(|k| f(m, mx + 4 * k) <= f(m, cmx + 4 * k));
                                    if inside {
                                        for k in 0..3u32 {
                                            m.set_u32(mn + 4 * k, m.u32(cmn + 4 * k));
                                            m.set_u32(mx + 4 * k, m.u32(cmx + 4 * k));
                                        }
                                        size = cnt;
                                        best = start;
                                    }
                                    cnt = 0;
                                    start = u + 1;
                                } else {
                                    cnt += 1;
                                }
                                u += 1;
                                total = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
                                if u > total {
                                    break;
                                }
                            }
                            alloc = total + 1 + n;
                        }
                        let cell = poly + 0xc;
                        let nb = si_hrc::image::geom::polygon_node_allocate(m, alloc);
                        let mut nn = 0i32;
                        let rest = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
                        let sep = |m: &mut Mem, nn: &mut i32| {
                            let d = nb.wrapping_add((*nn as u32).wrapping_mul(0x24));
                            m.set_u32(d, 0xffff_ffff);
                            m.set_u32(d + 4, 0xffff_ffff);
                            *nn += 1;
                        };
                        if best == -1 {
                            append_corners_from(m, nb, &mut nn, list, 0, n);
                            if rest != 0 {
                                sep(m, &mut nn);
                                append_corners(m, nb, &mut nn, poly, 0, rest);
                            }
                        } else {
                            let before = best - 1;
                            if 0 < size {
                                for k in 0..size {
                                    let d = nb.wrapping_add((k as u32).wrapping_mul(0x24));
                                    let s = node_at(m, poly, best + k);
                                    for w in 0..9 {
                                        m.set_u32(d + 4 * w, m.u32(s + 4 * w));
                                    }
                                    nn += 1;
                                }
                            }
                            sep(m, &mut nn);
                            if 0 < before {
                                append_corners(m, nb, &mut nn, poly, 0, before);
                                sep(m, &mut nn);
                            }
                            let from = best + size + 1;
                            if from < rest {
                                append_corners(m, nb, &mut nn, poly, from, rest);
                                sep(m, &mut nn);
                            }
                            if 0 < n {
                                append_corners_from(m, nb, &mut nn, list, 0, n);
                            }
                        }
                        if m.u32(list) != 0 {
                            nodes_dispose(m, list, -1, n);
                        }
                        nodes_dispose(m, cell, -1, m.u16(poly + mo::POLYGON_NB_NODES) as i32);
                        m.set_u16(poly + mo::POLYGON_NB_NODES, nn as u16);
                        m.set_u32(poly + 0xc, nb);
                    }
                }
                remove_nodes_duplicate(m, poly);
                if m.u16(poly + mo::POLYGON_NB_NODES) < 3 {
                    remove_polygon_from_mesh(m, mesh, m.i32(ed + 8));
                }
            } else {
                let polys = [super::polygon_at(m, mesh, pa0), super::polygon_at(m, mesh, pb0)];
                let l = m.local(24);
                let li = |m: &Mem, i: usize| m.i32(l + 4 * i as u32);
                locate_edge(m, polys[0], e, l, l + 8, l + 16);
                locate_edge(m, polys[1], e, l + 4, l + 12, l + 20);
                let t4 = (li(m, 2) == 0) as usize;
                let t8 = (li(m, 2) != 0) as usize;
                let n = (m.u16(polys[1] + mo::POLYGON_NB_NODES) as i32 - 2).wrapping_add(m.u16(polys[0] + mo::POLYGON_NB_NODES) as i32);
                let nb = si_hrc::image::geom::polygon_node_allocate(m, n);
                let cnt = |m: &Mem, p: u32| m.u16(p + mo::POLYGON_NB_NODES) as i32;
                let mut nn = 0i32;
                append_corners(m, nb, &mut nn, polys[t4], 0, li(m, t4));
                append_corners(m, nb, &mut nn, polys[t8], li(m, t8) + 1, li(m, t8 + 4) + 1);
                append_corners(m, nb, &mut nn, polys[t8], li(m, t8 + 2), li(m, t8));
                append_corners(m, nb, &mut nn, polys[t4], li(m, t4) + 1, cnt(m, polys[t4]));
                let b2 = li(m, t8 + 2);
                if b2 != 1 && -1 < b2 - 1 {
                    let d = nb.wrapping_add((nn as u32).wrapping_mul(0x24));
                    m.set_u32(d, 0xffff_ffff);
                    m.set_u32(d + 4, 0xffff_ffff);
                    nn += 1;
                    append_corners(m, nb, &mut nn, polys[t8], 0, li(m, t8) - 1);
                }
                append_corners(m, nb, &mut nn, polys[t8], li(m, t8 + 4) + 1, cnt(m, polys[t8]));
                nodes_dispose(m, polys[0] + 0xc, -1, cnt(m, polys[0]));
                nodes_dispose(m, polys[1] + 0xc, -1, cnt(m, polys[1]));
                m.set_u32(polys[0] + 0xc, nb);
                m.set_u16(polys[0] + mo::POLYGON_NB_NODES, nn as u16);
                m.set_u32(polys[1] + 0xc, 0);
                m.set_u16(polys[1] + mo::POLYGON_NB_NODES, 0);
                let g2 = m.u32(SI_GBL_INDEX2);
                if g2 != 0 {
                    let p1 = m.i32(ed + 0xc);
                    let slot = |i: i32| g2.wrapping_add((i as u32).wrapping_mul(4));
                    if p1 == np(m) - 1 {
                        m.set_i32(slot(p1), -1);
                    } else {
                        m.set_i32(slot(p1), -1);
                        let mut i = p1 + 1;
                        while i < np(m) {
                            m.set_i32(slot(i), m.i32(slot(i)).wrapping_sub(1));
                            i += 1;
                        }
                    }
                }
                for i in 0..nn {
                    let w = m.i32(nb.wrapping_add((i as u32).wrapping_mul(0x24)));
                    if w != -1 {
                        let vr = vtx(m, mesh, w);
                        for k in 0..m.u16(vr + mo::VERTEX_NB_STAR) as u32 {
                            let s = m.u32(vr + 0xc).wrapping_add(12 * k);
                            if m.i32(s + 8) == m.i32(ed + 0xc) {
                                m.set_u32(s + 8, m.u32(ed + 8));
                            }
                            let e3 = edge_abs(m, mesh, m.i32(s + 4));
                            if m.i32(e3 + 8) == m.i32(ed + 0xc) {
                                m.set_u32(e3 + 8, m.u32(ed + 8));
                            }
                            if m.i32(e3 + 0xc) == m.i32(ed + 0xc) {
                                m.set_u32(e3 + 0xc, m.u32(ed + 8));
                            }
                        }
                    }
                }
                if 0 < nn - 1 {
                    let mut i = 0i32;
                    loop {
                        let i1 = i + 1;
                        let a = m.i32(nb.wrapping_add((i as u32).wrapping_mul(0x24)));
                        if a != -1 && i1 < nn {
                            let mut j = i1;
                            let mut hit = a == m.i32(nb.wrapping_add((i1 as u32).wrapping_mul(0x24)));
                            while !hit {
                                j += 1;
                                if nn <= j {
                                    break;
                                }
                                hit = a == m.i32(nb.wrapping_add((j as u32).wrapping_mul(0x24)));
                            }
                            if hit {
                                ret = 1;
                            }
                        }
                        if !(i1 < nn - 1 && ret == 0) {
                            break;
                        }
                        i = i1;
                    }
                }
            }
            m.set_i32(ed + 8, -2);
            m.set_i32(ed + 0xc, -2);
        }
        // the numbering packed down
        let ne = m.i32(mesh + mo::NB_EDGES);
        let tmap = m.si_calloc((ne.wrapping_add(1).wrapping_mul(4)) as u32, 4);
        let mut removed = 0i32;
        let mut i = 1i32;
        if 0 < ne {
            loop {
                let r = edge_rec(m, mesh, i);
                let (ra, rb) = (m.i32(r + 8), m.i32(r + 0xc));
                let mut drop = false;
                if ra == -2 || rb == -2 {
                    drop = true;
                } else if ra == -3 {
                    if rb == -3 {
                        drop = true;
                    } else {
                        m.set_i32(r + 8, -1);
                    }
                } else if rb == -3 {
                    m.set_i32(r + 0xc, -1);
                }
                if drop {
                    removed += 1;
                } else {
                    let u = i - removed;
                    let d = edge_rec(m, mesh, u);
                    for k in 0..5 {
                        m.set_u32(d + 4 * k, m.u32(r + 4 * k));
                    }
                    m.set_i32(tmap.wrapping_add((i as u32).wrapping_mul(4)), u);
                    let g3 = m.u32(SI_GBL_INDEX3);
                    if g3 != 0 && track != 0 {
                        m.set_i32(g3.wrapping_add((i as u32).wrapping_mul(4)), u);
                    }
                }
                i += 1;
                if i > m.i32(mesh + mo::NB_EDGES) {
                    break;
                }
            }
        }
        m.set_i32(mesh + mo::NB_EDGES, m.i32(mesh + mo::NB_EDGES).wrapping_sub(removed));
        removed = 0;
        let map2_len = (np(m) as u32).wrapping_mul(4);
        let map2 = m.si_malloc(map2_len);
        // The original looks a polygon number up in `map2` without a bound; a stale number (an edge
        // or star entry still naming a polygon that the compression above moved) reads past the
        // 4*np bytes. What lies there is the allocator's bookkeeping or unwritten memory, never a
        // table entry. The image leaves the bytes behind a block as it found them (zero), which
        // would name polygon 0 and remove it; the original's answer is a number no polygon has
        // (checked: SI_removeHolesFromMesh case 140 of re/out/tri1/holes), so the port answers one.
        let map2_get = |m: &Mem, i: i32| -> u32 {
            let off = (i as u32).wrapping_mul(4);
            if off >= map2_len {
                MAP2_PAST_END
            } else {
                m.u32(map2.wrapping_add(off))
            }
        };
        let mut poly = m.u32(mesh + mo::POLYGONS);
        for i in 0..np(m) {
            if m.u16(poly + mo::POLYGON_NB_NODES) == 0 {
                if m.u32(poly + 0xc) != 0 {
                    nodes_dispose(m, poly + 0xc, -1, -1);
                }
                removed += 1;
            } else {
                let d = m.u32(mesh + mo::POLYGONS).wrapping_add(((i - removed) as u32).wrapping_mul(0x18));
                for k in 0..6 {
                    m.set_u32(d + 4 * k, m.u32(poly + 4 * k));
                }
                m.set_i32(map2.wrapping_add((i as u32).wrapping_mul(4)), i - removed);
            }
            poly = poly.wrapping_add(0x18);
        }
        m.set_i32(mesh + mo::NB_POLYGONS, np(m).wrapping_sub(removed));
        let map_edge = |m: &Mem, x: i32| {
            if x < 0 {
                m.i32(tmap.wrapping_add((x.wrapping_neg() as u32).wrapping_mul(4))).wrapping_neg()
            } else {
                m.i32(tmap.wrapping_add((x as u32).wrapping_mul(4)))
            }
        };
        for vi in 0..m.i32(mesh + mo::NB_VERTICES) {
            let vr = vtx(m, mesh, vi);
            for k in 0..m.u16(vr + mo::VERTEX_NB_STAR) as u32 {
                let s = m.u32(vr + 0xc).wrapping_add(12 * k);
                let ne2 = map_edge(m, m.i32(s + 4));
                m.set_i32(s + 4, ne2);
                if m.i32(s + 8) != -1 {
                    m.set_u32(s + 8, map2_get(m, m.i32(s + 8)));
                }
            }
        }
        i = 1;
        if 0 < m.i32(mesh + mo::NB_EDGES) {
            loop {
                let r = edge_rec(m, mesh, i);
                if m.i32(r + 8) != -1 {
                    let t = map2_get(m, m.i32(r + 8)) as i32;
                    m.set_i32(r + 8, t);
                    if t == m.i32(r + 0xc) {
                        ret = 1;
                    }
                }
                if m.i32(r + 0xc) != -1 {
                    let t = map2_get(m, m.i32(r + 0xc)) as i32;
                    m.set_i32(r + 0xc, t);
                    if m.i32(r + 8) == t {
                        ret = 1;
                    }
                }
                i += 1;
                if i > m.i32(mesh + mo::NB_EDGES) {
                    break;
                }
            }
        }
        let mut poly = m.u32(mesh + mo::POLYGONS);
        for _ in 0..np(m) {
            for j in 0..m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
                let c = node_at(m, poly, j);
                if m.i32(c) != -1 {
                    let ne2 = map_edge(m, m.i32(c + 4));
                    m.set_i32(c + 4, ne2);
                }
            }
            poly = poly.wrapping_add(0x18);
        }
        if tmap != 0 {
            m.free(tmap);
        }
        if map2 != 0 {
            m.free(map2);
        }
        set_edge_flag(m, mesh);
        m.set_sp(sp);
        return ret as i32;
    }
    // an edge with one polygon
    let g2 = m.u32(SI_GBL_INDEX2);
    if g2 != 0 {
        let n = np(m);
        let slot = |i: i32| g2.wrapping_add((i as u32).wrapping_mul(4));
        if pa == n - 1 {
            m.set_i32(slot(n - 1), -1);
        } else {
            m.set_i32(slot(n - 1), pa);
            m.set_i32(slot(pa), -1);
        }
    }
    super::edit::mesh_remove_ipolygon(m, mesh, pa, 1, track);
    rebuild::compute_hard_edge(m, mesh, -1);
    if m.u8(mesh + mo::FLAGS) & 8 != 0 {
        rebuild::computes_vertex_normal(m, mesh, -1);
    }
    m.set_sp(sp);
    0
}

/// Corners `from..to` of the list at the address held in `cell`... (the list block itself) appended
/// to `buf` at `*nn` by [`nodes_copy`] without models.
fn append_corners_from(m: &mut Mem, buf: u32, nn: &mut i32, cell: u32, from: i32, to: i32) {
    let list = m.u32(cell);
    let mut i = from;
    while i < to {
        let d = buf.wrapping_add((*nn as u32).wrapping_mul(0x24));
        let s = list.wrapping_add((i as u32).wrapping_mul(0x24));
        nodes_copy(m, 0, 0, d, s);
        *nn += 1;
        i += 1;
    }
}

/// The factor `_DAT_0865fa1c` of the midpoints: 0.5 as `f32`.
const HALF: f32 = 0.5;

/// What `SI_removeEdgeFromMesh`'s polygon table answers for an index past its end: not a polygon number.
const MAP2_PAST_END: u32 = 0x7fff_fff8;

/// [soft 0x084518e0 si_collapseEdgeInMesh]: edge `e` collapsed: its first vertex `v0` is merged
/// into its second `v1`. A copy of the mesh is kept. While `v0` has polygons, the last one
/// of its star that is one of the edge's two polygons (else the last of the star) is taken out
/// of the mesh ([`super::edit::mesh_remove_ipolygon`], no compression, no tracking). One that
/// holds the edge has each contour rewritten without the corners at `v0` and without repeats
/// of the corner before; the texture pair and colour of the last dropped corner are kept and
/// the corners at `v1` take the mean of their pair and that pair (and the unsigned half sum of
/// the colours), the closing repeat dropped; a polygon left with under 3 corners (a contour
/// under 2) is dropped: counted in a list (`SI_realloc` of 16 bytes per entry), its place in
/// `SI_gblIndex2` fixed up when installed. Any other polygon has its corners at `v0` named
/// `v1` (and the first vertex of their edge too: the original reads one corner past the end),
/// and every kept polygon is filed again ([`rebuild::insert_ipolygon`]). A refusal empties the
/// mesh and puts the copy back with "initial or resulting mesh is corrupted, can't collapse
/// edge" (an uncited hook, `SI_statusDisplayMessage (0x083cca94)`; the list is then not
/// freed). Else `v1` moves to the midpoint of the two vertices (sums at 80 bits, halved,
/// stored as `f32`), the normals and hard edges are computed, `v0` is compressed away, and the
/// dropped polygons (sorted by `qsort` and [`compare_indices`], from the highest) compressed
/// away.
pub fn collapse_edge_in_mesh(m: &mut Mem, o: &mut dyn Outside, mesh: u32, e: i32) {
    const SI_GBL_INDEX2: u32 = 0x08788970;
    let sp = m.sp();
    let mut removed5c = 0i32;
    let mut ok = 1i16;
    let backup = m.local(4);
    let l28 = m.local(0x28); // local_28: a corner whose pair and colour are kept
    for k in 0..10 {
        m.set_u32(l28 + 4 * k, 0);
    }
    let copy = super::obj::mesh_copy(m, o, mesh);
    m.set_u32(backup, copy);
    let mut list = 0u32; // local_74
    let mut n78 = 0i32; // local_78
    let ed = edge_rec(m, mesh, e);
    let (v0, v1, pa, pb) = (m.i32(ed), m.i32(ed + 4), m.i32(ed + 8), m.i32(ed + 0xc));
    let vr0 = vtx(m, mesh, v0);
    let vr1 = vtx(m, mesh, v1);
    let half = HALF;
    let mut s8 = n78;
    let mut gate = m.u16(vr0 + mo::VERTEX_NB_STAR) as i16;
    let mut failed = false;
    loop {
        n78 = s8;
        if gate == 0 {
            break;
        }
        let mut l38 = -1i32;
        let nstar = m.u16(vr0 + mo::VERTEX_NB_STAR) as u32;
        if nstar != 0 {
            let mut p = m.u32(vr0 + 0xc).wrapping_add(8);
            for _ in 0..nstar {
                let q = m.i32(p);
                if (q == pa || q == pb) && q != -1 {
                    l38 = q;
                }
                p = p.wrapping_add(12);
            }
        }
        if l38 == -1 && nstar != 0 {
            let mut p = m.u32(vr0 + 0xc).wrapping_add(8);
            for _ in 0..nstar {
                if m.i32(p) != -1 {
                    l38 = m.i32(p);
                }
                p = p.wrapping_add(12);
            }
        }
        let poly = super::polygon_at(m, mesh, l38);
        let mut keep = true;
        ok = super::edit::mesh_remove_ipolygon(m, mesh, l38, 0, 0) as i16;
        if ok != 0 {
            if l38 == pa || l38 == pb {
                let mut start = 0i32;
                let mut out = 0i32;
                let mut u = 0i32;
                let count = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
                loop {
                    if u == count || m.i32(node_at(m, poly, u)) == -1 {
                        let end = u - 1;
                        if start != 0 {
                            let c = node_at(m, poly, out);
                            m.set_u32(c, 0xffff_ffff);
                            out += 1;
                            m.set_u32(c + 4, 0);
                        }
                        let begin = out;
                        let mut k = start;
                        while k <= end {
                            let c = node_at(m, poly, k);
                            let w = m.i32(c);
                            if w == v0 {
                                super::obj::nodes_copy_uv(m, 0, 0, l28, c);
                                m.set_u32(l28 + 0x1c, m.u32(c + 0x1c));
                            } else if out == begin || w != m.i32(node_at(m, poly, out - 1)) {
                                let d = node_at(m, poly, out);
                                m.set_u32(d, w as u32);
                                m.set_u32(d + 4, 0);
                                for off in [8u32, 0xc, 0x10] {
                                    m.set_u32(d + off, m.u32(c + off));
                                }
                                super::obj::nodes_copy_uv(m, 0, 0, d, c);
                                m.set_u32(d + 0x1c, m.u32(c + 0x1c));
                                out += 1;
                            }
                            k += 1;
                        }
                        k = start;
                        while k <= end {
                            let c = node_at(m, poly, k);
                            if m.i32(c) == v1 {
                                let a = ((x(m.f32(c + 0x14)) + x(m.f32(l28 + 0x14))) * x(half)).to_f32();
                                m.set_f32(c + 0x14, a);
                                let b = ((x(m.f32(c + 0x18)) + x(m.f32(l28 + 0x18))) * x(half)).to_f32();
                                m.set_f32(c + 0x18, b);
                                let col = m.u32(l28 + 0x1c).wrapping_add(m.u32(c + 0x1c)) >> 1;
                                m.set_u32(c + 0x1c, col);
                            }
                            k += 1;
                        }
                        if m.i32(node_at(m, poly, out - 1)) == m.i32(node_at(m, poly, begin)) {
                            out -= 1;
                        }
                        if start == 0 {
                            if out - begin < 3 {
                                keep = false;
                                u = count;
                            }
                        } else if out - begin < 2 {
                            out = begin - 1;
                        }
                        start = u + 1;
                    }
                    u += 1;
                    if u > count {
                        break;
                    }
                }
                m.set_u16(poly + mo::POLYGON_NB_NODES, out as u16);
                if !keep {
                    n78 = s8 + 1;
                    let g2 = m.u32(SI_GBL_INDEX2);
                    if g2 != 0 {
                        let at = m.i32(mesh + mo::NB_POLYGONS).wrapping_sub(removed5c).wrapping_sub(1);
                        let slot = |i: i32| g2.wrapping_add((i as u32).wrapping_mul(4));
                        if l38 == at {
                            m.set_i32(slot(l38), -1);
                        } else {
                            if l38 < at {
                                m.set_i32(slot(at), l38);
                            } else {
                                let mut i = 0;
                                while i < l38 {
                                    let t = m.i32(slot(i));
                                    if m.i32(slot(l38)) < t {
                                        m.set_i32(slot(i), t - 1);
                                    }
                                    i += 1;
                                }
                            }
                            m.set_i32(slot(l38), -1);
                        }
                        removed5c += 1;
                    }
                    list = m.si_realloc(list, (n78 as u32).wrapping_mul(0x10));
                    m.set_i32(list.wrapping_add((s8 as u32).wrapping_mul(4)), l38);
                    s8 = n78;
                    gate = ok;
                    if m.u16(vr0 + mo::VERTEX_NB_STAR) == 0 {
                        break;
                    }
                    continue;
                }
            } else {
                let mut k = 0i32;
                while k <= m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
                    let c = node_at(m, poly, k);
                    if m.i32(c) == v0 {
                        m.set_i32(c, v1);
                        let en = m.i32(c + 4);
                        let r = edge_rec(m, mesh, en);
                        m.set_i32(r, v1);
                    }
                    k += 1;
                }
            }
            ok = rebuild::insert_ipolygon(m, o, mesh, l38) as i16;
            if ok == 0 {
                failed = true;
                break;
            }
        }
        s8 = n78;
        gate = ok;
        if m.u16(vr0 + mo::VERTEX_NB_STAR) == 0 {
            break;
        }
    }
    if failed || ok == 0 {
        super::obj::mesh_delete(m, mesh);
        let b = m.u32(backup);
        m.copy(mesh, b, 0x54);
        if b != 0 {
            m.free(b);
        }
        m.set_u32(backup, 0);
        o.call(m, &super::callee::SI_STATUS_DISPLAY_MESSAGE, &[0x086d4f40, 3]);
    } else {
        for k in 0..3u32 {
            let s = ((x(m.f32(vr1 + 4 * k)) + x(m.f32(vr0 + 4 * k))) * x(half)).to_f32();
            m.set_f32(vr1 + 4 * k, s);
        }
        rebuild::computes_polygon_normal(m, mesh, -1);
        rebuild::compute_hard_edge(m, mesh, -1);
        rebuild::computes_vertex_normal(m, mesh, -1);
        super::edit::mesh_compress_vertices_array(m, mesh, v0);
        si_mem::clib::qsort(m, list, n78 as u32, 4, &mut |m, a, b| compare_indices(m, a, b));
        for k in 0..n78 {
            let n = m.i32(list.wrapping_add((k as u32).wrapping_mul(4)));
            super::edit::mesh_compress_polygons_array(m, mesh, n);
        }
        if list != 0 {
            m.free(list);
        }
        super::obj::mesh_dispose(m, backup);
    }
    m.set_sp(sp);
}

/// [soft 0x08451f1c si_collapsePolygonInMesh]: polygon `n` collapsed to a point: a vertex at
/// the mean of its corners' vertices (each coordinate summed as `f32`, each sum taken at 80
/// bits, the total divided by the corner count at 80 bits and stored as `f32`) is added
/// ([`add_vertex_to_mesh`]); the polygons that use any of the corner vertices are collected
/// (once each, `SI_realloc` of 16 bytes per entry), sorted ([`compare_indices`]) and, one at a
/// time, taken out ([`super::edit::mesh_remove_ipolygon`], no compression, no tracking),
/// the first corner at any of the vertices renamed to the new vertex, every other such corner
/// and every repeat dropped from its contour (a contour left with under 2 corners is dropped,
/// the outer one with under 3 drops the polygon), and filed again when kept. A refusal puts
/// the copy of the mesh back with "initial or resulting mesh is corrupted, can't collapse
/// polygon" (an uncited hook, `SI_statusDisplayMessage (0x083cca94)`; the three arrays are
/// then not freed). Else the normals and hard edges are computed, the dropped polygons
/// compressed away (from the highest; `SI_gblIndex2` fixed up when installed), the corner
/// vertices sorted and compressed away.
pub fn collapse_polygon_in_mesh(m: &mut Mem, o: &mut dyn Outside, mesh: u32, n: i32) {
    const SI_GBL_INDEX2: u32 = 0x08788970;
    let sp = m.sp();
    let mut removed4c = 0i32;
    let mut ok = 1i16;
    let backup = m.local(4);
    let copy = super::obj::mesh_copy(m, o, mesh);
    m.set_u32(backup, copy);
    let mut list = 0u32;
    let mut nlist = 0i32;
    let (mut sx, mut sy, mut sz) = (0f32, 0f32, 0f32);
    let poly = super::polygon_at(m, mesh, n);
    let nmemb = m.u16(poly + mo::POLYGON_NB_NODES) as u32;
    let base = m.si_malloc(nmemb << 2);
    let bslot = |k: i32| base.wrapping_add((k as u32).wrapping_mul(4));
    let mut k = 0i32;
    if nmemb != 0 {
        loop {
            let c = node_at(m, poly, k);
            let w = m.i32(c);
            m.set_i32(bslot(k), w);
            let v = vtx(m, mesh, w);
            sx = (x(sx) + x(m.f32(v))).to_f32();
            sy = (x(sy) + x(m.f32(v + 4))).to_f32();
            k += 1;
            sz = (x(sz) + x(m.f32(v + 8))).to_f32();
            if k >= nmemb as i32 {
                break;
            }
        }
    }
    let cnt = x(m.u16(poly + mo::POLYGON_NB_NODES) as f32);
    let (cx, cy, cz) = ((x(sx) / cnt).to_f32(), (x(sy) / cnt).to_f32(), (x(sz) / cnt).to_f32());
    add_vertex_to_mesh(m, o, mesh, cx, cy, cz, 0xffff_ffff);
    let nv = m.i32(mesh + mo::NB_VERTICES);
    for k in 0..nmemb as i32 {
        let vr = vtx(m, mesh, m.i32(bslot(k)));
        let mut j = 0u32;
        while j < m.u16(vr + mo::VERTEX_NB_STAR) as u32 {
            let q = m.i32(m.u32(vr + 0xc).wrapping_add(8 + 12 * j));
            j += 1;
            if q != -1 {
                let mut fresh = true;
                for t in 0..nlist {
                    if m.i32(list.wrapping_add((t as u32).wrapping_mul(4))) == q {
                        fresh = false;
                    }
                }
                if fresh {
                    list = m.si_realloc(list, ((nlist + 1) as u32).wrapping_mul(0x10));
                    m.set_i32(list.wrapping_add((nlist as u32).wrapping_mul(4)), q);
                    nlist += 1;
                }
            }
        }
    }
    si_mem::clib::qsort(m, list, nlist as u32, 4, &mut |m, a, b| compare_indices(m, a, b));
    let flags = m.si_malloc((nlist as u32).wrapping_mul(2));
    let fslot = |i: i32| flags.wrapping_add((i as u32).wrapping_mul(2));
    let mut failed = false;
    let mut i = 0i32;
    while i < nlist {
        m.set_u16(fslot(i), 1);
        let p = m.i32(list.wrapping_add((i as u32).wrapping_mul(4)));
        let pr = super::polygon_at(m, mesh, p);
        ok = super::edit::mesh_remove_ipolygon(m, mesh, p, 0, 0) as i16;
        if ok != 0 {
            let mut first = true; // bVar2
            let mut start = 0i32;
            let mut out = 0i32;
            let mut u = 0i32;
            let mut count = m.u16(pr + mo::POLYGON_NB_NODES) as i32;
            loop {
                if u == count || m.i32(node_at(m, pr, u)) == -1 {
                    if start != 0 {
                        let c = node_at(m, pr, out);
                        out += 1;
                        m.set_u32(c, 0xffff_ffff);
                        m.set_u32(c + 4, 0);
                    }
                    let begin = out;
                    let mut k = start;
                    while k <= u - 1 {
                        let mut skip = false;
                        for t in 0..nmemb as i32 {
                            if m.i32(node_at(m, pr, k)) == m.i32(bslot(t)) {
                                if first {
                                    m.set_i32(node_at(m, pr, k), nv - 1);
                                    first = false;
                                } else {
                                    skip = true;
                                }
                            }
                        }
                        if !skip && (out == begin || m.i32(node_at(m, pr, k)) != m.i32(node_at(m, pr, out - 1))) {
                            let (d, s) = (node_at(m, pr, out), node_at(m, pr, k));
                            m.set_u32(d, m.u32(s));
                            m.set_u32(d + 4, 0);
                            for off in [8u32, 0xc, 0x10] {
                                m.set_u32(d + off, m.u32(s + off));
                            }
                            super::obj::nodes_copy_uv(m, 0, 0, d, s);
                            m.set_u32(d + 0x1c, m.u32(s + 0x1c));
                            out += 1;
                        }
                        k += 1;
                    }
                    if m.i32(node_at(m, pr, out - 1)) == m.i32(node_at(m, pr, begin)) {
                        out -= 1;
                    }
                    if start == 0 {
                        if out - begin < 3 {
                            u = m.u16(pr + mo::POLYGON_NB_NODES) as i32;
                            m.set_u16(fslot(i), 0);
                        }
                    } else if out - begin < 2 {
                        out = begin - 1;
                    }
                    start = u + 1;
                    count = m.u16(pr + mo::POLYGON_NB_NODES) as i32;
                }
                u += 1;
                if u > count {
                    break;
                }
            }
            m.set_u16(pr + mo::POLYGON_NB_NODES, out as u16);
        }
        if m.u16(fslot(i)) != 0 {
            ok = rebuild::insert_ipolygon(m, o, mesh, p) as i16;
            if ok == 0 {
                failed = true;
                break;
            }
        }
        i += 1;
    }
    if failed || ok == 0 {
        super::obj::mesh_delete(m, mesh);
        let b = m.u32(backup);
        m.copy(mesh, b, 0x54);
        if b != 0 {
            m.free(b);
        }
        m.set_u32(backup, 0);
        o.call(m, &super::callee::SI_STATUS_DISPLAY_MESSAGE, &[0x086d4f80, 3]);
    } else {
        rebuild::computes_polygon_normal(m, mesh, -1);
        rebuild::compute_hard_edge(m, mesh, -1);
        for i in 0..nlist {
            let p = m.i32(list.wrapping_add((i as u32).wrapping_mul(4)));
            if m.u16(fslot(i)) != 1 {
                let g2 = m.u32(SI_GBL_INDEX2);
                if g2 != 0 {
                    let at = m.i32(mesh + mo::NB_POLYGONS).wrapping_sub(removed4c).wrapping_sub(1);
                    let slot = |i: i32| g2.wrapping_add((i as u32).wrapping_mul(4));
                    if p == at {
                        m.set_i32(slot(p), -1);
                    } else {
                        if p < at {
                            m.set_i32(slot(at), p);
                        } else {
                            let mut j = 0;
                            while j < p {
                                let t = m.i32(slot(j));
                                if m.i32(slot(p)) < t {
                                    m.set_i32(slot(j), t - 1);
                                }
                                j += 1;
                            }
                        }
                        m.set_i32(slot(p), -1);
                    }
                    removed4c += 1;
                }
                super::edit::mesh_compress_polygons_array(m, mesh, p);
            }
        }
        rebuild::computes_vertex_normal(m, mesh, -1);
        si_mem::clib::qsort(m, base, nmemb, 4, &mut |m, a, b| compare_indices(m, a, b));
        for k in 0..nmemb as i32 {
            super::edit::mesh_compress_vertices_array(m, mesh, m.i32(bslot(k)));
        }
        if list != 0 {
            m.free(list);
        }
        if flags != 0 {
            m.free(flags);
        }
        if base != 0 {
            m.free(base);
        }
        super::obj::mesh_dispose(m, backup);
    }
    m.set_sp(sp);
}

/// Whether `val` is the vertex of a star entry of vertex record `vr`.
fn star_names(m: &Mem, vr: u32, val: i32) -> bool {
    let n = m.u16(vr + mo::VERTEX_NB_STAR) as u32;
    let star = m.u32(vr + 0xc);
    (0..n).any(|k| m.i32(star.wrapping_add(12 * k)) == val)
}

/// The four messages of an edge/vertex mismatch: `fmts` the texts, `head` the second
/// message's numbers, the vertex-record words of the edge next.
fn report_edge_mismatch(m: &mut Mem, o: &mut dyn Outside, fmts: [u32; 4], head: [u32; 4], rec: u32) {
    let _ = o.call(m, &super::callee::PRINTF, &[fmts[0]]);
    let _ = o.call(m, &super::callee::PRINTF, &[fmts[1], head[0], head[1], head[2], head[3]]);
    let words = [m.u32(rec), m.u32(rec + 4), m.u32(rec + 8), m.u32(rec + 12)];
    let _ = o.call(m, &super::callee::PRINTF, &[fmts[2], words[0], words[1], words[2], words[3]]);
    let _ = o.call(m, &super::callee::PRINTF, &[fmts[3]]);
}

/// [soft 0x084525ec SI_validMesh]: see finding 0015, 2026-10-28 (second part).
///
/// Answers 1 for a mesh whose books agree, else 0 at the first fault; with `track` not 0 the
/// fault is first told with `printf` (the texts of the binary).
pub fn valid_mesh(m: &mut Mem, o: &mut dyn Outside, mesh: u32, track: i16) -> u32 {
    let sp = m.sp();
    let r = valid_mesh_body(m, o, mesh, track);
    m.set_sp(sp);
    r
}

fn valid_mesh_body(m: &mut Mem, o: &mut dyn Outside, mesh: u32, track: i16) -> u32 {
    use super::callee::PRINTF;
    // vertices: every star entry names an edge that names the vertex and the polygon back
    let mut vr = m.u32(mesh + mo::VERTICES);
    let mut v = 0i32;
    if 0 < m.i32(mesh + mo::NB_VERTICES) {
        loop {
            let mut j = 0i32;
            if m.u16(vr + mo::VERTEX_NB_STAR) != 0 {
                let mut cc = 0u32;
                loop {
                    let star = m.u32(vr + 0xc);
                    let e = m.i32(star.wrapping_add(4 + cc));
                    if e != 0 {
                        let rec = edge_abs(m, mesh, e);
                        let poly = m.u32(m.u32(vr + 0xc).wrapping_add(8 + cc));
                        let (first, second, f1, f2) = if e < 0 {
                            (m.i32(rec + 4), m.i32(rec + 12), [0x086d4fbf, 0x086d4fe0, 0x086d5020, 0x086d504b], [0x086d5060, 0x086d50a0, 0x086d50e0, 0x086d510b])
                        } else {
                            (m.i32(rec), m.i32(rec + 8), [0x086d510d, 0x086d5140, 0x086d5180, 0x086d51ab], [0x086d51c0, 0x086d5200, 0x086d5240, 0x086d526b])
                        };
                        let head = [v as u32, j as u32, e as u32, poly];
                        if first != v {
                            if track != 0 {
                                report_edge_mismatch(m, o, f1, head, rec);
                            }
                            return 0;
                        }
                        if second != poly as i32 {
                            if track != 0 {
                                report_edge_mismatch(m, o, f2, head, rec);
                            }
                            return 0;
                        }
                    }
                    cc = cc.wrapping_add(12);
                    j += 1;
                    if j >= m.u16(vr + mo::VERTEX_NB_STAR) as i32 {
                        break;
                    }
                }
            }
            v += 1;
            vr = vr.wrapping_add(0x14);
            if v >= m.i32(mesh + mo::NB_VERTICES) {
                break;
            }
        }
    }
    // polygons: the normal is the one computed, and every corner's edge joins it to the next
    let mut pr = m.u32(mesh + mo::POLYGONS);
    let mut p = 0i32;
    let mut np = m.i32(mesh + mo::NB_POLYGONS);
    if 0 < np {
        loop {
            let old = [m.u32(pr), m.u32(pr + 4), m.u32(pr + 8)];
            rebuild::computes_polygon_normal(m, mesh, p);
            let dx = (x(f32::from_bits(old[0])) - x(m.f32(pr))).abs();
            let dy = (x(f32::from_bits(old[1])) - x(m.f32(pr + 4))).abs();
            let dz = (x(f32::from_bits(old[2])) - x(m.f32(pr + 8))).abs();
            let dev = ((dx + dy) + dz).to_f32();
            if x(dev).gt(x(1e-4f64)) {
                for k in 0..3 {
                    m.set_u32(pr + 4 * k, old[k as usize]);
                }
                if track != 0 {
                    let _ = o.call(m, &PRINTF, &[0x086d5280, p as u32]);
                }
                return 0;
            }
            let mut start = 0i32;
            let mut k = 0i32;
            let cnt = m.u16(pr + mo::POLYGON_NB_NODES) as i32;
            if cnt != 0 {
                let mut cc = 0u32;
                loop {
                    let nodes = m.u32(pr + 0xc);
                    let vertex = m.i32(nodes.wrapping_add(cc));
                    if vertex == -1 {
                        start = k + 1;
                    } else {
                        let next = if k == cnt - 1 || m.i32(nodes.wrapping_add(cc).wrapping_add(0x24)) == -1 {
                            m.i32(nodes.wrapping_add((start as u32).wrapping_mul(0x24)))
                        } else {
                            m.i32(nodes.wrapping_add(cc).wrapping_add(0x24))
                        };
                        let e = m.i32(nodes.wrapping_add(cc + 4));
                        let rec = edge_abs(m, mesh, e);
                        let head = [p as u32, k as u32, m.u32(m.u32(pr + 0xc).wrapping_add(cc)), e as u32];
                        if e < 0 {
                            if m.i32(rec) != next || m.i32(rec + 4) != vertex {
                                if track != 0 {
                                    report_edge_mismatch(m, o, [0x086d52c0, 0x086d52e0, 0x086d5320, 0x086d534b], head, rec);
                                }
                                return 0;
                            }
                            if m.i32(rec + 12) != p {
                                if track != 0 {
                                    report_edge_mismatch(m, o, [0x086d5360, 0x086d53a0, 0x086d53e0, 0x086d540b], head, rec);
                                }
                                return 0;
                            }
                        } else {
                            if m.i32(rec) != vertex || m.i32(rec + 4) != next {
                                if track != 0 {
                                    report_edge_mismatch(m, o, [0x086d5420, 0x086d5440, 0x086d5480, 0x086d54ab], head, rec);
                                }
                                return 0;
                            }
                            if m.i32(rec + 8) != p {
                                if track != 0 {
                                    report_edge_mismatch(m, o, [0x086d54c0, 0x086d5500, 0x086d5540, 0x086d556b], head, rec);
                                }
                                return 0;
                            }
                        }
                    }
                    cc = cc.wrapping_add(0x24);
                    k += 1;
                    if k >= cnt {
                        break;
                    }
                }
            }
            p += 1;
            pr = pr.wrapping_add(0x18);
            np = m.i32(mesh + mo::NB_POLYGONS);
            if p >= np {
                break;
            }
        }
    }
    // polygons: every corner's vertex is a neighbour of the vertex before it in its ring
    let mut pr = m.u32(mesh + mo::POLYGONS);
    let mut p = 0i32;
    if 0 < np {
        loop {
            let mut start = 0i32;
            let mut idx = 0i32;
            let mut c0 = (-0x24i32) as u32;
            let mut c4 = 0u32;
            loop {
                let nodes = m.u32(pr + 0xc);
                if idx == m.u16(pr + mo::POLYGON_NB_NODES) as i32 || m.i32(nodes.wrapping_add(c4)) == -1 {
                    let mut prev2 = m.i32(nodes.wrapping_add(c0).wrapping_sub(0x24));
                    if start <= idx - 1 {
                        let mut bl = nodes.wrapping_add((start as u32).wrapping_mul(0x24));
                        let mut prev = m.i32(nodes.wrapping_add(c0));
                        loop {
                            let cur = m.i32(bl);
                            let vr = vtx(m, mesh, prev);
                            if !star_names(m, vr, cur) || !star_names(m, vr, prev2) {
                                if track != 0 {
                                    let args = if !star_names(m, vr, cur) { [0x086d5580, p as u32, cur as u32] } else { [0x086d55c0, p as u32, prev2 as u32] };
                                    let _ = o.call(m, &PRINTF, &args);
                                }
                                return 0;
                            }
                            prev2 = prev;
                            bl = bl.wrapping_add(0x24);
                            start += 1;
                            prev = cur;
                            if start > idx - 1 {
                                break;
                            }
                        }
                    }
                    start = idx + 1;
                }
                c0 = c0.wrapping_add(0x24);
                c4 = c4.wrapping_add(0x24);
                idx += 1;
                if idx > m.u16(pr + mo::POLYGON_NB_NODES) as i32 {
                    break;
                }
            }
            p += 1;
            pr = pr.wrapping_add(0x18);
            if p >= np {
                break;
            }
        }
    }
    // edges: one with no polygon at all is a fault; the border flag (0x20) is set exactly
    // when one side has none
    let mut e = 1i32;
    if 0 < m.i32(mesh + mo::NB_EDGES) {
        loop {
            let rec = edge_rec(m, mesh, e);
            let (pa, pb) = (m.i32(rec + 8), m.i32(rec + 12));
            if pa == -1 && pb == -1 {
                if track != 0 {
                    let _ = o.call(m, &PRINTF, &[0x086d5600, e as u32]);
                }
                return 0;
            }
            if ((pa == -1) != (pb == -1)) as u16 != (m.u16(rec + 0x12) >> 5) & 1 {
                if track != 0 {
                    let _ = o.call(m, &PRINTF, &[0x086d5640, e as u32]);
                }
                return 0;
            }
            e += 1;
            if m.i32(mesh + mo::NB_EDGES) < e {
                break;
            }
        }
    }
    // polygons with holes: each hole's corners lie inside the rest, in the plane of the polygon
    let table = m.si_malloc((m.i32(mesh + mo::NB_VERTICES) as u32).wrapping_mul(12));
    let mut pr = m.u32(mesh + mo::POLYGONS);
    let mut p = 0i32;
    if 0 < m.i32(mesh + mo::NB_POLYGONS) {
        loop {
            let mut holed = false;
            let mut left = m.u16(pr + mo::POLYGON_NB_NODES) as u32;
            if left != 0 {
                let mut c = m.u32(pr + 0xc);
                while left != 0 {
                    if m.i32(c) == -1 {
                        holed = true;
                    }
                    c = c.wrapping_add(0x24);
                    left -= 1;
                }
            }
            if holed {
                let mat = si_math::algebra::matrix_z_align(&Vector::from_array([m.f32(pr), m.f32(pr + 4), m.f32(pr + 8)]));
                let mut k = 0i32;
                if m.u16(pr + mo::POLYGON_NB_NODES) != 0 {
                    let mut cc = 0u32;
                    loop {
                        let vi = m.i32(m.u32(pr + 0xc).wrapping_add(cc));
                        if vi != -1 {
                            let vp = vtx(m, mesh, vi);
                            let (vx, vy, vz) = (x(m.f32(vp)), x(m.f32(vp + 4)), x(m.f32(vp + 8)));
                            let at = table.wrapping_add((vi as u32).wrapping_mul(12));
                            let t = |col: usize| ((vx * x(mat[0][col]) + vy * x(mat[1][col])) + vz * x(mat[2][col]) + x(mat[3][col])).to_f32();
                            m.set_f32(at, t(0));
                            m.set_f32(at + 4, t(1));
                            m.set_f32(at + 8, t(2));
                        }
                        cc = cc.wrapping_add(0x24);
                        k += 1;
                        if k >= m.u16(pr + mo::POLYGON_NB_NODES) as i32 {
                            break;
                        }
                    }
                }
                let pp = m.local(4);
                m.set_u32(pp, 0);
                let mut ring = 0i32;
                let mut idx = 0i32;
                let mut cnt = m.u16(pr + mo::POLYGON_NB_NODES) as i32;
                let mut off = 0u32;
                loop {
                    if idx == cnt || m.i32(m.u32(pr + 0xc).wrapping_add(off)) == -1 {
                        if ring == 0 {
                            ring = idx + 1;
                        } else {
                            let last = idx - 1;
                            nodes_allocate(m, 0, pp, 0, cnt);
                            let mut kept = 0i32;
                            let mut i = 0i32;
                            if m.u16(pr + mo::POLYGON_NB_NODES) != 0 {
                                let (mut src, mut dst) = (0u32, 0u32);
                                loop {
                                    if i < ring - 1 || last < i {
                                        let w = m.u32(m.u32(pr + 0xc).wrapping_add(src));
                                        m.set_u32(m.u32(pp).wrapping_add(dst), w);
                                        dst = dst.wrapping_add(0x24);
                                        kept += 1;
                                    }
                                    src = src.wrapping_add(0x24);
                                    i += 1;
                                    if i >= m.u16(pr + mo::POLYGON_NB_NODES) as i32 {
                                        break;
                                    }
                                }
                            }
                            if ring <= last {
                                let mut at = (ring as u32).wrapping_mul(0x24);
                                loop {
                                    let vi = m.i32(m.u32(pr + 0xc).wrapping_add(at));
                                    let q = table.wrapping_add((vi as u32).wrapping_mul(12));
                                    let (qx, qy, qz) = (m.f32(q), m.f32(q + 4), m.f32(q + 8));
                                    let inside = point_inside_contour(m, qx, qy, qz, table, kept, m.u32(pp)) as u16 as i16;
                                    if inside == 0 {
                                        if table != 0 {
                                            m.free(table);
                                        }
                                        if track != 0 {
                                            let _ = o.call(m, &PRINTF, &[0x086d5680, ring as u32, p as u32]);
                                        }
                                        return 0;
                                    }
                                    at = at.wrapping_add(0x24);
                                    ring += 1;
                                    if ring > last {
                                        break;
                                    }
                                }
                            }
                            nodes_dispose(m, pp, -1, m.u16(pr + mo::POLYGON_NB_NODES) as i32);
                            ring = idx + 1;
                        }
                    }
                    off = off.wrapping_add(0x24);
                    idx += 1;
                    cnt = m.u16(pr + mo::POLYGON_NB_NODES) as i32;
                    if idx > cnt {
                        break;
                    }
                }
            }
            p += 1;
            pr = pr.wrapping_add(0x18);
            if p >= m.i32(mesh + mo::NB_POLYGONS) {
                break;
            }
        }
    }
    if table != 0 {
        m.free(table);
    }
    1
}

/// The geometries of a model, in the order `SI_modelNextDefGet` gives them.
fn defs_of(m: &mut Mem, model: u32) -> Vec<u32> {
    let cursor = m.local(4);
    m.set_u32(cursor, 0);
    let mut out = Vec::new();
    loop {
        let d = crate::model_image::touch::next_def_get(m, si_mem::Ptr32::new(model), cursor);
        if d == 0 {
            break;
        }
        out.push(d);
    }
    m.release(4);
    out
}

/// Every corner of every polygon of a geometry, as (corner address); the polygon array and the
/// corner arrays are read as the walk reaches them.
fn corners_of(m: &Mem, def: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut i = 0i32;
    if 0 < m.i32(def + 0x18) {
        loop {
            let poly = m.u32(def + 4).wrapping_add((i as u32).wrapping_mul(0x18));
            i += 1;
            let mut k = 0i32;
            if m.u16(poly + 0x10) != 0 {
                loop {
                    out.push(m.u32(poly + 0xc).wrapping_add((k as u32).wrapping_mul(0x24)));
                    k += 1;
                    if k >= m.u16(poly + 0x10) as i32 {
                        break;
                    }
                }
            }
            if i >= m.i32(def + 0x18) {
                break;
            }
        }
    }
    out
}

/// [soft 0x08453584 SI_meshPopulateUVDomains]: for a model with more than one texture domain
/// (+0x424), every corner of every geometry gets its first texture pair copied into the
/// pairs of all 7 further domains but domain `which` − 1. Answers 1, else 0.
pub fn mesh_populate_uv_domains(m: &mut Mem, model: u32, which: i32) -> u32 {
    if m.i32(model + 0x424) < 2 {
        return 0;
    }
    let sp = m.sp();
    for def in defs_of(m, model) {
        for c in corners_of(m, def) {
            for t in 0..7i32 {
                if t != which.wrapping_sub(1) {
                    let ex = m.u32(c + 0x20);
                    let at = ex.wrapping_add((t as u32).wrapping_mul(8));
                    m.set_u32(at, m.u32(c + 0x14));
                    m.set_u32(at + 4, m.u32(c + 0x18));
                }
            }
        }
    }
    m.set_sp(sp);
    1
}

/// [soft 0x084536f8 SI_meshAssociateTxtUVDomain]: texture `tex` taken out of the list of the
/// domain that holds it (the list of domain d is at `*(+0x428) + 8 (d - 1)`: an array and its
/// count; the first match shifts the rest down, an emptied list is freed, else shrunk) and
/// put at the end of the list of domain `dom` (a new list, or one grown by 4 words) when that
/// is another domain between 1 and the model's count. Answers 1; 0 for no model.
pub fn mesh_associate_txt_uv_domain(m: &mut Mem, model: u32, tex: i32, dom: i32) -> u32 {
    if model == 0 {
        return 0;
    }
    let tab = |m: &Mem| m.u32(model + 0x428);
    let slot = |m: &Mem, d: i32, k: u32| tab(m).wrapping_add((d as u32).wrapping_mul(8)).wrapping_add(k).wrapping_sub(8);
    let mut found = 0i32;
    let mut i = 0i32;
    if 0 < m.i32(model + 0x424).wrapping_sub(1) {
        loop {
            let cnt = m.i32(tab(m).wrapping_add(4 + (i as u32).wrapping_mul(8)));
            let d = i + 1;
            let mut j = 0i32;
            while j < cnt {
                let arr = m.u32(tab(m).wrapping_add((i as u32).wrapping_mul(8)));
                if m.i32(arr.wrapping_add((j as u32).wrapping_mul(4))) == tex {
                    found = d;
                }
                j += 1;
            }
            i = d;
            if i >= m.i32(model + 0x424).wrapping_sub(1) {
                break;
            }
        }
    }
    if found != 0 {
        if found == dom {
            return 1;
        }
        let mut j = 0i32;
        if 0 < m.i32(slot(m, found, 4)) {
            loop {
                let arr = m.u32(slot(m, found, 0));
                if m.i32(arr.wrapping_add((j as u32).wrapping_mul(4))) == tex {
                    let mut cnt = m.i32(slot(m, found, 4));
                    while j < cnt.wrapping_sub(1) {
                        let arr = m.u32(slot(m, found, 0));
                        let at = arr.wrapping_add((j as u32).wrapping_mul(4));
                        m.set_u32(at, m.u32(at + 4));
                        cnt = m.i32(slot(m, found, 4));
                        j += 1;
                    }
                }
                j += 1;
                if j >= m.i32(slot(m, found, 4)) {
                    break;
                }
            }
        }
        let cnt = m.i32(slot(m, found, 4));
        if cnt < 2 {
            let p = m.u32(slot(m, found, 0));
            if p != 0 {
                m.free(p);
                m.set_u32(slot(m, found, 0), 0);
            }
            m.set_u32(slot(m, found, 0), 0);
        } else {
            let np = m.si_realloc(m.u32(slot(m, found, 0)), (cnt.wrapping_sub(1) as u32).wrapping_mul(0x10));
            m.set_u32(slot(m, found, 0), np);
        }
        let c = m.i32(slot(m, found, 4));
        m.set_i32(slot(m, found, 4), c.wrapping_sub(1));
    }
    if found != dom && 0 < dom && dom < m.i32(model + 0x424) {
        if m.u32(slot(m, dom, 0)) == 0 {
            let np = m.si_calloc((m.i32(slot(m, dom, 4)).wrapping_add(1)) as u32, 4);
            m.set_u32(slot(m, dom, 0), np);
        } else {
            let np = m.si_realloc(m.u32(slot(m, dom, 0)), (m.i32(slot(m, dom, 4)).wrapping_add(4) as u32).wrapping_mul(4));
            m.set_u32(slot(m, dom, 0), np);
        }
        let at = m.u32(slot(m, dom, 0)).wrapping_add((m.i32(slot(m, dom, 4)) as u32).wrapping_mul(4));
        m.set_i32(at, tex);
        let c = m.i32(slot(m, dom, 4));
        m.set_i32(slot(m, dom, 4), c.wrapping_add(1));
    }
    1
}

/// [soft 0x0845390c SI_meshSetMultiUVFlag]: for a mesh model (type 4 at +0x30) turns the
/// several-texture-domain mode on (a model with one domain gets 8, a table of
/// `SI_calloc(7, 8)`, and every corner a block of `SI_calloc(14, 4)` holding its texture pair
/// 7 times) or, with `on` 0, off (the per-domain lists, the table and every corner's block
/// freed, the count 1). Answers 1; 0 for no model or another type.
pub fn mesh_set_multi_uv_flag(m: &mut Mem, model: u32, on: i16) -> u32 {
    if model == 0 || m.i32(model + 0x30) != 4 {
        return 0;
    }
    if on != 0 {
        if m.i32(model + 0x424) == 1 {
            let t = m.si_calloc(7, 8);
            m.set_u32(model + 0x428, t);
            m.set_i32(model + 0x424, 8);
            let sp = m.sp();
            for def in defs_of(m, model) {
                let n = m.i32(def + 0x18);
                let mut i = 0i32;
                if 0 < n {
                    loop {
                        let poly = m.u32(def + 4).wrapping_add((i as u32).wrapping_mul(0x18));
                        i += 1;
                        let mut k = 0i32;
                        if m.u16(poly + 0x10) != 0 {
                            loop {
                                let c = m.u32(poly + 0xc).wrapping_add((k as u32).wrapping_mul(0x24));
                                let b = m.si_calloc(0xe, 4);
                                m.set_u32(c + 0x20, b);
                                k += 1;
                                for t in 0..7u32 {
                                    let b = m.u32(c + 0x20);
                                    m.set_u32(b + 8 * t, m.u32(c + 0x14));
                                    m.set_u32(b + 8 * t + 4, m.u32(c + 0x18));
                                }
                                if k >= m.u16(poly + 0x10) as i32 {
                                    break;
                                }
                            }
                        }
                        if i >= m.i32(def + 0x18) {
                            break;
                        }
                    }
                }
            }
            m.set_sp(sp);
            return 1;
        }
        return 1;
    }
    if 1 < m.i32(model + 0x424) {
        let mut i = 0i32;
        if 0 < m.i32(model + 0x424).wrapping_sub(1) {
            loop {
                let at = m.u32(model + 0x428).wrapping_add((i as u32).wrapping_mul(8));
                let p = m.u32(at);
                if p != 0 {
                    m.free(p);
                    m.set_u32(m.u32(model + 0x428).wrapping_add((i as u32).wrapping_mul(8)), 0);
                }
                i += 1;
                if i >= m.i32(model + 0x424).wrapping_sub(1) {
                    break;
                }
            }
        }
        if m.u32(model + 0x428) != 0 {
            let t = m.u32(model + 0x428);
            m.free(t);
            m.set_u32(model + 0x428, 0);
        }
        m.set_u32(model + 0x428, 0);
        m.set_i32(model + 0x424, 1);
        let sp = m.sp();
        for def in defs_of(m, model) {
            for c in corners_of(m, def) {
                let p = m.u32(c + 0x20);
                if p != 0 {
                    m.free(p);
                    m.set_u32(c + 0x20, 0);
                }
            }
        }
        m.set_sp(sp);
    }
    1
}

/// [soft 0x08454258 acceptMultiUVchanges]: the dialog's OK for the texture domains of the
/// model of element `elem` (`+4`): with `on`, the first selected row of `list` (an array of
/// 0x14-byte rows, count at +0) is the domain to switch on (the mode set, texture `tex`
/// associated with it) or, with none selected, the status line says so (`SI_statusDisplayMessage`)
/// and 0 is answered; without `on`, a model with several domains first asks
/// (`SI_systemWarning`, 0 = no, answer 0) and then goes back to the one domain. Answers 1.
pub fn accept_multi_uv_changes(m: &mut Mem, o: &mut dyn Outside, elem: u32, on: i16, list: u32, tex: i32) -> u32 {
    if elem == 0 || list == 0 {
        return 1;
    }
    let model = m.u32(elem + 4);
    if on != 0 {
        let n = m.i16(list) as i32;
        if 0 < n {
            let mut i = 0i32;
            let mut p = m.u32(list + 4);
            let mut found = true;
            while m.i16(p) == 0 {
                p = p.wrapping_add(20);
                i += 1;
                if n <= i {
                    found = false;
                    break;
                }
            }
            if found && i < n {
                mesh_set_multi_uv_flag(m, model, on);
                mesh_associate_txt_uv_domain(m, model, tex, i);
                return 1;
            }
        }
        o.call(m, &super::callee::SI_STATUS_DISPLAY_MESSAGE, &[0x086d5768, 2]);
        return 0;
    }
    if 1 < m.i32(model + 0x424) {
        let r = o.call(m, &super::callee::SI_SYSTEM_WARNING, &[0x086d5740, 0x086d5700, 0x086d56ea]) as u16 as i16;
        if r == 0 {
            return 0;
        }
    }
    mesh_set_multi_uv_flag(m, model, 0);
    mesh_associate_txt_uv_domain(m, model, tex, 0);
    1
}

/// [soft 0x084549f8 SI_meshStoreUVInfo]: the texture pairs of every corner of a mesh model
/// (type 4) and its per-domain lists kept in the record `info` (0x14 bytes, cleared first):
/// `+0` an array of (corner, saved pairs) per corner, `+4` the corner count, `+8` the pairs
/// (`SI_calloc(corners × nb_uv × 2, 4)`: the pair, then the further pairs copied from the
/// corner's block), `+0x10` `nb_uv`, and for more than one domain `+0xc` a copy of the
/// domain table (each list copied into a fresh block).
pub fn mesh_store_uv_info(m: &mut Mem, model: u32, info: u32) {
    if info == 0 {
        return;
    }
    si_mem::clib::memset(m, info, 0, 0x14);
    if model == 0 || m.i32(model + 0x30) != 4 {
        return;
    }
    let sp = m.sp();
    let defs = defs_of(m, model);
    let mut total = 0u32;
    for &d in &defs {
        let mut n = m.i32(d + 0x18);
        if 0 < n {
            let mut p = m.u32(d + 4).wrapping_add(0x10);
            while n != 0 {
                total = total.wrapping_add(m.u16(p) as u32);
                p = p.wrapping_add(0x18);
                n -= 1;
            }
        }
    }
    m.set_u32(info + 4, total);
    let nuv = m.i32(model + 0x424);
    let a = m.si_calloc(total, 8);
    m.set_u32(info, a);
    let b = m.si_calloc(total.wrapping_mul(nuv as u32).wrapping_mul(2), 4);
    m.set_u32(info + 8, b);
    let mut idx = 0u32;
    for &d in &defs {
        let mut i = 0i32;
        if 0 < m.i32(d + 0x18) {
            loop {
                let poly = m.u32(d + 4).wrapping_add((i as u32).wrapping_mul(0x18));
                i += 1;
                let mut k = 0i32;
                if m.u16(poly + 0x10) != 0 {
                    let mut at = idx.wrapping_mul(nuv as u32);
                    let mut coff = 0u32;
                    loop {
                        let c = coff.wrapping_add(m.u32(poly + 0xc));
                        let rec = m.u32(info).wrapping_add(idx.wrapping_mul(8));
                        let dst = m.u32(info + 8).wrapping_add(at.wrapping_mul(8));
                        m.set_u32(rec, c);
                        m.set_u32(rec + 4, dst);
                        m.set_u32(dst, m.u32(c + 0x14));
                        m.set_u32(dst + 4, m.u32(c + 0x18));
                        if 1 < nuv && m.u32(c + 0x20) != 0 {
                            si_mem::clib::memcpy(m, dst + 8, m.u32(c + 0x20), (nuv as u32).wrapping_mul(8).wrapping_sub(8));
                        }
                        at = at.wrapping_add(nuv as u32);
                        idx = idx.wrapping_add(1);
                        coff = coff.wrapping_add(0x24);
                        k += 1;
                        if k >= m.u16(poly + 0x10) as i32 {
                            break;
                        }
                    }
                }
                if i >= m.i32(d + 0x18) {
                    break;
                }
            }
        }
    }
    let nuv = m.i32(model + 0x424);
    m.set_i32(info + 0x10, nuv);
    if 1 < m.i32(model + 0x424) {
        let t = m.si_calloc(nuv.wrapping_sub(1) as u32, 8);
        m.set_u32(info + 0xc, t);
        let mut i = 0i32;
        if 0 < m.i32(info + 0x10).wrapping_sub(1) {
            loop {
                let ent = m.u32(info + 0xc).wrapping_add((i as u32).wrapping_mul(8));
                let cnt = m.i32(m.u32(model + 0x428).wrapping_add(4 + (i as u32).wrapping_mul(8)));
                m.set_i32(ent + 4, cnt);
                let blk = m.si_malloc((m.i32(m.u32(info + 0xc).wrapping_add(4 + (i as u32).wrapping_mul(8))) as u32) << 2);
                m.set_u32(m.u32(info + 0xc).wrapping_add((i as u32).wrapping_mul(8)), blk);
                let ent = m.u32(info + 0xc);
                let mut j = 0i32;
                if 0 < m.i32(ent.wrapping_add(4 + (i as u32).wrapping_mul(8))) {
                    loop {
                        let src = m.u32(m.u32(model + 0x428).wrapping_add((i as u32).wrapping_mul(8)));
                        let dst = m.u32(m.u32(info + 0xc).wrapping_add((i as u32).wrapping_mul(8)));
                        m.set_u32(dst.wrapping_add((j as u32).wrapping_mul(4)), m.u32(src.wrapping_add((j as u32).wrapping_mul(4))));
                        j += 1;
                        if j >= m.i32(m.u32(info + 0xc).wrapping_add(4 + (i as u32).wrapping_mul(8))) {
                            break;
                        }
                    }
                }
                i += 1;
                if i >= m.i32(info + 0x10).wrapping_sub(1) {
                    break;
                }
            }
        }
    }
    m.set_sp(sp);
}

/// [soft 0x08454ca0 SI_meshRestoreUVInfoCoords]: every saved corner of `info` gets its
/// texture pair back and, with `fresh` not 0, its block of further pairs made anew
/// (`SI_calloc(2 × (nb_uv − 1), 4)`, or freed for a model of one domain), then the further
/// pairs copied back when there are several domains.
pub fn mesh_restore_uv_info_coords(m: &mut Mem, model: u32, info: u32, fresh: i16) {
    let nuv = m.i32(info + 0x10);
    let mut i = 0i32;
    if 0 < m.i32(info + 4) {
        let k = nuv.wrapping_sub(1);
        loop {
            let rec = m.u32(info).wrapping_add((i as u32).wrapping_mul(8));
            let c = m.u32(rec);
            let src = m.u32(rec + 4);
            if fresh != 0 {
                if m.i32(model + 0x424) < 2 {
                    if m.u32(c + 0x20) != 0 {
                        let p = m.u32(c + 0x20);
                        m.free(p);
                        m.set_u32(c + 0x20, 0);
                    }
                } else {
                    let b = m.si_calloc((k.wrapping_mul(2)) as u32, 4);
                    m.set_u32(c + 0x20, b);
                }
            }
            m.set_u32(c + 0x14, m.u32(src));
            m.set_u32(c + 0x18, m.u32(src + 4));
            if 1 < nuv && m.u32(c + 0x20) != 0 {
                si_mem::clib::memcpy(m, m.u32(c + 0x20), src + 8, (k as u32).wrapping_mul(8));
            }
            i += 1;
            if i >= m.i32(info + 4) {
                break;
            }
        }
    }
}

/// [soft 0x08454d70 SI_meshRestoreUVInfo]: the state [`mesh_store_uv_info`] kept put back in
/// a mesh model: when the domain count differs the model's lists and table are freed and a
/// table of `SI_calloc(n − 1, 8)` made; the lists are rebuilt from the saved copies (each in
/// a fresh `SI_malloc` block); the count set; the corners' pairs restored
/// ([`mesh_restore_uv_info_coords`], told whether the table was made anew).
pub fn mesh_restore_uv_info(m: &mut Mem, model: u32, info: u32) {
    let mut fresh = 0i16;
    if model == 0 || m.i32(model + 0x30) != 4 || info == 0 || m.u32(info) == 0 || m.u32(info + 8) == 0 {
        return;
    }
    let mut nuv = m.i32(model + 0x424);
    if m.i32(info + 0x10) != nuv {
        let mut i = 0i32;
        while i < nuv.wrapping_sub(1) {
            let at = m.u32(model + 0x428).wrapping_add((i as u32).wrapping_mul(8));
            let p = m.u32(at);
            if p != 0 {
                m.free(p);
                m.set_u32(m.u32(model + 0x428).wrapping_add((i as u32).wrapping_mul(8)), 0);
            }
            nuv = m.i32(model + 0x424);
            i += 1;
        }
        if m.u32(model + 0x428) != 0 {
            let t = m.u32(model + 0x428);
            m.free(t);
            m.set_u32(model + 0x428, 0);
        }
        m.set_i32(model + 0x424, m.i32(info + 0x10));
        let t = m.si_calloc(m.i32(info + 0x10).wrapping_sub(1) as u32, 8);
        m.set_u32(model + 0x428, t);
        fresh = 1;
    }
    let mut n = m.i32(info + 0x10);
    let mut i = 0i32;
    if 0 < n.wrapping_sub(1) {
        loop {
            let off = (i as u32).wrapping_mul(8);
            m.set_u32(m.u32(model + 0x428).wrapping_add(4 + off), m.u32(m.u32(info + 0xc).wrapping_add(4 + off)));
            let blk = m.si_malloc((m.i32(m.u32(info + 0xc).wrapping_add(4 + off)) as u32) << 2);
            m.set_u32(m.u32(model + 0x428).wrapping_add(off), blk);
            let mut j = 0i32;
            let ent = m.u32(info + 0xc);
            let i1 = i + 1;
            if 0 < m.i32(ent.wrapping_add(4 + off)) {
                loop {
                    let src = m.u32(m.u32(info + 0xc).wrapping_add(off));
                    let dst = m.u32(m.u32(model + 0x428).wrapping_add(off));
                    m.set_u32(dst.wrapping_add((j as u32).wrapping_mul(4)), m.u32(src.wrapping_add((j as u32).wrapping_mul(4))));
                    j += 1;
                    if j >= m.i32(m.u32(info + 0xc).wrapping_add(4 + off)) {
                        break;
                    }
                }
            }
            n = m.i32(info + 0x10);
            i = i1;
            if i >= n.wrapping_sub(1) {
                break;
            }
        }
    }
    m.set_i32(model + 0x424, n);
    mesh_restore_uv_info_coords(m, model, info, fresh);
}

/// [soft 0x08454f20 SI_meshDeleteUVInfo]: the saved lists, their table and the two arrays of
/// [`mesh_store_uv_info`] freed, the pointers cleared, the counts 0.
pub fn mesh_delete_uv_info(m: &mut Mem, info: u32) {
    if info == 0 || m.u32(info) == 0 || m.u32(info + 8) == 0 {
        return;
    }
    let mut i = 0i32;
    while i < m.i32(info + 0x10).wrapping_sub(1) {
        let at = m.u32(info + 0xc).wrapping_add((i as u32).wrapping_mul(8));
        let p = m.u32(at);
        if p != 0 {
            m.free(p);
            m.set_u32(m.u32(info + 0xc).wrapping_add((i as u32).wrapping_mul(8)), 0);
        }
        i += 1;
    }
    for off in [0xcu32, 0, 8] {
        let p = m.u32(info + off);
        if p != 0 {
            m.free(p);
            m.set_u32(info + off, 0);
        }
    }
    m.set_u32(info + 4, 0);
    m.set_u32(info + 0x10, 0);
}

/// [soft 0x08453668 SI_meshGetTxtUVDomain]: the first domain (numbered from 1) of the model
/// whose texture list holds `tex` is stored through `out`, else 0; answers 1, and 0 (nothing
/// stored) for no model or no place to store.
pub fn mesh_get_txt_uv_domain(m: &mut Mem, model: u32, tex: i32, out: u32) -> u32 {
    if model == 0 || out == 0 {
        return 0;
    }
    let mut i = 0i32;
    if 0 < m.i32(model + 0x424).wrapping_sub(1) {
        loop {
            let cnt = m.i32(m.u32(model + 0x428).wrapping_add(4 + (i as u32).wrapping_mul(8)));
            let d = i + 1;
            let mut j = 0i32;
            while j < cnt {
                let arr = m.u32(m.u32(model + 0x428).wrapping_add((i as u32).wrapping_mul(8)));
                if m.i32(arr.wrapping_add((j as u32).wrapping_mul(4))) == tex {
                    m.set_i32(out, d);
                    return 1;
                }
                j += 1;
            }
            i = d;
            if i >= m.i32(model + 0x424).wrapping_sub(1) {
                break;
            }
        }
    }
    m.set_i32(out, 0);
    1
}

/// The rows of a text list (`list`: a count at +0 and the rows at +4, 0x14 bytes each with the
/// text at +4) given back: each row's text, the rows, the pointer cleared, the count 0.
fn textlist_clear(m: &mut Mem, list: u32) {
    if m.u32(list + 4) != 0 {
        let mut i = 0i32;
        if 0 < m.i16(list) as i32 {
            let mut off = 0u32;
            loop {
                let t = m.u32(m.u32(list + 4).wrapping_add(4).wrapping_add(off));
                if t != 0 {
                    m.free(t);
                    m.set_u32(m.u32(list + 4).wrapping_add(4).wrapping_add(off), 0);
                }
                off = off.wrapping_add(0x14);
                i += 1;
                if i >= m.i16(list) as i32 {
                    break;
                }
            }
        }
        if m.u32(list + 4) != 0 {
            let r = m.u32(list + 4);
            m.free(r);
            m.set_u32(list + 4, 0);
        }
    }
    m.set_u32(list + 4, 0);
    m.set_u16(list, 0);
}

/// `%-<w>s` of the C library: left-justified, padded with blanks, never cut.
fn pad_right(s: &[u8], w: usize) -> Vec<u8> {
    let mut v = s.to_vec();
    while v.len() < w {
        v.push(b' ');
    }
    v
}

/// [soft 0x08453e98 initVisualMdlList]: the rows of the model list of the texture dialog
/// (`list`, see `textlist_clear`) made anew for the `n` elements of `elems` (an array of element
/// pointers): row i is marked when its element is `sel`, and its text is
/// `"%-27s %-15s default"` or `"%-27s %-15s %d"` of the element's name (the text at +0x10 of
/// the record its first word points to, "" for none, then "-" when that is not empty, then the
/// text at +0x14), ON or OFF (several domains in the model of the element, +4) and the domain
/// of texture `tex` ([`mesh_get_txt_uv_domain`]; "default" when 0). Then the dialog is told
/// (`SI_setTexview (0x0838c390)`, view 3).
pub fn init_visual_mdl_list(m: &mut Mem, o: &mut dyn Outside, dialog: u32, list: u32, elems: u32, n: i32, sel: u32, tex: i32) {
    textlist_clear(m, list);
    if 0 < n && elems != 0 {
        let rows = m.si_calloc(n as u32, 0x14);
        m.set_u32(list + 4, rows);
        if rows != 0 {
            m.set_u16(list, n as u16);
            let sp = m.sp();
            let b1 = m.local(0x1000);
            let b2 = m.local(0x400);
            let dom = m.local(4);
            let mut i = 0i32;
            let mut off = 0u32;
            let mut cur = elems;
            while i < n {
                let e = m.u32(cur);
                let row = m.u32(list + 4).wrapping_add(off);
                m.set_u16(row, (sel == e) as u16);
                m.set_u16(row + 2, 0);
                mesh_get_txt_uv_domain(m, m.u32(e + 4), tex, dom);
                let rec = m.u32(e);
                let name = m.u32(rec + 0x10);
                let nm = if name == 0 { Vec::new() } else { m.cstr(name) };
                let sep: &[u8] = if nm.is_empty() { b"" } else { b"-" };
                let name2 = m.u32(rec + 0x14);
                let mut t = nm.clone();
                t.extend_from_slice(sep);
                t.extend_from_slice(&if name2 == 0 { b"(null)".to_vec() } else { m.cstr(name2) });
                m.put_cstr(b1, &t);
                let onoff: &[u8] = if 1 < m.i32(m.u32(e + 4) + 0x424) { b"ON" } else { b"OFF" };
                let d = m.i32(dom);
                let mut line = pad_right(&t, 27);
                line.push(b' ');
                line.extend_from_slice(&pad_right(onoff, 15));
                line.push(b' ');
                if d == 0 {
                    line.extend_from_slice(b"default");
                } else {
                    line.extend_from_slice(d.to_string().as_bytes());
                }
                m.put_cstr(b2, &line);
                let s = m.strdup(b2);
                cur = cur.wrapping_add(4);
                i += 1;
                m.set_u32(m.u32(list + 4).wrapping_add(4).wrapping_add(off), s);
                off = off.wrapping_add(0x14);
            }
            m.set_sp(sp);
            let cnt = m.i16(list) as i32 as u32;
            let r = m.u32(list + 4);
            o.call(m, &super::callee::SI_SETTEXVIEW, &[dialog, 3, cnt, r]);
        }
    }
}

/// [soft 0x084540c4 initVisualDomainList]: for a dialog and a text list, the rows given back and
/// made anew: one ("default"), or with `eight` not 0 eight ("default", "1" .. "7"); the row of
/// the domain of texture `tex` in the model of element `elem` is marked (none for no element);
/// then the dialog is told (`SI_setTexview (0x0838c390)`, view 4).
pub fn init_visual_domain_list(m: &mut Mem, o: &mut dyn Outside, dialog: u32, list: u32, elem: u32, tex: i32, eight: i16) {
    if dialog == 0 || list == 0 {
        return;
    }
    textlist_clear(m, list);
    let rows = m.si_calloc(if eight != 0 { 8 } else { 1 }, 0x14);
    m.set_u32(list + 4, rows);
    if rows != 0 {
        m.set_u16(list, if eight == 0 { 1 } else { 8 });
        let sp = m.sp();
        let b = m.local(0x40);
        let dom = m.local(4);
        m.set_i32(dom, -1);
        if elem != 0 {
            mesh_get_txt_uv_domain(m, m.u32(elem + 4), tex, dom);
        }
        let mut i = 0i32;
        let mut off = 0u32;
        while i < m.i16(list) as i32 {
            let text = if i == 0 { b"default".to_vec() } else { i.to_string().into_bytes() };
            m.put_cstr(b, &text);
            let s = m.strdup(b);
            m.set_u32(m.u32(list + 4).wrapping_add(4).wrapping_add(off), s);
            m.set_u16(m.u32(list + 4).wrapping_add(off), (m.i32(dom) == i) as u16);
            m.set_u16(m.u32(list + 4).wrapping_add(2).wrapping_add(off), 0);
            off = off.wrapping_add(0x14);
            i += 1;
        }
        m.set_sp(sp);
        let cnt = m.i16(list) as i32 as u32;
        let r = m.u32(list + 4);
        o.call(m, &super::callee::SI_SETTEXVIEW, &[dialog, 4, cnt, r]);
    }
}
