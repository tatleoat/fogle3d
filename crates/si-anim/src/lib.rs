// SPDX-License-Identifier: MIT
//! Shape animation, cluster animation and action lists of Softimage|3D 4.0 on the shared
//! memory image (findings 0024 and 0030).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`; a routine that is not ported is written `name (0xADDRESS)`.
//! Source files: `shape.c` ([`shape`]), `cluster.c` ([`cluster`]), `actions.c`
//! ([`actions`]; and, from the third pass, [`walk`]: the element walk `hieranimFcvProcess`
//! and its per-curve steps, and [`actedit`]: the editing routines that go through it).
//!
//! Every function here takes the image ([`si_mem::Mem`]) and the arguments the original
//! takes. Records are read and written in place: the model node and its geometry (mesh,
//! patch, spline, face, NURBS curve and surface) are `si-types` layouts; the records of
//! cluster animation and of an action list are `record!` layouts of this crate
//! ([`records`]). Every allocation call of the original is made, with its size and in its
//! order, so the heap a routine leaves is the original's block for block (which
//! `anim-emucheck` compares with the emulated original). Floating-point work follows the
//! instruction listing on `si_x87::F80`: a value is rounded where the original stores it.
//! Drawing, dialogs and every routine of another source file are reached through
//! [`outside::Outside`], by the original routine's name and address.

pub mod actedit;
pub mod actions;
pub mod cluster;
pub mod dictin;
pub mod dopebuf;
pub mod edanim;
pub mod fcvdial;
pub mod fcvedit;
pub mod paste;
pub mod globals;
pub mod outside;
pub mod records;
pub mod scene1;
pub mod scene2;
pub mod shape;
pub mod walk;

#[cfg(test)]
mod tests;

pub use outside::{NoOutside, Outside, Routine};
pub use si_mem::{CStr32, Mem, Opaque, Ptr32};
pub use si_types::model::Model;

use si_x87::{x, F80};

/// `a + (b − a) × t` as the shape routines compute it: both floats loaded, difference,
/// product and sum in the register, one rounding at the store.
pub(crate) fn lerp(a: f32, b: f32, t: F80) -> f32 {
    (x(a) + (x(b) - x(a)) * t).to_f32()
}

/// The same from the image: `dst = a + (b − a) × t` for one float at three addresses.
pub(crate) fn lerp_at(m: &mut Mem, a: u32, b: u32, t: F80, dst: u32) {
    let v = lerp(m.f32(a), m.f32(b), t);
    m.set_f32(dst, v);
}
