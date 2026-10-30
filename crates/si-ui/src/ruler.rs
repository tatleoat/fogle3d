// SPDX-License-Identifier: MIT
//! `ruler.c` of the program (0x083ac990 to 0x083ae2db): the rulers along the edges of a view.
//!
//! A ruler is a 0x24-byte record, which this file reaches by offset (the record has no
//! declared type here): the view window (+0), the kind (+4: 1 horizontal, 2 vertical,
//! 3 dope sheet, 4 sound), +8 and +0xc (cleared), the size in pixels (+0x10), the visible
//! range (floats at +0x14 and +0x18), the tick length (+0x1c) and the active flag (+0x20, a
//! short).
//!
//! Ported: all 17 functions of the file: the creation and bookkeeping routines,
//! `SI_rulerDraw`, the two drawing routines of the horizontal and vertical rulers, the tick
//! interval `rulerIncSize`, the two `SetView` routines and the two string helpers.
//!
//! The two drawing routines are the same algorithm on two axes. A tick interval comes from
//! `rulerIncSize`; the first tick is the view's lower limit less its remainder modulo the
//! interval (the `fprem` loop), and ticks are stepped in single-precision values (each sum
//! is stored as a float, compared as a float, and added to again from the stored float; only
//! the first tick and the pixel positions are computed in the 80-bit registers). The
//! direction depends on whether the lower limit is below the upper one. Long ticks carry a
//! label, then a second loop of short ticks starts half an interval further on.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.

use crate::globals::*;
use crate::mem::{Addr, P};
use crate::records::Grafport;
use crate::Ui;
use si_x87::{Round, F80};

/// `sprintf("%<width>.<precision>f", v)` as the C library prints it.
fn format_f(width: usize, precision: usize, v: f64) -> Vec<u8> {
    let body = if v.is_nan() {
        if v.is_sign_negative() { "-nan".to_string() } else { "nan".to_string() }
    } else if v.is_infinite() {
        if v < 0.0 { "-inf".to_string() } else { "inf".to_string() }
    } else {
        format!("{:.*}", precision, v)
    };
    format!("{:>w$}", body, w = width).into_bytes()
}

impl Ui {
    /// A new ruler record, its fields set by `rulerInitialize`, bound to a view window
    /// [soft 0x083ac990 rulerNew]. 0 when the allocation fails.
    pub fn ruler_new(&mut self, g: P<Grafport>) -> Addr {
        let r = self.o.mem.SI_malloc(&mut self.m, 0x24);
        if r != 0 {
            self.ruler_initialize(r);
            self.m.set_u32(r, g.addr());
        }
        r
    }

    /// A horizontal ruler: kind 1, size 30, tick length 5; returns the ruler [soft 0x083ac9c4 si_rulerCreateHoriz].
    pub fn si_ruler_create_horiz(&mut self, g: P<Grafport>) -> Addr {
        let r = self.ruler_new(g);
        self.m.set_u32(r + 4, 1);
        self.m.set_u32(r + 0x10, 0x1e);
        self.m.set_u32(r + 0x1c, 5);
        r
    }

    /// A vertical ruler: kind 2, size 60, tick length 10; returns the ruler [soft 0x083ac9f0 si_rulerCreateVert].
    pub fn si_ruler_create_vert(&mut self, g: P<Grafport>) -> Addr {
        let r = self.ruler_new(g);
        self.m.set_u32(r + 4, 2);
        self.m.set_u32(r + 0x10, 0x3c);
        self.m.set_u32(r + 0x1c, 10);
        r
    }

    /// The defaults: no view, kind 2, range -5 to 5, size 50, tick length 5, not active
    /// [soft 0x083aca1c rulerInitialize].
    pub fn ruler_initialize(&mut self, r: Addr) {
        self.m.set_f32(r + 0x14, -5.0);
        self.m.set_f32(r + 0x18, 5.0);
        self.m.set_u32(r, 0);
        self.m.set_u32(r + 4, 2);
        self.m.set_u32(r + 8, 0);
        self.m.set_u32(r + 0xc, 0);
        self.m.set_u32(r + 0x1c, 5);
        self.m.set_u32(r + 0x10, 0x32);
        self.m.set_i16(r + 0x20, 0);
    }

