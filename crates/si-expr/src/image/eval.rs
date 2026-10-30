// SPDX-License-Identifier: MIT
//! The evaluator on the image (`exp_parsing.c`, `exp_fct.c`; finding 0013, sections 8 and
//! 12): the update's call `SI_animExpressionEvaluate(node, time)` and everything under it.
//!
//! The algorithm is the value form's ([`crate::eval`]); here every operand is a value
//! record (`SI_AttributeValue`, 12 bytes) in the image: the two operand records of a node
//! are locals of the image's stack, the arguments of a function call are the evaluator's
//! own arrays on the heap (one per level of nested calls, kept between calls in four
//! statics and grown with `SI_calloc`/`SI_realloc` as the original grows them), and a
//! parameter is read and written through the element's object ([`super::elem`]).

use super::calls::ExprCalls;
use super::elem;
use super::rec::{self, cid, ExprNode, ExprRelData, Expression, NopNode, RandNode, VarNode};
use crate::scene::{EvalUndefined, Scene};
use crate::tree::{nop, op};
use crate::value::AttrValue;
use si_dict::globals::SI_ATD;
use si_mem::{Mem, Outside, Ptr32};
use si_types::dict::{AttGrpIndex, AttributeValue, ElemIdentity};
use si_x87::{Round, F80};

pub type Result<T> = core::result::Result<T, EvalUndefined>;

/// The float 1.0 as the evaluator stores it.
const ONE: u32 = 0x3f80_0000;

/// The value of a value record in the register [soft 0x080f0b18 SI_getAttrValInFloat]:
/// type 1 the float at +4, 2 the short at +4, 4 the integer at +4, 9 the float at +8,
/// anything else 0.
pub fn get_attr_val_in_float(m: &Mem, v: Ptr32<AttributeValue>) -> F80 {
    let a = v.addr();
    match m.i32(a) {
        1 => F80::from_f32(m.f32(a + 4)),
        2 => F80::from_i32(m.i16(a + 4) as i32),
        4 => F80::from_i32(m.i32(a + 4)),
        9 => F80::from_f32(m.f32(a + 8)),
        _ => F80::ZERO,
    }
}

/// A value record as an integer [soft 0x080f0b68 SI_getAttrValInInt]: as the float form,
/// the two float types cut toward zero.
pub fn get_attr_val_in_int(m: &Mem, v: Ptr32<AttributeValue>) -> i32 {
    let a = v.addr();
    match m.i32(a) {
        1 => F80::from_f32(m.f32(a + 4)).to_i32(Round::Zero),
        2 => m.i16(a + 4) as i32,
        4 => m.i32(a + 4),
        9 => F80::from_f32(m.f32(a + 8)).to_i32(Round::Zero),
        _ => 0,
    }
}

/// The address of the text of an error code, 0 for a code the list does not have
/// [soft 0x08248eb0 SI_exprGetErrorMsg]: the first of `errorMsgSize` pairs of
/// `exprErrorList` with that code.
pub fn expr_get_error_msg(m: &Mem, code: i32) -> u32 {
    let n = m[rec::ERROR_MSG_SIZE];
    let mut i: u32 = 0;
    while ((i & 0xffff) as i32) < n {
        let at = rec::EXPR_ERROR_LIST + (i & 0xffff) * 8;
        if m.i32(at) == code {
            return m.u32(at + 4);
        }
        i += 1;
    }
    0
}

/// The operator of a node without the parenthesis bit [soft 0x0824b020 SI_expGetOper].
pub fn exp_get_oper(m: &Mem, node: Ptr32<ExprNode>) -> u32 {
    m.u32(node.addr()) & 0xffff_ff7f
}

/// The tree of the first variable whose name equals a variable leaf's
/// [soft 0x0824b030 getVarExpr]; null if none.
pub fn get_var_expr(m: &Mem, leaf: Ptr32<NopNode>, nvars: i32, vars: Ptr32<VarNode>) -> Ptr32<ExprNode> {
    if nvars != 0 {
        let name = m.u32(leaf.addr() + 4);
        let mut i = 0;
        while i < nvars {
            let v = vars.add(i);
            if si_mem::clib::strcmp(m, name, m.u32(v.addr())) == 0 {
                return Ptr32::new(m.u32(v.addr() + 4));
            }
            i += 1;
        }
    }
    Ptr32::NULL
}

