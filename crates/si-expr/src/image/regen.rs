// SPDX-License-Identifier: MIT
//! An expression tied to its relation, on the image (`exp_parsing.c`, `exp_fct.c`,
//! `express_io.c`; finding 0013, sections 7, 8 and 12): references given their elements
//! from the relation's sub-relations, the relation's data (`SI_ExprRelData`) made, the
//! random tables filled.

use super::calls::ExprCalls;
use super::rec::{self, cid, ExprNode, ExprRelData, Expression};
use crate::tree::nop;
use si_mem::{Mem, Outside, Ptr32};
use si_types::dict::{ARelation, Elem, ElemDict};

/// The random calls of a tree, appended to the list at `*list` with `*count` entries
/// [soft 0x0824dfa0 getExprRandList]: left before right (not the target of an assignment),
/// a function's arguments in order. The list grows five entries at a time (`SI_calloc(6,
/// 4)`, then `SI_realloc` to count × 4 + 0x18).
pub fn get_expr_rand_list(m: &mut Mem, node: Ptr32<ExprNode>, list: Ptr32<u32>, count: Ptr32<i32>) {
    let mut arr = m.u32(list.addr());
    if node.is_null() {
        return;
    }
    let oper = super::eval::exp_get_oper(m, node);
    if oper == 0 {
        let leaf = m.u32(node.addr() + 4);
        let kind = m.i32(leaf);
        if kind == nop::RAND {
            let n = m.read(count);
            if n == (n / 5) * 5 {
                arr = if n == 0 { m.si_calloc(6, 4) } else { m.si_realloc(arr, (n as u32).wrapping_mul(4) + 0x18) };
                m.set_u32(list.addr(), arr);
            }
            m.set_u32(arr.wrapping_add((n as u32).wrapping_mul(4)), leaf);
            m.write(count, n + 1);
        } else if kind == nop::FCT {
            let f = m.u32(leaf + 4);
            let mut i = 0;
            while i < m.i32(f + 4) {
                let a = m.u32(m.u32(f + 8) + 4 * i as u32);
                get_expr_rand_list(m, Ptr32::new(a), list, count);
                i += 1;
            }
        }
    } else {
        if !crate::eval::node_is_assign_oper(oper) {
            get_expr_rand_list(m, Ptr32::new(m.u32(node.addr() + 4)), list, count);
        }
        let r = m.u32(node.addr() + 8);
        if r != 0 {
            get_expr_rand_list(m, Ptr32::new(r), list, count);
        }
    }
}

/// The random calls of an expression [soft 0x0824e0ac si_getExprRandListNode]: those of
/// the variables' definitions, then those of the right-hand side; the list (null when
/// there is none) and its length in `*count` (left alone for a null expression).
pub fn get_expr_rand_list_node(m: &mut Mem, expr: Ptr32<Expression>, count: Ptr32<i32>) -> u32 {
    let sp = m.sp();
    let list = Ptr32::<u32>::new(m.local(4));
    m.write(list, 0);
    if !expr.is_null() {
        m.write(count, 0);
        let mut i = 0;
        while i < m.i32(expr.addr() + 4) {
            let v = m.u32(m.u32(expr.addr() + 8) + 8 * i as u32 + 4);
            get_expr_rand_list(m, Ptr32::new(v), list, count);
            i += 1;
        }
        let root = m.u32(expr.addr());
        if root != 0 {
            get_expr_rand_list(m, Ptr32::new(m.u32(root + 8)), list, count);
        }
    }
    let r = m.read(list);
    m.set_sp(sp);
    r
}

