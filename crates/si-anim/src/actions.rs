// SPDX-License-Identifier: MIT
//! `actions.c`: action lists. An action list (`SI_Actions`, two words: a count and an
//! array) holds the actions of an animation file laid on the time line; each action
//! (`SI_ActionsElem`, 0x30 bytes) has a name, a time span in seconds, flags and the data
//! of its path, cycles and anchor keys (finding 0030 lays the record out).

use crate::globals as g;
use crate::outside::{self as out, Outside};
use si_mem::{clib, record, CStr32, Mem, Opaque, Ptr32};
use si_x87::{x, Round, F80};

record! {
    /// An action list (`SI_Actions`).
    pub struct Actions = "SI_Actions", size 0x08 {
        0x00 count: i32, "how many actions";
        0x04 elems: Ptr32<ActionsElem>, "the actions, sorted by start time (SI_actionsSort)";
    }
}

record! {
    /// One action (`SI_ActionsElem`, 0x30 bytes).
    pub struct ActionsElem = "SI_ActionsElem", size 0x30 {
        0x00 name: CStr32, "the action's name (a string of its own)";
        0x04 file: CStr32, "a second string of its own, 0 when absent; the two floats after it are copied only with it";
        0x08 start: f32, "start time in seconds";
        0x0c end: f32, "end time in seconds";
        0x10 clip_start: f32, "copied with the second string";
        0x14 clip_end: f32, "copied with the second string";
        0x18 flags: u32, "bit 0 selected; bit 2 to be modified (the dope sheet's drag)";
        0x1c keys: Ptr32<Opaque>, "entries of 36 bytes, `nb_keys` of them";
        0x20 nb_keys: i32, "how many";
        0x24 cycles: i32, "the action is repeated so many times (more than 1 divides its length)";
        0x28 path: Ptr32<ActionPath>, "the path data of the action (0x1c bytes), 0 for none";
        0x2c unk_2c: u16, "copied";
        0x2e pad_2e: [u8; 2], "never written";
    }
}

record! {
    /// The path data of an action (0x1c bytes from `SI_calloc`).
    pub struct ActionPath = "(path data of SI_ActionsElem)", size 0x1c {
        0x00 kind: i32, "−1: none";
        0x04 elem: i32, "the element number in chapter SI_cid[8]";
        0x08 fcurve: Ptr32<Opaque>, "the element's curve of type 0x70";
        0x0c unk_0c: [u32; 4], "not copied";
    }
}

/// The tolerance of the time comparisons, 1e-4 seconds (the doubles at 0x0864cb50 and
/// 0x0864cb18).
pub const TIME_EPS: f64 = 1e-4;

fn elem(m: &Mem, a: Ptr32<Actions>, i: i32) -> Ptr32<ActionsElem> {
    m[a].elems.add(i)
}

/// [soft 0x081df2f8 SI_actionsInit]
pub fn actions_init(m: &mut Mem, a: Ptr32<Actions>) {
    m[a].count = 0;
    m[a].elems = Ptr32::NULL;
}

/// [soft 0x081e6804 SI_actionsSecToStartFrm]
///
/// The first frame of a time: trunc(t × frame rate), plus one when t ≥ 0.
pub fn actions_sec_to_start_frm(m: &Mem, t: f32) -> i32 {
    let n = (x(t) * m[g::SI_FRAME_RATE]).to_i32(Round::Zero);
    if F80::ZERO.le(x(t)) {
        n.wrapping_add(1)
    } else {
        n
    }
}

/// [soft 0x081e6848 SI_actionsSecToEndFrm]
///
/// The last frame of a time: trunc(t × frame rate), less one when t < 0.
pub fn actions_sec_to_end_frm(m: &Mem, t: f32) -> i32 {
    let n = (x(t) * m[g::SI_FRAME_RATE]).to_i32(Round::Zero);
    if F80::ZERO.gt(x(t)) {
        n.wrapping_sub(1)
    } else {
        n
    }
}

/// [soft 0x081e55b0 SI_actionsElemGetTotLength]
///
/// An action's length in frames, cycles included: last frame − first frame + 1.
pub fn actions_elem_get_tot_length(m: &Mem, e: Ptr32<ActionsElem>) -> i32 {
    let end = actions_sec_to_end_frm(m, m[e].end);
    let start = actions_sec_to_start_frm(m, m[e].start);
    end.wrapping_sub(start).wrapping_add(1)
}

/// [soft 0x081e55f0 SI_actionsElemGetLength]
///
/// The length of one cycle: the whole length divided by the cycles when there are more
/// than one (C's division, toward zero).
pub fn actions_elem_get_length(m: &Mem, e: Ptr32<ActionsElem>) -> i32 {
    let t = actions_elem_get_tot_length(m, e);
    let c = m[e].cycles;
    if c > 1 {
        t.wrapping_div(c)
    } else {
        t
    }
}

/// [soft 0x081e4678 SI_actionsCountSelected]
pub fn actions_count_selected(m: &Mem, a: Ptr32<Actions>) -> i32 {
    if a.is_null() {
        return 0;
    }
    let mut n = 0;
    let count = m[a].count;
    if count > 0 {
        let mut f = m[a].elems.addr().wrapping_add(0x18);
        let mut left = count as u32;
        while left != 0 {
            if m.u8(f) & 1 != 0 {
                n += 1;
            }
            f = f.wrapping_add(0x30);
            left -= 1;
        }
    }
    n
}

