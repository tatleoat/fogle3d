// SPDX-License-Identifier: MIT
//! Events (`event.c` of the program, 0x083939a0; not the IRIS GL layer's file of that name).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! The program has one event record in use at a time, `SI_activeEvent`. `SI_getMainEvent`
//! takes one entry from the IRIS GL queue, classifies its device (`SI_fndEvent`), and lets
//! the handler for that class fill the record: a key goes to the swift-key machinery or
//! picks a menu cell by its letter; a mouse button is hit-tested against the parts of the
//! screen (`SI_fndCanvasPart`) and handed to that part. The caller gets the record and acts
//! on its type.
//!
//! IRIS GL devices met here (finding 0016): 101, 102, 103 the right, middle and left mouse
//! buttons; 105 to 108 the buttons of the tablet's puck; 266, 267 the pointer; 270, 271 the
//! tablet's position; 513 KEYBD; 528 REDRAW; 534 INPUTCHANGE; 539 WINFREEZE; 540 WINTHAW;
//! 541; 542 WINQUIT; the rest are keys.

use crate::globals::*;
use crate::mem::{Addr, P};
use crate::records::{ev, off, Control, Event, Group};
use crate::sys::LogArg;
use crate::Ui;
use si_x87::{Round, F80};

/// `sprintf("%3.4f", v)` as the C library prints it.
fn fmt_3_4f(v: f64) -> Vec<u8> {
    if v.is_nan() {
        return if v.is_sign_negative() { b"-nan".to_vec() } else { b"nan".to_vec() };
    }
    if v.is_infinite() {
        return if v < 0.0 { b"-inf".to_vec() } else { b"inf".to_vec() };
    }
    format!("{:.4}", v).into_bytes()
}

/// `strtod(text, NULL)` for the decimal forms: blanks, a sign, digits with an optional
/// point and exponent; 0 when the text does not start with a number. (The C library also
/// reads hexadecimal numbers, "inf" and "nan"; the check's stand-in does not, and neither
/// does this.)
fn strtod(text: &[u8]) -> f64 {
    let mut i = 0;
    while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let start = i;
    if i < text.len() && (text[i] == b'-' || text[i] == b'+') {
        i += 1;
    }
    let digits = |mut j: usize| {
        while j < text.len() && text[j].is_ascii_digit() {
            j += 1;
        }
        j
    };
    let int_end = digits(i);
    let mut end;
    if int_end > i {
        end = int_end;
        if end < text.len() && text[end] == b'.' {
            end = digits(end + 1);
        }
    } else if i < text.len() && text[i] == b'.' && digits(i + 1) > i + 1 {
        end = digits(i + 1);
    } else {
        return 0.0;
    }
    if end < text.len() && (text[end] == b'e' || text[end] == b'E') {
        let mut j = end + 1;
        if j < text.len() && (text[j] == b'-' || text[j] == b'+') {
            j += 1;
        }
        if digits(j) > j {
            end = digits(j);
        }
    }
    std::str::from_utf8(&text[start..end]).ok().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0)
}

impl Ui {
    /// Whether an action string is one of the "Edit …" actions of the transformation boxes
    /// [soft 0x083939a0 si_actionIsOnEditSRT]: the string is looked up in `descSRT` by
    /// content; found, the answer is whether it starts with "Edit ".
    pub fn si_action_is_on_edit_srt(&mut self, action: Addr) -> i32 {
        let wanted = self.m.cstr(action);
        let n = self.m[SIZE_OF_DESC_SRT];
        let mut i = 0;
        while i < n {
            let s = self.m.cstr(self.m[DESC_SRT][i as usize]);
            if s == wanted {
                return s.starts_with(b"Edit ") as i32;
            }
            i += 1;
        }
        0
    }

    /// [soft 0x08393a08 SI_DoubleClickStatus]
    pub fn si_double_click_status(&mut self) -> i32 {
        self.m[DOUBLE_CLICK] as i32
    }

    /// [soft 0x08393a18 SI_getCurrentSwiftKey]
    pub fn si_get_current_swift_key(&mut self) -> i32 {
        self.m[DIRECT_KEY] as i32
    }

    /// Show the module the current swift key belongs to
    /// [soft 0x08393a28 SI_changeCurrentSwiftKeyModule].
    pub fn si_change_current_swift_key_module(&mut self) {
        let key = self.m[DIRECT_KEY] as i32;
        let module = self.o.keys.SI_getQuickModule(&mut self.m, key);
        if (0..=4).contains(&module) {
            self.si_set_active_canvas(module);
            self.m[SWIFTKEY_MODULE_CHANGED] = 1;
        }
    }

    /// True when a mouse button is down, or the tablet reports one.
    fn any_button(&mut self) -> bool {
        for b in [SI_BUTTON_LEFT, SI_BUTTON_MIDDLE, SI_BUTTON_RIGHT] {
            let dev = self.m[b];
            if self.o.gl.getbutton(&mut self.m, dev as u16) != 0 || self.m[SI_TABLET_BUTTON] == dev as i32 {
                return true;
            }
        }
        false
    }

    /// Whether the user has released a mouse button since the operation began: the test
    /// long operations make to let themselves be interrupted
    /// [soft 0x08393aa8 SI_mouseInterrupt]. With a button down and something in the queue, entries are read
    /// up to and including a mouse button's three (button, x, y); if that was a release, the
    /// next three are read too and the answer is 1.
    pub fn si_mouse_interrupt(&mut self) -> i32 {
        if self.m[SI_TABLET_ACTIVE] != 0 {
            self.o.devices.SI_tabletGetData(&mut self.m);
        }
        if self.any_button() && self.o.gl.qtest(&mut self.m) != 0 {
            let (mut value, mut x, mut y) = (0i16, 0i16, 0i16);
            loop {
                let dev = self.o.gl.qread(&mut self.m, &mut value);
                if (dev.wrapping_sub(0x65) as u32) <= 2 {
                    break;
                }
            }
            self.o.gl.qread(&mut self.m, &mut x);
            self.o.gl.qread(&mut self.m, &mut y);
            if value == 0 {
                if self.m[SI_TABLET_ACTIVE] != 0 {
                    while self.o.gl.qtest(&mut self.m) == 0 {
                        self.o.devices.SI_tabletGetData(&mut self.m);
                    }
                }
                self.o.gl.qread(&mut self.m, &mut value);
                self.o.gl.qread(&mut self.m, &mut x);
                self.o.gl.qread(&mut self.m, &mut y);
                return 1;
            }
        }
        0
    }

    /// A zeroed event record [soft 0x08393bc4 newEvent].
    pub fn new_event(&mut self) -> P<Event> {
        P::at(self.o.mem.SI_calloc(&mut self.m, 1, 0x18))
    }

    /// Start-up of the event handling [soft 0x08393bdc SI_initEvent]: keyboard auto-repeat
    /// off; devices 1, KEYBD, the swift-key keys, WINQUIT, WINFREEZE, WINTHAW, INPUTCHANGE
    /// and the three mouse buttons queued, each button tied to the pointer's x and y; the
    /// two event records allocated.
    pub fn si_init_event(&mut self) {
        self.m[SI_DEVICE_USE] = 0;
        self.si_disable_repeat();
        self.o.gl.qdevice(&mut self.m, 1);
        self.o.gl.qdevice(&mut self.m, 0x201);
        self.qdevice_supra_mode();
        self.o.gl.qdevice(&mut self.m, 0x21e);
        self.o.gl.qdevice(&mut self.m, 0x21b);
        self.o.gl.qdevice(&mut self.m, 0x21c);
        self.o.gl.qdevice(&mut self.m, 0x216);
        for b in [SI_BUTTON_LEFT, SI_BUTTON_MIDDLE, SI_BUTTON_RIGHT] {
            let (dev, cx, cy) = (self.m[b] as u16, self.m[SI_COORDX] as u16, self.m[SI_COORDY] as u16);
            self.o.gl.qdevice(&mut self.m, dev);
            self.o.gl.tie(&mut self.m, dev, cx, cy);
        }
        self.m[SI_ACTIVE_EVENT] = self.new_event();
        self.m[SI_ACTIVE_DIAL_EVENT] = self.new_event();
        self.o.keys.SI_initSupraKeys(&mut self.m);
    }

    /// The fixed positions of the lower part of the screen and of the module names in the
    /// title bar [soft 0x08393d00 SI_eventInitPos].
    ///
    /// The module names are laid out from the right end of the active canvas's title bar:
    /// for module 4 down to 0, x moves left by the width of the name plus 10 (7 on the small
    /// screen) and the module's position is x + 5 (not in the Dialog Editor, which has no
    /// modules). `serverY` is the bar's top minus the font's descender. Then, by screen
    /// size, the time line, the current-frame box, the status box and the value field of
    /// the transformation cells (see the finding for the numbers).
    pub fn si_event_init_pos(&mut self) {
        let canvas = self.m[SI_ACTIVE_CANVAS];
        let mut x = self.m[canvas].title_right;
        let top = self.m[canvas].title_top;
        let mut i: i32 = ((self.o.server.SI_isDialogEditor(&mut self.m) == 0) as i32) << 2;
        let descender = self.o.gl.getdescender(&mut self.m) as i16;
        self.m[SERVER_Y] = top.wrapping_sub(descender);
        let f = self.m[SI_IRISF];
        self.o.gl.font(&mut self.m, f);
        let gap: i16 = if self.m[SI_SCREEN_RES] == 1 { 7 } else { 10 };
        if self.o.server.SI_isDialogEditor(&mut self.m) == 0 {
            loop {
                // the network's word at +8 + 16 × module is that module's canvas; +4 its name
                let module_canvas = self.m.u32(self.m[SI_NETWORK].wrapping_add(8 + i as u32 * 0x10));
                let name = self.m.cstr(self.m.u32(module_canvas.wrapping_add(4)));
                let w = self.o.gl.strwidth(&mut self.m, &name) as i16;
                x = x.wrapping_sub(w.wrapping_add(gap));
                let at = match i {
                    0 => MODEL_X,
                    1 => MOTION_X,
                    2 => ACTOR_X,
                    3 => MATTER_X,
                    _ => TOOLS_X,
                };
                self.m[at] = x.wrapping_add(5);
                i -= 1;
                if i < 0 {
                    break;
                }
            }
        }
        let m = &mut self.m;
        let res = m[SI_SCREEN_RES];
        if res == 1 {
            m[CURR_FRM_R] = 0x397;
            m[CURR_FRM_B] = 0x2c;
            m[CURR_FRM_T] = 0x3c;
            m[SHOW_STAT_L] = 0x104;
            m[SHOW_STAT_R] = 0x154;
            m[SHOW_STAT_T] = 0x14;
            m[TIME_LIN_L] = 0x68;
            m[TIME_LIN_R] = 0x367;
            m[TIME_LIN_B] = 0x2a;
            m[TIME_LIN_T] = 0x3c;
            m[SI_LEFT_BOX] = 10;
            m[SI_RIGHT_BOX] = 0x41;
            m[SI_TOP_BOX] = 0xd;
        } else {
            if res == 0 {
                m[CURR_FRM_R] = 0x47d;
                m[TIME_LIN_R] = 0x441;
            } else {
                let wider: i16 = if res == 2 || res == 6 { 0x140 } else { 0x280 };
                m[TIME_LIN_R] = wider + 0x441;
                m[CURR_FRM_R] = wider + 0x47d;
            }
            m[TIME_LIN_L] = 0x82;
            m[SHOW_STAT_T] = 0x18;
            m[SHOW_STAT_R] = 0x168;
            m[SHOW_STAT_L] = 0x122;
            m[CURR_FRM_T] = 0x4c;
            m[CURR_FRM_B] = 0x37;
            m[TIME_LIN_B] = 0x35;
            m[TIME_LIN_T] = 0x4c;
            m[SI_LEFT_BOX] = 0xe;
            m[SI_RIGHT_BOX] = 0x59;
            m[SI_TOP_BOX] = 0x11;
        }
        m[LOGO_L] = 0x82;
        m[LOGO_R] = 0x16d;
        m[SHOW_STAT_B] = 0;
        m[CURR_FRM_L] = m[TIME_LIN_R];
        self.m[SI_ENV_DOCUMENTATION] = self.o.c.getenv(&mut self.m, b"SI_DOCUMENTATION");
    }

