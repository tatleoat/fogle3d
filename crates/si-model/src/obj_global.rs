// SPDX-License-Identifier: MIT
//! Whole-mesh routines of the original's `obj_global.c` (finding 0015): copying, freeing,
//! counting, and moving a mesh through a matrix. The editing tools of that file (extrusion,
//! bevel, clean-up, triangulation, the removal routines) are not ported; the finding lists
//! them.

use si_math::algebra::{matrix_determinant3, matrix_inverse, matrix_z_align};
use si_math::types::{Matrix, Vector};
use si_x87::{x, F80};

use crate::mesh::Mesh;

/// The length a transformed normal must exceed to be rescaled: the double 0.0001
/// (0x0865f938), compared with the length rounded to float.
pub(crate) const NORMAL_MIN: f64 = 1e-4;

/// A copy of a mesh [soft 0x084404e0 SI_meshCopy]. Class: copy.
///
/// Copied: the angle and its cosine, the flags, every vertex with its star, every polygon
/// with its corners ([`node_copy`]), every edge, the display integers, and the
/// approximation settings (+0x38 to +0x4f, +0x50, +0x52). Not copied: the word at +0x2c,
/// which the copy has as a new mesh has it (0).
///
/// The display list is copied as `nb_display` integers, one fewer than
/// `SI_meshOptimizeDisplay` wrote: the copy lacks the closing −1 of the last line (the
/// original's zeroed allocation holds a 0 there, or nothing when `nb_display` is a multiple
/// of 64). At the end the copy's two selection lists are built and take over the source's
/// coordinate systems ([`crate::mesh_polygon::copy_polygon_lists`]); a status message of
/// that step is dropped here, [`mesh_copy_with`] keeps it.
pub fn mesh_copy(src: &Mesh) -> Mesh {
    mesh_copy_with(src, &mut crate::mesh_b::MeshEnv::default())
}

/// [`mesh_copy`] with the status line.
pub fn mesh_copy_with(src: &Mesh, env: &mut crate::mesh_b::MeshEnv) -> Mesh {
    let mut m = Mesh::default();
    m.angle = src.angle;
    m.cos_angle = src.cos_angle;
    m.flags = src.flags;
    m.vertices = src.vertices.clone();
    m.polygons = src.polygons.clone();
    m.edges = src.edges.clone();
    if m.edges.is_empty() {
        // SI_edgeAllocate(0) still makes the first block, with record 0 set to −1
        m.edges.push(crate::mesh::Edge { v0: -1, v1: -1, p0: -1, p1: -1, x10: 0, flag: 0 });
    } else {
        // record 0 of a block from SI_edgeAllocate is −1 in its four words; the copy loop
        // starts at edge 1
        m.edges[0] = crate::mesh::Edge { v0: -1, v1: -1, p0: -1, p1: -1, x10: 0, flag: 0 };
    }
    m.nb_display = src.nb_display;
    m.display = src.display.iter().take(src.nb_display.max(0) as usize).copied().collect();
    m.mr_first = src.mr_first;
    m.mr_approx = src.mr_approx;
    m.mr_view = src.mr_view;
    m.mr = src.mr;
    m.mr_min = src.mr_min;
    m.mr_max = src.mr_max;
    m.mr_rec = src.mr_rec;
    crate::mesh_polygon::copy_polygon_lists(Some(src), &mut m, env);
    m
}

pub(crate) fn through_a(v: [f32; 3], m: &Matrix, j: usize) -> F80 {
    ((x(v[1]) * m[1][j] + x(v[0]) * m[0][j]) + x(v[2]) * m[2][j]) + m[3][j]
}

/// A normal through a matrix, the three sums as the compiler ordered them (each differently)
/// and left in the register.
pub(crate) fn normal_through(v: [f32; 3], n: &Matrix) -> [F80; 3] {
    [
        through_a(v, n, 0),
        ((x(v[0]) * n[0][1] + x(v[1]) * n[1][1]) + x(v[2]) * n[2][1]) + n[3][1],
        (x(v[2]) * n[2][2] + (x(v[1]) * n[1][2] + x(v[0]) * n[0][2])) + n[3][2],
    ]
}

