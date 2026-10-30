// SPDX-License-Identifier: MIT
//! Tree utilities (`exp_parsing.c`, `exp_fct.c`; finding 0013): walking, searching, copying,
//! and tying a loaded expression's references to the elements of its relations.

use crate::eval::node_is_assign_oper;
use crate::format::collect_arg_list;
use crate::scene::EvalUndefined;
use crate::token::{self, chapter};
use crate::tree::*;

/// The original's message text at `addr` (a C string of the Linux `soft`), read at run time
/// from the user's own install (`si-rt`; release model: no message of the original ships).
pub fn rt(addr: u32) -> &'static str {
    std::str::from_utf8(si_rt::message(addr)).expect("a message of the original is text")
}

/// Whether an expression refers to any element [soft 0x0824df3c SI_isExprHasArgs]: its
/// right-hand side first, then each variable's definition, stopping at the first reference.
pub fn is_expr_has_args(expr: Option<&Expression>) -> bool {
    let Some(expr) = expr else { return false };
    let Some(root) = expr.root.as_deref() else { return false };
    // (a statement without a right-hand side faults in the original)
    let mut none = None;
    if root.right.as_deref().is_some_and(|r| collect_arg_list(r, &mut none)) {
        return true;
    }
    expr.vars.iter().any(|v| v.expr.as_deref().is_some_and(|e| collect_arg_list(e, &mut none)))
}

/// The same under its other name [soft 0x0824df24 SI_isExprHasArgsForSumatra].
pub fn is_expr_has_args_for_sumatra(expr: Option<&Expression>) -> bool {
    is_expr_has_args(expr)
}

/// Sets the parser's "only new objects" flag [soft 0x0824ece0 SI_expCheckNewObjOnly].
pub fn exp_check_new_obj_only(parser: &mut crate::parse::Parser, flag: i16) {
    parser.check_new_obj_only = flag;
}

