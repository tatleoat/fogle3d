// SPDX-License-Identifier: MIT
//! `scene.c`, second slice (finding 0030, section 13): the scene window's key-saving helpers, its
//! button handler, the function-curve list of the scene (`SI_scngl`) and the motion mode
//! setters, in the original's address order.
//!
//! Same conventions as [`crate::scene1`]: the program's screen, menus, dialogs and every routine
//! of another source file are reached through [`Outside`] by name and address; record layouts
//! are addressed by named offsets on the one image. Where the original reads a stack local it
//! never wrote (a mode of the active canvas outside 1 to 3), the port reads 0 and a comment
//! says so.

use crate::globals as g;
use crate::outside::{self as out, Outside, Routine};
use crate::scene1 as s1;
use si_mem::Mem;
use si_x87::{fmov, x};

macro_rules! routines {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine this module names.
        pub const ROUTINES: &[&Routine] = &[ $( &$konst ),* ];
    };
}

routines! {
    /// `SI_checkSupra (0x0841c020)`: whether the event is one of the supra (multi-selection) kind.
    SI_CHECK_SUPRA = "SI_checkSupra", 0x0841c020;
    /// `SI_srtHandle (0x083a69d8)`: the scale, rotation, translation handle of the global manipulator.
    SI_SRT_HANDLE = "SI_srtHandle", 0x083a69d8;
    /// `SI_DoubleClickStatus (0x08393a08)`: whether the last click was a double click.
    SI_DOUBLE_CLICK_STATUS = "SI_DoubleClickStatus", 0x08393a08;
    /// `SI_schematicAction (0x083507b8)`: a click in the schematic window.
    SI_SCHEMATIC_ACTION = "SI_schematicAction", 0x083507b8;
    /// `SI_sprdSheetAction (0x083543a0)`: a click in the spreadsheet window.
    SI_SPRD_SHEET_ACTION = "SI_sprdSheetAction", 0x083543a0;
    /// `SI_ptsSelect (0x0828f9bc)`: selects points (mode 0, 1 or 2).
    SI_PTS_SELECT = "SI_ptsSelect", 0x0828f9bc;
    /// `SI_refreshAll (0x08325dc8)`: refreshes everything.
    SI_REFRESH_ALL = "SI_refreshAll", 0x08325dc8;
    /// `SI_mdlrectzoom (0x0841da54)`: the rectangle zoom of the model window.
    SI_MDLRECTZOOM = "SI_mdlrectzoom", 0x0841da54;
    /// `SI_doCam (0x08363ca4)`: the camera mode of the event.
    SI_DO_CAM = "SI_doCam", 0x08363ca4;
    /// `SI_tumbling (0x08365dc4)`: camera tumbling.
    SI_TUMBLING = "SI_tumbling", 0x08365dc4;
    /// `SI_tracking (0x08365ddc)`: camera tracking.
    SI_TRACKING = "SI_tracking", 0x08365ddc;
    /// `SI_dolly (0x083660f8)`: camera dolly.
    SI_DOLLY = "SI_dolly", 0x083660f8;
    /// `SI_cameraRoll (0x083666f8)`: camera roll.
    SI_CAMERA_ROLL = "SI_cameraRoll", 0x083666f8;
    /// `SI_fcurveCopyCutPasteTag (0x08207664)`: copy, cut or paste of the tagged keys.
    SI_FCURVE_COPY_CUT_PASTE_TAG = "SI_fcurveCopyCutPasteTag", 0x08207664;
    /// `SI_fcurveMoveKey (0x08207abc)`: moves a key.
    SI_FCURVE_MOVE_KEY = "SI_fcurveMoveKey", 0x08207abc;
    /// `SI_fcurveSlopeManagement (0x08205974)`: the slope of a key.
    SI_FCURVE_SLOPE_MANAGEMENT = "SI_fcurveSlopeManagement", 0x08205974;
    /// `SI_editFcurvePoint (0x08207350)`: edits a point of a curve.
    SI_EDIT_FCURVE_POINT = "SI_editFcurvePoint", 0x08207350;
    /// `SI_fcurveInterpPoint (0x08205bc4)`: the interpolation of a point.
    SI_FCURVE_INTERP_POINT = "SI_fcurveInterpPoint", 0x08205bc4;
    /// `SI_fcurveTransScl (0x08207b3c)`: translates or scales the curve.
    SI_FCURVE_TRANS_SCL = "SI_fcurveTransScl", 0x08207b3c;
    /// `SI_fcurveTransSclTag (0x08207bb4)`: translates or scales the tagged keys.
    SI_FCURVE_TRANS_SCL_TAG = "SI_fcurveTransSclTag", 0x08207bb4;
    /// `SI_fcurveRotateSlopeZeroTag (0x08207c24)`: rotates the slope of the tagged keys / sets it to zero.
    SI_FCURVE_ROTATE_SLOPE_ZERO_TAG = "SI_fcurveRotateSlopeZeroTag", 0x08207c24;
    /// `si_copyModelKeyToBuffer (0x0820b668)`: copies a model's key to the buffer.
    SI_COPY_MODEL_KEY_TO_BUFFER = "si_copyModelKeyToBuffer", 0x0820b668;
    /// `si_copyLightKeyToBuffer (0x0820b794)`: copies a light's key to the buffer.
    SI_COPY_LIGHT_KEY_TO_BUFFER = "si_copyLightKeyToBuffer", 0x0820b794;
    /// `si_copyCameraKeyToBuffer (0x0820b870)`: copies a camera's key to the buffer.
    SI_COPY_CAMERA_KEY_TO_BUFFER = "si_copyCameraKeyToBuffer", 0x0820b870;
    /// `si_pasteModelKeyFromBuffer (0x0820b9bc)`: pastes a model's key from the buffer.
    SI_PASTE_MODEL_KEY_FROM_BUFFER = "si_pasteModelKeyFromBuffer", 0x0820b9bc;
    /// `si_pasteLightKeyFromBuffer (0x0820bad4)`: pastes a light's key from the buffer.
    SI_PASTE_LIGHT_KEY_FROM_BUFFER = "si_pasteLightKeyFromBuffer", 0x0820bad4;
    /// `si_pasteCameraKeyFromBuffer (0x0820bb90)`: pastes a camera's key from the buffer.
    SI_PASTE_CAMERA_KEY_FROM_BUFFER = "si_pasteCameraKeyFromBuffer", 0x0820bb90;
    /// `si_fcurveSetKeyTags (0x08201710)`: tags keys.
    SI_FCURVE_SET_KEY_TAGS = "si_fcurveSetKeyTags", 0x08201710;
    /// `SI_fcurveRectSetKeyTags (0x08201854)`: tags the keys in a rectangle.
    SI_FCURVE_RECT_SET_KEY_TAGS = "SI_fcurveRectSetKeyTags", 0x08201854;
    /// `si_fcurveSetKeyLocks (0x08201c04)`: locks keys.
    SI_FCURVE_SET_KEY_LOCKS = "si_fcurveSetKeyLocks", 0x08201c04;
    /// `si_fcurveRectSetKeyLocks (0x08201d5c)`: locks the keys in a rectangle.
    SI_FCURVE_RECT_SET_KEY_LOCKS = "si_fcurveRectSetKeyLocks", 0x08201d5c;
    /// `si_modelActive (0x0822b300)`: the active model.
    SI_MODEL_ACTIVE = "si_modelActive", 0x0822b300;
    /// `si_itemSetShapeKey (0x08242734)`: sets a shape key.
    SI_ITEM_SET_SHAPE_KEY = "si_itemSetShapeKey", 0x08242734;
    /// `SI_relaxConstraintByMouse (0x081dcc6c)`: relaxes a constraint by mouse.
    SI_RELAX_CONSTRAINT_BY_MOUSE = "SI_relaxConstraintByMouse", 0x081dcc6c;
    /// `SI_refreshSelected (0x08325d48)`: refreshes the selection.
    SI_REFRESH_SELECTED = "SI_refreshSelected", 0x08325d48;
    /// `SI_distanceInfo (0x08410a6c)`: the distance measure (takes the address of the event pointer).
    SI_DISTANCE_INFO = "SI_distanceInfo", 0x08410a6c;
    /// `SI_chnCopySelection (0x08571564)`: copies the chain selection.
    SI_CHN_COPY_SELECTION = "SI_chnCopySelection", 0x08571564;
    /// `si_animDialAssociate (0x0822da04)`: associates an animation dial.
    SI_ANIM_DIAL_ASSOCIATE = "si_animDialAssociate", 0x0822da04;
    /// `SI_selLightHandler (0x08281498)`: the light selection handler (mode 1 or 2).
    SI_SEL_LIGHT_HANDLER = "SI_selLightHandler", 0x08281498;
    /// `SI_edGetElemFromWave (0x080fd448)`: the element of a wave; stores its index through the second argument.
    SI_ED_GET_ELEM_FROM_WAVE = "SI_edGetElemFromWave", 0x080fd448;
    /// `SI_elemdictRemoveUncompleteRelPair (0x0813382c)`: takes a half-made relation pair out of a dictionary.
    SI_ELEMDICT_REMOVE_UNCOMPLETE_REL_PAIR = "SI_elemdictRemoveUncompleteRelPair", 0x0813382c;
    /// `SI_pathElementSet (0x082200c0)`: sets the path element of a node.
    SI_PATH_ELEMENT_SET = "SI_pathElementSet", 0x082200c0;
    /// `SI_edGetElemFromLight (0x080fd0bc)`: the element of a light; stores its index through the second argument.
    SI_ED_GET_ELEM_FROM_LIGHT = "SI_edGetElemFromLight", 0x080fd0bc;
    /// `SI_edGetElemFromCamera (0x080fcfb0)`: the element of a camera; stores its index through the second argument.
    SI_ED_GET_ELEM_FROM_CAMERA = "SI_edGetElemFromCamera", 0x080fcfb0;
    /// `SI_savekeyLit (0x0821bef4)`: sets a key on a light's parameter.
    SI_SAVEKEY_LIT = "SI_savekeyLit", 0x0821bef4;
    /// `SI_savekeySingleCam (0x0821b6c4)`: sets a key on a camera's parameter.
    SI_SAVEKEY_SINGLE_CAM = "SI_savekeySingleCam", 0x0821b6c4;
    /// `SI_saveKeyWaveTranslate (0x084c57f4)`: sets keys on a wave's translation.
    SI_SAVE_KEY_WAVE_TRANSLATE = "SI_saveKeyWaveTranslate", 0x084c57f4;
    /// `SI_saveKeySingleWaveFcv (0x084c546c)`: sets a key on one curve of a wave.
    SI_SAVE_KEY_SINGLE_WAVE_FCV = "SI_saveKeySingleWaveFcv", 0x084c546c;
    /// `SI_numericSrtHandle (0x083a6b34)`: the numeric entry of the scale, rotation, translation panel.
    SI_NUMERIC_SRT_HANDLE = "SI_numericSrtHandle", 0x083a6b34;
    /// `SI_displayMouse (0x083cc630)`: shows the three mouse-button texts.
    SI_DISPLAY_MOUSE = "SI_displayMouse", 0x083cc630;
    /// `SI_statusDisplayMouse (0x083cc650)`: shows the three mouse-button texts in the status area.
    SI_STATUS_DISPLAY_MOUSE = "SI_statusDisplayMouse", 0x083cc650;
    /// `getbutton (0x0860e684)`: the state of a device button.
    GETBUTTON = "getbutton", 0x0860e684;
    /// `si_playctrlDialogBox (0x0820fa40)`: the playback control dialog (a short result).
    SI_PLAYCTRL_DIALOG_BOX = "si_playctrlDialogBox", 0x0820fa40;
    /// `SI_controlOff (0x0837efb0)`: turns a control off.
    SI_CONTROL_OFF = "SI_controlOff", 0x0837efb0;
    /// `SI_setSingleSelection (0x08181028)`: puts the graph lists in single-selection mode with a refresh function.
    SI_SET_SINGLE_SELECTION = "SI_setSingleSelection", 0x08181028;
    /// `SI_setMultiSelection (0x0818112c)`: puts the graph lists in multi-selection mode with a refresh function.
    SI_SET_MULTI_SELECTION = "SI_setMultiSelection", 0x0818112c;
    /// `SI_getCurrentSwiftKey (0x08393a18)`: the swift key pressed (-1 for none).
    SI_GET_CURRENT_SWIFT_KEY = "SI_getCurrentSwiftKey", 0x08393a18;
    /// `SI_getQuickDescription (0x083a9608)`: the text of a swift key.
    SI_GET_QUICK_DESCRIPTION = "SI_getQuickDescription", 0x083a9608;
    /// `SI_changeCurrentSwiftKeyModule (0x08393a28)`.
    SI_CHANGE_CURRENT_SWIFT_KEY_MODULE = "SI_changeCurrentSwiftKeyModule", 0x08393a28;
    /// `SI_dialogRequest2 (0x08141d5c)`: a request dialog with two buttons (a short result).
    SI_DIALOG_REQUEST2 = "SI_dialogRequest2", 0x08141d5c;
    /// `SI_fcurveRemoveKeyIndex (0x081ff92c)`: removes a curve's key by its 1-based index.
    SI_FCURVE_REMOVE_KEY_INDEX = "SI_fcurveRemoveKeyIndex", 0x081ff92c;
    /// `SI_graflistAddData (0x0832d240)`: adds a node of a type with data to a graph list; the node.
    SI_GRAFLIST_ADD_DATA = "SI_graflistAddData", 0x0832d240;
    /// `SI_fcvSnapshot (0x0820ab4c)`: takes a snapshot of a curve.
    SI_FCV_SNAPSHOT = "SI_fcvSnapshot", 0x0820ab4c;
    /// `SI_selectGrafnode (0x08333df8)`: selects a graph node.
    SI_SELECT_GRAFNODE = "SI_selectGrafnode", 0x08333df8;
    /// `SI_graflistRemoveGrafnode (0x0832c9ac)`: removes a node from a graph list.
    SI_GRAFLIST_REMOVE_GRAFNODE = "SI_graflistRemoveGrafnode", 0x0832c9ac;
    /// `SI_fcvFreeze (0x0820ab28)`.
    SI_FCV_FREEZE = "SI_fcvFreeze", 0x0820ab28;
    /// `SI_fcvEqual (0x082030d8)`: whether a curve equals a snapshot (a short result).
    SI_FCV_EQUAL = "SI_fcvEqual", 0x082030d8;
    /// `SI_fcvScrollBuild (0x083978d8)`: rebuilds the curve window's scroll.
    SI_FCV_SCROLL_BUILD = "SI_fcvScrollBuild", 0x083978d8;
    /// `SI_myclearSelect (0x08335040)`.
    SI_MYCLEAR_SELECT = "SI_myclearSelect", 0x08335040;
    /// The C library's clock call (a PLT import, `plt:time`).
    TIME = "plt:time", 0x080c4328;
    /// `getvaluator (0x0860e6a8)`: the GL library's device value (a short).
    GETVALUATOR = "getvaluator", 0x0860e6a8;
    /// `SI_viewGrafportSelect (0x083d7120)`: the graf port under a point; fills a record through the second argument.
    SI_VIEW_GRAFPORT_SELECT = "SI_viewGrafportSelect", 0x083d7120;
    /// `SI_fcurveSetInterp (0x08202128)`: sets a curve's interpolation.
    SI_FCURVE_SET_INTERP = "SI_fcurveSetInterp", 0x08202128;
    /// `SI_callRefreshElementByFcurve (0x0820b16c)`.
    SI_CALL_REFRESH_ELEMENT_BY_FCURVE = "SI_callRefreshElementByFcurve", 0x0820b16c;
    /// `SI_superModeOff (0x0837fae0)`: turns the super mode off (a short argument).
    SI_SUPER_MODE_OFF = "SI_superModeOff", 0x0837fae0;
    /// `SI_schemTurnOffOpnCls (0x083508ac)`.
    SI_SCHEM_TURN_OFF_OPN_CLS = "SI_schemTurnOffOpnCls", 0x083508ac;
}

