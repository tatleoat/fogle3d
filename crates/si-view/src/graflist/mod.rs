// SPDX-License-Identifier: MIT
//! `graflist.c` (0x0832bfd0 to 0x08339a6e, 119 functions): the graph list.
//!
//! | Module | Content |
//! |---|---|
//! | [`list`] | the array of lists, a list's nodes: creation, insertion, removal, the walks and the look-ups |
//! | [`flags`] | selection state and display type of the nodes: the counts and the routines behind the display menus |
//! | [`select`] | selecting, unselecting and toggling a node, with what that does to the nodes below it and to the list's active node |
//! | [`events`] | the selection commands of the menus, on the scene's own list |
//! | [`edit`] | adding an object to a list and taking one out: the nodes made, their order, and the calls into the rest of the program |
//! | [`status`] | the status line's text for the selection; a point's distance from a camera |
//! | [`scene`] | emptying a scene: chapter data, elements, the list, and the camera put back |
//!
//! A graph node (`__SI_GrafNode`, 0x24 bytes, `si_types::scene::GrafNode`) stands for one
//! thing a view can show: a model node, a camera, a light, a wave, a material, a texture.
//! A list (`SI_GrafList`, 0x24 bytes) chains its nodes both ways through +0x1c and +0x20.
//! **The chain's order carries the hierarchy**: a model's root node (kind 2) is followed
//! at once by the nodes below it (kind 3), depth first, and its word at +0x10 says how
//! many they are; the node in front of a camera (kind 5) or a light (kind 7) is followed by
//! the object's own node (4, 6) and counts 1.

pub mod edit;
pub mod events;
pub mod flags;
pub mod list;
pub mod scene;
pub mod select;
pub mod status;

pub use edit::*;
pub use events::*;
pub use flags::*;
pub use list::*;
pub use scene::*;
pub use select::*;
pub use status::*;
