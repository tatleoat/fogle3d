// SPDX-License-Identifier: MIT
//! `graf.c` of the program (0x08399f20): what the main loop does with an event before the
//! module sees it (redraws, the window being iconified and brought back, the dope sheet's
//! clicks), and the automatic backup of the scene.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.

use crate::globals::*;
use crate::mem::P;
use crate::records::{cam, Event, Grafport};
use crate::{fault, Ui};

impl Ui {
    /// The program's window was iconified (event 0x16) [soft 0x08399f20 si_handleStow]:
    /// the colour map the program found is put back and the keyboard's auto-repeat
    /// restored. If the screen is larger than the program's own size (`getgdesc` 0 and 1
    /// against `SI_xmaxscreen`, `SI_ymaxscreen`), the pointer's position is remembered and
    /// the pointer given the whole screen.
    pub fn si_handle_stow(&mut self) {
        self.si_color_map_restore(0);
        self.si_restore_repeat();
        let width = self.o.gl.getgdesc(&mut self.m, 0).wrapping_sub(1);
        let height = self.o.gl.getgdesc(&mut self.m, 1).wrapping_sub(1);
        if width > self.m[SI_XMAXSCREEN] as i32 || height > self.m[SI_YMAXSCREEN] as i32 {
            let cx = self.m[SI_COORDX] as u16;
            self.m[UNSTOW_CUR_X] = self.o.gl.getvaluator(&mut self.m, cx);
            let cy = self.m[SI_COORDY] as u16;
            self.m[UNSTOW_CUR_Y] = self.o.gl.getvaluator(&mut self.m, cy);
            let x = self.m[UNSTOW_CUR_X] as i16;
            self.o.gl.setvaluator(&mut self.m, 0x10a, x, 0, width as i16);
            let y = self.m[UNSTOW_CUR_Y] as i16;
            self.o.gl.setvaluator(&mut self.m, 0x10b, y, 0, height as i16);
        }
    }

    /// The program's window is back (event 0x17) [soft 0x08399fd4 si_handleUnstow]:
    /// auto-repeat off, the program's colour map loaded, the server background mapped to
    /// the active module's colour (`SI_appcol`, three 16-bit words a module), and the
    /// pointer confined to the program's screen again at the remembered position.
    pub fn si_handle_unstow(&mut self) {
        let module = self.m.i16(self.m[SI_ACTIVE_CANVAS].addr()) as i32;
        self.si_disable_repeat();
        self.si_color_map_restore(1);
        let at = SI_APPCOL.addr().wrapping_add((module as u32).wrapping_mul(6));
        let (r, g, b) = (self.m.i16(at), self.m.i16(at.wrapping_add(2)), self.m.i16(at.wrapping_add(4)));
        let index = self.m[SI_SERVER_BACK] as u16;
        self.o.gl.mapcolor(&mut self.m, index, r, g, b);
        self.o.gl.gflush(&mut self.m);
        if self.m[UNSTOW_CUR_X] != -1 {
            let (x, max) = (self.m[UNSTOW_CUR_X] as i16, self.m[SI_XMAXSCREEN]);
            self.o.gl.setvaluator(&mut self.m, 0x10a, x, 0, max);
        }
        if self.m[UNSTOW_CUR_Y] != -1 {
            let (y, max) = (self.m[UNSTOW_CUR_Y] as i16, self.m[SI_YMAXSCREEN]);
            self.o.gl.setvaluator(&mut self.m, 0x10b, y, 0, max);
        }
    }

