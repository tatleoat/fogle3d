// SPDX-License-Identifier: MIT
//! `revolve.c`: the polygonal revolution of a face or a spline, the patches of a revolved face and
//! the model a revolution makes. Finding 0034, "geomupd4".
//!
//! Everything works on `si-mem`'s image. A routine of the original that is not ported is called
//! through [`si_mem::Outside`], written `name (0xADDRESS)`.

use crate::effects::{SI_STATUS_DISPLAY_MESSAGE, SLEEP};
use crate::extrusion::SplineSide;
use si_math::types::Vector;
use si_mem::{Mem, Outside, Ptr32, Routine};
use si_model::mesh_image::{self, g, off as mo};
use si_x87::F80;

/// `SI_faceCopy (0x08434990)`, `face.c`.
pub const SI_FACE_COPY: Routine = Routine::new("SI_faceCopy", 0x0843_4990);
/// `SI_modelDisposeNode (0x08461a54)`, `model.c`.
pub const SI_MODEL_DISPOSE_NODE: Routine = Routine::new("SI_modelDisposeNode", 0x0846_1a54);

/// The tolerance of the on-axis tests: the **double** 0.0001 at 0x0864b738.
const EPS: f64 = 1e-4;

fn lt(v: F80) -> bool {
    v.abs().lt(F80::from_f64(EPS))
}

/// `|v| <= eps`, true for a NaN (the original's flag test lets the unordered case through).
fn le(v: F80) -> bool {
    !v.abs().gt(F80::from_f64(EPS))
}

fn f(v: f32) -> F80 {
    F80::from_f32(v)
}

fn vec3(p: [f32; 3]) -> Vector {
    Vector { x: p[0], y: p[1], z: p[2] }
}

/// The two points of the axis spline `SI_pointsColinearDelta` is asked about (axis kinds 3 and 4):
/// the first and second keys, or the second and third when the spline is closed and of kind 2 or 3.
#[derive(Clone, Copy)]
struct AxisPts {
    a: [f32; 3],
    b: [f32; 3],
}

/// What an on-axis test needs: the axis kind and, for 3 and 4, the points.
#[derive(Clone, Copy)]
struct Axis {
    kind: i32,
    pts: Option<AxisPts>,
}

impl Axis {
    /// The test on a point in floats (kinds 0, 1, 2: the two coordinates off the axis are
    /// within the tolerance, the first strictly; 3 and 4: the point is on the line of the axis spline).
    fn on_f32(&self, p: [f32; 3]) -> bool {
        match self.kind {
            0 => lt(f(p[1])) && le(f(p[2])),
            1 => lt(f(p[0])) && le(f(p[2])),
            2 => lt(f(p[0])) && le(f(p[1])),
            3 | 4 => {
                let a = self.pts.expect("axis spline");
                si_math::algebra::points_colinear_delta(&vec3(a.a), &vec3(a.b), &vec3(p))
            }
            // the original leaves the flag as the stack held it; false here
            _ => false,
        }
    }
}

/// One point of a contour: its coordinates as floats (a NURBS point divided by its weight in the
/// x87 unit and stored as a float), and for a NURBS curve the raw doubles x, y, z, w.
struct Pt {
    f: [f32; 3],
    raw: Option<[f64; 4]>,
}

struct Cont {
    nb: i32,
    closed: i16,
    pts: Vec<Pt>,
}

