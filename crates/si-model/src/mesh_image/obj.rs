// SPDX-License-Identifier: MIT
//! A mesh copied and disposed of on the image (`obj_global.c`), with the corner routines of
//! `meshEditFct.c`, the list disposal of `meshPolygon.c` and the corner colours of
//! `meshUtil.c` that they call. The value forms are [`crate::obj_global`],
//! [`crate::mesh_polygon`] and [`crate::mesh_util`].

use si_mem::Mem;

use super::{edge_record, off as mo, polygon_at};
use crate::model_image::Outside;

/// [soft 0x08453d90 SI_nodesCopyUV]: the texture pair of corner `src` into corner `dst`, and
/// the further pairs (+0x20): without the two models, 14 words into a block `SI_calloc(14,
/// 4)` made when `dst` has none; with them, only when both have the same `nb_uv` (+0x424),
/// `nb_uv` − 1 pairs into a block of 2 × `nb_uv` − 2 words. Answers 0 for a null corner.
pub fn nodes_copy_uv(m: &mut Mem, model_a: u32, model_b: u32, dst: u32, src: u32) -> i32 {
    if dst == 0 || src == 0 {
        return 0;
    }
    m.set_u32(dst + 0x14, m.u32(src + 0x14));
    m.set_u32(dst + 0x18, m.u32(src + 0x18));
    if model_a == 0 || model_b == 0 {
        if m.u32(src + 0x20) != 0 {
            if m.u32(dst + 0x20) == 0 {
                let b = m.si_calloc(0xe, 4);
                m.set_u32(dst + 0x20, b);
            }
            let (from, to) = (m.u32(src + 0x20), m.u32(dst + 0x20));
            for k in 0..14 {
                m.set_u32(to + 4 * k, m.u32(from + 4 * k));
            }
        }
    } else {
        let n = m.i32(model_a + 0x424);
        if n == m.i32(model_b + 0x424) && m.u32(src + 0x20) != 0 {
            if m.u32(dst + 0x20) == 0 {
                let b = m.si_calloc((n.wrapping_mul(2).wrapping_sub(2)) as u32, 4);
                m.set_u32(dst + 0x20, b);
            }
            let (from, to) = (m.u32(src + 0x20), m.u32(dst + 0x20));
            let bytes = (n.wrapping_sub(1) as u32).wrapping_mul(8);
            m.copy(to, from, bytes);
        }
    }
    1
}

/// [soft 0x08453d44 SI_nodesCopy]: corner `src` into corner `dst` ([`nodes_copy_uv`] first,
/// then the nine words, `dst`'s own pointer at +0x20 kept). Answers 0 for a null corner.
pub fn nodes_copy(m: &mut Mem, model_a: u32, model_b: u32, dst: u32, src: u32) -> i32 {
    if dst == 0 || src == 0 {
        return 0;
    }
    nodes_copy_uv(m, model_a, model_b, dst, src);
    let keep = m.u32(dst + 0x20);
    for k in 0..9 {
        m.set_u32(dst + 4 * k, m.u32(src + 4 * k));
    }
    m.set_u32(dst + 0x20, keep);
    1
}

/// [soft 0x08453cd0 SI_nodesDispose]: with `which` = −1, the further pairs of the first
/// `count` corners of the array `*pp` and the array itself freed, `*pp` null; any other
/// `which` does nothing. Answers 1.
pub fn nodes_dispose(m: &mut Mem, pp: u32, which: i32, count: i32) -> i32 {
    if which == -1 {
        let mut left = count;
        let mut off = 0u32;
        while 0 < left {
            let at = m.u32(pp).wrapping_add(off + 0x20);
            let b = m.u32(at);
            if b != 0 {
                m.free(b);
                m.set_u32(at, 0);
            }
            off += mo::NODE;
            left -= 1;
        }
        let a = m.u32(pp);
        if a != 0 {
            m.free(a);
            m.set_u32(pp, 0);
        }
        m.set_u32(pp, 0);
    }
    1
}

