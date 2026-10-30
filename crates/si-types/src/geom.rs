// SPDX-License-Identifier: MIT
//! Layouts of the geometry a model node can hold, and their allocators.
//!
//! Evidence: "token" is a field of the structure's record in a model file, named as in the
//! vendor's hierarchy file format pages and found by the order in which the binary writer
//! (`meshBinaryWrite`, `splineBinaryWrite`, `faceBinaryWrite`, `patchBinaryWrite`,
//! `nurbsCrvBinaryWrite`, `nurbsSrfBinaryWrite`) writes the offsets; "attribute N" is
//! parameter id N of `Relations.rsrc`, read by the `SI_…AttrGetValue` routines.

use crate::{layout, CStr32, Double, Matrix, Opaque, Ptr32, Zero, IDENTITY};

/// Records pointed to but not laid out here.
pub enum MeshListNode {}
pub enum NurbsCompCrv {}

layout! {
    /// A point or direction (`SI_Vector`): **three** floats. The SDK's `SAA_Vector` has a
    /// fourth (`w`); the internal one does not.
    pub struct Vector = "SI_Vector", size 0xc,
        "SI_Vector_fread and SI_Vector_fwrite step through arrays by 0xc and swap three words per item; SI_vectorMultiplyMatrix indexes by 0xc" {
        0x00 x: f32, Read, "First float.";
        0x04 y: f32, Read, "Second float.";
        0x08 z: f32, Read, "Third float.";
    }
}

layout! {
    /// A polygon mesh (`SI_Mesh`).
    pub struct Mesh = "SI_Mesh", size 0x54,
        "SI_meshAllocate allocates 0x54 bytes" {
        0x00 vertices: Ptr32<Vertex>, Read, "Array of nb_vertices vertices (SI_vertexAllocate).";
        0x04 polygons: Ptr32<Polygon>, Read, "Array of nb_polygons polygons (SI_polygonAllocate).";
        0x08 edges: Ptr32<Ptr32<Edge>>, Read, "Table of blocks of 1024 edges; edge n (counted from 1) is entry n & 0x3ff of block n >> 10.";
        0x0c angle: f32, Read, "Attribute 319 (PolyDisc, the discontinuity angle); token angle; 60.0 at creation.";
        0x10 cos_angle: f32, Named, "Set at creation to the cosine of 1.04719755 radians (60 degrees), as a float.";
        0x14 nb_vertices: i32, Read, "Attribute 314 (PolyNbVert).";
        0x18 nb_polygons: i32, Read, "Attribute 312 (PolyNbPoly).";
        0x1c nb_edges: i32, Read, "Attribute 313 (PolyNbEdg).";
        0x20 flags: u16, Read, "Token flag; bit 1 is attribute 318 (PolyIsAutom), bit 2 attribute 317 (PolyProNor); 0x804 at creation.";
        0x22 pad_22: [u8; 2], Unknown, "No access found.";
        0x24 nb_display: i32, Named, "Length of display (SI_meshOptimizeDisplay).";
        0x28 display: Ptr32<i32>, Named, "Integers built by SI_meshOptimizeDisplay and used by the drawing routines.";
        0x2c unk_2c: i32, Unknown, "0 at creation.";
        0x30 polygon_list0: Ptr32<MeshListNode>, Named, "A polygon list (SI_meshCopyPolygonLists, SI_meshUpdatePolygonLists).";
        0x34 polygon_list1: Ptr32<MeshListNode>, Named, "A second polygon list.";
        0x38 mr_view: i16, Named, "Third of nine values written after the edges; by position the patch's view-dependent flag. 0 at creation.";
        0x3a pad_3a: [u8; 2], Unknown, "No access found.";
        0x3c mr_edge: f32, Named, "5.0 at creation: with the next two the same triple (5, 0.1, 45) as the patch's and the surface's approximation settings.";
        0x40 mr_ucurv: f32, Named, "0.1 at creation.";
        0x44 mr_vcurv: f32, Named, "45.0 at creation.";
        0x48 mr_min: i16, Named, "1 at creation.";
        0x4a mr_max: i16, Named, "5 at creation.";
        0x4c mr_rec: i16, Named, "0 at creation.";
        0x4e pad_4e: [u8; 2], Unknown, "No access found.";
        0x50 mr_approx: i16, Named, "2 at creation; second of the nine values written after the edges.";
        0x52 mr_first: i16, Unknown, "0 at creation; first of the nine values written after the edges.";
    }
}