/// The scene a value-form built-in function sees from the image: the frame globals, the
/// C library's generator through the outside world, the printed text.
pub struct ImageScene<'a> {
    pub m: &'a mut Mem,
    pub o: &'a mut dyn Outside,
}

impl Scene for ImageScene<'_> {
    fn frame_start(&self) -> i32 {
        self.m[rec::SI_FRAME_START]
    }
    fn frame_end(&self) -> i32 {
        self.m[rec::SI_FRAME_END]
    }
    fn frame_current(&self) -> i32 {
        self.m[rec::SI_FRAME_CURRENT]
    }
    fn query_mode(&self) -> i16 {
        self.m[rec::SI_QUERY_MODE]
    }
    fn rand(&mut self) -> i32 {
        self.o.rand(self.m)
    }
    fn srand(&mut self, seed: u32) {
        self.o.srand(self.m, seed)
    }
    fn print(&mut self, text: &str) {
        self.o.print(1, text.as_bytes())
    }
}

/// A random record as the value form's.
fn rand_node(m: &Mem, r: Ptr32<RandNode>) -> crate::tree::RandNode {
    let a = r.addr();
    crate::tree::RandNode {
        form: m.i32(a),
        index: m.i16(a + 4),
        seed: m.i32(a + 8),
        real: m.i16(a + 0xc),
        low: m.f32(a + 0x10),
        width: m.f32(a + 0x14),
    }
}

/// One random number [soft 0x082490f0 si_exprRandomEval] on the record in the image:
/// rand() × width × 2⁻³¹ + low in the register; through a double, `rint` and a float for
/// an integer-valued call. The C library's `rand` is the outside world's.
pub fn expr_random_eval(m: &mut Mem, o: &mut dyn Outside, r: Ptr32<RandNode>) -> F80 {
    let node = rand_node(m, r);
    crate::fct::random_eval(&mut ImageScene { m, o }, &node)
}

/// The functions of the table that reach the scene (elements' curves, centres, the
/// camera, the texture noise, relations): not on the image, called by their address.
const SCENE_FUNCTIONS: [u32; 7] = [0x0824a214, 0x0824a2fc, 0x0824a3e0, 0x0824a5fc, 0x0824a720, 0x0824ad6c, 0x0824ae44];

/// Calls a function of `math_fct_array` on `nargs` value records at `args`. The value
/// form's function computes; it is handed the records as they lie in the image (a string
/// record's text read from it).
fn call_builtin(m: &mut Mem, o: &mut dyn Outside, address: u32, nargs: u32, args: u32) -> Result<F80> {
    if SCENE_FUNCTIONS.contains(&address) {
        let r = o.call(m, si_mem::Routine::at(address), &[nargs, args]);
        return Ok(r.st0.map_or(F80::ZERO, |b| {
            F80::from_bits(u64::from_le_bytes(b[..8].try_into().unwrap()), u16::from_le_bytes([b[8], b[9]]))
        }));
    }
    let mut vals: Vec<AttrValue<'static>> = Vec::with_capacity(nargs as usize);
    for i in 0..nargs {
        let a = args + 12 * i;
        let kind = m.i32(a);
        vals.push(if kind == 5 {
            let p = m.u32(a + 4);
            AttrValue::Str(if p == 0 { None } else { Some(m.cstr(p)) })
        } else {
            AttrValue::Raw { kind, w1: m.u32(a + 4), w2: m.u32(a + 8) }
        });
    }
    let f = crate::fct::builtin_by_address(address).ok_or(EvalUndefined::UnknownFunction(-1))?;
    f(&mut ImageScene { m, o }, nargs, &vals)
}

/// Where a value record of the evaluator goes: its type, +4, +8.
fn put(m: &mut Mem, v: u32, kind: u32, w1: u32, w2: u32) {
    m.set_u32(v, kind);
    m.set_u32(v + 4, w1);
    m.set_u32(v + 8, w2);
}

/// What one evaluation needs besides the tree: the relation's random tables (a `float **`),
/// the index into them, the frame and the time.
#[derive(Clone, Copy, Debug)]
pub struct EvalArgs {
    pub rand_values: u32,
    pub frame_index: i32,
    pub frame: i32,
    pub time: f32,
}

