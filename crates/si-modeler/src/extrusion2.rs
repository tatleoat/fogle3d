// SPDX-License-Identifier: MIT
//! `extrusion.c` (second slice): the meshes an extrusion makes from a face, a spline or a
//! NURBS curve, straight or along a path. Finding 0034.
//!
//! Everything works on `si-mem`'s image. A routine of the original that is not ported is
//! called through [`si_mem::Outside`], written `name (0xADDRESS)`.

use crate::extrusion::{dispose_face_at, ModelSide, SplineSide};
use si_hrc::image::spline::face_allocate;
use si_mem::{Mem, Outside};
use si_model::mesh_image::{self, g, off as mo};
use si_nurbs::image::Halt;
use si_x87::F80;

/// A float through the x87 unit (a load and a store: a signalling NaN comes out quiet).
pub(crate) fn through_x87(v: f32) -> f32 {
    F80::from_f32(v).to_f32()
}

/// One contour of the section being extruded: its point count and whether it is closed.
struct Ring {
    nb: i32,
    closed: bool,
}

/// A point carried by the 4 x 4 matrix at `mat` (a row vector times the matrix, the fourth
/// row added), in the order the original adds: `((x * m0 + y * m4) + z * m8) + m12`, each
/// step rounded in the x87 unit, the result stored as a float.
fn transform(m: &Mem, mat: u32, p: [f32; 3]) -> [f32; 3] {
    let f = |k: u32| F80::from_f32(m.f32(mat + 4 * k));
    let (x, y, z) = (F80::from_f32(p[0]), F80::from_f32(p[1]), F80::from_f32(p[2]));
    let col = |c: u32| ((x * f(c) + y * f(c + 4)) + z * f(c + 8) + f(c + 12)).to_f32();
    [col(0), col(1), col(2)]
}

/// The two cap polygons, `poly` the first: the contours' points in the reverse order with a
/// separator (-1) between contours, then (the polygon after) the points of the last row in
/// order, numbered from `second_start`. `n` nodes are `total` plus `cap_extra`.
fn write_caps(m: &mut Mem, poly: u32, rings: &[Ring], total: i32, cap_extra: i16, second_start: i32) {
    // the first cap: the contours' points in the reverse order, a separator (-1) between contours
    let n = (total as u16).wrapping_add(cap_extra as u16);
    let round = |n: u16| (n as u32).wrapping_sub((((n as u32).wrapping_sub(1) & 3).wrapping_sub(3)) as u32);
    m.set_u16(poly + 0x10, n);
    let nodes = m.si_calloc(round(n), mo::NODE);
    m.set_u32(poly + 0xc, nodes);
    let (mut sep, mut put, mut value) = (0u32, 0u32, 0i32);
    for (k, r) in rings.iter().enumerate() {
        let mut start = put;
        if k != 0 {
            start = put.wrapping_add(mo::NODE);
            m.set_i32(m.u32(poly + 0xc).wrapping_add(sep), -1);
            sep = sep.wrapping_add(mo::NODE);
        }
        put = start;
        if r.nb != 0 {
            value = r.nb.wrapping_sub(1).wrapping_add(value);
            let mut j = 0;
            while j < r.nb {
                put = put.wrapping_add(mo::NODE);
                m.set_i32(m.u32(poly + 0xc).wrapping_add(start), value);
                value = value.wrapping_sub(1);
                start = start.wrapping_add(mo::NODE);
                sep = sep.wrapping_add(mo::NODE);
                j += 1;
            }
            value = r.nb.wrapping_add(1).wrapping_add(value);
        }
    }
    // the second cap, the polygon after: the points of the last row in order
    let second = poly.wrapping_add(mo::POLYGON);
    m.set_u16(second + 0x10, n);
    let nodes = m.si_calloc(round(n), mo::NODE);
    m.set_u32(second + 0xc, nodes);
    let (mut sep, mut put, mut value) = (0u32, 0u32, second_start);
    for (k, r) in rings.iter().enumerate() {
        let mut start = put;
        if k != 0 {
            start = put.wrapping_add(mo::NODE);
            m.set_i32(m.u32(second + 0xc).wrapping_add(sep), -1);
            sep = sep.wrapping_add(mo::NODE);
        }
        put = start;
        if r.nb != 0 {
            let mut j = 0;
            while j < r.nb {
                put = put.wrapping_add(mo::NODE);
                m.set_i32(m.u32(second + 0xc).wrapping_add(start), value);
                value = value.wrapping_add(1);
                start = start.wrapping_add(mo::NODE);
                sep = sep.wrapping_add(mo::NODE);
                j += 1;
            }
        }
    }
}

