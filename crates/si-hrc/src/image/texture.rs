// SPDX-License-Identifier: MIT
//! 2D and 3D texture records on the image: the routines of `t2d_fct.c` and `t3d_fct.c`
//! that the readers need to make one and to copy the reader's working copy into it.
//!
//! `si-types` has the two records (`LclMap`, 0x1d8 bytes; `Texture3D`, 0x158 bytes) with
//! most of their fields not placed, so the fields are addressed here by the named offsets
//! of [`lclmap`] and [`t3d`]; a pointer to a record is typed with `si-types`' record.
//! The names are this crate's, from what the file tokens mean (finding 0004).

use super::globals::{DEFAULT_MAP_METHOD, SI_IDENTITE, S_NO_NAME};
use super::outside::{Outside, Routine};
use super::udf::{udf_copy, udf_dispose};
use super::userdata::user_data_copy_from_setup;
use si_mem::{Mem, Ptr32};
pub use si_types::scene::{LclMap, Texture3D};

/// Offsets in a 2D texture (`__SI_LclMap`), as far as a file fills them.
pub mod lclmap {
    /// The picture's path: a string in a block of its own (file token 1; `NO_NAME` at first).
    pub const PICTURE: u32 = 0x14;
    /// The texture's own name: a string in a block of its own, or null (token 50).
    pub const NAME: u32 = 0x18;
    pub const ANIM_TYPE: u32 = 0x28;
    /// The mapping method (token 10); 7 and 8 are the reflection maps.
    pub const METHOD: u32 = 0x2a;
    pub const PIXEL_INTERP: u32 = 0x2c;
    pub const CROP_FLAG: u32 = 0x2e;
    /// Four shorts: the cropping bounds umin, vmin, umax, vmax.
    pub const CROP: u32 = 0x30;
    pub const SCALE_U: u32 = 0x58;
    pub const SCALE_V: u32 = 0x5c;
    pub const OFFSET_U: u32 = 0x68;
    pub const OFFSET_V: u32 = 0x6c;
    /// Vectors: rotation (token 32), translation (34), scaling (33).
    pub const ROTATION: u32 = 0x94;
    pub const TRANSLATION: u32 = 0xa0;
    pub const SCALING: u32 = 0xac;
    /// A matrix, the identity in a new texture.
    pub const MATRIX: u32 = 0xb8;
    pub const REPEAT_U: u32 = 0xf8;
    pub const REPEAT_V: u32 = 0xfa;
    pub const SWAP_UV: u32 = 0xfc;
    pub const ALTERNATE_U: u32 = 0xfe;
    pub const ALTERNATE_V: u32 = 0x100;
    pub const MASK: u32 = 0x10a;
    pub const BLENDING: u32 = 0x10c;
    pub const CHANNEL: u32 = 0x110;
    /// Flags `SI_updTxt2dEffects` derives: which factors are in use.
    pub const USES_AMBIENT: u32 = 0x112;
    pub const USES_DIFFUSE: u32 = 0x114;
    pub const USES_SPECULAR: u32 = 0x116;
    pub const USES_REFLECTIVITY: u32 = 0x118;
    pub const USES_TRANSPARENCY: u32 = 0x11a;
    pub const USES_ROUGHNESS: u32 = 0x120;
    pub const IS_REFLECTION_MAP: u32 = 0x124;
    /// Eight floats from here are copied as a block: the factors.
    pub const AMBIENT: u32 = 0x128;
    pub const DIFFUSE: u32 = 0x12c;
    pub const SPECULAR: u32 = 0x130;
    pub const REFLECTIVITY: u32 = 0x134;
    pub const TRANSPARENCY: u32 = 0x138;
    pub const FACTOR_17: u32 = 0x13c;
    pub const ROUGHNESS: u32 = 0x140;
    pub const REFLECTION_MAP: u32 = 0x144;
    pub const ENV_ROTATION: u32 = 0x168;
    /// The next 2D texture of the same owner.
    pub const NEXT: u32 = 0x16c;
    pub const BLACK_WHITE: u32 = 0x18e;
    /// The shader reference (`__SI_UDF`), or null.
    pub const UDF: u32 = 0x198;
    pub const DISPLACEMENT: u32 = 0x19c;
    pub const HARDWARE_1: u32 = 0x1a0;
    pub const HARDWARE_2: u32 = 0x1a2;
    pub const TOKEN_235: u32 = 0x1cc;
    pub const TOKEN_236: u32 = 0x1ce;
    /// The user data list (`__SI_UserNode`), or null.
    pub const USER_DATA: u32 = 0x1d0;
    pub const TOKEN_237: u32 = 0x1d4;
    pub const SIZE: u32 = 0x1d8;
}

