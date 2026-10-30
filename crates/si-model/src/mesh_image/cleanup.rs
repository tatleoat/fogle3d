// SPDX-License-Identifier: MIT
//! `cleanup.c` on the image (finding 0015, eighth pass): `SI_newMeshCleanup`, the clean-up
//! a patch's and a NURBS surface's conversion to a mesh end with (and the mesh dialog's
//! "clean up"), with the four routines of its file it calls and the vertex normal of
//! `meshUtil.c` it asks for.
//!
//! What the clean-up does, in its order (read from the listing; the steps the conversions
//! take are 1, 2, 3 and 6, since they pass `merge_polygons` = 0):
//!
//! 1. *Vertices closer than the tolerance* (`merge_vertices`): each vertex is given the
//!    nearest earlier vertex within the tolerance (all pairs, or with `by_star` only the
//!    neighbours of its star, then handing on the earlier vertex's own choice); each
//!    earlier vertex with followers becomes a **new vertex** at the end of the array (the
//!    average of the group's positions), and every corner of the group moves to it with its
//!    normal corrected by the difference of the vertex normals (group average minus its
//!    own).
//! 2. *Polygons*: repeated corners and "come backs" (a → b → a) removed, contours of fewer
//!    than three corners dropped (with the original's off-by-one, see [`remove_come_back`]),
//!    and a polygon with fewer than three corners left, or a normal of squared length below
//!    0.01, deleted: the last polygon moves into its place.
//! 3. The cosine of `angle` (the C library's `cos`, as a double).
//! 4. *Coplanar neighbours merged* (`merge_polygons`; only the dialog asks): along each edge
//!    whose two polygons have the same material, normals within `angle` and the same
//!    texture pairs and colours on both sides, the two become one; an edge between two
//!    contours of one polygon joins them.
//! 5. The stars and edges rebuilt (`SI_meshUpdateNodes(mesh, 0)`), then every vertex with
//!    exactly two star entries whose turn is "straighter" than the angle (the cosine of
//!    `SI_pointsCosAngle` above that of `angle`) leaves its polygons.
//! 6. Vertices without polygons dropped (the array is compacted, not shrunk), polygons of
//!    fewer than three corners dropped, and the mesh rebuilt again without corner normals.
//!
//! Every block the original allocates, grows and frees is allocated, grown and freed here
//! in its order and with its size, on the image (the five work arrays included). Where the
//! original reads memory it never wrote, or a stale stack slot, the code says so at that
//! place.

use si_math::algebra::{points_cos_angle, vector_cos_angle};
use si_math::types::Vector;
use si_mem::Mem;
use si_x87::{x, F80};

use super::obj::{mesh_dispose_edges, nodes_copy, nodes_copy_uv, nodes_dispose};
use super::rebuild::{computes_polygon_normal, update_nodes};
use super::{callee, edge_record, f3, off as mo, polygon_at, vertex_at};
use crate::env::stale;
use crate::model_image::Outside;

/// 0.0001 (`.rodata` 0x0865f710, a double): the smallest corner-normal and polygon-normal
/// length taken as a direction.
const MIN_LENGTH: f64 = 1e-4;
/// 0.01 (0x0865f718): the smallest squared polygon-normal length a polygon keeps.
const MIN_NORMAL2: f64 = 0.01;
/// 1e-6 (0x0865f748): two texture coordinates are the same when closer than this.
const UV_EPS: f64 = 1e-6;

/// The address of corner `k` of a corner array (36-byte records).
fn node(nodes: u32, k: i32) -> u32 {
    nodes.wrapping_add((k as u32).wrapping_mul(mo::NODE))
}

/// The corner copy every step of the clean-up makes (`cleanup.c` writes it out at each
/// place): the vertex, the normal, the texture pairs through `SI_nodesCopyUV` without
/// models (into `uv_dst`, which is `dst` everywhere but in one place), the colour. The edge
/// word and `dst`'s own pointer to further pairs stay.
fn copy_corner_uv_to(m: &mut Mem, dst: u32, src: u32, uv_dst: Option<u32>) {
    m.set_u32(dst, m.u32(src));
    for k in 0..3 {
        m.set_u32(dst + mo::NODE_NORMAL + 4 * k, m.u32(src + mo::NODE_NORMAL + 4 * k));
    }
    if let Some(u) = uv_dst {
        nodes_copy_uv(m, 0, 0, u, src);
    }
    m.set_u32(dst + mo::NODE_COLOUR, m.u32(src + mo::NODE_COLOUR));
}

fn copy_corner(m: &mut Mem, dst: u32, src: u32) {
    copy_corner_uv_to(m, dst, src, Some(dst));
}

/// The polygon record at `dst` takes the one at `src` (normal, flag, count, corners,
/// material, in that order); `src` keeps no corners.
fn move_polygon(m: &mut Mem, dst: u32, src: u32) {
    for k in 0..3 {
        m.set_u32(dst + 4 * k, m.u32(src + 4 * k));
    }
    m.set_u16(dst + mo::POLYGON_FLAG, m.u16(src + mo::POLYGON_FLAG));
    m.set_u16(dst + mo::POLYGON_NB_NODES, m.u16(src + mo::POLYGON_NB_NODES));
    m.set_u32(dst + mo::POLYGON_NODES, m.u32(src + mo::POLYGON_NODES));
    m.set_u16(dst + mo::POLYGON_MATERIAL, m.u16(src + mo::POLYGON_MATERIAL));
    m.set_u16(src + mo::POLYGON_NB_NODES, 0);
    m.set_u32(src + mo::POLYGON_NODES, 0);
}

/// A count rounded up to a multiple of four, as the original forms it for a corner block
/// (`n − (((n − 1) & 3) − 3)`, in unsigned arithmetic: 0 stays 0).
fn round4(n: u32) -> u32 {
    n.wrapping_sub(((n.wrapping_sub(1)) & 3).wrapping_sub(3))
}

/// The positions of three vertices as `si-math` vectors.
fn vector(v: [f32; 3]) -> Vector {
    Vector { x: v[0], y: v[1], z: v[2] }
}

/// [soft 0x08458960 SI_computeMeshVertexNormal] on the image: over every star entry of
/// vertex `v` that has a polygon, the normals of that polygon's corners at `v` summed in
/// floats; the sums times 1 / (the number of such entries), each product rounded to float.
/// A vertex without polygons gives 0 × ∞ (the indefinite NaN). The value form is
/// [`crate::mesh_util::compute_mesh_vertex_normal`]; the original reads the star and the
/// polygons without a range check, as here.
pub fn compute_mesh_vertex_normal(m: &Mem, mesh: u32, v: i32) -> [f32; 3] {
    let vr = vertex_at(m, mesh, v);
    let mut sum = [0.0f32; 3];
    let mut count = 0u32;
    let n = m.u16(vr + mo::VERTEX_NB_STAR) as u32;
    let star = m.u32(vr + mo::VERTEX_STAR);
    let mut k = 0u32;
    while k < n {
        let p = m.i32(star.wrapping_add(12 * k + 8));
        k += 1;
        if p == -1 {
            continue;
        }
        count += 1;
        let poly = polygon_at(m, mesh, p);
        let nodes = m.u32(poly + mo::POLYGON_NODES);
        let nb = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
        for c in 0..nb {
            let cn = node(nodes, c);
            if m.i32(cn) == v {
                let nrm = f3(m, cn + mo::NODE_NORMAL);
                sum = [(x(sum[0]) + nrm[0]).to_f32(), (x(sum[1]) + nrm[1]).to_f32(), (x(sum[2]) + nrm[2]).to_f32()];
            }
        }
    }
    let inv = F80::ONE / F80::from_i64(count as i64);
    [(inv * sum[0]).to_f32(), (inv * sum[1]).to_f32(), (inv * sum[2]).to_f32()]
}

