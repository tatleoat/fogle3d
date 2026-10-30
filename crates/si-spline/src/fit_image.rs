// SPDX-License-Identifier: MIT
//! Curve fitting on the one memory image (finding 0020, section 7; finding 0024): the
//! recursion of `fitCurve.c` (`fitCubic`, `SI_fitBezCurve`, `SI_fitCurve`,
//! `drawBezierCurve`) and, because the recursion's memory is theirs too, the image forms of
//! the helpers that allocate (`bezierValue`, `chordLengthParameterize`, `computeMaxError`,
//! `newtonRaphsonRootFind`, `reparameterize`, `generateBezier`) and of the three tangent
//! estimates.
//!
//! Points are `SI_DblPnt`, three doubles (0x18 bytes), at addresses in the image. The
//! arithmetic of the helpers is [`crate::fit`]'s (values in, values out, compared with the
//! original in the second pass); what is added here is where the operands lie and every
//! allocation call of the original, with its size and in its order, so that the heap a fit
//! leaves is the original's block for block.
//!
//! The result of a fit is built in the spline the static `SI_cBez` points to; `join` says
//! whether the next segment starts at a corner.

use si_mem::{Mem, Ptr32};
use si_types::geom::{Spline, SplineKey};
use si_x87::{trig, x, F80};

use crate::fit::{self, DblPnt};
use crate::spline_image::{spline_allocate, spline_delete_point, spline_key_allocate, spline_update_matrix, Outside, JOIN, SI_C_BEZ};

const TINY: f64 = 1.0e-4;
/// The size of an `SI_DblPnt`.
pub const PNT: u32 = 0x18;

fn d(v: F80) -> f64 {
    v.to_f64()
}

/// The point at `a`.
pub fn pt(m: &Mem, a: u32) -> DblPnt {
    [m.f64(a), m.f64(a.wrapping_add(8)), m.f64(a.wrapping_add(16))]
}

/// Stores a point at `a`.
pub fn put(m: &mut Mem, a: u32, p: DblPnt) {
    m.set_f64(a, p[0]);
    m.set_f64(a.wrapping_add(8), p[1]);
    m.set_f64(a.wrapping_add(16), p[2]);
}

/// Point `i` of the array at `base`.
fn at(base: u32, i: i32) -> u32 {
    base.wrapping_add((i as u32).wrapping_mul(PNT))
}

/// The point at `t` of the Bézier curve of the given degree over the points at `v`,
/// written to `ret` [soft 0x084397e0 bezierValue]: `SI_malloc((degree + 1) × 0x18)`, the
/// points copied into it, de Casteljau's triangle in place (every level rounded to double,
/// `1 − t` in the register), the block's first point taken, the block freed (when not
/// null), the point stored. A degree of −1 asks for no bytes, gets null and reads the
/// point at address 0.
pub fn bezier_value(m: &mut Mem, ret: u32, degree: i32, v: u32, t: f64) {
    let n = degree.wrapping_add(1);
    let block = m.si_malloc((n as u32).wrapping_mul(PNT));
    if degree >= 0 {
        for i in 0..n {
            m.copy(at(block, i), at(v, i), PNT);
        }
    }
    let mut i = 1;
    while i <= degree {
        let s = F80::ONE - t;
        for j in 0..=(degree - i) {
            let (p, q) = (pt(m, at(block, j)), pt(m, at(block, j + 1)));
            put(m, at(block, j), [d((s * p[0]) + (x(t) * q[0])), d((x(t) * q[1]) + (s * p[1])), d((x(t) * q[2]) + (s * p[2]))]);
        }
        i += 1;
    }
    let value = pt(m, block);
    if block != 0 {
        m.free(block);
    }
    put(m, ret, value);
}

/// The unit direction from point `end` to the next [soft 0x08439950 computeLeftTangent].
pub fn compute_left_tangent(m: &Mem, pts: u32, end: i32) -> DblPnt {
    fit::compute_left_tangent(&[pt(m, at(pts, end)), pt(m, at(pts, end.wrapping_add(1)))], 0).unwrap()
}

