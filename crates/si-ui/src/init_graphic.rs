// SPDX-License-Identifier: MIT
//! `init_graphic.c` of the program (0x0839be00): the patterns, cursors, colours and
//! measures the interface is drawn with, and the colour maps.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.

use crate::data::bitmap;
use crate::globals::*;
use crate::mem::P;
use crate::sys::LogArg;
use crate::{fault, Ui};

// What `SI_colorInit` stores, block by block (`re/tools/si-ui/gen_colorinit.py tables`
// reads the routine's stores from the binary; the tests between the blocks are in the
// function). All 16-bit variables; the two 32-bit ones are set in the function.
const PLANES8_FIRST: &[(P<i16>, i16)] = &[
    (SI_BACKGROUND, 0), (SI_DIALOG_CLEAR, 0), (SI_DIALOG_LIGHT, 1), (SI_DIALOG_DARK, 2), (SI_DIALOG_GRAY, 3),
    (SI_DIALOG_BACK, 4), (SI_DIALOG_BUT, 5), (SI_DIALOG_TITLE, 6), (SI_DIALOG_GHOST, 7), (SI_SERVER_TEXT, 0),
    (SI_SERVER_TEXT_HILITE, 0xa),
];
const PLANES8_EDITOR: &[(P<i16>, i16)] = &[
    (SI_SERVER_BACK, 0xb), (SI_BUTTON_BACK, 8), (SI_BUTTON_SHADOW, 3), (SI_BUTTON_BACK_HILITE1, 9), (SI_VIEW_BACK, 8),
    (SI_WINDOW_BACK, 0xd),
];
const PLANES8_PROGRAM: &[(P<i16>, i16)] = &[
    (SI_SERVER_BACK, 4), (SI_BUTTON_BACK, 1), (SI_BUTTON_SHADOW, 1), (SI_BUTTON_BACK_HILITE1, 8), (SI_VIEW_BACK, 1),
    (SI_WINDOW_BACK, 2),
];
const PLANES8_REST: &[(P<i16>, i16)] = &[
    (SI_CONTROL_BACK, 0xc), (SI_CONTROL_TEXT, 1), (SI_BUTTON_BACK_HILITE2, 8), (SI_BUTTON_SHADOW_HILITE, 8),
    (SI_CONTROL_BACK_HILITE, 0xc), (SI_CONTROL_TEXT_HILITE, 0xa), (SI_WINDOW_TEXT, 0), (SI_WINDOW_TITLE, 0xe),
    (SI_WINDOW_VIEWPORT, 1), (SI_WINDOW_TEXT_HILITE, 1), (SI_WINDOW_FRAME_HILITE, 0xe), (SI_STATUS_BACK, 0xc),
    (SI_STATUS_LIGHT, 1), (SI_STATUS_TEXT, 0), (SI_SLIDER_COLOR, 0xc), (SI_STATUS_WARNING1, 0xa),
    (SI_STATUS_WARNING2, 0xa), (SI_PLAY_HILITE1, 8), (SI_PLAY_HILITE2, 1), (SI_GRID_LIGHT, 0), (SI_CONTROLER_BACK, 1),
    (SI_CONTROLER_BACK2, 1), (SI_VIEW_AXIS, 0), (SI_VIEW_RULER_BACK, 1), (SI_VIEW_RULER, 0), (SI_VIEW_FIELD1, 0),
    (SI_VIEW_FIELD2, 1), (SI_VIEW_FIELD3, 1), (SI_VIEW_FIELD4, 1), (SI_COLOR_BOX_HILITE, 0xa), (SI_COLOR1, 0),
    (SI_COLOR2, 0), (SI_COLOR3, 0), (SI_COLOR4, 0), (SI_COLOR5, 0), (SI_COLOR6, 0), (SI_COLOR7, 0), (SI_COLOR8, 0),
    (SI_COLOR9, 0), (SI_COLOR10, 0), (SI_COLOR11, 0), (SI_COLOR12, 0), (SI_COLOR_FIRST, 0), (SI_COLOR_LAST, 0),
    (SI_COLOR_NB, 1), (SI_FCURVE_COLOR1, 0), (SI_FCURVE_COLOR2, 0), (SI_FCURVE_COLOR3, 0), (SI_DOPE_DIALOG_FORE, 0),
    (SI_DOPE_DIALOG_BACK, 1), (SI_DOPE_DIALOG_HILITE, 0xa), (SI_DOPE_DIALOG1, 7), (SI_DOPE_DIALOG2, 5),
    (SI_DOPE_DIALOG3, 2), (SI_DOPE_DIALOG4, 0xc), (SI_DOPE_DIALOG5, 6), (SI_SYSTEM_WM, 0xf),
    (SI_WHOLE_SYSTEM_WM, 0xf), (SI_UNSELECT_WM, 1), (SI_OBJECT_WM, 0xf), (SI_SELECT1_WM, 2), (SI_SELECT2_WM, 4),
    (SI_SELECT3_WM, 6), (SI_SELECT4_WM, 8), (SI_SELECT5_WM, 0xa), (SI_SELECT6_WM, 0xc), (SI_SELECT7_WM, 0xe),
    (SI_SELECT_STEP, 2), (SI_GRID_WM, 0xf), (SI_GRID_DARK, 0), (SI_X_AXIS_COLOR, 0), (SI_Y_AXIS_COLOR, 0),
    (SI_Z_AXIS_COLOR, 0),
];
const DEEP: &[(P<i16>, i16)] = &[
    (SI_BACKGROUND, 0), (SI_DIALOG_CLEAR, 0), (SI_DIALOG_LIGHT, 1), (SI_DIALOG_DARK, 2), (SI_DIALOG_GRAY, 3),
    (SI_DIALOG_BACK, 4), (SI_DIALOG_BUT, 5), (SI_DIALOG_TITLE, 6), (SI_DIALOG_GHOST, 7), (SI_SERVER_BACK, 4),
    (SI_SERVER_TEXT, 5), (SI_SERVER_TEXT_HILITE, 6), (SI_BUTTON_BACK, 8), (SI_BUTTON_SHADOW, 9),
    (SI_CONTROL_BACK, 0xa), (SI_CONTROL_TEXT, 0xb), (SI_BUTTON_BACK_HILITE1, 0xc), (SI_BUTTON_BACK_HILITE2, 0x10),
    (SI_BUTTON_SHADOW_HILITE, 0xd), (SI_CONTROL_BACK_HILITE, 0xe), (SI_CONTROL_TEXT_HILITE, 0xf),
    (SI_VIEW_BACK, 0x11), (SI_WINDOW_TEXT, 0x12), (SI_WINDOW_TITLE, 0x13), (SI_WINDOW_BACK, 0x14),
    (SI_WINDOW_VIEWPORT, 0x15), (SI_WINDOW_TEXT_HILITE, 0x16), (SI_WINDOW_FRAME_HILITE, 0x17), (SI_STATUS_BACK, 0x18),
    (SI_STATUS_LIGHT, 0x19), (SI_STATUS_TEXT, 0x1a), (SI_SLIDER_COLOR, 0x1b), (SI_STATUS_WARNING1, 0x1c),
    (SI_STATUS_WARNING2, 0x1d), (SI_PLAY_HILITE1, 0x1e), (SI_PLAY_HILITE2, 0x1f), (SI_CONTROLER_BACK, 0x20),
    (SI_CONTROLER_BACK2, 0x21), (SI_VIEW_AXIS, 0x22), (SI_VIEW_RULER_BACK, 0x23), (SI_VIEW_RULER, 0x24),
    (SI_VIEW_FIELD1, 0x25), (SI_VIEW_FIELD2, 0x26), (SI_VIEW_FIELD3, 0x27), (SI_VIEW_FIELD4, 0x28),
    (SI_COLOR_BOX_HILITE, 0x29), (SI_COLOR1, 0x2a), (SI_COLOR2, 0x2b), (SI_COLOR3, 0x2c), (SI_COLOR4, 0x2d),
    (SI_COLOR5, 0x2e), (SI_COLOR6, 0x2f), (SI_COLOR7, 0x30), (SI_COLOR8, 0x31), (SI_COLOR9, 0x32), (SI_COLOR10, 0x33),
    (SI_COLOR11, 0x34), (SI_COLOR12, 0x35), (SI_COLOR_FIRST, 0x2a), (SI_COLOR_LAST, 0x35), (SI_COLOR_NB, 0xc),
    (SI_FCURVE_COLOR1, 0x36), (SI_FCURVE_COLOR2, 0x37), (SI_FCURVE_COLOR3, 0x38), (SI_DOPE_DIALOG_BACK, 0x39),
    (SI_DOPE_DIALOG1, 0x3a), (SI_DOPE_DIALOG2, 0x3b), (SI_DOPE_DIALOG3, 0x3c), (SI_DOPE_DIALOG4, 0x3d),
    (SI_DOPE_DIALOG5, 0x3e), (SI_SYSTEM_WM, 0x3f), (SI_WHOLE_SYSTEM_WM, 0x3ff), (SI_UNSELECT_WM, 0x3f),
    (SI_OBJECT_WM, 0x1ff), (SI_SELECT1_WM, 0x40), (SI_SELECT2_WM, 0x80), (SI_SELECT3_WM, 0xc0),
    (SI_SELECT4_WM, 0x100), (SI_SELECT5_WM, 0x140), (SI_SELECT6_WM, 0x180), (SI_SELECT7_WM, 0x1c0),
    (SI_SELECT_STEP, 0x40), (SI_GRID_WM, 0x200), (SI_GRID_DARK, 0x200), (SI_X_AXIS_COLOR, 0x36),
    (SI_Y_AXIS_COLOR, 0x37), (SI_Z_AXIS_COLOR, 0x38), (SI_MENU1_WM, 0x400), (SI_MENU2_WM, 0x800),
    (SI_MENU3_WM, 0xc00),
];
const COMMON: &[(P<i16>, i16)] = &[
    (SI_SHADOW_MASK, 2), (SI_POPUP_NULL, 0), (SI_POPUP_BACKGRND, 1), (SI_POPUP_HTEXT, 1), (SI_POPUP_TEXT, 2),
    (SI_POPUP_CNTR, 2), (SI_POPUP_DEACTIVATE, 1), (SI_POPUP_HILITE, 2), (SI_POPUP_HICON, 1), (SI_POPUP_ICON, 2),
    (SI_POPUP_SHADOW, 3), (SI_POPUP_GHOST, 1),
];
const SMALL_SCREEN: &[(P<i16>, i16)] = &[
    (SI_ULEADING, 1), (SI_DLEADING, 2), (SI_ASCEND, 7), (SI_DESCEND, 2), (SI_BASELINE, 0xe), (SI_MARGIN, 0x11),
    (SI_TEXASCEND, 0xb), (SI_TEXDESCEND, 2), (SI_TEXWIDTH, 7), (SI_TEXHEIGHT, 0xd), (SI_TITLE_HEIGHT, 0x11),
    (SI_TITLE_SETUP, 0x78), (SI_TITLE_LAYOUT, 0xd5), (SI_DOV_RESETX, 0xe4), (SI_ORTHO_WIDGET_X, 0xc3),
    (SI_ORTHO_WIDGET_Y, 0xcf), (SI_ORTHO_WIDGET_Z, 0xdb), (SI_SPREAD_SHEET_QUERY, 0xdc),
    (SI_SPREAD_SHEET_SORT, 0x107), (SI_SPREAD_SHEET_FIELDS, 0x12a), (SI_SPREAD_SHEET_EXEC, 0x162),
    (SI_SPREAD_SHEET_SELECT, 0x193), (SI_SPREAD_SHEET_DE_SELECT, 0x1af), (SI_SPREAD_SHEET_QUERIES, 0x1d9),
    (SI_CPKEY_X, 0xee), (SI_SLOPE_X, 0x12a), (SI_INTERP_X, 0x166), (SI_SETKEY_X, 0x206), (SI_EDITKEY_X, 0x242),
    (SI_EDITTAG_X, 0x288), (SI_EDITFCV_X, 0x2ce), (SI_CONSTANT_X, 0x30a), (SI_LINEAR_X, 0x32d), (SI_SPLINE_X, 0x350),
    (SI_SLOPE0_X, 0x373), (SI_PLATEAU_X, 0x396), (SI_D_SLINK_X, 0x159), (SI_D_SUNLINK_X, 0x12c),
    (SI_D_SACTIV_X, 0xfa), (SI_D_SUNACTIV_X, 0xcd), (SI_D_SCOPY_X, 0xa0), (SI_D_SCUT_X, 0x7d), (SI_D_SPASTE_X, 0x5f),
    (SI_D_SCLEAR_X, 0x32), (SI_CTRL_RESET_HEIGHT, 0xe), (SI_CTRL_ZONE_MARGIN_X, 4), (SI_CTRL_ZONE_MARGIN_Y, 7),
    (SI_CTRL_WIDTH, 0x52), (SI_CTRL_HEIGHT, 0x38), (SI_CTRL_CAM_SMALL_RAY, 5), (SI_CTRL_CAM_BIG_RAY, 0x17),
    (SI_MEMO_HEIGHT, 0xa), (SI_MEMO_WIDTH, 0xc), (SI_MEMO_LEFT_OFFSET, 0x73), (SI_MEMO_SPACING, 8),
];
const SCREEN_2_6: &[(P<i16>, i16)] = &[
    (SI_ULEADING, 1), (SI_DLEADING, 2), (SI_ASCEND, 0xa), (SI_DESCEND, 3), (SI_BASELINE, 0x10), (SI_MARGIN, 0x14),
    (SI_TEXASCEND, 0xd), (SI_TEXDESCEND, 2), (SI_TEXWIDTH, 9), (SI_TEXHEIGHT, 0xf), (SI_TITLE_HEIGHT, 0x11),
    (SI_TITLE_SETUP, 0x78), (SI_TITLE_LAYOUT, 0xd9), (SI_DOV_RESETX, 0x104), (SI_ORTHO_WIDGET_X, 0xdc),
    (SI_ORTHO_WIDGET_Y, 0xeb), (SI_ORTHO_WIDGET_Z, 0xfa), (SI_SPREAD_SHEET_QUERY, 0xdc),
    (SI_SPREAD_SHEET_SORT, 0x107), (SI_SPREAD_SHEET_FIELDS, 0x12a), (SI_SPREAD_SHEET_EXEC, 0x162),
    (SI_SPREAD_SHEET_SELECT, 0x193), (SI_SPREAD_SHEET_DE_SELECT, 0x1af), (SI_SPREAD_SHEET_QUERIES, 0x1d9),
    (SI_CPKEY_X, 0xee), (SI_SLOPE_X, 0x12a), (SI_INTERP_X, 0x166), (SI_SETKEY_X, 0x206), (SI_EDITKEY_X, 0x242),
    (SI_EDITTAG_X, 0x288), (SI_EDITFCV_X, 0x2ce), (SI_CONSTANT_X, 0x30a), (SI_LINEAR_X, 0x32d), (SI_SPLINE_X, 0x350),
    (SI_SLOPE0_X, 0x373), (SI_PLATEAU_X, 0x396), (SI_D_SLINK_X, 0x182), (SI_D_SUNLINK_X, 0x158),
    (SI_D_SACTIV_X, 0x11e), (SI_D_SUNACTIV_X, 0xec), (SI_D_SCOPY_X, 0xba), (SI_D_SCUT_X, 0x90), (SI_D_SPASTE_X, 0x6e),
    (SI_D_SCLEAR_X, 0x3c), (SI_CTRL_RESET_HEIGHT, 0x10), (SI_CTRL_ZONE_MARGIN_X, 7), (SI_CTRL_ZONE_MARGIN_Y, 0xa),
    (SI_CTRL_WIDTH, 0x5c), (SI_CTRL_HEIGHT, 0x42), (SI_CTRL_CAM_SMALL_RAY, 5), (SI_CTRL_CAM_BIG_RAY, 0x1c),
    (SI_MEMO_HEIGHT, 0xa), (SI_MEMO_WIDTH, 0xf), (SI_MEMO_LEFT_OFFSET, 0x73), (SI_MEMO_SPACING, 0xa),
];
const OTHER_SCREENS: &[(P<i16>, i16)] = &[
    (SI_ULEADING, 1), (SI_DLEADING, 2), (SI_ASCEND, 0xa), (SI_DESCEND, 3), (SI_BASELINE, 0x10), (SI_MARGIN, 0x14),
    (SI_TEXASCEND, 0xd), (SI_TEXDESCEND, 2), (SI_TEXWIDTH, 9), (SI_TEXHEIGHT, 0xf), (SI_TITLE_HEIGHT, 0x11),
    (SI_TITLE_SETUP, 0x78), (SI_TITLE_LAYOUT, 0xd9), (SI_DOV_RESETX, 0xf5), (SI_ORTHO_WIDGET_X, 0xf5),
    (SI_ORTHO_WIDGET_Y, 0x104), (SI_ORTHO_WIDGET_Z, 0x113), (SI_SPREAD_SHEET_QUERY, 0xdc),
    (SI_SPREAD_SHEET_SORT, 0x107), (SI_SPREAD_SHEET_FIELDS, 0x12a), (SI_SPREAD_SHEET_EXEC, 0x162),
    (SI_SPREAD_SHEET_SELECT, 0x193), (SI_SPREAD_SHEET_DE_SELECT, 0x1af), (SI_SPREAD_SHEET_QUERIES, 0x1d9),
    (SI_CPKEY_X, 0x1a2), (SI_SLOPE_X, 0x1d1), (SI_INTERP_X, 0x201), (SI_SETKEY_X, 0xf0), (SI_EDITKEY_X, 0x128),
    (SI_EDITTAG_X, 0x165), (SI_EDITFCV_X, 0x287), (SI_CONSTANT_X, 0x2c4), (SI_LINEAR_X, 0x2e5), (SI_SPLINE_X, 0x306),
    (SI_SLOPE0_X, 0x327), (SI_PLATEAU_X, 0x348), (SI_D_SLINK_X, 0x20d), (SI_D_SUNLINK_X, 0x235),
    (SI_D_SACTIV_X, 0xf0), (SI_D_SUNACTIV_X, 0x122), (SI_D_SCOPY_X, 0x154), (SI_D_SCUT_X, 0x17c),
    (SI_D_SPASTE_X, 0x1a9), (SI_D_SCLEAR_X, 0x1db), (SI_CTRL_RESET_HEIGHT, 0x10), (SI_CTRL_ZONE_MARGIN_X, 7),
    (SI_CTRL_ZONE_MARGIN_Y, 0xa), (SI_CTRL_WIDTH, 0x5c), (SI_CTRL_HEIGHT, 0x42), (SI_CTRL_CAM_SMALL_RAY, 5),
    (SI_CTRL_CAM_BIG_RAY, 0x1c), (SI_MEMO_HEIGHT, 0xa), (SI_MEMO_WIDTH, 0xf), (SI_MEMO_LEFT_OFFSET, 0x73),
    (SI_MEMO_SPACING, 0xa),
];

