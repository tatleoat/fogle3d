// SPDX-License-Identifier: MIT
//! The arms of `nurbs-trimcheck` for the nurbs13 routines (vectors of `re/tools/si-nurbs/gen_geom13.py`).

use super::{Answer, Loaded};
use si_nurbs::geom13 as g;
use si_nurbs::geom13sys as sys;

pub const NAMES: &[&str] = &["SI_skin2Nurbs", "SI_skin2NurbsFarin", "setup_closed_system", "setup_closed_input", "LU_decomp", "solve_LU", "SI_createSkinFromNurbs"];

pub fn run(name: &str, l: &mut Loaded, p: &dyn Fn(&Loaded, &str) -> u32, i: &dyn Fn(&str) -> i32, _f: &dyn Fn(&str) -> f64) -> Answer {
    match name {
        "SI_skin2Nurbs" => {
            let a = p(l, "mar");
            Answer::Eax(g::skin2_nurbs(&mut l.m, &mut l.o, a, i("cnt"), i("ty"), i("cl"), i("fg") as u32))
        }
        "SI_skin2NurbsFarin" => {
            let a = p(l, "mar");
            let sk = p(l, "skr");
            Answer::Eax(g::skin2_nurbs_farin(&mut l.m, &mut l.o, a, i("cnt"), sk, i("cl")))
        }
        "SI_createSkinFromNurbs" => {
            let (a, sl, sk) = (p(l, "mar"), p(l, "slr"), p(l, "skr"));
            Answer::Eax(g::create_skin_from_nurbs(&mut l.m, &mut l.o, a, sl, sk) as u32)
        }
        "setup_closed_system" => {
            let (k, mm) = (p(l, "kp"), p(l, "mr"));
            sys::setup_closed_system(&mut l.m, k, i("n"), mm);
            Answer::None
        }
        "setup_closed_input" => {
            let (k, s, d) = (p(l, "kp"), p(l, "srcp"), p(l, "dstp"));
            sys::setup_closed_input(&mut l.m, k, s, d, i("n"));
            Answer::None
        }
        "LU_decomp" => {
            let (a, lo, up) = (p(l, "ar"), p(l, "lr"), p(l, "ur"));
            sys::lu_decomp(&mut l.m, a, i("n"), lo, up);
            Answer::None
        }
        "solve_LU" => {
            let (up, lo, r, cr) = (p(l, "ur"), p(l, "lr"), p(l, "rhsp"), p(l, "crp"));
            sys::solve_lu(&mut l.m, up, lo, r, cr, i("n"));
            Answer::None
        }
        _ => unreachable!("{name}"),
    }
}
