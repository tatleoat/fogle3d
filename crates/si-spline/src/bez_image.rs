// SPDX-License-Identifier: MIT
//! `bez.c`: the ray tracer's Bézier patch record (`BezPatch`, 0x7b0 bytes) on the one memory
//! image (finding 0040, section 1).
//!
//! Read from code: a patch holds its two orders (`sOrder` at +0, `tOrder` at +4), a copy of
//! a 48-byte box record (twelve words, at +8; the dump prints the doubles at +8, +0x10,
//! +0x20 and +0x28 as the original rectangle's `umin`, `vmin`, `umax`, `vmax`), two point
//! arrays of 0x24-byte entries (four doubles and a word; +0x98 the 2D points, +0x9c the 3D
//! points) and room for 25 entries of each inside the record (+0xa0 and +0x424). Up to 25
//! points the arrays are that inline room; beyond that each is a block of its own, whose
//! addresses the record keeps at +0x7a8 and +0x7ac so that they can be freed.
//!
//! `free` and `SI_calloc` are the image's ([`si_mem::Mem::free`], [`si_mem::Mem::si_calloc`]);
//! `fprintf` goes through [`Outside`], format texts at their `.data` addresses, `double`s as
//! two words.

use si_mem::{Mem, Outside, Routine};

/// The C library's `fprintf`, through the binary's stub.
pub const FPRINTF: Routine = Routine::new("fprintf", 0x080c_3b88);

/// Field offsets of the record.
pub mod off {
    pub const S_ORDER: u32 = 0;
    pub const T_ORDER: u32 = 4;
    pub const BBOX: u32 = 8;
    /// The 2D point array (a pointer).
    pub const PTS2: u32 = 0x98;
    /// The 3D point array (a pointer).
    pub const PTS3: u32 = 0x9c;
    /// Inline room for 25 points of each kind.
    pub const INLINE2: u32 = 0xa0;
    pub const INLINE3: u32 = 0x424;
    /// The blocks, when the arrays do not fit the inline room.
    pub const HEAP2: u32 = 0x7a8;
    pub const HEAP3: u32 = 0x7ac;
    pub const SIZE: u32 = 0x7b0;
    /// One point: four doubles and a word.
    pub const POINT: u32 = 0x24;
}

/// The texts the dump prints, in `.data`.
pub mod text {
    pub const DUMPING: u32 = 0x086d_ac40;
    pub const EMPTY: u32 = 0x086d_ac5e;
    pub const RECT: u32 = 0x086d_ac80;
    pub const ORDERS: u32 = 0x086d_acb4;
    pub const NO_POINTS: u32 = 0x086d_acca;
    pub const POINTS: u32 = 0x086d_ace4;
    pub const POINT3: u32 = 0x086d_ad00;
    pub const POINT2: u32 = 0x086d_ad40;
    pub const NEWLINE: u32 = 0x086d_ad69;
}

fn copy_box(m: &mut Mem, dst: u32, src: u32) {
    for i in 0..12 {
        let w = m.u32(src.wrapping_add(4 * i));
        m.set_u32(dst.wrapping_add(4 * i), w);
    }
}

/// A new patch record of the given orders with a copy of a box [soft 0x08503460 si_bezPatchCreate];
/// null when the allocation fails. A record whose point count `sOrder * tOrder` is at most 25
/// (as a signed number) uses its inline arrays, otherwise two blocks of `count` points.
pub fn create(m: &mut Mem, s_order: i32, t_order: i32, bbox: u32) -> u32 {
    let p = m.si_calloc(1, off::SIZE);
    if p == 0 {
        return 0;
    }
    m.set_i32(p + off::S_ORDER, s_order);
    m.set_i32(p + off::T_ORDER, t_order);
    copy_box(m, p + off::BBOX, bbox);
    let n = s_order.wrapping_mul(t_order);
    if n < 0x1a {
        m.set_u32(p + off::PTS2, p + off::INLINE2);
        m.set_u32(p + off::PTS3, p + off::INLINE3);
    } else {
        let a = m.si_calloc(n as u32, off::POINT);
        m.set_u32(p + off::HEAP2, a);
        let b = m.si_calloc(n as u32, off::POINT);
        m.set_u32(p + off::HEAP3, b);
        m.set_u32(p + off::PTS2, a);
        m.set_u32(p + off::PTS3, b);
    }
    p
}

