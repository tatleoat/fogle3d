// SPDX-License-Identifier: MIT
//! `shdlist-checkkit <directory of vector files>`: the Textlist routines of `shd_fct.c`
//! (`shd_list.rs`) on `si-checkkit` against the emulated original (vectors from
//! `re/tools/si-render/shdlist/gen_shdlist.py`, finding 0023, section 39).

use si_checkkit::Kit;
use si_render::shd_list::*;

fn main() {
    Kit::new(&[
        ("shdlistLineSprintf", |m, _o, a| { shdlist_line_sprintf(m, a.v("udf"), a.v("out")); ().into() }),
        ("SI_shdlistFill", |m, _o, a| si_shdlist_fill(m, a.v("udfs"), a.v("count") as i32).into()),
        ("SI_shdlistAdd", |m, _o, a| (si_shdlist_add(m, a.v("list"), a.v("udf")) as u32).into()),
        ("SI_shdlistDelete", |m, _o, a| (si_shdlist_delete(m, a.v("list"), a.v("index") as i32) as u32).into()),
        ("SI_shdlistMoveup", |m, _o, a| { si_shdlist_moveup(m, a.v("list"), a.v("index") as i32); ().into() }),
        ("SI_shdlistModify", |m, _o, a| { si_shdlist_modify(m, a.v("list"), a.v("index") as i32, a.v("udf")); ().into() }),
    ])
    .setup(store_strings)
    .run()
}
