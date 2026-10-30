// SPDX-License-Identifier: MIT
//! The evaluator (`exp_parsing.c`, `exp_fct.c`; finding 0013).
//!
//! An expression is evaluated by walking its tree [soft 0x0824bc5c si_evalExprExec]. Every
//! operand and every result is a 12-byte value record ([`AttrValue`]); numbers travel as
//! floats: each operator takes its operands into the x87 register, computes there and
//! stores the result to a `float`, so every node of the tree is a rounding point.

// adapter: not in place; the evaluator on value trees through `Scene`; the update's path is on the image in
// `crate::image::eval`. This form stays for the corpus and emulation checks and for the parser-side callers.

use crate::fct;
use crate::scene::{EvalUndefined, Scene};
use crate::token::{self, chapter};
use crate::tree::*;
use crate::value::AttrValue;
use crate::util::rt;
use si_x87::{Round, F80};
use std::sync::LazyLock;

/// Error codes (`SI_exprErrors`) with their texts (`exprErrorList`, 0x08699e20). The
/// evaluator raises only 4; 1 to 3 come from the cluster look-up of the parser; nothing in
/// the expression source files raises 5, 6 or 7 (the functions they name answer 0 for a bad
/// argument and have no way to report).
pub static ERROR_MESSAGES: LazyLock<[(i32, &str); 8]> = LazyLock::new(|| [
    (0, ""),
    (1, rt(0x08699d20)),
    (2, rt(0x08699d5f)),
    (3, rt(0x08699d76)),
    (4, rt(0x08699d8c)),
    (5, rt(0x08699dc0)),
    (6, rt(0x08699ddf)),
    (7, rt(0x08699df5)),
]);

/// The division-by-zero code.
pub const ERR_DIVIDE_BY_ZERO: i32 = 4;

/// The text of an error code [soft 0x08248eb0 SI_exprGetErrorMsg]; `None` (a null pointer)
/// for a code the list does not have.
pub fn get_error_msg(code: i32) -> Option<&'static str> {
    ERROR_MESSAGES.iter().find(|e| e.0 == code).map(|e| e.1)
}

/// Whether an operator is one of the five assignments [soft 0x0824b000 expNodeIsAssignOper].
pub fn node_is_assign_oper(oper: u32) -> bool {
    (1..6).contains(&(oper as i32))
}

/// The tree of the first variable called like a variable leaf [soft 0x0824b030 getVarExpr].
fn get_var_expr<'a>(expr: &'a Expression, name: &[u8]) -> Option<&'a ExprNode> {
    expr.vars.iter().find(|v| v.name == name).and_then(|v| v.expr.as_deref())
}

/// `SI_ExprRelData`, 0x20 bytes: what a relation of an expression carries besides the tree
/// [soft 0x0824b2f8 si_exprInitRelData].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExprRelData {
    /// +0x00: the target's fcurve type.
    pub fcv: i32,
    /// +0x04, +0x08, +0x0c: the target's access information (as in [`ArgNode`]).
    pub access0: i32,
    pub access1: i32,
    pub access_name: Option<Vec<u8>>,
    /// +0x10: the target's name for fcurve type 566.
    pub custom_name: Option<Vec<u8>>,
    /// +0x14: 0 for an expression with a target, 1 for a spreadsheet query (only the
    /// right-hand side is evaluated then).
    pub query: i16,
    /// +0x16: number of random calls.
    pub nrand: i16,
    /// +0x18: number of frames the tables were filled for.
    pub nframes: i32,
    /// +0x1c: per random call, a table with one value per frame; null for a call without a
    /// seed.
    pub values: Option<Vec<Option<Vec<f32>>>>,
}

/// The random calls of a tree in the order the original lists them
/// [soft 0x0824dfa0 getExprRandList]: left before right, function arguments in order; the
/// target of an assignment is not looked at.
fn collect_rand<'a>(node: Option<&'a ExprNode>, out: &mut Vec<&'a RandNode>) -> Result<(), EvalUndefined> {
    let Some(node) = node else { return Ok(()) };
    let oper = node.get_oper();
    if oper == op::LEAF {
        match node.leaf.as_deref().ok_or(EvalUndefined::NullPointer(1))? {
            NopNode { kind: nop::RAND, data: NopData::Rand(r) } => out.push(r),
            NopNode { kind: nop::FCT, data: NopData::Fct(f) } => {
                for a in &f.args {
                    collect_rand(a.as_deref(), out)?;
                }
            }
            _ => {}
        }
    } else {
        if !node_is_assign_oper(oper) {
            collect_rand(node.left.as_deref(), out)?;
        }
        collect_rand(node.right.as_deref(), out)?;
    }
    Ok(())
}

