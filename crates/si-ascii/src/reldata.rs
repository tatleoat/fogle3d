// SPDX-License-Identifier: MIT
//! Relation data: the `RELDATA … EndOfRELDATA` part of a relation line in a scene description
//! (finding 0006).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `physicalio.c` (reader and writer), `a_utilio.c`
//! (quoted strings), and the allocators of the subsystems that own each record.
//!
//! What a relation carries depends on its relation id. The original allocates one C structure
//! per kind and the reader stores each value at a fixed offset in it; the writer prints the
//! same offsets back. Most kinds are a list of keywords, each followed by a fixed number of
//! values, so they are described here by tables ([`Layout`]) and kept as the structure's
//! little-endian byte image ([`Record`]): that is exactly what the original leaves in memory,
//! whatever the file contained.

use crate::cnum::CLib;
use crate::token::AsciiFile;

/// Relation ids with a reader or writer branch of their own.
pub const REL_CNS_POS_LIM: i32 = 0x66; // 102
pub const REL_CNS_ROT_LIM: i32 = 0x67; // 103
pub const REL_CNS_DIR: i32 = 0x74; // 116
pub const REL_SYMMETRY: i32 = 0x96; // 150
pub const REL_TRAJECTORY: i32 = 0xa0; // 160
pub const REL_QUICK_STRETCH: i32 = 0xef; // 239
pub const REL_BOOL: i32 = 0xf5; // 245
pub const REL_CLUSTER_CENTRE: i32 = 0xf7; // 247
pub const REL_EXTRUSION: i32 = 0xfa; // 250
pub const REL_REVOLUTION: i32 = 0xfb; // 251
pub const REL_SKINNING: i32 = 0xfc; // 252
pub const REL_FOUR_SIDED: i32 = 0xfd; // 253
pub const REL_ZIP: i32 = 0xff; // 255
pub const REL_EXTRACT: i32 = 0x100; // 256
pub const REL_TRIMMING: i32 = 0x101; // 257
pub const REL_SHADER_ARRAY: i32 = 0x227; // 551
pub const REL_DYNAMICS: i32 = 0x320; // 800
pub const REL_DYN_OBST: i32 = 0x327; // 807
pub const REL_NAMED_SELECTION: i32 = 10000;
pub const REL_CUSTOM: i32 = 20000;
pub const REL_EXP: i32 = 20010;

/// The type of one field of a record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ty {
    /// `short`: the low 16 bits of `atoi`; printed sign-extended with `%d`.
    I16,
    /// `int`: `atoi`; printed with `%d`.
    I32,
    /// `float`: `(float)atof`; printed with `%.7g`.
    F32,
    /// `double`: `atof`; printed with `%.7g` after a conversion to `float`.
    F64,
}

/// A keyword and the fields its values go to, in the order they follow the keyword.
pub struct Keyword {
    pub name: &'static str,
    pub fields: &'static [(Ty, u16)],
}

/// One kind of record: its size, the non-zero values its allocator sets, and its keywords in
/// the order the writer prints them. The reader accepts them in any order and any number of
/// times, and skips a token it does not know.
pub struct Layout {
    pub relid: i32,
    pub size: usize,
    pub init: &'static [(Ty, u16, f64)],
    pub keywords: &'static [Keyword],
}

use Ty::{F32, F64, I16, I32};

macro_rules! kw {
    ($name:literal, $($t:ident @ $off:literal),+) => {
        Keyword { name: $name, fields: &[$(($t, $off)),+] }
    };
}

/// Relation 102: A zeroed 0x34-byte record.
static POSITION_LIMIT: Layout = Layout {
    relid: REL_CNS_POS_LIM,
    size: 0x34,
    init: &[],
    keywords: &[
        kw!("MIN_LIMIT", F32 @ 0x0, F32 @ 0x4, F32 @ 0x8),
        kw!("MAX_LIMIT", F32 @ 0xc, F32 @ 0x10, F32 @ 0x14),
        kw!("MIN_ACTIVE", I16 @ 0x18, I16 @ 0x1a, I16 @ 0x1c),
        kw!("MAX_ACTIVE", I16 @ 0x1e, I16 @ 0x20, I16 @ 0x22),
        kw!("DAMPING", F32 @ 0x24, F32 @ 0x28),
        kw!("IS_REF_WORLD", I16 @ 0x2c),
        kw!("IS_AREA_BOX", I16 @ 0x2e),
        kw!("LIMIT_RADIUS", F32 @ 0x30),
    ],
};

/// Relation 103: A zeroed 0x44-byte record. The six floats at 0x24–0x38 are not in the file: the reader
/// fills them from the limits (see [`rotation_limit_radians`]).
static ROTATION_LIMIT: Layout = Layout {
    relid: REL_CNS_ROT_LIM,
    size: 0x44,
    init: &[],
    keywords: &[
        kw!("MIN_LIMIT", F32 @ 0x0, F32 @ 0x4, F32 @ 0x8),
        kw!("MAX_LIMIT", F32 @ 0xc, F32 @ 0x10, F32 @ 0x14),
        kw!("MIN_ACTIVE", I16 @ 0x18, I16 @ 0x1a, I16 @ 0x1c),
        kw!("MAX_ACTIVE", I16 @ 0x1e, I16 @ 0x20, I16 @ 0x22),
        kw!("DAMPING", F32 @ 0x3c, F32 @ 0x40),
    ],
};

/// Relation 116: Two zeroed bytes.
static LIMIT_REACHED: Layout =
    Layout { relid: REL_CNS_DIR, size: 2, init: &[], keywords: &[kw!("LIMIT_REACHED", I16 @ 0x0)] };

/// Relation 150: 0x30 bytes; the reader creates it with [`symmetry_create`] and nothing set.
static SYMMETRY: Layout = Layout {
    relid: REL_SYMMETRY,
    size: 0x30,
    init: &[],
    keywords: &[
        kw!("PLANE", I16 @ 0x0),
        kw!("REF", I16 @ 0x8),
        kw!("HCLEN", I16 @ 0x2),
        kw!("FSTRT", I16 @ 0x4),
        kw!("FDURN", I16 @ 0x6),
        kw!("INIT_LROT", F32 @ 0xc, F32 @ 0x10, F32 @ 0x14),
        kw!("INIT_GROT", F32 @ 0x18, F32 @ 0x1c, F32 @ 0x20),
        kw!("INIT_AROT", F32 @ 0x24, F32 @ 0x28, F32 @ 0x2c),
    ],
};

/// Relation 160: Four zeroed bytes.
static FIRST_FRAME: Layout =
    Layout { relid: REL_TRAJECTORY, size: 4, init: &[], keywords: &[kw!("FIRST_FRAME", I32 @ 0x0)] };

