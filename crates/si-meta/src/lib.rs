// SPDX-License-Identifier: MIT
//! Metaballs of Softimage|3D 4.0 on the shared memory image (finding 0038).
//!
//! `metaball.c` (0x0845aa90 … 0x08461909 in the Linux `soft`, 61 functions): the metaball
//! (`SI_Metaball`) and metasystem (`SI_MetaSystem`) records, the groups of a system and
//! their primitive links, the unblend pairs, the surface of a system and its drawing.
//! Every function takes the image, whoever answers for the rest of the program
//! ([`si_mem::Outside`]) and the original's arguments.

pub mod adopt;
pub mod corp;
pub mod init;
pub mod licence;
pub mod live;
pub mod animation;
pub mod metaball;
pub mod surface;
pub mod traverse;