/// What the two mesh builders share, after the section's points are known: the mesh `mesh`
/// (its slot at `pmesh`) is given its vertices (the section, then `steps` copies each carried by
/// the matrix from the one before), its quadrilaterals and, with `caps`, its two cap
/// polygons; the converted face (its slot at `pface`) is disposed of and the mesh rebuilt.
/// `points` are the section's points ring by ring, `total` their count as the original
/// counted it, `closed0` whether the first ring is closed, `cap_extra` what is added to the
/// count for the caps' separators.
#[allow(clippy::too_many_arguments)]
fn build(m: &mut Mem, o: &mut dyn Outside, pmesh: u32, pface: u32, rings: &[Ring], points: &[[f32; 3]], total: i32, steps: i32, mat: u32, caps: i16, closed0: bool, cap_extra: i16) -> u32 {
    let mesh = m.u32(pmesh);
    m.set_i32(mesh + mo::NB_VERTICES, steps.wrapping_add(1).wrapping_mul(total));
    let verts = si_hrc::image::geom::vertex_allocate(m, m.i32(mesh + mo::NB_VERTICES));
    m.set_u32(mesh + mo::VERTICES, verts);
    let count = steps.wrapping_mul(total);
    let mut at = verts;
    for p in points {
        m.set_f32(at, p[0]);
        m.set_f32(at + 4, p[1]);
        m.set_f32(at + 8, p[2]);
        at = at.wrapping_add(mo::VERTEX);
    }
    // the next rows: each row's points carried by the matrix from the row before (the read
    // position starts at the first row, the write position where the section ended)
    let mut from = m.u32(mesh);
    let mut left = count;
    while left != 0 {
        let p = transform(m, mat, [m.f32(from), m.f32(from + 4), m.f32(from + 8)]);
        m.set_f32(at, p[0]);
        m.set_f32(at + 4, p[1]);
        m.set_f32(at + 8, p[2]);
        at = at.wrapping_add(mo::VERTEX);
        from = from.wrapping_add(mo::VERTEX);
        left = left.wrapping_sub(1);
    }
    m.set_i32(mesh + mo::NB_POLYGONS, count);
    if !closed0 {
        m.set_i32(mesh + mo::NB_POLYGONS, m.i32(mesh + mo::NB_POLYGONS).wrapping_sub(steps));
    }
    if caps != 0 {
        m.set_i32(mesh + mo::NB_POLYGONS, m.i32(mesh + mo::NB_POLYGONS).wrapping_add(2));
    }
    if m.i32(mesh + mo::NB_POLYGONS) > m[g::SI_NB_POL_MAX] {
        mesh_image::obj::mesh_dispose(m, pmesh);
        mesh_image::face::face_dispose(m, pface);
        return 0;
    }
    let polygons = si_hrc::image::geom::polygon_allocate(m, m.i32(mesh + mo::NB_POLYGONS));
    m.set_u32(mesh + mo::POLYGONS, polygons);
    let mut poly = polygons;
    let mut s = 0i32;
    while s < steps {
        let mut off = 0i32;
        for r in rings {
            if r.nb != 0 {
                let (mut prev, mut i) = if r.closed { (r.nb.wrapping_sub(1), 0) } else { (0, 1) };
                while i < r.nb {
                    let here = s.wrapping_mul(total).wrapping_add(off);
                    let next = s.wrapping_add(1).wrapping_mul(total).wrapping_add(off);
                    mesh_image::rebuild::polygon_initialize(m, poly, &[prev.wrapping_add(here), i.wrapping_add(here), i.wrapping_add(next), prev.wrapping_add(next)]);
                    poly = poly.wrapping_add(mo::POLYGON);
                    prev = i;
                    i += 1;
                }
            }
            off = off.wrapping_add(r.nb);
        }
        s += 1;
    }
    if caps != 0 {
        write_caps(m, poly, rings, total, cap_extra, count);
    }
    mesh_image::face::face_dispose(m, pface);
    mesh_image::rebuild::update(m, &mut ModelSide(o), mesh);
    mesh_image::obj::mesh_init_vertex_colors(m, mesh);
    mesh
}

/// The mesh of a face (its outline and holes made of splines) extruded straight
/// [soft 0x08162600 meshExtrudeFace]: `steps` copies of the section, each carried by the
/// matrix from the one before, joined by quadrilaterals, with two cap polygons when `caps`
/// (dropped for an outline that is not closed). The face is made linear first
/// (`SI_faceConvertToLinear`, `step` its sampling). Answers the mesh, or 0.
pub fn mesh_extrude_face(m: &mut Mem, o: &mut dyn Outside, face: u32, step: i16, mat: u32, steps: i32, caps: i16) -> u32 {
    let sp = m.sp();
    let pmesh = m.local(4);
    let pface = m.local(4);
    let r = mesh_extrude_face_in(m, o, pmesh, pface, face, step, mat, steps, caps);
    m.set_sp(sp);
    r
}

