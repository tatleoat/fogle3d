// SPDX-License-Identifier: MIT
//! `slider.c` of the program (0x083c5fb0 to 0x083c6fbb): the dialogs' slider buttons.
//!
//! Ported: the two drawing routines and `SI_sliderSet`. Left uncited: `SI_handleHsliderBut` (0x083c67f0) and
//! `SI_handleVsliderBut` (0x083c6bc8), which follow the mouse in a loop until the buttons are
//! released.  `SI_sliderSet` (0x083c6694, with the part the symbol table does not name,
//! 0x083c6750) is ported too.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.

use crate::globals::*;
use crate::mem::{Addr, P};
use si_x87::{Round, F80};
use crate::records::Scroll;
use crate::Ui;

impl Ui {
    /// Draw a slider button in the dialogs' draw mode (overlay planes in colour-map mode while a
    /// dialog is open) [soft 0x083c5fb0 drawSliderBut].
    pub fn draw_slider_but(&mut self, p: P<Scroll>, pos: i32, flag: i16) {
        let overlay = self.m[SI_RGBMODE] == 0 && self.m[SI_DIALOGS_OPEN] != 0;
        let mut saved = 0;
        if overlay {
            saved = self.o.gl.getdrawmode(&mut self.m);
            self.o.gl.drawmode(&mut self.m, 0x20);
        }
        self.draw_slider_but_gl(p, pos, flag);
        if self.m[SI_RGBMODE] == 0 && self.m[SI_DIALOGS_OPEN] != 0 {
            self.o.gl.drawmode(&mut self.m, saved);
        }
    }

    /// A slider button: the groove (back colour when idle, light when the control's state is
    /// non-zero), then the knob at `pos` pixels from the left (kind 9) or the bottom (kind 10),
    /// kept inside the groove; `flag` zero draws the knob in the gray colour of its neighbours'
    /// frame, otherwise in dark [soft 0x083c6034 drawSliderButGL].
    pub fn draw_slider_but_gl(&mut self, p: P<Scroll>, pos: i32, flag: i16) {
        let s = self.m[p];
        let (x1, y1, x2, y2) = (s.x1 as i32, s.y1 as i32, s.x2 as i32, s.y2 as i32);
        if s.state == 0 {
            let b = self.m[SI_DIALOG_BACK];
            self.ctl(b, false);
        } else {
            let l = self.m[SI_DIALOG_LIGHT];
            self.ctl(l, false);
        }
        self.ctl_sboxfi(x1 + 1, y1 + 2, x2 - 1, y2 - 2);
        // the groove's frame, in the knob's neighbouring colour
        if flag == 0 {
            let g = self.m[SI_DIALOG_GRAY];
            self.ctl(g, true);
        } else {
            let d = self.m[SI_DIALOG_DARK];
            self.ctl(d, true);
        }
        let mut pos = pos;
        if s.kind == 9 {
            if pos < 1 {
                pos = 1;
            } else if x2 - x1 - 1 <= pos {
                pos = x2 - x1 - 2;
            }
            let g = self.m[SI_DIALOG_GRAY];
            self.ctl(g, true);
            self.ctl_sboxfi(x1 + 2, y1 + 4, x1 + pos, y2 - 4);
        } else if s.kind == 10 {
            if pos < 1 {
                pos = 1;
            } else if y2 - y1 - 1 <= pos {
                pos = y2 - y1 - 2;
            }
            let g = self.m[SI_DIALOG_GRAY];
            self.ctl(g, true);
            self.ctl_sboxfi(x1 + 4, y1 + 1, x2 - 4, y1 + pos);
        }
    }

    /// Put a slider (control `item` of the dialog, kind 9 or 10) at the value the text names
    /// and redraw it [soft 0x083c6694 SI_sliderSet]. The control's data holds the slider's
    /// minimum (+4) and maximum (+8) as floats; the knob goes to
    /// `(value - min) / ((max - min) / width)` pixels, to `width - 2` when that step is
    /// below 0.0001. Redrawn only when the control's flag word (+0x24) is non-zero.
    /// The x87 order: `max - min` is stored as a float, the text's value is stored as a float
    /// and `value - min` stays in the register until the final division and truncation.
    pub fn si_slider_set(&mut self, dialog: Addr, item: i16, text: Addr, flag: i16) {
        let ctl = self.m.u32(dialog.wrapping_add(0x14)).wrapping_add((item as i32).wrapping_mul(0x28) as u32).wrapping_sub(0x28);
        if self.m.u32(ctl).wrapping_sub(9) > 1 {
            return;
        }
        let data = self.m.u32(ctl.wrapping_add(0x10));
        let min = f32::from_bits(self.m.u32(data.wrapping_add(4)));
        let max = f32::from_bits(self.m.u32(data.wrapping_add(8)));
        let span = (F80::from_f32(max) - min).to_f32();
        let t = self.m.cstr(text);
        let value = self.o.c.__strtod_internal(&mut self.m, &t, 0, 0);
        let value = F80::from_f64(value).to_f32();
        let off = F80::from_f32(value) - min;
        let kind = self.m.u32(ctl);
        let width = if kind == 9 {
            self.m.i16(ctl.wrapping_add(8)) as i32 - self.m.i16(ctl.wrapping_add(6)) as i32
        } else {
            self.m.i16(ctl.wrapping_add(0xc)) as i32 - self.m.i16(ctl.wrapping_add(0xa)) as i32
        };
        let step = (F80::from_f32(span) / F80::from_i32(width)).to_f32();
        let pos = if (step.abs() as f64) < 0.0001 {
            self.m.i16(ctl.wrapping_add(8)) as i32 - self.m.i16(ctl.wrapping_add(6)) as i32 - 2
        } else {
            (off / step).to_i32(Round::Zero)
        };
        if self.m.i16(ctl.wrapping_add(0x24)) != 0 {
            self.draw_slider_but(P::at(ctl), pos, flag);
        }
    }
}
