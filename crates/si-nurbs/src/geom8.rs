// SPDX-License-Identifier: MIT
//! nurbsgeom8: `nurbsCrvAddKnotNonUniform` and its neighbours on the memory image (finding 0025, section 25).

use crate::image::{at8, Halt};
use si_mem::Mem;
use si_x87::{x, F80};

/// The chord floor of the chord-length knots: the double 0.01 at 0x086619c0.
const FLOOR: f64 = 0.01;

/// Distance between points `i` and `i + 1` of a control polygon (stride 0x24): each of x, y, z divided by w in the register and
/// rounded to a float, the three differences taken in the register, `dz^2 + (dy^2 + dx^2)`, square root; a result below 0.01
/// (a NaN passes) becomes 0.01.
fn floored_chord(m: &Mem, pts: u32, i: i32) -> F80 {
    let p = pts.wrapping_add((i as u32).wrapping_mul(0x24));
    let q = p.wrapping_add(0x24);
    let c = |m: &Mem, a: u32, o: u32| -> F80 { F80::from_f32((x(m.f64(a + o)) / x(m.f64(a + 0x18))).to_f32()) };
    let (dx, dy, dz) = (c(m, p, 0) - c(m, q, 0), c(m, p, 8) - c(m, q, 8), c(m, p, 0x10) - c(m, q, 0x10));
    let d = (dz * dz + (dy * dy + dx * dx)).sqrt();
    if d.lt(x(FLOOR)) { x(FLOOR) } else { d }
}

