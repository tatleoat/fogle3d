// SPDX-License-Identifier: MIT
//! Records of the four source files that `si-types` does not lay out. Each field's evidence
//! is in finding 0028: read from the code of the routines named, and for the routines that
//! are checked, measured (the cases compare every byte).

use si_mem::{record, CStr32, Opaque, Ptr32};
use si_types::model::Model;

record! {
    /// A lattice (`SI_Lattice`, 0x44 bytes from `SI_malloc`): a box of control points, laid
    /// out x outermost, then y, then z.
    pub struct Lattice = "SI_Lattice", size 0x44 {
        0x00 active: u16, "not 0: the lattice deforms (the lattice file's ACTIVE; 1 at creation)";
        0x02 nx: i16, "control points along x";
        0x04 ny: i16, "along y";
        0x06 nz: i16, "along z";
        0x08 interp: [i32; 3], "per axis: 0 linear between two points, else a cubic B-spline through four";
        0x14 keys: Ptr32<[f32; 3]>, "the points, nx*ny*nz vectors from SI_calloc: index (i*ny + j)*nz + k";
        0x18 flags: Ptr32<u16>, "one 16-bit word per point from SI_calloc; bit 0 = tagged (SI_latticeRandomize)";
        0x1c min: [f32; 3], "the box's lower corner: where point (0, 0, 0) rests";
        0x28 max: [f32; 3], "its upper corner";
        0x34 step: [f32; 3], "the rest distance between points per axis: (max - min) / (n - 1)";
        0x40 unk_40: u32, "0 from latticeAllocate; SI_latticeInitialize leaves it unwritten; no routine of lattice.c reads it";
    }
}

record! {
    /// A spline deformation (`SI_SplFitting`, 0xa8 bytes): the model follows a curve.
    pub struct SplFitting = "SI_SplFitting", size 0xa8 {
        0x00 active: u16, "not 0: the fitting deforms; 1 at creation";
        0x02 pad_02: [u8; 2], "never written";
        0x04 model: Ptr32<Model>, "the node whose curve is followed (kind 9: a NURBS curve, else a spline)";
        0x08 frenet: Ptr32<Opaque>, "the frames along the curve, built by the curve's package and freed with the record";
        0x0c scale: [f32; 3], "1, 1, 1 at creation";
        0x18 roll: f32, "an angle in radians about the curve";
        0x1c trans: [f32; 3], "translation";
        0x28 matrix: [[f32; 4]; 4], "identity at creation; SI_fittingUpdateMatrix fills seven elements from scale, roll and translation";
        0x68 normal_matrix: [[f32; 4]; 4], "the transposed inverse of it";
    }
}

record! {
    /// A patch deformation (`SI_PtchFitting`, 0xac bytes): the model follows a surface.
    pub struct PtchFitting = "SI_PtchFitting", size 0xac {
        0x00 active: u16, "not 0: the fitting deforms; 1 at creation";
        0x02 pad_02: [u8; 2], "never written";
        0x04 model: Ptr32<Model>, "the node whose surface is followed";
        0x08 scale: [f32; 3], "1, 1, 1 at creation";
        0x14 rot: [f32; 3], "three angles in radians";
        0x20 trans: [f32; 3], "translation";
        0x2c matrix: [[f32; 4]; 4], "identity at creation; SI_pfittingUpdateMatrix fills the upper three rows and the translation";
        0x6c normal_matrix: [[f32; 4]; 4], "the transposed inverse of it; not set at creation";
    }
}

record! {
    /// A cluster of points (`kluster`, 0x20 bytes): a named, sorted list of point numbers of
    /// one model, in a chain from the model's word at +0x348.
    pub struct Kluster = "kluster", size 0x20 {
        0x00 name: CStr32, "the name: the pointer SI_klusterAddToList was handed; freed with the cluster";
        0x04 count: i32, "how many points";
        0x08 unk_08: i32, "0 at creation; SI_modelKlusterResetCluInd clears it";
        0x0c indexes: Ptr32<i32>, "the point numbers, ascending: one word each, or for a face (kind 2) two: curve and point";
        0x10 state: i32, "0 active, 1 not active, 2 to be disposed, 3 active and 4 not active with its animation to be deleted";
        0x14 unk_14: u32, "never written in klusters.c";
        0x18 anim: Ptr32<Opaque>, "the cluster's shape keys (the cluster animation's record)";
        0x1c next: Ptr32<Kluster>, "the next cluster of the model";
    }
}

record! {
    /// The per-process record `SI_procData` points to (`SI_procDataStorage`): as finding
    /// 0027 has it.
    pub struct ProcData = "SI_procData record", size 0x0c {
        0x00 dict: i32, "index of the current element dictionary in SI_elemDicts";
        0x04 list: i32, "index of the scene's graph list in SI_grafLists";
        0x08 camera: Ptr32<Opaque>, "the scene's camera";
    }
}

