// SPDX-License-Identifier: MIT
//! The length table of a spline and the two mappings that stand on it: distance along the
//! curve to parameter and back. Motion along a path goes through these (finding 0017, 9a).
//!
//! The table (`rec.lengths`, +0x6c) holds `samples × segments + 1` floats: entry k is the
//! length of the polyline through the first k + 1 sample points, `samples` (`rec.nb_lengths`,
//! +0x68) equal steps of the parameter to a segment. Entry `p × samples` is therefore the
//! length up to the start of segment p, and the last entry the whole length. Everything
//! that asks for a length builds the table with 100 samples a segment when it is missing.

use si_math::algebra::matrix_multiply;
use si_math::types::Matrix;
use si_x87::{fmov, x, Round, F80};

use crate::basis::{basis_of, matrix_precision};
use crate::edit::touch;
use crate::eval::{nb_segment, wrap_down, wrap_up, ONE_THIRD, TWO_THIRDS};
use crate::{held, Curve, Host, Vector, BEZIER, SEGMENT_LINEAR};

/// The samples a segment of a table built on demand.
pub const DEFAULT_SAMPLES: i16 = 100;

/// Below this distance (the double 0.0001, 0x08660628) [`distance_to_factor`] answers 0, and
/// within it of the whole length it answers the last parameter.
const DISTANCE_EPS: f64 = 1e-4;
/// The slack (the double 1e-06, 0x08660630) by which a table entry may exceed the distance
/// and still count as "not beyond it" in the search.
const SEARCH_EPS: f64 = 1e-6;

