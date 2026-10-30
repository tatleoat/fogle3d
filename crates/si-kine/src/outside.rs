// SPDX-License-Identifier: MIT
//! The parts of the program the image-form modules of this crate call that are not on the
//! image, or not in this package.
//!
//! Nothing here reproduces a function of the original. A callee outside the package is
//! named by a [`Routine`] (its name and its address in the Linux binary) and reached
//! through [`Outside::call`] with the original's arguments as 32-bit stack words in C order
//! (a `float` its bits, a `double` two words, a structure by value its words): the calling
//! convention of `si_mem::Registry` and the shape `si-dict` uses, so that the day the
//! callee exists on the image an implementation of the trait can hand the call to it by
//! address. Each routine has a typed wrapper in the module that calls it.
//!
//! Mathematics that takes and returns values (`si-math`, `si-x87`, and this crate's own
//! `SI_matrixZyzeulerRotation`) is called directly: finding 0024 exempts leaf libraries.

use si_mem::Mem;
use si_x87::F80;

/// A routine of the original outside this package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Routine {
    pub name: &'static str,
    pub addr: u32,
}

/// What a callee leaves: `eax`, and the x87 register for a `float` or `double` result.
#[derive(Clone, Copy, Debug)]
pub struct Ret {
    pub eax: u32,
    pub st0: F80,
}

impl Ret {
    pub fn int(eax: u32) -> Ret {
        Ret { eax, st0: F80::from_i32(0) }
    }

    pub fn float(st0: F80) -> Ret {
        Ret { eax: 0, st0 }
    }
}

impl Default for Ret {
    fn default() -> Ret {
        Ret::int(0)
    }
}

/// Whoever answers for the routines that are not on the image.
pub trait Outside {
    /// `routine(args…)`: what the original would leave in `eax` or in the x87 register.
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> Ret;

    /// What a routine prints on the standard output (`printf`), already formatted.
    fn stdout(&mut self, _text: &[u8]) {}
}

/// Answers zero to everything and remembers what was asked.
#[derive(Default)]
pub struct NoOutside {
    pub calls: Vec<(&'static Routine, Vec<u32>)>,
}

impl Outside for NoOutside {
    fn call(&mut self, _m: &mut Mem, routine: &'static Routine, args: &[u32]) -> Ret {
        self.calls.push((routine, args.to_vec()));
        Ret::default()
    }
}

macro_rules! routines {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine this crate reaches through [`Outside`].
        pub const ALL: &[&Routine] = &[ $( &$konst ),* ];
    };
}

