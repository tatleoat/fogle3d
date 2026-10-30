// SPDX-License-Identifier: MIT
//! `custom_dso.c`: the run-time of the custom effects that live in a shared object (finding 0041, thirteenth
//! continuation, unit custom12): the `OPTIONS:` line reader, the relation record, the symbol resolver, the exit
//! and render/save function walkers and the parameter array.
//!
//! The constant strings of the program are not in the image: a message is shown with the program's address of
//! its text (the call carries the address), and the text is compared as a literal here.

use crate::custom_io::{SI_CUS_RESOLVE_SYMBOL, SI_CUS_SET_CURRENT_DSO_EFFECT};
use crate::custom_udf::PLT_FGETS;
use crate::custom_init::SI_RELDICT_GET_NB_RES;
use crate::custom_parse::cus_init_custom_elem_array;
use crate::custom_util::*;
use crate::shdb_init::SI_STATUS_DISPLAY_MESSAGE;
use si_mem::{clib, Mem, Outside, Routine};

pub const SI_FILE_TOKEN_READ: Routine = Routine::new("SI_fileTokenRead", 0x0840_4298);
const SI_DB_IN_TOKEN: u32 = 0x0878_0f40;
const CURRENT_DSO_EFFECT: u32 = 0x086d_d0a8;
/// The five effect-class records of the program, in the order `SI_cusExit` walks them (model, motion, actor, matter, tools);
/// each holds the head of its effect list at +0xfc.
const CLASS_ORDER: [u32; 5] = [CUS_MODEL, CUS_MOTION, CUS_ACTOR, CUS_MATTER, CUS_TOOLS];

pub(crate) fn display(m: &mut Mem, o: &mut dyn Outside, text: u32) {
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[text, 3]);
}

/// [soft 0x0852e84c SI_cusSetCurrentDsoEffect]: stores the effect.
pub fn si_cus_set_current_dso_effect(m: &mut Mem, eff: u32) {
    m.set_u32(CURRENT_DSO_EFFECT, eff);
}

/// [soft 0x0852c4a0 SI_cusResolveSymbol]: the address of the one symbol of type `ty` in the dynamic link `link`
/// (its symbol list is at +4, the library handle at +8) stored through `out` (0 when there is none); 0, or 6 for a
/// null link, more than one symbol of the type, a failed lookup or a failed resolve.
pub fn si_cus_resolve_symbol(m: &mut Mem, o: &mut dyn Outside, link: u32, ty: u32, out: u32) -> u32 {
    m.set_u32(out, 0);
    if link == 0 {
        return 6;
    }
    let list = link.wrapping_add(4);
    let n = crate::custom_dyn::si_dynamic_symbol_list_get_num_sym_by_type(m, ty, list) as i32;
    if n > 1 {
        return 6;
    }
    if n == 1 {
        let local = m.local(4);
        let r = crate::custom_dyn::si_dynamic_symbol_list_get_sym_by_type(m, ty, list, 1, local);
        let sym = m.u32(local);
        m.release(4);
        if r as u16 == 0 {
            return 6;
        }
        let handle = m.u32(link.wrapping_add(8));
        if crate::custom_dyn::si_dynamic_symbol_resolve(m, o, handle, sym) != 0 {
            return 6;
        }
        m.set_u32(out, m.u32(sym.wrapping_add(8)));
    }
    0
}

/// [soft 0x0852a1b0 cusReadLine]: reads one line of the file into a block that grows by 0x80 bytes while the line
/// does not fit (each `fgets` goes on over the terminator of the last), drops "\n" or "\r\n"; the block size is
/// stored through `size` and the block through `out`. 1; 0 for a null argument or out of memory (the block is
/// then not freed). A read that finds no byte ends the line with whatever was read.
pub fn cus_read_line(m: &mut Mem, o: &mut dyn Outside, fp: u32, size: u32, out: u32) -> u32 {
    if fp == 0 || size == 0 || out == 0 {
        return 0;
    }
    let mut buf = m.si_malloc(0x80);
    if buf == 0 {
        return 0;
    }
    m.set_u8(buf, 0);
    let mut k: u32 = 1;
    loop {
        let (n, at) = if k == 1 { (0x80, buf.wrapping_add(k * 0x80 - 0x80)) } else { (0x81, buf.wrapping_add(k * 0x80 - 0x80).wrapping_sub(1)) };
        let r = o.call(m, PLT_FGETS, &[at, n, fp]).eax;
        let mut done = r == 0;
        if !done {
            let len = clib::strlen(m, buf);
            if m.u8(buf.wrapping_add(len).wrapping_sub(1)) == b'\n' {
                if m.u8(buf.wrapping_add(len).wrapping_sub(2)) == b'\r' {
                    m.set_u8(buf.wrapping_add(len).wrapping_sub(2), 0);
                } else {
                    m.set_u8(buf.wrapping_add(len).wrapping_sub(1), 0);
                }
                done = true;
            }
        }
        if done {
            m.set_u32(size, k << 7);
            m.set_u32(out, buf);
            return 1;
        }
        k += 1;
        buf = m.si_realloc(buf, k << 7);
        if buf == 0 {
            return 0;
        }
    }
}

/// `rankVals`: the names of the RANK option and the value of each (the dword after the 0x20-byte name).
const RANK_VALS: [(&[u8], u32); 6] = [(b"FIRST", 0), (b"PRE_EXP", 96), (b"POST_EXP", 98), (b"POST_IK", 100), (b"POST_SHAPE", 102), (b"POST_DEF", 115)];

/// `strtok(s, d)` with a one-character literal delimiter.
fn tok1(m: &mut Mem, s: u32, d: u8, saved: &mut u32) -> u32 {
    let p = m.local(4);
    m.set_bytes(p, &[d, 0]);
    let r = clib::strtok(m, s, p, saved);
    m.release(4);
    r
}

/// Advances over spaces, cuts at the next space (`strchr(p, ' ')` set to NUL).
fn cut_word(m: &mut Mem, mut p: u32) -> u32 {
    while m.u8(p) == b' ' {
        p = p.wrapping_add(1);
    }
    let sp = clib::strchr(m, p, b' ');
    if sp != 0 {
        m.set_u8(sp, 0);
    }
    p
}

