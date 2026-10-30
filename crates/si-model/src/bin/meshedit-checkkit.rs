// SPDX-License-Identifier: MIT
//! `meshedit-checkkit <vector file or directory>...`: the mesh-editing support of
//! `meshEditFct.c` on the image (finding 0015, 2026-10-28), against vectors from
//! `re/tools/si-model/gen_meshedit.py` (finding 0011, "A check binary with si-checkkit").

use si_checkkit::Kit;
use si_mem::{Mem, Routine};
use si_model::mesh_image::edit_fct as f;
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
        ("compareIndices", |m, _o, a| f::compare_indices(m, a.v("a"), a.v("b")).into()),
        ("setEdgeFlag", |m, _o, a| f::set_edge_flag(m, a.v("mesh")).into()),
        ("si_vertexReallocate", |m, _o, a| f::vertex_reallocate(m, a.v("mesh"), a.s("n")).into()),
        ("edgeReallocate", |m, _o, a| f::edge_reallocate(m, a.v("mesh"), a.s("n")).into()),
        ("nodeReallocate", |m, _o, a| f::node_reallocate(m, a.v("poly"), a.s("n")).into()),
        ("neighborReallocate", |m, _o, a| f::neighbor_reallocate(m, a.v("v"), a.s("n")).into()),
        ("SI_nodeSetUV", |m, _o, a| f::node_set_uv(m, a.i("d"), a.f("u"), a.f("v"), a.v("node")).into()),
        ("computeNodeAttributes", |m, _o, a| f::compute_node_attributes(m, a.f("t"), a.v("nodes"), a.i("dst"), a.i("a"), a.i("b")).into()),
        ("findContourSize", |m, _o, a| f::find_contour_size(m, a.v("mesh"), a.i("count"), a.v("node"), a.v("min"), a.v("max")).into()),
        ("locateEdge", |m, _o, a| f::locate_edge(m, a.v("poly"), a.i("edge"), a.v("a"), a.v("b"), a.v("c")).into()),
        ("removeNodesDuplicate", |m, _o, a| f::remove_nodes_duplicate(m, a.v("poly")).into()),
        ("removePolygonFromMesh", |m, _o, a| f::remove_polygon_from_mesh(m, a.v("mesh"), a.i("n")).into()),
        ("nodeShiftRight", |m, _o, a| f::node_shift_right(m, a.v("poly"), a.i("at")).into()),
        ("SI_nodesAllocate", |m, _o, a| f::nodes_allocate(m, a.v("model"), a.v("pp"), a.i("count"), a.i("n")).into()),
        ("insertOutside", |m, _o, a| f::insert_outside(m, a.v("model"), a.v("poly"), a.v("nodes"), a.v("count")).into()),
        ("contourIsCCW", |m, _o, a| (f::contour_is_ccw(m, a.v("verts"), a.v("nodes"), a.i("n")) as u8).into()),
        ("pointInsideContour", |m, _o, a| f::point_inside_contour(m, a.f("x"), a.f("y"), a.f("z"), a.v("verts"), a.i("count"), a.v("nodes")).into()),
        ("dispatchHoles", |m, _o, a| f::dispatch_holes(m, a.v("poly"), a.v("verts"), a.v("nodes_a"), a.v("count_a"), a.v("nodes_b"), a.v("count_b"), a.i("skip")).into()),
        ("attachHoles", |m, _o, a| f::attach_holes(m, a.v("poly"), a.v("nodes"), a.v("count"), a.i("skip_a"), a.i("skip_b")).into()),
        ("SI_addVertexToMesh", |m, o, a| f::add_vertex_to_mesh(m, &mut Side(o), a.v("mesh"), a.f("x"), a.f("y"), a.f("z"), a.v("e")).into()),
        ("SI_addEdgeToMesh", |m, o, a| f::add_edge_to_mesh(m, &mut Side(o), a.v("model"), a.v("mesh"), a.s("a"), a.s("b"), a.s("c")).into()),
        ("si_addPolygonToMesh", |m, o, a| f::add_polygon_to_mesh(m, &mut Side(o), a.v("model"), a.v("mesh"), a.i("n"), a.v("vs")).into()),
        ("si_removeVertexFromMesh", |m, o, a| f::remove_vertex_from_mesh(m, &mut Side(o), a.v("model"), a.v("mesh"), a.i("v"), a.i("track")).into()),
        ("si_collapseVertexInMesh", |m, o, a| f::collapse_vertex_in_mesh(m, &mut Side(o), a.v("model"), a.v("mesh"), a.i("v"), a.i("track")).into()),
        ("SI_removeEdgeFromMesh", |m, o, a| { let _ = o; f::remove_edge_from_mesh(m, a.v("mesh"), a.i("edge"), a.s("track")).into() }),
        ("si_collapseEdgeInMesh", |m, o, a| f::collapse_edge_in_mesh(m, &mut Side(o), a.v("mesh"), a.i("edge")).into()),
        ("SI_meshPopulateUVDomains", |m, _o, a| f::mesh_populate_uv_domains(m, a.v("model"), a.i("which")).into()),
        ("SI_meshAssociateTxtUVDomain", |m, _o, a| f::mesh_associate_txt_uv_domain(m, a.v("model"), a.i("tex"), a.i("dom")).into()),
        ("SI_meshSetMultiUVFlag", |m, _o, a| f::mesh_set_multi_uv_flag(m, a.v("model"), a.s("on")).into()),
        ("acceptMultiUVchanges", |m, o, a| f::accept_multi_uv_changes(m, &mut Side(o), a.v("elem"), a.s("on"), a.v("list"), a.i("tex")).into()),
        ("SI_meshStoreUVInfo", |m, _o, a| { f::mesh_store_uv_info(m, a.v("model"), a.v("info")); 0u32.into() }),
        ("SI_meshRestoreUVInfoCoords", |m, _o, a| { f::mesh_restore_uv_info_coords(m, a.v("model"), a.v("info"), a.s("fresh")); 0u32.into() }),
        ("SI_meshRestoreUVInfo", |m, _o, a| { f::mesh_restore_uv_info(m, a.v("model"), a.v("info")); 0u32.into() }),
        ("SI_meshDeleteUVInfo", |m, _o, a| { f::mesh_delete_uv_info(m, a.v("info")); 0u32.into() }),
        ("initVisualMdlList", |m, o, a| { f::init_visual_mdl_list(m, &mut Side(o), a.v("dialog"), a.v("list"), a.v("elems"), a.i("n"), a.v("sel"), a.i("tex")); 0u32.into() }),
        ("initVisualDomainList", |m, o, a| { f::init_visual_domain_list(m, &mut Side(o), a.v("dialog"), a.v("list"), a.v("elem"), a.i("tex"), a.s("eight")); 0u32.into() }),
        ("SI_validMesh", |m, o, a| f::valid_mesh(m, &mut Side(o), a.v("mesh"), a.s("track")).into()),
        ("si_collapsePolygonInMesh", |m, o, a| f::collapse_polygon_in_mesh(m, &mut Side(o), a.v("mesh"), a.i("poly")).into()),
        ("SI_canRemoveEdgeFromMesh", |m, o, a| { let _ = o; f::can_remove_edge_from_mesh(m, a.v("mesh"), a.i("edge")).into() }),
    ])
    .given_lib_column()
    .run()
}
