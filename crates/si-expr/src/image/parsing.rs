// SPDX-License-Identifier: MIT
//! The clause side of `exp_parsing.c` on the image: the spreadsheet query's clauses read from
//! an expression tree, and the Sumatra curve-name table look-up (finding 0013).
//!
//! A query (`SI_SprdQuery`) holds the expression at +0 (its first word the data record whose
//! +0 word is checked), a flag word at +0xc, the clause count at +0x10 and the clause array
//! at +0x14. Callees not on the image (`SI_sprdCreateDefaultClause`, `SI_sprdGenClauseStr`)
//! go to the outside world (`calls.rs`).

use super::calls::ExprCalls;
use super::eval::exp_get_oper;
use si_dict::globals::SI_ATD;
use si_mem::{Mem, Outside, Ptr32};

/// `SI_edSumatraTo3x` +4: the number of 12-byte entries; +8: the table.
const SUMATRA_COUNT: u32 = 0x08680e54;
const SUMATRA_TABLE: u32 = 0x08680e58;

/// The index of the table entry whose first word is `a`; −1 when none; when an entry
/// matches the first word but not the third, the index of the last such one
/// [soft 0x0824cba8 findSumatrafcvName].
pub fn find_sumatra_fcv_name(m: &Mem, a: i32, b: i32) -> i32 {
    let n = m.i32(SUMATRA_COUNT);
    let table = m.u32(SUMATRA_TABLE);
    let mut found = -1;
    let mut i = 0;
    while i < n {
        let at = table.wrapping_add((i as u32).wrapping_mul(12));
        if a == m.i32(at) {
            found = i;
            if b == m.i32(at.wrapping_add(8)) {
                return i;
            }
        }
        i += 1;
    }
    found
}

/// The clause `(query.count - 1)` of the query's array.
fn last_clause(m: &Mem, q: u32) -> u32 {
    m.u32(m.u32(q + 0x14).wrapping_add(m.u32(q + 0x10).wrapping_mul(4)).wrapping_sub(4))
}

/// A clause begun (`first` ≠ 0: a new default clause named from a field or a variable leaf)
/// or completed from a leaf (`first` = 0) [soft 0x0824efdc getAClause].
pub fn get_a_clause(m: &mut Mem, o: &mut dyn Outside, q: u32, leaf: u32, first: i16) -> i32 {
    if first != 0 {
        let kind = m.u32(leaf);
        if kind != 9 && kind != 0x17 {
            return 0;
        }
        let d = m.u32(leaf + 4);
        if m.i16(d + 4) == 0 {
            return 0;
        }
        let n = m.i32(q + 0x10);
        let arr = if n == 0 { m.si_calloc(1, 4) } else { m.si_realloc(m.u32(q + 0x14), (n as u32).wrapping_mul(4).wrapping_add(4)) };
        m.set_u32(q + 0x14, arr);
        m.set_i32(q + 0x10, n.wrapping_add(1));
        let c = o.sprd_create_default_clause(m);
        let slot = m.u32(q + 0x14).wrapping_add(m.u32(q + 0x10).wrapping_mul(4)).wrapping_sub(4);
        m.set_u32(slot, c);
        if kind == 0x17 {
            let v = m.u32(d + 0x20);
            m.set_u32(c, v);
            return 1;
        }
        let name = si_dict::attdict::get_name_from_fcurve(m, SI_ATD, m.i32(d + 0x10));
        m.set_u32(c, name.addr());
        return 1;
    }
    let c = last_clause(m, q);
    let arg = m.u32(leaf + 4);
    match m.u32(leaf) {
        0 | 0x14 => {
            m.set_i32(c + 8, 4);
            m.set_u32(c + 0xc, arg);
        }
        1 => {
            m.set_i32(c + 8, 1);
            m.set_u32(c + 0xc, arg);
        }
        2 => {
            m.set_i32(c + 8, 5);
            m.set_u32(c + 0xc, arg);
        }
        3 => {
            m.set_i32(c + 8, 3);
            let (a, b) = (m.u32(arg), m.u32(arg + 4));
            m.set_u32(c + 0xc, a);
            m.set_u32(c + 0x10, b);
        }
        0x10 | 0x11 => {
            m.set_i32(c + 8, 2);
            let v = (m.u32(leaf) == 0x10) as u16;
            m.set_u16(c + 0xc, v);
        }
        _ => return 0,
    }
    1
}

