// SPDX-License-Identifier: MIT
//! Unit tests on made-up input. Expected values were worked out by hand from the reading of
//! the original described in finding 0013; they pin the Rust to that reading.

use crate::env::Dict;
use crate::eval::{self, EvalArgs};
use crate::format::{self, ArgType};
use crate::lex::{tok, Scanner, Sem};
use crate::parse::{Parser, VarDef};
use crate::scene::{EmptyScene, EvalUndefined, Scene};
use crate::tree::*;
use crate::util::rt;
use crate::value::AttrValue;

/// A made-up dictionary: three fcurve names, one enumeration with element type codes, one
/// relation.
struct TestDict;

impl Dict for TestDict {
    fn enum_val_by_any_code(&self, enum_type: &mut i32, code: &[u8]) -> Option<i32> {
        let codes: [&[u8]; 3] = [b"MOD", b"CAM", b"SPOT"];
        *enum_type = if *enum_type == -1 { 0 } else { *enum_type + 1 };
        if *enum_type > 0 {
            return None;
        }
        codes.iter().position(|c| *c == code).map(|i| i as i32 + 10)
    }
    fn enum_code_by_value(&self, enum_type: i32, id: i32) -> Option<Vec<u8>> {
        if enum_type != 0 {
            return None;
        }
        Some(match id {
            10 => b"MOD".to_vec(),
            11 => b"CAM".to_vec(),
            12 => b"SPOT".to_vec(),
            _ => return None,
        })
    }
    fn rel_id_by_name(&self, name: &[u8]) -> i32 {
        if name == b"Parent" {
            110
        } else {
            -1
        }
    }
    fn rel_name(&self, id: i32) -> Vec<u8> {
        if id == 110 { b"Parent".to_vec() } else { b"unknown".to_vec() }
    }
    fn fcurve_from_name(&self, name: &[u8]) -> i32 {
        match name {
            b"etrnx" => 139,
            b"rotz" => 3,
            b"wshp" => 0x9a,
            b"clustr" => 0xb7,
            b"cw" => 0xb8,
            b"posx" => 40,
            _ => -1,
        }
    }
    fn name_from_fcurve(&self, fcv: i32) -> Vec<u8> {
        match fcv {
            139 => b"etrnx".to_vec(),
            3 => b"rotz".to_vec(),
            0x9a => b"wshp".to_vec(),
            0xb7 => b"clustr".to_vec(),
            0xb8 => b"cw".to_vec(),
            40 => b"posx".to_vec(),
            _ => b"notf".to_vec(),
        }
    }
}

fn tokens(text: &str) -> Vec<i32> {
    let mut s = Scanner::new();
    s.set_parsed_string(text.as_bytes());
    let mut out = Vec::new();
    loop {
        let t = s.yylex(&TestDict).unwrap();
        if t == 0 {
            return out;
        }
        out.push(t);
    }
}

#[test]
fn scanner_token_classes() {
    assert_eq!(tokens("12 .5 1.5 Fc abc \"s-1.x\" a.etrnx"), vec![tok::INT, tok::FLOAT, tok::FLOAT, tok::FC, tok::IDENT, tok::STRING, tok::ELEMPARAM]);
    assert_eq!(tokens("== != < <= > >= && || ! += -= *= /="), (282..=282).chain([285, 290, 291, 292, 293, 294, 295, 284, 286, 287, 288, 289]).collect::<Vec<_>>());
    assert_eq!(tokens("+-*/%(),.="), "+-*/%(),.=".bytes().map(|c| c as i32).collect::<Vec<_>>());
    // an enumeration code, a relation name, `ALL` and `LIKE`
    assert_eq!(tokens("SPOT Parent ALL LIKE"), vec![tok::ENUMVAL, tok::RELNAME, tok::ENUMVAL, tok::LIKE]);
    // an element type word that no enumeration has is its own token (which no rule takes)
    assert_eq!(tokens("LIG"), vec![279]);
    // an empty string is not a string: two quote characters
    assert_eq!(tokens("\"\""), vec![34, 34]);
    // an identifier cannot start with an underscore
    assert_eq!(tokens("_a"), vec![95, tok::IDENT]);
    // a digit string followed by letters with no point after: a number, then a word
    assert_eq!(tokens("1a"), vec![tok::INT, tok::IDENT]);
    // a dash inside a reference needs a later point to belong to it
    assert_eq!(tokens("a.b-c"), vec![tok::ELEMPARAM, 45, tok::IDENT]);
    assert_eq!(tokens("a-b.c"), vec![tok::ELEMPARAM]);
}

#[test]
fn scanner_reference_pieces() {
    let mut s = Scanner::new();
    s.set_parsed_string(b"box.G.etrnx 2.e1 3.x");
    assert_eq!(s.yylex(&TestDict), Ok(tok::ELEMPARAM));
    assert_eq!(s.lval.a, Sem::Text(b"box".to_vec()));
    assert_eq!(s.lval.b, Sem::Text(b"etrnx".to_vec()));
    assert_eq!(s.lval.gl, b'G');
    assert_eq!(s.yylex(&TestDict), Ok(tok::FLOAT));
    assert_eq!(s.lval.a, Sem::Float(20.0));
    // not a number all the way: element `3`, parameter `x`
    assert_eq!(s.yylex(&TestDict), Ok(tok::ELEMPARAM));
    assert_eq!(s.lval.a, Sem::Text(b"3".to_vec()));
    assert_eq!(s.lval.gl, b'D');
}

#[test]
fn scanner_keeps_unread_bytes_between_parses() {
    let mut s = Scanner::new();
    s.set_parsed_string(b"+2");
    assert_eq!(s.yylex(&TestDict), Ok(43));
    // the `2` was taken from the string to end the `+`; a new string does not flush it
    assert_eq!(s.pending(), b"2");
    s.set_parsed_string(b"3");
    assert_eq!(s.yylex(&TestDict), Ok(tok::INT));
    assert_eq!(s.lval.a, Sem::Int(23));
}