/// The entries of a colour map: 497 of the normal planes and 4 of the overlay planes.
const MAP_NORMAL: u32 = 0x1f1;
const MAP_ALL: u32 = 0x1f5;

impl Ui {
    /// Define the line styles, fill patterns and cursors, then the colours
    /// [soft 0x0839be00 SI_graphicInit]: line styles 1 (0x3333) and 2 (0xaaaa); the seven
    /// 16 × 16 patterns; the cursors 1 to 11 and 13 (16 × 16) with their hot spots and,
    /// between `curstype(2)` and `curstype(0)`, the large cursor 12 (32 × 32, hot spot 5,
    /// 28); `SI_colorInit`.
    pub fn si_graphic_init(&mut self) {
        self.o.gl.deflinestyle(&mut self.m, 1, 0x3333);
        self.o.gl.deflinestyle(&mut self.m, 2, 0xaaaa);
        for (index, name) in [
            (1, "itemGhostPat"),
            (5, "subSelPolygonPat"),
            (6, "subSelPolygonTranspPat"),
            (7, "itemGhostTranspPat"),
            (2, "colorBoxPat"),
            (3, "dopGhostactPat"),
            (4, "dopGhostkeyPat"),
        ] {
            self.o.gl.defpattern(&mut self.m, index, 0x10, bitmap(name));
        }
        for (index, name, x, y) in [
            (1, "cursor1", 0, 0xf),
            (2, "cursor2", 0xf, 0),
            (3, "cursor3", 0, 0),
            (4, "cursor4", 0xf, 0xf),
            (5, "cursor5", 0xf, 0xf),
            (6, "cursor6", 0xf, 0xf),
            (7, "cursor7", 0xf, 0xf),
            (8, "cursor8", 0xf, 0xf),
            (9, "cursor9", 0xf, 0xf),
            (10, "cursor10", 0xf, 0xf),
            (0xb, "cursor11", 0xf, 0xf),
            (0xd, "cursor13", 0, fault!(0xf, 0xe)),
        ] {
            self.o.gl.defcursor(&mut self.m, index, bitmap(name));
            self.o.gl.curorigin(&mut self.m, index, x, y);
        }
        self.o.gl.curstype(&mut self.m, 2);
        self.o.gl.defcursor(&mut self.m, 0xc, bitmap("largecursor"));
        self.o.gl.curorigin(&mut self.m, 0xc, 5, 0x1c);
        self.o.gl.curstype(&mut self.m, 0);
        self.si_color_init();
    }