/// Evaluates a tree into the value record `out` [soft 0x0824bc5c si_evalExprExec]. `err`
/// receives 4 on a division by zero (it is not cleared here). The original returns a
/// pointer nobody reads.
///
/// Where the original reads a word of its stack that nothing wrote (the third word of a
/// leaf's record, which goes to `out` + 8; the operands of an assignment whose target
/// element is missing), the port's local holds what the image's stack held there.
pub fn eval_expr_exec(m: &mut Mem, o: &mut dyn Outside, expr: Ptr32<Expression>, node: Ptr32<ExprNode>, out: Ptr32<AttributeValue>, a: EvalArgs, err: Ptr32<i32>) -> Result<()> {
    let sp = m.sp();
    let r = eval_inner(m, o, expr, node, out.addr(), a, err.addr());
    m.set_sp(sp);
    r
}

fn eval_inner(m: &mut Mem, o: &mut dyn Outside, expr: Ptr32<Expression>, node: Ptr32<ExprNode>, out: u32, a: EvalArgs, err: u32) -> Result<()> {
    let oper = exp_get_oper(m, node);
    // the frame's locals: the left operand (local_10, local_c, local_8), the right one
    // (local_1c, local_18, local_14), an attribute path and an element identity, the record
    // an assignment writes
    let left = m.local(12);
    let right = m.local(12);
    let path = Ptr32::<AttGrpIndex>::new(m.local(0x14));
    let ident = Ptr32::<ElemIdentity>::new(m.local(0x10));
    let set = Ptr32::<AttributeValue>::new(m.local(12));
    let ed = rec::cur_dict(m);
    if oper != op::LEAF {
        m.set_u32(out + 4, 0);
        m.set_u32(out, 1);
        m.set_u32(right + 4, 0);
        m.set_u32(right, 1);
        let mut target: Option<(u32, u32, u32)> = None; // (leaf, argument record, element)
        if !crate::eval::node_is_assign_oper(oper) {
            let l = Ptr32::new(m.u32(node.addr() + 4));
            eval_inner(m, o, expr, l, left, a, err)?;
        } else if m.u32(node.addr() + 4) != 0 {
            // the left operand is the target's value; where the original leaves it unwritten
            // (no such element, or no attribute path for the parameter) the value form's
            // reading, the float 0, is kept
            put(m, left, 1, 0, m.u32(left + 8));
            let leaf = m.u32(m.u32(node.addr() + 4) + 4);
            let arg = m.u32(leaf + 4);
            let e = si_dict::elemdict::get_elem(m, ed, m.i32(arg + 8), m.i32(arg + 0xc));
            target = Some((leaf, arg, e.addr()));
            if !e.is_null() {
                if m.i32(leaf) == nop::CUSTOM_ARG {
                    m.set_u32(left + 4, 0);
                    m.set_u32(left, 1);
                    elem::ed_element_static_value_get(m, o, e, m.i32(arg + 8), m.i32(arg + 0x10), Ptr32::new(arg + 0x14), Ptr32::new(left), 0);
                } else if si_dict::attdict::get_grp_index_from_fcurve(m, SI_ATD, m.i32(arg + 0x10), path) != 0 {
                    let (c, i) = (m.i32(arg + 8), m.i32(arg + 0xc));
                    m.set_i32(ident.addr(), c);
                    m.set_i32(ident.addr() + 4, i);
                    m.set_u32(left + 4, 0);
                    m.set_u32(left, 1);
                    elem::ed_element_att_value_get(m, o, ident, path, Ptr32::new(left));
                }
            }
        } else {
            // an assignment without a target: the original reads through a pointer it never
            // set
            return Err(EvalUndefined::NullPointer(10));
        }
        let r = m.u32(node.addr() + 8);
        if r != 0 {
            if oper == op::AND {
                if get_attr_val_in_float(m, Ptr32::new(left)).eq(F80::ZERO) {
                    m.set_f32(out + 4, 0.0);
                    m.set_u32(out, 1);
                    return Ok(());
                }
            } else if oper == op::OR && !get_attr_val_in_float(m, Ptr32::new(left)).eq(F80::ZERO) {
                m.set_u32(out + 4, ONE);
                m.set_u32(out, 1);
                return Ok(());
            }
            eval_inner(m, o, expr, Ptr32::new(r), right, a, err)?;
        }
        let (lk, rk) = (m.i32(left), m.i32(right));
        if lk == 5 || rk == 5 || lk == 3 || rk == 3 {
            if lk != rk {
                return Ok(());
            }
            let yes = if lk == 5 {
                let (p, q) = (m.u32(left + 4), m.u32(right + 4));
                let cmp = if p == 0 {
                    if q == 0 { 0 } else { -1 }
                } else if q == 0 {
                    1
                } else {
                    si_mem::clib::strcmp(m, p, q)
                };
                match oper {
                    op::CONTAINS => si_mem::clib::strstr(m, p, q) != 0,
                    op::EQ => cmp == 0,
                    op::NE => cmp != 0,
                    op::LT => cmp < 0,
                    op::LE => cmp <= 0,
                    op::GT => cmp > 0,
                    op::GE => cmp >= 0,
                    _ => false,
                }
            } else {
                let (a1, a2, b1, b2) = (m.u32(left + 4), m.u32(left + 8), m.u32(right + 4), m.u32(right + 8));
                let all = |x: u32, y: u32| x == u32::MAX && y == u32::MAX;
                let equal = (a1 == b1 && a2 == b2) || all(a1, a2) || all(b1, b2);
                if oper == op::NE { !equal } else { equal }
            };
            m.set_f32(out + 4, if yes { 1.0 } else { 0.0 });
            return Ok(());
        }
        let l = get_attr_val_in_float(m, Ptr32::new(left));
        let rv = get_attr_val_in_float(m, Ptr32::new(right));
        let truth = |b: bool| if b { F80::ONE } else { F80::ZERO };
        let result = match oper {
            op::ASSIGN..=op::DIV_ASSIGN => {
                let (leaf, arg, e) = target.expect("an assignment has its target here");
                if m.i32(arg + 8) == rec::chap(m, cid::CHN_DRV) {
                    let obj = m.u32(e + 4);
                    o.chn_set_value(m, Ptr32::new(obj), m.i32(arg + 0x14), rv.to_f32());
                    rv
                } else if e == 0 {
                    rv
                } else {
                    let v = match oper {
                        op::ADD_ASSIGN => l + rv,
                        op::SUB_ASSIGN => l - rv,
                        op::MUL_ASSIGN => l * rv,
                        op::DIV_ASSIGN => {
                            if F80::ZERO.eq(rv) {
                                m.set_i32(err, crate::eval::ERR_DIVIDE_BY_ZERO);
                                rv
                            } else {
                                l / rv
                            }
                        }
                        _ => rv,
                    };
                    let custom = if m.i32(leaf) == nop::CUSTOM_ARG {
                        m.u32(arg + 0x20)
                    } else {
                        m.set_u32(set.addr(), 1);
                        0
                    };
                    m.set_f32(set.addr() + 4, v.to_f32());
                    elem::ed_element_static_value_set(m, o, Ptr32::new(e), m.i32(arg + 8), m.i32(arg + 0x10), Ptr32::new(arg + 0x14), set, -1, custom);
                    v
                }
            }
            op::ADD => l + rv,
            op::SUB => l - rv,
            op::MUL => l * rv,
            op::DIV => {
                if F80::ZERO.eq(rv) {
                    m.set_i32(err, crate::eval::ERR_DIVIDE_BY_ZERO);
                    l
                } else {
                    l / rv
                }
            }
            op::MOD => {
                if F80::ZERO.eq(rv) {
                    m.set_i32(err, crate::eval::ERR_DIVIDE_BY_ZERO);
                    l
                } else {
                    let (x, y) = (l.to_i32(Round::Zero), rv.to_i32(Round::Zero));
                    if y == 0 || (x == i32::MIN && y == -1) {
                        return Err(EvalUndefined::IntegerDivide);
                    }
                    F80::from_i32(x % y)
                }
            }
            op::NEG => -l,
            op::NOT => truth(F80::ZERO.eq(l)),
            op::EQ => truth(l.eq(rv)),
            op::NE => truth(!l.eq(rv)),
            op::LT => truth(l.lt(rv)),
            op::LE => truth(l.lt(rv) || l.eq(rv)),
            op::GT => truth(l.gt(rv)),
            op::GE => truth(l.gt(rv) || l.eq(rv)),
            op::AND => truth(!F80::ZERO.eq(l) && !F80::ZERO.eq(rv)),
            op::OR => truth(!F80::ZERO.eq(l) || !F80::ZERO.eq(rv)),
            _ => {
                o.printf(m, Ptr32::new(0x08699f4c));
                o.print(1, b"Unknown oper...\n");
                l
            }
        };
        m.set_f32(out + 4, result.to_f32());
        m.set_u32(out, 1);
        return Ok(());
    }
    // a leaf
    let leaf = m.u32(node.addr() + 4);
    let kind = m.i32(leaf);
    let data = m.u32(leaf + 4);
    m.set_u32(left + 4, 0);
    m.set_u32(left, 1);
    let f32w = |v: f32| v.to_bits();
    match kind {
        nop::INT | nop::RELATION => put(m, left, 4, data, m.u32(left + 8)),
        nop::FLOAT => m.set_u32(left + 4, data),
        nop::STRING => put(m, left, 5, data, m.u32(left + 8)),
        nop::ENUM => {
            let (e0, e1) = (m.u32(data), m.u32(data + 4));
            put(m, left, 3, e0, e1);
        }
        nop::PREDEF_4 => m.set_u32(left + 4, f32w(m[rec::SI_FRAME_START] as f32)),
        nop::PREDEF_5 => m.set_u32(left + 4, f32w(m[rec::SI_FRAME_END] as f32)),
        nop::PREDEF_6 => m.set_u32(left + 4, f32w(a.frame as f32)),
        nop::PREDEF_7 => m.set_u32(left + 4, f32w(a.time)),
        nop::RAND => {
            let r = Ptr32::<RandNode>::new(data);
            let index = m.i16(data + 4) as i32;
            let table = if a.rand_values == 0 { 0 } else { m.u32(a.rand_values.wrapping_add((index as u32).wrapping_mul(4))) };
            let v = if a.rand_values == 0 || table == 0 || m.i32(data + 8) < 0 {
                expr_random_eval(m, o, r).to_f32().to_bits()
            } else {
                m.u32(table.wrapping_add((a.frame_index as u32).wrapping_mul(4)))
            };
            m.set_u32(left + 4, v);
        }
        nop::ARG | nop::CUSTOM_ARG => {
            let custom = if kind == nop::CUSTOM_ARG { m.u32(data + 0x20) } else { 0 };
            let chap = m.i32(data + 8);
            let e = si_dict::elemdict::get_elem(m, ed, chap, m.i32(data + 0xc));
            if !e.is_null() {
                if chap == rec::chap(m, cid::CHN_DRV) {
                    let obj = m.u32(e.addr() + 4);
                    let r = o.chn_get_value(m, Ptr32::new(obj), m.i32(data + 0x14), a.time);
                    let v = r.st0.map_or(F80::ZERO, |b| F80::from_bits(u64::from_le_bytes(b[..8].try_into().unwrap()), u16::from_le_bytes([b[8], b[9]])));
                    m.set_f32(left + 4, v.to_f32());
                } else if m[rec::SI_QUERY_MODE] == 1 {
                    if kind != nop::CUSTOM_ARG && si_dict::attdict::get_grp_index_from_fcurve(m, SI_ATD, m.i32(data + 0x10), path) != 0 {
                        m.set_i32(ident.addr(), chap);
                        m.set_i32(ident.addr() + 4, m.i32(data + 0xc));
                        elem::ed_element_att_value_get(m, o, ident, path, Ptr32::new(left));
                    }
                } else {
                    elem::ed_element_static_value_get(m, o, e, chap, m.i32(data + 0x10), Ptr32::new(data + 0x14), Ptr32::new(left), custom);
                }
            }
        }
        nop::FCT => {
            let index = m.i32(data);
            if index < 0 {
                // si_getFctPtr answers null: the original returns without writing `out`
                return Ok(());
            }
            let entry = crate::token::get_fct_ptr(index).ok_or(EvalUndefined::UnknownFunction(index))?;
            let nargs = m.i32(data + 4);
            if nargs < 1 {
                m.set_u32(left + 4, 0);
            } else {
                let top = enter_level(m);
                let have = m.u32(m.u32(rec::NB_VALS_ALLOC.addr()) + 4 * top as u32);
                if have < nargs as u32 {
                    let slot = m.u32(rec::VALS.addr()) + 4 * top as u32;
                    let old = m.u32(slot);
                    if old != 0 {
                        m.free(old);
                        m.set_u32(slot, 0);
                    }
                    let counts = m.u32(rec::NB_VALS_ALLOC.addr());
                    m.set_u32(counts + 4 * top as u32, nargs as u32);
                    let block = m.si_calloc(nargs as u32, 0xc);
                    let slot = m.u32(rec::VALS.addr()) + 4 * top as u32;
                    m.set_u32(slot, block);
                }
                let mut i = 0;
                while i < m.i32(data + 4) {
                    let top = m[rec::TOP_OF_THE_STACK];
                    let vals = m.u32(m.u32(rec::VALS.addr()) + 4 * top as u32);
                    let arg = m.u32(m.u32(data + 8) + 4 * i as u32);
                    let k = if entry.nargs == -1 { entry.arg_kinds[0] } else { *entry.arg_kinds.get(i as usize).unwrap_or(&1) };
                    let dst = vals + 12 * i as u32;
                    if k == 10 {
                        m.set_i32(dst, 10);
                        m.set_u32(dst + 4, arg);
                    } else {
                        m.set_i32(dst, k);
                        eval_inner(m, o, expr, Ptr32::new(arg), dst, a, err)?;
                    }
                    i += 1;
                }
                let top = m[rec::TOP_OF_THE_STACK];
                let vals = m.u32(m.u32(rec::VALS.addr()) + 4 * top as u32);
                let n = m.u32(data + 4);
                let v = call_builtin(m, o, entry.address, n, vals)?;
                m.set_f32(left + 4, v.to_f32());
                m[rec::TOP_OF_THE_STACK] -= 1;
            }
        }
        nop::VAR => {
            let t = get_var_expr(m, Ptr32::new(leaf), m.i32(expr.addr() + 4), Ptr32::new(m.u32(expr.addr() + 8)));
            if !t.is_null() {
                eval_inner(m, o, expr, t, left, a, err)?;
            }
        }
        nop::UNUSED_12 => {
            o.printf(m, Ptr32::new(0x08699f1c));
            o.print(1, b"Not yet implemented...\n");
        }
        nop::PREDEF_14 => m.set_u32(left + 4, 0x4049_0fdb),
        nop::PREDEF_15 => m.set_u32(left + 4, 0x402d_f854),
        nop::PREDEF_16 | nop::PREDEF_18 | nop::PREDEF_21 => put(m, left, 4, 1, m.u32(left + 8)),
        nop::PREDEF_17 | nop::PREDEF_22 => put(m, left, 4, 0, m.u32(left + 8)),
        nop::PREDEF_19 => put(m, left, 4, 2, m.u32(left + 8)),
        _ => {
            o.printf(m, Ptr32::new(0x08699f34));
            o.print(1, b"Unknown object type...\n");
        }
    }
    let (k, w1, w2) = (m.u32(left), m.u32(left + 4), m.u32(left + 8));
    put(m, out, k, w1, w2);
    Ok(())
}

