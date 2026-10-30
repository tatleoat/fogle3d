// SPDX-License-Identifier: MIT
//! `window.c`: a view window's title bar: the table of its gadgets and how each is drawn.
//!
//! The bar is drawn into the main IRIS GL window as a sequence of IRIS GL calls (through
//! the `Gl` trait); the check compares that sequence, call for call and argument for
//! argument, with the original's.

use crate::data;
use crate::globals::*;
use crate::mem::P;
use crate::records::{cam, off, Gadget, Grafport};
use crate::{fault, Ui};
use si_x87::{Round, F80};

/// The pause while a clicked gadget is shown pressed, in microseconds.
const PRESSED: u32 = 150_000;

/// A text of the title bar: a constant of the code, or the label the table has for a part.
#[derive(Clone, Copy)]
enum Text<'a> {
    Lit(&'a [u8]),
    Label(i32),
}

impl Ui {
    /// The label of the schematic view's mode gadget for a camera's mode word (+6)
    /// [soft 0x083d1e98 schemGetModeMenuString]: one of six fixed strings for 0 (plain), 1
    /// (light), 2 (model), 4 (motion), 8 (matter), 15 (all); for any other value the
    /// modes in it are spelled into a static buffer as "Lt/Ml/Mn/Mr" (bits 0 to 3; the
    /// last stroke is replaced by the end of the string). Returns the string's address.
    pub fn schem_get_mode_menu_string(&mut self, camera: u32) -> u32 {
        let mode = self.m.i16(camera.wrapping_add(6)) as i32;
        match mode {
            0 => data::SCHEM_PLAIN,
            1 => data::SCHEM_LIGHT,
            2 => data::SCHEM_MODEL,
            4 => data::SCHEM_MOTION,
            8 => data::SCHEM_MATTER,
            0xf => data::SCHEM_ALL,
            _ => {
                let buffer = data::SCHEM_BUFFER;
                let mut n = 0u32;
                for bit in 0..5 {
                    let pair: &[u8; 2] = match (self.m.i16(camera.wrapping_add(6)) as i32) & (1 << bit) {
                        1 => b"Lt",
                        2 => fault!(b"Ml", b"Mo"),
                        4 => b"Mn",
                        8 => b"Mr",
                        _ => continue,
                    };
                    self.m.set_u8(buffer + n, pair[0]);
                    self.m.set_u8(buffer + n + 1, pair[1]);
                    self.m.set_u8(buffer + n + 2, b'/');
                    n += 3;
                }
                // with no mode bit at all this is the byte before the buffer (the end of
                // the string in front of it, already zero)
                self.m.set_u8((buffer + n).wrapping_sub(1), 0);
                buffer
            }
        }
    }

    /// Hide the windows that a zoomed or expanded window covers
    /// [soft 0x083d9378 setVisibleFields]. For each window that is zoomed or expanded, by its identifier: a
    /// zoomed one hides the other three; one expanded upwards hides its vertical neighbour,
    /// one expanded sideways its side neighbour. (Nothing is shown again here.)
    pub fn set_visible_fields(&mut self) {
        let views = self.m[SI_TEMPLATE].views;
        let mut g = P::NULL;
        while self.si_window_get_next(&mut g) != 0 {
            let w = self.m[g];
            if w.zoomed == 0 && w.expand == 0 {
                continue;
            }
            // (the other three in the order they are hidden when zoomed; the vertical
            // neighbour; the side neighbour)
            let (zoomed, vertical, side): ([i32; 3], i32, i32) = match w.id {
                1001 => ([2, 3, 0], 0, 3),
                1000 => ([2, 3, 1], 1, 2),
                1002 => ([0, 3, 1], 3, 0),
                1003 => ([2, 0, 1], 2, 1),
                _ => continue,
            };
            if w.zoomed != 0 {
                for k in zoomed {
                    self.m[views.add(k)].visible = 0;
                }
            } else {
                match self.m[g].expand {
                    2 => self.m[views.add(fault!(vertical, side))].visible = 0,
                    1 => self.m[views.add(side)].visible = 0,
                    _ => {}
                }
            }
        }
    }

