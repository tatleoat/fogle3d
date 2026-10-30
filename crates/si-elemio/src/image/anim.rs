// SPDX-License-Identifier: MIT
//! The animation file and the struct key file: `hierani.c` (the record and the element
//! list), `physicalio.c` (an item's header), `utilio.c` (the two action lists) and
//! `dictin.c` (the file as a whole).
//!
//! Each of these source files has a `token` of its own; the action lists use `utilio.c`'s,
//! which the inner token of a value also goes through.

use super::byteswap::byteswap_fread;
use super::fcurve::{fcurve_binary_read, fcv_convert_angle_unit_to_degree, staticval_binary_read};
use super::globals::*;
use super::headers::dbfile_read_bin_header;
use super::off::{actions, actions_elem, hier_anim, hier_elem, path_acts};
use super::outside::{Outside, FCLOSE, FOPEN, SI_ACTIONS_ELEM_SELECT, SI_ACTIONS_GET_NEW_NAME, SI_ELEMDICT_FIND_LIST_BY_NAME};
use super::utilio::string_binary_read;
use super::{faults, fmov, NoReturn, Run};
use si_mem::Mem;
use si_x87::F80;

/// The float an action's old frame numbers are divided by when the scene has no frame rate
/// (0x0865c6a8): 30.
pub const DEFAULT_RATE_BITS: u32 = 0x41f0_0000;
/// The double that, divided by the frame rate, is the half frame an action's times are
/// moved by (0x0865c6f8).
pub const HALF: f64 = 0.5;

/// [soft 0x081df2f8 SI_actionsInit]
/// An empty action list.
pub fn actions_init(m: &mut Mem, list: u32) {
    m.set_u32(list + actions::NB_ELEMS, 0);
    m.set_u32(list + actions::ELEMS, 0);
}

/// The double added to an element's start when a new element's place is looked for
/// (0x0864cb00).
pub const TIME_SLACK: f64 = 0.0001;

/// [soft 0x081df3e4 SI_actionsAddElemTime]
/// Puts an element into an action list in the order of time and answers its index; −1 when
/// the start is after the end or the cycle count is not positive (a start or end that is
/// not a number passes that test).
///
/// The element array grows by one record (a cleared block for the first, `SI_realloc` for
/// each further one, whose new record holds whatever the block held). The new element goes
/// in front of the first element whose start plus 0.0001 is greater than the new element's
/// **end** (the sum is compared at the x87's precision), the elements from there on are
/// moved up, and the record gets a copy of the name (without a name, one from
/// `SI_actionsGetNewName (0x081e3f0c)`), the times, the flag word, the cycle count and the
/// program's current interpolation (the word at +0x30 of `SI_glbAnim`).
pub fn actions_add_elem_time(m: &mut Mem, o: &mut dyn Outside, list: u32, name: u32, start_bits: u32, end_bits: u32, cycles: i32, flags: u32) -> i32 {
    let (start, end) = (F80::from_f32(f32::from_bits(start_bits)), F80::from_f32(f32::from_bits(end_bits)));
    if start.gt(end) || cycles <= 0 {
        return -1;
    }
    let old = m.u32(list + actions::ELEMS);
    let elems = if old == 0 { m.si_calloc(1, actions_elem::SIZE) } else { m.si_realloc(old, (m.u32(list + actions::NB_ELEMS).wrapping_add(1)).wrapping_mul(actions_elem::SIZE)) };
    m.set_u32(list + actions::ELEMS, elems);
    if m.u32(list + actions::ELEMS) == 0 {
        m.set_u32(list + actions::NB_ELEMS, 0);
        return -1;
    }
    let count = m.i32(list + actions::NB_ELEMS);
    let mut index: i32 = 0;
    if index < count {
        let slack = F80::from_f64(TIME_SLACK);
        let elems = m.u32(list + actions::ELEMS);
        let mut found = (slack + F80::from_f32(m.f32(elems + actions_elem::START))).gt(end);
        if !found {
            let mut at = 0u32;
            loop {
                at = at.wrapping_add(actions_elem::SIZE);
                index += 1;
                if index >= count {
                    break;
                }
                let elems = m.u32(list + actions::ELEMS);
                if (slack + F80::from_f32(m.f32(elems.wrapping_add(at).wrapping_add(actions_elem::START)))).gt(end) {
                    found = true;
                    break;
                }
            }
        }
        if found {
            let n = m.i32(list + actions::NB_ELEMS);
            if index < n {
                let elems = m.u32(list + actions::ELEMS);
                let from = elems.wrapping_add((index as u32).wrapping_mul(actions_elem::SIZE));
                si_mem::clib::memmove(m, from.wrapping_add(actions_elem::SIZE), from, ((n - index) as u32).wrapping_mul(actions_elem::SIZE));
            }
        }
    }
    let e = m.u32(list + actions::ELEMS).wrapping_add((index as u32).wrapping_mul(actions_elem::SIZE));
    let text = if name == 0 { o.call(m, &SI_ACTIONS_GET_NEW_NAME, &[list, 0]) } else { m.strdup(name) };
    m.set_u32(e + actions_elem::NAME, text);
    m.set_u32(e + actions_elem::START, fmov(start_bits));
    m.set_u32(e + actions_elem::END, fmov(end_bits));
    m.set_u32(e + actions_elem::FLAGS, flags);
    m.set_u32(e + actions_elem::WORD_1C, 0);
    m.set_u32(e + actions_elem::WORD_20, 0);
    let interpolation = m[SI_GLB_ANIM_30];
    m.set_u16(e + actions_elem::INTERPOLATION, interpolation);
    m.set_i32(e + actions_elem::CYCLES, cycles);
    m.set_u32(e + actions_elem::NAME2, 0);
    m.set_u32(e + actions_elem::PATH, 0);
    let n = m.u32(list + actions::NB_ELEMS);
    m.set_u32(list + actions::NB_ELEMS, n.wrapping_add(1));
    index
}