#[allow(clippy::too_many_arguments)]
fn mesh_extrude_face_in(m: &mut Mem, o: &mut dyn Outside, pmesh: u32, pface: u32, face: u32, step: i16, mat: u32, steps: i32, caps_in: i16) -> u32 {
    let mut caps = caps_in;
    m.set_u32(pmesh, 0);
    if m.i16(face + 0xc) == 0 {
        caps = 0;
    }
    let mesh = mesh_image::mesh_allocate(m);
    m.set_u32(pmesh, mesh);
    if mesh == 0 {
        return 0;
    }
    let lin = mesh_image::face::face_convert_to_linear(m, &mut ModelSide(o), face, step, 1);
    m.set_u32(pface, lin);
    if lin == 0 {
        return 0;
    }
    let outline = m.u16(lin + 0x14) as i32;
    if outline == 0 {
        mesh_image::obj::mesh_dispose(m, pmesh);
        mesh_image::face::face_dispose(m, pface);
        return 0;
    }
    let nb_holes = m.i16(lin + 0x78);
    let mut total = outline;
    if nb_holes > 0 {
        for k in 0..nb_holes as u32 {
            total = total.wrapping_add(m.u16(m.u32(lin + 0x7c).wrapping_add(0x74 * k + 0x10)) as i32);
        }
    }
    // the contours: the outline, then the holes (none when the count is negative)
    let mut rings = Vec::new();
    let mut points = Vec::new();
    if nb_holes >= 0 {
        for k in -1..nb_holes as i32 {
            let c = if k == -1 { lin + 4 } else { m.u32(lin + 0x7c).wrapping_add(0x74 * k as u32) };
            let nb = m.u16(c + 0x10);
            if nb != 0 {
                let keys = m.u32(c + 0x14);
                for j in 0..nb as u32 {
                    let at = keys.wrapping_add(0x30 * j);
                    points.push([through_x87(m.f32(at + 0xc)), through_x87(m.f32(at + 0x10)), through_x87(m.f32(at + 0x14))]);
                }
            }
            rings.push(Ring { nb: nb as i32, closed: m.i16(c + 8) != 0 });
        }
    }
    let closed0 = m.i16(lin + 0xc) != 0;
    build(m, o, pmesh, pface, &rings, &points, total, steps, mat, caps, closed0, nb_holes)
}

/// The mesh of a face of NURBS curves extruded straight [soft 0x08162b8c meshExtrudeNurbsFace]:
/// as [`mesh_extrude_face`], the curves made linear first (their points divided by the
/// weight), the caps dropped when the first curve of the face is not closed.
pub fn mesh_extrude_nurbs_face(m: &mut Mem, o: &mut dyn Outside, face: u32, step: i16, mat: u32, steps: i32, caps_in: i16) -> u32 {
    let sp = m.sp();
    let pmesh = m.local(4);
    let pface = m.local(4);
    m.set_u32(pface, 0);
    m.set_u32(pmesh, 0);
    let mut caps = caps_in;
    if m.i16(m.u32(face + 0x80) + 0x20) == 0 {
        caps = 0;
    }
    let mesh = mesh_image::mesh_allocate(m);
    m.set_u32(pmesh, mesh);
    let mut r = 0;
    if mesh != 0 {
        let lin = mesh_image::face::face_convert_to_linear(m, &mut ModelSide(o), face, step, 1);
        m.set_u32(pface, lin);
        if lin != 0 {
            r = nurbs_in(m, o, pmesh, pface, lin, mat, steps, caps);
        }
    }
    m.set_sp(sp);
    r
}

#[allow(clippy::too_many_arguments)]
fn nurbs_in(m: &mut Mem, o: &mut dyn Outside, pmesh: u32, pface: u32, lin: u32, mat: u32, steps: i32, caps: i16) -> u32 {
    let count = m.i16(lin + 0x78);
    let mut total = 0i32;
    for k in 0..count.max(0) as u32 {
        total = total.wrapping_add(m.i32(m.u32(lin + 0x80).wrapping_add(100 * k + 4)));
    }
    if total == 0 {
        mesh_image::obj::mesh_dispose(m, pmesh);
        mesh_image::face::face_dispose(m, pface);
        return 0;
    }
    let mut rings = Vec::new();
    let mut points = Vec::new();
    for k in 0..count.max(0) as u32 {
        let c = m.u32(lin + 0x80).wrapping_add(100 * k);
        let nb = m.i32(c + 4);
        let pts = m.u32(c + 0x2c);
        for j in 0..nb.max(0) as u32 {
            let at = pts.wrapping_add(0x24 * j);
            let w = F80::from_f64(m.f64(at + 0x18));
            let q = |d: u32| (F80::from_f64(m.f64(at + d)) / w).to_f32();
            points.push([q(0), q(8), q(0x10)]);
        }
        rings.push(Ring { nb, closed: m.i16(c + 0x20) != 0 });
    }
    let closed0 = m.i16(m.u32(lin + 0x80) + 0x20) != 0;
    build(m, o, pmesh, pface, &rings, &points, total, steps, mat, caps, closed0, count.wrapping_sub(1))
}

/// A face made from a spline, extruded: `SI_faceAllocate` then `SI_splineTransfer` into its
/// outline. Answers the face, or 0 (nothing allocated).
fn face_of_spline(m: &mut Mem, spline: u32) -> u32 {
    let face = face_allocate(m).addr();
    if face != 0 {
        si_spline::intersect_image::spline_transfer(m, si_mem::Ptr32::new(spline), si_mem::Ptr32::new(face + 4));
    }
    face
}

