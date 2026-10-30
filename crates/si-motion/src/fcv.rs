// SPDX-License-Identifier: MIT
//! Function curves in the memory image: the bridge to `si-fcurve`.
//!
//! `si-fcurve` (finding 0009) ports the curve routines on its own record. A curve in the
//! image is the original's 100-byte `fcurve` with its array of 36-byte keys; the functions
//! here read one into an `si_fcurve::Fcurve`, run the ported routine and write back what
//! the original would have written.
//!
//! adapter: not in place. The curve routines themselves run on `si-fcurve`'s record, not on
//! the image (finding 0024, level 1); this bridge goes when that crate is on the image. What
//! the routines allocate is reproduced here call for call ([`store`], [`key_record`],
//! [`add_key`]) and measured: the image's heap after a routine of this crate is the
//! original's, block for block (finding 0017, section 12).

use crate::glob;
use crate::mem::MemExt;
use si_mem::{Addr, Mem};
use crate::off::{fcurve as fo, key as ko};
use si_fcurve::{Env, Fcurve, Key};
use si_x87::F80;

/// The globals `si-fcurve` reads, taken from the image.
pub fn env(m: &Mem) -> Env {
    let mut e = Env::new();
    e.frame_rate = m.f32(glob::SI_FRAME_RATE);
    e.frame_current = m.i32(glob::SI_FRAME_CURRENT);
    e.key_tolerance = m.f32(glob::SI_GLB_ANIM + glob::anim::KEY_TOLERANCE);
    e.default_interp = m.i16(glob::SI_GLB_ANIM + glob::anim::DEFAULT_INTERP);
    e.use_hidden_for_matte = m.i16(glob::SI_USE_HIDDEN_FOR_MATTE);
    e.standalone_renderer = m.i32(glob::SI_STANDALONE_RENDERER);
    e.update_option = m.i32(glob::SI_UPDATE_OPTION);
    e.key_type_propag_mode = m.i16(glob::SI_KEY_TYPE_PROPAG_MODE);
    e.cus_val_num_callback = m.i32(glob::SI_CUS_VAL_NUM_CALLBACK);
    e.fcurve_dirty = m.i32(glob::SI_FCURVE_DIRTY);
    e.cus_fcurve_dirty = m.i32(glob::SI_CUS_FCURVE_DIRTY);
    e
}

/// The texts of the curve code (`fcurve.c`) that reach this crate's hosts, at their addresses
/// in the binary's data (read from the calls in `SI_fcurveAddKeyAfter`, 0x081ff588).
pub mod text {
    use si_mem::Addr;
    /// "Overlapping keys, new one replaces old." (`SI_statusDisplayMessage(text, 0)` at
    /// 0x081ff75a).
    pub const OVERLAPPING: Addr = 0x0869_5120;
    /// The five arguments of the question about a locked key (`SI_dialogRequest2` at
    /// 0x081ff718): "Do you want to replace the locked key?", two empty lines, and the two
    /// buttons. A non-zero answer (16 bits) replaces the key.
    pub const LOCKED_KEY: [Addr; 5] = [0x0869_50e0, 0x0869_50d8, 0x0869_50d7, 0x0869_50d4, 0x0869_50d0];
}

/// Reads the key record at `a`.
pub fn load_key(m: &Mem, a: Addr) -> Key {
    Key {
        time: m.f32(a + ko::TIME),
        value: m.f32(a + ko::VALUE),
        right_slope: m.f32(a + ko::RIGHT_SLOPE),
        slope_type: m.i16(a + ko::SLOPE_TYPE),
        interp: m.i16(a + ko::INTERP),
        tagged: m.i16(a + ko::TAGGED),
        left_slope: m.f32(a + ko::LEFT_SLOPE),
        flags: m.u16(a + ko::FLAGS),
        spline_index: m.i16(a + ko::SPLINE_INDEX),
        field_20: m.i16(a + ko::FIELD_20),
    }
}

