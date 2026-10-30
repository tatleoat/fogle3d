// SPDX-License-Identifier: MIT
//! The NURBS ray tracer's Bezier-clipping intersection code: the original's `ray_trace.c`
//! (finding 0044), on the memory image. Ported so far: the bounding-box helpers and the
//! trivial reject (bottom-up from the call graph of the file).
//!
//! Records (names are ours): `nurbsBbox` = six doubles, minimum x y z at +0, maximum at
//! +0x18; `SI_NurbsPoint` = x y z w (doubles); `BezPatch`: +0 and +4 the control net's
//! counts, +0x38 / +0x40 the minimum x / y of the net, +0x50 / +0x58 the maximum, +0x98 the
//! control points (0x24 bytes each, x and y first). Arithmetic follows the x87 sequences.

use si_mem::{Mem, Outside, Routine};
use si_x87::F80;

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

/// [soft 0x085147a0 update3DBbox]: `(point, bbox)`: widens the box by the point's
/// homogeneous division (x/w, y/w, z/w: `1/w` is one 80-bit register, each coordinate is
/// stored rounded; the minima are compared with the unrounded x product, the rest rounded).
pub fn update_3d_bbox(m: &mut Mem, pt: u32, bb: u32) {
    let inv = x(1.0) / x(m.f64(pt + 0x18));
    let x80 = x(m.f64(pt)) * inv;
    let xr = x80.to_f64();
    let yr = (x(m.f64(pt + 8)) * inv).to_f64();
    let zr = (inv * x(m.f64(pt + 0x10))).to_f64();
    if x80.lt(x(m.f64(bb))) {
        m.set_f64(bb, xr);
    }
    if x(yr).lt(x(m.f64(bb + 8))) {
        m.set_f64(bb + 8, yr);
    }
    if x(zr).lt(x(m.f64(bb + 0x10))) {
        m.set_f64(bb + 0x10, zr);
    }
    if x(xr).gt(x(m.f64(bb + 0x18))) {
        m.set_f64(bb + 0x18, xr);
    }
    if x(yr).gt(x(m.f64(bb + 0x20))) {
        m.set_f64(bb + 0x20, yr);
    }
    if x(zr).gt(x(m.f64(bb + 0x28))) {
        m.set_f64(bb + 0x28, zr);
    }
}

/// [soft 0x08514740 update2DBbox]: `(point, bbox)`: widens the box's x and y range (the minima
/// at +0 / +8, the maxima at +0x18 / +0x20) by the point's x and y.
pub fn update_2d_bbox(m: &mut Mem, pt: u32, bb: u32) {
    let (px, py) = (m.f64(pt), m.f64(pt + 8));
    if px < m.f64(bb) {
        m.set_f64(bb, px);
    }
    if py < m.f64(bb + 8) {
        m.set_f64(bb + 8, py);
    }
    if m.f64(bb + 0x18) < px {
        m.set_f64(bb + 0x18, px);
    }
    if m.f64(bb + 0x20) < py {
        m.set_f64(bb + 0x20, py);
    }
}

/// [soft 0x08514e7c SI_nurbsCountIntervals]: `(dir)`: the number of knot spans longer than
/// 1e-6 (`dir` +8 the knot count, +0xc the knots).
pub fn nurbs_count_intervals(m: &Mem, dir: u32) -> i32 {
    let (n, knots) = (m.i32(dir + 8), m.u32(dir + 0xc));
    let mut c = 0;
    let mut i = 0i32;
    while i < n.wrapping_sub(1) {
        let a = knots.wrapping_add((i as u32).wrapping_mul(8));
        if x(1e-6).lt(x(m.f64(a + 8)) - x(m.f64(a))) {
            c += 1;
        }
        i += 1;
    }
    c
}

/// [soft 0x085146b0 calculate2DBbox]: `(patch)`: the x / y range of the patch's `nu × nv`
/// control points into +0x38 / +0x40 (minima) and +0x50 / +0x58 (maxima), starting from
/// ±3.4028234663852886e38.
pub fn calculate_2d_bbox(m: &mut Mem, patch: u32) {
    const BIG: f64 = 3.4028234663852886e+38;
    let mut n = m.i32(patch).wrapping_mul(m.i32(patch + 4));
    let mut p = m.u32(patch + 0x98);
    let (mut minx, mut miny, mut maxx, mut maxy) = (BIG, BIG, -BIG, -BIG);
    while n > 0 {
        let (px, py) = (m.f64(p), m.f64(p + 8));
        if px < minx {
            minx = px;
        }
        if maxx < px {
            maxx = px;
        }
        if py < miny {
            miny = py;
        }
        if maxy < py {
            maxy = py;
        }
        p = p.wrapping_add(0x24);
        n -= 1;
    }
    m.set_f64(patch + 0x38, minx);
    m.set_f64(patch + 0x40, miny);
    m.set_f64(patch + 0x50, maxx);
    m.set_f64(patch + 0x58, maxy);
}

/// [soft 0x0851496c trivialReject]: `(patch, tol)`: 1 when the patch's 2-D box is out of the
/// way of the ray at the origin: the box is wider than `tol` in x or in y, and the origin with
/// slack `tol` lies below the minima or above the maxima in either direction (the box minima
/// / maxima are `patch` +0x38 / +0x40 and +0x50 / +0x58).
pub fn trivial_reject(m: &Mem, patch: u32, tol: f64) -> u32 {
    let (x0, y0, x1, y1) = (m.f64(patch + 0x38), m.f64(patch + 0x40), m.f64(patch + 0x50), m.f64(patch + 0x58));
    let t = x(tol);
    let wx = (x(x1) - x(x0)).abs();
    let wy = (x(y1) - x(y0)).abs();
    if t.le(wx) || !wy.lt(t) {
        let d2 = t + x(0.0);
        if d2.lt(x(x0)) || d2.lt(x(y0)) || x(x1).lt(x(0.0) - t) || x(y1).lt(x(0.0) - t) {
            return 1;
        }
    }
    0
}

/// [soft 0x085128c4 perturbMinMax]: `(d, lo, hi)`: widens the interval `[*lo, *hi]` by `d / 2`
/// on each side and clamps it to `[0, 1]`.
pub fn perturb_min_max(m: &mut Mem, d: f64, lo: u32, hi: u32) {
    let h = x(d) * x(0.5);
    let l = x(m.f64(lo)) - h;
    m.set_f64(lo, l.to_f64());
    if l.lt(x(0.0)) {
        m.set_f64(lo, 0.0);
    }
    let u = x(m.f64(hi)) + h;
    m.set_f64(hi, u.to_f64());
    if u.gt(x(1.0)) {
        m.set_f64(hi, 1.0);
    }
}

/// [soft 0x08512a1c calcDistFromLine]: `(pos, npos, neg, nneg, zero, nzero, n, pts, total, a, b,
/// flag)`: for each of the `total` homogeneous points (0x24 bytes each) the signed distance
/// `a * x + b * y` to a line, with a parameter `t` that runs 0, 1/(n-1), ... (restarting at 0
/// every `n` points when `flag`, else stepping every `total / n` points). The pair `(t, dist)`
/// (16 bytes) goes to the positive list when the distance exceeds 1.0000000000000002e-6, to the
/// negative list when it is below that negated, else `(t, 0)` to the zero list; the counts
/// are in memory.
#[allow(clippy::too_many_arguments)]
pub fn calc_dist_from_line(
    m: &mut Mem, pos: u32, npos: u32, neg: u32, nneg: u32, zero: u32, nzero: u32, n: i32, pts: u32, total: i32, a: f64, b: f64,
    flag: i16,
) {
    let step = x(1.0) / F80::from_i32(n.wrapping_sub(1));
    let per = if n == 0 { 0 } else { total.wrapping_div(n) };
    let mut t = x(0.0);
    let mut i = 0i32;
    let mut off = 0u32;
    while i < total {
        if flag == 0 {
            if i != 0 && i.wrapping_rem(per) == 0 {
                t = t + step;
            }
        } else if i.wrapping_rem(n) == 0 {
            t = x(0.0);
        } else {
            t = t + step;
        }
        let p = pts.wrapping_add(off);
        let dist = x(b) * x(m.f64(p + 8)) + x(a) * x(m.f64(p));
        if dist.abs().lt(x(1.0000000000000002e-06)) {
            let k = m.u32(nzero);
            let r = zero.wrapping_add(k.wrapping_shl(4));
            m.set_f64(r, t.to_f64());
            m.set_u32(r + 8, 0);
            m.set_u32(r + 0xc, 0);
            m.set_u32(nzero, k.wrapping_add(1));
        } else if x(0.0).lt(dist) {
            let k = m.u32(npos);
            let r = pos.wrapping_add(k.wrapping_shl(4));
            m.set_f64(r, t.to_f64());
            m.set_f64(r + 8, dist.to_f64());
            m.set_u32(npos, k.wrapping_add(1));
        } else if dist.lt(x(0.0)) {
            let k = m.u32(nneg);
            let r = neg.wrapping_add(k.wrapping_shl(4));
            m.set_f64(r, t.to_f64());
            m.set_f64(r + 8, dist.to_f64());
            m.set_u32(nneg, k.wrapping_add(1));
        }
        off = off.wrapping_add(0x24);
        i += 1;
    }
}

/// [soft 0x08512b68 cleanupBclip]: `(a, a0, b, b0, c, c0)`: frees each of the three arrays
/// that is neither null nor the caller's own stack buffer (`a0`, `b0`, `c0`).
pub fn cleanup_b_clip(m: &mut Mem, a: u32, a0: u32, b: u32, b0: u32, c: u32, c0: u32) {
    for (p, q) in [(a, a0), (b, b0), (c, c0)] {
        if p != q && p != 0 {
            m.free(p);
        }
    }
}