    /// Fill the table of a view window's parts from the static table, once
    /// [soft 0x083d955c initGadget].
    ///
    /// Every entry's label is cleared first. Then each row of the static table goes to the
    /// entry of its part number: labels, view type, the room it needs, handler; its width
    /// is the text width of its label + 1 (+ the width of the second label); its offset is
    /// the row's own, or, where that is 0, the place the layout has reached. The layout
    /// starts again with each run of rows of one view type: at 250 pixels from the window's
    /// left edge (215 for the dope sheet, 150 for the spreadsheet, 230 for the schematic
    /// view on screen size 1), and each gadget is followed by 7 pixels (5 for the
    /// function-curve and free orthogonal views on screen size 1). Last the parts placed
    /// by other measures: "1:1" (8) at `SI_dovResetx`, SETUP (6) at `si_titleSetup`, CLIP
    /// (50) `SI_memoSpacing` + 30 beyond that, the layout box (7) at `si_titleLayout` (5
    /// less on screen size 1) and 22 wide, the selection box (2) at 1 and 12 wide.
    pub fn init_gadget(&mut self) {
        if self.m[GADGET_LIST_INIT] != 0 {
            return;
        }
        let row = |ui: &Ui, i: u32, at: u32| ui.m.u32(data::GADGET_TITLES_AT + 0x1c * i + at);
        // where a run of rows of one view type begins
        let mut starts = Vec::new();
        let mut ended = false;
        for i in 0..0x37u32 {
            if !ended {
                if row(self, i, 0) as i32 == -1 {
                    ended = true;
                }
                if !ended && (i == 0 || row(self, i, 0x10) != row(self, i - 1, 0x10)) {
                    starts.push(i);
                }
            }
            self.m[GADGET_INFO.cast::<Gadget>().add(i as i32)].label = 0;
        }
        let small = self.m[SI_SCREEN_RES] == 1;
        let (mut n, mut x) = (0usize, 0i32);
        if row(self, 0, 0) as i32 != -1 {
            let mut i = 0u32;
            loop {
                // Past the last run the original compares with a word of its stack that it
                // never set; here nothing begins there.
                if starts.get(n) == Some(&i) {
                    n += 1;
                    x = match row(self, i, 0x10) as i32 {
                        2008 => 0xd7,
                        2010 => 0x96,
                        2007 if self.m[SI_SCREEN_RES] == 1 => 0xe6,
                        _ => 0xfa,
                    };
                }
                let part = row(self, i, 0) as i32;
                let entry = GADGET_INFO.cast::<Gadget>().add(part);
                self.m[entry].label = row(self, i, 4);
                self.m[entry].label2 = row(self, i, 8);
                self.m[entry].camera = row(self, i, 0x10) as i32;
                self.m[entry].d_014 = row(self, i, 0x14) as i32;
                self.m[entry].handler = row(self, i, 0x18);
                let own = row(self, i, 0xc) as i32;
                self.m[entry].offset = if own == 0 { x } else { own };
                let label = self.m.cstr(row(self, i, 4));
                let width = self.o.gl.strwidth(&mut self.m, &label);
                let entry = GADGET_INFO.cast::<Gadget>().add(row(self, i, 0) as i32);
                self.m[entry].width = width.wrapping_add(1);
                if row(self, i, 8) != 0 {
                    let label2 = self.m.cstr(row(self, i, 8));
                    let width2 = self.o.gl.strwidth(&mut self.m, &label2);
                    let entry = GADGET_INFO.cast::<Gadget>().add(row(self, i, 0) as i32);
                    self.m[entry].width = self.m[entry].width.wrapping_add(width2);
                }
                let kind = row(self, i, 0x10) as i32;
                let tight = if kind == 2004 || kind == 2009 { (small as i32) * 2 } else { 0 };
                let entry = GADGET_INFO.cast::<Gadget>().add(row(self, i, 0) as i32);
                x = x.wrapping_add(self.m[entry].width.wrapping_sub(tight - fault!(7, 6)));
                i += 1;
                if row(self, i, 0) as i32 == -1 {
                    break;
                }
            }
        }
        let info = |part: i32| GADGET_INFO.cast::<Gadget>().add(part);
        self.m[info(8)].offset = self.m[SI_DOV_RESETX] as i32;
        self.m[info(6)].offset = self.m[SI_TITLE_SETUP] as i32;
        self.m[info(50)].offset = self.m[SI_MEMO_SPACING] as i32 + 0x1e + self.m[info(6)].offset;
        self.m[info(7)].offset = 0;
        if self.m[SI_SCREEN_RES] == 1 {
            self.m[info(7)].offset = 5;
        }
        self.m[info(7)].offset = self.m[SI_TITLE_LAYOUT] as i32 - self.m[info(7)].offset;
        self.m[info(7)].width = 0x16;
        self.m[info(2)].offset = 1;
        self.m[info(2)].width = 0xc;
        self.m[GADGET_LIST_INIT] = 1;
    }