/// [soft 0x081e27cc SI_actionsUnselectAll]
///
/// Every action unselected; the dope sheet then shows no list and no item (also when `a`
/// is null).
pub fn actions_unselect_all(m: &mut Mem, a: Ptr32<Actions>) {
    if !a.is_null() {
        let mut i = 0;
        while i < m[a].count {
            let f = m[a].elems.addr().wrapping_add((i as u32).wrapping_mul(0x30)).wrapping_add(0x18);
            m.and_u8(f, 0xfe);
            i += 1;
        }
    }
    m[g::SI_DOPE_SH][0x70 / 4] = 0;
    m[g::SI_DOPE_SH][0x6c / 4] = 0xffff_ffff;
}

/// [soft 0x081df8ec SI_actionsGetElemIdByName]
///
/// The number of the action of a name (`strcmp`), −1 for none or for no name.
pub fn actions_get_elem_id_by_name(m: &Mem, a: Ptr32<Actions>, name: u32) -> i32 {
    if name == 0 {
        return -1;
    }
    let mut i = 0;
    while i < m[a].count {
        let e = elem(m, a, i);
        if clib::strcmp(m, m[e].name.addr(), name) == 0 {
            break;
        }
        i += 1;
    }
    if i == m[a].count {
        -1
    } else {
        i
    }
}

/// [soft 0x081e28f0 SI_compareActions]
///
/// The order of two actions by start time, within 1e-4 s: 1 when the first starts later,
/// −1 when the second does, else 0.
pub fn compare_actions(m: &Mem, a: u32, b: u32) -> i32 {
    let (sa, sb) = (m.f32(a.wrapping_add(8)), m.f32(b.wrapping_add(8)));
    if (x(sa) - TIME_EPS).gt(x(sb)) {
        return 1;
    }
    if x(sb).gt(x(sa) + TIME_EPS) {
        -1
    } else {
        0
    }
}

/// [soft 0x081e2940 SI_actionsSort]
///
/// `qsort` of the actions by `SI_compareActions`.
pub fn actions_sort(m: &mut Mem, a: Ptr32<Actions>) {
    let (base, n) = (m[a].elems.addr(), m[a].count as u32);
    clib::qsort(m, base, n, 0x30, &mut |m, p, q| compare_actions(m, p, q));
}

/// [soft 0x081e0ea0 SI_actionsGetMinStart]
///
/// The start of the first action (of the first selected one when `selected`) through
/// `out`: 1, or 0 when there is none.
pub fn actions_get_min_start(m: &mut Mem, a: Ptr32<Actions>, selected: i16, outp: u32) -> i32 {
    if selected != 0 {
        let n = m[a].count;
        let mut i = 0;
        while i < n && m.u8(elem(m, a, i).addr() + 0x18) & 1 == 0 {
            i += 1;
        }
        if i >= n {
            return 0;
        }
        let v = si_x87::fmov(m[elem(m, a, i)].start);
        m.set_f32(outp, v);
    } else {
        if m[a].count <= 0 {
            return 0;
        }
        let v = si_x87::fmov(m[elem(m, a, 0)].start);
        m.set_f32(outp, v);
    }
    1
}

/// [soft 0x081e0f24 SI_actionsGetMaxEnd]
///
/// The end of the last action that is selected, through `out`: 1, or 0 when none is.
/// Without `selected` the original reads the end of the action whose number is in a
/// register it never set (the caller's `ebx`): the port takes that number from a local of
/// the image's stack that nobody wrote, so that it is as undefined as the original's.
pub fn actions_get_max_end(m: &mut Mem, a: Ptr32<Actions>, selected: i16, outp: u32) -> i32 {
    let i = if selected != 0 {
        let mut i = m[a].count.wrapping_sub(1);
        while i >= 0 && m.u8(elem(m, a, i).addr() + 0x18) & 1 == 0 {
            i -= 1;
        }
        if i < 0 {
            return 0;
        }
        i
    } else {
        let l = m.local(4);
        let i = m.i32(l);
        m.release(4);
        i
    };
    let v = si_x87::fmov(m[elem(m, a, i)].end);
    m.set_f32(outp, v);
    1
}

/// [soft 0x081e0fb8 SI_actionsModifyFromTime]
///
/// Marks (bit 2) the actions a drag at time `t` moves: those ending at most 1e-4 s after
/// `t` (side 0), or starting at least 1e-4 s before it (side 1); the others are unmarked.
pub fn actions_modify_from_time(m: &mut Mem, a: Ptr32<Actions>, t: f32, side: i16) {
    if a.is_null() {
        return;
    }
    let mut i = 0;
    while i < m[a].count {
        let e = elem(m, a, i);
        let mark = if side == 0 { x(m[e].end).le(x(TIME_EPS) + t) } else { x(m[e].start).ge(x(t) - TIME_EPS) };
        if mark {
            m.or_u8(e.addr() + 0x18, 4);
        } else {
            m.and_u8(e.addr() + 0x18, 0xfb);
        }
        i += 1;
    }
}

