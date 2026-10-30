// SPDX-License-Identifier: MIT
//! `custom_dso.c`, second part (finding 0041, fourteenth continuation, unit custom13): the update of one effect, the
//! pick/argument/result machinery and the dynamic-link loaders. The constant strings of the program are not in the
//! image: a message is shown with the program's address of its text (the call carries the address) or as text built on
//! the heap (the checks compare both).

use crate::custom_dso::*;
use crate::custom_fcv::elem_dicts;
use crate::custom_io::{SI_CUS_DSO_ALLOC_ARGS_RES, SI_CUS_DSO_ALLOC_OCCURRENCE_ARRAYS, SI_CUS_EFFECT_LOAD_DSO, SI_CUS_RESOLVE_SYMBOL, SI_CUS_SET_CURRENT_DSO_EFFECT};
use crate::custom_udf::SI_CUS_DIALOG_HANDLE;
use crate::custom_parse::SI_CUS_DIALOG_SET;
use crate::custom_udf::SI_ELEMDICT_FIND_ELEM_BY_DATA;
use crate::shdb_init::SI_STATUS_DISPLAY_MESSAGE;
use crate::custom_enc::{st0_f32, SI_FCURVE_VALUE};
use crate::custom_util::*;
use si_mem::{clib, Mem, Outside, Routine};

/// `SI_cusUpdateState`, the state word the update functions are given.
const SI_CUS_UPDATE_STATE: u32 = 0x086e_49ec;

/// [soft 0x0852df3c si_cusUpdateEffect]: evaluates the effect's value curves at `frame` (each value of the list at
/// effect+0x64 whose kind short (+0xc) is 0 and that has a curve (+0x2c) gets the curve's value, stored as a float at
/// +0x10), finds the update node's entry in the dso record's occurrence table (record +0x20, count +0x1c, 0x14-byte
/// entries matching on the short at +4, the word at +8 and the short at +0xc; -1 when absent), makes the effect
/// current and calls the record's UPDATE function: the context form (+0x2c: context kind 10, the update state, the
/// entry index) or else the argument form (+0x28: the effect's +0x64, the count at +0xc less one when the effect's
/// +0xc is set, +0x10, +0x1c, +0x20, the index, the address of +0x24). The effect is made not current again; the
/// results 0x5f and 0x7c disable the effect (+0x100 = 0, short +0x118 = 1), 0x5f with the message "<name> UPDATE_FUNCTION
/// reported unrecoverable error.". Nothing is done when the record has neither function.
pub fn si_cus_update_effect_f(m: &mut Mem, o: &mut dyn Outside, eff: u32, frame: f32, node: u32) {
    let rec = m.u32(eff.wrapping_add(0x104));
    if m.u32(rec.wrapping_add(0x28)) == 0 && m.u32(rec.wrapping_add(0x2c)) == 0 {
        return;
    }
    let mut v = m.u32(eff.wrapping_add(0x64));
    while v != 0 {
        let fc = m.u32(v.wrapping_add(0x2c));
        if fc != 0 && m.u16(v.wrapping_add(0xc)) == 0 {
            let r = o.call(m, SI_FCURVE_VALUE, &[fc, frame.to_bits(), 0]).st0.unwrap_or([0; 10]);
            m.set_f32(v.wrapping_add(0x10), st0_f32(r));
        }
        v = m.u32(v.wrapping_add(0x44));
    }
    let n = m.u32(rec.wrapping_add(0x1c)) as i32;
    let tab = m.u32(rec.wrapping_add(0x20));
    let (k0, k1, k2) = (m.u16(node) as i16 as i32 as u32, m.u32(node.wrapping_add(4)), m.u16(node.wrapping_add(2)) as i16 as i32 as u32);
    let mut idx: i32 = 0;
    let mut e = tab;
    while idx < n {
        if m.u32(e.wrapping_add(4)) == k0 && m.u32(e.wrapping_add(8)) == k1 && m.u32(e.wrapping_add(0xc)) == k2 {
            break;
        }
        idx += 1;
        e = e.wrapping_add(0x14);
    }
    if idx == n {
        idx = -1;
    }
    si_cus_set_current_dso_effect(m, eff);
    let mut r: u32 = 0;
    if m.u32(rec.wrapping_add(0x2c)) != 0 {
        let ctx = m.local(0x20);
        si_cus_context_init(m, ctx, eff, 10, m.u32(SI_CUS_UPDATE_STATE), idx as u32);
        r = o.call(m, Routine::at(m.u32(rec.wrapping_add(0x2c))), &[ctx]).eax;
        m.release(0x20);
    } else if m.u32(rec.wrapping_add(0x28)) != 0 {
        let mut count = m.u32(rec.wrapping_add(0xc));
        if m.u32(eff.wrapping_add(0xc)) != 0 {
            count = count.wrapping_sub(1);
        }
        let args = [m.u32(eff.wrapping_add(0x64)), count, m.u32(rec.wrapping_add(0x10)), m.u32(rec.wrapping_add(0x1c)), m.u32(rec.wrapping_add(0x20)), idx as u32, rec.wrapping_add(0x24)];
        r = o.call(m, Routine::at(m.u32(rec.wrapping_add(0x28))), &args).eax;
    }
    si_cus_set_current_dso_effect(m, 0);
    if r == 0x5f || r == 0x7c {
        m.set_u32(eff.wrapping_add(0x100), 0);
        m.set_u16(eff.wrapping_add(0x118), 1);
        if r == 0x5f {
            let name = m.cstr(m.u32(eff.wrapping_add(4)));
            message2(m, o, &name, b" UPDATE_FUNCTION reported unrecoverable error.");
        }
    }
}

pub const SI_STATUS_DISPLAY_MOUSE: Routine = Routine::new("SI_statusDisplayMouse", 0x083c_c650);
pub const SI_ELEMDICT_FIND_ELEM_FROM_GRAFNODE: Routine = Routine::new("SI_elemdictFindElemFromGrafNode", 0x080f_cbec);
pub const SI_DB_GRAF_TYPE2_CHAPTER: Routine = Routine::new("SI_dbGrafType2Chapter", 0x080f_caf0);
pub const SI_ATTDICT_EVAL_COND: Routine = Routine::new("SI_attdictEvalCond", 0x0811_f01c);
pub const PLT_SLEEP: Routine = Routine::new("plt:sleep", 0x080c_4468);

fn msg(m: &mut Mem, o: &mut dyn Outside, text: u32, mode: u32) {
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[text, mode]);
}