/// The random calls of an expression [soft 0x0824e0ac si_getExprRandListNode]: those of the
/// variables' definitions first, then those of the right-hand side.
pub fn get_expr_rand_list(expr: &Expression) -> Result<Vec<&RandNode>, EvalUndefined> {
    let mut out = Vec::new();
    for v in &expr.vars {
        collect_rand(v.expr.as_deref(), &mut out)?;
    }
    if let Some(root) = expr.root.as_deref() {
        collect_rand(root.right.as_deref(), &mut out)?;
    }
    Ok(out)
}

/// Fills the random tables [soft 0x08249134 SI_exprGenRandomValues]: for every random call
/// with a seed (zero or above), the generator is seeded with it and one value is drawn per
/// frame. A call without a seed gets no table and draws afresh at each evaluation.
///
/// The tables are filed by the call's place in [`get_expr_rand_list`], while the evaluator
/// looks a table up by the number the parser gave the call, which starts again at 0 for
/// each variable and for the main text. With random calls in more than one of them the two
/// numberings disagree: a call can read the table of another.
pub fn gen_random_values(s: &mut dyn Scene, data: &mut ExprRelData, nframes: i32, expr: &Expression) -> Result<(), EvalUndefined> {
    let list = get_expr_rand_list(expr)?;
    if list.is_empty() {
        return Ok(());
    }
    // the table of tables is allocated once, for the count of that time
    let values = data.values.get_or_insert_with(|| vec![None; list.len()]);
    data.nrand = list.len() as i16;
    data.nframes = nframes;
    for (i, r) in list.iter().enumerate() {
        if r.seed >= 0 {
            let slot = values.get_mut(i).ok_or(EvalUndefined::RandomTable)?;
            s.srand(r.seed as u32);
            let mut table = Vec::new();
            for _ in 0..nframes.max(0) {
                table.push(fct::random_eval(s, r).to_f32());
            }
            *slot = Some(table);
        }
    }
    Ok(())
}

/// A fresh relation record for an expression [soft 0x0824b2f8 si_exprInitRelData]: the
/// random tables, and unless it is for a query the target's fcurve type, access information
/// and custom name.
pub fn init_rel_data(s: &mut dyn Scene, expr: &Expression, query: i16, nframes: i32) -> Result<ExprRelData, EvalUndefined> {
    let mut data = ExprRelData::default();
    gen_random_values(s, &mut data, nframes, expr)?;
    if query == 0 {
        if let Some(root) = expr.root.as_deref() {
            // a statement without a target faults here in the original
            let leaf = root.left.as_deref().and_then(|t| t.leaf.as_deref()).ok_or(EvalUndefined::NullPointer(2))?;
            let NopData::Arg(a) = &leaf.data else { return Err(EvalUndefined::NullPointer(3)) };
            data.fcv = a.fcv;
            data.access0 = a.access0;
            data.access1 = a.access1;
            data.access_name = a.access_name.clone();
            if a.fcv == FCV_CUSTOM {
                data.custom_name = Some(a.custom_name.clone().ok_or(EvalUndefined::NullPointer(4))?);
            }
        }
    }
    data.query = query;
    Ok(data)
}

/// Frees a relation record [soft 0x08249c74 SI_exprRelDataDispose]. In Rust the record
/// drops; this is here for the name.
pub fn rel_data_dispose(data: ExprRelData) {
    drop(data);
}

/// What one evaluation needs besides the tree.
pub struct EvalArgs<'a> {
    /// The random tables of the relation (`SI_ExprRelData` +0x1c).
    pub rand_values: Option<&'a [Option<Vec<f32>>]>,
    /// The index into those tables: the current frame minus the first frame.
    pub frame_index: i32,
    /// The current frame (the value of `Fc`).
    pub frame: i32,
    /// The current time in seconds (the value of `T`).
    pub time: f32,
}

