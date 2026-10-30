// SPDX-License-Identifier: MIT
//! Curve fitting: the building blocks of `fitCurve.c` (finding 0020, section 7).
//!
//! The original fits Bézier segments to a run of points by least squares, the method (and
//! the function names) of the "FitCurves" code of *Graphics Gems* (1990), in three
//! dimensions and in **doubles**: a point is an `SI_DblPnt`, three doubles. Ported and
//! checked here are the vector helpers, the Bernstein polynomials, de Casteljau's
//! evaluation, the three tangent estimates, the chord-length parameters, the error
//! measure, the Newton-Raphson reparameterisation and the least-squares step
//! ([`generate_bezier`]). The recursion that drives them (`fitCubic`, `SI_fitBezCurve`,
//! `SI_fitCurve`, `drawBezierCurve`) is in [`crate::fit_image`], on the memory image.
//!
//! Intermediate values stay in the 80-bit register and are rounded where the original
//! stores them: to double in a point, to **float** for every vector length.

use si_x87::{x, F80};

/// `SI_DblPnt`: three doubles.
pub type DblPnt = [f64; 3];

const TINY: f64 = 1.0e-4;

fn d(v: F80) -> f64 {
    v.to_f64()
}

/// `a + b`, each coordinate rounded to double [soft 0x08439d94 v2AddII].
pub fn v2_add(a: DblPnt, b: DblPnt) -> DblPnt {
    [d(x(a[0]) + b[0]), d(x(a[1]) + b[1]), d(x(a[2]) + b[2])]
}

/// `v × s` [soft 0x08439dd0 v2ScaleII].
pub fn v2_scale(v: DblPnt, s: f64) -> DblPnt {
    [d(x(v[0]) * s), d(x(v[1]) * s), d(x(s) * v[2])]
}

/// `a − b` [soft 0x08439e10 v2Sub].
pub fn v2_sub(a: DblPnt, b: DblPnt) -> DblPnt {
    [d(x(a[0]) - b[0]), d(x(a[1]) - b[1]), d(x(a[2]) - b[2])]
}

/// The cosine of the angle between two vectors [soft 0x08436fe0 dvectorCosAngle]:
/// `(a·b) / (|a| × |b|)`, or 1 when the product of the lengths is below 0.0001. The result
/// is left in the register (80 bits).
pub fn dvector_cos_angle(a: &DblPnt, b: &DblPnt) -> F80 {
    let la = (((x(a[1]) * a[1]) + (x(a[0]) * a[0])) + (x(a[2]) * a[2])).sqrt();
    let lb = (((x(b[1]) * b[1]) + (x(b[0]) * b[0])) + (x(b[2]) * b[2])).sqrt();
    let prod = la * lb;
    if prod.lt(x(TINY)) {
        return F80::ONE;
    }
    ((x(a[2]) * b[2]) + ((x(b[1]) * a[1]) + (x(b[0]) * a[0]))) / prod
}

/// The cosine of the turn at `b` on the way from `a` to `c`
/// [soft 0x08437068 dpointsCosAngle]: [`dvector_cos_angle`] of `b − a` and `c − b`, each
/// difference rounded to double. 1 is straight on; the fit calls a point a corner when this
/// is below the cosine of its angle argument.
pub fn dpoints_cos_angle(a: &DblPnt, b: &DblPnt, c: &DblPnt) -> F80 {
    dvector_cos_angle(&v2_sub(*b, *a), &v2_sub(*c, *b))
}

/// The point at `t` of the Bézier curve of the given degree over `v[0..=degree]`
/// [soft 0x084397e0 bezierValue]: de Casteljau's triangle on a copy, every level rounded to
/// double, `1 − t` kept in the register. A negative degree returns `v[0]`'s place in an
/// unfilled copy (uninitialised memory in the original; `None` here).
pub fn bezier_value(degree: i32, v: &[DblPnt], t: f64) -> Option<DblPnt> {
    if degree < 0 || v.len() < degree as usize + 1 {
        return None;
    }
    let mut w: Vec<DblPnt> = v[..=degree as usize].to_vec();
    let s = F80::ONE - t;
    for i in 1..=degree as usize {
        for j in 0..=(degree as usize - i) {
            w[j] = [d((s * w[j][0]) + (x(t) * w[j + 1][0])), d((x(t) * w[j + 1][1]) + (s * w[j][1])), d((x(t) * w[j + 1][2]) + (s * w[j][2]))];
        }
    }
    Some(w[0])
}

