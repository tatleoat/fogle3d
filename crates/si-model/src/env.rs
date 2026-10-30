// SPDX-License-Identifier: MIT
//! What the routines of this crate call in parts of the program that are not ported, as one
//! object.
//!
//! The calls are grouped in four traits, each beside the routines that make them:
//! [`crate::transform::Hooks`], [`crate::transform::GeometryHooks`],
//! [`crate::hierarchy::Hooks`] and [`crate::node_copy::Hooks`]. Every method has a default
//! that does nothing (or the plain thing), so `()` is a usable set. [`Env`] is all four; a
//! routine that needs more than one group takes `&mut dyn Env`, which also passes where one
//! group is asked for.
//!
//! When an original routine behind a method is ported, the method goes and its callers call
//! the port: the traits list exactly what is still outside.

use crate::{hierarchy, node_copy, transform};

/// Every hook group of the crate.
pub trait Env: transform::Hooks + transform::GeometryHooks + hierarchy::Hooks + node_copy::Hooks {}

impl<T: transform::Hooks + transform::GeometryHooks + hierarchy::Hooks + node_copy::Hooks + ?Sized> Env for T {}

/// Places where a port cannot follow the original, counted per thread.
///
/// Several routines take the inverse of a matrix into a stack slot and go on whether or not
/// there was one (`SI_matrixInverse` writes nothing for a singular matrix). The original
/// then computes with whatever the slot held: bytes of an earlier call. The ports go on
/// with zeros (or with the last inverse they themselves put there) and note it here, so
/// that a caller, and the emulation check, can tell such a result from a faithful one.
pub mod stale {
    use std::cell::Cell;

    thread_local! {
        static COUNT: Cell<u32> = const { Cell::new(0) };
    }

    /// Notes one use of a slot the original never filled.
    pub fn note() {
        COUNT.with(|c| c.set(c.get() + 1));
    }

    /// The number of such uses since the last [`reset`].
    pub fn count() -> u32 {
        COUNT.with(|c| c.get())
    }

    pub fn reset() {
        COUNT.with(|c| c.set(0));
    }
}
