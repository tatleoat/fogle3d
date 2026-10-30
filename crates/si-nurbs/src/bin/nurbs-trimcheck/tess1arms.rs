// SPDX-License-Identifier: MIT
//! The arms of `nurbs-trimcheck` for the nurbstess1 routines (vectors of `re/tools/si-nurbs/gen_tess1.py`).

use super::{Answer, Loaded};
use si_nurbs::chgknots as ck;
use si_nurbs::flatten as f;
use si_nurbs::split as sp;

pub const NAMES: &[&str] = &["SI_nurbsCrvFlatten", "si_nurbsCrvChangeKnotsSeq", "si_nurbsSrfChangeKnotsSeq", "nrb_CRVsubdivc", "nrb_subdivc", "si_OLDnurbsSrfChangeKnotsSeq", "SI_nurbsSrfInsertKnot", "SI_nurbsSrfNoTriangles", "SI_nurbsCrvSamples", "si_nurbsSrfSampleTrims", "SI_nurbsCrvSplit", "SI_nurbsSrfSplit"];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, i: &dyn Fn(&str) -> i32, _f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "SI_nurbsCrvFlatten" => {
            let c = p(l, "cur");
            // a refusal (the original would not end its span search) answers 0: such cases are not compared
            { let r = f::crv_flatten(&mut l.m, c); if std::env::var("NURBS_TRACE").is_ok() { eprintln!("flatten order {} np {} nk {} closed {} per {} -> {r:x?}", l.m.i32(c), l.m.i32(c+4), l.m.i32(c+8), l.m.i16(c+0x20), l.m.i32(c+0x34)); } Answer::Eax(r.unwrap_or(0)) }
        }
        "si_nurbsCrvChangeKnotsSeq" => { let (c, k) = (p(l, "cur"), p(l, "nkp")); Answer::Eax(ck::crv_change_knots_seq(&mut l.m, c, i("n"), k).unwrap_or(0) as u32) }
        "si_nurbsSrfChangeKnotsSeq" => { let (c, k) = (p(l, "sfr"), p(l, "nkp")); Answer::Eax(ck::srf_change_knots_seq(&mut l.m, c, i("n"), k).unwrap_or(0) as u32) }
        "nrb_CRVsubdivc" => { let (c, o) = (p(l, "cur"), p(l, "our")); let _ = ck::crv_subdivc(&mut l.m, c, o, i("left"), i("idx")); Answer::None }
        "nrb_subdivc" => { let (c, o) = (p(l, "sfr"), p(l, "our")); let _ = ck::srf_subdivc(&mut l.m, c, o, i("left"), i("idx")); Answer::None }
        "si_OLDnurbsSrfChangeKnotsSeq" => { let (c, k) = (p(l, "sfr"), p(l, "nkp")); { let r = ck::old_srf_change_knots_seq(&mut l.m, c, i("n"), k); if std::env::var("NURBS_TRACE").is_ok() { eprintln!("old changer -> {r:?}"); } Answer::Eax(r.unwrap_or(0) as u32) } }
        "SI_nurbsSrfInsertKnot" => { let c = p(l, "sr"); { let r = sp::srf_insert_knot(&mut l.m, c, _f("t"), i("d")); if std::env::var("NURBS_TRACE").is_ok() { eprintln!("insert knot d={} -> {r:?}", i("d")); } Answer::Eax(r.unwrap_or(0) as u32) } }
        "SI_nurbsSrfNoTriangles" => { let c = p(l, "sfr"); Answer::Eax(sp::srf_no_triangles(&mut l.m, c).unwrap_or(0)) }
        "SI_nurbsCrvSamples" => { let (c, n, a) = (p(l, "cur"), p(l, "cnt"), p(l, "arr")); let _ = si_nurbs::tess2::crv_samples_img(&mut l.m, c, i("mode"), _f("step"), n, a); Answer::None }
        "si_nurbsSrfSampleTrims" => { let c = p(l, "sfr"); let _ = si_nurbs::tess2::sample_trims(&mut l.m, c); Answer::None }
        "SI_nurbsCrvSplit" => { let (c, a, b) = (p(l, "cur"), p(l, "o1"), p(l, "o2")); let _ = sp::crv_split(&mut l.m, c, _f("t"), a, b); Answer::None }
        "SI_nurbsSrfSplit" => { let (c, a, b) = (p(l, "sr"), p(l, "o1"), p(l, "o2")); let r = sp::srf_split(&mut l.m, &mut l.o, c, _f("t"), i("d"), a, b); if std::env::var("NURBS_TRACE").is_ok() { eprintln!("srf split d={} -> {r:?}", i("d")); } Answer::None }
        _ => unreachable!("{name}"),
    }
}
