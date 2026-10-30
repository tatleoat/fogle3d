// SPDX-License-Identifier: MIT
//! The layer's state: one root record with a sub-record per source file, and one record per
//! window with a sub-record per source file again.
//!
//! In the original these are heap blocks reached from the global `g_context`
//! (0x08700f80). Offsets, sizes and meanings are read from the code that fills and reads
//! them; the names of the structures (`CTX_Context`, `WND_Window`, `EVT_Context`,
//! `GFX_Context`, `FNT_Font`, `CRS_Cursor`) are the binary's, from the mangled parameter
//! types; field names are ours. A field the layer never touches is not represented.

use crate::sys::{Ptr, Xid};
use std::collections::VecDeque;

/// `CTX_Context`, 0x38 bytes, allocated by `ctx_initContext`.
#[derive(Debug, Default)]
pub struct Context {
    /// +0x00 the `Display *`.
    pub display: Ptr,
    /// +0x04 the root window of the default screen.
    pub root: Xid,
    /// +0x08 the default screen's number.
    pub screen: i32,
    /// +0x0c driver work-arounds chosen from the OpenGL vendor string (`FIX_*`).
    pub gl_fixes: u32,
    /// +0x10 width of the screen in pixels.
    pub screen_width: i32,
    /// +0x14 height of the screen in pixels.
    pub screen_height: i32,
    /// +0x18 pointer to three atoms (0xc bytes).
    pub atoms: Atoms,
    /// +0x1c pointer to the hidden start-up window (0x10 bytes); null while it is created.
    pub init_window: Option<InitWindow>,
    /// +0x20 pointer to the cursor state (0x10c bytes).
    pub crs: CrsContext,
    // +0x24 is never written.
    /// +0x28 pointer to the event state (0xc021c bytes).
    pub evt: EvtContext,
    /// +0x2c pointer to the font state (0xc bytes).
    pub fnt: FntContext,
    /// +0x30 pointer to the graphics state (0x3030 bytes).
    pub gfx: GfxContext,
    /// +0x34 pointer to the window state (0x4c bytes).
    pub wnd: WndContext,
}

/// Mesa ("Brian Paul" in the vendor string) sets 1 | 4, NVIDIA sets 8, an ATI FireGL sets 2.
/// Bit 0: `gsync` does nothing.
pub const FIX_NO_GSYNC: u32 = 1;
/// Bit 1: a filled screen box starts one pixel lower.
pub const FIX_BOX_Y: u32 = 2;
/// Bit 2: before a clear, the draw buffer is switched away and back.
pub const FIX_RESTORE_BUFFERS: u32 = 4;
/// Bit 3: a filled screen box starts one pixel lower and one pixel to the left.
pub const FIX_BOX_XY: u32 = 8;

/// The block at `CTX_Context + 0x18`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Atoms {
    /// +0 `_MOTIF_WM_HINTS`
    pub motif_wm_hints: Xid,
    /// +4 `WM_DELETE_WINDOW`
    pub wm_delete_window: Xid,
    /// +8 `WM_STATE`
    pub wm_state: Xid,
}

/// The block at `CTX_Context + 0x1c`: a 128 × 128 window that is never mapped. Its GLX
/// context is made current whenever no drawable is current, so OpenGL calls made with no
/// IRIS GL window open have somewhere to go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InitWindow {
    /// +0x0
    pub window: Xid,
    /// +0x4 the `XVisualInfo *`
    pub visual_info: Ptr,
    /// +0x8
    pub colormap: Xid,
    /// +0xc
    pub glx_context: Ptr,
}

// ------------------------------------------------------------------ windows