layout! {
    /// A vertex of a mesh.
    pub struct Vertex = "(vertex of SI_Mesh)", size 0x14,
        "SI_vertexAllocate allocates records of 0x14 bytes" {
        0x00 pos: Vector, Read, "Token position.";
        0x0c vectors: Ptr32<Vector>, Unknown, "An array of nb_vectors vectors, copied with the vertex by SI_meshCopy; what they are is not read.";
        0x10 nb_vectors: u16, Unknown, "Length of vectors.";
        0x12 flag: i16, Read, "Token flag, written after the position.";
    }
}

layout! {
    /// A polygon of a mesh.
    pub struct Polygon = "(polygon of SI_Mesh)", size 0x18,
        "SI_polygonAllocate allocates records of 0x18 bytes" {
        0x00 vec_00: Vector, Unknown, "Three words copied with the polygon; not written to files.";
        0x0c nodes: Ptr32<PolyNode>, Read, "Array of nb_nodes corners (SI_polygonNodeAllocate).";
        0x10 nb_nodes: u16, Read, "Number of corners; written before them.";
        0x12 material: i16, Read, "Index into the node's materials; the reader resets it to 0 when out of range.";
        0x14 flag: i16, Read, "Token flag, written after the material in newer files.";
        0x16 pad_16: [u8; 2], Unknown, "No access found.";
    }
}

layout! {
    /// A corner of a polygon (`SI_Node`).
    pub struct PolyNode = "SI_Node", size 0x24,
        "SI_polygonNodeAllocate allocates records of 0x24 bytes" {
        0x00 vertex: i32, Read, "Index of the vertex; -1 at creation.";
        0x04 unk_04: i32, Unknown, "0 at creation; not written to files.";
        0x08 normal: Vector, Read, "Token normal.";
        0x14 u: f32, Read, "Token uv, first value.";
        0x18 v: f32, Read, "Token uv, second value.";
        0x1c color: [u8; 4], Read, "Token colour: four bytes, written as one word. 0xffb2b2b2 at creation.";
        0x20 extra_uv: Ptr32<[f32; 2]>, Read, "nb_uv - 1 further pairs, nb_uv being the model's.";
    }
}

layout! {
    /// An edge of a mesh.
    pub struct Edge = "(edge of SI_Mesh)", size 0x14,
        "SI_edgeAllocate allocates blocks of 0x400 records of 0x14 bytes" {
        0x00 v0: i32, Read, "First vertex; -1 at creation.";
        0x04 v1: i32, Read, "Second vertex; -1 at creation.";
        0x08 unk_08: i32, Unknown, "-1 at creation; copied with the edge, not written to files.";
        0x0c unk_0c: i32, Unknown, "-1 at creation; copied with the edge, not written to files.";
        0x10 unk_10: i16, Unknown, "0 at creation.";
        0x12 flag: i16, Read, "Token flag.";
    }
}

