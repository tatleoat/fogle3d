// SPDX-License-Identifier: MIT
//! Tags, locks and key types as the curve editor and the menus set them: the functions of
//! `fcurve.c` that walk the editor's list of curves or an element's curves (finding 0009).
//!
//! The list, the pointer event, the projection of a curve to the screen and the hit test
//! belong to other source files. Each function here takes what the original gets from them:
//!
//! - `curves`: the curves of the list nodes the original accepts, in list order. A node is
//!   accepted when its type word (+0) is 9 (a curve node), its word at +8 is non-zero, and
//!   (except for the two single-key functions) its curve has at least one key;
//! - `hit(i, curve)`: for the pointer functions, what `SI_hitProjDst` returns for curve
//!   number `i` after `SI_grafnodeInitProjection`: the index of the projected point nearest
//!   the pointer and its distance. A curve projects three points per key, the key first;
//! - `points(i, curve)`: for the rectangle functions, the projected points of curve number
//!   `i` as screen coordinates (x, y).

use crate::keyflag::{self, locked};
use crate::{ElementCurves, Env, Fcurve};

/// The three operations of the rectangle functions, by menu code.
const RECT_TOGGLE: i16 = 0x65;
const RECT_CLEAR: i16 = 0x66;
const RECT_SET: i16 = 0x67;

/// The screen rectangle `SI_screenRectangleGet` returns: x from `x0` to `x1`, y from `y0` to
/// `y1`, bounds included.
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x0: i32,
    pub x1: i32,
    pub y0: i32,
    pub y1: i32,
}

impl Rect {
    fn contains(&self, (x, y): (i32, i32)) -> bool {
        self.x0 <= x && x <= self.x1 && self.y0 <= y && y <= self.y1
    }
}

/// Tags every key of every listed curve with `tag`, or with `toggle` inverts every tag
/// [soft 0x08201634 si_fcurveSetAllKeyTags]. An undo record is made for each curve first.
pub fn set_all_key_tags(curves: &mut [&mut Fcurve], tag: i16, toggle: i16, env: &mut Env) {
    for c in curves.iter_mut() {
        if c.nkeys() <= 0 {
            continue;
        }
        env.host.save_history(c);
        for k in c.keys.iter_mut().flatten() {
            k.tagged = if toggle == 0 { tag } else { (k.tagged == 0) as i16 };
        }
    }
}

/// The key a hit belongs to: `Ok(None)` for no hit, `Err(())` where the original abandons
/// the whole walk (the nearest point is a slope handle, not a key).
fn hit_key(index: i32, distance: i32) -> Result<Option<usize>, ()> {
    if index < 0 || distance == 0x7fff_ffff {
        return Ok(None);
    }
    if index % 3 != 0 {
        return Err(());
    }
    Ok(Some(index as usize / 3))
}

/// Tags the key under the pointer with `tag`, or with `toggle` inverts its tag
/// [soft 0x08201710 si_fcurveSetKeyTags], in each listed curve in turn; stops at the first
/// curve whose nearest point is not a key. An undo record is made for every curve reached.
pub fn set_key_tags(curves: &mut [&mut Fcurve], hit: &mut dyn FnMut(usize, &Fcurve) -> (i32, i32), tag: i16, toggle: i16,
                    env: &mut Env) {
    for (i, c) in curves.iter_mut().enumerate() {
        env.host.save_history(c);
        let (index, distance) = hit(i, c);
        let Ok(key) = hit_key(index, distance) else { return };
        if let Some(k) = key.and_then(|k| c.keys.as_mut().and_then(|keys| keys.get_mut(k))) {
            k.tagged = if toggle == 0 { tag } else { (k.tagged == 0) as i16 };
        }
    }
}

/// Tags (`mode` 0x67), untags (0x66) or inverts the tag (0x65) of the keys inside the
/// rectangle [soft 0x08201854 SI_fcurveRectSetKeyTags]. An undo record is made for each
/// curve.
pub fn rect_set_key_tags(curves: &mut [&mut Fcurve], rect: Rect, points: &mut dyn FnMut(usize, &Fcurve) -> Vec<(i32, i32)>,
                         mode: i16, env: &mut Env) {
    for (i, c) in curves.iter_mut().enumerate() {
        if c.nkeys() <= 0 {
            continue;
        }
        env.host.save_history(c);
        let pts = points(i, c);
        for (k, p) in c.keys.iter_mut().flatten().zip(pts.iter().step_by(3)) {
            if rect.contains(*p) {
                match mode {
                    RECT_CLEAR => k.tagged = 0,
                    RECT_TOGGLE => k.tagged = (k.tagged == 0) as i16,
                    RECT_SET => k.tagged = 1,
                    _ => {}
                }
            }
        }
    }
}

/// Locks or unlocks every key of every listed curve
/// [soft 0x08201acc si_fcurveSetAllKeyLocks].
///
/// Without `toggle`: a non-zero `lock` becomes the key type of every key; zero unlocks the
/// locked keys and leaves path and raw keys alone. With `toggle`: locked keys become
/// free-form, all others (path and raw keys too) locked.
pub fn set_all_key_locks(curves: &mut [&mut Fcurve], lock: i16, toggle: i16) {
    for k in curves.iter_mut().flat_map(|c| c.keys.iter_mut().flatten()) {
        if toggle == 0 {
            if lock != 0 || k.key_type() == keyflag::KEY_FRAME {
                k.flags = k.flags & 0x70 | lock as u16;
            }
        } else if k.key_type() == keyflag::KEY_FRAME {
            k.flags &= 0x70;
        } else {
            k.flags = locked(k.flags);
        }
    }
}