si_mem::globals! {
    /// The active canvas (a view record): its half word at +0x26 is the mode (1 to 3; 3 is the
    /// motion window), the half words at +0x28, +0x2a and +0x2c flag the X, Y and Z channels.
    SI_ACTIVE_CANVAS: u32 = 0x087817f4, "SI_activeCanvas";
    /// The base colour of a selected curve.
    SI_SELECT7_WM: u16 = 0x08781638, "SI_select7WM";
    /// The curve colour 1 (the default).
    SI_FCURVE_COLOR1: u16 = 0x0878145c, "SI_fcurveColor1";
    /// The curve colour 2.
    SI_FCURVE_COLOR2: u16 = 0x0878145e, "SI_fcurveColor2";
    /// The curve colour 3.
    SI_FCURVE_COLOR3: u16 = 0x08781460, "SI_fcurveColor3";
    /// The copy mode of the function-curve window.
    FCV_COPY_MODE: u16 = 0x08697d84, "fcvCopyMode";
    /// The scene file's name (a block of the heap).
    SI_SCENE_FILE: u32 = 0x086ceba4, "SI_sceneFile";
    /// The last shape state `SI_setInterpDot` showed.
    PREV_SHAP_STATE: i32 = 0x08699088, "prevShapState.352";
    /// The last cluster state `SI_setInterpDot` showed.
    PREV_CLUS_STATE: i32 = 0x0869908c, "prevClusState.353";
    /// The device button last pressed on the tablet.
    SI_TABLET_BUTTON: i32 = 0x086794b4, "SI_tabletButton";
    /// The menu of the item being copied (`copyMenuId`).
    COPY_MENU_ID: i32 = 0x0878fb44, "copyMenuId";
    /// The item being copied (`copyMenuItem`).
    COPY_MENU_ITEM: u32 = 0x0878fb48, "copyMenuItem";
    /// The constraint type of the motion window.
    MOTION_CONSTRAINT_TYPE: i16 = 0x0878fb4c, "motionConstraintType";
    /// Non-zero while the schematic window is in its own mode.
    SI_SCHEMATIC_MODE: u16 = 0x0868b0c8, "SI_schematicMode";
    /// Set in the renderer that stands alone (no screen).
    SI_STANDALONE_RENDERER: u16 = 0x08679400, "SI_standaloneRenderer";
}

fn call(m: &mut Mem, o: &mut dyn Outside, r: &'static Routine, args: &[u32]) -> u32 {
    o.call(m, r, args)
}

/// `SI_menuStateSet(1, a, b, item, 0, value)` as the first slice makes it.
fn menu_state(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, item: u32, value: u32) {
    call(m, o, &s1::SI_MENU_STATE_SET, &[1, a, b, item, 0, value]);
}

/// A float moved through the x87 unit by `fld m32` / `fstp m32` (what the original does to pass
/// a float it holds in a local): the bits of the word, a signalling NaN come out quiet.
fn fl(w: u32) -> u32 {
    fmov(f32::from_bits(w)).to_bits()
}

/// The current element dictionary: `SI_elemDicts + SI_procData->dict × 0x70`, computed as `(d × 7) × 16`.
fn cur_dict(m: &Mem) -> u32 {
    g::cur_dict(m)
}

/// The scene's function-curve list and its first node (the word at +4).
fn scngl_first(m: &Mem) -> (u32, u32) {
    let l = m[g::SI_SCNGL];
    (l, m.u32(l.wrapping_add(4)))
}

