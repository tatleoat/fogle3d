// SPDX-License-Identifier: MIT
//! `actions.c`, eighth pass: the dope sheet's paste onto the associated elements and the
//! drag from the buffer (finding 0030, section 16). Records are addressed by named offsets
//! (this module reads them from code; no layout is established beyond what the notes say).
//!
//! A *selection* (what `SI_actionsPasteToAssElems` is handed): +0 a count of 40-byte entries,
//! +4 their array, +8 a chapter, +0xc and +0x14 the counts of two arrays of 48-byte actions
//! (+0x10 and +0x18). An entry: +8 the number of curves, +0x18 the array of curve records
//! (the first word of a record the curve's type), +0x1c an array of 16-bit flags telling
//! which of them to use.

use crate::actions::{self as ac, Actions};
use crate::globals as g;
use crate::outside::{self as out, Outside};
use crate::walk::{self, step};
use si_mem::Mem;
use si_x87::{x, Round, F80};

/// The attribute dictionary `SI_ATD` (0x08781438; the callee reads it).
const ATD: u32 = 0x0878_1438;

/// Seconds to a frame count the way the paste does it: `trunc(t × frame rate ± 0.5)`, plus
/// when the time is positive, minus otherwise (the product stays an extended value).
fn round_frames(m: &Mem, t: f32) -> i32 {
    let p = x(t) * m[g::SI_FRAME_RATE];
    let adj = if F80::ZERO.lt(x(t)) { p + 0.5f64 } else { p + (-0.5f64) };
    adj.to_i32(Round::Zero)
}

fn find_related(m: &mut Mem, o: &mut dyn Outside, elem: u32, group: u32, ty: u32, chapter: u32, p: u32) -> u32 {
    let d = g::cur_dict(m);
    o.call(m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, elem, 1, group, ty, chapter, p, p])
}

/// `SI_actionsPasteToAssElems` [soft 0x081e10bc SI_actionsPasteToAssElems] (emulator check: finding 0030, section 16)
///
/// `ed` is the dictionary, `sel` a selection (module note), `afile` the animation file
/// (record whose +4 is its data: the action list is at data + 0xc) or 0, `id` the element
/// number the pasted curves belong to, `idx` the action of the selection to paste (counted
/// through its first array and then its second), `pframe` a frame counter that is advanced
/// by the action's length, `mv` (the action moves the keys that follow it) and `same` (the
/// entries are pasted onto the element `id` itself instead of the one each stands for, and
/// only the first entry is used).
///
/// Answers 1 when an action list was at hand (the file's, or the one of the first
/// animation-file relation of the root element), else 0. In order: the action's first
/// frame is `t0 = (frame − 0.5) / rate` (`frame` = `*pframe` + `SI_frameStart`), its length
/// `dur = len / rate` (floats); with `mv` the keys of the list's actions after `t0` are
/// moved on by `dur` (a walk with `haFcvProcessMove` over the root element), otherwise the
/// keys from `t0 − 1e-4` to `t0 + dur + 1e-4` are removed (`haFcvProcessRemove`). Then, for
/// each entry and each curve it marks, the keys within the action's window are copied to
/// the element's curve of that type (made when missing), shifted to `t0`, and on frames when
/// `SI_glbAnim` +0x32 says so. Then an action of the list is made after the pasted span
/// (`SI_actionsAddElem`, named by `SI_actionsGetNewName`), and when the source action has a
/// path (+0x28) a null object follows it (`SI_actionCreateNullOnPath`, its keys copied,
/// `SI_actionCnsToNull`). Finally `*pframe` grows by the length and the list is switched back
/// on. For the curves of `mv == 0` the last two words of the removal's data are never set
/// (they are what the stack held).
#[allow(clippy::too_many_arguments)]
pub fn actions_paste_to_ass_elems(m: &mut Mem, o: &mut dyn Outside, ed: u32, sel: u32, afile: u32, id: u32, idx: i32, pframe: u32, mv: i16, same: i16) -> u32 {
    let sp = m.sp();
    let r = paste_body(m, o, ed, sel, afile, id, idx, pframe, mv, same);
    m.set_sp(sp);
    r
}

