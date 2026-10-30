// SPDX-License-Identifier: MIT
//! `extrusion.c` (the polyline slice, unit lin1): a face extruded along a polyline path into
//! patches or into a mesh. Finding 0034, "lin1".
//!
//! Both routines build one 4 x 4 matrix per key of the path the same way (see [`frames`]);
//! they differ in where the first frame's parallel test sits and in what is built from the
//! matrices. Everything works on `si-mem`'s image.
//!
//! The small helpers (unit length, cross product, the caps) are private copies of those in
//! `extrusion2.rs`; `SI_createMatrixZScalingForExtrusion (0x0815bf5c)` is the port in
//! `effects/extrude_linear.rs` (unit geomupd8).

use crate::extrusion::{ModelSide, SplineSide};
use si_math::algebra;
use si_math::types::{Matrix, Vector, IDENTITY};
use si_mem::{Mem, Outside, Ptr32};
use si_model::mesh_image::{self, g, off as mo};
use si_x87::F80;

/// The doubles at 0x0864a5d0 (patch) and 0x0864a608 (mesh): a length or a squared length
/// not above this is taken for zero.
const EPS: f64 = 0.0001;
/// The doubles at 0x0864a5d8 and 0x0864a610: the first frame's parallel test.
const EPS_PARALLEL: f64 = 1e-6;
/// The doubles at 0x0864a5e0 and 0x0864a618: the half of a bend's angle.
const HALF: f64 = 0.5;
/// The float at 0x0864a5e8 (0.5), the cubic patch's +0x14.
const PATCH_CUBIC_CONST: f32 = 0.5;

fn f80(v: f32) -> F80 {
    F80::from_f32(v)
}

/// A float through the x87 unit (a load and a store: a signalling NaN comes out quiet).
fn through_x87(v: f32) -> f32 {
    F80::from_f32(v).to_f32()
}

fn read_matrix(m: &Mem, at: u32) -> Matrix {
    let mut r = [[0.0f32; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = m.f32(at.wrapping_add(16 * i as u32 + 4 * j as u32));
        }
    }
    r
}

fn write_matrix(m: &mut Mem, at: u32, r: &Matrix) {
    for (i, row) in r.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            m.set_f32(at.wrapping_add(16 * i as u32 + 4 * j as u32), *v);
        }
    }
}

/// `(x² + y²) + z²` in the register.
fn len2(t: [F80; 3]) -> F80 {
    (t[0] * t[0] + t[1] * t[1]) + t[2] * t[2]
}

/// A direction made unit length as the original does it: `t` as the register holds it, its
/// length `sqrt((x² + y²) + z²)` rounded to float; `t` divided by that length when the length
/// is above 0.0001, else `t` stored as it is. The result is the stored floats.
fn unit(t: [F80; 3]) -> [f32; 3] {
    let len = len2(t).sqrt().to_f32();
    if f80(len).abs().gt(F80::from_f64(EPS)) {
        let l = f80(len);
        [(t[0] / l).to_f32(), (t[1] / l).to_f32(), (t[2] / l).to_f32()]
    } else {
        [t[0].to_f32(), t[1].to_f32(), t[2].to_f32()]
    }
}

/// A vector rounded to the floats it was stored as (the register value no longer used).
fn rounded(t: [F80; 3]) -> [F80; 3] {
    t.map(|v| f80(v.to_f32()))
}

/// `a x b`, each component the exact difference of two products, kept unrounded.
fn cross(a: [f32; 3], b: [f32; 3]) -> [F80; 3] {
    [
        f80(a[1]) * f80(b[2]) - f80(a[2]) * f80(b[1]),
        f80(a[2]) * f80(b[0]) - f80(a[0]) * f80(b[2]),
        f80(a[0]) * f80(b[1]) - f80(a[1]) * f80(b[0]),
    ]
}

/// The sum of two stored float vectors, kept unrounded.
fn add(a: [f32; 3], b: [f32; 3]) -> [F80; 3] {
    [f80(a[0]) + f80(b[0]), f80(a[1]) + f80(b[1]), f80(a[2]) + f80(b[2])]
}

fn vec(p: [f32; 3]) -> Vector {
    Vector::from_array(p)
}

