// SPDX-License-Identifier: MIT
//! The parts of the program the graph list calls that are not in this crate: drawing, the
//! schematic view, dialogs, the editing layer over the dictionaries, the model tree.
//!
//! Nothing here reproduces a function of the original. A routine of another source file is
//! named by a [`Routine`] (its name and its address in the Linux binary) and reached
//! through [`Outside::call`] with the original's arguments as 32-bit stack words in C
//! order: the calling convention of `si_mem::Registry`, so that the day the routine exists
//! on the image an implementation of the trait can hand the call to it by address. A
//! `short` argument is passed as its 16 bits, zero-extended (the callee reads no more). A
//! result the callee writes through a pointer goes to an address of the image: a local on
//! the image's stack (`Mem::local`).

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
}

/// Answers 0 to everything and remembers what was asked.
#[derive(Default)]
pub struct NoOutside {
    pub calls: Vec<(&'static Routine, Vec<u32>)>,
}

impl Outside for NoOutside {
    fn call(&mut self, _m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        self.calls.push((routine, args.to_vec()));
        0
    }
}

macro_rules! outside {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine this crate reaches through [`Outside`].
        pub const ALL: &[&Routine] = &[ $( &$konst ),* ];
    };
}

outside! {
    // ---- edtools.c: the editing layer between the graph list and the element dictionary
    /// `SI_elemdictFindElemFromGrafNode (0x080fcbec)`: the element of a graph node; writes
    /// its chapter and its index through the second and third argument.
    SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE = "SI_elemdictFindElemFromGrafNode", 0x080fcbec;
    /// `SI_elemdictFindTrueElemFromGrafNode (0x080fcd34)`: the same for the node a user
    /// means (the node in front of a camera or light stands for its object).
    SI_ELEMDICT_FIND_TRUE_ELEM_FROM_GRAF_NODE = "SI_elemdictFindTrueElemFromGrafNode", 0x080fcd34;
    /// `SI_edGetElemFromModel (0x080fcf20)`: a model node's element; writes its index.
    SI_ED_GET_ELEM_FROM_MODEL = "SI_edGetElemFromModel", 0x080fcf20;
    /// `SI_edGetElemFromLight (0x080fd0bc)`: a light's element; writes its index.
    SI_ED_GET_ELEM_FROM_LIGHT = "SI_edGetElemFromLight", 0x080fd0bc;
    /// `SI_elemdictUpdateModelHierarchy (0x080fd698)`: rebuilds the hierarchy relations of
    /// the MODELS chapter.
    SI_ELEMDICT_UPDATE_MODEL_HIERARCHY = "SI_elemdictUpdateModelHierarchy", 0x080fd698;
    /// `SI_edGrafNodeSetDeleteFlag (0x080fdcf4)`: marks a node's element deleted or not.
    SI_ED_GRAF_NODE_SET_DELETE_FLAG = "SI_edGrafNodeSetDeleteFlag", 0x080fdcf4;
    /// `SI_edLinkModel (0x080fe41c)`: the dictionary's side of making one node the son of
    /// another.
    SI_ED_LINK_MODEL = "SI_edLinkModel", 0x080fe41c;
    /// `SI_edCutModel (0x080fe5ec)`: the dictionary's side of cutting a node loose.
    SI_ED_CUT_MODEL = "SI_edCutModel", 0x080fe5ec;
    /// `SI_edSetLinetype (0x080ff174)`: sets an element's display type (hidden, ghosted,
    /// visible) and with it the graph node's.
    SI_ED_SET_LINETYPE = "SI_edSetLinetype", 0x080ff174;
    /// `SI_edRelaxRelFromDelModel (0x080ff5f0)`: removes the relations of one kind of a
    /// model that is being deleted.
    SI_ED_RELAX_REL_FROM_DEL_MODEL = "SI_edRelaxRelFromDelModel", 0x080ff5f0;
    /// `SI_edAddModel (0x08101ad4)`: gives a model node its element.
    SI_ED_ADD_MODEL = "SI_edAddModel", 0x08101ad4;
    /// `SI_edAddCamera (0x08101b08)`: gives a camera its elements.
    SI_ED_ADD_CAMERA = "SI_edAddCamera", 0x08101b08;
    /// `SI_edAddLight (0x08101d70)`: gives a light its elements.
    SI_ED_ADD_LIGHT = "SI_edAddLight", 0x08101d70;
    /// `SI_edDisposeLight (0x08101f18)`.
    SI_ED_DISPOSE_LIGHT = "SI_edDisposeLight", 0x08101f18;
    /// `SI_edAddMaterial (0x0810203c)`: gives a material its element; 0 when refused.
    SI_ED_ADD_MATERIAL = "SI_edAddMaterial", 0x0810203c;
    /// `SI_edDisposeMaterial (0x08102114)`.
    SI_ED_DISPOSE_MATERIAL = "SI_edDisposeMaterial", 0x08102114;
    /// `SI_edAddTexture2D (0x08102314)`: gives a 2D texture its element; 0 when refused.
    SI_ED_ADD_TEXTURE_2D = "SI_edAddTexture2D", 0x08102314;
    /// `SI_edDisposeTexture2D (0x0810253c)`.
    SI_ED_DISPOSE_TEXTURE_2D = "SI_edDisposeTexture2D", 0x0810253c;
    /// `SI_edAddTexture3D (0x081027e0)`: gives a 3D texture its element; 0 when refused.
    SI_ED_ADD_TEXTURE_3D = "SI_edAddTexture3D", 0x081027e0;
    /// `SI_edDisposeTexture3D (0x081028d0)`.
    SI_ED_DISPOSE_TEXTURE_3D = "SI_edDisposeTexture3D", 0x081028d0;
    /// `SI_edAddWave (0x081033b4)`: gives a wave its element.
    SI_ED_ADD_WAVE = "SI_edAddWave", 0x081033b4;
    /// `SI_edSetEditable (0x08107764)`: only called for the reduced product.
    SI_ED_SET_EDITABLE = "SI_edSetEditable", 0x08107764;
    /// `SI_edDisposeAnimFile (0x080efc64)`, `edanim.c`.
    SI_ED_DISPOSE_ANIM_FILE = "SI_edDisposeAnimFile", 0x080efc64;
    // ---- elemdict.c (`si-dict` has these on the image)
    /// `SI_elemdictGetElem (0x08132b50)`: the element at a chapter and index.
    SI_ELEMDICT_GET_ELEM = "SI_elemdictGetElem", 0x08132b50;
    /// `SI_elemdictFindRelatedElem (0x0813508c)`: the first element related to one by a
    /// relation; writes the chapter and index found.
    SI_ELEMDICT_FIND_RELATED_ELEM = "SI_elemdictFindRelatedElem", 0x0813508c;
    /// `SI_elemdictFindNextRelatedElem (0x08135154)`: the next one.
    SI_ELEMDICT_FIND_NEXT_RELATED_ELEM = "SI_elemdictFindNextRelatedElem", 0x08135154;
    /// `SI_elemdictSetDeleteElem (0x08135ad4)`.
    SI_ELEMDICT_SET_DELETE_ELEM = "SI_elemdictSetDeleteElem", 0x08135ad4;
    /// `SI_elemlistDispose (0x08135fd4)`.
    SI_ELEMLIST_DISPOSE = "SI_elemlistDispose", 0x08135fd4;
    /// `SI_elemdictCreateDefaultDsc (0x081369cc)`.
    SI_ELEMDICT_CREATE_DEFAULT_DSC = "SI_elemdictCreateDefaultDsc", 0x081369cc;
    /// `SI_edGetElemFromScene (0x080fd620)`, `edtools.c`: (&index); the scene's element.
    SI_ED_GET_ELEM_FROM_SCENE = "SI_edGetElemFromScene", 0x080fd620;
    // ---- history (tab1.c)
    /// `SI_historyOpenCluster (0x080c7270)`.
    SI_HISTORY_OPEN_CLUSTER = "SI_historyOpenCluster", 0x080c7270;
    /// `SI_historyCloseCluster (0x080c7298)`.
    SI_HISTORY_CLOSE_CLUSTER = "SI_historyCloseCluster", 0x080c7298;
    /// `SI_grafnodeSaveHistoryDelete (0x080c973c)`: keeps a deleted node for undo.
    SI_GRAFNODE_SAVE_HISTORY_DELETE = "SI_grafnodeSaveHistoryDelete", 0x080c973c;
    /// `SI_modelSaveHistoryGeometry (0x080c8898)`.
    SI_MODEL_SAVE_HISTORY_GEOMETRY = "SI_modelSaveHistoryGeometry", 0x080c8898;
    /// `SI_modelSaveHistoryParenting (0x080ca4d4)`.
    SI_MODEL_SAVE_HISTORY_PARENTING = "SI_modelSaveHistoryParenting", 0x080ca4d4;
    // ---- animation, expressions, constraints, channels, custom effects
    /// `SI_updateGnFunc (0x08176618)`, `func.c`: chooses a node's drawing function.
    SI_UPDATE_GN_FUNC = "SI_updateGnFunc", 0x08176618;
    /// `SI_patchUpdateSurface (0x0847cee4)`, `patch.c`: rebuilds a patch's surface (`si-spline` has it).
    SI_PATCH_UPDATE_SURFACE = "SI_patchUpdateSurface", 0x0847cee4;
    /// `SI_adjustGnFunc (0x08176700)`, `func.c`: the same for a new node.
    SI_ADJUST_GN_FUNC = "SI_adjustGnFunc", 0x08176700;
    /// `SI_modelMasterFromInstance (0x081a7490)`, `instance.c` (`si-rensetup` has it).
    SI_MODEL_MASTER_FROM_INSTANCE = "SI_modelMasterFromInstance", 0x081a7490;
    /// `SI_skeChainRoot (0x081d20c0)`, `skeleton.c`: the root of the chain a joint or an
    /// effector belongs to.
    SI_SKE_CHAIN_ROOT = "SI_skeChainRoot", 0x081d20c0;
    /// `SI_constraintRelaxFromData (0x081f55f4)`, `constraint.c`.
    SI_CONSTRAINT_RELAX_FROM_DATA = "SI_constraintRelaxFromData", 0x081f55f4;
    /// `SI_dopeFlagsSet (0x081fa5d0)`, `dopeShMng.c`: tells the dope sheet what changed.
    SI_DOPE_FLAGS_SET = "SI_dopeFlagsSet", 0x081fa5d0;
    /// `SI_fcurveRemove (0x081ff0a0)`, `fcurve.c`: disposes of a curve through its pointer.
    SI_FCURVE_REMOVE = "SI_fcurveRemove", 0x081ff0a0;
    /// `SI_hieranimDispose (0x08138b5c)`, `hierani.c`.
    SI_HIERANIM_DISPOSE = "SI_hieranimDispose", 0x08138b5c;
    /// `SI_pathDelete (0x0821f284)`, `motionPath.c`.
    SI_PATH_DELETE = "SI_pathDelete", 0x0821f284;
    /// `SI_edRemoveElemAnim (0x08221190)`, `motionReset.c`.
    SI_ED_REMOVE_ELEM_ANIM = "SI_edRemoveElemAnim", 0x08221190;
    /// `SI_modelResetGeomRelatedAnim (0x082216b4)`, `motionReset.c`.
    SI_MODEL_RESET_GEOM_RELATED_ANIM = "SI_modelResetGeomRelatedAnim", 0x082216b4;
    /// `SI_elemdictActiveFcvOper (0x0822c8a4)`, `motionUtils.c`: applies a routine to an
    /// element's curves.
    SI_ELEMDICT_ACTIVE_FCV_OPER = "SI_elemdictActiveFcvOper", 0x0822c8a4;
    /// `SI_modelAFunction (0x0822cc48)`, `motionUtils.c`: applies a routine to the curves of
    /// a model node.
    SI_MODEL_A_FUNCTION = "SI_modelAFunction", 0x0822cc48;
    /// `SI_fcvScnglBDelete (0x0823c8dc)`, `scene.c`: the routine the two above are
    /// handed (never called from here; its address is an argument).
    SI_FCV_SCNGL_B_DELETE = "SI_fcvScnglBDelete", 0x0823c8dc;
    /// `SI_expressDataDispose (0x0824e5b0)`, `exp_parsing.c`.
    SI_EXPRESS_DATA_DISPOSE = "SI_expressDataDispose", 0x0824e5b0;
    /// `SI_edRemoveElemExpressions (0x082559d0)`, `express_io.c`.
    SI_ED_REMOVE_ELEM_EXPRESSIONS = "SI_edRemoveElemExpressions", 0x082559d0;
    /// `SI_edRemoveExpressionsFromArg (0x08255ac0)`, `express_io.c`.
    SI_ED_REMOVE_EXPRESSIONS_FROM_ARG = "SI_edRemoveExpressionsFromArg", 0x08255ac0;
    /// `SI_lightDispose (0x082818f0)`, `light_fct.c`.
    SI_LIGHT_DISPOSE = "SI_lightDispose", 0x082818f0;
    /// `SI_globalMapListDelete (0x082b2164)`, `t2d_dial.c`.
    SI_GLOBAL_MAP_LIST_DELETE = "SI_globalMapListDelete", 0x082b2164;
    /// `SI_cusDeleteElement (0x08549998)`, `custom_util.c`.
    SI_CUS_DELETE_ELEMENT = "SI_cusDeleteElement", 0x08549998;
    /// `SI_cusDeleteModel (0x08549570)`, `custom_util.c`.
    SI_CUS_DELETE_MODEL = "SI_cusDeleteModel", 0x08549570;
    /// `SI_cusDeleteIcon (0x0854a094)`, `custom_util.c`.
    SI_CUS_DELETE_ICON = "SI_cusDeleteIcon", 0x0854a094;
    /// `SI_cusElemCallbackCallAll (0x0854b614)`, `custom_util.c`.
    SI_CUS_ELEM_CALLBACK_CALL_ALL = "SI_cusElemCallbackCallAll", 0x0854b614;
    /// `SI_cusElemCallbackUpdateEffectsForDeletion (0x0854bc44)`, `custom_util.c`.
    SI_CUS_ELEM_CALLBACK_UPDATE_EFFECTS_FOR_DELETION = "SI_cusElemCallbackUpdateEffectsForDeletion", 0x0854bc44;
    /// `SI_udfDispose (0x0854c3c0)`, `udf_fct.c`.
    SI_UDF_DISPOSE = "SI_udfDispose", 0x0854c3c0;
    /// `SI_udfDisposeAll (0x0854c588)`, `udf_fct.c`.
    SI_UDF_DISPOSE_ALL = "SI_udfDisposeAll", 0x0854c588;
    /// `SI_chnGrafNodeDelete (0x0856136c)`, `chn_channel.c`.
    SI_CHN_GRAF_NODE_DELETE = "SI_chnGrafNodeDelete", 0x0856136c;
    /// `SI_dialogChnReset (0x085710a8)`, `chn_dial.c`.
    SI_DIALOG_CHN_RESET = "SI_dialogChnReset", 0x085710a8;
    /// `SI_sprdUpdateSelect (0x085aaa7c)`, `sprd_process.c`: tells the spreadsheet the
    /// selection changed.
    SI_SPRD_UPDATE_SELECT = "SI_sprdUpdateSelect", 0x085aaa7c;
    /// `SI_sprdAllContentDispose (0x085aace8)`, `sprd_process.c`.
    SI_SPRD_ALL_CONTENT_DISPOSE = "SI_sprdAllContentDispose", 0x085aace8;
    /// `SI_particleSetupReset (0x084c0438)`, `particle_dial.c`.
    SI_PARTICLE_SETUP_RESET = "SI_particleSetupReset", 0x084c0438;
    // ---- routines of this package that stay calls by address (see `crate::wired`)
    /// `SI_graflistDeleteNode (0x0832d75c)`, `graflist.c`: the Delete command for one node.
    SI_GRAFLIST_DELETE_NODE = "SI_graflistDeleteNode", 0x0832d75c;
    /// `SI_schemComputeHierarchiesBBox (0x0833bd74)`, `graphsort.c`: lays the schematic
    /// view's hierarchies out again.
    SI_SCHEM_COMPUTE_HIERARCHIES_BBOX = "SI_schemComputeHierarchiesBBox", 0x0833bd74;
    /// `SI_schematicHierOperation (0x0833c5dc)`, `graphsort.c`.
    SI_SCHEMATIC_HIER_OPERATION = "SI_schematicHierOperation", 0x0833c5dc;
    // ---- graphsort.c's own that `SI_schematicHierOperation` calls (the first is ported,
    // `crate::graphsort::schem_elem_oper`, and closes the seam of `crate::wired`)
    /// `si_schemElemOper (0x0833c15c)`: (dictionary, element, operation, a 16-bit 1, two
    /// floats 0).
    SI_SCHEM_ELEM_OPER = "si_schemElemOper", 0x0833c15c;
    /// `schemUpdateHierarchy (0x0833c2ec)`: (dictionary, chapter, root element, grafport,
    /// operation).
    SCHEM_UPDATE_HIERARCHY = "schemUpdateHierarchy", 0x0833c2ec;
    /// `schemCollapseNslElem (0x0833c834)`: (grafport, dictionary, element, operation).
    SCHEM_COLLAPSE_NSL_ELEM = "schemCollapseNslElem", 0x0833c834;
    /// `SI_nselectionSelectContent (0x083465a4)`, `nselections.c`: selects, unselects or
    /// toggles what a named selection holds.
    SI_NSELECTION_SELECT_CONTENT = "SI_nselectionSelectContent", 0x083465a4;
    // ---- algebra.c (`si-math` has both as functions of values)
    /// `SI_matrixMultiplySub3 (0x0851650c)`: the product of the upper 3×3 parts of two
    /// matrices (result, left, right).
    SI_MATRIX_MULTIPLY_SUB3 = "SI_matrixMultiplySub3", 0x0851650c;
    /// `SI_matrixExtractXyzRotation (0x085172b0)`: the three angles of a rotation matrix
    /// (matrix, angles).
    SI_MATRIX_EXTRACT_XYZ_ROTATION = "SI_matrixExtractXyzRotation", 0x085172b0;
    /// `SI_vectorNormalize (0x08516158)`: a vector made of length 1 in place.
    SI_VECTOR_NORMALIZE = "SI_vectorNormalize", 0x08516158;
    /// `SI_vectorMultiplyMatrix (0x08516d74)`: (result, matrix, vector, a 16-bit switch).
    SI_VECTOR_MULTIPLY_MATRIX = "SI_vectorMultiplyMatrix", 0x08516d74;
    /// `SI_matrixXRotation (0x08517080)`: the rotation matrix of an angle (a float, the
    /// matrix).
    SI_MATRIX_X_ROTATION = "SI_matrixXRotation", 0x08517080;
    /// `SI_matrixYRotation (0x085170e0)`.
    SI_MATRIX_Y_ROTATION = "SI_matrixYRotation", 0x085170e0;
    /// `SI_matrixZRotation (0x08517140)`.
    SI_MATRIX_Z_ROTATION = "SI_matrixZRotation", 0x08517140;
    // ---- what the named selections call
    /// `SI_edGetElemFromNSelection (0x080fd5b8)`, `edtools.c`: a named selection's
    /// element; writes its index.
    SI_ED_GET_ELEM_FROM_N_SELECTION = "SI_edGetElemFromNSelection", 0x080fd5b8;
    /// `SI_edGetCamFromCamInt (0x080fd040)`, `edtools.c`: the index of a camera's element
    /// from that of its interest.
    SI_ED_GET_CAM_FROM_CAM_INT = "SI_edGetCamFromCamInt", 0x080fd040;
    /// `SI_edGetLigFromLigInt (0x080fd150)`, `edtools.c`: the same for a light.
    SI_ED_GET_LIG_FROM_LIG_INT = "SI_edGetLigFromLigInt", 0x080fd150;
    /// `SI_relRemoveRelations (0x081396d8)`, `relations.c`.
    SI_REL_REMOVE_RELATIONS = "SI_relRemoveRelations", 0x081396d8;
    /// `SI_elemdictAddElem (0x08132a5c)`, `elemdict.c` (`si-dict` has it on the image).
    SI_ELEMDICT_ADD_ELEM = "SI_elemdictAddElem", 0x08132a5c;
    /// `SI_elemdictAddUnCompleteRelation (0x08132fb4)`, `elemdict.c`.
    SI_ELEMDICT_ADD_UN_COMPLETE_RELATION = "SI_elemdictAddUnCompleteRelation", 0x08132fb4;
    /// `SI_elemdictRemoveBackwardRelId (0x08133b18)`, `elemdict.c`.
    SI_ELEMDICT_REMOVE_BACKWARD_REL_ID = "SI_elemdictRemoveBackwardRelId", 0x08133b18;
    /// `SI_elemdictGetCurRelation (0x08134db0)`, `elemdict.c`: the relation the last
    /// look-up of a related element stands at.
    SI_ELEMDICT_GET_CUR_RELATION = "SI_elemdictGetCurRelation", 0x08134db0;
    /// `SI_elemdictFindElemByData (0x08134364)`, `elemdict.c`: the index of the element
    /// of a chapter whose object this is.
    SI_ELEMDICT_FIND_ELEM_BY_DATA = "SI_elemdictFindElemByData", 0x08134364;
    /// `SI_dbListNodeAllocate (0x0812250c)`, `dbexternal.c`: a new name record.
    SI_DB_LIST_NODE_ALLOCATE = "SI_dbListNodeAllocate", 0x0812250c;
    /// `SI_dbFormatFullName (0x081223dc)`, `dbexternal.c`.
    SI_DB_FORMAT_FULL_NAME = "SI_dbFormatFullName", 0x081223dc;
    /// `SI_klusterGetActive (0x0843b4b8)`, `klusters.c`.
    SI_KLUSTER_GET_ACTIVE = "SI_klusterGetActive", 0x0843b4b8;
    /// `SI_klusterActivate (0x0843b4f0)`, `klusters.c`.
    SI_KLUSTER_ACTIVATE = "SI_klusterActivate", 0x0843b4f0;
    /// `SI_statusDisplayMessage (0x083cca94)`, `status.c`: a text for the status line.
    SI_STATUS_DISPLAY_MESSAGE = "SI_statusDisplayMessage", 0x083cca94;
    /// `SI_fcvScnglClear (0x0823c948)`, `scene.c`: empties the list of selected curves.
    SI_FCV_SCNGL_CLEAR = "SI_fcvScnglClear", 0x0823c948;
    /// `SI_fcvScnglAdd (0x0823c7a8)`, `scene.c`: puts a curve into that list.
    SI_FCV_SCNGL_ADD = "SI_fcvScnglAdd", 0x0823c7a8;
    /// `SI_fcvGetRelInfo (0x0810f698)`, `dbvsfcv.c`: three words about a curve type,
    /// through its last three arguments.
    SI_FCV_GET_REL_INFO = "SI_fcvGetRelInfo", 0x0810f698;
    /// `SI_fcvGetRelGrpFromType (0x0810f624)`, `dbvsfcv.c`.
    SI_FCV_GET_REL_GRP_FROM_TYPE = "SI_fcvGetRelGrpFromType", 0x0810f624;
    /// `SI_edFindFcv (0x080edb20)`, `edanim.c`: the first curve of an element in a group.
    SI_ED_FIND_FCV = "SI_edFindFcv", 0x080edb20;
    /// `SI_edFindNextFcv (0x080edb74)`, `edanim.c`: the next.
    SI_ED_FIND_NEXT_FCV = "SI_edFindNextFcv", 0x080edb74;
    // ---- what the builder of a loaded scene's list calls (db2scn.c and its callees)
    /// `SI_cusReadingAnimModel (0x0853b274)`, `custom_util.c`: a 16-bit switch, handed to
    /// `SI_graflistAddData` as "comes selected".
    SI_CUS_READING_ANIM_MODEL = "SI_cusReadingAnimModel", 0x0853b274;
    /// `SI_modelUpdateDependency (0x0846f200)`, `model.c`.
    SI_MODEL_UPDATE_DEPENDENCY = "SI_modelUpdateDependency", 0x0846f200;
    /// `SI_edCreateObjectFromInterest (0x08109db8)`, `db2scn.c`: makes the interest of a
    /// camera or light an object of its own (dictionary, relation dictionary, chapter,
    /// index, element, object).
    SI_ED_CREATE_OBJECT_FROM_INTEREST = "SI_edCreateObjectFromInterest", 0x08109db8;
    /// `SI_elemdictUpdateHierInfo (0x08134cc8)`, `elemdict.c`.
    SI_ELEMDICT_UPDATE_HIER_INFO = "SI_elemdictUpdateHierInfo", 0x08134cc8;
    /// `SI_elemdictPropagatePrfxAndShare (0x08136350)`, `elemdict.c`.
    SI_ELEMDICT_PROPAGATE_PRFX_AND_SHARE = "SI_elemdictPropagatePrfxAndShare", 0x08136350;
    /// `SI_dbConvertModelInternals2DB (0x081094b0)`, `scn2db.c`.
    SI_DB_CONVERT_MODEL_INTERNALS_2_DB = "SI_dbConvertModelInternals2DB", 0x081094b0;
    // ---- views, schematic, user interface
    /// `SI_fastSetDirty (0x08329898)`, `fastplay.c`: the cached playback pictures are stale.
    SI_FAST_SET_DIRTY = "SI_fastSetDirty", 0x08329898;
    /// `SI_fastPlayDeleteAll (0x0832a108)`, `fastplay.c`.
    SI_FAST_PLAY_DELETE_ALL = "SI_fastPlayDeleteAll", 0x0832a108;
    /// `SI_schematicAdjustNewNode (0x0834fe20)`, `schematic.c`: places a new node in the
    /// schematic view.
    SI_SCHEMATIC_ADJUST_NEW_NODE = "SI_schematicAdjustNewNode", 0x0834fe20;
    /// `SI_schematicCheckDeleteExpand (0x08350414)`, `schematic.c`.
    SI_SCHEMATIC_CHECK_DELETE_EXPAND = "SI_schematicCheckDeleteExpand", 0x08350414;
    /// `si_schematicRightReadjustX (0x08350c50)`, `schematic.c`: moves what lies to the
    /// right of an element in the schematic view.
    SI_SCHEMATIC_RIGHT_READJUST_X = "si_schematicRightReadjustX", 0x08350c50;
    /// `SI_schemSetDefaultColor (0x083516d0)`, `schematic.c`.
    SI_SCHEM_SET_DEFAULT_COLOR = "SI_schemSetDefaultColor", 0x083516d0;
    /// `si_softSetGrafport (0x08369490)`, `view.c`.
    SI_SOFT_SET_GRAFPORT = "si_softSetGrafport", 0x08369490;
    /// `SI_unsetGrafport (0x0836a2d8)`, `view.c`.
    SI_UNSET_GRAFPORT = "SI_unsetGrafport", 0x0836a2d8;
    /// `SI_buttonKeepHilite (0x0837fa1c)`, `control.c`.
    SI_BUTTON_KEEP_HILITE = "SI_buttonKeepHilite", 0x0837fa1c;
    /// `SI_buttonOff (0x0837fa74)`, `control.c`.
    SI_BUTTON_OFF = "SI_buttonOff", 0x0837fa74;
    /// `SI_fcvScrollBuild (0x083978d8)`, `fcvscroll.c`.
    SI_FCV_SCROLL_BUILD = "SI_fcvScrollBuild", 0x083978d8;
    /// `SI_modeBoxRefresh (0x083a6380)`, `modeBox.c`.
    SI_MODE_BOX_REFRESH = "SI_modeBoxRefresh", 0x083a6380;
    /// `SI_windowGetNextActive (0x083d1ca4)`, `uiLite.c`: the next view window; writes it
    /// through its argument.
    SI_WINDOW_GET_NEXT_ACTIVE = "SI_windowGetNextActive", 0x083d1ca4;
    /// `SI_windowGetNextVisible (0x083d6500)`: the same over the visible windows.
    SI_WINDOW_GET_NEXT_VISIBLE = "SI_windowGetNextVisible", 0x083d6500;
    /// `SI_windowSet (0x083d6588)`.
    SI_WINDOW_SET = "SI_windowSet", 0x083d6588;
    /// `SI_windowUnset (0x083d65f4)`.
    SI_WINDOW_UNSET = "SI_windowUnset", 0x083d65f4;
    /// `SI_setUItoPrefFlags (0x08424f98)`, `infompr.c`.
    SI_SET_UI_TO_PREF_FLAGS = "SI_setUItoPrefFlags", 0x08424f98;
    // ---- the model tree and its geometry
    /// `SI_patchRemoveFitting (0x0843a580)`, `fitting.c`.
    SI_PATCH_REMOVE_FITTING = "SI_patchRemoveFitting", 0x0843a580;
    /// `SI_modelDispose (0x08461920)`, `model.c`: disposes of a node through its pointer.
    SI_MODEL_DISPOSE = "SI_modelDispose", 0x08461920;
    /// `SI_userDataDispose (0x084619e4)`, `model.c`.
    SI_USER_DATA_DISPOSE = "SI_userDataDispose", 0x084619e4;
    /// `SI_modelDeleteDefinition (0x084678a8)`, `model.c`.
    SI_MODEL_DELETE_DEFINITION = "SI_modelDeleteDefinition", 0x084678a8;
    /// `SI_modelComputeBounding (0x08467e70)`, `model.c`.
    SI_MODEL_COMPUTE_BOUNDING = "SI_modelComputeBounding", 0x08467e70;
    /// `SI_modelTouchGeometry (0x0846d48c)`, `model.c`: a node's display data is stale.
    SI_MODEL_TOUCH_GEOMETRY = "SI_modelTouchGeometry", 0x0846d48c;
    /// `SI_modelNbSubModels (0x0846d5ec)`, `model.c`: how many nodes lie below a node.
    SI_MODEL_NB_SUB_MODELS = "SI_modelNbSubModels", 0x0846d5ec;
    /// `SI_modelNextSon (0x0846d70c)`, `model.c`.
    SI_MODEL_NEXT_SON = "SI_modelNextSon", 0x0846d70c;
    /// `SI_modelCutFather (0x0846d790)`, `model.c`: takes a node out of its father's sons.
    SI_MODEL_CUT_FATHER = "SI_modelCutFather", 0x0846d790;
    /// `SI_modelChangeName (0x0846db14)`, `model.c`.
    SI_MODEL_CHANGE_NAME = "SI_modelChangeName", 0x0846db14;
    /// `SI_modelRemoveAllFittings (0x08476d0c)`, `model_b.c`.
    SI_MODEL_REMOVE_ALL_FITTINGS = "SI_modelRemoveAllFittings", 0x08476d0c;
    /// `SI_modelNextNode (0x08478340)`, `model_b.c`: the next node of a hierarchy, depth
    /// first; null after the last.
    SI_MODEL_NEXT_NODE = "SI_modelNextNode", 0x08478340;
    /// `SI_modelRoot (0x0847838c)`, `model_b.c`.
    SI_MODEL_ROOT = "SI_modelRoot", 0x0847838c;
    /// `SI_modelDynaInfoDispose (0x084a1248)`, `settings.c`.
    SI_MODEL_DYNA_INFO_DISPOSE = "SI_modelDynaInfoDispose", 0x084a1248;
    /// `SI_modelUnlinkAllWaves (0x084c7018)`, `wave_sys.c`.
    SI_MODEL_UNLINK_ALL_WAVES = "SI_modelUnlinkAllWaves", 0x084c7018;
    /// `SI_waveDispose (0x084c6f68)`, `wave_sys.c`: disposes of a wave through its pointer.
    SI_WAVE_DISPOSE = "SI_waveDispose", 0x084c6f68;
    // ---- hidden.c: what the hidden-line view calls
    /// `fprintf`, through the binary's stub (0x080c3b88).
    FPRINTF = "fprintf", 0x080c3b88;
    /// `SI_chunkMalloc (0x0857cb64)`: a block from a chunk buffer (`chunkHid` here).
    SI_CHUNK_MALLOC = "SI_chunkMalloc", 0x0857cb64;
    /// `SI_chunkFreeAll (0x0857cb00)`: releases every block of a chunk buffer.
    SI_CHUNK_FREE_ALL = "SI_chunkFreeAll", 0x0857cb00;
    /// `SI_chunkDestroy (0x0857ca5c)`: destroys a chunk buffer given its address.
    SI_CHUNK_DESTROY = "SI_chunkDestroy", 0x0857ca5c;
}

/// A call with nothing to tell apart from the others: `routine(args…)`.
#[inline]
pub fn call(o: &mut dyn Outside, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
    o.call(m, routine, args)
}
