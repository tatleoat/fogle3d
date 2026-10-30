// SPDX-License-Identifier: MIT
//! `mi2control_yacc.c`: `mi2_yyparse`, the command parser of mental ray's input (package 62, finding 0042). A bison 1.x LALR(1) parser over 51
//! states and 37 rules; the commands it recognises (exit, help, abort, set, render, translate, init, connect, disconnect, the queries, open, close
//! and the scene render with its ranges) call the `si_mi2*` routines of `mi2control.c` and answer on `stderr` (one `<command> ok` line each) and on
//! `si_stdOutput` (`output_ok` / `output_error`). The scanner is [`crate::mi2control_lex::mi2_yylex`], called through `Outside` (as are the
//! commands), the parse stacks are Rust vectors (the original keeps 200 entries in its frame and moves to alloca'd blocks of up to 10000).
//! The tables and the texts are the original's, read at run time from the user's install (`si-rt`).

use crate::mi2control_lex::{mi2_yyerror, output_error, output_ok, FPRINTF};
use si_mem::{Mem, Outside, Routine};
use si_rt::Table;
use std::sync::LazyLock;

pub const MI2_YYLEX: Routine = Routine::new("mi2_yylex", 0x0842_e9e0);
pub const EXIT: Routine = Routine::new("exit", 0x080c_4b58);
pub const SI_MI2_EXIT_TRANSLATOR: Routine = Routine::new("si_mi2ExitTranslator", 0x0842_dca8);
pub const SI_MI2_INIT_TRANSLATOR: Routine = Routine::new("si_mi2InitTranslator", 0x0842_db20);
pub const SI_MI2_CONNECT_TO_FILE: Routine = Routine::new("si_mi2ConnectToFile", 0x0842_dee8);
pub const SI_MI2_DISCONNECT: Routine = Routine::new("si_mi2Disconnect", 0x0842_df70);
pub const SI_MI2_QUERY_TRANSLATOR: Routine = Routine::new("si_mi2QueryTranslator", 0x0842_e080);
pub const SI_MI2_QUERY_NAME: Routine = Routine::new("si_mi2QueryName", 0x0842_e0c0);
pub const SI_MI2_QUERY_FRAME: Routine = Routine::new("si_mi2QueryFrame", 0x0842_e03c);
pub const SI_MI2_QUERY_RANGE: Routine = Routine::new("si_mi2QueryRange", 0x0842_dfc0);
pub const SI_MI2_QUERY_OUTPUT: Routine = Routine::new("si_mi2QueryOutput", 0x0842_e12c);
pub const SI_MI2_QUERY_CAPS: Routine = Routine::new("si_mi2QueryCaps", 0x0842_e174);
pub const SI_MI2_OPEN_SCENE: Routine = Routine::new("si_mi2OpenScene", 0x0842_dcb4);
pub const SI_MI2_CLOSE_SCENE: Routine = Routine::new("si_mi2CloseScene", 0x0842_deb4);
pub const SI_MI2_SCENE_RENDER: Routine = Routine::new("si_mi2SceneRender", 0x0842_dad0);

/// The parser's globals: `mi2_yychar`, `mi2_yylval`, `mi2_yynerrs`; `stderr`.
pub const MI2_YYCHAR: u32 = 0x0881_ada0;
pub const MI2_YYLVAL: u32 = 0x0881_ada4;
pub const MI2_YYNERRS: u32 = 0x0881_ada8;
const STDERR: u32 = 0x0878_0c88;

/// The texts the parser prints, at their addresses: one range of the original's read-only data
/// (0x086d30a0 .. 0x086d31a2: the stack-overflow text, the `<command> ok` lines, the parse-error text,
/// the reply formats), read when first stored from the user's own install (`si-rt`; release model:
/// nothing of the original ships), checked against our checksum of it. The addresses the routines
/// below say are inside it.
pub static MESSAGES: Table = Table::new("si_mental.mi2yacc_messages", 0x086d_30a0, 0x102, 0x8d29_fc2f_baca_f961);

/// Puts the texts into the image (read-only data).
pub fn store_messages(m: &mut Mem) {
    m.set_rom(MESSAGES.addr, MESSAGES.get());
}