layout! {
    /// A spline (`SI_Spline`).
    pub struct Spline = "SI_Spline", size 0x74,
        "SI_splineAllocate allocates 0x74 bytes; a face embeds one in 0x74 bytes and keeps its holes 0x74 apart" {
        0x00 name: CStr32, Read, "Token name; SI_splineAllocate sets spl<n>.";
        0x04 unk_04: i16, Unknown, "0x10 at creation; SI_splineTouch writes it.";
        0x06 phantom: i16, Read, "Attribute 309 (SplPhantom); token phantom.";
        0x08 closed: i16, Read, "Attribute 306 (SplClose); token close.";
        0x0a pad_0a: [u8; 2], Unknown, "No access found.";
        0x0c kind: i32, Read, "Attribute 305 (SplTyp); token type, written as 16 bits.";
        0x10 nb_keys: i16, Read, "Attribute 304 (SplNbPoint).";
        0x12 pad_12: [u8; 2], Unknown, "No access found.";
        0x14 keys: Ptr32<SplineKey>, Read, "Array of nb_keys keys.";
        0x18 unk_18: i32, Unknown, "0 at creation.";
        0x1c tension: f32, Read, "Attribute 307 (SplTension).";
        0x20 step: i16, Read, "Attribute 308 (SplStep); 10 at creation.";
        0x22 pad_22: [u8; 2], Unknown, "No access found.";
        0x24 matrix: Matrix, Unknown, "Identity at creation.";
        0x64 unk_64: i16, Unknown, "2 at creation; read by SI_splineAddPoint.";
        0x66 unk_66: i16, Unknown, "1 at creation; read by SI_splineAddPoint.";
        0x68 nb_lengths: u16, Named, "SI_splineUpdateLength.";
        0x6a pad_6a: [u8; 2], Unknown, "No access found.";
        0x6c lengths: Ptr32<Opaque>, Named, "Table built by SI_splineUpdateLength; freed with the spline.";
        0x70 unk_70: i16, Unknown, "0 at creation; SI_checkSplineToFcurve writes it.";
        0x72 pad_72: [u8; 2], Unknown, "No access found.";
    }
}

layout! {
    /// A key (control point with its two neighbours) of a spline.
    pub struct SplineKey = "(key of SI_Spline)", size 0x30,
        "splineBinaryWrite steps through the keys by 0x30" {
        0x00 prev: Vector, Named, "Token previous: first vector written.";
        0x0c pos: Vector, Named, "Token position.";
        0x18 next: Vector, Named, "Token next.";
        0x24 unk_24: [u8; 4], Unknown, "Not written to files.";
        0x28 constraint: i16, Named, "Token constraint.";
        0x2a segment: i16, Named, "Token segment.";
        0x2c flag: i16, Named, "Token flag.";
        0x2e pad_2e: [u8; 2], Unknown, "No access found.";
    }
}

layout! {
    /// A face (`SI_Face`): an outline and holes, as splines or as NURBS curves.
    pub struct Face = "SI_Face", size 0x88,
        "SI_faceAllocate allocates 0x88 bytes" {
        0x00 name: CStr32, Named, "Read as a string by SI_faceDump; freed with the face; null at creation.";
        0x04 spline: Spline, Read, "The outline when is_spline is set: initialised with SI_splineInit, written with splineBinaryWrite.";
        0x78 nb_holes: i16, Read, "Attribute 310 (FaceNbHole): number of holes; number of curves when the face is made of NURBS curves.";
        0x7a pad_7a: [u8; 2], Unknown, "No access found.";
        0x7c holes: Ptr32<Spline>, Read, "Array of nb_holes splines.";
        0x80 curves: Ptr32<NurbsCrv>, Read, "Array of nb_holes NURBS curves when is_spline is 0.";
        0x84 is_spline: i16, Read, "Attribute 311 (FaceIsSpline); 1 at creation.";
        0x86 pad_86: [u8; 2], Unknown, "No access found.";
    }
}