/// The window state, 0x4c bytes (`WND_init`).
#[derive(Debug, Default)]
pub struct WndContext {
    /// +0x00 count, +0x04 pointer: the window table, 0x44 bytes per window, kept dense
    /// (closing a window moves the later ones down).
    pub windows: Vec<Window>,
    /// +0x08 pointer to the current window (an address inside the table): here its index.
    pub current: Option<usize>,
    /// +0x0c the X event mask every window is given (changed by `qdevice`/`unqdevice`).
    pub event_mask: u32,
    /// +0x10 which constraints are pending for the next `winopen`/`winconstraints`
    /// (`CONSTRAINT_*`); cleared when they are applied.
    pub constraints: u32,
    /// +0x14, +0x18 `minsize`
    pub min_width: i32,
    pub min_height: i32,
    /// +0x1c, +0x20 `maxsize`
    pub max_width: i32,
    pub max_height: i32,
    /// +0x24 left edge, +0x28 upper edge in IRIS GL coordinates (y up) of `prefposition`.
    pub pos_x: i32,
    pub pos_y: i32,
    /// +0x2c, +0x30 `prefsize`/`prefposition`
    pub pref_width: i32,
    pub pref_height: i32,
    /// +0x34, +0x38 `keepaspect`
    pub aspect_x: i32,
    pub aspect_y: i32,
    /// +0x3c, +0x40 `stepunit`
    pub step_x: i32,
    pub step_y: i32,
    /// +0x44, +0x48 `fudge` (stored, never read)
    pub fudge_x: i32,
    pub fudge_y: i32,
}

pub const CONSTRAINT_MINSIZE: u32 = 0x01;
pub const CONSTRAINT_MAXSIZE: u32 = 0x02;
pub const CONSTRAINT_POSITION: u32 = 0x04;
pub const CONSTRAINT_SIZE: u32 = 0x08;
pub const CONSTRAINT_ASPECT: u32 = 0x10;
pub const CONSTRAINT_STEP: u32 = 0x20;
pub const CONSTRAINT_FUDGE: u32 = 0x40;
pub const CONSTRAINT_NOBORDER: u32 = 0x80;
pub const CONSTRAINT_NOPORT: u32 = 0x100;

/// `WND_Window`, 0x44 bytes.
#[derive(Debug, Default)]
pub struct Window {
    /// +0x00 the IRIS GL window identifier (`gid`), 1 or more.
    pub id: i32,
    /// +0x04 the X window.
    pub xwindow: Xid,
    /// +0x08 its `XVisualInfo *`.
    pub visual_info: Ptr,
    /// +0x0c its GLX context.
    pub glx_context: Ptr,
    /// +0x10 its colormap.
    pub colormap: Xid,
    /// +0x14 identifier of the parent window; 0 for a top-level window.
    pub parent: i32,
    /// +0x18, +0x1c, +0x20, +0x24: x, y, width, height from the last `XGetGeometry`
    /// (X coordinates: y down, relative to the X parent).
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// +0x28 pointer to the saved screen contents under a sub-window (0x1c bytes), or null.
    pub backing: Option<BackingStore>,
    /// +0x2c pointer to the cursor record (8 bytes).
    pub crs: CrsWindow,
    // +0x30 pointer to four bytes that event.c allocates and never uses.
    /// +0x34 pointer to the character position (0x18 bytes).
    pub fnt: FntWindow,
    /// +0x38 pointer to the drawing state (0xb4 bytes).
    pub gfx: GfxWindow,
    /// +0x3c pointer to the rubber-band, sub-region and dialog records (0x4c bytes).
    pub msc: MscWindow,
    /// +0x40 pointer to the picking state (0x38 bytes).
    pub pck: PckWindow,
}

/// The saved screen contents under a sub-window, 0x1c bytes (`WND_backingStoreGet`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BackingStore {
    /// +0x00, +0x04 the rectangle's corner in X root coordinates.
    pub x: i32,
    pub y: i32,
    /// +0x08, +0x0c its size.
    pub width: i32,
    pub height: i32,
    /// +0x10 bytes per pixel of the image (bits per pixel / 8).
    pub bytes_per_pixel: i32,
    /// +0x14 bytes per line of the image.
    pub bytes_per_line: i32,
    /// +0x18 the `XImage *`.
    pub image: Ptr,
}

// ------------------------------------------------------------------ cursors

