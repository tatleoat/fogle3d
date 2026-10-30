// SPDX-License-Identifier: MIT
//! Evaluation: a curve's point and derivatives, the one-span curve across a surface, a
//! surface's point.
//!
//! The method is de Boor's algorithm (C. de Boor, *A Practical Guide to Splines*; the
//! triangular scheme on the `order` control points of the span), in homogeneous
//! coordinates, with the derivative taken by differencing the first levels of the triangle.
//! Where the original departs from the published form is said at each routine.

use crate::knots::interv;
use crate::own::{knots_allocate, srf_swap_uv, Curve, Host, Surface};
use crate::{co, set_coord, CrvRef, Dir, NurbsCrv, NurbsPoint, SrfRef, Statics, EPS};
use si_types::{Double, Zero};
use si_x87::F80;

/// [soft 0x084d51f8 si_binom]: the binomial coefficient "n over k" in 32-bit integers:
/// 1 for `k` = 0 or `k` = `n`; `n` for `k` = 1 or `k` = `n − 1`; else, with
/// `m = max(k, n − k)`, the product `n × (n − 1) × … × (m + 1)` divided by the factorial of
/// the other of the two (products wrap; `n` = 13 is the first whose product does, for
/// `k` = 6: 13 × 12 × … × 8 = 1,235,520 does not, but 14 over 7 does).
///
/// `None` where the original divides by zero (a negative `k` or `n` can make the divisor
/// product wrap to 0; none of the evaluators passes one).
pub fn binom(n: i32, k: i32) -> Option<i32> {
    if k == 0 || n == k {
        return Some(1);
    }
    if k == 1 || k == n.wrapping_sub(1) {
        return Some(n);
    }
    let other = n.wrapping_sub(k);
    let m = if k >= other { k } else { other };
    let mut num = n;
    let mut i = n.wrapping_sub(1);
    while i > m {
        num = num.wrapping_mul(i);
        i -= 1;
    }
    let mut den = if m == k { other } else { k };
    let mut i = den.wrapping_sub(1);
    while i > 1 {
        den = den.wrapping_mul(i);
        i -= 1;
    }
    if den == 0 || (num == i32::MIN && den == -1) {
        return None;
    }
    Some(num / den)
}

/// The distances from the parameter to the knots on either side of the span, as every
/// evaluator forms them first: `dl[i] = t − knots[left − i]` and
/// `dr[i] = knots[left + 1 + i] − t` for `i` in `0 .. order − 1`, each rounded to `double`.
///
/// On a closed direction a knot index outside the vector is reached through the period:
/// for `i > left`, `dl[i] = ((t − knots[0]) + knots[last]) − knots[left + nb_knots − 1 − i]`;
/// for `left + 1 + i ≥ nb_knots`,
/// `dr[i] = ((knots[last] − t) + knots[left + 2 + i − nb_knots]) − knots[0]`.
/// Both are computed in the register and rounded once, **except** in the segment extraction
/// (`two_step`), which stores `(t − knots[0]) + knots[last]` as a `double` before the last
/// subtraction.
pub fn knot_distances(d: Dir, left: i32, t: f64, two_step: bool) -> Option<(Vec<f64>, Vec<f64>)> {
    let deg = d.rec.order.wrapping_sub(1);
    let nk = d.rec.nb_knots;
    let closed = d.rec.closed != 0;
    let t = F80::from_f64(t);
    let mut dl = Vec::new();
    let mut dr = Vec::new();
    for i in 0..deg.max(0) {
        let v = if !closed || i <= left {
            t - d.k(left.wrapping_sub(i))?
        } else {
            let a = (t - d.k(0)?) + d.k(nk.wrapping_sub(1))?;
            let a = if two_step { F80::from_f64(a.to_f64()) } else { a };
            a - d.k(left.wrapping_add(nk).wrapping_sub(1).wrapping_sub(i))?
        };
        dl.push(v.to_f64());
    }
    for i in 0..deg.max(0) {
        let j = left.wrapping_add(1).wrapping_add(i);
        let v = if !closed || j < nk {
            d.k(j)? - t
        } else {
            ((d.k(nk.wrapping_sub(1))? - t) + d.k(j.wrapping_sub(nk).wrapping_add(1))?) - d.k(0)?
        };
        dr.push(v.to_f64());
    }
    Some((dl, dr))
}

/// The index of the first control point of a span: `left − order + 2`, plus the point count
/// when that is negative (once).
pub fn first_point(left: i32, order: i32, n: i32) -> i32 {
    let f = left.wrapping_sub(order).wrapping_add(2);
    if f < 0 {
        f.wrapping_add(n)
    } else {
        f
    }
}

/// `(a % n)` as the original's `idiv` gives it; `None` for the divisions that trap.
fn rem(a: i32, n: i32) -> Option<i32> {
    if n == 0 || (a == i32::MIN && n == -1) {
        None
    } else {
        Some(a % n)
    }
}

/// The `order` control points of a curve's span, each index taken modulo the point count.
pub fn span_points(c: CrvRef, first: i32) -> Option<Vec<NurbsPoint>> {
    let n = c.rec.dir.nb_points;
    let mut p = Vec::new();
    for j in 0..c.rec.dir.order {
        let i = rem(first.wrapping_add(j), n)?;
        p.push(*c.points.get(usize::try_from(i).ok()?)?);
    }
    Some(p)
}

