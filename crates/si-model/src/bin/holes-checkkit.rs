// SPDX-License-Identifier: MIT
//! `holes-checkkit <vector file or directory>...`: the hole helpers of `triangulate.c` on the
//! image (finding 0015), against vectors from `re/tools/tri1/gen_holes.py`.

use si_checkkit::Kit;
use si_mem::{Mem, Routine};
use si_model::mesh_image::holes as h;
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
        ("PolygonHasEdge", |m, _o, a| (h::polygon_has_edge(m, a.v("mesh"), a.v("poly"), a.i("a"), a.i("b")) as u8).into()),
        ("EdgeNotThere", |m, _o, a| (h::edge_not_there(m, a.v("list"), a.i("n"), a.i("a"), a.i("b")) as u8).into()),
        ("FindEdgeIdFromTwoVertices", |m, _o, a| h::find_edge_id_from_two_vertices(m, a.v("mesh"), a.i("v"), a.i("w"), a.v("out")).into()),
        ("SI_removeHolesFromMesh", |m, o, a| h::remove_holes_from_mesh(m, &mut Side(o), a.v("mesh")).into()),
    ])
    .given_lib_column()
    .run()
}
