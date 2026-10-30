// SPDX-License-Identifier: MIT
//! Evaluation: the value of a curve at a time (finding 0009).
//!
//! Every function follows the disassembly instruction by instruction: each `X87` operation
//! below is one x87 instruction of the original, in its order, and each `to_f32()` is a
//! store to a `float`. Comparisons name the outcome the original branches on; an unordered
//! comparison (a NaN) takes the branch the status-word test sends it down.
//!
//! The evaluators return the value as the original returns it, in the x87 register. In two
//! cases (linear interpolation, gradient extrapolation) that is an unrounded 80-bit value;
//! a caller that assigns it to a `float` rounds it with [`X87::to_f32`], and
//! [`Fcurve::last_value`] always holds the rounded value.

use crate::x87::X87;
use crate::{extrap, Env, Fcurve};

/// The tolerance on times, in seconds: the double 1e-5 (0x0864d240, 0x0864d250).
pub const EPSILON: f64 = 1e-5;

/// Inputs for which the original has no defined result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Undefined {
    /// It reads a key record in front of the array (index −1). Reached with a NaN time, and
    /// with any time once that has left the search index at −1.
    BeforeKeys,
    /// The search walks past the last key into the unused part of the allocation.
    PastKeys,
    /// A cycle loop never ends: the time is infinite, or so many periods away that the loop
    /// counter stops changing.
    NeverReturns,
}

/// The two messages for an extrapolation mode that does not exist (0x08695160 and 0x086951a0),
/// read from the user's install.
fn pre_error() -> &'static str {
    si_rt::text(0x0869_5160)
}
fn post_error() -> &'static str {
    si_rt::text(0x0869_51a0)
}

fn xf(v: f32) -> X87 {
    X87::from_f32(v)
}

fn eps() -> X87 {
    X87::from_f64(EPSILON)
}

/// A frame number as seconds, in the register [soft 0x08201554 SI_frameToSec].
pub fn frame_to_sec(frame: i32, env: &Env) -> X87 {
    X87::from_i32(frame) / xf(env.frame_rate)
}

/// The frame a time falls on [soft 0x08201568 SI_secToFrame]: time × rate, plus 0.5 if the
/// time is above zero and minus 0.5 otherwise, truncated toward zero.
pub fn sec_to_frame(sec: f32, env: &Env) -> i32 {
    let frames = xf(sec) * xf(env.frame_rate);
    let half = if X87::ZERO.lt(xf(sec)) { 0.5 } else { -0.5 };
    (frames + X87::from_f64(half)).to_i32_trunc()
}

/// A time as a fractional frame number, in the register [soft 0x082015b8 SI_secAsFrame].
pub fn sec_as_frame(sec: f32, env: &Env) -> X87 {
    xf(sec) * xf(env.frame_rate)
}

/// `(float)frame / SI_frameRate` as several functions compute it inline: the frame number is
/// converted to `float` first (frames beyond 2^24 lose their low bits), the quotient stays
/// in the register.
pub(crate) fn frame_time(frame: i32, env: &Env) -> X87 {
    xf(X87::from_i32(frame).to_f32()) / xf(env.frame_rate)
}

/// The first `n` in `lo..=hi` for which `exits(n)` holds, where `exits` changes at most once
/// along the range. The cycle loops of the original try n = 1, 2, 3, … in turn; their test
/// value is `T ± n·P` with `T` and `P` fixed, which moves one way only as `n` grows (every
/// rounding involved is monotonic), so the first exit can be found by bisection.
fn first_exit(lo: i64, hi: i64, exits: &dyn Fn(i64) -> bool) -> Option<i64> {
    if exits(lo) {
        return Some(lo);
    }
    if !exits(hi) {
        return None;
    }
    let (mut a, mut b) = (lo, hi);
    while b - a > 1 {
        let mid = a + (b - a) / 2;
        if exits(mid) {
            b = mid;
        } else {
            a = mid;
        }
    }
    Some(b)
}

