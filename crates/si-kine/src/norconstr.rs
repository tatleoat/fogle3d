// SPDX-License-Identifier: MIT
//! The normal constraint's search on a polygon mesh (`constraint.c`), on the shared memory
//! image (finding 0024): the mesh, its polygons and vertices are `si-types`' records in
//! place, the node is the 0x430-byte `__SI_Model`.
//!
//! Given a point, the search answers the normal of the mesh there: at a vertex the mean of
//! the normals of the polygons around it, elsewhere the normal of the polygon whose plane
//! the point projects into, turned to the point's side. The orientation step that uses the
//! answer is `limits::nor_constr_map_direction` (first pass, old form).
//!
//! A polygon's first three floats (`vec_00` in `si-types`) are read here as its normal.
//!
//! Rounding points are from the instruction listing (finding 0008): r32(…) is a store to
//! float, everything inside is in the x87 register in the order written.

use si_math::algebra::{self, Line, Plane};
use si_mem::{Mem, Ptr32};
use si_types::geom::{Mesh, Polygon, Vector};
use si_types::model::Model;
use si_x87::{fmov, trig, x, F80};

use crate::outside::{si_status_display_message, Outside};
use crate::rodata::S_PATCH_NOT_DONE;

fn mv(v: Vector) -> si_math::types::Vector {
    si_math::types::Vector::new(v.x, v.y, v.z)
}

/// The position of the vertex a polygon's corner `k` names.
fn corner(m: &Mem, mesh: Ptr32<Mesh>, poly: Ptr32<Polygon>, k: i32) -> Vector {
    let vertex = m[m[poly].nodes.add(k)].vertex;
    m[m[mesh].vertices.add(vertex)].pos
}

/// The mean normal at a vertex of the mesh [soft 0x081f355c norConstrMdlAtVertex].
///
/// The first vertex nearer to `p` than 0.0001 (the distance rounded to float, the bound a
/// double) is taken; the normals of all polygons that have it as a corner are summed (each
/// sum a float) and, when there are two or more, multiplied by 1/count kept in the
/// register. Answers 1 and writes `out`; without such a vertex 0, and `out` is not
/// written. A vertex no polygon uses gives (0, 0, 0) and 1.
pub fn nor_constr_mdl_at_vertex(m: &mut Mem, mesh: Ptr32<Mesh>, p: Vector, out: Ptr32<Vector>) -> i32 {
    let nb_vertices = m[mesh].nb_vertices;
    let vertices = m[mesh].vertices;
    let mut found = None;
    for i in 0..nb_vertices {
        let v = m[vertices.add(i)].pos;
        let (dx, dy) = (x(p.x) - v.x, x(p.y) - v.y);
        let d = ((dx * dx + dy * dy) + (x(p.z) - v.z) * (x(p.z) - v.z)).sqrt().to_f32();
        if x(d).lt(x(0.0001f64)) {
            found = Some(i);
            break;
        }
    }
    let Some(found) = found else { return 0 };
    let (mut sx, mut sy, mut sz) = (0.0f32, 0.0f32, 0.0f32);
    let mut count = 0i32;
    let mut i = 0;
    while i < m[mesh].nb_polygons {
        let poly = m[mesh].polygons.add(i);
        i += 1;
        let mut k = 0;
        while k < m[poly].nb_nodes as i32 {
            if found == m[m[poly].nodes.add(k)].vertex {
                count += 1;
                let n = m[poly].vec_00;
                sx = (x(sx) + n.x).to_f32();
                sy = (x(sy) + n.y).to_f32();
                sz = (x(sz) + n.z).to_f32();
            }
            k += 1;
        }
    }
    if count > 1 {
        let f = F80::ONE / F80::from_i32(count);
        m[out].x = (f * sx).to_f32();
        m[out].y = (f * sy).to_f32();
        m[out].z = (f * sz).to_f32();
    } else {
        m[out].x = fmov(sx);
        m[out].y = fmov(sy);
        m[out].z = fmov(sz);
    }
    1
}

