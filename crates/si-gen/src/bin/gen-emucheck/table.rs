// SPDX-License-Identifier: MIT
//! From a vector file's routine name to the port: one line per function.

use crate::{Args, Recorded, Ret};
use si_gen::list as l;
use si_mem::Mem;

pub fn run(name: &str, m: &mut Mem, o: &mut Recorded, a: &Args) -> Option<Ret> {
    Some(match name {
        "SI_genListCreate" => l::create(m).into(),
        "SI_genListElemCreate" => l::elem_create(m).into(),
        "SI_genListDestroy" => l::destroy(m, o, a.v("plist"), a.v("destroy")).into(),
        "SI_genListIterInit" => l::iter_init(m, a.v("list"), a.v("first"), a.v("last")).into(),
        "SI_genListIterNext" => l::iter_next(m, a.v("list"), a.v("first"), a.v("last")).into(),
        "SI_genListIterPrev" => l::iter_prev(m, a.v("list"), a.v("first"), a.v("last")).into(),
        "SI_genListIterSet" => l::iter_set(m, a.v("list"), a.v("e"), a.v("first"), a.v("last")).into(),
        "SI_genListInsertB" => l::insert_b(m, a.v("list"), a.v("data")).into(),
        "SI_genListInsertE" => l::insert_e(m, a.v("list"), a.v("data")).into(),
        "SI_genListDump" => l::dump(m, o, a.v("list"), a.v("dump"), a.v("file")).into(),
        "SI_genListElemDestroy" => l::elem_destroy(m, o, a.v("pe"), a.v("destroy")).into(),
        "SI_genListElemDump" => l::elem_dump(m, o, a.v("e"), a.v("dump"), a.v("file")).into(),
        "SI_genListElemGetData" => l::elem_get_data(m, a.v("e")).into(),
        "SI_genListGetHead" => l::get_head(m, a.v("list")).into(),
        "SI_genListElemSetData" => l::elem_set_data(m, a.v("e"), a.v("data")).into(),
        "SI_genListInsertElemAfter" => l::insert_elem_after(m, a.v("after"), a.v("e")).into(),
        "SI_genListElemReplaceData" => l::elem_replace_data(m, o, a.v("e"), a.v("destroy"), a.v("data")).into(),
        "SI_genListRemove" => l::remove(m, o, a.v("list"), a.v("e"), a.v("destroy")).into(),
        "SI_genListSetFlags" => l::set_flags(m, a.v("list"), a.v("flags")).into(),
        "SI_genListGetFlags" => l::get_flags(m, a.v("list")).into(),
        "SI_genListCopy" => l::copy(m, o, a.v("list"), a.v("copy"), a.v("cmp")).into(),
        "SI_genListInsertS" => l::insert_s(m, o, a.v("list"), a.v("data"), a.v("cmp")).into(),
        "SI_genListFindNth" => l::find_nth(m, a.v("list"), a.i("n")).into(),
        "SI_genListElemOf" => l::elem_of(m, o, a.v("list"), a.v("data"), a.v("cmp")).into(),
        _ => return None,
    })
}