    /// Set the active flag and recompute the view window's viewport
    /// [soft 0x083aca68 SI_rulerSetActive].
    pub fn si_ruler_set_active(&mut self, r: Addr, active: i16) {
        self.m.set_i16(r + 0x20, active);
        let g = self.m.u32(r);
        self.si_get_window_viewport(P::at(g));
    }

    /// Draw a ruler on the system window with the application font, when the view is big
    /// enough for it; the window and font are put back afterwards [soft 0x083aca88 SI_rulerDraw].
    /// The vertical kinds need the ruler's size to fit the view's width, every kind the size
    /// plus the title bar to fit its height; a kind that is none of the four prints an error.
    pub fn si_ruler_draw(&mut self, r: Addr) {
        let saved = self.o.gl.winget(&mut self.m);
        let sysid = self.m[SI_SYSID];
        self.o.gl.winset(&mut self.m, sysid);
        let appf = self.m[SI_APPF];
        self.o.gl.font(&mut self.m, appf);
        let kind = if r != 0 { self.m.u32(r + 4) as i32 } else { 0 };
        // read only where a kind is known: the kind of a null ruler is 0
        let (size, width, height);
        let title = self.m[SI_TITLE_HEIGHT] as i32;
        if r != 0 && (1..=4).contains(&kind) {
            let g = self.m.u32(r);
            size = self.m.u32(r + 0x10) as i32;
            width = self.m.i16(g + 0x16) as i32 - self.m.i16(g + 0x14) as i32;
            height = self.m.i16(g + 0x1a) as i32 - self.m.i16(g + 0x18) as i32;
        } else {
            (size, width, height) = (0, 0, 0);
        }
        match kind {
            2 | 3 | 4 => {
                if size <= width && title + size <= height {
                    match kind {
                        2 => self.ruler_draw_vert(r),
                        3 => self.ruler_draw_vert_dope(r),
                        _ => self.ruler_draw_vert_sound(r),
                    }
                }
            }
            1 => {
                if title + size <= height {
                    self.ruler_draw_horiz(r);
                }
            }
            _ => self.o.c.printf(&mut self.m, b"Error -- rulerDraw: unknown ruler type.\n"),
        }
        let irisf = self.m[SI_IRISF];
        self.o.gl.font(&mut self.m, irisf);
        self.o.gl.winset(&mut self.m, saved);
    }

    /// The dope sheet's ruler: the dope sheet's own drawing of its items in the ruler's view
    /// [soft 0x083adf84 rulerDrawVertDope].
    pub fn ruler_draw_vert_dope(&mut self, r: Addr) {
        let g = self.m.u32(r);
        self.o.controls.SI_dopeSheetAllItemDraw(&mut self.m, g);
    }

    /// The sound ruler draws nothing [soft 0x083ae118 rulerDrawVertSound].
    pub fn ruler_draw_vert_sound(&mut self, _r: Addr) {}

    /// Release the ruler's view-window pointer's block and clear the pointer
    /// [soft 0x083adf58 rulerFree]. (The original frees what the first word points to, not
    /// the ruler.)
    pub fn ruler_free(&mut self, r: Addr) {
        if r != 0 && self.m.u32(r) != 0 {
            let b = self.m.u32(r);
            self.o.mem.free(&mut self.m, b);
            self.m.set_u32(r, 0);
        }
    }

    /// Copy every field of a ruler to another [soft 0x083ae298 si_rulerTransfer].
    pub fn si_ruler_transfer(&mut self, from: Addr, to: Addr) {
        for off in [0u32, 4, 8, 0xc, 0x10, 0x14, 0x18, 0x1c] {
            let v = self.m.u32(from + off);
            self.m.set_u32(to + off, v);
        }
        let v = self.m.i16(from + 0x20);
        self.m.set_i16(to + 0x20, v);
    }