/// Moves a mesh through a matrix [soft 0x08441cd8 SI_meshTransform]: `m` for the positions,
/// `n` for the normals (its caller, `SI_modelMultiplyDef`, passes the transpose of `m`'s
/// inverse). Class: model.
///
/// Every vertex position becomes `((y·m₁ⱼ + x·m₀ⱼ) + z·m₂ⱼ) + m₃ⱼ`, rounded once.
///
/// **Normals are carried, not recomputed**, and only when the mesh flag has bit 0x4:
///
/// - a polygon normal goes through `n` (row 3 of `n` is added, like a point) and, if its new
///   length (from the unrounded components, rounded to float) exceeds 0.0001, is divided by
///   it;
/// - a corner normal goes through `n` the same way and is then brought back to **the length
///   it had before** (new components times old length over new length, the quotient kept in
///   the register): a normal that was not of length 1 stays so;
/// - both are negated when the determinant of `m`'s upper 3 × 3 part is negative.
///
/// Without bit 0x4 no normal is touched: the mesh keeps the normals of its old position.
/// Stars, edges and discontinuity bits are never touched.
pub fn mesh_transform(mesh: &mut Mesh, m: &Matrix, n: &Matrix) {
    let flip = x(0.0f32).gt(matrix_determinant3(m));
    for v in &mut mesh.vertices {
        let p = v.pos;
        v.pos = [through_a(p, m, 0).to_f32(), through_a(p, m, 1).to_f32(), through_a(p, m, 2).to_f32()];
    }
    if mesh.flags & 4 == 0 {
        return;
    }
    let negate = |v: &mut [f32; 3]| *v = [(-x(v[0])).to_f32(), (-x(v[1])).to_f32(), (-x(v[2])).to_f32()];
    let length = |t: &[F80; 3]| (t[2] * t[2] + (t[1] * t[1] + t[0] * t[0])).sqrt().to_f32();
    for p in &mut mesh.polygons {
        let t = normal_through(p.normal, n);
        p.normal = [t[0].to_f32(), t[1].to_f32(), t[2].to_f32()];
        let len = length(&t);
        if x(len).abs().gt(x(NORMAL_MIN)) {
            p.normal = [(x(p.normal[0]) / len).to_f32(), (x(p.normal[1]) / len).to_f32(), (x(p.normal[2]) / len).to_f32()];
        }
        if flip {
            negate(&mut p.normal);
        }
        for c in p.nodes.iter_mut().filter(|c| c.vertex != -1) {
            let o = c.normal;
            let before = ((x(o[0]) * o[0] + x(o[1]) * o[1]) + x(o[2]) * o[2]).sqrt().to_f32();
            let t = normal_through(o, n);
            c.normal = [t[0].to_f32(), t[1].to_f32(), t[2].to_f32()];
            let after = length(&t);
            if x(after).abs().gt(x(NORMAL_MIN)) {
                let ratio = x(before) / after;
                c.normal = [(x(c.normal[0]) * ratio).to_f32(), (x(c.normal[1]) * ratio).to_f32(), (ratio * c.normal[2]).to_f32()];
            }
            if flip {
                negate(&mut c.normal);
            }
        }
    }
}

/// Copies one corner onto another as `SI_meshCopy` does it (`SI_nodesCopy`, 0x08453d44, and
/// `SI_nodesCopyUV`, 0x08453d90, called without models; both belong to another source file
/// and are described here, not cited): the nine words of the record, and the further
/// texture coordinates if the source has any. The original copies fourteen floats (seven
/// pairs) whatever the source's array holds, reading past a shorter one; here the pairs
/// that exist are copied.
pub fn node_copy(dst: &mut crate::mesh::PolyNode, src: &crate::mesh::PolyNode) {
    let keep = core::mem::take(&mut dst.extra_uv);
    *dst = src.clone();
    if src.extra_uv.is_empty() {
        dst.extra_uv = keep;
    }
}

/// Frees a mesh's edges [soft 0x08448594 si_meshDisposeEdges]: the blocks, the table, and
/// the count set to 0. Nothing happens when the table pointer is null. Class: integer.
pub fn mesh_dispose_edges(mesh: &mut Mesh) {
    mesh.edges.clear();
}

/// Empties a mesh [soft 0x08440390 si_meshDelete]: the polygon lists, the stars and the
/// vertex array, the corner arrays and the polygon array, the edges, the display list.
/// The record itself stays, with its counts at 0 and its other fields as they were.
/// Class: integer.
pub fn mesh_delete(mesh: &mut Mesh) {
    mesh.vertices.clear();
    mesh.polygons.clear();
    mesh_dispose_edges(mesh);
    mesh.display.clear();
}

/// Frees a mesh and clears the caller's pointer [soft 0x084404a0 SI_meshDispose].
pub fn mesh_dispose(mesh: &mut Option<Box<Mesh>>) {
    if let Some(m) = mesh.as_mut() {
        mesh_delete(m);
    }
    *mesh = None;
}

/// The number of triangles a mesh makes [soft 0x08444d9c SI_meshNbTriangles]: for every
/// polygon that is not hidden (flag 0x800), its corner count minus 2, plus 1 for every
/// separator among its corners. Class: integer.
///
/// (A polygon of n corners with one hole of m corners has n + m + 1 records and counts
/// n + m: the two bridging triangles included. A polygon with fewer than two corners
/// counts negative.)
pub fn mesh_nb_triangles(mesh: &Mesh) -> i32 {
    let mut total = 0i32;
    for p in &mesh.polygons {
        if p.flag & 0x800 == 0 {
            total = total.wrapping_add(p.nodes.len() as u16 as i32 - 2);
            total += p.nodes.iter().filter(|n| n.vertex == -1).count() as i32;
        }
    }
    total
}

