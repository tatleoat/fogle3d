// SPDX-License-Identifier: MIT
//! `window.c`: zooming a view window, expanding it upwards, and the limits of what a view
//! shows. These are the routines of the file with x87 arithmetic; where a value is rounded
//! to single precision (stored and reloaded) is taken from the instruction listing and
//! said at each expression.

use crate::globals::*;
use crate::mem::P;
use crate::records::{cam, off, Grafport};
use crate::{fault, Ui};
use si_x87::{fmov, F80};

impl Ui {
    /// The entry of the window's rotoscope pictures that is in use, if the window has a
    /// table of them and the entry's picture is loaded (its pointer at +0x10).
    fn roto_picture(&self, g: P<Grafport>) -> Option<u32> {
        let table = self.m.u32(g.addr() + off::GRAF_ROTO_PICTURES);
        if table == 0 {
            return None;
        }
        let index = self.m.u32(g.addr() + off::GRAF_ROTO_INDEX);
        let entry = table.wrapping_add(index.wrapping_mul(0x14));
        (self.m.i32(entry.wrapping_add(0x10)) != 0).then_some(entry)
    }

    /// The field of view of a camera that looks through a rotoscope picture, after the
    /// drawing area changed: the default for the picture (`SI_dovGetDefaultFovCam` stores
    /// it in the camera) times the drawing area's height over the picture's, divided by
    /// the camera's float at +0x50. The ratio picture height / area height is rounded to
    /// single precision before the call; its reciprocal, the product and the quotient are
    /// in the 80-bit registers, rounded once when stored.
    fn roto_fov(&mut self, g: P<Grafport>, camera: u32) {
        let table = self.m.u32(g.addr() + off::GRAF_ROTO_PICTURES);
        let index = self.m.u32(g.addr() + off::GRAF_ROTO_INDEX);
        let height = self.m.i16(table.wrapping_add(index.wrapping_mul(0x14)).wrapping_add(6));
        let height = F80::from_i32(height as i32).to_f32();
        let w = self.m[g];
        let ratio = fault!((height / F80::from_i32(w.view_top as i32 - w.view_bottom as i32)).to_f32(), height / (w.view_top as i32 - w.view_bottom as i32) as f32);
        self.o.views.SI_dovGetDefaultFovCam(&mut self.m, g, camera.wrapping_add(off::CAMERA_FOV));
        let camera = self.m[g].camera;
        let fov = (F80::ONE / ratio) * self.m.f32(camera.wrapping_add(off::CAMERA_FOV));
        let fov = (fov / self.m.f32(camera.wrapping_add(off::CAMERA_50))).to_f32();
        self.m.set_f32(camera.wrapping_add(off::CAMERA_FOV), fov);
    }

    /// Give a window the whole area of the views and hide the other three
    /// [soft 0x083d4ce8 SI_windowZoomWin]. With a loaded rotoscope picture the picture's
    /// viewport is set again and either its offset reset and the inside repainted (the
    /// camera's word at +0x44 is zero) or, for a view that is not the perspective one, the
    /// field of view fitted to the new height. A hidden dope sheet clears the dope sheet's
    /// "shown" word.
    pub fn si_window_zoom_win(&mut self, g: P<Grafport>) {
        self.window_small_to_big(g);
        if self.roto_picture(g).is_some() {
            self.si_get_window_viewport(g);
            self.o.views.SI_dovSetViewport(&mut self.m, g);
            let camera = self.m[g].camera;
            if self.m.i16(camera.wrapping_add(off::CAMERA_44)) == 0 {
                self.o.views.SI_dovOffsetReset(&mut self.m, g);
                self.si_roto_viewport_clr(g);
            } else if self.m.i16(camera.wrapping_add(off::CAMERA_TYPE)) != cam::PERSPECTIVE {
                self.roto_fov(g, camera);
            }
        } else {
            self.si_get_window_viewport(g);
        }
        for i in 0..4 {
            let w = self.m[SI_TEMPLATE].views.add(i);
            if w != g {
                self.m[w].visible = 0;
                let w = self.m[SI_TEMPLATE].views.add(i);
                if self.camera_type(w) == cam::DOPE_SHEET {
                    self.m[DAT_0878FAA8] = 0;
                }
            }
        }
    }