/// [soft 0x0852cde8 cusPickObject]: lets the user pick up to `maxn` objects of class `cls` for parameter `idx` and
/// adds each (an element record: the processor word, the class, the picked instance, `p6`, 0) to the parameter array
/// (`SI_cusParamAdd`). Returns 1; 0 for a null count/total/array pointer or when an event other than a pick (an event
/// of type 5 or 0 with code 0x65, any other type, or a failed array add) ends the loop, and also after the first
/// pick when the user answers 0x66 (`maxn` > 1 and the parameter already has an element: the pick ends with 1).
/// `mouse` is the text shown for the mouse buttons, `prompt` the message line; the picked graph node's element is
/// looked up in the dictionary, its chapter must be `cls`, a custom icon cannot be picked, and the attribute condition
/// `cond` must hold (each failure shows its message and waits a second); a picked model with `mode` 2 stands for its
/// root model. After an add the relation check `cusVerifyInstancePicked` (against `rel`, result flag `res`) shows
/// "Instance picked for parameter affecting geometry." with mode 2.
pub fn cus_pick_object(m: &mut Mem, o: &mut dyn Outside, rel_arg: u32, res: u16, mouse: u32, prompt: u32, cls: u32, p6: u32, cond: u32, mode: i32, maxn: i32, idx: u32, p11: u32, counts: u32, total: u32, arr: u32) -> u32 {
    let _ = p11;
    if counts == 0 || total == 0 || arr == 0 {
        return 0;
    }
    let mut ret: u32 = 1;
    let mut k: i32 = 0;
    if mouse != 0 {
        let second = if maxn > 1 { 0x086d_ec58 } else { 0x086d_ec61 };
        o.call(m, SI_STATUS_DISPLAY_MOUSE, &[mouse, second, 0x086d_ec45]);
    }
    if prompt != 0 {
        msg(m, o, prompt, 0);
    }
    // the stack: +0 element (5 words), +0x14 the element identity (class, id, -1, 0), +0x24 the dictionary outputs
    let loc = m.local(0x40);
    let src = loc;
    let ident = loc.wrapping_add(0x14);
    let outs = loc.wrapping_add(0x24);
    let pd = m.u32(SI_PROC_DATA);
    m.set_u32(src, m.u32(pd));
    m.set_u32(src.wrapping_add(4), cls);
    m.set_u32(src.wrapping_add(0xc), p6);
    m.set_u32(src.wrapping_add(0x10), 0);
    m.set_u32(ident, cls);
    m.set_u32(ident.wrapping_add(8), 0xffff_ffff);
    m.set_u32(ident.wrapping_add(0xc), 0);
    let mut ev: u32 = 0;
    // the loop of the original: `top` tests the count and the event, `pick` handles a selection
    'outer: loop {
        // top
        if k >= maxn {
            break;
        }
        ev = o.call(m, crate::custom_util::SI_GET_EVENT, &[]).eax;
        let t = m.u32(ev);
        if t != 5 && t != 0 {
            ret = 0;
            break;
        }
        let code = m.u16(ev.wrapping_add(6));
        if code == 0x65 {
            ret = 0;
            break;
        }
        if code == 0x66 && maxn > 1 && (m.u32(counts.wrapping_add(idx.wrapping_mul(4))) as i32) > 0 {
            break;
        }
        // pick (cea0)
        if m.u32(ev) != 5 {
            continue;
        }
        let node = o.call(m, crate::custom_util::SI_GL_SELECT, &[m.u32(ev.wrapping_add(0xc))]).eax;
        if node == 0 {
            continue;
        }
        let mut bad = true;
        let text: u32;
        if o.call(m, SI_ELEMDICT_FIND_ELEM_FROM_GRAFNODE, &[node, outs, ident.wrapping_add(4)]).eax == 0 {
            text = 0x086d_ec80;
        } else {
            let ch = o.call(m, SI_DB_GRAF_TYPE2_CHAPTER, &[m.u32(node)]).eax;
            if ch != cls {
                text = 0x086d_ecc0;
            } else if cls == m.u32(CID_MODELS_P) && m.u32(node.wrapping_add(4)) != 0 && m.u32(m.u32(node.wrapping_add(4)).wrapping_add(0x2ec)) == 0xd {
                text = 0x086d_ece0;
            } else if o.call(m, SI_ATTDICT_EVAL_COND, &[cond, ident]).eax as u16 != 1 {
                text = 0x086d_ed20;
            } else {
                bad = false;
                text = 0;
            }
        }
        if bad {
            msg(m, o, text, 3);
            o.call(m, PLT_SLEEP, &[1]);
            if prompt != 0 {
                msg(m, o, prompt, 0);
            }
            continue 'outer;
        }
        o.call(m, crate::custom_util::SI_FLASH_OBJECT, &[node, 5]);
        k += 1;
        let inst = if cls == m.u32(CID_MODELS_P) && mode == 2 {
            let root = o.call(m, crate::custom_util::SI_MODEL_ROOT, &[m.u32(node.wrapping_add(4))]).eax;
            let dict = elem_dicts(m);
            o.call(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, cls, root]).eax
        } else {
            m.u32(ident.wrapping_add(4))
        };
        m.set_u32(src.wrapping_add(8), inst);
        if si_cus_param_add(m, src, idx as i32, counts, total, arr) as u16 == 0 {
            ret = 0;
            break;
        }
        let dict = elem_dicts(m);
        if cus_verify_instance_picked(m, o, SI_RELDICT_P, dict, rel_arg, cls, inst, res as i16 as i32 as u32) as u16 == 1 {
            msg(m, o, 0x086d_ed60, 2);
        }
    }
    let _ = ev;
    m.release(0x40);
    o.call(m, SI_STATUS_DISPLAY_MOUSE, &[0x086d_ed94, 0x086d_ed93, 0x086d_ed92]);
    msg(m, o, 0x086d_ed95, 0);
    ret
}

const SI_RELDICT_GET_NB_DISTINCT_RES: Routine = Routine::new("SI_reldictGetNbDistinctRes", 0x0813_ed60);
const SI_RELDICT_GET_NB_DISTINCT_ARGS: Routine = Routine::new("SI_reldictGetNbDistinctArgs", 0x0813_eeac);
const SI_RELDICT_GET_NB_ARG_INSTANCES: Routine = Routine::new("SI_reldictGetNbArgInstances", 0x0813_f17c);
const SI_RELDICT_GET_NB_RES_INSTANCES: Routine = Routine::new("SI_reldictGetNbResInstances", 0x0813_f04c);
const SI_RELDICT_GET_NEXT_DISTINCT_ARG: Routine = Routine::new("SI_reldictGetNextDistinctArg", 0x0813_eff4);
const SI_RELDICT_GET_NEXT_DISTINCT_RES: Routine = Routine::new("SI_reldictGetNextDistinctRes", 0x0813_efac);
const SI_RELDICT_GET_ARG_COND: Routine = Routine::new("SI_reldictGetArgCond", 0x0813_f654);
const SI_RELDICT_GET_RES_COND: Routine = Routine::new("SI_reldictGetResCond", 0x0813_f550);
const SI_RELDICT_GET_ARG_PICK_MESSAGE: Routine = Routine::new("SI_reldictGetArgPickMessage", 0x0813_f404);
const SI_RELDICT_GET_RES_PICK_MESSAGE: Routine = Routine::new("SI_reldictGetResPickMessage", 0x0813_f2f8);
const SI_RELDICT_GET_MAX_FANOUT: Routine = Routine::new("SI_reldictGetMaxFanout", 0x0813_f9cc);
use crate::custom_init::{SI_RELDICT_GET_CHAP, SI_RELDICT_GET_GRP, SI_RELDICT_GET_NAME, SI_RELDICT_GET_SCOPE, SI_RELDICT_IS_NEW_SRC};
use crate::custom_dyn::{si_dynamic_link_add_symbol, si_dynamic_link_get_handle, si_dynamic_link_load, si_dynamic_link_set_handle, si_dynamic_link_set_name, SI_ERROR};

/// The relation dictionary of the program (`SI_RD`).
const SI_RD: u32 = SI_RELDICT_P;

