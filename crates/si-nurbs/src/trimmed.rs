// SPDX-License-Identifier: MIT
//! The tessellation of a trimmed NURBS surface: the original's `trimmed.c` (finding 0044),
//! on the memory image.
//!
//! A trim curve (a composite curve in the surface's parameter plane) is sampled into a ring
//! of `uvSample` records (0x24 bytes, below); the samples of all the trim curves are cut by
//! vertical strips, and the pieces inside the surface are turned into triangles. Every
//! routine here takes the image and the arguments the original takes.
//!
//! The records, read from the code (names are ours):
//!
//! - `uvSample` (0x24): +0 next, +4 previous (a ring), +8 mate (another sample, or null),
//!   +0xc u (double), +0x14 v (double), +0x1c flags, +0x20 index (−1 until numbered);
//! - `SampleCrv` (8): +0 next curve, +4 the ring of samples;
//! - `SampleCrvSegment` (0x18): +0 next, +4 previous (a ring), +8 first sample, +0xc last
//!   sample, +0x10 flags, +0x14 sample count;
//! - `vStrip` (12): +0 and +4 counts, +8 the ring of segments.
//!
//! Arithmetic follows the x87 sequences: sums and products of two doubles stay in the
//! 80-bit register until the original stores them, comparisons are `fcom` with the tests of
//! the condition bits (`lt`, `gt` are false for an unordered pair, as the tests read them).

use crate::image::{crv_value, sample_knot_vector, Halt};
use si_mem::Mem;
use si_x87::{Round, F80};

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

/// `uvSample` field offsets.
pub const NEXT: u32 = 0;
pub const PREV: u32 = 4;
pub const MATE: u32 = 8;
pub const U: u32 = 0xc;
pub const V: u32 = 0x14;
pub const FLAGS: u32 = 0x1c;
pub const INDEX: u32 = 0x20;

fn u_of(m: &Mem, s: u32) -> F80 {
    x(m.f64(s + U))
}
fn v_of(m: &Mem, s: u32) -> F80 {
    x(m.f64(s + V))
}

/// 0x08661d90: the tolerance of the extremum test (5e-6); 0x08661d98: that of the
/// same-parameter test (1e-5).
const TOL1: f64 = 5e-6;
const TOL2: f64 = 1e-5;
/// 0x08661d70, 0x08661d78, 0x08661d80: a trim curve's coordinates are snapped to a grid of
/// 2e-5 (`trunc(50000 * c + 0.5) * 2e-5`, the product in the register).
const SNAP_SCALE: f64 = 50000.0;
const SNAP_HALF: f64 = 0.5;
const SNAP_STEP: f64 = 2e-5;
/// 0x08661d88: a sample closer than this to the last in both coordinates is dropped.
const SAME: f64 = 1e-5;
/// 0x08661df0: the tolerance of the count of vertices.
const COUNT_TOL: f64 = 1e-5;

/// [soft 0x084ff8a0 NewuvSample]: `NewuvSample(u, v, flags)`: a zeroed `uvSample` from
/// `SI_calloc(1, 0x24)` with the two coordinates, the flags and the index −1; null when the
/// allocation fails.
pub fn new_uv_sample(m: &mut Mem, u: f64, v: f64, flags: i32) -> u32 {
    let s = m.si_calloc(1, 0x24);
    if s == 0 {
        return 0;
    }
    m.set_f64(s + U, u);
    m.set_f64(s + V, v);
    m.set_u32(s + NEXT, 0);
    m.set_u32(s + MATE, 0);
    m.set_i32(s + FLAGS, flags);
    m.set_i32(s + INDEX, -1);
    s
}

/// [soft 0x084fe148 FreeSampleList]: frees a ring of samples. A ring is cut open first (the
/// previous of the head gets next = null); a list that is not a ring is freed as it stands.
pub fn free_sample_list(m: &mut Mem, list: u32) {
    if list == 0 {
        return;
    }
    let prev = m.u32(list + PREV);
    if prev != 0 {
        m.set_u32(prev + NEXT, 0);
    }
    let mut p = list;
    loop {
        let next = m.u32(p + NEXT);
        if p != 0 {
            m.free(p);
        }
        p = next;
        if p == 0 {
            break;
        }
    }
}

/// [soft 0x084fde50 ClassifyOneSample]: the flags of a sample from its neighbours in the
/// ring (previous `p`, next `n`):
///
/// - 0x2: v is a local extreme (below both neighbours' v minus 5e-6, or above both plus
///   5e-6); 0x1: the same for u;
/// - 0x4: v is more than 1e-5 below the previous v; 0x8: more than 1e-5 above it;
///   otherwise 0x10 (level with the previous) and, when v is also off the next by more
///   than 5e-6 either way, 0x200;
/// - 0x100: not level with the previous but within 5e-6 of the next v;
/// - 0x80: u is within 1e-5 of the previous u (strictly).
pub fn classify_one_sample(m: &mut Mem, s: u32) {
    let (e1, e2) = (x(TOL1), x(TOL2));
    let p = m.u32(s + PREV);
    let n = m.u32(s + NEXT);
    m.set_u32(s + FLAGS, 0);
    let mut fl = 0u32;
    // the order of the two blocks (v then u) is the original's; the flags are or-ed into memory
    for (off, bit) in [(V, 2u32), (U, 1u32)] {
        let pv = x(m.f64(p + off));
        let nv = x(m.f64(n + off));
        let sv = x(m.f64(s + off));
        let lo = pv - e1;
        let hit = (sv.lt(lo) && (nv - e1).gt(sv)) || (sv.gt(pv + e1) && (nv + e1).lt(sv));
        if hit {
            fl |= bit;
        }
    }
    m.set_u32(s + FLAGS, fl);
    // block 3: v against the previous, tolerance 1e-5
    let pv = v_of(m, p);
    let sv = v_of(m, s);
    if sv.lt(pv - e2) {
        fl |= 4;
        m.set_u32(s + FLAGS, fl);
    } else if sv.gt(pv + e2) {
        fl |= 8;
        m.set_u32(s + FLAGS, fl);
    } else {
        let old = fl;
        fl |= 0x10;
        m.set_u32(s + FLAGS, fl);
        let nv = v_of(m, n);
        if sv.lt(nv - e1) || sv.gt(nv + e1) {
            fl = old | 0x210;
            m.set_u32(s + FLAGS, fl);
        }
    }
    // block 4: within 1e-5 of the next v
    if fl & 0x10 == 0 {
        let nv = v_of(m, n);
        if sv.gt(nv - e1) && sv.lt(nv + e1) {
            fl |= 0x100;
            m.set_u32(s + FLAGS, fl);
        }
    }
    // block 5: u within 1e-5 of the previous u
    let pu = u_of(m, p);
    let su = u_of(m, s);
    if su.gt(pu - e2) && su.lt(pu + e2) {
        m.set_u8(s + FLAGS, m.u8(s + FLAGS) | 0x80);
    }
}

/// [soft 0x084fe034 ClassifySamples]: classifies every sample of a `SampleCrv`'s ring, and
/// keeps a state: set by a sample with 0x100, cleared by a later one with 0x200 (the flag
/// 0x200 is removed again from a sample met while the state is clear).
pub fn classify_samples(m: &mut Mem, crv: u32) {
    let head = m.u32(crv + 4);
    let mut open = false;
    let mut s = head;
    loop {
        classify_one_sample(m, s);
        let fl = m.u32(s + FLAGS);
        if fl & 0x100 != 0 {
            open = true;
        } else if !open {
            if fl & 0x200 != 0 {
                m.set_u32(s + FLAGS, fl & !0x200);
            }
        } else if fl & 0x200 != 0 {
            open = false;
        }
        s = m.u32(s + NEXT);
        if s == head {
            break;
        }
    }
}

/// [soft 0x084fe08c CounterClockwise]: whether the ring of `crv` runs counter-clockwise,
/// written to `out` (16 bits); returns 0 when the ring has no usable extreme. The test takes
/// the sample before the one with the least u (or, when none is below `f32::MAX`, the
/// greatest) as `a`, with `b` and `c` the two after it: the sign of
/// `(c.u − b.u)(a.v − b.v) − (a.u − b.u)(c.v − b.v)`, every product and difference in the
/// register. The original records the *previous* sample in the ring walk as the extreme's
/// index (null for the first sample of the walk).
pub fn counter_clockwise(m: &mut Mem, crv: u32, out: u32) -> u16 {
    match ring_is_ccw(m, crv) {
        None => 0,
        Some(c) => {
            m.set_u16(out, c as u16);
            1
        }
    }
}

/// The body of [`counter_clockwise`] with the 16-bit answer returned.
fn ring_is_ccw(m: &Mem, crv: u32) -> Option<bool> {
    let head = m.u32(crv + 4);
    let mut minv = x(f32::MAX as f64);
    let mut maxv = x(-(f32::MAX as f64));
    let (mut min_i, mut max_i) = (0u32, 0u32);
    let (mut prev, mut cur) = (0u32, head);
    loop {
        let u = u_of(m, cur);
        if u.lt(minv) {
            minv = u;
            min_i = prev;
        }
        if u.gt(maxv) {
            maxv = u;
            max_i = prev;
        }
        prev = cur;
        cur = m.u32(cur + NEXT);
        if cur == m.u32(crv + 4) {
            break;
        }
    }
    let a = if min_i != 0 {
        min_i
    } else if max_i != 0 {
        max_i
    } else {
        return None;
    };
    let b = m.u32(a + NEXT);
    let c = m.u32(b + NEXT);
    let t1 = v_of(m, a) - v_of(m, b);
    let t2 = u_of(m, c) - u_of(m, b);
    let t3 = u_of(m, a) - u_of(m, b);
    let t4 = v_of(m, c) - v_of(m, b);
    let d = t2 * t1 - t3 * t4;
    Some(F80::ZERO.lt(d))
}

/// [soft 0x084ff800 NewSampleCrvSegment]: a segment from two samples (`SI_calloc(1, 0x18)`):
/// +8 and +0xc are the samples; the flags (+0x10) are 1 or 2 (2 when the first has 0x20),
/// or-ed with 4 or 8 (8 when the second has 0x20), and 0x10 when both have 0x20 and the
/// first's u is below the second's, or both have 0x40 and the first's u is above it.
pub fn new_sample_crv_segment(m: &mut Mem, s0: u32, s1: u32) -> u32 {
    let seg = m.si_calloc(1, 0x18);
    if seg == 0 {
        return 0;
    }
    m.set_u32(seg + 8, s0);
    m.set_u32(seg + 0xc, s1);
    let f0 = m.u8(s0 + FLAGS);
    let f1 = m.u8(s1 + FLAGS);
    let ecx = if f0 & 0x20 != 0 { 2 } else { 1 };
    m.set_u32(seg + 0x10, ecx);
    let mut edx = ecx | 4;
    if f1 & 0x20 != 0 {
        edx = ecx | 8;
    }
    m.set_u32(seg + 0x10, edx);
    if f0 & 0x20 != 0 && f1 & 0x20 != 0 && u_of(m, s0).lt(u_of(m, s1)) {
        m.set_u32(seg + 0x10, edx | 0x10);
        return seg;
    }
    if f0 & 0x40 != 0 && f1 & 0x40 != 0 && u_of(m, s0).gt(u_of(m, s1)) {
        let v = m.u8(seg + 0x10) | 0x10;
        m.set_u8(seg + 0x10, v);
    }
    seg
}

/// [soft 0x084fe184 FreeSampleCrv]: frees a chain of sample curves: the rest of the chain
/// first (recursively), then this curve's ring, then the record.
pub fn free_sample_crv(m: &mut Mem, crv: u32) {
    let next = m.u32(crv);
    if next != 0 {
        free_sample_crv(m, next);
    }
    let ring = m.u32(crv + 4);
    if ring != 0 {
        free_sample_list(m, ring);
    }
    m.set_u32(crv + 4, 0);
    if crv != 0 {
        m.free(crv);
    }
}

/// [soft 0x084fe1d0 FreevStripArray]: `(strips, n)`: for every strip with a segment ring the
/// ring is cut open at the head's previous and each segment's samples are freed with the
/// segment; then the array.
pub fn free_vstrip_array(m: &mut Mem, strips: u32, n: i32) {
    let mut i = 0i32;
    while i < n {
        let head = m.u32(strips.wrapping_add((i as u32).wrapping_mul(12)).wrapping_add(8));
        if head != 0 {
            let prev = m.u32(head + 4);
            m.set_u32(prev, 0);
            let mut seg = head;
            loop {
                let next = m.u32(seg);
                let list = m.u32(seg + 8);
                free_sample_list(m, list);
                if seg != 0 {
                    m.free(seg);
                }
                seg = next;
                if seg == 0 {
                    break;
                }
            }
        }
        i += 1;
    }
    if strips != 0 {
        m.free(strips);
    }
}

/// [soft 0x085012d0 FreeTrimmedTemporaryData]: `(a, b, crvs, c, d, strips, n)`, all pointers
/// to the caller's locals: the strips (with `n`), `d`, `c`, the sample curves, `b` and
/// `a` are freed in that order, each local set to null.
pub fn free_trimmed_temporary_data(m: &mut Mem, a: u32, b: u32, crvs: u32, c: u32, d: u32, strips: u32, n: i32) {
    let s = m.u32(strips);
    if s != 0 {
        free_vstrip_array(m, s, n);
        m.set_u32(strips, 0);
    }
    for p in [d, c] {
        let v = m.u32(p);
        if v != 0 {
            m.free(v);
            m.set_u32(p, 0);
        }
    }
    let v = m.u32(crvs);
    if v != 0 {
        free_sample_crv(m, v);
        m.set_u32(crvs, 0);
    }
    for p in [b, a] {
        let v = m.u32(p);
        if v != 0 {
            m.free(v);
            m.set_u32(p, 0);
        }
    }
}

/// [soft 0x08501390 UnlinkMates]: `(strips, n, counts, limit)`: for strips 1.. while fewer
/// than `limit` strips with a positive count were met, every sample with 0x40 of every
/// segment of the strip gives up its mate (both ends' mate set to null).
pub fn unlink_mates(m: &mut Mem, strips: u32, n: i32, counts: u32, limit: i32) {
    let mut i = 1i32;
    let mut met = 0i32;
    while i < n && met < limit {
        let next = i + 1;
        if m.i32(counts.wrapping_add((i as u32).wrapping_mul(4))) > 0 {
            met += 1;
            let head = m.u32(strips.wrapping_add((i as u32).wrapping_mul(12)).wrapping_add(8));
            let mut seg = head;
            if seg != 0 {
                loop {
                    let first = m.u32(seg + 8);
                    let mut s = first;
                    loop {
                        if m.u8(s + FLAGS) & 0x40 != 0 {
                            let mate = m.u32(s + MATE);
                            if mate != 0 {
                                m.set_u32(mate + MATE, 0);
                                m.set_u32(s + MATE, 0);
                            }
                        }
                        s = m.u32(s + NEXT);
                        if s == first {
                            break;
                        }
                    }
                    seg = m.u32(seg + NEXT);
                    if seg == head {
                        break;
                    }
                }
            }
        }
        i = next;
    }
}

