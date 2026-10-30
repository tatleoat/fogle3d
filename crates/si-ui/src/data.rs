// SPDX-License-Identifier: MIT
//! Initialised data of the original's `.data` section that the ported routines read through
//! pointers: the static table of a view window's title-bar gadgets and the strings its
//! entries and the gadget code point to. [`init`] writes them at their addresses;
//! `re/tools/si-ui/check_init.py` compares every byte the crate writes into `.data` with
//! the binary.
//!
//! Nothing here reproduces a function; it is the constant data of `window.c`.

use crate::mem::Mem;
use crate::window::HANDLERS;

/// Runs of strings: the address of the first, then each string; every one is followed by
/// its terminating zero and the next begins right after it.
pub const STRINGS: &[(u32, &[&[u8]])] = &[
    // the labels of `gadgetTitles`, a label and its second label (a glyph, or empty) per row
    (
        0x086c_adc0,
        &[
            b"CPTAG", b"", b"SLOPE", b"", b"INTERP", b"", b"SNAP", b"", b"SWAP", b"", b"SETKEY", b"", b"EDITKEY", b"", b"EDITTAG", b"",
            b"EDITFCV", b"", b"CON", b"", b"LIN", b"", b"SPL", b"", b"SL0", b"", b"PLA", b"", b"CLEAN_ALL", b"", b"XXXXXXXXXXXX", b"\x06",
            b"OPEN/CLOSE", b"", b"DEFCOL", b"", b"CLEAN", b"", b"ACTIV", b"", b"DEACT", b"", b"COPY", b"", b"CUT", b"", b"PASTE", b"",
            b"CLEAR", b"", b"SETUP", b"", b"LINK", b"", b"UNLINK", b"", b"MERGE", b"", b"ALIGN", b"", b"ACT_KEY", b"", b"X", b"", b"Y", b"",
            b"Z", b"", b"QUERY", b"", b"COLUMNS", b"", b"SORT", b"", b"UPDATE", b"", b"QUERIES", b"\x06", b"SETUP", b"", b"SEL", b"",
            b"DESEL", b"", b"1:1", b"", b"SETUP", b"", b"\x07", b"\x05", b"CLIP", b"", b"IN_GRAFPORT", b"", b"IN_CAM_HANDLER", b"",
            b"IN_SELECT", b"", b"IN_ZOOM", b"", b"IN_MEMO_CAM", b"", b"IN_DISP_MODE", b"", b"IN_TITLE", b"", b"FCV_LST", b"",
        ],
    ),
    // `schemGetModeMenuString`: the names of the schematic view's modes, and the 15 blanks
    // of the buffer in which a combination is spelled out
    (
        0x086c_b564,
        &[
            b"Plain Mode", b"Light Mode", b"Model Mode", b"Motion Mode", b"Matter Mode", b"All Modes", b"               ",
            // and what follows them: the formats of the layout file's reader (`windowReadFile`
            // to `windowRead`), from the mode of its `fopen` on
            b"r", b"%d", b"%d", b"%*s %s", b"%*s %hd", b"%*s %hd", b"%*s %hd", b"%*s %hd", b"%*s %f", b"%*s %f", b"%*s %f", b"%*s %f",
            b"%*s %f %f %f", b"%*s %f %f %f", b"%*s %f", b"%*s %f", b"%*s %f", b"%*s %*s %hd", b"%*s %hd", b"%*s %hd", b"%*s %hd",
            b"%*s %hd", b"%*s %hd", b"%*s %hd", b"%*s %f %f %f", b"%*s %f %f %f", b"%*s %d", b"%*s %hd", b"%*s %*s %d", b"%*s %hd",
            b"%*s %d", b"%*s %d", b"%hd", b"%hd", b"%hd", b"%hd", b"%hd %hd %hd %hd",
        ],
    ),
    // `SI_windowGadjet`: the windows' letters, "LOCK" once per place it is used, the glyphs
    // after the title (6) and of the layout box (7, 5), the dope sheet's two labels, the
    // names of the display modes
    (
        0x086c_b820,
        &[
            b"C", b"D", b"A", b"B", b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"\x06", b"\x06",
            b"\x06", b"\x06", b"\x07", b"\x07", b"\x05", b"\x07", b"\x05", b"\x07", b"\x05", b"\x07", b"\x05", b"ACT_KEY", b"ACT_DEF", b"LOCK",
            b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"LOCK", b"WIRE", b"SHADE", b"ROTO-W", b"ROTO-SH", b"DEPTH",
            b"GHOST", b"MATTE", b"", b"\x06", b"\x06", b"\x06", b"\x06",
        ],
    ),
    // `deWindowDraw`
    (0x086c_b8ee, &[b"editor"]),
];

