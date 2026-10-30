// SPDX-License-Identifier: MIT
//! The contour fit of `fitCurve.c` on the one memory image (finding 0020, section 17;
//! finding 0024): the fit of a traced picture contour with Bézier segments. Ported here
//! are its parameters (`setFitParam`), the point it places on a line (`newPoint`) and the
//! routine that hands runs of contour points to the fit (`generateCurve`).
//!
//! The four parameters are statics of the file: `fitError`, `lookAhead`, `maxDistLine`,
//! `minDistLine` ([`crate::spline_image`]'s globals, with the binary's initial values 0.05,
//! 7, 1.5 and 0.2).

use si_mem::Mem;
use si_x87::x;

use crate::fit_image::{fit_bez_curve, PNT};
use crate::spline_image::{Callee, Outside, FIT_ERROR, JOIN, LOOK_AHEAD, MAX_DIST_LINE, MIN_DIST_LINE};

/// Sets the four parameters of the contour fit [soft 0x08436ef0 setFitParam]: the largest
/// and the smallest distance from a line, the fit error (doubles) and the look-ahead. No
/// caller in the binary but `dialogTestFit`.
pub fn set_fit_param(m: &mut Mem, look_ahead: i32, max_dist_line: f64, min_dist_line: f64, fit_error: f64) {
    m.set_f64(MAX_DIST_LINE.addr(), max_dist_line);
    m.set_f64(MIN_DIST_LINE.addr(), min_dist_line);
    m.set_f64(FIT_ERROR.addr(), fit_error);
    m[LOOK_AHEAD] = look_ahead;
}

/// The point on the line through `a` and `b` at the fraction `d` of the way in x, stored
/// at `ret` [soft 0x084380f8 newPoint]. With `s = (b.y − a.y) / (b.x − a.x)` (0 when
/// `|b.x − a.x|` is not above 0.001): when `0.01 < |s| < 100` the result is
/// `x = float(a.x + (b.x − a.x) × d)`, `y = float((b.y − s × b.x) + s × x)` with that
/// rounded x, and `b.z`; for any other slope (flat, steep, not a number) it is `b`
/// unchanged. Everything else stays in the register.
pub fn new_point(m: &mut Mem, ret: u32, a: [f32; 3], b: [f32; 3], d: f64) {
    let dx = x(b[0]) - a[0];
    let s = if dx.abs().gt(x(0.001f64)) { (x(b[1]) - a[1]) / dx } else { si_x87::F80::ZERO };
    let mut out = b;
    if s.abs().gt(x(0.01f64)) && s.abs().lt(x(100.0f64)) {
        let c = x(b[1]) - (s * b[0]);
        let nx = (x(a[0]) + ((x(b[0]) - a[0]) * d)).to_f32();
        out[0] = nx;
        out[1] = (c + (s * nx)).to_f32();
    }
    for (k, v) in out.iter().enumerate() {
        // b's coordinates go through as words; the computed ones are floats
        m.set_u32(ret + 4 * k as u32, v.to_bits());
    }
}

/// Fits the runs of a contour's points between the places where their flag word changes
/// [soft 0x08437554 generateCurve]. `flags` has one 16-bit word a point, `n` is the last
/// index. With `cur` the flag of point 0 and `start` 0, for `i` from 0 to `n`:
///
/// - at `i == n`: `SI_fitBezCurve(points + start, i − start, fitError)`, `join = 1`, and
///   the routine ends;
/// - where `flags[i]` differs from `cur`: with `k` 1 when `cur` was 0, else 0,
///   `SI_fitBezCurve(points + start, i − start + k, fitError)`, `join = 1`,
///   `start = i − 1 + k`, and `cur` becomes the new flag.
///
/// A negative `n` does nothing.
pub fn generate_curve(m: &mut Mem, points: u32, flags: u32, n: i32) {
    let mut start: i32 = 0;
    let mut i: i32 = 0;
    let mut cur = m.u16(flags);
    if i > n {
        return;
    }
    loop {
        if i == n {
            let error = m.f64(FIT_ERROR.addr());
            fit_bez_curve(m, points.wrapping_add((start as u32).wrapping_mul(PNT)), i.wrapping_sub(start), error);
            m[JOIN] = 1;
            return;
        }
        let flag = m.u16(flags.wrapping_add((i as u32) << 1));
        if flag != cur {
            let error = m.f64(FIT_ERROR.addr());
            let k = (cur == 0) as i32;
            cur = flag;
            fit_bez_curve(m, points.wrapping_add((start as u32).wrapping_mul(PNT)), i.wrapping_sub(start).wrapping_add(k), error);
            m[JOIN] = 1;
            start = (i - 1).wrapping_add(k);
        }
        i += 1;
        if i > n {
            return;
        }
    }
}

