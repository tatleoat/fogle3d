// SPDX-License-Identifier: MIT
//! Knot insertion (`eval.c`): a knot inserted one or more times into a curve without
//! changing its shape.
//!
//! The method is Boehm's (W. Boehm, "Inserting new knots into B-spline curves", 1980; the
//! multiple insertion is algorithm A5.1 of *The NURBS Book* in its effect): the triangle of
//! de Boor's algorithm is built `r` levels deep on the span's control points, and the new
//! control polygon is read off its edges. Departures from the published form are said at
//! each routine.

use crate::eval::knot_distances;
use crate::knots::interv;
use crate::own::{knots_allocate, points_allocate, Curve, Host, Surface};
use crate::{co, set_coord, Dir, NurbsPoint, Statics, EPS};
use si_types::{Double, Zero};
use si_x87::{Round, F80};

/// [soft 0x084d407c SI_nurbsKnotsCorrectMultAndValue]: before an insertion, snaps the
/// parameter to the knot it coincides with and lowers the number of insertions so that the
/// knot's multiplicity does not exceed `order − 1`.
///
/// - when `|t − range_end|` is below 1e-6, the knot looked at is the **last of the vector**
///   (index `nb_knots − 1`), else knot `left`;
/// - when `|t − knot|` is below 1e-6: `t` becomes that knot exactly; the multiplicity is 1
///   plus the number of knots directly below it that lie within 1e-6 of it (the scan stops
///   at index 0); `r` is lowered to `(order − 1) − multiplicity` when above it. That limit
///   is 0 for a knot of full multiplicity and **negative** beyond.
///
/// Differences are formed in the register. `None` for a read outside the knots.
pub fn knots_correct_mult_and_value(d: Dir, t: &mut f64, left: i32, r: &mut i32) -> Option<()> {
    let eps = F80::from_f64(EPS);
    let tv = F80::from_f64(*t);
    let mut left = left;
    if (tv - F80::from_f64(d.rec.range_end.0)).abs().lt(eps) {
        left = d.rec.nb_knots.wrapping_sub(1);
    }
    let kv = d.k(left)?;
    if !(tv - kv).abs().lt(eps) {
        return Some(());
    }
    *t = kv.to_f64();
    let mut mult = 1;
    let mut i = left;
    if i > 0 {
        loop {
            i -= 1;
            if !(kv - d.k(i)?).abs().lt(eps) {
                break;
            }
            mult += 1;
            if i <= 0 {
                break;
            }
        }
    }
    let lim = d.rec.order.wrapping_sub(1).wrapping_sub(mult);
    if *r > lim {
        *r = lim;
    }
    Some(())
}

/// Where the first point of level `l` of the insertion triangle lies: level 0 holds
/// `order` points, each later level one fewer, one after the other.
fn tri_get(t: &[NurbsPoint], i: i32) -> Option<NurbsPoint> {
    t.get(usize::try_from(i).ok()?).copied()
}

/// One planned copy into the new point array.
enum Src {
    Old(i32),
    Tri(i32),
}

/// `a % b` as `idiv` gives it; `None` where it traps.
fn rem(a: i32, b: i32) -> Option<i32> {
    if b == 0 || (a == i32::MIN && b == -1) {
        None
    } else {
        Some(a % b)
    }
}

