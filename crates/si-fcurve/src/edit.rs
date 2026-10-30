// SPDX-License-Identifier: MIT
//! Key editing: insertion, removal, moving and scaling of keys, and the automatic slopes
//! (finding 0009). The functions of `fcurve.c` other than the evaluators.
//!
//! Where a new time, value or slope is computed, the sequence of register operations is the
//! original's and `to_f32()` marks the store. A value that the original merely copies with a
//! load and a store goes through [`q`], because that pair makes a signalling NaN quiet.
//!
//! The original addresses keys by pointer into the curve's array; here they are addressed by
//! their position in it. Its 1-based key indices are kept where it uses them.

use crate::eval::{frame_time, sec_as_frame, sec_to_frame, EPSILON};
use crate::keyflag::{self, locked};
use crate::x87::X87;
use crate::{interp, slope, Env, Fcurve, Key};

fn xf(v: f32) -> X87 {
    X87::from_f32(v)
}

fn xd(v: f64) -> X87 {
    X87::from_f64(v)
}

/// A `float` copied through the register (`fld`, `fstp`).
pub(crate) fn q(v: f32) -> f32 {
    xf(v).to_f32()
}

/// The slope the scaling functions store when asked to scale time by zero.
const SLOPE_AT_ZERO_SCALE: f32 = 100000.0;

/// The slope of the line through two points [soft 0x08202178 calculateSlope], in the
/// register: `(v1 − v0) / (t1 − t0)`.
pub fn calculate_slope(t0: f32, v0: f32, t1: f32, v1: f32) -> X87 {
    (xf(v1) - xf(v0)) / (xf(t1) - xf(t0))
}

impl Fcurve {
    fn km(&mut self) -> &mut Vec<Key> {
        self.keys.get_or_insert_with(Vec::new)
    }

    /// The start index the searching functions derive from their argument: clamped to the
    /// last key, then to 0.
    fn clamp_start(&self, start: i32) -> i32 {
        let n = self.nkeys();
        let s = if start >= n { n - 1 } else { start };
        s.max(0)
    }

    /// The time the curve spans, plus one frame [soft 0x081ff23c fcurveRange], in the
    /// register. A curve without a key array spans one frame.
    ///
    /// For an array that is present but empty the original reads the record in front of it;
    /// here that case also gives one frame.
    pub fn range(&self, env: &Env) -> X87 {
        let one_frame = xf(1.0) / xf(env.frame_rate);
        match self.k() {
            [] => one_frame,
            keys => one_frame + (xf(keys[keys.len() - 1].time) - xf(keys[0].time)),
        }
    }

    /// [`Fcurve::range`] in frames [soft 0x081ff1d0 fcurveFrameRange]: times the frame rate,
    /// plus 0.5 (minus 0.5 for a range not above zero), truncated.
    pub fn frame_range(&self, env: &Env) -> i32 {
        let frames = self.range(env) * xf(env.frame_rate);
        let half = if self.range(env).gt(X87::ZERO) { 0.5 } else { -0.5 };
        (frames + xd(half)).to_i32_trunc()
    }

    /// Looks for a key at `time`, from index `start` on [soft 0x081ff2b0 SI_fcurveFindKey].
    ///
    /// Returns the index of the first key within the key tolerance (compared in frames). If
    /// there is none, the index of the last key at or before `time` that the scan passed,
    /// or −1. The scan stops at the first key later than `time` plus the tolerance.
    pub fn find_key(&self, start: i32, time: f32, env: &Env) -> i32 {
        let mut found = -1;
        let tol = xf(env.key_tolerance);
        let mut i = self.clamp_start(start);
        while i < self.nkeys() {
            let kt = self.k()[i as usize].time;
            if (tol / xf(env.frame_rate) + xf(time)).lt(xf(kt)) {
                return found;
            }
            if (sec_as_frame(time, env) - sec_as_frame(kt, env)).abs().le(tol) {
                return i;
            }
            if xf(time).ge(xf(kt)) {
                found = i;
            }
            i += 1;
        }
        found
    }

    /// Index of the first key after `time`, from `start` on; the key count if there is none
    /// [soft 0x081ff38c SI_fcvKeyTimeGreaterThan].
    ///
    /// The original looks at the last key before anything else, also when there is none;
    /// here an empty curve answers 0.
    pub fn key_time_greater_than(&self, start: i32, time: f32) -> i32 {
        let n = self.nkeys();
        let Some(last) = self.k().last() else { return 0 };
        // "not below the last key" leaves at once; an unordered comparison does not
        if xf(time).ge(xf(last.time)) {
            return n;
        }
        let mut i = self.clamp_start(start);
        while i < n {
            if xf(time).lt(xf(self.k()[i as usize].time)) {
                return i;
            }
            i += 1;
        }
        n
    }

    /// Inserts a key, or replaces the key on the same frame, announcing the replacement
    /// [soft 0x081ff400 SI_fcurveAddKey]. Returns the key's index, or −1 if the user refused
    /// to replace a locked key.
    pub fn add_key(&mut self, key: Key, env: &mut Env) -> i32 {
        self.add_key_after(key, 0, 1, 1, env)
    }

    /// Writes `key` into the array at `index` [soft 0x081ff420 SI_fcurveInitKey]: value,
    /// slopes, interpolation, slope type, flags and tag always, the time only if `set_time`.
    /// `lock` 1 makes the key a locked one. The curve's value bounds widen to take the value
    /// in, and the slopes of the key and of its two neighbours are recomputed.
    ///
    /// The original does not check `index`: outside the key count it writes the record into
    /// whatever lies at that position (a spare slot of the allocation, or memory that is not
    /// the curve's), and still widens the bounds and sets the search index. Rust must differ
    /// in the first of these: the write is not made; the rest is. (Found by the emulation
    /// check, finding 0011: the bounds of a curve with an empty array changed in the
    /// original and not in the port.)
    pub fn init_key(&mut self, key: &Key, index: i32, lock: i16, set_time: i16, env: &Env) {
        self.cache = index;
        if index >= 0 && index < self.nkeys() {
            let k = &mut self.km()[index as usize];
            if set_time != 0 {
                k.time = q(key.time);
            }
            k.value = q(key.value);
            k.left_slope = q(key.left_slope);
            k.right_slope = q(key.right_slope);
            k.interp = key.interp;
            k.slope_type = key.slope_type;
            k.spline_index = key.spline_index;
            k.flags = key.flags;
            k.field_20 = key.field_20;
            if env.key_type_propag_mode != 0 {
                k.flags |= keyflag::PROPAG;
            }
            if lock == 1 {
                k.flags = locked(k.flags);
            }
            k.tagged = key.tagged;
        }
        if xf(key.value).lt(xf(self.min_value)) {
            self.min_value = q(key.value);
        }
        if xf(key.value).gt(xf(self.max_value)) {
            self.max_value = q(key.value);
        }
        self.manage_slope(index);
        self.manage_slope(index - 1);
        self.manage_slope(index + 1);
    }

