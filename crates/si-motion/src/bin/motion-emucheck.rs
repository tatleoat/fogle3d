// SPDX-License-Identifier: MIT
//! Compares the ports of `si-motion` with the original routines as the emulation harness
//! ran them (`re/tools/emu`, finding 0011; vectors made by `re/tools/si-motion/emu_gen.py`
//! and its successors).
//!
//!     motion-emucheck <directory with one sub-directory of .vec files per routine>
//!
//! A case is a set of named buffers (an update node, its arguments, curves with their keys,
//! the object, globals at their addresses in the binary), the routine's arguments, and what
//! the original left: every buffer again, the calls it made into parts of the program that
//! the harness replaced by recorded stubs, and its result. The check loads the buffers into
//! the shared memory image (`si_mem::Mem`, finding 0024) **at the addresses they had in the
//! emulator** (the vectors carry them as `a:<buffer>`; `si_mem::load_case` lays a case out
//! as the harness does, and the check holds its placement against those addresses), runs
//! the port, and requires every word of every buffer, the sequence of calls with their
//! argument words, and the result to be the same bits.
//!
//! The image's heap is the harness's heap (same base, same headers, same order), so a block
//! the port allocates lies where the original's lay whenever the port made the original's
//! allocation calls in the original's order. The check counts how often that holds and
//! prints the count behind the table; a block that lies elsewhere is still compared, through
//! the pairing of the two sides' pointers.
//!
//! `EMUCHECK_MUTATE=1` flips one bit of the first byte the port changed in the image (and of
//! the value it returned) before the comparison: every case in which the port changes or
//! returns something the vectors hold must then differ. `EMUCHECK_CALLS=1` prints both call
//! sequences beside a difference.

use si_mem::{Addr, Mem, Names, Region, Snapshot, Word as W};
use si_motion::animcopy;
use si_motion::assoc;
use si_motion::fcvoper;
use si_motion::host::{Arg, Ret, Trace};
use si_motion::key;
use si_motion::keyedit;
use si_motion::lattice;
use si_motion::nextkey;
use si_motion::path;
use si_motion::play;
use si_motion::playrun;
use si_motion::reset;
use si_motion::savekey;
use si_motion::savekey2;
use si_motion::update::{self, constr, geom, params, shape, texfile, transfo};
use si_motion::utils;
use si_motion::MemExt;
use si_x87::vectors::{check_dir, check_dir_with, print_report, Case, Check, Verdict, Word};
use si_x87::F80;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How the cases were loaded and where the port's new blocks lay, over the whole run.
static BY_LOAD_CASE: AtomicUsize = AtomicUsize::new(0);
static BY_PLACE_AT: AtomicUsize = AtomicUsize::new(0);
/// Pointers into blocks the routine allocated (words of buffers, arguments of recorded
/// calls) that were compared: each as the number the original had.
static HEAP_PTRS: AtomicUsize = AtomicUsize::new(0);
/// Cases whose vector carries the original's blocks, and those in which the port's heap is
/// the original's block for block ([`whole_heap`]).
static WHOLE_HEAP_CASES: AtomicUsize = AtomicUsize::new(0);
static WHOLE_HEAP_SAME: AtomicUsize = AtomicUsize::new(0);
/// Under `EMUCHECK_MUTATE`: cases in which a bit was flipped.
static MUTATED: AtomicUsize = AtomicUsize::new(0);
/// Of those, the cases whose memory still compared equal.
static SURVIVED: AtomicUsize = AtomicUsize::new(0);

fn mutating() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some()
}