/// [soft 0x081e1040 SI_actionsModifySelected]
///
/// Marks the selected actions and unmarks the others.
pub fn actions_modify_selected(m: &mut Mem, a: Ptr32<Actions>) {
    if a.is_null() {
        return;
    }
    let mut i = 0;
    while i < m[a].count {
        let e = elem(m, a, i);
        let f = m[e].flags;
        m[e].flags = if f & 1 != 0 { f | 4 } else { f & !4 };
        i += 1;
    }
}

/// [soft 0x081e107c SI_actionsModifyAll]
pub fn actions_modify_all(m: &mut Mem, a: Ptr32<Actions>, on: i16) {
    if a.is_null() {
        return;
    }
    let mut i = 0;
    while i < m[a].count {
        let e = elem(m, a, i).addr() + 0x18;
        if on != 0 {
            m.or_u8(e, 4);
        } else {
            m.and_u8(e, 0xfb);
        }
        i += 1;
    }
}

/// [soft 0x081e3e50 actionsFreeElem]
///
/// Frees what every action holds: its two strings, its keys (when it has any), its path
/// data; the pointers are cleared.
pub fn actions_free_elem(m: &mut Mem, a: Ptr32<Actions>) {
    let mut i = 0;
    while i < m[a].count {
        let e = elem(m, a, i);
        for off in [0x00u32, 0x04] {
            let p = m.u32(e.addr() + off);
            if p != 0 {
                m.free(p);
                let e = elem(m, a, i);
                m.set_u32(e.addr() + off, 0);
            }
        }
        let e = elem(m, a, i);
        if !m[e].keys.is_null() && m[e].nb_keys > 0 {
            let p = m[e].keys.addr();
            m.free(p);
            let e = elem(m, a, i);
            m[e].keys = Ptr32::NULL;
        }
        let e = elem(m, a, i);
        let p = m[e].path.addr();
        if p != 0 {
            m.free(p);
            let e = elem(m, a, i);
            m[e].path = Ptr32::NULL;
        }
        i += 1;
    }
}

/// [soft 0x081df310 SI_actionsDispose]
///
/// Empties a list: the dope sheet's selection given up if it shows this list, every
/// action's data freed, the array freed, the count 0.
pub fn actions_dispose(m: &mut Mem, a: Ptr32<Actions>) {
    if m[g::SI_DOPE_SH][0x70 / 4] == a.addr() {
        actions_unselect_all(m, a);
    }
    actions_free_elem(m, a);
    let p = m[a].elems;
    if !p.is_null() {
        m.free(p.addr());
        m[a].elems = Ptr32::NULL;
    }
    m[a].count = 0;
}

/// [soft 0x081e5a14 SI_actionsElemCopy]
///
/// A copy of an action with strings, keys (room for the count rounded up to a multiple of
/// 8) and path data of its own; the path's curve is looked up again by its element. Fields
/// the source has no data for are left as they were in `dst`.
pub fn actions_elem_copy(m: &mut Mem, o: &mut dyn Outside, dst: Ptr32<ActionsElem>, src: Ptr32<ActionsElem>) {
    let s = m[src].name.addr();
    let p = m.strdup(s);
    m[dst].name = Ptr32::new(p);
    let f = m[src].file.addr();
    if f != 0 {
        let p = m.strdup(f);
        m[dst].file = Ptr32::new(p);
        m[dst].clip_start = si_x87::fmov(m[src].clip_start);
        m[dst].clip_end = si_x87::fmov(m[src].clip_end);
    } else {
        m[dst].file = Ptr32::NULL;
    }
    m[dst].start = si_x87::fmov(m[src].start);
    m[dst].end = si_x87::fmov(m[src].end);
    m[dst].flags = m[src].flags;
    let n = m[src].nb_keys;
    m[dst].nb_keys = n;
    if n > 0 {
        let rounded = (n as u32).wrapping_add(7).wrapping_sub((n as u32).wrapping_sub(1) & 7);
        let p = m.si_malloc(rounded.wrapping_mul(36));
        m[dst].keys = Ptr32::new(p);
        let len = (m[dst].nb_keys as u32).wrapping_mul(36);
        let from = m[src].keys.addr();
        clib::memcpy(m, p, from, len);
    }
    m[dst].unk_2c = m[src].unk_2c;
    m[dst].cycles = m[src].cycles;
    let sp = m[src].path;
    if !sp.is_null() && m[sp].kind != -1 {
        if m[dst].path.is_null() {
            let p = m.si_calloc(1, 0x1c);
            m[dst].path = Ptr32::new(p);
        }
        let (sp, dp) = (m[src].path, m[dst].path);
        m[dp].kind = m[sp].kind;
        let (sp, dp) = (m[src].path, m[dst].path);
        m[dp].elem = m[sp].elem;
        let id = m[m[src].path].elem as u32;
        let cid8 = m[g::SI_CID][8] as u32;
        let dict = g::cur_dict(m);
        let e = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[dict, cid8, id]);
        let fc = o.call(m, &out::SI_ED_FIND_FCV, &[e, 1, 0x70]);
        let dp = m[dst].path;
        m[dp].fcurve = Ptr32::new(fc);
    }
}

