// SPDX-License-Identifier: MIT
//! `actions.c`, third pass: the editing routines of an action list that reach the curves
//! through the element walk ([`crate::walk`]): activating and deactivating an action (its
//! keys taken out of the curves and kept in the action, or put back at its times),
//! deleting one with its keys, saving the curves for undo, anchor keys, interpolation, a
//! new name, a new start or end.
//!
//! The dictionary these routines hand on is the current one (`SI_elemDicts` + the current
//! number × 0x70, [`g::cur_dict`]) unless the routine takes one.

use crate::actions::{self as ac, Actions, ActionsElem, TIME_EPS};
use crate::globals as g;
use crate::outside::{self as out, Outside};
use crate::walk::{self, step};
use si_mem::{clib, Mem, Ptr32};
use si_x87::{fmov, x, F80};

/// The size of a key of a function curve.
const KEY: u32 = 0x24;

fn elem(m: &Mem, a: Ptr32<Actions>, i: i32) -> Ptr32<ActionsElem> {
    m[a].elems.add(i)
}

/// Two floats on the image's stack, the record the walk's steps read through `data`.
fn pair(m: &mut Mem, a: f32, b: f32) -> u32 {
    let p = m.local(8);
    m.set_f32(p, a);
    m.set_f32(p.wrapping_add(4), b);
    p
}

/// [soft 0x081e2984 SI_actionsFcurveSaveHistory]
///
/// Saves every curve of an element's hierarchy for undo: the walk with
/// `haFcvProcessSaveHistory` (nothing for a null element).
pub fn actions_fcurve_save_history(m: &mut Mem, o: &mut dyn Outside, root: u32, chapter: i32) {
    if root != 0 {
        let d = g::cur_dict(m);
        walk::hieranim_fcv_process(m, o, d, step::SAVE_HISTORY, root, 0, chapter, 0);
    }
}

/// [soft 0x081df544 SI_actionsDeleteElem]
///
/// Removes action `i` with its keys: the keys of every curve of the hierarchy of `root`
/// from its start − 1e-4 to its end + 1e-4 (`haFcvProcessRemove`), then the action
/// (`SI_actionsDelDefElem`).
pub fn actions_delete_elem(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>, i: i32, root: u32, chapter: i32) {
    let e = elem(m, a, i);
    let eps = x(TIME_EPS);
    let lo = (x(m[e].start) - eps).to_f32();
    let hi = (eps + x(m[e].end)).to_f32();
    let sp = m.sp();
    let tmp = pair(m, lo, hi);
    let d = g::cur_dict(m);
    walk::hieranim_fcv_process(m, o, d, step::REMOVE, root, tmp, chapter, 0);
    m.set_sp(sp);
    ac::actions_del_def_elem(m, a, i);
}

/// [soft 0x081e4740 SI_actionsElemActivate]
///
/// Turns an action on or off. On (`on` not 0, bit 1 of its flags clear): the bit is set,
/// the curves' keys from start − 1e-4 to end + 1e-4 are removed (`haFcvProcessRemove`) and
/// the action's own keys put back at its times (`actionsElemFcvAct`), which it then no
/// longer holds (freed). Off (bit 1 set): the bit is cleared and every curve's keys within
/// the action move into it (`haFcvProcessActionDeact`). Nothing without an action, an
/// animation file for the list or a root element; the chapter handed to the walk is the
/// word `SI_edGetRootElemFromAnimFile` answers through its third argument.
pub fn actions_elem_activate(m: &mut Mem, o: &mut dyn Outside, dict: u32, a: Ptr32<Actions>, e: Ptr32<ActionsElem>, on: i16) {
    if e.is_null() {
        return;
    }
    let sp = m.sp();
    let (l14, l18) = (m.local(4), m.local(4));
    let file = ac::ed_get_anim_file_from_actions(m, dict, a.addr());
    let root = ac::ed_get_root_elem_from_anim_file(m, o, dict, file, l18, l14);
    if root != 0 && file != 0 {
        let flags = m[e].flags;
        if on != 0 {
            if flags & 2 == 0 {
                let eps = x(TIME_EPS);
                let lo = (x(m[e].start) - eps).to_f32();
                m[e].flags = flags | 2;
                let hi = (eps + x(m[e].end)).to_f32();
                let tmp = pair(m, lo, hi);
                let chapter = m.i32(l18);
                walk::hieranim_fcv_process(m, o, dict, step::REMOVE, root, tmp, chapter, 0);
                let (s, t) = (m[e].start, m[e].end);
                let len = (x(t) - x(s)).to_f32();
                actions_elem_fcv_act(m, o, dict, e, fmov(s), fmov(t), 0.0, len);
                if m[e].nb_keys > 0 {
                    let k = m[e].keys.addr();
                    if k != 0 {
                        m.free(k);
                        m[e].keys = Ptr32::NULL;
                    }
                    m[e].keys = Ptr32::NULL;
                    m[e].nb_keys = 0;
                }
            }
        } else if flags & 2 != 0 {
            m[e].flags = (flags & !0xff) | (flags & 0xfd);
            let chapter = m.i32(l18);
            walk::hieranim_fcv_process(m, o, dict, step::ACTION_DEACT, root, e.addr(), chapter, 0);
        }
    }
    m.set_sp(sp);
}

/// [soft 0x081e4874 actionsElemFcvAct]
///
/// Puts an action's keys back into their curves with their times mapped from the action's
/// window to `start` … `end`. The window: from the action's start + `off` to its start +
/// `len`; a kept key's time t becomes start + (t − (off + action start)) × r, r = (end −
/// start) / (len − off) stored as a float. The keys are kept in runs, one curve (+0x10 of a
/// kept key) after another: for each curve still in the dictionary's chapter `SI_cid[0]`
/// (`SI_elemdictFindElemByData`) the run's keys after the window's start and up to its end
/// are inserted into the curve before its first key not before `start` (its keys made room
/// for with `SI_realloc` or `SI_malloc`, the later ones moved up with `memmove`). A curve
/// left without room ends the routine.
#[allow(clippy::too_many_arguments)]
pub fn actions_elem_fcv_act(m: &mut Mem, o: &mut dyn Outside, dict: u32, e: Ptr32<ActionsElem>, start: f32, end: f32, off: f32, len: f32) {
    let s = x(m[e].start);
    let hi = (s + x(len)).to_f32();
    let ratio = ((x(end) - x(start)) / (x(len) - x(off))).to_f32();
    let lo = (s + x(off)).to_f32();
    let keys = m[e].keys.addr();
    let mut i = 0i32;
    while i < m[e].nb_keys {
        let fcv = m.u32(keys.wrapping_add((i as u32).wrapping_mul(KEY)).wrapping_add(0x10));
        if fcv == 0 {
            i += 1;
            continue;
        }
        let c0 = m[g::SI_CID][0] as u32;
        if (o.call(m, &out::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, c0, fcv]) as i32) < 0 {
            i += 1;
            continue;
        }
        let nb = m[e].nb_keys;
        let key = |j: i32| keys.wrapping_add((j as u32).wrapping_mul(KEY));
        let mut j = i;
        while j < nb {
            let k = key(j);
            if m.u32(k.wrapping_add(0x10)) != fcv {
                break;
            }
            let t = x(m.f32(k));
            if !t.le(x(hi)) || t.gt(x(lo)) {
                break;
            }
            j += 1;
        }
        if j == nb {
            i += 1;
            continue;
        }
        let t = x(m.f32(key(j)));
        if t.gt(x(hi)) || m.u32(key(j).wrapping_add(0x10)) != fcv {
            i += 1;
            continue;
        }
        let first = j;
        let mut count = 0i32;
        let fcount = m.i32(fcv.wrapping_add(0xc));
        if t.le(x(hi)) {
            let mut jj = j;
            loop {
                jj += 1;
                count += 1;
                if jj >= nb || m.u32(key(jj).wrapping_add(0x10)) != fcv || !x(hi).ge(x(m.f32(key(jj)))) {
                    break;
                }
            }
        }
        let p = if fcount > 0 {
            let old = m.u32(fcv.wrapping_add(8));
            m.si_realloc(old, keys_room(count.wrapping_add(fcount)))
        } else {
            m.si_malloc(keys_room(fcount.wrapping_add(count)))
        };
        m.set_u32(fcv.wrapping_add(8), p);
        let fk = m.u32(fcv.wrapping_add(8));
        if fk == 0 {
            return;
        }
        let fc = m.i32(fcv.wrapping_add(0xc));
        let next = first.wrapping_add(count);
        let mut at = 0i32;
        if at < fc && x(start).gt(x(m.f32(fk))) {
            loop {
                at += 1;
                if at >= fc || !x(start).gt(x(m.f32(fk.wrapping_add((at as u32).wrapping_mul(KEY))))) {
                    break;
                }
            }
        }
        let rest = fc - at;
        if rest > 0 {
            let dst = m.u32(fcv.wrapping_add(8)).wrapping_add((count.wrapping_add(at) as u32).wrapping_mul(KEY));
            clib::memmove(m, dst, fk.wrapping_add((at as u32).wrapping_mul(KEY)), (rest as u32).wrapping_mul(KEY));
        }
        if count > 0 {
            let mut src = (first as u32).wrapping_mul(KEY);
            let mut dst = (at as u32).wrapping_mul(KEY);
            for _ in 0..count {
                let to = m.u32(fcv.wrapping_add(8)).wrapping_add(dst);
                m.copy(to, keys.wrapping_add(src), KEY);
                let k = m.u32(fcv.wrapping_add(8)).wrapping_add(dst);
                let base = x(off) + x(m[e].start);
                let v = x(start) + (x(m.f32(k)) - base) * x(ratio);
                m.set_f32(k, v.to_f32());
                src = src.wrapping_add(KEY);
                dst = dst.wrapping_add(KEY);
            }
        }
        let n = m.i32(fcv.wrapping_add(0xc));
        m.set_i32(fcv.wrapping_add(0xc), n.wrapping_add(count));
        i = next;
    }
}