    /// Queue every key of the swift-key table [soft 0x08393fe4 qdeviceSupraMode].
    pub fn qdevice_supra_mode(&mut self) {
        let mut p = SUPRA_KEYS.cast::<i16>();
        while self.m[p] != 0 {
            let key = self.m[p];
            p = p.add(1);
            self.o.gl.qdevice(&mut self.m, key as u16);
        }
    }

    /// The reverse [soft 0x08394020 SI_unqdeviceSupraMode].
    pub fn si_unqdevice_supra_mode(&mut self) {
        let mut p = SUPRA_KEYS.cast::<i16>();
        while self.m[p] != 0 {
            let key = self.m[p];
            p = p.add(1);
            self.o.gl.unqdevice(&mut self.m, key as u16);
        }
    }

    /// [soft 0x08394058 SI_getEvent]
    pub fn si_get_event(&mut self) -> P<Event> {
        self.si_get_main_event(0)
    }

    /// The next event for the main loop [soft 0x0839406c SI_getMainEvent].
    ///
    /// First whatever went on is stopped (`SI_stopActivity`) and, outside a swift-key mode,
    /// **input typed or clicked ahead is thrown away**: queue entries are read and dropped
    /// until the queue is empty or its head is one of REDRAW, INPUTCHANGE, WINFREEZE,
    /// WINTHAW or 541. Then an event pushed back earlier is handed out, or the next queue
    /// entry is read and handled. Outside the Dialog Editor the event is passed through
    /// `si_eventFilter`; if that claims it, the event's type becomes 0.
    pub fn si_get_main_event(&mut self, replay: i16) -> P<Event> {
        self.si_stop_activity();
        if self.m[SI_SUPRA_MODE] == 0 {
            loop {
                let dev = self.o.gl.qtest(&mut self.m);
                if dev == 0 || dev == 0x216 || dev == 0x210 || (0x21b..0x21e).contains(&dev) {
                    break;
                }
                let mut dropped = 0i16;
                self.o.gl.qread(&mut self.m, &mut dropped);
            }
        }
        if self.m[IS_E] == 0 {
            let event = self.si_get_next_event(replay);
            if self.o.server.SI_isDialogEditor(&mut self.m) == 0 && self.si_event_filter(event) as i16 != 0 {
                // The original calls SI_licHeartbeat(flexlm_job) here, a call into the
                // licence client. The Rust program has no licence manager (AGENTS.md): the
                // call is not made, the rest of the branch is.
                self.m[event].kind = ev::NONE;
            }
            event
        } else {
            self.pop_qe()
        }
    }

    /// Read one queue entry and turn it into the active event
    /// [soft 0x0839412c si_getNextEvent].
    ///
    /// With `replay` set and a swift key being replayed (and the current module not in a
    /// mode of its own), no entry is read: the key press of the swift key's sequence is
    /// handled again. Otherwise the event is cleared, the queue is read (this is where the
    /// program waits) and the entry is handled by its class.
    pub fn si_get_next_event(&mut self, replay: i16) -> P<Event> {
        let mut value = 0i16;
        if self.m[SI_SUPRA_MODE] != 0 {
            self.check_keyboard();
        }
        let active = self.m[SI_ACTIVE];
        let read = replay == 0
            || self.m[SWIFTMODE] == -1
            || (active == 0 && self.m[SI_MDL_MOD] != 0)
            || (active == 1 && self.o.modules.SI_scnGetMode(&mut self.m) != 0)
            || (active == 2 && self.o.modules.SI_actorGetMode(&mut self.m) != 0)
            || (active == 3 && self.o.modules.SI_mtrGetMode(&mut self.m) != 0)
            || (active == 4 && self.o.modules.SI_toolsGetMode(&mut self.m) != 0);
        if !read {
            let key = self.m[SWIFTMODE] as i32;
            let dev = self.o.keys.SI_getQuickKeySequence(&mut self.m, key, 0);
            self.key_down(dev, value);
            return self.m[SI_ACTIVE_EVENT];
        }
        let event = self.m[SI_ACTIVE_EVENT];
        self.m[event].kind = ev::NONE;
        self.m[SWIFTMODE] = -1;
        if self.m[SI_TABLET_ACTIVE] != 0 {
            while self.o.gl.qtest(&mut self.m) == 0 {
                self.o.devices.SI_tabletGetData(&mut self.m);
            }
        }
        let dev = self.o.gl.qread(&mut self.m, &mut value);
        let event = self.m[SI_ACTIVE_EVENT];
        self.m[event].device = dev;
        let (mut x, mut y) = (0i16, 0i16);
        match self.si_fnd_event(dev, value) {
            1 => {
                self.m[GB_REDRAW_ROTOSCOPY] = 1;
                self.m[DOUBLE_CLICK] = 0;
                self.o.journal.SI_keyLog(&mut self.m, dev, value);
                self.key_down(dev, value);
            }
            2 => {
                self.m[GB_REDRAW_ROTOSCOPY] = 1;
                self.m[DOUBLE_CLICK] = 0;
                self.o.journal.SI_keyLog(&mut self.m, dev, value);
                self.key_up(dev);
            }
            3 => {
                self.m[GB_REDRAW_ROTOSCOPY] = 1;
                self.o.gl.qread(&mut self.m, &mut x);
                self.o.gl.qread(&mut self.m, &mut y);
                self.o.journal.SI_mouseLog(&mut self.m, dev, value, x, y);
                self.si_mouse_down_(dev, value, x, y);
            }
            4 => {
                self.m[GB_REDRAW_ROTOSCOPY] = 1;
                self.o.gl.qread(&mut self.m, &mut x);
                self.o.gl.qread(&mut self.m, &mut y);
                self.o.journal.SI_mouseLog(&mut self.m, dev, value, x, y);
            }
            5 => {
                self.m[DOUBLE_CLICK] = 0;
                self.puck_down(dev);
            }
            6 => self.m[DOUBLE_CLICK] = 0,
            7 => {
                // a REDRAW: count two redraws for every visible view that shows shaded
                self.m[DOUBLE_CLICK] = 0;
                self.m[GN_3D_REDRAWS_NEEDED] = 0;
                let n = self.m[SI_TEMPLATE].n_views as i32;
                let views = self.m[SI_TEMPLATE].views;
                for i in 0..n.max(0) {
                    let g = views.add(i);
                    if self.m[g].visible != 0 && self.m[g].display == 2 {
                        self.m[GN_3D_REDRAWS_NEEDED] += 2;
                    }
                }
                let event = self.m[SI_ACTIVE_EVENT];
                self.m[event].kind = ev::REDRAW;
            }
            8 => {
                self.m[DOUBLE_CLICK] = 0;
                let event = self.m[SI_ACTIVE_EVENT];
                self.m[event].kind = ev::WINFREEZE;
            }
            9 => {
                self.m[DOUBLE_CLICK] = 0;
                let event = self.m[SI_ACTIVE_EVENT];
                self.m[event].kind = ev::WINTHAW;
            }
            0xb => {
                self.m[DOUBLE_CLICK] = 0;
                let event = self.m[SI_ACTIVE_EVENT];
                self.m[event].kind = ev::WINQUIT;
            }
            0xc => {
                self.m[DOUBLE_CLICK] = 0;
                self.o.journal.SI_keyLog(&mut self.m, dev, value);
                self.keyb(dev, value);
            }
            _ => {}
        }
        self.m[SI_ACTIVE_EVENT]
    }

    /// Show a module: 0 Model, 1 Motion, 2 Actor, 3 Matter, 4 Tools
    /// [soft 0x08394450 si_setActiveCanvas]. The module switch is driven as if its name in the title bar had
    /// been clicked; then the mode box is redrawn (also for a number that is no module).
    pub fn si_set_active_canvas(&mut self, module: i32) {
        let x = match module {
            0 => Some(MODEL_X),
            1 => Some(MOTION_X),
            2 => Some(ACTOR_X),
            3 => Some(MATTER_X),
            4 => Some(TOOLS_X),
            _ => None,
        };
        if let Some(x) = x {
            let (canvas, x, y) = (self.m[SI_ACTIVE_CANVAS], self.m[x], self.m[SERVER_Y]);
            self.o.server.si_serverMonitor(&mut self.m, canvas, x, y);
        }
        self.o.boxes.SI_modeBoxRefresh(&mut self.m);
    }

    fn wait_for_tablet(&mut self) {
        if self.m[SI_TABLET_ACTIVE] != 0 {
            while self.o.gl.qtest(&mut self.m) == 0 {
                self.o.devices.SI_tabletGetData(&mut self.m);
            }
        }
    }

