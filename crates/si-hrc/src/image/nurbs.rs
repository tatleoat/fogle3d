// SPDX-License-Identifier: MIT
//! NURBS curves, surfaces and composite curves on the image: the allocators and the curve's
//! copy and dispose of `nurbs.c`, and the three readers of `binary.c` (finding 0004,
//! "NURBS").
//!
//! `si-types` has the records (`NurbsCrv` 100 bytes, `NurbsPoint` 0x24, `NurbsSrf` 0xdc);
//! a pointer is typed with them and the fields are addressed by the offsets of [`crv`] and
//! [`srf`] below, each read from the listing. The composite curve has no record in
//! `si-types`: [`comp`] gives its offsets. `si-nurbs` has the allocators, the copy and the
//! dispose in its own form; it depends on this crate, so they are ported here from the
//! listing (the lead reconciles duplicates).

use super::globals::FILE_VERSION;
use super::io::{alloc_fits, byteswap_fread, double_fread, file_skip_field, float_fread};
use super::matio::next_token;
use super::outside::{Outside, Routine};
use super::proto::{slot, Prototypes};
use si_mem::{Mem, Ptr32};
pub use si_types::geom::{NurbsCrv, NurbsSrf};

/// Offsets in a NURBS curve (`__SI_NurbsCrv`).
pub mod crv {
    pub const ORDER: u32 = 0x00;
    pub const NB_POINTS: u32 = 0x04;
    pub const NB_KNOTS: u32 = 0x08;
    /// The knots: doubles in a block of their own.
    pub const KNOTS: u32 = 0x0c;
    /// Two doubles: where the curve's parameter starts and ends.
    pub const START: u32 = 0x10;
    pub const END: u32 = 0x18;
    pub const CLOSED: u32 = 0x20;
    pub const UNIFORMITY: u32 = 0x24;
    /// The control points: records of 0x24 bytes (four doubles and an int flag).
    pub const POINTS: u32 = 0x2c;
    pub const CLOSING: u32 = 0x30;
    pub const STEP: u32 = 0x34;
    pub const KEYFRAMED: u32 = 0x4c;
    /// A count and two arrays of that many doubles (the curve's length data), and one more
    /// block, all freed with the curve.
    pub const NB_LENGTHS: u32 = 0x50;
    pub const LENGTHS_A: u32 = 0x54;
    pub const LENGTHS_B: u32 = 0x58;
    pub const EXTRA: u32 = 0x5c;
    pub const SIZE: u32 = 0x64;
    /// A control point.
    pub const POINT: u32 = 0x24;
    pub const POINT_FLAG: u32 = 0x20;
}

/// Offsets in a NURBS surface (`SI_NurbsSrf`): the fields of a curve's direction twice
/// (U at +0, V at +0x24), then what the surface has of its own.
pub mod srf {
    /// The V direction's fields lie this far behind the U direction's.
    pub const V: u32 = 0x24;
    pub const ORDER: u32 = 0x00;
    pub const NB_POINTS: u32 = 0x04;
    pub const NB_KNOTS: u32 = 0x08;
    pub const KNOTS: u32 = 0x0c;
    pub const START: u32 = 0x10;
    pub const END: u32 = 0x18;
    pub const CLOSED: u32 = 0x20;
    /// The number of control points: the product of the two counts.
    pub const TOTAL_POINTS: u32 = 0x48;
    pub const UNIFORMITY: u32 = 0x4c;
    pub const POINTS: u32 = 0x54;
    pub const BORDER: u32 = 0x5c;
    /// The trims: a count and an array of pointers to composite curves.
    pub const NB_TRIMS: u32 = 0x60;
    pub const TRIMS: u32 = 0x64;
    /// The curves projected on the surface, likewise.
    pub const NB_PROJECTED: u32 = 0x68;
    pub const PROJECTED: u32 = 0x6c;
    /// Display steps in U and V, then the numbers of wireframe curves (ints).
    pub const STEPS: u32 = 0x70;
    pub const CURVES: u32 = 0x78;
    /// Four capping flags (tokens 12 to 15).
    pub const CAPS: u32 = 0x88;
    /// The closings in U and V (ints).
    pub const CLOSING: u32 = 0x90;
    /// The approximation: method, view, three floats (5, 0.1 and 45 in a new surface),
    /// minimum, maximum, recursion.
    pub const APPROX_METHOD: u32 = 0x98;
    pub const APPROX_VIEW: u32 = 0x9c;
    pub const APPROX_FLOATS: u32 = 0xa0;
    pub const APPROX_MIN: u32 = 0xac;
    pub const APPROX_MAX: u32 = 0xae;
    pub const APPROX_RECURSION: u32 = 0xb0;
    pub const SIZE: u32 = 0xdc;
}