/// Walks a tree and calls `visit` on every leaf that is not a function call, until one
/// call answers true [soft 0x0824eb34 SI_exprTraverse]. Returns whether the walk was
/// stopped (the original returns the leaf it stopped at).
///
/// Oddities kept: the arguments of a function call are walked but the call itself is not
/// visited; a call whose function index is negative is skipped with its arguments; a call
/// with no argument stops the walk as if `visit` had said so; a binary operator without a
/// right operand, or any operator without a left one, is a fault.
pub fn expr_traverse(
    node: Option<&mut ExprNode>,
    visit: &mut dyn FnMut(&mut NopNode) -> bool,
) -> Result<bool, EvalUndefined> {
    let node = node.ok_or(EvalUndefined::NullPointer(20))?;
    let oper = node.get_oper();
    if oper == op::LEAF {
        let leaf = node.leaf.as_deref_mut().ok_or(EvalUndefined::NullPointer(21))?;
        if leaf.kind == nop::FCT {
            let NopData::Fct(f) = &mut leaf.data else { return Err(EvalUndefined::NullPointer(22)) };
            if token::get_fct_ptr(f.index).is_none() && f.index < 0 {
                return Ok(false);
            }
            if f.args.is_empty() {
                return Ok(true);
            }
            for a in f.args.iter_mut() {
                if expr_traverse(a.as_deref_mut(), visit)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        return Ok(visit(leaf));
    }
    if oper == op::NEG || oper == op::NOT {
        return expr_traverse(node.left.as_deref_mut(), visit);
    }
    if expr_traverse(node.left.as_deref_mut(), visit)? {
        return Ok(true);
    }
    expr_traverse(node.right.as_deref_mut(), visit)
}

/// Counts and lists the leaves of one kind [soft 0x0824ec88 SI_exprSearchNopNodes]
/// [soft 0x0824ec20 exprCheckNopNode]: of the right-hand side when `rhs_only` is 1, of the
/// whole statement otherwise. The list holds the kinds' data by clone (the original hands
/// out pointers into the tree).
pub fn expr_search_nop_nodes(expr: &mut Expression, kind: i32, rhs_only: i16) -> Result<Vec<NopNode>, EvalUndefined> {
    let root = expr.root.as_deref_mut().ok_or(EvalUndefined::NullPointer(23))?;
    let start = if rhs_only == 1 { root.right.as_deref_mut() } else { Some(root) };
    let mut found = Vec::new();
    expr_traverse(start, &mut |leaf| {
        if leaf.kind == kind {
            found.push(leaf.clone());
        }
        false
    })?;
    Ok(found)
}

/// Gives every generic reference (`this`) of an expression the element of its target
/// [soft 0x08248f98 si_exprExpandGenRef] [soft 0x08248f2c expandAnElement]: the variables'
/// definitions, then the right-hand side. `valid` stands for
/// `SI_isValidFcv` (0x0820a654) (is this fcurve type one that such an element has?);
/// it is asked unless a query is running, and one "no" makes the result true: the
/// original's flag `si_IncompatibleFcv`.
pub fn expr_expand_gen_ref(
    expr: &mut Expression,
    target_chapter: i32,
    target_elem: i32,
    query_mode: bool,
    valid: &mut dyn FnMut(i32, i32, i32) -> bool,
) -> Result<bool, EvalUndefined> {
    let mut incompatible = false;
    let mut expand = |leaf: &mut NopNode| {
        if leaf.kind == nop::ARG || leaf.kind == nop::CUSTOM_ARG {
            if let NopData::Arg(a) = &mut leaf.data {
                if a.generic == 1 {
                    a.chapter = target_chapter;
                    a.elem = target_elem;
                    if !query_mode && !valid(token::from_chap_to_graf(target_chapter), a.fcv, target_elem) {
                        incompatible = true;
                    }
                }
            }
        }
        false
    };
    for v in expr.vars.iter_mut() {
        expr_traverse(v.expr.as_deref_mut(), &mut expand)?;
    }
    let root = expr.root.as_deref_mut().ok_or(EvalUndefined::NullPointer(24))?;
    expr_traverse(root.right.as_deref_mut(), &mut expand)?;
    Ok(incompatible)
}

/// A constant operand [soft 0x0824e918 si_checkFloatNode]: an integer or real leaf gives
/// its value; a negation or a `!` of such a node gives the negated or inverted value; the
/// answer says whether it was one. Anything else gives 0 and "no".
pub fn check_float_node(node: Option<&ExprNode>) -> Result<(bool, f32), EvalUndefined> {
    let node = node.ok_or(EvalUndefined::NullPointer(25))?;
    match node.get_oper() {
        op::LEAF => match node.leaf.as_deref().ok_or(EvalUndefined::NullPointer(26))? {
            NopNode { kind: nop::INT, data: NopData::Int(v) } => Ok((true, *v as f32)),
            NopNode { kind: nop::FLOAT, data: NopData::Float(v) } => Ok((true, *v)),
            _ => Ok((false, 0.0)),
        },
        op::NEG => {
            let (ok, v) = check_float_node(node.left.as_deref())?;
            Ok((ok, -v))
        }
        op::NOT => {
            let (ok, v) = check_float_node(node.left.as_deref())?;
            Ok((ok, if v == 0.0 { 1.0 } else { 0.0 }))
        }
        _ => Ok((false, 0.0)),
    }
}

/// Whether a statement is `target = leaf × scale ± offset` with constant scale and offset
/// [soft 0x0824e9e4 SI_checkExprScalingOffsetOnly]: the scale, the offset and the leaf. The
/// constant may stand on either side of `*`, `+` or `-`; `constant − x` negates the scale.
pub fn check_expr_scaling_offset_only(expr: &Expression) -> Result<Option<(f32, f32, &ExprNode)>, EvalUndefined> {
    let root = expr.root.as_deref().ok_or(EvalUndefined::NullPointer(27))?;
    if !node_is_assign_oper(root.get_oper()) {
        return Ok(None);
    }
    let mut node = root.right.as_deref().ok_or(EvalUndefined::NullPointer(28))?;
    let mut offset = 0.0f32;
    let mut negate = false;
    let oper = node.get_oper();
    if oper == op::ADD || oper == op::SUB {
        let (ok, v) = check_float_node(node.right.as_deref())?;
        if ok {
            offset = if oper == op::SUB { -v } else { v };
            node = node.left.as_deref().ok_or(EvalUndefined::NullPointer(29))?;
        } else {
            let (ok, v) = check_float_node(node.left.as_deref())?;
            if !ok {
                return Ok(None);
            }
            offset = v;
            negate = oper == op::SUB;
            node = node.right.as_deref().ok_or(EvalUndefined::NullPointer(30))?;
        }
    }
    let mut scale = 1.0f32;
    if node.get_oper() == op::MUL {
        let (ok, v) = check_float_node(node.right.as_deref())?;
        if ok {
            scale = v;
            node = node.left.as_deref().ok_or(EvalUndefined::NullPointer(31))?;
        } else {
            let (ok, v) = check_float_node(node.left.as_deref())?;
            if !ok {
                return Ok(None);
            }
            scale = v;
            node = node.right.as_deref().ok_or(EvalUndefined::NullPointer(32))?;
        }
    }
    if negate {
        scale = -scale;
    }
    Ok((node.get_oper() == op::LEAF).then_some((scale, offset, node)))
}

/// A copy of a leaf [soft 0x0824e628 expNopNodeCopy]. The original duplicates the records
/// of random calls, references, function calls and channels and the names of variables and
/// of custom parameters; it shares, between the copy and the source, the text of a string,
/// the record of an enumeration value and a reference's cluster name. Rust's copy shares
/// nothing; the difference shows only when both trees are freed.
pub fn nop_node_copy(leaf: &NopNode) -> NopNode {
    leaf.clone()
}

/// A copy of a tree [soft 0x0824e7e4 expNodeCopy].
pub fn node_copy(node: &ExprNode) -> ExprNode {
    node.clone()
}

/// A copy of an expression with its variables [soft 0x0824e868 SI_expDataCopy].
pub fn exp_data_copy(expr: &Expression) -> Expression {
    expr.clone()
}

/// Frees a leaf [soft 0x0824e44c exprDispose]. What the original leaves allocated: the text
/// of a string, the record of an enumeration value, the name of a variable and a
/// reference's cluster name.
pub fn expr_dispose(leaf: NopNode) {
    drop(leaf);
}

/// Frees a tree [soft 0x0824e510 exprNodeDispose].
pub fn expr_node_dispose(node: ExprNode) {
    drop(node);
}

/// Frees an expression's trees [soft 0x0824e5b0 SI_expressDataDispose]: the variables'
/// definitions and their array, then the statement. The 12-byte record itself and the
/// variables' names are left allocated by the original.
pub fn express_data_dispose(expr: Expression) {
    drop(expr);
}

/// Frees an expression and its relation record [soft 0x0824e574 SI_expressDataDisposeAll].
pub fn express_data_dispose_all(expr: &mut Option<Expression>, data: &mut Option<crate::eval::ExprRelData>) {
    *expr = None;
    *data = None;
}

/// One cluster of a model as the look-up below sees it (the `kluster` list at +0x348 of
/// the model record).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClusterInfo {
    /// +0x00: the name.
    pub name: Vec<u8>,
    /// +0x18 not null: the cluster has animation data.
    pub has_data: bool,
    /// The 16-bit word at +8 of that data is 2: weighted interpolation.
    pub weighted: bool,
    /// The word at +0x14 of that data: how many weights.
    pub weights: i32,
    /// What `SI_edGetElemFromCluster` answers for it: its id.
    pub id: i32,
}

/// Finds the cluster a reference names [soft 0x0824e194 si_expFindValidCluster]: the first
/// cluster with data and that name. `Ok(id)`, or the error code: 1 the parameter does not
/// fit the cluster's interpolation, 2 the weight's number is out of range, 3 no such
/// cluster.
pub fn exp_find_valid_cluster(clusters: &[ClusterInfo], name: &[u8], fcv: i32, sub: i32) -> Result<i32, i32> {
    for c in clusters.iter().filter(|c| c.has_data && c.name == name) {
        return if c.weighted {
            if fcv == 0xb8 && sub < c.weights {
                Ok(c.id)
            } else if fcv != 0xb7 {
                Err(2)
            } else {
                Err(1)
            }
        } else if fcv == 0xb7 {
            Ok(c.id)
        } else if fcv != 0xb8 {
            Err(2)
        } else {
            Err(1)
        };
    }
    Err(3)
}

/// One entry of the list a relation gives an expression: the element behind an argument
/// number. Entry 0 is the target; it also carries the target's fcurve type.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RelArg {
    pub chapter: i32,
    pub elem: i32,
    pub fcv: i32,
}