/// [soft 0x08138aec SI_hieranimInit]
/// An empty animation record for a chapter: no elements, two empty action lists.
pub fn hieranim_init(m: &mut Mem, anim: u32, chapter: i32) {
    m.set_u32(anim + hier_anim::NB_ELEMS, 0);
    m.set_u32(anim + hier_anim::ELEMS, 0);
    m.set_i32(anim + hier_anim::CHAPTER, chapter);
    actions_init(m, anim + hier_anim::ACTIONS);
    actions_init(m, anim + hier_anim::ACTIONS2);
}

/// [soft 0x08138b28 SI_hieranimAllocate]
/// A new, empty animation record (`SI_malloc(0x1c)`; no test of the result).
///
/// Not done, here and in the two routines below: the original increments
/// `SI_secureLicAlloc` (0x08679660) before each allocation; that counter belongs to the
/// licence scheme, which is not ported.
pub fn hieranim_allocate(m: &mut Mem, chapter: i32) -> u32 {
    let anim = m.si_malloc(hier_anim::SIZE);
    hieranim_init(m, anim, chapter);
    anim
}

/// [soft 0x08138cc4 SI_hieranimAddElem]
/// Appends an element (a copy of the name, the father's index, no items) and answers its
/// index. The element array grows by one record: `SI_malloc(0x28)` for the first,
/// `SI_realloc` for each further one.
pub fn hieranim_add_elem(m: &mut Mem, anim: u32, name: u32, father: i32) -> i32 {
    let n = m.i32(anim + hier_anim::NB_ELEMS);
    let old = m.u32(anim + hier_anim::ELEMS);
    let elems = if old == 0 { m.si_malloc(hier_elem::SIZE) } else { m.si_realloc(old, ((n as u32).wrapping_mul(5).wrapping_add(5)).wrapping_mul(8)) };
    m.set_u32(anim + hier_anim::ELEMS, elems);
    let e = m.u32(anim + hier_anim::ELEMS).wrapping_add((n as u32).wrapping_mul(hier_elem::SIZE));
    let copy = m.strdup(name);
    m.set_u32(e + hier_elem::NAME, copy);
    m.set_i32(e + hier_elem::FATHER, father);
    for off in [hier_elem::NB_ITEMS, hier_elem::DATA, hier_elem::TYPES, hier_elem::ONES, hier_elem::FLAGS, hier_elem::REL_IDS, hier_elem::OCC_IDS, hier_elem::WORD_24] {
        m.set_u32(e + off, 0);
    }
    let count = m.i32(anim + hier_anim::NB_ELEMS);
    m.set_i32(anim + hier_anim::NB_ELEMS, count.wrapping_add(1));
    n
}

