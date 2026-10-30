// SPDX-License-Identifier: MIT
//! Evaluation at a parameter: segment counts, the mapping from a global parameter to a
//! segment and a local parameter, and position, tangent and second derivative.
//!
//! **The parameter.** A curve of `n` segments ([`nb_segment`]) is parametrised over
//! `[0, n]`. An open curve clamps a parameter to that range; a closed one wraps it by
//! adding or subtracting `n`. The whole part is then peeled off one unit at a time, each
//! subtraction rounded to float, and counts the segment.
//!
//! **Keys and segments.** A linear or Bézier curve has a segment between consecutive keys
//! (`keys − 1`, plus one when closed). A B-spline or cardinal curve needs four consecutive
//! keys a segment (`keys − 3`, or `keys` when closed), so its first and last keys are not
//! on the curve unless made so (the phantom points of [`crate::edit::set_dummy`]).

use si_math::algebra::{hvector_multiply_matrix, matrix_multiply};
use si_math::types::{Hvector, Matrix};
use si_x87::{fmov, x, F80};

use crate::basis::basis_of;
use crate::{Curve, Vector, BEZIER, SEGMENT_LINEAR};

/// How many subtractions a wrapping loop is followed literally before its end is computed
/// in closed form (see [`wrap_down`]).
const LOOP_BUDGET: u32 = 1 << 16;

/// Two thirds and one third as the original has them: the **doubles** nearest the decimal
/// numbers 0.6666667 and 0.3333333 (0x086606d0, 0x086606d8; a second copy at 0x08660638,
/// 0x08660640). They place the inner control points of a straight Bézier segment.
pub(crate) const TWO_THIRDS: f64 = f64::from_bits(0x3fe5_5555_673a_a1bc);
pub(crate) const ONE_THIRD: f64 = f64::from_bits(0x3fd5_5555_318a_bc87);

/// Segments and keys-per-segment step for a type, a key count and a closed flag
/// [soft 0x0848c848 SI_splineNbSegmentStep]: returns `(segments, step)`.
///
/// This routine counts a Bézier curve by **control points** (three to a segment, the
/// layout of [`crate::convert::bezier_expand`]), unlike [`nb_segment`], which counts keys.
/// Types above 3 give no segments.
pub fn nb_segment_step(kind: i32, nb: i32, closed: i16) -> (i32, i32) {
    let closed = closed as i32;
    match kind {
        1 => (if nb > 3 - closed { (nb - 1 + closed) / 3 } else { 0 }, 3),
        0 => (if nb > 1 { nb - 1 + closed } else { 0 }, 1),
        2 | 3 => (if nb > 3 - closed { nb + closed * 3 - 3 } else { 0 }, 1),
        _ => (0, 1),
    }
}

/// The number of segments of a spline [soft 0x0848c8dc SI_splineNbSegment].
///
/// Types 0 and 1 (compared unsigned): `keys − 1 + closed` when there are at least two
/// keys. Every other type, 4 and above included: `keys − 3 + 3 × closed` when
/// `keys > 3 − closed`. `closed` is used as a number, not as a flag.
pub fn nb_segment(c: &Curve) -> i32 {
    let nb = c.rec.nb_keys as u16 as i32;
    let closed = c.rec.closed as i32;
    if (c.rec.kind as u32) < 2 {
        if nb > 1 {
            nb - 1 + closed
        } else {
            0
        }
    } else if 3 - closed < nb {
        closed * 3 - 3 + nb
    } else {
        0
    }
}