/// One level of de Boor's triangle on `m` pairs of neighbours, `p[b]` taking
/// `p[b + 1] + a × (p[b] − p[b + 1])` with `a = dr[b] / (dl[m − 1 − b] + dr[b])` kept in the
/// register (difference, product and sum too; one rounding to `double` a coordinate).
///
/// **Departures from the published algorithm**: when `1 − a` is below 1e-6 the point is
/// left as it is, and when `a` is below 1e-6 it becomes a copy of its neighbour (all 36
/// bytes, the flag word included), both without arithmetic. A NaN factor fails both tests
/// and blends.
///
/// The two tests are one-sided, so they also **clamp the factor to [0, 1]**: a parameter
/// outside the range (the span search clamps only its own copy) gives factors above 1 or
/// below 0, and each such factor keeps or copies a point. Far enough beyond an end, every
/// factor does, and the result is the last (first) control point of the end span: the end
/// point of a clamped curve, a point off the curve for an unclamped one.
pub fn blend_level(p: &mut [NurbsPoint], dl: &[f64], dr: &[f64], m: i32) -> Option<()> {
    let eps = F80::from_f64(EPS);
    for b in 0..m.max(0) as usize {
        let li = m as usize - 1 - b;
        let r = F80::from_f64(*dr.get(b)?);
        let a = r / (F80::from_f64(*dl.get(li)?) + r);
        if (F80::ONE - a).lt(eps) {
            continue;
        }
        let next = *p.get(b + 1)?;
        let cur = p.get_mut(b)?;
        if a.lt(eps) {
            *cur = next;
        } else {
            for q in 0..4 {
                let v = co(&next, q) + a * (co(cur, q) - co(&next, q));
                set_coord(cur, q, v.to_f64());
            }
        }
    }
    Some(())
}

/// One derivative level: `p[b] = (p[b + 1] − p[b]) × f` with
/// `f = m / (dl[m − 1 − b] + dr[b])` in the register, one rounding a coordinate. No
/// tolerance: a span of zero length divides by zero.
pub fn difference_level(p: &mut [NurbsPoint], dl: &[f64], dr: &[f64], m: i32) -> Option<()> {
    for b in 0..m.max(0) as usize {
        let li = m as usize - 1 - b;
        let f = F80::from_i32(m) / (F80::from_f64(*dl.get(li)?) + F80::from_f64(*dr.get(b)?));
        let next = *p.get(b + 1)?;
        let cur = p.get_mut(b)?;
        for q in 0..4 {
            let v = (co(&next, q) - co(cur, q)) * f;
            set_coord(cur, q, v.to_f64());
        }
    }
    Some(())
}

/// [soft 0x084d3550 SI_nurbsCrvValue]: the `nd`-th derivative of a curve at `t` (the point
/// for `nd` = 0), as three doubles.
///
/// 1. The span: [`interv`] with "open" = not closed; then [`knot_distances`].
/// 2. For each derivative order `d` from 0 to `nd` the `order` points of the span are taken
///    afresh, `d` levels of differences are applied ([`difference_level`], on `degree`,
///    `degree − 1`, … pairs), then the remaining `degree − d` levels of blending
///    ([`blend_level`]); the first point is the homogeneous derivative `A⁽ᵈ⁾` (x, y, z, w).
/// 3. `inv = 1 / w⁽⁰⁾`, or 1 when `|w⁽⁰⁾|` is not above 1e-6 (**a point of zero weight is
///    returned undivided**); kept in the register.
/// 4. The rational derivative by the quotient rule, in the order
///    `C⁽ᵈ⁾ = (A⁽ᵈ⁾ − Σ_{j<d} binom(d, j) × w⁽ᵈ⁻ʲ⁾ × C⁽ʲ⁾) × inv`, `j` ascending, each
///    subtraction rounded to `double`, the product `binom × w` kept in the register.
///
/// A derivative order above the degree does not give zero: the difference levels beyond
/// the degree have nothing to do and the last real one is returned again.
///
/// `None`: the span search does not end, a read outside the knots or points, a point count
/// of zero (a division trap), an order below 1 or a negative `nd` (the original then
/// returns what its stack held).
pub fn crv_value(st: &mut Statics, c: CrvRef, t: f64, nd: i32) -> Option<[f64; 3]> {
    crv_value_parts(st, c, t, nd).map(|p| p.value)
}

/// What [`crv_value`] leaves in the original's scratch (the arrays the routine fills before it
/// gives them back; they sit on the stack below order 5, on the heap from order 5, and a heap
/// compare sees the words in the freed blocks).
pub struct CrvScratch {
    /// The result.
    pub value: [f64; 3],
    /// The two knot-distance arrays (`order - 1` doubles each).
    pub dl: Vec<f64>,
    pub dr: Vec<f64>,
    /// The de Boor table as the last derivative order (`nd`) leaves it (`order` points).
    pub table: Vec<NurbsPoint>,
    /// The homogeneous derivatives `A(0)..A(nd)`, one point each (the nd+1 table of the heap form).
    pub derivs: Vec<NurbsPoint>,
    /// The rational derivatives `C(0)..C(nd)` (x, y, z; the nd+1 array of 0x18 bytes a record).
    pub rational: Vec<[f64; 3]>,
}

