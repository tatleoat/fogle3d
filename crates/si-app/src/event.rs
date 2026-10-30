// SPDX-License-Identifier: MIT
//! The program's event reader: the part of the user interface's `event.c` (0x083939a0) that
//! the main loop stands on. The rest of that file (what a key or a mouse button *does*)
//! is not ported and is reached through [`EventHandlers`].
//!
//! An event is read in two steps. The IRIS GL queue gives (device, value); `SI_fndEvent`
//! sorts the device into a class; the class's handler turns it into an `SI_Event` whose
//! kind the module's loop switches on. Besides the queue there is a small stack of
//! *posted* events (`si_pushQE`): the program's way to tell itself "module chosen",
//! "initialise", "refresh", "quit". Posted events are served before the queue is looked
//! at.

use crate::env::World;
use crate::state::{App, Event, EventRef};

/// Kinds of events (`SI_EventType`) the frame itself produces or tests. Names are ours,
/// from what the receiving code does.
pub const EV_NONE: i32 = 0;
/// A module's canvas is shown for the first time: the module initialises.
pub const EV_INIT: i32 = 1;
/// Another module was chosen: the module's loop returns to the main loop.
pub const EV_SWITCH: i32 = 2;
/// A module's canvas is shown again: the module refreshes.
pub const EV_REFRESH: i32 = 3;
/// The program ends: the module's loop returns.
pub const EV_QUIT: i32 = 4;
/// The window manager iconified (0x16) or restored (0x17) a window.
pub const EV_STOW: i32 = 0x16;
pub const EV_UNSTOW: i32 = 0x17;
/// The window must be redrawn.
pub const EV_REDRAW: i32 = 0x1b;
/// The window manager asked to close the window.
pub const EV_CLOSE: i32 = 0x1c;

/// IRIS GL devices the reader names (finding 0016 has the layer's side).
pub const DEV_KEYBD: i32 = 0x201;
pub const DEV_REDRAW: i32 = 0x210;
pub const DEV_INPUTCHANGE: i32 = 0x216;
pub const DEV_WINFREEZE: i32 = 0x21b;
pub const DEV_WINTHAW: i32 = 0x21c;
pub const DEV_541: i32 = 0x21d;
pub const DEV_WINQUIT: i32 = 0x21e;

/// The classes [`App::fnd_event`] sorts a queue entry into.
pub const CLASS_NONE: i32 = 0;
pub const CLASS_KEY_DOWN: i32 = 1;
pub const CLASS_KEY_UP: i32 = 2;
pub const CLASS_MOUSE_DOWN: i32 = 3;
pub const CLASS_MOUSE_UP: i32 = 4;
pub const CLASS_PUCK_DOWN: i32 = 5;
pub const CLASS_PUCK_UP: i32 = 6;
pub const CLASS_REDRAW: i32 = 7;
pub const CLASS_FREEZE: i32 = 8;
pub const CLASS_THAW: i32 = 9;
pub const CLASS_541: i32 = 10;
pub const CLASS_QUIT: i32 = 0xb;
pub const CLASS_CHARACTER: i32 = 0xc;

/// The handlers of `event.c` that are not ported: each fills in the event record.
#[allow(non_snake_case)]
pub trait EventHandlers {
    fn keyDown(&mut self, event: &mut Event, device: i32, value: i16);
    fn keyUp(&mut self, event: &mut Event, device: i32);
    fn keyb(&mut self, event: &mut Event, device: i32, value: i16);
    fn si_mouseDown(&mut self, event: &mut Event, device: i32, value: i16, x: i16, y: i16);
    fn puckDown(&mut self, event: &mut Event, device: i32);
    /// `si_eventFilter(event)`: handles the events that concern no module (redraws,
    /// stowing, the close request, status dialogs); not 0 when it used the event up.
    fn si_eventFilter(&mut self, event: &mut Event) -> i16;
}

#[allow(non_snake_case)]
impl EventHandlers for crate::env::Recorder {
    fn keyDown(&mut self, event: &mut Event, device: i32, value: i16) {
        self.call("keyDown", &[device.into(), value.into()]);
        event.kind = self.next("keyDown.kind") as i32;
    }
    fn keyUp(&mut self, event: &mut Event, device: i32) {
        self.call("keyUp", &[device.into()]);
        event.kind = self.next("keyUp.kind") as i32;
    }
    fn keyb(&mut self, event: &mut Event, device: i32, value: i16) {
        self.call("keyb", &[device.into(), value.into()]);
        event.kind = self.next("keyb.kind") as i32;
    }
    fn si_mouseDown(&mut self, event: &mut Event, device: i32, value: i16, x: i16, y: i16) {
        self.call("si_mouseDown", &[device.into(), value.into(), x.into(), y.into()]);
        event.kind = self.next("si_mouseDown.kind") as i32;
    }
    fn puckDown(&mut self, event: &mut Event, device: i32) {
        self.call("puckDown", &[device.into()]);
        event.kind = self.next("puckDown.kind") as i32;
    }
    fn si_eventFilter(&mut self, _event: &mut Event) -> i16 {
        self.call("si_eventFilter", &[crate::env::Arg::Ptr]) as i16
    }
}

