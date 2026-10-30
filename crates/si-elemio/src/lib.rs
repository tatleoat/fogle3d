// SPDX-License-Identifier: MIT
//! The binary element files of a Softimage|3D database other than models, materials and
//! textures: camera (`.cam`), light (`.lig`), lattice (`.lat`), animation (`.ani`), struct
//! keys (`.key`), cluster (`.cls`) and shape (`.shp`) files. Finding 0005.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.
//!
//! Two forms. The modules of this level read a file into Rust values of their own and
//! write it back (the value form: `si-scene` and `si-rensetup` use it). [`image`] is the
//! readers of camera, light, lattice, animation and struct key files a second time, **on
//! the shared memory image** of `si-mem` (finding 0024): they build the original's records
//! in the image with the original's allocation calls, and are checked against the original
//! routine stepped through in the emulation harness.
//!
//! The readers reproduce what the original leaves in memory for any input. Where the original
//! would never return (a file that ends before its zero token), overrun a buffer or read
//! memory it never set, the code stops or substitutes a defined value and says so at that
//! spot.

pub mod anim;
pub mod camera;
pub mod cluster;
pub mod fcurve;
pub mod geom;
pub mod image;
pub mod io;
pub mod lattice;
pub mod light;
pub mod proto;
pub mod shape;
pub mod x87;

pub use io::{Constraint, End, Reader, Statics, Udf, UserNode, Writer};