/// [`crv_value`] with the scratch it leaves behind.
pub fn crv_value_parts(st: &mut Statics, c: CrvRef, t: f64, nd: i32) -> Option<CrvScratch> {
    let d = c.dir();
    let left = interv(st, d, t, (d.rec.closed == 0) as i16)?;
    let order = d.rec.order;
    let deg = order.wrapping_sub(1);
    let (dl, dr) = knot_distances(d, left, t, false)?;
    let first = first_point(left, order, d.rec.nb_points);
    if order < 1 || nd < 0 {
        return None;
    }
    let mut a: Vec<NurbsPoint> = Vec::new();
    let mut table: Vec<NurbsPoint> = Vec::new();
    for dd in 0..=nd {
        let mut p = span_points(c, first)?;
        for cc in 0..dd {
            difference_level(&mut p, &dl, &dr, deg.wrapping_sub(cc))?;
        }
        for cc in dd..deg {
            blend_level(&mut p, &dl, &dr, deg - cc)?;
        }
        a.push(p[0]);
        table = p;
    }
    let w0 = F80::from_f64(a[0].w.0);
    let inv = if w0.abs().gt(F80::from_f64(EPS)) { F80::ONE / w0 } else { F80::ONE };
    let mut r: Vec<[f64; 3]> = Vec::new();
    for dd in 0..=nd as usize {
        let mut v = [si_x87::fmov64(a[dd].x.0), si_x87::fmov64(a[dd].y.0), si_x87::fmov64(a[dd].z.0)];
        for j in 0..dd {
            let f = F80::from_i32(binom(dd as i32, j as i32)?) * F80::from_f64(a[dd - j].w.0);
            for q in 0..3 {
                v[q] = (F80::from_f64(v[q]) - f * F80::from_f64(r[j][q])).to_f64();
            }
        }
        for q in v.iter_mut() {
            *q = (F80::from_f64(*q) * inv).to_f64();
        }
        r.push(v);
    }
    Some(CrvScratch { value: r[nd as usize], dl, dr, table, derivs: a, rational: r })
}

/// [soft 0x084d3ca0 SI_nurbsCrvValue4D]: the homogeneous point of a curve at `t`: steps 1
/// and 2 of [`crv_value`] for the point only, without the division. The result is a whole
/// control-point record, so it carries a **flag word**: that of the span's first point, or
/// of a later one where a blend factor below 1e-6 copied a point over it.
pub fn crv_value4d(st: &mut Statics, c: CrvRef, t: f64) -> Option<NurbsPoint> {
    let d = c.dir();
    let left = interv(st, d, t, (d.rec.closed == 0) as i16)?;
    let order = d.rec.order;
    let deg = order.wrapping_sub(1);
    let (dl, dr) = knot_distances(d, left, t, false)?;
    let first = first_point(left, order, d.rec.nb_points);
    if order < 1 {
        return None;
    }
    let mut p = span_points(c, first)?;
    for cc in 0..deg {
        blend_level(&mut p, &dl, &dr, deg - cc)?;
    }
    Some(p[0])
}

/// The numerical part of [`srf_extract_curve_seg`]: fills `out_points[..order_v]`,
/// `out_knots[..out.nb_knots]` and the range of `out`, whose order and knot count are
/// given.
/// The working arrays that `SI_nurbsSrfExtractCurveSeg` leaves in its static scratch blocks (`dl`, `dr`: `order_u - 1` doubles
/// each; `aj`: `order_u * order_v` points, entry `j * order_u + i`), as the extraction at `nd` leaves them.
pub type SegScratch = (Vec<f64>, Vec<f64>, Vec<NurbsPoint>);

