// SPDX-License-Identifier: MIT
//! A definition bent over a patch or a NURBS surface (`SI_patchFit`, `model.c`; finding 0015,
//! "`SI_patchFit` on the image") on the image, with the routines of `model.c` it calls that
//! no other module has: `SI_surfaceValueAndTangent` and `SI_surfaceValueAndNormal`
//! (the point and the tangents or the normal of the followed surface, patch or NURBS) and
//! the optimisation of `nurbsCrv` (`SI_svtAllocOptimBegin`, `SI_svtAllocOptimEnd`).
//!
//! Every point goes the same way: through the fitting's matrix (`SI_PtchFitting` +0x2c) as a
//! row vector, column 0 being the surface parameter `u`, column 2 the parameter `v` and
//! column 1 the height `h`; the surface answers the point `P` and the normal `N` at `(u, v)`,
//! and the point becomes `P + h·N`. `u` and `v` reach the evaluator as doubles of the
//! unrounded 80-bit sums; `h` is rounded to a float (the compiler stored it and read it back).
//!
//! All six kinds are written, and the followed NURBS surface (the NURBS branch of
//! `SI_surfaceValueAndTangent`, `nurbs_value_and_tangent`); all are checked against the original on
//! sample scenes' images (finding 0030, sections 16 to 18, the NURBS surface in section 18).

use si_mem::{Mem, Ptr32};
use si_spline::patch_eval_image::patch_value_and_tangent;
use si_x87::{x, F80};

use super::fit::callees as fc;
use super::readjust::multiply_def;
use super::{g, mx, Outside};

/// The global `nurbsCrv` (0x08688c60): the curve `SI_svtAllocOptimBegin` makes for the
/// evaluator of a followed NURBS surface to reuse.
pub const NURBS_CRV: u32 = 0x0868_8c60;

/// 0.0001 (`.rodata` 0x0864a510, a double): a normal no longer than this keeps its length.
const SHORT: f64 = 0.0001;

/// `SI_svtAllocOptimBegin(model)` [soft 0x08156310 SI_svtAllocOptimBegin]: for a model with a
/// stored definition (+0x48) of kind 10 only, `nurbsCrv` becomes a zeroed 100-byte record
/// whose point array (+0x2c) holds `n` entries of 0x24 bytes and whose knot array (+0xc) holds
/// `2n − 2` doubles, `n` being the surface's word at +0x24; its counts at +0, +4 and +8 are
/// `n`, `n` and `2n − 2`.
pub fn svt_alloc_optim_begin(m: &mut Mem, model: u32) {
    if model == 0 || m.u32(model + 0x48) == 0 || m.i32(model + 0x30) != 10 {
        return;
    }
    let n = m.u32(m.u32(model + 0x48) + 0x24);
    let crv = m.si_calloc(1, 100);
    m.set_u32(NURBS_CRV, crv);
    m.set_u16(crv + 0x20, 0);
    let points = m.si_calloc(n, 0x24);
    let crv = m.u32(NURBS_CRV);
    m.set_u32(crv + 0x2c, points);
    m.set_u32(crv, n);
    m.set_u32(crv + 4, n);
    let knots_n = n.wrapping_mul(2).wrapping_sub(2);
    m.set_u32(crv + 8, knots_n);
    let knots = m.si_calloc(m.u32(crv + 8), 8);
    let crv = m.u32(NURBS_CRV);
    m.set_u32(crv + 0xc, knots);
}

/// `SI_svtAllocOptimEnd()` [soft 0x08156398 SI_svtAllocOptimEnd]: `SI_nurbsCrvDispose
/// (&nurbsCrv)`, then the global is 0.
pub fn svt_alloc_optim_end(m: &mut Mem) {
    si_hrc::image::nurbs::nurbs_crv_dispose(m, NURBS_CRV);
    m.set_u32(NURBS_CRV, 0);
}

/// The statics of `SI_nurbsSrfExtractCurveSeg`: the scratch arrays it grows and never frees
/// (`aj`, `dl`, `dr` and their sizes).
const SEG_AJ: u32 = 0x086d_9c0c;
const SEG_DL: u32 = 0x086d_9c10;
const SEG_DR: u32 = 0x086d_9c14;
const SEG_NB_DLR: u32 = 0x086d_9c18;
const SEG_NB_AJ: u32 = 0x086d_9c1c;