/// The clauses of a query found in an expression tree; answers non-zero when the tree is
/// a conjunction of comparisons the clauses can hold [soft 0x0824f124 searchForClause].
pub fn search_for_clause(m: &mut Mem, o: &mut dyn Outside, node: u32, q: u32, first: i16) -> i32 {
    if node == 0 {
        return 0;
    }
    let before = m.i32(q + 0x10);
    let oper = exp_get_oper(m, Ptr32::new(node)) as i32;
    if oper == 0 {
        let leaf = m.u32(node + 4);
        return get_a_clause(m, o, q, leaf, first) as i16 as i32;
    }
    if (oper.wrapping_sub(0xb) as u32) > 1 {
        let left = m.u32(node + 4);
        let mut r = search_for_clause(m, o, left, q, 1) as i16;
        if r != 0 {
            let arr = m.u32(q + 0x14);
            if m.u32(m.u32(node + 4)) == 0 {
                let c = m.u32(arr.wrapping_add((before as u32).wrapping_mul(4)));
                m.set_i32(c + 4, oper);
            } else {
                let c = last_clause(m, q);
                m.set_i32(c + 0x14, oper);
            }
            let right = m.u32(node + 8);
            if right != 0 {
                r = search_for_clause(m, o, right, q, 0) as i16;
            }
            if r != 0 {
                if (m.u8(node) as i8) < 0 {
                    let arr = m.u32(q + 0x14);
                    let c = m.u32(arr.wrapping_add((before as u32).wrapping_mul(4)));
                    let v = m.i16(c + 0x1c).wrapping_add(1);
                    m.set_i16(c + 0x1c, v);
                    let c = last_clause(m, q);
                    let v = m.i16(c + 0x1e).wrapping_add(1);
                    m.set_i16(c + 0x1e, v);
                }
                return r as i32;
            }
        }
    }
    0
}

/// A query's clauses made from its expression, and their texts [soft 0x0824f210 SI_getClauseFromQuery].
pub fn get_clause_from_query(m: &mut Mem, o: &mut dyn Outside, q: u32) {
    if q == 0 {
        return;
    }
    let data = m.u32(q);
    if data == 0 || m.u32(data) == 0 {
        return;
    }
    m.set_i32(q + 0x10, 0);
    let tree = m.u32(m.u32(data) + 8);
    let r = search_for_clause(m, o, tree, q, 1);
    if r as i16 == 0 {
        m.set_i32(q + 0x10, 0);
        m.set_i16(q + 0xc, 1);
    } else {
        let count = m.i32(q + 0x10);
        m.set_i16(q + 0xc, 0);
        let mut i = 0;
        while i < count {
            let arr = m.u32(q + 0x14);
            o.sprd_gen_clause_str(m, count, arr, i);
            i += 1;
        }
    }
}

/// The literals the printer appends: a blank, and the two dots of `GeneratePrefixForSumatra`.
const S_BLANK: u32 = 0x08699f5d;
const S_DOT_A: u32 = 0x08699f7d;
const S_DOT_B: u32 = 0x08699f7f;
/// `SI_edSumatraTo3x` +0x14: the prefix the converter has set (a string address).
const SUMATRA_PREFIX: u32 = 0x08680e64;
/// `SI_RD`.
const SI_RD_ADDR: u32 = 0x087815f0;

