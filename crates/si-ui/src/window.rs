// SPDX-License-Identifier: MIT
//! The windows of the views (`window.c` of the program, 0x083d1d90; not the IRIS GL layer's
//! file of that name).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! The program has one IRIS GL window for the whole screen (`SI_sysid`). The "windows" of
//! this file are rectangles inside it: the screen template (`SI_template`, a canvas) points
//! to an array of view windows (`__SI_Grafport`), four in the standard layout read from
//! `rsrc/Windows.rsrc`:
//!
//! | Index | Identifier | Place | Starts as | Letter in the log |
//! |---|---|---|---|---|
//! | 0 | 1000 | lower left | Front | C |
//! | 1 | 1001 | upper left | Top | A |
//! | 2 | 1002 | lower right | Right | D |
//! | 3 | 1003 | upper right | Perspective | B |
//!
//! A view window is a frame, a title bar of `SI_titleHeight` pixels at its top with
//! gadgets, and the drawing area. A click in the views finds its window by rectangle
//! (`SI_viewSelect`), then the part of the window (`fndViewPart`), and calls that part's
//! handler from the table `gadgetInfo` (`si_viewSelectHandle`).

mod callbacks;
mod layout;
mod read;
mod title;
mod zoom;
pub use callbacks::HANDLERS;

use crate::globals::*;
use crate::mem::P;
use crate::records::{cam, ev, off, Callback, Canvas, Event, Gadget, Grafport};
use crate::rec::InOut;
use crate::sys::LogArg;
use crate::Ui;
use si_x87::{Round, F80};

impl Ui {
    fn n_views(&self) -> i32 {
        self.m[SI_TEMPLATE].n_views as i32
    }
    fn views(&self) -> P<Grafport> {
        self.m[SI_TEMPLATE].views
    }
    pub(crate) fn camera_type(&self, g: P<Grafport>) -> i16 {
        self.m.i16(self.m[g].camera.wrapping_add(off::CAMERA_TYPE))
    }
    /// The call through `SI_sboxfi`: a filled box in screen coordinates.
    fn sboxfi(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        // Reassigned only by the hardware check at start-up, which is not ported; any
        // other value would be a routine with sboxfi's arguments.
        self.o.gl.sboxfi(&mut self.m, x1, y1, x2, y2);
    }

    /// Clear a view's whole inside to the viewport colour in the front buffer and redraw
    /// its frame and title [soft 0x083d1d90 si_roto_viewport_clr] (used for the rotoscope
    /// display modes, whose picture does not fill the window).
    pub fn si_roto_viewport_clr(&mut self, g: P<Grafport>) {
        let mask = self.o.gl.getwritemask(&mut self.m);
        self.o.gl.frontbuffer(&mut self.m, 1);
        let whole = self.m[SI_WHOLE_SYSTEM_WM];
        self.o.gl.writemask(&mut self.m, whole as u16);
        let c = self.m[SI_WINDOW_VIEWPORT];
        self.o.gl.color(&mut self.m, c as u16);
        let w = self.m[g];
        let title = self.m[SI_TITLE_HEIGHT] as i32;
        self.sboxfi(w.left as i32 + 1, w.bottom as i32 + 1, w.right as i32 - 1, w.top as i32 - title);
        self.o.gl.writemask(&mut self.m, mask as u16);
        self.si_window_draw(g);
        self.o.gl.frontbuffer(&mut self.m, 0);
    }

    /// Place a view window: its rectangle (left, right, bottom, top) and from it the
    /// drawing area [soft 0x083d2da0 windowSetGraf].
    ///
    /// The numbers of the layout file are for the standard screen (`SI_screenRes` 0). On
    /// the small screen (1) every x is multiplied by 0.8 and every y by 0.75, truncated. On
    /// the larger ones (2 to 6) the views grow by (dx, dy) = (160, 88), (320, 5), (320,
    /// 28), (320, 88), (160, 0) per window, which moves the edges by window identifier and
    /// expansion state (see the finding for the table). The drawing area is then the
    /// rectangle inset by one pixel, less the title bar at the top.
    pub fn window_set_graf(&mut self, g: P<Grafport>, left: i16, right: i16, bottom: i16, top: i16) {
        let (mut left, mut right, mut bottom, mut top) = (left, right, bottom, top);
        let res = self.m[SI_SCREEN_RES];
        if res == 1 {
            let scale = |v: i16, by: f64| (F80::from_i32(v as i32) * by).to_i32(Round::Zero) as i16;
            left = scale(left, 0.8);
            bottom = scale(bottom, 0.75);
            right = scale(right, 0.8);
            top = scale(top, 0.75);
        } else if (2..=6).contains(&res) {
            let (dx, dy): (i16, i16) = match res {
                2 => (0xa0, 0x58),
                3 => (0x140, 5),
                4 => (0x140, 0x1c),
                5 => (0x140, 0x58),
                _ => (0xa0, 0),
            };
            let e = self.m[g].expand;
            let add = |a: i16, b: i16| a.wrapping_add(b);
            let twice = |a: i16| a.wrapping_mul(2);
            match self.m[g].id {
                1000 => match e {
                    0 => {
                        right = add(right, dx);
                        top = add(top, dy);
                    }
                    1 => {
                        right = add(right, twice(dx));
                        top = add(top, dy);
                    }
                    2 => {
                        right = add(right, dx);
                        top = add(top, twice(dy));
                    }
                    _ => {}
                },
                1001 => match e {
                    0 => {
                        right = add(right, dx);
                        bottom = add(bottom, dy);
                        top = add(top, twice(dy));
                    }
                    1 => {
                        right = add(right, twice(dx));
                        bottom = add(bottom, dy);
                        top = add(top, twice(dy));
                    }
                    2 => {
                        right = add(right, dx);
                        top = add(top, twice(dy));
                    }
                    _ => {}
                },
                1002 => match e {
                    0 => {
                        left = add(left, dx);
                        right = add(right, twice(dx));
                        top = add(top, dy);
                    }
                    1 => {
                        right = add(right, twice(dx));
                        top = add(top, dy);
                    }
                    2 => {
                        right = add(right, twice(dx));
                        left = add(left, dx);
                        top = add(top, twice(dy));
                    }
                    _ => {}
                },
                1003 => match e {
                    0 => {
                        left = add(left, dx);
                        right = add(right, twice(dx));
                        bottom = add(bottom, dy);
                        top = add(top, twice(dy));
                    }
                    1 => {
                        right = add(right, twice(dx));
                        bottom = add(bottom, dy);
                        top = add(top, twice(dy));
                    }
                    2 => {
                        right = add(right, twice(dx));
                        left = add(left, dx);
                        top = add(top, twice(dy));
                    }
                    _ => {}
                },
                _ => {}
            }
            // The original adds dx to the stored right edge here for a dope-sheet window,
            // and overwrites the field two instructions later: no effect but the reads.
            if self.m[g].camera != 0 {
                let _ = self.camera_type(g);
            }
        }
        let title = self.m[SI_TITLE_HEIGHT];
        let w = &mut self.m[g];
        w.left = left;
        w.bottom = bottom;
        w.right = right;
        w.top = top;
        w.view_left = left.wrapping_add(1);
        w.view_right = right.wrapping_sub(1);
        w.view_bottom = bottom.wrapping_add(1);
        w.view_top = top.wrapping_sub(title);
        w.view2 = [w.view_left, w.view_right, w.view_bottom, w.view_top];
    }

    /// Draw a view window's frame, title bar and empty drawing area
    /// [soft 0x083d3038 SI_windowDraw]: in the main IRIS GL window, with the whole-system write mask; the
    /// inside in the window-background colour with a line around the drawing area when the
    /// view has a margin (the camera's word at +0xc) or shows a ruler; the drawing area in
    /// the viewport colour; the frame in the text colour, or the highlight colour for the
    /// selected window; then the title bar's gadgets. In the Dialog Editor `deWindowDraw`
    /// draws instead.
    pub fn si_window_draw(&mut self, g: P<Grafport>) {
        let current = self.o.gl.winget(&mut self.m);
        if self.o.server.SI_isDialogEditor(&mut self.m) != 0 {
            self.de_window_draw(g);
            return;
        }
        let sysid = self.m[SI_SYSID];
        if current != sysid {
            self.o.gl.winset(&mut self.m, sysid);
        }
        let mask = self.o.gl.getwritemask(&mut self.m);
        let whole = self.m[SI_WHOLE_SYSTEM_WM];
        self.o.gl.writemask(&mut self.m, whole as u16);
        let w = self.m[g];
        let title = self.m[SI_TITLE_HEIGHT] as i32;
        if self.m.i16(w.camera.wrapping_add(off::CAMERA_0C)) != 0
            || self.m.i16(w.ruler_v.wrapping_add(off::RULER_ACTIVE)) != 0
            || self.m.i16(w.ruler_h.wrapping_add(off::RULER_ACTIVE)) != 0
        {
            let c = self.m[SI_WINDOW_BACK];
            self.o.gl.color(&mut self.m, c as u16);
            self.sboxfi(w.left as i32 + 1, w.bottom as i32 + 1, w.right as i32 - 1, w.top as i32 - title);
            let c = self.m[SI_WINDOW_TEXT];
            self.o.gl.color(&mut self.m, c as u16);
            let v = self.m[g];
            self.o.gl.sboxi(&mut self.m, v.view_left as i32 - 1, v.view_bottom as i32 - 1, v.view_right as i32 + 1, v.view_top as i32 + 1);
        }
        let c = self.m[SI_WINDOW_VIEWPORT];
        self.o.gl.color(&mut self.m, c as u16);
        let v = self.m[g];
        self.sboxfi(v.view_left as i32, v.view_bottom as i32, v.view_right as i32, v.view_top as i32);
        self.o.gl.writemask(&mut self.m, mask as u16);
        let c = if self.m[g].hilite != 0 { self.m[SI_WINDOW_FRAME_HILITE] } else { self.m[SI_WINDOW_TEXT] };
        self.o.gl.color(&mut self.m, c as u16);
        self.o.gl.sboxi(&mut self.m, w.left as i32, w.bottom as i32, w.right as i32, w.top as i32);
        self.si_window_gadjet(g, -1, 0);
        if current != self.m[SI_SYSID] {
            self.o.gl.winset(&mut self.m, current);
        }
    }

