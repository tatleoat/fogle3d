// SPDX-License-Identifier: MIT
//! Softimage|3D render set-up (finding 0023): what the program does between the user asking
//! for a render or a preview and the renderer being called, working on the dictionaries.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. One module per original source file:
//!
//! | Module | Original | Content |
//! |---|---|---|
//! | [`world`] | (records) | the heap the dictionaries' objects live in, the records this crate reads by offset, the render globals |
//! | [`outside`] | | the calls into parts of the program that are other packages', as one trait |
//! | [`edlocal`] | `elemdict.c`, `edtools.c`, `instance.c`, `model.c` | routines of other files the set-up needs and no crate had |
//! | [`rmain`] | `rens_main.c` | the render dictionary made and deleted, selective lights, the editors' preview |
//! | [`propagate`] | `rens_propagate.c` | hidden and select states, transformations, materials and textures handed down the hierarchy; the default material |
//! | [`update`] | `rens_update.c` | a render dictionary brought to another frame |
//! | [`rdict`] | `renu_dict.c` | material, texture and shader relations copied, attached, removed, replaced |
//! | [`util`] | `renu_util.c` | the render camera, circle of confusion, picture names, the test for something to render, transmit flags |
//! | [`instance`] | `rens_instance.c` | instances expanded into real elements, kept up to date, taken away |
//! | [`sys`] | `renu_mutex.c`, `renu_pipeio.c`, `renu_process.c`, `renu_threads.c`, `renu_dynalink.c`, `renu_memory.c`, `renu_filepath.c` | semaphores, pipes, child processes, threads, shared libraries, tracked memory, the search for a program, on a trait |
//! | [`udf`] | `rens_udf.c` | the scan of a render dictionary for the shaders a render uses |
//! | [`subregion`] | `renu_subregion.c` | the sub-region box made from its fractions, clipped, tested, turned right way up |
//! | [`handover`] | (`matter.c`, `rnd_dial.c`) | what the native renderer is handed, as a trait |
//! | [`bridge`] | | a scene loaded by `si-scene` turned into the records the set-up works on |
//! | [`canon`] | | the canonical form of a world, for the comparison with the emulated original |
//! | [`native`] | `instance.c`, `matter.c`, `rnd_dial.c`, `edtools.c`, parts of `envtools.c`, `classic.c`, `skeleton.c`, `model_b.c`, `settings.c`, `invkin.c`, `algebra.c` | the second to fourth pass, on the shared memory image: the two commands that hand the scene to the native renderer, the hierarchy copy they need with its envelope and compensation copies, the render controls' start-up state, the callees that exist on the image wired to the ports of `si-dict`, `si-model`, `si-view` and `si-render`, and an adapter for what a scene loaded by `si-scene` still lacks in the image (finding 0023, sections 16 to 18) |
//!
//! The mental ray front end is what calls most of this (`SI_prepareRendering`); the native
//! renderer is handed the scene graph's own nodes by `SI_frameRender` and
//! `SI_sequenceRender`, which are other source files (finding 0023, section 2).

pub mod bridge;
pub mod canon;
pub mod edlocal;
pub mod handover;
pub mod instance;
pub mod native;
pub mod outside;
pub mod propagate;
pub mod rdict;
pub mod rmain;
pub mod subregion;
pub mod sys;
pub mod udf;
pub mod update;
pub mod util;
pub mod world;

#[cfg(test)]
mod tests;