/// Room for `n` keys: the count rounded up to a multiple of 8, 0x24 bytes a key.
fn keys_room(n: i32) -> u32 {
    let n = n as u32;
    n.wrapping_sub((n.wrapping_sub(1) & 7).wrapping_sub(7)).wrapping_mul(KEY)
}

/// [soft 0x081df948 SI_actionsActivateSelected]
///
/// Turns the selected actions of a list on or off (`SI_actionsElemActivate` with the
/// current dictionary), after saving the list (`SI_actionsSaveHistory`) and the curves
/// (`haFcvProcessSaveHistory`, chapter the word at +8 of the file's data) for undo in one
/// group (`SI_historyOpenCluster`, `…CloseCluster`). Nothing without the list's
/// animation file, its data, or a root element.
pub fn actions_activate_selected(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>, on: i16) {
    if a.is_null() {
        return;
    }
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    let d = g::cur_dict(m);
    let file = ac::ed_get_anim_file_from_actions(m, d, a.addr());
    let d = g::cur_dict(m);
    let root = ac::ed_get_root_elem_from_anim_file(m, o, d, file, l8, l4);
    if file != 0 {
        let fd = m.u32(file.wrapping_add(4));
        if fd != 0 && root != 0 {
            o.call(m, &out::SI_HISTORY_OPEN_CLUSTER, &[]);
            let (v8, v4) = (m.u32(l8), m.u32(l4));
            o.call(m, &out::SI_ACTIONS_SAVE_HISTORY, &[fd, v8, v4]);
            let chapter = m.i32(fd.wrapping_add(8));
            let d = g::cur_dict(m);
            walk::hieranim_fcv_process(m, o, d, step::SAVE_HISTORY, root, 0, chapter, 0);
            o.call(m, &out::SI_HISTORY_CLOSE_CLUSTER, &[]);
            let mut i = 0;
            while i < m[a].count {
                let e = elem(m, a, i);
                if m[e].flags & 1 != 0 {
                    let d = g::cur_dict(m);
                    actions_elem_activate(m, o, d, a, e, on);
                }
                i += 1;
            }
        }
    }
    m.set_sp(sp);
}

/// The animation file and root element of a list (`SI_edGetAnimFileFromActions`,
/// `SI_edGetRootElemFromAnimFile` with the current dictionary), and the local the latter
/// stored its chapter word in; the caller gives the locals back.
fn file_and_root(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>) -> (u32, u32, u32) {
    let l8 = m.local(8);
    let l4 = l8.wrapping_add(4);
    let d = g::cur_dict(m);
    let file = ac::ed_get_anim_file_from_actions(m, d, a.addr());
    let d = g::cur_dict(m);
    let root = ac::ed_get_root_elem_from_anim_file(m, o, d, file, l8, l4);
    (file, root, l8)
}

/// Whether `i` names an action of the list as these routines test it: a list, `i` at
/// most the count (`i` equal to the count passes) and not negative.
fn in_list(m: &Mem, a: Ptr32<Actions>, i: i32) -> bool {
    !a.is_null() && m[a].count >= i && i >= 0
}

/// Runs a step on the curves of an action's hierarchy with the action active: an action
/// that is off is turned on first (`SI_actionsElemActivate` with 1) and off again after.
fn with_action_on(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>, e: Ptr32<ActionsElem>, root: u32, chapter: i32, f: u32) {
    let mut was_on = true;
    if m[e].flags & 2 == 0 {
        was_on = false;
        let d = g::cur_dict(m);
        actions_elem_activate(m, o, d, a, e, 1);
    }
    let d = g::cur_dict(m);
    walk::hieranim_fcv_process(m, o, d, f, root, e.addr(), chapter, 0);
    if !was_on {
        let d = g::cur_dict(m);
        actions_elem_activate(m, o, d, a, e, 0);
    }
}

/// [soft 0x081e5b44 SI_actionsElemAddAnchorKeys]
///
/// Keys at both ends of action `i` on every curve of the hierarchy
/// (`haFcvProcessAddAnchorKeys`), the action turned on for it when it is off. The
/// chapter is the word at +8 of the animation file's data, read before the file is tested:
/// a list without an animation file, or a file without data, makes the original read
/// through a null pointer (the image records the fault).
pub fn actions_elem_add_anchor_keys(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>, i: i32) {
    if !in_list(m, a, i) {
        return;
    }
    let sp = m.sp();
    let (file, root, l8) = file_and_root(m, o, a);
    let fd = m.u32(file.wrapping_add(4));
    let chapter = m.i32(fd.wrapping_add(8));
    m.set_i32(l8, chapter);
    if file != 0 && root != 0 {
        let e = elem(m, a, i);
        with_action_on(m, o, a, e, root, chapter, step::ADD_ANCHOR_KEYS);
    }
    m.set_sp(sp);
}

/// [soft 0x081e5f90 SI_actionsElemSetInterp]
///
/// Gives action `i` an interpolation (+0x2c) and, unless it is 5 (mixed), the keys of
/// every curve within the action too (`haFcvProcessSetInterp`, the action turned on for it
/// when it is off). Nothing without an animation file or root element.
pub fn actions_elem_set_interp(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>, i: i32, interp: i32) {
    if !in_list(m, a, i) {
        return;
    }
    let sp = m.sp();
    let (file, root, l8) = file_and_root(m, o, a);
    if file != 0 && root != 0 {
        let e = elem(m, a, i);
        m[e].unk_2c = interp as u16;
        if interp as i16 != 5 {
            let chapter = m.i32(l8);
            with_action_on(m, o, a, e, root, chapter, step::SET_INTERP);
        }
    }
    m.set_sp(sp);
}

/// [soft 0x081e615c SI_actionsElemUpdateInterp]
///
/// Reads the interpolation of action `i` back from the keys (`haFcvProcessUpdateInterp`:
/// 5 when a curve's key differs); nothing when it is 5 already.
pub fn actions_elem_update_interp(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>, i: i32) {
    if !in_list(m, a, i) {
        return;
    }
    let sp = m.sp();
    let (file, root, l8) = file_and_root(m, o, a);
    if file != 0 && root != 0 {
        let e = elem(m, a, i);
        if m[e].unk_2c != 5 {
            let chapter = m.i32(l8);
            let d = g::cur_dict(m);
            walk::hieranim_fcv_process(m, o, d, step::UPDATE_INTERP, root, e.addr(), chapter, 0);
        }
    }
    m.set_sp(sp);
}

/// [soft 0x081e4c90 SI_actionsElemSetName]
///
/// Renames action `i` (a copy of `name` with `strdup`, the old name freed; nothing when it
/// is the same). 0 for no list, `i` outside it, no name, an empty name, or a name another
/// action has (`strcmp`); else 1.
pub fn actions_elem_set_name(m: &mut Mem, a: Ptr32<Actions>, i: i32, name: u32) -> i32 {
    let mut ok = !(a.is_null() || i < 0 || i >= m[a].count || name == 0 || m.u8(name) == 0);
    if ok {
        let mut k = 0;
        while k < m[a].count {
            if k != i {
                let other = m[elem(m, a, k)].name.addr();
                if clib::strcmp(m, name, other) == 0 {
                    ok = false;
                }
            }
            k += 1;
            if !ok {
                break;
            }
        }
        if ok {
            let e = elem(m, a, i);
            let old = m[e].name.addr();
            if old != 0 {
                if clib::strcmp(m, name, old) == 0 {
                    return 1;
                }
                m.free(old);
                m.set_u32(e.addr(), 0);
            }
            let p = m.strdup(name);
            m.set_u32(e.addr(), p);
        }
    }
    ok as i32
}

/// [soft 0x081e51c4 SI_actionsElemSetEnd]
///
/// Moves action `i` of an animation file (`file`'s data + 0xc is the list) so that it
/// ends on frame `end`: `SI_actionsElemSetStart` with its first frame + `end` − its last
/// frame. Nothing without a file, data, or for `i` not below the count.
pub fn actions_elem_set_end(m: &mut Mem, o: &mut dyn Outside, file: u32, i: i32, end: i32) {
    if file == 0 {
        return;
    }
    let d = m.u32(file.wrapping_add(4));
    if d == 0 || i >= m.i32(d.wrapping_add(0xc)) {
        return;
    }
    let a: Ptr32<Actions> = Ptr32::new(d.wrapping_add(0xc));
    let e = elem(m, a, i);
    let sf = ac::actions_sec_to_start_frm(m, fmov(m[e].start));
    let ef = ac::actions_sec_to_end_frm(m, fmov(m[e].end));
    actions_elem_set_start(m, o, file, i, sf.wrapping_add(end).wrapping_sub(ef));
}

/// Four floats on the image's stack (the walk's `data` for the move step).
fn quad(m: &mut Mem, p: u32, v: [f32; 4]) {
    for (k, f) in v.iter().enumerate() {
        m.set_f32(p.wrapping_add(4 * k as u32), *f);
    }
}