    /// What the program itself does with an event before a module gets it
    /// [soft 0x0839a078 si_eventFilter]. The answer is non-zero when the event is done
    /// with. By event type:
    ///
    /// | Type | Action | Answer |
    /// |---|---|---|
    /// | 5 (a click in a view) | in a dope sheet: `SI_dopeSheetHandle`; if that changed something, two refreshes of the window (0x27, 7) | 1; 0 for other views |
    /// | 7 (dialog) | `si_statDialHandler` | its answer |
    /// | 0xb | nothing | 1 |
    /// | 0xc | refresh everything (0x3f) | 0 |
    /// | 0xd (time line) | `refreshActiveWind` of the event's window | 1 |
    /// | 0xe | refresh of the event's window (5) | 1 |
    /// | 0xf (zoomed) | refresh of the event's window (0x3f) | 0 |
    /// | 0x16, 0x17 | `si_handleStow`, `si_handleUnstow` | 1 |
    /// | 0x1b (REDRAW) | auto-repeat off, `SI_redrawEverything(1)`, the dialogs refreshed | 1 |
    /// | 0x1c (WINQUIT) | `SI_serverClean`, `SI_serverExit(0, 0)` | 1 |
    /// | 0x1e | all views drawn, refresh 0x37, drawn again, refresh 0x17 | 1 |
    /// | 0x1f | the same for the event's window | 1 |
    /// | 0x20 | the title bar of the event's window, or of all, in the front buffer | 1 |
    /// | others | | 0 |
    ///
    /// Afterwards, with the automatic backup on, `autoSave` gets its turn.
    pub fn si_event_filter(&mut self, event: P<Event>) -> i32 {
        let mut answer = 1i32;
        let data = |ui: &Ui| P::<Grafport>::at(ui.m[event].data);
        match self.m[event].kind {
            5 => {
                let g = data(self);
                if self.camera_type(g) != cam::DOPE_SHEET {
                    answer = 0;
                } else if self.o.views.SI_dopeSheetHandle(&mut self.m, event) != 0 {
                    self.o.views.SI_refreshSystem(&mut self.m, g, 0, 0x27);
                    self.o.views.SI_refreshSystem(&mut self.m, g, 0, 7);
                }
            }
            7 => answer = self.o.dialogs.si_statDialHandler(&mut self.m, event),
            0xb => {}
            0xc => {
                self.o.views.SI_refreshSystem(&mut self.m, P::NULL, 0, 0x3f);
                answer = 0;
            }
            0xd => {
                let g = data(self);
                self.refresh_active_wind(g);
            }
            0xe => {
                let g = data(self);
                self.o.views.SI_refreshSystem(&mut self.m, g, 0, 5);
            }
            0xf => {
                let g = data(self);
                self.o.views.SI_refreshSystem(&mut self.m, g, 0, 0x3f);
                answer = 0;
            }
            0x16 => self.si_handle_stow(),
            0x17 => self.si_handle_unstow(),
            0x1b => {
                self.si_disable_repeat();
                self.o.server.SI_redrawEverything(&mut self.m, 1);
                self.o.dialogs.SI_dialogRefresh(&mut self.m);
            }
            0x1c => {
                self.o.server.SI_serverClean(&mut self.m);
                self.o.server.SI_serverExit(&mut self.m, 0, 0);
            }
            0x1e => {
                self.si_window_draw_all();
                self.o.views.SI_refreshSystem(&mut self.m, P::NULL, 0, 0x37);
                self.si_window_draw_all();
                self.o.views.SI_refreshSystem(&mut self.m, P::NULL, 0, 0x17);
            }
            0x1f => {
                let g = data(self);
                self.si_window_draw(g);
                self.o.views.SI_refreshSystem(&mut self.m, g, 0, 0x37);
                self.si_window_draw(g);
                self.o.views.SI_refreshSystem(&mut self.m, g, 0, 0x17);
            }
            0x20 => {
                let g = data(self);
                if g.is_null() {
                    self.si_window_gadjet_all(1);
                } else {
                    self.si_window_gadjet(g, -1, 1);
                }
            }
            _ => answer = 0,
        }
        if self.m[SI_BACKUP_ACTIVE] != 0 {
            self.auto_save();
        }
        answer as i16 as i32
    }

