// SPDX-License-Identifier: MIT
//! The booleans of Softimage|3D 4.0 on the shared memory image (finding 0037).
//!
//! `boolean.c` (0x0830c990 … 0x083158af in the Linux `soft`): a BSP-tree constructive solid
//! geometry on polygon objects. One module a source file of the original.

pub mod boolean;
pub mod clean;
pub mod merge;
pub mod mesh;
pub mod ops;
pub mod split;
pub mod util;
