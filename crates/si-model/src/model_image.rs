// SPDX-License-Identifier: MIT
//! `model.c` on the shared memory image (finding 0024; fourth pass of finding 0015).
//!
//! The functions here take the image and what the original takes: a node is an
//! `__SI_Model` at an address ([`si_types::model::Model`]), a global is at its address, a
//! block is what the original's allocation call returns. They are written from the
//! original's code, beside the earlier forms of this crate that keep nodes in stores of
//! their own ([`crate::transform`], [`crate::setters`], [`crate::node_copy`],
//! [`crate::hierarchy`]), which stay in use by the crate's other modules.
//!
//! | Module | Content |
//! |---|---|
//! | [`state`] | a node's global values from its local ones (`SI_modelUpdateSonState`), the classical method's bookkeeping, a transformation copied from node to node, the matrices of parenting and cutting, the manipulators' reference frames |
//! | [`tree`] | the walks over a tree of nodes, a node taken from its father, `SI_modelCut`, `SI_modelCreateParent` |
//! | [`defs`] | definitions: inserting, deleting, substituting, exchanging; the lattice arrays; the fitting queries; what counts as deformed; what the tools may transform |
//! | [`setup`] | the copies Duplicate makes "from setup": user data, shapes, clusters, animation, and the walk over two trees |
//! | [`points`] | a definition's points by number, whatever its kind |
//! | [`bounds`] | the bounding boxes: of a node's working definition, of its control points, of a tree, of an element seen through a matrix |
//! | [`setters`] | the setters the animation update calls (local rotation, the three in global terms; local scaling, translation, all three), the compensation of sons and geometry, the sons' initial poses, a node's global values |
//! | [`kine`] | a node's velocities and accelerations from frame to frame (`SI_modelKineStateCompute`, `settings.c`) |
//! | [`readjust`] | a node's working definition rebuilt from its stored one and what deforms it (`SI_modelReadjustDefinition`), a definition through a matrix (`SI_modelMultiplyDef`), the working definition given up (`SI_modelDeleteDefinition`), the eight questions of what deforms a node |
//! | [`fit`] | a definition bent along a curve (`SI_splineFit`), every case |
//! | [`touch`] | a node's geometry marked as changed (`SI_modelTouchDefinition`, `SI_modelTouchGeometry`) and drawn (`SI_modelDraw`) |
//! | [`metaball`] | the small routines of `metaball.c` that `SI_metaballTouch` calls: the system a ball hangs under, a ball's box put into another (`metaballCheckBoundingBox`) |
//! | [`fastplay`] | `fastplay.c`: a window's fast-play table marked dirty (`SI_fastSetDirty`) |
//! | [`triangle`] | `grTriangle.c`: a node's render data and triangle mesh given up (`SI_removeTriangle`, `SI_mdlRemoveTmesh`) |
//!
//! **Calls out of the package** go through one trait, [`Outside`]: the callee is named by
//! its address in the Linux binary and gets the 32-bit words the original pushes, in C
//! order. The table [`callee`] lists every such routine as `name (0xADDRESS)`. Nine
//! routines of `model.c` itself that exist in this crate only in their store form are in
//! the same table, marked. No adapter from the image to the stores is written for them
//! (a node's attachments have no counterpart in [`crate::node::Models`] to copy from):
//! until their image forms exist, whoever wires the program has to answer those nine
//! calls, and a check answers them from the recorded original.
//!
//! **Mathematics** (`algebra.c`) is `si-math`, a leaf library on values: [`mx`] holds the
//! calls with addresses, each reading its operands from the image and storing what the
//! original stores.
//!
//! Records whose layout `si-types` has are used through it; a field of a record without an
//! established layout is addressed by a named offset ([`off`]), and the place says so.

use si_math::types::{Matrix, Vector as MVector};
use si_mem::{Mem, Ptr32};

pub mod bounds;
pub mod defs;
pub mod dump;
pub mod fastplay;
pub mod fit;
pub mod kine;
pub mod metaball;
pub mod patch_fit;
pub mod points;
pub mod readjust;
pub mod setters;
pub mod setup;
pub mod state;
pub mod touch;
pub mod tree;
pub mod triangle;
pub mod zip;

/// A routine of the original that is called, not reproduced, here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Callee {
    /// The routine's name in the Linux binary, without its parameter types.
    pub name: &'static str,
    /// Its address there.
    pub addr: u32,
}

/// The rest of the program, as the functions of this module call it.
pub trait Outside {
    /// Calls `callee` with the 32-bit words the original pushes, first argument first (a
    /// `float` is its bits, a pointer its address), and answers what it leaves in `eax`.
    fn call(&mut self, m: &mut Mem, callee: &Callee, args: &[u32]) -> u32;
}

/// The name [`through_pointer`] gives a routine it knows only by its address.
pub const THROUGH_POINTER: &str = "(through a pointer)";

/// A routine reached through a function pointer the caller handed in (a callback): only
/// its address in the binary is known here.
pub fn through_pointer(addr: u32) -> Callee {
    Callee { name: THROUGH_POINTER, addr }
}

/// An outside world in which every routine does nothing and answers 0.
pub struct NoOutside;

impl Outside for NoOutside {
    fn call(&mut self, _: &mut Mem, _: &Callee, _: &[u32]) -> u32 {
        0
    }
}