    /// The small square on the common corner of the four views, the handle for dragging
    /// their shape [soft 0x083d3220 winCusLayDispArea]: 21 pixels square around (right, top)
    /// of the first view + 3, dark outline, grey inside, in the overlay planes
    /// (`drawmode(0x20)`).
    pub fn win_cus_lay_disp_area(&mut self) {
        let current = self.o.gl.winget(&mut self.m);
        let sysid = self.m[SI_SYSID];
        if current != sysid {
            self.o.gl.winset(&mut self.m, sysid);
        }
        let mode = self.o.gl.getdrawmode(&mut self.m);
        self.o.gl.drawmode(&mut self.m, 0x20);
        let first = self.m[self.views()];
        let (x, y) = (first.right as i32, first.top as i32);
        let c = self.m[SI_DIALOG_DARK];
        self.o.gl.color(&mut self.m, c as u16);
        self.o.gl.sboxi(&mut self.m, x - 7, y - 7, x + 0xd, y + 0xd);
        let c = self.m[SI_DIALOG_GRAY];
        self.o.gl.color(&mut self.m, c as u16);
        self.sboxfi(x - 6, y - 6, x + 0xc, y + 0xc);
        self.o.gl.drawmode(&mut self.m, mode);
        if current != self.m[SI_SYSID] {
            self.o.gl.winset(&mut self.m, current);
        }
    }

    /// Announce the layout mode (swift-key mode 57) [soft 0x083d32e0 si_winCusLayDispMsg]:
    /// with all four views visible, the prompts and the handle; otherwise an error message.
    pub fn si_win_cus_lay_disp_msg(&mut self) {
        let mut n = 0;
        let mut g = P::NULL;
        while self.si_window_get_next_visible(&mut g) != 0 {
            n += 1;
        }
        if n == 4 {
            self.o.status.SI_statusDisplayMessage(&mut self.m, b"Drag the mouse to change the 4 views' shape", 0);
            self.o.status.SI_statusDisplayMouse(&mut self.m, b"Resize views", b"Restore layout", b"");
            self.win_cus_lay_disp_area();
        } else {
            self.o.status.SI_statusDisplayMessage(&mut self.m, b"Cannot change the window layout. 4 views must be visible", 2);
        }
    }

    /// Leave the layout mode: redraw everything [soft 0x083d335c si_winCusLayClear].
    pub fn si_win_cus_lay_clear(&mut self) {
        let current = self.o.gl.winget(&mut self.m);
        let sysid = self.m[SI_SYSID];
        if current != sysid {
            self.o.gl.winset(&mut self.m, sysid);
        }
        self.o.views.SI_refreshSystem(&mut self.m, P::NULL, 0, 0x37);
        self.si_window_draw_all();
        self.o.views.SI_refreshAll(&mut self.m, 1);
    }

    /// The view window under a point: the first visible one whose rectangle holds it.
    fn window_at(&self, x: i16, y: i16) -> Option<P<Grafport>> {
        let n = self.n_views();
        let mut i = 0;
        let mut hit = false;
        if n > 0 {
            loop {
                let w = self.m[self.views().add(i)];
                hit = w.left <= x && x <= w.right && w.bottom <= y && y <= w.top && w.visible != 0;
                i += 1;
                if !(i < n && !hit) {
                    break;
                }
            }
        }
        hit.then(|| self.views().add(i - 1))
    }

    /// A mouse button went down in the area of the views [soft 0x083d33a0 SI_viewSelect].
    ///
    /// In the layout mode (swift-key mode 57, not while learning swift keys) the click
    /// belongs to the layout handle. Otherwise the window under the point is found, then
    /// the part of it; a part that is not the drawing area is logged ("View A: [9]
    /// IN_TITLE"); and the part's handler is called. In learn mode the event becomes 0x21
    /// with the part and the window instead (except for the stored-camera boxes).
    pub fn si_view_select(&mut self, _canvas: P<Canvas>, event: P<Event>, x: i16, y: i16) {
        if self.m[SI_SUPRA_MODE] != 0 && self.m[SI_SUPRA_MODE] == 0x39 && self.o.menus.si_learnMode(&mut self.m) == 0 {
            self.win_cus_lay_handle(event, x, y);
            return;
        }
        let Some(g) = self.window_at(x, y) else { return };
        let part = self.fnd_view_part(g, x as i32, y as i32);
        if part == -1 {
            self.m[event].kind = ev::NONE;
            return;
        }
        if part != 0 {
            let label = self.get_gadget_label(part);
            let letter = match self.m[g].id {
                1000 => b'C',
                1002 => b'D',
                1001 => b'A',
                _ => b'B',
            };
            self.o.journal.SI_log(&mut self.m, 1, b"View %c: [%d] %s", &[LogArg::I(letter as i32), LogArg::I(part), LogArg::S(label)]);
        }
        if self.o.menus.si_learnMode(&mut self.m) == 0 {
            self.si_view_select_handle(event, x, y, part, g);
        } else if part != 4 {
            let e = &mut self.m[event];
            e.kind = 0x21;
            e.first = part as i16;
            e.data = g.addr();
            e.x = x;
            e.y = y;
        }
    }

    /// Clicks in the layout mode [soft 0x083d3540 winCusLayHandle]. Nothing happens unless
    /// all views are visible.
    ///
    /// - The left button on the handle (within −7..+13 of the first view's right and top
    ///   edges): the inner edges of the four views are saved the first time; the views'
    ///   pictures are switched off; the pointer is confined to the area where the centre may
    ///   go (5 pixels inside the views' area, 6 plus the wider vertical ruler where one is
    ///   shown; between a title bar's height + 6 above the bottom and + 3 below the top) and
    ///   the drag is followed; then the pictures come back and the pointer is freed. Event
    ///   0x1e. The left button elsewhere clears the event.
    /// - The middle or right button: the saved edges are put back (if any were saved).
    ///   Event 0x1e.
    ///
    /// Afterwards every view that shows a spreadsheet is told its new size.
    pub fn win_cus_lay_handle(&mut self, event: P<Event>, x: i16, y: i16) {
        let current = self.o.gl.winget(&mut self.m);
        let v = self.views();
        let n = self.n_views() as u32;
        let mut hidden = false;
        let mut i = 0u32;
        if n != 0 {
            loop {
                hidden = self.m[v.add(i as i32)].visible == 0;
                i += 1;
                if n <= i || hidden {
                    break;
                }
            }
        }
        if hidden {
            return;
        }
        let g = [v, v.add(1), v.add(2), v.add(3)];
        let device = self.m[event].second;
        if device > 100 && device < 0x67 {
            if self.m[FIRST_CUS_LAY] != 0 {
                self.window_toggle_off_rgb();
                let c = self.m[WIN_CENTER];
                self.m[g[0]].right = c[0];
                self.m[g[2]].left = c[1];
                self.m[g[1]].right = c[2];
                self.m[g[3]].left = c[3];
                self.m[g[0]].top = c[4];
                self.m[g[1]].bottom = c[5];
                self.m[g[2]].top = c[6];
                self.m[g[3]].bottom = c[7];
                for w in g {
                    self.si_get_window_viewport(w);
                }
                self.restore_rgb();
                self.win_cus_lay_disp_area();
                self.m[event].kind = 0x1e;
            }
        } else if device == 0x67 {
            let first = self.m[self.views()];
            let (xi, yi) = (x as i32, y as i32);
            if first.right as i32 - 7 <= xi && xi <= first.right as i32 + 0xd && first.top as i32 - 7 <= yi && yi <= first.top as i32 + 0xd {
                let t = self.m[SI_TEMPLATE];
                let title = self.m[SI_TITLE_HEIGHT];
                let y_min = title.wrapping_add(t.views_bottom);
                let y_max = t.views_top.wrapping_sub(title);
                let mut x_min = t.views_left.wrapping_add(5);
                let mut x_max = t.views_right.wrapping_sub(5);
                let ruler = |ui: &Ui, w: P<Grafport>| {
                    let r = ui.m[w].ruler_v;
                    (ui.m.i16(r.wrapping_add(off::RULER_ACTIVE)), r)
                };
                let wider = |ui: &Ui, a: u32, b: u32| {
                    let (wa, wb) = (ui.m.i32(a.wrapping_add(off::RULER_WIDTH)), ui.m.i32(b.wrapping_add(off::RULER_WIDTH)));
                    if wa < wb {
                        wb
                    } else {
                        wa
                    }
                };
                let ((a0, r0), (a1, r1)) = (ruler(self, g[0]), ruler(self, g[1]));
                if a0 != 0 || a1 != 0 {
                    x_min = t.views_left.wrapping_add(6).wrapping_add(wider(self, r1, r0) as i16);
                }
                let (a2, r2) = ruler(self, g[2]);
                if a2 != 0 || ruler(self, g[3]).0 != 0 {
                    let r3 = ruler(self, g[3]).1;
                    x_max = t.views_right.wrapping_sub(6).wrapping_sub(wider(self, r3, r2) as i16);
                }
                let sysid = self.m[SI_SYSID];
                if current != sysid {
                    self.o.gl.winset(&mut self.m, sysid);
                }
                if self.m[FIRST_CUS_LAY] == 0 {
                    let c = [
                        self.m[g[0]].right,
                        self.m[g[2]].left,
                        self.m[g[1]].right,
                        self.m[g[3]].left,
                        self.m[g[0]].top,
                        self.m[g[1]].bottom,
                        self.m[g[2]].top,
                        self.m[g[3]].bottom,
                    ];
                    self.m[WIN_CENTER] = c;
                    self.m[FIRST_CUS_LAY] = 1;
                }
                self.window_toggle_off_rgb();
                self.o.gl.gflush(&mut self.m);
                let (cx, cy) = (self.m[SI_COORDX] as u16, self.m[SI_COORDY] as u16);
                let at_x = self.m[g[0]].right.wrapping_add(3);
                self.o.gl.setvaluator(&mut self.m, cx, at_x, x_min, x_max);
                let at_y = self.m[g[0]].top.wrapping_add(3);
                self.o.gl.setvaluator(&mut self.m, cy, at_y, y_min.wrapping_add(6), y_max.wrapping_sub(3));
                self.win_cus_lay_reshape_handle(g[0], g[1], g[2], g[3]);
                self.restore_rgb();
                self.m[event].kind = 0x1e;
                let max = self.m[SI_XMAXSCREEN];
                let (cx, cy) = (self.m[SI_COORDX] as u16, self.m[SI_COORDY] as u16);
                let at_x = self.m[g[0]].right.wrapping_add(3);
                self.o.gl.setvaluator(&mut self.m, cx, at_x, 0, max);
                let at_y = self.m[g[0]].top.wrapping_add(3);
                self.o.gl.setvaluator(&mut self.m, cy, at_y, 0, max);
                if current != self.m[SI_SYSID] {
                    self.o.gl.winset(&mut self.m, current);
                }
            } else {
                self.m[event].kind = ev::NONE;
            }
        }
        for w in g {
            if self.camera_type(w) == cam::SPREADSHEET {
                self.o.views.SI_spdViewSizeNew(&mut self.m, w);
            }
        }
    }

