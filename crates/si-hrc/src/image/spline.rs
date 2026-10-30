// SPDX-License-Identifier: MIT
//! Splines and faces on the image: the spline's allocators and copy of `spline.c`, the face
//! allocator of `face.c`, and `splineBinaryRead` and `faceBinaryRead` of `binary.c`
//! (finding 0004, "Patch, Spline, Spline key, Face").
//!
//! `si-types` has the records (`Spline` 0x74 bytes, `SplineKey` 0x30, `Face` 0x88); a
//! pointer is typed with them and the fields are addressed by the offsets of [`spline`]
//! and [`face`] below, each read from the listing. `si-spline` has the same five spline
//! routines on the image; it depends on this crate, so they are ported here a second time
//! (the lead reconciles duplicates).

use super::globals::{SI_IDENTITE, SPLINE_COUNT};
use super::io::{alloc_fits, byteswap_fread, file_skip_field, string_binary_read, vector_fread};
use super::matio::next_token;
use super::nurbs::{nurbs_crv_binary_read, nurbs_crv_dispose, nurbs_crv_init};
use super::outside::{Outside, Routine};
use super::proto::{slot, Prototypes};
use super::texture::matrix_copy;
use si_mem::{Mem, Ptr32};
pub use si_types::geom::{Face, Spline};
use si_x87::x87::F80;

/// Offsets in a spline (`SI_Spline`) and in one of its keys.
pub mod spline {
    /// The name: a string in a block of its own.
    pub const NAME: u32 = 0x00;
    pub const PHANTOM: u32 = 0x06;
    pub const CLOSED: u32 = 0x08;
    /// The type as an int: 0 linear, 1 Bézier, 2 B-spline, 3 cardinal.
    pub const TYPE: u32 = 0x0c;
    /// The number of keys (unsigned short) and their array.
    pub const NB_KEYS: u32 = 0x10;
    pub const KEYS: u32 = 0x14;
    pub const TENSION: u32 = 0x1c;
    pub const STEP: u32 = 0x20;
    pub const MATRIX: u32 = 0x24;
    /// A second array in a block of its own (the lengths), freed with the spline.
    pub const LENGTHS: u32 = 0x6c;
    pub const SIZE: u32 = 0x74;
    /// A key: the previous tangent, the position, the next tangent, a float that is 1 in a
    /// new key, and three shorts (constraint, segment, flag).
    pub const KEY: u32 = 0x30;
    pub const KEY_POSITION: u32 = 0x0c;
    pub const KEY_NEXT: u32 = 0x18;
    pub const KEY_CONSTRAINT: u32 = 0x28;
    pub const KEY_SEGMENT: u32 = 0x2a;
    pub const KEY_FLAG: u32 = 0x2c;
}

/// Offsets in a face (`SI_Face`).
pub mod face {
    /// The outline: a spline record inside the face.
    pub const OUTLINE: u32 = 0x04;
    /// The number of holes (or, in a face made of NURBS curves, of curves).
    pub const NB_HOLES: u32 = 0x78;
    /// The holes: an array of spline records.
    pub const HOLES: u32 = 0x7c;
    /// The curves of a NURBS face: an array of curve records of 100 bytes.
    pub const CURVES: u32 = 0x80;
    /// 1 for a face made of splines (the default), 0 for one made of NURBS curves.
    pub const IS_SPLINE: u32 = 0x84;
    pub const SIZE: u32 = 0x88;
}

/// `SI_splineUpdateMatrix (0x0848abd0)`, `spline.c`: the spline's basis matrix from its
/// type, tension and step.
pub const SI_SPLINE_UPDATE_MATRIX: Routine = Routine { name: "SI_splineUpdateMatrix", addr: 0x0848abd0 };
/// `SI_allDummy (0x0848e2f0)`, `spline.c`: recomputes the phantom keys of a spline.
pub const SI_ALL_DUMMY: Routine = Routine { name: "SI_allDummy", addr: 0x0848e2f0 };