/// [soft 0x08500c9c ProjectPtToEdge]: `(a, b, c)`: the doubled signed area of the triangle
/// `b, a, c`, `(a.v − b.v)(c.u − b.u) − (a.u − b.u)(c.v − b.v)`; the original leaves it
/// unrounded in st0.
pub fn project_pt_to_edge(m: &Mem, a: u32, b: u32, c: u32) -> F80 {
    let t1 = v_of(m, a) - v_of(m, b);
    let t2 = u_of(m, c) - u_of(m, b);
    let t3 = u_of(m, a) - u_of(m, b);
    let t4 = v_of(m, c) - v_of(m, b);
    t1 * t2 - t3 * t4
}

/// [soft 0x08500c6c PtInteriorToEdge]: whether [`project_pt_to_edge`] is above 0.
pub fn pt_interior_to_edge(m: &Mem, a: u32, b: u32, c: u32) -> bool {
    F80::ZERO.lt(project_pt_to_edge(m, a, b, c))
}

/// [soft 0x085007d4 advanceTri]: `(pa, pb, pc)`: the three pointers are cells holding
/// samples; the triangulation of a monotone chain steps either the walk on `*pb` backwards
/// (previous) or that on `*pc` forwards (next), and the sample stepped *from* is written
/// to `*pa`.
pub fn advance_tri(m: &mut Mem, pa: u32, pb: u32, pc: u32) {
    let (from, s, b) = advance_tri_core(m, m.u32(pb), m.u32(pc));
    m.set_u32(pa, from);
    m.set_u32(pc, b);
    m.set_u32(pb, s);
}

/// The body of [`advance_tri`] on the values of the cells: `(s, b)` in, `(from, s, b)` out.
fn advance_tri_core(m: &Mem, s: u32, b: u32) -> (u32, u32, u32) {
    let (mut s, mut b) = (s, b);
    let take_s = |m: &Mem, s: &mut u32| {
        let r = *s;
        *s = m.u32(r + PREV);
        r
    };
    let take_b = |m: &Mem, b: &mut u32| {
        let r = *b;
        *b = m.u32(r + NEXT);
        r
    };
    let from;
    if m.u8(b + FLAGS + 1) & 4 != 0 {
        from = take_s(m, &mut s);
    } else if m.u8(s + FLAGS + 1) & 4 != 0 {
        from = take_b(m, &mut b);
    } else {
        let sp = m.u32(s + PREV);
        let bn = m.u32(b + NEXT);
        if !pt_interior_to_edge(m, b, sp, s) {
            from = take_b(m, &mut b);
        } else if !pt_interior_to_edge(m, bn, sp, s) {
            from = take_s(m, &mut s);
        } else if !pt_interior_to_edge(m, s, b, bn) {
            from = take_s(m, &mut s);
        } else if !pt_interior_to_edge(m, sp, b, bn) {
            from = take_b(m, &mut b);
        } else if !u_of(m, bn).lt(u_of(m, sp)) && !u_of(m, bn).unordered(u_of(m, sp)) {
            from = take_s(m, &mut s);
        } else {
            from = take_b(m, &mut b);
        }
    }
    (from, s, b)
}

/// [soft 0x084fff64 CountVerticesTriangles]: `(strips, n, ptri, pvert)`. The third word receives the triangle total (each
/// segment adds its count - 2), the fourth the sample total (every sample that is not a mated 0x20 one); the original's
/// callers size the triangle array with the first and the vertex array with the second. For each strip the two
/// counters (+0 and +4) are zeroed, then for every segment its samples are counted (+0x14
/// of the segment): a sample counts unless it has 0x400, and then only when its u exceeds
/// both neighbours' by more than 1e-5; a sample without 0x20, or with 0x20 and no mate,
/// adds a triangle; samples with 0x20 add to the strip's +0 counter, with 0x40 (and not
/// 0x20) to its +4. Each segment adds its count − 2 to the vertex total.
pub fn count_vertices_triangles(m: &mut Mem, strips: u32, n: i32, ptri: u32, pvert: u32) {
    let eps = x(COUNT_TOL);
    let mut nv = 0i32;
    let mut nt = 0i32;
    let mut i = 0i32;
    while i < n {
        let rec = strips.wrapping_add((i as u32).wrapping_mul(12));
        let head = m.u32(rec + 8);
        if head != 0 {
            let (mut c40, mut c20) = (0i32, 0i32);
            m.set_u32(rec + 4, 0);
            m.set_u32(rec, 0);
            let mut seg = head;
            loop {
                let first = m.u32(seg + 8);
                let base = nv.wrapping_sub(2);
                let mut cnt = 0i32;
                let mut s = first;
                loop {
                    let fl = m.u32(s + FLAGS);
                    let next = m.u32(s + NEXT);
                    if fl & 0x400 == 0 {
                        cnt += 1;
                    } else if u_of(m, s).gt(u_of(m, next) + eps) {
                        let prev = m.u32(s + PREV);
                        if (u_of(m, prev) + eps).lt(u_of(m, s)) {
                            cnt += 1;
                        }
                    }
                    if fl & 0x20 == 0 || m.u32(s + MATE) == 0 {
                        nt += 1;
                    }
                    if fl & 0x20 != 0 {
                        c20 += 1;
                    } else if fl & 0x40 != 0 {
                        c40 += 1;
                    }
                    s = next;
                    if s == first {
                        break;
                    }
                }
                m.set_i32(seg + 0x14, cnt);
                nv = base.wrapping_add(cnt);
                seg = m.u32(seg + NEXT);
                if seg == head {
                    break;
                }
            }
            let rec = strips.wrapping_add((i as u32).wrapping_mul(12));
            m.set_i32(rec + 4, c40);
            m.set_i32(rec, c20);
        }
        i += 1;
    }
    m.set_i32(ptri, nv);
    m.set_i32(pvert, nt);
}

/// A coordinate of a trim curve as the original snaps it to its grid (the product, the sum
/// and the product by 2e-5 stay in the register; the cut is toward zero).
fn snap(c: f64) -> F80 {
    let t = (x(SNAP_SCALE) * x(c) + x(SNAP_HALF)).to_i32(Round::Zero);
    F80::from_i32(t) * x(SNAP_STEP)
}

/// [soft 0x084fd970 NewSampleCrv]: the ring of samples of a composite trim curve `comp`
/// (+0 boundary flag, +4 count of curves, +8 their array), clipped to the box
/// `[u0, u1] × [v0, v1]`. Each curve is sampled by `SI_SampleKnotVector` (at its +0x34
/// steps); the curve's value at every sample but the last is snapped to a grid of 2e-5,
/// clamped to the box, and kept when it differs from the last kept sample by more than 1e-5
/// in u or in v. The result `SampleCrv` holds the ring, reversed when its direction
/// disagrees with the boundary flag (counter-clockwise rings are wanted for a boundary
/// curve, clockwise ones for a hole). Null when an allocation fails or the ring has no
/// extreme.
pub fn new_sample_crv(m: &mut Mem, comp: u32, u0: f64, u1: f64, v0: f64, v1: f64) -> Result<u32, Halt> {
    let nb = m.i32(comp + 4);
    let counts = m.si_calloc(nb as u32, 4);
    if counts == 0 {
        return Ok(0);
    }
    let arrs = m.si_calloc(nb as u32, 4);
    if arrs == 0 {
        m.free(counts);
        return Ok(0);
    }
    let at = |b: u32, i: i32, s: u32| b.wrapping_add((i as u32).wrapping_mul(s));
    let mut i = 0i32;
    while i < m.i32(comp + 4) {
        let curve = m.u32(at(m.u32(comp + 8), i, 4));
        let per = m.i32(curve + 0x34);
        sample_knot_vector(m, curve, per, at(arrs, i, 4), at(counts, i, 4))?;
        i += 1;
    }
    let first = m.si_calloc(1, 0x24);
    let free_all = |m: &mut Mem| {
        let mut i = 0i32;
        while i < m.i32(comp + 4) {
            let a = m.u32(at(arrs, i, 4));
            if a != 0 {
                m.free(a);
                m.set_u32(at(arrs, i, 4), 0);
            }
            i += 1;
        }
    };
    if first == 0 {
        free_all(m);
        m.free(arrs);
        m.free(counts);
        return Ok(0);
    }
    let (u0, u1, v0, v1) = (x(u0), x(u1), x(v0), x(v1));
    let mut last = 0u32;
    let mut b = 0i32;
    let nb2 = m.i32(comp + 4);
    while b < nb2 {
        let curve = m.u32(at(m.u32(comp + 8), b, 4));
        let cnt = m.i32(at(counts, b, 4));
        let mut j = 0i32;
        while j < cnt.wrapping_sub(1) {
            let t = m.f64(at(m.u32(at(arrs, b, 4)), j, 8));
            let val = crv_value(m, curve, t, 0)?;
            let mut u = snap(val[0]);
            if u.lt(u0) {
                u = u0;
            }
            if u.gt(u1) {
                u = u1;
            }
            let mut v = snap(val[1]);
            if v.lt(v0) {
                v = v0;
            }
            if v.gt(v1) {
                v = v1;
            }
            if last == 0 {
                last = first;
                m.set_f64(first + U, u.to_f64());
                m.set_f64(first + V, v.to_f64());
                m.set_i32(first + FLAGS, -1);
                m.set_i32(first + INDEX, -1);
                m.set_u32(first + PREV, first);
                m.set_u32(first + NEXT, first);
            } else {
                let eps = x(SAME);
                let keep = eps.lt((u - u_of(m, last)).abs()) || (v - v_of(m, last)).abs().gt(eps);
                if keep {
                    let s = new_uv_sample(m, u.to_f64(), v.to_f64(), -1);
                    if s == 0 {
                        free_all(m);
                        m.free(arrs);
                        m.free(counts);
                        free_sample_list(m, first);
                        return Ok(0);
                    }
                    let head = m.u32(last + NEXT);
                    m.set_u32(s + NEXT, head);
                    m.set_u32(head + PREV, s);
                    m.set_u32(s + PREV, last);
                    m.set_u32(last + NEXT, s);
                    last = s;
                }
            }
            j += 1;
        }
        b += 1;
    }
    free_all(m);
    m.free(arrs);
    m.free(counts);
    let crv = m.si_calloc(1, 8);
    if crv == 0 {
        free_sample_list(m, first);
        return Ok(0);
    }
    m.set_u32(crv + 4, first);
    let Some(ccw) = ring_is_ccw(m, crv) else {
        free_sample_list(m, first);
        m.free(crv);
        return Ok(0);
    };
    let boundary = m.u16(comp) != 0;
    if ccw != boundary {
        // reverse the ring: cut it open, then push each sample after the head
        let head = m.u32(crv + 4);
        let hp = m.u32(head + PREV);
        m.set_u32(hp + NEXT, 0);
        let mut cur = m.u32(head + NEXT);
        m.set_u32(head + NEXT, head);
        m.set_u32(head + PREV, head);
        while cur != 0 {
            let s = cur;
            cur = m.u32(cur + NEXT);
            let hn = m.u32(head + NEXT);
            m.set_u32(s + NEXT, hn);
            m.set_u32(hn + PREV, s);
            m.set_u32(s + PREV, head);
            m.set_u32(head + NEXT, s);
        }
    }
    classify_samples(m, crv);
    Ok(crv)
}

/// [soft 0x084fd0d0 si_validTrimCurve]: `(bbox, comp)`: whether the composite curve makes a
/// usable sample ring in the box; the ring is freed again. The box is passed by value (12
/// words); its doubles at +0, +0x18, +8 and +0x20 are `u0, u1, v0, v1`.
pub fn si_valid_trim_curve(m: &mut Mem, bbox: [u32; 12], comp: u32) -> Result<u32, Halt> {
    let d = |i: usize| f64::from_bits(bbox[i] as u64 | (bbox[i + 1] as u64) << 32);
    let crv = new_sample_crv(m, comp, d(0), d(6), d(2), d(8))?;
    if crv == 0 {
        return Ok(0);
    }
    let ring = m.u32(crv + 4);
    if ring != 0 {
        m.free(ring);
        m.set_u32(crv + 4, 0);
    }
    m.free(crv);
    Ok(1)
}

/// 0x08661e08: two knots closer than this are one (1e-5).
const KNOT_TOL: f64 = 1e-5;

/// [soft 0x08500cd4 MakeBreakVector]: `(samples, n, dir, &count)`: an `n`-word flag array
/// (`SI_calloc(n, 4)`) that marks, for every knot of the direction repeated up to the full
/// multiplicity (order − 1; each knot where the order is 2), the entry of the sampled
/// parameter `samples[]` that lies within 1e-5 of it; `*count` is the number of marks (a
/// mark at entry 0 that comes from order 2 is cleared again and not counted). The direction
/// record: +0 order, +8 point count, +0xc knots, +0x20 closed. Null when the allocation fails.
pub fn make_break_vector(m: &mut Mem, samples: u32, n: i32, dir: u32, count: u32) -> u32 {
    let order = m.i32(dir);
    let closed = m.u16(dir + 0x20) != 0;
    let mult = order.wrapping_sub(1);
    let npts = m.i32(dir + 8);
    let start = if closed { 0 } else { mult };
    let end = if closed { npts.wrapping_sub(1) } else { npts.wrapping_sub(mult) };
    let knots = m.u32(dir + 0xc);
    let b = m.si_calloc(n as u32, 4);
    if b == 0 {
        return 0;
    }
    for i in 0..n.max(0) as u32 {
        m.set_u32(b + 4 * i, 0);
    }
    let tol = x(KNOT_TOL);
    let at = |base: u32, i: i32| base.wrapping_add((i as u32).wrapping_mul(8));
    let (mut run, mut found, mut e) = (1i32, 0i32, 0i32);
    let mut j = start;
    while j < end {
        let k = x(m.f64(at(knots, j)));
        let close = (k - x(m.f64(at(knots, j.wrapping_add(1))))).abs().lt(tol);
        if close || mult == 1 {
            run += 1;
            if run == mult || mult == 1 {
                found += 1;
                while (k - x(m.f64(at(samples, e)))).abs().ge(tol) {
                    e = e.wrapping_add(1);
                }
                m.set_u32(b.wrapping_add((e as u32).wrapping_mul(4)), 1);
                if mult == 1 && e == 0 {
                    m.set_u32(b, 0);
                    found -= 1;
                }
                run = 1;
            }
        } else {
            run = 1;
        }
        j = j.wrapping_add(1);
    }
    m.set_i32(count, found);
    b
}

/// 0x08661e10: the distance within which a sample is "at" a break parameter (1e-5).
const BREAK_TOL: f64 = 1e-5;

