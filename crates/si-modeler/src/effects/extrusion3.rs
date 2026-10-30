// SPDX-License-Identifier: MIT
//! `extrusion.c` (third slice): the surfaces an extrusion makes from a curve or a spline, and the
//! update of the extrusion effect. Finding 0034, "geomupd2".

use si_math::algebra;
use si_math::types::{Hvector, Vector};
use si_mem::{Mem, Ptr32};
use si_x87::F80;

/// `DAT_0864a598`, the binary's float the cubic patch takes at +0x14.
/// The float at 0x0864a598 (read-only data, 0.5), kept as a constant because the image's data is not in `Mem`.
const PATCH_CUBIC_CONST: u32 = 0x3f00_0000;

fn mat_from(m: &Mem, a: u32) -> [[f32; 4]; 4] {
    let mut r = [[0f32; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = m.f32(a + 16 * i as u32 + 4 * j as u32);
        }
    }
    r
}

/// The vector of the step along an axis (`SI_ExtrudeAxis`: 0 x, 1 y, 2 z). For any other axis the
/// original leaves the vector as the stack held it; zeros here.
fn step_vector(axis: i32, step: f32) -> Vector {
    match axis {
        0 => Vector { x: step, y: 0.0, z: 0.0 },
        1 => Vector { x: 0.0, y: step, z: 0.0 },
        2 => Vector { x: 0.0, y: 0.0, z: step },
        _ => Vector { x: 0.0, y: 0.0, z: 0.0 },
    }
}

/// `depth / steps` as the x87 unit divides them and the store rounds to float.
fn quotient(depth: f32, steps: i32) -> f32 {
    (F80::from_f32(depth) / F80::from_i32(steps)).to_f32()
}

