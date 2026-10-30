// SPDX-License-Identifier: MIT
//! The arms of `nurbs-trimcheck` for `si_nurbs::geom5` (vectors of `re/tools/si-nurbs/gen_geom5.py`).

use super::{Answer, Loaded};
use si_nurbs::geom5 as g;

pub const NAMES: &[&str] = &[
    "updateStartEnd",
    "dirtyCurves",
    "markDirtyCcrvs",
    "nurbsCrvTransferOriginal",
    "nurbsSrfTransferOriginal",
    "SI_nurbsCrvApplyFunc2Points",
    "SI_nurbsSrfApplyFunc2Points",
];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, i: &dyn Fn(&str) -> i32, _f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "updateStartEnd" => {
            let s = p(l, "sp");
            // a refusal (the original traps) is answered like no change: such cases are not compared
            let _ = g::update_start_end(&mut l.m, s, i("cl"), i("row"), i("cnt"), i("lo"), i("hi"), i("nk"));
            Answer::None
        }
        "dirtyCurves" => {
            let (s, d, d2) = (p(l, "srf"), p(l, "dir"), p(l, "dir2"));
            let _ = g::dirty_curves(&mut l.m, s, d, d2, i("n"), i("first"), i("lo0"), i("stride"), i("a"), i("b"), i("base"));
            Answer::None
        }
        "markDirtyCcrvs" => {
            let s = p(l, "sr");
            let _ = g::mark_dirty_ccrvs(&mut l.m, s, i("i"));
            Answer::None
        }
        "nurbsCrvTransferOriginal" => {
            let (d, s) = (p(l, "dsr"), p(l, "srr"));
            g::nurbs_crv_transfer_original(&mut l.m, d, s);
            Answer::None
        }
        "nurbsSrfTransferOriginal" => {
            let (d, s) = (p(l, "dr"), p(l, "sr"));
            g::nurbs_srf_transfer_original(&mut l.m, &mut l.o, d, s);
            Answer::None
        }
        "SI_nurbsCrvApplyFunc2Points" => {
            let (s, d) = (p(l, "srr"), p(l, "dsr"));
            let f = si_mem::Routine::new("nurbsDump", i("cb") as u32);
            g::crv_apply_func2_points(&mut l.m, &mut l.o, s, d, f, i("mask") as u32);
            Answer::None
        }
        "SI_nurbsSrfApplyFunc2Points" => {
            let (s, d) = (p(l, "sr"), p(l, "dr"));
            let f = si_mem::Routine::new("nurbsDump", i("cb") as u32);
            g::srf_apply_func2_points(&mut l.m, &mut l.o, s, d, f, i("mask") as u32);
            Answer::None
        }
        _ => unreachable!("{name}"),
    }
}
