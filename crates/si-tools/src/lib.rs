// SPDX-License-Identifier: MIT
//! The model converters of Softimage|3D 4.0 on the shared memory image (finding 0039).
//!
//! Package 58, "Model converters": `owave2soft.c` (the Wavefront `.obj` reader and the
//! builder that turns what it read into models), `osoft2wave.c`, `osoft2vert.c`. Every
//! function takes the image, whoever answers for the rest of the program
//! ([`si_mem::Outside`]) and the original's arguments.

pub mod omovie2soft;
pub mod overt2soft;
pub mod owave2soft;
pub mod osoft2vert;
pub mod osoft2wave;
pub mod stdio;