    /// Inserts `key` in time order, or overwrites the key that is on the same frame
    /// [soft 0x081ff588 SI_fcurveAddKeyAfter]. Returns the key's index.
    ///
    /// The scan for a key on the same frame (within the key tolerance) starts at `start`, or
    /// at the index of the last evaluation or insertion when that lies beyond `start` and
    /// its key is at least a tolerance before the new one. Keys before the scan's start are
    /// not looked at, so with a late start a key can be inserted next to one on its frame.
    ///
    /// - A key on the same frame is overwritten in place; it keeps its time unless
    ///   `set_time`. With `warn`, replacing a locked key first asks the user (a refusal
    ///   returns −1 and changes nothing) and leaves the key locked; replacing any other
    ///   prints a status message.
    /// - Otherwise the key is inserted before the first later key (after any key with an
    ///   equal time).
    pub fn add_key_after(&mut self, key: Key, start: i32, warn: i16, set_time: i16, env: &mut Env) -> i32 {
        let n = self.nkeys();
        let start = self.clamp_start(start);
        let tol = xf(env.key_tolerance);
        let mut lock = 0i16;
        let mut existing = false;
        let index;
        if n == 0 {
            // a new array of eight records (an array that was emptied key by key is lost)
            self.keys = Some(vec![Key::new()]);
            index = 0;
        } else {
            let mut i = start;
            let c = self.cache;
            if c > start && c < n {
                let limit = (sec_as_frame(key.time, env) - tol).to_f32();
                if sec_as_frame(self.k()[c as usize].time, env).le(xf(limit)) {
                    i = c;
                }
            }
            while i < self.nkeys() {
                let d = (sec_as_frame(key.time, env) - sec_as_frame(self.k()[i as usize].time, env)).abs();
                if d.le(tol) {
                    if warn != 0 {
                        if self.k()[i as usize].key_type() == keyflag::KEY_FRAME {
                            if env.host.replace_locked_key() == 0 {
                                return -1;
                            }
                            lock = 1;
                        } else {
                            env.host.status_message("Overlapping keys, new one replaces old.", 0);
                        }
                    }
                    existing = true;
                    break;
                }
                i += 1;
            }
            if existing {
                index = i;
            } else {
                // the array grows by eight records whenever the count reaches a multiple of 8
                let keys = self.km();
                let mut j = keys.len();
                keys.push(Key::new());
                while j > 0 && xf(key.time).lt(xf(keys[j - 1].time)) {
                    keys[j] = keys[j - 1];
                    j -= 1;
                }
                index = j as i32;
            }
        }
        let set = (!existing || set_time != 0) as i16;
        self.init_key(&key, index, lock, set, env);
        env.host.fast_set_dirty();
        self.mark_dirty(env);
        index
    }

    /// Frees the key array [soft 0x081ff8bc SI_fcurveRemoveAllKeys]. Nothing else changes:
    /// not the search index, not the value bounds, not the dirty mark.
    pub fn remove_all_keys(&mut self, env: &mut Env) {
        self.keys = None;
        env.host.fast_set_dirty();
    }

    /// The 1-based index of the key at array position `slot`, 0 if the position is not below
    /// the key count [soft 0x081ffc08 fcurveKeyIndex] (the original compares a key's address
    /// with each record's).
    pub fn key_index(&self, slot: usize) -> i32 {
        if slot < self.k().len() { slot as i32 + 1 } else { 0 }
    }

    /// Removes the key at array position `slot` [soft 0x081ff904 si_fcurveRemoveKey].
    pub fn remove_key(&mut self, slot: usize, env: &mut Env) {
        let index = self.key_index(slot);
        self.remove_key_index(index, env);
    }

    /// Removes the key with the 1-based index `index1`
    /// [soft 0x081ff92c SI_fcurveRemoveKeyIndex] and recomputes the slopes of the keys on
    /// either side of the gap. False (NULL in the original) for an index out of range.
    pub fn remove_key_index(&mut self, index1: i32, env: &mut Env) -> bool {
        let i = index1 - 1;
        if i < 0 || i >= self.nkeys() {
            return false;
        }
        self.km().remove(i as usize);
        if index1 - 2 >= 0 {
            self.manage_slope(index1 - 2);
        }
        self.manage_slope(i);
        env.host.fast_set_dirty();
        self.mark_dirty(env);
        true
    }

    /// Removes the keys with 1-based indices `first1..=last1`, or with `struct_only` only
    /// the struct keys among them [soft 0x081ffa08 fcurveRemoveKeyBetweenIndex].
    ///
    /// The struct-only branch compacts the range with a loop of the original's own, kept
    /// here as it is: each struct key is overwritten by the next key of the range that is
    /// not one, but that key is not consumed, and the keys behind the range are then moved
    /// down by the number of struct keys counted. When struct keys and others are mixed in
    /// the range, keys end up duplicated or lost.
    pub fn remove_key_between_index(&mut self, first1: i32, last1: i32, struct_only: i16, env: &mut Env) -> bool {
        let mut lo = first1 - 1;
        let mut hi = last1 - 1;
        let n = self.nkeys();
        if hi < 0 || lo >= n {
            return false;
        }
        if lo < 0 {
            lo = 0;
        }
        if hi >= n {
            hi = n - 1;
        }
        if lo > hi {
            return false;
        }
        let (lo_u, end) = (lo as usize, hi as usize + 1);
        let keys = self.km();
        if struct_only == 0 {
            keys.drain(lo_u..end);
        } else {
            let mut removed = 0usize;
            let mut j = lo_u + 1;
            for i in lo_u..end {
                if keys[i].flags & keyflag::STRUCT != 0 {
                    removed += 1;
                    if j < end {
                        let mut found = true;
                        if keys[j].flags & keyflag::STRUCT != 0 {
                            loop {
                                j += 1;
                                if j >= end {
                                    found = false;
                                    break;
                                }
                                if keys[j].flags & keyflag::STRUCT == 0 {
                                    break;
                                }
                            }
                        }
                        if found {
                            keys[i] = keys[j];
                        }
                    }
                }
                j += 1;
            }
            for k in end..keys.len() {
                keys[k - removed] = keys[k];
            }
            let len = keys.len() - removed;
            keys.truncate(len);
        }
        if lo - 1 >= 0 {
            self.manage_slope(lo - 1);
        }
        self.manage_slope(lo);
        env.host.fast_set_dirty();
        true
    }