    /// Open the menu of a control for the button that chose it and report whether the menu
    /// produced an event [soft 0x083944f0 launchMenu].
    ///
    /// - right button (101), or 108: nothing opens; the button's x and y are taken off the
    ///   queue if anything is there, the event is cleared, the answer is 0.
    /// - middle button (102): the three queue entries are read and the menu's last choice
    ///   is repeated (`SI_popupFindLastSelect` fills the event).
    /// - puck button 105: one entry is read, then the last choice is repeated.
    /// - puck button 106: one entry is read and the menu opens at the tablet's position.
    /// - left button (103): the three entries are read and the menu opens at their x, y.
    /// - anything else (a key): the menu opens at the pointer.
    pub fn launch_menu(&mut self, device: i32, control: P<Control>) -> i32 {
        let (mut v, mut x, mut y) = (0i16, 0i16, 0i16);
        let event = |ui: &Ui| ui.m[SI_ACTIVE_EVENT];
        match device {
            0x65 | 0x6c => {
                if self.o.gl.qtest(&mut self.m) != 0 {
                    self.o.gl.qread(&mut self.m, &mut x);
                    self.o.gl.qread(&mut self.m, &mut y);
                }
                let e = event(self);
                self.m[e].kind = ev::NONE;
                return 0;
            }
            0x66 | 0x69 => {
                if device == 0x66 {
                    self.wait_for_tablet();
                    self.o.gl.qread(&mut self.m, &mut v);
                    self.o.gl.qread(&mut self.m, &mut x);
                    self.o.gl.qread(&mut self.m, &mut y);
                } else {
                    self.o.gl.qread(&mut self.m, &mut v);
                    self.o.gl.getvaluator(&mut self.m, 0x10e);
                    self.o.gl.getvaluator(&mut self.m, 0x10f);
                }
                let e = event(self).addr();
                let menu = self.m[control].menu;
                self.o.menus.SI_popupFindLastSelect(&mut self.m, menu, e, e + 4, e + 6, e + 0x10);
            }
            _ => {
                if device == 0x6a {
                    self.o.gl.qread(&mut self.m, &mut v);
                    x = self.o.gl.getvaluator(&mut self.m, 0x10e) as i16;
                    y = self.o.gl.getvaluator(&mut self.m, 0x10f) as i16;
                } else {
                    self.wait_for_tablet();
                    if device == 0x67 {
                        self.o.gl.qread(&mut self.m, &mut v);
                        self.o.gl.qread(&mut self.m, &mut x);
                        self.o.gl.qread(&mut self.m, &mut y);
                    } else {
                        x = self.o.gl.getvaluator(&mut self.m, 0x10a) as i16;
                        y = self.o.gl.getvaluator(&mut self.m, 0x10b) as i16;
                    }
                }
                let e = event(self);
                let menu = self.m[control].menu;
                self.o.menus.si_menuSelect(&mut self.m, menu, e, x, y);
            }
        }
        let e = event(self);
        (self.m[e].kind != 0) as i32
    }

    /// Take the newest pushed-back event [soft 0x083946f0 popQE]: the first entry of `qE`
    /// is copied to `dEvt`, the others move down one place.
    pub fn pop_qe(&mut self) -> P<Event> {
        self.m[D_EVT] = self.m[Q_E][0];
        let n = self.m[NB_E] as i32;
        let mut i = 1;
        while i < n {
            // the original has no bound of its own: nbE never exceeds 9
            let e = self.m[Q_E.cast::<Event>().add(i)];
            self.m[Q_E.cast::<Event>().add(i - 1)] = e;
            i += 1;
        }
        self.m[NB_E] = (n - 1) as i16;
        self.m[IS_E] = (self.m[NB_E] != 0) as i16;
        D_EVT
    }

    /// Push an event back, to be handed out by the next `SI_getMainEvent` before anything
    /// is read from the queue [soft 0x0839476c si_pushQE]. The entries move up one place
    /// and the new one becomes the first; at most 9 are counted (a tenth push overwrites
    /// the last place). The new entry's `extra` and `device` keep what the first place held.
    pub fn si_push_qe(&mut self, kind: i32, first: i16, second: i16, x: i16, y: i16, data: Addr) {
        let n = self.m[NB_E] as i32;
        let q = Q_E.cast::<Event>();
        let mut i = n - 1;
        while i >= 0 {
            let e = self.m[q.add(i)];
            self.m[q.add(i + 1)] = e;
            i -= 1;
        }
        let e = &mut self.m[q];
        e.kind = kind;
        e.first = first;
        e.second = second;
        e.x = x;
        e.y = y;
        e.data = data;
        self.m[IS_E] = 1;
        self.m[NB_E] = (n + 1).min(9) as i16;
    }

    /// The class of a queue entry [soft 0x08394828 SI_fndEvent]:
    ///
    /// | Result | Device | Value |
    /// |---|---|---|
    /// | 1 key down, 2 key up | 3, 5–12, 14–59, 61–70, 73–76, 80–83, 143–157, 167–169 | non-zero, zero |
    /// | 1 | 4 | any |
    /// | 3 mouse down, 4 mouse up | 101, 102, 103 | non-zero, zero |
    /// | 5 puck down, 6 puck up | 105–108 | non-zero, zero |
    /// | 12 | 513 (KEYBD) | |
    /// | 7 | 528 (REDRAW) | |
    /// | 8, 9, 10, 11 | 539, 540, 541, 542 | |
    /// | 0 | everything else | |
    ///
    /// Device 534 (INPUTCHANGE) is handled on the spot and answers 0: further INPUTCHANGE
    /// entries at the head of the queue are read, the last value counting; if it names the
    /// main window or an open dialog's window, that window becomes the focus window of the
    /// template and the current IRIS GL window (with the normal draw mode for the main one).
    pub fn si_fnd_event(&mut self, device: i32, value: i16) -> i32 {
        let key = |value: i16| if value == 0 { 2 } else { 1 };
        match device {
            0x65..=0x67 => {
                if value == 0 {
                    4
                } else {
                    3
                }
            }
            4 => 1,
            3 | 5..=0xc | 0xe..=0x3b | 0x3d..=0x46 | 0x49..=0x4c | 0x50..=0x53 | 0x8f..=0x9d | 0xa7..=0xa9 => key(value),
            0x69..=0x6c => {
                if value == 0 {
                    6
                } else {
                    5
                }
            }
            0x201 => 0xc,
            0x210 => 7,
            0x21b => 8,
            0x21c => 9,
            0x21d => 10,
            0x21e => 0xb,
            0x216 => {
                let mut value = value;
                while self.o.gl.qtest(&mut self.m) == 0x216 {
                    self.o.gl.qread(&mut self.m, &mut value);
                }
                if value != 0
                    && (value as i32 == self.m[SI_SYSID] || self.o.dialogs.si_dialIsOpenWinId(&mut self.m, value as i32) != 0)
                {
                    self.m[SI_TEMPLATE].winid = value;
                    self.o.gl.winset(&mut self.m, value as i32);
                    if value as i32 == self.m[SI_SYSID] {
                        self.o.gl.drawmode(&mut self.m, 0x10);
                    }
                }
                0
            }
            _ => 0,
        }
    }

    /// Which part of the screen a point is in [soft 0x08394a9c SI_fndCanvasPart]:
    ///
    /// | Result | Part |
    /// |---|---|
    /// | 9 | a dialog (while dialogs are open, the current IRIS GL window is asked for its dialog; in the Dialog Editor, the edited dialog's rectangle with 20 pixels above it) |
    /// | 5, 14 | the canvas's title bar; 14 for x in 130..=365 (the logo) |
    /// | 2 | the left menu column |
    /// | 3 | the right menu column |
    /// | 1 | the views |
    /// | 6 | the colour box |
    /// | 7 | the play box |
    /// | 8 | the current-frame box |
    /// | 10 | the status box |
    /// | 4 | the time line |
    /// | 13 | the mode box |
    /// | 0 | nothing; and in the Dialog Editor everything from the colour box on |
    ///
    /// Without open dialogs the main window is made the current IRIS GL window first.
    pub fn si_fnd_canvas_part(&mut self, canvas: P<crate::records::Canvas>, x: i32, y: i32) -> i32 {
        if self.m[SI_DIALOG_ACTIVE] == 0 {
            let id = self.m[SI_SYSID];
            self.o.gl.winset(&mut self.m, id);
        } else {
            let w = self.o.gl.winget(&mut self.m);
            let dialog = self.o.dialogs.si_fndDialogActive(&mut self.m, w);
            if dialog != 0 {
                self.m[DIALOG_HIT] = dialog;
                let id = self.m.u32(dialog + off::DIALOG_WINID);
                self.o.gl.winset(&mut self.m, id as i32);
                self.m[SI_TEMPLATE].winid = self.m.u32(dialog + off::DIALOG_WINID) as i16;
                return 9;
            }
        }
        if self.o.server.SI_isDialogEditor(&mut self.m) != 0 {
            let id = self.m[DE_DIALOG_SYS] as i32;
            let d = self.o.dialogs.SI_dialogGet(&mut self.m, id);
            if d != 0 {
                let r = |i: u32| self.m.i16(d + off::DIALOG_RECT + 2 * i) as i32;
                if !(x < r(0) || r(1) < x || y < r(2) || r(3) + 0x14 < y) {
                    return 9;
                }
            }
        }
        let c = self.m[canvas];
        let inside = |l: i16, r: i16, b: i16, t: i16| l as i32 <= x && x <= r as i32 && b as i32 <= y && y <= t as i32;
        if inside(c.title_left, c.title_right, c.title_bottom, c.title_top) {
            self.m[LOGO_R] = 0x16d;
            self.m[LOGO_L] = 0x82;
            return if !(0x82..=0x16d).contains(&x) { 5 } else { 0xe };
        }
        if inside(c.left_left, c.left_right, c.left_bottom, c.left_top) {
            return 2;
        }
        if inside(c.right_left, c.right_right, c.right_bottom, c.right_top) {
            return 3;
        }
        if inside(c.views_left, c.views_right, c.views_bottom, c.views_top) {
            return 1;
        }
        let g = |p: P<i16>| self.m[p] as i32;
        let part = if !(x < g(SI_COLOR_BOX_L) - 2 || g(SI_COLOR_BOX_R) < x || y < g(SI_COLOR_BOX_B) - 2 || g(SI_COLOR_BOX_T) < y) {
            6
        } else if !(x < g(SI_PLAY_BOX_L) || g(SI_PLAY_BOX_R) < x || y < g(SI_PLAY_BOX_B) - 2 || g(SI_PLAY_BOX_T) < y) {
            7
        } else if !(x < g(CURR_FRM_L) || g(CURR_FRM_R) < x || y < g(CURR_FRM_B) || g(CURR_FRM_T) < y) {
            8
        } else if !(x < g(SHOW_STAT_L) || g(SHOW_STAT_R) < x || y < g(SHOW_STAT_B) || g(SHOW_STAT_T) < y) {
            10
        } else if !(x < g(TIME_LIN_L) || g(TIME_LIN_R) < x || y < g(TIME_LIN_B) || g(TIME_LIN_T) < y) {
            4
        } else {
            let mode = self.m[SI_MODEPOS];
            if x < mode[0] as i32 || (mode[1] as i32) < x || y < 0 || (mode[3] as i32) < y {
                return 0;
            }
            0xd
        };
        if self.o.server.SI_isDialogEditor(&mut self.m) == 0 {
            part
        } else {
            0
        }
    }