layout! {
    /// A patch (`SI_Patch`): a grid of points with a spline type in each direction.
    pub struct Patch = "SI_Patch", size 0x204,
        "SI_patchAllocate allocates 0x204 bytes" {
        0x000 unk_000: i32, Unknown, "0 at creation.";
        0x004 u_type: i32, Read, "Attribute 290 (PtchSplnTyp, U).";
        0x008 v_type: i32, Read, "Attribute 291.";
        0x00c nb_u: u16, Read, "Attribute 288 (PtchNbPoint, U).";
        0x00e nb_v: u16, Read, "Attribute 289.";
        0x010 u_closed: i16, Read, "Attribute 292 (PtchClose, U).";
        0x012 v_closed: i16, Read, "Attribute 293.";
        0x014 u_tension: f32, Read, "Attribute 294 (PtchTension, U).";
        0x018 v_tension: f32, Read, "Attribute 295.";
        0x01c u_step: u16, Read, "Attribute 296 (PtchStep, U); 3 at creation.";
        0x01e v_step: u16, Read, "Attribute 297; 3 at creation.";
        0x020 u_nb_curves: u16, Read, "Attribute 298 (PtchNbCrv, U); 1 at creation.";
        0x022 v_nb_curves: u16, Read, "Attribute 299; 1 at creation.";
        0x024 basis: [Matrix; 2], Named, "Two matrices filled by SI_patchUpdateMatrix and read by SI_patchValue. Not set at creation.";
        0x0a4 matrices: [Matrix; 4], Unknown, "Four matrices, identity at creation.";
        0x1a4 points: Ptr32<[f32; 4]>, Read, "nb_u × nb_v points of 16 bytes each; the writer stores the first three floats of each.";
        0x1a8 tags: Ptr32<i16>, Read, "One 16-bit value per point; the last token.";
        0x1ac unk_1ac: i16, Unknown, "0 at creation.";
        0x1ae pad_1ae: [u8; 2], Unknown, "No access found.";
        0x1b0 surface0: Ptr32<f32>, Named, "Built by SI_patchUpdateSurface; freed with the patch.";
        0x1b4 surface0_dims: [u16; 2], Named, "Set with surface0.";
        0x1b8 surface1: Ptr32<f32>, Named, "Built by SI_patchUpdateSurface; freed with the patch.";
        0x1bc surface1_dims: [u16; 2], Named, "Set with surface1.";
        0x1c0 unk_1c0: i16, Unknown, "0 at creation; SI_patchComputesSubdivision writes it.";
        0x1c2 pad_1c2: [u8; 2], Unknown, "No access found.";
        0x1c4 subdiv0: Ptr32<Opaque>, Named, "SI_patchComputesSubdivision; freed with the patch.";
        0x1c8 subdiv1: Ptr32<Opaque>, Named, "SI_patchComputesSubdivision; freed with the patch.";
        0x1cc u_bottom: i16, Read, "Attribute 300 (PtchBottom, U).";
        0x1ce u_top: i16, Read, "Attribute 302 (PtchTop, U).";
        0x1d0 v_bottom: i16, Read, "Attribute 301 (PtchBottom, V).";
        0x1d2 v_top: i16, Read, "Attribute 303 (PtchTop, V).";
        0x1d4 unk_1d4: i16, Unknown, "0 at creation; the last of nine 16-bit tokens written after the tensions.";
        0x1d6 unk_1d6: i16, Unknown, "0 at creation.";
        0x1d8 modif0: Ptr32<i16>, Named, "SI_patchUpdateSurface; freed with the patch.";
        0x1dc modif1: Ptr32<i16>, Named, "SI_patchUpdateSurface; freed with the patch.";
        0x1e0 unk_1e0: i16, Unknown, "Written by SI_dragPoint and the tag routines. Not set at creation.";
        0x1e2 pad_1e2: [u8; 2], Unknown, "No access found.";
        0x1e4 unk_1e4: i32, Unknown, "0 at creation; patchUpdateDef writes it.";
        0x1e8 mr_view: i16, Named, "Second of the eight approximation tokens of newer files; 0 at creation.";
        0x1ea pad_1ea: [u8; 2], Unknown, "No access found.";
        0x1ec mr_edge: f32, Named, "Third approximation token; 5.0 at creation.";
        0x1f0 mr_ucurv: f32, Named, "Fourth; 0.1 at creation.";
        0x1f4 mr_vcurv: f32, Named, "Fifth; 45.0 at creation.";
        0x1f8 mr_min: i16, Named, "Sixth; 1 at creation.";
        0x1fa mr_max: i16, Named, "Seventh; 10 at creation.";
        0x1fc mr_rec: i16, Named, "Eighth; 0 at creation.";
        0x1fe pad_1fe: [u8; 2], Unknown, "No access found.";
        0x200 mr_approx: i16, Named, "First approximation token; 1 at creation.";
        0x202 pad_202: [u8; 2], Unknown, "No access found.";
    }
}

