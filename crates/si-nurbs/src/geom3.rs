// SPDX-License-Identifier: MIT
//! Surface-side routines of `nrb_b.c` on the memory image (finding 0025, section 18): the
//! knots of a surface made from its points (`SI_knotsSrfReparametrization` with
//! `nurbsSrfAveragePointDelta`), the exchange of rows for a conversion
//! (`si_nurbsSrfInverseForConversion`) and the shift of a closed direction's knots
//! (`SI_shiftDirKnotsClosed`).

use crate::image::{
    at8, dir_inverse, LEFT, crv_inverse, free_clear, free_some, knot_count, knots_crv_reparam_chord_centri, knots_crv_reparam_chord_centri_quad,
    mov64, point_copy, range_from_knots, srf_dispose_lines, SI_GBL_INDEX,
};
use crate::trimutils::si_trm_cvr_rve;
use si_hrc::image::nurbs::knots_allocate;
use crate::{Dir, NurbsDir, Statics};
use si_mem::{Mem, Outside};
use si_types::Ptr32;
use si_x87::{x, F80};

/// `_DAT_08661770`, the least average distance: 0.01.
const MIN_DELTA: f64 = 0.01;

/// The point at index `i`, divided by its weight, in the register.
fn unit_point(m: &Mem, pts: u32, i: i32) -> [F80; 3] {
    let a = pts.wrapping_add(0x24u32.wrapping_mul(i as u32));
    let w = x(m.f64(a.wrapping_add(0x18)));
    [x(m.f64(a)) / w, x(m.f64(a.wrapping_add(8))) / w, x(m.f64(a.wrapping_add(0x10))) / w]
}

/// The distance between points `i` and `j` of the surface's net: the three differences of the
/// weight-divided coordinates unrounded, the squares added as z² + (y² + x²), the root, and for
/// a parametrisation word of 4 (+0x4c) its root again.
fn net_distance(m: &Mem, s: u32, i: i32, j: i32) -> F80 {
    let pts = m.u32(s + 0x54);
    let a = unit_point(m, pts, i);
    let b = unit_point(m, pts, j);
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    let mut d = (dz * dz + (dy * dy + dx * dx)).sqrt();
    if m.i32(s + 0x4c) == 4 {
        d = d.sqrt();
    }
    d
}

/// 0.01 when it is above the value in the register, else the value.
fn at_least_min(v: F80) -> F80 {
    let c = x(MIN_DELTA);
    if c.gt(v) { c } else { v }
}

/// [soft 0x084de8ec nurbsSrfAveragePointDelta]: `(srf, out, dir)`: the average distance of neighbouring points of the net,
/// one entry per span of a direction; `dir` non-zero: the U direction (across the `nu` columns, averaged over the `nv`
/// rows), 0: V. For a closed direction one more entry (the average over the rows of the distance of the first and last
/// column, at least 0.01), and the entries then turned one place to the right with that last one first. Each entry is
/// the register sum divided by the other direction's count (an integer), stored as a `double`. The closing entry is divided by `nv` in both
/// directions (for V that is not the count it sums over; as the original does).
pub fn average_point_delta(m: &mut Mem, s: u32, out: u32, dir: i16) {
    let nu = m.i32(s + 4);
    let nv = m.i32(s + 0x28);
    // (count of spans, other count, closed word, stride of a step along the direction, stride across)
    let (count, other, closed) = if dir != 0 { (nu, nv, m.i16(s + 0x20)) } else { (nv, nu, m.i16(s + 0x44)) };
    let mut slot: i32 = 0;
    let mut i: i32 = 0;
    while i < count.wrapping_sub(1) {
        let mut acc = F80::ZERO;
        let mut k: i32 = 0;
        // the original re-reads the other count at each outer step; the record does not change
        while k < other {
            let (a, b) = if dir != 0 {
                let a = i.wrapping_add(k.wrapping_mul(nu));
                (a, a.wrapping_add(1))
            } else {
                let a = k.wrapping_add(i.wrapping_mul(nu));
                (a, a.wrapping_add(nu))
            };
            acc = acc + net_distance(m, s, a, b);
            k += 1;
        }
        m.set_f64(at8(out, slot), (acc / F80::from_i32(other)).to_f64());
        i += 1;
        slot += 1;
    }
    if closed == 0 {
        return;
    }
    let mut acc = F80::ZERO;
    if dir != 0 {
        let mut k = 0;
        while k < nv {
            acc = acc + net_distance(m, s, k.wrapping_mul(nu), nu.wrapping_sub(1).wrapping_add(k.wrapping_mul(nu)));
            k += 1;
        }
    } else {
        let mut k = 0;
        while k < nu {
            let b = m.i32(s + 0x48).wrapping_sub(nu.wrapping_sub(k));
            acc = acc + net_distance(m, s, k, b);
            k += 1;
        }
    }
    let v = at_least_min(acc / F80::from_i32(nv));
    m.set_f64(at8(out, slot), v.to_f64());
    // the last entry (index count − 1) is loaded first, the others move up one place, the loaded one goes first
    let last = x(m.f64(at8(out, count.wrapping_sub(1))));
    let mut j = count.wrapping_sub(1);
    while j > 0 {
        let v = x(m.f64(at8(out, j - 1)));
        m.set_f64(at8(out, j), v.to_f64());
        j -= 1;
    }
    m.set_f64(out, last.to_f64());
}

