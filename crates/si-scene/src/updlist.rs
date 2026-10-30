// SPDX-License-Identifier: MIT
//! The update list as a value: the ordered list of things to recompute that the
//! topological sort of the dictionary produces.
//!
//! The list itself is in the memory image (`SI_UpdList` at +0x0c of the dictionary, its
//! steps records of `si_dict::records`), made by `si-dict`'s ports of `toposort.c` and
//! `updlist.c`. What is here is the copy of it that [`crate::world::to_stores`] makes for
//! the code of this crate that is not on the image and for the checks' reports.
// adapter: not in place

use si_types::{Opaque, Ptr32};

/// One argument of an update step (20 bytes in the image): an element the step's relation
/// reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UpdArg {
    /// +0x00 (16 bits): the chapter of the argument element; +0x02 (16 bits): the list, on
    /// that element, that holds the relation's other entry; +0x04: its index.
    pub chapid: i16,
    pub group: i16,
    pub elemid: i32,
    /// +0x08: the argument element's object.
    pub data: Ptr32<Opaque>,
    /// +0x0c (16 bits): the relation entry's word at +0x14 (non-zero: active).
    pub flag: i16,
    /// +0x10: the curve of the relation's active range (`SI_edGetFcvFromRelation`): the
    /// object of the animation element the relation owns, null when it owns none.
    pub fcurve: Ptr32<Opaque>,
}

/// One step of an update list (`SI_UpdElem`, 0x2c bytes in the image).
#[derive(Clone, Debug, PartialEq)]
pub struct UpdNode {
    /// +0x00 (16 bits): the chapter of the element to update. 0xffff in the end mark the
    /// sort appends for the viewer.
    pub chapid: i16,
    /// +0x02 (16 bits): the relation list the node comes from (1 transformation, 2 geometry …).
    pub group: i16,
    /// +0x04: the element's index.
    pub elemid: i32,
    /// +0x08: the element's object.
    pub data: Ptr32<Opaque>,
    /// +0x14: what to do: a relation id, or -100 (recompute a model's transformation), -101
    /// (the same for a wave), -200 (recompute a model's geometry).
    pub relid: i32,
    /// +0x18 (16 bits): number of arguments; +0x1c: the arguments.
    pub args: Vec<UpdArg>,
    /// +0x20 (16 bits): 1, or the "active" word of the relation entry whose data the node
    /// carries.
    pub flag: i16,
    /// +0x24: the relation's data, null when the relation has none.
    pub rel_data: Ptr32<Opaque>,
    // +0x0c, +0x10: not set by the routine. +0x28: a pointer, null here, freed with the node.
}

/// The nodes of one update list. The record's count equals `nodes.len()`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdListStore {
    pub nodes: Vec<UpdNode>,
}
