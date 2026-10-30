// SPDX-License-Identifier: MIT
//! The parser of the expression language (`exp_yacc.c`) and the functions that drive it
//! (`exp_parsing.c`), finding 0013.
//!
//! The original parser is generated (bison 1.x, the classic skeleton)
//! [soft 0x0824f740 EXP_yyparse]. Its grammar was recovered from the tables and is proved
//! equal to them by `re/tools/si-expr/yacc_check.py`, which also writes
//! [`crate::yacc_tables`]. The driver below is the skeleton's algorithm: default reductions
//! without a look at the next token, the same error recovery over the two `error` rules, the
//! same stack limit. The 80 actions follow the action switch of the original.
//!
//! The parser runs in two modes. With "syntax only" set it builds nothing: every allocation
//! answers a null pointer and the actions only check. Otherwise it builds the tree. The
//! callers validate a text in the first mode and build in the second.

// adapter: not in place; the parser builds value trees (`crate::tree`); the image has them only through the copy
// of `crate::image::tree`, which does not make the actions' allocation calls in their order.

use crate::env::Dict;
use crate::lex::{tok, LexUndefined, Lval, Scanner, Sem};
use crate::token::{self, chapter};
use crate::tree::*;
use crate::util::rt;
use crate::yacc_tables as yt;

/// Where the original parser has no defined behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Undefined {
    /// See [`LexUndefined`].
    Lex(LexUndefined),
    /// An action uses, as a pointer to a node, the semantic value of an `error` token (a
    /// copy of whatever the scanner's value variable last held) or a pointer already freed.
    /// Reached only when a tree is built from a text with a syntax error.
    StalePointer(u8),
    /// A reference whose element was not looked up reaches a function that reads the scene
    /// through the element's record.
    MissingElement,
}

impl From<LexUndefined> for Undefined {
    fn from(e: LexUndefined) -> Self {
        Undefined::Lex(e)
    }
}

/// The default text of the error routine when it is given a null message (0x0869afd3: an
/// empty string).
const EMPTY: &str = "";

/// One variable as the parser's callers hand it over (`SI_ExpVarDef`, 8 bytes): a name and
/// the text of its definition.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VarDef {
    pub name: Vec<u8>,
    pub text: Option<Vec<u8>>,
}

/// The parser and everything the original keeps in globals around it.
pub struct Parser {
    pub scanner: Scanner,
    /// `si_checkExprSyntaxOnly` (0x08699ecc): check, build nothing.
    pub check_syntax_only: i16,
    /// `si_validateArgs` (0x08699ece): look referenced elements up in the scene.
    pub validate_args: i16,
    /// `si_loadingMode` (0x08699ed0): references take their numbers from an argument list.
    pub loading_mode: i16,
    /// `si_parserGenAssNode` (0x08699ed2): a text that starts with `=` yields an assignment
    /// node with no target instead of the bare right-hand side.
    pub gen_ass_node: i16,
    /// `si_parseVariableAccepted` (0x08699ed4).
    pub variable_accepted: i16,
    /// `si_acceptFcv2Fcv` (0x08699ed6): set by the callers, not read by the parser.
    pub accept_fcv2fcv: i16,
    /// `si_checkNewObjOnly` (0x08699ed8).
    pub check_new_obj_only: i16,
    /// `si_hasThisToken` (0x08699eda): a reference to `this` was seen.
    pub has_this_token: i16,
    /// `si_IncompatibleFcv` (0x08699edc).
    pub incompatible_fcv: i16,
    /// `SI_queryMode` (0x08699ec0).
    pub query_mode: i16,
    /// `argNumCurVal` (0x08699ec8): the last argument number handed out.
    pub arg_num_cur_val: i32,
    /// `glob_arg_list` (0x08699ec4): the argument names of the expression being loaded.
    pub glob_arg_list: Option<Vec<Vec<u8>>>,
    /// `parserVarsUsed` / `parserNbVarsUsed`: the variables a main text may name.
    pub vars_used: Vec<VarDef>,
    /// `error_occurred` (0x0869ad80).
    pub error_occurred: i16,
    /// `last_error_msg` (0x0869ad84).
    pub last_error_msg: String,
    /// `rand_counter` (0x0869ad88): random calls seen in this text.
    rand_counter: i32,
    /// `parsingAffectedObj` (0x0878fe88): the parser is still in the target of the
    /// statement.
    parsing_affected_obj: i16,
    /// `fctArgStack` and `topOfTheStack` (0x0869ad8c, 0x0869ad90): argument counts of the
    /// function calls being parsed, in syntax-only mode.
    fct_arg_stack: Vec<i32>,
    /// `returnedParsingExpTree` (0x0878fd60).
    pub returned: Option<Box<ExprNode>>,
    /// `EXP_yynerrs`.
    pub nerrs: i32,
    /// What the original prints on standard output (`Invalid arg …`).
    pub stdout: Vec<u8>,
}

impl Default for Parser {
    fn default() -> Self {
        Parser::new()
    }
}

/// One slot of the value stack.
type Slot = Lval;

struct Stack {
    states: Vec<i16>,
    values: Vec<Slot>,
}

impl Stack {
    /// `yyvsp[n]`: n = 0 is the top of the stack.
    fn v(&mut self, n: isize) -> &mut Slot {
        let i = (self.values.len() as isize - 1 + n) as usize;
        &mut self.values[i]
    }
}

