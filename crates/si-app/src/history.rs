// SPDX-License-Identifier: MIT
//! The undo/redo history of the editor, on the shared image (finding 0036, `tab1.c`): the
//! list of recorded changes (`historyNode` records, 0x20 bytes each), the snapshots a change
//! takes of a model, a camera, a light, a spreadsheet cell, a function curve, an audio
//! track or an action list, and the dialog that sets its depth.
//!
//! Everything works on the image at the original's layout (finding 0024): the history
//! header `SI_HistoryStruct` (0x20 bytes) is where `pCurrentHst` points, a node is
//! `calloc`ed by `SI_calloc`, and what is not in this file (the disposal and copy routines
//! of the geometry, the dictionary, the dialogs) is called through [`HistoryCalls`].
//!
//! # The header (`pCurrentHst`)
//! +0x00 the head of the cluster being built (a node), +0x04 `short` cluster open, +0x06
//! `short` history on, +0x08 the oldest node, +0x0c the count, +0x10 the longest history
//! (5 at start), +0x14 the redo position, +0x18 the newest node, +0x1c the undo position.
//! # A node
//! +0x00 the graph node it belongs to, +0x08 the kind, +0x0c its data, +0x10 and +0x14 the
//! cluster chain (next and back), +0x18 next and +0x1c previous in the list.
//! Kinds (read from `historyNodeDispose` and the savers): 0 transform, 1 geometry, 2 camera,
//! 3 light, 4 deleted graph node, 5 added graph node, 6 and 7 unlink and link, 8 function
//! curve, 10 spreadsheet cell, 11 audio markers, 12 actions, 13 motion-stractions.

use crate::image::g as ig;
use si_mem::{outside_calls, Mem, Outside};

outside_calls! {
    /// What the history calls that is not in this file: the geometry's disposal and copy
    /// routines, the dictionary, the graph lists, the dialogs, the refresh. Answered 0 until
    /// their own files are ported.
    pub trait HistoryCalls {
        // ---- disposal
        fn si_fcurve_remove(pfcurve: u32) = "SI_fcurveRemove", 0x081ff0a0;
        fn si_grafnode_dispose(pnode: u32) = "SI_grafnodeDispose", 0x0832ca54;
        fn si_audio_marker_mng_markers_dispose(markers: u32) = "SI_audioMarkerMngMarkersDispose", 0x08375884;
        fn si_actions_dispose(actions: u32) = "SI_actionsDispose", 0x081df310;
        fn si_mesh_dispose(pmesh: u32) = "SI_meshDispose", 0x084404a0;
        fn si_cluster_data_dispose(pdata: u32) = "SI_clusterDataDispose", 0x081e7f0c;
        fn si_patch_dispose(ppatch: u32) = "SI_patchDispose", 0x0847a530;
        fn si_subel_user_data_dispose(pklusters: u32, pnodes: u32) = "SI_subelUserDataDispose", 0x084631a4;
        fn si_spline_dispose(pspline: u32) = "SI_splineDispose", 0x0848ab48;
        fn si_lattice_dispose(plattice: u32) = "SI_latticeDispose", 0x0843dff0;
        fn si_nurbs_crv_dispose(pcrv: u32) = "SI_nurbsCrvDispose", 0x084db870;
        fn si_face_dispose(pface: u32) = "SI_faceDispose", 0x08434828;
        fn si_nurbs_srf_dispose(psrf: u32) = "SI_nurbsSrfDispose", 0x084dba5c;
        // ---- copies
        fn si_face_copy(face: u32) -> u32 = "SI_faceCopy", 0x08434990;
        fn si_patch_copy(patch: u32) -> u32 = "SI_patchCopy", 0x0847a2b8;
        fn si_mesh_copy(mesh: u32) -> u32 = "SI_meshCopy", 0x084404e0;
        fn si_metaball_copy(ball: u32, model: u32) -> u32 = "SI_metaballCopy", 0x0845ab50;
        fn si_spline_copy(spline: u32) -> u32 = "SI_splineCopy", 0x0848aa74;
        fn si_nurbs_crv_copy(crv: u32) -> u32 = "SI_nurbsCrvCopy", 0x084dbbf4;
        fn si_nurbs_srf_copy(srf: u32) -> u32 = "SI_nurbsSrfCopy", 0x084dc1a8;
        fn si_lattice_copy(lattice: u32) -> u32 = "SI_latticeCopy", 0x0843dd34;
        fn si_matrix_copy(src: u32, dst: u32) = "SI_matrixCopy", 0x08516224;
        fn si_actions_copy_all(dst: u32, src: u32) = "SI_actionsCopyAll", 0x081e2810;
        fn si_modelkluster_list_duplicate(model: u32, a: i32, b: i32, c: i32) -> u32 = "SI_modelklusterListDuplicate", 0x0843cc70;
        fn si_subel_user_data_copy_from_setup(klusters: u32, nodes: u32, pk: u32, pn: u32) = "SI_subelUserDataCopyFromSetup", 0x08463260;
        // ---- fittings and lattices of a model
        fn si_model_node_spline_fitting(model: u32) -> u32 = "SI_modelNodeSplineFitting", 0x08472b88;
        fn si_model_tree_spline_fitting(model: u32) -> u32 = "SI_modelTreeSplineFitting", 0x08472ba4;
        fn si_model_node_patch_fitting(model: u32) -> u32 = "SI_modelNodePatchFitting", 0x08472c88;
        fn si_model_branch_patch_fitting(model: u32) -> u32 = "SI_modelBranchPatchFitting", 0x08472ca4;
        fn si_model_node_lattice(model: u32) -> u32 = "SI_modelNodeLattice", 0x0843ff68;
        fn si_model_tree_lattice(model: u32) -> u32 = "SI_modelTreeLattice", 0x0843ff88;
        // ---- the dictionary of elements
        fn si_ed_element_att_value_get(elem: u32, index: u32, value: u32) -> i16 = "SI_edElementAttValueGet", 0x080f00f0;
        fn si_ed_element_att_value_set(elem: u32, index: u32, value: u32) = "SI_edElementAttValueSet", 0x080f0184;
        fn si_elemdict_find_elem_by_data(dict: u32, cid: u32, data: u32) -> u32 = "SI_elemdictFindElemByData", 0x08134364;
        fn si_elemdict_find_elem_from_grafnode(gn: u32, pa: u32, pb: u32) -> u32 = "SI_elemdictFindElemFromGrafNode", 0x080fcbec;
        fn si_elemdict_find_range_related_elem(dict: u32, elem: u32, a: i32, b: i32, lo: u32, hi: u32, cid: u32, pa: u32, pb: u32) -> u32 = "SI_elemdictFindRangeRelatedElem", 0x081353c4;
        fn si_ed_check_for_uneditable(elem: u32, cid: u32) -> i16 = "SI_edCheckForUneditable", 0x08107a44;
        fn si_ed_set_uneditable(dict: u32, elem: u32) = "SI_edSetUneditable", 0x081074c0;
        fn si_ed_find_son(elem: u32) -> u32 = "SI_edFindSon", 0x080fcec0;
        fn si_ed_find_next_son(elem: u32) -> u32 = "SI_edFindNextSon", 0x080fcef0;
        // ---- function curves
        fn si_fcurve_allocate(a: u32, b: u32) -> u32 = "SI_fcurveAllocate", 0x0820a810;
        fn si_fcurve_transfer(src: u32, dst: u32) = "SI_fcurveTransfer", 0x081ff0e4;
        // ---- the graph lists, the models and the matrices (links and transforms)
        fn si_find_gl_data(list: u32, b: u32, a: u32) -> u32 = "SI_findGlData", 0x0832ee68;
        fn si_matrix_multiply_sub3(a: u32, b: u32, c: u32) = "SI_matrixMultiplySub3", 0x0851650c;
        fn si_matrix_extract_xyz_rotation(mat: u32, rot: u32) = "SI_matrixExtractXyzRotation", 0x085172b0;
        fn si_matrix_inverse(src: u32, dst: u32) = "SI_matrixInverse", 0x0851672c;
        fn si_matrix_xyz_rotation(rot: u32, mat: u32) = "SI_matrixXyzRotation", 0x0851882c;
        fn si_dyna_phy_props_set_new_root(model: u32) = "SI_dynaPhyPropsSetNewRoot", 0x084a15a4;
        fn si_dyna_phy_props_validate_parent(parent: u32, child: u32) -> i32 = "SI_dynaPhyPropsValidateParent", 0x084a1794;
        fn si_model_cut_father(model: u32) = "SI_modelCutFather", 0x0846d790;
        fn si_model_update_dependency(model: u32, a: u32) = "SI_modelUpdateDependency", 0x0846f200;
        fn si_model_readjust_definition(model: u32) = "SI_modelReadjustDefinition", 0x0846e340;
        fn si_model_next_son(model: u32, son: u32) -> u32 = "SI_modelNextSon", 0x0846d70c;
        fn si_model_update_son_state(father: u32, model: u32, a: i32) = "SI_modelUpdateSonState", 0x084662f8;
        fn si_graflist_cut_model(list: u32, gn: u32, model: u32, a: i32) = "SI_graflistCutModel", 0x0833562c;
        fn si_graflist_link_model(list: u32, gn_parent: u32, gn: u32, a: i32, b: i32) = "SI_graflistLinkModel", 0x083353fc;
        fn si_model_set_rotation(model: u32, rot: u32, a: i32, b: i32) = "SI_modelSetRotation", 0x08463f54;
        fn si_inv_kin_reset_pref_angle(model: u32, a: i32, x: u32, y: u32, z: u32) = "SI_invKinResetPrefAngle", 0x084adcec;
        fn si_inv_kin_set_goal(model: u32, x: u32, y: u32, z: u32) = "SI_invKinSetGoal", 0x084ae884;
        fn si_model_compensate_matrix(model: u32, flag: i32, a: i32) = "SI_modelCompensateMatrix", 0x08465660;
        fn si_fitting_update_matrix(fit: u32) = "SI_fittingUpdateMatrix", 0x0843a264;
        fn si_spline_fit_touch(model: u32, tree: i16) = "SI_splineFitTouch", 0x081535c8;
        fn si_pfitting_update_matrix(fit: u32) = "SI_pfittingUpdateMatrix", 0x0843a484;
        fn si_patch_fit_touch(model: u32, tree: i16) = "SI_patchFitTouch", 0x08153578;
        // ---- the dialog of the set-up
        fn si_dialog_open(id: i32) -> u32 = "SI_dialogOpen", 0x083859b8;
        fn si_dialog_draw(dialog: u32) = "SI_dialogDraw", 0x08383b94;
        fn si_dialog_get_event(dialog: u32) -> i16 = "SI_dialogGetEvent", 0x08385e3c;
        fn si_dialog_close(dialog: u32) = "SI_dialogClose", 0x08385b28;
        fn si_set_state_item(dialog: u32, item: i32, state: i32) = "SI_setStateItem", 0x0838b824;
        fn si_get_texed_long(dialog: u32, item: i32) -> i32 = "SI_getTexedLong", 0x0838c204;
        fn si_set_texed(dialog: u32, item: i32, text: u32) = "SI_setTexed", 0x0838bd10;
        // ---- undo and redo (`SI_historyUndoRedo`)
        fn si_audio_mng_at_find_by_id(container: u32, id: u32) -> u32 = "SI_audioMngAtFindById", 0x08375fdc;
        fn si_audio_marker_mng_count_selected(track: u32) -> i32 = "SI_audioMarkerMngCountSelected", 0x0837581c;
        fn si_elemdict_get_elem(dict: u32, cid: u32, data: u32) -> u32 = "SI_elemdictGetElem", 0x08132b50;
        fn si_actions_unselect_all(actions: u32) = "SI_actionsUnselectAll", 0x081e27cc;
        fn si_refresh_counter() = "SI_refreshCounter", 0x080d1d20;
        fn si_wrath_unsupported_mdl_type(model: u32) -> i16 = "SI_wrathUnsupportedMdlType", 0x08107b68;
        fn si_wrath_unsupported_mdl_type2(a: u32, b: u32) -> i16 = "SI_wrathUnsupportedMdlType2", 0x08107b90;
        fn si_model_swap_geometry(model: u32, rec: u32, a: u32, b: u32, c: u32) = "SI_modelSwapGeometry", 0x084675f0;
        fn si_patch_zip_update_index(model: u32) = "SI_patchZipUpdateIndex", 0x081a3e00;
        fn si_update_gn_func(gn: u32) = "SI_updateGnFunc", 0x08176618;
        fn si_ed_set_editable(dict: u32, elem: u32, cid: u32) = "SI_edSetEditable", 0x08107764;
        fn si_model_swap_lattice(model: u32, tree: i32, count: i32, arr: u32) = "SI_modelSwapLattice", 0x08440174;
        fn si_find_cluster(id: u32, list: u32, model: u32) -> u32 = "SI_findCluster", 0x0843d828;
        fn si_ed_add_cluster(model: u32, kluster: u32) = "SI_edAddCluster", 0x08103510;
        fn si_kluster_dispose_flagged(model: u32) = "SI_klusterDisposeFlagged", 0x0843b60c;
        fn si_cluster_model_update(model: u32) = "SI_clusterModelUpdate", 0x081f1a1c;
        fn si_graflist_add_data(list: u32, b: u32, a: u32, flag: i32) -> u32 = "SI_graflistAddData", 0x0832d240;
        fn si_schematic_adjust_new_node(list: u32, gn: u32) = "SI_schematicAdjustNewNode", 0x0834fe20;
        fn si_ed_graf_node_set_delete_flag(gn: u32, a: i32, new: u32, b: i32) = "SI_edGrafNodeSetDeleteFlag", 0x080fdcf4;
        fn si_graflist_delete_grafnode(list: u32, gn: u32) = "SI_graflistDeleteGrafnode", 0x0832cd50;
        fn si_fcurve_initialize(fc: u32) = "SI_fcurveInitialize", 0x0820a9d0;
        fn si_fcurve_remove_all_keys(fc: u32) = "SI_fcurveRemoveAllKeys", 0x081ff8bc;
        fn si_scene_key_edit(a: i32, frame: i32, b: i32) = "SI_sceneKeyEdit", 0x0821d17c;
        fn si_sprd_refresh_sprd_view(v: u32) = "SI_sprdRefreshSprdView", 0x085aa6ec;
        fn si_update_geometry_relation() -> i16 = "SI_updateGeometryRelation", 0x0822b18c;
        fn si_refresh_system(port: u32, list: u32, what: i32) = "SI_refreshSystem", 0x08323d44;
        fn si_scene_refresh() = "SI_sceneRefresh", 0x0820f470;
    }
}

