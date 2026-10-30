// SPDX-License-Identifier: MIT
//! `window.c`: reading the layout of the views from the resource file `rsrc/Windows.rsrc`
//! at start-up: the view windows, their camera tables, and per view type a camera, a grid
//! and two rulers. The file is text; it is read with the C library's `fscanf`, `fgetc` and
//! `fgets` through [`crate::file`].
//!
//! The file, as the code reads it (finding 0021, 3.1):
//!
//! ```text
//! # comment lines
//! <one line that is skipped>
//! <number of views>            rest of the line skipped
//! <number of view types>       rest of the line skipped
//! per view:   <name: one line> <identifier> <visible> <selected> <enabled>
//!             <left> <bottom> <right> <top>   four pop-up menus (`si_popupRead`)   rest of the line skipped
//! per type:   a camera: 15 lines "<label> <value…>" (name, type, three words, floats)
//!             a grid: 11 lines, the first with two labels
//!             two rulers: 4 lines each, the first with two labels
//! ```

use crate::globals::*;
use crate::mem::P;
use crate::records::{off, Gadget, Grafport};
use crate::{fault, Ui};

impl Ui {
    /// Skip the rest of the line: characters are read up to and with a line feed, or to
    /// the end of the file.
    fn skip_line(&mut self, stream: u32) {
        loop {
            if self.files.feof(&mut self.m, stream) != 0 {
                break;
            }
            let c = self.files.fgetc(&mut self.m, stream);
            if c as u8 == b'\n' {
                break;
            }
        }
    }

    /// Read the layout file if it can be read [soft 0x083d2104 SI_windowReadInit]: 0 when
    /// it was read (the template then takes over the first module's measures,
    /// `setTemplate`), −1 when its content was not accepted, `errno` when `access` refuses
    /// the file.
    pub fn si_window_read_init(&mut self, path: u32) -> i32 {
        if self.files.access(&mut self.m, path, 4) != 0 {
            return self.files.errno;
        }
        if self.window_read_file(path) as i16 == 0 {
            return -1;
        }
        self.set_template();
        0
    }

