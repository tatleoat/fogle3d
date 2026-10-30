// SPDX-License-Identifier: MIT
//! Rubber bands, saved sub-regions and dialog windows drawn with plain X
//! (`misc.c`, 0x08613b90, 23 functions).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! These are not IRIS GL calls but extensions of the layer (`GLOB_…`) for things the
//! program did on SGI machines with overlay planes: a rubber band rectangle, a region whose
//! contents are saved and put back, and moving, hiding and showing a dialog (a sub-window)
//! without redrawing what is under it. All of it works on X images taken from the root
//! window. Coordinates given by the program are IRIS GL screen coordinates (y up).

use crate::context::{st, IrisGl};
use crate::state::*;

impl IrisGl {
    /// The current window's index and the screen height, after the two context requests
    /// every routine here starts with.
    fn msc_cur(&mut self) -> Option<(usize, i32)> {
        self.ctx_get();
        self.ctx_get();
        let ctx = self.ctx.as_deref().unwrap();
        Some((ctx.wnd.current?, ctx.screen_height))
    }

    /// Start a rubber band [soft 0x08613b90 msc_initRubberBand]. Arguments: the fixed
    /// corner (x, y), then the left edge, upper edge, width and height of the range the
    /// band may cover.
    ///
    /// The range and two strips of screen, three pixels thick, are saved: a horizontal
    /// one across the range at the corner's row and a vertical one down the range at the
    /// corner's column. A GC with white foreground and solid lines is created for the
    /// current window.
    fn msc_init_rubber_band(&mut self, px: i32, py: i32, left: i32, top: i32, width: i32, height: i32) {
        let Some((c, sh)) = self.msc_cur() else { return };
        let (ctx, sys) = st!(self);
        let xwindow = ctx.wnd.windows[c].xwindow;
        let m = &mut ctx.wnd.windows[c].msc;
        let y = (sh - (top + 1)) - height;
        m.band_x = left;
        m.band_width = width;
        m.band_y = y;
        m.band_height = height;
        m.band_row_start = sys.x.XGetImage(ctx.display, ctx.root, left, (sh - (py + 1)) - 1, width as u32, 3, 0xffff_ffff, 2);
        m.band_col_start = sys.x.XGetImage(ctx.display, ctx.root, px - 1, y, 3, height as u32, 0xffff_ffff, 2);
        sys.x.XSync(ctx.display, 0);
        m.band_col_end = 0;
        m.band_row_end = 0;
        let white = sys.x.screen_word(ctx.display, ctx.screen, 0x34);
        m.band_gc = sys.x.XCreateGC(ctx.display, xwindow, 0x24, white, 0);
    }

    /// End a rubber band: the four saved strips and the GC are released
    /// [soft 0x08613ca0 msc_exitRubberBand].
    fn msc_exit_rubber_band(&mut self) {
        let Some((c, _)) = self.msc_cur() else { return };
        let (ctx, sys) = st!(self);
        let m = ctx.wnd.windows[c].msc;
        for image in [m.band_row_start, m.band_col_start, m.band_row_end, m.band_col_end] {
            if image != 0 {
                sys.x.XDestroyImage(image);
            }
        }
        sys.x.XFreeGC(ctx.display, m.band_gc);
    }

    /// Draw the band from (x1, y1) to (x2, y2) [soft 0x08613d28 msc_drawRubberBand].
    ///
    /// The strips at the moving corner saved before are released and saved anew at
    /// (x2, y2); then the rectangle's outline is drawn into the current window with
    /// `XDrawRectangle`, and `XSync`.
    fn msc_draw_rubber_band(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        let Some((c, sh)) = self.msc_cur() else { return };
        let (ctx, sys) = st!(self);
        let xwindow = ctx.wnd.windows[c].xwindow;
        let m = &mut ctx.wnd.windows[c].msc;
        let ya = sh - (y1 + 1);
        let yb = sh - (y2 + 1);
        if m.band_row_end != 0 {
            sys.x.XDestroyImage(m.band_row_end);
        }
        if m.band_col_end != 0 {
            sys.x.XDestroyImage(m.band_col_end);
        }
        m.band_row_end = sys.x.XGetImage(ctx.display, ctx.root, m.band_x, yb - 1, m.band_width as u32, 3, 0xffff_ffff, 2);
        m.band_col_end = sys.x.XGetImage(ctx.display, ctx.root, x2 - 1, m.band_y, 3, m.band_height as u32, 0xffff_ffff, 2);
        let w = (x2 - x1).wrapping_abs();
        let h = (yb - ya).wrapping_abs();
        let y = if ya < yb { ya } else { yb };
        let x = if x1 < x2 { x1 } else { x2 };
        let (_, t) = sys.x.XTranslateCoordinates(ctx.display, ctx.root, xwindow, x, y);
        sys.x.XDrawRectangle(ctx.display, xwindow, m.band_gc, t[0] as i32, t[1] as i32, w as u32, h as u32);
        sys.x.XSync(ctx.display, 0);
    }