/// [soft 0x0852c944 cusPickParameters]: asks the user for every parameter of operator `id` that has no element yet
/// (`res` = 1: the results, else the arguments; the effect's record at +0x104 holds the arrays: arguments at
/// +4 (unused word), +8 (counts), +0xc (total), +0x10 (array), results at +0x14, +0x18, +0x1c, +0x20). For each distinct
/// parameter whose count is 0 (all, for arguments): when `name` names the parameter (the icon parameter) the effect's
/// own model is added (`SI_error` "Custom Icon can't be result" and 0 for a result); otherwise a result that is a new
/// source is skipped and, in mode 0, `cusPickObject` asks (with the relation's own pick message, or `cusGenPickMsg`'s).
/// Then, in mode 0 or when the name matches (mode 1 skips the others), the elements already picked for the first
/// instance are copied for each further instance of the parameter. 1 when all went well, 0 for a null effect or a
/// failed pick or add. Deviation: in mode 2 with several instances the original copies with a class left uninitialised
/// on the stack when no earlier branch set it; the port uses 0.
pub fn cus_pick_parameters(m: &mut Mem, o: &mut dyn Outside, eff: u32, name: u32, id: u32, res: u16, mode: u16) -> u32 {
    if eff == 0 {
        return 0;
    }
    let res_i = res as i16 as i32 as u32;
    let is_res = res == 1;
    let n = if is_res {
        o.call(m, SI_RELDICT_GET_NB_DISTINCT_RES, &[SI_RD, id]).eax
    } else {
        o.call(m, SI_RELDICT_GET_NB_DISTINCT_ARGS, &[SI_RD, id]).eax
    } as i32;
    let f_nb = if is_res { SI_RELDICT_GET_NB_RES_INSTANCES } else { SI_RELDICT_GET_NB_ARG_INSTANCES };
    let f_next = if is_res { SI_RELDICT_GET_NEXT_DISTINCT_RES } else { SI_RELDICT_GET_NEXT_DISTINCT_ARG };
    let f_cond = if is_res { SI_RELDICT_GET_RES_COND } else { SI_RELDICT_GET_ARG_COND };
    let f_msg = if is_res { SI_RELDICT_GET_RES_PICK_MESSAGE } else { SI_RELDICT_GET_ARG_PICK_MESSAGE };
    let rec = m.u32(eff.wrapping_add(0x104));
    let (arrp, totalp, counts, x) = if is_res {
        (rec.wrapping_add(0x20), rec.wrapping_add(0x1c), m.u32(rec.wrapping_add(0x18)), m.u32(rec.wrapping_add(0x14)))
    } else {
        (rec.wrapping_add(0x10), rec.wrapping_add(0xc), m.u32(rec.wrapping_add(8)), m.u32(rec.wrapping_add(4)))
    };
    let _ = x;
    let mut id = id;
    let mut i: i32 = 0;
    let mut pos: u32 = 0;
    let mut chap: u32 = 0;
    let elem = m.local(0x14);
    let mut result = 1;
    'all: while i < n {
        let cnt = m.u32(counts.wrapping_add(pos.wrapping_mul(4)));
        if res == 0 || cnt == 0 {
            let mut do_pick_tail = true;
            let mut named = false;
            if name != 0 {
                let nm = o.call(m, SI_RELDICT_GET_NAME, &[SI_RD, id, res_i]).eax;
                if clib::strcmp(m, nm, name) == 0 {
                    named = true;
                    if is_res {
                        o.call(m, SI_ERROR, &[4, 0x086d_ec1d]);
                        result = 0;
                        break 'all;
                    }
                    let pd = m.u32(SI_PROC_DATA);
                    m.set_u32(elem, m.u32(pd));
                    let models = m.u32(CID_MODELS_P);
                    m.set_u32(elem.wrapping_add(4), models);
                    chap = models & 0xffff;
                    let dict = elem_dicts(m);
                    let inst = o.call(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, models, m.u32(eff.wrapping_add(0xc))]).eax;
                    m.set_u32(elem.wrapping_add(8), inst);
                    let g = o.call(m, SI_RELDICT_GET_GRP, &[SI_RD, id, res_i]).eax;
                    m.set_u32(elem.wrapping_add(0xc), g);
                    m.set_u32(elem.wrapping_add(0x10), 0);
                    if si_cus_param_add(m, elem, pos as i32, counts, totalp, arrp) as u16 == 0 {
                        result = 0;
                        break 'all;
                    }
                }
            }
            if !named {
                if is_res && o.call(m, SI_RELDICT_IS_NEW_SRC, &[SI_RD, id]).eax as u16 == 1 {
                    do_pick_tail = false;
                }
                if do_pick_tail && mode == 0 {
                    chap = o.call(m, SI_RELDICT_GET_CHAP, &[SI_RD, id, res_i]).eax & 0xffff;
                    let cond = o.call(m, f_cond, &[SI_RD, id]).eax;
                    let mut msg_t = o.call(m, f_msg, &[SI_RD, id]).eax;
                    if msg_t == 0 {
                        let nm = o.call(m, SI_RELDICT_GET_NAME, &[SI_RD, id, res_i]).eax;
                        msg_t = cus_gen_pick_msg(m, o, chap as i16 as i32 as u32, cond, nm, res_i, i as u32);
                    }
                    let fan = o.call(m, SI_RELDICT_GET_MAX_FANOUT, &[SI_RD, id, res_i]).eax as u16 as i16 as i32;
                    let scope = o.call(m, SI_RELDICT_GET_SCOPE, &[SI_RD, id, res_i]).eax as u16 as i16 as i32;
                    let grp = o.call(m, SI_RELDICT_OP_PARAM_INSTANCE_GET_GRP, &[SI_RD, id, 0, res_i]).eax;
                    if cus_pick_object(m, o, id, res, 0x086d_ec39, msg_t, chap as i16 as i32 as u32, grp, cond, scope, fan, pos, x, counts, totalp, arrp) as u16 == 0 {
                        result = 0;
                        break 'all;
                    }
                }
            }
            if do_pick_tail {
                // cc73
                let mut replicate = true;
                if mode == 1 {
                    if name == 0 {
                        replicate = false;
                    } else {
                        let nm = o.call(m, SI_RELDICT_GET_NAME, &[SI_RD, id, res_i]).eax;
                        if clib::strcmp(m, nm, name) != 0 {
                            replicate = false;
                        }
                    }
                }
                if replicate {
                    let mut s: u32 = 0;
                    let mut j: u32 = 0;
                    while (j as i32) < pos as i32 {
                        let c = m.u32(counts.wrapping_add(j.wrapping_mul(4))) as i32;
                        s = s.wrapping_add(if c < 1 { 1 } else { c as u32 });
                        j += 1;
                    }
                    let mut k: i32 = 1;
                    while k < o.call(m, f_nb, &[SI_RD, id]).eax as i32 {
                        let mut j: i32 = 0;
                        let mut off = s.wrapping_mul(0x14);
                        while j < m.u32(counts.wrapping_add(pos.wrapping_mul(4))) as i32 {
                            let pd = m.u32(SI_PROC_DATA);
                            m.set_u32(elem, m.u32(pd));
                            m.set_u32(elem.wrapping_add(4), chap as i16 as i32 as u32);
                            let arr = m.u32(arrp);
                            m.set_u32(elem.wrapping_add(8), m.u32(arr.wrapping_add(off).wrapping_add(8)));
                            let g = o.call(m, SI_RELDICT_OP_PARAM_INSTANCE_GET_GRP, &[SI_RD, id, k as u32, res_i]).eax;
                            m.set_u32(elem.wrapping_add(0xc), g);
                            let arr = m.u32(arrp);
                            m.set_u32(elem.wrapping_add(0x10), m.u32(arr.wrapping_add(off).wrapping_add(0x10)));
                            if si_cus_param_add(m, elem, (pos as i32).wrapping_add(k), counts, totalp, arrp) as u16 == 0 {
                                result = 0;
                                break 'all;
                            }
                            off = off.wrapping_add(0x14);
                            j += 1;
                        }
                        k += 1;
                    }
                }
            }
        }
        // cda3
        i += 1;
        pos = pos.wrapping_add(o.call(m, f_nb, &[SI_RD, id]).eax);
        id = o.call(m, f_next, &[SI_RD, id]).eax;
    }
    m.release(0x14);
    result
}