/// [soft 0x08138e0c SI_hieranimAddItem]
/// Appends an item to the element of that index (nothing for an index outside the list):
/// its data, type, action relation id and occurrence id, a 1, and the flag word, one in
/// each of the element's six arrays. The arrays are six blocks, allocated for one entry by
/// the first item and grown by one entry (`SI_realloc`) by each further one.
pub fn hieranim_add_item(m: &mut Mem, anim: u32, owner: i32, kind: u16, rel_id: u32, occ_id: u16, flag: u16, data: u32) {
    if owner < 0 || owner >= m.i32(anim + hier_anim::NB_ELEMS) {
        return;
    }
    let e = m.u32(anim + hier_anim::ELEMS).wrapping_add((owner as u32).wrapping_mul(hier_elem::SIZE));
    let n = m.u32(e + hier_elem::NB_ITEMS);
    if m.u32(e + hier_elem::DATA) == 0 {
        for (off, size) in [(hier_elem::DATA, 4), (hier_elem::TYPES, 2), (hier_elem::REL_IDS, 4), (hier_elem::OCC_IDS, 2), (hier_elem::ONES, 2), (hier_elem::FLAGS, 2)] {
            let p = m.si_malloc(size);
            m.set_u32(e + off, p);
        }
    } else {
        let four = n.wrapping_add(1).wrapping_mul(4);
        let two = n.wrapping_add(1).wrapping_mul(2);
        for (off, size) in [(hier_elem::DATA, four), (hier_elem::TYPES, two), (hier_elem::REL_IDS, four), (hier_elem::OCC_IDS, two), (hier_elem::ONES, two), (hier_elem::FLAGS, two)] {
            let old = m.u32(e + off);
            let p = m.si_realloc(old, size);
            m.set_u32(e + off, p);
        }
    }
    let p = m.u32(e + hier_elem::DATA);
    m.set_u32(p.wrapping_add(n.wrapping_mul(4)), data);
    let p = m.u32(e + hier_elem::TYPES);
    m.set_u16(p.wrapping_add(n.wrapping_mul(2)), kind);
    let p = m.u32(e + hier_elem::REL_IDS);
    m.set_u32(p.wrapping_add(n.wrapping_mul(4)), rel_id);
    let p = m.u32(e + hier_elem::OCC_IDS);
    m.set_u16(p.wrapping_add(n.wrapping_mul(2)), occ_id);
    let p = m.u32(e + hier_elem::ONES);
    m.set_u16(p.wrapping_add(n.wrapping_mul(2)), 1);
    let p = m.u32(e + hier_elem::FLAGS);
    m.set_u16(p.wrapping_add(n.wrapping_mul(2)), flag);
    let count = m.u32(e + hier_elem::NB_ITEMS);
    m.set_u32(e + hier_elem::NB_ITEMS, count.wrapping_add(1));
}

