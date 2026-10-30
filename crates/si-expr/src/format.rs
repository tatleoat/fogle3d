// SPDX-License-Identifier: MIT
//! The printer: a tree back to text (`exp_parsing.c`, finding 0013).
//!
//! The original builds the text in a heap block that it grows by 100 bytes, appending one
//! piece at a time with or without a blank after it [soft 0x0824c830 SI_checkAllocStr]. The
//! blanks are what gives a printed expression its look: `cond( C < A, A, B )`.

// adapter: not in place; the printer reads value trees; not on the image.

use crate::env::Dict;
use crate::parse::Undefined;
use crate::token::{self, chapter};
use crate::tree::*;

/// How a reference's element is written (`SI_ArgType`; the names are ours).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgType {
    /// 1: by the element's name in the scene (the dialog).
    Name,
    /// 2: by its argument number, `arg<n>` or `this<n>` (a stored expression).
    Number,
    /// Any other value: `???`, or `this`.
    Other(i32),
}

/// How a variable is written (`SI_ExprVarType`): 0 by its name alone; anything else as
/// `[name=]definition`.
pub type VarType = i32;

/// Appends `piece`, and a blank if `sep` [soft 0x0824c830 SI_checkAllocStr].
fn append(dest: &mut Vec<u8>, piece: &[u8], sep: bool) {
    dest.extend_from_slice(piece);
    if sep {
        dest.push(b' ');
    }
}

/// Removes the zeros at the end of a number printed with six decimals, but keeps one digit
/// after the point [soft 0x0824c984 trim0char]: `0.300000` becomes `0.3`, `2.000000`
/// becomes `2.0`.
pub fn trim_zero_chars(s: &mut Vec<u8>) {
    if s.is_empty() {
        // the original looks at the byte in front of an empty text
        return;
    }
    let mut p = s.len() - 1;
    if p > 0 {
        while s[p] == b'0' {
            p -= 1;
            if p == 0 {
                break;
            }
        }
    }
    if s[p] == b'.' {
        p += 1;
    }
    // the terminator goes after the byte at `p`; in a text that ends with a point that is
    // one byte past the old terminator, which keeps the text as it was
    s.truncate((p + 1).min(s.len()));
}

/// `sprintf("%3.6f")` of a float (widened to double) followed by [`trim_zero_chars`].
fn real(v: f64) -> Vec<u8> {
    let mut s = c_fixed6(v);
    trim_zero_chars(&mut s);
    s
}

/// `%3.6f` as the C library prints it: six decimals, correctly rounded; `inf` and `nan` in
/// lower case with a minus sign where the sign bit is set.
fn c_fixed6(v: f64) -> Vec<u8> {
    if v.is_nan() {
        return if v.is_sign_negative() { b"-nan".to_vec() } else { b"nan".to_vec() };
    }
    if v.is_infinite() {
        return if v < 0.0 { b"-inf".to_vec() } else { b"inf".to_vec() };
    }
    format!("{v:.6}").into_bytes()
}

/// The text of an operator [soft 0x0824ed34 SI_getExprOperStr]; empty for the two unary
/// operators and for anything else, which the original leaves unwritten.
pub fn get_expr_oper_str(oper: u32) -> &'static str {
    match oper {
        op::ASSIGN => "=",
        op::ADD_ASSIGN => "+=",
        op::SUB_ASSIGN => "-=",
        op::MUL_ASSIGN => "*=",
        op::DIV_ASSIGN => "/=",
        op::ADD => "+",
        op::SUB => "-",
        op::MUL => "*",
        op::DIV => "/",
        op::MOD => "%",
        op::CONTAINS => "LIKE",
        op::EQ => "==",
        op::NE => "!=",
        op::LT => "<",
        op::LE => "<=",
        op::GT => ">",
        op::GE => ">=",
        op::AND => "&&",
        op::OR => "||",
        _ => "",
    }
}