    /// While a clicked gadget is drawn pressed: wait, then take the three queue entries of
    /// the click (with a tablet, poll it until the queue has something).
    fn pressed_pause(&mut self) {
        self.o.c.usleep(&mut self.m, PRESSED);
        if self.m[SI_TABLET_ACTIVE] != 0 {
            while self.o.gl.qtest(&mut self.m) == 0 {
                self.o.devices.SI_tabletGetData(&mut self.m);
            }
        }
        let mut value = 0i16;
        for _ in 0..3 {
            self.o.gl.qread(&mut self.m, &mut value);
        }
    }

    fn charstr_at(&mut self, text: u32) {
        let s = self.m.cstr(text);
        self.o.gl.charstr(&mut self.m, &s);
    }

    fn strwidth_at(&mut self, text: u32) -> i32 {
        let s = self.m.cstr(text);
        self.o.gl.strwidth(&mut self.m, &s)
    }

    /// Draw the gadgets of one view type in a title bar [soft 0x083d9844 drawMenuGadget]:
    /// every entry of the table for that type whose label fits between 15 pixels right of
    /// the window's left edge and its right edge (label width + the entry's room), at `x0`
    /// + the entry's offset and height `y`, label and second label. The colour is the
    /// text colour (the highlight text colour in a frozen window); OPEN/CLOSE (20) while
    /// the schematic view is open and ACT_KEY (52) while its switch is off are in the
    /// frame-highlight colour; CLEAN_ALL (21) is left out while its word is set. The entry
    /// `part` is first drawn pressed (control-highlight colour) for a moment.
    pub fn draw_menu_gadget(&mut self, g: P<Grafport>, kind: i32, x0: i32, y: i32, part: i32) {
        let normal = if self.m[g].enabled != 0 { self.m[SI_WINDOW_TEXT] } else { self.m[SI_WINDOW_TEXT_HILITE] };
        for i in 0..0x37i32 {
            let entry = GADGET_INFO.cast::<Gadget>().add(i);
            if self.m[entry].camera != kind {
                continue;
            }
            if i == 0x15 && self.m[DAT_087FB9AC] != 0 {
                continue;
            }
            let mut colour = normal;
            if i == 0x14 {
                if self.m[SI_SCHEMATIC_MODE] != 0 {
                    colour = self.m[SI_WINDOW_FRAME_HILITE];
                }
            } else if i == 0x34 && self.m[DAT_0878FB22] == 0 {
                colour = self.m[SI_WINDOW_FRAME_HILITE];
            }
            if x0.wrapping_add(self.m[entry].offset) <= self.m[g].left as i32 + 0xf {
                continue;
            }
            let width = self.strwidth_at(self.m[entry].label);
            let e = self.m[entry];
            if width.wrapping_add(fault!(e.d_014, e.width)).wrapping_add(x0.wrapping_add(e.offset)) >= self.m[g].right as i32 {
                continue;
            }
            let draw = |ui: &mut Ui, colour: i16| {
                let x = x0.wrapping_add(ui.m[entry].offset);
                ui.o.gl.cmov2i(&mut ui.m, x, y);
                ui.o.gl.color(&mut ui.m, colour as u16);
                ui.charstr_at(ui.m[entry].label);
                if ui.m[entry].label2 != 0 {
                    let width = ui.strwidth_at(ui.m[entry].label);
                    let x = width.wrapping_add(x0.wrapping_add(ui.m[entry].offset)).wrapping_sub(1);
                    ui.o.gl.cmov2i(&mut ui.m, x, y);
                    ui.charstr_at(ui.m[entry].label2);
                }
            };
            if part == i {
                let pressed = self.m[SI_CONTROL_TEXT_HILITE];
                draw(self, pressed);
                self.pressed_pause();
            }
            draw(self, colour);
        }
    }

