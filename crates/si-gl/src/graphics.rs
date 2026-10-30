// SPDX-License-Identifier: MIT
//! Drawing, matrices, colours, buffers and pixels (`graphics.c`, 0x0860f400, 179 functions).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! Most IRIS GL drawing calls are one OpenGL call. What the layer adds of its own:
//!
//! - **Colour-map mode is emulated.** There is no indexed visual: a 4096-entry table of
//!   RGB triples lives in the layer, `color(i)` issues `glColor4d` with entry
//!   `i & writemask`, and `mapcolor` only changes the table (pixels already drawn keep
//!   their colour).
//! - **Matrix modes.** In MSINGLE (0, the start) everything goes to the OpenGL model-view
//!   stack, projections included. Leaving or entering MSINGLE empties the three OpenGL
//!   stacks (pop until OpenGL reports an underflow) and loads identities.
//! - **Projections restore the pick matrix.** `window`, `ortho`, `ortho2`, `perspective`
//!   and `loadmatrix` start from `gluPickMatrix` instead of the identity while picking.
//! - **Frame buffer configuration** (`RGBmode`, `doublebuffer`, `gconfig`, …) only switches
//!   the layer's own flags and the draw buffer: every window has the same double-buffered
//!   RGBA visual.
//! - **Angles** are tenths of a degree (`rotate`, `perspective`, `lookat`, arcs).
//!
//! Floating-point arguments are computed as the original's x87 code computes them and
//! rounded where it stores them.

use crate::context::{st, IrisGl};
use crate::state::*;
use si_x87::{x, Round, F80};

/// A 4 × 4 matrix as IRIS GL passes it: sixteen floats, rows first.
pub type Matrix = [[f32; 4]; 4];

fn flat(m: &Matrix) -> [f32; 16] {
    let mut o = [0.0f32; 16];
    for (i, r) in m.iter().enumerate() {
        o[i * 4..i * 4 + 4].copy_from_slice(r);
    }
    o
}

/// An integer over a double constant, rounded to double where the original stores it.
fn int_over(a: i32, d: f64) -> f64 {
    (x(a) / x(d)).to_f64()
}

/// An integer over a double constant, rounded to float.
fn int_over_f32(a: i32, d: f64) -> f32 {
    (x(a) / x(d)).to_f32()
}

impl IrisGl {
    /// The current window's drawing state (`*(current + 0x38)`), if there is a window.
    fn gfx_cur(&mut self) -> Option<&mut GfxWindow> {
        let ctx = self.ctx.as_deref_mut()?;
        let c = ctx.wnd.current?;
        Some(&mut ctx.wnd.windows[c].gfx)
    }

    /// Fill the colour map [soft 0x0860f400 gfx_initCmap].
    ///
    /// All 4096 entries black, then: 0..=15 the sixteen start-up colours; 32..=55 a grey
    /// ramp, entry 32 + i having the level `255 (i + 1) / 25`; 56..=255 a colour cube,
    /// entry 56 + n having red `((n / 8) mod 5) × 51`, green `(n mod 8) × 255 / 7`, blue
    /// `(n / 40) × 51`.
    fn gfx_init_cmap(gfx: &mut GfxContext) {
        gfx.cmap.iter_mut().for_each(|b| *b = 0);
        gfx.cmap[..48].copy_from_slice(crate::data::VGA_COLOURS.get());
        let mut level = 0xff;
        for i in 0..0x18 {
            let v = (level / 0x19) as u8;
            let at = 0x60 + i * 3;
            gfx.cmap[at + 2] = v;
            gfx.cmap[at + 1] = v;
            gfx.cmap[at] = v;
            level += 0xff;
        }
        for n in 0..200i32 {
            let at = (0xa8 + n * 3) as usize;
            let q = n >> 3;
            let b = (q / 5) as u8;
            gfx.cmap[at] = ((q as u8).wrapping_add(b.wrapping_mul(0xfb))).wrapping_mul(0x33);
            gfx.cmap[at + 1] = (((n & 7) * 0xff) / 7) as u8;
            gfx.cmap[at + 2] = b.wrapping_mul(0x33);
        }
    }

    /// Allocate the graphics state [soft 0x0860f54c GFX_init]: RGB flag 1, the bit counts
    /// of the start-up window's visual from `glXGetConfig` (red 8, green 9, blue 10, alpha
    /// 11, depth 12, stencil 13, double buffer 5), and the colour map.
    pub(crate) fn gfx_init(&mut self) {
        let (ctx, sys) = st!(self);
        let mut gfx = GfxContext::default();
        gfx.config.rgb = 1;
        let vi = ctx.init_window.expect("GFX_init: the original dereferences the start-up window record").visual_info;
        let mut get = |attrib: i32| sys.glx.glXGetConfig(ctx.display, vi, attrib).1;
        gfx.config.red = get(8);
        gfx.config.green = get(9);
        gfx.config.blue = get(10);
        gfx.config.alpha = get(0xb);
        gfx.config.depth = get(0xc);
        gfx.config.stencil = get(0xd);
        gfx.config.double = get(5) as u8;
        Self::gfx_init_cmap(&mut gfx);
        ctx.gfx = gfx;
    }