/// Whether a point of a polygon's plane lies inside the polygon, by counting crossings
/// [soft 0x081f2edc pointInConcavePolygon]. Answers 0 or 1.
///
/// The polygon is turned so that its normal `n` lies along X: with a = r32(atan2(n.y,
/// n.x)) (0 when both are exactly 0) and R the Z rotation by a, the second angle is
/// b = r32(atan2(−(R row 2 · n), R row 0 · n)), both sums unrounded; the frame is the
/// Z-Y-Z rotation by (a, b, 0). In that frame a line from the point along +Z is crossed
/// with each edge in the YZ plane (`SI_lineLineIntersection`); a crossing counts when it
/// lies on the edge (`SI_pointInEdge`) and above the point (its z minus the point's z,
/// unrounded, is greater than 0). The answer is the count's parity.
pub fn point_in_concave_polygon(m: &mut Mem, p: Vector, poly: Ptr32<Polygon>, mesh: Ptr32<Mesh>) -> i32 {
    let mut count = 0i32;
    let n = m[poly].vec_00;
    let a = if x(n.x).eq(x(0.0f32)) && x(n.y).eq(x(0.0f32)) { 0.0f32 } else { trig::fpatan(x(n.y), x(n.x)).to_f32() };
    let r = algebra::matrix_z_rotation(a);
    let t1 = x(r[0][2]) * n.z + (x(r[0][1]) * n.y + x(r[0][0]) * n.x);
    let t2 = x(n.z) * r[2][2] + (x(n.y) * r[2][1] + x(n.x) * r[2][0]);
    let b = trig::fpatan(-t2, t1).to_f32();
    let r = crate::iktools::matrix_zyz_euler_rotation(a, b, 0.0);
    // the point in the polygon's frame, each coordinate a float
    let q = Vector {
        x: (x(r[0][2]) * p.z + (x(r[0][1]) * p.y + x(r[0][0]) * p.x)).to_f32(),
        y: (x(r[1][2]) * p.z + (x(r[1][0]) * p.x + x(r[1][1]) * p.y)).to_f32(),
        z: ((x(p.y) * r[2][1] + x(p.x) * r[2][0]) + x(p.z) * r[2][2]).to_f32(),
    };
    let up = Vector { x: 0.0, y: 0.0, z: 1.0 };
    let ray = Line { point: mv(q), dir: mv(up) };
    let mut k = 0i32;
    while k < m[poly].nb_nodes as i32 {
        let v = corner(m, mesh, poly, k);
        let from = Vector {
            x: ((x(r[0][1]) * v.y + x(r[0][0]) * v.x) + x(r[0][2]) * v.z).to_f32(),
            y: ((x(r[1][1]) * v.y + x(r[1][0]) * v.x) + x(r[1][2]) * v.z).to_f32(),
            z: ((x(r[2][1]) * v.y + x(v.x) * r[2][0]) + x(r[2][2]) * v.z).to_f32(),
        };
        let next = if k == m[poly].nb_nodes as i32 - 1 { 0 } else { k + 1 };
        let w = corner(m, mesh, poly, next);
        let to = Vector {
            x: (x(r[0][2]) * w.z + (x(r[0][1]) * w.y + x(r[0][0]) * w.x)).to_f32(),
            y: (x(r[1][2]) * w.z + (x(r[1][1]) * w.y + x(r[1][0]) * w.x)).to_f32(),
            z: ((x(w.y) * r[2][1] + x(w.x) * r[2][0]) + x(w.z) * r[2][2]).to_f32(),
        };
        let edge = Line {
            point: mv(from),
            dir: si_math::types::Vector::new((x(to.x) - from.x).to_f32(), (x(to.y) - from.y).to_f32(), (x(to.z) - from.z).to_f32()),
        };
        // the crossing point is a local the original leaves unset where the callee does;
        // it is read only after the callee said it wrote all of it
        let mut hit = si_math::types::Vector::default();
        if algebra::line_line_intersection(&ray, &edge, &mut hit) && algebra::point_in_edge(hit, mv(from), mv(to)) {
            let (dx, dy, dz) = (x(hit.x) - q.x, x(hit.y) - q.y, x(hit.z) - q.z);
            let s = dz * up.z + (dy * up.y + dx * up.x);
            if x(0.0f32).lt(s) {
                count += 1;
            }
        }
        k += 1;
    }
    (count % 2) as i16 as i32
}

