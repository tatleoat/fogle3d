// SPDX-License-Identifier: MIT
//! The three "are these the same" tests of the material and texture files (finding 0023,
//! section 36), on the memory image: `si_textureEqual` (`t2d_fct.c`), `si_texture3DEqual`
//! (`t3d_fct.c`) and `si_materialEqual` (`mat_fct.c`). They decide whether two materials may
//! share one render record; the 3D and 2D tests are the ones the material test calls.
//!
//! Every float test is the original's: both floats are loaded exactly into the register,
//! subtracted there (rounded to the register's 64-bit mantissa, not to a float), the absolute
//! value is compared with the double 0.0001 and passes only when it is below it (a NaN fails);
//! two floats compared for equality pass only when equal and neither is a NaN (`fucompp`).
//! Class: model (the register subtraction).

use si_mem::Mem;
use si_x87::{x, F80};

/// 0x08655e98, 0x08654d40 and 0x08656330 (double): the tolerance of the three tests.
const EPS: f64 = 1e-4;

/// `|a - b| < 0.0001`, `a` and `b` floats at the two records' offset.
fn near(m: &Mem, a: u32, b: u32, off: u32) -> bool {
    (x(m.f32(a + off)) - x(m.f32(b + off))).abs().lt(x(EPS))
}

/// Floats equal by `fucompp` (C3 set, C2 clear): equal and ordered.
fn same_f32(m: &Mem, a: u32, b: u32, off: u32) -> bool {
    x(m.f32(a + off)).eq(x(m.f32(b + off)))
}

/// `|a - b| < 0.0001` for two 16-bit words (sign-extended, subtracted as ints, `fild`).
fn near_i16(m: &Mem, a: u32, b: u32, off: u32) -> bool {
    let d = i32::from(m.i16(a + off)).wrapping_sub(i32::from(m.i16(b + off)));
    F80::from_i32(d).abs().lt(x(EPS))
}

/// Same pointer is "equal" in the two texture tests, a null one is not.
fn pre_texture(a: u32, b: u32) -> Option<bool> {
    if a == b {
        Some(true)
    } else if a == 0 || b == 0 {
        Some(false)
    } else {
        None
    }
}

/// Two 2D textures [soft 0x082b2d1c si_textureEqual]: `si_textureEqual(a, b)`. Same pointer
/// 1, a null one 0. Else, in the original's order, 0 at the first difference of: the ints at
/// +0x28; the words at +0x2c, +0x18e, +0x2e; the words at +0x30, +0x34, +0x32, +0x36 (signed,
/// differing by less than 0.0001, so equal); the floats at +0x58, +0x5c, +0x68, +0x6c,
/// +0x94, +0x98, +0x9c (within 0.0001); the ints +0xf8, +0xfc, +0x100; the words +0x104,
/// +0x10a; the float +0x10c (exactly); the word +0x110; the floats +0x128, +0x12c, +0x144,
/// +0x130, +0x138, +0x13c, +0x140, +0x168 (within 0.0001); +0x198 non-zero in either; the int
/// +0x1cc. Then the answer is whether the two textures' next ones (+0x16c) are equal in turn.
pub fn texture_equal(m: &Mem, a: u32, b: u32) -> i32 {
    let (mut a, mut b) = (a, b);
    // the original recurses on the +0x16c pair and answers with that call's truth value
    loop {
        if let Some(r) = pre_texture(a, b) {
            return i32::from(r);
        }
        let ok = m.i32(a + 0x28) == m.i32(b + 0x28)
            && m.u16(a + 0x2c) == m.u16(b + 0x2c)
            && m.u16(a + 0x18e) == m.u16(b + 0x18e)
            && m.u16(a + 0x2e) == m.u16(b + 0x2e)
            && near_i16(m, a, b, 0x30)
            && near_i16(m, a, b, 0x34)
            && near_i16(m, a, b, 0x32)
            && near_i16(m, a, b, 0x36)
            && [0x58, 0x5c, 0x68, 0x6c, 0x94, 0x98, 0x9c].iter().all(|&o| near(m, a, b, o))
            && m.i32(a + 0xf8) == m.i32(b + 0xf8)
            && m.i32(a + 0xfc) == m.i32(b + 0xfc)
            && m.i32(a + 0x100) == m.i32(b + 0x100)
            && m.u16(a + 0x104) == m.u16(b + 0x104)
            && m.u16(a + 0x10a) == m.u16(b + 0x10a)
            && same_f32(m, a, b, 0x10c)
            && m.u16(a + 0x110) == m.u16(b + 0x110)
            && [0x128, 0x12c, 0x144, 0x130, 0x138, 0x13c, 0x140, 0x168].iter().all(|&o| near(m, a, b, o))
            && m.i32(a + 0x198) == 0
            && m.i32(b + 0x198) == 0
            && m.i32(a + 0x1cc) == m.i32(b + 0x1cc);
        if !ok {
            return 0;
        }
        let (na, nb) = (m.u32(a + 0x16c), m.u32(b + 0x16c));
        a = na;
        b = nb;
    }
}