/// [soft 0x08430a20 si_svertexReallocate]: room for `n` more vertices. When the room left
/// in the vertex array's last block of 32 (`−count & 31`, the count read as a short) is
/// `n` or less, the array is reallocated to the count plus `n` rounded up to 32 records
/// (also when it would have fitted exactly); without an array one of `n` rounded up to 32
/// is allocated zeroed and the count set to 0.
pub fn svertex_reallocate(m: &mut Mem, mesh: u32, n: i16) {
    let n = n as i32;
    let room = (m.u16(mesh + mo::NB_VERTICES) as u32).wrapping_neg() & 0x1f;
    if (room as i32) <= n {
        let up = |t: u32| t.wrapping_sub((t.wrapping_sub(1) & 0x1f).wrapping_sub(0x1f));
        if m.u32(mesh + mo::VERTICES) == 0 {
            let p = m.si_calloc(up(n as u32), mo::VERTEX);
            m.set_u32(mesh + mo::VERTICES, p);
            m.set_i32(mesh + mo::NB_VERTICES, 0);
        } else {
            let t = (m.i32(mesh + mo::NB_VERTICES) + n) as u32;
            let p = m.si_realloc(m.u32(mesh + mo::VERTICES), up(t).wrapping_mul(mo::VERTEX));
            m.set_u32(mesh + mo::VERTICES, p);
        }
    }
}

/// [soft 0x08430a8c removeComeBack]: one defect of the corner list `nodes` (its count, a
/// short, at `count_at`) removed; answers 1 if one was, 0 when the list is clean. Contour
/// by contour (a contour here starts at the separator before it, so a hole's length counts
/// its separator):
///
/// - a contour of fewer than three records: the records from its end (the separator after
///   it, or the end) on are copied over its start, raw (nine words, the pointer to further
///   pairs too), and the count goes down by its length plus one. For every contour but the
///   first this drops one record too many, and for the first it keeps the separator after
///   it: the original's arithmetic, kept;
/// - a "come back" (the corners before and after a corner are the same vertex, the
///   contour taken cyclically): from two after it on, each corner moves two places down
///   (`SI_nodesCopy`), and the count goes down by two.
pub fn remove_come_back(m: &mut Mem, nodes: u32, count_at: u32) -> i16 {
    if m.u16(count_at) == 0 {
        return 0;
    }
    let n = m.u16(count_at) as i32;
    let mut start = 0i32;
    let mut k = 0i32;
    while k <= n {
        if k == n || m.i32(node(nodes, k)) == -1 {
            let len = k - start;
            if len <= 2 {
                if k < n {
                    let (mut t, mut d) = (k, start);
                    while t < m.u16(count_at) as i32 {
                        for w in 0..9 {
                            m.set_u32(node(nodes, d) + 4 * w, m.u32(node(nodes, t) + 4 * w));
                        }
                        t += 1;
                        d += 1;
                    }
                }
                let c = m.u16(count_at) as i32;
                if c < len + 1 {
                    m.set_u16(count_at, 0);
                } else {
                    m.set_u16(count_at, m.u16(count_at).wrapping_sub((len + 1) as u16));
                }
                return 1;
            }
            let mut prev = m.i32(node(nodes, k - 1));
            let mut u = start;
            while u < k {
                let next = if u + 1 == k { m.i32(node(nodes, start)) } else { m.i32(node(nodes, u + 1)) };
                if prev == next {
                    if u != n - 1 && u + 2 < n {
                        let mut t = u + 2;
                        while t < m.u16(count_at) as i32 {
                            nodes_copy(m, 0, 0, node(nodes, t - 2), node(nodes, t));
                            t += 1;
                        }
                    }
                    m.set_u16(count_at, m.u16(count_at).wrapping_sub(2));
                    return 1;
                }
                prev = m.i32(node(nodes, u));
                u += 1;
            }
            start = k;
        }
        k += 1;
    }
    0
}

/// [soft 0x08434508 adjPolHasSameInfo]: whether polygon `a` has, at the corners where it
/// meets corner `k` of polygon `b` and the corner after it (cyclically over the whole
/// array, not the contour), the same texture pairs (each coordinate closer than 1e-6) and
/// colours. `a`'s corner at `b`'s vertex `k` is the first one of `a` with that vertex,
/// looked for up to `a`'s first separator; `a`'s other corner is the one before it, or
/// else the one after it, that has the vertex of `b`'s next corner. Answers 0 or 1.
pub fn adj_pol_has_same_info(m: &Mem, a: u32, b: u32, k: i32) -> i16 {
    let bn = m.u32(b + mo::POLYGON_NODES);
    let v = m.i32(node(bn, k));
    let k1 = if k == m.u16(b + mo::POLYGON_NB_NODES) as i32 - 1 { 0 } else { k + 1 };
    let w = m.i32(node(bn, k1));
    let n = m.u16(a + mo::POLYGON_NB_NODES) as i32;
    let an = m.u32(a + mo::POLYGON_NODES);
    let mut u = 0i32;
    // the vertex last looked at (a register the original leaves alone for an empty `a`;
    // then `u` = `n` decides first)
    let mut c = 0i32;
    if u < n {
        c = m.i32(an);
        if c != v && c != -1 {
            loop {
                u += 1;
                if u >= n {
                    break;
                }
                c = m.i32(node(an, u));
                if c == v || c == -1 {
                    break;
                }
            }
        }
    }
    if u == n || c == -1 || c != v {
        return 0;
    }
    let same = |m: &Mem, p: u32, q: u32| {
        let eps = x(UV_EPS);
        let du = (x(m.f32(p + mo::NODE_UV)) - m.f32(q + mo::NODE_UV)).abs();
        if !eps.gt(du) {
            return false;
        }
        let dv = (x(m.f32(p + mo::NODE_UV + 4)) - m.f32(q + mo::NODE_UV + 4)).abs();
        dv.lt(eps) && m.u32(p + mo::NODE_COLOUR) == m.u32(q + mo::NODE_COLOUR)
    };
    if !same(m, node(an, u), node(bn, k)) {
        return 0;
    }
    let mut j = if u == 0 { n - 1 } else { u - 1 };
    if w != m.i32(node(an, j)) {
        j = if u == n - 1 { 0 } else { u + 1 };
        if w != m.i32(node(an, j)) {
            return 0;
        }
    }
    same(m, node(an, j), node(bn, k1)) as i16
}

/// [soft 0x084346cc edgeInfoIsConsistent]: whether, along the first contour of the edge's
/// first polygon (+8), every corner whose edge (the corner's signed edge number, its other
/// polygon: +0xc of the record for a positive number, +8 for a negative one) runs along the
/// edge's second polygon (+0xc) meets it with the same texture pairs and colours
/// ([`adj_pol_has_same_info`]). The walk stops at a corner without an edge; 1 only when it
/// reached the contour's end.
pub fn edge_info_is_consistent(m: &Mem, mesh: u32, edge: u32) -> i16 {
    let q = m.i32(edge + 0xc);
    let p = polygon_at(m, mesh, m.i32(edge + 8));
    let other = polygon_at(m, mesh, q);
    let mut k = 0i32;
    if m.u16(p + mo::POLYGON_NB_NODES) != 0 && m.i32(m.u32(p + mo::POLYGON_NODES)) != -1 {
        let mut e = m.i32(m.u32(p + mo::POLYGON_NODES) + 4);
        while e != 0 {
            let r = if e < 0 { edge_record(m, mesh, e.wrapping_neg()) + 8 } else { edge_record(m, mesh, e) + 0xc };
            if m.i32(r) == q && adj_pol_has_same_info(m, other, p, k) == 0 {
                break;
            }
            k += 1;
            let nodes = m.u32(p + mo::POLYGON_NODES);
            if k >= m.u16(p + mo::POLYGON_NB_NODES) as i32 || m.i32(node(nodes, k)) == -1 {
                break;
            }
            e = m.i32(node(nodes, k) + 4);
        }
    }
    let nodes = m.u32(p + mo::POLYGON_NODES);
    (k == m.u16(p + mo::POLYGON_NB_NODES) as i32 || m.i32(node(nodes, k)) == -1) as i16
}

