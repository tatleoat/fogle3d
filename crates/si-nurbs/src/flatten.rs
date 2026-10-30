// SPDX-License-Identifier: MIT
//! `SI_nurbsCrvFlatten` (`nrb_draw.c`, finding 0025 section 25) on the memory image: a curve resampled
//! as a curve of the same shape whose knots and points are the display samples (the renderer's NURBS
//! curve and trim-curve flattening). The scratch is a table of `order` homogeneous points that is
//! advanced by forward differences from span to span.

use crate::eval::binom;
use crate::eval::{blend_level, first_point, knot_distances, span_points};
use crate::image::{free_some, keep, statics, CrvHeld, Halt};
use crate::knots::interv;
use crate::NurbsPoint;
use crate::sample::samples_per_knot;
use crate::{Dir, EPS};
use si_hrc::image::nurbs::{nurbs_crv_allocate, nurbs_crv_dispose, nurbs_points_allocate};
use si_mem::Mem;
use si_types::geom::NurbsDir;
use si_types::Ptr32;
use si_x87::F80;

fn rd(m: &Mem, a: u32, n: i32) -> Option<Vec<f64>> {
    if a == 0 || n < 0 || n > 1 << 24 || !m.is_mapped(a, (n as u32).wrapping_mul(8)) {
        return None;
    }
    Some((0..n as u32).map(|i| m.f64(a + 8 * i)).collect())
}

/// [soft 0x084d74f0 SI_samplesPerKnot], on the image, for `dir` (a curve or a surface direction): the array it
/// makes (`SI_calloc(count, 8)`, one entry a span), the span count and the routine's result.
pub fn samples_per_knot_img(m: &mut Mem, dir: u32, per: i32) -> Result<(u32, i32, i32), Halt> {
    let rec: NurbsDir = m.read(Ptr32::new(dir));
    let knots = rd(m, rec.knots.addr(), rec.nb_knots).ok_or(Halt::Refused("SI_samplesPerKnot"))?;
    let d = Dir { rec: &rec, knots: &knots };
    let (v, count, total) = samples_per_knot(d, per).ok_or(Halt::Refused("SI_samplesPerKnot"))?;
    let a = m.si_calloc(count as u32, 8);
    match v {
        Some(v) => {
            for (i, x) in v.iter().enumerate() {
                m.set_f64(a + 8 * i as u32, *x);
            }
        }
        None if count > 0 => return Err(Halt::Refused("SI_samplesPerKnot")),
        None => {}
    }
    Ok((a, count, total))
}

/// [soft 0x084d3ca0 SI_nurbsCrvValue4D], on the image, as [`crate::image::crv_value4d`] but the scratch blocks of an
/// order above 4 (two `SI_calloc(order - 1, 8)` and `SI_nurbsPointsAllocate(order)`) are left holding what the original's
/// evaluator leaves in them (the knot distances and the de Boor table) when they are given back, so that a check that
/// compares the freed blocks sees the same words.
fn crv_value4d_scratch(m: &mut Mem, curve: u32, t: f64) -> Result<NurbsPoint, Halt> {
    let c = CrvHeld::at(m, curve);
    let mut stt = statics(m);
    let cv = c.view();
    let d = cv.dir();
    let order = d.rec.order;
    let r = (|| {
        let left = interv(&mut stt, d, t, (d.rec.closed == 0) as i16)?;
        let deg = order.wrapping_sub(1);
        let (dl, dr) = knot_distances(d, left, t, false)?;
        let first = first_point(left, order, d.rec.nb_points);
        if order < 1 {
            return None;
        }
        let mut p = span_points(cv, first)?;
        for cc in 0..deg {
            blend_level(&mut p, &dl, &dr, deg - cc)?;
        }
        Some((dl, dr, p))
    })();
    keep(m, stt);
    let (dl, dr, p) = r.ok_or(Halt::Refused("SI_nurbsCrvValue4D"))?;
    if order > 4 {
        let a = m.si_calloc(order.wrapping_sub(1) as u32, 8);
        let b = m.si_calloc(order.wrapping_sub(1) as u32, 8);
        let q = nurbs_points_allocate(m, order);
        for (i, v) in dl.iter().enumerate() {
            m.set_f64(a + 8 * i as u32, *v);
        }
        for (i, v) in dr.iter().enumerate() {
            m.set_f64(b + 8 * i as u32, *v);
        }
        for (i, v) in p.iter().enumerate() {
            m.write(Ptr32::new(q + 0x24 * i as u32), *v);
        }
        free_some(m, q);
        free_some(m, a);
        free_some(m, b);
    }
    Ok(p[0])
}

