// SPDX-License-Identifier: MIT
//! `hidden.c` (0x0833cb70 to 0x08345469): the hidden-line view. A scene's triangles and
//! edges are collected in lists, cut against each other and drawn as lines.
//!
//! Every function takes the image and the arguments the original takes. The original's
//! scratch variables (`static` locals of the compiler: `a.258`, `tmp.414`, and so on) are
//! globals in the binary; they are written here in the original's order and width, because
//! the one image is the interface (finding 0024). Floating-point work is done on
//! [`si_x87::F80`], rounded where the original stores to memory.

pub mod geom;
pub mod edges;
pub mod lists;
pub mod tree;
pub mod x87stack;
