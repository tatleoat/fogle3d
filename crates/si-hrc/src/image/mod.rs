// SPDX-License-Identifier: MIT
//! The readers and writers of model, material and texture files on the shared memory
//! image (findings 0004 and 0024). The writers (`wio`, `matwrite`, `modelwrite` and the
//! table writer of `proto`) came with the third pass of finding 0004 and are measured
//! against files the original wrote (`re/tools/si-hrc/savedcheck`); what follows describes
//! the readers.
//!
//! This is the second port of `b_matio.c`, `binary.c` and their helpers. The first one
//! (the crate's other modules) reads a file into Rust values of its own; this one does what
//! the original does: it allocates the node, mesh, material and texture records in
//! `si-mem`'s image, at the original's layout, and fills them from the file (since the
//! second pass every object kind of a model: splines, faces, patches, NURBS curves and
//! surfaces, metaballs, and the clusters, envelopes, inverse kinematics and dynamics that
//! hang on a node). Every function
//! takes the image, the outside world ([`Outside`]: the C library's streams and the
//! routines of other source files, each written `name (0xADDRESS)`) and the arguments the
//! original takes; every `SI_malloc`, `SI_calloc`, `SI_realloc`, `malloc`, `strdup` and
//! `free` of the original is made, with its size and in its order, including what a
//! truncated file leaves allocated. `hrc-emucheck` holds each function against the emulated
//! original on made-up files: the heap block for block and byte for byte, the frees in
//! order, the stream's position, the outside calls.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]` where a function is reproduced in full and was compared with the
//! emulated original; a function written but not compared is marked `unchecked`.
//!
//! Left out of every function here, with the project's standing reasons: the counter
//! `SI_secureLicAlloc` (0x08679660), which the original bumps before some allocations and
//! its allocation wrappers clear again (a word of the program's self-check: licence code,
//! not ported).
//!
//! Locals whose address the original passes to a callee (the field a read stores into, the
//! working copy of a texture) lie on the image's stack, so that what a truncated file
//! leaves in them is what it leaves in the original: the bytes that arrived, and beyond
//! them whatever the memory held.

pub mod chunks;
pub mod geom;
pub mod globals;
pub mod io;
pub mod material;
pub mod matio;
pub mod matwrite;
pub mod mesh;
pub mod meta;
pub mod modelwrite;
pub mod node;
pub mod nurbs;
pub mod outside;
pub mod patch;
pub mod proto;
pub mod shape;
pub mod spline;
pub mod texture;
pub mod udf;
pub mod userdata;
pub mod values;
pub mod wio;

pub use outside::{Files, MemOutside, Outside, Routine, Stream};

#[cfg(test)]
mod tests;

use si_mem::Mem;

/// Writes the initial values of the globals and constants this module reads: the scalars
/// of [`globals::ALL`], the identity matrix and the strings. A fresh image needs this once
/// before any reader runs.
pub fn init(m: &mut Mem) {
    si_mem::init_globals(m, globals::ALL);
    globals::store_data(m);
}
