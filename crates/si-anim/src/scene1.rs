// SPDX-License-Identifier: MIT
//! `scene.c`, first slice (finding 0030, section 10): the scene-level animation window's
//! entry points of the original, in address order: the constraint pickers, the channel menu,
//! the scene's set-up and refresh, and the function-curve table menus.
//!
//! The original is a screen program: its routines draw, show messages, wait for events and
//! ask the user. They are reached here through [`Outside`] by name and address (the hooks
//! answer 0 until their own source files are ported). Where the original sleeps
//! (`usleep`, `sleep`) nothing in memory changes and nothing is done. Where it branches on
//! `SI_lightLicense` (the light product's flag) the full product's path is taken, as
//! AGENTS.md says.
//!
//! Record layouts are addressed by named offsets on the one image (not established as
//! `record!` layouts): a graph-list node (`GN_*`), an event (`EV_*`) and the scene's graph
//! lists (`SI_grafLists`: 0x24 bytes per list, the current one the word at +4 of
//! `SI_procData`).

use crate::outside::{self as out, Outside, Routine};
use si_mem::Mem;

/// A routine of another source file this slice calls: name and address, as in
/// [`crate::outside`].
macro_rules! routines {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine this module names.
        pub const ROUTINES: &[&Routine] = &[ $( &$konst ),* ];
    };
}

routines! {
    /// `SI_fcvScnglClear (0x0823c948)`: clears the selected function curves of the scene.
    SI_FCV_SCNGL_CLEAR = "SI_fcvScnglClear", 0x0823c948;
    /// `CheckForAllowedFCurves (0x0823d9a0)`: whether the light product may animate an item's curves.
    CHECK_FOR_ALLOWED_FCURVES = "CheckForAllowedFCurves", 0x0823d9a0;
    /// `SI_elemdictCopyElemAnim (0x0822bd8c)`: copies the animation of an element to another.
    SI_ELEMDICT_COPY_ELEM_ANIM = "SI_elemdictCopyElemAnim", 0x0822bd8c;
    /// `si_elemdictSelectElemAnim (0x0822bad8)`: selects the animation of an element; the count it selected.
    SI_ELEMDICT_SELECT_ELEM_ANIM = "si_elemdictSelectElemAnim", 0x0822bad8;
    /// `SI_elemdictResetElemAnim (0x08220f08)`: resets the animation of an element.
    SI_ELEMDICT_RESET_ELEM_ANIM = "SI_elemdictResetElemAnim", 0x08220f08;
    /// `SI_fcvGetRelInfo (0x0810f698)`: the relation and the second word of a function-curve attribute.
    SI_FCV_GET_REL_INFO = "SI_fcvGetRelInfo", 0x0810f698;
    /// `SI_attdictGetFcurveFlag (0x0811ff64)`: whether an attribute of a dictionary is a function curve.
    SI_ATTDICT_GET_FCURVE_FLAG = "SI_attdictGetFcurveFlag", 0x0811ff64;
    /// `SI_reldictGetRelName (0x0813a0f4)`: the name of a relation of a dictionary.
    SI_RELDICT_GET_REL_NAME = "SI_reldictGetRelName", 0x0813a0f4;
    /// `SI_windowSetFcurveActive (0x083d8c70)`: makes the function-curve window (in)active.
    SI_WINDOW_SET_FCURVE_ACTIVE = "SI_windowSetFcurveActive", 0x083d8c70;
    /// `SI_refreshView (0x08325e14)`: redraws the views.
    SI_REFRESH_VIEW = "SI_refreshView", 0x08325e14;
    /// `SI_fcvTableSet (0x0822b3c4)`: sets the function-curve table to a mode (animation tables, not ported yet).
    SI_FCV_TABLE_SET = "SI_fcvTableSet", 0x0822b3c4;
    /// `SI_attdictFlagAllMaterialFcurves (0x080fd2bc)`: flags the function curves of every material attribute.
    SI_ATTDICT_FLAG_ALL_MATERIAL_FCURVES = "SI_attdictFlagAllMaterialFcurves", 0x080fd2bc;
    /// `SI_attdictSetFcurveFlag (0x0811ff18)`: sets the function-curve flag of an attribute of a dictionary.
    SI_ATTDICT_SET_FCURVE_FLAG = "SI_attdictSetFcurveFlag", 0x0811ff18;
    /// `SI_edGetElemFromMaterial (0x080fd288)`: the element of a material.
    SI_ED_GET_ELEM_FROM_MATERIAL = "SI_edGetElemFromMaterial", 0x080fd288;
    /// `SI_edGetElemFromTexture2D (0x080fd3e0)`: the element of a 2D texture.
    SI_ED_GET_ELEM_FROM_TEXTURE2D = "SI_edGetElemFromTexture2D", 0x080fd3e0;
    /// `SI_edGetElemFromTexture3D (0x080fd414)`: the element of a 3D texture.
    SI_ED_GET_ELEM_FROM_TEXTURE3D = "SI_edGetElemFromTexture3D", 0x080fd414;
    /// `SI_edGetElemFromScene (0x080fd620)`: the scene's element.
    SI_ED_GET_ELEM_FROM_SCENE = "SI_edGetElemFromScene", 0x080fd620;
    /// `SI_edGetElemFromLattice (0x080fd47c)`: the element of a lattice's side.
    SI_ED_GET_ELEM_FROM_LATTICE = "SI_edGetElemFromLattice", 0x080fd47c;
    /// `SI_menuStateSet (0x083a3d34)`: sets one state of a menu item (six words).
    SI_MENU_STATE_SET = "SI_menuStateSet", 0x083a3d34;
    /// `SI_getChnMenuState (0x08568944)`: stores the channel menu's four states through its four addresses.
    SI_GET_CHN_MENU_STATE = "SI_getChnMenuState", 0x08568944;
    /// `SI_setChnMenuState (0x08568998)`: takes the channel menu's four states.
    SI_SET_CHN_MENU_STATE = "SI_setChnMenuState", 0x08568998;
    /// `SI_buttonKeepHilite (0x0837fa1c)`: keeps a button highlighted.
    SI_BUTTON_KEEP_HILITE = "SI_buttonKeepHilite", 0x0837fa1c;
    /// `SI_buttonOff (0x0837fa74)`: turns a button off.
    SI_BUTTON_OFF = "SI_buttonOff", 0x0837fa74;
    /// `SI_initShowFlag (0x084176b0)`: sets up the show flags.
    SI_INIT_SHOW_FLAG = "SI_initShowFlag", 0x084176b0;
    /// `SI_setInterpDot (0x0823caa0)`: sets the interpolation dot for an item (scene.c, second slice).
    SI_SET_INTERP_DOT = "SI_setInterpDot", 0x0823caa0;
    /// `SI_motionDirectionFlagSet (0x0823d974)`: sets the direction flag (scene.c, second slice).
    SI_MOTION_DIRECTION_FLAG_SET = "SI_motionDirectionFlagSet", 0x0823d974;
    /// `SI_graflistTypeCount (0x0832dc74)`: counts the list's nodes of a type range and stores the first.
    SI_GRAFLIST_TYPE_COUNT = "SI_graflistTypeCount", 0x0832dc74;
    /// `SI_elemdictFindElemFromGrafNode (0x080fcbec)`: the element of a graph node (stores chapter and index).
    SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE = "SI_elemdictFindElemFromGrafNode", 0x080fcbec;
    /// `SI_itemRelaxConstraint (0x081f533c)`: removes the constraints of an item.
    SI_ITEM_RELAX_CONSTRAINT = "SI_itemRelaxConstraint", 0x081f533c;
    /// `SI_sceneRefresh (0x0820f470)`: redraws the scene.
    SI_SCENE_REFRESH = "SI_sceneRefresh", 0x0820f470;
    /// `SI_getEvent (0x08394058)`: the next event of the queue.
    SI_GET_EVENT = "SI_getEvent", 0x08394058;
    /// `SI_refreshSelectedBothBuffers (0x08325d70)`: redraws the selection in both buffers.
    SI_REFRESH_SELECTED_BOTH_BUFFERS = "SI_refreshSelectedBothBuffers", 0x08325d70;
    /// `SI_gLSelectCheckNSE (0x0832ef28)`: the graph node under the pointer of a window.
    SI_GL_SELECT_CHECK_NSE = "SI_gLSelectCheckNSE", 0x0832ef28;
    /// `SI_gLSelect (0x0832ef0c)`: the graph node picked in a window.
    SI_GL_SELECT = "SI_gLSelect", 0x0832ef0c;
    /// `SI_edElmSetPntsConstr (0x080ffeb8)`: sets an element's 2- or 3-point constraint.
    SI_ED_ELM_SET_PNTS_CONSTR = "SI_edElmSetPntsConstr", 0x080ffeb8;
    /// `si_constr3PntsFormFace (0x081f3c64)`: the face for a 3-point constraint.
    SI_CONSTR_3PNTS_FORM_FACE = "si_constr3PntsFormFace", 0x081f3c64;
    /// `SI_foolDisplay (0x08323a70)`: draws a graph node as picked.
    SI_FOOL_DISPLAY = "SI_foolDisplay", 0x08323a70;
    /// `swapbuffers (0x08611804)`: swaps the display's buffers.
    SWAPBUFFERS = "swapbuffers", 0x08611804;
    /// `SI_refreshSystem (0x08323d44)`: refreshes a part of the display.
    SI_REFRESH_SYSTEM = "SI_refreshSystem", 0x08323d44;
    /// `SI_itemSetConstraint (0x081f4fec)`: sets a constraint between two items (a short result).
    SI_ITEM_SET_CONSTRAINT = "SI_itemSetConstraint", 0x081f4fec;
    /// `SI_refreshAllControls (0x0837fc60)`: refreshes every control.
    SI_REFRESH_ALL_CONTROLS = "SI_refreshAllControls", 0x0837fc60;
    /// `SI_elemdictFindSerialRelation (0x0813335c)`: the serial relation of an element.
    SI_ELEMDICT_FIND_SERIAL_RELATION = "SI_elemdictFindSerialRelation", 0x0813335c;
    /// `SI_flashObject (0x0823c9d8)`: flashes a graph node (scene.c, a later slice).
    SI_FLASH_OBJECT = "SI_flashObject", 0x0823c9d8;
    /// `SI_copyElemExpression (0x082493fc)`: copies an element's expressions to another (a short result).
    SI_COPY_ELEM_EXPRESSION = "SI_copyElemExpression", 0x082493fc;
    /// `SI_graflistCountSelected (0x0832db90)`: counts the list's selected nodes and stores the first.
    SI_GRAFLIST_COUNT_SELECTED = "SI_graflistCountSelected", 0x0832db90;
    /// `si_replaceAnyTranslationByExplPath (0x08245584)`.
    SI_REPLACE_ANY_TRANSLATION_BY_EXPL_PATH = "si_replaceAnyTranslationByExplPath", 0x08245584;
    /// `SI_elemdictRemoveRelationPair (0x0813362c)`.
    SI_ELEMDICT_REMOVE_RELATION_PAIR = "SI_elemdictRemoveRelationPair", 0x0813362c;
    /// `si_itemSetPathTiming (0x0821e790)`.
    SI_ITEM_SET_PATH_TIMING = "si_itemSetPathTiming", 0x0821e790;
    /// `SI_elemdictCheckCycle (0x08140d80)`: a short result.
    SI_ELEMDICT_CHECK_CYCLE = "SI_elemdictCheckCycle", 0x08140d80;
    /// `si_itemResetPath (0x0821f1bc)`.
    SI_ITEM_RESET_PATH = "si_itemResetPath", 0x0821f1bc;
    /// `SI_elemdictRemoveRelationId (0x08133998)`.
    SI_ELEMDICT_REMOVE_RELATION_ID = "SI_elemdictRemoveRelationId", 0x08133998;
    /// `SI_schemSetDefaultColor (0x083516d0)`.
    SI_SCHEM_SET_DEFAULT_COLOR = "SI_schemSetDefaultColor", 0x083516d0;
    /// `SI_getMainEvent (0x0839406c)`: the main event (waits for it).
    SI_GET_MAIN_EVENT = "SI_getMainEvent", 0x0839406c;
    /// `SI_initWave (0x084c5c70)`.
    SI_INIT_WAVE = "SI_initWave", 0x084c5c70;
    /// `SI_initParticle (0x084c0450)`.
    SI_INIT_PARTICLE = "SI_initParticle", 0x084c0450;
    /// `SI_needInter (0x0817e06c)`: a short result.
    SI_NEED_INTER = "SI_needInter", 0x0817e06c;
    /// `SI_graflistSetMultiSelection (0x0832dd70)`.
    SI_GRAFLIST_SET_MULTI_SELECTION = "SI_graflistSetMultiSelection", 0x0832dd70;
    /// `scnWind (0x08235bfc)`: the scene window's event handler (not ported yet).
    SCN_WIND = "scnWind", 0x08235bfc;
    /// `scnMenu (0x0823701c)`: scene.c, a later slice.
    SCN_MENU = "scnMenu", 0x0823701c;
    /// `scnButton (0x0823c4ac)`: scene.c, a later slice.
    SCN_BUTTON = "scnButton", 0x0823c4ac;
    /// `SI_fastKey (0x08403d80)`: a short result.
    SI_FAST_KEY = "SI_fastKey", 0x08403d80;
    /// `SI_getNbWindows (0x083d9340)`.
    SI_GET_NB_WINDOWS = "SI_getNbWindows", 0x083d9340;
    /// `SI_sceneStepFrame (0x0820f2d8)`.
    SI_SCENE_STEP_FRAME = "SI_sceneStepFrame", 0x0820f2d8;
    /// `scnSetMode (0x0823d1f8)`: scene.c, a later slice.
    SCN_SET_MODE = "scnSetMode", 0x0823d1f8;
    /// `SI_displayMessages (0x0839ef90)`.
    SI_DISPLAY_MESSAGES = "SI_displayMessages", 0x0839ef90;
    /// `SI_setFilterTrackTable (0x081dd71c)`.
    SI_SET_FILTER_TRACK_TABLE = "SI_setFilterTrackTable", 0x081dd71c;
}