/// The unit direction from point `end` to the one before
/// [soft 0x084399f4 computeRightTangent].
pub fn compute_right_tangent(m: &Mem, pts: u32, end: i32) -> DblPnt {
    fit::compute_right_tangent(&[pt(m, at(pts, end.wrapping_sub(1))), pt(m, at(pts, end))], 1).unwrap()
}

/// The tangent at an interior point, pointing backwards
/// [soft 0x08439a98 computeCenterTangent].
pub fn compute_center_tangent(m: &Mem, pts: u32, center: i32) -> DblPnt {
    fit::compute_center_tangent(&[pt(m, at(pts, center.wrapping_sub(1))), pt(m, at(pts, center)), pt(m, at(pts, center.wrapping_add(1)))], 1).unwrap()
}

/// The parameters of points `first..=last` by chord length, in a new block
/// [soft 0x08439bd4 chordLengthParameterize]: `SI_malloc((last − first) × 8 + 8)`, the
/// first entry 0, each next one the distance from the point before plus the entry before
/// (stored as a double), then every entry but the first divided by the last. `last` one
/// below `first` asks for no bytes and writes the first entry through null.
pub fn chord_length_parameterize(m: &mut Mem, pts: u32, first: i32, last: i32) -> u32 {
    let u = m.si_malloc((last.wrapping_sub(first) as u32).wrapping_mul(8).wrapping_add(8));
    m.set_u32(u, 0);
    m.set_u32(u.wrapping_add(4), 0);
    let slot = |i: i32| u.wrapping_add((i.wrapping_sub(first) as u32).wrapping_mul(8));
    let mut i = first.wrapping_add(1);
    while i <= last {
        let (p, q) = (pt(m, at(pts, i)), pt(m, at(pts, i - 1)));
        let (dx, dy, dz) = (x(p[0]) - q[0], x(p[1]) - q[1], x(p[2]) - q[2]);
        let before = m.f64(slot(i - 1));
        m.set_f64(slot(i), d((((dx * dx) + (dy * dy)) + (dz * dz)).sqrt() + before));
        i += 1;
    }
    let mut i = first.wrapping_add(1);
    while i <= last {
        let total = m.f64(slot(last));
        let v = m.f64(slot(i));
        m.set_f64(slot(i), d(x(v) / total));
        i += 1;
    }
    u
}

/// The largest squared distance of an interior point from the curve at its parameter
/// [soft 0x08439c90 computeMaxError]; the point it is at goes to `split`, which starts as
/// `(last − first + 1) / 2` and is taken by every point whose distance is not below the
/// largest so far. One [`bezier_value`] (a block allocated and freed) per interior point.
/// The result is the double, loaded.
pub fn compute_max_error(m: &mut Mem, pts: u32, first: i32, last: i32, bez: u32, u: u32, split: u32) -> F80 {
    m.set_i32(split, last.wrapping_sub(first).wrapping_add(1) / 2);
    let mut max = 0.0f64;
    let local = m.local(PNT);
    let mut i = first.wrapping_add(1);
    while i < last {
        let t = m.f64(u.wrapping_add((i.wrapping_sub(first) as u32).wrapping_mul(8)));
        bezier_value(m, local, 3, bez, t);
        let v = fit::v2_sub(pt(m, local), pt(m, at(pts, i)));
        let dist = ((x(v[0]) * v[0]) + (x(v[1]) * v[1])) + (x(v[2]) * v[2]);
        if !(dist.lt(x(max)) || dist.unordered(x(max))) {
            max = d(dist);
            m.set_i32(split, i);
        }
        i += 1;
    }
    m.release(PNT);
    x(max)
}

