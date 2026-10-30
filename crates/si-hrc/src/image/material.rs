// SPDX-License-Identifier: MIT
//! Material records on the image: the two routines of `mat_fct.c` that make one.
//!
//! `si-types` has the record (`Material`, 0xdcc bytes) with most of its fields not placed
//! (and its first field wrongly as a pointer: the name is 30 bytes inline), so the fields
//! are addressed here by the named offsets of [`mat`]; a pointer to a material is typed
//! with `si-types`' record.

use super::globals::S_DEFAULT;
use si_mem::{Mem, Ptr32};
pub use si_types::scene::Material;

/// Offsets in a material (`__SI_Material`), as far as a file fills them.
pub mod mat {
    /// The name, inline: 30 bytes of room, copied into without a bound.
    pub const NAME: u32 = 0x00;
    /// The shading model (file tokens 2, 0x218 and 0x21b).
    pub const SHADING: u32 = 0x1e;
    pub const GLOW_FLAG: u32 = 0x28;
    pub const GLOW: u32 = 0x2c;
    /// Colours: three floats each, a fourth after them.
    pub const AMBIENT: u32 = 0x34;
    pub const DIFFUSE: u32 = 0x44;
    pub const SPECULAR: u32 = 0x54;
    pub const SPECULAR_EXPONENT: u32 = 0x64;
    pub const REFLECTIVITY: u32 = 0x6a8;
    pub const TOKEN_10: u32 = 0x6ac;
    pub const TRANSPARENCY: u32 = 0x6b8;
    pub const TOKEN_8: u32 = 0x6bc;
    pub const REFRACTION: u32 = 0x6dc;
    /// The next material of the scene's list.
    pub const NEXT: u32 = 0x6e4;
    /// The first 2D texture of the material's list (linked through a texture's +0x16c).
    pub const TEXTURES: u32 = 0x6ec;
    /// Set with [`TEXTURES`] to the first texture read.
    pub const TEXTURE_CURRENT: u32 = 0x6f0;
    /// The material's 3D texture, or null.
    pub const TEXTURE3D: u32 = 0x6fc;
    pub const BLUR_FLAG: u32 = 0x758;
    pub const BLUR_WIDTH: u32 = 0x75c;
    pub const BLUR_DECAY: u32 = 0x760;
    /// Shader references (`__SI_UDF`): material, volume, shadow (tokens 21 to 23), and a
    /// fourth (token 0xff1a).
    pub const UDF_MATERIAL: u32 = 0xdb0;
    pub const UDF_VOLUME: u32 = 0xdb4;
    pub const UDF_SHADOW: u32 = 0xdb8;
    pub const UDF_FOURTH: u32 = 0xdbc;
    /// The user data list (`__SI_UserNode`), or null.
    pub const USER_DATA: u32 = 0xdc4;
    /// Two shorts, 1 at first (tokens 0x21c and 0x21d).
    pub const TOKEN_21C: u32 = 0xdc8;
    pub const TOKEN_21D: u32 = 0xdca;
    pub const SIZE: u32 = 0xdcc;
}

const ONE: u32 = 0x3f80_0000;

/// [soft 0x082889b0 SI_materialInitialize]: the defaults of a material: the name
/// `default`, ambient 0.5, diffuse 0.7, specular 1, exponent 50, blur decay 3 (the floats
/// at 0x08654d30), shading model 4. Most of the record's 0xdcc bytes are not written.
pub fn material_initialize(m: &mut Mem, mat: Ptr32<Material>) {
    if mat.is_null() {
        return;
    }
    let p = mat.addr();
    // the eight bytes of "default" with its zero, as two words
    let (w0, w1) = (m.u32(S_DEFAULT), m.u32(S_DEFAULT + 4));
    m.set_u32(p, w0);
    m.set_u32(p + 4, w1);
    let (half, diffuse, exponent, decay) = (0.5f32.to_bits(), 0.7f32.to_bits(), 50.0f32.to_bits(), 3.0f32.to_bits());
    for (off, v) in [
        (0x24, ONE), (0x2c, 0), (0x30, 0), (0x3c, half), (0x38, half), (0x34, half), (0x4c, diffuse), (0x48, diffuse), (0x44, diffuse), (0x60, 0),
        (0x50, 0), (0x40, 0), (0x64, exponent), (0x6a8, 0), (0x6ac, 0), (0x6b8, 0), (0x6bc, 0), (0x6b0, 0), (0x6c0, 0), (0x5c, ONE), (0x58, ONE),
        (0x54, ONE), (0x6c8, half), (0x6d0, 0), (0x6cc, 0), (0x6d8, 0), (0x6d4, 0), (0x6dc, ONE), (0x740, 0), (0x744, 0), (0x748, ONE), (0x6e0, 0),
        (0x6e4, 0), (0x6ec, 0), (0x6f0, 0), (0x6f4, 0), (0x6f8, 0), (0x6fc, 0), (0x74c, 0), (0x75c, ONE), (0x760, decay), (0xdc0, ONE), (0x750, 0),
        (0xda8, 0xffff_ffff), (0xdb0, 0), (0xdb4, 0), (0xdb8, 0), (0xdbc, 0), (0xdc4, 0),
    ] {
        m.set_u32(p + off, v);
    }
    // the working block of 16 floats, 0x700 to 0x73c
    for k in 0..16 {
        m.set_u32(p + 0x700 + 4 * k, 0);
    }
    for (off, v) in [
        (0x1e, 4u16), (0x20, 2), (0x28, 0), (0x6b4, 0), (0x6c4, 0), (0x6e8, 0), (0x754, 0), (0x756, 0), (0x758, 0), (0xda4, 1), (0xdac, 1), (0xdc8, 1),
        (0xdca, 1),
    ] {
        m.set_u16(p + off, v);
    }
}

/// [soft 0x08288c74 SI_materialAllocate]: a block of 0xdcc bytes (`SI_malloc`: not
/// cleared) with the defaults.
pub fn material_allocate(m: &mut Mem) -> Ptr32<Material> {
    let mat: Ptr32<Material> = Ptr32::new(m.si_malloc(mat::SIZE));
    if !mat.is_null() {
        material_initialize(m, mat);
    }
    mat
}
