// SPDX-License-Identifier: MIT
//! The arms of `nurbs-trimcheck` for `si_nurbs::geom3` (vectors of `re/tools/si-nurbs/gen_geom3.py`).

use super::{Answer, Loaded};
use si_nurbs::geom3 as g;

pub const NAMES: &[&str] = &["nurbsSrfAveragePointDelta", "SI_shiftDirKnotsClosed", "SI_knotsSrfReparametrization", "si_nurbsSrfInverseForConversion", "SI_nurbsSrfInverse", "SI_nurbsSrfSwapUV", "si_nurbsClampFactor", "SI_nurbsCrvKnotInsert", "SI_nurbsSrfKnotInsertU", "SI_nurbsSrfKnotInsertV", "SI_nurbsSrfKnotInsert", "nurbsFrenetCoord", "SI_nurbsCrvIndexHit"];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, i: &dyn Fn(&str) -> i32, f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "nurbsSrfAveragePointDelta" => {
            let (s, o) = (p(l, "sr"), p(l, "outp"));
            g::average_point_delta(&mut l.m, s, o, i("dir") as i16);
            Answer::None
        }
        "SI_shiftDirKnotsClosed" => {
            let d = p(l, "dirp");
            g::shift_dir_knots_closed(&mut l.m, d, i("sh"));
            Answer::None
        }
        "SI_knotsSrfReparametrization" => {
            let s = p(l, "sr");
            g::knots_srf_reparametrization(&mut l.m, &mut l.o, s);
            Answer::None
        }
        "si_nurbsSrfInverseForConversion" => {
            let s = p(l, "sr");
            Answer::Eax(g::si_nurbs_srf_inverse_for_conversion(&mut l.m, &mut l.o, s, i("u") as i16, i("v") as i16) as u32)
        }
        "SI_nurbsSrfInverse" => {
            let s = p(l, "sr");
            Answer::Eax(g::si_nurbs_srf_inverse(&mut l.m, &mut l.o, s, i("u") as i16, i("v") as i16) as u32)
        }
        "si_nurbsClampFactor" => {
            let (f, d) = (p(l, "fvr"), p(l, "dirr"));
            si_nurbs::bridge::si_nurbs_clamp_factor(&mut l.m, f, d);
            Answer::None
        }
        "SI_nurbsSrfSwapUV" => {
            let s = p(l, "sr");
            Answer::Eax(si_nurbs::bridge::srf_swap_uv(&mut l.m, s) as u32)
        }
        "SI_nurbsCrvKnotInsert" => {
            let c = p(l, "cur");
            Answer::Eax(si_nurbs::bridge::crv_knot_insert(&mut l.m, c, f("t"), i("r"), i("co") as i16) as u32)
        }
        "SI_nurbsSrfKnotInsertU" | "SI_nurbsSrfKnotInsertV" => {
            let s = p(l, "sr");
            let r = si_nurbs::bridge::srf_knot_insert_dir(&mut l.m, &mut l.o, s, f("t"), i("r"), name.ends_with('V'));
            // the 16-bit answer: the upper half of eax is not the routine's
            Answer::Eax(r as u16 as u32)
        }
        "SI_nurbsSrfKnotInsert" => {
            let s = p(l, "sr");
            Answer::Eax(si_nurbs::bridge::srf_knot_insert(&mut l.m, &mut l.o, s, f("t"), i("r"), i("d")) as u32)
        }
        "nurbsFrenetCoord" => {
            let (c, fs, o) = (p(l, "cr"), p(l, "slotp"), p(l, "outp"));
            g::nurbs_frenet_coord(&mut l.m, c, f("t"), fs, o, &mut |_, _, _| panic!("SI_nurbsUpdateFrenet reached"));
            Answer::None
        }
        "SI_nurbsCrvIndexHit" => {
            let (c, ib, tb) = (p(l, "cur"), p(l, "ibp"), p(l, "tbp"));
            Answer::Eax(g::nurbs_crv_index_hit(&mut l.m, c, ib, tb, i("fl") as i16) as u32)
        }
        _ => unreachable!("{name}"),
    }
}