/// One level more on the evaluator's stack of argument arrays: the level's slots exist
/// (both arrays grown by one word, the new words 0) and `topOfTheStack` is the new level.
fn enter_level(m: &mut Mem) -> i32 {
    let top = m[rec::TOP_OF_THE_STACK];
    let next = top + 1;
    if m[rec::NB_ENTRIES_IN_STACK] == next {
        if m[rec::NB_ENTRIES_IN_STACK] == 0 {
            m[rec::NB_ENTRIES_IN_STACK] = top + 2;
            m[rec::TOP_OF_THE_STACK] = next;
            let p = m.si_calloc(1, 4);
            m.set_u32(rec::NB_VALS_ALLOC.addr(), p);
            let q = m.si_calloc(1, 4);
            m.set_u32(rec::VALS.addr(), q);
        } else {
            m[rec::NB_ENTRIES_IN_STACK] = top + 2;
            m[rec::TOP_OF_THE_STACK] = next;
            let old = m.u32(rec::NB_VALS_ALLOC.addr());
            let p = m.si_realloc(old, (top as u32).wrapping_mul(4) + 8);
            m.set_u32(rec::NB_VALS_ALLOC.addr(), p);
            let old = m.u32(rec::VALS.addr());
            let q = m.si_realloc(old, (m[rec::NB_ENTRIES_IN_STACK] as u32) << 2);
            m.set_u32(rec::VALS.addr(), q);
        }
        let t = m[rec::TOP_OF_THE_STACK] as u32;
        let p = m.u32(rec::NB_VALS_ALLOC.addr());
        m.set_u32(p + 4 * t, 0);
        let q = m.u32(rec::VALS.addr());
        m.set_u32(q + 4 * t, 0);
    }
    m[rec::TOP_OF_THE_STACK] = next;
    next
}

