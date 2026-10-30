// SPDX-License-Identifier: MIT
//! The parts of the program these three source files call that are not in this crate:
//! the geometry packages, the editing layer over the dictionaries, function curves,
//! drawing and dialogs.
//!
//! Nothing here reproduces a function of the original. A routine of another source file is
//! named by a [`Routine`] (its name and its address in the Linux binary) and reached
//! through [`Outside::call`] with the original's arguments as 32-bit stack words in C
//! order: the calling convention of `si_mem::Registry` and the shape of `si-deform`'s,
//! `si-view`'s and `si-dict`'s traits, so that it can become `si_mem::Outside` when the
//! lead decides. A `short` argument is passed as its 16 bits, zero-extended (the callee
//! reads no more); a `float` as its bits; a `double` as two words, low first. A result the
//! callee writes through a pointer goes to an address of the image: a local on the image's
//! stack (`Mem::local`). A routine that answers a `float` or `double` does so in the x87
//! register, unrounded: [`Outside::call_f`] answers all 80 bits.
//!
//! The block below is printed by `re/tools/si-anim/aoutside.py` with every address taken
//! from the binary's symbol table; `re/tools/si-anim/check_outside.py` holds them against
//! it again.

use si_mem::Mem;
use si_x87::F80;

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
    /// `routine(args…)` for a routine that answers in the x87 register.
    fn call_f(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> F80;

    /// What the program prints on `stdout` (`printf`'s output, already formatted); dropped by default.
    fn print(&mut self, _text: &[u8]) {}

    /// A call through a function pointer a ported routine was handed (the per-curve step of
    /// `hieranimFcvProcess` that is not one of this crate's): the code address and the
    /// arguments. An address of [`ALL`] goes to [`Outside::call`]; any other answers 0.
    fn call_addr(&mut self, m: &mut Mem, addr: u32, args: &[u32]) -> u32 {
        match ALL.iter().find(|r| r.addr == addr) {
            Some(r) => self.call(m, r, args),
            None => 0,
        }
    }
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
    fn call_f(&mut self, _m: &mut Mem, routine: &'static Routine, args: &[u32]) -> F80 {
        self.calls.push((routine, args.to_vec()));
        F80::ZERO
    }
}

/// The two stack words of a `double` argument, low first.
pub fn dw(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

macro_rules! outside {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine this crate names.
        pub const ALL: &[&Routine] = &[ $( &$konst ),* ];
    };
}

