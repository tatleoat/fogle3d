// SPDX-License-Identifier: MIT
//! `server.c` (0x083bd000): the "server", the part of the program that owns the five
//! module canvases and switches between them, and the way out of the program.
//!
//! Ported because the main loop is made of it: activation, the click on a module's name,
//! the clean-up and the exit. `SI_serverDraw` (the drawing of the module names) and
//! `IsAcademic` are not read.

use crate::env::{Exit, Handle, Run, World};
use crate::event::{EventHandlers, EV_INIT, EV_QUIT, EV_REFRESH, EV_SWITCH};
use crate::state::{App, EventRef};

/// The last argument of each event the server posts is a text, always the empty string;
/// what is stored in the event is that string's address in the original's data. The port
/// stores the same numbers so that the posted events compare word for word.
pub const EMPTY_TEXT_SWITCH: u32 = 0x086c_72e2;
pub const EMPTY_TEXT_INIT: u32 = 0x086c_72e6;
pub const EMPTY_TEXT_REFRESH: u32 = 0x086c_72e7;
pub const EMPTY_TEXT_QUIT: u32 = 0x086c_7380;

/// A module's event loop, run by the main loop and by the clean-up.
///
/// `client` stands for `SI_modelClient`, `SI_motionClient`, `SI_actorClient`,
/// `SI_matterClient`, `SI_toolsClient` (module 0 to 4). The original's loops read events
/// with `SI_getMainEvent(1)` ([`App::get_main_event`]) and handle them until an event of
/// kind 2 (another module was chosen) or 4 (quit) arrives; they return a short (always 0
/// in the model client) and leave the event in the caller's pointer.
pub trait Clients<W: World + EventHandlers> {
    fn client(&mut self, app: &mut App<W>, module: i16) -> (i16, EventRef);
}

impl<W: World + EventHandlers> App<W> {
    /// [soft 0x083bde7c SI_isDialogEditor]: 0. (The Dialog Editor is a sibling program
    /// built from the same sources, where this returns something else.)
    pub fn is_dialog_editor(&self) -> i16 {
        0
    }

    /// [soft 0x083bd0e0 SI_clientActivate]: shows the canvas of the network entry `index`.
    /// The mode box is initialised the first time and refreshed; the canvas is drawn,
    /// the buffers swapped (or, with `redraw` not 0, the whole system refreshed) and the
    /// canvas drawn again; then an event is posted for the module: 1 (initialise) the
    /// first time its canvas is shown, 3 (refresh) afterwards.
    pub fn client_activate(&mut self, index: i16, redraw: i16) {
        self.g.active_canvas = self.g.network[index as usize].canvas;
        if self.is_dialog_editor() == 0 {
            if self.g.first_time != 0 {
                self.w.si_modeBoxInit();
                self.g.first_time = 0;
            }
            self.w.SI_modeBoxRefresh();
        }
        self.g.button_back_hilite = [self.g.server_back; 2];
        self.w.SI_canvasDraw(self.g.active_canvas, redraw);
        if self.is_dialog_editor() == 0 && redraw != 0 {
            self.w.SI_refreshSystem(0, 0, 0x25);
        } else {
            self.w.swapbuffers();
        }
        self.w.SI_canvasDraw(self.g.active_canvas, redraw);
        let entry = &mut self.g.network[index as usize];
        if entry.started == 0 {
            entry.started = 1;
            self.push_qe(EV_INIT, 0, 0, 0, 0, EMPTY_TEXT_INIT);
        } else {
            self.push_qe(EV_REFRESH, 0, 0, 0, 0, EMPTY_TEXT_REFRESH);
        }
    }