/// [soft 0x08236a2c savekeyOnModelSelectedSRT]
///
/// Sets keys on a model's scale (canvas mode 1), rotation (mode 2: the model's angles times
/// 180/pi, computed in the register and rounded at the store) or translation (mode 3: the
/// local or the global values; when the model's element has a curve of type 0x70 the whole
/// triple is keyed at once) for the channels the canvas flags. A canvas mode outside 1 to 3
/// leaves the original with unwritten locals; the port uses zeros there.
pub fn savekey_on_model_selected_srt(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let canvas = m[SI_ACTIVE_CANVAS];
    let mode = m.u16(canvas.wrapping_add(0x26));
    let (mut l20, mut l1c) = (0u32, 0u32);
    let mut v = [0u32; 3];
    match mode {
        1 => {
            l20 = 1;
            l1c = 0x64;
            v = [m.u32(model + 0x100), m.u32(model + 0x104), m.u32(model + 0x108)];
        }
        2 => {
            let c = x(57.29577951308232f64);
            l20 = 4;
            l1c = 0x65;
            v[0] = (c * x(m.f32(model + 0x10c))).to_f32().to_bits();
            v[1] = (c * x(m.f32(model + 0x110))).to_f32().to_bits();
            v[2] = (c * x(m.f32(model + 0x114))).to_f32().to_bits();
        }
        3 => {
            let dict = cur_dict(m);
            let elem = call(m, o, &out::SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict, m.u32(0x08781680), model, 0]);
            if elem != 0 {
                let fcv = call(m, o, &out::SI_ED_FIND_FCV, &[elem, 1, 0x70]);
                if fcv != 0 {
                    call(m, o, &out::SI_SAVEKEY_MODEL, &[model, 1, 0x70, 7, 0, 0, 0, 0]);
                    return;
                }
            }
            l20 = 0x8b;
            l1c = 0x71;
            let base = if m.u32(model + 0x30) == 5 || m.u32(model + 0x2ec) == 3 { 0x1b8 } else { 0x158 };
            v = [m.u32(model + base), m.u32(model + base + 4), m.u32(model + base + 8)];
        }
        _ => {}
    }
    let canvas = m[SI_ACTIVE_CANVAS];
    let (fy, fz, fx) = (m.u16(canvas + 0x2a), m.u16(canvas + 0x2c), m.u16(canvas + 0x28));
    if fx != 0 {
        call(m, o, &out::SI_SAVEKEY_MODEL, &[model, 1, l1c, l20, 1, fl(v[0]), 0, 0]);
    }
    if fy != 0 {
        call(m, o, &out::SI_SAVEKEY_MODEL, &[model, 1, l1c, l20.wrapping_add(1), 1, fl(v[1]), 0, 0]);
    }
    if fz != 0 {
        call(m, o, &out::SI_SAVEKEY_MODEL, &[model, 1, l1c, l20.wrapping_add(2), 1, fl(v[2]), 0, 0]);
    }
}

/// [soft 0x08236c14 savekeyOnLightSelectedSRT]
///
/// The same for a light node (type 7 is a light's interest, else its position): only in the
/// motion window (canvas mode 3), else the status line says "not available" (text
/// 0x08698840, level 3). When the light's element has the curve the whole triple is keyed at
/// once; otherwise one key per flagged channel.
pub fn savekey_on_light_selected_srt(m: &mut Mem, o: &mut dyn Outside, node: u32) {
    let canvas = m[SI_ACTIVE_CANVAS];
    let data = m.u32(node.wrapping_add(4));
    if (m.u16(canvas.wrapping_add(0x26)) as i16) != 3 {
        call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08698840, 3]);
        return;
    }
    let sp = m.sp();
    let l10 = m.local(4);
    let elem = call(m, o, &SI_ED_GET_ELEM_FROM_LIGHT, &[data, l10]);
    let interest = m.u32(node) == 7;
    let (mut esi, mut edi) = if interest { (0x842, 0x20) } else { (0x840, 0x1f) };
    if elem != 0 {
        let fcv = call(m, o, &out::SI_ED_FIND_FCV, &[elem, 1, esi]);
        if fcv != 0 {
            call(m, o, &SI_SAVEKEY_LIT, &[data, 1, esi, edi, 0, 0]);
            m.set_sp(sp);
            return;
        }
    }
    let base = if interest {
        edi = 0x97;
        esi = 0x843;
        0x58
    } else {
        edi = 0x94;
        esi = 0x841;
        0x34
    };
    let v = [m.u32(data + base), m.u32(data + base + 4), m.u32(data + base + 8)];
    let canvas = m[SI_ACTIVE_CANVAS];
    let (fy, fz, fx) = (m.u16(canvas + 0x2a), m.u16(canvas + 0x2c), m.u16(canvas + 0x28));
    if fx != 0 {
        call(m, o, &SI_SAVEKEY_LIT, &[data, 0, esi, edi, fl(v[0]), 0]);
    }
    if fy != 0 {
        call(m, o, &SI_SAVEKEY_LIT, &[data, 0, esi, edi + 1, fl(v[1]), 0]);
    }
    if fz != 0 {
        call(m, o, &SI_SAVEKEY_LIT, &[data, 0, esi, edi + 2, fl(v[2]), 0]);
    }
    m.set_sp(sp);
}

/// [soft 0x08236d8c savekeyOnCameraSelectedSRT]
///
/// The same for a camera node (type 4 is a camera's interest, else its position).
pub fn savekey_on_camera_selected_srt(m: &mut Mem, o: &mut dyn Outside, node: u32) {
    let canvas = m[SI_ACTIVE_CANVAS];
    let data = m.u32(node.wrapping_add(4));
    if (m.u16(canvas.wrapping_add(0x26)) as i16) != 3 {
        call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08698880, 3]);
        return;
    }
    let sp = m.sp();
    let l10 = m.local(4);
    let elem = call(m, o, &SI_ED_GET_ELEM_FROM_CAMERA, &[data, l10]);
    let interest = m.u32(node) == 4;
    let (mut esi, mut edi) = if interest { (0x458, 9) } else { (0x45a, 0xa) };
    if elem != 0 {
        let fcv = call(m, o, &out::SI_ED_FIND_FCV, &[elem, 1, esi]);
        if fcv != 0 {
            call(m, o, &SI_SAVEKEY_SINGLE_CAM, &[data, 1, esi, edi, 0, 0, 0]);
            m.set_sp(sp);
            return;
        }
    }
    let base = if interest {
        edi = 0x8e;
        esi = 0x459;
        0x10
    } else {
        edi = 0x91;
        esi = 0x45b;
        0x1c
    };
    let v = [m.u32(data + base), m.u32(data + base + 4), m.u32(data + base + 8)];
    let canvas = m[SI_ACTIVE_CANVAS];
    let (fy, fz, fx) = (m.u16(canvas + 0x2a), m.u16(canvas + 0x2c), m.u16(canvas + 0x28));
    if fx != 0 {
        call(m, o, &SI_SAVEKEY_SINGLE_CAM, &[data, 1, esi, edi, 1, fl(v[0]), 0]);
    }
    if fy != 0 {
        call(m, o, &SI_SAVEKEY_SINGLE_CAM, &[data, 1, esi, edi + 1, 1, fl(v[1]), 0]);
    }
    if fz != 0 {
        call(m, o, &SI_SAVEKEY_SINGLE_CAM, &[data, 1, esi, edi + 2, 1, fl(v[2]), 0]);
    }
    m.set_sp(sp);
}

/// [soft 0x08236f08 savekeyOnWaveSelectedSRT]
///
/// The same for a wave node: curve number 0xa3, 0xa6 or 0xa9 (canvas modes 1 to 3) and the
/// two after it; in the motion window, when the wave's element has the curve of type 0x264,
/// all three at once. A mode outside 1 to 3 leaves the original with a register it never
/// set (the caller's); the port uses 0.
pub fn savekey_on_wave_selected_srt(m: &mut Mem, o: &mut dyn Outside, node: u32) {
    let canvas = m[SI_ACTIVE_CANVAS];
    let data = m.u32(node.wrapping_add(4));
    let esi = match m.u16(canvas.wrapping_add(0x26)) as i16 {
        1 => 0xa3,
        2 => 0xa6,
        3 => {
            let dict = cur_dict(m);
            let elem = call(m, o, &out::SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict, m.u32(0x087816d0), data, 0]);
            if elem != 0 && call(m, o, &out::SI_ED_FIND_FCV, &[elem, 1, 0x264]) != 0 {
                call(m, o, &SI_SAVE_KEY_WAVE_TRANSLATE, &[data, 3, 0]);
                return;
            }
            0xa9
        }
        _ => 0,
    };
    let canvas = m[SI_ACTIVE_CANVAS];
    let (fy, fz, fx) = (m.u16(canvas + 0x2a), m.u16(canvas + 0x2c), m.u16(canvas + 0x28));
    if fx != 0 {
        call(m, o, &SI_SAVE_KEY_SINGLE_WAVE_FCV, &[data, esi]);
    }
    if fy != 0 {
        call(m, o, &SI_SAVE_KEY_SINGLE_WAVE_FCV, &[data, esi + 1]);
    }
    if fz != 0 {
        call(m, o, &SI_SAVE_KEY_SINGLE_WAVE_FCV, &[data, esi + 2]);
    }
}

/// A 16-bit variable of the binary with no symbol: set while a swift key's module is busy (the
/// button handler tests it).
const SWIFT_KEY_BUSY: u32 = 0x08781594;

/// The refresh function the single- and multi-selection modes are given.
const MOTION_REFRESH_MULTI_FN: u32 = 0x082312c0;

/// [soft 0x0823c4ac scnButton]
///
/// The scene window's button handler (the event's code is the half word at +4, its second code
/// the half word at +6): buttons 1 to 3 (the scale, rotation and translation panel) hand a
/// numeric entry (second code 4) to the panel; button 10 selects one item at a time and shows
/// its interpolation dot (the list's selected node when its type is 2 or 3, else none); 20
/// selects many and shows none; 0x2cb and 0x2ce show the mouse texts (the second with the
/// playback dialog, which refreshes the scene when it answers nonzero) and turn the control
/// off; any other code is a swift key: with none pressed nothing happens, with a module busy
/// the control goes off and the swift-key module changes, else the status line says the key
/// is not valid in motion.
pub fn scn_button(m: &mut Mem, o: &mut dyn Outside, ev: u32) {
    let code = m.u16(ev.wrapping_add(4)) as i16;
    match code {
        1..=3 => {
            if m.u16(ev.wrapping_add(6)) == 4 {
                call(m, o, &SI_NUMERIC_SRT_HANDLE, &[code as i32 as u32]);
            }
        }
        0x2cb => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x08699003, 0x08699002, 0x08699001]);
            call(m, o, &SI_CONTROL_OFF, &[ev, 1]);
        }
        0x2ce => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x08699012, 0x08699005, 0x08699004]);
            if call(m, o, &SI_PLAYCTRL_DIALOG_BOX, &[]) as u16 != 0 {
                call(m, o, &s1::SI_SCENE_REFRESH, &[]);
            }
            call(m, o, &SI_CONTROL_OFF, &[ev, 1]);
        }
        10 => {
            call(m, o, &SI_SET_SINGLE_SELECTION, &[MOTION_REFRESH_MULTI_FN]);
            let proc = m[g::SI_PROC_DATA];
            let list = m[s1::SI_GRAF_LISTS].wrapping_add(m.u32(proc.wrapping_add(4)).wrapping_mul(0x24));
            let sel = m.u32(list.wrapping_add(8));
            if m.u16(list) == 0 && sel != 0 && m.u32(sel).wrapping_sub(2) <= 1 {
                let data = m.u32(sel.wrapping_add(4));
                set_interp_dot(m, o, data);
            } else {
                set_interp_dot(m, o, 0);
            }
        }
        0x14 => {
            call(m, o, &SI_SET_MULTI_SELECTION, &[MOTION_REFRESH_MULTI_FN]);
            set_interp_dot(m, o, 0);
        }
        _ => {
            let key = call(m, o, &SI_GET_CURRENT_SWIFT_KEY, &[]);
            if key == 0xffff_ffff {
                return;
            }
            if m.u16(SWIFT_KEY_BUSY) == 0 {
                let sp = m.sp();
                let buf = m.local(0x100);
                let desc = call(m, o, &SI_GET_QUICK_DESCRIPTION, &[key]);
                let mut text = b"Swift key ".to_vec();
                text.extend(if desc == 0 { b"(null)".to_vec() } else { m.cstr(desc) });
                text.extend(b" not valid in Motion");
                m.put_cstr(buf, &text);
                call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[buf, 2]);
                m.set_sp(sp);
            } else {
                call(m, o, &SI_CONTROL_OFF, &[ev, 1]);
                call(m, o, &SI_CHANGE_CURRENT_SWIFT_KEY_MODULE, &[]);
            }
        }
    }
}