#[test]
fn a_refused_text_leaves_bytes_for_the_next_parse() {
    si_rt::skip_without_install!();
    // The unknown function makes the action return at once. To end the `)` the scanner had
    // already taken the `+` from the text; it stays in the scanner's buffer.
    let mut p = Parser::new();
    let r = p.valid_custom_express(&TestDict, b"a.etrnx = foo(1)+2", &[], 0, 1).unwrap();
    assert_eq!(r.map(|e| e.0), Some(-1));
    assert_eq!(p.last_error_msg, "Unknown function");
    assert_eq!(p.scanner.pending(), b"+");
    // The next check parses a variable as `= 3`, which now scans as `+= 3`: refused.
    let vars = [VarDef { name: b"A".to_vec(), text: Some(b"3".to_vec()) }];
    let r = p.valid_custom_express(&TestDict, b"a.etrnx = A", &vars, 0, 1).unwrap();
    assert_eq!(r.map(|e| e.0), Some(0));
    assert_eq!(p.last_error_msg, "parse error");
    // that parse was given up after the `+=` (the rule after the operator tests the error
    // flag); the scanner had taken the blank behind it, which harms nothing
    assert_eq!(p.scanner.pending(), b" ");
    assert_eq!(p.valid_custom_express(&TestDict, b"a.etrnx = A", &vars, 0, 1).unwrap(), None);
    // a plain syntax error does not leave bytes: the `error` rules let the parser read on
    let mut p = Parser::new();
    assert!(p.valid_custom_express(&TestDict, b"a.etrnx = 1 * *2", &[], 0, 1).unwrap().is_some());
    assert_eq!(p.scanner.pending(), b"");
}

fn check(text: &str, vars: &[VarDef]) -> Option<(i32, isize, String)> {
    let mut p = Parser::new();
    p.valid_custom_express(&TestDict, text.as_bytes(), vars, 0, 1).unwrap().map(|(v, at)| (v, at, p.last_error_msg.clone()))
}

fn build(text: &str, vars: &[VarDef], args: Option<&[Vec<u8>]>) -> Expression {
    let mut p = Parser::new();
    assert_eq!(p.valid_custom_express(&TestDict, text.as_bytes(), vars, 0, 1).unwrap(), None, "{text}");
    p.gen_cus_expr_parsing_tree(&TestDict, text.as_bytes(), vars, 0, args).unwrap()
}

fn rhs_text(e: &Expression) -> String {
    let root = e.root.as_deref().unwrap();
    String::from_utf8(format::si_get_expr_node_format(&TestDict, e, root.right.as_deref(), ArgType::Number, 0).unwrap().unwrap()).unwrap()
}

#[test]
fn parser_accepts_and_rejects() {
    si_rt::skip_without_install!();
    assert_eq!(check("a.etrnx = 1 + 2 * 3", &[]), None);
    assert_eq!(check("= 1", &[]), None);
    // a syntax error: the message is the generated parser's, the position is where the
    // scanner stood. At the end of the text that is behind the two blanks the scanner's
    // copy has and the caller's text has not: two bytes past the caller's terminator.
    assert_eq!(check("a.etrnx = 1 +", &[]), Some((-1, 15, "parse error".into())));
    assert_eq!(check("a.etrnx = ( 1", &[]), Some((-1, 15, "parse error".into())));
    // in the middle: the scanner has taken the offending token and one byte more, so the
    // position is that of the first byte it has not taken: the `2`, the `+`
    assert_eq!(check("a.etrnx = 1 + * 2", &[]), Some((-1, 16, "parse error".into())));
    assert_eq!(check("a.etrnx = 1 2 + 3", &[]), Some((-1, 14, "parse error".into())));
    assert_eq!(check("a.etrnx = foo( 1 )", &[]), Some((-1, 10, "Unknown function".into())));
    assert_eq!(check("a.etrnx = sin( 1, 2 )", &[]), Some((-1, 10, rt(0x0869af00).into())));
    assert_eq!(check("a.etrnx = sin", &[]), Some((-1, 10, rt(0x0869af40).into())));
    assert_eq!(check("a.etrnx = A", &[]), Some((-1, 10, rt(0x0869afa9).into())));
    let vars = [VarDef { name: b"A".to_vec(), text: Some(b"b.rotz ".to_vec()) }];
    assert_eq!(check("a.etrnx = A", &vars), None);
    // a variable may not name another
    let bad = [VarDef { name: b"A".to_vec(), text: Some(b"B + 1".to_vec()) }];
    assert_eq!(check("a.etrnx = A", &bad), Some((0, 0, rt(0x0869af80).into())));
    assert_eq!(check("a. = 1", &[]), Some((-1, 3, rt(0x0869aea0).into())));
    assert_eq!(check("CAM a.G.posx = 1", &[]).map(|e| e.2), Some(rt(0x0869adc0).into()));
    assert_eq!(check("a.G.posx = 1", &[]).map(|e| e.2), Some(rt(0x0869ae60).into()));
    assert_eq!(check("a.etrnx = b._x", &[]).map(|e| e.2), Some("Unknown parameter".into()));
    assert_eq!(check("SPOT a.etrnx = 1", &[]).map(|e| e.2), Some("Invalid Chapter Id".into()));
    // `ALL` in the place of an element type: the parser gives up without a message
    assert_eq!(check("ALL a.etrnx = 1", &[]).map(|e| e.2), Some(String::new()));
    // an unknown parameter name is taken for a custom effect's when arguments are not
    // validated
    assert_eq!(check("a.etrnx = b.whatever", &[]), None);
}

