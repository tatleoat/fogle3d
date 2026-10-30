// SPDX-License-Identifier: MIT
//! `actions.c`, third pass: the walk over an element's hierarchy that hands every
//! function curve to a per-curve step (`hieranimFcvProcess`), and the steps themselves
//! (`haFcvProcess…`). Almost every editing routine of an action list goes through it: an
//! action's keys are taken out of the curves when it is deactivated, put back when it is
//! activated, moved, scaled, cycled, locked or given anchor keys, one curve at a time.
//!
//! A step is called through a function pointer the caller hands the walk (the original's
//! code address). [`fcv_step`] runs the port of a step this crate has; any other address
//! goes to [`Outside::call_addr`].
//!
//! Records, by named offsets (finding 0030, 2.4): a function curve's +0 is its kind (0xc4
//! is skipped by the interpolation and anchor steps), +8 its keys (0x24 bytes each: +0 the
//! time in seconds, +4 the value, +0xe a 16-bit interpolation), +0xc their number. An
//! element of a dictionary: +4 its data, +8 its graf node, +0x54 a word that is 0 for one in
//! use. The model in an element's data: +4 its name, +0x1e a 16-bit kind, +0x30 a word,
//! +0x2bc a model it belongs to.

use crate::actions::{self as ac, ActionsElem, TIME_EPS};
use crate::globals as g;
use crate::outside::{self as out, Outside};
use si_mem::{Mem, Ptr32};
use si_x87::{fmov, x, Round, F80};

/// The code addresses of the per-curve steps (what the callers hand the walk).
pub mod step {
    pub const SAVE_HISTORY: u32 = 0x081e2960;
    pub const REMOVE: u32 = 0x081e29c4;
    pub const CHECK_EMPTY: u32 = 0x081e29f4;
    pub const MOVE: u32 = 0x081e2a14;
    pub const SCALE: u32 = 0x081e2b20;
    pub const SET_KEYS_ON_FRAMES: u32 = 0x081e2b60;
    pub const ACTION_DEACT: u32 = 0x081e4b3c;
    pub const ADD_ANCHOR_KEYS: u32 = 0x081e5c78;
    pub const SET_INTERP: u32 = 0x081e60d8;
    pub const UPDATE_INTERP: u32 = 0x081e6220;
    pub const CYCLE: u32 = 0x081e66d4;
    pub const KEY_LOCKS: u32 = 0x081e67c4;
    pub const LAST_KEY_TIME: u32 = 0x081e72bc;
}

/// The size of a key of a function curve.
const KEY: u32 = 0x24;

/// `fn(fcv, data, which)`: a step the walk was handed, by its code address.
pub fn fcv_step(m: &mut Mem, o: &mut dyn Outside, f: u32, fcv: u32, data: u32, which: i32) {
    match f {
        step::SAVE_HISTORY => {
            ha_fcv_process_save_history(m, o, fcv);
        }
        step::REMOVE => {
            ha_fcv_process_remove(m, o, fcv, data);
        }
        step::CHECK_EMPTY => ha_fcv_process_check_empty(m, fcv, data),
        step::MOVE => ha_fcv_process_move(m, o, fcv, data),
        step::SCALE => ha_fcv_process_scale(m, o, fcv, data),
        step::SET_KEYS_ON_FRAMES => ha_fcv_process_set_keys_on_frames(m, o, fcv, data),
        step::ACTION_DEACT => ha_fcv_process_action_deact(m, o, fcv, Ptr32::new(data)),
        step::ADD_ANCHOR_KEYS => ha_fcv_process_add_anchor_keys(m, o, fcv, Ptr32::new(data), which),
        step::SET_INTERP => ha_fcv_process_set_interp(m, fcv, Ptr32::new(data)),
        step::UPDATE_INTERP => ha_fcv_process_update_interp(m, fcv, Ptr32::new(data)),
        step::CYCLE => ha_fcv_process_cycle(m, o, fcv, Ptr32::new(data), which),
        step::KEY_LOCKS => ha_fcv_process_key_locks(m, o, fcv, data),
        step::LAST_KEY_TIME => ha_fcv_process_last_key_time(m, fcv, data),
        _ => {
            o.call_addr(m, f, &[fcv, data, which as u32]);
        }
    }
}

