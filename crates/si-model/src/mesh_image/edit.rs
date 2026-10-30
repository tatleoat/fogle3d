// SPDX-License-Identifier: MIT
//! The editing tools of `obj_global.c` on the image (finding 0015, 2026-10-28 pass): removing
//! polygons and vertices from a mesh, compressing its arrays, deleting the selected polygons.
//! Everything works on the mesh record at its address (the layout of [`super::off`]) and
//! makes the original's calls to `free` and `SI_calloc`, in its order.

use si_mem::Mem;

use super::off as mo;
use crate::model_image::Outside;

/// `SI_gblIndex` (0x0878896c): the table of vertex indices the editing tools keep.
const SI_GBL_INDEX: u32 = 0x0878896c;
/// `SI_gblIndex2` (0x08788970): the same for polygons.
const SI_GBL_INDEX2: u32 = 0x08788970;
/// `SI_gblIndex3` (0x08788974): the same for edges (entry 0 counts the edges added).
const SI_GBL_INDEX3: u32 = 0x08788974;

/// The edge record `e` as the original addresses it in the block table: block `e >> 10`
/// (arithmetic), record `e & 0x3ff`, 0x14 bytes each.
fn edge_rec(m: &Mem, mesh: u32, e: i32) -> u32 {
    let table = m.u32(mesh + mo::EDGES);
    let block = m.u32(table.wrapping_add(((e >> 10) as u32).wrapping_mul(4)));
    block.wrapping_add(((e & 0x3ff) as u32).wrapping_mul(0x14))
}

/// The vertex record `v` (0x14 bytes).
fn vertex_rec(m: &Mem, mesh: u32, v: i32) -> u32 {
    m.u32(mesh + mo::VERTICES).wrapping_add((v as u32).wrapping_mul(0x14))
}

/// One 12-byte star entry copied over another.
fn copy_star(m: &mut Mem, star: u32, to: i32, from: i32) {
    let (t, f) = (star.wrapping_add((to as u32).wrapping_mul(12)), star.wrapping_add((from as u32).wrapping_mul(12)));
    for k in 0..3 {
        m.set_u32(t + 4 * k, m.u32(f + 4 * k));
    }
}

/// The edge a star entry names, seen from the polygon side `want`: for a negative edge
/// number the first polygon (+8) of the edge −n, for a positive one the second (+0xc).
fn far_polygon(m: &Mem, mesh: u32, e: u32) -> i32 {
    if (e as i32) < 1 {
        m.i32(edge_rec(m, mesh, (e as i32).wrapping_neg()) + 8)
    } else {
        m.i32(edge_rec(m, mesh, e as i32) + 0xc)
    }
}

