// SPDX-License-Identifier: MIT
//! A polygon mesh as the program holds it in memory (`SI_Mesh`, finding 0015).
//!
//! The original's mesh is three arrays hanging off a 0x54-byte record: vertices (20 bytes
//! each), polygons (24 bytes, each owning an array of 36-byte corners) and edges (20 bytes,
//! numbered from 1, in blocks of 1024 behind a table of block pointers). A file stores only
//! positions, corners and a list of edge flags; everything that connects them is rebuilt
//! after reading by `SI_meshUpdateNodes` ([`crate::mesh_b`]):
//!
//! - each **vertex** owns its *star*: an array of 12-byte entries (neighbour vertex, signed
//!   edge number, polygon), ordered around the vertex;
//! - each **corner** carries the signed number of the edge that leaves it towards the next
//!   corner of its contour;
//! - each **edge** carries its two vertices and the polygon on each side.
//!
//! How this maps to the original's pointers: every array the original reaches through a
//! pointer is a `Vec` here and every index is the original's index. Edge `n` of the original
//! (entry `n & 0x3ff` of block `n >> 10`) is `edges[n]`; entry 0 exists and is never a real
//! edge, as in the original, whose first block is allocated whole and zeroed. A count field
//! of the record (`nb_vertices`, `nb_polygons`, `nb_edges`, a vertex's star count, a
//! polygon's corner count) is the length of the `Vec`, except `nb_edges`, which is
//! `edges.len() - 1`.
//!
//! Offsets in the field comments are those of the original records.

/// One entry of a vertex's star (12 bytes).
///
/// Entry `k` of vertex `v` says: `vertex` is a neighbour of `v`; `edge` is the edge between
/// them, positive when `v` is the edge's first vertex and negative when it is the second;
/// `polygon` is the polygon that has the corner sequence (next entry's vertex, `v`,
/// `vertex`), or −1 where the fan around `v` is open after this entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Star {
    pub vertex: i32,  // +0
    pub edge: i32,    // +4
    pub polygon: i32, // +8
}

/// A vertex (20 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 3], // +0
    /// +0xc (pointer) and +0x10 (16-bit count): the star.
    pub star: Vec<Star>,
    /// +0x12. Bit 15 marks the vertex for a partial normal update
    /// (`SI_meshComputesVertexNormal` with −2).
    pub flag: i16,
}

/// A corner of a polygon (`SI_Node`, 36 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct PolyNode {
    /// +0: vertex index; −1 separates two contours of one polygon (an outline and its holes).
    pub vertex: i32,
    /// +4: the edge from this corner to the next of its contour, signed like a star entry's;
    /// 0 until the mesh is rebuilt.
    pub edge: i32,
    pub normal: [f32; 3], // +8
    pub uv: [f32; 2],     // +0x14
    /// +0x1c: four bytes, here as the 32-bit value a file stores big-endian.
    pub color: u32,
    /// +0x20: the pairs of the model's further texture coordinate sets.
    pub extra_uv: Vec<[f32; 2]>,
}

impl Default for PolyNode {
    /// What `SI_polygonNodeAllocate` (0x08459da8) puts in a corner.
    fn default() -> Self {
        PolyNode { vertex: -1, edge: 0, normal: [0.0; 3], uv: [0.0; 2], color: 0xffb2_b2b2, extra_uv: Vec::new() }
    }
}

/// A polygon (24 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polygon {
    /// +0: the polygon's unit normal, or zero for a degenerate polygon.
    pub normal: [f32; 3],
    /// +0xc (pointer) and +0x10 (16-bit count).
    pub nodes: Vec<PolyNode>,
    pub material: i16, // +0x12
    /// +0x14. Bit 0x400: the corners keep their normals (`SI_meshComputesVertexNormal` skips
    /// the polygon); bit 0x800: hidden (`SI_meshOptimizeDisplay`).
    pub flag: i16,
}