/// [soft 0x081e2bd8 getPosCnsFromElm]
///
/// The relation by which an element of a chapter is constrained to a path, through `out`:
/// 0x6f for a model (`SI_cid[8]`) or chapter `SI_cid[28]`; 0x45c for an element of
/// `SI_cid[1]` in use (its +0x54 is 0), else 0x457; for `SI_cid[6]` 0x844 when the kind at
/// +0x1e of the element's data is 2 and the element is in use, else 0x83f. 1, or 0 (and
/// nothing stored) for any other chapter.
pub fn get_pos_cns_from_elm(m: &mut Mem, chapter: i32, elem: u32, outp: u32) -> i32 {
    let cid = |m: &Mem, i: usize| m[g::SI_CID][i];
    let v = if chapter == cid(m, 8) || chapter == cid(m, 0x1c) {
        0x6f
    } else if chapter == cid(m, 1) {
        if m.u32(elem.wrapping_add(0x54)) != 0 {
            0x457
        } else {
            0x45c
        }
    } else if chapter == cid(m, 6) {
        let d = m.u32(elem.wrapping_add(4));
        if m.u16(d.wrapping_add(0x1e)) == 2 && m.u32(elem.wrapping_add(0x54)) == 0 {
            0x844
        } else {
            0x83f
        }
    } else {
        return 0;
    };
    m.set_u32(outp, v);
    1
}