/// Builds the length table [soft 0x0848cda4 SI_splineUpdateLength].
///
/// `samples` is forced to 1 for a linear spline. With `touch_first` equal to 1 the spline
/// is touched first ([`touch`]: phantom points recomputed, the old table freed). A spline
/// without segments is left without a table. Otherwise:
///
/// 1. D = E(1/samples) × basis, the forward-difference matrix times the type's basis;
/// 2. for each segment, the geometry G as [`crate::eval::fill_geometry`] builds it (for a
///    type other than Bézier the four key indices are taken modulo the key count) and
///    F = D × G: rows third, second, first difference and point;
/// 3. the first point of the first segment starts the table at 0; then `samples` times:
///    row 3 += row 2, row 2 += row 1, row 1 += row 0 (each element rounded to float), and
///    the next entry is `float(√(dz² + (dy² + dx²)) + previous entry)`, the differences from
///    the previous point, their squares, the sum and the root all in the register.
///
/// The running point is **not** reset at a segment's start: the forward differences carry
/// their rounding along a segment, and the first step of the next segment is measured from
/// where the last one ended up, not from the key. Class: model.
///
/// The original overwrites the table pointer without freeing an old table (callers either
/// pass `touch_first` or have checked that there is none).
pub fn update_length(c: &mut Curve, samples: i16, touch_first: i16, host: &mut dyn Host) {
    let samples = if c.rec.kind == 0 { 1 } else { samples };
    if touch_first == 1 {
        touch(c, host);
    }
    let nseg = nb_segment(c);
    if nseg == 0 {
        return;
    }
    c.rec.nb_lengths = samples as u16;
    let steps = samples as i32;
    // SI_calloc(steps × segments + 1, 4): null for a count of 0 (finding 0010)
    let total = steps.wrapping_mul(nseg).wrapping_add(1);
    c.lengths = vec![0.0; total.max(0) as usize];
    c.rec.lengths = if total == 0 { si_types::Ptr32::NULL } else { held() };
    let d = matrix_multiply(&matrix_precision(steps), &basis_of(c.rec.kind, c.rec.tension));
    let nb = c.rec.nb_keys as u16 as i32;
    let row = |v: Vector| [fmov(v.x), fmov(v.y), fmov(v.z), 1.0];
    let mut count = 0usize;
    let mut prev = [0.0f32; 3];
    for seg in 0..nseg.max(0) {
        let mut g: Matrix = [[0.0, 0.0, 0.0, 1.0]; 4];
        if c.rec.kind == BEZIER {
            let k0 = c.key(seg as usize);
            let k1 = if seg == nb - 1 { c.key(0) } else { c.key(seg as usize + 1) };
            g[0] = row(k0.pos);
            g[3] = row(k1.pos);
            if k1.segment == SEGMENT_LINEAR {
                let mix = |a: f32, b: f32, wa: f64, wb: f64| (x(wa) * a + x(wb) * b).to_f32();
                g[1] = [
                    mix(k0.pos.x, k1.pos.x, TWO_THIRDS, ONE_THIRD),
                    mix(k0.pos.y, k1.pos.y, TWO_THIRDS, ONE_THIRD),
                    mix(k0.pos.z, k1.pos.z, TWO_THIRDS, ONE_THIRD),
                    1.0,
                ];
                g[2] = [
                    mix(k0.pos.x, k1.pos.x, ONE_THIRD, TWO_THIRDS),
                    mix(k0.pos.y, k1.pos.y, ONE_THIRD, TWO_THIRDS),
                    mix(k0.pos.z, k1.pos.z, ONE_THIRD, TWO_THIRDS),
                    1.0,
                ];
            } else {
                g[1] = row(k0.next);
                g[2] = row(k1.prev);
            }
        } else {
            for (k, r) in g.iter_mut().enumerate() {
                // a signed division by the key count; nb is not 0 when there are segments
                *r = row(c.key(((seg + k as i32) % nb) as usize).pos);
            }
        }
        let mut f = matrix_multiply(&d, &g);
        if count == 0 {
            set(&mut c.lengths, 0, 0.0);
            prev = [fmov(f[3][0]), fmov(f[3][1]), fmov(f[3][2])];
            count = 1;
        }
        for _ in 0..steps.max(0) {
            for r in (1..4).rev() {
                for j in 0..4 {
                    f[r][j] = (x(f[r][j]) + f[r - 1][j]).to_f32();
                }
            }
            let dx = x(prev[0]) - f[3][0];
            let dy = x(prev[1]) - f[3][1];
            let dz = x(prev[2]) - f[3][2];
            let before = c.lengths.get(count - 1).copied().unwrap_or(0.0);
            let entry = ((dz * dz + (dy * dy + dx * dx)).sqrt() + before).to_f32();
            set(&mut c.lengths, count, entry);
            prev = [f[3][0], f[3][1], f[3][2]];
            count += 1;
        }
    }
}

/// A table write. With a negative `samples` the original's table is a single word and it
/// writes none but entry 0; with ordinary arguments every write is inside the table.
fn set(table: &mut [f32], at: usize, v: f32) {
    if let Some(e) = table.get_mut(at) {
        *e = v;
    }
}

/// Entry `at` of the table as the original's address arithmetic finds it: the index times
/// four is taken modulo 2³², so the integer indefinite (what a NaN converts to) reads entry
/// 0. `None` where that address is outside the table: the original reads whatever memory is
/// there.
fn entry(c: &Curve, at: i32) -> Option<f32> {
    c.lengths.get((at as u32 & 0x3fff_ffff) as usize).copied()
}

fn build_if_missing(c: &mut Curve, host: &mut dyn Host) {
    if c.rec.lengths.is_null() {
        update_length(c, DEFAULT_SAMPLES, 1, host);
    }
}

/// The length of the whole curve [soft 0x0848df48 SI_splineLength]: the last table entry
/// (built first if there is no table), 0 for a spline without segments. The value is a
/// float; the original returns it in the register.
pub fn length(c: &mut Curve, host: &mut dyn Host) -> f32 {
    build_if_missing(c, host);
    let nseg = nb_segment(c);
    if nseg == 0 {
        return 0.0;
    }
    entry(c, (c.rec.nb_lengths as i32).wrapping_mul(nseg)).unwrap_or(0.0)
}