fn take(s: &mut Sem) -> Sem {
    std::mem::replace(s, Sem::Stale)
}

fn int_of(s: &Sem) -> i32 {
    match s {
        Sem::Int(v) => *v,
        Sem::Float(f) => f.to_bits() as i32,
        _ => 0,
    }
}

fn float_of(s: &Sem) -> f32 {
    match s {
        Sem::Float(f) => *f,
        Sem::Int(v) => f32::from_bits(*v as u32),
        _ => 0.0,
    }
}

impl Parser {
    /// The globals as the program's data section has them at start-up.
    pub fn new() -> Parser {
        Parser {
            scanner: Scanner::new(),
            check_syntax_only: 1,
            validate_args: 1,
            loading_mode: 0,
            gen_ass_node: 1,
            variable_accepted: 1,
            accept_fcv2fcv: 1,
            check_new_obj_only: 0,
            has_this_token: 0,
            incompatible_fcv: 0,
            query_mode: 0,
            arg_num_cur_val: 0,
            glob_arg_list: None,
            vars_used: Vec::new(),
            error_occurred: 0,
            last_error_msg: String::new(),
            rand_counter: 0,
            parsing_affected_obj: 0,
            fct_arg_stack: Vec::new(),
            returned: None,
            nerrs: 0,
            stdout: Vec::new(),
        }
    }

    /// Records an error [soft 0x08250960 EXP_yyerror]: sets the flag and the message and
    /// keeps (or sets) the error position.
    pub fn yyerror(&mut self, msg: &str) {
        self.error_occurred = 1;
        self.last_error_msg = msg.to_string();
        self.scanner.keep_error_ptr();
    }

    /// The message of the last error [soft 0x08250994 SI_expGetParsingErrorMsg].
    pub fn parsing_error_msg(&self) -> &str {
        if self.last_error_msg.is_empty() {
            EMPTY
        } else {
            &self.last_error_msg
        }
    }

    /// The next argument number [soft 0x0824e17c si_getArgNextNum].
    fn arg_next_num(&mut self) -> i32 {
        self.arg_num_cur_val += 1;
        self.arg_num_cur_val
    }

    /// The place of `name` in the argument list of the expression being loaded
    /// [soft 0x0824e120 si_getArgNumFromList]; −1 and a line on standard output if it is not
    /// there. (A null list would fault in the original; Rust answers −1.)
    pub fn arg_num_from_list(&mut self, name: &[u8]) -> i32 {
        if let Some(list) = &self.glob_arg_list {
            if let Some(i) = list.iter().position(|a| a == name) {
                return i as i32;
            }
        }
        self.stdout.extend_from_slice(b"Invalid arg ");
        self.stdout.extend_from_slice(name);
        self.stdout.push(b'\n');
        -1
    }

    /// The number a reference to `elem` gets while loading, −1 otherwise
    /// [soft 0x0825153c setArgsNum].
    fn set_args_num(&mut self, elem: &[u8]) -> i32 {
        if self.loading_mode == 0 {
            -1
        } else {
            self.arg_num_from_list(elem)
        }
    }

    /// Whether `name` is a variable with a non-empty definition
    /// [soft 0x0824b0b4 si_isVarUsed].
    pub fn is_var_used(&self, name: &[u8]) -> bool {
        self.vars_used.iter().any(|v| v.name == name && v.text.as_deref().is_some_and(|t| !t.is_empty()))
    }

    /// A tree node, or null in syntax-only mode [soft 0x082509a0 AllocExprNode].
    fn alloc_expr_node(&self, left: Sem, right: Sem, oper: u32) -> Result<Sem, Undefined> {
        if self.check_syntax_only != 0 {
            return Ok(Sem::Null);
        }
        let node = |s: Sem, what: u8| -> Result<Option<Box<ExprNode>>, Undefined> {
            match s {
                Sem::Node(n) => Ok(Some(n)),
                s if s.is_null() => Ok(None),
                _ => Err(Undefined::StalePointer(what)),
            }
        };
        Ok(Sem::Node(Box::new(if oper == op::LEAF {
            match left {
                Sem::Nop(n) => ExprNode { oper, leaf: Some(n), left: None, right: None },
                s if s.is_null() => ExprNode { oper, leaf: None, left: None, right: None },
                _ => return Err(Undefined::StalePointer(1)),
            }
        } else {
            ExprNode::new(oper, node(left, 2)?, node(right, 3)?)
        })))
    }

    /// A leaf record of the given kind with its own record zeroed, or null in syntax-only
    /// mode [soft 0x082509dc AllocExpr]. A reference record takes the next argument number.
    fn alloc_expr(&mut self, kind: i32) -> Sem {
        if self.check_syntax_only != 0 {
            return Sem::Null;
        }
        let data = match kind {
            nop::ENUM => NopData::Enum(EnumNode { enum_type: 0, value: 0 }),
            nop::RAND => NopData::Rand(RandNode { form: 0, index: 0, seed: 0, real: 0, low: 0.0, width: 0.0 }),
            nop::ARG | nop::CUSTOM_ARG => NopData::Arg(ArgNode {
                num: self.arg_next_num(),
                generic: 0,
                chapter: 0,
                elem: 0,
                fcv: 0,
                access0: 0,
                access1: 0,
                access_name: None,
                custom_name: None,
            }),
            nop::FCT => NopData::Fct(FctNode { index: 0, args: Vec::new() }),
            nop::CHANNEL => NopData::Channel(ChannelNode { driver: 0, dof: 0 }),
            _ => NopData::None,
        };
        Sem::Nop(Box::new(NopNode { kind, data }))
    }