/// `do t −= m while keep(t, m)` in the register, as the wrapping loops of the original
/// run it (no rounding to float between the turns).
///
/// The original has no bound on this loop. For a float `t` up to 2⁴⁰ times `m` every
/// subtraction is exact, so the loop ends in the exact remainder; the port follows it for
/// [`LOOP_BUDGET`] turns and then computes that remainder directly, which is the same
/// value. **Beyond that ratio, and for an infinite `t` or a zero `m`, the original does not
/// return** (the subtraction stops changing the value, or takes more turns than any run
/// could wait for); the port answers 0 there, and that is the one place it differs.
pub(crate) fn wrap_down(mut t: F80, m: F80, keep: fn(F80, F80) -> bool, at_end: fn(F80, F80) -> F80) -> F80 {
    for _ in 0..LOOP_BUDGET {
        t = t - m;
        if !keep(t, m) {
            return t;
        }
    }
    let ratio = t / m;
    if ratio.is_nan() || !ratio.lt(x(1.099_511_627_776e12f64)) {
        return F80::ZERO;
    }
    at_end(t.fmod(m), m)
}

/// `do t += m while t < 0`, with the same budget and the same remark as [`wrap_down`].
pub(crate) fn wrap_up(mut t: F80, m: F80) -> F80 {
    for _ in 0..LOOP_BUDGET {
        t = t + m;
        if !t.lt(F80::ZERO) {
            return t;
        }
    }
    let ratio = t / m;
    if ratio.is_nan() || !(-ratio).lt(x(1.099_511_627_776e12f64)) {
        return F80::ZERO;
    }
    let r = (-t).fmod(m);
    if r.eq(F80::ZERO) {
        F80::ZERO
    } else {
        m - r
    }
}

