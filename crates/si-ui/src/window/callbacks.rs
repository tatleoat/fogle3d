// SPDX-License-Identifier: MIT
//! The handlers of the parts of a view window (`window.c`, 0x083d9af4 to 0x083dae2b): the
//! 52 functions the table `gadgetInfo` points to, and the two menu helpers three of them use.
//!
//! Every handler gets the record `si_viewSelectHandle` builds (event, point, window, part)
//! and leaves its answer in the event. Three answers recur:
//!
//! - **void** (`voidEvent`): done, nothing for the caller to do;
//! - **0x1e**: the window's content changed; `first` the window's identifier, `second` −1,
//!   `data` the window (the point is left as it was);
//! - **0x1f**: the same with the point cleared to (0, 0): redraw the window.
//!
//! Nearly all title-bar gadgets first redraw themselves pressed (`SI_windowGadjet(window,
//! part, 1)`).

use crate::globals::*;
use crate::mem::P;
use crate::records::{cam, off, Callback, Event, Grafport};
use crate::Ui;

/// The handlers by their address in the original, as the gadget table names them.
pub const HANDLERS: [(u32, &str); 52] = [
    (0x083d9af4, "inCutPasteKeyCB"),
    (0x083d9b60, "inSlopeManagementCB"),
    (0x083d9bd0, "inInterpManagementCB"),
    (0x083d9c40, "inFcvSnapCB"),
    (0x083d9c74, "inFcvSwapCB"),
    (0x083d9ca8, "inSetKeyCB"),
    (0x083d9d18, "inEditKeyCB"),
    (0x083d9d88, "inEditTagKeyCB"),
    (0x083d9df8, "inEditFcurveCB"),
    (0x083d9e68, "inConstantInterpCB"),
    (0x083d9ea4, "inLinearInterpCB"),
    (0x083d9ee0, "inSplineInterpCB"),
    (0x083d9f1c, "inSlope0InterpCB"),
    (0x083d9f58, "inPlateauInterpCB"),
    (0x083d9f94, "inSchemCleanAllCB"),
    (0x083d9ff4, "inSchemViewCB"),
    (0x083da074, "inSchemOpenCloseCB"),
    (0x083da0cc, "inSchemDefcolCB"),
    (0x083da100, "inSchemCleanCB"),
    (0x083da160, "inDsZnActivCB"),
    (0x083da1bc, "inDsZnUnactivCB"),
    (0x083da218, "inDsCopyCB"),
    (0x083da274, "inDsCutCB"),
    (0x083da2d0, "inDsPasteCB"),
    (0x083da32c, "inDsClearCB"),
    (0x083da388, "inDsSetupCB"),
    (0x083da3d8, "inDsLinkCB"),
    (0x083da434, "inDsUnlinkCB"),
    (0x083da490, "inDsMergeCB"),
    (0x083da4e4, "inDsAlignCB"),
    (0x083da540, "inDsActModeCB"),
    (0x083da5a0, "inOrthoXyzCB"),
    (0x083da608, "inSpreadSetupCB"),
    (0x083da64c, "inSpreadQueryCB"),
    (0x083da690, "inSpreadFieldsCB"),
    (0x083da6d8, "inSpreadSortCB"),
    (0x083da720, "inSpreadExecQueryCB"),
    (0x083da7ac, "inSpreadSelectCB"),
    (0x083da804, "inSpreadDeselectCB"),
    (0x083da858, "inSpreadQueriesCB"),
    (0x083da8d4, "inDocropCB"),
    (0x083da9f8, "inSetupCB"),
    (0x083daa74, "inClipCB"),
    (0x083dab10, "inLayoutCB"),
    (0x083dab80, "inGrafportCB"),
    (0x083dabd8, "inCamHandlerCB"),
    (0x083dac4c, "inSelectCB"),
    (0x083dac70, "inZoomCB"),
    (0x083dac8c, "inMemoCamCB"),
    (0x083dad04, "inDispModeCB"),
    (0x083dad84, "inTitleCB"),
    (0x083dadb8, "inFcvLstCB"),
];