/// Relation 239: 0x128 bytes: twelve 0x18-byte entries (mode at +0, option at +4, three constraint
/// flags at +6, sensitivity at +0xc, maximum deformation at +0x10) and three shorts. Each
/// keyword lists one value per entry, in the entry order 0 1 2 6 7 8 3 4 5 9 10 11.
/// The initial values are the ones of the program's default record (`dfltQSData`,
/// 0x086f6ba0) in the fields the keywords name; see [`quick_stretch_create`] (extracted with
/// `re/tools/si-ascii/qs_defaults.py`).
static QUICK_STRETCH: Layout = Layout {
    relid: REL_QUICK_STRETCH,
    size: 0x128,
    init: &[
        (I16, 0x0, 1.0), (I16, 0x90, 1.0), (I16, 0xc4, 1.0), (I16, 0x10c, 1.0), (I16, 0x120, 1.0),
        (F32, 0xc, 0.005), (F32, 0x24, 0.005), (F32, 0x3c, 0.005), (F32, 0x9c, 0.005), (F32, 0xb4, 0.005), (F32, 0xcc, 0.005),
        (F32, 0x54, 0.0005), (F32, 0x6c, 0.0005), (F32, 0x84, 0.0005), (F32, 0xe4, 0.0005), (F32, 0xfc, 0.0005), (F32, 0x114, 0.0005),
        (F32, 0x10, 100.0), (F32, 0x28, 100.0), (F32, 0x40, 100.0), (F32, 0xa0, 100.0), (F32, 0xb8, 100.0), (F32, 0xd0, 100.0),
        (F32, 0x58, 1000.0), (F32, 0x70, 1000.0), (F32, 0x88, 1000.0), (F32, 0xe8, 1000.0), (F32, 0x100, 1000.0), (F32, 0x118, 1000.0),
    ],
    keywords: &[
        kw!("MODE", I16 @ 0x0, I16 @ 0x18, I16 @ 0x30, I16 @ 0x90, I16 @ 0xa8, I16 @ 0xc0, I16 @ 0x48, I16 @ 0x60, I16 @ 0x78, I16 @ 0xd8, I16 @ 0xf0, I16 @ 0x108),
        kw!("SENSITIVITY", F32 @ 0xc, F32 @ 0x24, F32 @ 0x3c, F32 @ 0x9c, F32 @ 0xb4, F32 @ 0xcc, F32 @ 0x54, F32 @ 0x6c, F32 @ 0x84, F32 @ 0xe4, F32 @ 0xfc, F32 @ 0x114),
        kw!("OPTION", I16 @ 0x4, I16 @ 0x1c, I16 @ 0x34, I16 @ 0x94, I16 @ 0xac, I16 @ 0xc4, I16 @ 0x4c, I16 @ 0x64, I16 @ 0x7c, I16 @ 0xdc, I16 @ 0xf4, I16 @ 0x10c),
        kw!("CONSTRAINT",
            I16 @ 0x6, I16 @ 0x8, I16 @ 0xa, I16 @ 0x1e, I16 @ 0x20, I16 @ 0x22, I16 @ 0x36, I16 @ 0x38, I16 @ 0x3a,
            I16 @ 0x96, I16 @ 0x98, I16 @ 0x9a, I16 @ 0xae, I16 @ 0xb0, I16 @ 0xb2, I16 @ 0xc6, I16 @ 0xc8, I16 @ 0xca,
            I16 @ 0x4e, I16 @ 0x50, I16 @ 0x52, I16 @ 0x66, I16 @ 0x68, I16 @ 0x6a, I16 @ 0x7e, I16 @ 0x80, I16 @ 0x82,
            I16 @ 0xde, I16 @ 0xe0, I16 @ 0xe2, I16 @ 0xf6, I16 @ 0xf8, I16 @ 0xfa, I16 @ 0x10e, I16 @ 0x110, I16 @ 0x112),
        kw!("MAXDEFORM", F32 @ 0x10, F32 @ 0x28, F32 @ 0x40, F32 @ 0xa0, F32 @ 0xb8, F32 @ 0xd0, F32 @ 0x58, F32 @ 0x70, F32 @ 0x88, F32 @ 0xe8, F32 @ 0x100, F32 @ 0x118),
        kw!("PARENT_OVER", I16 @ 0x120),
        kw!("AUTO_SWING", I16 @ 0x122),
        kw!("WIND_SENSITIVE", I16 @ 0x124),
    ],
};

/// Relation 250 [soft 0x08156830 SI_extrusionDataAllocate]. The reader matches `STEP` before
/// `SUBDIVISION` and so on; the order here is the writer's.
static EXTRUSION: Layout = Layout {
    relid: REL_EXTRUSION,
    size: 0x38,
    init: &[
        (F32, 0x4, 1.0), (I32, 0x8, 5.0), (I32, 0xc, 5.0), (I32, 0x10, 2.0), (I16, 0x1e, 3.0), (F32, 0x20, 0.25),
        (I16, 0x24, 1.0), (I16, 0x26, 1.0), (I16, 0x28, 1.0), (I32, 0x34, 3.0),
    ],
    keywords: &[
        kw!("SIMPLE", I32 @ 0x10),
        kw!("CLOSE", I16 @ 0x0),
        kw!("DEPTH", F32 @ 0x4),
        kw!("STEP", I32 @ 0x8),
        kw!("SUBDIVISION", I32 @ 0xc),
        kw!("IS_PATCH", I16 @ 0x14),
        kw!("SPL_TYPE", I32 @ 0x18),
        kw!("BEVEL", I16 @ 0x1c),
        kw!("NB_BEVEL", I16 @ 0x1e),
        kw!("RADIUS", F32 @ 0x20),
        kw!("BEVEL_TOP", I16 @ 0x24),
        kw!("BEVEL_SIDE", I16 @ 0x26),
        kw!("BEVEL_BOTTOM", I16 @ 0x28),
        kw!("EXTRU_STEP", I32 @ 0x34),
    ],
};

/// Relation 251 [soft 0x08195ab0 SI_revolutionDataAllocate]
static REVOLUTION: Layout = Layout {
    relid: REL_REVOLUTION,
    size: 0x20,
    init: &[(F32, 0x4, 1.0), (I32, 0x8, 5.0), (I32, 0xc, 1.0), (I32, 0x10, 5.0), (I32, 0x18, 1.0), (I32, 0x1c, 3.0)],
    keywords: &[
        kw!("CLOSE", I16 @ 0x0),
        kw!("DEGREE", F32 @ 0x4),
        kw!("STEP", I32 @ 0x8),
        kw!("AXIS", I32 @ 0xc),
        kw!("SUBDIVISION", I32 @ 0x10),
        kw!("IS_PATCH", I16 @ 0x14),
        kw!("SPL_TYPE", I32 @ 0x18),
        kw!("REVOL_SUB", I32 @ 0x1c),
    ],
};

/// Relation 252 [soft 0x08142330 SI_skinningDataAllocate]
static SKINNING: Layout = Layout {
    relid: REL_SKINNING,
    size: 0x1c,
    init: &[(I32, 0x8, 5.0), (I32, 0x18, 3.0)],
    keywords: &[
        kw!("CLOSE", I16 @ 0x0),
        kw!("NB_SPLINE", I32 @ 0x4),
        kw!("CAP", I16 @ 0xc),
        kw!("SUBDIVISION", I32 @ 0x8),
        kw!("IS_PATCH", I16 @ 0xe),
        kw!("SPL_TYPE", I32 @ 0x10),
        kw!("NRB_UNIFORMITY", I16 @ 0x14),
        kw!("NRB_TYPE", I16 @ 0x16),
        kw!("STEP", I32 @ 0x18),
    ],
};

/// Relation 253 [soft 0x0819b440 SI_fourSidedDataAllocate]
static FOUR_SIDED: Layout =
    Layout { relid: REL_FOUR_SIDED, size: 4, init: &[], keywords: &[kw!("PARAMETRIZATION", I32 @ 0x0)] };

/// Relation 255: A zeroed 0x1c-byte record.
static ZIP: Layout = Layout {
    relid: REL_ZIP,
    size: 0x1c,
    init: &[],
    keywords: &[
        kw!("INDICE_SUP", I32 @ 0x0),
        kw!("INDICE_ZIP", I32 @ 0x4),
        kw!("U_SUP", I16 @ 0x8),
        kw!("U_ZIP", I16 @ 0xa),
        kw!("PT_DW", I32 @ 0xc),
        kw!("PT_UP", I32 @ 0x10),
        kw!("RW_DW", I32 @ 0x14),
        kw!("RW_UP", I32 @ 0x18),
    ],
};

/// Relation 256 [soft 0x0814e0fc SI_extractDataAllocate]
static EXTRACT: Layout = Layout {
    relid: REL_EXTRACT,
    size: 0x18,
    init: &[(I16, 0x0, 1.0)],
    keywords: &[
        kw!("IN_U_DIR", I16 @ 0x0),
        kw!("IS_ISOCURVE", I32 @ 0x4),
        kw!("LOCATION", F64 @ 0x8),
        kw!("HOLE_NO", I32 @ 0x10),
        kw!("TRIM_NO", I32 @ 0x14),
    ],
};