/// A position, or zeros for an index outside the vertex array (the original reads whatever
/// memory lies there).
fn pos_of(mesh: &Mesh, vertex: i32) -> [f32; 3] {
    mesh.vertices.get(vertex as usize).map_or([0.0; 3], |v| v.pos)
}

/// The squared length below which a polygon's normal counts as missing in
/// [`mesh_polygon_centroid`]: the double 0.0001 (0x0865f988).
const CENTROID_NORMAL_MIN: f64 = 1e-4;

/// The centroid of polygon `p` [soft 0x084467b8 SI_meshPolygonCentroid]: the point and
/// whether `p` names a polygon (the point is zero when it does not). Class: model.
///
/// **A polygon without a normal** (the squared length (x² + y²) + z² of its stored normal
/// is below 0.0001): the sum of its corners' positions, accumulated in floats, times
/// 1 / (number of corner records), separators included in the divisor.
///
/// **Otherwise the centroid of the area**, in the polygon's own plane:
///
/// 1. the corners are taken through `SI_matrixZAlign(normal)`, each coordinate formed as
///    ((y·m₁ⱼ + x·m₀ⱼ) + z·m₂ⱼ) + m₃ⱼ and rounded to float, except that the sum of the z
///    values is accumulated from the unrounded ones; z̄ = that sum / the number of points;
/// 2. per contour, with `a` the previous point and `b` the current one (starting with the
///    contour's last as `a`): c = a.x·b.y − b.x·a.y; the area sum S += c stays in the
///    register for the whole polygon; x += (b.x + a.x)·c and y += (b.y + a.y)·c are float
///    variables, rounded at each step;
/// 3. the result is (r32(x / (3·S)), r32(y / (3·S)), z̄) through the inverse of the matrix
///    of step 1, as ((x·i₀ⱼ + y·i₁ⱼ) + z̄·i₂ⱼ) + i₃ⱼ.
///
/// The contour walk of step 2 runs over the index of the corner **records** but stops at
/// the number of **points** (records that are not separators): in a polygon with holes the
/// last contour is cut short by one point per separator before it, and closed from the
/// point before the cut. That is the original's behaviour and is reproduced.
///
/// The original copies the corners into arrays of 2,048 entries on its stack; a polygon
/// with more corners overruns them. Here they are as long as needed.
pub fn mesh_polygon_centroid(mesh: &Mesh, p: i32) -> ([f32; 3], bool) {
    if p < 0 || p >= mesh.nb_polygons() {
        return ([0.0; 3], false);
    }
    let poly = &mesh.polygons[p as usize];
    let n = poly.normal;
    let mut out = [0.0f32; 3];
    if ((x(n[0]) * n[0] + x(n[1]) * n[1]) + x(n[2]) * n[2]).lt(x(CENTROID_NORMAL_MIN)) {
        if !poly.nodes.is_empty() {
            for c in poly.nodes.iter().filter(|c| c.vertex != -1) {
                let q = pos_of(mesh, c.vertex);
                out = [(x(out[0]) + q[0]).to_f32(), (x(out[1]) + q[1]).to_f32(), (x(out[2]) + q[2]).to_f32()];
            }
            let count = poly.nodes.len() as u16 as i32;
            for o in &mut out {
                *o = ((x(1.0f32) / x(count)) * *o).to_f32();
            }
        }
        return (out, true);
    }
    // the points, and for every corner record the number of its point or −1
    let mut pts: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<i32> = Vec::with_capacity(poly.nodes.len());
    for c in &poly.nodes {
        if c.vertex == -1 {
            idx.push(-1);
        } else {
            idx.push(pts.len() as i32);
            pts.push(pos_of(mesh, c.vertex));
        }
    }
    let za = matrix_z_align(&Vector::from_array(n));
    let inv = match matrix_inverse(&za) {
        Some(m) => m,
        None => {
            // SI_matrixInverse writes nothing for a matrix it cannot invert (NaN components
            // in the normal); the original goes on with what its stack held
            crate::env::stale::note();
            [[0.0; 4]; 4]
        }
    };
    let mut zsum = x(0.0f32);
    for q in &mut pts {
        let [px, py, pz] = *q;
        let col = |j: usize| ((x(py) * za[1][j] + x(px) * za[0][j]) + x(pz) * za[2][j]) + za[3][j];
        let z = col(2);
        *q = [col(0).to_f32(), col(1).to_f32(), z.to_f32()];
        zsum = zsum + z;
    }
    let npts = pts.len() as i32;
    let zavg = zsum / x(npts);
    let mut area = x(0.0f32);
    let mut start = 0i32;
    for i in 0..=npts {
        if i == npts || idx[i as usize] == -1 {
            if start <= i - 1 {
                let mut a = pts[idx[(i - 1) as usize] as usize];
                for k in start..i {
                    let b = pts[idx[k as usize] as usize];
                    let c = x(a[0]) * b[1] - x(b[0]) * a[1];
                    area = area + c;
                    let nx = ((x(b[0]) + a[0]) * c + out[0]).to_f32();
                    let ny = ((x(b[1]) + a[1]) * c + out[1]).to_f32();
                    let nz = ((x(a[2]) + b[2]) * c + out[2]).to_f32();
                    out = [nx, ny, nz];
                    a = b;
                }
            }
            start = i + 1;
        }
    }
    let t = area * x(3.0f64);
    let cx = (x(out[0]) / t).to_f32();
    let cy = (x(out[1]) / t).to_f32();
    let row = |j: usize| ((x(cx) * inv[0][j] + x(cy) * inv[1][j]) + zavg * inv[2][j]) + inv[3][j];
    ([row(0).to_f32(), row(1).to_f32(), row(2).to_f32()], true)
}

