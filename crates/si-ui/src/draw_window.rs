// SPDX-License-Identifier: MIT
//! `drawWindow.c` of the program (0x08392400): the "shader windows", extra IRIS GL windows
//! that another part of the program opens with a callback for their events, kept in a
//! growing list. (The file's name notwithstanding, the views' own drawing is in
//! `window.c`.) Two small C++ classes: `SimpleListPtrC`, a list of pointers, and
//! `ShaderWindowC`, one window.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. `this` is the first argument, as GCC 2.95 passes it. The
//! compiler's code for an exception thrown by `operator new` or a constructor (the pieces
//! of `SI_uiOpenWindow` that delete the half-made object and throw again) has no
//! counterpart here: this port's allocation calls do not throw.

use crate::globals::*;
use crate::mem::P;
use crate::records::{Event, ShaderWindow, SimpleList};
use crate::{fault, Ui};

impl Ui {
    /// `SimpleListPtrC::SimpleListPtrC(count, step)` [soft 0x08392794 __14SimpleListPtrCii]: a
    /// list of `count` null pointers that grows by `step`.
    pub fn simple_list_new(&mut self, this: P<SimpleList>, count: i32, step: i32) -> P<SimpleList> {
        self.m[this].count = count;
        self.m[this].step = step;
        self.m[this].items = self.o.mem.__builtin_vec_new(&mut self.m, (count as u32) << 2);
        let mut i = 0i32;
        while i < self.m[this].count {
            let at = self.m[this].items.wrapping_add((i as u32).wrapping_mul(4));
            self.m.set_u32(at, 0);
            i += 1;
        }
        this
    }

    /// `SimpleListPtrC::~SimpleListPtrC()` [soft 0x083927d8 _._14SimpleListPtrC]: the array is
    /// freed; with bit 0 of the hidden argument the list itself too.
    pub fn simple_list_delete(&mut self, this: P<SimpleList>, how: u32) {
        let items = self.m[this].items;
        if items != 0 {
            self.o.mem.__builtin_vec_delete(&mut self.m, items);
        }
        self.m[this].items = 0;
        if how & 1 != 0 {
            self.o.mem.__builtin_delete(&mut self.m, this.addr());
        }
    }

    /// `SimpleListPtrC::add(item)` [soft 0x08392814 add]: the item goes into the first
    /// free place; a full list is first copied into an array `step` longer. Returns the
    /// place.
    pub fn simple_list_add(&mut self, this: P<SimpleList>, item: u32) -> i32 {
        let item_at = |ui: &Ui, i: i32| ui.m[this].items.wrapping_add((i as u32).wrapping_mul(4));
        let mut i = 0i32;
        if self.m[this].count > 0 {
            let mut v = self.m.u32(item_at(self, 0));
            while v != 0 {
                i += 1;
                if i >= self.m[this].count {
                    break;
                }
                v = self.m.u32(item_at(self, i));
            }
        }
        if i == self.m[this].count {
            let n = i.wrapping_add(self.m[this].step);
            let grown = self.o.mem.__builtin_vec_new(&mut self.m, (n as u32).wrapping_mul(4));
            let old = self.m[this].items;
            for k in 0..i.max(0) as u32 {
                let v = self.m.u32(old.wrapping_add(4 * k));
                self.m.set_u32(grown.wrapping_add(4 * k), v);
            }
            let mut k = i;
            while k < n {
                self.m.set_u32(grown.wrapping_add((k as u32).wrapping_mul(4)), 0);
                k += 1;
            }
            if self.m[this].items != 0 {
                let old = self.m[this].items;
                self.o.mem.__builtin_vec_delete(&mut self.m, old);
            }
            self.m[this].items = grown;
            self.m[this].count = n;
        }
        let at = item_at(self, i);
        self.m.set_u32(at, item);
        i
    }

    /// `SimpleListPtrC::remove(place)` [soft 0x083928c0 remove]: the item is freed and its
    /// place cleared.
    pub fn simple_list_remove(&mut self, this: P<SimpleList>, place: i32) {
        let at = self.m[this].items.wrapping_add((place as u32).wrapping_mul(4));
        let item = self.m.u32(at);
        if item != 0 {
            self.o.mem.__builtin_delete(&mut self.m, item);
            let at = self.m[this].items.wrapping_add((place as u32).wrapping_mul(4));
            self.m.set_u32(at, 0);
        }
    }