/// The contours of a converted face in the order the original walks them: for a face of NURBS
/// curves (+0x84 = 0) each curve (100 bytes, count at +4, points at +0x2c, closed at +0x20);
/// for a face of splines the outline (+4) then the holes (0x74 each at +0x7c), none at all when
/// the hole count (+0x78) is negative.
fn contours(m: &Mem, face: u32) -> (bool, Vec<Cont>) {
    let mut out = Vec::new();
    let count = m.i16(face + 0x78);
    if m.i16(face + 0x84) == 0 {
        for k in 0..count.max(0) as u32 {
            let c = m.u32(face + 0x80).wrapping_add(100 * k);
            let nb = m.i32(c + 4);
            let mut pts = Vec::new();
            for j in 0..nb.max(0) as u32 {
                let at = m.u32(c + 0x2c).wrapping_add(0x24 * j);
                let raw = [m.f64(at), m.f64(at + 8), m.f64(at + 0x10), m.f64(at + 0x18)];
                let w = F80::from_f64(raw[3]);
                let q = |d: f64| (F80::from_f64(d) / w).to_f32();
                pts.push(Pt { f: [q(raw[0]), q(raw[1]), q(raw[2])], raw: Some(raw) });
            }
            out.push(Cont { nb, closed: m.i16(c + 0x20), pts });
        }
        (true, out)
    } else {
        if count >= 0 {
            for k in -1..count as i32 {
                let c = if k == -1 { face + 4 } else { m.u32(face + 0x7c).wrapping_add(0x74 * k as u32) };
                let nb = m.u16(c + 0x10) as i32;
                let mut pts = Vec::new();
                for j in 0..nb as u32 {
                    let at = m.u32(c + 0x14).wrapping_add(0x30 * j);
                    pts.push(Pt { f: [m.f32(at + 0xc), m.f32(at + 0x10), m.f32(at + 0x14)], raw: None });
                }
                out.push(Cont { nb, closed: m.i16(c + 8), pts });
            }
        }
        (false, out)
    }
}

/// A point of the axis spline's keys (the key records are 0x30 bytes, the position at +0xc).
fn key3(m: &Mem, keys: u32, i: u32) -> [f32; 3] {
    let at = keys.wrapping_add(0x30 * i).wrapping_add(0xc);
    [m.f32(at), m.f32(at + 4), m.f32(at + 8)]
}

/// The point `0.. ` of a NURBS curve divided by its weight, the reciprocal of the weight formed
/// first and stored as a float: `(float)x * (1.0f / (float)w)`.
fn recip_pt(m: &Mem, at: u32) -> [f32; 3] {
    let w = m.f64(at + 0x18) as f32;
    let r = (F80::ONE / f(w)).to_f32();
    let g = |d: u32| (f(m.f64(at + d) as f32) * f(r)).to_f32();
    [g(0), g(8), g(0x10)]
}

/// The row-vector product of the original's expression `z*m8 + y*m4 + x*m0 + m12` and its
/// siblings: each sum in the x87 unit in the order given by `order`, stored as a float.
fn xform(mat: &[[f32; 4]; 4], p: [f32; 3], spline: bool) -> [f32; 3] {
    let (x, y, z) = (f(p[0]), f(p[1]), f(p[2]));
    let c = |r: usize, k: usize| f(mat[r][k]);
    // column 0: the face of curves adds z, y, x; the face of splines z, x, y
    let v0 = if spline { ((z * c(2, 0) + x * c(0, 0)) + y * c(1, 0)) + c(3, 0) } else { ((z * c(2, 0) + y * c(1, 0)) + x * c(0, 0)) + c(3, 0) };
    let v1 = ((z * c(2, 1) + x * c(0, 1)) + y * c(1, 1)) + c(3, 1);
    let v2 = ((z * c(2, 2) + x * c(0, 2)) + y * c(1, 2)) + c(3, 2);
    [v0.to_f32(), v1.to_f32(), v2.to_f32()]
}

fn write_mat(m: &mut Mem, at: u32, mat: &[[f32; 4]; 4]) {
    for i in 0..4 {
        for j in 0..4 {
            m.set_f32(at + 16 * i as u32 + 4 * j as u32, mat[i][j]);
        }
    }
}

fn read_mat(m: &Mem, at: u32) -> [[f32; 4]; 4] {
    let mut r = [[0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            r[i][j] = m.f32(at + 16 * i as u32 + 4 * j as u32);
        }
    }
    r
}

