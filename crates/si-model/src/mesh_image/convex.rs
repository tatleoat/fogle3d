// SPDX-License-Identifier: MIT
//! The convex splitter of `triangulate.c` on the image (finding 0015, 2026-10-29, package
//! "tri1"): `makeConvexContours`, which cuts an outline (with holes) into convex pieces, and
//! `makeConvexMesh`, which does it for every polygon of a mesh.

use si_mem::Mem;
use si_x87::{x, F80};

use super::rebuild::update_nodes;
use super::{f3, off as mo, put_f3, vertex_at};
use crate::model_image::Outside;
use crate::triangulate::{find_concavity_pt, merge_holes, pol_valid, take_md0};

type P = [f32; 3];

/// The tolerance of the unit-vector tests: the double 0.0001 (0x086625d8).
const EPS: f64 = 1e-4;

/// The float at the end of a score scan: FLT_MAX (0x086625e8).
const FLT_MAX: f32 = 3.4028235e38;

fn contour_prev(m: &Mem, c: u32) -> u32 {
    m.u32(c + 8)
}
fn contour_next(m: &Mem, c: u32) -> u32 {
    m.u32(c + 0xc)
}

/// Takes the contour `c` out of the list at `head` (`*head` when it is the first), frees its
/// array and itself.
fn unlink_free(m: &mut Mem, head: u32, c: u32) {
    let (prev, next) = (contour_prev(m, c), contour_next(m, c));
    if prev == 0 {
        m.set_u32(head, next);
    } else {
        m.set_u32(prev + 0xc, next);
    }
    if next != 0 {
        m.set_u32(next + 8, prev);
    }
    let arr = m.u32(c + 4);
    if arr != 0 {
        m.free(arr);
        m.set_u32(c + 4, 0);
    }
    if c != 0 {
        m.free(c);
    }
}

/// Puts the contour `c` first in the list at `head`.
fn link_first(m: &mut Mem, head: u32, c: u32) {
    let first = m.u32(head);
    m.set_u32(c + 0xc, first);
    m.set_u32(c + 8, 0);
    if first != 0 {
        m.set_u32(first + 8, c);
    }
    m.set_u32(head, c);
}