/// The globals of `tab1.c` and the ones its routines read.
pub mod g {
    si_mem::globals! {
        /// `pCurrentHst`: the history being worked on; at start `factoryHst`.
        P_CURRENT_HST: u32 = 0x086791c0, "pCurrentHst", init 0x086791a0;
        /// `SI_cid.ANIMATION` and `SI_cid.MODELS`: class ids of the element dictionary.
        SI_CID_ANIMATION: u32 = 0x08781660, "SI_cid";
        SI_CID_MODELS: u32 = 0x08781680, "";
        /// `_SI_temp_`: the buffer `sprintf` composes the dialog's depth text in.
        SI_TEMP: [u8; 0x1e] = 0x08780f00, "_SI_temp_";
        /// `SI_hrchCpst`: positions kept in the world when a model is re-parented.
        SI_HRCH_CPST: i16 = 0x0867949c, "SI_hrchCpst", init 1;
        /// `SI_historyActive`: set while an undo or redo runs.
        SI_HISTORY_ACTIVE: i16 = 0x0867965c, "SI_historyActive";
        /// `SI_addInDictionary`: whether a graph node added is entered in the dictionary.
        SI_ADD_IN_DICTIONARY: i16 = 0x086792b0, "SI_addInDictionary", init 1;
        /// `PREF_CONFIRM_KEYEDIT` (0x08781484, named in finding 0019 from `setup.rs`).
        PREF_CONFIRM_KEYEDIT: i16 = 0x08781484, "";
        /// `SI_frameCurrent`.
        SI_FRAME_CURRENT: i32 = 0x08782040, "SI_frameCurrent";
        /// The audio track being shown (a `SI_audioContainer` entry) and whether it has marked
        /// markers [guess: names from use].
        AUDIO_SHOWN_TRACK: u32 = 0x087fba3c, "";
        AUDIO_HAS_SELECTED: i16 = 0x087fba54, "";
        /// The action list shown and its selection word [guess: names from use].
        ACTIONS_SELECTION: u32 = 0x0878fb0c, "";
        ACTIONS_SHOWN: u32 = 0x0878fb10, "";
    }
}

