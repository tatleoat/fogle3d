// SPDX-License-Identifier: MIT
//! `scroll.c` of the program, first slice (0x083ae2e0 to 0x083b6adb): the small routines.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! Ported: `SI_scrollSetInterval`, `SI_SetColSliderRefresh`, `SI_scrollGetDrawFuncPtr`, and the draw
//! functions of the graph area, the h/v slider and the static label (each with its one-line wrapper).
//! The rest of the slice (the draw functions of the controls and `SI_scrollRead`) is not
//! ported yet.

use crate::globals::*;
use crate::mem::{Addr, P};
use crate::records::Scroll;
use crate::Ui;
use si_x87::{Round, F80};

/// The address of `si_refreshLightArea` (0x083db9ac) in the program's image: the routine
/// the colour sliders call back for a refresh. Only ever stored, never called, by this file.
const REFRESH_LIGHT_AREA: u32 = 0x083d_b9ac;

impl Ui {
    /// The record of control number `index` (counted from 0) of a dialog: the dialog's control
    /// array (+0x14), 0x28 bytes each.
    fn dialog_control(&self, dialog: Addr, index: i32) -> Addr {
        self.m.u32(dialog.wrapping_add(0x14)).wrapping_add(index.wrapping_mul(0x28) as u32)
    }

    /// Set the interval of a slider control (kind 9 or 10, number `item` counted from 0) of a dialog to `[lo, hi]`
    /// [soft 0x083ae2e0 SI_scrollSetInterval]: the two floats go into the slider's data
    /// (+4, +8). The data's first word names another control of the dialog (counted from 1, when it is in
    /// 1 ..= the dialog's control count); if that one is a text field (kind 5) its data gets
    /// the same two floats (+0x14, +0x18) and, unless the dialog's +0x1c is -1, the field is
    /// redrawn by `SI_sliderSet` from its text.
    ///
    /// The floats pass through the 80-bit registers, which quiets a signalling NaN.
    pub fn si_scroll_set_interval(&mut self, dialog: Addr, item: i32, lo: f32, hi: f32) {
        let (lo, hi) = (F80::from_f32(lo).to_f32(), F80::from_f32(hi).to_f32());
        let ctl = self.dialog_control(dialog, item);
        if self.m.u32(ctl).wrapping_sub(9) > 1 {
            return;
        }
        let data = self.m.u32(ctl.wrapping_add(0x10));
        if data == 0 {
            return;
        }
        self.m.set_f32(data.wrapping_add(4), lo);
        self.m.set_f32(data.wrapping_add(8), hi);
        let other = self.m.i16(data) as i32;
        if other <= 0 || other > self.m.i16(dialog.wrapping_add(0xc)) as i32 {
            return;
        }
        let ctl2 = self.dialog_control(dialog, other - 1);
        if self.m.u32(ctl2) != 5 {
            return;
        }
        let data2 = self.m.u32(ctl2.wrapping_add(0x10));
        self.m.set_f32(data2.wrapping_add(0x14), lo);
        self.m.set_f32(data2.wrapping_add(0x18), hi);
        if self.m.u32(dialog.wrapping_add(0x1c)) != 0xffff_ffff {
            let text = self.m.u32(data2);
            let item2 = self.m.i16(data2.wrapping_add(0x1c));
            self.si_slider_set(dialog, item2, text, 0);
        }
    }

    /// Install the refresh routine of a colour slider [soft 0x083b093c SI_SetColSliderRefresh]:
    /// for a control of kind 13 whose window's first refresh slot (+0x108 of the record
    /// the window's +0x14 points to) is still empty, the routine `refresh` is stored in four
    /// slots of that record (+0x108, +0x1a8, +0x248, +0x2e8, the four buffers). A null
    /// `refresh` installs `si_refreshLightArea` when the control's word at +0x46 is non-zero.
    pub fn si_set_col_slider_refresh(&mut self, control: Addr, refresh: u32) {
        let data = self.m.u32(control.wrapping_add(0x10));
        if self.m.u32(control) != 13 {
            return;
        }
        let window = self.m.u32(data.wrapping_add(0x3c));
        let slots = self.m.u32(window.wrapping_add(0x14));
        if self.m.u32(slots.wrapping_add(0x108)) != 0 {
            return;
        }
        let mut refresh = refresh;
        if refresh == 0 && self.m.i16(data.wrapping_add(0x46)) != 0 {
            refresh = REFRESH_LIGHT_AREA;
        }
        for off in [0x108u32, 0x1a8, 0x248, 0x2e8] {
            let slots = self.m.u32(window.wrapping_add(0x14));
            self.m.set_u32(slots.wrapping_add(off), refresh);
        }
    }

