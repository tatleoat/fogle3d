// SPDX-License-Identifier: MIT
//! `shdb_yacc.c`: the node builders and `SHDB_yyparse`, a bison 1.x parser for the
//! `_SHADER_PREVIEWER` text. The tables are read from the image where the original keeps them
//! ([`crate::shdb_data`]).
//!
//! The parser's own stacks (200 states growing to 10000, 16 bytes of value each) are locals of
//! the C function on the machine stack; here they are `Vec`s. Nothing else reaches them. The
//! original's value of `$$` for a rule with no right-hand side, and the unused words of `$$`,
//! come from uninitialised stack; here they are 0.

use crate::shdb_global::{SHDB_YYCHAR, SHDB_YYDEBUG, SHDB_YYLVAL, SHDB_YYNERRS, SI_SHDB_ROOT};
use crate::shdb_lex::{shdb_yyerror, shdb_yylex, si_shdb_error_token};
use crate::shdb_support::{si_shdb_alloc_node, CHILD, NEXT};
use crate::{fprintf, through_x87, STDERR};
use si_mem::{Mem, Outside};

// the tables (`.rodata`) and the texts of the debug output
const YYTRANSLATE: u32 = 0x0866_3940;
const YYPRHS: u32 = 0x0866_3a80;
const YYRHS: u32 = 0x0866_3b40;
const YYRLINE: u32 = 0x0866_3d00;
const YYTNAME: u32 = 0x0866_3dc0;
const YYR1: u32 = 0x0866_3f00;
const YYR2: u32 = 0x0866_3fc0;
const YYDEFACT: u32 = 0x0866_4080;
const YYPACT: u32 = 0x0866_41e0;
const YYTABLE: u32 = 0x0866_4340;
const YYCHECK: u32 = 0x0866_4440;
/// `yypgoto` and `yydefgoto` as the code addresses them (the rule's left-hand side, a
/// nonterminal numbered from 41, indexes them from 41 entries before their start).
const YYPGOTO_BASE: u32 = 0x0866_42ae;
const YYDEFGOTO_BASE: u32 = 0x0866_414e;

const S_STARTING: u32 = 0x086e_56e0;
const S_OVERFLOW: u32 = 0x086e_56f0;
const S_STACK_SIZE: u32 = 0x086e_5706;
const S_ENTERING: u32 = 0x086e_5722;
const S_READING: u32 = 0x086e_5735;
const S_END_OF_INPUT: u32 = 0x086e_5747;
const S_NEXT_TOKEN: u32 = 0x086e_575d;
const S_CLOSE: u32 = 0x086e_5772;
const S_SHIFTING: u32 = 0x086e_5775;
const S_REDUCING: u32 = 0x086e_57a0;
const S_RHS_ITEM: u32 = 0x086e_57c1;
const S_ARROW: u32 = 0x086e_57c5;
const S_STATE_STACK: u32 = 0x086e_57cd;
const S_STACK_ITEM: u32 = 0x086e_57dd;
const S_NEWLINE: u32 = 0x086e_57e1;
const S_PARSE_ERROR: u32 = 0x086e_57e3;
const S_DISCARDING: u32 = 0x086e_57ef;
const S_ERR_STATE_STACK: u32 = 0x086e_580a;
const S_ERR_STACK_ITEM: u32 = 0x086e_5821;
const S_ERR_NEWLINE: u32 = 0x086e_5825;
const S_SHIFTING_ERROR: u32 = 0x086e_5827;

/// [soft 0x08550998 addNodeFloat]: a node with a type, a child and one float.
pub fn add_node_float(m: &mut Mem, kind: i32, parent: u32, value: u32) -> u32 {
    let n = si_shdb_alloc_node(m);
    // the float goes through the x87 stack: a signalling NaN comes out quiet
    m.set_u32(n + 4, through_x87(value));
    m.set_u32(n, kind as u32);
    m.set_u32(n + CHILD, parent);
    n
}

/// [soft 0x085509bc addNodeVector3]: a node with a type, a child and three words.
pub fn add_node_vector3(m: &mut Mem, kind: i32, parent: u32, x: u32, y: u32, z: u32) -> u32 {
    let n = si_shdb_alloc_node(m);
    m.set_u32(n, kind as u32);
    m.set_u32(n + CHILD, parent);
    m.set_u32(n + 4, x);
    m.set_u32(n + 8, y);
    m.set_u32(n + 0xc, z);
    n
}