/// Writes a key record at `a`, with `curve` as its pointer back to the curve.
pub fn store_key(m: &mut Mem, a: Addr, k: &Key, curve: Addr) {
    m.set_f32(a + ko::TIME, k.time);
    m.set_f32(a + ko::VALUE, k.value);
    m.set_f32(a + ko::RIGHT_SLOPE, k.right_slope);
    m.set_i16(a + ko::SLOPE_TYPE, k.slope_type);
    m.set_i16(a + ko::INTERP, k.interp);
    m.set_ptr(a + ko::CURVE, curve);
    m.set_i16(a + ko::TAGGED, k.tagged);
    m.set_f32(a + ko::LEFT_SLOPE, k.left_slope);
    m.set_u16(a + ko::FLAGS, k.flags);
    m.set_i16(a + ko::SPLINE_INDEX, k.spline_index);
    m.set_i16(a + ko::FIELD_20, k.field_20);
}

/// Reads the curve at `a` with its keys. The key count is taken as the file readers bound
/// it: a count whose records do not all lie in the block the array is in yields the keys
/// that do (the original would read past the array).
pub fn load(m: &Mem, a: Addr) -> Fcurve {
    let keys_at = m.ptr(a + fo::KEYS);
    let n = m.i32(a + fo::NB_KEYS);
    let keys = if keys_at == 0 {
        None
    } else {
        let mut v = Vec::new();
        let mut i = 0;
        while i < n && m.in_block(keys_at.wrapping_add((i as u32).wrapping_mul(ko::SIZE)), ko::SIZE) {
            v.push(load_key(m, keys_at + i as u32 * ko::SIZE));
            i += 1;
        }
        Some(v)
    };
    let mut name = [0u8; 34];
    name.copy_from_slice(&{
        let b = m.bytes(a + fo::NAME, 34).to_vec();
        if b.len() == 34 { b } else { vec![0; 34] }
    });
    Fcurve {
        kind: m.i32(a + fo::KIND),
        owner: m.u32(a + fo::OWNER),
        keys,
        last_time: m.f32(a + fo::LAST_TIME),
        last_value: m.f32(a + fo::LAST_VALUE),
        pre_extrap: m.i16(a + fo::PRE_EXTRAP),
        post_extrap: m.i16(a + fo::POST_EXTRAP),
        flags: m.i16(a + fo::FLAGS),
        interp: m.i16(a + fo::INTERP),
        active: m.i16(a + fo::ACTIVE),
        range_start: m.f32(a + fo::RANGE_START),
        range_end: m.f32(a + fo::RANGE_END),
        active_copy: m.i16(a + fo::ACTIVE_COPY),
        name,
        cache: m.i32(a + fo::CACHE),
        aux: None,
        min_value: m.f32(a + fo::MIN_VALUE),
        max_value: m.f32(a + fo::MAX_VALUE),
        field_60: m.i16(a + fo::FIELD_60),
        dirty: m.i16(a + fo::DIRTY),
    }
}

/// Writes back everything of the curve record at `a` except the key array pointer and the
/// snapshot pointer, and the keys into the existing array when they still fit the block it
/// occupies. Otherwise the array is made as `SI_fcurveAddKeyAfter` (0x081ff588) makes it:
/// for a curve without one `SI_malloc` of eight records, else `SI_realloc` of the old one to
/// the count rounded up to a multiple of 8 (the C library's `realloc` as the image has it: a
/// new block with the old bytes, the old one freed).
pub fn store(m: &mut Mem, a: Addr, f: &Fcurve) {
    m.set_i32(a + fo::KIND, f.kind);
    m.set_u32(a + fo::OWNER, f.owner);
    m.set_f32(a + fo::LAST_TIME, f.last_time);
    m.set_f32(a + fo::LAST_VALUE, f.last_value);
    m.set_i16(a + fo::PRE_EXTRAP, f.pre_extrap);
    m.set_i16(a + fo::POST_EXTRAP, f.post_extrap);
    m.set_i16(a + fo::FLAGS, f.flags);
    m.set_i16(a + fo::INTERP, f.interp);
    m.set_i16(a + fo::ACTIVE, f.active);
    m.set_f32(a + fo::RANGE_START, f.range_start);
    m.set_f32(a + fo::RANGE_END, f.range_end);
    m.set_i16(a + fo::ACTIVE_COPY, f.active_copy);
    m.set_bytes(a + fo::NAME, &f.name);
    m.set_i32(a + fo::CACHE, f.cache);
    m.set_f32(a + fo::MIN_VALUE, f.min_value);
    m.set_f32(a + fo::MAX_VALUE, f.max_value);
    m.set_i16(a + fo::FIELD_60, f.field_60);
    m.set_i16(a + fo::DIRTY, f.dirty);
    let mut keys_at = m.ptr(a + fo::KEYS);
    match &f.keys {
        None => {
            if keys_at != 0 {
                m.free(keys_at);
            }
            m.set_ptr(a + fo::KEYS, 0);
            m.set_i32(a + fo::NB_KEYS, 0);
        }
        Some(keys) => {
            let need = keys.len() as u32 * ko::SIZE;
            if keys_at == 0 || !m.in_block(keys_at, need) {
                let cap = si_fcurve::capacity(keys.len() as i32).max(8) as u32;
                keys_at = if keys_at == 0 { m.si_malloc(cap * ko::SIZE) } else { m.si_realloc(keys_at, cap * ko::SIZE) };
                m.set_ptr(a + fo::KEYS, keys_at);
            }
            for (i, k) in keys.iter().enumerate() {
                store_key(m, keys_at + i as u32 * ko::SIZE, k, a);
            }
            m.set_i32(a + fo::NB_KEYS, keys.len() as i32);
        }
    }
}

