// SPDX-License-Identifier: MIT
//! What the readers leave in memory, reduced to the fields a file can set or the writers
//! read. Each `Default` gives the values the original's allocation routine puts in those
//! fields. Where that routine is named in backticks with its address, only the fields kept
//! here are reproduced (the original structure has many more); where it is cited in
//! brackets, the structure here is the whole of what it sets up. A field's comment gives
//! its offset in the original structure where that helps to follow the readers.
//!
//! One simplification: a string or list that the original holds through a pointer is a
//! plain vector here, empty when the pointer is null. The writers treat a null string and an
//! empty one alike (one zero byte), so nothing a file can hold is lost.

pub type Vec3 = [f32; 3];

/// One block of user data (the original's 20-byte `__SI_UserNode`: tag, flag, size, data,
/// next).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UserNode {
    /// The tag buffer. For the old 4-character form: the four bytes read (the original
    /// holds them in a zeroed 5-byte buffer), or `_` and the swapped bytes for a tag
    /// converted from a file before 3.50009.
    pub tag: Vec<u8>,
    /// +4. 1 for the old form; read from the file ("big-endian flag" in the vendor's page)
    /// for the new one.
    pub flag: i16,
    /// +8, as stored.
    pub size: i32,
    pub data: Vec<u8>,
}

/// A user-defined shader reference (the original's 60-byte `__SI_UDF`)
/// `SI_udfAllocate` (0x0854c2f4): only its zeroed fields are reproduced, not the global
/// list the original registers it in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Udf {
    pub name: Vec<u8>,
    /// +0xc, +0x10: stored as 4-byte values in the file, kept as shorts.
    pub active: i16,
    pub internal: i16,
    /// +0x14
    pub order: i32,
}

/// A 2D texture (`__SI_LclMap`, 0x1d8 bytes) `SI_textureInitialize` (0x082b2508).
#[derive(Clone, Debug, PartialEq)]
pub struct Texture2D {
    pub name: Vec<u8>,             // +0x14, "NO_NAME"
    pub filename: Vec<u8>,         // +0x18
    pub anim: i16,                 // +0x28
    pub method: i16,               // +0x2a
    pub pixel_interp: i16,         // +0x2c
    pub crop_flag: i16,            // +0x2e
    /// +0x30 to +0x38. Tokens 0x16 to 0x19 each read and write *two* shorts, and their
    /// targets overlap: 0x16 fills elements 0 and 1, 0x17 elements 2 and 3, 0x18 elements 1
    /// and 2, 0x19 elements 3 and 4. The four crop bounds are elements 0 to 3; element 4 is
    /// the neighbouring field.
    pub crop: [i16; 5],
    pub uscale: f32,  // +0x58
    pub vscale: f32,  // +0x5c
    pub uoffset: f32, // +0x68
    pub voffset: f32, // +0x6c
    pub rotation: Vec3,    // +0x94
    pub translation: Vec3, // +0xa0
    pub scaling: Vec3,     // +0xac
    pub urepeat: i16, // +0xf8
    pub vrepeat: i16, // +0xfa
    pub swap_uv: i16, // +0xfc
    pub alt_u: i16,   // +0xfe
    pub alt_v: i16,   // +0x100
    /// +0x102, +0x104: not in the file; set from the model the texture hangs on.
    pub wrap_u: i16,
    pub wrap_v: i16,
    pub mask: i16,          // +0x10a
    pub blend: f32,         // +0x10c
    pub channel: i16,       // +0x110
    pub ambient: f32,       // +0x128
    pub diffuse: f32,       // +0x12c
    pub specular: f32,      // +0x130
    pub reflectivity: f32,  // +0x134
    pub transparency: f32,  // +0x138
    pub old_factor: f32,    // +0x13c, token 0x11 only
    pub roughness: f32,     // +0x140
    pub reflection_map: f32, // +0x144
    pub env_rotation: f32,  // +0x168
    pub black_white: i16,   // +0x18e
    pub udf: Option<Udf>,   // +0x198
    pub displacement: i16,  // +0x19c
    pub hw_flag: i16,       // +0x1a0
    pub hw_tiling: i16,     // +0x1a2
    pub x1cc: i16,
    pub x1ce: i16,
    pub user_data: Vec<UserNode>, // +0x1d0
    pub x1d4: i16,
}