/// The uniform knots of a direction without any: count ([`knot_count`], into +8), `si_knotsAllocate`, `z = order −
/// (closed × (order − 2) + 1)` zeros, then 1, 2, … up to index count − z, and an open direction's last order − 2 repeat
/// the one before them. Answers the count.
fn uniform_knots(m: &mut Mem, dir: u32) -> i32 {
    let order = m.i32(dir);
    let nk = knot_count(m, dir);
    m.set_i32(dir + 8, nk);
    let k = knots_allocate(m, nk);
    m.set_u32(dir + 0xc, k);
    let lead = order.wrapping_sub(2);
    let zeros = order.wrapping_sub((m.i16(dir + 0x20) as i32).wrapping_mul(lead).wrapping_add(1));
    let mut i = 0;
    while i < zeros {
        m.set_u64(at8(k, i), 0);
        i += 1;
    }
    let mut j: i32 = 1;
    let mut i = zeros;
    while i <= nk.wrapping_sub(zeros) {
        m.set_f64(at8(k, i), j as f64);
        j = j.wrapping_add(1);
        i += 1;
    }
    if m.i16(dir + 0x20) == 0 {
        for t in 0..lead.max(0) {
            let to = nk.wrapping_sub(lead).wrapping_add(t);
            mov64(m, at8(k, to - 1), at8(k, to));
        }
    }
    nk
}

/// The knots of a direction (of the surface record at `dir`) from the average distances of the net:
/// `si_knotsAllocate(n − 1 + closed)`, [`average_point_delta`], the chord function for the order, the scratch freed.
fn knots_from_net(m: &mut Mem, s: u32, dir: u32, uside: bool) {
    let n = m.i32(dir + 4);
    let closed = m.i16(dir + 0x20) as i32;
    let d = knots_allocate(m, n.wrapping_sub(1).wrapping_add(closed));
    average_point_delta(m, s, d, uside as i16);
    if m.i32(dir) == 3 {
        knots_crv_reparam_chord_centri_quad(m, dir, d);
    } else {
        knots_crv_reparam_chord_centri(m, dir, d);
    }
    free_some(m, d);
}

/// The normalising of the knots of a direction made from chords (parametrisation 2): when |knot 0| is above 1e-6 every
/// knot less knot 0 (the difference in the register, stored; 0 when its magnitude is below 1e-6); then a knot less than
/// 1e-6 above the one before is set to it.
fn normalise_knots(m: &mut Mem, dir: u32) {
    let k = m.u32(dir + 0xc);
    let k0 = x(m.f64(k));
    let eps = x(crate::EPS);
    if eps.lt(k0.abs()) {
        let mut i = 0;
        while i < m.i32(dir + 8) {
            let v = x(m.f64(at8(k, i))) - k0;
            m.set_f64(at8(k, i), v.to_f64());
            if v.abs().lt(eps) {
                m.set_u64(at8(k, i), 0);
            }
            i += 1;
        }
    }
    let mut i = 0;
    while i < m.i32(dir + 8).wrapping_sub(1) {
        let a = x(m.f64(at8(k, i)));
        if (x(m.f64(at8(k, i + 1))) - a).lt(eps) {
            m.set_f64(at8(k, i + 1), a.to_f64());
        }
        i += 1;
    }
}