/// The header `factoryHst` (0x086791a0): what `pCurrentHst` points at before a history is
/// made. Its bytes are those of the binary's `.data`.
pub const FACTORY_HST: u32 = 0x086791a0;
const FACTORY_HST_BYTES: [u8; 0x20] = [
    0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Writes `factoryHst` and `pCurrentHst` as the program starts.
pub fn init_data(m: &mut Mem) {
    m.set_bytes(FACTORY_HST, &FACTORY_HST_BYTES);
    si_mem::init_globals(m, g::ALL);
    m.put_cstr(0x0867_921f, b"1");
}

/// Offsets in the header (`SI_HistoryStruct`).
pub mod hst {
    pub const CLUSTER_HEAD: u32 = 0x00;
    pub const CLUSTER_OPEN: u32 = 0x04;
    pub const ACTIVE: u32 = 0x06;
    pub const HEAD: u32 = 0x08;
    pub const COUNT: u32 = 0x0c;
    pub const MAX: u32 = 0x10;
    pub const REDO: u32 = 0x14;
    pub const TAIL: u32 = 0x18;
    pub const UNDO: u32 = 0x1c;
}

/// Offsets in a node (`historyNode`, 0x20 bytes).
pub mod node {
    pub const GN: u32 = 0x00;
    pub const KIND: u32 = 0x08;
    pub const DATA: u32 = 0x0c;
    pub const CHAIN_NEXT: u32 = 0x10;
    pub const CHAIN_BACK: u32 = 0x14;
    pub const NEXT: u32 = 0x18;
    pub const PREV: u32 = 0x1c;
}


fn hst(m: &Mem) -> u32 {
    m[g::P_CURRENT_HST]
}

/// The element dictionary of the current process: `SI_elemDicts + 0x70 * SI_procData[0]`
/// (the original computes it as `(n * 8 - n) * 0x10`).
pub(crate) fn elem_dict(m: &Mem) -> u32 {
    let n = m.u32(m[ig::SI_PROC_DATA]);
    (n.wrapping_mul(8).wrapping_sub(n)).wrapping_mul(0x10).wrapping_add(m[ig::SI_ELEM_DICTS])
}

/// What `fld dword; fstp dword` does to a `float`'s bits: a signalling NaN comes out quiet
/// (the invalid-operation exception is masked), every other value is unchanged.
pub(crate) fn x87_move(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// `fld [src]; fstp [dst]` on a `float`.
pub(crate) fn fcopy(m: &mut Mem, dst: u32, src: u32) {
    let v = x87_move(m.u32(src));
    m.set_u32(dst, v);
}

/// Two `float`s exchanged through the x87 unit (`fld a; fld b; fstp a; fstp b`).
fn fswap(m: &mut Mem, a: u32, b: u32) {
    let (x, y) = (x87_move(m.u32(a)), x87_move(m.u32(b)));
    m.set_u32(a, y);
    m.set_u32(b, x);
}

/// Two words exchanged through registers.
fn swap(m: &mut Mem, a: u32, b: u32) {
    let (x, y) = (m.u32(a), m.u32(b));
    m.set_u32(a, y);
    m.set_u32(b, x);
}

/// A fresh node (`SI_calloc(1, 0x20)` and the four link words set to 0); 0 when the
/// allocation fails.
fn new_node(m: &mut Mem) -> u32 {
    let n = m.si_calloc(1, 0x20);
    if n != 0 {
        m.set_u32(n + node::PREV, 0);
        m.set_u32(n + node::NEXT, 0);
        m.set_u32(n + node::CHAIN_NEXT, 0);
        m.set_u32(n + node::CHAIN_BACK, 0);
    }
    n
}

// ---------------------------------------------------------------------------- the header

/// [soft 0x080ca848 SI_initHistoryStruct]: a header made empty (history on, depth 5), unless
/// it is null or `factoryHst`; 1 when it was.
pub fn init_history_struct(m: &mut Mem, h: u32) -> i32 {
    if h == 0 || h == FACTORY_HST {
        return 0;
    }
    m.set_u32(h + hst::CLUSTER_HEAD, 0);
    m.set_i16(h + hst::CLUSTER_OPEN, 0);
    m.set_i16(h + hst::ACTIVE, 1);
    m.set_u32(h + hst::HEAD, 0);
    m.set_u32(h + hst::COUNT, 0);
    m.set_u32(h + hst::MAX, 5);
    m.set_u32(h + hst::REDO, 0);
    m.set_u32(h + hst::TAIL, 0);
    m.set_u32(h + hst::UNDO, 0);
    1
}

/// [soft 0x080ca804 SI_createHistoryStruct]: a new empty header; null when it cannot be had.
pub fn create_history_struct(m: &mut Mem) -> u32 {
    let h = m.si_calloc(1, 0x20);
    if h != 0 && init_history_struct(m, h) as i16 == 0 {
        m.free(h);
        return 0;
    }
    h
}

/// [soft 0x080ca8a8 SI_destroyHistoryStruct]: frees a header (never `factoryHst` or null);
/// 1 when it did. The nodes are not touched.
pub fn destroy_history_struct(m: &mut Mem, h: u32) -> i32 {
    if h == 0 || h == FACTORY_HST {
        return 0;
    }
    m.free(h);
    1
}

/// [soft 0x080ca8e4 SI_getCurrentHistoryStruct]: `pCurrentHst`.
pub fn get_current_history_struct(m: &Mem) -> u32 {
    hst(m)
}

/// [soft 0x080ca8f0 SI_setCurrentHistoryStruct]: makes `h` current unless it is null; 1 when
/// it did.
pub fn set_current_history_struct(m: &mut Mem, h: u32) -> i32 {
    if h == 0 {
        return 0;
    }
    m[g::P_CURRENT_HST] = h;
    1
}

/// [soft 0x080ca7f0 SI_historySetActive]: history on or off.
pub fn history_set_active(m: &mut Mem, on: i16) {
    let h = hst(m);
    m.set_i16(h + hst::ACTIVE, on);
}

/// [soft 0x080c7270 SI_historyOpenCluster]: starts a cluster (several changes undone
/// together) unless one is open; 1 when it did.
pub fn history_open_cluster(m: &mut Mem) -> i32 {
    let h = hst(m);
    if m.i16(h + hst::CLUSTER_OPEN) == 1 {
        return 0;
    }
    m.set_u32(h + hst::CLUSTER_HEAD, 0);
    m.set_i16(h + hst::CLUSTER_OPEN, 1);
    1
}

/// [soft 0x080c7298 SI_historyCloseCluster]: ends the open cluster; 1 when one was open.
pub fn history_close_cluster(m: &mut Mem) -> i32 {
    let h = hst(m);
    if m.i16(h + hst::CLUSTER_OPEN) == 0 {
        return 0;
    }
    m.set_i16(h + hst::CLUSTER_OPEN, 0);
    1
}

/// [soft 0x080c9824 SI_historyDump]: prints the header and every node, newest last.
pub fn history_dump(m: &mut Mem, o: &mut dyn Outside) {
    let h = hst(m);
    let (head, tail) = (m.u32(h + hst::HEAD), m.u32(h + hst::TAIL));
    o.print(1, format!("Head = 0x{head:x}; Tail = 0x{tail:x}\n").as_bytes());
    let h = hst(m);
    let (undo, redo) = (m.u32(h + hst::UNDO), m.u32(h + hst::REDO));
    o.print(1, format!("Undo = 0x{undo:x}; Redo = 0x{redo:x}\n").as_bytes());
    let mut n = m.u32(hst(m) + hst::HEAD);
    while n != 0 {
        let next = m.u32(n + node::NEXT);
        o.print(1, format!("ptr = 0x{n:x}\t").as_bytes());
        o.print(1, format!("gn = 0x{:x}\t", m.u32(n + node::GN)).as_bytes());
        o.print(1, format!("type = {}\n", m.i32(n + node::KIND)).as_bytes());
        o.print(1, b"\n");
        n = next;
    }
    o.print(1, b"\n\n");
}

// ---------------------------------------------------------------------------- disposal

/// [soft 0x080c8cc8 geoSubsDispose]: frees a geometry record (`**p` is the record: +0 the
/// model kind, +4 the target (0 and 1 geometry, 2 and 3 lattices, 4 clusters), +8 the count
/// of shapes, +0xc the array of shapes, +0x10 one more shape) and clears `*p`; always 1.
pub fn geo_subs_dispose(m: &mut Mem, o: &mut dyn Outside, p: u32) -> i32 {
    let rec = m.u32(p);
    if rec == 0 {
        return 1;
    }
    // `Some(true)`: free the array at +0xc after the target's work; `None`: go straight to the end
    let mut free_array = false;
    match m.u32(rec + 4) {
        0 | 1 => {
            if m.i16(rec + 8) >= 1 {
                let kind = m.u32(rec);
                // (kind) -> dispose routine on the address of a cell
                let each = |m: &mut Mem, o: &mut dyn Outside, cell: u32| match kind {
                    1 => Some(o.si_patch_dispose(m, cell)),
                    2 => Some(o.si_face_dispose(m, cell)),
                    4 => Some(o.si_mesh_dispose(m, cell)),
                    6 => Some(o.si_spline_dispose(m, cell)),
                    9 => Some(o.si_nurbs_crv_dispose(m, cell)),
                    10 => Some(o.si_nurbs_srf_dispose(m, cell)),
                    _ => None,
                };
                if matches!(kind, 1 | 2 | 4 | 6 | 9 | 10) {
                    let mut i: i16 = 0;
                    while i < m.i16(m.u32(p) + 8) {
                        let cell = m.u32(m.u32(p) + 0xc).wrapping_add((i as i32 as u32).wrapping_mul(4));
                        each(m, o, cell);
                        i = i.wrapping_add(1);
                    }
                    let r = m.u32(p);
                    if m.u32(r + 0x10) != 0 {
                        each(m, o, r + 0x10);
                    }
                }
                free_array = true;
            }
        }
        2 | 3 => {
            if m.i16(rec + 8) >= 1 {
                let mut i: i16 = 0;
                while i < m.i16(m.u32(p) + 8) {
                    let cell = m.u32(m.u32(p) + 0xc).wrapping_add((i as i32 as u32).wrapping_mul(4));
                    o.si_lattice_dispose(m, cell);
                    i = i.wrapping_add(1);
                }
                free_array = true;
            }
        }
        4 => {
            let r = m.u32(rec + 0xc);
            if r != 0 {
                let mut x = m.u32(r);
                while x != 0 {
                    let next = m.u32(x + 0x1c);
                    o.si_cluster_data_dispose(m, x + 0x18);
                    m.set_u32(x + 0x18, 0);
                    if m.u32(x) != 0 {
                        let a = m.u32(x);
                        m.free(a);
                        m.set_u32(x, 0);
                    }
                    if m.u32(x + 0xc) != 0 {
                        let a = m.u32(x + 0xc);
                        m.free(a);
                        m.set_u32(x + 0xc, 0);
                    }
                    m.free(x);
                    x = next;
                }
                let r = m.u32(m.u32(p) + 0xc);
                if r != 0 {
                    o.si_subel_user_data_dispose(m, r + 4, r + 8);
                    let r = m.u32(m.u32(p) + 0xc);
                    o.si_subel_user_data_dispose(m, r + 0xc, r + 0x10);
                    let r = m.u32(m.u32(p) + 0xc);
                    o.si_subel_user_data_dispose(m, r + 0x14, r + 0x18);
                    free_array = true;
                }
            }
        }
        _ => {}
    }
    if free_array {
        let r = m.u32(p);
        if m.u32(r + 0xc) != 0 {
            let a = m.u32(r + 0xc);
            m.free(a);
            m.set_u32(m.u32(p) + 0xc, 0);
        }
    }
    if m.u32(p) != 0 {
        let a = m.u32(p);
        m.free(a);
        m.set_u32(p, 0);
    }
    m.set_u32(p, 0);
    1
}

/// [soft 0x080c8348 historyNodeDispose]: frees the data of the node `**pnode` by its kind;
/// with `all` not 0 it goes on along the cluster chain (+0x10). Only the first node's own
/// record is freed, through the cell (the original leaks the rest); the cell ends at 0.
pub fn history_node_dispose(m: &mut Mem, o: &mut dyn Outside, pnode: u32, all: i16) {
    let mut cur = m.u32(pnode);
    while cur != 0 {
        let next = if all != 0 { m.u32(cur + node::CHAIN_NEXT) } else { 0 };
        let data = m.u32(cur + node::DATA);
        let free_data = |m: &mut Mem| {
            if data != 0 {
                m.free(data);
            }
        };
        match m.u32(cur + node::KIND) {
            0 | 2 | 3 | 6 | 7 => free_data(m),
            1 => {
                let l = m.local(4);
                m.set_u32(l, data);
                geo_subs_dispose(m, o, l);
                m.release(4);
            }
            4 => {
                let l = m.local(4);
                let gn = m.u32(cur + node::GN);
                m.set_u32(l, gn);
                o.si_grafnode_dispose(m, l);
                m.release(4);
            }
            8 => {
                let l = m.local(4);
                m.set_u32(l, data);
                o.si_fcurve_remove(m, l);
                m.release(4);
            }
            10 => {
                if m.i32(data + 0x24) == 5 && m.u32(data + 0x28) != 0 {
                    let s = m.u32(data + 0x28);
                    m.free(s);
                    m.set_u32(data + 0x28, 0);
                }
                free_data(m);
            }
            0xb => {
                let first = m.u32(data);
                o.si_audio_marker_mng_markers_dispose(m, first);
                free_data(m);
            }
            0xc | 0xd => {
                o.si_actions_dispose(m, data);
                free_data(m);
            }
            _ => {}
        }
        let first = m.u32(pnode);
        if first != 0 {
            m.free(first);
            m.set_u32(pnode, 0);
        }
        cur = next;
    }
    m.set_u32(pnode, 0);
}

/// [soft 0x080c8498 SI_historyDispose]: disposes of every node and empties the list.
pub fn history_dispose(m: &mut Mem, o: &mut dyn Outside) {
    let l = m.local(4);
    let mut n = m.u32(hst(m) + hst::HEAD);
    m.set_u32(l, n);
    while n != 0 {
        let next = m.u32(m.u32(l) + node::NEXT);
        history_node_dispose(m, o, l, 1);
        m.set_u32(l, next);
        n = next;
    }
    m.release(4);
    let h = hst(m);
    m.set_u32(h + hst::TAIL, 0);
    m.set_u32(h + hst::HEAD, 0);
    m.set_u32(h + hst::REDO, 0);
    m.set_u32(h + hst::UNDO, 0);
    m.set_u32(h + hst::COUNT, 0);
}

/// Takes the oldest node off the list and disposes of it (the part of `historyAddNode` and
/// `SI_historySetup` that keeps the list within its depth); `clear_tail_if_empty` is whether
/// the original tests for an emptied list (`historyAddNode` does, `SI_historySetup` does not).
fn drop_oldest(m: &mut Mem, o: &mut dyn Outside, test_empty: bool) {
    let h = hst(m);
    let first = m.u32(h + hst::HEAD);
    let l = m.local(4);
    m.set_u32(l, first);
    let next = m.u32(first + node::NEXT);
    m.set_u32(h + hst::HEAD, next);
    if test_empty && next == 0 {
        m.set_u32(h + hst::TAIL, 0);
    } else {
        m.set_u32(next + node::PREV, 0);
    }
    m.set_u32(first + node::NEXT, 0);
    history_node_dispose(m, o, l, 1);
    m.release(4);
}

/// [soft 0x080c8504 historyAddNode]: appends a node (or, with a cluster open and begun,
/// chains it to the cluster); first drops everything after the undo position (the redo
/// nodes), then the oldest node if the list is over its depth; redo is cleared and the undo
/// position is the newest node.
pub fn history_add_node(m: &mut Mem, o: &mut dyn Outside, n: u32) {
    let mut h = hst(m);
    while m.u32(h + hst::UNDO) != m.u32(h + hst::TAIL) {
        let tail = m.u32(h + hst::TAIL);
        let l = m.local(4);
        m.set_u32(l, tail);
        let prev = m.u32(m.u32(hst(m) + hst::TAIL) + node::PREV);
        m.set_u32(hst(m) + hst::TAIL, prev);
        if prev == 0 {
            m.set_u32(h + hst::HEAD, 0);
        } else {
            m.set_u32(prev + node::NEXT, 0);
        }
        m.set_u32(tail + node::PREV, 0);
        history_node_dispose(m, o, l, 1);
        m.release(4);
        h = hst(m);
        let c = m.u32(h + hst::COUNT);
        m.set_u32(h + hst::COUNT, c.wrapping_sub(1));
    }
    let cur = hst(m);
    if m.i16(cur + hst::CLUSTER_OPEN) == 0 || m.u32(cur + hst::CLUSTER_HEAD) == 0 {
        m.set_u32(n + node::PREV, m.u32(cur + hst::TAIL));
        m.set_u32(n + node::NEXT, 0);
        let tail = m.u32(h + hst::TAIL);
        if tail == 0 {
            m.set_u32(h + hst::HEAD, n);
        } else {
            m.set_u32(tail + node::NEXT, n);
        }
        let cur = hst(m);
        m.set_u32(cur + hst::TAIL, n);
        let c = m.u32(cur + hst::COUNT);
        m.set_u32(cur + hst::COUNT, c.wrapping_add(1));
        if m.i16(cur + hst::CLUSTER_OPEN) != 0 {
            m.set_u32(cur + hst::CLUSTER_HEAD, n);
        }
    } else {
        let head = m.u32(cur + hst::CLUSTER_HEAD);
        m.set_u32(head + node::CHAIN_NEXT, n);
        m.set_u32(n + node::CHAIN_BACK, m.u32(h + hst::CLUSTER_HEAD));
        m.set_u32(h + hst::CLUSTER_HEAD, n);
    }
    let h = hst(m);
    if (m.i32(h + hst::MAX)) < m.i32(h + hst::COUNT) {
        drop_oldest(m, o, true);
        let h = hst(m);
        let c = m.u32(h + hst::COUNT);
        m.set_u32(h + hst::COUNT, c.wrapping_sub(1));
    }
    let h = hst(m);
    m.set_u32(h + hst::REDO, 0);
    m.set_u32(h + hst::UNDO, m.u32(h + hst::TAIL));
}

/// [soft 0x080c9e8c SI_historySetup]: the dialog that turns the history on or off and sets
/// its depth. On OK the depth is kept at 1 or more and the list is cut to it (or disposed
/// of when the history is off); on cancel the old state and depth are put back.
pub fn history_setup(m: &mut Mem, o: &mut dyn Outside) {
    let d = o.si_dialog_open(m, 0x7d2);
    if d == 0 {
        return;
    }
    let h = hst(m);
    let depth = m.u32(h + hst::MAX);
    let active = m.i16(h + hst::ACTIVE);
    o.si_dialog_draw(m, d);
    let ev = loop {
        let ev = o.si_dialog_get_event(m, d);
        if ev == 1 || ev == 2 {
            break ev;
        }
        if ev == 3 {
            let h = hst(m);
            let b = m.u8(h + hst::ACTIVE) ^ 1;
            m.set_u8(h + hst::ACTIVE, b);
            let s = m.i16(h + hst::ACTIVE) as i32;
            o.si_set_state_item(m, d, 3, s);
        }
    };
    o.si_dialog_close(m, d);
    if ev == 1 {
        let v = o.si_get_texed_long(m, d, 5);
        let h = hst(m);
        m.set_i32(h + hst::MAX, v);
        if v < 1 {
            o.si_set_texed(m, d, 5, 0x0867921f);
            let h = hst(m);
            m.set_u32(h + hst::MAX, 1);
        }
        let h = hst(m);
        if m.i16(h + hst::ACTIVE) == 0 {
            history_dispose(m, o);
        } else if m.i32(h + hst::MAX) < m.i32(h + hst::COUNT) {
            loop {
                drop_oldest(m, o, false);
                let h = hst(m);
                let c = m.i32(h + hst::COUNT);
                m.set_i32(h + hst::COUNT, c.wrapping_sub(1));
                if !(c.wrapping_sub(1) > m.i32(h + hst::MAX)) {
                    break;
                }
            }
        }
    } else {
        let h = hst(m);
        m.set_i16(h + hst::ACTIVE, active);
        o.si_set_state_item(m, d, 3, active as i32);
        let h = hst(m);
        m.set_u32(h + hst::MAX, depth);
        m.put_cstr(g::SI_TEMP.0, &si_mem::clib::decimal(depth as i32));
        o.si_set_texed(m, d, 5, g::SI_TEMP.0);
    }
}

// ---------------------------------------------------------------------------- snapshots

/// [soft 0x080c8b5c modelHistClusterSnapshot]: a record of a model's clusters (+0 the model
/// kind, +4 = 4, +0xc a 7-word block: the duplicated list of clusters and three pairs of
/// user-data copies); null on failure.
pub fn model_hist_cluster_snapshot(m: &mut Mem, o: &mut dyn Outside, model: u32) -> u32 {
    let r = m.si_calloc(1, 0x14);
    if r == 0 {
        return 0;
    }
    let kind = m.u32(model + 0x30);
    m.set_u32(r, kind);
    m.set_u32(r + 4, 4);
    let block = m.si_calloc(7, 4);
    m.set_u32(r + 0xc, block);
    if block == 0 {
        m.free(r);
        return 0;
    }
    if m.u32(model + 0x348) == 0 {
        if m.u32(model + 0x408) == 0 && m.u32(model + 0x410) == 0 && m.u32(model + 0x418) == 0 {
            m.set_u32(r + 0xc, 0);
            return r;
        }
    } else {
        let d = o.si_modelkluster_list_duplicate(m, model, 1, 0, 1);
        m.set_u32(m.u32(r + 0xc), d);
        if d == 0 {
            if m.u32(r + 0xc) != 0 {
                let b = m.u32(r + 0xc);
                m.free(b);
                m.set_u32(r + 0xc, 0);
            }
            m.free(r);
            return 0;
        }
    }
    for (a, b, out) in [(0x408u32, 0x40cu32, 4u32), (0x410, 0x414, 0xc), (0x418, 0x41c, 0x14)] {
        let (k, n) = (m.u32(model + a), m.u32(model + b));
        if k != 0 && n != 0 {
            let blk = m.u32(r + 0xc);
            o.si_subel_user_data_copy_from_setup(m, k, n, blk + out, blk + out + 4);
        }
    }
    r
}

/// [soft 0x080c9930 modelHistLatSnapShot]: a record of a model's lattices (+4 = 2 for the
/// node's, 3 for the tree's; +8 their count, +0xc the array of copies); null when it has none.
pub fn model_hist_lat_snapshot(m: &mut Mem, o: &mut dyn Outside, model: u32, tree: i16) -> u32 {
    let r = m.si_calloc(1, 0x14);
    if r == 0 {
        return 0;
    }
    let src;
    if tree == 0 {
        if m.i16(model + 0x4c) == 0 {
            return 0;
        }
        m.set_u32(r + 4, 2);
        let c = m.i16(model + 0x4c);
        m.set_i16(r + 8, c);
        src = m.u32(model + 0x50);
    } else {
        if m.i16(model + 0x54) == 0 {
            return 0;
        }
        m.set_u32(r + 4, 3);
        let c = m.i16(model + 0x54);
        m.set_i16(r + 8, c);
        src = m.u32(model + 0x58);
    }
    let arr = m.si_calloc(m.i16(r + 8) as i32 as u32, 4);
    m.set_u32(r + 0xc, arr);
    let mut i: i32 = 0;
    while i < m.i16(r + 8) as i32 {
        let off = (i as u32).wrapping_mul(4);
        let lat = m.u32(src.wrapping_add(off));
        let copy = o.si_lattice_copy(m, lat);
        let arr = m.u32(r + 0xc);
        m.set_u32(arr.wrapping_add(off), copy);
        i += 1;
    }
    r
}

/// [soft 0x080c9050 modelHistGeoSnapshot]: a record of a model's geometry (+0 the model
/// kind, +4 the target asked for, +8 the count, +0xc the array of copies, +0x10 the copy of
/// the extra shape); null when the allocation fails.
pub fn model_hist_geo_snapshot(m: &mut Mem, o: &mut dyn Outside, model: u32, target: u32) -> u32 {
    let r = m.si_calloc(1, 0x14);
    if r == 0 {
        return r;
    }
    let kind = m.u32(model + 0x30);
    m.set_u32(r, kind);
    m.set_u32(r + 4, target);
    m.set_u16(r + 8, m.u16(model + 0x34));
    let arr = m.si_calloc(m.u16(model + 0x34) as u32, 4);
    m.set_u32(r + 0xc, arr);
    m.set_u32(r + 0x10, 0);
    if !matches!(kind, 1 | 2 | 4 | 6 | 8 | 9 | 10) {
        return r;
    }
    let copy = |m: &mut Mem, o: &mut dyn Outside, p: u32| match kind {
        1 => o.si_patch_copy(m, p),
        2 => o.si_face_copy(m, p),
        4 => o.si_mesh_copy(m, p),
        6 => o.si_spline_copy(m, p),
        8 => o.si_metaball_copy(m, p, model),
        9 => o.si_nurbs_crv_copy(m, p),
        _ => o.si_nurbs_srf_copy(m, p),
    };
    if m.u16(model + 0x34) != 0 {
        let mut i: i16 = 0;
        loop {
            let src = m.u32(m.u32(model + 0x38).wrapping_add((i as i32 as u32).wrapping_mul(4)));
            let c = copy(m, o, src);
            i = i.wrapping_add(1);
            let arr = m.u32(r + 0xc);
            m.set_u32(arr.wrapping_add(((i as i32 - 1) as u32).wrapping_mul(4)), c);
            if !((i as i32) < m.u16(model + 0x34) as i32) {
                break;
            }
        }
    }
    let extra = m.u32(model + 0x3c);
    if extra == 0 {
        return r;
    }
    let c = copy(m, o, extra);
    m.set_u32(r + 0x10, c);
    r
}

/// [soft 0x080c9354 cameraHistorySnapshot]: a copy of a camera's ten setting words (four of
/// them moved through the x87 unit); null on failure.
pub fn camera_history_snapshot(m: &mut Mem, cam: u32) -> u32 {
    let r = m.si_calloc(1, 0x28);
    if r == 0 {
        return 0;
    }
    for (i, off) in [0x10u32, 0x14, 0x18, 0x1c, 0x20, 0x24].into_iter().enumerate() {
        let v = m.u32(cam + off);
        m.set_u32(r + 4 * i as u32, v);
    }
    for (i, off) in [0x30u32, 0x34, 0x38, 0x40].into_iter().enumerate() {
        fcopy(m, r + 0x18 + 4 * i as u32, cam + off);
    }
    r
}

/// [soft 0x080c93b4 cameraSwapHistory]: exchanges a camera's settings with the record.
pub fn camera_swap_history(m: &mut Mem, cam: u32, rec: u32) {
    for k in 0..3 {
        swap(m, cam + 0x10 + 4 * k, rec + 4 * k);
    }
    for k in 0..3 {
        swap(m, cam + 0x1c + 4 * k, rec + 0xc + 4 * k);
    }
    for (c, r) in [(0x30u32, 0x18u32), (0x34, 0x1c), (0x38, 0x20), (0x40, 0x24)] {
        fswap(m, cam + c, rec + r);
    }
}

/// [soft 0x080c99e4 lightHistorySnapshot]: a copy of a light's settings (0x8c bytes, with its
/// matrix); null on failure.
pub fn light_history_snapshot(m: &mut Mem, o: &mut dyn Outside, light: u32) -> u32 {
    let r = m.si_calloc(1, 0x8c);
    if r == 0 {
        return 0;
    }
    let mv = |m: &mut Mem, i: u32, off: u32| {
        let v = m.u32(light + off);
        m.set_u32(r + 4 * i, v);
    };
    mv(m, 0, 0x58);
    mv(m, 1, 0x5c);
    mv(m, 2, 0x60);
    fcopy(m, r + 0xc, light + 0x70);
    fcopy(m, r + 0x10, light + 0x74);
    for (i, off) in (5u32..).zip([0x24u32, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c]) {
        mv(m, i, off);
    }
    fcopy(m, r + 0x30, light + 0x90);
    for (i, off) in (0xdu32..).zip([0x1c4u32, 0x1c8, 0x1cc, 0x1d0, 0x1d4, 0x1d8]) {
        mv(m, i, off);
    }
    o.si_matrix_copy(m, light + 0x1e0, r + 0x4c);
    r
}

/// [soft 0x080c9ab0 lightSwapHistory]: exchanges a light's settings with the record.
pub fn light_swap_history(m: &mut Mem, o: &mut dyn Outside, light: u32, rec: u32) {
    for k in 0..3 {
        swap(m, light + 0x58 + 4 * k, rec + 4 * k);
    }
    fswap(m, light + 0x70, rec + 0xc);
    fswap(m, light + 0x74, rec + 0x10);
    for k in 0..4 {
        swap(m, light + 0x24 + 4 * k, rec + 0x14 + 4 * k);
    }
    for k in 0..3 {
        swap(m, light + 0x34 + 4 * k, rec + 0x24 + 4 * k);
    }
    fswap(m, light + 0x90, rec + 0x30);
    for k in 0..3 {
        swap(m, light + 0x1c4 + 4 * k, rec + 0x34 + 4 * k);
    }
    for k in 0..3 {
        swap(m, light + 0x1d0 + 4 * k, rec + 0x40 + 4 * k);
    }
    let tmp = m.local(0x40);
    o.si_matrix_copy(m, light + 0x1e0, tmp);
    o.si_matrix_copy(m, rec + 0x4c, light + 0x1e0);
    o.si_matrix_copy(m, tmp, rec + 0x4c);
    m.release(0x40);
}

/// [soft 0x080c944c audioHistorySnapShot]: a record of an audio track's markers (0x10 bytes:
/// the two marker lists, a word, the track id).
pub fn audio_history_snapshot(m: &mut Mem, track_id: i32, a: u32, b: u32, c: i32) -> u32 {
    let r = m.si_calloc(1, 0x10);
    if r != 0 {
        m.set_u32(r, a);
        m.set_u32(r + 4, b);
        m.set_i32(r + 8, c);
        m.set_i32(r + 0xc, track_id);
    }
    r
}

/// [soft 0x080c9514 actionsHistorySnapShot]: a copy of a hierarchy's action list (`motion` 0:
/// the one at +0xc, else the one at +0x14) with two numbers; null when either is negative.
pub fn actions_history_snapshot(m: &mut Mem, o: &mut dyn Outside, hier: u32, a: i32, b: i32, motion: i16) -> u32 {
    if a < 0 || b < 0 {
        return 0;
    }
    let r = m.si_calloc(1, 0x10);
    if r != 0 {
        let src = if motion == 0 { hier + 0xc } else { hier + 0x14 };
        o.si_actions_copy_all(m, r, src);
        m.set_i32(r + 0xc, a);
        m.set_i32(r + 8, b);
    }
    r
}

// ---------------------------------------------------------------------------- savers

fn on(m: &Mem) -> bool {
    m.i16(hst(m) + hst::ACTIVE) != 0
}

/// [soft 0x080c973c SI_grafnodeSaveHistoryDelete]: records the deletion of a graph node (the
/// node is kept for undo); with the history off the node is disposed of at once.
pub fn grafnode_save_history_delete(m: &mut Mem, o: &mut dyn Outside, gn: u32) -> i32 {
    if !on(m) {
        let l = m.local(4);
        m.set_u32(l, gn);
        o.si_grafnode_dispose(m, l);
        m.release(4);
        return 0;
    }
    let n = new_node(m);
    if n == 0 {
        return 0;
    }
    m.set_u32(n + node::GN, gn);
    m.set_u32(n + node::KIND, 4);
    history_add_node(m, o, n);
    1
}

/// [soft 0x080c97bc grafnodeSaveHistoryAdd]: records the adding of a graph node.
pub fn grafnode_save_history_add(m: &mut Mem, o: &mut dyn Outside, gn: u32) -> i32 {
    if on(m) {
        let n = new_node(m);
        if n != 0 {
            m.set_u32(n + node::GN, gn);
            m.set_u32(n + node::KIND, 5);
            history_add_node(m, o, n);
            return 1;
        }
    }
    0
}

/// [soft 0x080c96a4 SI_cameraSaveHistory]: records a camera's settings (graph node kinds 4
/// and 5).
pub fn camera_save_history(m: &mut Mem, o: &mut dyn Outside, gn: u32) -> i32 {
    if on(m) && m.u32(gn).wrapping_sub(4) < 2 {
        let cam = m.u32(gn + 4);
        let n = new_node(m);
        if n != 0 {
            m.set_u32(n + node::GN, gn);
            m.set_u32(n + node::KIND, 2);
            let d = camera_history_snapshot(m, cam);
            m.set_u32(n + node::DATA, d);
            if d != 0 {
                history_add_node(m, o, n);
                return 1;
            }
            m.free(n);
        }
    }
    0
}

/// [soft 0x080c9c20 SI_lightSaveHistory]: records a light's settings (graph node kinds 6 to
/// 8, 0x13 and 0x14).
pub fn light_save_history(m: &mut Mem, o: &mut dyn Outside, gn: u32) -> i32 {
    if on(m) {
        let k = m.u32(gn);
        if k.wrapping_sub(6) < 3 || k == 0x13 || k == 0x14 {
            let light = m.u32(gn + 4);
            let n = new_node(m);
            if n != 0 {
                m.set_u32(n + node::GN, gn);
                m.set_u32(n + node::KIND, 3);
                let d = light_history_snapshot(m, o, light);
                m.set_u32(n + node::DATA, d);
                if d != 0 {
                    history_add_node(m, o, n);
                    return 1;
                }
                m.free(n);
            }
        }
    }
    0
}

/// [soft 0x080c9484 SI_audioSaveHistory]: records an audio track's markers.
pub fn audio_save_history(m: &mut Mem, o: &mut dyn Outside, track: u32, a: u32, b: u32, c: i32) -> i32 {
    if on(m) {
        let n = new_node(m);
        if n != 0 {
            m.set_u32(n + node::KIND, 0xb);
            let id = m.i32(track + 0xc);
            let d = audio_history_snapshot(m, id, a, b, c);
            m.set_u32(n + node::DATA, d);
            if d != 0 {
                history_add_node(m, o, n);
                return 1;
            }
            m.free(n);
        }
    }
    0
}

fn actions_save(m: &mut Mem, o: &mut dyn Outside, hier: u32, a: i32, b: i32, kind: u32, motion: i16) -> i32 {
    if on(m) {
        let n = new_node(m);
        if n != 0 {
            m.set_u32(n + node::KIND, kind);
            let d = actions_history_snapshot(m, o, hier, a, b, motion);
            m.set_u32(n + node::DATA, d);
            if d != 0 {
                history_add_node(m, o, n);
                return 1;
            }
            m.free(n);
        }
    }
    0
}

/// [soft 0x080c9588 SI_actionsSaveHistory]: records a hierarchy's action list.
pub fn actions_save_history(m: &mut Mem, o: &mut dyn Outside, hier: u32, a: i32, b: i32) -> i32 {
    actions_save(m, o, hier, a, b, 0xc, 0)
}

/// [soft 0x080c9614 SI_mstractSaveHistory]: records a hierarchy's motion-stractions list.
pub fn mstract_save_history(m: &mut Mem, o: &mut dyn Outside, hier: u32, a: i32, b: i32) -> i32 {
    actions_save(m, o, hier, a, b, 0xd, 1)
}

/// [soft 0x080c87b0 SI_modelSaveHistoryTransform]: records a model's transform (graph node
/// kinds 2 and 3; `tree` and `mode` as the snapshot reads them, `target` 0 to 2).
pub fn model_save_history_transform(m: &mut Mem, o: &mut dyn Outside, gn: u32, tree: i16, mode: i16, target: u32) -> i32 {
    if on(m) && m.u32(gn).wrapping_sub(2) < 2 {
        let model = m.u32(gn + 4);
        let n = new_node(m);
        if n != 0 {
            m.set_u32(n + node::GN, gn);
            m.set_u32(n + node::KIND, 0);
            let d = m.si_calloc(1, 0x38);
            if d != 0 {
                m.set_u32(n + node::DATA, d);
                // a 0x38-byte record on the stack: only its first eight bytes are set here
                let l = m.local(0x38);
                m.set_i16(l, tree);
                m.set_i16(l + 2, mode);
                m.set_u32(l + 4, target);
                model_hist_xform_snapshot(m, o, model, l);
                let words = m.bytes(l, 0x38).to_vec();
                m.set_bytes(d, &words);
                m.release(0x38);
                history_add_node(m, o, n);
                return 1;
            }
            m.free(n);
        }
    }
    0
}

/// [soft 0x080c8634 modelHistXformSnapshot]: fills a transform record (+0 tree flag, +4
/// target: 0 the model's own scale, rotation and translation, 1 a spline fitting, 2 a patch
/// fitting) from the model.
pub fn model_hist_xform_snapshot(m: &mut Mem, o: &mut dyn Outside, model: u32, rec: u32) {
    let w = |m: &mut Mem, dst: u32, src: u32| {
        let v = m.u32(src);
        m.set_u32(rec + dst, v);
    };
    let last;
    match m.i32(rec + 4) {
        0 => {
            for (d, s) in [(8u32, 0x100u32), (0xc, 0x104), (0x10, 0x108), (0x14, 0x10c), (0x18, 0x110), (0x1c, 0x114)] {
                w(m, d, model + s);
            }
            if m.i32(model + 0x30) == 5 {
                let ik = m.u32(model + 0x318);
                if ik != 0 {
                    w(m, 0x2c, ik + 0x20);
                    w(m, 0x30, ik + 0x24);
                    w(m, 0x34, ik + 0x28);
                }
            }
            if m.i32(model + 0x2ec) == 3 {
                w(m, 0x20, model + 0x1b8);
                w(m, 0x24, model + 0x1bc);
                last = model + 0x1c0;
            } else {
                w(m, 0x20, model + 0x158);
                w(m, 0x24, model + 0x15c);
                last = model + 0x160;
            }
        }
        1 => {
            let fit = if m.i16(rec) == 0 { o.si_model_node_spline_fitting(m, model) } else { o.si_model_tree_spline_fitting(m, model) };
            if fit == 0 {
                return;
            }
            w(m, 8, fit + 0xc);
            w(m, 0xc, fit + 0x10);
            w(m, 0x10, fit + 0x14);
            fcopy(m, rec + 0x1c, fit + 0x18);
            w(m, 0x20, fit + 0x1c);
            w(m, 0x24, fit + 0x20);
            last = fit + 0x24;
        }
        2 => {
            let fit = if m.i16(rec) == 0 { o.si_model_node_patch_fitting(m, model) } else { o.si_model_branch_patch_fitting(m, model) };
            if fit == 0 {
                return;
            }
            for k in 0..8 {
                w(m, 8 + 4 * k, fit + 8 + 4 * k);
            }
            last = fit + 0x28;
        }
        _ => return,
    }
    w(m, 0x28, last);
}

/// [soft 0x080c8898 SI_modelSaveHistoryGeometry]: records a model's geometry (graph node
/// kinds 2 and 3). `target`: 0 shapes, 1 clusters and shapes (two nodes in a cluster), 2 the
/// node's lattices, 3 the tree's, 4 clusters only.
pub fn model_save_history_geometry(m: &mut Mem, o: &mut dyn Outside, gn: u32, target: i32) -> i32 {
    if !on(m) || m.u32(gn).wrapping_sub(2) > 1 {
        return 0;
    }
    let model = m.u32(gn + 4);
    let k31c = m.u32(model + 0x31c);
    if (k31c.wrapping_sub(5) < 2 || k31c == 8 || m.u32(model + 0x30) == 8) && target == 1 {
        return 0;
    }
    let n = new_node(m);
    if n == 0 {
        return 0;
    }
    m.set_u32(n + node::KIND, 1);
    m.set_u32(n + node::GN, gn);
    match target {
        0 => {
            let k = m.u32(model + 0x30);
            if k == 0 || k == 5 {
                m.free(n);
                return 0;
            }
            let d = model_hist_geo_snapshot(m, o, model, target as u32);
            m.set_u32(n + node::DATA, d);
            if d == 0 {
                m.free(n);
                return 0;
            }
        }
        1 => {
            let k = m.u32(model + 0x30);
            if k == 0 || k == 5 {
                m.free(n);
                return 0;
            }
            let d = model_hist_cluster_snapshot(m, o, model);
            m.set_u32(n + node::DATA, d);
            if d == 0 {
                m.free(n);
                return 0;
            }
            let n2 = new_node(m);
            if n2 == 0 {
                m.free(n);
                return 0;
            }
            m.set_u32(n2 + node::KIND, 1);
            m.set_u32(n2 + node::GN, gn);
            let d2 = model_hist_geo_snapshot(m, o, model, 0);
            m.set_u32(n2 + node::DATA, d2);
            if d2 == 0 {
                m.free(n);
                m.free(n2);
                return 0;
            }
            let open = m.i16(hst(m) + hst::CLUSTER_OPEN);
            history_open_cluster(m);
            history_add_node(m, o, n);
            history_add_node(m, o, n2);
            let h = hst(m);
            m.set_i16(h + hst::CLUSTER_OPEN, open);
            return 1;
        }
        2 | 3 => {
            let has = if target == 2 { o.si_model_node_lattice(m, model) } else { o.si_model_tree_lattice(m, model) };
            if has == 0 {
                m.free(n);
                return 0;
            }
            let d = model_hist_lat_snapshot(m, o, model, (target == 3) as i16);
            m.set_u32(n + node::DATA, d);
            if d == 0 {
                m.free(n);
                return 0;
            }
        }
        4 => {
            let k = m.u32(model + 0x30);
            if k == 0 || k == 5 {
                return 1;
            }
            let d = model_hist_cluster_snapshot(m, o, model);
            m.set_u32(n + node::DATA, d);
            if d == 0 {
                m.free(n);
                return 0;
            }
        }
        _ => return 1,
    }
    history_add_node(m, o, n);
    1
}

/// [soft 0x080ca6f4 SI_fcurveSaveHistory]: records a function curve (its element `elem`,
/// whose +4 is the curve) by copying it, and the curve behind it, into new curves.
pub fn fcurve_save_history(m: &mut Mem, o: &mut dyn Outside, elem: u32) -> i32 {
    if on(m) && elem != 0 {
        let fc = m.u32(elem + 4);
        if fc != 0 {
            let n = new_node(m);
            if n != 0 {
                m.set_u32(n + node::KIND, 8);
                let dict = elem_dict(m);
                let found = o.si_elemdict_find_elem_by_data(m, dict, m[g::SI_CID_ANIMATION], fc);
                let cell = m.si_calloc(1, 4);
                m.set_u32(cell, found);
                m.set_u32(n + node::GN, cell);
                let (a, b) = (m.u32(fc), m.u32(fc + 4));
                let c1 = o.si_fcurve_allocate(m, a, b);
                o.si_fcurve_transfer(m, fc, c1);
                let (a, b) = (m.u32(fc), m.u32(fc + 4));
                let c2 = o.si_fcurve_allocate(m, a, b);
                m.set_u32(c1 + 0x54, c2);
                let back = m.u32(fc + 0x54);
                o.si_fcurve_transfer(m, back, c2);
                m.set_u32(n + node::DATA, c1);
                history_add_node(m, o, n);
                return 1;
            }
        }
    }
    0
}

/// [soft 0x080c9cc8 SI_sprdSaveHistory]: records a spreadsheet cell (`ident` is its 16-byte
/// identity, `idx` its 20-byte attribute index): a record of both and the cell's value, a
/// text value copied.
pub fn sprd_save_history(m: &mut Mem, o: &mut dyn Outside, ident: u32, idx: u32) -> i32 {
    if on(m) {
        let n = new_node(m);
        if n != 0 {
            m.set_u32(n + node::GN, 0);
            m.set_u32(n + node::KIND, 10);
            let rec = m.si_calloc(1, 0x30);
            if rec == 0 {
                m.free(n);
            } else {
                m.set_u32(n + node::DATA, rec);
                for k in 0..4 {
                    let v = m.u32(ident + 4 * k);
                    m.set_u32(rec + 4 * k, v);
                }
                for k in 0..5 {
                    let v = m.u32(idx + 4 * k);
                    m.set_u32(rec + 0x10 + 4 * k, v);
                }
                let val = m.local(12);
                let got = o.si_ed_element_att_value_get(m, rec, rec + 0x10, val);
                if got == 1 {
                    let (t, s, e) = (m.u32(val), m.u32(val + 4), m.u32(val + 8));
                    m.set_u32(rec + 0x24, t);
                    m.set_u32(rec + 0x28, s);
                    m.set_u32(rec + 0x2c, e);
                    if t != 5 {
                        m.release(12);
                        history_add_node(m, o, n);
                        return 1;
                    }
                    if s != 0 {
                        let copy = m.strdup(s);
                        m.set_u32(rec + 0x28, copy);
                        m.release(12);
                        history_add_node(m, o, n);
                        return 1;
                    }
                }
                m.release(12);
                m.free(rec);
                m.free(n);
            }
        }
    }
    0
}

/// [soft 0x080c9e04 sprdSwapHistory]: exchanges a recorded spreadsheet cell's value with the
/// cell's current one.
pub fn sprd_swap_history(m: &mut Mem, o: &mut dyn Outside, rec: u32) {
    let val = m.local(12);
    let got = o.si_ed_element_att_value_get(m, rec, rec + 0x10, val);
    if got != 0 {
        if m.i32(val) == 5 {
            let s = m.u32(val + 4);
            let copy = m.strdup(s);
            m.set_u32(val + 4, copy);
        }
        o.si_ed_element_att_value_set(m, rec, rec + 0x10, rec + 0x24);
        if m.i32(rec + 0x24) == 5 && m.u32(rec + 0x28) != 0 {
            let s = m.u32(rec + 0x28);
            m.free(s);
            m.set_u32(rec + 0x28, 0);
        }
        let (t, s, e) = (m.u32(val), m.u32(val + 4), m.u32(val + 8));
        m.set_u32(rec + 0x24, t);
        m.set_u32(rec + 0x28, s);
        m.set_u32(rec + 0x2c, e);
    }
    m.release(12);
}

/// [soft 0x080ca4d4 SI_modelSaveHistoryParenting]: records the linking (`unlink` 0 means the
/// new parent is `parent`) or the cutting of a model's parent, with a cluster round it; on
/// linking it also saves both models' action lists.
pub fn model_save_history_parenting(m: &mut Mem, o: &mut dyn Outside, child: u32, parent: u32, unlink: i16) -> i32 {
    if on(m) && child != 0 && parent != 0 && m.u32(child).wrapping_sub(2) < 2 && m.u32(parent).wrapping_sub(2) < 2 {
        history_open_cluster(m);
        if unlink == 0 {
            let l = m.local(0x14);
            let (c, d) = (l + 4, l);
            let (e, f) = (l + 0xc, l + 8);
            let p1 = o.si_elemdict_find_elem_from_grafnode(m, parent, d, c);
            let p2 = o.si_elemdict_find_elem_from_grafnode(m, child, f, e);
            let dict = elem_dict(m);
            let cid = m[g::SI_CID_ANIMATION];
            let r1 = o.si_elemdict_find_range_related_elem(m, dict, p1, 1, 0, 0x3a98, 0x3aa2, cid, l + 0x10, l + 0x10);
            let dict = elem_dict(m);
            let r2 = o.si_elemdict_find_range_related_elem(m, dict, p2, 1, 0, 0x3a98, 0x3aa2, cid, l + 0x10, l + 0x10);
            if r1 != 0 && m.u32(r1 + 4) != 0 && r2 != 0 && m.u32(r2 + 4) != 0 {
                let (pa, pb) = (m.i32(l + 8), m.i32(l + 0xc));
                let (qa, qb) = (m.i32(l), m.i32(l + 4));
                let h2 = m.u32(r2 + 4);
                let h1 = m.u32(r1 + 4);
                actions_save_history(m, o, h2, pa, pb);
                actions_save_history(m, o, h1, qa, qb);
                mstract_save_history(m, o, h2, pa, pb);
                mstract_save_history(m, o, h1, qa, qb);
            }
            m.release(0x14);
        }
        let n = new_node(m);
        if n != 0 {
            m.set_u32(n + node::GN, child);
            m.set_u32(n + node::KIND, if unlink != 0 { 7 } else { 6 });
            let link = m.si_calloc(1, 8);
            if link != 0 {
                m.set_u32(n + node::DATA, link);
                m.set_u32(link + 4, parent);
                m.set_u32(link, 0);
                history_add_node(m, o, n);
                history_close_cluster(m);
                return 1;
            }
            m.free(n);
        }
        history_close_cluster(m);
    }
    0
}

// ---------------------------------------------------------------------------- links and transforms

/// A float stored from the unit: the register value rounded to a `float` (`fstp m32`).
fn fstore(m: &mut Mem, addr: u32, v: si_x87::F80) {
    m.set_u32(addr, v.to_f32().to_bits());
}

fn f32_at(m: &Mem, addr: u32) -> f32 {
    f32::from_bits(m.u32(addr))
}

/// The point in `model + 0x158` (x, y, z) taken through the 4x3 matrix at `mat` (rows of
/// four floats: x, y, z coefficients at +0, +4, +8; the translation at +0x30), each result
/// `((z*c + x*a) ...)`: summed in the order the original does, in extended precision, and
/// stored as floats.
fn point_through(m: &mut Mem, model: u32, mat: u32) {
    use si_x87::x;
    let (f1, f2, f3) = (x(f32_at(m, model + 0x158)), x(f32_at(m, model + 0x15c)), x(f32_at(m, model + 0x160)));
    let c = |m: &Mem, off: u32| f32_at(m, mat + off);
    let r1 = ((f2 * c(m, 0x10) + f1 * c(m, 0)) + f3 * c(m, 0x20)) + c(m, 0x30);
    fstore(m, model + 0x158, r1);
    let r2 = ((f2 * c(m, 0x14) + f1 * c(m, 4)) + f3 * c(m, 0x24)) + c(m, 0x34);
    fstore(m, model + 0x15c, r2);
    let r3 = ((f2 * c(m, 0x18) + f1 * c(m, 8)) + f3 * c(m, 0x28)) + c(m, 0x38);
    fstore(m, model + 0x160, r3);
}

fn graf_list(m: &Mem) -> u32 {
    let pd = m[ig::SI_PROC_DATA];
    m[ig::SI_GRAF_LISTS].wrapping_add(m.u32(pd + 4).wrapping_mul(0x24))
}

/// [soft 0x080c98d8 historyFixGrafnode]: in every node of the list, and every node of its
/// cluster chain, puts `new` where the graph node `old` was (also the second node of a
/// link record).
pub fn history_fix_grafnode(m: &mut Mem, old: u32, new: u32) {
    let mut outer = m.u32(hst(m) + hst::HEAD);
    while outer != 0 {
        let mut n = outer;
        while n != 0 {
            if m.u32(n + node::GN) == old {
                m.set_u32(n + node::GN, new);
            }
            if m.u32(n + node::KIND).wrapping_sub(6) < 2 {
                let d = m.u32(n + node::DATA);
                if m.u32(d + 4) == old {
                    m.set_u32(d + 4, new);
                }
            }
            n = m.u32(n + node::CHAIN_NEXT);
        }
        outer = m.u32(outer + node::NEXT);
    }
}

/// [soft 0x080c8074 verifyHierNSEstate]: an element of the models class that cannot be edited
/// is made so, and so are all its sons.
pub fn verify_hier_nse_state(m: &mut Mem, o: &mut dyn Outside, elem: u32) {
    let cid = m[g::SI_CID_MODELS];
    if o.si_ed_check_for_uneditable(m, elem, cid) != 0 {
        let dict = elem_dict(m);
        o.si_ed_set_uneditable(m, dict, elem);
    }
    let mut son = o.si_ed_find_son(m, elem);
    while son != 0 {
        verify_hier_nse_state(m, o, son);
        son = o.si_ed_find_next_son(m, elem);
    }
}

/// [soft 0x080ca028 historyDeleteLink]: undoes the linking of the graph node `gn` to the model
/// its link record names: the node's model is cut from its father, keeping its place in the
/// world when `SI_hrchCpst` is set; 0 when the record's graph node is not found.
pub fn history_delete_link(m: &mut Mem, o: &mut dyn Outside, gn: u32, link: u32) -> i32 {
    use si_x87::x;
    let other = m.u32(link + 4);
    let (b, a) = (m.u32(other + 4), m.u32(other));
    let list = graf_list(m);
    let found = o.si_find_gl_data(m, list, b, a);
    if found == 0 {
        return 0;
    }
    if m.u32(link) != 0 {
        return 1;
    }
    if m.u32(found).wrapping_sub(2) >= 2 {
        return 0;
    }
    let p4 = m.u32(found + 4);
    let p5 = m.u32(gn + 4);
    if m[g::SI_HRCH_CPST] != 0 {
        for (off, from) in [(0x100u32, 0x16cu32), (0x104, 0x170), (0x108, 0x174)] {
            let v = x(f32_at(m, p5 + off)) * f32_at(m, p4 + from);
            fstore(m, p5 + off, v);
        }
        o.si_matrix_multiply_sub3(m, p5 + 0x118, p4 + 0x178, p5 + 0x118);
        o.si_matrix_extract_xyz_rotation(m, p5 + 0x118, p5 + 0x10c);
        point_through(m, p5, p4 + 0x1c4);
    }
    o.si_dyna_phy_props_set_new_root(m, p5);
    let fath = m.u32(p5 + 0x2bc);
    o.si_model_cut_father(m, p5);
    if m.u32(fath + 0x30) == 5 {
        let first = m.u32(fath + 0x2c4);
        if first == 0 || m.u32(first + 0x2c0) == 0 {
            m.set_u32(fath + 0x31c, 0);
        }
    }
    if m.u32(p5 + 0x44) != p5 {
        o.si_model_update_dependency(m, p5, 0);
        o.si_model_readjust_definition(m, p5);
        let mut son = 0;
        loop {
            son = o.si_model_next_son(m, p5, son);
            if son == 0 {
                break;
            }
            o.si_model_readjust_definition(m, son);
        }
    }
    o.si_model_update_son_state(m, 0, p5, 1);
    let list = graf_list(m);
    o.si_graflist_cut_model(m, list, gn, p4, 1);
    if m.u32(gn + 8) == 2 {
        let mut cur = gn;
        let mut i: i16 = 0;
        while (i as i32) <= m.u16(gn + 0x10) as i32 {
            m.set_u32(cur + 8, 0);
            cur = m.u32(cur + 0x1c);
            i = i.wrapping_add(1);
        }
    }
    1
}

/// [soft 0x080ca27c historyCreateLink]: redoes the linking of the graph node `gn` to the model
/// its link record names (the model is entered as the last son, its place in the father's
/// world kept when `SI_hrchCpst` is set); 0 when it cannot be done.
pub fn history_create_link(m: &mut Mem, o: &mut dyn Outside, gn: u32, link: u32) -> i32 {
    use si_x87::x;
    let other = m.u32(link + 4);
    let (b, a) = (m.u32(other + 4), m.u32(other));
    let list = graf_list(m);
    let found = o.si_find_gl_data(m, list, b, a);
    if found == 0 {
        return 0;
    }
    if m.u32(link) != 0 {
        return 1;
    }
    if m.u32(found).wrapping_sub(2) >= 2 {
        return 0;
    }
    let p5 = m.u32(found + 4);
    let p6 = m.u32(gn + 4);
    if o.si_dyna_phy_props_validate_parent(m, p5, p6) != 1 {
        return 0;
    }
    if m[g::SI_HRCH_CPST] != 0 {
        for (off, from) in [(0x100u32, 0x16cu32), (0x104, 0x170), (0x108, 0x174)] {
            let v = x(f32_at(m, p6 + off)) / f32_at(m, p5 + from);
            fstore(m, p6 + off, v);
        }
        let inv = m.local(0x40);
        o.si_matrix_inverse(m, p5 + 0x178, inv);
        o.si_matrix_multiply_sub3(m, p6 + 0x118, inv, p6 + 0x118);
        o.si_matrix_extract_xyz_rotation(m, p6 + 0x118, p6 + 0x10c);
        o.si_matrix_inverse(m, p5 + 0x1c4, inv);
        point_through(m, p6, inv);
        m.release(0x40);
    }
    let first = m.u32(p5 + 0x2c4);
    if first == 0 {
        m.set_u32(p5 + 0x2c4, p6);
    } else {
        let mut last = first;
        while m.u32(last + 0x2c0) != 0 {
            last = m.u32(last + 0x2c0);
        }
        m.set_u32(last + 0x2c0, p6);
    }
    m.set_u32(p6 + 0x2bc, p5);
    if m.u32(p5 + 0x30) == 5 {
        m.set_u32(p5 + 0x31c, 1);
    }
    o.si_model_update_son_state(m, p5, p6, 1);
    let list = graf_list(m);
    o.si_graflist_link_model(m, list, found, gn, 0, 1);
    if m.u32(found + 8).wrapping_sub(2) >= 2 {
        return 1;
    }
    if m.u8(gn + 0x17) & 0x10 != 0 && m[ig::SI_LIGHT_LICENSE] != 0 {
        return 1;
    }
    m.set_u32(gn + 8, 2);
    1
}

/// [soft 0x080c80ec modelHistXformSet]: puts a transform record back on a model: the model's
/// own scale, rotation and position (an inverse-kinematics goal when the model is a chain's
/// root), or a spline or patch fitting's numbers; the model and the fitting are marked changed.
pub fn model_hist_xform_set(m: &mut Mem, o: &mut dyn Outside, model: u32, rec: u32) {
    let b = m.u8(model + 0xa) | 0xe;
    m.set_u8(model + 0xa, b);
    let b = m.u8(model + 0x3fa) | 0xe;
    m.set_u8(model + 0x3fa, b);
    let w = |m: &mut Mem, dst: u32, src: u32| {
        let v = m.u32(src);
        m.set_u32(dst, v);
    };
    match m.i32(rec + 4) {
        1 => {
            let tree = m.i16(rec);
            let fit = if tree == 0 { o.si_model_node_spline_fitting(m, model) } else { o.si_model_tree_spline_fitting(m, model) };
            if fit != 0 {
                w(m, fit + 0xc, rec + 8);
                w(m, fit + 0x10, rec + 0xc);
                w(m, fit + 0x14, rec + 0x10);
                fcopy(m, fit + 0x18, rec + 0x1c);
                w(m, fit + 0x1c, rec + 0x20);
                w(m, fit + 0x20, rec + 0x24);
                w(m, fit + 0x24, rec + 0x28);
                o.si_fitting_update_matrix(m, fit);
                o.si_spline_fit_touch(m, model, m.i16(rec));
            }
        }
        0 => {
            if m.i32(model + 0x30) == 5 {
                w(m, model + 0x100, rec + 8);
                w(m, model + 0x104, rec + 0xc);
                w(m, model + 0x108, rec + 0x10);
                o.si_model_set_rotation(m, model, rec + 0x14, 0, 0);
                let (a, b2, c) = (m.u32(rec + 0x2c), m.u32(rec + 0x30), m.u32(rec + 0x34));
                o.si_inv_kin_reset_pref_angle(m, model, 0, a, b2, c);
            } else {
                for (d, s) in [(0x100u32, 8u32), (0x104, 0xc), (0x108, 0x10), (0x10c, 0x14), (0x110, 0x18), (0x114, 0x1c)] {
                    w(m, model + d, rec + s);
                }
                o.si_matrix_xyz_rotation(m, model + 0x10c, model + 0x118);
                if m.i32(model + 0x2ec) == 3 {
                    let (a, b2, c) = (m.u32(rec + 0x20), m.u32(rec + 0x24), m.u32(rec + 0x28));
                    o.si_inv_kin_set_goal(m, model, a, b2, c);
                } else {
                    w(m, model + 0x158, rec + 0x20);
                    w(m, model + 0x15c, rec + 0x24);
                    w(m, model + 0x160, rec + 0x28);
                }
                let flag = (m.i16(rec) != 0) as i32;
                let s = m.i16(rec + 2) as i32;
                o.si_model_compensate_matrix(m, model, flag, s);
                let father = m.u32(model + 0x2bc);
                o.si_model_update_son_state(m, father, model, 1);
            }
        }
        2 => {
            let tree = m.i16(rec);
            let fit = if tree == 0 { o.si_model_node_patch_fitting(m, model) } else { o.si_model_branch_patch_fitting(m, model) };
            if fit != 0 {
                for k in 0..9 {
                    w(m, fit + 8 + 4 * k, rec + 8 + 4 * k);
                }
                o.si_pfitting_update_matrix(m, fit);
                o.si_patch_fit_touch(m, model, m.i16(rec));
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------- undo and redo

/// Where `SI_historyUndoRedo` keeps its locals (offsets in one frame; the original's
/// `ebp`-relative slots: the addresses are not the original's, nothing compares them).
mod fr {
    pub const SIZE: u32 = 0x100;
    pub const A4: u32 = 0x00; // two out words of SI_elemdictFindElemFromGrafNode
    pub const AC: u32 = 0x08; // SI_subelUserDataCopyFromSetup outputs (kluster, nodes)
    pub const B4: u32 = 0x10;
    pub const BC: u32 = 0x18;
    pub const C4: u32 = 0x20; // class id, then the second out word
    pub const A0: u32 = 0x28; // two out words of SI_elemdictFindRangeRelatedElem
    pub const XFORM: u32 = 0x30; // the 0x2c-byte transform record
    pub const FCURVE: u32 = 0x60; // a function curve (0x58 bytes)
}

/// [soft 0x080c72bc SI_historyUndoRedo]: undoes (`redo` 0) or redoes one cluster of the history.
/// The cluster is the chain of nodes from the undo position (walked from its end back) or the
/// redo position (walked forward); each node's change is swapped with the live data, so a
/// second pass over the same node puts it back. Returns 0 when the history is off, empty in
/// that direction, or a link record could not be applied (then the cursors and the
/// `SI_historyActive` word stay as they were: the original leaves them), else 1.
/// `SI_lightLicense` is read from the image as the original does.
pub fn history_undo_redo(m: &mut Mem, o: &mut dyn Outside, redo: i16) -> i32 {
    let h = hst(m);
    if m.i16(h + hst::ACTIVE) == 0 {
        return 0;
    }
    let mut cur = if redo != 0 { m.u32(h + hst::REDO) } else { m.u32(h + hst::UNDO) };
    if cur == 0 {
        return 0;
    }
    m.set_i16(g::SI_HISTORY_ACTIVE.0, 1);
    let f = m.local(fr::SIZE);
    // flags: keys edited (transform), spreadsheet, geometry relation
    let (mut keys, mut sprd, mut rel) = (false, false, false);
    let mut gn = 0u32; // [ebp-0xd4]
    let mut elem = 0u32; // [ebp-0xd8]
    if redo == 0 {
        while m.u32(cur + node::CHAIN_NEXT) != 0 {
            cur = m.u32(cur + node::CHAIN_NEXT);
        }
    }
    while cur != 0 {
        let kind = m.u32(cur + node::KIND);
        if kind == 8 {
            let dict = elem_dict(m);
            let cell = m.u32(cur + node::GN);
            let data = m.u32(cell);
            elem = o.si_elemdict_get_elem(m, dict, m[g::SI_CID_ANIMATION], data);
        } else {
            gn = m.u32(cur + node::GN);
        }
        let data = m.u32(cur + node::DATA);
        match kind {
            0 => {
                let list = graf_list(m);
                let (b, a) = (m.u32(gn + 4), m.u32(gn));
                gn = o.si_find_gl_data(m, list, b, a);
                if gn != 0 {
                    if m.u32(gn).wrapping_sub(2) < 2 {
                        let model = m.u32(gn + 4);
                        let rec = f + fr::XFORM;
                        // a record on the stack: the first eight bytes from the node's data
                        let (w0, w1, w2) = (m.u16(data), m.u16(data + 2), m.u32(data + 4));
                        m.set_u16(rec, w0);
                        m.set_u16(rec + 2, w1);
                        m.set_u32(rec + 4, w2);
                        model_hist_xform_snapshot(m, o, model, rec);
                        model_hist_xform_set(m, o, model, data);
                        for k in 0..9 {
                            let v = m.u32(rec + 8 + 4 * k);
                            m.set_u32(data + 8 + 4 * k, v);
                        }
                    }
                    o.si_refresh_counter(m);
                    keys = true;
                }
            }
            1 => {
                let list = graf_list(m);
                let (b, a) = (m.u32(gn + 4), m.u32(gn));
                gn = o.si_find_gl_data(m, list, b, a);
                if gn != 0 && m.u32(gn).wrapping_sub(2) < 2 {
                    let model = m.u32(gn + 4);
                    let target = m.u32(data + 4);
                    if target <= 4 {
                        undo_geometry(m, o, f, gn, model, data, target);
                    }
                    if matches!(target, 0 | 2 | 3) {
                        rel = true;
                    }
                }
            }
            2 => {
                let list = graf_list(m);
                let (b, a) = (m.u32(gn + 4), m.u32(gn));
                gn = o.si_find_gl_data(m, list, b, a);
                if gn != 0 && m.u32(gn).wrapping_sub(4) < 2 {
                    let model = m.u32(gn + 4);
                    camera_swap_history(m, model, data);
                }
            }
            3 => {
                let list = graf_list(m);
                let (b, a) = (m.u32(gn + 4), m.u32(gn));
                gn = o.si_find_gl_data(m, list, b, a);
                if gn != 0 {
                    let k = m.u32(gn);
                    if k.wrapping_sub(6) <= 2 || k == 0x13 || k == 0x14 {
                        let light = m.u32(gn + 4);
                        light_swap_history(m, o, light, data);
                    }
                }
            }
            4 => {
                // a deleted graph node comes back: it is added again, the record that stood
                // for it replaced everywhere, and the old record freed
                if m.u32(gn) == 7 {
                    m.set_u32(gn, 6);
                }
                m.set_i16(g::SI_ADD_IN_DICTIONARY.0, 0);
                let list = graf_list(m);
                let (b, a) = (m.u32(gn + 4), m.u32(gn));
                let new = o.si_graflist_add_data(m, list, b, a, 1);
                m.set_i16(g::SI_ADD_IN_DICTIONARY.0, 1);
                let c4 = f + fr::C4;
                elem = o.si_elemdict_find_elem_from_grafnode(m, gn, c4, c4 + 4);
                m.set_u32(cur + node::KIND, 5);
                history_fix_grafnode(m, gn, new);
                o.si_ed_graf_node_set_delete_flag(m, gn, 0, new, 0);
                if elem == 0 {
                    let list = graf_list(m);
                    o.si_schematic_adjust_new_node(m, list, new);
                }
                if m[ig::SI_LIGHT_LICENSE] != 0 {
                    let e = o.si_elemdict_find_elem_from_grafnode(m, new, c4, c4 + 4);
                    if m.u32(c4) == m[g::SI_CID_MODELS] {
                        verify_hier_nse_state(m, o, e);
                    }
                }
                if gn != 0 {
                    m.free(gn);
                    gn = 0;
                }
            }
            5 => {
                let list = graf_list(m);
                let (b, a) = (m.u32(gn + 4), m.u32(gn));
                gn = o.si_find_gl_data(m, list, b, a);
                if gn != 0 {
                    let list = graf_list(m);
                    o.si_graflist_delete_grafnode(m, list, gn);
                    m.set_u32(cur + node::KIND, 4);
                    m.set_u32(cur + node::GN, gn);
                }
            }
            6 | 7 => {
                let list = graf_list(m);
                let (b, a) = (m.u32(gn + 4), m.u32(gn));
                gn = o.si_find_gl_data(m, list, b, a);
                if gn != 0 {
                    let ok = if kind == 6 { history_delete_link(m, o, gn, data) } else { history_create_link(m, o, gn, data) };
                    if ok as i16 == 0 {
                        // the original returns here: no cursor change, history stays "active"
                        m.release(fr::SIZE);
                        return 0;
                    }
                    m.set_u32(cur + node::KIND, if kind == 6 { 7 } else { 6 });
                }
            }
            8 => {
                if elem != 0 && m.u16(elem + 0x48) == 0 {
                    let esi = m.u32(elem + 4);
                    let edi = data;
                    if esi != 0 && edi != 0 {
                        let loc = f + fr::FCURVE;
                        o.si_fcurve_initialize(m, loc);
                        o.si_fcurve_transfer(m, esi, loc);
                        let w = m.u32(esi + 0x54);
                        m.set_u32(loc + 0x54, w);
                        o.si_fcurve_remove_all_keys(m, esi);
                        o.si_fcurve_transfer(m, edi, esi);
                        let w = m.u32(edi + 0x54);
                        m.set_u32(esi + 0x54, w);
                        o.si_fcurve_remove_all_keys(m, edi);
                        o.si_fcurve_transfer(m, loc, edi);
                        let w = m.u32(loc + 0x54);
                        m.set_u32(edi + 0x54, w);
                    }
                }
            }
            9 => {}
            10 => {
                sprd = true;
                sprd_swap_history(m, o, data);
            }
            11 => {
                let (esi, edi, w8) = (m.u32(data), m.u32(data + 4), m.u32(data + 8));
                let id = m.u32(data + 0xc);
                let t = o.si_audio_mng_at_find_by_id(m, 0x087f_b9c0, id);
                if t != 0 {
                    for k in 0..3 {
                        let v = m.u32(t + 0xa4 + 4 * k);
                        m.set_u32(data + 4 * k, v);
                    }
                    m.set_u32(t + 0xa4, esi);
                    m.set_u32(t + 0xa8, edi);
                    m.set_u32(t + 0xac, w8);
                    if t == m[g::AUDIO_SHOWN_TRACK] {
                        let n = o.si_audio_marker_mng_count_selected(m, t);
                        m.set_i16(g::AUDIO_HAS_SELECTED.0, (n >= 0) as i16);
                    }
                }
            }
            12 | 13 => {
                let (w4, w0) = (m.u32(data + 4), m.u32(data));
                let dict = elem_dict(m);
                let e = o.si_elemdict_get_elem(m, dict, m.u32(data + 0xc), m.u32(data + 8));
                let dict = elem_dict(m);
                let r = o.si_elemdict_find_range_related_elem(m, dict, e, 1, 0, 0x3a98, 0x3aa2, m[g::SI_CID_ANIMATION], f + fr::A0, f + fr::A0);
                if r != 0 && m.u16(r + 0x48) == 0 {
                    let lists = m.u32(r + 4);
                    if lists != 0 {
                        let acts = if kind == 12 { lists + 0xc } else { lists + 0x14 };
                        let (a4, a0) = (m.u32(acts + 4), m.u32(acts));
                        m.set_u32(data + 4, a4);
                        m.set_u32(data, a0);
                        m.set_u32(acts + 4, w4);
                        m.set_u32(acts, w0);
                        if kind == 12 {
                            let shown = m[g::ACTIONS_SHOWN];
                            if acts == shown {
                                m[g::ACTIONS_SELECTION] = 0xffff_ffff;
                            } else {
                                o.si_actions_unselect_all(m, shown);
                            }
                            m[g::ACTIONS_SHOWN] = acts;
                        }
                    }
                }
            }
            _ => {}
        }
        cur = if redo != 0 { m.u32(cur + node::CHAIN_NEXT) } else { m.u32(cur + node::CHAIN_BACK) };
    }
    let h = hst(m);
    if redo != 0 {
        let r = m.u32(h + hst::REDO);
        m.set_u32(h + hst::UNDO, r);
        let n = m.u32(r + node::NEXT);
        m.set_u32(h + hst::REDO, n);
    } else {
        let u = m.u32(h + hst::UNDO);
        m.set_u32(h + hst::REDO, u);
        let p = m.u32(u + node::PREV);
        m.set_u32(h + hst::UNDO, p);
    }
    if keys && m[g::PREF_CONFIRM_KEYEDIT] == 0 {
        let frame = m[g::SI_FRAME_CURRENT];
        o.si_scene_key_edit(m, 1, frame, 1);
    }
    if sprd || keys {
        o.si_sprd_refresh_sprd_view(m, 0);
    }
    if rel {
        if o.si_update_geometry_relation(m) == 0 {
            o.si_refresh_system(m, 0, 0, 0x27);
            o.si_refresh_system(m, 0, 0, 7);
        }
    } else {
        o.si_scene_refresh(m);
    }
    m.set_i16(g::SI_HISTORY_ACTIVE.0, 0);
    m.release(fr::SIZE);
    1
}

/// The geometry arm of `SI_historyUndoRedo` (node kind 1): `rec` is the node's data (+0
/// the kind of geometry, +4 the target 0 to 4, +8 the count, +0xc the array or block), `gn`
/// the graph-list entry found for the node, `model` its model.
fn undo_geometry(m: &mut Mem, o: &mut dyn Outside, f: u32, gn: u32, model: u32, rec: u32, target: u32) {
    let lic = m[ig::SI_LIGHT_LICENSE] != 0;
    match target {
        0 | 1 => {
            if lic {
                let (a, b) = (m.u32(rec), m.u32(rec + 0xc));
                if o.si_wrath_unsupported_mdl_type2(m, a, b) != 0 {
                    return;
                }
            }
            o.si_model_swap_geometry(m, model, rec, rec + 8, rec + 0xc, rec + 0x10);
            o.si_patch_zip_update_index(m, model);
            o.si_update_gn_func(m, gn);
            if !lic {
                return;
            }
            let a4 = f + fr::A4;
            let mut editable = m.u8(gn + 0x17) & 0x10 != 0;
            if !editable {
                if o.si_wrath_unsupported_mdl_type(m, model) != 0 {
                    let e = o.si_elemdict_find_elem_from_grafnode(m, gn, a4, a4);
                    let dict = elem_dict(m);
                    o.si_ed_set_uneditable(m, dict, e);
                    return;
                }
                editable = m.u8(gn + 0x17) & 0x10 != 0;
            }
            if editable && o.si_wrath_unsupported_mdl_type(m, model) == 0 {
                let e = o.si_elemdict_find_elem_from_grafnode(m, gn, a4, a4);
                let dict = elem_dict(m);
                let cid = m[g::SI_CID_MODELS];
                o.si_ed_set_editable(m, dict, e, cid);
            }
        }
        2 | 3 => {
            let (n, arr) = (m.i16(rec + 8) as i32, m.u32(rec + 0xc));
            o.si_model_swap_lattice(m, model, (target == 3) as i32, n, arr);
        }
        _ => undo_clusters(m, o, f, model, rec),
    }
}

/// Target 4 of the geometry arm: the model's cluster list and its three user-data pairs
/// exchanged with the record's block (`rec + 0xc`).
fn undo_clusters(m: &mut Mem, o: &mut dyn Outside, f: u32, model: u32, rec: u32) {
    let dup = if m.u32(model + 0x348) != 0 { o.si_modelkluster_list_duplicate(m, model, 1, 0, 1) } else { 0 };
    let mut k = m.u32(model + 0x348);
    while k != 0 {
        m.set_u32(k + 0x10, 2);
        k = m.u32(k + 0x1c);
    }
    let blk = m.u32(rec + 0xc);
    if blk != 0 {
        let mut c = m.u32(blk);
        if c != 0 {
            loop {
                let list = m.u32(model + 0x348);
                let found = o.si_find_cluster(m, c, list, model);
                if found != 0 {
                    let t = o.si_find_cluster(m, c, dup, model);
                    let v = if t != 0 { m.u32(t + 0x10) } else { 0 };
                    m.set_u32(found + 0x10, v);
                } else {
                    let mut tail = m.u32(model + 0x348);
                    if tail == 0 {
                        m.set_u32(model + 0x348, c);
                    } else {
                        while m.u32(tail + 0x1c) != 0 {
                            tail = m.u32(tail + 0x1c);
                        }
                        m.set_u32(tail + 0x1c, c);
                    }
                    o.si_ed_add_cluster(m, model, c);
                }
                c = m.u32(c + 0x1c);
                if c == 0 {
                    break;
                }
            }
        }
        o.si_kluster_dispose_flagged(m, model);
        o.si_cluster_model_update(m, model);
        let blk = m.u32(rec + 0xc);
        m.set_u32(blk, dup);
    }
    // the three user-data pairs: (model offsets, block offsets, local slot)
    for (mo, bo, slot) in [(0x408u32, 4u32, fr::AC), (0x410, 0xc, fr::B4), (0x418, 0x14, fr::BC)] {
        let (k, n) = (m.u32(model + mo), m.u32(model + mo + 4));
        if k == 0 || n == 0 {
            continue;
        }
        let blk = m.u32(rec + 0xc);
        if blk != 0 {
            let (pk, pn) = (f + slot, f + slot + 4);
            o.si_subel_user_data_copy_from_setup(m, k, n, pk, pn);
            let blk = m.u32(rec + 0xc);
            let (bk, bn) = (m.u32(blk + bo), m.u32(blk + bo + 4));
            m.set_u32(model + mo, bk);
            m.set_u32(model + mo + 4, bn);
            let blk = m.u32(rec + 0xc);
            let (lk, ln) = (m.u32(pk), m.u32(pn));
            m.set_u32(blk + bo, lk);
            let blk = m.u32(rec + 0xc);
            m.set_u32(blk + bo + 4, ln);
        } else {
            m.set_u32(model + mo, 0);
            m.set_u32(model + mo + 4, 0);
        }
    }
}