    /// Redraw after the time changed [soft 0x0839a234 refreshActiveWind]. Without a
    /// window: everything (0x17; first 0x37 during fast playback). With one, by its
    /// display mode: shaded and matte (1, 6) 0x37 then 0x17; the rotoscope modes (2, 3)
    /// the same (only 0x37 when the overlay has 4 planes or fewer) and then the same pair
    /// for every visible window before this one that is in a rotoscope mode; any other
    /// mode 0x1f.
    pub fn refresh_active_wind(&mut self, g: P<Grafport>) {
        if g.is_null() {
            if self.m[SI_FAST_PLAY_BACK_ACTIVE] != 0 {
                self.o.views.SI_refreshSystem(&mut self.m, P::NULL, 0, 0x37);
            }
            self.o.views.SI_refreshSystem(&mut self.m, P::NULL, 0, 0x17);
            return;
        }
        match self.m[g].display {
            1 | 6 => {
                self.o.views.SI_refreshSystem(&mut self.m, g, 0, 0x37);
                self.o.views.SI_refreshSystem(&mut self.m, g, 0, 0x17);
            }
            2 | 3 => {
                self.o.views.SI_refreshSystem(&mut self.m, g, 0, 0x37);
                if self.m[SI_OLBIT_PLANES] > fault!(4, 5) {
                    self.o.views.SI_refreshSystem(&mut self.m, g, 0, 0x17);
                }
                let mut other = P::NULL;
                while self.si_window_get_next_visible(&mut other) != 0 && other != g {
                    let d = self.m[other].display;
                    if d == 2 || d == 3 {
                        self.o.views.SI_refreshSystem(&mut self.m, other, 0, 0x37);
                        self.o.views.SI_refreshSystem(&mut self.m, other, 0, 0x17);
                    }
                }
            }
            _ => self.o.views.SI_refreshSystem(&mut self.m, g, 0, 0x1f),
        }
    }

    /// The automatic backup [soft 0x0839a3a8 autoSave]: when the period has passed since
    /// the last one (`time`), the scene is saved into the BACKUP database under the busy
    /// cursor: the database is found (none: a message, the backup is switched off),
    /// locked for writing (not possible: a message), renamed to "BACKUP_TMP" (not
    /// possible: a message), created anew and, if that worked, the scene saved in it and
    /// the renamed one deleted; then "Scene backuped" and the time is noted.
    pub fn auto_save(&mut self) {
        let due = self.m[SI_AUTOSAVEPERIODE].wrapping_add(self.m[SI_AUTOSAVETIME]);
        if due > self.o.c.time(&mut self.m, 0) {
            return;
        }
        self.si_cursor_change(5);
        let database = self.o.server.SI_dbDirGetBackupDatabase(&mut self.m, SI_DBDIR.addr());
        if database == 0 {
            self.o.status.SI_statusDisplayMessage(&mut self.m, b"AutoSave Deactivated - no BACKUP database", 2);
            self.m[SI_BACKUP_ACTIVE] = 0;
            self.si_cursor_change(0);
            return;
        }
        if self.o.server.SI_dbSetDbWriteLock(&mut self.m, database, 1) == 0 {
            self.o.status.SI_statusDisplayMessage(&mut self.m, b"AutoSave not done: BACKUP database is locked", 2);
            return;
        }
        if self.o.server.SI_dbRenameExtDatabase(&mut self.m, database, b"BACKUP_TMP") != 0 {
            self.o.status.SI_statusDisplayMessage(&mut self.m, b"AutoSave : cannot rename BACKUP database..", 2);
            return;
        }
        let old = self.o.mem.strdup(&mut self.m, SI_DB_WORK_FNAME.addr());
        if self.o.server.SI_dbCreateExtDatabase(&mut self.m, database) != 0 {
            self.o.server.SI_dbSetDbWriteLock(&mut self.m, database, 1);
            let dictionary = self.current_dictionary_of_scene();
            self.o.server.SI_dbSaveScene(&mut self.m, database, dictionary, 1, 0, 0, 0);
            self.o.server.SI_dbSetDbWriteLock(&mut self.m, database, 0);
            self.o.server.SI_dbDeleteExtDatabase(&mut self.m, old);
        }
        if old != 0 {
            self.o.mem.free(&mut self.m, old);
        }
        self.o.status.SI_statusDisplayMessage(&mut self.m, b"Scene backuped", 2);
        self.o.status.SI_statusDisplayMessage(&mut self.m, b" ", 0);
        self.m[SI_AUTOSAVETIME] = self.o.c.time(&mut self.m, 0).wrapping_sub(1);
        self.si_cursor_change(0);
    }
}