/// `SI_createMatrixZScalingForExtrusion (0x0815bf5c)`, the port in [`super::extrude_linear`]:
/// the matrix that widens the section by `min(1 / sin(angle), max)` along the bend's direction
/// (`axis` the bend's axis, `point` the outgoing direction).
fn z_scaling(axis: Vector, point: Vector, angle: f32, max: f32) -> Matrix {
    super::extrude_linear::create_matrix_z_scaling_for_extrusion(axis, point, angle, max)
}

/// Where the first frame's parallel test sits.
#[derive(Clone, Copy, PartialEq)]
enum FirstTest {
    /// On `c = a x f` made unit length (the patch routine): `sqrt((c.y² + c.x²) + c.z²)` of the
    /// stored floats.
    AfterUnit,
    /// On `c` as the register holds it, before it is made unit length (the mesh routine).
    BeforeUnit,
}

/// The state the matrix builder carries from key to key: the rotation `r`, the widening `z`
/// and its inverse `zinv` (each kept when not recomputed, as the original's locals are).
struct Carry {
    r: Matrix,
    z: Matrix,
    zinv: Matrix,
}

/// The two directions on either side of a key and what the widening needs from them: `ev` the
/// incoming difference, `fv` the outgoing one (both unrounded). Answers the shorter length
/// (rounded to float; the outgoing one on a tie or a NaN), the two made unit length from their
/// stored floats, and their sum made unit length.
fn bend(ev: [F80; 3], fv: [F80; 3]) -> (f32, [f32; 3], [f32; 3], [f32; 3]) {
    let (e2, f2) = (len2(ev), len2(fv));
    let minlen = if f2.sqrt().gt(e2.sqrt()) { e2 } else { f2 }.sqrt().to_f32();
    let e = unit(rounded(ev));
    let f = unit(rounded(fv));
    let b = unit(add(f, e));
    (minlen, e, f, b)
}

/// Half the angle at `b` between the directions to `a` and `c` (`SI_pointsAngle`), as a float.
fn half_angle(m: &Mem, a: u32, b: u32, c: u32) -> f32 {
    let p = |at: u32| vec([m.f32(at), m.f32(at + 4), m.f32(at + 8)]);
    (f80(algebra::points_angle(&p(a), &p(b), &p(c))) * F80::from_f64(HALF)).to_f32()
}

/// The rotation from `a` to `b` about `xv` (`a x b`, unrounded) made unit length.
fn turn(a: [f32; 3], b: [f32; 3], xv: [F80; 3]) -> Matrix {
    let axis = unit(xv);
    let angle = algebra::vector_angle(&vec(a), &vec(b));
    algebra::matrix_axis_rotation(angle, &vec(axis))
}