/// Offsets in a 3D texture (`__SI_Texture3D`), as far as a file fills them.
pub mod t3d {
    /// The name: a string in a block of its own, or null.
    pub const NAME: u32 = 0x00;
    pub const TYPE: u32 = 0x0c;
    pub const SPACING: u32 = 0x10;
    pub const TAPER: u32 = 0x14;
    pub const STRENGTH: u32 = 0x18;
    pub const ITERATIONS: u32 = 0x1c;
    pub const POWER: u32 = 0x20;
    pub const MASK: u32 = 0x24;
    pub const BLENDING: u32 = 0x28;
    pub const CHANNEL: u32 = 0x2c;
    /// Five RGBA colours, 16 bytes apart.
    pub const COLORS: u32 = 0x30;
    /// The three factors between the colours.
    pub const FACTORS: u32 = 0x80;
    /// The turbulence object `SI_updTxt3dTurb` makes; null until then.
    pub const TURBULENCE: u32 = 0x8c;
    pub const SCALING: u32 = 0x90;
    pub const ROTATION: u32 = 0x9c;
    pub const TRANSLATION: u32 = 0xa8;
    pub const AMBIENT: u32 = 0x108;
    pub const DIFFUSE: u32 = 0x10c;
    pub const SPECULAR: u32 = 0x110;
    pub const REFLECTION: u32 = 0x114;
    pub const TRANSPARENCY: u32 = 0x118;
    pub const ROUGHNESS: u32 = 0x120;
    /// The shader reference (`__SI_UDF`), or null.
    pub const UDF: u32 = 0x14c;
    pub const DISPLACEMENT: u32 = 0x150;
    /// The user data list (`__SI_UserNode`), or null.
    pub const USER_DATA: u32 = 0x154;
    pub const SIZE: u32 = 0x158;
}

/// `SI_updTxt3dTurb (0x082b8d20)`, `t3d_fct.c`: rebuilds what a 3D texture derives from its
/// parameters: the turbulence object of its type with the colour ramp, the matrix of its
/// scaling, rotation and translation, the flags of the factors in use. It belongs to the
/// renderer's 3D texture code and is not on the image here.
pub const SI_UPD_TXT3D_TURB: Routine = Routine { name: "SI_updTxt3dTurb", addr: 0x082b8d20 };

const ONE: u32 = 0x3f80_0000;

/// [soft 0x08516224 SI_matrixCopy]: sixteen words from `src` to `dst`.
pub fn matrix_copy(m: &mut Mem, src: u32, dst: u32) {
    for k in 0..16 {
        let w = m.u32(src.wrapping_add(4 * k));
        m.set_u32(dst.wrapping_add(4 * k), w);
    }
}

/// [soft 0x082b2508 SI_textureInitialize]: the defaults of a 2D texture. The picture name
/// is a new copy of `NO_NAME`; the mapping method is the global default. The record's
/// first 16 bytes and some 60 others are not written.
pub fn texture_initialize(m: &mut Mem, t: Ptr32<LclMap>) {
    if t.is_null() {
        return;
    }
    let p = t.addr();
    let name = m.strdup(S_NO_NAME);
    m.set_u32(p + lclmap::PICTURE, name);
    let method = m[DEFAULT_MAP_METHOD];
    for off in [
        0x54, 0x50, 0x4c, 0x48, 0x3c, 0x38, 0x74, 0x70, 0x6c, 0x68, 0x64, 0x60, 0x10, 0x18, 0x20, 0x24, 0x88, 0x8c, 0x90, 0x94, 0x98, 0x9c, 0xa0,
        0xa4, 0xa8, 0x140, 0x13c, 0x138, 0x130, 0x164, 0x160, 0x15c, 0x158, 0x154, 0x150, 0x14c, 0x148, 0x168, 0x16c, 0x170, 0x190, 0x198, 0x1d0,
    ] {
        m.set_u32(p + off, 0);
    }
    for off in [0x5c, 0x58, 0x7c, 0x80, 0x84, 0xac, 0xb0, 0xb4, 0x10c, 0x144, 0x12c, 0x1b4, 0x1b0, 0x1c0, 0x1c4] {
        m.set_u32(p + off, ONE);
    }
    for (off, v) in [
        (0x1c, 0u16), (0x28, 0), (0x2a, method as u16), (0x2c, 1), (0x18e, 0), (0x2e, 0), (0x30, 0), (0x32, 0), (0x34, 0), (0x36, 0), (0x7a, 0),
        (0x78, 0), (0xf8, 1), (0xfa, 1), (0xfc, 0), (0xfe, 0), (0x100, 0), (0x102, 0), (0x104, 0), (0x106, 0), (0x108, 0), (0x10a, 3), (0x110, 2),
        (0x1cc, 0), (0x1ce, 0), (0x18c, 1), (0x194, 7), (0x19c, 0), (0x1a2, 0), (0x1a0, 0), (0x1c8, 1), (0x1d4, 0),
    ] {
        m.set_u16(p + off, v);
    }
    // the ten derived flags, 0x112 to 0x124
    for k in 0..10 {
        m.set_u16(p + 0x124 - 2 * k, 0);
    }
    matrix_copy(m, SI_IDENTITE.addr(), p + lclmap::MATRIX);
    m.set_f32(p + lclmap::AMBIENT, 0.75); // the float at 0x08655e80
    for off in [0x1a4, 0x1a8, 0x1ac] {
        m.set_f32(p + off, -1.0); // the float at 0x08655e84
    }
    m.set_u32(p + 0x1bc, 0xffff_ffff);
}

