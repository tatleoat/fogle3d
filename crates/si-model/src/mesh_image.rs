// SPDX-License-Identifier: MIT
//! Polygon meshes on the shared memory image (finding 0024; seventh pass of finding 0015):
//! `mesh_b.c` (the rebuild), the copy, disposal and triangulation of `obj_global.c`, the
//! corner routines of `meshEditFct.c`, the triangulator of `triangulate.c` as the image
//! sees it, and the conversions of the other object kinds to meshes that a triangulated
//! save calls.
//!
//! The functions take the image and what the original takes: a mesh is an `SI_Mesh` at an
//! address ([`si_types::geom::Mesh`]), its arrays are the heap blocks the original's
//! allocators make (`si-hrc`'s image allocators of `mesh_b.c` and `obj_global.c`), and every
//! allocation, reallocation and free is the original's call with its size, in its order.
//! The earlier forms of this crate that keep a mesh in `Vec`s ([`crate::mesh_b`],
//! [`crate::obj_global`], [`crate::triangulate`], [`crate::face`]) stay in use by the rest of
//! the crate and are the arithmetic this module calls where a routine's floating point is
//! the same (said at each place).
//!
//! | Module | Original | Content |
//! |---|---|---|
//! | [`rebuild`] | `mesh_b.c` | stars and edges, polygon normals, border and discontinuity edges, corner normals, display lines; `SI_meshUpdateNodes`, `SI_meshUpdate` |
//! | [`obj`] | `obj_global.c`, `meshEditFct.c`, `meshPolygon.c`, `meshUtil.c` | copy, disposal, the corners' copy and disposal, a list's disposal, the corners' colours |
//! | [`lists`] | `meshPolygon.c` | the lists of selected polygons and their vertices, each node's coordinate system |
//! | [`tri`] | `triangulate.c`, `obj_global.c` | `SI_triangulateContour` on the image, `SI_meshTriangulate` |
//! | [`face`] | `face.c` | a face as one polygon with holes (`SI_faceConvertToMesh`) |
//! | [`patch`] | `patch.c` | a patch as a grid of quadrilaterals (`SI_patchConvertToMesh`; ninth pass) |
//! | [`nurbs`] | `nrb_convert.c` | a NURBS surface as a mesh (`SI_nurbsConvertToMesh`; tenth pass) |
//! | [`mattxt`] | `t2d_fct.c`, `grMaterial.c` | a node's 2D textures of one method by material, for the conversions (ninth pass) |
//! | [`cleanup`] | `cleanup.c`, `meshUtil.c` | `SI_newMeshCleanup`: close vertices merged, degenerate corners and polygons removed, coplanar polygons merged (eighth pass) |
//!
//! **Calls out of the package** go through [`crate::model_image::Outside`], the callee
//! named by its address ([`callee`], and [`face::CALLEES`] for the spline and NURBS
//! routines of `face.c`).

use si_hrc::image::geom::edge_at;
use off as mo;
use si_mem::{Mem, Ptr32};
use si_types::geom::Mesh;

use crate::model_image::Callee;

pub mod cleanup;
pub mod face;
pub mod lists;
pub mod measure;
pub mod mattxt;
pub mod nurbs;
pub mod dump;
pub mod edit;
pub mod edit_fct;
pub mod obj;
pub mod patch;
pub mod rebuild;
pub mod tri;
pub mod holes;
pub mod convex;

/// The routines called through [`crate::model_image::Outside`], each `name (0xADDRESS)`.
pub mod callee {
    use super::Callee;

    /// `SI_showActivity` (0x083d1a60): the busy cursor (nothing while `SI_busyCursor` is 0).
    pub const SI_SHOW_ACTIVITY: Callee = Callee { name: "SI_showActivity", addr: 0x083d1a60 };
    /// `SI_statusDisplayMessage` (0x083cca94): a text on the status line.
    pub const SI_STATUS_DISPLAY_MESSAGE: Callee = Callee { name: "SI_statusDisplayMessage", addr: 0x083cca94 };
    /// `SI_systemWarning` (0x081422d8): a question box; answers the button.
    pub const SI_SETTEXVIEW: Callee = Callee { name: "SI_setTexview", addr: 0x0838c390 };
    pub const SI_SYSTEM_WARNING: Callee = Callee { name: "SI_systemWarning", addr: 0x081422d8 };
    /// `printf` (0x080c48b8) through the binary's stub.
    pub const PRINTF: Callee = Callee { name: "printf", addr: 0x080c_48b8 };
    /// Every routine of the table.
    pub const ALL: &[Callee] = &[SI_SHOW_ACTIVITY, SI_STATUS_DISPLAY_MESSAGE, SI_SETTEXVIEW, SI_SYSTEM_WARNING, PRINTF, super::lists::SI_STOP_ACTIVITY, super::mattxt::SI_MODEL_MASTER_FROM_INSTANCE, super::dump::FPRINTF, super::dump::FFLUSH];
}