impl Default for Texture2D {
    /// The method's default is the global `defaultMapMethod` (0x086a4d40), 1 at start-up.
    /// `reflectivity` is the one field the original does not initialise: a file without
    /// token 0x1d leaves stale stack there; here it is 0.
    fn default() -> Self {
        Texture2D {
            name: b"NO_NAME".to_vec(), filename: Vec::new(), anim: 0, method: 1, pixel_interp: 1, crop_flag: 0,
            crop: [0; 5], uscale: 1.0, vscale: 1.0, uoffset: 0.0, voffset: 0.0, rotation: [0.0; 3],
            translation: [0.0; 3], scaling: [1.0; 3], urepeat: 1, vrepeat: 1, swap_uv: 0, alt_u: 0, alt_v: 0,
            wrap_u: 0, wrap_v: 0, mask: 3, blend: 1.0, channel: 2, ambient: 0.75, diffuse: 1.0, specular: 0.0,
            reflectivity: 0.0, transparency: 0.0, old_factor: 0.0, roughness: 0.0, reflection_map: 1.0,
            env_rotation: 0.0, black_white: 0, udf: None, displacement: 0, hw_flag: 0, hw_tiling: 0, x1cc: 0,
            x1ce: 0, user_data: Vec::new(), x1d4: 0,
        }
    }
}

/// A 3D texture (`__SI_Texture3D`, 0x158 bytes) `SI_texture3DInitialize` (0x082b8ab0).
#[derive(Clone, Debug, PartialEq)]
pub struct Texture3D {
    pub name: Vec<u8>,         // +0
    pub kind: i32,             // +0xc
    pub spacing: f32,          // +0x10
    pub taper: f32,            // +0x14
    pub strength: f32,         // +0x18
    pub iterations: i32,       // +0x1c
    pub power: f32,            // +0x20
    pub mask: i16,             // +0x24
    pub blending: f32,         // +0x28
    pub channel: i16,          // +0x2c
    pub colours: [[f32; 4]; 5], // +0x30
    pub factors: [f32; 3],     // +0x80
    pub scaling: Vec3,         // +0x90
    pub rotation: Vec3,        // +0x9c
    pub translation: Vec3,     // +0xa8
    pub ambient: f32,          // +0x108
    pub diffuse: f32,          // +0x10c
    pub specular: f32,         // +0x110
    pub reflection: f32,       // +0x114
    pub transparency: f32,     // +0x118
    pub roughness: f32,        // +0x120
    pub udf: Option<Udf>,      // +0x14c
    pub displacement: i16,     // +0x150
    pub user_data: Vec<UserNode>, // +0x154
}

impl Default for Texture3D {
    /// `reflection` (+0x114) is not initialised by the original; 0 here.
    fn default() -> Self {
        Texture3D {
            name: Vec::new(), kind: 0, spacing: 1.0, taper: 0.0, strength: 1.0, iterations: 1, power: 1.0, mask: 3,
            blending: 1.0, channel: 1,
            colours: [[0.0; 4], [0.25, 0.25, 0.25, 0.0], [0.5, 0.5, 0.5, 0.0], [0.75, 0.75, 0.75, 0.0], [1.0, 1.0, 1.0, 0.0]],
            factors: [0.25, 0.5, 0.75], scaling: [1.0; 3], rotation: [0.0; 3], translation: [0.0; 3], ambient: 0.75,
            diffuse: 1.0, specular: 0.0, reflection: 0.0, transparency: 0.0, roughness: 0.0, udf: None,
            displacement: 0, user_data: Vec::new(),
        }
    }
}