/// The cubic Bernstein polynomials, results left in the register:
/// `(1 − t)³` [soft 0x084398f0 bezMult0].
pub fn bez_mult0(t: f64) -> F80 {
    let s = F80::ONE - t;
    s * (s * s)
}

/// `3t(1 − t)²` [soft 0x08439904 bezMult1].
pub fn bez_mult1(t: f64) -> F80 {
    (x(t) * 3.0f64) * ((F80::ONE - t) * (F80::ONE - t))
}

/// `3t²(1 − t)` [soft 0x08439920 bezMult2].
pub fn bez_mult2(t: f64) -> F80 {
    (F80::ONE - t) * ((x(t) * 3.0f64) * t)
}

/// `t³` [soft 0x08439940 bezMult3].
pub fn bez_mult3(t: f64) -> F80 {
    (x(t) * t) * t
}

/// A direction made a unit vector the way all three tangent routines do it: the length is
/// `float(√((y² + x²) + z²))` of the unrounded components, and when its absolute value is
/// not above 0.0001 the vector is returned as it is, each component rounded to double.
fn unit(v: [F80; 3], z_first: bool) -> DblPnt {
    let sum = if z_first { (v[2] * v[2]) + ((v[0] * v[0]) + (v[1] * v[1])) } else { ((v[1] * v[1]) + (v[0] * v[0])) + (v[2] * v[2]) };
    let len = sum.sqrt().to_f32();
    if x(len).abs().gt(x(TINY)) {
        [d(v[0] / len), d(v[1] / len), d(v[2] / len)]
    } else {
        [d(v[0]), d(v[1]), d(v[2])]
    }
}

/// The unit direction from point `end` to the next [soft 0x08439950 computeLeftTangent].
pub fn compute_left_tangent(pts: &[DblPnt], end: i32) -> Option<DblPnt> {
    let (a, b) = (pts.get(usize::try_from(end).ok()?)?, pts.get(usize::try_from(end + 1).ok()?)?);
    Some(unit([x(b[0]) - a[0], x(b[1]) - a[1], x(b[2]) - a[2]], false))
}

/// The unit direction from point `end` to the one before
/// [soft 0x084399f4 computeRightTangent].
pub fn compute_right_tangent(pts: &[DblPnt], end: i32) -> Option<DblPnt> {
    let (a, b) = (pts.get(usize::try_from(end).ok()?)?, pts.get(usize::try_from(end - 1).ok()?)?);
    Some(unit([x(b[0]) - a[0], x(b[1]) - a[1], x(b[2]) - a[2]], false))
}

/// The tangent at an interior point [soft 0x08439a98 computeCenterTangent]: half the sum of
/// `p[c − 1] − p[c]` and `p[c] − p[c + 1]` (each a double), made a unit vector. It points
/// **backwards** along the points, as a right tangent does.
pub fn compute_center_tangent(pts: &[DblPnt], center: i32) -> Option<DblPnt> {
    let c = usize::try_from(center).ok()?;
    let (p0, p1, p2) = (pts.get(c.checked_sub(1)?)?, pts.get(c)?, pts.get(c + 1)?);
    let (v1, v2) = (v2_sub(*p0, *p1), v2_sub(*p1, *p2));
    Some(unit([(x(v1[0]) + v2[0]) * 0.5f64, (x(v1[1]) + v2[1]) * 0.5f64, x(0.5f64) * (x(v1[2]) + v2[2])], true))
}

/// The parameters of points `first..=last` by chord length
/// [soft 0x08439bd4 chordLengthParameterize]: the cumulated distances (each sum rounded to
/// double), every one but the first then divided by the last. Two points at one place give
/// 0 / 0. `None` for `last < first` (the original writes through a null pointer).
pub fn chord_length_parameterize(pts: &[DblPnt], first: i32, last: i32) -> Option<Vec<f64>> {
    if last < first || first < 0 || last as usize >= pts.len() {
        return None;
    }
    let (first, last) = (first as usize, last as usize);
    let mut u = vec![0.0f64; last - first + 1];
    for i in first + 1..=last {
        let (p, q) = (&pts[i], &pts[i - 1]);
        let (dx, dy, dz) = (x(p[0]) - q[0], x(p[1]) - q[1], x(p[2]) - q[2]);
        u[i - first] = d((((dx * dx) + (dy * dy)) + (dz * dz)).sqrt() + u[i - first - 1]);
    }
    let total = u[last - first];
    for v in u.iter_mut().skip(1) {
        *v = d(x(*v) / total);
    }
    Some(u)
}

