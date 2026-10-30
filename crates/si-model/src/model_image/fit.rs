// SPDX-License-Identifier: MIT
//! A definition bent along a curve (`SI_splineFit`, `model.c`) on the image: the frame of
//! the routine, a patch, a face (of splines or of NURBS curves), a mesh (its normals too), a
//! spline, a NURBS curve and a NURBS surface: the whole routine. Its callees are other
//! routines (through [`Outside`], or [`multiply_def`]).
//!
//! Every point goes the same way: through the fitting's matrix (`SI_SplFitting` +0x28,
//! si-deform `records.rs`) as a row vector, column 1 being the distance along the curve
//! `u`, columns 0 and 2 the offsets `a` and `b`; the curve's package answers the point `P`
//! and the frame `F` (three axes, nine floats) at `u`
//! (`SI_curveDistanceToValueFrenet`, `deformation.c`, through [`Outside`]); the point
//! becomes `P + a·F1 + b·F3`. Each case rounds at its own places (the compiler kept other
//! values in registers), so each is written from its own code.

use si_mem::Mem;
use si_x87::{x, F80};

use super::readjust::multiply_def;
use super::{g, mx, Callee, Outside};

/// The routines of other source files only the fitting calls, each `name (0xADDRESS)`.
pub mod callees {
    use super::Callee;

    /// `SI_curveDistanceToValueFrenet` (0x081561dc; `deformation.c`): the point and the
    /// frame at a distance along the fitting's curve (spline or NURBS curve).
    pub const SI_CURVE_DISTANCE_TO_VALUE_FRENET: Callee = Callee { name: "SI_curveDistanceToValueFrenet", addr: 0x081561dc };
    /// `SI_patchComputeSurface` (0x0847c1d0): a patch's display curves from its points.
    pub const SI_PATCH_COMPUTE_SURFACE: Callee = Callee { name: "SI_patchComputeSurface", addr: 0x0847c1d0 };
    /// `SI_meshComputesPolygonNormal` (0x084599f4): a mesh's polygon normals (all for −1).
    pub const SI_MESH_COMPUTES_POLYGON_NORMAL: Callee = Callee { name: "SI_meshComputesPolygonNormal", addr: 0x084599f4 };
    /// `SI_meshComputesVertexNormal` (0x08459e8c): a mesh's corner normals (all for −1).
    pub const SI_MESH_COMPUTES_VERTEX_NORMAL: Callee = Callee { name: "SI_meshComputesVertexNormal", addr: 0x08459e8c };
    /// Every routine of this table.
    pub const ALL: &[Callee] = &[SI_CURVE_DISTANCE_TO_VALUE_FRENET, SI_PATCH_COMPUTE_SURFACE, SI_MESH_COMPUTES_POLYGON_NORMAL, SI_MESH_COMPUTES_VERTEX_NORMAL];
}

/// The matrix of the fitting, element (row, column), at `fit + 0x28`.
fn el(m: &Mem, fit: u32, row: u32, col: u32) -> f32 {
    m.f32(fit + 0x28 + 16 * row + 4 * col)
}

/// Columns 0, 1 and 2 of `p` through the fitting's matrix, each rounded to float, in the
/// order the original forms them: column 0 starts with y in the patch case
/// (`y_first`), with x in the NURBS cases; column 1 always with y; column 2 is
/// `z·m[2][2] + (y·m[1][2] + x·m[0][2])`. Only which NaN survives depends on the order.
fn through(m: &Mem, fit: u32, p: [f32; 3], y_first: bool) -> (f32, f32, f32) {
    let e = |r, c| el(m, fit, r, c);
    let [px, py, pz] = p;
    // the same value either way; the operand order only decides between two NaNs
    #[allow(clippy::if_same_then_else)]
    let c0 = if y_first { x(py) * e(1, 0) + x(px) * e(0, 0) } else { x(px) * e(0, 0) + x(py) * e(1, 0) };
    let a = ((c0 + x(pz) * e(2, 0)) + e(3, 0)).to_f32();
    let u = (((x(py) * e(1, 1) + x(px) * e(0, 1)) + x(pz) * e(2, 1)) + e(3, 1)).to_f32();
    let b = ((x(pz) * e(2, 2) + (x(py) * e(1, 2) + x(px) * e(0, 2))) + e(3, 2)).to_f32();
    (a, u, b)
}

/// The point and the frame at `u` along the fitting's curve: `SI_curveDistanceToValueFrenet
/// (fit, u, &P, fit + 8, &F)` with both on the stack; answers `P` and `F` as read back.
fn frenet(m: &mut Mem, o: &mut dyn Outside, fit: u32, u: f32) -> ([f32; 3], [f32; 9]) {
    let point = m.local(12);
    let frame = m.local(36);
    o.call(m, &callees::SI_CURVE_DISTANCE_TO_VALUE_FRENET, &[fit, u.to_bits(), point, fit.wrapping_add(8), frame]);
    let p = [m.f32(point), m.f32(point + 4), m.f32(point + 8)];
    let mut f = [0f32; 9];
    for (k, v) in f.iter_mut().enumerate() {
        *v = m.f32(frame + 4 * k as u32);
    }
    m.release(36);
    m.release(12);
    (p, f)
}

