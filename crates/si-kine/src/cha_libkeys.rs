// SPDX-License-Identifier: MIT
//! `cha_libkeys.c` on the shared memory image: reading an animation file or a
//! structure-key file into the open scene and matching what it holds to the selected
//! elements (finding 0022, "Actor").
//!
//! The file reads an animation (or a library of actions, or structure keys) into a fresh
//! element of the scene's dictionary, finds for each selected hierarchy the element it
//! applies to, lets the matching functions write the keys onto the scene, and removes the
//! temporary element again.
//!
//! **Not ported** (dialogs, called through [`Outside`]): `SI_dialogFilterTracks`
//! (0x081dd74c), `dialogGetAnimation` (0x081dda18), `dialogGetActions` (0x081de22c),
//! `getActionsValidate` (0x081de75c), `dialogGetMasters` (0x081de900),
//! `SI_dialogSaveStructKey` (0x081def78).

use si_mem::Mem;

use crate::actout as ao;
use crate::actout::{call, call_eax};
use crate::character::{DOPE_FRAME, DOPE_MODE, SI_CID_MODELS, SI_FRAME_CURRENT};
use crate::outside::{si_status_display_message, Outside};

/// `SI_cid.ANIMATION`, `SI_cid.STRUCT_KEYS`: chapter numbers of the scene dictionary.
pub const SI_CID_ANIMATION: u32 = 0x0878_1660;
pub const SI_CID_STRUCT_KEYS: u32 = 0x0878_16c4;
/// `FilterTrackTable`: 0x2a8 16-bit entries.
pub const FILTER_TRACK_TABLE: u32 = 0x0878_8fc0;
pub const FILTER_TRACKS: u32 = 0x2a8;
/// `SI_pDbiin`: the database input record the file is read from.
pub const SI_PDBIIN: u32 = 0x0878_2058;
/// The text buffer the match count is formatted in.
pub const WORK_STR: u32 = 0x0878_8dc0;
/// A 16-bit word of the animation settings that stops the key refresh (0x08781484).
pub const NO_KEY_REFRESH: u32 = 0x0878_1484;
const SI_GRAF_LISTS_ADDR: u32 = 0x0867_9498;
const SI_PROC_DATA_ADDR: u32 = 0x0867_9314;
const SI_RD: u32 = crate::character::SI_RD;

fn graf_list(m: &Mem) -> u32 {
    let slot = m.i32(m.u32(SI_PROC_DATA_ADDR).wrapping_add(4));
    m.u32(SI_GRAF_LISTS_ADDR).wrapping_add((slot as u32).wrapping_mul(0x24))
}

fn message(m: &mut Mem, o: &mut dyn Outside, text: u32, severity: i32) {
    si_status_display_message(o, m, text, severity);
}

/// The first element of a chapter that is in use, new (flag 1) and a hierarchy root; its
/// index goes to `*out` (−1 when there is none) [soft 0x081dd5f0 dbGetNewlyReadElement].
/// The original writes `*out` even when `out` is null.
pub fn db_get_newly_read_element(m: &mut Mem, dict: u32, chap: i32, out: u32) -> u32 {
    let list = m.u32(dict.wrapping_add(4)).wrapping_add((chap as u32).wrapping_mul(0x40));
    let nb = m.i32(list.wrapping_add(4));
    let elems = m.u32(list.wrapping_add(8));
    let mut i = 0;
    let mut found = 0u32;
    while i < nb {
        let e = elems.wrapping_add((i as u32).wrapping_mul(0x84));
        if m.i16(e.wrapping_add(0x48)) == 0 && m.i16(e.wrapping_add(0x46)) == 1 && m.i32(e.wrapping_add(0x54)) == 0 {
            found = e;
            break;
        }
        i += 1;
    }
    if out == 0 {
        crate::fault::note();
    } else {
        m.set_i32(out, if found != 0 { i } else { -1 });
    }
    found
}