    /// Draw a view window in the Dialog Editor [soft 0x083dae2c deWindowDraw]: the frame
    /// and a 16-pixel title line in the text colour (the highlight colour when selected),
    /// the inside in the viewport colour, the title bar in the title colour with the word
    /// "editor" centred in it, the selection box, and the zoom box's glyph (the outline of
    /// the small-in-large or large-in-small corner), all with the whole-system write mask.
    ///
    /// The text's place: half the bar's width + 0.5 and half the name's text width + 0.5,
    /// each computed in the 80-bit registers and truncated toward zero; the first as a
    /// 16-bit value from the bar's left edge, less the second.
    pub fn de_window_draw(&mut self, g: P<Grafport>) {
        let mask = self.o.gl.getwritemask(&mut self.m);
        let whole = self.m[SI_WHOLE_SYSTEM_WM];
        self.o.gl.writemask(&mut self.m, whole as u16);
        let c = if self.m[g].hilite != 0 { self.m[SI_WINDOW_FRAME_HILITE] } else { self.m[SI_WINDOW_TEXT] };
        self.o.gl.color(&mut self.m, c as u16);
        let w = self.m[g];
        self.o.gl.recti(&mut self.m, w.left as i32, w.bottom as i32, w.right as i32, w.top as i32);
        let w = self.m[g];
        self.o.gl.recti(&mut self.m, w.left as i32, w.top as i32 - 0x10, w.right as i32, w.top as i32);
        let c = self.m[SI_WINDOW_VIEWPORT];
        self.o.gl.color(&mut self.m, c as u16);
        let w = self.m[g];
        self.sboxfi(w.left as i32 + 1, w.bottom as i32 + 1, w.right as i32 - 1, w.top as i32 - 0x11);
        let w = self.m[g];
        // 16-bit values from here on, as the original keeps them
        let t1 = w.top.wrapping_sub(1);
        let ty = w.top.wrapping_sub(0xf);
        let l1 = w.left.wrapping_add(1);
        let r1 = w.right.wrapping_sub(1);
        let c = self.m[SI_WINDOW_TITLE];
        self.o.gl.color(&mut self.m, c as u16);
        self.sboxfi(l1 as i32, ty as i32, r1 as i32, t1 as i32);
        let name_width = self.strwidth_at(self.m[g].name);
        let half_name = (F80::from_i32(name_width) * 0.5f64 + 0.5f64).to_i32(fault!(Round::Zero, Round::Nearest));
        let descender = self.o.gl.getdescender(&mut self.m);
        let half_bar = (F80::from_i32(r1 as i32 - l1 as i32 + 1) * 0.5f64 + 0.5f64).to_i32(Round::Zero);
        let c = if self.m[g].enabled != 0 { self.m[SI_WINDOW_TEXT] } else { self.m[SI_WINDOW_TEXT_HILITE] };
        self.o.gl.color(&mut self.m, c as u16);
        self.o.gl.cmov2i(&mut self.m, (half_bar as i16 as i32 + l1 as i32).wrapping_sub(half_name), descender.wrapping_add(ty as i32).wrapping_add(1));
        self.o.gl.charstr(&mut self.m, b"editor");
        let c = self.m[SI_WINDOW_TEXT];
        self.o.gl.color(&mut self.m, c as u16);
        self.o.gl.recti(&mut self.m, l1 as i32 + 2, ty as i32 + 2, l1 as i32 + 0xc, t1 as i32 - 2);
        let s = |v: i16, d: i16| v.wrapping_add(d);
        if self.m[g].zoomed != 0 {
            self.o.gl.move2s(&mut self.m, s(r1, -10), s(ty, 8));
            self.o.gl.draw2s(&mut self.m, s(r1, -12), s(ty, 8));
            self.o.gl.draw2s(&mut self.m, s(r1, -12), s(ty, 2));
            self.o.gl.draw2s(&mut self.m, s(r1, -5), s(ty, 2));
            self.o.gl.draw2s(&mut self.m, s(r1, -5), s(t1, -9));
            self.o.gl.recti(&mut self.m, r1 as i32 - 10, t1 as i32 - 9, r1 as i32 - 2, t1 as i32 - 2);
        } else {
            self.o.gl.move2s(&mut self.m, s(r1, -10), s(t1, -6));
            self.o.gl.draw2s(&mut self.m, s(r1, -10), s(t1, -2));
            self.o.gl.draw2s(&mut self.m, s(r1, -2), s(t1, -2));
            self.o.gl.draw2s(&mut self.m, s(r1, -2), s(t1, -9));
            self.o.gl.draw2s(&mut self.m, s(r1, -5), s(t1, -9));
            self.o.gl.recti(&mut self.m, r1 as i32 - 12, ty as i32 + 2, r1 as i32 - 5, ty as i32 + 8);
        }
        self.o.gl.writemask(&mut self.m, mask as u16);
    }

