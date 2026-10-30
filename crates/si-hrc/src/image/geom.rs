// SPDX-License-Identifier: MIT
//! The allocators of a polygon mesh's records (`mesh_b.c`, `obj_global.c`), on the image.
//!
//! `si-types` has the records (`Mesh`, `Vertex`, `Polygon`, `PolyNode`, `Edge`); the
//! readers address their fields by the offsets of [`mesh`] below, which are the same
//! layout, and type a pointer to a mesh with `si-types`' record.

use super::Outside;
use si_mem::{Mem, Ptr32};
pub use si_types::geom::Mesh;

/// Offsets in a polygon mesh and its arrays.
pub mod mesh {
    /// The vertex array (20 bytes a vertex).
    pub const VERTICES: u32 = 0x00;
    /// The polygon array (24 bytes a polygon).
    pub const POLYGONS: u32 = 0x04;
    /// The table of the edge blocks the rebuild makes (1024 edges of 20 bytes a block).
    pub const EDGES: u32 = 0x08;
    /// The discontinuity angle in degrees, and its cosine.
    pub const ANGLE: u32 = 0x0c;
    pub const ANGLE_COS: u32 = 0x10;
    pub const NB_VERTICES: u32 = 0x14;
    pub const NB_POLYGONS: u32 = 0x18;
    /// The number of edges, numbered from 1 in the edge table.
    pub const NB_EDGES: u32 = 0x1c;
    pub const FLAGS: u32 = 0x20;
    pub const SIZE: u32 = 0x54;
    /// A vertex: position, star array, star count, flag.
    pub const VERTEX: u32 = 0x14;
    pub const VERTEX_STAR: u32 = 0x0c;
    pub const VERTEX_NB_STAR: u32 = 0x10;
    pub const VERTEX_FLAG: u32 = 0x12;
    /// A polygon: normal, corner array, corner count, material index, flag.
    pub const POLYGON: u32 = 0x18;
    pub const POLYGON_NODES: u32 = 0x0c;
    pub const POLYGON_NB_NODES: u32 = 0x10;
    pub const POLYGON_MATERIAL: u32 = 0x12;
    pub const POLYGON_FLAG: u32 = 0x14;
    /// A corner (`SI_Node`): vertex index, edge, normal, texture pair, colour, further
    /// texture pairs.
    pub const NODE: u32 = 0x24;
    pub const NODE_NORMAL: u32 = 0x08;
    pub const NODE_UV: u32 = 0x14;
    pub const NODE_COLOUR: u32 = 0x1c;
    pub const NODE_MORE_UV: u32 = 0x20;
    /// An edge: two vertices, two polygons, a short, the flag.
    pub const EDGE: u32 = 0x14;
    pub const EDGE_FLAG: u32 = 0x12;
}

/// [soft 0x08458c20 SI_meshAllocate]: an empty mesh: angle 60 degrees with its cosine
/// (the C library's `cos` of the double 1.04719755 at 0x0865faf8, stored as a float),
/// flags 0x804, and the approximation defaults 5, 0.1 and 45 (the floats at 0x0865faec).
pub fn mesh_allocate(m: &mut Mem, o: &mut dyn Outside) -> Ptr32<Mesh> {
    let p = m.si_malloc(mesh::SIZE);
    if p != 0 {
        m.set_f32(p + mesh::ANGLE, 60.0);
        for off in [0x14, 0x00, 0x18, 0x04, 0x1c, 0x08] {
            m.set_u32(p + off, 0);
        }
        let c = o.cos(m, 1.04719755);
        m.set_f32(p + mesh::ANGLE_COS, c as f32);
        m.set_f32(p + 0x3c, 5.0);
        m.set_f32(p + 0x40, 0.1);
        m.set_f32(p + 0x44, 45.0);
        m.set_u16(p + mesh::FLAGS, 0x804);
        for off in [0x24, 0x28, 0x2c, 0x30, 0x34] {
            m.set_u32(p + off, 0);
        }
        for (off, v) in [(0x52, 0u16), (0x50, 2), (0x38, 0), (0x48, 1), (0x4a, 5), (0x4c, 0)] {
            m.set_u16(p + off, v);
        }
    }
    Ptr32::new(p)
}

