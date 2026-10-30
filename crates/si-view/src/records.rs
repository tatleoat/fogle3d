// SPDX-License-Identifier: MIT
//! Records the graph list reads that `si-types` does not lay out. Each field's evidence is
//! in finding 0027.

use si_mem::{record, Opaque, Ptr32};

record! {
    /// The per-process record `SI_procData` points to (`SI_procDataStorage`, 12 bytes of
    /// `.bss`).
    pub struct ProcData = "SI_procData record", size 0x0c {
        0x00 dict: i32, "index of the current element dictionary in SI_elemDicts";
        0x04 list: i32, "index of the scene's graph list in SI_grafLists (SI_grafListCreate stores 0)";
        0x08 camera: Ptr32<Opaque>, "the scene's camera (SI_itemGetByName answers it for kinds 4 and 5)";
    }
}

record! {
    /// A named selection or a tag (`SI_NamedSelection`, 12 bytes from `SI_malloc`): what a
    /// graph node of kind 0xc stands for.
    pub struct NamedSelection = "SI_NamedSelection", size 0x0c {
        0x00 kind: i32, "0 a named selection, 1 a tag (SI_nselectionAllocate; SI_nselectionUpdate makes it 1 for an element with a relation 0x2711)";
        0x04 name: si_mem::CStr32, "the name: the pointer SI_nselectionAllocate was handed; nselectionDispose frees it";
        0x08 unk_08: u32, "0 at creation; no other access in nselections.c";
    }
}

record! {
    /// What a relation from a named selection to an animation carries after a scene was
    /// read (`SI_NSelFcvData`, 16 bytes): which curve of which element is meant.
    pub struct NSelFcvData = "SI_NSelFcvData", size 0x10 {
        0x00 chap: i32, "chapter of the element that owns the curve";
        0x04 elem: i32, "its index";
        0x08 fcv_type: i32, "the curve's type: compared with the first word of a curve";
        0x0c nth: i32, "which of the element's curves of that type, counted from 0";
    }
}

/// Bits of `GrafNode::flags` (+0x16) as `graflist.c` uses them. Read from code; the names
/// are ours.
pub mod flag {
    /// Not selectable (toggled by `SI_setSelectability`).
    pub const UNSELECTABLE: u16 = 0x0001;
    /// Tested by `SI_selectGrafnode` together with a manipulation global before
    /// `SI_setUItoPrefFlags` is called.
    pub const PREF: u16 = 0x0800;
    /// The node is not editable (the reduced product's marker): a list keeps such a node as
    /// its second active node, and its selection lives in the three bits below instead of
    /// the selection word.
    pub const UNEDITABLE: u16 = 0x1000;
    /// Selection of a node that is not editable: as a node …
    pub const SEL_NODE: u16 = 0x2000;
    /// … as a branch …
    pub const SEL_BRANCH: u16 = 0x4000;
    /// … inherited from a branch.
    pub const SEL_INHERITED: u16 = 0x8000;
    /// The three together.
    pub const SEL_ANY: u16 = 0xe000;
}

/// Values of `GrafNode::selection` (+0x08).
pub mod sel {
    pub const NONE: i32 = 0;
    pub const NODE: i32 = 1;
    pub const INHERITED: i32 = 2;
    pub const BRANCH: i32 = 3;
}

/// Values of `GrafNode::kind` (+0x00) beyond those `si-types` names, as the routines of
/// `graflist.c` compare them. What each stands for is read from the callees they are
/// handed to (`SI_lightDispose`, `SI_fcurveRemove`, `SI_edDisposeAnimFile`, …).
pub mod kind {
    pub const SPLINE: i32 = 1;
    pub const MODEL_ROOT: i32 = 2;
    pub const MODEL_NODE: i32 = 3;
    pub const CAMERA: i32 = 4;
    pub const CAMERA_ROOT: i32 = 5;
    pub const LIGHT: i32 = 6;
    pub const LIGHT_ROOT: i32 = 7;
    pub const LIGHT_8: i32 = 8;
    pub const FCURVE: i32 = 9;
    pub const WAVE: i32 = 0xb;
    pub const NSELECTION: i32 = 0xc;
    pub const MATERIAL: i32 = 0xe;
    pub const TEXTURE_2D: i32 = 0xf;
    pub const TEXTURE_3D: i32 = 0x10;
    pub const ANIM_FILE: i32 = 0x12;
    pub const LIGHT_13: i32 = 0x13;
    pub const LIGHT_14: i32 = 0x14;
}
