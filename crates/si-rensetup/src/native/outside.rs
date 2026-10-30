// SPDX-License-Identifier: MIT
//! The callees of other packages, behind one trait.
//!
//! `instance.c`, the two render commands and the routines of `edtools.c` ported beside
//! them call 80 routines that belong to other source files: the dictionary, the model library, the scene graph's list, the lattice, envelope
//! and material code, the GL layer, dialogues and the status line, and the renderer's
//! entry. None of them is ported in this package, and none is cited here: each is named
//! below as `name (0xADDRESS)`, which is how a routine that is not ported is written.
//!
//! The trait has one method, the original's own way of calling: a code address and the
//! argument words the caller pushes, in C order; the answer is what the callee leaves in
//! `eax`. That is the calling convention of `si_mem::Registry` and of the traits of the
//! same shape in `si-view`, `si-model`, `si-dict` and `si-render`: [`super::world::World`]
//! is the implementation that hands each call to the port that exists for it, and a check
//! can answer from the calls an emulated run recorded. A pointer argument is an address in
//! the image; an output argument is the address of a local the caller took from the
//! image's stack (`Mem::local`).
//!
//! Not behind the trait, because they are not another package's state: the C library's
//! `free`, `strstr` and `sprintf` and the program's `SI_malloc (0x080d0ee8)` and
//! `SI_calloc (0x080d0f50)`, which are `si-mem`'s allocator; and the four matrix routines
//! of `algebra.c`, which `si-math` has as functions of values.

use si_mem::{FnPtr, Mem};

/// The rest of the program, as the routines of this module call it.
pub trait Outside {
    /// Calls the routine at `f` with the stack words `args` and answers its `eax`.
    fn call(&mut self, m: &mut Mem, f: FnPtr, args: &[u32]) -> u32;
}