    /// Follow the drag of the views' common corner while the left button is held
    /// [soft 0x083d3950 winCusLayReshapeHandle]. The pointer is kept 13 pixels inside the
    /// views' area in x and a title bar's height inside it in y; each time it has moved,
    /// the four windows' inner edges move with it, their drawing areas are recomputed and
    /// everything is redrawn.
    pub fn win_cus_lay_reshape_handle(&mut self, g0: P<Grafport>, g1: P<Grafport>, g2: P<Grafport>, g3: P<Grafport>) {
        let held = |ui: &mut Ui| ui.o.gl.getbutton(&mut ui.m, 0x67) != 0 || ui.m[SI_TABLET_BUTTON] == 0x67;
        let pointer = |ui: &mut Ui| {
            let cx = ui.m[SI_COORDX] as u16;
            let x = ui.o.gl.getvaluator(&mut ui.m, cx);
            let cy = ui.m[SI_COORDY] as u16;
            let y = ui.o.gl.getvaluator(&mut ui.m, cy);
            (x, y)
        };
        if held(self) {
            self.o.gl.getwritemask(&mut self.m);
            let (mut last_x, mut last_y) = pointer(self);
            loop {
                if self.m[SI_TABLET_ACTIVE] != 0 {
                    self.o.devices.SI_tabletGetButtonUpEvent(&mut self.m);
                }
                let (mut x, mut y) = pointer(self);
                let t = self.m[SI_TEMPLATE];
                let title = self.m[SI_TITLE_HEIGHT] as i32;
                let low = t.views_left as i32 + 0xd;
                let high = t.views_right as i32 - 0xd;
                if x <= low {
                    x = low;
                } else if high <= x {
                    x = high;
                }
                let high = t.views_top as i32 - title;
                let low = t.views_bottom as i32 + title;
                if high < y {
                    y = high;
                } else if y < low {
                    y = low;
                }
                if x != last_x || y != last_y {
                    let dy = (y as i16).wrapping_sub(last_y as i16);
                    let dx = (x as i16).wrapping_sub(last_x as i16);
                    let add = |v: &mut i16, d: i16| *v = v.wrapping_add(d);
                    add(&mut self.m[g0].right, dx);
                    add(&mut self.m[g2].left, dx);
                    add(&mut self.m[g1].right, dx);
                    add(&mut self.m[g3].left, dx);
                    add(&mut self.m[g0].top, dy);
                    add(&mut self.m[g1].bottom, dy);
                    add(&mut self.m[g2].top, dy);
                    add(&mut self.m[g3].bottom, dy);
                    for w in [g0, g1, g2, g3] {
                        self.si_get_window_viewport(w);
                    }
                    self.si_window_draw_all();
                    self.o.views.SI_drawGrid(&mut self.m, P::NULL, 0);
                    self.win_cus_lay_disp_area();
                    self.o.gl.swapbuffers(&mut self.m);
                    last_y = y;
                    last_x = x;
                }
                if !held(self) {
                    break;
                }
            }
        }
        self.win_cus_lay_disp_area();
    }

    /// Call the handler of a part of a view window [soft 0x083d3ae8 si_viewSelectHandle].
    /// Without a window, the one under the point is taken. A window that is not enabled
    /// only answers its zoom box (part 3) and its title (part 9); for those, and for a part
    /// without a handler, the event is voided.
    pub fn si_view_select_handle(&mut self, event: P<Event>, x: i16, y: i16, part: i32, g: P<Grafport>) {
        let mut g = g;
        if g.is_null() {
            if let Some(found) = self.window_at(x, y) {
                g = found;
            }
        }
        let handler = self.m.u32(GADGET_INFO.addr().wrapping_add(0x18).wrapping_add((part as u32).wrapping_mul(Gadget::SIZE)));
        if (self.m[g].enabled == 0 && part != 3 && part != 9) || handler == 0 {
            self.void_event(event);
        } else {
            let cb = Callback { event, x, y, window: g, part };
            self.call_gadget_handler(handler, cb);
        }
    }

    /// The call through a gadget's handler pointer: one of this file's `in…CB` functions by
    /// its address in the original, or something else's.
    pub fn call_gadget_handler(&mut self, handler: u32, cb: Callback) {
        if let Some((_, name)) = HANDLERS.iter().find(|(addr, _)| *addr == handler) {
            self.call_handler_by_name(name, &cb);
        } else {
            self.o.views.gadgetCallback(&mut self.m, &[cb.event.addr(), (cb.x as u16 as u32) | (cb.y as u16 as u32) << 16, cb.window.addr(), cb.part as u32]);
        }
    }

    /// Select a view window, or deselect it if it was selected
    /// [soft 0x083d3bd4 windowToggleSelect]. Another selected window loses the highlight first (its frame
    /// and title line are redrawn in the text colour). Frames are drawn in the front
    /// buffer; the buffers are swapped at the end. Returns 0, which the caller stores as
    /// the event's type.
    pub fn window_toggle_select(&mut self, g: P<Grafport>) -> i32 {
        let frame = |ui: &mut Ui, w: P<Grafport>, colour: i16| {
            ui.o.gl.color(&mut ui.m, colour as u16);
            ui.o.gl.frontbuffer(&mut ui.m, 1);
            let r = ui.m[w];
            let title = ui.m[SI_TITLE_HEIGHT] as i32;
            ui.o.gl.sboxi(&mut ui.m, r.left as i32, r.bottom as i32, r.right as i32, r.top as i32);
            ui.o.gl.sboxi(&mut ui.m, r.left as i32, r.top as i32 + 1 - title, r.right as i32, r.top as i32);
            ui.o.gl.frontbuffer(&mut ui.m, 0);
        };
        for i in 0..self.n_views().max(0) {
            let w = self.views().add(i);
            if w != g && self.m[w].hilite != 0 {
                self.m[w].hilite = 0;
                if self.m[w].visible != 0 {
                    let c = self.m[SI_WINDOW_TEXT];
                    frame(self, w, c);
                }
                break;
            }
        }
        let was = self.m[g].hilite;
        self.m[g].hilite = (was == 0) as i16;
        let c = if was == 0 { self.m[SI_WINDOW_FRAME_HILITE] } else { self.m[SI_WINDOW_TEXT] };
        frame(self, g, c);
        self.o.gl.swapbuffers(&mut self.m);
        0
    }

    /// Switch the pictures of the four views off [soft 0x083d4304 windowToggleOffRgb].
    pub fn window_toggle_off_rgb(&mut self) {
        let v = self.views();
        for i in 0..4 {
            self.window_toggle_off_rgb_wind(v.add(i));
        }
    }

    /// Forget which display mode each view had before it was switched off
    /// [soft 0x083d49b4 si_resetToggleFlag].
    pub fn si_reset_toggle_flag(&mut self) {
        for i in 0..self.n_views().max(0) {
            let at = TOGGLE_RGB.addr().wrapping_add(4 * i as u32);
            self.m.set_i32(at, 0);
        }
    }

    /// Bring the views' pictures back [soft 0x083d49e4 restoreRgb].
    pub fn restore_rgb(&mut self) {
        let n = self.n_views() as u32;
        let mut i = 0u32;
        while i < n {
            let w = self.views().add(i as i32);
            self.restore_rgb_wind(w);
            i += 1;
        }
    }

    /// Whether a view shows the given display mode [soft 0x083d6304 SI_windowIsActive].
    pub fn si_window_is_active(&mut self, display: i32) -> i32 {
        (!self.window_get_disp_typ(display).is_null()) as i32
    }

    /// The first view window with the given display mode, or null
    /// [soft 0x083d6350 windowGetDispTyp].
    pub fn window_get_disp_typ(&mut self, display: i32) -> P<Grafport> {
        let mut g = P::NULL;
        loop {
            if self.si_window_get_next(&mut g) == 0 {
                return P::NULL;
            }
            if self.m[g].display == display {
                return g;
            }
        }
    }

    /// The first active view window whose camera is of the given type, or null
    /// [soft 0x083d6390 SI_windowCamActive]. "Active" is `SI_windowGetNextActive`'s notion
    /// (`uiLite.c`, not ported).
    pub fn si_window_cam_active(&mut self, camera_type: i32) -> P<Grafport> {
        let mut g = P::NULL;
        loop {
            if self.o.dialogs.SI_windowGetNextActive(&mut self.m, InOut(&mut g)) == 0 {
                return P::NULL;
            }
            if self.camera_type(g) as i32 == camera_type {
                return g;
            }
        }
    }

    /// The selected view window, or null [soft 0x083d63e0 SI_windowGetHilite].
    pub fn si_window_get_hilite(&mut self, out: &mut P<Grafport>) {
        *out = P::NULL;
        let n = self.m[SI_TEMPLATE].n_views;
        if n != 0 {
            let v = self.views();
            let mut i = 0u32;
            if self.m[v].hilite == 0 {
                loop {
                    i += 1;
                    if (n as i32 as u32) <= i {
                        return;
                    }
                    if self.m[v.add(i as i32)].hilite != 0 {
                        break;
                    }
                }
            }
            *out = v.add(i as i32);
        }
    }

    /// The selected view window if it is one of the camera views (front, top, right,
    /// perspective, free orthogonal); otherwise a message and null
    /// [soft 0x083d6430 SI_windowViewGetHilite].
    pub fn si_window_view_get_hilite(&mut self, out: &mut P<Grafport>) {
        self.si_window_get_hilite(out);
        if !out.is_null() {
            let t = self.camera_type(*out);
            if t < 2000 || (cam::PERSPECTIVE < t && t != cam::ORTHO) {
                self.o.status.SI_statusDisplayMessage(&mut self.m, b"The selected view type is not supported.", 2);
                *out = P::NULL;
            }
        }
    }

    /// The view window after `*g` in the array, the first one for null; 0 at the end
    /// [soft 0x083d6484 SI_windowGetNext]. (Asked with null it answers the first window
    /// even when the template has none.)
    pub fn si_window_get_next(&mut self, g: &mut P<Grafport>) -> i32 {
        let mut i = 0u32;
        if !g.is_null() {
            let mut found = false;
            let n = self.n_views() as u32;
            let mut p = self.views();
            if n != 0 {
                loop {
                    found = p == *g;
                    i += 1;
                    if n <= i {
                        break;
                    }
                    p = p.add(1);
                    if found {
                        break;
                    }
                }
            }
            if !found || i == n {
                return 0;
            }
        }
        *g = P::at(self.views().addr().wrapping_add(i.wrapping_mul(Grafport::SIZE)));
        1
    }

    /// The next visible view window [soft 0x083d6500 SI_windowGetNextVisible].
    pub fn si_window_get_next_visible(&mut self, g: &mut P<Grafport>) -> i32 {
        let mut i = 0u32;
        let n = self.n_views() as u32;
        if !g.is_null() {
            let mut p = self.views();
            if n == 0 {
                return 0;
            }
            loop {
                let other = p != *g;
                i += 1;
                if n <= i {
                    return 0;
                }
                p = p.add(1);
                if !other {
                    break;
                }
            }
        }
        while i < n {
            let w = P::<Grafport>::at(self.views().addr().wrapping_add(i.wrapping_mul(Grafport::SIZE)));
            if self.m[w].visible != 0 {
                *g = w;
                return 1;
            }
            i += 1;
        }
        0
    }