outside! {
    /// `SI_patchComputeSurface (0x0847c1d0)`: rebuilds a patch's surface from its control points.
    SI_PATCH_COMPUTE_SURFACE = "SI_patchComputeSurface", 0x0847c1d0;
    /// `SI_splineTouch (0x0848c924)`: marks a spline as changed (its tables are rebuilt on next use).
    SI_SPLINE_TOUCH = "SI_splineTouch", 0x0848c924;
    /// `SI_nurbsCrvMovePoint (0x084e9eec)`: moves one control point of a NURBS curve to x, y, z (three doubles).
    SI_NURBS_CRV_MOVE_POINT = "SI_nurbsCrvMovePoint", 0x084e9eec;
    /// `SI_nurbsSrfMovePoint (0x084e9f2c)`: moves one control point of a NURBS surface to x, y, z, w (four doubles).
    SI_NURBS_SRF_MOVE_POINT = "SI_nurbsSrfMovePoint", 0x084e9f2c;
    /// `SI_knotsCrvReparametrization (0x084de590)`: recomputes a NURBS curve's knots from its points.
    SI_KNOTS_CRV_REPARAMETRIZATION = "SI_knotsCrvReparametrization", 0x084de590;
    /// `SI_knotsSrfReparametrization (0x084ded40)`: recomputes a NURBS surface's knots from its points.
    SI_KNOTS_SRF_REPARAMETRIZATION = "SI_knotsSrfReparametrization", 0x084ded40;
    /// `SI_edGetElemFromShape (0x080fd4dc)`: the SHAPES element of a model; stores its index through the second argument.
    SI_ED_GET_ELEM_FROM_SHAPE = "SI_edGetElemFromShape", 0x080fd4dc;
    /// `SI_elemdictGetElem (0x08132b50)`: the element of a chapter by index.
    SI_ELEMDICT_GET_ELEM = "SI_elemdictGetElem", 0x08132b50;
    /// `SI_elemdictGetElemFromHierAnim (0x081286e4)`: the element a hierarchy-animation entry stands for, in a chapter.
    SI_ELEMDICT_GET_ELEM_FROM_HIER_ANIM = "SI_elemdictGetElemFromHierAnim", 0x081286e4;
    /// `SI_getExprFcurveUsed (0x08249a00)`: which curves of an element expressions drive: their number and list through the two pointers; answers a buffer.
    SI_GET_EXPR_FCURVE_USED = "SI_getExprFcurveUsed", 0x08249a00;
    /// `SI_edFindFcv (0x080edb20)`: the first curve of a type an element has.
    SI_ED_FIND_FCV = "SI_edFindFcv", 0x080edb20;
    /// `SI_edFindNextFcv (0x080edb74)`: the next curve of that type.
    SI_ED_FIND_NEXT_FCV = "SI_edFindNextFcv", 0x080edb74;
    /// `SI_fcurveValue (0x082002dc)`: a function curve's value at a time (in the x87 register).
    SI_FCURVE_VALUE = "SI_fcurveValue", 0x082002dc;
    /// `SI_edGetElemFromCluster (0x080fd510)`: a cluster's element; stores its index through the second argument.
    SI_ED_GET_ELEM_FROM_CLUSTER = "SI_edGetElemFromCluster", 0x080fd510;
    /// `SI_elemdictFindRangeRelatedElem (0x081353c4)`: the first element related to one by a relation of a range of types; stores chapter and index.
    SI_ELEMDICT_FIND_RANGE_RELATED_ELEM = "SI_elemdictFindRangeRelatedElem", 0x081353c4;
    /// `SI_elemdictFindRangeNextRelatedElem (0x08135484)`: the next such element.
    SI_ELEMDICT_FIND_RANGE_NEXT_RELATED_ELEM = "SI_elemdictFindRangeNextRelatedElem", 0x08135484;
    /// `si_fcvScnglDelete (0x0823c8f8)`: deletes a curve and its place in the scene's lists.
    SI_FCV_SCNGL_DELETE = "si_fcvScnglDelete", 0x0823c8f8;
    /// `SI_edFcvDispose (0x080ee2f0)`: frees the curves of two types an object carries in a chapter.
    SI_ED_FCV_DISPOSE = "SI_edFcvDispose", 0x080ee2f0;
    /// `SI_refreshFcurve (0x08323ce0)`: redraws the function curve windows.
    SI_REFRESH_FCURVE = "SI_refreshFcurve", 0x08323ce0;
    /// `SI_statusDisplayMessage (0x083cca94)`: shows a message in the status line.
    SI_STATUS_DISPLAY_MESSAGE = "SI_statusDisplayMessage", 0x083cca94;
    /// `SI_meshComputesPolygonNormal (0x084599f4)`: recomputes polygon normals of a mesh (all for -1).
    SI_MESH_COMPUTES_POLYGON_NORMAL = "SI_meshComputesPolygonNormal", 0x084599f4;
    /// `SI_savekeyModel (0x08219fe0)`: sets a key on a model's parameter (the shape key here).
    SI_SAVEKEY_MODEL = "SI_savekeyModel", 0x08219fe0;
    /// `SI_edGetElemFromModel (0x080fcf20)`: a model node's element; stores its index through the second argument.
    SI_ED_GET_ELEM_FROM_MODEL = "SI_edGetElemFromModel", 0x080fcf20;
    /// `SI_elemGetNbVertices (0x084769b4)`: how many points an element's geometry has in a definition.
    SI_ELEM_GET_NB_VERTICES = "SI_elemGetNbVertices", 0x084769b4;
    /// `SI_fcurveAllocate (0x0820a810)`: a new function curve of a type for an owner.
    SI_FCURVE_ALLOCATE = "SI_fcurveAllocate", 0x0820a810;
    /// `SI_edFcvInsert (0x080ee440)`: puts a curve into an element's list of curves (chapter, index, number).
    SI_ED_FCV_INSERT = "SI_edFcvInsert", 0x080ee440;
    /// `SI_fkeyFrameCreate (0x0820b1b0)`: a new key: time, value, a third float, two shorts.
    SI_FKEY_FRAME_CREATE = "SI_fkeyFrameCreate", 0x0820b1b0;
    /// `SI_fcurveAddKey (0x081ff400)`: adds a key to a curve.
    SI_FCURVE_ADD_KEY = "SI_fcurveAddKey", 0x081ff400;
    /// `SI_edFcvFilterDispose (0x080ee0a0)`: frees the curves of a type an object carries in a chapter, from a number on.
    SI_ED_FCV_FILTER_DISPOSE = "SI_edFcvFilterDispose", 0x080ee0a0;
    /// `SI_fcurveRemove (0x081ff0a0)`: frees a curve (through the address of the caller's pointer).
    SI_FCURVE_REMOVE = "SI_fcurveRemove", 0x081ff0a0;
    /// `SI_fcurveRemoveAllKeys (0x081ff8bc)`: takes every key off a curve.
    SI_FCURVE_REMOVE_ALL_KEYS = "SI_fcurveRemoveAllKeys", 0x081ff8bc;
    /// `SI_modelDeleteIDefinition (0x08467c24)`: takes one definition (a shape) off a model.
    SI_MODEL_DELETE_I_DEFINITION = "SI_modelDeleteIDefinition", 0x08467c24;
    /// `si_fcurveRemoveKeyBetween (0x08201350)`: takes the keys between two times off a curve.
    SI_FCURVE_REMOVE_KEY_BETWEEN = "si_fcurveRemoveKeyBetween", 0x08201350;
    /// `si_fcurveIncValueAbove (0x08200e2c)`: adds a value to the keys above a value.
    SI_FCURVE_INC_VALUE_ABOVE = "si_fcurveIncValueAbove", 0x08200e2c;
    /// `SI_modelResetFcv (0x08221224)`: resets a model's curves of a type.
    SI_MODEL_RESET_FCV = "SI_modelResetFcv", 0x08221224;
    /// `SI_elemdictFindRelatedElem (0x0813508c)`: the element related to one by a relation of a type; stores chapter and index.
    SI_ELEMDICT_FIND_RELATED_ELEM = "SI_elemdictFindRelatedElem", 0x0813508c;
    /// `SI_edIsElementAnimated (0x080edd90)`: whether an element has animation.
    SI_ED_IS_ELEMENT_ANIMATED = "SI_edIsElementAnimated", 0x080edd90;
    /// `SI_modelResetShape (0x08221614)`: takes a model's shape animation away.
    SI_MODEL_RESET_SHAPE = "SI_modelResetShape", 0x08221614;
    /// `SI_edAddShape (0x081036a8)`: gives a model a SHAPES element.
    SI_ED_ADD_SHAPE = "SI_edAddShape", 0x081036a8;
    /// `SI_edFcvAdd (0x080ee650)`: adds a curve to an element's list of curves (chapter, index).
    SI_ED_FCV_ADD = "SI_edFcvAdd", 0x080ee650;
    /// `SI_secAsFrame (0x082015b8)`: a time in seconds as a frame (in the x87 register).
    SI_SEC_AS_FRAME = "SI_secAsFrame", 0x082015b8;
    /// `SI_fkeyFrame (0x0820b29c)`: the frame of a key.
    SI_FKEY_FRAME = "SI_fkeyFrame", 0x0820b29c;
    /// `SI_fcurveFrameValue (0x08200258)`: a function curve's value at a frame (in the x87 register).
    SI_FCURVE_FRAME_VALUE = "SI_fcurveFrameValue", 0x08200258;
    /// `si_modelSetIDefinition (0x0821cbc4)`: makes a model's definition of a number the one shown.
    SI_MODEL_SET_I_DEFINITION = "si_modelSetIDefinition", 0x0821cbc4;
    /// `SI_modelAddIDefinition (0x084677c8)`: inserts a definition (a copy of a geometry) at a number.
    SI_MODEL_ADD_I_DEFINITION = "SI_modelAddIDefinition", 0x084677c8;
    /// `SI_modelCopyOrgDef (0x08467d6c)`: keeps a copy of a geometry as the model's original definition.
    SI_MODEL_COPY_ORG_DEF = "SI_modelCopyOrgDef", 0x08467d6c;
    /// `SI_meshCopy (0x084404e0)`: a copy of a mesh.
    SI_MESH_COPY = "SI_meshCopy", 0x084404e0;
    /// `SI_patchCopy (0x0847a2b8)`: a copy of a patch.
    SI_PATCH_COPY = "SI_patchCopy", 0x0847a2b8;
    /// `SI_faceCopy (0x08434990)`: a copy of a face.
    SI_FACE_COPY = "SI_faceCopy", 0x08434990;
    /// `SI_splineCopy (0x0848aa74)`: a copy of a spline.
    SI_SPLINE_COPY = "SI_splineCopy", 0x0848aa74;
    /// `SI_nurbsCrvCopy (0x084dbbf4)`: a copy of a NURBS curve.
    SI_NURBS_CRV_COPY = "SI_nurbsCrvCopy", 0x084dbbf4;
    /// `SI_nurbsSrfCopy (0x084dc1a8)`: a copy of a NURBS surface.
    SI_NURBS_SRF_COPY = "SI_nurbsSrfCopy", 0x084dc1a8;
    /// `SI_fkeyCreate (0x0820b248)`: a new key: time, value, a third float, two shorts.
    SI_FKEY_CREATE = "SI_fkeyCreate", 0x0820b248;
    /// `SI_elemdictFindElemByData (0x08134364)`: the number of the element of a chapter whose data is a pointer.
    SI_ELEMDICT_FIND_ELEM_BY_DATA = "SI_elemdictFindElemByData", 0x08134364;
    /// `SI_fcvGetName (0x0810f678)`: a curve's name.
    SI_FCV_GET_NAME = "SI_fcvGetName", 0x0810f678;
    /// `SI_dbFormatFullName (0x081223dc)`: an element's full name, in a block of its own.
    SI_DB_FORMAT_FULL_NAME = "SI_dbFormatFullName", 0x081223dc;
    /// `si_singleFcurveCopy (0x0822b3f4)`: copies an element's curve of a type onto another element's (index, type).
    SI_SINGLE_FCURVE_COPY = "si_singleFcurveCopy", 0x0822b3f4;
    /// `SI_fcurveTransfer (0x081ff0e4)`: copies one curve's keys into another.
    SI_FCURVE_TRANSFER = "SI_fcurveTransfer", 0x081ff0e4;
    /// `SI_modelTouchGeometry (0x0846d48c)`: marks a model's geometry as changed.
    SI_MODEL_TOUCH_GEOMETRY = "SI_modelTouchGeometry", 0x0846d48c;
    /// `SI_fcurveCheckAdd1 (0x08219560)`: an element's curve of a type in a chapter, made when missing; stores it through the last argument.
    SI_FCURVE_CHECK_ADD1 = "SI_fcurveCheckAdd1", 0x08219560;
    /// `SI_getFcurveDname (0x08203afc)`: sets a curve's display name (from a buffer of the caller's).
    SI_GET_FCURVE_DNAME = "SI_getFcurveDname", 0x08203afc;
    /// `SI_updateWeightFcvNames (0x0810b094)`: renames an element's weight curves after their places.
    SI_UPDATE_WEIGHT_FCV_NAMES = "SI_updateWeightFcvNames", 0x0810b094;
    /// `SI_frameToSec (0x08201554)`: a frame as a time in seconds (in the x87 register).
    SI_FRAME_TO_SEC = "SI_frameToSec", 0x08201554;
    /// `SI_envelopCompUpdate (0x084b6210)`: brings a model's envelope up to date.
    SI_ENVELOP_COMP_UPDATE = "SI_envelopCompUpdate", 0x084b6210;
    /// `SI_hvectorMultiplyMatrix (0x08516c28)`: multiplies an array of homogeneous points by a matrix.
    SI_HVECTOR_MULTIPLY_MATRIX = "SI_hvectorMultiplyMatrix", 0x08516c28;
    /// `SI_vectorMultiplyMatrix (0x08516d74)`: multiplies an array of vectors by a matrix.
    SI_VECTOR_MULTIPLY_MATRIX = "SI_vectorMultiplyMatrix", 0x08516d74;
    /// `SI_audioMarkerMngUnselectAll (0x08375848)`: unselects the markers of an audio track.
    SI_AUDIO_MARKER_MNG_UNSELECT_ALL = "SI_audioMarkerMngUnselectAll", 0x08375848;
    /// `SI_dialogRequest3 (0x08141e84)`: the request dialog with three answers (request.c): message, second line (a buffer of the caller's), third line, three buttons.
    SI_DIALOG_REQUEST3 = "SI_dialogRequest3", 0x08141e84;
    /// `SI_reldictGetRelChapGrp (0x0813a464)`: the chapter and group of a relation type (reldict, type, &chapter).
    SI_RELDICT_GET_REL_CHAP_GRP = "SI_reldictGetRelChapGrp", 0x0813a464;
    /// `SI_fcvSimplePasteInsert (0x08202f34)`: pastes the keys of one curve into another (to, from, two times).
    SI_FCV_SIMPLE_PASTE_INSERT = "SI_fcvSimplePasteInsert", 0x08202f34;
    /// `SI_elemdictAddCompleteRelation (0x08132dc4)`: relates two elements with an entry's active word (nine arguments).
    SI_ELEMDICT_ADD_COMPLETE_RELATION = "SI_elemdictAddCompleteRelation", 0x08132dc4;
    /// `SI_klusterGetActive (0x0843b4b8)`: the model's active cluster.
    SI_KLUSTER_GET_ACTIVE = "SI_klusterGetActive", 0x0843b4b8;
    /// `SI_elemdictFindNextRelatedElem (0x08135154)`: the next element related to one by a relation of a type; stores chapter and index.
    SI_ELEMDICT_FIND_NEXT_RELATED_ELEM = "SI_elemdictFindNextRelatedElem", 0x08135154;
    /// `SI_elemdictGetRelation (0x08134dec)`: the n-th entry of a relation of an element (its relation record).
    SI_ELEMDICT_GET_RELATION = "SI_elemdictGetRelation", 0x08134dec;
    /// `SI_edGetFcvFromRelation (0x080ee6f0)`: the function curve a relation entry names (an ANIMATION element's data).
    SI_ED_GET_FCV_FROM_RELATION = "SI_edGetFcvFromRelation", 0x080ee6f0;
    /// `SI_edFindSon (0x080fcec0)`: the first son of an element in the current dictionary.
    SI_ED_FIND_SON = "SI_edFindSon", 0x080fcec0;
    /// `SI_edFindNextSon (0x080fcef0)`: the next son.
    SI_ED_FIND_NEXT_SON = "SI_edFindNextSon", 0x080fcef0;
    /// `SI_edGetElemFromAnimFile (0x080fd5ec)`: the element of an animation file's data; stores its index through the second argument.
    SI_ED_GET_ELEM_FROM_ANIM_FILE = "SI_edGetElemFromAnimFile", 0x080fd5ec;
    /// `SI_fcurveSaveHistory (0x080ca6f4)`: saves a function curve for undo.
    SI_FCURVE_SAVE_HISTORY = "SI_fcurveSaveHistory", 0x080ca6f4;
    /// `SI_fcurveRemoveKeyBetweenTimes (0x082013cc)`: takes the keys between two times off a curve.
    SI_FCURVE_REMOVE_KEY_BETWEEN_TIMES = "SI_fcurveRemoveKeyBetweenTimes", 0x082013cc;
    /// `SI_fcvMoveKeyTime (0x081ffd7c)`: moves the keys of a curve between two times by an offset.
    SI_FCV_MOVE_KEY_TIME = "SI_fcvMoveKeyTime", 0x081ffd7c;
    /// `SI_fcvScaleKeyTime (0x081fffa0)`: scales the key times of a curve about a pivot.
    SI_FCV_SCALE_KEY_TIME = "SI_fcvScaleKeyTime", 0x081fffa0;
    /// `SI_fcvSetKeysOnFrames (0x081ffe3c)`: puts the keys between two times on whole frames.
    SI_FCV_SET_KEYS_ON_FRAMES = "SI_fcvSetKeysOnFrames", 0x081ffe3c;
    /// `si_fcurveSetIntervalKeyLocks (0x08202028)`: locks or unlocks the keys between two times.
    SI_FCURVE_SET_INTERVAL_KEY_LOCKS = "si_fcurveSetIntervalKeyLocks", 0x08202028;
    /// `SI_fcvCycleKeysBetweenTimes (0x08203408)`: repeats the keys between two times so many times.
    SI_FCV_CYCLE_KEYS_BETWEEN_TIMES = "SI_fcvCycleKeysBetweenTimes", 0x08203408;
    /// `SI_fcvCntKeysBetweenTimes (0x082031dc)`: how many keys lie between two times.
    SI_FCV_CNT_KEYS_BETWEEN_TIMES = "SI_fcvCntKeysBetweenTimes", 0x082031dc;
    /// `getbutton (0x0860e684)`: the state of a device button (here 6 and 5: the keys that flip the paste mode).
    GETBUTTON = "getbutton", 0x0860e684;
    /// `SI_historyOpenCluster (0x080c7270)`: opens a group of undo steps.
    SI_HISTORY_OPEN_CLUSTER = "SI_historyOpenCluster", 0x080c7270;
    /// `SI_historyCloseCluster (0x080c7298)`: closes it.
    SI_HISTORY_CLOSE_CLUSTER = "SI_historyCloseCluster", 0x080c7298;
    /// `SI_actionsSaveHistory (0x080c9588)`: saves an action list for undo.
    SI_ACTIONS_SAVE_HISTORY = "SI_actionsSaveHistory", 0x080c9588;
    /// `SI_elemdictGetElemFromData (0x08137f94)`: the element of a chapter whose data is a pointer; stores its index through the last argument.
    SI_ELEMDICT_GET_ELEM_FROM_DATA = "SI_elemdictGetElemFromData", 0x08137f94;
    /// `SI_relGetConsRelFromGrafType (0x080fcba0)`: the constraint relation of a graf node's type.
    SI_REL_GET_CONS_REL_FROM_GRAF_TYPE = "SI_relGetConsRelFromGrafType", 0x080fcba0;
    /// `SI_elemdictFindElemByNameOnly (0x08134770)`: the number of the element of a chapter by name only (no hierarchy), -1 for none.
    SI_ELEMDICT_FIND_ELEM_BY_NAME_ONLY = "SI_elemdictFindElemByNameOnly", 0x08134770;
    /// `SI_actionCreateNullOnPath (0x0821e268)`: makes the null a path action follows between two frames; the last argument is where it would store the null's record (a block of the caller's, left unwritten by the stand-in).
    SI_ACTION_CREATE_NULL_ON_PATH = "SI_actionCreateNullOnPath", 0x0821e268;
    /// `SI_actionCnsToNull (0x0821e3c0)`: moves an action's path constraint to a null between two frames.
    SI_ACTION_CNS_TO_NULL = "SI_actionCnsToNull", 0x0821e3c0;
    /// `SI_fcurveInitialize (0x0820a9d0)`: sets a function curve record to its starting state.
    SI_FCURVE_INITIALIZE = "SI_fcurveInitialize", 0x0820a9d0;
    /// `SI_fcvCopyKeysBetweenTimes (0x08203234)`: copies the keys of one curve between two times into another, at a new start time (fourth float: a scale).
    SI_FCV_COPY_KEYS_BETWEEN_TIMES = "SI_fcvCopyKeysBetweenTimes", 0x08203234;
    /// `SI_attdictGetInfoFromFcurve (0x08120264)`: an attribute's description from a curve's type: a relation type, a relation group, two counts (through four pointers).
    SI_ATTDICT_GET_INFO_FROM_FCURVE = "SI_attdictGetInfoFromFcurve", 0x08120264;
    /// `SI_freeDopeShBuf (0x081fef44)`: frees the dope sheet's buffer tree (the pointer's address) and sets it empty.
    SI_FREE_DOPE_SH_BUF = "SI_freeDopeShBuf", 0x081fef44;
    /// `SI_dialogRequest (0x08141c00)`: the request dialog (four message lines of the binary's text).
    SI_DIALOG_REQUEST = "SI_dialogRequest", 0x08141c00;
    /// `SI_elemdictFindElemByPrjName (0x08134678)`: the element of a chapter by its prefix and name.
    SI_ELEMDICT_FIND_ELEM_BY_PRJ_NAME = "SI_elemdictFindElemByPrjName", 0x08134678;
    /// `SI_relGetAnimFRelFromChapter (0x0810f540)`: the relation type that ties a chapter's elements to their animation file.
    SI_REL_GET_ANIM_F_REL_FROM_CHAPTER = "SI_relGetAnimFRelFromChapter", 0x0810f540;
    /// `SI_hieranimDispose (0x08138b5c)`: frees a hierarchy's animation record.
    SI_HIERANIM_DISPOSE = "SI_hieranimDispose", 0x08138b5c;
    /// `SI_findGlData (0x0832ee68)`: the graph node of a graph list that holds the data of a type (or 0).
    SI_FIND_GL_DATA = "SI_findGlData", 0x0832ee68;
    /// `SI_graflistDeleteGrafnode (0x0832cd50)`: takes a node off a graph list.
    SI_GRAFLIST_DELETE_GRAFNODE = "SI_graflistDeleteGrafnode", 0x0832cd50;
    /// `SI_elemdictSetDeleteElem (0x08135ad4)`: deletes an element of a chapter (dictionary, chapter, element, three more numbers).
    SI_ELEMDICT_SET_DELETE_ELEM = "SI_elemdictSetDeleteElem", 0x08135ad4;
    /// `SI_elemdictSortAnimElements (0x08137980)`: sorts the animation chapter's elements.
    SI_ELEMDICT_SORT_ANIM_ELEMENTS = "SI_elemdictSortAnimElements", 0x08137980;
    /// `SI_fcvCheckTypeRange (0x0810f6e4)`: whether a curve's type lies in a range.
    SI_FCV_CHECK_TYPE_RANGE = "SI_fcvCheckTypeRange", 0x0810f6e4;
    /// `SI_reldictGetRelName (0x0813a0f4)`: a relation's name.
    SI_RELDICT_GET_REL_NAME = "SI_reldictGetRelName", 0x0813a0f4;
    /// `SI_elemdictFindElemFromGrafNode (0x080fcbec)`: the element of a graph node; stores its chapter and index.
    SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE = "SI_elemdictFindElemFromGrafNode", 0x080fcbec;
    /// `SI_elemdictGetNewName (0x0813697c)`: a free name in a chapter for a prefix, in a block of its own.
    SI_ELEMDICT_GET_NEW_NAME = "SI_elemdictGetNewName", 0x0813697c;
    /// `SI_dbListNodeAllocate (0x0812250c)`: a new list node.
    SI_DB_LIST_NODE_ALLOCATE = "SI_dbListNodeAllocate", 0x0812250c;
    /// `SI_dbListNodeCopyInfo (0x08131ba8)`: copies an element's information to a list node.
    SI_DB_LIST_NODE_COPY_INFO = "SI_dbListNodeCopyInfo", 0x08131ba8;
    /// `SI_elemdictAddElem (0x08132a5c)`: puts a node and its data into a chapter; answers the index.
    SI_ELEMDICT_ADD_ELEM = "SI_elemdictAddElem", 0x08132a5c;
    /// `SI_hieranimAllocate (0x08138b28)`: a new animation record for a chapter.
    SI_HIERANIM_ALLOCATE = "SI_hieranimAllocate", 0x08138b28;
    /// `SI_graflistAddData (0x0832d240)`: adds data to a graph list.
    SI_GRAFLIST_ADD_DATA = "SI_graflistAddData", 0x0832d240;
    /// `SI_elemdictAddRelation (0x08132f78)`: relates two elements.
    SI_ELEMDICT_ADD_RELATION = "SI_elemdictAddRelation", 0x08132f78;
    /// `SI_schematicHierOperation (0x0833c5dc)`: tells the schematic about a hierarchy operation.
    SI_SCHEMATIC_HIER_OPERATION = "SI_schematicHierOperation", 0x0833c5dc;
    /// `SI_elemdictAddUnCompleteRelation (0x08132fb4)`: relates two elements, the second not yet complete.
    SI_ELEMDICT_ADD_UN_COMPLETE_RELATION = "SI_elemdictAddUnCompleteRelation", 0x08132fb4;
    /// `SI_fastSetDirty (0x08329898)`: marks a view dirty.
    SI_FAST_SET_DIRTY = "SI_fastSetDirty", 0x08329898;
    /// `SI_dopeFlagsSet (0x081fa5d0)`: sets the dope sheet's flags.
    SI_DOPE_FLAGS_SET = "SI_dopeFlagsSet", 0x081fa5d0;
    /// `SI_dbGrafType2Chapter (0x080fcaf0)`: the chapter of a graph type.
    SI_DB_GRAF_TYPE2_CHAPTER = "SI_dbGrafType2Chapter", 0x080fcaf0;
    /// `SI_attdictValidateAttr (0x0811f118)`: whether an attribute index is valid for an element record (a 16-bit answer).
    SI_ATTDICT_VALIDATE_ATTR = "SI_attdictValidateAttr", 0x0811f118;
    /// `SI_attdictAttribGroupGetByIndex (0x0811ee3c)`: an attribute group's record by index.
    SI_ATTDICT_ATTRIB_GROUP_GET_BY_INDEX = "SI_attdictAttribGroupGetByIndex", 0x0811ee3c;
    /// `SI_attdictAttributeGetByIndex (0x0811ef0c)`: an attribute's record by index.
    SI_ATTDICT_ATTRIBUTE_GET_BY_INDEX = "SI_attdictAttributeGetByIndex", 0x0811ef0c;
    /// `SI_edElementAttValueGet (0x080f00f0)`: an element's attribute value: a type word and two value words through the third pointer (a 16-bit answer).
    SI_ED_ELEMENT_ATT_VALUE_GET = "SI_edElementAttValueGet", 0x080f00f0;
    /// `SI_enmdictEnumCodeGetByValue (0x08138834)`: the name of an enumeration code by its value.
    SI_ENMDICT_ENUM_CODE_GET_BY_VALUE = "SI_enmdictEnumCodeGetByValue", 0x08138834;
    /// `SI_edGetElemFromLight (0x080fd0bc)`: a light's element; stores its index through the second argument.
    SI_ED_GET_ELEM_FROM_LIGHT = "SI_edGetElemFromLight", 0x080fd0bc;
    /// `SI_elemdictSetCurrRelPointer (0x08134fdc)`: points an element's current relation of a type at its first one; stores its index (a 16-bit answer).
    SI_ELEMDICT_SET_CURR_REL_POINTER = "SI_elemdictSetCurrRelPointer", 0x08134fdc;
    /// `SI_elemdictGetCurRelation (0x08134db0)`: an element's current relation record.
    SI_ELEMDICT_GET_CUR_RELATION = "SI_elemdictGetCurRelation", 0x08134db0;
    /// `SI_elemdictSetRelToSerialid (0x0813354c)`: sets an element's current relation of a type to a serial number.
    SI_ELEMDICT_SET_REL_TO_SERIALID = "SI_elemdictSetRelToSerialid", 0x0813354c;
    /// `SI_elemdictFindSerialRelation (0x0813335c)`: the next serial relation of a type an element has; stores its index.
    SI_ELEMDICT_FIND_SERIAL_RELATION = "SI_elemdictFindSerialRelation", 0x0813335c;
    /// `SI_modelRoot (0x0847838c)`: the root of a model's hierarchy.
    SI_MODEL_ROOT = "SI_modelRoot", 0x0847838c;
    /// `SI_actionsDrFcurveMarkers (0x081e7930)`: draws the action markers in the function curve window (actions.c, drawing) (named, not called).
    SI_ACTIONS_DR_FCURVE_MARKERS = "SI_actionsDrFcurveMarkers", 0x081e7930;
    /// `fcurveActElemDraw (0x081e7a8c)`: draws one action's marker and name (actions.c, drawing) (named, not called).
    FCURVE_ACT_ELEM_DRAW = "fcurveActElemDraw", 0x081e7a8c;
}
