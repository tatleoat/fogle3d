// SPDX-License-Identifier: MIT
//! Knot vectors: the uniform start sequences, the span search (new and old form), the
//! reversal of a direction.
//!
//! Layout of a knot vector, read from the span search and from every evaluator: a direction
//! of order `k` with `n` points holds **`n + k − 2`** knots when open, and its parameter
//! range is `knots[k − 2] ..= knots[nb_knots − k + 1]` (the first and the last knot of the
//! textbook vector of `n + k` are not stored: full multiplicity at an end is `k − 1` equal
//! knots). A closed direction holds `n + 1` knots, one period; its range is
//! `knots[0] ..= knots[nb_knots − 1]` and knots outside are formed by adding or subtracting
//! the period `knots[nb_knots − 1] − knots[0]`.

use crate::{Dir, NurbsDir, Statics, KNOT_EPS};
use si_types::Double;
use si_x87::F80;

/// [soft 0x084dc518 si_knotsInitSequence]: fills the knots of a direction with a uniform
/// sequence of step `d = 1 / ((nb_points − order) + 1)`, kept in the register unrounded;
/// every knot is one product rounded to `double`.
///
/// - `kind` 0: `knots[i] = d × ((i − order) + 1)` for every knot: an unclamped sequence
///   whose knot `order − 1` is 0;
/// - `kind` 1: `order` zeros, then `d × ((i − order) + 1)` up to index `nb_points − 1`,
///   then ones up to the end: a clamped sequence **in the textbook layout** of
///   `nb_points + order` knots, not in the layout the evaluators read (module comment);
/// - any other `kind`: nothing.
///
/// `None` where the original writes outside the array.
pub fn init_sequence(dir: &NurbsDir, knots: &mut [f64], kind: i32) -> Option<()> {
    let d = F80::ONE / (F80::from_i32(dir.nb_points.wrapping_sub(dir.order)) + F80::ONE);
    let val = |i: i32| (d * (F80::from_i32(i.wrapping_sub(dir.order)) + F80::ONE)).to_f64();
    let mut put = |i: i32, v: f64| -> Option<()> {
        *knots.get_mut(usize::try_from(i).ok()?)? = v;
        Some(())
    };
    if kind == 0 {
        let mut i = 0;
        while i < dir.nb_knots {
            put(i, val(i))?;
            i += 1;
        }
    } else if kind == 1 {
        let mut i = 0;
        if i < dir.order {
            loop {
                put(i, 0.0)?;
                i += 1;
                if i >= dir.order {
                    break;
                }
            }
        }
        let mut i = dir.order;
        while i < dir.nb_points {
            put(i, val(i))?;
            i += 1;
        }
        let mut i = dir.nb_points;
        while i < dir.nb_knots {
            put(i, 1.0)?;
            i += 1;
        }
    }
    Some(())
}

/// [soft 0x084d76b4 si_nurbsInterv]: the knot span of a parameter: the index `left` with
/// `knots[left] ≤ t < knots[left + 1]`.
///
/// 1. `t` is clamped to the direction's range: raised to `range_start` when that is above
///    it, lowered to `range_end` when that is below it (a NaN passes both tests). **Only
///    the search uses the clamped value**: the caller goes on with its own parameter (the
///    evaluators' blend factors then leave [0, 1] and are caught by their 1e-6 tests).
/// 2. The search runs between `lo` and `hi`: `order − 2` and `nb_knots − order + 1` when
///    `open` is not 0, else 0 and `nb_knots − 1`. Every evaluator passes `open` = "the
///    direction is not closed".
/// 3. It starts from the index the last call returned ([`Statics::left`]) when that lies in
///    `lo .. hi`, else from `(hi + lo) / 2`.
/// 4. **At the end of the range** (`|t − knots[hi]|` below 1e-5, the difference in the
///    register): the answer is `hi − 1`, and on a closed direction it then steps down while
///    `|t − knots[left]|` is still below 1e-5, with no lower bound (it reads in front of
///    the array when every knot is within 1e-5 of the parameter).
/// 5. Otherwise a bisection: while `t < knots[left]` or `t ≥ knots[left + 1]`, `hi` or `lo`
///    becomes `left` and `left` the mean (rounded towards zero). So a parameter **exactly on
///    a knot belongs to the span that starts there**, and on a multiple knot to the last of
///    the equal knots. A NaN ends the search at once with the start index.
///
/// The bisection has no exit of its own: when the clamped parameter lies outside
/// `knots[lo] .. knots[hi]` (a range that disagrees with the knots) it never returns.
/// `None` for that and for a read outside the array.
pub fn interv(st: &mut Statics, d: Dir, t: f64, open: i16) -> Option<i32> {
    let r = d.rec;
    let mut t = F80::from_f64(t);
    let rs = F80::from_f64(r.range_start.0);
    if rs.gt(t) {
        t = rs;
    }
    let re = F80::from_f64(r.range_end.0);
    if re.lt(t) {
        t = re;
    }
    let (mut lo, mut hi) = if open != 0 { (r.order.wrapping_sub(2), r.nb_knots.wrapping_sub(r.order).wrapping_add(1)) } else { (0, r.nb_knots.wrapping_sub(1)) };
    if st.left < lo || st.left >= hi {
        st.left = hi.wrapping_add(lo) / 2;
    }
    let tol = F80::from_f64(KNOT_EPS);
    if tol.gt((t - d.k(hi)?).abs()) {
        st.left = hi.wrapping_sub(1);
        if r.closed != 0 {
            // a run of knots all equal to `t` walks off the start of the array: the word before it is the heap's
            // chunk header, which is not near `t` (unless `t` is about 0), so the walk ends at -1
            loop {
                match d.k(st.left) {
                    Some(v) if (t - v).abs().lt(tol) => st.left = st.left.wrapping_sub(1),
                    Some(_) => break,
                    None if st.left == -1 => break,
                    None => return None,
                }
            }
        }
        return Some(st.left);
    }
    // every step halves hi − lo; a state that survives this many steps is a fixed point
    for _ in 0..80 {
        let below = t.lt(d.k(st.left)?);
        if !below {
            let k1 = d.k(st.left.checked_add(1)?)?;
            if !t.ge(k1) {
                return Some(st.left);
            }
        }
        if below {
            hi = st.left;
        } else {
            lo = st.left;
        }
        st.left = hi.wrapping_add(lo) / 2;
    }
    None
}

