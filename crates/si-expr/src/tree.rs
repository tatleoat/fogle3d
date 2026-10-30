// SPDX-License-Identifier: MIT
//! The parse tree of an expression, as the parser builds it in memory (finding 0013).
//!
//! Three records make a tree in the original. Sizes and offsets are of the 32-bit build.
//!
//! `expr_node`, 12 bytes [soft 0x082509a0 AllocExprNode]:
//!
//! | Offset | Field |
//! |---|---|
//! | +0 | operator code ([`op`]); bit 7 ([`PAREN`]) marks a node written in parentheses |
//! | +4 | operator 0: the leaf record (`SI_ExpNopNode`); otherwise the left operand |
//! | +8 | the right operand; null for the unary operators and for a leaf |
//!
//! `SI_ExpNopNode`, 8 bytes [soft 0x082509dc AllocExpr]: a kind ([`nop`]) at +0 and at +4
//! either the value itself (integer, float, string pointer, name pointer) or a pointer to a
//! record of its own (enumeration 8 bytes, random number 0x18, argument 0x24, function call
//! 0xc, channel 8). Kinds with no data leave +4 zero.
//!
//! `SI_Expression`, 12 bytes [soft 0x0824b9ac SI_genCusExprParsingTree]: the root at +0, the
//! number of variables at +4 and at +8 an array of 8-byte entries (name, tree).

// adapter: not in place; trees as Rust values; on the image they are `crate::image::rec`'s records, laid there by
// `crate::image::tree` from these (the parser still builds these).

/// Operator codes of `expr_node` +0 (`SI_ExpNodeTypes`). The names are ours; the values are
/// the constants the parser's actions pass to `AllocExprNode` [soft 0x0824f740 EXP_yyparse]
/// and the evaluator switches on [soft 0x0824bc5c si_evalExprExec].
pub mod op {
    /// Not an operator: the node is a leaf and +4 points at a `SI_ExpNopNode`.
    pub const LEAF: u32 = 0;
    pub const ASSIGN: u32 = 1;
    pub const ADD_ASSIGN: u32 = 2;
    pub const SUB_ASSIGN: u32 = 3;
    pub const MUL_ASSIGN: u32 = 4;
    pub const DIV_ASSIGN: u32 = 5;
    pub const ADD: u32 = 6;
    pub const SUB: u32 = 7;
    pub const MUL: u32 = 8;
    pub const DIV: u32 = 9;
    pub const MOD: u32 = 10;
    pub const NEG: u32 = 11;
    pub const NOT: u32 = 12;
    pub const CONTAINS: u32 = 13;
    pub const EQ: u32 = 14;
    pub const NE: u32 = 15;
    pub const LT: u32 = 16;
    pub const LE: u32 = 17;
    pub const GT: u32 = 18;
    pub const GE: u32 = 19;
    pub const AND: u32 = 20;
    pub const OR: u32 = 21;
}

/// Bit 7 of the operator word: the node was written inside parentheses
/// (set by the parser's rule for `( expr )`).
pub const PAREN: u32 = 0x80;

/// Kinds of `SI_ExpNopNode` +0 (`SI_ExpNopTypes`). Names are ours.
pub mod nop {
    /// An integer constant; +4 is the `int`.
    pub const INT: i32 = 0;
    /// A real constant; +4 is the `float`.
    pub const FLOAT: i32 = 1;
    /// A string in double quotes; +4 points at the text.
    pub const STRING: i32 = 2;
    /// A value of an enumeration; +4 points at two ints (enumeration number, value id).
    pub const ENUM: i32 = 3;
    pub const PREDEF_4: i32 = 4;
    pub const PREDEF_5: i32 = 5;
    pub const PREDEF_6: i32 = 6;
    pub const PREDEF_7: i32 = 7;
    /// A random number; +4 points at a [`super::RandNode`].
    pub const RAND: i32 = 8;
    /// A reference to a parameter of an element; +4 points at an [`super::ArgNode`].
    pub const ARG: i32 = 9;
    /// A function call; +4 points at a [`super::FctNode`].
    pub const FCT: i32 = 10;
    /// A variable; +4 points at its name.
    pub const VAR: i32 = 11;
    pub const UNUSED_12: i32 = 12;
    /// A channel of an input device; +4 points at two ints (driver, degree of freedom).
    pub const CHANNEL: i32 = 13;
    pub const PREDEF_14: i32 = 14;
    pub const PREDEF_15: i32 = 15;
    pub const PREDEF_16: i32 = 16;
    pub const PREDEF_17: i32 = 17;
    pub const PREDEF_18: i32 = 18;
    pub const PREDEF_19: i32 = 19;
    /// The id of a relation, written by its name; +4 is the `int`.
    pub const RELATION: i32 = 20;
    pub const PREDEF_21: i32 = 21;
    pub const PREDEF_22: i32 = 22;
    /// A reference to a custom effect's parameter, named by text; same record as [`ARG`].
    pub const CUSTOM_ARG: i32 = 23;
}

/// The fcurve type that means "a custom effect's parameter, see the name" (0x236).
pub const FCV_CUSTOM: i32 = 566;

