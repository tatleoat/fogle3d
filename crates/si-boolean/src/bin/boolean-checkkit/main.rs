// SPDX-License-Identifier: MIT
//! `boolean-checkkit <directory of vector files>`: the ports of `boolean.c` held against the
//! emulated original (finding 0011, "A check binary with si-checkkit"; finding 0037). Vectors:
//! `re/tools/si-boolean/gen_boolean.py`.

use si_boolean::boolean as b;
use si_checkkit::{Args, Kit};

fn dbl(a: &Args, name: &str) -> f64 {
    a.case().input(&format!("v_{name}")).f64()
}

fn flt(a: &Args, name: &str) -> f32 {
    a.case().input(&format!("v_{name}")).f32()
}

fn main() {
    Kit::new(&[
        ("si_getMem", |m, o, a| b::si_get_mem(m, o, a.i("size")).into()),
        ("getPoint", |m, o, _a| b::get_point(m, o).into()),
        ("findPointId", |m, _o, a| b::find_point_id(m, a.i("n")).into()),
        ("getEdge", |m, o, _a| b::get_edge(m, o).into()),
        ("findEdgeId", |m, _o, a| b::find_edge_id(m, a.i("n")).into()),
        ("resetEdgeId", |m, _o, _a| b::reset_edge_id(m).into()),
        ("nbResFeatures", |m, _o, a| b::nb_res_features(m, a.v("face")).into()),
        ("extractFace", |m, _o, a| b::extract_face(m, a.v("face")).into()),
        ("resetFace", |m, _o, a| b::reset_face(m, a.v("face")).into()),
        ("resetListOfFaces", |m, _o, a| b::reset_list_of_faces(m, a.v("face")).into()),
        ("openObject", |m, _o, a| b::open_object(m, a.v("face")).into()),
        ("findEdgeNode", |m, _o, a| b::find_edge_node(m, a.v("face"), a.v("edge"), a.v("pcon")).into()),
        ("markSelectedFace", |m, _o, a| b::mark_selected_face(m, a.v("face"), a.v("tree"), a.v("which") as u8).into()),
        ("findLastFaceOfList", |m, _o, a| b::find_last_face_of_list(m, a.v("face")).into()),
        ("mergeListOfFaces", |m, _o, a| b::merge_list_of_faces(m, a.v("a"), a.v("b")).into()),
        ("mergeTwoListsOfFaces", |m, _o, a| {
            b::merge_two_lists_of_faces(m, a.v("a"), a.v("alast"), a.v("b"), a.v("blast"), a.v("plast")).into()
        }),
        ("setStatus", |m, _o, a| b::set_status(m, a.v("face"), a.v("status") as i8).into()),
        ("removeFaceFromList", |m, _o, a| b::remove_face_from_list(m, a.v("plist"), a.v("face")).into()),
        ("reverseFace", |m, _o, a| b::reverse_face(m, a.v("face")).into()),
        ("reverseTree", |m, _o, a| b::reverse_tree(m, a.v("tree")).into()),
        ("ptIsOnPlane", |m, _o, a| b::pt_is_on_plane(m, a.v("tree"), a.v("pt")).into()),
        ("getPtPos", |m, _o, a| (b::get_pt_pos(m, a.v("tree"), a.v("pt")) as u32).into()),
        ("testBox", |m, _o, a| b::test_box(m, a.v("mn"), a.v("mx"), a.v("pl")).into()),
        ("si_whichSide", |m, _o, a| b::which_side(m, a.v("face"), a.v("tree")).into()),
        ("locatePt", |m, _o, a| {
            let (pt, i) = b::locate_pt(m, [dbl(&a, "x"), dbl(&a, "y"), dbl(&a, "z")]);
            m.set_i32(a.v("pi"), i);
            pt.into()
        }),
        ("findPoint", |m, _o, a| b::find_point(m, a.v("tree"), [dbl(&a, "x"), dbl(&a, "y"), dbl(&a, "z")]).into()),
        ("splitEdge", |m, o, a| b::split_edge(m, o, a.v("tree"), dbl(&a, "dist"), a.v("p3"), a.v("p4")).into()),
        ("addEdgeToInter", |m, _o, a| b::add_edge_to_inter(m, a.v("a"), a.v("b"), a.v("edge")).into()),
        ("addInterNode", |m, _o, a| b::add_inter_node(m, a.v("pfirst"), a.v("phead"), a.v("node"), a.v("vec")).into()),
        ("getInterListNode", |m, o, a| b::get_inter_list_node(m, o, a.v("flag")).into()),
        ("classifyPoint", |m, _o, a| (b::classify_point(m, a.v("tree"), a.v("v")) as u32).into()),
        ("computeStatus", |m, _o, a| (b::compute_status(m, a.v("tree"), a.v("face")) as u32).into()),
        ("getNbEdges", |m, _o, a| (b::get_nb_edges(m, a.v("node")) as u32).into()),
        ("deleteEdge", |m, _o, a| b::delete_edge(m, a.v("face"), a.v("edge")).into()),
        ("si_setClassifying", |m, _o, a| b::set_classifying(m, a.v("v") as u8).into()),
        ("si_setCleanup", |m, _o, a| b::set_cleanup(m, a.v("e") as u8, a.v("p") as u8).into()),
        ("si_initBoolean", |m, _o, _a| b::init_boolean(m).into()),
        ("divideList", |m, o, a| b::divide_list(m, o, a.v("list"), a.i("add"), a.i("limit"), a.v("keep"), a.v("moved")).into()),
        ("colinear", |m, _o, a| b::colinear(m, a.v("n1"), a.v("n2")).into()),
        ("computeFaceSize", |m, _o, a| b::compute_face_size(m, a.v("node"), a.v("min"), a.v("max"), a.v("flag") as u8 != 0).into()),
        ("si_computeFaceGroupSize", |m, _o, a| b::compute_face_group_size(m, a.v("grp")).into()),
        ("nearPointEdge", |m, _o, a| b::near_point_edge(m, a.v("edge"), dbl(&a, "eps")).into()),
        ("mergeSimilarEdge", |m, _o, a| b::merge_similar_edge(m, a.v("edge"), a.v("which")).into()),
        ("computeNormal", |m, _o, a| b::compute_normal(m, a.v("face")).into()),
        ("computeAveragePt", |m, _o, a| b::compute_average_pt(m, a.v("out"), a.v("face")).into()),
        ("verifyEdge", |m, o, a| b::verify_edge(m, o, a.v("tree"), a.v("edge")).into()),
        ("si_cleanUpFaces", |m, o, a| si_boolean::clean::si_clean_up_faces(m, o, a.v("plist")).into()),
        ("si_buildMesh2", |m, o, a| (si_boolean::mesh::si_build_mesh2(m, o, a.v("src"), a.v("pout"), a.v("faces")) as u32).into()),
        ("si_buildMesh", |m, o, a| (si_boolean::mesh::si_build_mesh(m, o, a.v("pout"), a.v("faces")) as u32).into()),
        ("si_cleanUpEdges", |m, _o, a| si_boolean::clean::si_clean_up_edges(m, a.v("plist")).into()),
        ("addPtToEdge", |m, o, a| b::add_pt_to_edge(m, o, a.v("edge"), a.v("pt"), a.v("face")).into()),
        ("insertEdge__FP4edgeT0", |m, _o, a| b::insert_edge(m, a.v("a"), a.v("b")).into()),
        ("splitOnEdge", |m, o, a| b::split_on_edge(m, o, a.v("a"), a.v("b"), a.v("face")).into()),
        ("findOnEdge", |m, o, a| b::find_on_edge(m, o, a.v("a"), a.v("b"), a.v("plist"), a.v("face"), a.v("pout")).into()),
        ("buildFace", |m, o, a| b::build_face(m, o, a.v("src"), a.v("list"), a.v("pstatus")).into()),
        ("cleanUpTree", |m, _o, a| b::clean_up_tree(m, a.v("tree"), a.v("ctree")).into()),
        ("processRegion", |m, o, a| b::process_region(m, o, a.v("r1"), a.v("r2"), a.v("r3"), a.v("r4"), a.v("faces"), a.v("plast")).into()),
        ("classifyRegion", |m, o, a| b::classify_region(m, o, a.v("regions"), a.v("tree"), a.v("back"), a.v("front")).into()),
        ("classifyCoincident", |m, o, a| b::classify_coincident(m, o, a.v("tree"), a.v("group")).into()),
        ("classifyFaces", |m, o, a| b::classify_faces(m, o, a.v("tree"), a.v("group")).into()),
        ("xxxx", |m, o, a| b::xxxx(m, o, a.v("tree"), a.v("group")).into()),
        ("si_getResultingFaces", |m, _o, a| b::si_get_resulting_faces(m, a.v("tree"), a.v("pfirst"), a.v("plast")).into()),
        ("si_releaseMem", |m, _o, _a| b::si_release_mem(m).into()),
        ("polygonTesselate", |m, o, a| b::polygon_tesselate(m, o, a.v("mesh"), a.v("poly"), a.v("pcount")).into()),
        ("computeBarycentricUVCoord", |m, o, a| {
            b::compute_barycentric_uv_coord(m, o, a.v("poly"), a.v("which"), flt(&a, "a"), flt(&a, "b"), flt(&a, "c"), a.v("node")).into()
        }),
        ("computeBarycentricColor", |m, _o, a| b::compute_barycentric_color(m, a.v("poly"), flt(&a, "a"), flt(&a, "b"), flt(&a, "c"), a.v("out")).into()),
        ("computeTriangleBarycentricArea", |m, _o, a| {
            let p = [flt(&a, "px"), flt(&a, "py"), flt(&a, "pz")];
            b::compute_triangle_barycentric_area(m, a.v("mesh"), a.i("n"), a.v("polys"), p, a.v("pidx"), a.v("oa"), a.v("ob"), a.v("oc")).into()
        }),
        ("si_buildBsp", |m, o, a| b::si_build_bsp(m, o, a.v("tree"), a.v("group"), a.v("status") as u8).into()),
        ("dummyInsert", |m, o, a| b::dummy_insert(m, o, a.v("tree"), a.v("face"), a.v("which") as u8).into()),
        ("resetRegion", |m, _o, a| b::reset_region(m, a.v("r")).into()),
        ("si_setMergeDist", |m, _o, a| b::set_merge_dist(m, dbl(&a, "dist")).into()),
    ])
    .normalise_stack_args()
    .setup(|m| si_mem::init_globals(m, b::ALL))
    .run()
}
