// SPDX-License-Identifier: MIT
//! `window.c`: the routines that change the layout of the four views as a whole.

use crate::globals::*;
use crate::mem::P;
use crate::records::{cam, Gadget, Grafport};
use crate::{fault, Ui};

impl Ui {
    /// The window's view-type entry to return to, after a view-type change that did not
    /// set it: the fixed entry of its place in the layout when it stands at 6 (lower left
    /// 0, upper left 1, lower right 2, upper right 3; other identifiers keep 6), else 6.
    fn previous_view_default(&mut self, g: P<Grafport>) {
        if self.m[g].view_item == 6 {
            match self.m[g].id {
                1001 => self.m[g].view_item = 1,
                1000 => self.m[g].view_item = 0,
                1002 => self.m[g].view_item = 2,
                1003 => self.m[g].view_item = 3,
                _ => {}
            }
        } else {
            self.m[g].view_item = 6;
        }
    }

    /// Bring every visible window back to its plain state before the layout is replaced
    /// [soft 0x083d445c SI_windowResetAll] (not in the stand-alone renderer): un-zoomed,
    /// un-expanded, shown, in wire frame; then the triangle buffers are cleaned up and the
    /// remembered display modes forgotten.
    pub fn si_window_reset_all(&mut self) {
        if self.m[SI_STANDALONE_RENDERER] != 0 {
            return;
        }
        self.si_reset_toggle_flag();
        let mut g = P::NULL;
        while self.si_window_get_next_visible(&mut g) != 0 {
            if self.m[g].zoomed != 0 {
                self.si_un_zoom_window(g);
            }
            match self.m[g].expand {
                1 => self.window_un_expand_horiz(g),
                2 => self.window_un_expand_vert(g),
                _ => {}
            }
            self.m[g].visible = 1;
            self.o.views.SI_dispModeToggle(&mut self.m, g, 0);
            self.m[g].visible = 1;
        }
        self.o.views.SI_cleanUpTriangle(&mut self.m, 0);
        self.si_reset_toggle_flag();
    }