/// The cursor state, 0x10c bytes (`CRS_init`).
#[derive(Debug)]
pub struct CrsContext {
    /// +0x00 the cursor size type of `curstype` (0: 16 × 16, 2: 32 × 32).
    pub size_type: u32,
    /// +0x04 count, +0x08 pointer: the cursor table, 0x38 bytes per cursor. Entry 0 is the
    /// invisible cursor (identifier −1), entry 1 the default arrow (identifier 0).
    pub cursors: Vec<Cursor>,
    /// +0x0c a table of every byte with its bits in reverse order.
    pub bit_reverse: [u8; 256],
}

impl Default for CrsContext {
    fn default() -> Self {
        CrsContext { size_type: 0, cursors: Vec::new(), bit_reverse: [0; 256] }
    }
}

/// `CRS_Cursor`, 0x38 bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    /// +0x00 the IRIS GL cursor index.
    pub id: i32,
    /// +0x04 the picture, +0x08 the mask (pixmaps of depth 1).
    pub source: Xid,
    pub mask: Xid,
    /// +0x0c the X cursor, 0 after `crs_destroyCursor`.
    pub xcursor: Xid,
    /// +0x10 foreground and +0x1c background colour: `XColor` records of 12 bytes (pixel,
    /// red, green, blue, flags, pad).
    pub fg: [u8; 12],
    pub bg: [u8; 12],
    /// +0x28, +0x2c size in pixels.
    pub width: i32,
    pub height: i32,
    /// +0x30, +0x34 the hot spot, X convention (from the upper left corner).
    pub hot_x: i32,
    pub hot_y: i32,
}

/// The per-window cursor record, 8 bytes (`CRS_initWindow`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CrsWindow {
    /// +0 index of the cursor chosen with `setcursor`.
    pub cursor: i32,
    /// +4 non-zero while the cursor is shown (`curson`/`cursoff`).
    pub visible: i32,
}

// ------------------------------------------------------------------ events

/// One device of the device table, 0xc bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Device {
    /// +0 non-zero while the device is queued (`qdevice`).
    pub queued: i32,
    /// +4, +6 for a button: the two valuators tied to it (`tie`), 0 for none. For the
    /// devices 0x100..=0x200 `EVT_init` writes the long 0x8000 over both.
    pub tie: [u16; 2],
    /// +8 written by `EVT_init` for the devices 0x100..=0x200 (1), never read.
    pub word8: u16,
}

/// One queue entry, 0xc bytes (`EVT_addEvent`): +0 is the link to the next entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Event {
    /// +4 the device number.
    pub device: u16,
    /// +6 the value.
    pub value: i16,
    /// +8 a time stamp in hundredths of a second (never read by the layer).
    pub time: u32,
}

/// Number of devices in the table: every 16-bit device number has an entry.
pub const DEVICE_COUNT: usize = 0x10000;

/// The event state, 0xc021c bytes (`EVT_init`).
#[derive(Debug)]
pub struct EvtContext {
    /// +0x00000 head and +0x00004 tail of the singly linked queue.
    pub queue: VecDeque<Event>,
    /// +0x00008 the device table, 0x10000 entries of 0xc bytes.
    pub devices: Vec<Device>,
    /// +0xc0008 non-zero while the layer holds a pointer grab (`setvaluator`).
    pub grabbed: i32,
    /// +0xc000c, +0xc0010 the pointer position last reported as MOUSEX and MOUSEY events
    /// (−1 before the first).
    pub last_x: i32,
    pub last_y: i32,
    /// +0xc0014 keycode to device number, 256 shorts (`evt_initKeymap`).
    pub keymap: [u16; 256],
    /// +0xc0214 pointer, +0xc0218 length: device number to keycode (the highest keycode
    /// that maps to the device).
    pub keycode_of: Vec<i32>,
}

impl Default for EvtContext {
    fn default() -> Self {
        EvtContext {
            queue: VecDeque::new(),
            devices: Vec::new(),
            grabbed: 0,
            last_x: 0,
            last_y: 0,
            keymap: [0; 256],
            keycode_of: Vec::new(),
        }
    }
}