/// [soft 0x08500e30 AddUKnotsToSampleCrvs]: `(params, n, flags, k, crvs)`: the first `k`
/// entries of `flags[0..n]` equal to 1 name the break parameters `params[index]` (increasing).
/// Every ring of samples walks along them, two cursors (the last break passed, the next one),
/// and where an edge crosses a break a sample is cut in with the v interpolated linearly (the
/// edge's own samples within 1e-5 of the break are marked 0x400 instead); a ring runs
/// forwards past breaks above and backwards past breaks below its current sample. 1 on
/// success, 0 when an allocation fails.
pub fn add_u_knots_to_sample_crvs(m: &mut Mem, params: u32, n: i32, flags: u32, k: i32, crvs: u32) -> u32 {
    let ks = m.si_calloc(k as u32, 4);
    if ks == 0 {
        return 0;
    }
    let (mut j, mut f) = (0i32, 0i32);
    while j < n && f < k {
        if m.i32(flags.wrapping_add((j as u32).wrapping_mul(4))) == 1 {
            m.set_i32(ks.wrapping_add((f as u32).wrapping_mul(4)), j);
            f += 1;
        }
        j += 1;
    }
    let e = x(BREAK_TOL);
    let p = |m: &Mem, i: i32| -> F80 {
        let idx = m.i32(ks.wrapping_add((i as u32).wrapping_mul(4)));
        x(m.f64(params.wrapping_add((idx as u32).wrapping_mul(8))))
    };
    // the two registers the original keeps across rings
    let (mut lv, mut hv) = (F80::ZERO, F80::ZERO);
    let mut crv = crvs;
    while crv != 0 {
        let head = m.u32(crv + 4);
        let mut hi = 0i32;
        if hi < k {
            let hu = x(m.f64(head + U));
            if (p(m, 0) + e).lt(hu) {
                loop {
                    hi += 1;
                    if hi >= k || !(p(m, hi) + e).lt(hu) {
                        break;
                    }
                }
            }
        }
        let mut lo = hi - 1;
        if lo >= 0 {
            lv = p(m, lo);
        }
        if hi < k {
            hv = p(m, hi);
        }
        let mut cur = head;
        loop {
            let nx = m.u32(cur);
            let (u, v) = (u_of(m, cur), v_of(m, cur));
            let (nu, nv) = (u_of(m, nx), v_of(m, nx));
            let mut flagged = false;
            // the new sample at `at` on the edge cur..nx
            let make = |m: &mut Mem, at: F80| -> u32 {
                let val = v + (nv - v) * (at - u) / (nu - u);
                let s = new_uv_sample(m, at.to_f64(), val.to_f64(), 0);
                if s != 0 {
                    let nxt = m.u32(cur + NEXT);
                    m.set_u32(s + NEXT, nxt);
                    m.set_u32(nxt + PREV, s);
                    m.set_u32(s + PREV, cur);
                    m.set_u32(cur + NEXT, s);
                    classify_one_sample(m, s);
                }
                s
            };
            let or400 = |m: &mut Mem| {
                let fl = m.u32(cur + FLAGS);
                m.set_u32(cur + FLAGS, fl | 0x400);
            };
            if hi < k {
                if u.lt(hv - e) {
                    if nu.gt(hv + e) {
                        flagged = true;
                        if make(m, hv) == 0 {
                            m.free(ks);
                            return 0;
                        }
                        lo = hi;
                        hi += 1;
                        lv = hv;
                        if hi < k {
                            hv = p(m, hi);
                        }
                    }
                } else if (hv + e).gt(u) {
                    or400(m);
                    if nu.gt(hv + e) {
                        flagged = true;
                        lo = hi;
                        hi += 1;
                        lv = hv;
                        if hi < k {
                            let q = p(m, hi);
                            hv = q;
                            if (q + e).lt(nu) {
                                if make(m, q) == 0 {
                                    m.free(ks);
                                    return 0;
                                }
                                lo = hi;
                                hi += 1;
                                lv = q;
                                if hi < k {
                                    hv = p(m, hi);
                                }
                            }
                        }
                    }
                }
            }
            if lo >= 0 && !flagged {
                if u.gt(lv + e) {
                    if nu.lt(lv - e) {
                        if make(m, lv) == 0 {
                            m.free(ks);
                            return 0;
                        }
                        hi = lo;
                        lo -= 1;
                        hv = lv;
                        if lo >= 0 {
                            lv = p(m, lo);
                        }
                    }
                } else if (lv - e).lt(u) {
                    or400(m);
                    if nu.lt(lv - e) {
                        hi = lo;
                        lo -= 1;
                        hv = lv;
                        if lo >= 0 {
                            let q = p(m, lo);
                            lv = q;
                            if nu.lt(q - e) {
                                if make(m, q) == 0 {
                                    m.free(ks);
                                    return 0;
                                }
                                hi = lo;
                                lo -= 1;
                                hv = q;
                                if lo >= 0 {
                                    lv = p(m, lo);
                                }
                            }
                        }
                    }
                }
            }
            cur = m.u32(cur + NEXT);
            if cur == head {
                break;
            }
        }
        crv = m.u32(crv);
    }
    m.free(ks);
    1
}

/// 0x08661db0: how far a sample's v must be from the one before (behind a run of marked
/// samples) to count as a turning point (0.005); 0x08661db8: two critical points closer than
/// 1e-5 in the sorted coordinate are one.
const CRIT_V: f64 = 0.005;
const CRIT_SAME: f64 = 1e-5;

/// [soft 0x084fe24c FindUCritcalPoints]: `(crvs, &head, by_v)`: collects, into a new ring
/// whose head pointer goes to `*head`, a copy of every sample of the sample curves that has
/// flag 1, and, when `by_v` is set, the ends of runs of samples with flag 0x80 whose v differs
/// by more than 0.005 from the neighbouring sample outside the run (the one after the run
/// for a run's first, the one before it for its last). The ring is sorted by u (v when
/// `by_v`) and a point within 1e-5 of one already there is dropped. 1, or 0 when an
/// allocation fails.
pub fn find_u_critical_points(m: &mut Mem, crvs: u32, head_out: u32, by_v: u16) -> u32 {
    let mut head = m.u32(head_out);
    let r = find_critical(m, crvs, &mut head, by_v);
    m.set_u32(head_out, head);
    r
}

/// [`find_u_critical_points`] with the head pointer held by the caller.
fn find_critical(m: &mut Mem, crvs: u32, head_out: &mut u32, by_v: u16) -> u32 {
    let mut acc = 0u32;
    let c = x(CRIT_V);
    let eps = x(CRIT_SAME);
    let mut crv = crvs;
    while crv != 0 {
        let first = m.u32(crv + 4);
        let mut s = first;
        loop {
            let fl = m.u32(s + FLAGS);
            let nx = m.u32(s + NEXT);
            let neg = |m: &Mem, p: u32| (m.u32(p + FLAGS) & 0x80) != 0;
            let mut add = fl & 1 != 0;
            if !add && by_v != 0 {
                let far = |m: &Mem, other: u32| {
                    let d = v_of(m, s) - v_of(m, other);
                    if d.lt(F80::ZERO) {
                        (-d).gt(c)
                    } else {
                        c.lt(d)
                    }
                };
                if !neg(m, nx) && neg(m, s) == false {
                    // neither marked: nothing
                } else if neg(m, nx) && !neg(m, s) {
                    let mut b = nx;
                    loop {
                        b = m.u32(b + NEXT);
                        if !neg(m, b) {
                            break;
                        }
                    }
                    add = far(m, b);
                } else if neg(m, s) && !neg(m, nx) {
                    let mut b = m.u32(s + PREV);
                    while neg(m, b) {
                        b = m.u32(b + PREV);
                    }
                    add = far(m, b);
                }
            }
            if add {
                let (su, sv) = (m.f64(s + U), m.f64(s + V));
                if acc == 0 {
                    let n = new_uv_sample(m, su, sv, fl as i32);
                    if n == 0 {
                        return 0;
                    }
                    m.set_u32(n + PREV, n);
                    m.set_u32(n + NEXT, n);
                    acc = n;
                    *head_out = n;
                } else {
                    let head = *head_out;
                    let mut b = head;
                    let key = |m: &Mem, p: u32| if by_v != 0 { v_of(m, p) } else { u_of(m, p) };
                    let mine = key(m, s);
                    let mut insert = true;
                    loop {
                        let t = key(m, b);
                        if mine.lt(t - eps) {
                            break;
                        }
                        if (t + eps).gt(mine) {
                            insert = false;
                            break;
                        }
                        b = m.u32(b + NEXT);
                        if b == head {
                            break;
                        }
                    }
                    if insert {
                        let n = new_uv_sample(m, su, sv, fl as i32);
                        if n == 0 {
                            return 0;
                        }
                        let bp = m.u32(b + PREV);
                        m.set_u32(n + PREV, bp);
                        m.set_u32(bp + NEXT, n);
                        m.set_u32(n + NEXT, b);
                        m.set_u32(b + PREV, n);
                        if b == *head_out && mine.lt(key(m, b)) {
                            *head_out = n;
                        }
                    }
                }
            }
            s = m.u32(s + NEXT);
            if s == m.u32(crv + 4) {
                break;
            }
        }
        crv = m.u32(crv);
    }
    1
}

/// One pass of [`add_u_critical_points_to_samplings`]: merges the coordinate (`off`: U or V)
/// of the sorted ring of critical points `head` into the sorted array `*arr_cell`
/// (`*n_cell` entries): the points the array lacks (not within 1e-5 of an entry) are added.
/// False when the allocation fails.
fn merge_critical(m: &mut Mem, head: u32, off: u32, arr_cell: u32, n_cell: u32) -> bool {
    let e = x(BREAK_TOL);
    let n = m.i32(n_cell);
    let arr = m.u32(arr_cell);
    let at = |base: u32, i: i32| base.wrapping_add((i as u32).wrapping_mul(8));
    let mut count = 0i32;
    let mut cur = head;
    let mut esi = 0i32;
    'count: while esi < n {
        let c = x(m.f64(cur + off));
        let a = x(m.f64(at(arr, esi)));
        if a.gt(c - e) {
            if (c + e).lt(a) {
                while a.gt(x(m.f64(cur + off))) {
                    count += 1;
                    cur = m.u32(cur + NEXT);
                    if cur == head {
                        break 'count;
                    }
                }
            } else {
                cur = m.u32(cur + NEXT);
                if cur == head {
                    break 'count;
                }
            }
        }
        esi += 1;
    }
    if count == 0 {
        return true;
    }
    let out = m.si_calloc(count.wrapping_add(n) as u32, 8);
    if out == 0 {
        return false;
    }
    let (mut flag, mut ob, mut cur, mut esi) = (false, 0i32, head, 0i32);
    while esi < m.i32(n_cell) {
        let a = x(m.f64(at(m.u32(arr_cell), esi)));
        let c = x(m.f64(cur + off));
        let lo = c - e;
        if a.le(lo) || flag {
            m.set_f64(at(out, ob), a.to_f64());
            ob += 1;
        } else if a.gt(lo) {
            if !(c + e).lt(a) {
                m.set_f64(at(out, ob), a.to_f64());
                cur = m.u32(cur + NEXT);
                ob += 1;
                if cur == head {
                    flag = true;
                }
            } else {
                loop {
                    let cv = x(m.f64(cur + off));
                    if !cv.lt(a) {
                        break;
                    }
                    m.set_f64(at(out, ob), cv.to_f64());
                    cur = m.u32(cur + NEXT);
                    ob += 1;
                    if cur == head {
                        flag = true;
                        break;
                    }
                }
                esi -= 1;
            }
        }
        esi += 1;
    }
    let old = m.u32(arr_cell);
    if old != 0 {
        m.free(old);
        m.set_u32(arr_cell, 0);
    }
    m.set_u32(arr_cell, out);
    m.set_i32(n_cell, ob);
    true
}

/// [soft 0x084fe4a8 AddUCritcalPointsToSamplings]: `(crvs, &nu, &us, &nv, &vs)`: the critical
/// points of the sample curves ([`find_u_critical_points`], by v, then by u) are merged into
/// the sorted v array and then the sorted u array of the samplings. A pass that finds no
/// critical points ends the routine with 1 (the pass after it is not done). 0 when an
/// allocation fails.
pub fn add_u_critical_points_to_samplings(m: &mut Mem, crvs: u32, nu: u32, us: u32, nv: u32, vs: u32) -> u32 {
    let mut head = 0u32;
    if find_critical(m, crvs, &mut head, 1) == 0 {
        free_sample_list(m, head);
        return 0;
    }
    if head == 0 {
        return 1;
    }
    if !merge_critical(m, head, V, vs, nv) {
        free_sample_list(m, head);
        return 0;
    }
    free_sample_list(m, head);
    head = 0;
    if find_critical(m, crvs, &mut head, 0) == 0 {
        free_sample_list(m, head);
        return 0;
    }
    if head == 0 {
        return 1;
    }
    if !merge_critical(m, head, U, us, nu) {
        free_sample_list(m, head);
        return 0;
    }
    free_sample_list(m, head);
    1
}

/// 0x08661de8: the margin of the horizontal-sample tests (1e-5).
const HORIZ_TOL: f64 = 1e-5;

/// The test both passes of [`add_horizontal_samples`] apply: `a + 1e-5 < x < b − 1e-5`, sums
/// kept in the register, an unordered pair failing both.
fn between(a: F80, b: F80, xv: F80) -> bool {
    let c = x(HORIZ_TOL);
    (a + c).lt(xv) && xv.lt(b - c)
}