/// The original's name of a recorded stub, as the host traits spell it.
fn host_name(orig: &str) -> &'static str {
    match orig {
        "SI_waveGetIconModel" => "wave_get_icon_model",
        "SI_invKinResetPrefAngle" => "inv_kin_reset_pref_angle",
        "SI_edIsItemConstrained" => "ed_is_item_constrained",
        "SI_invKinSetGoal" => "inv_kin_set_goal",
        "SI_texture2DSetDirty" => "texture2d_set_dirty",
        "SI_updTxt2dEffects" => "upd_txt2d_effects",
        "SI_updTxt3dTurb" => "upd_txt3d_turb",
        "SI_findGlData" => "find_gl_data",
        "SI_modelSetHiddenParam" => "model_set_hidden_param",
        "SI_modelPropagateMatteParam" => "model_propagate_matte_param",
        "SI_elemdictFindSon" => "elemdict_find_son",
        "SI_elemdictFindNextSon" => "elemdict_find_next_son",
        "SI_setAreaRect" => "set_area_rect",
        "SI_setAreaDisk" => "set_area_disk",
        "SI_dbChapter2GrafType" => "db_chapter2graf_type",
        "SI_edGetCamFromCamInt" => "ed_get_cam_from_cam_int",
        "SI_edGetLigFromLigInt" => "ed_get_lig_from_lig_int",
        "SI_modelUpdateSonState" => "model_update_son_state",
        "SI_dopeIsElemActive" => "dope_is_elem_active",
        "SI_elemdictIsElemUsed" => "elemdict_is_elem_used",
        "SI_reldictNbRelForDeactCount" => "reldict_nb_rel_for_deact_count",
        "SI_postAnimTouch" => "post_anim_touch",
        "SI_modelReadjustDefinition" => "model_readjust_definition",
        "SI_meshComputesPolygonNormal" => "mesh_computes_polygon_normal",
        "SI_skeChainRoot" => "ske_chain_root",
        "SI_dynaAlgoTypeSet" => "dyna_algo_type_set",
        "SI_modelSolveInvKin" => "model_solve_inv_kin",
        "SI_dynamicSimulationCompute" => "dynamic_simulation_compute",
        "SI_cusUpdateEffect" => "cus_update_effect",
        "SI_animExpressionEvaluate" => "anim_expression_evaluate",
        "isScriptUpToDate" => "is_script_up_to_date",
        "isSequenceUpToDate" => "is_sequence_up_to_date",
        "si_animModelConstraintUpdate" => "anim_model_constraint_update",
        "si_animModelPlaneConstrUpdate" => "anim_model_plane_constr_update",
        "si_animModelSymConstrUpdate" => "anim_model_sym_constr_update",
        "animCns3PtsUpdate" => "anim_cns_3pts_update",
        "animCns2PtsUpdate" => "anim_cns_2pts_update",
        "animModelObjPntCnsUpdate" => "anim_model_obj_pnt_cns_update",
        "animTrajectoryUpdate" => "anim_trajectory_update",
        "SI_camMat" => "cam_mat",
        "SI_elemdictFindElemByData" => "elemdict_find_elem_by_data",
        "SI_modelKineStateCompute" => "model_kine_state_compute",
        "SI_animBooleanUpdate" => "anim_boolean_update",
        "SI_extractionUpdate" => "extraction_update",
        "SI_revolutionUpdate" => "revolution_update",
        "SI_extrusionUpdate" => "extrusion_update",
        "SI_fourSidedUpdate" => "four_sided_update",
        "SI_skinningUpdate" => "skinning_update",
        "SI_zipUpdate" => "zip_update",
        "SI_trimUpdate" => "trim_update",
        "SI_inverseUpdate" => "inverse_update",
        "animShapeUpdate" => "anim_shape_update",
        "animClustersUpdate" => "anim_clusters_update",
        "animSplineDefUpdate" => "anim_spline_def_update",
        "animPatchDefUpdate" => "anim_patch_def_update",
        "SI_animMdlClusterCnsUpdate" => "anim_mdl_cluster_cns_update",
        "SI_edGetElemFromLattice" => "ed_get_elem_from_lattice",
        "SI_latticeInterpolate" => "lattice_interpolate",
        "VCCreatePrim" => "vc_create_prim",
        "VCSetPrimParam" => "vc_set_prim_param",
        "VCGetPrimParam" => "vc_get_prim_param",
        "VCDeletePrim" => "vc_delete_prim",
        "SI_metaballTouch" => "metaball_touch",
        "SI_envelopDispDef" => "envelop_disp_def",
        "SI_envelopCopyDef" => "envelop_copy_def",
        "SI_envelopCompUpdate" => "envelop_comp_update",
        "SI_envelopSkelRoot" => "envelop_skel_root",
        "SI_envSkelSetFlag" => "env_skel_set_flag",
        "SI_envelopNeedUpdate" => "envelop_need_update",
        "SI_modelLattice" => "model_lattice",
        "SI_modelPatchFitting" => "model_patch_fitting",
        "SI_modelSplineFitting" => "model_spline_fitting",
        "SI_modelWave" => "model_wave",
        "SI_quickStretchActive" => "quick_stretch_active",
        "SI_modelHasConstrainedClusters" => "model_has_constrained_clusters",
        "SI_lightSunGetPosition" => "light_sun_get_position",
        "SI_splineLength" => "spline_length",
        "SI_splineDistanceToFactor" => "spline_distance_to_factor",
        "SI_splineValue" => "spline_value",
        "SI_nurbsCurveLength" => "nurbs_curve_length",
        "SI_nurbsDistanceToFactor" => "nurbs_distance_to_factor",
        "SI_nurbsCrvValue" => "nurbs_crv_value",
        "si_tanConstrCreateData" => "tan_constr_create_data",
        "SI_edGetElemFromCamera" => "ed_get_elem_from_camera",
        "SI_edGetElemFromLight" => "ed_get_elem_from_light",
        "SI_splineUpdateLength" => "spline_update_length",
        "SI_splineNbSegment" => "spline_nb_segment",
        "SI_nurbsCurveUpdateLength" => "nurbs_curve_update_length",
        "SI_nurbsCrvIndexHit" => "nurbs_crv_index_hit",
        "SI_bezierMovePoint" => "bezier_move_point",
        "SI_splineChangePoint" => "spline_change_point",
        "SI_splineAddPoint" => "spline_add_point",
        "SI_nurbsCrvAddPoint" => "nurbs_crv_add_point",
        "SI_splineFactorToDistance" => "spline_factor_to_distance",
        "SI_nurbsFactorToDistance" => "nurbs_factor_to_distance",
        "SI_dbGrafType2Chapter" => "db_graf_type2chapter",
        "SI_elemdictAddRelation" => "elemdict_add_relation",
        "SI_edFcvDispose" => "ed_fcv_dispose",
        "SI_elemdictRemoveRelationId" => "elemdict_remove_relation_id",
        "SI_statusDisplayMessage" => "status_display_message",
        "SI_unselectGrafnode" => "unselect_grafnode",
        "SI_nextGrafnode" => "next_grafnode",
        "SI_edElementSetConstraint" => "ed_element_set_constraint",
        "si_relationActiveRangeSetKey" => "relation_active_range_set_key",
        "SI_elemdictGetRelation" => "elemdict_get_relation",
        "SI_splineAllocate" => "spline_allocate",
        "SI_splineUpdateMatrix" => "spline_update_matrix",
        "SI_allDummy" => "all_dummy",
        "SI_modelAllocate" => "model_allocate",
        "SI_modelAddDefinition" => "model_add_definition",
        "SI_graflistAddData" => "graflist_add_data",
        "si_tanConstrUpdTanData" => "tan_constr_upd_tan_data",
        "SI_modelSplineFittingReset" => "model_spline_fitting_reset",
        "SI_modelPatchFittingReset" => "model_patch_fitting_reset",
        "SI_edModelRelaxFitting" => "ed_model_relax_fitting",
        "SI_statusDisplayPrompt" => "status_display_prompt",
        "SI_statusDisplayMouse" => "status_display_mouse",
        "SI_sceneUpdate" => "scene_update",
        "SI_audioContainerPlayFrame" => "audio_container_play_frame",
        "SI_audioPlaySetMode" => "audio_play_set_mode",
        "SI_audioContainerPlayAbort" => "audio_container_play_abort",
        "winget" => "winget",
        "winset" => "winset",
        "getdrawmode" => "getdrawmode",
        "drawmode" => "drawmode",
        "SI_fastGetDirty" => "fast_get_dirty",
        "SI_windowGetNextVisible" => "window_get_next_visible",
        "SI_drawGrid" => "draw_grid",
        "SI_refreshSelectedFcurve" => "refresh_selected_fcurve",
        "si_postscriptDumpFrame" => "postscript_dump_frame",
        "SI_viewerUpdateScene" => "viewer_update_scene",
        "SI_dialogUpdateFcv" => "dialog_update_fcv",
        "SI_mouseInterrupt" => "mouse_interrupt",
        "SI_fsliderDraw" => "fslider_draw",
        "SI_refreshSystem" => "refresh_system",
        "times" => "times",
        "SI_reldictGetRelChapGrp" => "reldict_get_rel_chap_grp",
        "SI_relGetFcvType" => "rel_get_fcv_type",
        "SI_edFcvAdd" => "ed_fcv_add",
        "SI_latticeTouch" => "lattice_touch",
        "SI_latticeCopy" => "lattice_copy",
        "SI_latticeTransfer" => "lattice_transfer",
        "SI_modelAddILattice" => "model_add_i_lattice",
        "SI_modelDeleteILattice" => "model_delete_i_lattice",
        "SI_dialogRequest3" => "dialog_request3",
        "SI_attdictGetFcurveFlag" => "attdict_get_fcurve_flag",
        "SI_elemdictRemoveBackwardRelId" => "elemdict_remove_backward_rel_id",
        "SI_elemdictSetDeleteElem" => "elemdict_set_delete_elem",
        "SI_fcvGetRelInfo" => "fcv_get_rel_info",
        "si_fcvScnglDelete" => "fcv_scngl_delete",
        "SI_fastSetDirty" => "fast_set_dirty",
        "SI_modelDeleteIDefinition" => "model_delete_i_definition",
        "SI_edDisposeShape" => "ed_dispose_shape",
        "si_modelDeleteAllClusterAnim" => "model_delete_all_cluster_anim",
        "SI_modelDeleteClusterAnim" => "model_delete_cluster_anim",
        "SI_waveInitDecay" => "wave_init_decay",
        "SI_waveInitSpread" => "wave_init_spread",
        "SI_fcvScrollBuild" => "fcv_scroll_build",
        "SI_sceneKeySetColor" => "scene_key_set_color",
        "SI_setFcvTableEntriesFromChap" => "set_fcv_table_entries_from_chap",
        "SI_edDisposeCluster" => "ed_dispose_cluster",
        "SI_edDisposeLattice" => "ed_dispose_lattice",
        "SI_edFcvFilterDispose" => "ed_fcv_filter_dispose",
        "SI_modelDeleteLatticeAnimation" => "model_delete_lattice_animation",
        "si_shapeWeightFcurveDispose" => "shape_weight_fcurve_dispose",
        "si_resetShapeWeightFcurve" => "reset_shape_weight_fcurve",
        "SI_modelRemoveLattice" => "model_remove_lattice",
        "SI_klusterDisposeAll" => "kluster_dispose_all",
        "SI_clusterDispose" => "cluster_dispose",
        "SI_modelUnlinkAllWaves" => "model_unlink_all_waves",
        "SI_edCheckAnimFileDispose" => "ed_check_anim_file_dispose",
        "si_setFcvTableFromMenu" => "set_fcv_table_from_menu",
        "si_getDicInfoFromGrafnode" => "get_dic_info_from_grafnode",
        "SI_reldictCheckRelAnim" => "reldict_check_rel_anim",
        "SI_attdictSetFcurveFlag" => "attdict_set_fcurve_flag",
        "SI_edGetElemFromWave" => "ed_get_elem_from_wave",
        "SI_dialogRequest2" => "dialog_request2",
        "si_setExplicitPathTiming" => "set_explicit_path_timing",
        "si_pathTimeDialogBox" => "path_time_dialog_box",
        "si_pathActionDialogBox" => "path_action_dialog_box",
        "SI_elemdictRemoveUncompleteRelPair" => "elemdict_remove_uncomplete_rel_pair",
        "SI_actionsGetNewName" => "actions_get_new_name",
        "SI_actionsAddElem" => "actions_add_elem",
        "SI_actionsSecToStartFrm" => "actions_sec_to_start_frm",
        "SI_actionsSecToEndFrm" => "actions_sec_to_end_frm",
        "SI_edGetActions" => "ed_get_actions",
        "SI_elemdictGetElemFromData" => "elemdict_get_elem_from_data",
        "SI_dialogRequest" => "dialog_request",
        "SI_setDopFcvTableFromDopeMode" => "set_dop_fcv_table_from_dope_mode",
        "SI_elemdictFindElemFromGrafNode" => "elemdict_find_elem_from_graf_node",
        "SI_getWaveFcurvePointer" => "get_wave_fcurve_pointer",
        // third pass
        "SI_modelSetGlobalTranslation" => "model_set_global_translation",
        "SI_modelSetGlobalRotation" => "model_set_global_rotation",
        "SI_modelSetGlobalScaling" => "model_set_global_scaling",
        "SI_modelSetRotation" => "model_set_rotation",
        "SI_modelComputeBounding" => "model_compute_bounding",
        "SI_elemGetVertex" => "elem_get_vertex",
        "SI_elemSetVertex" => "elem_set_vertex",
        "SI_modelGetData" => "model_get_data",
        "SI_modelGetVertexPtr" => "model_get_vertex_ptr",
        "SI_modelCheckCreateDefinition" => "model_check_create_definition",
        "SI_modelTouchGeometry" => "model_touch_geometry",
        "SI_klusterGetTrueIndex" => "kluster_get_true_index",
        "SI_klusterCountVertexRef" => "kluster_count_vertex_ref",
        "si_rotlimConstraint" => "rotlim_constraint",
        "si_poslimConstraint" => "poslim_constraint",
        "si_tanVectorFromPath" => "tan_vector_from_path",
        "si_tanConstrComputeTanVector" => "tan_constr_compute_tan_vector",
        "si_tanConstrMapDirection" => "tan_constr_map_direction",
        "si_norConstrComputeNormalVec" => "nor_constr_compute_normal_vec",
        "si_norConstrMapDirection" => "nor_constr_map_direction",
        "si_constr3PtsMapDirection" => "constr3pts_map_direction",
        "si_animModelBoundPlaneUpdate" => "anim_model_bound_plane_update",
        "si_animModelPivotPlaneUpdate" => "anim_model_pivot_plane_update",
        "si_animPivotConstrInit" => "anim_pivot_constr_init",
        "SI_invKinSetCnsPosition" => "inv_kin_set_cns_position",
        "si_symmetryVector" => "symmetry_vector",
        "SI_edIsElementAnimated" => "ed_is_element_animated",
        "si_getExplPathFirstFrameFromRelation" => "get_expl_path_first_frame_from_relation",
        "SI_nurbsCrvMovePoint" => "nurbs_crv_move_point",
        "SI_nurbsSrfMovePoint" => "nurbs_srf_move_point",
        "SI_knotsCrvReparametrization" => "knots_crv_reparametrization",
        "SI_knotsSrfReparametrization" => "knots_srf_reparametrization",
        "SI_patchComputeSurface" => "patch_compute_surface",
        "SI_splineTouch" => "spline_touch",
        "SI_edGetElemFromShape" => "ed_get_elem_from_shape",
        "SI_getExprFcurveUsed" => "get_expr_fcurve_used",
        "SI_modelTouchDefinition" => "model_touch_definition",
        "SI_cusTransferToDefinition" => "cus_transfer_to_definition",
        "SI_fittingUpdateMatrix" => "fitting_update_matrix",
        "SI_pfittingUpdateMatrix" => "pfitting_update_matrix",
        "SI_stripPicExtension" => "strip_pic_extension",
        "SI_modelKlusterResetCluInd" => "model_kluster_reset_clu_ind",
        "si_clusterMeshInterpolate" => "cluster_mesh_interpolate",
        "si_clusterPatchInterpolate" => "cluster_patch_interpolate",
        "si_clusterSplineInterpolate" => "cluster_spline_interpolate",
        "si_clusterFaceInterpolate" => "cluster_face_interpolate",
        "si_clusterNurbsFaceInterpolate" => "cluster_nurbs_face_interpolate",
        "si_clusterNurbsCrvInterpolate" => "cluster_nurbs_crv_interpolate",
        "si_clusterNurbsSrfInterpolate" => "cluster_nurbs_srf_interpolate",
        "SI_meshComputeHardEdge" => "mesh_compute_hard_edge",
        "SI_meshComputesVertexNormal" => "mesh_computes_vertex_normal",
        "SI_shapeAddKey" => "shape_add_key",
        "SI_modelCreateParent" => "model_create_parent",
        "SI_savekeyCompensateInfo" => "savekey_compensate_info",
        "SI_modelNextNode" => "model_next_node",
        "SI_modelIsFather" => "model_is_father",
        "SI_splineIsPath" => "spline_is_path",
        "fopen" => "fopen",
        "fgets" => "fgets",
        "fclose" => "fclose",
        "si_patchInterpolate" => "patch_interpolate",
        "si_patchCardinalInterpolate" => "patch_cardinal_interpolate",
        "si_patchWeightInterpolate" => "patch_weight_interpolate",
        "si_faceInterpolate" => "face_interpolate",
        "si_faceCardinalInterpolate" => "face_cardinal_interpolate",
        "si_faceWeightInterpolate" => "face_weight_interpolate",
        "si_meshInterpolate" => "mesh_interpolate",
        "si_meshCardinalInterpolate" => "mesh_cardinal_interpolate",
        "si_meshWeightInterpolate" => "mesh_weight_interpolate",
        "si_splineInterpolate" => "spline_interpolate",
        "si_splineCardinalInterpolate" => "spline_cardinal_interpolate",
        "si_splineWeightInterpolate" => "spline_weight_interpolate",
        "si_nurbsCrvInterpolate" => "nurbs_crv_interpolate",
        "si_nurbsCrvCardinalInterpolate" => "nurbs_crv_cardinal_interpolate",
        "si_nurbsCrvWeightInterpolate" => "nurbs_crv_weight_interpolate",
        "si_nurbsSrfInterpolate" => "nurbs_srf_interpolate",
        "si_nurbsSrfCardinalInterpolate" => "nurbs_srf_cardinal_interpolate",
        "si_nurbsSrfWeightInterpolate" => "nurbs_srf_weight_interpolate",
        // fourth pass
        "SI_dopeFlagsSet" => "dope_flags_set",
        "SI_elemdictSortAnimElements" => "elemdict_sort_anim_elements",
        "SI_edGetElemFromMaterial" => "ed_get_elem_from_material",
        "SI_edGetElemFromTexture2D" => "ed_get_elem_from_texture2d",
        "SI_edGetElemFromScene" => "ed_get_elem_from_scene",
        "SI_edGetElemFromTexture3D" => "ed_get_elem_from_texture3d",
        "SI_displaySunPositionDialog" => "display_sun_position_dialog",
        "SI_patchTransferPoints" => "patch_transfer_points",
        "SI_patchUpdateSurface" => "patch_update_surface",
        "SI_faceTransferPoints" => "face_transfer_points",
        "SI_meshTransferVertices" => "mesh_transfer_vertices",
        "SI_splineTransferPoints" => "spline_transfer_points",
        "SI_nurbsCrvTransferPointsKnots" => "nurbs_crv_transfer_points_knots",
        "SI_nurbsSrfTransferPointsKnots" => "nurbs_srf_transfer_points_knots",
        "SI_reldictGetRelIndexFromId" => "reldict_get_rel_index_from_id",
        "SI_symmetryGetElem" => "symmetry_get_elem",
        "SI_modelSetScaling" => "model_set_scaling",
        "SI_modelSetTranslation" => "model_set_translation",
        // fifth pass
        "SI_windowCamActive" => "window_cam_active",
        "SI_graflistCountSelected" => "graflist_count_selected",
        "SI_viewerGetScene" => "viewer_get_scene",
        "SI_sceneFrameUpdate" => "scene_frame_update",
        "SI_refreshSelectedBothBuffers" => "refresh_selected_both_buffers",
        "SI_fcvScnglAdd" => "fcv_scngl_add",
        "SI_reldictGetRelName" => "reldict_get_rel_name",
        "SI_edGetElemFromCluster" => "ed_get_elem_from_cluster",
        "SI_edGetElemFromAnimFile" => "ed_get_elem_from_anim_file",
        "SI_fcvScnglBDelete" => "call_fcurve_function",
        "SI_elemlistClearWorkFlag" => "elemlist_clear_work_flag",
        "SI_actionsTmpActivateAll" => "actions_tmp_activate_all",
        "SI_actionsOffsetAndScaleDef" => "actions_offset_and_scale_def",
        "SI_shaderFcurveOffsetAndScaleSelected" => "shader_fcurve_offset_and_scale_selected",
        "SI_customFcurveOffsetAndScaleSelected" => "custom_fcurve_offset_and_scale_selected",
        "SI_shaderFcurveOffsetAndScaleAll" => "shader_fcurve_offset_and_scale_all",
        "SI_customFcurveOffsetAndScaleAll" => "custom_fcurve_offset_and_scale_all",
        "SI_dbMRequest2" => "db_m_request2",
        "SI_graflistCountSelHierarchies" => "graflist_count_sel_hierarchies",
        "SI_gLSelect" => "gl_select",
        "SI_elemdictFindElemByAuxData" => "elemdict_find_elem_by_aux_data",
        "SI_foolDisplay" => "fool_display",
        "swapbuffers" => "swapbuffers",
        "usleep" => "usleep",
        "SI_relGetAnimFRelFromChapter" => "rel_get_anim_f_rel_from_chapter",
        "SI_graflistTypeCount" => "graflist_type_count",
        "SI_elemdictSetAnimRelation" => "elemdict_set_anim_relation",
        "SI_edChapterAnimElemChangeName" => "ed_chapter_anim_elem_change_name",
        "SI_actionsUnselectAll" => "actions_unselect_all",
        "SI_hieranimAllocate" => "hieranim_allocate",
        "SI_hieranimDispose" => "hieranim_dispose",
        "SI_hieranimAddElem" => "hieranim_add_elem",
        "SI_hieranimAddItem" => "hieranim_add_item",
        "SI_elemdictRemoveRelationPair" => "elemdict_remove_relation_pair",
        "SI_getFilename" => "get_filename",
        "SI_windowGetHilite" => "window_get_hilite",
        "SI_postscriptRefresh" => "postscript_refresh",
        "SI_windowGetNextActive" => "window_get_next_active",
        "SI_actionsCountSelected" => "actions_count_selected",
        "SI_actionsGetMinStart" => "actions_get_min_start",
        "SI_actionsGetMaxEnd" => "actions_get_max_end",
        "SI_actionsElemGetLength" => "actions_elem_get_length",
        "SI_audioMarkerMngFindFirstActive" => "audio_marker_mng_find_first_active",
        "SI_audioMarkerMngFindLastActive" => "audio_marker_mng_find_last_active",
        "SI_fastPlayClearAllView" => "fast_play_clear_all_view",
        "getbutton" => "getbutton",
        _ => "?",
    }
}

/// Where the buffers of a case lie in the image.
struct Layout {
    addr: HashMap<String, Addr>,
    names: Names,
}

struct Image {
    mem: Mem,
    lay: Layout,
}

/// A word of the vector reader as the image's loader takes it (`si-mem` cannot depend on
/// `si-x87`; finding 0024, section 4.5).
fn word(w: &Word) -> W {
    match w {
        Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
        Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
        Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
    }
}

fn in_region(a: Addr, r: (u32, u32)) -> bool {
    a.wrapping_sub(r.0) < r.1
}

/// True for an address in the binary's `.rodata`, `.data` or `.bss`: a global.
fn is_global(a: Addr) -> bool {
    in_region(a, si_mem::RODATA) || in_region(a, si_mem::DATA) || in_region(a, si_mem::BSS)
}