/// One Newton-Raphson step for the parameter at which the curve at `q` is nearest to `p`
/// [soft 0x084395a8 newtonRaphsonRootFind]: the curve's value first, then the control
/// points of its first and second derivative (locals), then their values: three
/// [`bezier_value`] calls, of degree 3, 2 and 1. The arithmetic is
/// [`fit::newton_raphson_root_find`]'s. Left in the register.
pub fn newton_raphson_root_find(m: &mut Mem, q: u32, p: DblPnt, u: f64) -> F80 {
    // locals: Q(u), Q1[3], Q2[2], Q1(u), Q2(u)
    let frame = m.local(8 * PNT);
    let (qu_at, q1_at, q2_at, q1u_at, q2u_at) = (frame, frame + PNT, frame + 4 * PNT, frame + 6 * PNT, frame + 7 * PNT);
    bezier_value(m, qu_at, 3, q, u);
    for i in 0..3 {
        let (a, b) = (pt(m, at(q, i)), pt(m, at(q, i + 1)));
        put(m, at(q1_at, i), [d((x(b[0]) - a[0]) * 3.0f64), d((x(b[1]) - a[1]) * 3.0f64), d((x(b[2]) - a[2]) * 3.0f64)]);
    }
    for i in 0..2 {
        let (a, b) = (pt(m, at(q1_at, i)), pt(m, at(q1_at, i + 1)));
        let twice = |k: usize| {
            let diff = x(b[k]) - a[k];
            d(diff + diff)
        };
        put(m, at(q2_at, i), [twice(0), twice(1), twice(2)]);
    }
    bezier_value(m, q1u_at, 2, q1_at, u);
    bezier_value(m, q2u_at, 1, q2_at, u);
    let (qu, q1u, q2u) = (pt(m, qu_at), pt(m, q1u_at), pt(m, q2u_at));
    m.release(8 * PNT);
    let (t1, t2, t3) = (x(qu[0]) - p[0], x(qu[1]) - p[1], x(qu[2]) - p[2]);
    let num = (t3 * q1u[2]) + ((t2 * q1u[1]) + (t1 * q1u[0]));
    let den = (t3 * q2u[2]) + (((t1 * q2u[0]) + ((x(q1u[2]) * q1u[2]) + ((x(q1u[1]) * q1u[1]) + (x(q1u[0]) * q1u[0])))) + (t2 * q2u[1]));
    x(u) - (num / den)
}

/// A better parameter for each of the points `first..=last`, in a new block
/// [soft 0x08439518 reparameterize]: `SI_malloc((last − first) × 8 + 8)` and one
/// [`newton_raphson_root_find`] step a point, stored as a double.
pub fn reparameterize(m: &mut Mem, pts: u32, first: i32, last: i32, u: u32, bez: u32) -> u32 {
    let out = m.si_malloc((last.wrapping_sub(first) as u32).wrapping_mul(8).wrapping_add(8));
    let mut i = first;
    while i <= last {
        let slot = (i.wrapping_sub(first) as u32).wrapping_mul(8);
        let v = newton_raphson_root_find(m, bez, pt(m, at(pts, i)), m.f64(u.wrapping_add(slot)));
        m.set_f64(out.wrapping_add(slot), d(v));
        i += 1;
    }
    out
}

/// The least-squares cubic through points `first..=last` with given end directions, in a
/// new block of four points [soft 0x08438bf4 generateBezier]: `SI_malloc(0x60)` for the
/// curve first, then `SI_calloc(last − first + 1, 0x30)` for the table `A` (two vectors a
/// point), which is freed at the end when it is not null. The arithmetic is
/// [`fit::generate_bezier_with_a`]'s (finding 0020, section 7).
pub fn generate_bezier(m: &mut Mem, pts: u32, first: i32, last: i32, u: u32, t_hat1: DblPnt, t_hat2: DblPnt) -> u32 {
    let bez = m.si_malloc(4 * PNT);
    let n = last.wrapping_sub(first).wrapping_add(1);
    let a = m.si_calloc(n as u32, 2 * PNT);
    if n > 0 && a == 0 {
        // no room for the table: the original's first store into it goes through null
        m.set_f64(0, 0.0);
        return bez;
    }
    // the points and parameters the routine reads: first..=last, and both ends whatever
    // their order
    let (lo, hi) = (first.min(last), first.max(last));
    let points: Vec<DblPnt> = (lo..=hi).map(|i| pt(m, at(pts, i))).collect();
    let params: Vec<f64> = (0..n.max(0)).map(|i| m.f64(u.wrapping_add((i as u32).wrapping_mul(8)))).collect();
    let (table, curve) = fit::generate_bezier_with_a(&points, first - lo, last - lo, &params, t_hat1, t_hat2).unwrap();
    for (i, pair) in table.iter().enumerate() {
        put(m, a.wrapping_add(i as u32 * 2 * PNT), pair[0]);
        put(m, a.wrapping_add(i as u32 * 2 * PNT + PNT), pair[1]);
    }
    for (i, p) in curve.iter().enumerate() {
        put(m, at(bez, i as i32), *p);
    }
    if a != 0 {
        m.free(a);
    }
    bez
}