/// A NURBS curve extruded along an axis into a NURBS surface [soft 0x0815b6d8 SI_nurbsExtrudeToNurbs].
/// `kind` 1, 2, 3 gives order 2, 3, 4 in the new direction (kinds 2 and 3 take at least 2 and 3
/// steps); the control points of each new row are the previous row times a translation, summed in
/// the x87 unit and stored as doubles; the new direction's knots are 0..n for kind 1, clamped for 2
/// and 3. A curve without points gives the surface as allocated. The answer is the surface (0 when
/// the allocation fails, where the original reads address 0 and stops).
pub fn nurbs_extrude_to_nurbs(m: &mut Mem, crv: u32, kind: i32, depth: f32, steps: i32, axis: i32, p6: u32) -> u32 {
    let srf = si_hrc::image::nurbs::nurbs_srf_allocate(m).addr();
    if srf == 0 {
        return 0;
    }
    if m.i32(crv + 4) == 0 {
        return srf;
    }
    let mut steps = steps;
    if kind == 3 && steps < 3 {
        steps = 3;
    } else if kind == 2 && steps < 2 {
        steps = 2;
    }
    let step = quotient(depth, steps);
    let w = m.u32(crv);
    m.set_u32(srf, w);
    m.set_i32(srf + 0x24, if kind == 1 { 2 } else if kind == 2 { 3 } else { 4 });
    let w = m.u16(crv + 0x20);
    m.set_u16(srf + 0x20, w);
    let w = m.u32(crv + 0x34);
    m.set_u32(srf + 0x70, w);
    let (a, b) = (m.u64(crv + 0x10), m.u64(crv + 0x18));
    m.set_u64(srf + 0x10, a);
    m.set_u64(srf + 0x18, b);
    let n = m.i32(crv + 4);
    m.set_i32(srf + 4, n);
    let nk = m.i32(crv + 8);
    m.set_i32(srf + 8, nk);
    let k = si_hrc::image::nurbs::knots_allocate(m, nk);
    m.set_u32(srf + 0xc, k);
    m.set_u16(srf + 0x44, 0);
    m.set_i32(srf + 0x28, steps.wrapping_add(1));
    m.set_u32(srf + 0x74, p6);
    let order = m.i32(srf + 0x24);
    let cnt = steps.wrapping_add(1).wrapping_add(order).wrapping_sub(2);
    m.set_i32(srf + 0x2c, cnt);
    let k = si_hrc::image::nurbs::knots_allocate(m, cnt);
    m.set_u32(srf + 0x30, k);
    m.set_u16(srf + 0x50, 0);
    m.set_u16(srf + 0x5c, 0);
    m.set_i32(srf + 0x60, 0);
    let w = m.u32(crv + 0x24);
    m.set_u32(srf + 0x4c, w);
    let rows = m.i32(srf + 4).wrapping_mul(m.i32(srf + 0x28));
    m.set_i32(srf + 0x48, rows);
    let pts = si_hrc::image::nurbs::nurbs_points_allocate(m, rows);
    m.set_u32(srf + 0x54, pts);
    // the first row: four doubles of each 0x24-byte point
    let mut i: i16 = 0;
    while (i as i32) < m.i32(crv + 4) {
        let off = (i as i32 as u32).wrapping_mul(0x24);
        for d in [0u32, 8, 0x10, 0x18] {
            let v = m.u64(m.u32(crv + 0x2c).wrapping_add(off).wrapping_add(d));
            let dst = m.u32(srf + 0x54).wrapping_add(off).wrapping_add(d);
            m.set_u64(dst, v);
        }
        i = i.wrapping_add(1);
    }
    let t = algebra::matrix_translation(&step_vector(axis, step));
    let f = |k: usize| F80::from_f32(t[k / 4][k % 4]);
    let mut j: i16 = 1;
    while 1 < m.i32(srf + 0x28) && (j as i32) < m.i32(srf + 0x28) {
        let mut c: i16 = 0;
        while (c as i32) < m.i32(srf + 4) {
            let n = m.i32(srf + 4);
            let base = m.u32(srf + 0x54);
            let src = base.wrapping_add(((c as i32).wrapping_add(n.wrapping_mul(j as i32 - 1))).wrapping_mul(0x24) as u32);
            let dst = base.wrapping_add((((j as i32).wrapping_mul(n)).wrapping_add(c as i32)).wrapping_mul(0x24) as u32);
            let p: [F80; 4] = [0u32, 8, 0x10, 0x18].map(|d| F80::from_f64(m.f64(src + d)));
            // each output is ((p0 * M[0][k] + p1 * M[1][k]) + p2 * M[2][k]) + p3 * M[3][k] in the unit, stored as a double
            for k in 0..4usize {
                let s = ((p[0] * f(k) + p[1] * f(4 + k)) + p[2] * f(8 + k)) + p[3] * f(12 + k);
                m.set_f64(dst + 8 * k as u32, s.to_f64());
            }
            c = c.wrapping_add(1);
        }
        j = j.wrapping_add(1);
    }
    // the knots of the curve's direction
    let mut i: i16 = 0;
    while (i as i32) < m.i32(srf + 8) {
        let v = m.u64(m.u32(crv + 0xc).wrapping_add((i as i32 as u32).wrapping_mul(8)));
        m.set_u64(m.u32(srf + 0xc).wrapping_add((i as i32 as u32).wrapping_mul(8)), v);
        i = i.wrapping_add(1);
    }
    // the knots of the new direction
    let kv = |m: &Mem| m.u32(srf + 0x30);
    if (kind as u32).wrapping_sub(2) < 2 {
        let order = m.i32(srf + 0x24);
        let mut k: i16 = 0;
        if order != 1 && order - 1 >= 0 {
            while (k as i32) < order - 1 {
                m.set_u64(kv(m) + 8 * k as i32 as u32, 0);
                k = k.wrapping_add(1);
            }
        }
        let order = m.i32(srf + 0x24);
        let cnt = m.i32(srf + 0x2c);
        let mut v: i16 = 1;
        while (k as i32) < (cnt - order) + 1 {
            m.set_f64(kv(m) + 8 * k as i32 as u32, v as f64);
            v = v.wrapping_add(1);
            k = k.wrapping_add(1);
        }
        let last = (m.i32(srf + 0x28) - m.i32(srf + 0x24)) + 1;
        while (k as i32) < m.i32(srf + 0x2c) {
            m.set_f64(kv(m) + 8 * k as i32 as u32, last as f64);
            k = k.wrapping_add(1);
        }
    } else if kind == 1 {
        let mut k: i16 = 0;
        while 0 < m.i32(srf + 0x2c) && (k as i32) < m.i32(srf + 0x2c) {
            m.set_f64(kv(m) + 8 * k as i32 as u32, k as f64);
            k = k.wrapping_add(1);
        }
    }
    let v = m.u32(srf + 0x30);
    let order = m.i32(srf + 0x24);
    let a = m.u64(v.wrapping_add((order.wrapping_mul(8)) as u32).wrapping_sub(0x10));
    m.set_u64(srf + 0x34, a);
    let a = m.u64(v.wrapping_add((m.i32(srf + 0x28).wrapping_mul(8)) as u32).wrapping_sub(8));
    m.set_u64(srf + 0x3c, a);
    srf
}