/// Values of [`Kluster::state`].
pub mod state {
    pub const ACTIVE: i32 = 0;
    pub const INACTIVE: i32 = 1;
    pub const DISPOSE: i32 = 2;
    pub const ACTIVE_FLAGGED: i32 = 3;
    pub const INACTIVE_FLAGGED: i32 = 4;
}

/// Offsets in the geometry records, as the routines of this crate reach them (named
/// offsets: the records belong to the model, spline and NURBS packages and have no typed
/// layout on the image yet). Read from `SI_latticeTransform`, `klusterGetIndexes` and
/// `klusterTagCVs`; they agree with finding 0015's tables.
pub mod geom {
    /// Mesh: +0 the vertices, +4 the polygons, +0x14 the vertex count, +0x20 a flag byte
    /// whose bit 4 means "normals are carried, not recomputed".
    pub const MESH_VERTS: u32 = 0x00;
    pub const MESH_POLYS: u32 = 0x04;
    pub const MESH_NB_VERTS: u32 = 0x14;
    pub const MESH_FLAGS: u32 = 0x20;
    pub const MESH_CARRY_NORMALS: u8 = 0x04;
    /// Vertex, 0x14 bytes: position, +0xc its star (12-byte entries whose third word is a
    /// polygon number or -1), +0x10 how many (16 bits), +0x12 a flag byte (bit 0 tagged).
    pub const VERT_SIZE: u32 = 0x14;
    pub const VERT_STAR: u32 = 0x0c;
    pub const VERT_NB_STAR: u32 = 0x10;
    pub const VERT_FLAGS: u32 = 0x12;
    pub const STAR_SIZE: u32 = 0x0c;
    pub const STAR_POLY: u32 = 0x08;
    /// Polygon, 0x18 bytes: +0xc its corners, +0x10 how many (16 bits).
    pub const POLY_SIZE: u32 = 0x18;
    pub const POLY_CORNERS: u32 = 0x0c;
    pub const POLY_NB_CORNERS: u32 = 0x10;
    /// Corner, 0x24 bytes: +0 the vertex number, +8 the normal.
    pub const CORNER_SIZE: u32 = 0x24;
    pub const CORNER_VERT: u32 = 0x00;
    pub const CORNER_NORMAL: u32 = 0x08;
    /// Patch: +0xc, +0xe the point counts (16 bits), +0x1a4 the points (16 bytes each),
    /// +0x1a8 a 16-bit flag word per point.
    pub const PATCH_NU: u32 = 0x0c;
    pub const PATCH_NV: u32 = 0x0e;
    pub const PATCH_POINTS: u32 = 0x1a4;
    pub const PATCH_POINT_SIZE: u32 = 0x10;
    pub const PATCH_FLAGS: u32 = 0x1a8;
    /// Spline: +0xc its type (1: a key's two tangent points move with it), +0x10 the key
    /// count (16 bits), +0x14 the keys. Key, 0x30 bytes: three vectors (the point is the
    /// middle one), +0x2c a flag byte.
    pub const SPLINE_TYPE: u32 = 0x0c;
    pub const SPLINE_NB_KEYS: u32 = 0x10;
    pub const SPLINE_KEYS: u32 = 0x14;
    pub const KEY_SIZE: u32 = 0x30;
    pub const KEY_POINT: u32 = 0x0c;
    pub const KEY_FLAGS: u32 = 0x2c;
    /// Face: +4 the outline (a spline in place), +0x78 the number of holes (16 bits, signed),
    /// +0x7c the holes (splines, 0x74 bytes each), +0x80 the same curves as NURBS (0x64
    /// bytes each), +0x84 not 0: the face is made of splines.
    pub const FACE_OUTLINE: u32 = 0x04;
    pub const FACE_NB_HOLES: u32 = 0x78;
    pub const FACE_HOLES: u32 = 0x7c;
    pub const FACE_SPLINE_SIZE: u32 = 0x74;
    pub const FACE_NURBS: u32 = 0x80;
    pub const FACE_NURBS_SIZE: u32 = 0x64;
    pub const FACE_IS_SPLINE: u32 = 0x84;
    /// NURBS curve: +4 the point count, +0x2c the points, +0x3b a flag byte. Control point,
    /// 0x24 bytes: x, y, z, w as doubles, +0x20 a flag byte.
    pub const CRV_NB_POINTS: u32 = 0x04;
    pub const CRV_POINTS: u32 = 0x2c;
    pub const CRV_FLAGS: u32 = 0x3b;
    pub const CP_SIZE: u32 = 0x24;
    pub const CP_W: u32 = 0x18;
    pub const CP_FLAGS: u32 = 0x20;
    /// NURBS surface: +0x48 the point count, +0x54 the points, +0x5b a flag byte.
    pub const SRF_NB_POINTS: u32 = 0x48;
    pub const SRF_POINTS: u32 = 0x54;
    pub const SRF_FLAGS: u32 = 0x5b;
    /// The two bits set in a curve's or surface's flag byte after its points moved.
    pub const NURBS_MOVED: u8 = 0x60;
}
