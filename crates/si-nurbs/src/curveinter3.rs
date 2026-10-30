// SPDX-License-Identifier: MIT
//! `curveInter.c`, third part (package 15, finding 0044 section "Trim4"): `buildSegList`.

use crate::curveinter::*;
use crate::inter::{genlist, ray_dist_3d};
use si_mem::Mem;
use si_x87::F80;

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

/// The side test that sets one of the flag bits of a piece: `(a, a2, p, e)` with `a` the start's coordinate, `a2` the end's,
/// `p` the previous point's; `f` the margin for the end (5e-5 for y, 1e-4 for x), `e` 1e-4. All in 80 bits.
fn side_flag(a: f64, a2: f64, p: f64, f: f64) -> bool {
    let e = x(1e-4);
    let (xa, xa2, xp) = (x(a), x(a2), x(p));
    let c1 = xa.lt(xp - e) && (xa.lt(xa2 - e) || (xa - xa2).abs().lt(e));
    let c2 = xa.lt(xa2 - e) && (xa - xp).abs().lt(e);
    let c3 = (xp + e).lt(xa) && ((xa2 + x(f)).lt(xa) || (xa - xa2).abs().lt(e));
    let c4 = (xa2 + e).lt(xa) && (xa - xp).abs().lt(e);
    c1 || c2 || c3 || c4
}

/// [soft 0x08507c18 buildSegList]: `(list, crv, ci)` answers the bounding box (12 words: minima x y, a word never set, maxima
/// x y, a word never set; the original leaves those two stack words as they were, the port as zero): walks a trim curve's points
/// (0x24-byte entries at +0x2c, knots at +0xc, count at +4, closed word at +0x20), skips pieces shorter than 1e-4 in length
/// (squared distance below 1e-8), makes a segment record for each other one (owner `ci`, inserted into `list`, box grown
/// by its start point; on an open curve the last piece grows it by the end point too and keeps a copy of the box at +0x48) and sets
/// the flag bits 1 (x) and 2 (y) at +0x84 of each piece after the first on an open curve from the turn the previous, own and next
/// points make.
pub fn build_seg_list(m: &mut Mem, list: u32, crv: u32, ci: u32) -> [u32; 12] {
    let big = 3.4028234663852886e38f64;
    let mut bx = [0u32; 12];
    let put = |w: &mut [u32; 12], i: usize, v: f64| {
        let b = v.to_bits();
        w[i] = b as u32;
        w[i + 1] = (b >> 32) as u32;
    };
    put(&mut bx, 0, big);
    put(&mut bx, 2, big);
    put(&mut bx, 6, -big);
    put(&mut bx, 8, -big);
    let n = m.i32(crv + 4);
    let closed = m.i16(crv + 0x20) != 0;
    let nseg = if closed { n } else { n - 1 };
    let pts = m.u32(crv + 0x2c);
    let knots = m.u32(crv + 0xc);
    let scratch = m.local(0x60);
    let mut skip = false;
    let (mut p1x, mut p1y, mut a, mut w1) = (0f64, 0f64, 0f64, 0i32);
    let mut seg = 0u32;
    for i in 0..nseg.max(0) {
        let ent = pts.wrapping_add((i as u32).wrapping_mul(0x24));
        if !skip {
            p1x = m.f64(ent);
            p1y = m.f64(ent + 8);
            a = m.f64(knots + 8 * i as u32);
            w1 = m.i32(ent + 0x20);
        }
        let j = ((i + 1) % n) as u32;
        let e2 = pts.wrapping_add(j.wrapping_mul(0x24));
        let (p2x, p2y) = (m.f64(e2), m.f64(e2 + 8));
        let b = m.f64(knots + 8 * j);
        let w2 = m.i32(e2 + 0x20);
        for (k, v) in [p1x, p1y, 0.0, p2x, p2y, 0.0].iter().enumerate() {
            m.set_f64(scratch + 8 * k as u32, *v);
        }
        let dist = ray_dist_3d(m, scratch, scratch + 24);
        if dist.lt(x(1e-8)) {
            if !closed {
                if i == 0 {
                    skip = true;
                }
                if i == nseg - 1 && seg != 0 {
                    for (k, v) in [p2x, p2y, 0.0].iter().enumerate() {
                        m.set_f64(seg + 0x18 + 8 * k as u32, *v);
                    }
                    m.set_f64(seg + 0x38, b);
                    m.set_i32(seg + 0x44, w2);
                    bx = grow(m, scratch, bx, p2x, p2y);
                    for (k, v) in bx.iter().enumerate() {
                        m.set_u32(seg + 0x48 + 4 * k as u32, *v);
                    }
                }
            }
            continue;
        }
        skip = false;
        let dw = |a: f64, b: f64| -> [u32; 6] {
            let (ab, bb) = (a.to_bits(), b.to_bits());
            [ab as u32, (ab >> 32) as u32, bb as u32, (bb >> 32) as u32, 0, 0]
        };
        seg = segment_inter_create(m, dw(p1x, p1y), dw(p2x, p2y), a, b, w1 as u32, w2 as u32);
        m.set_u32(seg + 0x80, ci);
        genlist::insert_e(m, list, seg);
        bx = grow(m, scratch, bx, p1x, p1y);
        if !closed && i == nseg - 1 {
            bx = grow(m, scratch, bx, p2x, p2y);
            for (k, v) in bx.iter().enumerate() {
                m.set_u32(seg + 0x48 + 4 * k as u32, *v);
            }
        }
        if !closed && i <= 0 {
            continue;
        }
        let pe = if i == 0 {
            pts.wrapping_add((n as u32).wrapping_mul(0x24)).wrapping_sub(0x24)
        } else {
            pts.wrapping_add((((i - 1) % n) as u32).wrapping_mul(0x24))
        };
        let (ppx, ppy) = (m.f64(pe), m.f64(pe + 8));
        if side_flag(p1y, p2y, ppy, 5e-5) {
            let v = m.u8(seg + 0x84);
            m.set_u8(seg + 0x84, v | 2);
        }
        if side_flag(p1x, p2x, ppx, 1e-4) {
            let v = m.u8(seg + 0x84);
            m.set_u8(seg + 0x84, v | 1);
        }
    }
    m.release(0x60);
    bx
}

