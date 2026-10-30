// SPDX-License-Identifier: MIT
//! Raster fonts and character output (`font.c`, 0x0860e9a0, 23 functions).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! A font is a table of bitmaps drawn with `glBitmap`. Font 0 is built in (128 characters,
//! height 15); others come from `defrasterfont`. The character position (`cmov`) is kept
//! by the layer per window in the coordinates of the current transformation; after each
//! character it advances by the character's width divided by the number of pixels one unit
//! of x covers, which `charstr` measures by transforming two raster positions.

use crate::context::{st, IrisGl};
use crate::data;
use crate::state::*;
use si_x87::{x, Round, F80};

/// C `strlen`: the bytes before the first NUL (all of them if there is none).
fn c_len(s: &[u8]) -> usize {
    s.iter().position(|&b| b == 0).unwrap_or(s.len())
}

impl IrisGl {
    /// Fill a font record from IRIS GL's `defrasterfont` arguments
    /// [soft 0x0860e9a0 fnt_createFont].
    ///
    /// `chars` holds eight bytes per character: raster offset (short), width, height
    /// (bytes), x offset, y offset (signed bytes), advance (short). `raster` is 16-bit
    /// words, each bitmap row padded to a word. Per character: origin = the negated
    /// offsets, and unless the width is 0 a bitmap of `(width + 7) / 8` bytes a row, each
    /// word giving its high byte then its low byte. The font's descender is the largest
    /// distance a character reaches below the base line (the negated smallest y offset,
    /// never below 0). The raster's length is not used.
    fn fnt_create_font(id: i32, height: i32, nchars: i32, chars: &[u8], raster: &[u16]) -> Font {
        let mut font = Font { id, height, descender: 0, chars: Vec::new() };
        // the original reads `nchars` records and the raster unchecked
        let word = |i: usize| raster.get(i).copied().unwrap_or(0);
        for i in 0..nchars.max(0) as usize {
            let fc: [u8; 8] = match chars.get(i * 8..i * 8 + 8) {
                Some(b) => b.try_into().unwrap(),
                None => [0; 8],
            };
            let offset = u16::from_le_bytes([fc[0], fc[1]]) as usize;
            let (w, h) = (fc[2] as i32, fc[3] as i32);
            let (xoff, yoff) = (fc[4] as i8 as i32, fc[5] as i8 as i32);
            if yoff < font.descender {
                font.descender = yoff;
            }
            let mut out = FontChar { width: w, height: h, xorig: -xoff, yorig: -yoff, advance: i16::from_le_bytes([fc[6], fc[7]]) as i32, bitmap: None };
            if w != 0 {
                let bytes_per_row = (((w - 1) >> 3) + 1) as usize;
                let words_per_row = (((w - 1) >> 4) + 1) as usize;
                let mut bitmap = vec![0u8; h as usize * bytes_per_row];
                for row in 0..h as usize {
                    let src = offset + row * words_per_row;
                    let mut dst = row * bytes_per_row;
                    let mut remaining = bytes_per_row as i32;
                    for k in 0..words_per_row {
                        bitmap[dst] = (word(src + k) >> 8) as u8;
                        dst += 1;
                        if remaining - 1 > 0 {
                            bitmap[dst] = word(src + k) as u8;
                            dst += 1;
                        }
                        remaining -= 2;
                    }
                }
                out.bitmap = Some(bitmap);
            }
            font.chars.push(out);
        }
        font.descender = -font.descender;
        font
    }

    // `fnt_destroyFont` [soft 0x0860eb04 fnt_destroyFont] frees a font's bitmaps and its
    // character table; here the record is simply replaced.

    /// The font with the given index [soft 0x0860eb54 fnt_getFontFromID].
    fn fnt_get_font_from_id(&mut self, id: i32) -> Option<usize> {
        self.ctx_get();
        self.ctx.as_deref().unwrap().fnt.fonts.iter().position(|f| f.id == id)
    }

    /// Allocate the font state with the built-in font as font 0, current
    /// [soft 0x0860eb9c FNT_init].
    pub(crate) fn fnt_init(&mut self) {
        let font = Self::fnt_create_font(0, data::FONT_HEIGHT, data::FONT_NCHAR, data::FONT_DEF.get(), data::font_raster());
        let (ctx, _) = st!(self);
        ctx.fnt = FntContext { fonts: vec![font], current: 0 };
    }

    /// Allocate the window's character position, (0, 0, 0)
    /// [soft 0x0860ec04 FNT_initWindow].
    pub(crate) fn fnt_init_window(&mut self, index: usize) {
        let (ctx, _) = st!(self);
        ctx.wnd.windows[index].fnt = FntWindow::default();
    }

    /// Free it [soft 0x0860ec24 FNT_exitWindow].
    pub(crate) fn fnt_exit_window(&mut self, _index: usize) {}