/// [soft 0x081e2c5c hieranimFcvProcess]
///
/// Hands every function curve of an element's hierarchy to `f(fcv, data, which)`:
///
/// 1. the curves related to the element in chapter `SI_cid[0]` by relations 0 … 0x3a97 of
///    its forward lists 0, 1 and 2 (`SI_elemdictFindRangeRelatedElem` and `…Next…`; in list
///    0 those of relation 0x2af8, or 0x3a98 … 0x3aa2 as the relation word out says, are
///    left out), with `which` −1. With `recurse` set and no graf node (+8), or one whose +8
///    is 0, the element's own curves are skipped and the walk goes on to its sons;
/// 2. the path curves: the relation `getPosCnsFromElm` names for `chapter` (−1 from here on
///    when it names none) leads, in chapter `SI_cid[8]`, to models whose name starts with
///    `PATH_` and whose word at +0x30 is 0; for a model element (`chapter` `SI_cid[8]`)
///    only a path that does not belong to the element's model (+0x2bc, or, when that is 0,
///    is not the model itself). Each such path's curve of type 0x70 (`SI_edFindFcv`) and,
///    when it has one, the curve of the relation entry (`SI_elemdictGetRelation`,
///    `SI_edGetFcvFromRelation`) go to `f` with the index the relation answered. For
///    `SI_cid[1]` and `SI_cid[6]` elements in use the search is made a second time with
///    relation 0x457 or 0x83f (for `SI_cid[6]` only when the kind at +0x1e is 2);
/// 3. for a model element: the element related by 0xc8 in `SI_cid[21]`, those by 0x2bc in
///    `SI_cid[2]`, by 0xd2 and by 0xd7 in `SI_cid[5]`, each walked with `recurse` 0;
/// 4. the sons (`SI_edFindSon`, `SI_edFindNextSon`), with the chapter as it now is and the
///    same `recurse`.
#[allow(clippy::too_many_arguments)]
pub fn hieranim_fcv_process(m: &mut Mem, o: &mut dyn Outside, dict: u32, f: u32, elem: u32, data: u32, chapter: i32, recurse: i16) {
    let sp = m.sp();
    let (l4, l8, lc, l10, l14) = (m.local(4), m.local(4), m.local(4), m.local(4), m.local(4));
    let mut chapter = chapter;
    let mut count: i16 = 2;
    let cid = |m: &Mem, i: usize| m[g::SI_CID][i] as u32;
    let range = |m: &mut Mem, o: &mut dyn Outside, next: bool, group: u32| {
        let r = if next { &out::SI_ELEMDICT_FIND_RANGE_NEXT_RELATED_ELEM } else { &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM };
        let c0 = cid(m, 0);
        o.call(m, r, &[dict, elem, 1, group, 0, 0x3a97, c0, l8, l4])
    };
    let mut r = range(m, o, false, 0);
    let own = recurse == 0 || {
        let gn = m.u32(elem.wrapping_add(8));
        gn != 0 && m.u32(gn.wrapping_add(8)) != 0
    };
    if own {
        while r != 0 {
            let t = m.i32(l8);
            if !((t.wrapping_sub(0x3a98) as u32) <= 10 || t == 0x2af8) {
                let fcv = m.u32(r.wrapping_add(4));
                fcv_step(m, o, f, fcv, data, -1);
            }
            r = range(m, o, true, 0);
        }
        for group in [1, 2] {
            let mut r = range(m, o, false, group);
            while r != 0 {
                let fcv = m.u32(r.wrapping_add(4));
                fcv_step(m, o, f, fcv, data, -1);
                r = range(m, o, true, group);
            }
        }
        if get_pos_cns_from_elm(m, chapter, elem, lc) == 0 {
            chapter = -1;
        }
        if chapter != -1 && count != 0 {
            loop {
                let related = |m: &mut Mem, o: &mut dyn Outside, next: bool| {
                    let r = if next { &out::SI_ELEMDICT_FIND_NEXT_RELATED_ELEM } else { &out::SI_ELEMDICT_FIND_RELATED_ELEM };
                    let (rel, c8) = (m.u32(lc), cid(m, 8));
                    o.call(m, r, &[dict, elem, 1, 1, rel, c8, l14, l10])
                };
                let mut r = related(m, o, false);
                let mut i = 0u32;
                while r != 0 {
                    let path = if cid(m, 8) == m.u32(l14) { m.u32(r.wrapping_add(4)) } else { 0 };
                    if path != 0 && m.u32(path.wrapping_add(0x30)) == 0 {
                        let name = m.u32(path.wrapping_add(4));
                        if is_path_name(m, name) {
                            let mut take = true;
                            if chapter as u32 == cid(m, 8) {
                                let model = m.u32(elem.wrapping_add(4));
                                let owner = m.u32(model.wrapping_add(0x2bc));
                                take = if owner == 0 { m.u32(path.wrapping_add(0x2bc)) != model } else { owner != m.u32(path.wrapping_add(0x2bc)) };
                            }
                            if take {
                                let fcv = o.call(m, &out::SI_ED_FIND_FCV, &[r, 1, 0x70]);
                                if fcv != 0 {
                                    let which = m.i32(l10);
                                    fcv_step(m, o, f, fcv, data, which);
                                    let (rel, c8) = (m.u32(lc), cid(m, 8));
                                    let a = o.call(m, &out::SI_ELEMDICT_GET_RELATION, &[dict, elem, rel, c8, i]);
                                    let fcv = o.call(m, &out::SI_ED_GET_FCV_FROM_RELATION, &[a]);
                                    if fcv != 0 {
                                        let which = m.i32(l10);
                                        fcv_step(m, o, f, fcv, data, which);
                                    }
                                }
                            }
                        }
                    }
                    r = related(m, o, true);
                    i = i.wrapping_add(1);
                }
                let c6 = cid(m, 6);
                if (chapter as u32 == c6 || chapter as u32 == cid(m, 1)) && m.u32(elem.wrapping_add(0x54)) == 0 {
                    if chapter as u32 == cid(m, 1) {
                        m.set_u32(lc, 0x457);
                        count -= 1;
                    } else if chapter as u32 == c6 {
                        let d = m.u32(elem.wrapping_add(4));
                        if m.u16(d.wrapping_add(0x1e)) != 2 {
                            break;
                        }
                        m.set_u32(lc, 0x83f);
                        count -= 1;
                    }
                } else {
                    count = 0;
                }
                if count == 0 {
                    break;
                }
            }
        }
        if chapter as u32 == cid(m, 8) {
            let related = |m: &mut Mem, o: &mut dyn Outside, next: bool, rel: u32, c: usize| {
                let r = if next { &out::SI_ELEMDICT_FIND_NEXT_RELATED_ELEM } else { &out::SI_ELEMDICT_FIND_RELATED_ELEM };
                let ch = cid(m, c);
                o.call(m, r, &[dict, elem, 1, 2, rel, ch, l8, l4])
            };
            let r = related(m, o, false, 0xc8, 21);
            if r != 0 {
                let c = cid(m, 21) as i32;
                hieranim_fcv_process(m, o, dict, f, r, data, c, 0);
            }
            let mut r = related(m, o, false, 0x2bc, 2);
            while r != 0 {
                let c = cid(m, 2) as i32;
                hieranim_fcv_process(m, o, dict, f, r, data, c, 0);
                r = related(m, o, true, 0x2bc, 2);
            }
            for rel in [0xd2, 0xd7] {
                let r = related(m, o, false, rel, 5);
                if r != 0 {
                    let c = cid(m, 5) as i32;
                    hieranim_fcv_process(m, o, dict, f, r, data, c, 0);
                }
            }
        }
    }
    let mut s = o.call(m, &out::SI_ED_FIND_SON, &[elem]);
    while s != 0 {
        hieranim_fcv_process(m, o, dict, f, s, data, chapter, recurse);
        s = o.call(m, &out::SI_ED_FIND_NEXT_SON, &[elem]);
    }
    m.set_sp(sp);
}