impl<W: World + EventHandlers> App<W> {
    /// The event record a reference names.
    pub fn event(&self, r: EventRef) -> &Event {
        match r {
            EventRef::Active => &self.g.events.active,
            EventRef::Posted => &self.g.events.taken,
        }
    }

    /// [soft 0x08393bc4 newEvent]: a zeroed event record (0x18 bytes from `SI_calloc`).
    pub fn new_event() -> Event {
        Event::default()
    }

    /// [soft 0x08393fe4 qdeviceSupraMode]: queues the device of every supra key (the list
    /// ends at the first 0).
    pub fn qdevice_supra_mode(&mut self) {
        let keys = self.g.events.supra_keys.clone();
        for k in keys {
            if k == 0 {
                break;
            }
            self.w.qdevice(k as i32);
        }
    }

    /// [soft 0x08395c10 si_disableRepeat]: remembers the keyboard's repeat settings the
    /// first time, then switches key repeat off. (`SI_isDialogEditor()` is 0 in this
    /// program: [`App::is_dialog_editor`].)
    pub fn disable_repeat(&mut self) {
        if self.is_dialog_editor() == 0 {
            if self.g.events.saved_keywarp == 0 {
                self.g.events.keywarp = self.w.qcontrol_get();
                self.g.events.saved_keywarp = 1;
            }
            self.w.qcontrol_set(0);
        }
    }

    /// [soft 0x08393bdc SI_initEvent]: says which devices the program wants in its queue:
    /// device 1, the character device KEYBD, the supra keys, WINQUIT, WINFREEZE, WINTHAW,
    /// INPUTCHANGE and the three mouse buttons, each tied to the pointer's two valuators
    /// (so that a button event is followed by x and y). REDRAW is not asked for here.
    /// Then the two event records are made and the supra keys initialised.
    pub fn init_event(&mut self) {
        self.g.events.device_use = 0;
        self.disable_repeat();
        self.w.qdevice(1);
        self.w.qdevice(DEV_KEYBD);
        self.qdevice_supra_mode();
        self.w.qdevice(DEV_WINQUIT);
        self.w.qdevice(DEV_WINFREEZE);
        self.w.qdevice(DEV_WINTHAW);
        self.w.qdevice(DEV_INPUTCHANGE);
        let e = &self.g.events;
        let (x, y) = (e.coord_x as i32, e.coord_y as i32);
        for button in [e.button_left, e.button_middle, e.button_right] {
            self.w.qdevice(button as i32);
            self.w.tie(button as i32, x, y);
        }
        self.g.events.active = Self::new_event();
        // SI_activeDialEvent: the dialogs' own record, not kept here
        self.w.SI_initSupraKeys();
    }

    /// [soft 0x0839476c si_pushQE]: posts an event in front of the ones already posted.
    /// At most nine are counted; a tenth pushes the oldest out of the count. The two last
    /// words of the new first record keep what the old first record had there.
    pub fn push_qe(&mut self, kind: i32, a: i16, b: i16, c: i16, d: i16, data: u32) {
        let e = &mut self.g.events;
        let mut i = e.posted_count as i32 - 1;
        while i >= 0 {
            // a count above nine cannot be reached through this routine
            e.posted[i as usize + 1] = e.posted[i as usize];
            i -= 1;
        }
        let first = &mut e.posted[0];
        first.kind = kind;
        first.a = a;
        first.b = b;
        first.c = c;
        first.d = d;
        first.data = data;
        e.is_posted = 1;
        e.posted_count = (e.posted_count as i32 + 1).min(9) as i16;
    }

    /// [soft 0x083946f0 popQE]: takes the first posted event: copies it to `dEvt`, moves
    /// the others down, counts one less. (Called with nothing posted the count becomes −1
    /// and the "something is posted" flag stays up.)
    pub fn pop_qe(&mut self) -> EventRef {
        let e = &mut self.g.events;
        e.taken = e.posted[0];
        let mut i = 1;
        while i < e.posted_count as i32 {
            e.posted[i as usize - 1] = e.posted[i as usize];
            i += 1;
        }
        e.posted_count = e.posted_count.wrapping_sub(1);
        e.is_posted = (e.posted_count != 0) as i16;
        EventRef::Posted
    }

