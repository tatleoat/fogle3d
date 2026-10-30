// SPDX-License-Identifier: MIT
//! `shape.c`: shape animation. A model node keeps its shapes as definitions of the same
//! geometry (finding 0030); the routines here make the working definition from the keys.
//!
//! Three families, one routine per kind of geometry (mesh, patch, face, spline, NURBS curve,
//! NURBS surface): linear between two keys, cardinal through four, and weighted over all.

use crate::outside::{self as out, dw, Outside};
use crate::{lerp, lerp_at};
use si_mem::{Mem, Ptr32};
use si_types::geom::{Face, Mesh, NurbsCrv, NurbsSrf, Patch, Spline};
use si_x87::{x, Round, F80};

/// The smallest weight a NURBS point is given before two keys are blended: the double at
/// 0x0864f7d8 (and again at 0x0864f7e0).
pub const MIN_WEIGHT: f64 = 1e-5;

/// Offsets used below that are not fields of a typed record: a vertex is 0x14 bytes with
/// its position first, a polygon 0x18 bytes with its corners at +0xc and their number at
/// +0x10, a corner 0x24 bytes with its normal at +8, a spline key 0x30 bytes of which the
/// first 0x24 are three vectors, a patch point 0x10 bytes, a NURBS point 0x24 bytes of four
/// doubles and a flag word (`si_types::geom`).
const VERTEX: u32 = 0x14;
const POLYGON: u32 = 0x18;
const POLY_NODES: u32 = 0x0c;
const POLY_NB_NODES: u32 = 0x10;
const NODE: u32 = 0x24;
const NODE_NORMAL: u32 = 0x08;
const KEY: u32 = 0x30;
const PATCH_POINT: u32 = 0x10;
const NPOINT: u32 = 0x24;
const SPLINE: u32 = 0x74;
const SPLINE_NB_KEYS: u32 = 0x10;
const SPLINE_KEYS: u32 = 0x14;
const CURVE: u32 = 0x64;
const CURVE_NB_POINTS: u32 = 0x04;
const CURVE_POINTS: u32 = 0x2c;

/// [soft 0x0823dae0 si_meshInterpolate]
///
/// `dst` = `a` + (`b` − `a`) × `t`, for every vertex position and every corner normal. 0
/// when the three meshes do not have the same numbers of vertices and polygons. The
/// normals stop at the first polygon whose number of corners differs between `b` and
/// `dst` (the vertices are done by then, and `dst` is still returned).
pub fn mesh_interpolate(m: &mut Mem, a: Ptr32<Mesh>, b: Ptr32<Mesh>, t: f32, dst: Ptr32<Mesh>) -> Ptr32<Mesh> {
    let t = x(t);
    let nv = m[a].nb_vertices;
    if nv != m[dst].nb_vertices {
        return Ptr32::NULL;
    }
    let n = m[b].nb_vertices;
    if n != nv {
        return Ptr32::NULL;
    }
    let np = m[a].nb_polygons;
    if np != m[dst].nb_polygons || m[b].nb_polygons != np {
        return Ptr32::NULL;
    }
    let (mut pd, mut pa, mut pb) = (m[dst].vertices.addr(), m[a].vertices.addr(), m[b].vertices.addr());
    // (the original counts down from the number and stops at zero: a negative number runs on)
    let mut left = n as u32;
    while left != 0 {
        for c in [0, 4, 8] {
            lerp_at(m, pa + c, pb + c, t, pd + c);
        }
        pa = pa.wrapping_add(VERTEX);
        pb = pb.wrapping_add(VERTEX);
        pd = pd.wrapping_add(VERTEX);
        left -= 1;
    }
    let (mut qa, mut qd, mut qb) = (m[a].polygons.addr(), m[dst].polygons.addr(), m[b].polygons.addr());
    let mut left = m[dst].nb_polygons as u32;
    while left != 0 {
        if m.u16(qb + POLY_NB_NODES) != m.u16(qd + POLY_NB_NODES) {
            break;
        }
        left -= 1;
        if m.u16(qd + POLY_NB_NODES) != 0 {
            let mut k: i32 = 0;
            let mut off = NODE_NORMAL;
            loop {
                for c in [0, 4, 8] {
                    // the three corner arrays are fetched again for every coordinate
                    let (na, nb) = (m.u32(qa + POLY_NODES), m.u32(qb + POLY_NODES));
                    let v = lerp(m.f32(na.wrapping_add(off + c)), m.f32(nb.wrapping_add(off + c)), t);
                    let nd = m.u32(qd + POLY_NODES);
                    m.set_f32(nd.wrapping_add(off + c), v);
                }
                off = off.wrapping_add(NODE);
                k += 1;
                if k >= m.u16(qd + POLY_NB_NODES) as i32 {
                    break;
                }
            }
        }
        qa = qa.wrapping_add(POLYGON);
        qb = qb.wrapping_add(POLYGON);
        qd = qd.wrapping_add(POLYGON);
    }
    dst
}

/// [soft 0x0823dc4c si_patchInterpolate]
///
/// The control points (three floats of each 16-byte point) blended, then the surface
/// rebuilt. 0 when the point counts differ.
pub fn patch_interpolate(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Patch>, b: Ptr32<Patch>, t: f32, dst: Ptr32<Patch>) -> Ptr32<Patch> {
    let t = x(t);
    let nu = m[a].nb_u;
    if nu != m[b].nb_u || nu != m[dst].nb_u {
        return Ptr32::NULL;
    }
    let nv = m[a].nb_v;
    if nv != m[b].nb_v || nv != m[dst].nb_v {
        return Ptr32::NULL;
    }
    let (mut pa, mut pb, mut pd) = (m[a].points.addr(), m[b].points.addr(), m[dst].points.addr());
    let mut left = (m[a].nb_u as u32).wrapping_mul(m[a].nb_v as u32);
    while left != 0 {
        for c in [0, 4, 8] {
            lerp_at(m, pa + c, pb + c, t, pd + c);
        }
        pa = pa.wrapping_add(PATCH_POINT);
        pb = pb.wrapping_add(PATCH_POINT);
        pd = pd.wrapping_add(PATCH_POINT);
        left -= 1;
    }
    o.call(m, &out::SI_PATCH_COMPUTE_SURFACE, &[dst.addr(), 0]);
    dst
}

/// The nine floats of one spline key blended, in memory order.
fn key_lerp(m: &mut Mem, ka: u32, kb: u32, t: F80, kd: u32) {
    for c in (0..0x24).step_by(4) {
        lerp_at(m, ka + c, kb + c, t, kd + c);
    }
}

/// [soft 0x0823dd04 si_faceInterpolate]
///
/// A face made of splines: the outline, then each hole, key by key (nine floats a key; no
/// spline is touched afterwards). A face made of NURBS curves: per control point the four
/// doubles are blended as they are (homogeneous), the weight is stored in `dst` directly
/// and the point is moved to (x, y, z) / weight through `SI_nurbsCrvMovePoint`. 0 when the
/// numbers of holes differ.
pub fn face_interpolate(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Face>, b: Ptr32<Face>, t: f32, dst: Ptr32<Face>) -> Ptr32<Face> {
    let t = x(t);
    let n = m[a].nb_holes;
    if n != m[b].nb_holes || n != m[dst].nb_holes {
        return Ptr32::NULL;
    }
    if m[a].is_spline != 0 {
        let mut i: i16 = -1;
        if i >= n {
            return dst;
        }
        loop {
            let (sa, sb, sd) = if i == -1 {
                (a.addr().wrapping_add(4), b.addr().wrapping_add(4), dst.addr().wrapping_add(4))
            } else {
                let off = (i as i32).wrapping_mul(SPLINE as i32) as u32;
                (m[a].holes.addr().wrapping_add(off), m[b].holes.addr().wrapping_add(off), m[dst].holes.addr().wrapping_add(off))
            };
            let (mut kd, mut ka, mut kb) = (m.u32(sd + SPLINE_KEYS), m.u32(sa + SPLINE_KEYS), m.u32(sb + SPLINE_KEYS));
            let next = i.wrapping_add(1);
            let mut k: i16 = 0;
            if 0 < m.u16(sa + SPLINE_NB_KEYS) as i32 {
                loop {
                    key_lerp(m, ka, kb, t, kd);
                    ka = ka.wrapping_add(KEY);
                    kb = kb.wrapping_add(KEY);
                    kd = kd.wrapping_add(KEY);
                    k = k.wrapping_add(1);
                    if k as i32 >= m.u16(sa + SPLINE_NB_KEYS) as i32 {
                        break;
                    }
                }
            }
            i = next;
            if i >= m[dst].nb_holes {
                break;
            }
        }
    } else {
        let mut i: i16 = 0;
        if i >= n {
            return dst;
        }
        loop {
            let off = (i as i32).wrapping_mul(CURVE as i32) as u32;
            let ca = m[a].curves.addr().wrapping_add(off);
            let cb = m[b].curves.addr().wrapping_add(off);
            let cd = m[dst].curves.addr().wrapping_add(off);
            let next = i.wrapping_add(1);
            let mut k: i16 = 0;
            if m.i32(ca + CURVE_NB_POINTS) > 0 {
                loop {
                    let e = (k as i32 as u32).wrapping_mul(NPOINT);
                    let mut h = [F80::ZERO; 4];
                    for (c, v) in h.iter_mut().enumerate() {
                        // the two point arrays are fetched again for every coordinate
                        let (pa, pb) = (m.u32(ca + CURVE_POINTS), m.u32(cb + CURVE_POINTS));
                        let at = e.wrapping_add(8 * c as u32);
                        let va = x(m.f64(pa.wrapping_add(at)));
                        *v = va + (x(m.f64(pb.wrapping_add(at))) - va) * t;
                    }
                    let (px, py, pz) = ((h[0] / h[3]).to_f64(), (h[1] / h[3]).to_f64(), (h[2] / h[3]).to_f64());
                    let pd = m.u32(cd + CURVE_POINTS);
                    m.set_f64(pd.wrapping_add(e + 0x18), h[3].to_f64());
                    let (wx, wy, wz) = (dw(px), dw(py), dw(pz));
                    o.call(m, &out::SI_NURBS_CRV_MOVE_POINT, &[cd, k as i32 as u32, wx[0], wx[1], wy[0], wy[1], wz[0], wz[1]]);
                    k = k.wrapping_add(1);
                    if k as i32 >= m.i32(ca + CURVE_NB_POINTS) {
                        break;
                    }
                }
            }
            i = next;
            if i >= m[dst].nb_holes {
                break;
            }
        }
    }
    dst
}

/// One control point of a key as the two NURBS routines prepare it: x, y, z divided by the
/// weight (each quotient rounded to double) unless the weight is zero, and a weight below
/// [`MIN_WEIGHT`] replaced by it (a NaN weight stays).
fn npoint_euclid(m: &Mem, p: u32) -> [f64; 4] {
    let mut v = [m.f64(p), m.f64(p.wrapping_add(8)), m.f64(p.wrapping_add(0x10)), m.f64(p.wrapping_add(0x18))];
    let w = x(v[3]);
    if !w.eq(F80::ZERO) {
        for c in v.iter_mut().take(3) {
            *c = (x(*c) / w).to_f64();
        }
    }
    if x(MIN_WEIGHT).gt(x(v[3])) {
        v[3] = MIN_WEIGHT;
    }
    v
}

/// The four blended coordinates of one control point: a + (b − a) × t on doubles, in the
/// register (rounded to double where the caller stores them).
fn npoint_blend(m: &Mem, pa: u32, pb: u32, t: F80) -> [F80; 4] {
    let va = npoint_euclid(m, pa);
    let vb = npoint_euclid(m, pb);
    let mut r = [F80::ZERO; 4];
    for c in 0..4 {
        r[c] = x(va[c]) + (x(vb[c]) - x(va[c])) * t;
    }
    r
}

/// [soft 0x0823dfc0 si_nurbsCrvInterpolate]
///
/// Per control point: both keys' points are taken out of homogeneous form (divided by
/// their weight unless it is zero; a weight below 1e-5 becomes 1e-5), blended, the weight
/// stored in `dst` and the point moved with `SI_nurbsCrvMovePoint`; then the knots are
/// recomputed. 0 for a null curve or different point counts.
pub fn nurbs_crv_interpolate(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<NurbsCrv>, b: Ptr32<NurbsCrv>, t: f32, dst: Ptr32<NurbsCrv>) -> Ptr32<NurbsCrv> {
    if dst.is_null() || a.is_null() || b.is_null() {
        return Ptr32::NULL;
    }
    let n = m[a].dir.nb_points;
    if n != m[dst].dir.nb_points {
        return Ptr32::NULL;
    }
    let nb = m[b].dir.nb_points;
    if nb != n {
        return Ptr32::NULL;
    }
    let t = x(t);
    if nb > 0 {
        let mut i: u32 = 0;
        loop {
            let k = i as u16 as i16 as i32;
            let e = (k as u32).wrapping_mul(NPOINT);
            let pa = m[a].points.addr().wrapping_add(e);
            let pb = m[b].points.addr().wrapping_add(e);
            let r = npoint_blend(m, pa, pb, t);
            let pd = m[dst].points.addr();
            m.set_f64(pd.wrapping_add(e + 0x18), r[3].to_f64());
            let (wx, wy, wz) = (dw(r[0].to_f64()), dw(r[1].to_f64()), dw(r[2].to_f64()));
            o.call(m, &out::SI_NURBS_CRV_MOVE_POINT, &[dst.addr(), k as u32, wx[0], wx[1], wy[0], wy[1], wz[0], wz[1]]);
            i = i.wrapping_add(1);
            if i as u16 as i16 as i32 >= m[a].dir.nb_points {
                break;
            }
        }
    }
    o.call(m, &out::SI_KNOTS_CRV_REPARAMETRIZATION, &[dst.addr()]);
    dst
}