    /// Read the layout file [soft 0x083d215c windowReadFile]. 0 when the file cannot be
    /// opened or ends before the counts; else 1.
    ///
    /// After the comment lines and one more line: the number of views and the number of
    /// view types. The array of view windows is allocated in the template, and for every
    /// window its camera table: per view type a camera, a grid, two rulers, the camera
    /// buttons, four stored cameras and the graphic properties (`SI_calloc` each;
    /// `SI_GpropsInit`), the display word 1; entries 5 and 8 (function curves, spreadsheet)
    /// start expanded sideways. The window's rotoscopy word at +0xbc comes from item 0x18
    /// of dialog 1900 if that exists (else 1). The window's own lines are read
    /// (`windowRead`), its rotoscopy settings copied into every entry, and the window
    /// shows the entry of its own index (`SI_setCurrentCam`).
    ///
    /// Then per view type a camera, a grid and two rulers are read into scratch records
    /// and handed to the entry of that type (`SI_camIdConvert`) of every window
    /// (`SI_cameraTransfer` with a copy of the name, `SI_gridTransfer`, `si_rulerTransfer`
    /// with the window as the ruler's owner, `SI_memoCamInit`, `SI_ctrlCamInit`). Last the
    /// menus get their marks: the view type and the display mode of each window.
    ///
    /// Licence: with `SI_lightLicense` set the original greys the spreadsheet entry of the
    /// view-type menus (`SI_setMenuGhost`); this port takes the full product's path.
    pub fn window_read_file(&mut self, path: u32) -> i32 {
        let sp = self.m.sp();
        let mode = self.m.local(4);
        self.m.put_cstr(mode, b"r");
        let stream = self.files.fopen(&mut self.m, path, mode);
        self.m.set_sp(sp);
        if stream == 0 {
            return 0;
        }
        loop {
            if self.files.feof(&mut self.m, stream) != 0 {
                break;
            }
            let c = self.files.fgetc(&mut self.m, stream);
            if c as u8 != b'#' {
                self.files.ungetc(&mut self.m, c as i8 as i32, stream);
                break;
            }
            self.skip_line(stream);
        }
        self.skip_line(stream);
        if self.files.feof(&mut self.m, stream) != 0 {
            self.files.fclose(&mut self.m, stream);
            return 0;
        }
        // the two counts are locals that a failed conversion leaves as they were (in the
        // original: whatever its stack held; here zero)
        let counts = self.m.local(8);
        self.files.fscanf(&mut self.m, stream, b"%d", &[counts]);
        self.skip_line(stream);
        let n_views = self.m.u32(counts);
        self.m[SI_TEMPLATE].n_views = n_views as i16;
        let views = self.o.mem.SI_calloc(&mut self.m, n_views, 0x2b8);
        self.m[SI_TEMPLATE].views = P::at(views);
        self.files.fscanf(&mut self.m, stream, b"%d", &[counts + 4]);
        self.skip_line(stream);
        let n_types = self.m.u32(counts + 4);
        self.m.set_sp(sp);
        let window = |ui: &Ui, w: u32| P::<Grafport>::at(ui.m[SI_TEMPLATE].views.addr().wrapping_add(w.wrapping_mul(Grafport::SIZE)));
        let mut w = 0u32;
        while w < n_views {
            let g = window(self, w);
            let table = self.o.mem.SI_calloc(&mut self.m, n_types, 0x8c);
            self.m[g].cameras = table;
            let mut t = 0u32;
            while t < n_types {
                let entry = |ui: &Ui| ui.m[g].cameras.wrapping_add(t.wrapping_mul(0x8c));
                for (at, count, size) in [(0u32, 1u32, 0x13ci32), (4, 1, 0x30), (8, 1, 0x24), (0xc, 1, 0x24), (0x10, 1, 0x10), (0x14, 4, 0x34)] {
                    let block = self.o.mem.SI_calloc(&mut self.m, count, size);
                    let e = entry(self);
                    self.m.set_u32(e.wrapping_add(at), block);
                }
                let e = entry(self);
                self.m.set_i32(e.wrapping_add(0x18), 0);
                let e = entry(self);
                self.m.set_i32(e.wrapping_add(0x1c), 0);
                let e = entry(self);
                self.m.set_i32(e.wrapping_add(0x20), 1);
                let block = self.o.mem.SI_calloc(&mut self.m, 1, 0xa0);
                let e = entry(self);
                self.m.set_u32(e.wrapping_add(0x88), block);
                let props = self.m.u32(entry(self).wrapping_add(0x88));
                self.o.views.SI_GpropsInit(&mut self.m, props);
                t += 1;
            }
            // entries 5 and 8 start expanded sideways (whatever the number of entries)
            let table = self.m[g].cameras;
            self.m.set_i32(table.wrapping_add(5 * 0x8c + 0x18), 1);
            let table = self.m[g].cameras;
            self.m.set_i32(table.wrapping_add(8 * 0x8c + 0x18), 1);
            let g = window(self, w);
            let dialog = self.o.dialogs.SI_dialogGet(&mut self.m, 0x76c);
            if dialog == 0 {
                self.m.set_i32(g.addr() + 0xbc, 1);
            } else {
                let mut state = 0i16;
                self.o.dialogs.SI_getStateItem(&mut self.m, dialog, 0x18, &mut state);
                self.m.set_i32(g.addr() + 0xbc, state as i32);
            }
            self.window_read(stream, g);
            self.skip_line(stream);
            let mut t = 0u32;
            while t < n_types {
                for k in 0..24u32 {
                    let word = self.m.u32(g.addr() + 0x70 + 4 * k);
                    let entry = self.m[g].cameras.wrapping_add(t.wrapping_mul(0x8c));
                    self.m.set_u32(entry.wrapping_add(0x24 + 4 * k), word);
                }
                t += 1;
            }
            self.si_set_current_cam(g, w as i32);
            w += 1;
        }
        let camera = self.o.mem.SI_calloc(&mut self.m, 1, 0x13c);
        let grid = self.o.mem.SI_calloc(&mut self.m, 1, 0x30);
        let ruler1 = self.o.mem.SI_calloc(&mut self.m, 1, 0x24);
        let ruler2 = self.o.mem.SI_calloc(&mut self.m, 1, 0x24);
        let mut t = 0u32;
        while t < n_types {
            self.window_cam_read(stream, camera);
            self.window_grid_read(stream, grid);
            self.window_ruler_read(stream, ruler1);
            self.window_ruler_read(stream, ruler2);
            let kind = self.m.i16(camera.wrapping_add(off::CAMERA_TYPE)) as i32;
            let index = (self.o.views.SI_camIdConvert(&mut self.m, kind) as u32).wrapping_mul(0x8c);
            t += 1;
            let mut w = 0u32;
            while w < n_views {
                let g = window(self, w);
                let entry = |ui: &Ui| ui.m[g].cameras.wrapping_add(index);
                let to = self.m.u32(entry(self));
                self.o.views.SI_cameraTransfer(&mut self.m, camera, to);
                let name = self.m.u32(camera);
                let copy = self.o.mem.strdup(&mut self.m, name);
                let to = self.m.u32(entry(self));
                self.m.set_u32(to, copy);
                let to = self.m.u32(entry(self).wrapping_add(4));
                self.o.views.SI_gridTransfer(&mut self.m, grid, to);
                self.m.set_u32(ruler1, g.addr());
                let to = self.m.u32(entry(self).wrapping_add(0xc));
                self.si_ruler_transfer(ruler1, to);
                self.m.set_u32(ruler2, g.addr());
                let to = self.m.u32(entry(self).wrapping_add(8));
                self.si_ruler_transfer(ruler2, to);
                let e = entry(self);
                self.o.views.SI_memoCamInit(&mut self.m, e);
                let buttons = self.m.u32(entry(self).wrapping_add(0x10));
                self.o.views.SI_ctrlCamInit(&mut self.m, buttons);
                w += 1;
            }
        }
        let mut w = 0u32;
        while w < n_views {
            let g = window(self, w);
            let kind = self.camera_type(g) as i32;
            let index = self.o.views.SI_camIdConvert(&mut self.m, kind);
            let menu = self.m[g].menu_view;
            self.o.menus.SI_setMenuFlag(&mut self.m, menu, index, 1);
            // (SI_lightLicense: the full product's path, see above)
            let display = self.m[g].display;
            let index = self.o.views.SI_dispModeConvert(&mut self.m, g, display);
            let menu = self.m[g].menu_display;
            self.o.menus.SI_setMenuFlag(&mut self.m, menu, index, 1);
            w += 1;
        }
        let name = self.m.u32(camera);
        if name != 0 {
            self.o.mem.free(&mut self.m, name);
            self.m.set_u32(camera, 0);
        }
        for block in [camera, grid, ruler1, ruler2] {
            if block != 0 {
                self.o.mem.free(&mut self.m, block);
            }
        }
        self.files.fclose(&mut self.m, stream);
        1
    }