/// [soft 0x0852a3ac SI_cusParseOperatorOptions]: reads the `OPTIONS:` line that follows (a token `OPTIONS:`, then a
/// line of `KEY=VALUE` pairs, the pairs cut at spaces) into the effect's dso record (`eff`+0x104): PICK_TIME
/// (PRE_POP / POST_POP / NONE into +0x34), RANK (a name of `rankVals` into +0x38, its value through `rank`),
/// SELECT_ICON (NO / YES into +0x3a). A bad token or value shows a message and goes on, a missing value ends the
/// line. 1, but 0 for a null argument, a missing record or `*record` = 0, or when the line cannot be read.
pub fn si_cus_parse_operator_options(m: &mut Mem, o: &mut dyn Outside, fp: u32, eff: u32, rank: u32) -> u32 {
    if fp == 0 || eff == 0 {
        return 0;
    }
    let rec = m.u32(eff.wrapping_add(0x104));
    if rec == 0 || m.u32(rec) == 0 || rank == 0 {
        return 0;
    }
    if o.call(m, SI_FILE_TOKEN_READ, &[fp]).eax == 0 {
        return 1;
    }
    if m.cstr(SI_DB_IN_TOKEN) != b"OPTIONS:" {
        return 1;
    }
    let size = m.local(8);
    let line = size.wrapping_add(4);
    let ok = cus_read_line(m, o, fp, size, line);
    if ok as u16 == 0 {
        m.release(8);
        return 0;
    }
    let buf = m.u32(line);
    let mut saved = 0u32;
    let mut key = tok1(m, buf, b'=', &mut saved);
    while key != 0 {
        let word = cut_word(m, key);
        let val = tok1(m, 0, b' ', &mut saved);
        if val == 0 {
            display(m, o, 0x086d_d1c0);
            break;
        }
        let val = cut_word(m, val);
        let (k, v) = (m.cstr(word), m.cstr(val));
        if k == b"PICK_TIME" {
            match &v[..] {
                b"PRE_POP" => m.set_u32(rec.wrapping_add(0x34), 0),
                b"POST_POP" => m.set_u32(rec.wrapping_add(0x34), 1),
                b"NONE" => m.set_u32(rec.wrapping_add(0x34), 2),
                _ => display(m, o, 0x086d_d220),
            }
        } else if k == b"RANK" {
            match RANK_VALS.iter().find(|(n, _)| *n == &v[..]) {
                Some((_, val)) => {
                    m.set_u32(rank, *val);
                    m.set_u16(rec.wrapping_add(0x38), *val as u16);
                }
                None => display(m, o, 0x086d_d280),
            }
        } else if k == b"SELECT_ICON" {
            match &v[..] {
                b"NO" => m.set_u16(rec.wrapping_add(0x3a), 0),
                b"YES" => m.set_u16(rec.wrapping_add(0x3a), 1),
                _ => display(m, o, 0x086d_d2e0),
            }
        } else {
            display(m, o, 0x086d_d340);
        }
        key = tok1(m, 0, b'=', &mut saved);
    }
    m.release(8);
    m.free(buf);
    1
}

/// Frees the relation record the way SI_cusRelDataDestroy (0x0852e33c) does (ported in si-scene): the three
/// blocks, each cleared after its free, the record and the pointer at `pp`.
fn rel_data_destroy(m: &mut Mem, pp: u32) {
    let data = m.u32(pp);
    for off in [4u32, 0x20, 0x24] {
        let p = m.u32(data.wrapping_add(off));
        if p != 0 {
            m.free(p);
            m.set_u32(data.wrapping_add(off), 0);
        }
    }
    if data != 0 {
        m.free(data);
    }
    m.set_u32(pp, 0);
}

/// [soft 0x0852e170 SI_cusRelDataCreate]: a 0x28-byte relation record for the model (the element of the model gives
/// the id printed with "%d" into the 8-byte text at +4), holding `a`..`f` at +8, +0xc, +0x18, +0x1c, +0x14, +0x10
/// and copies of the two names at +0x20 and +0x24. 0 (everything freed) when the model has no element or a block
/// cannot be had.
#[allow(clippy::too_many_arguments)]
pub fn si_cus_rel_data_create(m: &mut Mem, o: &mut dyn Outside, model: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32, name: u32, name2: u32) -> u32 {
    let rec = m.si_calloc(0x28, 0x28);
    if rec == 0 {
        return 0;
    }
    let elem = o.call(m, crate::custom_util::SI_ED_GET_ELEM_FROM_MODEL, &[model, rec]).eax;
    if elem == 0 {
        m.free(rec);
        return 0;
    }
    let local = m.local(4);
    m.set_u32(local, rec);
    let fail = |m: &mut Mem| {
        rel_data_destroy(m, local);
        let r = m.u32(local);
        m.release(4);
        r
    };
    let t = m.si_malloc(8);
    m.set_u32(rec.wrapping_add(4), t);
    if t == 0 {
        return fail(m);
    }
    let n1 = clib::strlen(m, name).wrapping_add(1);
    let a = m.si_malloc(n1);
    m.set_u32(rec.wrapping_add(0x20), a);
    if a == 0 {
        return fail(m);
    }
    let n2 = clib::strlen(m, name2).wrapping_add(1);
    let b = m.si_malloc(n2);
    m.set_u32(rec.wrapping_add(0x24), b);
    if b == 0 {
        return fail(m);
    }
    let id = m.u32(elem.wrapping_add(0x5c)) as i32;
    m.put_cstr(t, id.to_string().as_bytes());
    m.set_u32(rec.wrapping_add(8), p2);
    m.set_u32(rec.wrapping_add(0xc), p3);
    m.set_u32(rec.wrapping_add(0x14), p6);
    m.set_u32(rec.wrapping_add(0x10), p7);
    m.set_u32(rec.wrapping_add(0x18), p4);
    m.set_u32(rec.wrapping_add(0x1c), p5);
    clib::strcpy(m, a, name);
    clib::strcpy(m, b, name2);
    m.release(4);
    rec
}