    /// The tick interval for a range shown over `count` ticks: the size of one step,
    /// `span / count`, rounded down to a multiple of 0.0001, 0.001, ... 10000 by its size
    /// (a single digit's worth of that unit; above 10 the digit is a multiple of 10, 100 ...)
    /// with the sign of the quotient; 0 when the quotient is 0 [soft 0x083adac8 rulerIncSize].
    /// The result is the register value the original leaves in `st0`: a float value.
    ///
    /// The quotient stays in the 80-bit register (it is compared unrounded); every product
    /// is stored as a float, floored (`frndint` with the rounding control set to down), stored
    /// again, multiplied back by the unit and stored.
    pub fn ruler_inc_size(&self, span: f32, count: f32) -> F80 {
        let t1 = F80::from_f32(span) / count;
        let t2 = t1.abs();
        // product stored as float, floored, stored as float
        let step = |k: f64| -> f32 { F80::from_f32((t2 * k).to_f32()).round_int(Round::Down).to_f32() };
        let scaled = |f: f32, m: f64| -> f32 { (F80::from_f32(f) * m).to_f32() };
        let c = |v: f64| F80::from_f64(v);
        let r: f32 = if c(0.001).gt(t2) {
            scaled(step(10000.0), 0.0001)
        } else if c(0.01).gt(t2) {
            scaled(step(1000.0), 0.001)
        } else if c(0.1).gt(t2) {
            scaled(step(100.0), 0.01)
        } else if c(1.0).gt(t2) {
            scaled(step(10.0), 0.1)
        } else if t2.lt(c(10.0)) {
            t2.round_int(Round::Down).to_f32()
        } else if t2.lt(c(100.0)) {
            scaled(step(0.1), 10.0)
        } else if t2.lt(c(1000.0)) {
            scaled(step(0.01), 100.0)
        } else if t2.lt(c(10000.0)) {
            scaled(step(0.001), 1000.0)
        } else {
            let hi = scaled(step(0.0001), 10000.0);
            F80::from_f32(hi).round_int(Round::Down).to_f32()
        };
        let r = F80::from_f32(r);
        // negative quotients negate the result (`fchs` unless 0 <= quotient or unordered)
        if F80::from_i32(0).gt(t1) {
            -r
        } else {
            r
        }
    }

    /// One long tick of a ruler and its label: a short line across the edge at pixel
    /// offset `d` from the origin, then the value at the label position.
    fn ruler_tick_long(&mut self, g: Addr, horiz: bool, d: i32, label_x: F80, inc: f32, offset: i32) {
        let (g14, g18, g1c, g20) = (self.m.i16(g + 0x14) as i32, self.m.i16(g + 0x18) as i32, self.m.i16(g + 0x1c) as i32, self.m.i16(g + 0x20) as i32);
        let at = if horiz { g1c.wrapping_add(d) } else { g20.wrapping_add(d) };
        self.o.gl.bgnline(&mut self.m);
        if horiz {
            self.o.gl.v2i(&mut self.m, [at, g20.wrapping_sub(10)]);
            self.o.gl.v2i(&mut self.m, [at, g20.wrapping_sub(2)]);
        } else {
            self.o.gl.v2i(&mut self.m, [g1c.wrapping_sub(10), at]);
            self.o.gl.v2i(&mut self.m, [g1c.wrapping_sub(2), at]);
        }
        self.o.gl.endline(&mut self.m);
        if horiz {
            self.o.gl.cmov2i(&mut self.m, at.wrapping_sub(offset), g18.wrapping_add(5));
        } else {
            self.o.gl.cmov2i(&mut self.m, g14.wrapping_add(5), at.wrapping_sub(3));
        }
        let text = self.ruler_string_format(label_x.to_f32(), inc);
        self.o.gl.charstr(&mut self.m, &text);
    }

    /// One short tick.
    fn ruler_tick_short(&mut self, g: Addr, horiz: bool, d: i32) {
        let (g1c, g20) = (self.m.i16(g + 0x1c) as i32, self.m.i16(g + 0x20) as i32);
        self.o.gl.bgnline(&mut self.m);
        if horiz {
            let px = g1c.wrapping_add(d);
            self.o.gl.v2i(&mut self.m, [px, g20.wrapping_sub(5)]);
            self.o.gl.v2i(&mut self.m, [px, g20.wrapping_sub(2)]);
        } else {
            let py = g20.wrapping_add(d);
            self.o.gl.v2i(&mut self.m, [g1c.wrapping_sub(5), py]);
            self.o.gl.v2i(&mut self.m, [g1c.wrapping_sub(2), py]);
        }
        self.o.gl.endline(&mut self.m);
    }

