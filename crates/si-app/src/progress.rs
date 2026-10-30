// SPDX-License-Identifier: MIT
//! `in_progress.c`: the "in progress" service: a dialog with a text and a slider in the
//! interactive program, lines on the standard error stream in the batch renderer.
//!
//! Every routine but the two queries does nothing while `SI_cusDsoGetRefreshFlag()` is
//! false (a custom effect has switched refreshing off).

use crate::env::{Bytes, World};
use crate::state::App;

/// The resource number of the in-progress dialog.
pub const IN_PROGRESS_DIALOG: i32 = 0x2713;

/// C's `%f` and `%2.2f` of a `float` widened to `double`.
fn fixed(v: f32, digits: usize) -> Bytes {
    let d = v as f64;
    if d.is_nan() {
        return if d.is_sign_negative() { b"-nan".to_vec() } else { b"nan".to_vec() };
    }
    if d.is_infinite() {
        return if d < 0.0 { b"-inf".to_vec() } else { b"inf".to_vec() };
    }
    format!("{d:.digits$}").into_bytes()
}

/// `%70s`: right-aligned in 70 columns.
fn pad70(text: &[u8]) -> Bytes {
    let mut s = vec![b' '; 70usize.saturating_sub(text.len())];
    s.extend_from_slice(text);
    s
}

impl<W: World> App<W> {
    /// [soft 0x08141980 SI_dialogInProgressOpen]: opens the dialog (item 2 the title, item
    /// 3 a blank) and draws it; in the batch renderer prints the title between new lines.
    pub fn in_progress_open(&mut self, title: &[u8]) {
        if !self.w.SI_cusDsoGetRefreshFlag() {
            return;
        }
        if self.g.standalone_renderer == 0 {
            let dialog = self.w.SI_dialogOpen(IN_PROGRESS_DIALOG);
            if dialog != 0 {
                self.g.in_progress_dial = dialog;
                self.w.SI_setItemTitle(dialog, 2, title);
                self.w.SI_setItemTitle(dialog, 3, b" ");
                self.w.SI_dialDraw(dialog, 0);
            }
        } else {
            self.w.err(&[b"\n".as_slice(), title, b" \n"].concat());
        }
    }

    /// [soft 0x08141a08 SI_dialogInProgressSetTexts]: both texts of the open dialog. (No
    /// refresh-flag test here.)
    pub fn in_progress_set_texts(&mut self, title: &[u8], text: &[u8]) {
        if self.g.standalone_renderer == 0 && self.g.in_progress_dial != 0 {
            let dialog = self.g.in_progress_dial;
            self.w.SI_setItemTitle(dialog, 2, title);
            self.w.SI_setItemTitle(dialog, 3, text);
        }
    }

    /// [soft 0x08141a4c SI_dialogInProgressIsOpen].
    pub fn in_progress_is_open(&self) -> bool {
        self.g.in_progress_dial != 0
    }

    /// [soft 0x08141a60 SI_dialogInProgressRedraw].
    pub fn in_progress_redraw(&mut self) {
        if self.g.standalone_renderer == 0 && self.g.in_progress_dial != 0 {
            self.w.SI_dialDraw(self.g.in_progress_dial, 0);
        }
    }

    /// [soft 0x08141a88 SI_dialogInProgressSet]: progress in percent. Dialog: the value,
    /// cut to 100 when above, goes to the slider as text (`%f`) and `text` to item 3.
    /// Batch renderer: a line of 70 blanks and a carriage return, then `text: 12.34% ` and
    /// a carriage return; at exactly 100 a new line, with the bell when an emergency save
    /// is running.
    ///
    /// The original formats the number into 16 bytes of stack; a value that needs more
    /// (below −99999) overruns them. The port has no such limit.
    pub fn in_progress_set(&mut self, text: &[u8], percent: f32) {
        if !self.w.SI_cusDsoGetRefreshFlag() {
            return;
        }
        if self.g.standalone_renderer == 0 {
            if self.g.in_progress_dial != 0 {
                let percent = if percent > 100.0 { 100.0 } else { percent };
                let number = fixed(percent, 6);
                let dialog = self.g.in_progress_dial;
                self.w.SI_setItemTitle(dialog, 3, text);
                self.w.SI_sliderSet(dialog, 1, &number, 1);
            }
        } else {
            let mut line = pad70(b" ");
            line.push(b'\r');
            self.w.err(&line);
            self.w.err(&[text, b": ", &fixed(percent, 2), b"% \r"].concat());
            if percent as f64 == 100.0 {
                if self.emergency_saving() != 0 {
                    self.w.setbell(1);
                    self.w.ringbell();
                }
                self.w.err(b"\n");
            }
        }
    }

    /// [soft 0x08141b9c SI_dialogInProgressClose]: closes the dialog; in the batch
    /// renderer a line of 70 blanks.
    pub fn in_progress_close(&mut self) {
        if !self.w.SI_cusDsoGetRefreshFlag() {
            return;
        }
        if self.g.standalone_renderer == 0 {
            if self.g.in_progress_dial != 0 {
                self.w.SI_dialogClose(self.g.in_progress_dial);
                self.g.in_progress_dial = 0;
            }
        } else {
            let mut line = pad70(b" ");
            line.push(b'\n');
            self.w.err(&line);
        }
    }
}
