// SPDX-License-Identifier: MIT
//! The arms of `nurbs-trimcheck` for the nurbsgeom8 routines (vectors of `re/tools/si-nurbs/gen_geom8.py`).

use super::{Answer, Loaded};
use si_nurbs::geom8 as g;

pub const NAMES: &[&str] = &["nurbsCrvAddKnotNonUniform", "openNormal", "fitPlaneToSamples"];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, i: &dyn Fn(&str) -> i32, f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "nurbsCrvAddKnotNonUniform" => {
            let c = p(l, "crr");
            if let Err(e) = g::crv_add_knot_non_uniform(&mut l.m, c, i("idx"), f("t")) {
                eprintln!("nurbsCrvAddKnotNonUniform: {e:?}");
            }
            Answer::None
        }
        "openNormal" => {
            let (d, sl) = (p(l, "crr"), p(l, "slp"));
            g::open_normal(&mut l.m, d, sl, i("per"));
            Answer::None
        }
        "fitPlaneToSamples" => {
            let pt = p(l, "ptsp");
            let o = [p(l, "o0p"), p(l, "o1p"), p(l, "o2p"), p(l, "o3p"), p(l, "o4p"), p(l, "o5p"), p(l, "o6p"), p(l, "o7p")];
            Answer::Eax(g::fit_plane_to_samples(&mut l.m, pt, i("n"), o))
        }
        _ => unreachable!("{name}"),
    }
}