/// Sets the key type of the key under the pointer to `lock`, or with `toggle` switches it
/// between free-form and locked [soft 0x08201c04 si_fcurveSetKeyLocks]; like
/// [`set_key_tags`] it stops at the first curve whose nearest point is not a key. Here a
/// toggle frees a key of any type other than free-form.
pub fn set_key_locks(curves: &mut [&mut Fcurve], hit: &mut dyn FnMut(usize, &Fcurve) -> (i32, i32), lock: i16, toggle: i16) {
    for (i, c) in curves.iter_mut().enumerate() {
        let (index, distance) = hit(i, c);
        let Ok(key) = hit_key(index, distance) else { return };
        if let Some(k) = key.and_then(|k| c.keys.as_mut().and_then(|keys| keys.get_mut(k))) {
            k.flags = if toggle == 0 {
                k.flags & 0x70 | lock as u16
            } else if k.key_type() != 0 {
                k.flags & 0x70
            } else {
                locked(k.flags)
            };
        }
    }
}

/// Locks (`mode` 0x67), frees (0x66) or toggles (0x65) the keys inside the rectangle
/// [soft 0x08201d5c si_fcurveRectSetKeyLocks].
pub fn rect_set_key_locks(curves: &mut [&mut Fcurve], rect: Rect, points: &mut dyn FnMut(usize, &Fcurve) -> Vec<(i32, i32)>,
                          mode: i16) {
    for (i, c) in curves.iter_mut().enumerate() {
        if c.nkeys() <= 0 {
            continue;
        }
        let pts = points(i, c);
        for (k, p) in c.keys.iter_mut().flatten().zip(pts.iter().step_by(3)) {
            if rect.contains(*p) {
                match mode {
                    RECT_CLEAR => k.flags &= 0x70,
                    RECT_TOGGLE => k.flags = if k.key_type() == 0 { locked(k.flags) } else { k.flags & 0x70 },
                    RECT_SET => k.flags = locked(k.flags),
                    _ => {}
                }
            }
        }
    }
}

/// Applies a key-type change to one curve: to all its keys, or to those on one frame.
fn apply_key_type(c: &mut Fcurve, flags: i16, at_time: bool, time: f32, env: &Env) {
    if at_time {
        c.set_time_key_type(flags as i32, time, env);
    } else {
        c.set_key_type(flags as i32);
    }
}

/// Sets the key type of a model's transformation keys
/// [soft 0x082025dc si_setModelAnimFcurveKeyType]: the curves of group 1 with the ids 0x64,
/// 0x65, 0x70 and 0x71 (the table `tabrel`); all their keys, or with `at_time` equal to 1
/// only those on the frame of `time`.
///
/// Type 2 (path key) on a model that has a path is handed to `si_keySplineToFcurve` for the
/// curve 0x70 instead; after any other change the path is checked against that curve.
/// `element` is `None` where the original finds no element for the model.
pub fn set_model_anim_fcurve_key_type(element: Option<&mut dyn ElementCurves>, key_type: i16, at_time: i16, time: f32,
                                      env: &Env) {
    let Some(element) = element else { return };
    if key_type == 2 && element.has_path(0) {
        element.key_spline_to_fcurve(0x70, 0);
        return;
    }
    for id in [0x64, 0x65, 0x70, 0x71] {
        element.each(1, id, &mut |c| apply_key_type(c, key_type, at_time == 1, time, env));
    }
    if element.has_path(0) {
        element.check_spline_to_fcurve(0x70, 0);
    }
}

/// The same for a camera [soft 0x082026f8 si_setCameraAnimFcurveKeyType]: the curves 0x458
/// to 0x45b of group 1; its two paths go with the curves 0x458 and 0x45a. Here any non-zero
/// `at_time` selects the keys of one frame.
pub fn set_camera_anim_fcurve_key_type(element: Option<&mut dyn ElementCurves>, key_type: i16, at_time: i16, time: f32,
                                       env: &Env) {
    set_two_path_key_type(element, [0x458, 0x45a], (0x458, 0x45b), key_type, at_time, time, env)
}

/// The same for a light [soft 0x08202868 si_setLightAnimFcurveKeyType]: the curves 0x840 to
/// 0x843 of group 1; its two paths go with the curves 0x840 and 0x842.
pub fn set_light_anim_fcurve_key_type(element: Option<&mut dyn ElementCurves>, key_type: i16, at_time: i16, time: f32,
                                      env: &Env) {
    set_two_path_key_type(element, [0x840, 0x842], (0x840, 0x843), key_type, at_time, time, env)
}

fn set_two_path_key_type(element: Option<&mut dyn ElementCurves>, path_ids: [i32; 2], range: (i32, i32), key_type: i16,
                         at_time: i16, time: f32, env: &Env) {
    let Some(element) = element else { return };
    if key_type == 2 {
        for (which, id) in path_ids.into_iter().enumerate() {
            if element.has_path(which) {
                element.key_spline_to_fcurve(id, which);
            }
        }
        return;
    }
    element.each_in_range(1, range.0, range.1, &mut |c| apply_key_type(c, key_type, at_time != 0, time, env));
    for (which, id) in path_ids.into_iter().enumerate() {
        if element.has_path(which) {
            element.check_spline_to_fcurve(id, which);
        }
    }
}