/// [soft 0x084ee4a8 nurbsCrvAddKnotNonUniform]: `(curve, idx, t)` adds one knot to the curve's knot vector (count at +8 grows
/// by one) for a curve of order above 3 whose type word (+0x24) is 2 and whose point count exceeds the order; otherwise (and
/// when the count is below the order) the knots are freed and zeroed (nothing at all if the count is below the order, which
/// returns before touching them). The new vector is a copy of the old one; then for an open curve with `idx` 0 or the last
/// point the end knots are rebuilt from chord lengths (see below) and `t` is not inserted; in every other case
/// `new[span + 1] = t` and the rest shifts up one place, `span` being `si_nurbsInterv(curve, t, open)` when the curve is
/// closed (open 0) or `0 < idx < count - 1` (open 1), else 0.
/// Chord rebuild at `idx == 0`: `new[order - 1]` is the register sum of the first `order - 2` floored chords, `new[j + 2] =
/// chord(j) + new[j + 1]` for j from order - 2 up to count - order, and every knot from `nk' - order + 1` on is `new[nk' - order]`
/// plus the register sum of the floored chords from count - order + 1 to count - 2. At `idx == count - 1`: `new[nk' - order]`
/// takes the chord at count - order (when order - 2 <= count - order) added to the knot before it, and every later knot is
/// that one plus the sum of the `order - 2` chords ending at `idx`.
pub fn crv_add_knot_non_uniform(m: &mut Mem, c: u32, idx: i32, t: f64) -> Result<(), Halt> {
    let order = m.i32(c);
    let n = m.i32(c + 4);
    if n < order {
        return Ok(());
    }
    let drop_knots = |m: &mut Mem| {
        let p = m.u32(c + 0xc);
        if p != 0 {
            m.free(p);
            m.set_u32(c + 0xc, 0);
        }
    };
    if m.i32(c + 0x24) != 2 || order == 2 || order == 3 || n == order {
        drop_knots(m);
        return Ok(());
    }
    let closed = m.i16(c + 0x20) != 0;
    let mut span: i32 = 0;
    if closed || (idx > 0 && idx < n.wrapping_sub(1)) {
        let rec: crate::NurbsDir = m.read(si_types::Ptr32::new(c));
        let kv = crate::fit2::knot_vec(m, &rec);
        let mut st = crate::Statics { left: m.i32(crate::image::LEFT) };
        let r = crate::knots::interv(&mut st, crate::Dir { rec: &rec, knots: &kv }, t, if closed { 0 } else { 1 });
        m.set_i32(crate::image::LEFT, st.left);
        span = r.ok_or(Halt::Refused("nurbsCrvAddKnotNonUniform"))?;
    }
    let old_n = m.i32(c + 8);
    let nk = old_n.wrapping_add(1);
    m.set_i32(c + 8, nk);
    let old = m.u32(c + 0xc);
    let new = si_hrc::image::nurbs::knots_allocate(m, nk);
    let mut i: i32 = 0;
    while i < old_n {
        let v = m.f64(at8(old, i));
        m.set_f64(at8(new, i), v);
        i += 1;
    }
    let pts = m.u32(c + 0x2c);
    let mut insert = true;
    let get = |m: &Mem, i: i32| x(m.f64(at8(new, i)));
    if !closed && idx == 0 {
        insert = false;
        let a = order.wrapping_sub(2);
        let last = n.wrapping_sub(1);
        let mut sum = F80::ZERO;
        let mut j: i32 = 0;
        while j < a {
            sum = sum + floored_chord(m, pts, j);
            j += 1;
        }
        m.set_f64(at8(new, order.wrapping_sub(1)), sum.to_f64());
        let mut j = a;
        while j < n.wrapping_sub(order).wrapping_add(1) {
            let v = floored_chord(m, pts, j) + get(m, j.wrapping_add(1));
            m.set_f64(at8(new, j.wrapping_add(2)), v.to_f64());
            j += 1;
        }
        let base = nk.wrapping_sub(order);
        let mut sum = F80::ZERO;
        let mut j = n.wrapping_sub(order).wrapping_add(1);
        while j < last {
            sum = sum + floored_chord(m, pts, j);
            j += 1;
        }
        let mut k = base.wrapping_add(1);
        while k < nk {
            let v = get(m, base) + sum;
            m.set_f64(at8(new, k), v.to_f64());
            k += 1;
        }
    } else if !closed && idx == n.wrapping_sub(1) {
        insert = false;
        let base = nk.wrapping_sub(order);
        let first = n.wrapping_sub(order);
        if first >= order.wrapping_sub(2) {
            let v = floored_chord(m, pts, first) + get(m, base.wrapping_sub(1));
            m.set_f64(at8(new, base), v.to_f64());
        }
        let mut sum = F80::ZERO;
        let mut j = idx.wrapping_sub(order).wrapping_add(2);
        let mut cnt = idx.wrapping_sub(j);
        if j < idx {
            while cnt != 0 {
                sum = sum + floored_chord(m, pts, j);
                j += 1;
                cnt -= 1;
            }
        }
        let mut k = base.wrapping_add(1);
        while k < nk {
            let v = sum + get(m, base);
            m.set_f64(at8(new, k), v.to_f64());
            k += 1;
        }
    }
    if insert {
        m.set_f64(at8(new, span.wrapping_add(1)), t);
        let mut j = span.wrapping_add(2);
        while j < nk {
            let v = m.f64(at8(old, j.wrapping_sub(1)));
            m.set_f64(at8(new, j), v);
            j += 1;
        }
    }
    drop_knots(m);
    m.set_u32(c + 0xc, new);
    Ok(())
}