si_mem::globals! {
    /// scene.c: which function-curve refresh option is set (0 to 2).
    SI_REFRESH_ELEM_FCV_OPTION: u16 = 0x08695e8c, "SI_refreshElemFcvOption", init 0;
    /// The scene's graph lists (0x24 bytes each; the graph-list code owns it).
    SI_GRAF_LISTS: u32 = 0x08679498, "SI_grafLists", init 0;
    /// The channel menu's state: capture.
    CAPTURE_MODE: i16 = 0x0878fb4e, "captureMode";
    /// The channel menu's state: control.
    CONTROL_MODE: i16 = 0x0878fb50, "controlMode";
    /// The channel menu's state: display refresh rate.
    DISPLAY_REFRESH_RATE: i16 = 0x0878fb52, "displayRefreshRate";
    /// The channel menu's state: supra keys.
    SUPRA_KEYS: i16 = 0x0878fb54, "supraKeys";
    /// The cursor settings (si-app owns them).
    SI_BUSY_CURSOR: i16 = 0x086792b2, "SI_busyCursor", init 1;
    /// Hierarchy compensation flag.
    SI_HRCH_CPST: i16 = 0x0867949c, "SI_hrchCpst", init 1;
    /// Right-handed flag.
    SI_RIGHT_HAND: i16 = 0x086793ec, "SI_rightHand", init 1;
    /// Zoom-on-cursor flag.
    SI_ZOOM_ON_CURSOR: i16 = 0x0867945c, "SI_zoomONCursor", init 0;
    /// Direction display flag.
    SI_SHW_DIRCNS: i16 = 0x08679518, "SI_shwDircns", init 1;
    /// The global manipulation settings (0x3c bytes): the 16-bit words at +0x1e, +0x20, +0x22
    /// and +0x24 are menu states, the word at +0 a mode.
    SI_GLB_MANIP: [u16; 30] = 0x08781560, "SI_glbManip";
    /// The update option.
    SI_UPDATE_OPTION: i32 = 0x0878fb24, "SI_updateOption";
    /// The window mode the 2- and 3-point pickers put on the nodes they pick.
    SI_SELECT2_WM: u16 = 0x0878162e, "SI_select2WM";
    /// The scene window's mode (0x2a, 0x2f: the path pickers).
    SCNMOD: i32 = 0x08697d88, "scnmod";
    /// The supra mode, passed to `SI_displayMessages`.
    SI_SUPRA_MODE: u32 = 0x08679404, "SI_supraMode";
    /// Product-tier flag: the light product (full product: 0).
    SI_LIGHT_LICENSE: u16 = 0x08679320, "SI_lightLicense";
    /// Product-tier flag: option -B, the base product (full product: 0).
    SI_BASE_LICENSE: u16 = 0x08781464, "SI_baseLicense";
    /// The branch-selected-curve flag.
    BRANCH_SELECTED_FCV: i32 = 0x08697d80, "branchSelectedFcv";
    /// Clear-selected-curve flag.
    SI_CLEAR_SELECTED_FCV: i16 = 0x08695e96, "SI_clearSelectedFcv", init 1;
    /// Slope drawing flag.
    SI_SLOPE_DRAW: i16 = 0x08695e88, "si_slopeDraw", init 1;
    /// Slope picking flag.
    SI_SLOPE_PICK: i16 = 0x08695e8a, "si_slopePick", init 1;
    /// Show-curve-groups flag.
    SI_SHOW_FCV_GRPS: i16 = 0x08695e98, "SI_showFcvGrps", init 1;
    /// The library track filter.
    LIB_TRACK_FILTER: i16 = 0x08697d8c, "libTrackFilter", init 1;
}

/// Writes the initial values of this module's globals.
pub fn init(m: &mut Mem) {
    si_mem::init_globals(m, ALL);
}

fn call(m: &mut Mem, o: &mut dyn Outside, r: &'static Routine, args: &[u32]) -> u32 {
    o.call(m, r, args)
}

/// The current graph list: `SI_grafLists + SI_procData->list × 0x24`.
fn graflist(m: &Mem) -> u32 {
    let proc = m[crate::globals::SI_PROC_DATA];
    m[SI_GRAF_LISTS].wrapping_add(m.u32(proc.wrapping_add(4)).wrapping_mul(0x24))
}

/// `SI_menuStateSet(1, a, b, item, 0, value)`.
fn menu_state(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, item: u32, value: u32) {
    call(m, o, &SI_MENU_STATE_SET, &[1, a, b, item, 0, value]);
}

/// [soft 0x0822f120 motionMultiMessage]
///
/// The request dialog that says the button does not work with many items selected, with its
/// button (four texts of the binary, at the addresses below).
pub fn motion_multi_message(m: &mut Mem, o: &mut dyn Outside) {
    call(m, o, &out::SI_DIALOG_REQUEST, &[0x08697de0, 0x08697da9, 0x08697d91, 0x08697d8e]);
}

/// [soft 0x0822f144 si_MultiFcvMessage]
///
/// The same dialog for function curves (its four texts are at the addresses below).
pub fn si_multi_fcv_message(m: &mut Mem, o: &mut dyn Outside) {
    call(m, o, &out::SI_DIALOG_REQUEST, &[0x08697e40, 0x08697e1a, 0x08697e02, 0x08697dff]);
}

/// [soft 0x082312b8 releaseScene]
///
/// Does nothing.
pub fn release_scene(_m: &mut Mem) {}

/// [soft 0x0823063c SI_motionSetFcvUpdOpt]
///
/// Sets the three radio states of the refresh option menu (0x2bc, 0x355): item *i* is on
/// when `SI_refreshElemFcvOption` is *i*.
pub fn motion_set_fcv_upd_opt(m: &mut Mem, o: &mut dyn Outside) {
    for i in 0..3u32 {
        let on = (m[SI_REFRESH_ELEM_FCV_OPTION] as u32 == i) as u32;
        menu_state(m, o, 0x2bc, 0x355, i, on);
    }
}

/// [soft 0x082312c0 SI_motionRefreshMulti]
///
/// Of the buttons 10 and 20 one stays highlighted and the other goes off: with the current
/// graph list's first word not 0 it is 20 that stays and 10 that goes off, else the other
/// way round.
pub fn motion_refresh_multi(m: &mut Mem, o: &mut dyn Outside) {
    let list = graflist(m);
    if m.u16(list) != 0 {
        call(m, o, &SI_BUTTON_KEEP_HILITE, &[0x14]);
        call(m, o, &SI_BUTTON_OFF, &[0xa]);
    } else {
        call(m, o, &SI_BUTTON_KEEP_HILITE, &[0xa]);
        call(m, o, &SI_BUTTON_OFF, &[0x14]);
    }
}

/// [soft 0x08231860 okGrafType]
///
/// Whether a graph-list node's type is allowed for a function-curve table of a graph type
/// (0 to 11): types 2 and 3 for graph type 2, 4 and 5 for graph type 4, types 6 to 8, 19 and
/// 20 for graph type 6, type 11 for graph type 11; graph type 0 admits every node, the other
/// graph types none (so does a graph type above 11). (A word, 0 or 1.)
pub fn ok_graf_type(m: &Mem, node: u32, graf_type: u32) -> u32 {
    let t = m.u32(node);
    match graf_type {
        2 => (t.wrapping_sub(2) <= 1) as u32,
        4 => (t.wrapping_sub(4) <= 1) as u32,
        6 => (t.wrapping_sub(6) <= 2 || t == 0x13 || t == 0x14) as u32,
        11 => (t == 0xb) as u32,
        0 => 1,
        _ => 0,
    }
}

/// [soft 0x0823130c SI_channelMenuInit]
///
/// Reads the channel menu's four states (`SI_getChnMenuState` stores them through the
/// addresses of `captureMode`, `controlMode`, `displayRefreshRate` and `supraKeys`) and
/// shows them in the two menus 0x196e/0x1970 (items 3 to 6) and 0x196e/0x1977 (items 3 to
/// 5, the capture state left out).
pub fn channel_menu_init(m: &mut Mem, o: &mut dyn Outside) {
    call(m, o, &SI_GET_CHN_MENU_STATE, &[CAPTURE_MODE.addr(), CONTROL_MODE.addr(), DISPLAY_REFRESH_RATE.addr(), SUPRA_KEYS.addr()]);
    let (cap, ctl, disp, supra) = (m[CAPTURE_MODE] as i32 as u32, m[CONTROL_MODE] as i32 as u32, m[DISPLAY_REFRESH_RATE] as i32 as u32, m[SUPRA_KEYS] as i32 as u32);
    menu_state(m, o, 0x196e, 0x1970, 3, cap);
    menu_state(m, o, 0x196e, 0x1970, 4, ctl);
    menu_state(m, o, 0x196e, 0x1970, 5, disp);
    menu_state(m, o, 0x196e, 0x1970, 6, supra);
    let ctl = m[CONTROL_MODE] as i32 as u32;
    menu_state(m, o, 0x196e, 0x1977, 3, ctl);
    let disp = m[DISPLAY_REFRESH_RATE] as i32 as u32;
    menu_state(m, o, 0x196e, 0x1977, 4, disp);
    let supra = m[SUPRA_KEYS] as i32 as u32;
    menu_state(m, o, 0x196e, 0x1977, 5, supra);
}