/// Fills a relation's random tables [soft 0x08249134 SI_exprGenRandomValues]: for each
/// random call with a seed, a table of `nframes` floats (an old one freed first), the
/// generator seeded and one value drawn per frame. The table of tables is allocated once,
/// for the count of that time. The list of calls is not freed.
pub fn expr_gen_random_values(m: &mut Mem, o: &mut dyn Outside, data: Ptr32<ExprRelData>, nframes: i32, expr: Ptr32<Expression>) {
    let d = data.addr();
    let sp = m.sp();
    let count = Ptr32::<i32>::new(m.local(4));
    m.write(count, 0);
    let list = get_expr_rand_list_node(m, expr, count);
    let n = m.read(count);
    m.set_sp(sp);
    if n == 0 {
        return;
    }
    if m.u32(d + 0x1c) == 0 {
        let t = m.si_calloc(n as u32, 4);
        m.set_u32(d + 0x1c, t);
    }
    m.set_i16(d + 0x16, n as i16);
    m.set_i32(d + 0x18, nframes);
    let mut i = 0;
    while i < n {
        let leaf = m.u32(list + 4 * i as u32);
        let r = m.u32(leaf + 4);
        if m.i32(r + 8) >= 0 {
            let slot = m.u32(d + 0x1c) + 4 * i as u32;
            let old = m.u32(slot);
            if old != 0 {
                m.free(old);
                m.set_u32(slot, 0);
            }
            let t = m.si_calloc(nframes as u32, 4);
            let slot = m.u32(d + 0x1c) + 4 * i as u32;
            m.set_u32(slot, t);
            let seed = m.u32(r + 8);
            o.srand(m, seed);
            let mut k = 0;
            while k < nframes {
                let v = super::eval::expr_random_eval(m, o, Ptr32::new(r)).to_f32();
                let t = m.u32(m.u32(d + 0x1c) + 4 * i as u32);
                m.set_f32(t + 4 * k as u32, v);
                k += 1;
            }
        }
        i += 1;
    }
}

/// A fresh relation record for an expression [soft 0x0824b2f8 si_exprInitRelData]
/// (`SI_calloc(1, 0x20)`): the random tables and, unless it is for a query, the target's
/// fcurve type, access information and, for a custom parameter, a copy of its name.
pub fn expr_init_rel_data(m: &mut Mem, o: &mut dyn Outside, expr: Ptr32<Expression>, query: i16, nframes: i32) -> Ptr32<ExprRelData> {
    let d = m.si_calloc(1, 0x20);
    expr_gen_random_values(m, o, Ptr32::new(d), nframes, expr);
    let root = m.u32(expr.addr());
    if query == 0 && root != 0 {
        let a = m.u32(m.u32(m.u32(root + 4) + 4) + 4);
        for k in 0..4 {
            let w = m.u32(a + 0x10 + 4 * k);
            m.set_u32(d + 4 * k, w);
        }
        if m.i32(a + 0x10) == 0x236 {
            let s = m.strdup(m.u32(a + 0x20));
            m.set_u32(d + 0x10, s);
        }
    }
    m.set_i16(d + 0x14, query);
    Ptr32::new(d)
}

/// The leaf of a reference, or null [soft 0x0824b084 get_object].
pub fn get_object(m: &Mem, node: Ptr32<ExprNode>) -> u32 {
    if super::eval::exp_get_oper(m, node) == 0 {
        let leaf = m.u32(node.addr() + 4);
        let k = m.i32(leaf);
        if k == nop::ARG || k == nop::CUSTOM_ARG {
            return leaf;
        }
    }
    0
}