/// One point of a patch, a mesh or a spline, `s` read, `d` written: through the matrix (column 0
/// starting with y), the frame at `u`, then x = f32(f32(a·F1x + Px) + b·F3x) with both
/// products unrounded; y = f32(f32(Py + f32(a·F1y)) + f32(b·F3y)), z likewise (the same
/// code in the three cases). Answers the frame, which the mesh's normals use.
fn fit_point(m: &mut Mem, o: &mut dyn Outside, fit: u32, s: u32, d: u32) -> [f32; 9] {
    let (a, u, b) = through(m, fit, [m.f32(s), m.f32(s + 4), m.f32(s + 8)], true);
    let (p, f) = frenet(m, o, fit, u);
    let x0 = (x(a) * f[0] + p[0]).to_f32();
    let y0 = (x(p[1]) + (x(a) * f[1]).to_f32()).to_f32();
    let z0 = (x(p[2]) + (x(a) * f[2]).to_f32()).to_f32();
    m.set_f32(d, x0);
    m.set_f32(d + 4, y0);
    m.set_f32(d + 8, z0);
    m.set_f32(d, (x(b) * f[6] + x0).to_f32());
    m.set_f32(d + 4, (x(y0) + (x(b) * f[7]).to_f32()).to_f32());
    m.set_f32(d + 8, (x(z0) + (x(b) * f[8]).to_f32()).to_f32());
    f
}

/// The patch case (0x08471833–0x08471990): the points of `src` (+0x1a4, 16 bytes each, the
/// fourth float left alone) to those of `dst` ([`fit_point`]), `i16(+0xc) × i16(+0xe)` of
/// them (a 16-bit product, none when it is not positive), then
/// `SI_patchComputeSurface(dst, 0)`.
fn fit_patch(m: &mut Mem, o: &mut dyn Outside, fit: u32, src: u32, dst: u32) {
    let mut n = m.i16(src + 0xc).wrapping_mul(m.i16(src + 0xe));
    let mut s = m.u32(src + 0x1a4);
    let mut d = m.u32(dst + 0x1a4);
    while n > 0 {
        fit_point(m, o, fit, s, d);
        s = s.wrapping_add(16);
        d = d.wrapping_add(16);
        n = n.wrapping_sub(1);
    }
    o.call(m, &callees::SI_PATCH_COMPUTE_SURFACE, &[dst, 0]);
}

/// The spline case (0x08472157–0x08472633): the keys of `src` (+0x14, 0x30 bytes each) to
/// those of `dst` (+0x14), both arrays read once; none when the 16-bit count `src + 0x10` is
/// 0, else while the 16-bit index is below it as unsigned, read again after each key. For
/// type 1 (`i32 src + 0xc`, read once) the three points at +0, +0xc and +0x18 of each key,
/// else the one at +0xc, each as [`fit_point`] (the same code as the patch's).
fn fit_spline(m: &mut Mem, o: &mut dyn Outside, fit: u32, src: u32, dst: u32) {
    let three = m.i32(src + 0xc) == 1;
    let mut s = m.u32(src + 0x14);
    let mut d = m.u32(dst + 0x14);
    if m.u16(src + 0x10) == 0 {
        return;
    }
    let mut k = 0i16;
    loop {
        let points: &[u32] = if three { &[0, 0xc, 0x18] } else { &[0xc] };
        for &at in points {
            fit_point(m, o, fit, s.wrapping_add(at), d.wrapping_add(at));
        }
        s = s.wrapping_add(0x30);
        d = d.wrapping_add(0x30);
        k = k.wrapping_add(1);
        if k as i32 >= m.u16(src + 0x10) as i32 {
            break;
        }
    }
}

/// The face case when its curves are splines (0x08471990–0x08471f20, the 16-bit word
/// `src + 0x84` not 0): the outline (the spline record at +4 of each definition) and then
/// each hole (`+0x7c`, 0x74 bytes each, the array's address read again for each hole) of
/// `src` to the same curve of `dst`, while the 16-bit index, from −1 for the outline, is
/// below the 16-bit count `src + 0x78` (signed, read again each time; none at all when it
/// is negative). Each curve as the spline case ([`fit_spline`]: the same instructions, the
/// type, count and keys at +0xc, +0x10 and +0x14 of the curve's record).
fn fit_face_splines(m: &mut Mem, o: &mut dyn Outside, fit: u32, src: u32, dst: u32) {
    let mut k = -1i16;
    while k < m.i16(src + 0x78) {
        let (sc, dc) = if k == -1 {
            (src.wrapping_add(4), dst.wrapping_add(4))
        } else {
            let off = (k as i32 * 0x74) as u32;
            (m.u32(src + 0x7c).wrapping_add(off), m.u32(dst + 0x7c).wrapping_add(off))
        };
        k = k.wrapping_add(1);
        fit_spline(m, o, fit, sc, dc);
    }
}

