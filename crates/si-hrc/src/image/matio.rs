// SPDX-License-Identifier: MIT
//! The readers of `b_matio.c` on the image: shader references, 2D and 3D textures,
//! materials, and the three routines that read one of them from a file of its own.
//!
//! A record is the data of its prototype's tokens in the list's order (finding 0004). A
//! reader walks the list of its prototype in the table; without a list it takes each token
//! from the file. The 2D and 3D texture readers fill a working copy on their stack and
//! hand it to the copy routine, so what a file does not store keeps the working copy's
//! defaults, **and what the defaults do not cover keeps whatever the stack held** (a 2D
//! texture's reflectivity factor at +0x134, a 3D texture's reflection factor at +0x114).

use super::globals::{FILE_VERSION_MATIO, SI_GRAF_LISTS, SI_PIPE, SI_PIPED_FILES, SI_PROC_DATA, SI_VERSION_INPUT};
use super::io::{byteswap_fread, file_skip_field, float_fread, is_old_udf_version, read_bin_header, rgb_fread, rgba_fread, string_binary_read, vector_fread};
use super::material::{mat, material_allocate, Material};
use super::proto::{prototype_allocate, prototype_binary_read, prototype_dispose, slot, Prototypes};
use super::texture::{lclmap, t3d, texture3d_copy, texture3d_initialize, texture_copy, texture_initialize, LclMap, Texture3D};
use super::udf::{udf_allocate, udf_dispose, Udf};
use super::userdata::user_data_read;
use super::Outside;
use si_mem::{Mem, Ptr32};
use si_x87::x87::F80;

/// The token that follows in a record: the next one of the prototype's list if the table
/// has one for the slot at `list_at` (a pointer into the table), otherwise a short from
/// the file. `index` counts through the list.
///
/// Without a list, at the end of the file, the read fails and the token keeps its value.
/// The original then goes round again with the same token, for ever: every further read
/// fails too. The port cannot do that. It records a fault at the stream's handle and
/// answers a token of 0, which ends the record.
pub(crate) fn next_token(m: &mut Mem, o: &mut dyn Outside, file: u32, list_at: u32, token_at: u32, index: &mut i16) {
    let list = m.u32(list_at);
    if list == 0 {
        if byteswap_fread(m, o, token_at, 2, 1, file) == 0 && m.i16(token_at) != 0 {
            m.record_fault(file, 2, si_mem::Access::Read);
            m.set_i16(token_at, 0);
        }
    } else {
        let t = m.i16(list.wrapping_add((*index as i32 * 2) as u32));
        m.set_i16(token_at, t);
        *index = index.wrapping_add(1);
    }
}

/// [soft 0x083e9374 readOldUDF]: the older form of a shader reference, read and thrown
/// away: tokens 1, 2 and 3 a string each, 4 a short, 5 an int. Without a list the tokens
/// are 1 to 5 in order. A token 6 ends the record like a 0, also when it comes from a list.
pub fn read_old_udf(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) {
    let scratch = m.local(8);
    let list_at = protos.addr().wrapping_add(4 * slot::UDF as u32);
    let mut index: i32 = 0;
    let mut token: i16;
    if !protos.is_null() && m.u32(list_at) != 0 {
        token = m.i16(m.u32(list_at));
        index = 1;
    } else {
        token = 1;
    }
    while token != 0 {
        match token {
            1 | 2 | 3 => {
                let s = string_binary_read(m, o, file);
                if s != 0 {
                    m.free(s);
                }
            }
            4 => {
                byteswap_fread(m, o, scratch + 4, 2, 1, file);
            }
            5 => {
                byteswap_fread(m, o, scratch, 4, 1, file);
            }
            _ => {
                file_skip_field(m, o, file, token);
            }
        }
        // (with a null table the original reads its slot through the null pointer here)
        let list = m.u32(list_at);
        if list == 0 {
            token = token.wrapping_add(1);
        } else {
            token = m.i16(list.wrapping_add((index * 2) as u32));
            index += 1;
        }
        if token == 6 {
            token = 0;
        }
    }
    m.release(8);
}