/// `str` appended to the block `dest` (a new one, 100 bytes at a time, when `dest` is null or
/// full), and a blank after it if `sep` [soft 0x0824c830 SI_checkAllocStr]. Answers the block.
pub fn check_alloc_str(m: &mut Mem, dest: u32, s: u32, sep: i16) -> u32 {
    let mut block = dest;
    let have = if dest == 0 {
        block = m.si_calloc(100, 1);
        0
    } else {
        m.strlen(dest)
    };
    let add = m.strlen(s);
    let blocks = ((add as i32).wrapping_add(have as i32).wrapping_add(2)) / 100 + 1;
    let mut out = block;
    if (have as i32) / 100 + 1 != blocks {
        out = m.si_calloc((blocks as u32).wrapping_mul(100), 1);
        si_mem::clib::strcpy(m, out, block);
        if block != 0 {
            m.free(block);
        }
    }
    si_mem::clib::strcat(m, out, s);
    if sep != 0 {
        si_mem::clib::strcat(m, out, S_BLANK);
    }
    out
}

/// The prefix of a name in a scene converted from the newer program: the converter's
/// prefix, a dot, and (for a model argument) the name of the model's root and a dot; null
/// when the converter has none [soft 0x0824cbf8 GeneratePrefixForSumatra].
pub fn generate_prefix_for_sumatra(m: &mut Mem, arg: u32, elem: u32) -> u32 {
    let prefix = m.u32(SUMATRA_PREFIX);
    if prefix == 0 {
        return 0;
    }
    let mut p = check_alloc_str(m, 0, prefix, 0);
    p = check_alloc_str(m, p, S_DOT_A, 0);
    let model_chap = m.u32(super::rec::cid::MODEL);
    if m.u32(arg + 8) == model_chap {
        let root = super::elem::model_root(m, m.u32(elem + 4));
        if root != 0 {
            let procd = m.i32(m.u32(si_dict::globals::SI_PROC_DATA.addr()));
            let dict = m.u32(si_dict::globals::SI_ELEM_DICTS.addr()).wrapping_add((procd as u32).wrapping_mul(0x70));
            let list = m.u32(dict + 4).wrapping_add(model_chap.wrapping_mul(0x40));
            let idx = m.u32(root + 0x354);
            let e = m.u32(list + 8).wrapping_add(idx.wrapping_mul(0x84));
            let node = m.u32(e);
            if m.u32(node + 0x24) != 0 {
                p = check_alloc_str(m, p, m.u32(node + 0x24), 0);
                p = check_alloc_str(m, p, S_DOT_B, 0);
            }
        }
    }
    p
}

/// The relation group of the curve a reference leaf names: the custom-effect group for a
/// custom argument, else `SI_fcvGetRelGrpFromType` of its curve id.
fn group_of(m: &mut Mem, o: &mut dyn Outside, kind: u32, data: u32) -> i32 {
    if kind == 0x17 {
        let sp = m.sp();
        let g = Ptr32::<i32>::new(m.local(4));
        super::misc::fcv_get_rel_grp_from_custom(m, g);
        let v = m.read(g);
        m.set_sp(sp);
        v
    } else {
        o.fcv_get_rel_grp_from_type(m, m.i32(data + 0x10))
    }
}

/// A relation record for an argument: its curve id, and for a custom one a copy of the name.
fn new_relation_data(m: &mut Mem, kind: u32, data: u32) -> u32 {
    let d = m.si_calloc(1, 0x14);
    let id = m.u32(data + 0x10);
    m.set_u32(d, id);
    if kind == 0x17 {
        let name = m.strdup(m.u32(data + 0x20));
        m.set_u32(d + 0x10, name);
    }
    d
}

