// SPDX-License-Identifier: MIT
//! The event queue and the input devices (`event.c`, 0x0860cbd0, 23 functions).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! The model, all of it read from the code:
//!
//! - **Devices** are 16-bit numbers. Buttons are 1..=255 and 0xf00..=0x2fff; of those,
//!   1..=83 and 143..=170 are keyboard keys and 101, 102, 103 the right, middle and left
//!   mouse buttons. Valuators that work are MOUSEX (0x10a) and MOUSEY (0x10b). Pseudo
//!   devices the layer generates: KEYBD (0x201), REDRAW (0x210), INPUTCHANGE (0x216),
//!   WINFREEZE (0x21b), WINTHAW (0x21c), WINQUIT (0x21e). (The names are IRIS GL's, from
//!   its `device.h`; the numbers are the binary's.)
//! - **The queue** is unbounded, first in first out, of (device, value) pairs. Nothing is
//!   read from X until the program asks (`qtest`, `qread`, `qreset`): then every X event
//!   already received is translated ([`IrisGl::evt_process_events`]).
//! - **Queueing rules.** A key press or release is queued for the key's device if that
//!   device is queued (value 1 or 0), and a press also as KEYBD with the character Xlib
//!   looks up, if KEYBD is queued. A mouse button is queued if its device is, followed by
//!   the current values of the valuators tied to it. Pointer motion is not taken from X
//!   events: around every X event the pointer is polled and a MOUSEX or MOUSEY event queued
//!   when the position differs from the last one reported. Focus in and out give
//!   INPUTCHANGE with the window identifier or 0; a `ConfigureNotify` gives REDRAW
//!   (an `Expose` practically never does, see `evt_process_events`); a `WM_STATE` change
//!   gives WINFREEZE or WINTHAW; the window manager's close request gives WINQUIT if that
//!   device is queued and otherwise **ends the program** with `exit(0)`.
//! - **Time.** Each entry is stamped with hundredths of a second from `time` and `clock`;
//!   nothing reads the stamp.

use crate::context::{st, IrisGl};
use crate::data;
use crate::state::*;

/// MOUSEX and MOUSEY.
pub const MOUSEX: u16 = 0x10a;
pub const MOUSEY: u16 = 0x10b;
/// The three mouse buttons.
pub const RIGHTMOUSE: u16 = 0x65;
pub const MIDDLEMOUSE: u16 = 0x66;
pub const LEFTMOUSE: u16 = 0x67;
pub const KEYBD: u16 = 0x201;
pub const RAWKEYBD: u16 = 0x202;
pub const REDRAW: u16 = 0x210;
pub const INPUTCHANGE: u16 = 0x216;
pub const WINFREEZE: u16 = 0x21b;
pub const WINTHAW: u16 = 0x21c;
pub const WINQUIT: u16 = 0x21e;

/// The original's test for "a button": 1..=255 or 0xf00..=0x2fff.
fn is_button(dev: u16) -> bool {
    dev.wrapping_sub(1) < 0xff || dev.wrapping_sub(0xf00) < 0x2100
}

/// The original's test for "a keyboard key": 1..=83 or 143..=170.
fn is_key(dev: u16) -> bool {
    dev.wrapping_sub(1) < 0x53 || dev.wrapping_sub(0x8f) < 0x1c
}

/// The device a keysym belongs to, 0 for none (the compare tree of `evt_initKeymap`).
pub fn keysym_device(keysym: u32) -> u16 {
    match data::KEYSYM_DEVICE.binary_search_by(|e| e.0.cmp(&keysym)) {
        Ok(i) => data::KEYSYM_DEVICE[i].1,
        Err(_) => 0,
    }
}