/// [soft 0x082b286c SI_textureAllocate]: a block of 0x1d8 bytes (`SI_malloc`: not cleared)
/// with the defaults.
pub fn texture_allocate(m: &mut Mem) -> Ptr32<LclMap> {
    let t: Ptr32<LclMap> = Ptr32::new(m.si_malloc(lclmap::SIZE));
    if !t.is_null() {
        texture_initialize(m, t);
    }
    t
}

/// [soft 0x082b289c SI_updTxt2dEffects]: the flags a 2D texture derives from its factors:
/// a factor is in use when it is above 0.0001 (ambient, diffuse, specular) or outside
/// −0.0001 … 0.0001 (reflectivity, transparency, roughness); the transparency flag is
/// also set by the black-and-white switch; the last flag marks the two reflection-map
/// methods. The constants are the doubles at 0x08655e88 and 0x08655e90.
pub fn upd_txt2d_effects(m: &mut Mem, t: Ptr32<LclMap>) {
    const EPS: f64 = 0.0001;
    let p = t.addr();
    let above = |m: &Mem, off: u32| (m.f32(p + off) as f64) > EPS;
    let outside_band = |m: &Mem, off: u32| {
        let v = m.f32(p + off) as f64;
        v < -EPS || v > EPS
    };
    let v = above(m, lclmap::AMBIENT);
    m.set_u16(p + lclmap::USES_AMBIENT, v as u16);
    let v = above(m, lclmap::DIFFUSE);
    m.set_u16(p + lclmap::USES_DIFFUSE, v as u16);
    let v = above(m, lclmap::SPECULAR);
    m.set_u16(p + lclmap::USES_SPECULAR, v as u16);
    let v = outside_band(m, lclmap::TRANSPARENCY) || m.i16(p + lclmap::BLACK_WHITE) != 0;
    m.set_u16(p + lclmap::USES_TRANSPARENCY, v as u16);
    let v = outside_band(m, lclmap::REFLECTIVITY);
    m.set_u16(p + lclmap::USES_REFLECTIVITY, v as u16);
    let v = outside_band(m, lclmap::ROUGHNESS);
    m.set_u16(p + lclmap::USES_ROUGHNESS, v as u16);
    let reflection_map = m.u16(p + lclmap::METHOD).wrapping_sub(7) < 2;
    m.set_u16(p + lclmap::IS_REFLECTION_MAP, reflection_map as u16);
}