/// [soft 0x08513044 initBClip]: `(pa, pb, pc, sa, sb, sc, n, lo, hi, nu, nv, pts, patch)`:
/// sets up the clipping of one Bezier patch: `*lo = 3.4028234663852886e38`, `*hi = -` that,
/// `*nu`, `*nv` the patch's counts, `*n` their product, `*pts` the patch's points (+0x98);
/// the three working arrays (`*pa`, `*pb`, `*pc`, 16 bytes per point) are the caller's
/// buffers `sa`, `sb`, `sc` when `n < 26`, else three `SI_calloc`s (all given back when one
/// fails: 0 is returned and the failed ones are left null / freed as the original does).
#[allow(clippy::too_many_arguments)]
pub fn init_b_clip(
    m: &mut Mem, pa: u32, pb: u32, pc: u32, sa: u32, sb: u32, sc: u32, n: u32, lo: u32, hi: u32, nu: u32, nv: u32, pts: u32, patch: u32,
) -> u32 {
    m.set_f64(lo, 3.4028234663852886e+38);
    m.set_f64(hi, -3.4028234663852886e+38);
    let a = m.i32(patch);
    m.set_i32(nu, a);
    let b = m.i32(patch + 4);
    m.set_i32(nv, b);
    m.set_i32(n, b.wrapping_mul(a));
    let pp = m.u32(patch + 0x98);
    m.set_u32(pts, pp);
    if m.i32(n) < 0x1a {
        m.set_u32(pa, sa);
        m.set_u32(pb, sb);
        m.set_u32(pc, sc);
        return 1;
    }
    let cnt = m.u32(n);
    let r = m.si_calloc(cnt, 0x10);
    m.set_u32(pa, r);
    if r != 0 {
        let r2 = m.si_calloc(m.u32(n), 0x10);
        m.set_u32(pb, r2);
        if r2 == 0 {
            let f = m.u32(pa);
            if f != 0 {
                m.free(f);
                m.set_u32(pa, 0);
            }
        } else {
            let r3 = m.si_calloc(m.u32(n), 0x10);
            m.set_u32(pc, r3);
            if r3 != 0 {
                return 1;
            }
        }
    }
    0
}

/// [soft 0x08513130 updateMinMaxFromClip]: `(a, na, b, nb, c, nc, lo, hi)`: widens `[*lo, *hi]`
/// by the abscissa where the segment from every point of `a` (`na` pairs of doubles, 16 bytes)
/// to every point of `b` (`nb` pairs) crosses zero ordinate `a.x + (b.x - a.x) * a.y / (a.y - b.y)`
/// (one 80-bit register chain, compared before it is stored), and by the first double of every
/// point of `c` (`nc` pairs).
#[allow(clippy::too_many_arguments)]
pub fn update_min_max_from_clip(m: &mut Mem, a: u32, na: i32, b: u32, nb: i32, c: u32, nc: i32, lo: u32, hi: u32) {
    let mut i = 0i32;
    while i < na {
        let pa = a.wrapping_add((i as u32).wrapping_shl(4));
        let mut j = 0i32;
        while j < nb {
            let pb = b.wrapping_add((j as u32).wrapping_shl(4));
            let ax = x(m.f64(pa));
            let q = ((x(m.f64(pb)) - ax) * x(m.f64(pa + 8))) / (x(m.f64(pa + 8)) - x(m.f64(pb + 8)));
            let v = ax + q;
            if v.lt(x(m.f64(lo))) {
                m.set_f64(lo, v.to_f64());
            }
            if v.gt(x(m.f64(hi))) {
                m.set_f64(hi, v.to_f64());
            }
            j += 1;
        }
        i += 1;
    }
    let mut k = 0i32;
    while k < nc {
        let v = m.f64(c.wrapping_add((k as u32).wrapping_shl(4)));
        if v < m.f64(lo) {
            m.set_f64(lo, v);
        }
        if m.f64(hi) < v {
            m.set_f64(hi, v);
        }
        k += 1;
    }
}

/// [soft 0x08512dc4 buildLine]: `(pts, py, px, n, k, flag)`: the unit normal `(px, py)` (the
/// order of the outputs is as the original's: the second output `py`, the third `px`) of the
/// chord a Bezier clip uses, from the control net of `n` columns by `k` rows (points of
/// 0x24 bytes). Walks pairs of corner sums (`flag`: along the first row / last row, else up the
/// first column) until one has a length of at least 1.0000000000000002e-6; when none does the
/// chord between the two corners is tried. 1 when a normal was found (and normalised), else 0.
pub fn build_line(m: &mut Mem, pts: u32, py: u32, px: u32, n: i32, k: i32, flag: i16) -> u32 {
    const TOL: f64 = 1.0000000000000002e-06;
    let at = |i: i32| pts.wrapping_add((i as u32).wrapping_mul(0x24));
    let nx = |m: &Mem, i: i32| x(m.f64(at(i)));
    let ny = |m: &Mem, i: i32| x(m.f64(at(i) + 8));
    let mut esi = (k.wrapping_sub(1)).wrapping_mul(n); // iVar6
    let mut edi = n.wrapping_sub(1).wrapping_add(esi); // iVar7
    let mut lc = 0i32; // local_c
    let mut l10 = n.wrapping_sub(1); // local_10
    let (mut e1, mut e2) = (l10, lc); // the 4-sum's other two points
    let (mut e3, mut e4) = (l10, 0i32);
    let guard = if flag != 0 { l10 >= 0 } else { esi >= 0 };
    let step = n; // p4
    let normalise = |m: &mut Mem| {
        let inv = x(1.0) / (x(m.f64(py)) * x(m.f64(py)) + x(m.f64(px)) * x(m.f64(px))).sqrt();
        let a = (x(m.f64(py)) * inv).to_f64();
        m.set_f64(py, a);
        let b = (x(m.f64(px)) * inv).to_f64();
        m.set_f64(px, b);
    };
    let short = |m: &Mem| {
        // sqrt(py^2 + px^2) < TOL
        let (a, b) = (x(m.f64(py)), x(m.f64(px)));
        (a * a + b * b).sqrt().lt(x(TOL))
    };
    if guard {
        loop {
            let cont;
            if flag == 0 {
                let vx = ((nx(m, edi) + nx(m, e1)) - nx(m, esi)) - nx(m, e2);
                m.set_f64(px, vx.to_f64());
                let vy = ((ny(m, edi) + ny(m, e1)) - ny(m, esi)) - ny(m, e2);
                m.set_f64(py, (-vy).to_f64());
                esi = esi.wrapping_sub(step);
                e2 = e2.wrapping_add(step);
                lc = lc.wrapping_add(step);
                edi = edi.wrapping_sub(step);
                e1 = e1.wrapping_add(step);
                l10 = l10.wrapping_add(step);
                cont = esi >= lc;
            } else {
                let vx = ((nx(m, edi) + nx(m, esi)) - nx(m, e3)) - nx(m, e4);
                m.set_f64(px, vx.to_f64());
                let vy = ((ny(m, edi) + ny(m, esi)) - ny(m, e3)) - ny(m, e4);
                m.set_f64(py, (-vy).to_f64());
                esi = esi.wrapping_add(1);
                edi = edi.wrapping_sub(1);
                lc = lc.wrapping_add(1);
                e4 = e4.wrapping_add(1);
                e3 = e3.wrapping_sub(1);
                l10 = l10.wrapping_sub(1);
                cont = l10 >= lc;
            }
            if !short(m) {
                normalise(m);
                return 1;
            }
            if !cont {
                break;
            }
        }
    }
    if flag != 0 {
        let s = (k.wrapping_sub(1)).wrapping_mul(n);
        let e = n.wrapping_sub(1).wrapping_add(s);
        m.set_f64(py, (nx(m, e) - nx(m, s)).to_f64());
        let y = ny(m, e) + ny(m, s);
        m.set_f64(px, (-y).to_f64());
    } else {
        let s = (k.wrapping_sub(1)).wrapping_mul(n);
        m.set_f64(py, (nx(m, s) - nx(m, 0)).to_f64());
        let y = ny(m, l10) + ny(m, 0);
        m.set_f64(px, (-y).to_f64());
    }
    if short(m) {
        return 0;
    }
    normalise(m);
    1
}

/// The body both clippers share: `tBezierClip` (`flag` 0) and `sBezierClip` (`flag` 1) differ
/// only in the `flag` they hand to buildLine / calcDistFromLine and in which of the patch's
/// two counts (`nv` for t, `nu` for s) is calcDistFromLine's `n`. Returns 2 when the working
/// arrays cannot be had, 0 when no clip line exists, 4 when the points lie all on one side
/// (no zero point and no point on both sides), else 1 with `[*lo, *hi]` narrowed.
fn bezier_clip(m: &mut Mem, patch: u32, lo: u32, hi: u32, flag: i16) -> u32 {
    let sp = m.sp();
    // the original's locals: three 25-point stack buffers, the counts and the output cells
    let (sa, sb, sc) = (m.local(400), m.local(400), m.local(400));
    let cells = m.local(0x34);
    let (pa, pb, pc, tot, nu, nv, pts) = (cells, cells + 4, cells + 8, cells + 12, cells + 16, cells + 20, cells + 24);
    let (npos, nneg, nzero, py, px) = (cells + 28, cells + 32, cells + 36, cells + 40, cells + 48);
    for c in [npos, nneg, nzero] {
        m.set_u32(c, 0);
    }
    let r;
    if init_b_clip(m, pa, pb, pc, sa, sb, sc, tot, lo, hi, nu, nv, pts, patch) as u16 == 0 {
        r = 2;
    } else {
        let (a, b, c, p) = (m.u32(pa), m.u32(pb), m.u32(pc), m.u32(pts));
        if build_line(m, p, py, px, m.i32(nu), m.i32(nv), flag) as u16 == 0 {
            cleanup_b_clip(m, a, sa, b, sb, c, sc);
            r = 0;
        } else {
            let n = if flag == 0 { m.i32(nv) } else { m.i32(nu) };
            let (ca, cb) = (m.f64(py), m.f64(px));
            calc_dist_from_line(m, a, npos, b, nneg, c, nzero, n, p, m.i32(tot), ca, cb, flag);
            if m.u32(nzero) == 0 && (m.u32(npos) == 0 || m.u32(nneg) == 0) {
                cleanup_b_clip(m, a, sa, b, sb, c, sc);
                r = 4;
            } else {
                update_min_max_from_clip(m, a, m.i32(npos), b, m.i32(nneg), c, m.i32(nzero), lo, hi);
                cleanup_b_clip(m, a, sa, b, sb, c, sc);
                r = 1;
            }
        }
    }
    m.set_sp(sp);
    r
}

