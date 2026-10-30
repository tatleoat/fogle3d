// SPDX-License-Identifier: MIT
//! The arms of `nurbs-trimcheck` for `si_nurbs::blend` (vectors of `re/tools/si-nurbs/gen_blend.py`).

use super::{Answer, Loaded};
use si_nurbs::blend as b;
use si_nurbs::blend2 as b2;

/// The routine names (label heads) this file runs.
pub const NAMES: &[&str] = &[
    "reverseKnotSeq", "extendEndKnotFromDeltas", "extendBeginKnotFromDeltas", "buildDeltaVector", "checkMult", "mergeKnotSequence",
    "0x084ce274", "0x084ce328", "0x084ce2c8", "insertIntoKnotList", "si_nurbsCrvFillSegmentPoints", "SI_nurbsFactorToDistance",
    "nurbsCrvExtendEndKnot", "nurbsCrvExtendBeginKnot", "nurbsSrfExtendEndKnot", "nurbsSrfExtendBeginKnot", "si_nurbsCrvExtendKnot",
    "si_nurbsSrfExtendKnot", "SI_nurbsCrvMerge", "SI_nurbsCrvBlend", "SI_nurbsCrvMergeLinear", "SI_nurbsCrvMergeJoin",
    "SI_nurbsSrfMerge", "SI_nurbsSrfBlend", "SI_nurbsSrfMergeLinear", "SI_nurbsSrfMergeJoin",
    "insertMissingKnots", "insertMissingKnotsSrf", "nurbsCrvMakeCompatibleKnots", "SI_nurbsCrvMakeCompatible", "SI_nurbsSrfMakeCompatible",
];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, i: &dyn Fn(&str) -> i32, f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "reverseKnotSeq" => {
            let k = p(l, "knp");
            Answer::Eax(b::reverse_knot_seq(&mut l.m, k, i("n")))
        }
        "extendEndKnotFromDeltas" => {
            let (k, d, c) = (p(l, "knp"), p(l, "dlp"), p(l, "cntp"));
            b::extend_end_knot_from_deltas(&mut l.m, k, i("n"), d, c);
            Answer::None
        }
        "extendBeginKnotFromDeltas" => {
            let (k, d, c) = (p(l, "knp"), p(l, "dlp"), p(l, "cntp"));
            b::extend_begin_knot_from_deltas(&mut l.m, k, i("n"), d, c);
            Answer::None
        }
        "buildDeltaVector" => Answer::Eax(b::build_delta_vector(&mut l.m, i("n"), f("a"), f("b"))),
        "checkMult" => {
            let k = p(l, "knp");
            Answer::Eax(b::check_mult(&l.m, k, i("i"), i("n")) as u32)
        }
        "mergeKnotSequence" => {
            let (a, bb, o) = (p(l, "dap"), p(l, "dbp"), p(l, "dop"));
            b::merge_knot_sequence(&mut l.m, a, bb, o, i("order"));
            Answer::None
        }
        "0x084ce274" => {
            let lp = p(l, "lhp");
            Answer::Eax(b::mesh_add_list(&mut l.m, lp, i("kp") as u32) as u32)
        }
        "0x084ce328" => {
            let lp = p(l, "lhp");
            Answer::Eax(b::mesh_cnt_list(&l.m, lp) as u32)
        }
        "0x084ce2c8" => {
            let lp = p(l, "lhp");
            Answer::Eax(b::mesh_free_list(&mut l.m, lp) as u32)
        }
        "insertIntoKnotList" => {
            let (lp, vp, st) = (p(l, "lhp"), p(l, "vp"), p(l, "start"));
            Answer::Eax(b::insert_into_knot_list(&mut l.m, lp, vp, i("count"), st))
        }
        "si_nurbsCrvFillSegmentPoints" => {
            let (d, k, o) = (p(l, "dirp"), p(l, "knp"), p(l, "outp"));
            b::crv_fill_segment_points(&mut l.m, d, k, o);
            Answer::None
        }
        "SI_nurbsFactorToDistance" => {
            let c = p(l, "crvp");
            match b::nurbs_factor_to_distance(&mut l.m, &mut l.o, c, f("f")) {
                Ok(v) => Answer::St0(v),
                Err(_) => Answer::None,
            }
        }
        "insertMissingKnots" => {
            let (k, c) = (p(l, "wkp"), p(l, "crvp"));
            b::insert_missing_knots(&mut l.m, &mut l.o, k, i("n"), c);
            Answer::None
        }
        "insertMissingKnotsSrf" => {
            let (k, s) = (p(l, "wkp"), p(l, "srfp"));
            b::insert_missing_knots_srf(&mut l.m, &mut l.o, k, i("n"), s, i("dir"));
            Answer::None
        }
        "nurbsCrvMakeCompatibleKnots" => {
            let (a, bb, c) = (p(l, "akp"), p(l, "bkp"), p(l, "cntp"));
            Answer::Eax(b::nurbs_crv_make_compatible_knots(&mut l.m, a, i("na"), bb, i("nb"), c))
        }
        "SI_nurbsCrvMakeCompatible" => {
            let a = p(l, "arrp");
            b::nurbs_crv_make_compatible(&mut l.m, &mut l.o, a, i("n"));
            Answer::None
        }
        "SI_nurbsSrfMakeCompatible" => {
            let a = p(l, "arrp");
            b::nurbs_srf_make_compatible(&mut l.m, &mut l.o, a, i("n"), i("dir"));
            Answer::None
        }
        "nurbsCrvExtendEndKnot" | "nurbsCrvExtendBeginKnot" | "nurbsSrfExtendEndKnot" | "nurbsSrfExtendBeginKnot" => {
            let (r, d) = (p(l, "rr"), p(l, "dlp"));
            let n = i("cnt");
            match name {
                "nurbsCrvExtendEndKnot" => b2::nurbs_crv_extend_end_knot(&mut l.m, r, d, n),
                "nurbsCrvExtendBeginKnot" => b2::nurbs_crv_extend_begin_knot(&mut l.m, r, d, n),
                "nurbsSrfExtendEndKnot" => b2::nurbs_srf_extend_end_knot(&mut l.m, r, d, n),
                _ => b2::nurbs_srf_extend_begin_knot(&mut l.m, r, d, n),
            }
            Answer::None
        }
        "si_nurbsCrvExtendKnot" | "si_nurbsSrfExtendKnot" => {
            let (a, bb) = (p(l, "ar"), p(l, "br"));
            let (n, p4, p5, p6) = (i("n"), f("p4"), f("p5"), f("p6"));
            if name == "si_nurbsCrvExtendKnot" {
                b2::si_nurbs_crv_extend_knot(&mut l.m, a, bb, n, p4, p5, p6);
            } else {
                b2::si_nurbs_srf_extend_knot(&mut l.m, a, bb, n, p4, p5, p6);
            }
            Answer::None
        }
        "SI_nurbsCrvMerge" | "SI_nurbsCrvBlend" => {
            let (a, bb) = (p(l, "ar"), p(l, "br"));
            let (p3, p4, p5) = (f("p3"), f("p4"), f("p5"));
            Answer::Eax(if name == "SI_nurbsCrvMerge" { b2::si_nurbs_crv_merge(&mut l.m, a, bb, p3, p4, p5) } else { b2::si_nurbs_crv_blend(&mut l.m, a, bb, p3, p4, p5) })
        }
        "SI_nurbsCrvMergeLinear" => {
            let (a, bb) = (p(l, "ar"), p(l, "br"));
            Answer::Eax(b2::si_nurbs_crv_merge_linear(&mut l.m, a, bb, f("p3")))
        }
        "SI_nurbsCrvMergeJoin" => {
            let (a, bb) = (p(l, "ar"), p(l, "br"));
            Answer::Eax(b2::si_nurbs_crv_merge_join(&mut l.m, a, bb))
        }
        "SI_nurbsSrfMerge" | "SI_nurbsSrfBlend" => {
            let (a, bb) = (p(l, "ar"), p(l, "br"));
            let (p3, p4, p5) = (f("p3"), f("p4"), f("p5"));
            Answer::Eax(if name == "SI_nurbsSrfMerge" { b2::si_nurbs_srf_merge(&mut l.m, &mut l.o, a, bb, p3, p4, p5) } else { b2::si_nurbs_srf_blend(&mut l.m, &mut l.o, a, bb, p3, p4, p5) })
        }
        "SI_nurbsSrfMergeLinear" => {
            let (a, bb) = (p(l, "ar"), p(l, "br"));
            Answer::Eax(b2::si_nurbs_srf_merge_linear(&mut l.m, &mut l.o, a, bb, f("p3")))
        }
        "SI_nurbsSrfMergeJoin" => {
            let (a, bb) = (p(l, "ar"), p(l, "br"));
            Answer::Eax(b2::si_nurbs_srf_merge_join(&mut l.m, &mut l.o, a, bb))
        }
        _ => unreachable!("{name}"),
    }
}