fn narrow(v: f64) -> f32 {
    x(v).to_f32()
}

/// `2a − b` of two doubles, rounded to float: the mirrored outer handle of an end key.
fn mirror(a: f64, b: f64) -> f32 {
    ((x(a) + a) - b).to_f32()
}

/// Appends one fitted segment (four points at `bez`) to the spline `SI_cBez` points to
/// [soft 0x08439e4c drawBezierCurve]. It draws nothing.
///
/// - The spline has no key: `SI_splineKeyAllocate(2)`; key 0 gets the first point as
///   position, the second as `next` handle and `2 × first − second` as `prev` handle,
///   constraint 1, segment type 1; key 1 gets the fourth point, the third as `prev` handle,
///   `2 × fourth − third` as `next` handle, constraint 4, segment type 1. The count is 2.
/// - Otherwise the last key's `next` handle becomes the second point and its constraint 1
///   when `join` is set (a corner), else 4; `join` is cleared; when the count is a multiple
///   of 16 the block grows by 16 records (`SI_realloc`); the new key is written as key 1
///   above, with flag 0 (**not** its float at +0x24: it holds what the block held), and
///   the count goes up by one.
///
/// Every coordinate is a double rounded to float.
pub fn draw_bezier_curve(m: &mut Mem, bez: u32) {
    let s: Ptr32<Spline> = m[SI_C_BEZ];
    let b = |m: &Mem, i: u32, k: u32| m.f64(bez + i * PNT + 8 * k);
    let end_key = |m: &mut Mem, k: Ptr32<SplineKey>| {
        let a = k.addr();
        for c in 0..3 {
            let v = mirror(b(m, 3, c), b(m, 2, c));
            m.set_f32(a + 0x18 + 4 * c, v);
        }
        for c in 0..3 {
            let v = narrow(b(m, 3, c));
            m.set_f32(a + 0x0c + 4 * c, v);
        }
        for c in 0..3 {
            let v = narrow(b(m, 2, c));
            m.set_f32(a + 4 * c, v);
        }
    };
    if m[s].nb_keys == 0 {
        let keys = spline_key_allocate(m, 2);
        m[s].keys = keys;
        let a = keys.addr();
        for c in 0..3 {
            let v = mirror(b(m, 0, c), b(m, 1, c));
            m.set_f32(a + 4 * c, v);
        }
        for c in 0..3 {
            let v = narrow(b(m, 0, c));
            m.set_f32(a + 0x0c + 4 * c, v);
        }
        for c in 0..3 {
            let v = narrow(b(m, 1, c));
            m.set_f32(a + 0x18 + 4 * c, v);
        }
        m[keys].constraint = 1;
        m[keys].segment = 1;
        end_key(m, keys.add(1));
        m[keys.add(1)].constraint = 4;
        m[keys.add(1)].segment = 1;
        m[s].nb_keys = 2;
    } else {
        // the last key: one record below the end of the keys in use
        let last = |m: &Mem| m[s].keys.add(m[s].nb_keys as u16 as i32 - 1);
        for c in 0..3 {
            let v = narrow(b(m, 1, c));
            let k = last(m).addr();
            m.set_f32(k + 0x18 + 4 * c, v);
        }
        let k = last(m);
        m[k].constraint = if m[JOIN] != 0 { 1 } else { 4 };
        m[JOIN] = 0;
        let n = m[s].nb_keys as u16 as u32;
        if n & 0xf == 0 {
            let block = m.si_realloc(m[s].keys.addr(), (n + 16) * SplineKey::SIZE as u32);
            m[s].keys = Ptr32::new(block);
        }
        let k = m[s].keys.add(m[s].nb_keys as u16 as i32);
        end_key(m, k);
        m[k].constraint = 4;
        m[k].segment = 1;
        m[k].flag = 0;
        m[s].nb_keys = m[s].nb_keys.wrapping_add(1);
    }
}

