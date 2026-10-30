// SPDX-License-Identifier: MIT
//! The element, relation, attribute and enum dictionaries of Softimage|3D 4.0 on the shared
//! memory image (findings 0024 and 0026).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`; a routine that is not ported is written `name (0xADDRESS)`.
//! Source files: `elemdict.c`, `enmdict.c`, `init_dict.c`, `updlist.c`, `toposort.c`,
//! `reldict.c`, `relations.c`, `attdict.c`.
//!
//! Every function here takes the image ([`si_mem::Mem`]) and the arguments the original
//! takes: pointers as [`si_mem::Ptr32`] of the record laid out in `si-types` (or in
//! [`records`] where `si-types` has none), numbers as they are. Records are read and
//! written in place; every `SI_malloc`, `SI_realloc`, `SI_calloc`, `malloc`, `calloc`,
//! `strdup` and `free` of the original is made, with its size and in its order, so that the
//! heap a routine leaves is the original's block for block (which `dict-emucheck` compares
//! with the emulated original). A routine that needs a part of the program that is not on
//! the image yet reaches it through [`outside::Outside`], by the original routine's name
//! and address.
//!
//! Two things every function here leaves out, with the project's standing reasons:
//! `SI_secureLicAlloc` (0x08679660), a counter the original bumps before each allocation
//! and its allocation wrappers clear again (a word of the program's self-check: licence
//! code, not ported; `si-app` says the same of the allocators), and the product-tier flag
//! `SI_lightLicense`, for which the full product's path is taken.
//!
//! The earlier ports of the same functions on owned stores (`si-types`, `si-scene`) and on
//! a private raw image (`si-motion`) stay where they are and in use; finding 0026 says for
//! each function how the two forms compare.

pub mod attdict;
pub mod elemdict;
pub mod elemmisc;
pub mod elemname;
pub mod elemrel;
pub mod elemsort;
pub mod dictin;
pub mod enmdict;
pub mod globals;
pub mod hieranim;
pub mod libc;
pub mod outside;
pub mod records;
pub mod relations;
pub mod reldict;
pub mod reldictops;
pub mod rodata;
pub mod toposort;
pub mod updlist;

#[cfg(test)]
mod tests;

pub use outside::{NoOutside, Outside, Routine};
pub use si_mem::{Addr, CStr32, FnPtr, Mem, Opaque, Ptr32};

/// Room on the image's stack for a routine's frame, `size` bytes below the stack pointer,
/// **left as it is**: a local the original reads without having set it holds what
/// earlier calls left at that depth. ([`Mem::local`] hands out bytes filled afresh, which
/// is right for a local that is only written.) Given back with [`Mem::release`].
pub(crate) fn frame(m: &mut Mem, size: u32) -> u32 {
    let at = m.sp().wrapping_sub(size);
    m.set_sp(at);
    at
}