/// What a `si-fcurve` routine tells the rest of the program while it runs (its `Host`), kept
/// in order so that it can be passed on to this crate's hosts afterwards.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `SI_fastSetDirty(NULL, -1, 1)`.
    FastSetDirty,
    /// `SI_statusDisplayMessage(text, kind)`.
    Status(String, i16),
    /// The question about a locked key (answered No).
    ReplaceLockedKey,
    /// `si_fcvScnglDelete(curve)`.
    ScnglDelete,
    /// `SI_fcurveSaveHistory(…)`.
    SaveHistory,
}

/// The collected calls, and the answer given to the question about a locked key.
struct Collect(std::rc::Rc<std::cell::RefCell<Vec<Event>>>, i16);

impl si_fcurve::Host for Collect {
    fn fast_set_dirty(&mut self) {
        self.0.borrow_mut().push(Event::FastSetDirty);
    }
    fn status_message(&mut self, text: &str, kind: i16) {
        self.0.borrow_mut().push(Event::Status(text.to_string(), kind));
    }
    fn replace_locked_key(&mut self) -> i16 {
        self.0.borrow_mut().push(Event::ReplaceLockedKey);
        self.1
    }
    fn scngl_delete(&mut self, _curve: &Fcurve) {
        self.0.borrow_mut().push(Event::ScnglDelete);
    }
    fn save_history(&mut self, _curve: &Fcurve) {
        self.0.borrow_mut().push(Event::SaveHistory);
    }
}

/// An environment for `si-fcurve` routines whose calls into the program are collected and
/// handed to this crate's host by [`Ops::flush`]: the curve routines run for real inside
/// the original, so the calls they make (the fast-playback cache is marked stale by nearly
/// every edit) are part of what a routine of this crate does.
pub struct Ops {
    pub env: Env,
    log: std::rc::Rc<std::cell::RefCell<Vec<Event>>>,
    /// The two counters of changed curves as the image had them when `env` last agreed
    /// with it.
    seen: (i32, i32),
}

impl Ops {
    pub fn new(m: &Mem) -> Ops {
        Ops::answering(m, 0)
    }

    /// As [`Ops::new`], with the answer the question about a locked key gets (non-zero:
    /// replace it).
    pub fn answering(m: &Mem, answer: i16) -> Ops {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut env = env(m);
        env.host = Box::new(Collect(log.clone(), answer));
        let seen = (env.fcurve_dirty, env.cus_fcurve_dirty);
        Ops { env, log, seen }
    }

    /// Writes to the image what the routines run so far added to the two counters of changed
    /// curves (`SI_fcurveDirty`, `SI_cusFcurveDirty`: the original increments them in place,
    /// and only while `SI_cusValNumCallback` is positive; finding 0009, "Dirty counting").
    pub fn sync(&mut self, m: &mut Mem) {
        for (at, now, seen) in
            [(glob::SI_FCURVE_DIRTY, self.env.fcurve_dirty, &mut self.seen.0), (glob::SI_CUS_FCURVE_DIRTY, self.env.cus_fcurve_dirty, &mut self.seen.1)]
        {
            let d = now.wrapping_sub(*seen);
            if d != 0 {
                let v = m.i32(at).wrapping_add(d);
                m.set_i32(at, v);
                *seen = now;
            }
        }
    }