/// [soft 0x08513210 tBezierClip]: `(patch, lo, hi)`: clips the t interval `[*lo, *hi]` against
/// the patch (see [`bezier_clip`]); 2 out of memory, 0 no line, 4 nothing to clip, 1 clipped.
pub fn t_bezier_clip(m: &mut Mem, patch: u32, lo: u32, hi: u32) -> u32 {
    bezier_clip(m, patch, lo, hi, 0)
}

/// [soft 0x08512bc0 sBezierClip]: `(patch, lo, hi)`: the same for the s interval.
pub fn s_bezier_clip(m: &mut Mem, patch: u32, lo: u32, hi: u32) -> u32 {
    bezier_clip(m, patch, lo, hi, 1)
}

/// `a + (b - a) * t` on the register, rounded at the store (the Bezier split's one formula).
fn lerp(a: f64, b: f64, t: F80) -> f64 {
    (x(a) + (x(b) - x(a)) * t).to_f64()
}

/// de Casteljau's `(x, y)` blend of the points `a` and `b` into `dst` (`t` a double).
fn lerp_pt(m: &mut Mem, dst: u32, a: u32, b: u32, t: F80) {
    let vx = lerp(m.f64(a), m.f64(b), t);
    m.set_f64(dst, vx);
    let vy = lerp(m.f64(a + 8), m.f64(b + 8), t);
    m.set_f64(dst + 8, vy);
}

/// Splits a Bezier net at `t` along the rows (`st` = the index step between neighbours along the
/// split direction, `n` = the points along it, `outer` lines of them starting `ostep` apart):
/// `splitBezT2D` and `splitBezS2D` are this with (`st` = nu, `n` = nv, `outer` = nu, `ostep`
/// 1) and (1, nu, nv, nu). Points are 0x24 bytes; only x and y are blended, the other 28 bytes
/// of a copied point are copied whole. Both halves' 2-D boxes are recomputed.
fn split_bez(m: &mut Mem, src: u32, t: f64, h1: u32, h2: u32, n: i32, outer: i32, ostep: i32, st: i32) -> u32 {
    let tt = x(t);
    let cnt = n.wrapping_sub(3);
    let nb4 = n.wrapping_sub(4);
    let (s, d1, d2) = (m.u32(src + 0x98), m.u32(h1 + 0x98), m.u32(h2 + 0x98));
    let at = |base: u32, i: i32| base.wrapping_add((i as u32).wrapping_mul(0x24));
    let sp = m.sp();
    let tmp = if n < 5 {
        m.local(0x48)
    } else {
        let r = m.si_calloc(cnt as u32, 0x24);
        if r == 0 {
            return 0;
        }
        r
    };
    let mut o = 0i32;
    while o < outer {
        let base = o.wrapping_mul(ostep);
        let ix = |k: i32| base.wrapping_add(k.wrapping_mul(st));
        let last = ix(n.wrapping_sub(1));
        m.copy(at(d1, base), at(s, base), 0x24);
        m.copy(at(d2, last), at(s, last), 0x24);
        let mut j = 0i32;
        while j < cnt {
            let (a, b) = (at(s, ix(j + 1)), at(s, ix(j + 2)));
            lerp_pt(m, at(tmp, j), a, b, tt);
            j += 1;
        }
        lerp_pt(m, at(d1, ix(1)), at(s, ix(0)), at(s, ix(1)), tt);
        let nm2 = ix(n.wrapping_sub(2));
        lerp_pt(m, at(d2, nm2), at(s, nm2), at(s, last), tt);
        let mut k = 0i32;
        while k < cnt {
            lerp_pt(m, at(d1, ix(k + 2)), at(d1, ix(k + 1)), tmp, tt);
            lerp_pt(m, at(d2, ix(cnt - k)), at(tmp, nb4 - k), at(d2, ix(cnt - k + 1)), tt);
            let mut q = 0i32;
            while q < nb4 {
                lerp_pt(m, at(tmp, q), at(tmp, q), at(tmp, q + 1), tt);
                q += 1;
            }
            k += 1;
        }
        lerp_pt(m, at(d1, last), at(d1, nm2), at(d2, ix(1)), tt);
        m.copy(at(d2, base), at(d1, last), 0x24);
        o += 1;
    }
    calculate_2d_bbox(m, h1);
    calculate_2d_bbox(m, h2);
    if n > 5 && tmp != 0 {
        m.free(tmp);
    }
    m.set_sp(sp);
    1
}

/// [soft 0x08513ca4 splitBezT2D]: `(patch, t, lo, hi)`: splits the net at `t` along the second
/// index (the `nv` rows) into `lo` and `hi`; 0 when the work array cannot be had. For `nv` of
/// exactly 5 the work array is `SI_calloc`ed and, as in the original, never freed.
pub fn split_bez_t2d(m: &mut Mem, patch: u32, t: f64, lo: u32, hi: u32) -> u32 {
    let (nu, nv) = (m.i32(patch), m.i32(patch + 4));
    split_bez(m, patch, t, lo, hi, nv, nu, 1, nu)
}

/// [soft 0x08513918 splitBezS2D]: the same along the first index (the `nu` columns).
pub fn split_bez_s2d(m: &mut Mem, patch: u32, t: f64, lo: u32, hi: u32) -> u32 {
    let (nu, nv) = (m.i32(patch), m.i32(patch + 4));
    split_bez(m, patch, t, lo, hi, nu, nv, nu, 1)
}

/// [soft 0x08512910 splitPatchAtMinMax]: `(patch, a, b, s_dir, p5, p6, p7)`: splits at `b`
/// into `p5` / `p6`, then splits `p5` again at `a / b` (0 when `b` is zero) into `p7` / `p6`
/// (so `p6` is rewritten). `s_dir` 0: along t, else along s. 1 when both splits worked.
pub fn split_patch_at_min_max(m: &mut Mem, patch: u32, a: f64, b: f64, s_dir: i16, p5: u32, p6: u32, p7: u32) -> u32 {
    let mut q = 0.0f64;
    if !x(0.0).eq(x(b)) {
        q = (x(a) / x(b)).to_f64();
    }
    let f: fn(&mut Mem, u32, f64, u32, u32) -> u32 = if s_dir == 0 { split_bez_t2d } else { split_bez_s2d };
    if f(m, patch, b, p5, p6) == 0 {
        return 0;
    }
    (f(m, p5, q, p7, p6) != 0) as u32
}

/// [soft 0x085129c4 binarySplitPatch]: `(patch, lo, hi, s_dir)`: splits at 0.5.
pub fn binary_split_patch(m: &mut Mem, patch: u32, lo: u32, hi: u32, s_dir: i16) -> u32 {
    let f: fn(&mut Mem, u32, f64, u32, u32) -> u32 = if s_dir == 0 { split_bez_t2d } else { split_bez_s2d };
    (f(m, patch, 0.5, lo, hi) != 0) as u32
}

/// [soft 0x08512470 interOnBorder]: `(patch, p2, p3, p4, p5, p6, s_lo, s_hi, t_lo, t_hi,
/// status)`: when `p6 > p2` or `1 - p3 < p6` the s interval is clipped, else when `p6 > p4`
/// or `1 - p5 < p6` the t interval; otherwise 0 with the status untouched. The status gets the
/// clipper's answer; 0 from the clipper gives status 1 and 0; 1 gives status 1 and 1 when the
/// interval does not meet [0, 1] (start above 1 or end below 0), else 0; any other answer gives
/// status 1 and 1. (`1 - p3` is the 80-bit register value.)
#[allow(clippy::too_many_arguments)]
pub fn inter_on_border(
    m: &mut Mem, patch: u32, p2: f64, p3: f64, p4: f64, p5: f64, p6: f64, s_lo: u32, s_hi: u32, t_lo: u32, t_hi: u32, status: u32,
) -> u32 {
    let v = x(p6);
    let one = x(1.0);
    let (lo, hi, r);
    if v.gt(x(p2)) || (one - x(p3)).lt(v) {
        r = s_bezier_clip(m, patch, s_lo, s_hi);
        lo = s_lo;
        hi = s_hi;
    } else if v.gt(x(p4)) || (one - x(p5)).lt(v) {
        r = t_bezier_clip(m, patch, t_lo, t_hi);
        lo = t_lo;
        hi = t_hi;
    } else {
        return 0;
    }
    m.set_u32(status, r);
    if r == 0 {
        m.set_u32(status, 1);
        return 0;
    }
    if r != 1 || one.lt(x(m.f64(lo))) || x(0.0).gt(x(m.f64(hi))) {
        m.set_u32(status, 1);
        return 1;
    }
    0
}

/// The ray's frame both projectors build: the direction `d` (ray +0x18, three doubles) and the
/// origin (ray +0) give two unit vectors `(a, b, c)` and `(d0, d1, d2)` perpendicular to the
/// direction, each from a cross product that is divided by its length rounded to a float when
/// that exceeds 1e-4. The first is the cross of the helper axis (0, 1, 0), or (1, 0, 0) when
/// the direction is within 1e-6 of vertical in x / z, with the direction; the second the cross of the
/// direction with the first. The first triple is stored (rounded to doubles) before the second is formed.
/// Returns the six doubles.
fn ray_frame(m: &Mem, ray: u32) -> [f64; 6] {
    let (dx, dy, dz) = (x(m.f64(ray + 0x18)), x(m.f64(ray + 0x20)), x(m.f64(ray + 0x28)));
    let up = if x(1e-6).lt((dx * dx + dz * dz).sqrt()) { (0.0, 1.0, 0.0) } else { (1.0, 0.0, 0.0) };
    let (ux, uy, uz) = (x(up.0), x(up.1), x(up.2));
    let t1 = uy * dz - uz * dy;
    let t3 = ux * dy - dx * uy;
    let t2 = uz * dx - dz * ux;
    let (mut a, mut b, mut c) = (t1.to_f64(), t2.to_f64(), t3.to_f64());
    let len = F80::from_f32((t3 * t3 + (t1 * t1 + t2 * t2)).sqrt().to_f32());
    if x(1e-4).lt(len.abs()) {
        a = (t1 / len).to_f64();
        b = (t2 / len).to_f64();
        c = (t3 / len).to_f64();
    }
    let (fa, fb, fc) = (x(a), x(b), x(c));
    let t4 = dy * fc - dz * fb;
    let t6 = fb * dx - dy * fa;
    let t5 = fa * dz - fc * dx;
    let (mut d0, mut d1, mut d2) = (t4.to_f64(), t5.to_f64(), t6.to_f64());
    let len = F80::from_f32((t6 * t6 + (t4 * t4 + t5 * t5)).sqrt().to_f32());
    if x(1e-4).lt(len.abs()) {
        d0 = (t4 / len).to_f64();
        d1 = (t5 / len).to_f64();
        d2 = (t6 / len).to_f64();
    }
    [a, b, c, d0, d1, d2]
}

