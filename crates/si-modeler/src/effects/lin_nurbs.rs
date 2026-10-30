// SPDX-License-Identifier: MIT
//! `extrusion.c` (package lin2): a NURBS curve extruded along a polyline path, the path being a
//! NURBS curve of order 2 whose control points are the corners. Finding 0034, "lin2".
//!
//! The section is carried from corner to corner by one 4 x 4 matrix per corner. Unlike the
//! smooth-path extrusion ([`super::extrude_nurbs`]) each matrix also widens the section across
//! the bend (`SI_createMatrixZScalingForExtrusion`), so the walls keep their thickness at a
//! mitred corner, up to the length of the shorter of the two segments that meet there.

use si_math::algebra::{
    matrix_axis_rotation, matrix_copy, matrix_inverse, matrix_multiply, matrix_transpose, matrix_z_align,
    points_angle, vector_angle,
};
use si_math::types::{Matrix, Vector, IDENTITY};
use si_mem::{Mem, Outside};
use si_nurbs::image::Halt;
use si_x87::{fmov64, F80};

/// The double at 0x0864a5a8: the length below which a direction is left as it is, and the
/// squared length of `a × b` below which two directions count as parallel.
const EPS: f64 = 0.0001;
/// The double at 0x0864a5b0: a second direction shorter than this makes the first frame
/// `SI_matrixZAlign`'s.
const EPS_PARALLEL: f64 = 1e-6;
/// The double at 0x0864a5b8.
const HALF: f64 = 0.5;

fn f80(v: f32) -> F80 {
    F80::from_f32(v)
}

fn vec(v: [f32; 3]) -> Vector {
    Vector::from_array(v)
}

/// `p − q` for two stored float points, kept unrounded.
fn sub(p: [f32; 3], q: [f32; 3]) -> [F80; 3] {
    [0, 1, 2].map(|d| f80(p[d]) - f80(q[d]))
}

/// `p + q` for two stored float vectors, kept unrounded.
fn add(p: [f32; 3], q: [f32; 3]) -> [F80; 3] {
    [0, 1, 2].map(|d| f80(p[d]) + f80(q[d]))
}

/// `a × b`, each component the difference of two exact products, kept unrounded.
fn cross(a: [f32; 3], b: [f32; 3]) -> [F80; 3] {
    [
        f80(a[1]) * f80(b[2]) - f80(a[2]) * f80(b[1]),
        f80(a[2]) * f80(b[0]) - f80(a[0]) * f80(b[2]),
        f80(a[0]) * f80(b[1]) - f80(a[1]) * f80(b[0]),
    ]
}

fn round(t: [F80; 3]) -> [f32; 3] {
    t.map(|v| v.to_f32())
}

/// `(x² + y²) + z²` in the register.
fn len2(t: [F80; 3]) -> F80 {
    (t[0] * t[0] + t[1] * t[1]) + t[2] * t[2]
}

/// A direction made unit length the way the original writes it out each time: the length is
/// `sqrt((x² + y²) + z²)` rounded to float; when its size is above 0.0001 each component of
/// `t` (as the register holds it) is divided by that float, else `t` is only rounded.
fn unit(t: [F80; 3]) -> [f32; 3] {
    let len = len2(t).sqrt().to_f32();
    if f80(len).abs().gt(F80::from_f64(EPS)) {
        let l = f80(len);
        t.map(|v| (v / l).to_f32())
    } else {
        round(t)
    }
}

/// [`unit`] of a vector already stored as floats.
fn unit_f(v: [f32; 3]) -> [f32; 3] {
    unit(v.map(f80))
}

