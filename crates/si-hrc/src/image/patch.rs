// SPDX-License-Identifier: MIT
//! Patches on the image: the allocator of `patch.c` and `patchBinaryRead` of `binary.c`
//! (finding 0004, "Patch, Spline, Spline key, Face").
//!
//! `si-types` has the record (`Patch`, 0x204 bytes); a pointer is typed with it and the
//! fields are addressed by the offsets of [`patch`] below, each read from the listing.
//! `si-spline` has the allocator on the image; it depends on this crate, so it is ported
//! here a second time (the lead reconciles duplicates).

use super::globals::SI_IDENTITE;
use super::io::{alloc_fits, byteswap_fread, file_skip_field, float_fread, vector_fread};
use super::matio::next_token;
use super::outside::{Outside, Routine};
use super::proto::{slot, Prototypes};
use super::texture::matrix_copy;
use si_mem::{Mem, Ptr32};
pub use si_types::geom::Patch;

/// Offsets in a patch (`SI_Patch`).
pub mod patch {
    /// The types in U and V as ints: 0 linear, 1 Bézier, 2 B-spline, 3 cardinal.
    pub const TYPE_U: u32 = 0x04;
    pub const TYPE_V: u32 = 0x08;
    /// The numbers of points in U and V (unsigned shorts).
    pub const NB_U: u32 = 0x0c;
    pub const NB_V: u32 = 0x0e;
    pub const CLOSED_U: u32 = 0x10;
    pub const CLOSED_V: u32 = 0x12;
    pub const TENSION_U: u32 = 0x14;
    pub const TENSION_V: u32 = 0x18;
    pub const STEP_U: u32 = 0x1c;
    pub const STEP_V: u32 = 0x1e;
    pub const CURVES_U: u32 = 0x20;
    pub const CURVES_V: u32 = 0x22;
    /// Four matrices, the identity in a new patch.
    pub const MATRICES: u32 = 0xa4;
    /// The points: 16 bytes each, a vector and a float that is 1.
    pub const POINTS: u32 = 0x1a4;
    /// One short per point: tagged or not.
    pub const TAGS: u32 = 0x1a8;
    /// The four capping flags (tokens 12 to 15) and the faceted flag (16).
    pub const CAPS: u32 = 0x1cc;
    pub const FACETED: u32 = 0x1d4;
    /// The approximation: view (18), the three floats (19 to 21: 5, 0.1 and 45 in a new
    /// patch), minimum, maximum and recursion (22 to 24), method (17).
    pub const APPROX_VIEW: u32 = 0x1e8;
    pub const APPROX_FLOATS: u32 = 0x1ec;
    pub const APPROX_MIN: u32 = 0x1f8;
    pub const APPROX_MAX: u32 = 0x1fa;
    pub const APPROX_RECURSION: u32 = 0x1fc;
    pub const APPROX_METHOD: u32 = 0x200;
    pub const SIZE: u32 = 0x204;
}

/// `SI_patchUpdateMatrix (0x0847a680)`, `patch.c`: the patch's basis matrices from its
/// types and tensions.
pub const SI_PATCH_UPDATE_MATRIX: Routine = Routine { name: "SI_patchUpdateMatrix", addr: 0x0847a680 };

/// [soft 0x0847a0d0 SI_patchAllocate]: a new patch: steps 3, one curve each way, four
/// identity matrices, the approximation floats 5, 0.1 and 45 (the three floats at
/// 0x086602d8), method 1, minimum 1, maximum 10; the rest of what it writes is zero. Not
/// written: +0x24 to +0xa3, +0x1ae, +0x1c2, +0x1e0, +0x1ea, +0x1fe, +0x202.
pub fn patch_allocate(m: &mut Mem) -> Ptr32<Patch> {
    let p = m.si_malloc(patch::SIZE);
    if p != 0 {
        for off in [0x14, 0x18, 0x00, 0x04, 0x08] {
            m.set_u32(p + off, 0);
        }
        for off in [0x0c, 0x0e, 0x10, 0x12] {
            m.set_u16(p + off, 0);
        }
        m.set_u16(p + patch::STEP_U, 3);
        m.set_u16(p + patch::STEP_V, 3);
        m.set_u16(p + patch::CURVES_U, 1);
        m.set_u16(p + patch::CURVES_V, 1);
        for k in 0..4 {
            matrix_copy(m, SI_IDENTITE.addr(), p + patch::MATRICES + 0x40 * k);
        }
        m.set_f32(p + patch::APPROX_FLOATS, 5.0);
        m.set_f32(p + patch::APPROX_FLOATS + 4, 0.1);
        m.set_f32(p + patch::APPROX_FLOATS + 8, 45.0);
        for off in [0x1a4, 0x1a8, 0x1b0, 0x1d8, 0x1b8, 0x1dc, 0x1c4, 0x1c8, 0x1e4] {
            m.set_u32(p + off, 0);
        }
        for off in [0x1ac, 0x1b4, 0x1b6, 0x1bc, 0x1be, 0x1c0, 0x1cc, 0x1ce, 0x1d0, 0x1d2, 0x1d4, 0x1d6, 0x1e8, 0x1fc] {
            m.set_u16(p + off, 0);
        }
        m.set_u16(p + patch::APPROX_METHOD, 1);
        m.set_u16(p + patch::APPROX_MIN, 1);
        m.set_u16(p + patch::APPROX_MAX, 10);
    }
    Ptr32::new(p)
}