    /// Put the layout into effect as the window records describe it (after a scene was
    /// loaded, say) [soft 0x083d4518 SI_windowRestoreAll]; not in the stand-alone renderer.
    ///
    /// 1. The schematic view's CLEAN_ALL gadget is laid out for its state, and the windows
    ///    covered by a zoomed or expanded one are hidden (`setVisibleFields`).
    /// 2. Every hidden window's display mode goes into its "+0x68" word (1 for wire frame)
    ///    and the window to wire frame. Whether the display mode equalled that word is
    ///    remembered **for the last window of the array only** and used for all below.
    /// 3. Function-curve views get the scene's list of objects.
    /// 4. Each visible window in turn: a zoomed one takes the whole area (the others are
    ///    hidden); an expanded one is expanded; with the remembered flag set and a display
    ///    mode other than wire frame the display is toggled to the mode of the "+0x68"
    ///    word; then the window is switched to its own view type again (`changeWindowAll`)
    ///    and the entry to return to and the "+0x68" word are put right.
    /// 5. With the flag set, the hidden windows' entries to return to are put right too.
    /// 6. Everything is drawn into both buffers, the active canvas's groups are
    ///    deactivated, and the first module whose word at +0xc of its entry in the network
    ///    is zero (the current one if it qualifies) becomes the active one.
    ///
    /// Licence: the original keeps a static flag that its first call sets to 1 and that
    /// nothing clears; were it 0, function-curve and dope-sheet windows would be switched
    /// away with the message "Licence does not allow window type" and their menu entries
    /// greyed. This port takes the full product's path only (AGENTS.md).
    pub fn si_window_restore_all(&mut self) {
        if self.m[SI_STANDALONE_RENDERER] != 0 {
            return;
        }
        if self.m[FPASS_328] != 0 {
            self.m[MOTION_LICENSE_329] = 1;
            self.m[FPASS_328] = 0;
        }
        let on = (self.m[DAT_087FB9AC] == 0) as i16;
        self.si_enable_schem_clean_all(on);
        self.set_visible_fields();
        let mut same = false;
        let mut g = P::NULL;
        while self.si_window_get_next(&mut g) != 0 {
            let display = self.m[g].display;
            same = display == self.m[g].d_068;
            if self.m[g].visible == 0 {
                self.m[g].d_068 = display;
                self.m[g].display = 0;
                if display == 0 {
                    self.m[g].d_068 = 1;
                }
            }
        }
        if self.m[SI_TEMPLATE].n_views > 0 {
            // the scene's list as an index: the byte distance divided by 36
            let index = (self.m[SI_SCNGL].wrapping_sub(self.m[SI_GRAF_LISTS]) as i32).wrapping_mul(0x38e3_8e39) >> 2;
            let mut i = 0;
            loop {
                let w = self.m[SI_TEMPLATE].views.add(i);
                if self.camera_type(w) == cam::FCURVE {
                    self.m[w].d_03c = index;
                }
                i += 1;
                if i >= self.m[SI_TEMPLATE].n_views as i32 {
                    break;
                }
            }
        }
        let mut g = P::NULL;
        while self.si_window_get_next_visible(&mut g) != 0 {
            if self.m[g].zoomed != 0 {
                self.window_small_to_big(g);
                if self.m[SI_TEMPLATE].n_views > 0 {
                    let mut i = 0;
                    loop {
                        let w = self.m[SI_TEMPLATE].views.add(i);
                        if w != g {
                            self.m[w].visible = 0;
                        }
                        i += 1;
                        if i >= self.m[SI_TEMPLATE].n_views as i32 {
                            break;
                        }
                    }
                }
                self.si_get_window_viewport(g);
            }
            match self.m[g].expand {
                1 => self.window_expand_horiz(g),
                2 => self.window_expand_vert(g),
                _ => {}
            }
            // (the full product's path; see the licence note above)
            if self.m[g].display != 0 && same {
                let wanted = self.m[g].d_068;
                self.m[g].display = 0;
                let entry = self.o.views.SI_dispModeConvert(&mut self.m, g, wanted);
                if self.o.views.SI_dispModeToggle(&mut self.m, g, entry) as i16 != 0 {
                    self.m[g].display = wanted;
                }
            }
            let item = self.m[g].view_item;
            self.update_current_window(g);
            let kind = self.camera_type(g) as i32;
            let index = self.o.views.SI_camIdConvert(&mut self.m, kind);
            self.change_window_all(g, index);
            if self.m[g].view_item == item || same {
                self.previous_view_default(g);
            }
            if self.m[g].d_068 == self.m[g].display {
                self.m[g].d_068 = if self.m[g].d_068 == 1 { 0 } else { fault!(1, 2) };
            }
        }
        if same {
            let mut g = P::NULL;
            while self.si_window_get_next(&mut g) != 0 {
                if self.m[g].visible == 0 {
                    self.previous_view_default(g);
                }
            }
        }
        self.si_window_draw_all();
        self.o.gl.swapbuffers(&mut self.m);
        self.si_window_draw_all();
        let canvas = self.m[SI_ACTIVE_CANVAS];
        self.o.controls.si_groupDeactivateCanvas(&mut self.m, canvas, 1);
        let network = self.m[SI_NETWORK];
        let entry = |module: i32| network.wrapping_add((module as u32).wrapping_shl(4));
        let active = self.m[SI_ACTIVE] as i32;
        if self.m.i16(entry(active).wrapping_add(0xc)) == 0 {
            self.m[SI_ACTIVE_CANVAS] = P::at(self.m.u32(entry(active).wrapping_add(8)));
        } else if self.m.i16(network.wrapping_add(0xc)) == 0 {
            self.m[SI_ACTIVE_CANVAS] = P::at(self.m.u32(network.wrapping_add(8)));
            self.m[SI_ACTIVE] = 0;
        } else {
            let mut module = 0;
            loop {
                module += 1;
                if module > 4 {
                    break;
                }
                let network = self.m[SI_NETWORK];
                let at = network.wrapping_add((module as u32) << 4);
                if self.m.i16(at.wrapping_add(0xc)) == 0 {
                    self.m[SI_ACTIVE_CANVAS] = P::at(self.m.u32(at.wrapping_add(8)));
                    self.m[SI_ACTIVE] = module as i16;
                    break;
                }
            }
        }
        let active = self.m[SI_ACTIVE] as i32;
        self.si_set_active_canvas(active);
    }