/// [soft 0x08455d40 SI_meshFreeList]: every node of the list `*pp` freed after its own two
/// lists (+4, +8; recursively), following +0x4c; `*pp` null. Answers 1.
pub fn mesh_free_list(m: &mut Mem, pp: u32) -> i32 {
    let mut node = m.u32(pp);
    while node != 0 {
        let next = m.u32(node + 0x4c);
        mesh_free_list(m, node + 4);
        mesh_free_list(m, node + 8);
        m.free(node);
        node = next;
    }
    m.set_u32(pp, 0);
    1
}

/// [soft 0x08448594 si_meshDisposeEdges]: the edge blocks (one per started 1024 edges, the
/// count read again each turn) and the table freed, the count 0.
pub fn mesh_dispose_edges(m: &mut Mem, mesh: u32) {
    if m.u32(mesh + mo::EDGES) == 0 {
        return;
    }
    let mut b = 0i32;
    while b < (m.i32(mesh + mo::NB_EDGES) >> 10) + 1 {
        let slot = m.u32(mesh + mo::EDGES).wrapping_add((b as u32).wrapping_mul(4));
        let block = m.u32(slot);
        if block != 0 {
            m.free(block);
            m.set_u32(slot, 0);
        }
        b += 1;
    }
    let t = m.u32(mesh + mo::EDGES);
    if t != 0 {
        m.free(t);
        m.set_u32(mesh + mo::EDGES, 0);
    }
    m.set_i32(mesh + mo::NB_EDGES, 0);
}

/// [soft 0x08440390 si_meshDelete]: everything a mesh owns freed, the record kept: the two
/// lists ([`mesh_free_list`]), each vertex's star and the vertex array (count 0), each
/// polygon's corners ([`nodes_dispose`] with −1) and the polygon array (count 0), the edges
/// ([`mesh_dispose_edges`]), the display lines (count 0).
pub fn mesh_delete(m: &mut Mem, mesh: u32) {
    if mesh == 0 {
        return;
    }
    mesh_free_list(m, mesh + 0x30);
    mesh_free_list(m, mesh + 0x34);
    let verts = m.u32(mesh + mo::VERTICES);
    if verts != 0 {
        let mut v = verts;
        let mut left = m.i32(mesh + mo::NB_VERTICES);
        while left != 0 {
            let s = m.u32(v + mo::VERTEX_STAR);
            if s != 0 {
                m.free(s);
                m.set_u32(v + mo::VERTEX_STAR, 0);
            }
            v = v.wrapping_add(mo::VERTEX);
            left = left.wrapping_sub(1);
        }
        let a = m.u32(mesh + mo::VERTICES);
        if a != 0 {
            m.free(a);
            m.set_u32(mesh + mo::VERTICES, 0);
        }
        m.set_i32(mesh + mo::NB_VERTICES, 0);
    }
    let polys = m.u32(mesh + mo::POLYGONS);
    if polys != 0 {
        let mut p = polys;
        let mut left = m.i32(mesh + mo::NB_POLYGONS);
        while left != 0 {
            if m.u32(p + mo::POLYGON_NODES) != 0 {
                let n = m.u16(p + mo::POLYGON_NB_NODES) as i32;
                nodes_dispose(m, p + mo::POLYGON_NODES, -1, n);
            }
            p = p.wrapping_add(mo::POLYGON);
            left = left.wrapping_sub(1);
        }
        let a = m.u32(mesh + mo::POLYGONS);
        if a != 0 {
            m.free(a);
            m.set_u32(mesh + mo::POLYGONS, 0);
        }
        m.set_i32(mesh + mo::NB_POLYGONS, 0);
    }
    if m.u32(mesh + mo::EDGES) != 0 {
        mesh_dispose_edges(m, mesh);
    }
    let d = m.u32(mesh + 0x28);
    if d != 0 {
        m.free(d);
        m.set_u32(mesh + 0x28, 0);
        m.set_i32(mesh + 0x24, 0);
    }
}

/// [soft 0x084404a0 SI_meshDispose]: the mesh `*pp` deleted ([`mesh_delete`]) and freed,
/// `*pp` null.
pub fn mesh_dispose(m: &mut Mem, pp: u32) {
    if m.u32(pp) == 0 {
        return;
    }
    mesh_delete(m, m.u32(pp));
    let a = m.u32(pp);
    if a != 0 {
        m.free(a);
        m.set_u32(pp, 0);
    }
    m.set_u32(pp, 0);
}