/// [soft 0x0852e8dc cusEffectListProcessExit]: for each effect of the list (next at +0xfc) with a dso record
/// (+0x104, non-empty): resolves the CLEANUP_FUNCTION symbol (type 0xb; failing shows a message) and, when that has
/// none, the CUSTOM_CLEANUP_FUNCTION (type 3); with the effect made current it calls the first with a context
/// (kind 0xb, `state`), the second with the dso record's fields (the effect's +100, the count at +0xc less one when
/// the effect's +0xc is set, +0x10, +0x1c, +0x20 and the address of +0x24).
pub fn cus_effect_list_process_exit(m: &mut Mem, o: &mut dyn Outside, mut list: u32, state: u32) {
    while list != 0 {
        let rec = m.u32(list.wrapping_add(0x104));
        if rec != 0 && m.u32(rec) != 0 {
            let locals = m.local(8);
            let (l28, l2c) = (locals, locals.wrapping_add(4));
            m.set_u32(l28, 0);
            m.set_u32(l2c, 0);
            let link = m.u32(rec);
            let mut r1 = si_cus_resolve_symbol(m, o, link, 0xb, l28);
            if r1 != 0 {
                display(m, o, 0x086d_ef60);
            }
            let mut go = true;
            if m.u32(l28) == 0 {
                if r1 == 0 {
                    r1 = si_cus_resolve_symbol(m, o, m.u32(rec), 3, l2c);
                    if r1 != 0 {
                        display(m, o, 0x086d_efa0);
                        go = false;
                    }
                } else {
                    go = false;
                }
            } else if r1 != 0 {
                go = false;
            }
            if go {
                let rec = m.u32(list.wrapping_add(0x104));
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[list]);
                let (f28, f2c) = (m.u32(l28), m.u32(l2c));
                if f28 == 0 {
                    if f2c != 0 {
                        let mut count = m.u32(rec.wrapping_add(0xc));
                        if m.u32(list.wrapping_add(0xc)) != 0 {
                            count = count.wrapping_sub(1);
                        }
                        let args = [m.u32(list.wrapping_add(100)), count, m.u32(rec.wrapping_add(0x10)), m.u32(rec.wrapping_add(0x1c)), m.u32(rec.wrapping_add(0x20)), rec.wrapping_add(0x24)];
                        o.call(m, Routine::at(f2c), &args);
                    }
                } else {
                    let ctx = m.local(0x20);
                    si_cus_context_init(m, ctx, list, 0xb, state, 0xffff_ffff);
                    o.call(m, Routine::at(f28), &[ctx]);
                    m.release(0x20);
                }
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[0]);
            }
            m.release(8);
        }
        list = m.u32(list.wrapping_add(0xfc));
    }
}

/// [soft 0x0852e85c SI_cusExit]: `cusEffectListProcessExit` for the effect lists of the model, motion, actor,
/// matter and tools classes, in that order.
pub fn si_cus_exit(m: &mut Mem, o: &mut dyn Outside, state: u32) {
    for c in CLASS_ORDER {
        let head = m.u32(c.wrapping_add(0x14c));
        let list = m.u32(head.wrapping_add(0xfc));
        cus_effect_list_process_exit(m, o, list, state);
    }
}

pub(crate) fn message2(m: &mut Mem, o: &mut dyn Outside, first: &[u8], second: &[u8]) {
    let n = (first.len() + second.len() + 1) as u32;
    let buf = m.si_malloc(n);
    if buf != 0 {
        let mut t = first.to_vec();
        t.extend_from_slice(second);
        m.put_cstr(buf, &t);
        display(m, o, buf);
        m.free(buf);
    }
}

/// [soft 0x0852eab0 cusEffectListProcessCustomFunctions]: for each effect of the list with a dso record, resolves
/// the symbol of type `kind` (0xc = SAVE_FUNCTION, else RENDER_FUNCTION; a failure shows "Could not resolve ..
/// symbol for custom effect <name>"), calls it with a context (`kind`, `state`) with the effect made current, and
/// on 0x5f ("reported unrecoverable error") or 0x7c disables the effect (+0x100 = 0, short +0x118 = 1).
pub fn cus_effect_list_process_custom_functions(m: &mut Mem, o: &mut dyn Outside, mut list: u32, kind: u32, state: u32) {
    loop {
        let eff = list;
        if eff == 0 {
            return;
        }
        list = m.u32(eff.wrapping_add(0xfc));
        let rec = m.u32(eff.wrapping_add(0x104));
        if rec == 0 || m.u32(rec) == 0 {
            continue;
        }
        let l28 = m.local(4);
        m.set_u32(l28, 0);
        let mut r = si_cus_resolve_symbol(m, o, m.u32(rec), kind, l28);
        if r != 0 {
            let text: &[u8] = if kind == 0xc { b"Could not resolve SAVE_FUNCTION symbol for custom effect " } else { b"Could not resolve RENDER_FUNCTION symbol for custom effect " };
            let name = m.cstr(m.u32(eff.wrapping_add(4)));
            message2(m, o, text, &name);
        } else if m.u32(l28) != 0 {
            let f = m.u32(l28);
            o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[eff]);
            let ctx = m.local(0x20);
            si_cus_context_init(m, ctx, eff, kind, state, 0xffff_ffff);
            r = o.call(m, Routine::at(f), &[ctx]).eax;
            m.release(0x20);
            o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[0]);
        }
        m.release(4);
        if r == 0x5f {
            let text: &[u8] = if kind == 0xc { b" SAVE_FUNCTION reported unrecoverable error. Disabling effect." } else { b" RENDER_FUNCTION reported unrecoverable error.  Disabling effect." };
            let name = m.cstr(m.u32(eff.wrapping_add(4)));
            message2(m, o, &name, text);
        }
        if r == 0x5f || r == 0x7c {
            m.set_u32(eff.wrapping_add(0x100), 0);
            m.set_u16(eff.wrapping_add(0x118), 1);
        }
    }
}

/// [soft 0x0852ea24 SI_cusCallCustomFunctions]: `cusEffectListProcessCustomFunctions` for the lists of the model,
/// motion, actor, matter and tools classes.
pub fn si_cus_call_custom_functions(m: &mut Mem, o: &mut dyn Outside, kind: u32, state: u32) {
    for c in CLASS_ORDER {
        let head = m.u32(c.wrapping_add(0x14c));
        let list = m.u32(head.wrapping_add(0xfc));
        cus_effect_list_process_custom_functions(m, o, list, kind, state);
    }
}

/// [soft 0x0852e57c SI_cusParamAdd]: (six stack arguments, the third unused) inserts the 0x14-byte element `src` into the array at `*arr` after the
/// elements of the earlier parameters: `counts[idx]` is the number of elements of parameter `idx` (an empty parameter
/// still owns one slot, which a first element replaces in place); `*total` the number of slots. 1, or 0 for a null
/// argument or out of memory (`*arr` is then 0).
pub fn si_cus_param_add(m: &mut Mem, src: u32, idx: i32, counts: u32, total: u32, arr: u32) -> u32 {
    if src == 0 || counts == 0 || total == 0 || arr == 0 {
        return 0;
    }
    let cnt = |m: &Mem, i: i32| m.u32(counts.wrapping_add((i as u32).wrapping_mul(4))) as i32;
    let mut at = m.u32(arr);
    let mut pos: i32 = 0;
    if idx >= 0 {
        for i in 0..=idx {
            pos = pos.wrapping_add(cnt(m, i).max(1));
        }
    }
    if cnt(m, idx) < 1 {
        at = at.wrapping_add((pos as u32).wrapping_mul(0x14)).wrapping_sub(0x14);
    } else {
        at = if at == 0 { m.si_malloc(0x14) } else { m.si_realloc(at, ((m.u32(total) as i32).wrapping_add(1) as u32).wrapping_mul(0x14)) };
        m.set_u32(arr, at);
        if at == 0 {
            return 0;
        }
        let mut i = m.u32(total) as i32;
        while pos < i {
            let s = at.wrapping_add((i as u32).wrapping_mul(0x14)).wrapping_sub(0x14);
            let d = at.wrapping_add((i as u32).wrapping_mul(0x14));
            for k in 0..5 {
                m.set_u32(d.wrapping_add(k * 4), m.u32(s.wrapping_add(k * 4)));
            }
            i -= 1;
        }
        m.set_u32(total, m.u32(total).wrapping_add(1));
        at = at.wrapping_add((pos as u32).wrapping_mul(0x14));
    }
    for k in 0..5 {
        m.set_u32(at.wrapping_add(k * 4), m.u32(src.wrapping_add(k * 4)));
    }
    let c = counts.wrapping_add((idx as u32).wrapping_mul(4));
    m.set_u32(c, m.u32(c).wrapping_add(1));
    1
}