/// [soft 0x0848a9f0 SI_splineInit]: the initial values of a spline record: no name, no
/// keys, 0x10 at +4, step 10, 2 and 1 at +0x64 and +0x66, the identity as its matrix. The
/// shorts at +0x0a, +0x12, +0x22, +0x6a and +0x72 are not written.
pub fn spline_init(m: &mut Mem, s: u32) {
    m.set_u32(s + spline::TENSION, 0);
    m.set_u32(s + spline::NAME, 0);
    m.set_u16(s + 4, 0x10);
    m.set_u16(s + spline::CLOSED, 0);
    m.set_u16(s + spline::PHANTOM, 0);
    m.set_u16(s + spline::NB_KEYS, 0);
    m.set_u32(s + spline::KEYS, 0);
    m.set_u32(s + 0x18, 0);
    m.set_u16(s + spline::STEP, 10);
    m.set_u32(s + spline::TYPE, 0);
    m.set_u16(s + 0x64, 2);
    m.set_u16(s + 0x66, 1);
    matrix_copy(m, SI_IDENTITE.addr(), s + spline::MATRIX);
    m.set_u16(s + 0x68, 0);
    m.set_u32(s + spline::LENGTHS, 0);
    m.set_u16(s + 0x70, 0);
}

/// [soft 0x0848a990 SI_splineAllocate]: a new spline, named `spl` and the running count
/// of splines made (`sprintf` with `spl%1hd`, then `strdup`).
pub fn spline_allocate(m: &mut Mem) -> Ptr32<Spline> {
    let p = m.si_malloc(spline::SIZE);
    if p != 0 {
        spline_init(m, p);
        let count = m[SPLINE_COUNT].wrapping_add(1);
        m[SPLINE_COUNT] = count;
        let mut t = b"spl".to_vec();
        t.extend_from_slice(&si_mem::clib::decimal(count as i16 as i32));
        let scratch = m.local(0x50);
        m.put_cstr(scratch, &t);
        let copy = m.strdup(scratch);
        m.release(0x50);
        m.set_u32(p + spline::NAME, copy);
    }
    Ptr32::new(p)
}

/// [soft 0x0848f8dc SI_splineKeyAllocate]: `n` keys in a zeroed block of the next multiple
/// of 16 records; each of the `n` is then cleared again field by field and gets 1.0 at
/// +0x24.
pub fn spline_key_allocate(m: &mut Mem, n: i32) -> u32 {
    let rounded = (n as u32).wrapping_sub(((n as u32).wrapping_sub(1) & 0xf).wrapping_sub(0xf));
    let p = m.si_calloc(rounded, spline::KEY);
    let mut k = p;
    let mut left = n;
    if 0 < n {
        loop {
            for off in [8, 4, 0, 0x14, 0x10, 0x0c, 0x20, 0x1c, 0x18] {
                m.set_u32(k + off, 0);
            }
            m.set_u32(k + 0x24, 0x3f80_0000);
            m.set_u16(k + 0x28, 0);
            m.set_u16(k + 0x2a, 0);
            m.set_u16(k + 0x2c, 0);
            left -= 1;
            k = k.wrapping_add(spline::KEY);
            if left == 0 {
                break;
            }
        }
    }
    p
}

/// [soft 0x0848aa9c SI_splineTransfer]: the content of `src` into `dst`: the closed and
/// phantom flags, the number of keys with a new key array holding copies of the keys, the
/// step, type, tension and matrix. The name is not copied, and what `dst` had as its key
/// array is not freed.
pub fn spline_transfer(m: &mut Mem, src: u32, dst: u32) {
    let v = m.u16(src + spline::CLOSED);
    m.set_u16(dst + spline::CLOSED, v);
    let v = m.u16(src + spline::PHANTOM);
    m.set_u16(dst + spline::PHANTOM, v);
    let v = m.u16(src + spline::NB_KEYS);
    m.set_u16(dst + spline::NB_KEYS, v);
    let keys = spline_key_allocate(m, m.u16(src + spline::NB_KEYS) as i32);
    m.set_u32(dst + spline::KEYS, keys);
    if m.u16(src + spline::NB_KEYS) != 0 {
        let mut i: i32 = 0;
        loop {
            let from = m.u32(src + spline::KEYS).wrapping_add(spline::KEY * i as u32);
            let to = m.u32(dst + spline::KEYS).wrapping_add(spline::KEY * i as u32);
            for w in 0..12 {
                let v = m.u32(from + 4 * w);
                m.set_u32(to + 4 * w, v);
            }
            i += 1;
            if !(i < m.u16(src + spline::NB_KEYS) as i32) {
                break;
            }
        }
    }
    let v = m.u16(src + spline::STEP);
    m.set_u16(dst + spline::STEP, v);
    let v = m.u32(src + spline::TYPE);
    m.set_u32(dst + spline::TYPE, v);
    let v = m.u32(src + spline::TENSION);
    m.set_u32(dst + spline::TENSION, v);
    matrix_copy(m, src + spline::MATRIX, dst + spline::MATRIX);
}