/// [soft 0x08440800 meshRemovePolygonAtVertex]: at vertex `vertex`, takes the star entry
/// of the polygon `polygon` whose neighbour is `next` out of the edge's side, and frees
/// what the removal empties: the star entries that no longer have a polygon on either side,
/// the edge record (the last edge moves into its place and every reference to it is
/// renumbered), and splits the star when it is no longer one fan. `track` = 1 keeps
/// `SI_gblIndex3` current. Answers 1; 0 when `vertex` is not a vertex, has no star, or the
/// two entries are not found.
pub fn mesh_remove_polygon_at_vertex(m: &mut Mem, mesh: u32, vertex: i32, polygon: i32, prev: i32, next: i32, track: i16) -> i32 {
    let mut keep = true;
    if !(0 <= vertex && vertex < m.i32(mesh + mo::NB_VERTICES)) {
        return 0;
    }
    let v = vertex_rec(m, mesh, vertex);
    if m.u16(v + 0x10) == 0 {
        return 0;
    }
    let (mut l24, mut l20) = (-1i32, -1i32);
    let n0 = m.u16(v + 0x10) as i32;
    let mut i = 0;
    while i < n0 {
        let e = m.u32(v + 0xc).wrapping_add((i as u32).wrapping_mul(12));
        if m.i32(e) == next && m.i32(e + 8) == polygon {
            l24 = i;
            if l20 != -1 {
                break;
            }
        }
        if m.i32(e) == prev {
            l20 = i;
            if l24 != -1 {
                break;
            }
        }
        i += 1;
    }
    if l20 == -1 || l24 == -1 {
        return 0;
    }
    let star = |m: &Mem, k: i32| m.u32(v + 0xc).wrapping_add((k as u32).wrapping_mul(12));
    m.set_i32(star(m, l24) + 8, -1);
    let mut l1c = m.i32(star(m, l24) + 4);
    if l1c < 0 {
        l1c = l1c.wrapping_neg();
    }
    let rec = edge_rec(m, mesh, l1c);
    let g3 = m.u32(SI_GBL_INDEX3);
    if (m.i32(rec + 8) == -1 || m.i32(rec + 0xc) == -1) && g3 != 0 && track == 1 {
        if m.i32(g3.wrapping_add((l1c as u32).wrapping_mul(4))) == -1 {
            let mut k = 1i32;
            loop {
                let top = m.i32(g3).wrapping_add(m.i32(mesh + mo::NB_EDGES));
                if k > top {
                    break;
                }
                let at = g3.wrapping_add((k as u32).wrapping_mul(4));
                if m.i32(at) == l1c {
                    m.set_i32(at, -1);
                }
                k += 1;
            }
        } else {
            m.set_i32(g3.wrapping_add((l1c as u32).wrapping_mul(4)), -1);
        }
    }
    l1c = m.i32(star(m, l24) + 4);
    if l1c < 0 {
        l1c = l1c.wrapping_neg();
        let r = edge_rec(m, mesh, l1c);
        m.set_i32(r + 0xc, -1);
    } else {
        let r = edge_rec(m, mesh, l1c);
        m.set_i32(r + 8, -1);
    }
    if m.i32(star(m, l20) + 8) == -1 {
        let mut k = l20;
        while k < m.u16(v + 0x10) as i32 - 1 {
            let s = m.u32(v + 0xc);
            copy_star(m, s, k, k + 1);
            k += 1;
        }
        star_remove_tail(m, v);
        if l20 < l24 {
            l24 -= 1;
        }
        keep = false;
    }
    let r = edge_rec(m, mesh, l1c);
    if m.i32(r + 8) == -1 && m.i32(r + 0xc) == -1 && m.u16(v + 0x10) != 0 {
        let mut k = l24;
        while k < m.u16(v + 0x10) as i32 - 1 {
            let s = m.u32(v + 0xc);
            copy_star(m, s, k, k + 1);
            k += 1;
        }
        star_remove_tail(m, v);
        keep = false;
    }
    if keep && l24 < l20 {
        // the fan is cut in two: the entries are put in the order that makes it one again
        let mut l30 = l20;
        let count = m.u16(v + 0x10) as u32;
        if l20 as u32 != count.wrapping_sub(1) {
            loop {
                let s = star(m, l30);
                let far = far_polygon(m, mesh, m.u32(s + 12 + 4));
                if far == -1 || far != m.i32(s + 8) {
                    break;
                }
                l30 += 1;
                if l30 as u32 == count.wrapping_sub(1) {
                    break;
                }
            }
        }
        let mut l2c = l24;
        let mut far;
        loop {
            let s = star(m, l2c);
            far = far_polygon(m, mesh, m.u32(s + 4));
            if l2c == 0 || far == -1 {
                break;
            }
            // the previous entry's polygon (offset −4 + 8 of the entry before)
            if far != m.i32(s.wrapping_sub(4)) {
                break;
            }
            l2c -= 1;
        }
        if m.i32(star(m, l30) + 8) == far {
            let new = m.si_calloc(count.wrapping_sub((count.wrapping_sub(1) & 3).wrapping_sub(3)), 12);
            let mut n = 0i32;
            let put = |m: &mut Mem, from: i32, n: &mut i32| {
                let s = m.u32(v + 0xc).wrapping_add((from as u32).wrapping_mul(12));
                let d = new.wrapping_add((*n as u32).wrapping_mul(12));
                for k in 0..3 {
                    m.set_u32(d + 4 * k, m.u32(s + 4 * k));
                }
                *n += 1;
            };
            let mut k = 0;
            while k < l2c {
                put(m, k, &mut n);
                k += 1;
            }
            let mut k = l20;
            while k <= l30 {
                put(m, k, &mut n);
                k += 1;
            }
            let mut k = l2c;
            while k <= l24 {
                put(m, k, &mut n);
                k += 1;
            }
            let mut k = l30 + 1;
            while k < m.u16(v + 0x10) as i32 {
                put(m, k, &mut n);
                k += 1;
            }
            let old = m.u32(v + 0xc);
            if old != 0 {
                m.free(old);
                m.set_u32(v + 0xc, 0);
            }
            m.set_u32(v + 0xc, new);
        }
    }
    // an edge with no polygon left: the last edge takes its place
    let r = edge_rec(m, mesh, l1c);
    if m.i32(r + 8) == -1 && m.i32(r + 0xc) == -1 {
        let last = m.i32(mesh + mo::NB_EDGES);
        let src = edge_rec(m, mesh, last);
        for k in 0..5 {
            m.set_u32(r + 4 * k, m.u32(src + 4 * k));
        }
        let g3 = m.u32(SI_GBL_INDEX3);
        if g3 != 0 && track == 1 {
            if m.i32(g3.wrapping_add((last as u32).wrapping_mul(4))) == -1 {
                let mut k = 1i32;
                loop {
                    let top = (last as i32).wrapping_add(m.i32(g3));
                    if k > top {
                        break;
                    }
                    let at = g3.wrapping_add((k as u32).wrapping_mul(4));
                    if m.i32(at) == last {
                        m.set_i32(at, l1c);
                    }
                    k += 1;
                }
            } else {
                m.set_i32(g3.wrapping_add((last as u32).wrapping_mul(4)), l1c);
            }
        }
        let g3 = m.u32(SI_GBL_INDEX3);
        m.set_i32(mesh + mo::NB_EDGES, m.i32(mesh + mo::NB_EDGES).wrapping_sub(1));
        if g3 != 0 && track == 1 {
            m.set_i32(g3, m.i32(g3).wrapping_add(1));
        }
        if l1c != last {
            let renumber = |m: &mut Mem, base: u32, count: u32, stride: u32| {
                let mut k = 0u32;
                while k < count {
                    let blk = m.u32(base + 0xc);
                    let at = blk.wrapping_add(4).wrapping_add(k * stride);
                    let x = m.i32(at);
                    if x == last {
                        m.set_i32(at, l1c);
                    } else if x == last.wrapping_neg() {
                        m.set_i32(at, l1c.wrapping_neg());
                    }
                    k += 1;
                }
            };
            // the record the last edge was copied from is still in place
            let (v0, v1, p0, p1) = (m.i32(src), m.i32(src + 4), m.i32(src + 8), m.i32(src + 0xc));
            for vv in [v0, v1] {
                let vr = vertex_rec(m, mesh, vv);
                let c = m.u16(vr + 0x10) as u32;
                renumber(m, vr, c, 12);
            }
            for pp in [p0, p1] {
                if pp != -1 {
                    let pr = m.u32(mesh + mo::POLYGONS).wrapping_add((pp as u32).wrapping_mul(0x18));
                    let c = m.u16(pr + 0x10) as u32;
                    renumber(m, pr, c, 0x24);
                }
            }
        }
    }
    1
}