    /// Whether the mouse button of a device is down now [soft 0x08394e0c SI_mouseDown]:
    /// 103 asks the left button, 102 the middle one, anything else the right one.
    pub fn si_mouse_down(&mut self, device: i32) -> i32 {
        let b = match device {
            0x67 => SI_BUTTON_LEFT,
            0x66 => SI_BUTTON_MIDDLE,
            _ => SI_BUTTON_RIGHT,
        };
        let dev = self.m[b] as u16;
        self.o.gl.getbutton(&mut self.m, dev) as i16 as i32
    }

    /// The pointer's position [soft 0x08394e54 SI_getMouse].
    pub fn si_get_mouse(&mut self) -> (i16, i16) {
        let cx = self.m[SI_COORDX] as u16;
        let x = self.o.gl.getvaluator(&mut self.m, cx) as i16;
        let cy = self.m[SI_COORDY] as u16;
        let y = self.o.gl.getvaluator(&mut self.m, cy) as i16;
        (x, y)
    }

    /// Queue the keys of text entry, with auto-repeat as it was found
    /// [soft 0x08394e94 SI_queueKeyboard]: the key pad's digits and signs, tab, backspace, and the
    /// punctuation keys.
    pub fn si_queue_keyboard(&mut self) {
        self.si_restore_repeat();
        for dev in [
            0x3b, 0x3a, 0x40, 0x41, 0x3f, 0x45, 0x46, 0x43, 0x44, 0x4b, 9, 0x3e, 0x34, 0x2b, 4, 0x2f, 0x36, 0x35, 0x52, 0x37,
            0x31, 0x38, 0x39, 0x2d, 0x32, 0xa7, 0xa8, 0xa9, 0x4c, 0x42,
        ] {
            self.o.gl.qdevice(&mut self.m, dev);
        }
    }

    /// The reverse, with auto-repeat off [soft 0x08395004 SI_unqueueKeyboard] (the same
    /// keys; 4 comes two places later in this list).
    pub fn si_unqueue_keyboard(&mut self) {
        self.si_disable_repeat();
        for dev in [
            0x3b, 0x3a, 0x40, 0x41, 0x3f, 0x45, 0x46, 0x43, 0x44, 0x4b, 9, 0x3e, 0x34, 0x2b, 0x2f, 0x36, 4, 0x35, 0x52, 0x37,
            0x31, 0x38, 0x39, 0x2d, 0x32, 0xa7, 0xa8, 0xa9, 0x4c, 0x42,
        ] {
            self.o.gl.unqdevice(&mut self.m, dev);
        }
    }

    /// [soft 0x08395174 si_resetKeyboard]
    pub fn si_reset_keyboard(&mut self) {
        self.si_restore_repeat();
    }

    /// The character of a key device, for a US keyboard [soft 0x08395184 SI_keyboardToCar].
    /// `shift` selects the upper symbol; a letter is upper case with `shift` or `caps`.
    /// The key pad's digits and signs have devices of their own. Anything else is `?`.
    pub fn si_keyboard_to_car(&mut self, device: i32, shift: i16, caps: i16, out: P<u8>) {
        let s = shift != 0;
        let letter = |c: u8| if shift == 0 && caps == 0 { c } else { c.to_ascii_uppercase() };
        let two = |plain: u8, shifted: u8| if s { shifted } else { plain };
        let c = match device {
            8 => two(b'1', b'!'),
            0x3a => b'1',
            10 => letter(b'q'),
            0xb => letter(b'a'),
            0xc => letter(b's'),
            0xe => two(b'2', b'@'),
            0x40 => b'2',
            0xf => two(b'3', b'#'),
            0x41 => b'3',
            0x10 => letter(b'w'),
            0x11 => letter(b'e'),
            0x12 => letter(b'd'),
            0x13 => letter(b'f'),
            0x14 => letter(b'z'),
            0x15 => letter(b'x'),
            0x16 => two(b'4', b'$'),
            0x3f => b'4',
            0x17 => two(b'5', b'%'),
            0x45 => b'5',
            0x18 => letter(b'r'),
            0x19 => letter(b't'),
            0x1a => letter(b'g'),
            0x1b => letter(b'h'),
            0x1c => letter(b'c'),
            0x1d => letter(b'v'),
            0x1e => two(b'6', b'^'),
            0x46 => b'6',
            0x1f => two(b'7', b'&'),
            0x43 => b'7',
            0x20 => letter(b'y'),
            0x21 => letter(b'u'),
            0x22 => letter(b'j'),
            0x23 => letter(b'k'),
            0x24 => letter(b'b'),
            0x25 => letter(b'n'),
            0x26 => two(b'8', b'*'),
            0xa8 => b'*',
            0x27 => two(b'9', b'('),
            0x4b => b'9',
            0x28 => letter(b'i'),
            0x29 => letter(b'o'),
            0x2a => letter(b'l'),
            0x2b => two(b';', b':'),
            0x2c => letter(b'm'),
            0x2d => two(b',', b'<'),
            0x2e => two(b'0', b')'),
            0x3b => b'0',
            0x2f => two(b'-', b'_'),
            0x4c => b'-',
            0x30 => letter(b'p'),
            0x31 => two(b'[', b'{'),
            0x32 => two(b'\'', b'"'),
            0x34 => two(b'.', b'>'),
            0x42 => b'.',
            0x35 => two(b'/', b'?'),
            0x36 => two(b'=', b'+'),
            0xa9 => b'+',
            0x37 => two(b'`', b'~'),
            0x38 => two(b']', b'}'),
            0x39 => two(b'\\', b'|'),
            0x44 => b'8',
            0x53 => b' ',
            0xa7 => b'/',
            _ => b'?',
        };
        self.m[out] = c;
    }

    /// Wait for the vertical retrace [soft 0x083956b8 scanSync]; the original then counts
    /// down from 1023 − `line` in an empty loop, which has no effect to reproduce.
    pub fn scan_sync(&mut self, _line: i16) {
        self.o.gl.gsync(&mut self.m);
    }