/// [soft 0x081e4dbc SI_actionsElemSetStart]
///
/// Moves action `i` of an animation file to start on frame `start` (its time (start − ½) /
/// rate), with its keys, every action turned off meanwhile (`SI_actionsTmpActivateAll`).
/// With the dope sheet's word at +0x7c clear the new place is validated first
/// (`SI_actionsValidateElem`, the action itself left out; refused: nothing changes), the
/// keys in the new span (± 1e-4 s, from the old start or end on where the old and new
/// spans overlap) are removed, the action's keys moved by the offset
/// (`haFcvProcessMove`), put on whole frames when `SI_glbAnim`'s word at +0x32 is set
/// (`haFcvProcessSetKeysOnFrames`), and the list sorted. With the word set, the keys from
/// the old start on (later) or up to the old end (earlier) move, and every later or
/// earlier action with them, unsorted. `i` is not tested for a negative value.
pub fn actions_elem_set_start(m: &mut Mem, o: &mut dyn Outside, file: u32, i: i32, start: i32) {
    if file == 0 {
        return;
    }
    let d = m.u32(file.wrapping_add(4));
    if d == 0 {
        return;
    }
    let a: Ptr32<Actions> = Ptr32::new(d.wrapping_add(0xc));
    let sp = m.sp();
    let (l24, l28) = (m.local(4), m.local(4));
    let tmp = m.local(16);
    let dict = g::cur_dict(m);
    let root = ac::ed_get_root_elem_from_anim_file(m, o, dict, file, l28, l24);
    if root == 0 || i >= m.i32(d.wrapping_add(0xc)) {
        m.set_sp(sp);
        return;
    }
    let ns = ((F80::from(start) - x(0.5f64)) / x(m[g::SI_FRAME_RATE])).to_f32();
    let e = elem(m, a, i);
    let diff = x(ns) - x(m[e].start);
    let delta = diff.to_f32();
    let dope = m[g::SI_DOPE_SH][0x7c / 4] as u16;
    if dope == 0 {
        let cycles = m[e].cycles;
        let ef = ac::actions_sec_to_end_frm(m, (diff + x(m[e].end)).to_f32());
        if ac::actions_validate_elem(m, a, start, ef, cycles, i) == 0 {
            m.set_sp(sp);
            return;
        }
    }
    let dict = g::cur_dict(m);
    ac::actions_tmp_activate_all(m, o, dict, a, 0);
    let os = fmov(m[e].start);
    let oe = fmov(m[e].end);
    let n = x(oe) + x(delta);
    m[e].end = n.to_f32();
    m[e].start = ns;
    let chapter = m.i32(l28);
    let d0 = x(delta);
    if dope != 0 {
        let later = F80::ZERO.lt(d0);
        let (from, dir) = if later { (os, 1.0f32) } else { (oe, -1.0) };
        quad(m, tmp, [from, from, delta, dir]);
        let dict = g::cur_dict(m);
        walk::hieranim_fcv_process(m, o, dict, step::MOVE, root, tmp, chapter, 0);
        let shift = |m: &mut Mem, k: i32| {
            let e = elem(m, a, k);
            m[e].start = (x(delta) + x(m[e].start)).to_f32();
            m[e].end = (x(delta) + x(m[e].end)).to_f32();
        };
        if later {
            let mut k = i.wrapping_add(1);
            if k < m[a].count {
                loop {
                    shift(m, k);
                    k += 1;
                    if k >= m[a].count {
                        break;
                    }
                }
            }
        } else {
            let mut k = i.wrapping_sub(1);
            while k >= 0 {
                shift(m, k);
                k -= 1;
            }
        }
    } else {
        let eps = x(TIME_EPS);
        let whole = |m: &mut Mem| {
            let lo = ((x(os) + d0) - eps).to_f32();
            let hi = (n + eps).to_f32();
            m.set_f32(tmp.wrapping_add(4), hi);
            m.set_f32(tmp, lo);
        };
        if F80::ZERO.lt(d0) {
            if (x(oe) - x(os)).lt(d0) {
                whole(m);
            } else {
                m.set_f32(tmp, oe);
                m.set_f32(tmp.wrapping_add(4), (n + eps).to_f32());
            }
        } else if (x(os) - x(oe)).gt(d0) {
            m.set_u32(tmp.wrapping_add(8), 0);
            m.set_u32(tmp.wrapping_add(12), 0);
            whole(m);
        } else {
            m.set_f32(tmp, ((x(os) + d0) - eps).to_f32());
            m.set_f32(tmp.wrapping_add(4), os);
        }
        let dict = g::cur_dict(m);
        walk::hieranim_fcv_process(m, o, dict, step::REMOVE, root, tmp, chapter, 0);
        m.set_f32(tmp, os);
        m.set_f32(tmp.wrapping_add(4), oe);
        m.set_f32(tmp.wrapping_add(8), delta);
        m.set_u32(tmp.wrapping_add(12), 0);
        let dict = g::cur_dict(m);
        walk::hieranim_fcv_process(m, o, dict, step::MOVE, root, tmp, chapter, 0);
        if m[g::SI_GLB_ANIM][0x32 / 2] != 0 {
            m.set_f32(tmp, (x(os) + d0).to_f32());
            m.set_f32(tmp.wrapping_add(4), (x(oe) + d0).to_f32());
            m.set_u32(tmp.wrapping_add(8), 0);
            let dict = g::cur_dict(m);
            walk::hieranim_fcv_process(m, o, dict, step::SET_KEYS_ON_FRAMES, root, tmp, chapter, 0);
        }
        ac::actions_sort(m, a);
    }
    let dict = g::cur_dict(m);
    ac::actions_tmp_activate_all(m, o, dict, a, 1);
    m.set_sp(sp);
}

// ---------------------------------------------------------------- lengths, cycles, masters

/// The two lists of an animation file's data: the actions at +0xc, the masters at +0x14
/// (each an `SI_Actions`).
fn lists(d: u32) -> (Ptr32<Actions>, Ptr32<Actions>) {
    (Ptr32::new(d.wrapping_add(0xc)), Ptr32::new(d.wrapping_add(0x14)))
}

/// A whole number of frames as the original rounds it before a store (`frndint` toward +∞
/// for a negative value, toward −∞ otherwise: no change for an integer).
fn rnd(v: F80) -> F80 {
    if v.lt(F80::ZERO) {
        v.round_int(si_x87::Round::Up)
    } else {
        v.round_int(si_x87::Round::Down)
    }
}

/// `(int)(v ± 0.5)`: the nearest frame, half away from zero by the sign of `sign`
/// (`fistp` truncating).
fn nearest(v: F80, sign: F80) -> i32 {
    let h = if F80::ZERO.lt(sign) { x(0.5f64) } else { x(-0.5f64) };
    (v + h).to_i32(si_x87::Round::Zero)
}

/// The dope sheet's 16-bit word at +0x7c: set, a change of an action moves the later (or
/// earlier) actions with it instead of being validated against them.
fn ripple(m: &Mem) -> bool {
    m[g::SI_DOPE_SH][0x7c / 4] as u16 != 0
}

/// Shifts the start and end of every action from `from` on by `d` (each sum stored).
fn shift_from(m: &mut Mem, a: Ptr32<Actions>, from: i32, d: f32) {
    let mut k = from;
    while k < m[a].count {
        let e = elem(m, a, k);
        m[e].start = (x(d) + x(m[e].start)).to_f32();
        m[e].end = (x(d) + x(m[e].end)).to_f32();
        k += 1;
    }
}

/// [soft 0x081e5618 SI_actionsElemSetLength]
///
/// Gives action `i` of an animation file a length of `len` frames a cycle: its end becomes
/// start + cycles × len / rate, its keys are scaled about its start by `len` / its length
/// (`haFcvProcessScale`; with the dope sheet's word +0x7c clear the place is validated
/// first and, when it grows, the keys up to the new end + 1e-4 are removed; with it set the
/// keys after it and the later actions move by the change). Every action is off meanwhile;
/// keys are put on frames afterwards when `SI_glbAnim` +0x32 is set (that walk with chapter
/// −1). Nothing for no file, data, root element, a length not positive or `i` not below
/// the count.
pub fn actions_elem_set_length(m: &mut Mem, o: &mut dyn Outside, file: u32, i: i32, len: i32) {
    if file == 0 {
        return;
    }
    let d = m.u32(file.wrapping_add(4));
    if d == 0 || len <= 0 {
        return;
    }
    let (a, _) = lists(d);
    let sp = m.sp();
    let (l2c, l30) = (m.local(4), m.local(4));
    let tmp = m.local(16);
    let dict = g::cur_dict(m);
    let root = ac::ed_get_root_elem_from_anim_file(m, o, dict, file, l30, l2c);
    if root == 0 || i >= m[a].count {
        m.set_sp(sp);
        return;
    }
    let e = elem(m, a, i);
    let q = x(rnd(F80::from(len)).to_f32()) / x(m[g::SI_FRAME_RATE]);
    let ne = F80::from(m[e].cycles) * q + x(m[e].start);
    let new_end = ne.to_f32();
    let delta = (ne - x(m[e].end)).to_f32();
    if !ripple(m) {
        let cycles = m[e].cycles;
        let ef = ac::actions_sec_to_end_frm(m, new_end);
        let sf = ac::actions_sec_to_start_frm(m, fmov(m[e].start));
        if ac::actions_validate_elem(m, a, sf, ef, cycles, i) == 0 {
            m.set_sp(sp);
            return;
        }
    }
    let dict = g::cur_dict(m);
    ac::actions_tmp_activate_all(m, o, dict, a, 0);
    let os = fmov(m[e].start);
    let oe = fmov(m[e].end);
    let olen = ac::actions_elem_get_length(m, e);
    let scale = (x(F80::from(len).to_f32()) / F80::from(olen)).to_f32();
    let chapter = m.i32(l30);
    let run = |m: &mut Mem, o: &mut dyn Outside, f: u32, v: [f32; 4], c: i32| {
        quad(m, tmp, v);
        let dict = g::cur_dict(m);
        walk::hieranim_fcv_process(m, o, dict, f, root, tmp, c, 0);
    };
    if ripple(m) {
        if F80::ZERO.lt(x(delta)) {
            run(m, o, step::MOVE, [oe, oe, delta, 1.0], chapter);
            run(m, o, step::SCALE, [os, os, oe, scale], chapter);
        } else {
            run(m, o, step::SCALE, [os, os, oe, scale], chapter);
            run(m, o, step::MOVE, [oe, oe, delta, 1.0], chapter);
        }
        shift_from(m, a, i.wrapping_add(1), delta);
    } else {
        if F80::ONE.lt(x(scale)) {
            let hi = ((x(oe) + x(delta)) + x(TIME_EPS)).to_f32();
            m.set_f32(tmp, oe);
            m.set_f32(tmp.wrapping_add(4), hi);
            let dict = g::cur_dict(m);
            walk::hieranim_fcv_process(m, o, dict, step::REMOVE, root, tmp, chapter, 0);
        }
        run(m, o, step::SCALE, [os, os, oe, scale], chapter);
    }
    m[e].end = new_end;
    if m[g::SI_GLB_ANIM][0x32 / 2] != 0 {
        let (s, t) = (fmov(m[e].start), fmov(m[e].end));
        m.set_f32(tmp, s);
        m.set_f32(tmp.wrapping_add(4), t);
        m.set_u32(tmp.wrapping_add(8), 0);
        let dict = g::cur_dict(m);
        walk::hieranim_fcv_process(m, o, dict, step::SET_KEYS_ON_FRAMES, root, tmp, -1, 0);
    }
    let dict = g::cur_dict(m);
    ac::actions_tmp_activate_all(m, o, dict, a, 1);
    m.set_sp(sp);
}

