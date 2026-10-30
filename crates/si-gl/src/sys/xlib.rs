// SPDX-License-Identifier: MIT
//! The Xlib calls of the layer.

use super::{outside, Ptr, Xid};
use crate::rec::{out_bytes, out_u, out_words, ret_u, Val};

/// An `XEvent` as `XNextEvent` fills it: 24 longs (96 bytes on i386). The layer reads only
/// the words named by the accessors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XEvent(pub [u32; 24]);

impl Default for XEvent {
    fn default() -> Self {
        XEvent([0; 24])
    }
}

impl XEvent {
    /// Offset 0: the event type.
    pub fn kind(&self) -> i32 {
        self.0[0] as i32
    }
    /// Offset 0x10: `window` of key, button, focus, property and client-message events
    /// (and `event` of a `ConfigureNotify`).
    pub fn word_10(&self) -> u32 {
        self.0[4]
    }
    /// Offset 0x14: `window` of a `ConfigureNotify`, `atom` of a `PropertyNotify`,
    /// `message_type` of a client message, and **`x` of an `Expose`**.
    pub fn word_14(&self) -> u32 {
        self.0[5]
    }
    /// Offset 0x1c: `state` of a `PropertyNotify`, `data.l[0]` of a client message.
    pub fn word_1c(&self) -> u32 {
        self.0[7]
    }
    /// Offset 0x24: `count` of an `Expose`.
    pub fn word_24(&self) -> u32 {
        self.0[9]
    }
    /// Offset 0x34: `keycode` of a key event, `button` of a button event.
    pub fn word_34(&self) -> u32 {
        self.0[13]
    }
}

/// The `XSetWindowAttributes` fields the layer sets (the others are never selected by a
/// value mask it passes). Offsets are those of the i386 structure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XSetWindowAttributes {
    /// +0x00, mask `CWBackPixmap` (0x1)
    pub background_pixmap: Xid,
    /// +0x0c, mask `CWBorderPixel` (0x8)
    pub border_pixel: u32,
    /// +0x10, mask `CWBitGravity` (0x10)
    pub bit_gravity: i32,
    /// +0x28, mask `CWEventMask` (0x800)
    pub event_mask: u32,
    /// +0x30, mask `CWOverrideRedirect` (0x200)
    pub override_redirect: i32,
    /// +0x34, mask `CWColormap` (0x2000)
    pub colormap: Xid,
}

impl XSetWindowAttributes {
    /// The fields selected by `valuemask`, in mask-bit order, for the log (the original
    /// leaves the unselected ones uninitialised).
    fn selected(&self, valuemask: u32) -> Val {
        let mut w = Vec::new();
        if valuemask & 0x1 != 0 {
            w.push(self.background_pixmap);
        }
        if valuemask & 0x8 != 0 {
            w.push(self.border_pixel);
        }
        if valuemask & 0x10 != 0 {
            w.push(self.bit_gravity as u32);
        }
        if valuemask & 0x200 != 0 {
            w.push(self.override_redirect as u32);
        }
        if valuemask & 0x800 != 0 {
            w.push(self.event_mask);
        }
        if valuemask & 0x2000 != 0 {
            w.push(self.colormap);
        }
        Val::W(w)
    }
}

/// `XSizeHints` (18 longs, 0x48 bytes), as `XAllocSizeHints` hands it out zeroed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XSizeHints {
    pub flags: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub min_width: i32,
    pub min_height: i32,
    pub max_width: i32,
    pub max_height: i32,
    pub width_inc: i32,
    pub height_inc: i32,
    pub min_aspect_x: i32,
    pub min_aspect_y: i32,
    pub max_aspect_x: i32,
    pub max_aspect_y: i32,
    pub base_width: i32,
    pub base_height: i32,
    pub win_gravity: i32,
}

