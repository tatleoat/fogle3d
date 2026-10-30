// SPDX-License-Identifier: MIT
//! Everything outside the program that the layer calls, one trait per library, each carrying
//! exactly the calls the original makes with exactly its arguments:
//!
//! | Trait | Library | Recording implementation |
//! |---|---|---|
//! | [`Xlib`] | `libX11` | [`RecXlib`] |
//! | [`Glx`] | the GLX part of `libGL` | [`RecGlx`] |
//! | [`Gl`] | `libGL` | [`RecGl`] |
//! | [`Glu`] | `libGLU` | [`RecGlu`] |
//! | [`Libc`] | the C library's system services | [`RecLibc`] |
//! | [`Libm`] | `libm` (`sin`, `cos`) | [`RecLibm`] |
//!
//! Method names are the imported symbols' names, so a method is one call of that symbol.
//! Where the original reads a structure the library owns through a pointer (the `Display`
//! and its screens, an `XVisualInfo`, an `XImage`, the selection buffer OpenGL fills), the
//! trait has a small accessor instead; those are documented as "memory, not a call" and the
//! recording implementations do not log them.
//!
//! The recording implementations share one [`crate::rec::Log`], so the order of calls across
//! libraries is kept. [`Sys::recording`] builds the set.
pub mod gl;
pub mod glu;
pub mod glx;
pub mod libc;
pub mod libm;
#[cfg(feature = "system")]
pub mod real;
pub mod xlib;

pub use gl::{Gl, RecGl};
pub use glu::{Glu, RecGlu};
pub use glx::{Glx, RecGlx};
pub use libc::{CArg, CStream, Exit, Libc, RecLibc, SemBuf};
pub use libm::{Libm, ModelLibm, RecLibm};
pub use xlib::{
    RecXlib, XEvent, XSetWindowAttributes, XSizeHints, XTextProperty, XWindowChanges, Xlib,
};

use crate::rec::{new_log, SharedLog};

/// A pointer the original got from a library and only hands back (a `Display *`, an
/// `XVisualInfo *`, a GLX context, an `XImage *`, a `GC`, a quadric): the value is opaque.
pub type Ptr = u32;
/// An X resource identifier (window, pixmap, colormap, cursor) or an atom.
pub type Xid = u32;

/// The libraries the layer stands on.
pub struct Sys {
    pub x: Box<dyn Xlib>,
    pub glx: Box<dyn Glx>,
    pub gl: Box<dyn Gl>,
    pub glu: Box<dyn Glu>,
    pub c: Box<dyn Libc>,
    pub m: Box<dyn Libm>,
}

impl Sys {
    /// The recording back end: every call is appended to the returned log and answered from
    /// the values configured in it (see [`crate::rec::Log::answer`]).
    pub fn recording() -> (Sys, SharedLog) {
        let log = new_log();
        let sys = Sys {
            x: Box::new(RecXlib::new(log.clone())),
            glx: Box::new(RecGlx::new(log.clone())),
            gl: Box::new(RecGl::new(log.clone())),
            glu: Box::new(RecGlu::new(log.clone())),
            c: Box::new(RecLibc::new(log.clone())),
            m: Box::new(RecLibm::new(log.clone())),
        };
        (sys, log)
    }
}

/// Declares a library trait and its recording implementation from one list.
///
/// The first part lists the calls whose arguments and result are plain numbers: for each,
/// the trait gets `fn Name(&mut self, args) -> R` and the recording implementation logs
/// `Name(args)` and returns the configured answer (0 without one). The part after `manual:`
/// holds the calls with pointer arguments, written out with their recording bodies.
macro_rules! outside {
    (
        $(#[$tm:meta])*
        pub trait $tr:ident, recorded by $rec:ident {
            $( $(#[$m:meta])* fn $name:ident ( $($a:ident : $t:ty),* $(,)? ) $(-> $r:ty)? ; )*
            manual:
            $( $(#[$mm:meta])* fn $mname:ident (&mut $s:ident $(, $ma:ident : $mt:ty)* $(,)? ) $(-> $mr:ty)? $body:block )*
        }
    ) => {
        $(#[$tm])*
        #[allow(non_snake_case)]
        pub trait $tr {
            $( $(#[$m])* fn $name(&mut self, $($a: $t),*) $(-> $r)?; )*
            $( $(#[$mm])* fn $mname(&mut self, $($ma: $mt),*) $(-> $mr)?; )*
        }

        /// The recording implementation: logs each call and answers from the shared log.
        pub struct $rec {
            pub log: $crate::rec::SharedLog,
        }

        impl $rec {
            pub fn new(log: $crate::rec::SharedLog) -> Self {
                $rec { log }
            }
        }

        #[allow(non_snake_case, unused_variables)]
        impl $tr for $rec {
            $( fn $name(&mut self, $($a: $t),*) $(-> $r)? {
                let ans = self.log.borrow_mut().call(
                    stringify!($name),
                    vec![$($crate::rec::IntoVal::into_val(&$a)),*],
                );
                $crate::rec::FromAns::from_ans(&ans)
            } )*
            $( fn $mname(&mut $s, $($ma: $mt),*) $(-> $mr)? $body )*
        }
    };
}
pub(crate) use outside;