/// [soft 0x0823c64c deleteFcurveTagKeys]
///
/// For every function curve of the scene's list (a node of type 9 with a curve and a selected
/// word) with keys: counts the keys that carry the tag 1 (half word at +0x14 of the 0x24-byte
/// key) and, of them, the ones whose low nibble of the half word at +0x1c is 1; when there are
/// any of the latter asks whether to delete them as well (the dialog answers a short); then saves the
/// curve for undo and removes the tagged keys from the last to the first (all of them when the
/// answer was nonzero, else those whose nibble is not 1).
pub fn delete_fcurve_tag_keys(m: &mut Mem, o: &mut dyn Outside) {
    let mut answer: u16 = 1;
    // the count of confirmed tag keys is kept from one curve to the next (as in the original)
    let mut count = 0i32;
    let (_, mut node) = scngl_first(m);
    let sp = m.sp();
    let l4 = m.local(4);
    while node != 0 {
        let fcv = m.u32(node.wrapping_add(4));
        if m.u32(node) == 9 && fcv != 0 && m.u32(node.wrapping_add(8)) != 0 {
            let n = m.u32(fcv.wrapping_add(0xc)) as i32;
            let keys = m.u32(fcv.wrapping_add(8));
            if n > 0 && keys != 0 {
                for i in (0..n).rev() {
                    let k = keys.wrapping_add((i as u32).wrapping_mul(0x24));
                    if m.u16(k.wrapping_add(0x14)) == 1 && m.u16(k.wrapping_add(0x1c)) & 0xf == 1 {
                        count += 1;
                    }
                }
                if count > 0 {
                    answer = call(m, o, &SI_DIALOG_REQUEST2, &[0x08699065, 0x08699064, 0x08699048, 0x08699045, 0x08699041]) as u16;
                }
                let elem = call(m, o, &out::SI_ED_GET_ELEM_FROM_ANIM_FILE, &[fcv, l4]);
                call(m, o, &out::SI_FCURVE_SAVE_HISTORY, &[elem]);
                let mut b = m.u32(fcv.wrapping_add(0xc)) as i32 - 1;
                while b >= 0 {
                    let k = m.u32(fcv.wrapping_add(8)).wrapping_add((b as u32).wrapping_mul(0x24));
                    if m.u16(k.wrapping_add(0x14)) == 1 && (answer != 0 || m.u16(k.wrapping_add(0x1c)) & 0xf != 1) {
                        call(m, o, &SI_FCURVE_REMOVE_KEY_INDEX, &[fcv, (b + 1) as u32]);
                    }
                    b -= 1;
                }
            }
        }
        node = m.u32(node.wrapping_add(0x1c));
    }
    m.set_sp(sp);
}

/// The curve types whose list node gets `SI_fcurveColor2` added to `SI_select7WM` (read from the
/// jump table of `SI_fcvScnglAdd`).
const FCV_COLOR2: &[u32] = &[0x2, 0x5, 0x17, 0x2e, 0x31, 0x34, 0x3f, 0x4d, 0x50, 0x53, 0x56, 0x59, 0x5c, 0x5f, 0x62, 0x6c, 0x70, 0x73, 0x77, 0x7a, 0x7d, 0x80, 0x83, 0x86, 0x89, 0x8c, 0x8f, 0x92, 0x95, 0x98, 0xa4, 0xa7, 0xaa, 0xb4, 0xbb, 0xc9, 0xcf, 0xd2, 0xd5, 0xde, 0xe1, 0xe4, 0xe7, 0xea, 0xed, 0xf0, 0xf3, 0xf6, 0xf9, 0xfc, 0xff];
/// The curve types whose node gets `SI_fcurveColor3`; every other type (and any outside 2 to 570)
/// gets `SI_fcurveColor1`.
const FCV_COLOR3: &[u32] = &[0x3, 0x6, 0x18, 0x2f, 0x32, 0x35, 0x40, 0x4e, 0x51, 0x54, 0x57, 0x5a, 0x5d, 0x60, 0x63, 0x6d, 0x71, 0x74, 0x78, 0x7b, 0x7e, 0x81, 0x84, 0x87, 0x8a, 0x8d, 0x90, 0x93, 0x96, 0x99, 0xa5, 0xa8, 0xab, 0xb5, 0xbc, 0xca, 0xd0, 0xd3, 0xd6, 0xdf, 0xe2, 0xe5, 0xe8, 0xeb, 0xee, 0xf1, 0xf4, 0xf7, 0xfa, 0xfd, 0x100, 0x236, 0x23a];

/// [soft 0x0823c7a8 SI_fcvScnglAdd]
///
/// Puts a function curve in the scene's list: nothing for a null curve; if a node of type 9
/// already holds it, only its display name is made again (`SI_getFcurveDname`) unless it is
/// of type 0x236 or 0x23a, which keep theirs; else a node of type 9 is added (`SI_graflistAddData`),
/// the curve named (types 0x236 and 0x23a take the text through `sprintf("%s")` into the
/// curve's own name at +0x2e), given a snapshot when it has none (word at +0x54), the node's
/// colour (word at +0x12) set from `SI_select7WM` plus the colour of the curve's type; with a
/// third argument and an empty selection state (the list's first half word 0) and a curve whose
/// half word at +0x2c is set the node is selected; the list's `addScanner` marker follows.
pub fn fcv_scngl_add(m: &mut Mem, o: &mut dyn Outside, fcv: u32, name: u32, select: u32) {
    if fcv == 0 {
        return;
    }
    let (scngl, mut node) = scngl_first(m);
    while node != 0 {
        if m.u32(node) == 9 && m.u32(node.wrapping_add(4)) == fcv {
            let t = m.u32(fcv);
            if t != 0x236 && t != 0x23a {
                call(m, o, &out::SI_GET_FCURVE_DNAME, &[fcv, name]);
            }
            return;
        }
        node = m.u32(node.wrapping_add(0x1c));
    }
    let node = call(m, o, &SI_GRAFLIST_ADD_DATA, &[scngl, fcv, 9, 0]);
    let t = m.u32(fcv);
    if t == 0x236 || t == 0x23a {
        // sprintf(fcv + 0x2e, "%s", name)
        let text = m.cstr(name);
        m.put_cstr(fcv.wrapping_add(0x2e), &text);
    } else {
        call(m, o, &out::SI_GET_FCURVE_DNAME, &[fcv, name]);
    }
    if m.u32(fcv.wrapping_add(0x54)) == 0 {
        call(m, o, &SI_FCV_SNAPSHOT, &[fcv]);
    }
    let colour = if FCV_COLOR2.contains(&t) {
        m[SI_FCURVE_COLOR2]
    } else if FCV_COLOR3.contains(&t) {
        m[SI_FCURVE_COLOR3]
    } else {
        m[SI_FCURVE_COLOR1]
    };
    let c = m[SI_SELECT7_WM].wrapping_add(colour);
    m.set_u16(node.wrapping_add(0x12), c);
    let scngl = m[g::SI_SCNGL];
    if select != 0 && m.u16(scngl) == 0 && m.u16(fcv.wrapping_add(0x2c)) != 0 {
        call(m, o, &SI_SELECT_GRAFNODE, &[scngl, node, 0]);
    }
    add_scanner(m, o);
}

/// [soft 0x0823c8dc SI_fcvScnglBDelete]
///
/// `si_fcvScnglDelete` of the curve; answers the curve.
pub fn fcv_scngl_b_delete(m: &mut Mem, o: &mut dyn Outside, fcv: u32) -> u32 {
    fcv_scngl_delete(m, o, fcv);
    fcv
}

/// [soft 0x0823c8f8 si_fcvScnglDelete]
///
/// Removes the node of type 9 that holds the curve from the scene's list and freezes the curve
/// (only the first such node).
pub fn fcv_scngl_delete(m: &mut Mem, o: &mut dyn Outside, fcv: u32) {
    let scngl = m[g::SI_SCNGL];
    if fcv == 0 || scngl == 0 {
        return;
    }
    let mut node = m.u32(scngl.wrapping_add(4));
    while node != 0 {
        if m.u32(node) == 9 && m.u32(node.wrapping_add(4)) == fcv {
            call(m, o, &SI_GRAFLIST_REMOVE_GRAFNODE, &[scngl, node]);
            call(m, o, &SI_FCV_FREEZE, &[fcv]);
            return;
        }
        node = m.u32(node.wrapping_add(0x1c));
    }
}