/// The text "PATH_" of the binary's read-only data (at 0x086949a5), which the walk
/// compares the first five characters of a model's name with (`strncmp(name, "PATH_", 5)`).
pub const PATH_PREFIX: &[u8; 5] = b"PATH_";

/// `strncmp(name, "PATH_", 5) == 0`, reading the name as `strncmp` does: up to the first
/// character that differs.
fn is_path_name(m: &Mem, name: u32) -> bool {
    PATH_PREFIX.iter().enumerate().all(|(k, &c)| m.u8(name.wrapping_add(k as u32)) == c)
}

/// [soft 0x081e2960 haFcvProcessSaveHistory]
///
/// Saves a curve for undo: `SI_fcurveSaveHistory` of what `SI_edGetElemFromAnimFile`
/// answers for it.
pub fn ha_fcv_process_save_history(m: &mut Mem, o: &mut dyn Outside, fcv: u32) -> u32 {
    let l = m.local(4);
    let e = o.call(m, &out::SI_ED_GET_ELEM_FROM_ANIM_FILE, &[fcv, l]);
    m.release(4);
    o.call(m, &out::SI_FCURVE_SAVE_HISTORY, &[e])
}

/// The float at `p + i × 4` as a load and a store pass it on (`flds`, `fstps`).
fn fl(m: &Mem, p: u32, i: u32) -> u32 {
    fmov(m.f32(p.wrapping_add(i * 4))).to_bits()
}

/// [soft 0x081e29c4 haFcvProcessRemove]
///
/// Takes the keys between the two times of `data` off the curve
/// (`SI_fcurveRemoveKeyBetweenTimes(fcv, 0, t0, t1, 0)`).
pub fn ha_fcv_process_remove(m: &mut Mem, o: &mut dyn Outside, fcv: u32, data: u32) -> u32 {
    let (a, b) = (fl(m, data, 0), fl(m, data, 1));
    o.call(m, &out::SI_FCURVE_REMOVE_KEY_BETWEEN_TIMES, &[fcv, 0, a, b, 0])
}

/// [soft 0x081e29f4 haFcvProcessCheckEmpty]
///
/// Notes a curve without keys: the 16-bit word at `data` becomes 1 when the curve's number
/// of keys is not positive (nothing for a null curve or `data`).
pub fn ha_fcv_process_check_empty(m: &mut Mem, fcv: u32, data: u32) {
    if data != 0 && fcv != 0 && m.i32(fcv.wrapping_add(0xc)) <= 0 {
        m.set_u16(data, 1);
    }
}