/// [soft 0x084ded40 SI_knotsSrfReparametrization], on the image: the
/// knots of a surface made anew from its parametrisation (+0x4c); the display lines are disposed first
/// (`SI_nurbsSrfDisposeLines`).
///
/// - 2: a direction without knots gets them from the net ([`knots_from_net`]), then [`normalise_knots`] and the range
///   ([`range_from_knots`]); U, then V.
/// - otherwise nothing for a direction with fewer points than its order (either one, and the routine ends before it
///   touches a range). 0 (uniform): a direction without knots gets uniform ones ([`uniform_knots`]), one that has some
///   keeps them and its count; any other value: the knots of both directions freed and made from the net (U first, V
///   second; the count taken after each). Then the ranges: closed, the first knot and knot count − 1; open, knot order −
///   2 and knot count − order + 1; U into +0x10/+0x18, V into +0x34/+0x3c.
pub fn knots_srf_reparametrization(m: &mut Mem, o: &mut dyn Outside, s: u32) {
    let (order_u, order_v) = (m.i32(s), m.i32(s + 0x24));
    srf_dispose_lines(m, o, s);
    if m.i32(s + 0x4c) == 2 {
        if m.u32(s + 0xc) == 0 {
            knots_from_net(m, s, s, true);
        }
        normalise_knots(m, s);
        let c = m.i32(s + 8);
        range_from_knots(m, s, c);
        if m.u32(s + 0x30) == 0 {
            knots_from_net(m, s, s + 0x24, false);
        }
        normalise_knots(m, s + 0x24);
        let c = m.i32(s + 0x2c);
        range_from_knots(m, s + 0x24, c);
        return;
    }
    if m.i32(s + 4) < order_u || m.i32(s + 0x28) < order_v {
        return;
    }
    let (mut count_u, mut count_v) = (m.i32(s + 8), m.i32(s + 0x2c));
    if m.i32(s + 0x4c) == 0 {
        if m.u32(s + 0xc) == 0 {
            count_u = uniform_knots(m, s);
        }
        if m.u32(s + 0x30) == 0 {
            count_v = uniform_knots(m, s + 0x24);
        }
    } else {
        free_clear(m, s + 0xc);
        knots_from_net(m, s, s, true);
        count_u = m.i32(s + 8);
        free_clear(m, s + 0x30);
        knots_from_net(m, s, s + 0x24, false);
        count_v = m.i32(s + 0x2c);
    }
    range_from_knots(m, s, count_u);
    range_from_knots(m, s + 0x24, count_v);
}

/// [soft 0x084de178 SI_shiftDirKnotsClosed]: `(dir, shift)`: nothing unless `shift` is above 0. The knot differences d[i] =
/// knot[i + 1] − knot[i] (count − 1 of them, stored as doubles, in a scratch block) are rotated by `shift` into a
/// second scratch block: closed, from index `shift` up to count − 1 then from 0 up to `shift`; open, the shift counted
/// from index order − 2: from order − 2 + `shift` up to count − (order − 1), then from order − 2 up to order − 2 + `shift`.
/// The knots are rebuilt from the front: the start is index 1, and for an open direction it skips the leading knots
/// that repeat (|knot[i] − knot[i − 1]| below 1e-6); each knot from there to the end index is the one before plus the
/// next rotated difference (the sum in the register, stored). The range (+0x10, +0x18) is the first and the last knot.
/// Both scratch blocks are freed.
pub fn shift_dir_knots_closed(m: &mut Mem, dir: u32, shift: i32) {
    if shift <= 0 {
        return;
    }
    let d = knots_allocate(m, m.i32(dir + 8).wrapping_sub(1));
    let r = knots_allocate(m, m.i32(dir + 8).wrapping_sub(1));
    let mut i: i32 = 0;
    while i < m.i32(dir + 8).wrapping_sub(1) {
        let k = m.u32(dir + 0xc);
        let v = x(m.f64(at8(k, i + 1))) - x(m.f64(at8(k, i)));
        m.set_f64(at8(d, i), v.to_f64());
        i += 1;
    }
    let mut j: i32 = 0;
    let (mut from, end, mut start2, stop2);
    if m.i16(dir + 0x20) == 0 {
        let order = m.i32(dir);
        start2 = order.wrapping_sub(2);
        from = order.wrapping_sub(2).wrapping_add(shift);
        end = m.i32(dir + 8).wrapping_sub(order.wrapping_sub(1));
        stop2 = from;
    } else {
        start2 = 0;
        from = shift;
        end = m.i32(dir + 8).wrapping_sub(1);
        stop2 = shift;
    }
    while from < end {
        mov64(m, at8(d, from), at8(r, j));
        j += 1;
        from += 1;
    }
    while start2 < stop2 {
        mov64(m, at8(d, start2), at8(r, j));
        j += 1;
        start2 += 1;
    }
    let mut i: i32 = 1;
    if m.i16(dir + 0x20) == 0 {
        // the original has no bound here: all-equal knots would run it off the block
        loop {
            let k = m.u32(dir + 0xc);
            let diff = (x(m.f64(at8(k, i))) - x(m.f64(at8(k, i - 1)))).abs();
            if !diff.lt(x(crate::EPS)) {
                break;
            }
            i += 1;
        }
    }
    let mut j: i32 = 0;
    while i <= end {
        let k = m.u32(dir + 0xc);
        let v = x(m.f64(at8(k, i - 1))) + x(m.f64(at8(r, j)));
        m.set_f64(at8(k, i), v.to_f64());
        j += 1;
        i += 1;
    }
    let k = m.u32(dir + 0xc);
    mov64(m, k, dir + 0x10);
    mov64(m, at8(k, m.i32(dir + 8).wrapping_sub(1)), dir + 0x18);
    free_some(m, d);
    free_some(m, r);
}