layout! {
    /// One parametric direction of a NURBS curve or surface. The original has a type named
    /// `SI_NurbsDir`; that this block is it is inferred from the block's recurrence (once in
    /// a curve, twice in a surface), not read from a signature.
    pub struct NurbsDir = "SI_NurbsDir (inferred)", size 0x24,
        "a surface holds the same nine fields at +0 and +0x24" {
        0x00 order: i32, Read, "Degree + 1 (attributes 321, 327, 328 return order - 1); 4 at creation.";
        0x04 nb_points: i32, Read, "Attributes 320, 325, 326.";
        0x08 nb_knots: i32, Read, "Length of knots.";
        0x0c knots: Ptr32<Double>, Read, "Knot vector.";
        0x10 range_start: Double, Named, "Token range, first value.";
        0x18 range_end: Double, Named, "Token range, second value.";
        0x20 closed: i16, Read, "Attributes 322, 329, 330.";
        0x22 pad_22: [u8; 2], Unknown, "No access found.";
    }
}

layout! {
    /// A NURBS curve (`__SI_NurbsCrv`).
    pub struct NurbsCrv = "__SI_NurbsCrv", size 0x64,
        "SI_nurbsCrvAllocate allocates 100 bytes; a face keeps its curves 100 bytes apart" {
        0x00 dir: NurbsDir, Read, "Order, counts, knots, range and closed flag.";
        0x24 param: i32, Read, "Attribute 324 (CrvParam, the parameterisation); written before the knots.";
        0x28 unk_28: i16, Unknown, "0 at creation; copied by SI_nurbsCrvCopy.";
        0x2a pad_2a: [u8; 2], Unknown, "No access found.";
        0x2c points: Ptr32<NurbsPoint>, Read, "Array of dir.nb_points control points.";
        0x30 closing: i32, Named, "Token closing; 1 at creation.";
        0x34 step: i32, Read, "Attribute 323 (CrvStep); 10 at creation.";
        0x38 flags_38: u32, Unknown, "0 at creation; the point-editing routines set bits in its last byte.";
        0x3c unk_3c: [u32; 2], Unknown, "0 at creation; copied as one 8-byte value.";
        0x44 unk_44: i16, Unknown, "0 at creation; SI_nurbsCrvCut writes it.";
        0x46 pad_46: [u8; 2], Unknown, "No access found.";
        0x48 unk_48: i32, Unknown, "0 at creation; read by the proportional-editing routines.";
        0x4c keyframed: i16, Named, "Token keyframed, the last one written.";
        0x4e unk_4e: i16, Unknown, "0 at creation.";
        0x50 length_data: [u32; 3], Named, "Three words set by SI_nurbsCurveUpdateLength.";
        0x5c unk_5c: [u32; 2], Unknown, "0 at creation.";
    }
}

layout! {
    /// A control point of a NURBS curve or surface (`SI_NurbsPoint`).
    pub struct NurbsPoint = "SI_NurbsPoint", size 0x24,
        "the writers step through control points by 0x24: four doubles and a word" {
        0x00 x: Double, Read, "First of four doubles written per point.";
        0x08 y: Double, Read, "Second.";
        0x10 z: Double, Read, "Third.";
        0x18 w: Double, Named, "Fourth: the weight (attribute 539).";
        0x20 flag: i32, Read, "A word written after the doubles.";
    }
}