/// The time of key `i` of a curve.
fn key_time(m: &Mem, fcv: u32, i: u32) -> f32 {
    m.f32(m.u32(fcv.wrapping_add(8)).wrapping_add(i.wrapping_mul(KEY)))
}

/// [soft 0x081e2a14 haFcvProcessMove]
///
/// Moves keys in time: `SI_fcvMoveKeyTime(fcv, a, b, c, 0)` with the first three floats of
/// `data`; the fourth chooses: −1 replaces `a` by the curve's first key time, 1 `b` by its
/// last, 2 both (each also stored back into `data`); with −1, 1 or 2 nothing is done for a
/// curve without keys.
pub fn ha_fcv_process_move(m: &mut Mem, o: &mut dyn Outside, fcv: u32, data: u32) {
    let d = x(m.f32(data.wrapping_add(0xc)));
    let n = |m: &Mem| m.i32(fcv.wrapping_add(0xc));
    let (a, b) = if x(-1.0f64).eq(d) {
        if n(m) <= 0 {
            return;
        }
        let t = fmov(key_time(m, fcv, 0));
        m.set_f32(data, t);
        (t.to_bits(), fl(m, data, 1))
    } else if F80::ONE.eq(d) {
        let n = n(m);
        if n <= 0 {
            return;
        }
        let t = fmov(key_time(m, fcv, n as u32 - 1));
        m.set_f32(data.wrapping_add(4), t);
        (fl(m, data, 0), t.to_bits())
    } else if x(2.0f64).eq(d) {
        if n(m) <= 0 {
            return;
        }
        let t0 = fmov(key_time(m, fcv, 0));
        m.set_f32(data, t0);
        let n = n(m);
        let t1 = fmov(key_time(m, fcv, (n as u32).wrapping_sub(1)));
        m.set_f32(data.wrapping_add(4), t1);
        (t0.to_bits(), t1.to_bits())
    } else {
        (fl(m, data, 0), fl(m, data, 1))
    };
    let c = fl(m, data, 2);
    o.call(m, &out::SI_FCV_MOVE_KEY_TIME, &[fcv, a, b, c, 0]);
}

/// [soft 0x081e2b20 haFcvProcessScale]
///
/// Scales key times: `SI_fcvScaleKeyTime(fcv, the four floats of data, 0, 1)`.
pub fn ha_fcv_process_scale(m: &mut Mem, o: &mut dyn Outside, fcv: u32, data: u32) {
    let v = [fl(m, data, 0), fl(m, data, 1), fl(m, data, 2), fl(m, data, 3)];
    o.call(m, &out::SI_FCV_SCALE_KEY_TIME, &[fcv, v[0], v[1], v[2], v[3], 0, 1]);
}

/// [soft 0x081e2b60 haFcvProcessSetKeysOnFrames]
///
/// Puts the keys between the two times of `data` on whole frames
/// (`SI_fcvSetKeysOnFrames`); when the third float is 2 the times are the curve's first and
/// last key times (stored back into `data`; nothing for a curve without keys).
pub fn ha_fcv_process_set_keys_on_frames(m: &mut Mem, o: &mut dyn Outside, fcv: u32, data: u32) {
    let (a, b) = if x(2.0f64).eq(x(m.f32(data.wrapping_add(8)))) {
        let n = m.i32(fcv.wrapping_add(0xc));
        if n <= 0 {
            return;
        }
        let t0 = fmov(key_time(m, fcv, 0));
        m.set_f32(data, t0);
        let n = m.i32(fcv.wrapping_add(0xc));
        let t1 = fmov(key_time(m, fcv, (n as u32).wrapping_sub(1)));
        m.set_f32(data.wrapping_add(4), t1);
        (t0.to_bits(), t1.to_bits())
    } else {
        (fl(m, data, 0), fl(m, data, 1))
    };
    o.call(m, &out::SI_FCV_SET_KEYS_ON_FRAMES, &[fcv, a, b]);
}

