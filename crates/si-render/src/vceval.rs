// SPDX-License-Identifier: MIT
//! `vceval.c`: the metaball field's evaluation (finding 0012, section 28). Doubles are stored in the
//! image; sums and products stay in the 80-bit register unless the original stores them.

use crate::volume::*;
use si_mem::Mem;
use si_x87::{Round, F80};

fn f(m: &Mem, a: u32) -> F80 {
    F80::from_f64(m.f64(a))
}

/// [soft 0x0857b7b0 VC_initEvalContrib]: `VC_initEvalContrib(n)`: the scratch arrays of the evaluation for `n`
/// metaballs (frees the old ones after the first call). 0 or 0x10 when a `malloc` fails.
pub fn init_eval_contrib(m: &mut Mem, n: i32) -> i32 {
    if m.i16(EVAL_FIRST_INIT) == 0 {
        let p = m.u32(LOC_MB_LIST + 8);
        m.free(p);
        m.set_u32(LOC_MB_LIST + 4, 0);
        for g in [EVAL_TMP_IDX, EVAL_TMP_DENS, EVAL_IDX, EVAL_DENS] {
            let p = m.u32(g);
            m.free(p);
        }
    }
    m.set_i16(EVAL_FIRST_INIT, 0);
    m.set_i32(LOC_MB_LIST + 4, n);
    let size = (n as u32).wrapping_mul(4);
    for (g, sz) in [(LOC_MB_LIST + 8, size), (EVAL_TMP_IDX, size), (EVAL_TMP_DENS, (n as u32) << 3), (EVAL_IDX, size), (EVAL_DENS, (n as u32) << 3)] {
        let p = m.malloc(sz);
        m.set_u32(g, p);
        if p == 0 {
            return 0x10;
        }
    }
    0
}

/// [soft 0x0857b8b0 VC_evalComputeSQr2]: `VC_evalComputeSQr2(v, index)`: the squared distance of the three transformed
/// coordinates `v` (made positive in place) for the metaball's exponents, in `st0`.
pub fn eval_compute_sqr2(m: &mut Mem, v: u32, index: i32) -> F80 {
    let k = f(m, mb_at(m, index) + mb::THRESH);
    let zero = F80::ZERO;
    let mut local = [0f64; 3];
    for i in 0..3u32 {
        let mut p = f(m, v + 8 * i);
        if p.lt(zero) {
            p = -p;
        }
        m.set_f64(v + 8 * i, p.to_f64());
        let e = f(m, mb_at(m, index) + mb::EXPON + 8 * i);
        let iv = e.to_i32(Round::Zero);
        let mut frac = e - F80::from_i32(iv);
        let pm = f(m, v + 8 * i); // the stored (rounded) value, read again
        let d4;
        if iv == 0 {
            let mut d3 = p;
            if !F80::from_f64(0.5).gt(frac) {
                d3 = p * k;
                frac = (frac - F80::from_f64(0.5)) + (frac - F80::from_f64(0.5));
            } else if !F80::from_f64(0.25).gt(frac) {
                d3 = k * (p * k).sqrt();
                frac = (frac - F80::from_f64(0.25)) * F80::from_f64(4.0);
            } else {
                d3 = k * (p * k).sqrt();
            }
            d4 = k * d3.sqrt();
            if frac.lt(F80::from_f64(0.25)) {
                local[i as usize] = d4.to_f64();
            } else {
                let d3 = d3 * frac;
                let d5 = d4 * (F80::ONE - frac);
                local[i as usize] = (d5 + d3).to_f64();
            }
        } else {
            let mut fr = frac;
            let d4v = match iv {
                1 => pm * k,
                2 => pm * pm,
                3 => (pm * pm * pm) / k,
                4 => {
                    let t = (pm * pm) / k;
                    t * t
                }
                5 => {
                    let t = (pm * pm) / k;
                    t * t * (pm / k)
                }
                6 => {
                    let t = (pm * pm * pm) / (k * k);
                    t * t
                }
                _ => {
                    fr = zero;
                    let t = (p * p * p) / (k * k);
                    t * t * (p / k)
                }
            };
            if fr.eq(zero) {
                local[i as usize] = d4v.to_f64();
            } else {
                let d5 = (F80::ONE - fr) * d4v;
                let d3 = ((d4v * pm) / k) * fr;
                local[i as usize] = (d5 + d3).to_f64();
            }
        }
    }
    F80::from_f64(local[0]) + F80::from_f64(local[1]) + F80::from_f64(local[2])
}

