// SPDX-License-Identifier: MIT
//! The expression records and globals at the original's layout (finding 0013, sections 5
//! and 12).
//!
//! The same records the value form describes in [`crate::tree`], here as `#[repr(C)]`
//! records of the shared memory image (finding 0024). Sizes are those of the allocation
//! calls: `AllocExprNode` (0x082509a0) `SI_calloc(1, 0xc)`, `AllocExpr` (0x082509dc)
//! `SI_calloc(1, 8)` and, by kind, `SI_calloc(1, 8 | 0x18 | 0x24 | 0xc | 8)`,
//! `si_exprInitRelData` `SI_calloc(1, 0x20)`, the scene description's RELDATA reader 0x14
//! bytes (finding 0006).

use si_mem::{record, CStr32, Opaque, Ptr32};

record! {
    /// `expr_node`: an operator and its operands, or a leaf.
    pub struct ExprNode = "expr_node", size 0xc {
        0x00 oper: u32, "operator code; bit 7 marks a node written in parentheses";
        0x04 left: Ptr32<Opaque>, "operator 0: the leaf record (NopNode); otherwise the left operand (ExprNode)";
        0x08 right: Ptr32<ExprNode>, "right operand; null for unary operators and leaves";
    }
}

record! {
    /// `SI_ExpNopNode`: what a leaf is.
    pub struct NopNode = "SI_ExpNopNode", size 0x8 {
        0x00 kind: i32, "leaf kind (crate::tree::nop)";
        0x04 data: u32, "the value itself (integer, float bits, string or name pointer) or a pointer to the kind's record";
    }
}

record! {
    /// `SI_ExpArgNode`: a reference `element.parameter`.
    pub struct ArgNode = "SI_ExpArgNode", size 0x24 {
        0x00 num: i32, "argument number (arg<n>)";
        0x04 generic: i16, "1 for `this`";
        0x06 pad_06: [u8; 2], "not written";
        0x08 chapter: i32, "chapter of the element";
        0x0c elem: i32, "element index; -1 `this`, -2 not looked up";
        0x10 fcv: i32, "fcurve type of the parameter; 566 a custom effect's";
        0x14 access0: i32, "SI_FcvAccessInfo word 0: shape or cluster index, or 1 for `.G`";
        0x18 access1: i32, "word 1: the cluster's id";
        0x1c access_name: CStr32, "word 2: the cluster's name while arguments are not validated";
        0x20 custom_name: CStr32, "the parameter's name for fcurve type 566";
    }
}

record! {
    /// `SI_ExpRandFctNode`: a call of one of the random functions.
    pub struct RandNode = "SI_ExpRandFctNode", size 0x18 {
        0x00 form: i32, "0 RAND, 1 randBool, 2 rand_0_1";
        0x04 index: i16, "its count among the random calls of the text parsed";
        0x06 pad_06: [u8; 2], "not written";
        0x08 seed: i32, "the seed; below zero: none";
        0x0c real: i16, "non-zero for real results";
        0x0e pad_0e: [u8; 2], "not written";
        0x10 low: f32, "lower bound";
        0x14 width: f32, "upper bound minus lower bound";
    }
}

record! {
    /// A function call's record.
    pub struct FctNode = "(function call)", size 0xc {
        0x00 index: i32, "index in math_fct_array";
        0x04 nargs: i32, "number of arguments";
        0x08 args: Ptr32<Ptr32<ExprNode>>, "the argument trees, allocated five at a time (addAnArgToFct)";
    }
}

record! {
    /// An enumeration value's or a channel's record: two words.
    pub struct Pair = "(enumeration value, channel)", size 0x8 {
        0x00 a: i32, "enumeration number, or driver";
        0x04 b: i32, "value id, or degree of freedom";
    }
}

record! {
    /// `SI_ExpVarNode`: one variable.
    pub struct VarNode = "SI_ExpVarNode", size 0x8 {
        0x00 name: CStr32, "the variable's name";
        0x04 expr: Ptr32<ExprNode>, "the tree of its definition";
    }
}

