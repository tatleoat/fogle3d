// SPDX-License-Identifier: MIT
//! `expdial-checkkit <directory of vector files>`: the non-dialog helpers of `express_dial.c`
//! (`si_expr::image::express_dial`) on `si-checkkit` against the emulated original (finding 0013).
//! Vectors: `re/tools/misc1/gen_expdial.py`.
use si_checkkit::Kit;
use si_expr::image::express_dial::*;

fn main() {
    // the parser's tables are read from the user's install (docs/design/runtime-data.md)
    si_rt::check_or_exit("expdial-checkkit");
    Kit::new(&[
        ("lhsIsCustom", |m, o, a| lhs_is_custom(m, o, a.v("expr"), a.v("custom"), a.v("fcv")).into()),
        ("checkStartName", |m, _o, a| check_start_name(m, a.v("from"), a.v("limit")).into()),
    ])
    .setup(|m| si_mem::init_globals(m, si_dict::globals::ALL))
    // the stand-in of the generator: stores model ^ 0xdeadbeef through its third argument, answers model * 7 + 1
    .stand_in("SI_cusFetchCustomValue", |m, args| {
        m.set_u32(args[2], args[0] ^ 0xDEAD_BEEF);
        si_mem::Ret::eax(args[0].wrapping_mul(7).wrapping_add(1))
    })
    .run()
}