/// [soft 0x0848aa74 SI_splineCopy]: a new spline with the content of `src`
/// ([`spline_transfer`]); it keeps the name the allocator gave it.
pub fn spline_copy(m: &mut Mem, src: u32) -> Ptr32<Spline> {
    let p = spline_allocate(m);
    if !p.is_null() {
        spline_transfer(m, src, p.addr());
    }
    p
}

/// [soft 0x08490168 SI_bezierShrink]: a new spline made of an old Bézier point list, in
/// which every third point is a key's position and the two between are tangents.
///
/// For a spline of another type than 1 the answer is a plain copy. Otherwise the new
/// spline has the source's closed flag and step, type 1, its matrix brought up to date
/// (`SI_splineUpdateMatrix`), and (points − 1) / 3 + 1 keys for an open source, points / 3
/// for a closed one. Point 3i is key i's position, point 3i + 1 its next tangent, point
/// 3i + 2 the previous tangent of the key after it, which for the last key of a closed
/// spline is the first key again. An open spline's two outer tangents are mirrored: the
/// first key's previous tangent is twice its position minus its next tangent, the last
/// key's next tangent twice its position minus its previous one (each sum and difference
/// formed in the x87 register and stored as a float), and both keys get constraint 2.
pub fn bezier_shrink(m: &mut Mem, o: &mut dyn Outside, src: u32) -> Ptr32<Spline> {
    if m.i32(src + spline::TYPE) != 1 {
        return spline_copy(m, src);
    }
    let new = spline_allocate(m);
    if new.is_null() {
        return new;
    }
    let p = new.addr();
    let v = m.u16(src + spline::CLOSED);
    m.set_u16(p + spline::CLOSED, v);
    let v = m.u16(src + spline::STEP);
    m.set_u16(p + spline::STEP, v);
    m.set_i32(p + spline::TYPE, 1);
    o.call(m, &SI_SPLINE_UPDATE_MATRIX, &[p]);
    let points = m.u16(src + spline::NB_KEYS);
    let keys = if m.i16(src + spline::CLOSED) == 0 { (((points as i32 - 1) / 3) as i16).wrapping_add(1) as u16 } else { points / 3 };
    m.set_u16(p + spline::NB_KEYS, keys);
    let mut filled: i32 = 0;
    let mut k = spline_key_allocate(m, m.u16(p + spline::NB_KEYS) as i32);
    m.set_u32(p + spline::KEYS, k);
    let mut from = m.u32(src + spline::KEYS);
    let mut i: i32 = 0;
    let vector = |m: &mut Mem, from: u32, to: u32| {
        for w in 0..3 {
            let v = m.u32(from.wrapping_add(4 * w));
            m.set_u32(to.wrapping_add(4 * w), v);
        }
    };
    if m.u16(src + spline::NB_KEYS) != 0 {
        loop {
            vector(m, from.wrapping_add(spline::KEY_POSITION), k.wrapping_add(spline::KEY_POSITION));
            if m.u16(src + spline::NB_KEYS) as i32 <= i + 1 {
                break;
            }
            vector(m, from.wrapping_add(spline::KEY + spline::KEY_POSITION), k.wrapping_add(spline::KEY_NEXT));
            if filled == m.u16(p + spline::NB_KEYS) as i32 - 1 {
                filled = 0;
                k = m.u32(p + spline::KEYS);
            } else {
                filled += 1;
                k = k.wrapping_add(spline::KEY);
            }
            if m.u16(src + spline::NB_KEYS) as i32 <= i + 2 {
                break;
            }
            vector(m, from.wrapping_add(2 * spline::KEY + spline::KEY_POSITION), k);
            i += 3;
            from = from.wrapping_add(3 * spline::KEY);
            if !(i < m.u16(src + spline::NB_KEYS) as i32) {
                break;
            }
        }
    }
    if m.i16(p + spline::CLOSED) == 0 {
        // twice the position minus the other tangent, per component
        let mirror = |m: &mut Mem, key: u32, other: u32, to: u32, w: u32| {
            let pos = F80::from_f32(m.f32(key.wrapping_add(spline::KEY_POSITION + 4 * w)));
            let v = (pos + pos - F80::from_f32(m.f32(key.wrapping_add(other + 4 * w)))).to_f32();
            m.set_f32(key.wrapping_add(to + 4 * w), v);
        };
        let first = m.u32(p + spline::KEYS);
        mirror(m, first, spline::KEY_NEXT, 0, 0);
        mirror(m, first, spline::KEY_NEXT, 0, 1);
        m.set_u16(first.wrapping_add(spline::KEY_CONSTRAINT), 2);
        mirror(m, first, spline::KEY_NEXT, 0, 2);
        let last = m.u32(p + spline::KEYS).wrapping_sub(spline::KEY).wrapping_add(m.u16(p + spline::NB_KEYS) as u32 * spline::KEY);
        mirror(m, last, 0, spline::KEY_NEXT, 0);
        mirror(m, last, 0, spline::KEY_NEXT, 1);
        m.set_u16(last.wrapping_add(spline::KEY_CONSTRAINT), 2);
        mirror(m, last, 0, spline::KEY_NEXT, 2);
    }
    new
}

