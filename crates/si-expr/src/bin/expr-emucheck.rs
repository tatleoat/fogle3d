// SPDX-License-Identifier: MIT
//! Compares the Rust port with vectors from the emulation harness (finding 0011):
//! original routines run in a CPU emulator on made-up input by
//! `re/tools/si-expr/emu_gen.py`.
//!
//!   expr-emucheck <vector directory>
//!
//! The made-up dictionaries here are the ones the generator pokes into the emulator's
//! memory: one enumeration and nine function-curve names.
//!
//! With `EXPR_IMAGE=1` the cases of `SI_expressionEvaluate` are run through the image form
//! (`si_expr::image`): the case's tree laid into a fresh image, an element dictionary with
//! no chapter, the frame globals, a relation record for a query, and
//! `image::expression_evaluate` called; the float, the error code and the printed text are
//! compared. `EMUCHECK_MUTATE=1` then flips one bit of the port's float,
//! `EMUCHECK_MUTATE=calls` adds a byte to what the port printed: a compared case must then
//! differ (the second only where something is printed).

use si_expr::env::Dict;
use si_expr::eval::{self, ExprRelData};
use si_expr::fct;
use si_expr::format::{self, ArgType};
use si_expr::parse::{Parser, VarDef};
use si_expr::scene::EmptyScene;
use si_expr::tree::*;
use si_expr::value::AttrValue;
use si_x87::vectors::{check_dir, print_report, Case, Check, Verdict};
use si_x87::F80;
use std::path::Path;

const ENUM_CODES: [(i32, &[u8]); 6] = [(8, b"MOD"), (2, b"CAM"), (6, b"LIG"), (7, b"MAT"), (40, b"SPOT"), (41, b"FLAT")];
const FCURVES: [(i32, &[u8]); 9] = [
    (1, b"sclx"),
    (3, b"rotz"),
    (8, b"shape"),
    (40, b"posx"),
    (139, b"etrnx"),
    (140, b"etrny"),
    (0x9a, b"wshp"),
    (0xb7, b"clustr"),
    (0xb8, b"cw"),
];

struct EmuDict;

impl Dict for EmuDict {
    fn enum_val_by_any_code(&self, enum_type: &mut i32, code: &[u8]) -> Option<i32> {
        if *enum_type >= 1 {
            return None;
        }
        *enum_type = if *enum_type == -1 { 0 } else { *enum_type + 1 };
        if *enum_type != 0 {
            return None;
        }
        ENUM_CODES.iter().find(|e| e.1 == code).map(|e| e.0)
    }
    fn enum_code_by_value(&self, enum_type: i32, id: i32) -> Option<Vec<u8>> {
        if enum_type != 0 {
            return None;
        }
        ENUM_CODES.iter().find(|e| e.0 == id).map(|e| e.1.to_vec())
    }
    fn rel_id_by_name(&self, _name: &[u8]) -> i32 {
        -1
    }
    fn rel_name(&self, _id: i32) -> Vec<u8> {
        b"unknown".to_vec()
    }
    fn fcurve_from_name(&self, name: &[u8]) -> i32 {
        if let Some(f) = FCURVES.iter().find(|f| f.1 == name) {
            return f.0;
        }
        match name {
            b"hidden" => 0xaf,
            b"camhid" => 0xb1,
            b"lighid" => 0xb0,
            b"t2norm" => 0x2b,
            _ => -1,
        }
    }
    fn name_from_fcurve(&self, fcv: i32) -> Vec<u8> {
        FCURVES.iter().find(|f| f.0 == fcv).map_or(b"notf".to_vec(), |f| f.1.to_vec())
    }
}

fn cstr(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().copied().take_while(|&c| c != 0).collect()
}

