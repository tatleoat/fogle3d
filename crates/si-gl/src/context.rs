// SPDX-License-Identifier: MIT
//! The layer's root: the global context and its start-up (`context.c`, 0x08619c80, 7
//! functions) and the two message routines (`error.c`, 0x0860cb30, 2 functions).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! There is no explicit initialisation call. Every entry point begins by asking for the
//! context ([`IrisGl::ctx_get`] or [`IrisGl::ctx_get_ng`]); the first such request opens the
//! display and builds all the state. The difference between the two requests matters for
//! the sequence of outside calls: `ctx_get` also asks GLX whether a drawable is current and
//! makes the hidden start-up window current if none is, so nearly every IRIS GL call starts
//! with one `glXGetCurrentDrawable`. A port has to make the same request the same number of
//! times as the original function does.

use crate::data;
use crate::state::*;
use crate::sys::{CArg, CStream, Ptr, Sys, XSetWindowAttributes};

/// The IRIS GL emulation layer: the libraries it calls and everything it remembers.
pub struct IrisGl {
    pub sys: Sys,
    /// `g_context` (0x08700f80): null until the first call into the layer.
    pub ctx: Option<Box<Context>>,
    /// `g_dbg_level` (0x08700f98): the number in the environment variable `SI_GLOB_TRACE`.
    /// Nothing in the layer reads it.
    pub dbg_level: i32,
    /// `grabX.137` (0x086feeec) and `grabY.138` (0x086feef0): statics of `setvaluator`,
    /// set when a MOUSEX, respectively MOUSEY, valuator was given a restricted range.
    pub grab_x: i32,
    pub grab_y: i32,
    /// `rx.135` (0x08833f40) and `ry.136` (0x08833f44): statics of `setvaluator`, the X
    /// position to warp the pointer to once both axes are restricted.
    pub grab_rx: i32,
    pub grab_ry: i32,
    /// `hasSync.477` (0x086ffab0): static of `gsync`; −1 not yet looked for, 0 the
    /// video-sync extension is missing, 1 it is there.
    pub has_sync: i32,
    /// `waitfunc.478` (0x086ffab4): static of `gsync`, the address of
    /// `glXWaitVideoSyncSGI`.
    pub wait_func: Ptr,
}

/// The context and the libraries, borrowed side by side. Only valid after a `ctx_get`.
macro_rules! st {
    ($s:ident) => {
        ($s.ctx.as_deref_mut().expect("the context exists after ctx_get"), &mut $s.sys)
    };
}
pub(crate) use st;

impl IrisGl {
    /// The layer before its first call: no context, the statics as the binary's data
    /// segment has them.
    pub fn new(sys: Sys) -> Self {
        IrisGl { sys, ctx: None, dbg_level: 0, grab_x: 0, grab_y: 0, grab_rx: 0, grab_ry: 0, has_sync: -1, wait_func: 0 }
    }

    // ------------------------------------------------------------------ error.c

    /// Print "GLOB - FATAL ERROR: ", the message, a new line, on the standard error
    /// stream, and leave the program with status 1 [soft 0x0860cb30 ERR_fatal].
    pub fn err_fatal(&mut self, format: &[u8], args: &[CArg]) -> ! {
        self.sys.c.fprintf(CStream::Stderr, b"GLOB - FATAL ERROR: ", &[]);
        self.sys.c.vfprintf(CStream::Stderr, format, args);
        self.sys.c.fprintf(CStream::Stderr, b"\n", &[]);
        self.sys.c.exit(1)
    }

    /// Print "GLOB - Warning: ", the message and a new line on the standard error stream
    /// [soft 0x0860cb80 ERR_warning].
    pub fn err_warning(&mut self, format: &[u8], args: &[CArg]) {
        self.sys.c.fprintf(CStream::Stderr, b"GLOB - Warning: ", &[]);
        self.sys.c.vfprintf(CStream::Stderr, format, args);
        self.sys.c.fprintf(CStream::Stderr, b"\n", &[]);
    }

    // ------------------------------------------------------------------ context.c

