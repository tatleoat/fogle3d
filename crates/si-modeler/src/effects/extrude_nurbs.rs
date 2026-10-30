// SPDX-License-Identifier: MIT
//! `extrusion.c` (sixth slice): the NURBS surfaces an extrusion makes along a path.
//! Finding 0034, "geomupd3" and "geomupd8" (nurbsExtrudeNurbsOnNurbs).

use si_mem::{Mem, Outside};
use si_nurbs::image::Halt;
use si_x87::{fmov64, F80};

/// The double at 0x0864a5c8: a second direction shorter than this makes the first frame
/// `SI_matrixZAlign`'s. (The one at 0x0864a5c0, 0.0001, is the length test of the helpers
/// shared with `meshExtrudeFaceOnSpline`, the same value.)
const EPS_PARALLEL: f64 = 1e-6;

/// A NURBS curve extruded along the curve of a model into a NURBS surface
/// [soft 0x0815e3f0 nurbsExtrudeNurbsOnNurbs].
///
/// 1. The path is the model's spline made a curve (`SI_spline2NurbsCrvKeepCurve`, kind 6) or a
///    copy of its curve (kind 9), carried by the model's matrix (+0x1c4); the original reads
///    through a null path for another kind or a missing model (a [`Halt`] here). A path
///    without points is disposed of and the answer is 0. Neither the path nor the matrices
///    below are freed afterwards.
/// 2. A 4 x 4 matrix for each path point (`SI_calloc`). Point `i` is the control point's
///    x, y, z each divided by its w in the register and rounded to float. The first matrix is a
///    frame from the curve's first and second derivatives (`SI_nurbsCrvValue`) at knot 1 (closed)
///    or the start of its range (+0x10): `a` the first rounded to float and made unit length as in
///    `meshExtrudeFaceOnSpline`, `c = a × second` (float times double in the register, each
///    component rounded to float); when the unrounded length of the rounded `c` is above 1e-6
///    (or a NaN) the frame has the columns `unit(a × unit(c))`, `unit(c)`, `a`, else it is
///    `SI_matrixZAlign(a)`; the matrix is its transpose.
/// 3. Each next matrix is the one before turned as in `meshExtrudeFaceOnSpline`, the direction at
///    point `i` being `unit(unit(p[i] − p[i−1]) + unit(p[i+1] − p[i]))`, the second difference
///    rounded to float before its length is taken; at the last point the next one is point 0 on
///    a closed path, and an open path takes `unit(p[i] − p[i−1])`. The fourth row of each matrix
///    is its point.
/// 4. The surface (`SI_nurbsSrfAllocate`; none: 0): the U direction from the section `crv` (order,
///    point and knot counts, range, closed flag, +0x34 into +0x70), the V direction from the path
///    (order, +0x34 into +0x74, point count; closed flag from the path; knot count points + 1
///    closed, else order + points − 2), +0x4c 2 when either curve's +0x24 is set (else 0), +0x50 1,
///    +0x5c and +0x60 0. Points: for each path matrix and each section point, the point's four
///    coordinates rounded to float carried by the matrix (`((x·m0j + y·m1j) + z·m2j) + w·m3j`, one
///    rounding to double). Knots (`si_knotsAllocate`): the section's, and for each of them once
///    more the path's (so none of the path's when the section has no knots), then the path's range.
pub fn nurbs_extrude_nurbs_on_nurbs(m: &mut Mem, o: &mut dyn Outside, crv: u32, model: u32) -> Result<u32, Halt> {
    let sp = m.sp();
    let r = on_nurbs(m, o, crv, model);
    m.set_sp(sp);
    r
}

