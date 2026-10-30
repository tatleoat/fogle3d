// SPDX-License-Identifier: MIT
//! Check of `SI_voluntaryExit` (`renutil.c`) on the vectors of
//! `re/tools/app2/gen_voluntary.py`: the image against what the original left
//! (`SI_renMessage`), the calls (queue, buttons, text, longjmp) replayed from the record,
//! and the answer in `eax`.

use crate::pass10::run;
use si_render::renutil_image;
use si_x87::vectors::{Case, Check, Verdict};

fn check_voluntary_exit(c: &Case) -> Verdict {
    run(
        c,
        &[],
        |m, _, hooks, _| {
            let answer = renutil_image::voluntary_exit(m, hooks, c.input("code").u32() as i32, c.input("status").u32() as u16 as i16);
            Ok(Some(("eax", answer)))
        },
        |_, _| Ok(()),
    )
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_voluntaryExit" => check_voluntary_exit,
        _ => return None,
    })
}
