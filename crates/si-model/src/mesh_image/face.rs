// SPDX-License-Identifier: MIT
//! A face as one polygon with holes, on the image (`face.c`; finding 0015, "The face
//! object"): the face made linear, its contours given their directions, the face freed,
//! and the conversion to a mesh. The value forms are in [`crate::face`]; the normals of the
//! contours are computed there (`spline_normal`, `curve_normal`, checked by emulation) on
//! the image's keys and points.
//!
//! What `face.c` calls in the spline and NURBS files goes through [`Outside`] except what
//! `si-hrc` has on the image (`SI_faceAllocate`, `SI_splineTransfer`, `SI_splineDispose`),
//! which is called as it is.

use si_mem::Mem;

use super::off as mo;
use crate::model_image::{Callee, Outside};

/// `SI_splineConvertToLinear` (0x0848beec, `spline.c`).
pub const SI_SPLINE_CONVERT_TO_LINEAR: Callee = Callee { name: "SI_splineConvertToLinear", addr: 0x0848beec };
/// `SI_splineCleanup` (0x0848eb28, `spline.c`).
pub const SI_SPLINE_CLEANUP: Callee = Callee { name: "SI_splineCleanup", addr: 0x0848eb28 };
/// `SI_splineInverse` (0x0848e35c, `spline.c`).
pub const SI_SPLINE_INVERSE: Callee = Callee { name: "SI_splineInverse", addr: 0x0848e35c };
/// `SI_nurbsCrvConvertToLinear` (0x084e0eb4).
pub const SI_NURBS_CRV_CONVERT_TO_LINEAR: Callee = Callee { name: "SI_nurbsCrvConvertToLinear", addr: 0x084e0eb4 };
/// `SI_nurbsCrvInit` (0x084dbd80).
pub const SI_NURBS_CRV_INIT: Callee = Callee { name: "SI_nurbsCrvInit", addr: 0x084dbd80 };
/// `SI_nurbsCrvDispose` (0x084db870).
pub const SI_NURBS_CRV_DISPOSE: Callee = Callee { name: "SI_nurbsCrvDispose", addr: 0x084db870 };
/// `SI_nurbsCrvInverse` (0x084dca24).
pub const SI_NURBS_CRV_INVERSE: Callee = Callee { name: "SI_nurbsCrvInverse", addr: 0x084dca24 };

/// The routines of other files `face.c` calls here.
pub const CALLEES: &[Callee] = &[SI_SPLINE_CONVERT_TO_LINEAR, SI_SPLINE_CLEANUP, SI_SPLINE_INVERSE, SI_NURBS_CRV_CONVERT_TO_LINEAR, SI_NURBS_CRV_INIT, SI_NURBS_CRV_DISPOSE, SI_NURBS_CRV_INVERSE];

/// A spline of the image as the value form reads it (kind, count, keys).
fn spline_value(m: &Mem, s: u32) -> si_hrc::Spline {
    let n = m.u16(s + 0x10);
    let keys = m.u32(s + 0x14);
    let v3 = |a: u32| [m.f32(a), m.f32(a + 4), m.f32(a + 8)];
    si_hrc::Spline {
        kind: m.i32(s + 0xc),
        nkeys: n,
        keys: if keys == 0 { Vec::new() } else { (0..n as u32).map(|k| { let at = keys + 0x30 * k; si_hrc::SplineKey { prev: v3(at), pos: v3(at + 0xc), next: v3(at + 0x18), ..Default::default() } }).collect() },
        ..Default::default()
    }
}

/// A NURBS curve of the image as the value form reads it (point count and points).
fn curve_value(m: &Mem, c: u32) -> si_hrc::NurbsCurve {
    let n = m.i32(c + 4);
    let pts = m.u32(c + 0x2c);
    si_hrc::NurbsCurve {
        npoints: n,
        points: if pts == 0 { Vec::new() } else { (0..n.max(0) as u32).map(|k| { let at = pts + 0x24 * k; si_hrc::NurbsPoint { xyzw: [m.f64(at), m.f64(at + 8), m.f64(at + 0x10), m.f64(at + 0x18)], ..Default::default() } }).collect() },
        ..Default::default()
    }
}