    /// [soft 0x083bd000 si_serverMonitor]: a click at `x` on the row of module names. The
    /// names are laid out from the right edge of the canvas leftwards, Tools first, each
    /// taking its text width and 10 pixels. The first enabled module whose name spans `x`
    /// is chosen: event 2 is posted and `SI_active` becomes its index. `y` is not used.
    /// (The original starts with the licence heartbeat: licence check, not ported.)
    ///
    pub fn server_monitor(&mut self, canvas: Handle, x: i16, _y: i16) {
        let mut i: i32 = if self.is_dialog_editor() == 0 { 4 } else { 0 };
        // licence check, not ported
        let mut hit = false;
        let mut right = self.w.mem_canvas_right_edge(canvas) as i32;
        loop {
            let name = self.w.mem_canvas_name(self.g.network[i as usize].canvas);
            let left = right - 10 - self.w.strwidth(&name);
            if self.g.network[i as usize].disabled == 0 {
                hit = left <= x as i32 && x as i32 <= right;
            }
            i -= 1;
            if i < 0 || hit {
                break;
            }
            right = left;
        }
        if hit {
            self.push_qe(EV_SWITCH, 0, 0, 0, 0, EMPTY_TEXT_SWITCH);
            self.g.active = (i + 1) as i16;
        }
    }

    /// [soft 0x083bda4c SI_serverClean]: tells every *other* module that was started to
    /// finish: for each, event 4 is posted and the module's client run (it reads the
    /// event and returns).
    pub fn server_clean<C: Clients<W>>(&mut self, clients: &mut C) {
        if self.is_dialog_editor() == 0 {
            for i in 0..5i16 {
                if i != self.g.active && self.g.network[i as usize].started != 0 {
                    self.push_qe(EV_QUIT, 0, 0, 0, 0, EMPTY_TEXT_QUIT);
                    let module = self.g.network[i as usize].module;
                    if (0..5).contains(&module) {
                        clients.client(self, module);
                    }
                }
            }
        } else {
            self.push_qe(EV_QUIT, 0, 0, 0, 0, EMPTY_TEXT_QUIT + 1);
        }
    }

    /// [soft 0x083bdb30 SI_softCleanup]: leaves the screen and the devices as they were
    /// found and ends the program with status 0. Custom effects are told (reason 4, or 5
    /// after a crash), the status file closed, the colour map and the pointer's speed put
    /// back, the keyboard reset, dialogs closed; every floating window that is open is
    /// cleared in the pop-up planes and closed; the main window's pop-up planes (and its
    /// overlay planes when there are more than 8 bit planes) are cleared, the window
    /// painted with the background colour and closed.
    pub fn soft_cleanup(&mut self, crashed: i16) -> Run<()> {
        if self.is_dialog_editor() == 0 {
            self.w.SI_cusExit(if crashed != 0 { 5 } else { 4 });
        }
        self.w.si_statusCloseFile();
        self.w.SI_colorMapRestore(0);
        self.w.SI_mouseWarpSet(self.g.oldmouse[0], self.g.oldmouse[1]);
        self.w.si_resetKeyboard();
        self.w.si_dialogCloseId(0);
        if self.is_dialog_editor() == 0 {
            while let Some((port, used, window)) = self.w.window_get_next() {
                if port != 0 && used != 0 && window != -1 {
                    self.w.winset(window);
                    self.w.drawmode(0x20);
                    self.w.color(0);
                    self.w.clear();
                    self.w.winclose(window);
                }
            }
        }
        self.w.drawmode(0x20);
        self.w.color(0);
        self.w.clear();
        if self.g.bit_planes > 8 {
            self.w.drawmode(0x40);
            self.w.color(0);
            self.w.clear();
        }
        self.w.drawmode(0x10);
        self.w.writemask(0xfff);
        self.w.frontbuffer(1);
        self.w.color(self.g.background);
        self.w.clear();
        self.w.winclose(self.g.sysid);
        self.w.gexit();
        Err(Exit(0))
    }