/// The geometry of the segment a parameter falls in [soft 0x0848f970 splineFillGeometry].
///
/// Returns `None` (the original's 0, with `u` untouched) for a spline without segments.
/// Otherwise `u` is replaced by the local parameter and the result is the matrix `G`,
/// one row a point with a fourth component of 1:
///
/// - open curve: `u ≤ 0` becomes 0, `u ≥ n` becomes n (a NaN passes both tests and stays);
/// - closed curve: while `u ≥ n` subtract n, then while `u < 0` add n, both in the register
///   and stored to float once at the end of each loop (so a tiny negative parameter becomes
///   exactly n);
/// - then while `u ≥ 1`: `u = float(u − 1)`, counting the segment `s`. A parameter of
///   exactly n therefore ends as segment n with u = 0, one past the last segment;
/// - Bézier: rows are key s's position, its `next` handle, the `prev` handle of the
///   following key (key 0 after the last) and that key's position; when the following
///   key's segment type is 2 the two inner rows are instead `p0 × 0.6666667 + p3 × 0.3333333`
///   and `p0 × 0.3333333 + p3 × 0.6666667` (doubles; products and sum in the register);
/// - every other type: the positions of keys s, s+1, s+2, s+3, each index returning to 0
///   after the last key.
///
/// Segment n of a closed curve starts at key `n`, which is one record past the keys in use:
/// the original reads whatever its key block holds there (a cleared record, or a stale key
/// left by a deletion). The port reads the same record of [`Curve::keys`] when the block
/// has it and a zero key otherwise.
pub fn fill_geometry(c: &Curve, u: &mut f32) -> Option<Matrix> {
    let nseg = nb_segment(c);
    if nseg <= 0 {
        return None;
    }
    let n = F80::from_i32(nseg);
    if c.rec.closed == 0 {
        let t = x(*u);
        if !(t.gt(F80::ZERO) || t.unordered(F80::ZERO)) {
            *u = 0.0;
        } else if !(n.gt(t) || n.unordered(t)) {
            *u = n.to_f32();
        }
    } else {
        let t = x(*u);
        if t.ge(n) {
            *u = wrap_down(t, n, |t, n| t.ge(n), |r, _| r).to_f32();
        }
        let t = x(*u);
        if t.lt(F80::ZERO) {
            *u = wrap_up(t, n).to_f32();
        }
    }
    let mut seg = 0usize;
    let mut t = x(*u);
    if t.ge(F80::ONE) {
        loop {
            t = x((t - F80::ONE).to_f32());
            seg += 1;
            if !t.ge(F80::ONE) {
                break;
            }
        }
        *u = t.to_f32();
    }
    let nb = c.rec.nb_keys as u16 as i32;
    let after = |i: usize| if i as i32 == nb - 1 { 0 } else { i + 1 };
    let row = |v: Vector| [fmov(v.x), fmov(v.y), fmov(v.z), 1.0];
    let k0 = c.key(seg);
    let i1 = after(seg);
    let k1 = c.key(i1);
    let mut g = [[0.0f32; 4]; 4];
    g[0] = row(k0.pos);
    if c.rec.kind == BEZIER {
        g[3] = row(k1.pos);
        if k1.segment == SEGMENT_LINEAR {
            let mix = |a: f32, b: f32, wa: f64, wb: f64| (x(a) * wa + x(wb) * b).to_f32();
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
        g[1] = row(k1.pos);
        let i2 = after(i1);
        g[2] = row(c.key(i2).pos);
        g[3] = row(c.key(after(i2)).pos);
    }
    Some(g)
}

/// The polynomial coefficients of a segment: basis × geometry, rows for u³, u², u, 1.
fn coefficients(c: &Curve, g: &Matrix) -> Matrix {
    matrix_multiply(&basis_of(c.rec.kind, c.rec.tension), g)
}

/// The point at a parameter [soft 0x0848b1e4 SI_splineValue].
///
/// With M = basis × geometry (each element rounded to float by the matrix product), u² and
/// u³ = u² × u kept in the register, each coordinate is
/// `float(((u × M[2] + (u² × M[1] + u³ × M[0])) + M[3]))`. Returns the point and the
/// original's result: for a spline without segments, its first key's position and 1, or
/// zeros and 0 when it has no keys at all. Class: model.
pub fn value(c: &Curve, u: f32) -> (Vector, i32) {
    let mut u = u;
    let Some(g) = fill_geometry(c, &mut u) else {
        if c.rec.nb_keys == 0 {
            return (Vector { x: 0.0, y: 0.0, z: 0.0 }, 0);
        }
        return (c.key(0).pos, 1);
    };
    let m = coefficients(c, &g);
    let u1 = x(u);
    let u2 = u1 * u1;
    let u3 = u2 * u1;
    let at = |j: usize| ((u1 * m[2][j] + (u2 * m[1][j] + u3 * m[0][j])) + m[3][j]).to_f32();
    (Vector { x: at(0), y: at(1), z: at(2) }, 1)
}

/// The first derivative with respect to the parameter [soft 0x0848b00c SI_splineTangent]:
/// the row `[1.5 × u × 2u, 2u, 1, 0]` (2u rounded to float first; the first element
/// `float((u × float(u + u)) × 1.5)`, which is 3u²) times M, through
/// `SI_hvectorMultiplyMatrix`, whose running sum is rounded to float after every term.
/// It is not normalised. `None` is the original's 0, for a spline without keys (the
/// destination is then left alone); a spline with keys but no segments gives zeros.
/// Class: model.
pub fn tangent(c: &Curve, u: f32) -> Option<Vector> {
    let mut u = u;
    let Some(g) = fill_geometry(c, &mut u) else {
        if c.rec.nb_keys == 0 {
            return None;
        }
        return Some(Vector { x: 0.0, y: 0.0, z: 0.0 });
    };
    let m = coefficients(c, &g);
    let two_u = (x(u) + u).to_f32();
    let h = Hvector::new(((x(u) * two_u) * 1.5f64).to_f32(), two_u, 1.0, 0.0);
    let r = hvector_multiply_matrix(&h, &m);
    Some(Vector { x: fmov(r.x), y: fmov(r.y), z: fmov(r.z) })
}

/// The second derivative with respect to the parameter
/// [soft 0x0848b10c SI_splineAcceleration]: the row `[float(u × 6), 2, 0, 0]` times M. `None` (the
/// original's 0, destination untouched) for a spline without segments. Class: model.
pub fn acceleration(c: &Curve, u: f32) -> Option<Vector> {
    let mut u = u;
    let g = fill_geometry(c, &mut u)?;
    let m = coefficients(c, &g);
    let h = Hvector::new((x(u) * 6.0f64).to_f32(), 2.0, 0.0, 0.0);
    let r = hvector_multiply_matrix(&h, &m);
    Some(Vector { x: fmov(r.x), y: fmov(r.y), z: fmov(r.z) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BSPLINE, CARDINAL, LINEAR};

    fn pts(n: usize) -> Vec<[f32; 3]> {
        (0..n).map(|i| [i as f32, (i * i) as f32, 0.5 * i as f32]).collect()
    }

    #[test]
    fn segment_counts() {
        for (kind, closed, nb, want) in [
            (LINEAR, false, 1, 0), (LINEAR, false, 2, 1), (LINEAR, true, 2, 2), (BEZIER, false, 5, 4), (BEZIER, true, 5, 5),
            (BSPLINE, false, 3, 0), (BSPLINE, false, 4, 1), (BSPLINE, true, 3, 3), (BSPLINE, true, 2, 0), (CARDINAL, false, 7, 4),
        ] {
            let c = Curve::from_points(kind, closed, &pts(nb));
            assert_eq!(nb_segment(&c), want, "{kind} {closed} {nb}");
        }
        assert_eq!(nb_segment_step(1, 7, 0), (2, 3));
        assert_eq!(nb_segment_step(1, 6, 1), (2, 3));
        assert_eq!(nb_segment_step(0, 5, 1), (5, 1));
        assert_eq!(nb_segment_step(3, 4, 0), (1, 1));
        assert_eq!(nb_segment_step(4, 9, 0), (0, 1));
    }

    #[test]
    fn linear_and_cardinal_pass_through_their_keys() {
        let p = pts(6);
        let lin = Curve::from_points(LINEAR, false, &p);
        for (i, q) in p.iter().enumerate() {
            let (v, ok) = value(&lin, i as f32);
            assert_eq!((v.x, v.y, v.z, ok), (q[0], q[1], q[2], 1));
        }
        assert_eq!(value(&lin, 2.5).0.x, 2.5);
        assert_eq!(value(&lin, -3.0).0.x, 0.0);
        assert_eq!(value(&lin, 99.0).0.x, 5.0);
        let mut car = Curve::from_points(CARDINAL, false, &p);
        car.rec.tension = 0.5;
        // segment s runs from key s + 1 to key s + 2
        for s in 0..3 {
            let v = value(&car, s as f32).0;
            assert_eq!((v.x, v.y), (p[s + 1][0], p[s + 1][1]));
        }
        assert_eq!(value(&car, 3.0).0.x, p[4][0]);
    }

    #[test]
    fn closed_parameters_wrap() {
        let c = Curve::from_points(LINEAR, true, &pts(4));
        assert_eq!(value(&c, 4.5).0, value(&c, 0.5).0);
        assert_eq!(value(&c, -0.5).0, value(&c, 3.5).0);
        assert_eq!(value(&c, 3.5).0.x, 1.5); // from key 3 back to key 0
        let mut u = 1.0e9f32;
        assert!(fill_geometry(&c, &mut u).is_some());
        assert_eq!(u, 0.0); // 1e9 is a multiple of 4: the closed form after the budget
        let mut u = f32::INFINITY;
        assert!(fill_geometry(&c, &mut u).is_some());
        assert_eq!(u, 0.0); // the original would not return
    }

    #[test]
    fn straight_bezier_segment_ignores_the_handles() {
        let mut c = Curve::from_points(BEZIER, false, &[[0.0, 0.0, 0.0], [3.0, 0.0, 0.0]]);
        c.keys[0].next = Vector { x: 0.0, y: 50.0, z: 0.0 };
        c.keys[1].prev = Vector { x: 3.0, y: 50.0, z: 0.0 };
        assert!(value(&c, 0.5).0.y > 30.0);
        c.keys[1].segment = SEGMENT_LINEAR;
        let v = value(&c, 0.5).0;
        assert_eq!(v.y, 0.0);
        assert!((v.x - 1.5).abs() < 1e-6);
        assert_eq!(tangent(&c, 0.5).unwrap().y, 0.0);
        assert!(acceleration(&Curve::new(), 0.0).is_none());
        assert!(tangent(&Curve::new(), 0.0).is_none());
    }
}