/// [soft 0x084ffc58 AddHorizontalSamples]: `(strips, n, ys, nx, xs, marks)`: for every strip
/// `i` with segments, and each of its segments, the horizontal lines `xs[0 .. nx]` (parameters
/// along the strip direction, ascending) are inserted into the segment's ring of samples.
/// Two walks, each over the ring from the segment's first sample:
///
/// - forward (next links): over consecutive pairs `(p, q)` that both carry 0x40 (an edge on
///   the strip's lower side, at `ys[i]`), a line `x` with `p.u + 1e-5 < x < q.u − 1e-5`
///   adds a sample `(x, ys[i])` with flags 0x40 after `p` and, where `p` has a mate, one
///   with 0x20 before the mate, the two made mates; the walk then goes on from the new
///   sample. A line at or beyond `q.u` moves the pair on; other pairs are passed over;
/// - backward (previous links): over samples without a mate that carry 0x20 (the upper
///   side, at `ys[i + 1]`), the same test adds a sample with flags 0x20 after `q` (the
///   previous one).
///
/// A new sample gets 0x400 when `marks` is not null and `marks[line] > 0`. 0 when an
/// allocation fails (what was done stays), else 1.
pub fn add_horizontal_samples(m: &mut Mem, strips: u32, n: i32, ys: u32, nx: i32, xs: u32, marks: u32) -> u32 {
    let at8 = |b: u32, i: i32| b.wrapping_add((i as u32).wrapping_mul(8));
    let mark = |m: &mut Mem, s: u32, line: i32| {
        if marks != 0 && m.i32(marks.wrapping_add((line as u32).wrapping_mul(4))) > 0 {
            let f = m.u32(s + FLAGS);
            m.set_u32(s + FLAGS, f | 0x400);
        }
    };
    let mut i = 0i32;
    while i < n {
        let rec = strips.wrapping_add((i as u32).wrapping_mul(12));
        let head = m.u32(rec + 8);
        if head != 0 {
            let y = m.f64(at8(ys, i));
            let mut seg = head;
            loop {
                // forward walk
                let mut k = 0i32;
                let p4 = m.u32(seg + 8);
                let mut p7 = p4;
                let mut p8 = m.u32(p4 + NEXT);
                while k < nx {
                    let f7 = m.u32(p7 + FLAGS);
                    let f8 = m.u32(p8 + FLAGS);
                    let stop;
                    if f7 & 0x40 == 0 || f8 & 0x40 == 0 {
                        p7 = p8;
                        p8 = m.u32(p7 + NEXT);
                        while p7 != p4 && (m.u32(p7 + FLAGS) & 0x40 == 0 || m.u32(p8 + FLAGS) & 0x40 == 0) {
                            p7 = p8;
                            p8 = m.u32(p7 + NEXT);
                        }
                        k -= 1;
                        stop = p7 == p4;
                    } else {
                        let xv = m.f64(at8(xs, k));
                        if between(u_of(m, p7), u_of(m, p8), x(xv)) {
                            let s = new_uv_sample(m, xv, y, 0x40);
                            if s == 0 {
                                return 0;
                            }
                            mark(m, s, k);
                            let nx7 = m.u32(p7 + NEXT);
                            m.set_u32(s + NEXT, nx7);
                            m.set_u32(nx7 + PREV, s);
                            m.set_u32(s + PREV, p7);
                            m.set_u32(p7 + NEXT, s);
                            let mate = m.u32(p7 + MATE);
                            if mate != 0 {
                                let s2 = new_uv_sample(m, xv, y, 0x20);
                                if s2 == 0 {
                                    return 0;
                                }
                                mark(m, s2, k);
                                let mp = m.u32(mate + PREV);
                                m.set_u32(s2 + PREV, mp);
                                m.set_u32(mp + NEXT, s2);
                                m.set_u32(s2 + NEXT, mate);
                                m.set_u32(mate + PREV, s2);
                                let sn = m.u32(p7 + NEXT);
                                m.set_u32(sn + MATE, s2);
                                m.set_u32(s2 + MATE, sn);
                            }
                            p7 = m.u32(p7 + NEXT);
                            stop = p7 == p4;
                        } else if u_of(m, p8).le(x(xv)) {
                            p7 = p8;
                            p8 = m.u32(p7 + NEXT);
                            k -= 1;
                            stop = p7 == p4;
                        } else {
                            stop = false;
                        }
                    }
                    if stop {
                        break;
                    }
                    k += 1;
                }
                // backward walk
                k = 0;
                let p4 = m.u32(seg + 8);
                let mut p7 = p4;
                let mut p8 = m.u32(p4 + PREV);
                while k < nx {
                    let stop;
                    if m.u32(p7 + MATE) == 0 && m.u32(p7 + FLAGS) & 0x20 != 0 {
                        let xv = m.f64(at8(xs, k));
                        if between(u_of(m, p7), u_of(m, p8), x(xv)) {
                            let s = new_uv_sample(m, xv, m.f64(at8(ys, i + 1)), 0x20);
                            if s == 0 {
                                return 0;
                            }
                            mark(m, s, k);
                            let n8 = m.u32(p8 + NEXT);
                            m.set_u32(s + NEXT, n8);
                            m.set_u32(n8 + PREV, s);
                            m.set_u32(s + PREV, p8);
                            m.set_u32(p8 + NEXT, s);
                            p7 = m.u32(p7 + PREV);
                            stop = p7 == p4;
                        } else if u_of(m, p8).le(x(xv)) {
                            p7 = p8;
                            p8 = m.u32(p7 + PREV);
                            k -= 1;
                            stop = p7 == p4;
                        } else {
                            stop = false;
                        }
                    } else {
                        p7 = p8;
                        p8 = m.u32(p7 + PREV);
                        while p7 != p4 && (m.u32(p7 + MATE) != 0 || m.u32(p7 + FLAGS) & 0x20 == 0) {
                            p7 = p8;
                            p8 = m.u32(p7 + PREV);
                        }
                        k -= 1;
                        stop = p7 == p4;
                    }
                    if stop {
                        break;
                    }
                    k += 1;
                }
                seg = m.u32(seg + NEXT);
                if seg == head {
                    break;
                }
            }
        }
        i += 1;
    }
    1
}

/// [soft 0x08501410 AddUSamplesFullMultiplicity]: `(strips, n)`: for every segment of every
/// strip, the samples that carry 0x400 (a break, set by [`add_u_knots_to_sample_crvs`] and
/// [`add_horizontal_samples`]) are looked at with their next sample `b` and previous `c`
/// (`b` and the flags read before the sample is touched; `u` below is the coordinate along
/// the strip, the tolerance 1e-5, sums and differences kept in the register):
///
/// - a sample with 0x20 and a mate: if `u < b.u − tol` and `u < c.u − tol` it loses 0x400;
/// - else, when `c.u + tol < u < b.u − tol` (the sample lies between its neighbours) or
///   `b.u + tol < u < c.u − tol` (reversed), a copy with the same `u`, `v` and flags is
///   inserted after it; the copy keeps 0x400 where `u + tol < c.u` and the sample loses it
///   (else the copy loses it); and a sample with 0x40 and a mate gets a 0x20 partner before
///   the mate (made its mate's new neighbour), unless the mate lies below both its
///   neighbours' `u − tol`;
/// - else, when `u < b.u − tol` and `u < c.u − tol`, the sample loses 0x400 and, with 0x40
///   and a mate, gets a new 0x20 partner that replaces the old mate (whose mate link is
///   cleared).
///
/// 0 when an allocation fails, else 1.
pub fn add_u_samples_full_multiplicity(m: &mut Mem, strips: u32, n: i32) -> u32 {
    let c = x(HORIZ_TOL);
    let mut i = 0i32;
    while i < n {
        let head = m.u32(strips.wrapping_add((i as u32).wrapping_mul(12)) + 8);
        if head != 0 {
            let mut seg = head;
            loop {
                let first = m.u32(seg + 8);
                let mut a = first;
                loop {
                    let b = m.u32(a + NEXT);
                    let fl = m.u32(a + FLAGS);
                    if fl & 0x400 != 0 {
                        let ua = u_of(m, a);
                        let ub = u_of(m, b);
                        let prev = m.u32(a + PREV);
                        let up = u_of(m, prev);
                        if fl & 0x20 != 0 && m.u32(a + MATE) != 0 {
                            if ua.lt(ub - c) && ua.lt(up - c) {
                                m.set_u32(a + FLAGS, fl & !0x400);
                            }
                        } else if (ua.lt(ub - c) && (up + c).lt(ua)) || ((ub + c).lt(ua) && ua.lt(up - c)) {
                            let s = new_uv_sample(m, ua.to_f64(), m.f64(a + V), fl as i32);
                            if s == 0 {
                                return 0;
                            }
                            m.set_u32(s + NEXT, b);
                            m.set_u32(b + PREV, s);
                            m.set_u32(s + PREV, a);
                            let prev = m.u32(a + PREV);
                            m.set_u32(a + NEXT, s);
                            if (u_of(m, a) + c).lt(u_of(m, prev)) {
                                let f = m.u32(a + FLAGS);
                                m.set_u32(a + FLAGS, f & !0x400);
                                let g = m.u32(s + FLAGS);
                                m.set_u32(s + FLAGS, g | 0x400);
                            } else {
                                let g = m.u32(s + FLAGS);
                                m.set_u32(s + FLAGS, g & !0x400);
                            }
                            let mate = m.u32(a + MATE);
                            if m.u32(a + FLAGS) & 0x40 != 0 && mate != 0 {
                                let um = u_of(m, mate);
                                let mn = m.u32(mate + NEXT);
                                let mp = m.u32(mate + PREV);
                                let skip = um.lt(u_of(m, mn) - c) && um.lt(u_of(m, mp) - c);
                                if !skip {
                                    let s2 = new_uv_sample(m, m.f64(a + U), m.f64(a + V), 0x20);
                                    if s2 == 0 {
                                        return 0;
                                    }
                                    let mp = m.u32(mate + PREV);
                                    m.set_u32(s2 + PREV, mp);
                                    m.set_u32(mp + NEXT, s2);
                                    m.set_u32(s2 + NEXT, mate);
                                    m.set_u32(mate + PREV, s2);
                                    let an = m.u32(a + NEXT);
                                    m.set_u32(an + MATE, s2);
                                    m.set_u32(s2 + MATE, an);
                                }
                            }
                        } else if ua.lt(ub - c) && ua.lt(up - c) {
                            m.set_u32(a + FLAGS, fl & !0x400);
                            let mate = m.u32(a + MATE);
                            if fl & 0x40 != 0 && mate != 0 {
                                let s2 = new_uv_sample(m, m.f64(a + U), m.f64(a + V), 0x20);
                                if s2 == 0 {
                                    return 0;
                                }
                                let mate = m.u32(a + MATE);
                                let mp = m.u32(mate + PREV);
                                m.set_u32(s2 + PREV, mp);
                                m.set_u32(mp + NEXT, s2);
                                m.set_u32(s2 + NEXT, mate);
                                m.set_u32(mate + PREV, s2);
                                m.set_u32(mate + MATE, 0);
                                m.set_u32(a + MATE, s2);
                                m.set_u32(s2 + MATE, a);
                            }
                        }
                    }
                    a = b;
                    if a == first {
                        break;
                    }
                }
                seg = m.u32(seg + NEXT);
                if seg == head {
                    break;
                }
            }
        }
        i += 1;
    }
    1
}

/// [soft 0x084ff8e8 SortedSegmentInsert]: `(seg, strip)`: puts the segment `seg` (+8 first
/// sample, +0xc last sample, +0x10 flags: 1 and 4 mark a segment spanning a whole strip
/// side, 2 and 8 the ends of its sample run; their meaning is only read from this ordering)
/// into the ring of the strip's segments (+8), before the first segment `c` it must precede,
/// at the end when there is none, and makes it the head when that is the first place. The
/// test depends on the flags of the segment and of `c`; `u` is the samples' first
/// coordinate, `min`/`max` run over a segment's first and last sample:
///
/// - `seg` has 1 and 4: a `c` without both 1 and 4 stops it when it has 1 and 8 and 2
///   (`c.flags & 10 == 10`), or when `max(seg) <= u(end of c)` (its last, or its first
///   when `c` has 1); a `c` with both stops it when `max(seg) <= min(c)`, or when
///   `min(seg) <= min(c)` and `max(c) <= max(seg)`;
/// - else `seg` has `flags & 10 == 10`: a `c` with `& 10 == 10` stops it when
///   `max(c) <= min(seg)`, or when `max(c) <= max(seg)` and `min(seg) <= min(c)`;
/// - else a `c` with 1 and 4 stops it when the end of `seg` (its first with 1, else last)
///   `<= min(c)`; a `c` with `& 10 == 10` stops it; otherwise when the end of `seg` (as
///   before) `<= u(end of c)` (its first with 1, else last) and `u(seg end 2) <= u(c end 2)`
///   (the first with 2, else the last).
///
/// The comparisons are on stored doubles; a NaN would be treated as the decompiled
/// flag tests do only for the plain orderings (none is made by the callers).
pub fn sorted_segment_insert(m: &mut Mem, seg: u32, strip: u32) {
    let head = m.u32(strip + 8);
    let mut c = head;
    let mut count = 0i32;
    let sf = m.u32(seg + 0x10);
    let uf = |m: &Mem, s: u32| m.f64(m.u32(s + 8) + U);
    let ul = |m: &Mem, s: u32| m.f64(m.u32(s + 0xc) + U);
    let min = |m: &Mem, s: u32| {
        let (a, b) = (uf(m, s), ul(m, s));
        if a < b {
            a
        } else {
            b
        }
    };
    let max = |m: &Mem, s: u32| {
        let (a, b) = (uf(m, s), ul(m, s));
        if a > b {
            a
        } else {
            b
        }
    };
    // "end 1" of a segment: its first sample with flag 1, else its last
    let end1 = |m: &Mem, s: u32, f: u32| if f & 1 == 0 { ul(m, s) } else { uf(m, s) };
    let end2 = |m: &Mem, s: u32, f: u32| if f & 2 == 0 { ul(m, s) } else { uf(m, s) };
    loop {
        // bound for rings that never come back to `head` (3 of 300 IntersectVStrips vectors: see finding 0044)
        if count > 100_000 {
            break;
        }
        let cf = m.u32(c + 0x10);
        let stop = if sf & 1 != 0 && sf & 4 != 0 {
            if cf & 1 == 0 || cf & 4 == 0 {
                if cf & 10 == 10 {
                    true
                } else {
                    max(m, seg) <= end1(m, c, cf)
                }
            } else if max(m, seg) <= min(m, c) {
                true
            } else {
                min(m, seg) <= min(m, c) && max(m, c) <= max(m, seg)
            }
        } else if sf & 10 == 10 {
            cf & 10 == 10 && (max(m, c) <= min(m, seg) || (max(m, c) <= max(m, seg) && min(m, seg) <= min(m, c)))
        } else {
            (cf & 1 != 0 && cf & 4 != 0 && end1(m, seg, sf) <= min(m, c))
                || cf & 10 == 10
                || (end1(m, seg, sf) <= end1(m, c, cf) && end2(m, seg, sf) <= end2(m, c, cf))
        };
        if stop {
            break;
        }
        c = m.u32(c + NEXT);
        count += 1;
        if c == head {
            break;
        }
    }
    let cp = m.u32(c + PREV);
    m.set_u32(seg + PREV, cp);
    m.set_u32(cp + NEXT, seg);
    m.set_u32(seg + NEXT, c);
    m.set_u32(c + PREV, seg);
    if count == 0 {
        m.set_u32(strip + 8, seg);
    }
}

