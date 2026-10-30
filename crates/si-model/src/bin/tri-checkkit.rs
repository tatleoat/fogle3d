// SPDX-License-Identifier: MIT
//! `tri-checkkit <vector file or directory>...`: `SI_meshTriangulate` on the image (finding 0015),
//! against vectors from `re/tools/tri2/gen_triangulate.py`.

use si_checkkit::Kit;
use si_mem::{Mem, Routine};
use si_model::mesh_image::tri;
use si_model::model_image::{Callee, Outside};

/// The kit's outside world as `si-model`'s image module calls out.
struct Side<'a>(&'a mut dyn si_mem::Outside);

impl Outside for Side<'_> {
    fn call(&mut self, m: &mut Mem, callee: &Callee, args: &[u32]) -> u32 {
        self.0.call(m, Routine::new(callee.name, callee.addr), args).eax
    }
}

fn main() {
    Kit::new(&[("SI_meshTriangulate", |m, o, a| (tri::mesh_triangulate(m, &mut Side(o), a.v("mesh"), a.i("arg") as i16) as u16).into())])
        .given_lib_column()
        .run()
}