/// The five modules' functions for one of the key-editing modes.
#[derive(Clone, Copy)]
enum KeyMode {
    CutPaste,
    Slope,
    Interp,
    SetKey,
    EditKey,
    EditTag,
    EditFcv,
}

impl Ui {
    fn gadjet_pressed(&mut self, cb: &Callback) {
        self.si_window_gadjet(cb.window, cb.part, 1);
    }
    /// The answers 0x1e, 0x1f and 0xd: the window in the event.
    fn window_event(&mut self, cb: &Callback, kind: i32, clear_point: bool) {
        let id = self.m[cb.window].id;
        let e = &mut self.m[cb.event];
        e.kind = kind;
        e.first = id;
        e.second = -1;
        if clear_point {
            e.x = 0;
            e.y = 0;
        }
        e.data = cb.window.addr();
    }
    /// The gadget drawn pressed, the active module put into a key-editing mode, the event
    /// voided.
    fn key_mode(&mut self, cb: &Callback, mode: KeyMode) {
        self.gadjet_pressed(cb);
        let (m, o) = (&mut self.m, &mut self.o.modules);
        match (m[SI_ACTIVE], mode) {
            (0, KeyMode::CutPaste) => o.SI_modSetCutPasteKeyMode(m),
            (1, KeyMode::CutPaste) => o.SI_motSetCutPasteKeyMode(m),
            (2, KeyMode::CutPaste) => o.SI_actSetCutPasteKeyMode(m),
            (3, KeyMode::CutPaste) => o.SI_matSetCutPasteKeyMode(m),
            (4, KeyMode::CutPaste) => o.SI_tolSetCutPasteKeyMode(m),
            (0, KeyMode::Slope) => o.SI_modSetSlopeManagementMode(m),
            (1, KeyMode::Slope) => o.SI_motSetSlopeManagementMode(m),
            (2, KeyMode::Slope) => o.SI_actSetSlopeManagementMode(m),
            (3, KeyMode::Slope) => o.SI_matSetSlopeManagementMode(m),
            (4, KeyMode::Slope) => o.SI_tolSetSlopeManagementMode(m),
            (0, KeyMode::Interp) => o.SI_modSetInterpManagementMode(m),
            (1, KeyMode::Interp) => o.SI_motSetInterpManagementMode(m),
            (2, KeyMode::Interp) => o.SI_actSetInterpManagementMode(m),
            (3, KeyMode::Interp) => o.SI_matSetInterpManagementMode(m),
            (4, KeyMode::Interp) => o.SI_tolSetInterpManagementMode(m),
            (0, KeyMode::SetKey) => o.SI_modSetSetKeyMode(m),
            (1, KeyMode::SetKey) => o.SI_motSetSetKeyMode(m),
            (2, KeyMode::SetKey) => o.SI_actSetSetKeyMode(m),
            (3, KeyMode::SetKey) => o.SI_matSetSetKeyMode(m),
            (4, KeyMode::SetKey) => o.SI_tolSetSetKeyMode(m),
            (0, KeyMode::EditKey) => o.SI_modSetEditKeyMode(m),
            (1, KeyMode::EditKey) => o.SI_motSetEditKeyMode(m),
            (2, KeyMode::EditKey) => o.SI_actSetEditKeyMode(m),
            (3, KeyMode::EditKey) => o.SI_matSetEditKeyMode(m),
            (4, KeyMode::EditKey) => o.SI_tolSetEditKeyMode(m),
            (0, KeyMode::EditTag) => o.SI_modSetEditTagMode(m),
            (1, KeyMode::EditTag) => o.SI_motSetEditTagMode(m),
            (2, KeyMode::EditTag) => o.SI_actSetEditTagMode(m),
            (3, KeyMode::EditTag) => o.SI_matSetEditTagMode(m),
            (4, KeyMode::EditTag) => o.SI_tolSetEditTagMode(m),
            (0, KeyMode::EditFcv) => o.SI_modSetEditFcvMode(m),
            (1, KeyMode::EditFcv) => o.SI_motSetEditFcvMode(m),
            (2, KeyMode::EditFcv) => o.SI_actSetEditFcvMode(m),
            (3, KeyMode::EditFcv) => o.SI_matSetEditFcvMode(m),
            (4, KeyMode::EditFcv) => o.SI_tolSetEditFcvMode(m),
            _ => {}
        }
        self.void_event(cb.event);
    }
    /// A dope-sheet gadget: drawn pressed, the operation on the window's items, answer 0x1e.
    fn dope_item(&mut self, cb: &Callback, operation: i32) {
        self.gadjet_pressed(cb);
        self.o.views.SI_dopeItemOper(&mut self.m, cb.window, operation, 0, 0x3f80_0000);
        self.window_event(cb, 0x1e, false);
    }
    /// An interpolation gadget of the function-curve view.
    fn interpolation(&mut self, cb: &Callback, kind: i16) {
        self.gadjet_pressed(cb);
        self.o.views.SI_setSelectedFcurveInterp(&mut self.m, kind);
        self.void_event(cb.event);
    }