/// The bisector refinement of [`make_convex_contours`]: the part of the loop that, when the
/// forward and backward pieces overlap, walks the best piece back from its end while a score
/// keeps falling, and gives back the new length of the piece. Read from the listing: the
/// differences of the points are kept in the register (80 bits) for the first direction
/// (`prev - cur`), stored as floats for the second (`next - cur`); the unit vectors and the
/// bisector are stored as floats; the score is a register value compared with the float
/// stored last.
fn refine(pts: &[P], work: &[i32], n: i32, c: i32, idx: i32, start: i32, mut count: i32) -> i32 {
    let at = |k: i32| pts.get(work[k.rem_euclid(n) as usize] as usize).copied().unwrap_or([0.0; 3]);
    let prev = at((c - 1 + n) % n);
    let cur = at(c);
    let next = at(idx % n);
    let eps = x(EPS);
    // prev - cur: kept unrounded, stored rounded
    let t = [x(prev[0]) - cur[0], x(prev[1]) - cur[1], x(prev[2]) - cur[2]];
    // next - cur: stored rounded and reloaded
    let u = [(x(next[0]) - cur[0]).to_f32(), (x(next[1]) - cur[1]).to_f32(), (x(next[2]) - cur[2]).to_f32()];
    let len1 = (t[2] * t[2] + (t[0] * t[0] + t[1] * t[1])).sqrt().to_f32();
    let a: [f32; 3] = if x(len1).abs().gt(eps) {
        [(t[0] / x(len1)).to_f32(), (t[1] / x(len1)).to_f32(), (t[2] / x(len1)).to_f32()]
    } else {
        [t[0].to_f32(), t[1].to_f32(), t[2].to_f32()]
    };
    let len2 = ((x(u[1]) * u[1] + x(u[0]) * u[0]) + x(u[2]) * u[2]).sqrt().to_f32();
    let b: [f32; 3] = if x(len2).abs().gt(eps) {
        [(x(u[0]) / x(len2)).to_f32(), (x(u[1]) / x(len2)).to_f32(), (x(u[2]) / x(len2)).to_f32()]
    } else {
        u
    };
    let half = x(0.5f64);
    let mut bis = [
        (half * (x(a[0]) + b[0])).to_f32(),
        ((x(a[1]) + b[1]) * half).to_f32(),
        (half * (x(a[2]) + b[2])).to_f32(),
    ];
    let len3 = (x(bis[2]) * bis[2] + (x(bis[1]) * bis[1] + x(bis[0]) * bis[0])).sqrt().to_f32();
    if x(len3).abs().gt(eps) {
        bis = [(x(bis[0]) / x(len3)).to_f32(), (x(bis[1]) / x(len3)).to_f32(), (x(bis[2]) / x(len3)).to_f32()];
    }
    let (bx, by, bz) = (bis[0], bis[1], bis[2]);
    let mut prev_score = FLT_MAX;
    if 0 < count {
        let mut j = start - 1 + count;
        loop {
            let q = at(j);
            let dx = x(q[0]) - cur[0];
            let dy = x(q[1]) - cur[1];
            let dz = x(q[2]) - cur[2];
            let aa: F80 = x(bx) * dz - x(bz) * dx;
            let bb: F80 = x(by) * dz - x(bz) * dy;
            let dd: F80 = x(bx) * dy - x(by) * dx;
            let t1 = aa * x(-bx) - bb * x(by);
            let e = x(bz) * bb - dd * x(bx);
            let f = x(bz) * aa + x(by) * dd;
            let score = t1 * t1 + (e * e + f * f);
            if !score.lt(x(prev_score)) {
                count += 1;
                break;
            }
            prev_score = score.to_f32();
            j -= 1;
            count -= 1;
            if count <= 0 {
                break;
            }
        }
    }
    count
}