impl IrisGl {
    /// Build the keycode-to-device map and its reverse [soft 0x0860cbd0 evt_initKeymap].
    ///
    /// For each keycode 0..=255 the unshifted keysym (`XKeycodeToKeysym(dpy, keycode, 0)`)
    /// is looked up in a fixed table of 124 keysyms; anything else is device 0. The
    /// highest device number seen plus one is the length of the reverse table, in which
    /// each device ends up with the highest keycode that maps to it (and device 0 with the
    /// highest keycode that maps to nothing).
    ///
    /// Between the two steps the original has a pass meant to move the second keycode of
    /// a doubled device to a key-pad device (thirteen pairs, `data::keypad_fixups`). Its
    /// guard tests the count of device 0xff, which no keysym maps to, so it never runs.
    /// It is reproduced with its guard.
    fn evt_init_keymap(&mut self) {
        let (ctx, sys) = st!(self);
        let evt = &mut ctx.evt;
        let mut count = [0u8; 256];
        let mut max: i32 = 0;
        for keycode in 0..256usize {
            let keysym = sys.x.XKeycodeToKeysym(ctx.display, keycode as u32, 0);
            let dev = keysym_device(keysym);
            evt.keymap[keycode] = dev;
            count[dev as usize] = count[dev as usize].wrapping_add(1);
            if dev as i32 > max {
                max = dev as i32;
            }
        }
        if count[0xff] > 1 {
            for &(dev, replacement) in data::keypad_fixups().iter() {
                if count[dev as usize] == 2 {
                    let mut k = 0xff;
                    while k > 0 && evt.keymap[k] != dev {
                        k -= 1;
                    }
                    if k > 0 {
                        evt.keymap[k] = replacement;
                        if replacement as i32 > max {
                            max = replacement as i32;
                        }
                    }
                }
            }
        }
        evt.keycode_of = vec![0; (max + 1) as usize];
        for keycode in 0..256usize {
            evt.keycode_of[evt.keymap[keycode] as usize] = keycode as i32;
        }
    }

    /// Queue the current values of the two valuators tied to a button
    /// [soft 0x0860d934 evt_processTie].
    fn evt_process_tie(&mut self, dev: u16) {
        self.ctx_get();
        for i in 0..2 {
            let tied = self.ctx.as_deref().unwrap().evt.devices[dev as usize].tie[i];
            if tied != 0 {
                let v = self.evt_get_valuator(tied);
                self.evt_add_event(tied, v as i16);
            }
        }
    }

    /// Poll the pointer and queue MOUSEX and MOUSEY events for a changed position, each
    /// only if its device is queued [soft 0x0860d9a8 evt_doMouseEvents].
    fn evt_do_mouse_events(&mut self) {
        self.ctx_get();
        if self.ctx.as_deref().unwrap().evt.devices[MOUSEX as usize].queued != 0 {
            let v = self.evt_get_valuator(MOUSEX);
            if v != self.ctx.as_deref().unwrap().evt.last_x {
                self.evt_add_event(MOUSEX, v as i16);
                self.ctx.as_deref_mut().unwrap().evt.last_x = v;
            }
        }
        if self.ctx.as_deref().unwrap().evt.devices[MOUSEY as usize].queued != 0 {
            let v = self.evt_get_valuator(MOUSEY);
            if v != self.ctx.as_deref().unwrap().evt.last_y {
                self.evt_add_event(MOUSEY, v as i16);
                self.ctx.as_deref_mut().unwrap().evt.last_y = v;
            }
        }
    }

