// SPDX-License-Identifier: MIT
//! `nurbs-trim2check <directory of vector files>`: intersections and rays of `si_nurbs::inter` (intersec.c, ray.c;
//! finding 0044, section trim1) against the emulated original, on `si-checkkit`. Vectors from
//! `re/tools/si-nurbs/gen_trim2.py`. `EMUCHECK_MUTATE=1|calls` for the two mutation switches.

use si_checkkit::{Args, Kit};
use si_nurbs::inter as i;
use si_nurbs::trimstat as ts;

/// A double passed by value as two stack words `v_<name>lo`, `v_<name>hi`.
fn d(a: &Args, name: &str) -> f64 {
    f64::from_bits(a.v(&format!("{name}lo")) as u64 | (a.v(&format!("{name}hi")) as u64) << 32)
}

/// Three doubles passed by value (six stack words) `v_<name>0lo` ...
fn p3(a: &Args, name: &str) -> [u32; 6] {
    let mut o = [0u32; 6];
    for k in 0..3 {
        o[2 * k] = a.v(&format!("{name}{k}lo"));
        o[2 * k + 1] = a.v(&format!("{name}{k}hi"));
    }
    o
}

fn main() {
    Kit::new(&[
        ("si_nurbsInterCreate", |m, _o, _a| i::inter_create(m).into()),
        ("si_nurbsInterCreateP", |m, _o, a| i::inter_create_p(m, d(a, "u"), d(a, "v"), p3(a, "p"), p3(a, "d"), d(a, "t"), a.v("ray")).into()),
        ("si_nurbsInterDestroy", |m, _o, a| i::inter_destroy(m, a.v("pp")).into()),
        ("si_nurbsInterComputeRayT", |m, _o, a| {
            i::inter_compute_ray_t(m, a.v("inter"));
            ().into()
        }),
        ("si_nurbsInterFindClosest", |m, _o, a| i::inter_find_closest(m, a.v("elem"), a.v("ray1"), a.v("ray2"), a.v("pflag")).into()),
        ("si_nurbsInterLink", |m, _o, a| i::inter_link(m, a.v("e1"), a.v("e2"), a.i("dir") as i16, d(a, "d4"), d(a, "d5")).into()),
        ("si_nurbsInterUnconn", |m, _o, a| i::inter_unconn(m, a.v("e"), a.i("dir") as i16).into()),
        ("si_nurbsInterUpdateNextCon", |m, _o, a| i::inter_update_next_con(m, a.v("e"), a.i("flag") as i16).into()),
        ("si_nurbsInterCrossBndry", |m, _o, a| i::inter_cross_bndry(m, a.v("e")).into()),
        ("si_nurbsInterClassifyBndry", |m, _o, a| i::inter_classify_bndry(m, a.v("ray"), a.v("trim")).into()),
        ("si_nurbsInterCopyFlags", |m, o, a| i::inter_copy_flags(m, o, a.v("r1"), a.v("r2")).into()),
        ("si_nurbsInterGetInCurve", |m, _o, a| i::inter_get_in_curve(m, a.v("i")).into()),
        ("si_nurbsInterSetInCurve", |m, _o, a| i::inter_set_in_curve(m, a.v("i"), a.i("n") as i16).into()),
        ("si_nurbsInterCompT", |m, _o, a| i::inter_comp_t(m, a.v("a"), a.v("b")).into()),
        ("si_nurbsInterCopy", |m, _o, a| i::inter_copy(m, a.v("a")).into()),
        ("si_nurbsInterAlreadyThere", |m, _o, a| i::inter_already_there(m, a.v("list"), a.v("inter"), a.v("srf")).into()),
        ("si_nurbsInterBuildLinearCrv", |m, _o, a| i::inter_build_linear_crv(m, a.v("inter"), a.i("n"), a.i("closed") as i16).into()),
        ("si_nurbsRayCreate", |m, _o, _a| i::ray_create(m).into()),
        ("si_nurbsRayCreateP", |m, _o, a| i::ray_create_p(m, p3(a, "o"), p3(a, "d"), d(a, "t"), p3(a, "e")).into()),
        ("si_nurbsRayCopy", |m, o, a| i::ray_copy(m, o, a.v("r")).into()),
        ("si_nurbsRayGetInters", |m, _o, a| i::ray_get_inters(m, a.v("r")).into()),
        ("si_nurbsRayGetNoInter", |m, _o, a| i::ray_get_no_inter(m, a.v("r")).into()),
        ("si_nurbsRayDestroy", |m, o, a| i::ray_destroy(m, o, a.v("pp")).into()),
        ("si_nurbsRayAddInter", |m, o, a| i::ray_add_inter(m, o, a.v("ray"), a.v("inter"), a.v("srf")).into()),
        ("si_nurbsRayInitRayCast", |m, _o, a| i::ray_init_ray_cast(m, p3(a, "a"), p3(a, "d"), p3(a, "c"), a.i("type"), a.v("o54"), a.v("o58"), a.v("o5c")).into()),
        ("si_writeStack", |m, _o, a| {
            ts::si_write_stack(m, a.v("stack"), a.v("ptr"), [d(a, "a"), d(a, "b"), d(a, "c"), d(a, "d")], [a.i("s1") as i16, a.i("s2") as i16, a.i("s3") as i16]);
            ().into()
        }),
        ("si_readStack", |m, _o, a| {
            ts::si_read_stack(m, a.v("stack"), [a.v("o1"), a.v("o2"), a.v("o3"), a.v("o4")], [a.v("p1"), a.v("p2"), a.v("p3")]);
            ().into()
        }),
        ("si_popStack", |m, _o, a| ts::si_pop_stack(m, a.v("stack")).into()),
        ("SI_initTrimStatus", |m, _o, _a| ts::si_init_trim_status(m).into()),
        ("si_updateTrimStatus", |m, _o, a| ts::si_update_trim_status(m, a.v("st")).into()),
        ("si_trimStatAllocate", |m, _o, _a| ts::si_trim_stat_allocate(m).into()),
        ("si_trimStatDispose", |m, _o, a| ts::si_trim_stat_dispose(m, a.v("st")).into()),
        ("si_trimLineAllocate", |m, _o, a| ts::si_trim_line_allocate(m, a.v("st")).into()),
        ("si_trmDataRem", |m, _o, a| ts::si_trm_data_rem(m, a.v("st"), a.i("idx"), a.v("node")).into()),
        ("si_countNumLines", |m, _o, a| ts::si_count_num_lines(m, a.v("st")).into()),
        ("getCrvIndex", |m, _o, a| (ts::get_crv_index(m, a.i("n"), a.v("a"), a.i("t")) as u32).into()),
        ("updateStepAndOrder", |m, _o, a| ts::update_step_and_order(m, a.v("comp"), a.v("crv")).into()),
        ("ptsOnBorder", |_m, _o, a| (ts::pts_on_border(d(a, "a"), d(a, "b"), d(a, "c"), d(a, "d"), d(a, "e"), d(a, "f")) as u32).into()),
        ("acrossBorder", |m, _o, a| ts::across_border(m, a.v("srf"), a.v("p"), a.v("q")).into()),
        ("linkSepCrv", |m, _o, a| ts::link_sep_crv(m, a.v("srf"), a.v("pts"), a.v("idx"), a.i("n"), a.i("k"), a.v("os"), a.v("oc"), a.v("ocl")).into()),
        ("nurbsRayValidNewRay", |m, _o, a| i::ray_valid_new_ray(m, a.v("ray"), a.v("a"), a.v("b")).into()),
    ])
    .route(i::route)
    .run()
}
