// SPDX-License-Identifier: MIT
//! The colour macro of the controls' drawing code (`scroll.c`, `slider.c`, `ruler.c`),
//! the same one `scroll.rs` has (its helpers are private there; the second slice of
//! `scroll.c` was ported by another unit, so this file repeats the rule rather than reach
//! into it). The compiler expanded the macro at every site; the sites differ only in
//! which role is drawn and whether the button role is also mapped.
//!
//! In colour-map mode (`SI_rgbmode == 0`) the role's colour index goes to `color`: the
//! "back" role is drawn in the "light" colour, "title" in "dark", "ghost" in "gray", and at
//! some sites "button" in "light". In RGB mode black is drawn for the "clear" role, else
//! the RGB triple of the first palette entry (dark, gray, light, back, button, title,
//! ghost) whose role equals the value, and nothing when none does.

use crate::globals::*;
use crate::Ui;

impl Ui {
    /// The colour index of role value `x` in colour-map mode; `button` says whether the
    /// site also maps the button role to "light".
    pub(crate) fn ctl_index(&self, x: i16, button: bool) -> u16 {
        if x == self.m[SI_DIALOG_BACK] {
            self.m[SI_DIALOG_LIGHT] as u16
        } else if x == self.m[SI_DIALOG_TITLE] {
            self.m[SI_DIALOG_DARK] as u16
        } else if x == self.m[SI_DIALOG_GHOST] {
            self.m[SI_DIALOG_GRAY] as u16
        } else if button && x == self.m[SI_DIALOG_BUT] {
            self.m[SI_DIALOG_LIGHT] as u16
        } else {
            x as u16
        }
    }

    /// Set the colour: in colour-map mode `index`, in RGB mode the palette entry of `x`.
    pub(crate) fn ctl_with(&mut self, index: u16, x: i16) {
        if self.m[SI_RGBMODE] == 0 {
            self.o.gl.color(&mut self.m, index);
            return;
        }
        if x == self.m[SI_DIALOG_CLEAR] {
            self.o.gl.RGBcolor(&mut self.m, 0, 0, 0);
            return;
        }
        let (r, g, b) = if x == self.m[SI_DIALOG_DARK] {
            (self.m[SI_RGB_DIAL_DARK_R], self.m[SI_RGB_DIAL_DARK_G], self.m[SI_RGB_DIAL_DARK_B])
        } else if x == self.m[SI_DIALOG_GRAY] {
            (self.m[SI_RGB_DIAL_GRAY_R], self.m[SI_RGB_DIAL_GRAY_G], self.m[SI_RGB_DIAL_GRAY_B])
        } else if x == self.m[SI_DIALOG_LIGHT] {
            (self.m[SI_RGB_DIAL_LIGHT_R], self.m[SI_RGB_DIAL_LIGHT_G], self.m[SI_RGB_DIAL_LIGHT_B])
        } else if x == self.m[SI_DIALOG_BACK] {
            (self.m[SI_RGB_DIAL_BACK_R], self.m[SI_RGB_DIAL_BACK_G], self.m[SI_RGB_DIAL_BACK_B])
        } else if x == self.m[SI_DIALOG_BUT] {
            (self.m[SI_RGB_DIAL_BUTT_R], self.m[SI_RGB_DIAL_BUTT_G], self.m[SI_RGB_DIAL_BUTT_B])
        } else if x == self.m[SI_DIALOG_TITLE] {
            (self.m[SI_RGB_DIAL_TITL_R], self.m[SI_RGB_DIAL_TITL_G], self.m[SI_RGB_DIAL_TITL_B])
        } else if x == self.m[SI_DIALOG_GHOST] {
            (self.m[SI_RGB_DIAL_GHOST_R], self.m[SI_RGB_DIAL_GHOST_G], self.m[SI_RGB_DIAL_GHOST_B])
        } else {
            return;
        };
        self.o.gl.RGBcolor(&mut self.m, r, g, b);
    }

    /// The macro for the role value `x` (the same value in both modes).
    pub(crate) fn ctl(&mut self, x: i16, button: bool) {
        let index = self.ctl_index(x, button);
        self.ctl_with(index, x);
    }

    /// `(*SI_sboxfi)(x1, y1, x2, y2)`: the original calls through the global pointer, which
    /// holds the GL layer's `sboxfi` (see `window.rs`).
    pub(crate) fn ctl_sboxfi(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.o.gl.sboxfi(&mut self.m, x1, y1, x2, y2);
    }
}
