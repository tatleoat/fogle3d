// SPDX-License-Identifier: MIT
//! Four routines of `nrb_geometry.c`: the length tables of a curve, built
//! (`SI_nurbsCurveUpdateLength`, with the touch it may ask for, `SI_nurbsCrvAnimUpdate`) and
//! read (its length; the parameter at a distance along it: the path code's and the tangency
//! constraint's questions, finding 0023).
//!
//! The tables: their count at +0x50, the parameters at +0x54 and the cumulated lengths at
//! +0x58 (`nb_length` doubles each; [`crate::own::Curve`] holds them beside the record). The
//! readers build them when the count is 0, as the original does
//! (`SI_nurbsCurveUpdateLength(curve, 100, 1)`).
//!
//! The arithmetic is shared by the two forms, the first on [`Curve`] (here) and the second
//! on the memory image (`image::nurbs_curve_update_length`, `image::nurbs_curve_length`,
//! `image::nurbs_distance_to_factor`); they differ only in where an entry of a table is
//! read and written. The touch's walk over the scene's objects and its calls into the
//! animation package are the image form's only.

use crate::own::Curve;
use crate::Statics;
use si_x87::F80;

/// The tolerance of `SI_nurbsDistanceToFactor`: the double nearest 0.0001 (0x08661e40,
/// bits 0x3f1a36e2eb1c432d).
pub const DISTANCE_EPS: f64 = 0.0001;

/// How many times a closed curve's length is taken off or added before the port gives up
/// (the original keeps going; for a length of 0, a negative one or one too small to move
/// the distance it never stops).
const WRAP_BUDGET: u32 = 1 << 20;

/// Why a routine of this module does not answer as the original would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The curve has no tables (+0x50 is 0) and building them was refused (one of the two
    /// variants below).
    Unbuilt,
    /// `SI_nurbsCurveUpdateLength`: the knot vector is not sampled (no knots: the original
    /// then reads the first parameter through a null array with a count it never set; or
    /// `SI_SampleKnotVector`'s refusal).
    Unsampled,
    /// `SI_nurbsCurveUpdateLength`: `SI_nurbsCrvValue` refused at a sample (a span search
    /// that does not end, a read outside the knots or the points).
    Unevaluated,
    /// A closed curve whose distance the original's loops never (or not within
    /// [`WRAP_BUDGET`] turns) bring inside the length.
    Endless,
    /// No entry of the cumulated lengths is above the distance (a NaN distance, one equal
    /// to the last entry after a closed curve's wrap, a count below 1): the original then
    /// indexes the tables with whatever `esi` held when it was called.
    NoSpan,
    /// The first form only: an entry outside the tables the curve holds (the original reads
    /// whatever lies there).
    Outside,
}

/// `SI_nurbsCurveLength` on a curve that may be absent, after its tables were built when
/// their count was 0: 0 for none, else the last cumulated length (`last` reading entry
/// `count − 1` of the table at +0x58).
pub(crate) fn curve_length_with(present: bool, count: i32, last: impl FnOnce(i32) -> Option<f64>) -> Result<F80, Refusal> {
    if !present {
        return Ok(F80::ZERO);
    }
    last(count.wrapping_sub(1)).map(F80::from_f64).ok_or(Refusal::Outside)
}

/// One entry of `SI_nurbsCurveUpdateLength`'s cumulated lengths: the entry before (`prev`,
/// a double) plus the distance between the points `p` and `q` of two samples in a row.
/// Each difference `p − q` is rounded to a double (stored), its square taken in the
/// register; `√((dx² + dy²) + dz²) + prev` in the register, then rounded to the double
/// stored.
pub(crate) fn chord_add(prev: f64, p: [f64; 3], q: [f64; 3]) -> f64 {
    let d: [F80; 3] = core::array::from_fn(|k| F80::from_f64((F80::from_f64(p[k]) - F80::from_f64(q[k])).to_f64()));
    let sum = (d[0] * d[0] + d[1] * d[1]) + d[2] * d[2];
    (sum.sqrt() + F80::from_f64(prev)).to_f64()
}