/// [soft 0x084fd60c CreateDomainBoundary]: `(nu, us, nv, vs)`: the sample ring of the
/// untrimmed surface's own boundary: a `SampleCrv` (`SI_calloc(1, 8)`) whose ring of
/// `2 × (nu − 2 + nv)` samples (`SI_calloc(1, 0x24)` each, every new one linked before the
/// last made, so the ring runs from the newest to the first, which is the curve's entry) is
/// filled from the newest: along the bottom edge `v = vs[0]` over `us[0 .. nu − 1]`, up the
/// right edge `u = us[nu − 1]` over `vs[0 .. nv − 1]`, back along the top `v = vs[nv − 1]`
/// over `us[nu − 1 .. 1]` descending, and down the left edge `u = us[0]` over `vs[nv − 1 .. 1]`
/// descending. Every coordinate is snapped to the grid of 2e-5 ([`snap`]); flags and index
/// are −1. Then [`classify_samples`]. Null when an allocation fails (the ring made so far
/// and the curve are given back).
pub fn create_domain_boundary(m: &mut Mem, nu: i32, us: u32, nv: i32, vs: u32) -> u32 {
    let total = nu.wrapping_sub(2).wrapping_add(nv).wrapping_mul(2);
    let crv = m.si_calloc(1, 8);
    if crv == 0 {
        return 0;
    }
    m.set_u32(crv, 0);
    let mut last = 0u32;
    let mut k = 0i32;
    while k < total {
        let s = m.si_calloc(1, 0x24);
        if s == 0 {
            if last != 0 {
                free_sample_list(m, last);
            }
            m.free(crv);
            return 0;
        }
        if last == 0 {
            m.set_u32(s + PREV, s);
            m.set_u32(s + NEXT, s);
            m.set_u32(crv + 4, s);
        } else {
            let lp = m.u32(last + PREV);
            m.set_u32(s + PREV, lp);
            m.set_u32(lp + NEXT, s);
            m.set_u32(s + NEXT, last);
            m.set_u32(last + PREV, s);
        }
        k += 1;
        last = s;
    }
    let at8 = |b: u32, i: i32| b.wrapping_add((i as u32).wrapping_mul(8));
    let mut s = last;
    let nu1 = nu.wrapping_sub(1);
    let nv1 = nv.wrapping_sub(1);
    let put = |m: &mut Mem, s: u32, u: Option<f64>, v: Option<f64>| {
        if let Some(v) = v {
            m.set_f64(s + V, snap(v).to_f64());
        }
        m.set_i32(s + FLAGS, -1);
        m.set_i32(s + INDEX, -1);
        if let Some(u) = u {
            m.set_f64(s + U, snap(u).to_f64());
        }
    };
    let v0 = m.f64(vs);
    let mut i = 0;
    while i < nu1 {
        let u = m.f64(at8(us, i));
        put(m, s, Some(u), Some(v0));
        s = m.u32(s + NEXT);
        i += 1;
    }
    let ulast = m.f64(at8(us, nu1));
    let mut i = 0;
    while i < nv1 {
        let v = m.f64(at8(vs, i));
        put(m, s, Some(ulast), Some(v));
        s = m.u32(s + NEXT);
        i += 1;
    }
    let vlast = m.f64(at8(vs, nv1));
    let mut i = 0;
    while i < nu1 {
        let u = m.f64(at8(us, nu1.wrapping_sub(i)));
        put(m, s, Some(u), Some(vlast));
        s = m.u32(s + NEXT);
        i += 1;
    }
    let u0 = m.f64(us);
    let mut i = 0;
    while i < nv1 {
        let v = m.f64(at8(vs, nv1.wrapping_sub(i)));
        put(m, s, Some(u0), Some(v));
        s = m.u32(s + NEXT);
        i += 1;
    }
    classify_samples(m, crv);
    crv
}

/// 0x08661dd8 / 0x08661de0: the starting minimum of the lowest-sample search of
/// `EncloseTrimRegions` (u) and its v.
const FAR_U: f64 = 9999999999.0;
const FAR_V: f64 = -9999999999.0;

/// `SampleCrvSegment` field offsets.
const SG_S0: u32 = 8;
const SG_S1: u32 = 0xc;
const SG_FLAGS: u32 = 0x10;

/// The u of the first / last sample of a segment.
fn seg_u0(m: &Mem, g: u32) -> f64 {
    m.f64(m.u32(g + SG_S0) + U)
}
fn seg_u1(m: &Mem, g: u32) -> f64 {
    m.f64(m.u32(g + SG_S1) + U)
}

/// The stop test of the inner scan of `EncloseTrimRegions`: whether the segment `c` ends the
/// run that follows `b` (flags `bf`, `t` the first sample of `b`).
fn enclose_stop(m: &Mem, b: u32, bf: u32, t: u32, c: u32, cf: u32) -> bool {
    let (b1, tu) = (seg_u1(m, b), m.f64(t + U));
    let (c0, c1) = (seg_u0(m, c), seg_u1(m, c));
    if bf & 4 == 0 {
        if bf & 8 == 0 {
            return false;
        }
        if cf & 0xa == 0xa {
            let mx = if c0 > c1 { c0 } else { c1 };
            if mx <= b1 {
                return true;
            }
        }
        if cf & 2 != 0 && c0 <= b1 && c1 <= tu {
            return true;
        }
        cf & 8 != 0 && c1 <= b1
    } else {
        if cf & 1 != 0 {
            if cf & 4 != 0 {
                let mn = if c0 < c1 { c0 } else { c1 };
                if mn >= b1 {
                    return true;
                }
            }
            if c0 >= b1 && c1 >= tu {
                return true;
            }
        }
        cf & 4 != 0 && c1 >= b1
    }
}

#[derive(PartialEq, Clone, Copy)]
enum EncSt {
    Skip,
    S430,
    S460,
    S4ab,
}

/// [soft 0x084ff3b4 EncloseTrimRegions]: `(strips, n)`. Per strip, over its ring of segments
/// `a`: while the segment after `a` (`b`) is a continuation of it (flag tests on 0x10, 0x1,
/// 0x2, 0x12 and 0x20 and u comparisons against the 1e-5 tolerance decide; a scan of the
/// following segments `c` stops at the first that does not overlap), `a`'s last sample is
/// joined to `b`'s first, `a` takes `b`'s last sample and `b` is unlinked and freed. Then
/// each segment's sample ring is closed (`s1.next = s0`) and its entry moved to the sample
/// of least u (a u within 1e-5 of the least: the larger v), `s1` being its previous. Last,
/// segments whose first and last sample are the same are unlinked and freed.
pub fn enclose_trim_regions(m: &mut Mem, strips: u32, n: i32) {
    let tol = x(TOL2);
    let mut i = 0i32;
    while i < n {
        let rec = strips.wrapping_add((i as u32).wrapping_mul(12));
        let head = m.u32(rec + 8);
        if head != 0 {
            let mut a = head;
            loop {
                let mut b = m.u32(a);
                let c = m.u32(b);
                let af = m.u32(a + SG_FLAGS);
                let mut st = if b == head || m.u32(b + SG_FLAGS) & 0x20 != 0 {
                    EncSt::Skip
                } else if af & 0x10 == 0 {
                    if af & 0x20 != 0 { EncSt::Skip } else { EncSt::S4ab }
                } else if af & 1 == 0 {
                    EncSt::S460
                } else if m.u32(b + SG_FLAGS) & 0x12 == 0x12 {
                    EncSt::Skip
                } else {
                    EncSt::S430
                };
                let mut c = c;
                while st != EncSt::Skip {
                    match st {
                        EncSt::S430 => {
                            let xa = x(seg_u0(m, a));
                            let y = if m.u32(b + SG_FLAGS) & 1 != 0 { seg_u0(m, b) } else { seg_u1(m, b) };
                            st = if (x(y) + tol).gt(xa) { EncSt::Skip } else { EncSt::S4ab };
                        }
                        EncSt::S460 => {
                            let af = m.u32(a + SG_FLAGS);
                            if af & 2 == 0 {
                                st = EncSt::S4ab;
                                continue;
                            }
                            let bf = m.u32(b + SG_FLAGS);
                            if bf & 0x10 != 0 && bf & 1 != 0 {
                                st = EncSt::Skip;
                                continue;
                            }
                            let xa = x(seg_u0(m, a));
                            let y = if bf & 2 != 0 { seg_u0(m, b) } else { seg_u1(m, b) };
                            st = if (x(y) - tol).lt(xa) { EncSt::Skip } else { EncSt::S4ab };
                        }
                        _ => {
                            // 0x084ff4ab: scan the following segments
                            let t = m.u32(b + SG_S0);
                            let bf = m.u32(b + SG_FLAGS);
                            let mut cur = c;
                            if cur != head && m.u32(cur + SG_FLAGS) & 0x20 == 0 {
                                loop {
                                    let cf = m.u32(cur + SG_FLAGS);
                                    if enclose_stop(m, b, bf, t, cur, cf) {
                                        break;
                                    }
                                    cur = m.u32(cur);
                                    if cur == head || m.u32(cur + SG_FLAGS) & 0x20 != 0 {
                                        break;
                                    }
                                }
                            }
                            // 0x084ff629: join b to a, free b
                            let a1 = m.u32(a + SG_S1);
                            m.set_u32(a1, t);
                            let a1 = m.u32(a + SG_S1);
                            m.set_u32(t + PREV, a1);
                            let b1 = m.u32(b + SG_S1);
                            m.set_u32(a + SG_S1, b1);
                            let (bp, bn) = (m.u32(b + 4), m.u32(b));
                            m.set_u32(bp, bn);
                            let bn = m.u32(b);
                            let bp = m.u32(b + 4);
                            m.set_u32(bn + 4, bp);
                            m.free(b);
                            b = cur;
                            c = m.u32(cur);
                            let ecx = m.u32(b + SG_FLAGS);
                            if ecx & 0x10 != 0 || b == head || ecx & 0x20 != 0 {
                                st = EncSt::Skip;
                                continue;
                            }
                            let af = m.u32(a + SG_FLAGS);
                            st = if af & 0x10 == 0 {
                                if af & 0x20 != 0 { EncSt::Skip } else { EncSt::S4ab }
                            } else if af & 1 == 0 {
                                EncSt::S460
                            } else if ecx & 0x12 == 0x12 {
                                EncSt::Skip
                            } else {
                                EncSt::S430
                            };
                        }
                    }
                }
                // 0x084ff698: close the ring, enter it at the sample of least u
                let s0 = m.u32(a + SG_S0);
                let s1 = m.u32(a + SG_S1);
                m.set_u32(s0 + PREV, s1);
                m.set_u32(s1 + NEXT, s0);
                let start = m.u32(a + SG_S0);
                let (mut minu, mut minv) = (FAR_U, FAR_V);
                let mut best = 0u32; // the original's local would keep an older value if nothing were taken; minu starts at 1e10
                let mut p = start;
                loop {
                    let u = m.f64(p + U);
                    let take = if u < minu {
                        true
                    } else {
                        let d = (x(u) - x(minu)).abs();
                        d.lt(tol) && minv < m.f64(p + V)
                    };
                    if take {
                        best = p;
                        minu = u;
                        minv = m.f64(p + V);
                    }
                    p = m.u32(p + NEXT);
                    if p == start {
                        break;
                    }
                }
                m.set_u32(a + SG_S0, best);
                let bp = m.u32(best + PREV);
                m.set_u32(a + SG_S1, bp);
                a = m.u32(a);
                if a == head {
                    break;
                }
            }
            // 0x084ff753: drop segments whose first and last sample coincide
            let mut e = m.u32(rec + 8);
            loop {
                let nx = m.u32(e);
                let esi = if e == nx { 0 } else { nx };
                if m.u32(e + SG_S0) == m.u32(e + SG_S1) {
                    if e == m.u32(rec + 8) {
                        m.set_u32(rec + 8, if e == nx { 0 } else { nx });
                    }
                    if m.u32(rec + 8) != 0 {
                        let (en, ep) = (m.u32(e), m.u32(e + 4));
                        m.set_u32(en + 4, ep);
                        m.set_u32(ep, en);
                    }
                    m.free(e);
                }
                e = esi;
                if e == 0 || e == m.u32(rec + 8) {
                    break;
                }
            }
        }
        i += 1;
    }
}

/// The vertex index of a sample: its own index, or its mate's when it has none (−1).
fn out_index(m: &Mem, s: u32) -> i32 {
    let i = m.i32(s + INDEX);
    if i == -1 { m.i32(m.u32(s + MATE) + INDEX) } else { i }
}