/// `SI_nurbsSrfExtractCurveSeg(surface, u, v, 0, nd, out)` [soft 0x084d603c
/// SI_nurbsSrfExtractCurveSeg], the form without the exchange of directions, on the image:
/// the one-span curve in V at `u` (or of the `nd`-th derivative in U), written into `out` or,
/// for a null `out`, into a curve made with `SI_calloc` (record, `order_v` points, `2 order_v -
/// 2` knots). The scratch arrays grow as the original's do (`SI_malloc` the first time,
/// `SI_realloc` after, never freed); their contents are not kept (every element is written
/// before it is read). `None` when the evaluator refuses.
fn extract_seg(m: &mut Mem, srf: u32, u: f64, v: f64, nd: i32, out: u32) -> Option<u32> {
    use si_nurbs::image::{CrvHeld, SrfHeld, LEFT};
    use si_nurbs::own::Curve;
    use si_nurbs::{eval, Statics};
    let held = SrfHeld::at(m, srf);
    let (ou, ov) = (held.rec.u.order, held.rec.v.order);
    let mut out = out;
    if out == 0 {
        out = m.si_calloc(1, 0x64);
        m.set_u16(out + 0x20, 0);
        let pts = m.si_calloc(ov as u32, 0x24);
        m.set_u32(out + 0x2c, pts);
        m.set_i32(out, ov);
        m.set_i32(out + 4, ov);
        m.set_i32(out + 8, ov.wrapping_add(ov).wrapping_sub(2));
        let knots = m.si_calloc(m.u32(out + 8), 8);
        m.set_u32(out + 0xc, knots);
    }
    let need = ou.wrapping_sub(1);
    if need > m.i32(SEG_NB_DLR) {
        let dl = m.u32(SEG_DL);
        m.set_i32(SEG_NB_DLR, need);
        let bytes = (need as u32).wrapping_shl(3);
        if dl == 0 {
            let a = m.si_malloc(bytes);
            m.set_u32(SEG_DL, a);
            let b = m.si_malloc((m.i32(SEG_NB_DLR) as u32).wrapping_shl(3));
            m.set_u32(SEG_DR, b);
        } else {
            let a = m.si_realloc(dl, bytes);
            m.set_u32(SEG_DL, a);
            let dr = m.u32(SEG_DR);
            let b = m.si_realloc(dr, (m.i32(SEG_NB_DLR) as u32).wrapping_shl(3));
            m.set_u32(SEG_DR, b);
        }
    }
    let nb = ou.wrapping_mul(ov);
    if nb > m.i32(SEG_NB_AJ) {
        let aj = m.u32(SEG_AJ);
        m.set_i32(SEG_NB_AJ, nb);
        let bytes = (nb as u32).wrapping_mul(36);
        let a = if aj == 0 { m.si_malloc(bytes) } else { m.si_realloc(aj, bytes) };
        m.set_u32(SEG_AJ, a);
    }
    let h = CrvHeld::at(m, out);
    let mut c = Curve { rec: h.rec, knots: Some(h.knots), points: Some(h.points), length_param: None, length_dist: None };
    let mut st = Statics { left: m.i32(LEFT) };
    let r = eval::seg_of_view(&mut st, held.view(), u, v, nd, Some(&mut c));
    m.set_i32(LEFT, st.left);
    r?;
    let pts = m.u32(out + 0x2c);
    for (j, p) in c.points.iter().flatten().enumerate() {
        m.write(Ptr32::new(pts + 0x24 * j as u32), *p);
    }
    let kn = m.u32(out + 0xc);
    for (i, k) in c.knots.iter().flatten().enumerate() {
        m.set_f64(kn + 8 * i as u32, *k);
    }
    m.set_f64(out + 0x10, c.rec.dir.range_start.0);
    m.set_f64(out + 0x18, c.rec.dir.range_end.0);
    Some(out)
}

/// One surface parameter brought into the range `[lo, hi]` of its direction: past either
/// end a closed direction wraps (`hi` and `lo` are subtracted or added until it is inside; no
/// wrapping when `hi` is not above 1e-6, the parameter is just held at `hi`), an open one holds
/// it at the end and gives back how far it was out (as a double). All in extended precision.
fn clamp_param(p: f64, lo: f64, hi: f64, closed: bool) -> (F80, Option<f64>) {
    let (lo_x, hi_x) = (x(lo), x(hi));
    let mut v = x(p);
    let mut excess = None;
    if v.gt(hi_x) {
        let mut w = v;
        v = hi_x;
        if !closed {
            excess = Some((w - hi_x).to_f64());
        } else if !(hi_x.lt(x(1e-6)) || hi_x.eq(x(1e-6))) {
            loop {
                w = w - hi_x;
                v = w;
                if !hi_x.lt(w) {
                    break;
                }
            }
        }
    }
    if v.lt(lo_x) {
        if !closed {
            excess = Some((v - lo_x).to_f64());
            v = lo_x;
        } else {
            loop {
                v = (v - lo_x) + hi_x;
                if !v.lt(lo_x) {
                    break;
                }
            }
            if hi_x.lt(v) {
                v = hi_x;
            }
        }
    }
    (v, excess)
}