/// [soft 0x084eb244 openNormal]: `(direction, points slot, per)` trims the extra end knots of an open direction `d` (order +0,
/// point count +4, knot count +8, knots +0xc) and the points they carry (`per` points to a count, the net at `*slot`, 0x24 bytes
/// each). Every knot difference is compared with 1e-6 in the register. `a` counts leading near-equal differences (below 1e-6, from
/// knot 1), `s` ends at 1 when the last difference is above it; the front scan stops at the first difference above
/// 1e-6 once the interior ones counted order - 2 (and is skipped when order - 2 <= 0, which also skips the first emptiness
/// test), the back scan trims near-equal differences down from the first big one. If the first scan found fewer than order - 2
/// big differences, or nothing remains between the scans, the knots are freed and zeroed (count 0). Otherwise a scratch block
/// of order + count doubles (`SI_calloc`) gets `order - 1` copies of knot `a`, the knots `a + 1` to `order - (s + 2) + e` (e
/// the back scan's index) and `s` copies of knot e; the table at `SI_gblIndex` gets -1 for the lost trailing points; the point
/// count becomes `n_new - order + 2`, the net is reallocated and copied, the knots replaced from the scratch.
pub fn open_normal(m: &mut Mem, d: u32, slot: u32, per: i32) {
    use crate::image::SI_GBL_INDEX;
    let tol = x(1e-06);
    let order = m.i32(d);
    let old = m.u32(d + 0xc);
    let tmp = m.si_calloc(order.wrapping_add(m.i32(d + 8)) as u32, 8);
    let diff = |m: &Mem, p: u32, j: i32| -> F80 { (x(m.f64(at8(p, j))) - x(m.f64(at8(p, j.wrapping_sub(1))))).abs() };
    let kn = |m: &Mem| m.u32(d + 0xc);
    let empty = |m: &mut Mem| {
        if tmp != 0 {
            m.free(tmp);
        }
        let p = m.u32(d + 0xc);
        if p != 0 {
            m.free(p);
            m.set_u32(d + 0xc, 0);
        }
        m.set_u32(d + 0xc, 0);
        m.set_i32(d + 8, 0);
    };
    let mut cnt: i32 = 0;
    let mut a: i32 = 0;
    let nk2 = m.i32(d + 8).wrapping_sub(2);
    let mut skip_check = false;
    if a < nk2 {
        if a >= order.wrapping_sub(2) {
            skip_check = true;
        } else {
            let mut j: i32 = 1;
            loop {
                if diff(m, kn(m), j).gt(tol) {
                    cnt += 1;
                }
                j += 1;
                a += 1;
                if !(a < nk2 && cnt < order.wrapping_sub(2)) {
                    break;
                }
            }
        }
    }
    if !skip_check && cnt < order.wrapping_sub(2) {
        empty(m);
        return;
    }
    let nk = m.i32(d + 8);
    let mut ebx = nk;
    if a < nk.wrapping_sub(2) {
        let mut j = a.wrapping_add(1);
        loop {
            if !diff(m, kn(m), j).lt(tol) {
                break;
            }
            j += 1;
            a += 1;
            if !(a < nk.wrapping_sub(2)) {
                break;
            }
        }
    }
    let mut s: i32 = 0;
    let mut e = ebx.wrapping_sub(1);
    if e > 0 {
        loop {
            if diff(m, kn(m), e).gt(tol) {
                s += 1;
            }
            e -= 1;
            if !(e > 0 && s <= 0) {
                break;
            }
        }
    }
    let mut s = order.wrapping_sub(2);
    if e > 0 {
        loop {
            if !diff(m, kn(m), e).lt(tol) {
                break;
            }
            s -= 1;
            e -= 1;
            if !(e > 0) {
                break;
            }
        }
    }
    if a >= e {
        empty(m);
        return;
    }
    let mut t: i32 = 0;
    while t < m.i32(d).wrapping_sub(1) {
        let v = m.f64(at8(old, a));
        m.set_f64(at8(tmp, t), v);
        t += 1;
    }
    let mut q = a.wrapping_add(1);
    while q <= m.i32(d).wrapping_sub(s.wrapping_add(2)).wrapping_add(e) {
        let v = m.f64(at8(old, q));
        m.set_f64(at8(tmp, t), v);
        q += 1;
        t += 1;
    }
    let mut r = s;
    while r > 0 {
        let v = m.f64(at8(old, e));
        m.set_f64(at8(tmp, t), v);
        t += 1;
        r -= 1;
    }
    ebx = t;
    let g = m.u32(SI_GBL_INDEX);
    if g != 0 {
        let mut i: i32 = 0;
        loop {
            let n = m.i32(d + 4);
            let lim = n.wrapping_sub(ebx.wrapping_sub(m.i32(d)).wrapping_add(2)).wrapping_mul(per);
            if !(i < lim) {
                break;
            }
            let at = n.wrapping_mul(per).wrapping_sub(1).wrapping_sub(i);
            m.set_u32(g.wrapping_add((at as u32).wrapping_mul(4)), 0xffff_ffff);
            i += 1;
        }
    }
    let newn = ebx.wrapping_sub(m.i32(d)).wrapping_add(2);
    let total = per.wrapping_mul(newn);
    m.set_i32(d + 4, newn);
    let np = si_hrc::image::nurbs::nurbs_points_allocate(m, total);
    let src = m.u32(slot);
    let mut i = 0u32;
    while (i as i32) < total {
        for w in 0..9u32 {
            let v = m.u32(src.wrapping_add(i * 0x24 + 4 * w));
            m.set_u32(np.wrapping_add(i * 0x24 + 4 * w), v);
        }
        i += 1;
    }
    let sp = m.u32(slot);
    if sp != 0 {
        m.free(sp);
        m.set_u32(slot, 0);
    }
    m.set_u32(slot, np);
    let p = m.u32(d + 0xc);
    if p != 0 {
        m.free(p);
        m.set_u32(d + 0xc, 0);
    }
    let nkn = m.si_calloc(ebx as u32, 8);
    m.set_u32(d + 0xc, nkn);
    let mut i: i32 = 0;
    while i < ebx {
        let v = m.f64(at8(tmp, i));
        m.set_f64(at8(nkn, i), v);
        i += 1;
    }
    if tmp != 0 {
        m.free(tmp);
    }
    m.set_i32(d + 8, ebx);
}

