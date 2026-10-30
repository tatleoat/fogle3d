// SPDX-License-Identifier: MIT
//! Trees between the value form and the image (finding 0013, section 12).
//!
//! The parser is not on the image yet: it builds a value-form tree ([`crate::tree`]), and
//! [`put_expression`] lays that tree into the image record by record with the allocation
//! calls of the parser's actions (`AllocExprNode`, `AllocExpr`, `addAnArgToFct`). The
//! order of those calls is the order of a walk of the finished tree, not the order in
//! which the parser's reductions make them, and the variable array and names are allocated
//! as plainly as possible.
//!
//! [`get_expression`] reads a tree of the image back into the value form, for the checks.

// adapter: not in place; allocation calls not reproduced in the parser's order

use super::rec::{ExprNode, Expression};
use crate::tree::{self as v, nop, NopData};
use si_mem::{Mem, Ptr32};

fn put_text(m: &mut Mem, t: &Option<Vec<u8>>) -> u32 {
    match t {
        None => 0,
        Some(s) => {
            let p = m.malloc(s.len() as u32 + 1);
            m.put_cstr(p, s);
            p
        }
    }
}

/// A leaf record (`AllocExpr(kind)`: `SI_calloc(1, 8)`, and the kind's record).
fn put_leaf(m: &mut Mem, n: &v::NopNode) -> u32 {
    let leaf = m.si_calloc(1, 8);
    m.set_i32(leaf, n.kind);
    let data = match &n.data {
        NopData::None => 0,
        NopData::Int(i) => *i as u32,
        NopData::Float(f) => f.to_bits(),
        NopData::Text(t) => put_text(m, t),
        NopData::Enum(e) => {
            let p = m.si_calloc(1, 8);
            m.set_i32(p, e.enum_type);
            m.set_i32(p + 4, e.value);
            p
        }
        NopData::Channel(c) => {
            let p = m.si_calloc(1, 8);
            m.set_i32(p, c.driver);
            m.set_i32(p + 4, c.dof);
            p
        }
        NopData::Rand(r) => {
            let p = m.si_calloc(1, 0x18);
            m.set_i32(p, r.form);
            m.set_i16(p + 4, r.index);
            m.set_i32(p + 8, r.seed);
            m.set_i16(p + 0xc, r.real);
            m.set_f32(p + 0x10, r.low);
            m.set_f32(p + 0x14, r.width);
            p
        }
        NopData::Arg(a) => {
            let p = m.si_calloc(1, 0x24);
            m.set_i32(p, a.num);
            m.set_i16(p + 4, a.generic);
            m.set_i32(p + 8, a.chapter);
            m.set_i32(p + 0xc, a.elem);
            m.set_i32(p + 0x10, a.fcv);
            m.set_i32(p + 0x14, a.access0);
            m.set_i32(p + 0x18, a.access1);
            let an = put_text(m, &a.access_name);
            m.set_u32(p + 0x1c, an);
            let cn = put_text(m, &a.custom_name);
            m.set_u32(p + 0x20, cn);
            p
        }
        NopData::Fct(f) => {
            let p = m.si_calloc(1, 0xc);
            m.set_i32(p, f.index);
            for a in &f.args {
                let t = match a {
                    Some(t) => put_node(m, t),
                    None => 0,
                };
                // addAnArgToFct: room for five more every fifth argument
                let k = m.i32(p + 4);
                if k % 5 == 0 {
                    let arr = if k == 0 { m.si_calloc(5, 4) } else { m.si_realloc(m.u32(p + 8), k as u32 * 4 + 0x14) };
                    m.set_u32(p + 8, arr);
                }
                m.set_u32(m.u32(p + 8) + 4 * k as u32, t);
                m.set_i32(p + 4, k + 1);
            }
            p
        }
    };
    m.set_u32(leaf + 4, data);
    leaf
}

/// A tree, operands first (`AllocExprNode`: `SI_calloc(1, 0xc)`).
pub fn put_node(m: &mut Mem, n: &v::ExprNode) -> u32 {
    let (a, b) = if n.get_oper() == v::op::LEAF {
        let leaf = n.leaf.as_deref().map_or(0, |l| put_leaf(m, l));
        (leaf, 0)
    } else {
        let l = n.left.as_deref().map_or(0, |t| put_node(m, t));
        let r = n.right.as_deref().map_or(0, |t| put_node(m, t));
        (l, r)
    };
    let p = m.si_calloc(1, 0xc);
    m.set_u32(p, n.oper);
    m.set_u32(p + 4, a);
    m.set_u32(p + 8, b);
    p
}