/// Relation 257 [soft 0x084fca40 SI_trimmingDataAllocate]: the parallel axis starts as the Z axis, the
/// perspective point as the origin. `PROJ_CURVE` is not in this table: its values go to a
/// separate array ([`Record::proj_curves`]).
static TRIMMING: Layout = Layout {
    relid: REL_TRIMMING,
    size: 0x48,
    init: &[(F64, 0x14, 1.0), (I32, 0x34, 2.0), (I32, 0x44, 1.0)],
    keywords: &[
        kw!("PROJ_METHOD", I32 @ 0x0),
        kw!("PARA_AXIS", F64 @ 0x4, F64 @ 0xc, F64 @ 0x14),
        kw!("PERSP_AXIS", F64 @ 0x1c, F64 @ 0x24, F64 @ 0x2c),
        kw!("COORDI", I32 @ 0x34),
        kw!("TRIM_REGION", I32 @ 0x38),
        kw!("NB_PROJ_CURVE", I32 @ 0x3c),
        // PROJ_CURVE goes here in the writer's order
        kw!("SURF_OPERATOR", I32 @ 0x44),
    ],
};

/// Offset of the projection-curve count in a trimming record.
const TRIM_NB_PROJ_CURVE: usize = 0x3c;

static LAYOUTS: [&Layout; 13] = [
    &POSITION_LIMIT, &ROTATION_LIMIT, &LIMIT_REACHED, &SYMMETRY, &FIRST_FRAME, &QUICK_STRETCH, &EXTRUSION,
    &REVOLUTION, &SKINNING, &FOUR_SIDED, &ZIP, &EXTRACT, &TRIMMING,
];

/// The layout of the keyword record a relation id carries, if it carries one.
pub fn layout(relid: i32) -> Option<&'static Layout> {
    LAYOUTS.iter().copied().find(|l| l.relid == relid)
}

/// A keyword record: the byte image of the original's structure.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub relid: i32,
    /// Little-endian image, `layout.size` bytes.
    pub image: Vec<u8>,
    /// Trimming only: the array `PROJ_CURVE` fills (a separate allocation in the original, its
    /// pointer at 0x40). `None` until a `PROJ_CURVE` keyword with a positive count is read.
    pub proj_curves: Option<Vec<i16>>,
}

/// One value of a record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    I16(i16),
    I32(i32),
    F32(f32),
    F64(f64),
}

impl Record {
    /// A record as its allocator leaves it.
    pub fn new(l: &'static Layout) -> Record {
        let mut r = Record { relid: l.relid, image: vec![0u8; l.size], proj_curves: None };
        for &(t, off, v) in l.init {
            r.set(t, off as usize, v);
        }
        r
    }

    pub fn layout(&self) -> &'static Layout {
        layout(self.relid).expect("a record is only built from a layout")
    }

    /// Stores a number the way the reader stores a converted token.
    fn set(&mut self, t: Ty, off: usize, v: f64) {
        match t {
            I16 => self.image[off..off + 2].copy_from_slice(&(v as i32 as i16).to_le_bytes()),
            I32 => self.image[off..off + 4].copy_from_slice(&(v as i32).to_le_bytes()),
            F32 => self.image[off..off + 4].copy_from_slice(&(v as f32).to_le_bytes()),
            F64 => self.image[off..off + 8].copy_from_slice(&v.to_le_bytes()),
        }
    }

    pub fn get(&self, t: Ty, off: usize) -> Value {
        let b = &self.image;
        match t {
            I16 => Value::I16(i16::from_le_bytes([b[off], b[off + 1]])),
            I32 => Value::I32(i32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])),
            F32 => Value::F32(f32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])),
            F64 => Value::F64(f64::from_le_bytes(b[off..off + 8].try_into().unwrap())),
        }
    }

    /// The values that follow `keyword`, in file order.
    pub fn values(&self, keyword: &str) -> Option<Vec<Value>> {
        let k = self.layout().keywords.iter().find(|k| k.name == keyword)?;
        Some(k.fields.iter().map(|&(t, off)| self.get(t, off as usize)).collect())
    }

    fn i32_at(&self, off: usize) -> i32 {
        match self.get(I32, off) {
            Value::I32(v) => v,
            _ => unreachable!(),
        }
    }
}

/// A symmetry record [soft 0x082436d4 SI_symmetryCreateData]: zeroed, then the four numbers
/// (stored as shorts at 0, 2, 4, 6: the fields `PLANE`, `HCLEN`, `FSTRT`, `FDURN` name), the
/// field at 8 (`REF`) cleared, and the three vectors that are given (`INIT_LROT`,
/// `INIT_GROT`, `INIT_AROT`). The relation data reader passes zeros and no vector.
pub fn symmetry_create(plane: i32, hclen: i32, fstrt: i32, fdurn: i32, vectors: [Option<[f32; 3]>; 3]) -> Record {
    let mut r = Record { relid: REL_SYMMETRY, image: vec![0u8; SYMMETRY.size], proj_curves: None };
    for (off, v) in [(0usize, plane), (2, hclen), (4, fstrt), (6, fdurn), (8, 0)] {
        r.set(I16, off, v as f64);
    }
    for (k, v) in vectors.iter().enumerate() {
        if let Some(v) = v {
            for (i, x) in v.iter().enumerate() {
                r.set(F32, 0xc + 12 * k + 4 * i, *x as f64);
            }
        }
    }
    r
}

/// Copies a quick-stretch record field by field [soft 0x085a24f0 SI_QuickStretchCopyData]:
/// the 87 fields the keywords name and nothing else (the bytes between them are left alone).
/// With no source the values come from the program's default record.
pub fn quick_stretch_copy(dst: &mut Record, src: Option<&Record>) {
    let default = Record::new(&QUICK_STRETCH);
    let src = src.unwrap_or(&default);
    for k in QUICK_STRETCH.keywords {
        for &(t, off) in k.fields {
            let (off, n) = (off as usize, if t == I16 { 2 } else { 4 });
            dst.image[off..off + n].copy_from_slice(&src.image[off..off + n]);
        }
    }
}

/// A new quick-stretch record [soft 0x085a24bc SI_QuickStretchCreateData]: zeroed, then
/// copied from `src` or from the defaults.
pub fn quick_stretch_create(src: Option<&Record>) -> Record {
    let mut r = Record { relid: REL_QUICK_STRETCH, image: vec![0u8; QUICK_STRETCH.size], proj_curves: None };
    quick_stretch_copy(&mut r, src);
    r
}

/// Copies a 4 × 4 matrix, source first [soft 0x08516224 SI_matrixCopy].
pub fn matrix_copy(src: &[[f32; 4]; 4], dst: &mut [[f32; 4]; 4]) {
    *dst = *src;
}

/// The identity matrix (`SI_identite`, 0x086dc160).
pub const IDENTITY: [[f32; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// A boolean-operation relation (245): 0x14 bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimBool {
    /// +0. The allocator sets 1; the reader clears it and sets it again on `ACTIVE`.
    pub active: i16,
    /// +4. 1 `union` (the allocator's value), 2 `intersection`, 3 `difference`.
    pub operation: i32,
    /// +8. 0 `none`, 1 `master`, 2 `slave`, 3 `respective`.
    pub material: i32,
    /// +0xc, +0xe, +0x10
    pub remove_edges: i16,
    pub remove_vertices: i16,
    pub material_animated: i16,
}

impl AnimBool {
    /// A new record [soft 0x0830a770 SI_animBoolAllocate]: active, operation `union`,
    /// material `none`, no flag.
    pub fn new() -> AnimBool {
        AnimBool { active: 1, operation: 1, material: 0, remove_edges: 0, remove_vertices: 0, material_animated: 0 }
    }
}

impl Default for AnimBool {
    fn default() -> Self {
        AnimBool::new()
    }
}

/// A custom-effect relation (20000): 0x28 zeroed bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Custom {
    /// +0. Not in the file: the element index of the effect's icon, looked up from `icon_id`
    /// by the relation reader.
    pub icon_elem: i32,
    /// +4. Third value: the id of the effect's icon among the scene's models, kept as text.
    pub icon_id: Option<Vec<u8>>,
    /// +8, +0xc. First and second values. The writer prints the second as the relation's
    /// sub-definition number.
    pub value_08: i32,
    pub value_0c: i32,
    /// +0x10, +0x14. Fourth and fifth values: handed to the dictionary as the two attribute
    /// groups of the relation.
    pub group_10: i32,
    pub group_14: i32,
    /// +0x18 `ROCC`, +0x1c `AOCC`
    pub rocc: i32,
    pub aocc: i32,
    /// +0x20 `ENAME`, +0x24 `MNAME`: the effect's name and its menu.
    pub ename: Option<Vec<u8>>,
    pub mname: Option<Vec<u8>>,
}

