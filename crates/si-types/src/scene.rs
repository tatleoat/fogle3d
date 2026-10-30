// SPDX-License-Identifier: MIT
//! Layouts of the scene-graph node and of the objects other than models: materials, 2D and
//! 3D textures, cameras and lights. These are a first pass: sizes are from the allocators,
//! and only the fields met while reading the dictionaries and the model are placed. The
//! rest of each record is one or more unknown blocks, to be divided by the packages that
//! own these objects.

use crate::geom::Vector;
use crate::{layout, CStr32, Code, Opaque, Ptr32, Zero};

/// Values of `GrafNode::kind` seen in `SI_graflistAddData` (the original's `SI_GrafType`).
pub const GRAF_MODEL_ROOT: i32 = 2;
pub const GRAF_MODEL_NODE: i32 = 3;
pub const GRAF_CAMERA: i32 = 4;
pub const GRAF_CAMERA_ROOT: i32 = 5;
pub const GRAF_LIGHT: i32 = 6;
pub const GRAF_LIGHT_ROOT: i32 = 7;
pub const GRAF_WAVE: i32 = 0xb;
pub const GRAF_MATERIAL: i32 = 0xe;
pub const GRAF_TEXTURE_2D: i32 = 0xf;
pub const GRAF_TEXTURE_3D: i32 = 0x10;

layout! {
    /// A node of the list of displayable objects (`__SI_GrafNode`): one per model node,
    /// camera, light, wave, material or texture on screen.
    pub struct GrafNode = "__SI_GrafNode", size 0x24,
        "grafnodeAllocate allocates 0x24 bytes" {
        0x00 kind: i32, Read, "What data is: 2 the root node of a model, 3 another node of it, 4 a camera, 6 a light, 5 and 7 the nodes created in front of those, 0xb a wave, 0xe a material, 0xf and 0x10 textures (SI_graflistAddData).";
        0x04 data: Ptr32<Opaque>, Read, "The object: a __SI_Model for kinds 2 and 3.";
        0x08 selection: i32, Read, "Selection state: 0 none, 1 node, 2 inherited, 3 branch; the element's attribute 265 when a graf node exists.";
        0x0c wire_type: i32, Read, "Display type (0 hidden, 1 ghosted, 2 visible), 2 at creation; attribute 268.";
        0x10 nb_sub: i16, Read, "For a model node, its number of sub-models (SI_modelNbSubModels); 1 for the node in front of a camera or light.";
        0x12 wire_color: i16, Read, "Wire-frame colour index: SI_select7WM + SI_color1 when added; attribute 267.";
        0x14 unk_14: i16, Unknown, "0 at creation; SI_sceneKeySetColor writes it.";
        0x16 flags: u16, Named, "Bit 0 clear = selectable (attribute 567); bit 0x1000 is tested when a relation is removed.";
        0x18 func: Ptr32<Code>, Named, "A function pointer set by SI_updateGnFunc and called when the node is drawn.";
        0x1c next: Ptr32<GrafNode>, Read, "Next node (graflistInsertGrafnode).";
        0x20 prev: Ptr32<GrafNode>, Read, "Previous node.";
    }
}

layout! {
    /// A list of graf nodes (`SI_GrafList`).
    pub struct GrafList = "SI_GrafList", size 0x24,
        "SI_grafListAllocate allocates 0x24 bytes; the global array SI_grafLists is indexed in steps of 0x24" {
        0x00 multi: i16, Named, "0 = one active node at a time: SI_graflistAddData then unselects the previous one.";
        0x02 pad_02: [u8; 2], Unknown, "No access found.";
        0x04 first: Ptr32<GrafNode>, Read, "Head of the list (graflistInsertGrafnode).";
        0x08 active: Ptr32<GrafNode>, Named, "The node SI_graflistSetActive made active.";
        0x0c unk_0c: i16, Unknown, "0 at creation.";
        0x0e unk_0e: i16, Unknown, "1 at creation.";
        0x10 unk_10: [u32; 4], Unknown, "Partly cleared at creation; not read.";
        0x20 active2: Ptr32<GrafNode>, Named, "Used in place of active when that is null.";
    }
}

impl GrafNode {
    /// A new node [soft 0x0832c890 grafnodeAllocate]: everything zero except the display
    /// type, which is 2 (visible). The original sets every field.
    pub fn allocate() -> GrafNode {
        let mut g = GrafNode::zero();
        g.wire_type = 2;
        g
    }
}

