// SPDX-License-Identifier: MIT
//! The display-line bookkeeping of a moved control point, on the memory image (`nrb_misc.c`;
//! finding 0025, section 20): `updateStartEnd`, `dirtyCurves` and `markDirtyCcrvs`, which
//! `SI_nurbsSrfMovePoint` and its relatives call through `si_mem::Outside`.
//!
//! A surface's line records (+0x84, 0x14 bytes each) hold a word at +0 (non-zero: the line
//! exists), a short at +4 (non-zero: the record is "dirty"/set), and the range of knot spans
//! at +8 and +0xc. All arithmetic is 32-bit and wraps; every division is C's `idiv` (a zero
//! divisor, or `i32::MIN / -1`, kills the original with `SIGFPE`: `Err`).

use crate::image::{crv_move_point, free_clear, mov64, point_copy, srf_dispose_lines, transfer_dir_knots, Halt};
use crate::misc::get_point_interval;
use si_hrc::image::nurbs::{knots_allocate, nurbs_points_allocate};
use si_mem::{Mem, Outside, Routine};
use si_x87::F80;

fn lines(m: &Mem, srf: u32) -> u32 {
    m.u32(srf.wrapping_add(0x84))
}

/// `idiv`: the remainder, or `None` where the processor traps.
fn rem(a: i32, b: i32) -> Option<i32> {
    if b == 0 || (a == i32::MIN && b == -1) {
        None
    } else {
        Some(a % b)
    }
}

fn quot(a: i32, b: i32) -> Option<i32> {
    if b == 0 || (a == i32::MIN && b == -1) {
        None
    } else {
        Some(a / b)
    }
}

/// `n` records from record `row` on, `+off` set to `v` (the line array's pointer is read again for every record).
fn set_range(m: &mut Mem, srf: u32, row: i32, n: i32, off: u32, v: i32) {
    let mut at = 0x14u32.wrapping_mul(row as u32);
    let mut k = n;
    while k > 0 {
        let base = lines(m, srf);
        m.set_i32(base.wrapping_add(at).wrapping_add(off), v);
        at = at.wrapping_add(0x14);
        k -= 1;
    }
}

/// The second branch of `updateStartEnd`: start = `lo`, end = `(lo − 1) % (nk − 1)`.
fn set_wrapped(m: &mut Mem, srf: u32, row: i32, n: i32, lo: i32, nk: i32) -> Result<(), Halt> {
    let mut at = 0x14u32.wrapping_mul(row as u32);
    let mut k = n;
    while k > 0 {
        let base = lines(m, srf);
        m.set_i32(base.wrapping_add(at).wrapping_add(8), lo);
        let base = lines(m, srf);
        let e = rem(lo.wrapping_sub(1), nk.wrapping_sub(1)).ok_or(Halt::Refused("updateStartEnd"))?;
        m.set_i32(base.wrapping_add(at).wrapping_add(0xc), e);
        at = at.wrapping_add(0x14);
        k -= 1;
    }
    Ok(())
}

/// [soft 0x084ea144 updateStartEnd] (static): `(srf, closed, row, count, lo, hi, nk)`: widens the span range of `count`
/// line records from `row` on (stride 0x14) to take in `lo..hi`. Open (`closed` = 0): `lo` below the start lowers the
/// start of every record, else a `hi` above the end raises it. Closed: when `hi` is above `lo` and the record's end is above
/// its start the range is widened on both sides; in every other case (`hi` not above `lo`, or an end not above the start)
/// start becomes `lo` and end `(lo − 1) % (nk − 1)` for all the records.
pub fn update_start_end(m: &mut Mem, srf: u32, closed: i32, row: i32, count: i32, lo: i32, hi: i32, nk: i32) -> Result<(), Halt> {
    let rec = |m: &Mem, off: u32| m.i32(lines(m, srf).wrapping_add(0x14u32.wrapping_mul(row as u32)).wrapping_add(off));
    if closed == 0 {
        if lo < rec(m, 8) {
            set_range(m, srf, row, count, 8, lo);
        } else if hi > rec(m, 0xc) {
            set_range(m, srf, row, count, 0xc, hi);
        }
        return Ok(());
    }
    if hi > lo {
        let (start, end) = (rec(m, 8), rec(m, 0xc));
        if end > start {
            if lo < start {
                set_range(m, srf, row, count, 8, lo);
            }
            if hi > rec(m, 0xc) {
                set_range(m, srf, row, count, 0xc, hi);
            }
            return Ok(());
        }
    }
    set_wrapped(m, srf, row, count, lo, nk)
}

