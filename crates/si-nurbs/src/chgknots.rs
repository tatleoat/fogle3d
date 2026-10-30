// SPDX-License-Identifier: MIT
//! The knot-sequence changers of `eval.c` (finding 0025 section 25) on the memory image: a curve or a surface
//! re-expressed on a new knot vector by inserting every needed knot at once (a de Boor triangle per new point).

use crate::image::{free_some, keep, statics, CrvHeld, Halt, SrfHeld};
use crate::knots::interv;
use crate::EPS;
use si_hrc::image::nurbs::{knots_allocate, nurbs_points_allocate};
use si_mem::Mem;
use si_x87::F80;

const PT: u32 = 0x24;

fn copy36(m: &mut Mem, dst: u32, src: u32) {
    for w in 0..9u32 {
        let v = m.u32(src + 4 * w);
        m.set_u32(dst + 4 * w, v);
    }
}

fn tmp_points(m: &mut Mem, left: i32, w: i32) -> Result<u32, Halt> {
    let n = (left.wrapping_add(1)).wrapping_mul(w);
    if left < 0 || n <= 0 || n > 1 << 20 {
        return Err(Halt::Refused("nrb_subdivc"));
    }
    Ok(nurbs_points_allocate(m, n))
}

fn knot(m: &Mem, base: u32, i: i32) -> F80 {
    F80::from_f64(m.f64(base.wrapping_add((i as u32).wrapping_mul(8))))
}

/// [soft 0x084d7dbc nrb_CRVsubdivc]: the point `idx` (1-based) of the curve `out` (its knots at +0xc are the new vector,
/// its points at +0x2c) from the span `left` of the curve `c`: a de Boor triangle in a scratch of
/// `(left + 1) × (order + 1)` points, cell (j, m) at `j × (order + 1) + m`.
pub fn crv_subdivc(m: &mut Mem, c: u32, out: u32, left: i32, idx: i32) -> Result<(), Halt> {
    let k = m.i32(c);
    let w = k.wrapping_add(1);
    let tmp = tmp_points(m, left, w)?;
    let pts = m.u32(c + 0x2c);
    let ck = m.u32(c + 0xc);
    let ok = m.u32(out + 0xc);
    let at = |j: i32, mm: i32| tmp.wrapping_add((j.wrapping_mul(w).wrapping_add(mm) as u32).wrapping_mul(PT));
    for j in left - k..left {
        if j >= 0 {
            copy36(m, at(j, 0), pts.wrapping_add((j as u32).wrapping_mul(PT)));
        }
    }
    let mut mm = 1;
    while mm < k {
        let mut j = left - k + mm;
        while j < left {
            if j.wrapping_mul(w).wrapping_add(mm) >= 0 {
                let u = knot(m, ok, idx.wrapping_add(k).wrapping_sub(mm).wrapping_sub(1));
                let d1 = u - knot(m, ck, j);
                let d2 = knot(m, ck, k.wrapping_sub(mm).wrapping_add(j)) - u;
                let inv = F80::ONE / (d1 + d2);
                let dst = at(j, mm);
                let a = at(j - 1, mm - 1);
                let b = at(j, mm - 1);
                for o in 0..4u32 {
                    let fa = F80::from_f64(m.f64(a + 8 * o));
                    let fb = F80::from_f64(m.f64(b + 8 * o));
                    let r = if o == 0 || o == 3 { (d2 * fa + d1 * fb) * inv } else { (d1 * fb + d2 * fa) * inv };
                    m.set_f64(dst + 8 * o, r.to_f64());
                }
            }
            j += 1;
        }
        mm += 1;
    }
    let src = at(left - 1, k - 1);
    let o = m.u32(out + 0x2c).wrapping_add((idx as u32).wrapping_mul(PT)).wrapping_sub(PT);
    copy36(m, o, src);
    free_some(m, tmp);
    Ok(())
}

fn interv_at(m: &mut Mem, held_u: bool, rec: u32, t: f64) -> Result<i32, Halt> {
    let mut stt = statics(m);
    let r = if held_u {
        let s = SrfHeld::at(m, rec);
        let v = s.view();
        interv(&mut stt, v.u(), t, 0)
    } else {
        let c = CrvHeld::at(m, rec);
        let v = c.view();
        interv(&mut stt, v.dir(), t, 0)
    };
    keep(m, stt);
    r.ok_or(Halt::Refused("si_nurbsInterv"))
}