/// [soft 0x0823c948 SI_fcvScnglClear]
///
/// Empties the scene's list: every node is taken out (the next one is read from the removed
/// node after the removal), the curves of type-9 nodes that equal their snapshot (word at
/// +0x54) frozen first; then the scroll is rebuilt, the marker node added again and, unless
/// the renderer stands alone, the list's display refreshed (mask 0x3f).
pub fn fcv_scngl_clear(m: &mut Mem, o: &mut dyn Outside) {
    let (_, mut node) = scngl_first(m);
    while node != 0 {
        if m.u32(node) == 9 {
            let fcv = m.u32(node.wrapping_add(4));
            let snap = m.u32(fcv.wrapping_add(0x54));
            if call(m, o, &SI_FCV_EQUAL, &[fcv, snap]) as u16 == 1 {
                call(m, o, &SI_FCV_FREEZE, &[fcv]);
            }
        }
        let scngl = m[g::SI_SCNGL];
        call(m, o, &SI_GRAFLIST_REMOVE_GRAFNODE, &[scngl, node]);
        node = m.u32(node.wrapping_add(0x1c));
    }
    call(m, o, &SI_FCV_SCROLL_BUILD, &[]);
    add_scanner(m, o);
    if m[SI_STANDALONE_RENDERER] == 0 {
        let scngl = m[g::SI_SCNGL];
        call(m, o, &s1::SI_REFRESH_SYSTEM, &[0, scngl, 0x3f]);
    }
}

/// [soft 0x0823c9d8 SI_flashObject]
///
/// Clears the selection, draws the current graph list once with nothing and once with the node
/// (the display function `SI_foolDisplay`, modes 1 and 2), swaps the buffers, waits for the
/// clock (`time`) to change and refreshes the display (mask 0x25). The second argument is not used but
/// as the place the first `time` stores into (the caller's own argument slot).
pub fn flash_object(m: &mut Mem, o: &mut dyn Outside, node: u32) {
    call(m, o, &SI_MYCLEAR_SELECT, &[]);
    let list = |m: &Mem| {
        let proc = m[g::SI_PROC_DATA];
        m[s1::SI_GRAF_LISTS].wrapping_add(m.u32(proc.wrapping_add(4)).wrapping_mul(0x24))
    };
    let l = list(m);
    call(m, o, &s1::SI_FOOL_DISPLAY, &[l, 0, 1, 0]);
    let l = list(m);
    call(m, o, &s1::SI_FOOL_DISPLAY, &[l, 0, 2, node]);
    call(m, o, &s1::SWAPBUFFERS, &[]);
    let sp = m.sp();
    let slot = m.local(4);
    call(m, o, &TIME, &[slot]);
    while call(m, o, &TIME, &[0]) == m.u32(slot) {}
    m.set_sp(sp);
    call(m, o, &s1::SI_REFRESH_SYSTEM, &[0, 0, 0x25]);
}

/// [soft 0x0823ca6c addScanner]
///
/// Makes sure the scene's list holds a node of type 10 (the scanner's marker): adds one (with no
/// data) when none is there.
pub fn add_scanner(m: &mut Mem, o: &mut dyn Outside) {
    let (scngl, mut node) = scngl_first(m);
    while node != 0 {
        if m.u32(node) == 0xa {
            return;
        }
        node = m.u32(node.wrapping_add(0x1c));
    }
    call(m, o, &SI_GRAFLIST_ADD_DATA, &[scngl, 0, 0xa, 0]);
}

/// [soft 0x0823caa0 SI_setInterpDot]
///
/// Shows, in the interpolation menu (0x2e5), the state of the shape item (0x33a) and the
/// cluster item (0x338) of a model: first the dots of the states shown last (`prevShapState`,
/// `prevClusState`; 0, 1 or 2) are cleared, then with a model the dot of its shape state (half
/// word at +0x2e0) is set and, when the model has both a shape list (+0x344) and a second one
/// (+0x348) and an active cluster with a key record (+0x18), the dot of that record's state
/// (half word at +8) in the cluster item. The states shown are remembered.
pub fn set_interp_dot(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let prev = m[PREV_SHAP_STATE];
    if (0..=2).contains(&prev) {
        menu_state(m, o, 0x2e5, 0x33a, prev as u32, 0);
    }
    let prev = m[PREV_CLUS_STATE];
    if (0..=2).contains(&prev) {
        menu_state(m, o, 0x2e5, 0x338, prev as u32, 0);
    }
    if model == 0 {
        return;
    }
    let s = m.u16(model.wrapping_add(0x2e0));
    if s <= 2 {
        menu_state(m, o, 0x2e5, 0x33a, s as u32, 1);
    }
    m[PREV_SHAP_STATE] = m.u16(model.wrapping_add(0x2e0)) as i16 as i32;
    if m.u32(model.wrapping_add(0x344)) == 0 || m.u32(model.wrapping_add(0x348)) == 0 {
        return;
    }
    let k = call(m, o, &out::SI_KLUSTER_GET_ACTIVE, &[model]);
    if k == 0 {
        return;
    }
    let rec = m.u32(k.wrapping_add(0x18));
    if rec == 0 {
        return;
    }
    let s = m.u16(rec.wrapping_add(8));
    if s <= 2 {
        menu_state(m, o, 0x2e5, 0x338, s as u32, 1);
    }
    m[PREV_CLUS_STATE] = m.u16(rec.wrapping_add(8)) as i16 as i32;
}

/// [soft 0x0823cc5c SI_sceneFileReset]
///
/// Frees the scene file's name and clears it.
pub fn scene_file_reset(m: &mut Mem) {
    let f = m[SI_SCENE_FILE];
    if f != 0 {
        m.free(f);
    }
    m[SI_SCENE_FILE] = 0;
}

/// [soft 0x0823cc8c inFcurveWindow]
///
/// Whether the pointer (the devices `SI_coordx` and `SI_coordy`, read as shorts) is over a
/// window whose mode record has the mode 0x7d4: `SI_viewGrafportSelect` of the active canvas
/// fills a record whose word at +0xc points to the window (its +0x2c the mode record).
pub fn in_fcurve_window(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let sp = m.sp();
    let rec = m.local(0x18);
    let x = call(m, o, &GETVALUATOR, &[m.u16(0x086792be) as u32]) as u16;
    let y = call(m, o, &GETVALUATOR, &[m.u16(0x086792c0) as u32]) as u16;
    let canvas = m[SI_ACTIVE_CANVAS];
    call(m, o, &SI_VIEW_GRAFPORT_SELECT, &[canvas, rec, x as i16 as i32 as u32, y as i16 as i32 as u32]);
    let win = m.u32(rec.wrapping_add(0xc));
    let modes = m.u32(win.wrapping_add(0x2c));
    let r = (m.u16(modes.wrapping_add(4)) == 0x7d4) as u32;
    m.set_sp(sp);
    r
}

/// [soft 0x0823ccec SI_setSelectedFcurveInterp]
///
/// Sets the interpolation of every selected curve of the scene's list (a node of type 9 with a
/// curve and a selected word): the curve is saved for undo first. Then both buffers' selection and
/// the elements by curve are refreshed.
pub fn set_selected_fcurve_interp(m: &mut Mem, o: &mut dyn Outside, interp: u16) {
    let (_, mut node) = scngl_first(m);
    let sp = m.sp();
    let l4 = m.local(4);
    while node != 0 {
        let fcv = m.u32(node.wrapping_add(4));
        if m.u32(node) == 9 && fcv != 0 && m.u32(node.wrapping_add(8)) != 0 {
            let elem = call(m, o, &out::SI_ED_GET_ELEM_FROM_ANIM_FILE, &[fcv, l4]);
            call(m, o, &out::SI_FCURVE_SAVE_HISTORY, &[elem]);
            call(m, o, &SI_FCURVE_SET_INTERP, &[fcv, interp as i16 as i32 as u32]);
        }
        node = m.u32(node.wrapping_add(0x1c));
    }
    m.set_sp(sp);
    call(m, o, &s1::SI_REFRESH_SELECTED_BOTH_BUFFERS, &[1]);
    call(m, o, &SI_CALL_REFRESH_ELEMENT_BY_FCURVE, &[]);
}

/// [soft 0x0823cd64 si_getSelectedFcurvePointer]
///
/// The curve of the one selected node of type 9 in the scene's list; with several selected the
/// "select only one" dialog (`motionMultiMessage`) and 0, with none (or a node without a curve)
/// the status line text 0x08699090 (level 3) and 0.
pub fn get_selected_fcurve_pointer(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let sp = m.sp();
    let l4 = m.local(4);
    let scngl = m[g::SI_SCNGL];
    let n = call(m, o, &s1::SI_GRAFLIST_TYPE_COUNT, &[scngl, 9, 0, l4, 0]) as i32;
    let first = m.u32(l4);
    m.set_sp(sp);
    if n > 1 {
        s1::motion_multi_message(m, o);
        return 0;
    }
    if first != 0 {
        let fcv = m.u32(first.wrapping_add(4));
        if fcv != 0 {
            return fcv;
        }
    }
    call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08699090, 3]);
    0
}

/// [soft 0x0823d110 SI_motSetEditKeyMode]
pub fn mot_set_edit_key_mode(m: &mut Mem, o: &mut dyn Outside) {
    scn_set_mode(m, o, 0x3e9, 1);
}

/// [soft 0x0823d12c SI_motSetCutPasteKeyMode]
pub fn mot_set_cut_paste_key_mode(m: &mut Mem, o: &mut dyn Outside) {
    scn_set_mode(m, o, 0x3e8, 1);
}

/// [soft 0x0823d148 SI_motSetSlopeManagementMode]
pub fn mot_set_slope_management_mode(m: &mut Mem, o: &mut dyn Outside) {
    scn_set_mode(m, o, 0x3ea, 1);
}

/// [soft 0x0823d164 SI_motSetSetKeyMode]
pub fn mot_set_set_key_mode(m: &mut Mem, o: &mut dyn Outside) {
    scn_set_mode(m, o, 0x3ec, 1);
}

/// [soft 0x0823d180 SI_motSetEditFcvMode]
pub fn mot_set_edit_fcv_mode(m: &mut Mem, o: &mut dyn Outside) {
    scn_set_mode(m, o, 0x3ed, 1);
}

/// [soft 0x0823d19c SI_motSetEditTagMode]
pub fn mot_set_edit_tag_mode(m: &mut Mem, o: &mut dyn Outside) {
    scn_set_mode(m, o, 0x3ee, 1);
}

/// [soft 0x0823d1b8 SI_motSetInterpManagementMode]
pub fn mot_set_interp_management_mode(m: &mut Mem, o: &mut dyn Outside) {
    scn_set_mode(m, o, 0x3eb, 1);
}

/// [soft 0x0823d1d4 SI_scnGetMode]
pub fn scn_get_mode(m: &Mem) -> u32 {
    m[s1::SCNMOD] as u32
}