/// Gives a reference the element of the list entry with its number
/// [soft 0x0824e268 set_rel_from_list]. For a model's cluster parameter the cluster is then
/// looked up by the name the parser kept, its id stored and the name dropped; `clusters`
/// answers the model's clusters (`None`: no such element, a fault in the original). An
/// error code from the look-up is returned for the status line.
pub fn set_rel_from_list(
    a: &mut ArgNode,
    list: &[RelArg],
    clusters: &mut dyn FnMut(i32, i32) -> Option<Vec<ClusterInfo>>,
) -> Result<Option<i32>, EvalUndefined> {
    // an argument number past the list reads behind it in the original
    let entry = list.get(a.num as u32 as usize).ok_or(EvalUndefined::MissingArgument)?;
    a.chapter = entry.chapter;
    a.elem = entry.elem;
    let mut status = None;
    if a.chapter == chapter::MODELS && (a.fcv == 0xb7 || a.fcv == 0xb8) {
        let list = clusters(entry.chapter, entry.elem).ok_or(EvalUndefined::NullPointer(33))?;
        a.access1 = -1;
        let name = a.access_name.clone().ok_or(EvalUndefined::NullPointer(34))?;
        match exp_find_valid_cluster(&list, &name, a.fcv, a.access0) {
            Ok(id) => a.access1 = id,
            Err(code) => status = Some(code),
        }
        a.access_name = None;
    }
    Ok(status)
}