    /// Erase the band drawn from (x1, y1) to (x2, y2) by putting the four saved strips
    /// back [soft 0x08613e78 msc_clearRubberBand]. Nothing if the two corners are equal.
    fn msc_clear_rubber_band(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        let Some((c, sh)) = self.msc_cur() else { return };
        if x1 == x2 && y1 == y2 {
            return;
        }
        let (ctx, sys) = st!(self);
        let xwindow = ctx.wnd.windows[c].xwindow;
        let m = ctx.wnd.windows[c].msc;
        let strips = [
            (m.band_row_start, m.band_x, (sh - (y1 + 1)) - 1, m.band_width, 3),
            (m.band_col_start, x1 - 1, m.band_y, 3, m.band_height),
            (m.band_row_end, m.band_x, (sh - (y2 + 1)) - 1, m.band_width, 3),
            (m.band_col_end, x2 - 1, m.band_y, 3, m.band_height),
        ];
        for (image, x, y, w, h) in strips {
            if image != 0 {
                let (_, t) = sys.x.XTranslateCoordinates(ctx.display, ctx.root, xwindow, x, y);
                let gc = sys.x.screen_word(ctx.display, ctx.screen, 0x2c);
                sys.x.XPutImage(ctx.display, xwindow, gc, image, 0, 0, t[0] as i32, t[1] as i32, w as u32, h as u32);
            }
        }
    }

    /// Save a region of the screen (lower left corner, width, height) for repeated
    /// restoring, and prepare OpenGL for drawing over it in the front buffer
    /// [soft 0x086140a8 msc_initSubregion].
    ///
    /// `glXWaitGL` (even without a window); attributes saved (`glPushAttrib(0x614e)`),
    /// front draw buffer, lighting, depth test, line and point smoothing off; the region
    /// is fetched from the root window and its position in the current window noted.
    fn msc_init_subregion(&mut self, x: i32, y: i32, width: i32, height: i32) {
        let cur = self.msc_cur();
        self.sys.glx.glXWaitGL();
        let Some((c, sh)) = cur else { return };
        let (ctx, sys) = st!(self);
        let xwindow = ctx.wnd.windows[c].xwindow;
        let m = &mut ctx.wnd.windows[c].msc;
        sys.gl.glPushAttrib(0x614e);
        sys.gl.glDrawBuffer(0x404);
        sys.gl.glDisable(0xb50);
        sys.gl.glDisable(0xb71);
        sys.gl.glDisable(0xb20);
        sys.gl.glDisable(0xb10);
        let xy = (sh - (y + 1)) - height;
        m.sub_image = sys.x.XGetImage(ctx.display, ctx.root, x, xy, width as u32, height as u32, 0xffff_ffff, 2);
        let (_, t) = sys.x.XTranslateCoordinates(ctx.display, ctx.root, xwindow, x, xy);
        m.sub_x = t[0] as i32;
        m.sub_y = t[1] as i32;
        m.sub_width = width;
        m.sub_height = height;
        m.sub_last = [-1; 4];
        sys.glx.glXWaitX();
    }

    /// Put the saved region back, release it and restore the OpenGL attributes
    /// [soft 0x086141cc msc_exitSubregion].
    fn msc_exit_subregion(&mut self) {
        let Some((c, _)) = self.msc_cur() else { return };
        let (ctx, sys) = st!(self);
        let xwindow = ctx.wnd.windows[c].xwindow;
        let m = ctx.wnd.windows[c].msc;
        sys.glx.glXWaitGL();
        let gc = sys.x.screen_word(ctx.display, ctx.screen, 0x2c);
        sys.x.XPutImage(ctx.display, xwindow, gc, m.sub_image, 0, 0, m.sub_x, m.sub_y, m.sub_width as u32, m.sub_height as u32);
        sys.x.XDestroyImage(m.sub_image);
        sys.x.XSync(ctx.display, 0);
        sys.glx.glXWaitX();
        sys.gl.glPopAttrib();
    }