    /// The ticks of `rulerDrawHoriz` and `rulerDrawVert`, which differ only in the axis they
    /// draw on: `[a, b]` is the visible range, `w` the extent in pixels, `v7` the range as a
    /// float, `inc` the tick interval from [`Ui::ruler_inc_size`] (not 0).
    fn ruler_ticks(&mut self, g: Addr, horiz: bool, (a, b): (f32, f32), w: i32, v7: f32, inc: F80, offset: i32) {
        let (ae, be) = (F80::from_f32(a), F80::from_f32(b));
        let inc32 = inc.to_f32();
        let ascending = ae.lt(be);
        // the first tick: the lower limit less its remainder modulo the interval; the remainder
        // goes through a double and a float
        let rf = ae.fmod(if ascending { inc } else { inc.abs() }).to_f64() as f32;
        let rfe = F80::from_f32(rf);
        let t = ae - rfe;
        // a tick is drawn while it has not passed the far limit (an unordered value ends it)
        let go = |v: F80| if ascending { v.le(be) } else { v.ge(be) };
        let wide = F80::from_i32(w);
        let wide32 = F80::from_f32(w as f32);
        let v7e = F80::from_f32(v7);
        let v7abs = F80::from_f64(v7.abs() as f64);
        // pixel offset of a value, truncated; the ascending minor ticks multiply by the width
        // as a float
        let pixels = |x: F80, minor: bool| -> i32 {
            let q = if ascending { ((x - ae) * if minor { wide32 } else { wide }) / v7e } else { ((ae - x) * wide) / v7abs };
            q.to_i32(Round::Zero)
        };
        // long ticks with labels
        if go(t) {
            let n = t + inc32;
            let mut stored = n.to_f32();
            if !n.eq(t) {
                let mut x = t;
                loop {
                    let d = pixels(x, false);
                    self.ruler_tick_long(g, horiz, d, x, inc32, offset);
                    let cur = F80::from_f32(stored);
                    if !go(cur) {
                        break;
                    }
                    let next = cur + inc32;
                    stored = next.to_f32();
                    if next.eq(cur) {
                        break;
                    }
                    x = cur;
                }
            }
        }
        // short ticks, starting half an interval on
        let half = F80::from_f64(0.5) * inc32;
        let y = if ascending { (half + (ae - rfe)).to_f32() } else { ((ae - rfe) - half).to_f32() };
        let ye = F80::from_f32(y);
        if go(ye) {
            let n = ye + inc32;
            let mut stored = n.to_f32();
            if !n.eq(ye) {
                let mut x = ye;
                loop {
                    let d = pixels(x, true);
                    self.ruler_tick_short(g, horiz, d);
                    let cur = F80::from_f32(stored);
                    if !go(cur) {
                        break;
                    }
                    let next = cur + inc32;
                    stored = next.to_f32();
                    if next.eq(cur) {
                        break;
                    }
                    x = cur;
                }
            }
        }
    }

    /// `color(SI_viewRulerBack)` / `color(SI_viewRuler)`.
    fn ruler_colour(&mut self, back: bool) {
        let c = if back { self.m[SI_VIEW_RULER_BACK] } else { self.m[SI_VIEW_RULER] };
        self.o.gl.color(&mut self.m, c as u16);
    }

    /// The five `ortho2`, `viewport` ... calls that every ruler routine opens its box with:
    /// the box is `x1..x2`, `y1..y2` (the drawing routines have already filled it).
    fn ruler_frame(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.o.gl.pushviewport(&mut self.m);
        self.o.gl.pushmatrix(&mut self.m);
        let half = |v: i32, plus: bool| if plus { (v as f64 + 0.5) as f32 } else { (v as f64 - 0.5) as f32 };
        self.o.gl.ortho2(&mut self.m, half(x1, false), half(x2, true), half(y1, false), half(y2, true));
        self.o.gl.viewport(&mut self.m, x1 as i16, x2 as i16, y1 as i16, y2 as i16);
    }