/// [soft 0x0852c734 cusSetResultsFromArgs]: for operator `id` and every distinct result whose name equals the name of one of
/// the operator's distinct arguments, adds to the results arrays (`rcounts`, `rtotal`, `rarr`; the `rtotalp`/`rarrp`
/// cells) one copy of every argument element (`args` = array of 20-byte elements, `acounts` = their per-parameter
/// counts) for each instance of the result, the element's group word (+0xc) replaced by the result's own group.
/// 1, or 0 for a null array or cell, a negative id or a failed add.
pub fn cus_set_results_from_args(m: &mut Mem, o: &mut dyn Outside, id: i32, acounts: u32, x: u32, rcounts: u32, args: u32, rtotalp: u32, rarrp: u32) -> u32 {
    let _ = x;
    if args == 0 || rarrp == 0 || id < 0 {
        return 0;
    }
    let idu = id as u32;
    let n_args = o.call(m, SI_RELDICT_GET_NB_DISTINCT_ARGS, &[SI_RD, idu]).eax as i32;
    let n_res = o.call(m, SI_RELDICT_GET_NB_DISTINCT_RES, &[SI_RD, idu]).eax as i32;
    let mut r_idx: i32 = 0;
    let mut r_base: i32 = 0;
    let mut r_id = idu;
    let mut nb_res = o.call(m, SI_RELDICT_GET_NB_RES_INSTANCES, &[SI_RD, idu]).eax as i32;
    let tmp = m.local(0x14);
    let mut ret = 1;
    'outer: while r_idx < n_res {
        let mut found = false;
        let mut a_pos: i32 = 0;
        let mut a_id = idu;
        let mut k = 0;
        while k < n_args {
            let s2 = o.call(m, SI_RELDICT_GET_NAME, &[SI_RD, a_id, 0]).eax;
            let s1 = o.call(m, SI_RELDICT_GET_NAME, &[SI_RD, r_id, 1]).eax;
            if clib::strcmp(m, s1, s2) == 0 {
                found = true;
                break;
            }
            a_pos = a_pos.wrapping_add(o.call(m, SI_RELDICT_GET_NB_ARG_INSTANCES, &[SI_RD, a_id]).eax as i32);
            a_id = o.call(m, SI_RELDICT_GET_NEXT_DISTINCT_ARG, &[SI_RD, a_id]).eax;
            k += 1;
        }
        if found {
            let mut skip: i32 = 0;
            for j in 0..a_pos {
                skip = skip.wrapping_add(m.u32(acounts.wrapping_add((j as u32).wrapping_mul(4))) as i32);
            }
            let mut inst = 0;
            while inst < nb_res {
                let cnt = m.u32(acounts.wrapping_add((a_pos as u32).wrapping_mul(4))) as i32;
                if cnt > 0 {
                    let mut src = args.wrapping_add((skip as u32).wrapping_mul(20));
                    let mut e = 0;
                    while e < cnt {
                        m.set_u32(tmp, m.u32(src));
                        m.set_u32(tmp.wrapping_add(4), m.u32(src.wrapping_add(4)));
                        m.set_u32(tmp.wrapping_add(8), m.u32(src.wrapping_add(8)));
                        let g = o.call(m, SI_RELDICT_OP_PARAM_INSTANCE_GET_GRP, &[SI_RD, r_id, inst as u32, 1]).eax;
                        m.set_u32(tmp.wrapping_add(0xc), g);
                        m.set_u32(tmp.wrapping_add(0x10), m.u32(src.wrapping_add(0x10)));
                        if si_cus_param_add(m, tmp, r_base.wrapping_add(inst), rcounts, rtotalp, rarrp) as u16 == 0 {
                            ret = 0;
                            break 'outer;
                        }
                        src = src.wrapping_add(0x14);
                        e += 1;
                    }
                }
                inst += 1;
            }
        }
        r_idx += 1;
        r_base = r_base.wrapping_add(nb_res);
        r_id = o.call(m, SI_RELDICT_GET_NEXT_DISTINCT_RES, &[SI_RD, r_id]).eax;
        nb_res = o.call(m, SI_RELDICT_GET_NB_RES_INSTANCES, &[SI_RD, r_id]).eax as i32;
    }
    m.release(0x14);
    ret
}

/// [soft 0x0852c2e8 setArgsAndResults]: for the effect `eff` and operator `id` picks the missing arguments, copies the
/// matching arguments to the results and picks the missing results (`mode` 1: only the icon parameter, no copy). 1 when
/// all went well (also for id -1), 0 for a null effect or a failed step. The original's tail for id -1 (adding the
/// effect's model as the last argument) cannot be reached after the early return and is not ported.
pub fn set_args_and_results(m: &mut Mem, o: &mut dyn Outside, eff: u32, id: i32, mode: u16) -> u32 {
    if eff == 0 {
        return 0;
    }
    if id == -1 {
        return 1;
    }
    let rec = m.u32(eff.wrapping_add(0x104));
    let name = si_cus_get_icon_name(m, eff);
    let idu = id as u32;
    if mode == 1 {
        if (m.u32(rec.wrapping_add(4)) as i32) > 0 && cus_pick_parameters(m, o, eff, name, idu, 0, 1) as u16 != 1 {
            return 0;
        }
        if (m.u32(rec.wrapping_add(0x14)) as i32) < 1 {
            return 1;
        }
        return (cus_pick_parameters(m, o, eff, name, idu, 1, 1) as u16 == 1) as u32;
    }
    if (m.u32(rec.wrapping_add(4)) as i32) > 0 {
        if cus_pick_parameters(m, o, eff, name, idu, 0, mode) as u16 != 1 {
            return 0;
        }
        if (m.u32(rec.wrapping_add(4)) as i32) > 0 {
            let r = cus_set_results_from_args(
                m, o, id, m.u32(rec.wrapping_add(8)), m.u32(rec.wrapping_add(0x14)), m.u32(rec.wrapping_add(0x18)),
                m.u32(rec.wrapping_add(0x10)), rec.wrapping_add(0x1c), rec.wrapping_add(0x20),
            );
            if r as u16 != 1 {
                return 0;
            }
        }
    }
    if (m.u32(rec.wrapping_add(0x14)) as i32) < 1 {
        return 1;
    }
    (cus_pick_parameters(m, o, eff, name, idu, 1, mode) as u16 == 1) as u32
}

const SI_ERROR_SET_FILTER: Routine = Routine::new("SI_errorSetFilter", 0x0814_1954);

/// [soft 0x0852b228 dynamicSecLinkLoad]: gives the secondary links of the dso data (`data`+0x40, 12-byte links) the
/// library handle of the main link (`data`+0), one per dialog item before the first item of kind 100 (the dialog's items
/// are 60 bytes at +0xc, their count at +8; without such an item the count itself is used, with it the count less one).
/// The answer of the last `SI_dynamicLinkSetHandle` (the loop stops at the first nonzero one), 0 when there is nothing
/// to do.
pub fn dynamic_sec_link_load(m: &mut Mem, data: u32, dialog: u32) -> i32 {
    if data == 0 || m.u32(data.wrapping_add(0x40)) == 0 || dialog == 0 {
        return 0;
    }
    let items = m.u32(dialog.wrapping_add(0xc));
    if items == 0 {
        return 0;
    }
    let mut n = m.u32(dialog.wrapping_add(8)) as i32;
    if n > 0 {
        let mut i: i32 = 0;
        let mut kind = m.u16(items) as i16;
        let mut found = true;
        while kind != 100 {
            i += 1;
            if n <= i {
                found = false;
                break;
            }
            kind = m.u16(items.wrapping_add((i as u32).wrapping_mul(60))) as i16;
        }
        if found {
            n -= 1;
        }
    }
    if n <= 0 {
        return 0;
    }
    let handle = si_dynamic_link_get_handle(m, m.u32(data));
    let links = m.u32(data.wrapping_add(0x40));
    let mut i: i32 = 0;
    let mut off: u32 = 0;
    loop {
        let r = si_dynamic_link_set_handle(m, handle, links.wrapping_add(off));
        off = off.wrapping_add(12);
        i += 1;
        if n <= i || r != 0 {
            return r;
        }
    }
}