/// Where every point of the new array comes from, in the original's order: (new index,
/// source), and for the old points that keep their value (old index, new index), which is
/// what `SI_gblIndex` receives. `k` order, `n` points, `r` insertions, `left` the span.
fn plan_of(k: i32, n: i32, r: i32, left: i32, closed: bool) -> Option<(Vec<(i32, Src)>, Vec<(i32, i32)>)> {
    let deg = k.wrapping_sub(1);
    let mut first = left.wrapping_sub(k).wrapping_add(2);
    if first < 0 {
        first = first.wrapping_add(n);
    }
    let mut plan: Vec<(i32, Src)> = Vec::new();
    let mut index: Vec<(i32, i32)> = Vec::new();
    let total = n.wrapping_add(r);
    if !closed || k.wrapping_sub(2) <= left {
        if first >= 0 {
            for i in 0..=first {
                plan.push((i, Src::Old(i)));
                index.push((i, i));
            }
        }
        let mut bc = k;
        let mut j = 0;
        while j < r - 1 {
            j += 1;
            plan.push((rem(first.wrapping_add(j), total)?, Src::Tri(bc)));
            bc += k - j;
        }
        for i in 0..(k - r).max(0) {
            plan.push((rem(first.wrapping_add(r).wrapping_add(i), total)?, Src::Tri(bc)));
            bc += 1;
        }
        bc -= 1;
        for i in 0..(r - 1).max(0) {
            bc -= (k - r) + i;
            plan.push((rem(first.wrapping_add(k).wrapping_add(i), total)?, Src::Tri(bc)));
        }
        let mut i = first.wrapping_add(deg);
        while i < n {
            plan.push((i.wrapping_add(r), Src::Old(i)));
            index.push((i, i.wrapping_add(r)));
            i += 1;
        }
    } else {
        // the span's points wrap round the start of the array: `w` of them lie at its end
        let w = left.wrapping_sub(k).wrapping_add(2).wrapping_abs();
        let cc = if r <= w { r } else { w };
        // level `cc`'s place in the triangle, computed by the original in doubles
        let base = F80::from_i32(cc.wrapping_mul(k)) - F80::from_i32(cc.wrapping_mul(cc.wrapping_sub(1))) * F80::from_f64(0.5);
        let e = (base + F80::from_i32(w - cc)).to_i32(Round::Zero);
        let mut bc = base.to_i32(Round::Zero);
        let mut pos = 0i32;
        let mut step = k - cc;
        while pos < r - cc {
            plan.push((pos, Src::Tri(bc)));
            pos += 1;
            bc += step;
            step -= 1;
        }
        let cnt = if w < r {
            k - r
        } else {
            bc = e;
            (F80::from_i32(k - cc) + base).to_i32(Round::Zero) - e
        };
        for _ in 0..cnt.max(0) {
            plan.push((pos, Src::Tri(bc)));
            pos += 1;
            bc += 1;
        }
        bc -= 1;
        for i in 0..r.max(0) {
            bc -= (k - r) + i;
            plan.push((pos, Src::Tri(bc)));
            pos += 1;
        }
        for i in 0..(n - k).max(0) {
            plan.push((pos, Src::Old(k - w + i)));
            index.push((k - w + i, pos));
            pos += 1;
        }
        bc = 0;
        for i in 0..cc.max(0) {
            plan.push((pos, Src::Tri(bc)));
            pos += 1;
            bc += k - i;
        }
        for _ in 0..(e - bc).max(0) {
            plan.push((pos, Src::Tri(bc)));
            pos += 1;
            bc += 1;
        }
    }
    Some((plan, index))
}

/// [soft 0x084d4130 SI_nurbsCrvKnotInsert]: inserts the knot `t` `r` times into a curve, in
/// place. Returns 0 when nothing is to be inserted, else 1.
///
/// 1. On a closed curve a parameter within 1e-6 of the range end becomes the **first knot**.
/// 2. The span of `t` ([`interv`]). With `correct` not 0,
///    [`knots_correct_mult_and_value`] may move `t` onto a knot and lower `r`. `r` = 0
///    returns 0 here; **a negative `r` goes on**: on a knot whose multiplicity already
///    exceeds `order − 1` the "insertion" then removes points and knots (the counts grow by
///    the negative `r`, the arrays are rebuilt by the same loops).
/// 3. The triangle: level 0 is the span's `order` points; level l + 1 is formed from level
///    l by the blend of the evaluators, with the same two short cuts: a factor within 1e-6
///    of 1 copies the left parent, one below 1e-6 the right parent (whole records, so the
///    new points inherit **flag words**), anything else is `right + a × (left − right)`
///    rounded to `double` a coordinate. `r` levels.
/// 4. The new points, `r` more than before: the old ones up to the span's first, the first
///    points of levels 1 to r − 1, all of level r, the last points of levels r − 1 down to
///    1, the old ones from the span's last on. On a closed curve the positions run modulo
///    the new count; and when the span's points wrap round the start of the array (`left`
///    below `order − 2`) the same pieces are laid out from index 0 in another order (the
///    port's comments). The new array is as long as the highest index written plus one,
///    which is the new count in every regular case.
/// 5. The knots with `r` copies of `t` after knot `left`; the range read back from them;
///    **the parameterisation word (+0x24) set to 2**.
///
/// Where `SI_gblIndex` is set, the new index of every old point that kept its value is
/// recorded in it.
///
/// `None`: the span search does not end; reads outside knots, points or the triangle (an
/// `r` above the degree); a division trap; a new knot count of zero.
pub fn crv_knot_insert(st: &mut Statics, host: &mut dyn Host, c: &mut Curve, t: f64, r: i32, correct: i16) -> Option<i32> {
    crv_knot_insert_scratch(st, host, c, t, r, correct, &mut None)
}