pub fn seg_kernel(st: &mut Statics, s: SrfRef, u: f64, v: f64, nd: i32, out: &mut NurbsCrv, out_knots: &mut [f64], out_points: &mut [NurbsPoint]) -> Option<SegScratch> {
    let (du, dv) = (s.u(), s.v());
    let lu = interv(st, du, u, (du.rec.closed == 0) as i16)?;
    let lv = interv(st, dv, v, (dv.rec.closed == 0) as i16)?;
    let (nu, ou) = (du.rec.nb_points, du.rec.order);
    let (nv, ov) = (dv.rec.nb_points, dv.rec.order);
    let deg = ou.wrapping_sub(1);
    if (ou as i64) * (ov as i64) > 1 << 20 {
        return None;
    }
    let (dl, dr) = knot_distances(du, lu, u, true)?;
    let fu = first_point(lu, ou, nu);
    let fv = first_point(lv, ov, nv);
    // aj[j × ou + i]: column i of the span in U, row j of the span in V
    let mut aj = vec![NurbsPoint::zero(); (ou.max(0) as usize) * (ov.max(0) as usize)];
    let mut iu = fu;
    for i in 0..ou.max(0) {
        if fu.wrapping_add(i) == nu {
            iu = 0;
        }
        let mut src = fv.wrapping_mul(nu).wrapping_add(iu);
        for j in 0..ov.max(0) {
            if fv.wrapping_add(j) == nv {
                src = iu;
            }
            aj[(j * ou + i) as usize] = *s.points.get(usize::try_from(src).ok()?)?;
            src = src.wrapping_add(nu);
        }
        iu = iu.wrapping_add(1);
    }
    let ou_ = ou.max(0) as usize;
    let rows = ov.max(0) as usize;
    // derivative levels: the difference is stored as a double, then the product
    for cc in 0..nd.max(0) {
        let m = deg.wrapping_sub(cc);
        for b in 0..m.max(0) as usize {
            let li = m as usize - 1 - b;
            let f = F80::from_i32(m) / (F80::from_f64(*dl.get(li)?) + F80::from_f64(*dr.get(b)?));
            for j in 0..rows {
                let next = *aj.get(j * ou_ + b + 1)?;
                let cur = aj.get_mut(j * ou_ + b)?;
                for q in 0..4 {
                    let diff = (co(&next, q) - co(cur, q)).to_f64();
                    set_coord(cur, q, (F80::from_f64(diff) * f).to_f64());
                }
            }
        }
    }
    let eps = F80::from_f64(EPS);
    let mut cc = nd;
    while cc < deg {
        let m = deg - cc;
        for b in 0..m.max(0) as usize {
            let li = m as usize - 1 - b;
            let r = F80::from_f64(*dr.get(b)?);
            let a = r / (F80::from_f64(*dl.get(li)?) + r);
            if (F80::ONE - a).lt(eps) {
                continue;
            }
            let copy = a.lt(eps);
            for j in 0..rows {
                let next = *aj.get(j * ou_ + b + 1)?;
                let cur = aj.get_mut(j * ou_ + b)?;
                if copy {
                    *cur = next;
                } else {
                    for q in 0..4 {
                        let val = co(&next, q) + a * (co(cur, q) - co(&next, q));
                        set_coord(cur, q, val.to_f64());
                    }
                }
            }
        }
        cc += 1;
    }
    for j in 0..rows {
        *out_points.get_mut(j)? = aj[j * ou_];
    }
    let scratch = (dl, dr, aj);
    // the knots of the span in V, through the period where the index leaves the vector
    let nkv = dv.rec.nb_knots;
    let base = lv.wrapping_sub(out.dir.order).wrapping_add(2);
    let mut i = 0;
    while i < out.dir.nb_knots {
        let idx = base.wrapping_add(i);
        let k = if idx < 0 {
            (dv.k(0)? + dv.k(idx.wrapping_add(nkv).wrapping_sub(1))?) - dv.k(nkv.wrapping_sub(1))?
        } else if idx < nkv {
            dv.k(idx)?
        } else {
            (dv.k(nkv.wrapping_sub(1))? + dv.k(idx.wrapping_sub(nkv).wrapping_add(1))?) - dv.k(0)?
        };
        *out_knots.get_mut(i as usize)? = k.to_f64();
        i += 1;
    }
    let at = |i: i32| usize::try_from(i).ok();
    out.dir.range_start = Double(*out_knots.get(at(out.dir.order.wrapping_sub(2))?)?);
    out.dir.range_end = Double(*out_knots.get(at(out.dir.nb_knots.wrapping_sub(out.dir.order).wrapping_add(1))?)?);
    Some(scratch)
}

/// The body of [`srf_extract_curve_seg`] between its two exchanges of U and V: the
/// one-span curve in V of a surface view at (`u`, `v`). `out`: the caller's curve, or
/// `None` to have one made (returned).
pub fn seg_of_view(st: &mut Statics, s: SrfRef, u: f64, v: f64, nd: i32, out: Option<&mut Curve>) -> Option<Option<Curve>> {
    let mut made = None;
    let c = match out {
        Some(c) => c,
        None => {
            // SI_calloc(1, 100), then SI_calloc for order_v points and 2 × order_v − 2 knots
            let ov = s.rec.v.order;
            let mut c = Curve { rec: NurbsCrv::zero(), knots: None, points: None, length_param: None, length_dist: None };
            c.points = (ov > 0 && ov < 1 << 20).then(|| vec![NurbsPoint::zero(); ov as usize]);
            c.rec.dir.order = ov;
            c.rec.dir.nb_points = ov;
            c.rec.dir.nb_knots = ov.wrapping_add(ov).wrapping_sub(2);
            c.knots = knots_allocate(c.rec.dir.nb_knots);
            c.sync();
            made.insert(c)
        }
    };
    let (had_k, had_p) = (c.knots.is_some(), c.points.is_some());
    let (mut knots, mut points) = (c.knots.take().unwrap_or_default(), c.points.take().unwrap_or_default());
    let r = seg_kernel(st, s, u, v, nd, &mut c.rec, &mut knots, &mut points);
    c.knots = had_k.then_some(knots);
    c.points = had_p.then_some(points);
    r.map(|_| made)
}