/// `SI_surfaceValueAndTangent(kind, surface, u, v, p, tu, tv)` SI_surfaceValueAndTangent (0x081563bc;
/// the NURBS branch is written but no check reaches it: the scene vectors have no kind-10 call): three points of doubles (x, y, z at +0, +8, +0x10) written at
/// `p`, `tu` and `tv`. For a patch (kind 1) the parameters are narrowed to float and
/// `SI_patchValueAndTangent` answers floats, which are widened. (Where the evaluator refuses
/// and writes nothing, the original widens locals it never set; here they hold the fill.)
///
/// A NURBS surface (kind 10): `u` and `v` are brought into the surface's ranges (+0x10/+0x18
/// and +0x34/+0x3c, the closed words at +0x20 and +0x44; see `clamp_param`). The one-span curve
/// in V at `u` is extracted (into the curve `SI_svtAllocOptimBegin` made, when there is
/// one); its homogeneous point at `v` divided by its weight is `p`, its first derivative is
/// `tv`. The curve of the derivative in U gives `tu = (D - Dw·p) / w`. A parameter that was
/// outside an open direction adds `excess × tu` (resp. `× tv`) to `p`. Any other kind
/// writes nothing.
pub fn surface_value_and_tangent(m: &mut Mem, kind: i32, surface: u32, u: f64, v: f64, p: u32, tu: u32, tv: u32) {
    if kind == 10 {
        nurbs_value_and_tangent(m, surface, u, v, p, tu, tv);
        return;
    }
    if kind != 1 {
        return;
    }
    let (uf, vf) = (u as f32, v as f32);
    let local = m.local(36);
    patch_value_and_tangent(m, Ptr32::new(surface), uf, vf, local, local + 12, local + 24);
    for (k, dst) in [p, tu, tv].into_iter().enumerate() {
        for c in 0..3u32 {
            let f = m.f32(local + 12 * k as u32 + 4 * c);
            m.set_f64(dst + 8 * c, f as f64);
        }
    }
    m.release(36);
}

fn nurbs_value_and_tangent(m: &mut Mem, surface: u32, u: f64, v: f64, p: u32, tu: u32, tv: u32) {
    use si_nurbs::image as ni;
    let (hu, lu, hv, lv) = (m.f64(surface + 0x18), m.f64(surface + 0x10), m.f64(surface + 0x3c), m.f64(surface + 0x34));
    let (cu, cv) = (m.u16(surface + 0x20) != 0, m.u16(surface + 0x44) != 0);
    let (u1, eu) = clamp_param(u, lu, hu, cu);
    let (v1, ev) = clamp_param(v, lv, hv, cv);
    let (ud, vd) = (u1.to_f64(), v1.to_f64());
    let optim = m.u32(NURBS_CRV);
    let Some(seg1) = extract_seg(m, surface, ud, vd, 0, optim) else { return };
    let Ok(p4) = ni::crv_value4d(m, seg1, vd) else { return };
    let Ok(d1) = ni::crv_value(m, seg1, vd, 1) else { return };
    let inv = x(1.0) / x(p4.w.0);
    m.set_f64(p, (x(p4.x.0) * inv).to_f64());
    m.set_f64(p + 8, (x(p4.y.0) * inv).to_f64());
    m.set_f64(p + 0x10, (x(p4.z.0) * inv).to_f64());
    for (k, c) in d1.iter().enumerate() {
        m.set_f64(tv + 8 * k as u32, *c);
    }
    let Some(seg2) = extract_seg(m, surface, ud, vd, 1, optim) else { return };
    let Ok(d4) = ni::crv_value4d(m, seg2, vd) else { return };
    let dw = x(d4.w.0);
    let t1 = inv * (x(d4.x.0) - dw * x(m.f64(p)));
    m.set_f64(tu, t1.to_f64());
    let t2 = inv * (x(d4.y.0) - dw * x(m.f64(p + 8)));
    m.set_f64(tu + 8, t2.to_f64());
    let t3 = inv * (x(d4.z.0) - dw * x(m.f64(p + 0x10)));
    m.set_f64(tu + 0x10, t3.to_f64());
    if let Some(e) = eu {
        let e = x(e);
        m.set_f64(p, (t1 * e + x(m.f64(p))).to_f64());
        m.set_f64(p + 8, (e * x(m.f64(tu + 8)) + x(m.f64(p + 8))).to_f64());
        m.set_f64(p + 0x10, (e * x(m.f64(tu + 0x10)) + x(m.f64(p + 0x10))).to_f64());
    }
    if let Some(e) = ev {
        let e = x(e);
        for k in 0..3u32 {
            m.set_f64(p + 8 * k, (e * x(m.f64(tv + 8 * k)) + x(m.f64(p + 8 * k))).to_f64());
        }
    }
    if optim == 0 {
        for seg in [seg1, seg2] {
            let pp = m.local(4);
            m.set_u32(pp, seg);
            si_hrc::image::nurbs::nurbs_crv_dispose(m, pp);
            m.release(4);
        }
    }
}

