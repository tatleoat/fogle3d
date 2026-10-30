// SPDX-License-Identifier: MIT
//! Checks of the eleventh pass (finding 0012, section 20), on the vectors of
//! `re/tools/si-render/emu/gen_pass11.py`: three routines of `renutil.c` (`SI_warning`,
//! `SI_dtime`, `SI_segvHandler`), the sub-region box in pixels (`matter.c`), the two
//! helpers of the sun (`sun_fct.c`), two routines of `t2d_fct.c` and one of
//! `mat_global.c`, each on the memory image. A case is loaded and compared as the tenth
//! pass's are ([`crate::pass10::run`]): the image against everything the case says the
//! original left, the calls of other files replayed from the record.

use crate::pass10::{at, run, verdict};
use crate::prep::mutate;
use si_render::{mat_global, matter_image, renutil_image, sun_fct, t2d_fct_image};
use si_x87::vectors::{Case, Check, Verdict};
use si_x87::F80;

/// The text argument: the case's buffer, or null in the `#null` file (a number there).
fn text_or_null(c: &Case, m: &si_mem::Mem, names: &si_mem::Names, name: &str) -> u32 {
    if c.name.ends_with("#null") {
        c.input(name).u32()
    } else {
        at(m, names, name)
    }
}

fn check_warning(c: &Case) -> Verdict {
    run(
        c,
        &[],
        |m, names, hooks, _| {
            let (a, b) = (at(m, names, "a"), at(m, names, "b"));
            renutil_image::warning(m, hooks, a, b, c.input("code").u32());
            Ok(None)
        },
        |_, _| Ok(()),
    )
}

fn check_dtime(c: &Case) -> Verdict {
    run(
        c,
        &[],
        |m, names, hooks, _| {
            let t = text_or_null(c, m, names, "t");
            renutil_image::dtime(m, hooks, t);
            Ok(None)
        },
        |_, _| Ok(()),
    )
}

fn check_segv(c: &Case) -> Verdict {
    run(
        c,
        &[],
        |m, _, hooks, _| {
            renutil_image::segv_handler(m, hooks);
            Ok(None)
        },
        |_, _| Ok(()),
    )
}

fn check_subregion(c: &Case) -> Verdict {
    run(
        c,
        &[],
        |m, names, hooks, _| {
            let ctl = at(m, names, "c");
            matter_image::update_subregion_values(m, hooks, ctl);
            Ok(None)
        },
        |_, _| Ok(()),
    )
}

/// A routine that answers in the x87 register: the answer against the case's `st0`.
fn same_st0(c: &Case, got: F80) -> Result<(), String> {
    let got = if mutate() {
        let (mant, se) = got.to_bits();
        F80::from_bits(mant ^ 1, se)
    } else {
        got
    };
    if c.output("st0").same_f80(got) {
        Ok(())
    } else {
        Err(format!("st0: original {:?}, port {got:?}", c.output("st0").words()))
    }
}

fn check_latlong(c: &Case) -> Verdict {
    verdict(same_st0(c, sun_fct::latlong2deg(c.input("v").i32())))
}

fn check_eq_time(c: &Case) -> Verdict {
    match crate::outer::load(c) {
        Ok((mut m, names)) => {
            si_render::data::init(&mut m);
            let got = sun_fct::eq_time(&m, at(&m, &names, "tm"));
            verdict(same_st0(c, got))
        }
        Err(e) => Verdict::Different(format!("the case does not load: {e}")),
    }
}

fn check_default_method(c: &Case) -> Verdict {
    run(
        c,
        &[],
        |m, _, _, _| {
            t2d_fct_image::set_default_method(m, c.input("method").i16());
            Ok(None)
        },
        |_, _| Ok(()),
    )
}

fn check_apply_transform(c: &Case) -> Verdict {
    run(
        c,
        &[],
        |m, names, hooks, _| {
            let (map, pair) = (at(m, names, "map"), at(m, names, "p"));
            t2d_fct_image::txt_apply_transform(m, hooks, map, pair);
            Ok(None)
        },
        |_, _| Ok(()),
    )
}

fn check_col_mode(c: &Case) -> Verdict {
    run(c, &[], |m, _, _, _| Ok(Some(("eax", mat_global::light_get_col_mode(m) as u32))), |_, _| Ok(()))
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_warning" => check_warning,
        "SI_dtime" | "SI_dtime#null" => check_dtime,
        "SI_segvHandler" => check_segv,
        "SI_updateSubregionValues" => check_subregion,
        "latlong2deg" => check_latlong,
        "eqTime" => check_eq_time,
        "SI_t2dSetDefaultMethod" => check_default_method,
        "SI_txtApplyTransform" => check_apply_transform,
        "SI_lightGetColMode" => check_col_mode,
        _ => return None,
    })
}