#[allow(clippy::too_many_arguments)]
fn paste_body(m: &mut Mem, o: &mut dyn Outside, ed: u32, sel: u32, afile: u32, id: u32, idx: i32, pframe: u32, mv: i16, same: i16) -> u32 {
    // locals the callees write through: the three curve-info words, the element number
    let (l30, l34, l38, l3c) = (m.local(4), m.local(4), m.local(4), m.local(4));
    let data = m.local(16);
    let mut acts2: u32 = 0;
    if sel == 0 || idx < 0 || idx >= m.i32(sel.wrapping_add(0x14)).wrapping_add(m.i32(sel.wrapping_add(0xc))) {
        return 0;
    }
    let chapter = m.u32(sel.wrapping_add(8));
    let root = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[ed, chapter, id]);
    let nb_a = m.i32(sel.wrapping_add(0xc));
    let act = if idx < nb_a {
        m.u32(sel.wrapping_add(0x10)).wrapping_add((idx as u32).wrapping_mul(48))
    } else {
        m.u32(sel.wrapping_add(0x18)).wrapping_add(((idx - nb_a) as u32).wrapping_mul(48))
    };
    if act == 0 {
        return 0;
    }
    let frame = m.i32(pframe).wrapping_add(m[g::SI_FRAME_START]);
    let t0 = ((F80::from_i32(frame) - 0.5f64) / m[g::SI_FRAME_RATE]).to_f32();
    let len = ac::actions_elem_get_length(m, si_mem::Ptr32::new(act));
    // (frndint of a whole number under either rounding control changes nothing)
    let dur = (x(F80::from_i32(len).to_f32()) / m[g::SI_FRAME_RATE]).to_f32();
    if afile != 0 && m.u32(afile.wrapping_add(4)) != 0 {
        acts2 = m.u32(afile.wrapping_add(4)).wrapping_add(0xc);
        ac::actions_tmp_activate_all(m, o, ed, si_mem::Ptr32::<Actions>::new(acts2), 0);
    }
    let dict_chapter = chapter as i32;
    if mv != 0 {
        if acts2 != 0 {
            let n = m.i32(acts2);
            let mut i = 0;
            while i < n {
                let rec = m.u32(acts2.wrapping_add(4)).wrapping_add((i as u32).wrapping_mul(0x30));
                if x(m.f32(rec.wrapping_add(8))).gt(x(t0)) {
                    let s = (x(m.f32(rec.wrapping_add(8))) + dur).to_f32();
                    m.set_f32(rec.wrapping_add(8), s);
                    let e = (x(dur) + m.f32(rec.wrapping_add(0xc))).to_f32();
                    m.set_f32(rec.wrapping_add(0xc), e);
                }
                i += 1;
            }
        }
        m.set_f32(data, t0);
        m.set_f32(data.wrapping_add(4), t0);
        m.set_f32(data.wrapping_add(8), dur);
        m.set_f32(data.wrapping_add(12), 1.0);
        walk::hieranim_fcv_process(m, o, ed, step::MOVE, root, data, dict_chapter, 0);
    } else {
        let eps = x(0.0001f64);
        m.set_f32(data, (x(t0) - eps).to_f32());
        m.set_f32(data.wrapping_add(4), (eps + (x(t0) + dur)).to_f32());
        walk::hieranim_fcv_process(m, o, ed, step::REMOVE, root, data, -1, 1);
    }
    // the entries
    let mut i = 0i32;
    loop {
        if i >= m.i32(sel) {
            break;
        }
        let ent = m.u32(sel.wrapping_add(4)).wrapping_add((i as u32).wrapping_mul(40));
        let tgt = if same != 0 {
            id
        } else {
            o.call(m, &out::SI_ELEMDICT_GET_ELEM_FROM_HIER_ANIM, &[ed, sel, ent, 0, id])
        };
        m.set_u32(l3c, tgt);
        let e2 = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[ed, m.u32(sel.wrapping_add(8)), tgt]);
        i += 1;
        if e2 != 0 && m.u16(e2.wrapping_add(0x42)) != 0 {
            let mut j = 0i32;
            while j < m.i32(ent.wrapping_add(8)) {
                let jj = j;
                j += 1;
                if m.u16(m.u32(ent.wrapping_add(0x1c)).wrapping_add((jj as u32).wrapping_mul(2))) == 0 {
                    continue;
                }
                let fc = m.u32(m.u32(ent.wrapping_add(0x18)).wrapping_add((jj as u32).wrapping_mul(4)));
                let ty = m.u32(fc);
                o.call(m, &out::SI_ATTDICT_GET_INFO_FROM_FCURVE, &[ATD, ty, l38, l34, l30, l30]);
                if matches!(m.u32(fc), 7 | 9 | 0x1f | 0xa | 0x20 | 0xac) {
                    let t38 = m.u32(l38);
                    if find_related(m, o, e2, 1, t38, 0xffff_ffff, l30) == 0 {
                        continue;
                    }
                }
                if m.u32(fc) == 0xc4 {
                    continue;
                }
                let astart = m.f32(act.wrapping_add(8));
                let aend = (x(dur) + astart).to_f32();
                if o.call(m, &out::SI_FCV_CNT_KEYS_BETWEEN_TIMES, &[fc, astart.to_bits(), aend.to_bits()]) == 0 {
                    continue;
                }
                let ty = m.u32(fc);
                o.call(m, &out::SI_ATTDICT_GET_INFO_FROM_FCURVE, &[ATD, ty, l38, l34, l30, l30]);
                let (a34, a38) = (m.u32(l34), m.u32(l38));
                let mut f = o.call(m, &out::SI_ED_FIND_FCV, &[e2, a34, a38]);
                while f != 0 && m.u32(f) != m.u32(fc) {
                    let (a34, a38) = (m.u32(l34), m.u32(l38));
                    f = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[e2, a34, a38]);
                }
                if f == 0 {
                    let data2 = m.u32(e2.wrapping_add(4));
                    f = o.call(m, &out::SI_FCURVE_ALLOCATE, &[ty, data2]);
                    let d2 = m.u32(e2.wrapping_add(4));
                    let ch = m.u32(sel.wrapping_add(8));
                    let ix = o.call(m, &out::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[ed, ch, d2]);
                    m.set_u32(l3c, ix);
                    o.call(m, &out::SI_ED_FCV_ADD, &[f, e2, ch, ix]);
                    if f == 0 {
                        continue;
                    }
                }
                let astart = m.f32(act.wrapping_add(8));
                let aend = (x(dur) + astart).to_f32();
                o.call(m, &out::SI_FCV_COPY_KEYS_BETWEEN_TIMES, &[f, fc, t0.to_bits(), astart.to_bits(), aend.to_bits(), 1.0f32.to_bits()]);
                if m[g::SI_GLB_ANIM][0x32 / 2] != 0 {
                    let end = (x(t0) + dur).to_f32();
                    o.call(m, &out::SI_FCV_SET_KEYS_ON_FRAMES, &[f, t0.to_bits(), end.to_bits()]);
                }
            }
        }
        // (the loop goes on while entries are left, unless the entries are pasted onto one
        // element and one has been done)
        if i >= m.i32(sel) {
            break;
        }
        if same != 0 && i > 0 {
            break;
        }
    }
    // the action list: the file's, or the first one the root element has
    let cid0 = m[g::SI_CID][0] as u32;
    let r = o.call(m, &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM, &[ed, root, 1, 0, 0x3a98, 0x3aa2, cid0, l30, l30]);
    if acts2 == 0 {
        if r != 0 && m.u32(r.wrapping_add(4)) != 0 {
            acts2 = m.u32(r.wrapping_add(4)).wrapping_add(0xc);
        }
        if acts2 == 0 {
            return 0;
        }
    }
    let a2 = si_mem::Ptr32::<Actions>::new(acts2);
    let name = ac::actions_get_new_name(m, a2, m.u32(act));
    // (`or al,2; and al,0xf6`: only the low byte of the word is touched)
    let fl = m.u32(act.wrapping_add(0x18));
    let flags = (fl & 0xffff_ff00) | (((fl as u8) | 2) & 0xf6) as u32;
    let end_fr = ac::actions_sec_to_end_frm(m, (x(t0) + dur).to_f32());
    let start_fr = ac::actions_sec_to_start_frm(m, t0);
    let newid = ac::actions_add_elem(m, a2, name, start_fr, end_fr, 1, flags as i32);
    let sc = m.u32(act.wrapping_add(0x28));
    if sc != 0 {
        if m.i32(sc) == -1 && m.u32(sc.wrapping_add(0xc)) != 0 {
            let nm = m.u32(sc.wrapping_add(0xc));
            let models = m[g::SI_CID][MODELS_CID] as u32;
            let v = o.call(m, &out::SI_ELEMDICT_FIND_ELEM_BY_NAME_ONLY, &[ed, models, 0, nm, 0xffff_ffff, 0]);
            let sc2 = m.u32(act.wrapping_add(0x28));
            m.set_u32(sc2, v);
        }
        let sc = m.u32(act.wrapping_add(0x28));
        if sc != 0 && m.i32(sc) != -1 {
            let rdata = m.u32(r.wrapping_add(4));
            let ent2 = m.u32(acts2.wrapping_add(4)).wrapping_add((newid as u32).wrapping_mul(48));
            let rec = m.si_calloc(1, 0x1c);
            m.set_u32(ent2.wrapping_add(0x28), rec);
            let first = m.u32(sc);
            m.set_u32(rec, first);
            let graf = m.u32(m.u32(root.wrapping_add(8)));
            let c8 = m.u32(rdata.wrapping_add(8));
            let rd = m.u32(root.wrapping_add(4));
            o.call(m, &out::SI_ELEMDICT_GET_ELEM_FROM_DATA, &[ed, c8, rd, l3c]);
            let models = m[g::SI_CID][MODELS_CID] as u32;
            let first = m.u32(m.u32(ent2.wrapping_add(0x28)));
            let me = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[ed, models, first]);
            let model = m.u32(me.wrapping_add(4));
            let endf = round_frames(m, m.f32(ent2.wrapping_add(0xc)));
            let startf = round_frames(m, m.f32(ent2.wrapping_add(8)));
            let outp = m.u32(ent2.wrapping_add(0x28)).wrapping_add(4);
            let null = o.call(m, &out::SI_ACTION_CREATE_NULL_ON_PATH, &[model, startf as u32, endf as u32, outp]);
            let sc = m.u32(act.wrapping_add(0x28));
            if m.u32(sc.wrapping_add(8)) != 0 {
                let rec = m.u32(ent2.wrapping_add(0x28));
                let cur = m.u32(rec.wrapping_add(4));
                let me = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[ed, models, cur]);
                let f = o.call(m, &out::SI_ED_FIND_FCV, &[me, 1, 0x70]);
                let astart = m.f32(act.wrapping_add(8));
                let aend = (x(dur) + astart).to_f32();
                let src = m.u32(m.u32(act.wrapping_add(0x28)).wrapping_add(8));
                o.call(m, &out::SI_FCV_COPY_KEYS_BETWEEN_TIMES, &[f, src, t0.to_bits(), astart.to_bits(), aend.to_bits(), 1.0f32.to_bits()]);
            }
            let cons = o.call(m, &out::SI_REL_GET_CONS_REL_FROM_GRAF_TYPE, &[graf]);
            let endf = round_frames(m, m.f32(ent2.wrapping_add(0xc)));
            let startf = round_frames(m, m.f32(ent2.wrapping_add(8)));
            let c8 = m.u32(rdata.wrapping_add(8));
            let l3 = m.u32(l3c);
            let rec = m.u32(ent2.wrapping_add(0x28));
            let cur = m.u32(rec.wrapping_add(4));
            o.call(m, &out::SI_ACTION_CNS_TO_NULL, &[c8, l3, graf, null, cur, cons, startf as u32, endf as u32, 0]);
        }
    }
    if name != 0 {
        m.free(name);
    }
    let cur = m.i32(pframe);
    m.set_u32(pframe, cur.wrapping_add(len) as u32);
    ac::actions_tmp_activate_all(m, o, ed, a2, 1);
    1
}

