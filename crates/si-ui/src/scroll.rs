// SPDX-License-Identifier: MIT
//! `scroll.c` of the program, second slice (0x083b6adc to 0x083bb6ee): the draw functions of
//! the text list with its scroll bar and of the tuning button, and `SI_scrollSize`.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! The drawing code picks every colour with one macro that the compiler expanded at each
//! site. In colour-map mode (`SI_rgbmode == 0`) it maps a role's colour index: the "back"
//! role is drawn in the "light" colour, the "title" role in "dark", the "ghost" role in
//! "gray", and at some sites the "button" role in "light"; every other value is used as it
//! is. In RGB mode it draws black for the "clear" role, otherwise the RGB triple of the
//! first palette entry whose role index equals the value (dark, gray, light, back, button,
//! title, ghost), and nothing when none does. The two helpers [`Ui::ctl`] and
//! [`Ui::ctl_with`] (in `ctl_colour.rs`) are that macro; the sites say which value each colour is picked for.

use crate::globals::*;
use crate::mem::{Addr, P};
use crate::records::{Scroll, ScrollData};
use crate::Ui;
use si_x87::{Round, F80};

impl Ui {
    fn sboxfi_(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        // `SI_sboxfi`: see window.rs
        self.o.gl.sboxfi(&mut self.m, x1, y1, x2, y2);
    }

    /// Draw a text list's frame [soft 0x083b6adc si_scrollTextViewFrameDraw]: [`Ui::scroll_text_view_frame_draw_gl`].
    pub fn si_scroll_text_view_frame_draw(&mut self, p: P<Scroll>, p2: i16) {
        self.scroll_text_view_frame_draw_gl(p, p2)
    }

    /// The frame of a text list and, through [`Ui::si_scroll_text_view_draw`], its contents
    /// [soft 0x083b6af8 scrollTextViewFrameDrawGL]. With `p2` zero (the list is in a
    /// window of the colour-map) the write mask is set to 0xfff meanwhile and restored.
    pub fn scroll_text_view_frame_draw_gl(&mut self, p: P<Scroll>, p2: i16) {
        let s = self.m[p];
        let (x1, y1, x2, y2) = (s.x1 as i32, s.y1 as i32, s.x2 as i32, s.y2 as i32);
        let mut saved = 0u16;
        if p2 == 0 {
            if y2 - 0x14 < y1 || x2 - 0x14 < x1 {
                return;
            }
            saved = self.o.gl.getwritemask(&mut self.m) as u16;
            self.o.gl.writemask(&mut self.m, 0xfff);
        }
        // the border and the frame use the window text colour on an 8-plane display,
        // otherwise the dark colour (the ghost one when the control is greyed out)
        let text = p2 == 0 && self.m[SI_BIT_PLANES] == 8;
        let edge = if s.state == 1 { self.m[SI_DIALOG_GHOST] } else { self.m[SI_DIALOG_DARK] };
        let x = if text { self.m[SI_WINDOW_TEXT] } else { edge };
        self.ctl(x, true);
        self.o.gl.sboxi(&mut self.m, x1, y1, x2, y2);
        if p2 == 0 {
            let x = self.m[SI_VIEW_RULER_BACK];
            self.ctl(x, true);
        } else {
            let x = self.m[SI_DIALOG_LIGHT];
            self.ctl(x, false);
        }
        self.sboxfi_(x1 + 1, y1 + 1, x2 - 1, y2 - 1);
        let x = if text { self.m[SI_WINDOW_TEXT] } else { edge };
        self.ctl(x, true);
        self.o.gl.sboxi(&mut self.m, x2, y1, x2 + 0x14, y2);
        self.o.gl.sboxi(&mut self.m, x2, y1, x2 + 0x14, y1 + 0x14);
        self.o.gl.sboxi(&mut self.m, x2, y2 - 0x14, x2 + 0x14, y2);
        self.si_scroll_text_view_draw(p, p2, 0);
        if p2 == 0 {
            self.o.gl.writemask(&mut self.m, saved);
        }
    }