/// The squared distance of two positions as the original forms it: the differences of the
/// second minus the first in the register, `dz² + (dy² + dx²)`.
fn distance2(p: [f32; 3], q: [f32; 3]) -> F80 {
    let dx = x(q[0]) - p[0];
    let dy = x(q[1]) - p[1];
    let dz = x(q[2]) - p[2];
    dz * dz + (dy * dy + dx * dx)
}

/// The length of a float vector in the register: `√((x² + y²) + z²)`.
fn length(v: [f32; 3]) -> F80 {
    ((x(v[0]) * v[0] + x(v[1]) * v[1]) + x(v[2]) * v[2]).sqrt()
}

/// The work arrays and the state the clean-up keeps across its steps (the original's
/// locals).
struct Work {
    mesh: u32,
    /// Per vertex: a normal sum (three floats) and a count (`SI_malloc(n × 16)`).
    normals: u32,
    /// Per vertex: the earlier vertex it goes with and the squared distance to it
    /// (`SI_malloc(n × 8)`).
    near: u32,
    /// Per polygon of the start: where it is now, −1 for gone (`SI_calloc(n, 4)`).
    pmap: u32,
    /// Per edge: the first edge of its pair (`SI_calloc(edges + 1, 4)`).
    twin: u32,
    /// Per vertex: the vertex it became (`SI_calloc(n, 4)`, grown with the new vertices).
    vmap: u32,
    /// The polygon count at the start.
    np0: i32,
    /// The end of the last contour the polygon steps looked at (the original's one local,
    /// read by the merging of two polygons when its own walk did not set it).
    last_end: i32,
    /// The block of the last merge of two polygons (the original's stack slot, which the
    /// joining of two contours of one polygon writes texture pairs into); `None` before the
    /// first merge, when the original's slot holds whatever the stack held.
    merged: Option<u32>,
}

impl Work {
    fn pmap_replace(&self, m: &mut Mem, from: i32, to: i32) {
        for k in 0..self.np0.max(0) as u32 {
            if m.i32(self.pmap + 4 * k) == from {
                m.set_i32(self.pmap + 4 * k, to);
            }
        }
    }
}

/// [soft 0x08430c30 SI_newMeshCleanup]: the mesh at `mesh` cleaned up in place (see the
/// module's comment for the steps). `tolerance` is the distance under which two vertices
/// become one, `angle` (radians) the angle under which two polygons may merge or a vertex
/// between two neighbours is dropped; `merge_vertices`, `merge_polygons` and `by_star`
/// switch steps 1 and 4 and choose step 1's search. The conversions of a patch and of a
/// NURBS surface call it with (0.001, 0.0, 1, 0, 1) and (0.001, 0.0, 1, 0, 0).
///
/// The calls out of the package, through `o`: `SI_showActivity` (0x083d1a60) and what the
/// rebuild calls.
#[allow(clippy::too_many_arguments)]
pub fn new_mesh_cleanup(m: &mut Mem, o: &mut dyn Outside, mesh: u32, tolerance: f32, angle: f32, merge_vertices: i16, merge_polygons: i16, by_star: i16) {
    let np0 = m.i32(mesh + mo::NB_POLYGONS);
    let normals = m.si_malloc((m.i32(mesh + mo::NB_VERTICES) as u32) << 4);
    let near = m.si_malloc((m.i32(mesh + mo::NB_VERTICES) as u32) << 3);
    let pmap = m.si_calloc(np0 as u32, 4);
    for k in 0..np0.max(0) as u32 {
        m.set_u32(pmap + 4 * k, k);
    }
    let twin = m.si_calloc((m.i32(mesh + mo::NB_EDGES) + 1) as u32, 4);
    let mut k = 1i32;
    while k <= m.i32(mesh + mo::NB_EDGES) {
        m.set_i32(twin + 4 * k as u32, k);
        k += 1;
    }
    // the square of the tolerance in the register, then stored as a double (exact)
    let tol2 = (x(tolerance) * tolerance).to_f64();
    let vmap = m.si_calloc(m.i32(mesh + mo::NB_VERTICES) as u32, 4);
    let mut k = 0i32;
    while k < m.i32(mesh + mo::NB_VERTICES) {
        let ku = k as u32;
        m.set_i32(vmap + 4 * ku, k);
        m.set_u32(normals + 16 * ku + 12, 0);
        m.set_u32(near + 8 * ku + 4, 0);
        m.set_i32(near + 8 * ku, k);
        k += 1;
    }
    let mut w = Work { mesh, normals, near, pmap, twin, vmap, np0, last_end: 0, merged: None };
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    if merge_vertices != 0 {
        merge_close_vertices(m, o, &mut w, x(tol2), by_star);
    }
    // the edges follow their vertices
    let mut e = 1i32;
    while e <= m.i32(mesh + mo::NB_EDGES) {
        let r = edge_record(m, mesh, e);
        m.set_i32(r, m.i32(w.vmap + 4 * m.u32(r)));
        m.set_i32(r + 4, m.i32(w.vmap + 4 * m.u32(r + 4)));
        e += 1;
    }
    // each edge's twin (the same two vertices the other way), whose first polygon becomes
    // the edge's second
    let mut e = 1i32;
    let mut ne = m.i32(mesh + mo::NB_EDGES);
    while e < ne {
        let r = edge_record(m, mesh, e);
        if m.i32(twin + 4 * e as u32) == e {
            let (a, b) = (m.i32(r), m.i32(r + 4));
            let mut f = e + 1;
            while f <= ne {
                let r2 = edge_record(m, mesh, f);
                if m.i32(r2) == b && m.i32(r2 + 4) == a {
                    m.set_i32(r + 0xc, m.i32(r2 + 8));
                    m.set_i32(twin + 4 * f as u32, e);
                    break;
                }
                f += 1;
            }
        }
        e += 1;
        ne = m.i32(mesh + mo::NB_EDGES);
    }
    clean_polygons(m, o, &mut w);
    let cos_angle = si_x87::trig::cos(angle as f64).to_f64();
    if merge_polygons != 0 {
        merge_coplanar_polygons(m, &mut w, cos_angle);
    }
    // the stars and edges built again, without corner normals
    if w.vmap != 0 {
        m.free(w.vmap);
    }
    if w.pmap != 0 {
        m.free(w.pmap);
    }
    mesh_dispose_edges(m, mesh);
    free_stars(m, mesh);
    update_nodes(m, o, mesh, 0);
    drop_straight_vertices(m, mesh, cos_angle);
    // vertices without a star dropped, the array compacted
    let map = m.si_calloc(m.i32(mesh + mo::NB_VERTICES) as u32, 4);
    let mut kept = 0i32;
    let verts = m.u32(mesh + mo::VERTICES);
    let mut dst = verts;
    let mut k = 0i32;
    while k < m.i32(mesh + mo::NB_VERTICES) {
        let src = verts.wrapping_add((k as u32).wrapping_mul(mo::VERTEX));
        if m.u16(src + mo::VERTEX_NB_STAR) != 0 {
            m.set_i32(map + 4 * k as u32, kept);
            kept += 1;
            if src != dst {
                for c in 0..3 {
                    m.set_u32(dst + 4 * c, m.u32(src + 4 * c));
                }
                m.set_u16(dst + mo::VERTEX_FLAG, m.u16(src + mo::VERTEX_FLAG));
                m.set_u16(dst + mo::VERTEX_NB_STAR, m.u16(src + mo::VERTEX_NB_STAR));
                m.set_u32(dst + mo::VERTEX_STAR, m.u32(src + mo::VERTEX_STAR));
                m.set_u16(src + mo::VERTEX_NB_STAR, 0);
                m.set_u32(src + mo::VERTEX_STAR, 0);
            }
            dst = dst.wrapping_add(mo::VERTEX);
        }
        k += 1;
    }
    // the corners follow, polygons of fewer than three corners go (the last moves in)
    let mut p = 0i32;
    while p < m.i32(mesh + mo::NB_POLYGONS) {
        let poly = polygon_at(m, mesh, p);
        let mut count = 0u32;
        let mut c = 0i32;
        while c < m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
            let cn = node(m.u32(poly + mo::POLYGON_NODES), c);
            let v = m.i32(cn);
            if v != -1 {
                m.set_i32(cn, m.i32(map.wrapping_add((v as u32).wrapping_mul(4))));
            }
            c += 1;
            count = m.u16(poly + mo::POLYGON_NB_NODES) as u32;
        }
        if m.u16(poly + mo::POLYGON_NB_NODES) < 3 {
            nodes_dispose(m, poly + mo::POLYGON_NODES, -1, count as i32);
            m.set_u16(poly + mo::POLYGON_NB_NODES, 0);
            let last = polygon_at(m, mesh, m.i32(mesh + mo::NB_POLYGONS) - 1);
            move_polygon(m, poly, last);
            m.set_i32(mesh + mo::NB_POLYGONS, m.i32(mesh + mo::NB_POLYGONS) - 1);
            p -= 1;
        }
        p += 1;
    }
    free_stars(m, mesh);
    m.set_i32(mesh + mo::NB_VERTICES, kept);
    mesh_dispose_edges(m, mesh);
    if map != 0 {
        m.free(map);
    }
    update_nodes(m, o, mesh, 0);
    for b in [w.normals, w.near, w.twin] {
        if b != 0 {
            m.free(b);
        }
    }
}