/// `SI_surfaceValueAndNormal(kind, surface, u, v, p, n)` [soft 0x08156240
/// SI_surfaceValueAndNormal]: the point at `p` and the unit normal at `n` (doubles). The
/// normal is `Tv × Tu` formed in extended precision (x = Tv.y·Tu.z − Tv.z·Tu.y, y = Tv.z·Tu.x −
/// Tu.z·Tv.x... each product unrounded, see the code), stored as doubles; its length, the
/// root of `(Ny² + Nx²) + Nz²` rounded to a float, divides each unrounded component when its
/// absolute value is above 0.0001, and the quotients are stored over them.
pub fn surface_value_and_normal(m: &mut Mem, kind: i32, surface: u32, u: f64, v: f64, p: u32, n: u32) {
    let local = m.local(48);
    let (tu, tv) = (local, local + 24);
    surface_value_and_tangent(m, kind, surface, u, v, p, tu, tv);
    let t = |at: u32, c: u32| m.f64(at + 8 * c);
    let (uu, vv) = ([t(tu, 0), t(tu, 1), t(tu, 2)], [t(tv, 0), t(tv, 1), t(tv, 2)]);
    let nx = x(vv[1]) * uu[2] - x(vv[2]) * uu[1];
    let ny = x(vv[2]) * uu[0] - x(uu[2]) * vv[0];
    let nz = x(uu[1]) * vv[0] - x(uu[0]) * vv[1];
    m.set_f64(n, nx.to_f64());
    m.set_f64(n + 0x10, nz.to_f64());
    m.set_f64(n + 8, ny.to_f64());
    let len = (((ny * ny) + nx * nx) + nz * nz).sqrt().to_f32();
    if x(len).abs().gt(x(SHORT)) {
        m.set_f64(n, (nx / len).to_f64());
        m.set_f64(n + 8, (ny / len).to_f64());
        m.set_f64(n + 0x10, (nz / len).to_f64());
    }
    m.release(48);
}

/// A point taken through the fitting's matrix (`fit + 0x2c`, rows of 16 bytes) as a row
/// vector: `u` and `v` as the unrounded sums, `h` rounded to float.
fn through(m: &Mem, fit: u32, s: u32) -> (F80, f32, F80) {
    through_xyz(m, fit, m.f32(s), m.f32(s + 4), m.f32(s + 8))
}

/// [`through`] for a point held as three floats (the NURBS cases form theirs from the
/// homogeneous coordinates).
fn through_xyz(m: &Mem, fit: u32, px: f32, py: f32, pz: f32) -> (F80, f32, F80) {
    let e = |r: u32, c: u32| m.f32(fit + 0x2c + 16 * r + 4 * c);
    // column 0 starts with y, 1 with x, 2 with z·m22 + (y·m12 + x·m02): the same values
    // whatever the order; only which NaN survives depends on it
    let u = ((x(py) * e(1, 0) + x(px) * e(0, 0)) + x(pz) * e(2, 0)) + e(3, 0);
    let h = (((x(px) * e(0, 1) + x(py) * e(1, 1)) + x(pz) * e(2, 1)) + e(3, 1)).to_f32();
    let v = (x(pz) * e(2, 2) + (x(py) * e(1, 2) + x(px) * e(0, 2))) + e(3, 2);
    (u, h, v)
}