#[test]
fn a_function_call_with_no_argument_is_a_syntax_error() {
    si_rt::skip_without_install!();
    // `error` is shifted and reduced to an expression, so the parser goes on; the error
    // flag stays up and the rule for the call returns failure
    assert_eq!(check("a.etrnx = av()", &[]).map(|e| e.2), Some("parse error".into()));
}

#[test]
fn precedence_and_associativity() {
    si_rt::skip_without_install!();
    let e = build("a.etrnx = 1 + 2 * 3 - 4 / 5 % 6", &[], None);
    // ((1 + (2 * 3)) - ((4 / 5) % 6))
    let rhs = e.root.as_ref().unwrap().right.as_ref().unwrap();
    assert_eq!(rhs.oper, op::SUB);
    assert_eq!(rhs.left.as_ref().unwrap().oper, op::ADD);
    assert_eq!(rhs.right.as_ref().unwrap().oper, op::MOD);
    assert_eq!(rhs.right.as_ref().unwrap().left.as_ref().unwrap().oper, op::DIV);
    // || lowest, then &&, then == != LIKE, then the four relations
    let e = build("a.etrnx = 1 < 2 == 3 > 4 && 5 || 6", &[], None);
    let rhs = e.root.as_ref().unwrap().right.as_ref().unwrap();
    assert_eq!(rhs.oper, op::OR);
    let and = rhs.left.as_ref().unwrap();
    assert_eq!(and.oper, op::AND);
    let eq = and.left.as_ref().unwrap();
    assert_eq!(eq.oper, op::EQ);
    assert_eq!(eq.left.as_ref().unwrap().oper, op::LT);
    assert_eq!(eq.right.as_ref().unwrap().oper, op::GT);
    // the unary operators bind tighter than everything: -a * b is (-a) * b, !a == b is (!a) == b
    let e = build("a.etrnx = -b.rotz * 2", &[], None);
    let rhs = e.root.as_ref().unwrap().right.as_ref().unwrap();
    assert_eq!(rhs.oper, op::MUL);
    assert_eq!(rhs.left.as_ref().unwrap().oper, op::NEG);
    let e = build("a.etrnx = ! 1 == 2", &[], None);
    assert_eq!(e.root.as_ref().unwrap().right.as_ref().unwrap().oper, op::EQ);
}

#[test]
fn constants_are_negated_in_place_unless_parenthesised() {
    si_rt::skip_without_install!();
    let e = build("a.etrnx = -1 * - 2.5 + -( 3 )", &[], None);
    assert_eq!(rhs_text(&e), "-1 * -2.5 + -( 3 )");
    let rhs = e.root.as_ref().unwrap().right.as_ref().unwrap();
    let neg = rhs.right.as_ref().unwrap();
    assert_eq!(neg.oper, op::NEG);
    assert_eq!(neg.left.as_ref().unwrap().oper, op::LEAF | PAREN);
}

#[test]
fn printer_layout() {
    si_rt::skip_without_install!();
    let e = build("a.etrnx = cond( b.rotz > 1 && c.etrnx<2 ,sin(Fc*4),-d.rotz)+T", &[], None);
    // a blank after every piece, none before a comma; the negation of a reference leaves
    // two blanks behind it
    assert_eq!(rhs_text(&e), "cond( arg2.rotz > 1 && arg3.etrnx < 2, sin( Fc * 4 ), -arg4.rotz  ) + T");
    let e = build("a.etrnx = RAND( 7 , 1 , 5 ) + RAND( , 0.5 , 2.0 ) + randBool( ) + rand_0_1( 3 )", &[], None);
    assert_eq!(rhs_text(&e), "RAND( 7, 1, 5  ) + RAND(  , 0.5, 2.0 ) + randBool(   ) + rand_0_1( 3 )");
    let e = build("a.etrnx = b.G.etrnx + b.wshp_3 + b.whatever + CAM c.posx + \"x-1\" + SPOT + Parent + ALL", &[], None);
    // (a cluster parameter needs the model for its name: not in this dictionary)
    assert_eq!(rhs_text(&e), "arg2.G.etrnx + arg3.wshp_3 + arg4.whatever + CAM arg5.posx + \"x-1\" + SPOT + Parent + ALL");
    let e = build("a.etrnx = this.rotz + PI + E + TRUE + FALSE + LinearInterp + SplineInterp + FORWARD + BACKWARD + Fs + Fe", &[], None);
    assert_eq!(rhs_text(&e), "this2.rotz + PI + E + TRUE + FALSE + LinearInterp + SplineInterp + FORWARD + BACKWARD + Fs + Fe");
}

#[test]
fn argument_numbers() {
    si_rt::skip_without_install!();
    // typed in: every reference takes the next number; the target is counted too
    let e = build("a.etrnx = b.rotz + b.rotz", &[], None);
    assert_eq!(format::get_expr_arg_list(&e).unwrap(), vec![b"arg2".to_vec(), b"arg3".to_vec()]);
    // loaded: the numbers are places in the block's list, and a name outside it keeps the
    // running number with a complaint on standard output
    let list = [b"ret_el".to_vec(), b"arg1".to_vec(), b"arg2".to_vec()];
    let mut p = Parser::new();
    let e = p.gen_cus_expr_parsing_tree(&TestDict, b"MOD ret_el.etrnx = arg2.rotz + arg1.rotz + arg9.rotz", &[], 0, Some(&list)).unwrap();
    assert_eq!(format::get_expr_arg_list(&e).unwrap(), vec![b"arg2".to_vec(), b"arg1".to_vec(), b"arg4".to_vec()]);
    assert_eq!(p.stdout, b"Invalid arg arg9\n");
}