    /// Follow the pointer along the time line while a mouse button is held
    /// [soft 0x083956e4 SI_setTimeline]. `x` is where the press was.
    ///
    /// Each time the pointer's x has changed: with the left or middle button the frame
    /// slider is set from x (`si_fsliderSet`) and the curve dialogs follow; with the right
    /// button the frame is computed here, `start + trunc(((x − left) + s/2) / s)` with
    /// `s = length / (end − start)` pixels per frame, limited to the range, and, when it
    /// changed, becomes the current frame, the dope sheet and curve views (or the selected
    /// objects) are refreshed and the slider redrawn. On release: a mouse-log entry, and in
    /// the program itself the scene is brought to the frame if fast playback is on, and all
    /// controls are refreshed.
    pub fn si_set_timeline(&mut self, x: i32) {
        let mut x = x;
        let mut last = 0i32;
        let mut first = 1i16;
        if self.m[SI_PLAY_BOX_MODE] == -1 {
            return;
        }
        if self.m[SI_PLAY_BOX_MODE] == 0 {
            self.o.motion.SI_fastPlayClearAllView(&mut self.m);
        }
        self.m[SI_PLAY_BACK_RUN] = 1;
        'held: while self.any_button() {
            if self.m[SI_PLAY_BOX_MODE] == 0 && self.m[SI_TABLET_ACTIVE] != 0 {
                self.o.devices.SI_tabletGetButtonUpEvent(&mut self.m);
            }
            'step: {
                if last == x {
                    break 'step;
                }
                last = x;
                let right = self.m[SI_BUTTON_RIGHT];
                if self.o.gl.getbutton(&mut self.m, right as u16) == 0 && self.m[SI_TABLET_BUTTON] != right as i32 {
                    if self.o.server.SI_isDialogEditor(&mut self.m) == 0 {
                        self.o.boxes.si_fsliderSet(&mut self.m, x, first as i32);
                    }
                    if self.m[SI_PLAY_BOX_MODE] == 0 {
                        self.o.dialogs.SI_dialogUpdateFcv(&mut self.m);
                    }
                    break 'step;
                }
                let (start, end) = (self.m[SI_FRAME_START], self.m[SI_FRAME_END]);
                // all in the 80-bit registers; the conversion truncates
                let scale = F80::from_i32(self.m[SI_STATUS_LEN] as i32) / F80::from_i32(end.wrapping_sub(start));
                let offset = F80::from_i32(x.wrapping_sub(self.m[SI_STATUS_RIGHT2] as i32));
                let t = (scale * 0.5f64 + offset) / scale;
                let mut frame = start.wrapping_add(t.to_i32(Round::Zero));
                if frame < start {
                    frame = start;
                }
                if frame > end {
                    frame = end;
                }
                if first == 0 && frame == self.m[SI_FRAME_CURRENT] {
                    break 'step;
                }
                self.m[SI_FRAME_CURRENT] = frame;
                if self.o.server.SI_isDialogEditor(&mut self.m) == 0 && self.m[SI_PLAY_BOX_MODE] == 1 {
                    // the stand-alone viewer: draw the slider and look at the buttons again
                    // without reading the pointer
                    self.o.boxes.SI_fsliderDraw(&mut self.m, frame);
                    continue 'held;
                }
                if self.m[DAT_08781482] == 0 {
                    self.o.views.SI_refreshDopeSheet(&mut self.m, 0);
                    self.o.views.SI_refreshFcurve(&mut self.m, 1);
                } else {
                    let seconds = frame as f32 / self.m[SI_FRAME_RATE];
                    self.o.motion.SI_sceneKeySetColor(&mut self.m, seconds, 1, 0);
                    self.o.views.SI_refreshFcurve(&mut self.m, 0);
                    self.o.views.SI_refreshSelected(&mut self.m, 0);
                }
                if self.o.server.SI_isDialogEditor(&mut self.m) == 0 {
                    let f = self.m[SI_FRAME_CURRENT];
                    self.o.boxes.SI_fsliderDraw(&mut self.m, f);
                }
            }
            let cx = self.m[SI_COORDX] as u16;
            x = self.o.gl.getvaluator(&mut self.m, cx);
            first = 0;
        }
        let cy = self.m[SI_COORDY] as u16;
        let y = self.o.gl.getvaluator(&mut self.m, cy) as i16;
        let cx = self.m[SI_COORDX] as u16;
        let px = self.o.gl.getvaluator(&mut self.m, cx) as i16;
        self.o.journal.SI_mouseLog(&mut self.m, 4, 0, px, y);
        self.m[SI_PLAY_BACK_RUN] = 0;
        if self.m[SI_PLAY_BOX_MODE] == 0 {
            if self.m[SI_FAST_PLAY_BACK_ACTIVE] != 0 {
                let seconds = self.m[SI_FRAME_CURRENT] as f32 / self.m[SI_FRAME_RATE];
                self.o.motion.SI_sceneUpdate(&mut self.m, seconds);
            }
            self.o.controls.SI_refreshAllControls(&mut self.m);
        }
    }

    /// Mark a group as held by a swift-key mode [soft 0x08395a8c SI_activeSupraMode]: state
    /// 2 becomes 999, any other 99, for the group and its first control; the group is
    /// redrawn, and for a transformation box all controls are refreshed.
    pub fn si_active_supra_mode(&mut self, id: i16, column: i16) {
        let g = self.find_control(id as i32, column as i32);
        if !g.is_null() {
            let state = if self.m[g].state == 2 { 999 } else { 99 };
            self.m[g].state = state;
            let c = self.m[g].controls;
            self.m[c].state = state;
            self.o.controls.si_groupDraw(&mut self.m, g, 1);
            if (self.m[g].id.wrapping_sub(1) as u16) < 3 {
                self.o.controls.SI_refreshAllControls(&mut self.m);
            }
        }
    }

    /// Undo that [soft 0x08395afc SI_deactiveSupraMode]: 999 becomes 2, anything else 0;
    /// and no swift-key mode is in force any more.
    pub fn si_deactive_supra_mode(&mut self, id: i16, column: i16) {
        let g = self.find_control(id as i32, column as i32);
        if !g.is_null() {
            let state = if self.m[g].state == 999 { 2 } else { 0 };
            self.m[g].state = state;
            let c = self.m[g].controls;
            self.m[c].state = state;
            self.o.controls.si_groupDraw(&mut self.m, g, 1);
            if (self.m[g].id.wrapping_sub(1) as u16) < 3 {
                self.o.controls.SI_refreshAllControls(&mut self.m);
            }
        }
        self.m[SI_SUPRA_MODE] = 0;
    }

    /// The group with an identifier in a menu column of the active canvas, 0 the left one,
    /// anything else the right one; null if there is none [soft 0x08395b74 findControl].
    pub fn find_control(&mut self, id: i32, column: i32) -> P<Group> {
        let canvas = self.m[self.m[SI_ACTIVE_CANVAS]];
        let (n, groups) = if column == 0 { (canvas.n_left, canvas.left_groups) } else { (canvas.n_right, canvas.right_groups) };
        for i in 0..(n as i32).max(0) {
            if self.m[groups.add(i)].id as i32 == id {
                return groups.add(i);
            }
        }
        P::NULL
    }

    /// Keyboard auto-repeat off, after remembering the setting once
    /// [soft 0x08395c10 si_disableRepeat]. Not in the Dialog Editor.
    pub fn si_disable_repeat(&mut self) {
        if self.o.server.SI_isDialogEditor(&mut self.m) == 0 {
            if self.m[SAVED_KEYWARP] == 0 {
                self.o.gl.qcontrol(&mut self.m, 0x15, None, 2, KEYWARP.addr());
                self.m[SAVED_KEYWARP] = 1;
            }
            self.o.gl.qcontrol(&mut self.m, 0x16, Some(&[0, 0]), 0, 0);
        }
    }

    /// Auto-repeat as it was remembered [soft 0x08395c70 si_restoreRepeat].
    pub fn si_restore_repeat(&mut self) {
        if self.m[SAVED_KEYWARP] != 0 {
            let k = self.m[KEYWARP];
            self.o.gl.qcontrol(&mut self.m, 0x16, Some(&[k as i16, (k >> 16) as i16]), 0, 0);
        }
    }

    /// The value field over a cell of a transformation box: the user clicked the right part
    /// of a scaling, rotation or translation cell and types a value
    /// [soft 0x08395c9c transformationBoxHandle].
    ///
    /// The system dialog 1526 (the last one of the table if none has that identifier) is
    /// placed over the cell, its text field given the cell's current value as "%3.4f", and
    /// run until its default item or item 3 (cancel) is chosen. On cancel the old value is
    /// shown again. Otherwise the text is read: a number alone is the new value; a text
    /// with an operator after its first character whose first number equals the old value
    /// (`old+2`) applies the operator with the number after it; any other text with an
    /// operator is evaluated by the dialog code (`si_calculateTextEdit`) and the result
    /// read. The value goes to `SI_directSrtHandle` unless it equals the old one and is not
    /// relative, and the cell is redrawn with the value.
    pub fn transformation_box_handle(&mut self, group: P<Group>, control: P<Control>) -> i32 {
        const DIALOG_SIZE: u32 = 0x3c;
        let table = self.m[SI_SYSTEM_DIALOG];
        let n = self.m[SI_SYSTEM_NBDIAL] as i32;
        let mut i = 0;
        if n > 0 {
            loop {
                let found = self.m.i16(table.wrapping_add(i as u32 * DIALOG_SIZE)) == 0x5f6;
                i += 1;
                if i >= n || found {
                    break;
                }
            }
        }
        let dialog = table.wrapping_add((i as u32).wrapping_mul(DIALOG_SIZE)).wrapping_sub(DIALOG_SIZE);
        if dialog == 0 {
            return 0;
        }
        let item = self.m.u32(dialog + 0x14);
        let edit = self.m.u32(item + 0x10);
        let saved = self.m.u16(edit + 4);
        self.m.set_u16(edit + 4, 6);
        let (gid, cid) = (self.m[group].id, self.m[control].id);
        let old = self.o.controls.si_get_ctrl_values(&mut self.m, gid, cid);
        let old_f = old as f32;
        self.o.dialogs.SI_setTexed(&mut self.m, dialog, 1, &fmt_3_4f(old));
        self.o.dialogs.SI_dialogStart(&mut self.m);
        let (cx, cy) = (self.m[control].x, self.m[control].y);
        let place = |ui: &mut Ui, at: u32, v: i16| {
            ui.m.set_i16(dialog + 4 + at, v);
            let item = ui.m.u32(dialog + 0x14);
            ui.m.set_i16(item + 6 + at, v);
        };
        let v = (self.m[SI_LEFT_BOX] as i16).wrapping_add(cx);
        place(self, 0, v);
        let v = (self.m[SI_RIGHT_BOX] as i16).wrapping_add(cx).wrapping_add(1);
        place(self, 2, v);
        place(self, 4, cy.wrapping_add(4));
        let v = (self.m[SI_TOP_BOX] as i16).wrapping_add(cy).wrapping_add(4);
        place(self, 6, v);
        self.o.gl.frontbuffer(&mut self.m, 1);
        let chosen = loop {
            let e = self.o.dialogs.SI_dialogHandleEvent(&mut self.m, dialog);
            let chosen = self.m[e].second;
            if chosen == self.m.i16(dialog + 0xe) || chosen == 3 {
                break chosen;
            }
        };
        let shown;
        if chosen == 3 {
            shown = fmt_3_4f(old_f as f64);
        } else {
            let mut relative = 0u8;
            let text = self.m.cstr(self.m.u32(edit));
            let mut skip = 0;
            while skip < text.len() && (text[skip] == b' ' || text[skip] == b'\t') {
                skip += 1;
            }
            let text = &text[skip..];
            let is_op = |c: u8| matches!(c, b'+' | b'-' | b'/' | b'*');
            let lead = (!text.is_empty() && is_op(text[0])) as usize;
            let rest = &text[lead..];
            let op = [b'+', b'-', b'/', b'*'].iter().find_map(|c| rest.iter().position(|b| b == c));
            let value = match op {
                None => strtod(text),
                Some(at) => {
                    relative = rest[at];
                    let first = strtod(text);
                    if old_f as f64 == first {
                        strtod(&rest[at + 1..])
                    } else {
                        self.m.set_u16(edit + 4, saved);
                        let mut words = [0u32; 10];
                        let item = self.m.u32(dialog + 0x14);
                        for (k, w) in words.iter_mut().enumerate() {
                            *w = self.m.u32(item + 4 * k as u32);
                        }
                        self.o.dialogs.si_calculateTextEdit(&mut self.m, dialog, words);
                        relative = 0;
                        strtod(&self.m.cstr(self.m.u32(edit)))
                    }
                }
            };
            let value = value as f32;
            if !(old_f == value) || relative != 0 {
                self.o.boxes.SI_directSrtHandle(&mut self.m, gid as i32, cid as i32, value, relative);
            }
            shown = fmt_3_4f(value as f64);
        }
        self.o.gl.frontbuffer(&mut self.m, 1);
        self.o.gl.drawmode(&mut self.m, 0x20);
        self.o.gl.color(&mut self.m, 0);
        self.o.gl.clear(&mut self.m);
        self.o.gl.drawmode(&mut self.m, 0x10);
        self.o.controls.SI_controlOff(&mut self.m, P::NULL, 1);
        self.o.controls.SI_controlSetStr(&mut self.m, gid as i32, cid as i32, &shown);
        self.o.gl.frontbuffer(&mut self.m, 0);
        self.o.dialogs.SI_dialogEnd(&mut self.m);
        self.o.controls.SI_refreshAllControls(&mut self.m);
        self.m.set_u16(edit + 4, saved);
        1
    }

    /// A group (and perhaps one of its controls) was chosen, by the mouse or by its letter
    /// [soft 0x08396288 activateGroup].
    ///
    /// The choice is logged and the group selected (as the right button does it when the
    /// device is the right button or more than one thing is active on the canvas). For the
    /// Exit button (group −100) the user is asked; "Ok" gives the event QUIT. For any other
    /// group the event becomes GROUP or CONTROL with the group's identifier, the control's
    /// (−1 without one, 4 for part 1 of a cell), and the group record. Then: part 2 of a
    /// transformation cell opens the value field (the click's three entries are put back
    /// into the queue first); a control that owns a menu (kind 4, or kind 3 when active)
    /// opens it, and so does a group whose only control is of kind 3 or 4; if the menu
    /// produced no event, or after the value field, the canvas's groups are deactivated.
    #[allow(clippy::too_many_arguments)]
    pub fn activate_group(&mut self, group: P<Group>, control: P<Control>, part: i16, device: i32, value: i16, x: i16, y: i16) -> i32 {
        let gid = self.m[group].id;
        if control.is_null() {
            if self.m[group].n_controls > 0 && !self.m[group].controls.is_null() {
                let name = self.m[self.m[group].controls].name;
                self.o.journal.SI_log(&mut self.m, 1, b"[%d] GROUP %s", &[LogArg::I(gid as i32), LogArg::S(name)]);
            }
        } else {
            let (cid, name) = (self.m[control].id, self.m[control].name);
            self.o.journal.SI_log(&mut self.m, 1, b"[%d,%d] %s", &[LogArg::I(gid as i32), LogArg::I(cid as i32), LogArg::S(name)]);
        }
        let canvas = self.m[SI_ACTIVE_CANVAS];
        let a = self.m[canvas].active;
        if device == 0x65 || 1 < a[0] as i32 + a[1] as i32 + a[2] as i32 {
            self.o.controls.SI_groupSelectRightButton(&mut self.m, canvas, group, control);
        } else {
            self.o.controls.SI_groupSelect(&mut self.m, canvas, group, control);
        }
        if self.m[group].id == -100 {
            return self.exit_button();
        }
        let e = self.m[SI_ACTIVE_EVENT];
        self.m[e].kind = if control.is_null() { ev::GROUP } else { ev::CONTROL };
        let e = self.m[SI_ACTIVE_EVENT];
        self.m[e].first = self.m[group].id;
        self.m[e].second = if control.is_null() { -1 } else { self.m[control].id };
        if part == 1 {
            let e = self.m[SI_ACTIVE_EVENT];
            self.m[e].second = 4;
        }
        let e = self.m[SI_ACTIVE_EVENT];
        self.m[e].x = 0;
        self.m[e].y = 0;
        self.m[e].data = group.addr();
        let gid = self.m[group].id;
        if gid < 4 && gid > 0 {
            self.o.boxes.SI_modeBoxRefresh(&mut self.m);
        }
        let learn = self.o.menus.si_learnMode(&mut self.m);
        let e = self.m[SI_ACTIVE_EVENT];
        if learn == 0 && !control.is_null() && (self.m[group].id.wrapping_sub(1) as u16) < 3 && part == 2 {
            let dev = device as u16;
            self.o.gl.qenter(&mut self.m, dev, value);
            self.o.gl.qenter(&mut self.m, dev, x);
            self.o.gl.qenter(&mut self.m, dev, y);
            self.transformation_box_handle(group, control);
        } else if self.m[e].kind != ev::CONTROL {
            if self.m[group].state == 0 || self.m[group].n_controls != 1 {
                return 0;
            }
            let only = self.m[group].controls;
            self.m[e].kind = ev::CONTROL;
            self.m[e].second = self.m[only].id;
            if (self.m[only].kind.wrapping_sub(3) as u16) > 1 {
                return 0;
            }
            let only = self.m[group].controls;
            if self.launch_menu(device, only) != 0 {
                return 0;
            }
        } else {
            if (self.m[control].state == 0 || self.m[control].kind != 3) && self.m[control].kind != 4 {
                return 0;
            }
            if self.launch_menu(device, control) != 0 {
                return 0;
            }
        }
        let canvas = self.m[SI_ACTIVE_CANVAS];
        self.o.controls.si_groupDeactivateCanvas(&mut self.m, canvas, 1);
        0
    }

    /// The Exit button's part of [soft 0x08396288 activateGroup].
    fn exit_button(&mut self) -> i32 {
        if self.o.menus.si_learnMode(&mut self.m) != 0 {
            self.o.status.SI_statusDisplayMessage(&mut self.m, b"Cannot set a Swift Key for the Exit button. ESC to exit learn mode", 2);
            let canvas = self.m[SI_ACTIVE_CANVAS];
            self.o.controls.si_groupDeactivateCanvas(&mut self.m, canvas, 1);
            let e = self.m[SI_ACTIVE_EVENT];
            self.m[e].kind = ev::NONE;
            return 0;
        }
        let second: &[u8] = if self.o.server.SI_isDialogEditor(&mut self.m) == 0 {
            if self.o.motion.SI_sceneDirty(&mut self.m) == 0 {
                b""
            } else {
                b"WARNING : The scene has been modified..."
            }
        } else {
            b"Unsaved changes will be lost..."
        };
        let ok = if self.o.server.SI_isDialogEditor(&mut self.m) == 0 {
            self.o.dialogs.SI_systemWarning(&mut self.m, b"This will exit the Softimage system", second, b"Press [Ok] to exit.")
        } else {
            self.o.dialogs.SI_systemWarning(&mut self.m, b"This will exit the Dialog Editor", second, b"Press [Ok] to exit.")
        };
        if ok == 0 {
            let canvas = self.m[SI_ACTIVE_CANVAS];
            self.o.controls.si_groupDeactivateCanvas(&mut self.m, canvas, 1);
            let e = self.m[SI_ACTIVE_EVENT];
            self.m[e].kind = ev::NONE;
        } else {
            self.o.devices.si_deviceMouse(&mut self.m);
            let e = self.m[SI_ACTIVE_EVENT];
            self.m[e].kind = ev::QUIT;
        }
        1
    }

    /// [soft 0x083965e4 si_setMnemonicMode]
    pub fn si_set_mnemonic_mode(&mut self, mode: i16) {
        self.m[MNEMONIC_MODE] = mode;
    }

    /// A button of the tablet's puck went down [soft 0x083965f4 puckDown]: the event PUCK
    /// with the device and the tablet's position.
    pub fn puck_down(&mut self, device: i32) -> i32 {
        let e = self.m[SI_ACTIVE_EVENT];
        self.m[e].kind = ev::PUCK;
        self.m[e].first = 0;
        self.m[e].second = device as i16;
        let x = self.o.gl.getvaluator(&mut self.m, 0x10e) as i16;
        let e = self.m[SI_ACTIVE_EVENT];
        self.m[e].x = x;
        let y = self.o.gl.getvaluator(&mut self.m, 0x10f) as i16;
        let e = self.m[SI_ACTIVE_EVENT];
        self.m[e].y = y;
        1
    }

    /// A mouse button went down at (x, y) [soft 0x0839664c si_mouseDown] (the internal one;
    /// `SI_mouseDown` is the button query).
    ///
    /// **Double click**: this press is one if it comes fewer than `SI_doubleClickDelay`
    /// (30) clock ticks of `times` after the last and less than 2 pixels from it in x and
    /// y. Then the part of the screen decides (see `SI_fndCanvasPart`):
    ///
    /// - views: a swift-mouse plug-in, if one is loaded and mode 61 is valid, gets the
    ///   point and nothing else happens (not even the double-click bookkeeping); otherwise
    ///   the event gets the device and value and `SI_viewSelect` takes over;
    /// - menu columns: the group and control under the point are looked up. In swift-key
    ///   learn mode a click on a cell of a transformation box only names its action (event
    ///   LEARN with the action string for group, axis, part and button); otherwise the
    ///   group is activated;
    /// - time line: followed while the button is held, then the event TIMELINE;
    /// - title bar: the module switch; the logo opens the information box;
    /// - colour box, play box, current-frame box, status box, mode box: their handlers;
    /// - a dialog: the event DIALOG with the device, value, point and the dialog.
    pub fn si_mouse_down_(&mut self, device: i32, value: i16, x: i16, y: i16) -> i32 {
        let mut column = 0i16;
        let mut result = 1i32;
        let now = self.o.c.times(&mut self.m);
        let (lx, ly) = (self.m[LAST_X] as i32, self.m[LAST_Y] as i32);
        let (xi, yi) = (x as i32, y as i32);
        self.m[DOUBLE_CLICK] = (now.wrapping_sub(self.m[SI_TIME_CLK]) < self.m[SI_DOUBLE_CLICK_DELAY]
            && lx - 2 < xi
            && xi < lx + 2
            && ly - 2 < yi
            && yi < ly + 2) as i16;
        let canvas = self.m[SI_ACTIVE_CANVAS];
        let part = self.si_fnd_canvas_part(canvas, xi, yi);
        let e = self.m[SI_ACTIVE_EVENT];
        match part {
            1 => {
                if self.m[SI_DETECT_SWIFT_MOUSE_DSO] != 0 && self.o.keys.SI_isSupraModeValid(&mut self.m, 0x3d) != 0 {
                    self.o.keys.SI_detectSwiftMouseDSO(&mut self.m, xi, yi);
                    return 1;
                }
                let e = self.m[SI_ACTIVE_EVENT];
                self.m[e].second = device as i16;
                self.m[e].first = value;
                let canvas = self.m[SI_ACTIVE_CANVAS];
                self.si_view_select(canvas, e, x, y);
            }
            2 | 3 => {
                if part == 2 {
                    column = 1;
                }
                let (mut group, mut control, mut cell) = (P::<Group>::NULL, P::<Control>::NULL, 0i16);
                let canvas = self.m[SI_ACTIVE_CANVAS];
                self.o.controls.si_groupFindIt(&mut self.m, canvas, column, x, y, &mut group, &mut control, &mut cell);
                let learn = self.o.menus.si_learnMode(&mut self.m);
                let e = self.m[SI_ACTIVE_EVENT];
                if learn == 0
                    || group.is_null()
                    || control.is_null()
                    || (self.m[group].id.wrapping_sub(1) as u16) > 2
                    || (self.m[control].id as u16) > 2
                    || (cell as u16) > 2
                {
                    if self.m[SI_CANVAS_MASK] & 0x20 != 0 {
                        self.o.boxes.si_colorBoxHide(&mut self.m);
                    }
                    if !group.is_null() {
                        self.m[MNEMONIC_MODE] = 0;
                        result = self.activate_group(group, control, cell, device, value, x, y) as i16 as i32;
                    }
                } else {
                    // The original leaves the button index uninitialised for a device that
                    // is none of the three mouse buttons; no caller passes one. 0 here.
                    let button = match device {
                        0x66 => 1,
                        0x65 => 2,
                        _ => 0,
                    };
                    self.m[e].kind = ev::LEARN;
                    self.m[e].first = device as i16;
                    self.m[e].second = value;
                    self.m[e].x = x;
                    self.m[e].y = y;
                    let index = (self.m[group].id as i32 - 1) * 27 + self.m[control].id as i32 * 9 + cell as i32 * 3 + button;
                    self.m[e].data = self.m[DESC_SRT][index as usize];
                }
            }
            4 => {
                self.si_set_timeline(xi);
                let f = self.m[SI_FRAME_CURRENT];
                self.o.journal.SI_log(&mut self.m, 1, b"[%d] current frame %d", &[LogArg::I(4), LogArg::I(f)]);
                let e = self.m[SI_ACTIVE_EVENT];
                self.m[e].data = 0;
                self.m[e].kind = ev::TIMELINE;
            }
            5 => {
                let canvas = self.m[SI_ACTIVE_CANVAS];
                self.o.server.si_serverMonitor(&mut self.m, canvas, x, y);
                let a = self.m[SI_ACTIVE] as i32;
                self.o.journal.SI_log(&mut self.m, 1, b"[%d] module %d", &[LogArg::I(a), LogArg::I(a)]);
            }
            6 => {
                let on_bar = !(xi < self.m[SI_COLOR_BOX_L] as i32 - 2 || self.m[SI_BAR] as i32 + (self.m[SI_COLOR_BOX_L] as i32) < xi);
                if self.m[SI_BIT_PLANES] < 9 {
                    self.o.status.SI_statusDisplayMessage(&mut self.m, b"Not available on 8 bitplanes machine...", 2);
                } else if self.m[SI_CANVAS_MASK] & 0x20 == 0 {
                    if !on_bar {
                        self.o.status.SI_statusDisplayMessage(&mut self.m, b"Open it before !", 2);
                    } else {
                        self.o.journal.SI_log(&mut self.m, 1, b"Color Box Display", &[]);
                        self.o.boxes.si_colorBoxDisplay(&mut self.m);
                        self.o.status.SI_statusDisplayMessage(&mut self.m, b"Select object", 0);
                    }
                } else if !on_bar {
                    let cell = self.o.boxes.si_colorBoxFnd(&mut self.m, x, y);
                    self.o.journal.SI_log(&mut self.m, 1, b"Color Box %d\n", &[LogArg::I(cell as i32)]);
                    let cell = self.o.boxes.si_colorBoxFnd(&mut self.m, x, y);
                    self.o.boxes.si_colorBoxSelect(&mut self.m, cell, 1);
                } else {
                    self.o.journal.SI_log(&mut self.m, 1, b"Color Box Hide", &[]);
                    self.o.boxes.si_colorBoxHide(&mut self.m);
                    self.o.status.si_statusDisplayMouseMenuDefault(&mut self.m);
                }
            }
            7 => {
                self.si_play_box(device, value, x, y);
            }
            8 => {
                self.m[e].kind = ev::HANDLED;
                let dev = device as u16;
                self.o.gl.qenter(&mut self.m, dev, value);
                self.o.gl.qenter(&mut self.m, dev, x);
                self.o.gl.qenter(&mut self.m, dev, y);
                self.o.boxes.SI_handleCurrentFrame(&mut self.m);
            }
            9 => {
                self.m[e].kind = ev::DIALOG;
                self.m[e].first = device as i16;
                self.m[e].second = value;
                self.m[e].x = x;
                self.m[e].y = y;
                self.m[e].data = self.m[DIALOG_HIT];
            }
            10 => {
                self.m[e].kind = ev::HANDLED;
                self.o.status.si_statusCloseFile(&mut self.m);
                let path = self.m[SI_STATUS_PATH];
                self.o.dialogs.SI_dialogShowStatus(&mut self.m, path);
                self.o.status.SI_statusOpenFile(&mut self.m);
                self.o.status.si_statusFlagSet(&mut self.m, 0);
                self.o.status.SI_statusDisplayMessage(&mut self.m, b"", 0);
            }
            0xd => self.o.boxes.SI_modeBoxHandle(&mut self.m, xi, yi),
            0xe => {
                if self.o.server.SI_isDialogEditor(&mut self.m) == 0 {
                    self.o.boxes.SI_infoLinux(&mut self.m);
                }
            }
            _ => {}
        }
        self.m[SI_TIME_CLK] = now;
        self.m[LAST_Y] = y;
        self.m[LAST_X] = x;
        result
    }

    /// A key went up [soft 0x08396b38 keyUp]: an open colour box closes; Alt (143, 144),
    /// Control (3, 145) end the letter mode; after 30 ms the swift-key mode in force is
    /// asked to end; the escape key (7) gives the event ESCAPE.
    pub fn key_up(&mut self, device: i32) -> i32 {
        if self.m[SI_CANVAS_MASK] & 0x20 != 0 {
            self.o.boxes.si_colorBoxHide(&mut self.m);
        }
        if (device.wrapping_sub(0x8f) as u32) < 2 || device == 3 || device == 0x91 {
            self.m[MNEMONIC_MODE] = 0;
        }
        self.o.c.usleep(&mut self.m, 30000);
        let mode = self.m[SI_SUPRA_MODE] as u32;
        self.o.keys.si_deactivateSupraMode(&mut self.m, mode);
        if device == 7 {
            let e = self.m[SI_ACTIVE_EVENT];
            self.m[e].kind = ev::ESCAPE;
            self.m[e].second = 0x1f;
            let x = self.o.gl.getvaluator(&mut self.m, 0x10a) as i16;
            let e = self.m[SI_ACTIVE_EVENT];
            self.m[e].x = x;
        }
        1
    }

    /// A key went down [soft 0x08396bcc keyDown]. The escape key (7) does nothing here.
    ///
    /// Outside a replay, after 30 ms, the keys held now (`si_getKeySequence`) are matched
    /// against the swift-key modes and the direct swift keys. A direct key that matches at
    /// least as far as a mode is run. Otherwise a mode that matches is entered. With
    /// neither, Alt or Control held makes the key a letter that picks a cell of the menu
    /// columns (mode 1 with Alt, 2 with Control), which is then activated as by a click at
    /// the pointer. During a replay the next key of the swift key's sequence is looked up;
    /// when the sequence ends the event is cleared.
    pub fn key_down(&mut self, device: i32, value: i16) -> i32 {
        let mut keys = [0i32; 4];
        let (mut mode_at, mut key_at) = (-1i16, -1i16);
        if device == 7 {
            return 0;
        }
        if self.m[SWIFTMODE] < 0 {
            self.o.c.usleep(&mut self.m, 30000);
            self.si_get_key_sequence(&mut keys);
            let mode = self.o.keys.si_findCurrentMode(&mut self.m, device as u16, &keys, &mut mode_at);
            self.m[DIRECT_KEY] = self.o.keys.si_findDirectKey(&mut self.m, device as u16, &keys, &mut key_at, 0);
            if mode != self.m[SI_SUPRA_MODE] {
                let old = self.m[SI_SUPRA_MODE];
                self.o.keys.si_leaveSupraMode(&mut self.m, old);
            }
            if self.m[DIRECT_KEY] == -1 || key_at < mode_at {
                if mode == 0 {
                    self.m[SWIFTMODE] = -1;
                    self.m[MNEMONIC_MODE] = if self.o.gl.getbutton(&mut self.m, 0x8f) != 0 || self.o.gl.getbutton(&mut self.m, 0x90) != 0 {
                        1
                    } else if self.o.gl.getbutton(&mut self.m, 3) != 0 || self.o.gl.getbutton(&mut self.m, 0x91) != 0 {
                        2
                    } else {
                        0
                    };
                    if self.m[SI_CANVAS_MASK] & 0x20 != 0 {
                        self.o.boxes.si_colorBoxHide(&mut self.m);
                    }
                    if self.m[MNEMONIC_MODE] != 0 {
                        let (mut group, mut control, mut cell) = (P::<Group>::NULL, P::<Control>::NULL, 0i16);
                        let (canvas, how) = (self.m[SI_ACTIVE_CANVAS], self.m[MNEMONIC_MODE]);
                        self.o.controls.si_groupMnemonicFindIt(&mut self.m, canvas, device as u8, how, &mut group, &mut control, &mut cell);
                        if !group.is_null() {
                            let cid = if control.is_null() { -1 } else { self.m[control].id as i32 };
                            let gid = self.m[group].id as i32;
                            self.o.journal.SI_log(&mut self.m, 1, b"[%d,%d] Mnemonic", &[LogArg::I(gid), LogArg::I(cid)]);
                            let x = self.o.gl.getvaluator(&mut self.m, 0x10a) as i16;
                            let y = self.o.gl.getvaluator(&mut self.m, 0x10b) as i16;
                            return self.activate_group(group, control, cell, device, value, x, y) as i16 as i32;
                        }
                    }
                } else {
                    if self.m[SWIFTKEY_MODULE_CHANGED] == 0 {
                        self.m[SWIFTMODE] = -1;
                    }
                    self.m[SI_SUPRA_MODE] = mode;
                    self.o.keys.si_enterSupraMode(&mut self.m, mode);
                }
            } else {
                let key = self.m[DIRECT_KEY];
                self.m[SWIFTMODE] = key;
                self.o.keys.SI_handleDirectKey(&mut self.m, key);
            }
        } else {
            for (i, k) in keys.iter_mut().enumerate() {
                let key = self.m[SWIFTMODE] as i32;
                *k = self.o.keys.SI_getQuickKeySequence(&mut self.m, key, i as i32);
            }
            if self.m[SWIFTKEY_MODULE_CHANGED] == 0 {
                let from = self.m[SWIFTMODE].wrapping_add(1);
                self.m[DIRECT_KEY] = self.o.keys.si_findDirectKey(&mut self.m, device as u16, &keys, &mut key_at, from);
            } else {
                let from = self.m[SWIFTMODE];
                self.m[DIRECT_KEY] = self.o.keys.si_findDirectKey(&mut self.m, device as u16, &keys, &mut key_at, from);
                self.m[SWIFTKEY_MODULE_CHANGED] = 0;
                self.m[SWIFTMODE] = self.m[SWIFTMODE].wrapping_sub(1);
            }
            let key = self.m[DIRECT_KEY];
            if key < 0 || key == self.m[SWIFTMODE] {
                let e = self.m[SI_ACTIVE_EVENT];
                let e = &mut self.m[e];
                e.x = 0;
                e.y = 0;
                e.second = 0x67;
                e.first = 1;
                e.kind = ev::NONE;
                e.data = 0;
                e.extra = 0;
            } else {
                self.o.keys.SI_handleDirectKey(&mut self.m, key);
            }
            self.m[SWIFTMODE] = self.m[DIRECT_KEY];
        }
        1
    }

    /// A character from the keyboard (device KEYBD) [soft 0x08396ef4 keyb]: with a dialog
    /// open and the focus on a window other than the main one, the dialog of the focus
    /// window gets the key (`si_dialogHandleKeyDown` fills the dialog event; if it took the
    /// key, the dialog event's `second` becomes the hit dialog's count at +0xe, or the one
    /// at +0x10 plus 1 when that is not positive). Otherwise the event is cleared.
    pub fn keyb(&mut self, device: i32, value: i16) -> i32 {
        let focus = self.m[SI_TEMPLATE].winid;
        if self.m[SI_DIALOG_ACTIVE] == 0 || self.m[SI_SYSID] == focus as i32 {
            let e = self.m[SI_ACTIVE_EVENT];
            self.m[e].kind = ev::NONE;
        } else {
            let dial_event = self.m[SI_ACTIVE_DIAL_EVENT];
            let dialog = self.o.dialogs.si_fndDialogActive(&mut self.m, focus as i32);
            if self.o.dialogs.si_dialogHandleKeyDown(&mut self.m, dialog, device, value as i32, dial_event) != 0 {
                let hit = self.m[DIALOG_HIT];
                let n = self.m.i16(hit + off::DIALOG_N_0E);
                let dial_event = self.m[SI_ACTIVE_DIAL_EVENT];
                self.m[dial_event].second = if n < 1 { self.m.i16(hit + off::DIALOG_N_10).wrapping_add(1) } else { n };
            }
        }
        1
    }

    /// Ask the swift-key mode in force to end if its keys are no longer held
    /// [soft 0x08396f7c checkKeyboard].
    pub fn check_keyboard(&mut self) -> i32 {
        let mode = self.m[SI_SUPRA_MODE] as u32;
        self.o.keys.si_deactivateSupraMode(&mut self.m, mode) as i32
    }

    /// A click in the play box [soft 0x08396f98 SI_playBox]. `SI_playBoxButton` says which
    /// button: 0 end, 1 start, 2 loop, 3 frame, 4 backward, 5 forward, 6 keyframe.
    ///
    /// In swift-key learn mode the click only names its action (event LEARN). Otherwise:
    /// start and end put the click back into the queue and call `SI_playBoxHandle`; loop,
    /// frame and keyframe are switches (frame and keyframe exclude each other and loop);
    /// backward and forward run the playback in a mode chosen from the switches,
    /// `SI_playbackRun(mode, right button?, middle button?)`:
    ///
    /// | | plain | loop | frame | keyframe | keyframe + loop |
    /// |---|---|---|---|---|---|
    /// | forward | 0 | 1 | 2 | 6 | 8 |
    /// | backward | 3 | 4 | 5 | 7 | 9 |
    pub fn si_play_box(&mut self, device: i32, value: i16, x: i16, y: i16) -> i32 {
        let button = self.o.boxes.SI_playBoxButton(&mut self.m, x, y);
        if self.o.menus.si_learnMode(&mut self.m) != 0 {
            let e = self.m[SI_ACTIVE_EVENT];
            self.m[e].kind = ev::LEARN;
            self.m[e].first = device as i16;
            self.m[e].second = value;
            self.m[e].x = x;
            self.m[e].y = y;
            // as in si_mouseDown: not initialised by the original for other devices
            let b = match device {
                0x66 => 1,
                0x65 => 2,
                _ => 0,
            };
            let e = self.m[SI_ACTIVE_EVENT];
            self.m[e].data = match button {
                0 => self.m[DESC_PLAY_BOX][3 + b],
                1 => self.m[DESC_PLAY_BOX][b],
                2 => self.m[DESC_PLAY_BOX][18 + b],
                3 => self.m[DESC_PLAY_BOX][12 + b],
                4 => self.m[DESC_PLAY_BOX][6 + b],
                5 => self.m[DESC_PLAY_BOX][9 + b],
                6 => self.m[DESC_PLAY_BOX][15 + b],
                _ => 0,
            };
            return 1;
        }
        let hilight = |ui: &mut Ui, b: i32, on: i16| ui.o.boxes.SI_playBoxHilight(&mut ui.m, b, on);
        match button {
            0 | 1 => {
                let text: &[u8] = if button == 0 { b"Playbox end" } else { b"Playbox start" };
                self.o.journal.SI_log(&mut self.m, 1, text, &[]);
                let dev = device as u16;
                self.o.gl.qenter(&mut self.m, dev, value);
                self.o.gl.qenter(&mut self.m, dev, x);
                self.o.gl.qenter(&mut self.m, dev, y);
                self.o.boxes.SI_playBoxHandle(&mut self.m, button as i16);
            }
            2 => {
                self.o.journal.SI_log(&mut self.m, 1, b"Playbox loop", &[]);
                if self.m[SI_ACT_LOOP] == 0 {
                    hilight(self, 3, 0);
                    self.m[SI_ACT_FRAME] = 0;
                    hilight(self, 2, 1);
                    self.m[SI_ACT_LOOP] = 1;
                } else {
                    hilight(self, 2, 0);
                    self.m[SI_ACT_LOOP] = 0;
                }
            }
            3 => {
                self.o.journal.SI_log(&mut self.m, 1, b"Playbox frame", &[]);
                if self.m[SI_ACT_FRAME] == 0 {
                    hilight(self, 2, 0);
                    self.m[SI_ACT_LOOP] = 0;
                    hilight(self, 6, 0);
                    self.m[SI_ACT_KEYFRM] = 0;
                    hilight(self, 3, 1);
                    self.m[SI_ACT_FRAME] = 1;
                } else {
                    hilight(self, 3, 0);
                    self.m[SI_ACT_FRAME] = 0;
                    self.o.status.SI_statusDisplayMouse(&mut self.m, b"", b"", b"");
                }
            }
            4 | 5 => {
                let forward = button == 5;
                self.o.journal.SI_log(&mut self.m, 1, if forward { b"Playbox forward" } else { b"Playbox back" }, &[]);
                hilight(self, if forward { 4 } else { 5 }, 0);
                hilight(self, button, 1);
                let (lp, frame, key) = (self.m[SI_ACT_LOOP] != 0, self.m[SI_ACT_FRAME] != 0, self.m[SI_ACT_KEYFRM] != 0);
                let mode = if lp && !key {
                    1
                } else if frame {
                    2
                } else if !key {
                    0
                } else if !lp {
                    6
                } else {
                    8
                };
                let mode = if forward {
                    mode
                } else {
                    [3, 4, 5, 0, 0, 0, 7, 0, 9][mode as usize]
                };
                let right = (device == self.m[SI_BUTTON_RIGHT] as i32) as i16;
                let middle = (device == self.m[SI_BUTTON_MIDDLE] as i32) as i16;
                self.o.motion.SI_playbackRun(&mut self.m, mode, right, middle);
                hilight(self, button, 0);
            }
            6 => {
                self.o.journal.SI_log(&mut self.m, 1, b"Playbox keyframe", &[]);
                if self.m[SI_ACT_KEYFRM] == 0 {
                    hilight(self, 2, 0);
                    self.m[SI_ACT_LOOP] = 0;
                    hilight(self, 3, 0);
                    self.m[SI_ACT_FRAME] = 0;
                    hilight(self, 6, 1);
                    self.m[SI_ACT_KEYFRM] = 1;
                } else {
                    hilight(self, 6, 0);
                    self.m[SI_ACT_KEYFRM] = 0;
                    self.o.status.SI_statusDisplayMouse(&mut self.m, b"", b"", b"");
                }
            }
            _ => {}
        }
        let e = self.m[SI_ACTIVE_EVENT];
        self.m[e].second = -1;
        self.m[e].kind = ev::NONE;
        self.o.controls.SI_refreshAllControls(&mut self.m);
        1
    }

    /// The swift-key keys held down now, at most four, in table order; the rest −1
    /// [soft 0x083975f4 si_getKeySequence].
    pub fn si_get_key_sequence(&mut self, out: &mut [i32; 4]) {
        *out = [-1; 4];
        let mut n = 0usize;
        while self.m[SUPRA_KEYS][n] != 0 {
            n += 1;
        }
        // the table has 62 keys; the original's own limit is 128
        let mut held = vec![0i16; n];
        self.o.gl.getdev(&mut self.m, n as i32, SUPRA_KEYS.addr(), &mut held);
        let mut k = 0;
        for (i, h) in held.iter().enumerate() {
            if *h == 1 {
                out[k] = self.m[SUPRA_KEYS][i] as u16 as i32;
                k += 1;
                if k > 3 {
                    return;
                }
            }
        }
    }

    /// [soft 0x083976ac SI_keyDown]
    pub fn si_key_down(&mut self, device: i32, value: i16) -> i32 {
        self.key_down(device, value) as i16 as i32
    }

    /// [soft 0x083976c8 SI_keyUp]
    pub fn si_key_up(&mut self, device: i32) -> i32 {
        self.key_up(device) as i16 as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtod_reads_what_the_stand_in_reads() {
        assert_eq!(strtod(b"  -12.5e1x"), -125.0);
        assert_eq!(strtod(b"1.e"), 1.0);
        assert_eq!(strtod(b".5+3"), 0.5);
        assert_eq!(strtod(b"+"), 0.0);
        assert_eq!(strtod(b"abc"), 0.0);
        assert_eq!(fmt_3_4f(1.5), b"1.5000");
        assert_eq!(fmt_3_4f(-0.00004), b"-0.0000");
    }
}