    /// Paint the area of the views in the view-background colour, all planes.
    fn clear_views_area(&mut self, mask: i32) {
        let whole = self.m[SI_WHOLE_SYSTEM_WM];
        self.o.gl.writemask(&mut self.m, whole as u16);
        let c = self.m[SI_VIEW_BACK];
        self.o.gl.color(&mut self.m, c as u16);
        let t = self.m[SI_TEMPLATE];
        self.sboxfi(t.views_left as i32, t.views_bottom as i32, t.views_right as i32, t.views_top as i32);
        self.o.gl.writemask(&mut self.m, mask as u16);
    }

    /// Put a zoomed window back with its redraw [soft 0x083d4e08 SI_unZoomWindow]: the
    /// pictures off, the views' area cleared, the windows put back and drawn
    /// (`SI_unZoomWindowWin`), the buffers swapped, the area cleared and every window drawn
    /// again, the pictures back; a spreadsheet is told its new size.
    ///
    /// A window in a rotoscope display mode (2 or 3) gets its inside repainted and its
    /// camera's aspect ratio rebuilt: |(width + 1) / (height + 1)| of the drawing area
    /// times the camera's float at +0x68, stored; then times the quotient of the height's
    /// and the width's ratios to the second copy of the drawing area (1 when the rotoscopy
    /// mode word is 2), stored again. All quotients are in the 80-bit registers.
    pub fn si_un_zoom_window(&mut self, g: P<Grafport>) {
        let n = self.m[SI_TEMPLATE].n_views as i32;
        self.window_toggle_off_rgb();
        let mask = self.o.gl.getwritemask(&mut self.m);
        self.clear_views_area(mask);
        self.si_un_zoom_window_win(g);
        self.o.gl.swapbuffers(&mut self.m);
        self.clear_views_area(mask);
        for i in 0..n.max(0) {
            let w = self.m[SI_TEMPLATE].views.add(i);
            self.si_window_draw(w);
        }
        let display = self.m[g].display;
        if display == 2 || display == 3 {
            self.si_roto_viewport_clr(g);
            let (mut across, mut up) = (F80::ONE, F80::ONE);
            let w = self.m[g];
            if self.m.i16(g.addr() + off::GRAF_ROTO_A0) != 2 {
                across = F80::from_i32(w.view_right as i32 - w.view_left as i32) / F80::from_i32(w.view2[1] as i32 - w.view2[0] as i32);
                up = F80::from_i32(w.view_top as i32 - w.view_bottom as i32) / F80::from_i32(w.view2[3] as i32 - w.view2[2] as i32);
            }
            let shape = F80::from_i32(w.view_right as i32 - w.view_left as i32 + 1) / F80::from_i32(w.view_top as i32 - w.view_bottom as i32 + 1);
            let correction = fault!(up / across, across / up);
            let camera = self.m[g].camera;
            let aspect = (shape.abs() * self.m.f32(camera.wrapping_add(off::CAMERA_68))).to_f32();
            self.m.set_f32(camera.wrapping_add(off::CAMERA_ASPECT), aspect);
            let camera = self.m[g].camera;
            let aspect = (correction * self.m.f32(camera.wrapping_add(off::CAMERA_ASPECT))).to_f32();
            self.m.set_f32(camera.wrapping_add(off::CAMERA_ASPECT), aspect);
        }
        self.restore_rgb();
        if self.camera_type(g) == cam::SPREADSHEET {
            self.o.views.SI_spdViewSizeNew(&mut self.m, g);
        }
    }