    /// Whether the routine that ran put the question about a locked key.
    pub fn asked(&self) -> bool {
        self.log.borrow().contains(&Event::ReplaceLockedKey)
    }

    /// Passes the collected calls on, in order. The status texts are the curve code's own
    /// literals: the one of `SI_fcurveAddKeyAfter` is passed by its address in the binary,
    /// any other is put in a local for the call. The question about a locked key is put by
    /// [`add_key`], which needs the answer before the routine goes on; the undo record has no
    /// host method in this crate and is dropped.
    pub fn flush<H: crate::reset::ResetHost + ?Sized>(&mut self, m: &mut Mem, h: &mut H) {
        self.sync(m);
        let events: Vec<Event> = self.log.borrow_mut().drain(..).collect();
        for e in events {
            match e {
                Event::FastSetDirty => h.fast_set_dirty(m, 0, -1, 1),
                Event::Status(t, kind) if t == "Overlapping keys, new one replaces old." => {
                    crate::key::KeyHost::status_display_message(h, m, text::OVERLAPPING, kind as i32);
                }
                Event::Status(text, kind) => {
                    let mut b = text.into_bytes();
                    b.push(0);
                    let at = m.local(b.len() as u32);
                    m.set_bytes(at, &b);
                    crate::key::KeyHost::status_display_message(h, m, at, kind as i32);
                    m.release(b.len() as u32);
                }
                Event::ScnglDelete | Event::ReplaceLockedKey | Event::SaveHistory => {}
            }
        }
    }

    /// Runs `f` on the curve at `a` (loaded, then stored back) and passes its calls on.
    pub fn on<H: crate::reset::ResetHost + ?Sized, R>(m: &mut Mem, h: &mut H, a: Addr, f: impl FnOnce(&mut Fcurve, &mut Env) -> R) -> R {
        let mut o = Ops::new(m);
        let mut c = load(m, a);
        let r = f(&mut c, &mut o.env);
        store(m, a, &c);
        o.flush(m, h);
        r
    }
}

/// The block a key lives in between its creation and its insertion: `si_fkeyNew`
/// (0x0820bcc4) takes 36 bytes from the C library's `malloc` for `SI_fkeyCreate` and
/// `SI_fkeyFrameCreate`, and the insertion (or the routine that did not insert the key)
/// frees them. The key is stored there as the creation leaves it; the block's pointer back
/// to a curve is not set, and nothing reads the block after the insertion (its bytes are
/// not measured).
pub fn key_record(m: &mut Mem, key: &Key) -> Addr {
    let record = m.malloc(ko::SIZE);
    if record != 0 {
        store_key(m, record, key, 0);
    }
    record
}

/// `SI_fcurveAddKey(curve, key)` (0x081ff400, ported in `si-fcurve`) on the curve at `a`,
/// with what the original does around the insertion:
///
/// - the **key record**: the original's key is a 36-byte block that `si_fkeyNew`
///   (0x0820bcc4) takes from the C library's `malloc` when the caller creates the key
///   (`SI_fkeyCreate`, `SI_fkeyFrameCreate`) and that the insertion frees
///   (`SI_fcurveInitKey`, or `SI_fcurveAddKeyAfter` itself when the key is refused). Every
///   caller in this crate creates its key immediately before adding it, so the block is
///   taken and freed here: the blocks allocated after it then lie where the original's lie;
/// - a **first key** always goes into a new array of eight records: the pointer of an array
///   that is there but empty (keys removed one by one) is overwritten, the old block neither
///   used nor freed (finding 0009, "Oddities"; measured in finding 0017);
/// - a **locked key** on the frame of the new one makes the original ask
///   `SI_dialogRequest2("Do you want to replace the locked key?", …)` before anything else
///   changes; No leaves the curve as it is and returns −1;
/// - the curve code's own calls (the fast-playback cache marked stale, the status line's
///   "Overlapping keys, new one replaces old.") and the counters of changed curves.
///
/// Returns the key's index.
pub fn add_key<H: crate::reset::ResetHost + ?Sized>(m: &mut Mem, h: &mut H, a: Addr, key: Key) -> i32 {
    let record = key_record(m, &key);
    if nb_keys(m, a) == 0 {
        m.set_ptr(a + fo::KEYS, 0);
    }
    // the question is the first thing the insertion does when it meets a locked key, so it
    // is run with the answer No first and, after a Yes, again from the start
    let mut o = Ops::new(m);
    let mut c = load(m, a);
    let mut r = c.add_key(key, &mut o.env);
    if o.asked() {
        let t = text::LOCKED_KEY;
        if h.dialog_request2(m, t[0], t[1], t[2], t[3], t[4]) != 0 {
            o = Ops::answering(m, 1);
            c = load(m, a);
            r = c.add_key(key, &mut o.env);
        }
    }
    store(m, a, &c);
    m.free(record);
    o.flush(m, h);
    r
}

