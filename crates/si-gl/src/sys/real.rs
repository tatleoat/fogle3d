// SPDX-License-Identifier: MIT
//! A sketch of the real back end: the imported symbols as `extern "C"` declarations, with
//! the C prototypes of Xlib, GLX, OpenGL 1.1 and GLU. **Declarations only**: nothing here
//! implements the traits of this module yet, nothing is called, and no window is opened.
//! Compiled only with the `system` feature.
//!
//! What an implementation still needs is listed in finding 0016 ("Open"): the pointer
//! types below are the libraries' own (64-bit on today's machines), while the layer's state
//! keeps them as the opaque 32-bit values of the original; the accessors of [`super::Xlib`]
//! (`screen_word`, `visual_info_word`, `image_word`) have to be answered with the Xlib
//! macros; and the selection buffer has to live on this side.
#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::os::raw::{c_char, c_double, c_float, c_int, c_long, c_uchar, c_uint, c_ulong, c_ushort, c_void};

pub type Display = c_void;
pub type XID = c_ulong;
pub type GLXContext = *mut c_void;
pub type GLenum = c_uint;

#[link(name = "X11")]
extern "C" {
    pub fn XInitThreads() -> c_int;
    pub fn XOpenDisplay(name: *const c_char) -> *mut Display;
    pub fn XInternAtom(dpy: *mut Display, name: *const c_char, only_if_exists: c_int) -> XID;
    pub fn XCreateColormap(dpy: *mut Display, w: XID, visual: *mut c_void, alloc: c_int) -> XID;
    pub fn XFreeColormap(dpy: *mut Display, colormap: XID) -> c_int;
    pub fn XCreateWindow(dpy: *mut Display, parent: XID, x: c_int, y: c_int, width: c_uint, height: c_uint, border_width: c_uint, depth: c_int, class: c_uint, visual: *mut c_void, valuemask: c_ulong, attributes: *mut c_void) -> XID;
    pub fn XChangeWindowAttributes(dpy: *mut Display, w: XID, valuemask: c_ulong, attributes: *mut c_void) -> c_int;
    pub fn XDestroyWindow(dpy: *mut Display, w: XID) -> c_int;
    pub fn XMapWindow(dpy: *mut Display, w: XID) -> c_int;
    pub fn XUnmapWindow(dpy: *mut Display, w: XID) -> c_int;
    pub fn XRaiseWindow(dpy: *mut Display, w: XID) -> c_int;
    pub fn XLowerWindow(dpy: *mut Display, w: XID) -> c_int;
    pub fn XMoveWindow(dpy: *mut Display, w: XID, x: c_int, y: c_int) -> c_int;
    pub fn XMoveResizeWindow(dpy: *mut Display, w: XID, x: c_int, y: c_int, width: c_uint, height: c_uint) -> c_int;
    pub fn XSync(dpy: *mut Display, discard: c_int) -> c_int;
    pub fn XFree(data: *mut c_void) -> c_int;
    pub fn XGetGeometry(dpy: *mut Display, d: XID, root: *mut XID, x: *mut c_int, y: *mut c_int, width: *mut c_uint, height: *mut c_uint, border_width: *mut c_uint, depth: *mut c_uint) -> c_int;
    pub fn XTranslateCoordinates(dpy: *mut Display, src_w: XID, dest_w: XID, src_x: c_int, src_y: c_int, dest_x: *mut c_int, dest_y: *mut c_int, child: *mut XID) -> c_int;
    pub fn XStringListToTextProperty(list: *mut *mut c_char, count: c_int, prop: *mut c_void) -> c_int;
    pub fn XSetWMName(dpy: *mut Display, w: XID, prop: *mut c_void);
    pub fn XSetWMProtocols(dpy: *mut Display, w: XID, protocols: *mut XID, count: c_int) -> c_int;
    pub fn XAllocSizeHints() -> *mut c_void;
    pub fn XSetWMNormalHints(dpy: *mut Display, w: XID, hints: *mut c_void);
    pub fn XChangeProperty(dpy: *mut Display, w: XID, property: XID, kind: XID, format: c_int, mode: c_int, data: *const c_uchar, nelements: c_int) -> c_int;
    pub fn XReconfigureWMWindow(dpy: *mut Display, w: XID, screen: c_int, value_mask: c_uint, changes: *mut c_void) -> c_int;
    pub fn XGetWindowProperty(dpy: *mut Display, w: XID, property: XID, offset: c_long, length: c_long, delete: c_int, req_type: XID, actual_type: *mut XID, actual_format: *mut c_int, nitems: *mut c_ulong, bytes_after: *mut c_ulong, prop: *mut *mut c_uchar) -> c_int;
    pub fn XKeycodeToKeysym(dpy: *mut Display, keycode: c_uchar, index: c_int) -> c_ulong;
    pub fn XEventsQueued(dpy: *mut Display, mode: c_int) -> c_int;
    pub fn XNextEvent(dpy: *mut Display, event: *mut c_void) -> c_int;
    pub fn XCheckMaskEvent(dpy: *mut Display, mask: c_long, event: *mut c_void) -> c_int;
    pub fn XLookupString(event: *mut c_void, buffer: *mut c_char, bytes: c_int, keysym: *mut c_ulong, status: *mut c_void) -> c_int;
    pub fn XLockDisplay(dpy: *mut Display);
    pub fn XUnlockDisplay(dpy: *mut Display);
    pub fn XQueryKeymap(dpy: *mut Display, keys: *mut c_char) -> c_int;
    pub fn XQueryPointer(dpy: *mut Display, w: XID, root: *mut XID, child: *mut XID, root_x: *mut c_int, root_y: *mut c_int, win_x: *mut c_int, win_y: *mut c_int, mask: *mut c_uint) -> c_int;
    pub fn XAutoRepeatOn(dpy: *mut Display) -> c_int;
    pub fn XAutoRepeatOff(dpy: *mut Display) -> c_int;
    pub fn XGrabPointer(dpy: *mut Display, grab_window: XID, owner_events: c_int, event_mask: c_uint, pointer_mode: c_int, keyboard_mode: c_int, confine_to: XID, cursor: XID, time: c_ulong) -> c_int;
    pub fn XUngrabPointer(dpy: *mut Display, time: c_ulong) -> c_int;
    pub fn XWarpPointer(dpy: *mut Display, src_w: XID, dest_w: XID, src_x: c_int, src_y: c_int, src_width: c_uint, src_height: c_uint, dest_x: c_int, dest_y: c_int) -> c_int;
    pub fn XCreatePixmapFromBitmapData(dpy: *mut Display, d: XID, data: *mut c_char, width: c_uint, height: c_uint, fg: c_ulong, bg: c_ulong, depth: c_uint) -> XID;
    pub fn XCreatePixmapCursor(dpy: *mut Display, source: XID, mask: XID, fg: *mut c_void, bg: *mut c_void, x: c_uint, y: c_uint) -> XID;
    pub fn XFreePixmap(dpy: *mut Display, pixmap: XID) -> c_int;
    pub fn XFreeCursor(dpy: *mut Display, cursor: XID) -> c_int;
    pub fn XDefineCursor(dpy: *mut Display, w: XID, cursor: XID) -> c_int;
    pub fn XCreateGC(dpy: *mut Display, d: XID, valuemask: c_ulong, values: *mut c_void) -> *mut c_void;
    pub fn XFreeGC(dpy: *mut Display, gc: *mut c_void) -> c_int;
    pub fn XDrawRectangle(dpy: *mut Display, d: XID, gc: *mut c_void, x: c_int, y: c_int, width: c_uint, height: c_uint) -> c_int;
    pub fn XGetImage(dpy: *mut Display, d: XID, x: c_int, y: c_int, width: c_uint, height: c_uint, plane_mask: c_ulong, format: c_int) -> *mut c_void;
    pub fn XGetSubImage(dpy: *mut Display, d: XID, x: c_int, y: c_int, width: c_uint, height: c_uint, plane_mask: c_ulong, format: c_int, dest_image: *mut c_void, dest_x: c_int, dest_y: c_int) -> *mut c_void;
    pub fn XPutImage(dpy: *mut Display, d: XID, gc: *mut c_void, image: *mut c_void, src_x: c_int, src_y: c_int, dest_x: c_int, dest_y: c_int, width: c_uint, height: c_uint) -> c_int;
}