    fn store(&mut self, block: &[(P<i16>, i16)]) {
        for (variable, value) in block {
            self.m[*variable] = *value;
        }
    }

    /// Set every colour index, write mask and measure of the interface
    /// [soft 0x0839c068 SI_colorInit].
    ///
    /// - With 8 bit planes: a map of 16 colours (three of them depend on whether this is
    ///   the Dialog Editor), write masks of 4 planes. Otherwise: 63 colours in order,
    ///   10 planes for the system, masks for seven selection colours and three menu planes.
    /// - The pop-up menus' colours (the overlay planes' indices 0 to 3).
    /// - By screen size (`SI_screenRes`): the font's measures, where the title bars'
    ///   gadgets stand, the camera buttons' and the stored-camera boxes' sizes; one set for
    ///   size 1 (the small screen), another for every other size. (Sizes 2 and 6 run
    ///   through a block of their own first and then through the common one, which sets
    ///   the same variables again.)
    /// - What is the same everywhere: `SI_titleHeight` 17, `si_titleSetup` 120,
    ///   `SI_memoLeftOffset` 115, the spreadsheet's gadget places, and the overlay's plane
    ///   mask (0x40 with 8 overlay planes, else 0x20).
    pub fn si_color_init(&mut self) {
        if self.m[SI_BIT_PLANES] == 8 {
            self.store(PLANES8_FIRST);
            if self.o.server.SI_isDialogEditor(&mut self.m) != 0 {
                self.store(PLANES8_EDITOR);
            } else {
                self.store(PLANES8_PROGRAM);
            }
            self.store(PLANES8_REST);
            self.m[SI_FULL_SYSTEM_WM] = -1;
        } else {
            self.store(DEEP);
            self.m[SI_FULL_SYSTEM_WM] = 0x00ff_ffff;
        }
        self.store(COMMON);
        match self.m[SI_SCREEN_RES] {
            1 => self.store(SMALL_SCREEN),
            2 | 6 => {
                self.store(SCREEN_2_6);
                self.store(OTHER_SCREENS);
            }
            _ => self.store(OTHER_SCREENS),
        }
        self.m[SI_OVERPLANE] = if self.m[SI_OLBIT_PLANES] == 8 { 0x40 } else { 0x20 };
    }