/// Round a register value through a stored double and load it back.
fn rd(v: F80) -> F80 {
    x(v.to_f64())
}

/// [soft 0x084cefd0 fitPlaneToSamples]: `(points, n, out1..out8)` fits a plane to `n` control points (x, y, z at +0, +8, +0x10 of
/// 0x24-byte points). The centroid is the register sums stored as doubles and scaled by `1.0 / n` (in the register). The loop
/// accumulates, stored as doubles every step, the second moments xx, yy, zz and xz (the product `dz * dx_rounded` stored first),
/// `xy` with the rounded x difference, and keeps `yz` unrounded in the register; the farthest sample from the centroid (squared
/// distance strictly above the running maximum, which starts at -FLT_MAX) gives the direction `M`. Moments below 1e-6 pick a
/// coordinate direction, else a direction `V` by elimination (the six cases of the listing). Outputs: `out1..4` the unit
/// cross of `V` and `M` with its offset, `out5..8` `M` normalised with its offset. Returns 0 (outputs untouched) when two of
/// the moments are below 1e-6. [With n <= 0, or no sample above -FLT_MAX (NaN data), M is stale stack data in the original;
/// the port uses zeros there.]
pub fn fit_plane_to_samples(m: &mut Mem, pts: u32, n: i32, outs: [u32; 8]) -> u32 {
    let tol = x(1e-06);
    let at = |i: i32, o: u32| pts.wrapping_add((i as u32).wrapping_mul(0x24)).wrapping_add(o);
    let (mut sx, mut sy, mut sz) = (F80::ZERO, F80::ZERO, F80::ZERO);
    let mut i = 0;
    while i < n {
        sx = sx + x(m.f64(at(i, 0)));
        sy = sy + x(m.f64(at(i, 8)));
        sz = sz + x(m.f64(at(i, 0x10)));
        i += 1;
    }
    let (sx, sy, sz) = (rd(sx), rd(sy), rd(sz));
    let inv = x(1.0f64) / F80::from_i32(n);
    let (cx, cy, cz) = (rd(inv * sx), rd(inv * sy), rd(inv * sz));
    let (mut xx, mut xy, mut yy, mut zz, mut xz) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut yz = F80::ZERO;
    let mut best = x(-3.4028234663852886e+38f64);
    let (mut mx, mut my, mut mz) = (x(0.0f64), x(0.0f64), x(0.0f64));
    let mut i = 0;
    while i < n {
        let dx = x(m.f64(at(i, 0))) - cx;
        let dy = x(m.f64(at(i, 8))) - cy;
        let dz = x(m.f64(at(i, 0x10))) - cz;
        let dxr = rd(dx);
        let d2 = dx * dx + dy * dy + dz * dz;
        if d2.gt(best) {
            best = rd(d2);
            mx = dxr;
            my = rd(dy);
            mz = rd(dz);
        }
        xy = (dxr * dy + x(xy)).to_f64();
        xx = (dx * dx + x(xx)).to_f64();
        yy = (dy * dy + x(yy)).to_f64();
        let t = rd(dz * dxr);
        zz = (dz * dz + x(zz)).to_f64();
        xz = (x(xz) + t).to_f64();
        yz = dy * dz + yz;
        i += 1;
    }
    let (xx, xy, yy, zz, xz) = (x(xx), x(xy), x(yy), x(zz), x(xz));
    let small = |v: F80| v.abs().lt(tol);
    let (vx, vy, vz);
    if small(xx) {
        if small(yy) || small(zz) {
            return 0;
        }
        (vx, vy, vz) = (x(1.0f64), x(0.0f64), x(0.0f64));
    } else if small(yy) {
        if small(zz) {
            return 0;
        }
        (vx, vy, vz) = (x(0.0f64), x(1.0f64), x(0.0f64));
    } else if small(zz) {
        (vx, vy, vz) = (x(0.0f64), x(0.0f64), x(1.0f64));
    } else if small(xy) {
        vx = rd(-xy / xx);
        vy = rd(-yz / yy);
        vz = x(1.0f64);
    } else if small(xz) {
        vx = rd(-xy / xx);
        vz = rd(-yz / zz);
        vy = x(1.0f64);
    } else {
        let det = rd(yy * xx);
        let t9 = det - xy * xy;
        let det = rd(t9);
        let v = if t9.abs().lt(tol) {
            (xz * yz - xy * zz) / (yz * xy - yy * xz)
        } else {
            (xy * xz - yz * xx) / det
        };
        vy = rd(v);
        vx = rd(-(xy * vy + xz) / xx);
        vz = x(1.0f64);
    }
    // the unit direction V and the farthest-sample direction M, normalised in the register
    let t10 = x(1.0f64) / (vy * vy + vx * vx + vz * vz).sqrt();
    let t11 = vx * t10;
    let a0 = rd(t11);
    let t12 = vy * t10;
    let t13 = vz * t10;
    let t14 = x(1.0f64) / (my * my + mx * mx + mz * mz).sqrt();
    let t15 = mx * t14;
    let a8 = rd(t15);
    let t16 = my * t14;
    let t17 = mz * t14;
    m.set_f64(outs[4], t15.to_f64());
    m.set_f64(outs[5], t16.to_f64());
    m.set_f64(outs[6], t17.to_f64());
    m.set_f64(outs[7], (-(t17 * cz + (t15 * cx + t16 * cy))).to_f64());
    let c0 = rd(rd(t12 * t17) - t16 * t13);
    let l70 = rd(t13 * a8);
    let t18 = l70 - t17 * a0;
    let l70 = rd(t18);
    let t19 = t16 * a0 - t12 * a8;
    let l78 = rd(t19);
    let u = x(1.0f64) / (t18 * t18 + c0 * c0 + t19 * t19).sqrt();
    let (cxn, cyn, czn) = (rd(c0 * u), rd(l70 * u), rd(l78 * u));
    m.set_f64(outs[0], cxn.to_f64());
    m.set_f64(outs[1], cyn.to_f64());
    m.set_f64(outs[2], czn.to_f64());
    m.set_f64(outs[3], (-((cyn * cy + cxn * cx) + cz * czn)).to_f64());
    1
}