/// One polygon of the mesh as a candidate for the point `p`
/// [soft 0x081f3350 norConstrFindClosePoly].
///
/// `*dist` gets the distance from `p` to the plane of the polygon's first three corners
/// (`SI_pointToPlaneDist`). The point is then dropped onto the plane along the polygon's
/// normal (`SI_linePlaneIntersection`, the plane through the first corner); if the foot is
/// inside the polygon the answer is 1 and `*flag` says whether `p` is behind the polygon:
/// (p − foot) · normal, unrounded, below −0.01. Otherwise 0, and `*flag` is not written.
/// The first three corners are read whatever the polygon's corner count.
pub fn nor_constr_find_close_poly(m: &mut Mem, p: Vector, index: i32, mesh: Ptr32<Mesh>, dist: Ptr32<f32>, flag: Ptr32<i16>) -> i32 {
    let poly = m[mesh].polygons.add(index);
    let a = corner(m, mesh, poly, 0);
    let b = corner(m, mesh, poly, 1);
    let c = corner(m, mesh, poly, 2);
    let d = algebra::point_to_plane_dist(mv(p), mv(a), mv(b), mv(c));
    m[dist] = d;
    let n = m[poly].vec_00;
    let line = Line { point: mv(p), dir: mv(n) };
    let plane = Plane { point: mv(a), normal: mv(n) };
    let foot = algebra::line_plane_intersection(&line, &plane);
    let foot = Vector { x: foot.x, y: foot.y, z: foot.z };
    if point_in_concave_polygon(m, foot, poly, mesh) as i16 == 0 {
        return 0;
    }
    let (dx, dy, dz) = (x(p.x) - foot.x, x(p.y) - foot.y, x(p.z) - foot.z);
    let n = m[poly].vec_00;
    let side = (dy * n.y + dx * n.x) + dz * n.z;
    m[flag] = side.lt(x(-0.01f64)) as i16;
    1
}

/// The normal of a mesh at the point `p` (in the mesh's own space)
/// [soft 0x081f36d4 norVectorFromMesh]. Answers 1 and writes `out`, or 0.
///
/// At a vertex, [`nor_constr_mdl_at_vertex`]'s answer. Otherwise every polygon is asked
/// ([`nor_constr_find_close_poly`]). Two values run through the loop in the register: the
/// plane distance of the last polygon accepted for comparison, D (100000 at first), and
/// the smallest mean corner distance so far, A (100000 at first; kept unrounded, so it
/// has the register's precision when the next polygon is compared with it). A polygon
/// that contains the point's foot is looked at when its distance d is below D or within
/// 0.0001 of it; D becomes d; the mean of the distances from `p` to its corners (each
/// square root added unrounded to a float sum, the sum divided by the corner count) is
/// compared with A, and a smaller one makes the polygon the best. The best polygon's
/// normal is the answer, negated when the point is behind it.
pub fn nor_vector_from_mesh(m: &mut Mem, mesh: Ptr32<Mesh>, p: Vector, out: Ptr32<Vector>) -> i32 {
    if nor_constr_mdl_at_vertex(m, mesh, p, out) as i16 != 0 {
        return 1;
    }
    let mut near = x(100000.0f32);
    let mut least = x(100000.0f32);
    let mut best = -1i32;
    let mut best_flag = 0i16;
    let dist: Ptr32<f32> = Ptr32::new(m.local(4));
    let flag: Ptr32<i16> = Ptr32::new(m.local(4));
    let mut i = 0i32;
    while i < m[mesh].nb_polygons {
        m[dist] = 0.0;
        m[flag] = 0;
        if nor_constr_find_close_poly(m, p, i, mesh, dist, flag) as i16 != 0 {
            let d = m[dist];
            if x(d).lt(near) || (x(d) - near).abs().lt(x(0.0001f64)) {
                near = x(d);
                m[dist] = 0.0;
                let poly = m[mesh].polygons.add(i);
                let mut k = 0i32;
                while k < m[poly].nb_nodes as i32 {
                    let v = corner(m, mesh, poly, k);
                    let (dx, dy) = (x(p.x) - v.x, x(p.y) - v.y);
                    let len = ((dx * dx + dy * dy) + (x(p.z) - v.z) * (x(p.z) - v.z)).sqrt();
                    let sum = (len + m[dist]).to_f32();
                    m[dist] = sum;
                    k += 1;
                }
                let mean = x(m[dist]) / F80::from_i32(m[poly].nb_nodes as i32);
                m[dist] = mean.to_f32();
                if mean.lt(least) {
                    least = mean;
                    best_flag = m[flag];
                    best = i;
                }
            }
        }
        i += 1;
    }
    m.release(8);
    if best == -1 {
        return 0;
    }
    let n = m[m[mesh].polygons.add(best)].vec_00;
    if best_flag != 0 {
        m[out].x = (-x(n.x)).to_f32();
        m[out].y = (-x(n.y)).to_f32();
        m[out].z = (-x(n.z)).to_f32();
    } else {
        m[out].x = fmov(n.x);
        m[out].y = fmov(n.y);
        m[out].z = fmov(n.z);
    }
    1
}