/// [soft 0x084d603c SI_nurbsSrfExtractCurveSeg]: the **one-span** curve in V that the
/// surface is at the parameter `u`, valid for the V span that holds `v`: an open curve of
/// `order_v` points and `2 × order_v − 2` knots. With `nd` above 0 it is the curve of the
/// `nd`-th derivative with respect to U instead. Every surface evaluator stands on this.
///
/// - The spans of `u` and `v` are found ([`interv`]); the `order_u × order_v` control points
///   are gathered, each index wrapping to 0 exactly once (when it reaches the count).
/// - De Boor's triangle in U on all rows at once: `nd` difference levels, where the
///   difference is **stored as a `double` and then multiplied** (two roundings; the curve
///   evaluator rounds once), then the blending levels with the same two 1e-6 short cuts as
///   the curve evaluator, decided once a factor for all rows.
/// - The curve's knots are the `2 × order_v − 2` knots of V around the span, taken through
///   the period where the index leaves the vector (a closed V), and its range is read from
///   them.
///
/// The new curve is made with `SI_calloc`, not with the curve allocator: order `order_v`,
/// not closed, **every other field zero** (step 0, closing 0). When the caller supplies a
/// curve (`out`), its order and knot count are trusted and only its points, knots and
/// range are written.
///
/// With `swap` = 1 the surface's directions are exchanged before and after
/// ([`srf_swap_uv`], which reallocates the points both times) and the two parameters once:
/// the result is the curve in U at `v`. An exchange that refuses (a surface without
/// points) still exchanges the parameters.
pub fn srf_extract_curve_seg(st: &mut Statics, host: &mut dyn Host, srf: &mut Surface, u: f64, v: f64, swap: i32, nd: i32, out: Option<&mut Curve>) -> Option<Option<Curve>> {
    let (mut u, mut v) = (u, v);
    if swap == 1 {
        srf_swap_uv(host, srf)?;
        (u, v) = (si_x87::fmov64(v), si_x87::fmov64(u));
    }
    let r = seg_of_view(st, srf.view(), u, v, nd, out);
    if swap == 1 {
        srf_swap_uv(host, srf)?;
    }
    r
}

/// [soft 0x084d6728 SI_nurbsSrfValue]: a surface's point at (u, v): the one-span curve of
/// [`srf_extract_curve_seg`] at `u` (without exchange), evaluated at `v` by [`crv_value`]; the curve is then
/// disposed of.
pub fn srf_value(st: &mut Statics, s: SrfRef, u: f64, v: f64) -> Option<[f64; 3]> {
    let c = seg_of_view(st, s, u, v, 0, None)??;
    crv_value(st, c.view(), v, 0)
}

/// [soft 0x084d67a8 SI_nurbsSrfValue4D]: the same through [`crv_value4d`]: the homogeneous
/// point, flag word included.
pub fn srf_value4d(st: &mut Statics, s: SrfRef, u: f64, v: f64) -> Option<NurbsPoint> {
    let c = seg_of_view(st, s, u, v, 0, None)??;
    crv_value4d(st, c.view(), v)
}

/// The tolerance of the curve extraction's upper test (0x08661688), (0x08661698):
/// the double nearest 0.999999 (bits 0x3feffffde7210be9).
pub const NEAR_ONE: f64 = 0.999999;

/// The body shared by the two whole-curve extractions: the surface at the parameter `t` of
/// direction `across` (false: U, true: V) as a curve in the other direction.
fn extract_whole(st: &mut Statics, s: SrfRef, t: f64, nd: i32, across_v: bool) -> Option<Curve> {
    let (d, other) = if across_v { (s.v(), s.u()) } else { (s.u(), s.v()) };
    let left = interv(st, d, t, (d.rec.closed == 0) as i16)?;
    let order = d.rec.order;
    let deg = order.wrapping_sub(1);
    let n = d.rec.nb_points;
    let rows = other.rec.nb_points;
    let nu = s.rec.u.nb_points;
    let mut c = Curve::allocate();
    c.points = crate::own::points_allocate(rows);
    let (dl, dr) = knot_distances(d, left, t, false)?;
    // the factors of every level, each rounded to double before it is used
    let mut coef: Vec<f64> = Vec::new();
    for cc in 0..deg.max(0) {
        let m = deg - cc;
        for b in 0..m as usize {
            let li = m as usize - 1 - b;
            let sum = F80::from_f64(*dl.get(li)?) + F80::from_f64(*dr.get(b)?);
            let f = if cc < nd { F80::from_i32(m) / sum } else { F80::from_f64(*dr.get(b)?) / sum };
            coef.push(f.to_f64());
        }
    }
    let first = first_point(left, order, n);
    if order < 1 && rows > 0 {
        // the point taken for each row is one the original never loaded
        return None;
    }
    let (near_one, eps) = (F80::from_f64(NEAR_ONE), F80::from_f64(EPS));
    for j in 0..rows.max(0) {
        let mut p: Vec<NurbsPoint> = Vec::new();
        for i in 0..order {
            let k = rem(first.wrapping_add(i), n)?;
            let idx = if across_v { j.wrapping_add(k.wrapping_mul(nu)) } else { j.wrapping_mul(nu).wrapping_add(k) };
            p.push(*s.points.get(usize::try_from(idx).ok()?)?);
        }
        let mut q = 0usize;
        for cc in 0..deg {
            let m = deg - cc;
            for b in 0..m as usize {
                let a = F80::from_f64(coef[q]);
                q += 1;
                let next = p[b + 1];
                let cur = &mut p[b];
                if cc < nd {
                    for k in 0..4 {
                        let v = (co(&next, k) - co(cur, k)) * a;
                        set_coord(cur, k, v.to_f64());
                    }
                } else if a.lt(near_one) {
                    if a.lt(eps) {
                        *cur = next;
                    } else {
                        for k in 0..4 {
                            let v = co(&next, k) + a * (co(cur, k) - co(&next, k));
                            set_coord(cur, k, v.to_f64());
                        }
                    }
                }
            }
        }
        *c.points.as_mut()?.get_mut(j as usize)? = p[0];
    }
    c.rec.step = if across_v { s.rec.u_step } else { s.rec.v_step };
    c.rec.param = s.rec.param;
    c.rec.dir.order = other.rec.order;
    c.rec.dir.nb_points = other.rec.nb_points;
    c.rec.dir.nb_knots = other.rec.nb_knots;
    c.knots = knots_allocate(other.rec.nb_knots);
    if other.rec.nb_knots > 0 {
        let nk = other.rec.nb_knots as usize;
        c.knots.as_mut()?.get_mut(..nk)?.copy_from_slice(other.knots.get(..nk)?);
    }
    c.rec.dir.range_start = other.rec.range_start;
    c.rec.dir.range_end = other.rec.range_end;
    c.rec.dir.closed = other.rec.closed;
    c.sync();
    Some(c)
}