/// The point and the normal of the followed surface at the parameters of the point at `s`,
/// as floats (`SI_surfaceValueAndNormal` through the fitting's followed node): `P`, `N` and
/// the height `h`.
fn point_and_normal(m: &mut Mem, fit: u32, s: u32) -> ([f32; 3], [f32; 3], f32) {
    let (u, h, v) = through(m, fit, s);
    let followed = m.u32(fit + 4);
    let (kind, surf) = (m.i32(followed + 0x30), m.u32(followed + 0x48));
    let local = m.local(48);
    surface_value_and_normal(m, kind, surf, u.to_f64(), v.to_f64(), local, local + 24);
    let g = |m: &Mem, at: u32| [m.f64(at) as f32, m.f64(at + 8) as f32, m.f64(at + 16) as f32];
    let (p, n) = (g(m, local), g(m, local + 24));
    m.release(48);
    (p, n, h)
}

/// The patch case (0x0846fcf0–0x0846fe9c): the points of `src` (+0x1a4, 16 bytes each, the
/// fourth float left alone) to those of `dst`, `i16(+0xc) × i16(+0xe)` of them (a 16-bit
/// product, none when it is not positive), each x = f32(Pxf + Nxf·h) with the product
/// unrounded, y = f32(f32(Nyf·h) + Pyf), z likewise; then `SI_patchComputeSurface(dst, 0)`.
fn fit_patch(m: &mut Mem, o: &mut dyn Outside, fit: u32, src: u32, dst: u32) {
    let mut n = m.i16(src + 0xc).wrapping_mul(m.i16(src + 0xe));
    let mut s = m.u32(src + 0x1a4);
    let mut d = m.u32(dst + 0x1a4);
    while n > 0 {
        let (p, nn, h) = point_and_normal(m, fit, s);
        m.set_f32(d, (x(p[0]) + x(nn[0]) * h).to_f32());
        m.set_f32(d + 4, (x((x(nn[1]) * h).to_f32()) + p[1]).to_f32());
        m.set_f32(d + 8, (x((x(nn[2]) * h).to_f32()) + p[2]).to_f32());
        s = s.wrapping_add(16);
        d = d.wrapping_add(16);
        n = n.wrapping_sub(1);
    }
    o.call(m, &fc::SI_PATCH_COMPUTE_SURFACE, &[dst, 0]);
}


/// One vertex's `P + h·N` stored at `d` with the unit normal `n` (floats): x = f32(Pxf + h·Nx)
/// with the product unrounded, y = f32(f32(h·Ny) + Pyf), z likewise.
fn put_displaced(m: &mut Mem, d: u32, p: [f32; 3], n: [f32; 3], h: f32) {
    m.set_f32(d, (x(p[0]) + x(n[0]) * h).to_f32());
    m.set_f32(d + 4, (x((x(n[1]) * h).to_f32()) + p[1]).to_f32());
    m.set_f32(d + 8, (x((x(n[2]) * h).to_f32()) + p[2]).to_f32());
}

/// The three doubles at `a` as floats.
fn floats(m: &Mem, a: u32) -> [f32; 3] {
    [m.f64(a) as f32, m.f64(a + 8) as f32, m.f64(a + 16) as f32]
}

/// A float vector divided by its length (a float: f32(√((a² + b²) + c²)) in the order `order`
/// names) when the length is above 0.0001 in absolute value; each component f32(c / len).
fn normalise(v: [f32; 3], sum: impl Fn([F80; 3]) -> F80) -> [f32; 3] {
    let len = sum([x(v[0]) * v[0], x(v[1]) * v[1], x(v[2]) * v[2]]).sqrt().to_f32();
    if !x(len).abs().gt(x(SHORT)) {
        return v;
    }
    [(x(v[0]) / len).to_f32(), (x(v[1]) / len).to_f32(), (x(v[2]) / len).to_f32()]
}