/// [soft 0x081e62b8 SI_actionsElemGenerateCycles]
///
/// Gives action `i` `cycles` cycles: its end becomes start + cycles × the length of a cycle
/// (frames made seconds and stored), its keys of one cycle are repeated
/// (`haFcvProcessCycle`). With the dope sheet's word +0x7c clear the new place is validated
/// first; with it set the keys after the action and the later actions move by the change.
/// With fewer cycles than before the keys of the dropped span (± 1e-4) are removed and,
/// for an action with a path, its constraint is moved to a null
/// (`SI_elemdictGetElemFromData`, `SI_relGetConsRelFromGrafType` of the root's graf node,
/// `SI_actionCnsToNull` with its nearest first and last frames). Keys on frames afterwards
/// when `SI_glbAnim` +0x32 is set. Nothing for `i` outside the list, cycles not positive, or
/// no animation file or root element.
pub fn actions_elem_generate_cycles(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>, i: i32, cycles: i32) {
    if a.is_null() || m[a].count <= i || i < 0 || cycles <= 0 {
        return;
    }
    let sp = m.sp();
    let (l30, l34) = (m.local(4), m.local(4));
    let tmp = m.local(16);
    let dict = g::cur_dict(m);
    let file = ac::ed_get_anim_file_from_actions(m, dict, a.addr());
    let dict = g::cur_dict(m);
    let root = ac::ed_get_root_elem_from_anim_file(m, o, dict, file, l34, l30);
    if file == 0 || root == 0 {
        m.set_sp(sp);
        return;
    }
    let e = elem(m, a, i);
    let len = ac::actions_elem_get_length(m, e);
    let clen = (x(rnd(F80::from(len)).to_f32()) / x(m[g::SI_FRAME_RATE])).to_f32();
    let shift = (F80::from(cycles.wrapping_sub(m[e].cycles)) * x(clen)).to_f32();
    if !ripple(m) {
        let ef = ac::actions_sec_to_end_frm(m, (F80::from(cycles) * x(clen) + x(m[e].start)).to_f32());
        let sf = ac::actions_sec_to_start_frm(m, fmov(m[e].start));
        if ac::actions_validate_elem(m, a, sf, ef, cycles, i) == 0 {
            m.set_sp(sp);
            return;
        }
    }
    let chapter = m.i32(l34);
    if F80::ZERO.gt(x(shift)) {
        let eps = x(TIME_EPS);
        let lo = ((F80::from(cycles) * x(clen) + x(m[e].start)) - eps).to_f32();
        let hi = (eps + (F80::from(m[e].cycles) * x(clen) + x(m[e].start))).to_f32();
        m.set_f32(tmp, lo);
        m.set_f32(tmp.wrapping_add(4), hi);
        let dict = g::cur_dict(m);
        walk::hieranim_fcv_process(m, o, dict, step::REMOVE, root, tmp, chapter, 0);
    }
    if ripple(m) {
        let t = fmov(m[e].end);
        quad(m, tmp, [t, t, shift, 1.0]);
        let dict = g::cur_dict(m);
        walk::hieranim_fcv_process(m, o, dict, step::MOVE, root, tmp, chapter, 0);
        shift_from(m, a, i.wrapping_add(1), shift);
    }
    let cc = F80::from(cycles) * x(clen);
    m[e].cycles = cycles;
    m[e].end = (cc + x(m[e].start)).to_f32();
    let dict = g::cur_dict(m);
    walk::hieranim_fcv_process(m, o, dict, step::CYCLE, root, e.addr(), chapter, 0);
    if F80::ZERO.gt(x(shift)) && !m[e].path.is_null() {
        let l38 = m.local(4);
        let dict = g::cur_dict(m);
        let data = m.u32(root.wrapping_add(4));
        o.call(m, &out::SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict, chapter as u32, data, l38]);
        let gt = m.u32(m.u32(root.wrapping_add(8)));
        let rel = o.call(m, &out::SI_REL_GET_CONS_REL_FROM_GRAF_TYPE, &[gt]);
        let rate = m[g::SI_FRAME_RATE];
        let ef = nearest(x(m[e].end) * x(rate), x(m[e].end));
        let sf = nearest(x(m[e].start) * x(rate), x(m[e].start));
        let pe = m[m[e].path].elem as u32;
        let idx = m.u32(l38);
        o.call(m, &out::SI_ACTION_CNS_TO_NULL, &[chapter as u32, idx, 0, 0, pe, rel, sf as u32, ef as u32, 1]);
    }
    if m[g::SI_GLB_ANIM][0x32 / 2] != 0 {
        let (s, t) = (fmov(m[e].start), fmov(m[e].end));
        m.set_f32(tmp, s);
        m.set_f32(tmp.wrapping_add(4), t);
        m.set_u32(tmp.wrapping_add(8), 0);
        let dict = g::cur_dict(m);
        walk::hieranim_fcv_process(m, o, dict, step::SET_KEYS_ON_FRAMES, root, tmp, chapter, 0);
    }
    m.set_sp(sp);
}

/// [soft 0x081e6888 SI_actionsGenInstance]
///
/// Makes action `ii` of an animation file (a new one, active, when `ii` is negative) an
/// instance of master `mi` from frame `a` to frame `b` (times (a − ½) / rate and (b + ½) /
/// rate) playing the master's frames `c` to `d` (clip times c / rate and (d + 1) / rate):
/// the keys in the new span (± 1e-4) are removed (all curves, chapter −1), the master's keys
/// put there (`actionsElemFcvAct`), on frames when `SI_glbAnim` +0x32 is set; the
/// instance takes the master's interpolation and name (+4, `strdup` unless it already has
/// that name) and the clip times. A new instance with `flag` set first moves every later
/// action and its keys by the span's length; an existing one that is off is turned on for
/// it (its kept keys freed) and off again after. Nothing for no file, data or root, or `mi`
/// outside the masters.
#[allow(clippy::too_many_arguments)]
pub fn actions_gen_instance(m: &mut Mem, o: &mut dyn Outside, file: u32, mi: i32, ii: i32, fa: i32, fb: i32, fc: i32, fd: i32, flag: i16) {
    let sp = m.sp();
    let (l24, l28) = (m.local(4), m.local(4));
    let tmp = m.local(16);
    let dict = g::cur_dict(m);
    let root = ac::ed_get_root_elem_from_anim_file(m, o, dict, file, l28, l24);
    let d = if file == 0 { 0 } else { m.u32(file.wrapping_add(4)) };
    if d == 0 || root == 0 {
        m.set_sp(sp);
        return;
    }
    let h = x(0.5f64);
    let r = x(m[g::SI_FRAME_RATE]);
    let t0 = ((F80::from(fa) - h) / r).to_f32();
    let t1 = ((F80::from(fb) + h) / r).to_f32();
    let c0 = F80::from(fc) / r;
    let c1 = (F80::from(fd) + F80::ONE) / r;
    let (acts, masters) = lists(d);
    let span = x(t1) - x(t0);
    let c0 = c0.to_f32();
    let c1 = c1.to_f32();
    if mi < 0 || mi >= m[masters].count {
        m.set_sp(sp);
        return;
    }
    let master = elem(m, masters, mi);
    let mut turned_on = false;
    let mut ii = ii;
    if ii < 0 {
        if flag != 0 {
            let sv = span.to_f32();
            quad(m, tmp, [t0, t0, sv, 1.0]);
            let dict = g::cur_dict(m);
            walk::hieranim_fcv_process(m, o, dict, step::MOVE, root, tmp, -1, 0);
            let mut k = 0;
            while k < m[acts].count {
                let e = elem(m, acts, k);
                let s = x(m[e].start);
                if s.gt(x(t0)) {
                    m[e].start = (s + span).to_f32();
                    m[e].end = (x(m[e].end) + span).to_f32();
                }
                k += 1;
            }
        } else {
            remove_span(m, o, tmp, root, t0, t1);
        }
    } else {
        let e = elem(m, acts, ii);
        if m[e].flags & 2 == 0 {
            turned_on = true;
            if m[e].nb_keys >= 0 {
                let k = m[e].keys.addr();
                if k != 0 {
                    m.free(k);
                    m[e].keys = Ptr32::NULL;
                }
            }
            m[e].nb_keys = 0;
            m[e].keys = Ptr32::NULL;
            m.or_u8(e.addr().wrapping_add(0x18), 2);
        }
        remove_span(m, o, tmp, root, t0, t1);
    }
    let dict = g::cur_dict(m);
    actions_elem_fcv_act(m, o, dict, master, t0, t1, c0, c1);
    if m[g::SI_GLB_ANIM][0x32 / 2] != 0 {
        m.set_f32(tmp, t0);
        m.set_f32(tmp.wrapping_add(4), t1);
        m.set_u32(tmp.wrapping_add(8), 0);
        let dict = g::cur_dict(m);
        walk::hieranim_fcv_process(m, o, dict, step::SET_KEYS_ON_FRAMES, root, tmp, -1, 0);
    }
    if ii < 0 {
        ii = ac::actions_add_elem_time(m, acts, 0, t0, t1, 1, 2);
    }
    let e = elem(m, acts, ii);
    let v = m[master].unk_2c;
    m[e].unk_2c = v;
    let mut has = m[e].file.addr() != 0;
    if has {
        let mn = m[master].name.addr();
        if clib::strcmp(m, m[e].file.addr(), mn) != 0 {
            let p = m[e].file.addr();
            if p != 0 {
                m.free(p);
                m.set_u32(e.addr().wrapping_add(4), 0);
            }
            m.set_u32(e.addr().wrapping_add(4), 0);
        }
        has = m[e].file.addr() != 0;
    }
    if !has {
        let mn = m[master].name.addr();
        let p = m.strdup(mn);
        m.set_u32(e.addr().wrapping_add(4), p);
    }
    m[e].clip_start = c0;
    m[e].clip_end = c1;
    if turned_on {
        let dict = g::cur_dict(m);
        actions_elem_activate(m, o, dict, acts, e, 0);
    }
    m.set_sp(sp);
}