/// `SI_openDialog` (0x083859a0): the dialog of a number; its address.
pub const SI_OPEN_DIALOG: Callee = Callee { name: "SI_openDialog", addr: 0x0838_59a0 };
/// `SI_showDialog` (0x083859f4).
pub const SI_SHOW_DIALOG: Callee = Callee { name: "SI_showDialog", addr: 0x0838_59f4 };
/// `SI_dialogClose` (0x08385b28).
pub const SI_DIALOG_CLOSE: Callee = Callee { name: "SI_dialogClose", addr: 0x0838_5b28 };
/// `SI_getDialogEvent` (0x08385e24): the next event of a dialog (1: accepted, 2: cancelled).
pub const SI_GET_DIALOG_EVENT: Callee = Callee { name: "SI_getDialogEvent", addr: 0x0838_5e24 };
/// `SI_getTexedFloat` (0x0838c138): the number in a text field, in the x87 register.
pub const SI_GET_TEXED_FLOAT: Callee = Callee { name: "SI_getTexedFloat", addr: 0x0838_c138 };
/// `SI_getTexedLong` (0x0838c204): the whole number in a text field.
pub const SI_GET_TEXED_LONG: Callee = Callee { name: "SI_getTexedLong", addr: 0x0838_c204 };
/// `SI_statusDisplayMessage` (0x083cca94).
pub const SI_STATUS_DISPLAY_MESSAGE: Callee = Callee { name: "SI_statusDisplayMessage", addr: 0x083c_ca94 };

/// The dialog's number, and the text shown when it is cancelled (`Process aborted!`,
/// in `.data`).
pub const FIT_DIALOG: u32 = 0x613;
pub const MSG_ABORTED: u32 = 0x086d_3720;

/// Asks for the contour fit's parameters [soft 0x08436f1c dialogTestFit]. No caller in
/// the binary. `SI_openDialog(0x613)`, `SI_showDialog`; `SI_getDialogEvent` until it
/// answers 1 or 2 (as a 16-bit number). On 2: `SI_dialogClose` and
/// `SI_statusDisplayMessage("Process aborted!", 0)`. On 1: the fields 5, 4 and 6
/// (`SI_getTexedFloat`, each rounded to double) and 3 (`SI_getTexedLong`), then
/// [`set_fit_param`] with field 3 as the look-ahead, 4 as the largest and 5 as the
/// smallest distance from a line and 6 as the fit error, and `SI_dialogClose`.
pub fn dialog_test_fit(m: &mut Mem, o: &mut dyn Outside) {
    let dialog = o.call(m, &SI_OPEN_DIALOG, &[FIT_DIALOG]);
    o.call(m, &SI_SHOW_DIALOG, &[dialog]);
    let event = loop {
        let e = o.call(m, &SI_GET_DIALOG_EVENT, &[dialog]);
        if (e.wrapping_sub(1) as u16) <= 1 || m.fault().is_some() {
            break e;
        }
    };
    if event as u16 == 2 {
        o.call(m, &SI_DIALOG_CLOSE, &[dialog]);
        o.call(m, &SI_STATUS_DISPLAY_MESSAGE, &[MSG_ABORTED, 0]);
        return;
    }
    let smallest = o.call_float(m, &SI_GET_TEXED_FLOAT, &[dialog, 5]).to_f64();
    let largest = o.call_float(m, &SI_GET_TEXED_FLOAT, &[dialog, 4]).to_f64();
    let error = o.call_float(m, &SI_GET_TEXED_FLOAT, &[dialog, 6]).to_f64();
    let look_ahead = o.call(m, &SI_GET_TEXED_LONG, &[dialog, 3]);
    set_fit_param(m, look_ahead as i32, largest, smallest, error);
    o.call(m, &SI_DIALOG_CLOSE, &[dialog]);
}