/// [soft 0x083e9154 si_udfRead]: a shader reference: token 1 the name, 2, 3 and 4 an int
/// each (active and internal are kept as shorts, order as it is). A token 5 ends the
/// record at once. Without a list the tokens are 1 to 5 in order.
///
/// An empty name disposes of the reference at once (the remaining tokens are still read
/// and dropped); the name `NULL` disposes of it at the end, unless a token 5 ended the
/// record. In a file whose version stores the old form (`SI_versionInput`) the record is
/// read by [`read_old_udf`] and the answer is null.
pub fn udf_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) -> Ptr32<Udf> {
    if is_old_udf_version(m[SI_VERSION_INPUT]) {
        read_old_udf(m, o, file, protos);
        return Ptr32::NULL;
    }
    let scratch = m.local(4);
    let list_at = protos.addr().wrapping_add(4 * slot::UDF as u32);
    let mut udf = udf_allocate(m, o);
    let mut index: i16 = 0;
    let mut token: i16;
    if protos.is_null() || m.u32(list_at) == 0 {
        token = 1;
    } else {
        token = m.i16(m.u32(list_at));
        index = 1;
    }
    let mut ended_by_5 = false;
    while token != 0 {
        match token {
            1 => {
                let s = string_binary_read(m, o, file);
                if udf.is_null() || s == 0 || m.strlen(s) == 0 {
                    udf_dispose(m, o, udf);
                    udf = Ptr32::NULL;
                } else {
                    let name = m.strdup(s);
                    m[udf].name = Ptr32::new(name);
                }
                if s != 0 {
                    m.free(s);
                }
            }
            2 => {
                byteswap_fread(m, o, scratch, 4, 1, file);
                if !udf.is_null() {
                    m[udf].active = m.u32(scratch) as i16;
                }
            }
            3 => {
                byteswap_fread(m, o, scratch, 4, 1, file);
                if !udf.is_null() {
                    m[udf].internal = m.u32(scratch) as i16;
                }
            }
            4 => {
                byteswap_fread(m, o, scratch, 4, 1, file);
                if !udf.is_null() {
                    m[udf].order = m.i32(scratch);
                }
            }
            5 => {
                ended_by_5 = true;
                break;
            }
            _ => {
                file_skip_field(m, o, file, token);
            }
        }
        // (with a null table the original reads its slot through the null pointer here)
        let list = m.u32(list_at);
        if list == 0 {
            token = token.wrapping_add(1);
            if token == 6 {
                token = 0;
            }
        } else {
            token = m.i16(list.wrapping_add((index as i32 * 2) as u32));
            index = index.wrapping_add(1);
        }
    }
    if !ended_by_5 && !udf.is_null() && !m[udf].name.is_null() && m.cstr(m[udf].name.addr()) == b"NULL" {
        udf_dispose(m, o, udf);
        udf = Ptr32::NULL;
    }
    m.release(4);
    udf
}