    /// `ShaderWindowC::ShaderWindowC(name, x, y, &width, &height, data, callback)`
    /// [soft 0x083928f4 __13ShaderWindowCPciiPiT4PvPFP8SI_EventlPv_v]: the record is filled (the name cut to 64 bytes),
    /// an IRIS GL window of that place and size is opened in the foreground, its real size
    /// is asked for and stored (also through the two pointers), and the window is put in
    /// RGB mode, on top, and cleared.
    #[allow(clippy::too_many_arguments)]
    pub fn shader_window_new(&mut self, this: P<ShaderWindow>, name: u32, x: i32, y: i32, width: u32, height: u32, data: u32, callback: u32) -> P<ShaderWindow> {
        si_mem::clib::strncpy(&mut self.m, this.addr(), name, 0x40);
        self.m[this].x = x;
        self.m[this].y = y;
        let w = self.m.i32(width);
        self.m[this].width = w;
        let h = self.m.i32(height);
        self.m[this].data = data;
        self.m[this].height = h;
        self.m[this].callback = callback;
        self.m[this].d_054 = 0;
        self.o.gl.prefposition(&mut self.m, x, w.wrapping_sub(1).wrapping_add(x), y, h.wrapping_sub(1).wrapping_add(y));
        self.o.gl.foreground(&mut self.m);
        let title = self.m.cstr(name);
        self.m[this].winid = self.o.gl.winopen(&mut self.m, &title);
        self.o.gl.getsize(&mut self.m, width, height);
        self.m[this].width = self.m.i32(width);
        self.m[this].height = self.m.i32(height);
        self.o.gl.RGBmode(&mut self.m);
        self.o.gl.gconfig(&mut self.m);
        self.o.gl.winpop(&mut self.m);
        self.o.gl.clear(&mut self.m);
        this
    }

    /// `ShaderWindowC::~ShaderWindowC()` [soft 0x083929ac _._13ShaderWindowC]: the block at
    /// +0x54 is freed, the IRIS GL window closed; with bit 0 of the hidden argument the
    /// record itself is freed.
    pub fn shader_window_delete(&mut self, this: P<ShaderWindow>, how: u32) {
        let block = self.m[this].d_054;
        if block != 0 {
            self.o.mem.free(&mut self.m, block);
        }
        let id = self.m[this].winid;
        self.o.gl.winclose(&mut self.m, id);
        if how & 1 != 0 {
            self.o.mem.__builtin_delete(&mut self.m, this.addr());
        }
    }

    /// `ShaderWindowC::hit(x, y)` [soft 0x083929f0 hit]: 1 when x is left of the window's
    /// left edge and of its right edge and y below its lower and its upper edge. (As
    /// written, that is the quarter plane to the lower left of the window, not its
    /// inside; nothing in the program calls it.)
    pub fn shader_window_hit(&mut self, this: P<ShaderWindow>, x: i32, y: i32) -> i32 {
        let w = self.m[this];
        (w.x > x && w.y > y && x < w.x.wrapping_add(w.width) && y < w.y.wrapping_add(w.height)) as i32
    }

    /// `ShaderWindowC::redraw(on)` [soft 0x08392a2c redraw]: in the window, REDRAW events
    /// (device 0x210) are queued (`on`) or not.
    pub fn shader_window_redraw(&mut self, this: P<ShaderWindow>, on: i16) {
        let id = self.m[this].winid;
        self.o.gl.winset(&mut self.m, id);
        if on == 0 {
            self.o.gl.unqdevice(&mut self.m, 0x210);
        } else {
            self.o.gl.qdevice(&mut self.m, 0x210);
        }
    }

    /// Open a shader window [soft 0x08392400 SI_uiOpenWindow]: the list is made at the
    /// first call (5 places, growing by 5); a window record is made and added. Returns
    /// its place in the list.
    #[allow(clippy::too_many_arguments)]
    pub fn si_ui_open_window(&mut self, name: u32, x: i32, y: i32, width: u32, height: u32, data: u32, callback: u32) -> i32 {
        if self.m[SHADER_WINDOW_ARRAY].is_null() {
            let block = self.o.mem.__builtin_new(&mut self.m, 0xc);
            self.m[SHADER_WINDOW_ARRAY] = self.simple_list_new(P::at(block), 5, fault!(5, 4));
        }
        let block = self.o.mem.__builtin_new(&mut self.m, 0x60);
        let window = self.shader_window_new(P::at(block), name, x, y, width, height, data, callback);
        let list = self.m[SHADER_WINDOW_ARRAY];
        self.simple_list_add(list, window.addr())
    }

    /// The window at a place of the list (no test of the place).
    fn shader_window_at(&self, place: i32) -> u32 {
        let list = self.m[SHADER_WINDOW_ARRAY];
        self.m.u32(self.m[list].items.wrapping_add((place as u32).wrapping_mul(4)))
    }

    /// Close the shader window at a place of the list [soft 0x0839250c SI_uiCloseWindow]:
    /// it is destroyed and freed and its place cleared. The place is not tested against
    /// the list's length.
    pub fn si_ui_close_window(&mut self, place: i32) {
        if self.m[SHADER_WINDOW_ARRAY].is_null() {
            return;
        }
        let window = self.shader_window_at(place);
        if window != 0 {
            self.shader_window_delete(P::at(window), 3);
        }
        let list = self.m[SHADER_WINDOW_ARRAY];
        let at = self.m[list].items.wrapping_add((place as u32).wrapping_mul(4));
        self.m.set_u32(at, 0);
    }