/// [soft 0x081e72bc haFcvProcessLastKeyTime]
///
/// The latest last key time: the float at `data` becomes the curve's last key time when
/// that is greater.
pub fn ha_fcv_process_last_key_time(m: &mut Mem, fcv: u32, data: u32) {
    let n = m.i32(fcv.wrapping_add(0xc));
    if n > 0 {
        let t = key_time(m, fcv, n as u32 - 1);
        if x(t).gt(x(m.f32(data))) {
            m.set_f32(data, fmov(t));
        }
    }
}

/// [soft 0x081e67c4 haFcvProcessKeyLocks]
///
/// Locks or unlocks the keys between the two times of `data`
/// (`si_fcurveSetIntervalKeyLocks(fcv, t0, t1, third float > 0)`); nothing for a null
/// curve.
pub fn ha_fcv_process_key_locks(m: &mut Mem, o: &mut dyn Outside, fcv: u32, data: u32) {
    if fcv == 0 {
        return;
    }
    let on = x(m.f32(data.wrapping_add(8))).gt(F80::ZERO) as u32;
    let (a, b) = (fl(m, data, 0), fl(m, data, 1));
    o.call(m, &out::SI_FCURVE_SET_INTERVAL_KEY_LOCKS, &[fcv, a, b, on]);
}

/// The number of leading keys of a curve whose time is at most `t` (the first key after
/// `t`, or a NaN, ends the count); what the two interpolation steps look for.
fn keys_up_to(m: &Mem, fcv: u32, t: f32) -> u32 {
    let n = m.i32(fcv.wrapping_add(0xc));
    let mut i = 0i32;
    if i < n {
        let keys = m.u32(fcv.wrapping_add(8));
        while i < n && x(t).ge(x(m.f32(keys.wrapping_add((i as u32).wrapping_mul(KEY))))) {
            i += 1;
        }
    }
    i as u32
}

/// [soft 0x081e60d8 haFcvProcessSetInterp]
///
/// The last key of the curve that lies within the action (its time at most the action's
/// end and at least its start) gets the action's interpolation (+0x2c) in its word at
/// +0xe. Nothing for a curve of kind 0xc4.
pub fn ha_fcv_process_set_interp(m: &mut Mem, fcv: u32, e: Ptr32<ActionsElem>) {
    if e.is_null() || fcv == 0 || m.u32(fcv) == 0xc4 {
        return;
    }
    let i = keys_up_to(m, fcv, m[e].end);
    if i as i32 > 0 {
        let k = m.u32(fcv.wrapping_add(8)).wrapping_add(i.wrapping_mul(KEY)).wrapping_sub(KEY);
        if x(m[e].start).le(x(m.f32(k))) {
            let v = m[e].unk_2c;
            m.set_u16(k.wrapping_add(0xe), v);
        }
    }
}

/// [soft 0x081e6220 haFcvProcessUpdateInterp]
///
/// Reads back the interpolation: when the last key within the action (as above) has
/// another interpolation than the action's, the action's becomes 5 (mixed). Nothing when
/// it is already 5 or for a curve of kind 0xc4.
pub fn ha_fcv_process_update_interp(m: &mut Mem, fcv: u32, e: Ptr32<ActionsElem>) {
    if e.is_null() || fcv == 0 {
        return;
    }
    let v = m[e].unk_2c;
    if v == 5 || m.u32(fcv) == 0xc4 {
        return;
    }
    let i = keys_up_to(m, fcv, m[e].end);
    if i as i32 > 0 {
        let k = m.u32(fcv.wrapping_add(8)).wrapping_add(i.wrapping_mul(KEY)).wrapping_sub(KEY);
        if x(m[e].start).le(x(m.f32(k))) && m.u16(k.wrapping_add(0xe)) != v {
            m[e].unk_2c = 5;
        }
    }
}

/// A whole number of frames in the register as the original rounds it before a store:
/// `frndint` toward +∞ for a negative value, toward −∞ otherwise (no change for an
/// integer, as here).
fn rnd_frames(v: F80) -> F80 {
    if v.lt(F80::ZERO) {
        v.round_int(Round::Up)
    } else {
        v.round_int(Round::Down)
    }
}