/// Offsets in a composite curve (0x18 bytes; no record in `si-types`).
pub mod comp {
    pub const BOUNDARY: u32 = 0x00;
    pub const NB_CURVES: u32 = 0x04;
    /// An array of pointers to curves.
    pub const CURVES: u32 = 0x08;
    pub const SIZE: u32 = 0x18;
}

/// `SI_nurbsSrfForceCapping (0x084f0134)`, `nurbs.c`: makes a surface's border in one of
/// four directions a cap.
pub const SI_NURBS_SRF_FORCE_CAPPING: Routine = Routine { name: "SI_nurbsSrfForceCapping", addr: 0x084f0134 };

/// [soft 0x084db66c SI_nurbsSrfAllocate]: a new surface: order 4 both ways, display steps
/// 3, one wireframe curve each way, closings 1, approximation method 1 with 5, 0.1 and 45
/// (the floats at 0x08661740), minimum 1, maximum 10; the rest of what it writes is zero.
/// Not written: the shorts at +0x22, +0x46, +0x52, +0x5e, +0x9a, +0x9e and +0xb2.
pub fn nurbs_srf_allocate(m: &mut Mem) -> Ptr32<NurbsSrf> {
    let p = m.si_malloc(srf::SIZE);
    if p != 0 {
        for w in (1..=7).chain(0xa..=0x10).chain([0x12, 0x13, 0x15, 0x16]).chain(0x18..=0x1b).chain([0x20, 0x21]).chain(0x2e..=0x36) {
            m.set_u32(p + 4 * w, 0);
        }
        m.set_u32(p + srf::ORDER, 4);
        m.set_u32(p + srf::V + srf::ORDER, 4);
        for off in [0x20, 0x44, 0x50, 0x5c, 0xb4, 0xb6, 0x88, 0x8a, 0x8c, 0x8e, 0x9c, 0xb0] {
            m.set_u16(p + off, 0);
        }
        m.set_u32(p + srf::STEPS, 3);
        m.set_u32(p + srf::STEPS + 4, 3);
        m.set_u32(p + srf::CURVES, 1);
        m.set_u32(p + srf::CURVES + 4, 1);
        m.set_f32(p + srf::APPROX_FLOATS, 5.0);
        m.set_f32(p + srf::APPROX_FLOATS + 4, 0.1);
        m.set_f32(p + srf::APPROX_FLOATS + 8, 45.0);
        m.set_u32(p + srf::CLOSING, 1);
        m.set_u32(p + srf::CLOSING + 4, 1);
        m.set_u16(p + srf::APPROX_METHOD, 1);
        m.set_u16(p + srf::APPROX_MIN, 1);
        m.set_u16(p + srf::APPROX_MAX, 10);
    }
    Ptr32::new(p)
}