/// [soft 0x08435d3c SI_faceOrdinatePoints]: every contour's normal by Newell's sums over
/// its points (the value form's arithmetic: [`crate::face::face_ordinate_points`]); the
/// outline's is the reference (with `face_up`, an outline whose normal has a negative z is
/// turned round, `SI_splineInverse` or `SI_nurbsCrvInverse`, and the reference negated);
/// every hole whose normal has a product with it above 0.0001 is turned round. A contour
/// without points is passed over. When the outline has none, the original compares the
/// holes with a reference it never set (zeros here, counted by `env::stale`).
pub fn face_ordinate_points(m: &mut Mem, o: &mut dyn Outside, face: u32, face_up: i16) {
    use si_x87::x;
    let mut reference: Option<[f32; 3]> = None;
    let mut rule = |first: bool, n: [f32; 3]| -> bool {
        if first {
            reference = Some(n);
            if face_up != 0 && x(n[2]).lt(x(0.0f32)) {
                reference = Some([(-x(n[0])).to_f32(), (-x(n[1])).to_f32(), (-x(n[2])).to_f32()]);
                return true;
            }
            return false;
        }
        let r = reference.unwrap_or_else(|| {
            crate::env::stale::note();
            [0.0; 3]
        });
        crate::face::dot_above(r, n)
    };
    let count = m.i16(face + 0x78) as i32;
    if m.i16(face + 0x84) == 0 {
        for k in 0..count.max(0) as u32 {
            let c = m.u32(face + 0x80).wrapping_add(100 * k);
            if let Some(n) = crate::face::curve_normal(&curve_value(m, c)) {
                if rule(k == 0, n) {
                    o.call(m, &SI_NURBS_CRV_INVERSE, &[c]);
                }
            }
        }
        return;
    }
    if count < 0 {
        return;
    }
    for which in -1..count {
        let s = if which == -1 { face + 4 } else { m.u32(face + 0x7c).wrapping_add(0x74 * which as u32) };
        if let Some(n) = crate::face::spline_normal(&spline_value(m, s)) {
            if rule(which == -1, n) {
                o.call(m, &SI_SPLINE_INVERSE, &[s]);
            }
        }
    }
}

/// [soft 0x08435750 SI_faceConvertToLinear]: a new face (`SI_faceAllocate`, `si-hrc`) of
/// straight pieces. NURBS face: a zeroed array of `count` curves of 100 bytes
/// (`SI_calloc`), each curve `SI_nurbsCrvConvertToLinear`, `SI_nurbsCrvInit` into its slot
/// and the converted one disposed of (`SI_nurbsCrvDispose`); the count, +0x84 = 0. Spline
/// face: a zeroed array of `count` splines of 0x74 bytes; the outline and each hole with
/// keys through `SI_splineConvertToLinear(spline, 1, step, 0.0)`, cleaned up
/// (`SI_splineCleanup`) when it was not linear and `cleanup` is set, transferred into its
/// place (`SI_splineTransfer`) and disposed of (`SI_splineDispose`); **an outline without
/// keys answers null** (the new face is lost). Then the count and
/// [`face_ordinate_points`] without the "face up" rule.
pub fn face_convert_to_linear(m: &mut Mem, o: &mut dyn Outside, face: u32, step: i16, cleanup: i16) -> u32 {
    let holder = m.local(8);
    m.set_u32(holder, 0);
    m.set_u32(holder + 4, 0);
    let new = si_hrc::image::spline::face_allocate(m).addr();
    if m.i16(face + 0x84) == 0 {
        let curves = m.si_calloc(m.i16(face + 0x78) as i32 as u32, 100);
        m.set_u32(new + 0x80, curves);
        let mut k: i16 = 0;
        while k < m.i16(face + 0x78) {
            let c = o.call(m, &SI_NURBS_CRV_CONVERT_TO_LINEAR, &[m.u32(face + 0x80).wrapping_add(100 * k as i32 as u32)]);
            m.set_u32(holder + 4, c);
            let slot = m.u32(new + 0x80).wrapping_add(100 * k as i32 as u32);
            o.call(m, &SI_NURBS_CRV_INIT, &[slot, c]);
            o.call(m, &SI_NURBS_CRV_DISPOSE, &[holder + 4]);
            k = k.wrapping_add(1);
        }
        m.set_i16(new + 0x78, m.i16(face + 0x78));
        m.set_i16(new + 0x84, 0);
        face_ordinate_points(m, o, new, 0);
    } else {
        let holes = m.si_calloc(m.i16(face + 0x78) as i32 as u32, 0x74);
        m.set_u32(new + 0x7c, holes);
        let mut k: i16 = -1;
        while -1 < m.i16(face + 0x78) {
            let (src, dst) = if k == -1 {
                if m.i16(face + 0x14) == 0 {
                    m.release(8);
                    return 0;
                }
                (face + 4, new + 4)
            } else {
                let src = m.u32(face + 0x7c).wrapping_add(0x74 * k as i32 as u32);
                if m.i16(src + 0x10) == 0 {
                    k = k.wrapping_add(1);
                    if m.i16(face + 0x78) <= k {
                        break;
                    }
                    continue;
                }
                (src, m.u32(new + 0x7c).wrapping_add(0x74 * k as i32 as u32))
            };
            let lin = o.call(m, &SI_SPLINE_CONVERT_TO_LINEAR, &[src, 1, step as i32 as u32, 0]);
            m.set_u32(holder, lin);
            if m.i32(src + 0xc) != 0 && cleanup != 0 {
                o.call(m, &SI_SPLINE_CLEANUP, &[lin]);
            }
            si_hrc::image::spline::spline_transfer(m, m.u32(holder), dst);
            si_hrc::image::spline::spline_dispose(m, holder);
            k = k.wrapping_add(1);
            if m.i16(face + 0x78) <= k {
                break;
            }
        }
        m.set_i16(new + 0x78, m.i16(face + 0x78));
        face_ordinate_points(m, o, new, 0);
    }
    m.release(8);
    new
}