/// What the original's three work arrays hold when it is done (the left and right knot distances, the triangle of
/// points); the owned form keeps them as locals, a caller that mirrors the heap wants them.
pub type Scratch = (Vec<f64>, Vec<f64>, Vec<NurbsPoint>, Vec<(usize, Result<NurbsPoint, i32>)>);

/// [`crv_knot_insert`] that also hands back the work arrays.
pub fn crv_knot_insert_scratch(st: &mut Statics, host: &mut dyn Host, c: &mut Curve, t: f64, r: i32, correct: i16, out: &mut Option<Scratch>) -> Option<i32> {
    let mut t = si_x87::fmov64(t);
    let mut r = r;
    let eps = F80::from_f64(EPS);
    let closed = c.rec.dir.closed != 0;
    if c.knots.is_some() && closed && (F80::from_f64(t) - F80::from_f64(c.rec.dir.range_end.0)).abs().lt(eps) {
        t = *c.knots.as_ref()?.first()?;
    }
    let left = interv(st, c.view().dir(), t, (!closed) as i16)?;
    let (k, n, nk) = (c.rec.dir.order, c.rec.dir.nb_points, c.rec.dir.nb_knots);
    let deg = k.wrapping_sub(1);
    if correct != 0 {
        knots_correct_mult_and_value(c.view().dir(), &mut t, left, &mut r)?;
    }
    if r == 0 {
        return Some(0);
    }
    let (dl, dr) = knot_distances(c.view().dir(), left, t, false)?;
    let mut first = left.wrapping_sub(k).wrapping_add(2);
    if first < 0 {
        first = first.wrapping_add(n);
    }
    if !(1..=4096).contains(&k) {
        return None;
    }
    // the triangle
    let old = c.points.as_deref().unwrap_or(&[]);
    // below order 5 the original's triangle is a stack area of ten points; indices past the triangle (a negative
    // count) read what the frame held there, which the new points then carry unwritten: zero points here
    let mut tri = points_allocate(((k + 1) * k / 2).max(if k <= 4 { 10 } else { 0 }))?;
    for j in 0..k {
        let i = rem(first.wrapping_add(j), n)?;
        tri[j as usize] = *old.get(usize::try_from(i).ok()?)?;
    }
    let mut src = 0i32;
    let mut lvl = 0;
    while lvl < r {
        let m = deg - lvl;
        let dst = src + k - lvl;
        for b in 0..m.max(0) {
            let rv = F80::from_f64(*dr.get(b as usize)?);
            let a = rv / (F80::from_f64(*dl.get((m - 1 - b) as usize)?) + rv);
            let leftp = tri_get(&tri, src + b)?;
            let rightp = tri_get(&tri, src + b + 1)?;
            let out = tri.get_mut(usize::try_from(dst + b).ok()?)?;
            if (F80::ONE - a).lt(eps) {
                *out = leftp;
            } else if a.lt(eps) {
                *out = rightp;
            } else {
                for q in 0..4 {
                    let v = co(&rightp, q) + a * (co(&leftp, q) - co(&rightp, q));
                    set_coord(out, q, v.to_f64());
                }
            }
        }
        src = dst;
        lvl += 1;
    }
    *out = Some((dl.clone(), dr.clone(), tri.clone(), Vec::new()));
    let (plan, index) = plan_of(k, n, r, left, closed)?;
    let size = plan.iter().map(|p| p.0).max().unwrap_or(0).max(0).checked_add(1)?;
    let mut new = points_allocate(size)?;
    // A copy from beyond the triangle (levels the insertion count does not reach: a negative
    // count) moves stack content in the original. It is harmless when a later copy lands on
    // the same point, which is the rule there; the port refuses only if such a point stays.
    let mut stale = vec![false; new.len()];
    for (at, s) in &plan {
        let p = match s {
            Src::Old(i) => Some(*old.get(usize::try_from(*i).ok()?)?),
            Src::Tri(i) => tri_get(&tri, *i),
        };
        let at = usize::try_from(*at).ok()?;
        stale[at] = p.is_none();
        if let Some(p) = p {
            *new.get_mut(at)? = p;
        }
    }
    if stale.iter().any(|s| *s) {
        return None;
    }
    if let Some(tab) = host.gbl_index() {
        for (at, v) in index {
            *tab.get_mut(usize::try_from(at).ok()?)? = v;
        }
    }
    c.points = Some(new);
    c.rec.param = 2;
    c.rec.dir.nb_points = n.wrapping_add(r);
    // the knots
    let nk2 = nk.wrapping_add(r);
    // (a negative count writes past the end of the new knot block: the vector grows to hold it)
    let kmax = left.max(nk.wrapping_sub(1).wrapping_add(r)).max(left.wrapping_add(r));
    let mut kn = knots_allocate(nk2.max(kmax.wrapping_add(1)))?;
    let oldk = c.knots.as_deref().unwrap_or(&[]);
    let at = |i: i32| usize::try_from(i).ok();
    if left >= 0 {
        for i in 0..=left {
            *kn.get_mut(at(i)?)? = *oldk.get(at(i)?)?;
        }
    }
    for i in 0..r.max(0) {
        *kn.get_mut(at(left.wrapping_add(1).wrapping_add(i))?)? = t;
    }
    let mut i = left.wrapping_add(1);
    while i < nk {
        *kn.get_mut(at(i.wrapping_add(r))?)? = *oldk.get(at(i)?)?;
        i += 1;
    }
    let rs = if closed { 0 } else { k - 2 };
    c.rec.dir.range_start = Double(*kn.get(at(rs)?)?);
    let re = if closed { nk2 - 1 } else { nk2 - k + 1 };
    c.rec.dir.range_end = Double(*kn.get(at(re)?)?);
    c.rec.dir.nb_knots = nk2;
    c.knots = Some(kn);
    c.sync_arrays();
    Some(1)
}