/// The mesh case (0x0846f487–0x0846fcef): each vertex of `src` (`+0`, 0x14 bytes, the count
/// `i32 +0x14`, a 16-bit index) to the same vertex of `dst`.
///
/// Without bit 4 of the byte at `src + 0x20` the point and the normal come from
/// `SI_surfaceValueAndNormal` as in the patch case, then `SI_meshComputesPolygonNormal(dst,
/// −1)` and `SI_meshComputesVertexNormal(dst, −1)`.
///
/// With it, the point and the two tangents Tu, Tv (floats) form the frame: N = Tv × Tu as
/// floats (x = Tv.y·Tu.z − Tv.z·Tu.y, y = Tv.z·Tu.x − Tv.x·Tu.z, z = Tv.x·Tu.y − Tv.y·Tu.x, each
/// product unrounded), and Tu, Tv and N are each made unit length ([`normalise`]; the sums are
/// z² + (y² + x²) for Tu, (y² + x²) + z² for Tv, (y² + x²) + z² for N with N's own order of
/// components). The rows Tu, N, Tv of a local identity matrix go through
/// `SI_matrixTransposeInverse`; the vertex is P + h·N (unit N); and the normals of the
/// polygons around the vertex are bent by that matrix like `SI_splineFit`'s
/// ([`super::fit::fit_normal`], the fitting's normal matrix at +0x6c here). Last
/// `SI_meshComputesPolygonNormal(dst, −1)` only.
fn fit_mesh(m: &mut Mem, o: &mut dyn Outside, fit: u32, src: u32, dst: u32) {
    let followed = m.u32(fit + 4);
    let (kind, surf) = (m.i32(followed + 0x30), m.u32(followed + 0x48));
    if m.u8(src + 0x20) & 4 == 0 {
        let (mut s, mut d) = (m.u32(src), m.u32(dst));
        let mut i = 0i16;
        while (i as i32) < m.i32(src + 0x14) {
            let (p, n, h) = point_and_normal(m, fit, s);
            put_displaced(m, d, p, n, h);
            s = s.wrapping_add(0x14);
            d = d.wrapping_add(0x14);
            i = i.wrapping_add(1);
        }
        o.call(m, &fc::SI_MESH_COMPUTES_POLYGON_NORMAL, &[dst, 0xffff_ffff]);
        o.call(m, &fc::SI_MESH_COMPUTES_VERTEX_NORMAL, &[dst, 0xffff_ffff]);
        return;
    }
    let local = m.local(64);
    mx::copy(m, g::SI_IDENTITE.addr(), local);
    let normals = m.local(64);
    let (mut s, mut d) = (m.u32(src), m.u32(dst));
    let mut i = 0i16;
    while (i as i32) < m.i32(src + 0x14) {
        let (u, h, v) = through(m, fit, s);
        let t = m.local(72);
        surface_value_and_tangent(m, kind, surf, u.to_f64(), v.to_f64(), t, t + 24, t + 48);
        let (p, tu, tv) = (floats(m, t), floats(m, t + 24), floats(m, t + 48));
        m.release(72);
        let nx = (x(tv[1]) * tu[2] - x(tv[2]) * tu[1]).to_f32();
        let ny = (x(tv[2]) * tu[0] - x(tv[0]) * tu[2]).to_f32();
        let nz = (x(tv[0]) * tu[1] - x(tv[1]) * tu[0]).to_f32();
        let tu = normalise(tu, |q| q[2] + (q[1] + q[0]));
        let tv = normalise([tv[0], tv[1], tv[2]], |q| (q[1] + q[0]) + q[2]);
        let nn = normalise([nx, ny, nz], |q| (q[1] + q[0]) + q[2]);
        for c in 0..3u32 {
            m.set_f32(local + 4 * c, tu[c as usize]);
            m.set_f32(local + 16 + 4 * c, nn[c as usize]);
            m.set_f32(local + 32 + 4 * c, tv[c as usize]);
        }
        let (inv, _) = si_math::algebra::matrix_transpose_inverse(&super::mat(m, local));
        super::put_mat(m, normals, &inv);
        put_displaced(m, d, p, nn, h);
        let mut j = 0i16;
        while (j as i32) < m.u16(d + 0x10) as i32 {
            let poly = m.u16(m.u32(d + 0xc).wrapping_add((j as i32 * 12) as u32) + 8);
            j = j.wrapping_add(1);
            if poly == 0xffff {
                continue;
            }
            let off = (poly as i16 as i32 * 0x18) as u32;
            let sp = m.u32(src + 4).wrapping_add(off);
            let dp = m.u32(dst + 4).wrapping_add(off);
            let mut k = 0i16;
            while (k as i32) < m.u16(sp + 0x10) as i32 {
                let at = (k as i32 * 0x24) as u32;
                if m.i32(m.u32(sp + 0xc).wrapping_add(at)) == i as i32 {
                    super::fit::fit_normal(m, fit, 0x6c, sp, dp, at, normals);
                }
                k = k.wrapping_add(1);
            }
        }
        s = s.wrapping_add(0x14);
        d = d.wrapping_add(0x14);
        i = i.wrapping_add(1);
    }
    m.release(128);
    o.call(m, &fc::SI_MESH_COMPUTES_POLYGON_NORMAL, &[dst, 0xffff_ffff]);
}