/// The exchange of the net's rows end for end, for one direction, from index `lo` (a closed direction starts at 1 in
/// [`si_nurbs_srf_inverse_for_conversion`], every direction at 0 in [`si_nurbs_srf_inverse`]); `SI_gblIndex` records
/// each pair.
fn swap_net(m: &mut Mem, s: u32, u: bool, lo: i32) {
    let (n_this, n_other) = if u { (s + 4, s + 0x28) } else { (s + 0x28, s + 4) };
    let nu = m.i32(s + 4);
    let mut hi = m.i32(n_this).wrapping_sub(1);
    let mut lo = lo;
    if lo < m.i32(n_this).wrapping_sub(1) {
        loop {
            let next = lo + 1;
            let hi1 = hi - 1;
            let mut t = m.i32(n_other);
            loop {
                t = t.wrapping_sub(1);
                if t < 0 {
                    break;
                }
                // U: the points of column hi and lo in row t; V: of row hi and lo in column t
                let (ia, ib) = if u {
                    (hi.wrapping_add(t.wrapping_mul(nu)), t.wrapping_mul(nu).wrapping_add(lo))
                } else {
                    (hi.wrapping_mul(nu).wrapping_add(t), nu.wrapping_mul(lo).wrapping_add(t))
                };
                let at = |m: &Mem, i: i32| m.u32(s + 0x54).wrapping_add(0x24u32.wrapping_mul(i as u32));
                let held: [u32; 9] = core::array::from_fn(|k| m.u32(at(m, ia).wrapping_add(4 * k as u32)));
                let (pa, pb) = (at(m, ia), at(m, ib));
                point_copy(m, pb, pa);
                let pb = at(m, ib);
                for (k, w) in held.iter().enumerate() {
                    m.set_u32(pb.wrapping_add(4 * k as u32), *w);
                }
                let g = m.u32(SI_GBL_INDEX);
                if g != 0 {
                    m.set_i32(g.wrapping_add(4u32.wrapping_mul(ia as u32)), ib);
                    let g = m.u32(SI_GBL_INDEX);
                    m.set_i32(g.wrapping_add(4u32.wrapping_mul(ib as u32)), ia);
                }
            }
            hi = hi1;
            lo = next;
            if !(next < hi1) {
                break;
            }
        }
    }
}