/// The keys of every curve from `t0` − 1e-4 to `t1` + 1e-4 removed (`haFcvProcessRemove`,
/// chapter −1), as `SI_actionsGenInstance` does before it puts a master's keys there.
fn remove_span(m: &mut Mem, o: &mut dyn Outside, tmp: u32, root: u32, t0: f32, t1: f32) {
    let eps = x(TIME_EPS);
    let hi = (eps + x(t1)).to_f32();
    let lo = (x(t0) - eps).to_f32();
    m.set_f32(tmp.wrapping_add(4), hi);
    m.set_f32(tmp, lo);
    let dict = g::cur_dict(m);
    walk::hieranim_fcv_process(m, o, dict, step::REMOVE, root, tmp, -1, 0);
}

/// The clip routines' common part: action `i` of the file's list, its master by the name
/// at +4 (`SI_actionsGetElemIdByName` in the masters), the clip frame the other end keeps,
/// and the instance made again with the new clip (one cycle meanwhile:
/// `SI_actionsElemGenerateCycles`).
fn set_clip(m: &mut Mem, o: &mut dyn Outside, file: u32, i: i32, frame: i32, end: bool) {
    if file == 0 {
        return;
    }
    let d = m.u32(file.wrapping_add(4));
    if d == 0 || frame < 0 {
        return;
    }
    let (acts, masters) = lists(d);
    if i < 0 || i >= m[acts].count {
        return;
    }
    let e = elem(m, acts, i);
    let mi = ac::actions_get_elem_id_by_name(m, masters, m[e].file.addr());
    if mi < 0 {
        return;
    }
    let other = |m: &Mem| {
        let c = if end { m[e].clip_start } else { m[e].clip_end };
        nearest(x(c) * x(m[g::SI_FRAME_RATE]), x(c))
    };
    let master = elem(m, masters, mi);
    let n = if end { frame.wrapping_sub(other(m)) } else { other(m).wrapping_sub(frame) };
    if n < 0 || n >= ac::actions_elem_get_length(m, master) {
        return;
    }
    let cyc = m[e].cycles;
    if cyc > 1 {
        actions_elem_generate_cycles(m, o, acts, i, 1);
    }
    let c = other(m);
    let ef = ac::actions_sec_to_end_frm(m, fmov(m[e].end));
    let sf = ac::actions_sec_to_start_frm(m, fmov(m[e].start));
    let (fc, fd) = if end { (c, frame) } else { (frame, c) };
    actions_gen_instance(m, o, file, mi, i, sf, ef, fc, fd, 0);
    if cyc > 1 {
        actions_elem_generate_cycles(m, o, acts, i, cyc);
    }
}

/// [soft 0x081e5238 SI_actionsElemSetClipStart]
///
/// Plays action `i`'s master from frame `frame` on: the instance made again
/// (`SI_actionsGenInstance`) with the clip from `frame` to its clip end's nearest frame
/// (+0x14 × rate). Nothing for a negative frame, `i` outside the list, no master of the
/// action's name, or a clip not shorter than the master.
pub fn actions_elem_set_clip_start(m: &mut Mem, o: &mut dyn Outside, file: u32, i: i32, frame: i32) {
    set_clip(m, o, file, i, frame, false);
}

/// [soft 0x081e53f4 SI_actionsElemSetClipEnd]
///
/// The same for the clip's end: from the clip start's nearest frame (+0x10 × rate) to
/// `frame`.
pub fn actions_elem_set_clip_end(m: &mut Mem, o: &mut dyn Outside, file: u32, i: i32, frame: i32) {
    set_clip(m, o, file, i, frame, true);
}

/// [soft 0x081e72ec SI_actionsDeleteMaster]
///
/// Removes master `mi` of an animation file: every action naming it (+4, `strcmp`) loses
/// the name (freed), then the master is taken out (`SI_actionsDelDefElem`).
pub fn actions_delete_master(m: &mut Mem, file: u32, mi: i32) {
    if mi < 0 || file == 0 {
        return;
    }
    let d = m.u32(file.wrapping_add(4));
    if d == 0 || mi >= m.i32(d.wrapping_add(0x14)) {
        return;
    }
    let (acts, masters) = lists(d);
    let master = elem(m, masters, mi);
    let mut k = 0;
    while k < m[acts].count {
        let e = elem(m, acts, k);
        let p = m[e].file.addr();
        if p != 0 && clib::strcmp(m, p, m[master].name.addr()) == 0 {
            let p = m[e].file.addr();
            if p != 0 {
                m.free(p);
                m.set_u32(e.addr().wrapping_add(4), 0);
            }
            m.set_u32(e.addr().wrapping_add(4), 0);
        }
        k += 1;
    }
    ac::actions_del_def_elem(m, masters, mi);
}

/// [soft 0x081e7570 SI_actionsMasterUpdateInst]
///
/// Makes every instance of master `mi` (an action whose +4 names it) again
/// (`SI_actionsGenInstance` at its frames, one cycle meanwhile) with its clip scaled by the
/// master's length / `len`: clip frames the nearest of clip × ratio × rate (the end one
/// less). `mi` is not tested.
pub fn actions_master_update_inst(m: &mut Mem, o: &mut dyn Outside, file: u32, mi: i32, len: i32) {
    if file == 0 {
        return;
    }
    let d = m.u32(file.wrapping_add(4));
    if d == 0 {
        return;
    }
    let (acts, masters) = lists(d);
    let master = elem(m, masters, mi);
    let olen = ac::actions_elem_get_length(m, master);
    let ratio = (x(F80::from(olen).to_f32()) / F80::from(len)).to_f32();
    let mut k = 0;
    while k < m[acts].count {
        let e = elem(m, acts, k);
        let p = m[e].file.addr();
        if p != 0 && clib::strcmp(m, p, m[master].name.addr()) == 0 {
            let cyc = m[e].cycles;
            if cyc > 1 {
                actions_elem_generate_cycles(m, o, acts, k, 1);
            }
            let e = elem(m, acts, k);
            let rate = x(m[g::SI_FRAME_RATE]);
            let ce = nearest(x(ratio) * x(m[e].clip_end) * rate, x(ratio) * x(m[e].clip_end)).wrapping_sub(1);
            let cs = nearest(x(ratio) * x(m[e].clip_start) * rate, x(ratio) * x(m[e].clip_start));
            let ef = ac::actions_sec_to_end_frm(m, fmov(m[e].end));
            let sf = ac::actions_sec_to_start_frm(m, fmov(m[e].start));
            actions_gen_instance(m, o, file, mi, k, sf, ef, cs, ce, 0);
            if cyc > 1 {
                actions_elem_generate_cycles(m, o, acts, k, cyc);
            }
        }
        k += 1;
    }
}

/// [soft 0x081e7734 SI_actionsGenMasterFrmAct]
///
/// Makes action `i` of an animation file a master: master `mi` when it is one, else a new
/// master at the end (the array grown by `SI_calloc` or `SI_realloc`, named after the
/// action by `SI_actionsGetNewName`, the action given a copy of that name at +4). The
/// master takes the action's times, kept keys (the action keeps none), interpolation, one
/// cycle, no flags, path or name at +4; the action's clip becomes 0 … its length in
/// seconds; the action is deactivated first when it is on (`haFcvProcessActionDeact`,
/// chapter −1) and every instance made again (`SI_actionsMasterUpdateInst` with the
/// length before). Its cycles are taken down to one meanwhile. The master's number, or −1
/// for no file, data or root, `i` outside the list or `mi` not below the masters' count.
pub fn actions_gen_master_frm_act(m: &mut Mem, o: &mut dyn Outside, file: u32, i: i32, mi: i32) -> i32 {
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    let dict = g::cur_dict(m);
    let root = ac::ed_get_root_elem_from_anim_file(m, o, dict, file, l8, l4);
    m.set_sp(sp);
    if root == 0 || file == 0 {
        return -1;
    }
    let d = m.u32(file.wrapping_add(4));
    if d == 0 {
        return -1;
    }
    let (acts, masters) = lists(d);
    if i < 0 || i >= m[acts].count || mi >= m[masters].count {
        return -1;
    }
    let e = elem(m, acts, i);
    let cyc = m[e].cycles;
    if cyc > 1 {
        actions_elem_generate_cycles(m, o, acts, i, 1);
    }
    if m[e].flags & 2 != 0 {
        let dict = g::cur_dict(m);
        walk::hieranim_fcv_process(m, o, dict, step::ACTION_DEACT, root, e.addr(), -1, 0);
    }
    let n = m[masters].count;
    let mut mi = mi;
    let (master, len) = if mi < 0 || mi >= n {
        mi = n;
        let p = if n == 0 {
            m.si_calloc(1, 0x30)
        } else {
            let old = m[masters].elems.addr();
            m.si_realloc(old, (mi as u32).wrapping_mul(0x30).wrapping_add(0x30))
        };
        m[masters].elems = Ptr32::new(p);
        let master = elem(m, masters, n);
        let nm = ac::actions_get_new_name(m, masters, m[e].name.addr());
        m.set_u32(master.addr(), nm);
        m[masters].count += 1;
        let p = m.strdup(m[master].name.addr());
        m.set_u32(e.addr().wrapping_add(4), p);
        (master, ac::actions_elem_get_length(m, e))
    } else {
        let master = elem(m, masters, mi);
        (master, ac::actions_elem_get_length(m, master))
    };
    m[master].start = fmov(m[e].start);
    m[master].end = fmov(m[e].end);
    m[master].nb_keys = m[e].nb_keys;
    m[master].keys = m[e].keys;
    m[e].nb_keys = 0;
    m[e].keys = Ptr32::NULL;
    m[master].cycles = 1;
    m[master].unk_2c = m[e].unk_2c;
    m[master].flags = 0;
    m.set_u32(master.addr().wrapping_add(4), 0);
    m[master].path = Ptr32::NULL;
    m.set_u32(e.addr().wrapping_add(0x10), 0);
    m[e].clip_end = (x(m[master].end) - x(m[master].start)).to_f32();
    actions_master_update_inst(m, o, file, mi, len);
    if cyc > 1 {
        actions_elem_generate_cycles(m, o, acts, i, cyc);
    }
    mi
}