/// [soft 0x085509f0 addNodeColor3]: a node with a type, a child and four words.
pub fn add_node_color3(m: &mut Mem, kind: i32, parent: u32, a: u32, b: u32, c: u32, d: u32) -> u32 {
    let n = si_shdb_alloc_node(m);
    m.set_u32(n, kind as u32);
    m.set_u32(n + CHILD, parent);
    m.set_u32(n + 4, a);
    m.set_u32(n + 8, b);
    m.set_u32(n + 0xc, c);
    m.set_u32(n + 0x10, d);
    n
}

/// [soft 0x08550a2c addNodeInteger]: a node with a type, a child and one integer.
pub fn add_node_integer(m: &mut Mem, kind: i32, parent: u32, value: i32) -> u32 {
    let n = si_shdb_alloc_node(m);
    m.set_u32(n, kind as u32);
    m.set_u32(n + CHILD, parent);
    m.set_u32(n + 4, value as u32);
    n
}

/// A float word compared with 0.0 (`fcomp`): both zeros equal. (The words compared are node
/// pointers or 0, never NaNs.)
fn is_zero(bits: u32) -> bool {
    bits & 0x7fff_ffff == 0
}

/// A float word cut to an integer, the rounding control set to truncation (the `fistp` of
/// the original); out of range and NaN give the x87's "integer indefinite".
fn trunc_i32(bits: u32) -> i32 {
    let v = f32::from_bits(bits);
    if v.is_nan() || v >= 2147483648.0 || v < -2147483648.0 {
        i32::MIN
    } else {
        v as i32
    }
}

/// The walk of `si_shdbAddNode (0x0854fad4)` on a chain whose head is held outside the image
/// (a slot of the parser's value stack): puts `node` at the end of the `link` chain and
/// answers the head.
pub(crate) fn add_to_chain(m: &mut Mem, head: u32, node: u32, link: u32) -> u32 {
    if head == 0 {
        return node;
    }
    let mut at = head;
    while m.u32(at + link) != 0 {
        at = m.u32(at + link);
    }
    m.set_u32(at + link, node);
    head
}

struct Parser {
    ss: Vec<i16>,
    vs: Vec<[u32; 4]>,
    /// index of the top entry; -1 before the first state
    sp: isize,
    stacksize: i32,
}

impl Parser {
    fn top(&self, back: isize, word: usize) -> u32 {
        self.vs.get((self.sp - back) as usize).map_or(0, |e| e[word])
    }
    fn set_vs(&mut self, at: isize, e: [u32; 4]) {
        let at = at as usize;
        if self.vs.len() <= at {
            self.vs.resize(at + 1, [0; 4]);
        }
        self.vs[at] = e;
    }
    fn set_ss(&mut self, at: isize, s: i16) {
        let at = at as usize;
        if self.ss.len() <= at {
            self.ss.resize(at + 1, 0);
        }
        self.ss[at] = s;
    }
}

fn dbg(m: &mut Mem, o: &mut dyn Outside, fmt: u32, args: &[u32]) {
    let err = m.u32(STDERR);
    fprintf(m, o, err, fmt, args);
}

fn debug_on(m: &Mem) -> bool {
    m.u32(SHDB_YYDEBUG) != 0
}

fn tname(m: &Mem, idx: i32) -> u32 {
    m.u32(YYTNAME.wrapping_add((idx as u32).wrapping_mul(4)))
}

/// The debug listing of the state stack: `first` is printed, then every state with `item`,
/// then `last`.
fn dump_states(m: &mut Mem, o: &mut dyn Outside, p: &Parser, first: u32, item: u32, last: u32) {
    dbg(m, o, first, &[]);
    for i in 0..=p.sp {
        dbg(m, o, item, &[p.ss[i as usize] as i32 as u32]);
    }
    dbg(m, o, last, &[]);
}

#[derive(Clone, Copy)]
enum Go {
    NewState,
    Backup,
    Default,
    Reduce(i32),
    ErrLab,
    ErrHandle,
}