// ------------------------------------------------------------------ fonts

/// The font state, 0xc bytes (`FNT_init`).
#[derive(Debug, Default)]
pub struct FntContext {
    /// +0 count, +4 pointer: the font table, 0x14 bytes per font; entry 0 is the built-in
    /// font, identifier 0.
    pub fonts: Vec<Font>,
    /// +8 pointer to the current font (an address inside the table): here its index.
    pub current: usize,
}

/// `FNT_Font`, 0x14 bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Font {
    /// +0x00 the IRIS GL font index.
    pub id: i32,
    /// +0x04 the height given to `defrasterfont`.
    pub height: i32,
    /// +0x08 the descender: how far the lowest character reaches below the base line.
    pub descender: i32,
    /// +0x0c count, +0x10 pointer: the characters, 0x18 bytes each.
    pub chars: Vec<FontChar>,
}

/// One character of a font, 0x18 bytes: the arguments of the `glBitmap` that draws it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontChar {
    /// +0x00, +0x04 size of the bitmap in pixels.
    pub width: i32,
    pub height: i32,
    /// +0x08, +0x0c the bitmap's origin (the negated offsets of the `Fontchar`).
    pub xorig: i32,
    pub yorig: i32,
    /// +0x10 how far the character position moves.
    pub advance: i32,
    /// +0x14 pointer to the bitmap, rows of `(width + 7) / 8` bytes, or null for a
    /// character of width 0.
    pub bitmap: Option<Vec<u8>>,
}

/// The per-window font record, 0x18 bytes: the character position of `cmov`, three doubles.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FntWindow {
    pub pos: [f64; 3],
}

// ------------------------------------------------------------------ graphics

/// The eight longs that describe a frame buffer configuration (the start of `GFX_Context`
/// and, twice, part of the per-window record).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GfxConfig {
    /// +0x00 1 for RGB mode, 0 for colour-map mode.
    pub rgb: i32,
    /// +0x04, +0x08, +0x0c, +0x10 bits of red, green, blue and alpha.
    pub red: i32,
    pub green: i32,
    pub blue: i32,
    pub alpha: i32,
    /// +0x14 bits of depth buffer.
    pub depth: i32,
    /// +0x18 bits of stencil.
    pub stencil: i32,
    /// +0x1c one byte: 1 for double buffering.
    pub double: u8,
}

/// One line style of `deflinestyle`, 8 bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineStyle {
    /// +0
    pub id: i32,
    /// +4
    pub pattern: u16,
}

/// One fill pattern of `defpattern`, 0x84 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pattern {
    /// +0
    pub id: i32,
    /// +4 the 32 × 32 stipple handed to `glPolygonStipple`.
    pub stipple: [u8; 128],
}

/// `GFX_Context`, 0x3030 bytes (`GFX_init`).
#[derive(Debug)]
pub struct GfxContext {
    /// +0x0000 what the visual of the start-up window offers (`rgb` is set to 1).
    pub config: GfxConfig,
    /// +0x0020 the colour map: 4096 entries of three bytes (red, green, blue).
    pub cmap: Vec<u8>,
    /// +0x3020 count, +0x3024 pointer.
    pub line_styles: Vec<LineStyle>,
    /// +0x3028 count, +0x302c pointer.
    pub patterns: Vec<Pattern>,
}

impl Default for GfxContext {
    fn default() -> Self {
        GfxContext {
            config: GfxConfig::default(),
            cmap: vec![0; 0x3000],
            line_styles: Vec::new(),
            patterns: Vec::new(),
        }
    }
}