    /// For the colour-map routines called before the program has its window: a window of
    /// IRIS GL to have a colour map at all. Returns its identifier.
    fn map_window(&mut self, name: &[u8]) -> i32 {
        self.o.gl.noport(&mut self.m);
        if self.m[SI_FOREGROUND] != 0 {
            self.o.gl.foreground(&mut self.m);
        }
        self.o.gl.winopen(&mut self.m, name)
    }

    /// Read the colour map in use into `PREV_MAP` (0: what the program found) or
    /// `SOFT_MAP` (anything else: its own) [soft 0x0839cdb8 SI_colorMapStore]: the 497
    /// entries of the normal planes, then in overlay draw mode the 4 of the overlay
    /// planes, and the map is handed to the graphic properties. Without a main window yet,
    /// a window is opened for the purpose (named "Dialog Editor" there, and brought to the
    /// foreground) and the mouse buttons are queued. Returns 1.
    pub fn si_color_map_store(&mut self, own: i16) -> i32 {
        let mut window = 0;
        if self.m[SI_SYSID] == -1 {
            self.o.gl.noport(&mut self.m);
            if self.m[SI_FOREGROUND] != 0 {
                self.o.gl.foreground(&mut self.m);
            }
            window = if self.o.server.SI_isDialogEditor(&mut self.m) != 0 {
                self.o.gl.foreground(&mut self.m);
                self.o.gl.winopen(&mut self.m, b"Dialog Editor")
            } else {
                self.o.gl.winopen(&mut self.m, b"soft")
            };
            self.o.gl.qdevice(&mut self.m, 0x67);
            self.o.gl.qdevice(&mut self.m, 0x65);
            self.o.gl.qdevice(&mut self.m, 0x66);
        }
        let map = if own == 0 { PREV_MAP.addr() } else { SOFT_MAP.addr() };
        for i in 0..MAP_NORMAL {
            self.o.gl.getmcolor(&mut self.m, i as u16, map + 6 * i, map + 6 * i + 2, map + 6 * i + 4);
        }
        self.o.gl.drawmode(&mut self.m, 0x20);
        for i in MAP_NORMAL..MAP_ALL {
            self.o.gl.getmcolor(&mut self.m, (i - MAP_NORMAL) as u16, map + 6 * i, map + 6 * i + 2, map + 6 * i + 4);
        }
        self.o.gl.drawmode(&mut self.m, 0x10);
        self.o.views.SI_gpropsSetPaletteEntries(&mut self.m, map, 1);
        if self.m[SI_SYSID] == -1 {
            self.o.gl.winclose(&mut self.m, window);
        }
        1
    }