/// True when `SI_nurbsDistanceToFactor` answers the start of the range before it asks for
/// the length: `|d| ≤ 0.0001` (a NaN fails the test).
pub(crate) fn near_start(distance: f64) -> bool {
    F80::from_f64(distance).abs().le(F80::from_f64(DISTANCE_EPS))
}

/// The record's fields `SI_nurbsDistanceToFactor` reads besides the tables.
pub(crate) struct Ends {
    /// +0x20 (16 bits): closed when not 0.
    pub closed: i16,
    /// +0x10, +0x18: the parameter range.
    pub start: f64,
    pub end: f64,
}

/// `SI_nurbsDistanceToFactor` past [`near_start`], with its reads handed in: `len` is what
/// `SI_nurbsCurveLength(curve)` answered, `count` the table's count, `dist(i)` and
/// `param(i)` entry `i` of the cumulated lengths and of the parameters.
pub(crate) fn distance_to_factor_with(
    ends: &Ends,
    distance: f64,
    len: F80,
    count: impl FnOnce() -> i32,
    dist: impl Fn(i32) -> Option<f64>,
    param: impl Fn(i32) -> Option<f64>,
) -> Result<F80, Refusal> {
    let eps = F80::from_f64(DISTANCE_EPS);
    let mut d = F80::from_f64(distance);
    if (d - len).abs().le(eps) {
        return Ok(F80::from_f64(ends.end));
    }
    if ends.closed != 0 {
        if d.gt(len) {
            let mut turns = 0;
            loop {
                d = d - len;
                if !d.gt(len) {
                    break;
                }
                turns += 1;
                if turns == WRAP_BUDGET {
                    return Err(Refusal::Endless);
                }
            }
        }
        if F80::ZERO.gt(d) {
            let mut turns = 0;
            loop {
                d = d + len;
                if !d.lt(F80::ZERO) {
                    break;
                }
                turns += 1;
                if turns == WRAP_BUDGET {
                    return Err(Refusal::Endless);
                }
            }
        }
    }
    if d.gt(len) {
        return Ok(F80::from_f64(ends.end));
    }
    if F80::ZERO.gt(d) {
        return Ok(F80::from_f64(ends.start));
    }
    // the first entry above the distance (ordered: a NaN entry is passed over)
    let n = count();
    let mut at = None;
    for i in 0..n.max(0) {
        if d.lt(F80::from_f64(dist(i).ok_or(Refusal::Outside)?)) {
            at = Some(i);
            break;
        }
    }
    let i = at.ok_or(Refusal::NoSpan)?;
    let get = |f: &dyn Fn(i32) -> Option<f64>, k: i32| f(k).map(F80::from_f64).ok_or(Refusal::Outside);
    let d0 = get(&dist, i.wrapping_sub(1))?;
    let t = (d - d0) / (get(&dist, i)? - d0);
    let p0 = get(&param, i.wrapping_sub(1))?;
    Ok(p0 + (get(&param, i)? - p0) * t)
}

fn entry(table: &Option<Vec<f64>>, i: i32) -> Option<f64> {
    table.as_deref()?.get(usize::try_from(i).ok()?).copied()
}