fn on_nurbs(m: &mut Mem, o: &mut dyn Outside, crv: u32, model: u32) -> Result<u32, Halt> {
    use crate::extrusion2::{add, advance, cross, f80, unit, write_matrix};
    use si_math::{algebra, types::Vector};
    let mut path = 0u32;
    if model != 0 {
        match m.i32(model + 0x30) {
            6 => path = si_nurbs::conv6::spline_to_crv_keep_curve(m, o, m.u32(model + 0x48))?,
            9 => path = si_nurbs::image::crv_copy(m, m.u32(model + 0x48)),
            _ => {}
        }
    }
    if path == 0 {
        return Err(Halt::Refused("nurbsExtrudeNurbsOnNurbs: no path (the original reads through null)"));
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
    let diff = |p: [f32; 3], q: [f32; 3]| -> [F80; 3] { [0, 1, 2].map(|d| f80(p[d]) - f80(q[d])) };
    let rounded = |t: [F80; 3]| -> [F80; 3] { t.map(|v| f80(v.to_f32())) };
    let p0 = pt(m, 0);
    let closed = m.u16(path + 0x20) != 0;
    let n = m.i32(path + 4);
    let mats = m.si_calloc(n as u32, 0x40);
    // closed: knot 1; open: the start of the range (+0x10)
    let at = |m: &Mem| if closed { m.u32(path + 0xc).wrapping_add(8) } else { path + 0x10 };
    let t = fmov64(m.f64(at(m)));
    let d1 = si_nurbs::image::crv_value(m, path, t, 1)?;
    let t = fmov64(m.f64(at(m)));
    let d2 = si_nurbs::image::crv_value(m, path, t, 2)?;
    let a = unit(d1.map(|v| f80(F80::from_f64(v).to_f32())));
    let (x2, y2, z2) = (F80::from_f64(d2[0]), F80::from_f64(d2[1]), F80::from_f64(d2[2]));
    let c = [
        (f80(a[1]) * z2 - f80(a[2]) * y2).to_f32(),
        (f80(a[2]) * x2 - f80(a[0]) * z2).to_f32(),
        (f80(a[0]) * y2 - f80(a[1]) * x2).to_f32(),
    ];
    let c = c.map(f80);
    let len = ((c[0] * c[0] + c[1] * c[1]) + c[2] * c[2]).sqrt();
    let e6 = F80::from_f64(EPS_PARALLEL);
    let frame = if len.gt(e6) || len.unordered(e6) {
        let c = unit(c);
        let d = unit(cross(a, c));
        [[d[0], c[0], a[0], 0.0], [d[1], c[1], a[1], 0.0], [d[2], c[2], a[2], 0.0], [0.0, 0.0, 0.0, 1.0]]
    } else {
        algebra::matrix_z_align(&Vector::from_array(a))
    };
    write_matrix(m, mats, &algebra::matrix_transpose(&frame));
    let mut a = a;
    let mut i: i16 = 1;
    while (i as i32) < n.wrapping_sub(1) {
        let k = i as i32;
        let p = pt(m, k);
        let e = unit(diff(p, pt(m, k - 1)));
        let f = unit(rounded(diff(pt(m, k + 1), p)));
        let b = unit(add(f, e));
        advance(m, mats, k, a, b, cross(a, b));
        a = b;
        i = i.wrapping_add(1);
    }
    let k = i as i32;
    let p = pt(m, k);
    let e = unit(diff(p, pt(m, k - 1)));
    let b = if closed {
        let f = unit(rounded(diff(p0, p)));
        unit(add(f, e))
    } else {
        e
    };
    advance(m, mats, k, a, b, cross(a, b));
    let mut i: i16 = 0;
    while (i as i32) < n {
        let p = pt(m, i as i32);
        let row = mats.wrapping_add((i as i32 as u32).wrapping_mul(0x40)).wrapping_add(0x30);
        for (d, v) in p.iter().enumerate() {
            m.set_f32(row + 4 * d as u32, *v);
        }
        i = i.wrapping_add(1);
    }

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
    let mut at: i16 = 0;
    let mut i: i16 = 0;
    while (i as i32) < n {
        let mat = mats.wrapping_add((i as i32 as u32).wrapping_mul(0x40));
        let mut j: i16 = 0;
        while (j as i32) < m.i32(crv + 4) {
            let q = m.u32(crv + 0x2c).wrapping_add((j as i32 as u32).wrapping_mul(0x24));
            let v: [F80; 4] = [0u32, 8, 0x10, 0x18].map(|d| f80(F80::from_f64(m.f64(q.wrapping_add(d))).to_f32()));
            let e = |m: &Mem, r: u32, col: u32| f80(m.f32(mat.wrapping_add(16 * r + 4 * col)));
            for col in 0..4u32 {
                let s = ((v[0] * e(m, 0, col) + v[1] * e(m, 1, col)) + v[2] * e(m, 2, col)) + v[3] * e(m, 3, col);
                let out = m.u32(srf + 0x54).wrapping_add((at as i32 as u32).wrapping_mul(0x24)).wrapping_add(8 * col);
                m.set_f64(out, s.to_f64());
            }
            at = at.wrapping_add(1);
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
