// SPDX-License-Identifier: MIT
//! Records and named offsets of the native hand-over, on the shared image (finding 0024).
//!
//! Model nodes, graf nodes, graf lists and the dictionary records are `si-types`' layouts
//! ([`si_types::model::Model`], [`si_types::scene::GrafNode`], [`si_types::scene::GrafList`],
//! [`si_types::dict::ElemDict`]). What `si-types` has no layout or no field for is here: two
//! small records of our own reading, and named offsets into records whose layout is not
//! established (the render controls, a light, a camera, an event).
//!
//! Nothing in this file reproduces a function of the original.

use si_mem::{record, Opaque, Ptr32};

record! {
    /// One entry of `memList`: a block to free once the frame is rendered. Read from
    /// `addMemEntry` (8 bytes from `SI_calloc`) and `SI_deleteMemReferences`.
    pub struct MemEntry = "memList entry", size 0x08 {
        0x00 block: Ptr32<Opaque>, "the block: the material array of a node copied from an instance's master";
        0x04 next: Ptr32<MemEntry>, "the entry made before this one";
    }
}

record! {
    /// The first three words of the record `SI_procData` points to (the record is longer;
    /// nothing in these files reads beyond +0x08).
    pub struct ProcData = "SI_procData record", size 0x0c {
        0x00 dict: i32, "index of the current dictionary in SI_elemDicts";
        0x04 graf_list: i32, "index of the scene's graf list in SI_grafLists";
        0x08 camera: Ptr32<Opaque>, "the scene's camera (SI_Camera)";
    }
}

record! {
    /// One vertex of an envelope's list: 0x14 bytes from `SI_malloc`
    /// (`SI_envelopVertAllocate`). The meanings of +0x04 and +0x08 are `si-hrc`'s reading of
    /// the hierarchy file (finding 0018).
    pub struct EnvVert = "envelope vertex", size 0x14 {
        0x00 index: i32, "the vertex";
        0x04 ratio: f32, "0 at creation";
        0x08 thickness: f32, "0 at creation";
        0x0c weight: f32, "100 at creation";
        0x10 next: Ptr32<EnvVert>, "the next vertex of the list";
    }
}

record! {
    /// One list of an envelope: 0x10 bytes from `SI_malloc` (`SI_envelopListAllocate`).
    pub struct EnvList = "envelope list", size 0x10 {
        0x00 owner: u32, "the routine's argument (a copy made by envelopInfoCopy has 0)";
        0x04 verts: Ptr32<EnvVert>, "the vertices";
        0x08 prev: Ptr32<EnvList>, "the list before this one";
        0x0c next: Ptr32<EnvList>, "the list after this one";
    }
}

record! {
    /// A node's envelope record (`__SI_Model` +0x324): 0xa0 bytes from `SI_malloc`
    /// (`SI_envelopInfoAllocate`). Read from that routine and `envelopInfoCopy`; the names
    /// of the settings are `si-hrc`'s.
    pub struct EnvInfo = "envelope record", size 0xa0 {
        0x00 level: i32, "-1 at creation; what SI_envelopLevel answers";
        0x04 lists: Ptr32<EnvList>, "the first list";
        0x08 matrix: [[f32; 4]; 4], "identity at creation";
        0x48 unk_48: [u32; 9], "nine words cleared at creation; not copied";
        0x6c joint_area: f32, "0 at creation";
        0x70 in_type: i32, "0 at creation";
        0x74 in_curve: Ptr32<f32>, "200 floats, or null";
        0x78 inner: [f32; 4], "0, 1, 0, 1 at creation";
        0x88 out_type: i32, "0 at creation";
        0x8c out_curve: Ptr32<f32>, "200 floats, or null";
        0x90 outer: [f32; 4], "0, 1, 0, 1 at creation";
    }
}

record! {
    /// One compensation record of a node in classic manipulation mode (0x1c bytes).
    pub struct CompInfo = "compensation record", size 0x1c {
        0x00 key: u32, "copied as a word";
        0x04 a: [u32; 3], "three words (a vector)";
        0x10 b: [u32; 3], "three words (a vector)";
    }
}

record! {
    /// A node's compensation records (`__SI_Model` +0x3e8): 8 bytes from `SI_malloc`
    /// (`compensateInfoListAllocate`).
    pub struct CompInfoList = "SI_CompInfoList", size 0x08 {
        0x00 count: i32, "number of records";
        0x04 records: Ptr32<CompInfo>, "the records, from SI_calloc(count, 0x1c)";
    }
}