fn reset_2d_box(m: &mut Mem, patch: u32) {
    const BIG: f64 = 3.4028234663852886e+38;
    m.set_f64(patch + 0x40, BIG);
    m.set_f64(patch + 0x38, BIG);
    m.set_f64(patch + 0x58, -BIG);
    m.set_f64(patch + 0x50, -BIG);
}

/// [soft 0x085119bc projBezPts2D]: `(patch, ray)`: projects the patch's source points (the
/// array at +0x9c, 0x24 bytes each, x y z w) into the plane perpendicular to the ray into the
/// patch's own control points (+0x98): `x' = a x + b y + c z + o w`, `y' = d0 x + d1 y + d2 z
/// + p w` with `o = -(a ox + b oy + c oz)` (each partial sum stored as a double) and `p` the
/// same for `d` kept on the register; z is zeroed, w set to 1, and the patch's 2-D box
/// (+0x38 / +0x40 minima, +0x50 / +0x58 maxima) is rebuilt from the results. 0 when the ray
/// or the source array is null, else 1.
pub fn proj_bez_pts_2d(m: &mut Mem, patch: u32, ray: u32) -> u32 {
    if ray == 0 || m.u32(patch + 0x9c) == 0 {
        return 0;
    }
    let (ox, oy, oz) = (x(m.f64(ray)), x(m.f64(ray + 8)), x(m.f64(ray + 0x10)));
    reset_2d_box(m, patch);
    let [a, b, c, d0, d1, d2] = ray_frame(m, ray);
    let s1 = (x(a) * ox).to_f64();
    let s2 = (x(b) * oy + x(s1)).to_f64();
    let off = (-(x(c) * oz + x(s2))).to_f64();
    let p = -((x(d0) * ox + x(d1) * oy) + x(d2) * oz);
    let n = m.i32(patch).wrapping_mul(m.i32(patch + 4));
    let mut i = 0i32;
    let mut o = 0u32;
    while i < n {
        let q = m.u32(patch + 0x9c).wrapping_add(o);
        let vx = ((x(a) * x(m.f64(q)) + x(b) * x(m.f64(q + 8))) + x(c) * x(m.f64(q + 0x10))) + x(off) * x(m.f64(q + 0x18));
        let dst = m.u32(patch + 0x98).wrapping_add(o);
        m.set_f64(dst, vx.to_f64());
        let q = m.u32(patch + 0x9c).wrapping_add(o);
        let vy = ((x(d0) * x(m.f64(q)) + x(d1) * x(m.f64(q + 8))) + x(d2) * x(m.f64(q + 0x10))) + p * x(m.f64(q + 0x18));
        let dst = m.u32(patch + 0x98).wrapping_add(o);
        m.set_f64(dst + 8, vy.to_f64());
        m.set_u32(dst + 0x10, 0);
        m.set_u32(dst + 0x14, 0);
        m.set_u32(dst + 0x18, 0);
        m.set_u32(dst + 0x1c, 0x3ff0_0000);
        let (px, py) = (x(m.f64(dst)), x(m.f64(dst + 8)));
        if px.lt(x(m.f64(patch + 0x38))) {
            m.set_f64(patch + 0x38, px.to_f64());
        }
        if py.lt(x(m.f64(patch + 0x40))) {
            m.set_f64(patch + 0x40, py.to_f64());
        }
        if px.gt(x(m.f64(patch + 0x50))) {
            m.set_f64(patch + 0x50, px.to_f64());
        }
        if py.gt(x(m.f64(patch + 0x58))) {
            m.set_f64(patch + 0x58, py.to_f64());
        }
        o = o.wrapping_add(0x24);
        i += 1;
    }
    1
}

/// [soft 0x08511308 projBezBboxPts2D]: `(patch, ray)`: the same frame, applied to the eight
/// corners of the patch's 3-D box (minima at +0x68 / +0x70 / +0x78, maxima at +0x80 / +0x88 /
/// +0x90; the corners in the order x, y, z with z varying fastest), each widening the 2-D box
/// through update2DBbox. Here both offsets are rounded to doubles after every partial sum.
pub fn proj_bez_bbox_pts_2d(m: &mut Mem, patch: u32, ray: u32) -> u32 {
    if ray == 0 || m.u32(patch + 0x9c) == 0 {
        return 0;
    }
    let (ox, oy, oz) = (x(m.f64(ray)), x(m.f64(ray + 8)), x(m.f64(ray + 0x10)));
    reset_2d_box(m, patch);
    let [a, b, c, d0, d1, d2] = ray_frame(m, ray);
    let s1 = (x(a) * ox).to_f64();
    let s2 = (x(b) * oy + x(s1)).to_f64();
    let off0 = (-(x(c) * oz + x(s2))).to_f64();
    let t1 = (x(d0) * ox).to_f64();
    let t2 = (x(d1) * oy + x(t1)).to_f64();
    let off1 = (-(x(d2) * oz + x(t2))).to_f64();
    let pt = m.local(16);
    for (cx, cy, cz) in [
        (0x68, 0x70, 0x78),
        (0x68, 0x70, 0x90),
        (0x68, 0x88, 0x78),
        (0x68, 0x88, 0x90),
        (0x80, 0x70, 0x78),
        (0x80, 0x70, 0x90),
        (0x80, 0x88, 0x78),
        (0x80, 0x88, 0x90),
    ] {
        let (px, py, pz) = (x(m.f64(patch + cx)), x(m.f64(patch + cy)), x(m.f64(patch + cz)));
        let vx = (x(c) * pz + (x(b) * py + x(a) * px)) + x(off0);
        m.set_f64(pt, vx.to_f64());
        let vy = (x(d2) * pz + (x(d1) * py + x(d0) * px)) + x(off1);
        m.set_f64(pt + 8, vy.to_f64());
        update_2d_bbox(m, pt, patch + 0x38);
    }
    m.release(16);
    1
}

/// `si_bezPatchInit (0x08503724)`: fills a caller's `BezPatch` record. It lives in `si-spline`,
/// which depends on this crate, so it is reached through [`Outside`]. Args: `(sOrder, tOrder,
/// &bbox, &record)`.
pub const SI_BEZ_PATCH_INIT: Routine = Routine::new("si_bezPatchInit", 0x0850_3724);
/// `si_bezPatchCleanup (0x085037cc)`: frees a caller's record's blocks; same reason. Args: `(&record)`.
pub const SI_BEZ_PATCH_CLEANUP: Routine = Routine::new("si_bezPatchCleanup", 0x0850_37cc);

/// [soft 0x0851258c interSetup]: `(patch, a, b, c, bbox, x0, x1, y0, y1, dx, dy)`: three
/// working patch records `a`, `b`, `c` initialised with the patch's orders and its box (+8)
/// by `si_bezPatchInit` (through `o`; their answers are ignored), then the box's x range
/// (+0 / +0x18) into `*x0` / `*x1`, its y range (+8 / +0x20) into `*y0` / `*y1`, and the
/// widths `*x1 - *x0`, `*y1 - *y0` into `*dx`, `*dy`.
#[allow(clippy::too_many_arguments)]
pub fn inter_setup(
    m: &mut Mem, o: &mut dyn Outside, patch: u32, a: u32, b: u32, c: u32, bbox: u32, x0: u32, x1: u32, y0: u32, y1: u32, dx: u32, dy: u32,
) {
    for rec in [a, b, c] {
        let (s, t) = (m.u32(patch), m.u32(patch + 4));
        o.call(m, SI_BEZ_PATCH_INIT, &[s, t, patch + 8, rec]);
    }
    let v = m.f64(bbox);
    m.set_f64(x0, v);
    let v = m.f64(bbox + 0x18);
    m.set_f64(x1, v);
    let v = m.f64(bbox + 8);
    m.set_f64(y0, v);
    let v = m.f64(bbox + 0x20);
    m.set_f64(y1, v);
    let w = (x(m.f64(x1)) - x(m.f64(x0))).to_f64();
    m.set_f64(dx, w);
    let w = (x(m.f64(y1)) - x(m.f64(y0))).to_f64();
    m.set_f64(dy, w);
}

/// [soft 0x08512554 interCleanup]: `(a, b, c)`: `si_bezPatchCleanup` on each record (through `o`).
pub fn inter_cleanup(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, c: u32) {
    for rec in [a, b, c] {
        o.call(m, SI_BEZ_PATCH_CLEANUP, &[rec]);
    }
}

/// `SI_nurbsSrfValueAndNormal (0x084d6824)`; it is ported (eval.rs) but works on typed records, so the ray tracer
/// reaches it through [`Outside`]. Args (the stack words): `(&point[3] hidden result, srf, u, v, &normal[3], 0, 0)`
/// with the two doubles as two words each.
pub const SI_NURBS_SRF_VALUE_AND_NORMAL: Routine = Routine::new("SI_nurbsSrfValueAndNormal", 0x084d_6824);
/// `si_nurbsInterCreateP (0x0850ca74)` (intersec.c, not ported): `(u, v, point[3] by value, normal[3] by value, 0, 0, ray)`,
/// answers a new `Inter`.
pub const SI_NURBS_INTER_CREATE_P: Routine = Routine::new("si_nurbsInterCreateP", 0x0850_ca74);
/// `si_nurbsInterComputeRayT (0x0850cc48)` (intersec.c, not ported): `(inter)`, its st0 answer is dropped.
pub const SI_NURBS_INTER_COMPUTE_RAY_T: Routine = Routine::new("si_nurbsInterComputeRayT", 0x0850_cc48);
/// `si_nurbsRayAddInter (0x0850dc64)` (ray.c, not ported): `(ray, inter, srf)`, answers nonzero when the hit was kept.
pub const SI_NURBS_RAY_ADD_INTER: Routine = Routine::new("si_nurbsRayAddInter", 0x0850_dc64);