/// `expr_node`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExprNode {
    /// +0: the operator code with the [`PAREN`] bit.
    pub oper: u32,
    /// +4 when the operator is 0.
    pub leaf: Option<Box<NopNode>>,
    /// +4 when the operator is not 0.
    pub left: Option<Box<ExprNode>>,
    /// +8.
    pub right: Option<Box<ExprNode>>,
}

impl ExprNode {
    /// The operator without the parenthesis bit [soft 0x0824b020 SI_expGetOper].
    pub fn get_oper(&self) -> u32 {
        self.oper & 0xffff_ff7f
    }

    pub fn is_paren(&self) -> bool {
        self.oper & PAREN != 0
    }

    /// A leaf node (`AllocExprNode(nop, 0, 0)`).
    pub fn leaf(n: NopNode) -> ExprNode {
        ExprNode { oper: op::LEAF, leaf: Some(Box::new(n)), left: None, right: None }
    }

    /// An operator node (`AllocExprNode(left, right, oper)`).
    pub fn new(oper: u32, left: Option<Box<ExprNode>>, right: Option<Box<ExprNode>>) -> ExprNode {
        ExprNode { oper, leaf: None, left, right }
    }
}

/// `SI_ExpNopNode`: the kind at +0 and what +4 holds.
#[derive(Clone, Debug, PartialEq)]
pub struct NopNode {
    pub kind: i32,
    pub data: NopData,
}

/// What `SI_ExpNopNode` +4 holds or points at.
#[derive(Clone, Debug, PartialEq)]
pub enum NopData {
    /// +4 is zero: the predefined tokens.
    None,
    /// +4 is an `int` (kinds 0 and 20).
    Int(i32),
    /// +4 is a `float` (kind 1).
    Float(f32),
    /// +4 points at text (kind 2: a string; kind 11: a variable's name). Null is `None`.
    Text(Option<Vec<u8>>),
    /// +4 points at two ints (kind 3).
    Enum(EnumNode),
    /// Kind 8.
    Rand(RandNode),
    /// Kinds 9 and 23.
    Arg(ArgNode),
    /// Kind 10.
    Fct(FctNode),
    /// Kind 13.
    Channel(ChannelNode),
}

/// The 8-byte record of an enumeration value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnumNode {
    /// +0: the number of the enumeration in the enumeration dictionary.
    pub enum_type: i32,
    /// +4: the first word of the dictionary's value record (its id).
    pub value: i32,
}

/// The 8-byte record of a channel reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelNode {
    /// +0: the driver's index.
    pub driver: i32,
    /// +4: the index of the degree of freedom.
    pub dof: i32,
}

/// `SI_ExpRandFctNode`, 0x18 bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RandNode {
    /// +0x00: which function: 0 the one with a range (`RAND`), 1 the one that gives 0 or 1,
    /// 2 the one that gives a real number from 0 to 1.
    pub form: i32,
    /// +0x04: the place of this call among the expression's random calls (`short`).
    pub index: i16,
    /// +0x08: the seed; below zero means "not seeded".
    pub seed: i32,
    /// +0x0c: non-zero for real results, zero for results rounded to integers (`short`).
    pub real: i16,
    /// +0x10: the lower bound.
    pub low: f32,
    /// +0x14: the width of the range (upper bound − lower bound).
    pub width: f32,
}

/// `SI_ExpArgNode`, 0x24 bytes: a reference `element.parameter`.
#[derive(Clone, Debug, PartialEq)]
pub struct ArgNode {
    /// +0x00: the argument's number (`arg<n>` in a stored expression).
    pub num: i32,
    /// +0x04: 1 if the element was written `this` (`short`).
    pub generic: i16,
    /// +0x08: the chapter id of the element.
    pub chapter: i32,
    /// +0x0c: the element's index in its chapter; −1 for `this`, −2 if not looked up.
    pub elem: i32,
    /// +0x10: the fcurve type (the id of the parameter); 566 for a custom effect's parameter.
    pub fcv: i32,
    /// +0x14: first word of the access information (`SI_FcvAccessInfo`): the shape or
    /// cluster index, or 1 for `.G` (global) and 0 otherwise.
    pub access0: i32,
    /// +0x18: second word of the access information (the cluster's id).
    pub access1: i32,
    /// +0x1c: third word: the cluster's name, a pointer, when arguments are not validated.
    pub access_name: Option<Vec<u8>>,
    /// +0x20: the parameter's name for fcurve type 566.
    pub custom_name: Option<Vec<u8>>,
}

/// The 0xc-byte record of a function call.
#[derive(Clone, Debug, PartialEq)]
pub struct FctNode {
    /// +0: the function's index in the function table.
    pub index: i32,
    /// +4: number of arguments; +8 points at that many `expr_node` pointers, allocated five
    /// at a time [soft 0x08250a74 addAnArgToFct].
    pub args: Vec<Option<Box<ExprNode>>>,
}

/// One entry of the variable array (`SI_ExpVarNode`, 8 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct VarNode {
    /// +0: the name.
    pub name: Vec<u8>,
    /// +4: the tree of its definition.
    pub expr: Option<Box<ExprNode>>,
}

/// `SI_Expression`, 12 bytes.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Expression {
    /// +0: the tree of the whole statement (an assignment node in a stored expression).
    pub root: Option<Box<ExprNode>>,
    /// +4 (count) and +8 (array).
    pub vars: Vec<VarNode>,
}