// ---------------------------------------------------------------------------------------------------------------
// run-time argument and result arrays, parameter and pick checks
// ---------------------------------------------------------------------------------------------------------------

const SI_RELDICT_GET_NB_ARGS: Routine = Routine::new("SI_reldictGetNbArgs", 0x0813_ec80);
const SI_RELDICT_GET_NB_RES_INSTANCES: Routine = Routine::new("SI_reldictGetNbResInstances", 0x0813_f04c);
const SI_RELDICT_GET_NB_ARG_INSTANCES: Routine = Routine::new("SI_reldictGetNbArgInstances", 0x0813_f17c);
pub(crate) const SI_RELDICT_OP_PARAM_INSTANCE_GET_GRP: Routine = Routine::new("SI_reldictOpParamInstanceGetGrp", 0x0813_f8bc);
const SI_ENMDICT_ENUM_VAL_GET_NAME_BY_ID: Routine = Routine::new("SI_enmdictEnumValGetNameById", 0x0813_89a4);
const SI_ELEMDICT_GET_ELEM_P: Routine = Routine::new("SI_elemdictGetElem", 0x0813_2b50);
pub(crate) const SI_RELDICT_P: u32 = 0x0878_15f0;
pub(crate) const SI_EVD: u32 = 0x0878_1454;
pub(crate) const CID_MODELS_P: u32 = 0x0878_1680;
pub(crate) const CID_CAMERAS: u32 = 0x0878_1664;
pub(crate) const CID_LIGHTS: u32 = 0x0878_1678;
pub(crate) const CID_MATERIALS: u32 = 0x0878_167c;
pub(crate) const CID_TEXTURES2D: u32 = 0x0878_16c8;
pub(crate) const CID_TEXTURES3D: u32 = 0x0878_16cc;
const CUS_REL_TABLE: u32 = 0x0878_1440;
const CUS_REL_COUNT: u32 = 0x0878_1444;
const PICK_MSG: u32 = 0x0882_24c0;

/// The counts of the arguments and results of operator `id` (-1: the effect's own, one argument when its +0xc is set).
fn dso_counts(m: &mut Mem, o: &mut dyn Outside, rec: u32, id: u32, nargs: u32, nres: u32) {
    if id == 0xffff_ffff {
        let has = (m.u32(rec.wrapping_add(0xc)) != 0) as u32;
        m.set_u32(nargs, has);
        m.set_u32(nres, 0);
    } else {
        let a = o.call(m, SI_RELDICT_GET_NB_ARGS, &[SI_RELDICT_P, id]).eax;
        m.set_u32(nargs, a);
        let r = o.call(m, SI_RELDICT_GET_NB_RES, &[SI_RELDICT_P, id]).eax;
        m.set_u32(nres, r);
    }
}

/// [soft 0x0852e478 si_cusDsoAllocArgsRes]: for the operator `id` (or the effect's own, -1) stores the argument and result
/// counts through `nargs`/`nres` and allocates an array of 0x14-byte elements for each non-empty count (initialised by
/// cusInitCustomElemArray), stored through `args_out`/`res_out`. 1; 0 for a null argument or out of memory (the argument
/// array is then freed).
pub fn si_cus_dso_alloc_args_res(m: &mut Mem, o: &mut dyn Outside, rec: u32, id: u32, nargs: u32, nres: u32, args_out: u32, res_out: u32) -> u32 {
    if rec == 0 || nargs == 0 || nres == 0 || args_out == 0 || res_out == 0 {
        return 0;
    }
    dso_counts(m, o, rec, id, nargs, nres);
    let n = m.u32(nargs) as i32;
    if n > 0 {
        let p = m.si_malloc((n as u32).wrapping_mul(20));
        m.set_u32(args_out, p);
        if p == 0 {
            return 0;
        }
        cus_init_custom_elem_array(m, n, p);
    }
    let n = m.u32(nres) as i32;
    if n > 0 {
        let p = m.si_malloc((n as u32).wrapping_mul(20));
        m.set_u32(res_out, p);
        if p == 0 {
            let a = m.u32(args_out);
            if a != 0 {
                m.free(a);
                m.set_u32(args_out, 0);
            }
            return 0;
        }
        cus_init_custom_elem_array(m, n, p);
    }
    1
}

/// [soft 0x0852e6b4 si_cusDsoAllocOccurrenceArrays]: as si_cusDsoAllocArgsRes, with a zeroed array of one word per
/// argument and per result (the occurrence counts).
pub fn si_cus_dso_alloc_occurrence_arrays(m: &mut Mem, o: &mut dyn Outside, rec: u32, id: u32, nargs: u32, nres: u32, args_out: u32, res_out: u32) -> u32 {
    if rec == 0 || nargs == 0 || nres == 0 || args_out == 0 || res_out == 0 {
        return 0;
    }
    dso_counts(m, o, rec, id, nargs, nres);
    let n = m.u32(nargs) as i32;
    if n > 0 {
        let p = m.si_calloc((n as u32).wrapping_mul(4), 4);
        m.set_u32(args_out, p);
        if p == 0 {
            return 0;
        }
    }
    let n = m.u32(nres) as i32;
    if n > 0 {
        let p = m.si_calloc((n as u32).wrapping_mul(4), 4);
        m.set_u32(res_out, p);
        if p == 0 {
            let a = m.u32(args_out);
            if a != 0 {
                m.free(a);
                m.set_u32(args_out, 0);
            }
            return 0;
        }
    }
    1
}