/// [soft 0x081e2810 SI_actionsCopyAll]
///
/// `dst` emptied and made a copy of `src` (an array from `SI_calloc`, each action copied).
pub fn actions_copy_all(m: &mut Mem, o: &mut dyn Outside, dst: Ptr32<Actions>, src: Ptr32<Actions>) {
    actions_dispose(m, dst);
    m[dst].count = m[src].count;
    let p = m.si_calloc(m[src].count as u32, 0x30);
    m[dst].elems = Ptr32::new(p);
    if p == 0 {
        return;
    }
    let mut i = 0;
    while i < m[src].count {
        let (d, s) = (elem(m, dst, i), elem(m, src, i));
        actions_elem_copy(m, o, d, s);
        i += 1;
    }
}

// ---------------------------------------------------------------- second pass

/// [soft 0x081df6d4 SI_actionsValidateElem]
///
/// Whether an action from frame `start` to frame `end` in `cycles` cycles fits the list:
/// its length must be a multiple of the cycles (when there are more than one: the integer
/// quotient held against the quotient in the register), and no other action (all but the
/// one numbered `skip`) may overlap it: neither of its ends may lie within another's frames
/// (`SI_actionsSecToStartFrm`, `SI_actionsSecToEndFrm`), nor another's first frame within
/// its own. 1 or 0, as a `short`.
pub fn actions_validate_elem(m: &mut Mem, a: Ptr32<Actions>, start: i32, end: i32, cycles: i32, skip: i32) -> i16 {
    let mut ok: i16 = 1;
    if cycles > 1 {
        let len = end.wrapping_sub(start).wrapping_add(1);
        let q = len.wrapping_div(cycles);
        ok = F80::from(q).eq(F80::from(len) / F80::from(cycles)) as i16;
    }
    let mut i = 0;
    if i >= m[a].count || ok == 0 {
        return ok;
    }
    let mut e = m[a].elems.addr();
    let mut off = 0u32;
    loop {
        if skip != i {
            let s = actions_sec_to_start_frm(m, m.f32(e.wrapping_add(8)));
            let n = actions_sec_to_end_frm(m, m.f32(e.wrapping_add(0xc)));
            if (s <= start && start <= n) || (s <= end && end <= n) || (start <= s && s <= end) {
                ok = 0;
            }
        }
        off = off.wrapping_add(0x30);
        i += 1;
        e = m[a].elems.addr().wrapping_add(off);
        if i >= m[a].count || ok == 0 {
            return ok;
        }
    }
}

/// [soft 0x081df3e4 SI_actionsAddElemTime]
///
/// Adds an action from `start` to `end` seconds in `cycles` cycles: the array grown by one
/// (`SI_calloc` for the first, `SI_realloc` after), the new action put before the first
/// whose start lies more than 1e-4 s after `end` (the ones after it moved up with
/// `memmove`), with a copy of `name` or, for none, a name of its own
/// (`SI_actionsGetNewName`), the flags given, the word at +0x30 of `SI_glbAnim`, no keys,
/// second string or path. Its number, or −1 when `start` lies after `end`, the cycles are
/// not positive, or no memory is left (the list is then emptied: count 0). The clip times
/// (+0x10, +0x14) are not written.
pub fn actions_add_elem_time(m: &mut Mem, a: Ptr32<Actions>, name: u32, start: f32, end: f32, cycles: i32, flags: i32) -> i32 {
    if x(start).gt(x(end)) || cycles <= 0 {
        return -1;
    }
    let p = if m[a].elems.is_null() {
        m.si_calloc(1, 0x30)
    } else {
        let n = (m[a].count as u32).wrapping_add(1).wrapping_mul(0x30);
        m.si_realloc(m[a].elems.addr(), n)
    };
    m[a].elems = Ptr32::new(p);
    if m[a].elems.is_null() {
        m[a].count = 0;
        return -1;
    }
    let count = m[a].count;
    let mut i = 0;
    if count > 0 {
        loop {
            let s = m[elem(m, a, i)].start;
            if (x(TIME_EPS) + s).gt(x(end)) {
                break;
            }
            i += 1;
            if i >= count {
                break;
            }
        }
        let count = m[a].count;
        if count > i {
            let base = m[a].elems.addr();
            let at = base.wrapping_add((i as u32).wrapping_mul(0x30));
            clib::memmove(m, at.wrapping_add(0x30), at, (count.wrapping_sub(i) as u32).wrapping_mul(0x30));
        }
    }
    let e = elem(m, a, i);
    let nm = if name == 0 { actions_get_new_name(m, a, 0) } else { m.strdup(name) };
    m.set_u32(e.addr(), nm);
    m[e].start = si_x87::fmov(start);
    m[e].end = si_x87::fmov(end);
    m[e].flags = flags as u32;
    m[e].keys = Ptr32::NULL;
    m[e].nb_keys = 0;
    m[e].unk_2c = m[g::SI_GLB_ANIM][0x30 / 2];
    m[e].cycles = cycles;
    m[e].file = Ptr32::NULL;
    m[e].path = Ptr32::NULL;
    m[a].count += 1;
    i
}