/// [soft 0x084d7c90 si_nurbsCrvChangeKnotsSeq]: the curve at `c` on the `n` knots at `knots` (the open form gets
/// `n + 2 − order` points, the closed form `n − 1`); the new points one by one by [`crv_subdivc`], then the curve's knot
/// and point arrays replaced (the points last), and +0x24 set to 2. Always 1.
pub fn crv_change_knots_seq(m: &mut Mem, c: u32, n: i32, knots: u32) -> Result<i32, Halt> {
    let sp0 = m.sp();
    let rec = m.local(0x68);
    let order = m.i32(c);
    let closed = m.i16(c + 0x20) != 0;
    let np = if !closed { n.wrapping_add(2).wrapping_sub(order) } else { n.wrapping_sub(1) };
    m.set_i32(rec, order);
    m.set_i32(rec + 4, np);
    m.set_i32(rec + 8, n);
    m.set_u32(rec + 0xc, knots);
    for o in [0x10u32, 0x14, 0x18, 0x1c] {
        let v = m.u32(c + o);
        m.set_u32(rec + o, v);
    }
    let v = m.u16(c + 0x20);
    m.set_u16(rec + 0x20, v);
    let newpts = nurbs_points_allocate(m, np);
    m.set_u32(rec + 0x2c, newpts);
    let mut i = 0;
    while i < np {
        let t = m.f64(knots.wrapping_add(8 * i as u32));
        let left = interv_at(m, false, c, t)?;
        i += 1;
        crv_subdivc(m, c, rec, left, i)?;
    }
    let old = m.u32(c + 0xc);
    if old != 0 {
        m.free(old);
        m.set_u32(c + 0xc, 0);
    }
    for w in 0..9u32 {
        let v = m.u32(rec + 4 * w);
        m.set_u32(c + 4 * w, v);
    }
    let ka = knots_allocate(m, n);
    m.set_u32(c + 0xc, ka);
    let mut i = 0;
    while i < m.i32(rec + 8) {
        let v = m.f64(knots.wrapping_add(8 * i as u32));
        let kn = m.u32(c + 0xc);
        m.set_f64(kn.wrapping_add(8 * i as u32), v);
        i += 1;
    }
    let op = m.u32(c + 0x2c);
    if op != 0 {
        m.free(op);
        m.set_u32(c + 0x2c, 0);
    }
    m.set_u32(c + 0x2c, newpts);
    m.set_u32(c + 0x24, 2);
    m.set_sp(sp0);
    Ok(1)
}

/// [soft 0x084d79cc nrb_subdivc]: the surface version of [`crv_subdivc`] for every row of the second direction: the
/// grid cell (row `r`, column `idx − 1`) of `out` from the span `left` of the first direction of `s`.
pub fn srf_subdivc(m: &mut Mem, s: u32, out: u32, left: i32, idx: i32) -> Result<(), Halt> {
    let k = m.i32(s);
    let w = k.wrapping_add(1);
    let tmp = tmp_points(m, left, w)?;
    let at = |j: i32, mm: i32| tmp.wrapping_add((j.wrapping_mul(w).wrapping_add(mm) as u32).wrapping_mul(PT));
    let sk = m.u32(s + 0xc);
    let ok = m.u32(out + 0xc);
    let eps = F80::from_f64(EPS);
    let mut r = 0;
    while r < m.i32(s + 0x28) {
        let nu = m.i32(s + 4);
        for j in left - k..left {
            if j >= 0 {
                let src = m.u32(s + 0x54).wrapping_add((r.wrapping_mul(nu).wrapping_add(j) as u32).wrapping_mul(PT));
                copy36(m, at(j, 0), src);
            }
        }
        let mut mm = 1;
        while mm < k {
            let mut j = left - k + mm;
            while j < left {
                if j.wrapping_mul(w).wrapping_add(mm) >= 0 {
                    let k0 = knot(m, sk, j);
                    let u = knot(m, ok, idx.wrapping_add(k).wrapping_sub(mm).wrapping_sub(1));
                    let t = (u - k0) * (F80::ONE / (knot(m, sk, k.wrapping_add(j).wrapping_sub(mm)) - k0));
                    let dst = at(j, mm);
                    let a = at(j - 1, mm - 1);
                    let b = at(j, mm - 1);
                    if (F80::ONE - t).lt(eps) {
                        copy36(m, dst, b);
                    } else if t.lt(eps) {
                        copy36(m, dst, a);
                    } else {
                        for o in 0..4u32 {
                            let fa = F80::from_f64(m.f64(a + 8 * o));
                            let fb = F80::from_f64(m.f64(b + 8 * o));
                            m.set_f64(dst + 8 * o, (fa + (fb - fa) * t).to_f64());
                        }
                    }
                }
                j += 1;
            }
            mm += 1;
        }
        let src = at(left - 1, k - 1);
        let nv = m.i32(out + 0x28);
        let o = m.u32(out + 0x54).wrapping_add(((r.wrapping_add((idx - 1).wrapping_mul(nv))) as u32).wrapping_mul(PT));
        copy36(m, o, src);
        r += 1;
    }
    free_some(m, tmp);
    Ok(())
}