fn st(m: &mut Mem, p: u32, off: u32, v: F80) {
    m.set_f64(p + off, v.to_f64());
}

/// [soft 0x084e62f4 SI_nurbsCrvFlatten]: the curve at `c` (null when its point count is below its order) as a new
/// curve of the same order... (see the finding): for each span with a sample count `s` of `SI_samplesPerKnot`
/// (spans with |s| below 1e-6 are skipped), `s` knots at `k0 + j × (k1 − k0)/s` (the quotient left in the register), the
/// first of each span flagged (+0x22 bit 0, and bit 4 when the span is shorter than 1e-6), and `s` points: the curve's
/// homogeneous points at the `order` parameters `k0 + j × delta` made into a forward-difference table and divided by
/// the weight (`(1/w) × coordinate`, the reciprocal unrounded), the weight 1. The last knot is the range end; an open
/// curve gets a last point from the table. Refusals (a span search that does not end) are `Err`.
pub fn crv_flatten(m: &mut Mem, c: u32) -> Result<u32, Halt> {
    if m.i32(c + 4) < m.i32(c) {
        return Ok(0);
    }
    let closed = m.i16(c + 0x20) != 0;
    let (samples, nspan, total) = samples_per_knot_img(m, c, m.i32(c + 0x34))?;
    let total = if closed { total } else { total.wrapping_add(1) };
    let sp0 = m.sp();
    let slot = m.local(4);
    let cur = nurbs_crv_allocate(m).addr();
    m.set_u32(slot, cur);
    let fail = |m: &mut Mem| {
        nurbs_crv_dispose(m, slot);
        m.set_sp(sp0);
        Ok(0)
    };
    if cur == 0 {
        m.set_sp(sp0);
        return Ok(0);
    }
    let pts = nurbs_points_allocate(m, total);
    m.set_u32(cur + 0x2c, pts);
    if pts == 0 {
        return fail(m);
    }
    let nk = if closed { total.wrapping_add(1) } else { total };
    let nkp = m.si_calloc(nk as u32, 8);
    m.set_u32(cur + 0xc, nkp);
    if nkp == 0 {
        return fail(m);
    }
    m.set_i32(cur + 4, total);
    m.set_i32(cur + 8, nk);
    m.set_u16(cur + 0x20, m.u16(c + 0x20));
    let order = m.i32(c);
    let tmp = m.si_calloc(order as u32, 0x24);
    if tmp == 0 {
        return fail(m);
    }
    let base = if closed { 0 } else { order.wrapping_sub(2) };
    let eps = F80::from_f64(EPS);
    let mut g: i32 = 0; // knots and flags written
    let mut p: i32 = 0; // points written
    let mut s = 0;
    while s < nspan {
        let samp = m.f64(samples + 8 * s as u32);
        let sf = F80::from_f64(samp);
        s += 1;
        if sf.abs().lt(eps) {
            continue;
        }
        let ki = (base + (s - 1)) as u32;
        let kn = m.u32(c + 0xc);
        let k0 = F80::from_f64(m.f64(kn + 8 * ki));
        let k1 = F80::from_f64(m.f64(kn + 8 * ki + 8));
        let width = k1 - k0;
        let delta = width / sf;
        if F80::ZERO.lt(sf) {
            let mut j: i32 = 0;
            loop {
                let ck = m.u32(cur + 0xc);
                m.set_f64(ck + 8 * g as u32, (delta * F80::from_i32(j) + k0).to_f64());
                if j == 0 {
                    let a = m.u32(cur + 0x2c) + 0x22 + 0x24u32.wrapping_mul(g as u32);
                    m.set_u8(a, m.u8(a) | 1);
                    if width.lt(eps) {
                        m.set_u8(a, m.u8(a) | 0x10);
                    }
                }
                j += 1;
                g += 1;
                if !F80::from_i32(j).lt(sf) {
                    break;
                }
            }
        }
        // the table: the curve at `order` parameters
        let mut j = 0;
        while j < order {
            let t = (delta * F80::from_i32(j) + k0).to_f64();
            let v = crv_value4d_scratch(m, c, t)?;
            let at = tmp + 0x24 * j as u32;
            m.write(Ptr32::new(at), v);
            j += 1;
        }
        let mut d = order.wrapping_sub(1);
        while d > 0 {
            let e = d - 1;
            let mut sign: i32 = -1;
            let mut i = e;
            while i >= 0 {
                let b = binom(d, i).ok_or(Halt::Refused("SI_nurbsCrvFlatten"))?;
                let cf = F80::from_i32(if sign < 0 { b.wrapping_neg() } else { b });
                let src = tmp + 0x24 * i as u32;
                let dst = tmp + 0x24 * d as u32;
                let mut v = [0.0f64; 4];
                for (k, vk) in v.iter_mut().enumerate() {
                    *vk = (cf * F80::from_f64(m.f64(src + 8 * k as u32))).to_f64();
                }
                for (k, vk) in v.iter().enumerate() {
                    let o = 8 * k as u32;
                    let sum = F80::from_f64(m.f64(dst + o)) + F80::from_f64(*vk);
                    st(m, dst, o, sum);
                }
                sign = -sign;
                i -= 1;
            }
            d = e;
        }
        if F80::ZERO.lt(F80::from_f64(m.f64(samples + 8 * (s - 1) as u32))) {
            let mut j: i32 = 0;
            loop {
                let w = F80::from_f64(m.f64(tmp + 0x18));
                let pp = m.u32(cur + 0x2c) + 0x24u32.wrapping_mul(p as u32);
                for k in 0..3u32 {
                    let r = (F80::ONE / w) * F80::from_f64(m.f64(tmp + 8 * k));
                    m.set_f64(pp + 8 * k, r.to_f64());
                }
                m.set_f64(pp + 0x18, 1.0);
                p += 1;
                j += 1;
                let mut k = 0;
                while k < m.i32(c) - 1 {
                    let a = tmp + 0x24 * k as u32;
                    for o in 0..4u32 {
                        let sum = F80::from_f64(m.f64(a + 8 * o)) + F80::from_f64(m.f64(a + 0x24 + 8 * o));
                        st(m, a, 8 * o, sum);
                    }
                    k += 1;
                }
                let sm = F80::from_f64(m.f64(samples + 8 * (s - 1) as u32));
                if !F80::from_i32(j).lt(sm) {
                    break;
                }
            }
        }
    }
    let ck = m.u32(cur + 0xc);
    let hi = m.f64(c + 0x18);
    m.set_f64(ck + 8 * g as u32, F80::from_f64(hi).to_f64());
    let w = F80::from_f64(m.f64(tmp + 0x18));
    let r = [(F80::ONE / w) * F80::from_f64(m.f64(tmp)), (F80::ONE / w) * F80::from_f64(m.f64(tmp + 8)), (F80::ONE / w) * F80::from_f64(m.f64(tmp + 0x10))];
    if !closed {
        let pp = m.u32(cur + 0x2c) + 0x24u32.wrapping_mul(p as u32);
        for (k, x) in r.iter().enumerate() {
            m.set_f64(pp + 8 * k as u32, x.to_f64());
        }
        m.set_f64(pp + 0x18, 1.0);
    }
    if samples != 0 {
        free_some(m, samples);
    }
    free_some(m, tmp);
    let ck = m.u32(cur + 0xc);
    m.set_f64(cur + 0x10, F80::from_f64(m.f64(ck)).to_f64());
    let nk = m.i32(cur + 8);
    let v = m.f64(m.u32(cur + 0xc).wrapping_add((nk as u32).wrapping_mul(8)).wrapping_sub(8));
    m.set_f64(cur + 0x18, F80::from_f64(v).to_f64());
    m.set_sp(sp0);
    Ok(cur)
}