    /// Intern the three atoms the layer uses, in this order: `WM_DELETE_WINDOW`,
    /// `_MOTIF_WM_HINTS`, `WM_STATE` [soft 0x08619c80 ctx_createAtoms].
    fn ctx_create_atoms(&mut self) {
        let (ctx, sys) = st!(self);
        ctx.atoms.wm_delete_window = sys.x.XInternAtom(ctx.display, b"WM_DELETE_WINDOW", 0);
        ctx.atoms.motif_wm_hints = sys.x.XInternAtom(ctx.display, b"_MOTIF_WM_HINTS", 0);
        ctx.atoms.wm_state = sys.x.XInternAtom(ctx.display, b"WM_STATE", 0);
    }

    /// Create the hidden start-up window with a GLX context of its own and make it current
    /// [soft 0x08619ce8 ctx_createInitWindow].
    ///
    /// The window is 128 × 128 at (0, 0) on the root, input-output, with only a colormap
    /// attribute; it is never mapped.
    fn ctx_create_init_window(&mut self) {
        self.ctx.as_deref_mut().unwrap().init_window = None;
        let visual_info = self.wnd_get_best_visual();
        let (ctx, sys) = st!(self);
        let glx_context = sys.glx.glXCreateContext(ctx.display, visual_info, 0, 1);
        if glx_context == 0 {
            self.err_fatal(b"Unable to create initial GLX context", &[]);
        }
        let (ctx, sys) = st!(self);
        let visual = sys.x.visual_info_word(visual_info, 0);
        let colormap = sys.x.XCreateColormap(ctx.display, ctx.root, visual, 0);
        let attributes = XSetWindowAttributes { colormap, ..Default::default() };
        let depth = sys.x.visual_info_word(visual_info, 0xc) as i32;
        let window = sys.x.XCreateWindow(ctx.display, ctx.root, 0, 0, 0x80, 0x80, 0, depth, 1, visual, 0x2000, &attributes);
        sys.glx.glXMakeCurrent(ctx.display, window, glx_context);
        sys.x.XSync(ctx.display, 0);
        ctx.init_window = Some(InitWindow { window, visual_info, colormap, glx_context });
    }

    /// Choose driver work-arounds from the OpenGL vendor and renderer strings
    /// [soft 0x08619db4 ctx_setGLFixes].
    ///
    /// "NVIDIA" in the vendor gives 8; else "Brian Paul" (Mesa) gives 5; else "ATI" in the
    /// vendor together with "FireGL" in the renderer gives 2. `glXGetClientString(dpy,
    /// GLX_VENDOR)` is called and its result dropped.
    fn ctx_set_gl_fixes(&mut self) {
        let (ctx, sys) = st!(self);
        // the original hands the pointers to strstr unchecked: a null vendor string crashes
        // it; an absent string is treated as empty here
        let vendor = sys.gl.glGetString(0x1f00).unwrap_or_default();
        let renderer = sys.gl.glGetString(0x1f01).unwrap_or_default();
        sys.glx.glXGetClientString(ctx.display, 1);
        if contains(&vendor, b"NVIDIA") {
            ctx.gl_fixes = 8;
        } else if contains(&vendor, b"Brian Paul") {
            ctx.gl_fixes = 5;
        } else if contains(&vendor, b"ATI") && contains(&renderer, b"FireGL") {
            ctx.gl_fixes = 2;
        }
    }