/// Makes an activation curve read `on` from `t0` to `t1`
/// [soft 0x08202c08 si_fcurveActiveRangeSet], on a curve of the image.
///
/// `si-fcurve` has this routine on its own record. It is put together here again from the
/// same pieces (`SI_fcurveIntegerValue`, `SI_fcurveRemoveKeyIndex`, `SI_fkeyCreate`,
/// `SI_fcurveAddKey`), each run on the image, because the history of the key array shows in
/// the image and not in a list of keys: the routine can take the last key out and then add
/// one, which in the original goes into a **new** array ([`add_key`]), and the records
/// behind the key count keep what the removals left there.
///
/// The state just before `t0` (at `t0 − 0.0001`, the double at 0x0864d2f0) and at `t1` are
/// read as flags. The keys with `t0 ≤ time ≤ t1` are removed. A key with the new state is
/// added at `t0` unless the curve read that state before it; a key with the opposite state
/// at `t1` when the curve did not read `on` there (`on` compared as the 16-bit number it is),
/// or, when switching off, when fewer than two keys were there to begin with. Last, every key
/// whose value equals both its neighbours' is removed.
pub fn active_range_set<H: crate::reset::ResetHost + ?Sized>(m: &mut Mem, h: &mut H, a: Addr, t0: f32, t1: f32, on: i16) {
    let inside: f32 = if on != 0 { 1.0 } else { 0.0 };
    let outside: f32 = if on != 0 { 0.0 } else { 1.0 };
    let before = integer_value(m, a, (F80::from_f32(t0) - 0.0001f64).to_f32(), 0, 1);
    let at_end = integer_value(m, a, t1, 0, 1);
    let need_end = at_end != on as i32 || (nb_keys(m, a) < 2 && F80::ZERO.lt(F80::from_f32(outside)));
    let key_at = |m: &Mem, i: i32| m.ptr(a + fo::KEYS).wrapping_add((i as u32).wrapping_mul(ko::SIZE));
    let mut i = 0;
    while i < nb_keys(m, a) {
        let kt = F80::from_f32(m.f32(key_at(m, i) + ko::TIME));
        if kt.lt(F80::from_f32(t0)) || !kt.le(F80::from_f32(t1)) {
            i += 1;
        } else {
            Ops::on(m, h, a, |f, env| f.remove_key_index(i + 1, env));
        }
    }
    if !F80::from_f32(inside).eq(F80::from_i32(before)) {
        let key = Key::create(t0, inside, 0.0, 0, m.i16(a + fo::INTERP));
        add_key(m, h, a, key);
    }
    if need_end {
        let key = Key::create(t1, outside, 0.0, 0, m.i16(a + fo::INTERP));
        add_key(m, h, a, key);
    }
    let mut i = 1;
    while i < nb_keys(m, a) {
        let mut next = i + 1;
        if next < nb_keys(m, a) {
            let v = F80::from_f32(m.f32(key_at(m, i) + ko::VALUE));
            if v.eq(F80::from_f32(m.f32(key_at(m, i - 1) + ko::VALUE))) && F80::from_f32(m.f32(key_at(m, i + 1) + ko::VALUE)).eq(v) {
                Ops::on(m, h, a, |f, env| f.remove_key_index(next, env));
                next = i;
            }
        }
        i = next;
    }
}

