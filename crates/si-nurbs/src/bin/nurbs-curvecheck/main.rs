// SPDX-License-Identifier: MIT
//! `nurbs-curvecheck <directory of vector files>`: `si_nurbs::curveinter` / `curveorient` / `trim2` (curveInter.c, curveOrient.c, trimUtils.c
//! and ray.c leftovers; finding 0044, section Trim2) against the emulated original, on `si-checkkit`. Vectors from
//! `re/tools/si-nurbs/gen_curveinter.py`. `EMUCHECK_MUTATE=1|calls` for the two mutation switches.

use si_checkkit::{Args, Kit};
use si_nurbs::curveinter as c;
use si_nurbs::curveinter2 as c2;
use si_nurbs::curveorient as co;
use si_nurbs::curveorient2 as co2;
use si_nurbs::inter as i;
use si_nurbs::trimutils2 as tu2;

/// A double passed by value as two stack words `v_<name>lo`, `v_<name>hi`.
fn d(a: &Args, name: &str) -> f64 {
    f64::from_bits(a.v(&format!("{name}lo")) as u64 | (a.v(&format!("{name}hi")) as u64) << 32)
}

/// `n` doubles by value as words `<name>0lo` ...
fn words(a: &Args, name: &str, n: usize) -> Vec<u32> {
    let mut o = vec![0u32; 2 * n];
    for k in 0..n {
        o[2 * k] = a.v(&format!("{name}{k}lo"));
        o[2 * k + 1] = a.v(&format!("{name}{k}hi"));
    }
    o
}

fn p3w(a: &Args, name: &str) -> [u32; 6] {
    words(a, name, 3).try_into().unwrap()
}

fn dbl(a: &Args, name: &str, n: usize) -> Vec<f64> {
    (0..n).map(|k| d(a, &format!("{name}{k}"))).collect()
}

fn b6(a: &Args, name: &str) -> [f64; 6] {
    dbl(a, name, 6).try_into().unwrap()
}

fn p3(a: &Args, name: &str) -> [f64; 3] {
    dbl(a, name, 3).try_into().unwrap()
}

/// The stand-in for `si_nurbsRayTraceSrf2` (same as `trace_body` of gen_trim8.py): the ray gets the list and count the slot buffer holds; 1.
fn trace(m: &mut si_mem::Mem, args: &[u32]) -> si_mem::Ret {
    let (ray, slot) = (args[1], args[2]);
    m.set_u32(ray + 0x54, m.u32(slot));
    m.set_u32(ray + 0x50, m.u32(slot + 4));
    si_mem::Ret::eax(1)
}

/// The stand-in for `BldLinFromComp` where its answer is disposed (same as `bld_body` of gen_trim8.py): a closed-when-odd curve of 4 points, order 3.
fn bld_stand(m: &mut si_mem::Mem, args: &[u32]) -> si_mem::Ret {
    let n = args[1];
    let rec = m.si_malloc(0x64);
    for k in 0..0x19 {
        m.set_u32(rec + 4 * k, 0);
    }
    let kn = m.si_malloc(0x20);
    for (k, v) in [0.0f64, 0.25, 0.5, 1.0].iter().enumerate() {
        m.set_f64(kn + 8 * k as u32, *v);
    }
    let pts = m.si_malloc(0x24 * 4);
    for k in 0..36 {
        m.set_u32(pts + 4 * k, 0);
    }
    for k in 0..4u32 {
        m.set_f64(pts + 0x24 * k, 0.25 * k as f64 + 0.1 * n as f64);
        m.set_f64(pts + 0x24 * k + 8, 0.5 * k as f64);
    }
    m.set_u32(rec, 3);
    m.set_u32(rec + 4, 4);
    m.set_u32(rec + 0xc, kn);
    m.set_u16(rec + 0x20, (n & 1) as u16);
    m.set_u32(rec + 0x2c, pts);
    m.set_u32(args[2], 3);
    si_mem::Ret::eax(rec)
}