/// [soft 0x083f0690 patchBinaryRead]: a patch from the file.
///
/// Tokens 1 and 2 are the types (shorts, kept as ints); 3 the two counts and the points
/// (zeroed arrays of nu × nv points of 16 bytes and of as many shorts; per point a vector
/// is read, the float behind it set to 1 and the point's short cleared); 4 and 5 the closed
/// flags (anything but 0 becomes 1); 6 and 7 the tensions; 8 to 18 and 22 to 24 a short
/// each; 19 to 21 a float each; 25 one short per point into the array token 3 made (the
/// counts being what the patch holds then).
///
/// **Any other token is skipped as a long field**: the reader hands `si_fileSkipField` the
/// token −1, not the token it met, so an unknown token is always taken to be followed by a
/// 4-byte length and that many bytes.
///
/// After the tokens the basis matrices are made (`SI_patchUpdateMatrix`).
pub fn patch_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) -> Ptr32<Patch> {
    let q = patch_allocate(m);
    if q.is_null() {
        return q;
    }
    let p = q.addr();
    const FRAME: u32 = 4;
    let frame = m.local(FRAME);
    let (scratch, token_at) = (frame, frame + 2);
    let list_at = protos.addr().wrapping_add(4 * slot::PATCH as u32);
    let points_of = |m: &Mem| (m.u16(p + patch::NB_U) as u32 * m.u16(p + patch::NB_V) as u32) as i32;
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        let token = m.i16(token_at);
        match token {
            1 | 2 => {
                byteswap_fread(m, o, scratch, 2, 1, file);
                let v = m.i16(scratch) as i32;
                m.set_i32(p + if token == 1 { patch::TYPE_U } else { patch::TYPE_V }, v);
            }
            3 => {
                byteswap_fread(m, o, p + patch::NB_U, 2, 1, file);
                byteswap_fread(m, o, p + patch::NB_V, 2, 1, file);
                let n = points_of(m) as u32;
                let mut point = if alloc_fits(m, o, file, n.wrapping_mul(0x10), n) { m.si_calloc(n, 0x10) } else { 0 };
                m.set_u32(p + patch::POINTS, point);
                let n = points_of(m) as u32;
                let mut tag = if alloc_fits(m, o, file, n.wrapping_mul(2), n) { m.si_calloc(n, 2) } else { 0 };
                m.set_u32(p + patch::TAGS, tag);
                let mut left = points_of(m);
                while 0 < left {
                    vector_fread(m, o, point, 0xc, 1, file);
                    m.set_u32(point.wrapping_add(0xc), 0x3f80_0000);
                    m.set_u16(tag, 0);
                    point = point.wrapping_add(0x10);
                    tag = tag.wrapping_add(2);
                    left -= 1;
                }
            }
            4 | 5 => {
                let at = p + if token == 4 { patch::CLOSED_U } else { patch::CLOSED_V };
                byteswap_fread(m, o, at, 2, 1, file);
                if m.i16(at) != 0 {
                    m.set_i16(at, 1);
                }
            }
            6 | 7 => {
                byteswap_fread(m, o, p + if token == 6 { patch::TENSION_U } else { patch::TENSION_V }, 4, 1, file);
            }
            8..=18 | 22..=24 => {
                let off = match token {
                    8 => patch::STEP_U,
                    9 => patch::STEP_V,
                    10 => patch::CURVES_U,
                    11 => patch::CURVES_V,
                    12..=16 => patch::CAPS + 2 * (token as u32 - 12),
                    17 => patch::APPROX_METHOD,
                    18 => patch::APPROX_VIEW,
                    22 => patch::APPROX_MIN,
                    23 => patch::APPROX_MAX,
                    _ => patch::APPROX_RECURSION,
                };
                byteswap_fread(m, o, p + off, 2, 1, file);
            }
            19..=21 => {
                float_fread(m, o, p + patch::APPROX_FLOATS + 4 * (token as u32 - 19), 4, 1, file);
            }
            25 => {
                let mut tag = m.u32(p + patch::TAGS);
                let mut left = points_of(m);
                while 0 < left {
                    byteswap_fread(m, o, tag, 2, 1, file);
                    tag = tag.wrapping_add(2);
                    left -= 1;
                }
            }
            _ => {
                file_skip_field(m, o, file, -1);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    o.call(m, &SI_PATCH_UPDATE_MATRIX, &[p]);
    m.release(FRAME);
    q
}