/// An expression into the image: its variables (array `SI_calloc(n, 8)`, names copied),
/// its statement, the record (`SI_calloc(1, 0xc)`).
pub fn put_expression(m: &mut Mem, e: &v::Expression) -> Ptr32<Expression> {
    let vars = if e.vars.is_empty() { 0 } else { m.si_calloc(e.vars.len() as u32, 8) };
    for (i, var) in e.vars.iter().enumerate() {
        let name = put_text(m, &Some(var.name.clone()));
        let t = var.expr.as_deref().map_or(0, |t| put_node(m, t));
        m.set_u32(vars + 8 * i as u32, name);
        m.set_u32(vars + 8 * i as u32 + 4, t);
    }
    let root = e.root.as_deref().map_or(0, |t| put_node(m, t));
    let p = m.si_calloc(1, 0xc);
    m.set_u32(p, root);
    m.set_i32(p + 4, e.vars.len() as i32);
    m.set_u32(p + 8, vars);
    Ptr32::new(p)
}

fn get_text(m: &Mem, p: u32) -> Option<Vec<u8>> {
    if p == 0 {
        None
    } else {
        Some(m.cstr(p))
    }
}

/// A tree of the image in the value form (a null operand is `None`). Depth is bounded so
/// that a cycle in damaged memory cannot run for ever.
pub fn get_node(m: &Mem, p: Ptr32<ExprNode>, depth: u32) -> Option<Box<v::ExprNode>> {
    if p.is_null() || depth > 10_000 {
        return None;
    }
    let a = p.addr();
    let oper = m.u32(a);
    if oper & 0xffff_ff7f == v::op::LEAF {
        let leaf = m.u32(a + 4);
        let l = if leaf == 0 {
            None
        } else {
            let kind = m.i32(leaf);
            let d = m.u32(leaf + 4);
            let data = match kind {
                nop::INT | nop::RELATION => NopData::Int(d as i32),
                nop::FLOAT => NopData::Float(f32::from_bits(d)),
                nop::STRING | nop::VAR => NopData::Text(get_text(m, d)),
                nop::ENUM => NopData::Enum(v::EnumNode { enum_type: m.i32(d), value: m.i32(d + 4) }),
                nop::CHANNEL => NopData::Channel(v::ChannelNode { driver: m.i32(d), dof: m.i32(d + 4) }),
                nop::RAND => NopData::Rand(v::RandNode {
                    form: m.i32(d),
                    index: m.i16(d + 4),
                    seed: m.i32(d + 8),
                    real: m.i16(d + 0xc),
                    low: m.f32(d + 0x10),
                    width: m.f32(d + 0x14),
                }),
                nop::ARG | nop::CUSTOM_ARG => NopData::Arg(v::ArgNode {
                    num: m.i32(d),
                    generic: m.i16(d + 4),
                    chapter: m.i32(d + 8),
                    elem: m.i32(d + 0xc),
                    fcv: m.i32(d + 0x10),
                    access0: m.i32(d + 0x14),
                    access1: m.i32(d + 0x18),
                    access_name: get_text(m, m.u32(d + 0x1c)),
                    custom_name: get_text(m, m.u32(d + 0x20)),
                }),
                nop::FCT => {
                    let n = m.i32(d + 4).clamp(0, 1000);
                    let arr = m.u32(d + 8);
                    NopData::Fct(v::FctNode {
                        index: m.i32(d),
                        args: (0..n).map(|i| get_node(m, Ptr32::new(m.u32(arr + 4 * i as u32)), depth + 1)).collect(),
                    })
                }
                _ => NopData::None,
            };
            Some(Box::new(v::NopNode { kind, data }))
        };
        return Some(Box::new(v::ExprNode { oper, leaf: l, left: None, right: None }));
    }
    let l = get_node(m, Ptr32::new(m.u32(a + 4)), depth + 1);
    let r = get_node(m, Ptr32::new(m.u32(a + 8)), depth + 1);
    Some(Box::new(v::ExprNode { oper, leaf: None, left: l, right: r }))
}

/// An expression of the image in the value form.
pub fn get_expression(m: &Mem, p: Ptr32<Expression>) -> v::Expression {
    let a = p.addr();
    let n = m.i32(a + 4).clamp(0, 10_000);
    let vars = m.u32(a + 8);
    v::Expression {
        root: get_node(m, Ptr32::new(m.u32(a)), 0),
        vars: (0..n)
            .map(|i| v::VarNode {
                name: m.cstr(m.u32(vars + 8 * i as u32)),
                expr: get_node(m, Ptr32::new(m.u32(vars + 8 * i as u32 + 4)), 0),
            })
            .collect(),
    }
}