    /// Is there a struct key with `t0 <= time <= t1` [soft 0x081ffc54 si_fcvCheckStructKey]?
    pub fn check_struct_key(&self, t0: f32, t1: f32) -> bool {
        self.k().iter().any(|k| xf(k.time).ge(xf(t0)) && xf(k.time).le(xf(t1)) && k.flags & keyflag::STRUCT != 0)
    }

    /// Sets or clears the struct-key flag of the keys with `t0 <= time`, scanning in order
    /// while `time <= t1` [soft 0x081ffccc si_fcvSetStructKeyState]. Clearing keeps only
    /// the key type: the other flag bits go too. Returns 1 if a key was touched.
    pub fn set_struct_key_state(&mut self, t0: f32, t1: f32, on: i16) -> i16 {
        let mut touched = 0;
        for k in self.keys.iter_mut().flatten() {
            if !xf(t1).ge(xf(k.time)) {
                break;
            }
            if xf(t0).le(xf(k.time)) {
                if on != 0 {
                    k.flags |= keyflag::STRUCT;
                } else {
                    k.flags &= keyflag::TYPE_MASK;
                }
                touched = 1;
            }
        }
        touched
    }

    /// Adds `dt` to the time of the keys with `t0 <= time`, scanning in order while
    /// `time <= t1`; with `struct_only`, of the struct keys only
    /// [soft 0x081ffd7c SI_fcvMoveKeyTime]. If a key moved, the keys are sorted again.
    /// Returns 1 if a key moved.
    pub fn move_key_time(&mut self, t0: f32, t1: f32, dt: f32, struct_only: i16, env: &mut Env) -> i16 {
        let mut moved = 0;
        for k in self.keys.iter_mut().flatten() {
            // each key is tested as the scan reaches it, after the keys before it have moved
            if !xf(t1).ge(xf(k.time)) {
                break;
            }
            if xf(k.time).ge(xf(t0)) && (struct_only == 0 || k.flags & keyflag::STRUCT != 0) {
                k.time = (xf(k.time) + xf(dt)).to_f32();
                moved = 1;
            }
        }
        if moved != 0 {
            self.sort_keys(env);
        }
        moved
    }

    /// Moves every key with `t0 <= time <= t1` onto its frame: the time becomes
    /// `(float)frame / rate` [soft 0x081ffe3c SI_fcvSetKeysOnFrames]. A key that lands on
    /// the time of the key before it is reported in the status line; nothing is merged or
    /// sorted. Returns 0 only for a curve without a key array.
    pub fn set_keys_on_frames(&mut self, t0: f32, t1: f32, env: &mut Env) -> i32 {
        let Some(keys) = self.keys.as_mut() else { return 0 };
        for i in 0..keys.len() {
            let kt = xf(keys[i].time);
            if kt.ge(xf(t0)) && kt.le(xf(t1)) {
                let frame = sec_to_frame(keys[i].time, env);
                keys[i].time = frame_time(frame, env).to_f32();
                if i != 0 && xf(keys[i].time).equals(xf(keys[i - 1].time)) {
                    env.host.status_message("Keys set to same frame", 2);
                }
            }
        }
        1
    }

    /// Scales key times about `pivot`: a key at `time` goes to
    /// `((start − pivot)·scale + pivot) + (time − start)·scale`
    /// [soft 0x081fffa0 SI_fcvScaleKeyTime]. Affected are the keys with `start <= time`,
    /// scanning in order while `time <= end`; with `struct_only` only struct keys; locked
    /// and path keys only with `all_types`. Sorts if a key moved; returns 1 if one did.
    pub fn scale_key_time(&mut self, start: f32, pivot: f32, end: f32, scale: f32, struct_only: i16, all_types: i16,
                          env: &mut Env) -> i16 {
        let (a, b, s) = (xf(start), xf(pivot), xf(scale));
        let base = (a - b) * s + b;
        let mut moved = 0;
        for k in self.keys.iter_mut().flatten() {
            if !xf(end).ge(xf(k.time)) {
                break;
            }
            if all_types == 0 && matches!(k.key_type(), keyflag::KEY_FRAME | keyflag::KEY_PATH) {
                continue;
            }
            if !xf(k.time).ge(a) || (struct_only != 0 && k.flags & keyflag::STRUCT == 0) {
                continue;
            }
            k.time = ((xf(k.time) - a) * s + base).to_f32();
            moved = 1;
        }
        if moved != 0 {
            self.sort_keys(env);
        }
        moved
    }

