// SPDX-License-Identifier: MIT
//! `prim2-checkkit <vector file or directory>...`: the routines of unit "prim2" (finding 0023,
//! section 44: `SI_besselEndNurbsCrv` and the NURBS primitives) on the image, against vectors
//! from `re/tools/prim2/gen_prim2.py` (finding 0011, "A check binary with si-checkkit").

use si_checkkit::Kit;
use si_model::{cross_image as c, nurbs_prim_image as n};

fn main() {
    Kit::new(&[
        ("SI_besselEndNurbsCrv", |m, _o, a| {
            c::bessel_end_nurbs_crv(m, a.v("crv"), a.s("flag"), [a.f("t0x"), a.f("t0y"), a.f("t0z"), a.f("t0w")], [a.f("t1x"), a.f("t1y"), a.f("t1z"), a.f("t1w")]).into()
        }),
        ("SI_revolveNurbs", |m, o, a| n::revolve_nurbs(m, o, a.v("crv"), a.i("steps"), a.i("stype"), a.f("angle"), a.i("axis"), a.v("model"), a.s("scale"), a.v("p8")).into()),
        ("SI_nurbsTorus", |m, o, a| n::nurbs_torus(m, o, a.f("radius"), a.f("offset"), a.i("n"), a.i("steps"), a.i("stype"), a.i("order")).unwrap_or(0).into()),
        ("SI_nurbsArc", |m, o, a| n::nurbs_arc(m, o, a.f("radius"), a.f("a"), a.f("b"), a.i("order"), a.i("n")).unwrap_or(0).into()),
        ("SI_nurbsSphere", |m, o, a| n::nurbs_sphere(m, o, a.f("radius"), a.i("segs"), a.i("steps"), a.i("stype"), a.i("order")).unwrap_or(0).into()),
    ])
    .given_lib_column()
    .run()
}