/// The parameter at a distance along the curve [soft 0x0848ca64 SI_splineDistanceToFactor],
/// unrounded as the original returns it.
///
/// 1. `|d| ≤ 0.0001` gives 0 at once (before any table is built).
/// 2. The distance is brought into the curve: clamped to `[0, length]` when open; when
///    closed, the length subtracted while `d > length`, then added while `d < 0`.
/// 3. Within 0.0001 of the whole length the answer is the number of segments.
/// 4. Otherwise an interpolation search over the table between `lo` = 0 and `hi` = the last
///    index: guess `lo + trunc(span × (d − t[lo]) / (t[hi] − t[lo]))`, moved one inside if it
///    falls on either end; an entry above `d + 1e-06` becomes the new `hi`, any other the
///    new `lo`; until the two are neighbours.
/// 5. The answer is `(lo + (d − t[lo]) / (t[hi] − t[lo])) / samples`.
///
/// All of it in the register; nothing is rounded to float. A NaN distance comes back as a
/// NaN. Where the original would not return (a closed curve of length 0 and a distance
/// beyond 0.0001; an infinite distance on a closed curve) the port's loops answer as
/// [`crate::eval::wrap_down`] says. Class: model.
pub fn distance_to_factor(c: &mut Curve, distance: f32, host: &mut dyn Host) -> F80 {
    let d = x(distance);
    if d.abs().le(x(DISTANCE_EPS)) {
        return F80::ZERO;
    }
    build_if_missing(c, host);
    let nseg = nb_segment(c);
    if nseg == 0 {
        return F80::ZERO;
    }
    let whole = x(length(c, host));
    let mut v = d;
    if c.rec.closed != 0 {
        if v.gt(whole) {
            v = wrap_down(v, whole, |t, m| t.gt(m), |r, m| if r.eq(F80::ZERO) { m } else { r });
        }
        if F80::ZERO.gt(v) {
            v = wrap_up(v, whole);
        }
    } else {
        if v.gt(whole) {
            v = whole;
        }
        if F80::ZERO.gt(v) {
            v = F80::ZERO;
        }
    }
    let samples = c.rec.nb_lengths as i32;
    let mut hi_at = samples.wrapping_mul(nseg);
    let mut hi = x(entry(c, hi_at).unwrap_or(0.0));
    let mut lo = x(entry(c, 0).unwrap_or(0.0));
    let mut lo_at = 0i32;
    if (v - hi).abs().le(x(DISTANCE_EPS)) {
        return F80::from_i32(nseg);
    }
    let mut span = hi_at;
    while span > 1 {
        let guess = (((v - lo) / (hi - lo)) * F80::from_i32(span)).to_i32(Round::Zero);
        let mut at = lo_at.wrapping_add(guess);
        if hi_at == at {
            at = hi_at.wrapping_sub(1);
        }
        if lo_at == at {
            at = lo_at.wrapping_add(1);
        }
        // outside the table the original reads other memory; the port reads 0
        let t = x(entry(c, at).unwrap_or(0.0));
        if t.gt(x(SEARCH_EPS) + v) {
            hi_at = at;
            hi = t;
        } else {
            lo_at = at;
            lo = t;
        }
        span = hi_at.wrapping_sub(lo_at);
    }
    let step = F80::from_i32(c.rec.nb_lengths as i32);
    if lo_at == hi_at {
        return F80::from_i32(lo_at) / step;
    }
    ((v - lo) / (hi - lo) + F80::from_i32(lo_at)) / step
}