    /// Opens (`insert` non-zero) or closes gaps of `gap` frames at every `step` frames from
    /// `first` to `last` [soft 0x08200094 si_fcvFrameStretch].
    ///
    /// For j = 0, 1, …, (last − first)/step: with T the time of frame
    /// `(step + gap')·j + first` (gap' is `gap`, negated when closing), closing first
    /// removes the keys strictly between T − 1e-5 and T + |gap| − 1e-5; then all keys from T
    /// to the last key's time move by gap' frames. Finally all slopes are recomputed.
    /// Returns 0 without doing anything if there are no keys or `last <= first`.
    ///
    /// The original divides by `step` without a test; a zero step (a fatal trap there)
    /// returns 0 here.
    pub fn frame_stretch(&mut self, first: i32, last: i32, gap: i32, step: i32, insert: i16, env: &mut Env) -> i32 {
        if self.keys.is_none() || self.nkeys() == 0 || last <= first {
            return 0;
        }
        let gap = if insert == 0 { gap.wrapping_neg() } else { gap };
        let dt = frame_time(gap, env);
        let Some(quotient) = last.wrapping_sub(first).checked_div(step) else { return 0 };
        let count = quotient.wrapping_add(1);
        let mut j = 0i32;
        while j < count {
            let t = frame_time(step.wrapping_add(gap).wrapping_mul(j).wrapping_add(first), env);
            // with no key left the original picks this up in front of the array, but then
            // neither call below does anything
            let last_time = self.k().last().map_or(0.0, |k| k.time);
            if insert == 0 {
                let upper = ((t - dt) - xd(EPSILON)).to_f32();
                let lower = (t - xd(EPSILON)).to_f32();
                self.remove_key_between_times(0, lower, upper, 0, env);
            }
            self.move_key_time(t.to_f32(), last_time, dt.to_f32(), 0, env);
            j += 1;
        }
        self.default_slope(env);
        1
    }

    /// Sorts the keys by time [soft 0x082071e8 SI_fcurveSortKeys] (`fcurveEdit.c`) and
    /// recomputes all slopes. A bubble sort that exchanges two neighbours only when the
    /// earlier one is more than 1e-5 later than the other, and stops after the first pass
    /// without an exchange.
    pub fn sort_keys(&mut self, env: &mut Env) {
        let n = self.k().len();
        if n <= 1 {
            return;
        }
        let keys = self.km();
        let mut pass = 1;
        loop {
            let mut sorted = true;
            for a in 0..n - pass {
                if xf(keys[a].time).gt(xd(EPSILON) + xf(keys[a + 1].time)) {
                    keys.swap(a, a + 1);
                    sorted = false;
                }
            }
            if pass >= n - 1 || sorted {
                break;
            }
            pass += 1;
        }
        self.default_slope(env);
    }

    /// Recomputes the slopes of every key [soft 0x08200d50 SI_fcurveDefaultSlope] and marks
    /// the curve dirty.
    pub fn default_slope(&mut self, env: &mut Env) {
        if self.nkeys() == 0 {
            return;
        }
        for i in 0..self.nkeys() {
            self.manage_slope(i);
        }
        self.mark_dirty(env);
    }

    /// Adds `increment` to the value of every key behind array position `slot`
    /// [soft 0x08200dc0 fcurveIncValueAfter] (of every key, if the position is not a key's).
    pub fn inc_value_after(&mut self, slot: usize, increment: f32, env: &mut Env) {
        let n = self.nkeys();
        let index = self.key_index(slot);
        if index < n {
            for j in index + 1..=n {
                let v = (xf(increment) + xf(self.k()[j as usize - 1].value)).to_f32();
                self.key_set_value(j as usize - 1, v, env);
            }
        }
    }

    /// Adds `increment` to every key value above `threshold`
    /// [soft 0x08200e2c si_fcurveIncValueAbove].
    pub fn inc_value_above(&mut self, threshold: f32, increment: f32, env: &mut Env) {
        for i in 0..self.k().len() {
            let v = xf(self.k()[i].value);
            if v.gt(xf(threshold)) {
                self.key_set_value(i, (v + xf(increment)).to_f32(), env);
            }
        }
    }

    /// Shifts the unlocked keys in time, then sorts [soft 0x08200e84 SI_fcurveOffset].
    /// False (NULL) for a curve without keys.
    pub fn offset(&mut self, dt: f32, env: &mut Env) -> bool {
        if self.nkeys() == 0 {
            return false;
        }
        for k in self.km() {
            if k.key_type() != keyflag::KEY_FRAME {
                k.time = (xf(k.time) + xf(dt)).to_f32();
            }
        }
        self.sort_keys(env);
        true
    }

    /// The slope adjustment that goes with a scaling of time: only for spline keys with a
    /// user slope; a scale of exactly zero stores 100000.
    fn rescale_slopes(k: &mut Key, scale: f32) {
        if k.interp > 1 && k.slope_type & slope::USER != 0 {
            if X87::ZERO.equals(xf(scale)) {
                k.right_slope = SLOPE_AT_ZERO_SCALE;
                k.left_slope = SLOPE_AT_ZERO_SCALE;
            } else {
                k.right_slope = (xf(k.right_slope) / xf(scale)).to_f32();
                k.left_slope = (xf(k.left_slope) / xf(scale)).to_f32();
            }
        }
    }

    /// Multiplies the time of every unlocked key by `scale` and divides its user slopes by
    /// it, then sorts [soft 0x08200ef4 si_fcurveScale]. A scale strictly between 0.99999 and
    /// 1.00001 does nothing. False (NULL) for a curve without keys.
    pub fn scale(&mut self, scale: f32, env: &mut Env) -> bool {
        if self.nkeys() == 0 {
            return false;
        }
        if xf(scale).lt(xd(1.00001)) && xf(scale).gt(xd(0.99999)) {
            return true;
        }
        for k in self.km() {
            if k.key_type() != keyflag::KEY_FRAME {
                k.time = (xf(k.time) * xf(scale)).to_f32();
                Fcurve::rescale_slopes(k, scale);
            }
        }
        self.sort_keys(env);
        true
    }

    /// Scales the values about `pivot` (`pivot + (value − pivot)·scale`) and multiplies user
    /// slopes by the scale, for every key that is not a path key; then recomputes the slopes
    /// [soft 0x08200fe0 si_fcurveValScale]. A scale strictly between 0.99999 and 1.00001
    /// does nothing.
    pub fn val_scale(&mut self, scale: f32, pivot: f32, env: &mut Env) {
        if self.nkeys() == 0 || (xf(scale).lt(xd(1.00001)) && xf(scale).gt(xd(0.99999))) {
            return;
        }
        let (s, p) = (xf(scale), xf(pivot));
        for k in self.km() {
            if k.key_type() != keyflag::KEY_PATH {
                k.value = (p + (xf(k.value) - p) * s).to_f32();
                if k.slope_type & slope::USER != 0 {
                    k.right_slope = (xf(k.right_slope) * s).to_f32();
                    k.left_slope = (xf(k.left_slope) * s).to_f32();
                }
            }
        }
        self.default_slope(env);
    }