/// An expression relation (20010): 0x14 zeroed bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Expression {
    /// +0. The id of the animated parameter (an fcurve type). If several numbers are listed
    /// the last one stays.
    pub fcurve: i32,
    /// +0x10. Only for parameter 566 (0x236): a name in single quotes.
    pub name: Option<Vec<u8>>,
}

/// A named-selection relation (10000): 0x10 zeroed bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NamedSelection {
    /// +0. The chapter named by the first token, as an index into the dictionary's lists
    /// (−1 if there is no such chapter). Resolved by the caller: see [`ReadCtx`].
    pub chapter: i32,
    pub chapter_name: Vec<u8>,
    /// +4. The element named by the second token: its index in that chapter, −1 if not found.
    pub elem: i32,
    pub elem_id: Vec<u8>,
    /// +8, +0xc. The parameter (an fcurve type) and its rank among the element's fcurves of
    /// that type.
    pub fcurve: i32,
    pub rank: i32,
}

/// The data of one relation.
#[derive(Clone, Debug, PartialEq)]
pub enum RelData {
    Record(Record),
    /// 220, 221 (seven floats), 230, 231 (nine), 800 (sixteen), 807 (one): the values of all
    /// keywords in the order they were read, *not* by keyword. `capacity` is the size the
    /// original allocated; more values than that overran its buffer.
    Floats { relid: i32, capacity: usize, values: Vec<f32> },
    /// 247: a 4 × 4 matrix, rows first; identity where the file has fewer than 16 numbers.
    Matrix([[f32; 4]; 4]),
    AnimBool(AnimBool),
    Custom(Custom),
    Expression(Expression),
    NamedSelection(NamedSelection),
}

/// What the dictionary does for the reader of a named-selection relation (10000): chapter and
/// element lookups. The scene description reader supplies it.
pub trait ReadCtx {
    /// `SI_elemdictFindListByName`
    fn find_list(&self, name: &[u8]) -> i32;
    /// `SI_dbGetElemFromDscTokenId` with scene index 0, returning the element's index (−1 when
    /// there is none).
    fn elem_index(&self, list: i32, token: &[u8]) -> i32;
}

/// Why a read stopped where the original would not have.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stuck {
    /// The file ended inside a loop that only stops at `EndOfRELDATA` (or another end word):
    /// the original reads empty tokens for ever.
    EndlessAtEof,
    /// A loop that makes no progress on this token: the original spins on it for ever.
    EndlessOnToken,
    /// The original would dereference a null pointer here.
    NullDeref,
}

/// The number of floats the generic kinds hold (`elemdictRelDataAllocate`, 0x08111d58, ported and checked in `si-scene`).
fn generic_capacity(relid: i32) -> Option<usize> {
    match relid {
        0xdc | 0xdd => Some(7),
        0xe6 | 0xe7 => Some(9),
        800 => Some(16),
        0x327 => Some(1),
        _ => None,
    }
}

/// How many floats follow a keyword of a generic kind.
fn generic_count(relid: i32, tok: &[u8]) -> usize {
    match relid {
        0xdc | 0xdd | 0xe6 | 0xe7 => match tok {
            b"SCLE" | b"ROTN" | b"TRNS" => 3,
            b"ROLL" => 1,
            _ => 0,
        },
        800 => match tok {
            b"ROOTCPOS" | b"ROOTCORI" => 5,
            b"COLL" | b"WIND" | b"GRAV" => 2,
            _ => 0,
        },
        0x327 => (tok == b"OBST") as usize,
        _ => 0,
    }
}

/// Stops the loops that have no end-of-file test: at the end of the file the token is empty
/// and stays empty.
fn eof_guard(f: &AsciiFile) -> Result<(), Stuck> {
    if f.token().is_empty() && f.only_blanks_left() {
        Err(Stuck::EndlessAtEof)
    } else {
        Ok(())
    }
}

/// Reads the data of a relation, starting after the word `RELDATA`
/// [soft 0x081144e8 SI_elemdictReadRelationData].
///
/// `Ok(None)` is the original's null result: the relation id has no data structure, and the
/// tokens up to `EndOfRELDATA` are skipped. On return the token buffer usually holds
/// `EndOfRELDATA`; the caller reads on from there.
///
/// Every kind but the custom effect loops until it sees `EndOfRELDATA` and never tests for the
/// end of the file; a truncated file is reported as [`Stuck::EndlessAtEof`] with what was
/// read so far lost, where the original would hang.
pub fn read(f: &mut AsciiFile, relid: i32, ctx: &dyn ReadCtx) -> Result<Option<RelData>, Stuck> {
    if let Some(l) = layout(relid) {
        let mut r = match relid {
            REL_SYMMETRY => symmetry_create(0, 0, 0, 0, [None, None, None]),
            REL_QUICK_STRETCH => quick_stretch_create(None),
            _ => Record::new(l),
        };
        f.token_read();
        while !f.is("EndOfRELDATA") {
            eof_guard(f)?;
            if let Some(k) = l.keywords.iter().find(|k| f.is(k.name)) {
                for &(t, off) in k.fields {
                    f.token_read();
                    let v = match t {
                        I16 | I32 => f.atoi() as f64,
                        F32 | F64 => f.atof(),
                    };
                    r.set(t, off as usize, v);
                }
            } else if relid == REL_TRIMMING && f.is("PROJ_CURVE") {
                // a fresh zeroed array of NB_PROJ_CURVE shorts (the previous one is leaked),
                // then that many values
                let n = r.i32_at(TRIM_NB_PROJ_CURVE);
                // The original allocates whatever the count says. Here a count larger than
                // the bytes left in the file cannot be honest: each value needs two bytes.
                let cap = (n.max(0) as usize).min((f.len() - f.tell()) / 2 + 1);
                let mut v = vec![0i16; cap];
                for i in 0..n.max(0) as usize {
                    f.token_read();
                    eof_guard(f)?;
                    if i < v.len() {
                        v[i] = f.atoi() as i16;
                    }
                }
                r.proj_curves = Some(v);
            }
            f.token_read();
        }
        if relid == REL_CNS_ROT_LIM {
            rotation_limit_radians(&mut r);
        }
        return Ok(Some(RelData::Record(r)));
    }
    match relid {
        REL_BOOL => Ok(Some(RelData::AnimBool(read_anim_bool(f)?))),
        REL_CLUSTER_CENTRE => {
            // the identity, then up to sixteen numbers
            let mut m = [[0f32; 4]; 4];
            matrix_copy(&IDENTITY, &mut m);
            f.token_read();
            'rows: for row in m.iter_mut() {
                for v in row.iter_mut() {
                    // the first token is converted whatever it is, EndOfRELDATA included
                    *v = f.atof32();
                    f.token_read();
                    if f.is("EndOfRELDATA") {
                        break 'rows;
                    }
                }
            }
            Ok(Some(RelData::Matrix(m)))
        }
        REL_NAMED_SELECTION => {
            // [soft 0x08347968 SI_nselReadSpecialRelationData]
            let mut n = NamedSelection::default();
            n.chapter_name = f.next_string();
            n.chapter = ctx.find_list(&n.chapter_name);
            n.elem_id = f.next_string();
            n.elem = ctx.elem_index(n.chapter, &n.elem_id);
            n.fcurve = f.next_int();
            n.rank = f.next_int();
            f.token_reach("EndOfRELDATA", b';');
            Ok(Some(RelData::NamedSelection(n)))
        }
        REL_CUSTOM => Ok(Some(RelData::Custom(read_custom(f)))),
        REL_EXP => {
            let mut e = Expression::default();
            f.token_read();
            while !f.is("EndOfRELDATA") {
                eof_guard(f)?;
                e.fcurve = f.atoi();
                if e.fcurve == 0x236 {
                    e.name = read_quoted(f);
                }
                f.token_read();
            }
            Ok(Some(RelData::Expression(e)))
        }
        _ => match generic_capacity(relid) {
            Some(capacity) => {
                let mut values = Vec::new();
                f.token_read();
                while !f.is("EndOfRELDATA") {
                    eof_guard(f)?;
                    for _ in 0..generic_count(relid, f.token()) {
                        // No bound in the original: a file with more values than the kind
                        // holds writes past the allocation, and only afterwards prints
                        // "ERROR: Invalid Relation Data encountered while reading scene".
                        values.push(f.next_f32());
                    }
                    f.token_read();
                }
                Ok(Some(RelData::Floats { relid, capacity, values }))
            }
            None => {
                f.token_reach("EndOfRELDATA", b';');
                Ok(None)
            }
        },
    }
}