/// [soft 0x08459d48 SI_vertexAllocate]: `n` vertices in a zeroed block of the next
/// multiple of 32 records; each of the `n` is then cleared again field by field. (A
/// negative `n` makes the original clear four thousand million records.)
pub fn vertex_allocate(m: &mut Mem, n: i32) -> u32 {
    let rounded = (n as u32).wrapping_sub(((n as u32).wrapping_sub(1) & 0x1f).wrapping_sub(0x1f));
    let p = m.si_calloc(rounded, mesh::VERTEX);
    let mut v = p;
    let mut left = n;
    while left != 0 {
        m.set_u32(v + 8, 0);
        m.set_u32(v + 4, 0);
        m.set_u32(v, 0);
        m.set_u16(v + 0x12, 0);
        m.set_u16(v + 0x10, 0);
        m.set_u32(v + 0xc, 0);
        v = v.wrapping_add(mesh::VERTEX);
        left = left.wrapping_sub(1);
    }
    p
}

/// [soft 0x08459e20 SI_polygonAllocate]: `n` polygons, like the vertices.
pub fn polygon_allocate(m: &mut Mem, n: i32) -> u32 {
    let rounded = (n as u32).wrapping_sub(((n as u32).wrapping_sub(1) & 0x1f).wrapping_sub(0x1f));
    let p = m.si_calloc(rounded, mesh::POLYGON);
    let mut v = p;
    let mut left = n;
    while left != 0 {
        m.set_u32(v + 8, 0);
        m.set_u32(v + 4, 0);
        m.set_u32(v, 0);
        m.set_u16(v + 0x14, 0);
        m.set_u16(v + 0x10, 0);
        m.set_u32(v + 0xc, 0);
        m.set_u16(v + 0x12, 0);
        v = v.wrapping_add(mesh::POLYGON);
        left = left.wrapping_sub(1);
    }
    p
}

/// [soft 0x08459da8 SI_polygonNodeAllocate]: corners for a polygon of `n`, the next
/// multiple of 4: every one of them (not only `n`) gets vertex −1 and the colour
/// 0xffb2b2b2.
pub fn polygon_node_allocate(m: &mut Mem, n: i32) -> u32 {
    let rounded = (n as u32).wrapping_sub(((n as u32).wrapping_sub(1) & 3).wrapping_sub(3));
    let p = m.si_calloc(rounded, mesh::NODE);
    let mut v = p;
    let mut left = rounded;
    if 0 < rounded as i32 {
        loop {
            for off in [0x14, 0x18, 0x10, 0x0c, 0x08] {
                m.set_u32(v + off, 0);
            }
            m.set_u32(v, 0xffff_ffff);
            m.set_u32(v + 4, 0);
            m.set_u32(v + 0x1c, 0xffb2_b2b2);
            m.set_u32(v + 0x20, 0);
            left -= 1;
            v = v.wrapping_add(mesh::NODE);
            if left == 0 {
                break;
            }
        }
    }
    p
}

/// [soft 0x08441fb8 SI_edgeAllocate]: a table of edge blocks for edges 0 to `n`: one
/// pointer per started 1024 edges, each to a zeroed block of 1024 edges; then edges 0 to
/// `n` get −1 in their two vertices and two polygons.
pub fn edge_allocate(m: &mut Mem, n: i32) -> u32 {
    let blocks = ((n.wrapping_add(1)) >> 10).wrapping_add(1);
    let table = m.si_calloc(blocks as u32, 4);
    let mut i = 0;
    while i < blocks {
        let b = m.si_calloc(0x400, mesh::EDGE);
        m.set_u32(table.wrapping_add(4 * i as u32), b);
        i += 1;
    }
    if (n as u32) < 0x8000_0000 {
        let mut k: i32 = 0;
        loop {
            let e = edge_at(m, table, k);
            for off in [0, 4, 8, 12] {
                m.set_u32(e + off, 0xffff_ffff);
            }
            m.set_u16(e + 0x10, 0);
            m.set_u16(e + 0x12, 0);
            k += 1;
            if !(k <= n) {
                break;
            }
        }
    }
    table
}

/// Edge number `k` of an edge table: record `k & 0x3ff` of block `k >> 10`.
pub fn edge_at(m: &Mem, table: u32, k: i32) -> u32 {
    m.u32(table.wrapping_add(((k >> 10) as u32).wrapping_mul(4))).wrapping_add((k as u32 & 0x3ff) * mesh::EDGE)
}
