// SPDX-License-Identifier: MIT
//! The x87 arithmetic the original was compiled for, as a software model shared by every crate
//! that has to get the same bits (finding 0008).
//!
//! - [`x87`]: the register value [`F80`] with the unit's arithmetic, comparisons and rounding
//!   stores; checked against a real unit in its tests.
//! - [`trig`]: `sin`, `cos`, `tan`, `asin`, `acos` and `fpatan` as the C library of the time
//!   computes them with the unit's transcendental instructions. These cannot be bit-exact
//!   (they differ between processors); the module is deterministic and as close as measured.
//! - `hw` (x86-64): the processor's own x87 unit as the backend of [`F80`]'s operators,
//!   stores, comparisons and integer conversions, with the software model as the reference
//!   and the fallback; chosen at compile time (build.rs: on by default on x86-64, `SI_X87=soft` turns it off).
//! - [`fx`]: the macro `fx!`, a whole expression of [`F80`] operations as one block on the
//!   unit (the software model's expression without the backend).
//! - [`vectors`]: the reader for the test vectors of the emulation harness (finding 0011),
//!   which every crate's emulation check uses.
pub mod fx;
#[cfg(hw_x87)]
pub mod hw;
pub mod trig;
pub mod vectors;
pub mod x87;

pub use x87::*;
