// SPDX-License-Identifier: MIT
//! Softimage|3D model, material and texture files: the prototype-and-token format
//! (finding 0004).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `binary.c` (readers), `binary_b.c` (writers),
//! `b_matio.c` (materials and textures), `utilio.c` (prototypes and helpers); helpers they
//! call come from `byteswap.c`, `filetoken.c`, `headers.c`, `dynamicio.c`, `invkin.c`,
//! `envtools.c`, `t2d_dial.c`, `metaball.c`, `klusters_b.c` and the allocators of the
//! structures in `face.c`, `mesh_b.c`, `nrb_b.c`, `klusters.c`, `settings.c`.
//!
//! - [`read_file`] reads a `.hrc`, `.mtr`, `.txt` or `.t3D` file into the structures of
//!   [`model`], following the prototype table of the file ([`Prototypes`]).
//! - [`write_model`], [`write_material`], [`write_texture`], [`write_texture3d`] write what
//!   the program writes: version 4.0 with its current table.
//! - [`write_as_read`] writes a file back with its own table (a check, not something the
//!   program does).
//! - [`image`] is the readers and the writers again, on the shared memory image of `si-mem`
//!   (finding 0024): they allocate, fill and write the original's records instead of values
//!   of this crate.
//! - [`read_file_with`] and [`Versions`]: the three version globals the readers consult,
//!   for a caller that reads several files as one program.

pub mod image;
pub mod io;
pub mod model;
pub mod proto;
pub mod read;
mod read_model;
pub mod write;

pub use model::*;
pub use proto::Prototypes;
pub use read::{read_file, read_file_with, Content, File, Kind, Notes, Reader, Versions};
pub use write::{write_as_read, write_content, write_material, write_model, write_texture, write_texture3d, Writer};

#[cfg(test)]
mod tests;