/// [soft 0x0852c51c si_cusEffectLoadDso]: loads the dso of the effect `eff` (record at +0x104, whose first word is the
/// link; directory text at +0x10c). A name without a slash is tried as "<dir>/<name>" first (with the error filter
/// raised to 5 around that load) and as the bare name after it; the dialog's secondary links get the handle at the end.
/// 1 when loaded, 0 for a missing part, out of memory or a failed load. Deviation: none known; the dlopen is the
/// outside hook of `SI_dynalinkLoad`.
pub fn si_cus_effect_load_dso(m: &mut Mem, o: &mut dyn Outside, eff: u32) -> u32 {
    if eff == 0 {
        return 0;
    }
    let dir = m.u32(eff.wrapping_add(0x10c));
    if dir == 0 {
        return 0;
    }
    let rec = m.u32(eff.wrapping_add(0x104));
    if rec == 0 {
        return 0;
    }
    let link = m.u32(rec);
    if link == 0 {
        return 0;
    }
    let name = m.u32(link);
    let mut loaded = false;
    if clib::strchr(m, name, 0x2fu8) == 0 {
        let len = clib::strlen(m, name);
        let bare = m.si_malloc(len.wrapping_add(1));
        if bare == 0 {
            return 0;
        }
        clib::strcpy(m, bare, name);
        let full_len = clib::strlen(m, dir).wrapping_add(len).wrapping_add(2);
        let full = m.si_malloc(full_len);
        if full == 0 {
            return 0;
        }
        crate::cfmt::sprintf(m, full, 0x086d_ec16, &[dir, 0x2f, bare]);
        if si_dynamic_link_set_name(m, full, link) as u16 != 1 {
            m.free(full);
            return 0;
        }
        m.free(full);
        let old = o.call(m, SI_ERROR_SET_FILTER, &[5]).eax;
        if si_dynamic_link_load(m, o, link) == 0 {
            loaded = true;
        }
        o.call(m, SI_ERROR_SET_FILTER, &[old]);
        if !loaded && si_dynamic_link_set_name(m, bare, link) as u16 != 1 {
            m.free(bare);
            return 0;
        }
        m.free(bare);
    }
    if !loaded && si_dynamic_link_load(m, o, link) != 0 {
        return 0;
    }
    dynamic_sec_link_load(m, rec, eff.wrapping_add(0x10));
    1
}

/// The token table of the `_DYNAMIC_LINK` section: 16 records of 0x108 bytes (+0 symbol type, +4 keyword, +0x104 flag
/// "read").
const SI_CUS_LOAD_EFFECT_EXTENSIONS: Routine = Routine::new("si_cusLoadEffectExtensions", 0x0853_2eb4);
const SYMTOKEN_DYN: u32 = 0x086d_d380;
/// The one-record table of the secondary `_DYNAMIC_LINK` section (the dialog setup function).
const SYMTOKEN_SEC: u32 = 0x086d_e580;

fn local_line(m: &mut Mem) -> u32 {
    m.local(0x204)
}

/// [soft 0x0852a6d4 si_cusParseDynamicLink]: reads the lines of a `_DYNAMIC_LINK` section of a `.cus` file up to
/// `_END`. `LINKNAME <lib>` (once; a name without `/`, `.so` appended when it has no dot, `so` when it ends in a
/// dot) names the link's library; a keyword of the token table (`CUSTOM_INIT_FUNCTION` ... ) adds the following word
/// as a symbol of the keyword's type, once (else `SI_error` 0x5d, or 0x5e for a missing word, whose second argument is
/// the delimiter text); `#` lines and unknown words are skipped. At `_END` it answers 1 unless both a custom and
/// a plain function of the same kind were declared (types 1/9, 2/10, 3/11: message "Error parsing .cus file:
/// Conflicting DYNAMIC_LINK symbols declared", mode 3, answer 0). 0 also for a null argument, an end of file (error 4),
/// a second or missing or path-like LINKNAME (error 4) or a failed add or name.
pub fn si_cus_parse_dynamic_link(m: &mut Mem, o: &mut dyn Outside, fp: u32, link: u32) -> u32 {
    if fp == 0 || link == 0 {
        return 0;
    }
    let buf = local_line(m);
    let r = (|| -> u32 {
        for k in 0..16u32 {
            m.set_u16(SYMTOKEN_DYN + k * 0x108 + 0x104, 0);
        }
        let mut seen_name = false;
        let mut saved = 0u32;
        loop {
            if si_cus_line_read(m, o, fp, buf, 0x200) == 0 {
                o.call(m, SI_ERROR, &[4, 0x086d_e400]);
                return 0;
            }
            m.set_u8(buf + 0x1ff, 0);
            if clib::strncmp(m, buf, 0x086d_e448, 4) == 0 {
                let s = |m: &Mem, k: u32| crate::custom_parse::symbol_already_read(m, 0x10, SYMTOKEN_DYN, k) as u16;
                let ok = (s(m, 1) == 0 || s(m, 9) == 0) && (s(m, 2) == 0 || s(m, 10) == 0) && (s(m, 3) == 0 || s(m, 0xb) == 0);
                if ok {
                    return 1;
                }
                msg(m, o, 0x086d_e520, 3);
                return 0;
            }
            let tok = clib::strtok(m, buf, 0x086d_e44d, &mut saved);
            if tok == 0 || m.u8(tok) == b'#' {
                continue;
            }
            if clib::strcmp(m, tok, 0x086d_e453) != 0 {
                for k in 0..16u32 {
                    let rec = SYMTOKEN_DYN + k * 0x108;
                    if clib::strcmp(m, tok, rec + 4) == 0 {
                        if m.u16(rec + 0x104) == 0 {
                            let w = clib::strtok(m, 0, 0x086d_e511, &mut saved);
                            if w != 0 {
                                if si_dynamic_link_add_symbol(m, link, w, m.u32(rec)) == 0 {
                                    return 0;
                                }
                                m.set_u16(rec + 0x104, 1);
                                break;
                            }
                            o.call(m, SI_ERROR, &[0x5e, 0x086d_e511]);
                        } else {
                            o.call(m, SI_ERROR, &[0x5d, tok]);
                        }
                        break;
                    }
                }
                continue;
            }
            if seen_name {
                o.call(m, SI_ERROR, &[4, 0x086d_e460]);
                return 0;
            }
            seen_name = true;
            let name = clib::strtok(m, 0, 0x086d_e489, &mut saved);
            if name == 0 {
                o.call(m, SI_ERROR, &[4, 0x086d_e4a0]);
                return 0;
            }
            if clib::strchr(m, name, 0x2f) != 0 {
                o.call(m, SI_ERROR, &[4, 0x086d_e4e0]);
                return 0;
            }
            let dot = clib::strchr(m, name, 0x2e);
            if dot == 0 {
                clib::strcat(m, name, 0x086d_e509);
            } else if clib::strlen(m, dot) == 1 {
                clib::strcat(m, name, 0x086d_e50e);
            }
            if si_dynamic_link_set_name(m, name, link) as u16 == 0 {
                return 0;
            }
        }
    })();
    m.release(0x204);
    r
}

/// [soft 0x0852aa58 si_cusReadSecondaryDialog]: reads the lines of the secondary dialog section of a `.cus` file:
/// each word (not `#`) names an extension file (no `/`; `.cusext` appended when there is no dot or it ends in a dot)
/// that `si_cusLoadEffectExtensions` loads for the effect; `_END` answers 1. 0 for a null file, an end of file, a
/// path-like name or a failed extension load (a negative answer).
pub fn si_cus_read_secondary_dialog(m: &mut Mem, o: &mut dyn Outside, fp: u32, eff: u32) -> u32 {
    if fp == 0 {
        return 0;
    }
    let buf = local_line(m);
    let r = (|| -> u32 {
        let mut idx: i32 = 0;
        let mut saved = 0u32;
        while si_cus_line_read(m, o, fp, buf, 0x200) != 0 {
            m.set_u8(buf + 0x1ff, 0);
            if clib::strncmp(m, buf, 0x086d_e563, 4) == 0 {
                return 1;
            }
            let name = clib::strtok(m, buf, 0x086d_e568, &mut saved);
            if name != 0 && m.u8(name) != b'#' {
                if clib::strchr(m, name, 0x2f) != 0 {
                    return 0;
                }
                let dot = clib::strchr(m, name, 0x2e);
                if dot == 0 {
                    clib::strcat(m, name, 0x086d_e56e);
                } else if clib::strlen(m, dot) == 1 {
                    clib::strcat(m, name, 0x086d_e577);
                }
                if (o.call(m, SI_CUS_LOAD_EFFECT_EXTENSIONS, &[eff, idx as u32, name]).eax as i32) < 0 {
                    return 0;
                }
                idx += 1;
            }
        }
        0
    })();
    m.release(0x204);
    r
}

