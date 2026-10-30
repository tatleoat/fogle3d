// SPDX-License-Identifier: MIT
//! The modelling tools' geometry on `si-mem`'s memory image (package 20, finding 0034).
//!
//! One module per original source file; each cites what it reproduces as `[soft 0xADDRESS name]`.
//!
//! | Module | Original | Content |
//! |---|---|---|
//! | [`extrusion`] | `extrusion.c` (first slice, 0x08156c84 to 0x0815e3f0) | the nodes an extrusion makes, the extrusion tool's glue |

pub mod extrusion;
pub mod effects;
pub mod extrusion2;
pub mod geomupd1;
