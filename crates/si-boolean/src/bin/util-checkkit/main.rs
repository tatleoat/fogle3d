// SPDX-License-Identifier: MIT
//! `util-checkkit <directory of vector files>`: the six constructors of `bool_util.c` (`util.rs`) held against the
//! emulated original (finding 0037). Vectors: `re/tools/si-boolean/gen_util.py`.

use si_boolean::boolean as b;
use si_boolean::util;
use si_checkkit::Kit;

fn main() {
    Kit::new(&[
        ("si_createFaceGroup", |m, o, _a| util::si_create_face_group(m, o).into()),
        ("si_createEdgeNode", |m, o, _a| util::si_create_edge_node(m, o).into()),
        ("si_createContour", |m, o, a| util::si_create_contour(m, o, a.v("a")).into()),
        ("si_createBoolFace", |m, o, a| util::si_create_bool_face(m, o, a.v("src")).into()),
        ("si_createRegBspNode", |m, o, a| util::si_create_reg_bsp_node(m, o, a.v("a")).into()),
        ("si_createFaceList", |m, o, a| util::si_create_face_list(m, o, a.v("a"), a.v("b")).into()),
    ])
    .normalise_stack_args()
    .setup(|m| si_mem::init_globals(m, b::ALL))
    .run()
}