/// The largest squared distance of an interior point from the curve at its parameter, and
/// the point it is at [soft 0x08439c90 computeMaxError]. The split point starts as
/// `(last − first + 1) / 2` and is replaced by every point whose distance is **not below**
/// the largest so far (so the last of several equal ones wins, and a point exactly on the
/// curve takes it from the default).
pub fn compute_max_error(pts: &[DblPnt], first: i32, last: i32, bez: &[DblPnt; 4], u: &[f64]) -> Option<(f64, i32)> {
    let mut split = (last - first + 1) / 2;
    let mut max = 0.0f64;
    for i in first + 1..last {
        let p = bezier_value(3, bez, *u.get(usize::try_from(i - first).ok()?)?)?;
        let v = v2_sub(p, *pts.get(usize::try_from(i).ok()?)?);
        let dist = ((x(v[0]) * v[0]) + (x(v[1]) * v[1])) + (x(v[2]) * v[2]);
        if !(dist.lt(x(max)) || dist.unordered(x(max))) {
            max = d(dist);
            split = i;
        }
    }
    Some((max, split))
}

/// One Newton-Raphson step for the parameter at which the curve `q` is nearest to `p`
/// [soft 0x084395a8 newtonRaphsonRootFind]: `u − ((Q(u) − p)·Q′(u)) / (Q′(u)·Q′(u) +
/// (Q(u) − p)·Q″(u))`, with `Q′`'s control points `3 × (qᵢ₊₁ − qᵢ)` and `Q″`'s twice the
/// differences of those (formed as `d + d`), all doubles. There is no guard on the
/// denominator. The result is left in the register.
pub fn newton_raphson_root_find(q: &[DblPnt; 4], p: DblPnt, u: f64) -> F80 {
    let qu = bezier_value(3, q, u).unwrap();
    let mut q1 = [[0.0f64; 3]; 3];
    for i in 0..3 {
        for k in 0..3 {
            q1[i][k] = d((x(q[i + 1][k]) - q[i][k]) * 3.0f64);
        }
    }
    let mut q2 = [[0.0f64; 3]; 2];
    for i in 0..2 {
        for k in 0..3 {
            let diff = x(q1[i + 1][k]) - q1[i][k];
            q2[i][k] = d(diff + diff);
        }
    }
    let q1u = bezier_value(2, &q1, u).unwrap();
    let q2u = bezier_value(1, &q2, u).unwrap();
    let (t1, t2, t3) = (x(qu[0]) - p[0], x(qu[1]) - p[1], x(qu[2]) - p[2]);
    let num = (t3 * q1u[2]) + ((t2 * q1u[1]) + (t1 * q1u[0]));
    let den = (t3 * q2u[2]) + (((t1 * q2u[0]) + ((x(q1u[2]) * q1u[2]) + ((x(q1u[1]) * q1u[1]) + (x(q1u[0]) * q1u[0])))) + (t2 * q2u[1]));
    x(u) - (num / den)
}

/// A better parameter for each of the points `first..=last`
/// [soft 0x08439518 reparameterize]: one [`newton_raphson_root_find`] step each, stored as
/// doubles.
pub fn reparameterize(pts: &[DblPnt], first: i32, last: i32, u: &[f64], bez: &[DblPnt; 4]) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    for i in first..=last {
        let p = *pts.get(usize::try_from(i).ok()?)?;
        out.push(d(newton_raphson_root_find(bez, p, *u.get(usize::try_from(i - first).ok()?)?)));
    }
    Some(out)
}