    /// [soft 0x08394828 SI_fndEvent]: the class of a queue entry.
    ///
    /// Keys are the devices 3, 5..12, 14..59, 61..70, 73..76, 80..83, 143..157, 167..169:
    /// class 1 for a value other than 0 (down), 2 for 0 (up). Device 4 is always "down".
    /// 101..103 are the mouse buttons (3 down, 4 up), 105..108 a puck's buttons (5, 6).
    /// KEYBD is 0xc, REDRAW 7, WINFREEZE 8, WINTHAW 9, device 541 10, WINQUIT 0xb.
    /// INPUTCHANGE is handled on the spot: the last of a run of them counts, and if its
    /// value is the main window or an open dialog's window, that becomes the current
    /// window (the main window is also put in normal draw mode); its class is 0.
    /// Everything else is class 0: ignored.
    pub fn fnd_event(&mut self, device: i32, value: i16) -> i32 {
        let key = |value: i16| if value == 0 { CLASS_KEY_UP } else { CLASS_KEY_DOWN };
        if device < 0x68 {
            if device > 100 {
                return if value == 0 { CLASS_MOUSE_UP } else { CLASS_MOUSE_DOWN };
            }
            if device < 0x3c {
                if device > 0xd {
                    return key(value);
                }
                if device == 4 {
                    return CLASS_KEY_DOWN;
                }
                if device < 5 {
                    if device == 3 {
                        return key(value);
                    }
                } else if device < 0xd {
                    return key(value);
                }
            } else if device < 0x4d {
                if device > 0x48 || (device < 0x47 && device > 0x3c) {
                    return key(value);
                }
            } else if device < 0x54 && device > 0x4f {
                return key(value);
            }
            return CLASS_NONE;
        }
        if device == DEV_KEYBD {
            return CLASS_CHARACTER;
        }
        if device < 0x202 {
            if device < 0x9e {
                if device > 0x8e {
                    return key(value);
                }
                if device < 0x6d && device > 0x68 {
                    return if value == 0 { CLASS_PUCK_UP } else { CLASS_PUCK_DOWN };
                }
            } else if device < 0xaa && device > 0xa6 {
                return key(value);
            }
            return CLASS_NONE;
        }
        if device == DEV_WINFREEZE {
            return CLASS_FREEZE;
        }
        if device < 0x21c {
            if device == DEV_REDRAW {
                return CLASS_REDRAW;
            }
            if device == DEV_INPUTCHANGE {
                let mut value = value;
                while self.w.qtest() == DEV_INPUTCHANGE {
                    value = self.w.qread().1;
                }
                if value != 0 && (value as i32 == self.g.sysid || self.w.si_dialIsOpenWinId(value as i32) != 0) {
                    self.w.mem_template_set_window(value);
                    self.w.winset(value as i32);
                    if value as i32 == self.g.sysid {
                        self.w.drawmode(0x10);
                    }
                }
            }
            return CLASS_NONE;
        }
        if device == DEV_541 {
            return CLASS_541;
        }
        if device < DEV_541 {
            return CLASS_THAW;
        }
        if device == DEV_WINQUIT {
            return CLASS_QUIT;
        }
        CLASS_NONE
    }

