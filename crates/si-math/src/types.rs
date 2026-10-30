// SPDX-License-Identifier: MIT
//! The value types of the mathematics core (finding 0008, "Types").
//!
//! Layouts are read from the code of the Linux `soft` binary (which offsets a function reads
//! and writes) and agree with the public SDK header `SAA_types.h`.

/// A 4 × 4 matrix of floats, 64 bytes, `m[row][column]`, rows stored one after another.
///
/// The demangled signatures of the binary say `float (*)[3]`: GCC 2.95 writes an array bound
/// as its highest index. Vectors are rows and multiply from the left (`v' = v × M`), so a
/// transformation's translation is row 3 and `A × B` applies `A` first.
pub type Matrix = [[f32; 4]; 4];

/// The same in doubles, 128 bytes (demangled `double (*)[3]`).
pub type MatrixD = [[f64; 4]; 4];

/// A 3 × 3 matrix of floats, 36 bytes (demangled `float (*)[2]`).
pub type Matrix3 = [[f32; 3]; 3];

/// The identity matrix. The original keeps it as a global variable in writable data
/// [soft 0x086dc160 SI_identite] and copies it wherever a matrix is initialised; nothing in
/// the files of this crate writes to it.
pub const IDENTITY: Matrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

pub const IDENTITY3: Matrix3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// `SI_Vector`: three floats, 12 bytes. (The SDK's `SAA_Vector` has a fourth member `w`; that
/// is [`Hvector`] here.)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vector {
    pub const fn new(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    pub const fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    pub const fn from_array(a: [f32; 3]) -> Vector {
        Vector { x: a[0], y: a[1], z: a[2] }
    }
}

/// `SI_Hvector`: four floats, 16 bytes; a point in homogeneous coordinates or one matrix row.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hvector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Hvector {
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Hvector {
        Hvector { x, y, z, w }
    }

    pub const fn to_array(self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }

    pub const fn from_array(a: [f32; 4]) -> Hvector {
        Hvector { x: a[0], y: a[1], z: a[2], w: a[3] }
    }
}

/// `SI_Pspoint`: a point on the picture plane after the perspective division, two floats
/// (x at offset 0, y at 4), 8 bytes. Read from the two stores of `SI_worldToPrsp1` and
/// `SI_worldToPrsp2` and from the renderer's vertex record, where the next member starts 8
/// bytes after it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pspoint {
    pub x: f32,
    pub y: f32,
}