/// The face case when its curves are NURBS curves (0x08471f20–0x08472157): curve `k` of
/// `src` (+0x80, 100 bytes each, while the 16-bit `k` is below the 16-bit count `+0x78`, read
/// again each time) to curve `k` of `dst`; for each of its points (count `i32 +4`, read
/// again; points `+0x2c`, 0x24 bytes, x, y, z, w as doubles, the array's address read again
/// at each access; a 16-bit index):
///
/// In: r = 1 / W (the 80-bit quotient, kept), p = f32(X·r), f32(Y·r), f32(r·Z); through the
/// matrix (column 0 starting with x) and the frame at `u` as the NURBS cases. Out, each sum
/// s = b·F3 + (a·F1 + P) with both products unrounded: x = f64(s) from the 80-bit sum, y and
/// z rounded to float first; w = 1 (the 1.0 the unit kept since the loop's start). After
/// each curve, even without points, the flags byte +0x3b of `src`'s then `dst`'s gets 0x60.
fn fit_face_nurbs(m: &mut Mem, o: &mut dyn Outside, fit: u32, src: u32, dst: u32) {
    let mut k = 0i16;
    while k < m.i16(src + 0x78) {
        let off = (k as i32 as u32).wrapping_mul(100);
        let sc = m.u32(src + 0x80).wrapping_add(off);
        let dc = m.u32(dst + 0x80).wrapping_add(off);
        k = k.wrapping_add(1);
        if m.i32(sc + 4) > 0 {
            let mut j = 0i16;
            loop {
                let at = (j as i32 as u32).wrapping_mul(0x24);
                let s = |m: &Mem| m.u32(sc + 0x2c).wrapping_add(at);
                let r = F80::ONE / m.f64(s(m) + 0x18);
                let p = [(m.f64(s(m)) * r).to_f32(), (m.f64(s(m) + 8) * r).to_f32(), (r * m.f64(s(m) + 0x10)).to_f32()];
                let (a, u, b) = through(m, fit, p, false);
                let (pt, f) = frenet(m, o, fit, u);
                let sum = |c: usize| x(b) * f[6 + c] + (x(a) * f[c] + pt[c]);
                let d = |m: &Mem| m.u32(dc + 0x2c).wrapping_add(at);
                let out = [sum(0).to_f64(), x(sum(1).to_f32()).to_f64(), x(sum(2).to_f32()).to_f64(), F80::ONE.to_f64()];
                for (c, v) in out.into_iter().enumerate() {
                    let q = d(m).wrapping_add(8 * c as u32);
                    m.set_f64(q, v);
                }
                j = j.wrapping_add(1);
                if j as i32 >= m.i32(sc + 4) {
                    break;
                }
            }
        }
        m.set_u8(sc + 0x3b, m.u8(sc + 0x3b) | 0x60);
        m.set_u8(dc + 0x3b, m.u8(dc + 0x3b) | 0x60);
    }
}

/// 0.0001 (`.rodata` 0x08660110, a double): a fitted normal no longer than this keeps the
/// length the matrices gave it.
const SHORT: f64 = 0.0001;

/// The length of the normal at `n`, as the mesh case forms it: f32(√((x² + y²) + z²)).
pub(super) fn length(m: &Mem, n: u32) -> f32 {
    let [nx, ny, nz] = [m.f32(n), m.f32(n + 4), m.f32(n + 8)];
    ((x(nx) * nx + x(ny) * ny) + x(nz) * nz).sqrt().to_f32()
}

/// One polygon's normal at vertex `i` (0x084714ca–0x0847164c): entry `at` of the normal
/// lists (+0xc, 0x24-byte entries, the normal at +8) of polygon `sp` of `src` and `dp` of
/// `dst`. The old normal n goes through the fitting's normal matrix (`fit` + `base`: 0x68 for a spline fitting, 0x6c for a patch fitting) as a
/// row vector, the three columns c0, c1, c2 kept unrounded, then through the frame copied
/// into `local` (rows F1, F2, F3 and `SI_identite`'s translation row t): x = f32(((c1·F2x
/// + c0·F1x) + c2·F3x) + tx), y likewise, z = f32((c2·F3z + (c1·F2z + c0·F1z)) + tz).
/// When the new length (as float) is above 0.0001 in absolute value, each coordinate is
/// multiplied by old ÷ new (both floats, the quotient unrounded). The destination's list
/// is read again at each store, as the original does.
pub(super) fn fit_normal(m: &mut Mem, fit: u32, base: u32, sp: u32, dp: u32, at: u32, local: u32) {
    let e = |m: &Mem, r: u32, c: u32| m.f32(fit + base + 16 * r + 4 * c);
    let l = |m: &Mem, r: u32, c: u32| m.f32(local + 16 * r + 4 * c);
    let n = m.u32(sp + 0xc).wrapping_add(at) + 8;
    let old = length(m, n);
    let [nx, ny, nz] = [m.f32(n), m.f32(n + 4), m.f32(n + 8)];
    let c0 = ((x(ny) * e(m, 1, 0) + x(nx) * e(m, 0, 0)) + x(nz) * e(m, 2, 0)) + e(m, 3, 0);
    let c1 = ((x(nx) * e(m, 0, 1) + x(ny) * e(m, 1, 1)) + x(nz) * e(m, 2, 1)) + e(m, 3, 1);
    let c2 = (x(nz) * e(m, 2, 2) + (x(ny) * e(m, 1, 2) + x(nx) * e(m, 0, 2))) + e(m, 3, 2);
    let out = |m: &Mem| m.u32(dp + 0xc).wrapping_add(at) + 8;
    let vx = (((c1 * l(m, 1, 0)) + c0 * l(m, 0, 0)) + c2 * l(m, 2, 0)) + l(m, 3, 0);
    m.set_f32(out(m), vx.to_f32());
    let vy = (((c1 * l(m, 1, 1)) + c0 * l(m, 0, 1)) + c2 * l(m, 2, 1)) + l(m, 3, 1);
    m.set_f32(out(m) + 4, vy.to_f32());
    let vz = (c2 * l(m, 2, 2) + (c1 * l(m, 1, 2) + c0 * l(m, 0, 2))) + l(m, 3, 2);
    m.set_f32(out(m) + 8, vz.to_f32());
    let new = length(m, out(m));
    if !x(new).abs().gt(x(SHORT)) {
        return;
    }
    let r = x(old) / new;
    let q = out(m);
    m.set_f32(q, (x(m.f32(q)) * r).to_f32());
    let q = out(m);
    m.set_f32(q + 4, (x(m.f32(q + 4)) * r).to_f32());
    let q = out(m);
    m.set_f32(q + 8, (r * m.f32(q + 8)).to_f32());
}