fn eval(text: &str, scene: &mut EmptyScene) -> Result<(f32, i32), EvalUndefined> {
    let e = build(text, &[], None);
    let data = eval::ExprRelData { query: 1, ..Default::default() };
    let frame = scene.frame_current;
    eval::expression_evaluate(scene, &e, &data, 0, frame, 0.5)
}

#[test]
fn evaluator_arithmetic() {
    si_rt::skip_without_install!();
    let mut s = EmptyScene { frame_current: 12, frame_start: 1, frame_end: 100, ..Default::default() };
    assert_eq!(eval("a.etrnx = 1 + 2 * 3", &mut s), Ok((7.0, 0)));
    assert_eq!(eval("a.etrnx = Fc + Fs + Fe + T", &mut s), Ok((113.5, 0)));
    // division by zero: the dividend, and error 4
    assert_eq!(eval("a.etrnx = 5 / 0", &mut s), Ok((5.0, 4)));
    assert_eq!(eval("a.etrnx = 5 % 0", &mut s), Ok((5.0, 4)));
    // the remainder cuts both operands to integers; a divisor below 1 traps
    assert_eq!(eval("a.etrnx = 7.9 % 2.9", &mut s), Ok((1.0, 0)));
    assert_eq!(eval("a.etrnx = 7 % 0.5", &mut s), Err(EvalUndefined::IntegerDivide));
    assert_eq!(eval("a.etrnx = -7 % 3", &mut s), Ok((-1.0, 0)));
    // comparisons give 1 or 0
    assert_eq!(eval("a.etrnx = ( 1 < 2 ) + ( 2 <= 2 ) + ( 3 > 2 ) + ( 2 >= 3 ) + ( 1 == 1 ) + ( 1 != 1 )", &mut s), Ok((4.0, 0)));
    assert_eq!(eval("a.etrnx = ! 0 + ! 5", &mut s), Ok((1.0, 0)));
    // `||` does not evaluate its right side when the left is not zero: no error 4 here
    assert_eq!(eval("a.etrnx = 1 || 1 / 0", &mut s), Ok((1.0, 0)));
    assert_eq!(eval("a.etrnx = 0 && 1 / 0", &mut s), Ok((0.0, 0)));
    assert_eq!(eval("a.etrnx = 0 || 1 / 0", &mut s), Ok((1.0, 4)));
    // strings: comparison and containment; mixed kinds give 0
    assert_eq!(eval("a.etrnx = ( \"abc\" LIKE \"bc\" ) + ( \"abc\" == \"abc\" ) + ( \"a\" < \"b\" ) + ( \"a\" == 1 )", &mut s), Ok((3.0, 0)));
    // enumeration values: `ALL` equals anything
    assert_eq!(eval("a.etrnx = ( SPOT == SPOT ) + ( SPOT == MOD ) + ( SPOT == ALL ) + ( SPOT != MOD )", &mut s), Ok((3.0, 0)));
    // LIKE between numbers: the left operand and a line on standard output
    assert_eq!(eval("a.etrnx = 3 LIKE 4", &mut s), Ok((3.0, 0)));
    assert_eq!(s.printed, "Unknown oper...\n");
    // a reference to an element that is not there is 0
    assert_eq!(eval("a.etrnx = b.rotz + 2", &mut s), Ok((2.0, 0)));
}

#[test]
fn every_node_rounds_to_a_float() {
    si_rt::skip_without_install!();
    // 16777216 + 1 is not a float: the sum is rounded when it is stored, before the next
    // operation, so adding 1 twice changes nothing; in one register it would give 16777218
    let mut s = EmptyScene::default();
    assert_eq!(eval("a.etrnx = 16777216 + 1 + 1", &mut s), Ok((16777216.0, 0)));
    // `av` adds in the register and rounds once
    assert_eq!(eval("a.etrnx = av( 16777216, 1, 1 ) * 3", &mut s), Ok((16777218.0, 0)));
}