/// The offset from `frame`'s global position to `node`'s, taken through the 3 × 3 part of
/// `frame`'s matrix at +0x1c4 (`gmat`) as matrix times column: the differences are
/// unrounded, each coordinate is rounded to float once. (Inlined in the normal and the
/// tangency constraint's routines; not a function of the original.)
pub(crate) fn offset_in_frame(m: &Mem, node: Ptr32<Model>, frame: Ptr32<Model>) -> Vector {
    let (a, b) = (m[node].gtrs, m[frame].gtrs);
    let (dx, dy, dz) = (x(a.x) - b.x, x(a.y) - b.y, x(a.z) - b.z);
    let g = m[frame].gmat;
    Vector {
        x: ((x(g[0][1]) * dy + x(g[0][0]) * dx) + x(g[0][2]) * dz).to_f32(),
        y: ((x(g[1][0]) * dx + x(g[1][1]) * dy) + x(g[1][2]) * dz).to_f32(),
        z: ((dy * g[2][1] + dx * g[2][0]) + dz * g[2][2]).to_f32(),
    }
}

/// `*out` taken through all four rows of `frame`'s matrix at +0x178 (`grot`) as row times
/// matrix: the fourth row is added, so what comes out is a point, the vector's tip seen
/// from that row. (Inlined likewise.)
pub(crate) fn through_grot(m: &mut Mem, frame: Ptr32<Model>, out: Ptr32<Vector>) {
    let v = m[out];
    let r = m[frame].grot;
    m[out].x = (((x(v.y) * r[1][0] + x(v.x) * r[0][0]) + x(v.z) * r[2][0]) + r[3][0]).to_f32();
    m[out].y = (((x(v.y) * r[1][1] + x(v.x) * r[0][1]) + x(v.z) * r[2][1]) + r[3][1]).to_f32();
    m[out].z = (((x(v.y) * r[1][2] + x(v.x) * r[0][2]) + x(v.z) * r[2][2]) + r[3][2]).to_f32();
}

/// The direction a normal constraint aims `node` along: the normal of `surface` at the
/// node's place [soft 0x081f393c si_norConstrComputeNormalVec]. Answers 1 and writes
/// `out`, or 0.
///
/// The offset from the surface's global position to the node's goes through the surface's
/// `gmat` ([`offset_in_frame`]); for a mesh (kind 4) with a definition
/// [`nor_vector_from_mesh`] answers in that space and the answer goes back through `grot`
/// ([`through_grot`]: the result is a point one normal away from the matrix's fourth row,
/// not a direction). A patch (kind 1) only gets a message on the status line; every other
/// kind answers 0.
pub fn nor_constr_compute_normal_vec(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, surface: Ptr32<Model>, out: Ptr32<Vector>) -> i32 {
    let local = offset_in_frame(m, node, surface);
    let kind = m[surface].kind;
    if kind == 4 {
        let mesh: Ptr32<Mesh> = m[surface].definition.cast();
        if mesh.is_null() || nor_vector_from_mesh(m, mesh, local, out) as i16 == 0 {
            return 0;
        }
        through_grot(m, surface, out);
        return 1;
    }
    if kind == 1 {
        si_status_display_message(o, m, S_PATCH_NOT_DONE.0, 2);
    }
    0
}

// ---------------------------------------------------------------------------------------
// The closest-point search on a patch (the second half of `constraint.c`'s normal
// constraint). Nothing in the program calls it (finding 0022: `norConstrSamplePatchShell`
// has one call site, its own), but the group is reproduced like the rest.

use si_spline::patch_eval_image::patch_update_matrix;
use si_spline::patch_image::{patch_add_row, patch_allocate, patch_dispose, patch_update_surface};
use si_types::geom::Patch;

/// The signed division of the original (`cdq`, `idiv`): a divisor of 0 is the processor's
/// divide error in the original, a recorded fault here with zeros for the answer.
fn idiv(m: &Mem, a: i32, b: i32) -> (i32, i32) {
    if b == 0 || (a == i32::MIN && b == -1) {
        m.record_fault(0, 0, si_mem::Access::Read);
        return (0, 0);
    }
    (a.wrapping_div(b), a.wrapping_rem(b))
}

/// Adds rows to a patch so that a coarse patch has the points to cut a shell from
/// [soft 0x081f2d90 norConstrDividPatchShell]: one row in V at display index 6 (5 when V
/// is linear) for a linear U, 10 (9) for the others, then one in U at 2 (1 for a linear
/// U), each through [`patch_add_row`] with no index table.
pub fn nor_constr_divid_patch_shell(m: &mut Mem, p: Ptr32<Patch>) {
    let first = if m[p].u_type == 0 {
        if m[p].v_type == 0 { 5 } else { 6 }
    } else if m[p].v_type == 0 {
        9
    } else {
        10
    };
    patch_add_row(m, p, first, 1, 0);
    let second = if m[p].u_type == 0 { 1 } else { 2 };
    patch_add_row(m, p, second, 0, 0);
}