    /// One 16-bit value into an image file being written: the image library's `putw` as
    /// the compiler laid it out in the caller: the count of free places in the image's
    /// buffer (+0x7a) goes down by one; if it went below zero `iflsbuf` takes the value,
    /// else the value is stored where the buffer pointer (+0x7c) points and the pointer
    /// moves on.
    fn image_putw(&mut self, image: u32, value: i32) {
        let room = self.m.u16(image.wrapping_add(0x7a)).wrapping_sub(1);
        self.m.set_u16(image.wrapping_add(0x7a), room);
        if (room as i16) < 0 {
            self.o.images.iflsbuf(&mut self.m, image, value);
        } else {
            let at = self.m.u32(image.wrapping_add(0x7c));
            self.m.set_u16(at, value as u16);
            let at = self.m.u32(image.wrapping_add(0x7c));
            self.m.set_u32(image.wrapping_add(0x7c), at.wrapping_add(2));
        }
    }

    /// Write the colour map in use to an image file [soft 0x0839cfcc SI_colorMapSave]: an
    /// image of the SGI image library, 4 values wide and 501 rows high, two bytes a
    /// value (`iopen(path, "w", 2, 2, 4, 501)` under `umask(0)`), its word at +0x68 set to
    /// 3; each row is the index and the entry's red, green and blue: 497 rows of the
    /// normal planes, then in overlay draw mode the 4 of the overlay planes, numbered on
    /// from 497. Returns 1; 0 with a message on the standard error stream when the file
    /// cannot be opened. Without a main window a window is opened for the purpose and the
    /// mouse buttons queued, as in `SI_colorMapStore`.
    ///
    /// When the file cannot be opened the original closes that window whether it opened
    /// one or not (with a main window it hands `winclose` a word it never set; here 0),
    /// and leaves the file-creation mask at 0.
    pub fn si_color_map_save(&mut self, path: u32) -> i32 {
        let mut window = 0;
        if self.m[SI_SYSID] == -1 {
            window = self.map_window(b"soft");
            self.o.gl.qdevice(&mut self.m, 0x67);
            self.o.gl.qdevice(&mut self.m, 0x65);
            self.o.gl.qdevice(&mut self.m, 0x66);
        }
        let mask = self.o.c.umask(&mut self.m, 0);
        let image = self.o.images.iopen(&mut self.m, path, b"w", Some([2, 2, 4, MAP_ALL]));
        if image == 0 {
            let reason = self.o.c.strerror(&mut self.m, self.files.errno);
            let stream = self.m[STDERR];
            self.o.c.fprintf(&mut self.m, stream, b"colorMapSave: cannot open output file %s...(%s)\n", &[LogArg::S(path), LogArg::S(reason)]);
            self.o.gl.winclose(&mut self.m, window);
            return 0;
        }
        self.m.set_i32(image.wrapping_add(0x68), 3);
        let sp = self.m.sp();
        let rgb = self.m.local(8);
        for i in 0..MAP_NORMAL {
            self.o.gl.getmcolor(&mut self.m, i as u16, rgb, rgb + 2, rgb + 4);
            self.image_putw(image, i as i32);
            for k in 0..3 {
                let v = self.m.i16(rgb + 2 * k) as i32;
                self.image_putw(image, v);
            }
        }
        let mode = self.o.gl.getdrawmode(&mut self.m);
        self.o.gl.drawmode(&mut self.m, 0x20);
        for i in MAP_NORMAL..MAP_ALL {
            self.o.gl.getmcolor(&mut self.m, (i - MAP_NORMAL) as u16, rgb, rgb + 2, rgb + 4);
            self.image_putw(image, i as i32);
            for k in 0..3 {
                let v = self.m.i16(rgb + 2 * k) as i32;
                self.image_putw(image, v);
            }
        }
        self.m.set_sp(sp);
        self.o.gl.drawmode(&mut self.m, mode);
        if self.m[SI_SYSID] == -1 {
            self.o.gl.winclose(&mut self.m, window);
        }
        self.o.images.iclose(&mut self.m, image);
        self.o.c.umask(&mut self.m, mask);
        1
    }

