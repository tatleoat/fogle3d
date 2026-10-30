// SPDX-License-Identifier: MIT
//! Records shared by the renderer's source files (finding 0012, "Records").
//!
//! Layouts are read from the code: which offsets the functions read and write. The triangle
//! `tn`, the vertex `SI_RVertex`, the scan-line record `slice` and the window `ClipBox` are in
//! [`si_math::geometry`] (as `Tn`, `RVertex`, `Span`, `ClipBox`), where the geometry stage's
//! first ten functions were ported.

pub use si_math::geometry::{ClipBox, RVertex, Span, Tn};
pub use si_math::types::{Matrix, Pspoint, Vector};

/// `SI_RGBAZO`: what a sample is worth, six floats, 24 bytes (the count `SI_Sample` copies
/// [soft 0x082602bc SI_Sample]).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgbazo {
    /// +0x00
    pub r: f32,
    /// +0x04
    pub g: f32,
    /// +0x08
    pub b: f32,
    /// +0x0c
    pub a: f32,
    /// +0x10
    pub z: f32,
    /// +0x14
    pub o: f32,
}

/// `SI_Ray`: 0x48 bytes (the stride of the array `SI_Sample` walks
/// [soft 0x082602bc SI_Sample]). Member names are provisional until every reader is ported;
/// the offsets are read.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ray {
    /// +0x00: where the ray starts.
    pub origin: Vector,
    /// +0x0c: its direction.
    pub dir: Vector,
    /// +0x18: three floats written by `si_trace` and read by `push`.
    pub v18: Vector,
    /// +0x24: distance along the ray of the nearest hit so far.
    pub t: f32,
    /// +0x28: the hit point.
    pub hit: Vector,
    /// +0x34: the triangle hit (`tn *`); `None` is a null pointer.
    pub tn: Option<usize>,
    /// +0x38
    pub w38: u32,
    /// +0x3c
    pub w3c: i16,
    /// +0x3e
    pub w3e: i16,
    /// +0x40
    pub w40: u32,
    /// +0x44
    pub w44: u32,
}
