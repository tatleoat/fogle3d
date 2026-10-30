// SPDX-License-Identifier: MIT
//! `qsmisc-checkkit <directory of vector files>`: `qstretch_misc.c` (`si_expr::image::qstretch_misc`) on `si-checkkit`
//! against the emulated original (finding 0013). Vectors: `re/tools/misc1/gen_qsmisc.py`.
use si_checkkit::Kit;
use si_expr::image::qstretch_misc::*;

fn main() {
    // the parser's tables are read from the user's install (docs/design/runtime-data.md)
    si_rt::check_or_exit("qsmisc-checkkit");
    Kit::new(&[
        ("SI_QSequal", |m, _o, a| qs_equal(m, a.v("a"), a.v("b")).into()),
        ("fcurveCheckTriplet", |m, o, a| fcurve_check_triplet(m, o, a.v("model"), a.i("relid")).into()),
        ("si_fcurveKinStateCheckAll", |m, o, a| fcurve_kin_state_check_all(m, o, a.v("model")).into()),
    ])
    .run()
}
