// SPDX-License-Identifier: MIT
//! The IRIS GL emulation layer of Softimage|3D 4.0 (finding 0016).
//!
//! The program was written against Silicon Graphics' IRIS GL. On Linux the calls are
//! implemented by a layer linked into `soft` (0x08607570–0x0861a0a5; `gl4emult.dll` on NT)
//! that translates them to OpenGL, GLX, Xlib and the OSS sound device. This crate reproduces
//! that layer's own logic and state, and puts every call that leaves the program behind a
//! trait in [`sys`].
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! - [`IrisGl`] is the layer: its methods are the IRIS GL entry points (`winopen`, `qread`,
//!   `bgnpolygon`, `v3f`, …) and it owns the state ([`state`]).
//! - One module per source file of the original: [`context`] (`context.c`, `error.c`),
//!   [`window`], [`event`], [`graphics`], [`font`], [`cursor`], [`pick`], [`misc`],
//!   [`unimplemented`] (the 291 calls the layer provides as empty functions).
//! - [`sys`]: the libraries as traits, with recording implementations over [`rec`].
//! - [`data`]: the layer's initialised tables.
pub mod context;
pub mod cursor;
pub mod data;
pub mod event;
pub mod font;
pub mod graphics;
pub mod misc;
pub mod pick;
pub mod rec;
pub mod state;
pub mod sys;
pub mod unimplemented;
pub mod window;

pub use context::IrisGl;