fn grow(m: &mut Mem, scratch: u32, bx: [u32; 12], px: f64, py: f64) -> [u32; 12] {
    curve_inter_update_bbox(m, scratch + 0x30, bx, px, py);
    let mut r = [0u32; 12];
    for (k, w) in r.iter_mut().enumerate() {
        *w = m.u32(scratch + 0x30 + 4 * k as u32);
    }
    r
}

/// [soft 0x085090b8 computeIntersectionOrientation]: `(e1, e2, ci1, ci2, p5, p6, p7, p8, op)` (the callers pass the second curve's
/// element first). Picks the data of the neighbouring pieces of both cut points (`p6` / `p8` for the second element, `p5` / `p7` for
/// the first: 0 the piece itself, else the previous or next one unless at the end of an open curve's list), takes the z of the cross
/// products of the pieces' directions ([`crate::curveinter2::cross_segments`]; products shorter than 1e-3 are skipped) and answers 0
/// where the signs disagree or none could be taken, else 1 or 2 by the sign of the last z (for `op` 0: 3 / 4 by sign, else 2).
/// The original keeps a "first" flag and a "positive" flag; the quirks (the second product of a pair sets the first flag without
/// recording its sign; a null piece skips its products) are kept as read from the listing.
pub fn compute_intersection_orientation(m: &mut Mem, e1: u32, e2: u32, ci1: u32, ci2: u32, p5: i16, p6: i16, p7: i16, p8: i16, op: i32) -> u32 {
    let data = |m: &Mem, e: u32| genlist::elem_get_data(m, e);
    let (mut t1, mut t2) = (0u32, 0u32);
    let (mut s1, mut s2) = (0u32, 0u32);
    if p6 == 0 {
        t1 = data(m, e2);
        t2 = t1;
    } else if p8 == 0 {
        t1 = data(m, e2);
        if m.u16(ci2 + 0x3a) != 0 || e2 != m.u32(m.u32(ci2) + 4) {
            t2 = data(m, m.u32(e2 + 8));
        }
    } else {
        if m.u16(ci2 + 0x3a) != 0 || e2 != m.u32(m.u32(ci2)) {
            t1 = data(m, m.u32(e2 + 4));
        }
        t2 = data(m, e2);
    }
    if p5 == 0 {
        s1 = data(m, e1);
        s2 = s1;
    } else if p7 == 0 {
        s1 = data(m, e1);
        if m.u16(ci1 + 0x3a) != 0 || e1 != m.u32(m.u32(ci1) + 4) {
            s2 = data(m, m.u32(e1 + 8));
        }
    } else {
        if m.u16(ci1 + 0x3a) != 0 || e1 != m.u32(m.u32(ci1)) {
            s1 = data(m, m.u32(e1 + 4));
        }
        s2 = data(m, e1);
    }
    let thr = x(1e-6); // 0x08661f68
    let mut z = 0.0f64; // the z of the last product (the original reads its stack slot)
    let mut first = true; // flag at ebp-0x48
    let mut pos = false; // flag at ebp-0x46
    // one product: Some(z) when long enough to count
    let cross = |m: &mut Mem, a: u32, b: u32, z: &mut f64| -> Option<f64> {
        let c = crate::curveinter2::cross_segments(m, a, b);
        let (cx, cy, cz) = (x(c[0]), x(c[1]), x(c[2]));
        let len2 = cx * cx + cy * cy + cz * cz;
        *z = c[2];
        if len2.lt(thr) {
            None
        } else {
            Some(c[2])
        }
    };
    let zero = x(0.0);
    // the first product of a pair: records the sign, or compares to it
    macro_rules! acc {
        ($a:expr, $b:expr) => {
            if let Some(v) = cross(m, $a, $b, &mut z) {
                if first {
                    first = false;
                    if zero.lt(x(v)) {
                        pos = true;
                    }
                } else if pos {
                    if x(v).lt(zero) {
                        return 0;
                    }
                } else if zero.lt(x(v)) {
                    return 0;
                }
            }
        };
    }
    // the second product: compares, but while no sign was recorded only clears the flag
    macro_rules! second {
        ($a:expr, $b:expr) => {
            if let Some(v) = cross(m, $a, $b, &mut z) {
                if first {
                    first = false;
                } else if pos {
                    if x(v).lt(zero) {
                        return 0;
                    }
                } else if zero.lt(x(v)) {
                    return 0;
                }
            }
        };
    }
    let done = |first: bool| first;
    if p6 == 0 {
        if p5 == 0 {
            // one product, no flags
            let c = crate::curveinter2::cross_segments(m, s1, t1);
            let (cx, cy, cz) = (x(c[0]), x(c[1]), x(c[2]));
            z = c[2];
            if (cx * cx + cy * cy + cz * cz).lt(thr) {
                return 0;
            }
        } else {
            if s1 != 0 && t2 != 0 {
                if let Some(v) = cross(m, s1, t1, &mut z) {
                    first = false;
                    if zero.lt(x(v)) {
                        pos = true;
                    }
                } else {
                }
            }
            if s2 != 0 && t1 != 0 {
                second!(s2, t1);
            }
            if done(first) {
                return 0;
            }
        }
    } else if p5 == 0 {
        if s1 != 0 {
            if t1 != 0 {
                if let Some(v) = cross(m, s1, t1, &mut z) {
                    first = false;
                    if zero.lt(x(v)) {
                        pos = true;
                    }
                }
            }
            if t2 != 0 {
                second!(s1, t2);
            }
        }
        if done(first) {
            return 0;
        }
    } else {
        if s1 != 0 {
            if t1 != 0 {
                acc!(s1, t1);
            }
            if t2 != 0 {
                acc!(s1, t2);
            }
        }
        if s2 != 0 {
            if t1 != 0 {
                acc!(s2, t1);
            }
            if t2 != 0 {
                second!(s2, t2);
            }
        }
        if done(first) {
            return 0;
        }
    }
    let v = x(z);
    if op == 0 {
        if zero.lt(v) {
            return 3;
        }
        if v.lt(zero) {
            return 4;
        }
    }
    if v.lt(zero) {
        1
    } else {
        2
    }
}