/// [soft 0x084db5a0 SI_nurbsCrvAllocate]: a new curve: order 4, display step 10, closing
/// 1, everything else zero. The shorts at +0x22, +0x2a and +0x46 are not written.
pub fn nurbs_crv_allocate(m: &mut Mem) -> Ptr32<NurbsCrv> {
    let p = m.si_malloc(crv::SIZE);
    if p != 0 {
        for off in [0x10, 0x14, 0x18, 0x1c, 0x3c, 0x40] {
            m.set_u32(p + off, 0);
        }
        m.set_u32(p + crv::ORDER, 4);
        for off in [0x04, 0x08, 0x0c] {
            m.set_u32(p + off, 0);
        }
        m.set_u16(p + crv::CLOSED, 0);
        m.set_u32(p + crv::UNIFORMITY, 0);
        m.set_u16(p + 0x28, 0);
        m.set_u32(p + crv::POINTS, 0);
        m.set_u32(p + crv::STEP, 10);
        m.set_u32(p + 0x38, 0);
        m.set_u32(p + crv::CLOSING, 1);
        m.set_u16(p + 0x44, 0);
        m.set_u32(p + 0x48, 0);
        m.set_u16(p + 0x4e, 0);
        m.set_u32(p + crv::NB_LENGTHS, 0);
        m.set_u16(p + crv::KEYFRAMED, 0);
        for off in [0x54, 0x58, 0x5c, 0x60] {
            m.set_u32(p + off, 0);
        }
    }
    Ptr32::new(p)
}

/// [soft 0x084dc47c SI_nurbsPointsAllocate]: `n` control points from `SI_malloc`, each
/// (0, 0, 0, 1) with flag 0.
pub fn nurbs_points_allocate(m: &mut Mem, n: i32) -> u32 {
    let p = m.si_malloc((n as u32).wrapping_mul(crv::POINT));
    if p != 0 && 0 < n {
        let mut at = p;
        let mut left = n;
        loop {
            for (w, v) in [0u32, 0, 0, 0, 0, 0, 0, 0x3ff0_0000, 0].into_iter().enumerate() {
                m.set_u32(at + 4 * w as u32, v);
            }
            at = at.wrapping_add(crv::POINT);
            left -= 1;
            if left == 0 {
                break;
            }
        }
    }
    p
}

/// [soft 0x084dc500 si_knotsAllocate]: `n` knots: zeroed doubles.
pub fn knots_allocate(m: &mut Mem, n: i32) -> u32 {
    m.si_calloc(n as u32, 8)
}

