// SPDX-License-Identifier: MIT
//! Models and polygon meshes in memory (finding 0015): the hierarchy of model nodes, a
//! node's transformation, the construction and upkeep of a polygon mesh, and mesh primitives.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. One module per original source file:
//!
//! | Module | Original | Content |
//! |---|---|---|
//! | [`mesh`] | (records) | the mesh in memory: vertices with their stars, polygons, corners, edges |
//! | [`mesh_b`] | `mesh_b.c` | the rebuild (`SI_meshUpdateNodes`): stars and edges, polygon normals, border and discontinuity edges, corner normals, display lines |
//! | [`mesh_polygon`] | `meshPolygon.c` | the lists of selected polygons and their vertices, a selection's coordinate system, moving selected polygons |
//! | [`mesh_util`] | `meshUtil.c` | flatness, corner attributes copied between a node's definitions |
//! | [`triangulate`] | `triangulate.c` | a polygon with holes turned into triangles: rounding, bridges to the holes, ear cutting |
//! | [`face`] | `face.c` | the face object (a region bounded by splines or NURBS curves) and its conversion to one polygon with holes |
//! | [`order`] | `order.c` | a mesh's polygons given one consistent direction, non-manifold edges and vertices doubled |
//! | [`mesh_image`] | `mesh_b.c`, `obj_global.c`, `meshPolygon.c`, `meshEditFct.c`, `meshUtil.c`, `triangulate.c`, `face.c` | meshes on `si-mem`'s memory image (finding 0024): the rebuild, the lists of selected polygons, copy and disposal, `SI_meshTriangulate`, a face as a mesh: what a triangulated save (`evalScene -m`) runs |
//! | [`order_image`] | `order.c` | the same file a second time, on `si-mem`'s memory image (finding 0024): not in use, kept beside [`order`] as the measure and the model of a conversion |
//! | [`model_image`] | `model.c` | the rest of `model.c`, and the node update a second time, on `si-mem`'s memory image: a node is the original's record at an address |
//! | [`load`] | (the tail of `meshBinaryRead`) | a file's mesh brought to the program's state after loading |
//! | [`obj_global`] | `obj_global.c` | copying, freeing, counting and moving a mesh (`SI_meshTransform`) |
//! | [`node`] | (records) | the arena of model nodes and how it maps to the original's pointers |
//! | [`hierarchy`] | `model.c`, `model_b.c`, `names.c` | links, traversal orders, definitions, duplication, deletion, skeleton names |
//! | [`node_copy`] | `model.c` | one node duplicated and freed, its definitions, fittings and user data, which node's deformation a geometry depends on |
//! | [`env`] | | the calls into parts of the program that are not ported, as one object |
//! | [`transform`] | `model.c`, `classic.c` | scaling, rotation, translation; local and global matrices down the hierarchy; freezing; a definition through a matrix |
//! | [`setters`] | `model.c` | the transformation setters in global terms, as additions, in the tools' "safe" forms and from a matrix; compensation of sons, initial poses and textures |
//! | [`primitive`] | `primitive.c` | cube, grid, sphere, tetrahedron, octahedron |
//! | [`primitive_image`] | `primitive.c` | spline, NURBS and patch primitives on the memory image |
//!
//! Two binaries check the crate: `model-check` runs the rebuild over every model file under
//! a directory and compares with what the files store; `model-emucheck` compares routine by
//! routine with vectors from the emulation harness (finding 0011).
//!
//! Floating point follows finding 0008: where the original keeps a value in an x87 register
//! the port computes on [`si_x87::F80`] and rounds where the original stores.

pub mod env;
pub mod face;
pub mod hierarchy;
pub mod load;
pub mod mesh;
pub mod mesh_b;
pub mod mesh_image;
pub mod mesh_polygon;
pub mod mesh_util;
pub mod model_image;
pub mod node;
pub mod node_copy;
pub mod obj_global;
pub mod order;
pub mod order_image;
pub mod primitive;
pub mod primitive_image;
pub mod cross_image;
pub mod nurbs_prim_image;
pub mod setters;
pub mod transform;
pub mod triangulate;

pub use mesh::{Edge, Mesh, PolyNode, Polygon, Star, Vertex};
pub use env::Env;
pub use node::{Definition, Models, NodeData, NodeId};