/// [soft 0x0851e65c makeConvexContours]: cuts the outline whose corners are the point numbers
/// at `list` (`n` words, −1 between contours; the points are 12-byte vectors at `pts`) into
/// convex pieces. Each piece is a contour record of 16 bytes (corner count, array of corner
/// numbers, previous, next) put first in the list whose head is at `out`. Answers 1, or 0 when
/// a hole cannot be joined.
///
/// 1. The holes (the −1 entries) are counted and a copy of the list is made in a block of
///    holes + n words; with holes it goes through `mergeHoles` ([`merge_holes`]). A table of 25
///    edge records of 16 bytes (from, to, piece to the left, piece to the right) is kept in
///    `SI_malloc`, grown by 25 when full.
/// 2. While corners remain: the first corner from the search position on that does not turn
///    left ([`find_concavity_pt`]) is taken; the longest convex piece ([`pol_valid`]) starting
///    at it going forward and the longest ending at it are measured (from 3 corners on); the
///    longer of them, when over 2 corners, is the best so far; when the two overlap by more
///    than the outline and both pass 3 corners, a bisector test shortens the best piece
///    ([`refine`]). When no concavity is left the best piece (or the whole rest) is cut off as
///    a contour, its edges are entered in the table, and its middle corners leave the work
///    list.
/// 3. Each table edge with a piece on both sides is tried: the two pieces joined along it, and
///    taken when the result is convex and crosses nothing ([`pol_valid`]); the joined piece
///    then stands in every table slot of the two and the two are freed.
/// 4. The table and the work list are freed.
///
/// Where the Rust differs: when a hole cannot be joined the original leaks the table, the
/// work list and `mergeHoles`' three blocks holding what its search left in them; here the
/// work list holds the corner list as it came and the blocks what they were given. The
/// points are read from the image up to the largest number in the list. Class: image.
pub fn make_convex_contours(m: &mut Mem, pts: u32, list: u32, n_in: i32, out: u32) -> i16 {
    let mut cap = 0x19i32;
    let mut table = m.si_malloc(400);
    let mut tcount = 0i32;
    let mut n = n_in;
    let mut holes = 0i32;
    for k in 0..n.max(0) {
        if m.i32(list + 4 * k as u32) == -1 {
            holes += 1;
        }
    }
    let work = m.si_malloc(((holes + n) as u32).wrapping_mul(4));
    let mut w: Vec<i32> = Vec::new();
    for k in 0..n.max(0) {
        let v = m.i32(list + 4 * k as u32);
        m.set_i32(work + 4 * k as u32, v);
        w.push(v);
    }
    let top = w.iter().copied().max().unwrap_or(-1).clamp(-1, 1 << 20);
    let pv: Vec<P> = (0..(top + 1) as u32)
        .map(|k| super::f3(m, pts + 12 * k))
        .collect();
    take_md0();
    if holes != 0 {
        let sep = w.clone();
        let holes0 = holes;
        let mut merged = sep.clone();
        let ok = merge_holes(&pv, &sep, &mut holes, &mut merged);
        let total = (holes0 + n) as u32;
        let a = m.si_malloc(total.wrapping_mul(4));
        for (k, &c) in sep.iter().enumerate() {
            m.set_i32(a + 4 * k as u32, c);
        }
        let b = m.si_malloc(total.wrapping_mul(4));
        let c = m.si_malloc((holes0 as u32 + 1).wrapping_mul(0x20));
        if !ok {
            if let Some(v) = take_md0() {
                m[super::g::MD0] = v;
            }
            return 0;
        }
        super::tri::free_merge_blocks(m, holes0, a, b, c);
        n = merged.len() as i32;
        for (k, &v) in merged.iter().enumerate() {
            m.set_i32(work + 4 * k as u32, v);
        }
        w = merged;
        w.resize((holes0 + n_in).max(n) as usize, 0);
    }
    let entry = |t: u32, i: i32| t.wrapping_add((i as u32).wrapping_mul(16));
    while n > 0 {
        let mut best_count = 0i32;
        let mut best_start = 0i32;
        let mut idx = 0i32;
        let mut flag = true;
        loop {
            let c = find_concavity_pt(&pv, &w[..n as usize], idx);
            if c < idx {
                break;
            }
            let mut fwd = 0;
            idx = c + 1;
            for k in 3..=n {
                if !pol_valid(&pv, &w[..n as usize], c, k) {
                    fwd = k - 1;
                    break;
                }
            }
            if best_count < fwd && 2 < fwd {
                flag = false;
                best_count = fwd;
                best_start = c;
            }
            let mut bwd = 0;
            for k in 3..=n {
                if !pol_valid(&pv, &w[..n as usize], (c - k + 1 + n) % n, k) {
                    bwd = k - 1;
                    break;
                }
            }
            if best_count < bwd && 2 < bwd {
                best_start = (c - bwd + 1 + n) % n;
                flag = false;
                best_count = bwd;
            }
            if n < fwd + bwd && 3 < fwd && 3 < bwd {
                best_count = refine(&pv, &w[..n as usize], n, c, idx, best_start, best_count);
            }
        }
        // the piece
        let ic = m.si_malloc(16);
        let count = if best_count == 0 { n } else { best_count };
        m.set_i32(ic, count);
        let arr = m.si_malloc((count as u32).wrapping_shl(2));
        m.set_u32(ic + 4, arr);
        for i in 0..count.max(0) {
            let v = w[((best_start + i) % n) as usize];
            m.set_i32(arr + 4 * i as u32, v);
        }
        let mut i = 0i32;
        let mut cnt = m.i32(ic);
        if 0 < cnt {
            loop {
                let arr = m.u32(ic + 4);
                let nxt = i + 1;
                let a = m.i32(arr + 4 * i as u32);
                let b = if nxt < cnt { m.i32(arr + 4 * nxt as u32) } else { m.i32(arr) };
                let mut append = true;
                if 0 < tcount {
                    let t0 = entry(table, 0);
                    if m.i32(t0) == a && m.i32(t0 + 4) == b {
                        m.set_u32(t0 + 8, ic);
                        append = false;
                    } else {
                        let mut j = 0i32;
                        loop {
                            let e = entry(table, j);
                            if m.i32(e) == b && m.i32(e + 4) == a {
                                m.set_u32(e + 12, ic);
                                append = false;
                                break;
                            }
                            j += 1;
                            if tcount <= j {
                                break;
                            }
                            let e = entry(table, j);
                            if m.i32(e) == a && m.i32(e + 4) == b {
                                m.set_u32(e + 8, ic);
                                append = false;
                                break;
                            }
                        }
                    }
                }
                if append {
                    let e = entry(table, tcount);
                    m.set_i32(e, a);
                    m.set_i32(e + 4, b);
                    m.set_u32(e + 8, ic);
                    m.set_u32(e + 12, 0);
                    tcount += 1;
                    if cap <= tcount {
                        cap += 0x19;
                        table = m.si_realloc(table, (cap as u32).wrapping_mul(16));
                    }
                }
                cnt = m.i32(ic);
                i = nxt;
                if i >= cnt {
                    break;
                }
            }
        }
        link_first(m, out, ic);
        if flag {
            break;
        }
        let mut i = (best_count - 1 + best_start) % n;
        if i < best_start {
            let mut dst = 0usize;
            while i <= best_start {
                w[dst] = w[i as usize];
                m.set_i32(work + 4 * dst as u32, w[dst]);
                dst += 1;
                i += 1;
            }
        } else {
            while i < n {
                best_start += 1;
                w[best_start as usize] = w[i as usize];
                m.set_i32(work + 4 * best_start as u32, w[best_start as usize]);
                i += 1;
            }
        }
        n = n + 2 - best_count;
    }
    // the pieces that join
    let mut e_i = 0i32;
    while e_i < tcount {
        let e = entry(table, e_i);
        let (ca, cb) = (m.u32(e + 8), m.u32(e + 12));
        if ca != 0 && cb != 0 {
            let (ea, eb) = (m.i32(e), m.i32(e + 4));
            let nc = m.si_malloc(16);
            let (na, nb) = (m.i32(ca), m.i32(cb));
            let total = nb.wrapping_add(na).wrapping_sub(2);
            m.set_i32(nc, total);
            let narr = m.si_malloc((total as u32).wrapping_mul(4));
            m.set_u32(nc + 4, narr);
            let (aarr, barr) = (m.u32(ca + 4), m.u32(cb + 4));
            let mut k = -1i32;
            let mut guard = 0;
            loop {
                k += 1;
                let v = m.i32(aarr + 4 * k as u32);
                m.set_i32(narr + 4 * k as u32, v);
                guard += 1;
                if v == ea || guard > (1 << 16) || m.fault().is_some() {
                    break;
                }
            }
            let mut ia = k + 1;
            let mut q = 0i32;
            guard = 0;
            while m.i32(barr + 4 * q as u32) != ea && guard < (1 << 16) && m.fault().is_none() {
                q += 1;
                guard += 1;
            }
            q += 1;
            if nb <= q {
                q = 0;
            }
            if m.i32(barr + 4 * q as u32) != eb {
                guard = 0;
                loop {
                    k += 1;
                    let v = m.i32(barr + 4 * q as u32);
                    m.set_i32(narr + 4 * k as u32, v);
                    q += 1;
                    if nb <= q {
                        q = 0;
                    }
                    guard += 1;
                    if m.i32(barr + 4 * q as u32) == eb || guard > (1 << 16) || m.fault().is_some() {
                        break;
                    }
                }
            }
            if ia < na {
                loop {
                    k += 1;
                    let v = m.i32(aarr + 4 * ia as u32);
                    m.set_i32(narr + 4 * k as u32, v);
                    ia += 1;
                    if ia >= na {
                        break;
                    }
                }
            }
            let arr_v: Vec<i32> = (0..total.max(0)).map(|k| m.i32(narr + 4 * k as u32)).collect();
            if !pol_valid(&pv, &arr_v, 0, total) {
                if m.u32(nc + 4) != 0 {
                    m.free(m.u32(nc + 4));
                    m.set_u32(nc + 4, 0);
                }
                m.free(nc);
            } else {
                for s in 0..tcount {
                    for f in [8u32, 12] {
                        let p = entry(table, s) + f;
                        let v = m.u32(p);
                        if v == ca || v == cb {
                            m.set_u32(p, nc);
                        }
                    }
                }
                unlink_free(m, out, ca);
                unlink_free(m, out, cb);
                link_first(m, out, nc);
            }
        }
        e_i += 1;
    }
    if table != 0 {
        m.free(table);
    }
    if work != 0 {
        m.free(work);
    }
    if let Some(v) = take_md0() {
        m[super::g::MD0] = v;
    }
    1
}