/// The matrices of a polyline path, one per key (`mats`, `n` blocks of 0x40 bytes; the keys of
/// `path`, 0x30 bytes each, position at +0xc).
///
/// 1. The first frame, at key 0: on an open path `f` = key 3 - key 0 and `a` = key 1 - key 0
///    (key 3 is read even on a shorter path, as in the original); on a closed one `e` = key 0 -
///    the last key, `f` = key 1 - key 0 and `a` = `unit(f + e)`, each made unit length from the
///    unrounded difference. `c = a x f`; when its length (where [`FirstTest`] says) is above
///    1e-6 or a NaN, the frame's columns are `unit(a x c)`, `c`, `a`; otherwise
///    `SI_matrixZAlign(a)`. The first matrix is its transpose; the rotation starts as the
///    identity, and so do the widening and its inverse.
/// 2. At each inner key i: `e` = key i - key i-1, `f` = key i+1 - key i, the shorter length,
///    `b = unit(f + e)`, the half angle at key i (`SI_pointsAngle`) and the bend's axis
///    `unit(e x f)`. When `|a x b|²` is above 0.0001 the rotation is the turn from `a` to `b`
///    times the widening `SI_createMatrixZScalingForExtrusion(axis, f, half, shorter)`, and the
///    matrix is (the one before x the widening's inverse kept) x the rotation; otherwise a copy
///    of the one before. Then `a = b` and the inverse is that of the widening (kept when the
///    inversion fails).
/// 3. At the last key (index max(1, n-1)): on a closed path the same as an inner key with key 0
///    for the next key, the half angle at key n-1 (from key n-2 to key 0); on an open one `b` =
///    the last difference, no widening. On a closed path, unless the last step was a copy, `a = b`
///    and the inverse is recomputed; then the bend at key 0 (from the last key to key 1, the half
///    angle at key 0) gives the rotation (kept from before when `a` and `b` are parallel), and
///    the first matrix is replaced by (the last one x the inverse) x the rotation.
/// 4. The fourth row of each matrix is its key's position.
///
/// With one key the second matrix is written past the block, as the original does.
fn frames(m: &mut Mem, path: u32, mats: u32, test: FirstTest) {
    let n = m.u16(path + 0x10) as i32;
    let closed = m.i16(path + 8) != 0;
    let kb = m.u32(path + 0x14);
    let key = |i: i32| kb.wrapping_add((i as u32).wrapping_mul(0x30)).wrapping_add(0xc);
    let diff = |m: &Mem, p: u32, q: u32| -> [F80; 3] { [0u32, 4, 8].map(|d| f80(m.f32(p.wrapping_add(d))) - f80(m.f32(q.wrapping_add(d)))) };
    let mat = |i: i32| mats.wrapping_add((i as u32).wrapping_mul(0x40));
    let eps = F80::from_f64(EPS);
    let mut k = Carry { r: algebra::matrix_copy(&IDENTITY), z: algebra::matrix_copy(&IDENTITY), zinv: algebra::matrix_copy(&IDENTITY) };

    let (a, f) = if !closed {
        let f = unit(diff(m, key(3), key(0)));
        let a = unit(diff(m, key(1), key(0)));
        (a, f)
    } else {
        let e = unit(diff(m, key(0), key(n - 1)));
        let f = unit(diff(m, key(1), key(0)));
        (unit(add(f, e)), f)
    };
    let cv = cross(a, f);
    let e6 = F80::from_f64(EPS_PARALLEL);
    let (wide, c) = match test {
        FirstTest::BeforeUnit => {
            let len = len2(cv).sqrt();
            (len.gt(e6) || len.unordered(e6), unit(cv))
        }
        FirstTest::AfterUnit => {
            let c = unit(cv);
            let len = ((f80(c[1]) * f80(c[1]) + f80(c[0]) * f80(c[0])) + f80(c[2]) * f80(c[2])).sqrt();
            (len.gt(e6) || len.unordered(e6), c)
        }
    };
    let frame = if wide {
        let d = unit(cross(a, c));
        [[d[0], c[0], a[0], 0.0], [d[1], c[1], a[1], 0.0], [d[2], c[2], a[2], 0.0], [0.0, 0.0, 0.0, 1.0]]
    } else {
        algebra::matrix_z_align(&vec(a))
    };
    write_matrix(m, mats, &algebra::matrix_transpose(&frame));

    let mut a = a;
    let mut i: i32 = 1;
    while i < n - 1 {
        let (minlen, e, f, b) = bend(diff(m, key(i), key(i - 1)), diff(m, key(i + 1), key(i)));
        let half = half_angle(m, key(i - 1), key(i), key(i + 1));
        let c2 = unit(cross(e, f));
        let xv = cross(a, b);
        let cur = if len2(xv).gt(eps) {
            k.r = turn(a, b, xv);
            k.z = z_scaling(vec(c2), vec(f), half, minlen);
            k.r = algebra::matrix_multiply(&k.r, &k.z);
            let t = algebra::matrix_multiply(&read_matrix(m, mat(i - 1)), &k.zinv);
            algebra::matrix_multiply(&t, &k.r)
        } else {
            algebra::matrix_copy(&read_matrix(m, mat(i - 1)))
        };
        write_matrix(m, mat(i), &cur);
        a = b;
        if let Some(inv) = algebra::matrix_inverse(&k.z) {
            k.zinv = inv;
        }
        i += 1;
    }

    // the last key
    let (b, last) = if closed {
        let (minlen, e, f, b) = bend(diff(m, key(i), key(i - 1)), diff(m, key(0), key(i)));
        (b, Some((minlen, f, unit(cross(e, f)))))
    } else {
        (unit(diff(m, key(i), key(i - 1))), None)
    };
    let xv = cross(a, b);
    let turned = len2(xv).gt(eps);
    let cur = if turned {
        k.r = turn(a, b, xv);
        if let Some((minlen, f, c2)) = last {
            let half = half_angle(m, key(n - 2), key(n - 1), key(0));
            k.z = z_scaling(vec(c2), vec(f), half, minlen);
            k.r = algebra::matrix_multiply(&k.r, &k.z);
        }
        let t = algebra::matrix_multiply(&read_matrix(m, mat(i - 1)), &k.zinv);
        algebra::matrix_multiply(&t, &k.r)
    } else {
        algebra::matrix_copy(&read_matrix(m, mat(i - 1)))
    };
    write_matrix(m, mat(i), &cur);
    if closed {
        if turned {
            a = b;
            if let Some(inv) = algebra::matrix_inverse(&k.z) {
                k.zinv = inv;
            }
        }
        // the bend at key 0, from the last key to key 1
        let (minlen, e, f, b) = bend(diff(m, key(0), key(n - 1)), diff(m, key(1), key(0)));
        let c2 = unit(cross(e, f));
        let xv = cross(a, b);
        if len2(xv).gt(eps) {
            k.r = turn(a, b, xv);
            let half = half_angle(m, key(n - 1), key(0), key(1));
            k.z = z_scaling(vec(c2), vec(f), half, minlen);
            k.r = algebra::matrix_multiply(&k.r, &k.z);
        }
        let t = algebra::matrix_multiply(&read_matrix(m, mat(i)), &k.zinv);
        let first = algebra::matrix_multiply(&t, &k.r);
        write_matrix(m, mats, &first);
    }

    for j in 0..n {
        for d in 0..3u32 {
            m.set_f32(mat(j).wrapping_add(0x30 + 4 * d), through_x87(m.f32(key(j) + 4 * d)));
        }
    }
}