    /// Choose the buffers to draw into, remembering the current choice for
    /// `SI_windowUnset` [soft 0x083d6588 SI_windowSet]: mode 0 the back buffer, 2 both,
    /// 1, 3, 4, 5 only switch the front buffer off. The window argument is not used.
    pub fn si_window_set(&mut self, mode: i32, _g: P<Grafport>) {
        self.m[BUFFER] = self.o.gl.getbuffer(&mut self.m);
        match mode {
            0 | 2 => {
                self.o.gl.frontbuffer(&mut self.m, (mode == 2) as i32);
                self.o.gl.backbuffer(&mut self.m, 1);
            }
            1 | 3 | 4 | 5 => self.o.gl.frontbuffer(&mut self.m, 0),
            _ => {}
        }
    }

    /// Put the buffers back as `SI_windowSet` found them (1 back, 2 front, anything else
    /// both) and make the main window current [soft 0x083d65f4 SI_windowUnset].
    pub fn si_window_unset(&mut self) {
        if self.m[BUFFER] == 0 {
            self.o.gl.frontbuffer(&mut self.m, 0);
            self.o.gl.backbuffer(&mut self.m, 0);
        }
        let (front, back) = match self.m[BUFFER] {
            1 => (0, 1),
            2 => (1, 0),
            _ => (1, 1),
        };
        self.o.gl.frontbuffer(&mut self.m, front);
        self.o.gl.backbuffer(&mut self.m, back);
        let sysid = self.m[SI_SYSID];
        self.o.gl.winset(&mut self.m, sysid);
    }

    /// Compute a view window's drawing area from its rectangle
    /// [soft 0x083d6670 SI_getWindowViewport].
    ///
    /// A perspective view whose camera has an aspect ratio other than 0 gets the largest
    /// area of that ratio centred in the window: with w = right − left − 2 and h = top −
    /// bottom − 1 − title height, if w / h is less than the ratio the full width is used
    /// and the height is w / ratio around the middle; otherwise the full height and h ×
    /// ratio around the middle. (Both copies of the area are set.)
    ///
    /// Any other view: the rectangle inset by one pixel, less the title bar; a shown ruler
    /// takes its width from the left (vertical ruler) or the bottom (horizontal ruler),
    /// never beyond the opposite edge + 1. The schematic view (2007) never has rulers; the
    /// dope sheet (2008) takes the rulers' widths whether they are shown or not. (Only the
    /// first copy of the area is set.)
    pub fn si_get_window_viewport(&mut self, g: P<Grafport>) {
        let camera = self.m[g].camera;
        let aspect = self.m.f32(camera.wrapping_add(off::CAMERA_ASPECT));
        let title = self.m[SI_TITLE_HEIGHT];
        if !(aspect == 0.0) && self.m.i16(camera.wrapping_add(off::CAMERA_TYPE)) == cam::PERSPECTIVE {
            let w = self.m[g];
            let (l, r, b, t) = (w.left as i32, w.right as i32, w.bottom as i32, w.top as i32);
            let asp = F80::from_f32(aspect);
            let width = F80::from_i32(r - l - 2);
            let height = F80::from_i32(t - b - 1 - title as i32);
            let cx = F80::from_f32((F80::from_i32(l + r) * 0.5f64).to_f32());
            let cy = F80::from_f32((F80::from_i32(b + t + 1 - title as i32) * 0.5f64).to_f32());
            let w = &mut self.m[g];
            if (width / height).lt(asp) {
                let half = width / (asp + asp);
                w.view_left = w.left.wrapping_add(1);
                w.view_right = w.right.wrapping_sub(1);
                w.view_bottom = (cy - half).to_i32(Round::Zero) as i16;
                w.view_top = (cy + half).to_i32(Round::Zero) as i16;
            } else {
                let half = (F80::from_f64(0.5) * height) * asp;
                w.view_bottom = w.bottom.wrapping_add(1);
                w.view_top = w.top.wrapping_sub(title);
                w.view_left = (cx - half).to_i32(Round::Zero) as i16;
                w.view_right = (cx + half).to_i32(Round::Zero) as i16;
            }
            w.view2 = [w.view_left, w.view_right, w.view_bottom, w.view_top];
            return;
        }
        // one edge: `low` is the window's edge, `high` the opposite one
        let edge = |ui: &Ui, ruler: u32, low: i16, high: i16| -> i16 {
            let t = ui.camera_type(g);
            if t == cam::SCHEMATIC || (t != cam::DOPE_SHEET && (ruler == 0 || ui.m.i16(ruler + off::RULER_ACTIVE) == 0)) {
                return low.wrapping_add(1);
            }
            if ruler == 0 {
                if (low as i32) < high as i32 + 1 {
                    low
                } else {
                    high.wrapping_add(1)
                }
            } else {
                let width = ui.m.i32(ruler + off::RULER_WIDTH);
                if high as i32 + 1 <= (low as i32).wrapping_add(width) {
                    high.wrapping_add(1)
                } else {
                    low.wrapping_add(width as i16)
                }
            }
        };
        let w = self.m[g];
        self.m[g].view_left = edge(self, w.ruler_v, w.left, w.right);
        self.m[g].view_right = w.right.wrapping_sub(1);
        let w = self.m[g];
        self.m[g].view_bottom = edge(self, w.ruler_h, w.bottom, w.top);
        self.m[g].view_top = w.top.wrapping_sub(title);
    }

    /// The part of a view window a point is in [soft 0x083d6b94 fndViewPart]; the part
    /// numbers are the indices of `gadgetInfo`. −1 is "no part"; 0 is the drawing area (and
    /// the frame).
    ///
    /// Below the title bar (and not in the Dialog Editor): 1 the camera control buttons in
    /// the upper right corner of the drawing area when they are shown, 32 the curve list of
    /// a function-curve view, else 0. In the title bar, by x: 2 the selection box (3 to 13
    /// pixels from the left), 3 the zoom box (the same from the right), 50 CLIP, 6 SETUP,
    /// 5 the display-mode name, 9 the title, 4 the stored-camera boxes, 7 the layout box,
    /// then the gadgets of the view's kind, 8 the "1:1" box.
    pub fn fnd_view_part(&mut self, g: P<Grafport>, x: i32, y: i32) -> i32 {
        let w = self.m[g];
        let (left, right, top) = (w.left as i32, w.right as i32, w.top as i32);
        let title = self.m[SI_TITLE_HEIGHT] as i32;
        if w.d_050 == -1 || w.view_top as i32 <= top - title {
            if self.o.server.SI_isDialogEditor(&mut self.m) == 0
                && w.left as i32 <= x
                && x <= w.right as i32
                && w.bottom as i32 <= y
                && y <= w.top as i32 - title
            {
                if self.m.i16(w.ctrl) != 0 && right - (self.m[SI_CTRL_WIDTH] as i32) < x && (top - title) - (self.m[SI_CTRL_HEIGHT] as i32) < y {
                    return 1;
                }
                if self.camera_type(g) != cam::FCURVE {
                    return 0;
                }
                if self.o.views.si_inWFcvList(&mut self.m, g, x, y) == 0 {
                    return 0;
                }
                return 0x20;
            }
            if y < top - title {
                return 0;
            }
        } else if x < left || right < x || y < w.bottom as i32 {
            if y < top - title {
                return 0;
            }
        } else if y <= top - title {
            return 0;
        }
        if top - 3 < y {
            return 0;
        }
        if left + 3 <= x && x <= left + 0xd {
            return 2;
        }
        if right - 0xd <= x && x <= right - 3 {
            return 3;
        }
        let setup = right - self.m[SI_TITLE_SETUP] as i32;
        let spacing = self.m[SI_MEMO_SPACING] as i32;
        let t = self.camera_type(g);
        let skip_setup = if x < setup + spacing * -2 - 0x1e {
            x < setup - spacing
        } else if x <= setup - spacing {
            if t < cam::PERSPECTIVE && 1999 < t {
                return 0x32;
            }
            x < setup - spacing
        } else {
            false
        };
        if !skip_setup && x <= right - 0x45 && w.display < 7 && 0 < w.display {
            return 6;
        }
        if right - 0x46 <= x && x <= right - 0x14 && 1999 < t && (t < cam::FCURVE || t == cam::ORTHO) {
            return 5;
        }
        if left + 0x1d <= x && x <= left + 0x6a {
            return 9;
        }
        let memo = left + self.m[SI_MEMO_LEFT_OFFSET] as i32;
        if t != cam::SPREADSHEET && memo <= x && x <= spacing * 3 + self.m[SI_MEMO_WIDTH] as i32 * 4 + memo {
            return 4;
        }
        let in_gadget = |ui: &mut Ui, part: i32, margin: i32| {
            ui.get_gadget_offset(part) - margin + left <= x && x <= left + ui.get_gadget_offset(part) + ui.get_gadget_width(part) + margin
        };
        match t {
            cam::DOPE_SHEET | cam::SPREADSHEET => self.locate_gadget(t as i32, x - left),
            cam::FCURVE | cam::SCHEMATIC | cam::ORTHO => {
                if in_gadget(self, 7, 0) {
                    return 7;
                }
                self.locate_gadget(t as i32, x - left)
            }
            _ => {
                if in_gadget(self, 7, 0) {
                    return 7;
                }
                if ((w.display.wrapping_sub(2) as u32) < 2
                    || (t == cam::PERSPECTIVE && self.m.i16(w.camera.wrapping_add(off::CAMERA_44)) != 0))
                    && in_gadget(self, 8, 1)
                {
                    return 8;
                }
                -1
            }
        }
    }