/// The mesh of a face revolved about an axis [soft 0x08198b64 meshRevolveFace]. `face` is a face
/// of splines or of NURBS curves, `step` the sampling for its conversion to straight pieces,
/// `steps` the number of segments, `angle` the sweep in radians, `axis` 0, 1, 2 for X, Y, Z
/// through the origin, 3 and 4 for the line of the spline `model` holds (kind 6; a NURBS curve,
/// kind 9, gets a status message, `sleep(1)` and no mesh), `caps` asks for the two end polygons
/// (given up for a closed profile, for a sweep of more than a turn, and when the profile
/// lies on the axis). The face is made linear first, points on the axis are shared between
/// rings, the edges between two points on the axis give no polygon. Answers the mesh, or 0.
///
/// The spline copy made for the axis model is never freed (the original leaks it). Where the
/// original would read through a null spline (axis kind 3 or 4 without a model of kind 6) the
/// image faults.
pub fn mesh_revolve_face(m: &mut Mem, o: &mut dyn Outside, face: u32, step: i16, steps: i32, angle_in: f32, axis_kind: i32, model: u32, caps_in: i16) -> u32 {
    let sp = m.sp();
    let r = revolve_in(m, o, face, step, steps, angle_in, axis_kind, model, caps_in);
    m.set_sp(sp);
    r
}