    /// Translate every X event already received into queue entries
    /// [soft 0x0860da38 evt_processEvents].
    ///
    /// The pointer is polled first and again after each event. Events are taken while
    /// `XEventsQueued(dpy, QueuedAfterFlush)` is non-zero. By X event type:
    ///
    /// | Type | What is queued |
    /// |---|---|
    /// | 2, 3 key press, release | the keycode's device with 1 or 0, if queued; for a press, KEYBD with the one character `XLookupString` gives, if KEYBD is queued |
    /// | 4, 5 button press, release | button 1 → 0x67, 2 → 0x66, 3 → 0x65 with 1 or 0, if queued, then the tied valuators |
    /// | 9 focus in | INPUTCHANGE with the window's identifier |
    /// | 10 focus out | INPUTCHANGE with 0 |
    /// | 12 expose, 22 configure | REDRAW with the identifier of the window whose X window is the long at offset 0x14 of the event |
    /// | 28 property | for `WM_STATE` set anew: WINFREEZE if the property's first long is 3 (iconic), WINTHAW if it is 1 (normal) |
    /// | 33 client message | for `WM_DELETE_WINDOW`: WINQUIT if that device is queued, else `exit(0)` |
    ///
    /// Offset 0x14 is the window of a `ConfigureNotify` but the **x coordinate** of an
    /// `Expose`, so an exposure gives a REDRAW only if its x happens to equal a window's
    /// X identifier; an `Expose` with a non-zero count is dropped before that. In effect
    /// REDRAW comes from configuration changes (and from `WND_backingStorePut`).
    /// INPUTCHANGE, REDRAW, WINFREEZE and WINTHAW are queued whether or not the device is
    /// queued: the device only decides whether X sends the events.
    pub(crate) fn evt_process_events(&mut self) {
        self.ctx_get();
        self.evt_do_mouse_events();
        loop {
            let (ctx, sys) = st!(self);
            if sys.x.XEventsQueued(ctx.display, 2) == 0 {
                return;
            }
            let ev = sys.x.XNextEvent(ctx.display);
            let kind = ev.kind();
            if kind == 0xc && ev.word_24() != 0 {
                continue;
            }
            match kind {
                2 | 3 => {
                    // the original indexes the 256-entry map with the keycode unchecked
                    let dev = ctx.evt.keymap[(ev.word_34() & 0xff) as usize];
                    if ctx.evt.devices[dev as usize].queued != 0 {
                        self.evt_add_event(dev, (kind == 2) as i16);
                    }
                    let (ctx, sys) = st!(self);
                    if ctx.evt.devices[KEYBD as usize].queued != 0 && kind == 2 {
                        let (n, ch) = sys.x.XLookupString(&ev);
                        if n == 1 {
                            self.evt_add_event(KEYBD, ch as i8 as i16);
                        }
                    }
                }
                4 | 5 => {
                    let button = ev.word_34();
                    let dev = if button == 1 && ctx.evt.devices[LEFTMOUSE as usize].queued != 0 {
                        Some(LEFTMOUSE)
                    } else if button == 2 && ctx.evt.devices[MIDDLEMOUSE as usize].queued != 0 {
                        Some(MIDDLEMOUSE)
                    } else if button == 3 && ctx.evt.devices[RIGHTMOUSE as usize].queued != 0 {
                        Some(RIGHTMOUSE)
                    } else {
                        None
                    };
                    if let Some(dev) = dev {
                        self.evt_add_event(dev, (kind == 4) as i16);
                        self.evt_process_tie(dev);
                    }
                }
                9 => {
                    if let Some(w) = self.wnd_get_from_xwindow(ev.word_10()) {
                        let id = self.ctx.as_deref().unwrap().wnd.windows[w].id;
                        self.evt_add_event(INPUTCHANGE, id as i16);
                    }
                }
                10 => self.evt_add_event(INPUTCHANGE, 0),
                0xc | 0x16 => {
                    if let Some(w) = self.wnd_get_from_xwindow(ev.word_14()) {
                        let id = self.ctx.as_deref().unwrap().wnd.windows[w].id;
                        self.evt_add_event(REDRAW, id as i16);
                    }
                }
                0x1c => {
                    if ev.word_14() == ctx.atoms.wm_state && ev.word_1c() == 0 {
                        if let Some(w) = self.wnd_get_from_xwindow(ev.word_10()) {
                            let (ctx, sys) = st!(self);
                            let id = ctx.wnd.windows[w].id;
                            let (_, data) = sys.x.XGetWindowProperty(ctx.display, ev.word_10(), ctx.atoms.wm_state, 0, 8, 0, 0);
                            match data {
                                Some(3) => self.evt_add_event(WINFREEZE, id as i16),
                                Some(1) => self.evt_add_event(WINTHAW, id as i16),
                                _ => {}
                            }
                        }
                    }
                }
                0x21 => {
                    if ev.word_1c() == ctx.atoms.wm_delete_window {
                        if let Some(w) = self.wnd_get_from_xwindow(ev.word_10()) {
                            let (ctx, sys) = st!(self);
                            if ctx.evt.devices[WINQUIT as usize].queued == 0 {
                                sys.c.exit(0);
                            }
                            let id = ctx.wnd.windows[w].id;
                            self.evt_add_event(WINQUIT, id as i16);
                        }
                    }
                }
                _ => {}
            }
            self.evt_do_mouse_events();
        }
    }