/// Offsets in `my_contour`, the traced outline the contour fit works on, as far as
/// `SI_findDiscontinuity` reads it (its size and any further field are not established):
/// the number of points, and the address of the points (three floats each).
pub const CONTOUR_COUNT: u32 = 0;
pub const CONTOUR_POINTS: u32 = 4;

/// Marks the corners of a closed contour [soft 0x08437600 SI_findDiscontinuity]: `flags`
/// gets one 16-bit word a point, 1 at a corner.
///
/// With `n` the contour's count, `last = n − 1` and `la` the static `lookAhead`: the
/// points go as doubles into `SI_malloc((3 n + 3) × 8)`; `last + 5` flags are cleared.
/// `last` not above `2 la`: the flags 0 and `last − 1` are set, and that is all.
/// Otherwise the turn at point `i` is `t(i) = |dpointsCosAngle(p[(last + i − la) mod
/// last], p[i], p[(i + la) mod last])|` (the indices but the middle one wrap by `last`,
/// not by the count). Walking `i` from 0: where `|t(i) − previous| > 0.2` the place of
/// the smallest turn among the next `la` points is looked for (`j` from `i + 1` while
/// `j < i + la + 1` and `j ≤ last` or `j mod last ≤ first`; a turn above the smallest so
/// far ends the search); that place is a corner: the first one found is remembered as
/// `first`, every later one sets the flag of the corner before it and its own; the walk
/// goes on `span + 1` points behind it (`span` the distance searched, `la` when nothing
/// smaller was found). At the end the last corner and `first` are flagged when the walk
/// found any (`first + last − start + 1 > 1`). The block of doubles is freed.
///
/// The middle index is not wrapped: beyond the last point the routine reads past its
/// block.
pub fn find_discontinuity(m: &mut Mem, contour: u32, flags: u32) {
    use crate::patch_image::idiv;
    let n = m.i32(contour + CONTOUR_COUNT);
    let last = n.wrapping_sub(1);
    let pts = m.si_malloc((n.wrapping_mul(3).wrapping_add(3) as u32) << 3);
    if last >= 0 {
        let mut off = 0u32;
        let mut left = n;
        loop {
            for c in 0..3u32 {
                let v = m.f32(m.u32(contour + CONTOUR_POINTS).wrapping_add(off).wrapping_add(4 * c));
                m.set_f64(pts.wrapping_add(off.wrapping_mul(2)).wrapping_add(8 * c), x(v).to_f64());
            }
            off = off.wrapping_add(0xc);
            left -= 1;
            if left == 0 || m.fault().is_some() {
                break;
            }
        }
    }
    let mut e = 0;
    while e < last.wrapping_add(5) {
        m.set_u16(flags.wrapping_add((e as u32) << 1), 0);
        e += 1;
    }
    let la = m[LOOK_AHEAD];
    let flag = |m: &mut Mem, i: i32| m.set_u16(flags.wrapping_add((i as u32) << 1), 1);
    if last <= la.wrapping_add(la) {
        m.set_u16(flags, 1);
        m.set_u16(flags.wrapping_add((last as u32) << 1).wrapping_sub(2), 1);
    } else {
        let md = |m: &Mem, v: i32| idiv(m, v, last).1;
        let at = |i: i32| pts.wrapping_add((i as u32).wrapping_mul(PNT));
        let turn = |m: &Mem, a: i32, b: i32, c: i32| crate::fit::dpoints_cos_angle(&crate::fit_image::pt(m, at(a)), &crate::fit_image::pt(m, at(b)), &crate::fit_image::pt(m, at(c))).abs();
        let around = |m: &Mem, i: i32| turn(m, md(m, last.wrapping_add(i).wrapping_sub(la)), i, md(m, i.wrapping_add(la)));
        let mut start = -1i32;
        let mut first = 0i32;
        let mut prev = turn(m, md(m, last - 1), la - 1, md(m, la.wrapping_add(la) - 1));
        let mut i = 0i32;
        while i < last && m.fault().is_none() {
            let cur = around(m, i);
            if (cur - prev).abs().gt(x(0.2f64)) {
                let mut best = cur;
                let (mut best_i, mut span) = (i, la);
                let mut j = i + 1;
                let limit = la.wrapping_add(i).wrapping_add(1);
                while j < limit && (j <= last || md(m, j) <= first) && m.fault().is_none() {
                    let next = around(m, j);
                    if best.lt(next) || best.unordered(next) {
                        break;
                    }
                    best = next;
                    best_i = j;
                    span = j - i;
                    j += 1;
                }
                i = best_i;
                if start != -1 {
                    let (a, b) = (md(m, start), md(m, i));
                    flag(m, a);
                    flag(m, b);
                } else {
                    first = i;
                }
                start = best_i;
                i = span.wrapping_add(best_i).wrapping_add(1);
                prev = around(m, i);
            } else {
                prev = cur;
            }
            i += 1;
        }
        if start <= last && first.wrapping_add(last).wrapping_sub(start).wrapping_add(1) > 1 {
            if start < 0 {
                start = 0;
            }
            let (a, b) = (md(m, start), md(m, first));
            flag(m, a);
            flag(m, b);
        }
    }
    if pts != 0 {
        m.free(pts);
    }
}