routines! {
    /// `SI_statusDisplayMessage (0x083cca94)`, `status.c`: a text on the status line, with
    /// a severity.
    SI_STATUS_DISPLAY_MESSAGE = "SI_statusDisplayMessage", 0x083cca94;
    /// `SI_splineNbSegment (0x0848c8dc)`, `spline.c`: the number of segments of a spline.
    SI_SPLINE_NB_SEGMENT = "SI_splineNbSegment", 0x0848c8dc;
    /// `SI_splineLength (0x0848df48)`, `spline.c`: the curve's length (a `float`).
    SI_SPLINE_LENGTH = "SI_splineLength", 0x0848df48;
    /// `SI_splineUpdateLength (0x0848cda4)`, `spline.c`: rebuilds the table of cumulated
    /// lengths (+0x6c) with so many samples a segment.
    SI_SPLINE_UPDATE_LENGTH = "SI_splineUpdateLength", 0x0848cda4;
    /// `SI_splineDistanceToFactor (0x0848ca64)`, `spline.c`: the parameter at a distance
    /// along the curve (a `float`, left in the register).
    SI_SPLINE_DISTANCE_TO_FACTOR = "SI_splineDistanceToFactor", 0x0848ca64;
    /// `SI_splineValue (0x0848b1e4)`, `spline.c`: the point at a parameter.
    SI_SPLINE_VALUE = "SI_splineValue", 0x0848b1e4;
    /// `SI_nurbsCurveLength (0x08502070)`, `nrb_geometry.c`: a NURBS curve's length.
    SI_NURBS_CURVE_LENGTH = "SI_nurbsCurveLength", 0x08502070;
    /// `SI_nurbsDistanceToFactor (0x08502f4c)`, `nrb_geometry.c`: the parameter at a
    /// distance (a `double` in, a `double` out).
    SI_NURBS_DISTANCE_TO_FACTOR = "SI_nurbsDistanceToFactor", 0x08502f4c;
    /// `SI_nurbsCrvValue (0x084d3550)`, `eval.c`: the point at a parameter, a structure
    /// of doubles returned through a hidden first pointer.
    SI_NURBS_CRV_VALUE = "SI_nurbsCrvValue", 0x084d3550;
    /// `SI_splineAllocate (0x0848a990)`, `spline.c`: a new, initialised spline.
    SI_SPLINE_ALLOCATE = "SI_splineAllocate", 0x0848a990;
    /// `SI_splineKeyAllocate (0x0848f8dc)`, `spline.c`: room for so many keys.
    SI_SPLINE_KEY_ALLOCATE = "SI_splineKeyAllocate", 0x0848f8dc;
    /// `SI_splineUpdateMatrix (0x0848abd0)`, `spline.c`: the spline's own matrix (+0x24).
    SI_SPLINE_UPDATE_MATRIX = "SI_splineUpdateMatrix", 0x0848abd0;
    /// `SI_splineTouch (0x0848c924)`, `spline.c`: marks the spline as changed.
    SI_SPLINE_TOUCH = "SI_splineTouch", 0x0848c924;
    /// `SI_splineTransfer (0x0848aa9c)`, `spline.c`: moves a spline's content into another
    /// record.
    SI_SPLINE_TRANSFER = "SI_splineTransfer", 0x0848aa9c;
    /// `SI_splineDispose (0x0848ab48)`, `spline.c`.
    SI_SPLINE_DISPOSE = "SI_splineDispose", 0x0848ab48;
    /// `SI_faceAllocate (0x084347d0)`, `face.c`: a new face.
    SI_FACE_ALLOCATE = "SI_faceAllocate", 0x084347d0;
    /// `SI_modelAllocate (0x08477cf4)`, `model_b.c`: a new node with a name.
    SI_MODEL_ALLOCATE = "SI_modelAllocate", 0x08477cf4;
    /// `SI_modelAddDefinition (0x08467038)`, `model.c`: gives a node a definition of a kind.
    SI_MODEL_ADD_DEFINITION = "SI_modelAddDefinition", 0x08467038;
    /// `SI_backSelected (0x083260a0)`, `draw.c`.
    SI_BACK_SELECTED = "SI_backSelected", 0x083260a0;
    /// `SI_graflistAddData (0x0832d240)`, `graflist.c`: adds an object of a kind to a list.
    SI_GRAFLIST_ADD_DATA = "SI_graflistAddData", 0x0832d240;
    /// `SI_refreshSelectedBothBuffers (0x08325d70)`, `draw.c`.
    SI_REFRESH_SELECTED_BOTH_BUFFERS = "SI_refreshSelectedBothBuffers", 0x08325d70;
    /// `SI_refreshCounter (0x080d1d20)`, `counter.c`.
    SI_REFRESH_COUNTER = "SI_refreshCounter", 0x080d1d20;
    /// `SI_modelTraversePnts (0x08474a94)`, `model.c`: calls a function for every point of
    /// a node's geometry with the point, its index and a pointer of the caller's.
    SI_MODEL_TRAVERSE_PNTS = "SI_modelTraversePnts", 0x08474a94;
    /// `SI_modelGetPnt (0x08475da8)`, `model.c`: the point of a node's geometry with an
    /// index, in global coordinates.
    SI_MODEL_GET_PNT = "SI_modelGetPnt", 0x08475da8;
    /// `SI_modelComputeBounding (0x08467e70)`, `model.c`: recomputes a node's bounding box.
    SI_MODEL_COMPUTE_BOUNDING = "SI_modelComputeBounding", 0x08467e70;
    /// `SI_fcurveIntegerValue (0x08200404)`, `fcurve.c`: a curve's value at a time, as an
    /// integer.
    SI_FCURVE_INTEGER_VALUE = "SI_fcurveIntegerValue", 0x08200404;
    /// `SI_modelSetGlobalTranslation (0x0846412c)`, `model.c`: puts a node at a global
    /// position.
    SI_MODEL_SET_GLOBAL_TRANSLATION = "SI_modelSetGlobalTranslation", 0x0846412c;
    /// `SI_modelSetTranslation (0x084640cc)`, `model.c`: sets a node's local translation.
    SI_MODEL_SET_TRANSLATION = "SI_modelSetTranslation", 0x084640cc;
    /// `SI_modelUpdateSonState (0x084662f8)`, `model.c`: the node update.
    SI_MODEL_UPDATE_SON_STATE = "SI_modelUpdateSonState", 0x084662f8;
    /// `SI_findGlData (0x0832ee68)`, `graflist.c`: the graf node of an object of a kind in
    /// a list.
    SI_FIND_GL_DATA = "SI_findGlData", 0x0832ee68;
    /// `SI_edGetElemFromModel (0x080fcf20)`, `edtools.c`: a node's element in the scene's
    /// dictionary, its index stored through the second argument.
    SI_ED_GET_ELEM_FROM_MODEL = "SI_edGetElemFromModel", 0x080fcf20;
    /// `SI_edGetElemFromCluster (0x080fd510)`, `edtools.c`: likewise for a cluster.
    SI_ED_GET_ELEM_FROM_CLUSTER = "SI_edGetElemFromCluster", 0x080fd510;
    /// `SI_edGetElemFromCamera (0x080fcfb0)`, `edtools.c`: likewise for a camera.
    SI_ED_GET_ELEM_FROM_CAMERA = "SI_edGetElemFromCamera", 0x080fcfb0;
    /// `SI_edGetCamFromCamInt (0x080fd040)`, `edtools.c`: from a camera's element index to
    /// its interest's.
    SI_ED_GET_CAM_FROM_CAM_INT = "SI_edGetCamFromCamInt", 0x080fd040;
    /// `SI_edElementSetConstraint (0x080fffac)`, `edtools.c`: adds a constraint relation
    /// between two items, with its data.
    SI_ED_ELEMENT_SET_CONSTRAINT = "SI_edElementSetConstraint", 0x080fffac;
    /// `SI_edElementRelaxConstraint (0x08100238)`, `edtools.c`: removes an item's
    /// constraint of one relation.
    SI_ED_ELEMENT_RELAX_CONSTRAINT = "SI_edElementRelaxConstraint", 0x08100238;
    /// `SI_edGetSelectedElements (0x0810479c)`, `edtools.c`: marks the elements of the
    /// selected objects.
    SI_ED_GET_SELECTED_ELEMENTS = "SI_edGetSelectedElements", 0x0810479c;
    /// `SI_klusterGetActive (0x0843b4b8)`, `klusters.c`: a node's active cluster.
    SI_KLUSTER_GET_ACTIVE = "SI_klusterGetActive", 0x0843b4b8;
    /// `SI_klusterCnsSetData (0x0843b8f4)`, `klusters.c`: fills a cluster constraint's data.
    SI_KLUSTER_CNS_SET_DATA = "SI_klusterCnsSetData", 0x0843b8f4;
    /// `SI_elemdictFindRelatedElem (0x0813508c)`, `elemdict.c` (`si-dict` has it on the image).
    SI_ELEMDICT_FIND_RELATED_ELEM = "SI_elemdictFindRelatedElem", 0x0813508c;
    /// `SI_elemdictFindRangeRelatedElem (0x081353c4)`, `elemdict.c`.
    SI_ELEMDICT_FIND_RANGE_RELATED_ELEM = "SI_elemdictFindRangeRelatedElem", 0x081353c4;
    /// `SI_elemdictFindRangeNextRelatedElem (0x08135484)`, `elemdict.c`.
    SI_ELEMDICT_FIND_RANGE_NEXT_RELATED_ELEM = "SI_elemdictFindRangeNextRelatedElem", 0x08135484;
    /// `SI_elemdictGetCurRelation (0x08134db0)`, `elemdict.c`.
    SI_ELEMDICT_GET_CUR_RELATION = "SI_elemdictGetCurRelation", 0x08134db0;
    /// `SI_elemdictGetRelationFromIds (0x08134e9c)`, `elemdict.c` (`si-dict` has it on the
    /// image): the forward entry of a relation that names an element by chapter and index.
    SI_ELEMDICT_GET_RELATION_FROM_IDS = "SI_elemdictGetRelationFromIds", 0x08134e9c;
    /// `SI_elemdictAddCompleteRelation (0x08132dc4)`, `elemdict.c`.
    SI_ELEMDICT_ADD_COMPLETE_RELATION = "SI_elemdictAddCompleteRelation", 0x08132dc4;
    /// `SI_elemdictAddSerialRelation (0x08133184)`, `elemdict.c`.
    SI_ELEMDICT_ADD_SERIAL_RELATION = "SI_elemdictAddSerialRelation", 0x08133184;
    /// `SI_elemdictRemoveRelationId (0x08133998)`, `elemdict.c`.
    SI_ELEMDICT_REMOVE_RELATION_ID = "SI_elemdictRemoveRelationId", 0x08133998;
    /// `SI_elemdictRemoveSerialRelId (0x08133a6c)`, `elemdict.c`.
    SI_ELEMDICT_REMOVE_SERIAL_REL_ID = "SI_elemdictRemoveSerialRelId", 0x08133a6c;
    /// `SI_elemdictGetRelSerialNumber (0x08139b68)`, `relations.c`.
    SI_ELEMDICT_GET_REL_SERIAL_NUMBER = "SI_elemdictGetRelSerialNumber", 0x08139b68;
    /// `SI_elemdictSetSelection (0x08136cb0)`, `elemdict.c`.
    SI_ELEMDICT_SET_SELECTION = "SI_elemdictSetSelection", 0x08136cb0;
    /// `SI_elemdictGetElem (0x08132b50)`, `elemdict.c`.
    SI_ELEMDICT_GET_ELEM = "SI_elemdictGetElem", 0x08132b50;
    /// `SI_elemdictIsElemUsed (0x08136d10)`, `elemdict.c`.
    SI_ELEMDICT_IS_ELEM_USED = "SI_elemdictIsElemUsed", 0x08136d10;
    /// `SI_elemdictCheckCycle (0x08140d80)`, `toposort.c`.
    SI_ELEMDICT_CHECK_CYCLE = "SI_elemdictCheckCycle", 0x08140d80;
    /// `SI_relTopoSortInit (0x0813ffbc)`, `toposort.c`.
    SI_REL_TOPO_SORT_INIT = "SI_relTopoSortInit", 0x0813ffbc;
    /// `SI_elemdictTopoSortSelectedElements (0x08140b28)`, `toposort.c`.
    SI_ELEMDICT_TOPO_SORT_SELECTED_ELEMENTS = "SI_elemdictTopoSortSelectedElements", 0x08140b28;
    /// `SI_modelDeleteDefinition (0x084678a8)`, `model.c`.
    SI_MODEL_DELETE_DEFINITION = "SI_modelDeleteDefinition", 0x084678a8;
    /// `SI_modelUpdateDependency (0x0846f200)`, `model.c`.
    SI_MODEL_UPDATE_DEPENDENCY = "SI_modelUpdateDependency", 0x0846f200;
    /// `SI_dynaConstraintRelaxFromData (0x08496060)`, `dyn_constr.c`.
    SI_DYNA_CONSTRAINT_RELAX_FROM_DATA = "SI_dynaConstraintRelaxFromData", 0x08496060;
    /// `SI_graflistCountSelected (0x0832db90)`, `graflist.c`.
    SI_GRAFLIST_COUNT_SELECTED = "SI_graflistCountSelected", 0x0832db90;
    /// `SI_dialogRequest2 (0x08141d5c)`, `request.c`: a question with two buttons.
    SI_DIALOG_REQUEST2 = "SI_dialogRequest2", 0x08141d5c;
    /// `getbutton (0x0860e684)`, `event.c`: whether a key or mouse button is down.
    GETBUTTON = "getbutton", 0x0860e684;
    /// `SI_updlistDump (0x081415e0)`, `updlist.c`: prints an update list.
    SI_UPDLIST_DUMP = "SI_updlistDump", 0x081415e0;
    /// `si_updListSetCurrent (0x0822b074)`, `motionUpdate.c`.
    SI_UPD_LIST_SET_CURRENT = "si_updListSetCurrent", 0x0822b074;
    /// `si_updNodeUpdate (0x0822afb8)`, `motionUpdate.c`: runs one step of an update list.
    SI_UPD_NODE_UPDATE = "si_updNodeUpdate", 0x0822afb8;
    /// `si_animModelConstraintUpdate (0x08223ab8)`, `motionUpdate.c`: the two limit
    /// constraints.
    SI_ANIM_MODEL_CONSTRAINT_UPDATE = "si_animModelConstraintUpdate", 0x08223ab8;
    /// `si_animModelPlaneConstrUpdate (0x08224434)`, `motionUpdate.c`: the plane constraints.
    SI_ANIM_MODEL_PLANE_CONSTR_UPDATE = "si_animModelPlaneConstrUpdate", 0x08224434;
    /// `si_animModelSymConstrUpdate (0x082241dc)`, `motionUpdate.c`: the symmetry constraint.
    SI_ANIM_MODEL_SYM_CONSTR_UPDATE = "si_animModelSymConstrUpdate", 0x082241dc;
    /// `SI_envSkelResetFlag (0x084bdbe0)`, `envtools.c`.
    SI_ENV_SKEL_RESET_FLAG = "SI_envSkelResetFlag", 0x084bdbe0;
    /// `bgnclosedline (0x08612b7c)`, `graphics.c`: starts a closed line.
    BGNCLOSEDLINE = "bgnclosedline", 0x08612b7c;
    /// `endclosedline (0x08612b90)`, `graphics.c`.
    ENDCLOSEDLINE = "endclosedline", 0x08612b90;
    /// `v3f (0x08612c60)`, `graphics.c`: a vertex, three floats by address.
    V3F = "v3f", 0x08612c60;
    /// `getcolor (0x08610f28)`, `graphics.c`: the current colour index.
    GETCOLOR = "getcolor", 0x08610f28;
    /// `color (0x08610e1c)`, `graphics.c`: sets the colour index.
    COLOR = "color", 0x08610e1c;
    /// `SI_drawCenter (0x083275ac)`, `draw.c`: the centre mark.
    SI_DRAW_CENTER = "SI_drawCenter", 0x083275ac;
    /// `SI_splineCircle (0x08487d54)`, `primitive.c`: a circle as a spline of a type, with
    /// a radius and a number of points.
    SI_SPLINE_CIRCLE = "SI_splineCircle", 0x08487d54;
    /// `SI_nurbsCircle (0x08487ee0)`, `primitive.c`: a circle as a NURBS curve.
    SI_NURBS_CIRCLE = "SI_nurbsCircle", 0x08487ee0;
    /// `SI_meshExtrudeSplineOnSpline (0x081630f0)`, `extrusion.c`.
    SI_MESH_EXTRUDE_SPLINE_ON_SPLINE = "SI_meshExtrudeSplineOnSpline", 0x081630f0;
    /// `SI_splineExtrudeToPatch (0x0815bafc)`, `extrusion.c`.
    SI_SPLINE_EXTRUDE_TO_PATCH = "SI_splineExtrudeToPatch", 0x0815bafc;
    /// `SI_nurbsExtrudeToNurbs (0x0815b6d8)`, `extrusion.c`.
    SI_NURBS_EXTRUDE_TO_NURBS = "SI_nurbsExtrudeToNurbs", 0x0815b6d8;
    /// `SI_patchForceCapping (0x0848120c)`, `patch.c`.
    SI_PATCH_FORCE_CAPPING = "SI_patchForceCapping", 0x0848120c;
    /// `SI_nurbsSrfForceCapping (0x084f0134)`, `nrb_point.c`.
    SI_NURBS_SRF_FORCE_CAPPING = "SI_nurbsSrfForceCapping", 0x084f0134;
    /// `SI_nurbsCrvDispose (0x084db870)`, `nrb_b.c`: by the address of the pointer.
    SI_NURBS_CRV_DISPOSE = "SI_nurbsCrvDispose", 0x084db870;
    /// `SI_modelFreezeScaling (0x0846d190)`, `model.c`.
    SI_MODEL_FREEZE_SCALING = "SI_modelFreezeScaling", 0x0846d190;
    /// `SI_modelSymmetry (0x08179cd8)`: the mirror of a node about a plane.
    SI_MODEL_SYMMETRY = "SI_modelSymmetry", 0x08179cd8;
    /// `SI_modelCopyNode (0x0846234c)`, `model.c`.
    SI_MODEL_COPY_NODE = "SI_modelCopyNode", 0x0846234c;
    /// `SI_modelCopy (0x084622cc)`, `model.c`.
    SI_MODEL_COPY = "SI_modelCopy", 0x084622cc;
    /// `SI_modelCopyFromSetup (0x084633e8)`, `model.c`.
    SI_MODEL_COPY_FROM_SETUP = "SI_modelCopyFromSetup", 0x084633e8;
    /// `si_skeMoveJointBranch (0x081d2c50)`, `sketransform.c`: a joint moved with its bone
    /// (the node, the vector's address, two 16-bit flags).
    SI_SKE_MOVE_JOINT_BRANCH = "si_skeMoveJointBranch", 0x081d2c50;
    /// `SI_modelFreezeRotation (0x0846d244)`, `model.c`.
    SI_MODEL_FREEZE_ROTATION = "SI_modelFreezeRotation", 0x0846d244;
    /// `SI_modelSubstituteIDefinition (0x08467158)`, `model.c`.
    SI_MODEL_SUBSTITUTE_I_DEFINITION = "SI_modelSubstituteIDefinition", 0x08467158;
    /// `SI_modelTouchGeometry (0x0846d48c)`, `model.c`.
    SI_MODEL_TOUCH_GEOMETRY = "SI_modelTouchGeometry", 0x0846d48c;
    /// `SI_setStateItem (0x0838b824)`, `dialog.c`: sets a toggle of a dialog.
    SI_SET_STATE_ITEM = "SI_setStateItem", 0x0838b824;
    /// `SI_setStateGhost (0x0838b868)`, `dialog.c`: ghosts or frees an item.
    SI_SET_STATE_GHOST = "SI_setStateGhost", 0x0838b868;
    /// `SI_getStateItem (0x0838bcec)`, `dialog.c`: a toggle's state, 16 bits stored
    /// through the third argument.
    SI_GET_STATE_ITEM = "SI_getStateItem", 0x0838bcec;
    /// `SI_setTexed (0x0838bd10)`, `dialog.c`: the text of a field.
    SI_SET_TEXED = "SI_setTexed", 0x0838bd10;
    /// `SI_getTexedFloat (0x0838c138)`, `dialog.c`: a field's text as a number.
    SI_GET_TEXED_FLOAT = "SI_getTexedFloat", 0x0838c138;
    // ---- third pass: the solver and the limits on the image
    /// `SI_edIsItemConstrained (0x08106bbc)`, `edtools.c`: whether an object's element has
    /// a relation (and, with the flag, one whose activation is on at a time); a `short`.
    SI_ED_IS_ITEM_CONSTRAINED = "SI_edIsItemConstrained", 0x08106bbc;
    /// `SI_elemdictFindSerialRelation (0x0813335c)`, `elemdict.c`: the first or the next
    /// relation entry of a kind of an element.
    SI_ELEMDICT_FIND_SERIAL_RELATION = "SI_elemdictFindSerialRelation", 0x0813335c;
    /// `SI_getExprFcurveUsed (0x08249a00)`, `exp_fct.c`: the curves an element's
    /// expressions drive (count and lists through pointers); answers the list.
    SI_GET_EXPR_FCURVE_USED = "SI_getExprFcurveUsed", 0x08249a00;
    /// `SI_activationFcvCheckTime (0x0822aa64)`, `motionUpdate.c`: whether an activation
    /// curve is on at a time; a `short`.
    SI_ACTIVATION_FCV_CHECK_TIME = "SI_activationFcvCheckTime", 0x0822aa64;
    /// `SI_classicSolvePartialInvKin (0x081cab70)`, `classic.c`: the solver under classical
    /// scaling.
    SI_CLASSIC_SOLVE_PARTIAL_INV_KIN = "SI_classicSolvePartialInvKin", 0x081cab70;
    /// `SI_modelSetRotation (0x08463f54)`, `model.c`: a node's angles, by address.
    SI_MODEL_SET_ROTATION = "SI_modelSetRotation", 0x08463f54;
    /// `SI_constrLocation (0x08222538)`, `motionUpdate.c`: where a constrained object is.
    SI_CONSTR_LOCATION = "SI_constrLocation", 0x08222538;
    /// `SI_QuickStretchCalculation (0x0859e280)`, `qstretch.c`: the quick stretch's
    /// deformation of a node's geometry (now `image::qscalc`; the name stays for hosts that
    /// answer it themselves).
    SI_QUICK_STRETCH_CALCULATION = "SI_QuickStretchCalculation", 0x0859e280;
    /// `SI_fcurveValue (0x082002dc)`, `fcurve.c`: a curve's value at a time (in the register).
    SI_FCURVE_VALUE = "SI_fcurveValue", 0x082002dc;
    /// `SI_getFcvTriplet (0x0822efcc)`, `motionUtils.c`: an element's three curves of an
    /// attribute, by kind.
    SI_GET_FCV_TRIPLET = "SI_getFcvTriplet", 0x0822efcc;
    /// `SI_reldictGetRelChapGrp (0x0813a464)`, `reldict.c`: a relation's chapter, its group
    /// through the third argument; negative when unknown.
    SI_RELDICT_GET_REL_CHAP_GRP = "SI_reldictGetRelChapGrp", 0x0813a464;
    /// The geometry copies (`SI_patchCopy (0x0847a2b8)`, `SI_faceCopy (0x08434990)`,
    /// `SI_meshCopy (0x084404e0)`, `SI_splineCopy (0x0848aa74)`, `SI_nurbsCrvCopy
    /// (0x084dbbf4)`, `SI_nurbsSrfCopy (0x084dc1a8)`): a new copy of a definition.
    SI_PATCH_COPY = "SI_patchCopy", 0x0847a2b8;
    SI_FACE_COPY = "SI_faceCopy", 0x08434990;
    SI_MESH_COPY = "SI_meshCopy", 0x084404e0;
    SI_SPLINE_COPY = "SI_splineCopy", 0x0848aa74;
    SI_NURBS_CRV_COPY = "SI_nurbsCrvCopy", 0x084dbbf4;
    SI_NURBS_SRF_COPY = "SI_nurbsSrfCopy", 0x084dc1a8;
    /// `SI_patchUpdateSurface (0x0847cee4)`, `patch.c`: recomputes a patch's surface.
    SI_PATCH_UPDATE_SURFACE = "SI_patchUpdateSurface", 0x0847cee4;
    /// `SI_modelComputeBoundingEL (0x0846ad84)`, `model.c`: the box of a node's control
    /// points through a matrix, and the node's two radii (+0x28, +0x2c).
    SI_MODEL_COMPUTE_BOUNDING_EL = "SI_modelComputeBoundingEL", 0x0846ad84;
    /// `SI_elemGetNbVertices (0x084769b4)`, `model.c`: an element's point count.
    SI_ELEM_GET_NB_VERTICES = "SI_elemGetNbVertices", 0x084769b4;
    /// `SI_elemGetVertex (0x08476b2c)`, `model.c`: a point of an element's geometry and
    /// its tag.
    SI_ELEM_GET_VERTEX = "SI_elemGetVertex", 0x08476b2c;
    /// `SI_elemSetVertex (0x08476d70)`, `model.c`: puts a point of an element's geometry.
    SI_ELEM_SET_VERTEX = "SI_elemSetVertex", 0x08476d70;
    /// `fprintf`, through the binary's stub (0x080c3b88).
    FPRINTF = "fprintf", 0x080c3b88;
}