    /// The schematic view's mode menu (Light, Model, Motion, Matter; several may be chosen)
    /// [soft 0x083d1e24 schemHandleModeMenu]: for a mouse button, the menu is run on the
    /// camera's mode bits (+6), the bits are remembered, the hierarchies' boxes recomputed;
    /// answers 1. Any other device: 0.
    pub fn schem_handle_mode_menu(&mut self, g: P<Grafport>, event: P<Event>) -> i16 {
        let device = self.m[event].second;
        if !(device < 0x68 && 100 < device) {
            return 0;
        }
        let (menu, bits) = (self.m[g].menu_schematic, self.m[g].camera.wrapping_add(6));
        self.o.menus.SI_popupListMultiSelection(&mut self.m, g, menu, bits);
        self.m[DAT_087FB96C] = self.m.i16(self.m[g].camera.wrapping_add(6)) as i32;
        let dict = self.current_dictionary_of_scene();
        self.o.views.SI_schemComputeHierarchiesBBox(&mut self.m, dict, 1, 0);
        1
    }

    /// The spreadsheet's menu of ready-made queries [soft 0x083d1fb8 sprdHandleQueriesMenu]:
    /// for a mouse button the menu is run; a choice loads that set-up and answers 1.
    pub fn sprd_handle_queries_menu(&mut self, g: P<Grafport>, event: P<Event>) -> i16 {
        let device = self.m[event].second;
        if device < 0x68 && 100 < device {
            let menu = self.m[g].menu_queries;
            let item = self.o.menus.SI_popupList(&mut self.m, g, menu);
            if item != -1 {
                self.o.views.SI_sprdLoadPreDefSetup(&mut self.m, g, item, 0);
                return 1;
            }
        }
        0
    }