#[allow(clippy::too_many_arguments)]
fn revolve_in(m: &mut Mem, o: &mut dyn Outside, face: u32, step: i16, steps: i32, angle_in: f32, axis_kind: i32, model: u32, caps_in: i16) -> u32 {
    let mut caps = caps_in;
    let is_curves = m.i16(face + 0x84) == 0;
    // the first curve (curves) or the outline (splines): its closed flag
    let first_curve = if is_curves { m.u32(face + 0x80) } else { 0 };
    let outline = face + 4;
    let closed0: i16 = if is_curves { m.i16(first_curve + 0x20) } else { m.i16(face + 0xc) };
    let mut axis_spline = 0u32;
    // the profile's first, second, third and last points, as the prologue forms them
    let (mut p0, mut p1, mut p2, mut pl) = ([0f32; 3], [0f32; 3], [0f32; 3], [0f32; 3]);
    let mut b9 = true;
    if model != 0 {
        if m.i32(model + 0x30) == 6 {
            let def = m.u32(model + 0x48);
            axis_spline = si_spline::intersect_image::spline_copy(m, Ptr32::new(def)).addr();
            si_spline::convert_image::spline_multiply_definition(m, &mut SplineSide(o), Ptr32::new(axis_spline), model + 0x1c4);
            if is_curves {
                let pts = m.u32(first_curve + 0x2c);
                p0 = recip_pt(m, pts);
                p1 = recip_pt(m, pts.wrapping_add(0x24));
                p2 = recip_pt(m, pts.wrapping_add(0x48));
                let n = m.i32(first_curve + 4);
                pl = recip_pt(m, m.u32(first_curve + 0x2c).wrapping_add(n.wrapping_mul(0x24) as u32).wrapping_sub(0x24));
            } else {
                let keys = m.u32(outline + 0x14);
                p0 = key3(m, keys, 0);
                p1 = key3(m, keys, 1);
                p2 = key3(m, keys, 2);
                pl = key3(m, keys, (m.u16(outline + 0x10) as u32).wrapping_sub(1));
            }
        } else if m.i32(model + 0x30) == 9 {
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x0868_dbe0, 3]);
            o.call(m, SLEEP, &[1]);
            return 0;
        }
    }
    if axis_spline != 0 && matches!(m.i32(axis_spline + 0xc), 2 | 3) && m.i16(axis_spline + 6) != 0 {
        b9 = false;
    }
    let axis_pts = if axis_spline != 0 && (axis_kind == 3 || axis_kind == 4) {
        let keys = m.u32(axis_spline + 0x14);
        Some(if b9 { AxisPts { a: key3(m, keys, 0), b: key3(m, keys, 1) } } else { AxisPts { a: key3(m, keys, 1), b: key3(m, keys, 2) } })
    } else if axis_kind == 3 || axis_kind == 4 {
        // the original reads the keys of a null spline: address 0x14 and up
        let keys = m.u32(0x14);
        Some(if b9 { AxisPts { a: key3(m, keys, 0), b: key3(m, keys, 1) } } else { AxisPts { a: key3(m, keys, 1), b: key3(m, keys, 2) } })
    } else {
        None
    };
    let ax = Axis { kind: axis_kind, pts: axis_pts };
    // the profile on the axis: no caps
    if closed0 == 0 {
        let on_axis = match axis_kind {
            0 => lt(f(p0[1])) && le(f(p0[2])) && le(f(pl[1])) && le(f(pl[2])),
            1 => lt(f(p0[0])) && le(f(p0[2])) && le(f(pl[0])) && le(f(pl[2])),
            2 => lt(f(p0[0])) && le(f(p0[1])) && le(f(pl[0])) && le(f(pl[1])),
            3 | 4 => {
                let pcd = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| si_math::algebra::points_colinear_delta(&vec3(a), &vec3(b), &vec3(c));
                let pa = axis_pts.expect("axis");
                let face_k0 = if is_curves { [0f32; 3] } else { key3(m, m.u32(outline + 0x14), 0) };
                let face_last = if is_curves { [0f32; 3] } else { key3(m, m.u32(outline + 0x14), (m.u16(outline + 0x10) as u32).wrapping_sub(1)) };
                if b9 {
                    (is_curves || pcd(p0, p1, face_k0)) && pcd(p0, pa.b, pl)
                } else {
                    (is_curves || pcd(p1, p2, face_k0)) && pcd(p1, p2, face_last)
                }
            }
            _ => true,
        };
        if !(on_axis) {
            caps = 0;
        }
    }
    // the sweep: more than (nearly) a turn is a full turn and closes on itself
    let mut angle = angle_in;
    let mut extra = 1i32;
    if F80::from_f64(6.2830853071795865).lt(f(angle).abs()) {
        angle = if angle < 0.0 { -6.2831855f32 } else { 6.2831855f32 };
        extra = 0;
        caps = 0;
    }
    angle = (f(angle) / F80::from_i32(steps)).to_f32();
    let pmesh = m.local(4);
    let mesh = mesh_image::mesh_allocate(m);
    m.set_u32(pmesh, mesh);
    if mesh == 0 {
        return 0;
    }
    let pface = m.local(4);
    let lin = mesh_image::face::face_convert_to_linear(m, &mut crate::extrusion::ModelSide(o), face, step, 1);
    m.set_u32(pface, lin);
    if lin == 0 {
        return 0;
    }
    let (_, conts) = contours(m, lin);
    let curves = m.i16(lin + 0x84) == 0;
    let hole_count = m.i16(lin + 0x78);
    // the points: the total, and how many lie on the axis (counted on the forms the original tests)
    let mut total = 0i32;
    let mut on_axis_count = 0i32;
    for c in &conts {
        total = total.wrapping_add(c.nb);
        for p in &c.pts {
            let hit = match (&p.raw, axis_kind) {
                (Some(r), 0) => lt(F80::from_f64(r[1]) / F80::from_f64(r[3])) && le(F80::from_f64((F80::from_f64(r[2]) / F80::from_f64(r[3])).to_f64())),
                (Some(r), 1) => lt(F80::from_f64(r[0]) / F80::from_f64(r[3])) && le(F80::from_f64((F80::from_f64(r[2]) / F80::from_f64(r[3])).to_f64())),
                (Some(r), 2) => lt(F80::from_f64(r[0]) / F80::from_f64(r[3])) && le(F80::from_f64((F80::from_f64(r[1]) / F80::from_f64(r[3])).to_f64())),
                _ => ax.on_f32(p.f),
            };
            if hit {
                on_axis_count += 1;
            }
        }
    }
    let rings = steps.wrapping_add(extra);
    let nverts = if closed0 == 0 { total.wrapping_sub(on_axis_count).wrapping_mul(rings).wrapping_add(on_axis_count) } else { rings.wrapping_mul(total) };
    m.set_i32(mesh + mo::NB_VERTICES, nverts);
    let verts = si_hrc::image::geom::vertex_allocate(m, nverts);
    m.set_u32(mesh + mo::VERTICES, verts);
    let idx = m.si_calloc((steps.wrapping_add(extra).wrapping_mul(total)) as u32, 4);
    let mut at = verts;
    let (mut e0, mut e4) = (0i32, 0i32);
    for c in &conts {
        for p in &c.pts {
            m.set_f32(at, p.f[0]);
            m.set_f32(at + 4, p.f[1]);
            m.set_f32(at + 8, p.f[2]);
            m.set_i32(idx.wrapping_add((e0 as u32).wrapping_mul(4)), e4);
            e0 = e0.wrapping_add(1);
            e4 = e4.wrapping_add(1);
            at = at.wrapping_add(mo::VERTEX);
        }
    }
    // the other rings
    let matp = m.local(64);
    let mut d4 = 1i32;
    while d4 < rings {
        let a = (f(angle) * F80::from_i32(d4)).to_f32();
        let mat = match axis_kind {
            0 => si_math::algebra::matrix_x_rotation(a),
            1 => si_math::algebra::matrix_y_rotation(a),
            2 => si_math::algebra::matrix_z_rotation(a),
            3 | 4 => {
                let k = axis_spline;
                let keys = m.u32(k + 0x14);
                let (q0, q1, q2) = (key3(m, keys, 0), key3(m, keys, 1), key3(m, keys, 2));
                let (s, e) = if b9 { (q0, q1) } else { (q1, q2) };
                let mut v = [(f(e[0]) - f(s[0])).to_f32(), (f(e[1]) - f(s[1])).to_f32(), (f(e[2]) - f(s[2])).to_f32()];
                let len = ((f(v[2]) * f(v[2]) + f(v[0]) * f(v[0])) + f(v[1]) * f(v[1])).sqrt().to_f32();
                if F80::from_f64(EPS).lt(f(len).abs()) {
                    v = [(f(v[0]) / f(len)).to_f32(), (f(v[1]) / f(len)).to_f32(), (f(v[2]) / f(len)).to_f32()];
                }
                let r = si_math::algebra::matrix_axis_rotation(a, &vec3(v));
                write_mat(m, matp, &r);
                si_model::model_image::state::correct_trans_of_ref_matrix(m, s, matp);
                read_mat(m, matp)
            }
            _ => [[0f32; 4]; 4],
        };
        for c in &conts {
            for p in &c.pts {
                // the test on the forms the original uses: raw doubles for the curves' kinds 0 to 2
                let on = match (&p.raw, axis_kind) {
                    (Some(r), 0) => !F80::from_f64(EPS).le(F80::from_f64(r[1]).abs()) && le(F80::from_f64(r[2])),
                    (Some(r), 1) => !F80::from_f64(EPS).le(F80::from_f64(r[0]).abs()) && le(F80::from_f64(r[2])),
                    (Some(r), 2) => !F80::from_f64(EPS).le(F80::from_f64(r[0]).abs()) && le(F80::from_f64(r[1])),
                    _ => ax.on_f32(p.f),
                };
                if !on || closed0 != 0 {
                    let v = xform(&mat, p.f, !curves);
                    m.set_f32(at, v[0]);
                    m.set_f32(at + 4, v[1]);
                    m.set_f32(at + 8, v[2]);
                    m.set_i32(idx.wrapping_add((e0 as u32).wrapping_mul(4)), e4);
                    e4 = e4.wrapping_add(1);
                    at = at.wrapping_add(mo::VERTEX);
                } else {
                    let v = m.u32(idx.wrapping_add((e0.wrapping_sub(total) as u32).wrapping_mul(4)));
                    m.set_u32(idx.wrapping_add((e0 as u32).wrapping_mul(4)), v);
                }
                e0 = e0.wrapping_add(1);
            }
        }
        d4 += 1;
    }
    // the polygon count
    let mut npoly = 0i32;
    for c in &conts {
        if c.nb == 0 {
            continue;
        }
        npoly = npoly.wrapping_add(c.nb).wrapping_sub(1).wrapping_add(c.closed as i32);
        let (mut i, mut prev) = if c.closed == 0 { (1, 0) } else { (0, c.nb - 1) };
        let mut prev_on = ax.on_f32(c.pts[prev as usize].f);
        while i < c.nb {
            let cur_on = ax.on_f32(c.pts[i as usize].f);
            if cur_on && prev_on {
                npoly = npoly.wrapping_sub(1);
            }
            prev_on = cur_on;
            prev = i;
            i += 1;
        }
        let _ = prev;
    }
    let cnt = npoly;
    let mut npoly = steps.wrapping_mul(cnt);
    if caps != 0 {
        npoly = steps.wrapping_mul(cnt).wrapping_add(2);
    }
    m.set_i32(mesh + mo::NB_POLYGONS, npoly);
    if npoly > m[g::SI_NB_POL_MAX] {
        mesh_image::obj::mesh_dispose(m, pmesh);
        mesh_image::face::face_dispose(m, pface);
        m.free(idx);
        return 0;
    }
    let polys = si_hrc::image::geom::polygon_allocate(m, npoly);
    m.set_u32(mesh + mo::POLYGONS, polys);
    let mut poly = polys;
    let ix = |m: &Mem, k: i32| m.i32(idx.wrapping_add((k as u32).wrapping_mul(4)));
    let mut d4 = 0i32;
    while d4 < steps {
        let mut d8 = 0i32;
        let nr = d4.wrapping_add(1) % rings;
        for c in &conts {
            if c.nb == 0 {
                continue;
            }
            let (mut i, mut prev) = if c.closed == 0 { (1, 0) } else { (0, c.nb - 1) };
            let mut prev_on = ax.on_f32(c.pts[prev as usize].f);
            while i < c.nb {
                let cur_on = ax.on_f32(c.pts[i as usize].f);
                let base = d4.wrapping_mul(total).wrapping_add(d8);
                let nbase = nr.wrapping_mul(total).wrapping_add(d8);
                if (!prev_on && !cur_on) || closed0 != 0 {
                    let corners = [ix(m, base.wrapping_add(i)), ix(m, base.wrapping_add(prev)), ix(m, nbase.wrapping_add(prev)), ix(m, nbase.wrapping_add(i))];
                    mesh_image::rebuild::polygon_initialize(m, poly, &corners);
                    poly = poly.wrapping_add(mo::POLYGON);
                } else if !prev_on {
                    // the point on the axis is the current one
                    let corners = [ix(m, base.wrapping_add(i)), ix(m, base.wrapping_add(prev)), ix(m, nbase.wrapping_add(prev))];
                    mesh_image::rebuild::polygon_initialize(m, poly, &corners);
                    poly = poly.wrapping_add(mo::POLYGON);
                } else if !cur_on {
                    let corners = [ix(m, base.wrapping_add(i)), ix(m, base.wrapping_add(prev)), ix(m, nbase.wrapping_add(i))];
                    mesh_image::rebuild::polygon_initialize(m, poly, &corners);
                    poly = poly.wrapping_add(mo::POLYGON);
                }
                prev_on = cur_on;
                prev = i;
                i += 1;
            }
            d8 = d8.wrapping_add(c.nb);
        }
        d4 += 1;
    }
    if caps != 0 {
        let nodes_count = if curves { (total as i16).wrapping_add(hole_count).wrapping_sub(1) } else { (total as i16).wrapping_add(hole_count) };
        let round = |n: u16| (n as u32).wrapping_sub((((n as u32).wrapping_sub(1) & 3).wrapping_sub(3)) as u32);
        // the first cap: the last ring in the reverse order, a separator between contours
        m.set_i16(poly + 0x10, nodes_count);
        let nodes = m.si_calloc(round(nodes_count as u16), mo::NODE);
        m.set_u32(poly + 0xc, nodes);
        let mut put = 0u32;
        let mut base = total.wrapping_mul(steps);
        for (k, c) in conts.iter().enumerate() {
            if k != 0 {
                m.set_i32(m.u32(poly + 0xc).wrapping_add(put), -1);
                put = put.wrapping_add(mo::NODE);
            }
            if c.nb != 0 {
                let mut at = base.wrapping_add(c.nb).wrapping_sub(1);
                for _ in 0..c.nb {
                    let v = ix(m, at);
                    m.set_i32(m.u32(poly + 0xc).wrapping_add(put), v);
                    put = put.wrapping_add(mo::NODE);
                    at = at.wrapping_sub(1);
                }
                base = base.wrapping_add(c.nb);
            }
        }
        // the second cap: the first ring in order
        let second = poly;
        m.set_i16(second + 0x18 + 0x10, nodes_count);
        let nodes = m.si_calloc(round(nodes_count as u16), mo::NODE);
        m.set_u32(second + 0x18 + 0xc, nodes);
        let mut put = 0u32;
        let mut at = 0i32;
        for (k, c) in conts.iter().enumerate() {
            if k != 0 {
                m.set_i32(m.u32(second + 0x18 + 0xc).wrapping_add(put), -1);
                put = put.wrapping_add(mo::NODE);
            }
            for _ in 0..c.nb.max(0) {
                let v = ix(m, at);
                m.set_i32(m.u32(second + 0x18 + 0xc).wrapping_add(put), v);
                put = put.wrapping_add(mo::NODE);
                at = at.wrapping_add(1);
            }
        }
    }
    m.free(idx);
    mesh_image::face::face_dispose(m, pface);
    mesh_image::rebuild::update(m, &mut crate::extrusion::ModelSide(o), mesh);
    mesh_image::obj::mesh_init_vertex_colors(m, mesh);
    mesh
}