    /// Redraw the area of the views [soft 0x083d6f64 SI_windowDrawAll]: in the main window,
    /// normal draw mode, all planes writable, the views' area of the active canvas filled
    /// with the view-background colour; then the zoomed window alone if one is zoomed, else
    /// each visible one of the four (a window showing a rotoscope mode, 2 or 3, through
    /// `si_roto_viewport_clr`). Window, draw mode and write mask are put back. In the
    /// Dialog Editor the canvas and the edited dialog are drawn into both buffers instead.
    pub fn si_window_draw_all(&mut self) {
        if self.o.server.SI_isDialogEditor(&mut self.m) != 0 {
            let id = self.m[DE_DIALOG_SYS] as i32;
            let dialog = self.o.dialogs.SI_dialogOpen(&mut self.m, id);
            let canvas = self.m[SI_ACTIVE_CANVAS];
            self.o.views.SI_canvasDraw(&mut self.m, canvas, 1);
            self.o.dialogs.SI_dialogDraw(&mut self.m, dialog);
            self.o.gl.swapbuffers(&mut self.m);
            let canvas = self.m[SI_ACTIVE_CANVAS];
            self.o.views.SI_canvasDraw(&mut self.m, canvas, 1);
            self.o.dialogs.SI_dialogDraw(&mut self.m, dialog);
            return;
        }
        let mode = self.o.gl.getdrawmode(&mut self.m);
        let mask = self.o.gl.getwritemask(&mut self.m);
        let current = self.o.gl.winget(&mut self.m);
        let sysid = self.m[SI_SYSID];
        self.o.gl.winset(&mut self.m, sysid);
        self.o.gl.drawmode(&mut self.m, 0x10);
        self.o.gl.writemask(&mut self.m, 0xfff);
        let c = self.m[SI_VIEW_BACK];
        self.o.gl.color(&mut self.m, c as u16);
        let canvas = self.m[self.m[SI_ACTIVE_CANVAS]];
        self.sboxfi(canvas.views_left as i32, canvas.views_bottom as i32, canvas.views_right as i32, canvas.views_top as i32);
        self.o.gl.writemask(&mut self.m, mask as u16);
        let n = self.n_views();
        let (mut i, mut zoomed) = (0, 0i16);
        if n > 0 {
            loop {
                zoomed = self.m[self.views().add(i)].zoomed;
                i += 1;
                if n <= i || zoomed != 0 {
                    break;
                }
            }
        }
        if zoomed == 0 {
            for k in 0..4 {
                let g = self.views().add(k);
                if self.m[g].visible != 0 {
                    let d = self.m[g].display;
                    if d < 4 && 1 < d {
                        self.si_roto_viewport_clr(g);
                    } else {
                        self.si_window_draw(g);
                    }
                }
            }
        } else {
            let g = self.views().add(i - 1);
            self.si_window_draw(g);
        }
        self.o.gl.winset(&mut self.m, current);
        self.o.gl.drawmode(&mut self.m, mode);
        self.o.gl.writemask(&mut self.m, mask as u16);
    }

    /// A click in the views that only selects a window
    /// [soft 0x083d7120 SI_viewGrafportSelect]: if the point is in the drawing area (or on the camera
    /// buttons) of a visible, enabled window, the event becomes 5 with the window's
    /// identifier, the point and the window; otherwise it is cleared.
    pub fn si_view_grafport_select(&mut self, _canvas: P<Canvas>, event: P<Event>, x: i16, y: i16) {
        if let Some(g) = self.window_at(x, y) {
            let part = self.fnd_view_part(g, x as i32, y as i32);
            if !(0..=1).contains(&part) {
                self.m[event].kind = ev::NONE;
                return;
            }
            if self.m[g].visible != 0 && self.m[g].enabled != 0 {
                let id = self.m[g].id;
                let e = &mut self.m[event];
                e.kind = 5;
                e.first = id;
                e.second = -1;
                e.x = x;
                e.y = y;
                e.data = g.addr();
                return;
            }
        }
        self.m[event].kind = ev::NONE;
    }

    /// How many view windows have a picture window of their own (the word at +0x50 is not
    /// −1) [soft 0x083d9340 SI_getNbWindows].
    pub fn si_get_nb_windows(&mut self) -> i32 {
        let mut count = 0;
        for i in 0..self.n_views().max(0) {
            if self.m[self.views().add(i)].d_050 != -1 {
                count += 1;
            }
        }
        count
    }

    /// The text of a part of a view window, null for a number outside the table
    /// [soft 0x083d94e0 getGadgetLabel].
    pub fn get_gadget_label(&mut self, part: i32) -> u32 {
        if (part as u32) < 0x37 {
            self.m[GADGET_INFO][part as usize].label
        } else {
            0
        }
    }

    /// Where a part starts, from the window's left edge; −1 outside the table
    /// [soft 0x083d9508 getGadgetOffset].
    pub fn get_gadget_offset(&mut self, part: i32) -> i32 {
        if (part as u32) < 0x37 {
            self.m[GADGET_INFO][part as usize].offset
        } else {
            -1
        }
    }

    /// A part's width; −1 outside the table [soft 0x083d9530 getGadgetWidth].
    pub fn get_gadget_width(&mut self, part: i32) -> i32 {
        if (part as u32) < 0x37 {
            self.m[GADGET_INFO][part as usize].width
        } else {
            -1
        }
    }

    /// The gadget of a kind of view at a distance from the window's left edge, −1 for none
    /// [soft 0x083d97fc locateGadget].
    pub fn locate_gadget(&mut self, camera_type: i32, x: i32) -> i32 {
        for i in 0..0x37 {
            let gadget = self.m[GADGET_INFO][i];
            if gadget.camera == camera_type && gadget.offset <= x && x <= gadget.offset.wrapping_add(gadget.width) {
                return i as i32;
            }
        }
        -1
    }

    /// Make an event mean "nothing more to do" [soft 0x083d9acc voidEvent]: type 0, second
    /// word −1, point (0, 0). A null pointer is accepted.
    pub fn void_event(&mut self, event: P<Event>) {
        if !event.is_null() {
            let e = &mut self.m[event];
            e.kind = ev::NONE;
            e.second = -1;
            e.x = 0;
            e.y = 0;
        }
    }

    /// Copy the first module's canvas into the screen template: the words at +0x10, +0x1c,
    /// +0x20 to +0x2c, the views' area and +0x64 to +0x6a; and no window has the focus yet
    /// [soft 0x083d2010 setTemplate].
    pub fn set_template(&mut self) {
        let first = P::<Canvas>::at(self.m.u32(self.m[SI_NETWORK].wrapping_add(8)));
        let c = self.m[first];
        let t = &mut self.m[SI_TEMPLATE];
        t.w_010 = c.w_010;
        t.d_01c = c.d_01c;
        t.w_020 = c.w_020;
        t.active = c.active;
        t.views_left = c.views_left;
        t.views_right = c.views_right;
        t.views_bottom = c.views_bottom;
        t.views_top = c.views_top;
        t.w_064 = c.w_064;
        t.winid = -1;
    }

    /// A click on a view window's zoom box [soft 0x083d3fb0 windowZoomHandle]. A zoomed
    /// window goes back to its place (event 0x1e). Otherwise by button: left zooms the
    /// window to the whole area of the views (event 0xf with the window); middle expands it
    /// upwards or back; right expands it sideways or back (event 0x1e). The camera buttons'
    /// turntable is erased first, and a window in a rotoscope mode is refreshed after.
    pub fn window_zoom_handle(&mut self, g: P<Grafport>, event: P<Event>) {
        if self.m.i16(self.m[g].ctrl) != 0 {
            self.o.views.SI_eraseTurnTable(&mut self.m, g);
        }
        let mut changed = true;
        if self.m[g].zoomed != 0 {
            self.si_un_zoom_window(g);
        } else {
            match self.m[event].second {
                0x66 => self.window_toggle_vertical(g),
                0x65 => self.window_toggle_horizontal(g),
                0x67 => {
                    self.zoom_window(g);
                    self.si_window_gadjet(g, -1, 1);
                    let id = self.m[g].id;
                    let e = &mut self.m[event];
                    e.kind = 0xf;
                    e.first = id;
                    e.second = -1;
                    e.x = 0;
                    e.y = 0;
                    e.data = g.addr();
                    changed = false;
                }
                _ => changed = false,
            }
        }
        if changed {
            self.m[event].kind = 0x1e;
        }
        if (self.m[g].display.wrapping_sub(2) as u32) < 2 {
            self.o.views.SI_refreshSystem(&mut self.m, g, 0, 7);
        }
    }

    /// Expand a window sideways, or put an expanded one back
    /// [soft 0x083d4084 windowToggleHorizontal].
    pub fn window_toggle_horizontal(&mut self, g: P<Grafport>) {
        let e = self.m[g].expand;
        if e == 0 {
            self.si_window_size_set(g, 1);
        } else if (0..3).contains(&e) {
            self.si_window_size_set(g, 0);
        }
    }

    /// Change a window's expansion: 0 none, 1 sideways, 2 upwards
    /// [soft 0x083d40b8 SI_windowSizeSet]. The window's picture is switched off, the edges moved, the
    /// viewport prepared and the picture restored.
    pub fn si_window_size_set(&mut self, g: P<Grafport>, how: i32) {
        self.window_toggle_off_rgb_wind(g);
        match how {
            1 => self.window_expand_horiz(g),
            0 => match self.m[g].expand {
                1 => self.window_un_expand_horiz(g),
                2 => self.window_un_expand_vert(g),
                _ => {}
            },
            2 => self.window_expand_vert(g),
            _ => {}
        }
        self.o.views.SI_prepareViewport(&mut self.m, g);
        self.restore_rgb_wind(g);
    }

    /// Expand a window upwards, or put an expanded one back
    /// [soft 0x083d4148 windowToggleVertical].
    pub fn window_toggle_vertical(&mut self, g: P<Grafport>) {
        let e = self.m[g].expand;
        if e == 0 {
            self.si_window_size_set(g, 2);
        } else if (0..3).contains(&e) {
            self.si_window_size_set(g, 0);
        }
    }

    /// A zoomed window gets back the rectangle the template saved for it
    /// [soft 0x083d4b98 windowBigToSmall].
    pub fn window_big_to_small(&mut self, g: P<Grafport>) {
        let t = self.m[SI_TEMPLATE];
        let w = &mut self.m[g];
        w.zoomed = 0;
        w.left = t.saved_left;
        w.right = t.saved_right;
        w.bottom = t.saved_bottom;
        w.top = t.saved_top;
    }

    /// Give a window the whole area of the views, two pixels inside it
    /// [soft 0x083d4bd4 windowSmallToBig]. Another window that was zoomed goes back first;
    /// this window's rectangle is saved in the template.
    pub fn window_small_to_big(&mut self, g: P<Grafport>) {
        let mut other = P::NULL;
        while self.si_window_get_next(&mut other) != 0 {
            if self.m[other].zoomed != 0 && g != other {
                self.window_big_to_small(other);
                break;
            }
        }
        let w = self.m[g];
        let t = &mut self.m[SI_TEMPLATE];
        t.saved_left = w.left;
        t.saved_right = w.right;
        t.saved_bottom = w.bottom;
        t.saved_top = w.top;
        let t = self.m[SI_TEMPLATE];
        let w = &mut self.m[g];
        w.zoomed = 1;
        w.left = t.views_left.wrapping_add(2);
        w.right = t.views_right.wrapping_sub(2);
        w.bottom = t.views_bottom.wrapping_add(2);
        w.top = t.views_top.wrapping_sub(2);
    }