/// Decodes a table of little-endian `i8`/`i16`/`u16` entries.
fn decode<T: Copy, const N: usize>(t: &Table, size: usize, f: fn(&[u8]) -> T) -> [T; N] {
    let b = t.get();
    assert_eq!(b.len(), N * size);
    std::array::from_fn(|i| f(&b[i * size..(i + 1) * size]))
}
fn i8_of(b: &[u8]) -> i8 {
    b[0] as i8
}
fn i16_of(b: &[u8]) -> i16 {
    i16::from_le_bytes([b[0], b[1]])
}
fn u16_of(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

// The parser's tables, ranges of the original's read-only data (bison 1.x names): each is named by
// its address and length and read at run time from the user's install, checked against our checksum.
/// `yytranslate` (0x0865df00): token number to symbol, for 0..=0x11a.
static YYTRANSLATE_T: Table = Table::new("si_mental.mi2yacc_yytranslate", 0x0865df00, 0x11b, 0x12fc_4bb2_a21a_5720);
static YYTRANSLATE: LazyLock<[i8; 283]> = LazyLock::new(|| decode(&YYTRANSLATE_T, 1, i8_of));
/// `yyr1` (0x0865e020): the left side of each rule.
static YYR1_T: Table = Table::new("si_mental.mi2yacc_yyr1", 0x0865e020, 0x60, 0x3e3b_63dd_e0b8_df1a);
static YYR1: LazyLock<[i16; 48]> = LazyLock::new(|| decode(&YYR1_T, 2, i16_of));
/// `yyr2` (0x0865e080): the length of each rule.
static YYR2_T: Table = Table::new("si_mental.mi2yacc_yyr2", 0x0865e080, 0x60, 0xe297_77bd_9e2c_18f6);
static YYR2: LazyLock<[i16; 48]> = LazyLock::new(|| decode(&YYR2_T, 2, i16_of));
/// `yydefact` (0x0865e0e0): the default rule of each state.
static YYDEFACT_T: Table = Table::new("si_mental.mi2yacc_yydefact", 0x0865e0e0, 0x66, 0x7438_28b0_c31a_dc54);
static YYDEFACT: LazyLock<[i16; 51]> = LazyLock::new(|| decode(&YYDEFACT_T, 2, i16_of));
/// `yydefgoto` (0x0865e146): indexed by the left side of a rule minus 30.
static YYDEFGOTO_T: Table = Table::new("si_mental.mi2yacc_yydefgoto", 0x0865e146, 0x16, 0xdefb_c463_06d7_a181);
static YYDEFGOTO: LazyLock<[i16; 11]> = LazyLock::new(|| decode(&YYDEFGOTO_T, 2, i16_of));
/// `yypact` (0x0865e160): the action index of each state.
static YYPACT_T: Table = Table::new("si_mental.mi2yacc_yypact", 0x0865e160, 0x66, 0x6140_0589_9331_3ad2);
static YYPACT: LazyLock<[i16; 51]> = LazyLock::new(|| decode(&YYPACT_T, 2, i16_of));
/// `yypgoto` (0x0865e1c6): indexed by the left side of a rule minus 30.
static YYPGOTO_T: Table = Table::new("si_mental.mi2yacc_yypgoto", 0x0865e1c6, 0x16, 0x44e2_acd3_cc2d_2a48);
static YYPGOTO: LazyLock<[i16; 11]> = LazyLock::new(|| decode(&YYPGOTO_T, 2, i16_of));
/// `yytable` (0x0865e1e0).
static YYTABLE_T: Table = Table::new("si_mental.mi2yacc_yytable", 0x0865e1e0, 0x66, 0x3010_43c0_d0a5_d25e);
static YYTABLE: LazyLock<[i16; 51]> = LazyLock::new(|| decode(&YYTABLE_T, 2, i16_of));
/// `yycheck` (0x0865e260).
static YYCHECK_T: Table = Table::new("si_mental.mi2yacc_yycheck", 0x0865e260, 0x66, 0x7631_4733_3a63_3df9);
static YYCHECK: LazyLock<[u16; 51]> = LazyLock::new(|| decode(&YYCHECK_T, 2, u16_of));

fn say(m: &mut Mem, o: &mut dyn Outside, text: u32) {
    let err = m.u32(STDERR);
    o.call(m, FPRINTF, &[err, text]);
}

/// A command that answers -1 when it went well: `<command> ok` on `stderr`, then (when `reply`) "ok" on `si_stdOutput`; any other answer is an
/// error reply there.
fn finish(m: &mut Mem, o: &mut dyn Outside, r: u32, text: u32, reply: bool) {
    if r == 0xffff_ffff {
        say(m, o, text);
        if reply {
            output_ok(m, o);
        }
    } else {
        output_error(m, o, r as i32);
    }
}

/// [soft 0x0842e1d0 mi2_yyparse]: 0 when the input was parsed to the end, 1 when it could not be parsed, 2 when the parser stack overflowed.
pub fn mi2_yyparse(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let mut size: usize = 200;
    let mut ss: Vec<i16> = vec![0; size];
    let mut vs: Vec<u32> = vec![0; size];
    let (mut state, mut errstatus): (i32, i32) = (0, 0);
    let (mut ssp, mut vsp): (usize, usize) = (0, 0);
    let mut first = true;
    let mut yyval: u32 = 0;
    m.set_u32(MI2_YYNERRS, 0);
    m.set_i32(MI2_YYCHAR, -2);
    // where the machine is: 0 new state, 1 backup, 2 default, 3 reduce, 4 error, 5 error handling
    let mut at = 0;
    let mut n: i32 = 0;
    loop {
        match at {
            0 => {
                // yynewstate
                if first {
                    first = false;
                } else {
                    ssp += 1;
                }
                if ssp >= ss.len() {
                    ss.resize(ssp + 1, 0);
                }
                ss[ssp] = state as i16;
                if ssp + 1 >= size {
                    let used = ssp + 1;
                    if size >= 10000 {
                        mi2_yyerror_text(m, o, 0x086d_30a0);
                        return 2;
                    }
                    size = (2 * size).min(10000);
                    ss.resize(size.max(used), 0);
                    vs.resize(size.max(used), 0);
                    if ssp + 1 >= size {
                        return 1;
                    }
                }
                at = 1;
            }
            1 => {
                // yybackup
                let p = YYPACT[state as usize] as i32;
                if p == -32768 {
                    at = 2;
                    continue;
                }
                if m.i32(MI2_YYCHAR) == -2 {
                    let c = o.call(m, MI2_YYLEX, &[]).eax;
                    m.set_u32(MI2_YYCHAR, c);
                }
                let c = m.i32(MI2_YYCHAR);
                let c1: i32 = if c <= 0 {
                    m.set_u32(MI2_YYCHAR, 0);
                    0
                } else if (c as u32) <= 0x11a {
                    YYTRANSLATE[c as usize] as i32
                } else {
                    41
                };
                let k = p + c1;
                if (k as u32) > 0x32 || YYCHECK[k as usize] as i32 != c1 {
                    at = 2;
                    continue;
                }
                let t = YYTABLE[k as usize] as i32;
                if t < 0 {
                    if t == -32768 {
                        at = 4;
                        continue;
                    }
                    n = -t;
                    at = 3;
                } else if t == 0 {
                    at = 4;
                } else if t == 0x32 {
                    return 0;
                } else {
                    if m.i32(MI2_YYCHAR) != 0 {
                        m.set_i32(MI2_YYCHAR, -2);
                    }
                    vsp += 1;
                    if vsp >= vs.len() {
                        vs.resize(vsp + 1, 0);
                    }
                    vs[vsp] = m.u32(MI2_YYLVAL);
                    if errstatus != 0 {
                        errstatus -= 1;
                    }
                    state = t;
                    at = 0;
                }
            }
            2 => {
                // yydefault
                n = YYDEFACT[state as usize] as i32;
                at = if n == 0 { 4 } else { 3 };
            }
            3 => {
                // yyreduce
                let len = YYR2[n as usize] as i32;
                if len > 0 {
                    yyval = vs[(vsp as i32 + 1 - len) as usize];
                }
                let v = |i: i32| vs[(vsp as i32 + i) as usize];
                let rule = n;
                if (4..=35).contains(&rule) {
                    match rule {
                        4 => {
                            let r = o.call(m, SI_MI2_EXIT_TRANSLATOR, &[]).eax;
                            if r == 0xffff_ffff {
                                say(m, o, 0x086d_30b6);
                                output_ok(m, o);
                                o.call(m, EXIT, &[0]);
                                say(m, o, 0x086d_30bf);
                            }
                        }
                        5 => say(m, o, 0x086d_30bf),
                        6 => say(m, o, 0x086d_30c8),
                        10 => say(m, o, 0x086d_30d2),
                        13 => say(m, o, 0x086d_30da),
                        14 => say(m, o, 0x086d_30e5),
                        15 => {
                            let r = o.call(m, SI_MI2_INIT_TRANSLATOR, &[v(0)]).eax;
                            finish(m, o, r, 0x086d_30f3, true);
                        }
                        16 => {
                            let r = o.call(m, SI_MI2_CONNECT_TO_FILE, &[v(0)]).eax;
                            finish(m, o, r, 0x086d_30fc, true);
                        }
                        17 => {
                            let r = o.call(m, SI_MI2_DISCONNECT, &[]).eax;
                            finish(m, o, r, 0x086d_3108, true);
                        }
                        18 => {
                            let r = o.call(m, SI_MI2_QUERY_TRANSLATOR, &[]).eax;
                            finish(m, o, r, 0x086d_3117, false);
                        }
                        19 => {
                            let r = o.call(m, SI_MI2_QUERY_NAME, &[]).eax;
                            finish(m, o, r, 0x086d_312c, false);
                        }
                        20 => {
                            let r = o.call(m, SI_MI2_QUERY_FRAME, &[]).eax;
                            finish(m, o, r, 0x086d_313b, false);
                        }
                        21 => {
                            let r = o.call(m, SI_MI2_QUERY_RANGE, &[]).eax;
                            finish(m, o, r, 0x086d_314b, false);
                        }
                        22 => {
                            let r = o.call(m, SI_MI2_QUERY_OUTPUT, &[]).eax;
                            finish(m, o, r, 0x086d_315b, false);
                        }
                        23 => {
                            let r = o.call(m, SI_MI2_QUERY_CAPS, &[]).eax;
                            finish(m, o, r, 0x086d_316c, false);
                        }
                        30 => {
                            let r = o.call(m, SI_MI2_OPEN_SCENE, &[v(0)]).eax;
                            finish(m, o, r, 0x086d_317b, true);
                        }
                        31 => {
                            let r = o.call(m, SI_MI2_CLOSE_SCENE, &[]).eax;
                            finish(m, o, r, 0x086d_3184, true);
                        }
                        32..=35 => {
                            let args = match rule {
                                32 => [v(-3), v(-2), v(-1), v(0)],
                                33 => [v(-2), v(-1), v(0), 0],
                                34 => [v(-1), v(0), 1, 0],
                                _ => [v(0), v(0), 1, 0],
                            };
                            let r = o.call(m, SI_MI2_SCENE_RENDER, &args).eax;
                            if r == 0xffff_ffff {
                                output_ok(m, o);
                            } else {
                                output_error(m, o, r as i32);
                            }
                        }
                        _ => {}
                    }
                }
                vsp = (vsp as i32 - len) as usize;
                ssp = (ssp as i32 - len) as usize;
                vsp += 1;
                if vsp >= vs.len() {
                    vs.resize(vsp + 1, 0);
                }
                vs[vsp] = yyval;
                let lhs = YYR1[n as usize] as usize;
                let top = ss[ssp] as i32;
                let k = YYPGOTO[lhs - 30] as i32 + top;
                if (k as u32) <= 0x32 && YYCHECK[k as usize] == top as u16 {
                    state = YYTABLE[k as usize] as i32;
                } else {
                    state = YYDEFGOTO[lhs - 30] as i32;
                }
                at = 0;
            }
            4 => {
                // yyerrlab
                if errstatus == 0 {
                    let e = m.u32(MI2_YYNERRS).wrapping_add(1);
                    m.set_u32(MI2_YYNERRS, e);
                    mi2_yyerror_text(m, o, 0x086d_318e);
                }
                if errstatus == 3 {
                    if m.i32(MI2_YYCHAR) == 0 {
                        return 1;
                    }
                    m.set_i32(MI2_YYCHAR, -2);
                }
                errstatus = 3;
                at = 5;
            }
            _ => {
                // yyerrhandle
                let p = YYPACT[state as usize] as i32;
                let mut pop = true;
                if p != -32768 {
                    let k = p + 1;
                    if (k as u32) <= 0x32 && YYCHECK[k as usize] == 1 {
                        let t = YYTABLE[k as usize] as i32;
                        if t < 0 {
                            if t != -32768 {
                                n = -t;
                                at = 3;
                                pop = false;
                            }
                        } else if t != 0 {
                            if t == 0x32 {
                                return 0;
                            }
                            vsp += 1;
                            if vsp >= vs.len() {
                                vs.resize(vsp + 1, 0);
                            }
                            vs[vsp] = m.u32(MI2_YYLVAL);
                            state = t;
                            at = 0;
                            pop = false;
                        }
                    }
                }
                if pop {
                    if ssp == 0 {
                        return 1;
                    }
                    vsp -= 1;
                    ssp -= 1;
                    state = ss[ssp] as i32;
                }
            }
        }
    }
}

/// `mi2_yyerror(text)` for a text of the image.
fn mi2_yyerror_text(m: &mut Mem, o: &mut dyn Outside, text: u32) {
    mi2_yyerror(m, o, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With an install: every table and the text range load (each checked against our checksum),
    /// and the stored texts read back from the image.
    #[test]
    fn tables_and_texts_load() {
        si_rt::skip_without_install!();
        assert_eq!((YYTRANSLATE.len(), YYR1.len(), YYR2.len(), YYDEFACT.len(), YYDEFGOTO.len()), (283, 48, 48, 51, 11));
        assert_eq!((YYPACT.len(), YYPGOTO.len(), YYTABLE.len(), YYCHECK.len()), (51, 11, 51, 51));
        assert_eq!(YYTRANSLATE[0], 0);
        let mut m = Mem::new();
        store_messages(&mut m);
        assert_eq!(m.cstr(0x086d_319e), b"ok\n");
        assert_eq!(m.cstr(0x086d_30b6), b"exit ok\n");
    }
}