/// The tail of a star removal both branches of [`mesh_remove_polygon_at_vertex`] share: the
/// count one less, the block freed when the old count was 1, the pointer cleared.
fn star_remove_tail(m: &mut Mem, v: u32) {
    let old = m.u16(v + 0x10);
    m.set_u16(v + 0x10, old.wrapping_sub(1));
    if old as i16 == 1 {
        let b = m.u32(v + 0xc);
        if b != 0 {
            m.free(b);
            m.set_u32(v + 0xc, 0);
        }
        m.set_u32(v + 0xc, 0);
    }
}

/// [soft 0x084450d4 SI_meshTagPolygonByVertex]: bit 0x400 of every polygon's flag (+0x14)
/// cleared, then set for each polygon without bit 0x800 all of whose corners (other than
/// separators, −1) are at vertices with a bit of `mask` in their flag (+0x12). Answers 1
/// (as 16 bits) when a polygon was tagged, else 0.
pub fn mesh_tag_polygon_by_vertex(m: &mut Mem, mesh: u32, mask: i32) -> u16 {
    let mut any = 0u16;
    let nb = m.i32(mesh + mo::NB_POLYGONS);
    let mut p = m.u32(mesh + mo::POLYGONS);
    for _ in 0..nb.max(0) {
        m.set_u16(p + 0x14, m.u16(p + 0x14) & 0xfbff);
        p = p.wrapping_add(0x18);
    }
    let mut i = 0;
    let mut p = m.u32(mesh + mo::POLYGONS);
    if 0 < m.i32(mesh + mo::NB_POLYGONS) {
        loop {
            i += 1;
            if m.u16(p + 0x14) & 0x800 == 0 {
                let mut all = true;
                let mut left = m.u16(p + 0x10) as u32;
                let mut c = m.u32(p + 0xc);
                while left != 0 {
                    let v = m.i32(c);
                    if v != -1 && (mask & m.i16(vertex_rec(m, mesh, v) + 0x12) as i32) == 0 {
                        all = false;
                    }
                    c = c.wrapping_add(0x24);
                    left -= 1;
                }
                if all {
                    m.set_u16(p + 0x14, m.u16(p + 0x14) | 0x400);
                    any = 1;
                }
            }
            p = p.wrapping_add(0x18);
            if i >= m.i32(mesh + mo::NB_POLYGONS) {
                break;
            }
        }
    }
    any
}