/// The least-squares cubic through points `first..=last` with given end directions
/// [soft 0x08438bf4 generateBezier]: the end points are the first and last point, and the
/// two handles lie along `t_hat1` and `t_hat2` at the distances the fit gives.
///
/// 1. Per point, `A₀ = t_hat1 × (B₁(u) / |t_hat1|)` and `A₁ = t_hat2 × (B₂(u) / |t_hat2|)`,
///    the length a float; **a direction whose length is not above 0.0001 is used as it is**,
///    without the Bernstein factor.
/// 2. `C = Σ [A₀·A₀, A₀·A₁; A₀·A₁, A₁·A₁]` and `X = Σ [A₀·tmp, A₁·tmp]` with
///    `tmp = p − (p_first B₀ + (p_first B₁ + (p_last B₂ + p_last B₃)))`, every vector
///    operation rounded to double and every sum stored as a double.
/// 3. `det = C₀₀C₁₁ − C₁₀C₀₁`; **when it is exactly 0 it becomes `C₀₀C₁₁ × 1e-11`**.
///    `αl = (C₁₁X₀ − C₀₁X₁) / det`, `αr = (C₀₀X₁ − C₀₁X₀) / det`, in the register.
/// 4. When either is below 0 both handles are put a third of the chord from their ends
///    (`√(…) / 3.0`); else at αl and αr. The direction is normalised again here (the same
///    float length, the same 0.0001), so a degenerate direction gives a handle at
///    `end + direction`, unscaled.
///
/// The result is the four control points. NaNs from a zero determinant with all-zero sums
/// pass both "below 0" tests and come out in the handles.
pub fn generate_bezier(pts: &[DblPnt], first: i32, last: i32, u: &[f64], t_hat1: DblPnt, t_hat2: DblPnt) -> Option<[DblPnt; 4]> {
    generate_bezier_with_a(pts, first, last, u, t_hat1, t_hat2).map(|r| r.1)
}