/// The distance along the curve at a parameter [soft 0x0848cc5c SI_splineFactorToDistance],
/// unrounded.
///
/// The parameter is clamped to `[0, n]` (open) or wrapped by n while above n and while
/// negative (closed), in the register. With `k = trunc(samples × u)`: the table entry k
/// when k is the last index, else `t[k] + (samples × u − k) × (t[k + 1] − t[k])`.
/// Class: model.
pub fn factor_to_distance(c: &mut Curve, u: f32, host: &mut dyn Host) -> F80 {
    build_if_missing(c, host);
    let nseg = nb_segment(c);
    if nseg == 0 {
        return F80::ZERO;
    }
    let n = F80::from_i32(nseg);
    let mut v = x(u);
    if c.rec.closed != 0 {
        if n.lt(v) {
            v = wrap_down(v, n, |t, m| m.lt(t), |r, m| if r.eq(F80::ZERO) { m } else { r });
        }
        if F80::ZERO.gt(v) {
            v = wrap_up(v, n);
        }
    } else {
        if n.lt(v) {
            v = n;
        }
        if F80::ZERO.gt(v) {
            v = F80::ZERO;
        }
    }
    let samples = c.rec.nb_lengths as i32;
    let scaled = v * F80::from_i32(samples);
    let k = scaled.to_i32(Round::Zero);
    let at_k = x(entry(c, k).unwrap_or(0.0));
    if k == nseg.wrapping_mul(samples) {
        return at_k;
    }
    let frac = scaled - F80::from_i32(k);
    let next = x(entry(c, k.wrapping_add(1)).unwrap_or(0.0));
    at_k + frac * (next - at_k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NoHost, BSPLINE, LINEAR};

    #[test]
    fn linear_table_is_the_chord_lengths() {
        let mut c = Curve::from_points(LINEAR, false, &[[0.0, 0.0, 0.0], [3.0, 4.0, 0.0], [3.0, 4.0, 12.0]]);
        update_length(&mut c, 100, 0, &mut NoHost);
        assert_eq!(c.rec.nb_lengths, 1);
        assert_eq!(c.lengths, [0.0, 5.0, 17.0]);
        assert_eq!(length(&mut c, &mut NoHost), 17.0);
        assert_eq!(distance_to_factor(&mut c, 2.5, &mut NoHost).to_f64(), 0.5);
        assert_eq!(distance_to_factor(&mut c, 11.0, &mut NoHost).to_f64(), 1.5);
        assert_eq!(distance_to_factor(&mut c, 17.00005, &mut NoHost).to_f64(), 2.0);
        assert_eq!(distance_to_factor(&mut c, 0.00005, &mut NoHost).to_f64(), 0.0);
        assert_eq!(distance_to_factor(&mut c, -4.0, &mut NoHost).to_f64(), 0.0);
        assert_eq!(factor_to_distance(&mut c, 1.5, &mut NoHost).to_f64(), 11.0);
        assert_eq!(factor_to_distance(&mut c, 9.0, &mut NoHost).to_f64(), 17.0);
        assert!(distance_to_factor(&mut c, f32::NAN, &mut NoHost).is_nan());
    }

    #[test]
    fn table_is_built_on_demand_with_a_hundred_samples() {
        let pts: Vec<[f32; 3]> = (0..6).map(|i| [i as f32, (i % 2) as f32, 0.0]).collect();
        let mut c = Curve::from_points(BSPLINE, true, &pts);
        let l = length(&mut c, &mut NoHost);
        assert_eq!(c.rec.nb_lengths, 100);
        assert_eq!(c.lengths.len(), 601);
        assert!(l > 5.0 && l < 12.0);
        // a closed curve wraps distances
        let a = distance_to_factor(&mut c, 1.0, &mut NoHost).to_f64();
        let b = distance_to_factor(&mut c, 1.0 + l, &mut NoHost).to_f64();
        assert!((a - b).abs() < 1e-4, "{a} {b}");
        let back = factor_to_distance(&mut c, a as f32, &mut NoHost).to_f64();
        assert!((back - 1.0).abs() < 1e-4);
        assert_eq!(length(&mut Curve::new(), &mut NoHost), 0.0);
    }
}