fn float(v: F80) -> AttrValue<'static> {
    AttrValue::float(v.to_f32())
}

/// Evaluates a tree into a value record [soft 0x0824bc5c si_evalExprExec]. `err` receives 4
/// on a division by zero (it is not cleared here).
pub fn eval_expr_exec<'a>(
    s: &mut dyn Scene,
    expr: &'a Expression,
    node: &'a ExprNode,
    args: &EvalArgs,
    err: &mut i32,
) -> Result<AttrValue<'a>, EvalUndefined> {
    let oper = node.get_oper();
    if oper == op::LEAF {
        return eval_leaf(s, expr, node.leaf.as_deref().ok_or(EvalUndefined::NullPointer(5))?, args, err);
    }
    // the operands start as the float 0
    let mut left: AttrValue = AttrValue::float(0.0);
    let mut right: AttrValue = AttrValue::float(0.0);
    let mut target: Option<(&NopNode, &ArgNode, bool)> = None;
    if !node_is_assign_oper(oper) {
        // (the original does not initialise the left operand's record: a leaf that writes
        // nothing would leave it undefined; every leaf below writes)
        left = eval_expr_exec(s, expr, node.left.as_deref().ok_or(EvalUndefined::NullPointer(6))?, args, err)?;
    } else if let Some(t) = node.left.as_deref() {
        // an assignment: the left operand is the target's current value
        let leaf = t.leaf.as_deref().ok_or(EvalUndefined::NullPointer(7))?;
        let NopData::Arg(a) = &leaf.data else { return Err(EvalUndefined::NullPointer(8)) };
        let exists = s.elem_exists(a.chapter, a.elem);
        target = Some((leaf, a, exists));
        if exists {
            let mut v = AttrValue::float(0.0);
            if leaf.kind == nop::CUSTOM_ARG {
                s.static_value_get(a, None, &mut v);
            } else {
                s.att_value_get(a, &mut v);
            }
            left = v;
        }
    }
    if let Some(r) = node.right.as_deref() {
        // `&&` and `||` look at the left operand first
        if oper == op::AND {
            if left.in_float().eq(F80::ZERO) {
                return Ok(AttrValue::float(0.0));
            }
        } else if oper == op::OR && !left.in_float().eq(F80::ZERO) {
            return Ok(AttrValue::float(1.0));
        }
        right = eval_expr_exec(s, expr, r, args, err)?;
    }
    let (lk, rk) = (left.kind(), right.kind());
    if lk == 5 || rk == 5 || lk == 3 || rk == 3 {
        // strings and enumeration values compare with their own kind only; a mixed pair
        // gives the float 0
        if lk != rk {
            return Ok(AttrValue::float(0.0));
        }
        if lk == 5 {
            let (AttrValue::Str(a), AttrValue::Str(b)) = (&left, &right) else { unreachable!() };
            let cmp: i32 = match (a, b) {
                (None, None) => 0,
                (None, Some(_)) => -1,
                (Some(_), None) => 1,
                (Some(a), Some(b)) => match a.cmp(b) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                },
            };
            let yes = match oper {
                // LIKE: the right string occurs in the left one (`strstr`)
                op::CONTAINS => {
                    let (Some(a), Some(b)) = (a, b) else { return Err(EvalUndefined::NullPointer(9)) };
                    b.is_empty() || a.windows(b.len()).any(|w| w == &b[..])
                }
                op::EQ => cmp == 0,
                op::NE => cmp != 0,
                op::LT => cmp < 0,
                op::LE => cmp <= 0,
                op::GT => cmp > 0,
                op::GE => cmp >= 0,
                _ => false,
            };
            return Ok(AttrValue::float(if yes { 1.0 } else { 0.0 }));
        }
        // two enumeration values: equal if both words agree, or if either is the value
        // `ALL` (−1, −1). `!=` negates; every other operator answers the equality.
        let (AttrValue::Raw { w1: a1, w2: a2, .. }, AttrValue::Raw { w1: b1, w2: b2, .. }) = (&left, &right) else {
            unreachable!()
        };
        let all = |w1: u32, w2: u32| w1 == u32::MAX && w2 == u32::MAX;
        let equal = (a1 == b1 && a2 == b2) || all(*a1, *a2) || all(*b1, *b2);
        let yes = if oper == op::NE { !equal } else { equal };
        return Ok(AttrValue::float(if yes { 1.0 } else { 0.0 }));
    }
    let l = left.in_float();
    let r = right.in_float();
    let truth = |b: bool| if b { F80::ONE } else { F80::ZERO };
    let result = match oper {
        op::ASSIGN..=op::DIV_ASSIGN => {
            let (leaf, a, exists) = match target {
                Some(t) => t,
                // an assignment without a target (a text that starts with `=`): the
                // original reads the target through an unset pointer
                None => return Err(EvalUndefined::NullPointer(10)),
            };
            if a.chapter == chapter::CHANNEL_DRIVERS {
                if !exists {
                    return Err(EvalUndefined::NullPointer(11));
                }
                s.chn_set_value(a, r.to_f32());
                r
            } else if !exists {
                // a missing element: nothing is written, the value is the right operand
                r
            } else {
                let v = match oper {
                    op::ADD_ASSIGN => l + r,
                    op::SUB_ASSIGN => l - r,
                    op::MUL_ASSIGN => l * r,
                    op::DIV_ASSIGN => {
                        if F80::ZERO.eq(r) {
                            *err = ERR_DIVIDE_BY_ZERO;
                            r
                        } else {
                            l / r
                        }
                    }
                    _ => r,
                };
                let custom = leaf.kind == nop::CUSTOM_ARG;
                s.static_value_set(a, if custom { None } else { Some(1) }, v.to_f32(), if custom { a.custom_name.as_deref() } else { None });
                v
            }
        }
        op::ADD => l + r,
        op::SUB => l - r,
        op::MUL => l * r,
        op::DIV => {
            if F80::ZERO.eq(r) {
                *err = ERR_DIVIDE_BY_ZERO;
                l
            } else {
                l / r
            }
        }
        op::MOD => {
            if F80::ZERO.eq(r) {
                *err = ERR_DIVIDE_BY_ZERO;
                l
            } else {
                // both operands cut to integers, then the integer remainder
                let (a, b) = (l.to_i32(Round::Zero), r.to_i32(Round::Zero));
                if b == 0 || (a == i32::MIN && b == -1) {
                    return Err(EvalUndefined::IntegerDivide);
                }
                F80::from_i32(a % b)
            }
        }
        op::NEG => -l,
        op::NOT => truth(F80::ZERO.eq(l)),
        op::EQ => truth(l.eq(r)),
        // an unordered pair (a NaN) is "not equal"
        op::NE => truth(!l.eq(r)),
        op::LT => truth(l.lt(r)),
        op::LE => truth(l.lt(r) || l.eq(r)),
        op::GT => truth(l.gt(r)),
        op::GE => truth(l.gt(r) || l.eq(r)),
        op::AND => truth(!F80::ZERO.eq(l) && !F80::ZERO.eq(r)),
        op::OR => truth(!F80::ZERO.eq(l) || !F80::ZERO.eq(r)),
        _ => {
            // LIKE between numbers, or an operator code the evaluator does not know
            s.print("Unknown oper...\n");
            l
        }
    };
    Ok(float(result))
}