/// Whether a step that takes an action is meant for this curve: `which` −1 (a curve of
/// the element itself), or the element number of the action's path (+4 of its path data).
fn for_action(m: &Mem, e: Ptr32<ActionsElem>, which: i32) -> bool {
    if which == -1 {
        return true;
    }
    let p = m[e].path;
    !p.is_null() && m[p].elem == which
}

/// [soft 0x081e66d4 haFcvProcessCycle]
///
/// Repeats the keys of one cycle of the action over its cycles:
/// `SI_fcvCycleKeysBetweenTimes(fcv, start, start + length / rate, cycles)`, the length
/// that of one cycle in frames (`SI_actionsElemGetLength`) made a float.
pub fn ha_fcv_process_cycle(m: &mut Mem, o: &mut dyn Outside, fcv: u32, e: Ptr32<ActionsElem>, which: i32) {
    if e.is_null() || fcv == 0 || !for_action(m, e, which) {
        return;
    }
    let len = F80::from(ac::actions_elem_get_length(m, e));
    let lf = rnd_frames(len).to_f32();
    let q = x(lf) / x(m[g::SI_FRAME_RATE]);
    let s = m[e].start;
    let end = (x(s) + q).to_f32();
    let cycles = m[e].cycles as u32;
    o.call(m, &out::SI_FCV_CYCLE_KEYS_BETWEEN_TIMES, &[fcv, fmov(s).to_bits(), end.to_bits(), cycles]);
}

/// Room for `n` keys as the action routines make it: the count rounded up to a multiple of
/// 8, 0x24 bytes a key.
fn keys_room(n: i32) -> u32 {
    let n = n as u32;
    let r = n.wrapping_sub((n.wrapping_sub(1) & 7).wrapping_sub(7));
    r.wrapping_mul(KEY)
}

/// [soft 0x081e4b3c haFcvProcessActionDeact]
///
/// Deactivating an action on one curve: the curve's keys within the action's times
/// (`SI_fcvCntKeysBetweenTimes`; each key with start ≤ time ≤ end) are appended to the
/// action's keys (+0x1c, grown with `SI_realloc` or made with `SI_malloc`, room for a
/// multiple of 8), then the curve's keys from start − 1e-4 to end + 1e-4 are removed.
pub fn ha_fcv_process_action_deact(m: &mut Mem, o: &mut dyn Outside, fcv: u32, e: Ptr32<ActionsElem>) {
    if e.is_null() || fcv == 0 {
        return;
    }
    let s = fmov(m[e].start);
    let t = fmov(m[e].end);
    let n = o.call(m, &out::SI_FCV_CNT_KEYS_BETWEEN_TIMES, &[fcv, s.to_bits(), t.to_bits()]) as i32;
    if n > 0 {
        let have = m[e].nb_keys;
        let p = if have > 0 {
            let old = m[e].keys.addr();
            m.si_realloc(old, keys_room(have.wrapping_add(n)))
        } else {
            m.si_malloc(keys_room(n))
        };
        m[e].keys = Ptr32::new(p);
        let mut i = 0;
        let mut at = 0u32;
        while i < m.i32(fcv.wrapping_add(0xc)) {
            let k = m.u32(fcv.wrapping_add(8)).wrapping_add(at);
            let kt = x(m.f32(k));
            if kt.ge(x(s)) && kt.le(x(t)) {
                let dst = m[e].keys.addr().wrapping_add((m[e].nb_keys as u32).wrapping_mul(KEY));
                m.copy(dst, k, KEY);
                m[e].nb_keys += 1;
            }
            i += 1;
            at = at.wrapping_add(KEY);
        }
    }
    let eps = x(TIME_EPS);
    let hi = (eps + t).to_f32();
    let lo = (x(s) - eps).to_f32();
    o.call(m, &out::SI_FCURVE_REMOVE_KEY_BETWEEN_TIMES, &[fcv, 0, lo.to_bits(), hi.to_bits(), 0]);
}