/// [soft 0x0823e180 si_nurbsSrfInterpolate]
///
/// As for a curve, but nothing is stored in `dst` directly: the blended weight goes to
/// `SI_nurbsSrfMovePoint` as its fourth double.
pub fn nurbs_srf_interpolate(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<NurbsSrf>, b: Ptr32<NurbsSrf>, t: f32, dst: Ptr32<NurbsSrf>) -> Ptr32<NurbsSrf> {
    if dst.is_null() || a.is_null() || b.is_null() {
        return Ptr32::NULL;
    }
    let n = m[a].nb_points;
    if n != m[dst].nb_points {
        return Ptr32::NULL;
    }
    let nb = m[b].nb_points;
    if nb != n {
        return Ptr32::NULL;
    }
    let t = x(t);
    if nb > 0 {
        let mut i: u32 = 0;
        loop {
            let k = i as u16 as i16 as i32;
            let e = (k as u32).wrapping_mul(NPOINT);
            let pa = m[a].points.addr().wrapping_add(e);
            let pb = m[b].points.addr().wrapping_add(e);
            let r = npoint_blend(m, pa, pb, t);
            let (wx, wy, wz, ww) = (dw(r[0].to_f64()), dw(r[1].to_f64()), dw(r[2].to_f64()), dw(r[3].to_f64()));
            o.call(m, &out::SI_NURBS_SRF_MOVE_POINT, &[dst.addr(), k as u32, wx[0], wx[1], wy[0], wy[1], wz[0], wz[1], ww[0], ww[1]]);
            i = i.wrapping_add(1);
            if i as u16 as i16 as i32 >= m[a].nb_points {
                break;
            }
        }
    }
    o.call(m, &out::SI_KNOTS_SRF_REPARAMETRIZATION, &[dst.addr()]);
    dst
}

/// [soft 0x0823e340 si_splineInterpolate]
///
/// Every key's three vectors blended (the point, the vector after it, the vector before
/// it, in that order), then the spline touched. 0 when the key counts differ.
pub fn spline_interpolate(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Spline>, b: Ptr32<Spline>, t: f32, dst: Ptr32<Spline>) -> Ptr32<Spline> {
    let t = x(t);
    let n = m[a].nb_keys;
    if n != m[dst].nb_keys || m[b].nb_keys != n {
        return Ptr32::NULL;
    }
    let (mut kd, mut ka, mut kb) = (m[dst].keys.addr(), m[a].keys.addr(), m[b].keys.addr());
    let mut left = m[dst].nb_keys as u16 as u32;
    while left != 0 {
        for c in [0x0c, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x00, 0x04, 0x08] {
            lerp_at(m, ka + c, kb + c, t, kd + c);
        }
        ka = ka.wrapping_add(KEY);
        kb = kb.wrapping_add(KEY);
        kd = kd.wrapping_add(KEY);
        left -= 1;
    }
    o.call(m, &out::SI_SPLINE_TOUCH, &[dst.addr()]);
    dst
}

/// The cardinal blend of four values with four weights, as every cardinal routine sums it:
/// ((c0·p0 + c1·p1) + c2·p2) + c3·p3 in the register, one rounding.
pub(crate) fn card4(c: [f32; 4], p: [f32; 4]) -> f32 {
    (((x(c[0]) * p[0] + x(c[1]) * p[1]) + x(c[2]) * p[2]) + x(c[3]) * p[3]).to_f32()
}

/// The point the cardinal routines put in place of a missing neighbour, `a` being the next
/// key and `b`, `c` the two after it: ((a + a) − (b + b)) + c, rounded to float.
pub(crate) fn virt(a: f32, b: f32, c: f32) -> f32 {
    (((x(a) + a) - (x(b) + b)) + c).to_f32()
}

/// The four weights `splineCardInterp` uses: the cardinal weights of
/// [`crate::cluster::cardinal_coeff`] but with t³ unrounded and t² and ½t kept as doubles.
fn spline_card_weights(t: F80) -> [f32; 4] {
    let tt = t * t;
    let t2 = tt.to_f32();
    let ttt = tt * t;
    let d2 = t2 as f64;
    let dht = (t * 0.5f64).to_f64();
    let c3 = (ttt * 0.5f64 - F80::from(0.5f64) * d2).to_f32();
    let c0 = ((ttt * -0.5f64 + d2) - dht).to_f32();
    let c1 = ((ttt * 1.5f64 - x(d2) * 2.5f64) + F80::ONE).to_f32();
    let c2 = ((ttt * -1.5f64 + (x(t2) + x(t2))) + dht).to_f32();
    [c0, c1, c2, c3]
}

/// [soft 0x082427b0 splineCardInterp]
///
/// `splineCardInterp(dst, s0, s1, s2, s3, p0, p1, p2, p3, t)`: every key of `dst` (nine
/// floats) is the cardinal blend of the keys of `s0` … `s3` at `t`. When `p0` is `p1` the
/// first neighbour is made up from the others (2·s1 − 2·s2 + s3); when `p2` is `p3` the last
/// one (2·s2 − 2·s1 + s0). The `p` are what the caller compares (the splines, or the faces
/// they belong to).
#[allow(clippy::too_many_arguments)]
pub fn spline_card_interp(m: &mut Mem, dst: Ptr32<Spline>, s0: Ptr32<Spline>, s1: Ptr32<Spline>, s2: Ptr32<Spline>, s3: Ptr32<Spline>, p0: u32, p1: u32, p2: u32, p3: u32, t: f32) {
    let t = x(t);
    let mut kd = m.u32(dst.addr() + SPLINE_KEYS);
    let mut k0 = m.u32(s0.addr() + SPLINE_KEYS);
    let mut k1 = m.u32(s1.addr() + SPLINE_KEYS);
    let mut k2 = m.u32(s2.addr() + SPLINE_KEYS);
    let mut k3 = m.u32(s3.addr() + SPLINE_KEYS);
    if m.u16(dst.addr() + SPLINE_NB_KEYS) == 0 {
        return;
    }
    let mut i: i32 = 0;
    loop {
        let c = spline_card_weights(t);
        let comp = |m: &Mem, k: u32, j: u32| m.f32(k.wrapping_add(4 * j));
        let mut first = [0f32; 9];
        let mut last = [0f32; 9];
        for j in 0..9 {
            first[j as usize] = if p0 == p1 { virt(comp(m, k1, j), comp(m, k2, j), comp(m, k3, j)) } else { comp(m, k0, j) };
        }
        for j in 0..9 {
            last[j as usize] = if p2 == p3 { virt(comp(m, k2, j), comp(m, k1, j), comp(m, k0, j)) } else { comp(m, k3, j) };
        }
        for j in 0..9u32 {
            let v = card4(c, [first[j as usize], comp(m, k1, j), comp(m, k2, j), last[j as usize]]);
            m.set_f32(kd.wrapping_add(4 * j), v);
        }
        i += 1;
        k0 = k0.wrapping_add(KEY);
        k3 = k3.wrapping_add(KEY);
        kd = kd.wrapping_add(KEY);
        k1 = k1.wrapping_add(KEY);
        k2 = k2.wrapping_add(KEY);
        if i >= m.u16(dst.addr() + SPLINE_NB_KEYS) as i32 {
            break;
        }
    }
}

/// [soft 0x0823ef94 si_splineCardinalInterpolate]
///
/// The cardinal blend of four keys of a spline shape into `dst`; when the first two are
/// the same spline and the last two too, the linear blend of the first and the third. The
/// spline is touched. 0 when the key counts differ.
#[allow(clippy::too_many_arguments)]
pub fn spline_cardinal_interpolate(m: &mut Mem, o: &mut dyn Outside, k0: Ptr32<Spline>, k1: Ptr32<Spline>, k2: Ptr32<Spline>, k3: Ptr32<Spline>, t: f32, dst: Ptr32<Spline>) -> Ptr32<Spline> {
    let n = m[k0].nb_keys;
    if n != m[k1].nb_keys || n != m[dst].nb_keys {
        return Ptr32::NULL;
    }
    let n2 = m[k2].nb_keys;
    if n != n2 || n2 != m[k3].nb_keys {
        return Ptr32::NULL;
    }
    if k1 == k0 && k3 == k2 {
        let tt = x(t);
        let (mut kd, mut ka, mut kb) = (m[dst].keys.addr(), m[k0].keys.addr(), m[k2].keys.addr());
        let mut left = m[dst].nb_keys as u16 as u32;
        while left != 0 {
            for c in [0x0c, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x00, 0x04, 0x08] {
                lerp_at(m, ka + c, kb + c, tt, kd + c);
            }
            ka = ka.wrapping_add(KEY);
            kb = kb.wrapping_add(KEY);
            kd = kd.wrapping_add(KEY);
            left -= 1;
        }
    } else {
        spline_card_interp(m, dst, k0, k1, k2, k3, k0.addr(), k1.addr(), k2.addr(), k3.addr(), t);
    }
    o.call(m, &out::SI_SPLINE_TOUCH, &[dst.addr()]);
    dst
}

/// [soft 0x0823ed1c si_faceCardinalInterpolate]
///
/// Faces made of splines only (0 for any of the four keys made of NURBS curves, or for
/// different numbers of holes): the outline and every hole of `dst` blended as splines,
/// linearly from the second key to the third when the first two keys are the same face
/// and the last two too, else by `splineCardInterp`, which then compares the faces. No
/// spline is touched.
#[allow(clippy::too_many_arguments)]
pub fn face_cardinal_interpolate(m: &mut Mem, f0: Ptr32<Face>, f1: Ptr32<Face>, f2: Ptr32<Face>, f3: Ptr32<Face>, t: f32, dst: Ptr32<Face>) -> Ptr32<Face> {
    let n = m[f0].nb_holes;
    if n != m[f1].nb_holes || n != m[dst].nb_holes {
        return Ptr32::NULL;
    }
    let n2 = m[f2].nb_holes;
    if n != n2 || n2 != m[f3].nb_holes {
        return Ptr32::NULL;
    }
    if m[f0].is_spline == 0 || m[f1].is_spline == 0 || m[f2].is_spline == 0 || m[f3].is_spline == 0 {
        return Ptr32::NULL;
    }
    let spline_of = |m: &Mem, f: Ptr32<Face>, i: i32| -> u32 {
        if i == -1 {
            f.addr().wrapping_add(4)
        } else {
            m[f].holes.addr().wrapping_add((i as u32).wrapping_mul(SPLINE))
        }
    };
    if f1 == f0 && f3 == f2 {
        let tt = x(t);
        let mut i: i32 = -1;
        if m[dst].nb_holes < 0 {
            return dst;
        }
        loop {
            let (sa, sb, sd) = (spline_of(m, f1, i), spline_of(m, f2, i), spline_of(m, dst, i));
            let (mut ka, mut kb, mut kd) = (m.u32(sa + SPLINE_KEYS), m.u32(sb + SPLINE_KEYS), m.u32(sd + SPLINE_KEYS));
            let mut k: i32 = 0;
            i += 1;
            if m.u16(sd + SPLINE_NB_KEYS) != 0 {
                loop {
                    for c in [0x0c, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x00, 0x04, 0x08] {
                        lerp_at(m, ka + c, kb + c, tt, kd + c);
                    }
                    ka = ka.wrapping_add(KEY);
                    kb = kb.wrapping_add(KEY);
                    kd = kd.wrapping_add(KEY);
                    k += 1;
                    if k >= m.u16(sd + SPLINE_NB_KEYS) as i32 {
                        break;
                    }
                }
            }
            if i >= m[dst].nb_holes as i32 {
                break;
            }
        }
    } else {
        let mut i: i32 = -1;
        if m[dst].nb_holes < 0 {
            return dst;
        }
        loop {
            let sd = spline_of(m, dst, i);
            let s0 = spline_of(m, f0, i);
            let s1 = spline_of(m, f1, i);
            let s2 = spline_of(m, f2, i);
            let s3 = spline_of(m, f3, i);
            spline_card_interp(m, Ptr32::new(sd), Ptr32::new(s0), Ptr32::new(s1), Ptr32::new(s2), Ptr32::new(s3), f0.addr(), f1.addr(), f2.addr(), f3.addr(), t);
            i += 1;
            if i >= m[dst].nb_holes as i32 {
                break;
            }
        }
    }
    dst
}