/// `SI_validCustomExpress(text, vars, nvars, &errvar, 0, 1)`: the returned pointer (as an
/// address: where the text or the variable's definition was put, plus the offset), the
/// variable index written through the pointer, the error flag and the message.
fn valid_custom_express(c: &Case) -> Verdict {
    let text = c.input("text").to_bytes();
    let nvars = c.input("nvars").i32() as usize;
    let mut vars = Vec::new();
    for i in 0..nvars {
        vars.push(VarDef { name: cstr(&c.input(&format!("n{i}")).u8s()), text: Some(cstr(&c.input(&format!("t{i}")).u8s())) });
    }
    let mut p = Parser::new();
    let got = match p.valid_custom_express(&EmuDict, &text, &vars, 0, 1) {
        Ok(g) => g,
        // the port says the original has no defined behaviour here
        Err(_) => return Verdict::Skip,
    };
    let (want_eax, want_var) = match got {
        None => (0u32, -1),
        Some((-1, at)) => (c.output("textat").u32().wrapping_add(at as u32), -1),
        Some((i, at)) => (c.output(&format!("t{i}at")).u32().wrapping_add(at as u32), i),
    };
    let eax = c.output("eax").u32();
    let errvar = c.output("errvar").written()[0].map(|v| v as u32 as i32);
    let erro = c.output("erro").u16s()[0] as i16;
    let msg = c.output("msg").to_bytes();
    let ok = eax == want_eax && errvar == Some(want_var) && erro == p.error_occurred && msg == p.last_error_msg.as_bytes();
    Verdict::from(ok, || {
        format!("port: pointer {want_eax:08x} variable {want_var} flag {} message {:?}", p.error_occurred, p.last_error_msg)
    })
}

fn args_of(c: &Case) -> (u32, Vec<AttrValue<'static>>, EmptyScene) {
    let n = c.input("n").u32();
    let w = c.input("args").u32s();
    let args = w.chunks(3).map(|r| AttrValue::Raw { kind: r[0] as i32, w1: r[1], w2: r[2] }).collect();
    let scene = EmptyScene { frame_current: c.input("fc").i32(), ..Default::default() };
    (n, args, scene)
}

fn st0(c: &Case, got: F80) -> Verdict {
    Verdict::from(c.output("st0").same_f80(got), || format!("port {got:?}"))
}

macro_rules! builtin {
    ($f:path) => {
        |c: &Case| {
            let (n, args, mut scene) = args_of(c);
            match $f(&mut scene, n, &args) {
                Ok(v) => st0(c, v),
                Err(_) => Verdict::Skip,
            }
        }
    };
}

fn floats(c: &Case) -> [f32; 5] {
    [c.input("x").f32(), c.input("x0").f32(), c.input("x1").f32(), c.input("y0").f32(), c.input("y1").f32()]
}

/// Reads a tree from the text the generator writes next to the memory image it gives the
/// original (the grammar is at the top of the tree section of `emu_gen.py`).
struct Canon<'a> {
    toks: std::str::Split<'a, char>,
}

impl<'a> Canon<'a> {
    fn next(&mut self) -> &'a str {
        self.toks.next().expect("token")
    }
    fn num<T: std::str::FromStr>(&mut self) -> T {
        self.next().parse().ok().expect("number")
    }
    fn hex_text(&mut self) -> Vec<u8> {
        let t = self.next();
        if t == "00" {
            return Vec::new();
        }
        (0..t.len() / 2).map(|i| u8::from_str_radix(&t[2 * i..2 * i + 2], 16).unwrap()).collect()
    }
    fn bits(&mut self) -> f32 {
        f32::from_bits(u32::from_str_radix(self.next(), 16).unwrap())
    }
    fn node(&mut self) -> Option<Box<ExprNode>> {
        match self.next() {
            "-" => None,
            "(" => {
                let oper: u32 = self.num();
                let (left, right) = (self.node(), self.node());
                assert_eq!(self.next(), ")");
                Some(Box::new(ExprNode { oper, leaf: None, left, right }))
            }
            "<" => {
                let oper: u32 = self.num();
                let leaf = self.leaf();
                assert_eq!(self.next(), ">");
                Some(Box::new(ExprNode { oper, leaf: Some(Box::new(leaf)), left: None, right: None }))
            }
            t => panic!("unexpected {t}"),
        }
    }
    fn leaf(&mut self) -> NopNode {
        match self.next() {
            "i" => NopNode { kind: 0, data: NopData::Int(self.num()) },
            "f" => NopNode { kind: 1, data: NopData::Float(self.bits()) },
            "s" => NopNode { kind: 2, data: NopData::Text(Some(self.hex_text())) },
            "e" => NopNode { kind: 3, data: NopData::Enum(EnumNode { enum_type: self.num(), value: self.num() }) },
            "p" => NopNode { kind: self.num(), data: NopData::None },
            "r" => NopNode {
                kind: 8,
                data: NopData::Rand(RandNode { form: self.num(), index: self.num(), seed: self.num(), real: self.num(), low: self.bits(), width: self.bits() }),
            },
            "a" => {
                let kind = self.num();
                let a = ArgNode {
                    num: self.num(),
                    generic: self.num(),
                    chapter: self.num(),
                    elem: self.num(),
                    fcv: self.num(),
                    access0: self.num(),
                    access1: self.num(),
                    access_name: None,
                    custom_name: None,
                };
                let name = self.hex_text();
                NopNode { kind, data: NopData::Arg(ArgNode { custom_name: (kind == 23).then_some(name), ..a }) }
            }
            "c" => {
                let index = self.num();
                let n: usize = self.num();
                NopNode { kind: 10, data: NopData::Fct(FctNode { index, args: (0..n).map(|_| self.node()).collect() }) }
            }
            "v" => NopNode { kind: 11, data: NopData::Text(Some(self.hex_text())) },
            t => panic!("unexpected leaf {t}"),
        }
    }
    fn expression(text: &'a str) -> Expression {
        let mut c = Canon { toks: text.split(',') };
        assert_eq!(c.next(), "E");
        let n: usize = c.num();
        let vars = (0..n).map(|_| VarNode { name: c.hex_text(), expr: c.node() }).collect();
        Expression { root: c.node(), vars }
    }
}