/// [`generate_bezier`] with the table `A` it builds on the way (one pair of vectors a
/// point, 0x30 bytes each in the original's `SI_calloc` block): the image form
/// ([`crate::fit_image::generate_bezier`]) leaves it in that block.
pub fn generate_bezier_with_a(pts: &[DblPnt], first: i32, last: i32, u: &[f64], t_hat1: DblPnt, t_hat2: DblPnt) -> Option<(Vec<[DblPnt; 2]>, [DblPnt; 4])> {
    let n = last - first + 1;
    let length = |v: &DblPnt| (((x(v[1]) * v[1]) + (x(v[0]) * v[0])) + (x(v[2]) * v[2])).sqrt().to_f32();
    let length_a = |v: &DblPnt| (((x(v[0]) * v[0]) + (x(v[1]) * v[1])) + (x(v[2]) * v[2])).sqrt().to_f32();
    let p_first = *pts.get(usize::try_from(first).ok()?)?;
    let p_last = *pts.get(usize::try_from(last).ok()?)?;
    let mut a: Vec<[DblPnt; 2]> = Vec::new();
    for i in 0..n.max(0) {
        let ui = *u.get(i as usize)?;
        let (mut v1, mut v2) = (t_hat1, t_hat2);
        let l1 = length_a(&v1) as f64;
        if x(l1).abs().gt(x(TINY)) {
            for c in v1.iter_mut() {
                *c = d((bez_mult1(ui) / l1) * *c);
            }
        }
        let l2 = length_a(&v2) as f64;
        if x(l2).abs().gt(x(TINY)) {
            for c in v2.iter_mut() {
                *c = d((bez_mult2(ui) / l2) * *c);
            }
        }
        a.push([v1, v2]);
    }
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for i in 0..n.max(0) {
        let [a0, a1] = a[i as usize];
        let ui = u[i as usize];
        c00 = d((((x(a0[0]) * a0[0]) + (x(a0[1]) * a0[1])) + (x(a0[2]) * a0[2])) + c00);
        c01 = d((((x(a0[0]) * a1[0]) + (x(a0[1]) * a1[1])) + (x(a0[2]) * a1[2])) + c01);
        c11 = d((((x(a1[0]) * a1[0]) + (x(a1[1]) * a1[1])) + (x(a1[2]) * a1[2])) + c11);
        let t3 = v2_scale(p_last, d(bez_mult3(ui)));
        let t2 = v2_scale(p_last, d(bez_mult2(ui)));
        let s = v2_add(t2, t3);
        let t1 = v2_scale(p_first, d(bez_mult1(ui)));
        let s = v2_add(t1, s);
        let t0 = v2_scale(p_first, d(bez_mult0(ui)));
        let s = v2_add(t0, s);
        let tmp = v2_sub(*pts.get(usize::try_from(first + i).ok()?)?, s);
        x0 = d((((x(a0[1]) * tmp[1]) + (x(a0[0]) * tmp[0])) + (x(a0[2]) * tmp[2])) + x0);
        x1 = d(((x(tmp[2]) * a1[2]) + ((x(tmp[1]) * a1[1]) + (x(tmp[0]) * a1[0]))) + x1);
    }
    let c10 = c01;
    let prod = x(c00) * c11;
    let mut det = prod - (x(c10) * c01);
    if det.eq(F80::ZERO) {
        det = prod * 1.0e-11f64;
    }
    let alpha_l = ((x(c11) * x0) - (x(c01) * x1)) / det;
    let alpha_r = ((x(c00) * x1) - (x(c01) * x0)) / det;
    let (dist_l, dist_r) = if alpha_l.lt(F80::ZERO) || alpha_r.lt(F80::ZERO) {
        let (dx, dy, dz) = (x(p_last[0]) - p_first[0], x(p_last[1]) - p_first[1], x(p_last[2]) - p_first[2]);
        let third = (((dx * dx) + (dy * dy)) + (dz * dz)).sqrt() / 3.0f64;
        (third, third)
    } else {
        (alpha_l, alpha_r)
    };
    let handle = |end: DblPnt, dir: DblPnt, dist: F80| -> DblPnt {
        let l = length(&dir);
        let dir = if x(l).abs().gt(x(TINY)) {
            let s = dist / l;
            [d(x(dir[0]) * s), d(x(dir[1]) * s), d(s * dir[2])]
        } else {
            dir
        };
        [d(x(end[0]) + dir[0]), d(x(end[1]) + dir[1]), d(x(end[2]) + dir[2])]
    };
    let curve = [p_first, handle(p_first, t_hat1, dist_l), handle(p_last, t_hat2, dist_r), p_last];
    Some((a, curve))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bernstein_and_de_casteljau_agree() {
        let q = [[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [3.0, 2.0, 1.0], [4.0, 0.0, 1.0]];
        for t in [0.0, 0.25, 0.5, 1.0] {
            let p = bezier_value(3, &q, t).unwrap();
            let b = [bez_mult0(t).to_f64(), bez_mult1(t).to_f64(), bez_mult2(t).to_f64(), bez_mult3(t).to_f64()];
            for k in 0..3 {
                let want: f64 = (0..4).map(|i| b[i] * q[i][k]).sum();
                assert!((p[k] - want).abs() < 1e-12);
            }
        }
        assert_eq!(dpoints_cos_angle(&q[0], &[1.0, 0.0, 0.0], &[2.0, 0.0, 0.0]).to_f64(), 1.0);
        assert_eq!(dpoints_cos_angle(&q[0], &[1.0, 0.0, 0.0], &[1.0, 1.0, 0.0]).to_f64(), 0.0);
        assert_eq!(dvector_cos_angle(&[0.0; 3], &[1.0, 0.0, 0.0]).to_f64(), 1.0);
    }

    #[test]
    fn points_of_a_cubic_are_fitted_by_that_cubic() {
        let q = [[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [3.0, 2.0, 0.0], [4.0, 0.0, 0.0]];
        let pts: Vec<DblPnt> = (0..=8).map(|i| bezier_value(3, &q, i as f64 / 8.0).unwrap()).collect();
        let mut u = chord_length_parameterize(&pts, 0, 8).unwrap();
        assert_eq!((u[0], u[8]), (0.0, 1.0));
        let t1 = compute_left_tangent(&pts, 0).unwrap();
        let t2 = compute_right_tangent(&pts, 8).unwrap();
        let mut bez = generate_bezier(&pts, 0, 8, &u, t1, t2).unwrap();
        for _ in 0..6 {
            u = reparameterize(&pts, 0, 8, &u, &bez).unwrap();
            bez = generate_bezier(&pts, 0, 8, &u, t1, t2).unwrap();
        }
        let (err, split) = compute_max_error(&pts, 0, 8, &bez, &u).unwrap();
        assert!(err < 1e-2, "{err} {bez:?}"); // a squared distance; the chord tangents are not the curve's
        assert!((1..8).contains(&split));
        let c = compute_center_tangent(&pts, 4).unwrap();
        assert!(c[0] < 0.0 && (c[0] * c[0] + c[1] * c[1] + c[2] * c[2] - 1.0).abs() < 1e-6);
        assert_eq!(v2_sub(v2_add([1.0, 2.0, 3.0], [1.0, 1.0, 1.0]), [2.0, 3.0, 4.0]), [0.0; 3]);
        assert_eq!(v2_scale([1.0, 2.0, 3.0], 2.0), [2.0, 4.0, 6.0]);
    }
}