/// The two halves of [`si_nurbs_srf_inverse_for_conversion`], one per direction: `u` the U direction (rows of the net
/// exchanged across the U count, `nu`) or V.
fn invert_direction(m: &mut Mem, o: &mut dyn Outside, s: u32, u: bool) {
    let (closed_off, knots_off, kcount_off) = if u { (s + 0x20, s + 0xc, s + 8) } else { (s + 0x44, s + 0x30, s + 0x2c) };
    swap_net(m, s, u, (m.i16(closed_off) != 0) as i32);
    // the span of the parameter, kept in the register across the curves' reversal
    let kn = m.u32(knots_off);
    let span = x(m.f64(at8(kn, m.i32(kcount_off).wrapping_sub(1)))) - x(m.f64(kn));
    let mut ti: i32 = 0;
    while ti < m.i32(s + 0x60) {
        let trim = |m: &Mem| m.u32(m.u32(s + 0x64).wrapping_add(4 * ti as u32));
        let mut ci: i32 = 0;
        while ci < m.i32(trim(m) + 4) {
            let c = m.u32(m.u32(trim(m) + 8).wrapping_add(4 * ci as u32));
            crv_inverse(m, o, c);
            let mut pi: i32 = 0;
            while pi < m.i32(m.u32(m.u32(trim(m) + 8).wrapping_add(4 * ci as u32)) + 4) {
                let c = m.u32(m.u32(trim(m) + 8).wrapping_add(4 * ci as u32));
                let p = m.u32(c + 0x2c).wrapping_add(0x24u32.wrapping_mul(pi as u32));
                let at = if u { p } else { p.wrapping_add(8) };
                let v = x(m.f64(p.wrapping_add(0x18))) * span - x(m.f64(at));
                m.set_f64(at, v.to_f64());
                pi += 1;
            }
            ci += 1;
        }
        let line = m.u32(trim(m) + 0xc);
        if line != 0 {
            m.set_i32(line + 4, m.i32(line + 4).wrapping_neg());
            let (lo_off, hi_off, node_off) = if u { (0x10, 0x20, 0) } else { (0x28, 0x18, 8) };
            let old = x(m.f64(line + lo_off));
            let v = span - x(m.f64(line + hi_off));
            m.set_f64(line + lo_off, v.to_f64());
            let v = span - old;
            m.set_f64(line + hi_off, v.to_f64());
            let mut node = m.u32(line + 8);
            while node != 0 {
                let v = span - x(m.f64(node + node_off));
                m.set_f64(node + node_off, v.to_f64());
                node = m.u32(node + 0x14);
            }
            si_trm_cvr_rve(m, line);
        }
        // the trim's curves in reverse order
        let n = m.i32(trim(m) + 4);
        let mut a: i32 = 0;
        while a < n / 2 {
            let arr = m.u32(trim(m) + 8);
            let held = m.u32(arr.wrapping_add(4 * a as u32));
            let other = m.u32(arr.wrapping_add(4u32.wrapping_mul((n - 1 - a) as u32)));
            m.set_u32(arr.wrapping_add(4 * a as u32), other);
            let arr = m.u32(trim(m) + 8);
            m.set_u32(arr.wrapping_add(4u32.wrapping_mul((n - a) as u32)).wrapping_sub(4), held);
            a += 1;
        }
        ti += 1;
    }
}

/// [soft 0x084dd29c si_nurbsSrfInverseForConversion]: `(srf, u, v)`: for `u` non-zero the net is mirrored across U (the
/// columns exchanged end for end from index 1 when U is closed, from 0 when open, through a copy on the stack), where
/// `SI_gblIndex` is set it records each pair (table[a] = b, table[b] = a); then each trim's curves are reversed
/// (`SI_nurbsCrvInverse`) and every curve point's U coordinate becomes weight × (last knot − first knot) − U, the trim
/// line's sign is negated, its U range and every node's U mirrored (`d − x`; `SI_trmCvrRve` on the line), and the trim's
/// curve list reversed. The same for `v` with V. Then `SI_knotsSrfReparametrization`. Answers 1.
pub fn si_nurbs_srf_inverse_for_conversion(m: &mut Mem, o: &mut dyn Outside, s: u32, u: i16, v: i16) -> i32 {
    if u != 0 {
        invert_direction(m, o, s, true);
    }
    if v != 0 {
        invert_direction(m, o, s, false);
    }
    knots_srf_reparametrization(m, o, s);
    1
}

