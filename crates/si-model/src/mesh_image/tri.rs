// SPDX-License-Identifier: MIT
//! Triangles on the image: `SI_triangulateContour` (`triangulate.c`) as its callers see it,
//! and `SI_meshTriangulate` (`obj_global.c`), which replaces every polygon of a mesh by
//! triangles (finding 0015, "Triangles from a polygon" and the seventh pass).

use si_mem::Mem;

use super::rebuild::{compute_hard_edge, computes_polygon_normal, insert_ipolygon, optimize_display, polygon_initialize};
use super::{callee, edge_record, f3, off as mo, polygon_at, vertex_at};
use crate::model_image::Outside;

/// [soft 0x0851dadc SI_triangulateContour] on the image: `n_points` points of 12 bytes at
/// `points` (already in the polygon's plane), `n_corners` point numbers at `corners` (−1
/// between contours), the triangle count stored at `count_at` and a new array of triangles
/// (three point numbers each) at `tris_at`; the seventh argument is not read. Answers 1, or
/// 0 when a hole cannot be joined.
///
/// What it leaves in memory is the original's: the global `mD0` (0x08822450) holding the
/// last cross product `SI_segmentIntersect` computed (rounded to float); every coordinate of every point rounded in
/// the caller's array; the caller's corner list compacted in place where a corner repeats
/// the one before it (the entries behind the new end as they were); `SI_malloc` of the work
/// list ((holes + corners) words), the count 0, `SI_malloc` of the triangles ((holes +
/// corners) × 3 − 6 words), and, with holes, `mergeHoles`' three blocks (two lists of
/// (holes + corners) words and a contour table of 32 bytes for each hole and one more),
/// freed when the holes are joined; the work list freed at the end. The rounding, the
/// merging and the order of the cuts are the value form's, [`crate::triangulate`]
/// (`SI_triangulateContour`, `mergeHoles`, `SI_segmentIntersect`, checked by emulation on
/// 11,000 polygons).
///
/// Where the Rust differs: when a hole cannot be joined the original leaks the work list
/// and `mergeHoles`' three blocks holding what its search left in them; here the work
/// list holds the compacted corners and the three blocks what they were given.
pub fn triangulate_contour(m: &mut Mem, n_points: i32, points: u32, n_corners: i32, corners: u32, count_at: u32, tris_at: u32) -> i16 {
    let mut pts: Vec<[f32; 3]> = (0..n_points.max(0) as u32).map(|k| f3(m, points + 12 * k)).collect();
    let original: Vec<i32> = (0..n_corners.max(0) as u32).map(|k| m.i32(corners + 4 * k)).collect();
    let mut list = original.clone();
    crate::triangulate::take_md0();
    let (ok, triangles) = crate::triangulate::triangulate_contour(&mut pts, &mut list);
    // the global the bridges' and the ears' crossing tests leave their last product in
    if let Some(v) = crate::triangulate::take_md0() {
        m[super::g::MD0] = v;
    }
    for (k, p) in pts.iter().enumerate() {
        super::put_f3(m, points + 12 * k as u32, *p);
    }
    // the compaction, as the value form made it: its length
    let mut n = 0usize;
    if let Some(&end) = original.last() {
        let mut before = end;
        for &c in &original {
            if c != before {
                before = c;
                n += 1;
            }
        }
    }
    for (k, &c) in list.iter().enumerate().take(n) {
        m.set_i32(corners + 4 * k as u32, c);
    }
    let holes = list[..n].iter().filter(|&&c| c == -1).count() as u32;
    let total = holes + n as u32;
    let work = m.si_malloc(total.wrapping_mul(4));
    m.set_i32(count_at, 0);
    let tris = m.si_malloc(total.wrapping_mul(3).wrapping_sub(6).wrapping_mul(4));
    m.set_u32(tris_at, tris);
    for (k, &c) in list.iter().enumerate().take(n) {
        m.set_i32(work + 4 * k as u32, c);
    }
    if holes != 0 {
        let a = m.si_malloc(total.wrapping_mul(4));
        for (k, &c) in list.iter().enumerate().take(n) {
            m.set_i32(a + 4 * k as u32, c);
        }
        let b = m.si_malloc(total.wrapping_mul(4));
        let c = m.si_malloc((holes + 1).wrapping_mul(0x20));
        if !ok {
            return 0;
        }
        free_merge_blocks(m, holes as i32, a, b, c);
    }
    for (k, t) in triangles.iter().enumerate() {
        let at = tris + 12 * k as u32;
        m.set_i32(at, t[0]);
        m.set_i32(at + 4, t[1]);
        m.set_i32(at + 8, t[2]);
        m.set_i32(count_at, k as i32 + 1);
    }
    if work != 0 {
        m.free(work);
    }
    1
}

