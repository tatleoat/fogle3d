// SPDX-License-Identifier: MIT
//! `bool_util.c` (0x083170d0 .. 0x08317267, finding 0037, "bool3"): the six constructors of the boolean engine's
//! records. Each takes its block from `si_getMem` (the file's own block allocator) and fills the fields named below;
//! the other bytes of a fresh block are whatever the allocator's memory held (the original leaves them alone, so does
//! the port).

use crate::boolean::si_get_mem;
use si_mem::{Mem, Outside};

/// `si_createFaceGroup()` [soft 0x083170d0 si_createFaceGroup]: 0x34 bytes, the list head at +0x30 cleared.
pub fn si_create_face_group(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let p = si_get_mem(m, o, 0x34);
    m.set_u32(p.wrapping_add(0x30), 0);
    p
}

/// `si_createEdgeNode()` [soft 0x083170ec si_createEdgeNode]: 0x14 bytes, +0 and +8 cleared, the byte at +4 is 2
/// (end flag), +0xc and +0x10 (next and previous) point at the node itself.
pub fn si_create_edge_node(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let p = si_get_mem(m, o, 0x14);
    m.set_u32(p, 0);
    m.set_u8(p.wrapping_add(4), 2);
    m.set_u32(p.wrapping_add(8), 0);
    m.set_u32(p.wrapping_add(0x10), p);
    m.set_u32(p.wrapping_add(0xc), p);
    p
}

/// `si_createContour(first)` [soft 0x08317118 si_createContour]: 0xc bytes, the first edge node at +0, +4 and +8 cleared.
pub fn si_create_contour(m: &mut Mem, o: &mut dyn Outside, first: u32) -> u32 {
    let p = si_get_mem(m, o, 0xc);
    m.set_u32(p, first);
    m.set_u32(p.wrapping_add(8), 0);
    m.set_u32(p.wrapping_add(4), 0);
    p
}

/// `si_createBoolFace(src)` [soft 0x08317144 si_createBoolFace]: 0x80 bytes. With `src` (a face) the 0x20 words are
/// copied (`rep movsd`); without, the fields the original sets are written (words 0..7, +0x50, +0x58, +0x5c, +0x60,
/// the byte +0x64 = 2, the byte +0x65 = 0xff, +0x68, +0x6c, +0x70, +0x7c = -1). Either way +0x54, +0x74 and +0x78 are
/// cleared last.
pub fn si_create_bool_face(m: &mut Mem, o: &mut dyn Outside, src: u32) -> u32 {
    let p = si_get_mem(m, o, 0x80);
    if src == 0 {
        for k in [0u32, 1, 2, 3, 4, 5, 6, 7, 0x18, 0x14, 0x16, 0x17] {
            m.set_u32(p.wrapping_add(k * 4), 0);
        }
        m.set_u8(p.wrapping_add(0x64), 2);
        m.set_u32(p.wrapping_add(0x70), 0);
        m.set_u32(p.wrapping_add(0x6c), 0);
        m.set_u32(p.wrapping_add(0x68), 0);
        m.set_u8(p.wrapping_add(0x65), 0xff);
        m.set_u32(p.wrapping_add(0x7c), 0xffff_ffff);
    } else {
        m.copy(p, src, 0x80);
    }
    m.set_u32(p.wrapping_add(0x78), 0);
    m.set_u32(p.wrapping_add(0x74), 0);
    m.set_u32(p.wrapping_add(0x54), 0);
    p
}

/// `si_createRegBspNode(region)` [soft 0x08317208 si_createRegBspNode]: 0x30 bytes, kind 1 (leaf) at +0, the region
/// type at +8, +4 cleared.
pub fn si_create_reg_bsp_node(m: &mut Mem, o: &mut dyn Outside, region: u32) -> u32 {
    let p = si_get_mem(m, o, 0x30);
    m.set_u32(p, 1);
    m.set_u32(p.wrapping_add(8), region);
    m.set_u32(p.wrapping_add(4), 0);
    p
}

/// `si_createFaceList(tree, face)` [soft 0x08317234 si_createFaceList]: 0x10 bytes, the face at +0, the tree at +4,
/// +8 and +0xc cleared.
pub fn si_create_face_list(m: &mut Mem, o: &mut dyn Outside, tree: u32, face: u32) -> u32 {
    let p = si_get_mem(m, o, 0x10);
    m.set_u32(p.wrapping_add(0xc), 0);
    m.set_u32(p.wrapping_add(8), 0);
    m.set_u32(p.wrapping_add(4), tree);
    m.set_u32(p, face);
    p
}