/// [soft 0x08434828 SI_faceDispose]: the face `*pp` freed with what it owns, `*pp` null:
/// the name; for a NURBS face (+0x84 = 0) each curve's points (+0x2c) and knots (+0xc), not
/// the curve array; for a spline face the outline's keys (+0x18), each hole's keys and the
/// hole array; then the record.
pub fn face_dispose(m: &mut Mem, pp: u32) {
    let face = m.u32(pp);
    if face == 0 {
        return;
    }
    let free_at = |m: &mut Mem, at: u32| {
        let b = m.u32(at);
        if b != 0 {
            m.free(b);
            m.set_u32(at, 0);
        }
    };
    free_at(m, m.u32(pp));
    let face = m.u32(pp);
    if m.i16(face + 0x84) == 0 {
        if m.u32(face + 0x80) != 0 {
            let mut k: i16 = 0;
            while k < m.i16(m.u32(pp) + 0x78) {
                let c = m.u32(m.u32(pp) + 0x80).wrapping_add(100 * k as i32 as u32);
                free_at(m, c + 0x2c);
                free_at(m, c + 0xc);
                k += 1;
            }
        }
    } else {
        free_at(m, face + 0x18);
        let face = m.u32(pp);
        if m.u32(face + 0x7c) != 0 {
            let mut k: i16 = 0;
            while k < m.i16(m.u32(pp) + 0x78) {
                free_at(m, m.u32(m.u32(pp) + 0x7c).wrapping_add(0x74 * k as i32 as u32 + 0x14));
                k += 1;
            }
            free_at(m, m.u32(pp) + 0x7c);
        }
    }
    free_at(m, pp);
    m.set_u32(pp, 0);
}

