// SPDX-License-Identifier: MIT
//! `sort-checkkit <vector file or directory>...`: the sorting helpers of `sort.c` (0x0851ca00-0x0851d020,
//! `si_nurbs::sort`) against the emulated original, on `si-checkkit`. Vectors from
//! `re/tools/tri1/gen_sort.py`. `EMUCHECK_MUTATE=1|calls` for the two mutation switches.

use si_checkkit::Kit;
use si_nurbs::sort as s;

fn main() {
    Kit::new(&[
        ("swapRecord", |m, _o, a| s::swap_record(m, a.i("a"), a.i("b")).into()),
        ("SI_swapGet", |m, _o, a| s::swap_get(m, a.v("pa"), a.v("pb")).into()),
        ("SI_swapDispose", |m, _o, _a| s::swap_dispose(m).into()),
        ("iswap", |m, _o, a| s::iswap(m, a.v("a"), a.v("b"), a.v("pa"), a.v("pb")).into()),
        ("ifindpivot", |m, _o, a| s::ifindpivot(m, a.v("arr"), a.i("lo"), a.i("hi")).into()),
        ("ipartition", |m, _o, a| s::ipartition(m, a.v("arr"), a.v("parr"), a.i("lo"), a.i("hi"), a.i("pivot"), a.s("note")).into()),
        ("SI_quicksort", |m, _o, a| s::quicksort(m, a.v("arr"), a.v("parr"), a.i("lo"), a.i("hi"), a.s("note")).into()),
        ("fswap", |m, _o, a| s::fswap(m, a.v("a"), a.v("b")).into()),
        ("ffindpivot", |m, _o, a| s::ffindpivot(m, a.v("arr"), a.i("lo"), a.i("hi")).into()),
        ("fpartition", |m, _o, a| s::fpartition(m, a.v("arr"), a.i("lo"), a.i("hi"), a.f("pivot"), a.s("note")).into()),
        ("SI_fquicksort", |m, _o, a| s::fquicksort(m, a.v("arr"), a.i("lo"), a.i("hi"), a.s("note")).into()),
        ("sswap", |m, _o, a| s::sswap(m, a.v("a"), a.v("b")).into()),
        ("sfindpivot", |m, _o, a| s::sfindpivot(m, a.v("arr"), a.i("lo"), a.i("hi")).into()),
        ("spartition", |m, _o, a| s::spartition(m, a.v("arr"), a.i("lo"), a.i("hi"), a.v("pivot"), a.s("note")).into()),
        ("SI_squicksort", |m, _o, a| s::squicksort(m, a.v("arr"), a.i("lo"), a.i("hi"), a.s("note")).into()),
    ])
    .run()
}