/// The body shared by the two surface insertions: `across_v` false inserts into U (every
/// row of the grid is a curve in U), true into V (every column).
/// [`srf_knot_insert_u`] / `_v` that also hands back the work arrays (the triangles of all cross lines in one).
pub fn srf_knot_insert_scratch(st: &mut Statics, host: &mut dyn Host, s: &mut Surface, t: f64, r: i32, across_v: bool, out: &mut Option<Scratch>) -> Option<i16> {
    srf_insert(st, host, s, t, r, across_v, out)
}

fn srf_insert(st: &mut Statics, host: &mut dyn Host, s: &mut Surface, t: f64, r: i32, across_v: bool, out: &mut Option<Scratch>) -> Option<i16> {
    let mut t = si_x87::fmov64(t);
    let mut r = r;
    let eps = F80::from_f64(EPS);
    let view = s.view();
    let d = if across_v { view.v() } else { view.u() };
    let closed = d.rec.closed != 0;
    let (k, n, nk) = (d.rec.order, d.rec.nb_points, d.rec.nb_knots);
    let has_knots = if across_v { s.knots_v.is_some() } else { s.knots_u.is_some() };
    if closed && has_knots && (F80::from_f64(t) - F80::from_f64(d.rec.range_end.0)).abs().lt(eps) {
        t = *d.knots.first()?;
    }
    let left = interv(st, d, t, (!closed) as i16)?;
    knots_correct_mult_and_value(d, &mut t, left, &mut r)?;
    if r == 0 {
        return Some(0);
    }
    let cross = if across_v { view.rec.u.nb_points } else { view.rec.v.nb_points };
    let nu = view.rec.u.nb_points;
    if !(1..=4096).contains(&k) || !(0..=1 << 16).contains(&cross) {
        return None;
    }
    let deg = k - 1;
    let (dl, dr) = knot_distances(d, left, t, false)?;
    let mut first = left.wrapping_sub(k).wrapping_add(2);
    if first < 0 {
        first = first.wrapping_add(n);
    }
    // old index of point `i` of the direction on cross line `c`, and the same in the new
    // array, whose count in the direction is `n + r`
    let total = n.wrapping_add(r);
    let old_at = |c: i32, i: i32| if across_v { c.wrapping_add(i.wrapping_mul(nu)) } else { c.wrapping_mul(nu).wrapping_add(i) };
    let new_at = |c: i32, i: i32| if across_v { c.wrapping_add(i.wrapping_mul(nu)) } else { c.wrapping_mul(total).wrapping_add(i) };
    let old = view.points;
    // one triangle a cross line, all in one block: an index beyond a line's triangle reads
    // the next line's
    let tri_len = (k + 1) * k / 2;
        let mut tri = points_allocate(cross.checked_mul(tri_len)?).unwrap_or_default();
    for j in 0..k {
        let i = rem(first.wrapping_add(j), n)?;
        for c in 0..cross {
            *tri.get_mut((c * tri_len + j) as usize)? = *old.get(usize::try_from(old_at(c, i)).ok()?)?;
        }
    }
    let mut src = 0i32;
    let mut lvl = 0;
    while lvl < r {
        let m = deg - lvl;
        let dst = src + k - lvl;
        for b in 0..m.max(0) {
            let rv = F80::from_f64(*dr.get(b as usize)?);
            let a = rv / (F80::from_f64(*dl.get((m - 1 - b) as usize)?) + rv);
            for c in 0..cross {
                let base = c * tri_len;
                let leftp = tri_get(&tri, base + src + b)?;
                let rightp = tri_get(&tri, base + src + b + 1)?;
                let out = tri.get_mut(usize::try_from(base + dst + b).ok()?)?;
                if (F80::ONE - a).lt(eps) {
                    *out = leftp;
                } else if a.lt(eps) {
                    *out = rightp;
                } else {
                    for q in 0..4 {
                        let v = co(&rightp, q) + a * (co(&leftp, q) - co(&rightp, q));
                        set_coord(out, q, v.to_f64());
                    }
                }
            }
        }
        src = dst;
        lvl += 1;
    }
    let (plan, index) = plan_of(k, n, r, left, closed)?;
    let mut size = total.checked_mul(cross)?;
    // a negative count writes past the end of the new block (into the heap beyond it); the vector grows to hold it
    for (at, _) in &plan {
        for c in 0..cross {
            size = size.max(new_at(c, *at).checked_add(1)?);
        }
    }
    let mut new = points_allocate(size).unwrap_or_default();
    // as in the curve routine: a copy from beyond the block of triangles (a negative count)
    // is harmless when a later copy lands on the same point
    let mut lost: Vec<Option<i32>> = vec![None; new.len()];
    let mut events: Vec<(usize, Result<NurbsPoint, i32>)> = Vec::new();
    for (at, sr) in &plan {
        for c in 0..cross {
            let p = match sr {
                Src::Old(i) => Some(*old.get(usize::try_from(old_at(c, *i)).ok()?)?),
                Src::Tri(i) => tri_get(&tri, c * tri_len + *i),
            };
            let to = usize::try_from(new_at(c, *at)).ok()?;
            *lost.get_mut(to)? = match (&p, sr) {
                (None, Src::Tri(i)) => Some(c * tri_len + *i),
                _ => None,
            };
            if let Some(p) = p {
                new[to] = p;
            }
            events.push((to, match (p, sr) {
                (Some(p), _) => Ok(p),
                (None, Src::Tri(i)) => Err(c * tri_len + *i),
                (None, _) => Err(0),
            }));
        }
    }
    // A negative count reads past the block of triangles: the original copies whatever the heap holds there. The
    // caller that has the image redoes those copies (`Scratch.3`: new index, index in the triangle block).
    let beyond = if lost.iter().any(|l| l.is_some()) { events } else { Vec::new() };
    *out = Some((dl.clone(), dr.clone(), tri.clone(), beyond));
    if let Some(tab) = host.gbl_index() {
        for (from, to) in index {
            for c in 0..cross {
                *tab.get_mut(usize::try_from(old_at(c, from)).ok()?)? = new_at(c, to);
            }
        }
    }
    // the knots
    let nk2 = nk.wrapping_add(r);
    // (a negative count writes past the end of the new knot block: the vector grows to hold it)
    let kmax = left.max(nk.wrapping_sub(1).wrapping_add(r)).max(left.wrapping_add(r));
    let mut kn = knots_allocate(nk2.max(kmax.wrapping_add(1)))?;
    let at = |i: i32| usize::try_from(i).ok();
    if left >= 0 {
        for i in 0..=left {
            *kn.get_mut(at(i)?)? = *d.knots.get(at(i)?)?;
        }
    }
    for i in 0..r.max(0) {
        *kn.get_mut(at(left.wrapping_add(1).wrapping_add(i))?)? = t;
    }
    let mut i = left.wrapping_add(1);
    while i < nk {
        *kn.get_mut(at(i.wrapping_add(r))?)? = *d.knots.get(at(i)?)?;
        i += 1;
    }
    let rs = *kn.get(at(if closed { 0 } else { k - 2 })?)?;
    let re = *kn.get(at(if closed { nk2 - 1 } else { nk2 - k + 1 })?)?;
    s.points = (size > 0).then_some(new);   // (longer than `nb_points` where the original overran its block)
    s.rec.points = if s.points.is_some() { crate::held() } else { si_types::Ptr32::NULL };
    s.rec.nb_points = total.wrapping_mul(cross);
    s.rec.param = 2;
    let dir = if across_v { &mut s.rec.v } else { &mut s.rec.u };
    dir.nb_points = total;
    dir.nb_knots = nk2;
    dir.range_start = Double(rs);
    dir.range_end = Double(re);
    dir.knots = crate::held();
    if across_v {
        s.knots_v = Some(kn);
    } else {
        s.knots_u = Some(kn);
    }
    host.srf_dispose_lines(s);
    Some(1)
}

