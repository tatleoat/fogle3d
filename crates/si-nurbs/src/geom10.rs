// SPDX-License-Identifier: MIT
//! nurbsgeom10: `partitionLinearData` (`curveFit.c`) on the memory image (finding 0025, section 28).

use crate::fit::classify_sample;
use crate::geom8::fit_plane_to_samples;
use si_mem::Mem;
use si_x87::{x, F80};

/// The double 3.4028234663852886e+38 at 0x086615d8 (its negative is at 0x086615d0).
const BIG: u64 = 0x47ef_ffff_e000_0000;
/// The double 1e-6 at 0x086615e0.
const EPS: u64 = 0x3eb0_c6f7_a0b5_ed8d;

fn pt(m: &Mem, pts: u32, i: i32) -> [f64; 3] {
    let p = pts.wrapping_add((i as u32).wrapping_mul(0x24));
    [m.f64(p), m.f64(p + 8), m.f64(p + 0x10)]
}

/// Distance between two points in the register: `sqrt((dx^2 + dy^2) + dz^2)`, `d = a - b`.
fn dist(m: &Mem, pts: u32, a: i32, b: i32) -> F80 {
    let (pa, pb) = (pt(m, pts, a), pt(m, pts, b));
    let d = |k: usize| x(pa[k]) - x(pb[k]);
    let (dx, dy, dz) = (d(0), d(1), d(2));
    ((dx * dx + dy * dy) + dz * dz).sqrt()
}