/// [soft 0x081df360 SI_actionsAddElem]
///
/// Adds an action from frame `start` to frame `end`: its times are (start − ½) / rate and
/// (end + ½) / rate; −1 when `start` lies after `end` or the action does not fit
/// (`SI_actionsValidateElem`), else what `SI_actionsAddElemTime` answers.
pub fn actions_add_elem(m: &mut Mem, a: Ptr32<Actions>, name: u32, start: i32, end: i32, cycles: i32, flags: i32) -> i32 {
    let h = x(0.5f64);
    let ts = ((F80::from(start) - h) / x(m[g::SI_FRAME_RATE])).to_f32();
    let te = ((F80::from(end) + h) / x(m[g::SI_FRAME_RATE])).to_f32();
    if start > end || actions_validate_elem(m, a, start, end, cycles, -1) == 0 {
        return -1;
    }
    actions_add_elem_time(m, a, name, ts, te, cycles, flags)
}

/// `sprintf(buf, "%s_%d" or "%s%d", s, n)` for [`actions_get_new_name`].
fn put_name(m: &mut Mem, buf: u32, s: u32, sep: bool, n: Option<i32>) {
    let mut text = m.cstr(s);
    if let Some(n) = n {
        if sep {
            text.push(b'_');
        }
        text.extend(clib::decimal(n));
    }
    m.put_cstr(buf, &text);
}

/// [soft 0x081e3f0c SI_actionsGetNewName]
///
/// A name for a new action that no action of the list has, in a block of its own (0 for no
/// list). With a name: a number after its last `_` (not at its start) is taken off when it
/// is positive, and the name becomes `name_N`, N one more than the highest number an
/// action named after it carries (`name_K` or `nameK`; `strtol`), or than the number it
/// carried itself, or stays as it is when that is not positive. Without one: `actionN`,
/// N at least the list's count plus one.
pub fn actions_get_new_name(m: &mut Mem, a: Ptr32<Actions>, name: u32) -> u32 {
    if a.is_null() {
        return 0;
    }
    let buf = m.local(0x100);
    let s;
    let mut n: i32;
    if name != 0 {
        s = m.strdup(name);
        let mut i = clib::strlen(m, s) as i32 - 1;
        n = 0;
        if i >= 0 {
            while m.u8(s.wrapping_add(i as u32)) != b'_' {
                i -= 1;
                if i < 0 {
                    break;
                }
            }
            if i > 0 {
                n = clib::strtol(m, s.wrapping_add(i as u32 + 1), 0, 10);
                if n > 0 {
                    m.set_u8(s.wrapping_add(i as u32), 0);
                }
            }
        }
    } else {
        n = m[a].count.wrapping_add(1);
        // strdup("action"): the string lies in the binary's read-only data, at 0x086949ab
        s = m.malloc(7);
        if s != 0 {
            m.put_cstr(s, b"action");
        }
    }
    let len = clib::strlen(m, s);
    let mut i = 0;
    while i < m[a].count {
        let nm = m[elem(m, a, i)].name.addr();
        if clib::strlen(m, nm) >= len && clib::strncmp(m, nm, s, len) == 0 {
            let mut p = nm.wrapping_add(len);
            if m.u8(p) == b'_' {
                p = p.wrapping_add(1);
            }
            let v = clib::strtol(m, p, 0, 10).wrapping_add(1);
            if v > n {
                n = v;
            }
        }
        i += 1;
    }
    if name != 0 {
        put_name(m, buf, s, true, (n > 0).then_some(n));
    } else {
        put_name(m, buf, s, false, Some(n));
    }
    if s != 0 {
        m.free(s);
    }
    let r = m.strdup(buf);
    m.release(0x100);
    r
}

/// [soft 0x081e4488 SI_actionsOffsetAndScaleDef]
///
/// Scales the stretch of the time line from `start` to `end` about `pivot` by `scale` and
/// moves the whole by `offset`: an action time before `start` moves with `start` (by
/// P − start + offset, P = pivot + (start − pivot)·scale, unrounded), one at or after `end`
/// with `end` (by Q − end + offset, Q the same for `end`, stored as a float), and one in
/// between is scaled: a start time from half a frame before it, an end time from half a
/// frame after it. With a negative scale, every action then shorter than a frame has its
/// frames exchanged (start from its last frame less ½, end from its first plus ½), and the
/// list is sorted.
#[allow(clippy::too_many_arguments)]
pub fn actions_offset_and_scale_def(m: &mut Mem, a: Ptr32<Actions>, start: f32, pivot: f32, end: f32, offset: f32, scale: f32) {
    if a.is_null() {
        return;
    }
    let (s0, b, s1, d) = (x(start), x(pivot), x(end), x(offset));
    let p = (s0 - b) * scale + b;
    let q = ((s1 - b) * scale + b).to_f32();
    let h = x(0.5f64);
    let mut count = m[a].count;
    let mut i = 0;
    if i < count {
        let r = x(m[g::SI_FRAME_RATE]);
        loop {
            let e = elem(m, a, i);
            let v = x(m[e].start);
            let ns = if v.lt(s0) {
                v + ((p - s0) + d)
            } else if v.lt(s1) {
                let hr = h / r;
                ((((v + hr) - s0) * scale + p) + d) - hr
            } else {
                v + ((x(q) - s1) + d)
            };
            m[e].start = ns.to_f32();
            let v = x(m[e].end);
            let ne = if v.lt(s0) {
                v + ((p - s0) + d)
            } else if v.lt(s1) {
                let hr = h / r;
                ((((v - hr) - s0) * scale + p) + d) + hr
            } else {
                v + ((x(q) - s1) + d)
            };
            m[e].end = ne.to_f32();
            i += 1;
            count = m[a].count;
            if i >= count {
                break;
            }
        }
    }
    if F80::ZERO.gt(x(scale)) {
        let mut i = 0;
        if i < count {
            loop {
                let frame = F80::ONE / m[g::SI_FRAME_RATE];
                let e = elem(m, a, i);
                if (x(m[e].end) - m[e].start).lt(frame) {
                    let sf = actions_sec_to_start_frm(m, m[e].start);
                    let ef = actions_sec_to_end_frm(m, m[e].end);
                    let r = m[g::SI_FRAME_RATE];
                    m[e].start = ((F80::from(ef) - h) / r).to_f32();
                    m[e].end = ((F80::from(sf) + h) / r).to_f32();
                }
                i += 1;
                if i >= m[a].count {
                    break;
                }
            }
        }
        actions_sort(m, a);
    }
}

