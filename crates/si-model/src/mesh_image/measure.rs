// SPDX-License-Identifier: MIT
//! The measuring and moving tools of `obj_global.c` on the image (finding 0015, 2026-10-28
//! pass): the centroid, the volume and the surface of a mesh, moving vertices. Float code
//! of the x87 unit: sums stay in the register, the stores to the compiler's temporaries
//! round to `f32`, and a few operands are read back from those stores.

use si_mem::Mem;
use si_x87::{x, F80};

use super::off as mo;

/// A vertex position as the three `f32` of its record.
fn pos(m: &Mem, mesh: u32, v: u32) -> [f32; 3] {
    let a = m.u32(mesh + mo::VERTICES).wrapping_add(v.wrapping_mul(0x14));
    [m.f32(a), m.f32(a + 4), m.f32(a + 8)]
}

/// The area-weighted centroid of the triangle fan (A, B, C): the weight is the triangle's
/// normal dotted with A; the sums of the corners are rounded to `f32` when stored.
fn fan_weight(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> (F80, [f32; 3]) {
    let d1 = [x(b[0]) - x(a[0]), x(b[1]) - x(a[1]), x(b[2]) - x(a[2])];
    let e = [x(c[0]) - x(a[0]), x(c[1]) - x(a[1]), x(c[2]) - x(a[2])];
    let nx = d1[1] * e[2] - d1[2] * e[1];
    let ny = d1[2] * e[0] - d1[0] * e[2];
    let nz = d1[0] * e[1] - d1[1] * e[0];
    let d = nz * x(a[2]) + (nx * x(a[0]) + ny * x(a[1]));
    let s = [
        ((x(a[0]) + x(b[0])) + x(c[0])).to_f32(),
        ((x(a[1]) + x(b[1])) + x(c[1])).to_f32(),
        ((x(a[2]) + x(b[2])) + x(c[2])).to_f32(),
    ];
    (d, s)
}

/// [soft 0x0844490c SI_meshCentroid]: the centroid of the mesh into `out` (three `f32`):
/// every contour of every polygon is walked as a fan of triangles whose first two corners
/// are the contour's last two; each adds its weight times the sum of its corners; the
/// total is divided out (0.25 / total) when its magnitude is above 0.0001.
pub fn mesh_centroid(m: &mut Mem, mesh: u32, out: u32) {
    let mut total = 0.0f32;
    for k in 0..3 {
        m.set_f32(out + 4 * k, 0.0);
    }
    let mut poly = m.u32(mesh + mo::POLYGONS);
    let np = m.i32(mesh + mo::NB_POLYGONS);
    let mut n = 0;
    while n < np {
        n += 1;
        let corners = m.u32(poly + 0xc);
        let mut count = m.u16(poly + 0x10) as i32;
        let corner_v = |m: &Mem, k: i32| m.u32(corners.wrapping_add((k as u32).wrapping_mul(0x24)));
        let mut start = 0i32;
        let mut i = 0i32;
        while i <= count {
            if i == count || corner_v(m, i) == 0xffff_ffff {
                let last = i - 1;
                let mut a = pos(m, mesh, corner_v(m, last - 1));
                let mut b = pos(m, mesh, corner_v(m, last));
                let mut j = start;
                while j <= last {
                    let c = pos(m, mesh, corner_v(m, j));
                    let (d, s) = fan_weight(a, b, c);
                    let acc = [d * x(s[0]) + x(m.f32(out)), d * x(s[1]) + x(m.f32(out + 4)), d * x(s[2]) + x(m.f32(out + 8))];
                    for (k, v) in acc.iter().enumerate() {
                        m.set_f32(out + 4 * k as u32, v.to_f32());
                    }
                    total = (d + x(total)).to_f32();
                    a = b;
                    b = c;
                    j += 1;
                }
                count = m.u16(poly + 0x10) as i32;
                start = i + 1;
            }
            i += 1;
        }
        poly = poly.wrapping_add(0x18);
    }
    if x(total).abs().gt(x(0.0001f64)) {
        let s = (x(0.25f64) / x(total)).to_f32();
        for k in 0..3 {
            m.set_f32(out + 4 * k, (x(s) * x(m.f32(out + 4 * k))).to_f32());
        }
    }
}

/// [soft 0x08444df8 SI_meshPolygonVolumeSurface]: the signed volume (`volume`, summed
/// over the polygon's triangle fan from its first corner, each triangle's cross product
/// dotted with the first corner) and the surface (`surface`, the cross product's length,
/// added when it faces the polygon's normal and taken away otherwise) of polygon `n`;
/// with `scale` not 0 the volume is divided by 6 and the surface halved.
pub fn mesh_polygon_volume_surface(m: &mut Mem, mesh: u32, n: i32, scale: i16, volume: u32, surface: u32) {
    m.set_f32(volume, 0.0);
    m.set_f32(surface, 0.0);
    let poly = m.u32(mesh + mo::POLYGONS).wrapping_add((n as u32).wrapping_mul(0x18));
    let mut start = 0i32;
    let mut i = 0i32;
    while i <= m.u16(poly + 0x10) as i32 {
        let corners = m.u32(poly + 0xc);
        let corner_v = |m: &Mem, k: i32| m.u32(m.u32(poly + 0xc).wrapping_add((k as u32).wrapping_mul(0x24)));
        let _ = corners;
        if i == m.u16(poly + 0x10) as i32 || corner_v(m, i) == 0xffff_ffff {
            let a = pos(m, mesh, corner_v(m, start));
            let mut b = pos(m, mesh, corner_v(m, start + 1));
            let first = start + 1;
            start = i + 1;
            if first < i - 1 {
                for t in 0..(i - 1 - first) {
                    let c = pos(m, mesh, corner_v(m, first + t + 1));
                    let f = [x(a[0]) - x(b[0]), x(a[1]) - x(b[1])];
                    let g = [x(c[0]) - x(b[0]), x(c[1]) - x(b[1]), x(c[2]) - x(b[2])];
                    let hz = x(a[2]) - x(b[2]);
                    let nx = g[1] * hz - g[2] * f[1];
                    let ny = g[2] * f[0] - g[0] * hz;
                    let nz = g[0] * f[1] - g[1] * f[0];
                    let len = (ny * ny + nx * nx + nz * nz).sqrt().to_f32();
                    let d = (ny * x(m.f32(poly + 4)) + nx * x(m.f32(poly))) + nz * x(m.f32(poly + 8));
                    let s = x(m.f32(surface));
                    let s = if d.gt(x(0.0f32)) { x(len) + s } else { s - x(len) };
                    m.set_f32(surface, s.to_f32());
                    let (rx, ry, rz) = (nx.to_f32(), ny.to_f32(), nz.to_f32());
                    let v = (x(a[0]) * x(rx) + x(a[1]) * x(ry)) + x(a[2]) * x(rz) + x(m.f32(volume));
                    m.set_f32(volume, v.to_f32());
                    b = c;
                }
            }
        }
        i += 1;
    }
    if scale != 0 {
        m.set_f32(volume, (x(m.f32(volume)) / x(6.0f64)).to_f32());
        m.set_f32(surface, (x(m.f32(surface)) * x(0.5f64)).to_f32());
    }
}

/// [soft 0x08445054 SI_meshComputesVolumeSurface]: the volume and the surface of the mesh
/// into `volume` and `surface`: the sums over its polygons (each unscaled), then divided
/// by 6 and halved.
pub fn mesh_computes_volume_surface(m: &mut Mem, mesh: u32, volume: u32, surface: u32) {
    m.set_f32(volume, 0.0);
    m.set_f32(surface, 0.0);
    let sp = m.sp();
    let pv = m.local(4);
    let ps = m.local(4);
    let mut n = 0;
    while n < m.i32(mesh + mo::NB_POLYGONS) {
        mesh_polygon_volume_surface(m, mesh, n, 0, pv, ps);
        m.set_f32(volume, (x(m.f32(volume)) + x(m.f32(pv))).to_f32());
        m.set_f32(surface, (x(m.f32(surface)) + x(m.f32(ps))).to_f32());
        n += 1;
    }
    m.set_sp(sp);
    m.set_f32(volume, (x(m.f32(volume)) / x(6.0f64)).to_f32());
    m.set_f32(surface, (x(m.f32(surface)) * x(0.5f64)).to_f32());
}

/// The edge record `e` (arithmetic shift into the block table, 0x14 bytes each).
fn edge_rec(m: &Mem, mesh: u32, e: i32) -> u32 {
    let table = m.u32(mesh + mo::EDGES);
    m.u32(table.wrapping_add(((e >> 10) as u32).wrapping_mul(4))).wrapping_add(((e & 0x3ff) as u32).wrapping_mul(0x14))
}

/// [soft 0x084462d8 SI_meshMoveVertex]: vertex `n` to the position at `to`; answers 0 for
/// an index outside the mesh. The normals of the polygons round the vertex are computed
/// again and their corners' vertices and edges marked (0x8000), then every marked edge gets
/// its discontinuity and every marked vertex its normal, the marks cleared.
pub fn mesh_move_vertex(m: &mut Mem, mesh: u32, n: i32, to: u32) -> i32 {
    use super::rebuild::{compute_hard_edge, computes_polygon_normal, computes_vertex_normal};
    if n < 0 || m.i32(mesh + mo::NB_VERTICES) <= n {
        return 0;
    }
    let v = m.u32(mesh + mo::VERTICES).wrapping_add((n as u32).wrapping_mul(0x14));
    for k in 0..3 {
        m.set_u32(v + 4 * k, m.u32(to + 4 * k));
    }
    let mut k = 0u32;
    if m.u16(v + 0x10) != 0 {
        loop {
            let next = k + 1;
            let p = m.i32(m.u32(v + 0xc).wrapping_add(8).wrapping_add(k.wrapping_mul(0xc)));
            if p != -1 {
                computes_polygon_normal(m, mesh, p);
                let poly = m.u32(mesh + mo::POLYGONS).wrapping_add((p as u32).wrapping_mul(0x18));
                if m.u16(poly + 0x10) != 0 {
                    let mut c = 0u32;
                    loop {
                        let corner = m.u32(poly + 0xc).wrapping_add(c.wrapping_mul(0x24));
                        let cv = m.i32(corner);
                        if cv != -1 {
                            let flag = m.u32(mesh + mo::VERTICES).wrapping_add((cv as u32).wrapping_mul(0x14)) + 0x12;
                            m.set_u16(flag, m.u16(flag) | 0x8000);
                            let e = m.i32(corner + 4).wrapping_abs();
                            let r = edge_rec(m, mesh, e) + 0x12;
                            m.set_u16(r, m.u16(r) | 0x8000);
                        }
                        c += 1;
                        if c as i32 >= m.u16(poly + 0x10) as i32 {
                            break;
                        }
                    }
                }
            }
            k = next;
            if k as i32 >= m.u16(v + 0x10) as i32 {
                break;
            }
        }
    }
    if m.i32(mesh + mo::NB_EDGES) > 0 {
        let mut e = 1i32;
        loop {
            let r = edge_rec(m, mesh, e) + 0x12;
            if (m.u16(r) as i16) < 0 {
                compute_hard_edge(m, mesh, e);
                m.set_u16(r, m.u16(r) & 0x7fff);
            }
            e += 1;
            if e > m.i32(mesh + mo::NB_EDGES) {
                break;
            }
        }
    }
    let mut i = 0;
    let mut rec = m.u32(mesh + mo::VERTICES);
    while i < m.i32(mesh + mo::NB_VERTICES) {
        if (m.u16(rec + 0x12) as i16) < 0 {
            computes_vertex_normal(m, mesh, i);
            m.set_u16(rec + 0x12, m.u16(rec + 0x12) & 0x7fff);
        }
        i += 1;
        rec = rec.wrapping_add(0x14);
    }
    1
}

/// [soft 0x08444bf0 SI_meshRandomize]: every vertex (the selected ones, flag 1 of +0x12,
/// when `selected` is not 0) moved `times` times by `(random − 0.5) × amount` on each axis,
/// the random numbers `rand` answers (the C library's `drand48`) taken x, y, z in turn; the
/// sums in the register, stored as `f32`. Then the normals of the polygons, the
/// discontinuity of the edges and the normals of the vertices, all of them.
pub fn mesh_randomize(m: &mut Mem, mesh: u32, amount: u32, times: i16, selected: i16, rand: &mut dyn FnMut() -> f64) {
    use super::rebuild::{compute_hard_edge, computes_polygon_normal, computes_vertex_normal};
    let half = x(0.5f64);
    let mut i = 0;
    while i < m.i32(mesh + mo::NB_VERTICES) {
        let v = m.u32(mesh + mo::VERTICES).wrapping_add((i as u32).wrapping_mul(0x14));
        i += 1;
        if selected != 0 && m.u8(v + 0x12) & 1 == 0 {
            continue;
        }
        for _ in 0..times.max(0) {
            for k in 0..3 {
                let r = rand();
                let moved = x(m.f32(v + 4 * k)) + (x(r) - half) * x(m.f32(amount + 4 * k));
                m.set_f32(v + 4 * k, moved.to_f32());
            }
        }
    }
    computes_polygon_normal(m, mesh, -1);
    compute_hard_edge(m, mesh, -1);
    computes_vertex_normal(m, mesh, -1);
}

/// [soft 0x0844462c SI_meshInverse]: the mesh turned inside out. Every vertex star is
/// reversed (the edges negated, the polygon column shifted by one place); every polygon's
/// normal and its corners' normals are negated and each contour is reversed (its edges
/// shifted by one place, its corners swapped end for end); every edge's two vertices are
/// swapped.
pub fn mesh_inverse(m: &mut Mem, mesh: u32) {
    let mut v = m.u32(mesh + mo::VERTICES);
    for _ in 0..m.i32(mesh + mo::NB_VERTICES) {
        let c = m.u16(v + 0x10) as u32;
        if c != 0 {
            let star = |m: &Mem| m.u32(v + 0xc);
            let mut carry = m.u32(((c * 0xc).wrapping_add(star(m))).wrapping_sub(4));
            for k in 0..c {
                let e = star(m).wrapping_add(4 + 0xc * k);
                m.set_u32(e, m.u32(e).wrapping_neg());
                let p = star(m).wrapping_add(8 + 0xc * k);
                let old = m.u32(p);
                m.set_u32(p, carry);
                carry = old;
            }
            let c = m.u16(v + 0x10) as u32;
            for k in 0..(c >> 1) {
                let (a, b) = (star(m).wrapping_add(0xc * k), star(m).wrapping_add(0xc * (c - 1 - k)));
                for w in 0..3 {
                    let t = m.u32(a + 4 * w);
                    m.set_u32(a + 4 * w, m.u32(b + 4 * w));
                    m.set_u32(b + 4 * w, t);
                }
            }
        }
        v = v.wrapping_add(0x14);
    }
    let mut poly = m.u32(mesh + mo::POLYGONS);
    for _ in 0..m.i32(mesh + mo::NB_POLYGONS) {
        for k in 0..3 {
            m.set_u32(poly + 4 * k, m.u32(poly + 4 * k) ^ 0x8000_0000);
        }
        let this = poly;
        poly = poly.wrapping_add(0x18);
        if m.u16(this + 0x10) == 0 {
            continue;
        }
        let corners = |m: &Mem| m.u32(this + 0xc);
        let mut start = 0i32;
        let mut i = 0i32;
        while i <= m.u16(this + 0x10) as i32 {
            let at = |m: &Mem, k: i32| corners(m).wrapping_add((k as u32).wrapping_mul(0x24));
            let ring_end = if i == m.u16(this + 0x10) as i32 {
                true
            } else {
                for w in 2..5 {
                    let a = at(m, i) + 4 * w;
                    m.set_u32(a, m.u32(a) ^ 0x8000_0000);
                }
                m.u32(at(m, i)) == 0xffff_ffff
            };
            if ring_end {
                let last = i - 1;
                let mut carry = m.u32(at(m, last) + 4);
                let mut k = start;
                while k <= last {
                    let a = at(m, k) + 4;
                    let old = m.u32(a);
                    m.set_u32(a, carry);
                    carry = old;
                    k += 1;
                }
                let mid = (start + last + 1) >> 1;
                let mut t = 0;
                while start + t < mid {
                    let (a, b) = (at(m, start + t), at(m, last - t));
                    for w in 0..9 {
                        let tmp = m.u32(a + 4 * w);
                        m.set_u32(a + 4 * w, m.u32(b + 4 * w));
                        m.set_u32(b + 4 * w, tmp);
                    }
                    t += 1;
                }
                start = i + 1;
            }
            i += 1;
        }
    }
    let mut e = 1;
    while e <= m.i32(mesh + mo::NB_EDGES) {
        let r = edge_rec(m, mesh, e);
        let t = m.u32(r);
        m.set_u32(r, m.u32(r + 4));
        m.set_u32(r + 4, t);
        e += 1;
    }
}

/// Marks the vertices and the edges of the contours of polygon record `poly` (0x8000 in
/// their flags), as `SI_meshMoveVertex` and `SI_meshTransformSelectedVertices` do.
fn mark_polygon(m: &mut Mem, mesh: u32, poly: u32) {
    if m.u16(poly + 0x10) == 0 {
        return;
    }
    let mut c = 0u32;
    loop {
        let corner = m.u32(poly + 0xc).wrapping_add(c.wrapping_mul(0x24));
        let cv = m.i32(corner);
        if cv != -1 {
            let flag = m.u32(mesh + mo::VERTICES).wrapping_add((cv as u32).wrapping_mul(0x14)) + 0x12;
            m.set_u16(flag, m.u16(flag) | 0x8000);
            let e = m.i32(corner + 4).wrapping_abs();
            let r = edge_rec(m, mesh, e) + 0x12;
            m.set_u16(r, m.u16(r) | 0x8000);
        }
        c += 1;
        if c as i32 >= m.u16(poly + 0x10) as i32 {
            break;
        }
    }
}

/// The last two stages of both tools: the marked edges get their discontinuity, the marked
/// vertices their normals, the marks cleared.
fn settle_marked(m: &mut Mem, mesh: u32) {
    use super::rebuild::{compute_hard_edge, computes_vertex_normal};
    if m.i32(mesh + mo::NB_EDGES) > 0 {
        let mut e = 1i32;
        loop {
            let r = edge_rec(m, mesh, e) + 0x12;
            if (m.u16(r) as i16) < 0 {
                compute_hard_edge(m, mesh, e);
                m.set_u16(r, m.u16(r) & 0x7fff);
            }
            e += 1;
            if e > m.i32(mesh + mo::NB_EDGES) {
                break;
            }
        }
    }
    let mut i = 0;
    let mut rec = m.u32(mesh + mo::VERTICES);
    while i < m.i32(mesh + mo::NB_VERTICES) {
        if (m.u16(rec + 0x12) as i16) < 0 {
            computes_vertex_normal(m, mesh, i);
            m.set_u16(rec + 0x12, m.u16(rec + 0x12) & 0x7fff);
        }
        i += 1;
        rec = rec.wrapping_add(0x14);
    }
}

/// [soft 0x084464c0 SI_meshTransformSelectedVertices]: the selected vertices (flag 1 of
/// +0x12) that have a visible polygon in their star are put through the matrix `matrix`
/// (16 floats, the translation in +0x30) or, when `to_point` is not 0, set to `point`; each
/// such vertex marks the polygons of its star (0x400). Marked polygons get their normal
/// again and mark their vertices and edges (0x8000); then the edges and the vertices so
/// marked are settled as `SI_meshMoveVertex` does.
pub fn mesh_transform_selected_vertices(m: &mut Mem, mesh: u32, point: u32, to_point: i16, matrix: u32) {
    use super::rebuild::computes_polygon_normal;
    let mut v = m.u32(mesh + mo::VERTICES);
    let mut i = 0;
    while i < m.i32(mesh + mo::NB_VERTICES) {
        i += 1;
        if m.u8(v + 0x12) & 1 != 0 {
            let mut visible = false;
            let mut e = m.u32(v + 0xc);
            let mut k = 0;
            while k < m.u16(v + 0x10) as i32 {
                let poly = m.i32(e + 8);
                if m.u32(e + 4) != 0 && poly != -1 && m.u8(m.u32(mesh + mo::POLYGONS).wrapping_add((poly as u32).wrapping_mul(0x18)) + 0x15) & 8 == 0 {
                    visible = true;
                    break;
                }
                e = e.wrapping_add(0xc);
                k += 1;
            }
            if visible {
                if to_point == 0 {
                    let (px, py, pz) = (x(m.f32(v)), x(m.f32(v + 4)), x(m.f32(v + 8)));
                    let f = |m: &Mem, k: u32| x(m.f32(matrix + 4 * k));
                    let nx = ((px * f(m, 0) + py * f(m, 4)) + pz * f(m, 8)) + f(m, 12);
                    let ny = ((py * f(m, 5) + px * f(m, 1)) + pz * f(m, 9)) + f(m, 13);
                    let nz = ((py * f(m, 6) + px * f(m, 2)) + pz * f(m, 10)) + f(m, 14);
                    m.set_f32(v, nx.to_f32());
                    m.set_f32(v + 4, ny.to_f32());
                    m.set_f32(v + 8, nz.to_f32());
                } else {
                    for k in 0..3 {
                        m.set_u32(v + 4 * k, m.u32(point + 4 * k));
                    }
                }
                if m.u16(v + 0x10) != 0 {
                    let mut k = 0u32;
                    loop {
                        let p = m.i32(m.u32(v + 0xc).wrapping_add(8).wrapping_add(k.wrapping_mul(0xc)));
                        if p != -1 {
                            let f = m.u32(mesh + mo::POLYGONS).wrapping_add((p as u32).wrapping_mul(0x18)) + 0x15;
                            m.set_u8(f, m.u8(f) | 4);
                        }
                        k += 1;
                        if k as i32 >= m.u16(v + 0x10) as i32 {
                            break;
                        }
                    }
                }
            }
        }
        v = v.wrapping_add(0x14);
    }
    if m.i32(mesh + mo::NB_POLYGONS) > 0 {
        let mut n = 0;
        let mut poly = m.u32(mesh + mo::POLYGONS);
        loop {
            let next = n + 1;
            if m.u8(poly + 0x15) & 4 != 0 {
                computes_polygon_normal(m, mesh, n);
                m.set_u16(poly + 0x14, m.u16(poly + 0x14) & 0xfbff);
                mark_polygon(m, mesh, poly);
            }
            n = next;
            poly = poly.wrapping_add(0x18);
            if n >= m.i32(mesh + mo::NB_POLYGONS) {
                break;
            }
        }
    }
    settle_marked(m, mesh);
}
