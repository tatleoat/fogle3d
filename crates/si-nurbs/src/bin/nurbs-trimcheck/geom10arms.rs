// SPDX-License-Identifier: MIT
//! The arms of `nurbs-trimcheck` for the nurbsgeom10 routines (vectors of `re/tools/si-nurbs/gen_geom10.py`).

use super::{Answer, Loaded};
use si_nurbs::geom10 as g;

pub const NAMES: &[&str] = &["partitionLinearData"];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, i: &dyn Fn(&str) -> i32, f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "partitionLinearData" => {
            let (c, od, oc, op, ol, on, os, oq) =
                (p(l, "crvp"), p(l, "odp"), p(l, "ocp"), p(l, "opp"), p(l, "olp"), p(l, "onp"), p(l, "osp"), p(l, "oqp"));
            Answer::Eax(g::partition_linear_data(&mut l.m, c, f("cs"), i("order"), od, oc, op, ol, on, os, oq))
        }
        _ => unreachable!("{name}"),
    }
}
