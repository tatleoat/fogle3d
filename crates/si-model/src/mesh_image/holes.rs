// SPDX-License-Identifier: MIT
//! The hole remover of `triangulate.c` on the image (finding 0015, 2026-10-29, package "tri1"):
//! `SI_removeHolesFromMesh` and its three small helpers. A polygon with holes is triangulated
//! in its own plane; the triangle sides that are not already sides of the polygon become edges of
//! the mesh (`SI_addEdgeToMesh`), and the ones that can go again are taken out (`SI_canRemove...`,
//! `SI_removeEdgeFromMesh`), which leaves the polygon whole with its holes joined by the bridges
//! the triangulation drew.

use si_mem::Mem;
use si_x87::x;

use super::edit_fct::{add_edge_to_mesh, can_remove_edge_from_mesh, remove_edge_from_mesh};
use super::tri::{mesh_triangulate, triangulate_contour};
use super::{f3, off as mo, put_f3, vertex_at};
use crate::model_image::Outside;

/// [soft 0x0851f674 PolygonHasEdge]: whether one of the edges listed in the corners of the
/// polygon at `poly` runs between the vertices `a` and `b` (either way round). The corner's edge
/// number is taken by its absolute value and found as record `n & 0x3ff` of block `n >> 10`
/// of the mesh's edge table.
pub fn polygon_has_edge(m: &Mem, mesh: u32, poly: u32, a: i32, b: i32) -> bool {
    let count = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
    let mut k = 0;
    if count != 0 {
        loop {
            let n = m.i32(m.u32(poly + mo::POLYGON_NODES).wrapping_add(4).wrapping_add((k as u32).wrapping_mul(0x24)));
            let n = if n < 0 { n.wrapping_neg() } else { n };
            let block = m.u32(m.u32(mesh + mo::EDGES).wrapping_add(((n >> 10) as u32).wrapping_mul(4)));
            let rec = block.wrapping_add(((n as u32) & 0x3ff).wrapping_mul(mo::EDGE));
            let (e0, e1) = (m.i32(rec), m.i32(rec + 4));
            if (e0 == a && e1 == b) || (e0 == b && e1 == a) {
                return true;
            }
            k += 1;
            if k >= count {
                break;
            }
        }
    }
    false
}

/// [soft 0x0851f704 EdgeNotThere]: whether the pair (`a`, `b`) is absent, in either order, from
/// the first `n` words of the list of vertex pairs at `list` (`n` counts words).
pub fn edge_not_there(m: &Mem, list: u32, n: i32, a: i32, b: i32) -> bool {
    let mut i = 0i32;
    if 0 < n {
        loop {
            let p = list.wrapping_add((i as u32).wrapping_mul(4));
            let (e0, e1) = (m.i32(p), m.i32(p.wrapping_add(4)));
            if (e0 == a && e1 == b) || (e0 == b && e1 == a) {
                return false;
            }
            i = i.wrapping_add(2);
            if i >= n {
                break;
            }
        }
    }
    true
}

/// [soft 0x0851f758 FindEdgeIdFromTwoVertices]: looks through the star of vertex `v` (12-byte
/// entries: neighbour, edge, polygon) for the neighbour `w`; stores the absolute value of its
/// edge number through `out` and answers 1, or answers 0.
pub fn find_edge_id_from_two_vertices(m: &mut Mem, mesh: u32, v: i32, w: i32, out: u32) -> u32 {
    let rec = vertex_at(m, mesh, v);
    let star = m.u32(rec + mo::VERTEX_STAR);
    let count = m.u16(rec + mo::VERTEX_NB_STAR) as i32;
    let mut k = 0;
    if count != 0 {
        loop {
            let e = star.wrapping_add((k as u32).wrapping_mul(12));
            if m.i32(e) == w {
                let n = m.i32(e + 4);
                m.set_i32(out, if n < 0 { n.wrapping_neg() } else { n });
                return 1;
            }
            k += 1;
            if k >= count {
                break;
            }
        }
    }
    0
}