    /// Read a camera's 15 lines [soft 0x083d286c windowCamRead]: after `SI_resetCamera`,
    /// each line a label and values: the name (a new string at +0), the view type (+4),
    /// 16-bit words for +0xa8, +0xa and +0xc, floats for +0x40 (field of view), +0x64
    /// (aspect ratio), +0x34 and +0x38 (clipping planes), three at +0x10 and three at
    /// +0x1c, and +0x28, +0x2c, +0x30; the rest of the last line is skipped.
    ///
    /// The name is read into a local buffer of 28 bytes without a limit; a longer word
    /// runs over the original's stack. Here the buffer is given room for a line.
    pub fn window_cam_read(&mut self, stream: u32, camera: u32) {
        self.o.views.SI_resetCamera(&mut self.m, camera, 0);
        let sp = self.m.sp();
        let name = self.m.local(0x100);
        self.files.fscanf(&mut self.m, stream, b"%*s %s", &[name]);
        let copy = self.o.mem.strdup(&mut self.m, name);
        self.m.set_sp(sp);
        self.m.set_u32(camera, copy);
        for at in [4u32, 0xa8, 0xa, 0xc] {
            self.files.fscanf(&mut self.m, stream, b"%*s %hd", &[camera.wrapping_add(at)]);
        }
        for at in [0x40u32, 0x64, 0x34, 0x38] {
            self.files.fscanf(&mut self.m, stream, b"%*s %f", &[camera.wrapping_add(at)]);
        }
        for at in [0x10u32, 0x1c] {
            self.files.fscanf(&mut self.m, stream, b"%*s %f %f %f", &[camera.wrapping_add(at), camera.wrapping_add(at + 4), camera.wrapping_add(at + 8)]);
        }
        for at in [0x28u32, 0x2c, 0x30] {
            self.files.fscanf(&mut self.m, stream, b"%*s %f", &[camera.wrapping_add(at)]);
        }
        self.skip_line(stream);
    }