/// The trims of one direction of [`si_nurbs_srf_inverse`] (`u` the U direction, else V): every curve reversed
/// (`SI_nurbsCrvInverse`) and each point's parameter mirrored, the trim line's sign negated with its range and nodes
/// mirrored (`SI_trmCvrRve` on it) and the trim's curve list reversed. `copy` is the copy of the surface made before the
/// direction was reversed, `span` the parameter span as a stored double. A point of a closed direction (on a trim whose
/// word at +0 is 0) is placed through the knot of the same span in the copy; any other is `span - x`.
fn mirror_trims(m: &mut Mem, o: &mut dyn Outside, s: u32, copy: u32, span: F80, u: bool) {
    let (closed_off, order_off, count_off, knots_off, pt_off, dir_off) =
        if u { (s + 0x20, s, s + 8, s + 0xc, 0u32, 0u32) } else { (s + 0x44, s + 0x24, s + 0x2c, s + 0x30, 8u32, 0x24u32) };
    let mut ti: i32 = 0;
    while ti < m.i32(s + 0x60) {
        let trim = |m: &Mem| m.u32(m.u32(s + 0x64).wrapping_add(4 * ti as u32));
        let mut ci: i32 = 0;
        while ci < m.i32(trim(m) + 4) {
            let c = m.u32(m.u32(trim(m) + 8).wrapping_add(4 * ci as u32));
            crv_inverse(m, o, c);
            let mut pi: i32 = 0;
            while pi < m.i32(m.u32(m.u32(trim(m) + 8).wrapping_add(4 * ci as u32)) + 4) {
                let c = m.u32(m.u32(trim(m) + 8).wrapping_add(4 * ci as u32));
                let at = m.u32(c + 0x2c).wrapping_add(0x24u32.wrapping_mul(pi as u32)).wrapping_add(pt_off);
                if m.i16(closed_off) != 0 && m.i16(trim(m)) == 0 {
                    // si_nurbsInterv(copy's direction, x, 0) with the left-hand static of the original
                    let cd = copy.wrapping_add(dir_off);
                    let rec: NurbsDir = m.read(Ptr32::new(cd));
                    let kv = crate::fit2::knot_vec(m, &rec);
                    let mut st = Statics { left: m.i32(LEFT) };
                    let r = crate::knots::interv(&mut st, Dir { rec: &rec, knots: &kv }, m.f64(at), 0);
                    m.set_i32(LEFT, st.left);
                    // The port stops where the original would not end or read outside the array.
                    let idx = r.unwrap_or(0);
                    let diff = x(m.f64(at)) - x(m.f64(at8(m.u32(cd + 0xc), idx)));
                    let k = if idx == 0 && (if u { (m.i32(order_off).wrapping_sub(3) as u32) <= 1 } else { m.i32(order_off) == 4 }) {
                        1
                    } else {
                        m.i32(order_off).wrapping_sub(4).wrapping_add(m.i32(count_off).wrapping_sub(idx))
                    };
                    let v = x(m.f64(at8(m.u32(knots_off), k))) - diff;
                    m.set_f64(at, v.to_f64());
                } else {
                    let v = span - x(m.f64(at));
                    m.set_f64(at, v.to_f64());
                }
                pi += 1;
            }
            ci += 1;
        }
        let line = m.u32(trim(m) + 0xc);
        if line != 0 {
            m.set_i32(line + 4, m.i32(line + 4).wrapping_neg());
            let (lo_off, hi_off, node_off) = if u { (0x10, 0x20, 0) } else { (0x28, 0x18, 8) };
            let old = x(m.f64(line + lo_off));
            let v = span - x(m.f64(line + hi_off));
            m.set_f64(line + lo_off, v.to_f64());
            let v = span - old;
            m.set_f64(line + hi_off, v.to_f64());
            let mut node = m.u32(line + 8);
            while node != 0 {
                let v = span - x(m.f64(node + node_off));
                m.set_f64(node + node_off, v.to_f64());
                node = m.u32(node + 0x14);
            }
            si_trm_cvr_rve(m, line);
        }
        let n = m.i32(trim(m) + 4);
        let mut a: i32 = 0;
        while a < n / 2 {
            let arr = m.u32(trim(m) + 8);
            let held = m.u32(arr.wrapping_add(4 * a as u32));
            let other = m.u32(arr.wrapping_add(4u32.wrapping_mul((n - 1 - a) as u32)));
            m.set_u32(arr.wrapping_add(4 * a as u32), other);
            let arr = m.u32(trim(m) + 8);
            m.set_u32(arr.wrapping_add(4u32.wrapping_mul((n - a) as u32)).wrapping_sub(4), held);
            a += 1;
        }
        ti += 1;
    }
}

/// [soft 0x084dcaf8 SI_nurbsSrfInverse]: `(srf, u, v)`. A copy of the surface is made first (`SI_nurbsSrfCopy`). For `u`
/// non-zero: the net's columns exchanged end for end from 0 (`SI_gblIndex` records each pair), the span (open: knot
/// `count - order + 1` less knot `order - 2`; closed: last less first) taken as a double, the direction reversed
/// (`si_nurbsDirInverse`), then the trims as [`mirror_trims`] says. The same for `v` with V (a closed V's test for a
/// point placed through the copy's knot is `order == 4` where U's is `order` 3 or 4, as read). Then
/// `SI_knotsSrfReparametrization`, the copy disposed, answer 1.
pub fn si_nurbs_srf_inverse(m: &mut Mem, o: &mut dyn Outside, s: u32, u: i16, v: i16) -> i32 {
    let copy = crate::image::srf_copy(m, o, s);
    let slot = m.local(4);
    m.set_u32(slot, copy);
    for (flag, is_u) in [(u, true), (v, false)] {
        if flag == 0 {
            continue;
        }
        swap_net(m, s, is_u, 0);
        let (closed_off, order_off, count_off, knots_off, dir) =
            if is_u { (s + 0x20, s, s + 8, s + 0xc, s) } else { (s + 0x44, s + 0x24, s + 0x2c, s + 0x30, s + 0x24) };
        let kn = m.u32(knots_off);
        let (hi, lo) = if m.i16(closed_off) == 0 {
            (m.i32(count_off).wrapping_sub(m.i32(order_off)).wrapping_add(1), m.i32(order_off).wrapping_sub(2))
        } else {
            (m.i32(count_off).wrapping_sub(1), 0)
        };
        let span = F80::from_f64((x(m.f64(at8(kn, hi))) - x(m.f64(at8(kn, lo)))).to_f64());
        dir_inverse(m, dir);
        mirror_trims(m, o, s, copy, span, is_u);
    }
    knots_srf_reparametrization(m, o, s);
    crate::image::srf_dispose(m, o, slot);
    m.release(4);
    1
}