/// [soft 0x0851f7b8 SI_removeHolesFromMesh]: for each polygon that has a hole (a corner whose
/// vertex is -1), the polygon's vertices are put in its plane (`SI_matrixZAlign` of its normal,
/// ((y·m₁ⱼ + x·m₀ⱼ) + z·m₂ⱼ) + m₃ⱼ stored as floats) and triangulated with
/// [`triangulate_contour`]; of each triangle the first side (in the order 0-1, 1-2, 2-0) that is
/// not a side of the polygon and not yet listed is listed as a pair of vertices. Then every listed
/// pair is added to the mesh as an edge, and the pairs whose edge can be removed again are
/// removed, round after round until a round removes none. Answers 1; 0 when an allocation fails;
/// when a triangulation fails the result of `SI_meshTriangulate(mesh, 0)` is answered at once
/// (as 0 or 1) and the work blocks are not freed.
///
/// Only the first hole-carrying corner of a polygon is looked for, and one pass per polygon.
/// `__builtin_vec_new` and `__builtin_vec_delete` (0x08642184, 0x086420e4, the compiler's
/// runtime) are `malloc` and `free` of the block.
///
/// Two things in the original depend on memory nobody wrote and cannot be matched: the point block of
/// `SI_addEdgeToMesh` is read at vertices outside the polygon, and `SI_removeEdgeFromMesh` looks a stale polygon
/// number up past the end of its table (the port answers a number no polygon has, see `MAP2_PAST_END`).
pub fn remove_holes_from_mesh(m: &mut Mem, o: &mut dyn Outside, mesh: u32) -> u32 {
    let (mut remap, mut vids, mut tris, mut edges) = (0u32, 0u32, 0u32, 0u32);
    let (mut total, mut n_edges) = (0i32, 0i32);
    let mut pts = 0u32;
    if 0 < m.i32(mesh + mo::NB_POLYGONS) {
        pts = m.malloc((m.i32(mesh + mo::NB_VERTICES) as u32).wrapping_mul(12));
        if pts == 0 {
            return 0;
        }
        let count_at = m.local(8);
        let tris_at = count_at + 4;
        let out = m.local(4);
        let mut p = 0i32;
        while p < m.i32(mesh + mo::NB_POLYGONS) {
            let poly = m.u32(mesh + mo::POLYGONS).wrapping_add((p as u32).wrapping_mul(mo::POLYGON));
            let ncorners = m.u16(poly + mo::POLYGON_NB_NODES) as u32;
            let mut k = 0u32;
            let mut hole = false;
            while k < ncorners {
                let nodes = m.u32(poly + mo::POLYGON_NODES);
                if m.i32(nodes.wrapping_add(k * mo::NODE)) == -1 {
                    hole = true;
                    break;
                }
                k += 1;
            }
            if hole {
                if remap != 0 {
                    m.free(remap);
                }
                remap = m.malloc(ncorners << 2);
                if remap == 0 {
                    return 0;
                }
                if vids != 0 {
                    m.free(vids);
                }
                vids = m.malloc(ncorners << 2);
                if vids == 0 {
                    return 0;
                }
                let mut used = 0u32;
                for c in 0..ncorners {
                    let v = m.i32(m.u32(poly + mo::POLYGON_NODES).wrapping_add(c * mo::NODE));
                    if v == -1 {
                        m.set_i32(remap + 4 * c, -1);
                    } else {
                        m.set_i32(vids + 4 * used, v);
                        m.set_u32(remap + 4 * c, used);
                        let src = vertex_at(m, mesh, v);
                        let xyz = f3(m, src);
                        put_f3(m, pts + 12 * used, xyz);
                        used += 1;
                    }
                }
                let z = si_math::algebra::matrix_z_align(&si_math::types::Vector::from_array(f3(m, poly)));
                for k in 0..used {
                    let at = pts + 12 * k;
                    let [px, py, pz] = f3(m, at);
                    let col = |j: usize| (((x(py) * z[1][j] + x(px) * z[0][j]) + x(pz) * z[2][j]) + z[3][j]).to_f32();
                    put_f3(m, at, [col(0), col(1), col(2)]);
                }
                if tris != 0 {
                    m.free(tris);
                }
                tris = 0;
                m.set_u32(tris_at, 0);
                let ok = triangulate_contour(m, used as i32, pts, ncorners as i32, remap, count_at, tris_at);
                tris = m.u32(tris_at);
                if ok == 0 {
                    return (mesh_triangulate(m, o, mesh, 0) != 0) as u32;
                }
                let n_tri = m.i32(count_at);
                total = total.wrapping_add(n_tri);
                edges = m.realloc(edges, (total as u32).wrapping_mul(8));
                if edges == 0 {
                    return 0;
                }
                let vid = |m: &Mem, t: u32, c: u32| m.i32(vids.wrapping_add((m.u32(t + 4 * c)).wrapping_mul(4)));
                for t in 0..n_tri.max(0) as u32 {
                    let tri = tris + 12 * t;
                    let v = [vid(m, tri, 0), vid(m, tri, 1), vid(m, tri, 2)];
                    for (a, b) in [(v[0], v[1]), (v[1], v[2]), (v[2], v[0])] {
                        if !polygon_has_edge(m, mesh, poly, a, b) && edge_not_there(m, edges, n_edges, a, b) {
                            m.set_i32(edges.wrapping_add((n_edges as u32).wrapping_mul(4)), a);
                            m.set_i32(edges.wrapping_add(((n_edges + 1) as u32).wrapping_mul(4)), b);
                            n_edges += 2;
                            break;
                        }
                    }
                }
            }
            p += 1;
        }
        let mut i = 0i32;
        while i < n_edges {
            let a = m.i32(edges + 4 * i as u32) as i16;
            let b = m.i32(edges + 4 * (i as u32 + 1)) as i16;
            add_edge_to_mesh(m, o, 0, mesh, a, b, 0);
            i += 2;
        }
        loop {
            let mut changed = false;
            let mut i = 0i32;
            while i < n_edges {
                let at = edges + 4 * i as u32;
                let (a, b) = (m.i32(at), m.i32(at + 4));
                if a != -1 && b != -1 {
                    if find_edge_id_from_two_vertices(m, mesh, a, b, out) as u16 != 0 {
                        let e = m.i32(out);
                        if can_remove_edge_from_mesh(m, mesh, e) as u16 == 1 {
                            remove_edge_from_mesh(m, mesh, e, 0);
                            m.set_i32(at + 4, -1);
                            m.set_i32(at, -1);
                            changed = true;
                        }
                    }
                }
                i += 2;
            }
            if !changed {
                break;
            }
        }
    }
    if pts != 0 {
        m.free(pts);
    }
    if remap != 0 {
        m.free(remap);
    }
    if tris != 0 {
        m.free(tris);
    }
    if edges != 0 {
        m.free(edges);
    }
    if vids != 0 {
        m.free(vids);
    }
    1
}