/// Pairs each item of a hierarchy animation with the element it matches and counts the
/// selected ones [soft 0x081dd674 hieranimAssociateElems].
pub fn hieranim_associate_elems(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, chap_idx: i32, flag: i16, out: u32) -> i32 {
    let hier = m.u32(elem.wrapping_add(4));
    let mut count = 0;
    let chapter = m.i32(hier.wrapping_add(8));
    m.set_i32(out, chapter);
    let nb = m.i32(hier);
    let mut k = 0;
    while k < nb {
        let item = m.u32(hier.wrapping_add(4)).wrapping_add((k as u32).wrapping_mul(0x28));
        m.set_u32(item.wrapping_add(0x24), 0);
        let r = call_eax(o, m, &ao::SI_ELEMDICT_GET_ELEM_FROM_HIER_ANIM, &[dict, hier, item, 0, chap_idx as u32]) as i32;
        if r >= 0 {
            let chapter = m.u32(hier.wrapping_add(8));
            let e = call_eax(o, m, &ao::SI_ELEMDICT_GET_ELEM, &[dict, chapter, r as u32]);
            m.set_u32(item.wrapping_add(0x24), e);
            if m.i16(e.wrapping_add(0x42)) != 0 {
                count += 1;
            }
        }
        k += 1;
    }
    if count == 0 && flag != 0 && m.i32(hier) == 1 {
        count = 1;
    }
    count
}

/// Sets the filter for one track, or for all tracks when `idx` is negative
/// [soft 0x081dd71c SI_setFilterTrackTable].
pub fn si_set_filter_track_table(m: &mut Mem, val: i16, idx: i32) {
    if idx < 0 {
        for i in 0..FILTER_TRACKS {
            m.set_i16(FILTER_TRACK_TABLE + 2 * i, val);
        }
    } else {
        m.set_i16(FILTER_TRACK_TABLE.wrapping_add((idx as u32).wrapping_mul(2)), val);
    }
}

/// Copies the filter into every item of a hierarchy animation: each channel's flag is the
/// table's entry for its type [soft 0x081dd9b0 filterApplyOnHierAnim].
pub fn filter_apply_on_hier_anim(m: &mut Mem, o: &mut dyn Outside, elem: u32) {
    let hier = m.u32(elem.wrapping_add(4));
    if m.i32(hier) > 0 {
        let mut k = 0;
        loop {
            let item = m.u32(hier.wrapping_add(4)).wrapping_add((k as u32).wrapping_mul(0x28));
            k += 1;
            let nj = m.i32(item.wrapping_add(8));
            let mut j = 0;
            while j < nj {
                let t = call_eax(o, m, &ao::SI_HIERANIM_GET_TYPE, &[item, j as u32]) as i32;
                let v = m.i16(FILTER_TRACK_TABLE.wrapping_add((t as u32).wrapping_mul(2)));
                let ones = m.u32(item.wrapping_add(0x1c));
                m.set_i16(ones.wrapping_add((j as u32).wrapping_mul(2)), v);
                j += 1;
            }
            if k >= m.i32(hier) {
                break;
            }
        }
    }
}

fn update_son_state(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let father = m.u32(model.wrapping_add(0x2bc));
    call(o, m, &ao::SI_MODEL_UPDATE_SON_STATE, &[father, model, 1]);
}