/// [soft 0x0854fbe0 SHDB_yyparse]: parses the text the scanner reads into the tree at
/// `si_shdbRoot`. Answers 0 (accepted), 1 (aborted by an error) or 2 (stack overflow).
///
/// Differences (the original reads uninitialised or machine-stack memory there): see the
/// module text; the first debug listing of a state stack passes an unused word in the
/// original, none here.
pub fn shdb_yyparse(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let mut p = Parser { ss: vec![0; 200], vs: vec![[0; 4]; 200], sp: -1, stacksize: 200 };
    let mut errstatus: i32 = 0;
    let mut yychar1: i32 = 0;
    if debug_on(m) {
        dbg(m, o, S_STARTING, &[]);
    }
    let mut state: i32 = 0;
    m.set_u32(SHDB_YYNERRS, 0);
    m.set_u32(SHDB_YYCHAR, 0xffff_fffe);

    let mut go = Go::NewState;
    loop {
        match go {
            Go::NewState => {
                p.sp += 1;
                p.set_ss(p.sp, state as i16);
                if p.sp >= (p.stacksize - 1) as isize {
                    if p.stacksize >= 10000 {
                        shdb_yyerror(m, o, S_OVERFLOW);
                        return 2;
                    }
                    p.stacksize = (p.stacksize * 2).min(10000);
                    let n = p.stacksize as usize;
                    p.ss.resize(n.max(p.ss.len()), 0);
                    p.vs.resize(n.max(p.vs.len()), [0; 4]);
                    if debug_on(m) {
                        dbg(m, o, S_STACK_SIZE, &[p.stacksize as u32]);
                    }
                    if p.sp >= (p.stacksize - 1) as isize {
                        return 1;
                    }
                }
                go = Go::Backup;
            }
            Go::Backup => {
                if debug_on(m) {
                    dbg(m, o, S_ENTERING, &[state as u32]);
                }
                let pact = m.i16(YYPACT + (state as u32) * 2) as i32;
                if pact == -0x8000 {
                    go = Go::Default;
                    continue;
                }
                if m.u32(SHDB_YYCHAR) == 0xffff_fffe {
                    if debug_on(m) {
                        dbg(m, o, S_READING, &[]);
                    }
                    let c = shdb_yylex(m, o);
                    m.set_u32(SHDB_YYCHAR, c as u32);
                }
                let yychar = m.u32(SHDB_YYCHAR) as i32;
                if yychar < 1 {
                    yychar1 = 0;
                    m.set_u32(SHDB_YYCHAR, 0);
                    if debug_on(m) {
                        dbg(m, o, S_END_OF_INPUT, &[]);
                    }
                } else {
                    yychar1 = if yychar < 0x127 { m.u8(YYTRANSLATE + yychar as u32) as i8 as i32 } else { 0x49 };
                    if debug_on(m) {
                        let nm = tname(m, yychar1);
                        dbg(m, o, S_NEXT_TOKEN, &[yychar as u32, nm]);
                        dbg(m, o, S_CLOSE, &[]);
                    }
                }
                let n = (pact + yychar1) as u32;
                if n > 0x7c || m.i16(YYCHECK + n * 2) as i32 != yychar1 {
                    go = Go::Default;
                    continue;
                }
                let n = m.i16(YYTABLE + n * 2) as i32;
                if n < 0 {
                    go = if n == -0x8000 { Go::ErrLab } else { Go::Reduce(-n) };
                } else if n == 0 {
                    go = Go::ErrLab;
                } else if n == 0x80 {
                    return 0;
                } else {
                    if debug_on(m) {
                        let nm = tname(m, yychar1);
                        let c = m.u32(SHDB_YYCHAR);
                        dbg(m, o, S_SHIFTING, &[c, nm]);
                    }
                    if m.u32(SHDB_YYCHAR) != 0 {
                        m.set_u32(SHDB_YYCHAR, 0xffff_fffe);
                    }
                    let lv = [m.u32(SHDB_YYLVAL), m.u32(SHDB_YYLVAL + 4), m.u32(SHDB_YYLVAL + 8), m.u32(SHDB_YYLVAL + 12)];
                    p.set_vs(p.sp + 1, lv);
                    state = n;
                    if errstatus != 0 {
                        errstatus -= 1;
                    }
                    go = Go::NewState;
                }
            }
            Go::Default => {
                let n = m.i16(YYDEFACT + (state as u32) * 2) as i32;
                go = if n == 0 { Go::ErrLab } else { Go::Reduce(n) };
            }
            Go::ErrLab => {
                if errstatus == 0 {
                    let e = m.u32(SHDB_YYNERRS).wrapping_add(1);
                    m.set_u32(SHDB_YYNERRS, e);
                    shdb_yyerror(m, o, S_PARSE_ERROR);
                }
                if errstatus != 3 {
                    errstatus = 3;
                    go = Go::ErrHandle;
                } else {
                    if m.u32(SHDB_YYCHAR) == 0 {
                        return 1;
                    }
                    if debug_on(m) {
                        let nm = tname(m, yychar1);
                        let c = m.u32(SHDB_YYCHAR);
                        dbg(m, o, S_DISCARDING, &[c, nm]);
                    }
                    m.set_u32(SHDB_YYCHAR, 0xffff_fffe);
                    errstatus = 3;
                    go = Go::ErrHandle;
                }
            }
            Go::ErrHandle => {
                let pact = m.i16(YYPACT + (state as u32) * 2) as i32;
                if pact != -0x8000 {
                    let n = (pact + 1) as u32;
                    if n < 0x7d && m.i16(YYCHECK + n * 2) == 1 {
                        let t = m.i16(YYTABLE + n * 2) as i32;
                        if t < 0 {
                            if t != -0x8000 {
                                go = Go::Reduce(-t);
                                continue;
                            }
                        } else if t != 0 {
                            if t == 0x80 {
                                return 0;
                            }
                            if debug_on(m) {
                                dbg(m, o, S_SHIFTING_ERROR, &[]);
                            }
                            let lv = [m.u32(SHDB_YYLVAL), m.u32(SHDB_YYLVAL + 4), m.u32(SHDB_YYLVAL + 8), m.u32(SHDB_YYLVAL + 12)];
                            p.set_vs(p.sp + 1, lv);
                            state = t;
                            go = Go::NewState;
                            continue;
                        }
                    }
                }
                if p.sp == 0 {
                    return 1;
                }
                p.sp -= 1;
                state = p.ss[p.sp as usize] as i32;
                if debug_on(m) {
                    dump_states(m, o, &p, S_ERR_STATE_STACK, S_ERR_STACK_ITEM, S_ERR_NEWLINE);
                }
            }
            Go::Reduce(rule) => {
                let len = m.i16(YYR2 + (rule as u32) * 2) as i32 as isize;
                let mut v = if len > 0 { p.vs[(p.sp + 1 - len) as usize] } else { [0; 4] };
                if debug_on(m) {
                    let line = m.i16(YYRLINE + (rule as u32) * 2) as i32 as u32;
                    dbg(m, o, S_REDUCING, &[rule as u32, line]);
                    let mut at = YYRHS + (m.i16(YYPRHS + (rule as u32) * 2) as i32 as u32) * 2;
                    while m.i16(at) > 0 {
                        let nm = tname(m, m.i16(at) as i32);
                        dbg(m, o, S_RHS_ITEM, &[nm]);
                        at += 2;
                    }
                    let nm = tname(m, m.i16(YYR1 + (rule as u32) * 2) as i32);
                    dbg(m, o, S_ARROW, &[nm]);
                }
                reduce(m, o, &mut p, rule, &mut v);
                p.sp -= len;
                if debug_on(m) {
                    dump_states(m, o, &p, S_STATE_STACK, S_STACK_ITEM, S_NEWLINE);
                }
                p.set_vs(p.sp + 1, v);
                let r1 = m.i16(YYR1 + (rule as u32) * 2) as i32 as u32;
                let top = p.ss[p.sp as usize] as i32;
                let n = (m.i16(YYPGOTO_BASE + r1 * 2) as i32).wrapping_add(top) as u32;
                state = if n < 0x7d && m.i16(YYCHECK + n * 2) as i32 == top {
                    m.i16(YYTABLE + n * 2) as i32
                } else {
                    m.i16(YYDEFGOTO_BASE + r1 * 2) as i32
                };
                go = Go::NewState;
            }
        }
    }
}