    /// Zoom a window with its redraw [soft 0x083d4c90 zoomWindow]: pictures off, the window
    /// enlarged (`SI_windowZoomWin`, which hides the other three), drawn into both buffers,
    /// pictures back; a spreadsheet is told its new size.
    pub fn zoom_window(&mut self, g: P<Grafport>) {
        self.window_toggle_off_rgb();
        self.si_window_zoom_win(g);
        self.si_window_draw(g);
        self.o.gl.swapbuffers(&mut self.m);
        self.si_window_draw(g);
        self.restore_rgb();
        if self.camera_type(g) == cam::SPREADSHEET {
            self.o.views.SI_spdViewSizeNew(&mut self.m, g);
        }
    }

    /// [soft 0x083d5194 windowSetAllVisible]
    pub fn window_set_all_visible(&mut self) {
        for i in 0..self.n_views().max(0) {
            let w = self.views().add(i);
            self.m[w].visible = 1;
        }
    }

    /// Change what a window shows, to the entry `item` of its view-type menu, with the
    /// menu's marks [soft 0x083d51c4 changeWindowAll]. For anything but 9 ("Freeze") every
    /// entry's mark is cleared and the new one set. Afterwards entry 9 is marked exactly
    /// when the window is not enabled. (A menu's entries are 0x30 bytes each with the mark
    /// at +4; the menu's count is its word at +0x18, the entries its pointer at +0x20.)
    pub fn change_window_all(&mut self, g: P<Grafport>, item: i32) {
        let menu = self.m[g].menu_view;
        if item != 9 {
            let n = self.m.i16(menu.wrapping_add(0x18)) as i32;
            for i in 0..n.max(0) {
                let entries = self.m.u32(menu.wrapping_add(0x20));
                self.m.set_i16(entries.wrapping_add(4 + i as u32 * 0x30), 0);
            }
            self.o.menus.SI_setMenuFlag(&mut self.m, menu, item, 1);
        }
        self.change_window(g, item);
        let frozen = (self.m[g].enabled == 0) as i16;
        let entries = self.m.u32(menu.wrapping_add(0x20));
        self.m.set_i16(entries.wrapping_add(0x1b4), frozen);
    }

    /// Remember the outer edges of the four views as the layout gave them
    /// [soft 0x083d5464 winStoreBorder]: left and bottom from window 1000, right and top
    /// from window 1003.
    pub fn win_store_border(&mut self, g: P<Grafport>) {
        let w = self.m[g];
        if w.id == 1000 {
            self.m[WIN_BORDER][0] = w.left;
            self.m[WIN_BORDER][1] = w.bottom;
        } else if w.id == 1003 {
            self.m[WIN_BORDER][2] = w.right;
            self.m[WIN_BORDER][3] = w.top;
        }
    }

    /// Save a window's rectangle in slot `slot` unless the slot is in use (its first word
    /// is not zero) [soft 0x083d54b0 winStoreWinPos].
    pub fn win_store_win_pos(&mut self, g: P<Grafport>, slot: i32) {
        let at = WIN_POS.addr().wrapping_add((slot as u32).wrapping_mul(8));
        if self.m.i16(at) == 0 {
            let w = self.m[g];
            for (k, v) in [w.left, w.right, w.bottom, w.top].into_iter().enumerate() {
                self.m.set_i16(at.wrapping_add(2 * k as u32), v);
            }
        }
    }

    /// Give a window the rectangle saved in a slot and free the slot
    /// [soft 0x083d54f8 winGetWinPos].
    pub fn win_get_win_pos(&mut self, g: P<Grafport>, slot: i32) {
        let at = WIN_POS.addr().wrapping_add((slot as u32).wrapping_mul(8));
        let v: Vec<i16> = (0..4).map(|k| self.m.i16(at.wrapping_add(2 * k))).collect();
        let w = &mut self.m[g];
        w.left = v[0];
        w.right = v[1];
        w.bottom = v[2];
        w.top = v[3];
        for k in 0..4 {
            self.m.set_i16(at.wrapping_add(2 * k), 0);
        }
    }

