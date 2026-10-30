// SPDX-License-Identifier: MIT
//! The writers of `b_matio.c` on the image: shader references, 2D and 3D textures,
//! materials, and the three files that hold one material or texture (finding 0004,
//! "Material", "Texture", "3D texture", "Shader reference").
//!
//! Every writer is a fixed sequence of fields in the order of the prototype list
//! `SI_prototypeBinaryWrite` writes for its kind, with the version tests of that list
//! repeated inside (see [`super::wio`]). A field is written straight from the record with a
//! swapping write (the record is swapped and restored around the `fwrite`); a field the
//! writer changes on the way (a texture's mask, a material's shading model) or derives (a
//! count, a length) goes through a local on the routine's stack.

use super::globals::{SI_PIPE, SI_PIPED_FILES, SI_SAVE_ELEM_ERROR_CODE, S_MTRL, S_TX3D, S_TXMP, S_WB_MTR, S_WB_T3D, S_WB_TXT};
use super::material::mat;
use super::modelwrite::{user_data_size, user_data_write};
use super::outside::Outside;
use super::proto::prototype_binary_write;
use super::texture::{lclmap, t3d};
use super::wio::{byteswap_fwrite, dbfile_write_bin_header, out_above_or_nan, out_below, rgb_fwrite, rgba_fwrite, string_binary_write, vector_fwrite};
use si_mem::Mem;

/// [soft 0x083e9068 si_udfWrite]: a shader reference: its name (a null string, that is a
/// lone zero byte, for a missing reference, a missing name or an empty one), then active
/// (+0xc) and internal (+0x10), each a short widened to an int, and order (+0x14), each
/// through a local int; 0 for all three when there is no reference. Answers 1.
pub fn udf_write(m: &mut Mem, o: &mut dyn Outside, udf: u32, file: u32) -> i32 {
    const FRAME: u32 = 4;
    let local = m.local(FRAME);
    let mut name = 0;
    if udf != 0 {
        let p = m.u32(udf);
        if p != 0 && m.u8(p) != 0 {
            name = p;
        }
    }
    string_binary_write(m, o, file, name);
    let fields: [Option<u32>; 3] = if udf == 0 {
        [None, None, None]
    } else {
        [Some(m.i16(udf + 0xc) as i32 as u32), Some(m.i16(udf + 0x10) as i32 as u32), Some(m.u32(udf + 0x14))]
    };
    for v in fields {
        m.set_u32(local, v.unwrap_or(0));
        byteswap_fwrite(m, o, local, 4, 1, file);
    }
    m.release(FRAME);
    1
}

/// A user-data token's body: the length (4 when there is no list; otherwise what
/// `si_userDataSize` answers) and the count through two locals, then the blocks.
fn user_data_token(m: &mut Mem, o: &mut dyn Outside, file: u32, list: u32, length_at: u32, count_at: u32) {
    m.set_u32(length_at, 4);
    m.set_u32(count_at, 0);
    if list != 0 {
        let size = user_data_size(m, list, 0, count_at);
        m.set_u32(length_at, size);
    }
    byteswap_fwrite(m, o, length_at, 4, 1, file);
    byteswap_fwrite(m, o, count_at, 4, 1, file);
    if list != 0 {
        user_data_write(m, o, file, list, 0);
    }
}