/// The globals this module reads.
pub mod g {
    si_mem::globals! {
        /// The node whose mesh is being rebuilt (`mesh_b.c`), for the collision messages.
        CURR_MDL: u32 = 0x086d59c0, "currMdl", init 0;
        /// The table the vertex and edge removal tools install (`SI_gblIndex3`); null at any
        /// other time.
        SI_GBL_INDEX3: u32 = 0x08788974, "SI_gblIndex3";
        /// `mD0` (`triangulate.c`): the last cross product of `SI_segmentIntersect`.
        MD0: f32 = 0x08822450, "mD0";
        /// The most polygons a conversion makes (`SI_patchConvertToMesh` gives up above it).
        SI_NB_POL_MAX: i32 = 0x086d3920, "SI_nbPolMax", init 60000;
        /// `SI_findMatInfo`'s one-entry array of materials for a node without any: it holds
        /// `SI_defaultMaterial` (0x0869dde0).
        PTRMAT: u32 = 0x086fd2e0, "ptrmat.231", init 0x0869dde0;
    }
}

/// Field offsets of the mesh records (the layout of `si-types`' `Mesh`, `Vertex`,
/// `Polygon`, `PolyNode` and `Edge`; the same as `si-hrc`'s readers use).
pub mod off {
    pub const VERTICES: u32 = 0x00;
    pub const POLYGONS: u32 = 0x04;
    pub const EDGES: u32 = 0x08;
    pub const ANGLE: u32 = 0x0c;
    pub const ANGLE_COS: u32 = 0x10;
    pub const NB_VERTICES: u32 = 0x14;
    pub const NB_POLYGONS: u32 = 0x18;
    pub const NB_EDGES: u32 = 0x1c;
    pub const FLAGS: u32 = 0x20;
    pub const VERTEX: u32 = 0x14;
    pub const VERTEX_STAR: u32 = 0x0c;
    pub const VERTEX_NB_STAR: u32 = 0x10;
    pub const VERTEX_FLAG: u32 = 0x12;
    pub const POLYGON: u32 = 0x18;
    pub const POLYGON_NODES: u32 = 0x0c;
    pub const POLYGON_NB_NODES: u32 = 0x10;
    pub const POLYGON_MATERIAL: u32 = 0x12;
    pub const POLYGON_FLAG: u32 = 0x14;
    pub const NODE: u32 = 0x24;
    pub const NODE_NORMAL: u32 = 0x08;
    pub const NODE_UV: u32 = 0x14;
    pub const NODE_COLOUR: u32 = 0x1c;
    pub const EDGE: u32 = 0x14;
    pub const EDGE_FLAG: u32 = 0x12;
}

/// The mesh at an address as a typed pointer.
pub fn mesh_ptr(a: u32) -> Ptr32<Mesh> {
    Ptr32::new(a)
}

/// The address of vertex `v` (20-byte records).
pub(crate) fn vertex_at(m: &Mem, mesh: u32, v: i32) -> u32 {
    m.u32(mesh + mo::VERTICES).wrapping_add((v as u32).wrapping_mul(mo::VERTEX))
}

/// The address of polygon `p` (24-byte records).
pub(crate) fn polygon_at(m: &Mem, mesh: u32, p: i32) -> u32 {
    m.u32(mesh + mo::POLYGONS).wrapping_add((p as u32).wrapping_mul(mo::POLYGON))
}

/// The address of edge `e` (counted from 1): record `e & 0x3ff` of block `e >> 10`.
pub(crate) fn edge_record(m: &Mem, mesh: u32, e: i32) -> u32 {
    edge_at(m, m.u32(mesh + mo::EDGES), e)
}

/// Three floats at an address.
pub(crate) fn f3(m: &Mem, a: u32) -> [f32; 3] {
    [m.f32(a), m.f32(a + 4), m.f32(a + 8)]
}

pub(crate) fn put_f3(m: &mut Mem, a: u32, v: [f32; 3]) {
    m.set_f32(a, v[0]);
    m.set_f32(a + 4, v[1]);
    m.set_f32(a + 8, v[2]);
}

/// [soft 0x08458c20 SI_meshAllocate] as `order.c`'s image form makes it (the C library's
/// cosine asked through `si_x87::trig`, replayed from a vector's record when there is
/// one).
pub fn mesh_allocate(m: &mut Mem) -> u32 {
    crate::order_image::mesh_allocate(m).addr()
}