/// [soft 0x0852d3b8 cusVerifyParams]: checks that the elements of the first `n` parameters (counts[i] elements each, at
/// least one slot, 0x14 bytes apart; the id at +8) exist in the element dictionary of the chapter the relation of
/// operator `id` gives; the operator id advances by the result count of the relation (1 when `flag` is set). A missing
/// element shows 'Invalid argument' (`flag` zero) or 'Invalid result' and returns 0, else 1.
pub fn cus_verify_params(m: &mut Mem, o: &mut dyn Outside, n: i32, counts: u32, arr: u32, id: u32, flag: u32) -> u32 {
    let flag16 = flag as u16;
    let step = if flag16 == 0 { o.call(m, SI_RELDICT_GET_NB_RES, &[SI_RELDICT_P, id]).eax } else { 1 };
    let mut id = id;
    let mut at: u32 = 0;
    let mut i = 0i32;
    while i < n {
        let c = m.u32(counts.wrapping_add((i as u32).wrapping_mul(4))) as i32;
        let slots = c.max(1);
        let mut e = arr.wrapping_add(8).wrapping_add(at.wrapping_mul(0x14));
        for _ in 0..slots {
            let chap = o.call(m, crate::custom_init::SI_RELDICT_GET_CHAP, &[SI_RELDICT_P, id, flag16 as i16 as i32 as u32]).eax;
            let dict = crate::custom_fcv::elem_dicts(m);
            let want = m.u32(e);
            if o.call(m, SI_ELEMDICT_GET_ELEM_P, &[dict, chap, want]).eax == 0 {
                display_two(m, o, if flag16 != 0 { 0x086d_ee08 } else { 0x086d_ee17 });
                return 0;
            }
            e = e.wrapping_add(0x14);
        }
        id = id.wrapping_add(step);
        at = at.wrapping_add(slots as u32);
        i += 1;
    }
    1
}

fn display_two(m: &mut Mem, o: &mut dyn Outside, text: u32) {
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[text, 3]);
}

/// [soft 0x0852d300 cusVerifyInstancePicked]: for a model (class `cls`, element id `id`, in dictionary `dict`) whose
/// shape record (+4) has the kind 0xf at +0x2ec: whether one of the instances of the relation (`rel`, `inst`) is
/// in group 2 (the result instances when `res` is 1, the argument instances otherwise).
pub fn cus_verify_instance_picked(m: &mut Mem, o: &mut dyn Outside, rel: u32, dict: u32, inst: u32, cls: u32, id: u32, res: u32) -> u32 {
    if cls != m.u32(CID_MODELS_P) {
        return 0;
    }
    let e = o.call(m, SI_ELEMDICT_GET_ELEM_P, &[dict, cls, id]).eax;
    if e == 0 {
        return 0;
    }
    let obj = m.u32(e.wrapping_add(4));
    if obj == 0 || m.u32(obj.wrapping_add(0x2ec)) != 0xf {
        return 0;
    }
    let res16 = res as u16;
    let n = if res16 == 1 {
        o.call(m, SI_RELDICT_GET_NB_RES_INSTANCES, &[rel, inst]).eax
    } else {
        o.call(m, SI_RELDICT_GET_NB_ARG_INSTANCES, &[rel, inst]).eax
    } as i32;
    let mut k = 0i32;
    while k < n {
        if o.call(m, SI_RELDICT_OP_PARAM_INSTANCE_GET_GRP, &[rel, inst, k as u32, res16 as i16 as i32 as u32]).eax == 2 {
            return 1;
        }
        k += 1;
    }
    0
}

/// [soft 0x0852d0f8 cusGenPickMsg]: builds the prompt "Pick <class> object for <name>, argument|result <n+1>" in a
/// static buffer, followed (when the operator's entry in the table at 0x8781440 has a text at +0x20) by " such that
/// <text> is " and the names of the enumeration values listed in `vals` (+4 array, +8 count, [0] the table index), each
/// followed by a blank, as long as the text is not longer than 0xff. The buffer, or 0 for a class that is none of
/// model, camera, light, material or the two texture kinds.
pub fn cus_gen_pick_msg(m: &mut Mem, o: &mut dyn Outside, cls: u32, vals: u32, name: u32, res: u32, num: u32) -> u32 {
    m.set_u8(PICK_MSG, 0);
    let kind: &[u8] = if cls == m.u32(CID_MODELS_P) {
        b"model"
    } else if cls == m.u32(CID_CAMERAS) {
        b"Camera"
    } else if cls == m.u32(CID_LIGHTS) {
        b"Light"
    } else if cls == m.u32(CID_MATERIALS) {
        b"Material"
    } else if cls == m.u32(CID_TEXTURES2D) {
        b"2D Texture"
    } else if cls == m.u32(CID_TEXTURES3D) {
        b"3D Texture"
    } else {
        return 0;
    };
    let mut ent = 0u32;
    let mut text = 0u32;
    if vals != 0 {
        let idx = m.u32(vals) as i32;
        if idx < m.u32(CUS_REL_COUNT) as i32 {
            ent = m.u32(CUS_REL_TABLE).wrapping_add((idx as u32).wrapping_mul(100));
        }
        if ent != 0 {
            text = m.u32(ent.wrapping_add(0x20));
        }
    }
    let nm = m.cstr(name);
    let mut s: Vec<u8> = Vec::new();
    s.extend_from_slice(b"Pick ");
    s.extend_from_slice(kind);
    s.extend_from_slice(b" object for ");
    s.extend_from_slice(&nm);
    s.extend_from_slice(if res as u16 != 0 { b", result " } else { b", argument " });
    s.extend_from_slice((num.wrapping_add(1) as i32).to_string().as_bytes());
    m.put_cstr(PICK_MSG, &s);
    if text == 0 {
        return PICK_MSG;
    }
    s.extend_from_slice(b" such that ");
    m.put_cstr(PICK_MSG, &s);
    s.extend_from_slice(&m.cstr(text));
    m.put_cstr(PICK_MSG, &s);
    s.extend_from_slice(b" is ");
    m.put_cstr(PICK_MSG, &s);
    let count = m.u32(vals.wrapping_add(8)) as i32;
    let mut i = 0i32;
    if i < count && s.len() <= 0xff {
        loop {
            let v = m.u32(m.u32(vals.wrapping_add(4)).wrapping_add((i as u32).wrapping_mul(4)));
            let ty = m.u16(ent.wrapping_add(0x30)) as i16 as i32 as u32;
            let r = o.call(m, SI_ENMDICT_ENUM_VAL_GET_NAME_BY_ID, &[SI_EVD, ty, v]).eax;
            s.extend_from_slice(&m.cstr(r));
            m.put_cstr(PICK_MSG, &s);
            s.push(b' ');
            m.put_cstr(PICK_MSG, &s);
            i += 1;
            if i >= count || s.len() > 0xff {
                break;
            }
        }
    }
    PICK_MSG
}

#[allow(dead_code)]
const _UNUSED: Routine = SI_CUS_RESOLVE_SYMBOL;