/// The relations, serial number `serial`, from every argument the tree `node` reads to the
/// element `e` (owned by the target `target`): each reference leaf adds one relation;
/// answers non-zero (as 16 bits) when any was added [soft 0x0824b134 installCustomExpress].
pub fn install_custom_express(m: &mut Mem, o: &mut dyn Outside, target: u32, node: u32, e: u32, serial: i32) -> i16 {
    let mut got: u16 = 0;
    let tdata = m.u32(target + 4);
    let tkind = m.u32(target);
    let grp = group_of(m, o, tkind, tdata);
    let obj = super::regen::get_object(m, Ptr32::new(node));
    if obj == 0 {
        let oper = exp_get_oper(m, Ptr32::new(node));
        if oper == 0 {
            let leaf = m.u32(node + 4);
            if m.u32(leaf) == 10 {
                let list = m.u32(leaf + 4);
                let n = m.i32(list + 4);
                let mut i = 0;
                while i < n {
                    let sub = m.u32(m.u32(list + 8).wrapping_add((i as u32).wrapping_mul(4)));
                    got |= install_custom_express(m, o, target, sub, e, serial) as u16;
                    i += 1;
                }
            }
        } else {
            got = install_custom_express(m, o, target, m.u32(node + 4), e, serial) as u16;
            let right = m.u32(node + 8);
            if right != 0 {
                got |= install_custom_express(m, o, target, right, e, serial) as u16;
            }
        }
    } else {
        let okind = m.u32(obj);
        let odata = m.u32(obj + 4);
        let ogrp = group_of(m, o, okind, odata);
        let d = new_relation_data(m, okind, odata);
        let ed = super::rec::cur_dict(m);
        si_dict::elemrel::add_serial_relation(
            m,
            ed,
            Ptr32::new(SI_RD_ADDR),
            m.i32(tdata + 8),
            m.i32(odata + 8),
            grp,
            ogrp,
            m.i32(tdata + 0xc),
            m.i32(odata + 0xc),
            0x4e2a,
            serial,
            1,
            0,
            Ptr32::new(d),
            1,
            1,
        );
        got = 1;
    }
    let _ = e;
    got as i16
}

/// The expression `expr` made the element's expression: added to the EXPRESSIONS chapter
/// (index −1, `*name` set when it was null) or put in the element `index`, tied by relation
/// 20010 to the target element and to every argument it reads. Answers the relation made
/// current on the target element (0 when the expression is not an assignment)
/// [soft 0x0824b384 SI_edElementSetExpression].
///
/// Differs from the original only where `SI_edAddExpression` answers without adding (the
/// dictionary switch off) and `*name` is null: the original then copies whatever was in a
/// register, the port leaves the name alone.
pub fn ed_element_set_expression(m: &mut Mem, o: &mut dyn Outside, expr: u32, name: u32, index: i32) -> u32 {
    let nframes = m.i32(super::rec::SI_FRAME_END.addr()).wrapping_sub(m.i32(super::rec::SI_FRAME_START.addr())).wrapping_add(1);
    let rel_data = super::regen::expr_init_rel_data(m, o, Ptr32::new(expr), 0, nframes).addr();
    let root = m.u32(expr);
    let oper = exp_get_oper(m, Ptr32::new(root));
    if !crate::eval::node_is_assign_oper(oper) {
        return 0;
    }
    let chap = super::rec::chap(m, super::rec::cid::EXP);
    if index == -1 {
        let added = super::exio::ed_add_expression(m, o, m.u32(name), expr);
        if m.u32(name) == 0 && added != 0 {
            let copy = m.strdup(added);
            m.set_u32(name, copy);
        }
    } else {
        let ed = super::rec::cur_dict(m);
        let e = si_dict::elemdict::get_elem(m, ed, chap, index);
        let old = m.u32(e.addr() + 4);
        o.express_data_dispose(m, old);
        m.set_u32(e.addr() + 4, expr);
        si_dict::elemdict::set_dirty(m, e, 1);
        let node = m.u32(e.addr());
        if si_mem::clib::strcmp(m, m.u32(node + 0x14), m.u32(name)) != 0 {
            let prefix = m.u32(node + 0x10);
            si_dict::elemname::change_name(m, &mut super::calls::DictOutside(o), ed, chap, e, si_mem::CStr32::new(prefix), si_mem::CStr32::new(m.u32(name)));
        }
    }
    let ed = super::rec::cur_dict(m);
    let mut at = si_dict::elemdict::find_elem_by_data(m, &mut super::calls::DictOutside(o), ed, chap, Ptr32::new(expr));
    if at < 0 {
        let ed = super::rec::cur_dict(m);
        at = si_dict::elemname::find_elem_by_name_only(m, ed, chap, Ptr32::NULL, si_mem::CStr32::new(m.u32(name)), -1, 0);
    }
    // the target's leaf record: (kind, data)
    let leaf = m.u32(m.u32(root + 4) + 4);
    let kind = m.u32(leaf);
    let data = m.u32(leaf + 4);
    let grp = group_of(m, o, kind, data);
    let ed = super::rec::cur_dict(m);
    let src = si_dict::elemdict::get_elem(m, ed, m.i32(data + 8), m.i32(data + 0xc));
    let serial = si_dict::relations::get_rel_serial_number(m);
    let ed = super::rec::cur_dict(m);
    si_dict::elemrel::add_serial_relation(
        m, ed, Ptr32::new(SI_RD_ADDR), m.i32(data + 8), chap, grp, grp, m.i32(data + 0xc), at, 0x4e2a, serial, 1, 0, Ptr32::new(rel_data), 1, 1,
    );
    let mut got = install_custom_express(m, o, leaf, m.u32(root + 8), src.addr(), serial) as u16;
    let n = m.i32(expr + 4);
    let mut i = 0;
    while i < n {
        let tree = m.u32(m.u32(expr + 8).wrapping_add(4).wrapping_add((i as u32).wrapping_mul(8)));
        got |= install_custom_express(m, o, leaf, tree, src.addr(), serial) as u16;
        i += 1;
    }
    if got == 0 {
        let d = new_relation_data(m, kind, data);
        let ed = super::rec::cur_dict(m);
        si_dict::elemrel::add_serial_relation(
            m, ed, Ptr32::new(SI_RD_ADDR), m.i32(data + 8), m.i32(data + 8), grp, grp, m.i32(data + 0xc), m.i32(data + 0xc), 0x4e2a, serial, 1, 0, Ptr32::new(d), 1, 1,
        );
    }
    let ed = super::rec::cur_dict(m);
    si_dict::elemrel::set_rel_to_serialid(m, ed, src, 0x4e2a, serial, 1).addr()
}

