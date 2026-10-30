// SPDX-License-Identifier: MIT
//! `mi2yacc-checkkit <directory of vector files>`: the command parser of `mi2control_yacc.c` (`si_mental::mi2control_yacc`) and the reply
//! routines of `mi2control_lex.c` on `si-checkkit` against the emulated original (finding 0042, round 11). Vectors: `re/tools/si-mental/gen_mi2yacc.py`.
use si_checkkit::Kit;
use si_mental::mi2control_lex::{mi2_yyerror, output_error, output_ok};
use si_mental::mi2control_yacc::*;

/// Where the vectors keep the token script and the answer list, and the scanner's value.
const TOKEN_SCRIPT: u32 = 0x0881_a710;
const ANSWERS: u32 = 0x0881_a714;
const COMMANDS: [u32; 13] = [
    0x0842_dca8, 0x0842_db20, 0x0842_dee8, 0x0842_df70, 0x0842_e080, 0x0842_e0c0, 0x0842_e03c, 0x0842_dfc0, 0x0842_e12c, 0x0842_e174, 0x0842_dcb4,
    0x0842_deb4, 0x0842_dad0,
];

fn main() {
    // the parser's tables and texts are read from the user's install (docs/design/runtime-data.md)
    si_rt::check_or_exit("mi2yacc-checkkit");
    Kit::new(&[
        ("mi2_yyparse", |m, o, _a| mi2_yyparse(m, o).into()),
        ("mi2_yyerror", |m, o, a| { mi2_yyerror(m, o, a.v("msg")); ().into() }),
        ("output_ok", |m, o, _a| { output_ok(m, o); ().into() }),
        ("output_error", |m, o, a| { output_error(m, o, a.i("code")); ().into() }),
    ])
    .setup(|m| {
        si_mental::mi2control_lex::store_messages(m);
        store_messages(m);
    })
    .normalise_stack_args()
    .effect(|m, r, _a, _answer| {
        if r.addr == 0x0842_e9e0 {
            // the scanner stand-in: the next (token, value) pair of the script; the end (token 0) stays
            let p = m.u32(TOKEN_SCRIPT);
            let (tok, lv) = (m.u32(p), m.u32(p + 4));
            if tok != 0 {
                m.set_u32(TOKEN_SCRIPT, p.wrapping_add(8));
            }
            m.set_u32(MI2_YYLVAL, lv);
        } else if COMMANDS.contains(&r.addr) {
            let p = m.u32(ANSWERS);
            m.set_u32(ANSWERS, p.wrapping_add(4));
        }
        Ok(())
    })
    .run()
}