/// [soft 0x083e73f8 si_textureBinaryWrite]: a 2D texture, the fields of its list in order:
/// the picture (+0x14), the animation type, the repetitions, scales and offsets, the pixel
/// interpolation and the mapping method, the six factors, the environment rotation, the
/// blending, the cropping flag, the four cropping pairs (two shorts each, from +0x30,
/// +0x34, +0x32, +0x36: finding 0004), the swap and alternation flags, the reflectivity,
/// the mask (4 written as 3) and the channel; above 2.62 the rotation, scaling and
/// translation and the black-and-white flag; above 2.65 the shader, the displacement, the
/// texture's name (+0x18), the two hardware shorts, the three shorts 0x235 to 0x237, the
/// user data and the mask again, unchanged. Answers 1.
pub fn texture_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, x: u32) -> i32 {
    // the frame: the short local, the user data's length and count
    const FRAME: u32 = 12;
    let frame = m.local(FRAME);
    let (short_at, length_at, count_at) = (frame, frame + 4, frame + 8);
    let via_short = |m: &mut Mem, o: &mut dyn Outside, v: i16| {
        m.set_i16(short_at, v);
        byteswap_fwrite(m, o, short_at, 2, 1, file);
    };
    let picture = m.u32(x + lclmap::PICTURE);
    string_binary_write(m, o, file, picture);
    let v = m.i16(x + lclmap::ANIM_TYPE);
    via_short(m, o, v);
    byteswap_fwrite(m, o, x + lclmap::REPEAT_U, 2, 1, file);
    byteswap_fwrite(m, o, x + lclmap::REPEAT_V, 2, 1, file);
    for off in [lclmap::SCALE_U, lclmap::SCALE_V, lclmap::OFFSET_U, lclmap::OFFSET_V] {
        byteswap_fwrite(m, o, x + off, 4, 1, file);
    }
    let v = m.i16(x + lclmap::PIXEL_INTERP);
    via_short(m, o, v);
    let v = m.i16(x + lclmap::METHOD);
    via_short(m, o, v);
    for off in [lclmap::AMBIENT, lclmap::DIFFUSE, lclmap::SPECULAR, lclmap::ROUGHNESS, lclmap::TRANSPARENCY, lclmap::REFLECTION_MAP, lclmap::ENV_ROTATION, lclmap::BLENDING] {
        byteswap_fwrite(m, o, x + off, 4, 1, file);
    }
    let v = m.i16(x + lclmap::CROP_FLAG);
    via_short(m, o, v);
    for off in [0, 4, 2, 6] {
        byteswap_fwrite(m, o, x + lclmap::CROP + off, 2, 2, file);
    }
    for off in [lclmap::SWAP_UV, lclmap::ALTERNATE_U, lclmap::ALTERNATE_V] {
        let v = m.i16(x + off);
        via_short(m, o, v);
    }
    byteswap_fwrite(m, o, x + lclmap::REFLECTIVITY, 4, 1, file);
    let mut mask = m.i16(x + lclmap::MASK);
    if mask == 4 {
        mask = 3;
    }
    via_short(m, o, mask);
    let v = m.i16(x + lclmap::CHANNEL);
    via_short(m, o, v);
    if out_above_or_nan(m, 2.62) {
        vector_fwrite(m, o, x + lclmap::ROTATION, 0xc, 1, file);
        vector_fwrite(m, o, x + lclmap::SCALING, 0xc, 1, file);
        vector_fwrite(m, o, x + lclmap::TRANSLATION, 0xc, 1, file);
        byteswap_fwrite(m, o, x + lclmap::BLACK_WHITE, 2, 1, file);
        if out_above_or_nan(m, 2.65) {
            let udf = m.u32(x + lclmap::UDF);
            udf_write(m, o, udf, file);
            byteswap_fwrite(m, o, x + lclmap::DISPLACEMENT, 2, 1, file);
            let name = m.u32(x + lclmap::NAME);
            string_binary_write(m, o, file, name);
            for off in [lclmap::HARDWARE_1, lclmap::HARDWARE_2, lclmap::TOKEN_235, lclmap::TOKEN_236, lclmap::TOKEN_237] {
                byteswap_fwrite(m, o, x + off, 2, 1, file);
            }
            let list = m.u32(x + lclmap::USER_DATA);
            user_data_token(m, o, file, list, length_at, count_at);
            byteswap_fwrite(m, o, x + lclmap::MASK, 2, 1, file);
        }
    }
    m.release(FRAME);
    1
}