/// [soft 0x08231424 SI_channelMenuSet]
///
/// A click on item `item` of the channel menu `menu` (0x1970 or 0x1977): toggles the state
/// it stands for. Capture (0x1970 item 3) may not be turned off while control is off and
/// control (0x1970 item 4, 0x1977 item 3) may not be turned off while capture is off:
/// the status line says so (texts 0x086985e0 / 0x08698620, level 2) and nothing else
/// happens. The refresh rate and supra keys flip their low byte. Then the four states go
/// to `SI_setChnMenuState` and the menu is shown again as in [`channel_menu_init`] (the one
/// that was clicked).
pub fn channel_menu_set(m: &mut Mem, o: &mut dyn Outside, menu: i32, item: i32) {
    let (menu, item) = (menu as u32, item as u32);
    // SI_lightLicense: the full product's path
    let mut toggled = false;
    if menu == 0x1970 && item == 3 {
        let cap = m[CAPTURE_MODE] as u16;
        if cap == 1 && m[CONTROL_MODE] == 0 {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x086985e0, 2]);
            return;
        }
        m[CAPTURE_MODE] = (cap ^ 1) as i16;
        toggled = true;
    }
    if !toggled && ((menu == 0x1970 && item == 4) || (menu == 0x1977 && item == 3)) {
        let ctl = m[CONTROL_MODE] as u16;
        if m[CAPTURE_MODE] == 0 && ctl == 1 {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08698620, 2]);
            return;
        }
        m[CONTROL_MODE] = (ctl ^ 1) as i16;
        toggled = true;
    }
    if !toggled && ((menu == 0x1970 && item == 5) || (menu == 0x1977 && item == 4)) {
        let v = m.u8(DISPLAY_REFRESH_RATE.addr());
        m.set_u8(DISPLAY_REFRESH_RATE.addr(), v ^ 1);
        toggled = true;
    }
    if !toggled && ((menu == 0x1970 && item == 6) || (menu == 0x1977 && item == 5)) {
        let v = m.u8(SUPRA_KEYS.addr());
        m.set_u8(SUPRA_KEYS.addr(), v ^ 1);
    }
    let (cap, ctl, disp, supra) = (m[CAPTURE_MODE] as i32 as u32, m[CONTROL_MODE] as i32 as u32, m[DISPLAY_REFRESH_RATE] as i32 as u32, m[SUPRA_KEYS] as i32 as u32);
    call(m, o, &SI_SET_CHN_MENU_STATE, &[cap, ctl, disp, supra]);
    if menu == 0x1970 {
        for (i, v) in [(3, cap), (4, ctl), (5, disp), (6, supra)] {
            menu_state(m, o, 0x196e, menu, i, v);
        }
    } else {
        for (i, v) in [(3, ctl), (4, disp), (5, supra)] {
            menu_state(m, o, 0x196e, 0x1977, i, v);
        }
    }
}

/// [soft 0x08231630 refreshScene]
///
/// Brings the scene window's menus up to date: `SI_motionRefreshMulti`, `SI_initShowFlag(1)`,
/// the interpolation dot of the current list's selected node when the list's first word is 0
/// and the node is a model or light (types 2, 3: its word at +4; else 0), then the states of
/// the menu 0x1fe/0xa0 (items 7, 0, 6, 8 from the cursor and hierarchy flags, 1, 2, 0xb, 3
/// from `SI_glbManip`'s words, 5 on its mode being 1), the direction flag, and the three
/// states of the update option menu (0x2e0).
pub fn refresh_scene(m: &mut Mem, o: &mut dyn Outside) {
    motion_refresh_multi(m, o);
    call(m, o, &SI_INIT_SHOW_FLAG, &[1]);
    let list = graflist(m);
    let mut arg = 0;
    if m.u16(list) == 0 {
        let node = m.u32(list.wrapping_add(8));
        if node != 0 && m.u32(node).wrapping_sub(2) <= 1 {
            arg = m.u32(node.wrapping_add(4));
        }
    }
    call(m, o, &SI_SET_INTERP_DOT, &[arg]);
    let w = |m: &Mem, off: u32| m.i16(SI_GLB_MANIP.addr() + off) as i32 as u32;
    let v = m[SI_BUSY_CURSOR] as i32 as u32;
    menu_state(m, o, 0x1fe, 0xa0, 7, v);
    let v = m[SI_HRCH_CPST] as i32 as u32;
    menu_state(m, o, 0x1fe, 0xa0, 0, v);
    let v = m[SI_RIGHT_HAND] as i32 as u32;
    menu_state(m, o, 0x1fe, 0xa0, 6, v);
    let v = m[SI_ZOOM_ON_CURSOR] as i32 as u32;
    menu_state(m, o, 0x1fe, 0xa0, 8, v);
    let v = w(m, 0x1e);
    menu_state(m, o, 0x1fe, 0xa0, 1, v);
    let v = w(m, 0x20);
    menu_state(m, o, 0x1fe, 0xa0, 2, v);
    let v = w(m, 0x22);
    menu_state(m, o, 0x1fe, 0xa0, 0xb, v);
    let v = (m.i32(SI_GLB_MANIP.addr()) == 1) as u32;
    menu_state(m, o, 0x1fe, 0xa0, 5, v);
    let v = w(m, 0x24);
    menu_state(m, o, 0x1fe, 0xa0, 3, v);
    let v = m[SI_SHW_DIRCNS] as i32 as u32;
    call(m, o, &SI_MOTION_DIRECTION_FLAG_SET, &[v]);
    for (item, val) in [(0u32, 1i32), (2, 0), (1, 2)] {
        let v = (m[SI_UPDATE_OPTION] == val) as u32;
        menu_state(m, o, 0x2e0, 0x2e0, item, v);
    }
}

/// The constraint relations `nbConstraintsOnElem` counts (the binary's table of 17 pairs, 136
/// bytes at 0x08698280, read from the user's install): the relation type, then the second
/// selector of the dictionary look-up.
static CONSTRAINT_GROUPS_TABLE: si_rt::Table = si_rt::Table::new("si_anim.constraint_groups", 0x08698280, 0x88, 0x969e_1878_a51f_1edb);
static CONSTRAINT_GROUPS: std::sync::LazyLock<[(u32, u32); 17]> = std::sync::LazyLock::new(|| {
    let b = CONSTRAINT_GROUPS_TABLE.get();
    let w = |i: usize| u32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
    std::array::from_fn(|k| (w(2 * k), w(2 * k + 1)))
});

/// [soft 0x0822fcb4 nbConstraintsOnElem]
///
/// The number of constraints on an element. For graph type 2 or 3 it counts, for each of the 17
/// constraint relations, the first related element and (except for the single-valued
/// relations 0x7c, 0x77, 0x7e and 0x7f) every next one, skipping those of the chapter
/// `SI_cid[2]` for the two relations 0xf6 and 0xf7. For other graph types it counts the
/// elements of the constraint relation of the graph type and one more if the element has a
/// relation of type 0x45d.
pub fn nb_constraints_on_elem(m: &mut Mem, o: &mut dyn Outside, elem: u32, graf_type: u32) -> u32 {
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    let mut n = 0u32;
    let find = |m: &mut Mem, o: &mut dyn Outside, next: bool, b: u32, a: u32| {
        let args = [crate::globals::cur_dict(m), elem, 1, b, a, u32::MAX, l8, l4];
        let r = if next { &out::SI_ELEMDICT_FIND_NEXT_RELATED_ELEM } else { &out::SI_ELEMDICT_FIND_RELATED_ELEM };
        o.call(m, r, &args)
    };
    if graf_type.wrapping_sub(2) <= 1 {
        for &(a, b) in CONSTRAINT_GROUPS.iter() {
            if find(m, o, false, b, a) == 0 {
                continue;
            }
            n += 1;
            if matches!(a, 0x7e | 0x7c | 0x7f | 0x77) {
                continue;
            }
            loop {
                if find(m, o, true, b, a) == 0 {
                    break;
                }
                if !(a.wrapping_sub(0xf6) <= 1 && m.u32(l8) == m[crate::globals::SI_CID][2] as u32) {
                    n += 1;
                }
            }
        }
    } else {
        let rel = call(m, o, &out::SI_REL_GET_CONS_REL_FROM_GRAF_TYPE, &[graf_type]);
        if find(m, o, false, 1, rel) != 0 {
            n = 1;
            while find(m, o, true, 1, rel) != 0 {
                n += 1;
            }
        }
        let args = [crate::globals::cur_dict(m), elem, 1, 1, 0x45d, u32::MAX, l8, l4];
        if o.call(m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &args) != 0 {
            n += 1;
        }
    }
    m.set_sp(sp);
    n
}

/// Whether a graph node takes part in the constraint commands: its word at +8 is not 0 or one
/// of its flag bits 0xe0 (byte +0x17) is set.
fn node_active(m: &Mem, n: u32) -> bool {
    m.u32(n.wrapping_add(8)) != 0 || m.u8(n.wrapping_add(0x17)) & 0xe0 != 0
}

/// [soft 0x0822fe88 SI_motionRelaxConstraint]
///
/// Without a selected item the status line says to select a constrained item first (text 0x08698320, level 3).
/// If any active node of the current list has more than one constraint (see
/// [`nb_constraints_on_elem`]) the command stops with 1; otherwise every active node is relaxed
/// and the scene redrawn. (The result is 0 or 1.)
pub fn motion_relax_constraint(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let sp = m.sp();
    let (node, l8, lc) = (m.local(4), m.local(4), m.local(4));
    let list = graflist(m);
    let ret = 'done: {
        if call(m, o, &SI_GRAFLIST_TYPE_COUNT, &[list, 0, 0, node, 1]) == 0 {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08698320, 3]);
            break 'done 0;
        }
        let mut n = m.u32(graflist(m).wrapping_add(4));
        while n != 0 {
            if node_active(m, n) {
                let e = call(m, o, &SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[n, lc, l8]);
                let t = m.u32(n);
                if (nb_constraints_on_elem(m, o, e, t) as i32) > 1 {
                    break 'done 1;
                }
            }
            n = m.u32(n.wrapping_add(0x1c));
        }
        let mut n = m.u32(graflist(m).wrapping_add(4));
        while n != 0 {
            if node_active(m, n) {
                let (d, t) = (m.u32(n.wrapping_add(4)), m.u32(n));
                call(m, o, &SI_ITEM_RELAX_CONSTRAINT, &[d, t]);
            }
            n = m.u32(n.wrapping_add(0x1c));
        }
        call(m, o, &SI_SCENE_REFRESH, &[]);
        0
    };
    m.set_sp(sp);
    ret
}

// ---------------------------------------------------------------------------------------------
// The pickers (second part of the first slice): event loops that wait for the user to click on
// graph nodes. The event is a record of the event queue: its type (word at +0; 5 is a click or
// key), its code (half word at +6) and its window (+0xc); the window's mode is the half word at
// the pointer at +0x2c, +4 (0x7d4 and 0x7d8 end the loop at once).

/// The address `SI_RD` (the dictionary of the relations of the path animation): passed by address.
const SI_RD: u32 = 0x087815f0;

fn status(m: &mut Mem, o: &mut dyn Outside, text: u32, level: u32) {
    call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[text, level]);
}

/// `sprintf` of a format whose only conversions are `%s`, taking the strings in order (no other
/// conversion occurs in the formats this file hands it).
fn fmt_s(fmt: &[u8], args: &[&[u8]]) -> Vec<u8> {
    let (mut out, mut i, mut k) = (Vec::new(), 0, 0);
    while i < fmt.len() {
        if fmt[i] == b'%' && fmt.get(i + 1) == Some(&b's') {
            out.extend_from_slice(args[k]);
            k += 1;
            i += 2;
        } else {
            out.push(fmt[i]);
            i += 1;
        }
    }
    out
}

/// A model node's data tested for "chain" (a bone chain has no 2-, 3-point, tangent or
/// normal constraint): the word at +0x30 is 5, or it is 0 and the word at +0x2ec is 1 to 4.
fn is_chain(m: &Mem, d: u32) -> bool {
    let k = m.u32(d.wrapping_add(0x30));
    k == 5 || (k == 0 && m.u32(d.wrapping_add(0x2ec)).wrapping_sub(1) <= 1) || (k == 0 && m.u32(d.wrapping_add(0x2ec)).wrapping_sub(3) <= 1)
}

/// The picked node's half word at +0x12 and word at +8, as `(w12, w8)`, which the pickers put
/// their own values in while the node counts as picked and put back afterwards.
fn pick(m: &mut Mem, node: u32) -> (u16, u32) {
    let saved = (m.u16(node.wrapping_add(0x12)), m.u32(node.wrapping_add(8)));
    m.set_u16(node.wrapping_add(0x12), m[SI_SELECT2_WM]);
    m.set_u32(node.wrapping_add(8), 1);
    saved
}

fn unpick(m: &mut Mem, node: u32, saved: (u16, u32)) {
    m.set_u16(node.wrapping_add(0x12), saved.0);
    m.set_u32(node.wrapping_add(8), saved.1);
}