/// The index of the models chapter in `SI_cid` (`SI_cid.MODELS`, 0x08781680).
const MODELS_CID: usize = 8;

// ------------------------------------------------------------------ the paste from the buffer

/// The dope sheet's clipboard record (`SI_dopeSh`, 0x0878faa0): word indices of its fields.
mod ds {
    pub const CHAPTER: usize = 0x2c / 4;
    pub const COUNT: usize = 0x30 / 4;
    pub const ENTRIES: usize = 0x34 / 4;
    pub const WIN_START: usize = 0x44 / 4;
    pub const WIN_END: usize = 0x48 / 4;
    pub const CHOSEN: usize = 0x6c / 4;
    pub const LIST: usize = 0x70 / 4;
    pub const MODE: usize = 0x7c / 4;
}
/// The window pseudo-action inside the record (+0x3c; its start and end are the window).
const DOPE_WINDOW_ELEM: u32 = 0x0878_fadc;
/// The buffer's root record (`SI_dopeSh` +0x28).
const DOPE_BUF: u32 = 0x0878_fac8;


fn rate(m: &Mem) -> si_x87::F80 {
    x(m[g::SI_FRAME_RATE])
}

/// Truncates `v + 0.5` for a positive sum `t` and `v - 0.5` otherwise (`fistp` under a
/// forced round-toward-zero control word); an out-of-range value gives 0x80000000.
fn frames_of(v: F80, t: F80) -> i32 {
    let adj = if t.gt(F80::ZERO) { v + 0.5f64 } else { v + (-0.5f64) };
    adj.to_i32(Round::Zero)
}