/// [soft 0x0823ea48 si_patchCardinalInterpolate]
///
/// The cardinal blend of four patch keys, point by point (three floats each), with the
/// weights of `si_getCardinalCoeff`; a missing first or last neighbour (the first two, or
/// the last two, keys the same patch) made up as for splines; the linear blend of the first
/// and third key when both pairs are; then the surface is rebuilt. 0 when the point counts
/// differ.
#[allow(clippy::too_many_arguments)]
pub fn patch_cardinal_interpolate(m: &mut Mem, o: &mut dyn Outside, p0: Ptr32<Patch>, p1: Ptr32<Patch>, p2: Ptr32<Patch>, p3: Ptr32<Patch>, t: f32, dst: Ptr32<Patch>) -> Ptr32<Patch> {
    let nu = m[p0].nb_u;
    if nu != m[p1].nb_u || nu != m[dst].nb_u {
        return Ptr32::NULL;
    }
    let nu2 = m[p2].nb_u;
    if nu != nu2 || nu2 != m[p3].nb_u {
        return Ptr32::NULL;
    }
    let nv = m[p0].nb_v;
    if nv != m[p1].nb_v || nv != m[dst].nb_v {
        return Ptr32::NULL;
    }
    let nv2 = m[p2].nb_v;
    if nv != nv2 || nv2 != m[p3].nb_v {
        return Ptr32::NULL;
    }
    if p1 == p0 && p3 == p2 {
        let tt = x(t);
        let (mut pa, mut pb, mut pd) = (m[p0].points.addr(), m[p2].points.addr(), m[dst].points.addr());
        let mut left = (m[p0].nb_u as u32).wrapping_mul(m[p0].nb_v as u32);
        while left != 0 {
            for c in [0, 4, 8] {
                lerp_at(m, pa + c, pb + c, tt, pd + c);
            }
            pa = pa.wrapping_add(PATCH_POINT);
            pb = pb.wrapping_add(PATCH_POINT);
            pd = pd.wrapping_add(PATCH_POINT);
            left -= 1;
        }
    } else {
        let (mut a0, mut a1, mut a2, mut a3, mut ad) = (m[p0].points.addr(), m[p1].points.addr(), m[p2].points.addr(), m[p3].points.addr(), m[dst].points.addr());
        let w = crate::cluster::cardinal_coeff(t);
        let c = [w[2], w[3], w[4], w[5]];
        let mut left = (m[p1].nb_u as u32).wrapping_mul(m[p1].nb_v as u32);
        while left != 0 {
            let first: [f32; 3] = core::array::from_fn(|j| {
                let j = 4 * j as u32;
                if p1 != p0 { m.f32(a0 + j) } else { virt(m.f32(a1 + j), m.f32(a2 + j), m.f32(a3 + j)) }
            });
            let last: [f32; 3] = core::array::from_fn(|j| {
                let j = 4 * j as u32;
                if p3 != p2 { m.f32(a3 + j) } else { virt(m.f32(a2 + j), m.f32(a1 + j), m.f32(a0 + j)) }
            });
            for j in 0..3 {
                let at = 4 * j as u32;
                let v = card4(c, [first[j], m.f32(a1 + at), m.f32(a2 + at), last[j]]);
                m.set_f32(ad + at, v);
            }
            a1 = a1.wrapping_add(PATCH_POINT);
            a2 = a2.wrapping_add(PATCH_POINT);
            ad = ad.wrapping_add(PATCH_POINT);
            a0 = a0.wrapping_add(PATCH_POINT);
            a3 = a3.wrapping_add(PATCH_POINT);
            left -= 1;
        }
    }
    o.call(m, &out::SI_PATCH_COMPUTE_SURFACE, &[dst.addr(), 0]);
    dst
}

/// [soft 0x0823e430 si_meshCardinalInterpolate]
///
/// The cardinal blend of four mesh keys: vertex positions with the weights of
/// `si_getCardinalCoeff` and a made-up first or last neighbour as for splines, then corner
/// normals the same way; the linear blend of the first and the third key when the first two
/// are the same mesh and the last two too. 0 for a null mesh or different numbers of
/// vertices or polygons; the normals stop (and `dst` is returned) at the first polygon whose
/// number of corners is not the same in every mesh.
#[allow(clippy::too_many_arguments)]
pub fn mesh_cardinal_interpolate(m: &mut Mem, m0: Ptr32<Mesh>, m1: Ptr32<Mesh>, m2: Ptr32<Mesh>, m3: Ptr32<Mesh>, t: f32, dst: Ptr32<Mesh>) -> Ptr32<Mesh> {
    if m0.is_null() || m1.is_null() || m2.is_null() || m3.is_null() || dst.is_null() {
        return Ptr32::NULL;
    }
    let nv = m[m0].nb_vertices;
    if nv != m[dst].nb_vertices || m[m1].nb_vertices != nv || m[m2].nb_vertices != nv || m[m3].nb_vertices != nv {
        return Ptr32::NULL;
    }
    let np = m[m0].nb_polygons;
    if np != m[dst].nb_polygons || m[m1].nb_polygons != np || m[m2].nb_polygons != np || m[m3].nb_polygons != np {
        return Ptr32::NULL;
    }
    if m1 == m0 && m3 == m2 {
        let tt = x(t);
        let (mut pd, mut pa, mut pb) = (m[dst].vertices.addr(), m[m0].vertices.addr(), m[m2].vertices.addr());
        let mut left = m[m3].nb_vertices as u32;
        while left != 0 {
            for c in [0, 4, 8] {
                lerp_at(m, pa + c, pb + c, tt, pd + c);
            }
            pa = pa.wrapping_add(VERTEX);
            pb = pb.wrapping_add(VERTEX);
            pd = pd.wrapping_add(VERTEX);
            left -= 1;
        }
        let (mut qd, mut qa, mut qb) = (m[dst].polygons.addr(), m[m1].polygons.addr(), m[m2].polygons.addr());
        let mut left = m[dst].nb_polygons as u32;
        while left != 0 {
            let nn = m.u16(qd + POLY_NB_NODES);
            if m.u16(qa + POLY_NB_NODES) != nn || m.u16(qb + POLY_NB_NODES) != nn {
                break;
            }
            left -= 1;
            if m.u16(qd + POLY_NB_NODES) != 0 {
                let mut k: i32 = 0;
                let mut off = NODE_NORMAL;
                loop {
                    for c in [0, 4, 8] {
                        let (nb, na) = (m.u32(qb + POLY_NODES), m.u32(qa + POLY_NODES));
                        let nd = m.u32(qd + POLY_NODES);
                        let v = lerp(m.f32(na.wrapping_add(off + c)), m.f32(nb.wrapping_add(off + c)), tt);
                        m.set_f32(nd.wrapping_add(off + c), v);
                    }
                    off = off.wrapping_add(NODE);
                    k += 1;
                    if k >= m.u16(qd + POLY_NB_NODES) as i32 {
                        break;
                    }
                }
            }
            qd = qd.wrapping_add(POLYGON);
            qa = qa.wrapping_add(POLYGON);
            qb = qb.wrapping_add(POLYGON);
        }
        return dst;
    }
    let w = crate::cluster::cardinal_coeff(t);
    let c = [w[2], w[3], w[4], w[5]];
    let (mut a0, mut a1, mut a2, mut a3) = (m[m0].vertices.addr(), m[m1].vertices.addr(), m[m2].vertices.addr(), m[m3].vertices.addr());
    let mut ad = m[dst].vertices.addr();
    let mut left = m[dst].nb_vertices as u32;
    while left != 0 {
        let first: [f32; 3] = core::array::from_fn(|j| {
            let j = 4 * j as u32;
            if m1 != m0 { m.f32(a0 + j) } else { virt(m.f32(a1 + j), m.f32(a2 + j), m.f32(a3 + j)) }
        });
        let last: [f32; 3] = core::array::from_fn(|j| {
            let j = 4 * j as u32;
            if m3 != m2 { m.f32(a3 + j) } else { virt(m.f32(a2 + j), m.f32(a1 + j), m.f32(a0 + j)) }
        });
        for j in 0..3 {
            let at = 4 * j as u32;
            let v = card4(c, [first[j], m.f32(a1 + at), m.f32(a2 + at), last[j]]);
            m.set_f32(ad + at, v);
        }
        a3 = a3.wrapping_add(VERTEX);
        a1 = a1.wrapping_add(VERTEX);
        a2 = a2.wrapping_add(VERTEX);
        ad = ad.wrapping_add(VERTEX);
        a0 = a0.wrapping_add(VERTEX);
        left -= 1;
    }
    let (mut qd, mut q0, mut q2, mut q1, mut q3) = (m[dst].polygons.addr(), m[m0].polygons.addr(), m[m2].polygons.addr(), m[m1].polygons.addr(), m[m3].polygons.addr());
    let mut left = m[dst].nb_polygons as u32;
    while left != 0 {
        let nn = m.u16(qd + POLY_NB_NODES);
        if m.u16(q0 + POLY_NB_NODES) != nn || m.u16(q1 + POLY_NB_NODES) != nn || m.u16(q2 + POLY_NB_NODES) != nn || m.u16(q3 + POLY_NB_NODES) != nn {
            break;
        }
        left -= 1;
        if m.u16(qd + POLY_NB_NODES) != 0 {
            let mut k: i32 = 0;
            let mut off = NODE_NORMAL;
            loop {
                // every corner array is fetched again for every value
                let node = |m: &Mem, q: u32, j: u32| m.f32(m.u32(q + POLY_NODES).wrapping_add(off + j));
                let first: [f32; 3] = core::array::from_fn(|j| {
                    let j = 4 * j as u32;
                    if m1 != m0 { node(m, q0, j) } else { virt(node(m, q1, j), node(m, q2, j), node(m, q3, j)) }
                });
                let last: [f32; 3] = core::array::from_fn(|j| {
                    let j = 4 * j as u32;
                    if m3 != m2 { node(m, q3, j) } else { virt(node(m, q2, j), node(m, q1, j), node(m, q0, j)) }
                });
                for j in 0..3 {
                    let at = 4 * j as u32;
                    let nd = m.u32(qd + POLY_NODES);
                    let v = card4(c, [first[j], node(m, q1, at), node(m, q2, at), last[j]]);
                    m.set_f32(nd.wrapping_add(off + at), v);
                }
                k += 1;
                off = off.wrapping_add(NODE);
                if k >= m.u16(qd + POLY_NB_NODES) as i32 {
                    break;
                }
            }
        }
        q2 = q2.wrapping_add(POLYGON);
        qd = qd.wrapping_add(POLYGON);
        q0 = q0.wrapping_add(POLYGON);
        q1 = q1.wrapping_add(POLYGON);
        q3 = q3.wrapping_add(POLYGON);
    }
    dst
}

/// The weight a cardinal NURBS routine gives a blended point: the blended value rounded
/// to double, or 1e-5 when the unrounded value is below that.
fn clamp_weight(w: F80) -> f64 {
    if w.lt(x(MIN_WEIGHT)) { MIN_WEIGHT } else { w.to_f64() }
}