record! {
    /// `SI_Expression`: the tree of a statement and its variables.
    pub struct Expression = "SI_Expression", size 0xc {
        0x00 root: Ptr32<ExprNode>, "the statement (an assignment node for a stored expression)";
        0x04 nvars: i32, "number of variables";
        0x08 vars: Ptr32<VarNode>, "the variables";
    }
}

record! {
    /// `SI_ExprRelData`: what an expression's relation (20010) carries besides the tree.
    pub struct ExprRelData = "SI_ExprRelData", size 0x20 {
        0x00 fcv: i32, "the target's fcurve type";
        0x04 access0: i32, "the target's access information";
        0x08 access1: i32, "";
        0x0c access_name: u32, "";
        0x10 custom_name: CStr32, "the target's name for fcurve type 566 (strdup)";
        0x14 query: i16, "1 for a spreadsheet query: only the right-hand side is evaluated";
        0x16 nrand: i16, "number of random calls";
        0x18 nframes: i32, "frames the random tables were filled for";
        0x1c values: Ptr32<Ptr32<f32>>, "per random call a table of one float per frame, null without a seed";
    }
}

/// The relation data the scene description's reader leaves for relation 20010 before
/// `SI_expressionsRegenerate` replaces it: 0x14 zeroed bytes, the parameter at +0 and, for
/// parameter 566, a name at +0x10 (finding 0006; `si_ascii::reldata::Expression`). It is
/// the first 0x14 bytes of [`ExprRelData`]; `SI_expressArgsRegenerate` reads it through an
/// `int *` and so does the port, word by word.
pub const REL_DATA_FILE_SIZE: u32 = 0x14;

si_mem::globals! {
    /// The spreadsheet's query mode (16 bits: the evaluator compares it with `cmpw`).
    SI_QUERY_MODE: i16 = 0x08699ec0, "SI_queryMode", init 0;
    /// The parser's running argument number (`si_getArgNextNum`).
    ARG_NUM_CUR_VAL: i32 = 0x08699ec8, "argNumCurVal", init 0;
    /// The parser builds nothing while it is set (`AllocExprNode`, `AllocExpr`).
    SI_CHECK_EXPR_SYNTAX_ONLY: i16 = 0x08699ecc, "si_checkExprSyntaxOnly", init 1;
    /// The error table's length (`SI_exprGetErrorMsg`).
    ERROR_MSG_SIZE: i32 = 0x08699e60, "errorMsgSize", init 8;
    /// The evaluator's stack of argument arrays, one level per nested function call
    /// (statics of `si_evalExprExec`): how many levels have room …
    NB_ENTRIES_IN_STACK: i32 = 0x08699f0c, "nbEntriesInStack.268", init 0;
    /// … the current level …
    TOP_OF_THE_STACK: i32 = 0x08699f10, "topOfTheStack.269", init -1;
    /// … per level the number of value records allocated …
    NB_VALS_ALLOC: Ptr32<u32> = 0x08699f14, "nb_vals_alloc.270", init 0;
    /// … and per level the array of value records.
    VALS: Ptr32<Ptr32<si_types::dict::AttributeValue>> = 0x08699f18, "vals.271", init 0;
    /// The frame globals of `play_control.c`.
    SI_FRAME_CURRENT: i32 = 0x08782040, "SI_frameCurrent";
    SI_FRAME_END: i32 = 0x08782044, "SI_frameEnd";
    SI_FRAME_RATE: f32 = 0x0878204c, "SI_frameRate";
    SI_FRAME_START: i32 = 0x08782050, "SI_frameStart";
    /// The export to the successor product (not ported: the full product's 3.x path).
    SI_ED_SUMATRA_TO_3X: i32 = 0x08680e50, "SI_edSumatraTo3x", init 0;
}