/// `SI_actionsPasteSelected` [soft 0x081e1978 SI_actionsPasteSelected] (emulator check: finding 0030, section 17)
///
/// Pastes the dope sheet's clipboard (`SI_dopeSh`: +0x30 entries of 48 bytes at +0x34, the
/// chapter at +0x2c, the window at +0x44 / +0x48) into the action list `a`. Answers 0 when
/// nothing can be pasted (no list, a negative item at +0x6c, a window that does not fit the
/// list, no animation file or root element, a chapter that is not the clipboard's), 1
/// otherwise (also, doing nothing, with an empty clipboard).
///
/// With a window length (`SI_actionsElemGetLength` of the window above 0) it is that;
/// without one, and with the paste mode (word at +0x7c; the button 5 or 6 held inverts its
/// truth), the window ends at the last entry's end. The list is saved for undo in a group
/// (`SI_historyOpenCluster` …), the curves of the root's hierarchy too, all actions are
/// turned off; when the paste inserts (no window length, mode set) the actions that start
/// after the window start are moved on by `span − 1/rate` and the curves with them
/// (`haFcvProcessMove`), otherwise the keys under the window are removed (`haFcvProcessRemove`,
/// once per entry, with the entry's times mapped by `(window end − window start) / span`).
/// The buffer is pasted onto the hierarchy (`actionsPasteBufNode`); then every entry makes an
/// action of the list (`SI_actionsAddElem`, from the entry's times mapped the same way, rounded
/// to frames by truncating `rate × t ± 0.5`) and, when it has a path, a null object on it
/// (`SI_actionCreateNullOnPath`, `SI_actionCnsToNull`); all actions are turned on again.
pub fn actions_paste_selected(m: &mut Mem, o: &mut dyn Outside, a: u32) -> u32 {
    let sp = m.sp();
    let r = paste_selected_body(m, o, a);
    m.set_sp(sp);
    r
}