    /// Scales the times about `pivot` (`(time − pivot)·scale + pivot`) for keys that are
    /// neither locked nor path keys, and divides the user slopes of every key by the scale;
    /// then sorts [soft 0x082010b0 SI_fcurveScaleOrigin].
    pub fn scale_origin(&mut self, scale: f32, pivot: f32, env: &mut Env) {
        if self.nkeys() == 0 {
            return;
        }
        for k in self.km() {
            if !matches!(k.key_type(), keyflag::KEY_FRAME | keyflag::KEY_PATH) {
                k.time = ((xf(k.time) - xf(pivot)) * xf(scale) + xf(pivot)).to_f32();
            }
            Fcurve::rescale_slopes(k, scale);
        }
        self.sort_keys(env);
    }

    /// Spreads the keys: key i of n moves by `amount·i/(n−1)`, locked keys excepted; then
    /// sorts [soft 0x08201184 fcurveSpread]. False (NULL) with fewer than two keys.
    pub fn spread(&mut self, amount: f32, env: &mut Env) -> bool {
        let n = self.nkeys();
        if n < 2 {
            return false;
        }
        let steps = X87::from_i32(n - 1);
        for (i, k) in self.km().iter_mut().enumerate().skip(1) {
            if k.key_type() != keyflag::KEY_FRAME {
                k.time = ((xf(amount) * X87::from_i32(i as i32)) / steps + xf(k.time)).to_f32();
            }
        }
        self.sort_keys(env);
        true
    }

    /// Scales the stretch of time from `start` to `end` about `pivot` and shifts everything
    /// by `offset`, keeping the keys outside the stretch attached to its ends
    /// [soft 0x08201210 SI_fcurveOffsetAndScale]. For an unlocked key at `time`, with
    /// S = (start − pivot)·scale + pivot kept in the register and
    /// E = (end − pivot)·scale + pivot rounded to `float`:
    ///
    /// - `time < start`: time + ((S − start) + offset);
    /// - `start <= time < end`: ((time − start)·scale + S) + offset, and its user slopes are
    ///   divided by the scale;
    /// - otherwise: time + ((E − end) + offset).
    ///
    /// Then the keys are sorted.
    pub fn offset_and_scale(&mut self, start: f32, pivot: f32, end: f32, offset: f32, scale: f32, env: &mut Env) {
        if self.nkeys() == 0 {
            return;
        }
        let (a, b, c, off, s) = (xf(start), xf(pivot), xf(end), xf(offset), xf(scale));
        let new_start = (a - b) * s + b;
        let new_end = xf(((c - b) * s + b).to_f32());
        for k in self.km() {
            if k.key_type() == keyflag::KEY_FRAME {
                continue;
            }
            let kt = xf(k.time);
            if kt.lt(a) {
                k.time = (kt + ((new_start - a) + off)).to_f32();
            } else if kt.lt(c) {
                k.time = (((kt - a) * s + new_start) + off).to_f32();
                Fcurve::rescale_slopes(k, scale);
            } else {
                k.time = (kt + ((new_end - c) + off)).to_f32();
            }
        }
        self.sort_keys(env);
    }

    /// Removes keys whose **value** lies in `[low, high)`
    /// [soft 0x08201350 si_fcurveRemoveKeyBetween]. The scan steps on after each removal, so
    /// of two such keys in a row the second survives. False (NULL) for a curve without keys.
    pub fn remove_key_between(&mut self, low: f32, high: f32, env: &mut Env) -> bool {
        if self.nkeys() == 0 {
            return false;
        }
        let mut i = 0;
        while i < self.k().len() {
            let v = xf(self.k()[i].value);
            if v.ge(xf(low)) && v.lt(xf(high)) {
                self.remove_key(i, env);
            }
            i += 1;
        }
        true
    }

    /// Removes the keys with `t0 < time < t1`, looking from index `start` on; with
    /// `struct_only`, the struct keys among them
    /// [soft 0x082013cc SI_fcurveRemoveKeyBetweenTimes]. False (NULL) for a curve without
    /// keys or when `t1 < t0`.
    pub fn remove_key_between_times(&mut self, start: i32, t0: f32, t1: f32, struct_only: i16, env: &mut Env) -> bool {
        let n = self.nkeys();
        if n == 0 || xf(t1).lt(xf(t0)) {
            return false;
        }
        let time = |i: i32| xf(self.k()[i as usize].time);
        let mut i = start;
        // a start below zero would index in front of the array in the original
        if i < 0 {
            i = 0;
        }
        while i < n && xf(t0).ge(time(i)) {
            i += 1;
        }
        let first1 = i + 1;
        while i < n && xf(t1).gt(time(i)) {
            i += 1;
        }
        self.remove_key_between_index(first1, i, struct_only, env);
        true
    }

    /// Removes the keys within `delta` seconds of the current frame's time
    /// [soft 0x082014b0 si_fcurveRemoveKeyRound], stepping on after each removal. False
    /// (NULL) with fewer than two keys.
    pub fn remove_key_round(&mut self, delta: f32, env: &mut Env) -> bool {
        let now = crate::eval::frame_to_sec(env.frame_current, env);
        if self.keys.is_none() || self.nkeys() < 2 {
            return false;
        }
        let mut i = 0;
        while i < self.k().len() {
            let kt = xf(self.k()[i].time);
            if kt.le(now + xf(delta)) && (now - xf(delta)).le(kt) {
                self.remove_key(i, env);
            }
            i += 1;
        }
        true
    }

    /// The positions of the first and the last tagged key; (false, −1, −1) if no key is
    /// tagged [soft 0x082015c8 si_fcvCheckTagIndex]. With exactly one tagged key the second
    /// index stays −1.
    pub fn check_tag_index(&self) -> (bool, i32, i32) {
        let (mut first, mut last) = (-1, -1);
        for (i, k) in self.k().iter().enumerate() {
            if k.tagged != 0 {
                if first == -1 {
                    first = i as i32;
                } else {
                    last = i as i32;
                }
            }
        }
        (first != -1, first, last)
    }

