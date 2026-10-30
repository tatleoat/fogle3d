// SPDX-License-Identifier: MIT
//! `sys_camera.c` of Softimage|3D 4.0 on the shared memory image (finding 0032).
//!
//! The source file (0x080d3b60 … 0x080d7704 in the Linux `soft`, 38 functions) holds what
//! the interactive program does with the scene's camera record (`SI_Camera`, 0x13c bytes,
//! laid out as `si-elemio`'s camera reader has it: [`si_elemio::image::off::camera`]): its
//! viewing matrix and the up-vector constraint that sets its roll ([`mat`]), its moves by
//! the mouse, its resets, the default cameras of the views, the picture format dialog.
//!
//! Every function here takes the image and the original's arguments; callees in other
//! source files go through [`si_mem::Outside`] with the typed names of [`outside`];
//! mathematics on values (`si-math`, `si-x87`) is called directly (finding 0024 exempts leaf
//! libraries). Globals are read at their addresses ([`globals`]).

pub mod dof;
pub mod globals;
pub mod mat;
pub mod outside;
pub mod trans;