/// One record of float points, as the spline case (kind 6) and each part of a face (kind 2)
/// have it (0x08470820, 0x0846ff12): `src` and `dst` hold a mode (`i32` +0xc), a count (`u16`
/// +0x10) and a pointer to the points (+0x14, 0x30 bytes per element). In mode 1 an element has
/// three points of three floats (+0, +0xc, +0x18), otherwise only the one at +0xc counts; each
/// goes the way a vertex of the mesh case does (no bit 4): `P + h·N` ([`put_displaced`]).
fn fit_record(m: &mut Mem, fit: u32, src: u32, dst: u32) {
    let mode_one = m.i32(src + 0xc) == 1;
    let (mut s, mut d) = (m.u32(src + 0x14), m.u32(dst + 0x14));
    if m.u16(src + 0x10) == 0 {
        return;
    }
    let mut i = 0i16;
    loop {
        let offsets: &[u32] = if mode_one { &[0, 0xc, 0x18] } else { &[0xc] };
        for &off in offsets {
            let (p, n, h) = point_and_normal(m, fit, s.wrapping_add(off));
            put_displaced(m, d.wrapping_add(off), p, n, h);
        }
        s = s.wrapping_add(0x30);
        d = d.wrapping_add(0x30);
        i = i.wrapping_add(1);
        if !((i as i32) < m.u16(src + 0x10) as i32) {
            break;
        }
    }
}

/// The face case (0x0846fea0): with a non-zero `u16` at +0x84 the face is the record at +4
/// followed by `i16(+0x78)` records of 0x74 bytes at the pointer +0x7c, each [`fit_record`];
/// without, it is `i16(+0x78)` NURBS curves of 0x64 bytes at the pointer +0x80, each with `i32`
/// +4 control points of 0x24 bytes at the pointer +0x2c ([`fit_face_point`]).
fn fit_face(m: &mut Mem, fit: u32, src: u32, dst: u32) {
    if m.u16(src + 0x84) != 0 {
        let mut i: i16 = -1;
        if !(i < m.i16(src + 0x78)) {
            return;
        }
        loop {
            let (a, b) = if i == -1 {
                (src.wrapping_add(4), dst.wrapping_add(4))
            } else {
                let off = (i as i32).wrapping_mul(0x74) as u32;
                (off.wrapping_add(m.u32(src + 0x7c)), off.wrapping_add(m.u32(dst + 0x7c)))
            };
            fit_record(m, fit, a, b);
            i = i.wrapping_add(1);
            if !(i < m.i16(src + 0x78)) {
                break;
            }
        }
        return;
    }
    let mut j: i16 = 0;
    if !(j < m.i16(src + 0x78)) {
        return;
    }
    loop {
        let off = (j as i32).wrapping_mul(0x64) as u32;
        let (a, b) = (off.wrapping_add(m.u32(src + 0x80)), off.wrapping_add(m.u32(dst + 0x80)));
        if m.i32(a + 4) > 0 {
            let mut k = 0i16;
            loop {
                let at = (k as i32).wrapping_mul(0x24) as u32;
                fit_face_point(m, fit, m.u32(a + 0x2c).wrapping_add(at), m.u32(b + 0x2c).wrapping_add(at));
                k = k.wrapping_add(1);
                if !((k as i32) < m.i32(a + 4)) {
                    break;
                }
            }
        }
        j = j.wrapping_add(1);
        if !(j < m.i16(src + 0x78)) {
            break;
        }
    }
}

/// One control point of a face's curve (0x084705a4): x, y, z times `1/w` (the reciprocal
/// unrounded) rounded to floats, taken through the fitting; the new point is
/// `P + (N·h)` with the product unrounded: x stored as a double of the unrounded sum, y and z
/// as doubles of floats of theirs; w = 1.
fn fit_face_point(m: &mut Mem, fit: u32, s: u32, d: u32) {
    let inv = x(1.0f64) / m.f64(s + 0x18);
    let f = |m: &Mem, c: u32| (x(m.f64(s + 8 * c)) * inv).to_f32();
    let (px, py, pz) = (f(m, 0), f(m, 1), f(m, 2));
    let (u, h, v) = through_xyz(m, fit, px, py, pz);
    let followed = m.u32(fit + 4);
    let (kind, surf) = (m.i32(followed + 0x30), m.u32(followed + 0x48));
    let local = m.local(48);
    surface_value_and_normal(m, kind, surf, u.to_f64(), v.to_f64(), local, local + 24);
    let (p, n) = (floats(m, local), floats(m, local + 24));
    m.release(48);
    let sum = |c: usize| x(p[c]) + x(n[c]) * h;
    m.set_f64(d, sum(0).to_f64());
    m.set_f64(d + 8, sum(1).to_f32() as f64);
    m.set_f64(d + 0x10, sum(2).to_f32() as f64);
    m.set_u32(d + 0x18, 0);
    m.set_u32(d + 0x1c, 0x3ff0_0000);
}