    /// Locks (`on`) or unlocks the keys with `t0 <= time <= t1`
    /// [soft 0x08202028 si_fcurveSetIntervalKeyLocks]. Unlocking sets the type to free-form,
    /// whatever it was.
    pub fn set_interval_key_locks(&mut self, t0: f32, t1: f32, on: i16) {
        for k in self.keys.iter_mut().flatten() {
            if xf(k.time).ge(xf(t0)) && xf(k.time).le(xf(t1)) {
                k.flags = if on == 0 { k.flags & 0x70 } else { locked(k.flags) };
            }
        }
    }

    /// Position of the last key [soft 0x082020d4 si_fcurveLastKey]. The original returns the
    /// address `keys + count − 1` unchecked; `None` stands for the address in front of an
    /// empty array.
    pub fn last_key(&self) -> Option<usize> {
        self.k().len().checked_sub(1)
    }

    /// Position of the key with 1-based index `index1` [soft 0x082020ec SI_fcurveKey]; an
    /// index out of range prints a message and gives `None` (NULL).
    pub fn key(&self, index1: i32, env: &mut Env) -> Option<usize> {
        if index1 < 1 || self.nkeys() < index1 {
            env.host.printf(&format!("Fatal Error: fcurveKey, index out of range {} (1, {})\n", index1, self.nkeys()));
            None
        } else {
            Some(index1 as usize - 1)
        }
    }

    /// Gives the curve and every key the interpolation `kind` and recomputes the slopes
    /// [soft 0x08202128 SI_fcurveSetInterp]. Does nothing for a curve without keys.
    pub fn set_interp(&mut self, kind: i16, env: &mut Env) {
        if self.nkeys() <= 0 {
            return;
        }
        self.interp = kind;
        for k in self.km() {
            k.interp = kind;
        }
        self.default_slope(env);
    }

    /// Derives the slopes of key `index` [soft 0x08202194 SI_fcnManageSlope]. Returns 0 for
    /// an index out of range, else 1.
    ///
    /// - A user slope stays; unless the key is also marked "double", the left slope is made
    ///   equal to the right one.
    /// - Otherwise both slopes are zero for the first key, the last key, and any slope-0 key.
    /// - A plateau key gets zero slopes if its value is within 1% (of its own magnitude) of a
    ///   neighbour's value, or if it is a local extremum by more than 0.0001 on both sides.
    /// - Otherwise a "double" key gets the slopes of the lines to its two neighbours, and any
    ///   other key the slope of the line through its two neighbours, for both sides.
    pub fn manage_slope(&mut self, index: i32) -> i32 {
        let n = self.nkeys();
        if index < 0 || index >= n {
            return 0;
        }
        let i = index as usize;
        let k = self.k()[i];
        if k.slope_type & slope::USER != 0 {
            if k.slope_type & slope::DOUBLE == 0 {
                self.km()[i].left_slope = q(k.right_slope);
            }
            return 1;
        }
        let flat = |keys: &mut Vec<Key>| {
            keys[i].right_slope = 0.0;
            keys[i].left_slope = 0.0;
        };
        if n == 1 || index == 0 || index == n - 1 || k.interp == interp::SLOPE0 {
            flat(self.km());
            return 1;
        }
        let (prev, next) = (self.k()[i - 1], self.k()[i + 1]);
        if k.interp == interp::PLATEAU {
            let tolerance = xf((xf(k.value) * xd(0.01)).abs().to_f32());
            if (xf(prev.value) - xf(k.value)).abs().le(tolerance) || (xf(k.value) - xf(next.value)).abs().le(tolerance) {
                flat(self.km());
                return 1;
            }
            let side = |d: X87| -> i16 {
                if d.lt(xd(-0.0001)) {
                    -1
                } else {
                    d.gt(xd(0.0001)) as i16
                }
            };
            let (below, above) = (side(xf(k.value) - xf(prev.value)), side(xf(k.value) - xf(next.value)));
            if (below == 1 && above == 1) || (below == -1 && above == -1) {
                flat(self.km());
                return 1;
            }
        }
        let keys = self.km();
        if k.slope_type & slope::DOUBLE != 0 {
            keys[i].left_slope = calculate_slope(prev.time, prev.value, k.time, k.value).to_f32();
            keys[i].right_slope = calculate_slope(k.time, k.value, next.time, next.value).to_f32();
        } else {
            let s = calculate_slope(prev.time, prev.value, next.time, next.value).to_f32();
            keys[i].right_slope = s;
            keys[i].left_slope = s;
        }
        1
    }

    /// The debugging listing [soft 0x082023f0 printKeys], as the text it prints. `address`
    /// stands for the curve's address, which the first line shows in hexadecimal.
    pub fn print_keys(&self, address: u32, env: &Env) -> String {
        let mut out = format!("fcurve ptr {:x}, size {}\n", address, self.nkeys());
        for (i, k) in self.k().iter().enumerate() {
            out.push_str(&format!(
                "  {} - t:{}({}) val:{}  left:{}  right: {}\n",
                i,
                format_g(k.time as f64),
                format_g(sec_as_frame(k.time, env).to_f64()),
                format_g(k.value as f64),
                format_g(k.left_slope as f64),
                format_g(k.right_slope as f64)
            ));
        }
        out
    }

    /// Overwrites the flags word of every key with `flags`
    /// [soft 0x08202480 setFcurveKeyType] (the key type and the flag bits alike).
    pub fn set_key_type(&mut self, flags: i32) {
        for k in self.keys.iter_mut().flatten() {
            k.flags = flags as u16;
        }
    }

    /// Overwrites the flags word of the keys on the frame of `time`
    /// [soft 0x082024b4 setTimeFcurveKeyType].
    pub fn set_time_key_type(&mut self, flags: i32, time: f32, env: &Env) {
        let frame = sec_to_frame(time, env);
        for k in self.keys.iter_mut().flatten() {
            if sec_to_frame(k.time, env) == frame {
                k.flags = flags as u16;
            }
        }
    }

