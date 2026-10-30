// SPDX-License-Identifier: MIT
//! The arms of `nurbs-trimcheck` for the last1 routines (vectors of `re/tools/last1/gen_last1.py`).

use super::{Answer, Loaded};
use si_nurbs::close as c;

pub const NAMES: &[&str] = &["SI_nurbsSrfClose", "SI_nurbsConvertToFace"];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, i: &dyn Fn(&str) -> i32, _f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "SI_nurbsSrfClose" => {
            let s = p(l, "sr");
            if let Err(e) = c::srf_close(&mut l.m, &mut l.o, s, i("how")) {
                eprintln!("SI_nurbsSrfClose: {e:?}");
            }
            Answer::None
        }
        "SI_nurbsConvertToFace" => {
            let (cr, fc) = (p(l, "crv"), p(l, "fcp"));
            c::nurbs_convert_to_face(&mut l.m, &mut l.o, cr, fc);
            Answer::None
        }
        _ => unreachable!("{name}"),
    }
}