    /// Put a zoomed window back in its place and bring the layout back
    /// [soft 0x083d500c SI_unZoomWindowWin]: the window gets the rectangle the template
    /// saved; with a loaded rotoscope picture the picture is scaled again (when the
    /// rotoscopy words allow), its viewport set, the field of view fitted and the inside
    /// repainted; then every window is visible again, and each in turn is zoomed if it is
    /// marked zoomed, or expanded as it is marked, and drawn.
    pub fn si_un_zoom_window_win(&mut self, g: P<Grafport>) {
        let n = self.m[SI_TEMPLATE].n_views as i32;
        self.window_big_to_small(g);
        if let Some(entry) = self.roto_picture(g) {
            let a = g.addr();
            if self.m.i16(a + off::GRAF_ROTO_74) != 0 && self.m.i16(a + off::GRAF_ROTO_A0) != 0 && self.m.i32(a + off::GRAF_ROTO_B8) == 0 {
                let scale = fmov(self.m.f32(a + off::GRAF_ROTO_SCALE));
                self.o.views.SI_dovPictScale(&mut self.m, a + off::GRAF_ROTO_BUF, entry.wrapping_add(4), scale);
            }
            self.si_get_window_viewport(g);
            self.o.views.SI_dovSetViewport(&mut self.m, g);
            let camera = self.m[g].camera;
            if self.m.i16(camera.wrapping_add(off::CAMERA_44)) != 0 && self.m.i16(camera.wrapping_add(off::CAMERA_TYPE)) != cam::PERSPECTIVE {
                self.roto_fov(g, camera);
            }
            self.si_roto_viewport_clr(g);
        } else {
            self.si_get_window_viewport(g);
        }
        self.window_set_all_visible();
        for i in 0..n.max(0) {
            let w = self.m[SI_TEMPLATE].views.add(i);
            if self.m[w].zoomed != 0 {
                self.si_window_zoom_win(w);
            } else {
                match self.m[w].expand {
                    1 => self.window_expand_horiz(w),
                    2 => self.window_expand_vert(w),
                    _ => {}
                }
            }
            let w = self.m[SI_TEMPLATE].views.add(i);
            self.si_window_draw(w);
        }
    }

    /// After a window's height changed: the camera's field of view (for the views that are
    /// not perspective: the height shown) follows it, new height over old, both less the
    /// title bar. Not for the perspective view, and not in a rotoscope display mode whose
    /// camera's word at +0x44 is zero. The old height is a float; the quotient and the
    /// product are in the 80-bit registers.
    fn fov_follows_height(&mut self, g: P<Grafport>, old_height: f32) {
        let camera = self.m[g].camera;
        if self.m.i16(camera.wrapping_add(off::CAMERA_TYPE)) == cam::PERSPECTIVE {
            return;
        }
        let display = self.m[g].display;
        if display > 3 || display < 2 || self.m.i16(camera.wrapping_add(off::CAMERA_44)) != 0 {
            let camera = self.m[g].camera;
            let w = self.m[g];
            let height = F80::from_i32(w.top as i32 - w.bottom as i32 - self.m[SI_TITLE_HEIGHT] as i32);
            let quotient = fault!(height / old_height, F80::from_f32((height / old_height).to_f32()));
            let fov = (quotient * self.m.f32(camera.wrapping_add(off::CAMERA_FOV))).to_f32();
            self.m.set_f32(camera.wrapping_add(off::CAMERA_FOV), fov);
        }
    }