/// [soft 0x084d52a4 SI_nurbsSrfExtractCurveU]: the whole curve in V that the surface is at
/// the parameter `u` (an isoparametric line), or with `nd` above 0 the curve of its `nd`-th
/// derivative with respect to U. A new curve from the curve allocator, with V's order,
/// counts, knots (copied), range and closed flag, the surface's parameterisation and V's
/// step count.
///
/// De Boor's triangle in U, one row of the grid after the other. It differs from the point
/// evaluators in three places: the factors of all levels are computed once and **rounded
/// to `double`** before use; a blending factor is skipped (the point kept) when it is
/// **not below 0.999999** rather than when 1 minus it is below 1e-6, so a NaN factor keeps
/// the point; and the difference levels round once (difference times factor).
pub fn srf_extract_curve_u(st: &mut Statics, s: SrfRef, u: f64, nd: i32) -> Option<Curve> {
    extract_whole(st, s, u, nd, false)
}

/// [soft 0x084d5964 SI_nurbsSrfExtractCurveV]: the mirror image: the curve in U at the
/// parameter `v`, with U's direction and step count. Its constants are a second pair
/// (0x08661698, 0x086616a0) with the same bits.
pub fn srf_extract_curve_v(st: &mut Statics, s: SrfRef, v: f64, nd: i32) -> Option<Curve> {
    extract_whole(st, s, v, nd, true)
}

/// [soft 0x084d5268 SI_nurbsSrfExtractCurve]: `dir` 0 is [`srf_extract_curve_u`], anything
/// else [`srf_extract_curve_v`].
pub fn srf_extract_curve(st: &mut Statics, s: SrfRef, t: f64, dir: i32, nd: i32) -> Option<Curve> {
    if dir == 0 {
        srf_extract_curve_u(st, s, t, nd)
    } else {
        srf_extract_curve_v(st, s, t, nd)
    }
}

// ------------------------------------------------------------------------------ normal

/// The double nearest 0.001 (0x086616b0): below this length a derivative counts as
/// vanished.
pub const DERIV_EPS: f64 = 0.001;
/// The double nearest 0.0001 (0x086616b8): the distance from the range end within which
/// the search for a derivative steps backwards, and the least length a normal is divided by.
pub const NORMAL_EPS: f64 = 0.0001;
/// The step of the search for a derivative, (0x086616c8) and its negative
/// (0x086616c0): the doubles nearest ±0.01, in parameter units.
pub const NUDGE: f64 = 0.01;

fn length3(v: &[f64; 3]) -> F80 {
    let (x, y, z) = (F80::from_f64(v[0]), F80::from_f64(v[1]), F80::from_f64(v[2]));
    ((x * x + y * y) + z * z).sqrt()
}

/// The cartesian derivative across, from the homogeneous point of the derivative curve:
/// `(D − D.w × pos) × inv`, all in the register, one rounding a coordinate.
fn across(d4: &NurbsPoint, pos: &[f64; 3], inv: F80) -> [f64; 3] {
    let w = F80::from_f64(d4.w.0);
    core::array::from_fn(|q| (inv * (co(d4, q) - w * F80::from_f64(pos[q]))).to_f64())
}

/// The two steps of the search: −0.01 in a direction whose parameter lies within 0.0001 of
/// the range end, else +0.01.
fn nudges(s: SrfRef, u: f64, v: f64) -> (f64, f64) {
    let tol = F80::from_f64(NORMAL_EPS);
    let su = if (F80::from_f64(u) - F80::from_f64(s.rec.u.range_end.0)).abs().lt(tol) { -NUDGE } else { NUDGE };
    let sv = if tol.gt((F80::from_f64(v) - F80::from_f64(s.rec.v.range_end.0)).abs()) { -NUDGE } else { NUDGE };
    (su, sv)
}