/// [soft 0x08139094 si_hieranimBinaryRead]
/// The element list of an animation or struct key file: an optional chapter name (token 3,
/// only as the first token; looked up with `SI_elemdictFindListByName (0x081325f4)`,
/// otherwise the MODELS chapter), then per element its name (1) and its father's index
/// (2), which adds the element; closed by token 0. The record is initialised for the
/// chapter first.
///
/// A name token that no father token follows loses its block when the next name comes.
pub fn hieranim_binary_read(m: &mut Mem, o: &mut dyn Outside, dict: u32, anim: u32, stream: u32) -> Run<()> {
    let before = faults(m);
    let token_read = |m: &mut Mem, o: &mut dyn Outside| -> Run<u16> {
        let got = byteswap_fread(m, o, TOKEN_HIERANI.addr(), 2, 1, stream);
        let token = m[TOKEN_HIERANI];
        if got == 0 && token != 0 {
            return Err(NoReturn::Endless("si_hieranimBinaryRead"));
        }
        Ok(token)
    };
    let mut token = token_read(m, o)?;
    let mut name = 0u32;
    let mut chapter = m[SI_CID_MODELS];
    if token == 3 {
        let text = string_binary_read(m, o, stream)?;
        chapter = o.call(m, &SI_ELEMDICT_FIND_LIST_BY_NAME, &[dict, text]) as i32;
        token = token_read(m, o)?;
        if text != 0 {
            m.free(text);
        }
    }
    hieranim_init(m, anim, chapter);
    while token != 0 {
        if token == 1 {
            name = string_binary_read(m, o, stream)?;
        } else if token == 2 {
            byteswap_fread(m, o, SI_SVALUE_HIERANI.addr(), 2, 1, stream);
            let father = m[SI_SVALUE_HIERANI] as i32;
            hieranim_add_elem(m, anim, name, father);
            if name != 0 {
                m.free(name);
                name = 0;
            }
        }
        if faults(m) != before {
            return Err(NoReturn::Fault("si_hieranimBinaryRead"));
        }
        token = token_read(m, o)?;
    }
    Ok(())
}

/// [soft 0x08110284 SI_elemdictReadAnimation]
/// One item of an animation or struct key file: a header of tokens (1 the owner's index, 4
/// the type, 5 and 6 an action relation's id and occurrence, 7 a flag word; 2 a text and 3
/// a number, read and dropped), then for an item that named its owner the function curve
/// (type 0) or the static value (type 1). Answers 0 when there is no further item: the
/// first token is 0, 10 (the first action list) or 99, or no owner token came.
///
/// The header loop sets its token to 0 before every read, so the end of the file ends the
/// items too. The results go through the pointers; `*occ_id` is stored only by token 6 and
/// `*owner` only by token 1. `old` is not 0 for a file without a header, whose angle
/// curves are then turned from radians to degrees. The first argument (the file's name) is
/// not looked at. The text under token 2 is never freed.
#[allow(clippy::too_many_arguments)]
pub fn elemdict_read_animation(m: &mut Mem, o: &mut dyn Outside, _path: u32, stream: u32, data: u32, kind: u32, owner: u32, rel_id: u32, occ_id: u32, old: i16, flag: u32) -> Run<i32> {
    let sp = m.sp();
    let value_local = m.local(4);
    let curve_local = m.local(4);
    m.set_u32(curve_local, 0);
    m.set_u32(data, 0);
    m.set_u32(kind, 0);
    m.set_u16(flag, 1);
    m.set_u32(rel_id, 0xffff_ffff);
    byteswap_fread(m, o, TOKEN_PHYSICALIO.addr(), 2, 1, stream);
    let mut got: i16 = 0;
    let run = (|| -> Run<i32> {
        loop {
            let token = m[TOKEN_PHYSICALIO];
            if token == 0 || token == 99 || token == 10 {
                break;
            }
            match token {
                1 => {
                    byteswap_fread(m, o, SI_SVALUE.addr(), 2, 1, stream);
                    let v = m[SI_SVALUE] as i32;
                    m.set_i32(owner, v);
                    got = 1;
                }
                2 => {
                    string_binary_read(m, o, stream)?;
                }
                3 => {
                    byteswap_fread(m, o, SI_SVALUE.addr(), 2, 1, stream);
                }
                4..=6 => {
                    byteswap_fread(m, o, SI_SVALUE.addr(), 2, 1, stream);
                    let v = m[SI_SVALUE] as i32;
                    m.set_i32(if token == 4 { kind } else if token == 5 { rel_id } else { occ_id }, v);
                }
                7 => {
                    byteswap_fread(m, o, SI_SVALUE.addr(), 2, 1, stream);
                    let v = m[SI_SVALUE];
                    m.set_i16(flag, v);
                }
                _ => {}
            }
            m[TOKEN_PHYSICALIO] = 0;
            byteswap_fread(m, o, TOKEN_PHYSICALIO.addr(), 2, 1, stream);
        }
        if got != 0 {
            let k = m.i32(kind);
            if k == 1 {
                got = staticval_binary_read(m, o, stream, value_local)? as i16;
                let v = m.u32(value_local);
                m.set_u32(data, v);
            } else if k == 0 {
                got = fcurve_binary_read(m, o, stream, curve_local, 0)? as i16;
                let c = m.u32(curve_local);
                m.set_u32(data, c);
                if old != 0 {
                    fcv_convert_angle_unit_to_degree(m, c);
                }
            }
        }
        Ok(got as i32)
    })();
    m.set_sp(sp);
    run
}