// ---------------------------------------------------------------------------------------------------------------
// the update list of the custom effects
// ---------------------------------------------------------------------------------------------------------------

const SI_CUS_GET_NB_POST_DEF_FX: Routine = Routine::new("si_cusGetNbPostDefFX", 0x0853_58c4);

/// [soft 0x0852d910 branchDefGeomUpdateNode]: whether the update node (kind +0x14 = -200, class +0, element +4) is a
/// model whose shape record (the 0x84-byte entry of the model table) lists a geometry-definition operator (type 0xd7,
/// 0xdd, 0xe7 or 0x259) among its 0x20-byte entries (count +0x30, array +0x3c).
pub fn branch_def_geom_update_node(m: &mut Mem, node: u32) -> u32 {
    if node == 0 || m.u32(node.wrapping_add(0x14)) != 0xffff_ff38 {
        return 0;
    }
    let idx = m.u32(node.wrapping_add(4)) as i32;
    let cls = m.u16(node) as i16 as i32;
    if cls as u32 != m.u32(CID_MODELS_P) || idx < 0 {
        return 0;
    }
    let tbl = m.u32(crate::custom_fcv::elem_dicts(m).wrapping_add(4));
    let ent = (cls as u32).wrapping_shl(6).wrapping_add(tbl);
    if idx >= m.u32(ent.wrapping_add(4)) as i32 {
        return 0;
    }
    let base = m.u32(ent.wrapping_add(8));
    let obj = m.u32(base.wrapping_add((idx as u32).wrapping_mul(0x84)).wrapping_add(0x64));
    let (list, count) = (m.u32(obj.wrapping_add(0x3c)), m.u32(obj.wrapping_add(0x30)) as i32);
    let mut at = list;
    for _ in 0..count.max(0) {
        let t = m.u32(at);
        if t == 0xd7 || t == 0xdd || t == 0xe7 || t == 0x259 {
            return 1;
        }
        at = at.wrapping_add(0x20);
    }
    0
}

/// [soft 0x0852dd6c SI_cusResortUpdList]: when any post-deformation effect exists, moves the nodes of the update list
/// (count +0, 0x2c-byte nodes at +4) that belong to a rank-115 (POST_DEF) custom model effect together
/// (cusUpdSubListResort, which answers how many nodes it took; 0 counts as one); then, unless the variable
/// SI_BRANCH_DEF_SUPPORT_OFF is set, resorts for branch definitions. 1; 0 for a null list.
pub fn si_cus_resort_upd_list(m: &mut Mem, o: &mut dyn Outside, list: u32) -> u32 {
    if o.call(m, SI_CUS_GET_NB_POST_DEF_FX, &[]).eax == 0 {
        return 1;
    }
    if list == 0 {
        return 0;
    }
    let count = m.u32(list) as i32;
    let mut i = 0i32;
    while i < count {
        let node = m.u32(list.wrapping_add(4)).wrapping_add((i as u32).wrapping_mul(0x2c));
        let mut step = 1i32;
        if m.u32(node.wrapping_add(0x14)) == 0x4e20 && m.u16(node) as i16 as i32 as u32 == m.u32(CID_MODELS_P) && m.u16(node.wrapping_add(2)) == 2 {
            let eff = crate::custom_parse::cus_upd_node_get_effect(m, o, node);
            if eff != 0 {
                let rec = m.u32(eff.wrapping_add(0x104));
                if rec != 0 && m.u32(rec) != 0 && m.u16(rec.wrapping_add(0x38)) == 0x73 {
                    let r = cus_upd_sub_list_resort(m, o, node, count - i);
                    step = if r == 0 { 1 } else { r };
                }
            }
        }
        i = i.wrapping_add(step);
    }
    if o.call(m, crate::custom_misc::GETENV, &[0x086d_eeec]).eax == 0 {
        cus_upd_list_resort_for_branch_def(m, o, list);
    }
    1
}

/// [soft 0x0852dea8 SI_cusUpdateEffect]: for the custom effect of the element the update node's previous entry names:
/// when its record (+0x104) is a real effect (+0x100 = 1) of kind 0 (+0x30), resets the geometry definition of the node
/// and updates the effect (si_cusUpdateEffect, with the effect, the frame and the node).
pub fn si_cus_update_effect(m: &mut Mem, o: &mut dyn Outside, node: u32, frame: f32) {
    let idx = m.u16(node.wrapping_add(0x18)) as i16 as i32 as u32;
    let ent = m.u32(node.wrapping_add(0x1c)).wrapping_add(idx.wrapping_mul(0x14));
    let (a, b) = (m.u16(ent.wrapping_sub(0x14)) as i16 as i32 as u32, m.u32(ent.wrapping_sub(0x10)));
    let dict = crate::custom_fcv::elem_dicts(m);
    let e = o.call(m, SI_ELEMDICT_GET_ELEM_P, &[dict, a, b]).eax;
    let out = m.local(4);
    let eff = o.call(m, crate::custom_io::SI_CUS_GET_EFFECT_BY_ICON, &[m.u32(e.wrapping_add(4)), out]).eax;
    m.release(4);
    if eff != 0 && m.u32(eff.wrapping_add(0x100)) == 1 {
        let rec = m.u32(eff.wrapping_add(0x104));
        if m.u16(rec.wrapping_add(0x30)) == 0 {
            cus_reset_geometry_definition(m, o, node);
            crate::custom_dso2::si_cus_update_effect_f(m, o, eff, frame, node);
        }
    }
}

const SI_UPD_LIST_GET_CURRENT: Routine = Routine::new("SI_updListGetCurrent", 0x0822_b084);
const SI_MODEL_DELETE_DEFINITION: Routine = Routine::new("SI_modelDeleteDefinition", 0x0846_78a8);
const POST_DEF_FLAG: u32 = 0x0878_1596;