    /// Sets the value bounds to the least and greatest key value, or both to 0 without keys
    /// [soft 0x082029d8 SI_fcvMinMaxValue]. (Values beyond ±1e10 do not count: the scan
    /// starts from those.)
    pub fn min_max_value(&mut self) {
        if self.nkeys() <= 0 {
            self.min_value = 0.0;
            self.max_value = 0.0;
            return;
        }
        let (mut least, mut greatest) = (xf(1e10), xf(-1e10));
        for k in self.k() {
            let v = xf(k.value);
            if v.lt(least) {
                least = v;
            }
            // the comparison for the greatest uses the least when that was just replaced:
            // the same number
            if v.gt(greatest) {
                greatest = v;
            }
        }
        self.min_value = least.to_f32();
        self.max_value = greatest.to_f32();
    }

    /// The frame of the first key after `frame` that is not a raw key
    /// [soft 0x08202a58 SI_fcurveNextKey].
    pub fn next_key(&self, frame: i32, env: &Env) -> Option<i32> {
        self.k().iter().map(|k| (sec_to_frame(k.time, env), k)).find(|(f, k)| *f > frame && k.key_type() != keyflag::RAW).map(|(f, _)| f)
    }

    /// The frame of the last key before `frame` that is not a raw key
    /// [soft 0x08202b2c SI_fcurvePreviousKey].
    pub fn previous_key(&self, frame: i32, env: &Env) -> Option<i32> {
        self.k().iter().rev().map(|k| (sec_to_frame(k.time, env), k)).find(|(f, k)| *f < frame && k.key_type() != keyflag::RAW).map(|(f, _)| f)
    }

    /// Makes an on/off curve (an activation curve) read `on` from `t0` to `t1`
    /// [soft 0x08202c08 si_fcurveActiveRangeSet].
    ///
    /// The value just before `t0` and the value at `t1` are read first (as flags). The keys
    /// in `[t0, t1]` are removed. A key with the new state is added at `t0` unless the curve
    /// already read that state before it; a key with the opposite state is added at `t1` if
    /// the curve did not read `on` there (or, when switching off, if at most one key was
    /// there to begin with). Last, a key whose value equals both neighbours' is removed.
    pub fn active_range_set(&mut self, t0: f32, t1: f32, on: i16, env: &mut Env) -> Result<(), crate::Undefined> {
        let inside: f32 = if on != 0 { 1.0 } else { 0.0 };
        let outside: f32 = if on != 0 { 0.0 } else { 1.0 };
        let before = self.integer_value((xf(t0) - xd(0.0001)).to_f32(), 0, 1, env)?;
        let need_start = !xf(inside).equals(X87::from_i32(before));
        let at_end = self.integer_value(t1, 0, 1, env)?;
        let need_end = at_end != on as i32 || (self.nkeys() <= 1 && X87::ZERO.lt(xf(outside)));
        let mut i = 0;
        while i < self.nkeys() {
            let kt = xf(self.k()[i as usize].time);
            if kt.ge(xf(t0)) && kt.le(xf(t1)) {
                self.remove_key_index(i + 1, env);
            } else {
                i += 1;
            }
        }
        if need_start {
            let key = Key::create(t0, inside, 0.0, 0, self.interp);
            self.add_key(key, env);
        }
        if need_end {
            let key = Key::create(t1, outside, 0.0, 0, self.interp);
            self.add_key(key, env);
        }
        let mut i = 1;
        while i < self.nkeys() {
            let next = i + 1;
            if next < self.nkeys() {
                let k = self.k();
                let v = xf(k[i as usize].value);
                if v.equals(xf(k[i as usize - 1].value)) && xf(k[next as usize].value).equals(v) {
                    self.remove_key_index(next, env);
                    continue;
                }
            }
            i = next;
        }
        Ok(())
    }

    /// Position of the first key within the key tolerance of `time`; the scan gives up at
    /// the first key later than `time` [soft 0x08202dcc SI_fcvGetKeyFrame].
    pub fn get_key_frame(&self, time: f32, env: &Env) -> Option<usize> {
        let tolerance = xf(env.key_tolerance) / xf(env.frame_rate);
        for (i, k) in self.k().iter().enumerate() {
            if (xf(time) - xf(k.time)).abs().le(tolerance) {
                return Some(i);
            }
            if xf(time).lt(xf(k.time)) {
                return None;
            }
        }
        None
    }

    /// Does the curve have a raw key [soft 0x08202e54 SI_fcurveCheckForRawKeys]?
    pub fn check_for_raw_keys(&self) -> bool {
        self.k().iter().any(|k| k.key_type() == keyflag::RAW)
    }

    /// Removes the raw keys [soft 0x08202ea4 SI_fcurveRemoveRawKeys]. Slopes, bounds and
    /// dirty mark are left as they are.
    pub fn remove_raw_keys(&mut self) {
        if let Some(keys) = self.keys.as_mut() {
            keys.retain(|k| k.key_type() != keyflag::RAW);
        }
    }

    /// Pastes the keys of `self` into `dst` at `t0` [soft 0x08202f34 SI_fcvSimplePasteInsert].
    /// The source's length is its range, `range_end − range_start`.
    ///
    /// - With `t1 − t0` below 0.0001 the paste inserts: the keys of `dst` from `t0` on move
    ///   back by the source's length, and the source keys go in at `t0 + time`.
    /// - Otherwise the paste replaces: the keys of `dst` strictly between `t0` and `t1` are
    ///   removed and the source keys go in at `t0 + time·(t1 − t0)/length` (all at `t0` if
    ///   the length is below 1e-5).
    ///
    /// Each key is added without a warning; one that meets a key on its frame replaces it.
    pub fn simple_paste_insert(&self, dst: &mut Fcurve, t0: f32, t1: f32, env: &mut Env) -> i32 {
        // The original copies the source's keys first, into `SI_malloc(capacity × 36)`. For
        // a source whose array is present but empty that is `SI_malloc(0)`, which answers
        // NULL, and the original takes that for a failed allocation: it returns 0 with the
        // destination untouched. (A source without an array skips the copy and goes on.)
        // Found by the emulation check, finding 0011.
        if self.keys.is_some() && self.nkeys() == 0 {
            return 0;
        }
        let span = xf(t1) - xf(t0);
        let insert = span.lt(xd(0.0001));
        let length = (xf(self.range_end) - xf(self.range_start)).to_f32();
        let mut keys: Vec<Key> = self.k().to_vec();
        let scale: f32;
        if insert {
            scale = 1.0;
            if let Some(last) = dst.k().last().copied() {
                dst.move_key_time(t0, last.time, length, 0, env);
            }
        } else {
            scale = if xf(length).lt(xd(EPSILON)) { 0.0 } else { (xf(span.to_f32()) / xf(length)).to_f32() };
            if dst.nkeys() > 0 {
                dst.remove_key_between_times(0, t0, t1, 0, env);
            }
        }
        for k in keys.iter_mut() {
            k.time = (xf(scale) * xf(k.time) + xf(t0)).to_f32();
            dst.add_key_after(k.copy(), 0, 0, 1, env);
        }
        1
    }