/// The mesh of a spline extruded straight [soft 0x08162598 meshExtrudeSpline]: the spline
/// becomes the outline of a face, [`mesh_extrude_face`] extrudes it, the face is disposed of.
pub fn mesh_extrude_spline(m: &mut Mem, o: &mut dyn Outside, spline: u32, step: i16, mat: u32, steps: i32, caps: i16) -> u32 {
    let face = face_of_spline(m, spline);
    if face == 0 {
        return 0;
    }
    let r = mesh_extrude_face(m, o, face, step, mat, steps, caps);
    dispose_face_at(m, face);
    r
}

/// A spline extruded along another spline [soft 0x081630f0 SI_meshExtrudeSplineOnSpline]: the
/// spline is the outline of a face, and the face goes along `path` through
/// [`crate::effects::lin_patch::mesh_extrude_face_on_linear_spline`] (a path of kind 0) or
/// [`mesh_extrude_face_on_spline`]; the face is disposed of.
pub fn mesh_extrude_spline_on_spline(m: &mut Mem, o: &mut dyn Outside, spline: u32, a: i16, path: u32, steps: i32, caps: i16) -> u32 {
    let face = face_of_spline(m, spline);
    if face == 0 {
        return 0;
    }
    let r = if m.i32(path + 0xc) == 0 {
        crate::effects::lin_patch::mesh_extrude_face_on_linear_spline(m, o, face, a, path, steps, caps)
    } else {
        mesh_extrude_face_on_spline(m, o, face, a, path, steps, caps)
    };
    dispose_face_at(m, face);
    r
}

/// The double at 0x0864a620, below which a length is taken for zero.
const EPS: f64 = 0.0001;
/// The double at 0x0864a628: a cross product shorter than this is taken for zero.
const EPS_PARALLEL: f64 = 1e-6;

pub(crate) fn f80(v: f32) -> F80 {
    F80::from_f32(v)
}

pub(crate) fn read_matrix(m: &Mem, at: u32) -> si_math::types::Matrix {
    let mut r = [[0.0f32; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = m.f32(at.wrapping_add(16 * i as u32 + 4 * j as u32));
        }
    }
    r
}

pub(crate) fn write_matrix(m: &mut Mem, at: u32, r: &si_math::types::Matrix) {
    for (i, row) in r.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            m.set_f32(at.wrapping_add(16 * i as u32 + 4 * j as u32), *v);
        }
    }
}

/// A direction made unit length the way the original does it: `t` is the vector as the
/// register holds it (not yet rounded to float), its length is `sqrt((x² + y²) + z²)` rounded
/// to float, and `t` is divided by that length when the length is above 0.0001; otherwise `t`
/// is stored as it is. The result is the stored floats.
pub(crate) fn unit(t: [F80; 3]) -> [f32; 3] {
    let len = ((t[0] * t[0] + t[1] * t[1]) + t[2] * t[2]).sqrt().to_f32();
    if f80(len).abs().gt(F80::from_f64(EPS)) {
        let l = f80(len);
        [(t[0] / l).to_f32(), (t[1] / l).to_f32(), (t[2] / l).to_f32()]
    } else {
        [t[0].to_f32(), t[1].to_f32(), t[2].to_f32()]
    }
}

/// `a x b`, each component the exact difference of two products, kept unrounded.
pub(crate) fn cross(a: [f32; 3], b: [f32; 3]) -> [F80; 3] {
    [
        f80(a[1]) * f80(b[2]) - f80(a[2]) * f80(b[1]),
        f80(a[2]) * f80(b[0]) - f80(a[0]) * f80(b[2]),
        f80(a[0]) * f80(b[1]) - f80(a[1]) * f80(b[0]),
    ]
}

/// The sum of two stored float vectors, kept unrounded.
pub(crate) fn add(a: [f32; 3], b: [f32; 3]) -> [F80; 3] {
    [f80(a[0]) + f80(b[0]), f80(a[1]) + f80(b[1]), f80(a[2]) + f80(b[2])]
}

/// The matrix of path point `i` from the one before: the one before carried round the axis
/// `cross` (a x b, unit length) by the angle between the directions `a` and `b`, or copied
/// when the two are parallel (the squared length of the cross product not above 0.0001).
pub(crate) fn advance(m: &mut Mem, mats: u32, i: i32, a: [f32; 3], b: [f32; 3], x: [F80; 3]) {
    use si_math::{algebra, types::Vector};
    let prev = read_matrix(m, mats.wrapping_add(0x40u32.wrapping_mul(i.wrapping_sub(1) as u32)));
    let cur = if ((x[0] * x[0] + x[1] * x[1]) + x[2] * x[2]).gt(F80::from_f64(EPS)) {
        let axis = unit(x);
        let angle = algebra::vector_angle(&Vector::from_array(a), &Vector::from_array(b));
        let rot = algebra::matrix_axis_rotation(angle, &Vector::from_array(axis));
        algebra::matrix_multiply(&prev, &rot)
    } else {
        algebra::matrix_copy(&prev)
    };
    write_matrix(m, mats.wrapping_add(0x40u32.wrapping_mul(i as u32)), &cur);
}