/// [soft 0x0852aba0 si_cusParseSecondaryDynamicLink]: reads the lines of the secondary `_DYNAMIC_LINK` section
/// up to `_END` (answer 1): `DIALOG_SETUP_FUNCTION <name>` adds the name as a symbol of type 4 to `link`, once (else
/// `SI_error` 0x5d); other words and `#` lines are skipped. 0 for a null argument, an end of file or a failed add.
pub fn si_cus_parse_secondary_dynamic_link(m: &mut Mem, o: &mut dyn Outside, fp: u32, link: u32) -> u32 {
    if fp == 0 || link == 0 {
        return 0;
    }
    let buf = local_line(m);
    let r = (|| -> u32 {
        m.set_u16(SYMTOKEN_SEC + 0x104, 0);
        let mut saved = 0u32;
        while si_cus_line_read(m, o, fp, buf, 0x200) != 0 {
            m.set_u8(buf + 0x1ff, 0);
            if clib::strncmp(m, buf, 0x086d_e688, 4) == 0 {
                return 1;
            }
            let tok = clib::strtok(m, buf, 0x086d_e68d, &mut saved);
            if tok != 0 && m.u8(tok) != b'#' && clib::strcmp(m, tok, SYMTOKEN_SEC + 4) == 0 {
                if m.u16(SYMTOKEN_SEC + 0x104) == 0 {
                    let w = clib::strtok(m, 0, 0x086d_e693, &mut saved);
                    if w != 0 {
                        if si_dynamic_link_add_symbol(m, link, w, m.u32(SYMTOKEN_SEC)) == 0 {
                            return 0;
                        }
                        m.set_u16(SYMTOKEN_SEC + 0x104, 1);
                    }
                } else {
                    o.call(m, SI_ERROR, &[0x5d, tok]);
                }
            }
        }
        0
    })();
    m.release(0x204);
    r
}

const SI_REFRESH_SYSTEM: Routine = Routine::new("SI_refreshSystem", 0x0832_3d44);
const SI_UPDATE_DEFORMATION: Routine = Routine::new("SI_updateDeformation", 0x0841_ea30);
const SI_SCENE_STEP_FRAME: Routine = Routine::new("SI_sceneStepFrame", 0x0820_f2d8);
const SI_CUS_DSO_EFFECT_CREATE_RELATIONS: Routine = Routine::new("SI_cusDsoEffectCreateRelations", 0x0854_9d58);
const SI_REL_TOPO_SORT_INIT: Routine = Routine::new("SI_relTopoSortInit", 0x0813_ffbc);
const SI_ELEMDICT_TOPO_SORT: Routine = Routine::new("SI_elemdictTopoSort", 0x0814_0828);

/// Takes the effect's model node out of the graphic list (`SI_findGlData(list, model, 2)`, then the delete).
fn drop_graf_node(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let gl = graf_list(m);
    let node = o.call(m, SI_FIND_GL_DATA, &[gl, model, 2]).eax;
    let gl = graf_list(m);
    o.call(m, SI_GRAFLIST_DELETE_GRAFNODE, &[gl, node]);
}

fn refresh_both(m: &mut Mem, o: &mut dyn Outside) {
    o.call(m, SI_REFRESH_SYSTEM, &[0, 0, 0x37]);
    o.call(m, SI_REFRESH_SYSTEM, &[0, 0, 0x17]);
}

/// [soft 0x0852ad40 cusInstantiatePersistentDsoEffect]: makes the effect `eff` (it needs a model and a dso record with a
/// link) a persistent one of the status: the record gets the update function `f5` (+0x2c) and `f7` (+0x28), the effect
/// is copied behind the last of the motion chain (si_cusAddMotionEffect, a hook here) and the source record is emptied (its
/// arrays freed, every field but the link cleared). A failed copy takes the model's graphic node out and shows
/// "Error instancing persistent effect" (3); else the new effect is made current and the init function `f4` (context
/// form, context kind 9) or `f6` (argument form) is called; the results 0x5f (with a message) and 0x7c take the node
/// out as well. Then the arguments and results are checked against `id` (`id` -1 skips it; with `f6` the result
/// counts are set to 1), the relations are made and the dictionary sorted; the checks and a cycle give the messages
/// at 0x086de720 and 0x086de760 and the node is taken out. Success redraws (unless `quiet`), 1 when the init result
/// was 0 (-1 else). The result of no init function at all is an uninitialised stack word in the original, taken 0.
pub fn cus_instantiate_persistent_dso_effect(
    m: &mut Mem,
    o: &mut dyn Outside,
    status: u32,
    eff: u32,
    quiet: u16,
    f4: u32,
    f5: u32,
    f6: u32,
    f7: u32,
    id: i32,
) -> i32 {
    if eff == 0 {
        return -1;
    }
    let rec = m.u32(eff.wrapping_add(0x104));
    if rec == 0 || m.u32(rec) == 0 || m.u32(eff.wrapping_add(0xc)) == 0 {
        return -1;
    }
    m.set_u32(rec.wrapping_add(0x2c), f5);
    m.set_u32(rec.wrapping_add(0x28), f7);
    let new = o.call(m, crate::custom_io::SI_CUS_ADD_MOTION_EFFECT, &[status, eff, 0xffff_ffff]).eax;
    for off in [0x10u32, 0x20, 8, 0x18] {
        let p = m.u32(rec.wrapping_add(off));
        if p != 0 {
            m.free(p);
            m.set_u32(rec.wrapping_add(off), 0);
        }
    }
    for off in [0x1cu32, 0xc, 0x20, 0x10, 4, 8, 0x14, 0x18, 0x24, 0x2c, 0x28] {
        m.set_u32(rec.wrapping_add(off), 0);
    }
    if new == 0 {
        let model = m.u32(eff.wrapping_add(0xc));
        m.set_u32(model.wrapping_add(0x2ec), 0);
        drop_graf_node(m, o, model);
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_e6a0, 3]);
        refresh_both(m, o);
        return -1;
    }
    let nrec = m.u32(new.wrapping_add(0x104));
    if m.u16(nrec.wrapping_add(0x38)) == 0x73 {
        crate::custom_misc::si_cus_inc_nb_post_def_fx(m);
    }
    let mut init = 0u32;
    if f4 != 0 || f6 != 0 {
        o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[new]);
        if f4 == 0 {
            let args = [
                m.u32(new.wrapping_add(100)),
                m.u32(nrec.wrapping_add(0xc)).wrapping_sub(1),
                m.u32(nrec.wrapping_add(0x10)),
                m.u32(nrec.wrapping_add(0x1c)),
                m.u32(nrec.wrapping_add(0x20)),
                nrec.wrapping_add(0x24),
            ];
            init = o.call(m, Routine::at(f6), &args).eax;
        } else {
            let ctx = m.local(0x20);
            si_cus_context_init(m, ctx, new, 9, 0, 0xffff_ffff);
            init = o.call(m, Routine::at(f4), &[ctx]).eax;
            m.release(0x20);
        }
        o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[0]);
        if init == 0x5f {
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_e6e0, 3]);
        }
        if init == 0x5f || init == 0x7c {
            let model = m.u32(new.wrapping_add(0xc));
            drop_graf_node(m, o, model);
            refresh_both(m, o);
            return -1;
        }
    }
    let ok = id == -1
        || (cus_verify_params(m, o, m.u32(nrec.wrapping_add(0x14)) as i32, m.u32(nrec.wrapping_add(0x18)), m.u32(nrec.wrapping_add(0x20)), id as u32, 1) as u16 != 0
            && cus_verify_params(m, o, m.u32(nrec.wrapping_add(4)) as i32, m.u32(nrec.wrapping_add(8)), m.u32(nrec.wrapping_add(0x10)), id as u32, 0) as u16 != 0);
    let msg;
    if ok {
        if f6 != 0 {
            let n = m.u32(nrec.wrapping_add(0x14)) as i32;
            let counts = m.u32(nrec.wrapping_add(0x18));
            for i in 0..n.max(0) as u32 {
                m.set_u32(counts.wrapping_add(i * 4), 1);
            }
        }
        o.call(m, SI_CUS_DSO_EFFECT_CREATE_RELATIONS, &[new, id as u32]);
        let ed = elem_dicts(m);
        o.call(m, SI_REL_TOPO_SORT_INIT, &[ed, 1]);
        let ed = elem_dicts(m);
        let r = o.call(m, SI_ELEMDICT_TOPO_SORT, &[SI_RELDICT_P, ed, ed.wrapping_add(0xc), 1, 0]).eax as u16;
        if r != 0 {
            if quiet == 0 {
                o.call(m, SI_UPDATE_DEFORMATION, &[]);
                o.call(m, SI_SCENE_STEP_FRAME, &[0, 0, 0, 0]);
                refresh_both(m, o);
            }
            return if init == 0 { 1 } else { -1 };
        }
        msg = 0x086d_e760u32;
    } else {
        msg = 0x086d_e720u32;
    }
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[msg, 3]);
    let model = m.u32(new.wrapping_add(0xc));
    drop_graf_node(m, o, model);
    o.call(m, SI_UPDATE_DEFORMATION, &[]);
    o.call(m, SI_SCENE_STEP_FRAME, &[0, 0, 0, 0]);
    refresh_both(m, o);
    -1
}