/// A tolerance in seconds as the anchor step compares it: `SI_glbAnim`'s float at +0x18
/// (frames) rounded toward zero in the register, stored as a float, divided by the frame
/// rate (unrounded).
fn tolerance(m: &Mem) -> F80 {
    let tol = f32::from_bits(u32::from(m[g::SI_GLB_ANIM][0x18 / 2]) | u32::from(m[g::SI_GLB_ANIM][0x1a / 2]) << 16);
    let r = rnd_frames(x(tol)).to_f32();
    x(r) / x(m[g::SI_FRAME_RATE])
}

/// [soft 0x081e5c78 haFcvProcessAddAnchorKeys]
///
/// Keys at both ends of the action on one curve (not on a curve of kind 0xc4): at the
/// time of its first frame and of the last frame of its first cycle, with the curve's
/// values there (`SI_fcurveFrameValue`), each unless a key already lies within the
/// tolerance (`SI_glbAnim` +0x18, frames) of that time. The first key takes the
/// interpolation of the last key before it (or `SI_glbAnim` +0x30), the second the
/// action's (+0x2c; `SI_glbAnim` +0x30 when that is 5); `SI_fkeyCreate`,
/// `SI_fcurveAddKey`.
pub fn ha_fcv_process_add_anchor_keys(m: &mut Mem, o: &mut dyn Outside, fcv: u32, e: Ptr32<ActionsElem>, which: i32) {
    if e.is_null() || fcv == 0 || m.u32(fcv) == 0xc4 || !for_action(m, e, which) {
        return;
    }
    let len = ac::actions_elem_get_length(m, e);
    let sf = ac::actions_sec_to_start_frm(m, fmov(m[e].start));
    let ef = len.wrapping_add(sf).wrapping_sub(1);
    let t0 = x(rnd_frames(F80::from(sf)).to_f32()) / x(m[g::SI_FRAME_RATE]);
    let t1 = (x(rnd_frames(F80::from(ef)).to_f32()) / x(m[g::SI_FRAME_RATE])).to_f32();
    let v0 = o.call_f(m, &out::SI_FCURVE_FRAME_VALUE, &[fcv, sf as u32]).to_f32();
    let v1 = o.call_f(m, &out::SI_FCURVE_FRAME_VALUE, &[fcv, ef as u32]).to_f32();
    let (mut near0, mut near1) = (false, false);
    let mut interp = m[g::SI_GLB_ANIM][0x30 / 2];
    let mut i = 0;
    let mut at = 0u32;
    while i < m.i32(fcv.wrapping_add(0xc)) {
        if !near0 {
            let kt = m.f32(m.u32(fcv.wrapping_add(8)).wrapping_add(at));
            let d = (t0 - x(kt)).abs();
            near0 = tolerance(m).ge(d);
        }
        if !near1 {
            let kt = m.f32(m.u32(fcv.wrapping_add(8)).wrapping_add(at));
            let d = (x(t1) - x(kt)).abs();
            near1 = tolerance(m).ge(d);
        }
        let k = m.u32(fcv.wrapping_add(8)).wrapping_add(at);
        if t0.gt(x(m.f32(k))) {
            interp = m.u16(k.wrapping_add(0xe));
        }
        i += 1;
        at = at.wrapping_add(KEY);
    }
    if !near0 {
        let key = o.call(m, &out::SI_FKEY_CREATE, &[t0.to_f32().to_bits(), v0.to_bits(), 0, 0, u32::from(interp)]);
        o.call(m, &out::SI_FCURVE_ADD_KEY, &[fcv, key]);
    }
    if !near1 {
        let mut v = m[e].unk_2c;
        if v == 5 {
            v = m[g::SI_GLB_ANIM][0x30 / 2];
        }
        let key = o.call(m, &out::SI_FKEY_CREATE, &[t1.to_bits(), v1.to_bits(), 0, 0, u32::from(v)]);
        o.call(m, &out::SI_FCURVE_ADD_KEY, &[fcv, key]);
    }
}