/// [soft 0x08448b44 si_meshTouch]: an empty function in this build.
pub fn mesh_touch(_mesh: &mut Mesh) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_b::{update, MeshEnv};

    #[test]
    fn copy_keeps_connectivity_and_drops_the_last_display_integer() {
        let mut m = Mesh::with_vertices(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        m.push_polygon(&[0, 1, 2]);
        update(&mut m, &mut MeshEnv::default());
        m.x2c = 7;
        let c = mesh_copy(&m);
        assert_eq!(c.vertices, m.vertices);
        assert_eq!(c.polygons, m.polygons);
        assert_eq!(c.edges[1..], m.edges[1..]);
        assert_eq!(c.edges[0].v0, -1);
        assert_eq!((c.flags, c.x2c, c.nb_display), (m.flags, 0, m.nb_display));
        assert_eq!(c.display.len() + 1, m.display.len());
    }

    #[test]
    fn a_move_carries_the_normals_and_only_with_flag_4() {
        let mut m = Mesh::with_vertices(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        m.push_polygon(&[0, 1, 2]);
        update(&mut m, &mut MeshEnv::default());
        assert_eq!(m.polygons[0].normal, [0.0, 0.0, 1.0]);
        // a quarter turn about X (z to −y with row vectors: y → z, z → −y), then a shift
        let turn = [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, -1.0, 0.0, 0.0], [5.0, 0.0, 0.0, 1.0]];
        let normals = [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        // corner normals of another length keep it
        m.polygons[0].nodes[1].normal = [0.0, 0.0, 2.0];
        let mut with = m.clone();
        mesh_transform(&mut with, &turn, &normals);
        assert_eq!(with.vertices[2].pos, [5.0, 0.0, 1.0]);
        assert_eq!(with.polygons[0].normal, [0.0, -1.0, 0.0]);
        assert_eq!(with.polygons[0].nodes[0].normal, [0.0, -1.0, 0.0]);
        assert_eq!(with.polygons[0].nodes[1].normal, [0.0, -2.0, 0.0]);
        // a mirror turns them round
        let mirror = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let mut flipped = m.clone();
        mesh_transform(&mut flipped, &mirror, &mirror);
        assert_eq!(flipped.polygons[0].normal, [0.0, 0.0, -1.0]);
        // without the flag the positions move and every normal stays
        let mut without = m.clone();
        without.flags &= !4;
        mesh_transform(&mut without, &turn, &normals);
        assert_eq!(without.vertices[2].pos, [5.0, 0.0, 1.0]);
        assert_eq!(without.polygons[0].normal, [0.0, 0.0, 1.0]);
        assert_eq!(without.polygons[0].nodes[1].normal, [0.0, 0.0, 2.0]);
    }

    #[test]
    fn triangles_count_holes_and_skip_hidden_polygons() {
        let mut m = Mesh::with_vertices(&[[0.0; 3]; 8]);
        m.push_polygon(&[0, 1, 2, 3]);
        m.push_polygon(&[0, 1, 2, 3, -1, 4, 5, 6, 7]);
        m.push_polygon(&[0, 1, 2]);
        m.polygons[2].flag = 0x800;
        assert_eq!(mesh_nb_triangles(&m), 2 + (9 - 2 + 1));
        mesh_delete(&mut m);
        assert_eq!((m.nb_vertices(), m.nb_polygons(), m.nb_edges()), (0, 0, 0));
    }
}
