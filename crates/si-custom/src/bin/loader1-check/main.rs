// SPDX-License-Identifier: MIT
//! Check binary of unit loader1 (the loader-only-optional leftovers): ports kept apart from `custom-checkkit`.

use si_custom::custom_dyn::*;

fn main() {
    si_checkkit::run(&[
        ("SI_cusRunStandalone", |m, o, a| si_cus_run_standalone(m, o, a.v("symbol"), a.v("argc") as i32, a.v("argv")).into()),
    ]);
}