/// Copies the image of a mesh into `si-model`'s value form (positions, stars, polygons with
/// their corners, edges, flags and angle): what the tests and the checks of this module use
/// to hold the image forms against the value forms. Nothing here reproduces a routine.
pub fn to_value(m: &Mem, a: u32) -> crate::mesh::Mesh {
    use crate::mesh::{Edge, Mesh as V, PolyNode, Polygon, Star, Vertex};
    let mut out = V { angle: m.f32(a + mo::ANGLE), cos_angle: m.f32(a + mo::ANGLE_COS), flags: m.u16(a + mo::FLAGS), ..V::default() };
    let nv = if m.u32(a + mo::VERTICES) == 0 { 0 } else { m.i32(a + mo::NB_VERTICES).max(0) };
    for v in 0..nv {
        let at = vertex_at(m, a, v);
        let (star, n) = (m.u32(at + mo::VERTEX_STAR), m.u16(at + mo::VERTEX_NB_STAR) as u32);
        let n = if star == 0 { 0 } else { n };
        out.vertices.push(Vertex {
            pos: f3(m, at),
            star: (0..n).map(|e| Star { vertex: m.i32(star + 12 * e), edge: m.i32(star + 12 * e + 4), polygon: m.i32(star + 12 * e + 8) }).collect(),
            flag: m.i16(at + mo::VERTEX_FLAG),
        });
    }
    let np = if m.u32(a + mo::POLYGONS) == 0 { 0 } else { m.i32(a + mo::NB_POLYGONS).max(0) };
    for p in 0..np {
        let at = polygon_at(m, a, p);
        let (nodes, n) = (m.u32(at + mo::POLYGON_NODES), m.u16(at + mo::POLYGON_NB_NODES) as u32);
        let n = if nodes == 0 { 0 } else { n };
        out.polygons.push(Polygon {
            normal: f3(m, at),
            nodes: (0..n)
                .map(|c| {
                    let node = nodes + mo::NODE * c;
                    PolyNode { vertex: m.i32(node), edge: m.i32(node + 4), normal: f3(m, node + mo::NODE_NORMAL), uv: [m.f32(node + mo::NODE_UV), m.f32(node + mo::NODE_UV + 4)], color: m.u32(node + mo::NODE_COLOUR), extra_uv: Vec::new() }
                })
                .collect(),
            material: m.i16(at + mo::POLYGON_MATERIAL),
            flag: m.i16(at + mo::POLYGON_FLAG),
        });
    }
    if m.u32(a + mo::EDGES) != 0 {
        out.edges.push(Edge::default());
        for e in 1..=m.i32(a + mo::NB_EDGES).max(0) {
            let r = edge_record(m, a, e);
            out.edges.push(Edge { v0: m.i32(r), v1: m.i32(r + 4), p0: m.i32(r + 8), p1: m.i32(r + 12), x10: m.i16(r + 0x10), flag: m.i16(r + mo::EDGE_FLAG) });
        }
    }
    let (nd, d) = (m.i32(a + 0x24), m.u32(a + 0x28));
    out.nb_display = nd;
    if d != 0 {
        out.display = (0..=nd.max(-1)).map(|k| m.i32(d + 4 * k as u32)).collect();
    }
    out
}

/// Lays a value-form mesh out on the image as the readers leave one before its rebuild:
/// `SI_meshAllocate`, the vertices and polygons by the original's allocators, the corners'
/// vertex, normal, texture pair and colour, the polygons' normal, material and flag, the
/// angle, cosine and flags; no stars, edges or display lines (the rebuild makes those).
/// What the tests and the checks of this module start from; nothing here reproduces a
/// routine.
pub fn from_value(m: &mut Mem, v: &crate::mesh::Mesh) -> u32 {
    let a = mesh_allocate(m);
    m.set_f32(a + mo::ANGLE, v.angle);
    m.set_f32(a + mo::ANGLE_COS, v.cos_angle);
    m.set_u16(a + mo::FLAGS, v.flags);
    m.set_i32(a + mo::NB_VERTICES, v.vertices.len() as i32);
    let verts = si_hrc::image::geom::vertex_allocate(m, v.vertices.len() as i32);
    m.set_u32(a + mo::VERTICES, verts);
    for (k, x) in v.vertices.iter().enumerate() {
        let at = verts + mo::VERTEX * k as u32;
        put_f3(m, at, x.pos);
        m.set_i16(at + mo::VERTEX_FLAG, x.flag);
    }
    m.set_i32(a + mo::NB_POLYGONS, v.polygons.len() as i32);
    let polys = si_hrc::image::geom::polygon_allocate(m, v.polygons.len() as i32);
    m.set_u32(a + mo::POLYGONS, polys);
    for (k, p) in v.polygons.iter().enumerate() {
        let at = polys + mo::POLYGON * k as u32;
        put_f3(m, at, p.normal);
        m.set_i16(at + mo::POLYGON_MATERIAL, p.material);
        m.set_i16(at + mo::POLYGON_FLAG, p.flag);
        m.set_u16(at + mo::POLYGON_NB_NODES, p.nodes.len() as u16);
        let nodes = si_hrc::image::geom::polygon_node_allocate(m, p.nodes.len() as i32);
        m.set_u32(at + mo::POLYGON_NODES, nodes);
        for (c, n) in p.nodes.iter().enumerate() {
            let node = nodes + mo::NODE * c as u32;
            m.set_i32(node, n.vertex);
            put_f3(m, node + mo::NODE_NORMAL, n.normal);
            m.set_f32(node + mo::NODE_UV, n.uv[0]);
            m.set_f32(node + mo::NODE_UV + 4, n.uv[1]);
            m.set_u32(node + mo::NODE_COLOUR, n.color);
        }
    }
    a
}

#[cfg(test)]
mod tests;