/// [soft 0x08501ed8 SI_nurbsCurveUpdateLength], first form: builds the curve's length
/// tables from `samples` (the original's `long`) samples a unit of the knot vector.
///
/// 1. `flag` 1: bits 0x60 of the record's byte at +0x3b, then the record's part of
///    [soft 0x08501d90 SI_nurbsCrvAnimUpdate]: a count that is not 0 is cleared with the
///    cumulated lengths (not the parameters); bit 0x10 of the byte at +0x38. (Its walk over
///    the scene's objects and its calls for a keyframed curve are the image form's,
///    `image::nurbs_crv_anim_update`.)
/// 2. The parameters: `SI_SampleKnotVector(curve, samples)` ([`crate::sample`]); the old
///    tables go, the parameters become the table at +0x54 and their count +0x50.
/// 3. The cumulated lengths: entry 0 is 0, entry `i` entry `i − 1` plus the distance
///    between the points at parameters `i − 1` and `i` (`SI_nurbsCrvValue(curve, t, 0)`;
///    [`chord_add`]).
pub fn curve_update_length(st: &mut Statics, c: &mut Curve, samples: i32, flag: i16) -> Result<(), Refusal> {
    if flag == 1 {
        c.rec.flags_38 |= 0x6000_0000;
        if c.nb_length() != 0 {
            c.rec.length_data[0] = 0;
            c.length_dist = None;
        }
        c.rec.flags_38 |= 0x10;
    }
    let params = match crate::sample::sample_knot_vector(Some(c.view().dir()), c.knots.is_some(), samples) {
        Some(Some(v)) => v,
        _ => return Err(Refusal::Unsampled),
    };
    c.length_param = None;
    c.length_dist = None;
    c.rec.length_data[0] = params.len() as u32;
    let mut table = vec![0.0f64; params.len()];
    // the first point is used only from the second sample on (a curve of one sample is not
    // refused for it)
    let first = crate::eval::crv_value(st, c.view(), params[0], 0);
    let mut p = if params.len() == 1 { [0.0; 3] } else { first.ok_or(Refusal::Unevaluated)? };
    for i in 1..params.len() {
        let q = crate::eval::crv_value(st, c.view(), params[i], 0).ok_or(Refusal::Unevaluated)?;
        table[i] = chord_add(table[i - 1], p, q);
        p = q;
    }
    c.length_param = Some(params);
    c.length_dist = Some(table);
    c.sync();
    Ok(())
}

/// [soft 0x08502070 SI_nurbsCurveLength]: the curve's length, the last of its cumulated
/// lengths (+0x58, entry `count − 1`) as loaded into the unit, after
/// [`curve_update_length`]`(curve, 100, 1)` when the count is 0; 0 for no curve.
pub fn curve_length(st: &mut Statics, c: Option<&mut Curve>) -> Result<F80, Refusal> {
    let Some(c) = c else { return curve_length_with(false, 0, |_| None) };
    if c.nb_length() == 0 {
        curve_update_length(st, c, 100, 1).map_err(|_| Refusal::Unbuilt)?;
    }
    curve_length_with(true, c.nb_length(), |i| entry(&c.length_dist, i))
}

/// [soft 0x08502f4c SI_nurbsDistanceToFactor]: the parameter at `distance` along the curve,
/// unrounded as the original returns it.
///
/// 1. `|d| ≤ 0.0001`: the start of the range (+0x10), before the length is asked for.
/// 2. Within 0.0001 of the length ([`curve_length`], which builds the tables when there are
///    none): the end of the range (+0x18).
/// 3. A closed curve (+0x20 not 0): the length taken off while `d > length`, then added
///    while `d < 0`, in the register.
/// 4. Still above the length: the end; below 0: the start.
/// 5. Otherwise the first entry `i` of the cumulated lengths above `d` (in order from 0)
///    and the straight line between entries `i − 1` and `i` of the two tables:
///    `p[i−1] + (p[i] − p[i−1]) × ((d − l[i−1]) / (l[i] − l[i−1]))`, all in the register.
///    For `i` = 0 the original reads the doubles in front of both tables (here
///    [`Refusal::Outside`]).
pub fn distance_to_factor(st: &mut Statics, c: &mut Curve, distance: f64) -> Result<F80, Refusal> {
    if near_start(distance) {
        return Ok(F80::from_f64(c.rec.dir.range_start.0));
    }
    let len = curve_length(st, Some(c))?;
    let ends = Ends { closed: c.rec.dir.closed, start: c.rec.dir.range_start.0, end: c.rec.dir.range_end.0 };
    distance_to_factor_with(&ends, distance, len, || c.nb_length(), |i| entry(&c.length_dist, i), |i| entry(&c.length_param, i))
}
