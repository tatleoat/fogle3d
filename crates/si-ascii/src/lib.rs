// SPDX-License-Identifier: MIT
//! Softimage|3D ASCII files (finding 0006): the token reader and the ASCII header, the scene
//! description (`.dsc`), the setup file (`.sts`), the channel and flock setups, the
//! relations resources, and the text form of the binary element files that the converter
//! `bin2ascii` writes ([`b2a`]).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`.

pub mod b2a;
pub mod chn;
pub mod cnum;
pub mod dsc;
pub mod flk;
pub mod rawfcv;
pub mod reldata;
pub mod setup;
pub mod srel;
pub mod token;

pub use token::{read_asc_header, AsciiFile};