fn paste_selected_body(m: &mut Mem, o: &mut dyn Outside, a: u32) -> u32 {
    let (l2c, l30, l34) = (m.local(4), m.local(4), m.local(4));
    let data = m.local(16);
    if a == 0 || (m[g::SI_DOPE_SH][ds::CHOSEN] as i32) < 0 {
        return 0;
    }
    if (m[g::SI_DOPE_SH][ds::COUNT] as i32) <= 0 {
        return 1;
    }
    let pressed = o.call(m, &out::GETBUTTON, &[6]) != 0 || o.call(m, &out::GETBUTTON, &[5]) != 0;
    let mode = m[g::SI_DOPE_SH][ds::MODE] as u16;
    let w4e: u16 = if pressed { (mode == 0) as u16 } else { mode };
    let len = ac::actions_elem_get_length(m, si_mem::Ptr32::new(DOPE_WINDOW_ELEM));
    let mut have_len = len > 0;
    if w4e == 0 && !have_len {
        // no window length and the paste is not an insert: the window ends where the last entry does
        let ents = m[g::SI_DOPE_SH][ds::ENTRIES];
        let cnt = m[g::SI_DOPE_SH][ds::COUNT];
        let last_end = m.f32(ents.wrapping_add(cnt.wrapping_mul(48)).wrapping_sub(0x24));
        let v = (x(f32::from_bits(m[g::SI_DOPE_SH][ds::WIN_START])) + x(last_end)).to_f32();
        m[g::SI_DOPE_SH][ds::WIN_END] = v.to_bits();
        have_len = true;
    }
    if have_len {
        let end = ac::actions_sec_to_end_frm(m, f32::from_bits(m[g::SI_DOPE_SH][ds::WIN_END]));
        let start = ac::actions_sec_to_start_frm(m, f32::from_bits(m[g::SI_DOPE_SH][ds::WIN_START]));
        if ac::actions_validate_elem(m, si_mem::Ptr32::new(a), start, end, 1, -1) == 0 {
            return 0;
        }
    }
    let d = g::cur_dict(m);
    let file = ac::ed_get_anim_file_from_actions(m, d, a);
    let d = g::cur_dict(m);
    let root = ac::ed_get_root_elem_from_anim_file(m, o, d, file, l30, l2c);
    if root == 0 || file == 0 {
        return 0;
    }
    let fdata = m.u32(file.wrapping_add(4));
    if m.u32(fdata.wrapping_add(8)) != m[g::SI_DOPE_SH][ds::CHAPTER] {
        return 0;
    }
    let prev = m[g::SI_DOPE_SH][ds::LIST];
    ac::actions_unselect_all(m, si_mem::Ptr32::new(prev));
    m[g::SI_DOPE_SH][ds::LIST] = a;
    let ents = m[g::SI_DOPE_SH][ds::ENTRIES];
    let cnt = m[g::SI_DOPE_SH][ds::COUNT];
    let first_start = m.f32(ents.wrapping_add(8));
    let l40 = first_start;
    let last_end = m.f32(ents.wrapping_add(cnt.wrapping_mul(48)).wrapping_sub(0x24));
    let l4c = (x(last_end) - x(l40)).to_f32();
    let l48 = f32::from_bits(m[g::SI_DOPE_SH][ds::WIN_START]);
    let l44 = (x(l48) - x(l40)).to_f32();
    let l3c = if have_len {
        ((x(f32::from_bits(m[g::SI_DOPE_SH][ds::WIN_END])) - x(l48)) / x(l4c)).to_f32()
    } else {
        1.0f32
    };
    o.call(m, &out::SI_HISTORY_OPEN_CLUSTER, &[]);
    let (v30, v2c) = (m.u32(l30), m.u32(l2c));
    o.call(m, &out::SI_ACTIONS_SAVE_HISTORY, &[fdata, v30, v2c]);
    let chapter = m.i32(fdata.wrapping_add(8));
    let d = g::cur_dict(m);
    walk::hieranim_fcv_process(m, o, d, step::SAVE_HISTORY, root, 0, chapter, 0);
    let d = g::cur_dict(m);
    ac::actions_tmp_activate_all(m, o, d, si_mem::Ptr32::<Actions>::new(a), 0);
    let insert = !have_len && w4e != 0;
    let one_over_rate = x(1.0f32) / m[g::SI_FRAME_RATE];
    if insert {
        let n = m.i32(a);
        let mut i = 0i32;
        if i < n {
            loop {
                let elems = m.u32(a.wrapping_add(4));
                let rec = elems.wrapping_add((i as u32).wrapping_mul(0x30));
                let start = m.f32(rec.wrapping_add(8));
                if x(start).gt(x(l48)) {
                    let dd = x(l4c) - one_over_rate;
                    m.set_f32(rec.wrapping_add(8), (x(start) + dd).to_f32());
                    let elems = m.u32(a.wrapping_add(4));
                    let rec = elems.wrapping_add((i as u32).wrapping_mul(0x30));
                    let e = (dd + m.f32(rec.wrapping_add(0xc))).to_f32();
                    m.set_f32(rec.wrapping_add(0xc), e);
                }
                i += 1;
                if i >= m.i32(a) {
                    break;
                }
            }
        }
        m.set_f32(data, l48);
        m.set_f32(data.wrapping_add(4), l48);
        m.set_f32(data.wrapping_add(8), (x(l4c) - one_over_rate).to_f32());
        m.set_f32(data.wrapping_add(12), 1.0);
        let d = g::cur_dict(m);
        let chapter = m.i32(fdata.wrapping_add(8));
        walk::hieranim_fcv_process(m, o, d, step::MOVE, root, data, chapter, 0);
    } else {
        // (the last two words of the removal's record are never set: they hold what the stack held)
        let k = x(0.0001f64);
        let s = x(l44) + x(l40);
        let mut i = 0i32;
        while i < m[g::SI_DOPE_SH][ds::COUNT] as i32 {
            let ent = m[g::SI_DOPE_SH][ds::ENTRIES].wrapping_add((i as u32).wrapping_mul(0x30));
            let a_ = (x(m.f32(ent.wrapping_add(8))) - x(l40)) * x(l3c);
            m.set_f32(data, ((a_ + s) - k).to_f32());
            let c_ = (x(m.f32(ent.wrapping_add(0xc))) - x(l40)) * x(l3c);
            m.set_f32(data.wrapping_add(4), ((c_ + s) + k).to_f32());
            let d = g::cur_dict(m);
            let chapter = m.i32(fdata.wrapping_add(8));
            walk::hieranim_fcv_process(m, o, d, step::REMOVE, root, data, chapter, 0);
            i += 1;
        }
    }
    let flag = if have_len { 0 } else { (w4e != 0) as i16 };
    crate::dopebuf::actions_paste_buf_node(m, o, root, si_mem::Ptr32::<Actions>::new(a), DOPE_BUF, l44, l3c, l40, flag);
    o.call(m, &out::SI_HISTORY_CLOSE_CLUSTER, &[]);
    let mut i = 0i32;
    while i < m[g::SI_DOPE_SH][ds::COUNT] as i32 {
        let ent = m[g::SI_DOPE_SH][ds::ENTRIES].wrapping_add((i as u32).wrapping_mul(0x30));
        let name = ac::actions_get_new_name(m, si_mem::Ptr32::<Actions>::new(a), m.u32(ent));
        let r = rate(m);
        let half = x(0.5f64) / m[g::SI_FRAME_RATE];
        let sum = x(l44) + x(l40);
        let es = m.f32(ent.wrapping_add(8));
        let s0 = x(es) - x(l40);
        let sc = x(l3c);
        let p = (s0 + half) * sc + sum;
        let q = sc * (half + (x(es) - x(l40)));
        let t = q + sum;
        let sf = frames_of(r * p, t);
        let ee = m.f32(ent.wrapping_add(0xc));
        let u = (x(ee) - x(l40)) - half;
        let v = u * sc + sum;
        let w = (x(ee) - x(l40)) - half;
        let t2 = w * sc + sum;
        let ef = frames_of(r * v, t2);
        let cycles = m.i32(ent.wrapping_add(0x24));
        let flags = m.i32(ent.wrapping_add(0x18));
        let newid = ac::actions_add_elem(m, si_mem::Ptr32::<Actions>::new(a), name, sf, ef, cycles, flags);
        let sp_ = m.u32(ent.wrapping_add(0x28));
        if sp_ != 0 && m.i32(sp_) != -1 {
            let esi = m.u32(a.wrapping_add(4)).wrapping_add((newid as u32).wrapping_mul(0x30));
            let rec = m.si_calloc(1, 0x1c);
            m.set_u32(esi.wrapping_add(0x28), rec);
            let first = m.u32(m.u32(ent.wrapping_add(0x28)));
            m.set_u32(rec, first);
            let graf = m.u32(m.u32(root.wrapping_add(8)));
            let d = g::cur_dict(m);
            let c8 = m.u32(fdata.wrapping_add(8));
            let rd = m.u32(root.wrapping_add(4));
            o.call(m, &out::SI_ELEMDICT_GET_ELEM_FROM_DATA, &[d, c8, rd, l34]);
            let models = m[g::SI_CID][MODELS_CID] as u32;
            let first = m.u32(m.u32(esi.wrapping_add(0x28)));
            let d = g::cur_dict(m);
            let me = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[d, models, first]);
            let model = m.u32(me.wrapping_add(4));
            let outp = m.u32(esi.wrapping_add(0x28)).wrapping_add(4);
            let null = o.call(m, &out::SI_ACTION_CREATE_NULL_ON_PATH, &[model, sf as u32, ef as u32, outp]);
            if m.u32(m.u32(ent.wrapping_add(0x28)).wrapping_add(8)) != 0 {
                let cur = m.u32(m.u32(esi.wrapping_add(0x28)).wrapping_add(4));
                let d = g::cur_dict(m);
                let me = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[d, models, cur]);
                o.call(m, &out::SI_ED_FIND_FCV, &[me, 1, 0x70]);
            }
            let cons = o.call(m, &out::SI_REL_GET_CONS_REL_FROM_GRAF_TYPE, &[graf]);
            let cur = m.u32(m.u32(esi.wrapping_add(0x28)).wrapping_add(4));
            let (c8, l3) = (m.u32(fdata.wrapping_add(8)), m.u32(l34));
            o.call(m, &out::SI_ACTION_CNS_TO_NULL, &[c8, l3, graf, null, cur, cons, sf as u32, ef as u32, 0]);
        }
        if name != 0 {
            m.free(name);
        }
        i += 1;
    }
    let d = g::cur_dict(m);
    ac::actions_tmp_activate_all(m, o, d, si_mem::Ptr32::<Actions>::new(a), 1);
    1
}