/// A face extruded along a polyline path into patches
/// [soft 0x0815f500 patchExtrudeFaceOnLinearSpline].
///
/// 1. A path without keys answers 0. A Bézier path (kind 1) is expanded (`SI_bezierExpand`, the
///    copy kept and used from then on). A 4 x 4 matrix is made for each key (`SI_calloc`, never
///    freed) as [`frames`] says, the first frame tested after `c` is made unit length.
/// 2. For the outline and each hole a patch (`SI_patchAllocate`), stored in a list of
///    `nb_holes + 1` words (`SI_malloc`, the answer). A contour without keys leaves its patch
///    empty. Otherwise the patch takes the contour's key count, type, closed flag, +0x1c and
///    +0x20, the path's closed flag, type (0.5 at +0x14 for type 3), +0x20 and key count, and a
///    point for each key of the path and of the contour: the contour's key carried by that
///    key's matrix (`((x·m0j + y·m1j) + z·m2j) + m3j`, four coordinates), and a zeroed flag
///    word for each (`SI_calloc`); then `SI_patchUpdateMatrix`. A Bézier contour is expanded
///    first (the copy kept).
pub fn patch_extrude_face_on_linear_spline(m: &mut Mem, _o: &mut dyn Outside, face: u32, path_in: u32) -> u32 {
    if m.u16(path_in + 0x10) == 0 {
        return 0;
    }
    let mut path = path_in;
    if m.i32(path + 0xc) == 1 {
        path = si_spline::convert_image::spline_bezier_expand(m, Ptr32::new(path)).addr();
    }
    let mats = m.si_calloc(m.u16(path + 0x10) as u32, 0x40);
    frames(m, path, mats, FirstTest::AfterUnit);

    let nbh = m.i16(face + 0x78);
    let list = m.si_malloc((nbh as i32 as u32).wrapping_mul(4).wrapping_add(4));
    let mut k: i16 = -1;
    while k < m.i16(face + 0x78) {
        let mut c = if k == -1 { face + 4 } else { m.u32(face + 0x7c).wrapping_add((k as i32 as u32).wrapping_mul(0x74)) };
        if m.i32(c + 0xc) == 1 {
            c = si_spline::convert_image::spline_bezier_expand(m, Ptr32::new(c)).addr();
        }
        let pat = si_hrc::image::patch::patch_allocate(m).addr();
        let slot = list.wrapping_add((k as i32 as u32).wrapping_mul(4)).wrapping_add(4);
        m.set_u32(slot, pat);
        k = k.wrapping_add(1);
        let nb = m.u16(c + 0x10);
        if nb == 0 {
            continue;
        }
        m.set_u16(pat + 0xe, nb);
        let w = m.u32(c + 0xc);
        m.set_u32(pat + 8, w);
        let w = m.u16(c + 8);
        m.set_u16(pat + 0x12, w);
        let w = through_x87(m.f32(c + 0x1c));
        m.set_f32(pat + 0x18, w);
        let w = m.u16(c + 0x20);
        m.set_u16(pat + 0x1e, w);
        let w = m.u16(path + 8);
        m.set_u16(pat + 0x10, w);
        let w = m.u32(path + 0xc);
        m.set_u32(pat + 4, w);
        let w = m.u16(path + 0x20);
        m.set_u16(pat + 0x1c, w);
        let rows = m.u16(path + 0x10);
        m.set_u16(pat + 0xc, rows);
        let nb = nb as i16 as i32;
        let pts = m.si_calloc((rows as i16 as i32).wrapping_mul(nb) as u32, 0x10);
        m.set_u32(pat + 0x1a4, pts);
        let mut at: i16 = 0;
        let mut row: i16 = 0;
        while row < rows as i16 {
            let mat = mats.wrapping_add((row as i32 as u32).wrapping_mul(0x40));
            let mut j: i16 = 0;
            while (j as i32) < m.u16(c + 0x10) as i32 {
                let kp = m.u32(c + 0x14).wrapping_add((j as i32 as u32).wrapping_mul(0x30)).wrapping_add(0xc);
                let (px, py, pz) = (f80(m.f32(kp)), f80(m.f32(kp + 4)), f80(m.f32(kp + 8)));
                let e = |m: &Mem, col: u32| f80(m.f32(mat.wrapping_add(col)));
                let out = m.u32(pat + 0x1a4).wrapping_add((at as i32 as u32).wrapping_mul(0x10));
                for col in 0..4u32 {
                    let v = ((px * e(m, 4 * col) + py * e(m, 0x10 + 4 * col)) + pz * e(m, 0x20 + 4 * col)) + e(m, 0x30 + 4 * col);
                    m.set_f32(out.wrapping_add(4 * col), v.to_f32());
                }
                at = at.wrapping_add(1);
                j = j.wrapping_add(1);
            }
            row = row.wrapping_add(1);
        }
        if m.i32(pat + 4) == 3 {
            m.set_f32(pat + 0x14, PATCH_CUBIC_CONST);
        }
        let cnt = (rows as i16 as i32).wrapping_mul(nb);
        let fl = m.si_calloc(cnt as u32, 2);
        m.set_u32(pat + 0x1a8, fl);
        let mut q: i16 = 0;
        while (q as i32) < cnt {
            let fl = m.u32(pat + 0x1a8);
            m.set_u16(fl.wrapping_add((q as i32 as u32).wrapping_mul(2)), 0);
            q = q.wrapping_add(1);
        }
        si_spline::patch_eval_image::patch_update_matrix(m, Ptr32::new(pat));
    }
    list
}

