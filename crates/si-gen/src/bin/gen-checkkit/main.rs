// SPDX-License-Identifier: MIT
//! `gen-checkkit <directory of vector files>`: `gen-emucheck` on `si-checkkit` (finding 0011,
//! "A check binary with si-checkkit"; the vectors and the comparison are those of finding 0033).
//! The old driver stays beside this one until the totals have agreed for a while.

use si_checkkit::Kit;
use si_gen::list as l;
use si_gen::tree as t;

fn main() {
    Kit::new(&[
        ("SI_genListCreate", |m, _o, _a| l::create(m).into()),
        ("SI_genListElemCreate", |m, _o, _a| l::elem_create(m).into()),
        ("SI_genListDestroy", |m, o, a| l::destroy(m, o, a.v("plist"), a.v("destroy")).into()),
        ("SI_genListIterInit", |m, _o, a| l::iter_init(m, a.v("list"), a.v("first"), a.v("last")).into()),
        ("SI_genListIterNext", |m, _o, a| l::iter_next(m, a.v("list"), a.v("first"), a.v("last")).into()),
        ("SI_genListIterPrev", |m, _o, a| l::iter_prev(m, a.v("list"), a.v("first"), a.v("last")).into()),
        ("SI_genListIterSet", |m, _o, a| l::iter_set(m, a.v("list"), a.v("e"), a.v("first"), a.v("last")).into()),
        ("SI_genListInsertB", |m, _o, a| l::insert_b(m, a.v("list"), a.v("data")).into()),
        ("SI_genListInsertE", |m, _o, a| l::insert_e(m, a.v("list"), a.v("data")).into()),
        ("SI_genListDump", |m, o, a| l::dump(m, o, a.v("list"), a.v("dump"), a.v("file")).into()),
        ("SI_genListElemDestroy", |m, o, a| l::elem_destroy(m, o, a.v("pe"), a.v("destroy")).into()),
        ("SI_genListElemDump", |m, o, a| l::elem_dump(m, o, a.v("e"), a.v("dump"), a.v("file")).into()),
        ("SI_genListElemGetData", |m, _o, a| l::elem_get_data(m, a.v("e")).into()),
        ("SI_genListGetHead", |m, _o, a| l::get_head(m, a.v("list")).into()),
        ("SI_genListElemSetData", |m, _o, a| l::elem_set_data(m, a.v("e"), a.v("data")).into()),
        ("SI_genListInsertElemAfter", |m, _o, a| l::insert_elem_after(m, a.v("after"), a.v("e")).into()),
        ("SI_genListElemReplaceData", |m, o, a| l::elem_replace_data(m, o, a.v("e"), a.v("destroy"), a.v("data")).into()),
        ("SI_genListRemove", |m, o, a| l::remove(m, o, a.v("list"), a.v("e"), a.v("destroy")).into()),
        ("SI_genListSetFlags", |m, _o, a| l::set_flags(m, a.v("list"), a.v("flags")).into()),
        ("SI_genListGetFlags", |m, _o, a| l::get_flags(m, a.v("list")).into()),
        ("SI_genListCopy", |m, o, a| l::copy(m, o, a.v("list"), a.v("copy"), a.v("cmp")).into()),
        ("SI_genListInsertS", |m, o, a| l::insert_s(m, o, a.v("list"), a.v("data"), a.v("cmp")).into()),
        ("SI_genListFindNth", |m, _o, a| l::find_nth(m, a.v("list"), a.i("n")).into()),
        ("SI_genListElemOf", |m, o, a| l::elem_of(m, o, a.v("list"), a.v("data"), a.v("cmp")).into()),
        ("SI_genTreeCreate", |m, _o, _a| t::create(m).into()),
        ("SI_genTreeElemCreate", |m, _o, _a| t::elem_create(m).into()),
        ("SI_genTreeGetRoot", |m, _o, a| t::get_root(m, a.v("tree")).into()),
        ("SI_genTreeElemGetData", |m, _o, a| t::elem_get_data(m, a.v("e")).into()),
        ("SI_genTreeElemSetData", |m, _o, a| t::elem_set_data(m, a.v("e"), a.v("data")).into()),
        ("SI_genTreeSetFlags", |m, _o, a| t::set_flags(m, a.v("tree"), a.v("flags")).into()),
        ("SI_genTreeGetFlags", |m, _o, a| t::get_flags(m, a.v("tree")).into()),
        ("SI_genTreeElemReplaceData", |m, o, a| t::elem_replace_data(m, o, a.v("e"), a.v("destroy"), a.v("data")).into()),
        ("SI_genTreeElemDestroy", |m, o, a| t::elem_destroy(m, o, a.v("pe"), a.v("destroy")).into()),
        ("SI_genTreeRemove", |m, o, a| t::remove(m, o, a.v("tree"), a.v("pe"), a.v("destroy")).into()),
        ("SI_genTreeRemoveSubTree", |m, o, a| t::remove_sub_tree(m, o, a.v("tree"), a.v("pe"), a.v("destroy")).into()),
        ("SI_genTreeElemDestroySubTree", |m, o, a| { t::elem_destroy_sub_tree(m, o, a.v("pe"), a.v("destroy")); ().into() }),
        ("SI_genTreeDestroy", |m, o, a| { t::destroy(m, o, a.v("ptree"), a.v("destroy")); ().into() }),
        ("SI_genTreeInsertS", |m, o, a| t::insert_s(m, o, a.v("tree"), a.v("data"), a.v("cmp")).into()),
        ("SI_genTreeElemInsert", |m, o, a| t::elem_insert(m, o, a.v("tree"), a.v("e"), a.v("data"), a.v("cmp")).into()),
    ])
    .run()
}