fn push_f64(a: &mut Vec<u32>, v: f64) {
    let b = v.to_bits();
    a.push(b as u32);
    a.push((b >> 32) as u32);
}

/// [soft 0x0851261c addIntersection]: `(p1, p2, x0, x1, y0, y1, patch, ray, srf, count, matrix)`: the hit of a
/// Bezier patch small enough to call flat. Its parameter box centre `u = (x0 + x1) / 2`, `v = (y0 + y1) / 2` (stored to
/// `*p1`, `*p2`) is mapped through the patch's own box (`patch` +8 / +0x10 minima, +0x20 / +0x28 maxima: `lo + (hi - lo)
/// * c`, stored again), the surface is evaluated there (point and normal), both are moved by the ray's 3 x 4 float matrix
/// (`matrix` rows 0, 4, 8 hold the linear part by columns, +0xc / +0x1c / +0x2c the translation; the normal gets the linear
/// part only), an intersection record is made and handed to the ray; `*count` grows when the ray kept it. Answers 5 when
/// `*count` exceeds `(nu - 1) * (nv - 1)` or the normal is nearly perpendicular to the ray (|n . d| < 0.01), else 1.
#[allow(clippy::too_many_arguments)]
pub fn add_intersection(
    m: &mut Mem, o: &mut dyn Outside, p1: u32, p2: u32, x0: f64, x1: f64, y0: f64, y1: f64, patch: u32, ray: u32, srf: u32, count: u32, mat: u32,
) -> u32 {
    let sp = m.sp();
    let u0 = ((x(x0) + x(x1)) * x(0.5)).to_f64();
    let v0 = ((x(y1) + x(y0)) * x(0.5)).to_f64();
    m.set_f64(p1, u0);
    m.set_f64(p2, v0);
    let u = (x(m.f64(patch + 0x20)) - x(m.f64(patch + 8))) * x(m.f64(p1)) + x(m.f64(patch + 8));
    m.set_f64(p1, u.to_f64());
    let v = (x(m.f64(patch + 0x28)) - x(m.f64(patch + 0x10))) * x(m.f64(p2)) + x(m.f64(patch + 0x10));
    m.set_f64(p2, v.to_f64());
    let (pt, nrm) = (m.local(24), m.local(24));
    let mut args = vec![pt, srf];
    push_f64(&mut args, m.f64(p1));
    push_f64(&mut args, m.f64(p2));
    args.extend_from_slice(&[nrm, 0, 0]);
    o.call(m, SI_NURBS_SRF_VALUE_AND_NORMAL, &args);
    let mk: Vec<F80> = (0..12).map(|k| F80::from_f32(m.f32(mat + 4 * k))).collect();
    let f = |k: u32| mk[k as usize];
    let n = [x(m.f64(nrm)), x(m.f64(nrm + 8)), x(m.f64(nrm + 16))];
    let p = [x(m.f64(pt)), x(m.f64(pt + 8)), x(m.f64(pt + 16))];
    // (a0 M[c] + a1 M[4 + c]) + a2 M[8 + c], the point with its translation M[3 + 4 c] added last
    let row = |a: &[F80; 3], c: u32| (a[0] * f(c) + a[1] * f(4 + c)) + a[2] * f(8 + c);
    let nw = [row(&n, 0).to_f64(), row(&n, 1).to_f64(), row(&n, 2).to_f64()];
    let pw = [(row(&p, 0) + f(3)).to_f64(), (row(&p, 1) + f(7)).to_f64(), (row(&p, 2) + f(11)).to_f64()];
    let mut a = Vec::new();
    push_f64(&mut a, m.f64(p1));
    push_f64(&mut a, m.f64(p2));
    for v in pw.iter().chain(nw.iter()) {
        push_f64(&mut a, *v);
    }
    a.extend_from_slice(&[0, 0, ray]);
    let inter = o.call(m, SI_NURBS_INTER_CREATE_P, &a).eax;
    o.call(m, SI_NURBS_INTER_COMPUTE_RAY_T, &[inter]);
    if o.call(m, SI_NURBS_RAY_ADD_INTER, &[ray, inter, srf]).eax != 0 {
        let c = m.i32(count).wrapping_add(1);
        m.set_i32(count, c);
    }
    let dot = ((x(nw[0]) * x(m.f64(ray + 0x18)) + x(nw[1]) * x(m.f64(ray + 0x20))) + x(nw[2]) * x(m.f64(ray + 0x28))).abs();
    let lim = m.i32(patch).wrapping_sub(1).wrapping_mul(m.i32(patch + 4).wrapping_sub(1));
    let r = if m.i32(count) > lim || dot.lt(x(0.01)) { 5 } else { 1 };
    m.set_sp(sp);
    r
}

/// The caller's arguments shared by `intersectBezT2D`, `intersectBezS2D` and `performBsplit`: `(srf, bbox, ray, p6, p7, p8,
/// matrix)`: the surface, the parameter box (`nurbsBbox`, six doubles), the ray, three `long *` (`p6` the hit count handed
/// to addIntersection, `p8` the call counter this module bumps), and the ray's 3 x 4 float matrix.
#[derive(Clone, Copy)]
pub struct RayArgs {
    pub srf: u32,
    pub bbox: u32,
    pub ray: u32,
    pub p6: u32,
    pub p7: u32,
    pub p8: u32,
    pub mat: u32,
}

/// The frame both intersect routines build: three working patch records and the cells interSetup fills.
struct Frame {
    a: u32,
    b: u32,
    c: u32,
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
    dx: u32,
    dy: u32,
    lo: u32, // the clip's [lo, hi] for the direction this routine clips
    hi: u32,
    st: u32,
    up: u32,
    vp: u32,
    olo: u32, // the other direction's interval, which only interOnBorder writes
    ohi: u32,
}

impl Frame {
    fn new(m: &mut Mem) -> Frame {
        let (a, b, c) = (m.local(0x7b0), m.local(0x7b0), m.local(0x7b0));
        let k = m.local(0x70);
        Frame { a, b, c, x0: k, x1: k + 8, y0: k + 0x10, y1: k + 0x18, dx: k + 0x20, dy: k + 0x28, lo: k + 0x30, hi: k + 0x38, st: k + 0x40, up: k + 0x48, vp: k + 0x50, olo: k + 0x58, ohi: k + 0x60 }
    }
    fn g(&self, m: &Mem, cell: u32) -> F80 {
        x(m.f64(cell))
    }
}

/// [soft 0x08513414 intersectBezT2D]: `(patch, t, srf, bbox, ray, p6, p7, p8, matrix)`: one step of the Bezier-clipping
/// intersection of the ray with a patch whose parameter box `bbox` is (x0, x1, y0, y1) = (+0, +0x18, +8, +0x20); `t` the
/// tolerance. Counts the call in `*p8` (intersectBezS2D counts in `*p7`); a box wider than `t` in y and in x is a hit candidate (interOnBorder, then
/// addIntersection); a box at most as wide in y as `t`, or a clip of the t interval, narrows or splits the patch and goes
/// on with intersectBezS2D / performBsplit. Answers 0 failure, 1 done, 2 / 5 from the callee, 4 from the callee. When the
/// patch is already thin in y (`t` at most `dy`) and not in x, intersectBezS2D answers directly and the working records
/// are not given back (the original's leak).
pub fn intersect_bez_t2d(m: &mut Mem, o: &mut dyn Outside, patch: u32, t: f64, ra: RayArgs) -> u32 {
    let sp = m.sp();
    let r = t2d(m, o, patch, t, ra);
    m.set_sp(sp);
    r
}

fn t2d(m: &mut Mem, o: &mut dyn Outside, patch: u32, t: f64, ra: RayArgs) -> u32 {
    let RayArgs { srf, bbox, ray, p6, p7: _, p8, mat } = ra;
    let n = m.i32(p8).wrapping_add(1);
    m.set_i32(p8, n);
    let f = Frame::new(m);
    inter_setup(m, o, patch, f.a, f.b, f.c, bbox, f.x0, f.x1, f.y0, f.y1, f.dx, f.dy);
    let tv = x(t);
    let (tlo, thi) = (f.lo, f.hi);
    let cleanup = |m: &mut Mem, o: &mut dyn Outside| inter_cleanup(m, o, f.a, f.b, f.c);
    if tv.gt(f.g(m, f.dy)) {
        if !tv.gt(f.g(m, f.dx)) {
            return intersect_bez_s2d(m, o, patch, t, ra);
        }
        let (sl, sh) = (f.olo, f.ohi);
        let r = inter_on_border(m, patch, m.f64(f.x0), m.f64(f.x1), m.f64(f.y0), m.f64(f.y1), t, sl, sh, tlo, thi, f.st);
        if r == 0 {
            let st = add_intersection(m, o, f.up, f.vp, m.f64(f.x0), m.f64(f.x1), m.f64(f.y0), m.f64(f.y1), patch, ray, srf, p6, mat);
            m.set_u32(f.st, st);
        }
        cleanup(m, o);
        return m.u32(f.st);
    }
    let r = t_bezier_clip(m, patch, tlo, thi);
    m.set_u32(f.st, r);
    if r == 2 || r == 5 {
        cleanup(m, o);
        return r;
    }
    if r == 4 {
        cleanup(m, o);
        return 1;
    }
    let bsplit = |m: &mut Mem, o: &mut dyn Outside, flag: i16| {
        let (x0, x1, y0, y1) = (m.f64(f.x0), m.f64(f.x1), m.f64(f.y0), m.f64(f.y1));
        perform_bsplit(m, o, patch, f.a, f.b, flag, t, [x0, x1, y0, y1], ra)
    };
    let flag;
    if r == 1 {
        if g1(m, tlo).gt(x(1.0)) || g1(m, thi).lt(x(0.0)) {
            cleanup(m, o);
            return 1;
        }
        let d = g1(m, thi) - g1(m, tlo);
        if d.lt(x(0.8)) {
            let h = f.g(m, f.y1) - f.g(m, f.y0);
            if !h.gt(tv) {
                let status = intersect_bez_s2d(m, o, patch, t, ra);
                cleanup(m, o);
                return status;
            }
            let tk = tv * x(10.0);
            if tk.gt(d) && (f.g(m, f.x1) - f.g(m, f.x0)).gt(tv) {
                perturb_min_max(m, tk.to_f64(), tlo, thi);
            }
            if split_patch_at_min_max(m, patch, m.f64(tlo), m.f64(thi), 0, f.a, f.b, f.c) == 0 {
                cleanup(m, o);
                return 0;
            }
            if trivial_reject(m, f.b, t) != 0 {
                cleanup(m, o);
                return 1;
            }
            let dy = f.g(m, f.dy);
            let ny1 = dy * g1(m, thi) + f.g(m, f.y0);
            m.set_f64(f.y1, ny1.to_f64());
            m.set_f64(bbox + 0x20, ny1.to_f64());
            let ny0 = f.g(m, f.y0) + dy * g1(m, tlo);
            m.set_f64(f.y0, ny0.to_f64());
            m.set_f64(bbox + 8, ny0.to_f64());
            let status = intersect_bez_s2d(m, o, f.b, t, ra);
            cleanup(m, o);
            return status;
        }
        flag = 0;
    } else {
        let h = f.g(m, f.y1) - f.g(m, f.y0);
        flag = if h.gt(tv) && ((f.g(m, f.x1) - f.g(m, f.x0)) * x(10.0)).lt(h) { 0 } else { 1 };
    }
    let status = bsplit(m, o, flag);
    cleanup(m, o);
    status
}