    /// The draw routine for a control kind [soft 0x083b2fa0 SI_scrollGetDrawFuncPtr]: the
    /// program's address of the radio circle (kind 4), check box (3), button (2) or notifier
    /// (8) draw routine, 0 for any other kind. The addresses are those of the original's image.
    pub fn si_scroll_get_draw_func_ptr(&self, kind: i32) -> u32 {
        match kind {
            4 => 0x083b_5294,
            3 => 0x083b_4758,
            2 => 0x083b_2fe8,
            8 => 0x083b_3d94,
            _ => 0,
        }
    }

    /// The graph area's draw routine [soft 0x083b436c scrollGrafDraw]: [`Ui::scroll_graf_draw_gl`].
    pub fn scroll_graf_draw(&mut self, p: P<Scroll>) {
        self.scroll_graf_draw_gl(p)
    }

    /// A graph area: its box outlined in "light" when the control's data starts with a zero
    /// word, else in "ghost" when the control is greyed out (state 1), else in "dark"
    /// [soft 0x083b4384 scrollGrafDrawGL].
    pub fn scroll_graf_draw_gl(&mut self, p: P<Scroll>) {
        let s = self.m[p];
        if self.m.i16(s.data) == 0 {
            let x = self.m[SI_DIALOG_LIGHT];
            self.ctl(x, true);
        } else if self.m[p].state == 1 {
            let x = self.m[SI_DIALOG_GHOST];
            self.ctl(x, false);
        } else {
            let x = self.m[SI_DIALOG_DARK];
            self.ctl(x, true);
        }
        self.o.gl.sboxi(&mut self.m, s.x1 as i32, s.y1 as i32, s.x2 as i32, s.y2 as i32);
    }

    /// The h/v slider's draw routine [soft 0x083b5d44 scrollHvSliderDraw]: [`Ui::scroll_hv_slider_draw_gl`].
    pub fn scroll_hv_slider_draw(&mut self, p: P<Scroll>) {
        self.scroll_hv_slider_draw_gl(p)
    }

    /// An h/v slider: its box outlined (`sboxs`) in "ghost" when the control is greyed out
    /// (state 1), else in "dark" [soft 0x083b5d5c scrollHvSliderDrawGL].
    pub fn scroll_hv_slider_draw_gl(&mut self, p: P<Scroll>) {
        if self.m[p].state == 1 {
            let x = self.m[SI_DIALOG_GHOST];
            self.ctl(x, false);
        } else {
            let x = self.m[SI_DIALOG_DARK];
            self.ctl(x, true);
        }
        let s = self.m[p];
        self.o.gl.sboxs(&mut self.m, s.x1, s.y1, s.x2, s.y2);
    }

    /// The static label's draw routine [soft 0x083b1ff0 scrollStaticDraw]: [`Ui::scroll_static_draw_gl`].
    pub fn scroll_static_draw(&mut self, p: P<Scroll>) {
        self.scroll_static_draw_gl(p)
    }

