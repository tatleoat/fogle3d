// SPDX-License-Identifier: MIT
//! The program's own renderer (finding 0012): the code of `SI_image` and everything below it
//! in the Linux `soft` binary (sha256 `ef21a382c3325d42…`), source files `rnd_main.c` to
//! `softtriangle.c` (0x08257a70 to 0x0827ab82). mental ray is a separate program and is not
//! here.
//!
//! One module per original source file. Functions are cited as `[soft 0xADDRESS name]`. The
//! original computes on the x87 unit; each port repeats the instruction sequence on
//! [`si_x87::F80`] register values and rounds where the original stores (finding 0008). The
//! doc comment of each function gives its exactness class: copy, single, model or libm.
//!
//! Program state that is not ported yet (the scene dictionary, the user interface, texture
//! files) comes in as a plain struct or a trait argument; each such place says so.
//!
//! `getRenderLicense` (`render.c`) is ported in [`licence`] without the licence client it
//! asks (FLEXlm): the client's checkout answer is a stand-in, a granted checkout.
pub mod types;
pub mod img;
pub mod inimage;
pub mod adapter;
pub mod bound;
pub mod shade;
pub mod camera;
pub mod jitter;
pub mod jitter_image;
pub mod render;
pub mod licence;
pub mod bsptree;
pub mod lights;
pub mod trace;
pub mod softarray;
pub mod fbuffer;
pub mod frame;
pub mod bands;
pub mod pool;
pub mod host;
pub mod data;
pub mod byteswap;
pub mod rle_image;
pub mod renutil_image;
pub mod initialize;
pub mod renutil;
pub mod rle;
pub mod rnd_main;
pub mod rnd_tag;
pub mod update;
pub mod geometry;
pub mod softtriangle;
pub mod softvertex;
pub mod facemesh;
pub mod blookup;
pub mod materials;
pub mod mem_chunk;
pub mod mem_chunk_image;
pub mod fromfile;
pub mod t2d_ren;
pub mod t2d_dial;
pub mod edtools;
pub mod edtools2;
pub mod edtools3;
pub mod t2d_fct;
pub mod polygon;
pub mod t3d_ren;
pub mod t3d_fct_image;
pub mod t2d_fct_image;
pub mod sun_fct;
pub mod mat_global;
pub mod mat_validation;
pub mod volume;
pub mod vceval;
pub mod bloom;
pub mod range;
pub mod bi_search;
pub mod io;
pub mod hooks;
pub mod user;
pub mod update_voxel;
pub mod split_proc;
pub mod matter_image;
pub mod tt;
pub mod tt_noise;
pub mod drand48;
pub mod blur;
pub mod blur_image;
pub mod equal_fct;
pub mod process_materials;
pub mod shd_copy;
pub mod shd_list;
pub mod shd_ui;
pub mod light_fct;
pub mod sun_position;
pub mod softvertex_image;
pub mod renutil_tree;
pub mod tex_leftovers;