/// The float evaluator's loop counter is a `float` incremented by 1.0: at 2^24 it stops
/// changing, and a loop that has not ended by then never does.
const FLOAT_COUNTER_LIMIT: i64 = 1 << 24;

impl Fcurve {
    /// The time an evaluation uses. With `check` zero it is `time`. Otherwise an inactive
    /// curve is evaluated at time 0, and an active one clamps the time to its range when the
    /// range is wider than 1e-5.
    fn clamped_time(&self, time: X87, check: i16) -> X87 {
        if check == 0 {
            return time;
        }
        if self.active == 0 {
            return X87::ZERO;
        }
        let (a, b) = (xf(self.range_start), xf(self.range_end));
        if (b - a).abs().gt(eps()) {
            if time.lt(a) {
                return a;
            }
            if time.gt(b) {
                return b;
            }
        }
        time
    }

    /// Start of the key search: the index kept from the last call, or 0 when that is out of
    /// range or its key lies after the time.
    fn search_start(&mut self, x: X87) -> Result<usize, Undefined> {
        let n = self.nkeys();
        if self.cache >= n {
            self.cache = 0;
        } else {
            if self.cache < 0 {
                // the original compares with the record in front of the array
                return Err(Undefined::BeforeKeys);
            }
            if (eps() + x).lt(xf(self.k()[self.cache as usize].time)) {
                self.cache = 0;
            }
        }
        Ok(self.cache as usize)
    }