/// A material (`__SI_Material`, 0xdcc bytes) `SI_materialAllocate` (0x08288c74)
/// `SI_materialInitialize` (0x082889b0).
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    /// +0: a 30-byte field in the original, filled with `strcpy`; a longer name overruns
    /// into the fields behind it there. Kept whole here.
    pub name: Vec<u8>,
    pub shading: i16,       // +0x1e
    pub glow: i16,          // +0x28
    pub glow_amount: f32,   // +0x2c
    pub ambient: Vec3,      // +0x34
    pub diffuse: Vec3,      // +0x44
    pub specular: Vec3,     // +0x54
    pub exponent: f32,      // +0x64
    pub reflectivity: f32,  // +0x6a8
    pub x6ac: f32,          // token 10 only
    pub transparency: f32,  // +0x6b8
    pub x6bc: f32,          // token 8 only
    pub refraction: f32,    // +0x6dc
    pub textures: Vec<Texture2D>,     // +0x6ec
    pub texture3d: Option<Texture3D>, // +0x6fc
    pub blur: i16,          // +0x758
    pub blur_width: f32,    // +0x75c
    pub blur_decay: f32,    // +0x760
    pub udf_material: Option<Udf>, // +0xdb0
    pub udf_volume: Option<Udf>,   // +0xdb4
    pub udf_shadow: Option<Udf>,   // +0xdb8
    pub udf_extra: Option<Udf>,    // +0xdbc, token 0xff1a
    pub user_data: Vec<UserNode>,  // +0xdc4
    pub xdc8: i16,
    pub xdca: i16,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            name: b"default".to_vec(), shading: 4, glow: 0, glow_amount: 0.0, ambient: [0.5; 3], diffuse: [0.7; 3],
            specular: [1.0; 3], exponent: 50.0, reflectivity: 0.0, x6ac: 0.0, transparency: 0.0, x6bc: 0.0,
            refraction: 1.0, textures: Vec::new(), texture3d: None, blur: 0, blur_width: 1.0, blur_decay: 3.0,
            udf_material: None, udf_volume: None, udf_shadow: None, udf_extra: None, user_data: Vec::new(),
            xdc8: 1, xdca: 1,
        }
    }
}

/// `SI_splineKeyAllocate` (0x0848f8dc)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplineKey {
    pub prev: Vec3,
    pub pos: Vec3,
    pub next: Vec3,
    pub constraint: i16, // +0x28
    pub segment: i16,    // +0x2a
    pub flag: i16,       // +0x2c
}

/// `SI_splineAllocate` (0x0848a990) `SI_splineInit` (0x0848a9f0). The allocator names
/// a spline `spl-N` from a running counter; that default is not reproduced (every file
/// from 2.045 on stores the name), so a spline without a stored name has none here.
#[derive(Clone, Debug, PartialEq)]
pub struct Spline {
    pub name: Vec<u8>,
    pub phantom: i16, // +6
    pub close: i16,   // +8
    pub kind: i32,    // +0xc
    /// +0x10: the stored count, unsigned.
    pub nkeys: u16,
    pub keys: Vec<SplineKey>,
    pub tension: f32, // +0x1c
    pub step: i16,    // +0x20
    /// Set when the keys came from token 2 (positions only). The original then rebuilds a
    /// Bézier spline with `SI_bezierShrink`, which is not ported.
    pub old_points: bool,
}

impl Default for Spline {
    fn default() -> Self {
        Spline { name: Vec::new(), phantom: 0, close: 0, kind: 0, nkeys: 0, keys: Vec::new(), tension: 0.0, step: 10, old_points: false }
    }
}

/// A control point of a NURBS curve or surface (36 bytes: four doubles and a flag)
/// [soft 0x084dc47c SI_nurbsPointsAllocate].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NurbsPoint {
    pub xyzw: [f64; 4],
    pub flag: i32,
}

impl Default for NurbsPoint {
    fn default() -> Self {
        NurbsPoint { xyzw: [0.0, 0.0, 0.0, 1.0], flag: 0 }
    }
}

/// `SI_nurbsCrvAllocate` (0x084db5a0)
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsCurve {
    pub order: i32,   // +0
    pub npoints: i32, // +4
    pub points: Vec<NurbsPoint>,
    pub nknots: i32, // +8
    pub knots: Vec<f64>,
    pub start: f64,      // +0x10
    pub end: f64,        // +0x18
    pub closed: i16,     // +0x20
    pub uniformity: i32, // +0x24
    pub closing: i32,    // +0x30
    pub step: i32,       // +0x34
    pub keyframed: i16,  // +0x4c
}