    /// The IRIS GL window of the shader window at a place, −1 when there is none
    /// [soft 0x0839254c SI_uiGetWindowID].
    pub fn si_ui_get_window_id(&mut self, place: i32) -> i32 {
        let list = self.m[SHADER_WINDOW_ARRAY];
        if list.is_null() || place < 0 || place >= self.m[list].count {
            return -1;
        }
        let window = self.shader_window_at(place);
        if window == 0 {
            return -1;
        }
        self.m[P::<ShaderWindow>::at(window)].winid
    }

    /// Close every shader window and free the list [soft 0x0839257c SI_uiCloseAllWindow].
    pub fn si_ui_close_all_window(&mut self) {
        if self.m[SHADER_WINDOW_ARRAY].is_null() {
            return;
        }
        let mut place = 0;
        while place < self.m[self.m[SHADER_WINDOW_ARRAY]].count {
            self.si_ui_close_window(place);
            place += 1;
        }
        let list = self.m[SHADER_WINDOW_ARRAY];
        if !list.is_null() {
            self.simple_list_delete(list, 3);
        }
        self.m[SHADER_WINDOW_ARRAY] = P::NULL;
    }

    /// A dialog's event loop hands window events on to the shader windows
    /// [soft 0x083925d0 si_uiHandleWindowEvent]. Without shader windows: 0. Queue devices
    /// 0x21b, 0x21c and 0x210 (WINFREEZE, WINTHAW, REDRAW) become the event types 0x16,
    /// 0x17, 0x1b; for those, or when `si_dialogMoved` is set, the event goes to every
    /// shader window with REDRAW unqueued in the dialog's window meanwhile. Returns 1 for
    /// the three devices, else `si_dialogMoved`.
    pub fn si_ui_handle_window_event(&mut self, dialog: u32, event: P<Event>, device: i32, _flag: i16) -> i32 {
        if self.m[SHADER_WINDOW_ARRAY].is_null() {
            return 0;
        }
        let mut answer = self.m[SI_DIALOG_MOVED] as i32;
        match device {
            0x21b => {
                self.m[event].kind = 0x16;
                answer = 1;
            }
            0x210 => {
                self.m[event].kind = 0x1b;
                answer = 1;
            }
            0x21c => {
                self.m[event].kind = 0x17;
                answer = 1;
            }
            _ => {}
        }
        if answer == 0 {
            return 0;
        }
        let id = self.m.i32(dialog.wrapping_add(crate::records::off::DIALOG_WINID));
        self.o.gl.winset(&mut self.m, id);
        self.o.gl.unqdevice(&mut self.m, 0x210);
        self.si_ui_propagate_event(event);
        let id = self.m.i32(dialog.wrapping_add(crate::records::off::DIALOG_WINID));
        self.o.gl.winset(&mut self.m, id);
        self.o.gl.qdevice(&mut self.m, 0x210);
        answer
    }

    /// Give an event to every shader window [soft 0x08392690 si_uiPropagateEvent]: each
    /// in turn is made current and brought to the top, its callback (if it has one) is
    /// called with the event, the IRIS GL window and the window's data word, and `gsync`
    /// waits for the display.
    pub fn si_ui_propagate_event(&mut self, event: P<Event>) {
        let list = self.m[SHADER_WINDOW_ARRAY];
        if list.is_null() || self.m[list].count <= 0 {
            return;
        }
        let mut place = 0;
        loop {
            let window = P::<ShaderWindow>::at(self.shader_window_at(place));
            if !window.is_null() {
                let id = self.m[window].winid;
                self.o.gl.winset(&mut self.m, id);
                self.o.gl.winpop(&mut self.m);
                if self.m[window].callback != 0 {
                    let (data, id) = (self.m[window].data, self.m[window].winid);
                    // the call through the record's function pointer
                    self.o.views.shaderWindowCallback(&mut self.m, event, id, data);
                }
                self.o.gl.gsync(&mut self.m);
            }
            place += 1;
            if place >= self.m[self.m[SHADER_WINDOW_ARRAY]].count {
                break;
            }
        }
    }

    fn shader_windows_redraw(&mut self, on: i16) {
        let list = self.m[SHADER_WINDOW_ARRAY];
        if list.is_null() {
            return;
        }
        let mut place = 0;
        while place < self.m[self.m[SHADER_WINDOW_ARRAY]].count {
            let window = self.shader_window_at(place);
            if window != 0 {
                self.shader_window_redraw(P::at(window), on);
            }
            place += 1;
        }
    }

    /// No REDRAW events from the shader windows [soft 0x08392708 si_uiDisableRedraw].
    pub fn si_ui_disable_redraw(&mut self) {
        self.shader_windows_redraw(0);
    }

    /// REDRAW events from the shader windows again [soft 0x08392750 si_uiEnableRedraw].
    pub fn si_ui_enable_redraw(&mut self) {
        self.shader_windows_redraw(1);
    }
}
