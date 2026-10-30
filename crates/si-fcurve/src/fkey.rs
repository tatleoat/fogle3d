// SPDX-License-Identifier: MIT
//! Single keys and the two key clipboards: the functions of `fkey.c` (finding 0009).
//!
//! A key record in the original points to the curve whose array holds it (+0x10), and
//! `SI_fkeySetTime` and `SI_fkeySetValue` act on that curve when the pointer is set. Here a
//! key on its own is a [`Key`] and a key in a curve is addressed through the curve, so each
//! of those two functions appears twice.

use crate::edit::q;
use crate::eval::{frame_time, sec_to_frame};
use crate::x87::X87;
use crate::{ElementCurves, Env, Fcurve, Key};

fn xf(v: f32) -> X87 {
    X87::from_f32(v)
}

impl Key {
    /// A new key [soft 0x0820b248 SI_fkeyCreate]: the same slope on both sides.
    pub fn create(time: f32, value: f32, slope: f32, slope_type: i16, interp: i16) -> Key {
        let mut k = Key::new();
        k.set_time(time);
        k.set_value(value);
        k.right_slope = q(slope);
        k.left_slope = q(slope);
        k.slope_type = slope_type;
        k.interp = interp;
        k.field_20 = -1;
        k
    }

    /// A new key at a frame [soft 0x0820b1b0 SI_fkeyFrameCreate]: at the time
    /// `(float)frame / rate`.
    pub fn frame_create(frame: i32, value: f32, slope: f32, slope_type: i16, interp: i16, env: &Env) -> Key {
        Key::create(frame_time(frame, env).to_f32(), value, slope, slope_type, interp)
    }

    /// The frame the key is on [soft 0x0820b29c SI_fkeyFrame] (0 for a NULL key).
    pub fn frame(&self, env: &Env) -> i32 {
        sec_to_frame(self.time, env)
    }

    /// [soft 0x0820b318 SI_fkeySetTime], for a key that is in no curve.
    pub fn set_time(&mut self, time: f32) {
        self.time = q(time);
    }

    /// [soft 0x0820b354 SI_fkeySetValue], for a key that is in no curve.
    pub fn set_value(&mut self, value: f32) {
        self.value = q(value);
    }

    /// Time from a frame number and value, as the curve editor's coordinates give them
    /// [soft 0x0820b378 si_fkeySetVectorFrame]: the time is `x / rate`.
    pub fn set_vector_frame(&mut self, x: f32, y: f32, env: &Env) {
        self.time = (xf(x) / xf(env.frame_rate)).to_f32();
        self.value = q(y);
    }

    /// Time and value [soft 0x0820b398 fkeySetVector].
    pub fn set_vector(&mut self, x: f32, y: f32) {
        self.time = q(x);
        self.value = q(y);
    }
}

impl Fcurve {
    /// Sets the value of the key at array position `slot` and recomputes every slope of the
    /// curve [soft 0x0820b354 SI_fkeySetValue].
    pub fn key_set_value(&mut self, slot: usize, value: f32, env: &mut Env) {
        if let Some(k) = self.keys.as_mut().and_then(|k| k.get_mut(slot)) {
            k.value = q(value);
        }
        self.default_slope(env);
    }

    /// What [soft 0x0820b318 SI_fkeySetTime] does to a key that is in a curve, as far as it
    /// is defined. The original removes the key from its curve, then writes the time through
    /// the same address and inserts "the key at that address" again:
    ///
    /// - if the key was the last one, the address is the record just behind the shortened
    ///   array, which still holds the key: the key moves to its new time, as intended;
    /// - otherwise the following key has moved into that record: its time is overwritten,
    ///   and the insertion finds it as an existing key on that frame and rewrites it in
    ///   place. The key that was to be moved is gone.
    ///
    /// In both cases the original ends by passing the address, which lies inside the key
    /// array, to `free`; that has no counterpart here. (The points dialog of the curve
    /// editor calls it this way.)
    pub fn key_set_time(&mut self, slot: usize, time: f32, env: &mut Env) {
        let Some(removed) = self.k().get(slot).copied() else { return };
        self.remove_key(slot, env);
        let mut key = self.k().get(slot).copied().unwrap_or(removed);
        key.time = q(time);
        if let Some(k) = self.keys.as_mut().and_then(|k| k.get_mut(slot)) {
            k.time = key.time;
        }
        self.add_key(key, env);
    }