/// Links `node` into `list` after `after`, or at the head when `after` is `None`
/// [soft 0x0832c8f0 graflistInsertGrafnode]. Nodes are addressed by index into `nodes`;
/// index `i` is the pointer value `i + 1`.
pub fn graflist_insert(list: &mut GrafList, nodes: &mut [GrafNode], after: Option<usize>, node: usize) {
    let ptr = |i: usize| Ptr32::<GrafNode>::new(i as u32 + 1);
    match after {
        None => {
            nodes[node].next = list.first;
            list.first = ptr(node);
        }
        Some(a) => {
            nodes[node].next = nodes[a].next;
            nodes[a].next = ptr(node);
        }
    }
    if !nodes[node].next.is_null() {
        let n = nodes[node].next.0 as usize - 1;
        nodes[n].prev = ptr(node);
    }
    nodes[node].prev = match after {
        None => Ptr32::NULL,
        Some(a) => ptr(a),
    };
}

layout! {
    /// A material (`__SI_Material`).
    pub struct Material = "__SI_Material", size 0xdcc,
        "SI_materialAllocate allocates 0xdcc bytes; the symbol SI_defaultMaterial is 0xdcc bytes" {
        0x000 name: CStr32, Read, "Written first by si_materialBinaryWrite, as a string.";
        0x004 unk_004: [u8; 0x1a], Unknown, "Not placed.";
        0x01e unk_01e: i16, Unknown, "Written by si_materialBinaryWrite as 16 bits.";
        0x020 unk_020: [u8; 8], Unknown, "Not placed.";
        0x028 shading: i16, Named, "Second value written by si_materialBinaryWrite: the shading model.";
        0x02a pad_02a: [u8; 2], Unknown, "Not placed.";
        0x02c unk_02c: f32, Unknown, "Third value written by si_materialBinaryWrite.";
        0x030 unk_030: [u8; 4], Unknown, "Not placed.";
        0x034 ambient: [f32; 4], Read, "Attributes 45 to 47 (MatAmbient) are the first three floats.";
        0x044 diffuse: [f32; 4], Read, "Attributes 48 to 50 (MatDiffuse).";
        0x054 specular: [f32; 4], Read, "Attributes 51 to 53 (MatSpecular).";
        0x064 spec_decay: f32, Read, "Attribute 54 (MatSpecDecay).";
        0x068 unk_068: [u8; 0x640], Unknown, "Not placed.";
        0x6a8 reflect: f32, Read, "Attribute 55 (MatReflect).";
        0x6ac unk_6ac: [u8; 0xc], Unknown, "Not placed.";
        0x6b8 transp: f32, Read, "Attribute 57 (MatTransp).";
        0x6bc unk_6bc: [u8; 0x20], Unknown, "Not placed.";
        0x6dc refract: f32, Named, "Written after reflectivity and transparency by si_materialBinaryWrite, where the file has the refraction index.";
        0x6e0 unk_6e0: [u8; 0x6dc], Unknown, "Not placed.";
        0xdbc unk_dbc: Ptr32<Opaque>, Unknown, "Tested by si_materialBinaryWrite.";
        0xdc0 unk_dc0: [u8; 4], Unknown, "Not placed.";
        0xdc4 user_data: Ptr32<Opaque>, Named, "A user-data list tested by si_materialBinaryWrite.";
        0xdc8 unk_dc8: [i16; 2], Unknown, "Written by si_materialBinaryWrite as two 16-bit values.";
    }
}

layout! {
    /// A 2D texture applied to a model (`__SI_LclMap`).
    pub struct LclMap = "__SI_LclMap", size 0x1d8,
        "SI_textureAllocate allocates 0x1d8 bytes; the symbol SI_defaultMap is 0x1d8 bytes" {
        0x000 unk_000: [u8; 0x16c], Unknown, "Not placed.";
        0x16c next: Ptr32<LclMap>, Read, "Next 2D texture of the same model: SI_modelBinaryWriteNode and SI_modelDisposeNode walk the list through it.";
        0x170 unk_170: [u8; 0x68], Unknown, "Not placed.";
    }
}

