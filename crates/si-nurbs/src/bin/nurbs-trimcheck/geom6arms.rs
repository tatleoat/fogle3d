// SPDX-License-Identifier: MIT
//! The arms of `nurbs-trimcheck` for the nurbsgeom6 routines (vectors of `re/tools/si-nurbs/gen_geom6.py`).

use super::{Answer, Loaded};
use si_nurbs::{conv6 as v, elev as e};

pub const NAMES: &[&str] = &["nurbsDirElevateOrder", "SI_nurbsCrvElevateDegree", "SI_nurbsSrfElevateDegree",
    "getBezierPoint", "initNurbsKnotsFromBezier", "nurbsCrvConvertToLinearBezier", "SI_nurbsCrvConvertToBezier",
    "SI_spline2NurbsCrvKeepCP", "SI_nurbsCrv2SplineKeepCP", "initNurbsPointsVKnotsFromBspline",
    "SI_nurbsSrfConvertToLinear", "SI_spline2NurbsCrvKeepCurve"];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, i: &dyn Fn(&str) -> i32, _f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "nurbsDirElevateOrder" => {
            let (s, d) = (p(l, "srr"), p(l, "dsr"));
            e::dir_elevate_order(&mut l.m, s, d);
            Answer::None
        }
        "SI_nurbsCrvElevateDegree" => {
            let c = p(l, "cur");
            // a refusal (the original would not end its span search) answers 0: such cases are not compared
            Answer::Eax(e::crv_elevate_degree(&mut l.m, c).unwrap_or(0))
        }
        "SI_nurbsSrfElevateDegree" => {
            let s = p(l, "sr");
            Answer::Eax(e::srf_elevate_degree(&mut l.m, &mut l.o, s).unwrap_or(0))
        }
        "getBezierPoint" => {
            let (pa, a, b) = (p(l, "par"), i("a"), i("b"));
            Answer::Eax(v::get_bezier_point(&l.m, pa, a, b) as u32)
        }
        "initNurbsKnotsFromBezier" => {
            let d = p(l, "drr");
            v::init_nurbs_knots_from_bezier(&mut l.m, d);
            Answer::None
        }
        "nurbsCrvConvertToLinearBezier" => {
            let c = p(l, "cur");
            Answer::Eax(v::crv_convert_to_linear_bezier(&mut l.m, &mut l.o, c))
        }
        "SI_nurbsCrvConvertToBezier" => {
            let c = p(l, "cur");
            Answer::Eax(v::crv_convert_to_bezier(&mut l.m, &mut l.o, c))
        }
        "SI_spline2NurbsCrvKeepCP" => {
            let sp = p(l, "spr");
            Answer::Eax(v::spline_to_crv_keep_cp(&mut l.m, sp).unwrap_or(0))
        }
        "SI_nurbsCrv2SplineKeepCP" => {
            let (c, k) = (p(l, "cur"), i("kd"));
            Answer::Eax(v::crv_to_spline_keep_cp(&mut l.m, &mut l.o, c, k))
        }
        "initNurbsPointsVKnotsFromBspline" => {
            let sr = p(l, "sr");
            // a refusal (the original would not end its span search) is not compared
            let _ = v::init_points_vknots_from_bspline(&mut l.m, &mut l.o, sr);
            Answer::None
        }
        "SI_nurbsSrfConvertToLinear" => {
            let sr = p(l, "sr");
            Answer::Eax(v::srf_convert_to_linear(&mut l.m, &mut l.o, sr).unwrap_or(0))
        }
        "SI_spline2NurbsCrvKeepCurve" => {
            let sp = p(l, "spr");
            Answer::Eax(v::spline_to_crv_keep_curve(&mut l.m, &mut l.o, sp).unwrap_or(0))
        }
        _ => unreachable!("{name}"),
    }
}