/// [soft 0x084d81a0 si_OLDnurbsInterv]: the earlier span search, kept for the earlier
/// subdivision routine. It returns an index **one above** the span: the number of knots
/// that are not above `t`.
///
/// - `t` below the first knot or above the last: 0;
/// - else the knots are scanned downwards from the last to index `nb_knots − order − 1`:
///   the first one that is **strictly below** `t` (or does not compare) gives its index
///   plus 1: a parameter equal to a knot passes it;
/// - if none of those is, the vector is scanned upwards from the start for the first knot
///   above `t` and that index is returned.
///
/// No tolerance, no range, no closed case, no static.
pub fn old_interv(d: Dir, t: f64) -> Option<i32> {
    let t = F80::from_f64(t);
    let n = d.rec.nb_knots;
    if t.lt(d.k(0)?) {
        return Some(0);
    }
    // fcom; and 0x45; jne: taken unless the parameter is above the last knot
    if t.gt(d.k(n.wrapping_sub(1))?) {
        return Some(0);
    }
    let mut i = n.wrapping_sub(1);
    let stop = n.wrapping_sub(d.rec.order).wrapping_sub(1);
    if i >= stop {
        loop {
            // fcom; and 0x45; dec; cmp 0x40; jae: leaves the scan when t is above the knot
            // or the two do not compare; "below" and "equal" go on
            if !t.le(d.k(i)?) {
                return Some(i.wrapping_add(1));
            }
            i = i.wrapping_sub(1);
            if i < stop {
                break;
            }
        }
    }
    let mut i = 0;
    // and 0x5: continues while neither "below" nor "unordered"
    while t.ge(d.k(i)?) {
        i += 1;
    }
    Some(i)
}

/// [soft 0x084dc954 si_nurbsDirInverse]: reverses the knot vector of a direction in place
/// by reversing its **intervals**: with `Δ[i] = knots[i + 1] − knots[i]` (each rounded to
/// `double`), `knots[i] = knots[i − 1] + Δ[nb_knots − i + first − 2]` for `i` from `first`
/// up, where `first` is 2 on a closed direction whose order is not 3, else 1. Knot 0 (and
/// knot 1 in the first case) keeps its value, so the reversed vector starts where the old
/// one started. Then the range is read back from the knots: `knots[order − 2]` and
/// `knots[nb_knots − order + 1]` when open, the first and the last knot when closed.
///
/// Always returns 1. `None` where the original reads or writes outside the array (a
/// direction without knots among them: it reads `knots[order − 2]` of a null array).
pub fn dir_inverse(dir: &mut NurbsDir, knots: &mut [f64]) -> Option<i32> {
    let n = dir.nb_knots;
    let at = |i: i32| usize::try_from(i).ok();
    let mut delta = vec![0.0f64; n.max(0) as usize];
    let mut i = 0;
    while i < n.wrapping_sub(1) {
        let v = (F80::from_f64(*knots.get(at(i + 1)?)?) - F80::from_f64(*knots.get(at(i)?)?)).to_f64();
        *delta.get_mut(at(i)?)? = v;
        i += 1;
    }
    let first = if dir.closed == 0 || dir.order == 3 { 1 } else { 2 };
    let mut i = first;
    while i < n {
        let dv = *delta.get(at(n.wrapping_sub(i).wrapping_add(first).wrapping_sub(2))?)?;
        let v = (F80::from_f64(*knots.get(at(i - 1)?)?) + F80::from_f64(dv)).to_f64();
        *knots.get_mut(at(i)?)? = v;
        i += 1;
    }
    let s = if dir.closed == 0 { dir.order.wrapping_sub(2) } else { 0 };
    dir.range_start = Double(*knots.get(at(s)?)?);
    let e = if dir.closed == 0 { n.wrapping_sub(dir.order).wrapping_add(1) } else { n.wrapping_sub(1) };
    dir.range_end = Double(*knots.get(at(e)?)?);
    Some(1)
}