/// What the two cardinal NURBS routines share; `surface` says which (the point counts at
/// +4 or +0x48, the points at +0x2c or +0x54; a curve's weight is stored in place and the
/// point moved with three doubles, a surface's point with four).
#[allow(clippy::too_many_arguments)]
fn nurbs_cardinal(m: &mut Mem, o: &mut dyn Outside, k: [u32; 4], t: f32, dst: u32, surface: bool) -> u32 {
    let (count, points) = if surface { (0x48, 0x54) } else { (CURVE_NB_POINTS, CURVE_POINTS) };
    let n = m.i32(dst + count);
    let n0 = m.i32(k[0] + count);
    if m.i32(k[1] + count) != n0 || n != n0 || m.i32(k[2] + count) != n || m.i32(k[3] + count) != n {
        return 0;
    }
    let mut left = n as u32;
    let mut i: u32 = 0;
    let call = |m: &mut Mem, o: &mut dyn Outside, i: u32, v: [F80; 4]| {
        let wl = clamp_weight(v[3]);
        let (wx, wy, wz) = (dw(v[0].to_f64()), dw(v[1].to_f64()), dw(v[2].to_f64()));
        if surface {
            let ww = dw(wl);
            o.call(m, &out::SI_NURBS_SRF_MOVE_POINT, &[dst, i, wx[0], wx[1], wy[0], wy[1], wz[0], wz[1], ww[0], ww[1]]);
        } else {
            let pd = m.u32(dst + points);
            m.set_f64(pd.wrapping_add(i.wrapping_mul(NPOINT)).wrapping_add(0x18), wl);
            o.call(m, &out::SI_NURBS_CRV_MOVE_POINT, &[dst, i, wx[0], wx[1], wy[0], wy[1], wz[0], wz[1]]);
        }
    };
    if k[1] == k[0] && k[3] == k[2] {
        let tt = x(t);
        let (mut pa, mut pb) = (m.u32(k[0] + points), m.u32(k[2] + points));
        while left != 0 {
            let v = npoint_blend(m, pa, pb, tt);
            call(m, o, i, v);
            i = i.wrapping_add(1);
            pa = pa.wrapping_add(NPOINT);
            pb = pb.wrapping_add(NPOINT);
            left -= 1;
        }
        return dst;
    }
    let (mut p0, mut p1, mut p2, mut p3) = (m.u32(k[0] + points), m.u32(k[1] + points), m.u32(k[2] + points), m.u32(k[3] + points));
    let w = crate::cluster::cardinal_coeff(t);
    let c = [x(w[2]), x(w[3]), x(w[4]), x(w[5])];
    while left != 0 {
        let a0 = npoint_euclid(m, p0);
        let a1 = npoint_euclid(m, p1);
        let a2 = npoint_euclid(m, p2);
        let a3 = npoint_euclid(m, p3);
        let two = |v: f64| x(v) + v;
        // the first neighbour: some coordinates stay in the register, others are stored
        let f: [F80; 4] = if k[1] != k[0] {
            [x(a0[0]), x(a0[1]), x(a0[2]), x(a0[3])]
        } else {
            [
                (two(a1[0]) - two(a2[0])) + a3[0],
                x(((x(two(a1[1]).to_f64()) - two(a2[1])) + a3[1]).to_f64()),
                x(((two(a1[2]) - two(a2[2])) + a3[2]).to_f64()),
                x(((two(a1[3]) - two(a2[3])) + a3[3]).to_f64()),
            ]
        };
        let l: [F80; 4] = if k[3] != k[2] {
            [x(a3[0]), x(a3[1]), x(a3[2]), x(a3[3])]
        } else {
            [
                (two(a2[0]) - two(a1[0])) + a0[0],
                (two(a2[1]) - two(a1[1])) + a0[1],
                x(((two(a2[2]) - two(a1[2])) + a0[2]).to_f64()),
                x(((x(two(a2[3]).to_f64()) - two(a1[3])) + a0[3]).to_f64()),
            ]
        };
        let v: [F80; 4] = core::array::from_fn(|j| ((c[0] * f[j] + c[1] * a1[j]) + c[2] * a2[j]) + c[3] * l[j]);
        call(m, o, i, v);
        i = i.wrapping_add(1);
        p0 = p0.wrapping_add(NPOINT);
        p1 = p1.wrapping_add(NPOINT);
        p2 = p2.wrapping_add(NPOINT);
        p3 = p3.wrapping_add(NPOINT);
        left -= 1;
    }
    dst
}

/// [soft 0x0823f0cc si_nurbsCrvCardinalInterpolate]
///
/// The cardinal blend of four NURBS curve keys, point by point, in Euclidean coordinates
/// (each key's point divided by its weight unless that is zero, its weight raised to 1e-5
/// at least), with the weights of `si_getCardinalCoeff` and a made-up first or last
/// neighbour as for splines (2·next − 2·the one after + the third); the linear blend of the
/// first and the third key when both pairs are the same curve. The blended weight is raised
/// to 1e-5 again, stored in `dst` and the point moved with `SI_nurbsCrvMovePoint`. The knots
/// are not recomputed. 0 when the point counts differ.
#[allow(clippy::too_many_arguments)]
pub fn nurbs_crv_cardinal_interpolate(m: &mut Mem, o: &mut dyn Outside, k0: Ptr32<NurbsCrv>, k1: Ptr32<NurbsCrv>, k2: Ptr32<NurbsCrv>, k3: Ptr32<NurbsCrv>, t: f32, dst: Ptr32<NurbsCrv>) -> Ptr32<NurbsCrv> {
    Ptr32::new(nurbs_cardinal(m, o, [k0.addr(), k1.addr(), k2.addr(), k3.addr()], t, dst.addr(), false))
}

/// [soft 0x0823f77c si_nurbsSrfCardinalInterpolate]
///
/// The same for NURBS surfaces: the point goes to `SI_nurbsSrfMovePoint` with its weight.
#[allow(clippy::too_many_arguments)]
pub fn nurbs_srf_cardinal_interpolate(m: &mut Mem, o: &mut dyn Outside, k0: Ptr32<NurbsSrf>, k1: Ptr32<NurbsSrf>, k2: Ptr32<NurbsSrf>, k3: Ptr32<NurbsSrf>, t: f32, dst: Ptr32<NurbsSrf>) -> Ptr32<NurbsSrf> {
    Ptr32::new(nurbs_cardinal(m, o, [k0.addr(), k1.addr(), k2.addr(), k3.addr()], t, dst.addr(), true))
}

// ---------------------------------------------------------------- weighted shapes

use crate::globals as g;
use si_types::model::Model;

/// [soft 0x0824323c isFcvAffectedByExpr]
///
/// Whether a curve of `kind` is among the `count` entries (12 bytes each) of `list` that
/// expressions drive, `info` being the curve's own entry: kind 0x9a compares the first
/// word, 0xb7 the second, 0xb8 both; any other kind is never driven.
pub fn is_fcv_affected_by_expr(m: &Mem, kind: i32, count: i32, list: u32, info: u32) -> i32 {
    let mut e = list;
    let mut i = 0;
    while i < count {
        let hit = match kind {
            0xb7 => m.u32(info + 4) == m.u32(e.wrapping_add(4)),
            0xb8 => m.u32(info) == m.u32(e) && m.u32(info + 4) == m.u32(e.wrapping_add(4)),
            0x9a => m.u32(info) == m.u32(e),
            _ => false,
        };
        if hit {
            return 1;
        }
        e = e.wrapping_add(12);
        i += 1;
    }
    0
}

/// [soft 0x082432a8 si_fcrvValWithExpr]
///
/// A curve's value at `time`: the float at +0x14 of the curve when an expression drives it
/// (`isFcvAffectedByExpr` on the curve's kind, its first word), else `SI_fcurveValue`.
#[allow(clippy::too_many_arguments)]
pub fn fcrv_val_with_expr(m: &mut Mem, o: &mut dyn Outside, fcv: u32, time: f32, which: i16, count: i32, list: u32, info: u32) -> F80 {
    let kind = m.i32(fcv);
    if is_fcv_affected_by_expr(m, kind, count, list, info) as u16 != 0 {
        return x(m.f32(fcv.wrapping_add(0x14)));
    }
    o.call_f(m, &out::SI_FCURVE_VALUE, &[fcv, time.to_bits(), which as i32 as u32])
}

/// The three static variables of one weighted routine.
#[derive(Clone, Copy)]
struct Statics {
    count: u32,
    buffer: u32,
    list: u32,
}

const MESH_ST: Statics = Statics { count: g::MESH_NBWSHP.0, buffer: g::MESH_WFCV.0, list: g::MESH_WFCV_SUBID.0 };
const PATCH_ST: Statics = Statics { count: g::PATCH_NBWSHP.0, buffer: g::PATCH_WFCV.0, list: g::PATCH_WFCV_SUBID.0 };
const FACE_ST: Statics = Statics { count: g::FACE_NBWSHP.0, buffer: g::FACE_WFCV.0, list: g::FACE_WFCV_SUBID.0 };
const SPLINE_ST: Statics = Statics { count: g::SPLINE_NBWSHP.0, buffer: g::SPLINE_WFCV.0, list: g::SPLINE_WFCV_SUBID.0 };
const NCRV_ST: Statics = Statics { count: g::NCRV_NBWSHP.0, buffer: g::NCRV_WFCV.0, list: g::NCRV_WFCV_SUBID.0 };
const NSRF_ST: Statics = Statics { count: g::NSRF_NBWSHP.0, buffer: g::NSRF_WFCV.0, list: g::NSRF_WFCV_SUBID.0 };

/// The type of the shape weight curves (curve type 0xca of the SHAPES element).
const WEIGHT_CURVE: u32 = 0xca;

/// What every weighted routine has found before it moves a point.
struct Weights {
    elem: u32,
    sum: f32,
    /// The local whose address goes to `si_fcrvValWithExpr` as the curve's entry: the
    /// curve's number, then a word the original never sets.
    info: u32,
    st: Statics,
}

/// A weight curve's value, set to 0 when it is at most 1e-5 and the weights are
/// normalised (the 16-bit word at +0x2e2 of the model is 0).
fn weight_value(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, w: &Weights, fcv: u32, time: f32, i: i32) -> F80 {
    m.set_i32(w.info, i);
    let (count, list) = (m.i32(w.st.count), m.u32(w.st.list));
    let v = fcrv_val_with_expr(m, o, fcv, time, 0, count, list, w.info);
    if v.le(x(MIN_WEIGHT)) && m[model].unk_2e2 == 0 {
        F80::ZERO
    } else {
        v
    }
}

/// The start every weighted routine shares: nothing without a second definition, an
/// original definition and the model's SHAPES element; the curves expressions drive; then
/// the sum of the weights |v| of the definitions 1 … n − 1 (one curve each, in order). None
/// when the routine returns there (also when the sum is below 1e-5).
fn weights_start(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, upd: u32, time: f32, st: Statics) -> Option<Weights> {
    let mut sum: f32 = 0.0;
    if m[model].nb_defs <= 1 || m[model].org_def.is_null() {
        return None;
    }
    let index = m.local(4);
    let elem = o.call(m, &out::SI_ED_GET_ELEM_FROM_SHAPE, &[model.addr(), index]);
    if elem == 0 {
        return None;
    }
    let dict = g::cur_dict(m);
    let e = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[dict, m.i16(upd) as i32 as u32, m.u32(upd.wrapping_add(4))]);
    let buffer = m.u32(st.buffer);
    let r = o.call(m, &out::SI_GET_EXPR_FCURVE_USED, &[e, st.count, buffer, 0, st.list, 0x9a]);
    m.set_u32(st.buffer, r);
    let info = m.local(8);
    let w = Weights { elem, sum: 0.0, info, st };
    let mut fcv = o.call(m, &out::SI_ED_FIND_FCV, &[elem, 0, WEIGHT_CURVE]);
    let mut i: i32 = 0;
    while i < m[model].nb_defs as i32 - 1 {
        if fcv != 0 {
            let v = weight_value(m, o, model, &w, fcv, time, i);
            sum = (v.abs() + sum).to_f32();
        }
        fcv = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[elem, 0, WEIGHT_CURVE]);
        i += 1;
    }
    if x(sum).lt(x(MIN_WEIGHT)) {
        return None;
    }
    Some(Weights { sum, ..w })
}

/// The loop every weighted routine makes over the definitions 1 … n − 1 with their curves:
/// `body(key definition, weight)` for each whose weight |v| exceeds 1e-5, the weight
/// divided by the sum when the weights are normalised.
fn weights_each(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, w: &Weights, time: f32, mut body: impl FnMut(&mut Mem, &mut dyn Outside, u32, F80)) {
    let mut fcv = o.call(m, &out::SI_ED_FIND_FCV, &[w.elem, 0, WEIGHT_CURVE]);
    let mut def: i32 = 1;
    let mut i: i32 = 0;
    while def < m[model].nb_defs as i32 {
        let key = m.u32(m[model].defs.addr().wrapping_add(4 * def as u32));
        if fcv != 0 {
            let v = weight_value(m, o, model, w, fcv, time, i);
            if v.abs().gt(x(MIN_WEIGHT)) {
                let v = if m[model].unk_2e2 == 0 { v / w.sum } else { v };
                body(m, o, key, v);
            }
            fcv = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[w.elem, 0, WEIGHT_CURVE]);
        }
        i += 1;
        def += 1;
    }
}

/// The weight mode: the 16-bit word at +0x2e2 of the model (0: the weights are normalised
/// and the keys summed; 1: the keys' differences from the original definition are summed
/// onto it).
fn weight_mode(m: &Mem, model: Ptr32<Model>) -> u16 {
    m[model].unk_2e2 as u16
}

/// One point of `n` floats moved by a key: the key's point times the weight (mode 0), or
/// its difference from the original's times the weight (mode 1), each rounded to float,
/// added to the working point. Under another mode the values of the previous point (or,
/// at first, whatever the original's locals held) are added again; `d` carries them.
fn weight_floats(m: &mut Mem, model: Ptr32<Model>, work: u32, key: u32, org: u32, v: F80, offs: &[u32], d: &mut [f32]) {
    match weight_mode(m, model) {
        0 => {
            for (k, &c) in offs.iter().enumerate() {
                d[k] = (v * m.f32(key.wrapping_add(c))).to_f32();
            }
        }
        1 => {
            for (k, &c) in offs.iter().enumerate() {
                d[k] = ((x(m.f32(key.wrapping_add(c))) - m.f32(org.wrapping_add(c))) * v).to_f32();
            }
        }
        _ => {}
    }
    for (k, &c) in offs.iter().enumerate() {
        let a = work.wrapping_add(c);
        let s = (x(m.f32(a)) + d[k]).to_f32();
        m.set_f32(a, s);
    }
}