/// Every vertex's star freed (`free`) and its count 0.
fn free_stars(m: &mut Mem, mesh: u32) {
    let verts = m.u32(mesh + mo::VERTICES);
    let mut k = 0i32;
    while k < m.i32(mesh + mo::NB_VERTICES) {
        let vr = verts.wrapping_add((k as u32).wrapping_mul(mo::VERTEX));
        let s = m.u32(vr + mo::VERTEX_STAR);
        if s != 0 {
            m.free(s);
            m.set_u32(vr + mo::VERTEX_STAR, 0);
        }
        m.set_u16(vr + mo::VERTEX_NB_STAR, 0);
        k += 1;
    }
}

/// Step 1: the groups of close vertices and their new vertices.
fn merge_close_vertices(m: &mut Mem, o: &mut dyn Outside, w: &mut Work, tol2: F80, by_star: i16) {
    let mesh = w.mesh;
    let n = m.i32(mesh + mo::NB_VERTICES);
    let near = w.near;
    // each vertex's nearest earlier one within the tolerance
    let mut i = 0i32;
    while i < n - 1 {
        let vr = vertex_at(m, mesh, i);
        let p = f3(m, vr);
        o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
        if by_star == 0 {
            for j in i + 1..n {
                let d = distance2(p, f3(m, vertex_at(m, mesh, j)));
                let slot = near + 8 * j as u32;
                if d.lt(tol2) && (m.i32(slot) == j || d.lt(x(m.f32(slot + 4)))) {
                    m.set_f32(slot + 4, d.to_f32());
                    m.set_i32(slot, i);
                }
            }
        } else {
            let root = m.i32(near + 8 * i as u32);
            let mut k = 0u32;
            while k < m.u16(vr + mo::VERTEX_NB_STAR) as u32 {
                let j = m.i32(m.u32(vr + mo::VERTEX_STAR).wrapping_add(12 * k));
                let d = distance2(p, f3(m, vertex_at(m, mesh, j)));
                let slot = near.wrapping_add((j as u32).wrapping_mul(8));
                if d.lt(tol2) && (m.i32(slot) == j || d.lt(x(m.f32(slot + 4)))) {
                    m.set_f32(slot + 4, d.to_f32());
                    m.set_i32(slot, root);
                }
                k += 1;
            }
        }
        i += 1;
    }
    // each vertex with followers: a new vertex at the end, the average of the group
    let mut i = 0i32;
    while i < n - 1 {
        if m.i32(w.vmap + 4 * i as u32) == i {
            let mut created = false;
            let mut count = 0i32;
            let mut new = i;
            o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
            for j in i + 1..n {
                if m.i32(w.vmap + 4 * j as u32) != j || m.i32(near + 8 * j as u32) != i {
                    continue;
                }
                if !created {
                    w.vmap = m.si_realloc(w.vmap, ((m.i32(mesh + mo::NB_VERTICES) + 1) as u32) << 2);
                    w.normals = m.si_realloc(w.normals, ((m.i32(mesh + mo::NB_VERTICES) + 1) as u32) << 4);
                    svertex_reallocate(m, mesh, 1);
                    new = m.i32(mesh + mo::NB_VERTICES);
                    let (nr, ir) = (vertex_at(m, mesh, new), vertex_at(m, mesh, i));
                    for c in 0..3 {
                        m.set_u32(nr + 4 * c, m.u32(ir + 4 * c));
                    }
                    count = 1;
                    m.set_u16(nr + mo::VERTEX_NB_STAR, 0);
                    m.set_u32(nr + mo::VERTEX_STAR, 0);
                    m.set_u16(nr + mo::VERTEX_FLAG, 0);
                    m.set_i32(w.vmap + 4 * new as u32, new);
                    m.set_i32(w.vmap + 4 * i as u32, new);
                    let nrm = compute_mesh_vertex_normal(m, mesh, i);
                    for (slot, cnt) in [(w.normals + 16 * i as u32, 1u32), (w.normals + 16 * new as u32, 1)] {
                        for c in 0..3 {
                            m.set_f32(slot + 4 * c, nrm[c as usize]);
                        }
                        m.set_u32(slot + 12, cnt);
                    }
                    m.set_i32(mesh + mo::NB_VERTICES, m.i32(mesh + mo::NB_VERTICES) + 1);
                    created = true;
                }
                let (nr, jr) = (vertex_at(m, mesh, new), vertex_at(m, mesh, j));
                for c in 0..3 {
                    m.set_f32(nr + 4 * c, (x(m.f32(nr + 4 * c)) + m.f32(jr + 4 * c)).to_f32());
                }
                count += 1;
                m.set_i32(w.vmap + 4 * j as u32, new);
                let nrm = compute_mesh_vertex_normal(m, mesh, j);
                let (js, ns) = (w.normals + 16 * j as u32, w.normals + 16 * new as u32);
                for c in 0..3 {
                    m.set_f32(js + 4 * c, nrm[c as usize]);
                }
                m.set_u32(js + 12, 1);
                for c in 0..3 {
                    m.set_f32(ns + 4 * c, (x(m.f32(ns + 4 * c)) + m.f32(js + 4 * c)).to_f32());
                }
                m.set_i32(ns + 12, m.i32(ns + 12) + 1);
            }
            if count > 0 {
                let nr = vertex_at(m, mesh, new);
                let c80 = F80::from_i32(count);
                for c in 0..3 {
                    m.set_f32(nr + 4 * c, (x(m.f32(nr + 4 * c)) / c80).to_f32());
                }
            }
        }
        i += 1;
    }
}