/// Reads an animation (cid = animation chapter) or structure keys into the scene and
/// applies them to the selected hierarchies; returns 1 when the selection was usable, else 0
/// [soft 0x081ddcdc SI_dbLoadHierAnimFromDict]. Only the dialogs it opens are outside.
pub fn si_db_load_hier_anim_from_dict(m: &mut Mem, o: &mut dyn Outside, dict: u32, cid: i32, s: i16) -> i16 {
    let slots = m.local(24);
    let (p_new_idx, p_chap, p_idx, p_chap_out, p_anim) = (slots, slots + 4, slots + 8, slots + 12, slots + 16);
    m.set_i32(p_anim, 0);
    let cid_anim = m.i32(SI_CID_ANIMATION);
    let cid_keys = m.i32(SI_CID_STRUCT_KEYS);
    let mut count = 0;
    let mut ok: i16 = 1;
    let mut newly = db_get_newly_read_element(m, dict, cid, p_new_idx);
    if newly == 0 {
        m.release(24);
        return 0;
    }
    let n = call_eax(o, m, &ao::SI_GRAFLIST_COUNT_SEL_HIERARCHIES, &[graf_list(m), 0]) as i32;
    if n == 0 {
        message(m, o, ao::S_SELECT_FIRST, 1);
        ok = 0;
    } else if n >= 2 && cid == cid_anim {
        message(m, o, ao::S_SINGLE_ANIM, 1);
        ok = 0;
    }
    if ok != 0 {
        let b = cid == cid_anim
            || (cid == cid_keys && (call_eax(o, m, &ao::SI_ED_CHECK_STRUCTURE_KEY, &[dict]) as u16) != 0);
        if b {
            let dbiin = m.u32(SI_PDBIIN);
            call(o, m, &ao::SI_ELEMDICT_READ_ELEMENTS, &[dict, dbiin]);
            if cid == cid_anim {
                call(o, m, &ao::SI_ELEMDICT_EXPAND_ANIMATIONS, &[dict]);
                call(o, m, &ao::SI_GRAFLIST_BUILD_FROM_ELEMDICT, &[dict, SI_RD, SI_CID_ANIMATION, 0]);
                call(o, m, &ao::DIALOG_GET_ANIMATION, &[dict, p_anim]);
            }
        }
        if cid == cid_keys {
            call(o, m, &ao::SI_GET_CONSTRAINT_UPDATE_LIST, &[1, 0]);
        }
        let mut next = m.u32(graf_list(m).wrapping_add(4));
        while next != 0 {
            let node = next;
            let mut any = false;
            let mut cnt = 0;
            let mut cur = node;
            let mut left = m.u16(node.wrapping_add(0x10)) as i32 + 1;
            while left > 0 {
                if m.i32(cur.wrapping_add(8)) != 0 {
                    any = true;
                    cnt += 1;
                }
                cur = m.u32(cur.wrapping_add(0x1c));
                left -= 1;
            }
            next = cur;
            if !any {
                continue;
            }
            let flag26: i16 = if cnt == 1 { (m.i32(node.wrapping_add(8)) != 0) as i16 } else { 0 };
            let found = call_eax(o, m, &ao::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[node, p_chap, p_idx]);
            if found == 0 {
                continue;
            }
            newly = db_get_newly_read_element(m, dict, cid, p_new_idx);
            let chap = m.i32(p_chap);
            let mut esi = chap;
            if newly != 0 {
                let hier = m.u32(newly.wrapping_add(4));
                if hier != 0 {
                    esi = m.i32(hier.wrapping_add(8));
                    call(o, m, &ao::SI_ACTIONS_DISPOSE, &[hier.wrapping_add(0xc)]);
                }
            }
            if esi == chap {
                call(o, m, &ao::SI_ED_GET_SELECTED_ELEMENTS, &[dict, 0]);
                let idx = m.i32(p_idx);
                count += hieranim_associate_elems(m, o, dict, newly, idx, flag26, p_chap_out);
                if s != 0 {
                    filter_apply_on_hier_anim(m, o, newly);
                }
                let frame = m.i32(SI_FRAME_CURRENT);
                let new_idx = m.i32(p_new_idx) as u32;
                if cid == cid_anim {
                    let anim = m.u32(p_anim);
                    call(o, m, &ao::SI_ELEMDICT_SET_ANIM_RELATION, &[dict, SI_RD, newly, new_idx, found, 1, 0, flag26 as i32 as u32, anim]);
                } else {
                    call(o, m, &ao::SI_ELEMDICT_MAP_STRUCT_KEY, &[dict, SI_RD, newly, new_idx, found, 1, flag26 as i32 as u32]);
                }
                m.set_i32(SI_FRAME_CURRENT, frame);
                if m.i32(p_chap_out) == m.i32(SI_CID_MODELS) {
                    let model = m.u32(found.wrapping_add(4));
                    update_son_state(m, o, model);
                    if cid == cid_keys {
                        let f = m.i32(SI_FRAME_CURRENT);
                        let t = call_f(o, m, &ao::SI_FRAME_TO_SEC, &[f as u32]);
                        call(o, m, &ao::SI_SCENE_CONSTR_UPDATE, &[0, t, 0]);
                    }
                }
            } else {
                message(m, o, ao::S_TYPE_MISMATCH_ANIM, 2);
            }
        }
    }
    if cid == cid_anim {
        call(o, m, &ao::SI_ELEMDICT_REMOVE_ALL_EMPTY_ANIM, &[dict, cid as u32]);
        let list = m.u32(dict.wrapping_add(4)).wrapping_add((cid_anim as u32).wrapping_mul(0x40));
        call(o, m, &ao::SI_ELEMDICT_SORT_ANIM_ELEMENTS, &[list]);
    } else {
        if newly == 0 {
            newly = db_get_newly_read_element(m, dict, cid, p_new_idx);
        }
        if newly != 0 {
            let hier = m.u32(newly.wrapping_add(4));
            if hier != 0 {
                free_hier_data(m, hier);
                call(o, m, &ao::SI_HIERANIM_DISPOSE, &[hier]);
            }
            let idx = m.i32(p_new_idx) as u32;
            call(o, m, &ao::SI_ELEMDICT_SET_DELETE_ELEM, &[dict, cid_keys as u32, newly, idx, u32::MAX, 0, 1]);
        }
    }
    call(o, m, &ao::SI_ELEMDICT_SET_NEW_FLAG, &[dict, 1, 0]);
    if ok != 0 {
        if count == 0 {
            message(m, o, ao::S_NO_MATCH, 2);
        } else {
            let total = call_eax(o, m, &ao::SI_GRAFLIST_TYPE_COUNT, &[graf_list(m), 0, 0, 0, 0]) as i32;
            let line = format!("{} matche(s) out of {} item selected", count, total);
            m.put_cstr(WORK_STR, line.as_bytes());
            si_status_display_message(o, m, WORK_STR, 1);
            if m.i16(NO_KEY_REFRESH) == 0 && cid == cid_keys {
                let f = if m.i16(DOPE_MODE) != 0 { m.i16(DOPE_FRAME) as i32 } else { m.i32(SI_FRAME_CURRENT) };
                let a = call_eax(o, m, &ao::SI_SCENE_KEY_EDIT, &[0, f as u32, 0]) as u16;
                let b = call_eax(o, m, &ao::SI_SCENE_KEY_EDIT, &[1, f as u32, 0]) as u16;
                let c = call_eax(o, m, &ao::SI_SCENE_KEY_EDIT, &[2, f as u32, 1]) as u16;
                if a == 0 && b == 0 && c == 0 {
                    call(o, m, &ao::SI_REFRESH_ALL, &[0]);
                    m.release(24);
                    return ok;
                }
            } else if cid != cid_anim {
                call(o, m, &ao::SI_REFRESH_ALL, &[0]);
                m.release(24);
                return ok;
            }
            call(o, m, &ao::SI_SCENE_REFRESH, &[]);
        }
    }
    m.release(24);
    ok
}