/// [soft 0x083ff614 SI_actionsElemBinaryRead]
/// One element of an action list: tokens up to 0x11 (finding 0005 has the table), the
/// results through the eleven pointers. Answers 1 when a name token (0x0c) came. The loop
/// starts with the token the caller read and sets its token to 0x11 before every read of
/// its own, so the end of the file ends the element.
///
/// A path record of 0x1c bytes is allocated before anything is read and freed again when
/// no path token (0x16 to 0x1a) made it the element's. The old tokens 0x0d and 0x0e hold
/// frame numbers, divided on the x87 by the scene's frame rate (30 when that is 0).
///
/// Not done: the increment of `SI_secureLicAlloc` (0x08679660) before the allocation.
#[allow(clippy::too_many_arguments)]
pub fn actions_elem_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, name: u32, name2: u32, start: u32, end: u32, clip_start: u32, clip_end: u32, cycles: u32, flag10: u32, interpolation: u32, path: u32, selected: u32) -> Run<u32> {
    let sp = m.sp();
    let curve_local = m.local(4);
    let mut named: u16 = 0;
    m.set_u32(curve_local, 0);
    m.set_u32(name, 0);
    m.set_u32(name2, 0);
    m.set_u32(start, 0);
    m.set_u32(end, 0);
    m.set_u32(clip_start, 0);
    m.set_u32(clip_end, 0);
    m.set_u16(flag10, 0);
    m.set_u16(selected, 0);
    m.set_u32(cycles, 1);
    m.set_u16(interpolation, 5);
    m.set_u32(path, 0);
    let record = m.si_calloc(1, path_acts::SIZE);
    m.set_u32(record + path_acts::WORD_00, 0xffff_ffff);
    m.set_u32(record + path_acts::WORD_04, 0xffff_ffff);
    let rate_bits = m[SI_FRAME_RATE];
    // 0 compared with the rate: equal (and not unordered) keeps the 30
    let rate = if f32::from_bits(rate_bits) == 0.0 { f32::from_bits(DEFAULT_RATE_BITS) } else { f32::from_bits(fmov(rate_bits)) };
    let run = (|| -> Run<u32> {
        loop {
            let token = m[TOKEN_UTILIO];
            if token == 0x11 {
                if m.u32(path) == 0 && record != 0 {
                    m.free(record);
                }
                return Ok(named as u32);
            }
            match token {
                0x0c => {
                    let s = string_binary_read(m, o, stream)?;
                    m.set_u32(name, s);
                    named = 1;
                }
                0x0d | 0x0e => {
                    byteswap_fread(m, o, SI_IVALUE.addr(), 4, 1, stream);
                    let v = (F80::from_i32(m[SI_IVALUE]) / F80::from_f32(rate)).to_f32();
                    m.set_u32(if token == 0x0d { start } else { end }, v.to_bits());
                }
                0x0f => m.set_u16(selected, 1),
                0x10 => m.set_u16(flag10, 1),
                0x12 | 0x13 | 0x1c | 0x1d => {
                    byteswap_fread(m, o, SI_FVALUE.addr(), 4, 1, stream);
                    let v = fmov(m[SI_FVALUE]);
                    m.set_u32(match token { 0x12 => start, 0x13 => end, 0x1c => clip_start, _ => clip_end }, v);
                }
                0x14 => {
                    byteswap_fread(m, o, SI_IVALUE.addr(), 4, 1, stream);
                    let v = m[SI_IVALUE];
                    m.set_i32(cycles, v);
                    if v == 0 {
                        m.set_i32(cycles, 1);
                    }
                }
                0x15 => {
                    byteswap_fread(m, o, SI_SVALUE.addr(), 2, 1, stream);
                    let v = m[SI_SVALUE];
                    m.set_i16(interpolation, v);
                }
                0x16..=0x19 => {
                    m.set_u32(path, record);
                    let s = string_binary_read(m, o, stream)?;
                    let off = match token { 0x16 => path_acts::TEXT_10, 0x17 => path_acts::TEXT_0C, 0x18 => path_acts::TEXT_18, _ => path_acts::TEXT_14 };
                    m.set_u32(record + off, s);
                }
                0x1a => {
                    m.set_u32(path, record);
                    fcurve_binary_read(m, o, stream, curve_local, 0)?;
                    let c = m.u32(curve_local);
                    if c != 0 {
                        m.set_u32(record + path_acts::CURVE, c);
                    }
                }
                0x1b => {
                    let s = string_binary_read(m, o, stream)?;
                    m.set_u32(name2, s);
                }
                _ => {}
            }
            m[TOKEN_UTILIO] = 0x11;
            byteswap_fread(m, o, TOKEN_UTILIO.addr(), 2, 1, stream);
        }
    })();
    m.set_sp(sp);
    run
}

