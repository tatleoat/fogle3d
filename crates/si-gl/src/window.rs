// SPDX-License-Identifier: MIT
//! Windows (`window.c`, 0x08618990, 38 functions).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! An IRIS GL window is an X window with a GLX context and a colormap of its own, all three
//! created from the same visual. Windows are numbered from 1; the number is what `winopen`
//! returns. A sub-window (`swinopen`) is an override-redirect child of a top-level window;
//! the layer saves the screen under it when it is placed and puts the picture back when it
//! goes (the program's dialogs and menus are such windows).
//!
//! IRIS GL measures y upwards from the lower edge of the screen, X downwards from the upper
//! edge: an IRIS y becomes `height − (y + 1)` in X, with the height of the screen or of the
//! parent window.

use crate::context::{st, IrisGl};
use crate::state::*;
use crate::sys::{XSetWindowAttributes, XSizeHints, XWindowChanges};

impl IrisGl {
    // `wnd_idSortFunc` [soft 0x08618990 wnd_idSortFunc] compares two identifiers by
    // subtraction for `qsort`; here it is the sort in `wnd_get_new_id`.

    /// The smallest window identifier not in use [soft 0x086189a4 wnd_getNewID]
    /// (with its comparison function [soft 0x08618990 wnd_idSortFunc]).
    ///
    /// The identifiers in use are sorted. If the smallest is 2 or more the answer is 1.
    /// Otherwise the answer is one more than the end of the run of consecutive numbers the
    /// list starts with.
    fn wnd_get_new_id(&mut self) -> i32 {
        self.ctx_get();
        let (ctx, _) = st!(self);
        if ctx.wnd.windows.is_empty() {
            return 1;
        }
        let mut ids: Vec<i32> = ctx.wnd.windows.iter().map(|w| w.id).collect();
        // the comparison is a subtraction: the same order for identifiers this small
        ids.sort_by(|a, b| a.wrapping_sub(*b).cmp(&0));
        let mut result = -1;
        let n = ids.len();
        if ids[0] < 2 {
            if n > 1 {
                if ids[0] == ids[1] - 1 {
                    let mut i = 0;
                    loop {
                        let j = i + 1;
                        if n - 1 <= j {
                            break;
                        }
                        i = j;
                        if ids[j] != ids[j + 1] - 1 {
                            result = ids[j] + 1;
                            break;
                        }
                    }
                } else {
                    result = ids[0] + 1;
                }
            }
            if result == -1 {
                result = ids[n - 1] + 1;
            }
        } else {
            result = 1;
        }
        result
    }

    /// The first window of the table that is not the given one
    /// [soft 0x08618a7c wnd_findNotMe].
    fn wnd_find_not_me(&mut self, index: usize) -> Option<usize> {
        self.ctx_get();
        let (ctx, _) = st!(self);
        let id = ctx.wnd.windows[index].id;
        ctx.wnd.windows.iter().position(|w| w.id != id)
    }

