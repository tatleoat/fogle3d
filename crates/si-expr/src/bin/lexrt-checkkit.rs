// SPDX-License-Identifier: MIT
//! `lexrt-checkkit <directory of vector files>`: the flex run-time of `exp_lex.c`
//! (`si_expr::image::lexrt`) on `si-checkkit` against the emulated original (finding 0013).
//! Vectors: `re/tools/si-expr/gen_lexrt.py`.
use si_checkkit::Kit;
use si_expr::image::lexrt::*;

fn main() {
    // the parser's tables are read from the user's install (docs/design/runtime-data.md)
    si_rt::check_or_exit("lexrt-checkkit");
    Kit::new(&[
        ("EXP_yy_load_buffer_state", |m, _o, _a| { exp_yy_load_buffer_state(m); ().into() }),
        ("EXP_yy_flush_buffer", |m, _o, a| { exp_yy_flush_buffer(m, a.v("b")); ().into() }),
        ("EXP_yy_init_buffer", |m, _o, a| { exp_yy_init_buffer(m, a.v("b"), a.v("file")); ().into() }),
        ("EXP_yy_create_buffer", |m, o, a| exp_yy_create_buffer(m, o, a.v("file"), a.i("size")).into()),
        ("EXP_yy_delete_buffer", |m, _o, a| { exp_yy_delete_buffer(m, a.v("b")); ().into() }),
        ("EXP_yyrestart", |m, o, a| { exp_yyrestart(m, o, a.v("file")); ().into() }),
        ("EXP_yy_switch_to_buffer", |m, _o, a| { exp_yy_switch_to_buffer(m, a.v("b")); ().into() }),
        ("EXP_yy_scan_buffer", |m, o, a| exp_yy_scan_buffer(m, o, a.v("base"), a.v("size")).into()),
        ("EXP_yy_scan_bytes", |m, o, a| exp_yy_scan_bytes(m, o, a.v("bytes"), a.i("len")).into()),
        ("EXP_yy_scan_string", |m, o, a| exp_yy_scan_string(m, o, a.v("s")).into()),
        ("0x08257330", |m, _o, _a| yy_get_previous_state(m).into()),
        ("0x0825738c", |m, _o, a| yy_try_nul_trans(m, a.v("state")).into()),
        ("0x082573d8", |m, o, a| { yyunput(m, o, a.i("c"), a.v("text")); ().into() }),
        ("0x08257140", |m, o, _a| yy_get_next_buffer(m, o).into()),
        ("0x08257488", |m, o, _a| yyinput(m, o).into()),
    ])
    .setup(store_messages)
    .run()
}