fn g1(m: &Mem, a: u32) -> F80 {
    x(m.f64(a))
}

/// [soft 0x08511d40 intersectBezS2D]: the same step with the roles of s and t exchanged (clips the s interval; the box
/// width in x, not y, decides which side to continue with; `t` at most `dx` goes to the clip, `t` at most `dy` goes
/// to intersectBezT2D, which answers directly without giving the working records back). One quirk reproduced: where a
/// split is chosen on an unclipped patch the original tests `(y0 - y1) * 10 < w` (the y difference with its sign
/// reversed), not `(y1 - y0)`.
pub fn intersect_bez_s2d(m: &mut Mem, o: &mut dyn Outside, patch: u32, t: f64, ra: RayArgs) -> u32 {
    let sp = m.sp();
    let r = s2d(m, o, patch, t, ra);
    m.set_sp(sp);
    r
}

fn s2d(m: &mut Mem, o: &mut dyn Outside, patch: u32, t: f64, ra: RayArgs) -> u32 {
    let RayArgs { srf, bbox, ray, p6, p7, p8: _, mat } = ra;
    let n = m.i32(p7).wrapping_add(1);
    m.set_i32(p7, n);
    let f = Frame::new(m);
    inter_setup(m, o, patch, f.a, f.b, f.c, bbox, f.x0, f.x1, f.y0, f.y1, f.dx, f.dy);
    let tv = x(t);
    let (slo, shi) = (f.lo, f.hi);
    let cleanup = |m: &mut Mem, o: &mut dyn Outside| inter_cleanup(m, o, f.a, f.b, f.c);
    if tv.gt(f.g(m, f.dx)) {
        if !tv.gt(f.g(m, f.dy)) {
            return intersect_bez_t2d(m, o, patch, t, ra);
        }
        let (tl, th) = (f.olo, f.ohi);
        let r = inter_on_border(m, patch, m.f64(f.x0), m.f64(f.x1), m.f64(f.y0), m.f64(f.y1), t, slo, shi, tl, th, f.st);
        if r == 0 {
            let st = add_intersection(m, o, f.up, f.vp, m.f64(f.x0), m.f64(f.x1), m.f64(f.y0), m.f64(f.y1), patch, ray, srf, p6, mat);
            m.set_u32(f.st, st);
        }
        cleanup(m, o);
        return m.u32(f.st);
    }
    let r = s_bezier_clip(m, patch, slo, shi);
    m.set_u32(f.st, r);
    if r == 2 || r == 5 {
        cleanup(m, o);
        return r;
    }
    if r == 4 {
        cleanup(m, o);
        return 1;
    }
    let flag;
    if r == 1 {
        if g1(m, slo).gt(x(1.0)) || g1(m, shi).lt(x(0.0)) {
            cleanup(m, o);
            return 1;
        }
        let d = g1(m, shi) - g1(m, slo);
        if d.lt(x(0.8)) {
            let w = f.g(m, f.x1) - f.g(m, f.x0);
            if !w.gt(tv) {
                let status = intersect_bez_t2d(m, o, patch, t, ra);
                cleanup(m, o);
                return status;
            }
            let tk = tv * x(10.0);
            if tk.gt(d) && (f.g(m, f.y1) - f.g(m, f.y0)).gt(tv) {
                perturb_min_max(m, tk.to_f64(), slo, shi);
            }
            if split_patch_at_min_max(m, patch, m.f64(slo), m.f64(shi), 1, f.a, f.b, f.c) == 0 {
                cleanup(m, o);
                return 0;
            }
            if trivial_reject(m, f.b, t) != 0 {
                cleanup(m, o);
                return 1;
            }
            let dx = f.g(m, f.dx);
            let nx1 = dx * g1(m, shi) + f.g(m, f.x0);
            m.set_f64(f.x1, nx1.to_f64());
            m.set_f64(bbox + 0x18, nx1.to_f64());
            let nx0 = f.g(m, f.x0) + dx * g1(m, slo);
            m.set_f64(f.x0, nx0.to_f64());
            m.set_f64(bbox, nx0.to_f64());
            let status = intersect_bez_t2d(m, o, f.b, t, ra);
            cleanup(m, o);
            return status;
        }
        flag = 1;
    } else {
        let w = f.g(m, f.x1) - f.g(m, f.x0);
        flag = if w.gt(tv) && ((f.g(m, f.y0) - f.g(m, f.y1)) * x(10.0)).lt(w) { 1 } else { 0 };
    }
    let (x0, x1, y0, y1) = (m.f64(f.x0), m.f64(f.x1), m.f64(f.y0), m.f64(f.y1));
    let status = perform_bsplit(m, o, patch, f.a, f.b, flag, t, [x0, x1, y0, y1], ra);
    cleanup(m, o);
    status
}

/// [soft 0x0851224c performBsplit]: `(patch, a, b, flag, srf, t, bbox, x0, x1, y0, y1, ray, p6, p7, p8, matrix)`: splits
/// the patch at 0.5 (`flag` 0 along t, else along s) into `a`, `b` and goes on with each half: the second half `b` first
/// with the middle of the box (y for flag 0, x for flag 1) written into `bbox`, then `a` with the box as it was but for the
/// other side's end set to the middle, the end restored afterwards. A half that trivialReject drops counts as done. Answers
/// 0 when the split fails or a half answers 0, 2 or 5 (the first half's answer), else 1.
#[allow(clippy::too_many_arguments)]
pub fn perform_bsplit(m: &mut Mem, o: &mut dyn Outside, patch: u32, a: u32, b: u32, flag: i16, t: f64, xy: [f64; 4], ra: RayArgs) -> u32 {
    if binary_split_patch(m, patch, a, b, flag) == 0 {
        return 0;
    }
    let [x0, x1, y0, y1] = xy;
    let bbox = ra.bbox;
    let mid;
    let mut r = 1u32;
    let step = |m: &mut Mem, o: &mut dyn Outside, p: u32| if flag != 0 { intersect_bez_t2d(m, o, p, t, ra) } else { intersect_bez_s2d(m, o, p, t, ra) };
    if flag != 0 {
        mid = (x(x1) + x(x0)) * x(0.5);
    } else {
        mid = (x(y1) + x(y0)) * x(0.5);
    }
    let mid = mid.to_f64();
    if trivial_reject(m, b, t) == 0 {
        m.set_f64(if flag != 0 { bbox } else { bbox + 8 }, mid);
        let q = step(m, o, b);
        if q == 0 || q == 2 || q == 5 {
            return q;
        }
        r = 1;
    }
    if trivial_reject(m, a, t) != 0 {
        return r;
    }
    if flag != 0 {
        m.set_f64(bbox + 8, y0);
        m.set_f64(bbox + 0x20, y1);
        m.set_f64(bbox, x0);
        let saved = m.f64(bbox + 0x18);
        m.set_f64(bbox + 0x18, mid);
        r = step(m, o, a);
        if r == 4 {
            r = 1;
        }
        m.set_f64(bbox + 0x18, saved);
    } else {
        m.set_f64(bbox, x0);
        m.set_f64(bbox + 0x18, x1);
        m.set_f64(bbox + 8, y0);
        let saved = m.f64(bbox + 0x20);
        m.set_f64(bbox + 0x20, mid);
        r = step(m, o, a);
        if r == 4 {
            r = 1;
        }
        m.set_f64(bbox + 0x20, saved);
    }
    r
}

// ---- the bounding-box hierarchy over the patches (updateHierBbox, buildBboxHier, intersectBboxHier) ----

/// [soft 0x08515b68 SI_genTreeElemGetData]: the data pointer of a tree element (+0), null for a null element. Tiny, so
/// done here and not through [`Outside`].
fn tree_elem_data(m: &Mem, e: u32) -> u32 {
    if e == 0 {
        0
    } else {
        m.u32(e)
    }
}