/// The double 2500.0 (0x0865f770): a chord whose squared length is above it is straight
/// when no point lies `minDistLine` from it.
const LONG_CHORD: f64 = 2500.0;

/// Copies one `SI_DblPnt` (6 words, `rep movs`).
fn copy_pnt(m: &mut Mem, to: u32, from: u32) {
    for w in 0..6u32 {
        let v = m.u32(from + 4 * w);
        m.set_u32(to + 4 * w, v);
    }
}

/// The contour's run of points cut down to the points that matter
/// [soft 0x084370c0 simplifyContour] (`points`, `n` of them, as `SI_DblPnt`s): the
/// recursive subdivision of a polyline by its point farthest from the chord.
///
/// With `d = p[n−1] − p[0]` (the x and y differences stored as doubles, z kept in the
/// register), `len2 = float64(float64(float64(dx²) + dy²) + dz²)` from the unrounded
/// differences, and for each interior point `v = p[i] − p[0]` (in the register) the
/// distance `|(d × v) × d| / len2` (the products and sums in the order of the port, the
/// square root of the 80-bit sum): the first point with the largest distance, compared
/// as a double (strictly above the best so far, which starts at 0).
///
/// - Above `maxDistLine` (the double static, 1.5): both halves `0..=split` and
///   `split..n` again (each with its own three results in this frame), then
///   `*n_out = n_a + n_b − 1`, `SI_malloc(24 × that)` for the points and
///   `SI_malloc(2 × that)` for the flags; the first half's points and flags, the last
///   flag set to 1 when the second half's first flag is not 0, then the second half from
///   its second point; the four blocks freed (points of a, of b, flags of b, of a).
/// - Else two points, the ends (`SI_malloc(0x30)`, `*n_out = 2`, `SI_malloc(4)` for the
///   flags): both flags 1 (a straight piece) when the largest distance is below
///   `minDistLine` (0.2) and the chord is long (`len2` above 2500) or the run has more
///   than 14 points; else 0.
///
/// A run of no point reads the point before the array, as the original does.
pub fn simplify_contour(m: &mut Mem, points: u32, n: i32, out_at: u32, flags_at: u32, n_at: u32) {

    let p = |i: i32| points.wrapping_add((i as u32).wrapping_mul(PNT));
    let last = p(n - 1);
    let dx = x(m.f64(last)) - m.f64(points);
    let dy = x(m.f64(last + 8)) - m.f64(points + 8);
    let dz = x(m.f64(last + 16)) - m.f64(points + 16);
    let l1 = (dx * dx).to_f64();
    let l2 = (dy * dy + l1).to_f64();
    let len2 = (dz * dz + l2).to_f64();
    let (dxd, dyd) = (dx.to_f64(), dy.to_f64());
    let ndx = (-dx).to_f64();
    let mut best = 0.0f64;
    let mut split = 0i32;
    let mut i = 1;
    while i < n - 1 {
        let q = p(i);
        let vx = x(m.f64(q)) - m.f64(points);
        let vy = x(m.f64(q + 8)) - m.f64(points + 8);
        let vz = x(m.f64(q + 16)) - m.f64(points + 16);
        let t13 = x(dxd) * vy - x(dyd) * vx;
        let t16 = x(dxd) * vz - vx * dz;
        let t19 = x(dyd) * vz - vy * dz;
        let t23 = dz * t16 + x(dyd) * t13;
        let t26 = dz * t19 - t13 * x(dxd);
        let t28 = x(ndx) * t16 - x(dyd) * t19;
        let dist = ((t23 * t23 + t26 * t26) + t28 * t28).sqrt() / x(len2);
        if dist.gt(x(best)) {
            best = dist.to_f64();
            split = i;
        }
        i += 1;
    }
    if x(best).gt(x(m[MAX_DIST_LINE].0)) {
        let frame = m.local(0x18);
        let (a, fa, na, b, fb, nb) = (frame, frame + 4, frame + 8, frame + 12, frame + 16, frame + 20);
        simplify_contour(m, points, split + 1, a, fa, na);
        simplify_contour(m, p(split), n - split, b, fb, nb);
        let total = m.i32(nb).wrapping_add(m.i32(na)).wrapping_sub(1);
        m.set_i32(n_at, total);
        let block = m.si_malloc((total as u32).wrapping_mul(PNT));
        m.set_u32(out_at, block);
        let fl = m.si_malloc((m.i32(n_at) as u32).wrapping_mul(2));
        m.set_u32(flags_at, fl);
        let mut k = 0u32;
        let mut i = 0;
        while i < m.i32(na) {
            copy_pnt(m, m.u32(out_at) + k * PNT, m.u32(a) + i as u32 * PNT);
            let f = m.u16(m.u32(fa) + 2 * i as u32);
            m.set_u16(m.u32(flags_at) + 2 * k, f);
            k += 1;
            i += 1;
        }
        if m.i16(m.u32(fb)) != 0 {
            m.set_u16(m.u32(flags_at) + 2 * k - 2, 1);
        }
        let mut i = 1;
        while i < m.i32(nb) {
            copy_pnt(m, m.u32(out_at) + k * PNT, m.u32(b) + i as u32 * PNT);
            let f = m.u16(m.u32(fb) + 2 * i as u32);
            m.set_u16(m.u32(flags_at) + 2 * k, f);
            k += 1;
            i += 1;
        }
        for at in [a, b, fb, fa] {
            let blk = m.u32(at);
            if blk != 0 {
                m.free(blk);
                m.set_u32(at, 0);
            }
        }
        m.release(0x18);
        return;
    }
    let straight = x(best).lt(x(m[MIN_DIST_LINE].0)) && (x(len2).gt(x(LONG_CHORD)) || n > 14);
    m.set_i32(n_at, 2);
    let block = m.si_malloc(0x30);
    m.set_u32(out_at, block);
    copy_pnt(m, block, points);
    copy_pnt(m, block + PNT, last);
    let fl = m.si_malloc((m.i32(n_at) as u32).wrapping_mul(2));
    m.set_u32(flags_at, fl);
    m.set_u16(fl, straight as u16);
    m.set_u16(fl + 2, straight as u16);
}