/// [soft 0x08446150 si_meshCompressPolygonsArray]: polygon `n` dropped by moving the last
/// polygon into its place: its record cleared (the corner block freed), the last record's
/// normal, counts, flags and corner block moved in (the last's count and block cleared),
/// the vertices' references to the last polygon and the edges' sides renumbered to `n`;
/// the count one less.
pub fn mesh_compress_polygons_array(m: &mut Mem, mesh: u32, n: i32) {
    let p = m.u32(mesh + mo::POLYGONS).wrapping_add((n as u32).wrapping_mul(0x18));
    for k in 0..3 {
        m.set_u32(p + 4 * k, 0);
    }
    m.set_u16(p + 0x14, 0);
    m.set_u16(p + 0x10, 0);
    if m.u32(p + 0xc) != 0 {
        let b = m.u32(p + 0xc);
        m.free(b);
        m.set_u32(p + 0xc, 0);
    }
    let last = m.i32(mesh + mo::NB_POLYGONS).wrapping_sub(1);
    if n != last {
        let q = m.u32(mesh + mo::POLYGONS).wrapping_add((last as u32).wrapping_mul(0x18));
        for k in 0..3 {
            m.set_u32(p + 4 * k, m.u32(q + 4 * k));
        }
        m.set_u16(p + 0x14, m.u16(q + 0x14));
        m.set_u16(p + 0x10, m.u16(q + 0x10));
        m.set_u32(p + 0xc, m.u32(q + 0xc));
        m.set_u16(p + 0x12, m.u16(q + 0x12));
        m.set_u16(q + 0x10, 0);
        m.set_u32(q + 0xc, 0);
        let count = m.u16(p + 0x10) as i32;
        let mut i = 0;
        if count != 0 {
            loop {
                let v = m.i32(m.u32(p + 0xc).wrapping_add((i as u32).wrapping_mul(0x24)));
                i += 1;
                if v != -1 {
                    let vr = vertex_rec(m, mesh, v);
                    let mut k = 0;
                    while k < m.u16(vr + 0x10) as i32 {
                        let at = m.u32(vr + 0xc).wrapping_add((k as u32).wrapping_mul(12) + 8);
                        if m.i32(at) == last {
                            m.set_i32(at, n);
                        }
                        k += 1;
                    }
                }
                if i >= count {
                    break;
                }
            }
        }
        let mut i = 0;
        if m.u16(p + 0x10) != 0 {
            loop {
                let e = m.i32(m.u32(p + 0xc).wrapping_add((i as u32).wrapping_mul(0x24) + 4));
                if e >= 1 {
                    let r = edge_rec(m, mesh, e);
                    m.set_i32(r + 8, n);
                } else if e < 0 {
                    let r = edge_rec(m, mesh, e.wrapping_neg());
                    m.set_i32(r + 0xc, n);
                }
                i += 1;
                if i >= m.u16(p + 0x10) as i32 {
                    break;
                }
            }
        }
    }
    m.set_i32(mesh + mo::NB_POLYGONS, m.i32(mesh + mo::NB_POLYGONS).wrapping_sub(1));
}