/// [soft 0x0823d1e0 setCutPasteTKeyMode]
pub fn set_cut_paste_t_key_mode(m: &mut Mem, o: &mut dyn Outside) {
    scn_set_mode(m, o, 3, 1);
}

/// [soft 0x0823d1f8 scnSetMode]
///
/// Sets the scene window's mode, clears `fcvCopyMode`, turns the super mode off for the short
/// `x`, the schematic's open/close off, and shows the mode's status line (`SI_scnmodStatus`).
pub fn scn_set_mode(m: &mut Mem, o: &mut dyn Outside, mode: u32, x: u16) {
    m[s1::SCNMOD] = mode as i32;
    m[FCV_COPY_MODE] = 0;
    call(m, o, &SI_SUPER_MODE_OFF, &[x as i16 as i32 as u32]);
    call(m, o, &SI_SCHEM_TURN_OFF_OPN_CLS, &[]);
    scnmod_status(m, o);
}

/// [soft 0x0823d974 SI_motionDirectionFlagSet]
///
/// Sets `SI_shwDircns` to the low half word of the argument and shows it as the state of item 5
/// of menu 0x2d3 (the whole argument).
pub fn motion_direction_flag_set(m: &mut Mem, o: &mut dyn Outside, v: u32) {
    m[s1::SI_SHW_DIRCNS] = v as u16 as i16;
    menu_state(m, o, 0x2d3, 0x2d3, 5, v);
}

/// [soft 0x0823d22c SI_scnmodStatus]
///
/// Shows the status line and the mouse-button texts of the current scene mode (`scnmod`); a mode
/// with no entry shows nothing. In mode 0x1f the texts depend on whether button 6 or 5 is down.
pub fn scnmod_status(m: &mut Mem, o: &mut dyn Outside) {
    match m[s1::SCNMOD] {
        0x0 => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x086990a3, 0x4]);
            call(m, o, &SI_STATUS_DISPLAY_MOUSE, &[0x086990a6, 0x086990a5, 0x086990a4]);
        }
        0x1 => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x086990a7, 0x4]);
            call(m, o, &SI_STATUS_DISPLAY_MOUSE, &[0x086990ce, 0x086990c1, 0x086990b3]);
        }
        0x2 => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x086990db, 0x4]);
            call(m, o, &SI_STATUS_DISPLAY_MOUSE, &[0x086990f8, 0x086990f0, 0x086990e7]);
        }
        0x3 => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x08699126, 0x08699116, 0x08699104]);
        }
        0x3e8 => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x08699159, 0x08699149, 0x08699137]);
        }
        0x3e9 => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x08699180, 0x08699175, 0x0869916a]);
        }
        0x3ea => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086991ab, 0x0869919b, 0x08699189]);
        }
        0x3eb => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086991cb, 0x086991c4, 0x086991bb]);
        }
        0x3ec => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086991e7, 0x086991db, 0x086991d2]);
        }
        0x3ed => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x0869920b, 0x086991fd, 0x086991ef]);
        }
        0x3ee => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x08699248, 0x08699232, 0x0869921c]);
        }
        0x2e => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x08699286, 0x0869927a, 0x08699261]);
        }
        0x6 | 0x24 | 0x2a | 0x2f => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086992b2, 0x086992a7, 0x0869929e]);
        }
        0x7 => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086992d5, 0x086992c6, 0x086992bd]);
        }
        0x2b => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x086992e4, 0x4]);
            call(m, o, &SI_DISPLAY_MOUSE, &[0x0869930f, 0x08699305, 0x086992fc]);
        }
        0x2c => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08699320, 0x4]);
            call(m, o, &SI_DISPLAY_MOUSE, &[0x08699369, 0x0869935f, 0x08699356]);
        }
        0x18 => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x08699394, 0x08699381, 0x08699378]);
        }
        0x8 => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086993ba, 0x086993b0, 0x086993a7]);
        }
        0xe => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086993dd, 0x086993d3, 0x086993c4]);
        }
        0xf => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x0869940a, 0x086993fa, 0x086993e5]);
        }
        0x10 => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08699418, 0x4]);
            call(m, o, &SI_STATUS_DISPLAY_MOUSE, &[0x08699440, 0x08699432, 0x08699426]);
        }
        0x11 => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x0869944c, 0x4]);
            call(m, o, &SI_STATUS_DISPLAY_MOUSE, &[0x0869946d, 0x0869945f, 0x08699453]);
        }
        0x12 => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08699479, 0x4]);
            call(m, o, &SI_STATUS_DISPLAY_MOUSE, &[0x086994a0, 0x08699490, 0x08699482]);
        }
        0x19 => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086994c9, 0x086994be, 0x086994ae]);
        }
        0x1a => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086994f9, 0x086994e8, 0x086994d2]);
        }
        0x1f => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08699520, 4]);
            let pressed = call(m, o, &GETBUTTON, &[6]) != 0 || call(m, o, &GETBUTTON, &[5]) != 0;
            if pressed {
                call(m, o, &SI_DISPLAY_MOUSE, &[0x0869956e, 0x08699560, 0x08699557]);
            } else {
                call(m, o, &SI_DISPLAY_MOUSE, &[0x08699591, 0x08699583, 0x0869957a]);
            }
        }
        0x20 => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x0869959d, 0x4]);
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086995bd, 0x086995b3, 0x086995aa]);
        }
        0x21 => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x086995c1, 0x4]);
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086995e2, 0x086995d9, 0x086995ce]);
        }
        0x22 => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x086995e6, 0x4]);
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086995ff, 0x086995f8, 0x086995f3]);
        }
        0x23 => {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08699604, 0x4]);
            call(m, o, &SI_DISPLAY_MOUSE, &[0x0869961c, 0x08699615, 0x08699610]);
        }
        0x27 | 0x28 => {
            call(m, o, &SI_STATUS_DISPLAY_MOUSE, &[0x08699637, 0x0869962a, 0x08699621]);
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08699644, 0x0]);
        }
        0x25 => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x08699668, 0x0869964e, 0x08699645]);
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08699681, 0x4]);
        }
        0x26 => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x0869969f, 0x0869969e, 0x08699695]);
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x086996ae, 0x4]);
        }
        0x29 => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x086996d8, 0x086996ce, 0x086996c5]);
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x086996e2, 0x4]);
        }
        0x2d => {
            call(m, o, &SI_DISPLAY_MOUSE, &[0x08699706, 0x086996fc, 0x086996f3]);
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08699710, 0x4]);
        }
        _ => {}
    }
}

/// [soft 0x0823d9a0 CheckForAllowedFCurves]
///
/// Whether the curves of the menu item `ty` may be edited for the node: the node (types 2 and 3
/// only) leads to a material (items 0x2c0, 0x2ea, 0x703) or a 3-D texture (the items 0x2eb, 0x31a,
/// 0x321, 0x706, 0x707, and 0x323 through the material slot's own texture) and that element's
/// data has bit 4 in the byte at +0x17. Any other item, or a missing part, gives 0.
pub fn check_for_allowed_fcurves(m: &mut Mem, o: &mut dyn Outside, ty: u32, node: u32) -> u32 {
    let mut ecx = 0u32;
    if m.u32(node).wrapping_sub(2) <= 1 {
        ecx = m.u32(node.wrapping_add(4));
    }
    if ecx == 0 {
        return 0;
    }
    let sp = m.sp();
    let aux = m.local(4);
    let slot = |m: &Mem| {
        let t = m.u32(ecx.wrapping_add(0x20c));
        (t, t.wrapping_add((m.i16(ecx.wrapping_add(0x208)) as i32 as u32).wrapping_mul(4)))
    };
    let texture_of = |m: &mut Mem, o: &mut dyn Outside, arg: u32| call(m, o, &s1::SI_ED_GET_ELEM_FROM_TEXTURE3D, &[arg, aux]);
    let mut data = 0u32;
    match ty {
        0x2c0 | 0x2ea | 0x703 => {
            let (t, at) = slot(m);
            if t != 0 {
                let mat = m.u32(at);
                let e = call(m, o, &s1::SI_ED_GET_ELEM_FROM_MATERIAL, &[mat, aux]);
                if e != 0 {
                    data = m.u32(e.wrapping_add(8));
                }
            }
        }
        0x323 => {
            let (_, at) = slot(m);
            let mat = m.u32(at);
            if mat != 0 {
                let e = texture_of(m, o, m.u32(mat.wrapping_add(0x6fc)));
                if e != 0 {
                    data = m.u32(e.wrapping_add(8));
                }
            }
        }
        0x2eb | 0x31a | 0x321 | 0x706 | 0x707 => {
            let t = m.u32(ecx.wrapping_add(0x224));
            if t != 0 {
                let e = texture_of(m, o, t);
                if e != 0 {
                    data = m.u32(e.wrapping_add(8));
                }
            }
        }
        _ => {}
    }
    m.set_sp(sp);
    if data != 0 && m.u8(data.wrapping_add(0x17)) & 0x10 != 0 { 1 } else { 0 }
}

/// The address `SI_RD` (the dictionary of the relations of the path animation): passed by address.
const SI_RD: u32 = 0x087815f0;

