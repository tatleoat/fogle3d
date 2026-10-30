// SPDX-License-Identifier: MIT
//! `prim-checkkit <vector file or directory>...`: the spline primitives of `primitive.c` on the
//! image (finding 0023, "mesh1"), against vectors from `re/tools/mesh1/gen_prim.py`
//! (finding 0011, "A check binary with si-checkkit").

use si_checkkit::Kit;
use si_model::primitive_image as p;
use si_spline::spline_image::{self, NoOutside};

fn main() {
    Kit::new(&[
        ("SI_splineCircle", |m, _o, a| p::spline_circle(m, a.i("type"), a.f("radius"), a.i("n")).into()),
        ("SI_splineSquare", |m, _o, a| p::spline_square(m, a.f("size")).into()),
        ("SI_splineArc", |m, _o, a| p::spline_arc(m, a.i("type"), a.f("radius"), a.f("start"), a.f("end"), a.s("n")).into()),
        ("SI_splineSpiral", |m, _o, a| p::spline_spiral(m, a.i("type"), a.f("r0"), a.f("r1"), a.f("a0"), a.f("a1"), a.f("height"), a.s("n")).into()),
        ("SI_nurbsCircle", |m, _o, a| p::nurbs_circle(m, a.f("radius"), a.i("order"), a.i("n")).unwrap_or(0).into()),
        ("SI_nurbsSpiral", |m, o, a| p::nurbs_spiral(m, o, a.f("r0"), a.f("r1"), a.f("a0"), a.f("a1"), a.f("height"), a.i("order"), a.s("n")).unwrap_or(0).into()),
        ("SI_patchSphere", |m, _o, a| p::patch_sphere(m, &mut NoOutside, a.f("radius"), a.s("n"), a.s("segs"), a.i("rev"), a.i("type")).into()),
        ("SI_patchTorus", |m, _o, a| p::patch_torus(m, &mut NoOutside, a.f("radius"), a.f("offset"), a.s("n"), a.s("segs"), a.i("rev"), a.i("type")).into()),
    ])
    .setup(spline_image::init)
    .given_lib_column()
    .run()
}