fn main() {
    Kit::new(&[
        ("si_nurbsCurveInterCreate", |m, _o, _a| c::curve_inter_create(m).into()),
        ("si_nurbsCurveInterDestroy", |m, o, a| c::curve_inter_destroy(m, o, a.v("pp")).into()),
        ("si_nurbsSegmentInterCreate", |m, _o, a| c::segment_inter_create(m, p3w(a, "a"), p3w(a, "b"), d(a, "c"), d(a, "d"), a.v("w1"), a.v("w2")).into()),
        ("si_nurbsSegmentInterDestroy", |m, _o, a| c::segment_inter_destroy(m, a.v("pp")).into()),
        ("si_nurbsUvMonSegCreate", |m, _o, a| c::uv_mon_seg_create(m, a.v("e1"), a.v("e2")).into()),
        ("si_nurbsUvMonSegDestroy", |m, _o, a| c::uv_mon_seg_destroy(m, a.v("pp")).into()),
        ("si_nurbsCurveInterUpdateBbox", |m, _o, a| {
            let bw: [u32; 12] = words(a, "b", 6).try_into().unwrap();
            c::curve_inter_update_bbox(m, a.v("res"), bw, d(a, "px"), d(a, "py")).into()
        }),
        ("si_nurbsCurveInterBboxInter", |_m, _o, a| c::curve_inter_bbox_inter(b6(a, "a"), b6(a, "b")).into()),
        ("si_nurbsCurveInterSelfSegInter", |m, o, a| c::curve_inter_self_seg_inter(m, o, a.v("e1"), a.v("e2")).into()),
        ("si_nurbsCurveInterCheckSelfInt", |m, o, a| c::curve_inter_check_self_int(m, o, a.v("ci")).into()),
        ("validBbox", |m, _o, a| c::valid_bbox(m, p3(a, "a"), p3(a, "b"), a.v("comp"), a.v("bx")).into()),
        ("buildLinearBorder", |m, _o, a| c::build_linear_border(m, a.v("srf")).into()),
        ("buildUvMonList", |m, _o, a| {
            let bw: [u32; 12] = (0..12).map(|k| a.v(&format!("bw{k}"))).collect::<Vec<_>>().try_into().unwrap();
            c::build_uv_mon_list(m, a.v("out"), a.v("list"), bw, a.v("flag") as i16).into()
        }),
        ("buildSegList", |m, _o, a| {
            let bx = si_nurbs::curveinter3::build_seg_list(m, a.v("list"), a.v("crv"), a.v("ci"));
            for (k, w) in bx.iter().enumerate() {
                m.set_u32(a.v("sret") + 4 * k as u32, *w);
            }
            a.v("sret").into()
        }),
        ("crossSegments", |m, _o, a| {
            let v = c2::cross_segments(m, a.v("s1"), a.v("s2"));
            for (k, x) in v.iter().enumerate() {
                m.set_f64(a.v("out") + 8 * k as u32, *x);
            }
            a.v("out").into()
        }),
        ("setIntersectionDirection", |m, _o, a| {
            c2::set_intersection_direction(m, a.v("e1"), a.v("e2"), a.v("e3"), a.v("e4"), a.v("orient") as i32, a.v("op") as i32).into()
        }),
        ("splitSegment", |m, o, a| {
            c2::split_segment(m, o, a.v("pe"), p3w(a, "pt"), d(a, "t"), a.v("list"), a.v("uv"), a.v("pe1"), a.v("pe2"));
            ().into()
        }),
        ("si_nurbsCurveInterSegInter", |m, o, a| {
            c2::seg_inter(m, o, a.v("pe1"), a.v("pe2"), a.v("ci1"), a.v("ci2"), a.v("uv1"), a.v("uv2"), a.v("op") as i32).into()
        }),
        ("si_nurbsCurveInterIntersectLists", |m, o, a| {
            c2::intersect_lists(m, o, a.v("l1"), a.v("l2"), a.v("op") as i32);
            ().into()
        }),
        ("initCurveInterObj", |m, o, a| c2::init_curve_inter_obj(m, o, a.v("crv"), a.v("inside") as i16).into()),
        ("si_nurbsCurveInterBuildNewInterList", |m, o, a| {
            c2::build_new_inter_list(m, o, a.v("curves"), a.v("a"), a.v("b"), a.v("pdst"), a.v("pdst2"), a.v("bbox"));
            ().into()
        }),
        ("si_nurbsCurveInterCheckFitSelfInter", |m, o, a| c2::check_fit_self_inter(m, o, a.v("crv"), a.v("bbox")).into()),
        ("computeIntersectionOrientation", |m, _o, a| {
            si_nurbs::curveinter3::compute_intersection_orientation(m, a.v("e1"), a.v("e2"), a.v("ci1"), a.v("ci2"), a.v("p5") as i16, a.v("p6") as i16, a.v("p7") as i16, a.v("p8") as i16, a.v("op") as i32).into()
        }),
        ("si_nurbsCurveOrientCreateP", |m, _o, a| co::curve_orient_create_p(m, a.v("a"), a.v("b"), a.v("c") as i16, a.v("d")).into()),
        ("si_nurbsCurveOrientDestroy", |m, _o, a| {
            co::curve_orient_destroy(m, a.v("pp"));
            ().into()
        }),
        ("inDomainSamp", |m, _o, a| co::in_domain_samp(m, a.v("trim"), d(a, "v")).into()),
        ("findPointInCurve", |m, _o, a| co::find_point_in_curve(m, a.v("co"), a.v("trim")).into()),
        ("reverseCurve", |m, _o, a| co::reverse_curve(m, a.v("co")).into()),
        ("si_nurbsCurveOrientProjPtPara", |m, _o, a| co::curve_orient_proj_pt_para(m, p3(a, "p"), p3(a, "d"), p3(a, "q"), a.v("out")).into()),
        ("projectPtPers", |m, _o, a| co::project_pt_pers(m, p3(a, "a"), p3(a, "b"), p3(a, "c"), p3(a, "d"), a.v("out")).into()),
        ("projectCrvSamples", |m, _o, a| co::project_crv_samples(m, a.v("trim"), a.v("co"), a.v("inter"), a.v("pout"), a.v("pidx")).into()),
        ("si_nurbsOrientTraverseNestingTree", |m, _o, a| {
            co::orient_traverse_nesting_tree(m, a.v("e"), a.v("flag") as i16);
            ().into()
        }),
        ("si_nurbsCurveOrientPointInCurve", |m, o, a| (co2::point_in_curve(m, o, a.v("pts"), a.v("n") as i32, p3(a, "p"), p3(a, "q"), p3(a, "d")) as u32).into()),
        ("findNeighborInPlane", |m, o, a| (co2::find_neighbor_in_plane(m, o, p3(a, "dir"), p3(a, "p"), a.v("pts"), a.v("n") as i32, a.v("idx") as i32, a.v("op") as i32, a.v("out")) as u32).into()),
        ("findTestPoints", |m, o, a| (co2::find_test_points(m, o, a.v("trim"), a.v("inter"), p3(a, "p"), a.v("o1"), a.v("o2"), a.v("o3")) as u32).into()),
        ("findLocForOrientTest", |m, o, a| (co2::find_loc_for_orient_test(m, o, a.v("trim"), a.v("co"), a.v("a"), a.v("b"), a.v("c")) as u32).into()),
        ("checkBoundaryCross", |m, o, a| {
            (co2::check_boundary_cross_regs(m, o, a.v("p1"), d(a, "a"), a.v("p3"), d(a, "b"), d(a, "c"), d(a, "d"), a.v("flag") as i16, a.v("trim"), 0, 0) as u32).into()
        }),
        ("setCorrection", |m, o, a| {
            co2::set_correction(m, o, a.v("p1"), d(a, "a"), a.v("p3"), d(a, "b"), d(a, "c"), d(a, "d"), a.v("flag") as i16, a.v("trim"), a.v("out"));
            ().into()
        }),
        ("findUvCriticalInters", |m, o, a| (co2::find_uv_critical_inters(m, o, a.v("co"), a.v("trim"), a.v("c0"), a.v("c1"), a.v("c2"), a.v("c3")) as u32).into()),
        ("findNonLinearInter", |m, _o, a| co2::find_non_linear_inter(m, a.v("i0"), a.v("i1"), a.v("i2"), a.v("i3")).into()),
        ("si_nurbsCurveOrientOrient", |m, o, a| co2::orient(m, o, a.v("co"), a.v("trim")).into()),
        ("si_nurbsCurveOrientMarkBCross", |m, o, a| co2::mark_b_cross(m, o, a.v("co"), a.v("trim")).into()),
        ("si_nurbsCurveInterBldLinFromComp", |m, o, a| c::bld_lin_from_comp(m, o, a.v("curves"), a.v("n") as i32, a.v("pmax")).into()),
        ("si_nurbsCurveInterBuildOldInterList", |m, o, a| {
            c2::build_old_inter_list(m, o, a.v("srf"), a.v("pdst"), a.v("pdst2"));
            ().into()
        }),
        ("SI_nurbsSrfGetTrimCrv", |m, o, a| tu2::srf_get_trim_crv(m, o, a.v("srf"), a.v("pn")).into()),
        ("SI_nurbsSrfGetProjCrv", |m, o, a| tu2::srf_get_proj_crv(m, o, a.v("srf"), a.v("pn")).into()),
        ("si_nurbsCurveInterPointOnLine", |_m, _o, a| c::curve_inter_point_on_line(p3(a, "a"), p3(a, "b"), p3(a, "c")).into()),
    ])
    .stand_in("si_nurbsRayTraceSrf2", trace)
    .stand_in("si_nurbsCurveInterBldLinFromComp", bld_stand)
    .route(|m, r, args| i::route(m, r, args).or_else(|| c::route(m, r, args)).or_else(|| c2::route2(m, r, args)))
    .run()
}