/// Opens (`insert` non-zero) or closes gaps of `gap` frames at every `step` frames from
/// `first` to `last` [soft 0x08200094 si_fcvFrameStretch], on a curve of the image.
///
/// `si-fcurve` has this routine on its own record. It is put together here again from the
/// same pieces (`SI_fcurveRemoveKeyBetweenTimes`, `SI_fcvMoveKeyTime`,
/// `SI_fcurveDefaultSlope`), each run on the image, for the reason [`active_range_set`]
/// is: closing a gap first moves keys and then removes some, and the records behind the key
/// count keep the times the keys had **when they were removed**, not the ones the curve
/// began with (measured through `SI_elemdictActiveFcvOper`: 4 of 200 cases differ when the
/// whole routine runs on a list of keys and is stored once).
///
/// Nothing for a null curve, a curve without key array or keys, or `last <= first`
/// (returns 0). Else, with gap' = `gap` (negated when closing) and
/// dt = `(float)gap' / rate` kept in the register: for j = 0 … (last − first) / step, with
/// T = `(float)((step + gap')·j + first) / rate` in the register, closing first removes the
/// keys strictly between `(float)(T − 1e-5)` and `(float)((T − dt) − 1e-5)`; then the keys
/// from `(float)T` up to the time the last key had before the removal move by `(float)dt`.
/// At the end all slopes are recomputed. Returns 1.
///
/// The original divides by `step` without a test: a step of zero (or the one quotient that
/// does not fit) stops it there; here that is a recorded fault ([`MemExt::crash`]). When
/// the removals have emptied the curve the original takes "the last key's time" from in
/// front of the array; neither call then does anything with it, and here it is 0.
#[allow(clippy::too_many_arguments)]
pub fn frame_stretch<H: crate::reset::ResetHost + ?Sized>(m: &mut Mem, h: &mut H, a: Addr, first: i32, last: i32, gap: i32, step: i32, insert: i16)
                                                         -> i32 {
    if a == 0 || m.ptr(a + fo::KEYS) == 0 || nb_keys(m, a) == 0 || last <= first {
        return 0;
    }
    let gap = if insert == 0 { gap.wrapping_neg() } else { gap };
    let rate = m.f32(glob::SI_FRAME_RATE);
    let time_of = |frame: i32| F80::from_f32(F80::from_i32(frame).to_f32()) / rate;
    let dt = time_of(gap);
    let Some(quotient) = last.wrapping_sub(first).checked_div(step) else {
        m.crash();
        return 0;
    };
    let count = quotient.wrapping_add(1);
    let eps = F80::from_f64(1e-5);
    let mut j = 0i32;
    while j < count {
        let t = time_of(step.wrapping_add(gap).wrapping_mul(j).wrapping_add(first));
        let n = nb_keys(m, a);
        let last_time = if n > 0 { si_x87::fmov(m.f32(m.ptr(a + fo::KEYS).wrapping_add((n as u32 - 1).wrapping_mul(ko::SIZE)) + ko::TIME)) } else { 0.0 };
        if insert == 0 {
            let upper = ((t - dt) - eps).to_f32();
            let lower = (t - eps).to_f32();
            Ops::on(m, h, a, |f, env| f.remove_key_between_times(0, lower, upper, 0, env));
        }
        Ops::on(m, h, a, |f, env| f.move_key_time(t.to_f32(), last_time, dt.to_f32(), 0, env));
        j += 1;
    }
    Ops::on(m, h, a, |f, env| f.default_slope(env));
    1
}

/// `si-fcurve`'s register value as this crate's.
pub fn f80(v: si_fcurve::X87) -> F80 {
    let b = v.to_bytes();
    F80::from_bits(u64::from_le_bytes(b[..8].try_into().unwrap()), u16::from_le_bytes([b[8], b[9]]))
}

fn store_eval_state(m: &mut Mem, a: Addr, f: &Fcurve) {
    m.set_f32(a + fo::LAST_TIME, f.last_time);
    m.set_f32(a + fo::LAST_VALUE, f.last_value);
    m.set_i32(a + fo::CACHE, f.cache);
}

