// SPDX-License-Identifier: MIT
//! Sampling: the parameters at which a direction is sampled for display and conversion, the
//! number of samples each span gets, a knot's place in a vector, points along a curve.

use crate::eval::crv_value;
use crate::{CrvRef, Dir, Statics, EPS};
use si_x87::{Round, F80};

/// The spans of a direction that the samplers walk: `first .. last`, the knot indices of
/// the range (`order − 2 .. count + 1 − order` when open, `0 .. count − 1` when closed).
fn span_range(d: Dir) -> (i32, i32) {
    if d.rec.closed == 0 {
        (d.rec.order.wrapping_sub(2), d.rec.nb_knots.wrapping_add(1).wrapping_sub(d.rec.order))
    } else {
        (0, d.rec.nb_knots.wrapping_sub(1))
    }
}

/// The step that makes "samples a span" mean something on a non-uniform vector: `per`
/// divided by the mean length of the spans longer than 0.01, that mean being raised to 0.1
/// when it is smaller (or when no span is that long). Sum and mean stay in the register;
/// the quotient is rounded to `double`.
fn per_unit(d: Dir, per: i32, first: i32, last: i32) -> Option<f64> {
    let wide = F80::from_f64(0.01);
    let mut sum = F80::ZERO;
    let mut n = 0;
    let mut i = first;
    while i < last {
        let w = d.k(i.checked_add(1)?)? - d.k(i)?;
        if w.gt(wide) {
            sum = sum + w;
            n += 1;
        }
        i += 1;
    }
    if n != 0 {
        sum = sum / F80::from_i32(n);
    }
    let least = F80::from_f64(0.1);
    if least.gt(sum) {
        sum = least;
    }
    Some((F80::from_i32(per) / sum).to_f64())
}

/// How many samples a span of width `w` gets: `per`, or **four times `per`** when the span
/// is so long that `w × per_unit` exceeds `4 × per` (the product in the register). The
/// second value goes through the C library's `rint` and a truncating conversion, which
/// change nothing on a whole number.
pub fn span_samples(w: F80, unit: f64, per: i32) -> i32 {
    let four = F80::from_i32(per.wrapping_shl(2));
    if four.lt(w * F80::from_f64(unit)) {
        F80::from_f64(four.to_f64()).round_int(Round::Nearest).to_i32(Round::Zero)
    } else {
        per
    }
}

/// [soft 0x084d727c SI_SampleKnotVector]: the parameters at which a direction is sampled,
/// `per` to a span of average length. Returns the new array (`SI_calloc`) and its count, or
/// `None` in the first place for a null direction or one without knots, where the original
/// returns without touching its two results.
///
/// Spans not longer than 1e-6 are skipped (no sample at a multiple knot). Every other span
/// contributes its start knot and, unless the order is 2 and `per` is 1, `m − 1` parameters
/// `j × (width / m) + knot` for j from 1 (`width / m` in the register, each parameter
/// rounded once), with `m` from [`span_samples`]. The last entry is the range's end knot.
///
/// The count is computed in a first pass as the sum of `m` (or 1 where `m` is 0) plus 1,
/// which agrees with what the second pass writes only for `m` ≥ 0: a negative `per` makes
/// the array too small for what is written into it. `Some(None)` there (the original writes
/// outside its block) and for a read outside the knots.
pub fn sample_knot_vector(d: Option<Dir>, has_knots: bool, per: i32) -> Option<Option<Vec<f64>>> {
    let d = d?;
    if !has_knots {
        return None;
    }
    Some(sample_knots(d, per))
}

fn sample_knots(d: Dir, per: i32) -> Option<Vec<f64>> {
    let (first, last) = span_range(d);
    let unit = per_unit(d, per, first, last)?;
    let tiny = F80::from_f64(EPS);
    let plain = d.rec.order == 2 && per == 1;
    let mut n = 0i32;
    let mut i = first;
    while i < last {
        let w = d.k(i.checked_add(1)?)? - d.k(i)?;
        if w.gt(tiny) {
            let m = if plain { 0 } else { span_samples(w, unit, per) };
            n = n.wrapping_add(if m != 0 { m } else { 1 });
        }
        i += 1;
    }
    let count = n.wrapping_add(1);
    if count <= 0 || count > 1 << 24 {
        return None;
    }
    let mut out = vec![0.0f64; count as usize];
    let mut at = 0usize;
    let mut i = first;
    while i < last {
        let k0 = d.k(i)?;
        let w = d.k(i.checked_add(1)?)? - k0;
        if w.gt(tiny) {
            *out.get_mut(at)? = k0.to_f64();
            at += 1;
            if !plain {
                let m = span_samples(w, unit, per);
                if m > 1 {
                    let part = w / F80::from_i32(m);
                    for j in 1..m {
                        *out.get_mut(at)? = (F80::from_i32(j) * part + k0).to_f64();
                        at += 1;
                    }
                }
            }
        }
        i += 1;
    }
    *out.get_mut(at)? = d.k(last)?.to_f64();
    Some(out)
}