/// [soft 0x085145ac updateHierBbox]: `(node)`: the box of a hierarchy node in its data patch (+0x68 / +0x70 / +0x78 the
/// minima, +0x80 / +0x88 / +0x90 the maxima): x and y minima become FLT_MAX and the x, y maxima -FLT_MAX (the z cells are
/// **not** reset, as in the original), then every child (+0x10, next sibling +8) widens it: a smaller minimum, a larger
/// maximum (strict compares) replace the cell.
pub fn update_hier_bbox(m: &mut Mem, node: u32) {
    let s = tree_elem_data(m, node);
    let big = 3.4028234663852886e+38_f64;
    m.set_f64(s + 0x70, big);
    m.set_f64(s + 0x68, big);
    m.set_f64(s + 0x88, -big);
    m.set_f64(s + 0x80, -big);
    let mut c = m.u32(node + 0x10);
    while c != 0 {
        let d = tree_elem_data(m, c);
        for off in [0x68u32, 0x70, 0x78] {
            if x(m.f64(d + off)).lt(x(m.f64(s + off))) {
                let v = m.f64(d + off);
                m.set_f64(s + off, v);
            }
        }
        for off in [0x80u32, 0x88, 0x90] {
            if x(m.f64(d + off)).gt(x(m.f64(s + off))) {
                let v = m.f64(d + off);
                m.set_f64(s + off, v);
            }
        }
        c = m.u32(c + 8);
    }
}

/// `si_bezPatchCreate (0x08503460)` (bezpatch, in si-spline): `(nu, nv, box[12 words])`; ported there on typed records,
/// reached through [`Outside`] here. Allocates the 0x7b0-byte record.
pub const SI_BEZ_PATCH_CREATE: Routine = Routine::new("si_bezPatchCreate", 0x0850_3460);
/// `SI_genTreeDestroy (0x08515848)`: `(&tree, flag)`; only on the allocation-failure path, which no check reaches.
pub const SI_GEN_TREE_DESTROY: Routine = Routine::new("SI_genTreeDestroy", 0x0851_5848);

fn half_up(n: i32) -> i32 {
    x((F80::from_i32(n) * x(0.5)).round_int(si_x87::Round::Up).to_f64()).to_i32(si_x87::Round::Zero)
}

fn levels_for(n: i32) -> f64 {
    let a = si_x87::trig::log10(F80::from_i32(n).to_f64());
    let b = si_x87::trig::log10(2.0);
    let q = x(a.to_f64()) / b;
    q.round_int(si_x87::Round::Up).to_f64()
}

/// [soft 0x08514058 buildBboxHier]: `(items, nu, nv)`: builds the bounding-box hierarchy over `nu * nv` patches (an
/// array of patch pointers, `items[u * nv + v]`): a `SI_genTree` with one element per patch, then, level by level, one
/// element per block of up to 2 x 2 elements of the level below (a 1 x 1 patch record from `si_bezPatchCreate` as its
/// data, its children linked +0x10 / +8 with parent +0xc and previous +4, then updateHierBbox), for
/// `ceil(log10(max side)/log10(2))` levels; the last element made becomes the tree root (tree +0) and the element count
/// goes to tree +0xc. Answers the tree, or 0 when an allocation failed. The 48 bytes handed to `si_bezPatchCreate` as the
/// new patch's box are an uninitialised local of the original, copied as they are.
pub fn build_bbox_hier(m: &mut Mem, o: &mut dyn Outside, items: u32, nu: i32, nv: i32) -> u32 {
    let sp = m.sp();
    let scratch = m.local(0x30);
    let r = build_hier(m, o, items, nu, nv, scratch);
    m.set_sp(sp);
    r
}

fn build_hier(m: &mut Mem, o: &mut dyn Outside, items: u32, nu: i32, nv: i32, scratch: u32) -> u32 {
    let tree = m.si_calloc(1, 0x14);
    if tree == 0 {
        return 0;
    }
    let n = nu.wrapping_mul(nv);
    let arr_a = m.si_calloc(n as u32, 4);
    if arr_a == 0 {
        return 0;
    }
    let arr_b = m.si_calloc(n as u32, 4);
    if arr_b == 0 {
        m.free(arr_a);
        return 0;
    }
    let (mut cur, mut nxt) = (arr_a, arr_b);
    let mut flip = false;
    let lu = levels_for(nu);
    let lv = levels_for(nv);
    let levels = if x(lv).lt(x(lu)) { x(lu).to_i32(si_x87::Round::Zero) } else { x(lv).to_i32(si_x87::Round::Zero) };
    let mut count: i32 = 0;
    let mut root = 0u32;
    for i in 0..n.max(0) as u32 {
        let e = m.si_calloc(1, 0x18);
        if e == 0 {
            return 0;
        }
        let d = m.u32(items + 4 * i);
        m.set_u32(e, d);
        m.set_u32(e + 0x14, tree);
        m.set_u32(arr_a + 4 * i, e);
        count = count.wrapping_add(1);
        root = e;
    }
    let (mut cnu, mut cnv) = (nu, nv);
    let mut hnv = half_up(cnv);
    let mut hnu = half_up(cnu);
    let mut prev = 0u32;
    for _ in 0..levels.max(0) {
        let mut outc = 0u32;
        let mut i = 0i32;
        while i < cnu {
            let su = if i < cnu - 1 || cnu & 1 == 0 { 2 } else { 1 };
            let mut j = 0i32;
            while j < cnv {
                let sv = if j < cnv - 1 || cnv & 1 == 0 { 2 } else { 1 };
                let e = m.si_calloc(1, 0x18);
                if e == 0 {
                    m.free(arr_a);
                    m.free(arr_b);
                    let cell = m.local(4);
                    m.set_u32(cell, tree);
                    o.call(m, SI_GEN_TREE_DESTROY, &[cell, 0]);
                    return 0;
                }
                let patch = o.call(m, SI_BEZ_PATCH_CREATE, &[1, 1, scratch]).eax;
                m.set_u32(e, patch);
                m.set_u32(e + 0x14, tree);
                m.set_u32(nxt + 4 * outc, e);
                outc += 1;
                count = count.wrapping_add(1);
                for k in 0..su {
                    let mut idx = (j + (k + i) * cnv) as u32;
                    for _ in 0..sv {
                        let c = m.u32(cur + 4 * idx);
                        if m.u32(e + 0x10) == 0 {
                            m.set_u32(e + 0x10, c);
                            m.set_u32(c + 0xc, e);
                            prev = m.u32(e + 0x10);
                        } else {
                            m.set_u32(prev + 8, c);
                            m.set_u32(c + 4, prev);
                            m.set_u32(c + 0xc, e);
                            prev = c;
                        }
                        idx += 1;
                    }
                }
                update_hier_bbox(m, e);
                root = e;
                j += sv;
            }
            i += su;
        }
        cnv = hnv;
        cnu = hnu;
        hnv = half_up(cnv);
        hnu = half_up(cnu);
        if flip {
            cur = arr_a;
            nxt = arr_b;
        } else {
            cur = arr_b;
            nxt = arr_a;
        }
        flip = !flip;
    }
    m.set_u32(tree, root);
    m.set_u32(tree + 0xc, count as u32);
    m.free(arr_a);
    m.free(arr_b);
    tree
}

/// [soft 0x08514848 intersectBboxHier]: `(node, t, srf, ray, matrix)`: walks the hierarchy from `node` over its siblings
/// (+8). A node with its patch's projected box rejected (projBezBboxPts2D then trivialReject) goes straight to the next
/// sibling; a leaf (no child at +0x10) projects the control points and, unless rejected again, hands the patch to
/// intersectBezS2D with a fresh unit parameter box (0, 0, 0, 1, 1, 1) and three zeroed counters: an answer other than 1
/// stops the walk with that answer; an inner node walks its children first and stops on an answer other than 1.
/// Answers 1 at the end of a sibling chain (or for a null node).
pub fn intersect_bbox_hier(m: &mut Mem, o: &mut dyn Outside, node: u32, t: f64, srf: u32, ray: u32, mat: u32) -> u32 {
    let sp = m.sp();
    let r = bbox_hier(m, o, node, t, srf, ray, mat);
    m.set_sp(sp);
    r
}

fn bbox_hier(m: &mut Mem, o: &mut dyn Outside, mut node: u32, t: f64, srf: u32, ray: u32, mat: u32) -> u32 {
    let bbox = m.local(0x30);
    let cells = m.local(12);
    while node != 0 {
        // every call of the original starts with its own box and counters
        for (k, v) in [0.0f64, 0.0, 0.0, 1.0, 1.0, 1.0].iter().enumerate() {
            m.set_f64(bbox + 8 * k as u32, *v);
        }
        for k in 0..3 {
            m.set_i32(cells + 4 * k, 0);
        }
        let data = m.u32(node);
        proj_bez_bbox_pts_2d(m, data, ray);
        if trivial_reject(m, data, t) as u16 == 0 {
            let child = m.u32(node + 0x10);
            if child == 0 {
                proj_bez_pts_2d(m, data, ray);
                if trivial_reject(m, data, t) as u16 == 0 {
                    let ra = RayArgs { srf, bbox, ray, p6: cells, p7: cells + 4, p8: cells + 8, mat };
                    let r = intersect_bez_s2d(m, o, data, t, ra);
                    if r != 1 {
                        return r;
                    }
                }
            } else {
                let r = bbox_hier(m, o, child, t, srf, ray, mat);
                if r != 1 {
                    return r;
                }
            }
        }
        node = m.u32(node + 8);
    }
    1
}

// ---- convertToBez ----

/// `SI_nurbsSrfCopy (0x084dc1a8)`: `(srf)`, answers the new surface. Ported on typed records (and, in `image`, on the
/// image) but reached through [`Outside`] here, as the callee of a conversion the check holds against stand-ins.
pub const SI_NURBS_SRF_COPY: Routine = Routine::new("SI_nurbsSrfCopy", 0x084d_c1a8);
/// `SI_nurbsSrfApplyMatrix (0x084e9644)`: `(srf, matrix, mask)`.
pub const SI_NURBS_SRF_APPLY_MATRIX: Routine = Routine::new("SI_nurbsSrfApplyMatrix", 0x084e_9644);
/// `SI_nurbsSrfKnotInsert (0x084d96bc)`: `(srf, t, r, dir)`, `t` two words; the 16-bit answer is ignored.
pub const SI_NURBS_SRF_KNOT_INSERT: Routine = Routine::new("SI_nurbsSrfKnotInsert", 0x084d_96bc);
/// `SI_nurbsSrfDispose (0x084dba5c)`: `(&srf)`.
pub const SI_NURBS_SRF_DISPOSE: Routine = Routine::new("SI_nurbsSrfDispose", 0x084d_ba5c);
/// `SI_bezPatchDestroy (0x08503504)`: `(patch)`; only on the allocation-failure path.
pub const SI_BEZ_PATCH_DESTROY: Routine = Routine::new("SI_bezPatchDestroy", 0x0850_3504);