/// [soft 0x081e73b4 SI_actionsAddMasterKeysToHA]
///
/// Writes the masters' keys back into the curves (`flag` set) or takes them out again
/// (`flag` 0: `haFcvProcessActionDeact` per master, chapter −1). Back: each master in turn
/// moves to the frame after the curves' last key (t = (nearest frame of the latest last
/// key time (`haFcvProcessLastKeyTime` from 0) + 1.5) / rate), its keys go there
/// (`actionsElemFcvAct` from t to t + its length) and it keeps none. The file's data is
/// read without a test.
pub fn actions_add_master_keys_to_ha(m: &mut Mem, o: &mut dyn Outside, dict: u32, file: u32, flag: i16) {
    let sp = m.sp();
    let (l1c, l20, l24) = (m.local(4), m.local(4), m.local(4));
    let root = ac::ed_get_root_elem_from_anim_file(m, o, dict, file, l20, l1c);
    let d = m.u32(file.wrapping_add(4));
    let (_, masters) = lists(d);
    let mut k = 0;
    if flag != 0 {
        while k < m[masters].count {
            let ms = elem(m, masters, k);
            m.set_u32(l24, 0);
            walk::hieranim_fcv_process(m, o, dict, step::LAST_KEY_TIME, root, l24, -1, 0);
            let t = x(m.f32(l24));
            let r = x(m[g::SI_FRAME_RATE]);
            let f = nearest(t * r, t);
            let t = ((F80::from(f) + x(1.5f64)) / r).to_f32();
            let span = x(m[ms].end) - x(m[ms].start);
            let te = (x(t) + span).to_f32();
            m.set_f32(l24, te);
            actions_elem_fcv_act(m, o, dict, ms, t, te, 0.0, span.to_f32());
            m[ms].start = t;
            m[ms].end = te;
            if m[ms].nb_keys > 0 {
                let p = m[ms].keys.addr();
                if p != 0 {
                    m.free(p);
                    m[ms].keys = Ptr32::NULL;
                }
                m[ms].keys = Ptr32::NULL;
                m[ms].nb_keys = 0;
            }
            k += 1;
        }
    } else {
        while k < m[masters].count {
            let ms = elem(m, masters, k);
            walk::hieranim_fcv_process(m, o, dict, step::ACTION_DEACT, root, ms.addr(), -1, 0);
            k += 1;
        }
    }
    m.set_sp(sp);
}

// ---------------------------------------------------------------- the selection's commands

/// The undo group the selection commands open first: the list (`SI_actionsSaveHistory`
/// with the two words `SI_edGetRootElemFromAnimFile` stored) and every curve of the
/// hierarchy (`haFcvProcessSaveHistory`, chapter `chapter`).
fn save_for_undo(m: &mut Mem, o: &mut dyn Outside, fd: u32, l8: u32, l4: u32, root: u32, chapter: i32) {
    o.call(m, &out::SI_HISTORY_OPEN_CLUSTER, &[]);
    let (v8, v4) = (m.u32(l8), m.u32(l4));
    o.call(m, &out::SI_ACTIONS_SAVE_HISTORY, &[fd, v8, v4]);
    let d = g::cur_dict(m);
    walk::hieranim_fcv_process(m, o, d, step::SAVE_HISTORY, root, 0, chapter, 0);
    o.call(m, &out::SI_HISTORY_CLOSE_CLUSTER, &[]);
}

/// [soft 0x081e047c SI_actionsClearSelected]
///
/// Deletes the selected actions with their keys (`SI_actionsDeleteElem`, chapter the word
/// at +8 of the file's data), after saving for undo. Nothing without an animation file,
/// data, root element or a selected action.
pub fn actions_clear_selected(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>) {
    let sp = m.sp();
    let (file, root, l8) = file_and_root(m, o, a);
    let l4 = l8.wrapping_add(4);
    if file != 0 && m.u32(file.wrapping_add(4)) != 0 && root != 0 && ac::actions_count_selected(m, a) > 0 {
        let fd = m.u32(file.wrapping_add(4));
        let chapter = m.i32(fd.wrapping_add(8));
        save_for_undo(m, o, fd, l8, l4, root, chapter);
        let mut i = 0;
        while i < m[a].count {
            if m[elem(m, a, i)].flags & 1 != 0 {
                let chapter = m.i32(fd.wrapping_add(8));
                actions_delete_elem(m, o, a, i, root, chapter);
                i -= 1;
            }
            i += 1;
        }
    }
    m.set_sp(sp);
}

/// [soft 0x081e0660 SI_actionsDelDefSelected]
///
/// Deletes the selected actions but keeps their keys in the curves: each is turned on
/// (`SI_actionsElemActivate` with 1) and taken out (`SI_actionsDelDefElem`), after saving
/// for undo.
pub fn actions_del_def_selected(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>) {
    if a.is_null() {
        return;
    }
    let sp = m.sp();
    let (file, root, l8) = file_and_root(m, o, a);
    let l4 = l8.wrapping_add(4);
    let fd = if file == 0 { 0 } else { m.u32(file.wrapping_add(4)) };
    if fd != 0 && root != 0 {
        let chapter = m.i32(fd.wrapping_add(8));
        save_for_undo(m, o, fd, l8, l4, root, chapter);
        let mut i = 0;
        while i < m[a].count {
            let e = elem(m, a, i);
            if m[e].flags & 1 != 0 {
                let d = g::cur_dict(m);
                actions_elem_activate(m, o, d, a, e, 1);
                ac::actions_del_def_elem(m, a, i);
                i -= 1;
            }
            i += 1;
        }
    }
    m.set_sp(sp);
}

/// [soft 0x081e0c30 SI_actionsAlignSelected]
///
/// Moves the whole list so that the first selected action starts on the current frame:
/// every key of the hierarchy by (`SI_frameCurrent` − its first frame) / rate
/// (`haFcvProcessMove` with mode 2: from the first key to the last), on frames after when
/// `SI_glbAnim` +0x32 is set, and every action by the same offset (unrounded), after saving
/// for undo (chapter the word `SI_edGetRootElemFromAnimFile` stored). Nothing without an
/// animation file, root element or selected action.
pub fn actions_align_selected(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>) {
    let sp = m.sp();
    let (file, root, l24) = file_and_root(m, o, a);
    let l20 = l24.wrapping_add(4);
    if root != 0 && file != 0 {
        let count = m[a].count;
        let mut first = 0u32;
        let mut k = 0;
        if k < count {
            first = m[a].elems.addr();
            let mut e = first;
            while m.u8(e.wrapping_add(0x18)) & 1 == 0 {
                e = e.wrapping_add(0x30);
                k += 1;
                if k >= count {
                    break;
                }
                first = e;
            }
        }
        if first != 0 && k < count {
            let fd = m.u32(file.wrapping_add(4));
            let chapter = m.i32(l24);
            save_for_undo(m, o, fd, l24, l20, root, chapter);
            let sf = ac::actions_sec_to_start_frm(m, fmov(m.f32(first.wrapping_add(8))));
            let df = m[g::SI_FRAME_CURRENT].wrapping_sub(sf);
            let d = x(rnd(F80::from(df)).to_f32()) / x(m[g::SI_FRAME_RATE]);
            let tmp = m.local(16);
            quad(m, tmp, [0.0, 0.0, d.to_f32(), 2.0]);
            let chapter = m.i32(l24);
            let dict = g::cur_dict(m);
            walk::hieranim_fcv_process(m, o, dict, step::MOVE, root, tmp, chapter, 0);
            if m[g::SI_GLB_ANIM][0x32 / 2] != 0 {
                m.set_f32(tmp, 0.0);
                m.set_f32(tmp.wrapping_add(4), 0.0);
                m.set_f32(tmp.wrapping_add(8), 2.0);
                let chapter = m.i32(l24);
                let dict = g::cur_dict(m);
                walk::hieranim_fcv_process(m, o, dict, step::SET_KEYS_ON_FRAMES, root, tmp, chapter, 0);
            }
            let mut k = 0;
            while k < m[a].count {
                let e = elem(m, a, k);
                m[e].start = (x(m[e].start) + d).to_f32();
                m[e].end = (x(m[e].end) + d).to_f32();
                k += 1;
            }
        }
    }
    m.set_sp(sp);
}