/// The frees at the end of `mergeHoles` (0x0851d9xx region; vectors of 2026-10-29): the two lists
/// swap their parts once a bridge, so after an odd number of holes the second is freed first,
/// after an even number the first; the contour table last.
pub(super) fn free_merge_blocks(m: &mut Mem, holes: i32, a: u32, b: u32, c: u32) {
    if holes & 1 == 1 {
        m.free(b);
        m.free(a);
    } else {
        m.free(a);
        m.free(b);
    }
    m.free(c);
}

/// The work arrays of [`mesh_triangulate`], each a heap block the original grows by its
/// steps.
struct Work {
    vert: u32,
    corner: u32,
    pos: u32,
    norm: u32,
    u: u32,
    v: u32,
    col: u32,
    hit: u32,
}

/// [soft 0x08443144 SI_meshTriangulate]: every polygon of `mesh` replaced by triangles, in
/// place; answers 1, or 0 when the new mesh cannot be made or a polygon's holes cannot be
/// joined (its triangles are then missing). `arg` is handed to the triangulator, which does
/// not read it.
///
/// Read from the listing (the order of operations, the blocks and their sizes):
///
/// 1. A new mesh (`SI_meshAllocate`) gets the vertices (`SI_vertexAllocate`; position and
///    flag, no star); the old edges' two vertices and flag go into three blocks of
///    `SI_malloc` (4 bytes an edge). The triangle count is counted in advance: per polygon
///    its corner records − 2 plus its separators (for every polygon, even of fewer than
///    three corners), and the new polygons are allocated for it. Eight work arrays follow,
///    `SI_calloc(1000, …)`: vertex numbers, corner numbers (grown by `SI_realloc` to
///    corners + 1 words for a polygon of more than 998 corners), positions, normals, u, v,
///    colours (grown together by 1000 entries when the distinct vertices reach the
///    capacity − 1), and one short a triangle (grown once a polygon by 1000).
/// 2. A polygon of three corners is taken as it is if its three vertices are in range and
///    the first differs from the other two, and unless one of its three directed edges
///    already runs along a polygon of the new mesh (a star entry of the new vertex with
///    that neighbour and a polygon). It gets `SI_polygonInitialize`, the three corners'
///    normal, u, v and colour, the material and the selection bit, and is filed
///    (`si_meshInsertIPolygon`).
/// 3. A polygon of four corners and more: its distinct vertices in order of first
///    appearance (each with the normal, u, v and colour of the corner that brought it), its
///    corners as their numbers (−1 for a separator), the positions through
///    `SI_matrixZAlign` of the stored normal ((y·m₁ⱼ + x·m₀ⱼ) + z·m₂ⱼ) + m₃ⱼ, stored as
///    floats), then [`triangulate_contour`]. A triangle **collides** when one of its three
///    directed edges runs along another polygon of the old mesh, or along a polygon of the
///    new one. On a collision the polygon's corner records are turned and the polygon is
///    tried again, up to (corners − 1) tries: for every contour end `e` (a separator's
///    position or the count) the records 0 … e − 1 are rotated by one (record 0 to e − 1),
///    which turns a polygon without holes by one corner and scrambles the contours of one
///    with holes. The triangles of the last try that do not collide are made as in 2, with
///    the attributes of their vertices' first corners.
/// 4. The old stars, edges, corners and polygons are freed (not the corners' further
///    texture pairs, and not the old vertex array, which is lost); the new vertices, edges
///    and polygons take their place, the new mesh's record is freed. Then
///    `SI_meshComputesPolygonNormal`, `SI_meshComputeHardEdge`, `SI_meshOptimizeDisplay`
///    (with `SI_showActivity` between) and `SI_meshUpdatePolygonLists`; **no corner
///    normals are computed**. Every edge gets bit 0x4; an edge between the two vertices of
///    an old edge (found in the first vertex's star, the first entry with that neighbour)
///    gets the old edge's whole flag. The work blocks are freed.
///
/// Where the Rust differs: the short a triangle of the last array is written only within
/// its block: the original grows it once a polygon, so a polygon of more than 1998
/// triangles writes past it into the heap; here those shorts are kept aside.
pub fn mesh_triangulate(m: &mut Mem, o: &mut dyn Outside, mesh: u32, arg: i16) -> i16 {
    let _ = arg;
    let mut result: i16 = 1;
    let new = super::mesh_allocate(m);
    if new == 0 {
        return 0;
    }
    let nv = m.i32(mesh + mo::NB_VERTICES);
    m.set_i32(new + mo::NB_VERTICES, nv);
    let verts = si_hrc::image::geom::vertex_allocate(m, nv);
    m.set_u32(new + mo::VERTICES, verts);
    if verts == 0 {
        m.free(new);
        return 0;
    }
    let (mut s, mut d) = (m.u32(mesh + mo::VERTICES), verts);
    let mut k = 0;
    while k < m.i32(new + mo::NB_VERTICES) {
        for w in 0..3 {
            m.set_u32(d + 4 * w, m.u32(s + 4 * w));
        }
        m.set_u16(d + mo::VERTEX_FLAG, m.u16(s + mo::VERTEX_FLAG));
        k += 1;
        s = s.wrapping_add(mo::VERTEX);
        d = d.wrapping_add(mo::VERTEX);
    }
    let old_edges = m.i32(mesh + mo::NB_EDGES);
    let size = (old_edges as u32).wrapping_mul(4);
    let (e0, e1, ef) = (m.si_malloc(size), m.si_malloc(size), m.si_malloc(size));
    let mut e = 1;
    while e <= old_edges {
        let r = edge_record(m, mesh, e);
        let at = 4 * (e as u32 - 1);
        m.set_u32(e0 + at, m.u32(r));
        m.set_u32(e1 + at, m.u32(r + 4));
        m.set_i32(ef + at, m.i16(r + mo::EDGE_FLAG) as i32);
        e += 1;
    }
    let mut total: i32 = 0;
    let np = m.i32(mesh + mo::NB_POLYGONS);
    for p in 0..np.max(0) {
        let poly = polygon_at(m, mesh, p);
        let n = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
        total = (n.wrapping_sub(2)).wrapping_add(total);
        let nodes = m.u32(poly + mo::POLYGON_NODES);
        for c in 0..n as u32 {
            if m.i32(nodes + mo::NODE * c) == -1 {
                total = total.wrapping_add(1);
            }
        }
    }
    let polys = si_hrc::image::geom::polygon_allocate(m, total);
    m.set_u32(new + mo::POLYGONS, polys);
    let mut w = Work {
        vert: m.si_calloc(1000, 4),
        corner: m.si_calloc(1000, 4),
        pos: m.si_calloc(1000, 12),
        norm: m.si_calloc(1000, 12),
        u: m.si_calloc(1000, 4),
        v: m.si_calloc(1000, 4),
        col: m.si_calloc(1000, 4),
        hit: m.si_calloc(1000, 2),
    };
    let mut cap_points: i32 = 1;
    let mut cap_tris: i32 = 1;
    let mut made: i32 = 0;
    // the collision shorts that fall beyond their block
    let mut hit_beyond: std::collections::BTreeMap<i32, i16> = Default::default();
    let mut p = 0;
    while p < m.i32(mesh + mo::NB_POLYGONS) {
        let poly = polygon_at(m, mesh, p);
        let cnt = m.u16(poly + mo::POLYGON_NB_NODES);
        if 2 < cnt {
            let material = m.i16(poly + mo::POLYGON_MATERIAL);
            let flag = m.u16(poly + mo::POLYGON_FLAG);
            if 0x3e6 < cnt {
                w.corner = m.si_realloc(w.corner, (cnt as u32) * 4 + 4);
            }
            let node = |m: &Mem, k: u32| m.u32(poly + mo::POLYGON_NODES).wrapping_add(mo::NODE * k);
            if cnt == 3 {
                let (a, b, c) = (m.i32(node(m, 0)), m.i32(node(m, 1)), m.i32(node(m, 2)));
                let n = m.i32(mesh + mo::NB_VERTICES);
                if 0 <= a && a < n && 0 <= b && b < n && 0 <= c && c < n && a != b && a != c {
                    let mut collide = false;
                    for (from, to) in [(a, b), (b, c), (c, a)] {
                        if star_has(m, new, from, to, |_| true) {
                            collide = true;
                            break;
                        }
                    }
                    if !collide {
                        let dst = polys + mo::POLYGON * made as u32;
                        polygon_initialize(m, dst, &[a, b, c]);
                        let dn = m.u32(dst + mo::POLYGON_NODES);
                        for k in 0..3u32 {
                            let (to, from) = (dn + mo::NODE * k, node(m, k));
                            for off in [0x08, 0x0c, 0x10] {
                                m.set_u32(to + off, m.u32(from + off));
                            }
                        }
                        for off in [0x14, 0x18, 0x1c] {
                            for k in 0..3u32 {
                                let (to, from) = (dn + mo::NODE * k, node(m, k));
                                m.set_u32(to + off, m.u32(from + off));
                            }
                        }
                        m.set_i16(dst + mo::POLYGON_MATERIAL, material);
                        if flag & 1 != 0 {
                            m.or_u8(dst + mo::POLYGON_FLAG, 1);
                        }
                        made += 1;
                        let idx = m.i32(new + mo::NB_POLYGONS);
                        m.set_i32(new + mo::NB_POLYGONS, idx + 1);
                        insert_ipolygon(m, o, new, idx);
                    }
                }
            } else {
                let z = si_math::algebra::matrix_z_align(&si_math::types::Vector::from_array(f3(m, poly)));
                let mut retried = false;
                let mut limit: i32 = 0;
                let mut tries: i32 = 0;
                let mut tri_mark = cap_tris * 1000 - 1;
                let count_at = m.local(8);
                let tris_at = count_at + 4;
                m.set_u32(tris_at, 0);
                loop {
                    let mut collide = false;
                    let mut npts: i32 = 0;
                    let cnt_now = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
                    if cnt_now != 0 {
                        let mut bytes12 = (cap_points as u32).wrapping_mul(12000);
                        let mut bytes4 = (cap_points as u32).wrapping_mul(4000);
                        let mut mark = cap_points * 1000 - 1;
                        let mut c = 0;
                        while c < m.u16(poly + mo::POLYGON_NB_NODES) as i32 {
                            let vx = m.i32(node(m, c as u32));
                            if vx == -1 {
                                m.set_i32(w.corner + 4 * c as u32, -1);
                            } else {
                                let mut j = 0;
                                while j < npts && m.i32(w.vert + 4 * j as u32) != vx {
                                    j += 1;
                                }
                                if j == npts {
                                    if mark <= j {
                                        bytes4 += 4000;
                                        bytes12 += 12000;
                                        mark += 1000;
                                        cap_points += 1;
                                        w.vert = m.si_realloc(w.vert, bytes4);
                                        w.pos = m.si_realloc(w.pos, bytes12);
                                        w.norm = m.si_realloc(w.norm, bytes12);
                                        w.u = m.si_realloc(w.u, bytes4);
                                        w.v = m.si_realloc(w.v, bytes4);
                                        w.col = m.si_realloc(w.col, bytes4);
                                    }
                                    let src = vertex_at(m, mesh, vx);
                                    let j12 = 12 * j as u32;
                                    for off in [0, 4, 8] {
                                        m.set_u32(w.pos + j12 + off, m.u32(src + off));
                                    }
                                    let nd = node(m, c as u32);
                                    for off in [0, 4, 8] {
                                        m.set_u32(w.norm + j12 + off, m.u32(nd + 8 + off));
                                    }
                                    m.set_u32(w.u + 4 * j as u32, m.u32(nd + 0x14));
                                    m.set_u32(w.v + 4 * j as u32, m.u32(nd + 0x18));
                                    m.set_u32(w.col + 4 * j as u32, m.u32(nd + 0x1c));
                                    m.set_i32(w.vert + 4 * j as u32, vx);
                                    npts = j + 1;
                                }
                                m.set_i32(w.corner + 4 * c as u32, j);
                            }
                            c += 1;
                        }
                    }
                    for k in 0..npts.max(0) as u32 {
                        let at = w.pos + 12 * k;
                        let [px, py, pz] = f3(m, at);
                        let col = |j: usize| {
                            use si_x87::x;
                            (((x(py) * z[1][j] + x(px) * z[0][j]) + x(pz) * z[2][j]) + z[3][j]).to_f32()
                        };
                        super::put_f3(m, at, [col(0), col(1), col(2)]);
                    }
                    let cnt_now = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
                    let ok = triangulate_contour(m, npts, w.pos, cnt_now, w.corner, count_at, tris_at);
                    if ok == 0 {
                        result = 0;
                    } else {
                        let ntri = m.i32(count_at);
                        if tri_mark <= ntri {
                            cap_tris += 1;
                            tri_mark += 1000;
                            w.hit = m.si_realloc(w.hit, (cap_tris as u32).wrapping_mul(2000));
                        }
                        let room = m.block_size(w.hit).unwrap_or(0) as i32 / 2;
                        hit_beyond.clear();
                        let tris = m.u32(tris_at);
                        for t in 0..ntri {
                            set_hit(m, w.hit, room, &mut hit_beyond, t, 0);
                            let tri = |m: &Mem, k: u32| m.i32(w.vert + 4 * m.i32(tris + 12 * t as u32 + 4 * k) as u32);
                            let (a, b, c) = (tri(m, 0), tri(m, 1), tri(m, 2));
                            for (from, to) in [(a, b), (b, c), (c, a)] {
                                if star_has(m, mesh, from, to, |q| q != p) {
                                    set_hit(m, w.hit, room, &mut hit_beyond, t, 1);
                                    collide = true;
                                }
                                let base = m.i32(new + mo::NB_POLYGONS);
                                if get_hit(m, w.hit, room, &hit_beyond, t) == 0 && star_has(m, new, from, to, |q| q != t + base) {
                                    set_hit(m, w.hit, room, &mut hit_beyond, t, 1);
                                    collide = true;
                                }
                            }
                        }
                        if !collide || (retried && tries == limit - 1) {
                            break;
                        }
                        tries += 1;
                        let mut u: i32 = 1;
                        loop {
                            let cnt_now = m.u16(poly + mo::POLYGON_NB_NODES) as i32;
                            if cnt_now < u || cnt_now == 0 {
                                break;
                            }
                            if u == cnt_now || m.i32(node(m, u as u32)) == -1 {
                                if !retried && limit < u {
                                    limit = u;
                                }
                                let first: Vec<u32> = (0..9).map(|k| m.u32(node(m, 0) + 4 * k)).collect();
                                for r in 0..(u - 1).max(0) as u32 {
                                    let (to, from) = (node(m, r), node(m, r + 1));
                                    for k in 0..9 {
                                        m.set_u32(to + 4 * k, m.u32(from + 4 * k));
                                    }
                                }
                                let last = node(m, (u - 1) as u32);
                                for (k, word) in first.iter().enumerate() {
                                    m.set_u32(last + 4 * k as u32, *word);
                                }
                            }
                            u += 1;
                        }
                        retried = true;
                        let t = m.u32(tris_at);
                        if t != 0 {
                            m.free(t);
                            m.set_u32(tris_at, 0);
                        }
                        continue;
                    }
                    if !collide {
                        break;
                    }
                }
                let ntri = m.i32(count_at);
                let room = m.block_size(w.hit).unwrap_or(0) as i32 / 2;
                let tris = m.u32(tris_at);
                for t in 0..ntri {
                    if get_hit(m, w.hit, room, &hit_beyond, t) == 1 {
                        continue;
                    }
                    let num = |m: &Mem, k: u32| m.i32(tris + 12 * t as u32 + 4 * k);
                    let (ia, ib, ic) = (num(m, 0), num(m, 1), num(m, 2));
                    let vert = |m: &Mem, i: i32| m.i32(w.vert + 4 * i as u32);
                    let dst = polys + mo::POLYGON * made as u32;
                    let corners = [vert(m, ia), vert(m, ib), vert(m, ic)];
                    polygon_initialize(m, dst, &corners);
                    let dn = m.u32(dst + mo::POLYGON_NODES);
                    for (k, i) in [ia, ib, ic].into_iter().enumerate() {
                        let to = dn + mo::NODE * k as u32;
                        for off in [0, 4, 8] {
                            m.set_u32(to + 8 + off, m.u32(w.norm + 12 * i as u32 + off));
                        }
                    }
                    for (arr, off) in [(w.u, 0x14), (w.v, 0x18), (w.col, 0x1c)] {
                        for (k, i) in [ia, ib, ic].into_iter().enumerate() {
                            m.set_u32(dn + mo::NODE * k as u32 + off, m.u32(arr + 4 * i as u32));
                        }
                    }
                    m.set_i16(dst + mo::POLYGON_MATERIAL, material);
                    if flag & 1 != 0 {
                        m.or_u8(dst + mo::POLYGON_FLAG, 1);
                    }
                    made += 1;
                    let idx = m.i32(new + mo::NB_POLYGONS);
                    m.set_i32(new + mo::NB_POLYGONS, idx + 1);
                    insert_ipolygon(m, o, new, idx);
                }
                let t = m.u32(tris_at);
                if t != 0 {
                    m.free(t);
                    m.set_u32(tris_at, 0);
                }
                m.release(8);
            }
        }
        p += 1;
    }
    // the old mesh's stars, edges, corners and polygons go; the vertex array is lost
    let mut v = 0;
    while v < m.i32(mesh + mo::NB_VERTICES) {
        let vr = vertex_at(m, mesh, v);
        let s = m.u32(vr + mo::VERTEX_STAR);
        if s != 0 {
            m.free(s);
            m.set_u32(vr + mo::VERTEX_STAR, 0);
        }
        m.set_u32(vr + mo::VERTEX_STAR, 0);
        m.set_u16(vr + mo::VERTEX_NB_STAR, 0);
        v += 1;
    }
    super::obj::mesh_dispose_edges(m, mesh);
    let mut p = 0;
    while p < m.i32(mesh + mo::NB_POLYGONS) {
        let poly = polygon_at(m, mesh, p);
        let nodes = m.u32(poly + mo::POLYGON_NODES);
        if nodes != 0 {
            m.free(nodes);
            m.set_u32(poly + mo::POLYGON_NODES, 0);
        }
        p += 1;
    }
    let a = m.u32(mesh + mo::POLYGONS);
    if a != 0 {
        m.free(a);
        m.set_u32(mesh + mo::POLYGONS, 0);
    }
    m.set_u32(mesh + mo::VERTICES, m.u32(new + mo::VERTICES));
    m.set_u32(new + mo::VERTICES, 0);
    m.set_i32(mesh + mo::NB_EDGES, m.i32(new + mo::NB_EDGES));
    m.set_u32(mesh + mo::EDGES, m.u32(new + mo::EDGES));
    m.set_u32(new + mo::EDGES, 0);
    m.set_i32(mesh + mo::NB_POLYGONS, made);
    m.set_u32(mesh + mo::POLYGONS, m.u32(new + mo::POLYGONS));
    m.set_u32(new + mo::POLYGONS, 0);
    m.free(new);
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    computes_polygon_normal(m, mesh, -1);
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    compute_hard_edge(m, mesh, -1);
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    optimize_display(m, o, mesh);
    super::lists::update_polygon_lists(m, o, mesh);
    let mut e = 1;
    while e <= m.i32(mesh + mo::NB_EDGES) {
        let r = edge_record(m, mesh, e);
        m.or_u8(r + mo::EDGE_FLAG, 4);
        e += 1;
    }
    for k in 0..old_edges.max(0) as u32 {
        let vr = vertex_at(m, mesh, m.i32(e0 + 4 * k));
        let other = m.i32(e1 + 4 * k);
        let mut u = 0u32;
        while u < m.u16(vr + mo::VERTEX_NB_STAR) as u32 {
            let at = m.u32(vr + mo::VERTEX_STAR) + 12 * u;
            let e = m.i32(at + 4);
            let r = edge_record(m, mesh, e.wrapping_abs());
            if m.i32(at) == other {
                m.set_i16(r + mo::EDGE_FLAG, m.i32(ef + 4 * k) as i16);
                break;
            }
            u += 1;
        }
    }
    for b in [e0, e1, ef, w.vert, w.corner, w.pos, w.norm, w.u, w.v, w.col, w.hit] {
        if b != 0 {
            m.free(b);
        }
    }
    result
}

/// Whether vertex `from` of `mesh` has a star entry with neighbour `to` and a polygon
/// (other than −1) that `keep` accepts; the scan stops at the first such entry.
fn star_has(m: &Mem, mesh: u32, from: i32, to: i32, keep: impl Fn(i32) -> bool) -> bool {
    let vr = vertex_at(m, mesh, from);
    let (s, n) = (m.u32(vr + mo::VERTEX_STAR), m.u16(vr + mo::VERTEX_NB_STAR) as u32);
    (0..n).any(|u| {
        let at = s + 12 * u;
        let q = m.i32(at + 8);
        m.i32(at) == to && q != -1 && keep(q)
    })
}

fn set_hit(m: &mut Mem, block: u32, room: i32, beyond: &mut std::collections::BTreeMap<i32, i16>, t: i32, v: i16) {
    if t < room {
        m.set_i16(block + 2 * t as u32, v);
    } else {
        beyond.insert(t, v);
    }
}

fn get_hit(m: &Mem, block: u32, room: i32, beyond: &std::collections::BTreeMap<i32, i16>, t: i32) -> i16 {
    if t < room {
        m.i16(block + 2 * t as u32)
    } else {
        beyond.get(&t).copied().unwrap_or(0)
    }
}