/// A new small patch cut from `src` [soft 0x081f2bbc norConstrExtractPatchShell]: the two
/// types, steps and tensions are copied, the new patch has 2 points a side in a linear
/// direction and 4 otherwise, and its points and tags are the source's from row `row`,
/// column `col` on (both wrapping round the source's counts, signed remainder); then
/// [`patch_update_matrix`] and [`patch_update_surface`]. Answers the new patch (null when
/// the allocation failed: the original then writes through it; not reproduced).
pub fn nor_constr_extract_patch_shell(m: &mut Mem, src: Ptr32<Patch>, row: i32, col: i32) -> Ptr32<Patch> {
    let n = patch_allocate(m);
    m[n].u_type = m[src].u_type;
    m[n].v_type = m[src].v_type;
    m[n].u_step = m[src].u_step;
    m[n].v_step = m[src].v_step;
    m[n].u_tension = fmov(m[src].u_tension);
    m[n].v_tension = fmov(m[src].v_tension);
    m[n].nb_u = if m[src].u_type == 0 { 2 } else { 4 };
    m[n].nb_v = if m[src].v_type == 0 { 2 } else { 4 };
    let count = m[n].nb_u as u32 * m[n].nb_v as u32;
    m[n].points = Ptr32::new(m.si_calloc(count, 16));
    let count = m[n].nb_u as u32 * m[n].nb_v as u32;
    m[n].tags = Ptr32::new(m.si_calloc(count, 2));
    let mut k = 0u32;
    let mut i = 0i32;
    while i < m[n].nb_u as i32 {
        let (_, r) = idiv(m, row.wrapping_add(i), m[src].nb_u as i32);
        let base = (m[src].nb_v as i32).wrapping_mul(r);
        i += 1;
        let mut j = 0i32;
        while j < m[n].nb_v as i32 {
            let (_, c) = idiv(m, col.wrapping_add(j), m[src].nb_v as i32);
            let at = base.wrapping_add(c) as u32;
            let (sp, dp) = (m[src].points.addr().wrapping_add(at << 4), m[n].points.addr().wrapping_add(k << 4));
            for w in 0..4 {
                let v = m.u32(sp.wrapping_add(4 * w));
                m.set_u32(dp.wrapping_add(4 * w), v);
            }
            let t = m.u16(m[src].tags.addr().wrapping_add(at << 1));
            let d = m[n].tags.addr().wrapping_add(k << 1);
            m.set_u16(d, t);
            k += 1;
            j += 1;
        }
    }
    patch_update_matrix(m, n);
    patch_update_surface(m, n);
    n
}

/// The 4x4 float matrix at `p` + `off` (the patch's two basis matrices).
fn basis_words(m: &Mem, p: Ptr32<Patch>, off: u32) -> [f32; 16] {
    let mut a = [0.0f32; 16];
    for (k, v) in a.iter_mut().enumerate() {
        *v = m.f32(p.addr().wrapping_add(off + 4 * k as u32));
    }
    a
}