/// [soft 0x0852d540 cusResetGeometryDefinition]: for an update node of a model (kind word +2 = 2, model record at +8 whose
/// definition list (+0x38) has a first word other than the model's cached one at +0x48): unless an earlier node of the
/// current update list for the same object is a geometry-defining operator kind (0x4e20, 0xd2, 0xd7, 0xdc, 0xdd, 0xe6,
/// 0xe7, 0x258, 0x259, 0xef, 0xcf), and unless post-deformation effects exist, the flag at 0x8781596 is set, the variable
/// SI_BRANCH_DEF_SUPPORT_OFF is unset and the object's operator list holds an entry of kind -105, deletes the model's
/// definition (SI_modelDeleteDefinition) and caches the first word of the list at +0x48.
pub fn cus_reset_geometry_definition(m: &mut Mem, o: &mut dyn Outside, node: u32) {
    if m.u16(node) as i16 as i32 as u32 != m.u32(CID_MODELS_P) || m.u16(node.wrapping_add(2)) != 2 {
        return;
    }
    let mdl = m.u32(node.wrapping_add(8));
    if mdl == 0 {
        return;
    }
    let defs = m.u32(mdl.wrapping_add(0x38));
    if defs == 0 || m.u32(mdl.wrapping_add(0x48)) == m.u32(defs) {
        return;
    }
    let list = o.call(m, SI_UPD_LIST_GET_CURRENT, &[]).eax;
    let mut flag = false;
    let mut k = 0i32;
    if k < m.u32(list) as i32 {
        let base = m.u32(list.wrapping_add(4));
        if base != node {
            let mut off = 0u32;
            loop {
                let n = base.wrapping_add(off);
                if m.u32(n.wrapping_add(4)) == m.u32(node.wrapping_add(4)) && m.u16(n) == m.u16(node) && m.u16(n.wrapping_add(2)) == 2 {
                    flag = false;
                    let kind = m.u32(n.wrapping_add(0x14));
                    if matches!(kind, 0x4e20 | 0xd2 | 0xd7 | 0xdc | 0xdd | 0xe6 | 0xe7 | 0x258 | 0x259 | 0xef | 0xcf) {
                        flag = true;
                    }
                }
                off = off.wrapping_add(0x2c);
                k += 1;
                if flag {
                    return;
                }
                if k >= m.u32(list) as i32 || base.wrapping_add(off) == node {
                    break;
                }
            }
        }
    }
    if flag {
        return;
    }
    if o.call(m, SI_CUS_GET_NB_POST_DEF_FX, &[]).eax as i32 > 0 && m.u16(POST_DEF_FLAG) != 0 && o.call(m, crate::custom_misc::GETENV, &[0x086d_eed2]).eax == 0 {
        let dict = crate::custom_fcv::elem_dicts(m);
        let tbl = m.u32(dict.wrapping_add(0x60));
        let obj = m.u32(tbl.wrapping_add(m.u32(node.wrapping_add(4)).wrapping_mul(8)).wrapping_add(4));
        let (ents, count) = (m.u32(obj.wrapping_add(0x3c)), m.u32(obj.wrapping_add(0x30)) as i32);
        let mut at = ents;
        let mut c = 0i32;
        while c < count {
            if m.u32(at) == 0xffff_ff97 {
                flag = true;
            }
            c += 1;
            at = at.wrapping_add(0x20);
            if c >= count || flag {
                break;
            }
        }
    }
    if !flag {
        o.call(m, SI_MODEL_DELETE_DEFINITION, &[mdl, 1]);
        let w = m.u32(m.u32(mdl.wrapping_add(0x38)));
        m.set_u32(mdl.wrapping_add(0x48), w);
    }
}

fn copy_node(m: &mut Mem, dst: u32, src: u32) {
    let b = m.bytes(src, 0x2c).to_vec();
    m.set_bytes(dst, &b);
}

/// [soft 0x0852db6c cusUpdSubListResort]: groups the nodes of an update list that follow a POST_DEF custom model effect
/// node (`list`, at most `n` nodes): the later POST_DEF effect nodes of the same object are collected behind the first
/// one (a copy of the head node is kept in a buffer that grows by one node per collected node and is written back after
/// the nodes that were passed over), POST_DEF effect nodes of another object are resorted recursively, and the scan ends
/// at the first -200 node of the object. Returns the number of nodes handled; 0 for a null list or out of memory.
pub fn cus_upd_sub_list_resort(m: &mut Mem, o: &mut dyn Outside, list: u32, n: i32) -> i32 {
    let mut i = 1i32;
    let mut held = 1i32;
    if list == 0 {
        return 0;
    }
    let mut buf = m.si_malloc(0x2c);
    if buf == 0 {
        return 0;
    }
    copy_node(m, buf, list);
    let key = m.u32(list.wrapping_add(4));
    let mut node = list.wrapping_add(0x2c);
    let mut size = 0x2cu32;
    while i < n {
        let mut handled = false;
        if m.u32(node.wrapping_add(0x14)) == 0x4e20 && m.u16(node) as i16 as i32 as u32 == m.u32(CID_MODELS_P) && m.u16(node.wrapping_add(2)) == 2 {
            let eff = crate::custom_parse::cus_upd_node_get_effect(m, o, node);
            if eff != 0 {
                let rec = m.u32(eff.wrapping_add(0x104));
                if rec != 0 && m.u32(rec) != 0 && m.u16(rec.wrapping_add(0x38)) == 0x73 {
                    handled = true;
                    if m.u32(node.wrapping_add(4)) == key {
                        size = size.wrapping_add(0x2c);
                        held += 1;
                        buf = m.si_realloc(buf, size);
                        if buf == 0 {
                            return 0;
                        }
                        copy_node(m, buf.wrapping_add(size).wrapping_sub(0x2c), node);
                        i += 1;
                    } else {
                        let r = cus_upd_sub_list_resort(m, o, node, n - i);
                        if r > 0 {
                            let mut dst = list.wrapping_add(((i - held) as u32).wrapping_mul(0x2c));
                            let mut src = node;
                            for _ in 0..r {
                                copy_node(m, dst, src);
                                dst = dst.wrapping_add(0x2c);
                                src = src.wrapping_add(0x2c);
                            }
                        }
                        i = i.wrapping_add(r);
                    }
                }
            }
        }
        if !handled {
            if held > 0 {
                copy_node(m, list.wrapping_add(((i - held) as u32).wrapping_mul(0x2c)), node);
            }
            i += 1;
            if m.u32(node.wrapping_add(0x14)) == 0xffff_ff38 && m.u32(node.wrapping_add(4)) == key {
                break;
            }
        }
        node = list.wrapping_add((i as u32).wrapping_mul(0x2c));
    }
    if held > 0 {
        let mut dst = list.wrapping_add(((i - held) as u32).wrapping_mul(0x2c));
        for k in 0..held {
            copy_node(m, dst, buf.wrapping_add((k as u32).wrapping_mul(0x2c)));
            dst = dst.wrapping_add(0x2c);
        }
    }
    if buf != 0 {
        m.free(buf);
    }
    i
}

const SI_ELEMDICT_FIND_SON: Routine = Routine::new("SI_elemdictFindSon", 0x0813_7ff4);
const SI_ELEMDICT_FIND_NEXT_SON: Routine = Routine::new("SI_elemdictFindNextSon", 0x0813_8030);
const SI_ELEMDICT_GET_ELEM_ID: Routine = Routine::new("SI_elemdictGetElemId", 0x0813_2b94);