    /// Load a colour map from an image file [soft 0x0839d484 SI_colorMapLoad]: the image
    /// must be 4 values wide (its 16-bit word at +6; its height is the word at +8). Every
    /// row but the last four is an index and its red, green, blue for `mapcolor`; the
    /// last four rows are the overlay planes' entries 0 to 3 (their own first value is
    /// not used), mapped in overlay draw mode. Returns 1; 0 with a message on the
    /// standard error stream when the file cannot be opened or is not 4 wide.
    pub fn si_color_map_load(&mut self, path: u32) -> i32 {
        let image = self.o.images.iopen(&mut self.m, path, b"r", None);
        if image == 0 {
            let reason = self.o.c.strerror(&mut self.m, self.files.errno);
            let stream = self.m[STDERR];
            self.o.c.fprintf(&mut self.m, stream, b"colorMapLoad: cannot open input file %s...(%s)\n", &[LogArg::S(path), LogArg::S(reason)]);
            return 0;
        }
        if self.m.u16(image.wrapping_add(6)) != 4 {
            let width = self.m.u16(image.wrapping_add(6)) as i32;
            let stream = self.m[STDERR];
            self.o.c.fprintf(&mut self.m, stream, b"colorMapLoad: incorrect xsize for map file %d\n", &[LogArg::I(width)]);
            self.o.images.iclose(&mut self.m, image);
            return 0;
        }
        let mut window = 0;
        if self.m[SI_SYSID] == -1 {
            window = self.map_window(b"soft");
        }
        let sp = self.m.sp();
        let row = self.m.local(8);
        let height = |ui: &Ui| ui.m.u16(image.wrapping_add(8)) as i32;
        let mut y = 0i32;
        while y <= height(self) - 5 {
            self.o.images.getrow(&mut self.m, image, row, y, 0);
            let (index, r, g, b) = (self.m.u16(row), self.m.i16(row + 2), self.m.i16(row + 4), self.m.i16(row + 6));
            self.o.gl.mapcolor(&mut self.m, index, r, g, b);
            y += 1;
        }
        let mode = self.o.gl.getdrawmode(&mut self.m);
        self.o.gl.drawmode(&mut self.m, 0x20);
        let mut y = height(self) - 4;
        let mut index = 0u16;
        while y < height(self) {
            self.o.images.getrow(&mut self.m, image, row, y, 0);
            let (r, g, b) = (self.m.i16(row + 2), self.m.i16(row + 4), self.m.i16(row + 6));
            self.o.gl.mapcolor(&mut self.m, index, r, g, b);
            y += 1;
            index = index.wrapping_add(1);
        }
        self.m.set_sp(sp);
        self.o.gl.drawmode(&mut self.m, mode);
        if self.m[SI_SYSID] == -1 {
            self.o.gl.winclose(&mut self.m, window);
        }
        self.o.images.iclose(&mut self.m, image);
        1
    }

