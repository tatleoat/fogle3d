// SPDX-License-Identifier: MIT
//! `ui_misc.c` of the program (0x083d17c0): the mouse's acceleration, the cursor shapes and
//! the "busy" cursor, and the reading of one line of a resource file.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.

use crate::file::Files;
use crate::globals::*;
use crate::{fault, Ui};
use si_x87::{Round, F80};

impl Ui {
    /// Read one line of a text stream into a new string
    /// [soft 0x083d17c0 si_stringReading]: up to 79 characters with `fgets`; if that line is empty (only
    /// its end), the next one is read instead; the line's end is cut off and the text
    /// copied with `strdup`. Returns the copy.
    ///
    /// The original looks for the line feed without a bound: a line of 79 characters or
    /// more, or the end of the file, leaves none in its 80-byte buffer and it searches on
    /// through its stack. Here the search stops at the buffer's end and the text is cut
    /// there.
    pub fn si_string_reading(&mut self, stream: u32) -> u32 {
        let sp = self.m.sp();
        let buffer = self.m.local(0x50);
        self.files.fgets(&mut self.m, buffer, 0x50, stream);
        if self.m.u8(buffer) == b'\n' {
            self.files.fgets(&mut self.m, buffer, 0x50, stream);
        }
        let mut at = buffer;
        while self.m.u8(at) != b'\n' && at < buffer + 0x4f {
            at += 1;
        }
        self.m.set_u8(at, 0);
        let copy = self.o.mem.strdup(&mut self.m, buffer);
        self.m.set_sp(sp);
        copy
    }

    /// Set the mouse's acceleration and threshold [soft 0x083d181c SI_mouseWarpSet]:
    /// `qcontrol(0x20)` with the two as 16-bit numbers, the first truncated, the second
    /// × 16 and truncated. 1, or 0 with a `perror("mousewarp")` when the call failed.
    pub fn si_mouse_warp_set(&mut self, acceleration: f32, threshold: f32) -> i32 {
        let a = F80::from_f32(acceleration).to_i32(Round::Zero) as i16;
        let b = (F80::from_f32(threshold) * fault!(16.0f64, 8.0f64)).to_i32(Round::Zero) as i16;
        if self.o.gl.qcontrol(&mut self.m, 0x20, Some(&[a, b]), 0, 0) < 0 {
            self.o.c.perror(&mut self.m, b"mousewarp");
            return 0;
        }
        1
    }

    /// Ask for the mouse's acceleration and threshold [soft 0x083d18a8 SI_mouseWarpGet]:
    /// `qcontrol(0x1f)` answers two 16-bit numbers; the first becomes a float as it is,
    /// the second / 16. 1, or 0 with a `perror("mousewarp")` and nothing stored.
    pub fn si_mouse_warp_get(&mut self, acceleration: u32, threshold: u32) -> i32 {
        let mut answer = [0i16; 2];
        if self.o.gl.qcontrol_out(&mut self.m, 0x1f, &mut answer) < 0 {
            self.o.c.perror(&mut self.m, b"mousewarp");
            return 0;
        }
        self.m.set_f32(acceleration, F80::from_i32(answer[0] as i32).to_f32());
        self.m.set_f32(threshold, (F80::from_i32(answer[1] as i32) * 0.0625f64).to_f32());
        1
    }

    /// Change the cursor's shape [soft 0x083d1910 SI_cursorChange] (not in the stand-alone
    /// renderer). 5 to 11 are the frames of the "busy" cursor and 13 the learn-mode
    /// cursor: set as they are. 0 is "back to normal": 13 in swift-key learn mode, else
    /// the large cursor 12 when the template's word at +0x78 is set, else IRIS GL's
    /// default 0. Anything else does nothing. With pictures shown (`SI_rgbActive`) the
    /// cursor is set in every window (`viewWindSetMouse`).
    pub fn si_cursor_change(&mut self, cursor: i32) {
        if self.m[SI_STANDALONE_RENDERER] != 0 {
            return;
        }
        let mut cursor = cursor;
        if cursor <= 0xb {
            if cursor < 5 {
                if cursor != 0 {
                    return;
                }
                cursor = if self.o.menus.si_learnMode(&mut self.m) != 0 {
                    0xd
                } else if self.m.i16(SI_TEMPLATE.addr() + 0x78) != 0 {
                    0xc
                } else {
                    0
                };
            }
        } else if cursor != 0xd {
            return;
        }
        self.o.gl.setcursor(&mut self.m, cursor as i16, 0, 0);
        if self.m[SI_RGB_ACTIVE] != 0 {
            self.view_wind_set_mouse(cursor);
        }
    }