/// `_DAT_08661e38`: a vector shorter than this is left as it is by `nurbs_frenet_coord`.
const FRENET_EPS: f64 = 0.0001;

/// One vector of the coordinate system at `a`, scaled to length one when the length (the root of the three squares
/// added x, y, z, rounded to a float) is above 0.0001.
fn frenet_normalise(m: &mut Mem, a: u32) {
    let (vx, vy, vz) = (x(m.f32(a)), x(m.f32(a + 4)), x(m.f32(a + 8)));
    let len = ((vx * vx + vy * vy) + vz * vz).sqrt().to_f32();
    if x(FRENET_EPS).lt(x(len).abs()) {
        let l = x(len);
        m.set_f32(a, (vx / l).to_f32());
        m.set_f32(a + 4, (vy / l).to_f32());
        m.set_f32(a + 8, (vz / l).to_f32());
    }
}

fn frenet_copy_from(m: &mut Mem, out: u32, src: u32) {
    for k in 0..9u32 {
        let v = m.u32(src + 4 * k);
        m.set_u32(out + 4 * k, v);
    }
    for k in 0..3u32 {
        frenet_normalise(m, out + 12 * k);
    }
}

/// [soft 0x08502904 nurbsFrenetCoord]: `(crv, t, frames_slot, out)`: the coordinate system (nine floats: tangent, normal,
/// binormal, each made unit) of the curve at parameter `t`, from the array of frames (36 bytes each, one per parameter
/// of the list at +0x54, count at +0x50) that `*frames_slot` points at. `update` stands for `SI_nurbsUpdateFrenet (0x085020a8)`,
/// which the original calls when `*frames_slot` is still 0. Open curve (+0x20 = 0): past the end the last frame, before
/// the start the first, else interpolation by bisection of the list; closed: `t` is brought into the range (subtract the
/// end while above it, add the start while below it, at most the end), then the same bisection.
pub fn nurbs_frenet_coord(m: &mut Mem, crv: u32, t: f64, frames_slot: u32, out: u32, update: &mut dyn FnMut(&mut Mem, u32, u32)) {
    for k in 0..9u32 {
        m.set_u32(out + 4 * k, 0);
    }
    if m.u32(frames_slot) == 0 {
        update(m, crv, frames_slot);
    }
    let params = m.u32(crv + 0x54);
    let (start, end) = (x(m.f64(crv + 0x10)), x(m.f64(crv + 0x18)));
    let last = m.i32(crv + 0x50).wrapping_sub(1);
    let mut t = x(t);
    let frame = |m: &Mem, i: i32| m.u32(frames_slot).wrapping_add(0x24u32.wrapping_mul(i as u32));
    if m.i16(crv + 0x20) == 0 {
        if end.lt(t) {
            let f = frame(m, last);
            frenet_copy_from(m, out, f);
            return;
        }
        if !start.le(t) {
            let f = frame(m, 0);
            frenet_copy_from(m, out, f);
            return;
        }
    } else {
        while end.lt(t) {
            t = t - end;
        }
        while t.lt(start) {
            t = t + start;
        }
        if end.lt(t) {
            t = end;
        }
    }
    let (mut lo, mut hi) = (0i32, last);
    let mut base = m.f64(params);
    while hi - lo > 1 {
        let mid = lo.wrapping_add(hi) / 2;
        let p = m.f64(params.wrapping_add(8u32.wrapping_mul(mid as u32)));
        if x(p).le(t) {
            lo = mid;
            base = p;
        } else {
            hi = mid;
        }
    }
    if hi == lo {
        let f = frame(m, hi);
        frenet_copy_from(m, out, f);
        return;
    }
    let dt = t - x(base);
    let (fa, fb) = (frame(m, lo), frame(m, hi));
    for k in 0..9u32 {
        let a = x(m.f32(fa + 4 * k));
        let b = x(m.f32(fb + 4 * k));
        m.set_f32(out + 4 * k, (a + (b - a) * dt).to_f32());
        if k % 3 == 2 {
            frenet_normalise(m, out + 12 * (k / 3));
        }
    }
}

