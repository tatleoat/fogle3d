// SPDX-License-Identifier: MIT
//! From a vector file's routine name to the port: one line per function.

use crate::{Args, Ret};
use si_mem::Mem;
use si_view::db2scn as d;
use si_view::func as f;
use si_view::graflist as g;
use si_view::graphsort as s;
use si_view::hidden::{edges as he, geom as hg, lists as hl, tree as ht};
use si_view::nselections as n;
use si_view::outside::Outside;

pub fn run(name: &str, m: &mut Mem, o: &mut dyn Outside, a: &Args) -> Option<Ret> {
    Some(match name {
        // graflist.c: the array of lists, nodes, look-ups
        "SI_schematicNoRefresh" => g::schematic_no_refresh(m, a.s("on")).into(),
        "SI_grafnodeDefColor" => g::grafnode_def_color(m).into(),
        "SI_grafnodeGetName" => g::grafnode_get_name(m, a.p("node")).into(),
        "SI_grafnodeChangeName" => g::grafnode_change_name(m, o, a.p("node"), a.p("name")).into(),
        "si_graflistFindParent" => g::graflist_find_parent(m, a.p("node")).into(),
        "SI_graflistFindRoot" => g::graflist_find_root(m, a.p("node")).into(),
        "SI_grafListExists" => g::graf_list_exists(m, a.i("index")).into(),
        "grafListInit" => g::graf_list_init(m, a.p("list")).into(),
        "SI_grafListAllocate" => g::graf_list_allocate(m).into(),
        "SI_grafListCreate" => g::graf_list_create(m).into(),
        "SI_grafListAdd" => g::graf_list_add(m).into(),
        "SI_graflistClear" => g::graflist_clear(m, a.p("list")).into(),
        "SI_grafListRemove" => g::graf_list_remove(m, a.i("index")).into(),
        "SI_graflistSetActive" => g::graflist_set_active(m, a.p("list"), a.p("node")).into(),
        "grafnodeAllocate" => g::grafnode_allocate(m).into(),
        "graflistInsertGrafnode" => g::graflist_insert_grafnode(m, a.p("list"), a.p("after"), a.p("node")).into(),
        "SI_graflistRemoveGrafnode" => g::graflist_remove_grafnode(m, o, a.p("list"), a.p("node")).into(),
        "SI_nextGrafnode" => g::next_grafnode(m, a.p("list"), a.p("cursor")).into(),
        "SI_findGlData" => g::find_gl_data(m, a.p("list"), a.p("data"), a.i("kind")).into(),
        "SI_stateGlData" => g::state_gl_data(m, a.p("list"), a.p("data")).into(),
        "graflistCheckUniqueName" => g::graflist_check_unique_name(m, a.p("list"), a.p("name"), a.i("kind")).into(),
        "SI_itemGetByName" => g::item_get_by_name(m, a.p("list"), a.p("name"), a.i("kind")).into(),
        "SI_grafnodeGetByName" => g::grafnode_get_by_name(m, a.p("list"), a.p("name"), a.i("kind")).into(),
        "SI_getHitSize" => g::get_hit_size(m, a.p("out")).into(),
        "SI_setHitSize" => g::set_hit_size(m, a.i("size")).into(),
        "SI_clearSelect" => g::clear_select().into(),
        // graflist.c: selection state and display type
        "SI_grafNodeIsSelected" => g::graf_node_is_selected(m, a.p("node")).into(),
        "SI_grafNodeIsWrathSelected" => g::graf_node_is_wrath_selected(m, a.p("node")).into(),
        "SI_graflistCountSelected" => g::graflist_count_selected(m, a.p("list"), a.p("first"), a.s("wrath")).into(),
        "SI_graflistCountSelHierarchies" => g::graflist_count_sel_hierarchies(m, a.p("list"), a.s("wrath")).into(),
        "SI_graflistTypeCount" => g::graflist_type_count(m, a.p("list"), a.i("kind1"), a.i("kind2"), a.p("last"), a.s("wrath")).into(),
        "graflistAllVisible" => g::graflist_all_visible(m, a.p("list")).into(),
        "SI_graflistUnghostAll" => g::graflist_unghost_all(m, a.p("list")).into(),
        "SI_graflistToggleGhostSelection" => g::graflist_toggle_ghost_selection(m, a.p("list")).into(),
        "SI_graflistGhostUnselected" => g::graflist_ghost_unselected(m, a.p("list")).into(),
        "SI_graflistGhostHidden" => g::graflist_ghost_hidden(m, a.p("list")).into(),
        "SI_graflistSetForceWireframeAll" => g::graflist_set_force_wireframe_all(m, a.p("list")).into(),
        "SI_graflistResetForceWireframeAll" => g::graflist_reset_force_wireframe_all(m, a.p("list")).into(),
        "SI_graflistToggleForceWireframeSelected" => g::graflist_toggle_force_wireframe_selected(m, a.p("list")).into(),
        "SI_graflistUnselectAll" => g::graflist_unselect_all(m, o, a.p("list")).into(),
        "SI_graflistSetMultiSelection" => g::graflist_set_multi_selection(m, o, a.p("list"), a.s("multi")).into(),
        "SI_unboundGlAll" => g::unbound_gl_all(m, o, a.p("list")).into(),
        "SI_boundGlAll" => g::bound_gl_all(m, o, a.p("list")).into(),
        "instanceComputeBoundingBox" => g::instance_compute_bounding_box(m, o, a.p("model"), a.p("father")).into(),
        "SI_toggleBoundGlSelected" => g::toggle_bound_gl_selected(m, o, a.p("list")).into(),
        "SI_unhideGlAll" => g::unhide_gl_all(m, o, a.p("list")).into(),
        "SI_hideGlUnselected" => g::hide_gl_unselected(m, o, a.p("list")).into(),
        "SI_setSelectability" => g::set_selectability(m, o, a.p("list"), a.s("mode")).into(),
        // graflist.c: selecting
        "SI_unselectGrafnode" => g::unselect_grafnode(m, o, a.p("list"), a.p("node")).into(),
        "SI_selectGrafnode" => g::select_grafnode(m, o, a.p("list"), a.p("node"), a.i("mode")).into(),
        "SI_graflistSelectData" => g::graflist_select_data(m, o, a.p("list"), a.p("data"), a.i("mode")).into(),
        "SI_toggleGrafnode" => g::toggle_grafnode(m, o, a.p("list"), a.p("node"), a.i("mode")).into(),
        "SI_toggleHideGlSelected" => g::toggle_hide_gl_selected(m, o, a.p("list"), a.s("keep")).into(),
        "SI_hideGlGhosted" => g::hide_gl_ghosted(m, o, a.p("list")).into(),
        // graflist.c: the selection commands
        "SI_allSelect" => g::all_select(m, o, a.p("event")).into(),
        "SI_clearSelection" => g::clear_selection(m, o, a.p("event")).into(),
        "SI_graflistCheckMulti" => g::graflist_check_multi(m, o).into(),
        "SI_toggleSelect" => g::toggle_select(m, o, a.p("event")).into(),
        "SI_myclearSelect" => g::my_clear_select(m, o).into(),
        "SI_graflistDeleteSelection" => g::graflist_delete_selection(m, o, a.p("list"), a.p("node")).into(),
        // graflist.c: taking an object out
        "pathRelaxFromData" => g::path_relax_from_data(m, o, a.p("node")).into(),
        "SI_grafnodeDispose" => g::grafnode_dispose(m, o, a.p("pnode")).into(),
        "sceneGraflistRemoveFcurves" => g::scene_graflist_remove_fcurves(m, o, a.p("node")).into(),
        "grafnodeDeleteAnimation" => g::grafnode_delete_animation(m, o, a.p("node")).into(),
        "grafnodeDeleteExpressions" => g::grafnode_delete_expressions(m, o, a.p("node")).into(),
        "SI_grafnodeCutConnections" => g::grafnode_cut_connections(m, o, a.p("node")).into(),
        "modelUnlinkAllLight" => g::model_unlink_all_light(m, o, a.p("model")).into(),
        // graflist.c: putting an object in, hierarchy changes
        "SI_graflistAddData" => g::graflist_add_data(m, o, a.p("list"), a.p("data"), a.i("kind"), a.s("select")).into(),
        "SI_graflistCutModel" => g::graflist_cut_model(m, o, a.p("list"), a.p("node"), a.p("father"), a.s("flag")).into(),
        "SI_graflistLinkModel" => g::graflist_link_model(m, o, a.p("list"), a.p("parent"), a.p("child"), a.s("select"), a.s("flag")).into(),
        "deleteQStretchCtrls" => g::delete_q_stretch_ctrls(m, o, a.p("list"), a.p("model")).into(),
        "SI_graflistDeleteGrafnode" => g::graflist_delete_grafnode(m, o, a.p("list"), a.p("node")).into(),
        // graflist.c: the Delete command, emptying a scene
        "SI_graflistDeleteNode" => g::graflist_delete_node(m, o, a.p("list"), a.p("node"), a.s("branch")).into(),
        "graflistDelete" => g::graflist_delete(m, o, a.p("list")).into(),
        "removeChapterData" => g::remove_chapter_data(m, o, a.p("chapter"), a.i("chap")).into(),
        "SI_sceneDeleteAllNotInHistory" => g::scene_delete_all_not_in_history(m, o).into(),
        // graflist.c: the status line, the camera distance
        "SI_selectedString" => g::selected_string(m, o, a.p("prefix")).into(),
        "SI_vectorDistancectCam" => Ret::St0(g::vector_distance_ct_cam(m, o, a.p("cam"), a.p("point"))),
        // db2scn.c: a loaded scene's elements enter the list
        "grafnodeElemAssoc" => d::grafnode_elem_assoc(m, a.p("node"), a.p("elem"), a.s("copy")).into(),
        "elemdictCompleteHierarchy" => d::elemdict_complete_hierarchy(m, o, a.p("node"), a.i("index")).into(),
        "SI_graflistBuildFromElemDict" => d::graflist_build_from_elem_dict(m, o, a.p("ed"), a.p("rd"), a.p("cids"), a.s("loaded")).into(),
        // nselections.c
        "nselectionCount" => n::nselection_count(m, a.s("selected"), a.p("last")).into(),
        "SI_nselectionAllocate" => n::nselection_allocate(m, a.p("name"), a.i("type")).into(),
        "SI_nselectionCreate" => n::nselection_create(m, o, a.p("name"), a.i("type")).into(),
        "nselectionDispose" => n::nselection_dispose(m, o, a.p("pp")).into(),
        "nselectionReplace" => n::nselection_replace(m, o, a.p("node"), a.s("items"), a.s("clusters"), a.s("curves"), a.s("created")).into(),
        "SI_nselectionSelectContent" => n::nselection_select_content(m, o, a.p("node"), a.i("oper")).into(),
        "tagValid" => n::tag_valid(m, a.i("index")).into(),
        "SI_tagMdlUpdate" => n::tag_mdl_update(m).into(),
        "SI_tagMdlRm" => n::tag_mdl_rm(m).into(),
        "SI_nselectionUpdate" => n::nselection_update(m, o, a.p("ed")).into(),
        // func.c
        "SI_updateGnFunc" => f::update_gn_func(m, o, a.p("node")).into(),
        // graphsort.c
        "si_isPrimaryMatterRelation" => s::is_primary_matter_relation(m, o, a.p("ed"), a.p("elem"), a.p("parent")).into(),
        "getPrimaryMatterRelationParent" => s::get_primary_matter_relation_parent(m, o, a.p("ed"), a.p("elem"), a.p("chap")).into(),
        "schemElemRelFlag" => s::schem_elem_rel_flag(m, o, a.p("ed"), a.p("elem"), a.p("rels"), a.s("backward"), a.s("relative")).into(),
        "si_schemElemFlag" => s::schem_elem_flag(m, o, a.p("ed"), a.p("parent"), a.p("elem"), a.s("relative")).into(),
        "elemRelGraphSortInit" => s::elem_rel_graph_sort_init(m, o, a.p("ed"), a.p("elem"), a.p("rels"), a.s("backward")).into(),
        "elemGraphSortInit" => s::elem_graph_sort_init(m, o, a.p("ed"), a.p("elem"), a.p("parent")).into(),
        "relGraphSortInitElement" => s::rel_graph_sort_init_element(m, o, a.p("ed"), a.p("elem"), a.i("chap"), a.s("reset"), a.s("deep")).into(),
        "relGraphSortInitChapter" => s::rel_graph_sort_init_chapter(m, o, a.p("ed"), a.p("chapter"), a.i("chap"), a.s("reset"), a.s("deep"), a.s("effects_only")).into(),
        "si_schemSortInit" => s::schem_sort_init(m, o, a.p("ed"), a.s("reset"), a.s("deep")).into(),
        "si_schemResetUserPos" => s::schem_reset_user_pos(m, a.p("ed")).into(),
        "schemCompareElems" => s::schem_compare_elems(m, a.p("a"), a.p("b")).into(),
        // hidden.c
        "vctXMat" => hg::vct_x_mat(m, o, a.v("v"), a.v("mat"), a.v("out")).into(),
        "triNrmLong" => hg::tri_nrm_long(m, a.v("p1"), a.v("p2"), a.v("p3"), a.v("out")).into(),
        "triangleOrgNrm" => hg::triangle_org_nrm(m, a.v("p1"), a.v("p2"), a.v("p3"), a.v("out")).into(),
        "edgeIntersection" => hg::edge_intersection(m, a.v("p1"), a.v("p2"), a.v("p3"), a.v("p4"), a.i("idx"), a.v("oa"), a.v("ob")).into(),
        "edgeTestVertices" => hg::edge_test_vertices(m, a.v("p1"), a.v("p2"), a.v("p3"), a.v("p4"), a.v("p5"), a.v("p6"), a.f64("n0"), a.f64("n1"), a.f64("n2")).into(),
        "edgInterTri2" => hg::edg_inter_tri2(m, o, a.v("p1"), a.v("p2"), a.v("p3"), a.v("p4"), a.v("p5"), a.v("p6"), a.v("out")).into(),
        "edgeFreeMem" => hl::edge_free_mem(m, a.v("p")).into(),
        "allFree" => hl::all_free(m, o).into(),
        "inlistStoreTriangle" => hl::inlist_store_triangle(m, o, a.v("list"), a.i("tri")).into(),
        "insertVector" => hl::insert_vector(m, o, a.v("list"), a.i("x"), a.i("y"), a.i("z")).into(),
        "insertOrgVector" => hl::insert_org_vector(m, o, a.v("list"), a.f32("x"), a.f32("y"), a.f32("z")).into(),
        "insertEdge__FP11hidEdgeListllllP9edge_infoi" => hl::insert_edge(m, o, a.v("list"), a.i("a"), a.i("b"), a.i("c"), a.i("d"), a.v("info"), a.i("kind")).into(),
        "lookInsertEdge" => he::look_insert_edge(m, o, a.i("v1"), a.i("v2"), a.i("t1"), a.i("t2"), a.v("table"), a.i("base"), a.i("flags")).into(),
        "edgeIntersectTriangle" => he::edge_intersect_triangle(m, o, a.v("p1"), a.v("p2"), a.v("p3"), a.v("p4"), a.v("p5"), a.v("p6"), a.v("o7"), a.v("o8"), a.v("o9")).into(),
        "intritreeStoreTriangle" => ht::intritree_store_triangle(m, o, a.i("a"), a.i("b"), a.i("c")).into(),
        "inedgtreeStoreTriangle" => ht::inedgtree_store_triangle(m, a.v("head"), a.i("tri")).into(),
        "xferList" => ht::xfer_list(m, a.v("list"), a.v("head"), a.i("skip1"), a.i("skip2")).into(),
        _ => return None,
    })
}