/// "Picked object #N" on the status line (level 2), N in decimal in a local buffer.
fn picked_message(m: &mut Mem, o: &mut dyn Outside, fmt: u32, buf: u32, n: u32) {
    let mut text = m.cstr(fmt);
    let at = text.windows(2).position(|w| w == b"%d").unwrap_or(text.len());
    text.truncate(at);
    text.extend(si_mem::clib::decimal(n as i32));
    m.put_cstr(buf, &text);
    status(m, o, buf, 2);
}

/// [soft 0x0822f168 SI_motionSelect2PntsConstr]
///
/// Picks two models or lights with clicks (code 0x67) and puts a 2-point constraint (code 0x7f)
/// on every other selected model or light of the current list (not on chains: a chain gets the status text 0x08697ea0). A node can be picked once; the second pick must be a different
/// node. Picks are shown by putting the node into mode `SI_select2WM` with its word at +8
/// set to 1 until the loop ends; codes 0x65 (ends) and 0x66 (starts again, the status text 0x08697e74)
/// give the nodes back. Window modes 0x7d4 and 0x7d8 end it at once. (Always 1.)
/// With `SI_lightLicense` the original refuses nodes whose byte at +0x17 has bit 0x10 set;
/// the full product's path is taken here (they are accepted).
pub fn motion_select_2pnts_constr(m: &mut Mem, o: &mut dyn Outside, event: u32) -> u32 {
    let sp = m.sp();
    let buf = m.local(0x50);
    let mut ev = event;
    let (mut state, mut first, mut second) = (0u32, 0u32, 0u32);
    let (mut s1, mut s2) = ((0u16, 0u32), (0u16, 0u32));
    loop {
        if m.u32(ev) == 5 {
            let win = m.u32(ev.wrapping_add(0xc));
            let mode = m.u16(m.u32(win.wrapping_add(0x2c)).wrapping_add(4));
            if mode == 0x7d4 || mode == 0x7d8 {
                m.set_sp(sp);
                return 1;
            }
            let code = m.u16(ev.wrapping_add(6));
            if code == 0x67 {
                let node = call(m, o, &SI_GL_SELECT_CHECK_NSE, &[win]);
                if node != 0 && m.u32(node).wrapping_sub(2) <= 1 {
                    let mut moved = false;
                    if state == 0 {
                        moved = true;
                        state = 1;
                        first = node;
                        s1 = pick(m, node);
                    } else if state == 1 && node != first {
                        moved = true;
                        state = 2;
                        second = node;
                        s2 = pick(m, node);
                    }
                    if moved {
                        call(m, o, &SI_REFRESH_SELECTED_BOTH_BUFFERS, &[1]);
                        picked_message(m, o, 0x08697e62, buf, state);
                    }
                }
            } else if code == 0x65 || code == 0x66 {
                if first != 0 {
                    unpick(m, first, s1);
                }
                if second != 0 {
                    unpick(m, second, s2);
                }
                call(m, o, &SI_REFRESH_SELECTED_BOTH_BUFFERS, &[1]);
                second = 0;
                first = 0;
                if code == 0x66 {
                    state = 0;
                    status(m, o, 0x08697e74, 2);
                } else {
                    break;
                }
            }
        }
        ev = call(m, o, &SI_GET_EVENT, &[]);
        if (state as i32) > 1 {
            break;
        }
    }
    if state == 2 && first != 0 && second != 0 {
        unpick(m, first, s1);
        unpick(m, second, s2);
        let mut n = m.u32(graflist(m).wrapping_add(4));
        while n != 0 {
            let w8 = m.u32(n.wrapping_add(8));
            if w8 != 0 && w8 != 2 && m.u32(n).wrapping_sub(2) <= 1 && n != first && n != second {
                let d = m.u32(n.wrapping_add(4));
                if d != 0 {
                    if is_chain(m, d) {
                        status(m, o, 0x08697ea0, 2);
                    } else {
                        let (a, b) = (m.u32(first.wrapping_add(4)), m.u32(second.wrapping_add(4)));
                        call(m, o, &SI_ED_ELM_SET_PNTS_CONSTR, &[d, a, b, 0, 0x7f]);
                    }
                }
            }
            n = m.u32(n.wrapping_add(0x1c));
        }
        call(m, o, &SI_SCENE_REFRESH, &[]);
    }
    m.set_sp(sp);
    1
}

/// [soft 0x0822f420 SI_motionSelect3PntsConstr]
///
/// As [`motion_select_2pnts_constr`] with three picks (all different; the loop ends after the
/// third) and the 3-point constraint (code 0x77). If the list has no other selected node of
/// the types 2 and 3 (`SI_graflistTypeCount`), `si_constr3PntsFormFace` is asked for a face and
/// the constraint is put on that (the scene redrawn); otherwise on every selected model or
/// light but the three picks, not on chains (a chain gets the status text 0x08697ee0), then the
/// scene is redrawn. (Always 1.)
pub fn motion_select_3pnts_constr(m: &mut Mem, o: &mut dyn Outside, event: u32) -> u32 {
    let sp = m.sp();
    let buf = m.local(0x50);
    let l54 = m.local(4);
    let mut ev = event;
    let (mut state, mut n1, mut n2, mut n3) = (0u32, 0u32, 0u32, 0u32);
    let (mut s1, mut s2, mut s3) = ((0u16, 0u32), (0u16, 0u32), (0u16, 0u32));
    loop {
        if m.u32(ev) == 5 {
            let win = m.u32(ev.wrapping_add(0xc));
            let mode = m.u16(m.u32(win.wrapping_add(0x2c)).wrapping_add(4));
            if mode == 0x7d4 || mode == 0x7d8 {
                m.set_sp(sp);
                return 1;
            }
            let code = m.u16(ev.wrapping_add(6));
            if code == 0x67 {
                let node = call(m, o, &SI_GL_SELECT_CHECK_NSE, &[win]);
                if node != 0 && m.u32(node).wrapping_sub(2) <= 1 {
                    let mut moved = false;
                    if state == 0 {
                        moved = true;
                        n1 = node;
                        s1 = pick(m, node);
                        state = 1;
                    } else if state == 1 && node != n1 {
                        moved = true;
                        n2 = node;
                        s2 = pick(m, node);
                        state = 2;
                    } else if state == 2 && node != n1 && node != n2 {
                        moved = true;
                        n3 = node;
                        s3 = pick(m, node);
                        state = 3;
                    }
                    if moved {
                        call(m, o, &SI_REFRESH_SELECTED_BOTH_BUFFERS, &[1]);
                        picked_message(m, o, 0x08697ebf, buf, state);
                    }
                }
            } else if code == 0x65 || code == 0x66 {
                if n1 != 0 {
                    unpick(m, n1, s1);
                }
                if n2 != 0 {
                    unpick(m, n2, s2);
                }
                if n3 != 0 {
                    unpick(m, n3, s3);
                }
                n3 = 0;
                n2 = 0;
                call(m, o, &SI_REFRESH_SELECTED_BOTH_BUFFERS, &[1]);
                n1 = 0;
                if code == 0x66 {
                    state = 0;
                    status(m, o, 0x08697ed1, 2);
                } else {
                    break;
                }
            }
        }
        ev = call(m, o, &SI_GET_EVENT, &[]);
        if (state as i32) > 2 {
            break;
        }
    }
    if state == 3 && n1 != 0 && n2 != 0 && n3 != 0 {
        unpick(m, n1, s1);
        unpick(m, n2, s2);
        unpick(m, n3, s3);
        let list = graflist(m);
        let (d1, d2, d3) = (m.u32(n1.wrapping_add(4)), m.u32(n2.wrapping_add(4)), m.u32(n3.wrapping_add(4)));
        if (call(m, o, &SI_GRAFLIST_TYPE_COUNT, &[list, 2, 3, l54, 0]) as i32) <= 0 {
            let face = call(m, o, &SI_CONSTR_3PNTS_FORM_FACE, &[]);
            if face != 0 {
                call(m, o, &SI_ED_ELM_SET_PNTS_CONSTR, &[face, d1, d2, d3, 0x77]);
                call(m, o, &SI_SCENE_REFRESH, &[]);
            }
        } else {
            let mut n = m.u32(graflist(m).wrapping_add(4));
            while n != 0 {
                let w8 = m.u32(n.wrapping_add(8));
                if w8 != 0 && w8 != 2 && m.u32(n).wrapping_sub(2) <= 1 && n != n1 && n != n2 && n != n3 {
                    let d = m.u32(n.wrapping_add(4));
                    if d != 0 {
                        if is_chain(m, d) {
                            status(m, o, 0x08697ee0, 2);
                        } else {
                            call(m, o, &SI_ED_ELM_SET_PNTS_CONSTR, &[d, d1, d2, d3, 0x77]);
                        }
                    }
                }
                n = m.u32(n.wrapping_add(0x1c));
            }
            call(m, o, &SI_SCENE_REFRESH, &[]);
        }
    }
    m.set_sp(sp);
    1
}