/// The reader's last step for a rotation limit (103): the six limits, in degrees in the file,
/// are also kept multiplied by 0.0174532925 (a double constant at 0x08649b78, not π/180
/// exactly) at 0x24–0x38, in the order min x y z, max x y z.
///
/// The original multiplies on the x87 stack and stores a single: the product is rounded to
/// the register's 64-bit mantissa first and to 24 bits second. [`x87_mul_to_f32`] does the
/// same two roundings.
fn rotation_limit_radians(r: &mut Record) {
    for i in 0..6 {
        let deg = match r.get(F32, i * 4) {
            Value::F32(v) => v,
            _ => unreachable!(),
        };
        let rad = x87_mul_to_f32(0.017_453_292_5, deg);
        r.image[0x24 + i * 4..0x28 + i * 4].copy_from_slice(&rad.to_le_bytes());
    }
}

/// `(float)((long double)a * b)` as the x87 unit computes it with its default 64-bit
/// precision: the exact product rounded to nearest-even at 64 bits, then at 24.
pub fn x87_mul_to_f32(a: f64, b: f32) -> f32 {
    let direct = (a * b as f64) as f32;
    if a == 0.0 || b == 0.0 || !a.is_finite() || !b.is_finite() || a.is_subnormal() || b.is_subnormal() {
        return direct;
    }
    // integer mantissas: a = ma × 2^ea (53 bits), b = mb × 2^eb (24 bits)
    let (ab, bb) = (a.to_bits(), (b as f64).to_bits());
    let ma = (ab & ((1u64 << 52) - 1)) | (1u64 << 52);
    let mb = (bb & ((1u64 << 52) - 1)) | (1u64 << 52);
    let ea = ((ab >> 52) & 0x7ff) as i32 - 1075;
    let eb = ((bb >> 52) & 0x7ff) as i32 - 1075;
    let neg = (ab >> 63) != (bb >> 63);
    let p = ma as u128 * mb as u128; // exact, up to 106 bits
    let top = 127 - p.leading_zeros() as i32;
    // first rounding: keep 64 bits
    let (mut m64, mut e) = if top >= 64 {
        let drop = (top - 63) as u32;
        let q = (p >> drop) as u64;
        let rem = p & ((1u128 << drop) - 1);
        let half = 1u128 << (drop - 1);
        let up = rem > half || (rem == half && q & 1 == 1);
        (q as u128 + up as u128, ea + eb + drop as i32)
    } else {
        (p, ea + eb)
    };
    if m64 >> 64 != 0 {
        m64 >>= 1;
        e += 1;
    }
    // second rounding: keep 24 bits
    let top = 127 - m64.leading_zeros() as i32;
    let exp = top + e;
    if !(-126..=127).contains(&exp) {
        return direct; // outside the single's normal range: not a case the limits reach
    }
    let drop = (top - 23).max(0) as u32;
    let mut q = (m64 >> drop) as u64;
    if drop > 0 {
        let rem = m64 & ((1u128 << drop) - 1);
        let half = 1u128 << (drop - 1);
        if rem > half || (rem == half && q & 1 == 1) {
            q += 1;
        }
    }
    let v = q as f32 * 2f32.powi(e + drop as i32);
    if neg {
        -v
    } else {
        v
    }
}

/// [soft 0x0811e56c animBoolRead]
fn read_anim_bool(f: &mut AsciiFile) -> Result<AnimBool, Stuck> {
    // the allocator's values, then the reader's own reset of the first field
    let mut b = AnimBool::new();
    b.active = 0;
    f.token_read();
    while !f.is("EndOfRELDATA") {
        eof_guard(f)?;
        if f.is("ACTIVE") {
            b.active = 1;
        } else if f.is("OPERATION") {
            f.token_read();
            match f.token() {
                b"union" => b.operation = 1,
                b"intersection" => b.operation = 2,
                b"difference" => b.operation = 3,
                _ => {}
            }
        } else if f.is("MATERIAL") {
            f.token_read();
            match f.token() {
                b"none" => b.material = 0,
                b"master" => b.material = 1,
                b"slave" => b.material = 2,
                b"respective" => b.material = 3,
                _ => {}
            }
        } else if f.is("REMOVE_EDGES") {
            b.remove_edges = 1;
        } else if f.is("REMOVE_VERTICES") {
            b.remove_vertices = 1;
        } else if f.is("MATERIAL_ANIMATED") {
            b.material_animated = 1;
        }
        f.token_read();
    }
    Ok(b)
}

/// The custom-effect branch of the reader: five positional values, then keywords. Unlike the
/// other kinds this one tests every read, so it stops cleanly at the end of the file: a value
/// is stored only when its read succeeded, and nothing more is read after a failure.
fn read_custom(f: &mut AsciiFile) -> Custom {
    let mut c = Custom::default();
    // the first two reads are made unconditionally
    if f.token_read() {
        c.value_08 = f.atoi();
    }
    let mut ok = f.token_read();
    if ok {
        c.value_0c = f.atoi();
    }
    if ok {
        ok = f.token_read();
        if ok {
            c.icon_id = Some(f.token().to_vec());
            ok = f.token_read();
            if ok {
                c.group_10 = f.atoi();
                ok = f.token_read();
                if ok {
                    c.group_14 = f.atoi();
                }
            }
        }
    }
    if ok {
        ok = f.token_read();
    }
    while ok && !f.is("EndOfRELDATA") {
        if f.is("ENAME") {
            ok = f.token_read();
            if ok {
                c.ename = Some(f.token().to_vec());
            }
        } else if f.is("MNAME") {
            ok = f.token_read();
            if ok {
                c.mname = Some(f.token().to_vec());
            }
        } else if f.is("AOCC") {
            ok = f.token_read();
            if ok {
                c.aocc = f.atoi();
            }
        } else if f.is("ROCC") {
            ok = f.token_read();
            if ok {
                c.rocc = f.atoi();
            }
        }
        if ok {
            ok = f.token_read();
        }
    }
    c
}