/// Step 2: each polygon's corners moved to their new vertices, its repeated corners and
/// "come backs" removed, and the polygon deleted when too little of it is left.
fn clean_polygons(m: &mut Mem, o: &mut dyn Outside, w: &mut Work) {
    let mesh = w.mesh;
    let mut p = 0i32;
    while p < m.i32(mesh + mo::NB_POLYGONS) {
        let poly = polygon_at(m, mesh, p);
        // the corners of moved vertices: the new vertex, the normal corrected
        let mut c = 0i32;
        while c < m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
            o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
            let cn = node(m.u32(poly + mo::POLYGON_NODES), c);
            let v = m.i32(cn);
            if v != -1 {
                let nv = m.i32(w.vmap.wrapping_add((v as u32).wrapping_mul(4)));
                if v != nv {
                    correct_corner_normal(m, w, cn, v, nv);
                    m.set_i32(cn, nv);
                }
            }
            c += 1;
        }
        // repeated corners, contour by contour (the contour's last corner before its first)
        let mut written = 0i32;
        let mut removed = 0i32;
        let mut start = 0i32;
        let mut dst = 0i32;
        let mut i = 0i32;
        if 0 <= m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
            loop {
                o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
                let count = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
                if i == count || m.i32(node(m.u32(poly + mo::POLYGON_NODES), i)) == -1 {
                    let end = i - 1;
                    w.last_end = end;
                    if start <= end {
                        // (the original reads this word before the test, also when the
                        // contour is empty and it lies before the array)
                        let mut prev = m.i32(node(m.u32(poly + mo::POLYGON_NODES), end));
                        let mut t = start;
                        while t <= end {
                            let nodes = m.u32(poly + mo::POLYGON_NODES);
                            let cur = m.i32(node(nodes, t));
                            if prev == cur {
                                removed += 1;
                            } else {
                                copy_corner(m, node(nodes, dst), node(nodes, t));
                                dst += 1;
                                written += 1;
                            }
                            prev = cur;
                            t += 1;
                        }
                    }
                    start = i + 1;
                    let count = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
                    let nodes = m.u32(poly + mo::POLYGON_NODES);
                    if i < count && written < count && m.i32(node(nodes, i)) == -1 {
                        m.set_i32(node(nodes, dst), -1);
                        dst += 1;
                        written += 1;
                    }
                }
                i += 1;
                if i > m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
                    break;
                }
            }
        }
        let left = (m.u16(poly + mo::POLYGON_NB_NODES) as i32 - removed) as u16;
        m.set_u16(poly + mo::POLYGON_NB_NODES, left);
        let delete = if left <= 2 {
            true
        } else {
            while remove_come_back(m, m.u32(poly + mo::POLYGON_NODES), poly + mo::POLYGON_NB_NODES) != 0 {}
            let count = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
            // the first contour's length, plus one when a separator ends it
            let mut first = count;
            if 0 < count {
                let nodes = m.u32(poly + mo::POLYGON_NODES);
                if m.i32(nodes) == -1 {
                    first = 1;
                } else {
                    let mut s = 0;
                    loop {
                        s += 1;
                        if s >= count {
                            break;
                        }
                        if m.i32(node(nodes, s)) == -1 {
                            first = s + 1;
                            break;
                        }
                    }
                }
            }
            if first <= 2 {
                m.set_u16(poly + mo::POLYGON_NB_NODES, 0);
            }
            if m.u16(poly + mo::POLYGON_NB_NODES) <= 2 {
                true
            } else {
                computes_polygon_normal(m, mesh, p);
                if m.u16(poly + mo::POLYGON_NB_NODES) <= 2 {
                    true
                } else {
                    let n = f3(m, poly);
                    ((x(n[0]) * n[0] + x(n[1]) * n[1]) + x(n[2]) * n[2]).lt(x(MIN_NORMAL2))
                }
            }
        };
        if delete {
            let nodes = m.u32(poly + mo::POLYGON_NODES);
            if nodes != 0 {
                m.free(nodes);
                m.set_u32(poly + mo::POLYGON_NODES, 0);
            }
            m.set_u16(poly + mo::POLYGON_NB_NODES, 0);
            let last = polygon_at(m, mesh, m.i32(mesh + mo::NB_POLYGONS) - 1);
            move_polygon(m, poly, last);
            m.set_i32(mesh + mo::NB_POLYGONS, m.i32(mesh + mo::NB_POLYGONS) - 1);
            w.pmap_replace(m, p, -1);
            let np = m.i32(mesh + mo::NB_POLYGONS);
            w.pmap_replace(m, np, p);
            p -= 1;
        }
        p += 1;
    }
}

/// The normal of corner `cn`, whose vertex `v` became `nv`: the group's average normal
/// (each component divided by the count in the register) plus the corner's normal, minus
/// `v`'s own vertex normal; then scaled to length 1 when its float length is above 0.0001.
/// The x component goes on from the register; y and z from their sums rounded to float.
fn correct_corner_normal(m: &mut Mem, w: &Work, cn: u32, v: i32, nv: i32) {
    let ns = w.normals.wrapping_add((nv as u32).wrapping_mul(16));
    let cnt = F80::from_i32(m.i32(ns + 12));
    let ax = x(m.f32(ns)) / cnt;
    let ay = x(m.f32(ns + 4)) / cnt;
    let az = x(m.f32(ns + 8)) / cnt;
    let sx = ax + m.f32(cn + mo::NODE_NORMAL);
    let sy = (ay + m.f32(cn + mo::NODE_NORMAL + 4)).to_f32();
    let sz = (az + m.f32(cn + mo::NODE_NORMAL + 8)).to_f32();
    let vs = w.normals.wrapping_add((v as u32).wrapping_mul(16));
    m.set_f32(cn + mo::NODE_NORMAL, (sx - m.f32(vs)).to_f32());
    m.set_f32(cn + mo::NODE_NORMAL + 4, (x(sy) - m.f32(vs + 4)).to_f32());
    m.set_f32(cn + mo::NODE_NORMAL + 8, (x(sz) - m.f32(vs + 8)).to_f32());
    let len = length(f3(m, cn + mo::NODE_NORMAL)).to_f32();
    if x(len).abs().gt(x(MIN_LENGTH)) {
        for c in 0..3 {
            let a = cn + mo::NODE_NORMAL + 4 * c;
            m.set_f32(a, (x(m.f32(a)) / len).to_f32());
        }
    }
}