/// [soft 0x0848ab48 SI_splineDispose]: frees the spline `*pp` points to (its key array,
/// the block at +0x6c, its name, the record) and clears `*pp`.
pub fn spline_dispose(m: &mut Mem, pp: u32) {
    if m.u32(pp) != 0 {
        for off in [spline::KEYS, spline::LENGTHS, spline::NAME] {
            let s = m.u32(pp);
            if m.u32(s + off) != 0 {
                m.free(m.u32(s + off));
                let s = m.u32(pp);
                m.set_u32(s + off, 0);
            }
        }
        if m.u32(pp) != 0 {
            m.free(m.u32(pp));
            m.set_u32(pp, 0);
        }
        m.set_u32(pp, 0);
    }
}

/// [soft 0x083f00dc splineBinaryRead]: a spline from the file.
///
/// Tokens: the type as a short (1), the closed flag (3), the tension (4, four bytes
/// swapped), the step (5), the phantom flag (6), the name (8; the name the allocator gave
/// is freed first), the keys (7: an unsigned short count, a key array of that many, and
/// per key the tokens of the spline key's list: three vectors 1 to 3, three shorts 4 to
/// 6), and the old point list (2: a count and one vector per key, its position).
///
/// After the tokens a spline of type 1 that had the old point list is replaced by what
/// [`bezier_shrink`] makes of it, and disposed of. Then the basis matrix is brought up to
/// date (`SI_splineUpdateMatrix`), and for a spline with the phantom flag set the phantom
/// keys (`SI_allDummy`).
///
/// The key loop counts in a short against the unsigned count: with more than 32,767 keys
/// it never ends in the original (the port's ends at the fault that follows).
pub fn spline_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) -> Ptr32<Spline> {
    let s = spline_allocate(m);
    if s.is_null() {
        return s;
    }
    // the frame: the spline's pointer (handed to `SI_splineDispose` by address), the
    // scratch short and the token
    const FRAME: u32 = 8;
    let frame = m.local(FRAME);
    let (spline_at, scratch, token_at) = (frame, frame + 4, frame + 6);
    m.set_u32(spline_at, s.addr());
    let p = s.addr();
    let table = protos.addr();
    let list_at = table.wrapping_add(4 * slot::SPLINE as u32);
    let key_list_at = table.wrapping_add(4 * slot::SPLINE_KEY as u32);
    let mut old_points = false;
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        match m.i16(token_at) {
            1 => {
                byteswap_fread(m, o, scratch, 2, 1, file);
                let v = m.i16(scratch) as i32;
                m.set_i32(p + spline::TYPE, v);
            }
            2 => {
                old_points = true;
                byteswap_fread(m, o, p + spline::NB_KEYS, 2, 1, file);
                let mut k = key_array(m, o, file, m.u16(p + spline::NB_KEYS));
                m.set_u32(p + spline::KEYS, k);
                let mut left = m.i16(p + spline::NB_KEYS);
                while 0 < left {
                    vector_fread(m, o, k.wrapping_add(spline::KEY_POSITION), 0xc, 1, file);
                    k = k.wrapping_add(spline::KEY);
                    left -= 1;
                }
            }
            3 => {
                byteswap_fread(m, o, p + spline::CLOSED, 2, 1, file);
            }
            4 => {
                byteswap_fread(m, o, p + spline::TENSION, 4, 1, file);
            }
            5 => {
                byteswap_fread(m, o, p + spline::STEP, 2, 1, file);
            }
            6 => {
                byteswap_fread(m, o, p + spline::PHANTOM, 2, 1, file);
            }
            7 => {
                byteswap_fread(m, o, p + spline::NB_KEYS, 2, 1, file);
                let mut k = key_array(m, o, file, m.u16(p + spline::NB_KEYS));
                m.set_u32(p + spline::KEYS, k);
                let mut i: i16 = 0;
                if m.i16(p + spline::NB_KEYS) != 0 {
                    loop {
                        let mut key_index: i16 = 0;
                        next_token(m, o, file, key_list_at, token_at, &mut key_index);
                        i = i.wrapping_add(1);
                        while m.i16(token_at) != 0 {
                            match m.i16(token_at) {
                                1 => {
                                    vector_fread(m, o, k, 0xc, 1, file);
                                }
                                2 => {
                                    vector_fread(m, o, k.wrapping_add(spline::KEY_POSITION), 0xc, 1, file);
                                }
                                3 => {
                                    vector_fread(m, o, k.wrapping_add(spline::KEY_NEXT), 0xc, 1, file);
                                }
                                4 => {
                                    byteswap_fread(m, o, k.wrapping_add(spline::KEY_CONSTRAINT), 2, 1, file);
                                }
                                5 => {
                                    byteswap_fread(m, o, k.wrapping_add(spline::KEY_SEGMENT), 2, 1, file);
                                }
                                6 => {
                                    byteswap_fread(m, o, k.wrapping_add(spline::KEY_FLAG), 2, 1, file);
                                }
                                t => {
                                    file_skip_field(m, o, file, t);
                                }
                            }
                            next_token(m, o, file, key_list_at, token_at, &mut key_index);
                        }
                        k = k.wrapping_add(spline::KEY);
                        if !((i as i32) < m.u16(p + spline::NB_KEYS) as i32) {
                            break;
                        }
                    }
                }
            }
            8 => {
                if m.u32(p + spline::NAME) != 0 {
                    m.free(m.u32(p + spline::NAME));
                    m.set_u32(p + spline::NAME, 0);
                }
                let name = string_binary_read(m, o, file);
                m.set_u32(p + spline::NAME, name);
            }
            t => {
                file_skip_field(m, o, file, t);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    if old_points && m.i32(p + spline::TYPE) == 1 {
        let shrunk = bezier_shrink(m, o, p);
        spline_dispose(m, spline_at);
        m.set_u32(spline_at, shrunk.addr());
    }
    let result = m.u32(spline_at);
    o.call(m, &SI_SPLINE_UPDATE_MATRIX, &[result]);
    if m.i16(result.wrapping_add(spline::PHANTOM)) != 0 {
        o.call(m, &SI_ALL_DUMMY, &[result]);
    }
    m.release(FRAME);
    Ptr32::new(result)
}

/// The key array for a count read from the file (the original calls
/// `SI_splineKeyAllocate` with it; see `io::alloc_fits` for the bound this project adds).
fn key_array(m: &mut Mem, o: &mut dyn Outside, file: u32, n: u16) -> u32 {
    let rounded = (n as u32).wrapping_sub(((n as u32).wrapping_sub(1) & 0xf).wrapping_sub(0xf));
    if alloc_fits(m, o, file, rounded.wrapping_mul(spline::KEY), n as u32) {
        spline_key_allocate(m, n as i32)
    } else {
        0
    }
}

/// [soft 0x084347d0 SI_faceAllocate]: a new face: a zero at +0, an initialised outline,
/// no holes, no curves, made of splines. The shorts at +0x7a and +0x86 are not written.
pub fn face_allocate(m: &mut Mem) -> Ptr32<Face> {
    let p = m.si_malloc(face::SIZE);
    if p != 0 {
        m.set_u32(p, 0);
        spline_init(m, p + face::OUTLINE);
        m.set_u16(p + face::NB_HOLES, 0);
        m.set_u32(p + face::HOLES, 0);
        m.set_u32(p + face::CURVES, 0);
        m.set_u16(p + face::IS_SPLINE, 1);
    }
    Ptr32::new(p)
}

/// [soft 0x083f043c faceBinaryRead]: a face from the file.
///
/// Token 3 says what the face is made of. For a face of splines, token 1 is the outline
/// (a spline is read, its content transferred into the face's own record, and disposed of)
/// and token 2 the holes (a count; if positive a zeroed array of that many spline records,
/// each filled the same way). For a face of NURBS curves, token 4 is its curves (a count,
/// a zeroed array of that many curve records of 100 bytes, also for a count of zero or
/// below, each filled from a curve that is read, copied with `SI_nurbsCrvInit` and
/// disposed of). A token that does not apply reads nothing. Anything else is skipped.
///
/// A spline's name is not transferred: the outline and the holes have none.
pub fn face_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) -> Ptr32<Face> {
    let f = face_allocate(m);
    if f.is_null() {
        return f;
    }
    let p = f.addr();
    // the frame: the spline and the curve being read (handed to their dispose routines by
    // address), and the token
    const FRAME: u32 = 12;
    let frame = m.local(FRAME);
    let (spline_at, curve_at, token_at) = (frame, frame + 4, frame + 8);
    m.set_u32(spline_at, 0);
    m.set_u32(curve_at, 0);
    let list_at = protos.addr().wrapping_add(4 * slot::FACE as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        match m.i16(token_at) {
            2 => {
                if m.i16(p + face::IS_SPLINE) != 0 {
                    byteswap_fread(m, o, p + face::NB_HOLES, 2, 1, file);
                    if 0 < m.i16(p + face::NB_HOLES) {
                        let n = m.i16(p + face::NB_HOLES) as u32;
                        let holes = if alloc_fits(m, o, file, n * spline::SIZE, n) { m.si_calloc(n, spline::SIZE) } else { 0 };
                        m.set_u32(p + face::HOLES, holes);
                    }
                    let mut i: i16 = 0;
                    while i < m.i16(p + face::NB_HOLES) {
                        let s = spline_binary_read(m, o, file, protos);
                        m.set_u32(spline_at, s.addr());
                        let to = m.u32(p + face::HOLES).wrapping_add((i as i32 as u32).wrapping_mul(spline::SIZE));
                        spline_transfer(m, m.u32(spline_at), to);
                        spline_dispose(m, spline_at);
                        i = i.wrapping_add(1);
                    }
                }
            }
            1 => {
                if m.i16(p + face::IS_SPLINE) != 0 {
                    let s = spline_binary_read(m, o, file, protos);
                    m.set_u32(spline_at, s.addr());
                    spline_transfer(m, m.u32(spline_at), p + face::OUTLINE);
                    spline_dispose(m, spline_at);
                }
            }
            3 => {
                byteswap_fread(m, o, p + face::IS_SPLINE, 2, 1, file);
            }
            4 => {
                if m.i16(p + face::IS_SPLINE) == 0 {
                    byteswap_fread(m, o, p + face::NB_HOLES, 2, 1, file);
                    let n = m.i16(p + face::NB_HOLES) as i32 as u32;
                    let curves = if alloc_fits(m, o, file, n.wrapping_mul(100), n) { m.si_calloc(n, 100) } else { 0 };
                    m.set_u32(p + face::CURVES, curves);
                    let mut i: i16 = 0;
                    while i < m.i16(p + face::NB_HOLES) {
                        let c = nurbs_crv_binary_read(m, o, file, protos);
                        m.set_u32(curve_at, c.addr());
                        let to = m.u32(p + face::CURVES).wrapping_add((i as i32 as u32).wrapping_mul(100));
                        nurbs_crv_init(m, to, m.u32(curve_at));
                        nurbs_crv_dispose(m, curve_at);
                        i = i.wrapping_add(1);
                    }
                }
            }
            t => {
                file_skip_field(m, o, file, t);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    m.release(FRAME);
    f
}