fn tree_of(c: &Case) -> Expression {
    let bytes = c.input("canon").to_bytes();
    Canon::expression(std::str::from_utf8(&bytes).unwrap())
}

/// `SI_expressionEvaluate` on a made-up tree, as a query (the right-hand side only): the
/// float written, the error code, and what was printed.
fn expression_evaluate(c: &Case) -> Verdict {
    let expr = tree_of(c);
    let mut scene = EmptyScene { frame_start: c.input("fs").i32(), frame_end: c.input("fe").i32(), ..Default::default() };
    let data = ExprRelData { query: 1, ..Default::default() };
    let got = eval::expression_evaluate(&mut scene, &expr, &data, c.input("idx").i32(), c.input("frame").i32(), c.input("time").f32());
    let Ok((v, err)) = got else { return Verdict::Skip };
    let printed = c.try_output("stdout").map(|f| f.to_bytes()).unwrap_or_default();
    let ok = c.output("out").same_f32(&[v]) && c.output("err").i32() == err && printed == scene.printed.as_bytes();
    Verdict::from(ok, || format!("port {v:?} ({:08x}) error {err} printed {:?}", v.to_bits(), scene.printed))
}

/// `SI_getExprNodeFormat` on the right-hand side of a made-up tree, references written as
/// argument numbers: the text.
fn expr_node_format(c: &Case) -> Verdict {
    let expr = tree_of(c);
    let rhs = expr.root.as_deref().and_then(|r| r.right.as_deref());
    let Ok(got) = format::si_get_expr_node_format(&EmuDict, &expr, rhs, ArgType::Number, 0) else { return Verdict::Skip };
    let got = got.unwrap_or_default();
    Verdict::from(c.output("text").to_bytes() == got, || format!("port {:?}", String::from_utf8_lossy(&got)))
}