/// [soft 0x084eec00 SI_nurbsCrvIndexHit]: `(crv, idx_out, t_slot, flag)`: the index of the control point a parameter
/// belongs to. With `flag` 0 the parameter is first read from the curve's sampled knot vector (`SI_SampleKnotVector` at
/// the step in +0x34) at the index `*idx_out` already holds. The span is found (`si_nurbsInterv`, open when +0x20 is 0),
/// the parameter snapped to a knot it meets (`SI_nurbsKnotsCorrectMultAndValue`, one insertion asked): when that leaves
/// no insertion (multiplicity too high) the answer is 0. Otherwise the point is that of the largest basis value of the
/// span, shifted by the larger neighbour: `i = 0` gives `first + 1`, `i = order - 1` gives `order + first - 1`, else `first + i` if
/// `b[i+1] < b[i-1]` else `first + i + 1`; above the point count it wraps down by the count. Writes `*idx_out` and answers 1.
/// A span search that would not end, or a refused sampling, answers 0 here.
pub fn nurbs_crv_index_hit(m: &mut Mem, crv: u32, idx_out: u32, t_slot: u32, flag: i16) -> i32 {
    let (mut arr_slot, mut cnt_slot) = (m.local(4), m.local(4));
    m.set_u32(arr_slot, 0);
    m.set_i32(cnt_slot, 0);
    let mut r = 1i32;
    let mut sampled = 0u32;
    if flag == 0 {
        let per = m.i32(crv.wrapping_add(0x34));
        if crate::image::sample_knot_vector(m, crv, per, arr_slot, cnt_slot).is_err() {
            m.release(8);
            return 0;
        }
        sampled = m.u32(arr_slot);
        let i = m.i32(idx_out);
        let v = m.f64(sampled.wrapping_add(8u32.wrapping_mul(i as u32)));
        m.set_f64(t_slot, v);
    }
    let closed16 = m.u16(crv.wrapping_add(0x20));
    let rec: crate::NurbsDir = m.read(si_types::Ptr32::new(crv));
    let kv = crate::fit2::knot_vec(m, &rec);
    let mut st = crate::Statics { left: m.i32(crate::image::LEFT) };
    let left = crate::knots::interv(&mut st, crate::Dir { rec: &rec, knots: &kv }, m.f64(t_slot), (closed16 == 0) as i16);
    m.set_i32(crate::image::LEFT, st.left);
    let left = match left {
        Some(v) => v,
        None => {
            m.release(8);
            return 0;
        }
    };
    let mut t = m.f64(t_slot);
    if crate::insert::knots_correct_mult_and_value(crate::Dir { rec: &rec, knots: &kv }, &mut t, left, &mut r).is_none() {
        m.release(8);
        return 0;
    }
    m.set_f64(t_slot, t);
    if r == 0 {
        m.release(8);
        return 0;
    }
    let first = m.local(4);
    let mut mat = crate::fit2::nurbs_matrix_basis_function(m, crv, t, first, 0);
    let b = crate::fit2::nurbs_crv_basis_functions(m, mat, 0);
    let order = m.i32(crv);
    let first_v = m.i32(first);
    let at = |m: &Mem, i: i32| x(m.f64(b.wrapping_add(8u32.wrapping_mul(i as u32))));
    let mut best = 0i32;
    let mut k = 1i32;
    while k < order {
        if at(m, best).lt(at(m, k)) {
            best = k;
        }
        k += 1;
    }
    let mut res;
    if best == 0 {
        res = first_v.wrapping_add(1);
    } else if best == order.wrapping_sub(1) {
        res = order.wrapping_add(first_v).wrapping_sub(1);
    } else if at(m, best + 1).lt(at(m, best - 1)) {
        res = first_v.wrapping_add(best);
    } else {
        res = best.wrapping_add(first_v).wrapping_add(1);
    }
    let count = m.i32(crv.wrapping_add(4));
    if count < res {
        res = res.wrapping_sub(count);
    }
    let slot = m.local(4);
    m.set_u32(slot, mat);
    crate::fit2::mat_dispose(m, slot);
    mat = 0;
    let _ = mat;
    if b != 0 {
        m.free(b);
    }
    if sampled != 0 {
        m.free(sampled);
    }
    m.set_i32(idx_out, res);
    m.release(20);
    arr_slot = 0;
    cnt_slot = 0;
    let _ = (arr_slot, cnt_slot);
    1
}
