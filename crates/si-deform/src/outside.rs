// SPDX-License-Identifier: MIT
//! The parts of the program these five source files call (`deformation.c` the fifth) that
//! are not in this crate: drawing, dialogs, the editing layer over the dictionaries, the
//! model tree, the geometry packages, cluster animation.
//!
//! Nothing here reproduces a function of the original. A routine of another source file is
//! named by a [`Routine`] (its name and its address in the Linux binary) and reached
//! through [`Outside::call`] with the original's arguments as 32-bit stack words in C
//! order: the calling convention of `si_mem::Registry` and the shape of `si-view`'s and
//! `si-dict`'s traits, so that it can become `si_mem::Outside` when the lead decides. A
//! `short` argument is passed as its 16 bits, zero-extended (the callee reads no more); a
//! `double` as two words, low first. A result the callee writes through a pointer goes to
//! an address of the image: a local on the image's stack (`Mem::local`).
//!
//! The block below is printed by `re/tools/si-deform/outside_table.py` with every address
//! taken from the binary's symbol table; `re/tools/si-deform/check_outside.py` holds them
//! against it again.

use si_mem::Mem;

/// A routine of the original outside this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Routine {
    pub name: &'static str,
    pub addr: u32,
}

/// Whoever answers for the routines that are not in this crate.
pub trait Outside {
    /// `routine(args…)`: what the original would leave in `eax`.
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32;
    /// `drand48()` of the C library: the next number of its 48-bit generator, in [0, 1).
    /// The generator's state is the library's, not the program's, so it is not in the image.
    fn drand48(&mut self) -> f64;
}

/// The C library's `drand48` as published: X' = (0x5DEECE66D × X + 0xB) mod 2^48, the
/// answer X' / 2^48; the state a program starts with when it never seeds it.
#[derive(Clone, Copy, Debug)]
pub struct Drand48(pub u64);

impl Default for Drand48 {
    fn default() -> Drand48 {
        Drand48(0x1234_abcd_330e)
    }
}

impl Drand48 {
    pub fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(0x5_deec_e66d).wrapping_add(0xb) & ((1 << 48) - 1);
        self.0 as f64 / (1u64 << 48) as f64
    }
}

/// Answers 0 to everything and remembers what was asked.
#[derive(Default)]
pub struct NoOutside {
    pub calls: Vec<(&'static Routine, Vec<u32>)>,
    pub random: Drand48,
}

impl Outside for NoOutside {
    fn call(&mut self, _m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        self.calls.push((routine, args.to_vec()));
        0
    }
    fn drand48(&mut self) -> f64 {
        self.random.next()
    }
}

macro_rules! outside {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine this crate names.
        pub const ALL: &[&Routine] = &[ $( &$konst ),* ];
    };
}