impl Default for NurbsCurve {
    fn default() -> Self {
        NurbsCurve { order: 4, npoints: 0, points: Vec::new(), nknots: 0, knots: Vec::new(), start: 0.0, end: 0.0,
                     closed: 0, uniformity: 0, closing: 1, step: 10, keyframed: 0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NurbsComposite {
    pub boundary: i16,
    pub ncurves: i32,
    pub curves: Vec<NurbsCurve>,
}

/// `SI_nurbsSrfAllocate` (0x084db66c)
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsSurface {
    pub order: [i32; 2],  // +0, +0x24
    pub count: [i32; 2],  // +4, +0x28
    pub points: Vec<NurbsPoint>,
    pub closed: [i16; 2], // +0x20, +0x44
    pub uniformity: i32,  // +0x4c
    pub nknots: [i32; 2], // +8, +0x2c
    pub knots: [Vec<f64>; 2],
    pub range: [f64; 4],  // +0x10, +0x18, +0x34, +0x3c
    pub border: i16,      // +0x5c
    pub ntrims: i32,      // +0x60
    pub trims: Vec<NurbsComposite>,
    pub nprojected: i32,  // +0x68
    pub projected: Vec<NurbsComposite>,
    pub steps: [i32; 2],   // +0x70
    pub curves: [i32; 2],  // +0x78
    pub caps: [i16; 4],    // +0x88
    pub closing: [i32; 2], // +0x90
    pub approx: i16,       // +0x98
    pub view: i16,         // +0x9c
    pub edge: f32,         // +0xa0
    pub curvature: [f32; 2], // +0xa4
    pub rec_min: i16,      // +0xac
    pub rec_max: i16,      // +0xae
    pub recursion: i16,    // +0xb0
}

impl Default for NurbsSurface {
    fn default() -> Self {
        NurbsSurface {
            order: [4, 4], count: [0, 0], points: Vec::new(), closed: [0, 0], uniformity: 0, nknots: [0, 0],
            knots: [Vec::new(), Vec::new()], range: [0.0; 4], border: 0, ntrims: 0, trims: Vec::new(), nprojected: 0,
            projected: Vec::new(), steps: [3, 3], curves: [1, 1], caps: [0; 4], closing: [1, 1], approx: 1, view: 0,
            edge: 5.0, curvature: [0.1, 45.0], rec_min: 1, rec_max: 10, recursion: 0,
        }
    }
}

/// `SI_patchAllocate` (0x0847a0d0)
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub kind: [i32; 2], // +4, +8
    pub nu: u16,        // +0xc
    pub nv: u16,        // +0xe
    /// 16 bytes each in the original, with a fourth component set to 1.
    pub points: Vec<Vec3>,
    pub tags: Vec<i16>,
    pub close: [i16; 2],   // +0x10
    pub tension: [f32; 2], // +0x14
    pub step: [i16; 2],    // +0x1c
    pub curves: [i16; 2],  // +0x20
    pub caps: [i16; 4],    // +0x1cc: U bottom, U top, V bottom, V top
    pub faceted: i16,      // +0x1d4
    pub view: i16,         // +0x1e8
    pub edge: f32,         // +0x1ec
    pub curvature: [f32; 2], // +0x1f0
    pub rec_min: i16,      // +0x1f8
    pub rec_max: i16,      // +0x1fa
    pub recursion: i16,    // +0x1fc
    pub approx: i16,       // +0x200
}

impl Default for Patch {
    fn default() -> Self {
        Patch { kind: [0, 0], nu: 0, nv: 0, points: Vec::new(), tags: Vec::new(), close: [0, 0], tension: [0.0, 0.0],
                step: [3, 3], curves: [1, 1], caps: [0; 4], faceted: 0, view: 0, edge: 5.0, curvature: [0.1, 45.0],
                rec_min: 1, rec_max: 10, recursion: 0, approx: 1 }
    }
}

/// [soft 0x084347d0 SI_faceAllocate]
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub exterior: Spline, // +4
    pub count: i16,       // +0x78: holes, or NURBS curves
    pub holes: Vec<Spline>,
    pub nurbs: Vec<NurbsCurve>,
    pub is_spline: i16, // +0x84
}

impl Default for Face {
    fn default() -> Self {
        Face { exterior: Spline::default(), count: 0, holes: Vec::new(), nurbs: Vec::new(), is_spline: 1 }
    }
}

/// [soft 0x08459d48 SI_vertexAllocate]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    pub pos: Vec3,
    pub flag: i16, // +0x12
}

