// SPDX-License-Identifier: MIT
//! `request.c`: the request dialogs: a message of up to three (or five) lines with one to
//! four buttons, or with a line of text to edit. Each opens a dialog resource, sets its
//! items, waits for a button and closes it.

use crate::env::{Bytes, Handle, World};
use crate::state::App;

/// The dialog resources: one button, one button with five lines, two, three, four buttons,
/// and three buttons with an editable text.
pub const REQUEST_1: i32 = 0x564;
pub const REQUEST_LONG: i32 = 0x569;
pub const REQUEST_2: i32 = 0x56e;
pub const REQUEST_3: i32 = 0x578;
pub const REQUEST_4: i32 = 0x57d;
pub const REQUEST_TEXT: i32 = 0x582;

fn eq_nocase(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

impl<W: World> App<W> {
    /// Which button the Escape key stands for: the first one labelled "Cancel" or "Exit"
    /// (in any letter case), else none. Stored in the dialog at +0x12.
    fn request_default(&mut self, dialog: Handle, buttons: &[&[u8]]) {
        self.w.mem_dialog_set_default(dialog, -1);
        for (i, label) in buttons.iter().enumerate() {
            if eq_nocase(label, b"Cancel") || eq_nocase(label, b"Exit") {
                self.w.mem_dialog_set_default(dialog, i as i16);
                break;
            }
        }
    }

    fn request_wait(&mut self, dialog: Handle, buttons: u16) -> i16 {
        loop {
            let item = self.w.SI_dialogGetEvent(dialog);
            if (item.wrapping_sub(1) as u16) < buttons {
                return item.wrapping_sub(1);
            }
        }
    }

    /// [soft 0x08141c00 SI_dialogRequest]: three lines and one button; waits for the
    /// button (item 1).
    pub fn dialog_request(&mut self, line1: &[u8], line2: &[u8], line3: &[u8], button: &[u8]) {
        let dialog = self.w.SI_dialogOpen(REQUEST_1);
        if dialog == 0 {
            return;
        }
        self.w.SI_setItemTitle(dialog, 1, button);
        self.w.SI_setItemTitle(dialog, 2, line1);
        self.w.SI_setItemTitle(dialog, 3, line2);
        self.w.SI_setItemTitle(dialog, 4, line3);
        self.w.SI_dialogDraw(dialog);
        while self.w.SI_dialogGetEvent(dialog) != 1 {}
        self.w.SI_dialogClose(dialog);
        self.w.SI_dialogFree(dialog);
    }

    /// [soft 0x08141c9c SI_dialogRequestLong]: five lines and one button.
    pub fn dialog_request_long(&mut self, lines: [&[u8]; 5], button: &[u8]) {
        let dialog = self.w.SI_dialogOpen(REQUEST_LONG);
        if dialog == 0 {
            return;
        }
        self.w.SI_setItemTitle(dialog, 1, button);
        for (i, line) in lines.iter().enumerate() {
            self.w.SI_setItemTitle(dialog, 2 + i as i32, line);
        }
        self.w.SI_dialogDraw(dialog);
        while self.w.SI_dialogGetEvent(dialog) != 1 {}
        self.w.SI_dialogClose(dialog);
        self.w.SI_dialogFree(dialog);
    }

    /// [soft 0x08141d5c SI_dialogRequest2]: three lines and two buttons; 0 or 1 for the
    /// button chosen, −1 when the dialog cannot be opened.
    pub fn dialog_request2(&mut self, line1: &[u8], line2: &[u8], line3: &[u8], button1: &[u8], button2: &[u8]) -> i32 {
        let dialog = self.w.SI_dialogOpen(REQUEST_2);
        if dialog == 0 {
            return -1;
        }
        self.request_default(dialog, &[button1, button2]);
        self.w.SI_setItemTitle(dialog, 1, button1);
        self.w.SI_setItemTitle(dialog, 2, button2);
        self.w.SI_setItemTitle(dialog, 3, line1);
        self.w.SI_setItemTitle(dialog, 4, line2);
        self.w.SI_setItemTitle(dialog, 5, line3);
        self.w.SI_dialogDraw(dialog);
        let chosen = self.request_wait(dialog, 2);
        self.w.SI_dialogClose(dialog);
        self.w.SI_dialogFree(dialog);
        chosen as i32
    }

    /// [soft 0x08141e84 SI_dialogRequest3]: three lines and three buttons; 0 to 2.
    pub fn dialog_request3(&mut self, lines: [&[u8]; 3], buttons: [&[u8]; 3]) -> i32 {
        let dialog = self.w.SI_dialogOpen(REQUEST_3);
        if dialog == 0 {
            return -1;
        }
        self.request_default(dialog, &buttons);
        for (i, b) in buttons.iter().enumerate() {
            self.w.SI_setItemTitle(dialog, 1 + i as i32, b);
        }
        for (i, l) in lines.iter().enumerate() {
            self.w.SI_setItemTitle(dialog, 4 + i as i32, l);
        }
        self.w.SI_dialogDraw(dialog);
        let chosen = self.request_wait(dialog, 3);
        self.w.SI_dialogClose(dialog);
        self.w.SI_dialogFree(dialog);
        chosen as i32
    }

    /// [soft 0x08142000 SI_dialogRequest34]: three lines and four buttons; 0 to 3.
    pub fn dialog_request34(&mut self, lines: [&[u8]; 3], buttons: [&[u8]; 4]) -> i32 {
        let dialog = self.w.SI_dialogOpen(REQUEST_4);
        if dialog == 0 {
            return -1;
        }
        self.request_default(dialog, &buttons);
        for (i, b) in buttons.iter().enumerate() {
            self.w.SI_setItemTitle(dialog, 1 + i as i32, b);
        }
        for (i, l) in lines.iter().enumerate() {
            self.w.SI_setItemTitle(dialog, 5 + i as i32, l);
        }
        self.w.SI_dialogDraw(dialog);
        let chosen = self.request_wait(dialog, 4);
        self.w.SI_dialogClose(dialog);
        self.w.SI_dialogFree(dialog);
        chosen as i32
    }

    /// [soft 0x081421d0 dialogRequest4]: two lines, a text to edit (item 6) and three
    /// buttons: the button (0 to 2) and the text as it was left; (−1, nothing) when the
    /// dialog cannot be opened. The dialog is closed but, unlike in the other requests,
    /// not freed.
    pub fn dialog_request4(&mut self, line1: &[u8], line2: &[u8], text: &[u8]) -> (i32, Option<Bytes>) {
        let dialog = self.w.SI_dialogOpen(REQUEST_TEXT);
        if dialog == 0 {
            return (-1, None);
        }
        self.w.SI_setItemTitle(dialog, 4, line1);
        self.w.SI_setItemTitle(dialog, 5, line2);
        self.w.SI_setTexed(dialog, 6, text);
        self.w.SI_dialogDraw(dialog);
        let chosen = loop {
            let item = self.w.SI_dialogGetEvent(dialog) as i32 - 1;
            if (item as u32) <= 2 {
                break item;
            }
        };
        self.w.SI_dialogClose(dialog);
        // the original hands the fetched text to strdup even when it is null
        let answer = self.w.get_texed(dialog, 6);
        (chosen, answer)
    }

    /// [soft 0x081422a0 request2states]: a two-button request; true for the first button.
    pub fn request2states(&mut self, line1: &[u8], line2: &[u8], line3: &[u8], button1: &[u8], button2: &[u8]) -> bool {
        self.dialog_request2(line1, line2, line3, button1, button2) as i16 == 0
    }

    /// [soft 0x081422d8 SI_systemWarning]: a request with "Ok" and "Cancel"; true for
    /// "Ok".
    pub fn system_warning(&mut self, line1: &[u8], line2: &[u8], line3: &[u8]) -> bool {
        self.dialog_request2(line1, line2, line3, b"Ok", b"Cancel") as i16 == 0
    }

    /// [soft 0x0814230c SI_systemNotice]: a request with the one button "Ok".
    pub fn system_notice(&mut self, line1: &[u8], line2: &[u8], line3: &[u8]) {
        self.dialog_request(line1, line2, line3, b"Ok");
    }
}