#[test]
fn builtin_functions() {
    si_rt::skip_without_install!();
    let mut s = EmptyScene { frame_current: 15, ..Default::default() };
    let e = |t: &str, s: &mut EmptyScene| eval(&format!("a.etrnx = {t}"), s).unwrap().0;
    assert_eq!(e("sin( 90 )", &mut s), 1.0);
    assert_eq!(e("cos( 0 )", &mut s), 1.0);
    // degrees go through the ten-digit factor: sin(180) is not 0 but the sine of 3.1415926500…
    assert!(e("sin( 180 )", &mut s) > 0.0 && e("sin( 180 )", &mut s) < 1e-8);
    // the tangent of an odd multiple of 90 is 0 by decree; of 180 it is computed
    assert_eq!(e("tan( 90 )", &mut s), 0.0);
    assert_eq!(e("tan( 270 )", &mut s), 0.0);
    assert_eq!(e("tan( 45 )", &mut s), 1.0);
    assert_eq!(e("atan( 1 )", &mut s), 45.0);
    assert_eq!(e("asin( 1 )", &mut s), 90.0);
    assert_eq!(e("asin( 1.5 )", &mut s), 0.0);
    assert_eq!(e("acos( 1 )", &mut s), 0.0);
    assert_eq!(e("log( 0 )", &mut s), 0.0);
    assert_eq!(e("log( 1 )", &mut s), 0.0);
    assert_eq!(e("exp( 0 )", &mut s), 1.0);
    assert_eq!(e("abs( -2.5 )", &mut s), 2.5);
    assert_eq!(e("sqrt( 16 )", &mut s), 4.0);
    assert_eq!(e("sqrt( -1 )", &mut s), 0.0);
    assert_eq!(e("pow( 2, 10 )", &mut s), 1024.0);
    assert_eq!(e("pow( -8, 0.5 )", &mut s), 0.0);
    assert_eq!(e("pow( 0, -1 )", &mut s), 0.0);
    assert_eq!(e("pow( 0, 0 )", &mut s), 1.0);
    assert_eq!(e("MIN( 3, -2 )", &mut s), -2.0);
    assert_eq!(e("MAX( 3, -2 )", &mut s), 3.0);
    assert_eq!(e("av( 1, 2, 6 )", &mut s), 3.0);
    // the condition is cut to an integer and must be exactly 1
    assert_eq!(e("cond( 1, 10, 20 )", &mut s), 10.0);
    assert_eq!(e("cond( 2, 10, 20 )", &mut s), 20.0);
    assert_eq!(e("cond( 1.9, 10, 20 )", &mut s), 10.0);
    assert_eq!(e("cond( 0.9, 10, 20 )", &mut s), 20.0);
    assert_eq!(e("fmod( 7.5, 2 )", &mut s), 1.5);
    assert_eq!(e("fmod( -7.5, 2 )", &mut s), -1.5);
    assert_eq!(e("fmod( 7.5, 0 )", &mut s), 0.0);
    // frame 15 of a 60-frame wave: a quarter turn
    assert_eq!(e("sinus( 60, 3, 0 )", &mut s), 3.0);
    // half a turn: the sine of the float nearest to pi, not zero
    let half = e("sinus( 60, 3, 15 )", &mut s);
    assert!(half < -2.6e-7 && half > -2.7e-7, "{half}");
    // the table wants three arguments although the dialog's text marks the offset optional
    assert_eq!(check("a.etrnx = sinus( 60, 3 )", &[]).map(|e| e.2), Some(rt(0x0869af00).into()));
    assert_eq!(e("ease_in( 10, 20, LinearInterp )", &mut s), 0.5);
    assert_eq!(e("ease_in( 10, 20, SplineInterp )", &mut s), 0.5);
    assert_eq!(e("ease_out( 10, 20, LinearInterp )", &mut s), 0.5);
    assert_eq!(e("ease_in( 20, 10, 1 )", &mut s), 1.0);
    assert_eq!(e("ease_in( 16, 20, 1 )", &mut s), 0.0);
    assert_eq!(e("ease_out( 0, 10, 1 )", &mut s), 0.0);
    // period 60 throughout, amplitude from 0 to 2 along a line: 1 at the middle frame
    assert_eq!(e("oscil( 10, 20, 60, 60, 1, 0, 2, 1 )", &mut s), 1.0);
    assert_eq!(e("oscil( 16, 20, 60, 60, 1, 0, 2, 1 )", &mut s), 0.0);
    // nothing in the scene: no curve, no positions, no relations
    assert_eq!(e("at_time( 1.0, b.rotz ) + at_frame( 3, b.rotz ) + ctr_dist( b., c. ) + ctr_dist_cam( b. ) + has_relation( b., Parent, FORWARD )", &mut s), 0.0);
}

#[test]
fn interpolation_shapes() {
    si_rt::skip_without_install!();
    use crate::fct::{hermit_interp, linear_interp};
    assert_eq!(linear_interp(2.5, 0.0, 10.0, 0.0, 4.0).to_f32(), 1.0);
    assert_eq!(linear_interp(1.0, 3.0, 3.0, 5.0, 6.0).to_f32(), 0.0);
    // the cubic 3t² − 2t³ at t = 1/4: 5/32
    assert_eq!(hermit_interp(2.5, 0.0, 10.0, 0.0, 1.0).to_f32(), 0.15625);
    assert_eq!(hermit_interp(0.0, 0.0, 10.0, 7.0, 9.0).to_f32(), 7.0);
    assert_eq!(hermit_interp(10.0, 0.0, 10.0, 7.0, 9.0).to_f32(), 9.0);
}

#[test]
fn random_numbers() {
    si_rt::skip_without_install!();
    // the C library's first number after srand(1) is 1804289383; times 2⁻³¹
    let mut s = EmptyScene::default();
    let r = RandNode { form: 2, index: 0, seed: -1, real: 1, low: 0.0, width: 1.0 };
    assert_eq!(crate::fct::random_eval(&mut s, &r).to_f32(), (1804289383.0f64 / 2147483648.0) as f32);
    // an integer range rounds to the nearest integer: 846930886 × 2⁻³¹ × 10 = 3.94… → 4
    let r = RandNode { form: 0, index: 0, seed: -1, real: 0, low: 0.0, width: 10.0 };
    assert_eq!(crate::fct::random_eval(&mut s, &r).to_f32(), 4.0);
    // a seeded call reads its table by frame; an unseeded one draws each time
    let e = build("a.etrnx = rand_0_1( 5 ) + 0", &[], None);
    let mut data = eval::init_rel_data(&mut s, &e, 1, 3).unwrap();
    data.query = 1;
    let table = data.values.as_ref().unwrap()[0].clone().unwrap();
    assert_eq!(table.len(), 3);
    for (i, want) in table.iter().enumerate() {
        assert_eq!(eval::expression_evaluate(&mut s, &e, &data, i as i32, 0, 0.0).unwrap().0, *want);
    }
    let mut again = EmptyScene::default();
    again.srand(5);
    assert_eq!(table[0], crate::fct::random_eval(&mut again, &RandNode { form: 2, index: 0, seed: 5, real: 1, low: 0.0, width: 1.0 }).to_f32());
}

#[test]
fn value_conversions() {
    si_rt::skip_without_install!();
    assert_eq!(AttrValue::float(2.9).in_int(), 2);
    assert_eq!(AttrValue::float(-2.9).in_int(), -2);
    assert_eq!(AttrValue::Raw { kind: 2, w1: 0xffff_fffe, w2: 0 }.in_float().to_f32(), -2.0);
    assert_eq!(AttrValue::Raw { kind: 9, w1: 7, w2: 1.5f32.to_bits() }.in_float().to_f32(), 1.5);
    assert_eq!(AttrValue::Str(Some(b"x".to_vec())).in_float().to_f32(), 0.0);
}

