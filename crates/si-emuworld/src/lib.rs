// SPDX-License-Identifier: MIT
//! A loaded sample scene as the starting memory of an emulation check (finding 0011,
//! "A real scene as the world").
//!
//! **Nothing in this crate reproduces a function of the original**, and nothing in it is
//! cited. It is engineering of ours round three packages that are on the shared memory
//! image (`si-mem`, finding 0024):
//!
//! - [`build`] loads one scene of a sample database the way `si-scene` does it, then puts
//!   into the same image what two more packages can put there today: the model, material
//!   and texture files read by `si-hrc`'s image readers, and the graph list built by
//!   `si-view`'s port of the list's builder. Every step that is glue of this crate and not
//!   a port is listed in [`build::World::notes`], and so is everything that is missing.
//! - [`dump`] writes that image out: every region with its bytes, the heap's live blocks,
//!   the globals the three packages declare, and a table of addresses worth calling a
//!   routine on. `re/tools/emu/sceneworld.py` maps such a file as the memory an original
//!   routine is stepped through in.
//! - [`case`] reads the vector lines the harness then writes, and [`check`] runs the
//!   ports on the same image and compares: the answer, the buffers of the case, and every
//!   byte of the image the routine changed.
//!
//! Binaries: `scene-dump` and `world-emucheck`.

pub mod build;
pub mod case;
pub mod check;
pub mod dump;