/// The offsets of `si_texture3DEqual`'s float tests, in the original's order (the ints and
/// words at +0xc, +0x1c, +0x24, +0x2c sit between them).
const T3D_NEAR: &[u32] = &[
    0x10, 0x14, 0x18, 0x20, 0x28, 0x30, 0x34, 0x38, 0x3c, 0x40, 0x44, 0x48, 0x4c, 0x50, 0x54, 0x58, 0x5c, 0x60,
    0x64, 0x68, 0x6c, 0x70, 0x74, 0x78, 0x7c, 0x80, 0x84, 0x88, 0x90, 0x94, 0x98, 0x9c, 0xa0, 0xa4, 0xa8, 0xac,
    0xb0, 0x108, 0x10c, 0x124, 0x110, 0x118, 0x11c, 0x120,
];

/// Two 3D textures [soft 0x082b92c0 si_texture3DEqual]: `si_texture3DEqual(a, b)`. Same
/// pointer 1, a null one 0. Else 1 when: the ints at +0xc and +0x1c are equal; the words at
/// +0x24 and +0x2c are equal; the floats at +0x10, +0x14, +0x18, +0x20, +0x28, +0x30 to
/// +0x74 in steps of 4, +0x78 to +0x88, +0x90 to +0xb0, +0x108, +0x10c, +0x124, +0x110,
/// +0x118, +0x11c, +0x120 are each within 0.0001; and +0x14c is zero in both.
pub fn texture3d_equal(m: &Mem, a: u32, b: u32) -> i32 {
    if let Some(r) = pre_texture(a, b) {
        return i32::from(r);
    }
    let ok = m.i32(a + 0xc) == m.i32(b + 0xc)
        && T3D_NEAR.iter().all(|&o| near(m, a, b, o))
        && m.i32(a + 0x1c) == m.i32(b + 0x1c)
        && m.u16(a + 0x24) == m.u16(b + 0x24)
        && m.u16(a + 0x2c) == m.u16(b + 0x2c)
        && m.i32(a + 0x14c) == 0
        && m.i32(b + 0x14c) == 0;
    i32::from(ok)
}

/// Two materials [soft 0x08288ca4 si_materialEqual]: `si_materialEqual(a, b)`. The same
/// pointer or a null one is 0 (unlike the texture tests: a material is never "equal" to
/// itself). Else 1 when, in the original's order: the floats at +0x34 to +0x60 in steps of 4
/// are each within 0.0001; the words +0x1e, +0x20 are equal; the floats +0x6a8, +0x6b8,
/// +0x6dc are within 0.0001; the float +0x64 is equal; the 2D textures at +0x6ec are equal;
/// the word +0x6e8 is equal, unless both are not above zero (as signed words); the ints
/// +0x6f4 equal; the 3D textures at +0x6fc equal; the word +0x758 equal; the floats +0x75c
/// and +0x760 equal; the ints +0xdb0 to +0xdbc zero in both; the int +0xdc8 equal.
pub fn material_equal(m: &Mem, a: u32, b: u32) -> i32 {
    if a == b || a == 0 || b == 0 {
        return 0;
    }
    let mut ok = (0x34..=0x60).step_by(4).all(|o| near(m, a, b, o))
        && m.u16(a + 0x1e) == m.u16(b + 0x1e)
        && m.u16(a + 0x20) == m.u16(b + 0x20)
        && [0x6a8, 0x6b8, 0x6dc].iter().all(|&o| near(m, a, b, o))
        && same_f32(m, a, b, 0x64)
        && texture_equal(m, m.u32(a + 0x6ec), m.u32(b + 0x6ec)) != 0;
    if ok {
        let (da, db) = (m.i16(a + 0x6e8), m.i16(b + 0x6e8));
        ok = (!(da > 0 || db > 0) || da == db)
            && m.i32(a + 0x6f4) == m.i32(b + 0x6f4)
            && texture3d_equal(m, m.u32(a + 0x6fc), m.u32(b + 0x6fc)) != 0
            && m.u16(a + 0x758) == m.u16(b + 0x758)
            && same_f32(m, a, b, 0x75c)
            && same_f32(m, a, b, 0x760)
            && [0xdb0, 0xdb4, 0xdb8, 0xdbc].iter().all(|&o| m.i32(a + o) == 0 && m.i32(b + o) == 0)
            && m.i32(a + 0xdc8) == m.i32(b + 0xdc8);
    }
    i32::from(ok)
}
