// SPDX-License-Identifier: MIT
//! `convex-checkkit <vector file or directory>...`: the convex splitter of `triangulate.c` on the
//! image (finding 0015), against vectors from `re/tools/tri1/gen_convex.py`.

use si_checkkit::Kit;
use si_mem::{Mem, Routine};
use si_model::mesh_image::convex as c;
use si_model::model_image::{Callee, Outside};

/// The kit's outside world as `si-model`'s image module calls out.
struct Side<'a>(&'a mut dyn si_mem::Outside);

impl Outside for Side<'_> {
    fn call(&mut self, m: &mut Mem, callee: &Callee, args: &[u32]) -> u32 {
        self.0.call(m, Routine::new(callee.name, callee.addr), args).eax
    }
}

fn main() {
    Kit::new(&[
        ("makeConvexContours", |m, _o, a| (c::make_convex_contours(m, a.v("pts"), a.v("list"), a.i("n"), a.v("out")) as u32).into()),
        ("makeConvexMesh", |m, o, a| c::make_convex_mesh(m, &mut Side(o), a.v("mesh")).into()),
    ])
    .given_lib_column()
    .run()
}
