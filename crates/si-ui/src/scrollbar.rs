// SPDX-License-Identifier: MIT
//! `scrollbar.c` of the program (0x083bb6f0 to 0x083bc65a): the small scroll bar of the
//! toolkit, a record of 0x40 bytes with a slider between two arrow boxes, horizontal or
//! vertical. The record's layout is [`crate::records::Scrollbar`].
//!
//! Left uncited: `SI_scrollbarHandleEvent` (0x083bba84), which polls the mouse buttons in a
//! loop until they are released (an interaction loop).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.

use crate::mem::{Addr, P};
use crate::globals::*;
use crate::records::Scrollbar;
use crate::Ui;
use si_x87::{Round, F80};

impl Ui {
    /// A new scroll bar [soft 0x083bb6f0 SI_scrollbarCreate]; null when the allocation fails.
    /// `range`, `visible_cap` and `pos` become the bar's range, visible units (at most
    /// the range) and position; `inc_*` its three button moves, `page` the page move.
    #[allow(clippy::too_many_arguments)]
    pub fn si_scrollbar_create(
        &mut self, grafport: Addr, x: i32, y: i32, length: i32, vertical: i16, range: u32, visible_cap: u32, pos: u32,
        page: u32, inc_left: u32, inc_middle: u32, inc_right: u32, user: Addr, callback: Addr,
    ) -> P<Scrollbar> {
        let at = self.o.mem.SI_malloc(&mut self.m, 0x40);
        if at == 0 {
            return P::NULL;
        }
        let m = &mut self.m;
        m.set_u32(at, grafport);
        m.set_i32(at + 4, x);
        m.set_i32(at + 8, y);
        m.set_i32(at + 0xc, length);
        m.set_i16(at + 0x10, vertical);
        m.set_u32(at + 0x14, range);
        m.set_u32(at + 0x20, page);
        m.set_u32(at + 0x24, inc_left);
        m.set_u32(at + 0x28, inc_middle);
        m.set_u32(at + 0x2c, inc_right);
        let ticks = self.o.c.__sysconf(&mut self.m, 2);
        // a quarter of a second in ticks (the division by four rounds towards zero)
        let quarter = (ticks + if ticks < 0 { 3 } else { 0 }) >> 2;
        self.m.set_i16(at + 0x30, quarter as i16);
        let ticks = self.o.c.__sysconf(&mut self.m, 2);
        self.m.set_i16(at + 0x32, (ticks / 0x14) as i16);
        self.m.set_u32(at + 0x34, user);
        self.m.set_u32(at + 0x38, callback);
        self.m.set_u32(at + 0x3c, 0);
        self.m.set_u32(at + 0x1c, 0);
        let visible = if visible_cap < range { visible_cap } else { range };
        self.m.set_u32(at + 0x18, visible);
        self.si_scrollbar_set_slider(P::at(at), pos as i32, 0);
        P::at(at)
    }

    /// Move the slider to `pos`, kept between 0 and range minus visible [soft 0x083bb7d0 SI_scrollbarSetSlider]:
    /// when that changes it, the bar is redrawn and, with `notify` non-zero, the callback told.
    pub fn si_scrollbar_set_slider(&mut self, p: P<Scrollbar>, pos: i32, notify: i16) {
        let sb = self.m[p];
        let mut pos = pos;
        if pos < 0 {
            pos = 0;
        } else if sb.range.wrapping_sub(sb.visible) < pos as u32 {
            pos = sb.range.wrapping_sub(sb.visible) as i32;
        }
        if pos as u32 != sb.pos {
            self.m.set_u32(p.addr() + 0x1c, pos as u32);
            self.si_scrollbar_draw(p, 1);
            let sb = self.m[p];
            if notify != 0 && sb.callback != 0 {
                self.o.controls.scrollbarCallback(&mut self.m, p.addr(), sb.user, pos);
            }
        }
    }

    /// [soft 0x083bb838 SI_scrollbarSetIncrements]
    pub fn si_scrollbar_set_increments(&mut self, p: P<Scrollbar>, left: u32, middle: u32, right: u32) {
        self.m.set_u32(p.addr() + 0x24, left);
        self.m.set_u32(p.addr() + 0x28, middle);
        self.m.set_u32(p.addr() + 0x2c, right);
    }