/// The parameter part of a reference [soft 0x0824c9fc SI_expGetFtypeName]: the fcurve's
/// name, `name_<n>` for a weighted shape, `name_<cluster>` and `name_<cluster>_<n>` for the
/// cluster parameters, `G.name` for a model's parameter marked global.
pub fn get_ftype_name(dict: &dyn Dict, a: &ArgNode) -> Vec<u8> {
    let mut buf: Vec<u8> = if a.fcv == FCV_CUSTOM {
        a.custom_name.clone().unwrap_or_default()
    } else if a.fcv == 0 {
        Vec::new()
    } else {
        dict.name_from_fcurve(a.fcv)
    };
    if a.chapter != chapter::MODELS {
        return buf;
    }
    if a.fcv == 0x9a {
        if a.access0 != -1 {
            buf.extend_from_slice(format!("_{}", a.access0).as_bytes());
        }
    } else if a.fcv == 0xb7 || a.fcv == 0xb8 {
        // the cluster's name comes from the model, by the cluster's id; a missing element
        // or cluster prints as nothing
        let name = dict.cluster_name(a.chapter, a.elem, a.access1).unwrap_or_default();
        buf.push(b'_');
        buf.extend_from_slice(&name);
        if a.fcv == 0xb8 && a.access0 != -1 {
            buf.extend_from_slice(format!("_{}", a.access0).as_bytes());
        }
    } else if a.access0 == 1 {
        // (not while converting for the successor product: that mode is not ported)
        buf = b"G.".to_vec();
        buf.extend_from_slice(&dict.name_from_fcurve(a.fcv));
    }
    buf
}