    /// Expand a window over its neighbour above or below: it takes the full height of the
    /// four views [soft 0x083d5bd0 windowExpandVert].
    ///
    /// Not for a zoomed window (which only gets the full height set). First everything in
    /// the way is undone: this window's own sideways expansion, the vertical neighbour's
    /// expansion of any kind, the other two windows' sideways expansions, and a zoom of any
    /// other window. Then the camera table notes the expansion (+0x18 of the entry of the
    /// view type shown), the neighbour's picture is switched off and the neighbour hidden,
    /// the window's rectangle is saved (slot = identifier − 1000) and its bottom and top
    /// become the full extent. The field of view follows the height.
    ///
    /// | Window | Vertical neighbour |
    /// |---|---|
    /// | 1000 lower left | 1001 upper left |
    /// | 1001 upper left | 1000 |
    /// | 1002 lower right | 1003 upper right |
    /// | 1003 upper right | 1002 |
    pub fn window_expand_vert(&mut self, g: P<Grafport>) {
        let before = self.m[g];
        let old_height = F80::from_i32(before.top as i32 - before.bottom as i32 - self.m[SI_TITLE_HEIGHT] as i32).to_f32();
        'expand: {
            if self.m[g].zoomed != 0 {
                break 'expand;
            }
            let camera = self.m[g].camera;
            let v = self.m[SI_TEMPLATE].views;
            let w = |i: i32| v.add(i);
            let kind = self.m.i16(camera.wrapping_add(off::CAMERA_TYPE)) as i32;
            let index = self.o.views.SI_camIdConvert(&mut self.m, kind);
            if self.m[g].expand == 1 {
                self.si_window_size_set(g, 0);
            }
            // (vertical neighbour, the two others whose sideways expansion is undone, the
            // order in which a zoomed window is looked for, the slot)
            let (side, others, zoom_order, slot): (i32, [i32; 2], [i32; 3], i32) = match self.m[g].id {
                1001 => (0, [2, 3], [0, 2, 3], 1),
                1000 => (1, [3, 2], [2, 1, 3], 0),
                1002 => (3, [1, 0], [0, 1, 3], 2),
                1003 => (2, [0, 1], [0, 2, 1], 3),
                _ => break 'expand,
            };
            if self.m[w(side)].expand != 0 {
                self.si_window_size_set(w(side), 0);
            }
            for o in others {
                if self.m[w(o)].expand == 1 {
                    self.si_window_size_set(w(o), 0);
                }
            }
            for z in zoom_order {
                if self.m[w(z)].zoomed == 1 {
                    self.si_un_zoom_window(w(z));
                    break;
                }
            }
            let entry = self.m[g].cameras.wrapping_add((index as u32).wrapping_mul(0x8c)).wrapping_add(0x18);
            self.m.set_i32(entry, 2);
            if self.m[w(side)].id != -1 {
                self.window_toggle_off_rgb_wind(w(side));
            }
            let neighbour_camera = self.m[w(side)].camera;
            self.m[w(side)].visible = 0;
            if self.m.i16(neighbour_camera.wrapping_add(off::CAMERA_TYPE)) == cam::DOPE_SHEET {
                self.m[DAT_0878FAA8] = 0;
            }
            self.m[g].expand = 2;
            self.win_store_win_pos(g, slot);
        }
        let border = self.m[WIN_BORDER];
        self.m[g].bottom = border[1];
        self.m[g].top = border[3];
        self.si_get_window_viewport(g);
        self.fov_follows_height(g, old_height);
        if self.camera_type(g) == cam::SPREADSHEET {
            self.o.views.SI_spdViewSizeNew(&mut self.m, g);
        }
    }

    /// Undo an upward expansion [soft 0x083d60b8 windowUnExpandVert]: the camera table and
    /// the window note "not expanded"; the window gets its saved rectangle back; then, if
    /// the window is not zoomed, the vertical neighbour is shown again and its picture
    /// restored; if it is zoomed, the rectangle goes into the template as the place to
    /// return to instead. The field of view follows the height.
    pub fn window_un_expand_vert(&mut self, g: P<Grafport>) {
        let kind = self.camera_type(g) as i32;
        let index = self.o.views.SI_camIdConvert(&mut self.m, kind);
        let before = self.m[g];
        let old_height = F80::from_i32(before.top as i32 - before.bottom as i32 - self.m[SI_TITLE_HEIGHT] as i32).to_f32();
        let entry = self.m[g].cameras.wrapping_add((index as u32).wrapping_mul(0x8c)).wrapping_add(0x18);
        self.m.set_i32(entry, 0);
        let id = self.m[g].id;
        self.m[g].expand = 0;
        let side = match id {
            1001 => Some((1, 0)),
            1000 => Some((0, 1)),
            1002 => Some((2, 3)),
            1003 => Some((3, 2)),
            _ => None,
        };
        if let Some((slot, side)) = side {
            self.win_get_win_pos(g, slot);
            let w = self.m[g];
            let mut save = w.zoomed != 0;
            if !save {
                let neighbour = self.m[SI_TEMPLATE].views.add(side);
                self.m[neighbour].visible = 1;
                let neighbour = self.m[SI_TEMPLATE].views.add(side);
                self.restore_rgb_wind(neighbour);
                save = self.m[g].zoomed != 0;
            }
            if save {
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
        self.fov_follows_height(g, old_height);
        if self.camera_type(g) == cam::SPREADSHEET {
            self.o.views.SI_spdViewSizeNew(&mut self.m, g);
        }
    }

    /// Keep one limit within what the program accepts: a magnitude above `max` becomes
    /// `max`, one below `min` (zero included) becomes `min`, with the value's sign
    /// (`copysign` of the C library); a NaN is left alone.
    fn clamp_limit(&mut self, at: u32, min: f32, max: f32) {
        let v = self.m.f32(at);
        let magnitude = F80::from_f32(v).abs();
        let bound = if magnitude.gt(F80::from_f32(max)) {
            max
        } else if magnitude.lt(F80::from_f32(min)) {
            min
        } else {
            return;
        };
        let clamped = F80::from_f64(f64::copysign(bound as f64, F80::from_f32(v).to_f64())).to_f32();
        self.m.set_f32(at, clamped);
    }

    /// The left and right limits of what a view shows, in the view's own coordinates, into
    /// two floats [soft 0x083d6910 SI_windowLimitsHoriz]. For the front, top,
    /// function-curve, schematic and dope-sheet views: centre (the camera's float at
    /// +0x10) − half the width, and that + the width, where the width is the height shown
    /// (+0x40) × the aspect ratio (+0x64). The right view takes its centre from +0x18 and
    /// gives the two the other way round. Other views leave the floats as they are. Both
    /// are then kept within the limits (see `clamp_limit`).
    ///
    /// The first limit is rounded to single precision before the width is added; the
    /// products are in the 80-bit registers.
    pub fn si_window_limits_horiz(&mut self, g: P<Grafport>, out: u32) {
        let camera = self.m[g].camera;
        let kind = self.m.u16(camera.wrapping_add(off::CAMERA_TYPE)).wrapping_sub(2000) as i16 as i32;
        let width = |ui: &Ui| F80::from_f32(ui.m.f32(camera.wrapping_add(off::CAMERA_FOV))) * ui.m.f32(camera.wrapping_add(off::CAMERA_ASPECT));
        match kind {
            0 | 1 | 4 | 7 | 8 => {
                let half = width(self) * 0.5f64;
                let unrounded = F80::from_f32(self.m.f32(camera.wrapping_add(off::CAMERA_10))) - half;
                let low = unrounded.to_f32();
                self.m.set_f32(out, low);
                let high = (fault!(F80::from_f32(low), unrounded) + width(self)).to_f32();
                self.m.set_f32(out.wrapping_add(4), high);
            }
            2 => {
                let half = width(self) * 0.5f64;
                let low = (F80::from_f32(self.m.f32(camera.wrapping_add(off::CAMERA_18))) - half).to_f32();
                self.m.set_f32(out, low);
                let high = (F80::from_f32(low) + width(self)).to_f32();
                self.m.set_f32(out.wrapping_add(4), low);
                self.m.set_f32(out, high);
            }
            _ => {}
        }
        let (min, max) = (f32::from_bits(self.m[MINFLOAT_420]), f32::from_bits(self.m[MAXFLOAT_421]));
        self.clamp_limit(out, min, max);
        self.clamp_limit(out.wrapping_add(4), min, max);
    }

    /// The lower and upper limits of what a view shows
    /// [soft 0x083d6a44 SI_windowLimitsVert]: centre (the camera's float at +0x14) − half the height shown
    /// (+0x40), and that + the height. The top view takes its centre from +0x18 and gives
    /// the two the other way round. Other views (perspective, free orthogonal, spreadsheet)
    /// leave the floats as they are. Both are then kept within the limits.
    pub fn si_window_limits_vert(&mut self, g: P<Grafport>, out: u32) {
        let camera = self.m[g].camera;
        let kind = self.m.u16(camera.wrapping_add(off::CAMERA_TYPE)).wrapping_sub(2000) as i16 as i32;
        let height = |ui: &Ui| ui.m.f32(camera.wrapping_add(off::CAMERA_FOV));
        match kind {
            0 | 2 | 4 | 7 | 8 => {
                let half = F80::from_f32(height(self)) * 0.5f64;
                let unrounded = F80::from_f32(self.m.f32(camera.wrapping_add(off::CAMERA_14))) - half;
                let low = unrounded.to_f32();
                self.m.set_f32(out, low);
                let high = (fault!(F80::from_f32(low), unrounded) + height(self)).to_f32();
                self.m.set_f32(out.wrapping_add(4), high);
            }
            1 => {
                let half = F80::from_f32(height(self)) * 0.5f64;
                let low = (F80::from_f32(self.m.f32(camera.wrapping_add(off::CAMERA_18))) - half).to_f32();
                self.m.set_f32(out, low);
                let high = (F80::from_f32(low) + height(self)).to_f32();
                self.m.set_f32(out.wrapping_add(4), low);
                self.m.set_f32(out, high);
            }
            _ => {}
        }
        let (min, max) = (f32::from_bits(self.m[MINFLOAT_425]), f32::from_bits(self.m[MAXFLOAT_426]));
        self.clamp_limit(out, min, max);
        self.clamp_limit(out.wrapping_add(4), min, max);
    }
}