/// Fits points `first..=last` of the array at `pts` with Bézier segments, each appended
/// through [`draw_bezier_curve`] [soft 0x08438758 fitCubic]. `error` is compared with
/// squared distances.
///
/// - `last − first` is 1: one segment whose handles lie a third of the chord along each
///   tangent: `dist = √(…) / 3.0` stays in the register (80 bits); a tangent whose float
///   length is above 0.0001 is scaled by `dist / length`, any other is added as it is. The
///   four points are built in an `SI_malloc(0x60)` block, which is freed after the append.
/// - Otherwise: [`chord_length_parameterize`], [`generate_bezier`], [`compute_max_error`].
///   An error below `error`: the segment is appended. Else, when it is below `error²`: up
///   to 16 rounds of [`reparameterize`], [`generate_bezier`] and [`compute_max_error`],
///   appended as soon as the error is below `error`. **The curve of the round before is
///   not freed** (each round's `generateBezier` block stays allocated; only the last one
///   is released). Else, or after 16 rounds: the points are split at the worst one, with
///   [`compute_center_tangent`] there as the right tangent of the first half and its
///   negative as the left tangent of the second.
/// - A span of no points or of one (`last ≤ first`) takes the second road too.
pub fn fit_cubic(m: &mut Mem, pts: u32, first: i32, last: i32, t_hat1: DblPnt, t_hat2: DblPnt, error: f64) {
    // The routine's frame (0x7c bytes of locals, the saved registers, the 0x50 bytes of a
    // recursive call's arguments): taken from the image's stack so that a recursion that
    // does not end (a span of one point with an error nothing is below) ends where the
    // original's does, at the end of the stack. The original dies there; this returns.
    const FRAME: u32 = 0xe0;
    let frame = m.local(FRAME);
    if frame == 0 {
        return;
    }
    fit_cubic_in(m, frame, pts, first, last, t_hat1, t_hat2, error);
    m.release(FRAME);
}

#[allow(clippy::too_many_arguments)]
fn fit_cubic_in(m: &mut Mem, split: u32, pts: u32, first: i32, last: i32, t_hat1: DblPnt, t_hat2: DblPnt, error: f64) {
    let iteration_error = d(x(error) * error);
    if last.wrapping_sub(first) == 1 {
        let (pf, pl) = (pt(m, at(pts, first)), pt(m, at(pts, last)));
        let (dx, dy, dz) = (x(pl[0]) - pf[0], x(pl[1]) - pf[1], x(pl[2]) - pf[2]);
        let dist = (((dx * dx) + (dy * dy)) + (dz * dz)).sqrt() / 3.0f64;
        let bez = m.si_malloc(4 * PNT);
        m.copy(bez, at(pts, first), PNT);
        m.copy(bez.wrapping_add(3 * PNT), at(pts, last), PNT);
        let scaled = |t: DblPnt| -> DblPnt {
            let len = (((x(t[0]) * t[0]) + (x(t[1]) * t[1])) + (x(t[2]) * t[2])).sqrt().to_f32();
            if x(len).abs().gt(x(TINY)) {
                let s = dist / len;
                [d(x(t[0]) * s), d(x(t[1]) * s), d(s * t[2])]
            } else {
                t
            }
        };
        let (t1, t2) = (scaled(t_hat1), scaled(t_hat2));
        let p0 = pt(m, bez);
        put(m, bez.wrapping_add(PNT), fit::v2_add(p0, t1));
        let p3 = pt(m, bez.wrapping_add(3 * PNT));
        put(m, bez.wrapping_add(2 * PNT), fit::v2_add(p3, t2));
        draw_bezier_curve(m, bez);
        if bez != 0 {
            m.free(bez);
        }
        return;
    }
    let mut u = chord_length_parameterize(m, pts, first, last);
    let mut bez = generate_bezier(m, pts, first, last, u, t_hat1, t_hat2);
    let max = compute_max_error(m, pts, first, last, bez, u, split);
    if max.lt(x(error)) {
        draw_bezier_curve(m, bez);
        if bez != 0 {
            m.free(bez);
        }
        if u != 0 {
            m.free(u);
        }
        return;
    }
    if max.lt(x(iteration_error)) {
        for _ in 0..16 {
            let u_prime = reparameterize(m, pts, first, last, u, bez);
            bez = generate_bezier(m, pts, first, last, u_prime, t_hat1, t_hat2);
            let max = compute_max_error(m, pts, first, last, bez, u_prime, split);
            if max.lt(x(error)) {
                draw_bezier_curve(m, bez);
                if bez != 0 {
                    m.free(bez);
                }
                if u_prime != 0 {
                    m.free(u_prime);
                }
                if u != 0 {
                    m.free(u);
                }
                return;
            }
            if u != 0 {
                m.free(u);
            }
            u = u_prime;
        }
    }
    let at_split = m.i32(split);
    let mut center = compute_center_tangent(m, pts, at_split);
    fit_cubic(m, pts, first, at_split, t_hat1, center, error);
    center = [-center[0], -center[1], -center[2]];
    fit_cubic(m, pts, at_split, last, center, t_hat2, error);
    if bez != 0 {
        m.free(bez);
    }
    if u != 0 {
        m.free(u);
    }
}