/// One corner of a polygon (36 bytes) [soft 0x08459da8 SI_polygonNodeAllocate].
#[derive(Clone, Debug, PartialEq)]
pub struct PolyNode {
    pub vertex: i32,  // +0
    pub normal: Vec3, // +8
    pub uv: [f32; 2], // +0x14
    /// +0x1c, as the 32-bit value the file stores big-endian.
    pub colour: u32,
    /// +0x20: the texture coordinates of the model's further UV sets, one pair each.
    pub more_uv: Vec<[f32; 2]>,
}

impl Default for PolyNode {
    fn default() -> Self {
        PolyNode { vertex: -1, normal: [0.0; 3], uv: [0.0; 2], colour: 0xffb2_b2b2, more_uv: Vec::new() }
    }
}

/// [soft 0x08459e20 SI_polygonAllocate]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polygon {
    pub normal: Vec3, // +0, token 1 only
    /// +0x10: the stored node count, unsigned.
    pub nnodes: u16,
    pub nodes: Vec<PolyNode>,
    pub material: i16, // +0x12
    pub flag: i16,     // +0x14
}

/// An edge as the file lists it `SI_edgeAllocate` (0x08441fb8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub v: [i32; 2],
    pub flag: i16, // +0x12
}

impl Default for Edge {
    fn default() -> Self {
        Edge { v: [-1, -1], flag: 0 }
    }
}

/// `SI_meshAllocate` (0x08458c20)
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    pub angle: f32, // +0xc; its cosine is kept at +0x10
    pub nvertices: i32, // +0x14
    pub vertices: Vec<Vertex>,
    pub npolygons: i32, // +0x18
    pub polygons: Vec<Polygon>,
    /// The file's edge list. The original reads it into a scratch table, rebuilds the edges
    /// from the polygons (`SI_meshUpdateNodes`, not ported) and copies only each listed
    /// edge's flag across; on writing it lists the rebuilt edges. Here the list is kept and
    /// written as read.
    pub nedges: i32,
    pub edges: Vec<Edge>,
    pub flag: i16, // +0x20
    pub x38: i16,  // token 0x20c
    pub approx: [f32; 3], // +0x3c: tokens 0x40d to 0x40f
    pub x48: i16,  // 0x210
    pub x4a: i16,  // 0x211
    pub x4c: i16,  // 0x212
    pub x50: i16,  // 0x20b
    pub x52: i16,  // 0x20a
    /// False once a stored node normal was read: the original recomputes normals otherwise.
    pub compute_normals: bool,
}

impl Default for Mesh {
    fn default() -> Self {
        Mesh { angle: 60.0, nvertices: 0, vertices: Vec::new(), npolygons: 0, polygons: Vec::new(), nedges: 0,
               edges: Vec::new(), flag: 0x804, x38: 0, approx: [5.0, 0.1, 45.0], x48: 1, x4a: 5, x4c: 0, x50: 2,
               x52: 0, compute_normals: true }
    }
}

/// `SI_metaballAllocate` (0x0845aa90)
#[derive(Clone, Debug, PartialEq)]
pub struct Metaball {
    pub matrix: [f32; 16], // +0xc
    pub ratio: f64,        // +0xcc
    pub weight: f64,       // +0xd4
    pub exponent: [f64; 3], // +0xdc
}