/// The mesh case (0x08471287–0x08471833): each vertex of `src` (`+0`, 0x14 bytes, the
/// count `i32 +0x14`, a 16-bit index) to the same vertex of `dst` ([`fit_point`]).
///
/// Without bit 4 of the byte at `src + 0x20`: then `SI_meshComputesPolygonNormal(dst, −1)`
/// and `SI_meshComputesVertexNormal(dst, −1)`.
///
/// With it, the normals of the polygons around each vertex are bent along too:
/// `SI_matrixCopy(SI_identite, local)` first; after each vertex's point, rows 0 to 2 of the
/// local get the frame (F1, F2, F3, each float moved through the unit), and for each of
/// the vertex's polygon references in `dst` (+0xc, 12 bytes each, `u16 +0x10` of them; the
/// polygon number a 16-bit word at +8, 0xffff none, else sign-extended; polygons are 0x18
/// bytes at `+4`, the normal list's count `u16 +0x10`) every entry of `src`'s polygon
/// whose vertex (`i32 +0`) is this one goes through [`fit_normal`]. Then
/// `SI_meshComputesPolygonNormal(dst, −1)` only.
fn fit_mesh(m: &mut Mem, o: &mut dyn Outside, fit: u32, src: u32, dst: u32) {
    if m.u8(src + 0x20) & 4 == 0 {
        let (mut s, mut d) = (m.u32(src), m.u32(dst));
        let mut i = 0i16;
        while (i as i32) < m.i32(src + 0x14) {
            fit_point(m, o, fit, s, d);
            s = s.wrapping_add(0x14);
            d = d.wrapping_add(0x14);
            i = i.wrapping_add(1);
        }
        o.call(m, &callees::SI_MESH_COMPUTES_POLYGON_NORMAL, &[dst, 0xffff_ffff]);
        o.call(m, &callees::SI_MESH_COMPUTES_VERTEX_NORMAL, &[dst, 0xffff_ffff]);
        return;
    }
    let local = m.local(64);
    mx::copy(m, g::SI_IDENTITE.addr(), local);
    let (mut s, mut d) = (m.u32(src), m.u32(dst));
    let mut i = 0i16;
    while (i as i32) < m.i32(src + 0x14) {
        let f = fit_point(m, o, fit, s, d);
        for r in 0..3u32 {
            for c in 0..3u32 {
                m.set_f32(local + 16 * r + 4 * c, si_x87::fmov(f[(3 * r + c) as usize]));
            }
        }
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
                    fit_normal(m, fit, 0x68, sp, dp, at, local);
                }
                k = k.wrapping_add(1);
            }
        }
        s = s.wrapping_add(0x14);
        d = d.wrapping_add(0x14);
        i = i.wrapping_add(1);
    }
    m.release(64);
    o.call(m, &callees::SI_MESH_COMPUTES_POLYGON_NORMAL, &[dst, 0xffff_ffff]);
}

/// The NURBS cases: curve (0x08472633–0x08472810: count `i32 +4`, points `+0x2c`, flags
/// byte `+0x3b`) and surface (0x08472810–0x084729e1: `+0x48`, `+0x54`, `+0x5b`). Points
/// are 0x24 bytes, x, y, z, w as doubles; a 16-bit index.
///
/// In: p = f32(X / W) each (the quotient of the two doubles, rounded once). Out, with W′
/// the destination point's weight as a double, read at each use: x = f64((a·F1x + Px)·W′),
/// y = f64((Py + f32(a·F1y))·W′), z likewise; then x = f64(b·F3x·W′ + x), y = f64(f32(b·F3y)
/// ·W′ + y), z likewise. Then the flags byte of `src`, then of `dst`, gets bits 0x60.
fn fit_nurbs(m: &mut Mem, o: &mut dyn Outside, fit: u32, src: u32, dst: u32, (count, points, flags): (u32, u32, u32)) {
    if m.i32(src + count) > 0 {
        let mut k = 0i16;
        loop {
            let at = (k as i32 as u32).wrapping_mul(0x24);
            let s = m.u32(src + points).wrapping_add(at);
            let w = m.f64(s + 0x18);
            let q = |v: f64| (x(v) / w).to_f32();
            let p = [q(m.f64(s)), q(m.f64(s + 8)), q(m.f64(s + 0x10))];
            let (a, u, b) = through(m, fit, p, false);
            let (pt, f) = frenet(m, o, fit, u);
            let weight = |m: &Mem| m.f64(m.u32(dst + points).wrapping_add(at) + 0x18);
            let first: [F80; 3] = [x(a) * f[0] + pt[0], x(pt[1]) + (x(a) * f[1]).to_f32(), x(pt[2]) + (x(a) * f[2]).to_f32()];
            for (c, v) in first.into_iter().enumerate() {
                let w = weight(m);
                let d = m.u32(dst + points).wrapping_add(at) + 8 * c as u32;
                m.set_f64(d, (v * w).to_f64());
            }
            let second: [F80; 3] = [x(b) * f[6], x((x(b) * f[7]).to_f32()), x((x(b) * f[8]).to_f32())];
            for (c, v) in second.into_iter().enumerate() {
                let w = weight(m);
                let d = m.u32(dst + points).wrapping_add(at) + 8 * c as u32;
                m.set_f64(d, (v * w + m.f64(d)).to_f64());
            }
            k = k.wrapping_add(1);
            if (k as i32) >= m.i32(src + count) {
                break;
            }
        }
    }
    m.set_u8(src + flags, m.u8(src + flags) | 0x60);
    m.set_u8(dst + flags, m.u8(dst + flags) | 0x60);
}