    /// Load `PREV_MAP` (0) or `SOFT_MAP` into the colour map
    /// [soft 0x0839d2dc SI_colorMapRestore]: the reverse of `SI_colorMapStore` with `mapcolor`. Returns 1.
    pub fn si_color_map_restore(&mut self, own: i16) -> i32 {
        let mut window = 0;
        if self.m[SI_SYSID] == -1 {
            window = self.map_window(b"soft");
        }
        let map = if own == 0 { PREV_MAP.addr() } else { SOFT_MAP.addr() };
        let entry = |ui: &Ui, i: u32| (ui.m.i16(map + 6 * i), ui.m.i16(map + 6 * i + 2), ui.m.i16(map + 6 * i + 4));
        for i in 0..MAP_NORMAL {
            let (r, g, b) = entry(self, i);
            self.o.gl.mapcolor(&mut self.m, i as u16, r, g, b);
        }
        self.o.gl.drawmode(&mut self.m, 0x20);
        for i in MAP_NORMAL..MAP_ALL {
            let (r, g, b) = entry(self, i);
            self.o.gl.mapcolor(&mut self.m, (i - MAP_NORMAL) as u16, r, g, fault!(b, g));
        }
        self.o.gl.drawmode(&mut self.m, 0x10);
        self.o.views.SI_gpropsSetPaletteEntries(&mut self.m, map, 1);
        if self.m[SI_SYSID] == -1 {
            self.o.gl.winclose(&mut self.m, window);
        }
        1
    }
}