impl Default for Metaball {
    fn default() -> Self {
        let mut matrix = [0.0; 16];
        for i in 0..4 {
            matrix[5 * i] = 1.0;
        }
        Metaball { matrix, ratio: 0.5, weight: 2.0, exponent: [2.0; 3] }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetaGroup {
    pub name: Vec<u8>,
    /// Names of the group's metaballs, in the order of the original's list: each one read
    /// is linked in at the head, so this is the reverse of the file's order.
    pub primitives: Vec<Vec<u8>>,
}

/// `SI_metaSystemAllocate` (0x0845c78c)
#[derive(Clone, Debug, PartialEq)]
pub struct MetaSystem {
    pub wire_res: f32,   // +0x34
    pub render_res: f32, // +0x38
    pub show: i16,       // +0x42
    /// +0x5c, list order: a new group goes to the head; a name already present is reused.
    pub groups: Vec<MetaGroup>,
    /// +0x60, list order (each one read goes to the head).
    pub unblends: Vec<[Vec<u8>; 2]>,
}

impl Default for MetaSystem {
    fn default() -> Self {
        MetaSystem { wire_res: 0.5, render_res: 0.25, show: 0, groups: Vec::new(), unblends: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Definition {
    Patch(Patch),
    Face(Face),
    Mesh(Mesh),
    Spline(Spline),
    MetaSystem(MetaSystem),
    Metaball(Metaball),
    NurbsCurve(NurbsCurve),
    NurbsSurface(NurbsSurface),
}

/// `SI_dynamatterAllocate` (0x0849fa24) `SI_dynamatterSetDefault` (0x0849f19c)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynaMatter {
    pub density: f32,       // +0
    pub roughness: f32,     // +0xc
    pub kinetic: f32,       // +0x10
    pub elasticity: f32,    // +0x14
    pub rigidity: f32,      // +0x18
    pub plasticity: f32,    // +0x1c
    pub collision: i32,     // +0x80
}

impl Default for DynaMatter {
    fn default() -> Self {
        DynaMatter { density: 0.2, roughness: 0.9, kinetic: 0.3, elasticity: 0.6, rigidity: 1.0, plasticity: 0.0, collision: -2 }
    }
}

/// [soft 0x084bba28 SI_envelopVertAllocate]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvelopVertex {
    pub index: i32,
    pub ratio: f32,
    pub thickness: f32,
    pub weight: f32,
}

impl Default for EnvelopVertex {
    fn default() -> Self {
        EnvelopVertex { index: 0, ratio: 0.0, thickness: 0.0, weight: 100.0 }
    }
}

/// `SI_envelopInfoAllocate` (0x084bba94)
#[derive(Clone, Debug, PartialEq)]
pub struct Envelop {
    pub level: i32, // +0
    /// +4: one list of vertices per enveloped model [soft 0x084bba60 SI_envelopListAllocate].
    pub lists: Vec<Vec<EnvelopVertex>>,
    pub x54: Vec3, // filled only by the pre-prototype envelope chunk
    pub joint_area: f32, // +0x6c
    pub in_type: i32,    // +0x70
    pub in_curve: Option<Vec<f32>>, // +0x74
    pub inner: [f32; 4], // +0x78: begin, end, amplitude, scale
    pub out_type: i32,   // +0x88
    pub out_curve: Option<Vec<f32>>, // +0x8c
    pub outer: [f32; 4], // +0x90
    /// Envelope-vertex token 5 (files up to 2.59): one matrix per vertex, per list. The
    /// original hangs them on the model as an unnamed user node for `SI_envelopRebuild`
    /// (not ported), replacing whatever user nodes the model had.
    pub old_matrices: Vec<Vec<[f32; 16]>>,
}

impl Default for Envelop {
    fn default() -> Self {
        Envelop { level: -1, lists: Vec::new(), x54: [0.0; 3], joint_area: 0.0, in_type: 0, in_curve: None,
                  inner: [0.0, 1.0, 0.0, 1.0], out_type: 0, out_curve: None, outer: [0.0, 1.0, 0.0, 1.0],
                  old_matrices: Vec::new() }
    }
}

/// `SI_invKinAllocate` (0x084ad8f0), the three fields the file holds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InvKin {
    pub kind: i32,      // +0
    pub nb_joints: i32, // +4
    pub rotation: Vec3, // +0x20
}

/// [soft 0x0843a9b0 SI_klusterAllocate]
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    pub name: Vec<u8>,
    pub count: i32,        // +4
    pub indices: Vec<i32>, // +0xc
    pub state: i32,        // +0x10
}

impl Default for Cluster {
    fn default() -> Self {
        Cluster { name: Vec::new(), count: 0, indices: Vec::new(), state: 1 }
    }
}

/// Scaling, rotation, translation.
pub type Transfo = [Vec3; 3];

/// One node of a model hierarchy (`__SI_Model`, 0x430 bytes) `SI_modelAllocate` (0x08477cf4).
/// The allocator names a model `obj-N` from a running counter; that default is not
/// reproduced (every file stores the name), so the name starts empty here.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub name: Vec<u8>,         // +4
    pub flag8: bool,           // bit 8 of the byte at +8, token 0x224
    pub kind: i32,             // +0x30
    /// +0x38: the shapes of the node. A file gives at most one per definition token.
    pub definitions: Vec<Definition>,
    pub scaling: Vec3,     // +0x100
    pub rotation: Vec3,    // +0x10c
    pub translation: Vec3, // +0x158
    pub nb_materials: i16, // +0x206
    /// +0x20c: an external material (stored by name) leaves a null entry.
    pub materials: Vec<Option<Material>>,
    pub nb_textures: i16, // +0x216, as stored
    pub textures: Vec<Texture2D>, // +0x21c
    pub texture3d: Option<Texture3D>, // +0x224
    pub brother: Option<Box<Node>>, // +0x2c0
    pub son: Option<Box<Node>>,     // +0x2c4
    pub shape_interp: i16, // +0x2e0
    pub algorithm: i32,    // +0x2ec
    pub dynamics: Option<DynaMatter>, // +0x2f4
    pub joint_friction: Option<Vec3>, // +0x2f8
    pub chain_dynamics: bool, // +0x304 is set
    pub ik: Option<InvKin>,   // +0x318
    pub envelop_type: i32,    // +0x31c
    pub envelop: Option<Envelop>, // +0x324
    pub init_transfo: Option<Transfo>, // +0x328
    pub clusters: Vec<Cluster>,   // +0x348
    pub user_nodes: Vec<UserNode>, // +0x34c
    pub visibility: i16,  // +0x3ec
    pub motion_blur: i16, // +0x3ee
    pub matte: f32,       // +0x3f4
    pub garbage_matte: i16, // +0x3f8
    pub x404: i16,
    pub x406: i16,
    /// +0x408 to +0x41c: user data on vertices, polygons and edges. A cluster names the
    /// components; the array holds one list of blocks per component.
    pub ud_clusters: [Option<Cluster>; 3],
    pub ud_arrays: [Option<Vec<Vec<UserNode>>>; 3],
    pub x420: i16,
    /// +0x424: number of UV sets, 1 by default; +0x428: for each further set a list of ids.
    pub uv_sets: i32,
    pub uv_domains: Option<Vec<Vec<i32>>>,
}