/// [soft 0x0823cdb4 motionSubstitutePath]
///
/// With the node picked in the event's window (a model, camera, light or wave node whose data is
/// a path-driven one: the word at +0x30 is 6, or 9 with a filled list at +0x48), draws the graph
/// list once with it, swaps the buffers (the 200 ms `usleep` does nothing here) and refreshes
/// (mask 0x2d), then gives every node of the list that has its word at +8 set and a model,
/// camera, light or wave type the picked node's path: when the element has a curve of the path
/// kind (0x70, 0x458/0x45a, 0x840/0x842 or 0x264) the two half-made relation pairs (modes 1 and
/// 2) are taken out of the dictionary, the path word is replaced and `SI_pathElementSet` and
/// `SI_schemSetDefaultColor` are asked; with no curve the answer becomes 0. Nothing picked or a
/// wrong kind gives -1; otherwise 1 when every node had a curve. A node whose element is missing
/// is tried again for ever (as in the original).
pub fn motion_substitute_path(m: &mut Mem, o: &mut dyn Outside, event: u32) -> u32 {
    let mut failed = false;
    let node = call(m, o, &s1::SI_GL_SELECT, &[m.u32(event.wrapping_add(0xc))]);
    if node == 0 || m.u32(node).wrapping_sub(2) > 1 {
        return u32::MAX;
    }
    let data = m.u32(node.wrapping_add(4));
    if data == 0 {
        return u32::MAX;
    }
    let kind = m.u32(data.wrapping_add(0x30));
    if kind != 6 && kind != 9 {
        return u32::MAX;
    }
    if kind == 9 {
        let l = m.u32(data.wrapping_add(0x48));
        if l == 0 || (m.u32(l.wrapping_add(4)) as i32) < (m.u32(l) as i32) {
            return u32::MAX;
        }
    }
    let proc = m[g::SI_PROC_DATA];
    let list = m[s1::SI_GRAF_LISTS].wrapping_add(m.u32(proc.wrapping_add(4)).wrapping_mul(0x24));
    call(m, o, &s1::SI_FOOL_DISPLAY, &[list, 0, 2, node]);
    call(m, o, &s1::SWAPBUFFERS, &[]);
    call(m, o, &s1::SI_REFRESH_SYSTEM, &[0, 0, 0x2d]);
    let proc = m[g::SI_PROC_DATA];
    let mut esi = m.u32(m[s1::SI_GRAF_LISTS].wrapping_add(m.u32(proc.wrapping_add(4)).wrapping_mul(0x24)).wrapping_add(4));
    let sp = m.sp();
    let l4 = m.local(4);
    let l8 = m.local(4);
    while esi != 0 {
        if m.u32(esi.wrapping_add(8)) != 0 {
            let ty = m.u32(esi);
            if ty.wrapping_sub(2) <= 6 || ty == 0x13 || ty == 0xb {
                let ebx = m.u32(esi.wrapping_add(4));
                if ebx == 0 {
                    continue;
                }
                // (element, fcurve type, address of the path word, cid class of the owner)
                let (edi, fcv_ty, word, cls);
                match ty {
                    2 | 3 => {
                        edi = call(m, o, &out::SI_ED_GET_ELEM_FROM_MODEL, &[ebx, l4]);
                        if edi == 0 {
                            continue;
                        }
                        fcv_ty = 0x70;
                        word = ebx.wrapping_add(0x2c8);
                        cls = m[g::SI_CID][8] as u32;
                    }
                    4 | 5 => {
                        edi = call(m, o, &SI_ED_GET_ELEM_FROM_CAMERA, &[ebx, l4]);
                        if edi == 0 {
                            continue;
                        }
                        if ty == 4 {
                            fcv_ty = 0x458;
                            word = ebx.wrapping_add(0xa0);
                        } else {
                            fcv_ty = 0x45a;
                            word = ebx.wrapping_add(0xa4);
                        }
                        cls = m[g::SI_CID][1] as u32;
                    }
                    6 | 7 | 8 | 0x13 => {
                        edi = call(m, o, &SI_ED_GET_ELEM_FROM_LIGHT, &[ebx, l4]);
                        if edi == 0 {
                            continue;
                        }
                        if ty == 7 {
                            fcv_ty = 0x842;
                            word = ebx.wrapping_add(0x148);
                        } else {
                            fcv_ty = 0x840;
                            word = ebx.wrapping_add(0x144);
                        }
                        cls = m[g::SI_CID][6] as u32;
                    }
                    _ => {
                        edi = call(m, o, &SI_ED_GET_ELEM_FROM_WAVE, &[ebx, l4]);
                        if edi == 0 {
                            continue;
                        }
                        fcv_ty = 0x264;
                        word = m.u32(ebx.wrapping_add(0x74)).wrapping_add(0x2c8);
                        cls = m[g::SI_CID][28] as u32;
                    }
                }
                if call(m, o, &out::SI_ED_FIND_FCV, &[edi, 1, fcv_ty]) == 0 {
                    failed = true;
                } else {
                    let owner = m.u32(word);
                    call(m, o, &out::SI_ED_GET_ELEM_FROM_MODEL, &[owner, l8]);
                    let idx = m.u32(l8);
                    if (idx as i32) >= 0 {
                        for mode in [1u32, 2] {
                            let dict = g::cur_dict(m);
                            let elem = m.u32(l4);
                            let c8 = m[g::SI_CID][8] as u32;
                            call(
                                m,
                                o,
                                &SI_ELEMDICT_REMOVE_UNCOMPLETE_REL_PAIR,
                                &[dict, SI_RD, cls, c8, elem, idx, fcv_ty, mode, 0, u32::MAX],
                            );
                        }
                    }
                    m.set_u32(word, data);
                    let (a, b) = (m.u32(esi.wrapping_add(4)), m.u32(esi));
                    call(m, o, &SI_PATH_ELEMENT_SET, &[a, b, data]);
                    call(m, o, &s1::SI_SCHEM_SET_DEFAULT_COLOR, &[esi]);
                }
            }
        }
        esi = m.u32(esi.wrapping_add(0x1c));
    }
    m.set_sp(sp);
    call(m, o, &s1::SI_SCENE_REFRESH, &[]);
    if failed { 0 } else { 1 }
}

/// Writes `fmt` with its `%d` replaced by `n` into the buffer (the original's `sprintf`).
fn put_message(m: &mut Mem, fmt: u32, buf: u32, n: u32) {
    let mut text = m.cstr(fmt);
    let at = text.windows(2).position(|w| w == b"%d");
    if let Some(at) = at {
        let tail = text.split_off(at + 2);
        text.truncate(at);
        text.extend(si_mem::clib::decimal(n as i32));
        text.extend(tail);
    }
    m.put_cstr(buf, &text);
}

/// The float argument of the key copy and paste routines: the current frame over the frame rate,
/// the frame turned to a float first (the rounding in front of it does nothing to an integer).
fn frame_seconds(m: &Mem) -> u32 {
    let frame = m[g::SI_FRAME_CURRENT];
    (x(si_x87::F80::from(frame).to_f32()) / x(m[g::SI_FRAME_RATE])).to_f32().to_bits()
}

/// The graph list of the scene's process data: `SI_grafLists + SI_procData->list × 0x24`.
fn cur_list(m: &Mem) -> u32 {
    let proc = m[g::SI_PROC_DATA];
    m[s1::SI_GRAF_LISTS].wrapping_add(m.u32(proc.wrapping_add(4)).wrapping_mul(0x24))
}

/// The end of `scnWind` and of two of its modes: the interpolation dot follows the current
/// list's selected node when that is a model node (type 2 or 3), or is cleared.
fn interp_dot_from_list(m: &mut Mem, o: &mut dyn Outside) {
    let list = cur_list(m);
    if m.u16(list) == 0 {
        let node = m.u32(list.wrapping_add(8));
        if node != 0 && m.u32(node).wrapping_sub(2) <= 1 {
            let d = m.u32(node.wrapping_add(4));
            set_interp_dot(m, o, d);
            return;
        }
    }
    set_interp_dot(m, o, 0);
}

/// What a click does in the key buffer: the node's kind for the copy, cut and paste of keys.
#[derive(Clone, Copy)]
enum KeyKind {
    Model,
    Camera,
    Light,
    None,
}

fn key_kind(ty: u32) -> KeyKind {
    match ty {
        2 | 3 => KeyKind::Model,
        4 | 5 => KeyKind::Camera,
        6 | 7 | 8 | 19 => KeyKind::Light,
        _ => KeyKind::None,
    }
}

/// [soft 0x08235bfc scnWind]
///
/// The clicks of the scene window, by the window's mode and `scnmod`. A "supra" event
/// (`SI_checkSupra`) only refreshes the multiple selection and the interpolation dot. A click
/// with a channel flag set in the active canvas goes to the SRT handle (the mode 0 to 6 from
/// which of the X, Y, Z flags overlap). The schematic (0x7d7) and spreadsheet (0x7da) windows
/// have their own handlers; with `fcvCopyMode` set the next click on a node of the right kind
/// copies the menu action to it; otherwise button 0x65 or the tablet button ends modes 6 to 8,
/// 0x18, 0x1f, 0x24 to 0x2d, 0x2f and 0x3ec at once and the rest goes by `scnmod`: the
/// camera and pick modes call their handlers, the function-curve modes (0x3e8 to 0x3ee) theirs,
/// mode 3 copies (code 0x67), cuts (0x66) and pastes (0x65) the keys of the one selected node
/// (showing "N keys copied." and so on), modes 0xe, 0xf, 0x19, 0x1a tag and lock keys.
/// A mode word outside the table does nothing. The mode-flag word of the SRT handle is not set
/// when all three flags are gone after `SI_checkSupra` (an uninitialised local in the original;
/// 0 here), and the count shown for a node that is not a model, camera or light is 0 (also an
/// uninitialised local).
pub fn scn_wind(m: &mut Mem, o: &mut dyn Outside, event: u32) {
    let sp = m.sp();
    scn_wind_inner(m, o, event);
    m.set_sp(sp);
}