/// [soft 0x0850089c OutputTriangles]: `(strips, n, material, mat_id, ntri, verts, tris,
/// want_idx, idx)`. The `ntri` triangle records at `tris` (0x60 bytes each) are chained
/// (+0x5c next, null in the last), given the short `mat_id` (+0x58) and the material
/// `*material` (+0x4c, null without `material`). Then each strip's segments are walked as
/// monotone polygons: starting at the segment's first sample (the start is moved on one
/// when the first corner has the wrong turn), a triangle is written for the current corner
/// from the vertex numbers of its samples (a sample numbered −1 takes its mate's number):
/// pointers `verts + 0x4c × number` at +0/+4/+8, and with `want_idx` the three numbers at
/// `idx + 12 × k`; `advanceTri` steps on. A segment that did not give `count − 2`
/// triangles is walked a second time from its first 0x40 sample. Running past `ntri`
/// triangles calls `status(text, 2)` ("This model contains invisible triangles.").
pub fn output_triangles(
    m: &mut Mem, strips: u32, n: i32, material: u32, mat_id: i32, ntri: i32, verts: u32, tris: u32, want_idx: i16, idx: u32,
    status: &mut dyn FnMut(&str, i32),
) {
    let mut cnt = 0i32; // local_24: triangles counted so far (may pass ntri by one)
    let mut k = 0i32;
    while k < ntri {
        let rec = tris.wrapping_add((k as u32).wrapping_mul(0x60));
        let nx = if k < ntri - 1 { rec.wrapping_add(0x60) } else { 0 };
        m.set_u32(rec + 0x5c, nx);
        m.set_u16(rec + 0x58, mat_id as u16);
        let mv = if material == 0 { 0 } else { m.u32(material) };
        m.set_u32(rec + 0x4c, mv);
        k += 1;
    }
    let mut ti = 0i32; // local_18: next triangle slot
    let mut i = 0i32;
    while i < n {
        let first = m.u32(strips.wrapping_add((i as u32).wrapping_mul(12)) + 8);
        if first != 0 {
            let mut seg = first;
            loop {
                let (saved_ti, saved_cnt) = (ti, cnt);
                let start0 = m.u32(seg + SG_S0);
                let mut t0 = start0;
                let mut t1 = m.u32(t0 + PREV);
                let mut t2 = m.u32(t0 + NEXT);
                let t2n = m.u32(t2 + NEXT);
                if pt_interior_to_edge(m, t2n, t0, t2) && pt_interior_to_edge(m, t2n, t2, t1) && pt_interior_to_edge(m, t2n, t1, t0) {
                    t1 = t0;
                    t0 = t2;
                    t2 = m.u32(t2 + NEXT);
                }
                let start = t0;
                let mut region = 0i32; // local_30
                let wrote = |m: &mut Mem, t0: u32, t1: u32, t2: u32, ti: &mut i32| {
                    let (a, b, c) = (out_index(m, t0), out_index(m, t2), out_index(m, t1));
                    let o = tris.wrapping_add((*ti as u32).wrapping_mul(0x60));
                    m.set_u32(o, verts.wrapping_add((a as u32).wrapping_mul(0x4c)));
                    m.set_u32(o + 4, verts.wrapping_add((b as u32).wrapping_mul(0x4c)));
                    m.set_u32(o + 8, verts.wrapping_add((c as u32).wrapping_mul(0x4c)));
                    if want_idx != 0 {
                        let q = idx.wrapping_add((*ti as u32).wrapping_mul(12));
                        m.set_i32(q, a);
                        m.set_i32(q + 4, b);
                        m.set_i32(q + 8, c);
                    }
                    *ti += 1;
                };
                loop {
                    cnt += 1;
                    region += 1;
                    if ntri < cnt {
                        status(si_rt::text(0x086d_abc0), 2);
                        break;
                    }
                    wrote(m, t0, t1, t2, &mut ti);
                    if m.u8(t1 + FLAGS + 1) & 4 != 0 && m.u8(t2 + FLAGS + 1) & 4 != 0 {
                        t1 = m.u32(t1 + PREV);
                        t2 = m.u32(t2 + NEXT);
                    }
                    if t1 == m.u32(t2 + NEXT) || m.u8(t2 + FLAGS) & 0x20 != 0 || m.u8(t1 + FLAGS) & 0x40 != 0 {
                        break;
                    }
                    let (f, s2, b2) = advance_tri_core(m, t1, t2);
                    t0 = f;
                    t1 = s2;
                    t2 = b2;
                    if t0 == start {
                        break;
                    }
                }
                if region != m.i32(seg + 0x14) - 2 {
                    let s0 = m.u32(seg + SG_S0);
                    let mut q = s0;
                    loop {
                        if m.u8(q + FLAGS) & 0x40 != 0 {
                            break;
                        }
                        q = m.u32(q + NEXT);
                        if q == s0 {
                            break;
                        }
                    }
                    if q != s0 {
                        let stop = q;
                        t0 = q;
                        t1 = m.u32(q + PREV);
                        t2 = m.u32(q + NEXT);
                        cnt = saved_cnt;
                        ti = saved_ti;
                        loop {
                            cnt += 1;
                            if ntri < cnt {
                                status(si_rt::text(0x086d_abc0), 2);
                                break;
                            }
                            wrote(m, t0, t1, t2, &mut ti);
                            if m.u8(t1 + FLAGS + 1) & 4 != 0 && m.u8(t2 + FLAGS + 1) & 4 != 0 {
                                t1 = m.u32(t1 + PREV);
                                t2 = m.u32(t2 + NEXT);
                            }
                            if t1 == m.u32(t2 + NEXT) || m.u8(t2 + FLAGS) & 0x20 != 0 || m.u8(t1 + FLAGS) & 0x40 != 0 {
                                break;
                            }
                            let (f, s2, b2) = advance_tri_core(m, t1, t2);
                            t0 = f;
                            t1 = s2;
                            t2 = b2;
                            if t0 == stop {
                                break;
                            }
                        }
                    }
                }
                if cnt > ntri {
                    break;
                }
                seg = m.u32(seg);
                if seg == first {
                    break;
                }
            }
        }
        i += 1;
    }
}

/// Links `n` into a sample ring after `p` (the original's four stores, in its order).
fn link_after(m: &mut Mem, p: u32, n: u32) {
    let pn = m.u32(p + NEXT);
    m.set_u32(n + NEXT, pn);
    m.set_u32(pn + PREV, n);
    m.set_u32(n + PREV, p);
    m.set_u32(p + NEXT, n);
}

/// Links `n` into a ring in front of `c`.
fn link_before(m: &mut Mem, c: u32, n: u32) {
    let cp = m.u32(c + PREV);
    m.set_u32(n + PREV, cp);
    m.set_u32(cp + NEXT, n);
    m.set_u32(n + NEXT, c);
    m.set_u32(c + PREV, n);
}

fn set_flag_byte(m: &mut Mem, s: u32, off: u32, bits: u8) {
    let v = m.u8(s + off) | bits;
    m.set_u8(s + off, v);
}

/// A new segment `seg` into the strip record `rec`: the first one becomes the head of its own
/// ring; with `sorted` a later one goes through `SortedSegmentInsert`, without it the tail.
fn seg_into_strip(m: &mut Mem, rec: u32, seg: u32, sorted: bool) {
    let head = m.u32(rec + 8);
    if head == 0 {
        m.set_u32(rec + 8, seg);
        m.set_u32(seg + NEXT, seg);
        let h = m.u32(rec + 8);
        m.set_u32(h + PREV, seg);
    } else if sorted {
        sorted_segment_insert(m, seg, rec);
    } else {
        let tail = m.u32(head + PREV);
        m.set_u32(tail + NEXT, seg);
        let tail = m.u32(head + PREV);
        m.set_u32(seg + PREV, tail);
        m.set_u32(seg + NEXT, head);
        m.set_u32(head + PREV, seg);
    }
}

/// `a + ((vv - av) * (bu - au)) / (bv - av)` in the register, rounded once to `double` at the
/// store into the call's argument (and the register value reloaded for the second call).
fn interp(au: f64, av: f64, bu: f64, bv: f64, vv: f64) -> f64 {
    let t = (x(vv) - x(av)) * (x(bu) - x(au));
    let t = t / (x(bv) - x(av));
    (x(au) + t).to_f64()
}

/// [soft 0x084fe888 IntersectVStrips]: `(strips, n, ys, crvs)`. Cuts every sample ring of
/// `crvs` along the horizontal lines `ys[0 .. n)` (tolerance 1e-5) into pieces that lie in one
/// strip each: new samples are put on the lines (flags 0x20 on the line above... as the
/// original numbers them: 0x40 lower line, 0x20 upper), mates are tied, and each piece
/// becomes a `SampleCrvSegment` of the strip record (`NewSampleCrvSegment`, then
/// `SortedSegmentInsert`). A curve's `ring` is cleared when it is done. Returns 1; 0 when an
/// allocation fails, or after `print` got the "Tessellating trim surface failed" message.
pub fn intersect_vstrips(m: &mut Mem, strips: u32, n: i32, ys: u32, crvs: u32, print: &mut dyn FnMut(&str)) -> u32 {
    const E: f64 = 1e-5; // 0x08661dc8
    let e = x(E);
    let y = |m: &Mem, i: i32| m.f64(ys.wrapping_add((i as u32).wrapping_mul(8)));
    let mut b7 = false; // bVar7
    let mut b8 = false; // bVar8
    let mut unaff = 0i32;
    let mut crv = crvs;
    loop {
        if crv == 0 {
            return 1;
        }
        let s = m.u32(crv + 4);
        let mut j = 0i32;
        if n > 0 {
            let mut d = y(m, 0);
            while (x(d) + e).le(v_of(m, s)) {
                j += 1;
                if j >= n {
                    break;
                }
                d = y(m, j);
            }
        }
        let mut b5;
        if v_of(m, s).le(x(y(m, j)) - e) {
            b5 = false;
            j -= 1;
        } else {
            b5 = true;
            let mut p13 = s;
            let mut p6 = m.u32(s + NEXT);
            while m.u8(p6 + FLAGS) & 0x10 != 0 {
                p13 = p6;
                p6 = m.u32(p6 + NEXT);
            }
            let _ = p13;
            if m.u8(p6 + FLAGS) & 4 != 0 {
                j -= 1;
            }
        }
        let mut l1c = (x(y(m, j + 1)) - e).to_f64();
        let mut l2c = (x(y(m, j + 1)) + e).to_f64();
        let mut l24 = (x(y(m, j)) - e).to_f64();
        let mut l34 = (x(y(m, j)) + e).to_f64();
        let mut c = s;
        if !b5 || m.u8(s + FLAGS) & 0x12 != 0 || m.u8(m.u32(s + NEXT) + FLAGS) & 0x10 != 0 {
            loop {
                let p = m.u32(c + PREV);
                let v = m.f64(p + V);
                if v <= l24 || l2c <= v {
                    break;
                }
                c = p;
                if p == s {
                    break;
                }
            }
        }
        let _ = &mut b5;
        if c == s {
            let pv = m.f64(m.u32(c + PREV) + V);
            if !(pv <= l24 || l2c <= pv) {
                let seg = new_sample_crv_segment(m, c, m.u32(c + PREV));
                if seg == 0 {
                    return 0;
                }
                m.set_u32(seg + SG_FLAGS, 0x20);
                let rec = strips.wrapping_add((j as u32).wrapping_mul(12));
                seg_into_strip(m, rec, seg, false);
                m.set_u32(crv + 4, 0);
                crv = m.u32(crv);
                continue;
            }
        }
        // 0x084fea57
        let v = m.f64(c + V);
        let near_lo = l24 < v && v < l34;
        let near_hi = l1c < v && v < l2c;
        let mut split = false; // the paths that reach 0x084fec77
        let (mut val, mut vv, mut kind2) = (0.0f64, 0.0f64, 0i32);
        if near_lo || near_hi {
            let cp = m.u32(c + PREV);
            if near_hi && m.f64(cp + V) < l24 {
                set_flag_byte(m, c, FLAGS, 0x20);
                vv = y(m, j);
                val = interp(m.f64(c + U), v, m.f64(cp + U), m.f64(cp + V), vv);
                let n1 = new_uv_sample(m, val, vv, 0x20);
                if n1 == 0 {
                    return 0;
                }
                link_before(m, c, n1);
                kind2 = 0x40;
                split = true;
            } else if near_lo && l2c < m.f64(cp + V) {
                set_flag_byte(m, c, FLAGS, 0x40);
                vv = y(m, j + 1);
                val = interp(m.f64(c + U), v, m.f64(cp + U), m.f64(cp + V), vv);
                let n1 = new_uv_sample(m, val, vv, 0x40);
                if n1 == 0 {
                    return 0;
                }
                link_before(m, c, n1);
                kind2 = 0x20;
                split = true;
            } else {
                let v = m.f64(c + V);
                let mut f = 0x20u32;
                if l24 < v && v < l34 {
                    f = 0x40;
                }
                m.set_u32(c + FLAGS, f);
                let cpv = m.f64(m.u32(c + PREV) + V);
                let k = if f & 0x40 == 0 {
                    if cpv < l1c { 0x20 } else { 0x40 }
                } else if l34 < cpv {
                    0x40
                } else {
                    0x20
                };
                let n1 = new_uv_sample(m, m.f64(c + U), m.f64(c + V), k);
                if n1 == 0 {
                    return 0;
                }
                link_before(m, c, n1);
                m.set_u32(c + MATE, n1);
                m.set_u32(n1 + MATE, c);
            }
        } else {
            let cp = m.u32(c + PREV);
            let d2 = m.f64(cp + V);
            let mut uu = j;
            if l2c <= d2 {
                uu = j + 1;
            }
            vv = y(m, uu);
            val = interp(m.f64(c + U), v, m.f64(cp + U), d2, vv);
            let k = if uu == j { 0x20 } else { 0x40 };
            let n1 = new_uv_sample(m, val, vv, k);
            if n1 == 0 {
                return 0;
            }
            link_before(m, c, n1);
            kind2 = if uu == j { 0x40 } else { 0x20 };
            split = true;
        }
        if split {
            let p11 = new_uv_sample(m, val, vv, kind2);
            if p11 == 0 {
                return 0;
            }
            link_before(m, c, p11);
            let pp = m.u32(p11 + PREV);
            m.set_u32(p11 + MATE, pp);
            m.set_u32(pp + MATE, p11);
            c = p11;
        }
        // cut the ring open in front of c
        let cp = m.u32(c + PREV);
        m.set_u32(cp + NEXT, 0);
        m.set_u32(c + PREV, 0);
        let mut j2 = j;
        'pieces: while c != 0 {
            let mut p = c;
            if m.u32(p + NEXT) != 0 {
                // 0x084feda0
                loop {
                    let v = m.f64(p + V);
                    if v <= l24 || !(v < l34) {
                        if l1c < v && v < l2c {
                            set_flag_byte(m, p, FLAGS, 0x20);
                            m.set_f64(p + V, y(m, j2 + 1));
                        }
                    } else {
                        set_flag_byte(m, p, FLAGS, 0x40);
                        m.set_f64(p + V, y(m, j2));
                    }
                    let f = m.u32(p + FLAGS);
                    let nx = m.u32(p + NEXT);
                    let neg = |m: &Mem| {
                        let pr = m.u32(p + PREV);
                        let a = x(m.f64(pr + V)) - x(m.f64(p + V));
                        let b = x(m.f64(nx + U)) - x(m.f64(p + U));
                        let cc = x(m.f64(pr + U)) - x(m.f64(p + U));
                        let d = x(m.f64(nx + V)) - x(m.f64(p + V));
                        (a * b - cc * d).lt(F80::ZERO)
                    };
                    let mut leave = false;
                    if f & 2 == 0 {
                        if f & 0x60 != 0 && f & 0x300 != 0 && neg(m) {
                            b8 = true;
                            leave = true;
                        }
                    } else if f & 0x60 != 0 {
                        if neg(m) {
                            b7 = true;
                            leave = true;
                        }
                    }
                    if leave {
                        break;
                    }
                    let nxt = m.u32(p + NEXT);
                    let yv = m.f64(nxt + V);
                    if yv <= l24 || l2c <= yv {
                        break;
                    }
                    p = nxt;
                    if m.u32(p + NEXT) == 0 {
                        break;
                    }
                }
                // 0x084feeb0
                let nx = m.u32(p + NEXT);
                if nx != 0 {
                    let yv = m.f64(nx + V);
                    let mut uf = m.u32(p + FLAGS);
                    if yv <= l24 || l2c <= yv {
                        unaff = j2;
                        if l2c <= yv {
                            unaff = j2 + 1;
                        }
                    } else {
                        unaff = ((uf >> 5) & 1) as i32 + j2;
                    }
                    if b7 {
                        uf &= !2;
                        m.set_u32(p + FLAGS, uf);
                        let n1 = new_uv_sample(m, m.f64(p + U), m.f64(p + V), uf as i32);
                        if n1 == 0 {
                            return 0;
                        }
                        link_after(m, p, n1);
                        let k = if m.u8(p + FLAGS) & 0x40 != 0 { 0x20 } else { 0x40 };
                        let n2 = new_uv_sample(m, m.f64(p + U), m.f64(p + V), k);
                        if n2 == 0 {
                            return 0;
                        }
                        if m.u8(p + FLAGS + 1) & 4 != 0 {
                            set_flag_byte(m, n2, FLAGS + 1, 4);
                        }
                        if m.u8(p + FLAGS) & 0x20 == 0 {
                            m.set_u32(p + MATE, n2);
                            m.set_u32(n2 + MATE, p);
                        } else {
                            let pn = m.u32(p + NEXT);
                            m.set_u32(pn + MATE, n2);
                            m.set_u32(n2 + MATE, pn);
                        }
                        let sg = new_sample_crv_segment(m, n2, n2);
                        if sg == 0 {
                            return 0;
                        }
                        let idx = if m.u8(p + FLAGS) & 0x40 != 0 { j2 - 1 } else { j2 + 1 };
                        let rec = strips.wrapping_add((idx as u32).wrapping_mul(12));
                        seg_into_strip(m, rec, sg, true);
                    } else if b8 {
                        let v = m.f64(p + V);
                        if v <= l24 || !(v < l34) {
                            set_flag_byte(m, p, FLAGS, 0x20);
                        } else {
                            set_flag_byte(m, p, FLAGS, 0x40);
                        }
                        let f = m.u32(p + FLAGS);
                        unaff = (((f >> 6) ^ 1) & 1) as i32 + j2;
                        let k = if f & 0x40 != 0 { 0x20 } else { 0x40 };
                        let n1 = new_uv_sample(m, m.f64(p + U), m.f64(p + V), k);
                        if n1 == 0 {
                            return 0;
                        }
                        if m.u8(p + FLAGS + 1) & 4 != 0 {
                            set_flag_byte(m, n1, FLAGS + 1, 4);
                        }
                        link_after(m, p, n1);
                        let f = m.u32(p + FLAGS);
                        let mut tie = false;
                        if f & 0x100 == 0 {
                            if f & 0x200 != 0 {
                                let pu = m.f64(m.u32(p + PREV) + U);
                                if pu < m.f64(p + U) {
                                    tie = true;
                                }
                            }
                        } else if m.f64(n1 + U) < m.f64(p + U) {
                            tie = true;
                        } else if f & 0x200 != 0 {
                            // 0x084ff0cd: the same test as above
                            let pu = m.f64(m.u32(p + PREV) + U);
                            if pu < m.f64(p + U) {
                                tie = true;
                            }
                        }
                        if tie {
                            m.set_u32(p + MATE, n1);
                            m.set_u32(n1 + MATE, p);
                        }
                    } else if (uf & 0x40 != 0 && m.f64(nx + V) <= l24) || (uf & 0x20 != 0 && l2c <= m.f64(nx + V)) {
                        let k = if uf & 0x40 != 0 { 0x20 } else { 0x40 };
                        let n1 = new_uv_sample(m, m.f64(p + U), m.f64(p + V), k);
                        if n1 == 0 {
                            return 0;
                        }
                        if m.u8(p + FLAGS + 1) & 4 != 0 {
                            set_flag_byte(m, n1, FLAGS + 1, 4);
                        }
                        link_after(m, p, n1);
                        m.set_u32(p + MATE, n1);
                        m.set_u32(n1 + MATE, p);
                    } else {
                        let vv = y(m, unaff);
                        let val = interp(m.f64(p + U), m.f64(p + V), m.f64(nx + U), m.f64(nx + V), vv);
                        let k = if unaff == j2 { 0x20 } else { 0x40 };
                        let n1 = new_uv_sample(m, val, vv, k);
                        if n1 == 0 {
                            return 0;
                        }
                        link_after(m, p, n1);
                        let k2 = if unaff == j2 { 0x40 } else { 0x20 };
                        let n2 = new_uv_sample(m, val, vv, k2);
                        if n2 == 0 {
                            return 0;
                        }
                        link_after(m, p, n2);
                        let nn = m.u32(n2 + NEXT);
                        m.set_u32(n2 + MATE, nn);
                        m.set_u32(nn + MATE, n2);
                        p = n2;
                    }
                }
            }
            // 0x084ff23e
            let nx = m.u32(p + NEXT);
            if nx != 0 {
                m.set_u32(nx + PREV, 0);
            }
            m.set_u32(p + NEXT, 0);
            let sg = new_sample_crv_segment(m, c, p);
            if sg == 0 {
                return 0;
            }
            let rec = strips.wrapping_add((j2 as u32).wrapping_mul(12));
            seg_into_strip(m, rec, sg, true);
            if nx == 0 {
                break 'pieces;
            }
            let mut jn = j2;
            if !b7 {
                let yv = m.f64(m.u32(nx + NEXT) + V);
                if ((yv <= l24 || l2c <= yv) || b8) && {
                    jn = unaff;
                    unaff == j2
                } {
                    jn = j2 - 1;
                }
                if n <= jn {
                    print(si_rt::text(0x086d_ab80));
                    return 0;
                }
                let (a, b) = if jn < n - 1 { (y(m, jn), y(m, jn + 1)) } else { (y(m, jn), y(m, jn)) };
                l34 = a;
                l2c = b;
                l1c = (x(l2c) - e).to_f64();
                l2c = (x(l2c) + e).to_f64();
                l24 = (x(l34) - e).to_f64();
                l34 = (x(l34) + e).to_f64();
            }
            b7 = false;
            b8 = false;
            j2 = jn;
            c = nx;
        }
        m.set_u32(crv + 4, 0);
        crv = m.u32(crv);
    }
}