impl Default for Node {
    fn default() -> Self {
        Node {
            name: Vec::new(), flag8: false, kind: 0, definitions: Vec::new(), scaling: [1.0; 3], rotation: [0.0; 3],
            translation: [0.0; 3], nb_materials: 0, materials: Vec::new(), nb_textures: 0, textures: Vec::new(),
            texture3d: None, brother: None, son: None, shape_interp: 0, algorithm: 0, dynamics: None,
            joint_friction: None, chain_dynamics: false, ik: None, envelop_type: 0, envelop: None,
            init_transfo: None, clusters: Vec::new(), user_nodes: Vec::new(), visibility: 7, motion_blur: 0,
            matte: 1.0, garbage_matte: 0, x404: 0, x406: 0, ud_clusters: [None, None, None],
            ud_arrays: [None, None, None], x420: 0, uv_sets: 1, uv_domains: None,
        }
    }
}

impl Node {
    /// The node, its sons, then its brothers: the order the file stores them in.
    pub fn walk<'a>(&'a self, depth: usize, f: &mut dyn FnMut(usize, &'a Node)) {
        let mut n = Some(self);
        while let Some(x) = n {
            f(depth, x);
            if let Some(s) = &x.son {
                s.walk(depth + 1, f);
            }
            n = x.brother.as_deref();
        }
    }
}