macro_rules! callees {
    ($($(#[$doc:meta])* $ident:ident = $name:literal, $addr:literal;)*) => {
        $($(#[$doc])* pub const $ident: Callee = Callee { name: $name, addr: $addr };)*
        /// Every routine in the table.
        pub const ALL: &[Callee] = &[$($ident),*];
    };
}

/// The routines called through [`Outside`], each `name (0xADDRESS)`.
pub mod callee {
    use super::Callee;

    callees! {
        // ---- other source files
        /// `SI_windowGetNextActive` (0x083d1ca4), taking the address of a port variable
        SI_WINDOW_GET_NEXT_ACTIVE = "SI_windowGetNextActive", 0x083d1ca4;
        /// `SI_metaballGetParentSystem` (0x0845cd40): on the image now
        /// ([`super::metaball::get_parent_system`]); the callers of the fourth pass still reach it here
        SI_METABALL_GET_PARENT_SYSTEM = "SI_metaballGetParentSystem", 0x0845cd40;
        /// `SI_metaballTouch` (0x0845abf8)
        SI_METABALL_TOUCH = "SI_metaballTouch", 0x0845abf8;
        /// `SI_envelopCut` (0x084bc4b8)
        SI_ENVELOP_CUT = "SI_envelopCut", 0x084bc4b8;
        /// `SI_modelCompensateLocalCoord` (0x081cb9cc)
        SI_MODEL_COMPENSATE_LOCAL_COORD = "SI_modelCompensateLocalCoord", 0x081cb9cc;
        /// `SI_findGlData` (0x0832ee68)
        SI_FIND_GL_DATA = "SI_findGlData", 0x0832ee68;
        /// `SI_edGetElemFromModel` (0x080fcf20)
        SI_ED_GET_ELEM_FROM_MODEL = "SI_edGetElemFromModel", 0x080fcf20;
        /// `SI_elemdictFindRelatedElem` (0x0813508c)
        SI_ELEMDICT_FIND_RELATED_ELEM = "SI_elemdictFindRelatedElem", 0x0813508c;
        /// `SI_modelWave` (0x084c759c), `wave_sys.c`: on the image now
        /// ([`super::readjust::model_wave`])
        SI_MODEL_WAVE = "SI_modelWave", 0x084c759c;
        /// `SI_quickStretchActive` (0x085a23c0)
        SI_QUICK_STRETCH_ACTIVE = "SI_quickStretchActive", 0x085a23c0;
        /// `SI_envelopActive` (0x084bb9b4)
        SI_ENVELOP_ACTIVE = "SI_envelopActive", 0x084bb9b4;
        /// `SI_patchDispose` (0x0847a530)
        SI_PATCH_DISPOSE = "SI_patchDispose", 0x0847a530;
        /// `SI_faceDispose` (0x08434828)
        SI_FACE_DISPOSE = "SI_faceDispose", 0x08434828;
        /// `SI_meshDispose` (0x084404a0)
        SI_MESH_DISPOSE = "SI_meshDispose", 0x084404a0;
        /// `SI_splineDispose` (0x0848ab48)
        SI_SPLINE_DISPOSE = "SI_splineDispose", 0x0848ab48;
        /// `SI_metaballDispose` (0x0845ab04)
        SI_METABALL_DISPOSE = "SI_metaballDispose", 0x0845ab04;
        /// `SI_nurbsCrvDispose` (0x084db870)
        SI_NURBS_CRV_DISPOSE = "SI_nurbsCrvDispose", 0x084db870;
        /// `SI_nurbsSrfDispose` (0x084dba5c)
        SI_NURBS_SRF_DISPOSE = "SI_nurbsSrfDispose", 0x084dba5c;
        /// `SI_edIsItemConstrained` (0x08106bbc)
        SI_ED_IS_ITEM_CONSTRAINED = "SI_edIsItemConstrained", 0x08106bbc;
        /// `SI_mainGlobEnv` (0x084bb94c)
        SI_MAIN_GLOB_ENV = "SI_mainGlobEnv", 0x084bb94c;
        /// `SI_faceCopy` (0x08434990)
        SI_FACE_COPY = "SI_faceCopy", 0x08434990;
        /// `SI_patchCopy` (0x0847a2b8)
        SI_PATCH_COPY = "SI_patchCopy", 0x0847a2b8;
        /// `SI_meshCopy` (0x084404e0)
        SI_MESH_COPY = "SI_meshCopy", 0x084404e0;
        /// `SI_splineCopy` (0x0848aa74)
        SI_SPLINE_COPY = "SI_splineCopy", 0x0848aa74;
        /// `SI_nurbsCrvCopy` (0x084dbbf4)
        SI_NURBS_CRV_COPY = "SI_nurbsCrvCopy", 0x084dbbf4;
        /// `SI_nurbsSrfCopy` (0x084dc1a8)
        SI_NURBS_SRF_COPY = "SI_nurbsSrfCopy", 0x084dc1a8;
        /// `SI_envelopCompUpdate` (0x084b6210)
        SI_ENVELOP_COMP_UPDATE = "SI_envelopCompUpdate", 0x084b6210;
        /// `SI_patchUpdateSurface` (0x0847cee4)
        SI_PATCH_UPDATE_SURFACE = "SI_patchUpdateSurface", 0x0847cee4;
        /// `SI_patchUpdateMatrix` (0x0847a680)
        SI_PATCH_UPDATE_MATRIX = "SI_patchUpdateMatrix", 0x0847a680;
        /// `SI_nurbsSrfDisposeLines` (0x084e5468)
        SI_NURBS_SRF_DISPOSE_LINES = "SI_nurbsSrfDisposeLines", 0x084e5468;
        /// `SI_meshTransferVertices` (0x08446d00)
        SI_MESH_TRANSFER_VERTICES = "SI_meshTransferVertices", 0x08446d00;
        /// `SI_nurbsSrfTransferPointsKnots` (0x084ea07c)
        SI_NURBS_SRF_TRANSFER_POINTS_KNOTS = "SI_nurbsSrfTransferPointsKnots", 0x084ea07c;
        /// `SI_faceTransferPoints` (0x0843672c)
        SI_FACE_TRANSFER_POINTS = "SI_faceTransferPoints", 0x0843672c;
        /// `SI_splineTransferPoints` (0x08490664)
        SI_SPLINE_TRANSFER_POINTS = "SI_splineTransferPoints", 0x08490664;
        /// `SI_nurbsCrvTransferPointsKnots` (0x084ea00c)
        SI_NURBS_CRV_TRANSFER_POINTS_KNOTS = "SI_nurbsCrvTransferPointsKnots", 0x084ea00c;
        /// `SI_patchTransferPoints` (0x084810cc)
        SI_PATCH_TRANSFER_POINTS = "SI_patchTransferPoints", 0x084810cc;
        /// `SI_klusterListAssignToModel` (0x0843cd6c)
        SI_KLUSTER_LIST_ASSIGN_TO_MODEL = "SI_klusterListAssignToModel", 0x0843cd6c;
        /// `SI_modelklusterListDuplicate` (0x0843cc70)
        SI_MODELKLUSTER_LIST_DUPLICATE = "SI_modelklusterListDuplicate", 0x0843cc70;
        /// `SI_clusterModelUpdate` (0x081f1a1c)
        SI_CLUSTER_MODEL_UPDATE = "SI_clusterModelUpdate", 0x081f1a1c;
        /// `SI_klusterAllocate` (0x0843a9b0)
        SI_KLUSTER_ALLOCATE = "SI_klusterAllocate", 0x0843a9b0;
        /// `SI_elemdictCopyElemAnim` (0x0822bd8c)
        SI_ELEMDICT_COPY_ELEM_ANIM = "SI_elemdictCopyElemAnim", 0x0822bd8c;
        /// `SI_fcvTableSet` (0x0822b3c4)
        SI_FCV_TABLE_SET = "SI_fcvTableSet", 0x0822b3c4;
        /// `SI_pathElementSet` (0x082200c0)
        SI_PATH_ELEMENT_SET = "SI_pathElementSet", 0x082200c0;
        /// `SI_attdictSetFcurveFlag` (0x0811ff18)
        SI_ATTDICT_SET_FCURVE_FLAG = "SI_attdictSetFcurveFlag", 0x0811ff18;
        /// `SI_edGetElemFromShape` (0x080fd4dc)
        SI_ED_GET_ELEM_FROM_SHAPE = "SI_edGetElemFromShape", 0x080fd4dc;
        /// `SI_edAddShape` (0x081036a8)
        SI_ED_ADD_SHAPE = "SI_edAddShape", 0x081036a8;
        /// `SI_setFcvTableEntriesFromChap` (0x08234e3c)
        SI_SET_FCV_TABLE_ENTRIES_FROM_CHAP = "SI_setFcvTableEntriesFromChap", 0x08234e3c;
        /// `SI_updateWeightFcvNames` (0x0810b094)
        SI_UPDATE_WEIGHT_FCV_NAMES = "SI_updateWeightFcvNames", 0x0810b094;
        /// `si_deformationCopyFromSetup` (0x0843a600)
        SI_DEFORMATION_COPY_FROM_SETUP = "si_deformationCopyFromSetup", 0x0843a600;
        /// `SI_materialCopyShareSetup` (0x08294dac)
        SI_MATERIAL_COPY_SHARE_SETUP = "SI_materialCopyShareSetup", 0x08294dac;
        /// `SI_txtCopyFromSetup` (0x082954c0)
        SI_TXT_COPY_FROM_SETUP = "SI_txtCopyFromSetup", 0x082954c0;
        /// `SI_elemdictAddRelation` (0x08132f78)
        SI_ELEMDICT_ADD_RELATION = "SI_elemdictAddRelation", 0x08132f78;
        /// `SI_latticeCopyFromSetup` (0x0843de60)
        SI_LATTICE_COPY_FROM_SETUP = "SI_latticeCopyFromSetup", 0x0843de60;
        /// `SI_error` (0x081417d0)
        SI_ERROR = "SI_error", 0x081417d0;
        /// `SI_schematicCheckDeleteExpand` (0x08350414)
        SI_SCHEMATIC_CHECK_DELETE_EXPAND = "SI_schematicCheckDeleteExpand", 0x08350414;
        /// `SI_dynaPhyPropsValidateCopyDelete` (0x084a16f8)
        SI_DYNA_PHY_PROPS_VALIDATE_COPY_DELETE = "SI_dynaPhyPropsValidateCopyDelete", 0x084a16f8;
        /// `SI_modelSaveHistoryParenting` (0x080ca4d4)
        SI_MODEL_SAVE_HISTORY_PARENTING = "SI_modelSaveHistoryParenting", 0x080ca4d4;
        /// `SI_dynaPhyPropsSetNewRoot` (0x084a15a4)
        SI_DYNA_PHY_PROPS_SET_NEW_ROOT = "SI_dynaPhyPropsSetNewRoot", 0x084a15a4;
        /// `SI_graflistCutModel` (0x0833562c)
        SI_GRAFLIST_CUT_MODEL = "SI_graflistCutModel", 0x0833562c;
        /// `SI_graflistLinkModel` (0x083353fc)
        SI_GRAFLIST_LINK_MODEL = "SI_graflistLinkModel", 0x083353fc;
        /// `SI_edCutModel` (0x080fe5ec)
        SI_ED_CUT_MODEL = "SI_edCutModel", 0x080fe5ec;
        /// `SI_nurbsSrfInverse` (0x084dcaf8)
        SI_NURBS_SRF_INVERSE = "SI_nurbsSrfInverse", 0x084dcaf8;
        /// `SI_faceInverse` (0x08434bf8)
        SI_FACE_INVERSE = "SI_faceInverse", 0x08434bf8;
        /// `SI_indexModifPropagate` (0x08176988)
        SI_INDEX_MODIF_PROPAGATE = "SI_indexModifPropagate", 0x08176988;
        /// `SI_meshInverse` (0x0844462c)
        SI_MESH_INVERSE = "SI_meshInverse", 0x0844462c;
        /// `SI_indexModifAllocate` (0x08176860)
        SI_INDEX_MODIF_ALLOCATE = "SI_indexModifAllocate", 0x08176860;
        /// `SI_splineInverse` (0x0848e35c)
        SI_SPLINE_INVERSE = "SI_splineInverse", 0x0848e35c;
        /// `SI_nurbsCrvInverse` (0x084dca24)
        SI_NURBS_CRV_INVERSE = "SI_nurbsCrvInverse", 0x084dca24;
        /// `SI_patchZipUpdateIndex` (0x081a3e00)
        SI_PATCH_ZIP_UPDATE_INDEX = "SI_patchZipUpdateIndex", 0x081a3e00;
        /// `SI_patchInverse` (0x0847af64)
        SI_PATCH_INVERSE = "SI_patchInverse", 0x0847af64;
        /// `SI_nurbsCrvMovePoint` (0x084e9eec)
        SI_NURBS_CRV_MOVE_POINT = "SI_nurbsCrvMovePoint", 0x084e9eec;
        /// `SI_nurbsSrfMovePoint` (0x084e9f2c)
        SI_NURBS_SRF_MOVE_POINT = "SI_nurbsSrfMovePoint", 0x084e9f2c;
        /// `SI_nurbsCrvGetCtrlVertex` (0x084ea7a0)
        SI_NURBS_CRV_GET_CTRL_VERTEX = "SI_nurbsCrvGetCtrlVertex", 0x084ea7a0;
        /// `SI_nurbsSrfGetCtrlVertex` (0x084ea800)
        SI_NURBS_SRF_GET_CTRL_VERTEX = "SI_nurbsSrfGetCtrlVertex", 0x084ea800;
        /// `SI_faceGlobalToLocalIndex` (0x08434c80); store form: `crate::face`
        SI_FACE_GLOBAL_TO_LOCAL_INDEX = "SI_faceGlobalToLocalIndex", 0x08434c80;
        /// `si_metaballContourTraversePnts` (0x084614b0)
        SI_METABALL_CONTOUR_TRAVERSE_PNTS = "si_metaballContourTraversePnts", 0x084614b0;
        /// `SI_nurbsCrvConvertToLinear` (0x084e0eb4)
        SI_NURBS_CRV_CONVERT_TO_LINEAR = "SI_nurbsCrvConvertToLinear", 0x084e0eb4;
        /// `SI_nurbSrfUpdateLines` (0x084e432c)
        SI_NURB_SRF_UPDATE_LINES = "SI_nurbSrfUpdateLines", 0x084e432c;
        /// `SI_metaSysGetSurface` (0x0845b2c4)
        SI_META_SYS_GET_SURFACE = "SI_metaSysGetSurface", 0x0845b2c4;
        /// `SI_clustersApplyTransformation` (0x0843d608)
        SI_CLUSTERS_APPLY_TRANSFORMATION = "SI_clustersApplyTransformation", 0x0843d608;
        /// `SI_modelCompensateAngle` (0x081cafa0), `classic.c`
        SI_MODEL_COMPENSATE_ANGLE = "SI_modelCompensateAngle", 0x081cafa0;
        /// `SI_removeTriangle` (0x085efc38), `grTriangle.c`: on the image now
        /// ([`super::triangle::remove_triangle`]); the touch and the readjusting still call
        /// it through here
        SI_REMOVE_TRIANGLE = "SI_removeTriangle", 0x085efc38;
        /// `SI_glDeleteDispList` (0x085f2800), `ogl.c`, not ported: a node's display list
        /// given back to GL (`glDeleteLists`)
        SI_GL_DELETE_DISP_LIST = "SI_glDeleteDispList", 0x085f2800;
        /// `SI_splineTouch` (0x0848c924)
        SI_SPLINE_TOUCH = "SI_splineTouch", 0x0848c924;
        /// `si_meshTouch` (0x08448b44); store form: `crate::obj_global`
        SI_MESH_TOUCH = "si_meshTouch", 0x08448b44;
        /// `SI_nurbsCrvAnimUpdate` (0x08501d90)
        SI_NURBS_CRV_ANIM_UPDATE = "SI_nurbsCrvAnimUpdate", 0x08501d90;
        /// `SI_modelNodeWave` (0x084c7610), `wave_sys.c`; store form: `crate::node_copy`
        SI_MODEL_NODE_WAVE = "SI_modelNodeWave", 0x084c7610;
        /// `SI_modelBranchWave` (0x084c75dc), `wave_sys.c`; store form: `crate::node_copy`
        SI_MODEL_BRANCH_WAVE = "SI_modelBranchWave", 0x084c75dc;
        /// `pushmatrix` (0x0861052c), the GL layer
        PUSHMATRIX = "pushmatrix", 0x0861052c;
        /// `multmatrix` (0x08610514), the GL layer
        MULTMATRIX = "multmatrix", 0x08610514;
        /// `popmatrix` (0x0861053c), the GL layer
        POPMATRIX = "popmatrix", 0x0861053c;
        /// `SI_modelAllocate` (0x08477cf4), `model_b.c` (`si-rensetup` has its image form, which
        /// this crate cannot reach): a new node with a copy of the name
        SI_MODEL_ALLOCATE = "SI_modelAllocate", 0x08477cf4;
        /// `meshRevolveSpline` (0x08198afc): the mesh of a spline revolved
        MESH_REVOLVE_SPLINE = "meshRevolveSpline", 0x08198afc;
        /// `SI_modelDisposeNode` (0x08461a54), taking the address of the node pointer
        SI_MODEL_DISPOSE_NODE = "SI_modelDisposeNode", 0x08461a54;
        /// `SI_patchDraw` (0x0847d504)
        SI_PATCH_DRAW = "SI_patchDraw", 0x0847d504;
        /// `SI_faceDraw` (0x08434b20); store form: `crate::face`
        SI_FACE_DRAW = "SI_faceDraw", 0x08434b20;
        /// `SI_meshDraw` (0x08446de8)
        SI_MESH_DRAW = "SI_meshDraw", 0x08446de8;
        /// `SI_splineDraw` (0x0848b510)
        SI_SPLINE_DRAW = "SI_splineDraw", 0x0848b510;
        /// `SI_nurbsCrvDraw` (0x084e2e50)
        SI_NURBS_CRV_DRAW = "SI_nurbsCrvDraw", 0x084e2e50;
        /// `SI_nurbsSrfDraw` (0x084e471c)
        SI_NURBS_SRF_DRAW = "SI_nurbsSrfDraw", 0x084e471c;
        /// `SI_patchTransform` (0x084866a4)
        SI_PATCH_TRANSFORM = "SI_patchTransform", 0x084866a4;
        /// `SI_faceTransform` (0x08436c88); store form: `crate::face`
        SI_FACE_TRANSFORM = "SI_faceTransform", 0x08436c88;
        /// `SI_meshTransform` (0x08441cd8); store form: `crate::obj_global::mesh_transform`
        SI_MESH_TRANSFORM = "SI_meshTransform", 0x08441cd8;
        /// `SI_splineTransform` (0x08494138)
        SI_SPLINE_TRANSFORM = "SI_splineTransform", 0x08494138;
        /// `SI_nurbsCrvApplyMatrix` (0x084e9750)
        SI_NURBS_CRV_APPLY_MATRIX = "SI_nurbsCrvApplyMatrix", 0x084e9750;
        /// `SI_nurbsSrfApplyMatrix` (0x084e9644)
        SI_NURBS_SRF_APPLY_MATRIX = "SI_nurbsSrfApplyMatrix", 0x084e9644;
        // ---- `model.c` itself: in this crate in store form only, not on the image yet.
        // Not in place: these calls leave the image form; no adapter serves them here.
        /// `SI_modelReadjustDefinition` (0x0846e340); store form: `crate::node_copy::readjust_definition`
        SI_MODEL_READJUST_DEFINITION = "SI_modelReadjustDefinition", 0x0846e340;
        /// `SI_modelDeleteDefinition` (0x084678a8); store form: `crate::node_copy::delete_definition`
        SI_MODEL_DELETE_DEFINITION = "SI_modelDeleteDefinition", 0x084678a8;
        /// `SI_modelTouchGeometry` (0x0846d48c): on the image now ([`super::touch::touch_geometry`]);
        /// the fourth pass's forms still call it through here
        SI_MODEL_TOUCH_GEOMETRY = "SI_modelTouchGeometry", 0x0846d48c;
        /// `SI_modelTouchDefinition` (0x0846d530): on the image now ([`super::touch::touch_definition`]);
        /// the fourth pass's forms still call it through here
        SI_MODEL_TOUCH_DEFINITION = "SI_modelTouchDefinition", 0x0846d530;
        /// `SI_modelUpdateDependency` (0x0846f200); store form: `crate::node_copy::update_dependency`
        SI_MODEL_UPDATE_DEPENDENCY = "SI_modelUpdateDependency", 0x0846f200;
        /// `compensateInit_srt` (0x08465e3c): on the image now
        /// ([`super::setters::compensate_init_srt`]); `SI_modelCompensateParentingMatrix`
        /// still calls it through here, as its store form's partner
        /// `computeInit_srtParenting` is not on the image
        COMPENSATE_INIT_SRT = "compensateInit_srt", 0x08465e3c;
        /// `computeInit_srtParenting` (0x0846613c): on the image now
        /// ([`super::setters::compute_init_srt_parenting`]); `SI_modelCompensateParentingMatrix`
        /// still calls it through here
        COMPUTE_INIT_SRT_PARENTING = "computeInit_srtParenting", 0x0846613c;
        /// `SI_matrixRotationRefer` (0x08463ad4); store form: `crate::setters::matrix_rotation_refer`
        SI_MATRIX_ROTATION_REFER = "SI_matrixRotationRefer", 0x08463ad4;
        /// `SI_modelTransformationMatrix` (0x084738f0); store form: `crate::setters::transformation_matrix`
        SI_MODEL_TRANSFORMATION_MATRIX = "SI_modelTransformationMatrix", 0x084738f0;
    }
}

/// Offsets into records that have no established layout (each named where it is used).
pub mod off {
    /// `SI_InvKin` (what `__SI_Model` + 0x318 points to): the first word, the kind of
    /// inverse kinematics of a chain root.
    pub const INV_KIN_KIND: u32 = 0x00;
    /// `SI_MetaSystem` (what `SI_metaballGetParentSystem` answers): a 16-bit flag word,
    /// negative while the system is being rebuilt.
    pub const META_SYSTEM_FLAGS: u32 = 0x08;
}

si_mem::record! {
    /// `SI_glbManip`: the manipulation settings (setup keywords `PREF_…`). Only the fields
    /// some routine of `model.c` reads are named.
    pub struct GlbManip = "SI_glbManip", size 0x3c {
        0x00 scaling_method: i32, "1: the classical scaling method (a node's global matrix is its local matrix times its father's)";
        0x04 unk_04: i32, "not read here";
        0x08 rot_mode: i32, "PREF_ROTATION: how an added rotation is meant";
        0x0c trans_mode: i32, "PREF_TRANSLATION: how an added translation is meant";
        0x10 unk_10: [u32; 2], "not read here";
        0x18 reference: Ptr32<si_mem::Opaque>, "the reference object of the modes 3: a record whose word at +4 is the model";
        0x1c unk_1c: i16, "not read here";
        0x1e hierarchy_manipulation: i16, "PREF_HIERARCHY_MANIPULATION";
        0x20 keep_turns: i16, "angles taken out of a matrix stay within a turn of the previous ones";
        0x22 unk_22: [u8; 0x1a], "not read here";
    }
}

/// The global variables the routines of this module read or write.
pub mod g {
    use super::GlbManip;
    use si_math::types::Matrix;

    si_mem::globals! {
        /// The manipulation settings.
        SI_GLB_MANIP: GlbManip = 0x08781560, "SI_glbManip";
        /// Bit 0x10: `def_matrix` is not maintained. Bit 0x02: an update goes down to the
        /// sons only, not on to the brothers.
        SI_VIEWER_UPD_OPT: i32 = 0x08679678, "SI_viewerUpdOpt", init 0;
        /// The identity matrix (`.data`; [`super::init`] stores it).
        SI_IDENTITE: Matrix = 0x086dc160, "SI_identite";
        /// `SI_glbAnim`: the animation evaluation's state. The 16-bit word at +6 is its
        /// "in update" mark.
        SI_GLB_ANIM: [u16; 0x20] = 0x08781480, "SI_glbAnim";
        /// The process's data: its first word numbers the scene (an element dictionary of
        /// 0x70 bytes each), its second the list of displayed nodes (0x24 bytes each).
        SI_PROC_DATA: si_mem::Ptr32<[i32; 3]> = 0x08679314, "SI_procData", init 0x08782108;
        /// The element dictionaries.
        SI_ELEM_DICTS: u32 = 0x0867931c, "SI_elemDicts", init 0;
        /// The lists of displayed nodes.
        SI_GRAF_LISTS: u32 = 0x08679498, "SI_grafLists", init 0;
        /// The chapter numbers; the word at +0x20 is the chapter of the models.
        SI_CID: [u32; 0x24] = 0x08781660, "SI_cid";
        /// The settings of the Duplicate command (the offsets read are named in
        /// [`super::setup`]).
        SI_GLB_DUPLIC: [u16; 0x2c] = 0x087814c0, "SI_glbDuplic";
        /// The attribute dictionary (only its address is passed on).
        SI_ATD: [u32; 7] = 0x08781438, "SI_ATD";
        /// The relation dictionary (only its address is passed on).
        SI_RD: [u32; 4] = 0x087815f0, "SI_RD";
        /// Not zero (as 16 bits) during an export to the successor product.
        SI_ED_SUMATRA_TO_3X: i16 = 0x08680e50, "SI_edSumatraTo3x", init 0;
        /// Not zero: parenting and cutting also keep the initial pose (`init_srt`).
        SI_INIT_POSE_COMPENSATE: i16 = 0x087817a0, "SI_initPoseCompensate";
        /// Not zero: a node keeps its place when it is cut from its father.
        SI_HRCH_CPST: i16 = 0x0867949c, "SI_hrchCpst", init 1;
        /// The index of modifications of the tool that is running.
        SI_GBL_INDEX: u32 = 0x0878896c, "SI_gblIndex";
        /// The last error's number, as `SI_error` leaves it.
        SI_ERRNO: i32 = 0x08687170, "SI_errno", init 0;
        SI_FRAME_CURRENT: i32 = 0x08782040, "SI_frameCurrent";
        SI_FRAME_RATE: f32 = 0x0878204c, "SI_frameRate";
        /// `glbInit_srt` (`model.c`, file scope): the scratch list of the sons' initial
        /// poses in global terms, 0x58 bytes an entry (scaling, rotation matrix, position),
        /// from `computeInit_srtGlobalTransfo` to `compensateInit_srt`.
        GLB_INIT_SRT: u32 = 0x086d5e20, "glbInit_srt";
    }
}

/// The three messages of `SI_modelCut`, strings of `.data`.
/// The names `si_createPatchHierarchyFromSpline` gives the nodes it makes, strings of
/// `.rodata` (0x08689ea0 ..): the bottom and top caps, an extrusion, a revolution and the
/// null that holds them together.
pub(crate) const PATCH_NAMES: [(u32, &[u8]); 5] = [
    (0x0868_9ea0, b"bottom"),
    (0x0868_9ea7, b"top"),
    (0x0868_9eab, b"extru"),
    (0x0868_9eb1, b"revol"),
    (0x0868_9eb7, b"null"),
];

/// The addresses of the messages of `SI_modelCut`; their text is the original's own and is
/// read from the user's install when [`init`] copies it into the image.
pub(crate) const CUT_MESSAGES: [u32; 3] = [0x086d_5e40, 0x086d_5e60, 0x086d_5ea0];

/// Stores the initial values of this module's globals that are not zero, and the constants
/// of `.data` it reads from the image: `SI_identite` and the messages of `SI_modelCut`.
pub fn init(m: &mut Mem) {
    si_mem::init_globals(m, g::ALL);
    m[g::SI_IDENTITE] = si_math::types::IDENTITY;
    for addr in CUT_MESSAGES {
        m.put_cstr(addr, si_rt::message(addr));
    }
    for (addr, text) in PATCH_NAMES {
        m.put_cstr(addr, text);
    }
}

/// The address of a node's field: `&node->field` as a number.
macro_rules! at {
    ($node:expr, $field:ident) => {
        si_mem::field!($node, si_types::model::Model, $field).addr()
    };
}
pub(crate) use at;

/// The current scene's element dictionary: `SI_elemDicts` + 0x70 × the first word of the
/// process's data.
pub(crate) fn elem_dict(m: &Mem) -> u32 {
    let scene = m.i32(m[g::SI_PROC_DATA].addr());
    m[g::SI_ELEM_DICTS].wrapping_add((scene as u32).wrapping_mul(0x70))
}

/// The chapter of the models (`SI_cid` + 0x20).
pub(crate) fn models_chapter(m: &Mem) -> u32 {
    m[g::SI_CID][8]
}

/// Three floats at an address as `si-math`'s vector.
pub(crate) fn vec3(m: &Mem, a: u32) -> MVector {
    MVector::new(m.f32(a), m.f32(a + 4), m.f32(a + 8))
}

pub(crate) fn put_vec3(m: &mut Mem, a: u32, v: MVector) {
    m.set_f32(a, v.x);
    m.set_f32(a + 4, v.y);
    m.set_f32(a + 8, v.z);
}

/// Sixteen floats at an address.
pub(crate) fn mat(m: &Mem, a: u32) -> Matrix {
    let mut out = [[0.0f32; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, e) in row.iter_mut().enumerate() {
            *e = m.f32(a + (16 * i + 4 * j) as u32);
        }
    }
    out
}

pub(crate) fn put_mat(m: &mut Mem, a: u32, v: &Matrix) {
    for (i, row) in v.iter().enumerate() {
        for (j, e) in row.iter().enumerate() {
            m.set_f32(a + (16 * i + 4 * j) as u32, *e);
        }
    }
}

/// The routines of `algebra.c` (ported in `si-math` on values) called with addresses, as
/// the original calls them. Each reads all its operands before it stores, as the original
/// does where it works through locals, so an operand may be the destination.
pub mod mx {
    use super::{mat, put_mat, put_vec3, vec3};
    use si_math::algebra as a;
    use si_mem::Mem;

    /// `SI_matrixCopy(src, dst)`.
    pub fn copy(m: &mut Mem, src: u32, dst: u32) {
        let v = a::matrix_copy(&mat(m, src));
        put_mat(m, dst, &v);
    }

    /// `SI_matrixMultiply(a, b, out)`.
    pub fn multiply(m: &mut Mem, x: u32, y: u32, out: u32) {
        let v = a::matrix_multiply(&mat(m, x), &mat(m, y));
        put_mat(m, out, &v);
    }

    /// `SI_matrixMultiplySub3(a, b, out)`.
    pub fn multiply_sub3(m: &mut Mem, x: u32, y: u32, out: u32) {
        let v = a::matrix_multiply_sub3(&mat(m, x), &mat(m, y));
        put_mat(m, out, &v);
    }

    /// `SI_matrixInverse(a, out)`: nothing is stored when the matrix has no inverse. The
    /// answer says whether it had one (the original's callers here do not look).
    pub fn inverse(m: &mut Mem, x: u32, out: u32) -> bool {
        match a::matrix_inverse(&mat(m, x)) {
            Some(v) => {
                put_mat(m, out, &v);
                true
            }
            None => false,
        }
    }

    /// `SI_matrixXyzRotation(angles, out)`.
    pub fn xyz_rotation(m: &mut Mem, angles: u32, out: u32) {
        let v = a::matrix_xyz_rotation(&vec3(m, angles));
        put_mat(m, out, &v);
    }

    /// `SI_matrixExtractScaling(matrix, out)`.
    pub fn extract_scaling(m: &mut Mem, x: u32, out: u32) {
        let v = a::matrix_extract_scaling(&mat(m, x));
        put_vec3(m, out, v);
    }

    /// `SI_matrixExtractXyzRotation(matrix, angles)`: the angles at `angles` are the
    /// previous solution and are replaced. `keep_turns` is the short at `SI_glbManip` + 0x20,
    /// which the original reads itself.
    pub fn extract_xyz_rotation(m: &mut Mem, x: u32, angles: u32) {
        let keep = m[super::g::SI_GLB_MANIP].keep_turns != 0;
        let v = a::matrix_extract_xyz_rotation(&mat(m, x), &vec3(m, angles), keep);
        put_vec3(m, angles, v);
    }

    /// `SI_matrixExtractTransformations(matrix, scaling, angles, translation)`: as
    /// [`extract_xyz_rotation`] for the angles.
    pub fn extract_transformations(m: &mut Mem, x: u32, scl: u32, rot: u32, trs: u32) {
        let keep = m[super::g::SI_GLB_MANIP].keep_turns != 0;
        let (s, r, t) = a::matrix_extract_transformations(&mat(m, x), &vec3(m, rot), keep);
        put_vec3(m, scl, s);
        put_vec3(m, rot, r);
        put_vec3(m, trs, t);
    }

    /// `SI_vectorMultiplyMatrix(v, matrix, out)`.
    pub fn vector_multiply_matrix(m: &mut Mem, v: u32, x: u32, out: u32) {
        let r = a::vector_multiply_matrix(&vec3(m, v), &mat(m, x));
        put_vec3(m, out, r);
    }

    /// `SI_matrixTranspose(a, out)`.
    pub fn transpose(m: &mut Mem, x: u32, out: u32) {
        let v = a::matrix_transpose(&mat(m, x));
        put_mat(m, out, &v);
    }

    /// `SI_matrixScaling(factors, out)`.
    pub fn scaling(m: &mut Mem, s: u32, out: u32) {
        let v = a::matrix_scaling(&vec3(m, s));
        put_mat(m, out, &v);
    }

    /// `SI_matrixScalRotTrans(scaling, angles, translation, out)`.
    pub fn scal_rot_trans(m: &mut Mem, s: u32, r: u32, t: u32, out: u32) {
        let v = a::matrix_scal_rot_trans(&vec3(m, s), &vec3(m, r), &vec3(m, t));
        put_mat(m, out, &v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_types::geom::Vector;
    use si_types::model::Model;

    fn fresh() -> Mem {
        let mut m = Mem::new();
        init(&mut m);
        m
    }

    fn new_node(m: &mut Mem) -> Ptr32<Model> {
        let a = m.place(0x430, 8);
        let n: Ptr32<Model> = Ptr32::new(a);
        m[n].latdep = n;
        m[n].scal = Vector { x: 1.0, y: 1.0, z: 1.0 };
        m[n].gscl = Vector { x: 1.0, y: 1.0, z: 1.0 };
        m[n].update_flags2 = 0x3e;
        n
    }

    #[test]
    fn the_update_on_the_image_equals_the_update_on_the_store() {
        si_rt::skip_without_install!();
        // a father with an uneven scaling and a turned son, by both forms of the routine
        let mut m = fresh();
        let (father, son) = (new_node(&mut m), new_node(&mut m));
        m[father].scal = Vector { x: 2.0, y: 1.0, z: 0.5 };
        m[father].vrot = Vector { x: 0.25, y: -0.5, z: 1.0 };
        m[father].trans = Vector { x: 1.0, y: 2.0, z: 3.0 };
        m[father].son = son;
        m[son].father = father;
        m[son].vrot = Vector { x: 0.0, y: 0.0, z: 0.75 };
        m[son].trans = Vector { x: 4.0, y: 0.0, z: -1.0 };
        let mut models = crate::node::Models::default();
        let (f, s) = (models.allocate(None), models.allocate(None));
        *models.m_mut(f) = m.read(father);
        *models.m_mut(s) = m.read(son);
        models.m_mut(f).latdep = crate::node::Models::ptr(f);
        models.m_mut(f).son = crate::node::Models::ptr(s);
        models.m_mut(s).latdep = crate::node::Models::ptr(s);
        models.m_mut(s).father = crate::node::Models::ptr(f);
        state::update_son_state(&mut m, &mut NoOutside, Ptr32::NULL, father, 1);
        crate::transform::update_son_state(&mut models, None, f, true, &crate::transform::Globals::default(), &mut ());
        assert!(m.fault().is_none());
        for (image, store) in [(father, f), (son, s)] {
            let (a, b) = (m.read(image), *models.m(store));
            assert_eq!(a.gmat, b.gmat);
            assert_eq!(a.def_matrix, b.def_matrix);
            assert_eq!((a.gscl, a.gtrs, a.grot), (b.gscl, b.gtrs, b.grot));
            assert_eq!(a.flags_08 & 0x8000, 0);
        }
        // the son's position went through the father's matrix; its axes are not sheared
        assert_ne!(m[son].gtrs, m[son].trans);
    }

    #[test]
    fn definitions_are_inserted_and_removed_in_the_original_s_blocks() {
        si_rt::skip_without_install!();
        let mut m = fresh();
        let node = new_node(&mut m);
        assert_eq!(defs::add_idefinition(&mut m, node, 5, 4, 0x1000), 1);
        assert_eq!(setup::add_definition(&mut m, node, 4, 0x2000), 1);
        assert_eq!(defs::add_idefinition(&mut m, node, 1, 4, 0x3000), 1);
        let array = m[node].defs.addr();
        assert_eq!([m.u32(array), m.u32(array + 4), m.u32(array + 8)], [0x1000, 0x3000, 0x2000]);
        assert_eq!((m[node].nb_defs, m[node].kind, m[node].definition.addr()), (3, 4, 0x1000));
        // one block of four bytes, grown twice: three blocks in the harness's heap
        assert_eq!(m.heap_blocks().len(), 3);
        assert_eq!(m[node].update_flags & 1, 1);
        // a kind without geometry: nothing is disposed, the pointers move down
        m[node].kind = 0;
        assert_eq!(defs::delete_idefinition(&mut m, &mut NoOutside, node, 0), 1);
        assert_eq!((m[node].nb_defs, m.u32(array), m.u32(array + 4), m.u32(array + 8)), (2, 0x3000, 0x2000, 0));
        // the working pointer still names the removed first definition: only a removed
        // definition with a number above 0 is replaced as the working one
        assert_eq!(m[node].definition.addr(), 0x1000);
        assert_eq!(defs::delete_idefinition(&mut m, &mut NoOutside, node, 7), 0);
        assert!(m.fault().is_none());
    }

    #[test]
    fn user_data_is_copied_except_the_two_callbacks() {
        si_rt::skip_without_install!();
        let mut m = fresh();
        let names: [&[u8]; 3] = [b"mine", b"CUSELEMCB", b"CUSNEXTELEMCBS"];
        let records: Vec<u32> = names.iter().map(|_| m.place(0x14, 8)).collect();
        for (i, name) in names.iter().enumerate() {
            let text = m.place_cstr(name);
            let data = m.place_bytes(&[7, 8, 9]);
            m.set_u32(records[i], text);
            m.set_u16(records[i] + 4, 0x55);
            m.set_u32(records[i] + 8, 3);
            m.set_u32(records[i] + 0xc, data);
            m.set_u32(records[i] + 0x10, records.get(i + 1).copied().unwrap_or(0));
        }
        let out = m.place(4, 8);
        setup::user_data_copy_from_setup(&mut m, records[0], out);
        let first = m.u32(out);
        let second = m.u32(first + 0x10);
        assert_eq!(m.cstr(m.u32(first)), b"mine");
        assert_eq!(m.cstr(m.u32(second)), b"CUSNEXTELEMCBS");
        assert_eq!(m.u32(second + 0x10), 0);
        assert_eq!(m.bytes(m.u32(second + 0xc), 3), &[7, 8, 9]);
        assert!(m.fault().is_none());
    }

    /// Answers 0 to every call and keeps the callees' names.
    struct Names(Vec<&'static str>);
    impl Outside for Names {
        fn call(&mut self, _: &mut Mem, callee: &Callee, _: &[u32]) -> u32 {
            self.0.push(callee.name);
            0
        }
    }

    #[test]
    fn a_node_s_render_data_is_given_up_and_its_sons_only_when_asked() {
        si_rt::skip_without_install!();
        let mut m = fresh();
        let (father, son) = (new_node(&mut m), new_node(&mut m));
        m[father].son = son;
        m[son].father = father;
        for n in [father, son] {
            m[n].ren_count0 = 5;
            let (v, t) = (m.malloc(8), m.malloc(8));
            m[n].ren_vertices = Ptr32::new(v);
            m[n].ren_triangles = Ptr32::new(t);
        }
        // the father's triangle mesh, with its block at +4
        let tmesh = m.malloc(0x30);
        let block = m.malloc(4);
        m.set_u32(tmesh + 4, block);
        m[father].gl_list = Ptr32::new(tmesh);
        let mut o = Names(Vec::new());
        triangle::remove_triangle(&mut m, &mut o, father, 0);
        assert_eq!(o.0, ["SI_glDeleteDispList"]);
        assert_eq!((m[father].ren_count0, m[father].ren_vertices.addr(), m[father].ren_triangles.addr()), (0, 0, 0));
        assert_eq!((m[father].gl_list.addr(), m.freed().len()), (0, 4));
        // the son is left as it was; a 16-bit 0 in a wider word is still 0
        assert_eq!(m[son].ren_count0, 5);
        triangle::remove_triangle(&mut m, &mut o, father, 0x10000u32 as i16);
        assert_eq!(m[son].ren_count0, 5);
        triangle::remove_triangle(&mut m, &mut o, father, -1);
        assert_eq!((m[son].ren_count0, m[son].ren_vertices.addr(), m.freed().len()), (0, 0, 6));
        // a null node: nothing
        triangle::remove_triangle(&mut m, &mut o, Ptr32::NULL, 1);
        assert_eq!(o.0.len(), 1);
        assert!(m.fault().is_none());
    }

    #[test]
    fn the_wave_question_answers_the_list_and_says_whether_a_branch_s_wave_is_in_it() {
        si_rt::skip_without_install!();
        let mut m = fresh();
        let node = new_node(&mut m);
        let set = m.place(2, 2);
        m.set_i16(set, 7);
        assert_eq!(readjust::model_wave(&mut m, node, set), 0);
        assert_eq!(m.i16(set), 0);
        // the node's own wave (link 0), then a branch's (link 1)
        let (a, b) = (m.place(12, 4), m.place(12, 4));
        m.set_u32(a + 8, b);
        m.set_i32(b + 4, 1);
        m[node].waves = Ptr32::new(a);
        m.set_i16(set, 7);
        assert_eq!(readjust::model_wave(&mut m, node, set), a);
        assert_eq!(m.i16(set), 1);
        // the node's own only: the list all the same, the word 0
        m.set_i32(b + 4, 2);
        assert_eq!(readjust::model_wave(&mut m, node, set), a);
        assert_eq!(m.i16(set), 0);
        assert!(m.fault().is_none());
    }

    #[test]
    fn a_face_s_points_are_numbered_through_its_holes() {
        si_rt::skip_without_install!();
        let mut m = fresh();
        // a spline face: an outline of two keys and one hole of two
        let face = m.place(0x88, 8);
        let (outline, hole, keys) = (m.place(0x60, 8), m.place(0x74, 8), m.place(0x60, 8));
        m.set_u16(face + 0x14, 2);
        m.set_u32(face + 0x18, outline);
        m.set_u16(face + 0x78, 1);
        m.set_u32(face + 0x7c, hole);
        m.set_u16(face + 0x84, 1);
        m.set_u16(hole + 0x10, 2);
        m.set_u32(hole + 0x14, keys);
        m.set_u8(keys + 0x30 + 0x2c, 1);
        let tagged = m.place(2, 8);
        assert_eq!(points::get_vertex_ptr(&mut m, face, 2, 1, tagged), outline + 0x30 + 0xc);
        assert_eq!(m.i16(tagged), 0);
        assert_eq!(points::get_vertex_ptr(&mut m, face, 2, 3, tagged), keys + 0x30 + 0xc);
        assert_eq!(m.i16(tagged), 1);
        assert_eq!(points::get_vertex_ptr(&mut m, face, 2, 4, tagged), 0);
        assert!(m.fault().is_none());
    }

    /// A father scaled by 2 and moved to (10, 0, 0), with one son at (1, 2, 3).
    fn pair(m: &mut Mem) -> (Ptr32<Model>, Ptr32<Model>) {
        let (father, son) = (new_node(m), new_node(m));
        m[father].scal = Vector { x: 2.0, y: 2.0, z: 2.0 };
        m[father].trans = Vector { x: 10.0, y: 0.0, z: 0.0 };
        m[father].son = son;
        m[son].father = father;
        m[son].trans = Vector { x: 1.0, y: 2.0, z: 3.0 };
        // (the update writes three columns of `gmat`: the fourth is the allocator's)
        m[father].gmat = si_math::types::IDENTITY;
        m[son].gmat = si_math::types::IDENTITY;
        state::update_son_state(m, &mut NoOutside, Ptr32::NULL, father, 1);
        (father, son)
    }

    #[test]
    fn the_setters_on_the_image_keep_or_take_the_sons_as_the_mode_says() {
        si_rt::skip_without_install!();
        let mut m = fresh();
        let (father, son) = pair(&mut m);
        assert_eq!(m[son].gtrs, Vector { x: 12.0, y: 4.0, z: 6.0 });
        // a global position under a father goes through the father's inverse matrix
        let v = m.place(12, 4);
        m.set_f32(v, 10.0);
        m.set_f32(v + 8, 2.0);
        setters::set_global_translation(&mut m, &mut NoOutside, son, v, 1, 0);
        assert_eq!(m[son].trans, Vector { x: 0.0, y: 0.0, z: 1.0 });
        assert_eq!(m[son].update_flags & 8, 8);
        // the father moves in mode 0: the son keeps its global place
        let before = m[son].gtrs;
        m.set_f32(v, 0.0);
        m.set_f32(v + 4, 5.0);
        setters::set_translation(&mut m, &mut NoOutside, father, v, 0, 0);
        assert_eq!(m[father].gtrs, Vector { x: 0.0, y: 5.0, z: 2.0 });
        assert_eq!(m[son].gtrs, before);
        // in mode 1 it is taken along
        m.set_f32(v, 1.0);
        setters::set_translation(&mut m, &mut NoOutside, father, v, 1, 0);
        assert_eq!(m[son].gtrs.x, before.x + 1.0);
        // a joint's local rotation is not compensated (and a joint's sons follow it)
        m[father].kind = 5;
        let r = m.place(12, 4);
        m.set_f32(r + 8, 0.5);
        setters::set_rotation(&mut m, &mut NoOutside, father, r, 0, 0);
        assert_ne!(m[son].gtrs, before);
        assert!(m.fault().is_none());
    }

    #[test]
    fn the_global_state_takes_the_angles_into_a_shared_block() {
        si_rt::skip_without_install!();
        let mut m = fresh();
        let (_, son) = pair(&mut m);
        let (sr, t) = (m.place(12, 4), m.place(12, 4));
        setters::global_state(&mut m, son, sr, sr, t);
        // the angles were written after the scaling into the same block
        assert_eq!([m.f32(sr), m.f32(sr + 4), m.f32(sr + 8)], [0.0, 0.0, 0.0]);
        assert_eq!([m.f32(t), m.f32(t + 4), m.f32(t + 8)], [12.0, 4.0, 6.0]);
    }

    #[test]
    fn a_kinematic_record_gets_velocities_from_one_step_to_the_next() {
        si_rt::skip_without_install!();
        let mut m = fresh();
        let (_, son) = pair(&mut m);
        let k = m.place(0x60, 4);
        // the last position (11, 4, 6), an identity orientation, a velocity of (1, 0, 0)
        for (i, v) in [11.0f32, 4.0, 6.0].iter().enumerate() {
            m.set_f32(k + 4 * i as u32, *v);
        }
        for i in 0..3u32 {
            m.set_f32(k + 0xc + 16 * i, 1.0);
        }
        m.set_f32(k + 0x30, 1.0);
        m.set_u32(si_mem::field!(son, Model, kine_info).addr(), k);
        kine::kine_state_compute(&mut m, son, 0.5);
        assert_eq!([m.f32(k + 0x30), m.f32(k + 0x34), m.f32(k + 0x38)], [2.0, 0.0, 0.0]);
        assert_eq!([m.f32(k + 0x3c), m.f32(k + 0x40), m.f32(k + 0x44)], [2.0, 0.0, 0.0]);
        assert_eq!([m.f32(k), m.f32(k + 4), m.f32(k + 8)], [12.0, 4.0, 6.0]);
        // no turn: no angular velocity
        assert_eq!([m.f32(k + 0x48), m.f32(k + 0x4c), m.f32(k + 0x50)], [0.0, 0.0, 0.0]);
        assert!(m.fault().is_none());
    }
}