/// [soft 0x081dfa68 SI_actionsTmpActivateAll]
///
/// For a while, every action of a list turned off (`on` 0: each one whose bit 1 is clear
/// is deactivated through `SI_actionsElemActivate` with 1 and marked with bit
/// 3), or back on (`on` not 0: each one marked is activated with 0 and unmarked).
pub fn actions_tmp_activate_all(m: &mut Mem, o: &mut dyn Outside, ed: u32, a: Ptr32<Actions>, on: i16) {
    if a.is_null() {
        return;
    }
    let mut i = 0;
    while i < m[a].count {
        let e = elem(m, a, i);
        if on != 0 {
            if m.u8(e.addr() + 0x18) & 8 != 0 {
                crate::actedit::actions_elem_activate(m, o, ed, a, e, 0);
                let e = elem(m, a, i);
                m.and_u8(e.addr() + 0x18, 0xf7);
            }
        } else if m.u8(e.addr() + 0x18) & 2 == 0 {
            crate::actedit::actions_elem_activate(m, o, ed, a, e, 1);
            let e = elem(m, a, i);
            m.or_u8(e.addr() + 0x18, 8);
        }
        i += 1;
    }
}

// ---------------------------------------------------------------- the element look-ups

/// An element of a dictionary as these look-ups read it (0x84 bytes): +4 its data, +0x54 a
/// word that is 0 for an element in use (not established further).
const ELEM_SIZE: u32 = 0x84;
const ELEM_DATA: u32 = 0x04;
const ELEM_54: u32 = 0x54;
/// The action list lies at +0xc of an animation file's data.
const ANIMFILE_ACTIONS: u32 = 0x0c;

/// [soft 0x081df0d0 SI_edGetActions]
///
/// The action list of element `index` of chapter `chapter` of the current dictionary: the
/// list in the data of the first element related to it by a relation of types
/// 0x3a98–0x3aa2 in chapter `SI_cid[0]` (`SI_elemdictFindRangeRelatedElem`), at +0xc. 0
/// without the element, when its word at +0x54 is not 0, or without such a relation.
pub fn ed_get_actions(m: &mut Mem, o: &mut dyn Outside, chapter: i32, index: i32) -> u32 {
    let dict = g::cur_dict(m);
    let e = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[dict, chapter as u32, index as u32]);
    if e == 0 {
        return 0;
    }
    if m.u32(e.wrapping_add(ELEM_54)) != 0 {
        return 0;
    }
    let l = m.local(4);
    let cid0 = m[g::SI_CID][0] as u32;
    let dict = g::cur_dict(m);
    let r = o.call(m, &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM, &[dict, e, 1, 0, 0x3a98, 0x3aa2, cid0, l, l]);
    m.release(4);
    if r == 0 {
        0
    } else {
        m.u32(r.wrapping_add(ELEM_DATA)).wrapping_add(ANIMFILE_ACTIONS)
    }
}

/// The chapter `SI_cid[0]` of a dictionary (0x40 bytes a chapter from its word at +4), when
/// the dictionary has that many (its first word).
fn anim_chapter(m: &Mem, ed: u32) -> u32 {
    let cid0 = m[g::SI_CID][0];
    if ed != 0 && cid0 < m.i32(ed) {
        m.u32(ed.wrapping_add(4)).wrapping_add((cid0 as u32) << 6)
    } else {
        0
    }
}

/// The place of the animation file element whose action list is `a` in the chapter: the
/// first element in use with data whose list (+0xc) is `a`; `None` when none is (or the
/// chapter is empty).
fn anim_file_place(m: &Mem, chap: u32, a: u32) -> Option<(i32, u32)> {
    let n = m.i32(chap.wrapping_add(4));
    if n <= 0 {
        return None;
    }
    let elems = m.u32(chap.wrapping_add(8));
    let mut i = 0;
    while i < n {
        let el = elems.wrapping_add((i as u32).wrapping_mul(ELEM_SIZE));
        let data = m.u32(el.wrapping_add(ELEM_DATA));
        if m.u32(el.wrapping_add(ELEM_54)) == 0 && data != 0 && data.wrapping_add(ANIMFILE_ACTIONS) == a {
            return Some((i, el));
        }
        i += 1;
    }
    None
}

