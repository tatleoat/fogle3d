// SPDX-License-Identifier: MIT
//! The global variables `event.c` and `window.c` use, by address in the Linux `soft` binary.
//!
//! Each is a typed pointer into the memory image: `m[SI_SUPRA_MODE]` is the variable. A
//! variable in `.data` has its initial value here (`= value`), written by [`init`]; one in
//! `.bss` starts as zero. `re/tools/si-ui/check_globals.py` compares every address with the
//! binary's symbol table and every initial value with the bytes of `.data`.
//!
//! Variables defined in `event.c` itself are marked "own"; the others belong to other source
//! files and are only used here.

use crate::mem::{Addr, Mem, P};
use crate::records::{Canvas, Event, Gadget, SimpleList};

/// One entry of the table the check script reads: Rust name, address, size in bytes, the
/// original's symbol, and the initial value of a `.data` variable.
pub struct Global {
    pub name: &'static str,
    pub addr: u32,
    pub size: u32,
    pub symbol: &'static str,
    pub init: Option<i64>,
}

macro_rules! globals {
    ( $( $(#[$meta:meta])* $name:ident : $ty:ty = $addr:literal , $sym:literal $(, init $init:expr)? ; )* ) => {
        $( $(#[$meta])* pub const $name: P<$ty> = P::at($addr); )*
        /// Every variable declared in this file.
        pub const ALL: &[Global] = &[
            $( Global { name: stringify!($name), addr: $addr, size: core::mem::size_of::<$ty>() as u32,
                        symbol: $sym, init: globals!(@init $($init)?) } ),*
        ];
    };
    (@init) => { None };
    (@init $init:expr) => { Some($init as i64) };
}

globals! {
    // ---- event.c's own
    /// Set by a mouse press close in time and place to the one before.
    DOUBLE_CLICK: i16 = 0x086be9a0, "doubleClick", init 0;
    /// Non-zero while events pushed back with `si_pushQE` are waiting.
    IS_E: i16 = 0x086be9a2, "isE", init 0;
    /// How many are waiting, at most 9.
    NB_E: i16 = 0x086be9a4, "nbE", init 0;
    /// 0, or 1 while Alt (devices 143, 144), 2 while Control (3, 145) picks menu cells by letter.
    MNEMONIC_MODE: i16 = 0x086be9a6, "mnemonicMode", init 0;
    /// The swift key being replayed, -1 for none.
    SWIFTMODE: i16 = 0x086be9a8, "swiftmode", init -1;
    /// The swift key found for the last key press, -1 for none.
    DIRECT_KEY: i16 = 0x086be9aa, "directKey", init -1;
    /// The key devices that can start a swift-key (supra) mode, zero-terminated; 62 entries.
    SUPRA_KEYS: [i16; 63] = 0x086be9c0, "supraKeys";
    /// The action strings of the transformation boxes: [group 1..3][control 0..2][part 0..2][button L, M, R].
    DESC_SRT: [Addr; 81] = 0x086bf080, "descSRT";
    SIZE_OF_DESC_SRT: i32 = 0x086bf1c4, "sizeOfDescSRT", init 81;
    /// The action strings of the play box: seven buttons × L, M, R.
    DESC_PLAY_BOX: [Addr; 21] = 0x086bf360, "descPlayBox";
    SWIFTKEY_MODULE_CHANGED: i16 = 0x086bf3b4, "swiftkeyModuleChanged", init 0;
    /// Where the last mouse press was (statics of `si_mouseDown`).
    LAST_X: i16 = 0x086bf542, "last_x.339", init -1;
    LAST_Y: i16 = 0x086bf544, "last_y.340", init -1;
    /// Set by `SI_detectSwiftMouseDSO`'s loader; a function taking the pointer position.
    SI_DETECT_SWIFT_MOUSE_DSO: Addr = 0x088158c0, "SI_detectSwiftMouseDSO";
    /// Width and height of the value field opened over a transformation cell.
    SI_LEFT_BOX: i32 = 0x088158c4, "si_left_box";
    SI_TOP_BOX: i32 = 0x088158c8, "si_top_box";
    SI_RIGHT_BOX: i32 = 0x088158cc, "si_right_box";
    /// Where each module's name starts in the title bar.
    ACTOR_X: i16 = 0x088158d0, "actorX";
    CURR_FRM_B: i16 = 0x088158d2, "currFrmB";
    CURR_FRM_L: i16 = 0x088158d4, "currFrmL";
    CURR_FRM_R: i16 = 0x088158d6, "currFrmR";
    CURR_FRM_T: i16 = 0x088158d8, "currFrmT";
    MATTER_X: i16 = 0x088158da, "matterX";
    MODEL_X: i16 = 0x088158dc, "modelX";
    MOTION_X: i16 = 0x088158de, "motionX";
    SERVER_Y: i16 = 0x088158e0, "serverY";
    SHOW_STAT_B: i16 = 0x088158e2, "showStatB";
    SHOW_STAT_L: i16 = 0x088158e4, "showStatL";
    SHOW_STAT_R: i16 = 0x088158e6, "showStatR";
    SHOW_STAT_T: i16 = 0x088158e8, "showStatT";
    TIME_LIN_B: i16 = 0x088158ea, "timeLinB";
    TIME_LIN_L: i16 = 0x088158ec, "timeLinL";
    TIME_LIN_R: i16 = 0x088158ee, "timeLinR";
    TIME_LIN_T: i16 = 0x088158f0, "timeLinT";
    LOGO_R: i16 = 0x088158f2, "logoR";
    LOGO_L: i16 = 0x088158f4, "logoL";
    TOOLS_X: i16 = 0x088158f6, "toolsX";
    /// The keyboard's auto-repeat setting as it was found.
    KEYWARP: i32 = 0x088158f8, "keywarp";
    SAVED_KEYWARP: i16 = 0x088158fc, "savedKeywarp";
    /// The dialog under the pointer at the last hit test.
    DIALOG_HIT: Addr = 0x08815900, "dialogHit";
    /// Events pushed back, newest first.
    Q_E: [Event; 10] = 0x08815920, "qE";
    /// The event `popQE` hands out.
    D_EVT: Event = 0x08815a10, "dEvt";

    // ---- of other files
    SI_ACT_FRAME: i16 = 0x08679298, "SI_actFrame", init 0;
    SI_ACT_KEYFRM: i16 = 0x0867929a, "SI_actKeyfrm", init 0;
    SI_ACT_LOOP: i16 = 0x0867929c, "SI_actLoop", init 0;
    /// The mouse buttons' devices: 103, 102, 101.
    SI_BUTTON_LEFT: i16 = 0x086792b4, "SI_buttonLeft", init 0x67;
    SI_BUTTON_MIDDLE: i16 = 0x086792b6, "SI_buttonMiddle", init 0x66;
    SI_BUTTON_RIGHT: i16 = 0x086792b8, "SI_buttonRight", init 0x65;
    /// Bit 0x20: the colour box is open.
    SI_CANVAS_MASK: i16 = 0x086792ba, "SI_canvasMask", init 0;
    /// The pointer's valuators: 266 (MOUSEX), 267 (MOUSEY).
    SI_COORDX: i16 = 0x086792be, "SI_coordx", init 0x10a;
    SI_COORDY: i16 = 0x086792c0, "SI_coordy", init 0x10b;
    /// Clock ticks (`times`) within which a second press is a double click.
    SI_DOUBLE_CLICK_DELAY: i32 = 0x08679310, "SI_doubleClickDelay", init 30;
    SI_FAST_PLAY_BACK_ACTIVE: i16 = 0x08679324, "SI_fastPlayBackActive", init 0;
    SI_TABLET_ACTIVE: i32 = 0x086793f8, "SI_tabletActive", init 0;
    /// The swift-key mode in force, 0 for none.
    SI_SUPRA_MODE: i32 = 0x08679404, "SI_supraMode", init 0;
    SI_SYSTEM_NBDIAL: i16 = 0x08679408, "SI_systemNbdial", init 0;
    /// The IRIS GL window of the program's main window.
    SI_SYSID: i32 = 0x0867940c, "SI_sysid", init -1;
    /// The module shown: 0 Model, 1 Motion, 2 Actor, 3 Matter, 4 Tools.
    SI_ACTIVE: i16 = 0x08679480, "SI_active", init 0;
    SI_TABLET_BUTTON: i32 = 0x086794b4, "SI_tabletButton", init 0;
    SI_MDL_MOD: i32 = 0x0868b0c4, "SI_mdlMod", init 0;
    SI_PLAY_BACK_RUN: i16 = 0x08695e92, "SI_playBackRun", init 0;
    GB_REDRAW_ROTOSCOPY: i16 = 0x086ba860, "gbRedrawRotoscopy", init 1;
    GN_3D_REDRAWS_NEEDED: i32 = 0x086ba864, "gn3DRedrawsNeeded", init 0;
    /// The canvases: a record whose word at +8 + 16 × module points to that module's canvas.
    SI_NETWORK: Addr = 0x086cad78, "SI_network", init 0;
    /// 0 in the program, -1 without playback, else the stand-alone viewer.
    SI_PLAY_BOX_MODE: i8 = 0x086cad7e, "SI_playBoxMode", init 0;
    SI_SYSTEM_DIALOG: Addr = 0x086cad84, "SI_systemDialog", init 0;
    SI_ENV_DOCUMENTATION: Addr = 0x086cad88, "SI_ENV_DOCUMENTATION", init 0;
    DE_DIALOG_SYS: i16 = 0x086cad8c, "DE_dialogSys", init -20;
    SI_BIT_PLANES: i32 = 0x08781350, "SI_bitPlanes";
    SI_DIALOG_ACTIVE: i16 = 0x08781412, "SI_dialogActive";
    /// Tested by `SI_setTimeline`; no symbol.
    DAT_08781482: i16 = 0x08781482, "";
    /// The font of the interface's text.
    SI_IRISF: i16 = 0x08781790, "SI_irisf";
    /// 0 the full layout, 1 a small screen, 2 to 6 wider or taller ones.
    SI_SCREEN_RES: i16 = 0x087817ae, "SI_screenRes";
    /// Height of a view window's title bar.
    SI_TITLE_HEIGHT: i16 = 0x087817b8, "SI_titleHeight";
    /// The `times` value of the last mouse press.
    SI_TIME_CLK: i32 = 0x087817bc, "SI_timeClk";
    SI_ACTIVE_CANVAS: P<Canvas> = 0x087817f4, "SI_activeCanvas";
    SI_ACTIVE_DIAL_EVENT: P<Event> = 0x087817f8, "SI_activeDialEvent";
    SI_ACTIVE_EVENT: P<Event> = 0x087817fc, "SI_activeEvent";
    SI_FRAME_CURRENT: i32 = 0x08782040, "SI_frameCurrent";
    SI_FRAME_END: i32 = 0x08782044, "SI_frameEnd";
    SI_FRAME_RATE: f32 = 0x0878204c, "SI_frameRate";
    SI_FRAME_START: i32 = 0x08782050, "SI_frameStart";
    /// The screen layout in use: a canvas whose views are the four view windows.
    SI_TEMPLATE: Canvas = 0x08782080, "SI_template";
    /// The mode box: left edge, and at +2 and +6 its right and upper edges.
    SI_MODEPOS: [i16; 4] = 0x088176a0, "si_modepos";
    SI_BAR: i16 = 0x08817724, "si_bar";
    SI_COLOR_BOX_B: i16 = 0x08817726, "si_colorBoxB";
    SI_COLOR_BOX_L: i16 = 0x08817728, "si_colorBoxL";
    SI_COLOR_BOX_R: i16 = 0x0881772a, "si_colorBoxR";
    SI_COLOR_BOX_T: i16 = 0x0881772c, "si_colorBoxT";
    SI_PLAY_BOX_B: i16 = 0x0881772e, "si_playBoxB";
    SI_PLAY_BOX_L: i16 = 0x08817730, "si_playBoxL";
    SI_PLAY_BOX_R: i16 = 0x08817732, "si_playBoxR";
    SI_PLAY_BOX_T: i16 = 0x08817734, "si_playBoxT";
    SI_STATUS_PATH: Addr = 0x08817704, "si_statusPath";
    SI_DEVICE_USE: i32 = 0x08817758, "SI_deviceUse";
    SI_STATUS_LEN: i16 = 0x088177b8, "SI_statusLen";
    SI_STATUS_RIGHT2: i16 = 0x088177ba, "SI_statusRight2";

    // ---- window.c's own
    /// Set once the common corner of the four views has been dragged (`winCusLayHandle`).
    FIRST_CUS_LAY: i16 = 0x086cb77a, "firstCusLay.283", init 0;
    GADGET_LIST_INIT: i16 = 0x086cb8ec, "gadgetListInit.482", init 0;
    /// Static of `SI_windowGadjet`: the font's descender, asked for once (−1: not yet).
    BASE_445: i32 = 0x086cb81c, "base.445", init -1;
    /// Statics of `SI_windowGhostMenu`: "first call" and what the first call found.
    FPASS: i32 = 0x086cb77c, "fpass.314", init 1;
    MOTION_FLAG: i32 = 0x08817890, "motionFlag.315";
    /// What a frozen window showed before it was frozen: display mode, and the word at
    /// +0x68; by window identifier − 1000.
    FREEZE_DTYP: [i32; 4] = 0x08817eb8, "freeze_dtyp";
    FREEZE_LTYP: [i32; 4] = 0x08817ec8, "freeze_ltyp";
    /// The four views' inner edges before the first drag of their common corner, in the
    /// order the code saves them: right of window 0, left of 2, right of 1, left of 3, top
    /// of 0, bottom of 1, top of 2, bottom of 3.
    WIN_CENTER: [i16; 8] = 0x08817880, "winCenter.282";
    /// The parts of a view window, by part number.
    GADGET_INFO: [Gadget; 55] = 0x088178a0, "gadgetInfo";
    /// What `getbuffer` said when `SI_windowSet` was called.
    BUFFER: i32 = 0x08817ea4, "buffer";
    /// The display mode each view had before it was switched off for a redraw, by window
    /// identifier − 1000.
    TOGGLE_RGB: [i32; 4] = 0x08817ea8, "toggleRgb";

    /// The left and bottom edges of window 1000 and the right and top edges of window 1003
    /// as the layout file gave them: the full extent of the four views.
    WIN_BORDER: [i16; 4] = 0x08817f00, "winBorder";
    /// The rectangle each window had before it was expanded (left, right, bottom, top), by
    /// window identifier − 1000; all zero while not expanded.
    WIN_POS: [i16; 16] = 0x08817ee0, "winPos";

    /// Statics of `SI_windowRestoreAll`: "first call", and the flag the first call sets
    /// to 1 (a product-tier flag: see the function).
    FPASS_328: i32 = 0x086cb780, "fpass.328", init 1;
    MOTION_LICENSE_329: i16 = 0x08817894, "motion_license.329";
    /// Statics of `SI_enableSchemCleanAll`: "first call" and the two offsets it saved.
    FIRST_TIME_657: i16 = 0x086cb8f6, "firstTime.657", init 1;
    SAVE_CLEAN_LOFFSET_658: i32 = 0x08817898, "saveClean_loffset.658";
    SAVE_CLEAN_ALL_LOFFSET_659: i32 = 0x0881789c, "saveCleanAll_loffset.659";
    /// Statics of `SI_changeSetOfViews`: the window that was zoomed when the function-curve
    /// set of views was entered (index, −1 for none), and the lower left window's view-type
    /// entry and expansion then.
    ZOOM_WIN_ID_666: i16 = 0x086cb8f8, "zoomWinId.666", init -1;
    FRONT_CAM_ID_667: i16 = 0x086cb8fa, "frontCamId.667", init 0;
    FRONT_SIZE_668: i32 = 0x086cb8fc, "frontSize.668", init 0;
    /// Statics of `SI_windowLimitsHoriz`: the smallest and largest magnitude a limit may
    /// have, as float bits (the smallest normal float, and a little under half the largest).
    MINFLOAT_420: u32 = 0x086cb80c, "minfloat.420", init 0x00800000;
    MAXFLOAT_421: u32 = 0x086cb810, "maxfloat.421", init 0x7effffff;
    /// The same two for `SI_windowLimitsVert`.
    MINFLOAT_425: u32 = 0x086cb814, "minfloat.425", init 0x00800000;
    MAXFLOAT_426: u32 = 0x086cb818, "maxfloat.426", init 0x7effffff;

    // ---- of other files, used by window.c
    /// Non-zero in the stand-alone renderer, which has no views.
    SI_STANDALONE_RENDERER: i16 = 0x08679400, "SI_standaloneRenderer", init 0;
    /// The program's processing data; its first word is the index of the current scene's
    /// element dictionary.
    SI_PROC_DATA: Addr = 0x08679314, "SI_procData", init 0x08782108;
    /// The element dictionaries, 0x70 bytes each.
    SI_ELEM_DICTS: Addr = 0x0867931c, "SI_elemDicts", init 0;
    /// Non-zero while the schematic view shows hierarchies open.
    SI_SCHEMATIC_MODE: i16 = 0x0868b0c8, "SI_schematicMode", init 0;
    /// Cleared when a dope-sheet window is hidden; no symbol.
    DAT_0878FAA8: i16 = 0x0878faa8, "";
    /// The dope sheet's "ACT_KEY" switch; no symbol.
    DAT_0878FB22: i16 = 0x0878fb22, "";
    /// The schematic view's mode bits as last chosen; no symbol.
    DAT_087FB96C: i32 = 0x087fb96c, "";
    /// Set when a window becomes a dope sheet; no symbol (inside `SI_audioContainer`).
    DAT_087FB9C8: i32 = 0x087fb9c8, "";
    /// The lists of drawn objects, 36 bytes each (finding 0017).
    SI_GRAF_LISTS: Addr = 0x08679498, "SI_grafLists", init 0;
    /// The list of the scene's objects.
    SI_SCNGL: Addr = 0x08695e9c, "SI_scngl", init 0;
    SI_NO_OVERLAY_REFRESH_FIX: i16 = 0x08679402, "SI_noOverlayRefreshFix";
    /// The filled-screen-box routine in use; `sboxfi` unless the hardware check chose another.
    SI_SBOXFI: Addr = 0x086ee2a4, "SI_sboxfi", init 0x0861236c;
    SI_CTRL_HEIGHT: i16 = 0x08781386, "SI_ctrlHeight";
    SI_CTRL_WIDTH: i16 = 0x0878138a, "SI_ctrlWidth";
    SI_DIALOG_DARK: i16 = 0x0878141a, "SI_dialogDark";
    SI_DIALOG_GRAY: i16 = 0x0878141e, "SI_dialogGray";
    SI_DOV_RESETX: i16 = 0x08781436, "SI_dovResetx";
    SI_MEMO_LEFT_OFFSET: i16 = 0x087815a4, "SI_memoLeftOffset";
    SI_MEMO_SPACING: i16 = 0x087815a6, "SI_memoSpacing";
    SI_MEMO_WIDTH: i16 = 0x087815a8, "SI_memoWidth";
    SI_XMAXSCREEN: i16 = 0x08781794, "SI_xmaxscreen";
    SI_VIEW_BACK: i16 = 0x087817ce, "SI_viewBack";
    SI_WHOLE_SYSTEM_WM: i16 = 0x087817dc, "SI_wholeSystemWM";
    SI_WINDOW_BACK: i16 = 0x087817de, "SI_windowBack";
    SI_WINDOW_FRAME_HILITE: i16 = 0x087817e0, "SI_windowFrameHilite";
    SI_WINDOW_TEXT: i16 = 0x087817e2, "SI_windowText";
    SI_WINDOW_VIEWPORT: i16 = 0x087817e8, "SI_windowViewport";
    /// The font of the title bars' text.
    SI_APPF: i16 = 0x08781644, "SI_appf";
    SI_CONTROL_TEXT_HILITE: i16 = 0x08781380, "SI_controlTextHilite";
    SI_WINDOW_TEXT_HILITE: i16 = 0x087817e4, "SI_windowTextHilite";
    SI_WINDOW_TITLE: i16 = 0x087817e6, "SI_windowTitle";
    /// Non-zero while the schematic view's CLEAN_ALL gadget is not shown
    /// (`SI_enableSchemCleanAll`); no symbol of its own (`SI_glbSchem` + 0x4c).
    DAT_087FB9AC: i16 = 0x087fb9ac, "";
    SI_TITLE_LAYOUT: i16 = 0x08817708, "si_titleLayout";
    SI_TITLE_SETUP: i16 = 0x0881770a, "si_titleSetup";

    /// The C library's `stderr`, a variable the dynamic linker copies into the program's
    /// own data: a `FILE *` ([`crate::file`]); `Ui::new` sets it as the start-up does.
    STDERR: Addr = 0x08780c88, "stderr@@GLIBC_2.0";

    // ---- drawWindow.c's own
    /// The list of shader windows, null until the first is opened.
    SHADER_WINDOW_ARRAY: P<SimpleList> = 0x086be2a0, "shaderWindowArray", init 0;
    /// Of `dialog.c`: non-zero when a dialog was moved.
    SI_DIALOG_MOVED: i16 = 0x086bd9e0, "si_dialogMoved", init 0;

    // ---- graf.c's and ui_misc.c's own, and what they use of other files
    /// Where the pointer was when the program's window was iconified (−1: not noted).
    UNSTOW_CUR_X: i32 = 0x086bf7c0, "unstow_curX", init -1;
    UNSTOW_CUR_Y: i32 = 0x086bf7c4, "unstow_curY", init -1;
    /// Statics of `SI_showActivity`: the next frame of the busy cursor (11 down to 6) and
    /// the time of the next change in seconds, as float bits.
    CURSOR_ID_240: i32 = 0x086cada4, "cursorID.240", init 11;
    PREV_TIME_241: u32 = 0x086cada8, "prevTime.241", init 0;
    /// Non-zero while long operations show the busy cursor.
    SI_BUSY_CURSOR: i16 = 0x086792b2, "SI_busyCursor", init 1;
    /// Non-zero while a view shows a picture in a window of its own.
    SI_RGB_ACTIVE: i16 = 0x086793d0, "SI_rgbActive", init 0;
    /// Non-zero while the automatic backup is on.
    SI_BACKUP_ACTIVE: i16 = 0x086cad74, "SI_backupActive", init 0;
    /// The backup's period in seconds and the time of the last one.
    SI_AUTOSAVEPERIODE: i32 = 0x088177c4, "SI_autosaveperiode";
    SI_AUTOSAVETIME: i32 = 0x088177c8, "SI_autosavetime";
    /// The list of databases (`SI_DbDir`); only its address is used here.
    SI_DBDIR: [u8; 12] = 0x08781390, "SI_dbdir";
    /// The name of the work file of the scene.
    SI_DB_WORK_FNAME: [u8; 1024] = 0x08788140, "SI_dBWorkFname";
    /// The colour of each module's name bar: three 16-bit words (red, green, blue) a module.
    SI_APPCOL: [i16; 15] = 0x08817736, "SI_appcol";
    SI_YMAXSCREEN: i16 = 0x08781796, "SI_ymaxscreen";

    // ---- init_graphic.c's own, and everything SI_colorInit sets (colours as indices of
    // the colour map, write masks, the measures of the fonts and of the title bars)
    /// The colour map the program found, and its own: 501 entries of red, green, blue
    /// (16 bits each), the last four those of the overlay planes.
    PREV_MAP: [i16; 1503] = 0x08815c60, "PREV_MAP";
    SOFT_MAP: [i16; 1503] = 0x08816820, "SOFT_MAP";
    SI_FOREGROUND: i16 = 0x08781462, "SI_foreground";
    SI_OLBIT_PLANES: i32 = 0x087815b4, "SI_olbitPlanes";
    SI_BACKGROUND: i16 = 0x08781340, "SI_background";
    SI_BASELINE: i16 = 0x08781342, "SI_baseline";
    SI_BUTTON_BACK: i16 = 0x08781344, "SI_buttonBack";
    SI_BUTTON_BACK_HILITE1: i16 = 0x08781346, "SI_buttonBackHilite1";
    SI_BUTTON_BACK_HILITE2: i16 = 0x08781348, "SI_buttonBackHilite2";
    SI_BUTTON_SHADOW: i16 = 0x0878134a, "SI_buttonShadow";
    SI_BUTTON_SHADOW_HILITE: i16 = 0x0878134c, "SI_buttonShadowHilite";
    SI_COLOR1: i16 = 0x08781354, "SI_color1";
    SI_COLOR10: i16 = 0x08781356, "SI_color10";
    SI_COLOR11: i16 = 0x08781358, "SI_color11";
    SI_COLOR12: i16 = 0x0878135a, "SI_color12";
    SI_COLOR2: i16 = 0x0878135c, "SI_color2";
    SI_COLOR3: i16 = 0x0878135e, "SI_color3";
    SI_COLOR4: i16 = 0x08781360, "SI_color4";
    SI_COLOR5: i16 = 0x08781362, "SI_color5";
    SI_COLOR6: i16 = 0x08781364, "SI_color6";
    SI_COLOR7: i16 = 0x08781366, "SI_color7";
    SI_COLOR8: i16 = 0x08781368, "SI_color8";
    SI_COLOR9: i16 = 0x0878136a, "SI_color9";
    SI_COLOR_BOX_HILITE: i16 = 0x0878136c, "SI_colorBoxHilite";
    SI_COLOR_FIRST: i16 = 0x0878136e, "SI_colorFirst";
    SI_COLOR_LAST: i16 = 0x08781370, "SI_colorLast";
    SI_COLOR_NB: i16 = 0x08781372, "SI_colorNB";
    SI_CONSTANT_X: i16 = 0x08781374, "SI_constantX";
    SI_CONTROLER_BACK: i16 = 0x08781376, "SI_controlerBack";
    SI_CONTROLER_BACK2: i16 = 0x08781378, "SI_controlerBack2";
    SI_CONTROL_BACK: i16 = 0x0878137a, "SI_controlBack";
    SI_CONTROL_BACK_HILITE: i16 = 0x0878137c, "SI_controlBackHilite";
    SI_CONTROL_TEXT: i16 = 0x0878137e, "SI_controlText";
    SI_CTRL_CAM_BIG_RAY: i16 = 0x08781382, "SI_ctrlCamBigRay";
    SI_CTRL_CAM_SMALL_RAY: i16 = 0x08781384, "SI_ctrlCamSmallRay";
    SI_CTRL_RESET_HEIGHT: i16 = 0x08781388, "SI_ctrlResetHeight";
    SI_CTRL_ZONE_MARGIN_X: i16 = 0x0878138c, "SI_ctrlZoneMarginX";
    SI_CTRL_ZONE_MARGIN_Y: i16 = 0x0878138e, "SI_ctrlZoneMarginY";
    SI_DESCEND: i16 = 0x08781410, "SI_descend";
    SI_DIALOG_BACK: i16 = 0x08781414, "SI_dialogBack";
    SI_DIALOG_BUT: i16 = 0x08781416, "SI_dialogBut";
    SI_DIALOG_CLEAR: i16 = 0x08781418, "SI_dialogClear";
    SI_DIALOG_GHOST: i16 = 0x0878141c, "SI_dialogGhost";
    SI_DIALOG_LIGHT: i16 = 0x08781420, "SI_dialogLight";
    SI_DIALOG_TITLE: i16 = 0x08781422, "SI_dialogTitle";
    SI_DLEADING: i16 = 0x08781424, "SI_dleading";
    SI_DOPE_DIALOG1: i16 = 0x08781426, "SI_dopeDialog1";
    SI_DOPE_DIALOG2: i16 = 0x08781428, "SI_dopeDialog2";
    SI_DOPE_DIALOG3: i16 = 0x0878142a, "SI_dopeDialog3";
    SI_DOPE_DIALOG4: i16 = 0x0878142c, "SI_dopeDialog4";
    SI_DOPE_DIALOG5: i16 = 0x0878142e, "SI_dopeDialog5";
    SI_DOPE_DIALOG_BACK: i16 = 0x08781430, "SI_dopeDialogBack";
    SI_DOPE_DIALOG_FORE: i16 = 0x08781432, "SI_dopeDialogFore";
    SI_DOPE_DIALOG_HILITE: i16 = 0x08781434, "SI_dopeDialogHilite";
    SI_FCURVE_COLOR1: i16 = 0x0878145c, "SI_fcurveColor1";
    SI_FCURVE_COLOR2: i16 = 0x0878145e, "SI_fcurveColor2";
    SI_FCURVE_COLOR3: i16 = 0x08781460, "SI_fcurveColor3";
    SI_FULL_SYSTEM_WM: i32 = 0x08781468, "SI_fullSystemWM";
    SI_GRID_DARK: i16 = 0x0878146c, "SI_gridDark";
    SI_GRID_LIGHT: i16 = 0x0878146e, "SI_gridLight";
    SI_GRID_WM: i16 = 0x08781470, "SI_gridWM";
    SI_X_AXIS_COLOR: i16 = 0x08781472, "SI_X_Axis_Color";
    SI_Y_AXIS_COLOR: i16 = 0x08781474, "SI_Y_Axis_Color";
    SI_Z_AXIS_COLOR: i16 = 0x08781476, "SI_Z_Axis_Color";
    SI_MARGIN: i16 = 0x087815a0, "SI_margin";
    SI_MEMO_HEIGHT: i16 = 0x087815a2, "SI_memoHeight";
    SI_MENU1_WM: i16 = 0x087815aa, "SI_menu1WM";
    SI_MENU2_WM: i16 = 0x087815ac, "SI_menu2WM";
    SI_MENU3_WM: i16 = 0x087815ae, "SI_menu3WM";
    SI_OBJECT_WM: i16 = 0x087815b0, "SI_objectWM";
    SI_OVERPLANE: i32 = 0x087815cc, "SI_overplane";
    SI_PLAY_HILITE1: i16 = 0x087815d2, "SI_playHilite1";
    SI_PLAY_HILITE2: i16 = 0x087815d4, "SI_playHilite2";
    SI_POPUP_BACKGRND: i16 = 0x087815d6, "SI_popupBackgrnd";
    SI_POPUP_CNTR: i16 = 0x087815d8, "SI_popupCntr";
    SI_POPUP_DEACTIVATE: i16 = 0x087815da, "SI_popupDeactivate";
    SI_POPUP_GHOST: i16 = 0x087815dc, "SI_popupGhost";
    SI_POPUP_HICON: i16 = 0x087815de, "SI_popupHicon";
    SI_POPUP_HILITE: i16 = 0x087815e0, "SI_popupHilite";
    SI_POPUP_HTEXT: i16 = 0x087815e2, "SI_popupHtext";
    SI_POPUP_ICON: i16 = 0x087815e4, "SI_popupIcon";
    SI_POPUP_NULL: i16 = 0x087815e6, "SI_popupNull";
    SI_POPUP_SHADOW: i16 = 0x087815e8, "SI_popupShadow";
    SI_POPUP_TEXT: i16 = 0x087815ea, "SI_popupText";
    SI_SELECT1_WM: i16 = 0x0878162c, "SI_select1WM";
    SI_SELECT2_WM: i16 = 0x0878162e, "SI_select2WM";
    SI_SELECT3_WM: i16 = 0x08781630, "SI_select3WM";
    SI_SELECT4_WM: i16 = 0x08781632, "SI_select4WM";
    SI_SELECT5_WM: i16 = 0x08781634, "SI_select5WM";
    SI_SELECT6_WM: i16 = 0x08781636, "SI_select6WM";
    SI_SELECT7_WM: i16 = 0x08781638, "SI_select7WM";
    SI_SELECT_STEP: i16 = 0x0878163a, "SI_selectStep";
    SI_SERVER_BACK: i16 = 0x0878163c, "SI_serverBack";
    SI_SERVER_TEXT: i16 = 0x0878163e, "SI_serverText";
    SI_SERVER_TEXT_HILITE: i16 = 0x08781640, "SI_serverTextHilite";
    SI_SHADOW_MASK: i16 = 0x08781642, "SI_shadowMask";
    SI_SLIDER_COLOR: i16 = 0x08781798, "SI_sliderColor";
    SI_SLOPE_X: i16 = 0x0878179a, "SI_slopeX";
    SI_STATUS_BACK: i16 = 0x087817a2, "SI_statusBack";
    SI_STATUS_LIGHT: i16 = 0x087817a4, "SI_statusLight";
    SI_STATUS_TEXT: i16 = 0x087817a6, "SI_statusText";
    SI_STATUS_WARNING1: i16 = 0x087817a8, "SI_statusWarning1";
    SI_STATUS_WARNING2: i16 = 0x087817aa, "SI_statusWarning2";
    SI_SYSTEM_WM: i16 = 0x087817ac, "SI_systemWM";
    SI_TEXASCEND: i16 = 0x087817b0, "SI_texascend";
    SI_TEXDESCEND: i16 = 0x087817b2, "SI_texdescend";
    SI_TEXHEIGHT: i16 = 0x087817b4, "SI_texheight";
    SI_TEXWIDTH: i16 = 0x087817b6, "SI_texwidth";
    SI_ULEADING: i16 = 0x087817c2, "SI_uleading";
    SI_UNSELECT_WM: i16 = 0x087817c4, "SI_unselectWM";
    SI_VIEW_AXIS: i16 = 0x087817cc, "SI_viewAxis";
    SI_VIEW_FIELD1: i16 = 0x087817d0, "SI_viewField1";
    SI_VIEW_FIELD2: i16 = 0x087817d2, "SI_viewField2";
    SI_VIEW_FIELD3: i16 = 0x087817d4, "SI_viewField3";
    SI_VIEW_FIELD4: i16 = 0x087817d6, "SI_viewField4";
    SI_VIEW_RULER: i16 = 0x087817d8, "SI_viewRuler";
    SI_VIEW_RULER_BACK: i16 = 0x087817da, "SI_viewRulerBack";
    SI_CPKEY_X: i16 = 0x08817660, "si_cpkeyX";
    SI_D_SACTIV_X: i16 = 0x08817662, "si_dSActivX";
    SI_D_SCLEAR_X: i16 = 0x08817664, "si_dSClearX";
    SI_ORTHO_WIDGET_X: i16 = 0x08817666, "si_orthoWidgetX";
    SI_ORTHO_WIDGET_Y: i16 = 0x08817668, "si_orthoWidgetY";
    SI_ORTHO_WIDGET_Z: i16 = 0x0881766a, "si_orthoWidgetZ";
    SI_SPREAD_SHEET_FIELDS: i16 = 0x0881766c, "si_spreadSheetFields";
    SI_SPREAD_SHEET_QUERY: i16 = 0x0881766e, "si_spreadSheetQuery";
    SI_SPREAD_SHEET_SORT: i16 = 0x08817670, "si_spreadSheetSort";
    SI_SPREAD_SHEET_EXEC: i16 = 0x08817672, "si_spreadSheetExec";
    SI_SPREAD_SHEET_SELECT: i16 = 0x08817674, "si_spreadSheetSelect";
    SI_SPREAD_SHEET_DE_SELECT: i16 = 0x08817676, "si_spreadSheetDeSelect";
    SI_SPREAD_SHEET_QUERIES: i16 = 0x08817678, "si_spreadSheetQueries";
    SI_D_SCOPY_X: i16 = 0x0881767a, "si_dSCopyX";
    SI_D_SCUT_X: i16 = 0x0881767c, "si_dSCutX";
    SI_D_SLINK_X: i16 = 0x0881767e, "si_dSLinkX";
    SI_D_SPASTE_X: i16 = 0x08817680, "si_dSPasteX";
    SI_D_SUNACTIV_X: i16 = 0x08817682, "si_dSUnactivX";
    SI_D_SUNLINK_X: i16 = 0x08817684, "si_dSUnlinkX";
    SI_INTERP_X: i16 = 0x08817686, "si_interpX";
    SI_LINEAR_X: i16 = 0x08817688, "si_linearX";
    SI_SETKEY_X: i16 = 0x088176fc, "si_setkeyX";
    SI_SPLINE_X: i16 = 0x088176fe, "si_splineX";
    SI_SLOPE0_X: i16 = 0x08817700, "si_slope0X";
    SI_PLATEAU_X: i16 = 0x08817702, "si_plateauX";
    SI_ASCEND: i16 = 0x08817754, "SI_ascend";
    SI_EDITFCV_X: i16 = 0x0881775e, "SI_editfcvX";
    SI_EDITKEY_X: i16 = 0x08817760, "SI_editkeyX";
    SI_EDITTAG_X: i16 = 0x08817762, "SI_edittagX";
    // ---- scroll.c: colour mode, the arrow offsets, the RGB palette of the dialogs
    /// Non-zero when the program draws in RGB mode (`RGBcolor`) rather than colour-map mode.
    SI_RGBMODE: i16 = 0x086794a4, "SI_rgbmode", init 0;
    /// Half height of the tuning button's arrow heads.
    SI_ARR_H_OFF: i16 = 0x088177c0, "SI_arrHOff";
    /// Half width of the tuning button's arrow heads.
    SI_ARR_W_OFF: i16 = 0x088177c2, "SI_arrWOff";
    /// A label button's radius (`SI_scrollSize`).
    RADIUS: i32 = 0x088175f4, "radius";
    SI_RGB_DIAL_BACK_B: i16 = 0x08817764, "SI_rgbDialBackB";
    SI_RGB_DIAL_BACK_G: i16 = 0x08817766, "SI_rgbDialBackG";
    SI_RGB_DIAL_BACK_R: i16 = 0x08817768, "SI_rgbDialBackR";
    SI_RGB_DIAL_BUTT_B: i16 = 0x0881776a, "SI_rgbDialButtB";
    SI_RGB_DIAL_BUTT_G: i16 = 0x0881776c, "SI_rgbDialButtG";
    SI_RGB_DIAL_BUTT_R: i16 = 0x0881776e, "SI_rgbDialButtR";
    SI_RGB_DIAL_DARK_B: i16 = 0x08817770, "SI_rgbDialDarkB";
    SI_RGB_DIAL_DARK_G: i16 = 0x08817772, "SI_rgbDialDarkG";
    SI_RGB_DIAL_DARK_R: i16 = 0x08817774, "SI_rgbDialDarkR";
    SI_RGB_DIAL_GHOST_B: i16 = 0x08817776, "SI_rgbDialGhostB";
    SI_RGB_DIAL_GHOST_G: i16 = 0x08817778, "SI_rgbDialGhostG";
    SI_RGB_DIAL_GHOST_R: i16 = 0x0881777a, "SI_rgbDialGhostR";
    SI_RGB_DIAL_GRAY_B: i16 = 0x0881777c, "SI_rgbDialGrayB";
    SI_RGB_DIAL_GRAY_G: i16 = 0x0881777e, "SI_rgbDialGrayG";
    SI_RGB_DIAL_GRAY_R: i16 = 0x08817780, "SI_rgbDialGrayR";
    SI_RGB_DIAL_LIGHT_B: i16 = 0x08817788, "SI_rgbDialLightB";
    SI_RGB_DIAL_LIGHT_G: i16 = 0x0881778a, "SI_rgbDialLightG";
    SI_RGB_DIAL_LIGHT_R: i16 = 0x0881778c, "SI_rgbDialLightR";
    SI_RGB_DIAL_TITL_B: i16 = 0x0881778e, "SI_rgbDialTitlB";
    SI_RGB_DIAL_TITL_G: i16 = 0x08817790, "SI_rgbDialTitlG";
    SI_RGB_DIAL_TITL_R: i16 = 0x08817792, "SI_rgbDialTitlR";
    // ---- Controls: scrollbar.c, slider.c, ruler.c, scroll.c first slice
    /// Non-zero while a dialog is open (`drawSliderBut` draws in the overlay planes then).
    SI_DIALOGS_OPEN: i16 = 0x0867930c, "SI_dialogsOpen";
}

/// The address of the IRIS GL layer's `sboxfi`, the initial value of `SI_sboxfi`.
pub const SBOXFI: u32 = 0x0861_236c;

/// The key devices of `supraKeys` at 0x086be9c0, in order: 63 little-endian `i16` (126 bytes,
/// the last a zero). Not in this source: read from the user's own Softimage|3D 4.0 install
/// when [`init`] runs (`si-rt`, release model; `docs/design/runtime-data.md`), checked
/// against our checksum.
pub static SUPRA_KEYS_INIT: si_rt::Table = si_rt::Table::new("si_ui.supraKeys", 0x086b_e9c0, 126, 0x1434_bd98_e1fd_f55e);

/// Where the first action string of `descSRT` lies; the 81 strings follow one another.
pub const DESC_SRT_STRINGS: u32 = 0x086bea3e;
/// Where the first action string of `descPlayBox` lies.
pub const DESC_PLAY_BOX_STRINGS: u32 = 0x086bf1c8;

/// The 81 action strings of the transformation boxes in table order: for each of Scale,
/// Rotate, Translate and each of X, Y, Z: the cell itself, the dialog, the edit field, each
/// with " / L", " / M", " / R" for the mouse button.
pub fn desc_srt_strings() -> Vec<String> {
    let mut out = Vec::with_capacity(81);
    for (op, prep) in [("Scale", "in"), ("Rotate", "about"), ("Translate", "in")] {
        for axis in ["X", "Y", "Z"] {
            for part in [format!("{op} {prep} {axis}"), format!("{op} Dialog"), format!("Edit {op} {prep} {axis}")] {
                for button in ["L", "M", "R"] {
                    out.push(format!("{part} / {button}"));
                }
            }
        }
    }
    out
}

/// The 21 action strings of the play box in table order.
pub fn desc_play_box_strings() -> Vec<String> {
    let mut out = Vec::with_capacity(21);
    for name in ["start", "end", "backward", "forward", "frame", "keyframe", "loop"] {
        for button in ["L", "M", "R"] {
            out.push(format!("Playbox {name} / {button}"));
        }
    }
    out
}

/// Write the initial values: the scalars of the table, the swift-key devices, and the two
/// tables of action strings with their strings at the original's addresses.
pub fn init(m: &mut Mem) {
    crate::data::init(m);
    for g in ALL {
        if let Some(v) = g.init {
            m.set_bytes(g.addr, &v.to_le_bytes()[..g.size as usize]);
        }
    }
    let keys = SUPRA_KEYS_INIT.get();
    m[SUPRA_KEYS] = std::array::from_fn(|i| i16::from_le_bytes([keys[2 * i], keys[2 * i + 1]]));
    let mut at = DESC_SRT_STRINGS;
    for (i, s) in desc_srt_strings().iter().enumerate() {
        m.put_cstr(at, s.as_bytes());
        m[DESC_SRT][i] = at;
        at += s.len() as u32 + 1;
    }
    let mut at = DESC_PLAY_BOX_STRINGS;
    for (i, s) in desc_play_box_strings().iter().enumerate() {
        m.put_cstr(at, s.as_bytes());
        m[DESC_PLAY_BOX][i] = at;
        at += s.len() as u32 + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_action_tables_are_where_the_original_has_them() {
        si_rt::skip_without_install!();
        let mut m = crate::mem::image();
        init(&mut m);
        assert_eq!(m[DESC_SRT][0], 0x086bea3e);
        assert_eq!(m.cstr(m[DESC_SRT][6]), b"Edit Scale in X / L");
        assert_eq!(m[DESC_SRT][76], 0x086bf005);
        assert_eq!(m.cstr(m[DESC_SRT][80]), b"Edit Translate in Z / R");
        assert_eq!(m[DESC_PLAY_BOX][20], 0x086bf340);
        assert_eq!(m.cstr(m[DESC_PLAY_BOX][20]), b"Playbox loop / R");
        assert_eq!(m[SI_BUTTON_LEFT], 103);
        assert_eq!(m[SWIFTMODE], -1);
        // the swift-key devices: the first and the last are what the original's table starts and ends with
        assert_eq!((m[SUPRA_KEYS][0], m[SUPRA_KEYS][62]), (83, 0));
    }
}
