// SPDX-License-Identifier: MIT
//! The mathematics library calls of the layer: `sin` and `cos` (arcs and circles in
//! `graphics.c`).
//!
//! These are imports of `libm.so.6`. On i386 they return their result in the x87 register,
//! unrounded, which is why the trait hands back an [`F80`]. What a C library of 2002 returned
//! for a given argument is outside the binary (finding 0008), so the calls sit behind a trait
//! like every other library: the port is checked for making the same calls with the same
//! arguments and doing the same arithmetic around them.

use crate::rec::{SharedLog, Val};
use si_x87::{trig, F80};

pub trait Libm {
    /// `sin(x)`, the 80-bit result.
    fn sin(&mut self, x: f64) -> F80;
    /// `cos(x)`, the 80-bit result.
    fn cos(&mut self, x: f64) -> F80;
}

/// The project's model of the C library's trigonometry on the x87 unit (`si_x87::trig`).
pub struct ModelLibm;

impl Libm for ModelLibm {
    fn sin(&mut self, x: f64) -> F80 {
        trig::sin(x)
    }
    fn cos(&mut self, x: f64) -> F80 {
        trig::cos(x)
    }
}

/// The recording implementation: logs the call and answers with a configured double
/// (`[D(value)]`), or without one with **this machine's** C library on doubles, which is
/// what the emulation harness answers the original with.
pub struct RecLibm {
    pub log: SharedLog,
}

impl RecLibm {
    pub fn new(log: SharedLog) -> Self {
        RecLibm { log }
    }
    fn answer(&mut self, name: &'static str, x: f64, host: f64) -> F80 {
        let a = self.log.borrow_mut().call(name, vec![Val::D(x)]);
        match a.as_ref().and_then(|v| v.first()) {
            Some(Val::D(d)) => F80::from_f64(*d),
            _ => F80::from_f64(host),
        }
    }
}

impl Libm for RecLibm {
    fn sin(&mut self, x: f64) -> F80 {
        self.answer("sin", x, x.sin())
    }
    fn cos(&mut self, x: f64) -> F80 {
        self.answer("cos", x, x.cos())
    }
}
