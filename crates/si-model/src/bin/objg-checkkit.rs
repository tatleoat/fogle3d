// SPDX-License-Identifier: MIT
//! `objg-checkkit <vector file or directory>...`: the editing tools of `obj_global.c` on the
//! image (finding 0015, 2026-10-28), against vectors from `re/tools/si-model/gen_objg.py`
//! (finding 0011, "A check binary with si-checkkit").

use si_checkkit::Kit;
use si_mem::{Mem, Routine};
use si_model::mesh_image::{dump as du, edit as e, measure as me};
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
        ("SI_meshDump", |m, o, a| du::mesh_dump(m, &mut Side(o), a.v("file"), a.v("mesh"), a.s("flag"), a.v("name")).into()),
        ("SI_meshDumpVertexPolygon", |m, o, a| du::mesh_dump_vertex_polygon(m, &mut Side(o), a.v("file"), a.v("mesh")).into()),
        ("SI_meshDumpPolygonVertex", |m, o, a| du::mesh_dump_polygon_vertex(m, &mut Side(o), a.v("file"), a.v("mesh")).into()),
        ("SI_meshTransformSelectedVertices", |m, _o, a| me::mesh_transform_selected_vertices(m, a.v("mesh"), a.v("point"), a.s("to_point"), a.v("matrix")).into()),
        ("SI_meshMoveVertex", |m, _o, a| me::mesh_move_vertex(m, a.v("mesh"), a.i("n"), a.v("to")).into()),
        ("SI_meshInverse", |m, _o, a| me::mesh_inverse(m, a.v("mesh")).into()),
        ("SI_meshRandomize", |m, _o, a| {
            // the C library's drand48 from its start state, as the harness's stand-in has it
            let mut state: u64 = 0x1234_abcd_330e;
            let mut rand = move || {
                state = state.wrapping_mul(0x5_deec_e66d).wrapping_add(0xb) & 0xffff_ffff_ffff;
                state as f64 / (1u64 << 48) as f64
            };
            me::mesh_randomize(m, a.v("mesh"), a.v("amount"), a.s("times"), a.s("selected"), &mut rand).into()
        }),
        ("SI_meshPolygonVolumeSurface", |m, _o, a| me::mesh_polygon_volume_surface(m, a.v("mesh"), a.i("n"), a.s("scale"), a.v("volume"), a.v("surface")).into()),
        ("SI_meshComputesVolumeSurface", |m, _o, a| me::mesh_computes_volume_surface(m, a.v("mesh"), a.v("volume"), a.v("surface")).into()),
        ("SI_meshCentroid", |m, _o, a| me::mesh_centroid(m, a.v("mesh"), a.v("out")).into()),
        ("meshRemovePolygonAtVertex", |m, _o, a| e::mesh_remove_polygon_at_vertex(m, a.v("mesh"), a.i("vertex"), a.i("polygon"), a.i("prev"), a.i("next"), a.s("track")).into()),
        ("SI_meshTagPolygonByVertex", |m, _o, a| e::mesh_tag_polygon_by_vertex(m, a.v("mesh"), a.i("mask")).into()),
        ("si_meshCompressPolygonsArray", |m, _o, a| e::mesh_compress_polygons_array(m, a.v("mesh"), a.i("n")).into()),
        ("meshRemoveIPolygon", |m, _o, a| e::mesh_remove_ipolygon(m, a.v("mesh"), a.i("n"), a.s("compress"), a.s("track")).into()),
        ("si_meshCompressVerticesArray", |m, _o, a| e::mesh_compress_vertices_array(m, a.v("mesh"), a.i("n")).into()),
        ("meshRemoveUnusedVertices", |m, _o, a| e::mesh_remove_unused_vertices(m, a.v("mesh")).into()),
        ("SI_meshDeletePolygonSelected", |m, o, a| e::mesh_delete_polygon_selected(m, &mut Side(o), a.v("mesh"), a.s("by_vertex"), a.s("track")).into()),
    ])
    .run()
}