    /// [soft 0x083b78d0 SI_scrollTextViewDraw]: [`Ui::scroll_text_view_draw_gl`].
    pub fn si_scroll_text_view_draw(&mut self, p: P<Scroll>, p2: i16, p3: i16) {
        self.scroll_text_view_draw_gl(p, p2, p3)
    }

    /// The rows of a text list with its scroll bar [soft 0x083b78f0 scrollTextViewDrawGL].
    /// `p2` is zero for a window of the colour-map (the front buffer is then used when `p3`
    /// is set, and rows are drawn through the select write mask), non-zero otherwise.
    pub fn scroll_text_view_draw_gl(&mut self, p: P<Scroll>, p2: i16, p3: i16) {
        let s = self.m[p];
        let data = s.data;
        let d = self.m[P::<ScrollData>::at(data)];
        let items = d.items;
        let (x1, x2) = (s.x1 as i32, s.x2 as i32);
        let visible = d.visible as i32;
        let count = d.count as i32;
        let saved = self.o.gl.getwritemask(&mut self.m) as u16;
        // how many characters of a row's text fit, as an unsigned count
        let fit: u32 = if p2 == 0 {
            if p3 != 0 {
                self.o.gl.frontbuffer(&mut self.m, 1);
            }
            let font = self.m[SI_APPF];
            self.o.gl.font(&mut self.m, font);
            (F80::from_i32(x2 - (x1 - 5)) / 7.0f32).to_i32(Round::Zero) as u32
        } else {
            let v = F80::from_f32((F80::from_i32(x2 - (x1 + 0x14)) - 8.0f64).to_f32());
            (v / F80::from_i32(self.m[SI_TEXWIDTH] as i32)).to_i32(Round::Zero) as u32
        };
        self.scroll_slider_draw(p, p2, d.flag);
        self.o.gl.gsync(&mut self.m);
        self.o.gl.gsync(&mut self.m);
        self.o.gl.gsync(&mut self.m);
        let mut y = s.y2 as i32 - (self.m[SI_TEXASCEND] as i32 + 3);
        let mut row = d.first as i32;
        let mut done = 0i32;
        if count > 0 && visible > 0 {
            let mut item: Addr = items.wrapping_add((row as u32).wrapping_mul(0x14));
            loop {
                // the row's background
                if s.state == 1 {
                    let x = self.m[SI_DIALOG_LIGHT];
                    self.ctl(x, false);
                } else {
                    let fresh = self.m.i16(data.wrapping_add(8)) as i32;
                    let x = if row < fresh && self.m.i16(item) != 0 {
                        if self.m.i16(item.wrapping_add(2)) == 0 {
                            if p2 != 0 { self.m[SI_DIALOG_DARK] } else { self.m[SI_WINDOW_TEXT] }
                        } else {
                            self.m[SI_DIALOG_GHOST]
                        }
                    } else if p2 != 0 {
                        self.m[SI_DIALOG_LIGHT]
                    } else {
                        self.m[SI_VIEW_RULER_BACK]
                    };
                    self.ctl(x, true);
                }
                let before = done * -0x14;
                done += 1;
                let top = s.y2 as i32 + done * -0x14;
                self.sboxfi_(x1 + 1, top, x2 - 1, s.y2 as i32 + before - 1);
                if row < count {
                    let tx;
                    if p2 == 0 {
                        let mask = if self.m.i16(item) != 0 { self.m[SI_SELECT7_WM] } else { self.m[SI_UNSELECT_WM] };
                        self.o.gl.writemask(&mut self.m, mask as u16);
                        let at = self.m.u32(item.wrapping_add(0xc));
                        let c = self.m.u16(at);
                        self.o.gl.color(&mut self.m, c);
                        tx = x1 + 5;
                    } else {
                        if s.state == 1 {
                            let x = self.m[SI_DIALOG_GHOST];
                            self.ctl(x, false);
                        } else {
                            let a = self.m.i16(item);
                            let b = self.m.i16(item.wrapping_add(2));
                            let x = if a == 0 {
                                if b == 0 { self.m[SI_DIALOG_DARK] } else { self.m[SI_DIALOG_GHOST] }
                            } else if b == 0 {
                                self.m[SI_DIALOG_LIGHT]
                            } else {
                                self.m[SI_DIALOG_DARK]
                            };
                            self.ctl(x, true);
                        }
                        tx = x1 + 0x14;
                    }
                    self.o.gl.cmov2i(&mut self.m, tx, y);
                    let text = self.m.u32(item.wrapping_add(4));
                    if text != 0 {
                        let len = self.m.strlen(text);
                        if fit < len {
                            let at = text.wrapping_add(fit);
                            let kept = self.m.u8(at);
                            self.m.set_u8(at, 0);
                            let t = self.m.cstr(text);
                            self.o.gl.charstr(&mut self.m, &t);
                            let again = self.m.u32(item.wrapping_add(4));
                            self.m.set_u8(again.wrapping_add(fit), kept);
                        } else {
                            let t = self.m.cstr(text);
                            self.o.gl.charstr(&mut self.m, &t);
                        }
                    }
                    if p2 == 0 {
                        self.o.gl.writemask(&mut self.m, saved);
                    }
                    if self.m.u32(item.wrapping_add(0x10)) != 0 {
                        self.o.controls.scrollRowCallback(&mut self.m, items, row, x1 + 0x14, y);
                    }
                }
                y -= 0x14;
                item = item.wrapping_add(0x14);
                row += 1;
                if !(count > 0 && done < visible) {
                    break;
                }
            }
        }
        if p2 == 0 {
            if p3 != 0 {
                self.o.gl.frontbuffer(&mut self.m, 0);
            }
            let font = self.m[SI_IRISF];
            self.o.gl.font(&mut self.m, font);
        }
    }

