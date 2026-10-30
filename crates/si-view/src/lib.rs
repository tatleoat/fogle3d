// SPDX-License-Identifier: MIT
//! The graph list of Softimage|3D 4.0 on the shared memory image (findings 0024 and 0027):
//! the list of scene nodes every view draws from and the render commands walk.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`; a routine that is not ported is written `name (0xADDRESS)`.
//! Source files: `graflist.c` (this pass), `graphsort.c`, `nselections.c`, `displayMode.c`
//! (later passes).
//!
//! Every function here takes the image ([`si_mem::Mem`]) and the arguments the original
//! takes: pointers as [`si_mem::Ptr32`] of the records laid out in `si-types`
//! (`GrafList`, `GrafNode`, `Model`), numbers as they are. Records are read and written in
//! place; every `SI_malloc`, `SI_realloc`, `strdup` and `free` of the original is made,
//! with its size and in its order, so that the heap a routine leaves is the original's
//! block for block (which `view-emucheck` compares with the emulated original). Drawing,
//! dialogs, the views and every routine of another source file are reached through
//! [`outside::Outside`], by the original routine's name and address.
//!
//! Left out, with the project's standing reason: the product-tier flag `SI_lightLicense`
//! (0x08679320). Where a routine branches on it the full product's path is taken (the
//! flag is 0) and a comment says so at that spot; the flag is neither declared nor read.

pub mod db2scn;
pub mod func;
pub mod globals;
pub mod graflist;
pub mod graphsort;
pub mod hidden;
pub mod nselections;
pub mod outside;
pub mod records;
pub mod wired;

#[cfg(test)]
mod tests;

pub use outside::{NoOutside, Outside, Routine};
pub use wired::Wired;
pub use si_mem::{Addr, CStr32, FnPtr, Mem, Opaque, Ptr32};
pub use si_types::model::Model;
pub use si_types::scene::{GrafList, GrafNode};
