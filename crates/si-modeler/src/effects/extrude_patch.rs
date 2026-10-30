// SPDX-License-Identifier: MIT
//! `extrusion.c` (fifth slice): the patches an extrusion makes from a face or a spline.
//! Finding 0034, "geomupd3" and "geomupd8" (patchExtrudeFaceOnSpline).

use si_mem::{Mem, Outside, Ptr32};

/// The double at 0x0864a5f8 (the one at 0x0864a5f0, 0.0001, is the length test of
/// `meshExtrudeFaceOnSpline`'s helpers, the same value): a cross product shorter than this makes the first frame `SI_matrixZAlign`'s.
const EPS_PARALLEL: f64 = 1e-6;
/// The float at 0x0864a600 (0.5), the cubic patch's +0x14.
const PATCH_CUBIC_CONST: f32 = 0.5;

/// A face extruded along a curved path into patches [soft 0x08161540 patchExtrudeFaceOnSpline].
///
/// 1. A path without keys answers 0. A Bézier path (kind 1) is expanded (`SI_bezierExpand`, the copy
///    kept); a 4 x 4 matrix is made for each of its keys (`SI_calloc`, never freed).
/// 2. The first matrix is a frame from two directions at the first key. On an open path: `f` from key
///    0 to key 3 and `a` from key 0 to key `s` (`s` = 2 on a Bézier path, else 1); on a closed one: `e`
///    from the last key to key 0, `f` from key 0 to key `s` and `a` = `e + f`. Each is made unit length
///    as `meshExtrudeFaceOnSpline` does, except that the key-`s` difference is rounded to float before
///    its length is taken. When the length of `c = a × f` (unrounded) is above 1e-6 (or a NaN) the frame
///    is the columns `unit(a × unit(c))`, `unit(c)`, `a`; otherwise `SI_matrixZAlign(a)`. The first
///    matrix is its transpose.
/// 3. Each next matrix is the one before turned by the angle between the directions on either side of
///    the key, as in `meshExtrudeFaceOnSpline`; the fourth row of each is its key's position.
/// 4. For the outline and each hole a patch (`SI_patchAllocate`), stored in a list of `nb_holes + 1`
///    words (`SI_malloc`, the answer). A contour without keys leaves its patch empty. Otherwise the patch
///    takes the contour's key count, type, closed flag, +0x1c and +0x20, the path's closed flag, type
///    (0.5 at +0x14 for type 3), +0x20 and key count, and a point for each key of the path and of the
///    contour: the contour's key carried by that key's matrix (`((x·m0j + y·m1j) + z·m2j) + m3j`, four
///    coordinates), and a zeroed flag word for each (`SI_calloc`); then `SI_patchUpdateMatrix`. A
///    Bézier contour is expanded first (the copy kept).
pub fn patch_extrude_face_on_spline(m: &mut Mem, _o: &mut dyn Outside, face: u32, path_in: u32) -> u32 {
    use crate::extrusion2::{add, advance, cross, f80, through_x87, unit, write_matrix};
    use si_math::{algebra, types::Vector};
    use si_x87::F80;
    if m.u16(path_in + 0x10) == 0 {
        return 0;
    }
    let mut path = path_in;
    if m.i32(path + 0xc) == 1 {
        path = si_spline::convert_image::spline_bezier_expand(m, Ptr32::new(path)).addr();
    }
    let n = m.u16(path + 0x10) as i32;
    let mats = m.si_calloc(n as u32, 0x40);
    let closed = m.i16(path + 8) != 0;
    let kb = m.u32(path + 0x14);
    let key = |i: i32| kb.wrapping_add((i as u32).wrapping_mul(0x30)).wrapping_add(0xc);
    let diff = |m: &Mem, p: u32, q: u32| -> [F80; 3] { [0u32, 4, 8].map(|d| f80(m.f32(p.wrapping_add(d))) - f80(m.f32(q.wrapping_add(d)))) };
    let rounded = |t: [F80; 3]| -> [F80; 3] { t.map(|v| f80(v.to_f32())) };
    let s = if m.i32(path + 0xc) == 1 { 2 } else { 1 };
    let (a, f) = if !closed {
        let f = unit(diff(m, key(3), key(0)));
        let a = unit(rounded(diff(m, key(s), key(0))));
        (a, f)
    } else {
        let e = unit(diff(m, key(0), key(n - 1)));
        let f = unit(rounded(diff(m, key(s), key(0))));
        (unit(add(f, e)), f)
    };
    let c = cross(a, f);
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
    let mut i: i32 = 1;
    while i < n - 1 {
        let e = unit(diff(m, key(i), key(i - 1)));
        let f = unit(diff(m, key(i + 1), key(i)));
        let b = unit(add(f, e));
        advance(m, mats, i, a, b, cross(a, b));
        a = b;
        i += 1;
    }
    let b = if closed {
        let e = unit(diff(m, key(i), key(i - 1)));
        let f = unit(diff(m, key(0), key(i)));
        unit(add(f, e))
    } else {
        unit(diff(m, key(i), key(i - 1)))
    };
    advance(m, mats, i, a, b, cross(a, b));
    for k in 0..n {
        for d in 0..3u32 {
            m.set_f32(mats.wrapping_add(0x40 * k as u32 + 0x30 + 4 * d), through_x87(m.f32(key(k) + 4 * d)));
        }
    }

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

/// A spline extruded along a path into patches [soft 0x0815bef0 patchExtrudeSplineOnSpline]: the spline is
/// moved into a new face (`SI_splineTransfer`) and that face goes to `patchExtrudeFaceOnLinearSpline` (a
/// path with no curvature, +0xc zero) or `patchExtrudeFaceOnSpline`; the answer is the first patch of the
/// list they make. The face is disposed. Where the face cannot be allocated the original returns what
/// its saved register held; here 0.
pub fn patch_extrude_spline_on_spline(m: &mut Mem, o: &mut dyn Outside, spline: u32, path: u32) -> u32 {
    let sp = m.sp();
    let slot = m.local(4);
    let face = si_hrc::image::spline::face_allocate(m).addr();
    m.set_u32(slot, face);
    let mut list = 0u32;
    if face != 0 {
        si_hrc::image::spline::spline_transfer(m, spline, face + 4);
        list = if m.i32(path + 0xc) == 0 {
            super::lin_patch::patch_extrude_face_on_linear_spline(m, o, face, path)
        } else {
            patch_extrude_face_on_spline(m, o, face, path)
        };
        si_model::mesh_image::face::face_dispose(m, slot);
    }
    m.set_sp(sp);
    if list == 0 { 0 } else { m.u32(list) }
}

/// A face extruded into one patch for the outline and one for each hole [soft 0x0815be20 faceExtrudeToPatchs]:
/// `SI_splineExtrudeToPatch` on the outline (stored at `out[0]` when it makes one) and, for each hole, on a
/// copy turned round (`SI_splineInverse`), each stored at the next free entry.
pub fn face_extrude_to_patchs(m: &mut Mem, o: &mut dyn Outside, face: u32, kind: i32, depth: f32, steps: i32, out: u32, axis: i32, p6: u32) {
    let sp = m.sp();
    let slot = m.local(4);
    let first = super::extrusion3::spline_extrude_to_patch(m, o, face + 4, kind, depth, steps, axis, p6);
    if first != 0 {
        m.set_u32(out, first);
    }
    let mut n: u16 = (first != 0) as u16;
    let mut i: i16 = 0;
    while i < m.i16(face + 0x78) {
        let src = m.u32(face + 0x7c).wrapping_add((i as i32 as u32).wrapping_mul(0x74));
        let copy = si_hrc::image::spline::spline_copy(m, src).addr();
        m.set_u32(slot, copy);
        si_spline::convert_image::spline_inverse(m, &mut super::SplineS(o), Ptr32::new(copy));
        let patch = super::extrusion3::spline_extrude_to_patch(m, o, copy, kind, depth, steps, axis, p6);
        if patch != 0 {
            m.set_u32(out.wrapping_add((n as i16 as i32 as u32).wrapping_mul(4)), patch);
            n = n.wrapping_add(1);
        }
        si_hrc::image::spline::spline_dispose(m, slot);
        i = i.wrapping_add(1);
    }
    m.set_sp(sp);
}
