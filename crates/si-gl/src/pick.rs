// SPDX-License-Identifier: MIT
//! Picking and selecting (`pick.c`, 0x08614cd0, 14 functions).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! Both are OpenGL selection mode. `pick` additionally remembers a pick region around the
//! pointer; the region is not multiplied into the projection at once but each time the
//! program sets a projection afterwards (`window`, `ortho`, `ortho2`, `perspective`,
//! `loadmatrix` call [`IrisGl::pck_restore_pick_matrix`]). The hit records OpenGL writes
//! (name count, two depths, names) are rewritten into IRIS GL's (name count, names) as
//! shorts.

use crate::context::{st, IrisGl};
use crate::state::*;

impl IrisGl {
    /// Enter selection mode with a buffer for `names` names
    /// [soft 0x08614cd0 pck_initSelect].
    ///
    /// The selection buffer has five words per name. `glSelectBuffer`, `glRenderMode
    /// (GL_SELECT)`, `glInitNames`, `glPushName(0)`.
    fn pck_init_select(&mut self, names: i32) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let c = ctx.wnd.current.expect("pck_initSelect: the original dereferences the null current window");
        let p = &mut ctx.wnd.windows[c].pck;
        p.buffer_size = names.wrapping_mul(5);
        p.buffer = true;
        sys.gl.glSelectBuffer(p.buffer_size);
        sys.gl.glRenderMode(0x1c02);
        sys.gl.glInitNames();
        sys.gl.glPushName(0);
    }

    /// Leave selection mode and rewrite the hits [soft 0x08614d34 pck_translateSelect].
    /// Returns what `glRenderMode(GL_RENDER)` returned: the number of hits, negative when
    /// OpenGL's buffer overflowed. `None` for the output stands for a null pointer:
    /// nothing is done and 0 returned.
    ///
    /// For each of |hits| records: the name count, then each name, as shorts; the two
    /// depth words are skipped. Writing stops after as many shorts as the selection buffer
    /// has **words** (five times what the caller said its buffer holds): the original can
    /// overrun the caller's buffer; here writing also stops at the end of `out`.
    fn pck_translate_select(&mut self, out: Option<&mut [i16]>) -> i32 {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let c = ctx.wnd.current.expect("pck_translateSelect: the original dereferences the null current window");
        let p = &mut ctx.wnd.windows[c].pck;
        let Some(out) = out else { return 0 };
        let hits = sys.gl.glRenderMode(0x1c00);
        if hits != 0 {
            let words = sys.gl.selection(p.buffer_size.max(0) as usize);
            let word = |i: usize| words.get(i).copied().unwrap_or(0) as i32;
            let mut put = |at: usize, v: i16| {
                if let Some(o) = out.get_mut(at) {
                    *o = v;
                }
            };
            let mut room = p.buffer_size;
            let (mut src, mut dst, mut done) = (0usize, 0usize, 0);
            while hits.wrapping_abs() > done && room >= 1 {
                let mut n = word(src);
                put(dst, n as i16);
                src += 3;
                done += 1;
                loop {
                    dst += 1;
                    room -= 1;
                    if n < 1 {
                        break;
                    }
                    n -= 1;
                    if room < 1 {
                        break;
                    }
                    put(dst, word(src) as i16);
                    src += 1;
                }
            }
        }
        p.buffer = false;
        hits
    }

    /// Allocate the window's picking state: pick region 10 × 10
    /// [soft 0x08614e08 PCK_initWindow].
    pub(crate) fn pck_init_window(&mut self, index: usize) {
        let (ctx, _) = st!(self);
        ctx.wnd.windows[index].pck = PckWindow { size_x: 10, size_y: 10, ..Default::default() };
    }

    /// Free it [soft 0x08614e38 PCK_exitWindow].
    pub(crate) fn pck_exit_window(&mut self, _index: usize) {}

    /// Load the pick matrix onto the current OpenGL matrix: identity, then
    /// `gluPickMatrix` with the centre, size and viewport remembered by `pick`
    /// [soft 0x08614e58 PCK_restorePickMatrix].
    pub(crate) fn pck_restore_pick_matrix(&mut self) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        let p = &ctx.wnd.windows[c].pck;
        sys.gl.glLoadIdentity();
        sys.glu.gluPickMatrix(p.x as f64, p.y as f64, p.w as f64, p.h as f64, &p.viewport);
    }

    /// IRIS GL `pick(buffer, numnames)`: start picking [soft 0x08614ec8 pick]. Ignored
    /// without a window, with `numnames` 0, or while already picking.
    ///
    /// `glFlush`, the viewport is read, selection mode is entered; the pick region is the
    /// `picksize` region centred on the pointer, whose position is taken in **screen**
    /// coordinates (MOUSEX, MOUSEY), not window coordinates. A `gluPickMatrix` is issued
    /// on the projection stack between a push and a pop, so it has no lasting effect: the
    /// region takes effect at the program's next projection call.
    pub fn pick(&mut self, numnames: i32) {
        self.ctx_get();
        let Some(c) = self.cur() else { return };
        if numnames == 0 || self.ctx.as_deref().unwrap().wnd.windows[c].pck.picking != 0 {
            return;
        }
        self.sys.gl.glFlush();
        let mut vp = [0i32; 4];
        self.sys.gl.glGetIntegerv(0xba2, &mut vp);
        self.pck_init_select(numnames);
        self.sys.gl.glPushAttrib(0x1000);
        self.sys.gl.glMatrixMode(0x1701);
        self.sys.gl.glPushMatrix();
        self.sys.gl.glLoadIdentity();
        {
            let p = &mut self.ctx.as_deref_mut().unwrap().wnd.windows[c].pck;
            p.w = p.size_x;
            p.h = p.size_y;
        }
        let px = self.evt_get_valuator(0x10a);
        self.ctx.as_deref_mut().unwrap().wnd.windows[c].pck.x = px;
        let py = self.evt_get_valuator(0x10b);
        let (ctx, sys) = st!(self);
        let p = &mut ctx.wnd.windows[c].pck;
        p.y = py;
        p.viewport = vp;
        sys.glu.gluPickMatrix(p.x as f64, p.y as f64, p.w as f64, p.h as f64, &vp);
        p.picking = 1;
        sys.gl.glPopMatrix();
        sys.gl.glPopAttrib();
    }

    /// IRIS GL `endpick(buffer)`: the hits [soft 0x08614ff0 endpick]. 0 unless picking.
    pub fn endpick(&mut self, buffer: Option<&mut [i16]>) -> i32 {
        self.ctx_get();
        let Some(c) = self.cur() else { return 0 };
        let p = &self.ctx.as_deref().unwrap().wnd.windows[c].pck;
        if p.buffer && p.picking != 0 {
            let hits = self.pck_translate_select(buffer);
            self.ctx.as_deref_mut().unwrap().wnd.windows[c].pck.picking = 0;
            return hits;
        }
        0
    }

    /// IRIS GL `picksize`: the pick region's size, absolute values
    /// [soft 0x0861503c picksize]. Has an effect **only while a selection buffer exists**,
    /// that is between `pick` (or `gselect`) and its end; before that it is ignored, and
    /// `pick` copies the size when it starts.
    pub fn picksize(&mut self, deltax: i16, deltay: i16) {
        self.ctx_get();
        let Some(c) = self.cur() else { return };
        let p = &mut self.ctx.as_deref_mut().unwrap().wnd.windows[c].pck;
        if p.buffer {
            p.size_x = (deltax as i32).abs();
            p.size_y = (deltay as i32).abs();
        }
    }

    /// IRIS GL `loadname` [soft 0x08615080 loadname]. The short is sign-extended.
    pub fn loadname(&mut self, name: i16) {
        self.sys.gl.glLoadName(name as i32 as u32);
    }

    /// IRIS GL `popname` [soft 0x08615098 popname].
    pub fn popname(&mut self) {
        self.sys.gl.glPopName();
    }

    /// IRIS GL `pushname` [soft 0x086150a8 pushname].
    pub fn pushname(&mut self, name: i16) {
        self.sys.gl.glPushName(name as i32 as u32);
    }

    /// IRIS GL `initnames`: an empty name stack, then the name 0 pushed
    /// [soft 0x086150c0 initnames].
    pub fn initnames(&mut self) {
        self.sys.gl.glInitNames();
        self.sys.gl.glPushName(0);
    }

    /// IRIS GL `gselect(buffer, numnames)`: selection without a pick region
    /// [soft 0x086150dc gselect].
    pub fn gselect(&mut self, numnames: i32) {
        self.ctx_get();
        let Some(c) = self.cur() else { return };
        if numnames != 0 && self.ctx.as_deref().unwrap().wnd.windows[c].pck.selecting == 0 {
            self.pck_init_select(numnames);
            self.ctx.as_deref_mut().unwrap().wnd.windows[c].pck.selecting = 1;
        }
    }

    /// IRIS GL `endselect(buffer)` [soft 0x0861511c endselect].
    pub fn endselect(&mut self, buffer: Option<&mut [i16]>) -> i32 {
        self.ctx_get();
        let Some(c) = self.cur() else { return 0 };
        let p = &self.ctx.as_deref().unwrap().wnd.windows[c].pck;
        if p.buffer && p.selecting != 0 {
            let hits = self.pck_translate_select(buffer);
            self.ctx.as_deref_mut().unwrap().wnd.windows[c].pck.selecting = 0;
            return hits;
        }
        0
    }
}