/// `SI_nurbsSrfExtractCurve (0x084d5268)` (eval.c; ported, reached through [`Outside`] because the check holds this
/// file to the original with the callee's answer recorded): `(srf, t (double), dir, nd)`, a new iso-curve record.
pub const SI_NURBS_SRF_EXTRACT_CURVE: si_mem::Routine = si_mem::Routine::new("SI_nurbsSrfExtractCurve", 0x084d_5268);
/// `SI_nurbsSrfValue (0x084d6728)`: `(&point[3] result, srf, u, v)`.
pub const SI_NURBS_SRF_VALUE: si_mem::Routine = si_mem::Routine::new("SI_nurbsSrfValue", 0x084d_6728);
/// `SI_nurbsSrfValueAndNormal (0x084d6824)`: `(&point[3] result, srf, u, v, &normal[3], curve a, curve b)`.
pub const SI_NURBS_SRF_VALUE_AND_NORMAL: si_mem::Routine = si_mem::Routine::new("SI_nurbsSrfValueAndNormal", 0x084d_6824);
/// `SI_nurbsCrvDispose (0x084db870)`: `(&curve)`.
pub const SI_NURBS_CRV_DISPOSE: si_mem::Routine = si_mem::Routine::new("SI_nurbsCrvDispose", 0x084d_b870);

fn evp(a: &mut Vec<u32>, v: f64) {
    let b = v.to_bits();
    a.push(b as u32);
    a.push((b >> 32) as u32);
}

/// [soft 0x085000b0 EvaluateVertices]: `(srf, strips, n, out, ys, nx, xs, flg)`: numbers the samples of the `n` strips
/// (`vStrip` +0 and +4 the counts, +8 the ring of segments, each segment's +8 the ring of samples from its first) and
/// writes one vertex record of 0x4c bytes at `out + 0x4c * index` for each: floats `s, t` (`s = 1 - (u - xs[0]) / (xs[nx-1] -
/// xs[0])`, `t = (v - ys[0]) / (ys[n-1] - ys[0])`, each only when the span's inverse exceeds 1e-4 in size, else 1.0), the
/// point `x y z`, the unit normal, the point again at +0x24 (words 9..11) and 0 in word 16; the other words are what the
/// original's frame held. A sample with flag 0x20 and a mate is skipped (its mate is numbered); the others get `index`
/// = the running count. The point and normal come from `SI_nurbsSrfValueAndNormal` at `(u, v)` with the first available
/// iso-curve pair: the strip's own bottom pair for flag 0x20, the previous strip's for flag 0x40, none otherwise; with flag 0x400 `u` is
/// moved back by 0.001 for the normal (and `v` too for a flag-0x20 sample on a strip whose `flg[i + 1]` is positive without
/// curves) and the point is then taken again from `SI_nurbsSrfValue` at the exact `(u, v)`. The iso-curves are extracted
/// from `ys[i + 1]` and `ys[i]` when the strip's counts (times 4) exceed `srf + 4` (the `flg[i + 1] > 0` strip uses `ys[i+1] -
/// 0.001` for the second one), and given back when they are no longer needed; the pair of the upper edge becomes the
/// lower pair of the next strip.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_vertices(
    m: &mut Mem, o: &mut dyn si_mem::Outside, srf: u32, strips: u32, n: i32, out: u32, ys: u32, nx: i32, xs: u32, flg: u32,
) {
    let sp = m.sp();
    let (buf, nrm, vtx, cur) = (m.local(24), m.local(24), m.local(0x4c), m.local(16));
    let (ca, cb, cc, cd) = (cur, cur + 4, cur + 8, cur + 12);
    let dx = (x(1.0) / (x(m.f64(xs.wrapping_add((nx as u32).wrapping_mul(8)).wrapping_sub(8))) - x(m.f64(xs)))).to_f64();
    let dy = (x(1.0) / (x(m.f64(ys.wrapping_add((n as u32).wrapping_mul(8)).wrapping_sub(8))) - x(m.f64(ys)))).to_f64();
    for c in [ca, cb, cc, cd] {
        m.set_u32(c, 0);
    }
    let mut cnt: i32 = 0;
    let eps = 0.001f64;
    let tiny = 1e-4f64;
    let big = |d: f64| x(d).gt(x(tiny)) || x(-d).gt(x(tiny));
    let flg_pos = |m: &Mem, i: i32| flg != 0 && (m.i32(flg.wrapping_add((i as u32).wrapping_add(1).wrapping_mul(4))) > 0);
    let mut i = 0i32;
    while i < n {
        let sb = strips.wrapping_add((i as u32).wrapping_mul(12));
        let head = m.u32(sb + 8);
        if head != 0 {
            let srf4 = m.i32(srf + 4);
            let yi = m.f64(ys.wrapping_add((i as u32).wrapping_mul(8)));
            let yi1 = m.f64(ys.wrapping_add((i as u32).wrapping_add(1).wrapping_mul(8)));
            let ext = |m: &mut Mem, o: &mut dyn si_mem::Outside, t: f64, dir: u32, nd: u32| -> u32 {
                let mut a = vec![srf];
                evp(&mut a, t);
                a.push(dir);
                a.push(nd);
                o.call(m, SI_NURBS_SRF_EXTRACT_CURVE, &a).eax
            };
            if m.i32(sb).wrapping_mul(4) > srf4 {
                let r = ext(m, o, yi1, 1, 0);
                m.set_u32(ca, r);
                let t = if flg_pos(m, i) { (x(yi1) - x(eps)).to_f64() } else { yi1 };
                let r = ext(m, o, t, 1, 1);
                m.set_u32(cb, r);
            }
            if m.u32(cc) == 0 && m.i32(sb + 4).wrapping_mul(4) > srf4 {
                let r = ext(m, o, yi, 1, 0);
                m.set_u32(cc, r);
            }
            if m.u32(cd) == 0 && m.i32(sb + 4).wrapping_mul(4) > srf4 {
                let r = ext(m, o, yi, 1, 1);
                m.set_u32(cd, r);
            }
            let mut seg = head;
            loop {
                let first = m.u32(seg + 8);
                let mut e = first;
                loop {
                    let f = m.u32(e + 0x1c);
                    if !(f & 0x20 != 0 && m.u32(e + 8) != 0) {
                        m.set_u32(e + 0x20, cnt as u32);
                        let (u, v) = (m.f64(e + 0xc), m.f64(e + 0x14));
                        let svn = |m: &mut Mem, o: &mut dyn si_mem::Outside, u: f64, v: f64, a: u32, b: u32| {
                            let mut args = vec![buf, srf];
                            evp(&mut args, u);
                            evp(&mut args, v);
                            args.extend_from_slice(&[nrm, a, b]);
                            o.call(m, SI_NURBS_SRF_VALUE_AND_NORMAL, &args);
                        };
                        let exact = |m: &mut Mem, o: &mut dyn si_mem::Outside| {
                            let mut args = vec![buf, srf];
                            evp(&mut args, u);
                            evp(&mut args, v);
                            o.call(m, SI_NURBS_SRF_VALUE, &args);
                        };
                        let um = (x(u) - x(eps)).to_f64();
                        let (a0, b0) = (m.u32(ca), m.u32(cb));
                        let (c0, d0) = (m.u32(cc), m.u32(cd));
                        if f & 0x20 != 0 && a0 != 0 {
                            if f & 0x400 != 0 {
                                svn(m, o, um, v, a0, b0);
                                exact(m, o);
                            } else {
                                svn(m, o, u, v, a0, b0);
                            }
                        } else if f & 0x40 != 0 && c0 != 0 {
                            if f & 0x400 != 0 {
                                svn(m, o, um, v, c0, d0);
                                exact(m, o);
                            } else {
                                svn(m, o, u, v, c0, d0);
                            }
                        } else if f & 0x20 != 0 && flg_pos(m, i) {
                            let vm = (x(v) - x(eps)).to_f64();
                            if f & 0x400 != 0 {
                                svn(m, o, um, vm, 0, 0);
                            } else {
                                svn(m, o, u, vm, 0, 0);
                            }
                            exact(m, o);
                        } else if f & 0x400 != 0 {
                            svn(m, o, um, v, 0, 0);
                            exact(m, o);
                        } else {
                            svn(m, o, u, v, 0, 0);
                        }
                        let p = [m.f64(buf) as f32, m.f64(buf + 8) as f32, m.f64(buf + 16) as f32];
                        let nm = [m.f64(nrm) as f32, m.f64(nrm + 8) as f32, m.f64(nrm + 16) as f32];
                        for k in 0..3u32 {
                            m.set_f32(vtx + 8 + 4 * k, p[k as usize]);
                            m.set_f32(vtx + 0x14 + 4 * k, nm[k as usize]);
                            m.set_f32(vtx + 0x20 + 4 * k, p[k as usize]);
                        }
                        let sv = if big(dx) { (x(1.0) - (x(u) - x(m.f64(xs))) * x(dx)).to_f32() } else { 1.0 };
                        m.set_f32(vtx, sv);
                        let tv = if big(dy) { ((x(v) - x(m.f64(ys))) * x(dy)).to_f32() } else { 1.0 };
                        m.set_f32(vtx + 4, tv);
                        m.set_u32(vtx + 0x40, 0);
                        for w in 0..19 {
                            let val = m.u32(vtx + 4 * w);
                            m.set_u32(out.wrapping_add((cnt as u32).wrapping_mul(0x4c)).wrapping_add(4 * w), val);
                        }
                        cnt += 1;
                    }
                    e = m.u32(e);
                    if e == first {
                        break;
                    }
                }
                seg = m.u32(seg);
                if seg == head {
                    break;
                }
            }
            let nxt4 = m.i32(sb + 12 + 4).wrapping_mul(4);
            let a = m.u32(ca);
            if a != 0 && nxt4 <= srf4 {
                o.call(m, SI_NURBS_CRV_DISPOSE, &[ca]);
                m.set_u32(ca, 0);
            }
            let b = m.u32(cb);
            if (b != 0 && nxt4 <= srf4) || flg_pos(m, i) {
                o.call(m, SI_NURBS_CRV_DISPOSE, &[cb]);
                m.set_u32(cb, 0);
            }
            if m.u32(cc) != 0 {
                o.call(m, SI_NURBS_CRV_DISPOSE, &[cc]);
                o.call(m, SI_NURBS_CRV_DISPOSE, &[cd]);
            }
            let (a, b) = (m.u32(ca), m.u32(cb));
            m.set_u32(cc, a);
            m.set_u32(cd, b);
            m.set_u32(cb, 0);
            m.set_u32(ca, 0);
        }
        i += 1;
    }
    m.set_sp(sp);
}