    /// [soft 0x083bdcb8 SI_serverExit]: the way out. `signal` 0 is the ordinary end:
    /// clean up, exit. Otherwise this is the handler of a fault signal: the batch
    /// renderer cleans up and ends (unless it is itself the emergency save at work);
    /// the interactive program runs the emergency save if it is on, then cleans up.
    /// The second argument is not used.
    pub fn server_exit(&mut self, signal: i32, _unused: i32) -> Run<()> {
        if signal == 0 {
            self.emergency_cleanup(1);
            self.soft_cleanup(0)?;
        } else {
            if self.g.standalone_renderer != 0 && self.g.safety_net == 0 {
                self.soft_cleanup(1)?;
                return Err(Exit(1));
            }
            if self.emergency_save_enabled() as i16 != 0 && self.emergency_save_func() {
                self.g.safety_net = 1;
                self.emergency_save()?;
            }
            self.soft_cleanup(1)?;
        }
        Ok(())
    }

    // adapter: not in place
    /// `SI_callRender (0x083bdd38)`: the image port ([`crate::image::batch::call_render`])
    /// on a scratch image holding the struct form's renderer number; the copy `SI_ren`
    /// and the two words it writes are kept in the struct form.
    pub fn call_render(&mut self) -> i32 {
        use crate::image::{batch, g};
        let mut m = crate::image::adapter::scratch();
        m.set_i16(g::SI_DEFAULT_CONTROL.0 + batch::ctl::RENDERER, self.g.default_control.renderer);
        m[g::SI_CUS_UPDATE_STATE] = self.g.cus_update_state;
        let r = batch::call_render(&mut m, &mut crate::image::adapter::AppOutside(&mut self.w));
        self.g.ren = self.g.default_control.clone();
        self.g.default_control.word178 = m.i16(g::SI_DEFAULT_CONTROL.0 + batch::ctl::WORD178);
        self.g.default_control.word17a = m.i16(g::SI_DEFAULT_CONTROL.0 + batch::ctl::WORD17A);
        self.g.cus_update_state = m[g::SI_CUS_UPDATE_STATE];
        r
    }
}

/// The names of the five module clients, by module number.
pub const CLIENT_NAMES: [&str; 5] = ["SI_modelClient", "SI_motionClient", "SI_actorClient", "SI_matterClient", "SI_toolsClient"];

/// Module loops for the recorder: each behaves as the original's loops do towards the
/// frame, and is steered by the script.
///
/// A scripted client writes its call down, then reads events with
/// [`App::get_main_event`]`(1)` until one of kind 2 or 4 arrives, and returns the script's
/// answer for its name. After each other event it asks the script what the module's own
/// handlers would have done: `client.post` not 0 posts an event of that kind (2 with
/// `client.active` as the module chosen, as a click on a module's name does; 4 as the
/// quit command does). When the script's `client.events` budget of events is used up the
/// client posts the quit event itself, so that every run ends.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScriptedClients {
    /// Events read so far by all clients.
    pub read: u32,
}

impl Clients<crate::env::Recorder> for ScriptedClients {
    fn client(&mut self, app: &mut App<crate::env::Recorder>, module: i16) -> (i16, EventRef) {
        let name = CLIENT_NAMES[module as usize];
        app.w.note(name, &[crate::env::Arg::Ptr]);
        let result = app.w.next(name) as i16;
        loop {
            let r = app.get_main_event(1);
            let kind = app.event(r).kind;
            app.w.note("client.event", &[kind.into()]);
            if kind == EV_SWITCH || kind == EV_QUIT {
                return (result, r);
            }
            self.read += 1;
            let budget = app.w.next("client.events") as u32;
            if self.read >= budget {
                app.push_qe(EV_QUIT, 0, 0, 0, 0, 0);
                continue;
            }
            let post = app.w.next("client.post") as i32;
            if post == EV_SWITCH {
                app.push_qe(EV_SWITCH, 0, 0, 0, 0, 0);
                app.g.active = app.w.next("client.active") as i16;
            } else if post != 0 {
                app.push_qe(post, 0, 0, 0, 0, 0);
            }
        }
    }
}