/// The record freed [soft 0x08503504 SI_bezPatchDestroy]: `pp` is the address of a word that
/// holds the record's address. Nothing for a null `pp` or a null record; otherwise the two
/// blocks (when there are any), the point pointers cleared, the record freed and the word
/// cleared.
pub fn destroy(m: &mut Mem, pp: u32) {
    if pp == 0 || m.u32(pp) == 0 {
        return;
    }
    let r = m.u32(pp);
    let a = m.u32(r + off::HEAP2);
    if a != 0 {
        m.free(a);
        let r = m.u32(pp);
        m.set_u32(r + off::HEAP2, 0);
    }
    let r = m.u32(pp);
    let b = m.u32(r + off::HEAP3);
    if b != 0 {
        m.free(b);
        let r = m.u32(pp);
        m.set_u32(r + off::HEAP3, 0);
    }
    let r = m.u32(pp);
    m.set_u32(r + off::PTS2, 0);
    let r = m.u32(pp);
    m.set_u32(r + off::PTS3, 0);
    let r = m.u32(pp);
    if r != 0 {
        m.free(r);
        m.set_u32(pp, 0);
    }
    m.set_u32(pp, 0);
}

fn dbl(m: &Mem, a: u32) -> [u32; 2] {
    [m.u32(a), m.u32(a.wrapping_add(4))]
}

/// Prints the record to `file` [soft 0x08503598 si_bezPatchDump]: one `fprintf` per piece
/// of text.
pub fn dump(m: &mut Mem, o: &mut dyn Outside, p: u32, file: u32) {
    o.call(m, FPRINTF, &[file, text::DUMPING, p]);
    if p == 0 {
        o.call(m, FPRINTF, &[file, text::EMPTY]);
        return;
    }
    let (a, b, c, d) = (dbl(m, p + 8), dbl(m, p + 0x10), dbl(m, p + 0x20), dbl(m, p + 0x28));
    o.call(m, FPRINTF, &[file, text::RECT, a[0], a[1], b[0], b[1], c[0], c[1], d[0], d[1]]);
    let (so, to) = (m.i32(p), m.i32(p + 4));
    o.call(m, FPRINTF, &[file, text::ORDERS, so as u32, to as u32]);
    if m.u32(p + off::PTS3) == 0 {
        o.call(m, FPRINTF, &[file, text::NO_POINTS]);
    } else {
        o.call(m, FPRINTF, &[file, text::POINTS]);
        let mut i: i32 = 0;
        let mut at: u32 = 0;
        while i < m.i32(p).wrapping_mul(m.i32(p + 4)) {
            let q = m.u32(p + off::PTS3).wrapping_add(at);
            let mut args = vec![file, text::POINT3, i as u32];
            for k in [0, 8, 0x10, 0x18] {
                args.extend(dbl(m, q.wrapping_add(k)));
            }
            o.call(m, FPRINTF, &args);
            let q = m.u32(p + off::PTS2).wrapping_add(at);
            let mut args = vec![file, text::POINT2, i as u32];
            for k in [0, 8, 0x10, 0x18] {
                args.extend(dbl(m, q.wrapping_add(k)));
            }
            o.call(m, FPRINTF, &args);
            at = at.wrapping_add(off::POINT);
            i += 1;
        }
    }
    o.call(m, FPRINTF, &[file, text::NEWLINE]);
}

/// Fills a caller's record [soft 0x08503724 si_bezPatchInit]: like [`create`] on a record the
/// caller owns; the unused block pointers are cleared; 0 when the second block could not be
/// allocated, else 1.
pub fn init(m: &mut Mem, s_order: i32, t_order: i32, bbox: u32, p: u32) -> u32 {
    m.set_i32(p + off::S_ORDER, s_order);
    m.set_i32(p + off::T_ORDER, t_order);
    copy_box(m, p + off::BBOX, bbox);
    let n = s_order.wrapping_mul(t_order);
    if n < 0x1a {
        m.set_u32(p + off::PTS2, p + off::INLINE2);
        m.set_u32(p + off::PTS3, p + off::INLINE3);
        m.set_u32(p + off::HEAP2, 0);
        m.set_u32(p + off::HEAP3, 0);
    } else {
        let a = m.si_calloc(n as u32, off::POINT);
        m.set_u32(p + off::HEAP2, a);
        let b = m.si_calloc(n as u32, off::POINT);
        m.set_u32(p + off::HEAP3, b);
        m.set_u32(p + off::PTS2, a);
        m.set_u32(p + off::PTS3, b);
        if b == 0 {
            return 0;
        }
    }
    1
}

/// The blocks of a caller's record freed and the point pointers cleared
/// [soft 0x085037cc si_bezPatchCleanup]; the record's address comes back.
pub fn cleanup(m: &mut Mem, p: u32) -> u32 {
    let a = m.u32(p + off::HEAP2);
    if a != 0 {
        m.free(a);
        m.set_u32(p + off::HEAP2, 0);
    }
    let b = m.u32(p + off::HEAP3);
    if b != 0 {
        m.free(b);
        m.set_u32(p + off::HEAP3, 0);
    }
    m.set_u32(p + off::PTS2, 0);
    m.set_u32(p + off::PTS3, 0);
    p
}