    /// Lay out the schematic view's CLEAN_ALL and CLEAN gadgets for whether CLEAN_ALL is
    /// shown, and redraw the title bars of the visible schematic views
    /// [soft 0x083db1c0 SI_enableSchemCleanAll]. The two gadgets' offsets are remembered
    /// on the first call; with CLEAN_ALL shown (`on`) they have their own places, without
    /// it they are exchanged, so that CLEAN stands where CLEAN_ALL was.
    pub fn si_enable_schem_clean_all(&mut self, on: i16) {
        let clean_all = GADGET_INFO.cast::<Gadget>().add(21);
        let clean = GADGET_INFO.cast::<Gadget>().add(22);
        if self.m[FIRST_TIME_657] != 0 {
            self.m[SAVE_CLEAN_ALL_LOFFSET_659] = self.m[clean_all].offset;
            self.m[SAVE_CLEAN_LOFFSET_658] = self.m[clean].offset;
            self.m[FIRST_TIME_657] = 0;
        }
        if on == 0 {
            self.m[clean_all].offset = self.m[SAVE_CLEAN_LOFFSET_658];
            self.m[clean].offset = self.m[SAVE_CLEAN_ALL_LOFFSET_659];
        } else {
            self.m[clean_all].offset = self.m[SAVE_CLEAN_ALL_LOFFSET_659];
            self.m[clean].offset = self.m[SAVE_CLEAN_LOFFSET_658];
        }
        let mut g = P::NULL;
        while self.si_window_get_next_visible(&mut g) != 0 {
            if self.camera_type(g) == fault!(cam::SCHEMATIC, cam::DOPE_SHEET) {
                self.si_window_gadjet(g, -1, 1);
            }
        }
    }

    /// The common part of "make a function-curve view (a dope sheet) appear": look for a
    /// window of the type, un-zooming others on the way; a hidden one is expanded
    /// sideways and counts as not found. Returns (the last window looked at, the window
    /// found or null).
    fn find_view_of_type(&mut self, kind: i16) -> (P<Grafport>, P<Grafport>) {
        let mut g = P::NULL;
        let mut found = P::NULL;
        let mut done = false;
        loop {
            if self.si_window_get_next(&mut g) == 0 {
                break;
            }
            if self.m[g].zoomed != 0 {
                if self.camera_type(g) == kind {
                    done = true;
                    found = g;
                } else {
                    self.si_un_zoom_window(g);
                }
            }
            if self.camera_type(g) == kind {
                found = g;
                if self.m[g].visible == 0 {
                    self.si_window_size_set(g, 1);
                    found = P::NULL;
                }
                done = true;
            }
            if done {
                break;
            }
        }
        (g, found)
    }

    /// Turn the lower left window into the view type of entry `item` of its menu, expanded
    /// sideways: expansions in the way are undone first (upward ones of the upper right,
    /// upper left and lower left windows, any of the lower right one); the window's
    /// settings are written back, its display goes to wire frame, it is enabled and
    /// switched. Returns the window.
    fn take_lower_left(&mut self, item: i32) -> P<Grafport> {
        let w = |ui: &Ui, i: i32| ui.m[SI_TEMPLATE].views.add(i);
        for i in [3, 1, 0] {
            if self.m[w(self, i)].expand == 2 {
                let g = w(self, i);
                self.si_window_size_set(g, 0);
            }
        }
        let e = self.m[w(self, 2)].expand;
        if e == 2 || self.m[w(self, 2)].expand == 1 {
            let g = w(self, 2);
            self.si_window_size_set(g, 0);
        }
        let g = w(self, 0);
        self.update_current_window(g);
        self.o.views.SI_dispModeToggle(&mut self.m, g, 0);
        self.window_toggle_off_rgb_wind(g);
        self.m[g].enabled = 1;
        self.change_window_all(g, item);
        if self.m[g].expand == 0 {
            self.si_window_size_set(g, 1);
        }
        g
    }

