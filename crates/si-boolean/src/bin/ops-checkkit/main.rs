// SPDX-License-Identifier: MIT
//! `ops-checkkit <directory of vector files>`: the callers of the boolean engine (`ops.rs`: clipPlane.c, subdivide.c,
//! bool_ui.c, animBool.c) held against the emulated original (finding 0037). Vectors: `re/tools/si-boolean/gen_ops.py`.
//! The routines the originals ran as their own code (the constructors of `bool_util.c`, `si_splitFace`, `SI_malloc`)
//! are answered by the same routes as in `split-checkkit`.

use si_boolean::boolean as b;
use si_boolean::util;
use si_boolean::ops;
use si_checkkit::Kit;
use si_mem::{Mem, Outside, Ret, Routine};

include!("../split-checkkit/route.rs");

fn main() {
    Kit::new(&[
        ("SI_meshClipPlanes", |m, o, a| ops::si_mesh_clip_planes(m, o, a.v("mesh"), a.i("n"), a.v("planes")).into()),
        ("SI_meshCutPlanes", |m, o, a| {
            let f = |n: &str| a.case().input(n).f32();
            ops::si_mesh_cut_planes(
                m,
                o,
                a.v("mesh"),
                [f("v_minx"), f("v_miny"), f("v_minz")],
                [f("v_maxx"), f("v_maxy"), f("v_maxz")],
                [a.i("nx") as i16, a.i("ny") as i16, a.i("nz") as i16],
            )
            .into()
        }),
        ("si_copyModelMat", |m, o, a| {
            ops::si_copy_model_mat(m, o, a.v("model"), a.v("a"), a.v("b"));
            0u32.into()
        }),
        ("SI_animBooleanUpdate", |m, o, a| ops::si_anim_boolean_update(m, o, a.v("node")).into()),
    ])
    .normalise_stack_args()
    .route(route)
    .setup(|m| si_mem::init_globals(m, b::ALL))
    .run()
}