    /// [soft 0x0839412c si_getNextEvent]: waits for the next entry of the IRIS GL queue
    /// and makes the active event of it.
    ///
    /// With `quick` not 0, a quick-key mode pending (`swiftmode`) and the active module in
    /// its plain mode, no entry is read: the mode's key sequence is replayed as a key
    /// press instead. Otherwise the event's kind is cleared, with a tablet the tablet is
    /// polled until the queue has something, one entry is read (`qread` waits), and its
    /// class decides: keys and characters go to their handlers (and to the log), a mouse
    /// button reads the two tied valuator entries (x, y) and goes to `si_mouseDown` when
    /// pressed, REDRAW becomes event 0x1b (after counting how many 3D windows need two
    /// redraws), WINFREEZE 0x16, WINTHAW 0x17, WINQUIT 0x1c. A release of a mouse button
    /// and classes 0 and 10 leave the kind 0: an event that does nothing.
    pub fn get_next_event(&mut self, quick: i16) -> EventRef {
        if self.g.events.supra_mode != 0 {
            self.w.checkKeyboard();
        }
        let plain = quick == 0
            || self.g.events.swiftmode == -1
            || (self.g.active == 0 && self.w.mem_mdl_mod() != 0)
            || (self.g.active == 1 && self.w.SI_scnGetMode() != 0)
            || (self.g.active == 2 && self.w.SI_actorGetMode() != 0)
            || (self.g.active == 3 && self.w.SI_mtrGetMode() != 0)
            || (self.g.active == 4 && self.w.SI_toolsGetMode() != 0);
        if !plain {
            let sequence = self.w.SI_getQuickKeySequence(self.g.events.swiftmode as i32, 0);
            let mut ev = self.g.events.active;
            self.w.keyDown(&mut ev, sequence, 0);
            self.g.events.active = ev;
            return EventRef::Active;
        }
        self.g.events.active.kind = EV_NONE;
        self.g.events.swiftmode = -1;
        if self.g.tablet_active != 0 {
            while self.w.qtest() == 0 {
                self.w.SI_tabletGetData();
            }
        }
        let (device, value) = self.w.qread();
        self.g.events.active.device = device;
        let class = self.fnd_event(device, value);
        let mut ev = self.g.events.active;
        match class {
            CLASS_KEY_DOWN => {
                self.g.events.redraw_rotoscopy = 1;
                self.g.events.double_click = 0;
                self.key_log(device, value);
                self.w.keyDown(&mut ev, device, value);
            }
            CLASS_KEY_UP => {
                self.g.events.redraw_rotoscopy = 1;
                self.g.events.double_click = 0;
                self.key_log(device, value);
                self.w.keyUp(&mut ev, device);
            }
            CLASS_MOUSE_DOWN | CLASS_MOUSE_UP => {
                self.g.events.redraw_rotoscopy = 1;
                let x = self.w.qread().1;
                let y = self.w.qread().1;
                self.mouse_log(device, value, x, y);
                if class == CLASS_MOUSE_DOWN {
                    self.w.si_mouseDown(&mut ev, device, value, x, y);
                }
            }
            CLASS_PUCK_DOWN => {
                self.g.events.double_click = 0;
                self.w.puckDown(&mut ev, device);
            }
            CLASS_PUCK_UP => self.g.events.double_click = 0,
            CLASS_REDRAW => {
                self.g.events.double_click = 0;
                self.g.events.redraws_3d = self.w.mem_template_3d_redraws();
                ev.kind = EV_REDRAW;
            }
            CLASS_FREEZE => {
                self.g.events.double_click = 0;
                ev.kind = EV_STOW;
            }
            CLASS_THAW => {
                self.g.events.double_click = 0;
                ev.kind = EV_UNSTOW;
            }
            CLASS_QUIT => {
                self.g.events.double_click = 0;
                ev.kind = EV_CLOSE;
            }
            CLASS_CHARACTER => {
                self.g.events.double_click = 0;
                self.key_log(device, value);
                self.w.keyb(&mut ev, device, value);
            }
            _ => {}
        }
        self.g.events.active = ev;
        EventRef::Active
    }

    /// [soft 0x0839406c SI_getMainEvent]: the next event for a module's loop.
    ///
    /// First the busy cursor is taken down. Then, outside supra mode, **everything
    /// waiting in the queue is thrown away** up to the first REDRAW, INPUTCHANGE,
    /// WINFREEZE, WINTHAW or device 541: keys and clicks made while the program was busy
    /// are lost, and so is a WINQUIT. A posted event is served if there is one; otherwise
    /// the queue is read. An event the filter uses up comes back with kind 0 (at that
    /// point the original also calls the licence heartbeat: licence check, not ported).
    pub fn get_main_event(&mut self, quick: i16) -> EventRef {
        self.w.SI_stopActivity();
        if self.g.events.supra_mode == 0 {
            loop {
                let d = self.w.qtest();
                if d == 0 || d == DEV_INPUTCHANGE || d == DEV_REDRAW || (DEV_WINFREEZE..=DEV_541).contains(&d) {
                    break;
                }
                self.w.qread();
            }
        }
        if self.g.events.is_posted != 0 {
            return self.pop_qe();
        }
        let r = self.get_next_event(quick);
        if self.is_dialog_editor() == 0 {
            let mut ev = self.g.events.active;
            if self.w.si_eventFilter(&mut ev) != 0 {
                // licence check, not ported
                ev.kind = EV_NONE;
            }
            self.g.events.active = ev;
        }
        r
    }

    /// [soft 0x08394058 SI_getEvent]: `SI_getMainEvent(0)`.
    pub fn get_event(&mut self) -> EventRef {
        self.get_main_event(0)
    }

    /// [soft 0x08394450 si_setActiveCanvas]: a click on the module names: looks under the
    /// stored position of the module's name (which posts the module switch if it is
    /// there) and refreshes the mode box. A module number outside 0..4 only refreshes.
    pub fn set_active_canvas(&mut self, module: i32) {
        if (0..5).contains(&module) {
            let x = self.g.module_x[module as usize];
            self.server_monitor(self.g.active_canvas, x, self.g.server_y);
        }
        self.w.SI_modeBoxRefresh();
    }
}
