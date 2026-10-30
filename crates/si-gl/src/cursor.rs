// SPDX-License-Identifier: MIT
//! Cursors (`cursor.c`, 0x0860c200, 13 functions).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! The layer keeps a table of X pixmap cursors: entry 0 is an invisible cursor (index −1,
//! what `cursoff` shows), entry 1 the default arrow (index 0, red on white). Each window
//! remembers a cursor index and whether the cursor is shown.
//!
//! **`defcursor` cannot define a usable cursor.** It appends a record and builds the X
//! cursor, but never writes the record's index field (it stays 0 from the clearing), so
//! `setcursor` and `curorigin` with that index find nothing, and the search for index 0
//! finds the default arrow first. Reproduced as it is: in this build the cursor is always
//! the default arrow or hidden.

use crate::context::{st, IrisGl};
use crate::data;
use crate::state::*;
use crate::sys::CArg;

impl IrisGl {
    /// The cursor with the given index, searching from table entry 1 (the invisible
    /// cursor is never found) [soft 0x0860c200 crs_getFromID].
    fn crs_get_from_id(&mut self, id: i32) -> Option<usize> {
        self.ctx_get();
        let cursors = &self.ctx.as_deref().unwrap().crs.cursors;
        (1..cursors.len()).find(|&i| cursors[i].id == id)
    }

