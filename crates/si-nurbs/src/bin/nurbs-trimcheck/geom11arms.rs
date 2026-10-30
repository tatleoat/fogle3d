// SPDX-License-Identifier: MIT
//! The arms of `nurbs-trimcheck` for the nurbsgeom11 routines (vectors of `re/tools/si-nurbs/gen_geom11.py`).

use super::{Answer, Loaded};
use si_nurbs::geom11 as g;

pub const NAMES: &[&str] = &["fitNurbsPoints", "SI_fitNurbsCurve", "SI_nurbsUpdateFrenet"];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, i: &dyn Fn(&str) -> i32, f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "fitNurbsPoints" => {
            let (d, pt, pr) = (p(l, "dirp"), p(l, "ptsp"), p(l, "parp"));
            let r = g::fit_nurbs_points(&mut l.m, &mut l.o, d, f("tol"), i("n"), pt, pr);
            Answer::Eax(r)
        }
        "SI_fitNurbsCurve" => {
            let c = p(l, "crvp");
            Answer::Eax(g::si_fit_nurbs_curve(&mut l.m, &mut l.o, c, i("n"), i("a2") as u32, i("a3") as u32, f("ang")))
        }
        "SI_nurbsUpdateFrenet" => {
            let (c, s) = (p(l, "cur"), p(l, "slotp"));
            if let Err(e) = g::nurbs_update_frenet(&mut l.m, &mut l.o, c, s) {
                eprintln!("updateFrenet: {e:?}");
            }
            Answer::None
        }
        _ => unreachable!("{name}"),
    }
}