    /// Ask the server for the window's geometry and keep x, y, width and height
    /// [soft 0x08618abc wnd_updateGeometry].
    pub(crate) fn wnd_update_geometry(&mut self, index: usize) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let w = &mut ctx.wnd.windows[index];
        let (_, g) = sys.x.XGetGeometry(ctx.display, w.xwindow);
        w.x = g[1] as i32;
        w.y = g[2] as i32;
        w.width = g[3] as i32;
        w.height = g[4] as i32;
    }

    /// Set the window's title [soft 0x08618b18 wnd_setName].
    fn wnd_set_name(&mut self, index: usize, name: &[u8]) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let (_, prop) = sys.x.XStringListToTextProperty(name, 1);
        sys.x.XSetWMName(ctx.display, ctx.wnd.windows[index].xwindow, &prop);
    }

    /// Hand the pending constraints of the current window to the window manager and clear
    /// them [soft 0x08618b58 wnd_setconstraints]. `reconfigure` is 0 from `wnd_create` and
    /// 1 from `winconstraints`.
    ///
    /// - A `_MOTIF_WM_HINTS` property of five longs: flags 2 (decorations) and the
    ///   decorations 0 after `noborder`, 0x1a (border, title, menu) when a size is pending,
    ///   else 1 (all). The other three longs are whatever the stack held.
    /// - With `reconfigure`: `XReconfigureWMWindow` setting the border width to 0.
    /// - Size hints with minimum size, maximum size, resize increment and aspect always
    ///   present (flags 0xf0): the values given, or 1 × 1, the screen size, 1 × 1 and the
    ///   range 1 : screen height to screen width : 1. A pending position adds the position
    ///   (flags | 5), a pending size together with `reconfigure` the size (flags | 10).
    fn wnd_setconstraints(&mut self, reconfigure: i32) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let c = ctx.wnd.constraints;
        let cur = ctx.wnd.current.expect("wnd_setconstraints: the original dereferences the null current window");
        let xwindow = ctx.wnd.windows[cur].xwindow;
        let hints_ptr = sys.x.XAllocSizeHints();
        let decorations = if c & CONSTRAINT_NOBORDER != 0 {
            0
        } else if c & CONSTRAINT_SIZE == 0 {
            1
        } else {
            0x1a
        };
        // flags, functions, decorations, input mode, status; the original leaves functions,
        // input mode and status uninitialised (zero here)
        let motif = [2u32, 0, decorations, 0, 0];
        sys.x.XChangeProperty(ctx.display, xwindow, ctx.atoms.motif_wm_hints, ctx.atoms.motif_wm_hints, 0x20, 0, &motif, 5);
        if reconfigure != 0 {
            sys.x.XReconfigureWMWindow(ctx.display, xwindow, ctx.screen, 0x10, &XWindowChanges { border_width: 0 });
        }
        let mut h = XSizeHints { flags: 0xf0, ..Default::default() };
        let w = &ctx.wnd;
        h.min_width = if c & CONSTRAINT_MINSIZE != 0 { w.min_width } else { 1 };
        h.min_height = if c & CONSTRAINT_MINSIZE != 0 { w.min_height } else { 1 };
        h.max_width = if c & CONSTRAINT_MAXSIZE != 0 { w.max_width } else { ctx.screen_width };
        h.max_height = if c & CONSTRAINT_MAXSIZE != 0 { w.max_height } else { ctx.screen_height };
        if c & CONSTRAINT_ASPECT == 0 {
            h.min_aspect_x = 1;
            h.min_aspect_y = ctx.screen_height;
            h.max_aspect_x = ctx.screen_width;
            h.max_aspect_y = 1;
        } else {
            h.max_aspect_x = w.aspect_x;
            h.min_aspect_x = w.aspect_x;
            h.max_aspect_y = w.aspect_y;
            h.min_aspect_y = w.aspect_y;
        }
        h.width_inc = if c & CONSTRAINT_STEP != 0 { w.step_x } else { 1 };
        h.height_inc = if c & CONSTRAINT_STEP != 0 { w.step_y } else { 1 };
        let (pos_x, pos_y, pref_w, pref_h) = (w.pos_x, w.pos_y, w.pref_width, w.pref_height);
        if c & CONSTRAINT_POSITION != 0 {
            h.flags |= 5;
            h.x = pos_x;
            let parent_id = ctx.wnd.windows[cur].parent;
            match self.wnd_get_from_id(parent_id) {
                None => {
                    let (ctx, _) = st!(self);
                    h.y = ctx.screen_height - (pos_y + 1);
                }
                Some(p) => {
                    self.wnd_update_geometry(p);
                    let (ctx, _) = st!(self);
                    h.y = ctx.wnd.windows[p].height - (pos_y + 1);
                }
            }
        }
        if c & CONSTRAINT_SIZE != 0 && reconfigure != 0 {
            h.flags |= 10;
            h.width = pref_w;
            h.height = pref_h;
        }
        let (ctx, sys) = st!(self);
        sys.x.XSetWMNormalHints(ctx.display, xwindow, &h);
        sys.x.XFree(hints_ptr);
        ctx.wnd.constraints = 0;
    }

    /// Create a window: `parent` is 0 (or less) for a top-level window, else the identifier
    /// of the parent [soft 0x08618d3c wnd_create]. Returns the new window's index.
    ///
    /// The new window gets the smallest free identifier, the best visual, a new colormap
    /// and a new GLX context (direct, not sharing). Top-level: attributes background
    /// pixmap none, border pixel 0, bit gravity static, the layer's event mask, the
    /// colormap (mask 0x2819). Sub-window: the same without the gravity and with
    /// override-redirect (mask 0x2a09). Position from a pending `prefposition`, else
    /// (0, 0); size from a pending size, else 100 × 100. For a sub-window the screen under
    /// the pending position rectangle is saved first. Then: `WM_DELETE_WINDOW` protocol,
    /// the window becomes current, title, constraints, geometry, the GLX context is made
    /// current, the per-window records are set up, the window is mapped unless `noport`
    /// was pending, `XSync`, and a pause of 30 ms.
    fn wnd_create(&mut self, name: &[u8], parent: i32) -> usize {
        self.ctx_get();
        let id = self.wnd_get_new_id();
        let (ctx, _) = st!(self);
        ctx.wnd.windows.push(Window { id, ..Default::default() });
        let me = ctx.wnd.windows.len() - 1;
        let p = if parent < 1 { None } else { self.wnd_get_from_id(parent) };
        {
            let (ctx, _) = st!(self);
            ctx.wnd.windows[me].parent = match p {
                Some(p) => ctx.wnd.windows[p].id,
                None => 0,
            };
        }
        let vi = self.wnd_get_best_visual();
        let (ctx, sys) = st!(self);
        let visual = sys.x.visual_info_word(vi, 0);
        let depth = sys.x.visual_info_word(vi, 0xc) as i32;
        let colormap = sys.x.XCreateColormap(ctx.display, ctx.root, visual, 0);
        let glx = sys.glx.glXCreateContext(ctx.display, vi, 0, 1);
        {
            let w = &mut ctx.wnd.windows[me];
            w.visual_info = vi;
            w.colormap = colormap;
            w.glx_context = glx;
        }
        let mut attributes = XSetWindowAttributes { event_mask: ctx.wnd.event_mask, colormap, ..Default::default() };
        let valuemask = if p.is_some() {
            attributes.override_redirect = 1;
            0x2a09
        } else {
            attributes.bit_gravity = 10;
            0x2819
        };
        let (x, y);
        if ctx.wnd.constraints & CONSTRAINT_POSITION != 0 {
            let height = match p {
                Some(p) => {
                    self.wnd_update_geometry(p);
                    let (ctx, _) = st!(self);
                    ctx.wnd.windows[p].height
                }
                None => ctx.screen_height,
            };
            let (ctx, _) = st!(self);
            x = ctx.wnd.pos_x;
            y = height - (ctx.wnd.pos_y + 1);
        } else {
            x = 0;
            y = 0;
        }
        let (ctx, _) = st!(self);
        let (width, height) = if ctx.wnd.constraints & CONSTRAINT_SIZE != 0 {
            (ctx.wnd.pref_width, ctx.wnd.pref_height)
        } else {
            (100, 100)
        };
        let xparent = match p {
            Some(p) => {
                let w = &ctx.wnd;
                let (x1, y1, x2, y2) = (w.pos_x, w.pos_y - w.pref_height + 1, w.pref_width + w.pos_x - 1, w.pos_y);
                self.wnd_backing_store_get(me, x1, y1, x2, y2);
                let (ctx, _) = st!(self);
                ctx.wnd.windows[p].xwindow
            }
            None => ctx.root,
        };
        let (ctx, sys) = st!(self);
        let flags = ctx.wnd.constraints;
        let xwindow = sys.x.XCreateWindow(ctx.display, xparent, x, y, width as u32, height as u32, 0, depth, 1, visual, valuemask, &attributes);
        ctx.wnd.windows[me].xwindow = xwindow;
        sys.x.XSetWMProtocols(ctx.display, xwindow, ctx.atoms.wm_delete_window, 1);
        ctx.wnd.current = Some(me);
        self.wnd_set_name(me, name);
        self.wnd_setconstraints(0);
        self.wnd_update_geometry(me);
        let (ctx, sys) = st!(self);
        sys.glx.glXMakeCurrent(ctx.display, xwindow, glx);
        self.crs_init_window(me);
        self.evt_init_window(me);
        self.fnt_init_window(me);
        self.gfx_init_window(me);
        self.msc_init_window(me);
        self.pck_init_window(me);
        let (ctx, sys) = st!(self);
        if flags & CONSTRAINT_NOPORT == 0 {
            sys.x.XMapWindow(ctx.display, xwindow);
        }
        sys.x.XSync(ctx.display, 0);
        sys.c.usleep(30000);
        me
    }

    /// Allocate the window state [soft 0x08619008 WND_init]: no windows, no event mask,
    /// no pending constraints.
    pub(crate) fn wnd_init(&mut self) {
        let (ctx, _) = st!(self);
        ctx.wnd = WndContext::default();
    }

    /// Add `set` to and remove `clear` from the X event mask of every window
    /// [soft 0x0861908c WND_eventMask]. Does nothing when both are 0.
    pub(crate) fn wnd_event_mask(&mut self, set: u32, clear: u32) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        if set != 0 || clear != 0 {
            let mask = (ctx.wnd.event_mask | set) & !clear;
            ctx.wnd.event_mask = mask;
            let attributes = XSetWindowAttributes { event_mask: mask, ..Default::default() };
            for w in &ctx.wnd.windows {
                sys.x.XChangeWindowAttributes(ctx.display, w.xwindow, 0x800, &attributes);
            }
        }
    }

    /// The window with the given X window [soft 0x08619104 WND_getFromXWindow].
    pub(crate) fn wnd_get_from_xwindow(&mut self, xwindow: u32) -> Option<usize> {
        self.ctx_get_ng();
        let (ctx, _) = st!(self);
        ctx.wnd.windows.iter().position(|w| w.xwindow == xwindow)
    }

    /// The window with the given identifier [soft 0x0861914c WND_getFromID].
    pub(crate) fn wnd_get_from_id(&mut self, id: i32) -> Option<usize> {
        self.ctx_get_ng();
        let (ctx, _) = st!(self);
        ctx.wnd.windows.iter().position(|w| w.id == id)
    }

    /// Save the part of the screen a sub-window is about to cover
    /// [soft 0x0861918c WND_backingStoreGet]. The rectangle is given by two corners in
    /// IRIS GL screen coordinates. Does nothing for a top-level window.
    ///
    /// An earlier saved picture is destroyed. The corners are clamped to the screen, the
    /// rectangle becomes an X rectangle on the root window, and after `XSync` and
    /// `glFinish` its pixels are fetched with `XGetImage` (all planes, Z format).
    pub(crate) fn wnd_backing_store_get(&mut self, index: usize, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let old = ctx.wnd.windows[index].backing;
        if ctx.wnd.windows[index].parent == 0 {
            return;
        }
        let (mut hi, mut lo) = (y1, y2);
        if y1 < y2 {
            hi = y2;
            lo = y1;
        }
        if let Some(old) = old {
            // through the image's own destroy function; the original does not test the
            // image pointer (a failed XGetImage earlier would crash here)
            sys.x.XDestroyImage(old.image);
        }
        let clamp = |v: i32, size: i32| {
            let v = if v < 0 { 0 } else { v };
            if size <= v {
                size - 1
            } else {
                v
            }
        };
        let (sw, sh) = (ctx.screen_width, ctx.screen_height);
        let x1 = clamp(x1, sw);
        let x2 = clamp(x2, sw);
        hi = clamp(hi, sh);
        lo = clamp(lo, sh);
        let mut bs = BackingStore { x: x1, y: sh - (hi + 1), width: (x2 - x1) + 1, height: (lo - hi).wrapping_abs() + 1, ..Default::default() };
        ctx.wnd.windows[index].backing = Some(bs);
        sys.x.XSync(ctx.display, 0);
        sys.gl.glFinish();
        bs.image = sys.x.XGetImage(ctx.display, ctx.root, bs.x, bs.y, bs.width as u32, bs.height as u32, 0xffff_ffff, 2);
        // the original reads the image's fields unchecked (a null image crashes it)
        let bits = sys.x.image_word(bs.image, 0x2c) as i32;
        bs.bytes_per_pixel = (if bits < 0 { bits + 7 } else { bits }) >> 3;
        bs.bytes_per_line = sys.x.image_word(bs.image, 0x28) as i32;
        ctx.wnd.windows[index].backing = Some(bs);
    }

    /// Put the saved screen contents back, for a sub-window that is going away or moving
    /// [soft 0x086192d0 WND_backingStorePut]. Does nothing for a top-level window.
    ///
    /// Without a saved picture a REDRAW event for the parent is queued instead. With one:
    /// the draw buffer is switched to the front, the window raised, and the picture put
    /// **into the window itself** at (0, 0) with the screen's default GC; then the picture
    /// is destroyed and the draw buffer restored. (`getorigin` is called in between and its
    /// results are dropped.)
    pub(crate) fn wnd_backing_store_put(&mut self, index: usize) {
        self.ctx_get();
        let (ctx, _) = st!(self);
        let bs = ctx.wnd.windows[index].backing;
        let parent = ctx.wnd.windows[index].parent;
        if parent == 0 {
            return;
        }
        self.wnd_get_from_id(parent);
        match bs {
            Some(bs) if bs.image != 0 => {
                let (ctx, sys) = st!(self);
                let xwindow = ctx.wnd.windows[index].xwindow;
                let mut saved = [0i32; 1];
                sys.gl.glGetIntegerv(0xc01, &mut saved);
                sys.gl.glDrawBuffer(0x404);
                sys.gl.glFinish();
                sys.x.XRaiseWindow(ctx.display, xwindow);
                self.getorigin();
                let (ctx, sys) = st!(self);
                let gc = sys.x.screen_word(ctx.display, ctx.screen, 0x2c);
                sys.x.XPutImage(ctx.display, xwindow, gc, bs.image, 0, 0, 0, 0, bs.width as u32, bs.height as u32);
                sys.x.XDestroyImage(bs.image);
                sys.gl.glDrawBuffer(saved[0] as u32);
            }
            _ => self.evt_add_event(0x210, parent as i16),
        }
        let (ctx, _) = st!(self);
        ctx.wnd.windows[index].backing = None;
    }

    // ------------------------------------------------------------------ entry points

    /// IRIS GL `winopen`: create a top-level window, make it current, return its identifier
    /// [soft 0x086193e0 winopen].
    pub fn winopen(&mut self, name: &[u8]) -> i32 {
        self.sys.glx.glXWaitGL();
        let w = self.wnd_create(name, 0);
        self.sys.glx.glXWaitX();
        self.ctx.as_deref().unwrap().wnd.windows[w].id
    }

    /// IRIS GL `swinopen`: create a sub-window of `parent`, or of `parent`'s own parent if
    /// it is itself a sub-window, so sub-windows never nest [soft 0x0861940c swinopen].
    /// Returns −1 if there is no such window (and then `glXWaitX` is not called).
    pub fn swinopen(&mut self, parent: i32) -> i32 {
        self.sys.glx.glXWaitGL();
        let mut p = self.wnd_get_from_id(parent);
        if let Some(i) = p {
            let grand = self.ctx.as_deref().unwrap().wnd.windows[i].parent;
            if grand > 0 {
                p = self.wnd_get_from_id(grand);
            }
            if let Some(i) = p {
                let pid = self.ctx.as_deref().unwrap().wnd.windows[i].id;
                let w = self.wnd_create(b"", pid);
                self.sys.glx.glXWaitX();
                return self.ctx.as_deref().unwrap().wnd.windows[w].id;
            }
        }
        -1
    }

    /// IRIS GL `winclose` [soft 0x0861946c winclose].
    ///
    /// The saved screen contents are put back, the per-window records freed, and after
    /// `glFinish` and `XSync` the GLX context of some other window (the first other one in
    /// the table, or the start-up window) is made current before this window's context,
    /// X window, visual record and colormap are destroyed. The window leaves the table;
    /// the current window stays the one it was, or becomes that other window if it was
    /// this one (none if no window is left).
    pub fn winclose(&mut self, id: i32) {
        self.ctx_get();
        let Some(index) = self.wnd_get_from_id(id) else { return };
        self.sys.glx.glXWaitGL();
        let other = self.wnd_find_not_me(index);
        let other_id = match other {
            Some(o) => self.ctx.as_deref().unwrap().wnd.windows[o].id,
            None => 0,
        };
        self.wnd_backing_store_put(index);
        self.crs_exit_window(index);
        self.evt_exit_window(index);
        self.fnt_exit_window(index);
        self.gfx_exit_window(index);
        self.msc_exit_window(index);
        self.pck_exit_window(index);
        let (ctx, sys) = st!(self);
        sys.gl.glFinish();
        sys.x.XSync(ctx.display, 0);
        let (drawable, glx) = match other {
            Some(o) => (ctx.wnd.windows[o].xwindow, ctx.wnd.windows[o].glx_context),
            None => {
                let init = ctx.init_window.expect("winclose: the original dereferences the start-up window record");
                (init.window, init.glx_context)
            }
        };
        sys.glx.glXMakeCurrent(ctx.display, drawable, glx);
        let w = &ctx.wnd.windows[index];
        sys.glx.glXDestroyContext(ctx.display, w.glx_context);
        sys.x.XDestroyWindow(ctx.display, w.xwindow);
        sys.x.XFree(w.visual_info);
        sys.x.XFreeColormap(ctx.display, w.colormap);
        let current = ctx.wnd.current.expect("winclose: the original dereferences the null current window");
        let current_id = ctx.wnd.windows[current].id;
        // the original takes the last table entry with this identifier
        let closed_id = ctx.wnd.windows[index].id;
        let at = ctx.wnd.windows.iter().rposition(|w| w.id == closed_id).unwrap();
        ctx.wnd.windows.remove(at);
        ctx.wnd.current = None;
        let mut cur = self.wnd_get_from_id(current_id);
        if cur.is_none() {
            cur = self.wnd_get_from_id(other_id);
        }
        self.ctx.as_deref_mut().unwrap().wnd.current = cur;
        self.sys.glx.glXWaitX();
    }

    /// IRIS GL `foreground`: nothing [soft 0x08619654 foreground].
    pub fn foreground(&mut self) {}

    /// IRIS GL `minsize`, for the next `winopen` or `winconstraints`
    /// [soft 0x0861965c minsize]. Values below 1 become 1.
    pub fn minsize(&mut self, x: i32, y: i32) {
        self.ctx_get_ng();
        let (ctx, _) = st!(self);
        ctx.wnd.min_width = x.max(1);
        ctx.wnd.min_height = y.max(1);
        ctx.wnd.constraints |= CONSTRAINT_MINSIZE;
    }

    /// IRIS GL `maxsize` [soft 0x08619698 maxsize]. Values below 1 become 1.
    pub fn maxsize(&mut self, x: i32, y: i32) {
        self.ctx_get_ng();
        let (ctx, _) = st!(self);
        ctx.wnd.max_width = x.max(1);
        ctx.wnd.max_height = y.max(1);
        ctx.wnd.constraints |= CONSTRAINT_MAXSIZE;
    }

    /// IRIS GL `prefsize` [soft 0x086196d4 prefsize]. Not clamped.
    pub fn prefsize(&mut self, x: i32, y: i32) {
        self.ctx_get_ng();
        let (ctx, _) = st!(self);
        ctx.wnd.pref_width = x;
        ctx.wnd.pref_height = y;
        ctx.wnd.constraints |= CONSTRAINT_SIZE;
    }

    /// IRIS GL `prefposition(x1, x2, y1, y2)` [soft 0x086196fc prefposition]: the left
    /// edge, the upper edge and the absolute size of the rectangle.
    pub fn prefposition(&mut self, x1: i32, x2: i32, y1: i32, y2: i32) {
        self.ctx_get_ng();
        let (ctx, _) = st!(self);
        ctx.wnd.pos_x = if x1 < x2 { x1 } else { x2 };
        ctx.wnd.pos_y = if y2 < y1 { y1 } else { y2 };
        ctx.wnd.pref_width = x2.wrapping_sub(x1).wrapping_add(1).wrapping_abs();
        ctx.wnd.pref_height = y2.wrapping_sub(y1).wrapping_add(1).wrapping_abs();
        ctx.wnd.constraints |= CONSTRAINT_POSITION | CONSTRAINT_SIZE;
    }

    /// IRIS GL `keepaspect` [soft 0x08619758 keepaspect]. Each value is kept within
    /// 1..=0x7fff.
    pub fn keepaspect(&mut self, x: i32, y: i32) {
        self.ctx_get_ng();
        let (ctx, _) = st!(self);
        ctx.wnd.aspect_x = if x < 1 { 1 } else { x.min(0x7fff) };
        ctx.wnd.aspect_y = if y < 1 { 1 } else { y.min(0x7fff) };
        ctx.wnd.constraints |= CONSTRAINT_ASPECT;
    }

    /// IRIS GL `stepunit` [soft 0x086197b8 stepunit]. Values below 1 become 1.
    pub fn stepunit(&mut self, x: i32, y: i32) {
        self.ctx_get_ng();
        let (ctx, _) = st!(self);
        ctx.wnd.step_x = x.max(1);
        ctx.wnd.step_y = y.max(1);
        ctx.wnd.constraints |= CONSTRAINT_STEP;
    }

    /// IRIS GL `fudge` [soft 0x086197f4 fudge]. Stored (negative values as 0) and flagged;
    /// nothing reads the values.
    pub fn fudge(&mut self, x: i32, y: i32) {
        self.ctx_get_ng();
        let (ctx, _) = st!(self);
        ctx.wnd.fudge_x = x.max(0);
        ctx.wnd.fudge_y = y.max(0);
        ctx.wnd.constraints |= CONSTRAINT_FUDGE;
    }

    /// IRIS GL `noborder` [soft 0x08619828 noborder].
    pub fn noborder(&mut self) {
        self.ctx_get_ng();
        self.ctx.as_deref_mut().unwrap().wnd.constraints |= CONSTRAINT_NOBORDER;
    }

    /// IRIS GL `noport`: the next window is created without being mapped
    /// [soft 0x08619840 noport].
    pub fn noport(&mut self) {
        self.ctx_get_ng();
        self.ctx.as_deref_mut().unwrap().wnd.constraints |= CONSTRAINT_NOPORT;
    }

    /// IRIS GL `winconstraints`: apply the pending constraints to the current window
    /// [soft 0x08619858 winconstraints].
    pub fn winconstraints(&mut self) {
        self.ctx_get();
        self.sys.glx.glXWaitGL();
        if self.cur().is_some() {
            self.wnd_setconstraints(1);
        }
        self.sys.glx.glXWaitX();
    }

    /// IRIS GL `windepth`: always 0 [soft 0x0861988c windepth].
    pub fn windepth(&mut self) -> i32 {
        0
    }

    /// IRIS GL `winpop`: raise the current window [soft 0x08619898 winpop].
    pub fn winpop(&mut self) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        sys.glx.glXWaitGL();
        if let Some(c) = ctx.wnd.current {
            sys.x.XRaiseWindow(ctx.display, ctx.wnd.windows[c].xwindow);
        }
        sys.glx.glXWaitX();
    }

    /// IRIS GL `winpush`: lower the current window [soft 0x086198d8 winpush].
    pub fn winpush(&mut self) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        sys.glx.glXWaitGL();
        if let Some(c) = ctx.wnd.current {
            sys.x.XLowerWindow(ctx.display, ctx.wnd.windows[c].xwindow);
        }
        sys.glx.glXWaitX();
    }

    /// IRIS GL `winget`: the current window's identifier, −1 if there is none
    /// [soft 0x08619918 winget].
    pub fn winget(&mut self) -> i32 {
        self.ctx_get_ng();
        let (ctx, _) = st!(self);
        match ctx.wnd.current {
            Some(c) => ctx.wnd.windows[c].id,
            None => -1,
        }
    }

    /// IRIS GL `winset`: make a window current, with its GLX context
    /// [soft 0x0861993c winset]. An unknown identifier is ignored.
    pub fn winset(&mut self, id: i32) {
        self.ctx_get();
        if let Some(i) = self.wnd_get_from_id(id) {
            let (ctx, sys) = st!(self);
            sys.glx.glXWaitGL();
            sys.x.XSync(ctx.display, 0);
            let w = &ctx.wnd.windows[i];
            sys.glx.glXMakeCurrent(ctx.display, w.xwindow, w.glx_context);
            ctx.wnd.current = Some(i);
            sys.glx.glXWaitX();
        }
    }

    /// IRIS GL `wintitle` [soft 0x0861999c wintitle].
    pub fn wintitle(&mut self, name: &[u8]) {
        self.ctx_get();
        self.sys.glx.glXWaitGL();
        if let Some(c) = self.cur() {
            self.wnd_set_name(c, name);
        }
        self.sys.glx.glXWaitX();
    }

    /// IRIS GL `winmove(x, y)`: move the current window so that its lower left corner is at
    /// (x, y) [soft 0x086199d8 winmove].
    ///
    /// The X position is `H − (y + 1) − height`, where `height` is the window's after a
    /// fresh geometry query. `H` is the screen height for a top-level window. For a
    /// sub-window the parent's geometry is refreshed, but `H` is then taken from **this
    /// window's own** stored height, as it was before the query.
    pub fn winmove(&mut self, x: i32, y: i32) {
        self.ctx_get();
        let Some(c) = self.cur() else { return };
        self.sys.glx.glXWaitGL();
        let parent_id = self.ctx.as_deref().unwrap().wnd.windows[c].parent;
        let h = match self.wnd_get_from_id(parent_id) {
            Some(p) => {
                self.wnd_update_geometry(p);
                self.ctx.as_deref().unwrap().wnd.windows[c].height
            }
            None => self.ctx.as_deref().unwrap().screen_height,
        };
        self.wnd_update_geometry(c);
        let (ctx, sys) = st!(self);
        let w = &ctx.wnd.windows[c];
        sys.x.XMoveWindow(ctx.display, w.xwindow, x, h.wrapping_sub(y.wrapping_add(1)).wrapping_sub(w.height));
        self.wnd_update_geometry(c);
        self.sys.glx.glXWaitX();
    }

    /// IRIS GL `winposition(x1, x2, y1, y2)`: move and resize the current window
    /// [soft 0x08619a64 winposition].
    ///
    /// For a sub-window that has saved screen contents, they are put back, the window is
    /// unmapped, the screen under the new rectangle is saved, and the window is mapped
    /// again after the move.
    pub fn winposition(&mut self, x1: i32, x2: i32, y1: i32, y2: i32) {
        self.ctx_get();
        let Some(c) = self.cur() else { return };
        self.sys.glx.glXWaitGL();
        let width = x2.wrapping_sub(x1).wrapping_add(1).wrapping_abs();
        let height = y2.wrapping_sub(y1).wrapping_add(1).wrapping_abs();
        let left = if x2 > x1 { x1 } else { x2 };
        let top = if y2 < y1 { y1 } else { y2 };
        let parent_id = self.ctx.as_deref().unwrap().wnd.windows[c].parent;
        let y = match self.wnd_get_from_id(parent_id) {
            Some(p) => {
                if self.ctx.as_deref().unwrap().wnd.windows[c].backing.is_some() {
                    self.wnd_backing_store_put(c);
                    let (ctx, sys) = st!(self);
                    sys.x.XUnmapWindow(ctx.display, ctx.wnd.windows[c].xwindow);
                    self.wnd_backing_store_get(c, x1, y1, x2, y2);
                }
                self.wnd_update_geometry(p);
                self.ctx.as_deref().unwrap().wnd.windows[p].height - (top + 1)
            }
            None => self.ctx.as_deref().unwrap().screen_height - (top + 1),
        };
        let (ctx, sys) = st!(self);
        let xwindow = ctx.wnd.windows[c].xwindow;
        sys.x.XMoveResizeWindow(ctx.display, xwindow, left, y, width, height);
        if ctx.wnd.windows[c].backing.is_some() {
            sys.x.XMapWindow(ctx.display, xwindow);
        }
        sys.x.XSync(ctx.display, 0);
        self.wnd_update_geometry(c);
        self.sys.glx.glXWaitX();
    }

    /// IRIS GL `getorigin`: the current window's lower left corner on the screen, IRIS GL
    /// coordinates [soft 0x08619ba4 getorigin]. `None` without a current window (the
    /// original then leaves the caller's variables alone). `glXWaitX` is not called.
    pub fn getorigin(&mut self) -> Option<(i32, i32)> {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let c = ctx.wnd.current?;
        let xwindow = ctx.wnd.windows[c].xwindow;
        sys.glx.glXWaitGL();
        let (_, g) = sys.x.XGetGeometry(ctx.display, xwindow);
        let (_, t) = sys.x.XTranslateCoordinates(ctx.display, xwindow, ctx.root, 0, 0);
        Some((t[0] as i32, ctx.screen_height.wrapping_sub(t[1] as i32).wrapping_sub(g[4] as i32)))
    }

    /// IRIS GL `getsize`: the current window's width and height, from a fresh geometry
    /// query [soft 0x08619c34 getsize]. `None` without a current window.
    pub fn getsize(&mut self) -> Option<(i32, i32)> {
        self.ctx_get();
        let c = self.cur()?;
        self.sys.glx.glXWaitGL();
        self.wnd_update_geometry(c);
        let w = &self.ctx.as_deref().unwrap().wnd.windows[c];
        let r = (w.width, w.height);
        self.sys.glx.glXWaitX();
        Some(r)
    }
}