#[test]
fn evaluation_without_tables() {
    si_rt::skip_without_install!();
    let e = build("a.etrnx = 2 * 3", &[], None);
    let mut s = EmptyScene::default();
    let mut err = 0;
    let args = EvalArgs { rand_values: None, frame_index: 0, frame: 0, time: 0.0 };
    // the whole statement: the assignment's value is its right side; the target is missing
    let v = eval::eval_expr_exec(&mut s, &e, e.root.as_deref().unwrap(), &args, &mut err).unwrap();
    assert_eq!(v, AttrValue::float(6.0));
}

// ---- the image form (src/image), on made-up images

mod image_form {
    use super::*;
    use crate::image::{self, rec};
    use si_mem::{Mem, Ptr32};

    /// A fresh image with the crate's initial values, a process record and a dictionary of
    /// 36 empty chapters (no element: every reference is to a missing one).
    fn world() -> Mem {
        let mut m = Mem::new();
        rec::init_globals(&mut m);
        let pd = m.place(4, 4);
        m.set_u32(pd, 0);
        m.set_u32(si_dict::globals::SI_PROC_DATA.addr(), pd);
        let lists = m.place(36 * 0x40, 4);
        for k in 0..36 * 0x10 {
            m.set_u32(lists + 4 * k, 0);
        }
        let ed = m.place(0x70, 4);
        for k in 0..0x1c {
            m.set_u32(ed + 4 * k, 0);
        }
        m.set_u32(ed + 4, lists);
        m.set_u32(si_dict::globals::SI_ELEM_DICTS.addr(), ed);
        m
    }

    /// The right-hand side of `text` evaluated on the image as a query, and by the value
    /// form: both results and error codes.
    fn both(text: &str, vars: &[VarDef]) -> ((f32, i32), (f32, i32)) {
        let e = build(text, vars, None);
        let mut s = EmptyScene { frame_start: 1, frame_end: 50, ..Default::default() };
        let data = eval::ExprRelData { query: 1, ..Default::default() };
        let value = eval::expression_evaluate(&mut s, &e, &data, 4, 5, 0.5).unwrap();
        let mut m = world();
        m[rec::SI_FRAME_START] = 1;
        m[rec::SI_FRAME_END] = 50;
        let x = image::tree::put_expression(&mut m, &e);
        let rd = m.si_calloc(1, 0x20);
        m.set_i16(rd + 0x14, 1);
        let out = Ptr32::<f32>::new(m.place(4, 4));
        let err = Ptr32::<i32>::new(m.place(4, 4));
        let mut o = image::calls::Clib::default();
        image::expression_evaluate(&mut m, &mut o, x, Ptr32::new(rd), 4, 5, 0.5, out, err).unwrap();
        assert!(m.fault().is_none(), "{text}: {:?}", m.fault());
        ((m.read(out), m.read(err)), value)
    }

    #[test]
    fn a_tree_goes_into_the_image_and_comes_back() {
        let vars = [VarDef { name: b"A".to_vec(), text: Some(b"MAX( 1, Fc ) * 2".to_vec()) }];
        let e = build("a.etrnx = cond( A > 3, RAND( 7, 1, 5 ), -A ) + \"ab\" LIKE \"b\"", &vars, None);
        let mut m = world();
        let x = image::tree::put_expression(&mut m, &e);
        assert_eq!(image::tree::get_expression(&m, x), e);
        // a leaf is a 12-byte node whose +4 is the 8-byte leaf record
        let root = m.u32(x.addr());
        assert_eq!(m.u32(root) & 0x7f, op::ASSIGN);
        assert_eq!(m.block_size(root), Some(0xc));
    }

    #[test]
    fn the_image_evaluator_agrees_with_the_value_form() {
        let vars = [
            VarDef { name: b"A".to_vec(), text: Some(b"MAX( MIN( 1, 2 ), cond( Fc == 5, 3, 4 ) )".to_vec()) },
            VarDef { name: b"B".to_vec(), text: Some(b"av( Fc, 2, 3, 4, 5, 6 ) / 0".to_vec()) },
        ];
        for text in [
            "a.etrnx = 1 + 2 * 3 - 4 / 3",
            "a.etrnx = 16777216 + 1 + 1",
            "a.etrnx = A * 2 + T",
            "a.etrnx = B",
            "a.etrnx = 7 % 3 + ( 2 < 3 ) + ( 3 <= 3 ) - ! 0 + ( 1 && 0 ) + ( 0 || 2 )",
            "a.etrnx = sqrt( 2 ) + exp( 1 ) + abs( -3 ) + Fs + Fe",
            "a.etrnx = \"abc\" LIKE \"bc\" + ( \"a\" < \"b\" )",
            "a.etrnx = PI * E + TRUE + SplineInterp",
            "a.etrnx = b.rotz + 1",
        ] {
            let (img, val) = both(text, &vars);
            assert_eq!(img.0.to_bits(), val.0.to_bits(), "{text}");
            assert_eq!(img.1, val.1, "{text}");
        }
    }