    /// [soft 0x083b895c scrollSliderDraw]: [`Ui::scroll_slider_draw_gl`].
    pub fn scroll_slider_draw(&mut self, p: P<Scroll>, p2: i16, p3: i16) {
        self.scroll_slider_draw_gl(p, p2, p3)
    }

    /// The scroll bar right of a text list [soft 0x083b897c scrollSliderDrawGL]: with `p3`
    /// zero only the two arrow buttons (as a flat bar), otherwise the arrows, the shaft and
    /// the thumb. The data's `mode` (0, 1, 2 or other) picks which parts are dark.
    pub fn scroll_slider_draw_gl(&mut self, p: P<Scroll>, p2: i16, p3: i16) {
        let s = self.m[p];
        let (x2, y1, y2) = (s.x2 as i32, s.y1 as i32, s.y2 as i32);
        let d_at = s.data;
        let shaft = |this: &mut Ui| {
            let x = if p2 != 0 { this.m[SI_DIALOG_LIGHT] } else { this.m[SI_VIEW_RULER_BACK] };
            this.ctl(x, true);
        };
        let slider_colour = |this: &mut Ui| {
            let index = this.ctl_index(this.m[SI_SLIDER_COLOR], true);
            let gray = this.m[SI_DIALOG_GRAY];
            this.ctl_with(index, gray);
        };
        let down_arrow = |this: &mut Ui| {
            let (a, b) = (x2 + 5, y2 - 0xf);
            this.o.gl.bgnpolygon(&mut this.m);
            this.o.gl.v2i(&mut this.m, [a, b]);
            this.o.gl.v2i(&mut this.m, [a + 10, b]);
            this.o.gl.v2i(&mut this.m, [a + 5, b + 10]);
            this.o.gl.endpolygon(&mut this.m);
        };
        let up_arrow = |this: &mut Ui| {
            let (a, b) = (x2 + 5, y1 + 0xf);
            this.o.gl.bgnpolygon(&mut this.m);
            this.o.gl.v2i(&mut this.m, [a, b]);
            this.o.gl.v2i(&mut this.m, [a + 10, b]);
            this.o.gl.v2i(&mut this.m, [a + 5, b - 10]);
            this.o.gl.endpolygon(&mut this.m);
        };
        if p3 == 0 {
            shaft(self);
            self.sboxfi_(x2 + 1, y1 + 0x15, x2 + 0x13, y2 - 0x15);
            self.sboxfi_(x2 + 1, y1 + 1, x2 + 0x13, y1 + 0x13);
            self.sboxfi_(x2 + 1, y2 - 0x13, x2 + 0x13, y2 - 1);
            slider_colour(self);
            down_arrow(self);
            up_arrow(self);
            return;
        }
        let k = self.m.i16(d_at.wrapping_add(0xa));
        let (light, ruler, dark, text) =
            (self.m[SI_DIALOG_LIGHT], self.m[SI_VIEW_RULER_BACK], self.m[SI_DIALOG_DARK], self.m[SI_WINDOW_TEXT]);
        // the colour of each part by mode: a "lit" part is light (ruler colour in the
        // colour-map window), a dark one dark (window text colour)
        let lit = if p2 != 0 { light } else { ruler };
        let drk = if p2 != 0 { dark } else { text };
        let by_mode = |lit_modes: &[i16], dark_modes: &[i16]| -> i16 {
            if lit_modes.contains(&k) {
                lit
            } else if dark_modes.contains(&k) {
                drk
            } else {
                0
            }
        };
        // the lower arrow's button, the upper one's, then the arrows themselves
        let v = by_mode(&[0, 2], &[1]);
        self.ctl(v, true);
        self.sboxfi_(x2 + 1, y2 - 0x13, x2 + 0x13, y2 - 1);
        let v = by_mode(&[0, 1], &[2]);
        self.ctl(v, true);
        self.sboxfi_(x2 + 1, y1 + 1, x2 + 0x13, y1 + 0x13);
        let arrow = |this: &mut Ui, v: i16| {
            if s.state == 1 {
                let g = this.m[SI_DIALOG_GHOST];
                this.ctl(g, false);
            } else {
                this.ctl(v, true);
            }
        };
        let v = by_mode(&[1], &[0, 2]);
        arrow(self, v);
        down_arrow(self);
        let v = by_mode(&[2], &[0, 1]);
        arrow(self, v);
        up_arrow(self);
        // the thumb
        let first = self.m.i16(d_at.wrapping_add(6));
        let rows = self.m.i16(d_at.wrapping_add(0xc));
        let scale = f32::from_bits(self.m.u32(d_at.wrapping_add(0x10)));
        let off = (F80::from_i32(first as i32) * scale).to_i32(Round::Zero) as i16;
        let mut top = (y2 - 0x17 - off as i32) as i16;
        let span = (F80::from_i32(rows as i32) * scale).to_i32(Round::Zero);
        let mut bottom = top.wrapping_sub(span as i16);
        if (top as i32) - (bottom as i32) < 0xf {
            bottom = bottom.wrapping_sub(0xf);
        }
        let lo = (y1 + 0x17) as i16;
        if bottom < lo {
            bottom = lo;
            if (top as i32) < lo as i32 + 0xf {
                top = (y1 + 0x26) as i16;
            }
        }
        shaft(self);
        self.sboxfi_(x2 + 1, y1 + 0x15, x2 + 0x13, y2 - 0x15);
        slider_colour(self);
        self.sboxfi_(x2 + 3, bottom as i32, x2 + 0x11, top as i32);
    }