    /// Free a cursor's two pixmaps and its X cursor [soft 0x0860c24c crs_destroyCursor].
    fn crs_destroy_cursor(&mut self, index: usize) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let c = &mut ctx.crs.cursors[index];
        sys.x.XFreePixmap(ctx.display, c.source);
        sys.x.XFreePixmap(ctx.display, c.mask);
        sys.x.XFreeCursor(ctx.display, c.xcursor);
        c.xcursor = 0;
    }

    /// (Re)create a cursor's X cursor from its pixmaps, colours and hot spot, freeing the
    /// old one [soft 0x0860c2a0 crs_redefCursor].
    fn crs_redef_cursor(&mut self, index: usize) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let c = &mut ctx.crs.cursors[index];
        if c.xcursor != 0 {
            sys.x.XFreeCursor(ctx.display, c.xcursor);
        }
        c.xcursor = sys.x.XCreatePixmapCursor(ctx.display, c.source, c.mask, &c.fg, &c.bg, c.hot_x as u32, c.hot_y as u32);
    }

    /// Allocate the cursor state and the two standing cursors [soft 0x0860c2f8 CRS_init].
    ///
    /// Entry 0 (index −1): a 1 × 1 pixmap of one clear bit, used as picture and mask,
    /// both colours zero. Entry 1 (index 0): the built-in 16 × 16 arrow and its mask,
    /// foreground red (0xffff, 0, 0), background white, hot spot (0, 0). The pixmaps have
    /// depth 1. Then the table of bit-reversed bytes.
    pub(crate) fn crs_init(&mut self) {
        let (ctx, sys) = st!(self);
        let white = sys.x.screen_word(ctx.display, ctx.screen, 0x34);
        let black = sys.x.screen_word(ctx.display, ctx.screen, 0x38);
        ctx.crs = CrsContext { size_type: 0, cursors: vec![Cursor::default(); 2], bit_reverse: [0; 256] };
        let p = sys.x.XCreatePixmapFromBitmapData(ctx.display, ctx.root, &[0], 1, 1, black, black, 1);
        ctx.crs.cursors[0] = Cursor { id: -1, source: p, mask: p, width: 1, height: 1, ..Default::default() };
        self.crs_redef_cursor(0);
        let (ctx, sys) = st!(self);
        let source = sys.x.XCreatePixmapFromBitmapData(ctx.display, ctx.root, data::DEFAULT_CURSOR_IMAGE.get(), 0x10, 0x10, white, black, 1);
        let mask = sys.x.XCreatePixmapFromBitmapData(ctx.display, ctx.root, data::DEFAULT_CURSOR_MASK.get(), 0x10, 0x10, white, black, 1);
        let c = &mut ctx.crs.cursors[1];
        c.id = 0;
        c.source = source;
        c.mask = mask;
        c.bg[4..10].copy_from_slice(&[0xff; 6]);
        c.fg[4..6].copy_from_slice(&[0xff; 2]);
        c.width = 0x10;
        c.height = 0x10;
        self.crs_redef_cursor(1);
        let (ctx, _) = st!(self);
        for i in 0..256usize {
            ctx.crs.bit_reverse[i] = (i as u8).reverse_bits();
        }
    }

    /// Allocate the window's cursor record: cursor 0, shown; a top-level window gets the
    /// default arrow defined [soft 0x0860c4b0 CRS_initWindow].
    pub(crate) fn crs_init_window(&mut self, index: usize) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let w = &mut ctx.wnd.windows[index];
        w.crs = CrsWindow { cursor: 0, visible: 1 };
        if w.parent == 0 {
            sys.x.XDefineCursor(ctx.display, w.xwindow, ctx.crs.cursors[1].xcursor);
        }
    }

    /// Free it [soft 0x0860c50c CRS_exitWindow].
    pub(crate) fn crs_exit_window(&mut self, _index: usize) {}

    /// IRIS GL `cursoff`: show the invisible cursor in the current window
    /// [soft 0x0860c52c cursoff].
    pub fn cursoff(&mut self) {
        self.ctx_get();
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        let w = &mut ctx.wnd.windows[c];
        if w.crs.visible != 0 {
            sys.x.XDefineCursor(ctx.display, w.xwindow, ctx.crs.cursors[0].xcursor);
            w.crs.visible = 0;
        }
    }

    /// IRIS GL `curson`: show the window's cursor again [soft 0x0860c580 curson].
    pub fn curson(&mut self) {
        self.ctx_get();
        self.ctx_get();
        let Some(c) = self.cur() else { return };
        let crs = self.ctx.as_deref().unwrap().wnd.windows[c].crs;
        if crs.visible == 0 {
            // the original uses the search result unchecked (the window's cursor index
            // always names an existing cursor, see `setcursor`)
            let cursor = self.crs_get_from_id(crs.cursor).expect("curson: the original dereferences a null cursor");
            let (ctx, sys) = st!(self);
            let w = &mut ctx.wnd.windows[c];
            sys.x.XDefineCursor(ctx.display, w.xwindow, ctx.crs.cursors[cursor].xcursor);
            w.crs.visible = 1;
        }
    }

    /// IRIS GL `curstype`: the format `defcursor` expects, 0..=4 accepted
    /// [soft 0x0860c5d8 curstype]. Only 0 (16 × 16) and 2 (32 × 32) are implemented there.
    pub fn curstype(&mut self, kind: u32) {
        self.ctx_get();
        if kind < 5 {
            self.ctx.as_deref_mut().unwrap().crs.size_type = kind;
        }
    }

    /// IRIS GL `defcursor(n, curs)` [soft 0x0860c5f8 defcursor]. Index 0 is ignored.
    ///
    /// An existing cursor is destroyed first; else a cleared record is appended (and its
    /// index field left 0, see the module text). The glyph is converted to an X bitmap:
    /// rows in reverse order (IRIS GL's first row is the bottom one), each 16-bit word's
    /// two bytes swapped and each byte bit-reversed; for 32 × 32 a row is two such words.
    /// Picture and mask are both made from that one bitmap. Foreground red component
    /// 0xffff, then the X cursor is created. Another cursor size type prints a warning
    /// (whose "%d" has no argument) and leaves the record without pixmaps.
    pub fn defcursor(&mut self, n: i16, curs: &[u16]) {
        self.ctx_get();
        self.ctx_get();
        let cur = self.cur();
        let mut redefine_current = false;
        if n == 0 {
            return;
        }
        let at = match self.crs_get_from_id(n as i32) {
            Some(i) => {
                self.crs_destroy_cursor(i);
                if let Some(c) = cur {
                    redefine_current = n as i32 == self.ctx.as_deref().unwrap().wnd.windows[c].crs.cursor;
                }
                i
            }
            None => {
                let cursors = &mut self.ctx.as_deref_mut().unwrap().crs.cursors;
                cursors.push(Cursor::default());
                cursors.len() - 1
            }
        };
        let (ctx, sys) = st!(self);
        let rev = &ctx.crs.bit_reverse;
        // the original reads the glyph unchecked
        let word = |i: usize| curs.get(i).copied().unwrap_or(0);
        let (bitmap, size): (Vec<u8>, u32) = match ctx.crs.size_type {
            0 => {
                let mut b = Vec::with_capacity(32);
                for row in (0..16).rev() {
                    let w = word(row);
                    b.push(rev[(w >> 8) as usize]);
                    b.push(rev[(w & 0xff) as usize]);
                }
                (b, 0x10)
            }
            2 => {
                let mut b = Vec::with_capacity(128);
                for row in (0..32).rev() {
                    let (w0, w1) = (word(row * 2), word(row * 2 + 1));
                    b.push(rev[(w0 >> 8) as usize]);
                    b.push(rev[(w0 & 0xff) as usize]);
                    b.push(rev[(w1 >> 8) as usize]);
                    b.push(rev[(w1 & 0xff) as usize]);
                }
                (b, 0x20)
            }
            _ => {
                self.err_warning(b"Cursor size type %d not supported.", &[] as &[CArg]);
                return;
            }
        };
        let white = sys.x.screen_word(ctx.display, ctx.screen, 0x34);
        let black = sys.x.screen_word(ctx.display, ctx.screen, 0x38);
        let source = sys.x.XCreatePixmapFromBitmapData(ctx.display, ctx.root, &bitmap, size, size, white, black, 1);
        let mask = sys.x.XCreatePixmapFromBitmapData(ctx.display, ctx.root, &bitmap, size, size, white, black, 1);
        let c = &mut ctx.crs.cursors[at];
        c.source = source;
        c.mask = mask;
        c.width = size as i32;
        c.height = size as i32;
        c.fg[4..6].copy_from_slice(&[0xff; 2]);
        self.crs_redef_cursor(at);
        if redefine_current {
            let c = cur.unwrap();
            let id = self.ctx.as_deref().unwrap().wnd.windows[c].crs.cursor;
            let cursor = self.crs_get_from_id(id).expect("defcursor: the original dereferences a null cursor");
            let (ctx, sys) = st!(self);
            sys.x.XDefineCursor(ctx.display, ctx.wnd.windows[c].xwindow, ctx.crs.cursors[cursor].xcursor);
        }
    }

    /// IRIS GL `curorigin(n, xorigin, yorigin)`: a cursor's hot spot, measured from the
    /// glyph's lower left corner [soft 0x0860c998 curorigin]. Clamped to the glyph; the X
    /// cursor is recreated, and defined again in the current window if it is the one
    /// shown there.
    pub fn curorigin(&mut self, n: i16, xorigin: i16, yorigin: i16) {
        self.ctx_get();
        self.ctx_get();
        let cur = self.cur();
        if n < 0 {
            return;
        }
        let Some(at) = self.crs_get_from_id(n as i32) else { return };
        let c = &mut self.ctx.as_deref_mut().unwrap().crs.cursors[at];
        let mut hx = xorigin.max(0);
        if c.width - 1 < hx as i32 {
            hx = (c.width as i16).wrapping_sub(1);
        }
        let mut hy = yorigin.max(0);
        if c.height - 1 < hy as i32 {
            hy = (c.height as i16).wrapping_sub(1);
        }
        c.hot_x = hx as i32;
        c.hot_y = (c.height as i16).wrapping_sub(hy.wrapping_add(1)) as i32;
        self.crs_redef_cursor(at);
        if let Some(w) = cur {
            let (ctx, sys) = st!(self);
            let win = &ctx.wnd.windows[w];
            if win.crs.visible != 0 && n as i32 == win.crs.cursor {
                sys.x.XDefineCursor(ctx.display, win.xwindow, ctx.crs.cursors[at].xcursor);
            }
        }
    }

    /// IRIS GL `setcursor`: choose the current window's cursor [soft 0x0860ca84 setcursor].
    /// Ignored for a negative or undefined index, or the index already set.
    pub fn setcursor(&mut self, n: i16) {
        self.ctx_get();
        self.ctx_get();
        let Some(c) = self.cur() else { return };
        if n < 0 || self.ctx.as_deref().unwrap().wnd.windows[c].crs.cursor == n as i32 {
            return;
        }
        if let Some(at) = self.crs_get_from_id(n as i32) {
            let (ctx, sys) = st!(self);
            let w = &mut ctx.wnd.windows[c];
            w.crs.cursor = n as i32;
            if w.crs.visible != 0 {
                sys.x.XDefineCursor(ctx.display, w.xwindow, ctx.crs.cursors[at].xcursor);
            }
        }
    }

    /// IRIS GL `getcursor(&index, &color, &wtm, &b)`: the current window's cursor index
    /// (low 16 bits) and whether it is shown; the colour and write mask outputs are not
    /// written [soft 0x0860caf8 getcursor]. `None` without a window.
    pub fn getcursor(&mut self) -> Option<(i16, i32)> {
        self.ctx_get();
        let c = self.cur()?;
        let crs = self.ctx.as_deref().unwrap().wnd.windows[c].crs;
        Some((crs.cursor as i16, crs.visible))
    }
}
