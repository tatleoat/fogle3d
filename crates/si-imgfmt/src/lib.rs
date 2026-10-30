// SPDX-License-Identifier: MIT
//! The picture library of Softimage|3D beyond its own picture files (finding 0014): the
//! foreign image formats the program reads and writes, and its image operations.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. One module per original source file.
//!
//! Conventions shared by the modules:
//! - A reader takes the file's bytes and returns the picture the original leaves in memory,
//!   or the case in which the original refuses the file (an enum whose `message` gives the
//!   text the original shows). A writer returns the bytes the original writes.
//! - Readers check every width, height and count against the bytes left before allocating.
//! - Pictures are [`si_picture::Picture`] (planes, first row first) or
//!   [`si_picture::PictureRgba`] (packed, bottom row first), as in the original.
//!
//! The functions here were compared with the original routine run in the emulation harness
//! (`re/tools/emu`) on made-up input (`imgfmt-emucheck` repeats that on a directory of
//! vectors), and, for every format a standalone converter of the original covers, with that
//! converter's output on the oracle's sample pictures (`imgfmt-oracle`; the converters run
//! the same library code). Finding 0014 has the counts.
//!
//! Not ported yet (finding 0014, "Not done" and "Where the work stands"): `SI_pictureCompositeNew`;
//! the glue between the colour reduction and the image classes is in [`glue`] (written from the
//! listing, not compared with the original).
//! The image classes themselves are in [`image`] (sixth pass); their formats and processors
//! are plug-ins outside `soft`.

pub mod io;
pub mod softpic;

pub mod alias;
pub mod artisan;
pub mod aurora;
pub mod everest;
pub mod pict;
pub mod raw;
pub mod sgi;
pub mod sgiimage;
pub mod targa;
pub mod tiff;
pub mod vertigo;
pub mod wavefront;
pub mod yuv;

pub mod compose;
pub mod composite;
pub mod dialog;
pub mod dgt;
pub mod effect;
pub mod glue;
pub mod gradation;
pub mod neuquant;
pub mod quant;
pub mod reduce;
pub mod size;
pub mod texture;
pub mod tile;
pub mod image;
pub mod lin;
pub mod zoom;