/// [soft 0x0822f7e4 SI_motionSelectConstraint]
///
/// The command to constrain every selected node of the current list to the node the user picked
/// in the window of the event (`SI_gLSelectCheckNSE`), with the constraint of the relation `code`
/// (a half word: 0x6f to 0x81 and 0xf6, 0xf7 in the table of `SI_nbConstraintsOnElem`'s pairs).
/// The picked node must be a model (relations 0x75, 0x76, 0x72, 0x73, 0x7c, 0x7d, 0x7e, 0x80, 0x81,
/// 0xf6, 0xf7: status text 0x08697eff), otherwise a model, light or camera (types 2 to 5, 6
/// to 8, 0x13, 0x14); 0x7d wants a mesh (text 0x08697f3f), 0x7c a curve (text 0x08697f53). For each other node of the list that is active (its word at +8 neither 0 nor 2) the
/// code's own rule is tried (cameras and interests, root models for planes, models for scaling,
/// no chains for tangent and normal, a chain's joint for 0x81, else a model); a failing node says
/// why (level 3) and is skipped. Then both nodes are drawn as picked, the screen swapped and
/// refreshed (the half-second `usleep` does nothing here) and `SI_itemSetConstraint` is asked; if
/// it refuses (the cycle dialog, texts from 0x08698240) the constraint is relaxed and the
/// loop ends. The scene is redrawn if any node took the constraint, and the controls refreshed.
/// (The tangent constraint to a NURBS curve warns once, in a dialog, if the model is not on a path.)
pub fn motion_select_constraint(m: &mut Mem, o: &mut dyn Outside, event: u32, code: i32) {
    let sp = m.sp();
    let code = code as u16;
    let (mut done, mut cycle, mut warned) = (false, false, false);
    let l4 = m.local(4);
    let target = call(m, o, &SI_GL_SELECT_CHECK_NSE, &[m.u32(event.wrapping_add(0xc))]);
    if target != 0 {
        let tt = m.u32(target);
        let reference = matches!(code, 0x75 | 0x76 | 0x73 | 0x72 | 0x7c | 0x7d | 0x80 | 0x81 | 0xf7 | 0x7e | 0xf6);
        if reference && tt.wrapping_sub(2) > 1 {
            status(m, o, 0x08697eff, 3);
        } else if tt.wrapping_sub(2) > 3 && !matches!(tt, 8 | 6 | 7 | 0x13 | 0x14) {
            status(m, o, 0x08697f20, 3);
        } else if code == 0x7d && m.u32(target.wrapping_add(4)) != 0 && m.u32(m.u32(target.wrapping_add(4)).wrapping_add(0x30)) != 4 {
            status(m, o, 0x08697f3f, 3);
        } else if code == 0x7c && m.u32(target.wrapping_add(4)) != 0 && !matches!(m.u32(m.u32(target.wrapping_add(4)).wrapping_add(0x30)), 9 | 6) {
            status(m, o, 0x08697f53, 3);
        } else {
            let mut n = m.u32(graflist(m).wrapping_add(4));
            while n != 0 && !cycle {
                let w8 = m.u32(n.wrapping_add(8));
                if w8 != 0 && w8 != 2 && n != target {
                    let t = m.u32(n);
                    let d = m.u32(n.wrapping_add(4));
                    let mut message = 0u32;
                    let idx = code.wrapping_sub(111) as i16 as i32;
                    match idx {
                        0 | 17 => {
                            // 111, 128: cameras
                            if t.wrapping_sub(4) <= 1 {
                                if d != m.u32(m[crate::globals::SI_PROC_DATA].wrapping_add(8)) {
                                    message = 0x08697f80;
                                } else if t == 4 && m.u32(n.wrapping_add(0x20)) == target {
                                    message = 0x08697fc0;
                                } else if t == 5 && m.u32(n.wrapping_add(0x1c)) == target {
                                    message = 0x08698000;
                                }
                            }
                        }
                        6 | 7 => {
                            // 117, 118: plane constraints, root models only
                            if !(t.wrapping_sub(2) <= 1 && d != 0 && m.u32(d.wrapping_add(0x2bc)) == 0) {
                                message = 0x08698040;
                            }
                        }
                        4 => {
                            // 115: scaling
                            if t.wrapping_sub(2) > 1 {
                                message = 0x08698080;
                            }
                        }
                        13 => {
                            // 124: tangent
                            if t.wrapping_sub(2) <= 1 {
                                if d != 0 && is_chain(m, d) {
                                    message = 0x086980c0;
                                } else {
                                    let td = m.u32(target.wrapping_add(4));
                                    if m.u32(td.wrapping_add(0x30)) == 9 {
                                        let elem = call(m, o, &out::SI_ED_GET_ELEM_FROM_MODEL, &[d, l4]);
                                        if elem != 0 {
                                            let cid8 = m[crate::globals::SI_CID][8] as u32;
                                            let found = call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[crate::globals::cur_dict(m), elem, 1, 1, 0x70, cid8, l4, l4]);
                                            if found == 0 && !warned {
                                                call(m, o, &out::SI_DIALOG_REQUEST, &[0x08698140, 0x08698100, 0x086980e3, 0x086980e0]);
                                                warned = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        14 => {
                            // 125: normal
                            if t.wrapping_sub(2) <= 1 && d != 0 && is_chain(m, d) {
                                message = 0x08698180;
                            }
                        }
                        18 => {
                            // 129: a chain's joint
                            if !(t.wrapping_sub(2) <= 1 && d != 0 && m.u32(d.wrapping_add(0x30)) == 5) {
                                message = 0x086981a0;
                            }
                        }
                        _ => {
                            if t.wrapping_sub(2) > 1 {
                                message = 0x086981e0;
                            }
                        }
                    }
                    if message != 0 {
                        status(m, o, message, 3);
                        // the original sets a flag that skips this node
                    } else {
                        let list = graflist(m);
                        call(m, o, &SI_FOOL_DISPLAY, &[list, 0, 1, n]);
                        let list = graflist(m);
                        call(m, o, &SI_FOOL_DISPLAY, &[list, 0, 2, target]);
                        call(m, o, &SWAPBUFFERS, &[]);
                        // usleep(500000): nothing in memory changes
                        call(m, o, &SI_REFRESH_SYSTEM, &[0, 0, 0x25]);
                        let (nd, nt, td, tt) = (m.u32(n.wrapping_add(4)), m.u32(n), m.u32(target.wrapping_add(4)), m.u32(target));
                        let r = call(m, o, &SI_ITEM_SET_CONSTRAINT, &[nd, nt, td, tt, code as i16 as i32 as u32]);
                        if r as u16 == 0 {
                            call(m, o, &out::SI_DIALOG_REQUEST, &[0x08698240, 0x08698210, 0x0869820f, 0x0869820c]);
                            let (nd, nt) = (m.u32(n.wrapping_add(4)), m.u32(n));
                            call(m, o, &SI_ITEM_RELAX_CONSTRAINT, &[nd, nt]);
                            cycle = true;
                        } else {
                            done = true;
                        }
                    }
                }
                n = m.u32(n.wrapping_add(0x1c));
            }
            if done {
                call(m, o, &SI_SCENE_REFRESH, &[]);
            }
            call(m, o, &SI_REFRESH_ALL_CONTROLS, &[]);
        }
    }
    m.set_sp(sp);
}

/// [soft 0x0822ffa0 copyExpression]
///
/// Copies the expressions of the item the user picked (`SI_gLSelect`, in the window of the event)
/// to every other active node of the current list. The status text 0x08698341 (level 3) when the list
/// holds no selected node; text 0x08698353 when the picked item has no serial
/// relation 0x4e2a. For each other active node whose element has the same chapter as the
/// picked one, `SI_copyElemExpression` is asked (text 0x08698380 if
/// it answers a non-zero half word) and the "copied from A to B" status line (the elements' names:
/// the word at +0x24 of the record each element points to) is shown; for other chapters the
/// status text 0x08698400 (level 2).
pub fn copy_expression(m: &mut Mem, o: &mut dyn Outside, event: u32) {
    let sp = m.sp();
    let (l84, l88, l8c, l90, l94, l98) = (m.local(4), m.local(4), m.local(4), m.local(4), m.local(4), m.local(4));
    let buf = m.local(0x80);
    let list = graflist(m);
    if call(m, o, &SI_GRAFLIST_TYPE_COUNT, &[list, 0, 0, l84, 0]) == 0 {
        status(m, o, 0x08698341, 3);
    } else {
        let picked = call(m, o, &SI_GL_SELECT, &[m.u32(event.wrapping_add(0xc))]);
        if picked != 0 {
            let edi = call(m, o, &SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[picked, l8c, l88]);
            let args = [crate::globals::cur_dict(m), edi, 0x4e2a, l90, u32::MAX, 1, 1];
            if o.call(m, &SI_ELEMDICT_FIND_SERIAL_RELATION, &args) == 0 {
                status(m, o, 0x08698353, 3);
            } else {
                call(m, o, &SI_FLASH_OBJECT, &[picked, 0]);
                m.set_u32(l84, m.u32(graflist(m).wrapping_add(4)));
                while m.u32(l84) != 0 {
                    let n = m.u32(l84);
                    let w8 = m.u32(n.wrapping_add(8));
                    if w8 != 0 && w8 != 2 && picked != n {
                        let ebx = call(m, o, &SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[n, l98, l94]);
                        let chapter = m.u32(l8c);
                        if chapter == m.u32(l98) {
                            let k = m.u32(l94);
                            if call(m, o, &SI_COPY_ELEM_EXPRESSION, &[edi, ebx, k, chapter]) as u16 != 0 {
                                status(m, o, 0x08698380, 0);
                            }
                            let (to, from) = (m.u32(m.u32(ebx).wrapping_add(0x24)), m.u32(m.u32(edi).wrapping_add(0x24)));
                            // the status line is the format at 0x086983c0 with the two names (read from the user's install)
                            let text = fmt_s(si_rt::message(0x086983c0), &[&m.cstr(from), &m.cstr(to)]);
                            m.put_cstr(buf, &text);
                            status(m, o, buf, 0);
                        } else {
                            status(m, o, 0x08698400, 2);
                        }
                    }
                    m.set_u32(l84, m.u32(n.wrapping_add(0x1c)));
                }
            }
        }
    }
    m.set_sp(sp);
}

/// [soft 0x08230198 motionSelectPath]
///
/// The command to put the selected item on the path the user picks (in the scene mode 0x2a to
/// replace its translation by that path). More than one selected item: the "many items"
/// dialog; none, or one with bit 4 of its byte +0x17 set: nothing. In the mode 0x2a the item
/// must be a model or light (text 0x08698460, level 3); a sun (type 0x14)
/// cannot be assigned (text 0x086984a0, level 2). The picked node must be a
/// model or light whose data has the kind (+0x30) 6 or 9 (a curve); it is drawn as picked, the
/// screen swapped and refreshed (the 200 ms `usleep` does nothing here). The item itself:
/// text 0x086984e0 (level 3) when it is the path of itself; a kind 9 curve with no count or a count not above its
/// second (text 0x08698500, level 3, then `sleep(1)`).
/// In the mode 0x2a `si_replaceAnyTranslationByExplPath` is called. Otherwise the old path
/// relation (0xa0) of the item is taken out of the dictionary, `si_itemSetPathTiming` is called
/// (its last argument 1 when `scnmod` is 0x2f) and `SI_elemdictCheckCycle` asked: on a cycle a
/// dialog (texts from 0x0869853d) and `si_itemResetPath`; else the relation of the item is removed
/// by id. Last the item's colour is reset and the scene redrawn.
pub fn motion_select_path(m: &mut Mem, o: &mut dyn Outside, event: u32) {
    let sp = m.sp();
    let (l4, l8, lc, l10, l14) = (m.local(4), m.local(4), m.local(4), m.local(4), m.local(4));
    let scn = m[SCNMOD];
    let di = scn == 0x2a;
    let l16 = (scn == 0x2f) as u32;
    'body: {
        let list = graflist(m);
        if (call(m, o, &SI_GRAFLIST_COUNT_SELECTED, &[list, l4, 1]) as i32) > 1 {
            motion_multi_message(m, o);
            break 'body;
        }
        let sel = m.u32(l4);
        if sel == 0 || m.u8(sel.wrapping_add(0x17)) & 0x10 != 0 {
            break 'body;
        }
        if di && m.u32(sel).wrapping_sub(2) > 1 {
            status(m, o, 0x08698460, 3);
            break 'body;
        }
        if m.u32(sel) == 0x14 {
            status(m, o, 0x086984a0, 2);
            break 'body;
        }
        let ebx = call(m, o, &SI_GL_SELECT, &[m.u32(event.wrapping_add(0xc))]);
        if ebx == 0 || m.u32(ebx).wrapping_sub(2) > 1 {
            break 'body;
        }
        let esi = m.u32(ebx.wrapping_add(4));
        if esi == 0 {
            break 'body;
        }
        let kind = m.u32(esi.wrapping_add(0x30));
        if kind != 6 && kind != 9 {
            break 'body;
        }
        let list = graflist(m);
        call(m, o, &SI_FOOL_DISPLAY, &[list, 0, 2, ebx]);
        call(m, o, &SWAPBUFFERS, &[]);
        // usleep(200000): nothing in memory changes
        call(m, o, &SI_REFRESH_SYSTEM, &[0, 0, 0x2d]);
        if ebx == m.u32(l4) {
            status(m, o, 0x086984e0, 3);
            break 'body;
        }
        if kind == 9 {
            let p = m.u32(esi.wrapping_add(0x48));
            if p == 0 || m.i32(p.wrapping_add(4)) < m.i32(p) {
                status(m, o, 0x08698500, 3);
                // sleep(1): nothing in memory changes
                break 'body;
            }
        }
        let sel = m.u32(l4);
        if di {
            let d = m.u32(sel.wrapping_add(4));
            if d != 0 {
                call(m, o, &SI_REPLACE_ANY_TRANSLATION_BY_EXPL_PATH, &[d, esi, 1]);
            }
        } else {
            let (t, d) = (m.u32(sel), m.u32(sel.wrapping_add(4)));
            if t.wrapping_sub(2) <= 1 && d != 0 {
                let elem = call(m, o, &out::SI_ED_GET_ELEM_FROM_MODEL, &[d, l8]);
                let cid8 = m[crate::globals::SI_CID][8] as u32;
                let args = [crate::globals::cur_dict(m), elem, 1, 1, 0xa0, cid8, l10, lc];
                if o.call(m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &args) != 0 {
                    let args = [crate::globals::cur_dict(m), SI_RD, m.u32(l8), m.u32(lc), 0xa0, 1, 0];
                    o.call(m, &SI_ELEMDICT_REMOVE_RELATION_PAIR, &args);
                }
            }
            let (t, d) = (m.u32(sel), m.u32(sel.wrapping_add(4)));
            call(m, o, &SI_ITEM_SET_PATH_TIMING, &[d, t, esi, 1, l16 as i16 as i32 as u32]);
            let dict = crate::globals::cur_dict(m);
            if call(m, o, &SI_ELEMDICT_CHECK_CYCLE, &[SI_RD, dict]) as u16 != 0 {
                call(m, o, &out::SI_DIALOG_REQUEST, &[0x08698560, 0x08698541, 0x08698540, 0x0869853d]);
                call(m, o, &SI_ITEM_RESET_PATH, &[sel]);
            } else {
                let (t, d) = (m.u32(sel), m.u32(sel.wrapping_add(4)));
                if t.wrapping_sub(2) <= 1 && d != 0 {
                    call(m, o, &out::SI_ED_GET_ELEM_FROM_MODEL, &[d, l14]);
                    let args = [crate::globals::cur_dict(m), SI_RD, m.u32(l14), 0xa0];
                    o.call(m, &SI_ELEMDICT_REMOVE_RELATION_ID, &args);
                }
            }
        }
        call(m, o, &SI_SCHEM_SET_DEFAULT_COLOR, &[sel]);
        call(m, o, &SI_SCENE_REFRESH, &[]);
    }
    m.set_sp(sp);
}

// ---------------------------------------------------------------------------------------------
// The scene window's client and its set-up.



/// [soft 0x082304cc SI_motionClient]
///
/// The scene window's loop: waits for the main event (`SI_getMainEvent(1)`, stored through
/// `ev`) and acts on its type: 1 sets the scene up (`initScene`, `SI_initWave`,
/// `SI_initParticle`) and refreshes it; 2 answers what `SI_needInter` says (a half word); 3
/// refreshes the scene after making the multi-selection's mode (`SI_scngl`) the current list's;
/// 4 releases it and answers 0; 5 `scnWind`, 6 `scnMenu`, 8 and 9 `scnButton` (all with the
/// event pointer); 7 nothing; 26 `SI_fastKey` and, when it does not take the key, the number
/// of windows is compared before and after redrawing the selection and, if it changed, a frame
/// step is made; then `scnSetMode(0, 1)`; every other type shows the messages of the supra mode.
pub fn motion_client(m: &mut Mem, o: &mut dyn Outside, ev: u32) -> i32 {
    loop {
        let e = call(m, o, &SI_GET_MAIN_EVENT, &[1]);
        m.set_u32(ev, e);
        match m.u32(e).wrapping_sub(1) {
            0 => {
                init_scene(m, o);
                call(m, o, &SI_INIT_WAVE, &[]);
                call(m, o, &SI_INIT_PARTICLE, &[]);
                refresh_scene(m, o);
            }
            1 => return call(m, o, &SI_NEED_INTER, &[]) as u16 as i16 as i32,
            2 => {
                let list = graflist(m);
                let (mode, scngl) = (m.u16(list), m[crate::globals::SI_SCNGL]);
                if mode != m.u16(scngl) {
                    call(m, o, &SI_GRAFLIST_SET_MULTI_SELECTION, &[scngl, mode as i16 as i32 as u32]);
                }
                refresh_scene(m, o);
            }
            3 => {
                release_scene(m);
                return 0;
            }
            4 => {
                let e = m.u32(ev);
                call(m, o, &SCN_WIND, &[e]);
            }
            5 => {
                let e = m.u32(ev);
                call(m, o, &SCN_MENU, &[e]);
            }
            6 => {}
            7 | 8 => {
                let e = m.u32(ev);
                call(m, o, &SCN_BUTTON, &[e]);
            }
            25 => {
                let e = m.u32(ev);
                if call(m, o, &SI_FAST_KEY, &[e]) as u16 == 0 {
                    let before = call(m, o, &SI_GET_NB_WINDOWS, &[]);
                    call(m, o, &SI_REFRESH_SELECTED_BOTH_BUFFERS, &[1]);
                    let after = call(m, o, &SI_GET_NB_WINDOWS, &[]);
                    if before != after {
                        call(m, o, &SI_SCENE_STEP_FRAME, &[0, 0, 0, 0]);
                    }
                    call(m, o, &SCN_SET_MODE, &[0, 1]);
                }
            }
            _ => {
                let v = m[SI_SUPRA_MODE];
                call(m, o, &SI_DISPLAY_MESSAGES, &[v]);
            }
        }
    }
}

/// The menu states `initScene` sets first: (menu, sub-menu, item, flag, value), in order.
const INIT_MENUS: [(u32, u32, u32, u32, u32); 55] = [
    (0x6e, 0xa, 0x6, 0x1, 0x0), (0x2da, 0x2db, 0xc, 0x1, 0x0), (0x2da, 0x2db, 0xf, 0x1, 0x0), (0x2cf, 0x2d0, 0xa, 0x1, 0x0),
    (0x2cf, 0x2d0, 0x7, 0x1, 0x0), (0x2bc, 0x2bd, 0x9, 0x1, 0x0), (0x2bc, 0x2bd, 0xc, 0x1, 0x0), (0x2da, 0x2db, 0xd, 0x1, 0x0),
    (0x2da, 0x2db, 0x10, 0x1, 0x0), (0x2da, 0x2db, 0xe, 0x1, 0x0), (0x2da, 0x2db, 0x11, 0x1, 0x0), (0x2cf, 0x2d0, 0x8, 0x1, 0x0),
    (0x2cf, 0x2d0, 0xb, 0x1, 0x0), (0x2cf, 0x2d0, 0x9, 0x1, 0x0), (0x2cf, 0x2d0, 0xc, 0x1, 0x0), (0x2bc, 0x2bd, 0xa, 0x1, 0x0),
    (0x2bc, 0x2bd, 0xd, 0x1, 0x0), (0x2bc, 0x2bd, 0xb, 0x1, 0x0), (0x2bc, 0x2bd, 0xe, 0x1, 0x0), (0x2cf, 0x2cf, 0x4, 0x1, 0x0),
    (0x2bc, 0x2bc, 0x9, 0x1, 0x0), (0x2e6, 0x2e6, 0x7, 0x1, 0x0), (0x2da, 0x2da, 0x8, 0x1, 0x0), (0x801, 0x801, 0x0, 0x1, 0x0),
    (0x801, 0x80b, 0x0, 0x1, 0x0), (0x801, 0x80b, 0x0, 0x1, 0x0), (0x801, 0x80b, 0x1, 0x1, 0x0), (0x801, 0x80b, 0x2, 0x1, 0x0),
    (0x801, 0x80b, 0x3, 0x1, 0x0), (0x801, 0x80b, 0x4, 0x1, 0x0), (0x801, 0x80b, 0x5, 0x1, 0x0), (0x801, 0x801, 0x1, 0x1, 0x0),
    (0x2da, 0x2db, 0xa, 0x1, 0x0), (0x2bc, 0x31a, 0x3, 0x1, 0x0), (0x2bc, 0x31a, 0x1, 0x1, 0x0), (0x2bc, 0x31a, 0x4, 0x1, 0x0),
    (0x2bc, 0x31a, 0x2, 0x1, 0x0), (0x2bc, 0x31a, 0xc, 0x1, 0x0), (0x2bc, 0x31a, 0xd, 0x1, 0x0), (0x2bc, 0x31a, 0xe, 0x1, 0x0),
    (0x2bc, 0x31a, 0xf, 0x1, 0x0), (0x2bc, 0x31a, 0x10, 0x1, 0x0), (0x2bc, 0x31a, 0x11, 0x1, 0x0), (0x2bc, 0x31a, 0x12, 0x1, 0x0),
    (0x2bc, 0x31a, 0x13, 0x1, 0x0), (0x2bc, 0x31a, 0x14, 0x1, 0x0), (0x2bc, 0x31a, 0x15, 0x1, 0x0), (0x2bc, 0x31a, 0x16, 0x1, 0x0),
    (0x2bc, 0x31a, 0x6, 0x1, 0x0), (0x2bc, 0x31a, 0x7, 0x1, 0x0), (0x2bc, 0x31a, 0x8, 0x1, 0x0), (0x2bc, 0x31a, 0x9, 0x1, 0x0),
    (0x2bc, 0x31a, 0xa, 0x1, 0x0), (0x2bc, 0x31a, 0xb, 0x1, 0x0), (0x2bc, 0x31a, 0x5, 0x1, 0x0),
];
/// The menu states `initScene` sets next when `SI_lightLicense` is not 0.
const INIT_MENUS_LIGHT: [(u32, u32, u32, u32, u32); 30] = [
    (0x2bc, 0x2bd, 0xa, 0x1, 0x1), (0x2bc, 0x2bd, 0xb, 0x1, 0x1), (0x2bc, 0x2bd, 0xd, 0x1, 0x1), (0x2bc, 0x2bd, 0xe, 0x1, 0x1),
    (0x2cf, 0x2d0, 0x8, 0x1, 0x1), (0x2cf, 0x2d0, 0x9, 0x1, 0x1), (0x2cf, 0x2d0, 0xb, 0x1, 0x1), (0x2cf, 0x2d0, 0xc, 0x1, 0x1),
    (0x2e6, 0x2e7, 0xa, 0x1, 0x1), (0x2e6, 0x2e7, 0xb, 0x1, 0x1), (0x2e6, 0x2e7, 0xd, 0x1, 0x1), (0x2e6, 0x2e7, 0xe, 0x1, 0x1),
    (0x6e, 0xa, 0x8, 0x1, 0x1), (0x2bc, 0x2bc, 0xa, 0x1, 0x1), (0x2bc, 0x2bc, 0xb, 0x1, 0x1), (0x2e6, 0x2e6, 0x8, 0x1, 0x1),
    (0x2e6, 0x2e6, 0x9, 0x1, 0x1), (0x196e, 0x196e, 0x0, 0x1, 0x1), (0x196e, 0x196e, 0x1, 0x1, 0x1), (0x196e, 0x196e, 0x2, 0x1, 0x1),
    (0x196e, 0x196e, 0x3, 0x1, 0x1), (0x196e, 0x196e, 0x4, 0x1, 0x1), (0x196e, 0x196e, 0x5, 0x1, 0x1), (0x196e, 0x196e, 0x6, 0x1, 0x1),
    (0x196e, 0x196e, 0x7, 0x1, 0x1), (0x196e, 0x196e, 0x8, 0x1, 0x1), (0x196e, 0x196e, 0x9, 0x1, 0x1), (0x196e, 0x196e, 0xa, 0x1, 0x1),
    (0x196e, 0x196e, 0xb, 0x1, 0x1), (0x196e, 0x196e, 0xb, 0x1, 0x1),
];

fn menu_state5(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, item: u32, flag: u32, value: u32) {
    call(m, o, &SI_MENU_STATE_SET, &[1, a, b, item, flag, value]);
}

/// initScene (0x082306c4), partial: the licence manager's path is not ported.
///
/// Sets the scene window up: `scnSetMode(0, 1)`, the update option 0, `SI_setFilterTrackTable(1, -1)`,
/// then the state of 55 menu items (a table of the binary: `INIT_MENUS`), another 30 when
/// `SI_lightLicense` is not 0, then the three states of items 0xc/0xa/0xb of the menus
/// 0x2bc, 0x2e6 and 0x2da, which are 1 for the light and base products (`SI_lightLicense`,
/// `SI_baseLicense` not 0) and, for the full product, whatever its two licensed features say;
/// the full product's path is taken here (both granted, so 0). Then the states of the flags
/// `branchSelectedFcv`, `SI_clearSelectedFcv`, `si_slopeDraw`, `si_slopePick`, `SI_showFcvGrps`,
/// the refresh option menu (`SI_motionSetFcvUpdOpt`) and `libTrackFilter`.
/// The original asks the licence manager (`lmdecrypt`, `lc_checkout`) when both flags are 0;
/// this port has no licence manager and the check cannot run that path.
pub fn init_scene(m: &mut Mem, o: &mut dyn Outside) {
    call(m, o, &SCN_SET_MODE, &[0, 1]);
    m[SI_UPDATE_OPTION] = 0;
    call(m, o, &SI_SET_FILTER_TRACK_TABLE, &[1, u32::MAX]);
    for &(a, b, item, flag, value) in INIT_MENUS.iter() {
        menu_state5(m, o, a, b, item, flag, value);
    }
    if m[SI_LIGHT_LICENSE] != 0 {
        for &(a, b, item, flag, value) in INIT_MENUS_LIGHT.iter() {
            menu_state5(m, o, a, b, item, flag, value);
        }
    }
    let limited = (m[SI_BASE_LICENSE] != 0 || m[SI_LIGHT_LICENSE] != 0) as u32;
    menu_state5(m, o, 0x2bc, 0x2bc, 0xc, 1, limited);
    menu_state5(m, o, 0x2e6, 0x2e6, 0xa, 1, limited);
    menu_state5(m, o, 0x2da, 0x2da, 0xb, 1, limited);
    let v = m[BRANCH_SELECTED_FCV] as u32;
    menu_state5(m, o, 0x2bc, 0x2bc, 0x11, 0, v);
    let v = m[SI_CLEAR_SELECTED_FCV] as i32 as u32;
    menu_state5(m, o, 0x2bc, 0x2bc, 0x10, 0, v);
    let v = m[SI_SLOPE_DRAW] as i32 as u32;
    menu_state5(m, o, 0x2bc, 0x2e1, 0, 0, v);
    let v = m[SI_SLOPE_PICK] as i32 as u32;
    menu_state5(m, o, 0x2bc, 0x2e1, 1, 0, v);
    let v = m[SI_SHOW_FCV_GRPS] as i32 as u32;
    menu_state5(m, o, 0x2bc, 0x2e1, 2, 0, v);
    motion_set_fcv_upd_opt(m, o);
    let v = m[LIB_TRACK_FILTER] as i32 as u32;
    menu_state5(m, o, 0x6e, 0x26b9, 3, 0, v);
}

// ---------------------------------------------------------------------------------------------
// The menu look-ups and the table of the function curves.

/// [soft 0x082352fc getGraftypeFromMenu]
/// the graph type (a chapter of the animation: 0, 2, 4, 6 or
/// 11) an item of the menus stands for; 2 for any item not listed.
pub fn get_graftype_from_menu(menu: i32) -> i32 {
    let e = menu;
    if e == 0x304 {
        4
    } else if e > 0x304 {
        if e == 0x314 {
            4
        } else if e > 0x314 {
            if e > 0x32c {
                if e == 0x708 {
                    0
                } else if e < 0x708 {
                    if e > 0x32f { 2 } else { 4 }
                } else if e == 0x726 {
                    0
                } else {
                    2
                }
            } else if e >= 0x32b {
                11
            } else if e == 0x31b {
                0
            } else if e > 0x31b {
                if e == 0x329 { 11 } else { 2 }
            } else if e == 0x315 {
                6
            } else {
                2
            }
        } else if e == 0x30c {
            6
        } else if e > 0x30c {
            if e == 0x30e {
                4
            } else if e < 0x30e {
                11
            } else if e == 0x30f {
                6
            } else if e == 0x310 {
                11
            } else {
                2
            }
        } else if e == 0x307 {
            0
        } else if e > 0x307 {
            if e == 0x30b { 4 } else { 2 }
        } else if e == 0x306 {
            11
        } else {
            2
        }
    } else if e == 0x2e9 {
        6
    } else if e > 0x2e9 {
        if e == 0x2f2 {
            6
        } else if e > 0x2f2 {
            if e == 0x2f7 {
                6
            } else if e > 0x2f7 {
                if e == 0x302 {
                    4
                } else if e == 0x303 {
                    6
                } else {
                    2
                }
            } else if e == 0x2f6 {
                4
            } else {
                2
            }
        } else if e < 0x2ec {
            2
        } else if e <= 0x2ed || e == 0x2ef {
            11
        } else {
            2
        }
    } else if e > 0x2d9 {
        if e == 0x2dd {
            6
        } else if e > 0x2dd {
            if e == 0x2e2 {
                11
            } else if e == 0x2e8 {
                4
            } else {
                2
            }
        } else if e == 0x2dc {
            4
        } else {
            2
        }
    } else if e >= 0x2d8 {
        11
    } else if e == 0x2be {
        4
    } else if e == 0x2bf {
        6
    } else {
        2
    }
}

/// [soft 0x082354d0 getFcvOperFromMenu]
/// the operation (1: a value, 2: a time or other pair; 0 none)
/// an item of the function-curve menus stands for.
pub fn get_fcv_oper_from_menu(menu: i32) -> i32 {
    let e = menu;
    if e <= 0x30d {
        if e >= 0x30b || e == 0x2f2 {
            2
        } else if e > 0x2f2 {
            if e <= 0x2fb {
                if e >= 0x2fa || e == 0x2f6 {
                    2
                } else if e < 0x2f6 || e == 0x2f7 {
                    1
                } else {
                    0
                }
            } else if e <= 0x302 {
                if e >= 0x300 {
                    1
                } else if e > 0x2fd {
                    0
                } else {
                    1
                }
            } else if e == 0x306 {
                1
            } else {
                0
            }
        } else if e == 0x2d9 {
            2
        } else if e > 0x2d9 {
            if e == 0x2e2 {
                2
            } else if e > 0x2e2 {
                if e > 0x2ed || e < 0x2e7 { 0 } else { 1 }
            } else if e > 0x2dd || e < 0x2db {
                0
            } else {
                2
            }
        } else if e == 0x1ad {
            2
        } else if e > 0x1ad {
            if e > 0x2cd || e < 0x2cb { 0 } else { 2 }
        } else if e == 0x1ac {
            1
        } else {
            0
        }
    } else if e == 0x32e {
        1
    } else if e > 0x32e {
        if e == 0x354 {
            2
        } else if e > 0x354 {
            if e > 0x708 {
                if e == 0x726 { 2 } else { 0 }
            } else if e >= 0x703 {
                2
            } else if e == 0x35d {
                1
            } else if e == 0x35e {
                2
            } else {
                0
            }
        } else if e == 0x349 {
            1
        } else if e > 0x349 {
            if e == 0x34a {
                2
            } else if e == 0x353 {
                1
            } else {
                0
            }
        } else if e == 0x335 {
            1
        } else if e == 0x336 {
            2
        } else {
            0
        }
    } else if e == 0x31e {
        1
    } else if e > 0x31e {
        if e == 0x32b {
            2
        } else if e > 0x32b {
            if e == 0x32c {
                1
            } else if e == 0x32d {
                2
            } else {
                0
            }
        } else if e > 0x321 || e < 0x320 {
            0
        } else {
            1
        }
    } else if e < 0x314 {
        0
    } else if e <= 0x315 {
        1
    } else if e > 0x319 || e < 0x318 {
        0
    } else {
        2
    }
}

/// [soft 0x082356e8 cpyValidateElem]
///
/// Whether the item `to` may take the copy of `from` (their graph nodes `x` and `y`, elements of the
/// chapter `chapter`, for the menu item `menu` and the sub-item `sub`): not onto itself ("Cannot
/// copy onto itself.", level 3, 0); the three expression items (0x334 to 0x336) need the expression
/// flag (+0x2f4) of the first node's data; for the chapter of the lattices (`SI_cid` +0x14) the
/// two lattices' counts at +0x4c (sub-item 9) or +0x54 (12) must be equal ("... same number of
/// ..." message). 1 otherwise.
pub fn cpy_validate_elem(m: &mut Mem, o: &mut dyn Outside, menu: i32, sub: i32, a: u32, x: u32, y: u32, chapter: u32) -> u32 {
    let sp = m.sp();
    let say = |m: &mut Mem, o: &mut dyn Outside, text: u32| {
        call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[text, 3]);
    };
    let ret = if y == x {
        say(m, o, 0x0869864f);
        0
    } else if menu == 0x336 || menu == 0x334 || menu == 0x335 {
        let d = m.u32(a.wrapping_add(4));
        if d != 0 && m.u32(d.wrapping_add(0x2f4)) != 0 {
            1
        } else {
            say(m, o, 0x08698680);
            0
        }
    } else if chapter != m[crate::globals::SI_CID][5] as u32 {
        1
    } else {
        let dx = m.u32(x.wrapping_add(4));
        let dy = if dx != 0 { m.u32(y.wrapping_add(4)) } else { 0 };
        let mut bad = false;
        if dx != 0 && dy != 0 && sub == 9 {
            bad = m.u16(dy.wrapping_add(0x4c)) != m.u16(dx.wrapping_add(0x4c));
        } else if sub == 0xc {
            bad = m.u16(dy.wrapping_add(0x54)) != m.u16(dx.wrapping_add(0x54));
        }
        if bad {
            say(m, o, 0x086986c0);
            0
        } else {
            1
        }
    };
    m.set_sp(sp);
    ret
}

/// [soft 0x08234fe8 si_getDicInfoFromGrafnode]
///
/// The element a graph node stands for, by the menu item `menu` (and `ty` for the items of the
/// models, shapes, lattices and clusters), and the `SI_cid` chapter of it, stored through `cid_out`;
/// `aux` is passed on to the look-up routine of the element (it stores the index there). Items
/// not listed go to `SI_elemdictFindElemFromGrafNode`.
pub fn get_dic_info_from_grafnode(m: &mut Mem, o: &mut dyn Outside, node: u32, menu: i32, ty: i32, cid_out: u32, aux: u32) -> u32 {
    let cid = |m: &Mem, i: usize| m[crate::globals::SI_CID][i] as u32;
    let data = |m: &Mem| m.u32(node.wrapping_add(4));
    // 1 material (0x2c0...), 2 the 2D texture (3), 3 the 3D texture, 4 scene, 5 model, 6 the 0x2bd group, 7 default
    let class = match menu {
        0x31e | 0x31f | 0x704 => 3,
        0x320 | 0x322 | 0x705 => 4,
        0x321 | 0x323 | 0x707 => 6,
        0x2eb | 0x31a | 0x706 => 5,
        0x2c0 | 0x2ea | 0x703 => 1,
        0x307 | 0x31b | 0x708 | 0x726 => 2,
        0x1ab..=0x1ad => 7,
        0x2bd | 0x2e7 | 0x2db => 8,
        _ => 0,
    };
    let sp = m.sp();
    let mut r = 0u32;
    match class {
        1 => {
            // the material of the model's current material slot
            let d = data(m);
            let t = m.u32(d.wrapping_add(0x20c));
            if t != 0 {
                let mat = m.u32(t.wrapping_add((m.i16(d.wrapping_add(0x208)) as i32 as u32).wrapping_mul(4)));
                if mat != 0 {
                    r = call(m, o, &SI_ED_GET_ELEM_FROM_MATERIAL, &[mat, aux]);
                }
            }
            m.set_u32(cid_out, cid(m, 7));
        }
        3 | 4 => {
            let d = data(m);
            let mut tex = 0;
            if class == 3 {
                if m.u32(d.wrapping_add(0x21c)) != 0 {
                    tex = m.u32(d.wrapping_add(0x218));
                }
            } else {
                let t = m.u32(d.wrapping_add(0x20c));
                if t != 0 {
                    let mat = m.u32(t.wrapping_add((m.i16(d.wrapping_add(0x208)) as i32 as u32).wrapping_mul(4)));
                    if mat != 0 && m.u32(mat.wrapping_add(0x6ec)) != 0 {
                        tex = m.u32(mat.wrapping_add(0x6f0));
                    }
                }
            }
            if tex != 0 {
                r = call(m, o, &SI_ED_GET_ELEM_FROM_TEXTURE2D, &[tex, aux]);
            }
            m.set_u32(cid_out, cid(m, 26));
        }
        5 | 6 => {
            let d = data(m);
            let mut tex = 0;
            if class == 5 {
                tex = m.u32(d.wrapping_add(0x224));
            } else {
                let t = m.u32(d.wrapping_add(0x20c));
                if t != 0 {
                    let mat = m.u32(t.wrapping_add((m.i16(d.wrapping_add(0x208)) as i32 as u32).wrapping_mul(4)));
                    if mat != 0 {
                        tex = m.u32(mat.wrapping_add(0x6fc));
                    }
                }
            }
            if tex != 0 {
                r = call(m, o, &SI_ED_GET_ELEM_FROM_TEXTURE3D, &[tex, aux]);
            }
            m.set_u32(cid_out, cid(m, 27));
        }
        2 => {
            r = call(m, o, &SI_ED_GET_ELEM_FROM_SCENE, &[aux]);
            m.set_u32(cid_out, cid(m, 3));
        }
        7 => {
            let d = data(m);
            r = call(m, o, &out::SI_ED_GET_ELEM_FROM_MODEL, &[d, aux]);
            m.set_u32(cid_out, cid(m, 8));
        }
        8 => {
            let d = data(m);
            let idx = (ty as u32).wrapping_sub(5);
            let model = |m: &mut Mem, o: &mut dyn Outside| -> u32 {
                m.set_u32(cid_out, m[crate::globals::SI_CID][8] as u32);
                call(m, o, &out::SI_ED_GET_ELEM_FROM_MODEL, &[d, aux])
            };
            r = match idx {
                4 | 7 if m.u32(d.wrapping_add(if idx == 4 { 0x50 } else { 0x58 })) != 0 => {
                    let r = call(m, o, &SI_ED_GET_ELEM_FROM_LATTICE, &[d, (idx == 7) as u32, aux]);
                    m.set_u32(cid_out, cid(m, 5));
                    r
                }
                0 | 1 => {
                    let r = call(m, o, &out::SI_ED_GET_ELEM_FROM_SHAPE, &[d, aux]);
                    m.set_u32(cid_out, cid(m, 21));
                    r
                }
                2 | 3 => {
                    let k = call(m, o, &out::SI_KLUSTER_GET_ACTIVE, &[d]);
                    let r = call(m, o, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[k, aux]);
                    m.set_u32(cid_out, cid(m, 2));
                    r
                }
                _ => model(m, o),
            };
        }
        _ => {
            r = call(m, o, &SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[node, cid_out, aux]);
        }
    }
    m.set_sp(sp);
    r
}

/// The tables of [`set_fcv_table_from_menu`], lifted from the listing by `re/tools/si-anim/lift_fcvtable.py`.
mod fcvtable_data {
    include!("fcvtable_data.rs");
}

/// [soft 0x082318cc si_setFcvTableFromMenu]
///
/// Flags the function curves of the attributes an item of the menus (`menu`, and its sub-item
/// `second`) stands for, in the attribute dictionary `SI_ATD`: a sequence of
/// `SI_attdictSetFcurveFlag(SI_ATD, id, flag)` calls and, for the item 0x2c0's sub-item 0 and a
/// few more, one `SI_attdictFlagAllMaterialFcurves`. The routine is a tree of compares on the two
/// arguments, with jump tables, and reads nothing else, so what it does is a table: which sequence
/// for which (item, sub-item); the table is lifted from the listing (`fcvtable_data.rs`) and held
/// against the original for every item from 0 to 0x7ff, every compared constant and its
/// neighbours, and the sub-items 0 to 40 and the extremes.
pub fn set_fcv_table_from_menu(m: &mut Mem, o: &mut dyn Outside, menu: i32, second: u32) {
    const ATD: u32 = 0x08781438;
    for &(lo, hi, subs) in fcvtable_data::MAP.iter() {
        if menu < lo || menu > hi {
            continue;
        }
        for &(slo, shi, seq) in subs.iter() {
            if second < slo || second > shi {
                continue;
            }
            for &(id, flag) in fcvtable_data::SEQS[seq as usize].iter() {
                if id == 0xffff && flag == 0xffff {
                    call(m, o, &SI_ATTDICT_FLAG_ALL_MATERIAL_FCURVES, &[]);
                } else {
                    call(m, o, &SI_ATTDICT_SET_FCURVE_FLAG, &[ATD, id as u32, flag as u32]);
                }
            }
        }
        return;
    }
}

/// [soft 0x08234e3c SI_setFcvTableEntriesFromChap]
///
/// Resets the function-curve table (`SI_fcvTableSet(0)`) and fills it for the chapter `chap` of
/// `SI_cid`: for models seven menu items (0x2bd, 0x1ab, 0x334, 0x348, 0x2c0, 0x31f, 0x31a); for
/// shapes, clusters and lattices the function-curve flags of their attributes in `SI_ATD`; for the
/// others one or two menu items.
pub fn set_fcv_table_entries_from_chap(m: &mut Mem, o: &mut dyn Outside, chap: u32) {
    const ATD: u32 = 0x08781438;
    let cid = |m: &Mem, i: usize| m[crate::globals::SI_CID][i] as u32;
    let menu = |m: &mut Mem, o: &mut dyn Outside, id: u32| {
        set_fcv_table_from_menu(m, o, id as i32, 0);
    };
    let flag = |m: &mut Mem, o: &mut dyn Outside, id: u32| {
        call(m, o, &SI_ATTDICT_SET_FCURVE_FLAG, &[ATD, id, 1]);
    };
    call(m, o, &SI_FCV_TABLE_SET, &[0]);
    if chap == cid(m, 8) {
        for id in [0x2bd, 0x1ab, 0x334, 0x348, 0x2c0, 0x31f, 0x31a] {
            menu(m, o, id);
        }
    } else if chap == cid(m, 21) {
        flag(m, o, 8);
        flag(m, o, 0x9a);
    } else if chap == cid(m, 2) {
        flag(m, o, 0xb7);
        flag(m, o, 0xb8);
    } else if chap == cid(m, 5) {
        flag(m, o, 0x42);
        flag(m, o, 0x43);
    } else if chap == cid(m, 1) {
        menu(m, o, 0x2be);
    } else if chap == cid(m, 6) {
        menu(m, o, 0x2bf);
    } else if chap == cid(m, 7) {
        menu(m, o, 0x2c0);
    } else if chap == cid(m, 26) {
        menu(m, o, 0x31f);
    } else if chap == cid(m, 27) {
        menu(m, o, 0x31a);
    } else if chap == cid(m, 3) {
        menu(m, o, 0x31b);
        menu(m, o, 0x307);
    } else if chap == cid(m, 28) {
        menu(m, o, 0x329);
    }
}

/// [soft 0x082357a0 traitemenu]
///
/// The command of a menu item of the function-curve menus (`menu`, with `x` the graph node the
/// user clicked or 0, `ty` the item's sub-type): clears the function-curve table, takes the graph
/// type (`getGraftypeFromMenu`) and the operation (`getFcvOperFromMenu`: 0 select, 1 copy, 2
/// reset) of the item, and for a copy first the element of `x` (`si_getDicInfoFromGrafnode`; none:
/// done). Then every node of the current list is tested (`okGrafType`, its data, and per operation
/// its selection mode at +8 and `branchSelectedFcv`) and, if it takes part, its element is
/// taken, the table is set for the item (`si_setFcvTableFromMenu`; the light product asks
/// `CheckForAllowedFCurves` first) and the operation is made on its element: copy (after
/// `cpyValidateElem`), select, or reset. A select first clears the selected curves of the scene once
/// (`SI_clearSelectedFcv`). With no node taking part the status text 0x086986f8 is shown. After
/// a select that selected nothing the table is searched for the attributes flagged as curves
/// (`SI_fcvGetRelInfo`, `SI_attdictGetFcurveFlag`, 0 to 0x2a6) and the status line says which relation
/// they belong to (if just one) or that several are; a select that selected something makes the
/// function-curve window inactive; a reset redraws the views.
pub fn traitemenu(m: &mut Mem, o: &mut dyn Outside, x: u32, menu: i32, ty: u32) {
    let sp = m.sp();
    let (l6c, l68, l70, l74) = (m.local(4), m.local(4), m.local(4), m.local(4));
    let (l78, l7c) = (m.local(4), m.local(4));
    let buf = m.local(0x64);
    call(m, o, &SI_FCV_TABLE_SET, &[0]);
    let gt = get_graftype_from_menu(menu);
    let oper = get_fcv_oper_from_menu(menu);
    let mut l88 = 0;
    'body: {
        if oper == 1 {
            l88 = get_dic_info_from_grafnode(m, o, x, menu, ty as i32, l6c, l68);
            if l88 == 0 {
                break 'body;
            }
        }
        let (mut taken, mut selected) = (0u32, 0u32);
        let mut esi = m.u32(graflist(m).wrapping_add(4));
        while esi != 0 {
            let mut l84 = 0;
            let mut bx = false;
            if ok_graf_type(m, esi, gt as u32) as u16 != 0 {
                bx = m.u32(esi.wrapping_add(4)) != 0;
            }
            let w8 = m.u32(esi.wrapping_add(8));
            match oper {
                1 => bx = bx && w8 != 0 && w8 != 2,
                0 => {
                    bx = if bx {
                        if m[BRANCH_SELECTED_FCV] != 0 {
                            w8 != 0 || gt == 0
                        } else {
                            w8 == 1 || w8 == 3 || gt == 0
                        }
                    } else {
                        gt == 0
                    };
                }
                2 => {
                    bx = bx && (gt == 0 || (w8 != 0 && w8 != 2));
                }
                _ => {}
            }
            if oper == 0 && bx && selected == 0 && m[SI_CLEAR_SELECTED_FCV] != 0 {
                call(m, o, &SI_FCV_SCNGL_CLEAR, &[]);
            }
            if (0..=2).contains(&oper) && bx {
                l84 = get_dic_info_from_grafnode(m, o, esi, menu, ty as i32, l74, l70);
            }
            if bx && l84 != 0 {
                let allowed = m[SI_LIGHT_LICENSE] != 0 && call(m, o, &CHECK_FOR_ALLOWED_FCURVES, &[menu as u32, esi]) as u16 != 0;
                if !allowed {
                    call(m, o, &SI_FCV_TABLE_SET, &[0]);
                    set_fcv_table_from_menu(m, o, menu, ty);
                    taken += 1;
                    let dict = crate::globals::cur_dict(m);
                    let (v74, v70) = (m.u32(l74), m.u32(l70));
                    match oper {
                        1 => {
                            if cpy_validate_elem(m, o, menu, ty as i32, l84, esi, x, v74) as u16 != 0 {
                                let (v68, v70) = (m.u32(l68), m.u32(l70));
                                call(m, o, &SI_ELEMDICT_COPY_ELEM_ANIM, &[dict, l88, l84, v74, v68, v70]);
                            }
                        }
                        0 => {
                            selected = selected.wrapping_add(call(m, o, &SI_ELEMDICT_SELECT_ELEM_ANIM, &[dict, l84, v74, v70]));
                        }
                        2 => {
                            call(m, o, &SI_ELEMDICT_RESET_ELEM_ANIM, &[dict, l84, v74, v70, 1]);
                        }
                        _ => {}
                    }
                }
            }
            esi = m.u32(esi.wrapping_add(0x1c));
        }
        if taken == 0 {
            status(m, o, 0x086986f8, 0);
            break 'body;
        }
        if oper == 0 {
            if selected != 0 {
                call(m, o, &SI_WINDOW_SET_FCURVE_ACTIVE, &[0]);
            } else {
                let (mut count, mut last, mut rel_seen) = (0u32, 0u32, u32::MAX);
                for id in 0..=0x2a6u32 {
                    call(m, o, &SI_FCV_GET_REL_INFO, &[id, l7c, l78, l78]);
                    if call(m, o, &SI_ATTDICT_GET_FCURVE_FLAG, &[0x08781438, id]) as u16 != 0 {
                        let rel = m.u32(l7c);
                        if rel != rel_seen {
                            count += 1;
                            last = id;
                            rel_seen = rel;
                        }
                    }
                }
                let text = if count == 1 {
                    call(m, o, &SI_FCV_GET_REL_INFO, &[last, l7c, l78, l78]);
                    let name = call(m, o, &SI_RELDICT_GET_REL_NAME, &[SI_RD, m.u32(l7c)]);
                    let mut t = m.cstr(0x08698713);
                    let at = t.windows(2).position(|w| w == b"%s").unwrap_or(t.len());
                    let tail = t.split_off((at + 2).min(t.len()));
                    t.truncate(at);
                    t.extend(m.cstr(name));
                    t.extend(tail);
                    t
                } else {
                    m.cstr(0x0869872c)
                };
                m.put_cstr(buf, &text);
                status(m, o, buf, 0);
            }
        } else if oper == 2 {
            call(m, o, &SI_REFRESH_VIEW, &[1, 1]);
        }
    }
    m.set_sp(sp);
}

#[cfg(test)]
mod rt_tests {
    use super::*;

    /// With an install: the constraint table loads (checked against our checksum) and the
    /// `%s` formatter fills a made-up format.
    #[test]
    fn constraint_table_and_format() {
        si_rt::skip_without_install!();
        assert_eq!(CONSTRAINT_GROUPS.len(), 17);
        assert_eq!(CONSTRAINT_GROUPS[0].1, 1);
        assert_eq!(fmt_s(b"a %s b %s c", &[b"X", b"YY"]), b"a X b YY c");
        assert_eq!(fmt_s(si_rt::message(0x086983c0), &[b"A", b"B"]).len(), 32 - 4 + 2);
    }
}