/// [soft 0x083e7cf4 si_texture3DBinaryWrite]: a 3D texture: the name, the type and the
/// iterations (through the short local), strength and spacing, the five colours, the
/// three factors, scaling, rotation and translation, the ambient, diffuse, specular,
/// transparency, roughness and reflection factors, power, taper, blending, mask and
/// channel (through the short local); above 2.65 the shader, the displacement and the
/// user data. Answers 1.
pub fn texture3d_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, x: u32) -> i32 {
    const FRAME: u32 = 12;
    let frame = m.local(FRAME);
    let (short_at, length_at, count_at) = (frame, frame + 4, frame + 8);
    let via_short = |m: &mut Mem, o: &mut dyn Outside, v: u16| {
        m.set_u16(short_at, v);
        byteswap_fwrite(m, o, short_at, 2, 1, file);
    };
    let name = m.u32(x + t3d::NAME);
    string_binary_write(m, o, file, name);
    let v = m.u16(x + t3d::TYPE);
    via_short(m, o, v);
    let v = m.u16(x + t3d::ITERATIONS);
    via_short(m, o, v);
    byteswap_fwrite(m, o, x + t3d::STRENGTH, 4, 1, file);
    byteswap_fwrite(m, o, x + t3d::SPACING, 4, 1, file);
    for k in 0..5 {
        rgba_fwrite(m, o, x + t3d::COLORS + 0x10 * k, 0x10, 1, file);
    }
    for k in 0..3 {
        byteswap_fwrite(m, o, x + t3d::FACTORS + 4 * k, 4, 1, file);
    }
    vector_fwrite(m, o, x + t3d::SCALING, 0xc, 1, file);
    vector_fwrite(m, o, x + t3d::ROTATION, 0xc, 1, file);
    vector_fwrite(m, o, x + t3d::TRANSLATION, 0xc, 1, file);
    for off in [t3d::AMBIENT, t3d::DIFFUSE, t3d::SPECULAR, t3d::ROUGHNESS, t3d::TRANSPARENCY, t3d::REFLECTION, t3d::POWER, t3d::TAPER, t3d::BLENDING] {
        byteswap_fwrite(m, o, x + off, 4, 1, file);
    }
    let v = m.u16(x + t3d::MASK);
    via_short(m, o, v);
    let v = m.u16(x + t3d::CHANNEL);
    via_short(m, o, v);
    if out_above_or_nan(m, 2.65) {
        let udf = m.u32(x + t3d::UDF);
        udf_write(m, o, udf, file);
        byteswap_fwrite(m, o, x + t3d::DISPLACEMENT, 2, 1, file);
        let list = m.u32(x + t3d::USER_DATA);
        user_data_token(m, o, file, list, length_at, count_at);
    }
    m.release(FRAME);
    1
}