/// [soft 0x08445e74 meshRemoveIPolygon]: polygon `n` taken out of the mesh: for each contour
/// (corners up to a separator or the end) every corner's neighbour pair is handed to
/// [`mesh_remove_polygon_at_vertex`] and the corner's edge number set to 0; with `compress`
/// not 0 the polygon's slot is then filled by [`mesh_compress_polygons_array`]. Answers 1; 0
/// for a polygon number out of range or when a removal answers 0.
pub fn mesh_remove_ipolygon(m: &mut Mem, mesh: u32, n: i32, compress: i16, track: i16) -> i32 {
    if n < 0 || m.i32(mesh + mo::NB_POLYGONS) <= n {
        return 0;
    }
    let poly = m.u32(mesh + mo::POLYGONS).wrapping_add((n as u32).wrapping_mul(0x18));
    let corner = |m: &Mem, k: i32| m.u32(poly + 0xc).wrapping_add((k as u32).wrapping_mul(0x24));
    let mut start = 0i32;
    let mut c = 0i32;
    let mut count = m.u16(poly + 0x10) as i32;
    loop {
        if c == count || m.i32(corner(m, c)) == -1 {
            if start <= c - 1 {
                let mut prev = m.i32(corner(m, c - 2));
                let mut cur = m.i32(corner(m, c - 1));
                loop {
                    let next = m.i32(corner(m, start));
                    if mesh_remove_polygon_at_vertex(m, mesh, cur, n, prev, next, track) as i16 == 0 {
                        return 0;
                    }
                    let cn = corner(m, start);
                    start += 1;
                    m.set_u32(cn + 4, 0);
                    prev = cur;
                    cur = next;
                    if start > c - 1 {
                        break;
                    }
                }
            }
            start = c + 1;
        }
        c += 1;
        count = m.u16(poly + 0x10) as i32;
        if c > count {
            break;
        }
    }
    if compress != 0 {
        mesh_compress_polygons_array(m, mesh, n);
    }
    1
}

/// [soft 0x08445fb0 si_meshCompressVerticesArray]: vertex `n` dropped by moving the last
/// vertex into its place: with `SI_gblIndex` installed its entry for the last vertex becomes
/// `n` and the entry for `n` −1; the old star block of `n` is freed and the last vertex's
/// position, star and flags move in (the last's count and block cleared); every star entry,
/// polygon corner and edge end naming the last vertex is renumbered to `n`; the count one
/// less.
pub fn mesh_compress_vertices_array(m: &mut Mem, mesh: u32, n: i32) {
    let last = m.i32(mesh + mo::NB_VERTICES).wrapping_sub(1);
    let g = m.u32(SI_GBL_INDEX);
    if g != 0 {
        if n == last {
            m.set_i32(g.wrapping_add((n as u32).wrapping_mul(4)), -1);
        } else {
            m.set_i32(g.wrapping_add((last as u32).wrapping_mul(4)), n);
            m.set_i32(g.wrapping_add((n as u32).wrapping_mul(4)), -1);
        }
    }
    if n != last {
        let p = vertex_rec(m, mesh, n);
        let q = vertex_rec(m, mesh, last);
        if m.u32(p + 0xc) != 0 {
            let b = m.u32(p + 0xc);
            m.free(b);
            m.set_u32(p + 0xc, 0);
        }
        for k in 0..4 {
            m.set_u32(p + 4 * k, m.u32(q + 4 * k));
        }
        m.set_u16(p + 0x10, m.u16(q + 0x10));
        m.set_u16(p + 0x12, m.u16(q + 0x12));
        m.set_u32(q + 0xc, 0);
        m.set_u16(q + 0x10, 0);
        // the vertices' stars: the vertex numbers (+0 of an entry)
        let mut v = m.u32(mesh + mo::VERTICES);
        let mut i = 0;
        if 0 < m.i32(mesh + mo::NB_VERTICES) {
            loop {
                i += 1;
                let mut k = 0;
                while k < m.u16(v + 0x10) as i32 {
                    let at = m.u32(v + 0xc).wrapping_add((k as u32).wrapping_mul(12));
                    if m.i32(at) == last {
                        m.set_i32(at, n);
                    }
                    k += 1;
                }
                v = v.wrapping_add(0x14);
                if i >= m.i32(mesh + mo::NB_VERTICES) {
                    break;
                }
            }
        }
        // the polygons' corners
        let mut pg = m.u32(mesh + mo::POLYGONS);
        let mut i = 0;
        if 0 < m.i32(mesh + mo::NB_POLYGONS) {
            loop {
                i += 1;
                let mut k = 0;
                while k < m.u16(pg + 0x10) as i32 {
                    let at = m.u32(pg + 0xc).wrapping_add((k as u32).wrapping_mul(0x24));
                    if m.i32(at) == last {
                        m.set_i32(at, n);
                    }
                    k += 1;
                }
                pg = pg.wrapping_add(0x18);
                if i >= m.i32(mesh + mo::NB_POLYGONS) {
                    break;
                }
            }
        }
        let mut e = 1i32;
        if 0 < m.i32(mesh + mo::NB_EDGES) {
            loop {
                let r = edge_rec(m, mesh, e);
                if m.i32(r) == last {
                    m.set_i32(r, n);
                }
                if m.i32(r + 4) == last {
                    m.set_i32(r + 4, n);
                }
                e += 1;
                if e > m.i32(mesh + mo::NB_EDGES) {
                    break;
                }
            }
        }
    }
    m.set_i32(mesh + mo::NB_VERTICES, m.i32(mesh + mo::NB_VERTICES).wrapping_sub(1));
}