/// The keys of the cycles after the first of an action with more than one, unlocked
/// (`haFcvProcessKeyLocks` from its start + one cycle's length to its end, flag −1).
fn unlock_cycles(m: &mut Mem, o: &mut dyn Outside, e: Ptr32<ActionsElem>, root: u32, chapter: i32) {
    let len = ac::actions_elem_get_length(m, e);
    let s = x(m[e].start);
    let q = x(rnd(F80::from(len)).to_f32()) / x(m[g::SI_FRAME_RATE]);
    let tmp = m.local(12);
    m.set_f32(tmp, (q + s).to_f32());
    m.set_f32(tmp.wrapping_add(4), fmov(m[e].end));
    m.set_f32(tmp.wrapping_add(8), -1.0);
    let dict = g::cur_dict(m);
    walk::hieranim_fcv_process(m, o, dict, step::KEY_LOCKS, root, tmp, chapter, 0);
    m.release(12);
}

/// [soft 0x081e0858 SI_actionsMergeSelected]
///
/// Merges the selected actions (consecutive, at most one with a path:
/// `SI_actionsSelectedConsec`, `actionsConfirmMergeOnlyOnePath`) into the first: it takes
/// the last one's end and the path of any, loses its master's name (+4), and has one cycle;
/// the others are taken out (`SI_actionsDelDefElem`). The later cycles of each merged
/// action are unlocked first (`haFcvProcessKeyLocks`). Every action is off meanwhile;
/// saved for undo first.
pub fn actions_merge_selected(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>) {
    let sp = m.sp();
    let (file, root, l20) = file_and_root(m, o, a);
    let l1c = l20.wrapping_add(4);
    if root != 0 && file != 0 && ac::actions_selected_consec(m, a) != 0 && ac::actions_confirm_merge_only_one_path(m, a) as u16 != 0 {
        let fd = m.u32(file.wrapping_add(4));
        let chapter = m.i32(l20);
        save_for_undo(m, o, fd, l20, l1c, root, chapter);
        let d = g::cur_dict(m);
        ac::actions_tmp_activate_all(m, o, d, a, 0);
        let chapter = m.i32(m.u32(file.wrapping_add(4)).wrapping_add(8));
        let mut first = -1i32;
        let mut k = 0;
        while k < m[a].count {
            let e = elem(m, a, k);
            if first >= 0 {
                if m[e].flags & 1 == 0 {
                    break;
                }
                let f = elem(m, a, first);
                let p = m[f].file.addr();
                if p != 0 {
                    m.free(p);
                    m.set_u32(f.addr().wrapping_add(4), 0);
                }
                m[f].end = fmov(m[e].end);
                if m[e].cycles > 1 {
                    unlock_cycles(m, o, e, root, chapter);
                }
                let p = m[e].path;
                if !p.is_null() {
                    let f = elem(m, a, first);
                    m[f].path = p;
                    m[e].path = Ptr32::NULL;
                }
                ac::actions_del_def_elem(m, a, k);
                k -= 1;
            } else if m[e].flags & 1 != 0 {
                first = k;
                if m[e].cycles > 1 {
                    unlock_cycles(m, o, e, root, chapter);
                }
                m[e].cycles = 1;
            }
            k += 1;
        }
        let d = g::cur_dict(m);
        ac::actions_tmp_activate_all(m, o, d, a, 1);
    }
    m.set_sp(sp);
}

// ---------------------------------------------------------------- loading and time scaling

/// The kept keys of an action or master after a time scale: every key's time × `s`; a
/// key whose interpolation (16 bits at +0xe) is above 1 with bit 0 of its flags (+0xc) set
/// has its two tangents (+0x8, +0x18) divided by `s`, or made 100000.0 when `s` is 0.
fn scale_keys(m: &mut Mem, e: Ptr32<ActionsElem>, s: F80) {
    let keys = m[e].keys.addr();
    let mut j = 0;
    while j < m[e].nb_keys {
        let k = keys.wrapping_add((j as u32).wrapping_mul(KEY));
        m.set_f32(k, (x(m.f32(k)) * s).to_f32());
        if m.i16(k.wrapping_add(0xe)) > 1 && m.u8(k.wrapping_add(0xc)) & 1 != 0 {
            if s.eq(F80::ZERO) {
                m.set_f32(k.wrapping_add(8), 100000.0);
                m.set_f32(k.wrapping_add(0x18), 100000.0);
            } else {
                for off in [8u32, 0x18] {
                    let v = (x(m.f32(k.wrapping_add(off))) / s).to_f32();
                    m.set_f32(k.wrapping_add(off), v);
                }
            }
        }
        j += 1;
    }
}

/// [soft 0x081e42c4 si_actionsKeepTiming]
///
/// The actions and masters of an animation file after the curves' times were scaled by
/// `scale`: an action's start and end t become (t − ½ / (rate × scale)) × scale + ½ / rate
/// (half a frame kept at both scales), its clip times and kept keys' times are scaled (see
/// below), a master's start, end and keys simply scaled. Quirk of the original: an
/// action's first kept key is scaled twice (once alone, then with all the others).
pub fn actions_keep_timing(m: &mut Mem, file: u32, scale: f32) {
    if file == 0 {
        return;
    }
    let d = m.u32(file.wrapping_add(4));
    if d == 0 {
        return;
    }
    let (acts, masters) = lists(d);
    let s = x(scale);
    if 0 < m[acts].count {
        let r = x(m[g::SI_FRAME_RATE]);
        let h = x(0.5f64);
        let q1 = h / r;
        let q2 = h / (r * s);
        let mut i = 0;
        while i < m[acts].count {
            let e = elem(m, acts, i);
            m[e].start = ((x(m[e].start) - q2) * s + q1).to_f32();
            m[e].end = ((x(m[e].end) - q2) * s + q1).to_f32();
            m[e].clip_start = (x(m[e].clip_start) * s).to_f32();
            m[e].clip_end = (x(m[e].clip_end) * s).to_f32();
            if 0 < m[e].nb_keys {
                let k = m[e].keys.addr();
                m.set_f32(k, (x(m.f32(k)) * s).to_f32());
                scale_keys(m, e, s);
            }
            i += 1;
        }
    }
    let mut i = 0;
    while i < m[masters].count {
        let e = elem(m, masters, i);
        m[e].start = (x(m[e].start) * s).to_f32();
        m[e].end = (x(m[e].end) * s).to_f32();
        scale_keys(m, e, s);
        i += 1;
    }
}

/// [soft 0x081dfaec SI_actionsInitPathAnim]
///
/// After a load: every action whose path data has no element yet (−1 at +0) and a name
/// (+0xc) gets its path's element in chapter `SI_cid[8]` (`SI_elemdictFindElemByPrjName`
/// with the names at +0x10 and +0xc) and, when found, the element of the second pair of
/// names (+0x18, +0x14) at +4. When either is not found the four names and the path data
/// are freed and the action has no path.
pub fn actions_init_path_anim(m: &mut Mem, o: &mut dyn Outside, dict: u32, a: Ptr32<Actions>) {
    if a.is_null() {
        return;
    }
    let mut i = 0;
    while i < m[a].count {
        let path = |m: &Mem| m.u32(elem(m, a, i).addr().wrapping_add(0x28));
        let p = path(m);
        if p != 0 && m.i32(p) == -1 && m.u32(p.wrapping_add(0xc)) != 0 {
            let find = |m: &mut Mem, o: &mut dyn Outside, n1: u32, n2: u32| {
                let p = path(m);
                let (a1, a2) = (m.u32(p.wrapping_add(n1)), m.u32(p.wrapping_add(n2)));
                let c8 = m[g::SI_CID][8] as u32;
                o.call(m, &out::SI_ELEMDICT_FIND_ELEM_BY_PRJ_NAME, &[dict, c8, 0, a1, a2, 0, 0xffff_ffff, 1])
            };
            let r = find(m, o, 0x10, 0xc);
            let p = path(m);
            m.set_u32(p, r);
            if m.i32(path(m)) != -1 {
                let r = find(m, o, 0x18, 0x14);
                let p = path(m);
                m.set_u32(p.wrapping_add(4), r);
            }
            let p = path(m);
            if m.i32(p) == -1 || m.i32(p.wrapping_add(4)) == -1 {
                for off in [0xcu32, 0x10, 0x14, 0x18] {
                    let q = m.u32(path(m).wrapping_add(off));
                    if q != 0 {
                        m.free(q);
                        let p = path(m);
                        m.set_u32(p.wrapping_add(off), 0);
                    }
                }
                let p = path(m);
                if p != 0 {
                    m.free(p);
                    let e = elem(m, a, i);
                    m.set_u32(e.addr().wrapping_add(0x28), 0);
                }
            }
        }
        i += 1;
    }
}