/// [soft 0x083e8604 si_materialBinaryWrite]: a material: its name (the string inline at
/// +0), the shading model (7 written as 1, through a local), glow flag and amount, the
/// three colours, specular exponent, transparency, reflectivity, refractive index; with
/// `with_textures` the count of the 2D texture list and each texture after a zero
/// "external" short, then 1 and the 3D texture (after the same short) or 0; without it
/// two zero counts; the static blur flag, width and decay. Above 2.65 the material,
/// volume and shadow shaders, the shading model again (7 unless both shorts at +0xdc8
/// are 1: the int there is 0x10001), the user data; from 3.8109 the fourth shader with its
/// length (13 plus its name's length; 13 for a missing reference or name), the shading
/// model a third time and the two shorts. Answers 1.
pub fn material_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, p: u32, with_textures: i16) -> i32 {
    // the frame: the zero "external" short, the count, the shading model, the user data's
    // length and count, the fourth shader's length
    const FRAME: u32 = 0x18;
    let frame = m.local(FRAME);
    let (external_at, count_at, model_at, length_at, ud_count_at, udf_len_at) = (frame, frame + 2, frame + 4, frame + 8, frame + 12, frame + 16);
    m.set_i16(external_at, 0);
    string_binary_write(m, o, file, p + mat::NAME);
    let mut model = m.i16(p + mat::SHADING);
    if model == 7 {
        model = 1;
    }
    m.set_i16(model_at, model);
    byteswap_fwrite(m, o, model_at, 2, 1, file);
    byteswap_fwrite(m, o, p + mat::GLOW_FLAG, 2, 1, file);
    byteswap_fwrite(m, o, p + mat::GLOW, 4, 1, file);
    rgb_fwrite(m, o, p + mat::AMBIENT, 0xc, 1, file);
    rgb_fwrite(m, o, p + mat::DIFFUSE, 0xc, 1, file);
    rgb_fwrite(m, o, p + mat::SPECULAR, 0xc, 1, file);
    for off in [mat::SPECULAR_EXPONENT, mat::TRANSPARENCY, mat::REFLECTIVITY, mat::REFRACTION] {
        byteswap_fwrite(m, o, p + off, 4, 1, file);
    }
    if with_textures == 0 {
        m.set_i16(count_at, 0);
        byteswap_fwrite(m, o, count_at, 2, 1, file);
        byteswap_fwrite(m, o, count_at, 2, 1, file);
    } else {
        let mut n: i16 = 0;
        let mut x = m.u32(p + mat::TEXTURES);
        while x != 0 {
            x = m.u32(x + lclmap::NEXT);
            n = n.wrapping_add(1);
        }
        m.set_i16(count_at, n);
        byteswap_fwrite(m, o, count_at, 2, 1, file);
        let mut x = m.u32(p + mat::TEXTURES);
        while x != 0 {
            byteswap_fwrite(m, o, external_at, 2, 1, file);
            texture_binary_write(m, o, file, x);
            x = m.u32(x + lclmap::NEXT);
        }
        m.set_i16(count_at, 0);
        let x3 = m.u32(p + mat::TEXTURE3D);
        if x3 == 0 {
            byteswap_fwrite(m, o, count_at, 2, 1, file);
        } else {
            m.set_i16(count_at, 1);
            byteswap_fwrite(m, o, count_at, 2, 1, file);
            byteswap_fwrite(m, o, external_at, 2, 1, file);
            texture3d_binary_write(m, o, file, x3);
        }
    }
    byteswap_fwrite(m, o, p + mat::BLUR_FLAG, 2, 1, file);
    byteswap_fwrite(m, o, p + mat::BLUR_WIDTH, 4, 1, file);
    byteswap_fwrite(m, o, p + mat::BLUR_DECAY, 4, 1, file);
    if out_above_or_nan(m, 2.65) {
        for off in [mat::UDF_MATERIAL, mat::UDF_VOLUME, mat::UDF_SHADOW] {
            let udf = m.u32(p + off);
            udf_write(m, o, udf, file);
        }
        let mut model = m.i16(p + mat::SHADING);
        if m.u32(p + mat::TOKEN_21C) != 0x0001_0001 {
            model = 7;
        }
        m.set_i16(model_at, model);
        byteswap_fwrite(m, o, model_at, 2, 1, file);
        let list = m.u32(p + mat::USER_DATA);
        user_data_token(m, o, file, list, length_at, ud_count_at);
        if !out_below(m, 3.8109) {
            let udf = m.u32(p + mat::UDF_FOURTH);
            let mut length = 0xdu32;
            if udf != 0 {
                let name = m.u32(udf);
                if name != 0 {
                    length = (m.cstr(name).len() as u32).wrapping_add(0xd);
                }
            }
            m.set_u32(udf_len_at, length);
            byteswap_fwrite(m, o, udf_len_at, 4, 1, file);
            udf_write(m, o, udf, file);
            byteswap_fwrite(m, o, p + mat::SHADING, 2, 1, file);
            byteswap_fwrite(m, o, p + mat::TOKEN_21C, 2, 1, file);
            byteswap_fwrite(m, o, p + mat::TOKEN_21D, 2, 1, file);
        }
    }
    m.release(FRAME);
    1
}