/// A face extruded along a spline [soft 0x08165ea0 meshExtrudeFaceOnSpline]: the face is made
/// linear and so is the path (`SI_splineConvertToLinear`, cleaned up for a path of another
/// kind than 0). A 4 x 4 matrix is built for each point of the path: the first from the
/// path's direction there (a frame from the tangent and a second direction, with
/// `SI_matrixZAlign` when they are parallel), each next one the one before turned by the
/// angle between the directions on either side of the point; its last row is the point. The
/// face's points carried by each matrix are the mesh's vertices, quadrilaterals join
/// neighbouring rows (the last to the first on a closed path), and `caps` adds the two end
/// polygons (dropped for a closed path or an open outline). Answers the mesh, or 0.
pub fn mesh_extrude_face_on_spline(m: &mut Mem, o: &mut dyn Outside, face: u32, step: i16, path: u32, steps: i32, caps_in: i16) -> u32 {
    let sp = m.sp();
    let pface = m.local(4);
    let pspline = m.local(4);
    let r = on_spline(m, o, pface, pspline, face, step, path, steps, caps_in);
    m.set_sp(sp);
    r
}

#[allow(clippy::too_many_arguments)]
fn on_spline(m: &mut Mem, o: &mut dyn Outside, pface: u32, pspline: u32, face: u32, step: i16, path: u32, steps: i32, caps_in: i16) -> u32 {
    use si_mem::Ptr32;
    use si_math::{algebra, types::Vector};
    let mut caps = caps_in;
    if m.i16(path + 8) != 0 || m.i16(face + 0xc) == 0 {
        caps = 0;
    }
    let lin = mesh_image::face::face_convert_to_linear(m, &mut ModelSide(o), face, step, 1);
    m.set_u32(pface, lin);
    if lin == 0 {
        return 0;
    }
    let c8 = m.u16(lin + 0x14) as i32;
    let nbh = m.i16(lin + 0x78);
    let path_closed = m.i16(path + 8) as i32;
    let mut total = c8;
    if nbh > 0 {
        for k in 0..nbh as u32 {
            total = total.wrapping_add(m.u16(m.u32(lin + 0x7c).wrapping_add(0x74 * k + 0x10)) as i32);
        }
    }
    let c8 = total;
    let keys_n = m.u16(path + 0x10) as i32;
    let mut expect = keys_n.wrapping_sub(1).wrapping_add(path_closed);
    if m.i16(path + 8) == 0 && m.i32(path + 0xc).wrapping_sub(2) as u32 <= 1 {
        expect = expect.wrapping_sub(2);
    }
    let mut expect = path_closed.wrapping_add(expect.wrapping_mul(steps)).wrapping_mul(c8);
    if m.i16(lin + 0xc) == 0 {
        expect = expect.wrapping_sub(path_closed.wrapping_sub(1).wrapping_add(keys_n));
    }
    if caps != 0 {
        expect = expect.wrapping_add(2);
    }
    if m[g::SI_NB_POL_MAX] < expect {
        mesh_image::face::face_dispose(m, pface);
        return 0;
    }
    let a4 = si_spline::convert_image::spline_convert_to_linear(m, &mut SplineSide(o), Ptr32::new(path), 1, steps, 0.0).addr();
    m.set_u32(pspline, a4);
    if a4 == 0 {
        return 0;
    }
    if m.i32(path + 0xc) != 0 {
        si_spline::convert_image::spline_cleanup(m, &mut SplineSide(o), Ptr32::new(a4));
    }
    let n = m.u16(a4 + 0x10) as i32;
    let mats = m.si_calloc(n as u32, 0x40);
    let closed = m.i16(a4 + 8) != 0;
    let kb = m.u32(a4 + 0x14);
    let key = |i: i32| kb.wrapping_add((i as u32).wrapping_mul(0x30)).wrapping_add(0xc);
    // the difference of two key positions, unrounded
    let diff = |m: &Mem, p: u32, q: u32| -> [F80; 3] { [0u32, 4, 8].map(|d| f80(m.f32(p.wrapping_add(d))) - f80(m.f32(q.wrapping_add(d)))) };

    // the first frame: a = the direction along the path there, f = a second direction in the plane
    let (a, f) = if !closed {
        let f = unit(diff(m, key(3), key(0)));
        let a = unit(diff(m, key(1), key(0)));
        (a, f)
    } else {
        let e = unit(diff(m, key(0), key(n - 1)));
        let f = unit(diff(m, key(1), key(0)));
        let a = unit(add(f, e));
        (a, f)
    };
    let c = unit(cross(a, f));
    let len = ((f80(c[1]) * f80(c[1]) + f80(c[0]) * f80(c[0])) + f80(c[2]) * f80(c[2])).sqrt();
    let e6 = F80::from_f64(EPS_PARALLEL);
    let frame = if len.gt(e6) || len.unordered(e6) {
        let d = unit(cross(a, c));
        [[d[0], c[0], a[0], 0.0], [d[1], c[1], a[1], 0.0], [d[2], c[2], a[2], 0.0], [0.0, 0.0, 0.0, 1.0]]
    } else {
        algebra::matrix_z_align(&Vector::from_array(a))
    };
    write_matrix(m, mats, &algebra::matrix_transpose(&frame));
    let mut a = a;
    let mut i = 1;
    while i < n - 1 {
        let e = unit(diff(m, key(i), key(i - 1)));
        let f = unit(diff(m, key(i + 1), key(i)));
        let b = unit(add(f, e));
        advance(m, mats, i, a, b, cross(a, b));
        a = b;
        i += 1;
    }
    let b = if !closed {
        unit(diff(m, key(i), key(i - 1)))
    } else {
        let e = unit(diff(m, key(i), key(i - 1)));
        let f = unit(diff(m, key(0), key(i)));
        unit(add(f, e))
    };
    advance(m, mats, i, a, b, cross(a, b));
    for k in 0..n {
        for d in 0..3u32 {
            m.set_f32(mats.wrapping_add(0x40 * k as u32 + 0x30 + 4 * d), through_x87(m.f32(key(k) + 4 * d)));
        }
    }

    // the contours of the face
    let mut rings = Vec::new();
    let mut points = Vec::new();
    if nbh >= 0 {
        for k in -1..nbh as i32 {
            let c = if k == -1 { lin + 4 } else { m.u32(lin + 0x7c).wrapping_add(0x74 * k as u32) };
            let nb = m.u16(c + 0x10);
            if nb != 0 {
                let ks = m.u32(c + 0x14);
                for j in 0..nb as u32 {
                    let at = ks.wrapping_add(0x30 * j);
                    points.push([m.f32(at + 0xc), m.f32(at + 0x10), m.f32(at + 0x14)]);
                }
            }
            rings.push(Ring { nb: nb as i32, closed: m.i16(c + 8) != 0 });
        }
    }

    let mesh = mesh_image::mesh_allocate(m);
    m.set_i32(mesh + mo::NB_VERTICES, (n as u32).wrapping_mul(c8 as u32) as i32);
    let verts = si_hrc::image::geom::vertex_allocate(m, m.i32(mesh + mo::NB_VERTICES));
    m.set_u32(mesh + mo::VERTICES, verts);
    let mut at = verts;
    for k in 0..n {
        let mat = mats.wrapping_add(0x40 * k as u32);
        for p in &points {
            let q = transform(m, mat, *p);
            m.set_f32(at, q[0]);
            m.set_f32(at + 4, q[1]);
            m.set_f32(at + 8, q[2]);
            at = at.wrapping_add(mo::VERTEX);
        }
    }
    let segs = (n - 1).wrapping_add(path_closed);
    let mut count = segs.wrapping_mul(c8);
    m.set_i32(mesh + mo::NB_POLYGONS, count);
    if m.i16(lin + 0xc) == 0 {
        count = count.wrapping_sub(segs);
        m.set_i32(mesh + mo::NB_POLYGONS, count);
    }
    if caps != 0 {
        count = count.wrapping_add(2);
        m.set_i32(mesh + mo::NB_POLYGONS, count);
    }
    let polygons = si_hrc::image::geom::polygon_allocate(m, count);
    m.set_u32(mesh + mo::POLYGONS, polygons);
    let mut poly = polygons;
    // the quadrilaterals between row `from` and row `to` of the vertices
    let quads = |m: &mut Mem, poly: &mut u32, from: i32, to: i32| {
        let mut off = 0i32;
        for r in &rings {
            if r.nb != 0 {
                let (mut prev, mut i) = if r.closed { (r.nb.wrapping_sub(1), 0) } else { (0, 1) };
                while i < r.nb {
                    let here = from.wrapping_mul(c8).wrapping_add(off);
                    let next = to.wrapping_mul(c8).wrapping_add(off);
                    mesh_image::rebuild::polygon_initialize(m, *poly, &[prev.wrapping_add(here), i.wrapping_add(here), i.wrapping_add(next), prev.wrapping_add(next)]);
                    *poly = poly.wrapping_add(mo::POLYGON);
                    prev = i;
                    i += 1;
                }
                off = off.wrapping_add(r.nb);
            }
        }
    };
    let mut row = 0;
    while row < n - 1 {
        quads(m, &mut poly, row, row + 1);
        row += 1;
    }
    if path_closed != 0 {
        quads(m, &mut poly, row, 0);
    }
    if caps != 0 {
        write_caps(m, poly, &rings, c8, nbh, (n - 1).wrapping_mul(c8));
    }
    mesh_image::rebuild::update(m, &mut ModelSide(o), mesh);
    mesh_image::obj::mesh_init_vertex_colors(m, mesh);
    mesh_image::face::face_dispose(m, pface);
    si_spline::intersect_image::spline_dispose(m, pspline);
    mesh
}

