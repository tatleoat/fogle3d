// SPDX-License-Identifier: MIT
//! `si-mental`: the mental ray translator of Softimage|3D 4.0 on the shared memory image
//! (package 63, finding 0036). One module per source file of the original.

pub mod cprintf;
pub mod mi_calc;
pub mod mi_material;
pub mod mi_defparms;
pub mod mi_error;
pub mod mi2_light;
pub mod mi2_model;
pub mod mi2_tools;
pub mod mi2control;
pub mod mi_light;
pub mod mi_model;
pub mod mi_mr_main;
pub mod mi_patch;
pub mod mi_poly;
pub mod mi_tools;
pub mod mr_cmdline;
pub mod mr_display;
pub mod mr_main;
pub mod mr_tracer;
pub mod mr_messages;
pub mod mr_translator;
pub mod mr_util;
pub mod mi_texture;
pub mod mi2control_lex;
pub mod mi2control_yacc;