/// [soft 0x084d77f4 si_nurbsSrfChangeKnotsSeq]: the surface at `s` on `n` new knots in its first direction (that direction then
/// has `n − order` points); the grid is rebuilt one column at a time by [`srf_subdivc`] and the two directions' records
/// trade places (the new one lands in the second). Always 1.
pub fn srf_change_knots_seq(m: &mut Mem, s: u32, n: i32, knots: u32) -> Result<i32, Halt> {
    let sp0 = m.sp();
    let rec = m.local(0x68);
    let order = m.i32(s);
    let np = n.wrapping_sub(order);
    m.set_i32(rec, order);
    m.set_i32(rec + 4, np);
    m.set_i32(rec + 8, n);
    m.set_u32(rec + 0xc, knots);
    for o in [0x10u32, 0x14, 0x18, 0x1c] {
        let v = m.u32(s + o);
        m.set_u32(rec + o, v);
    }
    let v = m.u16(s + 0x20);
    m.set_u16(rec + 0x20, v);
    for w in 0..9u32 {
        let v = m.u32(s + 0x24 + 4 * w);
        m.set_u32(rec + 0x24 + 4 * w, v);
    }
    let total = np.wrapping_mul(m.i32(rec + 0x28));
    let newpts = nurbs_points_allocate(m, total);
    m.set_i32(rec + 0x48, total);
    m.set_u32(rec + 0x54, newpts);
    let mut i = 0;
    while i < np {
        let t = m.f64(knots.wrapping_add(8 * i as u32));
        let left = interv_at(m, true, s, t)?;
        i += 1;
        srf_subdivc(m, s, rec, left, i)?;
    }
    let old = m.u32(s + 0xc);
    if old != 0 {
        m.free(old);
        m.set_u32(s + 0xc, 0);
    }
    for w in 0..9u32 {
        let v = m.u32(rec + 4 * w);
        m.set_u32(s + 4 * w, v);
    }
    let ka = knots_allocate(m, n);
    m.set_u32(s + 0xc, ka);
    let mut i = 0;
    while i < m.i32(rec + 8) {
        let v = m.f64(knots.wrapping_add(8 * i as u32));
        let kn = m.u32(s + 0xc);
        m.set_f64(kn.wrapping_add(8 * i as u32), v);
        i += 1;
    }
    let op = m.u32(s + 0x54);
    if op != 0 {
        m.free(op);
        m.set_u32(s + 0x54, 0);
    }
    m.set_i32(s + 0x48, total);
    m.set_u32(s + 0x54, newpts);
    // the second direction's record goes to the scratch, the (new) first onto its place, the old second to the first
    for w in 0..9u32 {
        let v = m.u32(s + 0x24 + 4 * w);
        m.set_u32(rec + 4 * w, v);
    }
    for w in 0..9u32 {
        let v = m.u32(s + 4 * w);
        m.set_u32(s + 0x24 + 4 * w, v);
    }
    for w in 0..9u32 {
        let v = m.u32(rec + 4 * w);
        m.set_u32(s + 4 * w, v);
    }
    m.set_u32(s + 0x4c, 2);
    m.set_sp(sp0);
    Ok(1)
}

/// [soft 0x084d822c si_OLDnurbsSrfChangeKnotsSeq]: as [`srf_change_knots_seq`] but with the earlier span search
/// (`si_OLDnurbsInterv`, one above the span) and the points taken to homogeneous form first: x, y and z of every old point
/// are multiplied by its weight in place (each product rounded in the x87 register, then stored as a double) and, after the
/// swap of the records, the new points' x, y and z are divided by theirs. Always 1.
/// Since unit route3 this is [`crate::ins2::old_srf_change_knots_seq_img`] (the span search reads the neighbouring heap as the original does).
pub fn old_srf_change_knots_seq(m: &mut Mem, s: u32, n: i32, knots: u32) -> Result<i32, Halt> {
    // routed (unit route3) to the image form, which reads the knot array's neighbours as the original does
    crate::ins2::old_srf_change_knots_seq_img(m, s, n, knots)
}