/// Offsets into the render controls (`SI_RenControl`, 0x600 bytes: `SI_defaultControl` and
/// the renderer's copy `SI_ren`). The meanings are finding 0012's table of `SI_ren`, except
/// where a line says "read here".
pub mod ctl {
    /// 16 bits: 2 while a sequence is rendered; the preview stores 1.
    pub const SEQUENCE: u32 = 0x09e;
    /// 16 bits, read here: when set (and the preview is not in a dialogue) the picture's
    /// height follows from its width, the camera's ratio and the pixel ratio.
    pub const HEIGHT_FROM_RATIO: u32 = 0x0fe;
    /// 16 bits: field rendering.
    pub const FIELDS: u32 = 0x13c;
    /// Four 16-bit words, read here: 0, width − 1, 0, height − 1 (the picture's box).
    pub const BOX_X0: u32 = 0x142;
    pub const BOX_X1: u32 = 0x144;
    pub const BOX_Y0: u32 = 0x146;
    pub const BOX_Y1: u32 = 0x148;
    /// Float: pixel ratio.
    pub const PIXEL_RATIO: u32 = 0x154;
    /// 16 bits: sampling level.
    pub const LEVEL: u32 = 0x158;
    /// 16 bits: adaptive supersampling.
    pub const ADAPTIVE: u32 = 0x15a;
    /// 16 bits: adaptive sub-samples per pixel side.
    pub const ADAPTIVE_SAMPLES: u32 = 0x15c;
    /// Float: adaptive contrast threshold.
    pub const ADAPTIVE_THRESHOLD: u32 = 0x16c;
    /// Two 16-bit words: where the picture's window is put on the screen.
    pub const WINDOW_X: u32 = 0x174;
    pub const WINDOW_Y: u32 = 0x176;
    /// 16 bits: display on screen.
    pub const DISPLAY: u32 = 0x178;
    /// 16 bits: write a picture file.
    pub const PICTURE_FILE: u32 = 0x17a;
    /// 16 bits: the per-line statistics; the render window sets it when it disables the
    /// display (read here as written only).
    pub const LINE_STATISTICS: u32 = 0x1ec;
    /// Three 16-bit words: first frame, last frame, step.
    pub const FRAME_FIRST: u32 = 0x1f4;
    pub const FRAME_LAST: u32 = 0x1f6;
    pub const FRAME_STEP: u32 = 0x1f8;
    /// 16 bits: crop (sub-region) on.
    pub const CROP: u32 = 0x224;
    /// 16-bit switches of the sub-region box (finding 0023, section 11).
    pub const SUBREGION_226: u32 = 0x226;
    pub const SUBREGION_228: u32 = 0x228;
    pub const SUBREGION_22A: u32 = 0x22a;
    /// Four floats: the sub-region as fractions of the picture (left, right, top, bottom);
    /// `SI_updateSubregionValues` turns them into pixel counts in place.
    pub const CROP_LEFT: u32 = 0x22c;
    pub const CROP_RIGHT: u32 = 0x230;
    pub const CROP_TOP: u32 = 0x234;
    pub const CROP_BOTTOM: u32 = 0x238;
    /// Two 16-bit words: the cropped picture's width and height.
    pub const CROP_WIDTH: u32 = 0x23c;
    pub const CROP_HEIGHT: u32 = 0x23e;
    /// 16 bits: depth file output.
    pub const DEPTH_FILE: u32 = 0x24c;
    /// Read here: 31 words (0x7c bytes) the preview copies over the block at +0x584: the
    /// preview's own switches and picture size.
    pub const PREVIEW_BLOCK: u32 = 0x508;
    /// The block the preview's copy lands on; its first six 16-bit words are the switches
    /// textures, 2D textures, 3D textures, reflection, refraction, shadows.
    pub const SWITCHES: u32 = 0x584;
    /// Two 16-bit words: picture width and height.
    pub const WIDTH: u32 = 0x59a;
    pub const HEIGHT: u32 = 0x59c;
    /// Read here, inside the preview's block once copied: a 16-bit switch "the preview has
    /// sampling settings of its own", and those settings (adaptive, level, sub-samples as
    /// 16-bit words, the threshold as a float).
    pub const PREVIEW_OWN_SAMPLING: u32 = 0x5b8;
    pub const PREVIEW_ADAPTIVE: u32 = 0x5bc;
    pub const PREVIEW_LEVEL: u32 = 0x5c0;
    pub const PREVIEW_SAMPLES: u32 = 0x5c4;
    pub const PREVIEW_THRESHOLD: u32 = 0x5d4;
}

/// Offsets into a light record (`light`, 0x240 bytes; finding 0012).
pub mod light {
    /// 16 bits: the light's type (1 point, 2 spot, 3 infinite, 4 sun).
    pub const TYPE: u32 = 0x1e;
    /// The record's size.
    pub const SIZE: u32 = 0x240;
    /// The next light of the chain handed to the renderer.
    pub const NEXT: u32 = 0x140;
    /// 16 bits: the light is on.
    pub const ON: u32 = 0x176;
}

/// Offsets into a camera record (`SI_Camera`, 0x13c bytes).
pub mod camera {
    /// The record's size.
    pub const SIZE: u32 = 0x13c;
    /// Float: the picture ratio the camera was set up for (read here: the preview divides
    /// the picture's width by it).
    pub const RATIO: u32 = 0x64;
}

/// Offsets into the record `SI_getEvent` answers (read here, from `SI_swapMaster` only).
pub mod event {
    /// 32 bits: the kind; 5 is a mouse button, 0 nothing.
    pub const KIND: u32 = 0x00;
    /// 16 bits: the button's code. `SI_swapMaster` takes 0x65 as "cancel" and 0x66, 0x67
    /// as a pick.
    pub const BUTTON: u32 = 0x06;
    /// The window (`__SI_Grafport`) the event happened in.
    pub const WINDOW: u32 = 0x0c;
}

/// Bits of a model node's flag word (+0x08), as these files use them.
pub mod flag {
    /// Carried from an instance to the copy of its master.
    pub const INVERSE: u16 = 0x0008;
    /// The instance has been handed to the renderer (or expanded) in this visit.
    pub const VISITED: u16 = 0x0020;
    /// The node is a copy "without geometry".
    pub const COPY: u16 = 0x0040;
    /// The instance is being expanded: the hierarchy copy does not go into it again.
    pub const EXPANDING: u16 = 0x0200;
}

/// The algorithm number (+0x2ec) of an instance.
pub const ALGO_INSTANCE: i32 = 15;
/// Relation 20002: instance to master (list 4).
pub const REL_INSTANCE: u32 = 0x4e22;