/// [soft 0x082b29d4 textureTransfer]: copies what a 2D texture is from `src` to `dst`: the
/// two strings (duplicated; the destination's own freed first), the parameters, the
/// matrix, a copy of the shader reference (the destination's own disposed of first), and
/// a copy of the user data list appended. Then the derived flags are rebuilt.
pub fn texture_transfer(m: &mut Mem, o: &mut dyn Outside, src: Ptr32<LclMap>, dst: Ptr32<LclMap>) {
    let (s, d) = (src.addr(), dst.addr());
    for off in [lclmap::PICTURE, lclmap::NAME] {
        if m.u32(s + off) != 0 {
            if m.u32(d + off) != 0 {
                m.free(m.u32(d + off));
                m.set_u32(d + off, 0);
            }
            let copy = m.strdup(m.u32(s + off));
            m.set_u32(d + off, copy);
        }
    }
    for off in [0x28, 0x2a, 0x2c, 0x18e, 0x2e, 0x30, 0x32, 0x34, 0x36] {
        let v = m.u16(s + off);
        m.set_u16(d + off, v);
    }
    for off in [0x58, 0x5c, 0x68, 0x6c, 0x7c, 0x80, 0x84, 0x88, 0x8c, 0x90, 0xac, 0xb0, 0xb4, 0x94, 0x98, 0x9c, 0xa0, 0xa4, 0xa8] {
        let v = m.u32(s + off);
        m.set_u32(d + off, v);
    }
    matrix_copy(m, s + lclmap::MATRIX, d + lclmap::MATRIX);
    for off in [0xf8, 0xfa, 0xfc, 0xfe, 0x100, 0x102, 0x104, 0x10a] {
        let v = m.u16(s + off);
        m.set_u16(d + off, v);
    }
    let v = m.u32(s + lclmap::BLENDING);
    m.set_u32(d + lclmap::BLENDING, v);
    let v = m.u16(s + lclmap::CHANNEL);
    m.set_u16(d + lclmap::CHANNEL, v);
    for k in 0..8 {
        let v = m.u32(s + 0x128 + 4 * k);
        m.set_u32(d + 0x128 + 4 * k, v);
    }
    let v = m.u32(s + lclmap::ENV_ROTATION);
    m.set_u32(d + lclmap::ENV_ROTATION, v);
    m.set_u16(d + 0x194, 7);
    udf_dispose(m, o, Ptr32::new(m.u32(d + lclmap::UDF)));
    let copy = udf_copy(m, o, Ptr32::new(m.u32(s + lclmap::UDF)));
    m.set_u32(d + lclmap::UDF, copy.addr());
    let v = m.u16(s + lclmap::DISPLACEMENT);
    m.set_u16(d + lclmap::DISPLACEMENT, v);
    for k in 0..11 {
        let v = m.u32(s + 0x1a0 + 4 * k);
        m.set_u32(d + 0x1a0 + 4 * k, v);
    }
    upd_txt2d_effects(m, dst);
    let v = m.u16(s + lclmap::TOKEN_235);
    m.set_u16(d + lclmap::TOKEN_235, v);
    let v = m.u16(s + lclmap::TOKEN_236);
    m.set_u16(d + lclmap::TOKEN_236, v);
    if m.u32(s + lclmap::USER_DATA) != 0 {
        user_data_copy_from_setup(m, Ptr32::new(m.u32(s + lclmap::USER_DATA)), Ptr32::new(d + lclmap::USER_DATA));
    }
    let v = m.u16(s + lclmap::TOKEN_237);
    m.set_u16(d + lclmap::TOKEN_237, v);
}

/// [soft 0x082b2cf4 SI_textureCopy]: a new 2D texture that is a copy of `src`.
pub fn texture_copy(m: &mut Mem, o: &mut dyn Outside, src: Ptr32<LclMap>) -> Ptr32<LclMap> {
    let t = texture_allocate(m);
    if !t.is_null() {
        texture_transfer(m, o, src, t);
    }
    t
}

/// [soft 0x082b8ab0 SI_texture3DInitialize]: the defaults of a 3D texture: a ramp from
/// black over greys of 0.25, 0.5 and 0.75 (the floats at 0x08656310) to white with all five
/// alphas 0, factors 0.25, 0.5, 0.75, unit scaling, a zeroed matrix. The reflection factor
/// (+0x114) is among the bytes it does not write.
pub fn texture3d_initialize(m: &mut Mem, t: Ptr32<Texture3D>) {
    if t.is_null() {
        return;
    }
    let p = t.addr();
    let (q1, q2, q3) = (0.25f32.to_bits(), 0.5f32.to_bits(), 0.75f32.to_bits());
    for (off, v) in [
        (0x10, ONE), (0x14, 0), (0x18, ONE), (0x20, ONE), (0x28, ONE), (0x3c, 0), (0x38, 0), (0x34, 0), (0x30, 0), (0x48, q1), (0x44, q1), (0x40, q1),
        (0x58, q2), (0x54, q2), (0x50, q2), (0x68, q3), (0x64, q3), (0x60, q3), (0x7c, 0), (0x6c, 0), (0x00, 0), (0x04, 0), (0x0c, 0), (0x1c, 1),
        (0x78, ONE), (0x74, ONE), (0x70, ONE), (0x5c, 0), (0x4c, 0), (0x80, q1), (0x84, q2), (0x88, q3), (0x98, ONE), (0x94, ONE), (0x90, ONE),
        (0xa4, 0), (0xa0, 0), (0x9c, 0), (0xb0, 0), (0xac, 0), (0xa8, 0), (0x8c, 0), (0x108, q3), (0x120, 0), (0x11c, 0), (0x118, 0), (0x110, 0),
        (0x144, 0), (0x140, 0), (0x13c, 0), (0x138, 0), (0x134, 0), (0x130, 0), (0x12c, 0), (0x128, 0), (0x14c, 0), (0x154, 0), (0x124, ONE),
        (0x10c, ONE),
    ] {
        m.set_u32(p + off, v);
    }
    for (off, v) in [(0x08, 0u16), (0x24, 3), (0x2c, 1), (0x148, 1), (0x150, 0)] {
        m.set_u16(p + off, v);
    }
    // the matrix at 0xb4, and the ten shorts 0xf4 to 0x106
    for k in 0..16 {
        m.set_u32(p + 0xb4 + 4 * k, 0);
    }
    for k in 0..10 {
        m.set_u16(p + 0x106 - 2 * k, 0);
    }
}