/// A NURBS curve extruded along another path [soft 0x08163188 SI_meshExtrudeNurbsOnNurbs]: the
/// curve `curve` is sampled at its knots (`SI_SampleKnotVector`, `step` samples per span read
/// from its +0x34) into the outline of a face; the path is the spline (kind 6) or NURBS curve
/// (kind 9) of `model`, copied and carried by the model's matrix, a curve made linear into a
/// spline first (with `steps` taken as its sampling and the extrusion made in one step); the
/// face goes along it through `meshExtrudeFaceOnLinearSpline (0x08163914)` (a spline of kind
/// 0) or [`mesh_extrude_face_on_spline`]. A model that is absent or of another kind makes the
/// original read through a null spline: a [`Halt`].
pub fn mesh_extrude_nurbs_on_nurbs(m: &mut Mem, o: &mut dyn Outside, curve: u32, step: i16, model: u32, steps_in: i32, caps: i16) -> Result<u32, Halt> {
    let sp = m.sp();
    let r = nurbs_on_nurbs(m, o, curve, step, model, steps_in, caps);
    m.set_sp(sp);
    r
}

fn nurbs_on_nurbs(m: &mut Mem, o: &mut dyn Outside, curve: u32, step: i16, model: u32, steps_in: i32, caps: i16) -> Result<u32, Halt> {
    let mut steps = steps_in;
    let pspline = m.local(4);
    let psamples = m.local(4);
    let pcount = m.local(4);
    let pface = m.local(4);
    let pnurbs = m.local(4);
    let plinear = m.local(4);
    for slot in [pspline, psamples, pface, pnurbs, plinear] {
        m.set_u32(slot, 0);
    }
    m.set_i32(pcount, 0);
    si_nurbs::image::sample_knot_vector(m, curve, m.i32(curve + 0x34), psamples, pcount)?;
    let face = face_allocate(m).addr();
    m.set_u32(pface, face);
    let count = m.i32(pcount);
    let keys = si_hrc::image::spline::spline_key_allocate(m, count);
    m.set_u32(face + 0x18, keys);
    let closed = m.i16(curve + 0x20);
    m.set_i16(face + 0xc, closed);
    m.set_u16(face + 0x14, if closed == 0 { count as u16 } else { (count as u16).wrapping_sub(1) });
    for k in 0..m.u16(face + 0x14) as u32 {
        let t = m.f64(m.u32(psamples).wrapping_add(8 * k));
        let v = si_nurbs::image::crv_value(m, curve, t, 0)?;
        let at = m.u32(face + 0x18).wrapping_add(0x30 * k);
        for (d, c) in v.iter().enumerate() {
            m.set_f32(at + 0xc + 4 * d as u32, *c as f32);
        }
    }
    if take_path(m, o, model, &mut steps, 0, [pspline, pnurbs, plinear])? {
        return Ok(0);
    }
    finish(m, o, [pspline, pface, pnurbs, plinear], false, step, steps, caps)
}