/// `exprErrorList` (0x08699e20): (code, message address) pairs. The texts are the crate's
/// [`crate::eval::ERROR_MESSAGES`]; the addresses are where the binary keeps them.
pub const EXPR_ERROR_LIST: u32 = 0x08699e20;
pub const ERROR_TEXT_ADDRS: [u32; 8] = [0x08699d00, 0x08699d20, 0x08699d5f, 0x08699d76, 0x08699d8c, 0x08699dc0, 0x08699ddf, 0x08699df5];

/// Puts into the image the initial values of the globals above that have one, and the
/// error table with its texts (`.data` of `exp_parsing.c`, `exp_fct.c`): what the program
/// has from its file before `main`. Held against the binary by
/// `re/tools/si-expr/image_tables.py`.
pub fn init_globals(m: &mut si_mem::Mem) {
    si_mem::init_globals(m, ALL);
    for (i, &(code, text)) in crate::eval::ERROR_MESSAGES.iter().enumerate() {
        m.set_i32(EXPR_ERROR_LIST + 8 * i as u32, code);
        m.set_u32(EXPR_ERROR_LIST + 8 * i as u32 + 4, ERROR_TEXT_ADDRS[i]);
        m.put_cstr(ERROR_TEXT_ADDRS[i], text.as_bytes());
    }
    // the two literals of `SI_edAddExpression` (`express_io.c`): the expression file's entry and the
    // base of a new element's name
    m.put_cstr(0x0869b940, b"express_file");
    m.put_cstr(0x0869b94d, b"expr");
    // the literals of `SI_generateQueryFromClause` (`exp_parsing.c`) and `SI_copyElemExpression` (`exp_fct.c`)
    m.put_cstr(0x0869a0be, b"this.chptyp == ALL");
    m.put_cstr(0x0869a0d1, b"=");
    m.put_cstr(0x08699f5d, b" ");
    m.put_cstr(0x08699e78, b"%s %s.%s = %s");
    m.put_cstr(0x08699e86, b"Expression invalide(%s)!\n");
    // the ninth pair is the code 8 with a null text; errorMsgSize says 8 pairs are looked at
    m.set_i32(EXPR_ERROR_LIST + 0x40, 8);
    // the second 16-bit mode flag after si_checkExprSyntaxOnly (finding 0013, section 1)
    m.set_i16(0x08699ece, 1);
}

/// The chapter ids of the scene dictionary (`SI_cid`, 0x08781660), word by word.
pub mod cid {
    pub const BASE: u32 = 0x0878_1660;
    pub const CAM: u32 = BASE + 0x04;
    pub const CLS: u32 = BASE + 0x08;
    pub const DSC: u32 = BASE + 0x0c;
    pub const LAT: u32 = BASE + 0x14;
    pub const LIG: u32 = BASE + 0x18;
    pub const MAT: u32 = BASE + 0x1c;
    pub const MODEL: u32 = BASE + 0x20;
    pub const SHP: u32 = BASE + 0x54;
    pub const T2D: u32 = BASE + 0x68;
    pub const T3D: u32 = BASE + 0x6c;
    pub const WAV: u32 = BASE + 0x70;
    pub const EXP: u32 = BASE + 0x74;
    pub const CHN_DRV: u32 = BASE + 0x78;
}

/// The current element dictionary: `SI_elemDicts + *SI_procData × 0x70`.
pub fn cur_dict(m: &si_mem::Mem) -> Ptr32<si_types::dict::ElemDict> {
    let index = m.i32(m.u32(si_dict::globals::SI_PROC_DATA.addr()));
    Ptr32::new(m.u32(si_dict::globals::SI_ELEM_DICTS.addr()).wrapping_add((index as u32).wrapping_mul(0x70)))
}

/// A chapter id of `SI_cid`.
pub fn chap(m: &si_mem::Mem, which: u32) -> i32 {
    m.i32(which)
}