/// The per-window drawing state, 0xb4 bytes (`GFX_initWindow`). The first 0x20 bytes are
/// never used.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GfxWindow {
    /// +0x20 the configuration in force (set by `gconfig`).
    pub config: GfxConfig,
    /// +0x40 the configuration asked for since the last `gconfig` (`RGBmode`, `cmode`,
    /// `doublebuffer`, `RGBsize`, `zbsize`, `stensize`).
    pub wanted: GfxConfig,
    /// +0x60 the OpenGL draw and read buffer: GL_FRONT, GL_BACK, GL_FRONT_AND_BACK or 0.
    pub buffer: u32,
    /// +0x64 the current colour index.
    pub color: u16,
    /// +0x66 the write mask, 0xfff at the start.
    pub writemask: u16,
    /// +0x68, +0x6c, +0x70, +0x74 the current colour: red, green, blue, alpha, 0..255.
    pub rgba: [i32; 4],
    /// +0x78 index of the current line style, 0 for solid.
    pub line_style: i32,
    /// +0x7c the line style repeat factor.
    pub ls_repeat: i32,
    /// +0x80 index of the current pattern, 0 for solid.
    pub pattern: i32,
    /// +0x84 the draw mode reported by `getdrawmode`: what `drawmode` was given if that was
    /// NORMALDRAW (0x10) or 0x100, else NORMALDRAW.
    pub drawmode_effective: i32,
    /// +0x88 the draw mode last given to `drawmode`.
    pub drawmode: i32,
    /// +0x8c the matrix mode of `mmode` (0 MSINGLE, 1 MPROJECTION, 2 MVIEWING, 3 MTEXTURE).
    pub mmode: i32,
    /// +0x90 the graphics position of `move`/`draw`, three doubles.
    pub pos: [f64; 3],
    /// +0xa8 non-zero while a draw mode other than the two above forces the front buffer.
    pub overlay: i32,
    /// +0xac, +0xb0 the pixel zoom of `rectzoom`.
    pub zoom: [f32; 2],
}

// ------------------------------------------------------------------ misc

/// The per-window record of `misc.c`, 0x4c bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MscWindow {
    /// +0x00 the picture of a hidden dialog (`XImage *`), +0x04 its width, +0x08 its height.
    pub hidden_image: Ptr,
    pub hidden_width: i32,
    pub hidden_height: i32,
    /// +0x0c, +0x10 the X corner and +0x14, +0x18 the size of the rubber band's range.
    pub band_x: i32,
    pub band_y: i32,
    pub band_width: i32,
    pub band_height: i32,
    /// +0x1c, +0x20 the saved horizontal strips under the first and the moving corner,
    /// +0x24, +0x28 the saved vertical strips (`XImage *`, three pixels thick).
    pub band_row_start: Ptr,
    pub band_row_end: Ptr,
    pub band_col_start: Ptr,
    pub band_col_end: Ptr,
    /// +0x2c the `GC` the rubber band is drawn with.
    pub band_gc: Ptr,
    /// +0x30, +0x34 corner in window coordinates and +0x38, +0x3c size of the sub-region.
    pub sub_x: i32,
    pub sub_y: i32,
    pub sub_width: i32,
    pub sub_height: i32,
    /// +0x40 the saved picture of the sub-region (`XImage *`).
    pub sub_image: Ptr,
    /// +0x44 the four shorts last given to `GLOB_restoreSubregion` (−1 before the first).
    pub sub_last: [i16; 4],
}

// ------------------------------------------------------------------ picking

/// The per-window picking state, 0x38 bytes (`PCK_initWindow`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PckWindow {
    /// +0x00 pointer to the selection buffer (null outside picking), +0x04 its size in
    /// words: five per name the caller's buffer can hold. OpenGL owns the content; the
    /// layer only reads it back.
    pub buffer: bool,
    pub buffer_size: i32,
    /// +0x08, +0x0c the pick region's size in pixels (`picksize`), 10 × 10 at the start.
    pub size_x: i32,
    pub size_y: i32,
    /// +0x10 non-zero between `pick` and `endpick`.
    pub picking: i32,
    /// +0x14 non-zero between `gselect` and `endselect`.
    pub selecting: i32,
    /// +0x18, +0x1c the pick centre (pointer position), +0x20, +0x24 the region's size, and
    /// +0x28 the viewport at the time of `pick`: the arguments of `gluPickMatrix`.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub viewport: [i32; 4],
}