/// [soft 0x08446e48 meshRemoveUnusedVertices]: the vertices without a star entry (count 0)
/// dropped and the rest packed in order (a moved vertex's position, flags and star moved,
/// the old slot's count and block cleared; an empty vertex's block freed), `SI_gblIndex`
/// (when installed) given each old vertex's new number or −1, and every vertex number of the
/// polygons' corners and the edges' ends renumbered through a table of `SI_calloc(nbv, 4)`
/// words, freed at the end. The count is the number kept.
pub fn mesh_remove_unused_vertices(m: &mut Mem, mesh: u32) {
    let table = m.si_calloc(m.u32(mesh + mo::NB_VERTICES), 4);
    let mut kept = 0i32;
    let mut i = 0i32;
    let mut cur = m.u32(mesh + mo::VERTICES);
    let mut dst = cur;
    if 0 < m.i32(mesh + mo::NB_VERTICES) {
        loop {
            let g = m.u32(SI_GBL_INDEX);
            if m.u16(cur + 0x10) == 0 {
                if g != 0 {
                    m.set_i32(g.wrapping_add((i as u32).wrapping_mul(4)), -1);
                }
                if m.u32(cur + 0xc) != 0 {
                    let b = m.u32(cur + 0xc);
                    m.free(b);
                    m.set_u32(cur + 0xc, 0);
                }
            } else {
                if g != 0 {
                    m.set_i32(g.wrapping_add((i as u32).wrapping_mul(4)), kept);
                }
                m.set_i32(table.wrapping_add((i as u32).wrapping_mul(4)), kept);
                kept += 1;
                if cur != dst {
                    for k in 0..3 {
                        m.set_u32(dst + 4 * k, m.u32(cur + 4 * k));
                    }
                    m.set_u16(dst + 0x12, m.u16(cur + 0x12));
                    m.set_u16(dst + 0x10, m.u16(cur + 0x10));
                    m.set_u32(dst + 0xc, m.u32(cur + 0xc));
                    m.set_u16(cur + 0x10, 0);
                    m.set_u32(cur + 0xc, 0);
                }
                dst = dst.wrapping_add(0x14);
            }
            i += 1;
            cur = cur.wrapping_add(0x14);
            if i >= m.i32(mesh + mo::NB_VERTICES) {
                break;
            }
        }
    }
    let map = |m: &mut Mem, at: u32| {
        let v = m.i32(at);
        if v != -1 {
            m.set_i32(at, m.i32(table.wrapping_add((v as u32).wrapping_mul(4))));
        }
    };
    let mut i = 0;
    let mut v = m.u32(mesh + mo::VERTICES);
    if 0 < m.i32(mesh + mo::NB_VERTICES) {
        loop {
            i += 1;
            let mut k = 0;
            while k < m.u16(v + 0x10) as i32 {
                map(m, m.u32(v + 0xc).wrapping_add((k as u32).wrapping_mul(12)));
                k += 1;
            }
            v = v.wrapping_add(0x14);
            if i >= m.i32(mesh + mo::NB_VERTICES) {
                break;
            }
        }
    }
    let mut i = 0;
    let mut pg = m.u32(mesh + mo::POLYGONS);
    if 0 < m.i32(mesh + mo::NB_POLYGONS) {
        loop {
            i += 1;
            let mut k = 0;
            while k < m.u16(pg + 0x10) as i32 {
                map(m, m.u32(pg + 0xc).wrapping_add((k as u32).wrapping_mul(0x24)));
                k += 1;
            }
            pg = pg.wrapping_add(0x18);
            if i >= m.i32(mesh + mo::NB_POLYGONS) {
                break;
            }
        }
    }
    let mut e = 1i32;
    if 0 < m.i32(mesh + mo::NB_EDGES) {
        loop {
            let r = edge_rec(m, mesh, e);
            map(m, r);
            map(m, r + 4);
            e += 1;
            if e > m.i32(mesh + mo::NB_EDGES) {
                break;
            }
        }
    }
    m.set_i32(mesh + mo::NB_VERTICES, kept);
    if table != 0 {
        m.free(table);
    }
}