/// [soft 0x084404e0 SI_meshCopy]: a new mesh (`SI_meshAllocate`) with the angle, its cosine
/// and the flags of `src`; the vertices (`SI_vertexAllocate`) with position, flag and star,
/// each star in a zeroed block of its count rounded up to four (`SI_calloc`); the polygons
/// (`SI_polygonAllocate`) with normal, material, flag and corners (`SI_polygonNodeAllocate`,
/// each corner by [`nodes_copy`] without models); the edges (`SI_edgeAllocate`, then edge 1
/// to the count word for word); the display lines in a zeroed block of their count rounded
/// up to 64, of which the first count integers are copied (the closing −1 is not: the count
/// is one less than the integers); `SI_meshCopyPolygonLists(src, new)`; then the shorts at
/// +0x52 and +0x50 and the six words from +0x38. Null when the allocation fails.
pub fn mesh_copy(m: &mut Mem, o: &mut dyn Outside, src: u32) -> u32 {
    let new = super::mesh_allocate(m);
    if new == 0 {
        return 0;
    }
    m.set_u32(new + mo::ANGLE, m.u32(src + mo::ANGLE));
    m.set_u32(new + mo::ANGLE_COS, m.u32(src + mo::ANGLE_COS));
    m.set_u16(new + mo::FLAGS, m.u16(src + mo::FLAGS));
    let nv = m.i32(src + mo::NB_VERTICES);
    m.set_i32(new + mo::NB_VERTICES, nv);
    let verts = si_hrc::image::geom::vertex_allocate(m, nv);
    m.set_u32(new + mo::VERTICES, verts);
    let mut left = m.i32(new + mo::NB_VERTICES);
    let (mut s, mut d) = (m.u32(src + mo::VERTICES), verts);
    while left != 0 {
        for k in 0..3 {
            m.set_u32(d + 4 * k, m.u32(s + 4 * k));
        }
        m.set_u16(d + mo::VERTEX_FLAG, m.u16(s + mo::VERTEX_FLAG));
        let n = m.u16(s + mo::VERTEX_NB_STAR) as u32;
        m.set_u16(d + mo::VERTEX_NB_STAR, n as u16);
        let star = m.si_calloc(n.wrapping_sub((n.wrapping_sub(1) & 3).wrapping_sub(3)), 12);
        m.set_u32(d + mo::VERTEX_STAR, star);
        left = left.wrapping_sub(1);
        let mut k = 0u32;
        while k < m.u16(d + mo::VERTEX_NB_STAR) as u32 {
            let (from, to) = (m.u32(s + mo::VERTEX_STAR) + 12 * k, m.u32(d + mo::VERTEX_STAR) + 12 * k);
            for w in 0..3 {
                m.set_u32(to + 4 * w, m.u32(from + 4 * w));
            }
            k += 1;
        }
        s = s.wrapping_add(mo::VERTEX);
        d = d.wrapping_add(mo::VERTEX);
    }
    let np = m.i32(src + mo::NB_POLYGONS);
    m.set_i32(new + mo::NB_POLYGONS, np);
    let polys = si_hrc::image::geom::polygon_allocate(m, np);
    m.set_u32(new + mo::POLYGONS, polys);
    let mut left = m.i32(new + mo::NB_POLYGONS);
    let (mut s, mut d) = (m.u32(src + mo::POLYGONS), polys);
    while left != 0 {
        for k in 0..3 {
            m.set_u32(d + 4 * k, m.u32(s + 4 * k));
        }
        m.set_u16(d + mo::POLYGON_MATERIAL, m.u16(s + mo::POLYGON_MATERIAL));
        m.set_u16(d + mo::POLYGON_FLAG, m.u16(s + mo::POLYGON_FLAG));
        let n = m.u16(s + mo::POLYGON_NB_NODES);
        m.set_u16(d + mo::POLYGON_NB_NODES, n);
        let nodes = si_hrc::image::geom::polygon_node_allocate(m, n as i32);
        m.set_u32(d + mo::POLYGON_NODES, nodes);
        left = left.wrapping_sub(1);
        let mut k = 0u32;
        while k < m.u16(d + mo::POLYGON_NB_NODES) as u32 {
            let (to, from) = (m.u32(d + mo::POLYGON_NODES) + mo::NODE * k, m.u32(s + mo::POLYGON_NODES) + mo::NODE * k);
            nodes_copy(m, 0, 0, to, from);
            k += 1;
        }
        s = s.wrapping_add(mo::POLYGON);
        d = d.wrapping_add(mo::POLYGON);
    }
    let ne = m.i32(src + mo::NB_EDGES);
    m.set_i32(new + mo::NB_EDGES, ne);
    let table = si_hrc::image::geom::edge_allocate(m, ne);
    m.set_u32(new + mo::EDGES, table);
    let mut e = 1i32;
    while e <= m.i32(src + mo::NB_EDGES) {
        let (to, from) = (edge_record(m, new, e), edge_record(m, src, e));
        for k in 0..4 {
            m.set_u32(to + 4 * k, m.u32(from + 4 * k));
        }
        m.set_u16(to + 0x10, m.u16(from + 0x10));
        m.set_u16(to + mo::EDGE_FLAG, m.u16(from + mo::EDGE_FLAG));
        e += 1;
    }
    let nd = m.i32(src + 0x24);
    m.set_i32(new + 0x24, nd);
    let n = nd as u32;
    let display = m.si_calloc(n.wrapping_sub((n.wrapping_sub(1) & 0x3f).wrapping_sub(0x3f)), 4);
    m.set_u32(new + 0x28, display);
    let mut k = 0i32;
    while k < m.i32(src + 0x24) {
        let w = m.u32(m.u32(src + 0x28).wrapping_add(4 * k as u32));
        m.set_u32(m.u32(new + 0x28).wrapping_add(4 * k as u32), w);
        k += 1;
    }
    super::lists::copy_polygon_lists(m, o, src, new);
    m.set_u16(new + 0x52, m.u16(src + 0x52));
    m.set_u16(new + 0x50, m.u16(src + 0x50));
    for k in 0..6 {
        m.set_u32(new + 0x38 + 4 * k, m.u32(src + 0x38 + 4 * k));
    }
    new
}