/// The case's memory: each buffer at the address it had in the emulator.
///
/// The buffers of the harness's arena go through `si_mem::load_case`, in the order of their
/// addresses, which is the order the harness placed them in; that the loader then has every
/// buffer at its recorded address is checked. Should a file be laid out otherwise, each
/// buffer is placed at its recorded address instead (`Mem::place_at`). The globals are
/// stored where the binary has them. The image is strict from then on: an access in the
/// arena or the heap outside a live block is a fault of the port.
fn image(c: &Case) -> Result<Image, String> {
    let mut addr = HashMap::new();
    for f in &c.outputs {
        if let Some(name) = f.name.strip_prefix("a:") {
            addr.insert(name.to_string(), f.words()[0].value().unwrap_or(0) as u32);
        }
    }
    let mut arena: Vec<(&str, Addr, Vec<W>)> = Vec::new();
    let mut globals: Vec<(&str, Addr, Vec<W>)> = Vec::new();
    for f in &c.inputs {
        let Some(&a) = addr.get(&f.name) else { continue };
        // a pointer to a global is a number: resolved here, so that the arena's buffers can
        // be loaded before the globals are named
        let words: Vec<W> = f
            .words()
            .iter()
            .map(|w| match w {
                Word::Ref { name, offset } => match addr.get(name) {
                    Some(&g) if is_global(g) => W::Value { bytes: 4, value: g.wrapping_add(*offset) as u128 },
                    _ => word(w),
                },
                w => word(w),
            })
            .collect();
        if in_region(a, si_mem::ARENA) {
            arena.push((&f.name, a, words));
        } else if is_global(a) {
            globals.push((&f.name, a, words));
        } else {
            return Err(format!("buffer `{}` at {a:#x}: neither a global nor in the harness's arena", f.name));
        }
    }
    arena.sort_by_key(|b| b.1);
    let mut mem = Mem::new();
    let fields: Vec<(&str, Option<&[W]>)> = arena.iter().map(|(n, _, w)| (*n, Some(w.as_slice()))).collect();
    let mut names = match si_mem::load_case(&mut mem, &fields) {
        Ok(names) if arena.iter().all(|(n, a, _)| names.addr(&mem, n) == Some(*a)) => {
            BY_LOAD_CASE.fetch_add(1, Ordering::Relaxed);
            names
        }
        r => {
            BY_PLACE_AT.fetch_add(1, Ordering::Relaxed);
            if std::env::var_os("EMUCHECK_LAYOUT").is_some() {
                match r {
                    Ok(names) => {
                        let bad = arena.iter().find(|(n, a, _)| names.addr(&mem, n) != Some(*a)).unwrap();
                        eprintln!("{}: `{}` recorded at {:#x}, placed at {:#x?}", c.name, bad.0, bad.1, names.addr(&mem, bad.0));
                    }
                    Err(e) => eprintln!("{}: {e}", c.name),
                }
            }
            mem = Mem::new();
            let mut names = Names::new();
            for (n, a, w) in &arena {
                let len = si_mem::words_len(w);
                if !mem.place_at(*a, len) {
                    return Err(format!("buffer `{n}` at {a:#x} does not fit the arena"));
                }
                names.add(n, *a, len);
            }
            for (_, a, w) in &arena {
                mem.load_words(*a, w, &names)?;
            }
            names
        }
    };
    for (n, a, w) in &globals {
        names.add(n, *a, si_mem::words_len(w));
    }
    for (_, a, w) in &globals {
        if in_region(*a, si_mem::RODATA) {
            // a constant of the original: stored as one, the image refuses ordinary writes there
            let mut at = *a;
            for x in w {
                let bytes = match x {
                    W::Value { bytes, value } => value.to_le_bytes()[..*bytes as usize].to_vec(),
                    W::Unwritten { bytes } => vec![0; *bytes as usize],
                    W::Ref { name, offset } => names.addr(&mem, name).ok_or_else(|| format!("no buffer `{name}`"))?.wrapping_add(*offset).to_le_bytes().to_vec(),
                };
                mem.set_rom(at, &bytes);
                at = at.wrapping_add(bytes.len() as u32);
            }
        } else {
            mem.load_words(*a, w, &names)?;
        }
    }
    if let Some(f) = mem.fault() {
        return Err(format!("the case does not fit the image: {f:?}"));
    }
    mem.set_strict(true);
    Ok(Image { mem, lay: Layout { addr, names } })
}

/// Under `EMUCHECK_MUTATE`: the image before the port runs.
fn before(img: &Image) -> Option<Snapshot> {
    mutating().then(|| img.mem.snapshot())
}

/// Under `EMUCHECK_MUTATE`: flips the lowest bit of the first byte the port changed in the
/// globals, in a buffer of the case or inside a block of the heap that is still live (the
/// allocator's own headers and the blocks the port freed do not count: a freed buffer's
/// contents are not compared). Returns whether a byte was flipped.
fn mutate(img: &mut Image, before: Option<Snapshot>) -> bool {
    let Some(s) = before else { return false };
    let changed = img.mem.changed(&s);
    let hit = changed.iter().find_map(|(a, bytes)| {
        (0..bytes.len() as u32).map(|i| a + i).find(|&x| match img.mem.region_of(x) {
            Some(Region::Heap | Region::Arena) => img.mem.in_block(x, 1),
            Some(_) => true,
            None => false,
        })
    });
    let Some(a) = hit else { return false };
    img.mem.set_strict(false);
    let v = img.mem.u8(a);
    img.mem.set_u8(a, v ^ 1);
    img.mem.set_strict(true);
    MUTATED.fetch_add(1, Ordering::Relaxed);
    FLIPPED_AT.with(|f| f.set(a));
    true
}

/// A side effect of a stub, as the generator recorded it behind the call (`@fx(...)`): words
/// written through one of the call's pointer arguments (a local of the caller: the port's
/// own argument is used), or at an address of the case's buffers.
enum Fx {
    Arg { k: usize, off: u32, words: Vec<u32> },
    /// One 16-bit word through a pointer argument.
    Arg16 { k: usize, off: u32, value: u16 },
    Mem { addr: u32, words: Vec<u32> },
    /// Not an effect but an expectation: the C string the original handed over in argument k.
    Text { k: usize, text: Vec<u8> },
    /// An expectation: the words the original handed over behind pointer argument k.
    In { k: usize, words: Vec<u32> },
    /// The stub answered with its own argument k plus an offset: the port gets ITS argument.
    RetArg { k: usize, off: u32 },
    /// The stub wrote its own argument k (a pointer the routine had just made) at an address
    /// of the case: the port's host writes ITS argument k there.
    MemArg { addr: u32, k: usize },
}