/// Step 5's second half: a vertex of exactly two star entries with two different polygons
/// (or one and none), whose turn from the first neighbour over it to the second has a
/// cosine (`SI_pointsCosAngle`) above `cos_angle`, leaves both polygons (their later
/// corners move down one by one, `SI_nodesCopy`); its star is freed and its count 0.
fn drop_straight_vertices(m: &mut Mem, mesh: u32, cos_angle: f64) {
    let mut k = 0i32;
    while k < m.i32(mesh + mo::NB_VERTICES) {
        let vr = vertex_at(m, mesh, k);
        if m.u16(vr + mo::VERTEX_NB_STAR) == 2 {
            let star = m.u32(vr + mo::VERTEX_STAR);
            let (pa, pb) = (m.i32(star + 8), m.i32(star + 0x14));
            if pa != pb {
                let a = f3(m, vertex_at(m, mesh, m.i32(star)));
                let b = f3(m, vr);
                let c = f3(m, vertex_at(m, mesh, m.i32(star + 0xc)));
                let turn = points_cos_angle(&vector(a), &vector(b), &vector(c));
                if x(cos_angle).lt(x(turn)) {
                    for p in [pa, pb] {
                        if p < 0 {
                            continue;
                        }
                        let poly = polygon_at(m, mesh, p);
                        if m.u16(poly + mo::POLYGON_NB_NODES) == 0 {
                            continue;
                        }
                        let mut i = 0i32;
                        loop {
                            if m.i32(node(m.u32(poly + mo::POLYGON_NODES), i)) == k {
                                m.set_u16(poly + mo::POLYGON_NB_NODES, m.u16(poly + mo::POLYGON_NB_NODES).wrapping_sub(1));
                                let mut t = i;
                                while t < m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
                                    let nodes = m.u32(poly + mo::POLYGON_NODES);
                                    nodes_copy(m, 0, 0, node(nodes, t), node(nodes, t + 1));
                                    t += 1;
                                }
                                i -= 1;
                            }
                            i += 1;
                            if i >= m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
                                break;
                            }
                        }
                    }
                    let s = m.u32(vr + mo::VERTEX_STAR);
                    m.set_u16(vr + mo::VERTEX_NB_STAR, 0);
                    if s != 0 {
                        m.free(s);
                        m.set_u32(vr + mo::VERTEX_STAR, 0);
                    }
                }
            }
        }
        k += 1;
    }
}

/// Step 4: polygons merged along their edges (the dialog's "merge polygons").
fn merge_coplanar_polygons(m: &mut Mem, w: &mut Work, cos_angle: f64) {
    let mesh = w.mesh;
    computes_polygon_normal(m, mesh, -1);
    let mut e = 1i32;
    if e > m.i32(mesh + mo::NB_EDGES) {
        return;
    }
    loop {
        merge_along_edge(m, w, e, cos_angle);
        e += 1;
        if m.i32(mesh + mo::NB_EDGES) < e {
            break;
        }
    }
}

/// One edge of step 4.
fn merge_along_edge(m: &mut Mem, w: &mut Work, e: i32, cos_angle: f64) {
    let mesh = w.mesh;
    let rec = edge_record(m, mesh, e);
    if m.i32(w.twin + 4 * e as u32) != e {
        return;
    }
    let (pa, pb) = (m.i32(rec + 8), m.i32(rec + 0xc));
    if pa == -1 || pb == -1 {
        return;
    }
    let pm = |m: &Mem, p: i32| m.i32(w.pmap.wrapping_add((p as u32).wrapping_mul(4)));
    let (mut f0, mut f4) = (pm(m, pa), pm(m, pb));
    if f0 > f4 {
        let (v0, v1) = (m.u32(rec), m.u32(rec + 4));
        m.set_i32(rec + 8, pb);
        m.set_i32(rec + 0xc, pa);
        m.set_u32(rec, v1);
        m.set_u32(rec + 4, v0);
        f0 = pm(m, pb);
        f4 = pm(m, pa);
    }
    if f0 < 0 {
        return;
    }
    let pp = polygon_at(m, mesh, f0);
    let qq = polygon_at(m, mesh, f4);
    let n0 = f3(m, pp);
    let n1 = f3(m, qq);
    let (mat0, mat1) = (m.u16(pp + mo::POLYGON_MATERIAL), m.u16(qq + mo::POLYGON_MATERIAL));
    if length(n0).lt(x(MIN_LENGTH)) || length(n1).lt(x(MIN_LENGTH)) {
        return;
    }
    let vm = |m: &Mem, v: u32| m.i32(w.vmap.wrapping_add(v.wrapping_mul(4)));
    let a = vm(m, m.u32(rec));
    let b = vm(m, m.u32(rec + 4));
    if a == b {
        return;
    }
    let c = vector_cos_angle(&vector(n0), &vector(n1));
    if !x(cos_angle).lt(x(c)) || mat0 != mat1 || edge_info_is_consistent(m, mesh, rec) == 0 {
        return;
    }
    if f0 == f4 {
        join_contours(m, w, pp, f0, a, b);
    } else {
        merge_two(m, w, pp, qq, f0, f4, a, b);
    }
}

/// Step 4 for an edge between two contours of one polygon (`pp`, number `f`): the corners
/// a → b and b → a are looked for contour by contour (the last contour that has either
/// gives the start both walks wrap to); where both pairs share a corner, the pair a → b is
/// removed; else the two pieces between them become two contours, the one whose bounding
/// box does not hold the other's first.
fn join_contours(m: &mut Mem, w: &mut Work, pp: u32, f: i32, a: i32, b: i32) {
    let mesh = w.mesh;
    let (mut ab_a, mut ab_b, mut ba_b, mut ba_a) = (-1i32, -1i32, -1i32, -1i32);
    let mut found_start = 0i32;
    let mut start = 0i32;
    let n = m.u16(pp + mo::POLYGON_NB_NODES) as i32;
    let mut i = 0i32;
    while i <= n {
        let nodes = m.u32(pp + mo::POLYGON_NODES);
        if i == n || m.i32(node(nodes, i)) == -1 {
            let (mut got_ab, mut got_ba) = (false, false);
            let end = i - 1;
            w.last_end = end;
            let mut prev_i = end;
            // (the original reads this word before the test, also when the contour is
            // empty and it lies before the array)
            let mut prev = if start <= end { m.i32(node(nodes, end)) } else { 0 };
            let mut t = start;
            while t <= end {
                let cur = m.i32(node(nodes, t));
                if prev == a && cur == b {
                    found_start = start;
                    got_ab = true;
                    ab_a = prev_i;
                    ab_b = t;
                }
                if prev == b && cur == a {
                    found_start = start;
                    got_ba = true;
                    ba_b = prev_i;
                    ba_a = t;
                }
                prev_i = t;
                prev = cur;
                t += 1;
            }
            start = i + 1;
            if got_ab && got_ba {
                break;
            }
        }
        i += 1;
    }
    if ab_a == -1 || ab_b == -1 || ba_b == -1 || ba_a == -1 {
        return;
    }
    if ab_a == ba_a || ab_b == ba_b {
        // the pair a → b removed. The texture pairs of the corners kept go, through
        // `SI_nodesCopyUV`, into the block of the last merge of two polygons (the
        // original's stale stack slot), not into the corners
        let merged = w.merged;
        if merged.is_none() {
            stale::note(); // the slot holds what the stack held: the copy is left out here
        }
        // the block holds `room` corners; the original writes on past its end, into
        // whatever its heap has there (another block, by the layout of its heap): here
        // those copies are left out and counted as undefined
        let room = merged.and_then(|b| m.block_size(b)).map_or(0, |n| n / mo::NODE);
        let mut dst = 0i32;
        let mut k = 0i32;
        while k < m.u16(pp + mo::POLYGON_NB_NODES) as i32 {
            if k != ab_a && k != ab_b {
                let nodes = m.u32(pp + mo::POLYGON_NODES);
                let uv_dst = merged.map(|b| b.wrapping_add((dst as u32).wrapping_mul(mo::NODE)));
                let uv_dst = match uv_dst {
                    Some(_) if dst as u32 >= room => {
                        stale::note();
                        None
                    }
                    u => u,
                };
                copy_corner_uv_to(m, node(nodes, dst), node(nodes, k), uv_dst);
                dst += 1;
            }
            k += 1;
        }
        let cnt = m.u16(pp + mo::POLYGON_NB_NODES);
        m.set_u16(pp + mo::POLYGON_NB_NODES, if cnt <= 1 { 0 } else { cnt - 2 });
        if m.u16(pp + mo::POLYGON_NB_NODES) == 0 {
            nodes_dispose(m, pp + mo::POLYGON_NODES, -1, 0);
            m.set_u16(pp + mo::POLYGON_NB_NODES, 0);
            w.pmap_replace(m, f, -1);
            let np = m.i32(mesh + mo::NB_POLYGONS) - 1;
            m.set_i32(mesh + mo::NB_POLYGONS, np);
            if f != np {
                let last = polygon_at(m, mesh, np);
                move_polygon(m, pp, last);
                w.pmap_replace(m, np, f);
            }
        }
    } else {
        split_contours(m, w, pp, ab_a, ab_b, ba_b, ba_a, found_start);
    }
    // repeated corners (separators too) after the join, and a separator at the end
    let mut removed = 0i32;
    let mut prev = -1i32;
    let mut dst = 0i32;
    let mut k = 0i32;
    if m.u16(pp + mo::POLYGON_NB_NODES) != 0 {
        while k < m.u16(pp + mo::POLYGON_NB_NODES) as i32 {
            let nodes = m.u32(pp + mo::POLYGON_NODES);
            let cur = m.i32(node(nodes, k));
            if cur == prev {
                removed += 1;
            } else {
                copy_corner(m, node(nodes, dst), node(nodes, k));
                dst += 1;
                prev = cur;
            }
            k += 1;
        }
    }
    let cnt = m.u16(pp + mo::POLYGON_NB_NODES) as i32;
    m.set_u16(pp + mo::POLYGON_NB_NODES, if cnt < removed { 0 } else { (cnt - removed) as u16 });
    let cnt = m.u16(pp + mo::POLYGON_NB_NODES) as i32;
    // (for a count of 0 the original reads the word before the array, or below address 0)
    if m.i32(node(m.u32(pp + mo::POLYGON_NODES), cnt - 1)) == -1 {
        m.set_u16(pp + mo::POLYGON_NB_NODES, (cnt - 1) as u16);
    }
}