/// [soft 0x08458bc8 SI_meshInitVertexColors]: every corner's colour 0xffb2b2b2.
pub fn mesh_init_vertex_colors(m: &mut Mem, mesh: u32) {
    let mut p = 0;
    while p < m.i32(mesh + mo::NB_POLYGONS) {
        let poly = polygon_at(m, mesh, p);
        let nodes = m.u32(poly + mo::POLYGON_NODES);
        let mut k = 0u32;
        while k < m.u16(poly + mo::POLYGON_NB_NODES) as u32 {
            m.set_u32(nodes + mo::NODE * k + mo::NODE_COLOUR, 0xffb2_b2b2);
            k += 1;
        }
        p += 1;
    }
}

/// [soft 0x08441cd8 SI_meshTransform] on the image: every vertex through the matrix at
/// `matrix`; with bit 0x4 of the mesh's flags also every polygon normal and every corner
/// normal (of a corner that is not a separator) through the matrix at `normals`, a polygon
/// normal scaled to length 1 and a corner normal back to its old length when the new
/// length is above 0.0001, both negated when the matrix's upper 3 × 3 part has a negative
/// determinant. The arithmetic is [`crate::obj_global::mesh_transform`]'s (the value form,
/// emulated in the first pass), here on the image's floats.
pub fn mesh_transform(m: &mut Mem, mesh: u32, matrix: u32, normals: u32) {
    use crate::obj_global::{normal_through, through_a, NORMAL_MIN};
    use si_x87::{x, F80};
    let mm = crate::model_image::mat(m, matrix);
    let nn = crate::model_image::mat(m, normals);
    let flip = x(0.0f32).gt(si_math::algebra::matrix_determinant3(&mm));
    let verts = m.u32(mesh + mo::VERTICES);
    for v in 0..m.i32(mesh + mo::NB_VERTICES).max(0) as u32 {
        let at = verts.wrapping_add(v * mo::VERTEX);
        let p = super::f3(m, at);
        super::put_f3(m, at, [through_a(p, &mm, 0).to_f32(), through_a(p, &mm, 1).to_f32(), through_a(p, &mm, 2).to_f32()]);
    }
    if m.u16(mesh + mo::FLAGS) & 4 == 0 {
        return;
    }
    let negate = |v: [f32; 3]| [(-x(v[0])).to_f32(), (-x(v[1])).to_f32(), (-x(v[2])).to_f32()];
    let length = |t: &[F80; 3]| (t[2] * t[2] + (t[1] * t[1] + t[0] * t[0])).sqrt().to_f32();
    for p in 0..m.i32(mesh + mo::NB_POLYGONS).max(0) {
        let poly = polygon_at(m, mesh, p);
        let t = normal_through(super::f3(m, poly), &nn);
        let mut n = [t[0].to_f32(), t[1].to_f32(), t[2].to_f32()];
        let len = length(&t);
        if x(len).abs().gt(x(NORMAL_MIN)) {
            n = [(x(n[0]) / len).to_f32(), (x(n[1]) / len).to_f32(), (x(n[2]) / len).to_f32()];
        }
        if flip {
            n = negate(n);
        }
        super::put_f3(m, poly, n);
        let nodes = m.u32(poly + mo::POLYGON_NODES);
        for c in 0..m.u16(poly + mo::POLYGON_NB_NODES) as u32 {
            let cn = nodes.wrapping_add(c * mo::NODE);
            if m.i32(cn) == -1 {
                continue;
            }
            let o = super::f3(m, cn + mo::NODE_NORMAL);
            let before = ((x(o[0]) * o[0] + x(o[1]) * o[1]) + x(o[2]) * o[2]).sqrt().to_f32();
            let t = normal_through(o, &nn);
            let mut c3 = [t[0].to_f32(), t[1].to_f32(), t[2].to_f32()];
            let after = length(&t);
            if x(after).abs().gt(x(NORMAL_MIN)) {
                let ratio = x(before) / after;
                c3 = [(x(c3[0]) * ratio).to_f32(), (x(c3[1]) * ratio).to_f32(), (ratio * c3[2]).to_f32()];
            }
            if flip {
                c3 = negate(c3);
            }
            super::put_f3(m, cn + mo::NODE_NORMAL, c3);
        }
    }
}