/// [soft 0x084dbd80 SI_nurbsCrvInit]: the content of the curve `src` into the record
/// `dst`: order, counts, range, flags and steps; a new knot array with the knots; if the
/// source has points, a new point array with the points; and the length data (two new
/// arrays of doubles) if the source has any, otherwise none. What `dst` pointed to before
/// is not freed, and without source points `dst` keeps its point pointer.
pub fn nurbs_crv_init(m: &mut Mem, dst: u32, src: u32) {
    let w = |m: &mut Mem, off: u32| {
        let v = m.u32(src + off);
        m.set_u32(dst + off, v);
    };
    let s = |m: &mut Mem, off: u32| {
        let v = m.u16(src + off);
        m.set_u16(dst + off, v);
    };
    w(m, crv::ORDER);
    w(m, crv::NB_POINTS);
    for off in [0x10, 0x14, 0x18, 0x1c] {
        w(m, off);
    }
    s(m, crv::CLOSED);
    let knots = m.i32(src + crv::NB_KNOTS);
    m.set_i32(dst + crv::NB_KNOTS, knots);
    s(m, 0x28);
    w(m, crv::STEP);
    w(m, crv::UNIFORMITY);
    s(m, 0x44);
    w(m, 0x38);
    w(m, 0x3c);
    w(m, 0x40);
    s(m, crv::KEYFRAMED);
    let array = knots_allocate(m, knots);
    m.set_u32(dst + crv::KNOTS, array);
    let mut i: i32 = 0;
    while i < m.i32(src + crv::NB_KNOTS) {
        let v = m.u64(m.u32(src + crv::KNOTS).wrapping_add(8 * i as u32));
        let to = m.u32(dst + crv::KNOTS).wrapping_add(8 * i as u32);
        m.set_u64(to, v);
        i += 1;
    }
    if 0 < m.i32(src + crv::NB_POINTS) {
        let points = nurbs_points_allocate(m, m.i32(dst + crv::NB_POINTS));
        m.set_u32(dst + crv::POINTS, points);
        let mut i: i32 = 0;
        while i < m.i32(dst + crv::NB_POINTS) {
            let to = m.u32(dst + crv::POINTS).wrapping_add(crv::POINT * i as u32);
            let from = m.u32(src + crv::POINTS).wrapping_add(crv::POINT * i as u32);
            for k in 0..9 {
                let v = m.u32(from + 4 * k);
                m.set_u32(to + 4 * k, v);
            }
            i += 1;
        }
    }
    s(m, 0x4e);
    let n = m.i32(src + crv::NB_LENGTHS);
    if n < 1 {
        m.set_u32(dst + crv::NB_LENGTHS, 0);
        m.set_u32(dst + crv::LENGTHS_B, 0);
        m.set_u32(dst + crv::LENGTHS_A, 0);
    } else {
        m.set_i32(dst + crv::NB_LENGTHS, n);
        let b = m.si_calloc(n as u32, 8);
        m.set_u32(dst + crv::LENGTHS_B, b);
        let a = m.si_calloc(m.u32(dst + crv::NB_LENGTHS), 8);
        m.set_u32(dst + crv::LENGTHS_A, a);
        let mut i: i32 = 0;
        while i < m.i32(dst + crv::NB_LENGTHS) {
            for off in [crv::LENGTHS_B, crv::LENGTHS_A] {
                let v = m.u64(m.u32(src + off).wrapping_add(8 * i as u32));
                let to = m.u32(dst + off).wrapping_add(8 * i as u32);
                m.set_u64(to, v);
            }
            i += 1;
        }
    }
}

/// [soft 0x084db870 SI_nurbsCrvDispose]: frees the curve `*pp` points to: its knots, its
/// points, the block at +0x5c, the two length arrays if it has a positive number of
/// lengths, then the record; `*pp` is cleared.
pub fn nurbs_crv_dispose(m: &mut Mem, pp: u32) {
    if m.u32(pp) == 0 {
        return;
    }
    let release = |m: &mut Mem, off: u32| {
        let c = m.u32(pp);
        if m.u32(c + off) != 0 {
            m.free(m.u32(c + off));
            let c = m.u32(pp);
            m.set_u32(c + off, 0);
        }
    };
    release(m, crv::KNOTS);
    release(m, crv::POINTS);
    release(m, crv::EXTRA);
    let c = m.u32(pp);
    m.set_u32(c + 0x48, 0);
    if 0 < m.i32(c + crv::NB_LENGTHS) {
        release(m, crv::LENGTHS_A);
        release(m, crv::LENGTHS_B);
    }
    if m.u32(pp) != 0 {
        m.free(m.u32(pp));
        m.set_u32(pp, 0);
    }
}