/// A reference given the chapter and element of entry `num` of the argument list
/// [soft 0x0824e268 set_rel_from_list]; a cluster parameter of a model (0xb7, 0xb8) also
/// has its cluster looked up by name (`si_expFindValidCluster`, not ported: called through
/// the outside world), an error's text shown, and the name freed.
pub fn set_rel_from_list(m: &mut Mem, o: &mut dyn Outside, leaf: u32, list: u32) {
    let a = m.u32(leaf + 4);
    let num = m.i32(a);
    let entry = list.wrapping_add((num as u32).wrapping_mul(0x24));
    let c = m.i32(entry + 8);
    m.set_i32(a + 8, c);
    let e = m.i32(entry + 0xc);
    m.set_i32(a + 0xc, e);
    if m.i32(a + 8) == rec::chap(m, cid::MODEL) && (m.i32(a + 0x10) as u32).wrapping_sub(0xb7) < 2 {
        let ed = rec::cur_dict(m);
        let el = si_dict::elemdict::get_elem(m, ed, m.i32(entry + 8), m.i32(entry + 0xc));
        let model = m.u32(el.addr() + 4);
        m.set_i32(a + 0x18, -1);
        let sp = m.sp();
        let err = m.local(4);
        let (kl, name, fcv, index) = (m.u32(model + 0x348), m.u32(a + 0x1c), m.i32(a + 0x10), m.i32(a + 0x14));
        o.exp_find_valid_cluster(m, kl, name, fcv, index, a + 0x18, err);
        let code = m.i32(err);
        m.set_sp(sp);
        if code != 0 && super::eval::expr_get_error_msg(m, code) != 0 {
            let t = super::eval::expr_get_error_msg(m, code);
            o.status_display_message(m, Ptr32::new(t), 2);
        }
        let n = m.u32(a + 0x1c);
        if n != 0 {
            m.free(n);
            m.set_u32(a + 0x1c, 0);
        }
    }
}

/// Every reference of a tree given its element from the argument list
/// [soft 0x0824e36c si_exprResolveRelation]; the target of an assignment also takes the
/// fcurve type of entry 0 (the relation's parameter).
pub fn expr_resolve_relation(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<ExprNode>, list: u32) {
    let leaf = get_object(m, node);
    if leaf != 0 {
        set_rel_from_list(m, o, leaf, list);
        return;
    }
    let oper = super::eval::exp_get_oper(m, node);
    if oper == 0 {
        let leaf = m.u32(node.addr() + 4);
        if m.i32(leaf) == nop::FCT {
            let f = m.u32(leaf + 4);
            let mut i = 0;
            while i < m.i32(f + 4) {
                let a = m.u32(m.u32(f + 8) + 4 * i as u32);
                expr_resolve_relation(m, o, Ptr32::new(a), list);
                i += 1;
            }
        }
    } else {
        if !crate::eval::node_is_assign_oper(oper) {
            expr_resolve_relation(m, o, Ptr32::new(m.u32(node.addr() + 4)), list);
        } else {
            let t = m.u32(m.u32(node.addr() + 4) + 4);
            set_rel_from_list(m, o, t, list);
            let fcv = m.u32(list + 0x10);
            m.set_u32(m.u32(t + 4) + 0x10, fcv);
        }
        let r = m.u32(node.addr() + 8);
        if r != 0 {
            expr_resolve_relation(m, o, Ptr32::new(r), list);
        }
    }
}