    /// Copies the keys, or with `tagged_only` 1 the tagged keys, to the key clipboard
    /// [soft 0x0820b3b0 si_copyFkeyToBuffer]. In the clipboard the times are relative to the
    /// first copied key, and tag, flags and spline index are reset. Returns the number of
    /// keys, 0 if there is nothing to copy (the clipboard then keeps what it had).
    ///
    /// With `tagged_only` neither 0 nor 1 the original sizes the clipboard for all keys but
    /// fills in only the tagged ones; the rest, uninitialised there, are blank keys here.
    pub fn copy_keys_to_buffer(&self, tagged_only: i32, env: &mut Env) -> i32 {
        if self.nkeys() <= 0 {
            return 0;
        }
        let mut count = self.k().len();
        if tagged_only == 1 {
            count = self.k().iter().filter(|k| k.tagged == 1).count();
            if count == 0 {
                return 0;
            }
        }
        let mut buffer: Vec<Key> = self.k().iter().filter(|k| tagged_only == 0 || k.tagged == 1).copied().collect();
        buffer.resize(count, Key::new());
        let first = xf(buffer[0].time);
        for k in buffer.iter_mut() {
            k.tagged = 0;
            k.flags = 0;
            k.spline_index = -1;
            k.time = (xf(k.time) - first).to_f32();
        }
        env.fkey_buffer = Some(buffer);
        count as i32
    }

    /// Adds the keys of the key clipboard at `time` and tags them
    /// [soft 0x0820b4f0 si_pasteFkeyToBuffer]. Each is added as [`Fcurve::add_key`] does it,
    /// warnings included. Returns the number of keys in the clipboard.
    pub fn paste_keys_from_buffer(&mut self, time: f32, env: &mut Env) -> i32 {
        let Some(buffer) = env.fkey_buffer.clone() else { return 0 };
        for k in &buffer {
            let key = Key { time: (xf(k.time) + xf(time)).to_f32(), ..*k };
            let index = self.add_key(key, env);
            if index >= 0 && index < self.nkeys() {
                self.keys.as_mut().unwrap()[index as usize].tagged = 1;
            }
        }
        buffer.len() as i32
    }

    /// Adds the key that the per-type clipboard holds for this curve's type, at `time`
    /// [soft 0x0820b950 pasteTimeFkeyFromBuffer]. Returns 0 if the clipboard has none.
    ///
    /// (The original takes the address of a curve pointer and allocates a curve if that is
    /// NULL; its three callers never pass one. It also indexes the clipboard without a range
    /// test; a type outside it has no key here.)
    pub fn paste_time_key_from_buffer(&mut self, time: f32, env: &mut Env) -> i32 {
        let Some(Some(k)) = usize::try_from(self.kind).ok().and_then(|i| env.fkey_time_buffer.get(i)).copied() else { return 0 };
        self.add_key(Key { time, ..k }, env);
        1
    }
}

/// Copies the key of `curve` within 0.01 second of `time` into slot `slot` of the per-type
/// clipboard, and with `remove` takes it out of the curve
/// [soft 0x0820b588 copyTimeFKeyToBuffer]. The slot is emptied first; 1 if a key was copied.
pub fn copy_time_key_to_buffer(curve: Option<&mut Fcurve>, slot: i32, time: f32, remove: i32, env: &mut Env) -> i32 {
    let Some(place) = usize::try_from(slot).ok().filter(|&s| s < crate::TIME_BUFFER_SLOTS) else { return 0 };
    env.fkey_time_buffer[place] = None;
    let Some(curve) = curve else { return 0 };
    let hit = curve.k().iter().position(|k| (xf(time) - xf(k.time)).abs().le(X87::from_f64(0.01)));
    let Some(i) = hit else { return 0 };
    env.fkey_time_buffer[place] = Some(curve.k()[i]);
    if remove != 0 {
        curve.remove_key_index(i as i32 + 1, env);
    }
    1
}