/// [soft 0x083f2b38 nurbsCrvBinaryRead]: a NURBS curve from the file.
///
/// Tokens: the order (1), the points (2: an int count, a point array of that many, and
/// per point four doubles and then, when `binary.c`'s `fileVersion` is not below the
/// double 2.6 at 0x0865bf30, an int flag), the closed flag (3), the knots (4: an int
/// uniformity, an int count, a zeroed array, that many doubles in one read), the range (5:
/// two doubles), the display step (6), the closing (7), the key-framed flag (8 or 0x208).
///
/// The version is compared as a float against a double: a file whose version is the float
/// 2.6 (2.5999999) is below it and has no flags.
pub fn nurbs_crv_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) -> Ptr32<NurbsCrv> {
    let c = nurbs_crv_allocate(m);
    if c.is_null() {
        return c;
    }
    let p = c.addr();
    const FRAME: u32 = 4;
    let frame = m.local(FRAME);
    let token_at = frame;
    let list_at = protos.addr().wrapping_add(4 * slot::NURBS_CRV as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        match m.i16(token_at) {
            5 => {
                double_fread(m, o, p + crv::START, 8, 1, file);
                double_fread(m, o, p + crv::END, 8, 1, file);
            }
            2 => {
                byteswap_fread(m, o, p + crv::NB_POINTS, 4, 1, file);
                let n = m.i32(p + crv::NB_POINTS);
                let points = if alloc_fits(m, o, file, (n as u32).wrapping_mul(crv::POINT), n as u32) { nurbs_points_allocate(m, n) } else { 0 };
                m.set_u32(p + crv::POINTS, points);
                read_points(m, o, file, p + crv::NB_POINTS, p + crv::POINTS);
            }
            1 => {
                byteswap_fread(m, o, p + crv::ORDER, 4, 1, file);
            }
            3 => {
                byteswap_fread(m, o, p + crv::CLOSED, 2, 1, file);
            }
            4 => {
                byteswap_fread(m, o, p + crv::UNIFORMITY, 4, 1, file);
                byteswap_fread(m, o, p + crv::NB_KNOTS, 4, 1, file);
                read_knots(m, o, file, p + crv::NB_KNOTS, p + crv::KNOTS);
            }
            7 => {
                byteswap_fread(m, o, p + crv::CLOSING, 4, 1, file);
            }
            6 => {
                byteswap_fread(m, o, p + crv::STEP, 4, 1, file);
            }
            8 | 0x208 => {
                byteswap_fread(m, o, p + crv::KEYFRAMED, 2, 1, file);
            }
            t => {
                file_skip_field(m, o, file, t);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    m.release(FRAME);
    c
}

/// The control points of a curve or surface: for each of the `*count_at` points of the
/// array `*points_at`, four doubles in one read and, in a file not below version 2.6, the
/// int flag.
pub(crate) fn read_points(m: &mut Mem, o: &mut dyn Outside, file: u32, count_at: u32, points_at: u32) {
    let mut i: i32 = 0;
    while i < m.i32(count_at) {
        let at = m.u32(points_at).wrapping_add(crv::POINT.wrapping_mul(i as u32));
        double_fread(m, o, at, 8, 4, file);
        if (m[FILE_VERSION] as f64) >= 2.6 {
            let at = m.u32(points_at).wrapping_add(crv::POINT.wrapping_mul(i as u32)).wrapping_add(crv::POINT_FLAG);
            byteswap_fread(m, o, at, 4, 1, file);
        }
        i += 1;
    }
}

/// A knot vector: a zeroed array for the count at `count_at` (`si_knotsAllocate`), stored
/// at `knots_at`, and that many doubles read into it at once.
pub(crate) fn read_knots(m: &mut Mem, o: &mut dyn Outside, file: u32, count_at: u32, knots_at: u32) {
    let n = m.i32(count_at);
    let knots = if alloc_fits(m, o, file, (n as u32).wrapping_mul(8), n as u32) { knots_allocate(m, n) } else { 0 };
    m.set_u32(knots_at, knots);
    double_fread(m, o, m.u32(knots_at), 8, m.u32(count_at), file);
}

/// [soft 0x083f32e4 nurbsCompBinaryRead]: a composite curve: a zeroed record of 0x18 bytes
/// (the two bytes behind its first short are not written), the boundary flag (1), and the
/// curves (2: an int count, a zeroed array of that many pointers, and a curve read into
/// each). **Any other token is skipped as a long field** (the reader hands
/// `si_fileSkipField` the token −1).
pub fn nurbs_comp_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) -> u32 {
    let p = m.si_malloc(comp::SIZE);
    if p == 0 {
        return 0;
    }
    m.set_u32(p + comp::NB_CURVES, 0);
    m.set_u16(p + comp::BOUNDARY, 0);
    for off in [0x08, 0x10, 0x14, 0x0c] {
        m.set_u32(p + off, 0);
    }
    const FRAME: u32 = 4;
    let token_at = m.local(FRAME);
    let list_at = protos.addr().wrapping_add(4 * slot::NURBS_COMP as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        match m.i16(token_at) {
            1 => {
                byteswap_fread(m, o, p + comp::BOUNDARY, 2, 1, file);
            }
            2 => {
                byteswap_fread(m, o, p + comp::NB_CURVES, 4, 1, file);
                let n = m.u32(p + comp::NB_CURVES);
                let array = if alloc_fits(m, o, file, n.wrapping_mul(4), n) { m.si_calloc(n, 4) } else { 0 };
                m.set_u32(p + comp::CURVES, array);
                let mut i: i32 = 0;
                while i < m.i32(p + comp::NB_CURVES) {
                    let c = nurbs_crv_binary_read(m, o, file, protos);
                    let at = m.u32(p + comp::CURVES).wrapping_add(4 * i as u32);
                    m.set_u32(at, c.addr());
                    i += 1;
                }
            }
            _ => {
                file_skip_field(m, o, file, -1);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    m.release(FRAME);
    p
}

/// A count and that many composite curves: the count is read into `*count_at`; if it is
/// positive, a zeroed array of pointers goes to `*array_at` and a composite curve is read
/// into each entry.
fn read_composites(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>, count_at: u32, array_at: u32) {
    byteswap_fread(m, o, count_at, 4, 1, file);
    if 0 < m.i32(count_at) {
        let n = m.u32(count_at);
        let array = if alloc_fits(m, o, file, n.wrapping_mul(4), n) { m.si_calloc(n, 4) } else { 0 };
        m.set_u32(array_at, array);
        let mut i: i32 = 0;
        while i < m.i32(count_at) {
            let c = nurbs_comp_binary_read(m, o, file, protos);
            let at = m.u32(array_at).wrapping_add(4 * i as u32);
            m.set_u32(at, c);
            i += 1;
        }
    }
}

/// [soft 0x083f2d90 nurbsSrfBinaryRead]: a NURBS surface from the file.
///
/// Tokens: the two orders (1), the two counts and the points (2: the total is their
/// product; a point array of that many, each read as a curve's points are), the two
/// closed flags (3), the knots (4: uniformity, then count and knots of U, then of V), the
/// ranges (5: four doubles), the border flag (6), the trims (7: a count and that many
/// composite curves), display steps (8, 9) and wireframe curves (10, 11), four capping
/// flags (12 to 15), the closings (16, 17), the approximation (18, 19 shorts; 20 to 22
/// floats; 23 to 25 shorts), and the projected curves (0x1a: as the trims; 0xff1a: the
/// same behind a length that is read and not used).
///
/// After the tokens a capping flag that is set in a direction that is not closed makes
/// the surface cap itself there (`SI_nurbsSrfForceCapping`, directions 0 and 1 for the
/// flags of tokens 14 and 15 with V open, 2 and 3 for 12 and 13 with U open).
pub fn nurbs_srf_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) -> Ptr32<NurbsSrf> {
    let q = nurbs_srf_allocate(m);
    if q.is_null() {
        return q;
    }
    let p = q.addr();
    const FRAME: u32 = 8;
    let frame = m.local(FRAME);
    let (scratch4, token_at) = (frame, frame + 4);
    let list_at = protos.addr().wrapping_add(4 * slot::NURBS_SRF as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        let token = m.i16(token_at);
        match token {
            1 => {
                byteswap_fread(m, o, p + srf::ORDER, 4, 1, file);
                byteswap_fread(m, o, p + srf::V + srf::ORDER, 4, 1, file);
            }
            2 => {
                byteswap_fread(m, o, p + srf::NB_POINTS, 4, 1, file);
                byteswap_fread(m, o, p + srf::V + srf::NB_POINTS, 4, 1, file);
                let n = m.i32(p + srf::NB_POINTS).wrapping_mul(m.i32(p + srf::V + srf::NB_POINTS));
                m.set_i32(p + srf::TOTAL_POINTS, n);
                let n = m.i32(p + srf::NB_POINTS).wrapping_mul(m.i32(p + srf::V + srf::NB_POINTS));
                let points = if alloc_fits(m, o, file, (n as u32).wrapping_mul(crv::POINT), n as u32) { nurbs_points_allocate(m, n) } else { 0 };
                m.set_u32(p + srf::POINTS, points);
                read_points(m, o, file, p + srf::TOTAL_POINTS, p + srf::POINTS);
            }
            3 => {
                byteswap_fread(m, o, p + srf::CLOSED, 2, 1, file);
                byteswap_fread(m, o, p + srf::V + srf::CLOSED, 2, 1, file);
            }
            4 => {
                byteswap_fread(m, o, p + srf::UNIFORMITY, 4, 1, file);
                byteswap_fread(m, o, p + srf::NB_KNOTS, 4, 1, file);
                read_knots(m, o, file, p + srf::NB_KNOTS, p + srf::KNOTS);
                byteswap_fread(m, o, p + srf::V + srf::NB_KNOTS, 4, 1, file);
                read_knots(m, o, file, p + srf::V + srf::NB_KNOTS, p + srf::V + srf::KNOTS);
            }
            5 => {
                for off in [srf::START, srf::END, srf::V + srf::START, srf::V + srf::END] {
                    double_fread(m, o, p + off, 8, 1, file);
                }
            }
            7 => read_composites(m, o, file, protos, p + srf::NB_TRIMS, p + srf::TRIMS),
            8..=11 | 0x10 | 0x11 => {
                let off = match token {
                    8..=11 => srf::STEPS + 4 * (token as u32 - 8),
                    _ => srf::CLOSING + 4 * (token as u32 - 0x10),
                };
                byteswap_fread(m, o, p + off, 4, 1, file);
            }
            6 | 0xc..=0xf | 0x12 | 0x13 | 0x17..=0x19 => {
                let off = match token {
                    6 => srf::BORDER,
                    0xc..=0xf => srf::CAPS + 2 * (token as u32 - 0xc),
                    0x12 => srf::APPROX_METHOD,
                    0x13 => srf::APPROX_VIEW,
                    0x17 => srf::APPROX_MIN,
                    0x18 => srf::APPROX_MAX,
                    _ => srf::APPROX_RECURSION,
                };
                byteswap_fread(m, o, p + off, 2, 1, file);
            }
            0x14..=0x16 => {
                float_fread(m, o, p + srf::APPROX_FLOATS + 4 * (token as u32 - 0x14), 4, 1, file);
            }
            -0xe6 | 0x1a => {
                if token != 0x1a {
                    byteswap_fread(m, o, scratch4, 4, 1, file);
                }
                read_composites(m, o, file, protos, p + srf::NB_PROJECTED, p + srf::PROJECTED);
            }
            t => {
                file_skip_field(m, o, file, t);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    for (cap, closed, direction) in [(srf::CAPS + 4, srf::V + srf::CLOSED, 0u32), (srf::CAPS + 6, srf::V + srf::CLOSED, 1), (srf::CAPS, srf::CLOSED, 2), (srf::CAPS + 2, srf::CLOSED, 3)] {
        if m.i16(p + cap) != 0 && m.i16(p + closed) == 0 {
            o.call(m, &SI_NURBS_SRF_FORCE_CAPPING, &[p, direction, 1]);
        }
    }
    m.release(FRAME);
    q
}