/// An expression's references tied to its relation on the target element `e`
/// [soft 0x08256544 SI_expressArgsRegenerate].
///
/// The sub-relations of the relation's serial number on `e` (forward, the relation's group)
/// are walked from the one to the EXPRESSIONS chapter: each later one is an argument, its
/// chapter and element appended to a list of 0x24-byte entries (`SI_calloc(n + 2, 0x24)`,
/// then `SI_realloc` by 0x24). Entry 0 is the target (`chap`, `index`) with the parameter
/// and access information of the relation's old data (the description's RELDATA). Every
/// variable's tree and the statement are resolved against the list; with `make_data` the
/// relation gets a new `SI_ExprRelData` for the play range and the old data is freed. An
/// expression element without a tree takes the data away (freed). The list is freed.
pub fn express_args_regenerate(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, chap: i32, index: i32, exp_elem: Ptr32<Elem>, rel: Ptr32<ARelation>, make_data: i16) {
    let sp = m.sp();
    let lc = Ptr32::<i32>::new(m.local(4));
    let l8 = Ptr32::<i32>::new(m.local(4));
    let mut head: u32 = 0; // local_18
    let mut n: i32 = 0; // local_1c
    let mut list: u32 = 0; // local_20
    let group = m.i16(rel.addr() + 0x12) as i32;
    let serial = m.i32(rel.addr() + 0xc);
    let mut r = si_dict::elemrel::set_rel_to_serialid(m, ed, e, 0x4e2a, serial, 1).addr();
    if r != 0 {
        let mut size: u32 = 0x24;
        let mut at: u32 = 0;
        loop {
            let next_at;
            if m.i16(r + 0x10) as i32 == rec::chap(m, cid::EXP) {
                next_at = 0;
                size = 0x24;
                n = 0;
                head = r;
            } else {
                next_at = if head != 0 { at + 0x24 } else { at };
                if head != 0 {
                    size += 0x24;
                    list = if list == 0 { m.si_calloc((n + 2) as u32, 0x24) } else { m.si_realloc(list, size) };
                    let c = m.i16(r + 0x10) as i32;
                    m.set_i32(list + at + 0x2c, c);
                    let id = m.i32(r + 4);
                    m.set_i32(list + at + 0x30, id);
                    n += 1;
                }
            }
            let f = si_dict::elemrel::find_next_related_elem(m, ed, e, 1, group, 0x4e2a, -1, lc, l8);
            if f.is_null() {
                break;
            }
            r = si_dict::elemrel::get_cur_relation(m, ed, e, 1, group).addr();
            if r == 0 {
                break;
            }
            at = next_at;
            if !(head == 0 || m.i32(head + 0xc) == m.i32(r + 0xc)) {
                break;
            }
        }
    }
    if head != 0 {
        let old = m.u32(head + 0x18);
        let expr = m.u32(exp_elem.addr() + 4);
        let root = if expr == 0 { 0 } else { m.u32(expr) };
        if expr == 0 || root == 0 {
            m.set_u32(head + 0x18, 0);
            if old != 0 {
                m.free(old);
            }
            m.set_sp(sp);
            return;
        }
        if list != 0 {
            m.set_i32(list + 8, chap);
            m.set_i32(list + 0xc, index);
            let fcv = m.i32(old);
            m.set_i32(list + 0x10, fcv);
            if fcv == 0x236 && m.u32(old + 0x10) != 0 {
                let s = m.strdup(m.u32(old + 0x10));
                m.set_u32(list + 0x20, s);
            }
            for k in 0..3 {
                let w = m.u32(old + 4 + 4 * k);
                m.set_u32(list + 0x14 + 4 * k, w);
            }
        }
        let mut i = 0;
        while i < m.i32(expr + 4) {
            let v = m.u32(m.u32(expr + 8) + 4 + 8 * i as u32);
            expr_resolve_relation(m, o, Ptr32::new(v), list);
            i += 1;
        }
        expr_resolve_relation(m, o, Ptr32::new(root), list);
        if make_data != 0 {
            let span = m[rec::SI_FRAME_END].wrapping_sub(m[rec::SI_FRAME_START]).wrapping_add(1);
            let d = expr_init_rel_data(m, o, Ptr32::new(expr), 0, span);
            m.set_u32(head + 0x18, d.addr());
            if old != 0 {
                m.free(old);
            }
        }
        let s = m.i32(head + 0xc);
        si_dict::elemrel::set_rel_to_serialid(m, ed, e, 0x4e2a, s, 1);
    }
    if list != 0 {
        m.free(list);
    }
    m.set_sp(sp);
}

/// The C library's `getenv` for the one variable this file asks about: whether
/// `SI_DO_NOT_REMOVE_ORPHANED_EXP` is set. The outside world answers (`getenv`, plt
/// 0x080c3bb8) with the value's address or 0.
fn orphans_kept(m: &mut Mem, o: &mut dyn Outside) -> bool {
    let sp = m.sp();
    let name = m.local(32);
    m.put_cstr(name, b"SI_DO_NOT_REMOVE_ORPHANED_EXP");
    let r = o.call(m, si_mem::Routine::new("getenv", 0x080c3bb8), &[name]).eax;
    m.set_sp(sp);
    r != 0
}