/// The original's uninitialised locals: `n` floats of the image's stack.
fn garbage(m: &mut Mem, n: usize) -> Vec<f32> {
    let at = m.local(4 * n as u32);
    (0..n as u32).map(|k| m.f32(at + 4 * k)).collect()
}

const OFFS3: [u32; 3] = [0, 4, 8];
const OFFS9: [u32; 9] = [0, 4, 8, 0x0c, 0x10, 0x14, 0x18, 0x1c, 0x20];

fn add_floats(m: &mut Mem, work: u32, org: u32, offs: &[u32]) {
    for &c in offs {
        let s = (x(m.f32(work.wrapping_add(c))) + m.f32(org.wrapping_add(c))).to_f32();
        m.set_f32(work.wrapping_add(c), s);
    }
}

fn zero_floats(m: &mut Mem, at: u32, offs: &[u32]) {
    for &c in offs {
        m.set_u32(at.wrapping_add(c), 0);
    }
}

/// [soft 0x08240d14 si_splineWeightInterpolate]
///
/// The working spline (definition 0) as the weighted sum of the shape keys (definitions 1
/// on), each key's weight being the value of its weight curve at `time`; see
/// `weights_start` and `weight_floats`. In mode 1 the original definition is added at the
/// end. The spline is touched. Nothing is done (not even the touch) without weights.
pub fn spline_weight_interpolate(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, upd: u32, time: f32) {
    let sp = m.sp();
    spline_weight_inner(m, o, model, upd, time);
    m.set_sp(sp);
}

fn spline_weight_inner(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, upd: u32, time: f32) {
    let mut d = garbage(m, 9);
    let Some(w) = weights_start(m, o, model, upd, time, SPLINE_ST) else { return };
    let org = m[model].org_def.addr();
    let work = m.u32(m[model].defs.addr());
    let mut k = m.u32(work + SPLINE_KEYS);
    let mut j: i32 = 0;
    while j < m.u16(work + SPLINE_NB_KEYS) as i32 {
        zero_floats(m, k, &[8, 4, 0, 0x14, 0x10, 0x0c, 0x20, 0x1c, 0x18]);
        j += 1;
        k = k.wrapping_add(KEY);
    }
    weights_each(m, o, model, &w, time, |m, _o, key, v| {
        let (mut kk, mut ko, mut kw) = (m.u32(key + SPLINE_KEYS), m.u32(org + SPLINE_KEYS), m.u32(work + SPLINE_KEYS));
        let mut j: i32 = 0;
        while j < m.u16(work + SPLINE_NB_KEYS) as i32 {
            weight_floats(m, model, kw, kk, ko, v, &OFFS9, &mut d);
            kk = kk.wrapping_add(KEY);
            ko = ko.wrapping_add(KEY);
            kw = kw.wrapping_add(KEY);
            j += 1;
        }
    });
    if weight_mode(m, model) == 1 {
        let (mut kw, mut ko) = (m.u32(work + SPLINE_KEYS), m.u32(org + SPLINE_KEYS));
        let mut j: i32 = 0;
        while j < m.u16(work + SPLINE_NB_KEYS) as i32 {
            add_floats(m, kw, ko, &OFFS9);
            kw = kw.wrapping_add(KEY);
            ko = ko.wrapping_add(KEY);
            j += 1;
        }
    }
    o.call(m, &out::SI_SPLINE_TOUCH, &[work]);
}

/// [soft 0x0824037c si_patchWeightInterpolate]
///
/// The same for a patch, point by point (three floats of each); the surface is rebuilt at
/// the end.
pub fn patch_weight_interpolate(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, upd: u32, time: f32) {
    let sp = m.sp();
    patch_weight_inner(m, o, model, upd, time);
    m.set_sp(sp);
}

fn patch_weight_inner(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, upd: u32, time: f32) {
    let mut d = garbage(m, 3);
    let Some(w) = weights_start(m, o, model, upd, time, PATCH_ST) else { return };
    let org = m[model].org_def.addr();
    let work = m.u32(m[model].defs.addr());
    let pw = Ptr32::<Patch>::new(work);
    let count = |m: &Mem| (m[pw].nb_u as u32).wrapping_mul(m[pw].nb_v as u32);
    let mut at = m[pw].points.addr();
    let mut left = count(m);
    while left != 0 {
        zero_floats(m, at, &[8, 4, 0]);
        at = at.wrapping_add(PATCH_POINT);
        left -= 1;
    }
    weights_each(m, o, model, &w, time, |m, _o, key, v| {
        let mut aw = m[pw].points.addr();
        let mut ak = m[Ptr32::<Patch>::new(key)].points.addr();
        let mut ao = m[Ptr32::<Patch>::new(org)].points.addr();
        let mut left = count(m);
        while left != 0 {
            weight_floats(m, model, aw, ak, ao, v, &OFFS3, &mut d);
            ak = ak.wrapping_add(PATCH_POINT);
            aw = aw.wrapping_add(PATCH_POINT);
            ao = ao.wrapping_add(PATCH_POINT);
            left -= 1;
        }
    });
    if weight_mode(m, model) == 1 {
        let mut aw = m[pw].points.addr();
        let mut ao = m[Ptr32::<Patch>::new(org)].points.addr();
        let mut left = count(m);
        while left != 0 {
            add_floats(m, aw, ao, &OFFS3);
            ao = ao.wrapping_add(PATCH_POINT);
            aw = aw.wrapping_add(PATCH_POINT);
            left -= 1;
        }
    }
    o.call(m, &out::SI_PATCH_COMPUTE_SURFACE, &[work, 0]);
}

/// [soft 0x0823fe04 si_meshWeightInterpolate]
///
/// The same for a mesh's vertex positions (mode 1 adds the original's positions at the
/// end); then every corner normal of the working mesh is set to zero and made the sum of
/// the keys' normals times their weights (in every mode, each sum rounded once), which
/// asks the weight curves a second time. No normal is recomputed.
pub fn mesh_weight_interpolate(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, upd: u32, time: f32) {
    let sp = m.sp();
    mesh_weight_inner(m, o, model, upd, time);
    m.set_sp(sp);
}

fn mesh_weight_inner(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, upd: u32, time: f32) {
    let mut d = garbage(m, 3);
    let Some(w) = weights_start(m, o, model, upd, time, MESH_ST) else { return };
    let org = m[model].org_def.addr();
    let work = m.u32(m[model].defs.addr());
    let mw = Ptr32::<Mesh>::new(work);
    let mut at = m[mw].vertices.addr();
    let mut left = m[mw].nb_vertices as u32;
    while left != 0 {
        zero_floats(m, at, &[8, 4, 0]);
        at = at.wrapping_add(VERTEX);
        left -= 1;
    }
    weights_each(m, o, model, &w, time, |m, _o, key, v| {
        let mut aw = m[mw].vertices.addr();
        let mut ak = m[Ptr32::<Mesh>::new(key)].vertices.addr();
        let mut ao = m[Ptr32::<Mesh>::new(org)].vertices.addr();
        let mut left = m[mw].nb_vertices as u32;
        while left != 0 {
            weight_floats(m, model, aw, ak, ao, v, &OFFS3, &mut d);
            ak = ak.wrapping_add(VERTEX);
            aw = aw.wrapping_add(VERTEX);
            ao = ao.wrapping_add(VERTEX);
            left -= 1;
        }
    });
    if weight_mode(m, model) == 1 {
        let mut aw = m[mw].vertices.addr();
        let mut ao = m[Ptr32::<Mesh>::new(org)].vertices.addr();
        let mut left = m[mw].nb_vertices as u32;
        while left != 0 {
            add_floats(m, aw, ao, &OFFS3);
            ao = ao.wrapping_add(VERTEX);
            aw = aw.wrapping_add(VERTEX);
            left -= 1;
        }
    }
    // the corner normals
    let mut q = m[mw].polygons.addr();
    let mut left = m[mw].nb_polygons as u32;
    while left != 0 {
        let mut k: i32 = 0;
        left -= 1;
        let mut off = NODE_NORMAL;
        while k < m.u16(q + POLY_NB_NODES) as i32 {
            let nodes = m.u32(q + POLY_NODES);
            zero_floats(m, nodes.wrapping_add(off), &OFFS3);
            off = off.wrapping_add(NODE);
            k += 1;
        }
        q = q.wrapping_add(POLYGON);
    }
    weights_each(m, o, model, &w, time, |m, _o, key, v| {
        let mut qw = m[mw].polygons.addr();
        let mut qk = m[Ptr32::<Mesh>::new(key)].polygons.addr();
        let mut left = m[mw].nb_polygons as u32;
        while left != 0 {
            left -= 1;
            let mut k: i32 = 0;
            let mut off = NODE_NORMAL;
            while k < m.u16(qw + POLY_NB_NODES) as i32 {
                for c in OFFS3 {
                    let nk = m.u32(qk + POLY_NODES);
                    let p = v * m.f32(nk.wrapping_add(off + c));
                    let nw = m.u32(qw + POLY_NODES);
                    let s = (p + m.f32(nw.wrapping_add(off + c))).to_f32();
                    m.set_f32(nw.wrapping_add(off + c), s);
                }
                k += 1;
                off = off.wrapping_add(NODE);
            }
            qw = qw.wrapping_add(POLYGON);
            qk = qk.wrapping_add(POLYGON);
        }
    });
}

/// The splines of a face made of splines: the outline (−1), then each hole.
fn face_spline(m: &Mem, face: u32, i: i32) -> u32 {
    if i == -1 {
        face.wrapping_add(4)
    } else {
        m[Ptr32::<Face>::new(face)].holes.addr().wrapping_add((i as u32).wrapping_mul(SPLINE))
    }
}

/// [soft 0x08240758 si_faceWeightInterpolate]
///
/// The same for a face made of splines (nothing for a face of NURBS curves: the original
/// definition decides), spline by spline and key by key, nine floats a key. The splines
/// visited are those of each key face (its number of holes); in mode 1 those of the last
/// key face get the original's added. No spline is touched.
pub fn face_weight_interpolate(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, upd: u32, time: f32) {
    let sp = m.sp();
    face_weight_inner(m, o, model, upd, time);
    m.set_sp(sp);
}

fn face_weight_inner(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, upd: u32, time: f32) {
    let mut d = garbage(m, 9);
    let Some(w) = weights_start(m, o, model, upd, time, FACE_ST) else { return };
    let org = m[model].org_def.addr();
    if m[Ptr32::<Face>::new(org)].is_spline == 0 {
        return;
    }
    let work = m.u32(m[model].defs.addr());
    let nh = |m: &Mem, f: u32| m[Ptr32::<Face>::new(f)].nb_holes;
    if nh(m, work) >= 0 {
        let mut i: i32 = -1;
        loop {
            let s = face_spline(m, work, i);
            let mut k = m.u32(s + SPLINE_KEYS);
            let mut j: i32 = 0;
            i += 1;
            while j < m.u16(s + SPLINE_NB_KEYS) as i32 {
                zero_floats(m, k, &[8, 4, 0, 0x14, 0x10, 0x0c, 0x20, 0x1c, 0x18]);
                j += 1;
                k = k.wrapping_add(KEY);
            }
            if i >= nh(m, work) as i32 {
                break;
            }
        }
    }
    let mut last: u32 = 0;
    {
        // the original keeps the key face of the last definition visited in a local
        let mut fcv = o.call(m, &out::SI_ED_FIND_FCV, &[w.elem, 0, WEIGHT_CURVE]);
        let mut def: i32 = 1;
        let mut ci: i32 = 0;
        while def < m[model].nb_defs as i32 {
            let key = m.u32(m[model].defs.addr().wrapping_add(4 * def as u32));
            last = key;
            if fcv != 0 {
                let v = weight_value(m, o, model, &w, fcv, time, ci);
                if v.abs().gt(x(MIN_WEIGHT)) {
                    let v = if m[model].unk_2e2 == 0 { v / w.sum } else { v };
                    if nh(m, key) >= 0 {
                        let mut i: i32 = -1;
                        loop {
                            let (sw, sk, so) = (face_spline(m, work, i), face_spline(m, key, i), face_spline(m, org, i));
                            let (mut kw, mut kk, mut ko) = (m.u32(sw + SPLINE_KEYS), m.u32(sk + SPLINE_KEYS), m.u32(so + SPLINE_KEYS));
                            let mut j: i32 = 0;
                            i += 1;
                            while j < m.u16(sw + SPLINE_NB_KEYS) as i32 {
                                weight_floats(m, model, kw, kk, ko, v, &OFFS9, &mut d);
                                kk = kk.wrapping_add(KEY);
                                ko = ko.wrapping_add(KEY);
                                j += 1;
                                kw = kw.wrapping_add(KEY);
                            }
                            if i >= nh(m, key) as i32 {
                                break;
                            }
                        }
                    }
                }
                fcv = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[w.elem, 0, WEIGHT_CURVE]);
            }
            ci += 1;
            def += 1;
        }
    }
    if weight_mode(m, model) == 1 && nh(m, last) >= 0 {
        let mut i: i32 = -1;
        loop {
            let (sw, so) = (face_spline(m, work, i), face_spline(m, org, i));
            let (mut kw, mut ko) = (m.u32(sw + SPLINE_KEYS), m.u32(so + SPLINE_KEYS));
            let mut j: i32 = 0;
            i += 1;
            while j < m.u16(sw + SPLINE_NB_KEYS) as i32 {
                add_floats(m, kw, ko, &OFFS9);
                ko = ko.wrapping_add(KEY);
                j += 1;
                kw = kw.wrapping_add(KEY);
            }
            if i >= nh(m, last) as i32 {
                break;
            }
        }
    }
}