/// [soft 0x084ea484 dirtyCurves] (static): `(srf, dir, dir2, n, first, lo0, stride, a, b, base)` — `n` spans from `first`
/// on along `dir` (`dir2` is the direction whose closed word and knot count steer the update; the original is called with the
/// surface's two directions swapped between the two calls): for each, the record index is
/// `((first + i) % (nk(dir) − 1) [+ 2 − order for an open direction]) × stride + base`. With line records (+0x84 not 0):
/// a record that exists (+0 not 0) and is set (+4 not 0) goes to [`update_start_end`] (closed word of `dir2`, `nk` of
/// `dir2`); an existing record not set gets `[a − lo0, b − lo0]` and the set flag for `stride` records; a record that does
/// not exist makes the search step back to the nearest existing record, then `stride` records back from there, and goes the
/// same way with that index.
#[allow(clippy::too_many_arguments)]
pub fn dirty_curves(m: &mut Mem, srf: u32, dir: u32, dir2: u32, n: i32, first: i32, lo0: i32, stride: i32, a: i32, b: i32, base: i32) -> Result<(), Halt> {
    let mut i: i32 = 0;
    while i < n {
        let nk1 = m.i32(dir.wrapping_add(8)).wrapping_sub(1);
        let mut idx = rem(first.wrapping_add(i), nk1).ok_or(Halt::Refused("dirtyCurves"))?;
        if m.i16(dir.wrapping_add(0x20)) == 0 {
            idx = idx.wrapping_add(2).wrapping_sub(m.i32(dir));
        }
        idx = idx.wrapping_mul(stride).wrapping_add(base);
        i += 1;
        let ln = lines(m, srf);
        if ln != 0 {
            let at = |m: &Mem, k: i32, off: u32| m.i32(ln.wrapping_add(0x14u32.wrapping_mul(k as u32)).wrapping_add(off));
            let flag = |m: &Mem, k: i32| m.i16(ln.wrapping_add(0x14u32.wrapping_mul(k as u32)).wrapping_add(4));
            let call_update = |m: &mut Mem, k: i32| {
                update_start_end(m, srf, m.i16(dir2.wrapping_add(0x20)) as i32, k, stride, a.wrapping_sub(lo0), b.wrapping_sub(lo0), m.i32(dir2.wrapping_add(8)))
            };
            let fill = |m: &mut Mem, k: i32| {
                let mut p = 0x14u32.wrapping_mul(k as u32);
                let mut c = stride;
                while c > 0 {
                    let l = lines(m, srf);
                    m.set_i32(l.wrapping_add(p).wrapping_add(8), a.wrapping_sub(lo0));
                    let l = lines(m, srf);
                    m.set_i32(l.wrapping_add(p).wrapping_add(0xc), b.wrapping_sub(lo0));
                    let l = lines(m, srf);
                    m.set_i16(l.wrapping_add(p).wrapping_add(4), 1);
                    p = p.wrapping_add(0x14);
                    c -= 1;
                }
            };
            if at(m, idx, 0) != 0 {
                if flag(m, idx) == 0 {
                    fill(m, idx);
                } else {
                    call_update(m, idx)?;
                }
            } else if idx > 0 {
                let mut k = idx;
                loop {
                    k -= 1;
                    if at(m, k, 0) != 0 {
                        k += 1;
                        k = k.wrapping_sub(stride);
                        if k >= 0 {
                            if flag(m, k) != 0 {
                                call_update(m, k)?;
                            } else {
                                fill(m, k);
                            }
                        }
                        break;
                    }
                    if k <= 0 {
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}

/// [soft 0x084da69c SI_nurbsSrfNoVlines] as the image reads it (own::srf_no_vlines is the owned form).
fn no_vlines(m: &Mem, srf: u32) -> i32 {
    let (order, nk, curves) = (m.i32(srf), m.i32(srf.wrapping_add(8)), m.i32(srf.wrapping_add(0x7c)));
    if m.i16(srf.wrapping_add(0x20)) == 0 {
        nk.wrapping_sub(order.wrapping_sub(2).wrapping_mul(2).wrapping_add(1)).wrapping_mul(curves).wrapping_add(1)
    } else {
        nk.wrapping_sub(1).wrapping_mul(curves)
    }
}

/// [soft 0x084ea5ec markDirtyCcrvs] (static): `(srf, i)`: the display lines a moved control point `i` of an untrimmed
/// surface bears on. The point's column `i % nu − 1` and row `i / nu − 1` give the spans ([`get_point_interval`] on the
/// two directions); [`dirty_curves`] is run twice, once per direction (the second time with the V lines' base
/// `SI_nurbsSrfNoVlines`); then, for an open U direction with line records, the last V line's flag (+4 of record
/// `SI_nurbsSrfNoVlines − 1`... offset −0x10 from `20 × NoVlines`) is set to 1, and for an open V direction record
/// `+0x80 count − 1`'s.
pub fn mark_dirty_ccrvs(m: &mut Mem, srf: u32, i: i32) -> Result<(), Halt> {
    let nu = m.i32(srf.wrapping_add(4));
    let col = rem(i, nu).ok_or(Halt::Refused("markDirtyCcrvs"))?.wrapping_sub(1);
    let row = quot(i, nu).ok_or(Halt::Refused("markDirtyCcrvs"))?.wrapping_sub(1);
    // the eight words the two calls of `getPointInterval` fill: the original's locals, [ebp-0x10..-4] and [ebp-0x20..-0x14]
    let loc = m.local(32);
    let w = |k: u32| loc + 4 * k;
    // U: first, last, count, start at w(0..4); V at w(4..8)
    let r = get_point_interval(m, srf, col, w(0), w(1), w(2), w(3)).and_then(|_| get_point_interval(m, srf.wrapping_add(0x24), row, w(4), w(5), w(6), w(7)));
    if r.is_err() {
        m.release(32);
        return r;
    }
    let (f1, l1, c1, s1) = (m.i32(w(0)), m.i32(w(1)), m.i32(w(2)), m.i32(w(3)));
    let (f2, l2, c2, s2) = (m.i32(w(4)), m.i32(w(5)), m.i32(w(6)), m.i32(w(7)));
    m.release(32);
    let v = srf.wrapping_add(0x24);
    dirty_curves(m, srf, srf, v, c1, f1, s2, m.i32(srf.wrapping_add(0x7c)), f2, l2, 0)?;
    let nv_lines = no_vlines(m, srf);
    dirty_curves(m, srf, v, srf, c2, f2, s1, m.i32(srf.wrapping_add(0x78)), f1, l1, nv_lines)?;
    if m.i16(srf.wrapping_add(0x20)) == 0 && lines(m, srf) != 0 {
        let k = no_vlines(m, srf);
        let at = lines(m, srf).wrapping_add(0x14u32.wrapping_mul(k as u32)).wrapping_sub(0x10);
        m.set_i16(at, 1);
    }
    if m.i16(srf.wrapping_add(0x44)) == 0 && lines(m, srf) != 0 {
        let k = m.i32(srf.wrapping_add(0x80));
        let at = lines(m, srf).wrapping_sub(0x10).wrapping_add(0x14u32.wrapping_mul(k as u32));
        m.set_i16(at, 1);
    }
    Ok(())
}

// ------------------------------------------------------------------------------------ the transfers of the closers

/// [soft 0x084ea038 SI_nurbsCrvTransferPoints] and [soft 0x084ea00c SI_nurbsCrvTransferPointsKnots], on the image (the
/// owned forms are [`crate::misc::crv_transfer_points`], [`crate::misc::crv_transfer_points_knots`]): bits 0x60 of the
/// destination's +0x3b, the source's points (**its** count at +4, nine words each, both arrays read again at every point)
/// over the destination's, then the knots and range ([`transfer_dir_knots`]).
pub fn crv_transfer_points_knots(m: &mut Mem, src: u32, dst: u32) {
    m.or_u8(dst.wrapping_add(0x3b), 0x60);
    let mut i: i32 = 0;
    while i < m.i32(src.wrapping_add(4)) {
        let off = 0x24u32.wrapping_mul(i as u32);
        let (from, to) = (m.u32(src.wrapping_add(0x2c)).wrapping_add(off), m.u32(dst.wrapping_add(0x2c)).wrapping_add(off));
        point_copy(m, from, to);
        i += 1;
    }
    transfer_dir_knots(m, src, dst);
}

/// [soft 0x084ea860 nurbsCrvTransferOriginal]: `(dst, src)`: the curve `dst` takes the source's closed word (+0x20, 16 bits),
/// knot count (+8), point count (+4), range (+0x10, +0x18 through the unit) and +0x24; its points block is resized to
/// `count * 36` and its knots block to `knots * 8` (`SI_realloc`, which for a size of 0 leaves the block alone), then the
/// points and knots are copied ([`crv_transfer_points_knots`]).
pub fn nurbs_crv_transfer_original(m: &mut Mem, dst: u32, src: u32) {
    let w = m.u16(src.wrapping_add(0x20));
    m.set_u16(dst.wrapping_add(0x20), w);
    for off in [8u32, 4] {
        let v = m.u32(src.wrapping_add(off));
        m.set_u32(dst.wrapping_add(off), v);
    }
    mov64(m, src.wrapping_add(0x10), dst.wrapping_add(0x10));
    mov64(m, src.wrapping_add(0x18), dst.wrapping_add(0x18));
    let v = m.u32(src.wrapping_add(0x24));
    m.set_u32(dst.wrapping_add(0x24), v);
    let n = m.u32(src.wrapping_add(4)).wrapping_mul(0x24);
    let p = m.si_realloc(m.u32(dst.wrapping_add(0x2c)), n);
    m.set_u32(dst.wrapping_add(0x2c), p);
    let n = m.u32(dst.wrapping_add(8)).wrapping_mul(8);
    let k = m.si_realloc(m.u32(dst.wrapping_add(0xc)), n);
    m.set_u32(dst.wrapping_add(0xc), k);
    crv_transfer_points_knots(m, src, dst);
}

/// [soft 0x084ec1e4 nurbsSrfTransferOriginal]: `(dst, src)`: the display lines of `dst` disposed of
/// ([`srf_dispose_lines`], so `SI_genTreeDestroy` through `o`); the source's closed words (+0x20, +0x44; 16 bits), counts
/// (+8, +4, +0x2c, +0x28, +0x48, +0x4c as 32 bits) and ranges (+0x10, +0x18, +0x34, +0x3c) copied; the points (+0x54), U knots
/// (+0xc) and V knots (+0x30) blocks freed and cleared, then allocated anew for the new total and knot counts
/// (`SI_nurbsPointsAllocate`, `si_knotsAllocate` twice) and filled by `SI_nurbsSrfTransferPointsKnots` ([`crate::image::srf_transfer_points_knots`]).
pub fn nurbs_srf_transfer_original(m: &mut Mem, o: &mut dyn Outside, dst: u32, src: u32) {
    srf_dispose_lines(m, o, dst);
    for off in [0x20u32, 0x44] {
        let w = m.u16(src.wrapping_add(off));
        m.set_u16(dst.wrapping_add(off), w);
    }
    let v = m.u32(src.wrapping_add(8));
    m.set_u32(dst.wrapping_add(8), v);
    let v = m.u32(src.wrapping_add(4));
    m.set_u32(dst.wrapping_add(4), v);
    mov64(m, src.wrapping_add(0x10), dst.wrapping_add(0x10));
    mov64(m, src.wrapping_add(0x18), dst.wrapping_add(0x18));
    for off in [0x2cu32, 0x28] {
        let v = m.u32(src.wrapping_add(off));
        m.set_u32(dst.wrapping_add(off), v);
    }
    mov64(m, src.wrapping_add(0x34), dst.wrapping_add(0x34));
    mov64(m, src.wrapping_add(0x3c), dst.wrapping_add(0x3c));
    for off in [0x48u32, 0x4c] {
        let v = m.u32(src.wrapping_add(off));
        m.set_u32(dst.wrapping_add(off), v);
    }
    for off in [0x54u32, 0xc, 0x30] {
        free_clear(m, dst.wrapping_add(off));
    }
    let p = nurbs_points_allocate(m, m.i32(dst.wrapping_add(0x48)));
    m.set_u32(dst.wrapping_add(0x54), p);
    let k = knots_allocate(m, m.i32(dst.wrapping_add(8)));
    m.set_u32(dst.wrapping_add(0xc), k);
    let k = knots_allocate(m, m.i32(dst.wrapping_add(0x2c)));
    m.set_u32(dst.wrapping_add(0x30), k);
    crate::image::srf_transfer_points_knots(m, o, src, dst);
}

// ------------------------------------------------------------------------------------ a function applied to the points

/// `SI_nurbsSrfMovePoint (0x084e9f2c)`; called through `o` here (the port is [`crate::image::srf_move_point`]).
pub const SI_NURBS_SRF_MOVE_POINT: Routine = Routine::new("SI_nurbsSrfMovePoint", 0x084e_9f2c);
/// `SI_nurbsCapping (0x084f00a0)`; called through `o` (the port is [`crate::image::nurbs_capping`]).
pub const SI_NURBS_CAPPING: Routine = Routine::new("SI_nurbsCapping", 0x084f_00a0);

fn dwords(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

/// The point at `p` divided by its weight, each coordinate (x, y, z) `coordinate * (1.0 / w)`: the reciprocal stays in
/// the register, each product is stored as a double.
fn unit_xyz(m: &Mem, p: u32) -> [f64; 3] {
    let inv = F80::ONE / F80::from_f64(m.f64(p.wrapping_add(0x18)));
    core::array::from_fn(|k| (F80::from_f64(m.f64(p.wrapping_add(8 * k as u32))) * inv).to_f64())
}

/// Calls the per-point function `cb(x', y', z', &x, &y, &z)` (the doubles on the stack, then three pointers to the
/// destination point's coordinates) through `o`.
fn call_point_func(m: &mut Mem, o: &mut dyn Outside, cb: Routine, src_point: u32, dst_point: u32) {
    let [x, y, z] = unit_xyz(m, src_point);
    let mut a = Vec::with_capacity(9);
    a.extend(dwords(x));
    a.extend(dwords(y));
    a.extend(dwords(z));
    a.extend([dst_point, dst_point.wrapping_add(8), dst_point.wrapping_add(0x10)]);
    o.call(m, cb, &a);
}

/// [soft 0x084e982c SI_nurbsSrfApplyFunc2Points]: `(src, dst, func, mask)`. For each of the source's points (+0x48 of the
/// source, read again every time; both point arrays read once) whose flag word (+0x20) has a bit of `mask` (every point for
/// `mask` 0): `func(x/w, y/w, z/w, &dst.x, &dst.y, &dst.z)` through `o` (the divisions as `coordinate * (1.0 / w)`, one
/// rounding to `double`); bits 0x60 of the destination point's +0x23; then `SI_nurbsSrfMovePoint(dst, index, dst.x, dst.y,
/// dst.z, src.w)` through `o`, the index being the destination pointer's distance from the array in points (an `imul` by
/// 0x38e38e39 and a shift, exact for a distance that is a multiple of 36). Last bits 0x60 of the destination's +0x5b and
/// `SI_nurbsCapping(dst)` through `o`.
pub fn srf_apply_func2_points(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32, func: Routine, mask: u32) {
    let (mut sp, mut dp) = (m.u32(src.wrapping_add(0x54)), m.u32(dst.wrapping_add(0x54)));
    let mut i: i32 = 0;
    while i < m.i32(src.wrapping_add(0x48)) {
        if mask == 0 || (m.u32(sp.wrapping_add(0x20)) & mask) != 0 {
            call_point_func(m, o, func, sp, dp);
            m.or_u8(dp.wrapping_add(0x23), 0x60);
            let w = si_x87::fmov64(m.f64(sp.wrapping_add(0x18)));
            let [x, y, z] = [0u32, 8, 0x10].map(|k| si_x87::fmov64(m.f64(dp.wrapping_add(k))));
            let idx = ((dp.wrapping_sub(m.u32(dst.wrapping_add(0x54))) as i32).wrapping_mul(0x38e3_8e39u32 as i32)) >> 2;
            let mut a = vec![dst, idx as u32];
            for v in [x, y, z, w] {
                a.extend(dwords(v));
            }
            o.call(m, SI_NURBS_SRF_MOVE_POINT, &a);
        }
        i = i.wrapping_add(1);
        sp = sp.wrapping_add(0x24);
        dp = dp.wrapping_add(0x24);
    }
    m.or_u8(dst.wrapping_add(0x5b), 0x60);
    o.call(m, SI_NURBS_CAPPING, &[dst]);
}

/// [soft 0x084e9908 SI_nurbsCrvApplyFunc2Points]: the curve form, over the source's point count (+4): after the function
/// and the flag, `SI_nurbsCrvMovePoint(dst, i, dst.x * src.w, dst.y * src.w, dst.z * src.w)` ([`crv_move_point`], which
/// multiplies by the destination's own weight again; each product in the register, one rounding to `double`). Last bits 0x60 of
/// the destination's +0x3b.
pub fn crv_apply_func2_points(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32, func: Routine, mask: u32) {
    let (mut sp, mut dp) = (m.u32(src.wrapping_add(0x2c)), m.u32(dst.wrapping_add(0x2c)));
    let mut i: i32 = 0;
    while i < m.i32(src.wrapping_add(4)) {
        if mask == 0 || (m.u32(sp.wrapping_add(0x20)) & mask) != 0 {
            call_point_func(m, o, func, sp, dp);
            m.or_u8(dp.wrapping_add(0x23), 0x60);
            let w = F80::from_f64(m.f64(sp.wrapping_add(0x18)));
            let [x, y, z] = [0u32, 8, 0x10].map(|k| (F80::from_f64(m.f64(dp.wrapping_add(k))) * w).to_f64());
            crv_move_point(m, dst, i, x, y, z);
        }
        i = i.wrapping_add(1);
        sp = sp.wrapping_add(0x24);
        dp = dp.wrapping_add(0x24);
    }
    m.or_u8(dst.wrapping_add(0x3b), 0x60);
}
