// SPDX-License-Identifier: MIT
//! `log.c`: the session log, switched on by the environment variable `SI_LOG`.
//!
//! `SI_LOG` holds a number: bit 8 copies the log to the standard output, 4 logs mouse
//! events, 2 logs keys (bit 1 is always set in the stored number and means nothing else). The file is `SI_LOG.<process id>` in the current directory, opened
//! for appending.

use crate::env::{Bytes, World};
use crate::state::App;

/// The kind of a log line (`SI_LogType`): 0 system, 1 user, anything else blank.
pub const LOG_SYS: i32 = 0;
pub const LOG_USER: i32 = 1;

/// C's `atoi`: blanks, a sign, decimal digits; wraps like `strtol` cut to 32 bits would
/// only for numbers no log setting has.
fn atoi(s: &[u8]) -> i32 {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || (9..=13).contains(&s[i])) {
        i += 1;
    }
    let neg = i < s.len() && s[i] == b'-';
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        i += 1;
    }
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = (v * 10 + (s[i] - b'0') as i64).min(i32::MAX as i64 + 1);
        i += 1;
    }
    if neg {
        (-v).max(i32::MIN as i64) as i32
    } else {
        v.min(i32::MAX as i64) as i32
    }
}

impl<W: World> App<W> {
    fn log_write(&mut self, text: &[u8]) {
        if let Some(name) = self.g.log.file.clone() {
            self.w.write_file(&name, text, true);
        }
    }

    /// [soft 0x083fbe50 logTime]: the first 20 characters of `ctime(now)` and a blank, to
    /// the log and, when asked, to the screen (`ctime` is called again for that).
    fn log_time(&mut self) {
        let now = self.w.time();
        let stamp = |t: Bytes| -> Bytes {
            // "%20.20s ": cut to 20 characters, padded on the left to 20
            let cut = &t[..t.len().min(20)];
            let mut s = vec![b' '; 20 - cut.len()];
            s.extend_from_slice(cut);
            s.push(b' ');
            s
        };
        let t = self.w.ctime(now);
        self.log_write(&stamp(t));
        if self.g.log.to_screen != 0 {
            let t = self.w.ctime(now);
            self.w.out(&stamp(t));
        }
    }

    /// [soft 0x083fba70 SI_log]: one line of the log: time, `SYS  `, `USER ` or five
    /// blanks, the text, a new line; then a flush. Nothing unless `SI_LOG` was set and the
    /// file is open. `text` is the caller's format with its arguments already applied.
    pub fn si_log(&mut self, kind: i32, text: &[u8]) {
        if self.g.log.need_to_log.is_none() || self.g.log.file.is_none() {
            return;
        }
        self.log_time();
        let tag: &[u8] = match kind {
            LOG_SYS => b"SYS  ",
            LOG_USER => b"USER ",
            _ => b"     ",
        };
        self.log_write(tag);
        if self.g.log.to_screen != 0 {
            self.w.out(tag);
        }
        self.log_write(text);
        if self.g.log.to_screen != 0 {
            self.w.out(text);
        }
        self.log_write(b"\n");
        if self.g.log.to_screen != 0 {
            self.w.out(b"\n");
        }
    }

    /// [soft 0x083fbbc8 SI_mouseLog]: `M[device,value,x,y]` when mouse logging is on.
    pub fn mouse_log(&mut self, device: i32, value: i16, x: i16, y: i16) {
        if self.g.log.mouse != 0 && self.g.log.file.is_some() {
            self.log_time();
            let line = format!("M[{device},{value},{x},{y}]\n").into_bytes();
            self.log_write(&line);
            if self.g.log.to_screen != 0 {
                self.w.out(&line);
            }
        }
    }

    /// [soft 0x083fbc58 SI_keyLog]: `K[device,value]` when key logging is on.
    pub fn key_log(&mut self, device: i32, value: i16) {
        if self.g.log.keyboard != 0 && self.g.log.file.is_some() {
            self.log_time();
            let line = format!("K[{device},{value}]\n").into_bytes();
            self.log_write(&line);
            if self.g.log.to_screen != 0 {
                self.w.out(&line);
            }
        }
    }

    /// [soft 0x083fbcc8 SI_logOpen]: reads `SI_LOG`; unset, nothing is logged. Otherwise
    /// opens `SI_LOG.<pid>` for appending and takes the three switches from the number.
    /// The switches are only ever set, never cleared.
    pub fn log_open(&mut self) {
        self.g.log.need_to_log = self.w.getenv(b"SI_LOG");
        let Some(setting) = self.g.log.need_to_log.clone() else {
            self.g.log.check_need_to_log = 0;
            return;
        };
        let pid = self.w.getpid();
        if pid == -1 {
            self.w.out(b"CANNOT LOG: unknown process id\n");
            return;
        }
        let name = format!("SI_LOG.{pid}").into_bytes();
        if !self.w.write_file(&name, b"", true) {
            self.g.log.file = None;
            self.w.out(b"Could not open Log file \n");
            self.g.log.check_need_to_log = 0;
            return;
        }
        self.g.log.file = Some(name);
        // the number with its lowest bit set
        let v = atoi(&setting) | 1;
        self.g.log.check_need_to_log = v;
        if v & 8 != 0 {
            self.g.log.to_screen = 1;
        }
        if v & 4 != 0 {
            self.g.log.mouse = 1;
        }
        if v & 2 != 0 {
            self.g.log.keyboard = 1;
        }
    }

    /// [soft 0x083fbe34 SI_logClose]: closes the file if it is open (the pointer is kept,
    /// so a later log call would write to a closed stream; nothing does).
    pub fn log_close(&mut self) {
        // the in-memory file needs no closing
    }
}

#[cfg(test)]
mod tests {
    use super::atoi;

    #[test]
    fn atoi_reads_like_c() {
        assert_eq!(atoi(b"14"), 14);
        assert_eq!(atoi(b"  -3x"), -3);
        assert_eq!(atoi(b"x"), 0);
        assert_eq!(atoi(b""), 0);
        assert_eq!(atoi(b"99999999999"), i32::MAX);
    }
}