/// Fits the `n` points at `pts` into the spline `SI_cBez` points to
/// [soft 0x084385b0 SI_fitBezCurve]: the left tangent at the first point, the right
/// tangent at the last, and [`fit_cubic`] over `0..=n − 1`. For exactly two points the two
/// tangent estimates are written out in the routine itself (the same operations) and the
/// last index is 1. `error` goes to [`fit_cubic`] as it is: a squared distance. What
/// `fitFcvKeys` (function curves) calls as well.
pub fn fit_bez_curve(m: &mut Mem, pts: u32, n: i32, error: f64) {
    let (t1, t2, last) = if n == 2 {
        let both = [pt(m, pts), pt(m, pts.wrapping_add(PNT))];
        (fit::compute_left_tangent(&both, 0).unwrap(), fit::compute_right_tangent(&both, 1).unwrap(), 1)
    } else {
        let last = n.wrapping_sub(1);
        (compute_left_tangent(m, pts, 0), compute_right_tangent(m, pts, last), last)
    };
    fit_cubic(m, pts, 0, last, t1, t2, error);
}

fn key_pos(m: &Mem, a: u32) -> DblPnt {
    [m.f32(a) as f64, m.f32(a.wrapping_add(4)) as f64, m.f32(a.wrapping_add(8)) as f64]
}

