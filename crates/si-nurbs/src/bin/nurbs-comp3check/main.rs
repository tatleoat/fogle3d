// SPDX-License-Identifier: MIT
//! `nurbs-comp3check <directory of vector files>`: the tree-based composite routines of `si_nurbs::composite3`
//! (finding 0044, unit trim7) against the emulated original, on `si-checkkit`. Vectors from `re/tools/trim7/gen_comp3.py`.
//! `EMUCHECK_MUTATE=1|calls` for the two mutation switches.

use si_checkkit::{Args, Kit};
use si_nurbs::composite3 as c3;
use si_nurbs::trimprev as tp;

/// A box passed by value: twelve words `v_b0` ... `v_b11`.
fn bx(a: &Args) -> [u32; 12] {
    let mut b = [0u32; 12];
    for (i, w) in b.iter_mut().enumerate() {
        *w = a.v(&format!("b{i}"));
    }
    b
}

/// The port refused (`Halt`): the original reads memory nobody wrote; answer with a word of the fill pattern so that the
/// case counts as "not compared" (see `nurbs-rendercheck`).
fn unwritten(m: &si_mem::Mem) -> u32 {
    match m.fill() {
        si_mem::Fill::Pattern { second, .. } => 0x5a5a_0000 | second as u32,
        _ => 0,
    }
}

fn main() {
    Kit::new(&[
        ("pruneContainHier", |m, o, a| { c3::prune_contain_hier(m, &mut c3::Own(o), a.v("elem"), a.v("op")); ().into() }),
        ("buildContainHier", |m, o, a| c3::build_contain_hier(m, &mut c3::Own(o), a.v("la"), a.v("lb")).into()),
        ("addCompFromTree", |m, o, a| {
            c3::add_comp_from_tree(m, &mut c3::Own(o), a.v("elem"), a.v("comps"), a.v("pcount"), bx(a), a.v("old"), a.v("op"));
            ().into()
        }),
        ("si_nurbsCompCurveCreateSrfComps", |m, o, a| {
            c3::create_srf_comps(m, &mut c3::Own(o), a.v("la"), a.v("lb"), a.v("pcomps"), a.v("op"), bx(a), a.v("old"), a.v("trim"))
                .unwrap_or_else(|_| unwritten(m))
                .into()
        }),
        ("si_nurbsCompCurveCreateSrfCompsProj", |m, o, a| c3::create_srf_comps_proj(m, &mut c3::Own(o), a.v("pout"), a.v("old"), a.v("item")).into()),
        ("si_nurbsCompCurveInCurveExt", |m, o, a| c3::comp_curve_in_curve_ext(m, &mut c3::Own(o), a.v("a"), a.v("b")).into()),
        ("si_nurbsInterBuildLinearCrvCB", |m, _o, a| tp::inter_build_linear_crv_cb(m, a.v("p1"), a.v("p2"), a.i("n"), a.v("trim")).into()),
        ("si_nurbsTrimBuildListFromPreview", |m, o, a| tp::build_list_from_preview(m, &mut c3::Own(o), a.v("trim")).unwrap_or_else(|_| unwritten(m)).into()),
        ("rebuildComp", |m, o, a| { c3::rebuild_comp(m, &mut c3::Own(o), a.v("srf"), a.v("tree")); ().into() }),
        ("SI_nurbsCompositeCurveRebuild", |m, o, a| c3::composite_curve_rebuild(m, &mut c3::Own(o), a.v("srf"), a.v("crvs")).into()),
        ("SI_nurbsNoBorderTrimDelete", |m, o, a| c3::no_border_trim_delete(m, &mut c3::Own(o), a.v("srf"), a.v("crvs"), a.v("idx") as u16).into()),
    ])
    .normalise_stack_args()
    .run()
}