/// [soft 0x0857ba94 VC_evalList]: `VC_evalList(list, point, contrib)`: each listed metaball's contribution (index and
/// density) at `point` into `contrib`; the sum of the densities in `st0`.
pub fn eval_list(m: &mut Mem, list: u32, pt: u32, cb: u32) -> F80 {
    let n = m.i32(list);
    m.set_i32(cb, n);
    let mut total = 0f64;
    let mut i: i32 = 0;
    while i < m.i32(list) {
        let idx = m.u32(m.u32(list + 8) + 4 * i as u32);
        let cidx = m.u32(cb + 4);
        m.set_u32(cidx + 4 * i as u32, idx);
        let idx = m.i32(m.u32(cb + 4) + 4 * i as u32);
        let b = mb_at(m, idx);
        let t1 = f(m, pt) + f(m, b + 0x1ac);
        let t2 = f(m, pt + 8) + f(m, b + 0x1b4);
        let t3 = f(m, pt + 16) + f(m, b + 0x1bc);
        let mf = |m: &Mem, off: u32| f(m, b + off);
        let t4 = t1 * mf(m, 0x14c) + t2 * mf(m, 0x16c) + t3 * mf(m, 0x18c);
        let t5 = t1 * mf(m, 0x154) + t2 * mf(m, 0x174) + t3 * mf(m, 0x194);
        let t6 = t3 * mf(m, 0x19c) + (t2 * mf(m, 0x17c) + t1 * mf(m, 0x15c));
        let dens = |m: &mut Mem, v: f64| {
            let d = m.u32(cb + 8);
            m.set_f64(d + 8 * i as u32, v);
        };
        // The three coordinates, stored as doubles, are the array `VC_evalComputeSQr2` takes.
        let arr = m.local(24);
        m.set_f64(arr, t4.to_f64());
        m.set_f64(arr + 8, t5.to_f64());
        m.set_f64(arr + 16, t6.to_f64());
        let one = F80::ONE;
        let lo = F80::from_f64(-1.0);
        if one.le(t4) || one.le(t5) || one.le(t6) || t4.le(lo) || t5.le(lo) || t6.le(lo) {
            dens(m, 0.0);
        } else {
            let s = if m.i16(b + mb::FLAG30) == 0 { t6 * t6 + (t5 * t5 + t4 * t4) } else { eval_compute_sqr2(m, arr, idx) };
            if !one.gt(s) {
                dens(m, 0.0);
            } else {
                let w = wyvill2(s.to_f64());
                let k = f(m, mb_at(m, idx) + mb::KSCALE);
                dens(m, (w * k).to_f64());
            }
            let d = m.u32(cb + 8);
            total = (F80::from_f64(total) + f(m, d + 8 * i as u32)).to_f64();
        }
        m.release(24);
        i += 1;
    }
    F80::from_f64(total)
}

/// [soft 0x0857bcac VC_evalContrib]: `VC_evalContrib(list, point, contrib)`: the metaballs of `list` whose box holds
/// `point`, evaluated (split into connected parts when `VC_splitMBList` finds several; the part with the
/// largest sum wins).
pub fn eval_contrib(m: &mut Mem, list: u32, pt: u32, cb: u32) {
    m.set_i32(LOC_MB_LIST, 0);
    let mut i: i32 = 0;
    while i < m.i32(list) {
        let ix = m.u32(m.u32(list + 8) + 4 * i as u32);
        let b = mb_at(m, ix as i32);
        let mut inside = true;
        for k in 0..3u32 {
            let ext = f(m, b + mb::EXTENT + 8 * k);
            let mut s = f(m, pt + 8 * k) + f(m, b + 0x1ac + 8 * k);
            if s.lt(F80::ZERO) {
                s = -s;
            }
            if !s.lt(ext) {
                inside = false;
                break;
            }
        }
        if inside {
            let n = m.i32(LOC_MB_LIST);
            let dst = m.u32(LOC_MB_LIST + 8);
            m.set_u32(dst + 4 * n as u32, ix);
            m.set_i32(LOC_MB_LIST, n + 1);
        }
        i += 1;
    }
    let n = m.i32(LOC_MB_LIST);
    if n == 0 {
        m.set_i32(cb, 0);
        return;
    }
    let mut parts = 1;
    let mut lists = 0u32;
    if n != 1 {
        let pc = m.local(8);
        crate::split_proc::split_mb_list(m, LOC_MB_LIST, pc, pc + 4);
        parts = m.i32(pc);
        lists = m.u32(pc + 4);
        m.release(8);
    }
    if n == 1 || parts == 0 {
        eval_list(m, LOC_MB_LIST, pt, cb);
        return;
    }
    let mut best = F80::ZERO;
    m.set_i32(cb, 0);
    for j in 0..parts {
        let v = eval_list(m, lists + 12 * j as u32, pt, TMP_CBLIST);
        if best.lt(v) {
            let cnt = m.i32(TMP_CBLIST);
            m.set_i32(cb, cnt);
            best = v;
            let mut k = 0;
            while k < m.i32(cb) {
                let (ci, cd) = (m.u32(cb + 4), m.u32(cb + 8));
                let (ti, td) = (m.u32(EVAL_TMP_IDX), m.u32(EVAL_TMP_DENS));
                let w = m.u32(ti + 4 * k as u32);
                m.set_u32(ci + 4 * k as u32, w);
                let d = m.f64(td + 8 * k as u32);
                m.set_f64(cd + 8 * k as u32, d);
                k += 1;
            }
        }
    }
}