/// A new Bézier spline fitted through the key positions of a spline
/// [soft 0x084381b0 SI_fitCurve]; `angle` (radians) decides what is a corner, `tolerance`
/// is a distance.
///
/// 1. `tolerance²` and `c = cos(angle)` (the C library's) are kept as doubles; `join` is
///    cleared; the result is a new spline ([`spline_allocate`]; null ends the routine) of
///    type 1 with its matrix, and `SI_cBez` points to it.
/// 2. A closed source: with p0 its last key and p1 its first, the first key `i` at which
///    `dpointsCosAngle(p0, p1, next key) < c` is the start corner (0 when there is none).
/// 3. The positions are copied as doubles into `SI_calloc(keys + closed, 0x18)`, starting
///    at the start corner and wrapping, so a closed source's start is repeated at the end.
/// 4. With p0, p1 the first two points and `first` = 0: for each `i` from 1: at the last
///    point the run `first..=i` goes to [`fit_bez_curve`] (with `tolerance²`) and `join`
///    is set; at any other point where the cosine of the turn is below `c` the same, and
///    `first = i`. After a corner at `i` both p0 and p1 become point `i + 1`, so **the
///    point after a corner is never a corner** (its turn is measured from a zero vector,
///    whose cosine is 1).
/// 5. A closed source: the first key's `prev` handle is taken from the last key (the
///    duplicate of the start), the last key is deleted ([`spline_delete_point`], which
///    touches the spline) and the result is marked closed.
/// 6. The array of points is freed.
pub fn fit_curve(m: &mut Mem, o: &mut dyn Outside, spline: Ptr32<Spline>, angle: f64, tolerance: f64) -> Ptr32<Spline> {
    let tol2 = d(x(tolerance) * tolerance);
    let c = d(trig::cos(angle));
    m[JOIN] = 0;
    let bez = spline_allocate(m);
    m[SI_C_BEZ] = bez;
    if bez.is_null() {
        return bez;
    }
    m[bez].kind = 1;
    spline_update_matrix(m, bez);
    let mut corner: i16 = 0;
    // p0 and p1 are locals the original leaves unset until it needs them
    let (mut p0, mut p1): (DblPnt, DblPnt);
    if m[spline].closed != 0 {
        let keys = m[spline].keys.addr();
        let n = m[spline].nb_keys as u16 as u32;
        p0 = key_pos(m, keys.wrapping_add(n * 0x30).wrapping_sub(0x24));
        p1 = key_pos(m, keys.wrapping_add(0x0c));
        let mut i: i16 = 0;
        let mut n = m[spline].nb_keys as u16 as i32;
        if n != 0 {
            loop {
                let keys = m[spline].keys.addr();
                let next = if i as i32 == n - 1 { keys } else { keys.wrapping_add((i as i32 as u32).wrapping_mul(0x30)).wrapping_add(0x30) };
                let p2 = key_pos(m, next.wrapping_add(0x0c));
                if fit::dpoints_cos_angle(&p0, &p1, &p2).lt(x(c)) {
                    corner = i;
                    break;
                }
                p0 = p1;
                p1 = p2;
                i = i.wrapping_add(1);
                n = m[spline].nb_keys as u16 as i32;
                if i as i32 >= n {
                    break;
                }
            }
        }
    }
    let total = |m: &Mem| m[spline].nb_keys as u16 as i32 + m[spline].closed as i32;
    let pts = m.si_calloc(total(m) as u32, PNT);
    let mut from = corner;
    let mut i: i16 = 0;
    if total(m) > 0 {
        loop {
            if from as i32 == m[spline].nb_keys as u16 as i32 {
                from = 0;
            }
            let key = m[spline].keys.addr().wrapping_add((from as i32 as u32).wrapping_mul(0x30));
            let p = key_pos(m, key.wrapping_add(0x0c));
            put(m, at(pts, i as i32), p);
            from = from.wrapping_add(1);
            i = i.wrapping_add(1);
            if i as i32 >= total(m) {
                break;
            }
        }
    }
    p0 = pt(m, pts);
    let mut first: i16 = 0;
    p1 = pt(m, pts.wrapping_add(PNT));
    let mut i: i16 = 1;
    if total(m) > 1 {
        loop {
            let here = i as i32;
            if here == total(m) - 1 {
                fit_bez_curve(m, at(pts, first as i32), here - first as i32 + 1, tol2);
                m[JOIN] = 1;
                first = m[spline].nb_keys;
            } else {
                let p2 = pt(m, at(pts, here + 1));
                if fit::dpoints_cos_angle(&p0, &p1, &p2).lt(x(c)) {
                    fit_bez_curve(m, at(pts, first as i32), here - first as i32 + 1, tol2);
                    m[JOIN] = 1;
                    first = i;
                    // p0 and p1 become points i and i + 1 here, and just below p0 is
                    // replaced by p1: both end as point i + 1
                    p1 = pt(m, at(pts, here + 1));
                }
                p0 = p1;
                p1 = p2;
            }
            i = i.wrapping_add(1);
            if i as i32 >= total(m) {
                break;
            }
        }
    }
    if m[spline].closed != 0 {
        let keys = m[bez].keys.addr();
        let n = m[bez].nb_keys as u16 as u32;
        let end = keys.wrapping_add(n * 0x30);
        for w in 0..3 {
            let v = m.u32(end.wrapping_sub(0x30).wrapping_add(4 * w));
            m.set_u32(keys.wrapping_add(4 * w), v);
        }
        let n = m[bez].nb_keys as u16 as i32;
        spline_delete_point(m, o, bez, n - 1);
        m[bez].closed = 1;
    }
    if pts != 0 {
        m.free(pts);
    }
    bez
}
