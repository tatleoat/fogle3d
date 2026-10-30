// SPDX-License-Identifier: MIT
//! Records of the image read back into the value form's structures (the crate's other
//! modules): a material, a 2D or a 3D texture, with their shader references and user data.
//! `hrc-imagecheck` holds what the image readers built against what the value reader read
//! from the same file with these; `si-scene` gives the store form the values of the records
//! the load's material conversion makes elements of (records no file was read into).

use super::material::mat;
use super::texture::{lclmap, t3d};
use crate::{Material, Texture2D, Texture3D, Udf, UserNode};
use si_mem::Mem;

// adapter: not in place. Everything here copies records out of the image into values.

/// Three floats at `a`.
pub fn vec3(m: &Mem, a: u32) -> [f32; 3] {
    [m.f32(a), m.f32(a + 4), m.f32(a + 8)]
}

/// The C string at `p`; empty for null.
pub fn string_at(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 {
        Vec::new()
    } else {
        m.cstr(p)
    }
}

/// A shader reference (`__SI_UDF`) at `p`; `None` for null.
pub fn udf_of(m: &Mem, p: u32) -> Option<Udf> {
    if p == 0 {
        return None;
    }
    let u = si_mem::Ptr32::<super::udf::Udf>::new(p);
    Some(Udf { name: string_at(m, m[u].name.addr()), active: m[u].active, internal: m[u].internal, order: m[u].order })
}

/// The list of user data that starts at `first`.
pub fn user_data_of(m: &Mem, first: u32) -> Vec<UserNode> {
    let mut out = Vec::new();
    let mut p = si_mem::Ptr32::<super::userdata::UserNode>::new(first);
    while !p.is_null() && out.len() < 100_000 {
        let size = m[p].size;
        let data = if m[p].data.is_null() { Vec::new() } else { m.bytes(m[p].data.addr(), size).to_vec() };
        out.push(UserNode { tag: string_at(m, m[p].tag.addr()), flag: m[p].flag, size: size as i32, data });
        p = m[p].next;
    }
    out
}

/// A 2D texture (`__SI_LclMap`) at `p`.
pub fn texture_of(m: &Mem, p: u32) -> Texture2D {
    let s = |off: u32| m.i16(p + off);
    let f = |off: u32| m.f32(p + off);
    Texture2D {
        name: string_at(m, m.u32(p + lclmap::PICTURE)),
        filename: string_at(m, m.u32(p + lclmap::NAME)),
        anim: s(lclmap::ANIM_TYPE),
        method: s(lclmap::METHOD),
        pixel_interp: s(lclmap::PIXEL_INTERP),
        crop_flag: s(lclmap::CROP_FLAG),
        crop: [s(0x30), s(0x32), s(0x34), s(0x36), s(0x38)],
        uscale: f(lclmap::SCALE_U),
        vscale: f(lclmap::SCALE_V),
        uoffset: f(lclmap::OFFSET_U),
        voffset: f(lclmap::OFFSET_V),
        rotation: vec3(m, p + lclmap::ROTATION),
        translation: vec3(m, p + lclmap::TRANSLATION),
        scaling: vec3(m, p + lclmap::SCALING),
        urepeat: s(lclmap::REPEAT_U),
        vrepeat: s(lclmap::REPEAT_V),
        swap_uv: s(lclmap::SWAP_UV),
        alt_u: s(lclmap::ALTERNATE_U),
        alt_v: s(lclmap::ALTERNATE_V),
        wrap_u: s(0x102),
        wrap_v: s(0x104),
        mask: s(lclmap::MASK),
        blend: f(lclmap::BLENDING),
        channel: s(lclmap::CHANNEL),
        ambient: f(lclmap::AMBIENT),
        diffuse: f(lclmap::DIFFUSE),
        specular: f(lclmap::SPECULAR),
        reflectivity: f(lclmap::REFLECTIVITY),
        transparency: f(lclmap::TRANSPARENCY),
        old_factor: f(lclmap::FACTOR_17),
        roughness: f(lclmap::ROUGHNESS),
        reflection_map: f(lclmap::REFLECTION_MAP),
        env_rotation: f(lclmap::ENV_ROTATION),
        black_white: s(lclmap::BLACK_WHITE),
        udf: udf_of(m, m.u32(p + lclmap::UDF)),
        displacement: s(lclmap::DISPLACEMENT),
        hw_flag: s(lclmap::HARDWARE_1),
        hw_tiling: s(lclmap::HARDWARE_2),
        x1cc: s(lclmap::TOKEN_235),
        x1ce: s(lclmap::TOKEN_236),
        user_data: user_data_of(m, m.u32(p + lclmap::USER_DATA)),
        x1d4: s(lclmap::TOKEN_237),
    }
}