    /// Appends an argument to a function call, or counts it in syntax-only mode
    /// [soft 0x08250a74 addAnArgToFct].
    fn add_an_arg_to_fct(&mut self, fct: Sem, arg: Sem) -> Result<Sem, Undefined> {
        if self.check_syntax_only != 0 {
            // an empty stack would be written in front of its block in the original
            if let Some(top) = self.fct_arg_stack.last_mut() {
                *top += 1;
            }
            return Ok(Sem::Null);
        }
        let Sem::Nop(mut n) = fct else { return Err(Undefined::StalePointer(4)) };
        let NopData::Fct(f) = &mut n.data else { return Err(Undefined::StalePointer(5)) };
        f.args.push(match arg {
            Sem::Node(a) => Some(a),
            s if s.is_null() => None,
            _ => return Err(Undefined::StalePointer(6)),
        });
        Ok(Sem::Nop(n))
    }

    /// Checks a reference `elem.param` of `chapter` and fills in what it names
    /// [soft 0x08250b10 validElement]: the element's index, the fcurve type, and for shape
    /// and cluster parameters the pieces after the underscore. False after an error.
    #[allow(clippy::too_many_arguments)]
    fn valid_element(
        &mut self,
        dict: &dyn Dict,
        chap: i32,
        elem: &[u8],
        param: &[u8],
        elemid: &mut i32,
        fcv: &mut i32,
        sub: &mut i32,
        clus_name: &mut Option<Vec<u8>>,
        custom: &mut Option<Vec<u8>>,
    ) -> bool {
        // the parameter up to its first underscore, and whole
        let mut strip: Vec<u8> = Vec::new();
        let mut nostrip: Vec<u8> = Vec::new();
        if !param.is_empty() {
            // sscanf "%[^_]": fails when the text starts with an underscore
            strip = param.iter().copied().take_while(|&c| c != b'_').collect();
            if strip.is_empty() {
                self.yyerror("Unknown parameter");
                self.scanner.set_error_ptr(param);
                return false;
            }
            nostrip = param.to_vec();
        } else {
            *fcv = 0;
        }
        if self.validate_args == 0 {
            // only the first four bytes are compared: `thisway` is `this`
            if elem.len() >= 4 && &elem[..4] == b"this" {
                *elemid = -1;
                self.has_this_token = 1;
            }
        } else if elem == b"this" {
            *elemid = -1;
            self.has_this_token = 1;
        } else {
            *elemid = match elem.iter().position(|&c| c == b'-') {
                Some(d) => dict.find_elem(chap, Some(&elem[..d]), &elem[d + 1..], self.check_new_obj_only),
                None => dict.find_elem(chap, None, elem, self.check_new_obj_only),
            };
            if *elemid == -1 {
                self.yyerror("Unknown element");
                self.scanner.set_error_ptr(elem);
                return false;
            }
        }
        if param.is_empty() {
            *fcv = 0;
        } else {
            let mut found = false;
            // A custom effect's icon (a model of type 13) can have parameters of its own.
            // Finding them needs the model's record and the effect's attribute group: not
            // ported. Rust takes the path of an element that is not such an icon.
            let is_cus = nostrip.len() > 3 && nostrip.starts_with(b"CUS");
            if !is_cus {
                *fcv = dict.fcurve_from_name(&strip);
                if *fcv != -1 {
                    found = true;
                }
            }
            if !found && self.validate_args == 0 {
                *fcv = FCV_CUSTOM;
                found = true;
                *custom = Some(nostrip.clone());
            }
            if !found {
                self.yyerror("Unknown parameter");
                self.scanner.set_error_ptr(param);
                return false;
            }
        }
        if self.query_mode != 0 && *elemid == -1 {
            return true;
        }
        if chap == chapter::MODELS && matches!(*fcv, 0x9a | 8 | 0xb7 | 0xb8) {
            if self.validate_args != 0 && !dict.elem_exists(chap, *elemid) {
                return false;
            }
            match *fcv {
                0x9a => {
                    // With validation the model's interpolation mode decides whether the
                    // weighted form is allowed at all (not ported: Rust reads the text only).
                    // sscanf "%[^_]_%d"
                    let rest = &param[strip.len()..];
                    let n = if rest.first() == Some(&b'_') { parse_c_int(&rest[1..]) } else { None };
                    match n {
                        Some(v) => {
                            *sub = v;
                            if self.validate_args == 0 {
                                return true;
                            }
                        }
                        None => {
                            self.yyerror(rt(0x0869b140));
                            self.scanner.set_error_ptr(param);
                            return false;
                        }
                    }
                }
                8 => {
                    if self.validate_args == 0 {
                        return true;
                    }
                }
                _ => {
                    // sscanf "%[^_]_%s": the cluster's name is what follows the underscore
                    let rest = &param[strip.len()..];
                    let mut name: Vec<u8> =
                        if rest.first() == Some(&b'_') { rest[1..].to_vec() } else { Vec::new() };
                    let mut n = if name.is_empty() { 1 } else { 2 };
                    if n == 2 && *fcv == 0xb8 {
                        // a trailing `_<digits>` is the weight's number
                        let digits = name.iter().rev().take_while(|c| c.is_ascii_digit()).count();
                        let p = name.len() - digits;
                        if digits > 0 && p > 0 && name[p - 1] == b'_' {
                            *sub = parse_c_int(&name[p..]).unwrap_or(0);
                            name.truncate(p - 1);
                            n = 3;
                        }
                    }
                    if !((n == 2 && *fcv == 0xb7) || (n == 3 && *fcv == 0xb8)) {
                        self.yyerror(rt(0x0869b040));
                        self.scanner.set_error_ptr(param);
                        return false;
                    }
                    if self.validate_args == 0 {
                        *clus_name = Some(name);
                    }
                    // With validation the cluster is looked up in the model
                    // [soft 0x0824e194 si_expFindValidCluster]: needs the model, not ported.
                }
            }
        }
        true
    }