/// Appends the text of a leaf [soft 0x0824cccc getExprFormat].
///
/// The original has a second vocabulary for writing expressions for the successor product
/// (the flag `SI_edSumatraTo3x`); the flag is zero in this program unless that export runs,
/// and those branches are not ported.
pub fn get_expr_format(
    dict: &dyn Dict,
    expr: &Expression,
    leaf: &NopNode,
    dest: &mut Vec<u8>,
    arg_type: ArgType,
    var_type: VarType,
) -> Result<(), Undefined> {
    let word = |dest: &mut Vec<u8>, w: &str| append(dest, w.as_bytes(), true);
    match (leaf.kind, &leaf.data) {
        (nop::INT, NopData::Int(v)) => append(dest, v.to_string().as_bytes(), true),
        (nop::FLOAT, NopData::Float(v)) => append(dest, &real(*v as f64), true),
        (nop::STRING, NopData::Text(t)) => {
            append(dest, b"\"", false);
            // a null text would fault in the original
            append(dest, t.as_deref().ok_or(Undefined::StalePointer(20))?, false);
            append(dest, b"\"", true);
        }
        (nop::ENUM, NopData::Enum(e)) => {
            if e.enum_type == -1 && e.value == -1 {
                word(dest, "ALL");
            } else {
                // a value the dictionary does not have is a null text: a fault there
                let code = dict.enum_code_by_value(e.enum_type, e.value).ok_or(Undefined::StalePointer(21))?;
                append(dest, &code, true);
            }
        }
        (nop::RELATION, NopData::Int(id)) => append(dest, &dict.rel_name(*id), true),
        (nop::PREDEF_4, _) => word(dest, "Fs"),
        (nop::PREDEF_5, _) => word(dest, "Fe"),
        (nop::PREDEF_6, _) => word(dest, "Fc"),
        (nop::PREDEF_7, _) => word(dest, "T"),
        (nop::PREDEF_14, _) => word(dest, "PI"),
        (nop::PREDEF_15, _) => word(dest, "E"),
        (nop::PREDEF_16, _) => word(dest, "TRUE"),
        (nop::PREDEF_17, _) => word(dest, "FALSE"),
        (nop::PREDEF_18, _) => word(dest, "LinearInterp"),
        (nop::PREDEF_19, _) => word(dest, "SplineInterp"),
        (nop::PREDEF_21, _) => word(dest, "FORWARD"),
        (nop::PREDEF_22, _) => word(dest, "BACKWARD"),
        (nop::CHANNEL, NopData::Channel(c)) => {
            // null names print as "(null)" through the C library's "%s"
            let (driver, dof) = dict.chn_names(c.driver, c.dof);
            let mut s = b"CHN ".to_vec();
            s.extend_from_slice(&driver.unwrap_or_else(|| b"(null)".to_vec()));
            s.push(b'.');
            s.extend_from_slice(&dof.unwrap_or_else(|| b"(null)".to_vec()));
            append(dest, &s, true);
        }
        (nop::RAND, NopData::Rand(r)) => {
            match r.form {
                0 => word(dest, "RAND("),
                1 => word(dest, "randBool("),
                2 => word(dest, "rand_0_1("),
                _ => {}
            }
            if r.seed == -1 {
                append(dest, b" ", false);
            } else {
                append(dest, r.seed.to_string().as_bytes(), false);
            }
            if r.form == 0 {
                // bounds: reals with six decimals trimmed, integers truncated toward zero
                let low = if r.real != 0 { real(r.low as f64) } else { (trunc_i32(r.low as f64)).to_string().into_bytes() };
                let mut s = b", ".to_vec();
                s.extend_from_slice(&low);
                s.push(b',');
                append(dest, &s, true);
                // the upper bound is the sum of two floats formed in the x87 register: for
                // two floats that sum is exact in a double unless their exponents are more
                // than 29 apart, where the register's 64 bits round first
                let sum = (si_x87::F80::from_f32(r.low) + si_x87::F80::from_f32(r.width)).to_f64();
                let mut s = if r.real != 0 {
                    real(sum)
                } else {
                    let mut t = trunc_i32(sum).to_string().into_bytes();
                    t.push(b' ');
                    t
                };
                s.extend_from_slice(b" )");
                append(dest, &s, true);
            } else {
                word(dest, " )");
            }
        }
        (nop::ARG | nop::CUSTOM_ARG, NopData::Arg(a)) => {
            if a.chapter != chapter::MODELS {
                append(dest, token::get_obj_type_from_chapid(a.chapter).as_bytes(), false);
            }
            let numbered = |a: &ArgNode| {
                if a.generic != 0 { format!("this{}", a.num) } else { format!("arg{}", a.num) }.into_bytes()
            };
            let mut with_name = true;
            if a.generic == 1 {
                if arg_type == ArgType::Number {
                    append(dest, &numbered(a), false);
                } else {
                    append(dest, b"this", false);
                }
            } else {
                match arg_type {
                    ArgType::Number => append(dest, &numbered(a), false),
                    ArgType::Name => match dict.elem_prefix_name(a.chapter, a.elem) {
                        Some(name) => append(dest, &name, false),
                        // no element, or no name: nothing is written before the point
                        None => with_name = false,
                    },
                    ArgType::Other(_) => append(dest, b"???", false),
                }
            }
            let _ = with_name;
            append(dest, b".", false);
            if leaf.kind == nop::CUSTOM_ARG {
                append(dest, a.custom_name.as_deref().ok_or(Undefined::StalePointer(22))?, true);
            } else {
                append(dest, &get_ftype_name(dict, a), true);
            }
        }
        (nop::FCT, NopData::Fct(f)) => {
            append(dest, token::get_valid_fct(f.index).as_bytes(), false);
            append(dest, b"(", true);
            for (i, arg) in f.args.iter().enumerate() {
                if i > 0 {
                    // the blank before a comma is taken back
                    if dest.last() == Some(&b' ') {
                        dest.pop();
                    }
                    append(dest, b",", true);
                }
                get_expr_node_format(dict, expr, arg.as_deref().ok_or(Undefined::StalePointer(23))?, dest, arg_type, var_type)?;
            }
            append(dest, b")", true);
        }
        (nop::VAR, NopData::Text(name)) => {
            let name = name.as_deref().ok_or(Undefined::StalePointer(24))?;
            if var_type == 0 {
                append(dest, name, true);
            } else {
                // the definition of the first variable of that name, else a question mark
                let def = match expr.vars.iter().find(|v| v.name == name) {
                    Some(v) => si_get_expr_node_format(dict, expr, v.expr.as_deref(), arg_type, var_type)?,
                    None => Some(b"?".to_vec()),
                };
                let mut s = b"[".to_vec();
                s.extend_from_slice(name);
                s.extend_from_slice(b"=]");
                s.extend_from_slice(&def.unwrap_or_else(|| b"(null)".to_vec()));
                append(dest, &s, true);
            }
        }
        // kind 12 and anything unknown: nothing is written
        _ => {}
    }
    Ok(())
}