fn eval_leaf<'a>(
    s: &mut dyn Scene,
    expr: &'a Expression,
    leaf: &'a NopNode,
    args: &EvalArgs,
    err: &mut i32,
) -> Result<AttrValue<'a>, EvalUndefined> {
    Ok(match (leaf.kind, &leaf.data) {
        (nop::INT | nop::RELATION, NopData::Int(v)) => AttrValue::int(*v),
        (nop::FLOAT, NopData::Float(v)) => AttrValue::float(*v),
        (nop::STRING, NopData::Text(t)) => AttrValue::Str(t.clone()),
        (nop::ENUM, NopData::Enum(e)) => AttrValue::Raw { kind: 3, w1: e.enum_type as u32, w2: e.value as u32 },
        // the frame numbers become floats
        (nop::PREDEF_4, _) => AttrValue::float(s.frame_start() as f32),
        (nop::PREDEF_5, _) => AttrValue::float(s.frame_end() as f32),
        (nop::PREDEF_6, _) => AttrValue::float(args.frame as f32),
        (nop::PREDEF_7, _) => AttrValue::float(args.time),
        (nop::RAND, NopData::Rand(r)) => {
            let table = args.rand_values.and_then(|t| t.get(r.index as usize)).and_then(|t| t.as_ref());
            match table {
                Some(t) if r.seed >= 0 => {
                    AttrValue::float(*t.get(args.frame_index as usize).ok_or(EvalUndefined::RandomTable)?)
                }
                _ => float(fct::random_eval(s, r)),
            }
        }
        (nop::ARG | nop::CUSTOM_ARG, NopData::Arg(a)) => {
            let mut v = AttrValue::float(0.0);
            if s.elem_exists(a.chapter, a.elem) {
                if a.chapter == chapter::CHANNEL_DRIVERS {
                    v = float(s.chn_get_value(a, args.time));
                } else if s.query_mode() == 1 {
                    if leaf.kind != nop::CUSTOM_ARG {
                        s.att_value_get(a, &mut v);
                    }
                } else {
                    let custom = if leaf.kind == nop::CUSTOM_ARG { a.custom_name.as_deref() } else { None };
                    s.static_value_get(a, custom, &mut v);
                }
            }
            v
        }
        (nop::FCT, NopData::Fct(f)) => {
            let entry = token::get_fct_ptr(f.index).ok_or(EvalUndefined::UnknownFunction(f.index))?;
            if f.args.is_empty() {
                // a call without arguments is not made at all
                return Ok(AttrValue::float(0.0));
            }
            let mut vals: Vec<AttrValue<'a>> = Vec::with_capacity(f.args.len());
            for (i, a) in f.args.iter().enumerate() {
                let a = a.as_deref().ok_or(EvalUndefined::NullPointer(12))?;
                let kind = if entry.nargs == -1 { entry.arg_kinds[0] } else { *entry.arg_kinds.get(i).unwrap_or(&1) };
                vals.push(if kind == 10 { AttrValue::Node(a) } else { eval_expr_exec(s, expr, a, args, err)? });
            }
            let f_ptr = fct::builtin_by_address(entry.address).expect("every table entry has its function");
            float(f_ptr(s, vals.len() as u32, &vals)?)
        }
        (nop::VAR, NopData::Text(name)) => {
            let name = name.as_deref().ok_or(EvalUndefined::NullPointer(13))?;
            match get_var_expr(expr, name) {
                Some(tree) => eval_expr_exec(s, expr, tree, args, err)?,
                None => AttrValue::float(0.0),
            }
        }
        (nop::UNUSED_12, _) => {
            s.print(rt(0x08699f1c));
            AttrValue::float(0.0)
        }
        // PI and E are floats
        (nop::PREDEF_14, _) => AttrValue::float(f32::from_bits(0x4049_0fdb)),
        (nop::PREDEF_15, _) => AttrValue::float(f32::from_bits(0x402d_f854)),
        // TRUE, LinearInterp, FORWARD
        (nop::PREDEF_16 | nop::PREDEF_18 | nop::PREDEF_21, _) => AttrValue::int(1),
        // FALSE, BACKWARD
        (nop::PREDEF_17 | nop::PREDEF_22, _) => AttrValue::int(0),
        // SplineInterp
        (nop::PREDEF_19, _) => AttrValue::int(2),
        _ => {
            s.print(rt(0x08699f34));
            AttrValue::float(0.0)
        }
    })
}

/// The value of an expression at a frame and a time [soft 0x0824924c SI_expressionEvaluate]:
/// the whole statement (which writes the target), or only the right-hand side for a query;
/// the result as a float and the error code (0, or 4 after a division by zero).
pub fn expression_evaluate(
    s: &mut dyn Scene,
    expr: &Expression,
    data: &ExprRelData,
    frame_index: i32,
    frame: i32,
    time: f32,
) -> Result<(f32, i32), EvalUndefined> {
    let mut err = 0;
    let root = expr.root.as_deref().ok_or(EvalUndefined::NullPointer(14))?;
    let node = if data.query == 0 { root } else { root.right.as_deref().ok_or(EvalUndefined::NullPointer(15))? };
    let args = EvalArgs { rand_values: data.values.as_deref(), frame_index, frame, time };
    let v = eval_expr_exec(s, expr, node, &args, &mut err)?;
    Ok((v.in_float().to_f32(), err))
}