/// The two pieces of a polygon between the corners a → b and b → a, each collected into a
/// block of its own and written back as two contours from the start the walks wrap to.
#[allow(clippy::too_many_arguments)]
fn split_contours(m: &mut Mem, w: &mut Work, pp: u32, ab_a: i32, ab_b: i32, ba_b: i32, ba_a: i32, found_start: i32) {
    let mesh = w.mesh;
    let n0 = m.u16(pp + mo::POLYGON_NB_NODES) as u32;
    let blocks = m.local(8);
    let b0 = m.si_calloc(round4(n0), mo::NODE);
    m.set_u32(blocks, b0);
    let b1 = m.si_calloc(round4(m.u16(pp + mo::POLYGON_NB_NODES) as u32), mo::NODE);
    m.set_u32(blocks + 4, b1);
    let cnt = |m: &Mem| m.u16(pp + mo::POLYGON_NB_NODES) as i32;
    let at_end = |m: &Mem, k: i32| k == m.u16(pp + mo::POLYGON_NB_NODES) as i32 || m.i32(node(m.u32(pp + mo::POLYGON_NODES), k)) == -1;
    let mut s = ba_a + 1;
    if cnt(m) <= s || m.i32(node(m.u32(pp + mo::POLYGON_NODES), s)) == -1 {
        s = found_start;
    }
    let mut stop = ab_a + 1;
    if at_end(m, stop) {
        stop = found_start;
    }
    let tail = w.last_end + 1;
    let mut c0 = 0i32;
    if s != stop {
        loop {
            let nodes = m.u32(pp + mo::POLYGON_NODES);
            copy_corner(m, node(b0, c0), node(nodes, s));
            c0 += 1;
            if s + 1 == cnt(m) || m.i32(node(m.u32(pp + mo::POLYGON_NODES), s + 1)) == -1 {
                s = found_start;
            } else {
                s += 1;
            }
            if s == stop {
                break;
            }
        }
    }
    let mut c1 = 0i32;
    let mut t = ab_b;
    if t != ba_b {
        loop {
            let nodes = m.u32(pp + mo::POLYGON_NODES);
            copy_corner(m, node(b1, c1), node(nodes, t));
            c1 += 1;
            if t + 1 == cnt(m) || m.i32(node(m.u32(pp + mo::POLYGON_NODES), t + 1)) == -1 {
                t = found_start;
            } else {
                t += 1;
            }
            if t == ba_b {
                break;
            }
        }
    }
    // the bounding boxes of the two pieces (from each block's first corner, also for an
    // empty piece)
    let verts = m.u32(mesh + mo::VERTICES);
    let pos = |m: &Mem, v: i32| f3(m, verts.wrapping_add((v as u32).wrapping_mul(mo::VERTEX)));
    let mut lo = [[0.0f32; 3]; 2];
    let mut hi = [[0.0f32; 3]; 2];
    for (piece, (blk, count)) in [(b0, c0), (b1, c1)].into_iter().enumerate() {
        let first = pos(m, m.i32(blk));
        lo[piece] = first;
        hi[piece] = first;
        for d in 1..count {
            let q = pos(m, m.i32(node(blk, d)));
            for c in 0..3 {
                if x(q[c]).lt(x(lo[piece][c])) {
                    lo[piece][c] = q[c];
                }
            }
            for c in 0..3 {
                if x(q[c]).gt(x(hi[piece][c])) {
                    hi[piece][c] = q[c];
                }
            }
        }
    }
    let outside = (0..3).any(|c| !x(lo[1][c]).ge(x(lo[0][c]))) || (0..3).any(|c| !x(hi[1][c]).le(x(hi[0][c])));
    let mut at = found_start;
    let order: [usize; 2] = if outside { [1, 0] } else { [0, 1] };
    for (slot, &piece) in order.iter().enumerate() {
        let (blk, count) = if piece == 0 { (b0, c0) } else { (b1, c1) };
        for d in 0..count {
            let nodes = m.u32(pp + mo::POLYGON_NODES);
            copy_corner(m, node(nodes, at), node(blk, d));
            at += 1;
        }
        if slot == 0 {
            let nodes = m.u32(pp + mo::POLYGON_NODES);
            m.set_i32(node(nodes, at), -1);
            at += 1;
        }
    }
    // the corners after the contour the walks ended in, behind
    let mut k = tail;
    while k < cnt(m) {
        let nodes = m.u32(pp + mo::POLYGON_NODES);
        copy_corner(m, node(nodes, at), node(nodes, k));
        at += 1;
        k += 1;
    }
    m.set_u16(pp + mo::POLYGON_NB_NODES, m.u16(pp + mo::POLYGON_NB_NODES).wrapping_sub(1));
    nodes_dispose(m, blocks, -1, c0);
    nodes_dispose(m, blocks + 4, -1, c1);
    m.release(8);
}