/// `si_IncompatibleFcv`: set by [`expand_an_element`] when an element lacks the curve a generic reference names.
const SI_INCOMPATIBLE_FCV: u32 = 0x08699edc;
/// `si_validateArgs`.
const SI_VALIDATE_ARGS: u32 = 0x08699ece;
/// `SI_queryMode`.
const SI_QUERY_MODE: u32 = 0x08699ec0;

/// What [`expr_traverse`] calls on a leaf: the node, its leaf record; non-zero stops the walk.
pub type Visit<'a> = &'a mut dyn FnMut(&mut Mem, &mut dyn Outside, u32, u32) -> i16;

/// The leaf record the walk stopped at, or 0: `visit` on every leaf that is not a function
/// call, operands in order (a function call's arguments in order, the call itself not
/// visited; a call whose function is unknown is passed over; a call with no argument stops
/// the walk), until one visit answers non-zero [soft 0x0824eb34 SI_exprTraverse].
pub fn expr_traverse(m: &mut Mem, o: &mut dyn Outside, node: u32, visit: Visit) -> u32 {
    let oper = exp_get_oper(m, Ptr32::new(node)) as i32;
    if oper == 0 {
        let nop = m.u32(node + 4);
        if m.i32(nop) == 10 {
            let call = m.u32(nop + 4);
            let index = m.i32(call);
            if crate::token::get_fct_ptr(index).is_some() {
                let n = m.i32(call + 4);
                let mut i = 0;
                loop {
                    if n <= i {
                        // the leaf itself only when there was no argument to walk
                        return if i == 0 { nop } else { 0 };
                    }
                    let arg = m.u32(m.u32(call + 8).wrapping_add((i as u32).wrapping_mul(4)));
                    let r = expr_traverse(m, o, arg, visit);
                    if r != 0 {
                        return r;
                    }
                    i += 1;
                }
            }
        } else if visit(m, o, node, nop) != 0 {
            return nop;
        }
        return 0;
    }
    let next;
    if !(0xb..=0xc).contains(&oper) {
        let r = expr_traverse(m, o, m.u32(node + 4), visit);
        if r != 0 {
            return r;
        }
        next = m.u32(node + 8);
    } else {
        next = m.u32(node + 4);
    }
    expr_traverse(m, o, next, visit)
}