/// The weighted NURBS routines: the working definition's control points set to zero
/// (weight included), then for every key with a weight: per point the key's point in
/// Euclidean form ([`npoint_euclid`]) times the weight (mode 0), or its difference from the
/// original's in Euclidean form, the key's own weight still times the weight (mode 1), each
/// rounded to double, added to the working point's four doubles. At the end every point is
/// moved: in mode 1 to the original's Euclidean point plus the sums, else to the sums
/// themselves. A curve's weight is stored in place, a surface's goes with the point. Then
/// the knots are recomputed.
fn nurbs_weight(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, upd: u32, time: f32, surface: bool) {
    let (count, points, st) = if surface { (0x48u32, 0x54u32, NSRF_ST) } else { (CURVE_NB_POINTS, CURVE_POINTS, NCRV_ST) };
    let dl = m.local(32);
    let mut d: [f64; 4] = core::array::from_fn(|k| m.f64(dl + 8 * k as u32));
    let Some(w) = weights_start(m, o, model, upd, time, st) else { return };
    let org = m[model].org_def.addr();
    let work = m.u32(m[model].defs.addr());
    let n = |m: &Mem| m.i32(work + count);
    let pt = |m: &Mem, def: u32, i: i32| m.u32(def + points).wrapping_add((i as u32).wrapping_mul(NPOINT));
    let mut i = 0;
    while i < n(m) {
        let p = pt(m, work, i);
        for c in [0x10, 0x14, 0x08, 0x0c, 0x00, 0x04] {
            m.set_u32(p.wrapping_add(c), 0);
        }
        let p = pt(m, work, i);
        m.set_u32(p + 0x18, 0);
        m.set_u32(p + 0x1c, 0x3ff0_0000);
        let p = pt(m, work, i);
        m.set_u32(p + 0x18, 0);
        m.set_u32(p + 0x1c, 0);
        i += 1;
    }
    weights_each(m, o, model, &w, time, |m, _o, key, v| {
        let mut i = 0;
        while i < n(m) {
            let a = npoint_euclid(m, pt(m, key, i));
            match weight_mode(m, model) {
                0 => {
                    d = [(x(a[0]) * v).to_f64(), (x(a[1]) * v).to_f64(), (x(a[2]) * v).to_f64(), (v * a[3]).to_f64()];
                }
                1 => {
                    let b = npoint_euclid(m, pt(m, org, i));
                    d = [((x(a[0]) - b[0]) * v).to_f64(), ((x(a[1]) - b[1]) * v).to_f64(), ((x(a[2]) - b[2]) * v).to_f64(), (v * a[3]).to_f64()];
                }
                _ => {}
            }
            for (k, dk) in d.iter().enumerate() {
                let p = pt(m, work, i).wrapping_add(8 * k as u32);
                let s = (x(m.f64(p)) + *dk).to_f64();
                m.set_f64(p, s);
            }
            i += 1;
        }
    });
    let mut i = 0;
    if weight_mode(m, model) == 1 {
        while i < n(m) {
            let b = npoint_euclid(m, pt(m, org, i));
            let s: [f64; 4] = core::array::from_fn(|k| (x(b[k]) + m.f64(pt(m, work, i).wrapping_add(8 * k as u32))).to_f64());
            let (wx, wy, wz) = (dw(s[0]), dw(s[1]), dw(s[2]));
            if surface {
                let ww = dw(s[3]);
                o.call(m, &out::SI_NURBS_SRF_MOVE_POINT, &[work, i as u32, wx[0], wx[1], wy[0], wy[1], wz[0], wz[1], ww[0], ww[1]]);
            } else {
                m.set_f64(pt(m, work, i) + 0x18, s[3]);
                o.call(m, &out::SI_NURBS_CRV_MOVE_POINT, &[work, i as u32, wx[0], wx[1], wy[0], wy[1], wz[0], wz[1]]);
            }
            i += 1;
        }
    } else {
        while i < n(m) {
            let p = pt(m, work, i);
            let s: [f64; 4] = core::array::from_fn(|k| m.f64(p.wrapping_add(8 * k as u32)));
            let (wx, wy, wz) = (dw(s[0]), dw(s[1]), dw(s[2]));
            if surface {
                let ww = dw(s[3]);
                o.call(m, &out::SI_NURBS_SRF_MOVE_POINT, &[work, i as u32, wx[0], wx[1], wy[0], wy[1], wz[0], wz[1], ww[0], ww[1]]);
            } else {
                o.call(m, &out::SI_NURBS_CRV_MOVE_POINT, &[work, i as u32, wx[0], wx[1], wy[0], wy[1], wz[0], wz[1]]);
            }
            i += 1;
        }
    }
    let r = if surface { &out::SI_KNOTS_SRF_REPARAMETRIZATION } else { &out::SI_KNOTS_CRV_REPARAMETRIZATION };
    o.call(m, r, &[work]);
}

/// [soft 0x082411c8 si_nurbsCrvWeightInterpolate]
///
/// See `nurbs_weight`.
pub fn nurbs_crv_weight_interpolate(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, upd: u32, time: f32) {
    let sp = m.sp();
    nurbs_weight(m, o, model, upd, time, false);
    m.set_sp(sp);
}

/// [soft 0x082417fc si_nurbsSrfWeightInterpolate]
///
/// See `nurbs_weight`.
pub fn nurbs_srf_weight_interpolate(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, upd: u32, time: f32) {
    let sp = m.sp();
    nurbs_weight(m, o, model, upd, time, true);
    m.set_sp(sp);
}

// ---------------------------------------------------------------- shape keys

/// The definition a model shows: `defs[cur_def]`.
fn cur_definition(m: &Mem, model: Ptr32<Model>) -> u32 {
    m.u32(m[model].defs.addr().wrapping_add(4 * m[model].cur_def as u32))
}

/// The status-line messages of `modelInterp` and `si_itemSetShapeKey`: strings of the
/// binary's data, passed by address.
const MSG_NOT_INTERPOLABLE: u32 = 0x086999e0;
const MSG_NO_DEFINITION: u32 = 0x08699994;
const MSG_INTERP_FAILED: u32 = 0x086999b0;

/// [soft 0x08242c04 modelInterp]
///
/// `dst`'s current definition = the linear blend at `t` of those of `a` and `b`, by `a`'s
/// kind (a mesh then gets its polygon normals recomputed). `dst`, or 0 when the blend
/// refused; for a kind without shapes a message, and `dst`.
pub fn model_interp(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Model>, b: Ptr32<Model>, t: f32, dst: Ptr32<Model>) -> Ptr32<Model> {
    let kind = m[a].kind;
    if ![1, 2, 4, 6, 9, 10].contains(&kind) {
        o.call(m, &out::SI_STATUS_DISPLAY_MESSAGE, &[MSG_NOT_INTERPOLABLE, 3]);
        return dst;
    }
    let (da, db, dd) = (cur_definition(m, a), cur_definition(m, b), cur_definition(m, dst));
    let r = match kind {
        4 => {
            let r = mesh_interpolate(m, Ptr32::new(da), Ptr32::new(db), t, Ptr32::new(dd)).addr();
            if r == 0 {
                return Ptr32::NULL;
            }
            let dd = cur_definition(m, dst);
            o.call(m, &out::SI_MESH_COMPUTES_POLYGON_NORMAL, &[dd, 0xffff_ffff]);
            return dst;
        }
        1 => patch_interpolate(m, o, Ptr32::new(da), Ptr32::new(db), t, Ptr32::new(dd)).addr(),
        2 => face_interpolate(m, o, Ptr32::new(da), Ptr32::new(db), t, Ptr32::new(dd)).addr(),
        6 => spline_interpolate(m, o, Ptr32::new(da), Ptr32::new(db), t, Ptr32::new(dd)).addr(),
        9 => nurbs_crv_interpolate(m, o, Ptr32::new(da), Ptr32::new(db), t, Ptr32::new(dd)).addr(),
        _ => nurbs_srf_interpolate(m, o, Ptr32::new(da), Ptr32::new(db), t, Ptr32::new(dd)).addr(),
    };
    if r == 0 {
        Ptr32::NULL
    } else {
        dst
    }
}

/// [soft 0x08242734 si_itemSetShapeKey]
///
/// The scene window's "set shape key" for a model (graph kinds 2 and 3 only): the model's
/// definition made `p`'s current definition (a blend at 0 of `p` with itself) and keyed
/// (parameter 0xc8); a message instead when the model has no definition or the blend
/// refused.
pub fn item_set_shape_key(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, kind: i32, p: Ptr32<Model>) {
    if kind != 2 && kind != 3 {
        return;
    }
    if m[model].definition.is_null() {
        o.call(m, &out::SI_STATUS_DISPLAY_MESSAGE, &[MSG_NO_DEFINITION, 3]);
        return;
    }
    if model_interp(m, o, p, p, 0.0, model).is_null() {
        o.call(m, &out::SI_STATUS_DISPLAY_MESSAGE, &[MSG_INTERP_FAILED, 3]);
        return;
    }
    o.call(m, &out::SI_SAVEKEY_MODEL, &[model.addr(), 2, 0xc8, 8, 1, 0, 0, 0]);
}

/// [soft 0x08242060 si_shapeWeightFcurveDispose]
///
/// Frees a model's shape weight curves (type 0xca in chapter `SI_cid[21]`).
pub fn shape_weight_fcurve_dispose(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) {
    let cid = m[g::SI_CID][21] as u32;
    crate::edanim::ed_fcv_dispose(m, o, model.addr(), cid, 0, WEIGHT_CURVE, WEIGHT_CURVE);
}