#[link(name = "GL")]
extern "C" {
    pub fn glXChooseVisual(dpy: *mut Display, screen: c_int, attribs: *mut c_int) -> *mut c_void;
    pub fn glXCreateContext(dpy: *mut Display, vis: *mut c_void, share: GLXContext, direct: c_int) -> GLXContext;
    pub fn glXDestroyContext(dpy: *mut Display, ctx: GLXContext);
    pub fn glXMakeCurrent(dpy: *mut Display, drawable: XID, ctx: GLXContext) -> c_int;
    pub fn glXGetCurrentDrawable() -> XID;
    pub fn glXGetConfig(dpy: *mut Display, vis: *mut c_void, attrib: c_int, value: *mut c_int) -> c_int;
    pub fn glXGetClientString(dpy: *mut Display, name: c_int) -> *const c_char;
    pub fn glXGetProcAddressARB(name: *const c_uchar) -> Option<unsafe extern "C" fn()>;
    pub fn glXSwapBuffers(dpy: *mut Display, drawable: XID);
    pub fn glXWaitGL();
    pub fn glXWaitX();

    pub fn glBegin(mode: GLenum);
    pub fn glEnd();
    pub fn glVertex2f(x: c_float, y: c_float);
    pub fn glVertex2d(x: c_double, y: c_double);
    pub fn glVertex3d(x: c_double, y: c_double, z: c_double);
    pub fn glVertex2dv(v: *const c_double);
    pub fn glVertex2fv(v: *const c_float);
    pub fn glVertex2iv(v: *const c_int);
    pub fn glVertex2sv(v: *const i16);
    pub fn glVertex3dv(v: *const c_double);
    pub fn glVertex3fv(v: *const c_float);
    pub fn glVertex3iv(v: *const c_int);
    pub fn glVertex3sv(v: *const i16);
    pub fn glVertex4dv(v: *const c_double);
    pub fn glVertex4fv(v: *const c_float);
    pub fn glVertex4iv(v: *const c_int);
    pub fn glVertex4sv(v: *const i16);
    pub fn glNormal3fv(v: *const c_float);
    pub fn glColor3f(r: c_float, g: c_float, b: c_float);
    pub fn glColor4f(r: c_float, g: c_float, b: c_float, a: c_float);
    pub fn glColor4d(r: c_double, g: c_double, b: c_double, a: c_double);
    pub fn glClearColor(r: c_float, g: c_float, b: c_float, a: c_float);
    pub fn glClearDepth(depth: c_double);
    pub fn glClearStencil(s: c_int);
    pub fn glClear(mask: c_uint);
    pub fn glDrawBuffer(mode: GLenum);
    pub fn glReadBuffer(mode: GLenum);
    pub fn glMatrixMode(mode: GLenum);
    pub fn glLoadIdentity();
    pub fn glLoadMatrixf(m: *const c_float);
    pub fn glMultMatrixf(m: *const c_float);
    pub fn glPushMatrix();
    pub fn glPopMatrix();
    pub fn glPushAttrib(mask: c_uint);
    pub fn glPopAttrib();
    pub fn glOrtho(left: c_double, right: c_double, bottom: c_double, top: c_double, near: c_double, far: c_double);
    pub fn glFrustum(left: c_double, right: c_double, bottom: c_double, top: c_double, near: c_double, far: c_double);
    pub fn glViewport(x: c_int, y: c_int, width: c_int, height: c_int);
    pub fn glScissor(x: c_int, y: c_int, width: c_int, height: c_int);
    pub fn glDepthRange(near: c_double, far: c_double);
    pub fn glDepthFunc(func: GLenum);
    pub fn glEnable(cap: GLenum);
    pub fn glDisable(cap: GLenum);
    pub fn glIsEnabled(cap: GLenum) -> c_uchar;
    pub fn glFinish();
    pub fn glFlush();
    pub fn glGetError() -> GLenum;
    pub fn glGetIntegerv(pname: GLenum, params: *mut c_int);
    pub fn glGetFloatv(pname: GLenum, params: *mut c_float);
    pub fn glGetDoublev(pname: GLenum, params: *mut c_double);
    pub fn glGetBooleanv(pname: GLenum, params: *mut c_uchar);
    pub fn glGetString(name: GLenum) -> *const c_uchar;
    pub fn glPixelStorei(pname: GLenum, param: c_int);
    pub fn glPixelZoom(xfactor: c_float, yfactor: c_float);
    pub fn glRasterPos2i(x: c_int, y: c_int);
    pub fn glRasterPos3d(x: c_double, y: c_double, z: c_double);
    pub fn glBitmap(width: c_int, height: c_int, xorig: c_float, yorig: c_float, xmove: c_float, ymove: c_float, bitmap: *const c_uchar);
    pub fn glDrawPixels(width: c_int, height: c_int, format: GLenum, kind: GLenum, pixels: *const c_void);
    pub fn glReadPixels(x: c_int, y: c_int, width: c_int, height: c_int, format: GLenum, kind: GLenum, pixels: *mut c_void);
    pub fn glRotated(angle: c_double, x: c_double, y: c_double, z: c_double);
    pub fn glRotatef(angle: c_float, x: c_float, y: c_float, z: c_float);
    pub fn glTranslatef(x: c_float, y: c_float, z: c_float);
    pub fn glScalef(x: c_float, y: c_float, z: c_float);
    pub fn glLineStipple(factor: c_int, pattern: c_ushort);
    pub fn glPolygonStipple(mask: *const c_uchar);
    pub fn glPolygonMode(face: GLenum, mode: GLenum);
    pub fn glFogi(pname: GLenum, param: c_int);
    pub fn glFogfv(pname: GLenum, params: *const c_float);
    pub fn glHint(target: GLenum, mode: GLenum);
    pub fn glBlendFunc(sfactor: GLenum, dfactor: GLenum);
    pub fn glLogicOp(opcode: GLenum);
    pub fn glStencilOp(fail: GLenum, zfail: GLenum, zpass: GLenum);
    pub fn glStencilFunc(func: GLenum, reference: c_int, mask: c_uint);
    pub fn glStencilMask(mask: c_uint);
    pub fn glCallList(list: c_uint);
    pub fn glGenLists(range: c_int) -> c_uint;
    pub fn glNewList(list: c_uint, mode: GLenum);
    pub fn glEndList();
    pub fn glDeleteLists(list: c_uint, range: c_int);
    pub fn glIsList(list: c_uint) -> c_uchar;
    pub fn glSelectBuffer(size: c_int, buffer: *mut c_uint);
    pub fn glRenderMode(mode: GLenum) -> c_int;
    pub fn glInitNames();
    pub fn glPushName(name: c_uint);
    pub fn glPopName();
    pub fn glLoadName(name: c_uint);
}

#[link(name = "GLU")]
extern "C" {
    pub fn gluOrtho2D(left: c_double, right: c_double, bottom: c_double, top: c_double);
    pub fn gluPerspective(fovy: c_double, aspect: c_double, near: c_double, far: c_double);
    pub fn gluLookAt(ex: c_double, ey: c_double, ez: c_double, cx: c_double, cy: c_double, cz: c_double, ux: c_double, uy: c_double, uz: c_double);
    pub fn gluPickMatrix(x: c_double, y: c_double, w: c_double, h: c_double, viewport: *mut c_int);
    pub fn gluUnProject(winx: c_double, winy: c_double, winz: c_double, model: *const c_double, proj: *const c_double, viewport: *const c_int, x: *mut c_double, y: *mut c_double, z: *mut c_double) -> c_int;
    pub fn gluNewQuadric() -> *mut c_void;
    pub fn gluPartialDisk(quad: *mut c_void, inner: c_double, outer: c_double, slices: c_int, loops: c_int, start: c_double, sweep: c_double);
    pub fn gluDeleteQuadric(quad: *mut c_void);
}
