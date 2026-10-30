// SPDX-License-Identifier: MIT
//! The arms of `nurbs-trimcheck` for the nurbsgeom7 routines (vectors of `re/tools/si-nurbs/gen_geom7.py`).

use super::{Answer, Loaded};
use si_nurbs::geom7 as g;

pub const NAMES: &[&str] = &["checkValidOpenLinear", "nurbsSrfAveragePointsDistance", "SI_nurbsDistanceToValueFrenet"];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, i: &dyn Fn(&str) -> i32, _f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "checkValidOpenLinear" => {
            let d = p(l, "drr");
            g::check_valid_open_linear(&mut l.m, d);
            Answer::None
        }
        "nurbsSrfAveragePointsDistance" => {
            let (sr, fl) = (p(l, "sr"), i("fl"));
            Answer::Eax(g::srf_average_points_distance(&mut l.m, sr, fl as u16).unwrap_or(0))
        }
        "SI_nurbsDistanceToValueFrenet" => {
            let (c, d, pt, fs, ax) = (p(l, "cur"), i("d"), p(l, "ptp"), p(l, "slotp"), p(l, "axp"));
            // SI_nurbsUpdateFrenet (0x085020a8) is not ported; the vectors give the frames, so reaching it is a refusal
            let r = g::nurbs_distance_to_value_frenet(&mut l.m, &mut l.o, c, f32::from_bits(d as u32), pt, fs, ax, &mut |_, _, _| panic!("SI_nurbsUpdateFrenet reached"));
            if let Err(e) = r { eprintln!("dtvf: {e:?}"); }
            Answer::None
        }
        _ => unreachable!("{name}"),
    }
}