    /// After a window was turned into another view type: its title bar in the front
    /// buffer and everything redrawn twice.
    fn redraw_after_take(&mut self, g: P<Grafport>) {
        self.si_window_gadjet(g, -1, 1);
        self.si_window_draw_all();
        self.o.views.SI_refreshSystem(&mut self.m, P::NULL, 0, 0x37);
        self.si_window_draw_all();
        self.o.views.SI_refreshSystem(&mut self.m, P::NULL, 0, 0x17);
    }

    /// Make sure a function-curve view is on the screen
    /// [soft 0x083d8c70 SI_windowSetFcurveActive]. If none is found visible (see `find_view_of_type`), the
    /// lower left window becomes one (entry 5 of its menu), expanded sideways, and the
    /// curves are refreshed. Unless `quiet`, the view is zoomed to the scene's curves
    /// (`SI_autozoom`). A newly made view is then redrawn.
    pub fn si_window_set_fcurve_active(&mut self, quiet: i16) {
        let (mut g, found) = self.find_view_of_type(cam::FCURVE);
        if found.is_null() {
            g = self.take_lower_left(5);
            self.o.views.SI_refreshFcurve(&mut self.m, 0);
        }
        if quiet == 0 {
            let list = self.m[SI_SCNGL];
            self.o.views.SI_autozoom(&mut self.m, list, g, 0, 1, 1);
        }
        if found.is_null() {
            self.redraw_after_take(g);
        }
    }

    /// Make sure a dope sheet is on the screen
    /// [soft 0x083d8e70 SI_windowSetDopeSheetActive]: as for the function-curve view, with entry 7 of the
    /// menu and a refresh of the dope sheet. Unless `quiet`, the dope sheet's flags are
    /// set.
    pub fn si_window_set_dope_sheet_active(&mut self, quiet: i16) {
        let (mut g, found) = self.find_view_of_type(cam::DOPE_SHEET);
        if found.is_null() {
            g = self.take_lower_left(7);
            self.o.views.SI_refreshDopeSheet(&mut self.m, 0x28);
        }
        if quiet == 0 {
            self.m[DAT_087FB9C8] = 1;
            self.o.views.SI_dopeFlagsSet(&mut self.m, 1, 1, 1, 1);
        }
        if found.is_null() {
            self.redraw_after_take(g);
        }
    }

    /// Make the upper right window visible and a perspective view
    /// [soft 0x083d906c SI_windowSetPerspectiveActive]. A hidden one is shown after what
    /// covered it is undone (the upper left window's sideways expansion, the lower right
    /// one's upward expansion, a zoom of any of the other three); one of another view type
    /// is enabled and switched to entry 3 of its menu. Its title bar is drawn in the front
    /// buffer and everything redrawn into both buffers.
    pub fn si_window_set_perspective_active(&mut self) {
        let v = self.m[SI_TEMPLATE].views;
        let g = v.add(3);
        if self.m[g].visible == 0 {
            self.m[g].visible = 1;
            let w = |ui: &Ui, i: i32| ui.m[SI_TEMPLATE].views.add(i);
            if self.m[w(self, 1)].expand == 1 {
                let o = w(self, 1);
                self.si_window_size_set(o, 0);
            }
            if self.m[w(self, 2)].expand == 2 {
                let o = w(self, 2);
                self.si_window_size_set(o, 0);
            }
            for i in [1, 2, 0] {
                if self.m[w(self, i)].zoomed == 1 {
                    let o = w(self, i);
                    self.si_un_zoom_window(o);
                }
            }
        }
        if self.camera_type(g) != cam::PERSPECTIVE {
            self.handle_changing_displays(g, 3);
            self.m[g].enabled = 1;
            self.change_window_all(g, 3);
        }
        self.si_window_gadjet(g, -1, 1);
        self.si_window_draw_all();
        self.o.gl.swapbuffers(&mut self.m);
        self.si_window_draw_all();
    }