    /// The value of a curve of real numbers [soft 0x08200808 fcurveFloatValue].
    ///
    /// - No keys: 0.
    /// - Before the first key (`time < t0`), by [`Fcurve::pre_extrap`]: the first value; or
    ///   the line through the first key with its left slope (for a linear curve, the slope
    ///   of the first segment); or the time moved forward by whole periods.
    /// - From the last key on (`time >= tn`), by [`Fcurve::post_extrap`], likewise with the
    ///   last key's right slope and the time moved back.
    /// - Otherwise the segment is searched from the index of the last call. A key within
    ///   1e-5 of the time gives its value. Else the key before the time decides: constant,
    ///   linear, or the cubic Hermite curve through both keys with the first one's right
    ///   slope and the second one's left slope.
    ///
    /// An extrapolation mode other than 0, 1, 2 prints a message and gives 0; an
    /// interpolation other than 0 to 4 gives 0.
    pub fn float_value(&mut self, time: f32, check: i16, env: &mut Env) -> Result<X87, Undefined> {
        let n = self.nkeys() as usize;
        if n == 0 {
            return Ok(X87::ZERO);
        }
        let t = xf(time);
        let mut x = self.clamped_time(t, check);
        self.last_time = x.to_f32();
        let first = self.k()[0];
        let last = self.k()[n - 1];
        let t0 = xf(first.time);
        let tn = xf(last.time);

        if t0.gt(x) {
            match self.pre_extrap {
                extrap::GRADIENT => {
                    if n == 1 {
                        self.last_value = xf(first.value).to_f32();
                        return Ok(xf(first.value));
                    }
                    let second = self.k()[1];
                    let dt = xf(second.time) - t0;
                    let v = xf(first.value);
                    let slope = if self.interp == 1 && !X87::ZERO.equals(dt) {
                        (xf(second.value) - v) / dt
                    } else {
                        xf(first.left_slope)
                    };
                    let r = v - slope * (t0 - x);
                    self.last_value = r.to_f32();
                    return Ok(r);
                }
                extrap::CONSTANT => {
                    self.last_value = xf(first.value).to_f32();
                    return Ok(xf(first.value));
                }
                extrap::CYCLE => {
                    if n == 1 {
                        self.last_time = x.to_f32();
                        self.last_value = xf(first.value).to_f32();
                        return Ok(xf(first.value));
                    }
                    let period = tn - t0;
                    if !period.abs().gt(eps()) {
                        self.last_value = xf(first.value).to_f32();
                        return Ok(xf(first.value));
                    }
                    // do y = time + n·period, n = 1.0, 2.0, … while y < t0; note that the
                    // unclamped time is used
                    let wrapped = |k: i64| X87::from_i32(k as i32) * period + t;
                    let k = first_exit(1, FLOAT_COUNTER_LIMIT, &|k| !wrapped(k).lt(t0)).ok_or(Undefined::NeverReturns)?;
                    let mut y = wrapped(k);
                    if (y - t0).abs().lt(eps()) || y.gt(tn) {
                        y = tn;
                        self.cache = n as i32 - 1;
                    }
                    x = y;
                }
                _ => {
                    env.host.printf(pre_error());
                    return Ok(X87::ZERO);
                }
            }
        }

        if tn.le(x) {
            match self.post_extrap {
                extrap::GRADIENT => {
                    if n == 1 {
                        // the one case that leaves `last_value` alone
                        return Ok(xf(first.value));
                    }
                    let before = self.k()[n - 2];
                    let dt = tn - xf(before.time);
                    let v = xf(last.value);
                    let slope = if self.interp == 1 && !X87::ZERO.equals(dt) {
                        (v - xf(before.value)) / dt
                    } else {
                        xf(last.right_slope)
                    };
                    let r = v + slope * (x - tn);
                    self.last_value = r.to_f32();
                    return Ok(r);
                }
                extrap::CONSTANT => {
                    self.last_value = xf(last.value).to_f32();
                    return Ok(xf(last.value));
                }
                extrap::CYCLE => {
                    if n == 1 {
                        self.last_value = xf(first.value).to_f32();
                        return Ok(xf(first.value));
                    }
                    let period = tn - t0;
                    if !period.abs().gt(eps()) {
                        self.last_value = xf(last.value).to_f32();
                        return Ok(xf(last.value));
                    }
                    let mut y = x;
                    if x.gt(tn) {
                        // do y = time − n·period while y > tn
                        let wrapped = |k: i64| t - X87::from_i32(k as i32) * period;
                        let k =
                            first_exit(1, FLOAT_COUNTER_LIMIT, &|k| !wrapped(k).gt(tn)).ok_or(Undefined::NeverReturns)?;
                        y = wrapped(k);
                    }
                    // a time that lands on the first key is the end of a cycle, not its start
                    if (y - t0).abs().lt(eps()) {
                        y = tn;
                        self.cache = n as i32 - 1;
                    }
                    x = y;
                }
                _ => {
                    env.host.printf(post_error());
                    return Ok(X87::ZERO);
                }
            }
        }

        let mut i = self.search_start(x)?;
        loop {
            if i >= n {
                // every key lies at or before time + 1e-5 and none within 1e-5 of it: the
                // original goes on comparing with the memory behind the last key
                return Err(Undefined::PastKeys);
            }
            let kt = xf(self.k()[i].time);
            if !(x + eps()).ge(kt) {
                break;
            }
            if eps().gt((x - kt).abs()) {
                self.last_value = xf(self.k()[i].value).to_f32();
                self.cache = i as i32;
                return Ok(xf(self.k()[i].value));
            }
            i += 1;
        }
        self.cache = i as i32 - 1;
        if i == 0 {
            // only an unordered comparison (a NaN time) ends the search on its first key; the
            // original then takes the record in front of the array as the segment's start
            return Err(Undefined::BeforeKeys);
        }
        let (k0, k1) = (self.k()[i - 1], self.k()[i]);
        let (t0, v0, t1, v1) = (xf(k0.time), xf(k0.value), xf(k1.time), xf(k1.value));
        match k0.interp {
            1 => {
                let r = ((v1 - v0) * (x - t0)) / (t1 - t0) + v0;
                self.last_value = r.to_f32();
                Ok(r)
            }
            0 => {
                self.last_value = v0.to_f32();
                Ok(v0)
            }
            2..=4 => {
                // cubic Hermite, with h = 1/(t1 − t0) and d = x − t0:
                //   (2d³h³ − 3d²h² + 1)·v0 + (3d²h² − 2d³h³)·v1
                //   + (d³h² − 2d²h + d)·right slope of key 0 + (d³h² − d²h)·left slope of key 1
                let h = X87::ONE / (t1 - t0);
                let d = x - t0;
                let d2 = d * d;
                let h2 = h * h;
                let a = (d2 * d) * h2;
                let a2 = a + a;
                let d2x3 = d2 * X87::from_f64(3.0);
                let b = a2 * h;
                let c = h2 * d2x3;
                let e = h * d2;
                let w0 = X87::ONE + (b - c);
                let w1 = c + (-b);
                let p1 = v1 * w1;
                let p0 = w0 * v0;
                let s = p1 + p0;
                let f = a - e;
                let g = f - e;
                let k = d + g;
                let r = k * xf(k0.right_slope);
                let l = f * xf(k1.left_slope);
                let value = (l + (r + s)).to_f32();
                self.last_value = value;
                Ok(xf(value))
            }
            _ => Ok(X87::ZERO),
        }
    }