/// [soft 0x08501a78 EvaluateTrimmedNurbsFromBaseGeometry]: `(srf, base, out, n, strips)`: puts the trimmed surface's own
/// samples back on the surface it was made from. `srf` and `base` hold their parameter boxes at +0x10 / +0x18 (u low /
/// high) and +0x34 / +0x3c (v low / high). The u parameter counts as "to be mapped" (`mu`) when the low ends differ by
/// less than 1e-6 or, if not, the high ends do; the v parameter likewise (`mv`). The "difference" the original
/// forms is `base - srf` when that is not negative, else `-base - srf` (so, for a negative difference, minus the
/// sum; kept as read). Every sample of every segment of the `n` strips (except a flag-0x20 sample with a mate) then gets
/// `u = (u - base.lo) / (base.hi - base.lo) * (srf.hi - srf.lo) + srf.lo` (the two spans stored as doubles first) for a
/// mapped u, and the same for v; afterwards `SI_nurbsSrfValue(srf, u, v)` of each such sample is stored as three floats at
/// words 8..10 of the vertex record `out + 0x4c * k` (k counting from 0), nothing else of the record being touched.
pub fn evaluate_trimmed_nurbs_from_base_geometry(m: &mut Mem, o: &mut dyn si_mem::Outside, a: u32, b: u32, out: u32, n: i32, strips: u32) {
    let sp = m.sp();
    let k = x(1e-6);
    let rd = |m: &Mem, p: u32, off: u32| x(m.f64(p + off));
    // the original's "absolute difference" of the low / high ends (see above), tested against 1e-6 the way it does
    let near = |lo_b: F80, lo_a: F80| -> bool {
        let d = lo_b - lo_a;
        let y = if d.ge(F80::ZERO) { d } else { -lo_b - lo_a };
        k.gt(y)
    };
    let near_hi = |hi_b: F80, hi_a: F80| -> bool {
        let d = hi_b - hi_a;
        if d.ge(F80::ZERO) { d.lt(k) } else { (-hi_b - hi_a).lt(k) }
    };
    let mu = near(rd(m, b, 0x10), rd(m, a, 0x10)) || near_hi(rd(m, b, 0x18), rd(m, a, 0x18));
    let mv = near(rd(m, b, 0x34), rd(m, a, 0x34)) || near_hi(rd(m, b, 0x3c), rd(m, a, 0x3c));
    let walk = |m: &mut Mem, o: &mut dyn si_mem::Outside, f: &mut dyn FnMut(&mut Mem, &mut dyn si_mem::Outside, u32)| {
        let mut i = 0;
        while i < n {
            let head = m.u32(strips.wrapping_add((i as u32).wrapping_mul(12)) + 8);
            if head != 0 {
                let mut seg = head;
                loop {
                    let first = m.u32(seg + 8);
                    let mut e = first;
                    loop {
                        if !(m.u8(e + 0x1c) & 0x20 != 0 && m.u32(e + 8) != 0) {
                            f(m, o, e);
                        }
                        e = m.u32(e);
                        if e == first {
                            break;
                        }
                    }
                    seg = m.u32(seg);
                    if seg == head {
                        break;
                    }
                }
            }
            i += 1;
        }
    };
    if mu || mv {
        let (blo, span_b, span_a, alo) = (m.f64(b + 0x10), (rd(m, b, 0x18) - rd(m, b, 0x10)).to_f64(), (rd(m, a, 0x18) - rd(m, a, 0x10)).to_f64(), rd(m, a, 0x10));
        let (bvlo, vspan_b, vspan_a, avlo) =
            (m.f64(b + 0x34), (rd(m, b, 0x3c) - rd(m, b, 0x34)).to_f64(), (rd(m, a, 0x3c) - rd(m, a, 0x34)).to_f64(), rd(m, a, 0x34));
        walk(m, o, &mut |m, _o, e| {
            if mu {
                let r = ((rd(m, e, 0xc) - x(blo)) / x(span_b) * x(span_a)) + alo;
                m.set_f64(e + 0xc, r.to_f64());
            }
            if mv {
                let r = ((rd(m, e, 0x14) - x(bvlo)) / x(vspan_b) * x(vspan_a)) + avlo;
                m.set_f64(e + 0x14, r.to_f64());
            }
        });
    }
    let buf = m.local(24);
    let mut idx: u32 = 0;
    walk(m, o, &mut |m, o, e| {
        let mut args = vec![buf, a];
        evp(&mut args, m.f64(e + 0xc));
        evp(&mut args, m.f64(e + 0x14));
        o.call(m, SI_NURBS_SRF_VALUE, &args);
        for q in 0..3u32 {
            let v = m.f64(buf + 8 * q) as f32;
            m.set_f32(out.wrapping_add(idx.wrapping_mul(0x4c)).wrapping_add(0x20 + 4 * q), v);
        }
        idx += 1;
    });
    m.set_sp(sp);
}

/// [soft 0x084fd14c TriangulateTrimmedNurbsSrf]: `(base, srf, material, mat_id, pvert_n, ptri_n, pverts, ptris, want_idx, pidx)`,
/// with `status` for [`output_triangles`]. The same front half as [`no_trimmed_triangles`] (the surface is `srf`); the counts
/// are written to the two words `ptri_n` (triangles) and `pvert_n` (samples) by [`count_vertices_triangles`], then the
/// vertex array (`calloc(*pvert_n, 0x4c)`) is stored in `*pverts` and filled by [`evaluate_vertices`], the triangle array
/// (`calloc(*ptri_n, 0x60)`) in `*ptris`, with `want_idx` the index array (`calloc(*ptri_n, 12)`) in `*pidx`;
/// [`output_triangles`] fills them, and when `base` is nonzero and differs from `srf` the vertices are re-evaluated on it
/// ([`evaluate_trimmed_nurbs_from_base_geometry`]). A failed allocation or evaluation frees what was made (the vertex array is
/// nulled) and ends. The temporaries are always freed.
pub fn triangulate_trimmed_nurbs_srf(
    m: &mut Mem, o: &mut dyn si_mem::Outside, base: u32, srf: u32, material: u32, mat_id: i32, pvert_n: u32, ptri_n: u32, pverts: u32,
    ptris: u32, want_idx: i16, pidx: u32, status: &mut dyn FnMut(&str, i32),
) -> Result<(), Halt> {
    let sp = m.sp();
    let r = no_trimmed(m, srf, Some((ptri_n, pvert_n)), &mut |m, c| {
        let free_vertices = |m: &mut Mem| {
            let v = m.u32(pverts);
            if v != 0 {
                m.free(v);
                m.set_u32(pverts, 0);
            }
            m.set_u32(pverts, 0);
        };
        let v = m.si_calloc(m.u32(pvert_n), 0x4c);
        m.set_u32(pverts, v);
        if v == 0 {
            return Ok(0);
        }
        // the original answers 1 whatever happens, so its failure branch (free the vertices, end) is never taken
        evaluate_vertices(m, o, srf, m.u32(c.strips), m.i32(c.nv), v, m.u32(c.vs), m.i32(c.nu), m.u32(c.us), m.u32(c.brv));
        let t = m.si_calloc(m.u32(ptri_n), 0x60);
        m.set_u32(ptris, t);
        if t == 0 {
            free_vertices(m);
            return Ok(0);
        }
        if want_idx != 0 {
            let ix = m.si_calloc(m.u32(ptri_n), 12);
            m.set_u32(pidx, ix);
            if ix == 0 {
                let t = m.u32(ptris);
                if t != 0 {
                    m.free(t);
                    m.set_u32(ptris, 0);
                }
                free_vertices(m);
                return Ok(0);
            }
        }
        let (s, nv) = (m.u32(c.strips), m.i32(c.nv));
        output_triangles(m, s, nv, material, mat_id, m.i32(ptri_n), m.u32(pverts), m.u32(ptris), want_idx, m.u32(pidx), status);
        if base != 0 && base != srf {
            evaluate_trimmed_nurbs_from_base_geometry(m, o, base, srf, m.u32(pverts), nv, s);
        }
        Ok(0)
    });
    m.set_sp(sp);
    r.map(|_| ())
}

/// [soft 0x08501700 SI_noTrimmedTriangles]: `(srf)`: the whole tessellation pipeline of a trimmed surface up to the counting
/// step, nothing being output. The u and v directions (`srf` and `srf + 0x24`, `per` at +0x70 / +0x74) are sampled
/// ([`sample_knot_vector`]); the sample curves are made from the domain boundary (`srf + 0x5c` nonzero:
/// [`create_domain_boundary`]) or from the first trim composite `srf + 0x64 [0]` with the box `srf + 0x10 / 0x18 / 0x34 / 0x3c`
/// ([`new_sample_crv`]), and from the others (`srf + 0x60` of them, linked after the first); then the same sequence as
/// [`triangulate_trimmed_nurbs_srf`]'s front half: critical points, break vectors (u, v), the knot insertion when the u break vector has
/// entries, the strips (calloc `nv x 12`, the +8 words nulled), [`intersect_vstrips`], [`enclose_trim_regions`], the horizontal
/// samples, [`unlink_mates`] when the v break vector has entries, [`add_u_samples_full_multiplicity`] when the u one has, and
/// [`count_vertices_triangles`]. Every step that fails (null / 0) frees the temporaries and answers 0; otherwise the triangle total
/// (third word of the count) is answered after the temporaries are freed.
pub fn no_trimmed_triangles(m: &mut Mem, srf: u32) -> Result<u32, Halt> {
    let sp = m.sp();
    let r = no_trimmed(m, srf, None, &mut |m, c| Ok(m.u32(c.ptri)));
    m.set_sp(sp);
    r
}

/// The addresses of the pipeline's locals handed to what runs between the counting and the clean-up.
struct TrimCtx {
    strips: u32,
    nv: u32,
    nu: u32,
    us: u32,
    vs: u32,
    brv: u32,
    ptri: u32,
}

fn no_trimmed(m: &mut Mem, srf: u32, counts: Option<(u32, u32)>, post: &mut dyn FnMut(&mut Mem, &TrimCtx) -> Result<u32, Halt>) -> Result<u32, Halt> {
    // the original's frame: -4 nu, -8 us, -0xc nv, -0x10 vs, -0x14 strips, -0x18 break v, -0x1c break u, -0x20 sample curves,
    // -0x24 / -0x28 the break counts, -0x2c / -0x30 the triangle / vertex counts
    let l = m.local(0x30);
    let (nu, us, nv, vs, strips, brv, bru, crvs, cu, cv, nt, nvx) =
        (l, l + 4, l + 8, l + 12, l + 16, l + 20, l + 24, l + 28, l + 32, l + 36, l + 40, l + 44);
    for p in [crvs, bru, brv, us, vs, strips, cu, cv] {
        m.set_u32(p, 0);
    }
    let (ulo, uhi, vlo, vhi) = (m.f64(srf + 0x10), m.f64(srf + 0x18), m.f64(srf + 0x34), m.f64(srf + 0x3c));
    sample_knot_vector(m, srf, m.i32(srf + 0x70), us, nu)?;
    sample_knot_vector(m, srf + 0x24, m.i32(srf + 0x74), vs, nv)?;
    let free = |m: &mut Mem| free_trimmed_temporary_data(m, us, vs, crvs, bru, brv, strips, m.i32(nv));
    let first = if m.u16(srf + 0x5c) != 0 {
        create_domain_boundary(m, m.i32(nu), m.u32(us), m.i32(nv), m.u32(vs))
    } else {
        let comp = m.u32(m.u32(srf + 0x64));
        new_sample_crv(m, comp, ulo, uhi, vlo, vhi)?
    };
    m.set_u32(crvs, first);
    if first == 0 {
        free(m);
        return Ok(0);
    }
    let mut tail = first;
    let mut i = 1;
    while i < m.i32(srf + 0x60) {
        let comp = m.u32(m.u32(srf + 0x64).wrapping_add((i as u32).wrapping_mul(4)));
        let c = new_sample_crv(m, comp, ulo, uhi, vlo, vhi)?;
        if c == 0 {
            free(m);
            return Ok(0);
        }
        m.set_u32(tail, c);
        m.set_u32(c, 0);
        tail = c;
        i += 1;
    }
    if add_u_critical_points_to_samplings(m, m.u32(crvs), nu, us, nv, vs) as u16 == 0 {
        free(m);
        return Ok(0);
    }
    let b = make_break_vector(m, m.u32(us), m.i32(nu), srf, cu);
    m.set_u32(bru, b);
    if b == 0 {
        free(m);
        return Ok(0);
    }
    let b = make_break_vector(m, m.u32(vs), m.i32(nv), srf + 0x24, cv);
    m.set_u32(brv, b);
    if b == 0 {
        free(m);
        return Ok(0);
    }
    if m.i32(cu) > 0 && add_u_knots_to_sample_crvs(m, m.u32(us), m.i32(nu), m.u32(bru), m.i32(cu), m.u32(crvs)) as u16 == 0 {
        free(m);
        return Ok(0);
    }
    let s = m.si_calloc(m.u32(nv), 12);
    m.set_u32(strips, s);
    if s == 0 {
        free(m);
        return Ok(0);
    }
    for k in 0..m.i32(nv) {
        m.set_u32(s.wrapping_add((k as u32).wrapping_mul(12)) + 8, 0);
    }
    if intersect_vstrips(m, s, m.i32(nv), m.u32(vs), m.u32(crvs), &mut |_| {}) as u16 == 0 {
        free(m);
        return Ok(0);
    }
    enclose_trim_regions(m, s, m.i32(nv));
    if add_horizontal_samples(m, s, m.i32(nv), m.u32(vs), m.i32(nu), m.u32(us), m.u32(bru)) as u16 == 0 {
        free(m);
        return Ok(0);
    }
    if m.i32(cv) > 0 {
        unlink_mates(m, s, m.i32(nv), m.u32(brv), m.i32(cv));
    }
    if m.i32(cu) > 0 && add_u_samples_full_multiplicity(m, s, m.i32(nv)) as u16 == 0 {
        free(m);
        return Ok(0);
    }
    let (ptri, pvert) = counts.unwrap_or((nvx, nt));
    count_vertices_triangles(m, s, m.i32(nv), ptri, pvert);
    let c = TrimCtx { strips, nv, nu, us, vs, brv, ptri };
    let r = post(m, &c);
    free(m);
    r
}