    /// Switch between the modelling views and the set with a function-curve view, and back
    /// [soft 0x083db2a8 SI_changeSetOfViews].
    ///
    /// - The lower left window is a visible function-curve view and no other active
    ///   window is one: it goes back to what it was: its pictures off (with the neighbour
    ///   its expansion covers), un-expanded (or un-zoomed), and switched to the view type
    ///   remembered in `frontCamId`.
    /// - Another active window is a function-curve view: that window goes back to the
    ///   view type it showed before (+0x5c).
    /// - Otherwise the present state is remembered (which window is zoomed, or the lower
    ///   left window's view type and expansion) and a function-curve view is made
    ///   (`SI_windowSetFcurveActive(1)`); nothing more.
    ///
    /// After going back: the window's settings are written to its camera table; the
    /// window that was zoomed before is zoomed again, or everything is redrawn; the
    /// pictures come back; two refreshes with a redraw between.
    pub fn si_change_set_of_views(&mut self) {
        let v = self.m[SI_TEMPLATE].views;
        let w0 = v;
        let mut g = P::NULL;
        let mut found = 0i16;
        if self.camera_type(w0) != cam::FCURVE || self.m[w0].visible == 0 {
            found = self.is_fcurve_view(&mut g) as i16;
        }
        let kind = self.camera_type(w0);
        let back;
        if kind == cam::FCURVE && self.m[w0].visible != 0 && found == 0 {
            match self.m[w0].expand {
                1 => {
                    self.window_toggle_off_rgb_wind(w0);
                    self.window_toggle_off_rgb_wind(v.add(2));
                }
                2 => {
                    self.window_toggle_off_rgb_wind(w0);
                    self.window_toggle_off_rgb_wind(v.add(1));
                }
                _ => self.window_toggle_off_rgb_wind(w0),
            }
            if self.m[w0].zoomed == 0 {
                self.si_window_size_set(w0, 0);
            } else {
                self.si_un_zoom_window_win(w0);
            }
            let item = self.m[FRONT_CAM_ID_667] as i32;
            self.handle_changing_displays(w0, item);
            let item = self.m[FRONT_CAM_ID_667] as i32;
            self.change_window_all(w0, item);
            back = w0;
        } else if found != 0 {
            let item = self.m[g].view_item as i32;
            self.handle_changing_displays(g, item);
            let item = self.m[g].view_item as i32;
            self.change_window_all(g, item);
            back = g;
        } else {
            if self.m[w0].zoomed != 0 {
                self.m[ZOOM_WIN_ID_666] = 0;
            } else if self.m[v.add(1)].zoomed != 0 {
                self.m[ZOOM_WIN_ID_666] = 1;
            } else if self.m[v.add(2)].zoomed != 0 {
                self.m[ZOOM_WIN_ID_666] = 2;
            } else if self.m[v.add(3)].zoomed != 0 {
                self.m[ZOOM_WIN_ID_666] = 3;
            } else {
                if kind == cam::FCURVE && self.m[w0].visible == 0 {
                    self.m[FRONT_CAM_ID_667] = self.m[w0].view_item;
                } else {
                    let kind = self.camera_type(w0) as i32;
                    self.m[FRONT_CAM_ID_667] = self.o.views.SI_camIdConvert(&mut self.m, kind) as i16;
                }
                self.m[FRONT_SIZE_668] = self.m[w0].expand;
                self.m[ZOOM_WIN_ID_666] = -1;
            }
            self.si_window_set_fcurve_active(1);
            return;
        }
        self.update_current_window(back);
        if self.m[ZOOM_WIN_ID_666] == -1 {
            self.si_window_draw_all();
            self.restore_rgb();
        } else {
            self.restore_rgb();
            let zoomed = P::<Grafport>::at(v.addr().wrapping_add((self.m[ZOOM_WIN_ID_666] as i32 as u32).wrapping_mul(Grafport::SIZE)));
            self.zoom_window(zoomed);
            let zoomed = P::<Grafport>::at(v.addr().wrapping_add((self.m[ZOOM_WIN_ID_666] as i32 as u32).wrapping_mul(Grafport::SIZE)));
            self.si_window_draw(zoomed);
            self.m[ZOOM_WIN_ID_666] = -1;
        }
        self.o.views.SI_refreshSystem(&mut self.m, P::NULL, 0, 0x37);
        self.si_window_draw_all();
        self.o.views.SI_refreshSystem(&mut self.m, P::NULL, 0, 0x17);
    }
}