const SEG_AJ: u32 = 0x086d_9c0c;
const SEG_DL: u32 = 0x086d_9c10;
const SEG_DR: u32 = 0x086d_9c14;
const SEG_NB_DLR: u32 = 0x086d_9c18;
const SEG_NB_AJ: u32 = 0x086d_9c1c;

/// [soft 0x084d603c SI_nurbsSrfExtractCurveSeg] with `swap` 0 and a null output curve, on the image with the original's heap
/// traffic: three `SI_calloc` blocks (the 0x64-byte record, `order_v` points of 0x24 bytes and `2 order_v - 2` knots), then its
/// static scratch grown (`dl`, `dr` of `order_u - 1` doubles; `aj` of `order_u * order_v` points), `SI_malloc` the first time and
/// `SI_realloc` after, never freed. The working arrays the extraction at the derivative order `nd` leaves are written into those
/// blocks ([`crate::eval::seg_kernel`]); the curve gets its points, knots and range. Returns the curve and whether the
/// numerical part finished (the curve exists either way).
pub fn extract_seg_heap(m: &mut Mem, held: &crate::image::SrfHeld, u: f64, v: f64, nd: i32) -> (u32, bool) {
    use crate::image::{CrvHeld, LEFT};
    let (ou, ov) = (held.rec.u.order, held.rec.v.order);
    let out = m.si_calloc(1, 0x64);
    m.set_u16(out + 0x20, 0);
    let pts = m.si_calloc(ov as u32, 0x24);
    m.set_u32(out + 0x2c, pts);
    m.set_i32(out, ov);
    m.set_i32(out + 4, ov);
    m.set_i32(out + 8, ov.wrapping_add(ov).wrapping_sub(2));
    let knots = m.si_calloc(m.u32(out + 8), 8);
    m.set_u32(out + 0xc, knots);
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
    let mut h = CrvHeld::at(m, out);
    let mut st = crate::Statics { left: m.i32(LEFT) };
    let r = crate::eval::seg_kernel(&mut st, held.view(), u, v, nd, &mut h.rec, &mut h.knots, &mut h.points);
    m.set_i32(LEFT, st.left);
    // the extraction leaves its working arrays in the static scratch blocks
    let Some((dl, dr, aj)) = r else { return (out, false) };
    for (k, d) in dl.iter().enumerate() {
        m.set_f64(m.u32(SEG_DL) + 8 * k as u32, *d);
    }
    for (k, d) in dr.iter().enumerate() {
        m.set_f64(m.u32(SEG_DR) + 8 * k as u32, *d);
    }
    for (k, p) in aj.iter().enumerate() {
        m.write(si_types::Ptr32::new(m.u32(SEG_AJ) + 36 * k as u32), *p);
    }
    let pts = m.u32(out + 0x2c);
    for (j, p) in h.points.iter().enumerate() {
        m.write(si_types::Ptr32::new(pts + 0x24 * j as u32), *p);
    }
    let kn = m.u32(out + 0xc);
    for (i, k) in h.knots.iter().enumerate() {
        m.set_f64(kn + 8 * i as u32, *k);
    }
    m.set_f64(out + 0x10, h.rec.dir.range_start.0);
    m.set_f64(out + 0x18, h.rec.dir.range_end.0);
    (out, true)
}