    /// Open the display and build the state [soft 0x08619e68 ctx_initContext2].
    ///
    /// `XInitThreads`; the display named by `DISPLAY`, or ":0.0"; the default screen, its
    /// root window, width and height; then the atoms, the start-up window, the driver
    /// work-arounds and the initialisers of `window.c`, `graphics.c`, `event.c`, `cursor.c`
    /// and `font.c`, in that order. Either failure is fatal.
    fn ctx_init_context2(&mut self) {
        if self.sys.x.XInitThreads() == 0 {
            self.err_fatal(b"Can't set X11 display to threaded mode.", &[]);
        }
        let name = self.sys.c.getenv(b"DISPLAY").unwrap_or_else(|| b":0.0".to_vec());
        let display = self.sys.x.XOpenDisplay(&name);
        self.ctx.as_deref_mut().unwrap().display = display;
        if display == 0 {
            self.err_fatal(b"Can't open display '%s'", &[CArg::Str(&name)]);
        }
        let (ctx, sys) = st!(self);
        ctx.screen = sys.x.default_screen(display);
        ctx.root = sys.x.screen_word(display, ctx.screen, 0x8);
        ctx.screen_width = sys.x.screen_word(display, ctx.screen, 0xc) as i32;
        ctx.screen_height = sys.x.screen_word(display, ctx.screen, 0x10) as i32;
        self.ctx_create_atoms();
        self.ctx_create_init_window();
        self.ctx_set_gl_fixes();
        self.wnd_init();
        self.gfx_init();
        self.evt_init();
        self.crs_init();
        self.fnt_init();
    }

    /// Allocate the context and initialise it [soft 0x08619f5c ctx_initContext].
    ///
    /// If the environment has `SI_GLOB_TRACE`, the variable is read a second time and its
    /// value, as a decimal number, goes to `g_dbg_level`.
    fn ctx_init_context(&mut self) {
        if self.sys.c.getenv(b"SI_GLOB_TRACE").is_some() {
            // the original passes the second result to strtol unchecked
            let text = self.sys.c.getenv(b"SI_GLOB_TRACE").unwrap_or_default();
            self.dbg_level = strtol10(&text);
        }
        self.ctx = Some(Box::default());
        self.ctx_init_context2();
    }

    /// The context, created on first use; and if the start-up window exists and GLX reports
    /// no current drawable, that window is made current [soft 0x0861a038 CTX_getContext].
    pub fn ctx_get(&mut self) {
        if self.ctx.is_none() {
            self.ctx_init_context();
        }
        let (ctx, sys) = st!(self);
        if let Some(init) = ctx.init_window {
            if sys.glx.glXGetCurrentDrawable() == 0 {
                sys.glx.glXMakeCurrent(ctx.display, init.window, init.glx_context);
            }
        }
    }

    /// The context, created on first use, without touching GLX
    /// [soft 0x0861a088 CTX_getContextNG].
    pub fn ctx_get_ng(&mut self) {
        if self.ctx.is_none() {
            self.ctx_init_context();
        }
    }

    /// The current window's index in the window table (`*(ctx->wnd + 8)`).
    pub(crate) fn cur(&self) -> Option<usize> {
        self.ctx.as_deref().and_then(|c| c.wnd.current)
    }

    /// Read access to the state, for the callers above the layer and the checks.
    pub fn context(&self) -> Option<&Context> {
        self.ctx.as_deref()
    }
}

/// `strstr(haystack, needle) != NULL`
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

/// `strtol(text, NULL, 10)` on a 32-bit `long`: white space, an optional sign, digits;
/// out-of-range values saturate.
fn strtol10(text: &[u8]) -> i32 {
    let mut i = 0;
    while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut negative = false;
    if i < text.len() && (text[i] == b'+' || text[i] == b'-') {
        negative = text[i] == b'-';
        i += 1;
    }
    let mut v: i64 = 0;
    while i < text.len() && text[i].is_ascii_digit() {
        v = (v * 10 + (text[i] - b'0') as i64).min(1 << 40);
        i += 1;
    }
    let v = if negative { -v } else { v };
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// The visual every window is created with: double buffered RGBA with at least one bit of
/// red, green, blue, depth and stencil [soft 0x08619038 WND_getBestVisual]. Fatal if the
/// server has none. (Belongs to `window.c`; here because the start-up needs it first.)
impl IrisGl {
    pub(crate) fn wnd_get_best_visual(&mut self) -> Ptr {
        self.ctx_get_ng();
        let (ctx, sys) = st!(self);
        let vi = sys.glx.glXChooseVisual(ctx.display, ctx.screen, &data::VISUAL_ATTRIBUTES);
        if vi == 0 {
            self.err_fatal(b"No OpenGL RGB visual found. Needed: Double buffering, z-buffer and stencil buffer.", &[]);
        }
        vi
    }
}