    /// How the integer evaluator turns a key value into its result: as a flag (1 if the
    /// value is above zero, else 0), or as a number (value + 0.5 truncated toward zero, so
    /// −2.7 gives −2), which then passes through a `float`.
    fn integer_result(&mut self, value: f32, as_flag: i16) -> i32 {
        if as_flag != 0 {
            self.last_value = if X87::ZERO.lt(xf(value)) { 1.0 } else { 0.0 };
        } else {
            let rounded = (xf(value) + X87::from_f64(0.5)).to_i32_trunc();
            self.last_value = X87::from_i32(rounded).to_f32();
        }
        xf(self.last_value).to_i32_trunc()
    }

    /// The value of a curve of flags or counts [soft 0x08200404 SI_fcurveIntegerValue]: the
    /// value of the key at or before the time, never interpolated.
    ///
    /// Extrapolation: constant and gradient both give the end key; cycle moves the time by
    /// whole periods as the float evaluator does (but counts the periods in an `int`). A
    /// key is "at" the time only on exact equality; otherwise the last key whose time is
    /// not above time + 1e-5 is taken. With several keys spanning less than 1e-5 and cycle
    /// extrapolation the end key's value is returned truncated, without the flag or rounding
    /// step.
    pub fn integer_value(&mut self, time: f32, check: i16, as_flag: i16, env: &mut Env) -> Result<i32, Undefined> {
        let n = self.nkeys() as usize;
        if n == 0 {
            return Ok(0);
        }
        let t = xf(time);
        let mut x = self.clamped_time(t, check);
        self.last_time = x.to_f32();
        let first = self.k()[0];
        let last = self.k()[n - 1];
        let t0 = xf(first.time);
        let tn = xf(last.time);

        if t0.gt(x) {
            let mode = self.pre_extrap;
            if mode < 0 || mode > 2 {
                env.host.printf(pre_error());
                return Ok(0);
            }
            if mode <= 1 || n == 1 {
                return Ok(self.integer_result(first.value, as_flag));
            }
            let period = tn - t0;
            if !period.abs().gt(eps()) {
                self.last_value = xf(first.value).to_f32();
                return Ok(xf(first.value).to_i32_trunc());
            }
            // do y = n·period + time, n = 1, 2, … (an int that wraps) while y < t0
            let wrapped = |k: i64| X87::from_i32(k as i32) * period + t;
            let exits = |k: i64| !wrapped(k).lt(t0);
            let k = first_exit(1, i32::MAX as i64, &exits)
                .or_else(|| first_exit(i32::MIN as i64, 0, &exits))
                .ok_or(Undefined::NeverReturns)?;
            let mut y = wrapped(k);
            if (y - t0).abs().lt(eps()) || y.gt(tn) {
                y = tn;
                self.cache = n as i32 - 1;
            }
            x = y;
        }

        if tn.le(x) {
            let mode = self.post_extrap;
            if mode < 0 || mode > 2 {
                env.host.printf(post_error());
                return Ok(0);
            }
            if mode <= 1 {
                return Ok(self.integer_result(last.value, as_flag));
            }
            if n == 1 {
                return Ok(self.integer_result(first.value, as_flag));
            }
            let period = tn - t0;
            if !period.abs().gt(eps()) {
                self.last_value = xf(last.value).to_f32();
                return Ok(xf(last.value).to_i32_trunc());
            }
            let mut y = x;
            if tn.lt(x) {
                let wrapped = |k: i64| t - X87::from_i32(k as i32) * period;
                let exits = |k: i64| !wrapped(k).gt(tn);
                let k = first_exit(1, i32::MAX as i64, &exits)
                    .or_else(|| first_exit(i32::MIN as i64, 0, &exits))
                    .ok_or(Undefined::NeverReturns)?;
                y = wrapped(k);
            }
            if (y - t0).abs().lt(eps()) {
                y = tn;
                self.cache = n as i32 - 1;
            }
            x = y;
        }

        let mut i = self.search_start(x)?;
        let limit = eps() + x;
        if limit.ge(xf(self.k()[i].time)) {
            loop {
                if xf(self.k()[i].time).equals(x) {
                    self.cache = i as i32;
                    let v = self.k()[i].value;
                    return Ok(self.integer_result(v, as_flag));
                }
                i += 1;
                if i >= n {
                    // a time less than 1e-5 before the last key, but not on it: the original
                    // compares with the memory behind the last key
                    return Err(Undefined::PastKeys);
                }
                if !limit.ge(xf(self.k()[i].time)) {
                    break;
                }
            }
        }
        self.cache = i as i32 - 1;
        if i == 0 {
            return Err(Undefined::BeforeKeys);
        }
        let v = self.k()[i - 1].value;
        Ok(self.integer_result(v, as_flag))
    }