// ------------------------------------------------------------------ the drag from the buffer

/// A float of the removal / move / scale record the walk's steps read (`data` + 4 × `k`).
fn put4(m: &mut Mem, data: u32, v: [Option<f32>; 4]) {
    for (k, f) in v.iter().enumerate() {
        if let Some(f) = f {
            m.set_f32(data.wrapping_add(4 * k as u32), *f);
        }
    }
}

/// `SI_actionsUpdateFromBuffer` [soft 0x081e31d4 SI_actionsUpdateFromBuffer] (emulator check: finding 0030, section 17)
///
/// After the dope sheet's actions were dragged (`a` is the sheet's copy, `idx` the element
/// of chapter `chapter` that owns the file's list): brings the file's action list (the
/// first animation-file relation of the element; it must hold as many actions as `a`) and
/// the curves of the element's hierarchy in line with `a`. Nothing without the element
/// (whose word at +0x54 must be 0), the list, `a`, or when the counts differ. The list and
/// the curves are saved for undo (`SI_historyOpenCluster` …), every action is turned off.
///
/// `edge` (the 16-bit argument after `idx`) tells which kind of drag it was. With it set,
/// the drag took one end: from the first or the last selected action (decided from which
/// selected ends differ from the list's, `fwd` then says whether the later actions follow)
/// the keys of the element are moved (`haFcvProcessMove`), scaled (`…Scale`) or cut
/// (`…Remove`) by the difference of lengths and starts, and the actions behind are copied
/// from `a` (start and end). With it clear, every selected action is taken in turn (from the
/// first one up when the first selected start lies before the list's, else from the last one
/// down) and its keys moved, scaled or cut to the new place. `plain` (the last argument)
/// skips the key editing and copies the times only. With `SI_glbAnim` +0x32 set the keys of
/// every selected action are put on frames. The list is sorted and all actions turned on.
pub fn actions_update_from_buffer(m: &mut Mem, o: &mut dyn Outside, a: u32, chapter: i32, idx: i32, edge: i16, plain: i16) {
    let sp = m.sp();
    update_body(m, o, a, chapter, idx, edge, plain);
    m.set_sp(sp);
}

const K_MARGIN: f64 = 0.0001;

fn ha(m: &mut Mem, o: &mut dyn Outside, f: u32, elem: u32, data: u32, chapter: i32) {
    let d = g::cur_dict(m);
    walk::hieranim_fcv_process(m, o, d, f, elem, data, chapter, 0);
}