    /// Set a cursor in every IRIS GL window of the program
    /// [soft 0x083d19c0 viewWindSetMouse]: the views' own picture windows, the dialogs
    /// (`si_dialogsSetCursor`), the main window; the current window is put back.
    pub fn view_wind_set_mouse(&mut self, cursor: i32) {
        let current = self.o.gl.winget(&mut self.m);
        if self.m[SI_TEMPLATE].n_views > 0 {
            let mut i = 0;
            loop {
                let own = self.m[self.m[SI_TEMPLATE].views.add(i)].d_050;
                if own != -1 {
                    self.o.gl.winset(&mut self.m, own);
                    self.o.gl.setcursor(&mut self.m, cursor as i16, 0, 0);
                }
                i += 1;
                if i >= self.m[SI_TEMPLATE].n_views as i32 {
                    break;
                }
            }
        }
        self.o.dialogs.si_dialogsSetCursor(&mut self.m, cursor);
        let sysid = self.m[SI_SYSID];
        self.o.gl.winset(&mut self.m, sysid);
        self.o.gl.setcursor(&mut self.m, cursor as i16, 0, 0);
        self.o.gl.winset(&mut self.m, current);
    }

    /// Show that the program is busy [soft 0x083d1a60 SI_showActivity]: called over and
    /// over by long operations; with the busy cursor switched on (and not in the
    /// stand-alone renderer), once a second (`times` / 100 against the static time of the
    /// next change) the cursor becomes the next frame of the busy cursor: 11 down to 6
    /// and round again.
    pub fn si_show_activity(&mut self) {
        if self.m[SI_BUSY_CURSOR] == 0 || self.m[SI_STANDALONE_RENDERER] != 0 {
            return;
        }
        let ticks = self.o.c.times(&mut self.m);
        let now = F80::from_i32(ticks) / 100.0f32;
        if now.gt(F80::from_f32(f32::from_bits(self.m[PREV_TIME_241]))) {
            let frame = self.m[CURSOR_ID_240];
            self.m[PREV_TIME_241] = (now + F80::ONE).to_f32().to_bits();
            self.si_cursor_change(frame);
            self.m[CURSOR_ID_240] = self.m[CURSOR_ID_240].wrapping_sub(1);
            if self.m[CURSOR_ID_240] == 5 {
                self.m[CURSOR_ID_240] = 0xb;
            }
        }
    }

    /// The busy cursor back to the normal one [soft 0x083d1adc SI_stopActivity].
    pub fn si_stop_activity(&mut self) {
        if self.m[SI_BUSY_CURSOR] != 0 && self.m[SI_STANDALONE_RENDERER] == 0 {
            self.si_cursor_change(0);
        }
    }

    /// Switch the busy cursor on or off [soft 0x083d1b04 SI_toggleActivity]; switched off,
    /// the cursor goes back to normal. (Only the low byte of the switch is flipped.)
    pub fn si_toggle_activity(&mut self) {
        let v = self.m[SI_BUSY_CURSOR] as u16 ^ 1;
        self.m[SI_BUSY_CURSOR] = v as i16;
        if v == 0 && self.m[SI_STANDALONE_RENDERER] == 0 {
            self.si_cursor_change(0);
        }
    }
}

// `Files` is the stream layer of `crate::file`; named here for the doc links.
#[allow(unused)]
fn _files(_: &Files) {}
