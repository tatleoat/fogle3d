// SPDX-License-Identifier: MIT
//! The arms of `nurbs15check` (nurbs15): the image forms of the earlier changer and the knot insertion.

use super::{Answer, Loaded};
use si_nurbs::ins2 as x;

pub const NAMES: &[&str] = &["si_OLDnurbsSrfChangeKnotsSeq", "SI_nurbsSrfInsertKnot", "si_nurbsInterDist2D", "si_nurbsRayDist3D"];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, i: &dyn Fn(&str) -> i32, f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "si_OLDnurbsSrfChangeKnotsSeq" => {
            let (c, k) = (p(l, "sfr"), p(l, "nkp"));
            let r = x::old_srf_change_knots_seq_img(&mut l.m, c, i("n"), k);
            if std::env::var("NURBS_TRACE").is_ok() { eprintln!("old changer -> {r:?}"); }
            Answer::Eax(r.unwrap_or(0) as u32)
        }
        "SI_nurbsSrfInsertKnot" => {
            let c = p(l, "sr");
            let r = x::srf_insert_knot_img(&mut l.m, c, f("t"), i("d"));
            if std::env::var("NURBS_TRACE").is_ok() { eprintln!("insert knot d={} -> {r:?}", i("d")); }
            Answer::Eax(r.unwrap_or(0) as u32)
        }
        "si_nurbsInterDist2D" => Answer::St0(si_nurbs::dist::inter_dist_2d(f("a"), f("b"), f("c"), f("d"))),
        "si_nurbsRayDist3D" => Answer::St0(si_nurbs::dist::ray_dist_3d([f("a"), f("b"), f("c")], [f("d"), f("e"), f("f")])),
        _ => unreachable!("{name}"),
    }
}