/// [soft 0x0852d708 processBranchDef]: for every son of the element `rec` in the element dictionary (a branch definition
/// tree), takes the entries of `post_defs` (`n_post` indices of POST_DEF effect nodes in `list`, ascending) that lie before the
/// branch node `*cur` (the index of that node in the list) and name the son's model: the nodes up to the branch node move one
/// place down and the post-deformation node takes the branch node's old place, `*cur` drops by one, and the later indices
/// of `post_defs` are lowered by one (the entry that was handled becomes `*cur` + 1). Then the sons of the son are processed the same way. 1; 0 for a null argument.
pub fn process_branch_def(m: &mut Mem, o: &mut dyn Outside, rec: u32, cur: u32, list: u32, n_post: i32, post: u32) -> u32 {
    if rec == 0 || cur == 0 || list == 0 || post == 0 {
        return 0;
    }
    let dict = |m: &Mem| crate::custom_fcv::elem_dicts(m);
    let d = dict(m);
    let mut son = o.call(m, SI_ELEMDICT_FIND_SON, &[d, rec]).eax;
    while son != 0 {
        let d = dict(m);
        let id = o.call(m, SI_ELEMDICT_GET_ELEM_ID, &[d, m.u32(CID_MODELS_P), son]).eax;
        let mut i = 0i32;
        if i < n_post && (m.u32(post) as i32) < m.u32(cur) as i32 {
            loop {
                let pidx = m.u32(post.wrapping_add((i as u32).wrapping_mul(4)));
                let nodes = m.u32(list.wrapping_add(4));
                let node = nodes.wrapping_add(pidx.wrapping_mul(0x2c));
                if m.u32(node.wrapping_add(4)) == id {
                    let tmp = m.bytes(node, 0x2c).to_vec();
                    let last = n_post - 1;
                    let mut e = pidx as i32;
                    if e < m.u32(cur) as i32 {
                        loop {
                            copy_node(m, nodes.wrapping_add((e as u32).wrapping_mul(0x2c)), nodes.wrapping_add((e as u32).wrapping_mul(0x2c)).wrapping_add(0x2c));
                            e += 1;
                            if e >= m.u32(cur) as i32 {
                                break;
                            }
                        }
                    }
                    let count = m.u32(cur);
                    m.set_bytes(nodes.wrapping_add(count.wrapping_mul(0x2c)), &tmp);
                    m.set_u32(cur, count.wrapping_sub(1));
                    let mut j = i;
                    if j < last && (m.u32(post.wrapping_add((j as u32).wrapping_mul(4)).wrapping_add(4)) as i32) < m.u32(cur) as i32 {
                        loop {
                            let v = m.u32(post.wrapping_add((j as u32).wrapping_mul(4)).wrapping_add(4)).wrapping_sub(1);
                            m.set_u32(post.wrapping_add((j as u32).wrapping_mul(4)), v);
                            j += 1;
                            if j >= last || (m.u32(post.wrapping_add((j as u32).wrapping_mul(4)).wrapping_add(4)) as i32) >= m.u32(cur) as i32 {
                                break;
                            }
                        }
                    }
                    let c = m.u32(cur).wrapping_add(1);
                    m.set_u32(post.wrapping_add((j as u32).wrapping_mul(4)), c);
                } else {
                    i += 1;
                }
                if i >= n_post || (m.u32(post.wrapping_add((i as u32).wrapping_mul(4))) as i32) >= m.u32(cur) as i32 {
                    break;
                }
            }
        }
        process_branch_def(m, o, son, cur, list, n_post, post);
        let d = dict(m);
        son = o.call(m, SI_ELEMDICT_FIND_NEXT_SON, &[d, rec]).eax;
    }
    1
}

/// [soft 0x0852d9c0 cusUpdListResortForBranchDef]: collects the indices of the POST_DEF custom model effect nodes and of the
/// branch-definition geometry nodes of an update list; unless there is no POST_DEF node, hands each branch node's model
/// record to processBranchDef, which moves the POST_DEF effect nodes of the models in its tree behind the branch node.
pub fn cus_upd_list_resort_for_branch_def(m: &mut Mem, o: &mut dyn Outside, list: u32) {
    if list == 0 {
        return;
    }
    let mut post: u32 = 0;
    let (mut n_post, mut n_br) = (0u32, 0u32);
    let mut br: u32 = 0;
    let n = m.u32(list) as i32;
    let mut node = m.u32(list.wrapping_add(4));
    let mut i = 0i32;
    let mut stop_all = false;
    let mut br_failed = false;
    while i < n {
        let mut is_post = false;
        if m.u32(node.wrapping_add(0x14)) == 0x4e20 && m.u16(node) as i16 as i32 as u32 == m.u32(CID_MODELS_P) && m.u16(node.wrapping_add(2)) == 2 {
            let eff = crate::custom_parse::cus_upd_node_get_effect(m, o, node);
            if eff != 0 {
                let rec = m.u32(eff.wrapping_add(0x104));
                if rec != 0 && m.u32(rec) != 0 && m.u16(rec.wrapping_add(0x38)) == 0x73 {
                    is_post = true;
                    n_post += 1;
                    post = m.si_realloc(post, n_post.wrapping_mul(4));
                    if post == 0 {
                        stop_all = true;
                        break;
                    }
                    m.set_u32(post.wrapping_add(n_post.wrapping_mul(4)).wrapping_sub(4), i as u32);
                }
            }
        }
        if !is_post && branch_def_geom_update_node(m, node) as u16 != 0 {
            n_br += 1;
            br = m.si_realloc(br, n_br.wrapping_mul(4));
            if br == 0 {
                br_failed = true;
                break;
            }
            m.set_u32(br.wrapping_add(n_br.wrapping_mul(4)).wrapping_sub(4), i as u32);
        }
        i += 1;
        node = node.wrapping_add(0x2c);
    }
    let _ = br_failed;
    if !stop_all && post != 0 {
        if br != 0 {
            let mut at = br;
            for k in 0..n_br {
                let idx = m.u32(br.wrapping_add(k.wrapping_mul(4)));
                let nd = m.u32(list.wrapping_add(4)).wrapping_add(idx.wrapping_mul(0x2c));
                let tbl = m.u32(crate::custom_fcv::elem_dicts(m).wrapping_add(4));
                let ent = m.u32(tbl.wrapping_add((m.u16(nd) as i16 as i32 as u32).wrapping_shl(6)).wrapping_add(8));
                let rec = ent.wrapping_add(m.u32(nd.wrapping_add(4)).wrapping_mul(0x84));
                process_branch_def(m, o, rec, at, list, n_post as i32, post);
                at = at.wrapping_add(4);
            }
        }
        if post != 0 {
            m.free(post);
        }
    }
    if br != 0 {
        m.free(br);
    }
}