impl XSizeHints {
    pub fn words(&self) -> [u32; 18] {
        [
            self.flags,
            self.x as u32,
            self.y as u32,
            self.width as u32,
            self.height as u32,
            self.min_width as u32,
            self.min_height as u32,
            self.max_width as u32,
            self.max_height as u32,
            self.width_inc as u32,
            self.height_inc as u32,
            self.min_aspect_x as u32,
            self.min_aspect_y as u32,
            self.max_aspect_x as u32,
            self.max_aspect_y as u32,
            self.base_width as u32,
            self.base_height as u32,
            self.win_gravity as u32,
        ]
    }
}

/// `XWindowChanges`; the layer only ever selects `border_width` (mask 0x10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XWindowChanges {
    pub border_width: i32,
}

/// The `XTextProperty` that `XStringListToTextProperty` fills and `XSetWMName` reads:
/// four words the layer does not look into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XTextProperty(pub [u32; 4]);

outside! {
    /// Xlib, as the layer calls it.
    pub trait Xlib, recorded by RecXlib {
        fn XInitThreads() -> i32;
        /// `XCreateColormap(dpy, w, visual, alloc)`
        fn XCreateColormap(dpy: Ptr, w: Xid, visual: Ptr, alloc: i32) -> Xid;
        fn XFreeColormap(dpy: Ptr, colormap: Xid) -> i32;
        fn XSync(dpy: Ptr, discard: i32) -> i32;
        fn XFree(data: Ptr) -> i32;
        fn XMapWindow(dpy: Ptr, w: Xid) -> i32;
        fn XUnmapWindow(dpy: Ptr, w: Xid) -> i32;
        fn XRaiseWindow(dpy: Ptr, w: Xid) -> i32;
        fn XLowerWindow(dpy: Ptr, w: Xid) -> i32;
        fn XDestroyWindow(dpy: Ptr, w: Xid) -> i32;
        fn XMoveWindow(dpy: Ptr, w: Xid, x: i32, y: i32) -> i32;
        fn XMoveResizeWindow(dpy: Ptr, w: Xid, x: i32, y: i32, width: i32, height: i32) -> i32;
        /// `XSetWMProtocols(dpy, w, &atom, 1)` with the one atom the layer registers.
        fn XSetWMProtocols(dpy: Ptr, w: Xid, protocol: Xid, count: i32) -> i32;
        /// `XKeycodeToKeysym(dpy, keycode, index)`
        fn XKeycodeToKeysym(dpy: Ptr, keycode: u32, index: i32) -> u32;
        fn XEventsQueued(dpy: Ptr, mode: i32) -> i32;
        fn XLockDisplay(dpy: Ptr);
        fn XUnlockDisplay(dpy: Ptr);
        fn XAutoRepeatOn(dpy: Ptr) -> i32;
        fn XAutoRepeatOff(dpy: Ptr) -> i32;
        /// `XGrabPointer(dpy, grab_window, owner_events, event_mask, pointer_mode,
        /// keyboard_mode, confine_to, cursor, time)`
        fn XGrabPointer(dpy: Ptr, grab_window: Xid, owner_events: i32, event_mask: u32, pointer_mode: i32, keyboard_mode: i32, confine_to: Xid, cursor: Xid, time: u32) -> i32;
        fn XUngrabPointer(dpy: Ptr, time: u32) -> i32;
        /// `XWarpPointer(dpy, src_w, dest_w, src_x, src_y, src_width, src_height, dest_x, dest_y)`
        fn XWarpPointer(dpy: Ptr, src_w: Xid, dest_w: Xid, src_x: i32, src_y: i32, src_width: u32, src_height: u32, dest_x: i32, dest_y: i32) -> i32;
        fn XFreePixmap(dpy: Ptr, pixmap: Xid) -> i32;
        fn XFreeCursor(dpy: Ptr, cursor: Xid) -> i32;
        fn XDefineCursor(dpy: Ptr, w: Xid, cursor: Xid) -> i32;
        fn XFreeGC(dpy: Ptr, gc: Ptr) -> i32;
        /// `XDrawRectangle(dpy, d, gc, x, y, width, height)`
        fn XDrawRectangle(dpy: Ptr, d: Xid, gc: Ptr, x: i32, y: i32, width: u32, height: u32) -> i32;
        /// `XGetImage(dpy, d, x, y, width, height, plane_mask, format)`
        fn XGetImage(dpy: Ptr, d: Xid, x: i32, y: i32, width: u32, height: u32, plane_mask: u32, format: i32) -> Ptr;
        /// `XPutImage(dpy, d, gc, image, src_x, src_y, dest_x, dest_y, width, height)`
        fn XPutImage(dpy: Ptr, d: Xid, gc: Ptr, image: Ptr, src_x: i32, src_y: i32, dest_x: i32, dest_y: i32, width: u32, height: u32) -> i32;
        /// `XGetSubImage(dpy, d, x, y, width, height, plane_mask, format, dest_image, dest_x, dest_y)`
        fn XGetSubImage(dpy: Ptr, d: Xid, x: i32, y: i32, width: u32, height: u32, plane_mask: u32, format: i32, dest_image: Ptr, dest_x: i32, dest_y: i32) -> Ptr;
        /// The call through an `XImage`'s `f.destroy_image` pointer (offset 0x44): what the
        /// `XDestroyImage` macro expands to.
        fn XDestroyImage(image: Ptr) -> i32;
        /// `XAllocSizeHints()`: the address of the zeroed structure (the layer fills its own
        /// copy, [`XSizeHints`], and hands that to `XSetWMNormalHints`).
        fn XAllocSizeHints() -> Ptr;
        manual:
        /// `XOpenDisplay(name)`
        fn XOpenDisplay(&mut self, name: &[u8]) -> Ptr {
            let a = self.log.borrow_mut().call("XOpenDisplay", vec![Val::S(name.to_vec())]);
            ret_u(&a, 0)
        }
        /// `XInternAtom(dpy, name, only_if_exists)`
        fn XInternAtom(&mut self, dpy: Ptr, name: &[u8], only_if_exists: i32) -> Xid {
            let a = self.log.borrow_mut().call(
                "XInternAtom",
                vec![Val::P(dpy), Val::S(name.to_vec()), Val::U(only_if_exists as u32)],
            );
            ret_u(&a, 0)
        }
        /// `XCreateWindow(dpy, parent, x, y, width, height, border_width, depth, class,
        /// visual, valuemask, &attributes)`
        fn XCreateWindow(
            &mut self,
            dpy: Ptr,
            parent: Xid,
            x: i32,
            y: i32,
            width: u32,
            height: u32,
            border_width: u32,
            depth: i32,
            class: u32,
            visual: Ptr,
            valuemask: u32,
            attributes: &XSetWindowAttributes,
        ) -> Xid {
            let a = self.log.borrow_mut().call(
                "XCreateWindow",
                vec![
                    Val::P(dpy),
                    Val::U(parent),
                    Val::U(x as u32),
                    Val::U(y as u32),
                    Val::U(width),
                    Val::U(height),
                    Val::U(border_width),
                    Val::U(depth as u32),
                    Val::U(class),
                    Val::P(visual),
                    Val::U(valuemask),
                    attributes.selected(valuemask),
                ],
            );
            ret_u(&a, 0)
        }
        /// `XChangeWindowAttributes(dpy, w, valuemask, &attributes)`
        fn XChangeWindowAttributes(&mut self, dpy: Ptr, w: Xid, valuemask: u32, attributes: &XSetWindowAttributes) -> i32 {
            let a = self.log.borrow_mut().call(
                "XChangeWindowAttributes",
                vec![Val::P(dpy), Val::U(w), Val::U(valuemask), attributes.selected(valuemask)],
            );
            ret_u(&a, 0) as i32
        }
        /// `XGetGeometry(dpy, d, &root, &x, &y, &width, &height, &border_width, &depth)`:
        /// the status, then the seven values written. Answer: `[status, W(seven words)]`.
        fn XGetGeometry(&mut self, dpy: Ptr, d: Xid) -> (i32, [u32; 7]) {
            let a = self.log.borrow_mut().call(
                "XGetGeometry",
                vec![Val::P(dpy), Val::U(d), Val::Out, Val::Out, Val::Out, Val::Out, Val::Out, Val::Out, Val::Out],
            );
            let mut o = [0u32; 7];
            for (p, w) in o.iter_mut().zip(out_words(&a, 1)) {
                *p = w;
            }
            (ret_u(&a, 1) as i32, o)
        }
        /// `XTranslateCoordinates(dpy, src_w, dest_w, src_x, src_y, &dest_x, &dest_y,
        /// &child)`: the result, then (dest_x, dest_y, child). Answer: `[ret, W(three)]`.
        fn XTranslateCoordinates(&mut self, dpy: Ptr, src_w: Xid, dest_w: Xid, src_x: i32, src_y: i32) -> (i32, [u32; 3]) {
            let a = self.log.borrow_mut().call(
                "XTranslateCoordinates",
                vec![Val::P(dpy), Val::U(src_w), Val::U(dest_w), Val::U(src_x as u32), Val::U(src_y as u32), Val::Out, Val::Out, Val::Out],
            );
            let mut o = [0u32; 3];
            for (p, w) in o.iter_mut().zip(out_words(&a, 1)) {
                *p = w;
            }
            (ret_u(&a, 1) as i32, o)
        }
        /// `XQueryPointer(dpy, w, &root, &child, &root_x, &root_y, &win_x, &win_y, &mask)`:
        /// the result, then the seven values written. Answer: `[ret, W(seven words)]`.
        fn XQueryPointer(&mut self, dpy: Ptr, w: Xid) -> (i32, [u32; 7]) {
            let a = self.log.borrow_mut().call(
                "XQueryPointer",
                vec![Val::P(dpy), Val::U(w), Val::Out, Val::Out, Val::Out, Val::Out, Val::Out, Val::Out, Val::Out],
            );
            let mut o = [0u32; 7];
            for (p, w) in o.iter_mut().zip(out_words(&a, 1)) {
                *p = w;
            }
            (ret_u(&a, 1) as i32, o)
        }
        /// `XQueryKeymap(dpy, keys)`: the 32 bytes written. Answer: `[ret, B(32 bytes)]`.
        fn XQueryKeymap(&mut self, dpy: Ptr) -> [u8; 32] {
            let a = self.log.borrow_mut().call("XQueryKeymap", vec![Val::P(dpy), Val::Out]);
            let mut o = [0u8; 32];
            for (p, b) in o.iter_mut().zip(out_bytes(&a, 1)) {
                *p = b;
            }
            o
        }
        /// `XNextEvent(dpy, &event)`. Answer: `[ret, W(up to 24 words)]`.
        fn XNextEvent(&mut self, dpy: Ptr) -> XEvent {
            let a = self.log.borrow_mut().call("XNextEvent", vec![Val::P(dpy), Val::Out]);
            let mut e = XEvent::default();
            for (p, w) in e.0.iter_mut().zip(out_words(&a, 1)) {
                *p = w;
            }
            e
        }
        /// `XCheckMaskEvent(dpy, mask, &event)`: the layer discards the event.
        fn XCheckMaskEvent(&mut self, dpy: Ptr, mask: u32) -> i32 {
            let a = self.log.borrow_mut().call("XCheckMaskEvent", vec![Val::P(dpy), Val::U(mask), Val::Out]);
            ret_u(&a, 0) as i32
        }
        /// `XLookupString(&event, buffer, 1, NULL, NULL)`: the count and the one byte the
        /// buffer has room for. Answer: `[count, U(byte)]`.
        fn XLookupString(&mut self, event: &XEvent) -> (i32, u8) {
            let a = self.log.borrow_mut().call(
                "XLookupString",
                vec![Val::W(event.0.to_vec()), Val::Out, Val::U(1), Val::U(0), Val::U(0)],
            );
            (ret_u(&a, 0) as i32, out_u(&a, 1, 0) as u8)
        }
        /// `XGetWindowProperty(dpy, w, property, 0, 8, False, AnyPropertyType, &type,
        /// &format, &nitems, &bytes_after, &data)`: the status, and the first long of the
        /// returned data (`None` when the data pointer comes back null). The layer never
        /// frees the data. Answer: `[status, U(first long)]`; without the second value the
        /// pointer is null.
        fn XGetWindowProperty(&mut self, dpy: Ptr, w: Xid, property: Xid, offset: i32, length: i32, delete: i32, req_type: Xid) -> (i32, Option<u32>) {
            let a = self.log.borrow_mut().call(
                "XGetWindowProperty",
                vec![
                    Val::P(dpy),
                    Val::U(w),
                    Val::U(property),
                    Val::U(offset as u32),
                    Val::U(length as u32),
                    Val::U(delete as u32),
                    Val::U(req_type),
                    Val::Out,
                    Val::Out,
                    Val::Out,
                    Val::Out,
                    Val::Out,
                ],
            );
            (ret_u(&a, 0) as i32, a.as_ref().and_then(|v| v.get(1)).map(Val::u))
        }
        /// `XStringListToTextProperty(&name, 1, &prop)`: the status and the property.
        /// Answer: `[status, W(four words)]`.
        fn XStringListToTextProperty(&mut self, name: &[u8], count: i32) -> (i32, XTextProperty) {
            let a = self.log.borrow_mut().call(
                "XStringListToTextProperty",
                vec![Val::S(name.to_vec()), Val::U(count as u32), Val::Out],
            );
            let mut p = XTextProperty::default();
            for (d, w) in p.0.iter_mut().zip(out_words(&a, 1)) {
                *d = w;
            }
            (ret_u(&a, 1) as i32, p)
        }
        /// `XSetWMName(dpy, w, &prop)`
        fn XSetWMName(&mut self, dpy: Ptr, w: Xid, prop: &XTextProperty) {
            self.log.borrow_mut().call("XSetWMName", vec![Val::P(dpy), Val::U(w), Val::W(prop.0.to_vec())]);
        }
        /// `XChangeProperty(dpy, w, property, type, 32, mode, data, nelements)` with data
        /// of 32-bit items.
        fn XChangeProperty(&mut self, dpy: Ptr, w: Xid, property: Xid, kind: Xid, format: i32, mode: i32, data: &[u32], nelements: i32) -> i32 {
            let a = self.log.borrow_mut().call(
                "XChangeProperty",
                vec![
                    Val::P(dpy),
                    Val::U(w),
                    Val::U(property),
                    Val::U(kind),
                    Val::U(format as u32),
                    Val::U(mode as u32),
                    Val::W(data.to_vec()),
                    Val::U(nelements as u32),
                ],
            );
            ret_u(&a, 0) as i32
        }
        /// `XReconfigureWMWindow(dpy, w, screen, value_mask, &changes)` with mask 0x10
        /// (`CWBorderWidth`).
        fn XReconfigureWMWindow(&mut self, dpy: Ptr, w: Xid, screen: i32, value_mask: u32, changes: &XWindowChanges) -> i32 {
            let a = self.log.borrow_mut().call(
                "XReconfigureWMWindow",
                vec![Val::P(dpy), Val::U(w), Val::U(screen as u32), Val::U(value_mask), Val::W(vec![changes.border_width as u32])],
            );
            ret_u(&a, 0) as i32
        }
        /// `XSetWMNormalHints(dpy, w, hints)`
        fn XSetWMNormalHints(&mut self, dpy: Ptr, w: Xid, hints: &XSizeHints) {
            self.log.borrow_mut().call("XSetWMNormalHints", vec![Val::P(dpy), Val::U(w), Val::W(hints.words().to_vec())]);
        }
        /// `XCreatePixmapFromBitmapData(dpy, d, data, width, height, fg, bg, depth)`;
        /// `data` is the rows the call reads: `height` rows of `(width + 7) / 8` bytes.
        fn XCreatePixmapFromBitmapData(&mut self, dpy: Ptr, d: Xid, data: &[u8], width: u32, height: u32, fg: u32, bg: u32, depth: u32) -> Xid {
            let a = self.log.borrow_mut().call(
                "XCreatePixmapFromBitmapData",
                vec![Val::P(dpy), Val::U(d), Val::B(data.to_vec()), Val::U(width), Val::U(height), Val::U(fg), Val::U(bg), Val::U(depth)],
            );
            ret_u(&a, 0)
        }
        /// `XCreatePixmapCursor(dpy, source, mask, &fg, &bg, x, y)`; each colour is the 12
        /// bytes of an `XColor` (pixel, red, green, blue, flags, pad).
        fn XCreatePixmapCursor(&mut self, dpy: Ptr, source: Xid, mask: Xid, fg: &[u8; 12], bg: &[u8; 12], x: u32, y: u32) -> Xid {
            let a = self.log.borrow_mut().call(
                "XCreatePixmapCursor",
                vec![Val::P(dpy), Val::U(source), Val::U(mask), Val::B(fg.to_vec()), Val::B(bg.to_vec()), Val::U(x), Val::U(y)],
            );
            ret_u(&a, 0)
        }
        /// `XCreateGC(dpy, d, GCForeground | GCLineStyle, &values)`: the two fields that
        /// mask selects.
        fn XCreateGC(&mut self, dpy: Ptr, d: Xid, valuemask: u32, foreground: u32, line_style: i32) -> Ptr {
            let a = self.log.borrow_mut().call(
                "XCreateGC",
                vec![Val::P(dpy), Val::U(d), Val::U(valuemask), Val::W(vec![foreground, line_style as u32])],
            );
            ret_u(&a, 0)
        }

        // ---- memory, not calls: structures Xlib owns, read through the pointers

        /// `dpy->default_screen` (offset 0x84).
        fn default_screen(&mut self, dpy: Ptr) -> i32 {
            out_u(&self.log.borrow_mut().take_answer("default_screen"), 0, 0) as i32
        }
        /// One long of screen `screen` of the display (`dpy->screens[screen]`, 0x50 bytes
        /// each, table at offset 0x8c): 0x08 root window, 0x0c width, 0x10 height, 0x14
        /// width in millimetres, 0x18 height in millimetres, 0x2c default GC, 0x34 white
        /// pixel, 0x38 black pixel.
        fn screen_word(&mut self, dpy: Ptr, screen: i32, offset: u32) -> u32 {
            let name = match offset {
                0x08 => "screen.root",
                0x0c => "screen.width",
                0x10 => "screen.height",
                0x14 => "screen.mwidth",
                0x18 => "screen.mheight",
                0x2c => "screen.default_gc",
                0x34 => "screen.white_pixel",
                0x38 => "screen.black_pixel",
                _ => "screen.other",
            };
            out_u(&self.log.borrow_mut().take_answer(name), 0, 0)
        }
        /// One long of an `XVisualInfo`: 0x00 visual, 0x0c depth.
        fn visual_info_word(&mut self, vi: Ptr, offset: u32) -> u32 {
            let name = if offset == 0 { "visualinfo.visual" } else { "visualinfo.depth" };
            out_u(&self.log.borrow_mut().take_answer(name), 0, 0)
        }
        /// One long of an `XImage`: 0x28 bytes per line, 0x2c bits per pixel.
        fn image_word(&mut self, image: Ptr, offset: u32) -> u32 {
            let name = if offset == 0x28 { "image.bytes_per_line" } else { "image.bits_per_pixel" };
            out_u(&self.log.borrow_mut().take_answer(name), 0, 0)
        }
        /// `memmove` inside an `XImage`'s pixel data (`image->data`, offset 0x10): the
        /// layer shifts the saved picture itself while a dialog is dragged.
        fn image_data_move(&mut self, image: Ptr, dest: i32, src: i32, len: i32) {
            // logged (unlike the other accessors) so that the offsets can be compared with
            // the original's `memmove` arguments
            self.log.borrow_mut().call("image_data_move", vec![Val::U(image), Val::U(dest as u32), Val::U(src as u32), Val::U(len as u32)]);
        }
    }
}