/// [soft 0x084d74f0 SI_samplesPerKnot]: for each span of the range, the number of samples
/// it gets, as a `double`: 0 for a span not longer than 1e-6, else `m` of [`span_samples`],
/// or 1 where that is 0. Returns the array (one entry a span; null when there is none) and
/// the routine's result, the sum of the entries (accumulated as an integer through a
/// `double` addition and a truncating conversion).
pub fn samples_per_knot(d: Dir, per: i32) -> Option<(Option<Vec<f64>>, i32, i32)> {
    let (first, last) = span_range(d);
    let count = if d.rec.closed != 0 { last } else { last.wrapping_sub(first) };
    let unit = per_unit(d, per, first, last)?;
    let mut out = (count > 0 && count < 1 << 24).then(|| vec![0.0f64; count as usize]);
    let tiny = F80::from_f64(EPS);
    let mut total = 0i32;
    let mut j = 0usize;
    let mut i = first;
    while i < last {
        let w = d.k(i.checked_add(1)?)? - d.k(i)?;
        let slot = out.as_mut()?.get_mut(j)?;
        if w.gt(tiny) {
            let m = span_samples(w, unit, per);
            *slot = if m != 0 { m as f64 } else { 1.0 };
            total = (F80::from_i32(total) + F80::from_f64(*slot)).to_i32(Round::Zero);
        } else {
            *slot = 0.0;
        }
        i += 1;
        j += 1;
    }
    Some((out, count, total))
}

/// [soft 0x084d80f8 getKnotPosition]: copies `n` knots into `out` (room for `n + 1`) with
/// `t` inserted in order, and returns the index at which it went: before knot 0 when it is
/// strictly below it, else before the first later knot that is **not below** it, else at
/// the end. Once inserted, the value carried on is `knots[n − 1] + 1` (in the register,
/// unrounded), so that nothing is inserted twice in a sorted vector; in an unsorted one a
/// later knot above that is preceded by it again.
pub fn knot_position(out: &mut [f64], knots: &[f64], t: f64, n: i32) -> Option<i32> {
    let k = |i: i32| -> Option<F80> { knots.get(usize::try_from(i).ok()?).map(|&v| F80::from_f64(v)) };
    let mut put = |w: usize, v: F80| -> Option<()> {
        *out.get_mut(w)? = v.to_f64();
        Some(())
    };
    let mut t = F80::from_f64(t);
    let mut idx = -1;
    let mut w = 0usize;
    if t.lt(k(0)?) {
        put(0, t)?;
        t = k(n.wrapping_sub(1))? + F80::ONE;
        idx = 0;
        w = 1;
    }
    put(w, k(0)?)?;
    w += 1;
    let mut i = 1;
    while i < n {
        if t.le(k(i)?) {
            idx = i;
            put(w, t)?;
            t = k(n - 1)? + F80::ONE;
            w += 1;
        }
        put(w, k(i)?)?;
        w += 1;
        i += 1;
    }
    if idx == -1 {
        idx = n;
        put(usize::try_from(n).ok()?, t)?;
    }
    Some(idx)
}

/// [soft 0x084d7fec SI_nurbsCrvSamples]: points along a curve at a fixed parameter step:
/// `floor((range_end − range_start) / step) + 1` points (the quotient in the register,
/// floored there, stored as a `double` and converted), from the range start. The parameter
/// is kept **in the register** and grows by `step` a point without being rounded; each
/// point is evaluated at its rounding to `double`, lowered to the range end when above it.
/// So the last point is the end only when the step divides the range.
///
/// Does nothing unless `mode` is 0 (`None` in the first place). Returns the point array;
/// `Some(None)` where the original would write through a null array (a count of zero or
/// less, or beyond what can be allocated) or the evaluator fails.
pub fn crv_samples(st: &mut Statics, c: CrvRef, mode: i32, step: f64) -> Option<Option<(i32, Vec<[f64; 3]>)>> {
    if mode != 0 {
        return None;
    }
    Some((|| {
        let r = &c.rec.dir;
        let q = (F80::from_f64(r.range_end.0) - F80::from_f64(r.range_start.0)) / F80::from_f64(step);
        let n = F80::from_f64(q.round_int(Round::Down).to_f64()).to_i32(Round::Zero);
        let count = n.wrapping_add(1);
        if count <= 0 {
            return Some((count, Vec::new()));
        }
        if count > 1 << 20 {
            return None;
        }
        let mut out = Vec::new();
        let mut t = F80::from_f64(r.range_start.0);
        for _ in 0..count {
            let re = F80::from_f64(r.range_end.0);
            if re.lt(t) {
                t = re;
            }
            out.push(crv_value(st, c, t.to_f64(), 0)?);
            t = t + F80::from_f64(step);
        }
        Some((count, out))
    })())
}