/// A double to an integer as a C cast does it on the x87 unit: toward zero, and the
/// "integer indefinite" 0x80000000 for a value out of range or a NaN.
fn trunc_i32(v: f64) -> i32 {
    si_x87::F80::from_f64(v).to_i32(si_x87::Round::Zero)
}

/// Appends the text of a tree [soft 0x0824d94c getExprNodeFormat]: parentheses if the node
/// carries the mark, a leaf through [`get_expr_format`], otherwise left operand, operator,
/// right operand. The two unary operators are written before their operand, and the empty
/// operator text after it still gets its blank: `-x` prints with two blanks behind it.
pub fn get_expr_node_format(
    dict: &dyn Dict,
    expr: &Expression,
    node: &ExprNode,
    dest: &mut Vec<u8>,
    arg_type: ArgType,
    var_type: VarType,
) -> Result<(), Undefined> {
    let oper = node.get_oper();
    if node.is_paren() {
        append(dest, b"(", true);
    }
    if oper == op::LEAF {
        // a leaf with no record would fault in the original
        let leaf = node.leaf.as_deref().ok_or(Undefined::StalePointer(25))?;
        get_expr_format(dict, expr, leaf, dest, arg_type, var_type)?;
    } else {
        if oper == op::NEG {
            append(dest, b"-", false);
        } else if oper == op::NOT {
            append(dest, b"!", false);
        }
        // a null left operand would fault in the original (an assignment without a target
        // is never printed through here: its callers print the right side)
        let left = node.left.as_deref().ok_or(Undefined::StalePointer(26))?;
        get_expr_node_format(dict, expr, left, dest, arg_type, var_type)?;
        append(dest, get_expr_oper_str(oper).as_bytes(), true);
        if let Some(right) = node.right.as_deref() {
            get_expr_node_format(dict, expr, right, dest, arg_type, var_type)?;
        }
    }
    if node.is_paren() {
        append(dest, b")", true);
    }
    Ok(())
}

/// The text of a tree with the blanks at its end removed
/// [soft 0x0824da6c SI_getExprNodeFormat]; `None` (a null pointer) for a null tree.
pub fn si_get_expr_node_format(
    dict: &dyn Dict,
    expr: &Expression,
    node: Option<&ExprNode>,
    arg_type: ArgType,
    var_type: VarType,
) -> Result<Option<Vec<u8>>, Undefined> {
    let Some(node) = node else { return Ok(None) };
    let mut s = Vec::new();
    get_expr_node_format(dict, expr, node, &mut s, arg_type, var_type)?;
    // (a text of nothing but blanks makes the original step in front of its block)
    while s.last() == Some(&b' ') {
        s.pop();
    }
    Ok(Some(s))
}

/// The pieces of an expression as the dialog shows them
/// [soft 0x0824db00 SI_getCompleteExprFormat]: the text of the target, the variables with
/// the text of their definitions, and the text of the right-hand side.
pub struct CompleteFormat {
    pub target: Option<Vec<u8>>,
    pub vars: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    pub rhs: Option<Vec<u8>>,
}

pub fn get_complete_expr_format(
    dict: &dyn Dict,
    expr: &Expression,
    arg_type: ArgType,
    var_type: VarType,
) -> Result<CompleteFormat, Undefined> {
    let root = expr.root.as_deref().ok_or(Undefined::StalePointer(27))?;
    let mut vars = Vec::new();
    for v in &expr.vars {
        vars.push((v.name.clone(), si_get_expr_node_format(dict, expr, v.expr.as_deref(), arg_type, var_type)?));
    }
    Ok(CompleteFormat {
        target: si_get_expr_node_format(dict, expr, root.left.as_deref(), arg_type, var_type)?,
        vars,
        rhs: si_get_expr_node_format(dict, expr, root.right.as_deref(), arg_type, var_type)?,
    })
}