/// What the three file writers share: `name` with the suffix appended is opened for
/// writing (unless files go to a pipe: then the pipe's stream is written), the header
/// with `magic` and `version`, the prototype table, the tag and the record; then, unless
/// piped, `fflush`, `ferror` and `fclose`. Answers 1, or 0 when the file could not be
/// opened (`SI_saveElemErrorCode` 1) or a write failed (code 2).
///
/// The path is printed into 256 bytes of stack (`sprintf`); a longer one runs over the
/// original's frame. The port records a fault for it and answers 0.
#[allow(clippy::too_many_arguments)]
fn write_binary_file(m: &mut Mem, o: &mut dyn Outside, name: u32, suffix: &[u8], mode_at: u32, version: f32, magic: u32, tag_at: u32, write: &mut dyn FnMut(&mut Mem, &mut dyn Outside, u32)) -> i32 {
    const FRAME: u32 = 0x104;
    let path = m.local(FRAME);
    let mut text = m.cstr(name);
    text.extend_from_slice(suffix);
    if text.len() >= 0x100 {
        m.record_fault(path + 0x100, text.len() as u32 - 0xff, si_mem::Access::Write);
        m.release(FRAME);
        return 0;
    }
    m.put_cstr(path, &text);
    let mut file = m[SI_PIPE];
    if m[SI_PIPED_FILES] == 0 {
        file = o.fopen(m, path, mode_at);
    }
    let result;
    if file == 0 {
        m[SI_SAVE_ELEM_ERROR_CODE] = 1;
        result = 0;
    } else {
        dbfile_write_bin_header(m, o, file, version, magic);
        prototype_binary_write(m, o, file);
        o.fwrite(m, tag_at, 4, 1, file);
        write(m, o, file);
        let mut ok = 1;
        if m[SI_PIPED_FILES] == 0 {
            o.fflush(m, file);
            ok = (o.ferror(m, file) == 0) as i32;
            if ok == 0 {
                m[SI_SAVE_ELEM_ERROR_CODE] = 2;
            }
            if m[SI_PIPED_FILES] == 0 {
                o.fclose(m, file);
            }
        }
        result = ok;
    }
    m.release(FRAME);
    result
}

/// [soft 0x083e8a04 SI_materialWriteBinaryFile]: the material file `name` + `.mtr`: magic
/// 0x5380f640, `version`, the table, `MTRL`, the material without its textures.
pub fn material_write_binary_file(m: &mut Mem, o: &mut dyn Outside, name: u32, version: f32, material: u32) -> i32 {
    write_binary_file(m, o, name, b".mtr", S_WB_MTR, version, 0x5380_f640, S_MTRL, &mut |m, o, f| {
        material_binary_write(m, o, f, material, 0);
    })
}

/// [soft 0x083e8c28 SI_textureWriteBinaryFile]: the 2D texture file `name` + `.txt`:
/// magic 0x5380f643, `version`, the table, `TXMP`, the texture.
pub fn texture_write_binary_file(m: &mut Mem, o: &mut dyn Outside, name: u32, version: f32, texture: u32) -> i32 {
    write_binary_file(m, o, name, b".txt", S_WB_TXT, version, 0x5380_f643, S_TXMP, &mut |m, o, f| {
        texture_binary_write(m, o, f, texture);
    })
}

/// [soft 0x083e8e48 SI_texture3DWriteBinaryFile]: the 3D texture file `name` + `.t3D`:
/// magic 0x5380f644, `version`, the table, `TX3D`, the texture.
pub fn texture3d_write_binary_file(m: &mut Mem, o: &mut dyn Outside, name: u32, version: f32, texture: u32) -> i32 {
    write_binary_file(m, o, name, b".t3D", S_WB_T3D, version, 0x5380_f644, S_TX3D, &mut |m, o, f| {
        texture3d_binary_write(m, o, f, texture);
    })
}