    /// How many keys have `t0 <= time <= t1` [soft 0x082031dc SI_fcvCntKeysBetweenTimes].
    pub fn cnt_keys_between_times(&self, t0: f32, t1: f32) -> i32 {
        self.k().iter().filter(|k| xf(k.time).ge(xf(t0)) && xf(k.time).le(xf(t1))).count() as i32
    }

    /// Copies the keys of `src` with `t0 <= time <= t1` into `self`, as one block placed
    /// before the first key later than `at`, each at `at + (time − t0)·scale`
    /// [soft 0x08203234 SI_fcvCopyKeysBetweenTimes].
    ///
    /// The records are copied whole; no slope is recomputed, nothing is sorted, and keys of
    /// `self` that the block overlaps in time stay where they are.
    pub fn copy_keys_between_times(&mut self, src: &Fcurve, at: f32, t0: f32, t1: f32, scale: f32, env: &mut Env) {
        if src.cnt_keys_between_times(t0, t1) <= 0 {
            return;
        }
        let keys = self.km();
        let mut pos = 0;
        while pos < keys.len() && !xf(at).lt(xf(keys[pos].time)) {
            pos += 1;
        }
        let block: Vec<Key> = src
            .k()
            .iter()
            .filter(|k| xf(k.time).ge(xf(t0)) && xf(k.time).le(xf(t1)))
            .map(|k| Key { time: ((xf(k.time) - xf(t0)) * xf(scale) + xf(at)).to_f32(), ..*k })
            .collect();
        keys.splice(pos..pos, block);
        env.host.fast_set_dirty();
        self.mark_dirty(env);
    }

    /// Repeats the keys of `[t0, t1]` so that they appear `cycles` times in all
    /// [soft 0x08203408 SI_fcvCycleKeysBetweenTimes]. Copy number c (1 to cycles − 1) is the
    /// block shifted by `c·(float)(t1 − t0)`; its keys are locked. The copies overwrite the
    /// keys that lay in the time they now cover, `(t1, t1 + (cycles − 1)·(t1 − t0)]`.
    ///
    /// When the copies need fewer records than the overwritten keys had, the original only
    /// lowers the key count: the keys behind are not moved down, so the curve keeps the
    /// leftover overwritten keys and loses that many keys from its end. Kept as it is.
    pub fn cycle_keys_between_times(&mut self, t0: f32, t1: f32, cycles: i32, env: &mut Env) {
        let period = xf(t1) - xf(t0);
        let period_f32 = period.to_f32();
        let end = period * X87::from_i32(cycles.wrapping_sub(1)) + xf(t1);
        let (mut first, mut inside, mut covered) = (0usize, 0usize, 0usize);
        for (i, k) in self.k().iter().enumerate() {
            let kt = xf(k.time);
            if kt.ge(xf(t0)) && kt.le(xf(t1)) {
                inside += 1;
                if inside == 1 {
                    first = i;
                }
            } else if kt.gt(xf(t1)) && kt.le(end) {
                covered += 1;
            }
        }
        // (cycles − 1) × keys in the block; negative for cycles below 1, which only shortens
        // the array
        let added = (cycles as i64 - 1) * inside as i64;
        let block_end = first + inside;
        let tail = covered + block_end;
        let n = self.k().len();
        let new_len = (n as i64 + added - covered as i64).max(0) as usize;
        let keys = self.km();
        if added > covered as i64 {
            keys.resize(new_len, Key::new());
            if tail < n {
                keys.copy_within(tail..n, block_end + added as usize);
            }
        } else {
            keys.truncate(new_len);
        }
        for c in 1..cycles.max(1) as usize {
            let shift = xf(period_f32) * X87::from_i32(c as i32);
            for j in 0..inside {
                let mut k = keys[first + j];
                k.time = (xf(k.time) + shift).to_f32();
                k.flags = locked(k.flags);
                keys[inside * c + first + j] = k;
            }
        }
        env.host.fast_set_dirty();
        self.mark_dirty(env);
    }
}

/// Are two curves the same within 0.0001: key count, and time, value and both slopes of
/// every key [soft 0x082030d8 SI_fcvEqual]? Two curves without keys are equal; a difference
/// that is not a number does not count as one.
pub fn fcv_equal(a: Option<&Fcurve>, b: Option<&Fcurve>) -> i16 {
    let empty = |c: Option<&Fcurve>| c.map_or(true, |c| c.nkeys() == 0);
    if empty(a) {
        return empty(b) as i16;
    }
    let (a, Some(b)) = (a.unwrap(), b) else { return 0 };
    if b.nkeys() == 0 || a.nkeys() != b.nkeys() {
        return 0;
    }
    let differs = |x: f32, y: f32| (xf(x) - xf(y)).abs().gt(xd(0.0001));
    for (p, r) in a.k().iter().zip(b.k()) {
        if differs(p.time, r.time) || differs(p.value, r.value) || differs(p.left_slope, r.left_slope) || differs(p.right_slope, r.right_slope) {
            return 0;
        }
    }
    1
}

/// C's `printf("%g", v)`: six significant digits, trailing zeros removed, exponent form when
/// the decimal exponent is below −4 or at least 6. (`si-file` has the same routine; it is
/// repeated here so that this crate depends on nothing.)
pub fn format_g(v: f64) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if !v.is_finite() {
        return if v.is_nan() { "nan".into() } else if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let e = format!("{:.5e}", v);
    let (mantissa, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let strip = |s: &str| -> String {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() }
    };
    if !(-4..6).contains(&exp) {
        format!("{}e{}{:02}", strip(mantissa), if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        strip(&format!("{:.*}", (5 - exp) as usize, v))
    }
}