/// A generic reference (`this`) given the element it stands for: chapter and index copied from
/// `ident` into the argument record; unless a query runs, `SI_isValidFcv` of the element
/// class, the curve and the index is asked and a "no" sets `si_IncompatibleFcv`. Answers 0
/// [soft 0x08248f2c expandAnElement].
pub fn expand_an_element(m: &mut Mem, o: &mut dyn Outside, nop: u32, ident: u32) -> i16 {
    let kind = m.u32(nop);
    if kind == 9 || kind == 0x17 {
        let d = m.u32(nop + 4);
        if m.i16(d + 4) == 1 {
            let (chap, elem) = (m.u32(ident), m.u32(ident + 4));
            m.set_u32(d + 8, chap);
            m.set_u32(d + 0xc, elem);
            if m.i16(SI_QUERY_MODE) == 0 {
                let graf = from_chap_to_graf(m, chap as i32);
                let fcv = m.i32(d + 0x10);
                if o.is_valid_fcv(m, graf, fcv, elem as i32) == 0 {
                    m.set_i16(SI_INCOMPATIBLE_FCV, 1);
                }
            }
        }
    }
    0
}

/// Every generic reference of an expression expanded for the element `ident` (chapter, index):
/// the variables' trees, then the right-hand side [soft 0x08248f98 si_exprExpandGenRef].
pub fn expr_expand_gen_ref(m: &mut Mem, o: &mut dyn Outside, expr: u32, ident: u32) {
    m.set_i16(SI_INCOMPATIBLE_FCV, 0);
    let n = m.i32(expr + 4);
    let mut i = 0;
    while i < n {
        let tree = m.u32(m.u32(expr + 8).wrapping_add(4).wrapping_add((i as u32).wrapping_mul(8)));
        expr_traverse(m, o, tree, &mut |m, o, _node, nop| expand_an_element(m, o, nop, ident));
        i += 1;
    }
    let root = m.u32(expr);
    let rhs = m.u32(root + 8);
    expr_traverse(m, o, rhs, &mut |m, o, _node, nop| expand_an_element(m, o, nop, ident));
}

/// Whether the element `ident` satisfies a spreadsheet query: true when there is no query
/// (no expression, no relation data); else the query expression evaluated at the current
/// frame for the element, true unless the value is 0 [soft 0x08248ff4 SI_queryOnAnElement].
/// An evaluation that fails leaves the value unwritten; the port answers true.
pub fn query_on_an_element(m: &mut Mem, o: &mut dyn Outside, ident: u32, query: u32) -> bool {
    if query == 0 {
        return true;
    }
    let expr = m.u32(query);
    if expr == 0 || m.u32(expr) == 0 || m.u32(query + 4) == 0 {
        return true;
    }
    m.set_i16(SI_VALIDATE_ARGS, 1);
    m.set_i16(SI_QUERY_MODE, 1);
    expr_expand_gen_ref(m, o, expr, ident);
    let frame = m.i32(super::rec::SI_FRAME_CURRENT.addr());
    let rate = m.f32(super::rec::SI_FRAME_RATE.addr());
    let start = m.i32(super::rec::SI_FRAME_START.addr());
    // fild, frndint (exact for an integer), fstp to single, fld, fdiv by the rate, fstp to single
    let f = si_x87::F80::from_i32(frame).to_f32();
    let time = (si_x87::F80::from_f32(f) / si_x87::F80::from_f32(rate)).to_f32();
    let sp = m.sp();
    let result = Ptr32::<f32>::new(m.local(4));
    let err = Ptr32::<i32>::new(m.local(0x40));
    let r = super::eval::expression_evaluate(m, o, Ptr32::new(expr), Ptr32::new(m.u32(query + 4)), frame.wrapping_sub(start), frame, time, result, err);
    m.set_i16(SI_QUERY_MODE, 0);
    let v = m.f32(result.addr());
    m.set_sp(sp);
    r.is_err() || v != 0.0
}