    /// Draw a horizontal ruler along the bottom of its view: the box filled with the
    /// ruler-back colour, outlined, and the ticks in a viewport of their own
    /// [soft 0x083acc04 rulerDrawHoriz]. A ruler whose interval is 0 draws no ticks.
    pub fn ruler_draw_horiz(&mut self, r: Addr) {
        let g = self.m.u32(r);
        self.ruler_colour(true);
        let (x1, x2, y1) = (self.m.i16(g + 0x14) as i32, self.m.i16(g + 0x16) as i32, self.m.i16(g + 0x18) as i32);
        let y2 = y1.wrapping_add(self.m.u32(r + 0x10) as i32).wrapping_sub(1);
        self.ctl_sboxfi(x1, y1, x2, y2);
        self.ruler_colour(false);
        self.o.gl.sboxi(&mut self.m, x1, y1, x2, y2);
        let mut left = x1.wrapping_sub(1);
        let scale = self.m.u32(g + 0x48);
        if scale != 0 {
            left = left.wrapping_add(self.m.u32(scale + 0x10) as i32);
        }
        self.o.gl.sboxi(&mut self.m, left, y1, x2, y2);
        self.ruler_frame(left, y1, x2, y2);
        let out = self.m.local(8);
        self.si_window_limits_horiz(P::at(g), out);
        self.ruler_colour(false);
        let (a, b) = (self.m.f32(out), self.m.f32(out + 4));
        self.m.release(8);
        let count = self.m.u32(r + 0x1c) as i32;
        let v7 = (F80::from_f32(b) - a).to_f32();
        let inc = self.ruler_inc_size(v7, count as f32);
        if !inc.eq(F80::from_i32(0)) {
            let w = (self.m.i16(g + 0x1e) as i32).wrapping_sub(self.m.i16(g + 0x1c) as i32);
            let offset = self.ruler_string_offset(inc.abs().to_f32());
            self.ruler_ticks(g, true, (a, b), w, v7, inc, offset);
        }
        self.o.gl.popviewport(&mut self.m);
        self.o.gl.popmatrix(&mut self.m);
    }

    /// Draw a vertical ruler along the left of its view: as the horizontal one, the labels
    /// to the right of the ticks [soft 0x083ad3a8 rulerDrawVert].
    pub fn ruler_draw_vert(&mut self, r: Addr) {
        let g = self.m.u32(r);
        self.ruler_colour(true);
        let x1 = self.m.i16(g + 0x14) as i32;
        let x2 = x1.wrapping_add(self.m.u32(r + 0x10) as i32).wrapping_sub(1);
        let mut y1 = self.m.i16(g + 0x18) as i32;
        let scale = self.m.u32(g + 0x4c);
        if scale != 0 {
            y1 = y1.wrapping_add(self.m.u32(scale + 0x10) as i32);
        }
        let y1 = y1.wrapping_sub(1);
        let y2 = (self.m.i16(g + 0x1a) as i32).wrapping_sub(16);
        self.ctl_sboxfi(x1, y1, x2, y2);
        self.ruler_frame(x1, y1, x2, y2);
        self.ruler_colour(false);
        self.o.gl.sboxi(&mut self.m, x1, y1, x2, y2);
        let out = self.m.local(8);
        self.si_window_limits_vert(P::at(g), out);
        let (a, b) = (self.m.f32(out), self.m.f32(out + 4));
        self.m.release(8);
        let count = self.m.u32(r + 0x1c) as i32;
        let v7 = (F80::from_f32(b) - a).to_f32();
        let inc = self.ruler_inc_size(v7, count as f32);
        if !inc.eq(F80::from_i32(0)) {
            let w = (self.m.i16(g + 0x22) as i32).wrapping_sub(self.m.i16(g + 0x20) as i32);
            self.ruler_ticks(g, false, (a, b), w, v7, inc, 0);
        }
        self.o.gl.popviewport(&mut self.m);
        self.o.gl.popmatrix(&mut self.m);
    }

