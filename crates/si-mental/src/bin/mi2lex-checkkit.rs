// SPDX-License-Identifier: MIT
//! `mi2lex-checkkit <directory of vector files>`: the flex run-time of `mi2control_lex.c` (`si_mental::mi2control_lex`) on `si-checkkit`
//! against the emulated original (finding 0042, round 11). Vectors: `re/tools/si-mental/gen_mi2lex.py`.
use si_checkkit::Kit;
use si_mental::mi2control_lex::*;

fn main() {
    Kit::new(&[
        ("mi2_yy_load_buffer_state", |m, _o, _a| { mi2_yy_load_buffer_state(m); ().into() }),
        ("mi2_yy_flush_buffer", |m, _o, a| { mi2_yy_flush_buffer(m, a.v("b")); ().into() }),
        ("mi2_yy_init_buffer", |m, _o, a| { mi2_yy_init_buffer(m, a.v("b"), a.v("file")); ().into() }),
        ("mi2_yy_create_buffer", |m, o, a| mi2_yy_create_buffer(m, o, a.v("file"), a.i("size")).into()),
        ("mi2_yy_delete_buffer", |m, _o, a| { mi2_yy_delete_buffer(m, a.v("b")); ().into() }),
        ("mi2_yyrestart", |m, o, a| { mi2_yyrestart(m, o, a.v("file")); ().into() }),
        ("mi2_yy_switch_to_buffer", |m, _o, a| { mi2_yy_switch_to_buffer(m, a.v("b")); ().into() }),
        ("mi2_yy_scan_buffer", |m, o, a| mi2_yy_scan_buffer(m, o, a.v("base"), a.v("size")).into()),
        ("mi2_yy_scan_bytes", |m, o, a| mi2_yy_scan_bytes(m, o, a.v("bytes"), a.i("len")).into()),
        ("mi2_yy_scan_string", |m, o, a| mi2_yy_scan_string(m, o, a.v("s")).into()),
        ("0x0842f14c", |m, _o, _a| yy_get_previous_state(m).into()),
        ("0x0842f1b0", |m, _o, a| yy_try_nul_trans(m, a.v("state")).into()),
        ("0x0842f1fc", |m, o, a| { yyunput(m, o, a.i("c"), a.v("text")); ().into() }),
        ("0x0842ef50", |m, o, _a| yy_get_next_buffer(m, o).into()),
        ("0x0842f2a8", |m, o, _a| yyinput(m, o).into()),
        ("mi2_yylex", |m, o, _a| mi2_yylex(m, o).into()),
    ])
    .setup(store_messages)
    .effect(|m, r, a, answer| {
        if r.addr == 0x080c_50e8 && answer == 1 {
            // read(fd, buf, 1): the next byte of the case's source (the pointer at si_stdOutput's place), as the generator's stand-in hands it out
            let p = m.u32(SRC_PTR);
            let b = m.u8(p);
            m.set_u8(a[1], b);
            m.set_u32(SRC_PTR, p.wrapping_add(1));
        }
        Ok(())
    })
    .run()
}