    /// Read a grid's 11 lines [soft 0x083d29ec windowGridRead]: 16-bit words for +0 (the
    /// line has two labels), +2, +4, +6, +8, +0xa, +0xc; three floats at +0x10 and three
    /// at +0x1c; a 32-bit word at +0x28; a 16-bit word at +0x2c; the rest of the last line
    /// is skipped.
    pub fn window_grid_read(&mut self, stream: u32, grid: u32) {
        self.files.fscanf(&mut self.m, stream, b"%*s %*s %hd", &[grid]);
        for at in [2u32, 4, 6, 8, 0xa, 0xc] {
            self.files.fscanf(&mut self.m, stream, b"%*s %hd", &[grid.wrapping_add(at)]);
        }
        for at in [0x10u32, 0x1c] {
            self.files.fscanf(&mut self.m, stream, b"%*s %f %f %f", &[grid.wrapping_add(at), grid.wrapping_add(at + 4), grid.wrapping_add(at + 8)]);
        }
        self.files.fscanf(&mut self.m, stream, b"%*s %d", &[grid.wrapping_add(0x28)]);
        self.files.fscanf(&mut self.m, stream, b"%*s %hd", &[grid.wrapping_add(0x2c)]);
        self.skip_line(stream);
    }

    /// Read a ruler's 4 lines [soft 0x083d2b0c windowRulerRead] (nothing for a null
    /// ruler): a 32-bit word at +4 (the line has two labels), "shown" at +0x20 (16 bits),
    /// the width at +0x10 and a word at +0x1c.
    pub fn window_ruler_read(&mut self, stream: u32, ruler: u32) {
        if ruler == 0 {
            return;
        }
        self.files.fscanf(&mut self.m, stream, b"%*s %*s %d", &[ruler.wrapping_add(4)]);
        self.files.fscanf(&mut self.m, stream, b"%*s %hd", &[ruler.wrapping_add(off::RULER_ACTIVE)]);
        self.files.fscanf(&mut self.m, stream, b"%*s %d", &[ruler.wrapping_add(off::RULER_WIDTH)]);
        self.files.fscanf(&mut self.m, stream, b"%*s %d", &[ruler.wrapping_add(0x1c)]);
    }

    /// Read one view window's lines and give the window its starting state
    /// [soft 0x083d2b74 windowRead]: the name (a line), identifier, visible, selected,
    /// enabled; the rectangle as left, bottom, right, top; four pop-up menus
    /// (`si_popupRead`: view types, display modes, schematic modes, spreadsheet queries),
    /// each placed by `si_windowPopupSetPos`; the window is placed (`windowSetGraf`) and
    /// its outer edges remembered (`winStoreBorder`). Then the defaults: wire frame, the
    /// "+0x68" word 1, no picture window (−1), +0x54 = 2, the entry to return to 6, rulers
    /// made if there are none, depth cue 1 with −1, −1, not zoomed, not expanded, no
    /// spreadsheet, rotoscopy and ghost defaults (`SI_initRotoscopy`, `SI_ghostSetDefault`).
    pub fn window_read(&mut self, stream: u32, g: P<Grafport>) {
        self.m[g].name = self.si_string_reading(stream);
        let a = g.addr();
        for at in [4u32, 6, 8, 0xa] {
            self.files.fscanf(&mut self.m, stream, b"%hd", &[a + at]);
        }
        let sp = self.m.sp();
        let rect = self.m.local(8);
        self.files.fscanf(&mut self.m, stream, b"%hd %hd %hd %hd", &[rect, rect + 2, rect + 4, rect + 6]);
        let (left, bottom, right, top) = (self.m.i16(rect), self.m.i16(rect + 2), self.m.i16(rect + 4), self.m.i16(rect + 6));
        self.m.set_sp(sp);
        self.m[g].menu_view = self.o.menus.si_popupRead(&mut self.m, stream, 0, 0, 5);
        self.m[g].menu_display = self.o.menus.si_popupRead(&mut self.m, stream, 0, 0, 5);
        self.m[g].menu_schematic = self.o.menus.si_popupRead(&mut self.m, stream, 0, 0, 5);
        self.m[g].menu_queries = self.o.menus.si_popupRead(&mut self.m, stream, 0, 0, 5);
        for menu in [self.m[g].menu_view, self.m[g].menu_display] {
            self.si_window_popup_set_pos(g, menu);
        }
        let menu = self.m[g].menu_schematic;
        self.si_window_popup_set_pos(g, menu);
        let menu = self.m[g].menu_queries;
        self.si_window_popup_set_pos(g, menu);
        self.window_set_graf(g, left, right, bottom, top);
        self.win_store_border(g);
        self.m[g].display = 0;
        self.m[g].d_068 = 1;
        self.m[g].d_050 = -1;
        self.m[g].d_054 = 2;
        self.m[g].view_item = 6;
        if self.m[g].ruler_v == 0 {
            self.m[g].ruler_v = self.si_ruler_create_vert(g);
        }
        if self.m[g].ruler_h == 0 {
            self.m[g].ruler_h = self.si_ruler_create_horiz(g);
        }
        self.m[g].d_278 = -1;
        self.m[g].d_27c = -1;
        self.m[g].d_274 = 1;
        self.m[g].w_288 = [0, 0, 0];
        self.m[g].d_044 = 0;
        self.m[g].zoomed = 0;
        self.m[g].expand = 0;
        self.m[g].d_03c = 0;
        self.m[g].spreadsheet = 0;
        self.m[g].d_040 = 0;
        self.o.views.SI_initRotoscopy(&mut self.m, g);
        self.m[g].w_06c = 0;
        self.m[g].fast = 0;
        self.m[g].d_26c = 0;
        self.o.views.SI_ghostSetDefault(&mut self.m, g);
        self.m[g].d_290 = 0;
    }