/// [soft 0x08243418 SI_shapeanimCheckTopoValidity]
///
/// Whether every definition of a model has as many points as the first: 0 at the first
/// that differs; 1 otherwise, and when the model has no element or a single definition.
pub fn shapeanim_check_topo_validity(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> i32 {
    let index = m.local(4);
    let e = o.call(m, &out::SI_ED_GET_ELEM_FROM_MODEL, &[model.addr(), index]);
    m.release(4);
    if e == 0 || m[model].nb_defs <= 1 {
        return 1;
    }
    let cid8 = m[g::SI_CID][8] as u32;
    let n0 = o.call(m, &out::SI_ELEM_GET_NB_VERTICES, &[e, cid8, 0]);
    let mut i: i32 = 0;
    while i < m[model].nb_defs as i32 {
        let cid8 = m[g::SI_CID][8] as u32;
        if o.call(m, &out::SI_ELEM_GET_NB_VERTICES, &[e, cid8, i as u32]) != n0 {
            return 0;
        }
        i += 1;
    }
    1
}

// ---------------------------------------------------------------- second pass: the keys' curves

/// The shape curve's type (one key per shape key: the definition shown over time).
const SHAPE_CURVE: u32 = 0xc9;
/// The type of a new weight curve when it is made (it becomes 0xca once in the list).
const NEW_WEIGHT_CURVE: u32 = 0x9a;
/// Offset of a function curve's 16-bit word that `SI_fkeyFrameCreate` is handed with every
/// new key (`fcurve` +0x1e; not established further).
const FCURVE_1E: u32 = 0x1e;
/// Offsets of a function curve's two words a reset clears (+8, +0xc; +0xc is tested as
/// the number of keys by `SI_shapeRemoveKey`).
const FCURVE_08: u32 = 0x08;
const FCURVE_0C: u32 = 0x0c;

/// [soft 0x08241e28 SI_checkShapeWeightFcurve]
///
/// When a model's SHAPES element has no weight curve (type 0xca, chapter 0), makes `n` of
/// them: each a new curve (`SI_fcurveAllocate`, type 0x9a) put in the element's list in
/// chapter `SI_cid[21]` as number 1, 2, … with one key of value 1 at time 0.
pub fn check_shape_weight_fcurve(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<si_types::model::Model>, n: i32) {
    let index = m.local(4);
    let e = o.call(m, &out::SI_ED_GET_ELEM_FROM_SHAPE, &[model.addr(), index]);
    if e != 0 && o.call(m, &out::SI_ED_FIND_FCV, &[e, 0, WEIGHT_CURVE]) == 0 && n > 0 {
        let mut i = 0;
        while i < n {
            let f = o.call(m, &out::SI_FCURVE_ALLOCATE, &[NEW_WEIGHT_CURVE, model.addr()]);
            i += 1;
            let cid = m[g::SI_CID][21] as u32;
            crate::edanim::ed_fcv_insert(m, o, f, e, cid, m.u32(index), i as i32);
            let w = m.u16(f.wrapping_add(FCURVE_1E)) as u32;
            let k = o.call(m, &out::SI_FKEY_FRAME_CREATE, &[0, 1f32.to_bits(), 0, 1, w]);
            o.call(m, &out::SI_FCURVE_ADD_KEY, &[f, k]);
        }
    }
    m.release(4);
}

/// [soft 0x08241edc SI_removeShapeWeightFcurve]
///
/// Takes the weight curve of shape key `n` away (after making the curves when there were
/// none, one per key but the first): the curves from number `n` on are given up
/// (`SI_edFcvFilterDispose`) and the `n`-th curve of the element's list (counted from 0
/// with `SI_edFindFcv` and `n − 1` times `SI_edFindNextFcv`) freed. Nothing for `n` outside
/// 0 … definitions − 1, or when the list ends first.
pub fn remove_shape_weight_fcurve(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<si_types::model::Model>, n: i32) {
    let nd = m[model].nb_defs as i32;
    check_shape_weight_fcurve(m, o, model, nd - 1);
    let index = m.local(4);
    let fl = m.local(4);
    let e = o.call(m, &out::SI_ED_GET_ELEM_FROM_SHAPE, &[model.addr(), index]);
    'done: {
        if e == 0 || n < 0 || n >= m[model].nb_defs as i32 {
            break 'done;
        }
        let f = o.call(m, &out::SI_ED_FIND_FCV, &[e, 0, WEIGHT_CURVE]);
        m.set_u32(fl, f);
        let mut i = 0;
        while i < n - 1 {
            let f = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[e, 0, WEIGHT_CURVE]);
            m.set_u32(fl, f);
            if f == 0 {
                break 'done;
            }
            i += 1;
        }
        let cid = m[g::SI_CID][21] as u32;
        crate::edanim::ed_fcv_filter_dispose(m, o, model.addr(), cid, 0, WEIGHT_CURVE, NEW_WEIGHT_CURVE, n as i32);
        o.call(m, &out::SI_FCURVE_REMOVE, &[fl]);
    }
    m.release(8);
}

/// [soft 0x08241fb0 si_resetShapeWeightFcurve]
///
/// Every weight curve of a model (made first when there were none) emptied and given one
/// key of value 0 at time 0; the curve's words at +8 and +0xc cleared.
pub fn reset_shape_weight_fcurve(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<si_types::model::Model>) {
    let nd = m[model].nb_defs as i32;
    check_shape_weight_fcurve(m, o, model, nd - 1);
    let index = m.local(4);
    let e = o.call(m, &out::SI_ED_GET_ELEM_FROM_SHAPE, &[model.addr(), index]);
    if e != 0 {
        let mut f = o.call(m, &out::SI_ED_FIND_FCV, &[e, 0, WEIGHT_CURVE]);
        while f != 0 {
            o.call(m, &out::SI_FCURVE_REMOVE_ALL_KEYS, &[f]);
            m.set_u32(f.wrapping_add(FCURVE_08), 0);
            m.set_u32(f.wrapping_add(FCURVE_0C), 0);
            let w = m.u16(f.wrapping_add(FCURVE_1E)) as u32;
            let k = o.call(m, &out::SI_FKEY_FRAME_CREATE, &[0, 0, 0, 1, w]);
            o.call(m, &out::SI_FCURVE_ADD_KEY, &[f, k]);
            f = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[e, 0, WEIGHT_CURVE]);
        }
    }
    m.release(4);
}

/// [soft 0x08242638 SI_shapeRemoveKey]
///
/// Takes shape key `n` away: when the model has more than one definition and
/// `SI_modelDeleteIDefinition` removes the `n`-th, the shape curve (type 0xc9) loses its
/// keys between n − 1 and n (when it has more than one) and every key value above n − 1
/// is lowered by 1. When one definition is left, the shape and weight curves are reset
/// (`SI_modelResetFcv`). Nothing without a SHAPES element or a shape curve.
pub fn shape_remove_key(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<si_types::model::Model>, n: i32) {
    let index = m.local(4);
    let e = o.call(m, &out::SI_ED_GET_ELEM_FROM_SHAPE, &[model.addr(), index]);
    let f = if e != 0 { o.call(m, &out::SI_ED_FIND_FCV, &[e, 0, SHAPE_CURVE]) } else { 0 };
    if f != 0 {
        if m[model].nb_defs > 1 && o.call(m, &out::SI_MODEL_DELETE_I_DEFINITION, &[model.addr(), n as u16 as u32]) as u16 != 0 {
            let below = (F80::from(n) - F80::ONE).to_f32().to_bits();
            if m.i32(f.wrapping_add(FCURVE_0C)) > 1 {
                o.call(m, &out::SI_FCURVE_REMOVE_KEY_BETWEEN, &[f, below, (F80::from(n).to_f32()).to_bits()]);
            }
            o.call(m, &out::SI_FCURVE_INC_VALUE_ABOVE, &[f, below, (-1f32).to_bits()]);
        }
        if m[model].nb_defs == 1 {
            let cid = m[g::SI_CID][21] as u32;
            o.call(m, &out::SI_MODEL_RESET_FCV, &[model.addr(), cid, 0, SHAPE_CURVE, 0, 0, 0, 0]);
            let cid = m[g::SI_CID][21] as u32;
            o.call(m, &out::SI_MODEL_RESET_FCV, &[model.addr(), cid, 0, WEIGHT_CURVE, 0, 0, 0, 0]);
        }
    }
    m.release(4);
}

/// [soft 0x082432f8 SI_shapeCleanAll]
///
/// After a dictionary is loaded: for every element of chapter `SI_cid[8]` that is a model
/// (word at +4) and whose word at +0x48 is 0, the shapes nothing animates are taken away.
/// A model with several definitions whose SHAPES element (related by type 0xc8) is missing
/// or not animated (`SI_edIsElementAnimated`) loses every definition but the first, from
/// the last down; a model with one definition and a SHAPES element has its shape
/// animation reset (`SI_modelResetShape`).
pub fn shape_clean_all(m: &mut Mem, o: &mut dyn Outside, ed: u32) {
    let chap = m.u32(ed.wrapping_add(4)).wrapping_add((m[g::SI_CID][8] as u32) << 6);
    let mut i: i32 = 0;
    while i < m.i32(chap.wrapping_add(4)) {
        let el = m.u32(chap.wrapping_add(8)).wrapping_add((i as u32).wrapping_mul(0x84));
        let model = m.u32(el.wrapping_add(4));
        if m.u16(el.wrapping_add(0x48)) == 0 && model != 0 {
            let mut reset: i16 = 0;
            let l = m.local(4);
            let cid = m[g::SI_CID][21] as u32;
            let r = o.call(m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[ed, el, 1, 2, 0xc8, cid, l, l]);
            m.release(4);
            let nd = m.u16(model.wrapping_add(0x34));
            let delete = if nd <= 1 {
                if r != 0 {
                    reset = 1;
                }
                false
            } else if r == 0 {
                true
            } else {
                let cid = m[g::SI_CID][21] as u32;
                crate::edanim::ed_is_element_animated(m, o, ed, cid, r, 1, 0) as u16 == 0
            };
            if delete {
                let mut k = m.u16(model.wrapping_add(0x34)) as i32 - 1;
                while k > 0 {
                    o.call(m, &out::SI_MODEL_DELETE_I_DEFINITION, &[model, k as u16 as u32]);
                    k -= 1;
                }
            }
            if reset != 0 {
                o.call(m, &out::SI_MODEL_RESET_SHAPE, &[model]);
            }
        }
        i += 1;
    }
}

/// The float at +0x18 of `SI_glbAnim`: how close (in frames) a key must be to the current
/// frame to count as on it.
fn key_tolerance(m: &Mem) -> f32 {
    m.f32(g::SI_GLB_ANIM.0.wrapping_add(0x18))
}

/// `sprintf(dst, "%s%d", s, n)` (and the like): the text, the number in decimal.
fn put_text_number(m: &mut Mem, dst: u32, s: &[u8], n: i32) {
    let mut text = s.to_vec();
    text.extend(si_mem::clib::decimal(n));
    m.put_cstr(dst, &text);
}

/// [soft 0x08242d90 addShapeWeightFcurve]
///
/// Makes the weight curve of a new shape key `n + 1` (nothing for `n` at or beyond the
/// number of definitions): every existing weight curve gets a key of value 0 at the current
/// time (unless its first key lies more than the tolerance after it with value 0, or its
/// last more than the tolerance before it with value 0); a new curve is put in the list as
/// number n + 1; every weight curve's element is renamed after its curve's name and its
/// place in the list (`%s%d` in shape.c's `workStr`, a copy as the record's name at +0x14,
/// `SI_dbFormatFullName` at +0x24); the new curve gets a key of value 1 at the current
/// frame and keys of value 0 at the latest key time of the other curves lying more than the
/// tolerance before now and at the earliest lying more than it after now. Times are the
/// current frame (an integer, rounded toward zero in the register, as a float) divided by
/// the frame rate; the tolerance is `SI_glbAnim`'s float at +0x18 divided by it. The other
/// curves' word at +0x50 (the place of the key before now) is brought up to date.
pub fn add_shape_weight_fcurve(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<si_types::model::Model>, n: i32) {
    let nd = m[model].nb_defs as i32;
    if n >= nd {
        return;
    }
    check_shape_weight_fcurve(m, o, model, nd - 1);
    let index = m.local(4);
    let e = o.call(m, &out::SI_ED_GET_ELEM_FROM_SHAPE, &[model.addr(), index]);
    if e == 0 {
        m.release(4);
        return;
    }
    // the two times the original keeps in locals across the curves (and in the register
    // while it walks one curve's keys)
    let l20 = m.local(4);
    let l24 = m.local(4);
    let fc = F80::from(m[g::SI_FRAME_CURRENT]);
    let t = fc.round_int(if fc.lt(F80::ZERO) { Round::Up } else { Round::Down }).to_f32();
    let r = m[g::SI_FRAME_RATE];
    let time = (x(t) / r).to_f32();
    let eps = (x(key_tolerance(m)) / r).to_f32();
    let mut f = o.call(m, &out::SI_ED_FIND_FCV, &[e, 0, WEIGHT_CURVE]);
    while f != 0 {
        let keys = m.u32(f.wrapping_add(8));
        let first_zero = (x(m.f32(keys)) - time).gt(x(eps)) && x(m.f32(keys.wrapping_add(4))).eq(F80::ZERO);
        let skip = first_zero || {
            let last = keys.wrapping_add((m.i32(f.wrapping_add(FCURVE_0C)) as u32).wrapping_mul(0x24)).wrapping_sub(0x24);
            (x(time) - m.f32(last)).gt(x(eps)) && x(m.f32(last.wrapping_add(4))).eq(F80::ZERO)
        };
        if !skip {
            let w = m.u16(f.wrapping_add(FCURVE_1E)) as u32;
            let k = o.call(m, &out::SI_FKEY_CREATE, &[time.to_bits(), 0, 0, 1, w]);
            o.call(m, &out::SI_FCURVE_ADD_KEY, &[f, k]);
        }
        f = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[e, 0, WEIGHT_CURVE]);
    }
    let nf = o.call(m, &out::SI_FCURVE_ALLOCATE, &[NEW_WEIGHT_CURVE, model.addr()]);
    let cid = m[g::SI_CID][21] as u32;
    crate::edanim::ed_fcv_insert(m, o, nf, e, cid, m.u32(index), n + 1);
    let mut i: i32 = 0;
    let mut c = o.call(m, &out::SI_ED_FIND_FCV, &[e, 0, WEIGHT_CURVE]);
    while c != 0 {
        let (dict, cid0) = (g::cur_dict(m), m[g::SI_CID][0] as u32);
        let d = o.call(m, &out::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, cid0, c]);
        let (dict, cid0) = (g::cur_dict(m), m[g::SI_CID][0] as u32);
        let el = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[dict, cid0, d]);
        let name = o.call(m, &out::SI_FCV_GET_NAME, &[c]);
        let text = m.cstr(name);
        put_text_number(m, g::SHAPE_WORK_STR.0, &text, i);
        let rec = m.u32(el);
        let old = m.u32(rec.wrapping_add(0x14));
        if old != 0 {
            m.free(old);
            let rec = m.u32(el);
            m.set_u32(rec.wrapping_add(0x14), 0);
        }
        let p = m.strdup(g::SHAPE_WORK_STR.0);
        let rec = m.u32(el);
        m.set_u32(rec.wrapping_add(0x14), p);
        let full = o.call(m, &out::SI_DB_FORMAT_FULL_NAME, &[m.u32(el)]);
        let rec = m.u32(el);
        m.set_u32(rec.wrapping_add(0x24), full);
        i += 1;
        c = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[e, 0, WEIGHT_CURVE]);
    }
    let w = m.u16(nf.wrapping_add(FCURVE_1E)) as u32;
    let k = o.call(m, &out::SI_FKEY_FRAME_CREATE, &[m[g::SI_FRAME_CURRENT] as u32, 1f32.to_bits(), 0, 1, w]);
    o.call(m, &out::SI_FCURVE_ADD_KEY, &[nf, k]);
    let (mut before, mut after) = (false, false);
    let mut c = o.call(m, &out::SI_ED_FIND_FCV, &[e, 0, WEIGHT_CURVE]);
    while c != 0 {
        if c != nf {
            let keys = m.u32(c.wrapping_add(8));
            let at = keys.wrapping_add((m.i32(c.wrapping_add(0x50)) as u32).wrapping_mul(0x24));
            if (x(time) - m.f32(at)).le(x(eps)) {
                m.set_i32(c.wrapping_add(0x50), 0);
            }
            let mut j = m.i32(c.wrapping_add(0x50));
            if j < m.i32(c.wrapping_add(FCURVE_0C)) {
                let (mut va, mut vb) = (x(m.f32(l24)), x(m.f32(l20)));
                let mut off = (j as u32).wrapping_mul(0x24);
                loop {
                    let kt = x(m.f32(m.u32(c.wrapping_add(8)).wrapping_add(off)));
                    let mut go = true;
                    if (x(time) - kt).gt(x(eps)) {
                        if !before {
                            vb = kt;
                            before = true;
                        } else if kt.gt(vb) {
                            vb = kt;
                        }
                        m.set_i32(c.wrapping_add(0x50), j);
                    } else if (kt - time).gt(x(eps)) {
                        if !after {
                            va = kt;
                            after = true;
                        } else if kt.lt(va) {
                            va = kt;
                        }
                        go = false;
                    }
                    j += 1;
                    off = off.wrapping_add(0x24);
                    if j >= m.i32(c.wrapping_add(FCURVE_0C)) || !go {
                        break;
                    }
                }
                m.set_f32(l24, va.to_f32());
                m.set_f32(l20, vb.to_f32());
            }
        }
        c = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[e, 0, WEIGHT_CURVE]);
    }
    if before {
        let vb = m.f32(l20);
        if (x(time) - vb).gt(x(eps)) {
            let w = m.u16(nf.wrapping_add(FCURVE_1E)) as u32;
            let k = o.call(m, &out::SI_FKEY_CREATE, &[vb.to_bits(), 0, 0, 1, w]);
            o.call(m, &out::SI_FCURVE_ADD_KEY, &[nf, k]);
        }
    }
    if after {
        let va = m.f32(l24);
        if (x(va) - time).gt(x(eps)) {
            let w = m.u16(nf.wrapping_add(FCURVE_1E)) as u32;
            let k = o.call(m, &out::SI_FKEY_CREATE, &[va.to_bits(), 0, 0, 1, w]);
            o.call(m, &out::SI_FCURVE_ADD_KEY, &[nf, k]);
        }
    }
    m.release(12);
}

