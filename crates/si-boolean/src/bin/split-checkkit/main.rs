// SPDX-License-Identifier: MIT
//! `split-checkkit <directory of vector files>`: `si_splitFace` held against the emulated original (finding 0037).
//! Vectors: `gen_boolean.py` (label `si_splitFace`), made without stand-ins for the three constructors of `bool_util.c`
//! (the original runs its own); here the port's calls to them are answered by the Rust below.

use si_boolean::boolean as b;
use si_boolean::util;
use si_checkkit::Kit;
use si_mem::{Mem, Outside, Ret, Routine};

include!("route.rs");

fn main() {
    Kit::new(&[("si_splitFace", |m, o, a| {
        si_boolean::split::si_split_face(m, o, a.v("tree"), a.v("face"), a.v("pback"), a.v("pfront")).into()
    }),
    ("si_generateFaceList", |m, o, a| {
        si_boolean::mesh::si_generate_face_list(m, o, a.v("mesh"), a.v("flag") as i32, a.v("pa"), a.v("pb")).into()
    }),
    ("SI_computeBoolean", |m, o, a| {
        si_boolean::mesh::si_compute_boolean(m, o, a.v("setup"), a.v("a"), a.v("b"), a.v("poa"), a.v("pob")).into()
    })])
    .normalise_stack_args()
    .route(route)
    .setup(|m| si_mem::init_globals(m, b::ALL))
    .run()
}
