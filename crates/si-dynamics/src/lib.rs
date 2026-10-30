// SPDX-License-Identifier: MIT
//! Softimage|3D dynamics on the shared memory image (finding 0035).
//!
//! `collisions.c` (0x084a1990 … 0x084a8e03 in the Linux `soft`, 19 functions) detects
//! contacts between the bodies of a dynamics simulation and resolves them. Every function
//! takes the image and the original's arguments; callees in other source files go through
//! the hooks of [`outside`] (answered 0 until their own file is ported).

pub mod collisions;
pub mod contact;
pub mod contact_bbox;
pub mod dynamic;
pub mod particle_build;
pub mod particle_data;
pub mod particle_dial;
pub mod particle_fcrv;
pub mod particle_init;
pub mod particle_mrg;
pub mod particle_mf;
pub mod particle_mn;
pub mod particle_outside;
pub mod particle_util;
pub mod wave_dialog;
pub mod wave_fcrv;
pub mod wave_init;
pub mod wave_mn;
pub mod wave_sys;
pub mod wave_util;
pub mod invdynamic;
pub mod invdynamic_fan;
pub mod invdynamic_mesh;
pub mod invdynamic_node;
pub mod invdynamic_wind;
pub mod constr;
pub mod dynadial;
pub mod playback;
pub mod dynamicio;
pub mod outside;
pub mod routed;
pub mod settings;
pub mod settings_dyn;
pub mod settings_geom;
pub mod settings_rec;