/// The function-curve editor's class of a chapter id: the camera chapter 4, lights 6,
/// materials 14, 2D textures 15, 3D textures 16, waves 11, any other 2
/// [soft 0x0824f6cc si_fromChap2Graf]. The chapter ids are the scene dictionary's (`SI_cid`).
pub fn from_chap_to_graf(m: &Mem, chap: i32) -> i32 {
    use super::rec::cid;
    let is = |which: u32| chap == m.i32(which);
    if is(cid::CAM) {
        4
    } else if is(cid::LIG) {
        6
    } else if is(cid::MAT) {
        14
    } else if is(cid::T2D) {
        15
    } else if is(cid::T3D) {
        16
    } else if is(cid::WAV) {
        11
    } else {
        2
    }
}

/// `SI_queryMode` (16 bits).
const SI_QUERY_MODE_ADDR: u32 = 0x08699ec0;
/// The literals of the next function: the text of the empty query, and the `=` its text starts with.
const S_CHPTYP_ALL: u32 = 0x0869a0be;
const S_EQUALS: u32 = 0x0869a0d1;

/// The text and the tree of a spreadsheet query made from its clauses
/// [soft 0x0824ee98 SI_generateQueryFromClause]. Does nothing when the query's flag word
/// (16 bits at +0xc) is not 0. Otherwise the query's old tree, relation data and text are given up,
/// and with no clause the text is `this.chptyp == ALL` and the tree stays null. With clauses the
/// clauses' texts (each record's string at +0x18) are joined behind an `=` with blanks, parsed in
/// query mode, and the query gets the tree, its relation data for the frames of the scene
/// (`SI_frameEnd - SI_frameStart + 1`, a query's) and the right-hand side of the tree printed.
///
/// The parser, the printer and the relation data's disposal are outside calls (`calls.rs`).
pub fn generate_query_from_clause(m: &mut Mem, o: &mut dyn Outside, q: u32) {
    if m.i16(q + 0xc) != 0 {
        return;
    }
    let sp = m.sp();
    let out = m.local(4);
    m.set_u32(out, 0);
    let old = m.u32(q);
    o.express_data_dispose(m, old);
    m.set_u32(q, 0);
    let old = m.u32(q + 4);
    o.expr_rel_data_dispose(m, old);
    m.set_u32(q + 4, 0);
    let text = m.u32(q + 8);
    if text != 0 {
        m.free(text);
        m.set_u32(q + 8, 0);
    }
    if m.i32(q + 0x10) <= 0 {
        let s = m.strdup(S_CHPTYP_ALL);
        m.set_u32(q + 8, s);
        m.set_sp(sp);
        return;
    }
    let mut buf = check_alloc_str(m, 0, S_EQUALS, 1);
    let mut i = 0;
    while i < m.i32(q + 0x10) {
        let clause = m.u32(m.u32(q + 0x14).wrapping_add(4u32.wrapping_mul(i as u32)));
        let piece = m.u32(clause + 0x18);
        buf = check_alloc_str(m, buf, piece, 1);
        i += 1;
    }
    m.set_i16(SI_QUERY_MODE_ADDR, 1);
    o.gen_cus_expr_parsing_tree(m, si_mem::CStr32::new(buf), 0, 0, Ptr32::new(out), 1, 0);
    m.set_i16(SI_QUERY_MODE_ADDR, 0);
    let expr = m.u32(out);
    m.set_u32(q, expr);
    let frames = m.i32(super::rec::SI_FRAME_END.addr()).wrapping_sub(m.i32(super::rec::SI_FRAME_START.addr())).wrapping_add(1);
    let data = super::regen::expr_init_rel_data(m, o, Ptr32::new(expr), 1, frames);
    m.set_u32(q + 4, data.addr());
    let rhs = o.get_complete_expr_format(m, m.u32(q), Ptr32::NULL, Ptr32::NULL, Ptr32::NULL, 1, 0);
    m.set_u32(q + 8, rhs);
    if buf != 0 {
        m.free(buf);
    }
    m.set_sp(sp);
}