layout! {
    /// A 3D texture (`__SI_Texture3D`).
    pub struct Texture3D = "__SI_Texture3D", size 0x158,
        "SI_texture3DAllocate allocates 0x158 bytes" {
        0x000 unk_000: [u8; 0x10], Unknown, "Not placed.";
        0x010 spacing: f32, Read, "Attribute 69 (T3DSpacing).";
        0x014 unk_014: [u8; 4], Unknown, "Not placed.";
        0x018 strength: f32, Read, "Attribute 68 (T3DStrength).";
        0x01c iteration: u32, Named, "Attribute 405 (T3DIteration) reads this word and the one at +0x120.";
        0x020 power: f32, Read, "Attribute 70 (T3DPower).";
        0x024 unk_024: [u8; 4], Unknown, "Not placed.";
        0x028 blend: f32, Read, "Attribute 155 (T3DBlend).";
        0x02c unk_02c: [u8; 4], Unknown, "Not placed.";
        0x030 colors: [[f32; 4]; 5], Read, "Attributes 76 to 90 and 156 to 160 (T3DColor0 to T3DColor4, red, green, blue, alpha).";
        0x080 factors: [f32; 3], Read, "Attributes 73 to 75 (T3DFact1 to T3DFact3).";
        0x08c unk_08c: [u8; 4], Unknown, "Not placed.";
        0x090 scale: Vector, Read, "Attributes 91 to 93 (T3DScale).";
        0x09c rotation: Vector, Read, "Attributes 94 to 96 (T3DRotation).";
        0x0a8 transl: Vector, Read, "Attributes 97 to 99 (T3DTransl).";
        0x0b4 unk_0b4: [u8; 0x54], Unknown, "Not placed.";
        0x108 ambient: f32, Read, "Attribute 100 (T3DAmbient).";
        0x10c diffuse: f32, Read, "Attribute 101 (T3DDiffuse).";
        0x110 specular: f32, Read, "Attribute 102 (T3DSpecular).";
        0x114 reflection: f32, Read, "Attribute 103 (T3DReflection).";
        0x118 transp: f32, Read, "Attribute 104 (T3DTransp).";
        0x11c unk_11c: [u8; 4], Unknown, "Not placed.";
        0x120 iteration2: u32, Named, "Read with iteration for attribute 405.";
        0x124 unk_124: [u8; 0x34], Unknown, "Not placed.";
    }
}

layout! {
    /// A camera (`SI_Camera`).
    pub struct Camera = "SI_Camera", size 0x13c,
        "SI_newCamera allocates 0x13c bytes; the symbol si_paintCamera is 0x13c bytes" {
        0x000 name: CStr32, Named, "Read as a string by three functions; freed by cameraFree.";
        0x004 unk_004: [u8; 0xc], Unknown, "Five 16-bit values written by SI_cameraTransfer; not placed.";
        0x010 position: Vector, Named, "SI_addCamTrans adds a translation to these three floats.";
        0x01c interest: Vector, Named, "SI_addCitTrans adds a translation to these three floats.";
        0x028 unk_028: [u8; 0xf8], Unknown, "Not placed.";
        0x120 frustum: [f32; 6], Named, "SI_camSetFrustum and SI_camGetFrustum.";
        0x138 user_data: Ptr32<Opaque>, Named, "Passed by address to the camera's binary reader as a user-data list.";
    }
}

layout! {
    /// A light (`light`).
    pub struct Light = "light", size 0x240,
        "SI_lightAllocate allocates 0x240 bytes; the symbol SI_defaultLight is 0x240 bytes" {
        0x000 name: [u8; 0x1e], Named, "An inline string: si_lightInitialize copies the eight bytes of the text default here; 30 bytes is the room before kind.";
        0x01e kind: i16, Read, "The second argument of si_lightInitialize, which chooses defaults for 1, 2, 3 and 4: the resource file's LigType (point, spot, infinite, sun).";
        0x020 unk_020: [u8; 4], Unknown, "Not placed.";
        0x024 color: [f32; 3], Named, "1, 1, 1 at initialisation; SI_lightSetParam writes it.";
        0x030 unk_030: [u8; 4], Unknown, "0 at initialisation.";
        0x034 position: Vector, Named, "SI_addLigTrans adds a translation to these three floats.";
        0x040 unk_040: [u8; 0x18], Unknown, "Six floats, 0 at initialisation.";
        0x058 interest: Vector, Named, "SI_addLitTrans adds a translation to these three floats.";
        0x064 unk_064: [u8; 0x1dc], Unknown, "Not placed.";
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graf_nodes_link_both_ways() {
        let mut list = GrafList::zero();
        let mut nodes = vec![GrafNode::allocate(); 3];
        assert_eq!(nodes[0].wire_type, 2);
        graflist_insert(&mut list, &mut nodes, None, 0);
        graflist_insert(&mut list, &mut nodes, None, 1); // new head
        graflist_insert(&mut list, &mut nodes, Some(1), 2); // between 1 and 0
        let p = |i: u32| Ptr32::<GrafNode>::new(i + 1);
        assert_eq!(list.first, p(1));
        assert_eq!((nodes[1].next, nodes[2].next, nodes[0].next), (p(2), p(0), Ptr32::NULL));
        assert_eq!((nodes[1].prev, nodes[2].prev, nodes[0].prev), (Ptr32::NULL, p(1), p(2)));
    }
}