/// Step 4 for an edge between two polygons: `qq` (number `f4`) joins `pp` (number `f0`)
/// in a new corner block; which corners come in which order depends on whether the edge
/// lies on the first contour of each (see the finding).
#[allow(clippy::too_many_arguments)]
fn merge_two(m: &mut Mem, w: &mut Work, pp: u32, qq: u32, mut f0: i32, mut f4: i32, a: i32, b: i32) {
    let mesh = w.mesh;
    let pcount = |m: &Mem| m.u16(pp + mo::POLYGON_NB_NODES) as i32;
    let qcount = |m: &Mem| m.u16(qq + mo::POLYGON_NB_NODES) as i32;
    let pv = |m: &Mem, k: i32| m.i32(node(m.u32(pp + mo::POLYGON_NODES), k));
    let qv = |m: &Mem, k: i32| m.i32(node(m.u32(qq + mo::POLYGON_NODES), k));
    let total = (m.u16(qq + mo::POLYGON_NB_NODES).wrapping_add(m.u16(pp + mo::POLYGON_NB_NODES))).wrapping_sub(2);
    let blk = m.si_calloc(round4(total as u32), mo::NODE);
    w.merged = Some(blk);
    let mut aa = 0i32;
    let push = |m: &mut Mem, aa: &mut i32, src: u32| {
        copy_corner(m, node(blk, *aa), src);
        *aa += 1;
    };
    // a → b in `pp`, b → a in `qq`, with the start of the contour each lies on
    let (mut p_next, mut p_at, mut p_start) = (-1i32, -1i32, 0i32);
    {
        let nodes = m.u32(pp + mo::POLYGON_NODES);
        let mut u = 0i32;
        while u < pcount(m) {
            let v = m.i32(node(nodes, u));
            if v == -1 {
                p_start = u + 1;
            }
            if v == a {
                p_next = p_start;
                if u != pcount(m) - 1 && m.i32(node(nodes, u + 1)) != -1 {
                    p_next = u + 1;
                }
                p_at = u;
                if m.i32(node(nodes, p_next)) == b {
                    break;
                }
            }
            u += 1;
        }
    }
    let (mut q_next, mut q_at, mut q_start) = (-1i32, -1i32, 0i32);
    {
        let nodes = m.u32(qq + mo::POLYGON_NODES);
        let n = qcount(m);
        let mut u = 0i32;
        while u < n {
            let v = m.i32(node(nodes, u));
            if v == -1 {
                q_start = u + 1;
            }
            if v == b {
                q_next = q_start;
                if u != n - 1 && m.i32(node(nodes, u + 1)) != -1 {
                    q_next = u + 1;
                }
                q_at = u;
                if m.i32(node(nodes, q_next)) == a {
                    break;
                }
            }
            u += 1;
        }
    }
    if p_at == -1 || p_next == -1 || q_next == -1 || q_at == -1 {
        return; // the block is lost
    }
    let pn = |m: &Mem, k: i32| node(m.u32(pp + mo::POLYGON_NODES), k);
    let qn = |m: &Mem, k: i32| node(m.u32(qq + mo::POLYGON_NODES), k);
    let mut swapped = false;
    if p_start != 0 {
        // the edge on a hole of `pp`
        let mut u = 0i32;
        while u < pcount(m) {
            push(m, &mut aa, pn(m, u));
            if u == p_at {
                break;
            }
            u += 1;
        }
        loop {
            q_next += 1;
            if qcount(m) < q_next {
                break;
            }
            if q_next != qcount(m) && qv(m, q_next) != -1 {
                if q_next == q_at {
                    break;
                }
                push(m, &mut aa, qn(m, q_next));
                continue;
            }
            w.last_end = q_next;
            q_next = q_start - 1;
        }
        if p_at < p_next && p_next < pcount(m) {
            let mut u = p_next;
            while u < pcount(m) {
                push(m, &mut aa, pn(m, u));
                u += 1;
            }
        }
        for u in 0..q_start.max(0) {
            push(m, &mut aa, qn(m, u));
        }
        let mut u = w.last_end;
        while u < qcount(m) {
            push(m, &mut aa, qn(m, u));
            u += 1;
        }
    } else if q_start != 0 {
        // the edge on a hole of `qq`
        let resume = q_at + 1;
        let mut u = 0i32;
        while u < qcount(m) {
            push(m, &mut aa, qn(m, u));
            if u == q_at {
                break;
            }
            u += 1;
        }
        loop {
            p_next += 1;
            if pcount(m) < p_next {
                break;
            }
            if p_next != pcount(m) && pv(m, p_next) != -1 {
                if p_next == p_at {
                    break;
                }
                push(m, &mut aa, pn(m, p_next));
                continue;
            }
            w.last_end = p_next;
            p_next = -1;
        }
        let mut u = resume;
        while u < qcount(m) {
            push(m, &mut aa, qn(m, u));
            u += 1;
        }
        let mut u = w.last_end;
        while u < pcount(m) {
            push(m, &mut aa, pn(m, u));
            u += 1;
        }
        // the two numbers trade places (the polygon the corners go to stays `pp`)
        let old_f0 = f0;
        f0 = f4;
        f4 = old_f0;
        swapped = true;
        w.pmap_replace(m, f0, f4);
    } else {
        // the edge on the outlines of both
        for u in 0..p_at.max(0) {
            push(m, &mut aa, pn(m, u));
        }
        if q_next != q_at {
            loop {
                push(m, &mut aa, qn(m, q_next));
                q_next += 1;
                if q_next == qcount(m) || qv(m, q_next) == -1 {
                    q_next = 0;
                }
                if q_next == q_at {
                    break;
                }
            }
        }
        let mut rest = true;
        if p_next == 0 {
            if p_at != pcount(m) - 1 {
                p_next = p_at + 1;
            }
            rest = p_next != 0;
        }
        if rest {
            let mut u = p_next;
            while u < pcount(m) {
                push(m, &mut aa, pn(m, u));
                u += 1;
            }
        }
        // `qq`'s holes
        let n = qcount(m);
        let mut u = 0i32;
        if n != 0 && qv(m, 0) != -1 {
            loop {
                u += 1;
                if n <= u || qv(m, u) == -1 {
                    break;
                }
            }
        }
        if u != n && u < n {
            while u < qcount(m) {
                push(m, &mut aa, qn(m, u));
                u += 1;
            }
        }
    }
    if !swapped {
        w.pmap_replace(m, f4, f0);
    }
    m.set_u16(pp + mo::POLYGON_FLAG, 0);
    let pc = m.u16(pp + mo::POLYGON_NB_NODES) as i32;
    nodes_dispose(m, pp + mo::POLYGON_NODES, -1, pc);
    let count_at = m.local(4);
    m.set_u16(count_at, aa as u16);
    while remove_come_back(m, blk, count_at) != 0 {}
    let left = m.u16(count_at);
    m.release(4);
    m.set_u16(pp + mo::POLYGON_NB_NODES, left);
    m.set_u32(pp + mo::POLYGON_NODES, blk);
    computes_polygon_normal(m, mesh, f0);
    let qc = m.u16(qq + mo::POLYGON_NB_NODES) as i32;
    nodes_dispose(m, qq + mo::POLYGON_NODES, -1, qc);
    m.set_u16(qq + mo::POLYGON_FLAG, 0);
    m.set_u16(qq + mo::POLYGON_NB_NODES, 0);
    let np = m.i32(mesh + mo::NB_POLYGONS) - 1;
    m.set_i32(mesh + mo::NB_POLYGONS, np);
    if (swapped && f0 == np) || (!swapped && f4 == np) {
        return;
    }
    // the last polygon moves into `qq`'s place (with the numbers traded, the table sends
    // the last one's references to `pp`'s number: the original's)
    w.pmap_replace(m, m.i32(mesh + mo::NB_POLYGONS), f4);
    let last = polygon_at(m, mesh, m.i32(mesh + mo::NB_POLYGONS));
    move_polygon(m, qq, last);
}
