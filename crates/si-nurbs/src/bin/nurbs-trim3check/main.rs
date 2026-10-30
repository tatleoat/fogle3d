// SPDX-License-Identifier: MIT
//! `nurbs-trim3check <directory of vector files>`: trim.c of `si_nurbs::trim` (finding 0044, section trim3) against the
//! emulated original, on `si-checkkit`. Vectors from `re/tools/si-nurbs/gen_trim3.py`. `EMUCHECK_MUTATE=1|calls` for the
//! two mutation switches.

use si_checkkit::{Args, Kit};
use si_nurbs::trim as t;

/// Three doubles passed by value (six stack words) `v_<name>0lo` ... as the order the port takes them.
fn p3(a: &Args, name: &str) -> [u32; 6] {
    let mut o = [0u32; 6];
    for k in 0..3 {
        o[2 * k] = a.v(&format!("{name}{k}lo"));
        o[2 * k + 1] = a.v(&format!("{name}{k}hi"));
    }
    o
}

/// A double passed by value as two stack words `v_<name>lo`, `v_<name>hi`.
fn d(a: &Args, name: &str) -> f64 {
    f64::from_bits(a.v(&format!("{name}lo")) as u64 | (a.v(&format!("{name}hi")) as u64) << 32)
}

fn main() {
    Kit::new(&[
        ("si_nurbsTrimSetDirection", |m, _o, a| {
            t::set_direction(m, a.v("t"), p3(a, "d"));
            ().into()
        }),
        ("si_nurbsTrimSetCenter", |m, _o, a| {
            t::set_center(m, a.v("t"), p3(a, "d"));
            ().into()
        }),
        ("si_nurbsTrimSetCoordDirection", |m, _o, a| {
            t::set_coord_direction(m, a.v("t"), a.v("ty"), p3(a, "d"));
            ().into()
        }),
        ("si_nurbsTrimSetCoordCenter", |m, _o, a| {
            t::set_coord_center(m, a.v("t"), a.v("ty"), p3(a, "d"));
            ().into()
        }),
        ("si_nurbsTrimSetSurfaceOp", |m, _o, a| {
            t::set_surface_op(m, a.v("t"), a.v("v"));
            ().into()
        }),
        ("si_nurbsTrimSetProjection", |m, _o, a| {
            t::set_projection(m, a.v("t"), a.v("v"));
            ().into()
        }),
        ("si_nurbsTrimSetTrimming", |m, _o, a| {
            t::set_trimming(m, a.v("t"), a.v("v"));
            ().into()
        }),
        ("si_nurbsTrimSetRayList", |m, _o, a| t::set_ray_list(m, a.v("t"), a.v("v")).into()),
        ("si_nurbsTrimSetOrientList", |m, _o, a| t::set_orient_list(m, a.v("t"), a.v("v")).into()),
        ("si_nurbsTrimGetRayList", |m, _o, a| t::get_ray_list(m, a.v("t")).into()),
        ("si_nurbsTrimGetOrientList", |m, _o, a| t::get_orient_list(m, a.v("t")).into()),
        ("si_nurbsTrimShowPreview", |m, _o, a| {
            t::show_preview(m, a.v("t"), a.v("show") as u16);
            ().into()
        }),
        ("si_nurbsTrimSetStep", |m, o, a| {
            t::set_step(m, o, a.v("t"), a.v("step"));
            ().into()
        }),
        ("SI_nurbsTrimCreateP", |m, o, a| {
            let c = t::CreateArgs {
                trimming: a.v("op"),
                projection: a.v("proj"),
                coord: a.v("coord"),
                center: p3(a, "ctr"),
                dir: p3(a, "dir"),
                mat_a: a.v("ma"),
                mat_b: a.v("mb"),
                step: a.v("step"),
                surface_op: a.v("sop"),
            };
            t::create_p(m, o, a.v("srf"), a.v("crv"), &c).into()
        }),
        ("SI_nurbsTrimDestroy", |m, o, a| {
            t::destroy(m, o, a.v("pp"));
            ().into()
        }),
        ("si_nurbsTrimStartAtZero", |m, _o, a| t::start_at_zero(m, a.v("list")).into()),
        ("si_nurbsTrimClearInterLinks", |m, _o, a| t::clear_inter_links(m, a.v("list")).into()),
        ("si_nurbsTrimComputeLineInter", |m, _o, a| {
            t::compute_line_inter(m, p3(a, "p0"), p3(a, "p1"), p3(a, "p2"), p3(a, "p3"), a.v("opt"), a.v("os"), a.v("ot")).into()
        }),
        ("initInterSeg", |m, _o, a| {
            t::init_inter_seg(m, a.v("p1"), a.v("p2"), a.v("p3"), a.v("p4"), a.v("inter"), a.v("srf"));
            ().into()
        }),
        ("si_nurbsTrimOrientOrientList", |m, o, a| t::orient_orient_list(m, o, a.v("t")).into()),
        ("si_nurbsTrimConnectRayInter", |m, o, a| t::connect_ray_inter(m, o, a.v("t")).into()),
        ("si_nurbsTrimRayCast", |m, o, a| {
            let c = t::RayCastArgs { origin: p3(a, "d"), dir: p3(a, "o"), t: d(a, "t_"), end: p3(a, "e"), status: a.v("status"), perturb: a.v("perturb") as u16 };
            t::ray_cast(m, o, a.v("t"), &c).into()
        }),
        ("si_nurbsTrimBuildOrientList", |m, o, a| t::build_orient_list(m, o, a.v("rays")).into()),
        ("si_nurbsTrimBuildCompositeCurves", |m, o, a| t::build_composite_curves(m, o, a.v("t")).into()),
        ("si_nurbsTrimBuildCompositeCurvesProj", |m, o, a| t::build_composite_curves_proj(m, o, a.v("t")).into()),
        ("si_nurbsTrimBuildIntersectionLists", |m, o, a| {
            t::build_intersection_lists(m, o, a.v("t"));
            ().into()
        }),
        ("si_nurbsTrimProject", |m, o, a| t::project(m, o, a.v("t")).into()),
        ("SI_nurbsTrimTrim", |m, o, a| t::trim(m, o, a.v("t")).into()),
        ("si_nurbsTrimMoveInterToBorder", |m, _o, a| t::move_inter_to_border(m, a.v("out"), a.v("t"), a.v("inter")).into()),
        ("si_nurbsTrimPreview", |m, o, a| t::preview(m, o, a.v("t")).into()),
        ("parallelRayPerturb", |m, o, a| t::parallel_ray_perturb(m, o, a.v("ray"), a.v("t"), d(a, "tv"), a.v("status")).into()),
    ])
    .route(si_nurbs::inter::route)
    .run()
}