    /// Set up a new window's drawing state and its OpenGL context
    /// [soft 0x0860f628 GFX_initWindow].
    ///
    /// State: graphics position 0, colour index 0, write mask 0xfff, colour black with
    /// alpha 255, no line style (repeat 1), no pattern, normal draw mode, MSINGLE, front
    /// buffer, zoom 1. The configuration in force is the visual's with the mode set to
    /// colour map and single buffer; the wanted one a copy of it.
    ///
    /// OpenGL: front buffer for drawing and reading; identities on the projection, texture
    /// and model-view stacks; then on the model-view stack `glOrtho(−0.5, width − 0.5,
    /// −0.5, height − 0.5, −1, 1)`; viewport and scissor box the whole window, scissor
    /// test on; depth test, line stipple and polygon stipple off; `glFinish`.
    pub(crate) fn gfx_init_window(&mut self, index: usize) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let mut g = GfxWindow { writemask: 0xfff, rgba: [0, 0, 0, 0xff], ls_repeat: 1, drawmode_effective: 0x10, drawmode: 0x10, buffer: 0x404, zoom: [1.0, 1.0], ..Default::default() };
        g.config = ctx.gfx.config;
        g.config.rgb = 0;
        g.config.double = 0;
        g.wanted = g.config;
        let w = &mut ctx.wnd.windows[index];
        w.gfx = g;
        let (width, height) = (w.width, w.height);
        let gl = &mut sys.gl;
        gl.glDrawBuffer(g.buffer);
        gl.glReadBuffer(g.buffer);
        gl.glMatrixMode(0x1701);
        gl.glLoadIdentity();
        gl.glMatrixMode(0x1702);
        gl.glLoadIdentity();
        gl.glMatrixMode(0x1700);
        gl.glLoadIdentity();
        gl.glPushAttrib(0x1000);
        gl.glMatrixMode(0x1700);
        gl.glOrtho(-0.5, (x(width) - x(0.5f64)).to_f64(), -0.5, (x(height) - x(0.5f64)).to_f64(), -1.0, 1.0);
        gl.glViewport(0, 0, width, height);
        gl.glScissor(0, 0, width, height);
        gl.glEnable(0xc11);
        gl.glPopAttrib();
        gl.glDisable(0xb71);
        gl.glDisable(0xb24);
        gl.glDisable(0xb42);
        gl.glFinish();
    }

    /// Free the window's drawing state [soft 0x0860f874 GFX_exitWindow].
    pub(crate) fn gfx_exit_window(&mut self, _index: usize) {}

    /// A pixel block for `GFX_fastBlkRead`/`Write`: `width × height × 4` bytes
    /// [soft 0x0860f894 GFX_fastBlkAlloc]. The original returns `malloc`'s block,
    /// uninitialised.
    pub fn gfx_fast_blk_alloc(&mut self, width: i32, height: i32) -> Vec<u8> {
        vec![0; width.wrapping_mul(height).wrapping_mul(4).max(0) as usize]
    }

    /// Free such a block [soft 0x0860f8b4 GFX_fastBlkFree].
    pub fn gfx_fast_blk_free(&mut self, _block: Vec<u8>) {}

    /// The common prologue of the pixel transfers: modes off, both matrices saved and
    /// replaced by a window-sized 2D projection.
    fn gfx_pixel_setup(&mut self, c: usize, with_scissor_off: bool) {
        let (ctx, sys) = st!(self);
        let (width, height) = (ctx.wnd.windows[c].width, ctx.wnd.windows[c].height);
        let gl = &mut sys.gl;
        gl.glDisable(0xbe2);
        gl.glDisable(0xb71);
        gl.glDisable(0xb90);
        if with_scissor_off {
            gl.glDisable(0xc11);
        }
        gl.glMatrixMode(0x1700);
        gl.glPushMatrix();
        gl.glLoadIdentity();
        gl.glMatrixMode(0x1701);
        gl.glPushMatrix();
        gl.glLoadIdentity();
        sys.glu.gluOrtho2D(0.0, width as f64, 0.0, height as f64);
        sys.gl.glViewport(0, 0, width, height);
    }

    /// The common epilogue of the pixel transfers.
    fn gfx_pixel_restore(&mut self) {
        let gl = &mut self.sys.gl;
        gl.glPopMatrix();
        gl.glMatrixMode(0x1700);
        gl.glPopMatrix();
        gl.glPopAttrib();
    }

    /// Read an RGB block (three bytes a pixel, rows not padded) from the front buffer of
    /// the current window [soft 0x0860f8d0 GFX_fastBlkRead].
    ///
    /// The original prints "Stage 2" on the standard error stream on every call (a
    /// leftover), after the `glPushAttrib`.
    pub fn gfx_fast_blk_read(&mut self, x: i32, y: i32, width: i32, height: i32, block: &mut [u8]) {
        self.ctx_get();
        let Some(c) = self.cur() else { return };
        self.sys.gl.glPushAttrib(0x81bf3);
        self.sys.c.fprintf(crate::sys::CStream::Stderr, b"Stage 2\n", &[]);
        self.gfx_pixel_setup(c, true);
        let gl = &mut self.sys.gl;
        gl.glPixelStorei(0xd00, 0);
        gl.glPixelStorei(0xd01, 0);
        gl.glPixelStorei(0xd05, 1);
        gl.glReadBuffer(0x404);
        gl.glReadPixels(x, y, width, height, 0x1907, 0x1401, block);
        self.gfx_pixel_restore();
    }

    /// Draw an RGB block at window position (x, y) [soft 0x0860fa44 GFX_fastBlkWrite].
    pub fn gfx_fast_blk_write(&mut self, x: i32, y: i32, width: i32, height: i32, block: &[u8]) {
        self.ctx_get();
        let Some(c) = self.cur() else { return };
        self.sys.gl.glPushAttrib(0x81bf3);
        self.gfx_pixel_setup(c, true);
        let gl = &mut self.sys.gl;
        gl.glPixelStorei(0xcf0, 0);
        gl.glPixelStorei(0xcf1, 0);
        gl.glPixelStorei(0xcf5, 1);
        gl.glRasterPos2i(x, y);
        gl.glDrawPixels(width, height, 0x1907, 0x1401, block);
        self.gfx_pixel_restore();
    }

    /// A work-around run before every clear when driver fix 4 is on (Mesa): the draw
    /// buffer is switched to another buffer and back [soft 0x0860fb9c gfx_restoreBuffers].
    /// Returns 0 (do not clear) only when the fix is on and the draw buffer is none of
    /// front, back, front-and-back.
    fn gfx_restore_buffers(&mut self) -> i32 {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        if ctx.gl_fixes & FIX_RESTORE_BUFFERS != 0 {
            let mut buffer = [0i32; 1];
            sys.gl.glGetIntegerv(0xc01, &mut buffer);
            let other = match buffer[0] {
                0x404 => 0x405,
                0x405 => 0x404,
                0x408 => {
                    sys.gl.glDrawBuffer(0);
                    sys.gl.glDrawBuffer(0x408);
                    return 1;
                }
                _ => return 0,
            };
            sys.gl.glDrawBuffer(other);
            sys.gl.glDrawBuffer(buffer[0] as u32);
        }
        1
    }

    // ------------------------------------------------------------------ configuration

    /// IRIS GL `cmode`: ask for colour-map mode at the next `gconfig`
    /// [soft 0x0860fc24 cmode].
    pub fn cmode(&mut self) {
        self.ctx_get();
        if let Some(g) = self.gfx_cur() {
            g.wanted.rgb = 0;
        }
    }

    /// IRIS GL `RGBmode` [soft 0x0860fc48 RGBmode].
    pub fn rgbmode(&mut self) {
        self.ctx_get();
        if let Some(g) = self.gfx_cur() {
            g.wanted.rgb = 1;
        }
    }

    /// IRIS GL `acsize`: nothing [soft 0x0860fc6c acsize].
    pub fn acsize(&mut self) {}

    /// IRIS GL `stensize`: remembered, never applied [soft 0x0860fc74 stensize].
    pub fn stensize(&mut self, planes: i32) {
        self.ctx_get();
        if let Some(g) = self.gfx_cur() {
            g.wanted.stencil = planes;
        }
    }

    /// IRIS GL `mssize`: nothing [soft 0x0860fc98 mssize].
    pub fn mssize(&mut self) {}

    /// IRIS GL `zbsize`: remembered, never applied [soft 0x0860fca0 zbsize].
    pub fn zbsize(&mut self, planes: i32) {
        self.ctx_get();
        if let Some(g) = self.gfx_cur() {
            g.wanted.depth = planes;
        }
    }

    /// IRIS GL `RGBsize`: remembered for all four components, never applied
    /// [soft 0x0860fcc4 RGBsize].
    pub fn rgbsize(&mut self, planes: i32) {
        self.ctx_get();
        if let Some(g) = self.gfx_cur() {
            g.wanted.red = planes;
            g.wanted.green = planes;
            g.wanted.blue = planes;
            g.wanted.alpha = planes;
        }
    }

    /// IRIS GL `doublebuffer` [soft 0x0860fcfc doublebuffer].
    pub fn doublebuffer(&mut self) {
        self.ctx_get();
        if let Some(g) = self.gfx_cur() {
            g.wanted.double = 1;
        }
    }

    /// IRIS GL `singlebuffer` [soft 0x0860fd1c singlebuffer].
    pub fn singlebuffer(&mut self) {
        self.ctx_get();
        if let Some(g) = self.gfx_cur() {
            g.wanted.double = 0;
        }
    }

    /// IRIS GL `monobuffer`: nothing [soft 0x0860fd3c monobuffer].
    pub fn monobuffer(&mut self) {}
    /// IRIS GL `stereobuffer`: nothing [soft 0x0860fd44 stereobuffer].
    pub fn stereobuffer(&mut self) {}
    /// IRIS GL `multimap`: nothing [soft 0x0860fd4c multimap].
    pub fn multimap(&mut self) {}
    /// IRIS GL `onemap`: nothing [soft 0x0860fd54 onemap].
    pub fn onemap(&mut self) {}
    /// IRIS GL `overlay`: nothing [soft 0x0860fd5c overlay].
    pub fn overlay(&mut self) {}
    /// IRIS GL `underlay`: nothing [soft 0x0860fd64 underlay].
    pub fn underlay(&mut self) {}

    /// IRIS GL `gconfig`: the wanted colour mode comes into force; if the wanted buffering
    /// differs, it comes into force and drawing and reading go to the back buffer (double)
    /// or the front buffer (single) [soft 0x0860fd6c gconfig]. The wanted bit counts are
    /// ignored.
    pub fn gconfig(&mut self) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        let g = &mut ctx.wnd.windows[c].gfx;
        g.config.rgb = g.wanted.rgb;
        if g.config.double != g.wanted.double {
            g.config.double = g.wanted.double;
            g.buffer = if g.wanted.double == 0 { 0x404 } else { 0x405 };
            sys.gl.glDrawBuffer(g.buffer);
            sys.gl.glReadBuffer(g.buffer);
        }
    }

    // ------------------------------------------------------------------ viewport

    /// IRIS GL `pushviewport`: `glPushAttrib` of the viewport and scissor groups
    /// [soft 0x0860fdcc pushviewport].
    pub fn pushviewport(&mut self) {
        self.sys.gl.glPushAttrib(0x80800);
    }

    /// IRIS GL `popviewport` [soft 0x0860fde4 popviewport].
    pub fn popviewport(&mut self) {
        self.sys.gl.glPopAttrib();
    }

    /// IRIS GL `getviewport`: (left, right, bottom, top) from OpenGL's viewport
    /// [soft 0x0860fdf4 getviewport]. 16-bit arithmetic.
    pub fn getviewport(&mut self) -> (i16, i16, i16, i16) {
        let mut v = [0i32; 4];
        self.sys.gl.glGetIntegerv(0xba2, &mut v);
        let (l, b) = (v[0] as i16, v[1] as i16);
        (l, l.wrapping_add(v[2] as i16).wrapping_sub(1), b, b.wrapping_add(v[3] as i16).wrapping_sub(1))
    }

    /// IRIS GL `reshapeviewport`: viewport and scissor box the whole current window, as
    /// last measured; scissor test on [soft 0x0860fe40 reshapeviewport].
    pub fn reshapeviewport(&mut self) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        let (w, h) = (ctx.wnd.windows[c].width, ctx.wnd.windows[c].height);
        sys.gl.glViewport(0, 0, w, h);
        sys.gl.glScissor(0, 0, w, h);
        sys.gl.glEnable(0xc11);
    }

    /// IRIS GL `viewport(left, right, bottom, top)`: viewport and scissor box, scissor
    /// test on [soft 0x0860fe90 viewport].
    pub fn viewport(&mut self, left: i16, right: i16, bottom: i16, top: i16) {
        let (l, b) = (left as i32, bottom as i32);
        let (w, h) = ((right as i32 - l) + 1, (top as i32 - b) + 1);
        self.sys.gl.glViewport(l, b, w, h);
        self.sys.gl.glScissor(l, b, w, h);
        self.sys.gl.glEnable(0xc11);
    }

    /// IRIS GL `getscrmask`: (left, right, bottom, top) from OpenGL's scissor box
    /// [soft 0x0860fee4 getscrmask].
    pub fn getscrmask(&mut self) -> (i16, i16, i16, i16) {
        let mut v = [0i32; 4];
        self.sys.gl.glGetIntegerv(0xc10, &mut v);
        let (l, b) = (v[0] as i16, v[1] as i16);
        (l, l.wrapping_add(v[2] as i16).wrapping_sub(1), b, b.wrapping_add(v[3] as i16).wrapping_sub(1))
    }

    /// IRIS GL `scrmask(left, right, bottom, top)` [soft 0x0860ff30 scrmask].
    pub fn scrmask(&mut self, left: i16, right: i16, bottom: i16, top: i16) {
        self.sys.gl.glScissor(left as i32, bottom as i32, (right as i32 - left as i32) + 1, (top as i32 - bottom as i32) + 1);
        self.sys.gl.glEnable(0xc11);
    }

    // ------------------------------------------------------------------ projections and matrices

    /// The common start of the four projection calls: save the matrix mode, select the
    /// model-view stack in MSINGLE and the projection stack otherwise, and load the
    /// identity or, while picking, the pick matrix. `false` without a current window.
    fn gfx_begin_projection(&mut self) -> bool {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return false };
        let w = &ctx.wnd.windows[c];
        sys.gl.glPushAttrib(0x1000);
        sys.gl.glMatrixMode(if w.gfx.mmode == 0 { 0x1700 } else { 0x1701 });
        if w.pck.picking == 0 {
            sys.gl.glLoadIdentity();
        } else {
            self.pck_restore_pick_matrix();
        }
        true
    }

    /// IRIS GL `window`: a perspective frustum [soft 0x0860ff68 window].
    pub fn window(&mut self, left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) {
        if self.gfx_begin_projection() {
            self.sys.gl.glFrustum(left as f64, right as f64, bottom as f64, top as f64, near as f64, far as f64);
            self.sys.gl.glPopAttrib();
        }
    }

    /// IRIS GL `ortho` [soft 0x08610018 ortho].
    pub fn ortho(&mut self, left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) {
        if self.gfx_begin_projection() {
            self.sys.gl.glOrtho(left as f64, right as f64, bottom as f64, top as f64, near as f64, far as f64);
            self.sys.gl.glPopAttrib();
        }
    }

    /// IRIS GL `ortho2` [soft 0x086100c8 ortho2].
    pub fn ortho2(&mut self, left: f32, right: f32, bottom: f32, top: f32) {
        if self.gfx_begin_projection() {
            self.sys.glu.gluOrtho2D(left as f64, right as f64, bottom as f64, top as f64);
            self.sys.gl.glPopAttrib();
        }
    }

    /// IRIS GL `perspective(fovy, aspect, near, far)`, `fovy` in tenths of a degree
    /// [soft 0x08610168 perspective]: `gluPerspective(fovy / 10, …)`, the division by the
    /// float 10.
    pub fn perspective(&mut self, fovy: i16, aspect: f32, near: f32, far: f32) {
        if self.gfx_begin_projection() {
            let degrees = (x(fovy as i32) / x(10.0f32)).to_f64();
            self.sys.glu.gluPerspective(degrees, aspect as f64, near as f64, far as f64);
            self.sys.gl.glPopAttrib();
        }
    }

    /// IRIS GL `mmode` [soft 0x0861021c mmode]. Modes above 3 and the mode already set are
    /// ignored.
    ///
    /// Going to or from MSINGLE (0) empties the model-view, projection and texture stacks
    /// in turn: `glPopMatrix` until `glGetError` returns GL_STACK_UNDERFLOW (0x504), then
    /// `glLoadIdentity`. Then the OpenGL matrix mode is set: MPROJECTION (1) projection,
    /// MSINGLE and MVIEWING (2) model-view, MTEXTURE (3) texture.
    pub fn mmode(&mut self, mode: u16) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        if mode > 3 {
            return;
        }
        let g = &mut ctx.wnd.windows[c].gfx;
        let old = g.mmode;
        if old == mode as i32 {
            return;
        }
        if (mode == 0) != (old == 0) {
            for stack in [0x1700, 0x1701, 0x1702] {
                sys.gl.glMatrixMode(stack);
                loop {
                    sys.gl.glPopMatrix();
                    if sys.gl.glGetError() == 0x504 {
                        break;
                    }
                }
                sys.gl.glLoadIdentity();
            }
        }
        match mode {
            1 => sys.gl.glMatrixMode(0x1701),
            0 | 2 => sys.gl.glMatrixMode(0x1700),
            _ => sys.gl.glMatrixMode(0x1702),
        }
        g.mmode = mode as i32;
    }

    /// IRIS GL `getmmode`: −1 without a current window [soft 0x08610350 getmmode].
    pub fn getmmode(&mut self) -> i32 {
        self.ctx_get();
        match self.gfx_cur() {
            Some(g) => g.mmode,
            None => -1,
        }
    }

    /// IRIS GL `loadmatrix` [soft 0x0861037c loadmatrix].
    ///
    /// In MPROJECTION while picking: the pick matrix is restored and the matrix multiplied
    /// onto it. In MSINGLE: the model-view stack is selected first. Otherwise a plain
    /// `glLoadMatrixf`.
    pub fn loadmatrix(&mut self, m: &Matrix) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        let w = &ctx.wnd.windows[c];
        let m = flat(m);
        if w.gfx.mmode == 1 && w.pck.picking != 0 {
            self.pck_restore_pick_matrix();
            self.sys.gl.glMultMatrixf(&m);
        } else if w.gfx.mmode == 0 {
            sys.gl.glMatrixMode(0x1700);
            sys.gl.glLoadMatrixf(&m);
        } else {
            sys.gl.glLoadMatrixf(&m);
        }
    }

    /// IRIS GL `getmatrix`: the projection matrix in MPROJECTION, the texture matrix in
    /// MTEXTURE, else the model-view matrix [soft 0x086103f4 getmatrix]. Without a current
    /// window the matrix is left as it is.
    pub fn getmatrix(&mut self, m: &mut Matrix) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        let pname = match ctx.wnd.windows[c].gfx.mmode {
            1 => 0xba7,
            0 | 2 => 0xba6,
            3 => 0xba8,
            _ => return,
        };
        let mut f = flat(m);
        sys.gl.glGetFloatv(pname, &mut f);
        for (i, r) in m.iter_mut().enumerate() {
            r.copy_from_slice(&f[i * 4..i * 4 + 4]);
        }
    }

    /// IRIS GL `screenspace`: viewport and 2D projection such that coordinates are screen
    /// pixels [soft 0x08610468 screenspace].
    ///
    /// With the window's origin (ox, oy): `viewport(−ox, width − ox, −oy, height − oy)` in
    /// 16-bit arithmetic, then `ortho2(−0.5, width + 0.5, −0.5, height + 0.5)`, width and
    /// height being the screen's. Without a current window the original uses two
    /// uninitialised longs as the origin (0, 0 here).
    pub fn screenspace(&mut self) {
        let (ox, oy) = self.getorigin().unwrap_or((0, 0));
        let h = self.getgdesc(1);
        let top = (h.wrapping_sub(oy)) as i16;
        let bottom = oy.wrapping_neg() as i16;
        let w = self.getgdesc(0);
        self.viewport(ox.wrapping_neg() as i16, (w.wrapping_sub(ox)) as i16, bottom, top);
        let top = (x(self.getgdesc(1)) + x(0.5f64)).to_f32();
        let right = (x(self.getgdesc(0)) + x(0.5f64)).to_f32();
        self.ortho2(-0.5, right, -0.5, top);
    }

    /// IRIS GL `multmatrix` [soft 0x08610514 multmatrix].
    pub fn multmatrix(&mut self, m: &Matrix) {
        self.sys.gl.glMultMatrixf(&flat(m));
    }

    /// IRIS GL `pushmatrix` [soft 0x0861052c pushmatrix].
    pub fn pushmatrix(&mut self) {
        self.sys.gl.glPushMatrix();
    }

    /// IRIS GL `popmatrix` [soft 0x0861053c popmatrix].
    pub fn popmatrix(&mut self) {
        self.sys.gl.glPopMatrix();
    }

    /// IRIS GL `lookat(vx, vy, vz, px, py, pz, twist)`, twist in tenths of a degree
    /// [soft 0x0861054c lookat].
    ///
    /// General case: `glRotated(twist / 10, 0, 0, −1)` then `gluLookAt` with the up vector
    /// (0, 1, 0). When the view direction is vertical (|vx − px| < 0.0001 and |vz − pz| <
    /// 0.0001), where `gluLookAt` has no up vector: `glRotated(−twist / 10, 0, 0, 1)`,
    /// `glRotated(±90, 1, 0, 0)` (+90 when vy > py, or unordered) and
    /// `glTranslatef(−vx, −vy, −vz)`.
    pub fn lookat(&mut self, vx: f32, vy: f32, vz: f32, px: f32, py: f32, pz: f32, twist: i16) {
        let dx = (x(vx) - x(px)).abs();
        let dy = (x(vy) - x(py)).to_f64();
        let dz = (x(vz) - x(pz)).abs();
        let tw = x(twist as i32) * x(0.1f64);
        let eps = x(0.0001f64);
        // the first test passes only for eps > |dx|, the second only for |dz| < eps
        let vertical = eps.gt(dx) && dz.lt(eps);
        if vertical {
            self.sys.gl.glRotated((-tw).to_f64(), 0.0, 0.0, 1.0);
            // 0 < dy, or unordered: +90
            let zero = F80::ZERO;
            let angle = if zero.lt(x(dy)) || zero.unordered(x(dy)) { 90.0 } else { -90.0 };
            self.sys.gl.glRotated(angle, 1.0, 0.0, 0.0);
            self.sys.gl.glTranslatef(-vx, -vy, -vz);
        } else {
            self.sys.gl.glRotated(tw.to_f64(), 0.0, 0.0, -1.0);
            self.sys.glu.gluLookAt(vx as f64, vy as f64, vz as f64, px as f64, py as f64, pz as f64, 0.0, 1.0, 0.0);
        }
    }

    /// IRIS GL `rot(angle, axis)`, degrees; the axis is a character, 'x', 'y' or 'z' in
    /// either case; any other does nothing [soft 0x086106b4 rot].
    pub fn rot(&mut self, angle: f32, axis: u8) {
        match axis {
            b'X' | b'x' => self.sys.gl.glRotatef(angle, 1.0, 0.0, 0.0),
            b'Y' | b'y' => self.sys.gl.glRotatef(angle, 0.0, 1.0, 0.0),
            b'Z' | b'z' => self.sys.gl.glRotatef(angle, 0.0, 0.0, 1.0),
            _ => {}
        }
    }

    /// IRIS GL `rotate(angle, axis)`, tenths of a degree: `rot(angle × 0.1f, axis)`
    /// [soft 0x08610714 rotate].
    pub fn rotate(&mut self, angle: i16, axis: u8) {
        self.rot((x(angle as i32) * x(0.1f32)).to_f32(), axis);
    }

    /// IRIS GL `translate` [soft 0x08610744 translate].
    pub fn translate(&mut self, tx: f32, ty: f32, tz: f32) {
        self.sys.gl.glTranslatef(tx, ty, tz);
    }

    /// IRIS GL `scale` [soft 0x08610774 scale].
    pub fn scale(&mut self, sx: f32, sy: f32, sz: f32) {
        self.sys.gl.glScalef(sx, sy, sz);
    }

    // ------------------------------------------------------------------ inquiries

    /// IRIS GL `getgdesc` [soft 0x086107a4 getgdesc]. By inquiry number:
    ///
    /// | Inquiry | Answer |
    /// |---|---|
    /// | 0, 1 | screen width, height in pixels |
    /// | 2, 3 | screen width, height in millimetres |
    /// | 4, 5 | the depth range's near, far value × (2^(depth bits − 1) − 1), truncated |
    /// | 6, 9 / 7, 10 / 8, 11 | OpenGL's red / green / blue bits |
    /// | 12..=15 | 12 |
    /// | 16 | OpenGL's depth bits |
    /// | 21, 22 | OpenGL's alpha bits |
    /// | 33 | whether OpenGL's logic op is enabled |
    /// | 36, 54 | 8 |
    /// | 39 | −2 |
    /// | 55 | 1 |
    /// | 66 | 67 |
    /// | 72 | OpenGL's stencil bits |
    /// | 73 | OpenGL's maximum number of clip planes |
    /// | anything else | 0 |
    pub fn getgdesc(&mut self, inquiry: i32) -> i32 {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let mut one = [0i32; 1];
        match inquiry {
            0 => sys.x.screen_word(ctx.display, ctx.screen, 0xc) as i32,
            1 => sys.x.screen_word(ctx.display, ctx.screen, 0x10) as i32,
            2 => sys.x.screen_word(ctx.display, ctx.screen, 0x14) as i32,
            3 => sys.x.screen_word(ctx.display, ctx.screen, 0x18) as i32,
            4 | 5 => {
                sys.gl.glGetIntegerv(0xd56, &mut one);
                // only the low byte of the depth bit count is used
                let n = (1i32 << ((one[0] as u8).wrapping_sub(1) & 0x1f)).wrapping_sub(1);
                let mut range = [0.0f32; 2];
                sys.gl.glGetFloatv(0xb70, &mut range);
                let r = if inquiry == 4 { range[0] } else { range[1] };
                (x(n) * x(r)).to_i32(Round::Zero)
            }
            6 | 9 => {
                sys.gl.glGetIntegerv(0xd52, &mut one);
                one[0]
            }
            7 | 10 => {
                sys.gl.glGetIntegerv(0xd53, &mut one);
                one[0]
            }
            8 | 0xb => {
                sys.gl.glGetIntegerv(0xd54, &mut one);
                one[0]
            }
            0xc..=0xf => 0xc,
            0x10 => {
                sys.gl.glGetIntegerv(0xd56, &mut one);
                one[0]
            }
            0x15 | 0x16 => {
                sys.gl.glGetIntegerv(0xd55, &mut one);
                one[0]
            }
            0x21 => sys.gl.glIsEnabled(0xbf1) as i32,
            0x24 | 0x36 => 8,
            0x27 => -2,
            0x37 => 1,
            0x42 => 0x43,
            0x48 => {
                sys.gl.glGetIntegerv(0xd57, &mut one);
                one[0]
            }
            0x49 => {
                sys.gl.glGetIntegerv(0xd32, &mut one);
                one[0]
            }
            _ => 0,
        }
    }

    /// IRIS GL `getgconfig`: the current window's configuration in force
    /// [soft 0x086109a4 getgconfig]. 0 → 12; 1..=4 → red, green, blue, alpha bits; 5 →
    /// depth bits; 7 → 2^(depth bits − 1) − 1; 8 → stencil bits; 16 → double buffering;
    /// anything else, or no window, → 0.
    pub fn getgconfig(&mut self, buffer: i32) -> i32 {
        self.ctx_get();
        let Some(g) = self.gfx_cur() else { return 0 };
        match buffer {
            0 => 0xc,
            1 => g.config.red,
            2 => g.config.green,
            3 => g.config.blue,
            4 => g.config.alpha,
            5 => g.config.depth,
            7 => (1i32 << ((g.config.depth as u8).wrapping_sub(1) & 0x1f)).wrapping_sub(1),
            8 => g.config.stencil,
            0x10 => g.config.double as i32,
            _ => 0,
        }
    }

    // ------------------------------------------------------------------ patterns and line styles

    /// IRIS GL `defpattern(index, size, mask)` [soft 0x08610a28 defpattern]. Index 0 is
    /// ignored. Only size 16 is supported; 32 and anything else print a warning (the
    /// second always says "64 x 64"), but the pattern entry is created all the same.
    ///
    /// The 16 × 16 pattern becomes a 32 × 32 stipple: row i goes to rows i and i + 16, and
    /// all four bytes of a row are **the first byte in memory** of `mask[i]` (the low byte
    /// on this machine): the other half of each 16-bit row is never read.
    pub fn defpattern(&mut self, index: i16, size: i16, mask: &[u16]) {
        self.ctx_get();
        if index == 0 {
            return;
        }
        let gfx = &mut self.ctx.as_deref_mut().unwrap().gfx;
        let at = match gfx.patterns.iter().rposition(|p| p.id == index as i32) {
            Some(i) => i,
            None => {
                // the original appends a record without clearing it (zero here)
                gfx.patterns.push(Pattern { id: index as i32, stipple: [0; 128] });
                gfx.patterns.len() - 1
            }
        };
        if size == 0x10 {
            let p = &mut gfx.patterns[at];
            for i in 0..16 {
                let b = mask[i] as u8;
                p.stipple[i * 4..i * 4 + 4].copy_from_slice(&[b; 4]);
                p.stipple[0x40 + i * 4..0x40 + i * 4 + 4].copy_from_slice(&[b; 4]);
            }
        } else if size == 0x20 {
            self.err_warning(b"defpattern with size 32 x 32 is unsupported.", &[]);
        } else {
            self.err_warning(b"defpattern with size 64 x 64 is unsupported.", &[]);
        }
    }

    /// IRIS GL `setpattern` [soft 0x08610b30 setpattern]: an undefined index (0 included)
    /// turns the polygon stipple off; a defined one turns it on and loads its stipple with
    /// the unpack byte-swap and bit-order settings forced to 0 for the call.
    pub fn setpattern(&mut self, index: i16) {
        self.ctx_get();
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        let g = &mut ctx.wnd.windows[c].gfx;
        match ctx.gfx.patterns.iter().rev().find(|p| p.id == index as i32) {
            None => {
                sys.gl.glDisable(0xb42);
                g.pattern = 0;
            }
            Some(p) => {
                g.pattern = p.id;
                sys.gl.glEnable(0xb42);
                let (mut swap, mut lsb) = ([0i32; 1], [0i32; 1]);
                sys.gl.glGetIntegerv(0xcf0, &mut swap);
                sys.gl.glGetIntegerv(0xcf1, &mut lsb);
                sys.gl.glPixelStorei(0xcf0, 0);
                sys.gl.glPixelStorei(0xcf1, 0);
                sys.gl.glPolygonStipple(&p.stipple);
                sys.gl.glPixelStorei(0xcf0, swap[0]);
                sys.gl.glPixelStorei(0xcf1, lsb[0]);
            }
        }
    }

    /// IRIS GL `getpattern` [soft 0x08610c3c getpattern].
    pub fn getpattern(&mut self) -> i32 {
        self.ctx_get();
        self.gfx_cur().map_or(0, |g| g.pattern)
    }

    /// IRIS GL `deflinestyle(index, pattern)` [soft 0x08610c64 deflinestyle]. Index 0 is
    /// ignored.
    pub fn deflinestyle(&mut self, index: i16, pattern: u16) {
        self.ctx_get();
        if index == 0 {
            return;
        }
        let gfx = &mut self.ctx.as_deref_mut().unwrap().gfx;
        let at = match gfx.line_styles.iter().rposition(|s| s.id == index as i32) {
            Some(i) => i,
            None => {
                gfx.line_styles.push(LineStyle::default());
                gfx.line_styles.len() - 1
            }
        };
        gfx.line_styles[at] = LineStyle { id: index as i32, pattern };
    }

    /// IRIS GL `setlinestyle` [soft 0x08610d04 setlinestyle]: an undefined index turns the
    /// line stipple off; a defined one turns it on with the current repeat factor.
    pub fn setlinestyle(&mut self, index: i16) {
        self.ctx_get();
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        let g = &mut ctx.wnd.windows[c].gfx;
        match ctx.gfx.line_styles.iter().rev().find(|s| s.id == index as i32) {
            None => {
                sys.gl.glDisable(0xb24);
                g.line_style = 0;
            }
            Some(s) => {
                g.line_style = s.id;
                sys.gl.glEnable(0xb24);
                sys.gl.glLineStipple(g.ls_repeat, s.pattern);
            }
        }
    }

    /// IRIS GL `getlstyle` [soft 0x08610db0 getlstyle].
    pub fn getlstyle(&mut self) -> i32 {
        self.ctx_get();
        self.gfx_cur().map_or(0, |g| g.line_style)
    }

    /// IRIS GL `lsrepeat`: takes effect at the next `setlinestyle`
    /// [soft 0x08610dd4 lsrepeat].
    pub fn lsrepeat(&mut self, factor: i32) {
        self.ctx_get();
        if let Some(g) = self.gfx_cur() {
            g.ls_repeat = factor;
        }
    }

    /// IRIS GL `getlsrepeat` [soft 0x08610df8 getlsrepeat].
    pub fn getlsrepeat(&mut self) -> i32 {
        self.ctx_get();
        self.gfx_cur().map_or(0, |g| g.ls_repeat)
    }

    // ------------------------------------------------------------------ colour

    /// IRIS GL `color(index)` [soft 0x08610e1c color].
    ///
    /// The index is limited to 0xfff and remembered; the colour-map entry `index &
    /// writemask` is issued as `glColor4d(r / 255, g / 255, b / 255, 1)` and remembered as
    /// the current colour with alpha 255.
    pub fn color(&mut self, index: u16) {
        self.ctx_get();
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        let g = &mut ctx.wnd.windows[c].gfx;
        let index = index.min(0xfff);
        g.color = index;
        let at = (index & g.writemask) as usize * 3;
        let (r, gr, b) = (ctx.gfx.cmap[at] as i32, ctx.gfx.cmap[at + 1] as i32, ctx.gfx.cmap[at + 2] as i32);
        sys.gl.glColor4d(int_over(r, 255.0), int_over(gr, 255.0), int_over(b, 255.0), 1.0);
        g.rgba = [r, gr, b, 0xff];
    }

    /// IRIS GL `colorf`: `color` of the value truncated to an integer, low 16 bits
    /// [soft 0x08610ef0 colorf].
    pub fn colorf(&mut self, index: f32) {
        self.color(x(index).to_i32(Round::Zero) as u16);
    }

    /// IRIS GL `getcolor` [soft 0x08610f28 getcolor].
    pub fn getcolor(&mut self) -> u16 {
        self.ctx_get();
        self.gfx_cur().map_or(0, |g| g.color)
    }

    /// IRIS GL `writemask` [soft 0x08610f4c writemask]: only in normal draw mode; in
    /// colour-map mode the current colour index is issued again under the new mask.
    pub fn writemask(&mut self, mask: u16) {
        self.ctx_get();
        let Some(g) = self.gfx_cur() else { return };
        if g.drawmode == 0x10 {
            g.writemask = mask;
            if g.config.rgb == 0 {
                let index = g.color;
                self.color(index);
            }
        }
    }

    /// IRIS GL `getwritemask`: 0xfff without a window [soft 0x08610f90 getwritemask].
    pub fn getwritemask(&mut self) -> u16 {
        self.ctx_get();
        self.gfx_cur().map_or(0xfff, |g| g.writemask)
    }

    /// IRIS GL `mapcolor(index, r, g, b)`: change a colour-map entry
    /// [soft 0x08610fb8 mapcolor]. Index limited to 0xfff, components to 0..=255. Ignored
    /// while the effective draw mode is 0x100.
    pub fn mapcolor(&mut self, index: u16, red: i16, green: i16, blue: i16) {
        self.ctx_get();
        self.ctx_get();
        let (ctx, _) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        if ctx.wnd.windows[c].gfx.drawmode_effective == 0x100 {
            return;
        }
        let at = index.min(0xfff) as usize * 3;
        ctx.gfx.cmap[at] = red.clamp(0, 0xff) as u8;
        ctx.gfx.cmap[at + 1] = green.clamp(0, 0xff) as u8;
        ctx.gfx.cmap[at + 2] = blue.clamp(0, 0xff) as u8;
    }

    /// IRIS GL `getmcolor`: a colour-map entry, or zeros while the effective draw mode is
    /// 0x100 [soft 0x0861107c getmcolor]. `None` without a window.
    pub fn getmcolor(&mut self, index: u16) -> Option<(u16, u16, u16)> {
        self.ctx_get();
        self.ctx_get();
        let (ctx, _) = st!(self);
        let c = ctx.wnd.current?;
        if ctx.wnd.windows[c].gfx.drawmode_effective == 0x100 {
            return Some((0, 0, 0));
        }
        let at = index.min(0xfff) as usize * 3;
        Some((ctx.gfx.cmap[at] as u16, ctx.gfx.cmap[at + 1] as u16, ctx.gfx.cmap[at + 2] as u16))
    }

    /// IRIS GL `cpack`: a packed colour, red in the low byte, alpha in the high byte
    /// [soft 0x08611104 cpack].
    pub fn cpack(&mut self, packed: u32) {
        self.ctx_get();
        let Some(g) = self.gfx_cur() else { return };
        let (r, gr, b, a) = ((packed & 0xff) as i32, (packed >> 8 & 0xff) as i32, (packed >> 16 & 0xff) as i32, (packed >> 24) as i32);
        g.rgba = [r, gr, b, a];
        self.sys.gl.glColor4d(int_over(r, 255.0), int_over(gr, 255.0), int_over(b, 255.0), int_over(a, 255.0));
    }

    /// IRIS GL `RGBcolor` [soft 0x086111a4 RGBcolor]. Not clamped.
    pub fn rgbcolor(&mut self, red: i16, green: i16, blue: i16) {
        self.ctx_get();
        let Some(g) = self.gfx_cur() else { return };
        g.rgba = [red as i32, green as i32, blue as i32, 0xff];
        self.sys.gl.glColor4d(int_over(red as i32, 255.0), int_over(green as i32, 255.0), int_over(blue as i32, 255.0), 1.0);
    }

    /// IRIS GL `gRGBcolor`: the current colour, each component's low 16 bits
    /// [soft 0x0861123c gRGBcolor]. `None` without a window.
    pub fn grgbcolor(&mut self) -> Option<(i16, i16, i16)> {
        self.ctx_get();
        let g = self.gfx_cur()?;
        Some((g.rgba[0] as i16, g.rgba[1] as i16, g.rgba[2] as i16))
    }

    /// IRIS GL `lRGBrange(rmin, gmin, bmin, rmax, gmax, bmax, znear, zfar)`: depth-cue
    /// colours and range [soft 0x0861127c lRGBrange].
    ///
    /// The minimum colour becomes the fog colour, the maximum colour the current OpenGL
    /// colour (the layer's own current colour is not updated). Fog start and end are
    /// `znear + q` and `zfar + q` with `q = (zfar − znear) / 4`, `zfar` first raised by one
    /// if the two are equal.
    pub fn lrgbrange(&mut self, rmin: i16, gmin: i16, bmin: i16, rmax: i16, gmax: i16, bmax: i16, znear: i32, zfar: i32) {
        let f = |v: i16| int_over_f32(v as i32, 255.0);
        self.sys.gl.glFogfv(0xb66, &[f(rmin), f(gmin), f(bmin), 1.0]);
        self.sys.gl.glColor4f(f(rmax), f(gmax), f(bmax), 1.0);
        let zfar = if zfar == znear { zfar.wrapping_add(1) } else { zfar };
        let d = zfar.wrapping_sub(znear);
        let q = (if d < 0 { d.wrapping_add(3) } else { d }) >> 2;
        self.sys.gl.glFogi(0xb63, znear.wrapping_add(q));
        self.sys.gl.glFogi(0xb64, q.wrapping_add(zfar));
    }

    /// Whether there is a current window in RGB mode (the guard of the `c…` calls).
    fn gfx_rgb(&mut self) -> bool {
        self.ctx_get();
        self.gfx_cur().is_some_and(|g| g.config.rgb == 1)
    }

    /// IRIS GL `c3f`: ignored outside RGB mode, like all six `c…` calls
    /// [soft 0x0861137c c3f].
    pub fn c3f(&mut self, c: &[f32; 3]) {
        if self.gfx_rgb() {
            self.sys.gl.glColor3f(c[0], c[1], c[2]);
        }
    }

    /// IRIS GL `c3i`: components 0..=255 [soft 0x086113c8 c3i].
    pub fn c3i(&mut self, c: &[i32; 3]) {
        if self.gfx_rgb() {
            self.sys.gl.glColor4d(int_over(c[0], 255.0), int_over(c[1], 255.0), int_over(c[2], 255.0), 1.0);
        }
    }

    /// IRIS GL `c3s` [soft 0x0861143c c3s].
    pub fn c3s(&mut self, c: &[i16; 3]) {
        if self.gfx_rgb() {
            self.sys.gl.glColor4d(int_over(c[0] as i32, 255.0), int_over(c[1] as i32, 255.0), int_over(c[2] as i32, 255.0), 1.0);
        }
    }

    /// IRIS GL `c4f` [soft 0x086114b0 c4f].
    pub fn c4f(&mut self, c: &[f32; 4]) {
        if self.gfx_rgb() {
            self.sys.gl.glColor4f(c[0], c[1], c[2], c[3]);
        }
    }

    /// IRIS GL `c4i` [soft 0x08611504 c4i].
    pub fn c4i(&mut self, c: &[i32; 4]) {
        if self.gfx_rgb() {
            self.sys.gl.glColor4d(int_over(c[0], 255.0), int_over(c[1], 255.0), int_over(c[2], 255.0), int_over(c[3], 255.0));
        }
    }

    /// IRIS GL `c4s` [soft 0x08611584 c4s].
    pub fn c4s(&mut self, c: &[i16; 4]) {
        if self.gfx_rgb() {
            self.sys.gl.glColor4d(int_over(c[0] as i32, 255.0), int_over(c[1] as i32, 255.0), int_over(c[2] as i32, 255.0), int_over(c[3] as i32, 255.0));
        }
    }

    // ------------------------------------------------------------------ buffers

    /// IRIS GL `drawmode` [soft 0x08611608 drawmode].
    ///
    /// NORMALDRAW (0x10) and 0x100 are "normal": drawing returns to the window's buffer.
    /// Any other mode (overlay, underlay, pop-up planes, cursor) has no planes of its own
    /// here: drawing and reading are switched to the front buffer, and `getdrawmode`
    /// keeps answering NORMALDRAW.
    pub fn drawmode(&mut self, mode: i32) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        let g = &mut ctx.wnd.windows[c].gfx;
        if g.drawmode == mode {
            return;
        }
        if mode == 0x10 || mode == 0x100 {
            if g.overlay != 0 {
                sys.gl.glDrawBuffer(g.buffer);
                sys.gl.glReadBuffer(g.buffer);
                g.overlay = 0;
            }
            g.drawmode_effective = mode;
        } else {
            sys.gl.glDrawBuffer(0x404);
            sys.gl.glReadBuffer(0x404);
            g.overlay = 1;
            g.drawmode_effective = 0x10;
        }
        g.drawmode = mode;
    }

    /// IRIS GL `getdrawmode`: NORMALDRAW without a window [soft 0x086116b0 getdrawmode].
    pub fn getdrawmode(&mut self) -> i32 {
        self.ctx_get();
        self.gfx_cur().map_or(0x10, |g| g.drawmode_effective)
    }

    /// The guard of `backbuffer`, `frontbuffer`: a double-buffered window not drawing in
    /// the front buffer for another draw mode.
    fn gfx_buffer_window(&mut self) -> Option<usize> {
        self.ctx_get();
        let ctx = self.ctx.as_deref()?;
        let c = ctx.wnd.current?;
        let g = &ctx.wnd.windows[c].gfx;
        (g.config.double == 1 && g.overlay == 0).then_some(c)
    }

    /// IRIS GL `backbuffer(enable)`: add the back buffer to, or remove it from, the
    /// buffers drawn into [soft 0x086116dc backbuffer]. Only for a double-buffered window
    /// in a normal draw mode. `glFinish` first; draw and read buffer follow together.
    pub fn backbuffer(&mut self, enable: i32) {
        let Some(c) = self.gfx_buffer_window() else { return };
        let (ctx, sys) = st!(self);
        let g = &mut ctx.wnd.windows[c].gfx;
        sys.gl.glFinish();
        if enable == 0 {
            if g.buffer == 0x405 {
                g.buffer = 0;
            }
            if g.buffer == 0x408 {
                g.buffer = 0x404;
            }
        } else {
            if g.buffer == 0 {
                g.buffer = 0x405;
            }
            if g.buffer == 0x404 {
                g.buffer = 0x408;
            }
        }
        sys.gl.glDrawBuffer(g.buffer);
        sys.gl.glReadBuffer(g.buffer);
    }

    /// IRIS GL `frontbuffer(enable)` [soft 0x08611770 frontbuffer].
    pub fn frontbuffer(&mut self, enable: i32) {
        let Some(c) = self.gfx_buffer_window() else { return };
        let (ctx, sys) = st!(self);
        let g = &mut ctx.wnd.windows[c].gfx;
        sys.gl.glFinish();
        if enable == 0 {
            if g.buffer == 0x404 {
                g.buffer = 0;
            }
            if g.buffer == 0x408 {
                g.buffer = 0x405;
            }
        } else {
            if g.buffer == 0 {
                g.buffer = 0x404;
            }
            if g.buffer == 0x405 {
                g.buffer = 0x408;
            }
        }
        sys.gl.glDrawBuffer(g.buffer);
        sys.gl.glReadBuffer(g.buffer);
    }

    /// IRIS GL `swapbuffers`: only for a double-buffered window in a normal draw mode
    /// [soft 0x08611804 swapbuffers].
    pub fn swapbuffers(&mut self) {
        self.ctx_get();
        if let Some(c) = self.gfx_buffer_window() {
            let (ctx, sys) = st!(self);
            sys.glx.glXSwapBuffers(ctx.display, ctx.wnd.windows[c].xwindow);
        }
    }

    /// IRIS GL `mswapbuffers`: `swapbuffers` if the mask has NORMALDRAW (0x10)
    /// [soft 0x0861184c mswapbuffers].
    pub fn mswapbuffers(&mut self, mask: i32) {
        if mask & 0x10 != 0 {
            self.swapbuffers();
        }
    }

    /// IRIS GL `getbuffer`: 1 back, 2 front, 3 both, 0 none or no window
    /// [soft 0x08611864 getbuffer].
    pub fn getbuffer(&mut self) -> i32 {
        self.ctx_get();
        match self.gfx_cur().map(|g| g.buffer) {
            Some(0x405) => 1,
            Some(0x404) => 2,
            Some(0x408) => 3,
            _ => 0,
        }
    }

    /// IRIS GL `gsync`: wait for the vertical retrace through `glXWaitVideoSyncSGI`
    /// [soft 0x086118bc gsync].
    ///
    /// Nothing with driver fix 1 (Mesa). The first call looks for "GLX_SGI_video_sync" and
    /// "GLX_ARB_get_proc_address" in the client's GLX extension string and remembers the
    /// outcome; the function's address is fetched once with `glXGetProcAddressARB` and
    /// called as (1, 0, &count).
    pub fn gsync(&mut self) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        if ctx.gl_fixes & FIX_NO_GSYNC != 0 || self.has_sync == 0 {
            return;
        }
        if self.has_sync == -1 {
            // the original hands the result to strstr unchecked
            let ext = sys.glx.glXGetClientString(ctx.display, 3).unwrap_or_default();
            let has = |needle: &[u8]| ext.windows(needle.len()).any(|w| w == needle);
            if !has(b"GLX_SGI_video_sync") || !has(b"GLX_ARB_get_proc_address") {
                self.has_sync = 0;
                return;
            }
            self.has_sync = 1;
        }
        if self.has_sync == 1 {
            if self.wait_func == 0 {
                self.wait_func = sys.glx.glXGetProcAddressARB(b"glXWaitVideoSyncSGI");
                if self.wait_func == 0 {
                    return;
                }
            }
            sys.glx.glXWaitVideoSyncSGI(1, 0);
        }
    }

    /// IRIS GL `clear`: clear the colour buffer to the current colour, in normal draw mode
    /// only [soft 0x08611984 clear].
    pub fn clear(&mut self) {
        self.ctx_get();
        let Some(g) = self.gfx_cur() else { return };
        if g.drawmode != 0x10 {
            return;
        }
        let rgba = g.rgba;
        if self.gfx_restore_buffers() != 0 {
            let f = |v: i32| int_over_f32(v, 255.0);
            self.sys.gl.glClearColor(f(rgba[0]), f(rgba[1]), f(rgba[2]), f(rgba[3]));
            self.sys.gl.glClear(0x4000);
        }
    }

    /// IRIS GL `rectzoom` [soft 0x08611a1c rectzoom].
    pub fn rectzoom(&mut self, xfactor: f32, yfactor: f32) {
        self.ctx_get();
        if let Some(g) = self.gfx_cur() {
            g.zoom = [xfactor, yfactor];
        }
    }

    // ------------------------------------------------------------------ pixels

    /// IRIS GL `lrectwrite(x1, y1, x2, y2, pixels)`: draw a block of packed RGBA longs with
    /// its lower left corner at window pixel (x1, y1) [soft 0x08611a50 lrectwrite].
    ///
    /// Blending, depth test and stencil test off, a window-sized 2D projection, the
    /// scissor box set to the viewport that was in force, the pixel zoom of `rectzoom`.
    pub fn lrectwrite(&mut self, x1: i16, y1: i16, x2: i16, y2: i16, pixels: &[u8]) {
        self.ctx_get();
        let Some(c) = self.cur() else { return };
        let mut vp = [0i32; 4];
        self.sys.gl.glGetIntegerv(0xba2, &mut vp);
        self.sys.gl.glPushAttrib(0x81bf3);
        self.gfx_pixel_setup(c, false);
        let zoom = self.ctx.as_deref().unwrap().wnd.windows[c].gfx.zoom;
        let gl = &mut self.sys.gl;
        gl.glEnable(0xc11);
        gl.glScissor(vp[0], vp[1], vp[2], vp[3]);
        gl.glPixelZoom(zoom[0], zoom[1]);
        gl.glRasterPos2i(x1 as i32, y1 as i32);
        gl.glDrawPixels((x2 as i32 - x1 as i32) + 1, (y2 as i32 - y1 as i32) + 1, 0x1908, 0x1401, pixels);
        self.gfx_pixel_restore();
    }

    /// IRIS GL `lrectread`: read a block of packed RGBA longs; returns the number of
    /// pixels, 0 without a window [soft 0x08611bf8 lrectread].
    pub fn lrectread(&mut self, x1: i16, y1: i16, x2: i16, y2: i16, pixels: &mut [u8]) -> i32 {
        self.ctx_get();
        let mut w = (x2 as i32 - x1 as i32) + 1;
        let h = (y2 as i32 - y1 as i32) + 1;
        match self.cur() {
            None => w = 0,
            Some(c) => {
                let mut vp = [0i32; 4];
                self.sys.gl.glGetIntegerv(0xba2, &mut vp);
                self.sys.gl.glPushAttrib(0x81bf3);
                self.gfx_pixel_setup(c, false);
                self.sys.gl.glReadPixels(x1 as i32, y1 as i32, w, h, 0x1908, 0x1401, pixels);
                self.gfx_pixel_restore();
            }
        }
        w.wrapping_mul(h)
    }

    /// IRIS GL `readdisplay(x1, y1, x2, y2, pixels, hints)`: read screen pixels; here they
    /// come from the current window's front buffer, the screen coordinates shifted by the
    /// window's stored X position [soft 0x08611d54 readdisplay]. A non-zero `hints` sets
    /// every pixel's fourth byte to 0xff. Returns 1.
    ///
    /// The original does not test for a current window (it would fault without one).
    pub fn readdisplay(&mut self, x1: i16, y1: i16, x2: i16, y2: i16, pixels: &mut [u8], hints: i32) -> i32 {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let c = ctx.wnd.current.expect("readdisplay: the original dereferences the null current window");
        sys.gl.glFlush();
        let (left, right) = if x2 < x1 { (x2, x1) } else { (x1, x2) };
        let (bottom, top) = if y2 < y1 { (y2, y1) } else { (y1, y2) };
        let w = (right as i32 - left as i32) + 1;
        let h = (top as i32 - bottom as i32) + 1;
        let (wx, wy) = (ctx.wnd.windows[c].x as i16, ctx.wnd.windows[c].y as i16);
        sys.gl.glPushAttrib(0x20);
        sys.gl.glReadBuffer(0x404);
        sys.gl.glReadPixels(left.wrapping_sub(wx) as i32, bottom.wrapping_sub(wy) as i32, w, h, 0x1908, 0x1401, pixels);
        if hints != 0 {
            let n = w.wrapping_mul(h);
            for i in 0..n.max(0) as usize {
                // the original writes without knowing the buffer's size
                if let Some(a) = pixels.get_mut(i * 4 + 3) {
                    *a = 0xff;
                }
            }
        }
        sys.gl.glPopAttrib();
        1
    }

    // ------------------------------------------------------------------ rectangles

    /// A screen-aligned box as four vertices, outline (`mode` 2) or filled (9)
    /// [soft 0x08611e58 g_sbox].
    ///
    /// The corner is the smaller x and y, the size the absolute differences. For a filled
    /// box, driver fix 2 lowers y by one, fix 8 lowers x and y by one. Vertices: corner,
    /// up, up-right, right.
    fn g_sbox(&mut self, mode: u32, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.ctx_get();
        let fixes = self.ctx.as_deref().unwrap().gl_fixes;
        // x1 replaces x2 only when x1 < x2 (an unordered pair keeps x2)
        let mut bx = if x1 < x2 { x1 } else { x2 };
        let mut by = if y1 < y2 { y1 } else { y2 };
        let w = (x2 - x1).abs();
        let h = (y2 - y1).abs();
        if mode == 9 {
            if fixes & FIX_BOX_Y != 0 {
                by -= 1.0;
            } else if fixes & FIX_BOX_XY != 0 {
                bx -= 1.0;
                by -= 1.0;
            }
        }
        let gl = &mut self.sys.gl;
        gl.glBegin(mode);
        gl.glVertex2f(bx, by);
        let ty = by + h;
        gl.glVertex2f(bx, ty);
        let rx = bx + w;
        gl.glVertex2f(rx, ty);
        gl.glVertex2f(rx, by);
        gl.glEnd();
    }

    /// A line loop through the four corners, doubles [soft 0x08611f5c g_rect].
    fn g_rect(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        let gl = &mut self.sys.gl;
        gl.glBegin(2);
        gl.glVertex2d(x1, y1);
        gl.glVertex2d(x1, y2);
        gl.glVertex2d(x2, y2);
        gl.glVertex2d(x2, y1);
        gl.glEnd();
    }

    /// A polygon through the four corners, rounded to floats [soft 0x08611fc0 g_rectf].
    fn g_rectf(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        let gl = &mut self.sys.gl;
        gl.glBegin(9);
        gl.glVertex2f(x1 as f32, y1 as f32);
        gl.glVertex2f(x1 as f32, y2 as f32);
        gl.glVertex2f(x2 as f32, y2 as f32);
        gl.glVertex2f(x2 as f32, y1 as f32);
        gl.glEnd();
    }

    /// The frame of the six `sbox…` calls: the polygon mode is read (two values, preset to
    /// 0), forced to lines or fill for both faces, and put back per face.
    fn sbox_any(&mut self, fill: bool, x1: f32, y1: f32, x2: f32, y2: f32) {
        let mut mode = [0i32; 2];
        self.sys.gl.glGetIntegerv(0xb40, &mut mode);
        self.sys.gl.glPolygonMode(0x408, if fill { 0x1b02 } else { 0x1b01 });
        self.g_sbox(if fill { 9 } else { 2 }, x1, y1, x2, y2);
        self.sys.gl.glPolygonMode(0x404, mode[0] as u32);
        self.sys.gl.glPolygonMode(0x405, mode[1] as u32);
    }

    /// IRIS GL `sbox` [soft 0x08612030 sbox].
    pub fn sbox(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.sbox_any(false, x1, y1, x2, y2);
    }
    /// IRIS GL `sboxi` [soft 0x086120b0 sboxi].
    pub fn sboxi(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.sbox_any(false, x1 as f32, y1 as f32, x2 as f32, y2 as f32);
    }
    /// IRIS GL `sboxs` [soft 0x08612158 sboxs].
    pub fn sboxs(&mut self, x1: i16, y1: i16, x2: i16, y2: i16) {
        self.sbox_any(false, x1 as f32, y1 as f32, x2 as f32, y2 as f32);
    }
    /// IRIS GL `rect` [soft 0x08612210 rect].
    pub fn rect(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.g_rect(x1 as f64, y1 as f64, x2 as f64, y2 as f64);
    }
    /// IRIS GL `recti` [soft 0x08612248 recti].
    pub fn recti(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.g_rect(x1 as f64, y1 as f64, x2 as f64, y2 as f64);
    }
    /// IRIS GL `rects` [soft 0x08612298 rects].
    pub fn rects(&mut self, x1: i16, y1: i16, x2: i16, y2: i16) {
        self.g_rect(x1 as f64, y1 as f64, x2 as f64, y2 as f64);
    }
    /// IRIS GL `sboxf` [soft 0x086122ec sboxf].
    pub fn sboxf(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.sbox_any(true, x1, y1, x2, y2);
    }
    /// IRIS GL `sboxfi` [soft 0x0861236c sboxfi].
    pub fn sboxfi(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.sbox_any(true, x1 as f32, y1 as f32, x2 as f32, y2 as f32);
    }
    /// IRIS GL `sboxfs` [soft 0x08612414 sboxfs].
    pub fn sboxfs(&mut self, x1: i16, y1: i16, x2: i16, y2: i16) {
        self.sbox_any(true, x1 as f32, y1 as f32, x2 as f32, y2 as f32);
    }
    /// IRIS GL `rectf` [soft 0x086124cc rectf].
    pub fn rectf(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.g_rectf(x1 as f64, y1 as f64, x2 as f64, y2 as f64);
    }
    /// IRIS GL `rectfi` [soft 0x08612504 rectfi].
    pub fn rectfi(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.g_rectf(x1 as f64, y1 as f64, x2 as f64, y2 as f64);
    }
    /// IRIS GL `rectfs` [soft 0x08612554 rectfs].
    pub fn rectfs(&mut self, x1: i16, y1: i16, x2: i16, y2: i16) {
        self.g_rectf(x1 as f64, y1 as f64, x2 as f64, y2 as f64);
    }

    // ------------------------------------------------------------------ arcs and circles

    /// An arc from `start` to `end` (tenths of a degree, counter-clockwise from the
    /// positive x axis); nothing unless the radius is above 0 [soft 0x086125a8 g_arc].
    ///
    /// Outline: a line strip of |end − start| points, one every tenth of a degree, the
    /// first at `start` and none at `end` itself. The angle variable starts at π/2 −
    /// start·π/1800 and steps by ±π/1800; a point is (x − r·cos, y + r·sin) of it.
    ///
    /// Filled: `gluPartialDisk` at the translated origin with inner radius 0, one loop,
    /// `sweep / 45` slices, start angle `(900 − end) / 10` degrees (raised by 360 until
    /// not negative) and sweep `sweep / 10` degrees, where `sweep` is `end − start` as a
    /// 16-bit value raised by 3600 until not negative.
    fn g_arc(&mut self, cx: f64, cy: f64, radius: f64, start: i32, end: i32, fill: i32) {
        if !F80::ZERO.lt(x(radius)) {
            return;
        }
        if fill == 0 {
            let mut angle = (x(std::f64::consts::FRAC_PI_2) - (x(start) * x(std::f64::consts::PI)) / x(1800.0f64)).to_f64();
            let n = end.wrapping_sub(start).wrapping_abs();
            let step = if end < start { -0.0017453292519943296f64 } else { 0.0017453292519943296f64 };
            self.sys.gl.glBegin(3);
            for _ in 0..n.max(0) {
                let py = (self.sys.m.sin(angle) * x(radius) + x(cy)).to_f64();
                let px = (x(cx) - self.sys.m.cos(angle) * x(radius)).to_f64();
                self.sys.gl.glVertex2d(px, py);
                angle = (x(angle) + x(step)).to_f64();
            }
            self.sys.gl.glEnd();
        } else {
            let quad = self.sys.glu.gluNewQuadric();
            self.sys.gl.glPushAttrib(0x1000);
            self.sys.gl.glMatrixMode(0x1700);
            self.sys.gl.glPushMatrix();
            self.sys.gl.glTranslatef(cx as f32, cy as f32, 0.0);
            let mut sweep = end.wrapping_sub(start) as i16;
            while sweep < 0 {
                sweep = sweep.wrapping_add(0xe10);
            }
            let mut first = 900i32.wrapping_sub(end);
            while first < 0 {
                first = first.wrapping_add(0xe10);
            }
            self.sys.glu.gluPartialDisk(quad, 0.0, radius, sweep as i32 / 0x2d, 1, int_over(first, 10.0), int_over(sweep as i32, 10.0));
            self.sys.glu.gluDeleteQuadric(quad);
            self.sys.gl.glPopMatrix();
            self.sys.gl.glPopAttrib();
        }
    }

    /// IRIS GL `arc` [soft 0x08612774 arc].
    pub fn arc(&mut self, cx: f32, cy: f32, radius: f32, start: i16, end: i16) {
        self.g_arc(cx as f64, cy as f64, radius as f64, start as i32, end as i32, 0);
    }
    /// IRIS GL `arci` [soft 0x086127b0 arci].
    pub fn arci(&mut self, cx: i32, cy: i32, radius: i32, start: i16, end: i16) {
        self.arc(cx as f32, cy as f32, radius as f32, start, end);
    }
    /// IRIS GL `arcs` [soft 0x08612800 arcs].
    pub fn arcs(&mut self, cx: i16, cy: i16, radius: i16, start: i16, end: i16) {
        self.arc(cx as f32, cy as f32, radius as f32, start, end);
    }
    /// IRIS GL `arcf` [soft 0x08612850 arcf].
    pub fn arcf(&mut self, cx: f32, cy: f32, radius: f32, start: i16, end: i16) {
        self.g_arc(cx as f64, cy as f64, radius as f64, start as i32, end as i32, 1);
    }
    /// IRIS GL `arcfi` [soft 0x0861288c arcfi].
    pub fn arcfi(&mut self, cx: i32, cy: i32, radius: i32, start: i16, end: i16) {
        self.arcf(cx as f32, cy as f32, radius as f32, start, end);
    }
    /// IRIS GL `arcfs` [soft 0x086128dc arcfs].
    pub fn arcfs(&mut self, cx: i16, cy: i16, radius: i16, start: i16, end: i16) {
        self.arcf(cx as f32, cy as f32, radius as f32, start, end);
    }

    /// A circle as 80 vertices, a line loop or (filled) a polygon; nothing unless the
    /// radius is above 0 [soft 0x0861292c g_circ]. Vertex k is at angle k·π/40, the angle
    /// accumulated by adding the double 0.07853981633974483.
    fn g_circ(&mut self, cx: f64, cy: f64, radius: f64, fill: i32) {
        if !F80::ZERO.lt(x(radius)) {
            return;
        }
        let mut angle = 0.0f64;
        self.sys.gl.glBegin(if fill == 0 { 2 } else { 9 });
        for _ in 0..0x50 {
            let py = (self.sys.m.sin(angle) * x(radius) + x(cy)).to_f64();
            let px = (self.sys.m.cos(angle) * x(radius) + x(cx)).to_f64();
            self.sys.gl.glVertex2d(px, py);
            angle = (x(angle) + x(0.07853981633974483f64)).to_f64();
        }
        self.sys.gl.glEnd();
    }

    /// IRIS GL `circ` [soft 0x086129cc circ].
    pub fn circ(&mut self, cx: f32, cy: f32, radius: f32) {
        self.g_circ(cx as f64, cy as f64, radius as f64, 0);
    }
    /// IRIS GL `circi` [soft 0x086129fc circi].
    pub fn circi(&mut self, cx: i32, cy: i32, radius: i32) {
        self.circ(cx as f32, cy as f32, radius as f32);
    }
    /// IRIS GL `circs` [soft 0x08612a3c circs].
    pub fn circs(&mut self, cx: i16, cy: i16, radius: i16) {
        self.circ(cx as f32, cy as f32, radius as f32);
    }
    /// IRIS GL `circf` [soft 0x08612a80 circf].
    pub fn circf(&mut self, cx: f32, cy: f32, radius: f32) {
        self.g_circ(cx as f64, cy as f64, radius as f64, 1);
    }
    /// IRIS GL `circfi` [soft 0x08612ab0 circfi].
    pub fn circfi(&mut self, cx: i32, cy: i32, radius: i32) {
        self.circf(cx as f32, cy as f32, radius as f32);
    }
    /// IRIS GL `circfs` [soft 0x08612af0 circfs].
    pub fn circfs(&mut self, cx: i16, cy: i16, radius: i16) {
        self.circf(cx as f32, cy as f32, radius as f32);
    }

    // ------------------------------------------------------------------ primitives

    /// IRIS GL `bgnpoint`: `glBegin(GL_POINTS)` [soft 0x08612b34 bgnpoint].
    pub fn bgnpoint(&mut self) {
        self.sys.gl.glBegin(0);
    }
    /// IRIS GL `endpoint` [soft 0x08612b48 endpoint].
    pub fn endpoint(&mut self) {
        self.sys.gl.glEnd();
    }
    /// IRIS GL `bgnline`: `glBegin(GL_LINE_STRIP)` [soft 0x08612b58 bgnline].
    pub fn bgnline(&mut self) {
        self.sys.gl.glBegin(3);
    }
    /// IRIS GL `endline` [soft 0x08612b6c endline].
    pub fn endline(&mut self) {
        self.sys.gl.glEnd();
    }
    /// IRIS GL `bgnclosedline`: `glBegin(GL_LINE_LOOP)` [soft 0x08612b7c bgnclosedline].
    pub fn bgnclosedline(&mut self) {
        self.sys.gl.glBegin(2);
    }
    /// IRIS GL `endclosedline` [soft 0x08612b90 endclosedline].
    pub fn endclosedline(&mut self) {
        self.sys.gl.glEnd();
    }
    /// IRIS GL `bgnpolygon`: `glBegin(GL_POLYGON)` [soft 0x08612ba0 bgnpolygon].
    pub fn bgnpolygon(&mut self) {
        self.sys.gl.glBegin(9);
    }
    /// IRIS GL `endpolygon` [soft 0x08612bb4 endpolygon].
    pub fn endpolygon(&mut self) {
        self.sys.gl.glEnd();
    }
    /// IRIS GL `bgntmesh`: `glBegin(GL_TRIANGLE_STRIP)` [soft 0x08612bc4 bgntmesh].
    pub fn bgntmesh(&mut self) {
        self.sys.gl.glBegin(5);
    }
    /// IRIS GL `endtmesh` [soft 0x08612bd8 endtmesh].
    pub fn endtmesh(&mut self) {
        self.sys.gl.glEnd();
    }
    /// IRIS GL `v2d` [soft 0x08612be8 v2d].
    pub fn v2d(&mut self, v: &[f64; 2]) {
        self.sys.gl.glVertex2dv(v);
    }
    /// IRIS GL `v2f` [soft 0x08612c00 v2f].
    pub fn v2f(&mut self, v: &[f32; 2]) {
        self.sys.gl.glVertex2fv(v);
    }
    /// IRIS GL `v2i` [soft 0x08612c18 v2i].
    pub fn v2i(&mut self, v: &[i32; 2]) {
        self.sys.gl.glVertex2iv(v);
    }
    /// IRIS GL `v2s` [soft 0x08612c30 v2s].
    pub fn v2s(&mut self, v: &[i16; 2]) {
        self.sys.gl.glVertex2sv(v);
    }
    /// IRIS GL `v3d` [soft 0x08612c48 v3d].
    pub fn v3d(&mut self, v: &[f64; 3]) {
        self.sys.gl.glVertex3dv(v);
    }
    /// IRIS GL `v3f` [soft 0x08612c60 v3f].
    pub fn v3f(&mut self, v: &[f32; 3]) {
        self.sys.gl.glVertex3fv(v);
    }
    /// IRIS GL `v3i` [soft 0x08612c78 v3i].
    pub fn v3i(&mut self, v: &[i32; 3]) {
        self.sys.gl.glVertex3iv(v);
    }
    /// IRIS GL `v3s` [soft 0x08612c90 v3s].
    pub fn v3s(&mut self, v: &[i16; 3]) {
        self.sys.gl.glVertex3sv(v);
    }
    /// IRIS GL `v4d` [soft 0x08612ca8 v4d].
    pub fn v4d(&mut self, v: &[f64; 4]) {
        self.sys.gl.glVertex4dv(v);
    }
    /// IRIS GL `v4f` [soft 0x08612cc0 v4f].
    pub fn v4f(&mut self, v: &[f32; 4]) {
        self.sys.gl.glVertex4fv(v);
    }
    /// IRIS GL `v4i` [soft 0x08612cd8 v4i].
    pub fn v4i(&mut self, v: &[i32; 4]) {
        self.sys.gl.glVertex4iv(v);
    }
    /// IRIS GL `v4s` [soft 0x08612cf0 v4s].
    pub fn v4s(&mut self, v: &[i16; 4]) {
        self.sys.gl.glVertex4sv(v);
    }
    /// IRIS GL `n3f` [soft 0x08612d08 n3f].
    pub fn n3f(&mut self, v: &[f32; 3]) {
        self.sys.gl.glNormal3fv(v);
    }

    // ------------------------------------------------------------------ rendering modes

    /// IRIS GL `pntsmooth`: bit 0 turns point smoothing **and blending** on, else both
    /// off; bit 1 chooses the "nicest" hint, else "fastest" [soft 0x08612d20 pntsmooth].
    pub fn pntsmooth(&mut self, mode: u32) {
        self.smooth(0xb10, mode & 1 != 0);
        self.sys.gl.glHint(0xc51, if mode & 2 == 0 { 0x1101 } else { 0x1102 });
    }

    /// IRIS GL `linesmooth`: as `pntsmooth`, for lines [soft 0x08612da0 linesmooth].
    pub fn linesmooth(&mut self, mode: u32) {
        self.smooth(0xb20, mode & 1 != 0);
        self.sys.gl.glHint(0xc52, if mode & 2 == 0 { 0x1101 } else { 0x1102 });
    }

    /// IRIS GL `polysmooth`: bit 0 turns polygon smoothing and blending on, else both off
    /// [soft 0x08612e20 polysmooth].
    pub fn polysmooth(&mut self, mode: u32) {
        self.smooth(0xb41, mode & 1 != 0);
    }

    fn smooth(&mut self, cap: u32, on: bool) {
        if on {
            self.sys.gl.glEnable(cap);
            self.sys.gl.glEnable(0xbe2);
        } else {
            self.sys.gl.glDisable(cap);
            self.sys.gl.glDisable(0xbe2);
        }
    }

    /// IRIS GL `blendfunction(sfactor, dfactor)` [soft 0x08612e68 blendfunction].
    ///
    /// Source 0..=8 → GL_ZERO, GL_ONE, GL_DST_COLOR, GL_ONE_MINUS_DST_COLOR, GL_SRC_ALPHA,
    /// GL_ONE_MINUS_SRC_ALPHA, GL_DST_ALPHA, GL_ONE_MINUS_DST_ALPHA, GL_SRC_ALPHA_SATURATE;
    /// destination 0..=7 → GL_ZERO, GL_ONE, GL_SRC_COLOR, GL_ONE_MINUS_SRC_COLOR and the
    /// same four alpha factors. Anything else prints a warning and changes nothing. The
    /// call never turns blending on or off.
    pub fn blendfunction(&mut self, sfactor: i32, dfactor: i32) {
        let s = match sfactor {
            0 => 0,
            1 => 1,
            2 => 0x306,
            3 => 0x307,
            4 => 0x302,
            5 => 0x303,
            6 => 0x304,
            7 => 0x305,
            8 => 0x308,
            _ => {
                self.err_warning(b"Bad or unsupported source factor for blendfunction()", &[]);
                return;
            }
        };
        let d = match dfactor {
            0 => 0,
            1 => 1,
            2 => 0x300,
            3 => 0x301,
            4 => 0x302,
            5 => 0x303,
            6 => 0x304,
            7 => 0x305,
            _ => {
                self.err_warning(b"Bad or unsupported destination factor for blendfunction()", &[]);
                return;
            }
        };
        self.sys.gl.glBlendFunc(s, d);
    }

    /// IRIS GL `logicop` [soft 0x08612f40 logicop]: the logic op is disabled for LO_SRC (3)
    /// and enabled for every other value; operations 0..=15 map onto GL_CLEAR + n.
    pub fn logicop(&mut self, op: i32) {
        if op == 3 {
            self.sys.gl.glDisable(0xbf2);
        } else {
            self.sys.gl.glEnable(0xbf2);
        }
        if (0..=0xf).contains(&op) {
            self.sys.gl.glLogicOp(0x1500 + op as u32);
        }
    }

    /// IRIS GL `gflush` [soft 0x08613094 gflush].
    pub fn gflush(&mut self) {
        self.sys.gl.glFlush();
    }

    /// IRIS GL `finish` [soft 0x086130a4 finish].
    pub fn finish(&mut self) {
        self.sys.gl.glFinish();
    }

    // ------------------------------------------------------------------ move and draw

    /// IRIS GL `move`: set the graphics position [soft 0x086130b4 move]. (`move` is a
    /// Rust keyword, hence the name.)
    pub fn move_(&mut self, px: f32, py: f32, pz: f32) {
        self.ctx_get();
        if let Some(g) = self.gfx_cur() {
            g.pos = [px as f64, py as f64, pz as f64];
        }
    }
    /// IRIS GL `movei` [soft 0x086130f4 movei].
    pub fn movei(&mut self, px: i32, py: i32, pz: i32) {
        self.move_(px as f32, py as f32, pz as f32);
    }
    /// IRIS GL `moves` [soft 0x08613134 moves].
    pub fn moves(&mut self, px: i16, py: i16, pz: i16) {
        self.move_(px as f32, py as f32, pz as f32);
    }
    /// IRIS GL `move2` [soft 0x08613178 move2].
    pub fn move2(&mut self, px: f32, py: f32) {
        self.move_(px, py, 0.0);
    }
    /// IRIS GL `move2i` [soft 0x086131a0 move2i].
    pub fn move2i(&mut self, px: i32, py: i32) {
        self.move_(px as f32, py as f32, 0.0);
    }
    /// IRIS GL `move2s` [soft 0x086131d4 move2s].
    pub fn move2s(&mut self, px: i16, py: i16) {
        self.move_(px as f32, py as f32, 0.0);
    }

    /// IRIS GL `draw`: a line from the graphics position to the point, which becomes the
    /// new position [soft 0x08613208 draw].
    pub fn draw(&mut self, px: f32, py: f32, pz: f32) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        let g = &mut ctx.wnd.windows[c].gfx;
        sys.gl.glBegin(3);
        sys.gl.glVertex3dv(&g.pos);
        sys.gl.glVertex3d(px as f64, py as f64, pz as f64);
        sys.gl.glEnd();
        g.pos = [px as f64, py as f64, pz as f64];
    }
    /// IRIS GL `drawi` [soft 0x0861329c drawi].
    pub fn drawi(&mut self, px: i32, py: i32, pz: i32) {
        self.draw(px as f32, py as f32, pz as f32);
    }
    /// IRIS GL `draws` [soft 0x086132dc draws].
    pub fn draws(&mut self, px: i16, py: i16, pz: i16) {
        self.draw(px as f32, py as f32, pz as f32);
    }
    /// IRIS GL `draw2` [soft 0x08613320 draw2].
    pub fn draw2(&mut self, px: f32, py: f32) {
        self.draw(px, py, 0.0);
    }
    /// IRIS GL `draw2i` [soft 0x08613348 draw2i].
    pub fn draw2i(&mut self, px: i32, py: i32) {
        self.draw(px as f32, py as f32, 0.0);
    }
    /// IRIS GL `draw2s` [soft 0x0861337c draw2s].
    pub fn draw2s(&mut self, px: i16, py: i16) {
        self.draw(px as f32, py as f32, 0.0);
    }

    // ------------------------------------------------------------------ depth and stencil

    /// IRIS GL `getzbuffer`: whether OpenGL's depth test is on
    /// [soft 0x086133b0 getzbuffer].
    pub fn getzbuffer(&mut self) -> bool {
        self.sys.gl.glIsEnabled(0xb71) == 1
    }

    /// IRIS GL `zbuffer` [soft 0x086133d0 zbuffer].
    pub fn zbuffer(&mut self, enable: i32) {
        if enable == 0 {
            self.sys.gl.glDisable(0xb71);
        } else {
            self.sys.gl.glEnable(0xb71);
        }
    }

    /// IRIS GL `depthcue`: fog, linear, **together with the depth test** (on: also depth
    /// function "less or equal"; off: the depth test is turned off too)
    /// [soft 0x08613404 depthcue].
    pub fn depthcue(&mut self, enable: i32) {
        self.ctx_get();
        if self.cur().is_none() {
            return;
        }
        let gl = &mut self.sys.gl;
        if enable == 0 {
            gl.glDisable(0xb60);
            gl.glDisable(0xb71);
        } else {
            gl.glEnable(0xb60);
            gl.glEnable(0xb71);
            gl.glFogi(0xb65, 0x2601);
            gl.glDepthFunc(0x203);
        }
    }

    /// IRIS GL `lsetdepth(near, far)`: `glDepthRange` of the two values over
    /// 2^(depth bits − 1) − 1, the bit count being the start-up visual's
    /// [soft 0x08613480 lsetdepth].
    pub fn lsetdepth(&mut self, near: i32, far: i32) {
        self.ctx_get();
        let bits = self.ctx.as_deref().unwrap().gfx.config.depth;
        let n = (1i32 << ((bits as u8).wrapping_sub(1) & 0x1f)).wrapping_sub(1);
        self.sys.gl.glDepthRange((x(near) / x(n)).to_f64(), (x(far) / x(n)).to_f64());
    }

    /// IRIS GL `czclear(cval, zval)`: clear colour and depth buffers
    /// [soft 0x086134d4 czclear].
    ///
    /// In RGB mode `cval` is a packed colour (as `cpack`); in colour-map mode an index
    /// (12 bits, write mask not applied) with alpha 1. The depth value is `zval` over
    /// `getgconfig(7)`.
    pub fn czclear(&mut self, cval: u32, zval: i32) {
        self.ctx_get();
        self.ctx_get();
        let Some(c) = self.cur() else { return };
        if self.gfx_restore_buffers() == 0 {
            return;
        }
        let ctx = self.ctx.as_deref().unwrap();
        let f = |v: i32| (x(v) / x(255.0f32)).to_f32();
        let (r, g, b, a) = if ctx.wnd.windows[c].gfx.config.rgb == 1 {
            (f((cval & 0xff) as i32), f((cval >> 8 & 0xff) as i32), f((cval >> 16 & 0xff) as i32), f((cval >> 24) as i32))
        } else {
            let at = (cval & 0xfff) as usize * 3;
            (f(ctx.gfx.cmap[at] as i32), f(ctx.gfx.cmap[at + 1] as i32), f(ctx.gfx.cmap[at + 2] as i32), 1.0)
        };
        self.sys.gl.glClearColor(r, g, b, a);
        let n = self.getgconfig(7);
        self.sys.gl.glClearDepth((x(zval) / x(n)).to_f64());
        self.sys.gl.glClear(0x4100);
    }

    /// IRIS GL `zclear`: clear the depth buffer to 1 [soft 0x08613628 zclear].
    pub fn zclear(&mut self) {
        if self.gfx_restore_buffers() != 0 {
            self.sys.gl.glClearDepth(1.0);
            self.sys.gl.glClear(0x100);
        }
    }

    /// An IRIS GL stencil operation as OpenGL's: 1 zero, 2 replace, 3 increment,
    /// 4 decrement, 5 invert, anything else keep [soft 0x08613658 gfx_stencilOpConv].
    fn gfx_stencil_op_conv(op: i32) -> u32 {
        match op {
            1 => 0,
            2 => 0x1e01,
            3 => 0x1e02,
            4 => 0x1e03,
            5 => 0x150a,
            _ => 0x1e00,
        }
    }

    /// An IRIS GL comparison as OpenGL's: 0..=6 never, less, equal, less-or-equal,
    /// greater, not-equal, greater-or-equal; anything else always
    /// [soft 0x086136a0 gfx_stencilFuncConv].
    fn gfx_stencil_func_conv(func: i32) -> u32 {
        match func {
            0..=6 => 0x200 + func as u32,
            _ => 0x207,
        }
    }

    /// IRIS GL `stencil(enable, ref, func, mask, fail, pass, zpass)`
    /// [soft 0x086136fc stencil].
    ///
    /// The capability switched is 0x1802 (GL_STENCIL, a pixel-copy token), not
    /// GL_STENCIL_TEST (0xb90): as coded, the stencil test itself is never turned on or
    /// off by this call (OpenGL answers an invalid enumerant).
    pub fn stencil(&mut self, enable: i32, reference: i32, func: i32, mask: u32, fail: i32, pass: i32, zpass: i32) {
        if enable == 0 {
            self.sys.gl.glDisable(0x1802);
        } else {
            let (c, b, a) = (Self::gfx_stencil_op_conv(zpass), Self::gfx_stencil_op_conv(pass), Self::gfx_stencil_op_conv(fail));
            self.sys.gl.glStencilOp(a, b, c);
            self.sys.gl.glStencilFunc(Self::gfx_stencil_func_conv(func), reference, mask);
            self.sys.gl.glEnable(0x1802);
        }
    }

    /// IRIS GL `swritemask` [soft 0x08613784 swritemask].
    pub fn swritemask(&mut self, mask: u32) {
        self.sys.gl.glStencilMask(mask);
    }

    /// IRIS GL `sclear`: clear the stencil buffer to a value [soft 0x0861379c sclear].
    pub fn sclear(&mut self, value: i32) {
        self.sys.gl.glClearStencil(value);
        self.sys.gl.glClear(0x400);
    }

    // ------------------------------------------------------------------ mapping and objects

    /// The common part of `mapw` and `mapw2`: both matrices saved, the viewing object
    /// called as a display list, and viewport, projection, model-view and depth range read.
    fn mapw_begin(&mut self, object: u32) -> ([i32; 4], [f64; 16], [f64; 16], [f64; 2]) {
        let gl = &mut self.sys.gl;
        gl.glPushAttrib(0x81800);
        gl.glMatrixMode(0x1701);
        gl.glPushMatrix();
        gl.glMatrixMode(0x1700);
        gl.glPushMatrix();
        gl.glCallList(object);
        let (mut vp, mut proj, mut model, mut range) = ([0i32; 4], [0.0f64; 16], [0.0f64; 16], [0.0f64; 2]);
        gl.glGetIntegerv(0xba2, &mut vp);
        gl.glGetDoublev(0xba7, &mut proj);
        gl.glGetDoublev(0xba6, &mut model);
        gl.glGetDoublev(0xb70, &mut range);
        (vp, proj, model, range)
    }

    fn mapw_end(&mut self) {
        let gl = &mut self.sys.gl;
        gl.glMatrixMode(0x1700);
        gl.glPopMatrix();
        gl.glMatrixMode(0x1701);
        gl.glPopMatrix();
        gl.glPopAttrib();
    }

    /// IRIS GL `mapw(vobj, sx, sy)`: the world-space line under a screen point, as its
    /// points on the near and the far end of the depth range [soft 0x086137c0 mapw].
    pub fn mapw(&mut self, object: u32, sx: i16, sy: i16) -> ([f32; 3], [f32; 3]) {
        let (vp, proj, model, range) = self.mapw_begin(object);
        let (_, a) = self.sys.glu.gluUnProject(sx as f64, sy as f64, range[0], &model, &proj, &vp);
        let (_, b) = self.sys.glu.gluUnProject(sx as f64, sy as f64, range[1], &model, &proj, &vp);
        self.mapw_end();
        ([a[0] as f32, a[1] as f32, a[2] as f32], [b[0] as f32, b[1] as f32, b[2] as f32])
    }

    /// IRIS GL `mapw2(vobj, sx, sy)`: the world-space x and y under a screen point, taken
    /// at the middle of the depth range [soft 0x086139ac mapw2].
    pub fn mapw2(&mut self, object: u32, sx: i16, sy: i16) -> (f32, f32) {
        let (vp, proj, model, range) = self.mapw_begin(object);
        let z = ((x(range[0]) + x(range[1])) * x(0.5f64)).to_f64();
        let (_, a) = self.sys.glu.gluUnProject(sx as f64, sy as f64, z, &model, &proj, &vp);
        self.mapw_end();
        (a[0] as f32, a[1] as f32)
    }

    /// IRIS GL `genobj`: a new display list number [soft 0x08613b04 genobj].
    pub fn genobj(&mut self) -> u32 {
        self.sys.gl.glGenLists(1)
    }
    /// IRIS GL `closeobj`: `glEndList` [soft 0x08613b18 closeobj].
    pub fn closeobj(&mut self) {
        self.sys.gl.glEndList();
    }
    /// IRIS GL `makeobj`: `glNewList(obj, GL_COMPILE)` [soft 0x08613b28 makeobj].
    pub fn makeobj(&mut self, object: u32) {
        self.sys.gl.glNewList(object, 0x1300);
    }
    /// IRIS GL `callobj` [soft 0x08613b44 callobj].
    pub fn callobj(&mut self, object: u32) {
        self.sys.gl.glCallList(object);
    }
    /// IRIS GL `delobj` [soft 0x08613b5c delobj].
    pub fn delobj(&mut self, object: u32) {
        self.sys.gl.glDeleteLists(object, 1);
    }
    /// IRIS GL `isobj` [soft 0x08613b74 isobj].
    pub fn isobj(&mut self, object: u32) -> u8 {
        self.sys.gl.glIsList(object)
    }
}