    /// [soft 0x083bad24 scrollTuningButtonDraw]: [`Ui::scroll_tuning_button_draw_gl`].
    pub fn scroll_tuning_button_draw(&mut self, p: P<Scroll>) {
        self.scroll_tuning_button_draw_gl(p)
    }

    /// The tuning button: a box with a line across its middle and a triangle above and
    /// below it [soft 0x083bad3c scrollTuningButtonDrawGL]. Pressed (the control's state is
    /// 1) it is drawn light with a ghost border, otherwise back-coloured with a dark border.
    pub fn scroll_tuning_button_draw_gl(&mut self, p: P<Scroll>) {
        let s = self.m[p];
        let (x1, y1, x2, y2) = (s.x1 as i32, s.y1 as i32, s.x2 as i32, s.y2 as i32);
        if s.state == 1 {
            let l = self.m[SI_DIALOG_LIGHT];
            self.ctl(l, false);
            self.sboxfi_(x1 + 2, y1 + 2, x2 - 2, y2 - 2);
            let g = self.m[SI_DIALOG_GHOST];
            self.ctl(g, false);
            self.o.gl.sboxi(&mut self.m, x1, y1, x2, y2);
            self.ctl(g, false);
        } else {
            let index = self.m[SI_DIALOG_LIGHT] as u16;
            let back = self.m[SI_DIALOG_BACK];
            self.ctl_with(index, back);
            self.sboxfi_(x1 + 2, y1 + 2, x2 - 2, y2 - 2);
            let d = self.m[SI_DIALOG_DARK];
            self.ctl(d, true);
            self.o.gl.sboxi(&mut self.m, x1, y1, x2, y2);
            self.ctl(d, true);
        }
        let half = |a: i32, b: i32| ((b - a) as f64 * 0.5) as i32;
        let mid = y1 + half(y1, y2) as i16 as i32;
        self.o.gl.bgnline(&mut self.m);
        self.o.gl.v2i(&mut self.m, [x1, mid]);
        self.o.gl.v2i(&mut self.m, [x2, mid]);
        self.o.gl.endline(&mut self.m);
        let (h, w) = (self.m[SI_ARR_H_OFF], self.m[SI_ARR_W_OFF]);
        let xm = (x1 as f64 + (x2 - x1) as f64 * 0.5) as i32 as i16;
        let ym = (y1 as f64 + (y2 - y1) as f64 * 0.5) as i32 as i16;
        self.o.gl.bgnclosedline(&mut self.m);
        let y = ym.wrapping_sub(h);
        self.o.gl.v2s(&mut self.m, [xm.wrapping_sub(5), y]);
        self.o.gl.v2s(&mut self.m, [xm.wrapping_add(5), y]);
        let y = ym.wrapping_sub(w);
        self.o.gl.v2s(&mut self.m, [xm.wrapping_add(1), y]);
        self.o.gl.v2s(&mut self.m, [xm.wrapping_sub(1), y]);
        self.o.gl.endclosedline(&mut self.m);
        self.o.gl.bgnclosedline(&mut self.m);
        let y = h.wrapping_add(ym);
        self.o.gl.v2s(&mut self.m, [xm.wrapping_sub(5), y]);
        self.o.gl.v2s(&mut self.m, [xm.wrapping_add(5), y]);
        let y = ym.wrapping_add(w);
        self.o.gl.v2s(&mut self.m, [xm.wrapping_add(1), y]);
        self.o.gl.v2s(&mut self.m, [xm.wrapping_sub(1), y]);
        self.o.gl.endclosedline(&mut self.m);
    }

