// SPDX-License-Identifier: MIT
//! The generic containers of Softimage|3D 4.0 on the shared memory image (finding 0033).
//!
//! `genList.c` (0x08514ef0 … 0x08515823 in the Linux `soft`, 24 functions): a circular
//! doubly linked list of data pointers ([`list`]). `genTree.c`, which follows it in the
//! binary, is not ported yet.
//!
//! Every function takes the image and the original's arguments. The C library's `free`
//! and `SI_calloc` are the image's own ([`si_mem::Mem::free`], [`si_mem::Mem::si_calloc`]);
//! a function pointer the caller hands in (a destructor, a comparison, a copy, a dump) and
//! `fprintf` go through [`si_mem::Outside`].

pub mod list;
pub mod tree;