/// Every expression of the dictionary tied again to its relations, orphans given up
/// [soft 0x082567b4 SI_expressionsRegenerate].
///
/// For each element of the EXPRESSIONS chapter in use and read from the files (new flag
/// 1), in each of its backward relation lists but the fifth, every relation 20010 to a
/// target element is regenerated ([`express_args_regenerate`], with the target's relation
/// set to that serial number first and back to the one the target was at afterwards); an
/// element that had relations but holds no tree is given up (`SI_edDisposeExpression`, not
/// ported) and its serial relation removed. Unless `SI_DO_NOT_REMOVE_ORPHANED_EXP` is set,
/// a non-root element without any relation is given up too.
pub fn expressions_regenerate(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>) {
    let keep = orphans_kept(m, o);
    let sp = m.sp();
    let lc = Ptr32::<i32>::new(m.local(4));
    let l8 = Ptr32::<i32>::new(m.local(4));
    let exp = rec::chap(m, cid::EXP);
    let list = m.u32(ed.addr() + 4).wrapping_add((exp as u32).wrapping_mul(0x40));
    let mut index = 0;
    while index < m.i32(list + 4) {
        let mut related = false;
        let e = m.u32(list + 8) + index as u32 * 0x84;
        if m.i16(e + 0x48) == 0 && m.i16(e + 0x46) == 1 {
            let mut g = 0;
            let mut serial = 0;
            loop {
                let mut t = si_dict::elemrel::find_related_elem(m, ed, Ptr32::new(e), 0, g, 0x4e2a, -1, lc, l8).addr();
                let next;
                if g == 4 {
                    next = 5;
                    if t != 0 {
                        related = true;
                    }
                } else {
                    let mut first = true;
                    next = g + 1;
                    while t != 0 {
                        let r = si_dict::elemrel::get_cur_relation(m, ed, Ptr32::new(e), 0, g).addr();
                        if first {
                            related = true;
                            serial = m.i32(r + 0xc);
                        }
                        first = false;
                        let back = si_dict::elemrel::get_cur_relation(m, ed, Ptr32::new(t), 1, m.i16(r + 0x12) as i32).addr();
                        let rel = si_dict::elemrel::set_rel_to_serialid(m, ed, Ptr32::new(t), 0x4e2a, m.i32(r + 0xc), 1);
                        let (c, i) = (m.read(lc), m.read(l8));
                        express_args_regenerate(m, o, ed, Ptr32::new(t), c, i, Ptr32::new(e), rel, 1);
                        if back != 0 {
                            si_dict::elemrel::set_rel_to_serialid(m, ed, Ptr32::new(t), 0x4e2a, m.i32(back + 0xc), 1);
                        }
                        t = si_dict::elemrel::find_next_related_elem(m, ed, Ptr32::new(e), 0, g, 0x4e2a, -1, lc, l8).addr();
                    }
                    if !first && m.u32(e + 4) == 0 {
                        o.ed_dispose_expression(m, index);
                        let (c, i) = (m.read(lc), m.read(l8));
                        si_dict::elemrel::remove_serial_rel_id(m, &mut super::calls::DictOutside(o), ed, Ptr32::NULL, c, i, 0x4e2a, serial);
                    }
                }
                g = next;
                if g >= 6 {
                    break;
                }
            }
            if !keep && m.i32(e + 0x54) != 0 && m.i16(e + 0x48) == 0 && m.i16(e + 0x46) == 1 && !related {
                o.ed_dispose_expression(m, index);
            }
        }
        index += 1;
    }
    m.set_sp(sp);
}