/// [soft 0x081e6c08 SI_actionsGetMasterFrmHA]
///
/// Makes (or refreshes) a master action from the animation the hierarchy record `ha` holds
/// for the action `idx` (its records are `ha` +0xc/+0x10 then +0x14/+0x18, 0x30 bytes each),
/// and answers its number (−1 for a null `ha` or `idx` outside both lists). The latest last
/// key time of the whole hierarchy (`hieranimFcvProcess` with `haFcvProcessLastKeyTime`)
/// gives the master's start: (nearest frame of it + 1.5) / frame rate. The action's length
/// is taken in seconds. For each entry of `ha` (+4, 0x28 bytes: curve count at +8, curve
/// list +0x18, in-use flags +0x1c; with a non-zero `flag34` only the first entry is taken
/// and `chapter` is its element number, else the element is looked up with
/// `SI_elemdictGetElemFromHierAnim`; with a non-zero `flag32` an element whose half word
/// +0x42 is 0 is passed over) each curve in use that has keys in the action's window
/// (`SI_fcvCntKeysBetweenTimes`; for the types 7, 9, 0x1f, 0xa, 0x20 and 0xac the element
/// must also have a related element) is found among the element's curves (`SI_edFindFcv`,
/// `SI_edFindNextFcv`, by type) or made (`SI_fcurveAllocate`, `SI_edFcvAdd`), and its keys
/// copied (`SI_fcvCopyKeysBetweenTimes`). Then the animation file's element related by the
/// range 0x3a98..0x3aa2 is stored through `out_elem`; if its data holds the lists, the
/// master `res` is taken (a new one, from `SI_calloc` or `SI_realloc`, named by
/// `SI_actionsGetNewName`, when `res` is not one) or refreshed (renamed, and the actions that
/// held its name given the new one, when the names differ), given the start, the end, one
/// cycle's flags, and the curves' keys of the action taken off (`haFcvProcessActionDeact`);
/// a master that existed has its instances made again (`SI_actionsMasterUpdateInst`).
#[allow(clippy::too_many_arguments)]
pub fn actions_get_master_frm_ha(m: &mut Mem, o: &mut dyn Outside, dict: u32, ha: u32, out_elem: u32, chapter: u32, idx: i32, res: i32, flag32: i16, flag34: i16) -> i32 {
    const ATD: u32 = 0x0878_1438;
    let sp = m.sp();
    let (l24, l28, l2c, l30) = (m.local(4), m.local(4), m.local(4), m.local(4));
    let r = (|| {
        let mut res = res;
        if ha == 0 || idx < 0 || idx >= m.i32(ha.wrapping_add(0x14)).wrapping_add(m.i32(ha.wrapping_add(0xc))) {
            return -1;
        }
        let c8 = m.u32(ha.wrapping_add(8));
        let base = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[dict, c8, chapter]);
        let na = m.i32(ha.wrapping_add(0xc));
        let l4c = if idx < na {
            m.u32(ha.wrapping_add(0x10)).wrapping_add((idx as u32).wrapping_mul(0x30))
        } else {
            m.u32(ha.wrapping_add(0x18)).wrapping_add(((idx - na) as u32).wrapping_mul(0x30))
        };
        m.set_u32(l24, 0);
        walk::hieranim_fcv_process(m, o, dict, step::LAST_KEY_TIME, base, l24, -1, 0);
        let t = x(m.f32(l24));
        let rate = x(m[g::SI_FRAME_RATE]);
        let f = nearest(t * rate, t);
        m.set_f32(l24, ((F80::from(f) + x(1.5f64)) / rate).to_f32());
        let len = ac::actions_elem_get_length(m, Ptr32::new(l4c));
        let l50 = (x(len as f32) / rate).to_f32();
        let start = m.f32(l4c.wrapping_add(8));
        let win_end = (x(l50) + x(start)).to_f32();
        let entries = m.u32(ha.wrapping_add(4));
        let mut i = 0i32;
        while i < m.i32(ha) {
            let ent = entries.wrapping_add((i as u32).wrapping_mul(40));
            let e = if flag34 != 0 {
                chapter
            } else {
                o.call(m, &out::SI_ELEMDICT_GET_ELEM_FROM_HIER_ANIM, &[dict, ha, ent, 0, chapter])
            };
            let c8 = m.u32(ha.wrapping_add(8));
            let l40 = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[dict, c8, e]);
            i += 1;
            if l40 != 0 && !(flag32 != 0 && m.u16(l40.wrapping_add(0x42)) == 0) {
                let mut j = 0i32;
                while j < m.i32(ent.wrapping_add(8)) {
                    let used = m.u16(m.u32(ent.wrapping_add(0x1c)).wrapping_add((j as u32).wrapping_mul(2)));
                    let jj = j as u32;
                    j += 1;
                    if used == 0 {
                        continue;
                    }
                    let fcv = m.u32(m.u32(ent.wrapping_add(0x18)).wrapping_add(jj.wrapping_mul(4)));
                    let ty = m.u32(fcv);
                    o.call(m, &out::SI_ATTDICT_GET_INFO_FROM_FCURVE, &[ATD, ty, l30, l2c, l28, l28]);
                    if matches!(m.u32(fcv), 7 | 9 | 0x1f | 0xa | 0x20 | 0xac) {
                        let g30 = m.u32(l30);
                        let rel = o.call(m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, l40, 1, 1, g30, 0xffff_ffff, l28, l28]);
                        if rel == 0 {
                            continue;
                        }
                    }
                    let n = o.call(m, &out::SI_FCV_CNT_KEYS_BETWEEN_TIMES, &[fcv, start.to_bits(), win_end.to_bits()]) as i32;
                    if n == 0 {
                        continue;
                    }
                    let ty = m.u32(fcv);
                    o.call(m, &out::SI_ATTDICT_GET_INFO_FROM_FCURVE, &[ATD, ty, l30, l2c, l28, l28]);
                    let (g2c, g30) = (m.u32(l2c), m.u32(l30));
                    let mut found = o.call(m, &out::SI_ED_FIND_FCV, &[l40, g2c, g30]);
                    while found != 0 && m.u32(found) != m.u32(fcv) {
                        let (g2c, g30) = (m.u32(l2c), m.u32(l30));
                        found = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[l40, g2c, g30]);
                    }
                    if found == 0 {
                        let data = m.u32(l40.wrapping_add(4));
                        let ty = m.u32(fcv);
                        found = o.call(m, &out::SI_FCURVE_ALLOCATE, &[ty, data]);
                        let c = m.u32(ha.wrapping_add(8));
                        let data = m.u32(l40.wrapping_add(4));
                        let id = o.call(m, &out::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, c, data]);
                        o.call(m, &out::SI_ED_FCV_ADD, &[found, l40, c, id]);
                        if found == 0 {
                            continue;
                        }
                    }
                    let k0 = m.u32(l24);
                    o.call(m, &out::SI_FCV_COPY_KEYS_BETWEEN_TIMES, &[found, fcv, k0, start.to_bits(), win_end.to_bits(), 1.0f32.to_bits()]);
                }
            }
            if i >= m.i32(ha) {
                break;
            }
            if flag34 != 0 && i > 0 {
                break;
            }
        }
        let cid0 = m[g::SI_CID][0] as u32;
        let r = o.call(m, &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM, &[dict, base, 1, 0, 0x3a98, 0x3aa2, cid0, l28, l28]);
        m.set_u32(out_elem, r);
        let (mut masters, mut acts) = (0u32, 0u32);
        if r != 0 {
            let d = m.u32(r.wrapping_add(4));
            if d != 0 {
                masters = d.wrapping_add(0x14);
                acts = d.wrapping_add(0xc);
            }
        }
        if masters == 0 {
            return res;
        }
        let ml = Ptr32::<Actions>::new(masters);
        let count = m.i32(masters);
        let mut update = None;
        let master;
        if res < 0 || res >= count {
            res = count;
            let p = if count == 0 {
                m.si_calloc(1, 0x30)
            } else {
                let old = m.u32(masters.wrapping_add(4));
                m.si_realloc(old, (res as u32).wrapping_mul(3).wrapping_add(3).wrapping_shl(4))
            };
            m.set_u32(masters.wrapping_add(4), p);
            master = (count as u32).wrapping_mul(0x30).wrapping_add(p);
            let name = m.u32(l4c);
            let nm = ac::actions_get_new_name(m, ml, name);
            m.set_u32(master, nm);
            m.set_u32(masters, (m.i32(masters) + 1) as u32);
        } else {
            master = (res as u32).wrapping_mul(0x30).wrapping_add(m.u32(masters.wrapping_add(4)));
            let ln = ac::actions_elem_get_length(m, Ptr32::new(master));
            update = Some(ln);
            if clib::strcmp(m, m.u32(l4c), m.u32(master)) != 0 {
                let name = m.u32(l4c);
                let newname = ac::actions_get_new_name(m, ml, name);
                let n = m.i32(acts);
                let mut k = 0u32;
                while (k as i32) < n {
                    let ent = k.wrapping_mul(0x30).wrapping_add(m.u32(acts.wrapping_add(4)));
                    let nm = m.u32(ent.wrapping_add(4));
                    if nm != 0 && clib::strcmp(m, nm, m.u32(master)) == 0 {
                        let old = m.u32(ent.wrapping_add(4));
                        if old != 0 {
                            m.free(old);
                            m.set_u32(ent.wrapping_add(4), 0);
                        }
                        let c = m.strdup(newname);
                        m.set_u32(ent.wrapping_add(4), c);
                    }
                    k += 1;
                }
                let old = m.u32(master);
                if old != 0 {
                    m.free(old);
                    m.set_u32(master, 0);
                }
                if m.i32(master.wrapping_add(0x20)) > 0 {
                    let kp = m.u32(master.wrapping_add(0x1c));
                    if kp != 0 {
                        m.free(kp);
                        m.set_u32(master.wrapping_add(0x1c), 0);
                    }
                }
                m.set_u32(master, newname);
            }
        }
        m.set_f32(master.wrapping_add(8), m.f32(l24));
        m.set_f32(master.wrapping_add(0xc), (x(l50) + x(m.f32(l24))).to_f32());
        m.set_u32(master.wrapping_add(0x20), 0);
        m.set_u32(master.wrapping_add(0x1c), 0);
        m.set_u32(master.wrapping_add(0x24), 1);
        let w = m.u16(l4c.wrapping_add(0x2c));
        m.set_u16(master.wrapping_add(0x2c), w);
        m.set_u32(master.wrapping_add(0x18), 0);
        m.set_u32(master.wrapping_add(4), 0);
        m.set_u32(master.wrapping_add(0x28), 0);
        walk::hieranim_fcv_process(m, o, dict, step::ACTION_DEACT, base, master, -1, 0);
        if let Some(ln) = update {
            let file = m.u32(out_elem);
            actions_master_update_inst(m, o, file, res, ln);
        }
        res
    })();
    m.set_sp(sp);
    r
}