/// [soft 0x083e6b90 si_textureBinaryRead]: a 2D texture. The tokens are read into a
/// working copy on the stack (defaults from `SI_textureInitialize`); the answer is a new
/// texture copied from it (`SI_textureCopy`), after which the working copy's two strings
/// and its shader reference are freed. Its user data list is not: those blocks stay
/// allocated beside the copies the new texture got.
///
/// A short that is read through the scratch word (tokens 2, 9, 10, 19, 21, 26, 27, 28, 30
/// and 31) is stored whether or not the read succeeded: at the end of the file the field
/// gets what the scratch word held. Tokens 22 to 25 read two shorts each into the four
/// cropping bounds, overlapping (finding 0004). In a texture whose method is 7 or 8 the
/// blending and the six factors end up 0.
pub fn texture_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) -> Ptr32<LclMap> {
    // the frame: a 4-byte scratch, the scratch short with the token behind it, the working copy
    const FRAME: u32 = 8 + lclmap::SIZE;
    let frame = m.local(FRAME);
    let (scratch4, scratch, token_at, w) = (frame, frame + 4, frame + 6, frame + 8);
    texture_initialize(m, Ptr32::new(w));
    let list_at = protos.addr().wrapping_add(4 * slot::TEXTURE as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        let token = m.i16(token_at);
        // a short read into the scratch word and stored from it
        let via_scratch = |m: &mut Mem, o: &mut dyn Outside, off: u32| {
            byteswap_fread(m, o, scratch, 2, 1, file);
            let v = m.u16(scratch);
            m.set_u16(w + off, v);
        };
        match token {
            1 | 0x32 => {
                let off = if token == 1 { lclmap::PICTURE } else { lclmap::NAME };
                if m.u32(w + off) != 0 {
                    m.free(m.u32(w + off));
                    m.set_u32(w + off, 0);
                }
                let s = string_binary_read(m, o, file);
                m.set_u32(w + off, s);
            }
            2 => via_scratch(m, o, lclmap::ANIM_TYPE),
            3 => {
                byteswap_fread(m, o, w + lclmap::REPEAT_U, 2, 1, file);
            }
            4 => {
                byteswap_fread(m, o, w + lclmap::REPEAT_V, 2, 1, file);
            }
            5 | 6 | 7 | 8 | 0xb | 0xc | 0xd | 0xe | 0xf | 0x10 | 0x11 | 0x12 => {
                let off = match token {
                    5 => lclmap::SCALE_U,
                    6 => lclmap::SCALE_V,
                    7 => lclmap::OFFSET_U,
                    8 => lclmap::OFFSET_V,
                    0xb => lclmap::AMBIENT,
                    0xc => lclmap::DIFFUSE,
                    0xd => lclmap::SPECULAR,
                    0xe => lclmap::ROUGHNESS,
                    0xf => lclmap::TRANSPARENCY,
                    0x10 => lclmap::REFLECTION_MAP,
                    0x11 => lclmap::FACTOR_17,
                    _ => lclmap::ENV_ROTATION,
                };
                float_fread(m, o, w + off, 4, 1, file);
            }
            9 => via_scratch(m, o, lclmap::PIXEL_INTERP),
            10 => via_scratch(m, o, lclmap::METHOD),
            0x13 => {
                m.set_f32(w + lclmap::BLENDING, 1.0);
                via_scratch(m, o, lclmap::MASK);
            }
            0x14 => {
                byteswap_fread(m, o, w + lclmap::BLENDING, 4, 1, file);
            }
            0x15 => via_scratch(m, o, lclmap::CROP_FLAG),
            // two shorts each, into four consecutive ones
            0x16 => {
                byteswap_fread(m, o, w + lclmap::CROP, 2, 2, file);
            }
            0x17 => {
                byteswap_fread(m, o, w + lclmap::CROP + 4, 2, 2, file);
            }
            0x18 => {
                byteswap_fread(m, o, w + lclmap::CROP + 2, 2, 2, file);
            }
            0x19 => {
                byteswap_fread(m, o, w + lclmap::CROP + 6, 2, 2, file);
            }
            0x1a => via_scratch(m, o, lclmap::SWAP_UV),
            0x1b => via_scratch(m, o, lclmap::ALTERNATE_U),
            0x1c => via_scratch(m, o, lclmap::ALTERNATE_V),
            0x1d => {
                byteswap_fread(m, o, w + lclmap::REFLECTIVITY, 4, 1, file);
            }
            0x1e => via_scratch(m, o, lclmap::MASK),
            0x1f => via_scratch(m, o, lclmap::CHANNEL),
            0x20 => {
                vector_fread(m, o, w + lclmap::ROTATION, 0xc, 1, file);
            }
            0x21 => {
                vector_fread(m, o, w + lclmap::SCALING, 0xc, 1, file);
            }
            0x22 => {
                vector_fread(m, o, w + lclmap::TRANSLATION, 0xc, 1, file);
            }
            0x23 | 0x25 | 0x233 | 0x234 | 0x235 | 0x236 | 0x237 | 0x238 => {
                let off = match token {
                    0x23 => lclmap::BLACK_WHITE,
                    0x25 => lclmap::DISPLACEMENT,
                    0x233 => lclmap::HARDWARE_1,
                    0x234 => lclmap::HARDWARE_2,
                    0x235 => lclmap::TOKEN_235,
                    0x236 => lclmap::TOKEN_236,
                    0x237 => lclmap::TOKEN_237,
                    _ => lclmap::MASK,
                };
                byteswap_fread(m, o, w + off, 2, 1, file);
            }
            0x24 => {
                // a reference already read is not disposed of first
                let udf = udf_read(m, o, file, protos);
                m.set_u32(w + lclmap::UDF, udf.addr());
            }
            -0xcb => {
                // 0xff35: the token's length, unused, then the list
                byteswap_fread(m, o, scratch4, 4, 1, file);
                user_data_read(m, o, file, Ptr32::new(w + lclmap::USER_DATA));
            }
            _ => {
                file_skip_field(m, o, file, token);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    let t = texture_copy(m, o, Ptr32::new(w));
    for off in [lclmap::PICTURE, lclmap::NAME] {
        if m.u32(w + off) != 0 {
            m.free(m.u32(w + off));
            m.set_u32(w + off, 0);
        }
    }
    udf_dispose(m, o, Ptr32::new(m.u32(w + lclmap::UDF)));
    let p = t.addr();
    if m.u16(p + lclmap::METHOD).wrapping_sub(7) < 2 {
        for off in [lclmap::BLENDING, lclmap::AMBIENT, lclmap::TRANSPARENCY, lclmap::DIFFUSE, lclmap::REFLECTIVITY, lclmap::SPECULAR, lclmap::ROUGHNESS] {
            m.set_u32(p + off, 0);
        }
    }
    m.release(FRAME);
    t
}

/// [soft 0x083e77c8 si_texture3DBinaryRead]: a 3D texture, through a working copy on the
/// stack like the 2D one. The channel (+0x2c) is 2 in a file below version 2.1 (the double
/// at 0x0865b7a0, against `b_matio.c`'s `fileVersion`), before the tokens and again after.
///
/// The old tokens 7, 8 and 9 give three colours of three floats for ramp entries 1, 3 and
/// 5, with alpha 0. If 7 was among the tokens, entries 2 and 4 become the means of their
/// neighbours (alpha 0) and the three factors become f/2, f and f + (1 − f)/2 from the
/// first factor f, computed in the x87 unit and stored as floats. The working copy's name
/// and shader reference are freed at the end; its user data list is not.
pub fn texture3d_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) -> Ptr32<Texture3D> {
    // the frame: a 4-byte scratch, a float scratch, a short scratch with the token behind
    // it, the working copy
    const FRAME: u32 = 12 + t3d::SIZE;
    let frame = m.local(FRAME);
    let (scratch4, scratch_f, scratch, token_at, w) = (frame, frame + 4, frame + 8, frame + 10, frame + 12);
    let mut old_colours = false;
    texture3d_initialize(m, Ptr32::new(w));
    if (m[FILE_VERSION_MATIO] as f64) < 2.1 {
        m.set_u16(w + t3d::CHANNEL, 2);
    }
    let list_at = protos.addr().wrapping_add(4 * slot::TEXTURE3D as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        let token = m.i16(token_at);
        match token {
            1 => {
                if m.u32(w + t3d::NAME) != 0 {
                    m.free(m.u32(w + t3d::NAME));
                    m.set_u32(w + t3d::NAME, 0);
                }
                let s = string_binary_read(m, o, file);
                m.set_u32(w + t3d::NAME, s);
            }
            // a short read into the scratch word and stored from it, widened
            2 | 3 => {
                byteswap_fread(m, o, scratch, 2, 1, file);
                let v = m.i16(scratch) as i32;
                m.set_i32(w + if token == 2 { t3d::TYPE } else { t3d::ITERATIONS }, v);
            }
            7 | 8 | 9 => {
                let c = w + t3d::COLORS + 0x20 * (token as u32 - 7);
                rgb_fread(m, o, c, 0xc, 1, file);
                m.set_u32(c + 0xc, 0);
                if token == 7 {
                    old_colours = true;
                }
            }
            0xb | 0xc | 0xd => {
                let off = match token {
                    0xb => t3d::SCALING,
                    0xc => t3d::ROTATION,
                    _ => t3d::TRANSLATION,
                };
                vector_fread(m, o, w + off, 0xc, 1, file);
            }
            0x16..=0x1a => {
                rgba_fread(m, o, w + t3d::COLORS + 0x10 * (token as u32 - 0x16), 0x10, 1, file);
            }
            0x1e => {
                m.set_f32(w + t3d::BLENDING, 1.0);
                byteswap_fread(m, o, scratch, 2, 1, file);
                let mask = if m.i16(scratch) == 1 { 1 } else { 3 };
                m.set_u16(w + t3d::MASK, mask);
            }
            0x22 => {
                let udf = udf_read(m, o, file, protos);
                m.set_u32(w + t3d::UDF, udf.addr());
            }
            -0xdc => {
                // 0xff24: the token's length, unused, then the list
                byteswap_fread(m, o, scratch4, 4, 1, file);
                user_data_read(m, o, file, Ptr32::new(w + t3d::USER_DATA));
            }
            4 | 5 | 6 | 10 | 0xe..=0x15 | 0x1b | 0x1c | 0x1d | 0x1f | 0x20 | 0x21 | 0x23 => {
                // one item straight into its field; 6 and 20 go to a scratch float
                let (at, size) = match token {
                    4 => (w + t3d::STRENGTH, 4),
                    5 => (w + t3d::SPACING, 4),
                    6 | 0x14 => (scratch_f, 4),
                    10 => (w + t3d::FACTORS, 4),
                    0xe => (w + t3d::AMBIENT, 4),
                    0xf => (w + t3d::DIFFUSE, 4),
                    0x10 => (w + t3d::SPECULAR, 4),
                    0x11 => (w + t3d::ROUGHNESS, 4),
                    0x12 => (w + t3d::TRANSPARENCY, 4),
                    0x13 => (w + t3d::REFLECTION, 4),
                    0x15 => (w + t3d::POWER, 4),
                    0x1b => (w + t3d::FACTORS + 4, 4),
                    0x1c => (w + t3d::FACTORS + 8, 4),
                    0x1d => (w + t3d::TAPER, 4),
                    0x1f => (w + t3d::BLENDING, 4),
                    0x20 => (w + t3d::MASK, 2),
                    0x21 => (w + t3d::CHANNEL, 2),
                    _ => (w + t3d::DISPLACEMENT, 2),
                };
                byteswap_fread(m, o, at, size, 1, file);
            }
            _ => {
                file_skip_field(m, o, file, token);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    if old_colours {
        let half = F80::from_f64(0.5); // the double at 0x0865b7a8
        let c = w + t3d::COLORS;
        // entries 2 and 4 (at +0x10 and +0x30 of the ramp) from 1, 3 and 5
        for (dst, a, b) in [(0x10, 0x00, 0x20), (0x30, 0x20, 0x40)] {
            for k in 0..3 {
                let sum = F80::from_f32(m.f32(c + a + 4 * k)) + F80::from_f32(m.f32(c + b + 4 * k));
                m.set_f32(c + dst + 4 * k, (sum * half).to_f32());
            }
        }
        let f = F80::from_f32(m.f32(w + t3d::FACTORS));
        m.set_f32(w + t3d::FACTORS + 4, f.to_f32());
        m.set_f32(w + t3d::FACTORS, (f * half).to_f32());
        m.set_f32(w + t3d::FACTORS + 8, (f + (F80::ONE - f) * half).to_f32());
        m.set_u32(c + 0x1c, 0);
        m.set_u32(c + 0x3c, 0);
    }
    if (m[FILE_VERSION_MATIO] as f64) < 2.1 {
        m.set_u16(w + t3d::CHANNEL, 2);
    }
    let t = texture3d_copy(m, o, Ptr32::new(w));
    if m.u32(w + t3d::NAME) != 0 {
        m.free(m.u32(w + t3d::NAME));
        m.set_u32(w + t3d::NAME, 0);
    }
    udf_dispose(m, o, Ptr32::new(m.u32(w + t3d::UDF)));
    m.release(FRAME);
    t
}

/// [soft 0x083e7f90 si_materialBinaryRead]: a material, read straight into a new record
/// (`SI_materialAllocate`), which is first put at the head of the scene's list of
/// materials (`SI_grafLists`, entry `SI_procData[1]`, +0x14; linked through +0x6e4).
///
/// The name is copied into the record's 30 bytes without a bound. Token 16 is a list of
/// 2D textures, token 17 one of 3D textures: a short count, per item a short that says
/// "external", then a name (read and dropped) or a record. The 2D textures are chained
/// (+0x6ec the first, also stored at +0x6f0; a texture's +0x16c the next); of the 3D
/// textures the first is kept and each further one freed with its name only (its shader
/// reference and user data stay allocated). Token 9 (old) stores the mean of three floats
/// as the reflectivity, computed in the x87 unit. A shading model of 7 under token 0x218
/// becomes 1 with the two shorts at +0xdc8 set to 2.
pub fn material_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) -> Ptr32<Material> {
    let material = material_allocate(m);
    if material.is_null() {
        return material;
    }
    let p = material.addr();
    let entry = (m.i32(m[SI_PROC_DATA].wrapping_add(4)).wrapping_mul(0x24) as u32).wrapping_add(m[SI_GRAF_LISTS]);
    if entry != 0 {
        let head = m.u32(entry.wrapping_add(0x14));
        m.set_u32(p + mat::NEXT, head);
        m.set_u32(entry.wrapping_add(0x14), p);
    }
    // the frame: two 4-byte scratch words, the "external" short, the count, the token,
    // three floats
    const FRAME: u32 = 0x1c;
    let frame = m.local(FRAME);
    let (scratch_a, scratch_b, external_at, count_at, token_at, rgb) = (frame, frame + 4, frame + 8, frame + 10, frame + 12, frame + 0x10);
    let list_at = protos.addr().wrapping_add(4 * slot::MATERIAL as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        let token = m.i16(token_at);
        match token {
            1 => {
                let s = string_binary_read(m, o, file);
                si_mem::clib::strcpy(m, p + mat::NAME, s);
                if s != 0 {
                    m.free(s);
                }
            }
            3 | 4 | 5 => {
                let off = match token {
                    3 => mat::AMBIENT,
                    4 => mat::DIFFUSE,
                    _ => mat::SPECULAR,
                };
                rgb_fread(m, o, p + off, 0xc, 1, file);
            }
            9 => {
                rgb_fread(m, o, rgb, 0xc, 1, file);
                // the double at 0x0865bbb8
                let mean = (F80::from_f32(m.f32(rgb)) + F80::from_f32(m.f32(rgb + 4)) + F80::from_f32(m.f32(rgb + 8))) / F80::from_f64(3.0);
                m.set_f32(p + mat::REFLECTIVITY, mean.to_f32());
            }
            0x10 => {
                byteswap_fread(m, o, count_at, 2, 1, file);
                let mut i: i16 = 0;
                let mut last = 0u32;
                while i < m.i16(count_at) {
                    byteswap_fread(m, o, external_at, 2, 1, file);
                    let texture = if m.i16(external_at) == 0 {
                        texture_binary_read(m, o, file, protos).addr()
                    } else {
                        let s = string_binary_read(m, o, file);
                        if s != 0 {
                            m.free(s);
                        }
                        0
                    };
                    if texture != 0 {
                        if last == 0 {
                            m.set_u32(p + mat::TEXTURES, texture);
                            m.set_u32(p + mat::TEXTURE_CURRENT, texture);
                        } else {
                            m.set_u32(last + lclmap::NEXT, texture);
                        }
                        last = texture;
                    }
                    i += 1;
                }
            }
            0x11 => {
                byteswap_fread(m, o, count_at, 2, 1, file);
                let mut i: i16 = 0;
                while i < m.i16(count_at) {
                    byteswap_fread(m, o, external_at, 2, 1, file);
                    let texture = if m.i16(external_at) == 0 {
                        texture3d_binary_read(m, o, file, protos).addr()
                    } else {
                        let s = string_binary_read(m, o, file);
                        if s != 0 {
                            m.free(s);
                        }
                        0
                    };
                    if texture != 0 {
                        if m.u32(p + mat::TEXTURE3D) == 0 {
                            m.set_u32(p + mat::TEXTURE3D, texture);
                        } else {
                            if m.u32(texture + t3d::NAME) != 0 {
                                m.free(m.u32(texture + t3d::NAME));
                                m.set_u32(texture + t3d::NAME, 0);
                            }
                            m.free(texture);
                        }
                    }
                    i += 1;
                }
            }
            0x15 | 0x16 | 0x17 => {
                let off = match token {
                    0x15 => mat::UDF_MATERIAL,
                    0x16 => mat::UDF_VOLUME,
                    _ => mat::UDF_SHADOW,
                };
                let udf = udf_read(m, o, file, protos);
                m.set_u32(p + off, udf.addr());
            }
            0x218 => {
                byteswap_fread(m, o, p + mat::SHADING, 2, 1, file);
                if m.i16(p + mat::SHADING) == 7 {
                    m.set_i16(p + mat::SHADING, 1);
                    m.set_i16(p + mat::TOKEN_21C, 2);
                    m.set_i16(p + mat::TOKEN_21D, 2);
                }
            }
            -0xe7 => {
                // 0xff19: the token's length, unused, then the list
                byteswap_fread(m, o, scratch_a, 4, 1, file);
                user_data_read(m, o, file, Ptr32::new(p + mat::USER_DATA));
            }
            -0xe6 => {
                // 0xff1a: the token's length, unused, then a shader reference
                byteswap_fread(m, o, scratch_b, 4, 1, file);
                let udf = udf_read(m, o, file, protos);
                m.set_u32(p + mat::UDF_FOURTH, udf.addr());
            }
            2 | 6 | 7 | 8 | 10 | 0xb | 0xd | 0xe | 0xf | 0x12 | 0x13 | 0x14 | 0x21b | 0x21c | 0x21d => {
                let (off, size) = match token {
                    2 | 0x21b => (mat::SHADING, 2),
                    6 => (mat::SPECULAR_EXPONENT, 4),
                    7 => (mat::TRANSPARENCY, 4),
                    8 => (mat::TOKEN_8, 4),
                    10 => (mat::TOKEN_10, 4),
                    0xb => (mat::REFRACTION, 4),
                    0xd => (mat::REFLECTIVITY, 4),
                    0xe => (mat::GLOW_FLAG, 2),
                    0xf => (mat::GLOW, 4),
                    0x12 => (mat::BLUR_FLAG, 2),
                    0x13 => (mat::BLUR_WIDTH, 4),
                    0x14 => (mat::BLUR_DECAY, 4),
                    0x21c => (mat::TOKEN_21C, 2),
                    _ => (mat::TOKEN_21D, 2),
                };
                byteswap_fread(m, o, p + off, size, 1, file);
            }
            _ => {
                file_skip_field(m, o, file, token);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    m.release(FRAME);
    material
}

/// What the three file readers share: `name` with the suffix appended is opened (unless
/// files come from a pipe, when the pipe's stream is read), the header and the tag are
/// read, `b_matio.c`'s `fileVersion` is set, a prototype block is read if the tag is
/// `PROT` (and the tag after it), and the record is read if the tag is `want`. The header
/// reader's answer is not looked at: after a refused header the stream stands at its
/// start, the version is −1, and the first four bytes are the tag.
///
/// The path is built in 256 bytes of stack (`sprintf`); a longer one runs over the
/// original's frame. The port records a fault for it and answers null.
fn read_binary_file(
    m: &mut Mem,
    o: &mut dyn Outside,
    name: u32,
    suffix: &[u8],
    mode_at: u32,
    want: u32,
    read: &mut dyn FnMut(&mut Mem, &mut dyn Outside, u32, Ptr32<Prototypes>) -> u32,
) -> u32 {
    const FRAME: u32 = 0x10c;
    let frame = m.local(FRAME);
    let (protos_at, tag_at, version_at, path) = (frame, frame + 4, frame + 8, frame + 12);
    let mut result = 0;
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
    if file != 0 {
        read_bin_header(m, o, file, version_at, 0);
        byteswap_fread(m, o, tag_at, 4, 1, file);
        m[FILE_VERSION_MATIO] = m.f32(version_at);
        let protos = prototype_allocate(m);
        m.set_u32(protos_at, protos.addr());
        if m.u32(tag_at) == 0x5052_4f54 {
            prototype_binary_read(m, o, file, protos);
            byteswap_fread(m, o, tag_at, 4, 1, file);
        }
        if m.u32(tag_at) == want {
            result = read(m, o, file, Ptr32::new(m.u32(protos_at)));
        }
        if m[SI_PIPED_FILES] == 0 {
            o.fclose(m, file);
        }
        prototype_dispose(m, Ptr32::new(protos_at));
    }
    m.release(FRAME);
    result
}

/// `"rb"`, the mode the three file readers open with (one copy per reader in the binary).
pub const S_RB_MTR: u32 = 0x086ce536;
pub const S_RB_TXT: u32 = 0x086ce54f;
pub const S_RB_T3D: u32 = 0x086ce568;

/// [soft 0x083e8b14 SI_materialReadBinaryFile]: the material of the file `name` + `.mtr`
/// (tag `MTRL`), or null.
pub fn material_read_binary_file(m: &mut Mem, o: &mut dyn Outside, name: u32) -> Ptr32<Material> {
    Ptr32::new(read_binary_file(m, o, name, b".mtr", S_RB_MTR, 0x4d54_524c, &mut |m, o, f, p| material_binary_read(m, o, f, p).addr()))
}

/// [soft 0x083e8d30 SI_textureReadBinaryFile]: the 2D texture of the file `name` + `.txt`
/// (tag `TXMP`), or null.
pub fn texture_read_binary_file(m: &mut Mem, o: &mut dyn Outside, name: u32) -> Ptr32<LclMap> {
    Ptr32::new(read_binary_file(m, o, name, b".txt", S_RB_TXT, 0x5458_4d50, &mut |m, o, f, p| texture_binary_read(m, o, f, p).addr()))
}

/// [soft 0x083e8f50 SI_texture3DReadBinaryFile]: the 3D texture of the file `name` + `.t3D`
/// (tag `TX3D`), or null.
pub fn texture3d_read_binary_file(m: &mut Mem, o: &mut dyn Outside, name: u32) -> Ptr32<Texture3D> {
    Ptr32::new(read_binary_file(m, o, name, b".t3D", S_RB_T3D, 0x5458_3344, &mut |m, o, f, p| texture3d_binary_read(m, o, f, p).addr()))
}