/// [soft 0x08441b1c SI_meshDeletePolygonSelected]: the selected polygons removed. With
/// `by_vertex` = 0 every polygon without bit 0x800 gets bit 0x400 (to delete) from its
/// selected bit (flag bit 0) and loses it otherwise; with it, [`mesh_tag_polygon_by_vertex`]
/// (mask 1) tags them. Nothing tagged: answers 0. Otherwise, with `SI_gblIndex2` installed
/// and `track` = 1, that table is kept in step with the packing, each polygon removed with
/// [`mesh_remove_ipolygon`] (compressing), then the unused vertices dropped, the display
/// lines and the selection lists rebuilt, hard edges recomputed. Answers 1.
pub fn mesh_delete_polygon_selected(m: &mut Mem, o: &mut dyn Outside, mesh: u32, by_vertex: i16, track: i16) -> i32 {
    let mut tagged = false;
    if by_vertex == 0 {
        let mut p = m.u32(mesh + mo::POLYGONS);
        let nb = m.i32(mesh + mo::NB_POLYGONS);
        let mut i = 0;
        if 0 < nb {
            loop {
                let f = m.u16(p + 0x14);
                if f & 0x800 == 0 {
                    if f & 1 == 0 {
                        m.set_u16(p + 0x14, f & 0xfbff);
                    } else {
                        m.set_u16(p + 0x14, f | 0x400);
                        tagged = true;
                    }
                }
                i += 1;
                p = p.wrapping_add(0x18);
                if i >= m.i32(mesh + mo::NB_POLYGONS) {
                    break;
                }
            }
        }
    } else {
        tagged = mesh_tag_polygon_by_vertex(m, mesh, 1) as i16 != 0;
    }
    if !tagged {
        return 0;
    }
    let mut n = m.i32(mesh + mo::NB_POLYGONS);
    let g2 = m.u32(SI_GBL_INDEX2);
    if g2 != 0 && track == 1 {
        let mut removed = 0i32;
        let mut p = m.u32(mesh + mo::POLYGONS);
        let mut i = 0i32;
        while i < n {
            let flag_hi = m.u8(p.wrapping_add(0x15));
            p = p.wrapping_add(0x18);
            let this = i;
            i += 1;
            if flag_hi & 4 != 0 {
                n = n.wrapping_sub(removed).wrapping_sub(1);
                if this == n {
                    m.set_i32(g2.wrapping_add((this as u32).wrapping_mul(4)), -1);
                } else {
                    if this < n {
                        m.set_i32(g2.wrapping_add((n as u32).wrapping_mul(4)), this);
                    } else {
                        let mut k = 0i32;
                        while k < this {
                            let at = g2.wrapping_add((k as u32).wrapping_mul(4));
                            let x = m.i32(at);
                            if m.i32(g2.wrapping_add((this as u32).wrapping_mul(4))) < x {
                                m.set_i32(at, x.wrapping_sub(1));
                            }
                            k += 1;
                        }
                    }
                    m.set_i32(g2.wrapping_add((this as u32).wrapping_mul(4)), -1);
                }
                removed += 1;
                n = m.i32(mesh + mo::NB_POLYGONS);
            }
        }
    }
    let mut i = 0i32;
    let mut p = m.u32(mesh + mo::POLYGONS);
    if 0 < n {
        loop {
            if m.u8(p.wrapping_add(0x15)) & 4 == 0 || mesh_remove_ipolygon(m, mesh, i, 1, track) as i16 == 0 {
                i += 1;
                p = p.wrapping_add(0x18);
            }
            if i >= m.i32(mesh + mo::NB_POLYGONS) {
                break;
            }
        }
    }
    mesh_remove_unused_vertices(m, mesh);
    super::rebuild::optimize_display(m, o, mesh);
    super::lists::update_polygon_lists(m, o, mesh);
    super::rebuild::compute_hard_edge(m, mesh, -1);
    1
}