layout! {
    /// A NURBS surface (`SI_NurbsSrf`).
    pub struct NurbsSrf = "SI_NurbsSrf", size 0xdc,
        "SI_nurbsSrfAllocate allocates 0xdc bytes" {
        0x00 u: NurbsDir, Read, "The U direction.";
        0x24 v: NurbsDir, Read, "The V direction.";
        0x48 nb_points: i32, Read, "Total number of control points: the bound of the writer's loop.";
        0x4c param: i32, Read, "Attribute 339 (SrfParam).";
        0x50 unk_50: i16, Unknown, "0 at creation; set by the revolution and extrusion routines.";
        0x52 pad_52: [u8; 2], Unknown, "No access found.";
        0x54 points: Ptr32<NurbsPoint>, Read, "Array of nb_points control points.";
        0x58 unk_58: i32, Unknown, "0 at creation; si_handleAutoBorder writes it.";
        0x5c border: i16, Named, "Token border.";
        0x5e pad_5e: [u8; 2], Unknown, "No access found.";
        0x60 nb_trims: i32, Read, "Number of trim curves.";
        0x64 trims: Ptr32<Ptr32<NurbsCompCrv>>, Read, "The trim curves (composite curves).";
        0x68 nb_projected: i32, Read, "Number of projected curves (SI_nurbsSrfGetProjCrv); written last.";
        0x6c projected: Ptr32<Ptr32<NurbsCompCrv>>, Read, "The projected curves.";
        0x70 u_step: i32, Read, "Attribute 331 (SrfStep, U); 3 at creation.";
        0x74 v_step: i32, Read, "Attribute 332; 3 at creation.";
        0x78 u_nb_curves: i32, Read, "Attribute 333 (SrfNbCrv, U); 1 at creation.";
        0x7c v_nb_curves: i32, Read, "Attribute 334; 1 at creation.";
        0x80 lines0: Ptr32<Opaque>, Named, "Display lines (SI_nurbSrfUpdateLines, SI_nurbsSrfDisposeLines).";
        0x84 lines1: Ptr32<Opaque>, Named, "Display lines.";
        0x88 u_bottom: i16, Read, "Attribute 335 (SrfBottom, U).";
        0x8a u_top: i16, Read, "Attribute 337 (SrfTop, U).";
        0x8c v_bottom: i16, Read, "Attribute 336.";
        0x8e v_top: i16, Read, "Attribute 338.";
        0x90 unk_90: [i32; 2], Unknown, "1 and 1 at creation; written after the cap flags.";
        0x98 mr_approx: i16, Named, "1 at creation; by position and default the patch's approximation flag.";
        0x9a pad_9a: [u8; 2], Unknown, "No access found.";
        0x9c mr_view: i16, Named, "0 at creation.";
        0x9e pad_9e: [u8; 2], Unknown, "No access found.";
        0xa0 mr_edge: f32, Named, "5.0 at creation.";
        0xa4 mr_ucurv: f32, Named, "0.1 at creation.";
        0xa8 mr_vcurv: f32, Named, "45.0 at creation.";
        0xac mr_min: i16, Named, "1 at creation.";
        0xae mr_max: i16, Named, "10 at creation.";
        0xb0 mr_rec: i16, Named, "0 at creation.";
        0xb2 pad_b2: [u8; 2], Unknown, "No access found.";
        0xb4 unk_b4: [i16; 2], Unknown, "0 at creation; the trimming dialogs write the first.";
        0xb8 unk_b8: [Double; 2], Unknown, "0 at creation; read as doubles by nurbsSrfFunc.";
        0xc8 unk_c8: [u32; 5], Unknown, "0 at creation; two of them are read as generic lists by the trim drawing code.";
    }
}

/// The approximation defaults shared by mesh, patch and surface allocators.
const MR_EDGE: f32 = 5.0;
const MR_UCURV: f32 = 0.1;
const MR_VCURV: f32 = 45.0;

impl Mesh {
    /// A new, empty mesh [soft 0x08458c20 SI_meshAllocate].
    ///
    /// `cos_angle` is the cosine of a double constant of the original (0x0865faf8, the
    /// value 1.04719755, 60 degrees to nine digits), computed on the x87 unit and stored to
    /// a float; the nearest float to that cosine is exactly 0.5.
    /// Not set by the original and zero here: `pad_22`, `pad_3a`, `pad_4e`.
    pub fn allocate() -> Mesh {
        let mut m = Mesh::zero();
        m.angle = 60.0;
        m.cos_angle = f64::from_bits(0x3ff0_c152_37db_38a0).cos() as f32;
        m.mr_edge = MR_EDGE;
        m.mr_ucurv = MR_UCURV;
        m.mr_vcurv = MR_VCURV;
        m.flags = 0x804;
        m.mr_approx = 2;
        m.mr_min = 1;
        m.mr_max = 5;
        m
    }

    /// The number of edge blocks a mesh of `nb_edges` edges has
    /// [soft 0x08441fb8 SI_edgeAllocate]: edges are numbered from 1, 1024 to a block.
    pub fn edge_blocks(nb_edges: i32) -> i32 {
        ((nb_edges + 1) >> 10) + 1
    }
}