/// The length of the shorter of the two segments at a corner, rounded to float: the squared
/// lengths are summed from the unrounded differences; when the forward length is above the
/// backward one the backward square is taken, otherwise (a NaN included) the forward one, and
/// its root is taken again.
fn shorter(fwd: [F80; 3], back: [F80; 3]) -> f32 {
    let (lf, lb) = (len2(fwd), len2(back));
    let phi = if lf.sqrt().gt(lb.sqrt()) { lb } else { lf };
    phi.sqrt().to_f32()
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

/// `SI_createMatrixZScalingForExtrusion (0x0815bf5c)` as [`super::extrude_linear`] ports it, with this
/// module's float triples: `axis` the bend's axis, `point` the forward direction at the corner,
/// `angle` the half bend angle, `max` the largest widening.
fn z_scaling(axis: [f32; 3], point: [f32; 3], angle: f32, max: f32) -> Matrix {
    super::extrude_linear::create_matrix_z_scaling_for_extrusion(vec(axis), vec(point), angle, max)
}

/// A NURBS curve extruded along a polyline path into a NURBS surface
/// [soft 0x0815c194 nurbsExtrudeNurbsOnLinearNurbs].
///
/// 1. The path is the model's spline made a curve (`SI_spline2NurbsCrvKeepCurve`, kind 6) or a
///    copy of its curve (kind 9), carried by the model's matrix (+0x1c4); for another kind or a
///    missing model the original reads through a null path (a [`Halt`] here). A path without
///    points is disposed of and the answer is 0. Neither the path nor the matrices are freed.
/// 2. Corner `i` is control point `i`'s x, y, z each divided by its w and rounded to float. One
///    4 x 4 matrix per corner (`SI_calloc`). The first direction `a`: on a closed path the sum of
///    `unit(p0 − p[n−1])` and `unit(p1 − p0)` rounded to float, on an open one `p1 − p0` rounded,
///    then made unit length; with `e = unit(p0 − p[n−1])` and `c = unit(a × e)`, the first frame
///    has the columns `unit(a × c)`, `c`, `a` when `c`'s length is above 1e-6 (or a NaN), else it
///    is `SI_matrixZAlign(a)`; the first matrix is its transpose.
/// 3. At each inner corner: `e`, `f` the backward and forward segments (rounded, then unit
///    length), `b = unit(f + e)` the new direction, the half bend angle from `SI_pointsAngle`,
///    `g = unit(e × f)` and the shorter segment's length. When `|a × b|²` is above 0.0001 the
///    turn is the rotation by `SI_vectorAngle(a, b)` about `unit(a × b)` followed by the widening
///    `SI_createMatrixZScalingForExtrusion(g, f, half angle, shorter length)`, and the matrix is
///    the one before, times the inverse of the widening used there, times that turn; otherwise
///    it is a copy of the one before and the last widening stays. The inverse is refreshed
///    after every corner (`SI_matrixInverse`; left as it was when that fails).
/// 4. The last corner turns towards `unit(f + e)` with point 0 as the next one on a closed path
///    (widened with the angle at the last corner taken between the last two points and point 0),
///    towards `unit(p[n−1] − p[n−2])` unwidened on an open one. A closed path then rewrites the
///    first matrix as the last one turned once more towards the first corner's direction
///    (widened with the angle at point 0): without a new `a` or a new inverse when the last turn
///    was a copy, and with the turn left from before when this one is parallel.
/// 5. The fourth row of each matrix is its corner. The surface is filled as in
///    `nurbsExtrudeNurbsOnNurbs` (`SI_nurbsSrfAllocate`, `si_knotsAllocate`,
///    `SI_nurbsPointsAllocate`): U from the section, V from the path, each section point carried
///    by each matrix.
///
/// A path of one point makes the original read the corners before and after it, outside the
/// point array, and write a second matrix past the end of the one-matrix block: a [`Halt`] here
/// (as for a negative count).
pub fn nurbs_extrude_nurbs_on_linear_nurbs(m: &mut Mem, o: &mut dyn Outside, crv: u32, model: u32) -> Result<u32, Halt> {
    let sp = m.sp();
    let r = on_linear(m, o, crv, model);
    m.set_sp(sp);
    r
}

fn on_linear(m: &mut Mem, o: &mut dyn Outside, crv: u32, model: u32) -> Result<u32, Halt> {
    let mut path = 0u32;
    if model != 0 {
        match m.i32(model + 0x30) {
            6 => path = si_nurbs::conv6::spline_to_crv_keep_curve(m, o, m.u32(model + 0x48))?,
            9 => path = si_nurbs::image::crv_copy(m, m.u32(model + 0x48)),
            _ => {}
        }
    }
    if path == 0 {
        return Err(Halt::Refused("nurbsExtrudeNurbsOnLinearNurbs: no path (the original reads through null)"));
    }
    si_nurbs::image::crv_apply_matrix(m, path, model.wrapping_add(0x1c4), 0);
    if m.i32(path + 4) == 0 {
        let slot = m.local(4);
        m.set_u32(slot, path);
        si_hrc::image::nurbs::nurbs_crv_dispose(m, slot);
        return Ok(0);
    }
    let pt = |m: &Mem, i: i32| -> [f32; 3] {
        let p = m.u32(path + 0x2c).wrapping_add((i as u32).wrapping_mul(0x24));
        let w = F80::from_f64(m.f64(p.wrapping_add(0x18)));
        [0u32, 8, 0x10].map(|d| (F80::from_f64(m.f64(p.wrapping_add(d))) / w).to_f32())
    };
    let n = m.i32(path + 4);
    if n < 2 {
        // Where the Rust must differ: with one point the original reads the corners before and
        // after it outside the point array (heap words of whatever lies there) and writes a second
        // matrix past the end of its one-matrix block; a negative count goes to `SI_calloc` as a
        // huge one. No order-2 curve has fewer than two points.
        return Err(Halt::Refused("nurbsExtrudeNurbsOnLinearNurbs: a path of fewer than two points (the original reads and writes outside its arrays)"));
    }
    let p0 = pt(m, 0);
    let p1 = pt(m, 1);
    let pl = pt(m, n.wrapping_sub(1));
    let pl2 = pt(m, n.wrapping_sub(2));
    let closed = m.u16(path + 0x20) != 0;
    let mats = m.si_calloc(n as u32, 0x40);
    let at = |i: i32| mats.wrapping_add((i as u32).wrapping_mul(0x40));
    let eps = F80::from_f64(EPS);
    // the widening of the last turn and its inverse
    let mut z = matrix_copy(&IDENTITY);
    let mut zinv = matrix_copy(&IDENTITY);

    // The original zeroes the five direction locals first; `f` keeps the zeros on an open path
    // until the first inner corner.
    let mut e: [f32; 3];
    let mut f = [0.0f32; 3];
    let mut a: [f32; 3];
    let mut b: [f32; 3];
    let mut g: [f32; 3];
    if closed {
        e = unit(sub(p0, pl));
        f = unit(sub(p1, p0));
        a = round(add(f, e));
    } else {
        a = round(sub(p1, p0));
        e = unit(sub(p0, pl));
    }
    a = unit_f(a);
    let c = unit(cross(a, e));
    let clen = len2(c.map(f80)).sqrt();
    let e6 = F80::from_f64(EPS_PARALLEL);
    let frame = if clen.gt(e6) || clen.unordered(e6) {
        let d = unit(cross(a, c));
        [[d[0], c[0], a[0], 0.0], [d[1], c[1], a[1], 0.0], [d[2], c[2], a[2], 0.0], [0.0, 0.0, 0.0, 1.0]]
    } else {
        matrix_z_align(&vec(a))
    };
    write_matrix(m, mats, &matrix_transpose(&frame));
    let mut rot = matrix_copy(&IDENTITY);
    let mut shortest = 0.0f32; // every use is preceded by a store in the original too

    let last = n.wrapping_sub(1);
    let mut i: i16 = 1;
    if last > 1 {
        loop {
            let k = i as i32;
            let cur = pt(m, k);
            let prev = pt(m, k - 1);
            let next = pt(m, k + 1);
            let back = sub(cur, prev);
            let fwd = sub(next, cur);
            shortest = shorter(fwd, back);
            e = unit_f(round(back));
            f = unit_f(round(fwd));
            b = unit(add(f, e));
            let half = (f80(points_angle(&vec(prev), &vec(cur), &vec(next))) * F80::from_f64(HALF)).to_f32();
            g = unit(cross(e, f));
            let h = cross(a, b);
            // the original keeps a × b in e's locals
            if len2(h).gt(eps) {
                let axis = unit(h);
                let angle = vector_angle(&vec(a), &vec(b));
                rot = matrix_axis_rotation(angle, &vec(axis));
                z = z_scaling(g, f, half, shortest);
                rot = matrix_multiply(&rot, &z);
                let t = matrix_multiply(&read_matrix(m, at(k - 1)), &zinv);
                write_matrix(m, at(k), &matrix_multiply(&t, &rot));
            } else {
                let before = read_matrix(m, at(k - 1));
                write_matrix(m, at(k), &matrix_copy(&before));
            }
            a = b;
            if let Some(inv) = matrix_inverse(&z) {
                zinv = inv;
            }
            i = i.wrapping_add(1);
            if (i as i32) >= last {
                break;
            }
        }
    }

    let k = i as i32;
    let cur = pt(m, k);
    let prev = pt(m, k - 1);
    if closed {
        let back = sub(cur, prev);
        let fwd = sub(p0, cur);
        shortest = shorter(fwd, back);
        e = unit_f(round(back));
        f = unit_f(round(fwd));
        b = unit(add(f, e));
    } else {
        b = unit(sub(cur, prev));
    }
    g = unit(cross(e, f));
    let h = cross(a, b);
    if len2(h).gt(eps) {
        let axis = unit(h);
        let angle = vector_angle(&vec(a), &vec(b));
        rot = matrix_axis_rotation(angle, &vec(axis));
        if closed {
            let corner = points_angle(&vec(pl2), &vec(pl), &vec(p0));
            z = z_scaling(g, f, (F80::from_f64(HALF) * corner).to_f32(), shortest);
            rot = matrix_multiply(&rot, &z);
        }
        let t = matrix_multiply(&read_matrix(m, at(k - 1)), &zinv);
        write_matrix(m, at(k), &matrix_multiply(&t, &rot));
        if closed {
            a = b;
            if let Some(inv) = matrix_inverse(&z) {
                zinv = inv;
            }
        }
    } else {
        let before = read_matrix(m, at(k - 1));
        write_matrix(m, at(k), &matrix_copy(&before));
    }
    if closed {
        let back = sub(p0, pl);
        let fwd = sub(p1, p0);
        shortest = shorter(fwd, back);
        e = unit_f(round(back));
        f = unit_f(round(fwd));
        b = unit(add(f, e));
        g = unit(cross(e, f));
        let h = cross(a, b);
        if len2(h).gt(eps) {
            let axis = unit(h);
            let angle = vector_angle(&vec(a), &vec(b));
            rot = matrix_axis_rotation(angle, &vec(axis));
            let corner = points_angle(&vec(pl), &vec(p0), &vec(p1));
            z = z_scaling(g, f, (F80::from_f64(HALF) * corner).to_f32(), shortest);
            rot = matrix_multiply(&rot, &z);
        }
        let t = matrix_multiply(&read_matrix(m, at(k)), &zinv);
        write_matrix(m, mats, &matrix_multiply(&t, &rot));
    }
    let _ = (e, g);

    let mut i: i16 = 0;
    while (i as i32) < n {
        let p = pt(m, i as i32);
        let row = at(i as i32).wrapping_add(0x30);
        for (d, v) in p.iter().enumerate() {
            m.set_f32(row + 4 * d as u32, *v);
        }
        i = i.wrapping_add(1);
    }
    surface(m, crv, path, n, closed, mats)
}

/// The surface of step 5: the same filling as `nurbsExtrudeNurbsOnNurbs`'s.
fn surface(m: &mut Mem, crv: u32, path: u32, n: i32, closed: bool, mats: u32) -> Result<u32, Halt> {
    let srf = si_hrc::image::nurbs::nurbs_srf_allocate(m).addr();
    if srf == 0 {
        return Ok(0);
    }
    let cp = |m: &mut Mem, from: u32, to: u32| {
        let v = m.u32(from);
        m.set_u32(to, v);
    };
    cp(m, crv, srf);
    cp(m, path, srf + 0x24);
    cp(m, path + 0x34, srf + 0x74);
    let w = m.u16(crv + 0x20);
    m.set_u16(srf + 0x20, w);
    cp(m, crv + 0x34, srf + 0x70);
    m.set_u16(srf + 0x44, closed as u16);
    for off in [0x10u32, 0x18] {
        let v = fmov64(m.f64(crv + off));
        m.set_f64(srf + off, v);
    }
    cp(m, crv + 4, srf + 4);
    m.set_i32(srf + 0x28, n);
    m.set_u16(srf + 0x50, 1);
    m.set_u16(srf + 0x5c, 0);
    m.set_u32(srf + 0x60, 0);
    let rational = m.u32(crv + 0x24) != 0 || m.u32(path + 0x24) != 0;
    m.set_u32(srf + 0x4c, if rational { 2 } else { 0 });
    cp(m, crv + 8, srf + 8);
    let nv = if closed { m.i32(srf + 0x28).wrapping_add(1) } else { m.i32(srf + 0x24).wrapping_add(m.i32(srf + 0x28)).wrapping_sub(2) };
    m.set_i32(srf + 0x2c, nv);
    let ku = si_hrc::image::nurbs::knots_allocate(m, m.i32(srf + 8));
    m.set_u32(srf + 0xc, ku);
    let kv = si_hrc::image::nurbs::knots_allocate(m, m.i32(srf + 0x2c));
    m.set_u32(srf + 0x30, kv);
    m.set_i32(srf + 0x48, n.wrapping_mul(m.i32(crv + 4)));
    let pts = si_hrc::image::nurbs::nurbs_points_allocate(m, n.wrapping_mul(m.i32(crv + 4)));
    m.set_u32(srf + 0x54, pts);
    let mut out_at: i16 = 0;
    let mut i: i16 = 0;
    while (i as i32) < n {
        let mat = mats.wrapping_add((i as i32 as u32).wrapping_mul(0x40));
        let mut j: i16 = 0;
        while (j as i32) < m.i32(crv + 4) {
            let q = m.u32(crv + 0x2c).wrapping_add((j as i32 as u32).wrapping_mul(0x24));
            let v: [F80; 4] = [0u32, 8, 0x10, 0x18].map(|d| f80(F80::from_f64(m.f64(q.wrapping_add(d))).to_f32()));
            let el = |m: &Mem, r: u32, col: u32| f80(m.f32(mat.wrapping_add(16 * r + 4 * col)));
            for col in 0..4u32 {
                let s = ((v[0] * el(m, 0, col) + v[1] * el(m, 1, col)) + v[2] * el(m, 2, col)) + v[3] * el(m, 3, col);
                let out = m.u32(srf + 0x54).wrapping_add((out_at as i32 as u32).wrapping_mul(0x24)).wrapping_add(8 * col);
                m.set_f64(out, s.to_f64());
            }
            out_at = out_at.wrapping_add(1);
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    let mut i: i16 = 0;
    while (i as i32) < m.i32(srf + 8) {
        let off = (i as i32 as u32).wrapping_mul(8);
        let v = fmov64(m.f64(m.u32(crv + 0xc).wrapping_add(off)));
        m.set_f64(m.u32(srf + 0xc).wrapping_add(off), v);
        i = i.wrapping_add(1);
        let mut s: i16 = 0;
        while (s as i32) < m.i32(srf + 0x2c) {
            let off = (s as i32 as u32).wrapping_mul(8);
            let v = fmov64(m.f64(m.u32(path + 0xc).wrapping_add(off)));
            m.set_f64(m.u32(srf + 0x30).wrapping_add(off), v);
            s = s.wrapping_add(1);
        }
    }
    for (from, to) in [(0x10u32, 0x34u32), (0x18, 0x3c)] {
        let v = fmov64(m.f64(path + from));
        m.set_f64(srf + to, v);
    }
    Ok(srf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_leaves_short_vectors() {
        let t = [F80::from_f64(1e-6), F80::ZERO, F80::ZERO];
        assert_eq!(unit(t), [1e-6f64 as f32, 0.0, 0.0]);
        assert_eq!(unit_f([3.0, 0.0, 4.0]), [0.6, 0.0, 0.8]);
    }

    #[test]
    fn shorter_takes_the_backward_square_only_when_forward_is_longer() {
        let a = [F80::from_f64(3.0), F80::from_f64(4.0), F80::ZERO];
        let b = [F80::from_f64(1.0), F80::ZERO, F80::ZERO];
        assert_eq!(shorter(a, b), 1.0);
        assert_eq!(shorter(b, a), 1.0);
        let nan = [F80::INDEFINITE, F80::ZERO, F80::ZERO];
        assert!(shorter(nan, b).is_nan());
    }
}