/// One contour of the section being extruded: its point count and whether it is closed.
struct Ring {
    nb: i32,
    closed: bool,
}

/// A point carried by the 4 x 4 matrix at `mat`: `((x * m0 + y * m4) + z * m8) + m12` for x
/// (and the same per column), in the register, stored as a float.
fn transform(m: &Mem, mat: u32, p: [f32; 3]) -> [f32; 3] {
    let f = |k: u32| F80::from_f32(m.f32(mat + 4 * k));
    let (x, y, z) = (F80::from_f32(p[0]), F80::from_f32(p[1]), F80::from_f32(p[2]));
    let col = |c: u32| ((x * f(c) + y * f(c + 4)) + z * f(c + 8) + f(c + 12)).to_f32();
    [col(0), col(1), col(2)]
}

/// The two cap polygons, `poly` the first: the contours' points in the reverse order with a
/// separator (-1) between contours, then (the polygon after) the points of the last row in
/// order, numbered from `second_start`. Each has `total + cap_extra` nodes.
fn write_caps(m: &mut Mem, poly: u32, rings: &[Ring], total: i32, cap_extra: i16, second_start: i32) {
    let n = (total as u16).wrapping_add(cap_extra as u16);
    let round = |n: u16| (n as u32).wrapping_sub(((n as u32).wrapping_sub(1) & 3).wrapping_sub(3));
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

/// A face extruded along a polyline path into a mesh
/// [soft 0x08163914 meshExtrudeFaceOnLinearSpline].
///
/// 1. A null face or path answers 0. `caps` is dropped for a closed path or an open outline. The
///    face is made linear (`SI_faceConvertToLinear`, `step`, kept in a slot); the polygon count
///    the result will have is worked out from the path's key count, closed flag, kind and
///    `steps` and the contours' point counts, and above `SI_nbPolMax` the face is disposed of
///    and the answer is 0. The path is made linear (`SI_splineConvertToLinear`, regular,
///    `steps`; a null answers 0 and leaves the face), cleaned up for a path of another kind
///    than 0.
/// 2. A 4 x 4 matrix for each key of the linear path, as [`frames`] says, the first frame
///    tested before `c` is made unit length.
/// 3. The mesh (`SI_meshAllocate`): for each key, each contour point carried by its matrix is a
///    vertex; quadrilaterals join neighbouring rows (and the last row to the first on a closed
///    path), one fewer per row for an open outline; `caps` adds the two end polygons. Then
///    `SI_meshUpdate`, `SI_meshInitVertexColors`, and the linear face and path are disposed of.
pub fn mesh_extrude_face_on_linear_spline(m: &mut Mem, o: &mut dyn Outside, face: u32, step: i16, path: u32, steps: i32, caps_in: i16) -> u32 {
    let sp = m.sp();
    let pface = m.local(4);
    let pspline = m.local(4);
    m.set_u32(pface, 0);
    m.set_u32(pspline, 0);
    let r = mesh_linear(m, o, pface, pspline, face, step, path, steps, caps_in);
    m.set_sp(sp);
    r
}

#[allow(clippy::too_many_arguments)]
fn mesh_linear(m: &mut Mem, o: &mut dyn Outside, pface: u32, pspline: u32, face: u32, step: i16, path: u32, steps: i32, caps_in: i16) -> u32 {
    if path == 0 || face == 0 {
        return 0;
    }
    let mut caps = caps_in;
    if m.i16(path + 8) != 0 || m.i16(face + 0xc) == 0 {
        caps = 0;
    }
    let lin = mesh_image::face::face_convert_to_linear(m, &mut ModelSide(o), face, step, 1);
    m.set_u32(pface, lin);
    if lin == 0 {
        return 0;
    }
    let nbh = m.i16(lin + 0x78);
    let path_closed = m.i16(path + 8) as i32;
    let mut c8 = m.u16(lin + 0x14) as i32;
    if nbh > 0 {
        for k in 0..nbh as u32 {
            c8 = c8.wrapping_add(m.u16(m.u32(lin + 0x7c).wrapping_add(0x74 * k + 0x10)) as i32);
        }
    }
    let keys_n = m.u16(path + 0x10) as i32;
    let mut expect = keys_n.wrapping_sub(1).wrapping_add(path_closed);
    if m.i16(path + 8) == 0 && (m.i32(path + 0xc).wrapping_sub(2) as u32) < 2 {
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
    frames(m, a4, mats, FirstTest::BeforeUnit);

    // the contours of the face (none when its hole count is negative)
    let mut rings = Vec::new();
    let mut points = Vec::new();
    if nbh >= 0 {
        for k in -1..nbh as i32 {
            let c = if k == -1 { lin + 4 } else { m.u32(lin + 0x7c).wrapping_add(0x74 * k as u32) };
            let nb = m.u16(c + 0x10);
            let ks = m.u32(c + 0x14);
            for j in 0..nb as u32 {
                let at = ks.wrapping_add(0x30 * j);
                points.push([m.f32(at + 0xc), m.f32(at + 0x10), m.f32(at + 0x14)]);
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
    let lin_closed = m.i16(a4 + 8) as i32;
    let segs = (n - 1).wrapping_add(lin_closed);
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
    if lin_closed != 0 {
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