/// `count` vertices [soft 0x08459d48 SI_vertexAllocate]. The original rounds the allocation
/// up to a multiple of 32 records and clears it; the records past `count` are not part of
/// the mesh.
pub fn vertex_allocate(count: i32) -> Vec<Vertex> {
    vec![Vertex::zero(); count.max(0) as usize]
}

/// `count` polygons [soft 0x08459e20 SI_polygonAllocate], allocated like the vertices.
pub fn polygon_allocate(count: i32) -> Vec<Polygon> {
    vec![Polygon::zero(); count.max(0) as usize]
}

/// Corners for a polygon of `count` corners [soft 0x08459da8 SI_polygonNodeAllocate]. The
/// original rounds `count` up to a multiple of four and initialises that many records, so
/// the result is longer than `count` unless `count` is a multiple of four.
pub fn polygon_node_allocate(count: i32) -> Vec<PolyNode> {
    let n = count.wrapping_sub(((count.wrapping_sub(1) as u32 & 3) as i32).wrapping_sub(3));
    let mut node = PolyNode::zero();
    node.vertex = -1;
    node.color = 0xffb2_b2b2u32.to_le_bytes();
    vec![node; n.max(0) as usize]
}

/// The edge blocks for `nb_edges` edges [soft 0x08441fb8 SI_edgeAllocate]: every block has
/// 1024 zeroed records; records 0 to `nb_edges` inclusive then get -1 in their four words.
pub fn edge_allocate(nb_edges: i32) -> Vec<Vec<Edge>> {
    let blocks = Mesh::edge_blocks(nb_edges).max(0) as usize;
    let mut out = vec![vec![Edge::zero(); 0x400]; blocks];
    if nb_edges >= 0 {
        for n in 0..=nb_edges as usize {
            let e = &mut out[n >> 10][n & 0x3ff];
            e.v0 = -1;
            e.v1 = -1;
            e.unk_08 = -1;
            e.unk_0c = -1;
        }
    }
    out
}

impl Spline {
    /// Sets a spline's fields to their initial values [soft 0x0848a9f0 SI_splineInit].
    /// It leaves `pad_0a`, `pad_12`, `pad_22`, `pad_6a` and `pad_72` as they were.
    pub fn init(&mut self) {
        self.tension = 0.0;
        self.name = Ptr32::NULL;
        self.unk_04 = 0x10;
        self.closed = 0;
        self.phantom = 0;
        self.nb_keys = 0;
        self.keys = Ptr32::NULL;
        self.unk_18 = 0;
        self.step = 10;
        self.kind = 0;
        self.unk_64 = 2;
        self.unk_66 = 1;
        self.matrix = IDENTITY;
        self.nb_lengths = 0;
        self.lengths = Ptr32::NULL;
        self.unk_70 = 0;
    }

    /// A new spline [soft 0x0848a990 SI_splineAllocate], named `spl<n>` (the C format
    /// `spl%1hd`) from the original's static counter, which each call increments. The name
    /// is returned beside the record; the original stores a `strdup`ed copy at `name`.
    pub fn allocate(counter: &mut i32) -> (Spline, Vec<u8>) {
        let mut s = Spline::zero();
        s.init();
        *counter = counter.wrapping_add(1);
        (s, format!("spl{}", *counter as i16).into_bytes())
    }
}

impl Face {
    /// A new face [soft 0x084347d0 SI_faceAllocate]: an initialised outline spline, no
    /// holes, made of splines. Not set by the original and zero here: `pad_7a`, `pad_86`
    /// and the spline's padding.
    pub fn allocate() -> Face {
        let mut f = Face::zero();
        f.spline.init();
        f.is_spline = 1;
        f
    }
}

impl Patch {
    /// A new patch [soft 0x0847a0d0 SI_patchAllocate].
    /// Not set by the original and zero here: `basis`, `unk_1e0` and the padding.
    pub fn allocate() -> Patch {
        let mut p = Patch::zero();
        p.u_step = 3;
        p.v_step = 3;
        p.u_nb_curves = 1;
        p.v_nb_curves = 1;
        p.matrices = [IDENTITY; 4];
        p.mr_edge = MR_EDGE;
        p.mr_ucurv = MR_UCURV;
        p.mr_vcurv = MR_VCURV;
        p.mr_approx = 1;
        p.mr_min = 1;
        p.mr_max = 10;
        p
    }
}