/// A spline extruded along an axis into a patch [soft 0x0815bafc SI_splineExtrudeToPatch].
/// `kind` is the patch's type word (0 linear; any other adds an unextruded row at either end, `steps + 3`
/// rows in all, the first moved back one step), `p6` its 16-bit step. A Bézier spline (type 1) is expanded first and
/// the copy disposed at the end (not when the spline has no keys: the original returns the patch
/// then, leaving the copy). The answer is the patch, 0 when it could not be allocated.
pub fn spline_extrude_to_patch(m: &mut Mem, _o: &mut dyn si_mem::Outside, spline: u32, kind: i32, depth: f32, steps: i32, axis: i32, p6: u32) -> u32 {
    let sp0 = m.sp();
    let ptype = m.i32(spline + 0xc);
    let mut spline = spline;
    if ptype == 1 {
        spline = si_spline::convert_image::spline_bezier_expand(m, Ptr32::new(spline)).addr();
    }
    let pat = si_hrc::image::patch::patch_allocate(m).addr();
    if pat != 0 {
        let nkeys = m.u16(spline + 0x10) as u32;
        if nkeys == 0 {
            return pat;
        }
        let step = quotient(depth, steps);
        // The original keeps the negated quotient unrounded (80-bit) for the first row's shift.
        let mut back = F80::ZERO;
        let mut rows = (steps as i16).wrapping_add(1);
        if kind != 0 {
            rows = (steps as i16).wrapping_add(3);
            back = -(F80::from_f32(depth) / F80::from_i32(steps));
        }
        let cnt = nkeys.wrapping_mul(rows as i32 as u32);
        let pts = m.si_calloc(cnt, 0x10);
        m.set_u32(pat + 0x1a4, pts);
        let fl = m.si_calloc(cnt, 2);
        m.set_u32(pat + 0x1a8, fl);
        let mut c = cnt as i32;
        let mut s = c as i16;
        while s != 0 {
            s = c as i16;
            c = c.wrapping_sub(1);
            m.set_u16(m.u32(pat + 0x1a8).wrapping_add((s as i32 as u32).wrapping_mul(2)).wrapping_sub(2), 0);
            s = c as i16;
        }
        let mut i: i16 = 0;
        let keys = m.u32(spline + 0x14);
        loop {
            let at = m.u32(pat + 0x1a4).wrapping_add((i as i32 as u32).wrapping_mul(0x10));
            let key = keys.wrapping_add((i as i32 as u32).wrapping_mul(0x30));
            for d in [0u32, 4, 8] {
                let v = m.u32(key + 0xc + d);
                m.set_u32(at + d, v);
            }
            m.set_f32(at + 0xc, 1.0);
            if (0..3).contains(&axis) {
                let v = (F80::from_f32(m.f32(at + 4 * axis as u32)) + back).to_f32();
                m.set_f32(at + 4 * axis as u32, v);
            }
            i = i.wrapping_add(1);
            if (i as i32) >= nkeys as i32 {
                break;
            }
        }
        let t = algebra::matrix_translation(&step_vector(axis, step));
        let mut j: i16 = 1;
        while j < rows {
            for k in 0..(nkeys as u16 as i16) {
                let base = m.u32(pat + 0x1a4);
                let src = base.wrapping_add((((j as i32 - 1).wrapping_mul(nkeys as i32)).wrapping_add(k as i32)).wrapping_mul(0x10) as u32);
                let dst = base.wrapping_add((((nkeys as i32).wrapping_mul(j as i32)).wrapping_add(k as i32)).wrapping_mul(0x10) as u32);
                let v = Hvector { x: m.f32(src), y: m.f32(src + 4), z: m.f32(src + 8), w: m.f32(src + 12) };
                let r = algebra::hvector_multiply_matrix(&v, &t);
                m.set_f32(dst, r.x);
                m.set_f32(dst + 4, r.y);
                m.set_f32(dst + 8, r.z);
                m.set_f32(dst + 12, r.w);
            }
            j = j.wrapping_add(1);
        }
        let w = m.u32(spline + 0xc);
        m.set_u32(pat + 8, w);
        let w = m.u16(spline + 0x10);
        m.set_u16(pat + 0xe, w);
        let w = m.u16(spline + 8);
        m.set_u16(pat + 0x12, w);
        let w = m.u32(spline + 0x1c);
        m.set_u32(pat + 0x18, w);
        let w = m.u16(spline + 0x20);
        m.set_u16(pat + 0x1e, w);
        m.set_i16(pat + 0x1c, p6 as i16);
        m.set_i32(pat + 4, kind);
        if kind == 3 {
            let w = PATCH_CUBIC_CONST;
            m.set_u32(pat + 0x14, w);
        }
        m.set_i16(pat + 0xc, rows);
        si_spline::patch_eval_image::patch_update_matrix(m, Ptr32::new(pat));
    }
    if ptype == 1 {
        let pp = m.local(4);
        m.set_u32(pp, spline);
        si_hrc::image::spline::spline_dispose(m, pp);
    }
    m.set_sp(sp0);
    pat
}