/// Ties every reference of a tree to its element [soft 0x0824e36c si_exprResolveRelation]:
/// references by [`set_rel_from_list`]; an assignment's target likewise, and it also takes
/// the fcurve type of entry 0. Status codes of failed cluster look-ups are collected.
pub fn expr_resolve_relation(
    node: Option<&mut ExprNode>,
    list: &[RelArg],
    clusters: &mut dyn FnMut(i32, i32) -> Option<Vec<ClusterInfo>>,
    status: &mut Vec<i32>,
) -> Result<(), EvalUndefined> {
    let node = node.ok_or(EvalUndefined::NullPointer(35))?;
    let oper = node.get_oper();
    if oper == op::LEAF {
        match node.leaf.as_deref_mut().ok_or(EvalUndefined::NullPointer(36))? {
            NopNode { kind: nop::ARG | nop::CUSTOM_ARG, data: NopData::Arg(a) } => {
                status.extend(set_rel_from_list(a, list, clusters)?);
            }
            NopNode { kind: nop::FCT, data: NopData::Fct(f) } => {
                for arg in f.args.iter_mut() {
                    expr_resolve_relation(arg.as_deref_mut(), list, clusters, status)?;
                }
            }
            _ => {}
        }
        return Ok(());
    }
    if node_is_assign_oper(oper) {
        // the target's leaf is read as a reference whatever it is
        let target = node.left.as_deref_mut().and_then(|t| t.leaf.as_deref_mut()).ok_or(EvalUndefined::NullPointer(37))?;
        let NopData::Arg(a) = &mut target.data else { return Err(EvalUndefined::NullPointer(38)) };
        status.extend(set_rel_from_list(a, list, clusters)?);
        a.fcv = list.first().ok_or(EvalUndefined::MissingArgument)?.fcv;
    } else {
        expr_resolve_relation(node.left.as_deref_mut(), list, clusters, status)?;
    }
    if node.right.is_some() {
        expr_resolve_relation(node.right.as_deref_mut(), list, clusters, status)?;
    }
    Ok(())
}