/// A 3D texture (`__SI_Texture3D`) at `p`.
pub fn texture3d_of(m: &Mem, p: u32) -> Texture3D {
    let f = |off: u32| m.f32(p + off);
    let mut colours = [[0.0f32; 4]; 5];
    for (i, c) in colours.iter_mut().enumerate() {
        for (k, v) in c.iter_mut().enumerate() {
            *v = f(t3d::COLORS + 16 * i as u32 + 4 * k as u32);
        }
    }
    Texture3D {
        name: string_at(m, m.u32(p + t3d::NAME)),
        kind: m.i32(p + t3d::TYPE),
        spacing: f(t3d::SPACING),
        taper: f(t3d::TAPER),
        strength: f(t3d::STRENGTH),
        iterations: m.i32(p + t3d::ITERATIONS),
        power: f(t3d::POWER),
        mask: m.i16(p + t3d::MASK),
        blending: f(t3d::BLENDING),
        channel: m.i16(p + t3d::CHANNEL),
        colours,
        factors: vec3(m, p + t3d::FACTORS),
        scaling: vec3(m, p + t3d::SCALING),
        rotation: vec3(m, p + t3d::ROTATION),
        translation: vec3(m, p + t3d::TRANSLATION),
        ambient: f(t3d::AMBIENT),
        diffuse: f(t3d::DIFFUSE),
        specular: f(t3d::SPECULAR),
        reflection: f(t3d::REFLECTION),
        transparency: f(t3d::TRANSPARENCY),
        roughness: f(t3d::ROUGHNESS),
        udf: udf_of(m, m.u32(p + t3d::UDF)),
        displacement: m.i16(p + t3d::DISPLACEMENT),
        user_data: user_data_of(m, m.u32(p + t3d::USER_DATA)),
    }
}

/// A material (`__SI_Material`) at `p`, with its list of 2D textures and its 3D texture.
pub fn material_of(m: &Mem, p: u32) -> Material {
    let f = |off: u32| m.f32(p + off);
    let mut textures = Vec::new();
    let mut t = m.u32(p + mat::TEXTURES);
    while t != 0 && textures.len() < 100_000 {
        textures.push(texture_of(m, t));
        t = m.u32(t + lclmap::NEXT);
    }
    let t3 = m.u32(p + mat::TEXTURE3D);
    Material {
        name: m.cstr(p + mat::NAME),
        shading: m.i16(p + mat::SHADING),
        glow: m.i16(p + mat::GLOW_FLAG),
        glow_amount: f(mat::GLOW),
        ambient: vec3(m, p + mat::AMBIENT),
        diffuse: vec3(m, p + mat::DIFFUSE),
        specular: vec3(m, p + mat::SPECULAR),
        exponent: f(mat::SPECULAR_EXPONENT),
        reflectivity: f(mat::REFLECTIVITY),
        x6ac: f(mat::TOKEN_10),
        transparency: f(mat::TRANSPARENCY),
        x6bc: f(mat::TOKEN_8),
        refraction: f(mat::REFRACTION),
        textures,
        texture3d: if t3 == 0 { None } else { Some(texture3d_of(m, t3)) },
        blur: m.i16(p + mat::BLUR_FLAG),
        blur_width: f(mat::BLUR_WIDTH),
        blur_decay: f(mat::BLUR_DECAY),
        udf_material: udf_of(m, m.u32(p + mat::UDF_MATERIAL)),
        udf_volume: udf_of(m, m.u32(p + mat::UDF_VOLUME)),
        udf_shadow: udf_of(m, m.u32(p + mat::UDF_SHADOW)),
        udf_extra: udf_of(m, m.u32(p + mat::UDF_FOURTH)),
        user_data: user_data_of(m, m.u32(p + mat::USER_DATA)),
        xdc8: m.i16(p + mat::TOKEN_21C),
        xdca: m.i16(p + mat::TOKEN_21D),
    }
}