/// [soft 0x083ff938 SI_actionsBinaryRead]
/// One of the two action lists of an animation file (`which` 0: the first, closed by
/// token 0x0b; else the second, closed by 0x1f): the list is emptied, then elements are
/// read until the closing token or an element without a name. Each named element is handed
/// to `SI_actionsAddElemTime` with its times moved outwards by half a frame
/// (0.5 / `SI_frameRate`, on the x87; nothing when the rate is 0); when that added it, the
/// reader stores the interpolation, the second name, the path record and the clip times,
/// and selects it (`SI_actionsElemSelect (0x081e46b8)`) when the file says so.
///
/// The loop sets its token to the closing token before every read, so the end of the file
/// ends the list. The first read is not so protected: at the end of the file it leaves
/// what `utilio.c`'s token held, and that goes through the element reader once.
pub fn actions_binary_read(m: &mut Mem, o: &mut dyn Outside, anim: u32, stream: u32, which: i16) -> Run<()> {
    let sp = m.sp();
    // the eleven locals the element reader fills
    let l = m.local(0x2c);
    let (name, name2, start, end, clip_start, clip_end, cycles, flag10, interpolation, path, selected) =
        (l, l + 4, l + 8, l + 0xc, l + 0x10, l + 0x14, l + 0x18, l + 0x1c, l + 0x1e, l + 0x20, l + 0x24);
    m.set_u32(path, 0);
    let rate_bits = m[SI_FRAME_RATE];
    let half = if f32::from_bits(rate_bits) == 0.0 { 0.0f32 } else { (F80::from_f64(HALF) / F80::from_f32(f32::from_bits(rate_bits))).to_f32() };
    let (list, closing) = if which != 0 { (anim.wrapping_add(hier_anim::ACTIONS2), 0x1fu16) } else { (anim.wrapping_add(hier_anim::ACTIONS), 0x0bu16) };
    actions_init(m, list);
    byteswap_fread(m, o, TOKEN_UTILIO.addr(), 2, 1, stream);
    let mut ok: u16 = 1;
    let run = (|| -> Run<()> {
        while m[TOKEN_UTILIO] != closing && ok != 0 {
            ok = actions_elem_binary_read(m, o, stream, name, name2, start, end, clip_start, clip_end, cycles, flag10, interpolation, path, selected)? as u16;
            if ok != 0 {
                let flags = if m.u16(flag10) != 0 { 2 } else { 10 };
                let later = (F80::from_f32(half) + F80::from_f32(m.f32(end))).to_f32();
                let earlier = (F80::from_f32(m.f32(start)) - F80::from_f32(half)).to_f32();
                let (n, c) = (m.u32(name), m.u32(cycles));
                let index = actions_add_elem_time(m, o, list, n, earlier.to_bits(), later.to_bits(), c as i32, flags);
                if index >= 0 && index < m.i32(list + actions::NB_ELEMS) {
                    let e = m.u32(list + actions::ELEMS).wrapping_add((index as u32).wrapping_mul(actions_elem::SIZE));
                    let v = m.u16(interpolation);
                    m.set_u16(e + actions_elem::INTERPOLATION, v);
                    let v = m.u32(name2);
                    m.set_u32(e + actions_elem::NAME2, v);
                    let v = m.u32(path);
                    m.set_u32(e + actions_elem::PATH, v);
                    let v = fmov(m.u32(clip_start));
                    m.set_u32(e + actions_elem::CLIP_START, v);
                    let v = fmov(m.u32(clip_end));
                    m.set_u32(e + actions_elem::CLIP_END, v);
                    if m.u16(selected) != 0 {
                        o.call(m, &SI_ACTIONS_ELEM_SELECT, &[list, e, 1, 1]);
                    }
                }
            }
            let n = m.u32(name);
            if n != 0 {
                m.free(n);
                m.set_u32(name, 0);
            }
            m[TOKEN_UTILIO] = closing;
            byteswap_fread(m, o, TOKEN_UTILIO.addr(), 2, 1, stream);
        }
        Ok(())
    })();
    m.set_sp(sp);
    run
}

