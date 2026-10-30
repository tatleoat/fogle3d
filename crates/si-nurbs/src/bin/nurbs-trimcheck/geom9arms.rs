// SPDX-License-Identifier: MIT
//! The arms of `nurbs-trimcheck` for the nurbsgeom9 routines (vectors of `re/tools/si-nurbs/gen_geom9.py`).

use super::{Answer, Loaded};
use si_nurbs::geom9 as g;

pub const NAMES: &[&str] = &["updateNurbsEvaluationMatrix"];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, _i: &dyn Fn(&str) -> i32, _f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "updateNurbsEvaluationMatrix" => {
            let (d, mt, sg, pr) = (p(l, "dirp"), p(l, "matp"), p(l, "segp"), p(l, "parp"));
            Answer::Eax(g::update_nurbs_evaluation_matrix(&mut l.m, &mut l.o, d, mt, sg, pr) as u32)
        }
        _ => unreachable!("{name}"),
    }
}