    /// Allocate the event state [soft 0x0860dd1c EVT_init].
    ///
    /// All devices unqueued and untied, except that REDRAW and INPUTCHANGE start queued.
    /// The 257 entries from device 0x100 get the long 0x8000 and the short 1 in their
    /// second and third fields (the test that would give four of them 10000 instead can
    /// never be true). No pointer grab, last pointer position (−1, −1), then the key map.
    pub(crate) fn evt_init(&mut self) {
        let (ctx, _) = st!(self);
        let mut evt = EvtContext { devices: vec![Device::default(); DEVICE_COUNT], ..Default::default() };
        for d in &mut evt.devices[0x100..=0x200] {
            d.tie = [0x8000, 0];
            d.word8 = 1;
        }
        evt.devices[REDRAW as usize].queued = 1;
        evt.devices[INPUTCHANGE as usize].queued = 1;
        evt.grabbed = 0;
        evt.last_y = -1;
        evt.last_x = -1;
        ctx.evt = evt;
        self.evt_init_keymap();
    }

    /// Allocate the window's four unused bytes [soft 0x0860dde0 EVT_initWindow].
    pub(crate) fn evt_init_window(&mut self, _index: usize) {}

    /// Free them [soft 0x0860de00 EVT_exitWindow].
    pub(crate) fn evt_exit_window(&mut self, _index: usize) {}

    /// Whether a button is down right now, 1 or 0 [soft 0x0860de20 EVT_getButton].
    ///
    /// A keyboard key: the bit of its keycode in `XQueryKeymap`. A mouse button: its bit
    /// in the modifier mask of `XQueryPointer` on the root window (0x67 ↔ Button1, 0x66 ↔
    /// Button2, 0x65 ↔ Button3). Anything else 0. The display is locked around the query.
    pub(crate) fn evt_get_button(&mut self, dev: u16) -> i32 {
        self.ctx_get_ng();
        let (ctx, sys) = st!(self);
        let mut result = 0;
        sys.x.XLockDisplay(ctx.display);
        if is_button(dev) {
            if is_key(dev) {
                let keys = sys.x.XQueryKeymap(ctx.display);
                // the original reads the reverse table unchecked; device 170 can lie one
                // entry past its end (unallocated memory there, 0 here)
                let keycode = ctx.evt.keycode_of.get(dev as usize).copied().unwrap_or(0);
                let byte = (if keycode < 0 { keycode + 7 } else { keycode }) >> 3;
                let bit = (keycode as u8).wrapping_sub((byte << 3) as u8) & 0x1f;
                result = (keys[(byte & 0x1f) as usize] as i32 >> bit) & 1;
            } else if dev.wrapping_sub(0x65) < 3 {
                let (_, q) = sys.x.XQueryPointer(ctx.display, ctx.root);
                let buttons = (q[6] >> 8) as u8;
                if (dev == LEFTMOUSE && buttons & 1 != 0) || (dev == MIDDLEMOUSE && buttons & 2 != 0) || (dev == RIGHTMOUSE && buttons & 4 != 0) {
                    result = 1;
                }
            }
        }
        sys.x.XUnlockDisplay(ctx.display);
        result
    }

    /// The current value of a device [soft 0x0860df2c EVT_getValuator].
    ///
    /// A button: [`IrisGl::evt_get_button`]. MOUSEX: the pointer's x on the root window.
    /// MOUSEY: screen height − (pointer's y + 1). Both as 16-bit values, sign-extended.
    /// Any other valuator: 0 (the display is still locked and unlocked).
    pub(crate) fn evt_get_valuator(&mut self, dev: u16) -> i32 {
        self.ctx_get();
        if is_button(dev) {
            return self.evt_get_button(dev);
        }
        let (ctx, sys) = st!(self);
        let mut v: i16 = 0;
        sys.x.XLockDisplay(ctx.display);
        if dev < 0x10c && dev > 0x109 {
            let (_, q) = sys.x.XQueryPointer(ctx.display, ctx.root);
            v = 0;
            if dev == MOUSEX {
                v = q[2] as i16;
            }
            if dev == MOUSEY {
                v = (ctx.screen_height as i16).wrapping_sub((q[3] as i16).wrapping_add(1));
            }
        }
        sys.x.XUnlockDisplay(ctx.display);
        v as i32
    }