    /// The value of any curve [soft 0x082002dc SI_fcurveValue]. The curve's type selects the
    /// evaluator:
    ///
    /// - visibility and the other on/off parameters (see the list in the finding): the
    ///   integer evaluator, as a flag. Node visibility is read as a number instead while
    ///   `SI_useHiddenForMatte` is set;
    /// - the two photon counts: the integer evaluator, as a number, negative results
    ///   replaced by 0;
    /// - everything else: the float evaluator; for the four energy parameters a negative
    ///   result is replaced by 0.
    pub fn value(&mut self, time: f32, check: i16, env: &mut Env) -> Result<X87, Undefined> {
        match self.kind {
            0x26c | 0xb6 | 0xaf..=0xb2 | 0x111 | 0xc4 | 0x17a | 0x273 | 0x274 | 0x277..=0x279 => {
                let as_flag = if self.kind == 0xaf && env.use_hidden_for_matte != 0 { 0 } else { 1 };
                Ok(X87::from_i32(self.integer_value(time, check, as_flag, env)?))
            }
            0x271 | 0x272 => {
                let r = X87::from_i32(self.integer_value(time, check, 0, env)?);
                Ok(if r.lt(X87::ZERO) { X87::ZERO } else { r })
            }
            _ => {
                let v = self.float_value(time, check, env)?;
                Ok(if X87::ZERO.gt(v) && (0x26d..=0x270).contains(&self.kind) { X87::ZERO } else { v })
            }
        }
    }

    /// The value at a frame [soft 0x08200258 SI_fcurveFrameValue]: at the time
    /// `(float)frame / SI_frameRate`, without the activity and range check.
    pub fn frame_value(&mut self, frame: i32, env: &mut Env) -> Result<X87, Undefined> {
        let time = frame_time(frame, env).to_f32();
        self.value(time, 0, env)
    }

    /// [`Fcurve::value`] as a caller that stores the result in a `float` sees it. Inputs the
    /// original has no defined result for give 0.
    pub fn value_f32(&mut self, time: f32, check: i16, env: &mut Env) -> f32 {
        self.value(time, check, env).map_or(0.0, X87::to_f32)
    }
}