    /// CPTAG of the function-curve view [soft 0x083d9af4 inCutPasteKeyCB].
    pub fn in_cut_paste_key_cb(&mut self, cb: &Callback) {
        self.key_mode(cb, KeyMode::CutPaste);
    }
    /// SLOPE [soft 0x083d9b60 inSlopeManagementCB].
    pub fn in_slope_management_cb(&mut self, cb: &Callback) {
        self.key_mode(cb, KeyMode::Slope);
    }
    /// INTERP [soft 0x083d9bd0 inInterpManagementCB].
    pub fn in_interp_management_cb(&mut self, cb: &Callback) {
        self.key_mode(cb, KeyMode::Interp);
    }
    /// SNAP: a snapshot of the selected curves [soft 0x083d9c40 inFcvSnapCB].
    pub fn in_fcv_snap_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        self.o.views.SI_snapshotSelectedFcvs(&mut self.m);
        self.void_event(cb.event);
    }
    /// SWAP: the selected curves and their snapshot change places [soft 0x083d9c74 inFcvSwapCB].
    pub fn in_fcv_swap_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        self.o.views.SI_swapSelectedFcvsToAux(&mut self.m);
        self.void_event(cb.event);
    }
    /// SETKEY [soft 0x083d9ca8 inSetKeyCB].
    pub fn in_set_key_cb(&mut self, cb: &Callback) {
        self.key_mode(cb, KeyMode::SetKey);
    }
    /// EDITKEY [soft 0x083d9d18 inEditKeyCB].
    pub fn in_edit_key_cb(&mut self, cb: &Callback) {
        self.key_mode(cb, KeyMode::EditKey);
    }
    /// EDITTAG [soft 0x083d9d88 inEditTagKeyCB].
    pub fn in_edit_tag_key_cb(&mut self, cb: &Callback) {
        self.key_mode(cb, KeyMode::EditTag);
    }
    /// EDITFCV [soft 0x083d9df8 inEditFcurveCB].
    pub fn in_edit_fcurve_cb(&mut self, cb: &Callback) {
        self.key_mode(cb, KeyMode::EditFcv);
    }
    /// CON: constant interpolation for the selected curves [soft 0x083d9e68 inConstantInterpCB].
    pub fn in_constant_interp_cb(&mut self, cb: &Callback) {
        self.interpolation(cb, 0);
    }
    /// LIN [soft 0x083d9ea4 inLinearInterpCB].
    pub fn in_linear_interp_cb(&mut self, cb: &Callback) {
        self.interpolation(cb, 1);
    }
    /// SPL [soft 0x083d9ee0 inSplineInterpCB].
    pub fn in_spline_interp_cb(&mut self, cb: &Callback) {
        self.interpolation(cb, 2);
    }
    /// SL0: zero slopes [soft 0x083d9f1c inSlope0InterpCB].
    pub fn in_slope0_interp_cb(&mut self, cb: &Callback) {
        self.interpolation(cb, 3);
    }
    /// PLA: plateaus [soft 0x083d9f58 inPlateauInterpCB].
    pub fn in_plateau_interp_cb(&mut self, cb: &Callback) {
        self.interpolation(cb, 4);
    }
    /// CLEAN_ALL of the schematic view: lay out everything again
    /// [soft 0x083d9f94 inSchemCleanAllCB].
    pub fn in_schem_clean_all_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        let dict = self.current_dictionary_of_scene();
        self.o.views.SI_schematicSort(&mut self.m, cb.window, dict, 0);
        self.o.views.SI_refreshAll(&mut self.m, 1);
        self.void_event(cb.event);
    }
    /// The schematic view's mode menu gadget [soft 0x083d9ff4 inSchemViewCB].
    pub fn in_schem_view_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        if self.schem_handle_mode_menu(cb.window, cb.event) == 0 {
            self.void_event(cb.event);
        } else {
            self.window_event(cb, 0x1f, true);
        }
    }
    /// OPEN/CLOSE of the schematic view: a switch; switching it on leaves the super mode
    /// [soft 0x083da074 inSchemOpenCloseCB].
    pub fn in_schem_open_close_cb(&mut self, cb: &Callback) {
        let on = self.m[SI_SCHEMATIC_MODE] == 0;
        self.m[SI_SCHEMATIC_MODE] = on as i16;
        if on {
            self.o.controls.SI_superModeOff(&mut self.m, 1);
        }
        self.gadjet_pressed(cb);
        self.void_event(cb.event);
    }
    /// DEFCOL [soft 0x083da0cc inSchemDefcolCB].
    pub fn in_schem_defcol_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        self.o.views.SI_schemSetDefaultColors(&mut self.m);
        self.void_event(cb.event);
    }
    /// CLEAN: lay out the selected hierarchies [soft 0x083da100 inSchemCleanCB].
    pub fn in_schem_clean_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        let dict = self.current_dictionary_of_scene();
        self.o.views.SI_schematicSort(&mut self.m, cb.window, dict, 1);
        self.o.views.SI_refreshAll(&mut self.m, 1);
        self.void_event(cb.event);
    }
    /// ACTIV of the dope sheet (operation 14) [soft 0x083da160 inDsZnActivCB].
    pub fn in_ds_zn_activ_cb(&mut self, cb: &Callback) {
        self.dope_item(cb, 0xe);
    }
    /// DEACT (15) [soft 0x083da1bc inDsZnUnactivCB].
    pub fn in_ds_zn_unactiv_cb(&mut self, cb: &Callback) {
        self.dope_item(cb, 0xf);
    }
    /// COPY (4) [soft 0x083da218 inDsCopyCB].
    pub fn in_ds_copy_cb(&mut self, cb: &Callback) {
        self.dope_item(cb, 4);
    }
    /// CUT (5) [soft 0x083da274 inDsCutCB].
    pub fn in_ds_cut_cb(&mut self, cb: &Callback) {
        self.dope_item(cb, 5);
    }
    /// PASTE (6) [soft 0x083da2d0 inDsPasteCB].
    pub fn in_ds_paste_cb(&mut self, cb: &Callback) {
        self.dope_item(cb, 6);
    }
    /// CLEAR (7) [soft 0x083da32c inDsClearCB].
    pub fn in_ds_clear_cb(&mut self, cb: &Callback) {
        self.dope_item(cb, 7);
    }
    /// SETUP of the dope sheet: its dialog [soft 0x083da388 inDsSetupCB].
    pub fn in_ds_setup_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        self.o.views.SI_dopeSetupDialogue(&mut self.m);
        self.window_event(cb, 0x1e, false);
    }
    /// LINK (2) [soft 0x083da3d8 inDsLinkCB].
    pub fn in_ds_link_cb(&mut self, cb: &Callback) {
        self.dope_item(cb, 2);
    }
    /// UNLINK (3) [soft 0x083da434 inDsUnlinkCB].
    pub fn in_ds_unlink_cb(&mut self, cb: &Callback) {
        self.dope_item(cb, 3);
    }
    /// MERGE: operation 16 on the actions [soft 0x083da490 inDsMergeCB].
    pub fn in_ds_merge_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        self.o.views.SI_dopeActionsOper(&mut self.m, 0x10);
        self.window_event(cb, 0x1e, false);
    }
    /// ALIGN (17) [soft 0x083da4e4 inDsAlignCB].
    pub fn in_ds_align_cb(&mut self, cb: &Callback) {
        self.dope_item(cb, 0x11);
    }
    /// ACT_KEY: a switch of the dope sheet [soft 0x083da540 inDsActModeCB].
    pub fn in_ds_act_mode_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        self.window_event(cb, 0x1e, false);
        self.m[DAT_0878FB22] = (self.m[DAT_0878FB22] == 0) as i16;
    }
    /// X, Y, Z of the free orthogonal view: look along that axis
    /// [soft 0x083da5a0 inOrthoXyzCB].
    pub fn in_ortho_xyz_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        self.o.views.SI_orthoCamSetInterest(&mut self.m, cb.window, cb.part);
        self.window_event(cb, 0x1f, true);
    }
    /// SETUP of the spreadsheet [soft 0x083da608 inSpreadSetupCB].
    pub fn in_spread_setup_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        let setup = self.m.u32(cb.window.addr() + 0x2ac);
        self.o.views.SI_sprdSetupDialogue(&mut self.m, setup);
        self.void_event(cb.event);
    }
    /// QUERY: the query dialog on the window's set-up pointer [soft 0x083da64c inSpreadQueryCB].
    pub fn in_spread_query_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        self.o.views.SI_sprdQueryDialogue(&mut self.m, cb.window.addr() + 0x2ac);
        self.void_event(cb.event);
    }
    /// COLUMNS [soft 0x083da690 inSpreadFieldsCB].
    pub fn in_spread_fields_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        let setup = self.m.u32(cb.window.addr() + 0x2ac);
        self.o.views.SI_sprdSprdSetupDialogue(&mut self.m, setup, 0, 0);
        self.void_event(cb.event);
    }
    /// SORT [soft 0x083da6d8 inSpreadSortCB].
    pub fn in_spread_sort_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        let setup = self.m.u32(cb.window.addr() + 0x2ac);
        self.o.views.SI_sprdSprdSetupDialogue(&mut self.m, setup, 0, 1);
        self.void_event(cb.event);
    }
    /// UPDATE: run the query; with Alt (143) and Control (3) both held, also write the
    /// result out [soft 0x083da720 inSpreadExecQueryCB].
    pub fn in_spread_exec_query_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        let dict = self.current_dictionary_of_scene();
        self.o.views.SI_sprdExec(&mut self.m, cb.window, dict, 0);
        if self.o.gl.getbutton(&mut self.m, 0x8f) != 0 && self.o.gl.getbutton(&mut self.m, 3) != 0 {
            self.o.views.si_sprdWriteResultContent(&mut self.m, cb.window);
        }
        self.void_event(cb.event);
    }
    /// SEL: select the listed elements; the right button adds [soft 0x083da7ac inSpreadSelectCB].
    pub fn in_spread_select_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        let right = (self.m[cb.event].second == 0x65) as i16;
        self.o.views.SI_sprdSprdSelectElems(&mut self.m, cb.window, right);
        self.void_event(cb.event);
    }
    /// DESEL [soft 0x083da804 inSpreadDeselectCB].
    pub fn in_spread_deselect_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        let right = (self.m[cb.event].second == 0x65) as i16;
        self.o.views.SI_sprdSprdDeselectElems(&mut self.m, cb.window, right);
        self.void_event(cb.event);
    }
    /// QUERIES: the menu of ready-made queries [soft 0x083da858 inSpreadQueriesCB].
    pub fn in_spread_queries_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        if self.sprd_handle_queries_menu(cb.window, cb.event) == 0 {
            self.void_event(cb.event);
        } else {
            self.window_event(cb, 0x1f, true);
        }
    }
    /// The "1:1" box of a view that shows a rotoscope picture [soft 0x083da8d4 inDocropCB]:
    /// with the middle or left button. If the camera's word at +0x44 is clear: the camera
    /// position is saved, the picture cropped to the window, the camera made orthogonal
    /// unless the view has a margin, and the window refreshed twice. If it is set: the view
    /// is reset and the answer is 0xd. Then the gadget is drawn. Other buttons: void.
    pub fn in_docrop_cb(&mut self, cb: &Callback) {
        let device = self.m[cb.event].second;
        if !(device < 0x68 && 0x65 < device) {
            self.void_event(cb.event);
            return;
        }
        let g = cb.window;
        if self.m.i16(self.m[g].camera.wrapping_add(off::CAMERA_44)) == 0 {
            self.o.views.SI_doSaveCamPos(&mut self.m, g);
            self.m[g].roto[5] = self.m[g].roto[5].wrapping_sub(1);
            let w = self.m[g];
            self.o.views.SI_dovCrop(&mut self.m, g, w.left as i32, w.right as i32, w.bottom as i32, w.top as i32, 0x65);
            let camera = self.m[g].camera;
            if self.m.i16(camera.wrapping_add(off::CAMERA_0C)) == 0 {
                let on = (self.m.i16(camera.wrapping_add(off::CAMERA_44)) == 0) as i32;
                self.o.views.SI_dovSetCamOrtho(&mut self.m, g, on, 0x65);
            }
            self.o.views.SI_refreshSystem(&mut self.m, g, 0, 0x37);
            self.o.views.SI_refreshSystem(&mut self.m, g, 0, 0x37);
        } else {
            self.o.views.SI_dovViewReset(&mut self.m, g);
            self.window_event(cb, 0xd, true);
        }
        self.gadjet_pressed(cb);
    }
    /// SETUP of the display mode [soft 0x083da9f8 inSetupCB].
    pub fn in_setup_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        if self.o.views.SI_displayModeSetup(&mut self.m, cb.window, cb.event) == 0 {
            self.void_event(cb.event);
        } else {
            self.window_event(cb, 0x1f, true);
        }
    }
    /// CLIP: the camera's clipping planes back to the defaults of its view type
    /// [soft 0x083daa74 inClipCB].
    pub fn in_clip_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        let camera = self.m[cb.window].camera;
        let mut planes = [0u32; 2];
        let kind = self.m.i16(camera.wrapping_add(off::CAMERA_TYPE)) as i32;
        if self.o.views.SI_camGetDefaultFromId(&mut self.m, kind, &mut planes) != 0 {
            let camera = self.m[cb.window].camera;
            self.m.set_u32(camera.wrapping_add(0x34), planes[0]);
            self.m.set_u32(camera.wrapping_add(0x38), planes[1]);
        }
        self.window_event(cb, 0x1f, true);
    }
    /// The layout box: the view's layout dialog, or the schematic view's
    /// [soft 0x083dab10 inLayoutCB].
    pub fn in_layout_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        if self.m.i16(self.m[cb.window].camera.wrapping_add(off::CAMERA_TYPE)) == cam::SCHEMATIC {
            let dict = self.current_dictionary_of_scene();
            self.o.views.SI_dialogSchematicLayout(&mut self.m, cb.window, dict);
        } else {
            self.o.views.si_dialogLayout(&mut self.m, cb.window);
        }
        self.void_event(cb.event);
    }
    /// The drawing area [soft 0x083dab80 inGrafportCB]: for a visible window the event
    /// becomes 5 with the window's identifier, the point and the window (`second` keeps the
    /// device); else void.
    pub fn in_grafport_cb(&mut self, cb: &Callback) {
        if self.m[cb.window].visible == 0 {
            self.void_event(cb.event);
        } else {
            let id = self.m[cb.window].id;
            let e = &mut self.m[cb.event];
            e.kind = 5;
            e.first = id;
            e.x = cb.x;
            e.y = cb.y;
            e.data = cb.window.addr();
        }
    }
    /// The camera control buttons [soft 0x083dabd8 inCamHandlerCB]: the click is put back
    /// into the queue (device, 1; device, x; device, y) for the buttons' own loop.
    pub fn in_cam_handler_cb(&mut self, cb: &Callback) {
        let device = self.m[cb.event].second as u16;
        self.o.gl.qenter(&mut self.m, device, 1);
        self.o.gl.qenter(&mut self.m, device, cb.x);
        self.o.gl.qenter(&mut self.m, device, cb.y);
        self.o.views.SI_ctrlButHandler(&mut self.m, cb.window, cb.x, cb.y);
        self.void_event(cb.event);
    }
    /// The selection box [soft 0x083dac4c inSelectCB].
    pub fn in_select_cb(&mut self, cb: &Callback) {
        let kind = self.window_toggle_select(cb.window);
        self.m[cb.event].kind = kind;
    }
    /// The zoom box [soft 0x083dac70 inZoomCB].
    pub fn in_zoom_cb(&mut self, cb: &Callback) {
        self.window_zoom_handle(cb.window, cb.event);
    }
    /// The stored-camera boxes [soft 0x083dac8c inMemoCamCB].
    pub fn in_memo_cam_cb(&mut self, cb: &Callback) {
        let device = self.m[cb.event].second;
        if self.o.views.SI_memoCamHandle(&mut self.m, cb.window, device, cb.x) == 0 {
            self.void_event(cb.event);
        } else {
            self.window_event(cb, 0x1f, true);
        }
    }
    /// The display-mode name [soft 0x083dad04 inDispModeCB].
    pub fn in_disp_mode_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        if self.o.views.SI_displayModeHandle(&mut self.m, cb.window, cb.event) == 0 {
            self.void_event(cb.event);
        } else {
            self.window_event(cb, 0x1f, true);
        }
    }
    /// The title: the menu of view types [soft 0x083dad84 inTitleCB].
    pub fn in_title_cb(&mut self, cb: &Callback) {
        self.gadjet_pressed(cb);
        self.window_title_handle(cb.window, cb.event);
    }
    /// The curve list of a function-curve view [soft 0x083dadb8 inFcvLstCB]: the click goes
    /// back into the queue for the list's own loop.
    pub fn in_fcv_lst_cb(&mut self, cb: &Callback) {
        let device = self.m[cb.event].second as u16;
        self.o.gl.qenter(&mut self.m, device, 1);
        self.o.gl.qenter(&mut self.m, device, cb.x);
        self.o.gl.qenter(&mut self.m, device, cb.y);
        self.o.views.si_fcvTxtLstHandler(&mut self.m, cb.window, cb.x, cb.y);
        self.void_event(cb.event);
    }

    /// Call one of the handlers by its name in the original.
    pub fn call_handler_by_name(&mut self, name: &str, cb: &Callback) -> bool {
        match name {
            "inCutPasteKeyCB" => self.in_cut_paste_key_cb(cb),
            "inSlopeManagementCB" => self.in_slope_management_cb(cb),
            "inInterpManagementCB" => self.in_interp_management_cb(cb),
            "inFcvSnapCB" => self.in_fcv_snap_cb(cb),
            "inFcvSwapCB" => self.in_fcv_swap_cb(cb),
            "inSetKeyCB" => self.in_set_key_cb(cb),
            "inEditKeyCB" => self.in_edit_key_cb(cb),
            "inEditTagKeyCB" => self.in_edit_tag_key_cb(cb),
            "inEditFcurveCB" => self.in_edit_fcurve_cb(cb),
            "inConstantInterpCB" => self.in_constant_interp_cb(cb),
            "inLinearInterpCB" => self.in_linear_interp_cb(cb),
            "inSplineInterpCB" => self.in_spline_interp_cb(cb),
            "inSlope0InterpCB" => self.in_slope0_interp_cb(cb),
            "inPlateauInterpCB" => self.in_plateau_interp_cb(cb),
            "inSchemCleanAllCB" => self.in_schem_clean_all_cb(cb),
            "inSchemViewCB" => self.in_schem_view_cb(cb),
            "inSchemOpenCloseCB" => self.in_schem_open_close_cb(cb),
            "inSchemDefcolCB" => self.in_schem_defcol_cb(cb),
            "inSchemCleanCB" => self.in_schem_clean_cb(cb),
            "inDsZnActivCB" => self.in_ds_zn_activ_cb(cb),
            "inDsZnUnactivCB" => self.in_ds_zn_unactiv_cb(cb),
            "inDsCopyCB" => self.in_ds_copy_cb(cb),
            "inDsCutCB" => self.in_ds_cut_cb(cb),
            "inDsPasteCB" => self.in_ds_paste_cb(cb),
            "inDsClearCB" => self.in_ds_clear_cb(cb),
            "inDsSetupCB" => self.in_ds_setup_cb(cb),
            "inDsLinkCB" => self.in_ds_link_cb(cb),
            "inDsUnlinkCB" => self.in_ds_unlink_cb(cb),
            "inDsMergeCB" => self.in_ds_merge_cb(cb),
            "inDsAlignCB" => self.in_ds_align_cb(cb),
            "inDsActModeCB" => self.in_ds_act_mode_cb(cb),
            "inOrthoXyzCB" => self.in_ortho_xyz_cb(cb),
            "inSpreadSetupCB" => self.in_spread_setup_cb(cb),
            "inSpreadQueryCB" => self.in_spread_query_cb(cb),
            "inSpreadFieldsCB" => self.in_spread_fields_cb(cb),
            "inSpreadSortCB" => self.in_spread_sort_cb(cb),
            "inSpreadExecQueryCB" => self.in_spread_exec_query_cb(cb),
            "inSpreadSelectCB" => self.in_spread_select_cb(cb),
            "inSpreadDeselectCB" => self.in_spread_deselect_cb(cb),
            "inSpreadQueriesCB" => self.in_spread_queries_cb(cb),
            "inDocropCB" => self.in_docrop_cb(cb),
            "inSetupCB" => self.in_setup_cb(cb),
            "inClipCB" => self.in_clip_cb(cb),
            "inLayoutCB" => self.in_layout_cb(cb),
            "inGrafportCB" => self.in_grafport_cb(cb),
            "inCamHandlerCB" => self.in_cam_handler_cb(cb),
            "inSelectCB" => self.in_select_cb(cb),
            "inZoomCB" => self.in_zoom_cb(cb),
            "inMemoCamCB" => self.in_memo_cam_cb(cb),
            "inDispModeCB" => self.in_disp_mode_cb(cb),
            "inTitleCB" => self.in_title_cb(cb),
            "inFcvLstCB" => self.in_fcv_lst_cb(cb),
            _ => return false,
        }
        true
    }
}