    /// IRIS GL `font`: choose the current font; an undefined index is ignored
    /// [soft 0x0860ec44 font]. The current font is global, not per window.
    pub fn font(&mut self, index: i16) {
        self.ctx_get();
        if let Some(f) = self.fnt_get_font_from_id(index as i32) {
            self.ctx.as_deref_mut().unwrap().fnt.current = f;
        }
    }

    /// IRIS GL `getfont` [soft 0x0860ec74 getfont].
    pub fn getfont(&mut self) -> i32 {
        self.ctx_get();
        let f = &self.ctx.as_deref().unwrap().fnt;
        f.fonts[f.current].id
    }

    /// IRIS GL `getdescender` [soft 0x0860ec8c getdescender].
    pub fn getdescender(&mut self) -> i32 {
        self.ctx_get();
        let f = &self.ctx.as_deref().unwrap().fnt;
        f.fonts[f.current].descender
    }

    /// IRIS GL `getheight`: the font's height **plus its descender**
    /// [soft 0x0860eca4 getheight].
    pub fn getheight(&mut self) -> i32 {
        self.ctx_get();
        let f = &self.ctx.as_deref().unwrap().fnt;
        f.fonts[f.current].descender.wrapping_add(f.fonts[f.current].height)
    }

    /// IRIS GL `charstr`: draw a string at the character position and advance it
    /// [soft 0x0860ecc4 charstr]. `None` is a null pointer.
    ///
    /// Nothing without a window or for an empty string. The raster position is set to the
    /// character position; if OpenGL reports it invalid (clipped), nothing is drawn and
    /// the position does not move. Otherwise the window x of the position and of the
    /// position one unit to the right are read back: their difference `d` gives the scale
    /// `1 / d` (1 if `d` is 0). With the six unpack settings forced to 0, 0, 0, 0, 0 and
    /// alignment 1, each character whose code (as a signed char) is below the font's
    /// character count is drawn with `glRasterPos3d` and `glBitmap` (no raster move), and
    /// the position's x grows by advance × scale. The settings are then restored.
    pub fn charstr(&mut self, s: Option<&[u8]>) {
        self.ctx_get();
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let Some(c) = ctx.wnd.current else { return };
        let Some(s) = s else { return };
        let len = c_len(s);
        if len == 0 {
            return;
        }
        let gl = &mut sys.gl;
        let pos = &mut ctx.wnd.windows[c].fnt.pos;
        gl.glRasterPos3d(pos[0], pos[1], pos[2]);
        let mut valid = [0u8; 1];
        gl.glGetBooleanv(0xb08, &mut valid);
        if valid[0] == 0 {
            return;
        }
        gl.glRasterPos3d(pos[0], pos[1], pos[2]);
        let mut p0 = [0.0f64; 4];
        gl.glGetDoublev(0xb07, &mut p0);
        gl.glRasterPos3d((F80::ONE + x(pos[0])).to_f64(), pos[1], pos[2]);
        let mut p1 = [0.0f64; 4];
        gl.glGetDoublev(0xb07, &mut p1);
        let d = x(p1[0]) - x(p0[0]);
        let scale = if d.abs().eq(F80::ZERO) { 1.0 } else { (x(1.0f64) / d).to_f64() };
        let mut saved = [[0i32; 1]; 6];
        for (i, v) in saved.iter_mut().enumerate() {
            gl.glGetIntegerv(0xcf0 + i as u32, v);
        }
        for i in 0..5 {
            gl.glPixelStorei(0xcf0 + i, 0);
        }
        gl.glPixelStorei(0xcf5, 1);
        let font = &ctx.fnt.fonts[ctx.fnt.current];
        for &b in &s[..len] {
            let code = b as i8 as i32;
            if code < font.chars.len() as i32 {
                // a code of 0x80 and above is negative here: the original then reads
                // before the character table; such characters are skipped
                let Some(ch) = usize::try_from(code).ok().and_then(|i| font.chars.get(i)) else { continue };
                if let Some(bitmap) = &ch.bitmap {
                    gl.glRasterPos3d(pos[0], pos[1], pos[2]);
                    gl.glBitmap(ch.width, ch.height, ch.xorig as f32, ch.yorig as f32, 0.0, 0.0, bitmap);
                }
                pos[0] = (x(ch.advance) * x(scale) + x(pos[0])).to_f64();
            }
        }
        for (i, v) in saved.iter().enumerate() {
            gl.glPixelStorei(0xcf0 + i as u32, v[0]);
        }
    }

    /// IRIS GL `lcharstr(type, str)`: `charstr` for type 0 (STR_B, bytes), nothing for the
    /// wide types [soft 0x0860f068 lcharstr].
    pub fn lcharstr(&mut self, kind: i32, s: Option<&[u8]>) {
        if kind == 0 {
            self.charstr(s);
        }
    }