fn scn_wind_inner(m: &mut Mem, o: &mut dyn Outside, event: u32) {
    let canvas0 = m[SI_ACTIVE_CANVAS];
    let win = m.u32(event.wrapping_add(0xc));
    let (cx0, cy0, cz0) = (m.u16(canvas0.wrapping_add(0x28)), m.u16(canvas0.wrapping_add(0x2a)), m.u16(canvas0.wrapping_add(0x2c)));
    if call(m, o, &SI_CHECK_SUPRA, &[event, u32::MAX]) as u16 != 0 {
        s1::motion_refresh_multi(m, o);
        interp_dot_from_list(m, o);
        return;
    }
    let canvas = m[SI_ACTIVE_CANVAS];
    let wmode = m.u16(m.u32(win.wrapping_add(0x2c)).wrapping_add(4));
    if m.u16(canvas.wrapping_add(0x24)) != 0 && (cx0 != 0 || cy0 != 0 || cz0 != 0) && wmode != 0x7da {
        let (x_, y_, z_) = (m.u16(canvas.wrapping_add(0x28)), m.u16(canvas.wrapping_add(0x2a)), m.u16(canvas.wrapping_add(0x2c)));
        let both = x_ & y_;
        let mode: u32 = if z_ & both != 0 {
            0
        } else if both != 0 {
            4
        } else if z_ & x_ != 0 {
            5
        } else if z_ & y_ != 0 {
            6
        } else if x_ != 0 {
            1
        } else if y_ != 0 {
            2
        } else if z_ != 0 {
            3
        } else {
            0
        };
        let manip = m.u32(si_glb_manip_10());
        call(m, o, &SI_SRT_HANDLE, &[win, manip, mode]);
        return;
    }
    if wmode == 0x7d7 && (m[SI_SCHEMATIC_MODE] != 0 || call(m, o, &SI_DOUBLE_CLICK_STATUS, &[]) as u16 != 0) {
        let n = call(m, o, &s1::SI_GL_SELECT_CHECK_NSE, &[win]);
        call(m, o, &SI_SCHEMATIC_ACTION, &[win, n]);
        return;
    }
    if wmode == 0x7da {
        call(m, o, &SI_SPRD_SHEET_ACTION, &[event]);
        return;
    }
    let pressed = |m: &mut Mem, o: &mut dyn Outside| call(m, o, &GETBUTTON, &[0x65]) != 0 || m[SI_TABLET_BUTTON] == 0x65;
    if m[FCV_COPY_MODE] != 0 {
        if pressed(m, o) {
            m[FCV_COPY_MODE] = 0;
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08698742, 4]);
            call(m, o, &SI_STATUS_DISPLAY_MOUSE, &[0x08698745, 0x08698744, 0x08698743]);
            return;
        }
        let node = call(m, o, &s1::SI_GL_SELECT, &[win]);
        if node == 0 {
            return;
        }
        let menu = m[COPY_MENU_ID];
        let gt = s1::get_graftype_from_menu(menu);
        if s1::ok_graf_type(m, node, gt as u32) as u16 == 0 {
            return;
        }
        flash_object(m, o, node);
        let item = m[COPY_MENU_ITEM];
        s1::traitemenu(m, o, node, menu, item);
        scene_refresh_end(m, o);
        return;
    }
    let scn = m[s1::SCNMOD];
    if pressed(m, o) {
        let ends = match scn {
            0x1f => {
                call(m, o, &s1::SI_REFRESH_SYSTEM, &[0, 0, 0x25]);
                true
            }
            6..=8 | 0x18 | 0x24..=0x2d | 0x2f | 0x3ec => true,
            _ => false,
        };
        if ends {
            scn_set_mode(m, o, 0, 1);
            return;
        }
    }
    let code = m.u16(event.wrapping_add(6));
    match scn {
        1 => {
            call(m, o, &SI_MDLRECTZOOM, &[event]);
        }
        2 => {
            call(m, o, &SI_DO_CAM, &[event]);
        }
        0x20 => {
            call(m, o, &SI_TUMBLING, &[event]);
        }
        0x21 => {
            call(m, o, &SI_TRACKING, &[event]);
        }
        0x22 => {
            call(m, o, &SI_DOLLY, &[event]);
        }
        0x23 => {
            call(m, o, &SI_CAMERA_ROLL, &[event]);
        }
        0x3e8 => {
            call(m, o, &SI_FCURVE_COPY_CUT_PASTE_TAG, &[event]);
        }
        0x3e9 => {
            call(m, o, &SI_FCURVE_MOVE_KEY, &[event]);
        }
        0x3ea => {
            call(m, o, &SI_FCURVE_SLOPE_MANAGEMENT, &[event, u32::MAX]);
        }
        0x3ec => {
            call(m, o, &SI_EDIT_FCURVE_POINT, &[event]);
        }
        0x3eb => {
            call(m, o, &SI_FCURVE_INTERP_POINT, &[event, u32::MAX]);
        }
        0x3ed => {
            call(m, o, &SI_FCURVE_TRANS_SCL, &[event]);
        }
        0x3ee => {
            call(m, o, &SI_FCURVE_TRANS_SCL_TAG, &[event]);
        }
        0x27 | 0x28 => {
            call(m, o, &SI_SEL_LIGHT_HANDLER, &[event, if scn == 0x27 { 1 } else { 2 }]);
        }
        0x10 | 0x11 | 0x12 => {
            call(m, o, &SI_PTS_SELECT, &[event, scn as u32 - 0x10]);
            call(m, o, &SI_REFRESH_ALL, &[1]);
            interp_dot_from_list(m, o);
        }
        3 => key_buffer_click(m, o, code),
        6 | 0x2a | 0x2f => s1::motion_select_path(m, o, event),
        8 => {
            let model = call(m, o, &SI_MODEL_ACTIVE, &[]);
            let node = call(m, o, &s1::SI_GL_SELECT, &[win]);
            if node != 0 && m.u32(node).wrapping_sub(2) <= 1 {
                flash_object(m, o, node);
                let d = m.u32(node.wrapping_add(4));
                call(m, o, &SI_ITEM_SET_SHAPE_KEY, &[model, 2, d]);
                scene_refresh_end(m, o);
            } else {
                call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x0869880c, 3]);
            }
        }
        7 => {
            let ct = m[MOTION_CONSTRAINT_TYPE] as i32;
            s1::motion_select_constraint(m, o, event, ct);
        }
        0xe | 0x19 => {
            let (a, b) = match code {
                0x66 => (0, 0),
                0x65 => (0, 1),
                0x67 => (1, 0),
                _ => (u32::MAX, u32::MAX),
            };
            if a != u32::MAX {
                let r = if scn == 0xe { &SI_FCURVE_SET_KEY_TAGS } else { &SI_FCURVE_SET_KEY_LOCKS };
                call(m, o, r, &[event, a, b]);
            }
            fcv_refresh_view(m, o);
        }
        0xf | 0x1a => {
            let r = if scn == 0xf { &SI_FCURVE_RECT_SET_KEY_TAGS } else { &SI_FCURVE_RECT_SET_KEY_LOCKS };
            call(m, o, r, &[event, code as i16 as i32 as u32]);
            fcv_refresh_view(m, o);
        }
        0x24 => {
            let r = motion_substitute_path(m, o, event);
            scn_set_mode(m, o, 0, 1);
            if r == u32::MAX {
                call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x086987a0, 3]);
            } else if r == 0 {
                call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x086987e0, 3]);
            }
        }
        0x2b | 0x2c => {
            let r = if scn == 0x2b { s1::motion_select_2pnts_constr(m, o, event) } else { s1::motion_select_3pnts_constr(m, o, event) };
            if r as u16 != 0 {
                scn_set_mode(m, o, 0, 1);
            }
        }
        0x29 => {
            call(m, o, &SI_RELAX_CONSTRAINT_BY_MOUSE, &[event]);
            call(m, o, &SI_REFRESH_SELECTED, &[1]);
        }
        0x1f => {
            if code == 0x65 {
                scn_set_mode(m, o, 0, 1);
            } else {
                let slot = m.local(4);
                m.set_u32(slot, event);
                if call(m, o, &SI_DISTANCE_INFO, &[slot]) as u16 == 0 {
                    scn_set_mode(m, o, 0, 1);
                }
            }
        }
        0x18 => {
            call(m, o, &SI_CHN_COPY_SELECTION, &[event]);
        }
        0x25 | 0x26 => {
            call(m, o, &SI_ANIM_DIAL_ASSOCIATE, &[event, if scn == 0x25 { 1 } else { 0 }]);
        }
        0x2d => s1::copy_expression(m, o, event),
        0x2e => {
            call(m, o, &SI_FCURVE_ROTATE_SLOPE_ZERO_TAG, &[event]);
        }
        _ => {}
    }
}

/// The address of the word at +0x10 of `SI_glbManip` (read as a 32-bit word).
fn si_glb_manip_10() -> u32 {
    0x08781560 + 0x10
}

/// The end shared by the modes that changed the keys: the scene is refreshed.
fn scene_refresh_end(m: &mut Mem, o: &mut dyn Outside) {
    call(m, o, &s1::SI_SCENE_REFRESH, &[]);
}

/// The end of the key tag and lock modes: the view is refreshed (arguments 1, 0).
fn fcv_refresh_view(m: &mut Mem, o: &mut dyn Outside) {
    call(m, o, &s1::SI_REFRESH_VIEW, &[1, 0]);
}

/// Mode 3 of `scnWind`: with more than one graph type selected only the message of
/// `motionMultiMessage`, with none "(no selection)", else the click copies (0x67), cuts (0x66)
/// or pastes (0x65) the keys of the one selected node.
fn key_buffer_click(m: &mut Mem, o: &mut dyn Outside, code: u16) {
    let l74 = m.local(4);
    let buf = m.local(0x64);
    let list = cur_list(m);
    let n = call(m, o, &s1::SI_GRAFLIST_TYPE_COUNT, &[list, 0, 0, l74, 0]);
    if (n as i32) > 1 {
        s1::motion_multi_message(m, o);
        return;
    }
    let node = m.u32(l74);
    if node == 0 {
        call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08698746, 3]);
        return;
    }
    let ty = m.u32(node);
    let d = m.u32(node.wrapping_add(4));
    match code {
        0x67 | 0x66 => {
            let (fmt, flag) = if code == 0x67 { (0x08698763, 0) } else { (0x08698773, 1) };
            let t = frame_seconds(m);
            let r = match key_kind(ty) {
                KeyKind::Model => call(m, o, &SI_COPY_MODEL_KEY_TO_BUFFER, &[d, t, flag]),
                KeyKind::Light => call(m, o, &SI_COPY_LIGHT_KEY_TO_BUFFER, &[d, t, flag]),
                KeyKind::Camera => call(m, o, &SI_COPY_CAMERA_KEY_TO_BUFFER, &[d, t, flag]),
                KeyKind::None => 0,
            };
            put_message(m, fmt, buf, r);
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[buf, 0]);
            if code == 0x66 {
                scene_refresh_end(m, o);
            }
        }
        0x65 => {
            let t = frame_seconds(m);
            let r = match key_kind(ty) {
                KeyKind::Model => call(m, o, &SI_PASTE_MODEL_KEY_FROM_BUFFER, &[d, t]),
                KeyKind::Light => call(m, o, &SI_PASTE_LIGHT_KEY_FROM_BUFFER, &[d, t]),
                KeyKind::Camera => call(m, o, &SI_PASTE_CAMERA_KEY_FROM_BUFFER, &[d, t]),
                KeyKind::None => 0,
            };
            put_message(m, 0x08698780, buf, r);
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[buf, 0]);
            scene_refresh_end(m, o);
        }
        _ => {}
    }
}
