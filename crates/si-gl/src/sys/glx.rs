// SPDX-License-Identifier: MIT
//! The GLX calls of the layer.

use super::{outside, Ptr, Xid};
use crate::rec::{out_u, ret_u, Val};

outside! {
    /// GLX, as the layer calls it.
    pub trait Glx, recorded by RecGlx {
        /// `glXCreateContext(dpy, vis, share, direct)`
        fn glXCreateContext(dpy: Ptr, vis: Ptr, share: Ptr, direct: i32) -> Ptr;
        /// `glXMakeCurrent(dpy, drawable, ctx)`
        fn glXMakeCurrent(dpy: Ptr, drawable: Xid, ctx: Ptr) -> i32;
        fn glXGetCurrentDrawable() -> Xid;
        fn glXDestroyContext(dpy: Ptr, ctx: Ptr);
        fn glXWaitGL();
        fn glXWaitX();
        fn glXSwapBuffers(dpy: Ptr, drawable: Xid);
        /// The call through the pointer `glXGetProcAddressARB("glXWaitVideoSyncSGI")`
        /// returned: `glXWaitVideoSyncSGI(divisor, remainder, &count)`; the count written
        /// back is not read by the layer.
        fn glXWaitVideoSyncSGI(divisor: i32, remainder: i32) -> i32;
        manual:
        /// `glXChooseVisual(dpy, screen, attribList)`: the attribute list up to and
        /// including its terminating 0.
        fn glXChooseVisual(&mut self, dpy: Ptr, screen: i32, attribs: &[i32]) -> Ptr {
            let a = self.log.borrow_mut().call(
                "glXChooseVisual",
                vec![Val::P(dpy), Val::U(screen as u32), crate::rec::i32_words(attribs)],
            );
            ret_u(&a, 0)
        }
        /// `glXGetConfig(dpy, vis, attrib, &value)`: the result and the value written.
        /// Answer: `[return, value]`.
        fn glXGetConfig(&mut self, dpy: Ptr, vis: Ptr, attrib: i32) -> (i32, i32) {
            let a = self.log.borrow_mut().call(
                "glXGetConfig",
                vec![Val::P(dpy), Val::P(vis), Val::U(attrib as u32), Val::Out],
            );
            (ret_u(&a, 0) as i32, out_u(&a, 1, 0) as i32)
        }
        /// `glXGetClientString(dpy, name)`: `None` is a null pointer. Answer: `[S(text)]`.
        fn glXGetClientString(&mut self, dpy: Ptr, name: i32) -> Option<Vec<u8>> {
            let a = self.log.borrow_mut().call("glXGetClientString", vec![Val::P(dpy), Val::U(name as u32)]);
            match a.as_ref().and_then(|v| v.first()) {
                Some(Val::S(s)) => Some(s.clone()),
                _ => None,
            }
        }
        /// `glXGetProcAddressARB(name)`: the function's address, 0 if there is none.
        fn glXGetProcAddressARB(&mut self, name: &[u8]) -> Ptr {
            let a = self.log.borrow_mut().call("glXGetProcAddressARB", vec![Val::S(name.to_vec())]);
            ret_u(&a, 0)
        }
    }
}