    /// Append an entry to the queue [soft 0x0860dfe4 EVT_addEvent].
    ///
    /// The stamp is `(clock() / 10000) % 100 + time(NULL) * 100`, in 32 bits.
    pub(crate) fn evt_add_event(&mut self, dev: u16, value: i16) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        let c = sys.c.clock();
        let t = sys.c.time();
        let stamp = ((c / 10000) % 100).wrapping_add(t.wrapping_mul(100)) as u32;
        ctx.evt.queue.push_back(Event { device: dev, value, time: stamp });
    }

    /// Throw away every pending X event that matches the mask, after an `XSync` if
    /// `sync` is set [soft 0x0860e090 EVT_killXEvents].
    pub(crate) fn evt_kill_x_events(&mut self, mask: u32, sync: i32) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        sys.x.XLockDisplay(ctx.display);
        if sync != 0 {
            sys.x.XSync(ctx.display, 0);
        }
        while sys.x.XCheckMaskEvent(ctx.display, mask) != 0 {}
        sys.x.XUnlockDisplay(ctx.display);
    }

    // ------------------------------------------------------------------ entry points

    /// IRIS GL `qenter`: append an entry, whatever the device [soft 0x0860e100 qenter].
    pub fn qenter(&mut self, dev: u16, value: i16) {
        self.sys.glx.glXWaitGL();
        self.evt_add_event(dev, value);
    }

    /// IRIS GL `qreset`: discard pending X events at the server (`XSync` with discard),
    /// translate what was already received, then empty the queue [soft 0x0860e12c qreset].
    pub fn qreset(&mut self) {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        sys.glx.glXWaitGL();
        sys.x.XSync(ctx.display, 1);
        self.evt_process_events();
        self.ctx.as_deref_mut().unwrap().evt.queue.clear();
    }

    /// IRIS GL `qtest`: the device of the first queue entry, 0 if the queue is empty,
    /// after translating the X events received [soft 0x0860e188 qtest].
    pub fn qtest(&mut self) -> u16 {
        self.ctx_get();
        self.sys.glx.glXWaitGL();
        self.sys.glx.glXWaitX();
        self.evt_process_events();
        match self.ctx.as_deref().unwrap().evt.queue.front() {
            Some(e) => e.device,
            None => 0,
        }
    }

    /// IRIS GL `qread`: take the first queue entry, waiting for one
    /// [soft 0x0860e1bc qread]. Returns (device, value).
    ///
    /// Waiting is a loop of translating the X events received and `usleep(0)`: it does not
    /// block in Xlib.
    pub fn qread(&mut self) -> (u16, i16) {
        self.ctx_get();
        self.sys.glx.glXWaitGL();
        self.sys.glx.glXWaitX();
        loop {
            self.evt_process_events();
            if let Some(e) = self.ctx.as_deref_mut().unwrap().evt.queue.pop_front() {
                return (e.device, e.value);
            }
            self.sys.c.usleep(0);
        }
    }

    /// IRIS GL `qdevice`: start queueing a device [soft 0x0860e238 qdevice]. Nothing
    /// happens if it is queued already.
    ///
    /// | Device | Effect besides the flag |
    /// |---|---|
    /// | a keyboard key, KEYBD | key press and release are added to every window's event mask |
    /// | RAWKEYBD (0x202) | every device of the key map is flagged, and the same mask change |
    /// | a mouse button | button press and release are added to the mask (before the flag is set) |
    /// | REDRAW | structure and exposure events are added |
    /// | INPUTCHANGE | focus changes are added |
    /// | WINFREEZE, WINTHAW | property changes are added |
    /// | 0x21d | nothing, not even the flag |
    /// | any other | the flag only |
    pub fn qdevice(&mut self, dev: u16) {
        self.ctx_get();
        let d = dev as usize;
        if self.ctx.as_deref().unwrap().evt.devices[d].queued != 0 {
            return;
        }
        self.sys.glx.glXWaitGL();
        self.sys.glx.glXWaitX();
        let evt = &mut self.ctx.as_deref_mut().unwrap().evt;
        if is_key(dev) || dev == KEYBD {
            evt.devices[d].queued = 1;
            self.wnd_event_mask(3, 0);
        } else if dev == RAWKEYBD {
            for k in 0..256 {
                let m = evt.keymap[k] as usize;
                evt.devices[m].queued = 1;
            }
            evt.devices[d].queued = 1;
            self.wnd_event_mask(3, 0);
        } else if dev == MOUSEX || dev == MOUSEY {
            evt.devices[d].queued = 1;
        } else if (RIGHTMOUSE..=LEFTMOUSE).contains(&dev) {
            self.wnd_event_mask(0xc, 0);
            self.ctx.as_deref_mut().unwrap().evt.devices[d].queued = 1;
        } else if dev == INPUTCHANGE {
            evt.devices[d].queued = 1;
            self.wnd_event_mask(0x20_0000, 0);
        } else if dev == REDRAW {
            evt.devices[d].queued = 1;
            self.wnd_event_mask(0x2_8000, 0);
        } else if dev == WINFREEZE || dev == WINTHAW {
            evt.devices[d].queued = 1;
            self.wnd_event_mask(0x40_0000, 0);
        } else if dev == 0x21d {
        } else {
            evt.devices[d].queued = 1;
        }
        self.sys.glx.glXWaitX();
    }

    /// IRIS GL `unqdevice`: stop queueing a device [soft 0x0860e444 unqdevice]. Nothing
    /// happens if it is not queued.
    ///
    /// The reverse of `qdevice`, with these differences: keyboard keys, KEYBD and RAWKEYBD
    /// only clear flags (key events stay in the mask); the button events leave the mask
    /// only when none of the three mouse buttons is queued any more; and the devices
    /// 0x20b, 0x217, 0x21a and 0x21d are left queued.
    pub fn unqdevice(&mut self, dev: u16) {
        self.ctx_get();
        let d = dev as usize;
        if self.ctx.as_deref().unwrap().evt.devices[d].queued == 0 {
            return;
        }
        self.sys.glx.glXWaitGL();
        self.sys.glx.glXWaitX();
        let evt = &mut self.ctx.as_deref_mut().unwrap().evt;
        if is_key(dev) || dev == MOUSEX || dev == MOUSEY {
            evt.devices[d].queued = 0;
        } else if matches!(dev, 0x20b | 0x217 | 0x21a | 0x21d) {
        } else if (RIGHTMOUSE..=LEFTMOUSE).contains(&dev) {
            evt.devices[d].queued = 0;
            if evt.devices[LEFTMOUSE as usize].queued == 0
                && evt.devices[MIDDLEMOUSE as usize].queued == 0
                && evt.devices[RIGHTMOUSE as usize].queued == 0
            {
                self.wnd_event_mask(0, 0xc);
            }
        } else if dev == RAWKEYBD {
            for k in 0..256 {
                let m = evt.keymap[k] as usize;
                evt.devices[m].queued = 0;
            }
            evt.devices[d].queued = 0;
        } else if dev == REDRAW {
            evt.devices[d].queued = 0;
            self.wnd_event_mask(0, 0x2_8000);
        } else if dev == INPUTCHANGE {
            evt.devices[d].queued = 0;
            self.wnd_event_mask(0, 0x20_0000);
        } else if dev == WINFREEZE || dev == WINTHAW {
            evt.devices[d].queued = 0;
            self.wnd_event_mask(0, 0x40_0000);
        } else {
            evt.devices[d].queued = 0;
        }
        self.sys.glx.glXWaitX();
    }

    /// IRIS GL `tie`: tie two valuators to a button [soft 0x0860e638 tie]. Ignored unless
    /// the device is a button.
    pub fn tie(&mut self, button: u16, valuator1: u16, valuator2: u16) {
        self.ctx_get();
        if is_button(button) {
            self.ctx.as_deref_mut().unwrap().evt.devices[button as usize].tie = [valuator1, valuator2];
        }
    }

    /// IRIS GL `getbutton` [soft 0x0860e684 getbutton].
    pub fn getbutton(&mut self, dev: u16) -> i32 {
        self.sys.glx.glXWaitGL();
        self.evt_get_button(dev)
    }

    /// IRIS GL `getvaluator` [soft 0x0860e6a8 getvaluator].
    pub fn getvaluator(&mut self, dev: u16) -> i32 {
        self.sys.glx.glXWaitGL();
        self.evt_get_valuator(dev)
    }

    /// IRIS GL `getdev`: the values of several devices [soft 0x0860e6cc getdev].
    pub fn getdev(&mut self, devs: &[u16], values: &mut [i16]) {
        self.sys.glx.glXWaitGL();
        for (i, &dev) in devs.iter().enumerate() {
            values[i] = self.evt_get_valuator(dev) as i16;
        }
    }

    /// The layer's `qcontrol` [soft 0x0860e710 qcontrol]: two commands, anything else
    /// returns 0 and does nothing.
    ///
    /// - 0x15: needs room for two shorts of output (else −1) and writes 1, 1.
    /// - 0x16: needs two shorts of input (else −1); both zero turns the keyboard's
    ///   auto-repeat off (`XAutoRepeatOff`), anything else turns it on.
    pub fn qcontrol(&mut self, cmd: i32, idata: &[i16], odata: &mut [i16]) -> i32 {
        self.ctx_get();
        let (ctx, sys) = st!(self);
        match cmd {
            0x15 => {
                if odata.len() < 2 {
                    return -1;
                }
                odata[0] = 1;
                odata[1] = 1;
            }
            0x16 => {
                if idata.len() < 2 {
                    return -1;
                }
                if idata[0] == 0 && idata[1] == 0 {
                    sys.x.XAutoRepeatOff(ctx.display);
                } else {
                    sys.x.XAutoRepeatOn(ctx.display);
                }
            }
            _ => {}
        }
        0
    }

    /// IRIS GL `setvaluator(dev, init, min, max)`: for MOUSEX and MOUSEY, move the pointer
    /// [soft 0x0860e788 setvaluator]. Nothing without a current window; any other device
    /// only clears the two "restricted" statics.
    ///
    /// A range other than the full screen (min ≠ 0 or max ≠ size − 1) marks the axis as
    /// restricted and remembers the wanted position. Once both axes are marked, the
    /// pointer is grabbed, confined to the current window, the marks are cleared and the
    /// pointer is warped to the remembered position. Otherwise an existing grab is
    /// released, and the pointer is warped to its present position with the one coordinate
    /// replaced.
    pub fn setvaluator(&mut self, dev: u16, init: i16, min: i16, max: i16) {
        self.ctx_get();
        self.ctx_get();
        let Some(c) = self.cur() else { return };
        if dev.wrapping_sub(MOUSEX) > 1 {
            self.grab_x = 0;
            self.grab_y = 0;
            return;
        }
        self.sys.glx.glXWaitGL();
        self.sys.glx.glXWaitX();
        let (ctx, sys) = (self.ctx.as_deref_mut().unwrap(), &mut self.sys);
        if dev == MOUSEX && (min != 0 || max as i32 != ctx.screen_width - 1) {
            self.grab_x = 1;
            self.grab_rx = init as i32;
        }
        if dev == MOUSEY && (min != 0 || max as i32 != ctx.screen_height - 1) {
            self.grab_y = 1;
            self.grab_ry = ctx.screen_height - (init as i32 + 1);
        }
        let mut warp = true;
        if self.grab_x == 0 || self.grab_y == 0 {
            if ctx.evt.grabbed != 0 {
                ctx.evt.grabbed = 0;
                sys.x.XUngrabPointer(ctx.display, 0);
            }
        } else {
            let xwindow = ctx.wnd.windows[c].xwindow;
            sys.x.XGrabPointer(ctx.display, xwindow, 1, 0, 1, 1, xwindow, 0, 0);
            ctx.evt.grabbed = 1;
            self.grab_y = 0;
            self.grab_x = 0;
            sys.x.XWarpPointer(ctx.display, 0, ctx.root, 0, 0, 0, 0, self.grab_rx, self.grab_ry);
            warp = false;
        }
        if warp {
            let (_, q) = sys.x.XQueryPointer(ctx.display, ctx.root);
            let (mut x, mut y) = (q[2] as i32, q[3] as i32);
            if dev == MOUSEX {
                x = init as i32;
            }
            if dev == MOUSEY {
                y = ctx.screen_height - (init as i32 + 1);
            }
            sys.x.XWarpPointer(ctx.display, 0, ctx.root, 0, 0, 0, 0, x, y);
        }
        sys.glx.glXWaitX();
    }
}