/// The cases of a NURBS curve (kind 9, 0x08470e05: `i32` count +4, points at the pointer
/// +0x2c, flag byte +0x3b) and a NURBS surface (kind 10, 0x08471016: count +0x48, points at
/// +0x54, flag byte +0x5b): each control point of 0x24 bytes (doubles x, y, z, w) becomes the
/// point `(x/w, y/w, z/w)` rounded to floats, taken through the fitting; the new one is
/// `P + h·N` with the sum of x unrounded and of y and z with `f32(N·h)` (the product rounded),
/// stored as doubles, w = 1. Last the flag byte of both definitions gets 0x60.
fn fit_control_points(m: &mut Mem, fit: u32, src: u32, dst: u32, count: u32, points: u32, flag: u32) {
    let mut i = 0i16;
    if m.i32(src + count) > 0 {
        loop {
            let at = (i as i32).wrapping_mul(0x24) as u32;
            let s = m.u32(src + points).wrapping_add(at);
            let f = |m: &Mem, c: u32| (x(m.f64(s + 8 * c)) / m.f64(s + 0x18)).to_f32();
            let (px, py, pz) = (f(m, 0), f(m, 1), f(m, 2));
            let (u, h, v) = through_xyz(m, fit, px, py, pz);
            let followed = m.u32(fit + 4);
            let (kind, surf) = (m.i32(followed + 0x30), m.u32(followed + 0x48));
            let local = m.local(48);
            surface_value_and_normal(m, kind, surf, u.to_f64(), v.to_f64(), local, local + 24);
            let (p, n) = (floats(m, local), floats(m, local + 24));
            m.release(48);
            let d = m.u32(dst + points).wrapping_add(at);
            m.set_f64(d, (x(p[0]) + x(n[0]) * h).to_f64());
            m.set_f64(d + 8, (x((x(n[1]) * h).to_f32()) + p[1]).to_f64());
            m.set_f64(d + 0x10, (x((x(n[2]) * h).to_f32()) + p[2]).to_f64());
            m.set_u32(d + 0x18, 0);
            m.set_u32(d + 0x1c, 0x3ff0_0000);
            i = i.wrapping_add(1);
            if !((i as i32) < m.i32(src + count)) {
                break;
            }
        }
    }
    let b = m.u8(src + flag);
    m.set_u8(src + flag, b | 0x60);
    let b = m.u8(dst + flag);
    m.set_u8(dst + flag, b | 0x60);
}

/// `SI_patchFit(fit, kind, src, dst)` [soft 0x0846f444 SI_patchFit] (all six kinds written and
/// checked on the image, the followed NURBS surface with `SI_svtAllocOptimBegin`'s curve both
/// made and not: finding 0030, sections 16 to 18). `fit` is the
/// patch deformation (`SI_PtchFitting`, the followed node at +4), `kind` the kind of both
/// definitions, `src` the one read, `dst` the one written (the working definition; they may
/// be the same).
///
/// Nothing at all unless the followed node is a patch (kind 1) or a NURBS surface (10). Then
/// `SI_svtAllocOptimBegin(followed)`, the case by `kind` (1 patch, 2 face, 4 mesh, 6 spline,
/// 9 NURBS curve, 10 NURBS surface; 3, 5, 7, 8 and every other value none), then, whatever
/// the kind, `SI_svtAllocOptimEnd()` and `SI_modelMultiplyDef(followed→+0x44→+0x1c4, kind,
/// dst)` ([`multiply_def`]).
pub fn patch_fit(m: &mut Mem, o: &mut dyn Outside, fit: u32, kind: i32, src: u32, dst: u32) {
    let followed = m.u32(fit + 4);
    let target = m.i32(followed + 0x30);
    if target != 1 && target != 10 {
        return;
    }
    svt_alloc_optim_begin(m, followed);
    match kind {
        1 => fit_patch(m, o, fit, src, dst),
        4 => fit_mesh(m, o, fit, src, dst),
        2 => fit_face(m, fit, src, dst),
        6 => fit_record(m, fit, src, dst),
        9 => fit_control_points(m, fit, src, dst, 4, 0x2c, 0x3b),
        10 => fit_control_points(m, fit, src, dst, 0x48, 0x54, 0x5b),
        _ => {}
    }
    svt_alloc_optim_end(m);
    let matrix = m.u32(m.u32(fit + 4) + 0x44).wrapping_add(0x1c4);
    multiply_def(m, o, matrix, kind, dst);
}