/// `SI_splineFit` [soft 0x08471250 SI_splineFit] on the image. `fit` is the spline
/// deformation, `kind` the kind of both definitions, `src` the one read, `dst` the one
/// written (the working definition; they may be the same).
///
/// Nothing at all unless the followed node (`fit + 4`) is a spline (kind 6) or a NURBS
/// curve (9). Then by `kind`: a patch (1), a face (2: of splines when the 16-bit word
/// `src + 0x84` is not 0, else of NURBS curves), a mesh (4), a spline (6), a NURBS curve (9)
/// or surface (10) as `fit_patch`, `fit_face_splines`, `fit_face_nurbs`, `fit_mesh`,
/// `fit_spline` and `fit_nurbs` say; any other kind (and any outside 1 to 10) nothing.
/// Last, whatever the kind, `dst` goes through the matrix at `+0x1c4` of the record the
/// followed node's word +0x44 points to ([`multiply_def`], `SI_modelMultiplyDef`).
///
/// The point and frame along a NURBS curve (`SI_nurbsDistanceToValueFrenet`) are not this
/// routine's: it reaches them only through `SI_curveDistanceToValueFrenet`.
pub fn spline_fit(m: &mut Mem, o: &mut dyn Outside, fit: u32, kind: i32, src: u32, dst: u32) {
    let followed = m.u32(fit + 4);
    let curve = m.i32(followed + 0x30);
    if curve != 6 && curve != 9 {
        return;
    }
    match kind {
        2 if m.i16(src + 0x84) != 0 => fit_face_splines(m, o, fit, src, dst),
        1 => fit_patch(m, o, fit, src, dst),
        2 => fit_face_nurbs(m, o, fit, src, dst),
        4 => fit_mesh(m, o, fit, src, dst),
        6 => fit_spline(m, o, fit, src, dst),
        9 => fit_nurbs(m, o, fit, src, dst, (0x04, 0x2c, 0x3b)),
        10 => fit_nurbs(m, o, fit, src, dst, (0x48, 0x54, 0x5b)),
        _ => {}
    }
    let matrix = m.u32(m.u32(fit + 4) + 0x44).wrapping_add(0x1c4);
    multiply_def(m, o, matrix, kind, dst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_image::Callee;

    /// Answers the frame call with a fixed point and frame; records every call.
    struct Fixed(Vec<(&'static str, Vec<u32>)>, [f32; 3], [f32; 9]);
    impl Outside for Fixed {
        fn call(&mut self, m: &mut Mem, c: &Callee, a: &[u32]) -> u32 {
            self.0.push((c.name, a.to_vec()));
            if c.name == "SI_curveDistanceToValueFrenet" {
                for k in 0..3 {
                    m.set_f32(a[2] + 4 * k, self.1[k as usize]);
                }
                for k in 0..9 {
                    m.set_f32(a[4] + 4 * k, self.2[k as usize]);
                }
            }
            0
        }
    }

    /// A fitting whose matrix is the identity, following a node of `curve` kind whose
    /// word +0x44 points to a record with the identity at +0x1c4.
    fn world(m: &mut Mem, curve: i32) -> u32 {
        let fit = m.si_malloc(0xa8);
        let node = m.si_malloc(0x430);
        let rec = m.si_malloc(0x430);
        m.set_u32(fit + 4, node);
        m.set_i32(node + 0x30, curve);
        m.set_u32(node + 0x44, rec);
        for i in 0..4 {
            m.set_f32(fit + 0x28 + 20 * i, 1.0);
            m.set_f32(rec + 0x1c4 + 20 * i, 1.0);
        }
        fit
    }

    fn mem() -> Mem {
        let mut m = Mem::new();
        crate::model_image::init(&mut m);
        m
    }

    #[test]
    fn not_a_curve_does_nothing() {
        si_rt::skip_without_install!();
        let mut m = mem();
        let fit = world(&mut m, 4);
        let mut o = Fixed(vec![], [0.0; 3], [0.0; 9]);
        spline_fit(&mut m, &mut o, fit, 6, 0, 0);
        assert!(o.0.is_empty());
    }


    /// A spline of `type_` with two keys (0x30 bytes each): key k's three points are
    /// (2 + k, 5, 3), (9, -1, 0.5 + k), (0.25, 7, 1).
    fn spline(m: &mut Mem, type_: i32) -> u32 {
        let def = m.si_malloc(0x1c4);
        let keys = m.si_malloc(0x60);
        m.set_i32(def + 0xc, type_);
        // the count is the low 16 bits
        m.set_u32(def + 0x10, 0x7fff_0002);
        m.set_u32(def + 0x14, keys);
        for k in 0..2u32 {
            for (i, v) in [2.0 + k as f32, 5.0, 3.0, 9.0, -1.0, 0.5 + k as f32, 0.25, 7.0, 1.0].iter().enumerate() {
                m.set_f32(keys + 0x30 * k + 4 * i as u32, *v);
            }
            m.set_f32(keys + 0x30 * k + 0x24, 42.0);
        }
        def
    }

    #[test]
    fn spline_of_type_1_moves_three_points_per_key() {
        si_rt::skip_without_install!();
        let mut m = mem();
        let fit = world(&mut m, 6);
        let def = spline(&mut m, 1);
        let keys = m.u32(def + 0x14);
        let mut o = Fixed(vec![], [10.0, 20.0, 30.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        spline_fit(&mut m, &mut o, fit, 6, def, def);
        let f3 = |m: &Mem, a: u32| [m.f32(a), m.f32(a + 4), m.f32(a + 8)];
        // out = P + x·F1 + z·F3, u = y
        assert_eq!(f3(&m, keys), [12.0, 20.0, 33.0]);
        assert_eq!(f3(&m, keys + 0xc), [19.0, 20.0, 30.5]);
        assert_eq!(f3(&m, keys + 0x18), [10.25, 20.0, 31.0]);
        assert_eq!(f3(&m, keys + 0x30), [13.0, 20.0, 33.0]);
        assert_eq!(f3(&m, keys + 0x3c), [19.0, 20.0, 31.5]);
        // the rest of the key stays
        assert_eq!(m.f32(keys + 0x24), 42.0);
        let us: Vec<f32> = o.0.iter().filter(|c| c.0 == "SI_curveDistanceToValueFrenet").map(|c| f32::from_bits(c.1[1])).collect();
        assert_eq!(us, [5.0, -1.0, 7.0, 5.0, -1.0, 7.0]);
    }

    #[test]
    fn spline_of_another_type_moves_the_middle_point() {
        si_rt::skip_without_install!();
        let mut m = mem();
        let fit = world(&mut m, 6);
        let src = spline(&mut m, 0x10001);
        let dst = spline(&mut m, 1);
        let (sk, dk) = (m.u32(src + 0x14), m.u32(dst + 0x14));
        let mut o = Fixed(vec![], [10.0, 20.0, 30.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        spline_fit(&mut m, &mut o, fit, 6, src, dst);
        let f3 = |m: &Mem, a: u32| [m.f32(a), m.f32(a + 4), m.f32(a + 8)];
        assert_eq!(f3(&m, dk + 0xc), [19.0, 20.0, 30.5]);
        assert_eq!(f3(&m, dk + 0x3c), [19.0, 20.0, 31.5]);
        // the other points of the destination and the source stay
        assert_eq!(f3(&m, dk), [2.0, 5.0, 3.0]);
        assert_eq!(f3(&m, sk + 0xc), [9.0, -1.0, 0.5]);
        assert_eq!(o.0.iter().filter(|c| c.0 == "SI_curveDistanceToValueFrenet").count(), 2);
    }

    /// A face of splines: the outline (the spline record at +4: type `outline`, one key)
    /// and `holes` holes of 0x74 bytes (each of type 1, one key), every key's three points
    /// those of [`spline`]'s first key; +0x84 not 0 in its low half.
    fn face(m: &mut Mem, outline: i32, holes: i16) -> u32 {
        let def = m.si_malloc(0x1c4);
        let ho = m.si_malloc(0x74 * 3);
        m.set_u32(def + 0x84, 0x0000_0001);
        m.set_u32(def + 0x78, 0x5a5a_0000 | holes as u16 as u32);
        m.set_u32(def + 0x7c, ho);
        let key = |m: &mut Mem| {
            let k = m.si_malloc(0x30);
            for (i, v) in [2.0f32, 5.0, 3.0, 9.0, -1.0, 0.5, 0.25, 7.0, 1.0].iter().enumerate() {
                m.set_f32(k + 4 * i as u32, *v);
            }
            k
        };
        let k = key(m);
        m.set_i32(def + 4 + 0xc, outline);
        m.set_u16(def + 4 + 0x10, 1);
        m.set_u32(def + 4 + 0x14, k);
        for h in 0..3 {
            let k = key(m);
            m.set_i32(ho + 0x74 * h + 0xc, 1);
            m.set_u16(ho + 0x74 * h + 0x10, 1);
            m.set_u32(ho + 0x74 * h + 0x14, k);
        }
        def
    }

    #[test]
    fn face_of_splines_moves_the_outline_then_each_hole() {
        si_rt::skip_without_install!();
        let mut m = mem();
        let fit = world(&mut m, 6);
        let def = face(&mut m, 0, 2);
        let mut o = Fixed(vec![], [10.0, 20.0, 30.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        spline_fit(&mut m, &mut o, fit, 2, def, def);
        let f3 = |m: &Mem, a: u32| [m.f32(a), m.f32(a + 4), m.f32(a + 8)];
        // the outline (another type): its middle point only
        let ok = m.u32(def + 4 + 0x14);
        assert_eq!(f3(&m, ok), [2.0, 5.0, 3.0]);
        assert_eq!(f3(&m, ok + 0xc), [19.0, 20.0, 30.5]);
        // the two holes (type 1): three points each; the third is not counted
        let ho = m.u32(def + 0x7c);
        for h in 0..2 {
            let k = m.u32(ho + 0x74 * h + 0x14);
            assert_eq!([f3(&m, k), f3(&m, k + 0xc), f3(&m, k + 0x18)], [[12.0, 20.0, 33.0], [19.0, 20.0, 30.5], [10.25, 20.0, 31.0]]);
        }
        assert_eq!(f3(&m, m.u32(ho + 0x74 * 2 + 0x14)), [2.0, 5.0, 3.0]);
        let us: Vec<f32> = o.0.iter().filter(|c| c.0 == "SI_curveDistanceToValueFrenet").map(|c| f32::from_bits(c.1[1])).collect();
        assert_eq!(us, [-1.0, 5.0, -1.0, 7.0, 5.0, -1.0, 7.0]);
    }

    #[test]
    fn face_of_splines_with_a_negative_count_moves_nothing() {
        si_rt::skip_without_install!();
        let mut m = mem();
        let fit = world(&mut m, 6);
        let def = face(&mut m, 1, -1);
        let mut o = Fixed(vec![], [10.0, 20.0, 30.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        spline_fit(&mut m, &mut o, fit, 2, def, def);
        assert_eq!(m.f32(m.u32(def + 4 + 0x14)), 2.0);
        assert!(o.0.iter().all(|c| c.0 != "SI_curveDistanceToValueFrenet"));
    }

    #[test]
    fn face_nurbs_point_is_divided_and_gets_weight_one() {
        si_rt::skip_without_install!();
        let mut m = mem();
        let fit = world(&mut m, 6);
        let src = m.si_malloc(0x1c4);
        let dst = m.si_malloc(0x1c4);
        let (sc, dc) = (m.si_malloc(200), m.si_malloc(200));
        let (a, b) = (m.si_malloc(0x24), m.si_malloc(0x24));
        // the high half of +0x84 does not count; two curves, the second without points
        m.set_u32(src + 0x84, 0x1234_0000);
        m.set_u16(src + 0x78, 2);
        m.set_u32(src + 0x80, sc);
        m.set_u32(dst + 0x80, dc);
        m.set_i32(sc + 4, 1);
        m.set_u32(sc + 0x2c, a);
        m.set_u32(dc + 0x2c, b);
        for (k, v) in [4.0f64, 6.0, 8.0, 2.0].iter().enumerate() {
            m.set_f64(a + 8 * k as u32, *v);
        }
        m.set_f64(b + 0x18, 3.0);
        let mut o = Fixed(vec![], [1.0, 1.0, 1.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        spline_fit(&mut m, &mut o, fit, 2, src, dst);
        // p = (2, 3, 4): out = (1 + 2, 1, 1 + 4), w = 1 whatever the destination had
        assert_eq!([m.f64(b), m.f64(b + 8), m.f64(b + 0x10), m.f64(b + 0x18)], [3.0, 1.0, 5.0, 1.0]);
        assert_eq!(o.0[0].1[1], 3.0f32.to_bits());
        for c in [sc, dc, sc + 100, dc + 100] {
            assert_eq!(m.u8(c + 0x3b), 0x60);
        }
        assert_eq!(o.0.iter().filter(|c| c.0 == "SI_curveDistanceToValueFrenet").count(), 1);
    }

    #[test]
    fn patch_point_follows_the_frame() {
        si_rt::skip_without_install!();
        let mut m = mem();
        let fit = world(&mut m, 6);
        let def = m.si_malloc(0x1c4);
        let pts = m.si_malloc(32);
        m.set_u32(def + 0x1a4, pts);
        m.set_u16(def + 0xc, 1);
        m.set_u16(def + 0xe, 2);
        for (k, v) in [2.0f32, 5.0, 3.0, 9.0, -1.0, 0.5, 0.25, 7.0].iter().enumerate() {
            m.set_f32(pts + 4 * k as u32, *v);
        }
        // P = (10, 20, 30); F1 = x axis, F3 = z axis
        let mut o = Fixed(vec![], [10.0, 20.0, 30.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        spline_fit(&mut m, &mut o, fit, 1, def, def);
        // u = y, out = P + x·F1 + z·F3; the fourth float stays
        assert_eq!([m.f32(pts), m.f32(pts + 4), m.f32(pts + 8), m.f32(pts + 12)], [12.0, 20.0, 33.0, 9.0]);
        assert_eq!([m.f32(pts + 16), m.f32(pts + 20), m.f32(pts + 24), m.f32(pts + 28)], [9.0, 20.0, 30.25, 7.0]);
        let names: Vec<_> = o.0.iter().map(|c| c.0).collect();
        assert_eq!(names, ["SI_curveDistanceToValueFrenet", "SI_curveDistanceToValueFrenet", "SI_patchComputeSurface"]);
        assert_eq!(o.0[0].1[1], 5.0f32.to_bits());
        assert_eq!(o.0[0].1[3], fit + 8);
        assert_eq!(o.0[2].1, vec![def, 0]);
    }

    /// A mesh of `n` vertices (0x14 bytes, the first at (2, 5, 3)) and one polygon whose
    /// corners are the vertices `corners`, each with normal (2, 0, 0); vertex 0 refers to
    /// the polygon; `flags` at +0x20.
    fn mesh(m: &mut Mem, n: u32, corners: &[i32], flags: u32) -> u32 {
        let def = m.si_malloc(0x40);
        let verts = m.si_malloc(0x14 * n);
        let star = m.si_malloc(12);
        let poly = m.si_malloc(0x18);
        let list = m.si_malloc(0x24 * corners.len() as u32);
        m.set_u32(def, verts);
        m.set_u32(def + 4, poly);
        m.set_i32(def + 0x14, n as i32);
        m.set_i32(def + 0x18, 1);
        m.set_u32(def + 0x20, flags);
        for (k, v) in [2.0f32, 5.0, 3.0].iter().enumerate() {
            m.set_f32(verts + 4 * k as u32, *v);
        }
        m.set_u32(verts + 0xc, star);
        m.set_u16(verts + 0x10, 1);
        m.set_u16(star + 8, 0);
        m.set_u32(poly + 0xc, list);
        m.set_u16(poly + 0x10, corners.len() as u16);
        for (k, c) in corners.iter().enumerate() {
            m.set_i32(list + 0x24 * k as u32, *c);
            m.set_f32(list + 0x24 * k as u32 + 8, 2.0);
        }
        def
    }

    #[test]
    fn mesh_without_normals_asks_for_them() {
        si_rt::skip_without_install!();
        let mut m = mem();
        let fit = world(&mut m, 6);
        let def = mesh(&mut m, 2, &[0], 0);
        let mut o = Fixed(vec![], [10.0, 20.0, 30.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        spline_fit(&mut m, &mut o, fit, 4, def, def);
        let verts = m.u32(def);
        assert_eq!([m.f32(verts), m.f32(verts + 4), m.f32(verts + 8)], [12.0, 20.0, 33.0]);
        let names: Vec<_> = o.0.iter().map(|c| c.0).collect();
        assert_eq!(names, ["SI_curveDistanceToValueFrenet", "SI_curveDistanceToValueFrenet", "SI_meshComputesPolygonNormal", "SI_meshComputesVertexNormal"]);
        assert_eq!(o.0[2].1, vec![def, 0xffff_ffff]);
        // the corner's normal is left to the two calls
        assert_eq!(m.f32(m.u32(m.u32(def + 4) + 0xc) + 8), 2.0);
    }

    #[test]
    fn mesh_normals_turn_with_the_frame() {
        si_rt::skip_without_install!();
        let mut m = mem();
        let fit = world(&mut m, 6);
        for i in 0..4 {
            m.set_f32(fit + 0x68 + 20 * i, 1.0);
        }
        let def = mesh(&mut m, 1, &[0, 1], 4);
        // the frame turns x into y: F1 = y, F2 = −x, F3 = z
        let mut o = Fixed(vec![], [0.0; 3], [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        spline_fit(&mut m, &mut o, fit, 4, def, def);
        let list = m.u32(m.u32(def + 4) + 0xc);
        // the corner of vertex 0: (2, 0, 0) becomes (0, 2, 0), its length kept; the corner
        // of vertex 1 is not this vertex's
        assert_eq!([m.f32(list + 8), m.f32(list + 12), m.f32(list + 16)], [0.0, 2.0, 0.0]);
        assert_eq!([m.f32(list + 0x24 + 8), m.f32(list + 0x24 + 12), m.f32(list + 0x24 + 16)], [2.0, 0.0, 0.0]);
        let names: Vec<_> = o.0.iter().map(|c| c.0).collect();
        assert_eq!(names, ["SI_curveDistanceToValueFrenet", "SI_meshComputesPolygonNormal"]);
    }

    #[test]
    fn nurbs_point_is_divided_then_weighted() {
        si_rt::skip_without_install!();
        let mut m = mem();
        let fit = world(&mut m, 9);
        let src = m.si_malloc(0x1c4);
        let dst = m.si_malloc(0x1c4);
        let a = m.si_malloc(0x24);
        let b = m.si_malloc(0x24);
        m.set_i32(src + 4, 1);
        m.set_u32(src + 0x2c, a);
        m.set_u32(dst + 0x2c, b);
        for (k, v) in [4.0f64, 6.0, 8.0, 2.0].iter().enumerate() {
            m.set_f64(a + 8 * k as u32, *v);
        }
        m.set_f64(b + 0x18, 3.0);
        let mut o = Fixed(vec![], [1.0, 1.0, 1.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        spline_fit(&mut m, &mut o, fit, 9, src, dst);
        // p = (2, 3, 4): out = ((1 + 2), 1, (1 + 4)) × 3
        assert_eq!([m.f64(b), m.f64(b + 8), m.f64(b + 0x10), m.f64(b + 0x18)], [9.0, 3.0, 15.0, 3.0]);
        assert_eq!(o.0[0].1[1], 3.0f32.to_bits());
        assert_eq!(m.u8(src + 0x3b), 0x60);
        assert_eq!(m.u8(dst + 0x3b), 0x60);
        // the tail's call for a NURBS curve
        assert_eq!(o.0.last().unwrap().0, "SI_nurbsCrvApplyMatrix");
    }
}