    /// Set the two repeat delays, given in thousandths of a second, as ticks
    /// [soft 0x083bb858 SI_scrollbarSetDelays].
    pub fn si_scrollbar_set_delays(&mut self, p: P<Scrollbar>, first: u16, next: u16) {
        let t = self.o.c.__sysconf(&mut self.m, 2);
        self.m.set_i16(p.addr() + 0x30, (first as i32).wrapping_mul(t).wrapping_div(1000) as i16);
        let t = self.o.c.__sysconf(&mut self.m, 2);
        self.m.set_i16(p.addr() + 0x32, (next as i32).wrapping_mul(t).wrapping_div(1000) as i16);
    }

    /// [soft 0x083bb8bc SI_scrollbarSetUnits]
    pub fn si_scrollbar_set_units(&mut self, p: P<Scrollbar>, range: u32, visible_cap: u32, pos: u32, page: u32, notify: i16) {
        self.m.set_u32(p.addr() + 0x14, range);
        let visible = if visible_cap < range { visible_cap } else { range };
        self.m.set_u32(p.addr() + 0x18, visible);
        self.m.set_u32(p.addr() + 0x20, page);
        self.si_scrollbar_set_slider(p, pos as i32, notify);
    }

    /// [soft 0x083bb8fc SI_scrollbarSetUserData]
    pub fn si_scrollbar_set_user_data(&mut self, p: P<Scrollbar>, user: Addr) {
        self.m.set_u32(p.addr() + 0x34, user);
    }

    /// [soft 0x083bb90c SI_scrollbarSetCallback]
    pub fn si_scrollbar_set_callback(&mut self, p: P<Scrollbar>, callback: Addr) {
        self.m.set_u32(p.addr() + 0x38, callback);
    }

    /// [soft 0x083bb91c SI_scrollbarSetPos]: move the bar and redraw it.
    pub fn si_scrollbar_set_pos(&mut self, p: P<Scrollbar>, x: i32, y: i32, length: i32) {
        self.m.set_i32(p.addr() + 4, x);
        self.m.set_i32(p.addr() + 8, y);
        self.m.set_i32(p.addr() + 0xc, length);
        self.si_scrollbar_draw(p, 1);
    }

    /// [soft 0x083bb94c SI_scrollbarIsVertical]
    pub fn si_scrollbar_is_vertical(&mut self, p: P<Scrollbar>) -> i32 {
        self.m.i16(p.addr() + 0x10) as i32
    }

    /// [soft 0x083bb95c SI_scrollbarMoveUp]: the slider moves back by `by`.
    pub fn si_scrollbar_move_up(&mut self, p: P<Scrollbar>, by: i32, notify: i16) {
        let pos = self.m.i32(p.addr() + 0x1c);
        self.si_scrollbar_set_slider(p, pos.wrapping_sub(by), notify)
    }

    /// [soft 0x083bb980 SI_scrollbarMoveDown]: forward by `by`.
    pub fn si_scrollbar_move_down(&mut self, p: P<Scrollbar>, by: i32, notify: i16) {
        let pos = self.m.i32(p.addr() + 0x1c);
        self.si_scrollbar_set_slider(p, pos.wrapping_add(by), notify)
    }

    /// [soft 0x083bb9a4 SI_scrollbarMoveLeft]: back by `by`.
    pub fn si_scrollbar_move_left(&mut self, p: P<Scrollbar>, by: i32, notify: i16) {
        let pos = self.m.i32(p.addr() + 0x1c);
        self.si_scrollbar_set_slider(p, pos.wrapping_sub(by), notify)
    }

    /// [soft 0x083bb9c8 SI_scrollbarMoveRight]: forward by `by`.
    pub fn si_scrollbar_move_right(&mut self, p: P<Scrollbar>, by: i32, notify: i16) {
        let pos = self.m.i32(p.addr() + 0x1c);
        self.si_scrollbar_set_slider(p, pos.wrapping_add(by), notify)
    }

    /// [soft 0x083bb9ec SI_scrollbarPageUp]: back by a page.
    pub fn si_scrollbar_page_up(&mut self, p: P<Scrollbar>, notify: i16) {
        let (pos, page) = (self.m.i32(p.addr() + 0x1c), self.m.i32(p.addr() + 0x20));
        self.si_scrollbar_set_slider(p, pos.wrapping_sub(page), notify)
    }

    /// [soft 0x083bba14 SI_scrollbarPageDown]: forward by a page.
    pub fn si_scrollbar_page_down(&mut self, p: P<Scrollbar>, notify: i16) {
        let (pos, page) = (self.m.i32(p.addr() + 0x1c), self.m.i32(p.addr() + 0x20));
        self.si_scrollbar_set_slider(p, page.wrapping_add(pos), notify)
    }