/// Revolves a spline: a face holding a copy of it, revolved by [`mesh_revolve_face`], the face
/// freed again [soft 0x08198afc meshRevolveSpline]. Answers the mesh (0 when none was made).
#[allow(clippy::too_many_arguments)]
pub fn mesh_revolve_spline(m: &mut Mem, o: &mut dyn Outside, spline: u32, step: i16, steps: i32, angle: f32, axis: i32, model: u32, caps: i16) -> u32 {
    let face = si_hrc::image::spline::face_allocate(m).addr();
    si_spline::intersect_image::spline_transfer(m, Ptr32::new(spline), Ptr32::new(face + 4));
    let cell = m.local(4);
    m.set_u32(cell, face);
    let mesh = mesh_revolve_face(m, o, face, step, steps, angle, axis, model, caps);
    mesh_image::face::face_dispose(m, cell);
    m.release(4);
    mesh
}

/// Revolves each contour of a face into a patch [soft 0x081976e0 faceRevolutionToPatchs]:
/// the outline first, then each hole (inverted first, the holes being turned the other way);
/// `patches` receives the patches made, a contour that gave none being skipped. `kind` is the
/// spline type, `compensate` and `def` are passed on as they are.
#[allow(clippy::too_many_arguments)]
pub fn face_revolution_to_patchs(m: &mut Mem, o: &mut dyn Outside, face: u32, kind: i32, steps: i16, angle: f32, compensate: i16, axis: i32, model: u32, patches: u32, def: i32) {
    let first = si_spline::revolution_image::spline_revolution_to_patch(m, &mut SplineSide(o), Ptr32::new(face + 4), kind, steps, angle, axis, model, compensate, def);
    if first.addr() != 0 {
        m.set_u32(patches, first.addr());
    }
    let mut n: u16 = (first.addr() != 0) as u16;
    let mut i: i32 = 0;
    while i < m.u16(face + 0x78) as i16 as i32 {
        let sp = m.u32(face + 0x7c).wrapping_add((i * 0x74) as u32);
        si_spline::convert_image::spline_inverse(m, &mut SplineSide(o), Ptr32::new(sp));
        let p = si_spline::revolution_image::spline_revolution_to_patch(m, &mut SplineSide(o), Ptr32::new(sp), kind, steps, angle, axis, model, compensate, def);
        if p.addr() != 0 {
            m.set_u32(patches.wrapping_add((n as i16 as i32 as u32).wrapping_mul(4)), p.addr());
            n = n.wrapping_add(1);
        }
        i += 1;
    }
}