    /// Expand a window over its neighbour to the side: it takes the full width of the four
    /// views [soft 0x083d555c windowExpandHoriz].
    ///
    /// Not for a zoomed window (which only gets the full width set). First everything in
    /// the way is undone: this window's own upward expansion, the side neighbour's
    /// expansion of any kind, the other two windows' upward expansions, and a zoom of any
    /// other window. Then the camera record notes the expansion (+0x18 of the window's
    /// entry for the current view type), the neighbour's picture is switched off and the
    /// neighbour hidden, the window's rectangle is saved (slot = identifier − 1000) and its
    /// left and right edges become the full extent.
    ///
    /// | Window | Side neighbour |
    /// |---|---|
    /// | 1000 lower left | 1002 lower right |
    /// | 1001 upper left | 1003 upper right |
    /// | 1002 lower right | 1000 |
    /// | 1003 upper right | 1001 |
    pub fn window_expand_horiz(&mut self, g: P<Grafport>) {
        let v = self.views();
        let w = |i: i32| v.add(i);
        'expand: {
            if self.m[g].zoomed != 0 {
                break 'expand;
            }
            let kind = self.camera_type(g) as i32;
            let index = self.o.views.SI_camIdConvert(&mut self.m, kind);
            if self.m[g].expand == 2 {
                self.si_window_size_set(g, 0);
            }
            // (side neighbour, the two others whose upward expansion is undone, the order
            // in which a zoomed window is looked for, the slot)
            let (side, others, zoom_order, slot): (i32, [i32; 2], [i32; 3], i32) = match self.m[g].id {
                1001 => (3, [2, 0], [0, 2, 3], 1),
                1000 => (2, [3, 1], [2, 1, 3], 0),
                1002 => (0, [1, 3], [0, 1, 3], 2),
                1003 => (1, [0, 2], [0, 2, 1], 3),
                _ => break 'expand,
            };
            if self.m[w(side)].expand != 0 {
                self.si_window_size_set(w(side), 0);
            }
            for o in others {
                if self.m[w(o)].expand == 2 {
                    self.si_window_size_set(w(o), 0);
                }
            }
            for z in zoom_order {
                if self.m[w(z)].zoomed == 1 {
                    self.si_un_zoom_window(w(z));
                    break;
                }
            }
            let entry = self.m[g].cameras.wrapping_add(0x18).wrapping_add((index as u32).wrapping_mul(0x8c));
            self.m.set_i32(entry, 1);
            if self.m[w(side)].id != -1 {
                self.window_toggle_off_rgb_wind(w(side));
            }
            self.m[w(side)].visible = 0;
            if self.camera_type(w(side)) == cam::DOPE_SHEET {
                self.m[DAT_0878FAA8] = 0;
            }
            self.m[g].expand = 1;
            self.win_store_win_pos(g, slot);
        }
        let border = self.m[WIN_BORDER];
        self.m[g].left = border[0];
        self.m[g].right = border[2];
        self.si_get_window_viewport(g);
        if self.camera_type(g) == cam::SPREADSHEET {
            self.o.views.SI_spdViewSizeNew(&mut self.m, g);
        }
    }

    /// Undo a sideways expansion [soft 0x083d59e4 windowUnExpandHoriz]: the camera record
    /// and the window note "not expanded"; the window gets its saved rectangle back; then,
    /// if the window is not zoomed, the side neighbour is shown again and its picture
    /// restored; if it is zoomed, the rectangle goes into the template as the place to
    /// return to instead.
    pub fn window_un_expand_horiz(&mut self, g: P<Grafport>) {
        let kind = self.camera_type(g) as i32;
        let index = self.o.views.SI_camIdConvert(&mut self.m, kind);
        let entry = self.m[g].cameras.wrapping_add(0x18).wrapping_add((index as u32).wrapping_mul(0x8c));
        self.m.set_i32(entry, 0);
        let id = self.m[g].id;
        self.m[g].expand = 0;
        let side = match id {
            1001 => Some((1, 3)),
            1000 => Some((0, 2)),
            1002 => Some((2, 0)),
            1003 => Some((3, 1)),
            _ => None,
        };
        if let Some((slot, side)) = side {
            self.win_get_win_pos(g, slot);
            let w = self.m[g];
            if w.zoomed == 0 {
                let neighbour = self.views().add(side);
                self.m[neighbour].visible = 1;
                self.restore_rgb_wind(neighbour);
            } else {
                let t = &mut self.m[SI_TEMPLATE];
                t.saved_left = w.left;
                t.saved_right = w.right;
                t.saved_bottom = w.bottom;
                t.saved_top = w.top;
            }
        }
        // For a zoomed window with any other identifier the original stores four words it
        // never set into the template's saved rectangle; here the rectangle is left alone.
        self.si_get_window_viewport(g);
        if self.camera_type(g) == cam::SPREADSHEET {
            self.o.views.SI_spdViewSizeNew(&mut self.m, g);
        }
    }

    /// Draw a window's rulers, the horizontal one first [soft 0x083d6b68 windowDrawRulers].
    pub fn window_draw_rulers(&mut self, g: P<Grafport>) -> P<Grafport> {
        let r = self.m[g].ruler_h;
        self.si_ruler_draw(r);
        let r = self.m[g].ruler_v;
        self.si_ruler_draw(r);
        g
    }

    /// Redraw the title-bar gadgets of every visible window
    /// [soft 0x083d7214 si_windowGadjetAll].
    pub fn si_window_gadjet_all(&mut self, how: i16) {
        let mut g = P::NULL;
        while self.si_window_get_next_visible(&mut g) != 0 {
            self.si_window_gadjet(g, -1, how);
        }
    }

    /// Write a window's display settings back into its record for the view type in use
    /// [soft 0x083d917c updateCurrentWindow]: the entry of the window's camera table whose
    /// camera is the current one gets the display mode (+0x1c), the word at +0x68 (+0x20),
    /// the rotoscopy block (+0x24), the expansion (+0x18) and the graphic properties
    /// (+0x88). Returns the entry's index. (The search has no end of its own: the current
    /// camera is always one of the table's.)
    pub fn update_current_window(&mut self, g: P<Grafport>) -> i32 {
        let w = self.m[g];
        let mut index = 0u32;
        while self.m.u32(w.cameras.wrapping_add(index.wrapping_mul(0x8c))) != w.camera {
            index += 1;
        }
        let entry = w.cameras.wrapping_add(index.wrapping_mul(0x8c));
        self.m.set_i32(entry + 0x1c, w.display);
        let entry = self.m[g].cameras.wrapping_add(index.wrapping_mul(0x8c));
        self.m.set_i32(entry + 0x20, w.d_068);
        let entry = self.m[g].cameras.wrapping_add(index.wrapping_mul(0x8c));
        for (k, word) in w.roto.iter().enumerate() {
            self.m.set_u32(entry + 0x24 + 4 * k as u32, *word);
        }
        let entry = self.m[g].cameras.wrapping_add(index.wrapping_mul(0x8c));
        self.m.set_i32(entry + 0x18, w.expand);
        let entry = self.m[g].cameras.wrapping_add(index.wrapping_mul(0x8c));
        self.m.set_u32(entry + 0x88, w.props);
        index as i32
    }

    /// Whether a visible window shows a shaded mode (1, 3 or 6)
    /// [soft 0x083db168 SI_isShadeViewVisible].
    pub fn si_is_shade_view_visible(&mut self) -> i32 {
        let mut g = P::NULL;
        while self.si_window_get_next_visible(&mut g) != 0 {
            if matches!(self.m[g].display, 1 | 3 | 6) {
                return 1;
            }
        }
        0
    }

    /// The next active window that is a function-curve view; null and 0 at the end
    /// [soft 0x083db264 isFcurveView].
    pub fn is_fcurve_view(&mut self, g: &mut P<Grafport>) -> i32 {
        loop {
            if self.o.dialogs.SI_windowGetNextActive(&mut self.m, InOut(g)) == 0 {
                *g = P::NULL;
                return 0;
            }
            if self.camera_type(*g) == cam::FCURVE {
                return 1;
            }
        }
    }

    /// A click on a view window's title [soft 0x083d3d54 windowTitleHandle].
    ///
    /// - **Left button**: the menu of view types. No choice, or the entry already current:
    ///   nothing. "Freeze" (entry 9) toggles the window's `enabled` word; the menu's current
    ///   entry is put back; event 0x20. Any other entry switches the window to that view
    ///   type (a frozen window is enabled first): event 0x1f, or 0x1e when the window's
    ///   expansion changed with the type.
    /// - **Middle button**: back to the view type shown before (the entry at +0x5c); a
    ///   frozen window is first unfrozen, and stays untouched if that fails.
    /// - **Right button**: freeze or unfreeze. Freezing remembers the display mode and the
    ///   word at +0x68 (by window) and switches the display to wire frame; unfreezing puts
    ///   them back. Event 0x1f.
    /// - Any other device clears the event.
    pub fn window_title_handle(&mut self, g: P<Grafport>, event: P<Event>) {
        if event.is_null() {
            return;
        }
        let slot = |ui: &Ui, table: P<[i32; 4]>| table.addr().wrapping_add((ui.m[g].id as i32 as u32).wrapping_mul(4)).wrapping_sub(4000);
        let kind;
        match self.m[event].second {
            0x66 => {
                if self.m[g].enabled == 0 {
                    self.change_window_all(g, 9);
                }
                if self.m[g].enabled == 0 {
                    self.m[event].kind = ev::NONE;
                    return;
                }
                let (expand, zoomed) = (self.m[g].expand, self.m[g].zoomed);
                if self.m[g].view_item != 9 {
                    let item = self.m[g].view_item as i32;
                    self.handle_changing_displays(g, item);
                }
                let item = self.m[g].view_item as i32;
                self.change_window_all(g, item);
                if zoomed == 0 && expand != self.m[g].expand {
                    self.m[event].kind = 0x1e;
                    return;
                }
                kind = 0x1f;
            }
            0x65 => {
                if self.m[g].enabled == 0 {
                    self.m[g].enabled = 1;
                    let display = self.m.i32(slot(self, FREEZE_DTYP));
                    if display != 0 {
                        let entry = self.o.views.SI_dispModeConvert(&mut self.m, g, display);
                        self.o.views.SI_dispModeToggle(&mut self.m, g, entry);
                    }
                    self.m[g].d_068 = self.m.i32(slot(self, FREEZE_LTYP));
                    self.m[g].enabled = 0;
                } else {
                    let w = self.m[g];
                    let at = slot(self, FREEZE_LTYP);
                    self.m.set_i32(at, w.d_068);
                    let at = slot(self, FREEZE_DTYP);
                    self.m.set_i32(at, w.display);
                    if self.m[g].display != 0 {
                        self.o.views.SI_dispModeToggle(&mut self.m, g, 0);
                    }
                }
                self.m[event].kind = 0x1f;
                self.change_window_all(g, 9);
                kind = 0x1f;
            }
            0x67 => {
                let menu = self.m[g].menu_view;
                let current = self.m.i16(menu.wrapping_add(0x1a));
                let item = self.o.menus.SI_popupList(&mut self.m, g, menu);
                if item == -1 {
                    self.m[event].kind = ev::NONE;
                    return;
                }
                let (expand, zoomed) = (self.m[g].expand, self.m[g].zoomed);
                if item == current as i32 {
                    return;
                }
                if item != 9 {
                    if self.m[g].enabled == 0 {
                        self.m[g].enabled = 1;
                        let entries = self.m.u32(self.m[g].menu_view.wrapping_add(0x20));
                        self.m.set_i16(entries.wrapping_add(0x1b4), 0);
                        if self.camera_type(g) == cam::DOPE_SHEET && self.si_window_cam_active(cam::DOPE_SHEET as i32).is_null() {
                            self.o.views.SI_dopeFlagsSet(&mut self.m, 1, 1, 1, 1);
                        }
                    }
                    self.handle_changing_displays(g, item);
                }
                self.change_window_all(g, item);
                if item == 9 {
                    let menu = self.m[g].menu_view;
                    self.m.set_i16(menu.wrapping_add(0x1a), current);
                    let menu = self.m[g].menu_view;
                    self.o.menus.SI_setMenuFlag(&mut self.m, menu, current as i32, 1);
                    kind = 0x20;
                } else if zoomed == 0 && expand != self.m[g].expand {
                    self.m[event].kind = 0x1e;
                    return;
                } else {
                    kind = 0x1f;
                }
            }
            _ => {
                self.m[event].kind = ev::NONE;
                return;
            }
        }
        let id = self.m[g].id;
        let e = &mut self.m[event];
        e.kind = kind;
        e.first = id;
        e.second = -1;
        e.x = 0;
        e.y = 0;
        e.data = g.addr();
    }

    /// Make entry `item` of the camera table what the window shows
    /// [soft 0x083d4180 SI_setCurrentCam]: the window's name becomes a copy of the view-type menu's label for
    /// the entry; the entry shown so far is remembered at +0x5c (as `SI_camIdConvert` of its
    /// type; 6 when there was none); the entry's camera, grid, rulers, stored cameras,
    /// camera buttons, display mode, the word for +0x68, rotoscopy block, expansion and
    /// graphic properties are copied into the window.
    pub fn si_set_current_cam(&mut self, g: P<Grafport>, item: i32) {
        if self.m[g].name != 0 {
            let name = self.m[g].name;
            self.o.mem.free(&mut self.m, name);
            self.m[g].name = 0;
        }
        let entries = self.m.u32(self.m[g].menu_view.wrapping_add(0x20));
        let label = self.m.u32(entries.wrapping_add(8).wrapping_add((item as u32).wrapping_mul(0x30)));
        self.m[g].name = self.o.mem.strdup(&mut self.m, label);
        if self.m[g].camera == 0 {
            self.m[g].view_item = 6;
        } else {
            let kind = self.camera_type(g) as i32;
            self.m[g].view_item = self.o.views.SI_camIdConvert(&mut self.m, kind) as i16;
        }
        let entry = self.m[g].cameras.wrapping_add((item as u32).wrapping_mul(0x8c));
        let word = |ui: &Ui, at: u32| ui.m.u32(entry.wrapping_add(at));
        self.m[g].camera = word(self, 0);
        self.m[g].d_034 = word(self, 4);
        self.m[g].ruler_v = word(self, 8);
        self.m[g].ruler_h = word(self, 0xc);
        self.m[g].memo = word(self, 0x14);
        self.m[g].ctrl = word(self, 0x10);
        self.m[g].display = word(self, 0x1c) as i32;
        self.m[g].d_068 = word(self, 0x20) as i32;
        for k in 0..24 {
            self.m[g].roto[k] = word(self, 0x24 + 4 * k as u32);
        }
        let entry = self.m[g].cameras.wrapping_add((item as u32).wrapping_mul(0x8c));
        self.m[g].expand = self.m.i32(entry.wrapping_add(0x18));
        let entry = self.m[g].cameras.wrapping_add((item as u32).wrapping_mul(0x8c));
        self.m[g].props = self.m.u32(entry.wrapping_add(0x88));
    }

    /// Grey the entries of a window's view-type menu that its display mode excludes
    /// [soft 0x083d4268 SI_windowGhostMenu]: in a shaded or other special display mode (1,
    /// 3 to 6) the entries Fcurve (5), Schematic (6) and DopeSheet (7) are greyed; in wire
    /// frame (0) Schematic is released, and Fcurve and DopeSheet too if the static flag set
    /// on the first call is still set (it always is). Mode 2 and anything else: nothing.
    pub fn si_window_ghost_menu(&mut self, g: P<Grafport>) {
        let menu = self.m[g].menu_view;
        if self.m[FPASS] != 0 {
            self.m[MOTION_FLAG] = 1;
            self.m[FPASS] = 0;
        }
        let grey = |ui: &mut Ui, item: u32, on: i16| {
            let entries = ui.m.u32(menu.wrapping_add(0x20));
            ui.m.set_i16(entries.wrapping_add(item * 0x30 + 0xc), on);
        };
        match self.m[g].display {
            0 => {
                if self.m[MOTION_FLAG] != 0 {
                    grey(self, 5, 0);
                    grey(self, 7, 0);
                }
                grey(self, 6, 0);
            }
            1 | 3..=6 => {
                grey(self, 5, 1);
                grey(self, 6, 1);
                grey(self, 7, 1);
            }
            _ => {}
        }
    }

    /// Switch one window's picture off before its shape changes
    /// [soft 0x083d433c windowToggleOffRgbWind]. Only for a visible window with a picture window of its own
    /// (+0x50 not −1). The display mode is remembered in `toggleRgb` (by identifier − 1000).
    /// Wire frame: nothing more. Rotoscope modes 2 and 3: the display is closed and the
    /// window's inside painted in the window-background colour in the front buffer (mode 3
    /// first resets the OpenGL state of the picture window). Any other mode: the display is
    /// toggled to wire frame, keeping the word at +0x68.
    pub fn window_toggle_off_rgb_wind(&mut self, g: P<Grafport>) {
        let w = self.m[g];
        if w.visible == 0 || w.d_050 == -1 {
            return;
        }
        let mode = w.display;
        let at = TOGGLE_RGB.addr().wrapping_add((w.id as i32 as u32).wrapping_mul(4)).wrapping_sub(4000);
        self.m.set_i32(at, mode);
        if mode == 0 {
            return;
        }
        if mode != 2 && mode != 3 {
            let saved = self.m[g].d_068;
            self.o.views.SI_dispModeToggle(&mut self.m, g, 0);
            self.m[g].d_068 = saved;
            return;
        }
        if mode == 3 {
            self.o.gl.winset(&mut self.m, w.d_050);
            self.o.views.SI_txtDisableDirtyReset(&mut self.m, 1);
            let props = self.m[g].props;
            let dict = self.current_dictionary_of_scene();
            self.o.views.SI_resetGL(&mut self.m, dict, props);
            self.o.views.SI_txtDisableDirtyReset(&mut self.m, 0);
        }
        self.o.views.SI_displayModeClose(&mut self.m, g);
        if self.m[SI_NO_OVERLAY_REFRESH_FIX] != 1 {
            self.o.gl.frontbuffer(&mut self.m, 1);
        }
        let c = self.m[SI_WINDOW_BACK];
        self.o.gl.color(&mut self.m, c as u16);
        let w = self.m[g];
        let title = self.m[SI_TITLE_HEIGHT] as i32;
        self.sboxfi(w.left as i32 + 1, w.bottom as i32 + 1, w.right as i32 - 1, w.top as i32 - title);
        self.o.gl.frontbuffer(&mut self.m, 0);
    }

    /// The element dictionary of the current scene: `SI_elemDicts + 0x70 × *SI_procData`.
    pub(crate) fn current_dictionary_of_scene(&self) -> u32 {
        let index = self.m.u32(self.m[SI_PROC_DATA]);
        self.m[SI_ELEM_DICTS].wrapping_add(index.wrapping_mul(0x70))
    }

    /// Bring one window's picture back in the display mode `toggleRgb` remembered
    /// [soft 0x083d4a34 restoreRgbWind]. Only for a visible window. Modes 1 and 6: the
    /// viewport is prepared, the display toggled back to the mode (keeping +0x68) and the
    /// picture refreshed. Mode 2: in the window's own picture window if it has one, the
    /// inside is cleared, the rotoscope viewport set, the display opened and the picture
    /// placed. Mode 3: the same with the shaded overlay and a refresh. Modes 4, 5 and
    /// others: prepared and toggled back. The remembered mode is cleared.
    pub fn restore_rgb_wind(&mut self, g: P<Grafport>) {
        let current = self.o.gl.winget(&mut self.m);
        if self.m[g].visible == 0 {
            return;
        }
        let slot = |ui: &Ui| TOGGLE_RGB.addr().wrapping_add((ui.m[g].id as i32 as u32).wrapping_mul(4)).wrapping_sub(4000);
        let mode = self.m.i32(slot(self));
        let toggle_back = |ui: &mut Ui| {
            ui.o.views.SI_prepareViewport(&mut ui.m, g);
            let saved = ui.m[g].d_068;
            let mode = ui.m.i32(slot(ui));
            let entry = ui.o.views.SI_dispModeConvert(&mut ui.m, g, mode);
            ui.o.views.SI_dispModeToggle(&mut ui.m, g, entry);
            ui.m[g].d_068 = saved;
        };
        match mode {
            0 => {}
            1 | 6 => {
                toggle_back(self);
                self.o.views.SI_rgbRefreshOne(&mut self.m, g, 1);
            }
            2 => {
                let own = self.m[g].d_050;
                if current != own && own != -1 {
                    self.o.gl.winset(&mut self.m, own);
                }
                self.si_roto_viewport_clr(g);
                self.o.views.SI_dovSetViewport(&mut self.m, g);
                self.m[g].roto[5] = self.m[g].roto[5].wrapping_sub(1);
                self.o.views.SI_dispModeWindOpen(&mut self.m, g, 4);
                self.o.views.SI_dovPictureOffset(&mut self.m, g);
                if current != self.m[g].d_050 {
                    self.o.gl.winset(&mut self.m, current);
                }
            }
            3 => {
                self.si_roto_viewport_clr(g);
                self.o.views.SI_dovSetViewport(&mut self.m, g);
                self.o.views.SI_dispModeWindOpen(&mut self.m, g, 4);
                self.o.views.SI_drawOver2ShadeDo(&mut self.m, g);
                self.m[g].roto[5] = self.m[g].roto[5].wrapping_sub(1);
                self.o.views.SI_dovPictureOffset(&mut self.m, g);
                self.o.views.SI_rgbRefreshOne(&mut self.m, g, 1);
            }
            _ => toggle_back(self),
        }
        let at = slot(self);
        self.m.set_i32(at, 0);
    }

    /// Switch a window to entry `item` of its view-type menu
    /// [soft 0x083d5248 changeWindow]. Entry 9 ("Freeze") only toggles `enabled` (a dope sheet coming back
    /// when no other is active sets the dope-sheet flags). Entries 0 to 8: the camera-table
    /// entry becomes current (`SI_setCurrentCam`), the window's expansion is applied again,
    /// the window's object list becomes the scene's (for a function-curve view: the index
    /// of the scene's list among the lists), and the drawing area is recomputed and
    /// prepared as the view type needs (a spreadsheet is filled again). A window that was a
    /// dope sheet frees the dope sheet when it was the last; a window in a rotoscope mode
    /// gets its picture placed and is refreshed.
    pub fn change_window(&mut self, g: P<Grafport>, item: i32) {
        if (item as u32) > 9 {
            return;
        }
        let was_dope = self.camera_type(g) == cam::DOPE_SHEET;
        let dope = self.si_window_cam_active(cam::DOPE_SHEET as i32);
        if item == 9 {
            let was = self.m[g].enabled;
            self.m[g].enabled = (was == 0) as i16;
            if was_dope && was == 0 && dope.is_null() {
                self.o.views.SI_dopeFlagsSet(&mut self.m, 1, 1, 1, 1);
            }
            return;
        }
        self.si_set_current_cam(g, item);
        let entry = self.m[g].cameras.wrapping_add(0x18).wrapping_add((item as u32).wrapping_mul(0x8c));
        let expand = self.m[g].expand;
        self.m.set_i32(entry, expand);
        let expand = self.m[g].expand;
        self.si_window_size_set(g, expand);
        let lists = self.m[SI_GRAF_LISTS];
        self.m[g].d_03c = self.m.i32(self.m[SI_PROC_DATA].wrapping_add(4)).wrapping_mul(4) >> 2;
        match self.camera_type(g) {
            cam::FCURVE => {
                // the scene's list as an index: the byte distance divided by 36
                self.m[g].d_03c = (self.m[SI_SCNGL].wrapping_sub(lists) as i32).wrapping_mul(0x38e3_8e39) >> 2;
                self.si_get_window_viewport(g);
            }
            cam::PERSPECTIVE => self.si_get_window_viewport(g),
            cam::DOPE_SHEET => {
                self.m[DAT_087FB9C8] = 1;
                self.si_get_window_viewport(g);
                self.o.views.SI_prepareViewport(&mut self.m, g);
                if dope.is_null() {
                    self.o.views.SI_dopeFlagsSet(&mut self.m, 1, 1, 1, 1);
                }
            }
            cam::SPREADSHEET => {
                self.si_get_window_viewport(g);
                self.o.views.SI_prepareViewport(&mut self.m, g);
                self.o.views.SI_spdViewSizeNew(&mut self.m, g);
                let dict = self.current_dictionary_of_scene();
                self.o.views.SI_sprdExec(&mut self.m, g, dict, 1);
            }
            _ => {
                self.si_get_window_viewport(g);
                self.o.views.SI_prepareViewport(&mut self.m, g);
            }
        }
        if was_dope && self.si_window_cam_active(cam::DOPE_SHEET as i32).is_null() {
            self.o.views.SI_dopeSheetFree(&mut self.m);
        }
        if matches!(self.m[g].display, 2 | 3) {
            self.o.views.SI_dovPictSetPos(&mut self.m, g);
            self.o.views.SI_prepareViewport(&mut self.m, g);
            self.o.views.SI_dovSetViewport(&mut self.m, g);
            self.o.views.SI_refreshSystem(&mut self.m, g, 0, 7);
        }
    }

    /// Prepare a window for showing entry `item` of its camera table, before
    /// `changeWindow` makes the switch [soft 0x083d9208 handleChangingDisplays]: the current
    /// settings are written back to the table; if the new entry was left expanded
    /// differently (and the window is not zoomed) the window takes that expansion now, with
    /// the new camera's field of view in place for the moment; then the display is toggled
    /// to the new entry's mode with the new entry's rotoscopy block, camera and rulers
    /// already in the window but the old view type in the camera record, and the old
    /// camera pointer is put back.
    pub fn handle_changing_displays(&mut self, g: P<Grafport>, item: i32) {
        let at = (item as u32).wrapping_mul(0x8c);
        let camera = self.m[g].camera;
        let entry = |ui: &Ui| ui.m[g].cameras.wrapping_add(at);
        let type_now = self.m.i16(camera.wrapping_add(off::CAMERA_TYPE));
        let type_new = self.m.i16(self.m.u32(entry(self)).wrapping_add(off::CAMERA_TYPE));
        let grid_word = self.m.i16(self.m[g].d_034.wrapping_add(4));
        let display_new = self.m.i32(entry(self).wrapping_add(0x1c));
        let (expand, zoomed) = (self.m[g].expand, self.m[g].zoomed);
        let fov = self.m.u32(camera.wrapping_add(0x40));
        let index = self.update_current_window(g);
        let new_fov = self.m.u32(self.m.u32(entry(self)).wrapping_add(0x40));
        let cam_now = self.m[g].camera;
        self.m.set_u32(cam_now.wrapping_add(0x40), new_fov);
        if zoomed == 0 {
            let new_expand = self.m.i32(entry(self).wrapping_add(0x18));
            if expand != new_expand {
                self.si_window_size_set(g, new_expand);
                let old_entry = self.m[g].cameras.wrapping_add(0x18).wrapping_add((index as u32).wrapping_mul(0x8c));
                self.m.set_i32(old_entry, expand);
            }
        }
        let cam_now = self.m[g].camera;
        self.m.set_u32(cam_now.wrapping_add(0x40), fov);
        for k in 0..24 {
            self.m[g].roto[k] = self.m.u32(entry(self).wrapping_add(0x24 + 4 * k as u32));
        }
        let e = entry(self);
        self.m[g].camera = self.m.u32(e);
        self.m[g].ruler_h = self.m.u32(e.wrapping_add(0xc));
        self.m[g].ruler_v = self.m.u32(e.wrapping_add(8));
        let grid = self.m[g].d_034;
        self.m.set_i16(grid.wrapping_add(4), 0);
        self.si_get_window_viewport(g);
        let cam_now = self.m[g].camera;
        self.m.set_i16(cam_now.wrapping_add(off::CAMERA_TYPE), type_now);
        let converted = self.o.views.SI_dispModeConvert(&mut self.m, g, display_new);
        self.o.views.SI_dispModeToggle(&mut self.m, g, converted);
        let cam_now = self.m[g].camera;
        self.m.set_i16(cam_now.wrapping_add(off::CAMERA_TYPE), type_new);
        self.m[g].camera = camera;
        let grid = self.m[g].d_034;
        self.m.set_i16(grid.wrapping_add(4), grid_word);
    }
}