/// A string in single quotes [soft 0x083dc604 si_stringAsciiRead].
///
/// Spaces, tabs and newlines are skipped (not carriage returns); the next byte is taken as
/// the opening quote *whatever it is*; bytes are then copied up to the next `'` or the end of
/// the file. The result is what lies between the first byte and the last one. `None` (a null
/// pointer in the original) for an empty string `''` and when the file ends before a first
/// byte.
///
/// The original collects into a global buffer and clamps the write position at 238, so only
/// the first 237 bytes of a longer string survive. It measures with `strlen`, so a NUL byte
/// inside the quotes cuts the result one byte before it.
pub fn read_quoted(f: &mut AsciiFile) -> Option<Vec<u8>> {
    let first = loop {
        let c = f.getc();
        if c == -1 {
            return None;
        }
        if !(c == 0x20 || c == 9 || c == 0xa) {
            break c as u8;
        }
    };
    let mut buf = vec![first];
    let mut i = 1usize;
    let last = loop {
        let c = f.getc();
        if c == -1 || c == 0x27 {
            break c as u8; // the end of the file is stored as the byte 0xff
        }
        if i < buf.len() {
            buf[i] = c as u8;
        } else {
            buf.push(c as u8);
        }
        i = (i + 1).min(0xee);
    };
    buf.truncate(i);
    buf.push(last);
    if buf[0] == 0x27 && buf[1] == 0x27 {
        return None;
    }
    // strlen of the buffer, then: copy from the second byte, cut at strlen − 2
    let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    if n < 2 {
        // strlen 1 makes the original write its terminator one byte before its allocation,
        // strlen 0 cannot happen (the first byte is never NUL here unless the file has one:
        // then it copies the whole tail and cuts two bytes before the buffer). Both are
        // given as an empty string.
        return Some(Vec::new());
    }
    Some(buf[1..n - 1].to_vec())
}

fn put_word(out: &mut Vec<u8>, w: &str) {
    out.extend_from_slice(w.as_bytes());
    out.push(b' ');
}

fn put_int(out: &mut Vec<u8>, v: i32) {
    put_word(out, &v.to_string());
}

fn put_f32(out: &mut Vec<u8>, lib: CLib, v: f32) {
    put_word(out, &lib.fmt_g(v as f64, 7));
}

/// `(int)v` as the writer converts a float it prints with `%d`: the x87 `fistp` with the
/// rounding mode set to truncate, which gives 0x80000000 for a value that does not fit and
/// for a NaN.
fn x87_trunc(v: f32) -> i32 {
    let t = (v as f64).trunc();
    if t.is_nan() || !(-2147483648.0..=2147483647.0).contains(&t) {
        i32::MIN
    } else {
        t as i32
    }
}

/// `printf("%s ", p)` where `p` may be a null pointer: glibc prints `(null)`.
fn put_str(out: &mut Vec<u8>, s: &Option<Vec<u8>>) {
    match s {
        Some(s) => out.extend_from_slice(s),
        None => out.extend_from_slice(b"(null)"),
    }
    out.push(b' ');
}

/// Writes the data of a relation, `RELDATA` to `EndOfRELDATA`, each word followed by a space
/// [soft 0x081120e8 SI_elemdictWriteRelationData].
///
/// The original writes nothing unless the relation's first chapter is MODELS or the relation
/// is a custom effect or an expression (`first_is_models`), and nothing for a named
/// selection, which the relation writer handles itself ([`write_named_selection`]).
///
/// For the generic kinds the original prints its fixed number of floats; a record that holds
/// fewer (the file listed fewer) is completed with zeros, as the zeroed allocation would be.
/// After printing a generic kind the original frees the data and clears the relation's
/// pointer (it rebuilds the data from the scene at the next save); this writer leaves the
/// structure alone.
pub fn write(out: &mut Vec<u8>, lib: CLib, first_is_models: bool, relid: i32, data: &RelData) {
    if !first_is_models && relid != REL_EXP && relid != REL_CUSTOM {
        return;
    }
    match data {
        RelData::Record(r) => {
            put_word(out, "RELDATA");
            for k in r.layout().keywords {
                if r.relid == REL_TRIMMING && k.name == "SURF_OPERATOR" {
                    put_word(out, "PROJ_CURVE");
                    let n = r.i32_at(TRIM_NB_PROJ_CURVE).max(0) as usize;
                    // the original prints NB_PROJ_CURVE entries of its array (a null pointer
                    // if PROJ_CURVE was never read: it would fault); missing ones print as 0
                    for i in 0..n.min(r.proj_curves.as_ref().map_or(0, |v| v.len())) {
                        put_int(out, r.proj_curves.as_ref().unwrap()[i] as i32);
                    }
                }
                put_word(out, k.name);
                for &(t, off) in k.fields {
                    match r.get(t, off as usize) {
                        Value::I16(v) => put_int(out, v as i32),
                        Value::I32(v) => put_int(out, v),
                        Value::F32(v) => put_f32(out, lib, v),
                        Value::F64(v) => put_f32(out, lib, v as f32),
                    }
                }
            }
            put_word(out, "EndOfRELDATA");
        }
        RelData::Floats { relid, values, .. } => {
            let v = |i: usize| values.get(i).copied().unwrap_or(0.0);
            put_word(out, "RELDATA");
            let groups: &[(&str, usize)] = match *relid {
                0xdc | 0xdd => &[("SCLE", 3), ("ROLL", 1), ("TRNS", 3)],
                0xe6 | 0xe7 => &[("SCLE", 3), ("ROTN", 3), ("TRNS", 3)],
                800 => &[("ROOTCPOS", 5), ("ROOTCORI", 5), ("COLL", 2), ("WIND", 2), ("GRAV", 2)],
                _ => &[("OBST", 1)],
            };
            // dynamics and obstacle values are stored as floats but printed as integers
            let as_int = *relid == REL_DYNAMICS || *relid == REL_DYN_OBST;
            let mut i = 0;
            for &(name, n) in groups {
                put_word(out, name);
                for _ in 0..n {
                    if as_int {
                        put_int(out, x87_trunc(v(i)));
                    } else {
                        put_f32(out, lib, v(i));
                    }
                    i += 1;
                }
            }
            put_word(out, "EndOfRELDATA");
        }
        RelData::Matrix(m) => {
            put_word(out, "RELDATA");
            for row in m {
                for &v in row {
                    put_f32(out, lib, v);
                }
            }
            put_word(out, "EndOfRELDATA");
        }
        RelData::AnimBool(b) => write_anim_bool(out, b),
        RelData::Custom(c) => {
            put_word(out, "RELDATA");
            put_int(out, c.value_08);
            put_int(out, c.value_0c);
            put_str(out, &c.icon_id);
            put_int(out, c.group_10);
            put_int(out, c.group_14);
            put_word(out, "AOCC");
            put_int(out, c.aocc);
            put_word(out, "ROCC");
            put_int(out, c.rocc);
            put_word(out, "ENAME");
            put_str(out, &c.ename);
            put_word(out, "MNAME");
            put_str(out, &c.mname);
            put_word(out, "EndOfRELDATA");
        }
        RelData::Expression(e) => {
            put_word(out, "RELDATA");
            put_int(out, e.fcurve);
            if e.fcurve == 0x236 {
                out.push(b'\'');
                match &e.name {
                    Some(n) if !n.is_empty() => out.extend_from_slice(n),
                    _ => out.extend_from_slice(b"_UNKNOWN_"),
                }
                out.extend_from_slice(b"' ");
            }
            put_word(out, "EndOfRELDATA");
        }
        RelData::NamedSelection(_) => {}
    }
}