/// The path a model gives (the part both curve extrusions share): a spline of kind 6 copied and
/// carried by the model's matrix, or a NURBS curve of kind 9 copied, carried, and made linear
/// (its sampling `*steps`, which becomes 1) into a spline of kind `linear_kind`. Answers
/// whether the extrusion ends here with no mesh (a path with no points). `slots` are the
/// spline, curve and linear-curve slots on the stack.
fn take_path(m: &mut Mem, o: &mut dyn Outside, model: u32, steps: &mut i32, linear_kind: i32, slots: [u32; 3]) -> Result<bool, Halt> {
    let [pspline, pnurbs, plinear] = slots;
    if model != 0 {
        match m.i32(model + 0x30) {
            6 => {
                let s = si_hrc::image::spline::spline_copy(m, m.u32(model + 0x48)).addr();
                m.set_u32(pspline, s);
                si_spline::convert_image::spline_multiply_definition(m, &mut SplineSide(o), si_mem::Ptr32::new(s), model + 0x1c4);
                if m.u16(s + 0x10) == 0 {
                    si_spline::intersect_image::spline_dispose(m, pspline);
                    return Ok(true);
                }
            }
            9 => {
                let c = si_nurbs::image::crv_copy(m, m.u32(model + 0x48));
                m.set_u32(pnurbs, c);
                si_nurbs::image::crv_apply_matrix(m, c, model + 0x1c4, 0);
                if m.i32(c + 4) == 0 {
                    si_hrc::image::nurbs::nurbs_crv_dispose(m, pnurbs);
                    return Ok(true);
                }
                m.set_i32(c + 0x34, *steps);
                *steps = 1;
                let lin = si_nurbs::image::crv_convert_to_linear(m, c)?;
                m.set_u32(plinear, lin);
                let s = si_hrc::image::spline::spline_allocate(m).addr();
                m.set_u32(pspline, s);
                m.set_u16(s + 0x10, m.u16(lin + 4));
                let ks = si_hrc::image::spline::spline_key_allocate(m, m.u16(s + 0x10) as i32);
                m.set_u32(s + 0x14, ks);
                m.set_i32(s + 0xc, linear_kind);
                m.set_i16(s + 8, m.i16(lin + 0x20));
                for k in 0..m.u16(s + 0x10) as u32 {
                    let p = m.u32(lin + 0x2c).wrapping_add(0x24 * k);
                    let at = m.u32(s + 0x14).wrapping_add(0x30 * k);
                    for d in 0..3u32 {
                        m.set_f32(at + 0xc + 4 * d, m.f64(p + 8 * d) as f32);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(false)
}

/// The end both curve extrusions share: the face along the path
/// ([`crate::effects::lin_patch::mesh_extrude_face_on_linear_spline`] for a spline of kind 0, else
/// [`mesh_extrude_face_on_spline`]), then the path,
/// the face and the curves are disposed of. A missing path is a [`Halt`] (the original reads
/// through null). `slots`: spline, face, curve, linear curve; the two curves are disposed of in the
/// order `linear_first` says.
fn finish(m: &mut Mem, o: &mut dyn Outside, slots: [u32; 4], linear_first: bool, step: i16, steps: i32, caps: i16) -> Result<u32, Halt> {
    let [pspline, pface, pnurbs, plinear] = slots;
    let (spline, face) = (m.u32(pspline), m.u32(pface));
    if spline == 0 {
        return Err(Halt::Refused("extrusion on a curve: no path"));
    }
    let r = if m.i32(spline + 0xc) == 0 {
        crate::effects::lin_patch::mesh_extrude_face_on_linear_spline(m, o, face, step, spline, steps, caps)
    } else {
        mesh_extrude_face_on_spline(m, o, face, step, spline, steps, caps)
    };
    si_spline::intersect_image::spline_dispose(m, pspline);
    mesh_image::face::face_dispose(m, pface);
    let order = if linear_first { [plinear, pnurbs] } else { [pnurbs, plinear] };
    for slot in order {
        si_hrc::image::nurbs::nurbs_crv_dispose(m, slot);
    }
    Ok(r)
}

/// A NURBS face extruded along the path of a model [soft 0x08163490 meshExtrudeNurbsFaceOnCurve]:
/// as [`mesh_extrude_nurbs_on_nurbs`], but the face is the face's own curves sampled at their
/// knots (the first the outline, the others holes), and a NURBS path becomes a spline of kind
/// 2, so the extrusion always goes through [`mesh_extrude_face_on_spline`].
pub fn mesh_extrude_nurbs_face_on_curve(m: &mut Mem, o: &mut dyn Outside, face: u32, step: i16, model: u32, steps_in: i32, caps: i16) -> Result<u32, Halt> {
    let sp = m.sp();
    let r = nurbs_face_on_curve(m, o, face, step, model, steps_in, caps);
    m.set_sp(sp);
    r
}

fn nurbs_face_on_curve(m: &mut Mem, o: &mut dyn Outside, face: u32, step: i16, model: u32, steps_in: i32, caps: i16) -> Result<u32, Halt> {
    let mut steps = steps_in;
    let pspline = m.local(4);
    let psamples = m.local(4);
    let pcount = m.local(4);
    let pface = m.local(4);
    let pnurbs = m.local(4);
    let plinear = m.local(4);
    for slot in [pspline, psamples, pface, pnurbs, plinear] {
        m.set_u32(slot, 0);
    }
    m.set_i32(pcount, 0);
    // the curve's samples into a key block: `ks` the block, `closed_at` and `nb_at` where the closed word and key count go
    let sample = |m: &mut Mem, c: u32, ks: u32, closed_at: u32, nb_at: u32| -> Result<(), Halt> {
        let count = m.i32(pcount);
        let closed = m.i16(c + 0x20);
        m.set_i16(closed_at, closed);
        m.set_u16(nb_at, if closed == 0 { count as u16 } else { (count as u16).wrapping_sub(1) });
        for k in 0..m.u16(nb_at) as u32 {
            let t = m.f64(m.u32(psamples).wrapping_add(8 * k));
            let v = si_nurbs::image::crv_value(m, c, t, 0)?;
            for (d, x) in v.iter().enumerate() {
                m.set_f32(ks.wrapping_add(0x30 * k).wrapping_add(0xc + 4 * d as u32), *x as f32);
            }
        }
        Ok(())
    };
    let nb = m.i16(face + 0x78);
    let mut k = 0i32;
    while k < nb as i32 {
        let c = m.u32(face + 0x80).wrapping_add(100u32.wrapping_mul(k as u32));
        si_nurbs::image::sample_knot_vector(m, c, m.i32(c + 0x34), psamples, pcount)?;
        let out = m.u32(pface);
        if k == 0 {
            let f = face_allocate(m).addr();
            m.set_u32(pface, f);
            let holes = m.si_calloc((nb as i32 - 1) as u32, 0x74);
            m.set_u32(f + 0x7c, holes);
            m.set_i16(f + 0x78, nb.wrapping_sub(1));
            let ks = si_hrc::image::spline::spline_key_allocate(m, m.i32(pcount));
            m.set_u32(f + 0x18, ks);
            sample(m, c, ks, f + 0xc, f + 0x14)?;
        } else {
            let hole = m.u32(out + 0x7c).wrapping_add(0x74u32.wrapping_mul(k as u32 - 1));
            let ks = si_hrc::image::spline::spline_key_allocate(m, m.i32(pcount));
            m.set_u32(hole + 0x14, ks);
            sample(m, c, ks, hole + 8, hole + 0x10)?;
        }
        k += 1;
        let a = m.u32(psamples);
        if a != 0 {
            m.free(a);
            m.set_u32(psamples, 0);
        }
    }
    if take_path(m, o, model, &mut steps, 2, [pspline, pnurbs, plinear])? {
        return Ok(0);
    }
    finish(m, o, [pspline, pface, pnurbs, plinear], true, step, steps, caps)
}