    /// Put the saved region back if the four shorts differ from those of the last call
    /// [soft 0x08614274 msc_restoreSubregion]. Returns 1 if the caller should draw: on the
    /// first call (nothing is put back then) and whenever the values changed; 0 when they
    /// are the same or there is no window.
    fn msc_restore_subregion(&mut self, rect: &[i16; 4]) -> i32 {
        let Some((c, _)) = self.msc_cur() else { return 0 };
        let (ctx, sys) = st!(self);
        let xwindow = ctx.wnd.windows[c].xwindow;
        let m = &mut ctx.wnd.windows[c].msc;
        let mut result = 0;
        sys.glx.glXWaitGL();
        if m.sub_last[0] == -1 {
            result = 1;
        } else if (0..4).any(|i| (rect[i] as i32 - m.sub_last[i] as i32).abs() >= 1) {
            let gc = sys.x.screen_word(ctx.display, ctx.screen, 0x2c);
            sys.x.XPutImage(ctx.display, xwindow, gc, m.sub_image, 0, 0, m.sub_x, m.sub_y, m.sub_width as u32, m.sub_height as u32);
            result = 1;
        }
        m.sub_last = *rect;
        sys.glx.glXWaitX();
        result
    }

    /// The pointer's position on the root window (X coordinates) and the mouse buttons:
    /// bit 0 button 1, bit 1 button 2, bit 2 button 3 [soft 0x08614384 msc_getPointer].
    fn msc_get_pointer(&mut self) -> (i32, i32, i32) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let (_, q) = sys.x.XQueryPointer(ctx.display, ctx.root);
        let mask = q[6];
        let mut buttons = (mask >> 8) & 1;
        if mask & 0x200 != 0 {
            buttons |= 2;
        }
        if mask & 0x400 != 0 {
            buttons |= 4;
        }
        (q[2] as i32, q[3] as i32, buttons as i32)
    }

    /// Drag a dialog (the sub-window `id`) with the pointer for as long as a mouse button
    /// is down [soft 0x086143e8 msc_moveDialog]. Returns 0 if the window has no saved
    /// screen contents, else 1 and the total displacement in X pixels.
    ///
    /// The dialog's own picture is fetched once. Each step polls the pointer; a step
    /// **without horizontal movement** only calls `sleep(0)` (vertical-only movement is
    /// not followed until x changes too). Otherwise the displacement is zeroed per axis
    /// where it would take the dialog off the screen, and the saved contents are updated
    /// incrementally: the strips the dialog leaves are put back, the saved picture's
    /// pixels are shifted in place, and the strips it newly covers are fetched into it
    /// (`XGetSubImage`). If the displacement is as large as the dialog, the whole saved
    /// picture is put back and fetched anew. Then the window is moved and the dialog's
    /// picture drawn into it. Ends with `XSync` discarding events.
    ///
    /// The original looks the first window up and ignores it, and dereferences the second
    /// unchecked.
    fn msc_move_dialog(&mut self, parent: i32, id: i32) -> (i32, i32, i32) {
        self.ctx_get();
        self.wnd_get_from_id(parent);
        let w = self.wnd_get_from_id(id).expect("msc_moveDialog: the original dereferences a null window");
        let Some(bs) = self.ctx.as_deref().unwrap().wnd.windows[w].backing else { return (0, 0, 0) };
        let (ctx, sys) = st!(self);
        let xwindow = ctx.wnd.windows[w].xwindow;
        let (dpy, root, sw, sh) = (ctx.display, ctx.root, ctx.screen_width, ctx.screen_height);
        let (_, g) = sys.x.XGetGeometry(dpy, xwindow);
        let (mut gx, mut gy, gw, gh) = (g[1] as i32, g[2] as i32, g[3] as i32, g[4] as i32);
        let (mut total_x, mut total_y) = (0i32, 0i32);
        let picture = sys.x.XGetImage(dpy, root, gx, gy, gw as u32, gh as u32, 0xffff_ffff, 2);
        let (mut px, mut py, mut buttons) = self.msc_get_pointer();
        while buttons != 0 {
            let (nx, ny, b) = self.msc_get_pointer();
            buttons = b;
            let mut dx = nx - px;
            let mut dy = ny - py;
            let sys = &mut self.sys;
            if dx == 0 {
                sys.c.sleep(0);
                continue;
            }
            if dx + gx < 0 || sw < dx + gx + gw {
                dx = 0;
            }
            if dy + gy < 0 || sh < dy + gy + gh {
                dy = 0;
            }
            px = nx;
            py = ny;
            let gc = sys.x.screen_word(dpy, ctx_screen(&self.ctx), 0x2c);
            if dx.abs() < gw && dy.abs() < gh {
                if dx < 0 {
                    sys.x.XPutImage(dpy, xwindow, gc, bs.image, dx + bs.width, 0, dx + bs.width, 0, (-dx) as u32, bs.height as u32);
                } else if dx > 0 {
                    sys.x.XPutImage(dpy, xwindow, gc, bs.image, 0, 0, 0, 0, dx as u32, bs.height as u32);
                }
                if dy < 0 {
                    sys.x.XPutImage(dpy, xwindow, gc, bs.image, 0, dy + bs.height, 0, dy + bs.height, bs.width as u32, (-dy) as u32);
                } else if dy > 0 {
                    sys.x.XPutImage(dpy, xwindow, gc, bs.image, 0, 0, 0, 0, bs.width as u32, dy as u32);
                }
                let src_off = if dx < 1 { 0 } else { dx * bs.bytes_per_pixel };
                let dst_off = if dx < 0 { -dx * bs.bytes_per_pixel } else { 0 };
                let len = (bs.width - dx.abs()) * bs.bytes_per_pixel;
                if dy < 1 {
                    if dy < 0 {
                        let mut row = bs.height;
                        loop {
                            row -= 1;
                            if row < -dy {
                                break;
                            }
                            sys.x.image_data_move(bs.image, row * bs.bytes_per_line + dst_off, (dy + row) * bs.bytes_per_line + src_off, len);
                        }
                    } else {
                        for row in 0..bs.height {
                            sys.x.image_data_move(bs.image, row * bs.bytes_per_line + dst_off, row * bs.bytes_per_line + src_off, len);
                        }
                    }
                } else {
                    for row in 0..(bs.height - dy) {
                        sys.x.image_data_move(bs.image, row * bs.bytes_per_line + dst_off, (dy + row) * bs.bytes_per_line + src_off, len);
                    }
                }
                if dx < 0 {
                    sys.x.XGetSubImage(dpy, root, dx + gx, dy + gy, (-dx) as u32, bs.height as u32, 0xffff_ffff, 2, bs.image, 0, 0);
                } else if dx > 0 {
                    sys.x.XGetSubImage(dpy, root, gw + gx, dy + gy, dx as u32, bs.height as u32, 0xffff_ffff, 2, bs.image, bs.width - dx, 0);
                }
                if dy < 0 {
                    sys.x.XGetSubImage(dpy, root, dx + gx, dy + gy, bs.width as u32, (-dy) as u32, 0xffff_ffff, 2, bs.image, 0, 0);
                } else if dy > 0 {
                    sys.x.XGetSubImage(dpy, root, dx + gx, gh + gy, bs.width as u32, dy as u32, 0xffff_ffff, 2, bs.image, 0, bs.height - dy);
                }
            } else {
                sys.x.XPutImage(dpy, xwindow, gc, bs.image, 0, 0, 0, 0, bs.width as u32, bs.height as u32);
                sys.x.XGetSubImage(dpy, root, dx + gx, dy + gy, bs.width as u32, bs.height as u32, 0xffff_ffff, 2, bs.image, 0, 0);
            }
            gx += dx;
            gy += dy;
            total_x += dx;
            total_y += dy;
            sys.x.XMoveWindow(dpy, xwindow, gx, gy);
            sys.x.XPutImage(dpy, xwindow, gc, picture, 0, 0, 0, 0, gw as u32, gh as u32);
        }
        self.sys.x.XSync(dpy, 1);
        self.sys.x.XDestroyImage(picture);
        (1, total_x, total_y)
    }

    /// Hide a dialog: its picture is saved, the screen under it put back, the window
    /// unmapped and pending exposure events discarded [soft 0x0861499c msc_hideDialog].
    /// 0 for an unknown window or one without saved screen contents.
    fn msc_hide_dialog(&mut self, id: i32) -> i32 {
        self.ctx_get();
        let Some(w) = self.wnd_get_from_id(id) else { return 0 };
        let (ctx, sys) = st!(self);
        let win = &mut ctx.wnd.windows[w];
        if win.backing.is_none() {
            return 0;
        }
        let (_, g) = sys.x.XGetGeometry(ctx.display, win.xwindow);
        win.msc.hidden_image = sys.x.XGetImage(ctx.display, ctx.root, g[1] as i32, g[2] as i32, g[3], g[4], 0xffff_ffff, 2);
        win.msc.hidden_width = g[3] as i32;
        win.msc.hidden_height = g[4] as i32;
        self.wnd_backing_store_put(w);
        let (ctx, sys) = st!(self);
        sys.x.XUnmapWindow(ctx.display, ctx.wnd.windows[w].xwindow);
        self.evt_kill_x_events(0x8000, 1);
        1
    }

    /// Show a hidden dialog again: the screen under it is saved, the window mapped and
    /// its saved picture drawn and released [soft 0x08614a6c msc_unhideDialog]. Returns
    /// whether the window exists.
    fn msc_unhide_dialog(&mut self, id: i32) -> bool {
        self.ctx_get();
        let Some(w) = self.wnd_get_from_id(id) else { return false };
        let (ctx, sys) = st!(self);
        let xwindow = ctx.wnd.windows[w].xwindow;
        let (_, g) = sys.x.XGetGeometry(ctx.display, xwindow);
        let (gx, gy, gw, gh) = (g[1] as i32, g[2] as i32, g[3] as i32, g[4] as i32);
        let iy = ctx.screen_height - (gy + 1);
        self.wnd_backing_store_get(w, gx, iy, gw - 1 + gx, (iy + 1) - gh);
        let (ctx, sys) = st!(self);
        let m = ctx.wnd.windows[w].msc;
        sys.x.XMapWindow(ctx.display, xwindow);
        let gc = sys.x.screen_word(ctx.display, ctx.screen, 0x2c);
        sys.x.XPutImage(ctx.display, xwindow, gc, m.hidden_image, 0, 0, 0, 0, m.hidden_width as u32, m.hidden_height as u32);
        sys.x.XDestroyImage(m.hidden_image);
        true
    }

    /// Allocate the window's record of this file, cleared [soft 0x08614b60 MSC_initWindow].
    pub(crate) fn msc_init_window(&mut self, index: usize) {
        let (ctx, _) = st!(self);
        ctx.wnd.windows[index].msc = MscWindow::default();
    }

    /// Free it [soft 0x08614b80 MSC_exitWindow].
    pub(crate) fn msc_exit_window(&mut self, _index: usize) {}

    // ------------------------------------------------------------------ entry points

    /// [soft 0x08614ba0 GLOB_initRubberBand]
    pub fn glob_init_rubber_band(&mut self, px: i32, py: i32, left: i32, top: i32, width: i32, height: i32) {
        self.msc_init_rubber_band(px, py, left, top, width, height);
    }
    /// [soft 0x08614bd4 GLOB_exitRubberBand]
    pub fn glob_exit_rubber_band(&mut self) {
        self.msc_exit_rubber_band();
    }
    /// [soft 0x08614be4 GLOB_drawRubberBand]
    pub fn glob_draw_rubber_band(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.msc_draw_rubber_band(x1, y1, x2, y2);
    }
    /// [soft 0x08614c08 GLOB_clearRubberBand]
    pub fn glob_clear_rubber_band(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.msc_clear_rubber_band(x1, y1, x2, y2);
    }
    /// [soft 0x08614c2c GLOB_initSubregion]
    pub fn glob_init_subregion(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.msc_init_subregion(x, y, width, height);
    }
    /// [soft 0x08614c50 GLOB_exitSubregion]
    pub fn glob_exit_subregion(&mut self) {
        self.msc_exit_subregion();
    }
    /// [soft 0x08614c60 GLOB_restoreSubregion]
    pub fn glob_restore_subregion(&mut self, rect: &[i16; 4]) -> i32 {
        self.msc_restore_subregion(rect)
    }
    /// [soft 0x08614c78 GLOB_moveDialog]: (result, dx, dy).
    pub fn glob_move_dialog(&mut self, parent: i32, id: i32) -> (i32, i32, i32) {
        self.msc_move_dialog(parent, id)
    }
    /// [soft 0x08614c9c GLOB_hideDialog]
    pub fn glob_hide_dialog(&mut self, id: i32) -> i32 {
        self.msc_hide_dialog(id)
    }
    /// [soft 0x08614cb4 GLOB_unhideDialog]
    pub fn glob_unhide_dialog(&mut self, id: i32) -> bool {
        self.msc_unhide_dialog(id)
    }
}

fn ctx_screen(ctx: &Option<Box<Context>>) -> i32 {
    ctx.as_deref().map_or(0, |c| c.screen)
}