/// [soft 0x084d6824 SI_nurbsSrfValueAndNormal]: a surface's point and unit normal at
/// (u, v). Returns (point, normal).
///
/// With `a` and `b` both given, they are the caller's two one-span curves (the position
/// curve and the curve of the derivative across it) and **both are evaluated at `u`**: the
/// caller extracted them with U and V exchanged. Otherwise the routine extracts them itself
/// ([`seg_of_view`] with derivative order 0 and 1) and evaluates them at `v`.
///
/// 1. `P` = homogeneous point of the position curve; `inv = 1 / P.w` with **no test of the
///    weight**; the point is `P.xyz × inv`, each rounded to `double`. This is the result
///    whatever happens below.
/// 2. `across = (D.xyz − D.w × point) × inv` from the homogeneous point `D` of the
///    derivative curve; `along` = first derivative of the position curve ([`crv_value`]).
/// 3. A derivative whose length (square root of the sum of squares, in the register) is
///    below 0.001 is searched for nearby. The steps are ±0.01 in each parameter (negative
///    within 0.0001 of that direction's range end). Up to 99 tries: tries 1 and 2 evaluate
///    the curve already extracted at `v` plus the accumulated V step; every third try
///    extracts anew at (`u` + accumulated U step, `v` + accumulated V step) and advances the
///    U step. The first derivative of sufficient length replaces the vanished one; after 99
///    tries the last one does. The point is not changed by the search.
///    - without `a`: first `across` is tested and searched for, then `along`;
///    - with `a`: first `along`, **searched for with the formula of `across`** (the
///      derivative curve is evaluated, not the position curve), then `across`, searched for
///      with the formula of `along`. Read as a slip of the original; reproduced.
/// 4. The normal is `across × along` without `a`, `along × across` with it, each coordinate
///    rounded to `double`; its length is rounded to **`float`**, and when that is above
///    0.0001 the three coordinates are divided by it. Otherwise the normal stays as the raw
///    cross product (zero for a degenerate surface).
///
/// Which branches depend on `a` alone and which on both curves is the original's: a caller
/// that passes only one of the two gets the extraction of "neither" and the tests of "a".
pub fn srf_value_and_normal(st: &mut Statics, s: SrfRef, u: f64, v: f64, a: Option<CrvRef>, b: Option<CrvRef>) -> Option<([f64; 3], [f64; 3])> {
    srf_value_and_normal_traced(st, s, u, v, a, b, &mut [0; 2])
}

/// [`srf_value_and_normal`], also counting in `tries` how many nearby evaluations the first
/// and the second search for a vanished derivative made (0: no search).
pub fn srf_value_and_normal_traced(st: &mut Statics, s: SrfRef, u: f64, v: f64, a: Option<CrvRef>, b: Option<CrvRef>, tries: &mut [u32; 2]) -> Option<([f64; 3], [f64; 3])> {
    let (ga, gb) = (a.map(VCrv::Given), b.map(VCrv::Given));
    srf_value_and_normal_with(&mut ValueEnv { st, s }, s, u, v, ga.as_ref(), gb.as_ref(), tries)
}

/// What [`srf_value_and_normal_with`] needs of its world: the one-span curve extraction, the two curve evaluators and the disposal.
/// [`ValueEnv`] keeps the curves in Rust values; the image form (`image::srf_value_and_normal`) makes them on the heap, as the
/// original does, and gives them back in the original's order.
pub trait SvnEnv {
    type C;
    /// [soft 0x084d603c SI_nurbsSrfExtractCurveSeg] with `swap` 0 and the derivative order `nd`.
    fn extract(&mut self, u: f64, v: f64, nd: i32) -> Option<Self::C>;
    /// [soft 0x084d3ca0 SI_nurbsCrvValue4D]
    fn value4d(&mut self, c: &Self::C, t: f64) -> Option<NurbsPoint>;
    /// [soft 0x084d3550 SI_nurbsCrvValue]
    fn value(&mut self, c: &Self::C, t: f64, nd: i32) -> Option<[f64; 3]>;
    /// [soft 0x084db870 SI_nurbsCrvDispose]
    fn dispose(&mut self, c: Self::C);
}

/// The curves as Rust values (no heap).
pub struct ValueEnv<'a, 'b> {
    pub st: &'a mut Statics,
    pub s: SrfRef<'b>,
}

/// A curve of [`ValueEnv`]: one it extracted, or one the caller gave.
pub enum VCrv<'c> {
    Own(Curve),
    Given(CrvRef<'c>),
}

impl VCrv<'_> {
    fn view(&self) -> CrvRef<'_> {
        match self {
            VCrv::Own(c) => c.view(),
            VCrv::Given(r) => *r,
        }
    }
}

impl<'c> SvnEnv for ValueEnv<'_, 'c> {
    type C = VCrv<'c>;
    fn extract(&mut self, u: f64, v: f64, nd: i32) -> Option<VCrv<'c>> {
        Some(VCrv::Own(seg_of_view(self.st, self.s, u, v, nd, None)??))
    }
    fn value4d(&mut self, c: &VCrv<'c>, t: f64) -> Option<NurbsPoint> {
        crv_value4d(self.st, c.view(), t)
    }
    fn value(&mut self, c: &VCrv<'c>, t: f64, nd: i32) -> Option<[f64; 3]> {
        crv_value(self.st, c.view(), t, nd)
    }
    fn dispose(&mut self, _c: VCrv<'c>) {}
}