    /// One parse of the string the scanner holds [soft 0x0824f740 EXP_yyparse]: 0 accepted,
    /// 1 given up, 2 the stack limit.
    pub fn yyparse(&mut self, dict: &dyn Dict) -> Result<i32, Undefined> {
        const EMPTY_TOKEN: i32 = -2;
        const MAXDEPTH: usize = 10000;
        let mut st = Stack { states: Vec::new(), values: Vec::new() };
        let mut state: i16 = 0;
        let mut errstatus = 0;
        let mut yychar = EMPTY_TOKEN;
        // the skeleton's `yyval` keeps its value from one reduction to the next: an empty
        // rule without an action hands on what the previous reduction produced
        let mut yyval = Slot::default();
        self.nerrs = 0;
        st.states.push(0);
        st.values.push(Slot::default());
        'newstate: loop {
            if st.states.len() >= MAXDEPTH {
                self.yyerror(rt(0x0869ad94));
                return Ok(2);
            }
            // what to do in this state
            let mut reduce: i32 = 0;
            let mut error = false;
            let row = yt::ACTIONS[state as usize];
            if row.is_empty() {
                reduce = yt::DEFACT[state as usize] as i32;
                error = reduce == 0;
            } else {
                if yychar == EMPTY_TOKEN {
                    yychar = self.scanner.yylex(dict)?;
                }
                let sym = if yychar <= 0 {
                    yychar = 0;
                    0u8
                } else if yychar <= 301 {
                    yt::TRANSLATE[yychar as usize]
                } else {
                    yt::UNKNOWN
                };
                match row.iter().find(|e| e.0 == sym) {
                    None => {
                        reduce = yt::DEFACT[state as usize] as i32;
                        error = reduce == 0;
                    }
                    Some(&(_, v)) if v < 0 => reduce = -v as i32,
                    Some(&(_, 0)) => error = true,
                    Some(&(_, v)) => {
                        if v == yt::FINAL {
                            return Ok(0);
                        }
                        if yychar != 0 {
                            yychar = EMPTY_TOKEN;
                        }
                        st.values.push(self.scanner.lval.shift_copy());
                        if errstatus != 0 {
                            errstatus -= 1;
                        }
                        state = v;
                        st.states.push(state);
                        continue 'newstate;
                    }
                }
            }
            if error {
                if errstatus == 0 {
                    self.nerrs += 1;
                    self.yyerror("parse error");
                }
                if errstatus == 3 {
                    if yychar == 0 {
                        return Ok(1);
                    }
                    yychar = EMPTY_TOKEN;
                }
                errstatus = 3;
                loop {
                    if let Some(&(_, v)) = yt::ACTIONS[state as usize].iter().find(|e| e.0 == 1) {
                        if v < 0 {
                            reduce = -v as i32;
                            break;
                        }
                        if v > 0 {
                            if v == yt::FINAL {
                                return Ok(0);
                            }
                            // the `error` token's value is a copy of the scanner's value
                            // variable, whose owner is (or will be) the token it belongs to
                            let l = &self.scanner.lval;
                            st.values.push(Slot { a: stale_copy(&l.a), b: stale_copy(&l.b), gl: l.gl });
                            state = v;
                            st.states.push(state);
                            continue 'newstate;
                        }
                    }
                    if st.states.len() == 1 {
                        return Ok(1);
                    }
                    st.states.pop();
                    st.values.pop();
                    state = *st.states.last().unwrap();
                }
            }
            // reduce
            let n = reduce as usize;
            let len = yt::R2[n] as usize;
            if len > 0 {
                // a bit copy of the first right-hand value: owned things stay where they are
                // until an action takes them
                let first = &st.values[st.values.len() - len];
                yyval = Slot { a: stale_copy(&first.a), b: stale_copy(&first.b), gl: first.gl };
            }
            if let Some(code) = self.action(dict, n, len, &mut st, &mut yyval)? {
                return Ok(code);
            }
            for _ in 0..len {
                st.states.pop();
                st.values.pop();
            }
            let top = *st.states.last().unwrap();
            let lhs = yt::R1[n];
            state = yt::GOTOS[top as usize]
                .iter()
                .find(|g| g.0 == lhs)
                .map(|g| g.1 as i16)
                .expect("the automaton has this transition");
            st.states.push(state);
            st.values.push(Slot { a: take_keep(&mut yyval.a), b: take_keep(&mut yyval.b), gl: yyval.gl });
        }
    }

    /// The action of rule `n`. `Some(code)` makes the parser return at once.
    fn action(
        &mut self,
        dict: &dyn Dict,
        n: usize,
        len: usize,
        st: &mut Stack,
        yyval: &mut Slot,
    ) -> Result<Option<i32>, Undefined> {
        let syntax_only = self.check_syntax_only != 0;
        // `$k` of a rule with `len` right-hand symbols
        let d = |k: usize| k as isize - len as isize;
        match n {
            1 => {
                self.error_occurred = 0;
                self.last_error_msg.clear();
                self.rand_counter = 0;
                self.parsing_affected_obj = 1;
                self.scanner.init_error_ptr();
            }
            2 => {
                if self.error_occurred != 0 {
                    return Ok(Some(1));
                }
            }
            3 => {
                if self.error_occurred != 0 {
                    return Ok(Some(1));
                }
                self.parsing_affected_obj = 0;
            }
            5 => self.parsing_affected_obj = 0,
            4 => {
                if self.error_occurred == 0 && !syntax_only {
                    let target = take(&mut st.v(d(1)).a);
                    let oper = int_of(&st.v(d(2)).a) as u32;
                    let rhs = take(&mut st.v(d(4)).a);
                    self.set_returned(self.alloc_expr_node(target, rhs, oper)?);
                }
            }
            6 => {
                if self.error_occurred == 0 && !syntax_only {
                    let rhs = take(&mut st.v(d(3)).a);
                    if self.gen_ass_node == 0 {
                        self.set_returned(rhs);
                    } else {
                        self.set_returned(self.alloc_expr_node(Sem::Null, rhs, op::ASSIGN)?);
                    }
                }
            }
            7..=11 => yyval.a = Sem::Int(n as i32 - 6),
            12 => {
                let r = take(&mut st.v(d(1)).a);
                yyval.a = if r.is_null() { Sem::Null } else { self.alloc_expr_node(r, Sem::Null, op::LEAF)? };
            }
            14 => return self.reference(dict, st, yyval),
            15 => yyval.a = Sem::Int(chapter::MODELS),
            16 => {
                let (t, id) = (int_of(&st.v(0).a), int_of(&st.v(0).b));
                let Some(code) = dict.enum_code_by_value(t, id) else { return Ok(Some(1)) };
                let chap = token::get_chapid_from_text(&code);
                if chap == -1 {
                    self.yyerror("Invalid Chapter Id");
                    self.scanner.set_error_ptr(&code);
                    return Ok(Some(1));
                }
                yyval.a = Sem::Int(chap);
            }
            17 => {
                let p = take(&mut st.v(0).a);
                yyval.a = self.alloc_expr_node(p, Sem::Null, op::LEAF)?;
            }
            18 => {
                yyval.a = take(&mut st.v(-1).a);
                if !syntax_only {
                    match &mut yyval.a {
                        Sem::Node(node) => node.oper |= PAREN,
                        _ => return Err(Undefined::StalePointer(7)),
                    }
                }
            }
            // the value on top of the stack is the result
            19 | 27 | 29 | 48 | 65 | 66 | 67 | 70 | 73 | 75 | 76 | 78 => yyval.a = take(&mut st.v(0).a),
            20..=25 => {
                let oper = [op::ADD, op::SUB, op::MUL, op::DIV, op::MOD, op::CONTAINS][n - 20];
                let (l, r) = (take(&mut st.v(-2).a), take(&mut st.v(0).a));
                yyval.a = self.alloc_expr_node(l, r, oper)?;
            }
            26 => {
                if !syntax_only {
                    // A constant written without parentheses is negated in place.
                    let mut operand = take(&mut st.v(0).a);
                    let folded = match &mut operand {
                        Sem::Node(node) if node.oper == op::LEAF => match node.leaf.as_deref_mut() {
                            Some(NopNode { kind: nop::FLOAT, data: NopData::Float(f) }) => {
                                *f = -*f;
                                true
                            }
                            Some(NopNode { kind: nop::INT, data: NopData::Int(i) }) => {
                                *i = i.wrapping_neg();
                                true
                            }
                            Some(_) => false,
                            None => return Err(Undefined::StalePointer(8)),
                        },
                        Sem::Node(_) => false,
                        _ => return Err(Undefined::StalePointer(9)),
                    };
                    yyval.a = if folded { operand } else { self.alloc_expr_node(operand, Sem::Null, op::NEG)? };
                }
            }
            30 => {
                if !syntax_only {
                    let text = take(&mut st.v(0).a);
                    let mut node = self.alloc_expr(nop::STRING);
                    if let (Sem::Nop(nd), Sem::Text(t)) = (&mut node, text) {
                        nd.data = NopData::Text(Some(t));
                    }
                    yyval.a = node;
                } else if !st.v(0).a.is_null() {
                    // the text is freed; the result keeps the old pointer
                    st.v(0).a = Sem::Null;
                    yyval.a = Sem::Stale;
                }
            }
            31 => {
                let mut node = self.alloc_expr(nop::ENUM);
                if let Sem::Nop(nd) = &mut node {
                    nd.data = NopData::Enum(EnumNode { enum_type: int_of(&st.v(0).a), value: int_of(&st.v(0).b) });
                }
                yyval.a = node;
            }
            32 | 79 => {
                let mut node = self.alloc_expr(if n == 32 { nop::RELATION } else { nop::INT });
                if let Sem::Nop(nd) = &mut node {
                    nd.data = NopData::Int(int_of(&st.v(0).a));
                }
                yyval.a = node;
            }
            33..=44 => {
                let kind = [4, 5, 6, 7, 14, 15, 16, 17, 18, 19, 21, 22][n - 33];
                yyval.a = self.alloc_expr(kind);
            }
            45 => {
                let mut node = self.alloc_expr(nop::RAND);
                if let Sem::Nop(nd) = &mut node {
                    let seed = int_of(&st.v(-1).a);
                    let index = self.rand_counter as i16;
                    self.rand_counter += 1;
                    let which = int_of(&st.v(-3).a);
                    let mut r = RandNode { form: 0, index, seed, real: 0, low: 0.0, width: 0.0 };
                    if which == tok::RANDBOOL {
                        r.form = 1;
                        r.real = 0;
                        r.width = 1.0;
                    } else if which == tok::RAND01 {
                        r.form = 2;
                        r.real = 1;
                        r.width = 1.0;
                    }
                    nd.data = NopData::Rand(r);
                }
                yyval.a = node;
            }
            46 => {
                let mut node = self.alloc_expr(nop::RAND);
                if let Sem::Nop(nd) = &mut node {
                    let index = self.rand_counter as i16;
                    let seed = int_of(&st.v(-1).a);
                    self.rand_counter += 1;
                    nd.data = NopData::Rand(RandNode { form: 0, index, seed, real: 0, low: 0.0, width: 0.0 });
                }
                yyval.a = node;
            }
            // RAND( seed , <node> range ): the range rule hands the node on
            47 | 62 if !(n == 62 && syntax_only) => yyval.a = take(&mut st.v(-1).a),
            62 => {
                let count = self.fct_arg_stack.pop().unwrap_or(0);
                yyval.a = Sem::Count(count);
            }
            50 => return self.ident(st, yyval),
            51..=58 => {
                let (l, r) = (take(&mut st.v(-2).a), take(&mut st.v(0).a));
                yyval.a = self.alloc_expr_node(l, r, op::EQ + (n as u32 - 51))?;
            }
            59 => {
                let operand = take(&mut st.v(0).a);
                yyval.a = self.alloc_expr_node(operand, Sem::Null, op::NOT)?;
            }
            60 => yyval.a = self.alloc_expr(nop::VAR),
            61 => {
                if syntax_only {
                    self.fct_arg_stack.push(0);
                } else {
                    yyval.a = self.alloc_expr(nop::FCT);
                }
            }
            63 => {
                // the call's record is the value under this rule's symbols
                let (fct, arg) = (take(&mut st.v(-1).a), take(&mut st.v(0).a));
                yyval.a = self.add_an_arg_to_fct(fct, arg)?;
            }
            64 => {
                let (fct, arg) = (take(&mut st.v(-2).a), take(&mut st.v(0).a));
                yyval.a = self.add_an_arg_to_fct(fct, arg)?;
            }
            68 | 69 => yyval.a = Sem::Int(-1),
            71 | 72 => {
                // the random record made by the action in the middle of the RAND rule
                let mut node = take(&mut st.v(-3).a);
                if !syntax_only {
                    let Sem::Nop(nd) = &mut node else { return Err(Undefined::StalePointer(10)) };
                    let NopData::Rand(r) = &mut nd.data else { return Err(Undefined::StalePointer(11)) };
                    if n == 71 {
                        let (lo, hi) = (float_of(&st.v(-2).a), float_of(&st.v(0).a));
                        r.real = 1;
                        r.low = lo;
                        // one subtraction of two floats, stored to a float
                        r.width = hi - lo;
                    } else {
                        let (lo, hi) = (int_of(&st.v(-2).a), int_of(&st.v(0).a));
                        r.real = 0;
                        r.low = lo as f32;
                        r.width = hi.wrapping_sub(lo) as f32;
                    }
                }
                yyval.a = node;
            }
            74 => yyval.a = Sem::Float(-float_of(&st.v(0).a)),
            77 => yyval.a = Sem::Int(int_of(&st.v(0).a).wrapping_neg()),
            80 => {
                let mut node = self.alloc_expr(nop::FLOAT);
                if let Sem::Nop(nd) = &mut node {
                    nd.data = NopData::Float(float_of(&st.v(0).a));
                }
                yyval.a = node;
            }
            // 13, 28 (the two `error` rules) and 49 have no action
            _ => {}
        }
        Ok(None)
    }

    /// `returnedParsingExpTree = node` [soft 0x0824c820 si_setReturnedExpression].
    fn set_returned(&mut self, node: Sem) {
        self.returned = match node {
            Sem::Node(n) => Some(n),
            _ => None,
        };
    }

    /// Rule 14: `chapter ELEMPARAM`, a reference to a parameter of an element or to a
    /// channel.
    fn reference(&mut self, dict: &dyn Dict, st: &mut Stack, yyval: &mut Slot) -> Result<Option<i32>, Undefined> {
        let syntax_only = self.check_syntax_only != 0;
        let chap = int_of(&st.v(-1).a);
        let gl = st.v(0).gl;
        let (Sem::Text(elem), Sem::Text(param)) = (take(&mut st.v(0).a), take(&mut st.v(0).b)) else {
            return Err(Undefined::StalePointer(12));
        };
        if gl != b'D' && chap != chapter::MODELS {
            self.yyerror(rt(0x0869adc0));
        }
        if chap == chapter::SETUP_CHANNELS {
            let Some((driver, dof)) = dict.chn_match_names(&elem, &param) else {
                self.yyerror(rt(0x0869ae00));
                return Ok(Some(1));
            };
            let kind = dict.chn_dof_type(driver, dof);
            if self.parsing_affected_obj == 0 {
                if kind == 2 {
                    self.yyerror(rt(0x0869ae38));
                    return Ok(Some(1));
                }
            } else if kind == 1 {
                self.yyerror(rt(0x0869ae20));
                return Ok(Some(1));
            }
            yyval.a = if syntax_only {
                Sem::Null
            } else {
                let mut node = self.alloc_expr(nop::CHANNEL);
                if let Sem::Nop(nd) = &mut node {
                    nd.data = NopData::Channel(ChannelNode { driver, dof });
                }
                node
            };
            return Ok(None);
        }
        let mut kind = nop::ARG;
        let (mut elemid, mut fcv, mut sub, clus) = (-2, 0, -1, -1);
        let (mut clus_name, mut custom) = (None, None);
        let ok = self.valid_element(dict, chap, &elem, &param, &mut elemid, &mut fcv, &mut sub, &mut clus_name, &mut custom);
        if ok
            && gl != b'D'
            && chap == chapter::MODELS
            && !(139..=141).contains(&fcv)
            && !(1..=6).contains(&fcv)
        {
            self.yyerror(rt(0x0869ae60));
        }
        if fcv == FCV_CUSTOM {
            kind = nop::CUSTOM_ARG;
        }
        if self.error_occurred == 1 {
            return Ok(Some(1));
        }
        if !syntax_only {
            let num = self.set_args_num(&elem);
            let mut node = self.alloc_expr(kind);
            if let Sem::Nop(nd) = &mut node {
                if let NopData::Arg(a) = &mut nd.data {
                    if fcv == FCV_CUSTOM {
                        // a null name would fault in `strdup`; it is set whenever the type is 566
                        a.custom_name = Some(custom.unwrap_or_default());
                    }
                    if num != -1 {
                        a.num = num;
                    }
                    a.chapter = chap;
                    // Camera and light interests are replaced by their camera or light
                    // `convertElemid` (0x08251484, not ported): a scene look-up, which finds nothing
                    // for an element that was not looked up.
                    a.elem = elemid;
                    a.fcv = fcv;
                    a.generic = (elemid == -1) as i16;
                    if fcv == 0x9a || fcv == 8 {
                        a.access0 = sub;
                    } else if fcv == 0xb7 || fcv == 0xb8 {
                        a.access0 = sub;
                        a.access1 = clus;
                        if self.validate_args == 0 {
                            a.access_name = clus_name;
                        }
                    } else {
                        a.access0 = (gl == b'G') as i32;
                    }
                }
            }
            yyval.a = node;
        } else {
            if self.fct_arg_stack.is_empty() && fcv == 0 {
                self.yyerror(rt(0x0869aea0));
                return Ok(Some(1));
            }
            yyval.a = Sem::Null;
        }
        Ok(None)
    }

    /// Rule 50: `IDENT call`: a function call or a variable.
    fn ident(&mut self, st: &mut Stack, yyval: &mut Slot) -> Result<Option<i32>, Undefined> {
        let syntax_only = self.check_syntax_only != 0;
        let name = match &st.v(-2).a {
            Sem::Text(t) => t.clone(),
            _ => return Err(Undefined::StalePointer(13)),
        };
        let index = token::get_fct_index(&name);
        let mut call = take(&mut st.v(0).a);
        let mut msg: Option<&str> = None;
        if call.is_null() {
            msg = if !syntax_only {
                Some("Parsing error...")
            } else if index < 0 {
                if self.variable_accepted == 0 {
                    Some(rt(0x0869af80))
                } else if self.is_var_used(&name) {
                    None
                } else {
                    Some(rt(0x0869afa9))
                }
            } else {
                Some(rt(0x0869af40))
            };
        } else {
            let nargs = match &call {
                Sem::Count(c) => Some(*c),
                Sem::Nop(nd) => match &nd.data {
                    NopData::Fct(f) => Some(f.args.len() as i32),
                    _ => None,
                },
                _ => return Err(Undefined::StalePointer(14)),
            };
            if let Some(nargs) = nargs {
                if index == -1 {
                    msg = Some("Unknown function");
                } else {
                    let want = token::FUNCTIONS[index as usize].nargs;
                    if want != -1 && want != nargs {
                        msg = Some(rt(0x0869af00));
                    }
                }
            }
        }
        if let Some(m) = msg {
            self.yyerror(m);
            self.scanner.set_error_ptr(&name);
        }
        if syntax_only || self.error_occurred != 0 {
            // the name is freed
            st.v(-2).a = Sem::Null;
            if self.error_occurred != 0 {
                return Ok(Some(1));
            }
            // the result keeps the freed pointer
            yyval.a = Sem::Stale;
            return Ok(None);
        }
        if let Sem::Nop(nd) = &mut call {
            match &mut nd.data {
                NopData::Fct(f) => f.index = index,
                _ if nd.kind == nop::VAR => nd.data = NopData::Text(Some(name)),
                _ => {}
            }
        }
        st.v(-2).a = Sem::Null;
        yyval.a = call;
        Ok(None)
    }

    /// Parses `text` and says where it went wrong [soft 0x0824b6dc invoke_parser]: `None` if
    /// the parser did not answer 1; otherwise the offset in `text` of the error position
    /// moved back over blanks (−1 if that runs off the front). The tree is in
    /// [`Parser::returned`].
    pub fn invoke_parser(&mut self, dict: &dyn Dict, text: &[u8]) -> Result<Option<isize>, Undefined> {
        self.returned = None;
        self.scanner.set_parsed_string(text);
        if self.yyparse(dict)? != 1 {
            return Ok(None);
        }
        let mut p = self.scanner.error_ptr() as isize;
        while p >= 0 && self.scanner.byte_at(p as usize) == b' ' {
            p -= 1;
        }
        Ok(Some(p))
    }

    /// Whether `text` in double quotes scans as one string token
    /// [soft 0x0824b74c SI_expValidString].
    pub fn valid_string(&mut self, dict: &dyn Dict, text: &[u8]) -> Result<bool, Undefined> {
        let mut s = vec![b'"'];
        s.extend_from_slice(text);
        s.push(b'"');
        self.scanner.set_parsed_string(&s);
        Ok(self.scanner.yylex(dict)? == tok::STRING)
    }

    /// Checks an expression and its variables without building anything
    /// [soft 0x0824b7c8 SI_validCustomExpress]. Each variable with a non-blank definition is
    /// parsed as `= definition`; then the main text. `Ok(None)` if all is well; otherwise
    /// the index of the variable at fault (−1 for the main text) and the offset of the
    /// error in that text.
    pub fn valid_custom_express(
        &mut self,
        dict: &dyn Dict,
        text: &[u8],
        vars: &[VarDef],
        validate_args: i16,
        accept_fcv2fcv: i16,
    ) -> Result<Option<(i32, isize)>, Undefined> {
        self.check_syntax_only = 1;
        self.validate_args = validate_args;
        self.loading_mode = 0;
        self.accept_fcv2fcv = accept_fcv2fcv;
        self.has_this_token = 0;
        self.arg_num_cur_val = 0;
        self.gen_ass_node = 0;
        self.variable_accepted = 0;
        for (i, v) in vars.iter().enumerate() {
            let Some(def) = &v.text else { continue };
            if def.iter().all(|&c| c == b' ') {
                continue;
            }
            let mut s = b"= ".to_vec();
            s.extend_from_slice(def);
            if let Some(at) = self.invoke_parser(dict, &s)? {
                // the offset in the definition: two bytes less than in the text parsed
                return Ok(Some((i as i32, at - 2)));
            }
        }
        self.vars_used = vars.to_vec();
        self.gen_ass_node = 1;
        self.variable_accepted = 1;
        Ok(self.invoke_parser(dict, text)?.map(|at| (-1, at)))
    }

    /// Builds the trees of an expression and its variables
    /// [soft 0x0824b9ac SI_genCusExprParsingTree]. With `arg_list` the references take their
    /// numbers from it ("loading mode"). The original does not look at the parser's results
    /// here: it is called after a clean check.
    pub fn gen_cus_expr_parsing_tree(
        &mut self,
        dict: &dyn Dict,
        text: &[u8],
        vars: &[VarDef],
        validate_args: i16,
        arg_list: Option<&[Vec<u8>]>,
    ) -> Result<Expression, Undefined> {
        let mut expr = Expression::default();
        self.check_syntax_only = 0;
        self.validate_args = validate_args;
        self.has_this_token = 0;
        self.glob_arg_list = arg_list.map(|l| l.to_vec());
        self.loading_mode = arg_list.is_some() as i16;
        self.arg_num_cur_val = 0;
        self.gen_ass_node = 0;
        self.variable_accepted = 0;
        for v in vars {
            let Some(def) = &v.text else { continue };
            if def.iter().all(|&c| c == b' ') {
                continue;
            }
            let mut s = b"= ".to_vec();
            s.extend_from_slice(def);
            self.invoke_parser(dict, &s)?;
            expr.vars.push(VarNode { name: v.name.clone(), expr: self.returned.take() });
        }
        self.gen_ass_node = 1;
        self.variable_accepted = 1;
        self.invoke_parser(dict, text)?;
        expr.root = self.returned.take();
        if self.has_this_token == 0 || self.query_mode != 0 {
            self.incompatible_fcv = 0;
        }
        // With a `this` reference the original now fills in the target's element for every
        // generic reference [soft 0x08248f98 si_exprExpandGenRef]: see `util`.
        Ok(expr)
    }
}

/// The bits of a value as a second holder sees them: numbers are numbers, pointers are
/// stale copies.
fn stale_copy(s: &Sem) -> Sem {
    match s {
        Sem::Text(_) | Sem::Nop(_) | Sem::Node(_) => Sem::Stale,
        other => other.clone(),
    }
}

/// Moves an owned value out for the push and leaves the stale bits behind; numbers stay.
fn take_keep(s: &mut Sem) -> Sem {
    match s {
        Sem::Text(_) | Sem::Nop(_) | Sem::Node(_) => std::mem::replace(s, Sem::Stale),
        other => other.clone(),
    }
}

/// `%d` of `sscanf`: optional sign and digits at the start of the text.
fn parse_c_int(s: &[u8]) -> Option<i32> {
    let sign = matches!(s.first(), Some(b'-' | b'+')) as usize;
    let digits = s[sign..].iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    Some(si_ascii::cnum::atoi(&s[..sign + digits]))
}