    /// [soft 0x083bba38 SI_scrollbarPageLeft]: back by a page.
    pub fn si_scrollbar_page_left(&mut self, p: P<Scrollbar>, notify: i16) {
        let (pos, page) = (self.m.i32(p.addr() + 0x1c), self.m.i32(p.addr() + 0x20));
        self.si_scrollbar_set_slider(p, pos.wrapping_sub(page), notify)
    }

    /// [soft 0x083bba60 SI_scrollbarPageRight]: forward by a page.
    pub fn si_scrollbar_page_right(&mut self, p: P<Scrollbar>, notify: i16) {
        let (pos, page) = (self.m.i32(p.addr() + 0x1c), self.m.i32(p.addr() + 0x20));
        self.si_scrollbar_set_slider(p, page.wrapping_add(pos), notify)
    }

    /// Draw the bar, in the front buffer when `front` is non-zero [soft 0x083bbe60 SI_scrollbarDraw].
    pub fn si_scrollbar_draw(&mut self, p: P<Scrollbar>, front: i16) {
        if front != 0 {
            self.o.gl.frontbuffer(&mut self.m, 1);
        }
        self.scb_draw(p);
        if front != 0 {
            self.o.gl.frontbuffer(&mut self.m, 0);
        }
    }

    /// The slider's offset in pixels from the bar's start and its size: the integers
    /// the two products `pos * (length - 0x27) / range` and `visible * (length - 0x27) / range`
    /// (the unsigned values are loaded as 64-bit integers, multiplied and divided in the
    /// x87's extended precision, and truncated through the control word's rounding bits).
    fn scb_slider_metrics(&self, sb: &Scrollbar) -> (i32, i32) {
        let span = F80::from_i32(sb.length.wrapping_sub(0x27));
        let range = F80::from_i64(sb.range as i64);
        let off = ((F80::from_i64(sb.pos as i64) * span) / range).to_i64(Round::Zero) as i32;
        let size = ((span * F80::from_i64(sb.visible as i64)) / range).to_i64(Round::Zero) as i32;
        (off.wrapping_add(1), size)
    }

    /// The bar itself [soft 0x083bbea4 scbDraw]: a frame, the two arrow boxes (the pressed
    /// one filled) and the slider.
    pub fn scb_draw(&mut self, p: P<Scrollbar>) {
        let sb = self.m[p];
        if self.m.i16(sb.grafport.wrapping_add(6)) == 0 {
            return;
        }
        let vertical = sb.vertical != 0;
        let right = if vertical { sb.x.wrapping_add(0x13) } else { sb.x.wrapping_add(sb.length).wrapping_sub(1) };
        let bottom = if vertical { sb.y.wrapping_sub(sb.length).wrapping_add(1) } else { sb.y.wrapping_sub(0x13) };
        let (off, size) = self.scb_slider_metrics(&sb);
        let (viewport, text) = (self.m[SI_WINDOW_VIEWPORT] as u16, self.m[SI_WINDOW_TEXT] as u16);
        self.o.gl.color(&mut self.m, viewport);
        self.o.gl.sboxfi(&mut self.m, sb.x, sb.y, right, bottom);
        self.o.gl.color(&mut self.m, text);
        self.o.gl.sboxi(&mut self.m, sb.x, sb.y, right, bottom);
        if !vertical {
            self.o.gl.sboxi(&mut self.m, right.wrapping_sub(0x26), bottom, right.wrapping_sub(0x13), bottom.wrapping_add(0x13));
        } else {
            self.o.gl.sboxi(&mut self.m, right.wrapping_sub(0x13), bottom.wrapping_add(0x26), right, bottom.wrapping_add(0x13));
        }
        let top_arrow = bottom.wrapping_add(0x13);
        let part = sb.part;
        if !vertical {
            // the left box: filled when pressed, then its arrowhead
            self.o.gl.color(&mut self.m, text);
            if part == 3 {
                self.o.gl.sboxfi(&mut self.m, right.wrapping_sub(0x26), sb.y, right.wrapping_sub(0x13), bottom);
                self.o.gl.color(&mut self.m, viewport);
            }
            let (x, y) = (right.wrapping_sub(0x18), bottom.wrapping_add(5));
            self.scb_triangle([x, y], [x, y.wrapping_add(10)], [x.wrapping_sub(10), y.wrapping_add(5)]);
            // the right box
            self.o.gl.color(&mut self.m, text);
            if part == 4 {
                self.o.gl.sboxfi(&mut self.m, right.wrapping_sub(0x13), sb.y, right.wrapping_sub(1), bottom);
                self.o.gl.color(&mut self.m, viewport);
            }
            let (x, y) = (right.wrapping_sub(0xe), bottom.wrapping_add(5));
            self.scb_triangle([x, y], [x, y.wrapping_add(10)], [x.wrapping_add(10), y.wrapping_add(5)]);
        } else {
            self.o.gl.color(&mut self.m, text);
            if part == 1 {
                self.o.gl.sboxfi(&mut self.m, sb.x.wrapping_add(1), bottom.wrapping_add(0x26), right.wrapping_sub(1), top_arrow);
                self.o.gl.color(&mut self.m, viewport);
            }
            let (x, y) = (sb.x.wrapping_add(5), bottom.wrapping_add(0x18));
            self.scb_triangle([x, y], [x.wrapping_add(10), y], [x.wrapping_add(5), y.wrapping_add(10)]);
            self.o.gl.color(&mut self.m, text);
            if part == 2 {
                self.o.gl.sboxfi(&mut self.m, sb.x.wrapping_add(1), top_arrow, right.wrapping_sub(1), bottom);
                self.o.gl.color(&mut self.m, viewport);
            }
            let (x, y) = (sb.x.wrapping_add(5), bottom.wrapping_add(0xf));
            self.scb_triangle([x, y], [x.wrapping_add(10), y], [x.wrapping_add(5), y.wrapping_sub(10)]);
        }
        // the slider, in the view background colour
        let back = self.m[SI_VIEW_BACK] as u16;
        self.o.gl.color(&mut self.m, back);
        let (x1, y1, x2, y2);
        if !vertical {
            let start = off.wrapping_add(sb.x);
            let (mut a, mut e) = (start, size.wrapping_sub(1).wrapping_add(start));
            if size < 5 {
                e = start.wrapping_add(4);
                if right.wrapping_sub(0x27) < e {
                    let d = start.wrapping_add(0x2b).wrapping_sub(right);
                    a = start.wrapping_sub(d);
                    e = e.wrapping_sub(d);
                }
            }
            (x1, y1, x2, y2) = (a, sb.y.wrapping_sub(3), e, bottom.wrapping_add(3));
        } else {
            let mut base = sb.y.wrapping_sub(off);
            let mut lo = base.wrapping_sub(size).wrapping_add(1);
            if size < 5 {
                lo = base.wrapping_sub(4);
                if lo < bottom.wrapping_add(0x27) {
                    let d = bottom.wrapping_sub(base.wrapping_sub(0x2b));
                    base = base.wrapping_add(d);
                    lo = lo.wrapping_add(d);
                }
            }
            (x1, y1, x2, y2) = (sb.x.wrapping_add(3), base, right.wrapping_sub(3), lo);
        }
        self.o.gl.sboxfi(&mut self.m, x1, y1.wrapping_add(1), x2, y2.wrapping_sub(1));
    }