/// [`srf_value_and_normal`] over an [`SvnEnv`]: `a` and `b` are the caller's curves (the original's `param_6` and the stack
/// argument); the curves the routine extracts itself are given back as the original gives them back, which leaves some of them
/// alive: with `a` given, the position curve is never disposed (and with only `b` given, the derivative curve is not).
pub fn srf_value_and_normal_with<E: SvnEnv>(env: &mut E, s: SrfRef, u: f64, v: f64, a: Option<&E::C>, b: Option<&E::C>, tries: &mut [u32; 2]) -> Option<([f64; 3], [f64; 3])> {
    let (u, v) = (si_x87::fmov64(u), si_x87::fmov64(v));
    let mut own: Option<(E::C, E::C)> = None;
    let tv;
    let (ca, cb) = match (a, b) {
        (Some(a), Some(b)) => {
            tv = u;
            (a, b)
        }
        _ => {
            tv = v;
            let ea = env.extract(u, v, 0)?;
            let eb = env.extract(u, v, 1)?;
            let o = own.insert((ea, eb));
            (&o.0, &o.1)
        }
    };
    let given = a.is_some();
    let p4 = env.value4d(ca, tv)?;
    let inv = F80::ONE / F80::from_f64(p4.w.0);
    let pos: [f64; 3] = core::array::from_fn(|q| (co(&p4, q) * inv).to_f64());
    let d4 = env.value4d(cb, tv)?;
    let mut du = across(&d4, &pos, inv);
    let mut dv = env.value(ca, tv, 1)?;
    let small = F80::from_f64(DERIV_EPS);

    let first_fails = if !given { small.gt(length3(&du)) } else { length3(&dv).lt(small) };
    if first_fails {
        let mut c0 = env.extract(u, v, 0)?;
        let mut c1 = env.extract(u, v, 1)?;
        let (su, sv) = nudges(s, u, v);
        let (mut acc_u, mut acc_v) = (su, sv);
        let mut pos2 = pos;
        let mut inv2 = inv;
        let mut t = if !given { du } else { dv };
        let mut count = 1;
        while length3(&t).lt(small) && count <= 99 {
            if count % 3 != 0 {
                let tv2 = (F80::from_f64(v) + F80::from_f64(acc_v)).to_f64();
                let d = env.value4d(&c1, tv2)?;
                t = across(&d, &pos2, inv2);
                acc_v = (F80::from_f64(acc_v) + F80::from_f64(sv)).to_f64();
            } else {
                let t1 = (F80::from_f64(v) + F80::from_f64(acc_v)).to_f64();
                let t2 = (F80::from_f64(u) + F80::from_f64(acc_u)).to_f64();
                env.dispose(c0);
                env.dispose(c1);
                c0 = env.extract(t2, t1, 0)?;
                c1 = env.extract(t2, t1, 1)?;
                let p = env.value4d(&c0, t1)?;
                inv2 = F80::ONE / F80::from_f64(p.w.0);
                pos2 = core::array::from_fn(|q| (co(&p, q) * inv2).to_f64());
                let d = env.value4d(&c1, t1)?;
                t = across(&d, &pos2, inv2);
                acc_u = (F80::from_f64(acc_u) + F80::from_f64(su)).to_f64();
            }
            count += 1;
        }
        tries[0] = count as u32;
        if !given {
            du = t;
        } else {
            dv = t;
        }
        env.dispose(c0);
        env.dispose(c1);
    }

    let second_fails = if !given { length3(&dv).lt(small) } else { length3(&du).lt(small) };
    if second_fails {
        let mut c0 = env.extract(u, v, 0)?;
        let (su, sv) = nudges(s, u, v);
        let (mut acc_u, mut acc_v) = (su, sv);
        let mut t = if !given { dv } else { du };
        let mut count = 1;
        while length3(&t).lt(small) && count <= 99 {
            if count % 3 != 0 {
                let tv2 = (F80::from_f64(v) + F80::from_f64(acc_v)).to_f64();
                t = env.value(&c0, tv2, 1)?;
                acc_v = (F80::from_f64(acc_v) + F80::from_f64(sv)).to_f64();
            } else {
                let t1 = (F80::from_f64(v) + F80::from_f64(acc_v)).to_f64();
                let t2 = (F80::from_f64(u) + F80::from_f64(acc_u)).to_f64();
                env.dispose(c0);
                c0 = env.extract(t2, t1, 0)?;
                t = env.value(&c0, t1, 1)?;
                acc_u = (F80::from_f64(acc_u) + F80::from_f64(su)).to_f64();
            }
            count += 1;
        }
        tries[1] = count as u32;
        if !given {
            dv = t;
        } else {
            du = t;
        }
        env.dispose(c0);
    }

    let (ax, ay, az) = (F80::from_f64(dv[0]), F80::from_f64(dv[1]), F80::from_f64(dv[2]));
    let (bx, by, bz) = (F80::from_f64(du[0]), F80::from_f64(du[1]), F80::from_f64(du[2]));
    let mut n = if given {
        [(ay * bz - az * by).to_f64(), (az * bx - bz * ax).to_f64(), (by * ax - bx * ay).to_f64()]
    } else {
        [(by * az - bz * ay).to_f64(), (bz * ax - az * bx).to_f64(), (ay * bx - ax * by).to_f64()]
    };
    let (nx, ny, nz) = (F80::from_f64(n[0]), F80::from_f64(n[1]), F80::from_f64(n[2]));
    let len = F80::from_f32(((ny * ny + nx * nx) + nz * nz).sqrt().to_f32());
    if len.abs().gt(F80::from_f64(NORMAL_EPS)) {
        n = [(nx / len).to_f64(), (ny / len).to_f64(), (nz / len).to_f64()];
    }
    // the end: the position curve is disposed when `a` was not given, the derivative curve when `b` was not (the original tests
    // the caller's two arguments, not whether it made the curves)
    if let Some((ea, eb)) = own {
        if a.is_none() {
            env.dispose(ea);
        }
        if b.is_none() {
            env.dispose(eb);
        }
    }
    Some((pos, n))
}