    /// The bytes of a text of the bar: a constant, or the table's label of a part, read
    /// when it is used (a null label stops the routine there, as in the original).
    fn bar_bytes(&mut self, text: Text) -> Vec<u8> {
        match text {
            Text::Lit(b) => b.to_vec(),
            Text::Label(part) => {
                let at = self.get_gadget_label(part);
                self.m.cstr(at)
            }
        }
    }

    /// A text gadget of the bar's own (not of the table's runs): if its place is right of
    /// the window's left edge + 15 and the text, with its room, ends left of the right
    /// edge, it is drawn at (`x`, `y`) in `colour`; `pressed` first shows it in the
    /// control-highlight colour for a moment. `measure` is the text whose width is tested.
    #[allow(clippy::too_many_arguments)]
    fn bar_text(&mut self, g: P<Grafport>, x: &dyn Fn(&mut Ui) -> i32, fits: &dyn Fn(&mut Ui, i32) -> i32, measure: Text, text: Text, y: i32, colour: i16, pressed: bool) {
        if x(self) <= self.m[g].left as i32 + 0xf {
            return;
        }
        let measure = self.bar_bytes(measure);
        let width = self.o.gl.strwidth(&mut self.m, &measure);
        if fits(self, width) >= self.m[g].right as i32 {
            return;
        }
        if pressed {
            let at = x(self);
            self.o.gl.cmov2i(&mut self.m, at, y);
            let c = self.m[SI_CONTROL_TEXT_HILITE];
            self.o.gl.color(&mut self.m, c as u16);
            let t = self.bar_bytes(text);
            self.o.gl.charstr(&mut self.m, &t);
            self.pressed_pause();
        }
        let at = x(self);
        self.o.gl.cmov2i(&mut self.m, at, y);
        self.o.gl.color(&mut self.m, colour as u16);
        let t = self.bar_bytes(text);
        self.o.gl.charstr(&mut self.m, &t);
    }

