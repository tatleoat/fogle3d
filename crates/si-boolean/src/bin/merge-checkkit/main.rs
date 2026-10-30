// SPDX-License-Identifier: MIT
//! `merge-checkkit <directory of vector files>`: `SI_meshMerge` (`merge.rs`) held against the emulated original
//! (finding 0037). Vectors: `re/tools/si-boolean/gen_merge.py`. The boolean engine's functions run as the original's
//! own code in the vectors and are called directly by the port; the constructors of `bool_util.c` are routed to
//! the ports of `util.rs`.

use si_boolean::boolean as b;
use si_boolean::merge;
use si_boolean::util;
use si_checkkit::Kit;
use si_mem::{Mem, Outside, Ret, Routine};

include!("../split-checkkit/route.rs");

fn f64arg(a: &si_checkkit::Args, name: &str) -> f64 {
    use si_x87::vectors::Word;
    let f = a.case().try_input(&format!("v_{name}")).unwrap_or_else(|| panic!("no argument {name}"));
    match f.words().first() {
        Some(Word::Value { value, .. }) => f64::from_bits(*value as u64),
        _ => panic!("argument {name} has no value"),
    }
}

fn main() {
    Kit::new(&[("SI_meshMerge", |m, o, a| merge::si_mesh_merge(m, o, a.i("n"), a.v("models"), a.i("mode") as i16, f64arg(a, "dist")).into())])
        .normalise_stack_args()
        .route(route)
        .setup(|m| si_mem::init_globals(m, b::ALL))
        .run()
}
