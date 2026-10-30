// SPDX-License-Identifier: MIT
//! From a vector file's routine name to the port: one line per function.

use crate::{Args, Ret};
use si_deform::deformation as d;
use si_deform::fitting as f;
use si_deform::klusters as k;
use si_deform::lattice as l;
use si_deform::outside::Outside;
use si_mem::Mem;

pub fn run(name: &str, m: &mut Mem, o: &mut dyn Outside, a: &Args) -> Option<Ret> {
    Some(match name {
        // lattice.c
        "SI_isLatticeDeformed" => l::is_lattice_deformed(m, a.p("model")).into(),
        "latticeAllocate" => l::lattice_allocate(m).into(),
        "SI_latticeInitialize" => l::lattice_initialize(m, a.s("nx"), a.s("ny"), a.s("nz"), a.i3("interp"), a.v3("min"), a.v3("max")).into(),
        "SI_latticeReset" => l::lattice_reset(m, a.p("lat")).into(),
        "SI_latticeCopy" => l::lattice_copy(m, a.p("lat")).into(),
        "SI_latticeCopyFromSetup" => l::lattice_copy_from_setup(m, o, a.p("src"), a.p("dst")).into(),
        "SI_latticeDispose" => l::lattice_dispose(m, a.p("slot")).into(),
        "latticeValueTangent" => l::lattice_value_tangent(m, a.p("lat"), a.i("j"), a.i("k"), a.p("in"), a.p("out"), a.p("cs"), a.s("axis")).into(),
        "SI_latticeValue" => l::lattice_value(m, a.p("lat"), a.s("j"), a.s("k"), a.p("in"), a.p("out"), a.s("axis")).into(),
        "latticeVirtualKey" => l::lattice_virtual_key(m, a.p("lat"), a.i("i"), a.i("j"), a.i("k"), a.p("out")).into(),
        "SI_latticeRandomize" => l::lattice_randomize(m, o, a.p("lat"), a.p("amount"), a.s("times"), a.s("tagged")).into(),
        "SI_latticeInterpolate" => l::lattice_interpolate(m, a.p("dst"), a.f("t"), a.p("a"), a.p("b")).into(),
        "SI_latticeTransform" => l::lattice_transform(m, o, a.p("lat"), a.i("kind"), a.v("src"), a.v("dst")).into(),
        "SI_modelLattice" => l::model_lattice(m, a.p("model"), a.p("which")).into(),
        "SI_modelNodeLattice" => l::model_node_lattice(m, a.p("model")).into(),
        "SI_modelTreeLattice" => l::model_tree_lattice(m, a.p("model")).into(),
        "SI_modelDeleteLatticeAnimation" => l::model_delete_lattice_animation(m, o, a.p("model"), a.s("which")).into(),
        "SI_modelDeleteILattice" => l::model_delete_i_lattice(m, a.p("model"), a.s("which"), a.s("index")).into(),
        "SI_modelSwapLattice" => l::model_swap_lattice(m, o, a.p("model"), a.s("which"), a.s("count"), a.p("other")).into(),
        "SI_latticeTransfer" => l::lattice_transfer(m, a.p("from"), a.p("to")).into(),
        // fitting.c
        "SI_fittingAllocate" => f::fitting_allocate(m).into(),
        "fittingCopy" => f::fitting_copy(m, o, a.p("fit")).into(),
        "SI_fittingUpdateMatrix" => f::fitting_update_matrix(m, a.p("fit")).into(),
        "SI_fittingDispose" => f::fitting_dispose(m, o, a.p("model"), a.p("slot"), a.s("which")).into(),
        "SI_pfittingAllocate" => f::pfitting_allocate(m).into(),
        "pfittingCopy" => f::pfitting_copy(m, a.p("fit")).into(),
        "SI_pfittingUpdateMatrix" => f::pfitting_update_matrix(m, a.p("fit")).into(),
        "SI_pfittingDispose" => f::pfitting_dispose(m, o, a.p("model"), a.p("slot"), a.s("which")).into(),
        "SI_patchRemoveFitting" => f::patch_remove_fitting(m, o, a.p("model")).into(),
        "si_deformationCopyFromSetup" => f::deformation_copy_from_setup(m, o, a.p("src"), a.p("dst")).into(),
        // deformation.c
        "SI_postAnimTouch" => d::post_anim_touch(m, o, a.p("model"), a.s("branch")).into(),
        "SI_curveDistanceToValueFrenet" => d::curve_distance_to_value_frenet(m, o, a.v("fit"), a.f("distance"), a.v("point"), a.v("frames"), a.v("out")).into(),
        // klusters.c, klusters_b.c
        "SI_klusterAllocate" => k::kluster_allocate(m).into(),
        "SI_klusterGetTrueIndex" => k::kluster_get_true_index(m, a.p("model"), a.p("k"), a.i("i")).into(),
        "klusterGetCOG" => k::kluster_get_cog(m, o, a.v("elem"), a.p("k"), a.p("out")).into(),
        "SI_klusterAddToList" => k::kluster_add_to_list(m, o, a.p("model"), a.v("name"), a.s("register")).into(),
        "klusterAllocateIndexes" => k::kluster_allocate_indexes(m, a.i("kind"), a.i("n")).into(),
        "klusterCenterRelationDelete" => k::kluster_center_relation_delete(m, o, a.p("model"), a.p("k")).into(),
        "klusterDispose" => k::kluster_dispose(m, o, a.p("model"), a.p("k"), a.s("full")).into(),
        "SI_klusterDisposeAll" => k::kluster_dispose_all(m, o, a.p("model")).into(),
        "klusterGetIndexes" => k::kluster_get_indexes(m, o, a.p("model"), a.v("out"), a.s("count_only")).into(),
        "klusterGetDistinctName" => k::kluster_get_distinct_name(m, a.p("model")).into(),
        "SI_klusterGetByName" => k::kluster_get_by_name(m, a.p("model"), a.v("name")).into(),
        "klusterGetByNumber" => k::kluster_get_by_number(m, a.p("model"), a.i("n")).into(),
        "klusterTagCVs" => k::kluster_tag_cvs(m, a.p("model"), a.p("k"), a.i("def")).into(),
        "SI_klusterGetActive" => k::kluster_get_active(m, a.p("model")).into(),
        "SI_klusterActivate" => k::kluster_activate(m, o, a.p("model"), a.p("k"), a.s("tag"), a.s("clear")).into(),
        "klusterActivateByName" => k::kluster_activate_by_name(m, o, a.p("model"), a.v("name")).into(),
        "SI_klusterDisposeFlagged" => k::kluster_dispose_flagged(m, o, a.p("model")).into(),
        "klusterSetToDispose" => k::kluster_set_to_dispose(m, a.p("k"), a.s("anim_only")).into(),
        "klusterResetDisposeFlags" => k::kluster_reset_dispose_flags(m, a.p("model")).into(),
        "klusterCompare" => k::kluster_compare(m, o, a.p("model"), a.p("k"), a.v("old"), a.v("new"), a.i("n_old"), a.i("n_new")).into(),
        "klusterAddToModel" => k::kluster_add_to_model(m, o, a.p("model"), a.v("name"), a.s("create")).into(),
        "modelTagCount" => k::model_tag_count(m, o, a.p("model")).into(),
        "SI_klusterCnsSetData" => k::kluster_cns_set_data(m, a.p("model"), a.p("centre"), a.p("out")).into(),
        "SI_isClusterNameUnique" => k::is_cluster_name_unique(m, a.p("model"), a.p("k"), a.v("name")).into(),
        "SI_modelklusterListDuplicate" => k::model_kluster_list_duplicate(m, o, a.p("model"), a.s("mode"), a.s("register"), a.s("anim")).into(),
        "SI_klusterListMerge" => k::kluster_list_merge(m, o, a.p("a"), a.p("b")).into(),
        "SI_klusterListAssignToModel" => k::kluster_list_assign_to_model(m, o, a.p("model"), a.p("list")).into(),
        "klusterRemoveIndex" => k::kluster_remove_index(m, o, a.p("model"), a.p("k"), a.v("ud"), a.s("index")).into(),
        "klusterSortIndexes" => k::kluster_sort_indexes(m, o, a.p("model"), a.p("k"), a.v("ud")).into(),
        "klusterMoveIndexes" => k::kluster_move_indexes(m, o, a.p("model"), a.p("k"), a.v("ud"), a.v("map")).into(),
        "SI_klusterPolygonMoveIndexes" => k::kluster_polygon_move_indexes(m, o, a.p("model"), a.v("map")).into(),
        "SI_klusterEdgeMoveIndexes" => k::kluster_edge_move_indexes(m, o, a.p("model"), a.v("map")).into(),
        "SI_klusterMoveIndexes" => k::kluster_move_all_indexes(m, o, a.p("model"), a.v("map")).into(),
        "SI_klusterDisposeEmpty" => k::kluster_dispose_empty(m, o, a.p("model"), a.s("full")).into(),
        "SI_modelKlusterResetCluInd" => k::model_kluster_reset_clu_ind(m, a.p("model")).into(),
        "SI_modelklusterListCvtToFace" => k::model_kluster_list_cvt_to_face(m, o, a.p("model")).into(),
        "SI_clustersApplyTransformation" => k::clusters_apply_transformation(m, o, a.p("model"), a.v("matrix")).into(),
        "SI_klustersAddVector" => k::klusters_add_vector(m, a.p("k"), a.v3("v")).into(),
        "SI_klusterCountVertexRef" => k::kluster_count_vertex_ref(m, o, a.p("model"), a.i("vertex"), a.s("by_centre")).into(),
        "klsEqual" => k::kls_equal(m, a.p("a"), a.p("b"), a.i("kind")).into(),
        "SI_findCluster" => k::find_cluster(m, a.p("k"), a.p("list"), a.p("model")).into(),
        "SI_clusterCount" => k::cluster_count(m, a.p("model")).into(),
        _ => return None,
    })
}