/// [soft 0x0811e3d0 animBoolWrite]: flags are words that are present or absent; an operation
/// or material value outside its range prints the keyword with nothing after it.
fn write_anim_bool(out: &mut Vec<u8>, b: &AnimBool) {
    put_word(out, "RELDATA");
    if b.active != 0 {
        put_word(out, "ACTIVE");
    }
    put_word(out, "OPERATION");
    match b.operation {
        1 => put_word(out, "union"),
        2 => put_word(out, "intersection"),
        3 => put_word(out, "difference"),
        _ => {}
    }
    put_word(out, "MATERIAL");
    match b.material {
        0 => put_word(out, "none"),
        1 => put_word(out, "master"),
        2 => put_word(out, "slave"),
        3 => put_word(out, "respective"),
        _ => {}
    }
    if b.remove_edges != 0 {
        put_word(out, "REMOVE_EDGES");
    }
    if b.remove_vertices != 0 {
        put_word(out, "REMOVE_VERTICES");
    }
    if b.material_animated != 0 {
        put_word(out, "MATERIAL_ANIMATED");
    }
    put_word(out, "EndOfRELDATA");
}

/// The data of a named-selection relation in the layout the relation writer gives it
/// (`SI_nselWriteSpecialRelationData`, 0x08347b34): chapter name, element id, parameter and
/// rank. That function is **not ported**: it takes all four from the live scene (the related
/// element's chapter and number, the fcurve's type and its position among the element's
/// fcurves of that type). This prints the values that were read.
pub fn write_named_selection(out: &mut Vec<u8>, n: &NamedSelection) {
    put_word(out, "RELDATA");
    out.extend_from_slice(&n.chapter_name);
    out.push(b' ');
    out.extend_from_slice(&n.elem_id);
    out.push(b' ');
    put_int(out, n.fcurve);
    put_int(out, n.rank);
    put_word(out, "EndOfRELDATA");
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoDict;
    impl ReadCtx for NoDict {
        fn find_list(&self, name: &[u8]) -> i32 {
            if name == b"MODELS" { 8 } else { -1 }
        }
        fn elem_index(&self, _list: i32, token: &[u8]) -> i32 {
            crate::cnum::atoi(token)
        }
    }

    fn rd(text: &str, relid: i32) -> (Result<Option<RelData>, Stuck>, Vec<u8>) {
        let mut f = AsciiFile::new(text.as_bytes());
        let r = read(&mut f, relid, &NoDict);
        (r, f.token().to_vec())
    }

    fn wr(relid: i32, d: &RelData) -> String {
        let mut out = Vec::new();
        write(&mut out, CLib::Glibc, true, relid, d);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn keyword_records_round_trip_in_the_writers_order() {
        let text = "REVOL_SUB 4 CLOSE 1 DEGREE 180 STEP 7 AXIS 2 SUBDIVISION 9 IS_PATCH 1 SPL_TYPE 3 EndOfRELDATA ; ";
        let (r, tok) = rd(text, REL_REVOLUTION);
        let d = r.unwrap().unwrap();
        assert_eq!(tok, b"EndOfRELDATA");
        assert_eq!(wr(REL_REVOLUTION, &d),
                   "RELDATA CLOSE 1 DEGREE 180 STEP 7 AXIS 2 SUBDIVISION 9 IS_PATCH 1 SPL_TYPE 3 REVOL_SUB 4 EndOfRELDATA ");
        // absent keywords keep the allocator's values; unknown words are skipped
        let (r, _) = rd("NONSENSE 5 CLOSE 1 EndOfRELDATA ", REL_REVOLUTION);
        assert_eq!(wr(REL_REVOLUTION, &r.unwrap().unwrap()),
                   "RELDATA CLOSE 1 DEGREE 1 STEP 5 AXIS 1 SUBDIVISION 5 IS_PATCH 0 SPL_TYPE 1 REVOL_SUB 3 EndOfRELDATA ");
    }

    #[test]
    fn shorts_keep_sixteen_bits_and_doubles_print_as_singles() {
        let (r, _) = rd("IN_U_DIR 70000 LOCATION 0.1 EndOfRELDATA ", REL_EXTRACT);
        let d = r.unwrap().unwrap();
        // 70000 & 0xffff = 4464; 0.1 goes double → float → %.7g
        assert_eq!(wr(REL_EXTRACT, &d), "RELDATA IN_U_DIR 4464 IS_ISOCURVE 0 LOCATION 0.1 HOLE_NO 0 TRIM_NO 0 EndOfRELDATA ");
        let RelData::Record(rec) = d else { panic!() };
        assert_eq!(rec.values("LOCATION"), Some(vec![Value::F64(0.1)]));
    }

    #[test]
    fn constructors() {
        let s = symmetry_create(1, 2, 3, 4, [None, Some([5.0, 6.0, 7.0]), None]);
        assert_eq!(wr(REL_SYMMETRY, &RelData::Record(s)),
                   "RELDATA PLANE 1 REF 0 HCLEN 2 FSTRT 3 FDURN 4 INIT_LROT 0 0 0 INIT_GROT 5 6 7 INIT_AROT 0 0 0 EndOfRELDATA ");
        // a quick-stretch copy takes the keyword fields only
        let mut src = quick_stretch_create(None);
        src.image[2] = 9; // not a keyword field
        src.image[0x122] = 1; // AUTO_SWING
        let copy = quick_stretch_create(Some(&src));
        assert_eq!((copy.image[2], copy.image[0x122]), (0, 1));
        assert_eq!(quick_stretch_create(None), Record::new(&QUICK_STRETCH));
        assert_eq!(QUICK_STRETCH.keywords.iter().map(|k| k.fields.len()).sum::<usize>(), 87);
    }

    #[test]
    fn quick_stretch_defaults() {
        let (r, _) = rd("EndOfRELDATA ", REL_QUICK_STRETCH);
        let s = wr(REL_QUICK_STRETCH, &r.unwrap().unwrap());
        assert!(s.starts_with("RELDATA MODE 1 0 0 1 0 0 0 0 0 0 0 0 SENSITIVITY 0.005 0.005 0.005 0.005 0.005 0.005 0.0005 0.0005 "));
        assert!(s.ends_with("MAXDEFORM 100 100 100 100 100 100 1000 1000 1000 1000 1000 1000 PARENT_OVER 1 AUTO_SWING 0 WIND_SENSITIVE 0 EndOfRELDATA "));
    }

    #[test]
    fn trimming_curves() {
        let (r, _) = rd("NB_PROJ_CURVE 3 PROJ_CURVE 4 5 6 SURF_OPERATOR 2 EndOfRELDATA ", REL_TRIMMING);
        let s = wr(REL_TRIMMING, &r.unwrap().unwrap());
        assert_eq!(s, "RELDATA PROJ_METHOD 0 PARA_AXIS 0 0 1 PERSP_AXIS 0 0 0 COORDI 2 TRIM_REGION 0 NB_PROJ_CURVE 3 PROJ_CURVE 4 5 6 SURF_OPERATOR 2 EndOfRELDATA ");
        // a count that the file cannot back does not allocate for it
        let (r, _) = rd("NB_PROJ_CURVE 2000000000 PROJ_CURVE 4 5 ", REL_TRIMMING);
        assert_eq!(r, Err(Stuck::EndlessAtEof));
    }

    #[test]
    fn generic_floats_go_in_file_order() {
        // the reader stores by position, the writer prints by keyword
        let (r, _) = rd("TRNS 1 2 3 ROLL 4 SCLE 5 6 7 EndOfRELDATA ", 220);
        let d = r.unwrap().unwrap();
        assert_eq!(wr(220, &d), "RELDATA SCLE 1 2 3 ROLL 4 TRNS 5 6 7 EndOfRELDATA ");
        // dynamics values print truncated to integers
        let (r, _) = rd("ROOTCPOS 1.9 -1.9 0 0 0 EndOfRELDATA ", 800);
        assert!(wr(800, &r.unwrap().unwrap()).starts_with("RELDATA ROOTCPOS 1 -1 0 0 0 ROOTCORI 0 0 0 0 0 COLL 0 0 "));
        // too many values: kept, and the capacity says the original overran
        let (r, _) = rd("OBST 1 OBST 2 EndOfRELDATA ", 807);
        assert_eq!(r.unwrap().unwrap(), RelData::Floats { relid: 807, capacity: 1, values: vec![1.0, 2.0] });
    }

    #[test]
    fn matrix_stops_at_the_end_word() {
        let (r, tok) = rd("2 0 0 5 EndOfRELDATA ; ", REL_CLUSTER_CENTRE);
        let RelData::Matrix(m) = r.unwrap().unwrap() else { panic!() };
        assert_eq!(m[0], [2.0, 0.0, 0.0, 5.0]);
        assert_eq!(m[1], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(tok, b"EndOfRELDATA");
        // no numbers at all: the end word itself is converted (to 0) into the first cell, and
        // the reader goes on eating tokens, here to the end of the file, for all sixteen cells
        let (r, _) = rd("EndOfRELDATA ; ", REL_CLUSTER_CENTRE);
        let RelData::Matrix(m) = r.unwrap().unwrap() else { panic!() };
        assert_eq!(m, [[0.0; 4]; 4]);
        // sixteen numbers: one more token is read after the last, and left in the buffer
        let (r, tok) = rd("1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 EndOfRELDATA ; ", REL_CLUSTER_CENTRE);
        let RelData::Matrix(m) = r.unwrap().unwrap() else { panic!() };
        assert_eq!(m[3], [13.0, 14.0, 15.0, 16.0]);
        assert_eq!(tok, b"EndOfRELDATA");
    }

    #[test]
    fn custom_and_expression() {
        let (r, tok) = rd("1 3 12 0 0 AOCC 4 ROCC 5 ENAME Eff.v1 MNAME Motion EndOfRELDATA ; ", REL_CUSTOM);
        let d = r.unwrap().unwrap();
        assert_eq!(tok, b"EndOfRELDATA");
        assert_eq!(wr(REL_CUSTOM, &d), "RELDATA 1 3 12 0 0 AOCC 4 ROCC 5 ENAME Eff.v1 MNAME Motion EndOfRELDATA ");
        // a truncated custom record stops cleanly; absent names print as glibc prints null
        let (r, _) = rd("1 3", REL_CUSTOM);
        assert_eq!(wr(REL_CUSTOM, &r.unwrap().unwrap()), "RELDATA 1 0 (null) 0 0 AOCC 0 ROCC 0 ENAME (null) MNAME (null) EndOfRELDATA ");
        let (r, _) = rd("139 EndOfRELDATA ", REL_EXP);
        assert_eq!(wr(REL_EXP, &r.unwrap().unwrap()), "RELDATA 139 EndOfRELDATA ");
        let (r, _) = rd("566 'a name' EndOfRELDATA ", REL_EXP);
        assert_eq!(wr(REL_EXP, &r.unwrap().unwrap()), "RELDATA 566 'a name' EndOfRELDATA ");
        let (r, _) = rd("566 '' EndOfRELDATA ", REL_EXP);
        assert_eq!(wr(REL_EXP, &r.unwrap().unwrap()), "RELDATA 566 '_UNKNOWN_' EndOfRELDATA ");
        // expression data is written whatever the first chapter is; other kinds are not
        let mut out = Vec::new();
        write(&mut out, CLib::Glibc, false, REL_EXP, &RelData::Expression(Expression { fcurve: 7, name: None }));
        assert_eq!(out, b"RELDATA 7 EndOfRELDATA ");
        let mut out = Vec::new();
        write(&mut out, CLib::Glibc, false, 220, &RelData::Floats { relid: 220, capacity: 7, values: vec![] });
        assert!(out.is_empty());
    }

    #[test]
    fn anim_bool_and_named_selection_and_unknown() {
        let (r, _) = rd("ACTIVE OPERATION difference MATERIAL respective REMOVE_EDGES EndOfRELDATA ", REL_BOOL);
        let d = r.unwrap().unwrap();
        assert_eq!(wr(REL_BOOL, &d), "RELDATA ACTIVE OPERATION difference MATERIAL respective REMOVE_EDGES EndOfRELDATA ");
        // without ACTIVE the reader has cleared the allocator's 1; the operation stays union
        let (r, _) = rd("EndOfRELDATA ", REL_BOOL);
        assert_eq!(wr(REL_BOOL, &r.unwrap().unwrap()), "RELDATA OPERATION union MATERIAL none EndOfRELDATA ");
        let (r, tok) = rd("MODELS 4 139 2 junk EndOfRELDATA ; ", REL_NAMED_SELECTION);
        let RelData::NamedSelection(n) = r.unwrap().unwrap() else { panic!() };
        assert_eq!((n.chapter, n.elem, n.fcurve, n.rank), (8, 4, 139, 2));
        assert_eq!(tok, b"EndOfRELDATA");
        let mut out = Vec::new();
        write_named_selection(&mut out, &n);
        assert_eq!(out, b"RELDATA MODELS 4 139 2 EndOfRELDATA ");
        // a relation id with no data structure: skipped to the end word, null result
        let (r, tok) = rd("A 1 B 2 EndOfRELDATA ; ", 110);
        assert_eq!(r, Ok(None));
        assert_eq!(tok, b"EndOfRELDATA");
    }

    #[test]
    fn truncated_input_is_reported_not_looped_on() {
        assert_eq!(rd("CLOSE 1 ", REL_REVOLUTION).0, Err(Stuck::EndlessAtEof));
        assert_eq!(rd("139 ", REL_EXP).0, Err(Stuck::EndlessAtEof));
        assert_eq!(rd("SCLE 1 2 3", 220).0, Err(Stuck::EndlessAtEof));
    }

    #[test]
    fn rotation_limits_get_radians() {
        let (r, _) = rd("MIN_LIMIT -90 0 45 MAX_LIMIT 90 180 360 DAMPING 1 2 EndOfRELDATA ", REL_CNS_ROT_LIM);
        let RelData::Record(rec) = r.unwrap().unwrap() else { panic!() };
        let rad = |i: usize| f32::from_le_bytes(rec.image[0x24 + 4 * i..0x28 + 4 * i].try_into().unwrap());
        assert_eq!(rad(0), (-90.0f64 * 0.0174532925) as f32);
        assert_eq!(rad(5), (360.0f64 * 0.0174532925) as f32);
        assert_eq!(rec.values("DAMPING"), Some(vec![Value::F32(1.0), Value::F32(2.0)]));
    }

    #[test]
    fn x87_product_rounds_twice() {
        // agrees with plain double arithmetic wherever that has no double-rounding case
        for (a, b) in [(0.0174532925, 90.0f32), (0.0174532925, -37.5), (1.0, 3.0), (0.1, 0.1), (3.0, 1.0e-3)] {
            assert_eq!(x87_mul_to_f32(a, b), (a * b as f64) as f32, "{a} {b}");
        }
        // sign, zero
        assert_eq!(x87_mul_to_f32(-2.0, 1.5), -3.0);
        assert_eq!(x87_mul_to_f32(0.0, 1.5), 0.0);
    }

    #[test]
    fn quoted_strings() {
        let q = |s: &[u8]| read_quoted(&mut AsciiFile::new(s));
        assert_eq!(q(b"  'abc' x"), Some(b"abc".to_vec()));
        assert_eq!(q(b"''"), None);
        assert_eq!(q(b""), None);
        assert_eq!(q(b"xabc' y"), Some(b"abc".to_vec())); // the first byte is dropped whatever it is
        assert_eq!(q(b"'ab"), Some(b"ab".to_vec())); // ended by the end of the file
        let long: Vec<u8> = std::iter::once(b'\'').chain(std::iter::repeat(b'z').take(400)).chain(std::iter::once(b'\'')).collect();
        assert_eq!(q(&long).map(|v| v.len()), Some(237));
    }

    #[test]
    fn float_to_int_like_fistp() {
        assert_eq!(x87_trunc(1.9), 1);
        assert_eq!(x87_trunc(-1.9), -1);
        assert_eq!(x87_trunc(3.0e9), i32::MIN);
        assert_eq!(x87_trunc(f32::NAN), i32::MIN);
    }
}