/// The action of one rule: `v` holds `$$` (the words of `$1`) and is left as the rule makes
/// it. `top(k, w)` is word `w` of the k-th entry below the top of the value stack (the
/// last symbol of the rule).
fn reduce(m: &mut Mem, o: &mut dyn Outside, p: &mut Parser, rule: i32, v: &mut [u32; 4]) {
    let t0 = p.top(0, 0);
    let (t1, t2) = (p.top(1, 0), p.top(2, 0));
    let vec3 = (p.top(0, 0), p.top(0, 1), p.top(0, 2));
    let col4 = (p.top(0, 0), p.top(0, 1), p.top(0, 2), p.top(0, 3));
    // a node of one float whose child is the top value / whose float is the top value
    let child_float = |m: &mut Mem, kind: i32| add_node_float(m, kind, t0, 0);
    let value_float = |m: &mut Mem, kind: i32| add_node_float(m, kind, 0, t0);
    match rule {
        1 | 0x10 | 0x12 | 0x14 | 0x16 | 0x18 | 0x1a | 0x37 | 0x41 | 0x4f => v[0] = 0,
        2 => {
            let slot = (p.sp - 1) as usize;
            if is_zero(t1) {
                p.vs[slot][0] = t0;
            } else {
                // si_shdbAddNode (0x0854fad4), mode 1, on the slot of the value stack
                p.vs[slot][0] = add_to_chain(m, t1, t0, NEXT);
            }
            let root = p.vs[slot][0];
            m.set_u32(SI_SHDB_ROOT, root);
            v[0] = root;
        }
        3..=8 | 0x1e | 0x22 | 0x23 => v[0] = t0,
        9 | 0x1f | 0x24 | 0x29 | 0x2d | 0x30 | 0x35 | 0x3f | 0x49 => {
            v[0] = 0;
            si_shdb_error_token(m, o, t0);
        }
        10 => v[0] = child_float(m, 2),
        0xb => v[0] = child_float(m, 3),
        0xc => v[0] = child_float(m, 1),
        0xd => v[0] = child_float(m, 4),
        0xe => v[0] = child_float(m, 5),
        0xf => v[0] = child_float(m, 6),
        0x36 => v[0] = child_float(m, 0xf),
        0x40 => v[0] = child_float(m, 0x17),
        0x11 | 0x13 | 0x15 | 0x17 | 0x19 | 0x1b | 0x38 | 0x42 => {
            let slot = (p.sp - 1) as usize;
            if is_zero(t1) {
                p.vs[slot][0] = t0;
            } else {
                // si_shdbAddNode (0x0854fad4), mode 2
                p.vs[slot][0] = add_to_chain(m, t1, t0, CHILD);
            }
            v[0] = p.vs[slot][0];
        }
        0x1c | 0x20 | 0x25 => v[0] = value_float(m, 9),
        0x1d => v[0] = value_float(m, 8),
        0x26 => v[0] = value_float(m, 0xc),
        0x39 => v[0] = value_float(m, 0x10),
        0x3a => v[0] = value_float(m, 0x15),
        0x3b => v[0] = value_float(m, 0x14),
        0x46 => v[0] = value_float(m, 0x1b),
        0x47 => v[0] = value_float(m, 0x1c),
        0x21 | 0x2f => v[0] = add_node_vector3(m, 8, 0, vec3.0, vec3.1, vec3.2),
        0x27 => v[0] = add_node_vector3(m, 10, 0, vec3.0, vec3.1, vec3.2),
        0x28 => v[0] = add_node_vector3(m, 0xb, 0, vec3.0, vec3.1, vec3.2),
        0x2a | 0x2e => v[0] = add_node_vector3(m, 7, 0, vec3.0, vec3.1, vec3.2),
        0x2b => v[0] = add_node_color3(m, 0xd, 0, col4.0, col4.1, col4.2, col4.3),
        0x3c => v[0] = add_node_color3(m, 0x11, 0, col4.0, col4.1, col4.2, col4.3),
        0x3d | 0x48 => v[0] = add_node_color3(m, 0x12, 0, col4.0, col4.1, col4.2, col4.3),
        0x3e => v[0] = add_node_color3(m, 0x13, 0, col4.0, col4.1, col4.2, col4.3),
        0x43 => v[0] = add_node_color3(m, 0x19, 0, col4.0, col4.1, col4.2, col4.3),
        0x44 => v[0] = add_node_color3(m, 0x1a, 0, col4.0, col4.1, col4.2, col4.3),
        0x45 => v[0] = add_node_color3(m, 0x18, 0, col4.0, col4.1, col4.2, col4.3),
        0x2c => v[0] = add_node_integer(m, 0xe, 0, t0 as i32),
        0x31 => v[0] = add_node_integer(m, 0x1d, 0, trunc_i32(t0)),
        0x32 => v[0] = add_node_integer(m, 0x1e, 0, trunc_i32(t0)),
        0x33 => v[0] = add_node_integer(m, 0x1f, 0, t0 as i32),
        0x34 => v[0] = add_node_integer(m, 0x20, 0, t0 as i32),
        0x4a => {
            v[0] = t2;
            v[1] = t1;
            v[2] = t0;
            v[3] = 0;
        }
        0x4b => {
            v[0] = t2;
            v[1] = t1;
            v[2] = t0;
        }
        0x4c => {
            v[0] = t1;
            v[1] = t0;
            v[2] = 0;
        }
        0x4d => v[0] = t0,
        0x4e => v[0] = 1,
        0x50 => v[0] = !is_zero(t0) as u32,
        _ => {}
    }
}