outside! {
    /// `SI_edGetElemFromModel (0x080fcf20)`: a model node's element; stores its index through the second argument.
    SI_ED_GET_ELEM_FROM_MODEL = "SI_edGetElemFromModel", 0x080fcf20;
    /// `SI_edGetElemFromLattice (0x080fd47c)`: the element of a model's lattice set (0 node, 1 branch); stores its index.
    SI_ED_GET_ELEM_FROM_LATTICE = "SI_edGetElemFromLattice", 0x080fd47c;
    /// `SI_edGetElemFromCluster (0x080fd510)`: a cluster's element; stores its index.
    SI_ED_GET_ELEM_FROM_CLUSTER = "SI_edGetElemFromCluster", 0x080fd510;
    /// `SI_edAddLattice (0x0810383c)`: gives a model's lattice set its element.
    SI_ED_ADD_LATTICE = "SI_edAddLattice", 0x0810383c;
    /// `SI_edAddCluster (0x08103510)`: gives a cluster of a model its element.
    SI_ED_ADD_CLUSTER = "SI_edAddCluster", 0x08103510;
    /// `SI_edDisposeCluster (0x0810360c)`: removes a cluster's element.
    SI_ED_DISPOSE_CLUSTER = "SI_edDisposeCluster", 0x0810360c;
    /// `SI_edModelSetFitting (0x0810041c)`: records in the dictionary that a model is fitted on another (1 spline, 0 patch; node or branch).
    SI_ED_MODEL_SET_FITTING = "SI_edModelSetFitting", 0x0810041c;
    /// `SI_edFcvDispose (0x080ee2f0)`: frees the curves of two types an object carries in a chapter.
    SI_ED_FCV_DISPOSE = "SI_edFcvDispose", 0x080ee2f0;
    /// `SI_elemdictGetElem (0x08132b50)`: the element of a chapter by index.
    SI_ELEMDICT_GET_ELEM = "SI_elemdictGetElem", 0x08132b50;
    /// `SI_elemdictFindElemByData (0x08134364)`: the index of the element whose object is the pointer given.
    SI_ELEMDICT_FIND_ELEM_BY_DATA = "SI_elemdictFindElemByData", 0x08134364;
    /// `SI_elemdictFindRelatedElem (0x0813508c)`: the first element related to one by a relation type; stores chapter and index.
    SI_ELEMDICT_FIND_RELATED_ELEM = "SI_elemdictFindRelatedElem", 0x0813508c;
    /// `SI_elemdictFindRelatedElemSerial (0x08135554)`: the same for the relation with a given serial number.
    SI_ELEMDICT_FIND_RELATED_ELEM_SERIAL = "SI_elemdictFindRelatedElemSerial", 0x08135554;
    /// `SI_elemdictGetCurRelation (0x08134db0)`: the relation the last search stopped at.
    SI_ELEMDICT_GET_CUR_RELATION = "SI_elemdictGetCurRelation", 0x08134db0;
    /// `SI_elemdictRemoveSerialRelId (0x08133a6c)`: removes the relation with a serial number from an element.
    SI_ELEMDICT_REMOVE_SERIAL_REL_ID = "SI_elemdictRemoveSerialRelId", 0x08133a6c;
    /// `SI_elemdictRangeRelationsCount (0x08135770)`: how many relations of a range of types an element has.
    SI_ELEMDICT_RANGE_RELATIONS_COUNT = "SI_elemdictRangeRelationsCount", 0x08135770;
    /// `SI_elemdictAddRelation (0x08132f78)`: relates two elements.
    SI_ELEMDICT_ADD_RELATION = "SI_elemdictAddRelation", 0x08132f78;
    /// `SI_relRemoveRelations (0x081396d8)`: removes an element's relations.
    SI_REL_REMOVE_RELATIONS = "SI_relRemoveRelations", 0x081396d8;
    /// `SI_elemdictCopyElemAnim (0x0822bd8c)`: copies one element's animation to another of the same chapter.
    SI_ELEMDICT_COPY_ELEM_ANIM = "SI_elemdictCopyElemAnim", 0x0822bd8c;
    /// `SI_elemdictResetElemAnim (0x08220f08)`: removes an element's animation.
    SI_ELEMDICT_RESET_ELEM_ANIM = "SI_elemdictResetElemAnim", 0x08220f08;
    /// `SI_setFcvTableEntriesFromChap (0x08234e3c)`: prepares the table of curve types of a chapter.
    SI_SET_FCV_TABLE_ENTRIES_FROM_CHAP = "SI_setFcvTableEntriesFromChap", 0x08234e3c;
    /// `SI_attdictSetFcurveFlag (0x0811ff18)`: marks an attribute as one whose curves are copied.
    SI_ATTDICT_SET_FCURVE_FLAG = "SI_attdictSetFcurveFlag", 0x0811ff18;
    /// `SI_dbFormatFullName (0x081223dc)`: builds an element's full name from its parts; answers a new string.
    SI_DB_FORMAT_FULL_NAME = "SI_dbFormatFullName", 0x081223dc;
    /// `SI_elemGetVertex (0x08476b2c)`: a point of an element's geometry by number; stores three floats and a 16-bit flag.
    SI_ELEM_GET_VERTEX = "SI_elemGetVertex", 0x08476b2c;
    /// `SI_modelAddILattice (0x08472a0c)`: puts a lattice into a model's set at an index.
    SI_MODEL_ADD_I_LATTICE = "SI_modelAddILattice", 0x08472a0c;
    /// `SI_modelSplineFitting (0x08472b44)`: a model's active spline fitting; stores 0 (node) or 1 (branch) as 16 bits.
    SI_MODEL_SPLINE_FITTING = "SI_modelSplineFitting", 0x08472b44;
    /// `SI_modelPatchFitting (0x08472c3c)`: a model's active patch fitting; stores 0 or 1 as 16 bits.
    SI_MODEL_PATCH_FITTING = "SI_modelPatchFitting", 0x08472c3c;
    /// `SI_modelPatchFittingReset (0x08472cc0)`: takes a patch fitting off a model.
    SI_MODEL_PATCH_FITTING_RESET = "SI_modelPatchFittingReset", 0x08472cc0;
    /// `SI_modelUpdateDependency (0x0846f200)`: settles which node deforms each geometry of a branch.
    SI_MODEL_UPDATE_DEPENDENCY = "SI_modelUpdateDependency", 0x0846f200;
    /// `SI_modelResetFcv (0x08221224)`: removes a model's curves of one type.
    SI_MODEL_RESET_FCV = "SI_modelResetFcv", 0x08221224;
    /// `SI_modelDeleteClusterAnim (0x081e82c8)`: removes a cluster's animation.
    SI_MODEL_DELETE_CLUSTER_ANIM = "SI_modelDeleteClusterAnim", 0x081e82c8;
    /// `SI_latticeTouch (0x08153140)`: tells the update that a model's lattices changed.
    SI_LATTICE_TOUCH = "SI_latticeTouch", 0x08153140;
    /// `SI_meshComputesPolygonNormal (0x084599f4)`: recomputes polygon normals of a mesh (all for -1).
    SI_MESH_COMPUTES_POLYGON_NORMAL = "SI_meshComputesPolygonNormal", 0x084599f4;
    /// `SI_meshComputesVertexNormal (0x08459e8c)`: recomputes corner normals of a mesh (all for -1).
    SI_MESH_COMPUTES_VERTEX_NORMAL = "SI_meshComputesVertexNormal", 0x08459e8c;
    /// `SI_patchComputeSurface (0x0847c1d0)`: rebuilds a patch's surface from its control points.
    SI_PATCH_COMPUTE_SURFACE = "SI_patchComputeSurface", 0x0847c1d0;
    /// `SI_nurbsCrvMovePoint (0x084e9eec)`: moves one control point of a NURBS curve to x, y, z (three doubles).
    SI_NURBS_CRV_MOVE_POINT = "SI_nurbsCrvMovePoint", 0x084e9eec;
    /// `SI_nurbsSrfMovePoint (0x084e9f2c)`: moves one control point of a NURBS surface to x, y, z, w (four doubles).
    SI_NURBS_SRF_MOVE_POINT = "SI_nurbsSrfMovePoint", 0x084e9f2c;
    /// `SI_nurbsUpdateFrenet (0x085020a8)`: rebuilds the frames along a NURBS curve.
    SI_NURBS_UPDATE_FRENET = "SI_nurbsUpdateFrenet", 0x085020a8;
    /// `si_splineUpdateFrenet (0x0848d298)`: rebuilds the frames along a spline.
    SI_SPLINE_UPDATE_FRENET = "si_splineUpdateFrenet", 0x0848d298;
    /// `SI_faceGetIndexes (0x08436d28)`: the tagged points of a face: count, and their numbers through a pointer to the array pointer.
    SI_FACE_GET_INDEXES = "SI_faceGetIndexes", 0x08436d28;
    /// `SI_tagClear (0x0841f12c)`: clears every tag of the selected objects.
    SI_TAG_CLEAR = "SI_tagClear", 0x0841f12c;
    /// `SI_userDataDispose (0x084619e4)`: frees a chain of user data and clears the pointer to it.
    SI_USER_DATA_DISPOSE = "SI_userDataDispose", 0x084619e4;
    /// `SI_clusterDataDispose (0x081e7f0c)`: frees a cluster's key data and clears the pointer.
    SI_CLUSTER_DATA_DISPOSE = "SI_clusterDataDispose", 0x081e7f0c;
    /// `SI_clusterAnimRemoveVertex (0x081f09b4)`: takes one point out of every key of a cluster.
    SI_CLUSTER_ANIM_REMOVE_VERTEX = "SI_clusterAnimRemoveVertex", 0x081f09b4;
    /// `SI_clusterAnimAddVertex (0x081f0fac)`: adds one point to every key of a cluster.
    SI_CLUSTER_ANIM_ADD_VERTEX = "SI_clusterAnimAddVertex", 0x081f0fac;
    /// `SI_clusterAnimSwapVertices (0x081f0c24)`: exchanges two points in every key of a cluster.
    SI_CLUSTER_ANIM_SWAP_VERTICES = "SI_clusterAnimSwapVertices", 0x081f0c24;
    /// `SI_clusterAnimCvtToFace (0x081f1b00)`: converts a cluster's keys from spline to face numbering.
    SI_CLUSTER_ANIM_CVT_TO_FACE = "SI_clusterAnimCvtToFace", 0x081f1b00;
    /// `SI_clusterAnimApplyTransfo (0x081f1c50)`: multiplies a cluster's keys by a matrix.
    SI_CLUSTER_ANIM_APPLY_TRANSFO = "SI_clusterAnimApplyTransfo", 0x081f1c50;
    /// `SI_clusterAnimDuplicate (0x081f1680)`: copies a cluster's key data for a copy of the cluster.
    SI_CLUSTER_ANIM_DUPLICATE = "SI_clusterAnimDuplicate", 0x081f1680;
    /// `SI_quicksort (0x0851cc7c)`: sorts an array of numbers, a parallel array with it, and can note the exchanges.
    SI_QUICKSORT = "SI_quicksort", 0x0851cc7c;
    /// `SI_swapGet (0x0851ca58)`: the next noted exchange; 0 when there is none left.
    SI_SWAP_GET = "SI_swapGet", 0x0851ca58;
    /// `SI_swapDispose (0x0851caa8)`: frees the noted exchanges.
    SI_SWAP_DISPOSE = "SI_swapDispose", 0x0851caa8;
    /// `SI_modelReadjustDefinition (0x0846e340)`: rebuilds a node's working definition from its stored one and what deforms it.
    SI_MODEL_READJUST_DEFINITION = "SI_modelReadjustDefinition", 0x0846e340;
    /// `SI_modelNextSon (0x0846d70c)`: the node after the second in the subtree of the first, depth first; the first son for null; null after the last.
    SI_MODEL_NEXT_SON = "SI_modelNextSon", 0x0846d70c;
    /// `SI_splineDistanceToValueFrenet (0x0848f6f8)`: the point and the frame at a distance along a spline, from its frames.
    SI_SPLINE_DISTANCE_TO_VALUE_FRENET = "SI_splineDistanceToValueFrenet", 0x0848f6f8;
    /// `SI_nurbsDistanceToValueFrenet (0x08503068)`: the point and the frame at a distance along a NURBS curve, from its frames.
    SI_NURBS_DISTANCE_TO_VALUE_FRENET = "SI_nurbsDistanceToValueFrenet", 0x08503068;
    /// `bgnline (0x08612b58)`: starts a line strip (the GL layer).
    BGNLINE = "bgnline", 0x08612b58;
    /// `endline (0x08612b6c)`: ends a line strip.
    ENDLINE = "endline", 0x08612b6c;
    /// `v3f (0x08612c60)`: one point of a strip, three floats.
    V3F = "v3f", 0x08612c60;
    /// `SI_dialogOpen (0x083859b8)`: opens a dialog by its resource number.
    SI_DIALOG_OPEN = "SI_dialogOpen", 0x083859b8;
    /// `SI_dialogClose (0x08385b28)`: closes a dialog.
    SI_DIALOG_CLOSE = "SI_dialogClose", 0x08385b28;
    /// `SI_dialogDraw (0x08383b94)`: draws a dialog.
    SI_DIALOG_DRAW = "SI_dialogDraw", 0x08383b94;
    /// `SI_dialogGetEvent (0x08385e3c)`: waits for the next item of a dialog the user touched.
    SI_DIALOG_GET_EVENT = "SI_dialogGetEvent", 0x08385e3c;
    /// `SI_setStateGhost (0x0838b868)`: greys a dialog item out or not.
    SI_SET_STATE_GHOST = "SI_setStateGhost", 0x0838b868;
    /// `SI_setStateItem (0x0838b824)`: sets a dialog item's state.
    SI_SET_STATE_ITEM = "SI_setStateItem", 0x0838b824;
    /// `SI_setTexed (0x0838bd10)`: sets a dialog's text field.
    SI_SET_TEXED = "SI_setTexed", 0x0838bd10;
    /// `SI_getTexed (0x0838c2c0)`: reads a dialog's text field into a new string.
    SI_GET_TEXED = "SI_getTexed", 0x0838c2c0;
    /// `SI_setItemTitle (0x0838b664)`: sets a dialog item's title.
    SI_SET_ITEM_TITLE = "SI_setItemTitle", 0x0838b664;
    /// `SI_statusDisplayMessage (0x083cca94)`: shows a message in the status line (text, level).
    SI_STATUS_DISPLAY_MESSAGE = "SI_statusDisplayMessage", 0x083cca94;
    /// `SI_refreshSystem (0x08323d44)`: asks for a redraw.
    SI_REFRESH_SYSTEM = "SI_refreshSystem", 0x08323d44;
}