    /// Place a window's pop-up menu under its title bar
    /// [soft 0x083d8ad0 si_windowPopupSetPos]. A menu's 16-bit words: +2 its kind, +4 width, +6 height,
    /// +8 to +0xe its rectangle (left, right, bottom, top), +0x10 to +0x16 a second
    /// rectangle (the title's). By kind, in four classes (1, 5, 9, 13; 2, 6, 10, 14; 3,
    /// 7, 11, 15; 4, 8, 12, 16; any other kind: nothing):
    ///
    /// 1. the view-type menu: from 60 pixels right of the window's left edge;
    /// 2. the display-mode menu: ending 10 pixels left of the window's right edge;
    /// 3. the schematic mode menu: from the offset of its gadget (part 23);
    /// 4. the spreadsheet queries: from the offset of part 47.
    ///
    /// Each hangs from 10 pixels below the window's top down by its height; the width is
    /// set again from the rectangle; the second rectangle is left + 55 to left + 155 and
    /// from the menu's height + 30 below the top to 25 below it. All in 16 bits.
    pub fn si_window_popup_set_pos(&mut self, g: P<Grafport>, menu: u32) {
        let (left, top) = (self.m[g].left, self.m[g].top);
        let w = |ui: &Ui, at: u32| ui.m.i16(menu.wrapping_add(at));
        let kind = self.m.u16(menu.wrapping_add(2));
        if !(1..=0x10).contains(&kind) {
            return;
        }
        let (x1, x2) = match (kind - 1) % 4 {
            0 => {
                let x1 = left.wrapping_add(0x3c);
                (x1, x1.wrapping_add(w(self, 4)))
            }
            1 => {
                let right = self.m[g].right;
                (right.wrapping_sub(w(self, 4).wrapping_add(10)), right.wrapping_sub(10))
            }
            class => {
                let part = if class == 2 { 0x17 } else { 0x2f };
                let x1 = left.wrapping_add(self.get_gadget_offset(part) as i16);
                self.m.set_i16(menu.wrapping_add(8), x1);
                let x2 = (self.get_gadget_offset(part) as i16).wrapping_add(left).wrapping_add(w(self, 4));
                (x1, x2)
            }
        };
        self.m.set_i16(menu.wrapping_add(8), x1);
        self.m.set_i16(menu.wrapping_add(0xa), x2);
        let v = top.wrapping_sub(w(self, 6).wrapping_add(10));
        self.m.set_i16(menu.wrapping_add(0xc), v);
        self.m.set_i16(menu.wrapping_add(0xe), top.wrapping_sub(10));
        let v = x2.wrapping_sub(self.m.i16(menu.wrapping_add(8)));
        self.m.set_i16(menu.wrapping_add(4), v);
        self.m.set_i16(menu.wrapping_add(0x10), left.wrapping_add(0x37));
        self.m.set_i16(menu.wrapping_add(0x12), left.wrapping_add(fault!(0x9b, 0x9c)));
        let v = top.wrapping_sub(w(self, 6).wrapping_add(0x1e));
        self.m.set_i16(menu.wrapping_add(0x14), v);
        self.m.set_i16(menu.wrapping_add(0x16), top.wrapping_sub(0x19));
    }
}

#[allow(unused)]
fn _gadget(_: Gadget) {}