/// `SI_expressionEvaluate` through the image form (see the head of this file).
fn expression_evaluate_image(c: &Case) -> Verdict {
    use si_expr::image;
    use si_mem::{Mem, Ptr32};
    let expr = tree_of(c);
    let mut m = Mem::new();
    image::rec::init_globals(&mut m);
    // the process record (current dictionary 0) and a dictionary without chapters
    let pd = m.place(4, 4);
    m.set_u32(pd, 0);
    m.set_u32(si_dict::globals::SI_PROC_DATA.addr(), pd);
    // as the generator gives it: no chapter counted, an array of 36 empty chapters
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
    m[image::rec::SI_FRAME_START] = c.input("fs").i32();
    m[image::rec::SI_FRAME_END] = c.input("fe").i32();
    let x = image::tree::put_expression(&mut m, &expr);
    let rd = m.si_calloc(1, 0x20);
    m.set_i16(rd + 0x14, 1);
    let out = Ptr32::<f32>::new(m.place(4, 4));
    let err = Ptr32::<i32>::new(m.place(4, 4));
    let mut o = image::calls::Clib::default();
    let r = image::expression_evaluate(&mut m, &mut o, x, Ptr32::new(rd), c.input("idx").i32(), c.input("frame").i32(), c.input("time").f32(), out, err);
    if r.is_err() || m.fault().is_some() {
        return Verdict::Skip;
    }
    let mut v = m.read(out);
    let e = m.read(err);
    let mutate = std::env::var("EMUCHECK_MUTATE").unwrap_or_default();
    if mutate == "1" {
        v = f32::from_bits(v.to_bits() ^ 1);
    } else if mutate == "calls" && !o.printed.is_empty() {
        o.printed.push(b'!');
    }
    let printed = c.try_output("stdout").map(|f| f.to_bytes()).unwrap_or_default();
    let ok = c.output("out").same_f32(&[v]) && c.output("err").i32() == e && printed == o.printed;
    Verdict::from(ok, || format!("image form {v:?} ({:08x}) error {e} printed {:?}", v.to_bits(), String::from_utf8_lossy(&o.printed)))
}

fn lookup(name: &str) -> Option<Check> {
    let image = std::env::var_os("EXPR_IMAGE").is_some();
    Some(match name {
        "SI_validCustomExpress" => valid_custom_express,
        "SI_expressionEvaluate" if image => expression_evaluate_image,
        "SI_expressionEvaluate" => expression_evaluate,
        "SI_getExprNodeFormat" => expr_node_format,
        "si_exprSin" => builtin!(fct::expr_sin),
        "si_exprCos" => builtin!(fct::expr_cos),
        "si_exprTan" => builtin!(fct::expr_tan),
        "si_exprAtan" => builtin!(fct::expr_atan),
        "si_exprAsin" => builtin!(fct::expr_asin),
        "si_exprAcos" => builtin!(fct::expr_acos),
        "si_exprLog" => builtin!(fct::expr_log),
        "si_exprExp" => builtin!(fct::expr_exp),
        "si_exprFabs" => builtin!(fct::expr_fabs),
        "si_exprSqrt" => builtin!(fct::expr_sqrt),
        "si_conditionalFct" => builtin!(fct::conditional),
        "si_averageExpression" => builtin!(fct::average),
        "si_exprMin" => builtin!(fct::expr_min),
        "si_exprMax" => builtin!(fct::expr_max),
        "si_exprPower" => builtin!(fct::expr_power),
        "si_sinusFct" => builtin!(fct::sinus),
        "si_oscillationFct" => builtin!(fct::oscillation),
        "si_easeInFct" => builtin!(fct::ease_in),
        "si_easeOutFct" => builtin!(fct::ease_out),
        "si_fmodFct" => builtin!(fct::fmod),
        "linear_interp" => |c: &Case| {
            let v = floats(c);
            st0(c, fct::linear_interp(v[0], v[1], v[2], v[3], v[4]))
        },
        "hermit_interp" => |c: &Case| {
            let v = floats(c);
            st0(c, fct::hermit_interp(v[0], v[1], v[2], v[3], v[4]))
        },
        "isInteger" => |c: &Case| {
            let got = fct::is_integer(c.input("v").f32()) as u16;
            Verdict::from(c.output("ax").u16s()[0] == got, || format!("port {got}"))
        },
        "si_exprRandomEval" => |c: &Case| {
            let w = c.input("node").values();
            let node = RandNode {
                form: w[0] as i32,
                index: w[1] as i16,
                seed: w[3] as i32,
                real: w[4] as i16,
                low: f32::from_bits(w[6] as u32),
                width: f32::from_bits(w[7] as u32),
            };
            let mut scene = EmptyScene::default();
            st0(c, fct::random_eval(&mut scene, &node))
        },
        _ => return None,
    })
}

fn main() {
    // the parser's tables are read from the user's install (docs/design/runtime-data.md)
    si_rt::check_or_exit("expr-emucheck");
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: expr-emucheck <vector directory>");
        std::process::exit(2);
    };
    let rows = check_dir(Path::new(&dir), &lookup).expect("read the vectors");
    std::process::exit(if print_report(&rows) { 0 } else { 1 });
}