    #[test]
    fn nested_calls_grow_the_evaluators_arrays_and_give_them_back() {
        let mut m = world();
        let e = build("a.etrnx = MAX( MIN( 1, MAX( 2, 3 ) ), 0 )", &[], None);
        let x = image::tree::put_expression(&mut m, &e);
        let rd = m.si_calloc(1, 0x20);
        m.set_i16(rd + 0x14, 1);
        let out = Ptr32::<f32>::new(m.place(4, 4));
        let err = Ptr32::<i32>::new(m.place(4, 4));
        let mut o = image::calls::Clib::default();
        image::expression_evaluate(&mut m, &mut o, x, Ptr32::new(rd), 0, 0, 0.0, out, err).unwrap();
        assert_eq!(m.read(out), 1.0);
        // three levels, each with two records; the stack is back at −1
        assert_eq!(m[rec::NB_ENTRIES_IN_STACK], 3);
        assert_eq!(m[rec::TOP_OF_THE_STACK], -1);
        let counts = m.u32(rec::NB_VALS_ALLOC.addr());
        assert_eq!((0..3).map(|l| m.u32(counts + 4 * l)).collect::<Vec<_>>(), [2, 2, 2]);
    }

    #[test]
    fn a_seeded_random_call_reads_its_table() {
        let mut m = world();
        m[rec::SI_FRAME_START] = 1;
        m[rec::SI_FRAME_END] = 10;
        let e = build("a.etrnx = RAND( 3, 1, 100 )", &[], None);
        let x = image::tree::put_expression(&mut m, &e);
        let mut o = image::calls::Clib::default();
        let rd = image::regen::expr_init_rel_data(&mut m, &mut o, x, 1, 10);
        assert_eq!(m.i16(rd.addr() + 0x16), 1);
        assert_eq!(m.i32(rd.addr() + 0x18), 10);
        let table = m.u32(m.u32(rd.addr() + 0x1c));
        let mut c = crate::rand::CRand::default();
        c.srand(3);
        let mut s = EmptyScene { crand: c, ..Default::default() };
        let r = RandNode { form: 0, index: 0, seed: 3, real: 0, low: 1.0, width: 99.0 };
        let want: Vec<f32> = (0..10).map(|_| crate::fct::random_eval(&mut s, &r).to_f32()).collect();
        let got: Vec<f32> = (0..10).map(|k| m.f32(table + 4 * k)).collect();
        assert_eq!(got, want);
        let out = Ptr32::<f32>::new(m.place(4, 4));
        let err = Ptr32::<i32>::new(m.place(4, 4));
        image::expression_evaluate(&mut m, &mut o, x, rd, 6, 7, 0.0, out, err).unwrap();
        assert_eq!(m.read(out), want[6]);
    }

    #[test]
    fn a_models_rotation_is_set_in_radians_and_read_in_degrees() {
        let mut m = world();
        let mdl = m.place(0x430, 4);
        for k in 0..0x430 / 4 {
            m.set_u32(mdl + 4 * k, 0);
        }
        m[rec::SI_FRAME_RATE] = 30.0;
        let v = Ptr32::<si_types::dict::AttributeValue>::new(m.place(12, 4));
        m.set_i32(v.addr(), 1);
        m.set_f32(v.addr() + 4, 90.0);
        let mut o = image::calls::Clib::default();
        assert_eq!(image::elem::model_set_param(&mut m, &mut o, 6, mdl, 0, v, 0), 1);
        let rad = m.f32(mdl + 0x114);
        assert_eq!(rad, (si_x87::F80::from_f64(0.0174532925) * si_x87::F80::from_f32(90.0)).to_f32());
        let out = Ptr32::<si_types::dict::AttributeValue>::new(m.place(12, 4));
        assert_eq!(image::elem::model_get_param(&mut m, &mut o, 6, mdl, 0, out, 0), 1);
        assert_eq!(m.i32(out.addr()), 1);
        assert_eq!(m.f32(out.addr() + 4), (si_x87::F80::from_f32(rad) * si_x87::F80::from_f64(57.29577951308232)).to_f32());
        // a translation of an effector goes to its goal record, its root's local
        // translation added
        let root = m.place(0x430, 4);
        for k in 0..0x430 / 4 {
            m.set_u32(root + 4 * k, 0);
        }
        m.set_f32(root + 0x15c, 2.0);
        let ik = m.place(0x40, 4);
        for k in 0..0x10 {
            m.set_u32(ik + 4 * k, 0);
        }
        m.set_u32(mdl + 0x2bc, root);
        m.set_i32(mdl + 0x2ec, 3);
        m.set_u32(mdl + 0x318, ik);
        m.set_f32(v.addr() + 4, 5.0);
        image::elem::model_set_param(&mut m, &mut o, 0x8c, mdl, 0, v, 0);
        assert_eq!(m.f32(ik + 0xc), 7.0);
        assert_eq!(m.i32(ik + 0x38), 1);
        assert_eq!(m.f32(mdl + 0x15c), 0.0);
        assert!(m.fault().is_none());
    }

    /// `n` zeroed bytes in the arena.
    fn zeroed(m: &mut Mem, n: u32) -> u32 {
        let a = m.place(n, 4);
        for k in 0..n / 4 {
            m.set_u32(a + 4 * k, 0);
        }
        a
    }

    /// An outside world that answers the sons of elements from a table and counts the rest.
    struct Sons {
        sons: Vec<(u32, Vec<u32>)>,
        cursor: u32,
        calls: Vec<(&'static str, Vec<u32>)>,
    }

    impl si_mem::Outside for Sons {
        fn call(&mut self, _m: &mut Mem, r: si_mem::Routine, args: &[u32]) -> si_mem::Ret {
            self.calls.push((r.name, args.to_vec()));
            let kids = self.sons.iter().find(|s| s.0 == args[0]).map(|s| s.1.clone()).unwrap_or_default();
            let k = match r.name {
                "SI_edFindSon" => 0,
                "SI_edFindNextSon" => self.cursor,
                "SI_mapWireToIndexColour" => return si_mem::Ret::int(0x1_0005),
                _ => return si_mem::Ret::NONE,
            };
            self.cursor = k + 1;
            si_mem::Ret::int(kids.get(k as usize).copied().unwrap_or(0) as i32)
        }
    }