/// What `SI_shapeAddKey` copies a geometry with, by the model's kind, and the message it
/// shows when the number of points differs from the last definition's: the copy routine,
/// whether the counts agree, the message's address.
fn copy_of(m: &Mem, kind: i32, src: u32, last: u32) -> Option<(&'static out::Routine, bool, u32)> {
    let u16s = |a: u32, o: u32| m.u16(a.wrapping_add(o)) as u32;
    Some(match kind {
        4 => (&out::SI_MESH_COPY, m.u32(last.wrapping_add(0x14)) == m.u32(src.wrapping_add(0x14)), 0x08699800),
        1 => (&out::SI_PATCH_COPY, u16s(last, 0xc).wrapping_mul(u16s(last, 0xe)) == u16s(src, 0xc).wrapping_mul(u16s(src, 0xe)), 0x08699840),
        2 => (&out::SI_FACE_COPY, u16s(last, 0x14) == u16s(src, 0x14), 0x08699880),
        6 => (&out::SI_SPLINE_COPY, u16s(last, 0x10) == u16s(src, 0x10), 0x086998c0),
        9 => (&out::SI_NURBS_CRV_COPY, m.u32(last.wrapping_add(4)) == m.u32(src.wrapping_add(4)), 0x08699900),
        10 => (&out::SI_NURBS_SRF_COPY, m.u32(last.wrapping_add(0x48)) == m.u32(src.wrapping_add(0x48)), 0x08699940),
        _ => return None,
    })
}

/// [soft 0x08242088 SI_shapeAddKey]
///
/// The model's current geometry (`current`: the working definition at +0x48, else the
/// definition shown) added as a shape key at the current frame. The SHAPES element and the
/// shape curve (type 0xc9) are made when missing. With keys on the curve already: when one
/// lies on the current frame (within `SI_glbAnim`'s tolerance), the definition it names is
/// shown instead, or, with `ask`, the request dialog (`SI_dialogRequest3 (0x08141e84)`,
/// behind the trait) chooses between that (Replace), going on (Add) and nothing (Cancel).
/// Otherwise the new key's number is 0 before the first key, the last definition after the
/// last key, else one more than the curve's value at the frame, at most the last
/// definition; its weight curve is made (`addShapeWeightFcurve`), the geometry copied in
/// as definition number + 1 (refused with a message when its number of points differs from
/// the last definition's), the curve's values above the old one raised by 1 and a key set.
/// The first key keeps the first definition as the original and adds the copy as
/// definition 1 with a key of value 0.
pub fn shape_add_key(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<si_types::model::Model>, ask: i16, current: i16) {
    let sp = m.sp();
    let index = m.local(4);
    let buf = m.local(0x20);
    shape_add_key_in(m, o, model, ask, current, index, buf);
    m.set_sp(sp);
}

#[allow(clippy::too_many_arguments)]
fn shape_add_key_in(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<si_types::model::Model>, ask: i16, current: i16, index: u32, buf: u32) {
    let ma = model.addr();
    let mut e = o.call(m, &out::SI_ED_GET_ELEM_FROM_SHAPE, &[ma, index]);
    if e == 0 {
        o.call(m, &out::SI_ED_ADD_SHAPE, &[ma]);
        e = o.call(m, &out::SI_ED_GET_ELEM_FROM_SHAPE, &[ma, index]);
        if e == 0 {
            return;
        }
    }
    let mut f = o.call(m, &out::SI_ED_FIND_FCV, &[e, 0, SHAPE_CURVE]);
    if f == 0 {
        f = o.call(m, &out::SI_FCURVE_ALLOCATE, &[8, ma]);
        let cid = m[g::SI_CID][21] as u32;
        crate::edanim::ed_fcv_add(m, o, f, e, cid, m.u32(index));
    }
    let source = |m: &Mem| {
        if current != 0 {
            m.u32(ma.wrapping_add(0x48))
        } else {
            m.u32(m.u32(ma.wrapping_add(0x38)).wrapping_add((m.u16(ma.wrapping_add(0x36)) as u32) * 4))
        }
    };
    let key;
    if m.i32(f.wrapping_add(FCURVE_0C)) > 0 {
        let mut idx: i16 = -1;
        let mut i: i32 = 0;
        while i < m.i32(f.wrapping_add(FCURVE_0C)) {
            let at = m.u32(f.wrapping_add(8)).wrapping_add((i as u32).wrapping_mul(0x24));
            let fr = o.call_f(m, &out::SI_SEC_AS_FRAME, &[m.u32(at)]);
            if (F80::from(m[g::SI_FRAME_CURRENT]) - fr).abs().lt(x(key_tolerance(m))) {
                let at = m.u32(f.wrapping_add(8)).wrapping_add((i as u32).wrapping_mul(0x24));
                idx = (x(m.f32(at.wrapping_add(4))) + x(0.5f64)).to_i32(Round::Zero) as i16;
                let mut answer: u16 = 0;
                if ask != 0 {
                    put_text_number(m, buf, b"Replace shape #", idx as i32);
                    answer = o.call(m, &out::SI_DIALOG_REQUEST3, &[0x086997c0, buf, 0x0869978d, 0x08699785, 0x08699781, 0x0869977a]) as u16;
                }
                match answer {
                    0 => {
                        o.call(m, &out::SI_MODEL_SET_I_DEFINITION, &[ma, (idx as u16).wrapping_add(1) as u32]);
                        return;
                    }
                    2 => return,
                    _ => break,
                }
            }
            i += 1;
        }
        if idx < 0 {
            let fr = o.call(m, &out::SI_FKEY_FRAME, &[m.u32(f.wrapping_add(8))]) as i32;
            let fc = m[g::SI_FRAME_CURRENT];
            if fc >= fr {
                o.call_f(m, &out::SI_FCURVE_FRAME_VALUE, &[f, fc as u32]);
            }
        }
        let value: f32;
        let fr = o.call(m, &out::SI_FKEY_FRAME, &[m.u32(f.wrapping_add(8))]) as i32;
        if m[g::SI_FRAME_CURRENT] < fr {
            value = f32::from_bits(0xb8d1b717);
            idx = 0;
        } else {
            let last = m.u32(f.wrapping_add(8)).wrapping_add((m.i32(f.wrapping_add(FCURVE_0C)) as u32).wrapping_mul(0x24)).wrapping_sub(0x24);
            let fr = o.call(m, &out::SI_FKEY_FRAME, &[last]) as i32;
            let fc = m[g::SI_FRAME_CURRENT];
            if fc > fr {
                idx = m[model].nb_defs.wrapping_sub(1) as i16;
                value = F80::from(idx as i32).to_f32();
            } else {
                let v = o.call_f(m, &out::SI_FCURVE_FRAME_VALUE, &[f, fc as u32]);
                value = v.to_f32();
                idx = v.to_i32(Round::Zero).wrapping_add(1) as i16;
            }
        }
        if idx as i32 >= m[model].nb_defs as i32 {
            idx = m[model].nb_defs.wrapping_sub(1) as i16;
        }
        add_shape_weight_fcurve(m, o, model, idx as i32);
        let src = source(m);
        let kind = m.i32(ma.wrapping_add(0x30));
        let last = m.u32(m.u32(ma.wrapping_add(0x38)).wrapping_add((m[model].nb_defs as u32) * 4).wrapping_sub(4));
        match copy_of(m, kind, src, last) {
            Some((copy, same, message)) => {
                if !same {
                    o.call(m, &out::SI_STATUS_DISPLAY_MESSAGE, &[message, 2]);
                    return;
                }
                let c = o.call(m, copy, &[src]);
                let kind = m.u32(ma.wrapping_add(0x30));
                o.call(m, &out::SI_MODEL_ADD_I_DEFINITION, &[ma, (idx as u16).wrapping_add(1) as u32, kind, c]);
            }
            None => {
                o.call(m, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08699972, 3]);
            }
        }
        o.call(m, &out::SI_FCURVE_INC_VALUE_ABOVE, &[f, value.to_bits(), 1f32.to_bits()]);
        let w = m.u16(f.wrapping_add(FCURVE_1E)) as u32;
        let v = F80::from(idx as i32).to_f32().to_bits();
        key = o.call(m, &out::SI_FKEY_FRAME_CREATE, &[m[g::SI_FRAME_CURRENT] as u32, v, 0, 0, w]);
    } else {
        let d0 = m.u32(m.u32(ma.wrapping_add(0x38)));
        o.call(m, &out::SI_MODEL_COPY_ORG_DEF, &[ma, d0]);
        add_shape_weight_fcurve(m, o, model, 0);
        let src = source(m);
        let kind = m.i32(ma.wrapping_add(0x30));
        match copy_of(m, kind, src, src) {
            Some((copy, _, _)) => {
                let c = o.call(m, copy, &[src]);
                let kind = m.u32(ma.wrapping_add(0x30));
                o.call(m, &out::SI_MODEL_ADD_I_DEFINITION, &[ma, 1, kind, c]);
            }
            None => {
                o.call(m, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08699983, 3]);
            }
        }
        let w = m.u16(f.wrapping_add(FCURVE_1E)) as u32;
        key = o.call(m, &out::SI_FKEY_FRAME_CREATE, &[m[g::SI_FRAME_CURRENT] as u32, 0, 0, 0, w]);
    }
    o.call(m, &out::SI_FCURVE_ADD_KEY, &[f, key]);
}