/// [soft 0x0851f1b0 makeConvexMesh]: a new mesh in which every polygon of `mesh` is replaced by
/// the convex pieces [`make_convex_contours`] cuts it into; answers the new mesh.
///
/// 1. `SI_meshAllocate`; the vertex count and the first three words of every vertex (the
///    position) are copied into a fresh `SI_vertexAllocate` array.
/// 2. The pieces to come are counted in advance: for each polygon its corner records − 2 plus
///    its separators; the new polygon count is 0 for now and `SI_polygonAllocate` is given that
///    count.
/// 3. For each polygon, `SI_matrixZAlign` of its normal; every vertex is carried through it
///    (((y·m₁ⱼ + x·m₀ⱼ) + z·m₂ⱼ) + m₃ⱼ, stored as floats) into a block of 12 bytes a vertex; the
///    polygon's vertex numbers (−1 for separators) go into a block of a word a corner;
///    [`make_convex_contours`] cuts it. Each piece becomes a polygon of the new mesh: the old
///    normal, material and flag, its corner count, corners in a `SI_calloc` of the count rounded
///    up to a multiple of 4 (count + 3 − ((count − 1) & 3)) records of 36 bytes, each with the
///    vertex number and, copied from the last old corner of the same vertex, the three words at
///    +8, +0xc and +0x10 (the corner normal). The piece's array and record are freed, then the
///    corner block; the vertex block at the end. When the cutting fails the polygon is dropped.
/// 4. The new polygon count is set and `SI_meshUpdateNodes(new, 0)` builds the rest. Last, each
///    new edge takes the word at +0x12 (its flag) from the old edge between the same two vertices
///    (found in the star of the first vertex of the old mesh).
pub fn make_convex_mesh(m: &mut Mem, o: &mut dyn Outside, mesh: u32) -> u32 {
    let new = super::mesh_allocate(m);
    let nv = m.i32(mesh + mo::NB_VERTICES);
    m.set_i32(new + mo::NB_VERTICES, nv);
    let verts = si_hrc::image::geom::vertex_allocate(m, nv);
    m.set_u32(new + mo::VERTICES, verts);
    for k in 0..nv.max(0) as u32 {
        let (src, dst) = (m.u32(mesh + mo::VERTICES).wrapping_add(k * mo::VERTEX), m.u32(new + mo::VERTICES).wrapping_add(k * mo::VERTEX));
        for w in [0u32, 4, 8] {
            let v = m.u32(src + w);
            m.set_u32(dst + w, v);
        }
    }
    let np = m.i32(mesh + mo::NB_POLYGONS);
    let mut room = 0i32;
    for p in 0..np.max(0) {
        let poly = super::polygon_at(m, mesh, p);
        let mut left = m.u16(poly + mo::POLYGON_NB_NODES) as u32;
        room = room.wrapping_add(left as i32 - 2);
        let nodes = m.u32(poly + mo::POLYGON_NODES);
        let mut k = 0u32;
        while left != 0 {
            if m.i32(nodes + k * mo::NODE) == -1 {
                room = room.wrapping_add(1);
            }
            k += 1;
            left -= 1;
        }
    }
    m.set_i32(new + mo::NB_POLYGONS, 0);
    let polys = si_hrc::image::geom::polygon_allocate(m, room);
    m.set_u32(new + mo::POLYGONS, polys);
    let pts = m.si_malloc((nv as u32).wrapping_mul(12));
    let head = m.local(4);
    let mut made = 0i32;
    for p in 0..np.max(0) {
        let poly = super::polygon_at(m, mesh, p);
        let z = si_math::algebra::matrix_z_align(&si_math::types::Vector::from_array(f3(m, poly)));
        for k in 0..nv.max(0) as u32 {
            let [px, py, pz] = f3(m, vertex_at(m, mesh, k as i32));
            let col = |j: usize| (((x(py) * z[1][j] + x(px) * z[0][j]) + x(pz) * z[2][j]) + z[3][j]).to_f32();
            put_f3(m, pts + 12 * k, [col(0), col(1), col(2)]);
        }
        let ncorners = m.u16(poly + mo::POLYGON_NB_NODES) as u32;
        let list = m.si_malloc(ncorners.wrapping_mul(4));
        for k in 0..ncorners {
            let v = m.i32(m.u32(poly + mo::POLYGON_NODES) + k * mo::NODE);
            m.set_i32(list + 4 * k, v);
        }
        m.set_u32(head, 0);
        let ok = make_convex_contours(m, pts, list, ncorners as i32, head);
        if ok != 0 {
            let mut c = m.u32(head);
            while c != 0 {
                let next = m.u32(c + 0xc);
                let rec = m.u32(new + mo::POLYGONS).wrapping_add((made as u32).wrapping_mul(mo::POLYGON));
                for w in [0u32, 4, 8] {
                    let v = m.u32(poly + w);
                    m.set_u32(rec + w, v);
                }
                let (flag, mat) = (m.u16(poly + mo::POLYGON_FLAG), m.u16(poly + mo::POLYGON_MATERIAL));
                m.set_u16(rec + mo::POLYGON_FLAG, flag);
                m.set_u16(rec + mo::POLYGON_MATERIAL, mat);
                let count = m.i32(c);
                m.set_u16(rec + mo::POLYGON_NB_NODES, count as u16);
                let nodes = m.si_calloc(count.wrapping_sub(((count.wrapping_sub(1) & 3) - 3) as i32) as u32, 0x24);
                m.set_u32(rec + mo::POLYGON_NODES, nodes);
                made += 1;
                let arr = m.u32(c + 4);
                for k in 0..count.max(0) as u32 {
                    let vid = m.i32(arr + 4 * k);
                    m.set_i32(nodes + k * mo::NODE, vid);
                    for j in 0..m.u16(poly + mo::POLYGON_NB_NODES) as u32 {
                        let old = m.u32(poly + mo::POLYGON_NODES) + j * mo::NODE;
                        let v = m.i32(old);
                        if v != -1 && v == vid {
                            for w in [8u32, 0xc, 0x10] {
                                let t = m.u32(old + w);
                                m.set_u32(nodes + k * mo::NODE + w, t);
                            }
                        }
                    }
                }
                if arr != 0 {
                    m.free(arr);
                    m.set_u32(c + 4, 0);
                }
                m.free(c);
                c = next;
            }
            if list != 0 {
                m.free(list);
            }
        }
    }
    if pts != 0 {
        m.free(pts);
    }
    m.set_i32(new + mo::NB_POLYGONS, made);
    update_nodes(m, o, new, 0);
    let ne = m.i32(new + mo::NB_EDGES);
    let mut e = 1i32;
    if 0 < ne {
        loop {
            let rec = super::edge_record(m, new, e);
            e += 1;
            let old_v = vertex_at(m, mesh, m.i32(rec));
            let count = m.u16(old_v + mo::VERTEX_NB_STAR) as u32;
            let star = m.u32(old_v + mo::VERTEX_STAR);
            for k in 0..count {
                let s = star.wrapping_add(k * 12);
                if m.i32(s) == m.i32(rec + 4) {
                    let n = m.i32(s + 4);
                    let n = if n < 0 { n.wrapping_neg() } else { n };
                    let f = m.u16(super::edge_record(m, mesh, n) + 0x12);
                    m.set_u16(rec + 0x12, f);
                    break;
                }
            }
            if e > m.i32(new + mo::NB_EDGES) {
                break;
            }
        }
    }
    m.release(4);
    new
}