const SI_CUS_CMD: Routine = Routine::new("SI_cusCmd", 0x0853_b284);
const SI_CUS_DIALOG_ITEM_CALLBACK_REMOVE_ALL: Routine = Routine::new("SI_cusDialogItemCallbackRemoveAll", 0x0852_77b4);
const SI_CURRENT_CUSTOM_EFFECT_SET: Routine = Routine::new("SI_currentCustomEffectSet", 0x0853_ac4c);
const SI_DYNAMIC_LINK_UNLOAD: Routine = Routine::new("SI_dynamicLinkUnload", 0x085e_6f8c);
const SET_ARGS_AND_RESULTS: Routine = Routine::new("setArgsAndResults", 0x0852_c2e8);
const CUS_INSTANTIATE_PERSISTENT_DSO_EFFECT: Routine = Routine::new("cusInstantiatePersistentDsoEffect", 0x0852_ad40);
const SI_FRAME_CURRENT_W: u32 = 0x0878_2040;

/// The record's four arrays freed (0x10, 0x20, 8, 0x18) and its counters, pointers and flags cleared.
fn free_dso_arrays(m: &mut Mem, rec: u32) {
    for off in [0x10u32, 0x20, 8, 0x18] {
        let p = m.u32(rec.wrapping_add(off));
        if p != 0 {
            m.free(p);
            m.set_u32(rec.wrapping_add(off), 0);
        }
    }
    for off in [0x1cu32, 0xc, 0x20, 0x10, 4, 8, 0x14, 0x18, 0x24, 0x2c, 0x28] {
        m.set_u32(rec.wrapping_add(off), 0);
    }
}

fn model_node_out(m: &mut Mem, o: &mut dyn Outside, eff: u32) {
    let model = m.u32(eff.wrapping_add(0xc));
    m.set_u32(model.wrapping_add(0x2ec), 0);
    drop_graf_node(m, o, model);
    refresh_both(m, o);
}

/// Calls a cleanup function: the context form (`f3c`, context kind 11) or the argument form (`f40`).
fn call_cleanup(m: &mut Mem, o: &mut dyn Outside, eff: u32, rec: u32, f3c: u32, f40: u32, ctx: u32) -> Option<u32> {
    if f3c == 0 {
        if f40 != 0 {
            let mut n = m.u32(rec.wrapping_add(0xc));
            if m.u32(eff.wrapping_add(0xc)) != 0 {
                n = n.wrapping_sub(1);
            }
            let args = [m.u32(eff.wrapping_add(100)), n, m.u32(rec.wrapping_add(0x10)), m.u32(rec.wrapping_add(0x1c)), m.u32(rec.wrapping_add(0x20)), rec.wrapping_add(0x24)];
            return Some(o.call(m, Routine::at(f40), &args).eax);
        }
        None
    } else {
        si_cus_context_init(m, ctx, eff, 0xb, 6, 0xffff_ffff);
        Some(o.call(m, Routine::at(f3c), &[ctx]).eax)
    }
}