/// Bits of [`Edge::flag`].
pub const EDGE_USER_HARD: i16 = 0x08;
pub const EDGE_AUTO_HARD: i16 = 0x10;
pub const EDGE_BORDER: i16 = 0x20;
/// Set on every edge by `SI_meshOptimizeDisplay` (drawn, or hidden with its polygons).
pub const EDGE_DISPLAYED: u16 = 0x8000;

/// An edge (20 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub v0: i32, // +0
    pub v1: i32, // +4
    /// +8: the polygon that runs along the edge from `v0` to `v1`, or −1.
    pub p0: i32,
    /// +0xc: the polygon that runs from `v1` to `v0`, or −1.
    pub p1: i32,
    pub x10: i16, // +0x10, 0 in every edge the rebuild makes
    /// +0x12: 0x20 border (fewer than two polygons), 0x10 discontinuity found from the
    /// angle, 0x08 kept as it is by the automatic pass, 0x8000 display mark.
    pub flag: i16,
}

impl Default for Edge {
    /// A record of a zeroed edge block.
    fn default() -> Self {
        Edge { v0: 0, v1: 0, p0: 0, p1: 0, x10: 0, flag: 0 }
    }
}

/// What the first word of a list node points at: a polygon or a vertex of the mesh the list
/// belongs to (the original keeps the record's address; here it is the record's index).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListItem {
    #[default]
    Null,
    Polygon(i32),
    Vertex(i32),
}

/// A node of a mesh list (`__SI_MeshListNode`, 0x50 bytes, from `SI_calloc`).
#[derive(Clone, Debug, PartialEq)]
pub struct ListNode {
    pub item: ListItem,          // +0
    pub sub: MeshList,           // +4: a list of its own (a vertex's selected polygons)
    pub sub2: MeshList,          // +8: a second one; nothing in `meshPolygon.c` fills it
    pub matrix: [[f32; 4]; 4],   // +0xc: zero until a coordinate system is stored
}

/// A list of [`ListNode`]s (the original's pointer to the first node; the nodes are linked
/// through +0x4c). A new node goes to the **front** (`SI_meshAddList`), so the list reads
/// in the reverse of the order of insertion. Here the nodes are kept in a `Vec` in order of
/// insertion, the head last; [`MeshList::iter`] walks them head first, as the original does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshList {
    /// In order of insertion: the list's head is the last element.
    pub nodes: Vec<ListNode>,
}

