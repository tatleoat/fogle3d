// SPDX-License-Identifier: MIT
//! `parse-checkkit <directory of vector files>`: the clause code of `exp_parsing.c`
//! (`si_expr::image::parsing`) on `si-checkkit` against the emulated original (finding 0013).
//! Vectors: `re/tools/si-expr/gen_parse.py`.
use si_checkkit::Kit;
use si_expr::image::parsing::*;

fn main() {
    // the parser's tables are read from the user's install (docs/design/runtime-data.md)
    si_rt::check_or_exit("parse-checkkit");
    Kit::new(&[
        ("findSumatrafcvName", |m, _o, a| find_sumatra_fcv_name(m, a.i("a"), a.i("b")).into()),
        ("getAClause", |m, o, a| get_a_clause(m, o, a.v("q"), a.v("leaf"), a.i("first") as i16).into()),
        ("searchForClause", |m, o, a| search_for_clause(m, o, a.v("node"), a.v("q"), a.i("first") as i16).into()),
        ("SI_checkAllocStr", |m, _o, a| check_alloc_str(m, a.v("dest"), a.v("s"), a.i("sep") as i16).into()),
        ("GeneratePrefixForSumatra", |m, _o, a| generate_prefix_for_sumatra(m, a.v("arg"), a.v("elem")).into()),
        ("si_exprExpandGenRef", |m, o, a| { expr_expand_gen_ref(m, o, a.v("expr"), a.v("ident")); ().into() }),
        ("SI_queryOnAnElement", |m, o, a| query_on_an_element(m, o, a.v("ident"), a.v("query")).into()),
        ("SI_getClauseFromQuery", |m, o, a| { get_clause_from_query(m, o, a.v("q")); ().into() }),
    ])
    .setup(|m| {
        si_mem::init_globals(m, si_dict::globals::ALL);
        si_expr::image::rec::init_globals(m);
        // the literals of the printer in the original's data
        m.put_cstr(0x08699f5d, b" ");
        m.put_cstr(0x08699f7d, b".");
        m.put_cstr(0x08699f7f, b".");
    })
    .run()
}