    /// Draw a view window's title bar [soft 0x083d7264 SI_windowGadjet]; `part` −1 draws
    /// the bar's background too, any other part is the gadget that was clicked and is
    /// first shown pressed (control-highlight colour, 150 ms, the click's three queue
    /// entries taken); `front` non-zero draws into the front buffer.
    ///
    /// Nothing for a hidden window. In the main IRIS GL window, in the title font: the
    /// bar (frame in the text or frame-highlight colour, inside in the title colour); the
    /// selection box with the window's letter (C, A, D, B for 1000 to 1003); the zoom
    /// box's glyph; then by display mode SETUP on the right (modes 1 to 6) and, in the
    /// rotoscope modes of a view that is not perspective, "LOCK" and the "1:1" gadget; the
    /// title with its menu glyph; the stored-camera boxes (`SI_memoCamDraw`) and the layout
    /// box; then by view type: CLIP for front, top and right views whose clipping planes
    /// are narrower than the type's defaults; "LOCK" and "1:1" for the perspective view;
    /// the type's own gadgets for the others (`drawMenuGadget`; the schematic view's mode
    /// label and the dope sheet's ACT_KEY / ACT_DEF are set first); and for the camera
    /// views the name of the display mode with its menu glyph. A text is drawn only if it
    /// fits (see `bar_text`).
    pub fn si_window_gadjet(&mut self, g: P<Grafport>, part: i32, front: i16) {
        let w = self.m[g];
        let top = w.top as i32;
        let ty = top + 2 - self.m[SI_TITLE_HEIGHT] as i32;
        let right = w.right as i32;
        let left = w.left as i32;
        let (r1, l1) = (right - 1, left + 1);
        let current = self.o.gl.winget(&mut self.m) as i16 as i32;
        if self.m[g].visible == 0 {
            return;
        }
        let sysid = self.m[SI_SYSID];
        if current != sysid {
            self.o.gl.winset(&mut self.m, sysid);
        }
        let f = self.m[SI_APPF];
        self.o.gl.font(&mut self.m, f);
        self.init_gadget();
        if front != 0 {
            self.o.gl.frontbuffer(&mut self.m, 1);
        }
        let text = if self.m[g].enabled != 0 { self.m[SI_WINDOW_TEXT] } else { self.m[SI_WINDOW_TEXT_HILITE] };
        if self.m[BASE_445] == -1 {
            self.m[BASE_445] = self.o.gl.getdescender(&mut self.m);
        }
        if part == -1 {
            let c = if self.m[g].hilite != 0 { self.m[SI_WINDOW_FRAME_HILITE] } else { self.m[SI_WINDOW_TEXT] };
            self.o.gl.color(&mut self.m, c as u16);
            self.o.gl.sboxi(&mut self.m, left, ty - 1, right, top);
            let c = self.m[SI_WINDOW_TITLE];
            self.o.gl.color(&mut self.m, c as u16);
            self.sboxfi(l1, ty, r1, top - 1);
        }
        self.o.gl.color(&mut self.m, text as u16);
        self.o.gl.sboxi(&mut self.m, left + 2, ty + 1, left + 0xe, top - 2);
        self.o.gl.cmov2i(&mut self.m, left + 6, ty + 3);
        let letter: Option<&[u8]> = match self.m[g].id {
            1001 => Some(b"A"),
            1002 => Some(fault!(b"D", b"B")),
            1003 => Some(b"B"),
            1000 => Some(b"C"),
            _ => None,
        };
        if let Some(letter) = letter {
            self.o.gl.charstr(&mut self.m, letter);
        }
        let c = self.m[SI_WINDOW_TEXT];
        self.o.gl.color(&mut self.m, c as u16);
        // the zoom box: an arrow's outline and the small box it points from or to
        if self.m[g].zoomed != 0 {
            let mut v = [right - 0xb, ty + 8];
            self.o.gl.bgnline(&mut self.m);
            self.o.gl.v2i(&mut self.m, v);
            v[0] -= 2;
            self.o.gl.v2i(&mut self.m, v);
            v[1] -= 6;
            self.o.gl.v2i(&mut self.m, v);
            v[0] += 7;
            self.o.gl.v2i(&mut self.m, v);
            v[1] = top - 10;
            self.o.gl.v2i(&mut self.m, v);
            self.o.gl.endline(&mut self.m);
            self.o.gl.sboxi(&mut self.m, right - 0xb, top - 10, right - 3, top - 3);
        } else {
            let mut v = [right - 0xb, top - 7];
            self.o.gl.bgnline(&mut self.m);
            self.o.gl.v2i(&mut self.m, v);
            v[1] += 4;
            self.o.gl.v2i(&mut self.m, v);
            v[0] = right - 3;
            self.o.gl.v2i(&mut self.m, v);
            v[1] -= 7;
            self.o.gl.v2i(&mut self.m, v);
            v[0] = right - 6;
            self.o.gl.v2i(&mut self.m, v);
            self.o.gl.endline(&mut self.m);
            self.o.gl.sboxi(&mut self.m, right - 0xd, ty + 2, right - 6, ty + 8);
        }
        let y = ty.wrapping_add(self.m[BASE_445]);
        let mut room = 0x5a;

        // SETUP, from the right edge
        let setup = |ui: &mut Ui| {
            ui.bar_text(g, &|ui| r1.wrapping_sub(ui.get_gadget_offset(6)), &|ui, w| r1.wrapping_sub(ui.get_gadget_offset(6)).wrapping_add(w), Text::Label(6), Text::Label(6), y, text, part == 6);
        };
        // "LOCK" and the "1:1" gadget (part 8), from the left edge with 90 pixels of room
        let at8 = |ui: &mut Ui| ui.get_gadget_offset(8).wrapping_add(l1);
        let fits8 = |ui: &mut Ui, w: i32| ui.get_gadget_offset(8).wrapping_add(0x5a).wrapping_add(l1).wrapping_add(w);
        let lock = |ui: &mut Ui, colour: i16| ui.bar_text(g, &at8, &fits8, Text::Lit(b"LOCK"), Text::Lit(b"LOCK"), y, colour, false);
        let one_to_one = |ui: &mut Ui, colour: i16, pressed: bool| ui.bar_text(g, &at8, &fits8, Text::Label(8), Text::Label(8), y, colour, pressed);

        match self.m[g].display {
            1 | 4 | 5 | 6 => {
                setup(self);
                room = 0x84;
            }
            2 | 3 => {
                setup(self);
                let title = self.m[SI_WINDOW_TITLE];
                room = 0x84;
                let camera = self.m[g].camera;
                if self.m.i16(camera.wrapping_add(off::CAMERA_TYPE)) != cam::PERSPECTIVE {
                    if self.m.i16(camera.wrapping_add(off::CAMERA_44)) == 0 {
                        lock(self, text);
                        let title = self.m[SI_WINDOW_TITLE];
                        one_to_one(self, title, false);
                    } else {
                        lock(self, title);
                        one_to_one(self, text, part == 8);
                        room = fault!(0xa0, 0x9f);
                    }
                }
            }
            _ => {}
        }

        // the title and the glyph of its menu
        if self.get_gadget_offset(9).wrapping_add(l1) > self.m[g].left as i32 + 0xf {
            let offset = self.get_gadget_offset(9);
            let width = self.strwidth_at(self.m[g].name);
            if l1.wrapping_add(offset).wrapping_add(width).wrapping_add(room) < self.m[g].right as i32 {
                let title = |ui: &mut Ui, colour: i16| {
                    let y = ty.wrapping_add(ui.m[BASE_445]);
                    let x = ui.get_gadget_offset(9).wrapping_add(l1);
                    ui.o.gl.cmov2i(&mut ui.m, x, y);
                    ui.o.gl.color(&mut ui.m, colour as u16);
                    ui.charstr_at(ui.m[g].name);
                    let y = ty.wrapping_add(ui.m[BASE_445]);
                    let offset = ui.get_gadget_offset(9);
                    let width = ui.strwidth_at(ui.m[g].name);
                    ui.o.gl.cmov2i(&mut ui.m, width.wrapping_sub(1).wrapping_add(l1).wrapping_add(offset), y);
                    ui.o.gl.charstr(&mut ui.m, b"\x06");
                };
                if part == 9 {
                    let pressed = self.m[SI_CONTROL_TEXT_HILITE];
                    title(self, pressed);
                    self.pressed_pause();
                }
                title(self, text);
            }
        }

        // the stored-camera boxes and the layout box
        if self.camera_type(g) != cam::SPREADSHEET {
            self.o.views.SI_memoCamDraw(&mut self.m, g, l1, y, room);
            let kind = self.camera_type(g);
            if kind != cam::SPREADSHEET && self.camera_type(g) != cam::DOPE_SHEET && self.get_gadget_offset(7).wrapping_add(l1) > self.m[g].left as i32 + 0xf {
                let offset = self.get_gadget_offset(7);
                let width = self.o.gl.strwidth(&mut self.m, b"\x07");
                if offset.wrapping_add(0x3c).wrapping_add(l1).wrapping_add(width) < self.m[g].right as i32 {
                    let y1 = y.wrapping_add(1);
                    let layout = |ui: &mut Ui, colour: i16| {
                        let x = ui.get_gadget_offset(7).wrapping_add(l1);
                        ui.o.gl.cmov2i(&mut ui.m, x, y1);
                        ui.o.gl.color(&mut ui.m, colour as u16);
                        ui.o.gl.charstr(&mut ui.m, b"\x07");
                        let offset = ui.get_gadget_offset(7);
                        let width = ui.o.gl.strwidth(&mut ui.m, b"\x07");
                        ui.o.gl.cmov2i(&mut ui.m, width.wrapping_sub(1).wrapping_add(l1).wrapping_add(offset), y1);
                        ui.o.gl.charstr(&mut ui.m, b"\x05");
                    };
                    if part == 7 {
                        let pressed = self.m[SI_CONTROL_TEXT_HILITE];
                        layout(self, pressed);
                        self.pressed_pause();
                    }
                    layout(self, text);
                }
            }
        }

        // by view type
        let title = self.m[SI_WINDOW_TITLE];
        let camera = self.m[g].camera;
        let kind = self.m.i16(camera.wrapping_add(off::CAMERA_TYPE));
        let mut mode_name = false;
        match kind {
            cam::FRONT | cam::TOP | cam::RIGHT => {
                let mut planes = [0u32; 2];
                if self.o.views.SI_camGetDefaultFromId(&mut self.m, kind as i32, &mut planes) != 0 {
                    let camera = self.m[g].camera;
                    let (near, far) = (F80::from_f32(f32::from_bits(planes[0])), F80::from_f32(f32::from_bits(planes[1])));
                    if near.lt(F80::from_f32(self.m.f32(camera.wrapping_add(0x34)))) || far.gt(F80::from_f32(self.m.f32(camera.wrapping_add(0x38)))) {
                        self.bar_text(
                            g,
                            &|ui| r1.wrapping_sub(ui.get_gadget_offset(0x32)),
                            &|ui, w| r1.wrapping_sub(ui.get_gadget_offset(0x32)).wrapping_add(w),
                            Text::Label(0x32),
                            Text::Label(0x32),
                            y,
                            text,
                            part == 0x32,
                        );
                    }
                }
                mode_name = true;
            }
            cam::PERSPECTIVE => {
                if self.m.i16(camera.wrapping_add(off::CAMERA_44)) == 0 {
                    if (self.m[g].display.wrapping_sub(2) as u32) < 2 {
                        lock(self, text);
                    }
                    let title = self.m[SI_WINDOW_TITLE];
                    one_to_one(self, title, false);
                } else {
                    lock(self, title);
                    one_to_one(self, text, part == 8);
                }
                mode_name = true;
            }
            cam::FCURVE | cam::SPREADSHEET => self.draw_menu_gadget(g, kind as i32, l1, y, part),
            cam::SCHEMATIC => {
                let label = self.schem_get_mode_menu_string(camera);
                self.m[GADGET_INFO.cast::<Gadget>().add(23)].label = label;
                let kind = self.camera_type(g) as i32;
                self.draw_menu_gadget(g, kind, l1, y, part);
            }
            cam::DOPE_SHEET => {
                let label = if self.m[DAT_0878FB22] == 0 { data::ACT_DEF } else { data::ACT_KEY };
                self.m[GADGET_INFO.cast::<Gadget>().add(52)].label = label;
                let kind = self.camera_type(g) as i32;
                self.draw_menu_gadget(g, kind, l1, y, part);
            }
            cam::ORTHO => {
                mode_name = true;
                self.draw_menu_gadget(g, kind as i32, l1, y, part);
            }
            _ => {}
        }

        // the name of the display mode and the glyph of its menu
        if mode_name {
            let name: &[u8] = match self.m[g].d_068 {
                0 => b"WIRE",
                1 => b"SHADE",
                2 => b"ROTO-W",
                3 => b"ROTO-SH",
                4 => b"DEPTH",
                5 => b"GHOST",
                6 => b"MATTE",
                _ => b"",
            };
            let x = right - 0x47;
            if self.m[g].left as i32 + 0xf < x {
                let width = self.o.gl.strwidth(&mut self.m, name);
                if (right - 0x47).wrapping_add(width) < self.m[g].right as i32 {
                    let mode = |ui: &mut Ui, colour: i16| {
                        ui.o.gl.cmov2i(&mut ui.m, x, y);
                        ui.o.gl.color(&mut ui.m, colour as u16);
                        ui.o.gl.charstr(&mut ui.m, name);
                        let width = ui.o.gl.strwidth(&mut ui.m, name);
                        ui.o.gl.cmov2i(&mut ui.m, (right - 0x48).wrapping_add(width), y);
                        ui.o.gl.charstr(&mut ui.m, b"\x06");
                    };
                    if part == 5 {
                        let pressed = self.m[SI_CONTROL_TEXT_HILITE];
                        mode(self, pressed);
                        self.pressed_pause();
                    }
                    mode(self, text);
                }
            }
        }
        let f = self.m[SI_IRISF];
        self.o.gl.font(&mut self.m, f);
        if front != 0 {
            self.o.gl.frontbuffer(&mut self.m, 0);
        }
        if current != self.m[SI_SYSID] {
            self.o.gl.winset(&mut self.m, current);
        }
    }
}