impl MeshList {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The nodes from the head on.
    pub fn iter(&self) -> impl Iterator<Item = &ListNode> {
        self.nodes.iter().rev()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut ListNode> {
        self.nodes.iter_mut().rev()
    }

    /// The node `k` steps from the head.
    pub fn at(&self, k: usize) -> Option<&ListNode> {
        self.nodes.len().checked_sub(k + 1).map(|i| &self.nodes[i])
    }

    pub fn at_mut(&mut self, k: usize) -> Option<&mut ListNode> {
        self.nodes.len().checked_sub(k + 1).map(|i| &mut self.nodes[i])
    }
}

/// Bits of [`Polygon::flag`] (established by `meshPolygon.c`, finding 0015).
pub const POLY_SELECTED: i16 = 0x0001;
/// Scratch marks of the walk over connected polygons: visited, and "in a neighbour list".
pub const POLY_VISITED: i16 = 0x0004;
pub const POLY_QUEUED: i16 = 0x0008;
/// The polygon's normal is out of date (`SI_meshUpdateTagged` recomputes and clears);
/// `SI_meshComputesVertexNormal` leaves the corner normals of such a polygon alone.
pub const POLY_DIRTY: i16 = 0x0400;
pub const POLY_HIDDEN: i16 = 0x0800;

/// Bits of [`Mesh::flags`].
pub const MESH_FACETED: u16 = 0x0002;
pub const MESH_AUTO_4: u16 = 0x0004;
pub const MESH_AUTO_8: u16 = 0x0008;
pub const MESH_NEW: u16 = 0x0800;

/// A polygon mesh (`SI_Mesh`, 0x54 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,   // +0, count at +0x14
    pub polygons: Vec<Polygon>,  // +4, count at +0x18
    /// +8, count at +0x1c. Empty while the original's table pointer is null; otherwise
    /// `edges[0]` is the unused record 0 and the edges are `edges[1..]`.
    pub edges: Vec<Edge>,
    /// +0xc: discontinuity angle in degrees.
    pub angle: f32,
    /// +0x10: its cosine.
    pub cos_angle: f32,
    /// +0x20: 0x2 faceted (corner normals are polygon normals); 0x4 and 0x8 automatic
    /// discontinuity; 0x800 set at creation and turned into 0x8 by the first normal pass.
    pub flags: u16,
    /// +0x28, count at +0x24: the wire-frame polylines of `SI_meshOptimizeDisplay`, vertex
    /// indices with −1 after each line. `nb_display` is the original's count, one less than
    /// the number of integers written.
    pub display: Vec<i32>,
    pub nb_display: i32,
    pub x2c: i32,        // +0x2c
    /// +0x30: the selected polygons (flag bit 0x1), each with the coordinate system of its
    /// group of connected selected polygons (`SI_meshUpdatePolygonLists`).
    pub selected_polygons: MeshList,
    /// +0x34: the vertices of the selected polygons, each with the list of the selected
    /// polygons that have a corner there.
    pub selected_vertices: MeshList,
    pub mr_view: i16,    // +0x38
    pub mr: [f32; 3],    // +0x3c: 5, 0.1, 45
    pub mr_min: i16,     // +0x48
    pub mr_max: i16,     // +0x4a
    pub mr_rec: i16,     // +0x4c
    pub mr_approx: i16,  // +0x50
    pub mr_first: i16,   // +0x52
}

impl Default for Mesh {
    /// A new, empty mesh: what `SI_meshAllocate` (0x08458c20) sets up. That allocator is
    /// ported in `si-types` (`si_types::geom::Mesh::allocate`); this is its content in owned form.
    fn default() -> Self {
        let r = si_types::geom::Mesh::allocate();
        Mesh {
            vertices: Vec::new(), polygons: Vec::new(), edges: Vec::new(), angle: r.angle, cos_angle: r.cos_angle,
            flags: r.flags, display: Vec::new(), nb_display: 0, x2c: 0, selected_polygons: MeshList::default(),
            selected_vertices: MeshList::default(), mr_view: r.mr_view,
            mr: [r.mr_edge, r.mr_ucurv, r.mr_vcurv], mr_min: r.mr_min, mr_max: r.mr_max, mr_rec: r.mr_rec,
            mr_approx: r.mr_approx, mr_first: r.mr_first,
        }
    }
}

impl Mesh {
    /// The original's `nb_edges` (+0x1c).
    pub fn nb_edges(&self) -> i32 {
        self.edges.len().saturating_sub(1) as i32
    }

    pub fn nb_vertices(&self) -> i32 {
        self.vertices.len() as i32
    }

    pub fn nb_polygons(&self) -> i32 {
        self.polygons.len() as i32
    }

    /// A mesh with these positions and no polygons.
    pub fn with_vertices(positions: &[[f32; 3]]) -> Mesh {
        let mut m = Mesh::default();
        m.vertices = positions.iter().map(|&pos| Vertex { pos, star: Vec::new(), flag: 0 }).collect();
        m
    }

    /// Appends a polygon with the given corners (−1 separates contours) and returns its index.
    pub fn push_polygon(&mut self, corners: &[i32]) -> usize {
        let nodes = corners.iter().map(|&vertex| PolyNode { vertex, ..PolyNode::default() }).collect();
        self.polygons.push(Polygon { normal: [0.0; 3], nodes, material: 0, flag: 0 });
        self.polygons.len() - 1
    }
}