thread_local! {
    /// Under `EMUCHECK_MUTATE`: where the bit of the case being compared was flipped (0: nowhere).
    static FLIPPED_AT: std::cell::Cell<Addr> = const { std::cell::Cell::new(0) };
    /// Words of the case's buffers that the original leaves undefined in this case without
    /// the two runs of the harness showing it (a stale value of its own stack): (buffer,
    /// word index). Set by a check, emptied by `compare_but`.
    static UNDEFINED: RefCell<Vec<(&'static str, usize)>> = const { RefCell::new(Vec::new()) };
    /// The first text a port handed to a stub that differs from the original's.
    static TEXT_ERR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The function the generators pass where a routine takes a function pointer
/// (`fcurve *(*)(fcurve *, float)`): the one the program itself passes, and its address.
const FN_POINTER: (&str, u32) = ("SI_fcvScnglBDelete", 0x0823_c8dc);

/// One call the original made into a stub: the host method's name, the stack words, the
/// answer (integer register and x87 register) and the stub's side effects.
struct Step {
    orig: String,
    args: Vec<u32>,
    /// The argument words as the vector has them, for the shared replay tape.
    raw: Vec<u128>,
    in_st0: bool,
    eax: u32,
    st0: F80,
    fx: Vec<Fx>,
}

fn tape(c: &Case) -> Vec<Step> {
    let mut out: Vec<Step> = Vec::new();
    for k in &c.calls {
        let args: Vec<u32> = k.args.iter().map(|w| w.value().unwrap_or(0) as u32).collect();
        if k.name == "fx" {
            let words = args[3..].to_vec();
            let fx = match args[0] {
                1 => Fx::Arg { k: args[1] as usize, off: args[2], words },
                3 => Fx::Arg16 { k: args[1] as usize, off: args[2], value: args[3] as u16 },
                2 => {
                    let mut text: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
                    let n = text.iter().position(|b| *b == 0).unwrap_or(text.len());
                    text.truncate(n);
                    Fx::Text { k: args[1] as usize, text }
                }
                4 => Fx::In { k: args[1] as usize, words },
                5 => Fx::RetArg { k: args[1] as usize, off: args[2] },
                6 => Fx::MemArg { addr: args[1], k: args[2] as usize },
                _ => Fx::Mem { addr: args[1], words },
            };
            if let Some(s) = out.last_mut() {
                s.fx.push(fx);
            }
        } else if host_name(&k.name) != "?" {
            let (eax, st0) = if k.result.bytes() == 4 { (k.result.value().unwrap_or(0) as u32, F80::ZERO) } else { (0, k.result_f80()) };
            let mut args = args;
            if k.name == FN_POINTER.0 {
                // a call through a function pointer: the harness records it under the name
                // of the function the pointer names, the port's host is handed the pointer
                // as its first argument
                args.insert(0, FN_POINTER.1);
            }
            out.push(Step { orig: k.name.clone(), args, raw: k.arg_values(), in_st0: k.result.bytes() != 4, eax, st0, fx: Vec::new() });
        }
    }
    out
}

fn arg_words(a: &Arg) -> Vec<u32> {
    match *a {
        Arg::I(v) | Arg::F(v) => vec![v],
        Arg::D(v) => vec![v as u32, (v >> 32) as u32],
    }
}

/// A host that answers each call with what the original's stub answered at that position,
/// and plays the stub's recorded side effects.
fn host(c: &Case) -> Trace {
    TEXT_ERR.with(|e| *e.borrow_mut() = None);
    let steps = tape(c);
    let next = Rc::new(RefCell::new(0usize));
    Trace::with(move |m, name, args| {
        let mut i = next.borrow_mut();
        let r = match steps.get(*i) {
            Some(s) if host_name(&s.orig) == name => {
                let mine: Vec<u32> = args.iter().flat_map(arg_words).collect();
                for f in &s.fx {
                    let (at, words) = match f {
                        Fx::Arg { k, off, words } => (mine.get(*k).copied().unwrap_or(0).wrapping_add(*off), words),
                        Fx::Mem { addr, words } => (*addr, words),
                        Fx::Arg16 { k, off, value } => {
                            let at = mine.get(*k).copied().unwrap_or(0);
                            if at != 0 {
                                m.set_u16(at.wrapping_add(*off), *value);
                            }
                            continue;
                        }
                        Fx::Text { k, text } => {
                            let got = m.cstr(mine.get(*k).copied().unwrap_or(0));
                            if got != *text {
                                TEXT_ERR.with(|e| {
                                    e.borrow_mut().get_or_insert_with(|| {
                                        format!("{} text: port {:?}, original {:?}", s.orig, String::from_utf8_lossy(&got), String::from_utf8_lossy(text))
                                    });
                                });
                            }
                            continue;
                        }
                        Fx::RetArg { .. } => continue,
                        Fx::MemArg { addr, k } => {
                            m.set_u32(*addr, mine.get(*k).copied().unwrap_or(0));
                            continue;
                        }
                        Fx::In { k, words } => {
                            let at = mine.get(*k).copied().unwrap_or(0);
                            let got: Vec<u32> = (0..words.len() as u32).map(|j| if at != 0 { m.u32(at + 4 * j) } else { 0 }).collect();
                            if (at == 0) != words.is_empty() || (at != 0 && got != *words) {
                                TEXT_ERR.with(|e| {
                                    e.borrow_mut().get_or_insert_with(|| format!("{} argument {k} points at {got:08x?}, the original's at {words:08x?}", s.orig));
                                });
                            }
                            continue;
                        }
                    };
                    if at != 0 {
                        for (j, w) in words.iter().enumerate() {
                            m.set_u32(at + 4 * j as u32, *w);
                        }
                    }
                }
                if s.in_st0 {
                    // `check_dir` keeps every call that answered in st0 on its replay tape and
                    // wants each of them made: this one is ours, taken off the tape here
                    let _ = si_x87::trig::replay::next(&s.orig, &s.raw);
                }
                let mut eax = s.eax;
                for f in &s.fx {
                    if let Fx::RetArg { k, off } = f {
                        eax = mine.get(*k).copied().unwrap_or(0).wrapping_add(*off);
                    }
                }
                Ret { eax, st0: s.st0 }
            }
            _ => {
                // The port has left the recorded calls: the case differs already. A routine
                // whose loop is driven by a stand-in's answers (the next graf node, the mouse
                // that interrupts playback) may then never end, since nothing answers any
                // more; it is stopped here, and the runner counts the panic as a difference.
                if *i > steps.len() + RUNAWAY {
                    panic!("the port made more than {} calls where the original made {}: it left the recorded calls and did not come back", *i, steps.len());
                }
                Ret::default()
            }
        };
        *i += 1;
        r
    })
}

/// How many calls a port may make beyond the original's before the check stops it.
const RUNAWAY: usize = 100_000;

/// The blocks the original allocated in a case, for the vectors that carry them (fourth
/// pass: `h:<n>` is block n's address and size in the emulator, `heap<n>` its bytes as the
/// routine left them, `None` where it never wrote).
struct OrigHeap {
    blocks: Vec<(u32, u32, Vec<Option<u8>>)>,
}

fn orig_heap(c: &Case) -> Option<OrigHeap> {
    let mut blocks = Vec::new();
    for n in 0.. {
        let Some(h) = c.try_output(&format!("h:{n}")) else { break };
        let w = h.words();
        let (addr, size) = (w[0].value().unwrap_or(0) as u32, w[1].value().unwrap_or(0) as u32);
        let bytes = c.try_output(&format!("heap{n}")).map_or_else(Vec::new, |f| {
            f.words().iter().map(|w| if let Word::Value { value, .. } = w { Some(*value as u8) } else { None }).collect()
        });
        blocks.push((addr, size, bytes));
    }
    (!blocks.is_empty()).then_some(OrigHeap { blocks })
}

/// The port's heap against the original's, block for block, for the vectors that carry the
/// original's blocks (`h:<n>`, `heap<n>`, `freed`): the same blocks at the same addresses
/// with the same sizes in the same order, the same blocks freed in the same order, and in
/// every block, freed or not, pointed at or not, every byte the original wrote. A pointer
/// of the original into its heap is the same number in the port's, so the bytes need no
/// translation.
fn whole_heap(c: &Case, img: &mut Image, heap: &OrigHeap) -> Result<(), String> {
    let theirs: Vec<(u32, u32)> = heap.blocks.iter().map(|b| (b.0, b.1)).collect();
    if img.mem.heap_blocks() != theirs.as_slice() {
        return Err(format!("heap blocks (address, size): port {:x?}, original {theirs:x?}", img.mem.heap_blocks()));
    }
    let freed: Vec<u32> = c.try_output("freed").map_or_else(Vec::new, |l| {
        l.words()
            .iter()
            .map(|w| match w {
                Word::Ref { name, offset } => img.lay.names.addr(&img.mem, name).unwrap_or(0).wrapping_add(*offset),
                w => w.value().unwrap_or(0) as u32,
            })
            .collect()
    });
    if img.mem.freed() != freed.as_slice() {
        return Err(format!("freed blocks: port {:x?}, original {freed:x?}", img.mem.freed()));
    }
    // a freed block is still memory in the original; the image is asked the same way
    img.mem.set_strict(false);
    let mut r = Ok(());
    'blocks: for (n, (a, _, bytes)) in heap.blocks.iter().enumerate() {
        for (off, b) in bytes.iter().enumerate() {
            if let Some(b) = b {
                let got = img.mem.u8(a + off as u32);
                if got != *b {
                    let w = off & !3;
                    r = Err(format!(
                        "heap{n}+{off:#x}: port {got:02x}, original {b:02x} (word +{w:#x}: port {:08x}, original {:02x?})",
                        img.mem.u32(a + w as u32),
                        &bytes[w..(w + 4).min(bytes.len())]
                    ));
                    break 'blocks;
                }
            }
        }
    }
    img.mem.set_strict(true);
    r
}

/// Buffers and calls of the port against the case.
fn compare(c: &Case, img: &mut Image, t: &Trace) -> Verdict {
    compare_but(c, img, t, "")
}

/// [`compare`] leaving one buffer out. With the environment variable `EMUCHECK_CALLS` set, a
/// difference is reported together with the two sequences of calls (for finding where a
/// port left the original's path), with `EMUCHECK_HEAP` every case whose heap is not the
/// original's is named on the error stream.
fn compare_but(c: &Case, img: &mut Image, t: &Trace, skip: &str) -> Verdict {
    // a check that declares words undefined (a stale word of the original's stack that the
    // two runs of the harness do not show) cannot be held to the original's heap either: the
    // word went through a key's block on its way into the curve
    let exempt = UNDEFINED.with(|u| !u.borrow().is_empty());
    let mut v = compare_inner(c, img, t, skip);
    if let Some(h) = orig_heap(c) {
        WHOLE_HEAP_CASES.fetch_add(1, Ordering::Relaxed);
        match whole_heap(c, img, &h) {
            Ok(()) => {
                WHOLE_HEAP_SAME.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                if std::env::var_os("EMUCHECK_HEAP").is_some() {
                    eprintln!("{}: {e}", c.name);
                }
                if v == Verdict::Same && !exempt {
                    v = Verdict::Different(e);
                }
            }
        }
    }
    let flipped = FLIPPED_AT.with(|f| f.replace(0));
    if flipped != 0 && v == Verdict::Same {
        // the flipped bit went unnoticed: it lies where the vector holds nothing
        SURVIVED.fetch_add(1, Ordering::Relaxed);
        if std::env::var_os("EMUCHECK_HEAP").is_some() {
            let what = img.lay.names.name_of(&img.mem, flipped).map_or_else(|| "no buffer of the vector".to_string(), |(n, o)| format!("{n}+{o:#x}"));
            eprintln!("{}: the bit flipped at {flipped:#x} ({what}) went unnoticed", c.name);
        }
    }
    match v {
        Verdict::Different(e) if std::env::var_os("EMUCHECK_CALLS").is_some() => {
            let theirs: Vec<String> = tape(c).iter().map(|k| k.orig.clone()).collect();
            Verdict::Different(format!("{e}\n      port calls: {:?}\n      original calls: {:?}", t.names(), theirs))
        }
        v => v,
    }
}

fn compare_inner(c: &Case, img: &Image, t: &Trace, skip: &str) -> Verdict {
    let m = &img.mem;
    if let Some(f) = m.fault() {
        return Verdict::Different(format!("the port left the case's memory: {f:?}"));
    }
    if let Some(e) = TEXT_ERR.with(|e| e.borrow_mut().take()) {
        return Verdict::Different(e);
    }
    let undefined: Vec<(&'static str, usize)> = UNDEFINED.with(|u| u.borrow_mut().drain(..).collect());
    let carried = c.try_output("h:0").is_some();
    for f in &c.outputs {
        if f.name == skip {
            continue;
        }
        // a block the original allocated, in a vector that numbers the blocks without giving
        // their addresses: the same allocator numbers the port's blocks the same way
        let block = f.name.strip_prefix("heap").and_then(|k| k.parse::<usize>().ok()).filter(|_| !carried && !img.lay.addr.contains_key(&f.name));
        let a = match (img.lay.addr.get(&f.name), block) {
            (Some(&a), _) => a,
            (None, Some(k)) => match m.heap_blocks().get(k) {
                Some(b) => b.0,
                None => return Verdict::Different(format!("the original allocated `{}`, the port made {} blocks", f.name, m.heap_blocks().len())),
            },
            (None, None) => continue,
        };
        let freed_there = c.try_output("freed").is_some_and(|l| l.words().iter().any(|w| matches!(w, Word::Ref { name, .. } if *name == f.name)));
        if !m.is_mapped(a, 1) {
            // the port freed the buffer: the original must have, and its contents no longer count
            if freed_there {
                continue;
            }
            return Verdict::Different(format!("the port freed `{}`, the original did not", f.name));
        }
        if freed_there && !is_global(a) {
            return Verdict::Different(format!("the original freed `{}`, the port did not", f.name));
        }
        // The words as `Mem::compare_words` takes them: a word nobody wrote is not compared,
        // nor is one a check declared undefined; a pointer to a buffer is the buffer's
        // address, and a pointer to a block the original allocated (`&heapN`) is the address
        // of the N-th block of the image's heap, which is where the original's lay.
        let words: Vec<W> = f
            .words()
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if undefined.iter().any(|(b, k)| *b == f.name && *k == i) {
                    return W::Unwritten { bytes: w.bytes() };
                }
                if matches!(w, Word::Ref { name, .. } if name.starts_with("heap") && !img.lay.addr.contains_key(name)) {
                    HEAP_PTRS.fetch_add(1, Ordering::Relaxed);
                }
                word(w)
            })
            .collect();
        if let Err(e) = m.compare_words(a, &words, &img.lay.names) {
            return Verdict::Different(format!("{}: {e}", f.name));
        }
    }
    let recorded = tape(c);
    if recorded.len() != t.calls.len() {
        let names: Vec<_> = recorded.iter().map(|k| k.orig.as_str()).collect();
        return Verdict::Different(format!("calls: port {:?}, original {:?}", t.names(), names));
    }
    for (k, (orig, mine)) in recorded.iter().zip(&t.calls).enumerate() {
        if host_name(&orig.orig) != mine.name {
            return Verdict::Different(format!("call {k}: port {}, original {}", mine.name, orig.orig));
        }
        // the stack words of the port's call, each with whether it can be a pointer (an
        // integer argument) or is a `float` or half of a `double`
        let got: Vec<(u32, bool)> = mine
            .args
            .iter()
            .flat_map(|a| match *a {
                Arg::I(v) => vec![(v, true)],
                Arg::F(v) => vec![(v, false)],
                Arg::D(v) => vec![(v as u32, false), ((v >> 32) as u32, false)],
            })
            .collect();
        for (j, ((g, pointer), w)) in got.iter().zip(&orig.args).enumerate() {
            // an address of a local of the port: the original's is somewhere on its own
            // stack. Everything else is the same number on both sides, pointers into the
            // heap included.
            if *pointer && m.region_of(*g) == Some(Region::Stack) {
                continue;
            }
            if g != w {
                return Verdict::Different(format!("call {k} ({}) argument {j}: port {g:08x}, original {w:08x}", orig.orig));
            }
            if *pointer && m.heap_block_of(*g).is_some() {
                HEAP_PTRS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    Verdict::Same
}

fn at(lay: &Layout, name: &str) -> Addr {
    *lay.addr.get(name).unwrap_or(&0)
}

fn time(c: &Case) -> f32 {
    c.input("time").f32()
}

/// A routine of the form `f(node, time)`.
fn node_case(c: &Case, f: impl FnOnce(&mut Mem, &mut Trace, Addr, f32)) -> Verdict {
    let mut img = match image(c) {
        Ok(i) => i,
        Err(e) => return Verdict::Different(e),
    };
    let mut t = host(c);
    let node = at(&img.lay, "node");
    let s = before(&img);
    f(&mut img.mem, &mut t, node, time(c));
    mutate(&mut img, s);
    compare(c, &mut img, &t)
}

/// The address an argument of the case names: a whole buffer (`name=*…`), or a pointer
/// passed by value (`name=ptr`, printed `&buffer+offset`), or null.
fn ptr_arg(c: &Case, lay: &Layout, name: &str) -> Addr {
    if let Some(&a) = lay.addr.get(name) {
        return a;
    }
    match c.try_input(name).and_then(|f| f.words().first()) {
        Some(Word::Ref { name, offset }) => lay.addr.get(name).map_or(0, |a| a + offset),
        Some(w) => w.value().unwrap_or(0) as u32,
        None => 0,
    }
}

fn int_arg(c: &Case, name: &str) -> i32 {
    c.input(name).words()[0].value().unwrap_or(0) as u32 as i32
}

/// Runs a port on the case's image; `f` returns what the routine returns in eax, if it
/// returns anything (compared with the vector's `eax`).
fn run(c: &Case, f: impl FnOnce(&mut Mem, &mut Trace, &Layout) -> Option<u32>) -> Verdict {
    // the routines of the second pass are compared on cases with undefined output words too
    // (those words are skipped); not when the original's very calls or result depend on
    // memory it never initialised
    if c.has_note("UNINIT:calls") || c.has_note("UNINIT:eax") || c.has_note("UNINIT:st0") {
        return Verdict::Skip;
    }
    let mut img = match image(c) {
        Ok(i) => i,
        Err(e) => return Verdict::Different(e),
    };
    let mut t = host(c);
    let s = before(&img);
    let mut r = f(&mut img.mem, &mut t, &img.lay);
    if mutating() {
        // the result too, where the vector holds one
        let flipped = mutate(&mut img, s);
        if r.is_some() && c.try_output("eax").is_some() {
            r = r.map(|v| v ^ 1);
            if !flipped {
                MUTATED.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    compare(c, &mut img, &t).and(|| match (r, c.try_output("eax")) {
        (Some(got), Some(want)) => {
            let want = want.words()[0].value().unwrap_or(0) as u32;
            Verdict::from(got == want, || format!("result: port {got:#x}, original {want:#x}"))
        }
        _ => Verdict::Same,
    })
}

/// Three floats passed by value (an `SI_Vector` on the stack), bit for bit.
fn vec_arg(c: &Case, name: &str) -> [f32; 3] {
    let w = c.input(name).u32s();
    [f32::from_bits(w[0]), f32::from_bits(w[1]), f32::from_bits(w[2])]
}

/// The pointer in a field of an object (null for a null object).
fn field_ptr(m: &Mem, obj: Addr, off: u32) -> Addr {
    if obj == 0 {
        0
    } else {
        m.ptr(obj + off)
    }
}

/// The text behind a pointer field when the port replaced the pointer by a new block.
fn new_text(m: &Mem, obj: Addr, off: u32, before: Addr) -> Option<Vec<u8>> {
    let p = field_ptr(m, obj, off);
    (p != before && p != 0).then(|| m.cstr(p))
}

/// The one text the original allocated in the case (`heap0`, from `strdup`) against the
/// port's.
fn heap_text(c: &Case, mine: Option<&[u8]>) -> Verdict {
    let theirs = c.try_output("heap0").map(|f| {
        let b: Vec<u8> = f.words().iter().map(|w| w.value().unwrap_or(0) as u8).collect();
        let n = b.iter().position(|x| *x == 0).unwrap_or(b.len());
        b[..n].to_vec()
    });
    Verdict::from(theirs.as_deref() == mine, || {
        format!("new text: port {:?}, original {:?}", mine.map(String::from_utf8_lossy), theirs.as_deref().map(String::from_utf8_lossy))
    })
}

/// The routines of the third pass: the constraints, shapes, clusters and deformations of
/// `motionUpdate.c`, and what follows of `motionKey.c`, `motion.c` and `motionUtils.c`.
fn lookup3(name: &str) -> Option<Check> {
    Some(match name {
        "constrGetParamOrientation" => |c| {
            run(c, |m, _, i| {
                constr::constr_get_param_orientation(m, ptr_arg(c, i, "obj"), int_arg(c, "kind"), ptr_arg(c, i, "mdl"), ptr_arg(c, i, "out"));
                None
            })
        },
        "modelSetParamRotation" => |c| {
            run(c, |m, t, i| {
                constr::model_set_param_rotation(m, t, ptr_arg(c, i, "mdl"), vec_arg(c, "v"));
                None
            })
        },
        "directionConstraint" => |c| run(c, |m, t, i| Some(constr::direction_constraint(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "dir")) as u32)),
        "tangencyConstraint" => {
            |c| run(c, |m, t, i| Some(constr::tangency_constraint(m, t, ptr_arg(c, i, "node"), ptr_arg(c, i, "mdl"), ptr_arg(c, i, "path")) as u32))
        }
        "normalToSurfaceConstraint" => {
            |c| run(c, |m, t, i| Some(constr::normal_to_surface_constraint(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "surf")) as u32))
        }
        "constrAvgPntLocation" => |c| {
            run(c, |m, t, i| {
                constr::constr_avg_pnt_location(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "k"), ptr_arg(c, i, "out"));
                None
            })
        },
        "si_animModelConstraintUpdate" => |c| {
            run(c, |m, t, i| {
                constr::anim_model_constraint_update(m, t, ptr_arg(c, i, "node"), time(c), int_arg(c, "index"));
                None
            })
        },
        "animCheckTrajectoryLock" => |c| run(c, |m, t, i| Some(constr::anim_check_trajectory_lock(m, t, ptr_arg(c, i, "node"), time(c)) as u32)),
        "si_animModelPlaneConstrUpdate" => |c| {
            run(c, |m, t, i| {
                constr::anim_model_plane_constr_update(m, t, ptr_arg(c, i, "node"), time(c), int_arg(c, "index"), ptr_arg(c, i, "v"));
                None
            })
        },
        "si_animModelSymConstrUpdate" => |c| {
            run(c, |m, t, i| {
                constr::anim_model_sym_constr_update(m, t, ptr_arg(c, i, "node"), time(c), int_arg(c, "index"));
                None
            })
        },
        "animCns2PtsUpdate" => |c| {
            run(c, |m, t, i| {
                constr::anim_cns_2pts_update(m, t, ptr_arg(c, i, "node"), time(c));
                None
            })
        },
        "animCns3PtsUpdate" => |c| {
            run(c, |m, t, i| {
                constr::anim_cns_3pts_update(m, t, ptr_arg(c, i, "node"), time(c));
                None
            })
        },
        "animCns3PtsFaceUpdate" => |c| {
            run(c, |m, t, i| {
                constr::anim_cns_3pts_face_update(m, t, ptr_arg(c, i, "mdl"), vec_arg(c, "p1"), vec_arg(c, "p2"), vec_arg(c, "p3"));
                None
            })
        },
        "animModelObjPntCnsUpdate" => |c| {
            run(c, |m, t, i| {
                constr::anim_model_obj_pnt_cns_update(m, t, ptr_arg(c, i, "node"), time(c));
                None
            })
        },
        "cnsClusterCtrSetPtsLocation" => |c| {
            run(c, |m, t, i| {
                let p = |n: &str| ptr_arg(c, i, n);
                constr::cns_cluster_ctr_set_pts_location(m, t, p("node"), p("mdl"), p("k"), p("ctr"), p("mat"), p("post"));
                None
            })
        },
        "cnsFPntObjSetPtsLocation" => |c| {
            run(c, |m, t, i| {
                constr::cns_fpnt_obj_set_pts_location(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "k"), vec_arg(c, "p"), ptr_arg(c, i, "post"));
                None
            })
        },
        "SI_animMdlClusterCnsUpdate" => |c| {
            // Cluster centres without relation data: the original hands on a matrix it never
            // set. Mostly the two runs of the harness show that (the case is then marked);
            // where both garbage matrices give the same not-a-numbers it is not, and the
            // port, which uses zeros, cannot agree. Not compared.
            let no_data = matches!(c.input("node").words().get(9), Some(Word::Value { value: 0, .. }));
            if int_arg(c, "centre") as i16 != 0 && no_data && c.calls.iter().any(|k| k.name == "SI_elemSetVertex") {
                return Verdict::Skip;
            }
            run(c, |m, t, i| {
                let (plain, centre) = (int_arg(c, "plain") as i16, int_arg(c, "centre") as i16);
                constr::anim_mdl_cluster_cns_update(m, t, ptr_arg(c, i, "node"), plain, centre, ptr_arg(c, i, "post"), time(c));
                None
            })
        },
        "animTrajectoryUpdate" => |c| {
            run(c, |m, t, i| {
                constr::anim_trajectory_update(m, t, ptr_arg(c, i, "node"), time(c));
                None
            })
        },
        "animShapeUpdate" => |c| {
            run(c, |m, t, i| {
                shape::anim_shape_update(m, t, ptr_arg(c, i, "node"), time(c));
                None
            })
        },
        "savekeyValidTranslation" => |c| run(c, |m, t, i| Some(savekey::savekey_valid_translation(m, t, ptr_arg(c, i, "path")) as u32)),
        "savekeyMapTranslation" => |c| {
            run(c, |m, _, i| {
                savekey::savekey_map_translation(m, ptr_arg(c, i, "path"), ptr_arg(c, i, "v"));
                None
            })
        },
        "SI_savekeyModel" | "SI_savekeyModel#path" => |c| {
            // A path key at a frame before the percentage curve's first key: `SI_pathAddKeyPct`
            // inserts a key whose value it never sets (finding 0017, section 9a), and the
            // curve's value bounds (+0x58, +0x5c) keep that garbage. Called from here the
            // garbage is a stale word of the routine's own stack, the same in both runs of
            // the harness, so the case is not marked: it is compared without those two words
            // and counted as not compared.
            let word = |name: &str, i: usize| c.try_input(name).and_then(|f| f.words().get(i).and_then(|w| w.value())).map(|v| v as u32);
            let before_first = match (word("curve", 3), word("curvek", 0), word("g_frame", 0), word("g_frame", 3)) {
                (Some(n), Some(first), Some(frame), Some(rate)) => n > 0 && (frame as i32 as f32) / f32::from_bits(rate) < f32::from_bits(first) + 1e-4,
                _ => false,
            };
            if before_first {
                UNDEFINED.with(|u| *u.borrow_mut() = vec![("curve", 22), ("curve", 23)]);
            }
            let v = run(c, |m, t, i| {
                let a = |n: &str| int_arg(c, n);
                let (mdl, rec) = (ptr_arg(c, i, "mdl"), ptr_arg(c, i, "rec"));
                savekey::savekey_model(m, t, mdl, a("group"), a("rel"), a("kind"), a("check") as i16, c.input("value").f32(), a("keyed") as i16, rec);
                None
            });
            UNDEFINED.with(|u| u.borrow_mut().clear());
            match v {
                Verdict::Same if before_first => Verdict::Skip,
                v => v,
            }
        },
        "si_savekeyBranchScale" => |c| {
            run(c, |m, t, i| {
                savekey::savekey_branch_scale(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "si_savekeyBranchRotate" => |c| {
            run(c, |m, t, i| {
                savekey::savekey_branch_rotate(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "si_savekeyBranchTranslate" => |c| {
            run(c, |m, t, i| {
                savekey::savekey_branch_translate(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "si_savekeyBranchAll" => |c| {
            run(c, |m, t, i| {
                savekey::savekey_branch_all(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "si_savekeyBranchHidden" => |c| {
            run(c, |m, t, i| {
                savekey::savekey_branch_hidden(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "gn"));
                None
            })
        },
        "SI_savekeyObjectAll" => |c| {
            run(c, |m, t, i| {
                savekey::savekey_object_all(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "animClustersUpdate" => |c| {
            run(c, |m, t, i| {
                shape::anim_clusters_update(m, t, ptr_arg(c, i, "node"), time(c));
                None
            })
        },
        "animSplineDefUpdate" => |c| {
            run(c, |m, t, i| {
                shape::anim_spline_def_update(m, t, ptr_arg(c, i, "node"), time(c), ptr_arg(c, i, "post"));
                None
            })
        },
        "animPatchDefUpdate" => |c| {
            run(c, |m, t, i| {
                shape::anim_patch_def_update(m, t, ptr_arg(c, i, "node"), time(c), ptr_arg(c, i, "post"));
                None
            })
        },
        "saveMapImage" => |c| {
            let mut text = None;
            run(c, |m, _, i| {
                let tex = ptr_arg(c, i, "tex");
                let before = field_ptr(m, tex, texfile::tex::PICTURE);
                let r = texfile::save_map_image(m, tex, ptr_arg(c, i, "name"));
                text = new_text(m, tex, texfile::tex::PICTURE, before);
                Some(r as i32 as u32)
            })
            .and(|| heap_text(c, text.as_deref()))
        },
        "isScriptUpToDate" => |c| {
            let mut text = None;
            run(c, |m, t, i| {
                let tex = ptr_arg(c, i, "tex");
                let before = field_ptr(m, tex, texfile::tex::PICTURE);
                let r = texfile::is_script_up_to_date(m, t, tex, int_arg(c, "frame"));
                text = new_text(m, tex, texfile::tex::PICTURE, before);
                Some(r as u32)
            })
            .and(|| heap_text(c, text.as_deref()))
        },
        "isSequenceUpToDate" => |c| {
            let mut text = None;
            run(c, |m, t, i| {
                let tex = ptr_arg(c, i, "tex");
                let before = field_ptr(m, tex, texfile::tex::FILE);
                let r = texfile::is_sequence_up_to_date(m, t, tex, int_arg(c, "frame"));
                text = new_text(m, tex, texfile::tex::FILE, before);
                Some(r as u32)
            })
            .and(|| heap_text(c, text.as_deref()))
        },
        _ => return None,
    })
}

/// A path key at a frame before the percentage curve's first key: `SI_pathAddKeyPct` inserts
/// a key whose value it never sets (finding 0017, section 9a), and the curve's value bounds
/// (+0x58, +0x5c) keep that garbage: a stale word of the original's own stack, the same in
/// both runs of the harness, so the case is not marked. Such a case is compared without
/// those two words of the buffer `curve` and counted as not compared.
fn path_key_case(c: &Case, f: impl FnOnce() -> Verdict) -> Verdict {
    let word = |name: &str, i: usize| c.try_input(name).and_then(|f| f.words().get(i).and_then(|w| w.value())).map(|v| v as u32);
    let before_first = match (word("curve", 3), word("curvek", 0), word("g_frame", 0), word("g_frame", 3)) {
        (Some(n), Some(first), Some(frame), Some(rate)) => n > 0 && (frame as i32 as f32) / f32::from_bits(rate) < f32::from_bits(first) + 1e-4,
        _ => false,
    };
    if before_first {
        UNDEFINED.with(|u| *u.borrow_mut() = vec![("curve", 22), ("curve", 23)]);
    }
    let v = f();
    UNDEFINED.with(|u| u.borrow_mut().clear());
    match v {
        Verdict::Same if before_first => Verdict::Skip,
        v => v,
    }
}

/// The routines of the fourth pass: the save-key routines of `motionKey.c` that had only unit
/// tests (`key.rs`), the rest of `motionKey.c`, `motionUtils.c`, `motion.c`.
fn lookup4(name: &str) -> Option<Check> {
    Some(match name {
        "si_modelSetIDefinition" => |c| run(c, |m, t, i| Some(keyedit::model_set_i_definition(m, t, ptr_arg(c, i, "mdl"), int_arg(c, "n") as i16) as u32)),
        "relationCheckAddActiveFcv" => |c| {
            run(c, |m, t, i| {
                let a = |n: &str| int_arg(c, n);
                Some(keyedit::relation_check_add_active_fcv(m, t, ptr_arg(c, i, "rel"), ptr_arg(c, i, "e"), a("chap"), a("index"), a("flag") as i16) as u32)
            })
        },
        "si_relationActiveRangeSetKey" => |c| {
            run(c, |m, t, i| {
                let a = |n: &str| int_arg(c, n);
                let (from, to) = (c.input("from").f32(), c.input("to").f32());
                Some(keyedit::relation_active_range_set_key(m, t, ptr_arg(c, i, "rel"), ptr_arg(c, i, "e"), a("chap"), a("index"), from, to, a("on") as i16) as u32)
            })
        },
        "modelPairApplySymmetry" => |c| {
            run(c, |m, t, i| {
                let p = |n: &str| ptr_arg(c, i, n);
                keyedit::model_pair_apply_symmetry(m, t, p("a"), p("b"), int_arg(c, "which"), p("data"), int_arg(c, "deep") as i16);
                None
            })
        },
        "elemdictEditElemKey" => |c| {
            let mut r = 0i16;
            run(c, |m, t, i| {
                let a = |n: &str| int_arg(c, n);
                r = keyedit::elemdict_edit_elem_key(m, t, ptr_arg(c, i, "d"), ptr_arg(c, i, "e"), a("chap"), a("which"), time(c), a("deep") as i16);
                None
            })
            .and(|| {
                let ax = c.output("ax").words()[0].value().unwrap_or(0) as u16;
                Verdict::from(ax == r as u16, || format!("result: port {r}, original {ax}"))
            })
        },
        "SI_sceneKeyEdit" => |c| {
            let mut r = 0i16;
            run(c, |m, t, _| {
                r = keyedit::scene_key_edit(m, t, int_arg(c, "which"), int_arg(c, "frame"), int_arg(c, "update") as i16);
                None
            })
            .and(|| {
                let ax = c.output("ax").words()[0].value().unwrap_or(0) as u16;
                Verdict::from(ax == r as u16, || format!("result: port {r}, original {ax}"))
            })
        },
        "SI_savekeySingleCam" => |c| {
            path_key_case(c, || {
                run(c, |m, t, i| {
                    let a = |n: &str| int_arg(c, n);
                    savekey2::savekey_single_cam(m, t, ptr_arg(c, i, "cam"), a("group"), a("rel"), a("kind"), a("check") as i16, c.input("value").f32(), a("keyed") as i16);
                    None
                })
            })
        },
        "SI_savekeyCamera" => |c| {
            path_key_case(c, || {
                run(c, |m, t, i| {
                    savekey2::savekey_camera(m, t, ptr_arg(c, i, "cam"), ptr_arg(c, i, "gn"));
                    None
                })
            })
        },
        "si_savekeyTripleLit" => |c| {
            run(c, |m, t, i| {
                savekey2::savekey_triple_lit(m, t, ptr_arg(c, i, "lit"), int_arg(c, "group"), int_arg(c, "rel"), int_arg(c, "kind"), vec_arg(c, "v"));
                None
            })
        },
        "SI_savekeyLit" => |c| {
            path_key_case(c, || {
                run(c, |m, t, i| {
                    let a = |n: &str| int_arg(c, n);
                    savekey2::savekey_lit(m, t, ptr_arg(c, i, "lit"), a("group"), a("rel"), a("kind"), c.input("value").f32(), a("keyed") as i16);
                    None
                })
            })
        },
        "SI_savekeyLightMatter" => |c| {
            run(c, |m, t, i| {
                savekey2::savekey_light_matter(m, t, ptr_arg(c, i, "lit"));
                None
            })
        },
        "si_savekeyLight" => |c| {
            path_key_case(c, || {
                run(c, |m, t, i| {
                    savekey2::savekey_light(m, t, ptr_arg(c, i, "lit"), ptr_arg(c, i, "gn"));
                    None
                })
            })
        },
        "si_savekeyFitAll" => |c| {
            run(c, |m, t, i| {
                savekey2::savekey_fit_all(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "rec"), int_arg(c, "branch") as i16);
                None
            })
        },
        "si_savekeyPFitAll" => |c| {
            run(c, |m, t, i| {
                savekey2::savekey_pfit_all(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "rec"), int_arg(c, "branch") as i16);
                None
            })
        },
        "SI_fcurveCheckAdd" => |c| {
            run(c, |m, t, i| {
                let r = key::fcurve_check_add(m, t, ptr_arg(c, i, "e"), int_arg(c, "index"), int_arg(c, "kind"));
                m.set_ptr(ptr_arg(c, i, "out"), r);
                None
            })
        },
        "SI_fcurveCheckAdd1" => |c| {
            run(c, |m, t, i| {
                let r = key::fcurve_check_add1(m, t, ptr_arg(c, i, "e"), int_arg(c, "index"), int_arg(c, "rel"));
                m.set_ptr(ptr_arg(c, i, "out"), r);
                None
            })
        },
        "SI_fcurveCheckAdd3" => |c| {
            run(c, |m, t, i| {
                let r = key::fcurve_check_add3(m, t, ptr_arg(c, i, "e"), int_arg(c, "index"), int_arg(c, "rel"));
                for (k, n) in ["c1", "c2", "c3"].iter().enumerate() {
                    m.set_ptr(ptr_arg(c, i, n), r[k]);
                }
                None
            })
        },
        "fcurveRemoveKeyFromTime" => |c| {
            run(c, |m, t, i| {
                key::fcurve_remove_key_from_time(m, t, ptr_arg(c, i, "curve"), time(c), int_arg(c, "arg"));
                None
            })
        },
        "SI_fcurveCheckRemoveKey3" => |c| {
            run(c, |m, t, i| {
                key::fcurve_check_remove_key3(m, t, ptr_arg(c, i, "e"), int_arg(c, "arg"), int_arg(c, "rel"), time(c));
                None
            })
        },
        "savekeyMatSingleFcurve" | "savekeyMapFcurve" | "savekeyTX3DFcurve" => |c| {
            run(c, |m, t, i| {
                key::savekey_single_fcurve(m, t, ptr_arg(c, i, "e"), int_arg(c, "index"), int_arg(c, "rel"), c.input("value").f32());
                None
            })
        },
        "savekeyMatTripleFcurve" => |c| {
            run(c, |m, t, i| {
                key::savekey_mat_triple_fcurve(m, t, ptr_arg(c, i, "e"), int_arg(c, "index"), int_arg(c, "rel"), vec_arg(c, "v"));
                None
            })
        },
        "savekeyMapTripleFcurve" => |c| {
            run(c, |m, t, i| {
                key::savekey_map_triple_fcurve(m, t, ptr_arg(c, i, "e"), int_arg(c, "index"), int_arg(c, "rel"), vec_arg(c, "v"));
                None
            })
        },
        "SI_savekeyMaterial" => |c| {
            run(c, |m, t, i| {
                key::savekey_material(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "skTexture" => |c| {
            run(c, |m, t, i| {
                key::sk_texture(m, t, ptr_arg(c, i, "tex"));
                None
            })
        },
        "SI_savekeyLTexture" => |c| {
            run(c, |m, t, i| {
                key::savekey_l_texture(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "SI_savekeyGTexture" => |c| {
            run(c, |m, t, i| {
                key::savekey_g_texture(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "SI_depthFadingSaveKey" => |c| {
            run(c, |m, t, _| {
                key::depth_fading_save_key(m, t);
                None
            })
        },
        "SI_layerFogSaveKey" => |c| {
            run(c, |m, t, _| {
                key::layer_fog_save_key(m, t);
                None
            })
        },
        "savekeyTX3DQuadFcurve" => |c| {
            run(c, |m, t, i| {
                let w = c.input("v").u32s();
                let v = [f32::from_bits(w[0]), f32::from_bits(w[1]), f32::from_bits(w[2]), f32::from_bits(w[3])];
                let a = |n: &str| int_arg(c, n);
                key::savekey_tx3d_quad_fcurve(m, t, ptr_arg(c, i, "e"), ptr_arg(c, i, "tex"), a("index"), a("rel"), a("kind"), a("akind"), v);
                None
            })
        },
        "savekeyTX3DTripleFcurve" => |c| {
            run(c, |m, t, i| {
                key::savekey_tx3d_triple_fcurve(m, t, ptr_arg(c, i, "e"), int_arg(c, "index"), int_arg(c, "rel"), vec_arg(c, "v"));
                None
            })
        },
        "skTexture3D" => |c| {
            run(c, |m, t, i| {
                key::sk_texture3d(m, t, ptr_arg(c, i, "tex"));
                None
            })
        },
        "SI_savekeyLTexture3D" => |c| {
            run(c, |m, t, i| {
                key::savekey_l_texture3d(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "SI_savekeyGTexture3D" => |c| {
            run(c, |m, t, i| {
                key::savekey_g_texture3d(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "SI_savekeyPhysicalProps" => |c| {
            run(c, |m, t, i| {
                key::savekey_physical_props(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "SI_savekeyDynamicSetup" => |c| {
            run(c, |m, t, i| {
                key::savekey_dynamic_setup(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        _ => return None,
    })
}

/// The routines of the second pass: `motionPath.c`, `motion.c`, `motionUtils.c`,
/// `motionReset.c`, `mot_lattice.c`.
fn lookup2(name: &str) -> Option<Check> {
    Some(match name {
        "relGetPathRelFromGrafType" => |c| run(c, |_, _, _| Some(path::rel_get_path_rel_from_graf_type(int_arg(c, "kind")) as u32)),
        "getPathTiming" => |c| {
            run(c, |m, _, i| {
                let (a, b) = path::get_path_timing(m, ptr_arg(c, i, "curve"));
                m.set_i32(ptr_arg(c, i, "a"), a);
                m.set_i32(ptr_arg(c, i, "b"), b);
                None
            })
        },
        "SI_checkSplineToFcurve" => |c| run(c, |m, _, i| Some(path::check_spline_to_fcurve(m, ptr_arg(c, i, "curve"), ptr_arg(c, i, "spl")) as u32)),
        "SI_checkNrbCrvToFcurve" => |c| run(c, |m, _, i| Some(path::check_nrb_crv_to_fcurve(m, ptr_arg(c, i, "curve"), ptr_arg(c, i, "crv")) as u32)),
        "SI_invertFcurvePathKeys" => |c| {
            run(c, |m, _, i| Some(path::invert_fcurve_path_keys(m, ptr_arg(c, i, "curve"), ptr_arg(c, i, "spl"), ptr_arg(c, i, "dir")) as u32))
        },
        "SI_addFcurvePathKey" => |c| run(c, |m, _, i| Some(path::add_fcurve_path_key(m, ptr_arg(c, i, "curve"), int_arg(c, "n")) as u32)),
        "SI_incrementFcurvePathKey" => |c| run(c, |m, _, i| Some(path::increment_fcurve_path_key(m, ptr_arg(c, i, "curve"), int_arg(c, "n")) as u32)),
        "SI_deleteFcurvePathKey" => |c| run(c, |m, t, i| Some(path::delete_fcurve_path_key(m, t, ptr_arg(c, i, "curve"), int_arg(c, "n") as i16) as u32)),
        "si_keySplineToFcurve" => |c| run(c, |m, t, i| Some(path::key_spline_to_fcurve(m, t, ptr_arg(c, i, "curve"), ptr_arg(c, i, "spl")) as u32)),
        "SI_updateFcurvePathKeys" => |c| run(c, |m, t, i| Some(path::update_fcurve_path_keys(m, t, ptr_arg(c, i, "curve"), ptr_arg(c, i, "spl")) as u32)),
        "SI_updateFcurveNURBSPathKeys" => {
            |c| run(c, |m, t, i| Some(path::update_fcurve_nurbs_path_keys(m, t, ptr_arg(c, i, "curve"), ptr_arg(c, i, "crv")) as u32))
        }
        "pathOverwrite" => |c| {
            run(c, |m, t, i| {
                let (spl, curve, pt) = (ptr_arg(c, i, "spl"), ptr_arg(c, i, "curve"), ptr_arg(c, i, "pt"));
                Some(path::path_overwrite(m, t, spl, curve, c.input("factor").f32(), int_arg(c, "point") as i16, pt) as u32)
            })
        },
        "nurbsPathOverwrite" => |c| {
            run(c, |m, t, i| {
                let (crv, curve, pt) = (ptr_arg(c, i, "crv"), ptr_arg(c, i, "curve"), ptr_arg(c, i, "pt"));
                Some(path::nurbs_path_overwrite(m, t, crv, curve, c.input("u").f64(), int_arg(c, "point"), pt) as u32)
            })
        },
        "SI_modelSetPath" => |c| run(c, |m, _, i| Some(path::model_set_path(m, ptr_arg(c, i, "obj"), ptr_arg(c, i, "path")))),
        "si_lightSetPositionPath" => |c| run(c, |m, _, i| Some(path::light_set_position_path(m, ptr_arg(c, i, "obj"), ptr_arg(c, i, "path")))),
        "si_lightSetInterestPath" => |c| run(c, |m, _, i| Some(path::light_set_interest_path(m, ptr_arg(c, i, "obj"), ptr_arg(c, i, "path")))),
        "finalComputePathTrans" => |c| {
            run(c, |m, t, i| {
                let (e, obj, p) = (ptr_arg(c, i, "e"), ptr_arg(c, i, "obj"), ptr_arg(c, i, "path"));
                transfo::final_compute_path_trans(m, t, int_arg(c, "rel"), e, obj, p, c.input("percent").f32());
                None
            })
        },
        "animAllTypePathUpdate" => |c| node_case(c, |m, t, n, x| transfo::anim_all_type_path_update(m, t, n, x)),
        "SI_pathElementSet" => |c| {
            run(c, |m, t, i| {
                path::path_element_set(m, t, ptr_arg(c, i, "obj"), int_arg(c, "kind"), ptr_arg(c, i, "path"));
                None
            })
        },
        "SI_pathElementRelax" => |c| {
            run(c, |m, t, i| {
                path::path_element_relax(m, t, ptr_arg(c, i, "obj"), int_arg(c, "kind"));
                None
            })
        },
        "SI_findElementPath" => {
            |c| run(c, |m, t, i| Some(path::find_element_path(m, t, ptr_arg(c, i, "e"), int_arg(c, "x"), int_arg(c, "idx"), int_arg(c, "kind"))))
        }
        "si_itemShowPath" => |c| {
            run(c, |m, t, i| {
                path::item_show_path(m, t, ptr_arg(c, i, "obj"), int_arg(c, "kind"), int_arg(c, "show"));
                None
            })
        },
        "si_itemShowPathAll" => |c| {
            run(c, |m, t, _| {
                path::item_show_path_all(m, t, int_arg(c, "show"));
                None
            })
        },
        "SI_findPathFcurveOwner" => |c| run(c, |m, t, i| Some(path::find_path_fcurve_owner(m, t, ptr_arg(c, i, "geom")))),
        "SI_findNURBSPathFcurveOwner" => |c| run(c, |m, t, i| Some(path::find_nurbs_path_fcurve_owner(m, t, ptr_arg(c, i, "geom")))),
        "si_findPathSplineOwner" => |c| run(c, |m, t, i| Some(path::find_path_spline_owner(m, t, ptr_arg(c, i, "curve")))),
        "SI_actionCnsToNull" => |c| {
            run(c, |m, t, i| {
                let a = |n: &str| int_arg(c, n);
                path::action_cns_to_null(m, t, a("chap"), a("idx"), a("kind"), ptr_arg(c, i, "mdl"), a("tidx"), a("rel"), a("a"), a("b"), a("keep") as i16);
                None
            })
        },
        "SI_pathNew" => |c| run(c, |m, t, _| Some(path::path_new(m, t, int_arg(c, "kind")))),
        "si_itemRelaxRelationPtrs" => |c| {
            run(c, |m, t, i| {
                reset::item_relax_relation_ptrs(m, t, ptr_arg(c, i, "d"), int_arg(c, "chap"), ptr_arg(c, i, "e"), int_arg(c, "rel"));
                None
            })
        },
        "modelCopyLattice" => |c| {
            run(c, |m, _, i| {
                lattice::model_copy_lattice(m, ptr_arg(c, i, "dst"), ptr_arg(c, i, "src"));
                None
            })
        },
        "si_modelSetLattice" => {
            |c| run(c, |m, t, i| Some(lattice::model_set_lattice(m, t, ptr_arg(c, i, "mdl"), int_arg(c, "dir"), int_arg(c, "branch")) as u32))
        }
        "si_latticeAddKey" => |c| {
            run(c, |m, t, i| {
                lattice::lattice_add_key(m, t, ptr_arg(c, i, "mdl"), int_arg(c, "branch") as i16);
                None
            })
        },
        "si_latticeRemoveKey" => |c| run(c, |m, t, i| Some(lattice::lattice_remove_key(m, t, ptr_arg(c, i, "mdl"), int_arg(c, "branch")) as u32)),
        "SI_pathAddKeyPct" => |c| {
            run(c, |m, t, i| {
                path::path_add_key_pct(m, t, ptr_arg(c, i, "spl"), ptr_arg(c, i, "curve"), ptr_arg(c, i, "pt"), int_arg(c, "keyed") as i16);
                None
            })
        },
        "SI_nurbsPathAddKeyPct" => |c| {
            run(c, |m, t, i| {
                path::nurbs_path_add_key_pct(m, t, ptr_arg(c, i, "crv"), ptr_arg(c, i, "curve"), ptr_arg(c, i, "pt"), int_arg(c, "keyed") as i16);
                None
            })
        },
        "SI_actionCreateNullOnPath" => |c| {
            run(c, |m, t, i| Some(path::action_create_null_on_path(m, t, ptr_arg(c, i, "path"), int_arg(c, "a"), int_arg(c, "b"), ptr_arg(c, i, "idx"))))
        },
        "elemdictResetElemChapAnim" => |c| {
            run(c, |m, t, i| {
                let (d, e) = (ptr_arg(c, i, "d"), ptr_arg(c, i, "e"));
                Some(reset::elemdict_reset_elem_chap_anim(m, t, d, e, int_arg(c, "chap"), int_arg(c, "idx"), int_arg(c, "del") as i16, int_arg(c, "deep") as i16) as u32)
            })
        },
        "SI_elemdictResetElemAnim" => |c| {
            run(c, |m, t, i| {
                let (d, e) = (ptr_arg(c, i, "d"), ptr_arg(c, i, "e"));
                Some(reset::elemdict_reset_elem_anim(m, t, d, e, int_arg(c, "chap"), int_arg(c, "idx"), int_arg(c, "del") as i16) as u32)
            })
        },
        "SI_edRemoveElemChapAnim" => |c| {
            run(c, |m, t, i| {
                let (d, e) = (ptr_arg(c, i, "d"), ptr_arg(c, i, "e"));
                Some(reset::ed_remove_elem_chap_anim(m, t, d, int_arg(c, "chap"), e, int_arg(c, "idx"), int_arg(c, "del") as i16, int_arg(c, "deep") as i16) as u32)
            })
        },
        "SI_edRemoveElemAnim" => |c| {
            run(c, |m, t, i| {
                let (d, e) = (ptr_arg(c, i, "d"), ptr_arg(c, i, "e"));
                reset::ed_remove_elem_anim(m, t, d, int_arg(c, "chap"), e, int_arg(c, "idx"), int_arg(c, "del") as i16);
                None
            })
        },
        "SI_modelResetAll" => |c| {
            run(c, |m, t, i| {
                reset::model_reset_all(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "SI_modelResetShape" => |c| {
            run(c, |m, t, i| {
                reset::model_reset_shape(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "SI_modelResetGeomRelatedAnim" => |c| {
            run(c, |m, t, i| {
                reset::model_reset_geom_related_anim(m, t, ptr_arg(c, i, "d"), ptr_arg(c, i, "mdl"));
                None
            })
        },
        "si_itemSetPathTiming" => |c| {
            run(c, |m, t, i| {
                path::item_set_path_timing(m, t, ptr_arg(c, i, "obj"), int_arg(c, "kind"), ptr_arg(c, i, "path"), int_arg(c, "set") as i16, int_arg(c, "action") as i16);
                None
            })
        },
        "SI_setPathTranslation" => |c| {
            run(c, |m, t, i| {
                transfo::set_path_translation(m, t, int_arg(c, "rel"), int_arg(c, "chap"), ptr_arg(c, i, "obj"), ptr_arg(c, i, "curve"));
                None
            })
        },
        "SI_pathDelete" => |c| {
            run(c, |m, t, i| {
                path::path_delete(m, t, ptr_arg(c, i, "mdl"));
                None
            })
        },
        "si_itemResetPath" => |c| {
            run(c, |m, t, i| {
                path::item_reset_path(m, t, ptr_arg(c, i, "gnode"));
                None
            })
        },
        "SI_modelResetFcv" => |c| {
            run(c, |m, t, i| {
                let a = |n: &str| int_arg(c, n);
                reset::model_reset_fcv(m, t, ptr_arg(c, i, "mdl"), a("chap"), a("group"), a("rel"), a("filtered") as i16, a("first"), a("last"), ptr_arg(c, i, "flag"));
                None
            })
        },
        "SI_modelResetMaterial" => |c| {
            run(c, |m, t, i| {
                reset::model_reset_material(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "x"));
                None
            })
        },
        "SI_modelResetTexture" => |c| {
            run(c, |m, t, i| {
                reset::model_reset_texture(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "x"));
                None
            })
        },
        "SI_modelResetTexture3D" => |c| {
            run(c, |m, t, i| {
                reset::model_reset_texture3d(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "x"));
                None
            })
        },
        "modelResetTxtFcv" => |c| {
            run(c, |m, t, i| {
                reset::model_reset_txt_fcv(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "x"), int_arg(c, "rel"));
                None
            })
        },
        "modelResetTx3DFcv" => |c| {
            run(c, |m, t, i| {
                reset::model_reset_tx3d_fcv(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "x"), int_arg(c, "rel"));
                None
            })
        },
        "modelResetDynamics" => |c| {
            run(c, |m, t, i| {
                reset::model_reset_dynamics(m, t, ptr_arg(c, i, "mdl"), int_arg(c, "all") as i16, int_arg(c, "kind"));
                None
            })
        },
        "SI_fcvTableSet" => |c| {
            run(c, |m, _, _| {
                utils::fcv_table_set(m, int_arg(c, "v") as i16);
                None
            })
        },
        "si_modelActive" => |c| run(c, |m, t, _| Some(utils::model_active(m, t))),
        "si_singleFcurveCopy" => |c| {
            run(c, |m, t, i| {
                utils::single_fcurve_copy(m, t, ptr_arg(c, i, "a"), ptr_arg(c, i, "b"), int_arg(c, "idx"), int_arg(c, "rel"));
                None
            })
        },
        "SI_tripleFcurveCopy" => |c| {
            run(c, |m, t, i| {
                utils::triple_fcurve_copy(m, t, ptr_arg(c, i, "a"), ptr_arg(c, i, "b"), int_arg(c, "idx"), int_arg(c, "rel"));
                None
            })
        },
        "SI_getFcvTriplet" => |c| {
            run(c, |m, t, i| {
                let (c1, c2, c3) = (ptr_arg(c, i, "c1"), ptr_arg(c, i, "c2"), ptr_arg(c, i, "c3"));
                utils::get_fcv_triplet(m, t, ptr_arg(c, i, "e"), c1, c2, c3, int_arg(c, "group"), int_arg(c, "rel"));
                None
            })
        },
        "getWaveFcvPointer" => |c| {
            run(c, |m, t, i| {
                utils::get_wave_fcv_pointer(m, t, ptr_arg(c, i, "mdl"), ptr_arg(c, i, "out"));
                None
            })
        },
        "elemdictCheckElemKey" => {
            |c| run(c, |m, t, i| Some(utils::elemdict_check_elem_key(m, t, ptr_arg(c, i, "d"), ptr_arg(c, i, "e"), int_arg(c, "dope") as i16, time(c)) as u32))
        }
        "SI_sceneKeySetColor" => |c| {
            run(c, |m, t, _| {
                utils::scene_key_set_color(m, t, time(c), int_arg(c, "all") as i16, int_arg(c, "dope") as i16);
                None
            })
        },
        "timeElapsed" => |c| {
            let mut st0 = F80::ZERO;
            run(c, |m, t, i| {
                st0 = play::time_elapsed(m, t, ptr_arg(c, i, "slot"));
                None
            })
            .and(|| Verdict::from(c.output("st0").same_f80(st0), || format!("st0: port {st0:?}")))
        },
        "SI_displayFrameRate" => |c| {
            let mut st0 = F80::ZERO;
            run(c, |m, t, _| {
                st0 = F80::from_f32(play::display_frame_rate(m, t));
                None
            })
            .and(|| Verdict::from(c.output("st0").same_f80(st0), || format!("st0: port {st0:?}")))
        },
        "sceneRefreshLoop" => |c| {
            run(c, |m, t, _| {
                play::scene_refresh_loop(m, t, time(c));
                None
            })
        },
        "SI_sceneRefresh" => |c| {
            run(c, |m, t, _| {
                play::scene_refresh(m, t);
                None
            })
        },
        "SI_sceneRefreshSelection" => |c| {
            run(c, |m, t, _| {
                play::scene_refresh_selection(m, t);
                None
            })
        },
        "SI_sceneFrameRefreshLoop" => |c| {
            run(c, |m, t, _| {
                play::scene_frame_refresh_loop(m, t, int_arg(c, "frame"));
                None
            })
        },
        "SI_sceneStepFrame" => |c| {
            run(c, |m, t, _| {
                play::scene_step_frame(m, t, int_arg(c, "next"), int_arg(c, "prev"), int_arg(c, "first"), int_arg(c, "last"));
                None
            })
        },
        "playBackFrame" => |c| {
            run(c, |m, t, i| Some(play::play_back_frame(m, t, c.input("pos").f64(), ptr_arg(c, i, "step"), c.input("dir").f64()) as u32))
        },
        "playBackFrames" => |c| {
            run(c, |m, t, _| {
                let f = |n: &str| c.input(n).f32();
                play::play_back_frames(m, t, f("first"), f("last"), f("step"), int_arg(c, "mode"), int_arg(c, "pingpong"), int_arg(c, "to"));
                None
            })
        },
        _ => return None,
    })
}

/// The routines of the fifth pass: the rest of `motionUtils.c`, and the playback command
/// and the key browser of `motion.c`.
fn lookup5(name: &str) -> Option<Check> {
    Some(match name {
        "si_fcvNextBestKey" => |c| {
            run(c, |m, _, i| {
                let (fwd, wrap) = (int_arg(c, "fwd") as i16, int_arg(c, "wrap") as i16);
                Some(nextkey::fcv_next_best_key(m, ptr_arg(c, i, "curve"), fwd, wrap, int_arg(c, "first"), int_arg(c, "last"), ptr_arg(c, i, "best")) as i32 as u32)
            })
        },
        "elemdictNextKeyElemAnim" => |c| {
            run(c, |m, t, i| {
                let a = |n: &str| int_arg(c, n);
                let (d, e, best) = (ptr_arg(c, i, "d"), ptr_arg(c, i, "e"), ptr_arg(c, i, "best"));
                Some(nextkey::elemdict_next_key_elem_anim(m, t, d, e, a("chap"), a("index"), a("fwd") as i16, a("wrap") as i16, a("dope") as i16, best) as i32 as u32)
            })
        },
        "SI_edNextKeyElemAnim" => |c| {
            run(c, |m, t, i| {
                let a = |n: &str| int_arg(c, n);
                let (d, e, best) = (ptr_arg(c, i, "d"), ptr_arg(c, i, "e"), ptr_arg(c, i, "best"));
                Some(nextkey::ed_next_key_elem_anim(m, t, d, e, a("chap"), a("index"), a("fwd") as i16, a("wrap") as i16, a("dope") as i16, best) as u32)
            })
        },
        "SI_nextKeyDisplay" => |c| {
            run(c, |m, t, _| {
                let a = |n: &str| int_arg(c, n) as i16;
                Some(nextkey::next_key_display(m, t, a("fwd"), a("wrap"), a("refresh")) as i32 as u32)
            })
        },
        "si_elemdictValidFcurve" => {
            |c| run(c, |m, t, i| Some(animcopy::elemdict_valid_fcurve(m, t, ptr_arg(c, i, "d"), ptr_arg(c, i, "e"), int_arg(c, "chap")) as u32))
        }
        "si_elemdictSelectElemAnim" => |c| {
            run(c, |m, t, i| Some(animcopy::elemdict_select_elem_anim(m, t, ptr_arg(c, i, "d"), ptr_arg(c, i, "e"), int_arg(c, "chap"), int_arg(c, "index")) as u32))
        },
        "subChapCheckOK" => |c| run(c, |m, _, i| Some(animcopy::sub_chap_check_ok(m, ptr_arg(c, i, "e"), int_arg(c, "chap"), int_arg(c, "rel")) as u32)),
        "SI_elemdictCopyElemAnim" => |c| {
            run(c, |m, t, i| {
                let (d, a, b) = (ptr_arg(c, i, "d"), ptr_arg(c, i, "a"), ptr_arg(c, i, "b"));
                Some(animcopy::elemdict_copy_elem_anim(m, t, d, a, b, int_arg(c, "chap"), int_arg(c, "ia"), int_arg(c, "ib")) as u32)
            })
        },
        "SI_modelLoadFcurveMaterial" => |c| {
            run(c, |m, t, i| {
                animcopy::model_load_fcurve_material(m, t, ptr_arg(c, i, "to"), ptr_arg(c, i, "from"));
                None
            })
        },
        "SI_texture2DLoadFcurveAll" => |c| {
            run(c, |m, t, i| {
                animcopy::texture2d_load_fcurve_all(m, t, ptr_arg(c, i, "to"), ptr_arg(c, i, "from"));
                None
            })
        },
        "SI_texture3DLoadFcurveAll" => |c| {
            run(c, |m, t, i| {
                animcopy::texture3d_load_fcurve_all(m, t, ptr_arg(c, i, "to"), ptr_arg(c, i, "from"));
                None
            })
        },
        // sixth pass: the offset-and-scale family
        "SI_elemdictActiveFcvOper" => |c| {
            run(c, |m, t, i| {
                let a = |n: &str| int_arg(c, n);
                let (d, e, func) = (ptr_arg(c, i, "d"), ptr_arg(c, i, "e"), ptr_arg(c, i, "func"));
                let r = fcvoper::elemdict_active_fcv_oper(m, t, d, e, a("chap"), a("index"), a("oper"), func, five(c), a("with_act") as i16, a("once") as i16);
                Some(r as i32 as u32)
            })
        },
        "modelOffsetAndScale" => |c| {
            run(c, |m, t, i| {
                fcvoper::model_offset_and_scale(m, t, ptr_arg(c, i, "m"), five(c), int_arg(c, "with_act") as i16);
                None
            })
        },
        "SI_modelAFunction" => |c| {
            run(c, |m, t, i| {
                fcvoper::model_a_function(m, t, ptr_arg(c, i, "func"), ptr_arg(c, i, "m"), c.input("value").f32());
                None
            })
        },
        "objSeqOffsetAndScale" => |c| {
            run(c, |m, t, i| {
                let a = |n: &str| int_arg(c, n);
                let (gn, e) = (ptr_arg(c, i, "gn"), ptr_arg(c, i, "e"));
                Some(fcvoper::obj_seq_offset_and_scale(m, t, gn, e, five(c), a("with_act") as i16, a("chap"), a("index")) as i32 as u32)
            })
        },
        "objSeqOffsetAndScaleHier" => |c| {
            run(c, |m, t, i| {
                let a = |n: &str| int_arg(c, n);
                let (gn, e) = (ptr_arg(c, i, "gn"), ptr_arg(c, i, "e"));
                Some(fcvoper::obj_seq_offset_and_scale_hier(m, t, gn, e, five(c), a("with_act") as i16, a("chap"), a("index")) as i32 as u32)
            })
        },
        "SI_sequenceOffsetAndScale" => |c| {
            run(c, |m, t, _| {
                let a = |n: &str| int_arg(c, n);
                let f = |n: &str| c.input(n).f32();
                fcvoper::sequence_offset_and_scale(m, t, a("all") as i16, a("with_act") as i16, a("start"), a("pivot"), a("end"), f("offset"), f("scale"));
                None
            })
        },
        // sixth pass: animation files, the two tests, the PostScript names
        "si_animOpCheck" => |c| run(c, |m, t, _| Some(assoc::anim_op_check(m, t) as u32)),
        "SI_warningIfPlottingRequired" => |c| run(c, |m, t, i| Some(assoc::warning_if_plotting_required(m, t, ptr_arg(c, i, "d")) as i32 as u32)),
        "SI_postscriptPreview" => |c| {
            run(c, |m, t, _| {
                assoc::postscript_preview(m, t, int_arg(c, "on") as i16);
                None
            })
        },
        "si_postscriptDumpFrame" => |c| {
            // A frame number of eleven characters: the original's name ends one byte behind
            // its block, where the port does not write. When that byte is the first of the
            // next block's header the two texts handed to `SI_postscriptRefresh` differ by
            // it; the case is compared on everything else and counted as not compared.
            let overrun = int_arg(c, "frame") <= -1_000_000_000;
            let v = run(c, |m, t, _| {
                assoc::postscript_dump_frame(m, t, int_arg(c, "frame"));
                if overrun {
                    TEXT_ERR.with(|e| e.borrow_mut().take());
                }
                None
            });
            match v {
                Verdict::Same if overrun => Verdict::Skip,
                v => v,
            }
        },
        "elemdictStoreAnimInHastruct" => |c| {
            run(c, |m, t, i| {
                let (d, ha, e, count) = (ptr_arg(c, i, "d"), ptr_arg(c, i, "ha"), ptr_arg(c, i, "e"), ptr_arg(c, i, "count"));
                assoc::elemdict_store_anim_in_hastruct(m, t, d, ha, e, int_arg(c, "parent"), count);
                None
            })
        },
        "animRemoveFromHierarchy" => |c| {
            run(c, |m, t, i| {
                let (d, e) = (ptr_arg(c, i, "d"), ptr_arg(c, i, "e"));
                assoc::anim_remove_from_hierarchy(m, t, d, int_arg(c, "chap"), e, int_arg(c, "index"), int_arg(c, "del") as i16);
                None
            })
        },
        "animationAssociate" => |c| {
            run(c, |m, t, i| {
                let (e, ae) = (ptr_arg(c, i, "e"), ptr_arg(c, i, "ae"));
                Some(assoc::animation_associate(m, t, int_arg(c, "chap"), e, int_arg(c, "index"), ae, int_arg(c, "ai")) as u32)
            })
        },
        "animationDisassociate" => |c| {
            run(c, |m, t, i| {
                let (e, ae, picked) = (ptr_arg(c, i, "e"), ptr_arg(c, i, "ae"), ptr_arg(c, i, "picked"));
                Some(assoc::animation_disassociate(m, t, int_arg(c, "chap"), e, int_arg(c, "index"), ae, int_arg(c, "ai"), picked) as u32)
            })
        },
        "si_animDialAssociate" => |c| {
            run(c, |m, t, i| {
                assoc::anim_dial_associate(m, t, ptr_arg(c, i, "event"), int_arg(c, "associate") as i16);
                None
            })
        },
        // sixth pass: the playback command
        "SI_playbackRun" => |c| {
            // Under adaptive step the original never sets the step it hands to
            // `playBackFrames`. A turn of playback rewrites it before it is added, except
            // with a negative `SI_framePause` (such a turn draws nothing and rewrites
            // nothing); and a ping-pong loop that starts outside its range negates it before
            // any turn. What is used then is a stale word of the command's own stack, the
            // same in both runs of the harness, so the harness does not mark the case: not
            // compared, whatever the port (whose step is 0 there) makes of it.
            let word = |name: &str, i: usize| c.try_input(name).and_then(|f| f.words().get(i).and_then(|w| w.value())).map_or(0, |v| v as u32);
            let (adaptive, pause) = (word("g_adaptive", 0) as u16, word("g_frame", 2) as i32);
            let (mode, pingpong) = (int_arg(c, "mode"), int_arg(c, "pingpong") as i16);
            if adaptive != 0 && matches!(mode, 0 | 1 | 3 | 4) && (pause < 0 || (matches!(mode, 1 | 4) && pingpong != 0)) {
                return Verdict::Skip;
            }
            run(c, |m, t, _| {
                playrun::playback_run(m, t, mode, pingpong, int_arg(c, "from_current") as i16);
                None
            })
        },
        _ => return None,
    })
}

/// The five `float` arguments `a0` … `a4` of a case, bit for bit.
fn five(c: &Case) -> [f32; 5] {
    ["a0", "a1", "a2", "a3", "a4"].map(|n| c.input(n).f32())
}

fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "animScalingUpdate" | "animScalingUpdate#wave" => |c| node_case(c, |m, t, n, x| transfo::anim_scaling_update(m, t, n, x)),
        "animRotationUpdate" | "animRotationUpdate#wave" => |c| node_case(c, |m, t, n, x| transfo::anim_rotation_update(m, t, n, x)),
        "animExplicitPosUpdate" | "animExplicitPosUpdate#wave" => |c| node_case(c, |m, t, n, x| transfo::anim_explicit_pos_update(m, t, n, x)),
        "animCameraExplicitPosUpdate" => |c| node_case(c, |m, _, n, x| params::anim_camera_explicit_pos_update(m, n, x)),
        "animLightExplicitPosUpdate" => |c| node_case(c, |m, _, n, x| params::anim_light_explicit_pos_update(m, n, x)),
        "animMatParametersUpdate" => |c| node_case(c, |m, _, n, x| params::anim_mat_parameters_update(m, n, x)),
        "animTexParametersUpdate" => |c| node_case(c, |m, t, n, x| params::anim_tex_parameters_update(m, t, n, x)),
        "animTex3DParametersUpdate" => |c| node_case(c, |m, t, n, x| params::anim_tex3d_parameters_update(m, t, n, x)),
        "animFadParametersUpdate" => |c| node_case(c, |m, _, n, x| params::anim_fad_parameters_update(m, n, x)),
        "animFogParametersUpdate" => |c| node_case(c, |m, _, n, x| params::anim_fog_parameters_update(m, n, x)),
        "animCameraParametersUpdate" => |c| node_case(c, |m, _, n, x| params::anim_camera_parameters_update(m, n, x)),
        "animLightParametersUpdate" => |c| node_case(c, |m, t, n, x| params::anim_light_parameters_update(m, t, n, x)),
        "animModelParametersUpdate" => |c| node_case(c, |m, t, n, x| params::anim_model_parameters_update(m, t, n, x)),
        "animCameraConstraintUpdate" => |c| node_case(c, |m, t, n, x| params::anim_camera_constraint_update(m, t, n, x)),
        "animLightConstraintUpdate" => |c| node_case(c, |m, t, n, x| params::anim_light_constraint_update(m, t, n, x)),
        "updNodeParameterUpdate" => |c| node_case(c, |m, t, n, x| update::upd_node_parameter_update(m, t, n, x)),
        "updNodeTransformUpdate" | "updNodeTransformUpdate#wave" => |c| node_case(c, |m, t, n, x| update::upd_node_transform_update(m, t, n, x)),
        "updNodeGeometryUpdate" => |c| {
            let mut img = match image(c) {
                Ok(i) => i,
                Err(e) => return Verdict::Different(e),
            };
            let mut t = host(c);
            let (node, post) = (at(&img.lay, "node"), at(&img.lay, "post"));
            let s = before(&img);
            geom::upd_node_geometry_update(&mut img.mem, &mut t, node, time(c), post);
            mutate(&mut img, s);
            compare(c, &mut img, &t)
        },
        "SI_checkActiveTime" => |c| {
            let mut img = match image(c) {
                Ok(i) => i,
                Err(e) => return Verdict::Different(e),
            };
            let mut t = host(c);
            let (node, out) = (at(&img.lay, "node"), at(&img.lay, "out"));
            let s = before(&img);
            let r = update::check_active_time(&mut img.mem, &mut t, node, time(c));
            if let Some(v) = r {
                img.mem.set_f32(out, v);
            }
            mutate(&mut img, s);
            // a refused node: the original has or has not stored the time by then, the port
            // does not say; the stored time is compared only for a node that passes
            let ax = c.output("ax").words()[0].value().unwrap_or(0) as u16;
            compare_but(c, &mut img, &t, if r.is_some() { "" } else { "out" })
                .and(|| Verdict::from((ax != 0) == r.is_some(), || format!("result: port {r:?}, original {ax}")))
        },
        "SI_activationFcvCheckTime" => |c| {
            let mut img = match image(c) {
                Ok(i) => i,
                Err(e) => return Verdict::Different(e),
            };
            let t = host(c);
            let s = before(&img);
            let (out, on) = update::activation_fcv_check_time(&img.mem, at(&img.lay, "curve"), time(c));
            let o = at(&img.lay, "out");
            img.mem.set_f32(o, out);
            mutate(&mut img, s);
            compare(c, &mut img, &t).and(|| {
                let eax = c.output("eax").u32();
                Verdict::from(eax == on as u32, || format!("result: port {on}, original {eax}"))
            })
        },
        "si_modelComputePathTranslation" => |c| {
            let mut img = match image(c) {
                Ok(i) => i,
                Err(e) => return Verdict::Different(e),
            };
            let t = host(c);
            let (mdl, path, pt) = (at(&img.lay, "mdl"), at(&img.lay, "path"), at(&img.lay, "pt"));
            let s = before(&img);
            transfo::model_compute_path_translation(&mut img.mem, mdl, path, pt);
            mutate(&mut img, s);
            compare(c, &mut img, &t)
        },
        "SI_constrLocation" => |c| {
            let mut img = match image(c) {
                Ok(i) => i,
                Err(e) => return Verdict::Different(e),
            };
            let t = host(c);
            let (obj, out) = (at(&img.lay, "obj"), at(&img.lay, "out"));
            let s = before(&img);
            transfo::constr_location(&mut img.mem, obj, c.input("kind").i32(), out);
            mutate(&mut img, s);
            compare(c, &mut img, &t)
        },
        "fastModelUpdateSonState" => |c| {
            let mut img = match image(c) {
                Ok(i) => i,
                Err(e) => return Verdict::Different(e),
            };
            let mut t = host(c);
            let (root, nodes) = (at(&img.lay, "root"), at(&img.lay, "nodes"));
            let s = before(&img);
            transfo::fast_model_update_son_state(&mut img.mem, &mut t, root, nodes);
            mutate(&mut img, s);
            compare(c, &mut img, &t)
        },
        _ => return lookup2(name).or_else(|| lookup3(name)).or_else(|| lookup4(name)).or_else(|| lookup5(name)),
    })
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: motion-emucheck <directory of .vec files>");
        std::process::exit(2);
    });
    let _ = geom::SI_DBL_IDENTITY;
    // one sub-directory per routine, read one at a time (a routine's vectors are large)
    let mut subdirs: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| {
            eprintln!("motion-emucheck: {dir}: {e}");
            std::process::exit(2);
        })
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();
    if subdirs.is_empty() {
        subdirs.push(Path::new(&dir).to_path_buf());
    }
    let mut rows = Vec::new();
    for d in subdirs {
        let second = d.file_name().and_then(|n| n.to_str()).is_some_and(|n| (lookup2(n).is_some() || lookup3(n).is_some() || lookup4(n).is_some() || lookup5(n).is_some()) && n != "animAllTypePathUpdate");
        match if second { check_dir_with(&d, &lookup, true) } else { check_dir(&d, &lookup) } {
            Ok(r) => rows.extend(r),
            Err(e) => {
                eprintln!("motion-emucheck: {}: {e}", d.display());
                std::process::exit(2);
            }
        }
    }
    let clean = print_report(&rows);
    let n = |c: &AtomicUsize| c.load(Ordering::Relaxed);
    println!(
        "cases loaded: {} by si_mem::load_case with every buffer at its recorded address, {} by Mem::place_at",
        n(&BY_LOAD_CASE),
        n(&BY_PLACE_AT)
    );
    println!("pointers into blocks the routine allocated, compared as the numbers the original had: {}", n(&HEAP_PTRS));
    println!(
        "cases whose vector carries the original's blocks: {}; the port's heap the same block for block (addresses, sizes, order, frees, every byte the original wrote): {}",
        n(&WHOLE_HEAP_CASES),
        n(&WHOLE_HEAP_SAME)
    );
    if mutating() {
        println!(
            "EMUCHECK_MUTATE: a bit was flipped in {} cases; in {} of them memory and calls still compared equal",
            n(&MUTATED),
            n(&SURVIVED)
        );
    }
    std::process::exit(if clean { 0 } else { 1 });
}