/// [soft 0x084db870 SI_nurbsCrvDispose] of the curve at `curve`: knots, points, the record.
pub fn dispose_curve(m: &mut Mem, curve: u32) {
    let slot = m.local(4);
    m.set_u32(slot, curve);
    si_hrc::image::nurbs::nurbs_crv_dispose(m, slot);
    m.release(4);
}

/// [soft 0x084d6728 SI_nurbsSrfValue], on the image with the original's heap traffic: `SI_nurbsSrfExtractCurveSeg(surface, u, v,
/// 0, 0, null)` ([`extract_seg_heap`]), `SI_nurbsCrvValue` evaluates the curve at `v` (its own scratch for an order above 4,
/// [`crate::image::crv_value`]); `SI_nurbsCrvDispose` frees knots, points, the record. The value
/// itself is [`crate::eval::seg_of_view`] and the curve evaluator. `None` when the evaluator refuses.
pub fn srf_value_heap(m: &mut Mem, srf: u32, u: f64, v: f64) -> Option<[f64; 3]> {
    let held = crate::image::SrfHeld::at(m, srf);
    let _margins = held.margins(m);
    let (out, ok) = extract_seg_heap(m, &held, u, v, 0);
    let val = if ok { crate::image::crv_value(m, out, v, 0).ok() } else { None };
    dispose_curve(m, out);
    val
}