fn update_body(m: &mut Mem, o: &mut dyn Outside, a: u32, chapter: i32, idx: i32, edge: i16, plain: i16) {
    use crate::actions::ActionsElem;
    let l2c = m.local(4);
    let data = m.local(16);
    let w4e_slot = m.local(4); // (`w4e` is not set on one path of the original: it holds what the stack held)
    let tot = |m: &Mem, e: u32| ac::actions_elem_get_tot_length(m, si_mem::Ptr32::<ActionsElem>::new(e));
    let flag = |m: &Mem, e: u32| m.u8(e.wrapping_add(0x18)) & 4 != 0;
    let d = g::cur_dict(m);
    let elem = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[d, chapter as u32, idx as u32]);
    if elem == 0 || m.u32(elem.wrapping_add(0x54)) != 0 {
        return;
    }
    let cid0 = m[g::SI_CID][0] as u32;
    let d = g::cur_dict(m);
    let r = o.call(m, &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM, &[d, elem, 1, 0, 0x3a98, 0x3aa2, cid0, l2c, l2c]);
    let mut b = 0u32;
    if r != 0 && m.u32(r.wrapping_add(4)) != 0 {
        b = m.u32(r.wrapping_add(4)).wrapping_add(0xc);
    }
    if b == 0 || a == 0 || m.u32(b) != m.u32(a) {
        return;
    }
    o.call(m, &out::SI_HISTORY_OPEN_CLUSTER, &[]);
    let fdata = m.u32(r.wrapping_add(4));
    o.call(m, &out::SI_ACTIONS_SAVE_HISTORY, &[fdata, chapter as u32, idx as u32]);
    ha(m, o, step::SAVE_HISTORY, elem, 0, chapter);
    o.call(m, &out::SI_HISTORY_CLOSE_CLUSTER, &[]);
    let d = g::cur_dict(m);
    ac::actions_tmp_activate_all(m, o, d, si_mem::Ptr32::<Actions>::new(b), 0);
    let plain = plain != 0;
    let k = x(K_MARGIN);
    let ba = si_mem::Ptr32::<Actions>::new(b);
    if edge != 0 {
        let n = m.i32(a);
        let ecx = m.u32(a.wrapping_add(4));
        let last_e = ecx.wrapping_add((n as u32).wrapping_mul(0x30)).wrapping_sub(0x30);
        let b_elems = m.u32(b.wrapping_add(4));
        let mut w4e: u16;
        let fwd: bool;
        if flag(m, ecx) {
            if !flag(m, last_e) {
                w4e = 0;
                fwd = false;
            } else {
                let (a0s, b0s) = (m.f32(ecx.wrapping_add(8)), m.f32(b_elems.wrapping_add(8)));
                if x(b0s).eq(x(a0s)) {
                    w4e = 1;
                    fwd = true;
                } else {
                    let ae = m.f32(last_e.wrapping_add(0xc));
                    let be = m.f32(b_elems.wrapping_add((n as u32).wrapping_mul(0x30)).wrapping_sub(0x24));
                    w4e = 0;
                    if !x(be).eq(x(ae)) && x(b0s).lt(x(a0s)) {
                        w4e = 1;
                    }
                    fwd = w4e != 0;
                }
            }
        } else if flag(m, last_e) {
            w4e = 1;
            fwd = true;
        } else {
            w4e = m.u16(w4e_slot);
            fwd = w4e != 0;
        }
        let mut l3c: i32;
        if fwd {
            l3c = 0;
            let lim = n.wrapping_sub(1);
            if 0 < lim && !flag(m, ecx) {
                let mut p = ecx.wrapping_add(0x18);
                loop {
                    p = p.wrapping_add(0x30);
                    l3c += 1;
                    if l3c >= lim || m.u8(p) & 4 != 0 {
                        break;
                    }
                }
            }
        } else {
            l3c = n.wrapping_sub(1);
            if 0 < l3c && !flag(m, ecx.wrapping_add((l3c as u32).wrapping_mul(0x30))) {
                let mut p = ecx.wrapping_add((l3c as u32).wrapping_mul(0x30)).wrapping_add(0x18);
                loop {
                    p = p.wrapping_sub(0x30);
                    l3c -= 1;
                    if l3c <= 0 || m.u8(p) & 4 != 0 {
                        break;
                    }
                }
            }
        }
        let a_e = ecx.wrapping_add((l3c as u32).wrapping_mul(0x30));
        let b_e = b_elems.wrapping_add((l3c as u32).wrapping_mul(0x30));
        let lb = tot(m, b_e);
        let la = tot(m, a_e);
        let neg1 = -1.0f32;
        if lb != la {
            let l40 = m.f32(b_e.wrapping_add(8));
            let l44 = m.f32(b_e.wrapping_add(0xc));
            let r = F80::from_i32(tot(m, a_e)) / F80::from_i32(tot(m, b_e));
            let (as_, bs) = (m.f32(a_e.wrapping_add(8)), m.f32(b_e.wrapping_add(8)));
            if !x(as_).eq(x(bs)) {
                let l48 = (x(as_) - x(bs)).to_f32();
                if F80::ZERO.lt(x(l48)) {
                    if plain {
                        put4(m, data, [Some(l40), Some((x(l40) + x(l48)).to_f32()), None, None]);
                        ha(m, o, step::REMOVE, elem, data, chapter);
                    } else {
                        put4(m, data, [Some(l40), Some(l44), Some(l44), Some(r.to_f32())]);
                        ha(m, o, step::SCALE, elem, data, chapter);
                    }
                    put4(m, data, [Some(l40), Some(l40), Some(l48), Some(neg1)]);
                    ha(m, o, step::MOVE, elem, data, chapter);
                } else {
                    put4(m, data, [Some(l40), Some(l40), Some(l48), Some(neg1)]);
                    ha(m, o, step::MOVE, elem, data, chapter);
                    if !plain {
                        put4(m, data, [Some(l40), Some(l44), Some(l44), Some(r.to_f32())]);
                        ha(m, o, step::SCALE, elem, data, chapter);
                    }
                }
            } else {
                let l48 = (x(m.f32(a_e.wrapping_add(0xc))) - x(m.f32(b_e.wrapping_add(0xc)))).to_f32();
                if F80::ZERO.lt(x(l48)) {
                    put4(m, data, [Some(l44), Some(l44), Some(l48), Some(1.0)]);
                    ha(m, o, step::MOVE, elem, data, chapter);
                    if !plain {
                        put4(m, data, [Some(l40), Some(l40), Some(l44), Some(r.to_f32())]);
                        ha(m, o, step::SCALE, elem, data, chapter);
                    }
                } else {
                    if plain {
                        put4(m, data, [Some((x(l44) + x(l48)).to_f32()), Some(l44), None, None]);
                        ha(m, o, step::REMOVE, elem, data, chapter);
                    } else {
                        put4(m, data, [Some(l40), Some(l40), Some(l44), Some(r.to_f32())]);
                        ha(m, o, step::SCALE, elem, data, chapter);
                    }
                    put4(m, data, [Some(l44), Some(l44), Some(l48), Some(1.0)]);
                    ha(m, o, step::MOVE, elem, data, chapter);
                }
            }
        } else {
            let (bs, be) = (m.f32(b_e.wrapping_add(8)), m.f32(b_e.wrapping_add(0xc)));
            let l8 = (x(m.f32(a_e.wrapping_add(8))) - x(bs)).to_f32();
            let l4 = if w4e != 0 { 1.0f32 } else { neg1 };
            if plain {
                put4(m, data, [Some(be), Some(bs), Some(l8), Some(l4)]);
            } else {
                put4(m, data, [Some(bs), Some(be), Some(l8), Some(l4)]);
            }
            ha(m, o, step::MOVE, elem, data, chapter);
        }
        // the actions from the one that was changed to the end (or the start): the times of the sheet's copy
        let copy = |m: &mut Mem, i: i32| {
            let dst = m.u32(b.wrapping_add(4)).wrapping_add((i as u32).wrapping_mul(0x30));
            let src = m.u32(a.wrapping_add(4)).wrapping_add((i as u32).wrapping_mul(0x30));
            let v = m.f32(src.wrapping_add(8));
            m.set_f32(dst.wrapping_add(8), v);
            let dst = m.u32(b.wrapping_add(4)).wrapping_add((i as u32).wrapping_mul(0x30));
            let v = m.f32(src.wrapping_add(0xc));
            m.set_f32(dst.wrapping_add(0xc), v);
        };
        let _ = &mut w4e;
        if w4e != 0 {
            let mut i = l3c;
            if i < m.i32(b) {
                loop {
                    copy(m, i);
                    i += 1;
                    if i >= m.i32(b) {
                        break;
                    }
                }
            }
        } else {
            let mut i = l3c;
            while i >= 0 {
                copy(m, i);
                i -= 1;
            }
        }
    } else {
        let n = m.i32(a);
        let ecx = m.u32(a.wrapping_add(4));
        let edi = m.u32(b.wrapping_add(4));
        let lim = n.wrapping_sub(1);
        let mut i = 0i32;
        if 0 < lim && !flag(m, ecx) {
            let mut p = ecx.wrapping_add(0x18);
            loop {
                p = p.wrapping_add(0x30);
                i += 1;
                if i >= lim || m.u8(p) & 4 != 0 {
                    break;
                }
            }
        }
        let off = (i as u32).wrapping_mul(0x30);
        let fwd = x(m.f32(ecx.wrapping_add(off).wrapping_add(8))).lt(x(m.f32(edi.wrapping_add(off).wrapping_add(8))));
        i = if fwd { 0 } else { n.wrapping_sub(1) };
        loop {
            if fwd {
                if i >= m.i32(a) {
                    break;
                }
            } else if i < 0 {
                break;
            }
            let off = (i as u32).wrapping_mul(0x30);
            let esi = m.u32(a.wrapping_add(4)).wrapping_add(off);
            let edi = m.u32(b.wrapping_add(4)).wrapping_add(off);
            if flag(m, esi) {
                let (as_, bs) = (m.f32(esi.wrapping_add(8)), m.f32(edi.wrapping_add(8)));
                let skip = x(as_).eq(x(bs)) && x(m.f32(esi.wrapping_add(0xc))).eq(x(m.f32(edi.wrapping_add(0xc))));
                if !skip {
                    let lb = tot(m, edi);
                    let la = tot(m, esi);
                    let rate = m[g::SI_FRAME_RATE];
                    if lb != la {
                        let l40 = m.f32(edi.wrapping_add(8));
                        let l44 = m.f32(edi.wrapping_add(0xc));
                        let la = tot(m, esi);
                        // (the integer goes through a float: `fild`, `frndint` of a whole number, `fstp` to a float)
                        let t1 = x(F80::from_i32(la).to_f32()) / rate;
                        let lb = tot(m, edi);
                        let t2 = x(F80::from_i32(lb).to_f32()) / rate;
                        let l48 = (t1 - t2).to_f32();
                        let r = F80::from_i32(tot(m, esi)) / F80::from_i32(tot(m, edi));
                        let (bs, as_) = (m.f32(edi.wrapping_add(8)), m.f32(esi.wrapping_add(8)));
                        if !x(as_).eq(x(bs)) {
                            if !plain {
                                if F80::ONE.lt(r) {
                                    put4(m, data, [Some(((x(l40) - x(l48)) - k).to_f32()), Some(l40), None, None]);
                                    ha(m, o, step::REMOVE, elem, data, chapter);
                                }
                                put4(m, data, [Some(l40), Some(l44), Some(l44), Some(r.to_f32())]);
                                ha(m, o, step::SCALE, elem, data, chapter);
                            }
                            let v = m.f32(esi.wrapping_add(8));
                            m.set_f32(edi.wrapping_add(8), v);
                        } else {
                            if !plain {
                                if F80::ONE.lt(r) {
                                    put4(m, data, [Some(l44), Some(((x(l44) + x(l48)) + k).to_f32()), None, None]);
                                    ha(m, o, step::REMOVE, elem, data, chapter);
                                }
                                put4(m, data, [Some(l40), Some(l40), Some(l44), Some(r.to_f32())]);
                                ha(m, o, step::SCALE, elem, data, chapter);
                            }
                            let v = m.f32(esi.wrapping_add(0xc));
                            m.set_f32(edi.wrapping_add(0xc), v);
                        }
                    } else if plain {
                        let v = m.f32(esi.wrapping_add(8));
                        m.set_f32(edi.wrapping_add(8), v);
                        let v = m.f32(esi.wrapping_add(0xc));
                        m.set_f32(edi.wrapping_add(0xc), v);
                    } else {
                        let l40 = m.f32(edi.wrapping_add(8));
                        let l44 = m.f32(edi.wrapping_add(0xc));
                        let l48 = (x(m.f32(esi.wrapping_add(8))) - x(l40)).to_f32();
                        let to_x = |v: F80| v.to_f32();
                        let mut simple = false;
                        if F80::ZERO.lt(x(l48)) {
                            if (x(l44) - x(l40)).lt(x(l48)) {
                                simple = true;
                            } else {
                                put4(m, data, [Some(l44), Some(to_x((x(l44) + x(l48)) + k)), None, None]);
                            }
                        } else if (x(l40) - x(l44)).gt(x(l48)) {
                            simple = true;
                        } else {
                            put4(m, data, [Some(to_x((x(l40) + x(l48)) - k)), Some(l40), None, None]);
                        }
                        if simple {
                            put4(m, data, [Some(to_x((x(l40) + x(l48)) - k)), Some(to_x((x(l44) + x(l48)) + k)), None, None]);
                        }
                        ha(m, o, step::REMOVE, elem, data, chapter);
                        put4(m, data, [Some(l40), Some(l44), Some(l48), Some(0.0)]);
                        ha(m, o, step::MOVE, elem, data, chapter);
                        let v = m.f32(esi.wrapping_add(8));
                        m.set_f32(edi.wrapping_add(8), v);
                        let v = m.f32(esi.wrapping_add(0xc));
                        m.set_f32(edi.wrapping_add(0xc), v);
                    }
                }
            }
            if fwd {
                i += 1;
            } else {
                i -= 1;
            }
        }
    }
    if m[g::SI_GLB_ANIM][0x32 / 2] != 0 {
        let mut i = 0i32;
        while i < m.i32(a) {
            let e = m.u32(a.wrapping_add(4)).wrapping_add((i as u32).wrapping_mul(0x30));
            if flag(m, e) {
                put4(m, data, [Some(m.f32(e.wrapping_add(8))), Some(m.f32(e.wrapping_add(0xc))), Some(0.0), None]);
                ha(m, o, step::SET_KEYS_ON_FRAMES, elem, data, chapter);
            }
            i += 1;
        }
    }
    ac::actions_sort(m, ba);
    let d = g::cur_dict(m);
    ac::actions_tmp_activate_all(m, o, d, ba, 1);
}