    /// IRIS GL `strwidth`: the sum of the advances of the string's characters in the
    /// current font [soft 0x0860f084 strwidth].
    pub fn strwidth(&mut self, s: &[u8]) -> i32 {
        self.ctx_get();
        let f = &self.ctx.as_deref().unwrap().fnt;
        let font = &f.fonts[f.current];
        let mut width = 0i32;
        for &b in &s[..c_len(s)] {
            let code = b as i8 as i32;
            if code < font.chars.len() as i32 {
                // as in `charstr`: a negative code reads before the table in the original
                if let Some(ch) = usize::try_from(code).ok().and_then(|i| font.chars.get(i)) {
                    width = width.wrapping_add(ch.advance);
                }
            }
        }
        width
    }

    /// IRIS GL `lstrwidth(type, str)`: `strwidth` for type 0, else 0
    /// [soft 0x0860f154 lstrwidth].
    pub fn lstrwidth(&mut self, kind: i32, s: &[u8]) -> i32 {
        if kind == 0 {
            self.strwidth(s)
        } else {
            0
        }
    }

    /// IRIS GL `defrasterfont(n, ht, nc, chars, nr, raster)` [soft 0x0860f174
    /// defrasterfont] (which destroys a font it replaces
    /// [soft 0x0860eb04 fnt_destroyFont]). Index 0, the built-in font, cannot be redefined.
    pub fn defrasterfont(&mut self, n: i16, ht: i16, nc: i16, chars: &[u8], _nr: i16, raster: &[u16]) {
        self.ctx_get();
        if n == 0 {
            return;
        }
        let at = match self.fnt_get_font_from_id(n as i32) {
            Some(i) => i,
            None => {
                let f = &self.ctx.as_deref().unwrap().fnt;
                let current_id = f.fonts[f.current].id;
                self.ctx.as_deref_mut().unwrap().fnt.fonts.push(Font::default());
                // the table may have moved: the current font is found again by its index,
                // among the old entries (the new, zeroed one has index 0 like the built-in
                // font but comes after it)
                let cur = self.fnt_get_font_from_id(current_id);
                let f = &mut self.ctx.as_deref_mut().unwrap().fnt;
                f.current = cur.unwrap_or(0);
                f.fonts.len() - 1
            }
        };
        let font = Self::fnt_create_font(n as i32, ht as i32, nc as i32, chars, raster);
        self.ctx.as_deref_mut().unwrap().fnt.fonts[at] = font;
    }

    /// IRIS GL `deflfont`: nothing [soft 0x0860f248 deflfont].
    pub fn deflfont(&mut self) {}

    /// IRIS GL `cmov`: set the character position [soft 0x0860f250 cmov].
    pub fn cmov(&mut self, px: f32, py: f32, pz: f32) {
        self.ctx_get();
        let (ctx, _) = st!(self);
        if let Some(c) = ctx.wnd.current {
            ctx.wnd.windows[c].fnt.pos = [px as f64, py as f64, pz as f64];
        }
    }
    /// IRIS GL `cmov2` [soft 0x0860f284 cmov2].
    pub fn cmov2(&mut self, px: f32, py: f32) {
        self.cmov(px, py, 0.0);
    }
    /// IRIS GL `cmov2i` [soft 0x0860f2ac cmov2i].
    pub fn cmov2i(&mut self, px: i32, py: i32) {
        self.cmov(px as f32, py as f32, 0.0);
    }
    /// IRIS GL `cmov2s` [soft 0x0860f2e0 cmov2s].
    pub fn cmov2s(&mut self, px: i16, py: i16) {
        self.cmov(px as f32, py as f32, 0.0);
    }
    /// IRIS GL `cmovi` [soft 0x0860f314 cmovi].
    pub fn cmovi(&mut self, px: i32, py: i32, pz: i32) {
        self.cmov(px as f32, py as f32, pz as f32);
    }
    /// IRIS GL `cmovs` [soft 0x0860f354 cmovs].
    pub fn cmovs(&mut self, px: i16, py: i16, pz: i16) {
        self.cmov(px as f32, py as f32, pz as f32);
    }

    /// IRIS GL `getcpos`: the character position's x and y, truncated to integers, low 16
    /// bits [soft 0x0860f398 getcpos]. These are the layer's stored coordinates, not
    /// screen pixels as in IRIS GL. `None` without a window.
    pub fn getcpos(&mut self) -> Option<(i16, i16)> {
        self.ctx_get();
        let (ctx, _) = st!(self);
        let c = ctx.wnd.current?;
        let pos = ctx.wnd.windows[c].fnt.pos;
        Some((x(pos[0]).to_i32(Round::Zero) as i16, x(pos[1]).to_i32(Round::Zero) as i16))
    }
}