macro_rules! callees {
    ( $( $(#[$meta:meta])* $id:ident = $addr:literal, $name:literal; )* ) => {
        $( $(#[$meta])* pub const $id: FnPtr = FnPtr::new($addr); )*
        /// Address and name of every callee above.
        pub const CALLEES: &[(u32, &str)] = &[ $( ($addr, $name) ),* ];
    };
}

callees! {
    // ---- the renderer and its window
    /// SI_image (0x08257a70), `rnd_main.c`: the native renderer's entry
    /// (camera, graf list word +0x14, graf list word +0x1c, lights, roots); answers a status.
    SI_IMAGE = 0x08257a70, "SI_image";
    /// SI_chunkFreeAll (0x0857cb00), `mem_chunk.c`: (chunk buffer).
    SI_CHUNK_FREE_ALL = 0x0857cb00, "SI_chunkFreeAll";
    /// SI_openRenderWindow (0x082c9c14), `renu_display.c`: (controls); answers a status.
    SI_OPEN_RENDER_WINDOW = 0x082c9c14, "SI_openRenderWindow";
    /// SI_closeRenderWindow (0x082c9e18), `renu_display.c`: (controls).
    SI_CLOSE_RENDER_WINDOW = 0x082c9e18, "SI_closeRenderWindow";
    /// SI_doSubregion (0x082c9e50), `renu_subregion.c`: the user drags the sub-region box;
    /// 0 when cancelled.
    SI_DO_SUBREGION = 0x082c9e50, "SI_doSubregion";
    /// SI_rgbwindow (0x08290e38), `matter.c`: opens the full-screen RGB window of a preview.
    SI_RGBWINDOW = 0x08290e38, "SI_rgbwindow";
    /// SI_menuwindow (0x08291028), `matter.c`: (dialogue) closes that window and redraws.
    SI_MENUWINDOW = 0x08291028, "SI_menuwindow";
    /// SI_prepareRendering (0x082c57e4), `rens_main.c`: (execute parameters, element out,
    /// float out, preview flag); answers 0x7f when nothing is to be rendered.
    SI_PREPARE_RENDERING = 0x082c57e4, "SI_prepareRendering";
    /// SI_mentalRunTracer (0x0842a8a0): (execute parameters, 0); answers a status.
    SI_MENTAL_RUN_TRACER = 0x0842a8a0, "SI_mentalRunTracer";
    /// SI_cleanupTranslator (0x08581c78): (execute parameters).
    SI_CLEANUP_TRANSLATOR = 0x08581c78, "SI_cleanupTranslator";
    /// SI_assignMaxcoc (0x082cb734): (short, float).
    SI_ASSIGN_MAXCOC = 0x082cb734, "SI_assignMaxcoc";
    /// SI_deleteRenderDictionary (0x082c56e0): (dictionary, dictionary, element, 1).
    SI_DELETE_RENDER_DICTIONARY = 0x082c56e0, "SI_deleteRenderDictionary";
    /// SI_cusCallCustomFunctions (0x0852ea24), `custom_dso.c`: (event, state).
    SI_CUS_CALL_CUSTOM_FUNCTIONS = 0x0852ea24, "SI_cusCallCustomFunctions";

    // ---- the mental ray front end (`rens_main.c`) and what it calls
    /// initialiseRenderParms (0x082c65d8): (execute parameters).
    INITIALISE_RENDER_PARMS = 0x082c65d8, "initialiseRenderParms";
    /// SI_initializePreviewStruct (0x082c55e0): (preview structure).
    SI_INITIALIZE_PREVIEW_STRUCT = 0x082c55e0, "SI_initializePreviewStruct";
    /// SI_createRenderDictionary (0x082c5600): (scene dictionary, block, expand, default
    /// material); answers the default material's element.
    SI_CREATE_RENDER_DICTIONARY = 0x082c5600, "SI_createRenderDictionary";
    /// updateSelectiveLights (0x082c6628): (render dictionary).
    UPDATE_SELECTIVE_LIGHTS = 0x082c6628, "updateSelectiveLights";
    /// SI_mentalShaderScan (0x082c7be0), `rens_udf.c`: (render dictionary, controls, flag).
    SI_MENTAL_SHADER_SCAN = 0x082c7be0, "SI_mentalShaderScan";
    /// SI_getRenderCamera (0x082cb590), `renu_util.c`: (0); answers the camera.
    SI_GET_RENDER_CAMERA = 0x082cb590, "SI_getRenderCamera";
    /// SI_checkHighlightedWindow (0x0842c370), `mr_util.c`: (); answers a 16-bit switch.
    SI_CHECK_HIGHLIGHTED_WINDOW = 0x0842c370, "SI_checkHighlightedWindow";
    /// SI_makeUDFArrayRelations (0x0854c994), `udf_fct.c`: ().
    SI_MAKE_UDF_ARRAY_RELATIONS = 0x0854c994, "SI_makeUDFArrayRelations";
    /// SI_generateMIPerFrameFileName (0x0842c40c), `mr_util.c`: (name, 0, 0, -1); answers
    /// a new name or null.
    SI_GENERATE_MI_PER_FRAME_FILE_NAME = 0x0842c40c, "SI_generateMIPerFrameFileName";
    /// SI_clampMaxcoc (0x082cb620), `renu_util.c`: (live, &float).
    SI_CLAMP_MAXCOC = 0x082cb620, "SI_clampMaxcoc";
    /// SI_checkAllModels (0x082cbaac), `renu_util.c`: (render dictionary, mode); answers a
    /// 16-bit switch.
    SI_CHECK_ALL_MODELS = 0x082cbaac, "SI_checkAllModels";
    /// SI_updateInstTransforms (0x082c76fc), `rens_instance.c`: (scene dictionary, render
    /// dictionary).
    SI_UPDATE_INST_TRANSFORMS = 0x082c76fc, "SI_updateInstTransforms";
    /// SI_buildPicFileName (0x082cb7a8), `renu_util.c`: (name, frame, field, 1, 1); answers
    /// a new name.
    SI_BUILD_PIC_FILE_NAME = 0x082cb7a8, "SI_buildPicFileName";
    /// qtest (0x0860e188), `event.c`: (); answers whether the queue holds an event.
    QTEST = 0x0860e188, "qtest";
    /// qread (0x0860e1bc), `event.c`: (&16-bit value).
    QREAD = 0x0860e1bc, "qread";
    /// getenv (0x080c3bb8), the C library's through the binary's own stub: (name).
    GETENV = 0x080c3bb8, "getenv";
    /// usleep (0x080c37b8), the C library's: (microseconds).
    USLEEP = 0x080c37b8, "usleep";
    /// tempnam (0x080c3ba8), the C library's: (directory or null, prefix); a new name.
    TEMPNAM = 0x080c3ba8, "tempnam";
    /// unlink (0x080c3c78), the C library's: (path).
    UNLINK = 0x080c3c78, "unlink";

    // ---- the render abort checks (`geometry.c`)
    /// SI_cusCheckRenderAbort (0x08535d30), `custom_dso.c`: ().
    SI_CUS_CHECK_RENDER_ABORT = 0x08535d30, "SI_cusCheckRenderAbort";
    /// SI_tabletGetButtonUpEvent (0x083cec2c), `tablet.c`: ().
    SI_TABLET_GET_BUTTON_UP_EVENT = 0x083cec2c, "SI_tabletGetButtonUpEvent";
    /// getbutton (0x0860e684), `input.c`: (button); answers whether it is down.
    GETBUTTON = 0x0860e684, "getbutton";
    /// readdisplay (0x08611d54), `graphics.c`: (x1, y1, x2, y2 as shorts, pixel buffer, hints);
    /// reads screen pixels (four bytes each, red first) into the buffer; answers 1.
    READDISPLAY = 0x08611d54, "readdisplay";
    /// SI_voluntaryExit (0x08260918): (4, flag); does not return in the original.
    SI_VOLUNTARY_EXIT = 0x08260918, "SI_voluntaryExit";

    // ---- the mental ray file's names (`renu_filepath.c`)
    /// SI_getmi2Output (0x08580cfc), `mi_mr_main.c`: (); answers a 16-bit switch.
    SI_GETMI2_OUTPUT = 0x08580cfc, "SI_getmi2Output";
    /// SI_spdlUseNamesrvImgRef (0x08580ec4), `mi_mr_main.c`: (); answers a 16-bit switch.
    SI_SPDL_USE_NAMESRV_IMG_REF = 0x08580ec4, "SI_spdlUseNamesrvImgRef";
    /// SI_dbDirSearchDatabaseFromPath (0x081218f4), `dbexternal.c`: (directory list, path,
    /// mode, &record found, buffer); answers 100 when found.
    SI_DB_DIR_SEARCH_DATABASE_FROM_PATH = 0x081218f4, "SI_dbDirSearchDatabaseFromPath";

    // ---- the start-up of the stand-alone renderer (`sys_render.c`)
    /// access (0x080c3f18), the C library's through the binary's own stub: (path, mode);
    /// answers 0 when the file can be reached that way.
    ACCESS = 0x080c3f18, "access";
    /// getcwd (0x080c4d08), the C library's: (buffer or null, size); with a null buffer
    /// answers a new block holding the working directory.
    GETCWD = 0x080c4d08, "getcwd";
    /// SI_system (0x08126748), `dbfile.c`: (command line).
    SI_SYSTEM = 0x08126748, "SI_system";
    /// SI_historySetActive (0x080ca7f0), `history.c`: (flag).
    SI_HISTORY_SET_ACTIVE = 0x080ca7f0, "SI_historySetActive";
    /// SI_initDictionaries (0x0810824c), `dbloadsave.c`: (name, flag, flag).
    SI_INIT_DICTIONARIES = 0x0810824c, "SI_initDictionaries";
    /// SI_dbSetChapter (0x08122654), `dbexternal.c`: (database, chapter name).
    SI_DB_SET_CHAPTER = 0x08122654, "SI_dbSetChapter";
    /// SI_dbfilepathBuild (0x081226cc), `dbexternal.c`: (database).
    SI_DBFILEPATH_BUILD = 0x081226cc, "SI_dbfilepathBuild";
    /// SI_dbChapterCheckAccess (0x08120f20), `dbexternal.c`: (database, mode); answers a
    /// 16-bit flag.
    SI_DB_CHAPTER_CHECK_ACCESS = 0x08120f20, "SI_dbChapterCheckAccess";
    /// SI_initWave (0x084c5c70), `wave.c`: ().
    SI_INIT_WAVE = 0x084c5c70, "SI_initWave";
    /// SI_grafListCreate (0x0832c1bc), `graflist.c`: ().
    SI_GRAF_LIST_CREATE = 0x0832c1bc, "SI_grafListCreate";
    /// SI_grafListAdd (0x0832c204), `graflist.c`: (); answers the new list's index.
    SI_GRAF_LIST_ADD = 0x0832c204, "SI_grafListAdd";
    /// SI_newCamera (0x080d12dc), `camera.c`: (name, flag); answers the camera.
    SI_NEW_CAMERA = 0x080d12dc, "SI_newCamera";
    /// SI_animationInfoInit (0x080d335c), `animation.c`: ().
    SI_ANIMATION_INFO_INIT = 0x080d335c, "SI_animationInfoInit";
    /// SI_matterInit (0x08291bec), `matter.c`: ().
    SI_MATTER_INIT = 0x08291bec, "SI_matterInit";
    /// SI_metaballInit (0x0845abc8), `metaball.c`: ().
    SI_METABALL_INIT = 0x0845abc8, "SI_metaballInit";
    /// SI_cusInit (0x085325c4), `custom_dso.c`: ().
    SI_CUS_INIT = 0x085325c4, "SI_cusInit";

    // ---- the GL layer
    /// RGBcolor (0x086111a4), `graphics.c`: (red, green, blue).
    RGBCOLOR = 0x086111a4, "RGBcolor";
    /// cpack (0x08611104), `graphics.c`: (0xBBGGRR packed colour).
    CPACK = 0x08611104, "cpack";
    /// sboxi (0x086120b0), `graphics.c`: (x1, y1, x2, y2).
    SBOXI = 0x086120b0, "sboxi";
    /// recti (0x08612248), `graphics.c`: (x1, y1, x2, y2).
    RECTI = 0x08612248, "recti";
    /// sboxfi (0x0861236c), `graphics.c`: (x1, y1, x2, y2); reached through `SI_sboxfi`.
    SBOXFI = 0x0861236c, "sboxfi";
    /// getdescender (0x0860ec8c), `font.c`: ().
    GETDESCENDER = 0x0860ec8c, "getdescender";
    /// strwidth (0x0860f084), `font.c`: (text).
    STRWIDTH = 0x0860f084, "strwidth";
    /// cmov2i (0x0860f2ac), `font.c`: (x, y).
    CMOV2I = 0x0860f2ac, "cmov2i";
    /// charstr (0x0860ecc4), `font.c`: (text).
    CHARSTR = 0x0860ecc4, "charstr";

    // ---- status line, dialogues, events, redraw
    /// SI_statusDisplayMessage (0x083cca94), `status.c`: (text, kind).
    SI_STATUS_DISPLAY_MESSAGE = 0x083cca94, "SI_statusDisplayMessage";
    /// SI_displayStatusError (0x083cca7c), `status.c`: (text): the text on the status line
    /// as an error.
    SI_DISPLAY_STATUS_ERROR = 0x083cca7c, "SI_displayStatusError";
    /// SI_displayMouse (0x083cc630), `status.c`: (left, middle, right).
    SI_DISPLAY_MOUSE = 0x083cc630, "SI_displayMouse";
    /// SI_mdlStatusMessages (0x0818129c), `modeler.c`: ().
    SI_MDL_STATUS_MESSAGES = 0x0818129c, "SI_mdlStatusMessages";
    /// SI_dialogRequest (0x08141c00), `request.c`: (line, line, line, button).
    SI_DIALOG_REQUEST = 0x08141c00, "SI_dialogRequest";
    /// SI_systemNotice (0x0814230c), `request.c`: (line, line, line).
    SI_SYSTEM_NOTICE = 0x0814230c, "SI_systemNotice";
    /// SI_getEvent (0x08394058), `event.c`: (); answers the event record.
    SI_GET_EVENT = 0x08394058, "SI_getEvent";
    /// SI_gLSelect (0x0832ef0c), `graflist.c`: (window); answers the graf node picked.
    SI_GL_SELECT = 0x0832ef0c, "SI_gLSelect";
    /// SI_refreshAll (0x08325dc8), `draw.c`: (flag).
    SI_REFRESH_ALL = 0x08325dc8, "SI_refreshAll";
    /// SI_refreshCounter (0x080d1d20), `counter.c`: ().
    SI_REFRESH_COUNTER = 0x080d1d20, "SI_refreshCounter";
    /// sleep (0x080c4468), the C library's, through the binary's own stub: (seconds).
    SLEEP = 0x080c4468, "sleep";

    // ---- the dictionary
    /// SI_edGetElemFromModel (0x080fcf20), `edtools.c`: (model, &index); answers the
    /// model's element in the current dictionary.
    SI_ED_GET_ELEM_FROM_MODEL = 0x080fcf20, "SI_edGetElemFromModel";
    /// SI_elemdictGetElemFromData (0x08137f94), `elemdict.c`: (dictionary, chapter,
    /// object, &index).
    SI_ELEMDICT_GET_ELEM_FROM_DATA = 0x08137f94, "SI_elemdictGetElemFromData";
    /// SI_elemdictFindRelatedElem (0x0813508c), `elemdict.c`: (dictionary, element, side,
    /// list, relation, chapter, &index, &index).
    SI_ELEMDICT_FIND_RELATED_ELEM = 0x0813508c, "SI_elemdictFindRelatedElem";
    /// SI_elemdictFindNextRelatedElem (0x08135154), `elemdict.c`: the same arguments.
    SI_ELEMDICT_FIND_NEXT_RELATED_ELEM = 0x08135154, "SI_elemdictFindNextRelatedElem";
    /// SI_elemdictRelationsCount (0x08135700), `elemdict.c`: (dictionary, element, side,
    /// list, list, relation, chapter).
    SI_ELEMDICT_RELATIONS_COUNT = 0x08135700, "SI_elemdictRelationsCount";
    /// SI_elemdictAddRelation (0x08132f78), `elemdict.c`: (dictionary, relation
    /// dictionary, from, to, relation, 1, 0).
    SI_ELEMDICT_ADD_RELATION = 0x08132f78, "SI_elemdictAddRelation";
    /// SI_elemdictRemoveRelationId (0x08133998), `elemdict.c`: (dictionary, relation
    /// dictionary, number, relation).
    SI_ELEMDICT_REMOVE_RELATION_ID = 0x08133998, "SI_elemdictRemoveRelationId";
    /// SI_relRemoveRelations (0x081396d8), `relations.c`: (dictionary, element, chapter,
    /// index, side).
    SI_REL_REMOVE_RELATIONS = 0x081396d8, "SI_relRemoveRelations";
    /// SI_schematicHierOperation (0x0833c5dc), `graphsort.c`: (window, dictionary,
    /// element, chapter, operation).
    SI_SCHEMATIC_HIER_OPERATION = 0x0833c5dc, "SI_schematicHierOperation";
    /// SI_edLinkModel (0x080fe41c), `edtools.c`: (father's graf node, son's graf node, flag).
    SI_ED_LINK_MODEL = 0x080fe41c, "SI_edLinkModel";
    /// SI_edModelSetMaterial (0x08100c34), `edtools.c`: (model, material).
    SI_ED_MODEL_SET_MATERIAL = 0x08100c34, "SI_edModelSetMaterial";

    /// SI_elemdictFindElemByData (0x08134364), `elemdict.c`: (dictionary, chapter, object);
    /// answers the index of the object's element, or a negative number.
    SI_ELEMDICT_FIND_ELEM_BY_DATA = 0x08134364, "SI_elemdictFindElemByData";
    /// SI_elemdictGetElem (0x08132b50), `elemdict.c`: (dictionary, chapter, index).
    SI_ELEMDICT_GET_ELEM = 0x08132b50, "SI_elemdictGetElem";
    /// SI_elemdictFindSon (0x08137ff4), `elemdict.c`: (dictionary, element).
    SI_ELEMDICT_FIND_SON = 0x08137ff4, "SI_elemdictFindSon";
    /// SI_elemdictFindNextSon (0x08138030), `elemdict.c`: (dictionary, element).
    SI_ELEMDICT_FIND_NEXT_SON = 0x08138030, "SI_elemdictFindNextSon";
    /// SI_elemdictUpdateHierInfo (0x08134cc8), `elemdict.c`: (dictionary, chapter, list,
    /// relation).
    SI_ELEMDICT_UPDATE_HIER_INFO = 0x08134cc8, "SI_elemdictUpdateHierInfo";
    /// SI_elemdictSortHierElements (0x08140f30), `toposort.c`: (dictionary, chapter).
    SI_ELEMDICT_SORT_HIER_ELEMENTS = 0x08140f30, "SI_elemdictSortHierElements";
    /// SI_elemdictAddElem (0x08132a5c), `elemdict.c`: (dictionary, chapter, name record,
    /// object, second object, description, number); answers the new element's index.
    SI_ELEMDICT_ADD_ELEM = 0x08132a5c, "SI_elemdictAddElem";
    /// SI_elemdictGetNewName (0x0813697c), `elemdict.c`: (dictionary, chapter, name);
    /// answers a name no element of the chapter has.
    SI_ELEMDICT_GET_NEW_NAME = 0x0813697c, "SI_elemdictGetNewName";
    /// SI_dbListNodeAllocate (0x0812250c), `dbnode.c`: (); answers a new name record.
    SI_DB_LIST_NODE_ALLOCATE = 0x0812250c, "SI_dbListNodeAllocate";
    /// SI_dbFormatFullName (0x081223dc), `dbnode.c`: (name record); answers the full name.
    SI_DB_FORMAT_FULL_NAME = 0x081223dc, "SI_dbFormatFullName";

    // ---- the scene graph's list
    /// SI_graflistAddData (0x0832d240), `graflist.c`: (list, object, kind, flag); answers
    /// the new graf node.
    SI_GRAFLIST_ADD_DATA = 0x0832d240, "SI_graflistAddData";
    /// SI_graflistDeleteSelection (0x0832db14), `graflist.c`: (list, graf node).
    SI_GRAFLIST_DELETE_SELECTION = 0x0832db14, "SI_graflistDeleteSelection";
    /// SI_schematicNoRefresh (0x0832bfd0), `graflist.c`: (flag).
    SI_SCHEMATIC_NO_REFRESH = 0x0832bfd0, "SI_schematicNoRefresh";

    // ---- the model library
    /// SI_modelAllocate (0x08477cf4), `model_b.c`: (name or null); answers a new node.
    SI_MODEL_ALLOCATE = 0x08477cf4, "SI_modelAllocate";
    /// SI_modelUpdateSonState (0x084662f8), `model.c`: (father, node, flag).
    SI_MODEL_UPDATE_SON_STATE = 0x084662f8, "SI_modelUpdateSonState";
    /// SI_modelNextSon (0x0846d70c), `model.c`: (model, previous); answers the next node
    /// of the model's hierarchy.
    SI_MODEL_NEXT_SON = 0x0846d70c, "SI_modelNextSon";
    /// SI_modelCopy (0x084622cc), `model.c`: (model, father); answers the copy.
    SI_MODEL_COPY = 0x084622cc, "SI_modelCopy";
    /// SI_modelInverse (0x084732fc), `model.c`: (model).
    SI_MODEL_INVERSE = 0x084732fc, "SI_modelInverse";
    /// SI_modelSetState (0x084638a8), `model.c`: (model, &scaling, &rotation, &translation).
    SI_MODEL_SET_STATE = 0x084638a8, "SI_modelSetState";
    /// SI_modelCopyTransformation (0x08461f98), `model.c`: (from, to): `to` takes the place
    /// in space `from` has.
    SI_MODEL_COPY_TRANSFORMATION = 0x08461f98, "SI_modelCopyTransformation";
    /// SI_modelCopyFromSetup (0x084633e8), `model.c`: (from, to).
    SI_MODEL_COPY_FROM_SETUP = 0x084633e8, "SI_modelCopyFromSetup";
    /// SI_modelTransfoAnimCopy (0x08462f1c), `model.c`: (from, to).
    SI_MODEL_TRANSFO_ANIM_COPY = 0x08462f1c, "SI_modelTransfoAnimCopy";
    /// SI_modelCut (0x0846d838), `model.c`: (graf node, text buffer).
    SI_MODEL_CUT = 0x0846d838, "SI_modelCut";
    /// SI_modelUpdateDependency (0x0846f200), `model.c`: (model, model).
    SI_MODEL_UPDATE_DEPENDENCY = 0x0846f200, "SI_modelUpdateDependency";
    /// SI_modelCreateParent (0x0817b500), `mod_global.c`: (father's graf node, son's graf
    /// node, flag).
    SI_MODEL_CREATE_PARENT = 0x0817b500, "SI_modelCreateParent";
    /// SI_modelChangeName (0x0846db14), `model.c`: (model, name).
    SI_MODEL_CHANGE_NAME = 0x0846db14, "SI_modelChangeName";
    /// SI_modelAlgoTypeCopy (0x084a0efc), `settings.c`: (from, to).
    SI_MODEL_ALGO_TYPE_COPY = 0x084a0efc, "SI_modelAlgoTypeCopy";
    /// SI_invKinCopy (0x084adab0), `invkin.c`: (record); answers the copy.
    SI_INV_KIN_COPY = 0x084adab0, "SI_invKinCopy";
    /// SI_invKinDispose (0x084ada60), `invkin.c`: (&record).
    SI_INV_KIN_DISPOSE = 0x084ada60, "SI_invKinDispose";
    /// SI_dynamicChainsUpdate (0x081cc1cc), `dynadial.c`: ().
    SI_DYNAMIC_CHAINS_UPDATE = 0x081cc1cc, "SI_dynamicChainsUpdate";

    // ---- envelopes, lattices, metaballs, texture coordinates
    /// SI_envelopRebuild (0x084bd2a0), `envtools.c`: (model, flag, flag).
    SI_ENVELOP_REBUILD = 0x084bd2a0, "SI_envelopRebuild";
    /// SI_modelLattice (0x0843ff1c), `lattice.c`: (model, &16-bit index).
    SI_MODEL_LATTICE = 0x0843ff1c, "SI_modelLattice";
    /// SI_modelTreeLattice (0x0843ff88), `lattice.c`: (model).
    SI_MODEL_TREE_LATTICE = 0x0843ff88, "SI_modelTreeLattice";
    /// SI_modelRemoveLattice (0x08153714), `deformation.c`: (model, flag, flag).
    SI_MODEL_REMOVE_LATTICE = 0x08153714, "SI_modelRemoveLattice";
    /// SI_latticeCopyFromSetup (0x0843de60), `lattice.c`: (from, to).
    SI_LATTICE_COPY_FROM_SETUP = 0x0843de60, "SI_latticeCopyFromSetup";
    /// SI_metaGroupCopy (0x0845c078), `metaball.c`: (to, from).
    SI_META_GROUP_COPY = 0x0845c078, "SI_metaGroupCopy";
    /// SI_initCopyMultiUV (0x08413b14), `menuDuplicate.c`: ().
    SI_INIT_COPY_MULTI_UV = 0x08413b14, "SI_initCopyMultiUV";
    /// SI_copyMultUVHierarchy (0x08413aa0), `menuDuplicate.c`: (from, to).
    SI_COPY_MULT_UV_HIERARCHY = 0x08413aa0, "SI_copyMultUVHierarchy";

    // ---- materials and textures
    /// SI_materialCopy (0x08284888), `mat_dial.c`: (material); answers the copy.
    SI_MATERIAL_COPY = 0x08284888, "SI_materialCopy";
    /// SI_materialTransfer (0x08284504), `mat_dial.c`: (from, to, flag).
    SI_MATERIAL_TRANSFER = 0x08284504, "SI_materialTransfer";
    /// SI_copyMaterialShaders (0x082a6808), `shd_fct.c`: (to, from).
    SI_COPY_MATERIAL_SHADERS = 0x082a6808, "SI_copyMaterialShaders";
    /// SI_deleteMaterialShaders (0x082a69dc), `shd_fct.c`: (material).
    SI_DELETE_MATERIAL_SHADERS = 0x082a69dc, "SI_deleteMaterialShaders";
    /// SI_connectMaterialShaders (0x082a6ac0), `shd_fct.c`: (material).
    SI_CONNECT_MATERIAL_SHADERS = 0x082a6ac0, "SI_connectMaterialShaders";
    /// SI_txtCopyFromSetup (0x082954c0), `matter.c`: (from, to, flag, flag).
    SI_TXT_COPY_FROM_SETUP = 0x082954c0, "SI_txtCopyFromSetup";
}

/// The name of a callee by its address.
pub fn name_of(f: FnPtr) -> Option<&'static str> {
    CALLEES.iter().find(|c| c.0 == f.0).map(|c| c.1)
}