impl NurbsCrv {
    /// A new curve [soft 0x084db5a0 SI_nurbsCrvAllocate]: order 4, step 10, closing 1.
    /// Not set by the original and zero here: the padding.
    pub fn allocate() -> NurbsCrv {
        let mut c = NurbsCrv::zero();
        c.dir.order = 4;
        c.step = 10;
        c.closing = 1;
        c
    }
}

impl NurbsSrf {
    /// A new surface [soft 0x084db66c SI_nurbsSrfAllocate]: order 4 both ways.
    /// Not set by the original and zero here: the padding.
    pub fn allocate() -> NurbsSrf {
        let mut s = NurbsSrf::zero();
        s.u.order = 4;
        s.v.order = 4;
        s.u_step = 3;
        s.v_step = 3;
        s.u_nb_curves = 1;
        s.v_nb_curves = 1;
        s.unk_90 = [1, 1];
        s.mr_approx = 1;
        s.mr_edge = MR_EDGE;
        s.mr_ucurv = MR_UCURV;
        s.mr_vcurv = MR_VCURV;
        s.mr_min = 1;
        s.mr_max = 10;
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_defaults() {
        let m = Mesh::allocate();
        assert_eq!((m.angle, m.cos_angle, m.flags), (60.0, 0.5, 0x804));
        assert_eq!((m.mr_edge, m.mr_ucurv, m.mr_vcurv), (5.0, 0.1, 45.0));
        assert_eq!((m.mr_approx, m.mr_min, m.mr_max, m.mr_rec, m.mr_view), (2, 1, 5, 0, 0));
        assert!(m.vertices.is_null() && m.nb_edges == 0);
    }

    #[test]
    fn corner_allocation_rounds_up_to_four() {
        for (count, len) in [(0, 0), (1, 4), (3, 4), (4, 4), (5, 8), (8, 8)] {
            assert_eq!(polygon_node_allocate(count).len(), len, "{count}");
        }
        let n = polygon_node_allocate(3);
        assert_eq!((n[3].vertex, n[3].color), (-1, [0xb2, 0xb2, 0xb2, 0xff]));
    }

    #[test]
    fn edges_are_numbered_from_one_in_blocks_of_1024() {
        assert_eq!(Mesh::edge_blocks(0), 1);
        assert_eq!(Mesh::edge_blocks(1022), 1);
        assert_eq!(Mesh::edge_blocks(1023), 2);
        let e = edge_allocate(1024);
        assert_eq!(e.len(), 2);
        assert_eq!((e[0][0].v0, e[1][0].v0, e[1][1].v0), (-1, -1, 0)); // 0..=1024 set, 1025 not
    }

    #[test]
    fn spline_face_patch_and_nurbs_defaults() {
        let mut counter = 0;
        let (s, name) = Spline::allocate(&mut counter);
        assert_eq!(name, b"spl1");
        assert_eq!((s.unk_04, s.step, s.unk_64, s.unk_66, s.matrix), (0x10, 10, 2, 1, IDENTITY));
        let f = Face::allocate();
        assert_eq!((f.is_spline, f.nb_holes, f.spline.step), (1, 0, 10));
        let p = Patch::allocate();
        assert_eq!((p.u_step, p.v_nb_curves, p.mr_approx, p.mr_max, p.matrices[3]), (3, 1, 1, 10, IDENTITY));
        let c = NurbsCrv::allocate();
        assert_eq!((c.dir.order, c.step, c.closing), (4, 10, 1));
        let n = NurbsSrf::allocate();
        assert_eq!((n.u.order, n.v.order, n.u_step, n.v_nb_curves, n.mr_max), (4, 4, 3, 1, 10));
        // the V direction's range is a double at an offset that is not a multiple of 8
        assert_eq!(core::mem::offset_of!(NurbsSrf, v) + core::mem::offset_of!(NurbsDir, range_start), 0x34);
    }
}