    /// A static label: the box (three rows lower than its bottom edge) filled in "light"
    /// through the `SI_sboxfi` pointer, the label (the string behind the control's data) written at
    /// the bottom-left corner in "ghost" when greyed out, else in "dark"; a control in state 2
    /// then gets its box filled again with the dither pattern 1 in "dark"
    /// [soft 0x083b2008 scrollStaticDrawGL].
    pub fn scroll_static_draw_gl(&mut self, p: P<Scroll>) {
        let s = self.m[p];
        let (x1, y1, x2, y2) = (s.x1 as i32, s.y1 as i32, s.x2 as i32, s.y2 as i32);
        let x = self.m[SI_DIALOG_LIGHT];
        self.ctl(x, false);
        self.ctl_sboxfi(x1, y1 - 3, x2, y2);
        if self.m[p].state == 1 {
            let x = self.m[SI_DIALOG_GHOST];
            self.ctl(x, false);
        } else {
            let x = self.m[SI_DIALOG_DARK];
            self.ctl(x, true);
        }
        self.o.gl.cmov2i(&mut self.m, x1, y1);
        let label = self.m.u32(s.data);
        let text = self.m.cstr(label);
        self.o.gl.charstr(&mut self.m, &text);
        if self.m[p].state == 2 {
            self.o.gl.setpattern(&mut self.m, 1);
            let x = self.m[SI_DIALOG_DARK];
            self.ctl(x, true);
            self.ctl_sboxfi(x1, y1, x2, y2);
            self.o.gl.setpattern(&mut self.m, 0);
        }
    }

    /// The left arrow of a text-edit field's scroll [soft 0x083b1dbc scrollTexedLeftArrow]:
    /// [`Ui::scroll_texed_left_arrow_gl`].
    pub fn scroll_texed_left_arrow(&mut self, p: P<Scroll>, filled: i16) {
        self.scroll_texed_left_arrow_gl(p, filled)
    }

    /// The right arrow [soft 0x083b1ed4 scrollTexedRightArrow]: [`Ui::scroll_texed_right_arrow_gl`].
    pub fn scroll_texed_right_arrow(&mut self, p: P<Scroll>, filled: i16) {
        self.scroll_texed_right_arrow_gl(p, filled)
    }

    /// The left arrow: a line down the box's left edge, then a triangle pointing left of it,
    /// its tip 13 pixels out at half the height (truncated), outlined when `filled` is 0, else
    /// filled [soft 0x083b1dd8 scrollTexedLeftArrowGL].
    pub fn scroll_texed_left_arrow_gl(&mut self, p: P<Scroll>, filled: i16) {
        self.scroll_texed_arrow(p, filled, false)
    }

    /// The right arrow, the mirror image on the right edge [soft 0x083b1ef0 scrollTexedRightArrowGL].
    pub fn scroll_texed_right_arrow_gl(&mut self, p: P<Scroll>, filled: i16) {
        self.scroll_texed_arrow(p, filled, true)
    }

    /// The two arrows are the same code mirrored: `right` picks the right edge (+8) and
    /// the offsets 13 and 2 outward instead of the left edge (+6) and -13, -2.
    fn scroll_texed_arrow(&mut self, p: P<Scroll>, filled: i16, right: bool) {
        let s = self.m[p];
        let (edge, sign) = if right { (s.x2 as i32, 1) } else { (s.x1 as i32, -1) };
        let (y1, y2) = (s.y1 as i32, s.y2 as i32);
        self.o.gl.bgnline(&mut self.m);
        self.o.gl.v2i(&mut self.m, [edge, y2]);
        self.o.gl.v2i(&mut self.m, [edge, y1]);
        self.o.gl.endline(&mut self.m);
        let tip_x = edge.wrapping_add(13 * sign);
        // half the height, truncated toward zero (the original sets the rounding control)
        let half = (F80::from_i32(y2.wrapping_sub(y1)) * 0.5f64).to_i32(Round::Zero);
        let tip_y = y1.wrapping_add(half);
        if filled == 0 {
            self.o.gl.bgnclosedline(&mut self.m);
        } else {
            self.o.gl.bgnpolygon(&mut self.m);
        }
        self.o.gl.v2i(&mut self.m, [tip_x, tip_y]);
        self.o.gl.v2i(&mut self.m, [edge + 2 * sign, y2 - 2]);
        self.o.gl.v2i(&mut self.m, [edge + 2 * sign, y1 + 2]);
        if filled == 0 {
            self.o.gl.endclosedline(&mut self.m);
        } else {
            self.o.gl.endpolygon(&mut self.m);
        }
    }
}