/// [soft 0x08435960 SI_faceConvertToMesh]: a new mesh (`SI_meshAllocate`, first), the face
/// made linear ([`face_convert_to_linear`] with step −1; null answers null and the new mesh
/// is lost), then:
///
/// - one vertex per point, contour after contour (`SI_vertexAllocate` of the total): of a
///   face of NURBS curves (+0x84 = 0) each curve's points as r32(coordinate × (1 / w)),
///   the coordinates and the weight doubles, the quotient kept in the register (the value
///   form's arithmetic, checked by emulation); of a face of splines the outline's keys (+4) and then each
///   hole's (+0x7c, 0x74 bytes each), the positions (+0xc of a 0x30-byte key) as words. A
///   negative hole count (bit 15 of the byte at +0x79) skips the splines;
/// - one polygon (`SI_polygonAllocate(1)`) of points + contours − 1 corners (NURBS: the
///   count includes the outline) or points + holes (splines), its corners a **zeroed**
///   block of that count rounded up to four (`SI_calloc`, not `SI_polygonNodeAllocate`),
///   numbered in order with −1 between contours;
/// - [`face_dispose`] of the linear face, `SI_meshUpdate`, `SI_meshInitVertexColors`.
pub fn face_convert_to_mesh(m: &mut Mem, o: &mut dyn Outside, face: u32, cleanup: i16) -> u32 {
    let holder = m.local(8);
    m.set_u32(holder, 0);
    let mut total: u32 = 0;
    let mesh = super::mesh_allocate(m);
    if mesh == 0 {
        m.release(8);
        return 0;
    }
    let lin = face_convert_to_linear(m, o, face, -1, cleanup);
    m.set_u32(holder, lin);
    if lin == 0 {
        m.release(8);
        return 0;
    }
    let nurbs = m.i16(face + 0x84) == 0;
    if nurbs {
        let n = m.i16(lin + 0x78) as i32;
        let curves = m.u32(lin + 0x80);
        for c in 0..n.max(0) as u32 {
            total = total.wrapping_add(m.u32(curves + 100 * c + 4));
        }
        m.set_u32(mesh + mo::NB_VERTICES, total);
        let mut out = si_hrc::image::geom::vertex_allocate(m, total as i32);
        m.set_u32(mesh + mo::VERTICES, out);
        let mut c = 0i32;
        while c < m.i16(lin + 0x78) as i32 {
            let curve = m.u32(lin + 0x80).wrapping_add(100 * c as u32);
            c += 1;
            let mut k = 0i32;
            while k < m.i32(curve + 4) {
                let pt = m.u32(curve + 0x2c).wrapping_add(0x24 * k as u32);
                // 1 / w stays in the register (as in the value form, checked by emulation)
                let w = si_x87::x(1.0f32) / si_x87::x(m.f64(pt + 0x18));
                let coord = |m: &Mem, off: u32| (si_x87::x(m.f64(pt + off)) * w).to_f32();
                let (x, y, z) = (coord(m, 0), coord(m, 8), coord(m, 0x10));
                m.set_f32(out, x);
                m.set_f32(out + 4, y);
                m.set_f32(out + 8, z);
                out = out.wrapping_add(mo::VERTEX);
                k += 1;
            }
        }
    } else {
        total = m.u16(lin + 0x14) as u32;
        let n = m.i16(lin + 0x78) as i32;
        let holes = m.u32(lin + 0x7c);
        for h in 0..n.max(0) as u32 {
            total = total.wrapping_add(m.u16(holes + 0x74 * h + 0x10) as u32);
        }
        m.set_u32(mesh + mo::NB_VERTICES, total);
        let mut out = si_hrc::image::geom::vertex_allocate(m, total as i32);
        m.set_u32(mesh + mo::VERTICES, out);
        if m.u8(lin + 0x79) & 0x80 == 0 {
            let mut h = -1i32;
            loop {
                let spline = if h == -1 { lin + 4 } else { m.u32(lin + 0x7c).wrapping_add(0x74 * h as u32) };
                h += 1;
                let mut k = 0u32;
                while k < m.u16(spline + 0x10) as u32 {
                    let key = m.u32(spline + 0x14).wrapping_add(0x30 * k);
                    for off in [0, 4, 8] {
                        m.set_u32(out + off, m.u32(key + 0xc + off));
                    }
                    out = out.wrapping_add(mo::VERTEX);
                    k += 1;
                }
                if m.i16(lin + 0x78) as i32 <= h {
                    break;
                }
            }
        }
    }
    m.set_i32(mesh + mo::NB_POLYGONS, 1);
    let poly = si_hrc::image::geom::polygon_allocate(m, 1);
    m.set_u32(mesh + mo::POLYGONS, poly);
    let contours = m.i16(lin + 0x78);
    let count = if nurbs { (total as i16).wrapping_add(contours).wrapping_sub(1) } else { (total as i16).wrapping_add(contours) };
    m.set_i16(poly + mo::POLYGON_NB_NODES, count);
    let n = count as u16 as u32;
    let nodes = m.si_calloc(n.wrapping_sub((n.wrapping_sub(1) & 3).wrapping_sub(3)), mo::NODE);
    m.set_u32(poly + mo::POLYGON_NODES, nodes);
    let mut number = 0i32;
    let mut at = 0u32;
    let mut put = |m: &mut Mem, v: i32| {
        let nodes = m.u32(poly + mo::POLYGON_NODES);
        m.set_i32(nodes.wrapping_add(at), v);
        at = at.wrapping_add(mo::NODE);
    };
    if nurbs {
        let mut c = 0i32;
        while c < m.i16(lin + 0x78) as i32 {
            let curve = m.u32(lin + 0x80).wrapping_add(100 * c as u32);
            if c != 0 {
                put(m, -1);
            }
            c += 1;
            for _ in 0..m.i32(curve + 4).max(0) {
                put(m, number);
                number += 1;
            }
        }
    } else if m.u8(lin + 0x79) & 0x80 == 0 {
        let mut h = -1i32;
        loop {
            let spline = if h == -1 {
                lin + 4
            } else {
                put(m, -1);
                m.u32(lin + 0x7c).wrapping_add(0x74 * h as u32)
            };
            h += 1;
            for _ in 0..m.u16(spline + 0x10) {
                put(m, number);
                number += 1;
            }
            if m.i16(lin + 0x78) as i32 <= h {
                break;
            }
        }
    }
    face_dispose(m, holder);
    super::rebuild::update(m, o, mesh);
    super::obj::mesh_init_vertex_colors(m, mesh);
    m.release(8);
    mesh
}