/// [soft 0x0857beac VC_evalDens]: `VC_evalDens(list, point)`: the field's density at `point`, in `st0`.
pub fn eval_dens(m: &mut Mem, list: u32, pt: u32) -> F80 {
    let mut s = F80::from_f64(0.0001);
    eval_contrib(m, list, pt, CBLIST);
    let n = m.i32(CBLIST);
    let d = m.u32(EVAL_DENS);
    for i in 0..n.max(0) as u32 {
        s = s + f(m, d + 8 * i);
    }
    s
}

/// [soft 0x0857bef4 VC_evalSurf]: `VC_evalSurf(list, point, surf)`: the colour vector (three doubles) at `point`
/// as the density-weighted mean of the metaballs' vectors. 1 when set, 0 when the density is under 1e-6.
pub fn eval_surf(m: &mut Mem, list: u32, pt: u32, surf: u32) -> i32 {
    eval_contrib(m, list, pt, CBLIST);
    for k in 0..6 {
        m.set_u32(surf + 4 * k, 0);
    }
    let n = m.i32(CBLIST);
    if n == 0 {
        return 0;
    }
    let mut sum = F80::ZERO;
    for i in 0..n.max(0) as u32 {
        let ix = m.i32(m.u32(EVAL_IDX) + 4 * i);
        let col = m.u32(mb_at(m, ix) + mb::COLOR);
        let d = f(m, m.u32(EVAL_DENS) + 8 * i);
        for k in 0..3u32 {
            let v = (d * f(m, col + 8 * k) + f(m, surf + 8 * k)).to_f64();
            m.set_f64(surf + 8 * k, v);
        }
        sum = sum + d;
    }
    if !sum.lt(F80::from_f64(1e-6)) {
        for k in 0..3u32 {
            let v = (f(m, surf + 8 * k) / sum).to_f64();
            m.set_f64(surf + 8 * k, v);
        }
        return 1;
    }
    for k in 0..6 {
        m.set_u32(surf + 4 * k, 0);
    }
    0
}

/// [soft 0x0857c004 VC_evalNorm]: `VC_evalNorm(list, point, normal out, step, tries)`: the normalised density gradient by central
/// differences with a step that doubles while the sides agree. 0, or 0x11 for a zero gradient.
pub fn eval_norm(m: &mut Mem, list: u32, pt: u32, out: u32, step: f64, tries: i32) -> i32 {
    let mut h = step;
    let p = m.local(24);
    let (mut d1, mut d2, mut d3, mut d4) = (0f64, 0f64, 0f64, 0f64);
    let (mut d5, mut d6) = (F80::ZERO, F80::ZERO);
    let mut n = 0;
    loop {
        let (x, y, z) = (m.f64(pt), m.f64(pt + 8), m.f64(pt + 16));
        let hs = |m: &mut Mem, a: f64, b: f64, c: f64| {
            m.set_f64(p, a);
            m.set_f64(p + 8, b);
            m.set_f64(p + 16, c);
        };
        hs(m, (F80::from_f64(h) + F80::from_f64(x)).to_f64(), y, z);
        d1 = eval_dens(m, list, p).to_f64();
        hs(m, (F80::from_f64(x) - F80::from_f64(h)).to_f64(), y, z);
        d2 = eval_dens(m, list, p).to_f64();
        hs(m, x, (F80::from_f64(h) + F80::from_f64(y)).to_f64(), z);
        d3 = eval_dens(m, list, p).to_f64();
        hs(m, x, (F80::from_f64(y) - F80::from_f64(h)).to_f64(), z);
        d4 = eval_dens(m, list, p).to_f64();
        hs(m, x, y, (F80::from_f64(h) + F80::from_f64(z)).to_f64());
        d5 = eval_dens(m, list, p);
        hs(m, x, y, (F80::from_f64(z) - F80::from_f64(h)).to_f64());
        d6 = eval_dens(m, list, p);
        h = (F80::from_f64(h) + F80::from_f64(h)).to_f64();
        n += 1;
        let eq = |a: F80, b: F80| a.eq(b);
        if !eq(F80::from_f64(d1), F80::from_f64(d2)) || !eq(F80::from_f64(d4), F80::from_f64(d3)) || !eq(d6, d5) || n >= tries {
            break;
        }
    }
    m.release(24);
    let gx = F80::from_f64(d2) - F80::from_f64(d1);
    let gy = F80::from_f64(d4) - F80::from_f64(d3);
    let gz = d6 - d5;
    let s = gx * gx + gy * gy + gz * gz;
    if F80::ZERO.eq(s) {
        for k in 0..3u32 {
            m.set_f64(out + 8 * k, 0.0);
        }
        return 0x11;
    }
    let r = s.sqrt();
    m.set_f64(out, (gx / r).to_f64());
    m.set_f64(out + 8, (gy / r).to_f64());
    m.set_f64(out + 16, (gz / r).to_f64());
    0
}