/// [soft 0x081df164 SI_edGetAnimFileFromActions]
///
/// The animation file element (chapter `SI_cid[0]` of `ed`) whose action list is `a`; 0
/// for none.
pub fn ed_get_anim_file_from_actions(m: &Mem, ed: u32, a: u32) -> u32 {
    if a == 0 {
        return 0;
    }
    let chap = anim_chapter(m, ed);
    if chap == 0 {
        return 0;
    }
    anim_file_place(m, chap, a).map_or(0, |(_, el)| el)
}

/// [soft 0x081df1e4 SI_edGetAnimFileIdFromActions]
///
/// Its number in the chapter; −1 for none. No caller in the program.
pub fn ed_get_anim_file_id_from_actions(m: &Mem, ed: u32, a: u32) -> i32 {
    if a == 0 {
        return -1;
    }
    let chap = anim_chapter(m, ed);
    if chap == 0 {
        return -1;
    }
    anim_file_place(m, chap, a).map_or(-1, |(i, _)| i)
}

/// [soft 0x081df264 SI_edGetRootElemFromAnimFile]
///
/// The root element an animation file animates: `*first` gets the word at +8 of the file's
/// data, then the first element related to the file by a relation of types 0x3a98–0x3aa2
/// whose word at +0x54 is 0 (`SI_elemdictFindRangeRelatedElem`, then `…NextRelatedElem`;
/// any chapter; the index through a local, the chapter through `chapter`). 0 without a file
/// or data, or when none is found.
pub fn ed_get_root_elem_from_anim_file(m: &mut Mem, o: &mut dyn Outside, ed: u32, file: u32, first: u32, chapter: u32) -> u32 {
    if file == 0 || m.u32(file.wrapping_add(ELEM_DATA)) == 0 {
        return 0;
    }
    let v = m.u32(m.u32(file.wrapping_add(ELEM_DATA)).wrapping_add(8));
    m.set_u32(first, v);
    let l = m.local(4);
    let args = [ed, file, 0, 0, 0x3a98, 0x3aa2, 0xffff_ffff, l, chapter];
    let mut r = o.call(m, &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM, &args);
    while r != 0 && m.u32(r.wrapping_add(ELEM_54)) != 0 {
        r = o.call(m, &out::SI_ELEMDICT_FIND_RANGE_NEXT_RELATED_ELEM, &args);
    }
    m.release(4);
    r
}

/// [soft 0x081df5bc SI_actionsDelDefElem]
///
/// Takes action `i` out of the array: its strings, keys (when it has any) and path data
/// freed, the actions after it moved down (`memmove`; nothing moves for `i` outside the
/// list), the count lowered by one, the array made smaller (`SI_realloc`) or freed when
/// empty.
pub fn actions_del_def_elem(m: &mut Mem, a: Ptr32<Actions>, i: i32) {
    let at = |m: &Mem| m[a].elems.addr().wrapping_add((i as u32).wrapping_mul(0x30));
    for off in [0x00u32, 0x04] {
        let p = m.u32(at(m).wrapping_add(off));
        if p != 0 {
            m.free(p);
            let e = at(m);
            m.set_u32(e.wrapping_add(off), 0);
        }
    }
    let e = at(m);
    if m.u32(e.wrapping_add(0x1c)) != 0 && m.i32(e.wrapping_add(0x20)) > 0 {
        m.free(m.u32(e.wrapping_add(0x1c)));
        let e = at(m);
        m.set_u32(e.wrapping_add(0x1c), 0);
    }
    let p = m.u32(at(m).wrapping_add(0x28));
    if p != 0 {
        m.free(p);
        let e = at(m);
        m.set_u32(e.wrapping_add(0x28), 0);
    }
    let count = m[a].count;
    if i >= 0 && i < count - 1 {
        let e = at(m);
        clib::memmove(m, e, e.wrapping_add(0x30), ((count - i) as u32).wrapping_mul(0x30).wrapping_sub(0x30));
    }
    m[a].count -= 1;
    let n = m[a].count;
    if n > 0 {
        let p = m.si_realloc(m[a].elems.addr(), (n as u32).wrapping_mul(0x30));
        m[a].elems = Ptr32::new(p);
    } else if !m[a].elems.is_null() {
        m.free(m[a].elems.addr());
        m[a].elems = Ptr32::NULL;
    }
}

// ---------------------------------------------------------------- selection and checks