    fn scb_triangle(&mut self, a: [i32; 2], b: [i32; 2], c: [i32; 2]) {
        self.o.gl.bgnpolygon(&mut self.m);
        self.o.gl.v2i(&mut self.m, a);
        self.o.gl.v2i(&mut self.m, b);
        self.o.gl.v2i(&mut self.m, c);
        self.o.gl.endpolygon(&mut self.m);
    }

    /// Which part of the bar the point `(x, y)` is in [soft 0x083bc4dc scbDetectPart]: 6 before
    /// the slider, 5 on it, 7 after it, 1 to 4 the arrow boxes (1 and 2 vertical: top, bottom;
    /// 3 and 4 horizontal: left, right). The original leaves a second word in edx that
    /// nothing reads.
    pub fn scb_detect_part(&mut self, p: P<Scrollbar>, x: i32, y: i32) -> i32 {
        let sb = self.m[p];
        let vertical = sb.vertical != 0;
        let (point, start) = if vertical { (y.wrapping_neg(), sb.y.wrapping_neg()) } else { (x, sb.x) };
        let end = if vertical { !sb.y.wrapping_sub(sb.length) } else { sb.length.wrapping_add(sb.x).wrapping_sub(1) };
        let (off, mut size) = self.scb_slider_metrics(&sb);
        let mut off = off;
        if size < 5 {
            let e = start.wrapping_add(off);
            size = 5;
            if e.wrapping_add(5) > end.wrapping_sub(0x28) {
                off = off.wrapping_sub(e.wrapping_add(0x2d).wrapping_sub(end));
            }
        }
        let lo = start.wrapping_add(off);
        if point < lo {
            6
        } else if point < lo.wrapping_add(size) {
            5
        } else if point < end.wrapping_sub(0x26) {
            7
        } else if point < end.wrapping_sub(0x13) {
            if vertical { 1 } else { 3 }
        } else if vertical {
            2
        } else {
            4
        }
    }
}