/// `SI_fcurveValue(curve, time, check)` (0x082002dc; ported in `si-fcurve`, finding 0009)
/// on the curve at `a`: the value in the register, unrounded where the original leaves it
/// so, and the evaluation state (+0x10, +0x14, +0x50) written back.
///
/// For the inputs on which the original's result is undefined (finding 0009, "Inputs with
/// no defined result") this returns 0. What the original stored **before** it read memory
/// that is not the curve's is stored here too: the time it evaluated at (+0x10) and the
/// search index as far as the search got (+0x50; −1 when it took the record in front of the
/// array for a segment's start). `si-fcurve` leaves exactly that in its record when it
/// answers "undefined". Until the sixth pass this bridge dropped the two stores (four of
/// the seven differences the checks had counted since the first pass).
pub fn value(m: &mut Mem, a: Addr, time: f32, check: i16) -> F80 {
    let mut e = env(m);
    let mut f = load(m, a);
    let r = f.value(time, check, &mut e);
    store_eval_state(m, a, &f);
    r.map_or(F80::ZERO, f80)
}

/// `SI_fcurveIntegerValue(curve, time, check, as_flag)` (0x08200404) on the curve at `a`.
/// Undefined inputs give 0, with the stores the original made before them, as in [`value`].
pub fn integer_value(m: &mut Mem, a: Addr, time: f32, check: i16, as_flag: i16) -> i32 {
    let mut e = env(m);
    let mut f = load(m, a);
    let r = f.integer_value(time, check, as_flag, &mut e);
    store_eval_state(m, a, &f);
    r.unwrap_or(0)
}

/// `SI_fcurveNextKey(curve, frame, &out)` (0x08202a58, ported in `si-fcurve`) on the curve
/// at `a`: the frame of the first key after `frame` that is not a key of type 3. `None`
/// where the original returns 0 and leaves its output alone: no such key, or a null curve.
pub fn next_key(m: &Mem, a: Addr, frame: i32) -> Option<i32> {
    if a == 0 {
        return None;
    }
    load(m, a).next_key(frame, &env(m))
}

/// `SI_fcurvePreviousKey(curve, frame, &out)` (0x08202b2c): the last such key before `frame`.
pub fn previous_key(m: &Mem, a: Addr, frame: i32) -> Option<i32> {
    if a == 0 {
        return None;
    }
    load(m, a).previous_key(frame, &env(m))
}

/// Number of keys of the curve at `a` (+0x0c).
pub fn nb_keys(m: &Mem, a: Addr) -> i32 {
    m.i32(a + fo::NB_KEYS)
}

/// The curve's type (+0x00): which parameter it animates.
pub fn kind(m: &Mem, a: Addr) -> i32 {
    m.i32(a + fo::KIND)
}

/// Test support: builds a curve with the given keys (time, value) in the image and returns
/// its address. Linear interpolation, constant extrapolation, active.
pub fn make(m: &mut Mem, kind: i32, interp: i16, keys: &[(f32, f32)]) -> Addr {
    let a = m.si_calloc(1, fo::SIZE);
    let mut e = env(m);
    e.default_interp = interp;
    let mut f = Fcurve::allocate(kind, 0, &e);
    f.interp = interp;
    if !keys.is_empty() {
        f.keys = Some(
            keys.iter()
                .map(|&(t, v)| {
                    let mut k = Key::new();
                    k.time = t;
                    k.value = v;
                    k.interp = interp;
                    k
                })
                .collect(),
        );
    }
    store(m, a, &f);
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_evaluate() {
        let mut m = Mem::new();
        glob::init_defaults(&mut m);
        let a = make(&mut m, 1, 1, &[(0.0, 1.0), (1.0, 3.0)]);
        assert_eq!(nb_keys(&mut m, a), 2);
        assert_eq!(kind(&mut m, a), 1);
        let v = value(&mut m, a, 0.5, 1);
        assert_eq!(v.to_f32(), 2.0);
        assert_eq!(m.f32(a + fo::LAST_TIME), 0.5);
        assert_eq!(m.f32(a + fo::LAST_VALUE), 2.0);
        let f = load(&mut m, a);
        assert_eq!(f.k().len(), 2);
        assert_eq!(m.ptr(m.ptr(a + fo::KEYS) + ko::CURVE), a);
        assert!(m.fault().is_none());
    }

    #[test]
    fn integer_curve() {
        let mut m = Mem::new();
        glob::init_defaults(&mut m);
        let a = make(&mut m, 175, 0, &[(0.0, 0.0), (1.0, 1.0)]);
        assert_eq!(integer_value(&mut m, a, 0.5, 1, 1), 0);
        assert_eq!(integer_value(&mut m, a, 1.5, 1, 1), 1);
    }
}