/// [soft 0x08127760 SI_elemdictReadAnimations]
/// Reads an animation or struct key file and answers its `SI_HierAnim`: the header, the
/// element list, the items (each added to its owner's arrays), the two action lists. Null,
/// and a line on the standard error, when the file cannot be opened. With `SI_pipedFiles`
/// set the stream is `SI_pipe` and is left open.
///
/// The version the header gave becomes `SI_versionInput` (−1 for a file without a header,
/// whose angle curves are then converted). An item's occurrence id is a local nobody set
/// unless the file has token 6 (no sample file has): here it is what the image's stack
/// holds.
pub fn elemdict_read_animations(m: &mut Mem, o: &mut dyn Outside, dict: u32, path: u32) -> Run<u32> {
    let stream = if m[SI_PIPED_FILES] == 0 { o.call(m, &FOPEN, &[path, MODE_ANIMATION]) } else { m[SI_PIPE] };
    m[FILE_DICTIN] = stream;
    if stream == 0 {
        let mut text = CANNOT_OPEN.to_vec();
        text.extend_from_slice(&m.cstr(path));
        text.push(b'\n');
        o.print_err(m, &text);
        return Ok(0);
    }
    let sp = m.sp();
    let version = m.local(4);
    let l = m.local(0x18);
    let (data, kind, owner, rel_id, occ_id, flag) = (l, l + 4, l + 8, l + 0xc, l + 0x10, l + 0x14);
    let run = (|| -> Run<u32> {
        let anim = hieranim_allocate(m, 0);
        let stream = m[FILE_DICTIN];
        let magic = dbfile_read_bin_header(m, o, stream, version, WORK_STR.addr());
        let v = fmov(m.u32(version));
        m[SI_VERSION_INPUT] = v;
        let stream = m[FILE_DICTIN];
        hieranim_binary_read(m, o, dict, anim, stream)?;
        loop {
            let stream = m[FILE_DICTIN];
            let more = elemdict_read_animation(m, o, path, stream, data, kind, owner, rel_id, occ_id, (magic == -1) as i16, flag)?;
            if more as i16 == 0 {
                break;
            }
            let (d, f, oc, r, k, ow) = (m.u32(data), m.i16(flag), m.u32(occ_id), m.u32(rel_id), m.u32(kind), m.i32(owner));
            hieranim_add_item(m, anim, ow, k as u16, r, oc as u16, f as u16, d);
        }
        let stream = m[FILE_DICTIN];
        actions_binary_read(m, o, anim, stream, 0)?;
        let stream = m[FILE_DICTIN];
        actions_binary_read(m, o, anim, stream, 1)?;
        if m[SI_PIPED_FILES] == 0 {
            let stream = m[FILE_DICTIN];
            o.call(m, &FCLOSE, &[stream]);
        }
        Ok(anim)
    })();
    m.set_sp(sp);
    run
}