/// [soft 0x08446d00 SI_meshTransferVertices]: the positions of `src`'s vertices into
/// `dst`'s, as many as `dst` counts (counted down from that number, so a negative count
/// would not end), then for every polygon of `src` the normals of its corners that are not
/// separators into the corners of the same places of `dst`.
pub fn mesh_transfer_vertices(m: &mut Mem, src: u32, dst: u32) {
    let (mut from, mut to) = (m.u32(src + mo::VERTICES), m.u32(dst + mo::VERTICES));
    let mut n = m.i32(dst + mo::NB_VERTICES);
    while n != 0 {
        for k in 0..3 {
            m.set_u32(to + 4 * k, m.u32(from + 4 * k));
        }
        from = from.wrapping_add(mo::VERTEX);
        to = to.wrapping_add(mo::VERTEX);
        n = n.wrapping_sub(1);
    }
    let (mut ps, mut pd) = (m.u32(src + mo::POLYGONS), m.u32(dst + mo::POLYGONS));
    let mut p = 0i32;
    if 0 < m.i32(src + mo::NB_POLYGONS) {
        loop {
            p += 1;
            let mut c = 0i32;
            if m.u16(ps + mo::POLYGON_NB_NODES) != 0 {
                loop {
                    let off = (c as u32).wrapping_mul(mo::NODE);
                    let a = m.u32(ps + mo::POLYGON_NODES).wrapping_add(off);
                    if m.i32(a) != -1 {
                        let b = m.u32(pd + mo::POLYGON_NODES).wrapping_add(off);
                        for k in 0..3 {
                            m.set_u32(b + mo::NODE_NORMAL + 4 * k, m.u32(a + mo::NODE_NORMAL + 4 * k));
                        }
                    }
                    c += 1;
                    if c >= m.u16(ps + mo::POLYGON_NB_NODES) as i32 {
                        break;
                    }
                }
            }
            ps = ps.wrapping_add(mo::POLYGON);
            pd = pd.wrapping_add(mo::POLYGON);
            if p >= m.i32(src + mo::NB_POLYGONS) {
                break;
            }
        }
    }
}