    /// The setting-up half of the vertical rulers' drawing for a view, shared by the dope
    /// sheet's and the sound ruler's `SetView` routines (the two functions are the same code):
    /// fills the box, sets the viewport and projection, and finds the tick interval;
    /// true when there is a tick interval, else the viewport is put back and false.
    fn ruler_vert_set_view(&mut self, g: Addr) -> bool {
        self.ruler_colour(true);
        let x1 = self.m.i16(g + 0x14) as i32;
        let (p48, p4c) = (self.m.u32(g + 0x48), self.m.u32(g + 0x4c));
        let x2 = if p48 != 0 { x1.wrapping_add(self.m.u32(p48 + 0x10) as i32) } else { x1 }.wrapping_sub(1);
        let y1 = (self.m.i16(g + 0x18) as i32).wrapping_add(if p4c != 0 { self.m.u32(p4c + 0x10) as i32 } else { 0 }).wrapping_sub(1);
        let y2 = (self.m.i16(g + 0x1a) as i32).wrapping_sub(16);
        self.ctl_sboxfi(x1, y1, x2, y2);
        self.ruler_frame(x1, y1, x2, y2);
        self.ruler_colour(false);
        self.o.gl.sboxi(&mut self.m, x1, y1, x2, y2);
        let out = self.m.local(8);
        self.si_window_limits_vert(P::at(g), out);
        let (a, b) = (self.m.f32(out), self.m.f32(out + 4));
        self.m.release(8);
        // the ruler record of the view holds the tick length (the original reads it from the
        // record the window's +0x48 points to, as it does for the box's width)
        let count = if p48 != 0 { self.m.u32(p48 + 0x1c) as i32 } else { 0 };
        let inc = self.ruler_inc_size((F80::from_f32(b) - a).to_f32(), count as f32);
        if inc.eq(F80::from_i32(0)) {
            self.o.gl.popviewport(&mut self.m);
            self.o.gl.popmatrix(&mut self.m);
            false
        } else {
            true
        }
    }

    /// Set up the view for the dope sheet's ruler; true when it can be drawn
    /// [soft 0x083adf9c SI_rulerVertDopeSetView].
    pub fn si_ruler_vert_dope_set_view(&mut self, g: Addr) -> bool {
        self.ruler_vert_set_view(g)
    }

    /// Set up the view for the sound ruler; true when it can be drawn
    /// [soft 0x083ae120 SI_rulerVertSoundSetView].
    pub fn si_ruler_vert_sound_set_view(&mut self, g: Addr) -> bool {
        self.ruler_vert_set_view(g)
    }

    /// The text of a tick label: the value printed with as many decimals as the interval
    /// between ticks needs, `%4.4f` below 0.001, `%3.3f`, `%2.2f`, `%1.1f` below 1, `%2.1f`
    /// below 10, then `%3.0f` to `%6.0f` at 100, 1000, 10000 and above
    /// [soft 0x083add94 rulerStringFormat]. Returns the text for the buffer.
    pub fn ruler_string_format(&mut self, value: f32, interval: f32) -> Vec<u8> {
        let a = interval.abs() as f64;
        let v = value as f64;
        let (w, p) = if a < 0.001 {
            (4, 4)
        } else if a < 0.01 {
            (3, 3)
        } else if a < 0.1 {
            (2, 2)
        } else if a < 1.0 {
            (1, 1)
        } else if a < 10.0 {
            (2, 1)
        } else if a < 100.0 {
            (3, 0)
        } else if a < 1000.0 {
            (4, 0)
        } else if a < 10000.0 {
            (5, 0)
        } else {
            (6, 0)
        };
        format_f(w, p, v)
    }

    /// How far a tick label sits from its tick, by the size of the value: 25 below 0.001,
    /// 20 below 0.01, 15 below 0.1, 10 below 1, 15 below 10, 20 below 100, else 25
    /// [soft 0x083adec8 rulerStringOffset].
    pub fn ruler_string_offset(&self, v: f32) -> i32 {
        let v = v as f64;
        if v < 0.001 {
            25
        } else if v < 0.01 {
            20
        } else if v < 0.1 {
            15
        } else if v < 1.0 {
            10
        } else if v < 10.0 {
            15
        } else if v < 100.0 {
            20
        } else {
            25
        }
    }
}