/// One curve after another: each one's key at `time` goes to the clipboard slot of the
/// curve's type. Returns how many had one.
fn copy_each(time: f32, remove: i32, env: &mut Env) -> impl FnMut(&mut Fcurve) -> i32 + '_ {
    move |c: &mut Fcurve| {
        let kind = c.kind;
        copy_time_key_to_buffer(Some(c), kind, time, remove, env)
    }
}

/// Copies (or with `remove` cuts) the keys a model has at `time`, one per animated
/// parameter [soft 0x0820b668 si_copyModelKeyToBuffer]: every curve of the element's groups
/// 0, 1 and 2. `element` is `None` where the original finds no element for the model or
/// `si_elemdictValidFcurve` refuses it; the result is then 0.
pub fn copy_model_key_to_buffer(element: Option<&mut dyn ElementCurves>, time: f32, remove: i32, env: &mut Env) -> i32 {
    let Some(element) = element else { return 0 };
    let mut total = 0;
    let mut copy = copy_each(time, remove, env);
    for group in 0..3 {
        element.each_in_range(group, 0, 14999, &mut |c| total += copy(c));
    }
    total
}

/// The same for a light [soft 0x0820b794 si_copyLightKeyToBuffer]: the curve 0x834 of group
/// 0 and the curves 0x83e to 0x858 of group 1.
pub fn copy_light_key_to_buffer(element: Option<&mut dyn ElementCurves>, time: f32, remove: i32, env: &mut Env) -> i32 {
    copy_one_and_range(element, 0x834, (0x83e, 0x858), time, remove, env)
}

/// The same for a camera [soft 0x0820b870 si_copyCameraKeyToBuffer]: the curve 0x44c of
/// group 0 and the curves 0x456 to 0x478 of group 1.
pub fn copy_camera_key_to_buffer(element: Option<&mut dyn ElementCurves>, time: f32, remove: i32, env: &mut Env) -> i32 {
    copy_one_and_range(element, 0x44c, (0x456, 0x478), time, remove, env)
}

fn copy_one_and_range(element: Option<&mut dyn ElementCurves>, single: i32, range: (i32, i32), time: f32, remove: i32,
                      env: &mut Env) -> i32 {
    let Some(element) = element else { return 0 };
    let mut total = 0;
    let mut copy = copy_each(time, remove, env);
    if let Some(c) = element.find(0, single) {
        total = copy(c);
    }
    element.each_in_range(1, range.0, range.1, &mut |c| total += copy(c));
    total
}

/// Pastes the per-type clipboard into a model's curves at `time`
/// [soft 0x0820b9bc si_pasteModelKeyFromBuffer]: for every curve of groups 0, 1 and 2, the
/// key held for its type. Only parameters that already have a curve receive a key. Returns
/// the number of keys pasted; `element` is `None` where the original finds no element.
pub fn paste_model_key_from_buffer(element: Option<&mut dyn ElementCurves>, time: f32, env: &mut Env) -> i32 {
    let Some(element) = element else { return 0 };
    let mut total = 0;
    for group in 0..3 {
        element.each_in_range(group, 0, 14999, &mut |c| total += c.paste_time_key_from_buffer(time, env));
    }
    total
}

/// [soft 0x0820bad4 si_pasteLightKeyFromBuffer]
pub fn paste_light_key_from_buffer(element: Option<&mut dyn ElementCurves>, time: f32, env: &mut Env) -> i32 {
    paste_one_and_range(element, 0x834, (0x83e, 0x858), time, env)
}

/// [soft 0x0820bb90 si_pasteCameraKeyFromBuffer]
pub fn paste_camera_key_from_buffer(element: Option<&mut dyn ElementCurves>, time: f32, env: &mut Env) -> i32 {
    paste_one_and_range(element, 0x44c, (0x456, 0x478), time, env)
}

fn paste_one_and_range(element: Option<&mut dyn ElementCurves>, single: i32, range: (i32, i32), time: f32, env: &mut Env) -> i32 {
    let Some(element) = element else { return 0 };
    let mut total = 0;
    if let Some(c) = element.find(0, single) {
        total = c.paste_time_key_from_buffer(time, env);
    }
    element.each_in_range(1, range.0, range.1, &mut |c| total += c.paste_time_key_from_buffer(time, env));
    total
}