/// [soft 0x084d96f4 SI_nurbsSrfKnotInsertU]: inserts the knot `t` `r` times into the U
/// direction of a surface: [`crv_knot_insert`] on every row of the grid at once, with these
/// differences:
///
/// - the correction of [`knots_correct_mult_and_value`] is always applied;
/// - the new point array is `(nu + r) × nv` whatever is written into it;
/// - the triangles of all rows lie in one block from `SI_nurbsPointsAllocate`, so a blended
///   point's **flag word is 0** here (in the curve routine it is stack content up to order
///   4);
/// - the surface's total (+0x48) and parameterisation word (+0x4c, set to 2) are updated,
///   and the display lines are disposed of at the end (`SI_nurbsSrfDisposeLines`).
///
/// Returns 0 (a 16-bit result) when nothing is inserted, else 1.
pub fn srf_knot_insert_u(st: &mut Statics, host: &mut dyn Host, s: &mut Surface, t: f64, r: i32) -> Option<i16> {
    srf_insert(st, host, s, t, r, false, &mut None)
}

/// [soft 0x084da6fc SI_nurbsSrfKnotInsertV]: the mirror image for V: every column of the
/// grid; the new array is `nu × (nv + r)`. Its constants are a second set (0x08661730,
/// 0x08661738) with the same bits.
pub fn srf_knot_insert_v(st: &mut Statics, host: &mut dyn Host, s: &mut Surface, t: f64, r: i32) -> Option<i16> {
    srf_insert(st, host, s, t, r, true, &mut None)
}

/// [soft 0x084d96bc SI_nurbsSrfKnotInsert]: `dir` 0 inserts into U, anything else into V;
/// the 16-bit result is widened with its sign.
pub fn srf_knot_insert(st: &mut Statics, host: &mut dyn Host, s: &mut Surface, t: f64, r: i32, dir: i32) -> Option<i32> {
    let v = if dir == 0 { srf_knot_insert_u(st, host, s, t, r)? } else { srf_knot_insert_v(st, host, s, t, r)? };
    Some(v as i32)
}