/// [soft 0x085149ec convertToBez]: `(srf, *nv_out, *nu_out, matrix)`: cuts a NURBS surface into Bezier patches. A copy of
/// the surface (SI_nurbsSrfCopy) is moved by `matrix` (SI_nurbsSrfApplyMatrix, mask 0), then every knot span longer than 1e-6
/// in U, then in V, has its lower knot inserted again `order - 1` times (SI_nurbsSrfKnotInsert, dir 0 / 1) so that every
/// span is a Bezier piece. Then, per V span (outer) and U span (inner), a patch (`calloc(1, 0x7b0)`, then si_bezPatchInit
/// with the orders and the box u0, v0, u1, v1 of the span; the z cells of that box are two uninitialised doubles of the
/// original's frame, copied as they are) gets the `order_u x order_v` control points from the copy's grid (4-D points of
/// 0x24 bytes; the start row / column is `span index - order + 2`, wrapped by the grid's size) and its 3-D box (+0x68 /
/// +0x80 ranges from update3DBbox over the points); the array of patches is `[v span * (u spans) + u span]`. Gives back the
/// copy (SI_nurbsSrfDispose) and writes the V span count to `*nv_out`, the U span count to `*nu_out`. Answers the patch
/// array, or 0 when an allocation failed (the patches made so far are destroyed first, which no check reaches).
pub fn convert_to_bez(m: &mut Mem, o: &mut dyn Outside, srf: u32, nv_out: u32, nu_out: u32, mat: u32) -> u32 {
    let sp = m.sp();
    let cell = m.local(4);
    let scratch = m.local(0x30);
    let r = conv_bez(m, o, srf, nv_out, nu_out, mat, cell, scratch);
    m.set_sp(sp);
    r
}

#[allow(clippy::too_many_arguments)]
fn conv_bez(m: &mut Mem, o: &mut dyn Outside, srf: u32, nv_out: u32, nu_out: u32, mat: u32, cell: u32, scratch: u32) -> u32 {
    let eps = x(1e-6);
    let big = 3.4028234663852886e+38_f64;
    let copy = o.call(m, SI_NURBS_SRF_COPY, &[srf]).eax;
    m.set_u32(cell, copy);
    o.call(m, SI_NURBS_SRF_APPLY_MATRIX, &[copy, mat, 0]);
    for (dir, base) in [(0u32, 0u32), (1, 0x24)] {
        let n = m.i32(srf + base + 8);
        let knots = m.u32(srf + base + 0xc);
        let order = m.i32(srf + base);
        let mut i = 0i32;
        while i < n.wrapping_sub(1) {
            let a = knots.wrapping_add((i as u32).wrapping_mul(8));
            let (k0, k1) = (x(m.f64(a)), x(m.f64(a + 8)));
            if (k1 - k0).gt(eps) {
                let bits = k0.to_f64().to_bits();
                o.call(m, SI_NURBS_SRF_KNOT_INSERT, &[copy, bits as u32, (bits >> 32) as u32, order.wrapping_sub(1) as u32, dir]);
            }
            i += 1;
        }
    }
    let (nku, nkv) = (m.i32(copy + 8), m.i32(copy + 0x2c));
    let (nu_pts, nv_pts) = (m.i32(copy + 4), m.i32(copy + 0x28));
    let (ku, kv) = (m.u32(copy + 0xc), m.u32(copy + 0x30));
    let (ord_u, ord_v) = (m.i32(copy), m.i32(copy + 0x24));
    let cu = nurbs_count_intervals(m, copy);
    let cv = nurbs_count_intervals(m, copy + 0x24);
    let arr = m.si_calloc(cu.wrapping_mul(cv) as u32, 4);
    if arr == 0 {
        o.call(m, SI_NURBS_SRF_DISPOSE, &[cell]);
        return 0;
    }
    let mut base = 0i32;
    let mut j = 0i32;
    while j < nkv.wrapping_sub(1) {
        let a = kv.wrapping_add((j as u32).wrapping_mul(8));
        let (k0, k1) = (x(m.f64(a)), x(m.f64(a + 8)));
        if (k1 - k0).gt(eps) {
            let (v0, v1) = (k0.to_f64(), m.f64(a + 8));
            let mut made = 0i32;
            let mut i = 0i32;
            while i < nku.wrapping_sub(1) {
                let b = ku.wrapping_add((i as u32).wrapping_mul(8));
                let (l0, l1) = (x(m.f64(b)), x(m.f64(b + 8)));
                if (l1 - l0).gt(eps) {
                    let (u0, u1) = (l0.to_f64(), m.f64(b + 8));
                    let patch = m.si_calloc(1, 0x7b0);
                    if patch == 0 {
                        let done = base.wrapping_add(made);
                        for q in 0..done.max(0) as u32 {
                            let p = m.u32(arr + 4 * q);
                            o.call(m, SI_BEZ_PATCH_DESTROY, &[p]);
                        }
                        m.free(arr);
                        o.call(m, SI_NURBS_SRF_DISPOSE, &[cell]);
                        return 0;
                    }
                    m.set_f64(scratch, u0);
                    m.set_f64(scratch + 8, v0);
                    m.set_f64(scratch + 0x18, u1);
                    m.set_f64(scratch + 0x20, v1);
                    o.call(m, SI_BEZ_PATCH_INIT, &[ord_u as u32, ord_v as u32, scratch, patch]);
                    let src_pts = m.u32(copy + 0x54);
                    let dst_pts = m.u32(patch + 0x9c);
                    let mut vr = j.wrapping_sub(ord_v).wrapping_add(2);
                    if vr < 0 {
                        vr = vr.wrapping_add(nv_pts);
                    }
                    let u_start = {
                        let t = i.wrapping_sub(ord_u).wrapping_add(2);
                        if t < 0 {
                            t.wrapping_add(nu_pts)
                        } else {
                            t
                        }
                    };
                    let mut row = 0i32;
                    while row < ord_v {
                        if vr >= nv_pts {
                            vr = vr.wrapping_sub(nv_pts);
                        }
                        let rowbase = vr.wrapping_mul(nu_pts);
                        let mut u = u_start;
                        let mut dst = dst_pts.wrapping_add((row.wrapping_mul(ord_u) as u32).wrapping_mul(0x24));
                        let mut col = 0i32;
                        while col < ord_u {
                            if u >= nu_pts {
                                u = u.wrapping_sub(nu_pts);
                            }
                            let src = src_pts.wrapping_add((rowbase.wrapping_add(u) as u32).wrapping_mul(0x24));
                            for w in 0..9u32 {
                                let v = m.u32(src + 4 * w);
                                m.set_u32(dst + 4 * w, v);
                            }
                            dst = dst.wrapping_add(0x24);
                            u = u.wrapping_add(1);
                            col += 1;
                        }
                        vr = vr.wrapping_add(1);
                        row += 1;
                    }
                    for off in [0x68u32, 0x70, 0x78] {
                        m.set_f64(patch + off, big);
                    }
                    for off in [0x80u32, 0x88, 0x90] {
                        m.set_f64(patch + off, -big);
                    }
                    let npts = ord_u.wrapping_mul(ord_v);
                    let mut q = 0u32;
                    while (q as i32) < npts {
                        let p = m.u32(patch + 0x9c).wrapping_add(q.wrapping_mul(0x24));
                        update_3d_bbox(m, p, patch + 0x68);
                        q += 1;
                    }
                    m.set_u32(arr.wrapping_add((base.wrapping_add(made) as u32).wrapping_mul(4)), patch);
                    made += 1;
                }
                i += 1;
            }
            base = base.wrapping_add(cu);
        }
        j += 1;
    }
    o.call(m, SI_NURBS_SRF_DISPOSE, &[cell]);
    m.set_i32(nv_out, cv);
    m.set_i32(nu_out, cu);
    arr
}

/// [soft 0x08514ec4 si_nurbsTrimConvertToBez]: `(srf, *nv_out, *nu_out, matrix)`: a wrapper that hands its four arguments to
/// convertToBez unchanged and gives back its answer.
pub fn nurbs_trim_convert_to_bez(m: &mut Mem, o: &mut dyn Outside, srf: u32, nv_out: u32, nu_out: u32, mat: u32) -> u32 {
    convert_to_bez(m, o, srf, nv_out, nu_out, mat)
}

/// [soft 0x08511240 si_nurbsRayTraceSrf2]: `(srf, ray, tree_slot, matrix)`: traces a ray against a surface through the
/// bounding-box hierarchy kept in `*tree_slot`. 0 for a null surface or ray. When `*tree_slot` is empty the surface is cut
/// into Bezier patches (convertToBez with the matrix), the hierarchy built over them (buildBboxHier with the V and U span
/// counts), the patch array given back (`free`), and a 0x40-byte block (`calloc(1, 0x40)`, tree +4) takes a copy of the
/// matrix; answers 2 when the tree or that block could not be made. Then the walk (intersectBboxHier from the tree's
/// root, tolerance 1e-5) answers.
pub fn nurbs_ray_trace_srf2(m: &mut Mem, o: &mut dyn Outside, srf: u32, ray: u32, tree_slot: u32, mat: u32) -> u32 {
    if srf == 0 || ray == 0 {
        return 0;
    }
    if m.u32(tree_slot) == 0 {
        let sp = m.sp();
        let cells = m.local(8);
        let patches = convert_to_bez(m, o, srf, cells, cells + 4, mat);
        let (nv, nu) = (m.i32(cells), m.i32(cells + 4));
        let tree = build_bbox_hier(m, o, patches, nv, nu);
        m.set_u32(tree_slot, tree);
        m.set_sp(sp);
        if tree == 0 {
            return 2;
        }
        m.free(patches);
        let blk = m.si_calloc(1, 0x40);
        m.set_u32(tree + 4, blk);
        if blk == 0 {
            return 2;
        }
        si_mem::clib::memcpy(m, blk, mat, 0x40);
    }
    let root = m.u32(m.u32(tree_slot));
    intersect_bbox_hier(m, o, root, 1e-5, srf, ray, mat)
}