/// [soft 0x0852b2e0 si_cusDsoEffect]: runs a shared-library effect once: the effect's command list, the library load and the
/// lookup of its functions (init, update, cleanup, each in the plain and the "custom" form, then dialog setup, save and
/// render), the argument/result arrays, an optional dialog, then either the persistent instance (effect with a model)
/// or init, update and cleanup in one go. 1 when it ran without an error result, else -1.
pub fn si_cus_dso_effect(m: &mut Mem, o: &mut dyn Outside, status: u32, eff: u32, frame: f32, quiet: i16, dialog: i16) -> i32 {
    if eff == 0 {
        return -1;
    }
    let rec = m.u32(eff.wrapping_add(0x104));
    if rec == 0 || m.u32(rec) == 0 {
        return -1;
    }
    let loc = m.local(0x50);
    let (p2c, p30, p34, p38, p3c, p40, p44, p48, p4c, s26, ctx) = (loc, loc + 4, loc + 8, loc + 12, loc + 16, loc + 20, loc + 24, loc + 28, loc + 32, loc + 36, loc + 40);
    for a in [p2c, p30, p34, p38, p3c, p40, p44, p48, p4c] {
        m.set_u32(a, 0);
    }
    let r = (|| -> i32 {
        let mut cmd = m.u32(eff.wrapping_add(0xf8));
        while cmd != 0 {
            if m.u16(cmd) != 1 {
                let r = o.call(m, SI_CUS_CMD, &[status, eff, cmd, frame.to_bits(), quiet as i32 as u32, s26, 0]).eax as i32;
                if r < 0 {
                    return -1;
                }
            }
            cmd = m.u32(cmd.wrapping_add(0x30));
        }
        let link = m.u32(rec);
        if o.call(m, SI_CUS_EFFECT_LOAD_DSO, &[eff]).eax as u16 != 1 {
            if m.u32(eff.wrapping_add(0xc)) != 0 {
                model_node_out(m, o, eff);
            }
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_e7a0, 3]);
            return -1;
        }
        // the lookups: plain form first, the custom form when it is missing (a message each time one cannot be resolved)
        let mut res = 0u32;
        'load: {
            for (k1, k2, s1, s2, m1, m2) in [
                (9u32, 1u32, p2c, p30, 0x086d_e7e0u32, 0x086d_e820u32),
                (10, 2, p34, p38, 0x086d_e880, 0x086d_e8c0),
                (0xb, 3, p3c, p40, 0x086d_e920, 0x086d_e960),
            ] {
                res = o.call(m, SI_CUS_RESOLVE_SYMBOL, &[link, k1, s1]).eax;
                if res != 0 {
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[m1, 3]);
                }
                if m.u32(s1) == 0 && res == 0 {
                    res = o.call(m, SI_CUS_RESOLVE_SYMBOL, &[link, k2, s2]).eax;
                    if res != 0 {
                        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[m2, 3]);
                    }
                }
                if res != 0 {
                    break 'load;
                }
            }
            for (k, s, msg) in [(4u32, p44, 0x086d_e9c0u32), (0xc, p48, 0x086d_ea20), (0xe, p4c, 0x086d_ea60)] {
                res = o.call(m, SI_CUS_RESOLVE_SYMBOL, &[link, k, s]).eax;
                if res != 0 {
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[msg, 3]);
                    break 'load;
                }
            }
            // everything is resolved
            let (f2c, f30, f34, f38, f3c, f40, f44) = (m.u32(p2c), m.u32(p30), m.u32(p34), m.u32(p38), m.u32(p3c), m.u32(p40), m.u32(p44));
            let rel = o.call(m, crate::custom_init::SI_RELDICT_GET_REL_INDEX_FROM_NAME, &[SI_RELDICT_P, status.wrapping_add(2), m.u32(eff.wrapping_add(4))]).eax;
            let ok = o.call(m, SI_CUS_DSO_ALLOC_OCCURRENCE_ARRAYS, &[eff, rel, rec.wrapping_add(4), rec.wrapping_add(0x14), rec.wrapping_add(8), rec.wrapping_add(0x18)]).eax as u16 == 1
                && o.call(m, SI_CUS_DSO_ALLOC_ARGS_RES, &[eff, rel, rec.wrapping_add(0xc), rec.wrapping_add(0x1c), rec.wrapping_add(0x10), rec.wrapping_add(0x20)]).eax as u16 == 1;
            if !ok {
                if m.u32(eff.wrapping_add(0xc)) != 0 {
                    model_node_out(m, o, eff);
                }
                free_dso_arrays(m, rec);
                return -1;
            }
            m.set_u32(rec.wrapping_add(0x24), 0);
            if dialog != 0 {
                if m.u32(eff.wrapping_add(0x14)) == 0 {
                    if f44 != 0 {
                        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_eaa0, 2]);
                    }
                } else {
                    let dlg = m.u32(eff.wrapping_add(0x14));
                    if m.u32(rec.wrapping_add(0x34)) == 0 && o.call(m, SET_ARGS_AND_RESULTS, &[eff, rel, 0]).eax as u16 == 0 {
                        res = 0x5f;
                    }
                    let fc = m.u32(SI_FRAME_CURRENT_W);
                    o.call(m, SI_CUS_DIALOG_SET, &[dlg, eff.wrapping_add(0x20), fc]);
                    if res == 0 && f44 != 0 {
                        o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[eff]);
                        si_cus_context_init(m, ctx, eff, 4, 9, 0xffff_ffff);
                        res = o.call(m, Routine::at(f44), &[ctx]).eax;
                        if res == 0x5f || res == 0x7c {
                            if res == 0x5f {
                                o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_eae0, 3]);
                            }
                            let mut i = 1i32;
                            if (m.u16(m.u32(eff.wrapping_add(0x14)).wrapping_add(0xc)) as i16) > 0 {
                                loop {
                                    o.call(m, SI_CUS_DIALOG_ITEM_CALLBACK_REMOVE_ALL, &[eff.wrapping_add(0x10), i as u32]);
                                    i += 1;
                                    if i > m.u16(m.u32(eff.wrapping_add(0x14)).wrapping_add(0xc)) as i16 as i32 {
                                        break;
                                    }
                                }
                            }
                        }
                        o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[0]);
                    }
                    let mut handled = false;
                    if res != 0x5f && res != 0x7c {
                        o.call(m, SI_CURRENT_CUSTOM_EFFECT_SET, &[eff]);
                        let iv = o.call(m, SI_CUS_DIALOG_HANDLE, &[eff.wrapping_add(0x10), 0, 0, m.u32(eff.wrapping_add(0x108))]).eax as i32;
                        o.call(m, SI_CURRENT_CUSTOM_EFFECT_SET, &[0]);
                        handled = iv > 0;
                    }
                    if !handled {
                        // refused or failed: cleanup, free and give up
                        if f44 != 0 {
                            o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[eff]);
                            call_cleanup(m, o, eff, rec, f3c, f40, ctx);
                            o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[0]);
                        }
                        free_dso_arrays(m, rec);
                        if m.u32(eff.wrapping_add(0xc)) != 0 {
                            model_node_out(m, o, eff);
                        }
                        o.call(m, SI_DYNAMIC_LINK_UNLOAD, &[link]);
                        return -1;
                    }
                }
            }
            let mode = m.u32(rec.wrapping_add(0x34));
            if (mode.wrapping_sub(1) < 2 || (m.u32(eff.wrapping_add(0x14)) == 0 && mode == 0))
                && o.call(m, SET_ARGS_AND_RESULTS, &[eff, rel, (mode == 2) as u32]).eax as u16 == 0
            {
                free_dso_arrays(m, rec);
                if m.u32(eff.wrapping_add(0xc)) != 0 {
                    model_node_out(m, o, eff);
                }
                o.call(m, SI_DYNAMIC_LINK_UNLOAD, &[link]);
                return -1;
            }
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_eb14, 0]);
            if m.u32(eff.wrapping_add(0xc)) != 0 {
                return o.call(m, CUS_INSTANTIATE_PERSISTENT_DSO_EFFECT, &[status, eff, quiet as i32 as u32, f2c, f34, f30, f38, rel]).eax as i32;
            }
            if f2c != 0 || f30 != 0 {
                let rec = m.u32(eff.wrapping_add(0x104));
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[eff]);
                if f2c == 0 {
                    let args = [m.u32(eff.wrapping_add(100)), m.u32(rec.wrapping_add(0xc)), m.u32(rec.wrapping_add(0x10)), m.u32(rec.wrapping_add(0x1c)), m.u32(rec.wrapping_add(0x20)), rec.wrapping_add(0x24)];
                    res = o.call(m, Routine::at(f30), &args).eax;
                } else {
                    si_cus_context_init(m, ctx, eff, 9, 0, 0xffff_ffff);
                    res = o.call(m, Routine::at(f2c), &[ctx]).eax;
                }
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[0]);
            }
            let mut cleanup_res = 0u32;
            if res == 0x5f || res == 0x7c {
                // the init function gave up: cleanup only
                if res == 0x5f {
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_eb20, 3]);
                }
                let rec = m.u32(eff.wrapping_add(0x104));
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[eff]);
                if let Some(c) = call_cleanup(m, o, eff, rec, f3c, f40, ctx) {
                    cleanup_res = c;
                }
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[0]);
                if cleanup_res == 0x5f {
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_eb60, 3]);
                }
                o.call(m, SI_DYNAMIC_LINK_UNLOAD, &[link]);
                free_dso_arrays(m, rec);
                refresh_both(m, o);
                return -1;
            }
            if res == 0 {
                let rec = m.u32(eff.wrapping_add(0x104));
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[eff]);
                if f34 == 0 {
                    if f38 != 0 {
                        let mut n = m.u32(rec.wrapping_add(0xc));
                        if m.u32(eff.wrapping_add(0xc)) != 0 {
                            n = n.wrapping_sub(1);
                        }
                        let args = [m.u32(eff.wrapping_add(100)), n, m.u32(rec.wrapping_add(0x10)), m.u32(rec.wrapping_add(0x1c)), m.u32(rec.wrapping_add(0x20)), 0xffff_ffff, rec.wrapping_add(0x24)];
                        res = o.call(m, Routine::at(f38), &args).eax;
                    }
                } else {
                    si_cus_context_init(m, ctx, eff, 10, m.u32(SI_CUS_UPDATE_STATE), 0xffff_ffff);
                    res = o.call(m, Routine::at(f34), &[ctx]).eax;
                }
                o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[0]);
                if res == 0x5f {
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_eba0, 3]);
                }
            }
            let rec = m.u32(eff.wrapping_add(0x104));
            o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[eff]);
            if let Some(c) = call_cleanup(m, o, eff, rec, f3c, f40, ctx) {
                cleanup_res = c;
            }
            o.call(m, SI_CUS_SET_CURRENT_DSO_EFFECT, &[0]);
            if cleanup_res == 0x5f {
                o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_ebe0, 3]);
            }
            if res == 0 {
                res = cleanup_res;
            }
            o.call(m, SI_DYNAMIC_LINK_UNLOAD, &[link]);
            free_dso_arrays(m, rec);
            if quiet == 0 {
                o.call(m, SI_UPDATE_DEFORMATION, &[]);
                o.call(m, SI_SCENE_STEP_FRAME, &[0, 0, 0, 0]);
                refresh_both(m, o);
            }
            return if res == 0 { 1 } else { -1 };
        }
        // a lookup failed: the effect with a model loses its node, else the library is unloaded
        if m.u32(eff.wrapping_add(0xc)) == 0 {
            o.call(m, SI_DYNAMIC_LINK_UNLOAD, &[link]);
            return -1;
        }
        let model = m.u32(eff.wrapping_add(0xc));
        m.set_u32(model.wrapping_add(0x2ec), 0);
        drop_graf_node(m, o, model);
        refresh_both(m, o);
        -1
    })();
    m.release(0x50);
    r
}