    #[test]
    fn a_branch_is_hidden_and_shown_with_its_sons_graf_nodes() {
        let mut m = world();
        // the process record: dictionary 0, graf list 1
        let pd = zeroed(&mut m, 8);
        m.set_u32(pd + 4, 1);
        m.set_u32(si_dict::globals::SI_PROC_DATA.addr(), pd);
        let lists = zeroed(&mut m, 2 * 0x24);
        m.set_u32(0x0867_9498, lists);
        let (root_mdl, son_mdl) = (zeroed(&mut m, 0x2bc), zeroed(&mut m, 0x2bc));
        m.set_i32(son_mdl + 0x30, 8);
        m.set_u32(son_mdl + 0x48, 0x1234);
        let (root, son) = (zeroed(&mut m, 0x84), zeroed(&mut m, 0x84));
        m.set_u32(son + 4, son_mdl);
        m.set_u32(son + 0x20, 0x100);
        let (gn, son_gn) = (zeroed(&mut m, 0x24), zeroed(&mut m, 0x24));
        m.set_i32(son_gn, 3);
        m.set_u32(son_gn + 4, son_mdl);
        m.set_u32(lists + 0x24 + 4, son_gn);
        let mut o = Sons { sons: vec![(root, vec![son])], cursor: 0, calls: vec![] };
        image::elem::model_set_hidden_param(&mut m, &mut o, gn, root_mdl, root, 2, 1);
        assert_eq!((m.i32(gn + 0xc), m.i16(root_mdl + 0x2b8), m.u32(root + 0x20)), (2, 1, 2));
        assert_eq!((m.i32(son_gn + 0xc), m.i16(son_mdl + 0x2b8), m.u32(son + 0x20)), (2, 1, 0x102));
        let names: Vec<_> = o.calls.iter().map(|c| c.0).collect();
        assert_eq!(names, ["SI_edFindSon", "SI_metaballTouch", "SI_edFindSon", "SI_edFindNextSon"]);
        assert_eq!(o.calls[1].1, [0x1234, 0]);
        // any other state hides; without the branch the sons are left alone
        image::elem::model_set_hidden_param(&mut m, &mut o, gn, root_mdl, root, 3, 0x1_0000u32 as i16);
        assert_eq!((m.i32(gn + 0xc), m.i16(root_mdl + 0x2b8), m.u32(root + 0x20)), (0, 0, 0));
        assert_eq!(m.u32(son + 0x20), 0x102);
        assert!(m.fault().is_none());
    }

    #[test]
    fn the_general_attributes_of_an_element_are_read() {
        let mut m = world();
        // one element in chapter 2, with a name record and no graf node
        let lists = m.u32(m.u32(si_dict::globals::SI_ELEM_DICTS.addr()) + 4);
        let e = zeroed(&mut m, 0x84);
        m.set_i32(lists + 2 * 0x40 + 4, 1);
        m.set_u32(lists + 2 * 0x40 + 8, e);
        let node = zeroed(&mut m, 0x34);
        let version = m.place_cstr(b" -12x");
        m.set_u32(node + 0x18, version);
        m.set_u32(e, node);
        m.set_i32(e + 0x1c, 0x8003);
        m.set_i32(e + 0x54, 4);
        // one group of one attribute whose components are the level, the version, the
        // colour, the selectability and an id the routine does not know
        let ids = m.place(10, 4);
        for (k, id) in [0x10au16, 0x107, 0x10b, 0x237, 0x103].iter().enumerate() {
            m.set_u16(ids + 2 * k as u32, *id);
        }
        let attr = zeroed(&mut m, 0x64);
        m.set_i32(attr + 0x2c, 3);
        m.set_i16(attr + 0x56, 5);
        m.set_u32(attr + 0x58, ids);
        let which = zeroed(&mut m, 4);
        let group = zeroed(&mut m, 0x28);
        m.set_i32(group + 0xc, 1);
        m.set_u32(group + 0x10, which);
        let atd = si_dict::globals::SI_ATD.addr();
        m.set_i32(atd, 1);
        m.set_u32(atd + 4, group);
        m.set_u32(atd + 8, attr);
        m.set_i32(atd + 0xc, 1);
        let ident = zeroed(&mut m, 0x10);
        m.set_i32(ident, 2);
        let index = zeroed(&mut m, 0x14);
        m.set_i16(index, 2);
        let value = zeroed(&mut m, 0xc);
        let mut o = Sons { sons: vec![], cursor: 0, calls: vec![] };
        let mut get = |m: &mut Mem, k: i32| {
            m.set_i32(index + 0x10, k);
            m.set_u32(value + 4, 0xdead);
            let r = image::elem::general_attr_get_value(m, &mut o, Ptr32::new(ident), Ptr32::new(index), Ptr32::new(value));
            (r, m.i32(value), m.i32(value + 4))
        };
        assert_eq!(get(&mut m, 0), (1, 3, 4));
        assert_eq!(get(&mut m, 1), (1, 3, -12));
        // the colour index's answer is a short, widened with its sign
        assert_eq!(get(&mut m, 2), (1, 3, 5));
        // without a graf node the selectability is not answered
        assert_eq!(get(&mut m, 3), (0, 3, 0xdead));
        assert_eq!(get(&mut m, 4), (0, 3, 0xdead));
        // a component the attribute does not have, a deleted element
        assert_eq!(get(&mut m, 5).0, 0);
        m.set_i16(e + 0x48, 2);
        assert_eq!(get(&mut m, 0).0, 0);
        assert_eq!(o.calls.iter().map(|c| (c.0, c.1.clone())).collect::<Vec<_>>(), [("SI_mapWireToIndexColour", vec![0xffff_8003, 1])]);
        assert!(m.fault().is_none());
    }
}