fn bits64(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

/// `SI_statusDisplayMessage (0x083cca94)`: `text` is the address of a C string.
pub fn si_status_display_message(o: &mut dyn Outside, m: &mut Mem, text: u32, severity: i32) {
    o.call(m, &SI_STATUS_DISPLAY_MESSAGE, &[text, severity as u32]);
}

/// `SI_splineNbSegment (0x0848c8dc)`.
pub fn si_spline_nb_segment<T>(o: &mut dyn Outside, m: &mut Mem, spline: si_mem::Ptr32<T>) -> i32 {
    o.call(m, &SI_SPLINE_NB_SEGMENT, &[spline.addr()]).eax as i32
}

/// `SI_splineLength (0x0848df48)`: the register value the caller rounds.
pub fn si_spline_length<T>(o: &mut dyn Outside, m: &mut Mem, spline: si_mem::Ptr32<T>) -> F80 {
    o.call(m, &SI_SPLINE_LENGTH, &[spline.addr()]).st0
}

/// `SI_splineUpdateLength (0x0848cda4)`.
pub fn si_spline_update_length<T>(o: &mut dyn Outside, m: &mut Mem, spline: si_mem::Ptr32<T>, samples: i16, touch: i16) {
    o.call(m, &SI_SPLINE_UPDATE_LENGTH, &[spline.addr(), samples as i32 as u32, touch as i32 as u32]);
}

/// `SI_splineDistanceToFactor (0x0848ca64)`.
pub fn si_spline_distance_to_factor<T>(o: &mut dyn Outside, m: &mut Mem, spline: si_mem::Ptr32<T>, distance: f32) -> F80 {
    o.call(m, &SI_SPLINE_DISTANCE_TO_FACTOR, &[spline.addr(), distance.to_bits()]).st0
}

/// `SI_splineValue (0x0848b1e4)`: `out` is the address of an `SI_Vector`.
pub fn si_spline_value<T>(o: &mut dyn Outside, m: &mut Mem, spline: si_mem::Ptr32<T>, u: f32, out: u32) {
    o.call(m, &SI_SPLINE_VALUE, &[spline.addr(), u.to_bits(), out]);
}

/// `SI_nurbsCurveLength (0x08502070)`.
pub fn si_nurbs_curve_length<T>(o: &mut dyn Outside, m: &mut Mem, curve: si_mem::Ptr32<T>) -> F80 {
    o.call(m, &SI_NURBS_CURVE_LENGTH, &[curve.addr()]).st0
}

/// `SI_nurbsDistanceToFactor (0x08502f4c)`.
pub fn si_nurbs_distance_to_factor<T>(o: &mut dyn Outside, m: &mut Mem, curve: si_mem::Ptr32<T>, distance: f64) -> F80 {
    let d = bits64(distance);
    o.call(m, &SI_NURBS_DISTANCE_TO_FACTOR, &[curve.addr(), d[0], d[1]]).st0
}

/// `SI_nurbsCrvValue (0x084d3550)`: `result` is the address the structure is returned to.
pub fn si_nurbs_crv_value<T>(o: &mut dyn Outside, m: &mut Mem, result: u32, curve: si_mem::Ptr32<T>, u: f64, derivative: i32) {
    let b = bits64(u);
    o.call(m, &SI_NURBS_CRV_VALUE, &[result, curve.addr(), b[0], b[1], derivative as u32]);
}

/// `SI_splineAllocate (0x0848a990)`.
pub fn si_spline_allocate<T>(o: &mut dyn Outside, m: &mut Mem) -> si_mem::Ptr32<T> {
    si_mem::Ptr32::new(o.call(m, &SI_SPLINE_ALLOCATE, &[]).eax)
}

/// `SI_splineKeyAllocate (0x0848f8dc)`.
pub fn si_spline_key_allocate<T>(o: &mut dyn Outside, m: &mut Mem, count: i32) -> si_mem::Ptr32<T> {
    si_mem::Ptr32::new(o.call(m, &SI_SPLINE_KEY_ALLOCATE, &[count as u32]).eax)
}

/// `SI_splineUpdateMatrix (0x0848abd0)`.
pub fn si_spline_update_matrix<T>(o: &mut dyn Outside, m: &mut Mem, spline: si_mem::Ptr32<T>) {
    o.call(m, &SI_SPLINE_UPDATE_MATRIX, &[spline.addr()]);
}

/// `SI_splineTouch (0x0848c924)`.
pub fn si_spline_touch<T>(o: &mut dyn Outside, m: &mut Mem, spline: si_mem::Ptr32<T>) {
    o.call(m, &SI_SPLINE_TOUCH, &[spline.addr()]);
}

/// `SI_splineTransfer (0x0848aa9c)`: `to` is the address of the record that takes it.
pub fn si_spline_transfer<T>(o: &mut dyn Outside, m: &mut Mem, from: si_mem::Ptr32<T>, to: u32) {
    o.call(m, &SI_SPLINE_TRANSFER, &[from.addr(), to]);
}

/// `SI_splineDispose (0x0848ab48)`.
pub fn si_spline_dispose<T>(o: &mut dyn Outside, m: &mut Mem, spline: si_mem::Ptr32<T>) {
    o.call(m, &SI_SPLINE_DISPOSE, &[spline.addr()]);
}

/// `SI_faceAllocate (0x084347d0)`: the address of the new face.
pub fn si_face_allocate(o: &mut dyn Outside, m: &mut Mem) -> u32 {
    o.call(m, &SI_FACE_ALLOCATE, &[]).eax
}

/// `SI_modelAllocate (0x08477cf4)`: `name` is the address of a C string.
pub fn si_model_allocate<T>(o: &mut dyn Outside, m: &mut Mem, name: u32) -> si_mem::Ptr32<T> {
    si_mem::Ptr32::new(o.call(m, &SI_MODEL_ALLOCATE, &[name]).eax)
}

/// `SI_modelAddDefinition (0x08467038)`.
pub fn si_model_add_definition<T>(o: &mut dyn Outside, m: &mut Mem, node: si_mem::Ptr32<T>, kind: i32, definition: u32) -> i32 {
    o.call(m, &SI_MODEL_ADD_DEFINITION, &[node.addr(), kind as u32, definition]).eax as i32
}

/// `SI_backSelected (0x083260a0)`.
pub fn si_back_selected(o: &mut dyn Outside, m: &mut Mem, a: i16, b: i16) {
    o.call(m, &SI_BACK_SELECTED, &[a as i32 as u32, b as i32 as u32]);
}

/// `SI_graflistAddData (0x0832d240)`.
pub fn si_graflist_add_data(o: &mut dyn Outside, m: &mut Mem, list: u32, data: u32, kind: i32, flag: i16) -> u32 {
    o.call(m, &SI_GRAFLIST_ADD_DATA, &[list, data, kind as u32, flag as i32 as u32]).eax
}

/// `SI_refreshSelectedBothBuffers (0x08325d70)`.
pub fn si_refresh_selected_both_buffers(o: &mut dyn Outside, m: &mut Mem, flag: i16) {
    o.call(m, &SI_REFRESH_SELECTED_BOTH_BUFFERS, &[flag as i32 as u32]);
}

/// `SI_refreshCounter (0x080d1d20)`.
pub fn si_refresh_counter(o: &mut dyn Outside, m: &mut Mem) {
    o.call(m, &SI_REFRESH_COUNTER, &[]);
}

/// `SI_modelTraversePnts (0x08474a94)`: `callback` is a code address, `data` the address
/// handed to it.
pub fn si_model_traverse_pnts<T>(o: &mut dyn Outside, m: &mut Mem, node: si_mem::Ptr32<T>, callback: si_mem::FnPtr, data: u32, flag: i16) {
    o.call(m, &SI_MODEL_TRAVERSE_PNTS, &[node.addr(), callback.addr(), data, flag as i32 as u32]);
}

/// `SI_modelGetPnt (0x08475da8)`: `out` is the address of an `SI_Vector`.
pub fn si_model_get_pnt<T>(o: &mut dyn Outside, m: &mut Mem, node: si_mem::Ptr32<T>, index: i32, out: u32) {
    o.call(m, &SI_MODEL_GET_PNT, &[node.addr(), index as u32, out]);
}

/// `SI_modelComputeBounding (0x08467e70)`.
pub fn si_model_compute_bounding<T>(o: &mut dyn Outside, m: &mut Mem, node: si_mem::Ptr32<T>) {
    o.call(m, &SI_MODEL_COMPUTE_BOUNDING, &[node.addr()]);
}

/// `SI_fcurveIntegerValue (0x08200404)`.
pub fn si_fcurve_integer_value<T>(o: &mut dyn Outside, m: &mut Mem, curve: si_mem::Ptr32<T>, time: f32, a: i16, b: i16) -> i32 {
    o.call(m, &SI_FCURVE_INTEGER_VALUE, &[curve.addr(), time.to_bits(), a as i32 as u32, b as i32 as u32]).eax as i32
}

/// `SI_modelSetGlobalTranslation (0x0846412c)`: `position` is the address of an `SI_Vector`.
pub fn si_model_set_global_translation<T>(o: &mut dyn Outside, m: &mut Mem, node: si_mem::Ptr32<T>, position: u32, mode: i32, flag: i16) {
    o.call(m, &SI_MODEL_SET_GLOBAL_TRANSLATION, &[node.addr(), position, mode as u32, flag as i32 as u32]);
}

/// `SI_modelSetTranslation (0x084640cc)`: `translation` is the address of an `SI_Vector`.
pub fn si_model_set_translation<T>(o: &mut dyn Outside, m: &mut Mem, node: si_mem::Ptr32<T>, translation: u32, mode: i32, flag: i16) {
    o.call(m, &SI_MODEL_SET_TRANSLATION, &[node.addr(), translation, mode as u32, flag as i32 as u32]);
}

/// `SI_modelUpdateSonState (0x084662f8)`.
pub fn si_model_update_son_state<T>(o: &mut dyn Outside, m: &mut Mem, father: si_mem::Ptr32<T>, node: si_mem::Ptr32<T>, flag: i16) {
    o.call(m, &SI_MODEL_UPDATE_SON_STATE, &[father.addr(), node.addr(), flag as i32 as u32]);
}

/// `SI_findGlData (0x0832ee68)`: the address of the graf node.
pub fn si_find_gl_data(o: &mut dyn Outside, m: &mut Mem, list: u32, data: u32, kind: i32) -> u32 {
    o.call(m, &SI_FIND_GL_DATA, &[list, data, kind as u32]).eax
}
