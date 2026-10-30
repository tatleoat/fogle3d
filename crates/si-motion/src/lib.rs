// SPDX-License-Identifier: MIT
//! Animation evaluation of Softimage|3D 4.0: how the program takes a time and brings every
//! animated parameter of the scene to its value at that time, and the key, path and reset
//! commands around it (finding 0017).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files of the original: `motionUpdate.c` (the update at a
//! time), `motionKey.c` (keys), `motionPath.c` (path animation), `motionReset.c`,
//! `motionUtils.c`, `motion.c` (playback), `mot_lattice.c`, and what they need of
//! `toposort.c`, `updlist.c` and the element dictionary's look-ups. Ported so far: all of the
//! update (`motionUpdate.c`), the sort that orders it, all of `motionKey.c` (save key for
//! every kind of element, the key edit, activation curves), all of `motionPath.c`,
//! `motionReset.c`, `mot_lattice.c`, `motion.c` (playback, its top-level command and the key
//! browser) and `motionUtils.c`: every function of the eight source files of this package.
//! Each was run against the original in the emulation harness; the finding has the counts.
//!
//! The routines are written against the **shared memory image** (`si_mem::Mem`, finding
//! 0024): objects are addresses, fields are the original's offsets ([`off`]), globals sit at
//! the binary's addresses ([`glob`]), and what a routine allocates it takes from the image's
//! heap with the original's calls (`SI_malloc`, `SI_calloc`, `SI_realloc`, the C library's
//! `malloc`, `strdup` and `free`), so that its blocks lie where the original's lie. What the
//! original calls in other packages goes through host traits ([`host`]). [`mem`] holds what
//! this crate adds to the image: vectors and matrices.
//!
//! One bridge remains to a crate that is not on the image: [`fcv`] copies a curve out into
//! `si-fcurve`'s record, runs that crate's routine and stores the result (marked there).
//!
//! - [`table`]: which function curve drives which field of which structure;
//! - [`update`] and its sub-modules: `motionUpdate.c`;
//! - [`topo`]: the update list and the topological sort that orders it;
//! - [`ed`]: the element dictionary look-ups these files use;
//! - [`fcv`]: curves in the image, evaluated by `si-fcurve`;
//! - [`key`]: `motionKey.c`, the save-key commands of materials, textures and the scene;
//! - [`savekey`]: `motionKey.c`, the save-key commands of models;
//! - [`savekey2`]: `motionKey.c`, the save-key commands of cameras, lights and fittings;
//! - [`keyedit`]: `motionKey.c`, the key edit of the scene and the activation curves of relations;
//! - [`path`]: `motionPath.c`, path animation: path keys, timing, the path commands;
//! - [`play`]: `motion.c`, playback and scene refresh;
//! - [`reset`]: `motionReset.c`, removing animation;
//! - [`lattice`]: `mot_lattice.c`, lattice keys;
//! - [`utils`]: `motionUtils.c`, the first eight routines;
//! - [`nextkey`]: the key browser (`motionUtils.c` and `SI_nextKeyDisplay` of `motion.c`);
//! - [`animcopy`]: `motionUtils.c`, copying, selecting and testing an element's animation;
//! - [`fcvoper`]: `motionUtils.c`, one operation on all the curves of an element, a model or
//!   the scene (the offset-and-scale command);
//! - [`assoc`]: `motionUtils.c`, animation files and their elements, the PostScript preview;
//! - [`playrun`]: `motion.c`, the playback command;
//! - [`evalscene`]: what the original's sibling program `evalScene` prints after the update
//!   at a frame, for the check of the whole evaluation against its output
//!   (`re/tools/si-motion/evalcheck`).

pub mod animcopy;
pub mod assoc;
pub mod ed;
pub mod evalscene;
pub mod fcv;
pub mod fcvoper;
pub mod glob;
pub mod host;
pub mod key;
pub mod keyedit;
pub mod lattice;
pub mod mem;
pub mod nextkey;
pub mod off;
pub mod path;
pub mod play;
pub mod playrun;
pub mod reset;
pub mod savekey;
pub mod savekey2;
pub mod table;
pub mod topo;
pub mod update;
pub mod utils;

pub use mem::MemExt;
pub use si_mem::{Addr, Mem};

#[cfg(test)]
mod tests2;
#[cfg(test)]
mod tests3;
#[cfg(test)]
mod tests6;