/// [soft 0x082b8cf0 SI_texture3DAllocate]: a block of 0x158 bytes (`SI_malloc`) with the
/// defaults.
pub fn texture3d_allocate(m: &mut Mem) -> Ptr32<Texture3D> {
    let t: Ptr32<Texture3D> = Ptr32::new(m.si_malloc(t3d::SIZE));
    if !t.is_null() {
        texture3d_initialize(m, t);
    }
    t
}

/// [soft 0x082b90a8 texture3DTransfer]: copies what a 3D texture is from `src` to `dst`:
/// the name (duplicated; the destination's own is not freed), the parameters, a copy of
/// the shader reference, and a copy of the user data list. Then the derived state is
/// rebuilt by `SI_updTxt3dTurb (0x082b8d20)`.
pub fn texture3d_transfer(m: &mut Mem, o: &mut dyn Outside, src: Ptr32<Texture3D>, dst: Ptr32<Texture3D>) {
    let (s, d) = (src.addr(), dst.addr());
    if m.u32(s + t3d::NAME) != 0 {
        let copy = m.strdup(m.u32(s + t3d::NAME));
        m.set_u32(d + t3d::NAME, copy);
    }
    for off in [0x0c, 0x10, 0x14, 0x18, 0x1c, 0x20] {
        let v = m.u32(s + off);
        m.set_u32(d + off, v);
    }
    let v = m.u16(s + t3d::MASK);
    m.set_u16(d + t3d::MASK, v);
    let v = m.u32(s + t3d::BLENDING);
    m.set_u32(d + t3d::BLENDING, v);
    let v = m.u16(s + t3d::CHANNEL);
    m.set_u16(d + t3d::CHANNEL, v);
    // the ramp and its factors (0x30 to 0x88), then the three vectors (0x90 to 0xb0)
    for off in (0x30..=0x88).step_by(4).chain((0x90..=0xb0).step_by(4)) {
        let v = m.u32(s + off);
        m.set_u32(d + off, v);
    }
    for k in 0..8 {
        let v = m.u32(s + 0x108 + 4 * k);
        m.set_u32(d + 0x108 + 4 * k, v);
    }
    udf_dispose(m, o, Ptr32::new(m.u32(d + t3d::UDF)));
    let copy = udf_copy(m, o, Ptr32::new(m.u32(s + t3d::UDF)));
    m.set_u32(d + t3d::UDF, copy.addr());
    let v = m.u16(s + t3d::DISPLACEMENT);
    m.set_u16(d + t3d::DISPLACEMENT, v);
    o.call(m, &SI_UPD_TXT3D_TURB, &[d]);
    if m.u32(s + t3d::USER_DATA) != 0 {
        user_data_copy_from_setup(m, Ptr32::new(m.u32(s + t3d::USER_DATA)), Ptr32::new(d + t3d::USER_DATA));
    }
}

/// [soft 0x082b9298 SI_texture3DCopy]: a new 3D texture that is a copy of `src`.
pub fn texture3d_copy(m: &mut Mem, o: &mut dyn Outside, src: Ptr32<Texture3D>) -> Ptr32<Texture3D> {
    let t = texture3d_allocate(m);
    if !t.is_null() {
        texture3d_transfer(m, o, src, t);
    }
    t
}