/// Where on a patch a point is closest to the surface, and which way it runs there
/// [soft 0x081f1d10 norConstrFindCloseArea]: `(p.x, p.y, p.z)` is the point, `row` and `col`
/// receive the start (in control points) of the closest segment of the grid, `out` the
/// unit normal of the surface there.
///
/// The patch's two basis matrices (+0x24 and +0x64) are turned into eight weights each by
/// the fixed factors 0.125, 0.25, 0.5, 1 (the point on the curve at t = 1/2 and the
/// factors for 3/4): `f` (four floats) and `g` from the first, `Lf8`.. from the second. Then
/// for each segment of the U direction and each of V (`SI_splineNbSegmentStep` on the
/// type, the point count and the closed flag; the start advances by the step) the 4 by 4
/// block of control points is read (wrapping round the point counts, signed remainders),
/// the surface point and its two tangents are computed from the three coordinate planes of
/// the block, and when the distance is strictly less than the best so far (100000 at the
/// start) the answer is taken: `row`, `col`, the best distance, and `out` = the cross
/// product of the two tangents (each normalised first when longer than 0.0001) normalised
/// again the same way. Nothing is written for a segment that is not closer.
///
/// The arithmetic follows the instruction listing: every local is a float in memory, every
/// sum inside one statement stays in the x87 register (finding 0008), so each `r32(...)`
/// below is one store.
pub fn nor_constr_find_close_area(m: &mut Mem, p: Ptr32<Patch>, pt: Vector, row: Ptr32<i32>, col: Ptr32<i32>, out: Ptr32<Vector>) {
    let (q25, q125, q5, q75) = (x(0.25f32), x(0.125f32), x(0.5f32), x(0.75f32));
    let zero = x(0.0f32);
    let mut best = 100000.0f32;
    let a = basis_words(m, p, 0x24);
    let b = basis_words(m, p, 0x64);
    // the weights from the first matrix
    let f0 = (((x(a[4]) * q25 + x(a[0]) * q125) + x(a[8]) * q5) + x(a[12])).to_f32();
    let ec = (((x(a[5]) * q25 + x(a[1]) * q125) + x(a[9]) * q5) + x(a[13])).to_f32();
    let e8 = (((x(a[6]) * q25 + x(a[2]) * q125) + x(a[10]) * q5) + x(a[14])).to_f32();
    let e4 = (((x(a[7]) * q25 + x(a[3]) * q125) + x(a[11]) * q5) + x(a[15])).to_f32();
    let g110 = (x(a[12]) * zero + ((x(a[0]) * q75 + x(a[4])) + x(a[8]))).to_f32();
    let g10c = (x(a[13]) * zero + ((x(a[1]) * q75 + x(a[5])) + x(a[9]))).to_f32();
    let g108 = (x(a[14]) * zero + ((x(a[2]) * q75 + x(a[6])) + x(a[10]))).to_f32();
    let g104 = (((x(a[3]) * q75 + x(a[7])) + x(a[11])) + x(a[15]) * zero).to_f32();
    // and from the second
    let l100 = (((x(b[0]) * q125 + x(b[1]) * q25) + x(b[2]) * q5) + x(b[3])).to_f32();
    let lfc = (((x(b[4]) * q125 + x(b[5]) * q25) + x(b[6]) * q5) + x(b[7])).to_f32();
    let lf8 = (((x(b[8]) * q125 + x(b[9]) * q25) + x(b[10]) * q5) + x(b[11])).to_f32();
    let lf4 = ((x(b[14]) * q5 + (x(b[12]) * q125 + x(b[13]) * q25)) + x(b[15])).to_f32();
    let l120 = (x(b[3]) * zero + ((x(b[0]) * q75 + x(b[1])) + x(b[2]))).to_f32();
    let l118 = (x(b[11]) * zero + ((x(b[8]) * q75 + x(b[9])) + x(b[10]))).to_f32();
    let l11c = (zero * x(b[7]) + ((x(b[4]) * q75 + x(b[5])) + x(b[6]))).to_f32();
    let l114 = (((x(b[12]) * q75 + x(b[13])) + x(b[14])) + x(b[15]) * zero).to_f32();

    let (segs_u, step_u) = si_spline::eval::nb_segment_step(m[p].u_type, m[p].nb_u as i32, m[p].u_closed);
    let (segs_v, step_v) = si_spline::eval::nb_segment_step(m[p].v_type, m[p].nb_v as i32, m[p].v_closed);
    let (e_f0, e_ec, e_e8, e_e4) = (x(f0), x(ec), x(e8), x(e4));
    let mut r0 = 0i32;
    let mut iu = 0i32;
    while iu < segs_u {
        iu += 1;
        let mut c0 = 0i32;
        let mut iv = 0i32;
        while iv < segs_v {
            iv += 1;
            // the 4 by 4 block of control points, one plane a coordinate
            let mut xs = [0u32; 16];
            let mut ys = [0u32; 16];
            let mut zs = [0u32; 16];
            for ii in 0..4i32 {
                let (_, rr) = idiv(m, r0.wrapping_add(ii), m[p].nb_u as i32);
                let base = rr.wrapping_mul(m[p].nb_v as i32);
                for jj in 0..4i32 {
                    let (_, cc) = idiv(m, c0.wrapping_add(jj), m[p].nb_v as i32);
                    let at = m[p].points.addr().wrapping_add((base.wrapping_add(cc) as u32) << 4);
                    let k = (4 * ii + jj) as usize;
                    xs[k] = m.u32(at);
                    ys[k] = m.u32(at.wrapping_add(4));
                    zs[k] = m.u32(at.wrapping_add(8));
                }
            }
            let xf = |k: usize| x(f32::from_bits(xs[k]));
            let yf = |k: usize| x(f32::from_bits(ys[k]));
            let zf = |k: usize| x(f32::from_bits(zs[k]));
            let (w100, wfc, wf8, wf4) = (x(l100), x(lfc), x(lf8), x(lf4));
            let (w120, w11c, w118, w114) = (x(l120), x(l11c), x(l118), x(l114));
            // x plane
            let t1 = ((e_ec * xf(4) + e_f0 * xf(0)) + e_e8 * xf(8)) + e_e4 * xf(12);
            let t2 = ((e_f0 * xf(1) + e_ec * xf(5)) + e_e8 * xf(9)) + e_e4 * xf(13);
            let mut l194 = (t1 * w100 + t2 * wfc).to_f32();
            let t3 = ((e_f0 * xf(2) + e_ec * xf(6)) + e_e8 * xf(10)) + e_e4 * xf(14);
            l194 = (t3 * wf8 + x(l194)).to_f32();
            let mut l198 = (t1 * w120).to_f32();
            let t4 = e_e4 * xf(15) + ((e_f0 * xf(3) + e_ec * xf(7)) + e_e8 * xf(11));
            let t5 = t4 * wf4 + x(l194);
            l194 = t5.to_f32();
            l198 = (t2 * w11c + x(l198)).to_f32();
            let t6 = t4 * w114 + (t3 * w118 + x(l198));
            l198 = t6.to_f32();
            let mut l138 = l198;
            // y plane
            let t7 = (e_e8 * yf(8) + (e_f0 * yf(0) + e_ec * yf(4))) + e_e4 * yf(12);
            let mut l19c = (t7 * w100).to_f32();
            let t8 = e_e4 * yf(13) + (e_e8 * yf(9) + (e_f0 * yf(1) + e_ec * yf(5)));
            l19c = (t8 * wfc + x(l19c)).to_f32();
            let t9 = e_e4 * yf(14) + ((e_f0 * yf(2) + e_ec * yf(6)) + e_e8 * yf(10));
            l19c = (t9 * wf8 + x(l19c)).to_f32();
            let mut l1a0 = (t7 * w120).to_f32();
            l1a0 = (t8 * w11c + x(l1a0)).to_f32();
            let t10 = ((e_f0 * yf(3) + e_ec * yf(7)) + e_e8 * yf(11)) + e_e4 * yf(15);
            let t11 = t10 * wf4 + x(l19c);
            l19c = t11.to_f32();
            let t12 = t10 * w114 + (t9 * w118 + x(l1a0));
            l1a0 = t12.to_f32();
            let mut l134 = l1a0;
            // z plane
            let t13 = e_e4 * zf(12) + (e_e8 * zf(8) + (e_f0 * zf(0) + e_ec * zf(4)));
            let t14 = e_e4 * zf(13) + ((e_f0 * zf(1) + e_ec * zf(5)) + e_e8 * zf(9));
            let l1a4 = (e_f0 * zf(3) + e_ec * zf(7)).to_f32();
            let mut l1a8 = (t13 * w100).to_f32();
            l1a8 = (t14 * wfc + x(l1a8)).to_f32();
            let t16 = e_e4 * zf(14) + (e_e8 * zf(10) + (e_ec * zf(6) + e_f0 * zf(2)));
            let t15 = e_e8 * zf(11) + x(l1a4);
            let t17 = e_e4 * zf(15) + t15;
            let mut l1ac = (t13 * w120).to_f32();
            l1a8 = (t16 * wf8 + x(l1a8)).to_f32();
            let t18 = t17 * wf4 + x(l1a8);
            l1a8 = t18.to_f32();
            let t19 = t17 * w114 + ((t14 * w11c + x(l1ac)) + t16 * w118);
            l1ac = t19.to_f32();
            let mut l130 = l1ac;
            // the second weights set: the other tangent
            let (h110, h10c, h108, h104) = (x(g110), x(g10c), x(g108), x(g104));
            let ca = (h104 * xf(15) + ((h110 * xf(3) + h10c * xf(7)) + h108 * xf(11))) * wf4;
            let cb = (h104 * xf(14) + ((h110 * xf(2) + h10c * xf(6)) + h108 * xf(10))) * wf8;
            let cc_ = (((h110 * xf(1) + h10c * xf(5)) + h108 * xf(9)) + h104 * xf(13)) * wfc;
            let cd = ((h108 * xf(8) + (h10c * xf(4) + h110 * xf(0))) + h104 * xf(12)) * w100;
            let mut l144 = (ca + (cb + (cc_ + cd))).to_f32();
            let ce = (((h110 * yf(2) + h10c * yf(6)) + h108 * yf(10)) + h104 * yf(14)) * wf8;
            let cf = (((h110 * yf(1) + h10c * yf(5)) + h108 * yf(9)) + h104 * yf(13)) * wfc;
            let cg = (((h10c * yf(4) + h110 * yf(0)) + h108 * yf(8)) + h104 * yf(12)) * w100;
            let ch = (h104 * yf(15) + ((h110 * yf(3) + h10c * yf(7)) + h108 * yf(11))) * wf4;
            let mut l140 = ((ce + (cf + cg)) + ch).to_f32();
            let t20 = ((h110 * zf(0) + h10c * zf(4)) + h108 * zf(8)) + h104 * zf(12);
            let t21 = ((h110 * zf(1) + h10c * zf(5)) + h108 * zf(9)) + h104 * zf(13);
            let t22 = h104 * zf(14) + (h108 * zf(10) + (h110 * zf(2) + h10c * zf(6)));
            let mut l1b0 = (h110 * zf(3) + h10c * zf(7)).to_f32();
            l1b0 = (h108 * zf(11) + x(l1b0)).to_f32();
            let t23 = h104 * zf(15) + x(l1b0);
            let mut l13c = (t23 * wf4 + (t22 * wf8 + (t21 * wfc + t20 * w100))).to_f32();
            // the distance
            let dz = x(pt.z) - x(l1a8);
            let dy = x(pt.y) - x(l19c);
            let dx = x(pt.x) - x(l194);
            let dist = (dz * dz + (dy * dy + dx * dx)).sqrt().to_f32();
            if x(dist).lt(x(best)) {
                m.set_i32(row.addr(), r0);
                m.set_i32(col.addr(), c0);
                best = dist;
                let tiny = x(0.0001f64);
                // the first tangent, normalised when it is long enough
                let len = (x(l198) * x(l198) + x(l1a0) * x(l1a0) + x(l1ac) * x(l1ac)).sqrt().to_f32();
                if x(len).abs().gt(tiny) {
                    l138 = (x(l198) / x(len)).to_f32();
                    l134 = (x(l1a0) / x(len)).to_f32();
                    l130 = (x(l1ac) / x(len)).to_f32();
                }
                let len = ((x(l144) * x(l144) + x(l140) * x(l140)) + x(l13c) * x(l13c)).sqrt().to_f32();
                if x(len).abs().gt(tiny) {
                    l144 = (x(l144) / x(len)).to_f32();
                    l140 = (x(l140) / x(len)).to_f32();
                    l13c = (x(l13c) / x(len)).to_f32();
                }
                let t24 = x(l134) * x(l13c) - x(l130) * x(l140);
                let t26 = x(l138) * x(l140) - x(l144) * x(l134);
                let t25 = x(l130) * x(l144) - x(l138) * x(l13c);
                m[out].x = t24.to_f32();
                m[out].z = t26.to_f32();
                m[out].y = t25.to_f32();
                let len = ((t24 * t24 + t25 * t25) + t26 * t26).sqrt().to_f32();
                if x(len).abs().gt(tiny) {
                    m[out].x = (t24 / x(len)).to_f32();
                    m[out].y = (t25 / x(len)).to_f32();
                    m[out].z = (t26 / x(len)).to_f32();
                }
            }
            c0 = c0.wrapping_add(step_v);
        }
        r0 = r0.wrapping_add(step_u);
    }
}