/// [soft 0x081df7dc SI_actionsValidateAllStartEndFrames]
///
/// Marks (bit 4) every action whose first frame lies after its last, and every two actions
/// whose frames overlap (an end of one within the other's frames, or the second's first
/// frame within the first's): 1 when none was marked (as a `short`), 0 for no list. Only
/// `softForceSymbols` calls it.
pub fn actions_validate_all_start_end_frames(m: &mut Mem, a: Ptr32<Actions>) -> i16 {
    if a.is_null() {
        return 0;
    }
    let mut ok: i16 = 1;
    let mut i = 0;
    while i < m[a].count {
        let e = elem(m, a, i);
        let si = actions_sec_to_start_frm(m, m[e].start);
        let ei = actions_sec_to_end_frm(m, m[e].end);
        if si > ei {
            m.or_u8(e.addr() + 0x18, 0x10);
        }
        let mut j = i + 1;
        while j < m[a].count {
            let f = elem(m, a, j);
            let sj = actions_sec_to_start_frm(m, m[f].start);
            let ej = actions_sec_to_end_frm(m, m[f].end);
            if (sj <= si && si <= ej) || (sj <= ei && ei <= ej) || (si <= sj && sj <= ei) {
                m.or_u8(e.addr() + 0x18, 0x10);
                m.or_u8(f.addr() + 0x18, 0x10);
            }
            j += 1;
        }
        if m.u8(e.addr() + 0x18) & 0x10 != 0 {
            ok = 0;
        }
        i += 1;
    }
    ok
}

/// [soft 0x081e0780 actionsConfirmMergeOnlyOnePath]
///
/// Whether at most one selected action has path data: 1 or 0 (0 for no list).
pub fn actions_confirm_merge_only_one_path(m: &Mem, a: Ptr32<Actions>) -> i32 {
    if a.is_null() {
        return 0;
    }
    let count = m[a].count;
    let mut found = false;
    let mut i = 0;
    while i < count {
        let e = elem(m, a, i);
        if m.u8(e.addr() + 0x18) & 1 != 0 && !m[e].path.is_null() {
            if found {
                return 0;
            }
            found = true;
        }
        i += 1;
    }
    1
}

/// [soft 0x081e07d0 SI_actionsSelectedConsec]
///
/// Whether the selected actions follow one another: 1 when there is a selected one and
/// none after the first gap in their run, 0 otherwise (as a `short`; 0 for no list).
pub fn actions_selected_consec(m: &Mem, a: Ptr32<Actions>) -> i16 {
    if a.is_null() {
        return 0;
    }
    let (mut seen, mut first): (i16, i16) = (0, 1);
    let count = m[a].count;
    let mut i = 0;
    while i < count {
        let sel = m.u8(elem(m, a, i).addr() + 0x18) & 1 != 0;
        if first != 0 && sel {
            if seen == 0 {
                seen = 1;
            }
        } else if seen != 0 {
            if first != 0 {
                first = 0;
            } else if sel {
                return 0;
            }
        }
        i += 1;
    }
    seen
}

/// [soft 0x081e2884 SI_actionsGetDopeShItemId]
///
/// The dope sheet's item (`SI_dopeSh`: +0 their number, +4 the array) whose element's action
/// list (`SI_edGetActions` of its words at +0x10 and +0x14) is `a`; −1 for none or no list.
pub fn actions_get_dope_sh_item_id(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>) -> i32 {
    if a.is_null() {
        return -1;
    }
    let mut i: i32 = 0;
    while i < m[g::SI_DOPE_SH][0] as i32 {
        let item = m.u32(m[g::SI_DOPE_SH][1].wrapping_add((i as u32).wrapping_mul(4)));
        if item != 0 {
            let (c, n) = (m.i32(item.wrapping_add(0x10)), m.i32(item.wrapping_add(0x14)));
            if ed_get_actions(m, o, c, n) == a.addr() {
                break;
            }
        }
        i += 1;
    }
    if i == m[g::SI_DOPE_SH][0] as i32 {
        -1
    } else {
        i
    }
}

/// [soft 0x081e46b8 SI_actionsElemSelect]
///
/// Selects (`on`) or unselects action `e` of list `a`: the list the dope sheet showed is
/// unselected first when it is another, and so is this one unless `add` (then only the
/// dope sheet's item is given up); the dope sheet then shows `a`, its 16-bit word at +8 is
/// cleared, and the audio markers are unselected (`SI_audioMarkerMngUnselectAll` of the
/// track at +0x7c of `SI_audioContainer`, which is cleared).
pub fn actions_elem_select(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>, e: Ptr32<ActionsElem>, on: i16, add: i16) {
    let shown = m[g::SI_DOPE_SH][0x70 / 4];
    if a.addr() != shown {
        actions_unselect_all(m, Ptr32::new(shown));
    } else if add == 0 {
        actions_unselect_all(m, a);
    } else {
        m[g::SI_DOPE_SH][0x6c / 4] = 0xffff_ffff;
    }
    if on != 0 {
        m.or_u8(e.addr() + 0x18, 1);
    } else {
        m.and_u8(e.addr() + 0x18, 0xfe);
    }
    let track = m[g::SI_AUDIO_CONTAINER][0x7c / 4];
    m[g::SI_DOPE_SH][0x70 / 4] = a.addr();
    m.set_u16(g::SI_DOPE_SH.0 + 8, 0);
    o.call(m, &out::SI_AUDIO_MARKER_MNG_UNSELECT_ALL, &[track]);
    m[g::SI_AUDIO_CONTAINER][0x7c / 4] = 0;
}