/// "ACT_KEY" and "ACT_DEF" of `SI_windowGadjet`, the label of the dope sheet's part 52.
pub const ACT_KEY: u32 = 0x086c_b874;
pub const ACT_DEF: u32 = 0x086c_b87c;
/// The names of the schematic view's modes and the buffer for a combination.
pub const SCHEM_PLAIN: u32 = 0x086c_b564;
pub const SCHEM_LIGHT: u32 = 0x086c_b56f;
pub const SCHEM_MODEL: u32 = 0x086c_b57a;
pub const SCHEM_MOTION: u32 = 0x086c_b585;
pub const SCHEM_MATTER: u32 = 0x086c_b591;
pub const SCHEM_ALL: u32 = 0x086c_b59d;
pub const SCHEM_BUFFER: u32 = 0x086c_b5a7;

/// One row of `gadgetTitles` (0x086caf60, 0x1c bytes a row in the original: part, label,
/// second label, offset, view type, room, handler): the part number, the offset from the
/// window's left edge (0: `initGadget` lays the gadget out), the view type whose title bar
/// shows it (−1: every view), the room to the right the gadget needs to be drawn, and the
/// handler by name. The labels are the strings of the first run of [`STRINGS`], two a row.
pub type TitleRow = (i32, i32, i32, i32, &'static str);

pub const GADGET_TITLES_AT: u32 = 0x086c_af60;

pub const GADGET_TITLES: [TitleRow; 54] = [
    (10, 0, 2004, 15, "inCutPasteKeyCB"),
    (15, 0, 2004, 15, "inSlopeManagementCB"),
    (16, 0, 2004, 15, "inInterpManagementCB"),
    (36, 0, 2004, 15, "inFcvSnapCB"),
    (35, 0, 2004, 15, "inFcvSwapCB"),
    (14, 0, 2004, 15, "inSetKeyCB"),
    (12, 0, 2004, 15, "inEditKeyCB"),
    (13, 0, 2004, 15, "inEditTagKeyCB"),
    (11, 0, 2004, 15, "inEditFcurveCB"),
    (17, 0, 2004, 15, "inConstantInterpCB"),
    (18, 0, 2004, 15, "inLinearInterpCB"),
    (19, 0, 2004, 15, "inSplineInterpCB"),
    (33, 0, 2004, 15, "inSlope0InterpCB"),
    (34, 0, 2004, 15, "inPlateauInterpCB"),
    (21, 0, 2007, 15, "inSchemCleanAllCB"),
    (23, 0, 2007, 20, "inSchemViewCB"),
    (20, 0, 2007, 15, "inSchemOpenCloseCB"),
    (40, 0, 2007, 15, "inSchemDefcolCB"),
    (22, 0, 2007, 15, "inSchemCleanCB"),
    (28, 0, 2008, 15, "inDsZnActivCB"),
    (29, 0, 2008, 15, "inDsZnUnactivCB"),
    (25, 0, 2008, 15, "inDsCopyCB"),
    (24, 0, 2008, 15, "inDsCutCB"),
    (26, 0, 2008, 15, "inDsPasteCB"),
    (27, 0, 2008, 15, "inDsClearCB"),
    (49, 0, 2008, 15, "inDsSetupCB"),
    (30, 0, 2008, 15, "inDsLinkCB"),
    (31, 0, 2008, 15, "inDsUnlinkCB"),
    (53, 0, 2008, 15, "inDsMergeCB"),
    (54, 0, 2008, 15, "inDsAlignCB"),
    (52, 0, 2008, 15, "inDsActModeCB"),
    (37, 0, 2009, 15, "inOrthoXyzCB"),
    (38, 0, 2009, 15, "inOrthoXyzCB"),
    (39, 0, 2009, 15, "inOrthoXyzCB"),
    (41, 0, 2010, 15, "inSpreadQueryCB"),
    (43, 0, 2010, 15, "inSpreadFieldsCB"),
    (42, 0, 2010, 15, "inSpreadSortCB"),
    (44, 0, 2010, 15, "inSpreadExecQueryCB"),
    (47, 0, 2010, 15, "inSpreadQueriesCB"),
    (48, 0, 2010, 15, "inSpreadSetupCB"),
    (45, 0, 2010, 15, "inSpreadSelectCB"),
    (46, 0, 2010, 15, "inSpreadDeselectCB"),
    (8, 0, -1, 0, "inDocropCB"),
    (6, 0, -1, 0, "inSetupCB"),
    (7, 0, -1, 0, "inLayoutCB"),
    (50, 0, -1, 0, "inClipCB"),
    (0, 0, -1, 0, "inGrafportCB"),
    (1, 0, -1, 0, "inCamHandlerCB"),
    (2, 0, -1, 0, "inSelectCB"),
    (3, 0, -1, 0, "inZoomCB"),
    (4, 0, -1, 0, "inMemoCamCB"),
    (5, 0, -1, 0, "inDispModeCB"),
    (9, 30, -1, 0, "inTitleCB"),
    (32, 0, -1, 0, "inFcvLstCB"),
];

/// The fill patterns and cursor shapes `SI_graphicInit` hands to IRIS GL: name in the
/// binary, address, rows of 16 bits (16 rows; the large cursor 32 rows of two words).
pub const BITMAPS: &[(&str, u32, &[u16])] = &[
    ("colorBoxPat", 0x086bf8c0, &[0xfafa, 0xf5f5, 0xbebe, 0x7d7d, 0xafaf, 0x5f5f, 0xebeb, 0xd7d7, 0xfafa, 0xf5f5, 0xbebe, 0x7d7d, 0xafaf, 0x5f5f, 0xebeb, 0xd7d7]),
    ("cursor1", 0x086bf8e0, &[0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0xffff]),
    ("cursor10", 0x086bf900, &[0x07e0, 0x1818, 0x201c, 0x403e, 0x403e, 0xe07f, 0xf87f, 0xfeff, 0xff7f, 0xfe1f, 0xfe03, 0x7c02, 0x7c02, 0x3804, 0x1818, 0x07e0]),
    ("cursor11", 0x086bf920, &[0x07e0, 0x1818, 0x3804, 0x7c02, 0x7c02, 0xfe03, 0xfe1f, 0xff7f, 0xfeff, 0xf87f, 0xe07f, 0x403e, 0x403e, 0x201c, 0x1818, 0x07e0]),
    ("cursor13", 0x086bf940, &[0x0020, 0x0060, 0x00c8, 0x00c8, 0x0030, 0x0030, 0x0030, 0x0f34, 0x1f3e, 0x1863, 0x30c6, 0x318c, 0x6018, 0x6030, 0xffe0, 0xffc0]),
    ("cursor2", 0x086bf960, &[0xffff, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001]),
    ("cursor3", 0x086bf980, &[0xffff, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000]),
    ("cursor4", 0x086bf9a0, &[0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0xffff]),
    ("cursor5", 0x086bf9c0, &[0x7ffe, 0x43e2, 0x21c4, 0x308c, 0x2814, 0x2424, 0x2244, 0x2244, 0x23c4, 0x23c4, 0x27e4, 0x2ff4, 0x33cc, 0x2004, 0x4002, 0x7ffe]),
    ("cursor6", 0x086bf9e0, &[0x07e0, 0x1f18, 0x3f04, 0x7f02, 0x7f02, 0xff01, 0xff01, 0xff01, 0x80ff, 0x80ff, 0x80ff, 0x40fe, 0x40fe, 0x20fc, 0x18f8, 0x07e0]),
    ("cursor7", 0x086bfa00, &[0x07e0, 0x1ff8, 0x3fe4, 0x7fc2, 0x7fc2, 0x9f81, 0x8781, 0x8101, 0x8081, 0x81e1, 0x81f9, 0x43fe, 0x43fe, 0x27fc, 0x1ff8, 0x07e0]),
    ("cursor8", 0x086bfa20, &[0x07e0, 0x1ff8, 0x27fc, 0x43fe, 0x43fe, 0x81f9, 0x81e1, 0x8081, 0x8081, 0x8781, 0x9f81, 0x7fc2, 0x7fc2, 0x3fe4, 0x1ff8, 0x07e0]),
    ("cursor9", 0x086bfa40, &[0x07e0, 0x18f8, 0x20fc, 0x40fe, 0x40fe, 0x80ff, 0x80ff, 0x80ff, 0xff01, 0xff01, 0xff01, 0x7f02, 0x7f02, 0x3f04, 0x1f18, 0x07e0]),
    ("dopGhostactPat", 0x086bfa60, &[0x6666, 0x9999, 0x9999, 0x6666, 0x6666, 0x9999, 0x9999, 0x6666, 0x6666, 0x9999, 0x9999, 0x6666, 0x6666, 0x9999, 0x9999, 0x6666]),
    ("dopGhostkeyPat", 0x086bfa80, &[0x5555, 0xaaaa, 0x5555, 0xaaaa, 0x5555, 0xaaaa, 0x5555, 0xaaaa, 0x5555, 0xaaaa, 0x5555, 0xaaaa, 0x5555, 0xaaaa, 0x5555, 0xaaaa]),
    ("itemGhostPat", 0x086bfaa0, &[0x5555, 0xaaaa, 0x5555, 0xaaaa, 0x5555, 0xaaaa, 0x5555, 0xaaaa, 0x5555, 0xaaaa, 0x5555, 0xaaaa, 0x5555, 0xaaaa, 0x5555, 0xaaaa]),
    ("subSelPolygonPat", 0x086bfac0, &[0x1111, 0x2222, 0x4444, 0x8888, 0x1111, 0x2222, 0x4444, 0x8888, 0x1111, 0x2222, 0x4444, 0x8888, 0x1111, 0x2222, 0x4444, 0x8888]),
    ("subSelPolygonTranspPat", 0x086bfae0, &[0x1111, 0x0000, 0x4444, 0x0000, 0x1111, 0x0000, 0x4444, 0x0000, 0x1111, 0x0000, 0x4444, 0x0000, 0x1111, 0x0000, 0x4444, 0x0000]),
    ("itemGhostTranspPat", 0x086bfb00, &[0x1111, 0x0000, 0x4444, 0x0000, 0x1111, 0x0000, 0x4444, 0x0000, 0x1111, 0x0000, 0x4444, 0x0000, 0x1111, 0x0000, 0x4444, 0x0000]),
    ("largecursor", 0x086bfb20, &[0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0001, 0x8000, 0x0003, 0xe000, 0x0203, 0xf000, 0x0307, 0xf000, 0x0387, 0xe000, 0x03cf, 0xe000, 0x03ff, 0xc000, 0x03ff, 0xc000, 0x03ff, 0x8000, 0x03ff, 0xe000, 0x03ff, 0xfc00, 0x03ff, 0xfc00, 0x03ff, 0xf800, 0x03ff, 0xf000, 0x03ff, 0xe000, 0x03ff, 0xc000, 0x03ff, 0x8000, 0x03ff, 0x0000, 0x03fe, 0x0000, 0x03fc, 0x0000, 0x03f8, 0x0000, 0x03f0, 0x0000, 0x03e0, 0x0000, 0x03c0, 0x0000, 0x0380, 0x0000, 0x0300, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000]),
];

/// The address of a bitmap of [`BITMAPS`] by its name.
pub fn bitmap(name: &str) -> u32 {
    BITMAPS.iter().find(|(n, _, _)| *n == name).expect("a bitmap of SI_graphicInit").1
}

/// Write the strings and the table. The table's 55th row is its end: part −1, no labels,
/// offset −1, the rest zero.
pub fn init(m: &mut Mem) {
    let mut labels = Vec::new();
    for (k, (start, run)) in STRINGS.iter().enumerate() {
        let mut at = *start;
        for s in run.iter() {
            m.put_cstr(at, s);
            if k == 0 {
                labels.push(at);
            }
            at += s.len() as u32 + 1;
        }
    }
    for (i, (part, offset, camera, room, handler)) in GADGET_TITLES.iter().enumerate() {
        let at = GADGET_TITLES_AT + 0x1c * i as u32;
        let code = HANDLERS.iter().find(|(_, name)| name == handler).expect("a handler of window.c").0;
        m.set_i32(at, *part);
        m.set_u32(at + 4, labels[2 * i]);
        m.set_u32(at + 8, labels[2 * i + 1]);
        m.set_i32(at + 0xc, *offset);
        m.set_i32(at + 0x10, *camera);
        m.set_i32(at + 0x14, *room);
        m.set_u32(at + 0x18, code);
    }
    let end = GADGET_TITLES_AT + 0x1c * 54;
    m.set_i32(end, -1);
    m.set_i32(end + 0xc, -1);
    for (_, at, rows) in BITMAPS {
        for (i, row) in rows.iter().enumerate() {
            m.set_u16(at + 2 * i as u32, *row);
        }
    }
}