/// A call whose answer is a float in st0, handed on as its single-precision bits.
fn call_f(o: &mut dyn Outside, m: &mut Mem, r: &'static crate::outside::Routine, args: &[u32]) -> u32 {
    o.call(m, r, args).st0.to_f32().to_bits()
}

/// Frees the data blocks of every item of a hierarchy animation and clears the slots.
fn free_hier_data(m: &mut Mem, hier: u32) {
    let nb = m.i32(hier);
    let mut k = 0;
    while k < nb {
        let item = m.u32(hier.wrapping_add(4)).wrapping_add((k as u32).wrapping_mul(0x28));
        k += 1;
        let nj = m.i32(item.wrapping_add(8));
        let mut j = 0;
        while j < nj {
            let slot = m.u32(item.wrapping_add(0x18)).wrapping_add((j as u32).wrapping_mul(4));
            let p = m.u32(slot);
            if p != 0 {
                m.free(p);
            }
            m.set_u32(slot, 0);
            j += 1;
        }
    }
}

/// Reads a library of actions into the scene and lets the dialog pick (or, with `masters`,
/// the masters dialog); returns 1 when the dialog ran on a usable selection
/// [soft 0x081debbc SI_dbLoadActionsFromDict]. `filter` applies the track filter,
/// `masters` picks the masters dialog, `arg` is handed on to it.
pub fn si_db_load_actions_from_dict(m: &mut Mem, o: &mut dyn Outside, dict: u32, filter: i16, masters: i16, arg: i16) -> u8 {
    let slots = m.local(12);
    let (p_idx0, p_chap, p_idx) = (slots, slots + 4, slots + 8);
    let cid_anim = m.i32(SI_CID_ANIMATION);
    let mut hier = 0u32;
    let mut sel_elem = 0u32;
    let first = db_get_newly_read_element(m, dict, cid_anim, p_idx0);
    if first == 0 {
        m.release(12);
        return 0;
    }
    let n = call_eax(o, m, &ao::SI_GRAFLIST_COUNT_SEL_HIERARCHIES, &[graf_list(m), 0]) as i32;
    if n > 1 {
        message(m, o, ao::S_SINGLE_ACTIONS, 1);
        m.release(12);
        return 0;
    }
    let dbiin = m.u32(SI_PDBIIN);
    call(o, m, &ao::SI_ELEMDICT_READ_ELEMENTS, &[dict, dbiin]);
    call(o, m, &ao::SI_ELEMDICT_EXPAND_ANIMATIONS, &[dict]);
    call(o, m, &ao::SI_GRAFLIST_BUILD_FROM_ELEMDICT, &[dict, SI_RD, SI_CID_ANIMATION, 0]);
    let newly = db_get_newly_read_element(m, dict, cid_anim, p_idx0);
    if newly != 0 && m.u32(newly.wrapping_add(4)) != 0 {
        hier = m.u32(newly.wrapping_add(4));
    }
    let mut ok: u8;
    if hier == 0 || m.i32(hier.wrapping_add(0xc)) < 1 || m.i32(hier.wrapping_add(0x10)) == 0 {
        message(m, o, ao::S_NO_ACTIONS, 1);
        ok = 0;
    } else {
        let mut node = m.u32(graf_list(m).wrapping_add(4));
        let mut found_node = 0u32;
        while node != 0 {
            if m.i32(node.wrapping_add(8)) != 0 {
                found_node = node;
                break;
            }
            node = m.u32(node.wrapping_add(0x1c));
        }
        if found_node != 0 {
            sel_elem = call_eax(o, m, &ao::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[found_node, p_chap, p_idx]);
            if sel_elem != 0 && m.i32(sel_elem.wrapping_add(0x54)) > 0 {
                let root = m.i32(sel_elem.wrapping_add(0x50));
                m.set_i32(p_idx, root);
                let chap = m.u32(p_chap);
                sel_elem = call_eax(o, m, &ao::SI_ELEMDICT_GET_ELEM, &[dict, chap, root as u32]);
            }
        }
        if sel_elem == 0 {
            ok = 0;
        } else {
            let chap = m.i32(p_chap);
            let idx = m.i32(p_idx);
            let nh = m.u32(newly.wrapping_add(4));
            if m.i32(nh.wrapping_add(8)) == chap {
                call(o, m, &ao::SI_ED_GET_SELECTED_ELEMENTS, &[dict, 0]);
                let mut bx: i16 = 0;
                let tc = call_eax(o, m, &ao::SI_GRAFLIST_TYPE_COUNT, &[graf_list(m), 0, 0, 0, 0]) as i32;
                if tc == 1 && m.i16(sel_elem.wrapping_add(0x42)) != 0 {
                    bx = (m.i32(hier) > 0) as i16;
                }
                if filter != 0 {
                    filter_apply_on_hier_anim(m, o, newly);
                }
                if masters == 0 {
                    call(o, m, &ao::DIALOG_GET_ACTIONS, &[dict, hier, sel_elem, chap as u32, idx as u32, bx as i32 as u32]);
                } else {
                    call(o, m, &ao::DIALOG_GET_MASTERS, &[dict, hier, sel_elem, chap as u32, idx as u32, arg as i32 as u32]);
                }
                ok = 1;
                if m.i32(hier.wrapping_add(8)) == m.i32(SI_CID_MODELS) {
                    let e = call_eax(o, m, &ao::SI_ELEMDICT_GET_ELEM, &[dict, chap as u32, idx as u32]);
                    let model = m.u32(e.wrapping_add(4));
                    update_son_state(m, o, model);
                }
            } else {
                message(m, o, ao::S_TYPE_MISMATCH_ACTIONS, 2);
                ok = 0;
            }
        }
    }
    // the temporary element goes again
    if hier == 0 {
        crate::fault::note();
        m.release(12);
        return 0;
    }
    let nb = m.i32(hier);
    let mut k = 0;
    while k < nb {
        let item = m.u32(hier.wrapping_add(4)).wrapping_add((k as u32).wrapping_mul(0x28));
        k += 1;
        let nj = m.i32(item.wrapping_add(8));
        let mut j = 0;
        while j < nj {
            let slot = m.u32(item.wrapping_add(0x18)).wrapping_add((j as u32).wrapping_mul(4));
            let data = m.u32(slot);
            let at = call_eax(o, m, &ao::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, cid_anim as u32, data]) as i32;
            m.set_i32(p_idx0, at);
            call(o, m, &ao::SI_FCURVE_REMOVE, &[slot]);
            let e = call_eax(o, m, &ao::SI_ELEMDICT_GET_ELEM, &[dict, cid_anim as u32, at as u32]);
            call(o, m, &ao::SI_ELEMDICT_SET_DELETE_ELEM, &[dict, cid_anim as u32, e, at as u32, u32::MAX, 0, 1]);
            j += 1;
        }
    }
    let e = db_get_newly_read_element(m, dict, cid_anim, p_idx0);
    let at = m.i32(p_idx0) as u32;
    call(o, m, &ao::SI_HIERANIM_DISPOSE, &[hier]);
    call(o, m, &ao::SI_ELEMDICT_SET_DELETE_ELEM, &[dict, cid_anim as u32, e, at, u32::MAX, 0, 1]);
    let list = m.u32(dict.wrapping_add(4)).wrapping_add((cid_anim as u32).wrapping_mul(0x40));
    call(o, m, &ao::SI_ELEMDICT_SORT_ANIM_ELEMENTS, &[list]);
    if ok != 0 {
        call(o, m, &ao::SI_SCENE_REFRESH, &[]);
    } else {
        ok = 0;
    }
    m.release(12);
    ok
}