/// Cuts the patch down around a point, again and again [soft 0x081f2df4 norConstrSamplePatchShell]:
/// it stops when `depth` is above 30, when `*lo` is below 0.0002 or 0.0002 is above `*hi`
/// (unordered values go on); otherwise it divides the patch ([`nor_constr_divid_patch_shell`]), finds the closest segment
/// ([`nor_constr_find_close_area`]), replaces `*pp` by the 4x4 (or 2x2) shell cut from that
/// segment ([`nor_constr_extract_patch_shell`], the old patch freed with
/// [`patch_dispose`]) and calls itself with `depth + 1`.
pub fn nor_constr_sample_patch_shell(m: &mut Mem, pp: u32, pt: Vector, depth: i32, lo: Ptr32<f32>, hi: Ptr32<f32>, out: Ptr32<Vector>) {
    if depth > 30 {
        return;
    }
    let c = x(0.0002f64);
    if x(m[lo]).lt(c) {
        return;
    }
    if c.gt(x(m[hi])) {
        return;
    }
    let p: Ptr32<Patch> = Ptr32::new(m.u32(pp));
    nor_constr_divid_patch_shell(m, p);
    let a = m.local(4);
    let b = m.local(4);
    m.set_i32(a, 0);
    m.set_i32(b, 0);
    nor_constr_find_close_area(m, Ptr32::new(m.u32(pp)), pt, Ptr32::new(a), Ptr32::new(b), out);
    let (r, cl) = (m.i32(a), m.i32(b));
    let n = nor_constr_extract_patch_shell(m, Ptr32::new(m.u32(pp)), r, cl);
    patch_dispose(m, pp);
    m.set_u32(pp, n.addr());
    m.release(8);
    nor_constr_sample_patch_shell(m, pp, pt, depth + 1, lo, hi, out);
}