/// [soft 0x084ce53c partitionLinearData]: `(crv, cosine, order, dirs, count, pts, len, n, starts, counts)` (the cosine is a double
/// by value; the last seven are pointers to where the results go). Cuts the control polygon of `crv` (`n` = point count at +4, at
/// least 2; points at +0x2c are homogeneous and are divided through by w, giving a copy with weight 1.0) into pieces at its
/// corners. A point is a corner (flag bit 0) when it is an end of an open polygon or `classifySample` of it and its neighbours
/// (the previous and the next one, wrapping for a closed polygon) with the tolerance `len[i] * len[prev] * cosine` says so;
/// the segment lengths are `len`. For a closed polygon `fitPlaneToSamples` gives two planes and the points of extreme
/// distance to them (to within 1e-6) get flag bit 1; the polygon is rotated to start at the first corner (or flagged extreme)
/// and, when that corner is a true corner, closed again with a copy of the first point. `len` becomes the running chord length
/// (first entry 0). One direction record (0x24 bytes: order +0, knot count +8, closed +0x20, range +0x10/+0x18) is made per
/// piece, with its knots (the order - 1 first zeros or the chord length at each corner repeated order - 1 times, a
/// chord length per extreme point), `starts[k]` the index of its first point and `counts[k]` the points it covers. Answers 0
/// (nothing returned, the point copy leaked as the original does) when `n < 2`, an allocation fails or the plane fit does.
/// Where the original reads an index it never set (NaN data: no plane extreme found) the port takes 0.
#[allow(clippy::too_many_arguments)]
pub fn partition_linear_data(
    m: &mut Mem,
    crv: u32,
    cosine: f64,
    order: i32,
    o_dirs: u32,
    o_cnt: u32,
    o_pts: u32,
    o_len: u32,
    o_n: u32,
    o_starts: u32,
    o_counts: u32,
) -> u32 {
    let n0 = m.i32(crv.wrapping_add(4));
    if n0 <= 1 {
        return 0;
    }
    let closed = m.i16(crv.wrapping_add(0x20)) as i32;
    let mut pts = m.si_calloc(n0 as u32, 0x24);
    if pts == 0 {
        return 0;
    }
    let src = m.u32(crv.wrapping_add(0x2c));
    let mut n = n0;
    let mut last = n - 1; // [ebp-0xcc]
    for i in 0..n as u32 {
        let (s, d) = (src.wrapping_add(0x24 * i), pts.wrapping_add(0x24 * i));
        let t1 = F80::ONE / x(m.f64(s + 0x18));
        m.set_f64(d, (t1 * x(m.f64(s))).to_f64());
        m.set_f64(d + 8, (t1 * x(m.f64(s + 8))).to_f64());
        m.set_f64(d + 0x18, 1.0);
        m.set_f64(d + 0x10, (t1 * x(m.f64(s + 0x10))).to_f64());
    }
    let mut len = m.si_calloc(n as u32, 8);
    for i in 0..last {
        let d = dist(m, pts, i + 1, i);
        m.set_f64(len.wrapping_add(8 * i as u32), d.to_f64());
    }
    if closed != 0 {
        let d = dist(m, pts, n - 1, 0);
        m.set_f64(len.wrapping_add(8 * (n - 1) as u32), d.to_f64());
    }
    let mut flags = m.si_calloc(n as u32, 4);
    let fl = |m: &Mem, f: u32, i: i32| m.u32(f.wrapping_add(4 * i as u32));
    let mut i = 0;
    while i < n {
        let mut prev = i - 1;
        let mut next = i + 1;
        if closed != 0 {
            if i == 0 {
                prev = last;
            }
            if i == last {
                next = 0;
            }
        }
        if closed == 0 && (i == 0 || i == last) {
            m.set_u32(flags.wrapping_add(4 * i as u32), 1);
        } else {
            let tol = (x(m.f64(len.wrapping_add(8 * i as u32))) * x(m.f64(len.wrapping_add(8 * prev as u32))) * x(cosine)).to_f64();
            let r = classify_sample(pt(m, pts, i), pt(m, pts, prev), pt(m, pts, next), tol);
            m.set_u32(flags.wrapping_add(4 * i as u32), r as u32);
        }
        i += 1;
    }
    if closed != 0 {
        let sp = m.sp();
        let outs: [u32; 8] = std::array::from_fn(|_| m.local(8));
        let ok = fit_plane_to_samples(m, pts, n, outs) & 0xffff;
        if ok == 0 {
            m.set_sp(sp);
            if len != 0 {
                m.free(len);
            }
            if flags != 0 {
                m.free(flags);
            }
            return 0;
        }
        let c: Vec<F80> = outs.iter().map(|&p| x(m.f64(p))).collect();
        m.set_sp(sp);
        let (c40, c38, c30, c28, c20, c18, c10, c08) = (c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]);
        let big = f64::from_bits(BIG);
        let eps = x(f64::from_bits(EPS));
        let (mut max1, mut max2, mut min2) = (-big, -big, big);
        let mut min1 = x(big); // kept in the register
        let (mut i70, mut i6c, mut i74, mut i78) = (0i32, 0i32, 0i32, 0i32);
        // the coefficients are stored as doubles first (they are doubles already)
        let mut k = 0;
        while k < n {
            let p = pt(m, pts, k);
            let (px, py, pz) = (x(p[0]), x(p[1]), x(p[2]));
            let t2 = ((c38 * py + c40 * px) + c30 * pz) + c28;
            if t2.lt(min1 - eps) {
                i70 = k;
                min1 = t2;
            }
            if (x(max1) + eps).lt(t2) {
                max1 = t2.to_f64();
                i6c = k;
            }
            let t3 = (pz * c10 + (py * c18 + px * c20)) + c08;
            if t3.lt(x(min2) - eps) {
                min2 = t3.to_f64();
                i74 = k;
            }
            if (x(max2) + eps).lt(t3) {
                max2 = t3.to_f64();
                i78 = k;
            }
            k += 1;
        }
        for idx in [i70, i6c, i74, i78] {
            let a = flags.wrapping_add(4 * idx as u32);
            if m.u8(a) & 1 == 0 {
                m.set_u8(a, m.u8(a) | 2);
            }
        }
    }
    // the first corner (or extreme point) and the number of corners
    let mut start = -1i32;
    let mut cnt = 0i32;
    for i in 0..n {
        let f = fl(m, flags, i);
        if f & 1 != 0 {
            cnt += 1;
            if closed != 0 && (start < 0 || fl(m, flags, start) & 2 != 0) {
                start = i;
            }
        } else if closed != 0 && start < 0 && f & 2 != 0 {
            start = i;
        }
    }
    if closed != 0 && start >= 0 {
        let extra = fl(m, flags, start) & 1 != 0;
        let m2 = if extra { n + 1 } else { n } as u32;
        let np = m.si_calloc(m2, 0x24);
        let nl = m.si_calloc(m2, 8);
        let nf = m.si_calloc(m2, 4);
        let mv = |m: &mut Mem, dst: i32, srci: i32| {
            m.copy(np.wrapping_add(0x24 * dst as u32), pts.wrapping_add(0x24 * srci as u32), 0x24);
            let v = m.f64(len.wrapping_add(8 * srci as u32));
            m.set_f64(nl.wrapping_add(8 * dst as u32), v);
            let v = m.u32(flags.wrapping_add(4 * srci as u32));
            m.set_u32(nf.wrapping_add(4 * dst as u32), v);
        };
        let tail = n - start;
        for k in 0..tail {
            mv(m, k, start + k);
        }
        for k in 0..start {
            mv(m, tail + k, k);
        }
        if len != 0 {
            m.free(len);
        }
        len = nl;
        m.free(pts);
        pts = np;
        if flags != 0 {
            m.free(flags);
        }
        flags = nf;
        if m.u32(flags) & 1 != 0 {
            n += 1;
            m.copy(pts.wrapping_add(0x24 * (n - 1) as u32), pts, 0x24);
            let d = dist(m, pts, n - 2, 0);
            m.set_f64(len.wrapping_add(8 * (n - 1) as u32), d.to_f64());
            m.set_u32(flags.wrapping_add(4 * (n - 1) as u32), 1);
            cnt += 1;
            last = n - 1;
        }
    }
    // running chord length
    let mut k = last;
    while k > 0 {
        let v = m.f64(len.wrapping_add(8 * (k - 1) as u32));
        m.set_f64(len.wrapping_add(8 * k as u32), v);
        k -= 1;
    }
    m.set_u32(len, 0);
    m.set_u32(len + 4, 0);
    for k in 1..n {
        let s = x(m.f64(len.wrapping_add(8 * k as u32))) + x(m.f64(len.wrapping_add(8 * (k - 1) as u32)));
        m.set_f64(len.wrapping_add(8 * k as u32), s.to_f64());
    }
    let np = if fl(m, flags, 0) & 1 != 0 { cnt - 1 } else { 1 };
    let dirs = m.si_calloc(np as u32, 0x24);
    let starts = m.si_calloc(np as u32, 4);
    let counts = m.si_calloc(np as u32, 4);
    let d = |k: i32| dirs.wrapping_add(0x24u32.wrapping_mul(k as u32));
    if closed == 0 || cnt != 0 {
        m.set_i32(dirs + 8, order - 1);
    } else {
        m.set_i32(dirs + 8, 2);
    }
    m.set_i16(dirs + 0x20, if closed != 0 && cnt == 0 { 1 } else { 0 });
    m.set_i32(dirs + 0x10, 0);
    m.set_i32(dirs + 0x14, 0);
    m.set_i32(dirs, order);
    m.set_i32(starts, 0);
    m.set_i32(counts, 1);
    let mut cur = 0i32;
    for j in 1..n {
        let c = counts.wrapping_add(4 * cur as u32);
        m.set_i32(c, m.i32(c).wrapping_add(1));
        let f = fl(m, flags, j);
        if f & 1 != 0 {
            let nk = m.i32(d(cur) + 8).wrapping_sub(1).wrapping_add(order);
            m.set_i32(d(cur) + 8, nk);
            let lj = m.f64(len.wrapping_add(8 * j as u32));
            m.set_f64(d(cur) + 0x18, lj);
            cur += 1;
            if cur < np {
                m.set_i32(starts.wrapping_add(4 * cur as u32), j);
                m.set_i32(counts.wrapping_add(4 * cur as u32), 1);
                m.set_i16(d(cur) + 0x20, 0);
                m.set_i32(d(cur), order);
                m.set_i32(d(cur) + 8, order - 1);
                m.set_f64(d(cur) + 0x10, lj);
            }
        } else if f & 2 != 0 {
            m.set_i32(d(cur) + 8, m.i32(d(cur) + 8).wrapping_add(1));
        }
    }
    let k0 = m.si_calloc(m.u32(d(0) + 8), 8);
    m.set_u32(d(0) + 0xc, k0);
    let mut e = 0i32; // the next knot of the current piece
    loop {
        let stop = if closed == 0 || cnt != 0 { e >= order - 1 } else { e > 0 };
        if stop {
            break;
        }
        let kn = m.u32(d(0) + 0xc);
        m.set_f64(kn.wrapping_add(8 * e as u32), 0.0);
        e += 1;
    }
    let mut cur = 0i32;
    for j in 1..n {
        let f = fl(m, flags, j);
        let lj = m.f64(len.wrapping_add(8 * j as u32));
        if f & 1 != 0 {
            for _ in 0..(order - 1).max(0) {
                let kn = m.u32(d(cur) + 0xc);
                m.set_f64(kn.wrapping_add(8 * e as u32), lj);
                e += 1;
            }
            cur += 1;
            if cur < np {
                let kn = m.si_calloc(m.u32(d(cur) + 8), 8);
                m.set_u32(d(cur) + 0xc, kn);
                e = 0;
                for _ in 0..(order - 1).max(0) {
                    m.set_f64(kn.wrapping_add(8 * e as u32), lj);
                    e += 1;
                }
            }
        } else if f & 2 != 0 {
            let kn = m.u32(d(cur) + 0xc);
            m.set_f64(kn.wrapping_add(8 * e as u32), lj);
            e += 1;
        }
    }
    if m.i16(dirs + 0x20) != 0 {
        let nk = m.i32(dirs + 8);
        let kn = m.u32(dirs + 0xc);
        let v = (dist(m, pts, n - 1, 0) + x(m.f64(len.wrapping_add(8 * (n - 1) as u32)))).to_f64();
        m.set_f64(kn.wrapping_add(8 * (nk - 1) as u32), v);
        m.set_f64(dirs + 0x18, v);
    }
    if flags != 0 {
        m.free(flags);
    }
    m.set_u32(o_dirs, dirs);
    m.set_i32(o_cnt, np);
    m.set_u32(o_pts, pts);
    m.set_u32(o_len, len);
    m.set_i32(o_n, n);
    m.set_u32(o_starts, starts);
    m.set_u32(o_counts, counts);
    1
}