/// `target = right-hand side`, with element names and with variables written out
/// [soft 0x0824dbc8 SI_getExpressionString]; `None` if there is no right-hand side.
pub fn get_expression_string(dict: &dyn Dict, expr: &Expression) -> Result<Option<Vec<u8>>, Undefined> {
    let f = get_complete_expr_format(dict, expr, ArgType::Name, 1)?;
    let Some(rhs) = f.rhs else { return Ok(None) };
    let mut s = f.target.unwrap_or_else(|| b"(null)".to_vec());
    s.extend_from_slice(b" = ");
    s.extend_from_slice(&rhs);
    Ok(Some(s))
}

/// What the original prints on standard error for debugging
/// [soft 0x0824ecf0 SI_expPrint]: the whole statement with element names.
pub fn exp_print(dict: &dyn Dict, expr: &Expression) -> Result<Vec<u8>, Undefined> {
    let s = si_get_expr_node_format(dict, expr, expr.root.as_deref(), ArgType::Name, 0)?;
    let mut out = b"EXPRESSION:\n".to_vec();
    out.extend_from_slice(&s.unwrap_or_else(|| b"(null)".to_vec()));
    out.push(b'\n');
    Ok(out)
}

/// The leaf of a node if it is a reference [soft 0x0824b084 get_object].
pub fn get_object(node: &ExprNode) -> Option<&NopNode> {
    if node.get_oper() != op::LEAF {
        return None;
    }
    let leaf = node.leaf.as_deref()?;
    (leaf.kind == nop::ARG || leaf.kind == nop::CUSTOM_ARG).then_some(leaf)
}

/// Collects the names of the references of a tree, in the order met: left before right,
/// function arguments in order [soft 0x0824dd04 getExprArgList]. Every reference adds a
/// name, `this<n>` or `arg<n>`; nothing is left out for being there already. Returns
/// whether the tree has a reference at all (the original's other use of the function, with
/// no list, which stops at the first).
pub fn collect_arg_list(node: &ExprNode, list: &mut Option<&mut Vec<Vec<u8>>>) -> bool {
    if let Some(leaf) = get_object(node) {
        if let (Some(list), NopData::Arg(a)) = (list.as_deref_mut(), &leaf.data) {
            list.push(if a.generic != 0 { format!("this{}", a.num) } else { format!("arg{}", a.num) }.into_bytes());
        }
        return true;
    }
    let mut found = false;
    if node.get_oper() == op::LEAF {
        if let Some(NopNode { kind: nop::FCT, data: NopData::Fct(f) }) = node.leaf.as_deref() {
            for arg in f.args.iter().flatten() {
                found = collect_arg_list(arg, list);
                if list.is_none() && found {
                    break;
                }
            }
        }
    } else {
        if let Some(left) = node.left.as_deref() {
            found = collect_arg_list(left, list);
            if list.is_none() && found {
                return true;
            }
        }
        if let Some(right) = node.right.as_deref() {
            found = collect_arg_list(right, list);
        }
    }
    found
}

/// The argument names of an expression [soft 0x0824dea8 si_getExprArgList]: those of the
/// right-hand side, then those of each variable's definition. `None` (a null list) when
/// there is none.
pub fn get_expr_arg_list(expr: &Expression) -> Option<Vec<Vec<u8>>> {
    let mut names = Vec::new();
    {
        let mut list = Some(&mut names);
        if let Some(rhs) = expr.root.as_deref().and_then(|r| r.right.as_deref()) {
            collect_arg_list(rhs, &mut list);
        }
        for v in &expr.vars {
            if let Some(e) = v.expr.as_deref() {
                collect_arg_list(e, &mut list);
            }
        }
    }
    (!names.is_empty()).then_some(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_zeros_but_keeps_one_decimal() {
        let t = |s: &str| {
            let mut v = s.as_bytes().to_vec();
            trim_zero_chars(&mut v);
            String::from_utf8(v).unwrap()
        };
        assert_eq!(t("0.300000"), "0.3");
        assert_eq!(t("2.000000"), "2.0");
        assert_eq!(t("1.595000"), "1.595");
        assert_eq!(t("100.000000"), "100.0");
        assert_eq!(t("inf"), "inf");
        assert_eq!(t("0.000001"), "0.000001");
    }
}