    /// The rectangle a control takes [soft 0x083bb630 SI_scrollSize]: its left, bottom, right
    /// and top edges. The kind 0 starts 3 lower; kinds 3 and 4 (buttons with a label, the
    /// label's string pointer behind `data`) are as wide as the label plus the button's
    /// round ends and 10, and twice the radius high; kind 7 is 0x14 wider; every other kind
    /// takes its record's edges as they are.
    pub fn si_scroll_size(&mut self, p: P<Scroll>) -> (i32, i32, i32, i32) {
        let s = self.m[p];
        let (x1, y1, x2, y2) = (s.x1 as i32, s.y1 as i32, s.x2 as i32, s.y2 as i32);
        match s.kind as u32 {
            0 => (x1, y1 - 3, x2, y2),
            3 | 4 => {
                let label = self.m.u32(s.data);
                let text = self.m.cstr(label);
                let w = self.o.gl.strwidth(&mut self.m, &text);
                let r = self.m[RADIUS];
                (x1, y1, x1.wrapping_add(r.wrapping_mul(2)).wrapping_add(10).wrapping_add(w), y1.wrapping_add(r.wrapping_mul(2)))
            }
            7 => (x1, y1, x2 + 0x14, y2),
            _ => (x1, y1, x2, y2),
        }
    }
}