/// The value of an expression at a frame and a time [soft 0x0824924c SI_expressionEvaluate]:
/// the whole statement (which writes the target), or only its right-hand side for a query
/// (`SI_ExprRelData` +0x14); the result as a float into `*result`, the error code into
/// `*err` (0, or 4 after a division by zero).
pub fn expression_evaluate(m: &mut Mem, o: &mut dyn Outside, expr: Ptr32<Expression>, data: Ptr32<ExprRelData>, frame_index: i32, frame: i32, time: f32, result: Ptr32<f32>, err: Ptr32<i32>) -> Result<()> {
    m.set_i32(err.addr(), 0);
    let root = m.u32(expr.addr());
    let node = if m.i16(data.addr() + 0x14) == 0 { root } else { m.u32(root + 8) };
    let sp = m.sp();
    let v = Ptr32::<AttributeValue>::new(m.local(12));
    let a = EvalArgs { rand_values: m.u32(data.addr() + 0x1c), frame_index, frame, time };
    let r = eval_expr_exec(m, o, expr, Ptr32::new(node), v, a, err);
    if r.is_ok() {
        let f = get_attr_val_in_float(m, v).to_f32();
        m.set_f32(result.addr(), f);
    }
    m.set_sp(sp);
    r
}

/// The update's step for an expression [soft 0x082492b0 SI_animExpressionEvaluate]: the
/// signature of `si_motion`'s `UpdateHost::anim_expression_evaluate(node, time)`.
///
/// When the node has an object and arguments, its first argument is active and its
/// activation curve, if any, is on at the time (`SI_fcurveIntegerValue(curve, time, 0, 1)
/// > 0`), and that argument is an element of the EXPRESSIONS chapter holding a tree: the
/// random tables are filled again when there are random calls, the tables are shorter than
/// the play range and the node's first argument is that element; then the expression is
/// evaluated at the current frame (index: current − first) and the time, and an error's
/// text, if the list has one, goes to the status line.
pub fn anim_expression_evaluate(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<si_dict::records::UpdNode>, time: f32) -> Result<()> {
    let n = node.addr();
    let span = m[rec::SI_FRAME_END].wrapping_sub(m[rec::SI_FRAME_START]).wrapping_add(1);
    let args = m.u32(n + 0x1c);
    if m.u32(n + 8) == 0 || args == 0 || m.i16(args + 0xc) == 0 {
        return Ok(());
    }
    let act = m.u32(args + 0x10);
    if act != 0 && si_motion::fcv::integer_value(m, act, time, 0, 1) <= 0 {
        return Ok(());
    }
    let chap = m.i16(args) as i32;
    if chap != rec::chap(m, cid::EXP) {
        return Ok(());
    }
    let ed = rec::cur_dict(m);
    let e = si_dict::elemdict::get_elem(m, ed, chap, m.i32(args + 4));
    if e.is_null() {
        return Ok(());
    }
    let expr = m.u32(e.addr() + 4);
    if expr == 0 {
        return Ok(());
    }
    let data = m.u32(n + 0x24);
    if m.i16(data + 0x16) != 0 && m.i32(data + 0x18) < span && m.i16(n + 0x18) > 0 && m.i16(args) as i32 == rec::chap(m, cid::EXP) {
        super::regen::expr_gen_random_values(m, o, Ptr32::new(data), span, Ptr32::new(expr));
    }
    let sp = m.sp();
    let result = Ptr32::<f32>::new(m.local(4));
    let err = Ptr32::<i32>::new(m.local(4));
    let (cur, start) = (m[rec::SI_FRAME_CURRENT], m[rec::SI_FRAME_START]);
    let r = expression_evaluate(m, o, Ptr32::new(expr), Ptr32::new(data), cur.wrapping_sub(start), cur, time, result, err);
    let code = m.read(err);
    if r.is_ok() && code != 0 && expr_get_error_msg(m, code) != 0 {
        let text = expr_get_error_msg(m, code);
        o.status_display_message(m, Ptr32::new(text), 2);
    }
    m.set_sp(sp);
    r
}
