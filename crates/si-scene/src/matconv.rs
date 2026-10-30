// SPDX-License-Identifier: MIT
//! The material conversion of the load (`scn2db.c`, `SI_dbConvertModelMatTxt2DB`, which
//! gives a node's own material and textures elements of their own) and the copies it makes:
//! `SI_materialCopy` and `SI_materialTransfer` (`mat_dial.c`), `SI_txt2dCopy` and
//! `SI_txt2dTransfer` (`t2d_dial.c`), `SI_txt3dCopy` and `SI_txt3dTransfer` (`t3d_dial.c`),
//! `SI_copyT2DShader` and `SI_copyT3DShader` (`shd_fct.c`), `dbConvertUdf2DB` (`scn2db.c`),
//! on the memory image.
//! The disposals they mirror are [`crate::dispose`]'s; every callee outside these five
//! files goes through [`ByAddr`], but `edtools.c`'s look-ups and relations, which the
//! conversion ([`db_convert_model_mat_txt2db`]) calls as the ports of [`crate::edtools`].
//! The load calls the conversion through [`db_check_convert_models_mat_txt`]
//! (`scn2db.c`, `SI_dbCheckConvertModelsMatTxt`).
//!
//! **Compared with the emulated original** (`re/tools/si-scene/emu_gen_edmat.py`,
//! `scene-emucheck re/out/si-scene/vec_edmat`): `SI_malloc`, `SI_calloc`, the dictionary's
//! look-up and `SI_elemdictGetNewName` ran as they are; `SI_materialTransfer` (for
//! `SI_materialCopy`), `SI_txt2dTransfer`, `SI_txt3dTransfer`, `SI_udfCopy`, `SI_udfDispose`, `SI_edAddUDF`,
//! `SI_elemdictChangeName`, `SI_edSoftElemChangeName` and `SI_userDataCopyFromSetup` were
//! recorded stand-ins. For `SI_txt2dTransfer` `SI_updTxt2dEffects` ran as it is too
//! (`si-hrc`'s port, called here directly); for `SI_txt3dTransfer` `SI_updTxt3dTurb` was a
//! recorded stand-in (`si-render`'s port, called here by address). For the conversion
//! (the vectors' tenth part, on the sample scenes' hierarchies) those look-ups and
//! relations, `dbConvertUdf2DB`, the dictionary's routines and the conversion itself below a
//! node ran as they are; its other callees were recorded stand-ins.

use crate::dispose::{graf_list, SI_UDF_DISPOSE};
use crate::edtools::{
    attdict_flag_all_material_fcurves, ed_get_elem_from_material, ed_get_elem_from_model, ed_get_elem_from_texture2d, ed_get_elem_from_texture3d,
    ed_model_relax_g_texture2d, ed_model_relax_g_texture3d, ed_model_relax_material, ed_model_set_g_texture2d, ed_model_set_g_texture3d, ed_model_set_l_texture2d,
    ed_model_set_l_texture3d, ed_model_set_material, ByAddr, SI_COPY_T2D_SHADER, SI_ELEMDICT_FIND_ELEM_BY_DATA, SI_ELEMDICT_FIND_RELATED_ELEM,
    SI_ELEMDICT_GET_ELEM, SI_ELEMDICT_GET_NEW_NAME, SI_MATERIAL_COPY, SI_TEXTURE2D_LOAD_FCURVE_ALL, SI_TXT2D_COPY, SI_TXT2D_SET_WRAP,
};
use crate::globals::{SI_ADD_IN_DICTIONARY, SI_ED_SUMATRA_TO_3X, SI_GLB_SCHEM, S_T2D, S_T3D};
use crate::wave::SI_GRAFLIST_ADD_DATA;
use si_x87::x87::fmov;
use si_x87::F80;
use crate::world::current_dict;
use si_dict::globals::{SI_CID, SI_DBCID};
use si_mem::{Mem, Ptr32};

/// `SI_defaultMaterial` (0x0869dde0), `mat_dial.c`: the material every copy starts from,
/// 0xdcc bytes.
pub const SI_DEFAULT_MATERIAL: u32 = 0x0869_dde0;
/// `SI_materialTransfer (0x08284504)`, `mat_dial.c`: (from, to, rename); [`material_transfer`].
pub const SI_MATERIAL_TRANSFER: u32 = 0x0828_4504;
/// `SI_txt2dTransfer (0x082adb54)`, `t2d_dial.c`: (from, to, how the shader goes);
/// [`txt2d_transfer`].
pub const SI_TXT2D_TRANSFER: u32 = 0x082a_db54;
/// `SI_udfCopy (0x0854c70c)`, `udf.c`: (shader); `si-hrc`'s `image::udf::udf_copy`.
pub const SI_UDF_COPY: u32 = 0x0854_c70c;
/// `SI_edAddUDF (0x08103b5c)`, `edtools.c`: (owner, chapter, shader);
/// [`crate::edtools::ed_add_udf`].
pub const SI_ED_ADD_UDF: u32 = 0x0810_3b5c;

/// `SI_elemdictChangeName (0x08136450)`, `elemdict.c`: (dictionary, chapter, element,
/// prefix, name); `si-dict`'s `elemname::change_name`.
pub const SI_ELEMDICT_CHANGE_NAME: u32 = 0x0813_6450;
/// `SI_edSoftElemChangeName (0x080fe96c)`, `edtools.c`: (element, chapter, name). Ported in
/// store form only (`dictout.rs`), not on the image.
pub const SI_ED_SOFT_ELEM_CHANGE_NAME: u32 = 0x080f_e96c;
/// `SI_userDataCopyFromSetup (0x084630dc)`, `model.c`: (user data, &copy); `si-hrc`'s
/// `image::userdata::user_data_copy_from_setup`.
pub const SI_USER_DATA_COPY_FROM_SETUP: u32 = 0x0846_30dc;
/// `SI_glbDuplic` (0x087814c0) + 0x26, 16 bits: `DUP_USERDATA_DUP` of the setup file's
/// `DUPLICATE_SETUP` (`si-ascii`'s `DUPLICATE_NAMES`).
pub const GLB_DUPLIC_USERDATA_DUP: u32 = 0x0878_14e6;

/// How [`material_transfer`] moves one field: 16 bits, 32 bits, or a `float` through the
/// x87 unit (`flds`, `fstps`: a signalling NaN comes out quiet).
#[derive(Clone, Copy)]
enum Move {
    W16,
    W32,
    F32,
}

/// The fields [`material_transfer`] copies one by one, in its order.
#[rustfmt::skip]
const TRANSFERRED: [(u32, Move); 32] = {
    use Move::*;
    [
        (0x1e, W16), (0x20, W16), (0x24, F32), (0x28, W16), (0x2c, F32),
        (0x34, W32), (0x38, W32), (0x3c, W32), (0x40, W32), (0x44, W32), (0x48, W32),
        (0x4c, W32), (0x50, W32), (0x54, W32), (0x58, W32), (0x5c, W32), (0x60, W32),
        (0x64, F32), (0x6a8, F32), (0x6ac, F32), (0x6b8, F32), (0x6bc, F32),
        (0x6cc, W32), (0x6d0, W32), (0x6d4, W32), (0x6d8, W32), (0x6dc, F32),
        (0x758, W16), (0x75c, F32), (0x760, F32), (0xdc8, W16), (0xdca, W16),
    ]
};

/// Makes a material the copy of another [soft 0x08284504 SI_materialTransfer]: nothing when
/// either is null.
///
/// The name (`strcpy` at +0). With `rename` (16 bits) and an element for `to`
/// (`SI_edGetElemFromMaterial`, its index into an uninitialised local), the element is
/// renamed: `SI_elemdictGetNewName(current dictionary, MATERIALS, to)`, then
/// `SI_elemdictChangeName(current dictionary, MATERIALS, element, the prefix of its name
/// record (+0x10), new name)` and `SI_edSoftElemChangeName(element, MATERIALS, new name)`
/// (the record is read through the element without a test). Then the fields of
/// [`TRANSFERRED`] one by one, and 1 in the 16 bits at +0xdac. The four shaders at
/// +0xdb0: without `rename` the words are copied; with it the four of `to` are disposed
/// (`SI_udfDispose`), then each of `from`'s is copied (`SI_udfCopy`) into its place. The user data at +0xdc4: `SI_userDataCopyFromSetup(from's, &to's)` when
/// `DUP_USERDATA_DUP` is set and `from` has some, else null.
pub fn material_transfer<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, from: u32, to: u32, rename: i16) {
    if from == 0 || to == 0 {
        return;
    }
    si_mem::clib::strcpy(m, to, from);
    if rename != 0 {
        let index = m.local(4);
        let elem = ed_get_elem_from_material(o, m, to, Ptr32::new(index));
        if !elem.is_null() {
            let (chapter, dict) = (m[SI_CID].mat as u32, current_dict(m));
            let name = o.call_addr(m, SI_ELEMDICT_GET_NEW_NAME, &[dict.0, chapter, to]);
            let prefix = m.u32(m.u32(elem.0).wrapping_add(0x10));
            let (chapter, dict) = (m[SI_CID].mat as u32, current_dict(m));
            o.call_addr(m, SI_ELEMDICT_CHANGE_NAME, &[dict.0, chapter, elem.0, prefix, name]);
            let chapter = m[SI_CID].mat as u32;
            o.call_addr(m, SI_ED_SOFT_ELEM_CHANGE_NAME, &[elem.0, chapter, name]);
        }
        m.release(4);
    }
    for (at, how) in TRANSFERRED {
        transfer(m, from, to, at, how);
    }
    m.set_u16(to + 0xdac, 1);
    if rename == 0 {
        for k in 0..4 {
            m.set_u32(to + 0xdb0 + 4 * k, m.u32(from + 0xdb0 + 4 * k));
        }
    } else {
        for k in 0..4 {
            let shader = m.u32(to + 0xdb0 + 4 * k);
            o.call_addr(m, SI_UDF_DISPOSE, &[shader]);
        }
        for k in 0..4 {
            let shader = m.u32(from + 0xdb0 + 4 * k);
            let copy = o.call_addr(m, SI_UDF_COPY, &[shader]);
            m.set_u32(to + 0xdb0 + 4 * k, copy);
        }
    }
    let user = m.u32(from + 0xdc4);
    if m.u16(GLB_DUPLIC_USERDATA_DUP) != 0 && user != 0 {
        o.call_addr(m, SI_USER_DATA_COPY_FROM_SETUP, &[user, to + 0xdc4]);
    } else {
        m.set_u32(to + 0xdc4, 0);
    }
}

/// The fields [`txt2d_transfer`] copies one by one after the strings, in its order (the
/// eight words at +0x128 are one `rep movsl`).
#[rustfmt::skip]
const T2D_TRANSFERRED: [(u32, Move); 62] = {
    use Move::*;
    [
        (0x1c8, W16), (0x1c, W16), (0x106, W16), (0x108, W16),
        (0x174, W32), (0x178, W32), (0x17c, W32), (0x180, W32), (0x184, W32), (0x188, W32),
        (0x1cc, W16), (0x1ce, W16), (0x18c, W16), (0x190, W32),
        (0x28, W16), (0x2a, W16), (0x2c, W16), (0x18e, W16), (0x2e, W16), (0x30, W16), (0x32, W16), (0x34, W16), (0x36, W16),
        (0x58, F32), (0x5c, F32), (0x68, F32), (0x6c, F32),
        (0xf8, W16), (0xfa, W16), (0xfc, W16), (0xfe, W16), (0x100, W16), (0x102, W16), (0x104, W16), (0x10a, W16),
        (0x10c, F32), (0x110, W16),
        (0x128, W32), (0x12c, W32), (0x130, W32), (0x134, W32), (0x138, W32), (0x13c, W32), (0x140, W32), (0x144, W32),
        (0x168, F32),
        (0x7c, W32), (0x80, W32), (0x84, W32), (0x88, W32), (0x8c, W32), (0x90, W32), (0x94, W32), (0x98, W32), (0x9c, W32),
        (0xac, W32), (0xb0, W32), (0xb4, W32), (0xa0, W32), (0xa4, W32), (0xa8, W32),
        (0x194, W16),
    ]
};

/// One field of [`TRANSFERRED`] or [`T2D_TRANSFERRED`] from `from` to `to`.
fn transfer(m: &mut Mem, from: u32, to: u32, at: u32, how: Move) {
    match how {
        Move::W16 => m.set_u16(to + at, m.u16(from + at)),
        Move::W32 => m.set_u32(to + at, m.u32(from + at)),
        Move::F32 => m.set_u32(to + at, si_x87::x87::fmov(f32::from_bits(m.u32(from + at))).to_bits()),
    }
}

/// Frees the string at `to + at` when there is one, and clears the word.
fn free_string(m: &mut Mem, to: u32, at: u32) {
    let old = m.u32(to + at);
    if old != 0 {
        m.free(old);
        m.set_u32(to + at, 0);
    }
}

/// Makes a 2D texture (`__SI_LclMap`) the copy of another [soft 0x082adb54 SI_txt2dTransfer].
/// Neither is tested for null.
///
/// The strings, each a block of its own: the picture (+0x14) is `to`'s freed, then a
/// `strdup` of `from`'s (read without a test). The name (+0x18): when `from` has none,
/// `to` gets a `strdup` of `SI_elemdictGetNewName(current dictionary, SI_cid's TEXTURES2D,
/// "t2d")` and its own is not freed; else its own is freed and `from`'s copied. +0x20:
/// `to`'s freed, then a copy of `from`'s or null. +0x10: a copy of `from`'s or null, `to`'s
/// not freed. Then the fields of [`T2D_TRANSFERRED`] (floats through the x87 unit: a
/// signalling NaN comes out quiet). The shader (+0x198) by `how` (32 bits): 1, `to`'s
/// disposed (`SI_udfDispose`) and `SI_udfCopy` of `from`'s; 0, `from`'s word; 2, null;
/// anything else, left. Then the 16 bits at +0x19c, `SI_updTxt2dEffects(to)`, the user data
/// at +0x1d0 as [`material_transfer`] does it (`SI_userDataCopyFromSetup` when
/// `DUP_USERDATA_DUP` is set and `from` has some, else null), and the 16 bits at +0x1d4.
pub fn txt2d_transfer<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, from: u32, to: u32, how: i32) {
    free_string(m, to, 0x14);
    let copy = m.strdup(m.u32(from + 0x14));
    m.set_u32(to + 0x14, copy);
    if m.u32(from + 0x18) == 0 {
        let (chapter, dict) = (m[SI_CID].t2d as u32, current_dict(m));
        let name = o.call_addr(m, SI_ELEMDICT_GET_NEW_NAME, &[dict.0, chapter, S_T2D]);
        let copy = m.strdup(name);
        m.set_u32(to + 0x18, copy);
    } else {
        free_string(m, to, 0x18);
        let copy = m.strdup(m.u32(from + 0x18));
        m.set_u32(to + 0x18, copy);
    }
    free_string(m, to, 0x20);
    for at in [0x20, 0x10] {
        let s = m.u32(from + at);
        let copy = if s == 0 { 0 } else { m.strdup(s) };
        m.set_u32(to + at, copy);
    }
    for (at, how) in T2D_TRANSFERRED {
        transfer(m, from, to, at, how);
    }
    match how {
        1 => {
            let own = m.u32(to + 0x198);
            o.call_addr(m, SI_UDF_DISPOSE, &[own]);
            let shader = m.u32(from + 0x198);
            let copy = o.call_addr(m, SI_UDF_COPY, &[shader]);
            m.set_u32(to + 0x198, copy);
        }
        0 => m.set_u32(to + 0x198, m.u32(from + 0x198)),
        2 => m.set_u32(to + 0x198, 0),
        _ => {}
    }
    m.set_u16(to + 0x19c, m.u16(from + 0x19c));
    si_hrc::image::texture::upd_txt2d_effects(m, Ptr32::new(to));
    let user = m.u32(from + 0x1d0);
    if m.u16(GLB_DUPLIC_USERDATA_DUP) != 0 && user != 0 {
        o.call_addr(m, SI_USER_DATA_COPY_FROM_SETUP, &[user, to + 0x1d0]);
    } else {
        m.set_u32(to + 0x1d0, 0);
    }
    m.set_u16(to + 0x1d4, m.u16(from + 0x1d4));
}

/// A copy of a material [soft 0x08284888 SI_materialCopy]: null for null. Otherwise a block
/// of 0xdcc bytes (`SI_malloc`; null when refused) gets `SI_defaultMaterial`'s bytes, then
/// `SI_materialTransfer(material, copy, 0)`; the copy goes to the head of the scene graph
/// list's materials (+0x14 of the list, linked through +0x6e4). The list's index is read
/// again for the second store.
pub fn material_copy<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, material: u32) -> u32 {
    if material == 0 {
        return 0;
    }
    let copy = m.si_malloc(0xdcc);
    if copy == 0 {
        return 0;
    }
    m.copy(copy, SI_DEFAULT_MATERIAL, 0xdcc);
    o.call_addr(m, SI_MATERIAL_TRANSFER, &[material, copy, 0]);
    let first = m.u32(graf_list(m).wrapping_add(0x14));
    m.set_u32(copy + 0x6e4, first);
    let head = graf_list(m).wrapping_add(0x14);
    m.set_u32(head, copy);
    copy
}

/// A copy of a 2D texture [soft 0x082adfbc SI_txt2dCopy]: a cleared block of 0x1d8 bytes
/// (`SI_calloc(1, 0x1d8)`, cleared once more), `SI_txt2dTransfer(texture, copy, 1)`, then 7
/// in the 16 bits at +0x194 and −1 at +0x1bc. Null when the block is refused (the texture
/// itself is not looked at).
pub fn txt2d_copy<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, texture: u32) -> u32 {
    let copy = m.si_calloc(1, 0x1d8);
    if copy != 0 {
        si_mem::clib::bzero(m, copy, 0x1d8);
        o.call_addr(m, SI_TXT2D_TRANSFER, &[texture, copy, 1]);
        m.set_u16(copy + 0x194, 7);
        m.set_u32(copy + 0x1bc, 0xffff_ffff);
    }
    copy
}

/// Gives a 2D texture its shader [soft 0x082a6864 SI_copyT2DShader]: nothing when the
/// texture or the shader is null. A texture that has none yet (+0x198) gets
/// `SI_udfCopy(shader)`, when that is not null, after `SI_edAddUDF(texture,
/// SI_dbcid.TEXTURES2D, copy)`; a texture that has one hands that one to `SI_edAddUDF`
/// and the shader passed is not looked at.
pub fn copy_t2d_shader<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, texture: u32, shader: u32) {
    if texture == 0 || shader == 0 {
        return;
    }
    let own = m.u32(texture + 0x198);
    if own == 0 {
        let copy = o.call_addr(m, SI_UDF_COPY, &[shader]);
        if copy != 0 {
            let chapter = m[SI_DBCID].t2d as u32;
            o.call_addr(m, SI_ED_ADD_UDF, &[texture, chapter, copy]);
            m.set_u32(texture + 0x198, copy);
        }
    } else {
        let chapter = m[SI_DBCID].t2d as u32;
        o.call_addr(m, SI_ED_ADD_UDF, &[texture, chapter, own]);
    }
}

/// `SI_defaultTxt3d` (0x0869eda0), `t3d_dial.c`: the 3D texture every copy starts from,
/// 0x158 bytes.
pub const SI_DEFAULT_TXT3D: u32 = 0x0869_eda0;
/// `SI_txt3dTransfer (0x082b3d90)`, `t3d_dial.c`: (from, to, how the shader goes);
/// [`txt3d_transfer`].
pub const SI_TXT3D_TRANSFER: u32 = 0x082b_3d90;
/// `SI_updTxt3dTurb (0x082b8d20)`, `t3d_fct.c`: (texture); `si-render`'s
/// `t3d_fct_image::upd_txt3d_turb`.
pub const SI_UPD_TXT3D_TURB: u32 = 0x082b_8d20;
/// `txt` (0x0879e060), a static word of `t3d_dial.c` (`.bss`): where [`txt3d_transfer`]
/// leaves the name it makes for a texture that has none.
pub const T3D_DIAL_TXT: u32 = 0x0879_e060;

/// The fields [`txt3d_transfer`] copies one by one after the name, in its order (the eight
/// words at +0x108 are one `rep movsl`; +0x8c and +0xb4 to +0x104 are not copied).
#[rustfmt::skip]
const T3D_TRANSFERRED: [(u32, Move); 50] = {
    use Move::*;
    [
        (0x0c, W32), (0x10, F32), (0x14, F32), (0x18, F32), (0x1c, W32), (0x20, F32),
        (0x24, W16), (0x28, F32), (0x2c, W16),
        (0x30, W32), (0x34, W32), (0x38, W32), (0x3c, W32), (0x40, W32), (0x44, W32), (0x48, W32),
        (0x4c, W32), (0x50, W32), (0x54, W32), (0x58, W32), (0x5c, W32), (0x60, W32), (0x64, W32),
        (0x68, W32), (0x6c, W32), (0x70, W32), (0x74, W32), (0x78, W32), (0x7c, W32),
        (0x80, F32), (0x84, F32), (0x88, F32),
        (0x90, W32), (0x94, W32), (0x98, W32), (0x9c, W32), (0xa0, W32), (0xa4, W32), (0xa8, W32),
        (0xac, W32), (0xb0, W32),
        (0x108, W32), (0x10c, W32), (0x110, W32), (0x114, W32), (0x118, W32), (0x11c, W32), (0x120, W32), (0x124, W32),
        (0x150, W16),
    ]
};

/// Makes a 3D texture (`__SI_Texture3D`) the copy of another [soft 0x082b3d90
/// SI_txt3dTransfer]. Neither is tested for null.
///
/// The name (+0): `to` gets a `strdup` of `from`'s, its own not freed. When `from` has none,
/// the `strdup` of `SI_elemdictGetNewName(current dictionary, SI_cid's TEXTURES3D, "t3d")`
/// goes to `t3d_dial.c`'s static word `txt` ([`T3D_DIAL_TXT`]), not to `to`, whose name is
/// left as it was. Then the fields of [`T3D_TRANSFERRED`] (eight floats through the x87
/// unit: a signalling NaN comes out quiet). The shader (+0x14c) by `how` (32 bits): 1,
/// `to`'s disposed (`SI_udfDispose`) and `SI_udfCopy` of `from`'s; 0, `from`'s word; 2,
/// null; anything else, left. Then `SI_updTxt3dTurb(to)`, and the user data at +0x154 as
/// [`material_transfer`] does it.
pub fn txt3d_transfer<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, from: u32, to: u32, how: i32) {
    let name = m.u32(from);
    if name != 0 {
        let copy = m.strdup(name);
        m.set_u32(to, copy);
    } else {
        let (chapter, dict) = (m[SI_CID].t3d as u32, current_dict(m));
        let name = o.call_addr(m, SI_ELEMDICT_GET_NEW_NAME, &[dict.0, chapter, S_T3D]);
        let copy = m.strdup(name);
        m.set_u32(T3D_DIAL_TXT, copy);
    }
    for (at, how) in T3D_TRANSFERRED {
        transfer(m, from, to, at, how);
    }
    match how {
        1 => {
            let own = m.u32(to + 0x14c);
            o.call_addr(m, SI_UDF_DISPOSE, &[own]);
            let shader = m.u32(from + 0x14c);
            let copy = o.call_addr(m, SI_UDF_COPY, &[shader]);
            m.set_u32(to + 0x14c, copy);
        }
        0 => m.set_u32(to + 0x14c, m.u32(from + 0x14c)),
        2 => m.set_u32(to + 0x14c, 0),
        _ => {}
    }
    o.call_addr(m, SI_UPD_TXT3D_TURB, &[to]);
    let user = m.u32(from + 0x154);
    if m.u16(GLB_DUPLIC_USERDATA_DUP) != 0 && user != 0 {
        o.call_addr(m, SI_USER_DATA_COPY_FROM_SETUP, &[user, to + 0x154]);
    } else {
        m.set_u32(to + 0x154, 0);
    }
}

/// A copy of a 3D texture (`__SI_Texture3D`) [soft 0x082b4008 SI_txt3dCopy]: a block of
/// 0x158 bytes (`SI_malloc`) gets `SI_defaultTxt3d`'s bytes, then
/// `SI_txt3dTransfer(texture, copy, 1)`. Null when the block is refused; the texture itself
/// is not tested.
pub fn txt3d_copy<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, texture: u32) -> u32 {
    let copy = m.si_malloc(0x158);
    if copy != 0 {
        m.copy(copy, SI_DEFAULT_TXT3D, 0x158);
        o.call_addr(m, SI_TXT3D_TRANSFER, &[texture, copy, 1]);
    }
    copy
}

/// Gives a 3D texture its shader [soft 0x082a68cc SI_copyT3DShader]: [`copy_t2d_shader`]
/// with the shader at +0x14c and `SI_dbcid`'s TEXTURES3D.
pub fn copy_t3d_shader<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, texture: u32, shader: u32) {
    if texture == 0 || shader == 0 {
        return;
    }
    let own = m.u32(texture + 0x14c);
    if own == 0 {
        let copy = o.call_addr(m, SI_UDF_COPY, &[shader]);
        if copy != 0 {
            let chapter = m[SI_DBCID].t3d as u32;
            o.call_addr(m, SI_ED_ADD_UDF, &[texture, chapter, copy]);
            m.set_u32(texture + 0x14c, copy);
        }
    } else {
        let chapter = m[SI_DBCID].t3d as u32;
        o.call_addr(m, SI_ED_ADD_UDF, &[texture, chapter, own]);
    }
}

/// Gives a shader its element when it has none [soft 0x08109454 dbConvertUdf2DB]: nothing
/// for a null shader; else `SI_elemdictFindElemByData(current dictionary, chapter, shader)`,
/// and when that is negative, or `force` (16 bits) is not 0, `SI_edAddUDF(owner, chapter,
/// shader)`.
pub fn db_convert_udf2db<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, shader: u32, force: i16, chapter: i32, owner: u32) {
    if shader == 0 {
        return;
    }
    let dict = current_dict(m);
    let index = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict.0, chapter as u32, shader]) as i32;
    if index < 0 || force != 0 {
        o.call_addr(m, SI_ED_ADD_UDF, &[owner, chapter as u32, shader]);
    }
}

// ---------------------------------------------------------------- the conversion

/// A word of `scn2db.c`'s data (0x08680e5c, no symbol, 0 in the binary): the material the
/// conversion makes once, while `SI_edSumatraTo3x` is set, for nodes that have textures and
/// no material ([`db_convert_model_mat_txt2db`]).
pub const SUMATRA_MATERIAL: u32 = 0x0868_0e5c;
/// `glb2DTxtToPropagateForSumatra` (0x08785588), `scn2db.c` (`.bss`): the 2D textures a node
/// hands on to its sons while `SI_edSumatraTo3x` is set.
pub const GLB_2D_TXT_TO_PROPAGATE: u32 = 0x0878_5588;
/// `glb3DTxtToPropagateForSumatra` (0x0878558c), `scn2db.c` (`.bss`): the same for the 3D
/// texture.
pub const GLB_3D_TXT_TO_PROPAGATE: u32 = 0x0878_558c;
/// `SI_schemComputeElemBBox (0x0833b864)`, `graphsort.c`: (dictionary, element, chapter,
/// low corner, high corner, done); `si-view`'s `graphsort::schem_compute_elem_bbox`.
pub const SI_SCHEM_COMPUTE_ELEM_BBOX: u32 = 0x0833_b864;
/// `SI_edAddMaterial (0x0810203c)`, `edtools.c`: (graph node, unique, material);
/// [`crate::edtools::ed_add_material`].
pub const SI_ED_ADD_MATERIAL: u32 = 0x0810_203c;
/// `SI_edDisconnectSharedMaterialOr3DTxt (0x0810080c)`, `edtools.c`: (dictionary, element,
/// source, shared element, index, material, slot, global);
/// [`crate::edtools::ed_disconnect_shared_material_or3d_txt`].
pub const SI_ED_DISCONNECT_SHARED_MATERIAL_OR3D_TXT: u32 = 0x0810_080c;
/// `SI_elemdictRelationsCount (0x08135700)`, `elemrel.c`: (dictionary, element, forward,
/// group, other group, relation, chapter); `si-dict`'s `elemrel::relations_count`.
pub const SI_ELEMDICT_RELATIONS_COUNT: u32 = 0x0813_5700;

/// The conversion's frame: the index its look-ups write (-0x14), the corners of the node's
/// hierarchy (-0x24 and -0x1c, two floats each), and the two indices of the shared
/// material's look-ups (-0x28, -0x2c).
struct Frame {
    index: u32,
    low: u32,
    high: u32,
    mat_index: u32,
    source: u32,
}

/// Lays an element out beside the node's hierarchy, by `SI_glbSchem`'s first word: 0, at
/// the low corner's x and below its y by `below` (each subtracted in turn); 2, right of the
/// high corner's x by `right` (each added in turn), at its y; 1, the same plus half the
/// element's width (+0x14). Anything else, nothing. Each sum goes through the x87 unit and
/// is rounded once to a float; the copied coordinate is moved through it.
fn lay_out(m: &mut Mem, e: u32, low: [f32; 2], high: [f32; 2], below: &[f64], right: &[f64]) {
    let mode = m.i32(SI_GLB_SCHEM.addr());
    let (x, y) = match mode {
        0 => {
            let y = below.iter().fold(F80::from_f32(low[1]), |s, &k| s - F80::from_f64(k));
            (fmov(low[0]), y.to_f32())
        }
        1 | 2 => {
            let start = if mode == 1 { F80::from_f32(m.f32(e + 0x14)) * F80::from_f32(0.5) + F80::from_f32(high[0]) } else { F80::from_f32(high[0]) };
            let x = right.iter().fold(start, |s, &k| s + F80::from_f64(k));
            (x.to_f32(), fmov(high[1]))
        }
        _ => return,
    };
    m.set_u32(e + 0x0c, x.to_bits());
    m.set_u32(e + 0x10, y.to_bits());
}

/// Gives a model node's own materials and textures elements of their own, then does the same
/// below it [soft 0x08108490 SI_dbConvertModelMatTxt2DB]: `(node, force, recurse,
/// material)`, `force` and `recurse` 16 bits. The node is not tested for null.
///
/// **The hierarchy's corners.** While `SI_edSumatraTo3x` is 0 and the node has an element
/// (`SI_edGetElemFromModel`), its root is `SI_elemdictGetElem(current dictionary, MODELS,
/// the element's +0x50)`. A root whose box (+0x2c, four floats) is four zeros (−0 is zero, a
/// NaN is not) first gets `SI_schemComputeElemBBox(dictionary, root, MODELS, &low, &high,
/// 0)`; the corners are then the root's box, whatever that call left in them. Otherwise they
/// are zeros.
///
/// **The node's own.** Each material of the array at +0x20c (+0x206 of them, 16 bits, the
/// count read again after each, the array at each), each local 2D texture of a material
/// (+0x6ec, linked through +0x16c), its local 3D texture (+0x6fc), each global 2D texture
/// (+0x21c, linked through +0x16c) and the 3D texture (+0x224) is passed over when it has an
/// element and `force` is 0. Otherwise it gets `SI_graflistAddData(the scene graph list, it,
/// 0xe, 0x0f or 0x10, 0)`, its relation (`SI_edModelSetMaterial`,
/// `SI_edModelSetLTexture2D`/`3D` from its material, `SI_edModelSetGTexture2D`/`3D`), and
/// `dbConvertUdf2DB(shader, force, SI_cid's chapter, it)` for each of its shaders (a
/// material's four at +0xdb0, a 2D texture's at +0x198, a 3D texture's at +0x14c). While
/// `SI_edSumatraTo3x` is 0 the element it now has is laid out ([`lay_out`]): a material
/// below the hierarchy by 0.5 and 1.5 or right of it by 2.0 and 0.5, a texture by 4.0 or
/// 5.0. A material and a global texture are laid out only when the node has an element, a
/// local texture only when its material has one (looked up once, after the material). A
/// local 3D texture is laid out by the element it had **before** (so only with `force`).
///
/// **While `SI_edSumatraTo3x` is set** (a scene of the later program), the node's own are
/// moved:
/// - a node of type (+0x30) 0 or 5 with no son loses them: each material's relation is
///   taken away (`SI_edModelRelaxMaterial`; the count read once), then the 2D textures' (when
///   +0x216 is above 0) and the 3D texture's, and the words are cleared (the array is not
///   freed);
/// - a node then with no material but with 2D textures or a 3D texture gets the `material`
///   passed, or else [`SUMATRA_MATERIAL`], which is made once (`SI_materialCopy(
///   SI_defaultMaterial)`, then `SI_edAddMaterial(null, 1, copy)` with `SI_addInDictionary`
///   1, then 0); it is related and becomes the node's one material (a new array of one,
///   `SI_malloc(4)`, not tested). With no 2D texture (+0x216 0), the node gets a copy
///   (`SI_txt2dCopy`) of each of [`GLB_2D_TXT_TO_PROPAGATE`]'s, each with
///   `SI_txt2dSetWrap`, `SI_graflistAddData`, `SI_copyT2DShader` (the source's shader),
///   `SI_edModelSetGTexture2D` and `SI_texture2DLoadFcurveAll`, the first at +0x21c and
///   +0x218, the others linked behind it, +0x216 counting them. With no 3D texture it is
///   related to [`GLB_3D_TXT_TO_PROPAGATE`]'s and takes it;
/// - a node with more than one material and 2D textures or a 3D texture hands them to each
///   material but the first, which becomes `material`. A material whose element has more
///   than one relation 300 (`SI_elemdictRelationsCount`) is first made the node's own:
///   `SI_elemdictFindRelatedElem`, `SI_edDisconnectSharedMaterialOr3DTxt(dictionary, the
///   related element, its index, the material's element, the material's index, 1, slot, 1)`
///   and a `SI_elemdictGetElem(dictionary, 7, the material's index)` whose answer is not
///   used. The 2D textures are copied as above into its local ones (+0x6ec, +0x6f0, counted
///   at +0x6e8, the copies linked behind the first one it had, whose next is lost), the 3D
///   texture related to it (`SI_edModelSetLTexture3D`) when it has none.
/// - A copy whose method (+0x2a, 16 bits) is 4 becomes 6 on a node of type 4.
///
/// **Below.** Nothing more without `recurse`. While `SI_edSumatraTo3x` is set, each son
/// (+0x2c4, then +0x2c0) is converted with `material`; before the first and after each, a
/// node with materials makes its 2D and 3D textures the ones to hand on. Otherwise the son
/// and then the brother are converted, with no material.
pub fn db_convert_model_mat_txt2db<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, force: i16, recurse: i16, material: u32) {
    let at = m.local(0x20);
    let f = Frame { index: at, low: at + 4, high: at + 0xc, mat_index: at + 0x14, source: at + 0x18 };
    convert(o, m, model, force, recurse, material, &f);
    m.release(0x20);
}

/// The load's material conversion of a dictionary's new hierarchies
/// [soft 0x08109664 SI_dbCheckConvertModelsMatTxt]: `(dictionary)`. For each element of its
/// MODELS chapter (`SI_cid`'s, the chapter's address taken once; the count and the element
/// array read again at each) that is in use (+0x48, 16 bits, 0), at depth 0 (+0x54), new
/// to the load (+0x46, 16 bits, 1) and has an object (+0x04): the two globals to hand on
/// ([`GLB_2D_TXT_TO_PROPAGATE`], [`GLB_3D_TXT_TO_PROPAGATE`]) cleared, then
/// [`db_convert_model_mat_txt2db`]`(object, 0, 1, null)`. Then, while `SI_edSumatraTo3x`
/// is set, [`attdict_flag_all_material_fcurves`].
pub fn db_check_convert_models_mat_txt<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, ed: u32) {
    let l = (m[SI_CID].model as u32).wrapping_mul(0x40).wrapping_add(m.u32(ed + 4));
    let mut i = 0;
    let mut at = 0u32;
    while i < m.i32(l + 4) {
        let e = at.wrapping_add(m.u32(l + 8));
        if m.i16(e + 0x48) == 0 && m.u32(e + 0x54) == 0 && m.i16(e + 0x46) == 1 && m.u32(e + 4) != 0 {
            m.set_u32(GLB_2D_TXT_TO_PROPAGATE, 0);
            m.set_u32(GLB_3D_TXT_TO_PROPAGATE, 0);
            let model = m.u32(e + 4);
            db_convert_model_mat_txt2db(o, m, model, 0, 1, 0);
        }
        at = at.wrapping_add(0x84);
        i += 1;
    }
    if sumatra(m) {
        attdict_flag_all_material_fcurves(o, m);
    }
}

/// `SI_edSumatraTo3x`, tested as 16 bits.
fn sumatra(m: &Mem) -> bool {
    m[SI_ED_SUMATRA_TO_3X] != 0
}

/// A copy whose method (+0x2a, 16 bits) is 4 becomes 6 on a node of type 4 (+0x30).
fn method_for_type4(m: &mut Mem, model: u32, copy: u32) {
    if m.u32(model + 0x30) == 4 && m.u16(copy + 0x2a) == 4 {
        m.set_u16(copy + 0x2a, 6);
    }
}

/// A copy of the 2D texture `source` made a node's (`material` 0) or a material's own, at
/// the word `slot` (+0x21c or +0x6ec, or the last copy's +0x16c): its wrap, its graph node,
/// its shader, its relation and its curves. With `first`, the node's first copy is also
/// put at +0x218 once its wrap is set; that word is what the caller keeps as the last copy.
fn copy_texture2d<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, material: u32, slot: u32, source: u32, first: bool) -> u32 {
    let copy = o.call_addr(m, SI_TXT2D_COPY, &[source]);
    m.set_u32(slot, copy);
    method_for_type4(m, model, copy);
    let it = m.u32(slot);
    o.call_addr(m, SI_TXT2D_SET_WRAP, &[model, it]);
    let kept = m.u32(slot);
    if first {
        m.set_u32(model + 0x218, kept);
    }
    let (list, it) = (graf_list(m), m.u32(slot));
    o.call_addr(m, SI_GRAFLIST_ADD_DATA, &[list, it, 0xf, 0]);
    let (shader, it) = (m.u32(source + 0x198), m.u32(slot));
    o.call_addr(m, SI_COPY_T2D_SHADER, &[it, shader]);
    let it = m.u32(slot);
    if material == 0 {
        ed_model_set_g_texture2d(o, m, Ptr32::new(model), it);
    } else {
        ed_model_set_l_texture2d(o, m, material, it);
    }
    let it = m.u32(slot);
    o.call_addr(m, SI_TEXTURE2D_LOAD_FCURVE_ALL, &[it, source]);
    kept
}

#[allow(clippy::too_many_arguments)]
fn convert<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, force: i16, recurse: i16, mut material: u32, f: &Frame) {
    let index = Ptr32::new(f.index);
    let node = Ptr32::new(model);
    // the corners of the node's hierarchy
    let (mut low, mut high) = ([0.0f32; 2], [0.0f32; 2]);
    let mut root = 0;
    if !sumatra(m) {
        let e = ed_get_elem_from_model(o, m, node, index);
        if !e.is_null() {
            let (dict, models) = (current_dict(m), m[SI_CID].model as u32);
            root = o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict.0, models, m.u32(e.0 + 0x50)]);
        }
    }
    if root != 0 {
        if (0..4).all(|k| m.f32(root + 0x2c + 4 * k) == 0.0) {
            let (dict, models) = (current_dict(m), m[SI_CID].model as u32);
            o.call_addr(m, SI_SCHEM_COMPUTE_ELEM_BBOX, &[dict.0, root, models, f.low, f.high, 0]);
        }
        low = [fmov(m.f32(root + 0x2c)), fmov(m.f32(root + 0x30))];
        high = [fmov(m.f32(root + 0x34)), fmov(m.f32(root + 0x38))];
    }
    m.set_u32(f.low, low[0].to_bits());
    m.set_u32(f.low + 4, low[1].to_bits());
    m.set_u32(f.high, high[0].to_bits());
    m.set_u32(f.high + 4, high[1].to_bits());

    // the materials and their local textures
    if m.u32(model + 0x20c) != 0 && m.i16(model + 0x206) > 0 {
        let mut i = 0u32;
        loop {
            let mat = m.u32(m.u32(model + 0x20c) + 4 * i);
            let had = ed_get_elem_from_material(o, m, mat, index);
            if force != 0 || had.is_null() {
                let list = graf_list(m);
                o.call_addr(m, SI_GRAFLIST_ADD_DATA, &[list, mat, 0xe, 0]);
                ed_model_set_material(o, m, node, mat);
                for k in 0..4 {
                    let (chapter, shader) = (m[SI_CID].mat, m.u32(mat + 0xdb0 + 4 * k));
                    db_convert_udf2db(o, m, shader, force, chapter, mat);
                }
                let e = ed_get_elem_from_material(o, m, mat, index);
                if !sumatra(m) && !e.is_null() && !ed_get_elem_from_model(o, m, node, index).is_null() {
                    lay_out(m, e.0, low, high, &[0.5, 1.5], &[2.0, 0.5]);
                }
            }
            let mat_elem = ed_get_elem_from_material(o, m, mat, index);
            let mut t = m.u32(mat + 0x6ec);
            while t != 0 {
                let had = ed_get_elem_from_texture2d(o, m, t, index);
                if force != 0 || had.is_null() {
                    let list = graf_list(m);
                    o.call_addr(m, SI_GRAFLIST_ADD_DATA, &[list, t, 0xf, 0]);
                    ed_model_set_l_texture2d(o, m, mat, t);
                    let (chapter, shader) = (m[SI_CID].t2d, m.u32(t + 0x198));
                    db_convert_udf2db(o, m, shader, force, chapter, t);
                    let e = ed_get_elem_from_texture2d(o, m, t, index);
                    if !sumatra(m) && !e.is_null() && !mat_elem.is_null() {
                        lay_out(m, e.0, low, high, &[4.0], &[5.0]);
                    }
                }
                t = m.u32(t + 0x16c);
            }
            let t3 = m.u32(mat + 0x6fc);
            if t3 != 0 {
                let had = ed_get_elem_from_texture3d(o, m, t3, index);
                if force != 0 || had.is_null() {
                    let list = graf_list(m);
                    o.call_addr(m, SI_GRAFLIST_ADD_DATA, &[list, t3, 0x10, 0]);
                    ed_model_set_l_texture3d(o, m, mat, t3);
                    let (chapter, shader) = (m[SI_CID].t3d, m.u32(t3 + 0x14c));
                    db_convert_udf2db(o, m, shader, force, chapter, t3);
                    // the element it had before, not the one it has now
                    if !sumatra(m) && !had.is_null() && !mat_elem.is_null() {
                        lay_out(m, had.0, low, high, &[4.0], &[5.0]);
                    }
                }
            }
            i += 1;
            if i as i32 >= m.i16(model + 0x206) as i32 {
                break;
            }
        }
    }

    // the node's global textures
    let mut t = m.u32(model + 0x21c);
    while t != 0 {
        let had = ed_get_elem_from_texture2d(o, m, t, index);
        if force != 0 || had.is_null() {
            let list = graf_list(m);
            o.call_addr(m, SI_GRAFLIST_ADD_DATA, &[list, t, 0xf, 0]);
            ed_model_set_g_texture2d(o, m, node, t);
            let (chapter, shader) = (m[SI_CID].t2d, m.u32(t + 0x198));
            db_convert_udf2db(o, m, shader, force, chapter, t);
            let e = ed_get_elem_from_texture2d(o, m, t, index);
            if !sumatra(m) && !e.is_null() && !ed_get_elem_from_model(o, m, node, index).is_null() {
                lay_out(m, e.0, low, high, &[4.0], &[5.0]);
            }
        }
        t = m.u32(t + 0x16c);
    }
    let t3 = m.u32(model + 0x224);
    if t3 != 0 {
        let had = ed_get_elem_from_texture3d(o, m, t3, index);
        if force != 0 || had.is_null() {
            let list = graf_list(m);
            o.call_addr(m, SI_GRAFLIST_ADD_DATA, &[list, t3, 0x10, 0]);
            ed_model_set_g_texture3d(o, m, node, t3);
            let (chapter, shader) = (m[SI_CID].t3d, m.u32(t3 + 0x14c));
            db_convert_udf2db(o, m, shader, force, chapter, t3);
            let e = ed_get_elem_from_texture3d(o, m, t3, index);
            if !sumatra(m) && !e.is_null() && !ed_get_elem_from_model(o, m, node, index).is_null() {
                lay_out(m, e.0, low, high, &[4.0], &[5.0]);
            }
        }
    }

    if sumatra(m) {
        sumatra_move(o, m, model, &mut material, f);
    }

    // below
    if recurse == 0 {
        return;
    }
    if sumatra(m) {
        let hand_on = |m: &mut Mem| {
            if m.i16(model + 0x206) > 0 {
                let (t2, t3) = (m.u32(model + 0x21c), m.u32(model + 0x224));
                m.set_u32(GLB_2D_TXT_TO_PROPAGATE, t2);
                m.set_u32(GLB_3D_TXT_TO_PROPAGATE, t3);
            }
        };
        hand_on(m);
        let mut son = m.u32(model + 0x2c4);
        while son != 0 {
            db_convert_model_mat_txt2db(o, m, son, force, recurse, material);
            hand_on(m);
            son = m.u32(son + 0x2c0);
        }
    } else {
        for next in [0x2c4, 0x2c0] {
            let other = m.u32(model + next);
            if other != 0 {
                db_convert_model_mat_txt2db(o, m, other, force, recurse, 0);
            }
        }
    }
}

/// The part of [`db_convert_model_mat_txt2db`] made while `SI_edSumatraTo3x` is set: a leaf
/// of type 0 or 5 loses its own, a node with none gets a material and the textures handed
/// on, a node with more than one material hands its textures to all but the first (which
/// becomes `material`).
fn sumatra_move<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, material: &mut u32, f: &Frame) {
    let node = Ptr32::new(model);
    let kind = m.u32(model + 0x30);
    if (kind == 0 || kind == 5) && m.u32(model + 0x2c4) == 0 {
        let n = m.i16(model + 0x206) as i32;
        if n > 0 {
            for i in 0..n as u32 {
                let mat = m.u32(m.u32(model + 0x20c) + 4 * i);
                ed_model_relax_material(o, m, model, mat);
            }
            m.set_u32(model + 0x20c, 0);
            m.set_u16(model + 0x206, 0);
        }
        if m.i16(model + 0x216) > 0 {
            let mut t = m.u32(model + 0x21c);
            while t != 0 {
                ed_model_relax_g_texture2d(o, m, model, t);
                t = m.u32(t + 0x16c);
            }
            m.set_u32(model + 0x21c, 0);
            m.set_u16(model + 0x216, 0);
        }
        let t3 = m.u32(model + 0x224);
        if t3 != 0 {
            ed_model_relax_g_texture3d(o, m, model, t3);
            m.set_u32(model + 0x224, 0);
        }
    }
    let count = m.i16(model + 0x206);
    let textured = |m: &Mem| m.i16(model + 0x216) > 0 || m.u32(model + 0x224) != 0;
    if count == 0 {
        if !textured(m) {
            return;
        }
        if *material == 0 {
            if m.u32(SUMATRA_MATERIAL) == 0 {
                let copy = o.call_addr(m, SI_MATERIAL_COPY, &[SI_DEFAULT_MATERIAL]);
                m.set_u32(SUMATRA_MATERIAL, copy);
                m[SI_ADD_IN_DICTIONARY] = 1;
                o.call_addr(m, SI_ED_ADD_MATERIAL, &[0, 1, copy]);
                m[SI_ADD_IN_DICTIONARY] = 0;
            }
            *material = m.u32(SUMATRA_MATERIAL);
        }
        if *material != 0 {
            ed_model_set_material(o, m, node, *material);
            let array = m.si_malloc(4);
            m.set_u32(model + 0x20c, array);
            m.set_u32(array, *material);
            m.set_u16(model + 0x206, 1);
        }
        if m.u16(model + 0x216) == 0 {
            // the last copy (a local the original leaves unset until the first copy)
            let mut last = 0;
            let mut t = m.u32(GLB_2D_TXT_TO_PROPAGATE);
            while t != 0 {
                if m.u32(model + 0x21c) == 0 {
                    m.set_u16(model + 0x216, 1);
                    last = copy_texture2d(o, m, model, 0, model + 0x21c, t, true);
                } else {
                    m.set_u16(model + 0x216, m.u16(model + 0x216).wrapping_add(1));
                    copy_texture2d(o, m, model, 0, last + 0x16c, t, false);
                    last = m.u32(last + 0x16c);
                }
                t = m.u32(t + 0x16c);
            }
        }
        if m.u32(model + 0x224) == 0 {
            let t3 = m.u32(GLB_3D_TXT_TO_PROPAGATE);
            if t3 != 0 {
                ed_model_set_g_texture3d(o, m, node, t3);
                let t3 = m.u32(GLB_3D_TXT_TO_PROPAGATE);
                m.set_u32(model + 0x224, t3);
            }
        }
        return;
    }
    if count <= 0 {
        return;
    }
    *material = m.u32(m.u32(model + 0x20c));
    if count <= 1 || !textured(m) {
        return;
    }
    // the last copy of the material's local textures (kept from one material to the next)
    let mut last = 0;
    let mut i = 1u32;
    while (i as i32) < m.i16(model + 0x206) as i32 {
        let mat = m.u32(m.u32(model + 0x20c) + 4 * i);
        let e = ed_get_elem_from_material(o, m, mat, Ptr32::new(f.mat_index));
        let models = m[SI_CID].model as u32;
        let shared = o.call_addr(m, SI_ELEMDICT_RELATIONS_COUNT, &[0, e.0, 0, 0, 3, 300, models]) as i32;
        if shared > 1 {
            let (dict, models) = (current_dict(m), m[SI_CID].model as u32);
            let related = o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict.0, e.0, 0, 0, 300, models, f.index, f.source]);
            let (dict, source, own) = (current_dict(m), m.u32(f.source), m.u32(f.mat_index));
            o.call_addr(m, SI_ED_DISCONNECT_SHARED_MATERIAL_OR3D_TXT, &[dict.0, related, source, e.0, own, 1, i, 1]);
            let (dict, own) = (current_dict(m), m.u32(f.mat_index));
            o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict.0, 7, own]);
        }
        if m.i16(model + 0x216) > 0 {
            let mat = m.u32(m.u32(model + 0x20c) + 4 * i);
            if m.u32(mat + 0x6ec) != 0 {
                last = m.u32(mat + 0x6ec);
            }
            let mut t = m.u32(model + 0x21c);
            while t != 0 {
                if m.u32(mat + 0x6ec) == 0 {
                    m.set_u16(mat + 0x6e8, 1);
                    copy_texture2d(o, m, model, mat, mat + 0x6ec, t, false);
                    last = m.u32(mat + 0x6ec);
                    m.set_u32(mat + 0x6f0, last);
                } else {
                    m.set_u16(mat + 0x6e8, m.u16(mat + 0x6e8).wrapping_add(1));
                    copy_texture2d(o, m, model, mat, last + 0x16c, t, false);
                    last = m.u32(last + 0x16c);
                }
                t = m.u32(t + 0x16c);
            }
        }
        let array = m.u32(model + 0x20c);
        if m.u32(m.u32(array + 4 * i) + 0x6fc) == 0 {
            let t3 = m.u32(model + 0x224);
            if t3 != 0 {
                let mat = m.u32(array + 4 * i);
                ed_model_set_l_texture3d(o, m, mat, t3);
                let (mat, t3) = (m.u32(m.u32(model + 0x20c) + 4 * i), m.u32(model + 0x224));
                m.set_u32(mat + 0x6fc, t3);
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_dict::globals::{SI_ELEM_DICTS, SI_PROC_DATA};
    use si_mem::Ptr32;

    /// Every call logged; `SI_udfCopy` answers 0x7100, `SI_elemdictFindElemByData` what the
    /// test sets.
    struct Log(Vec<(u32, Vec<u32>)>, u32);
    impl ByAddr for Log {
        fn call_addr(&mut self, _m: &mut Mem, f: u32, args: &[u32]) -> u32 {
            self.0.push((f, args.to_vec()));
            match f {
                SI_UDF_COPY => 0x7100,
                SI_ELEMDICT_FIND_ELEM_BY_DATA => self.1,
                _ => 0,
            }
        }
    }

    fn image() -> Mem {
        let mut m = Mem::new();
        si_mem::init_globals(&mut m, si_dict::globals::ALL);
        let dicts = m.malloc(0x70);
        m[SI_ELEM_DICTS] = Ptr32::new(dicts);
        m
    }

    /// Every call logged; `SI_elemdictGetElemFromData` answers `self.1`,
    /// `SI_elemdictGetNewName` 0x7300, `SI_udfCopy` its argument + 1.
    struct Renaming(Vec<(u32, Vec<u32>)>, u32);
    impl ByAddr for Renaming {
        fn call_addr(&mut self, _m: &mut Mem, f: u32, args: &[u32]) -> u32 {
            self.0.push((f, args.to_vec()));
            match f {
                crate::edtools::SI_ELEMDICT_GET_ELEM_FROM_DATA => self.1,
                SI_ELEMDICT_GET_NEW_NAME => 0x7300,
                SI_UDF_COPY => args[0] + 1,
                _ => 0,
            }
        }
    }

    #[test]
    fn a_material_transfer_copies_its_fields_one_by_one() {
        let mut m = image();
        let (from, to) = (m.calloc(1, 0xdcc), m.calloc(1, 0xdcc));
        m.put_cstr(from, b"shiny");
        for k in 8..0x373 {
            m.set_u32(from + 4 * k, 0x100 + k);
            m.set_u32(to + 4 * k, 0xdead);
        }
        // a signalling NaN goes through the unit
        m.set_u32(from + 0x24, 0x7f80_0001);
        let mut o = Renaming(Vec::new(), 0);
        material_transfer(&mut o, &mut m, 0, to, 1);
        material_transfer(&mut o, &mut m, from, 0, 1);
        assert_eq!(m.u32(to + 0x20), 0xdead);
        material_transfer(&mut o, &mut m, from, to, 0);
        assert!(o.0.is_empty());
        assert_eq!(m.cstr(to), b"shiny");
        assert_eq!(m.u32(to + 0x24), 0x7fc0_0001);
        assert_eq!((m.u16(to + 0x20), m.u16(to + 0x22)), (0x108, 0));
        assert_eq!((m.u32(to + 0x30), m.u32(to + 0x34), m.u32(to + 0x60), m.u32(to + 0x68)), (0xdead, 0x10d, 0x118, 0xdead));
        assert_eq!((m.u32(to + 0x6a8), m.u32(to + 0x6b0), m.u32(to + 0x6dc), m.u32(to + 0x6e0)), (0x100 + 0x1aa, 0xdead, 0x100 + 0x1b7, 0xdead));
        assert_eq!((m.u16(to + 0x758), m.u32(to + 0x75c), m.u32(to + 0x760), m.u32(to + 0x764)), (0x100 + 0x1d6, 0x100 + 0x1d7, 0x100 + 0x1d8, 0xdead));
        assert_eq!((m.u16(to + 0xdac), m.u16(to + 0xdae), m.u32(to + 0xda8)), (1, 0, 0xdead));
        assert_eq!((m.u32(to + 0xdb0), m.u32(to + 0xdbc), m.u32(to + 0xdc0)), (0x100 + 0x36c, 0x100 + 0x36f, 0xdead));
        assert_eq!((m.u32(to + 0xdc4), m.u16(to + 0xdc8), m.u16(to + 0xdca)), (0, 0x100 + 0x372, 0));
    }

    #[test]
    fn a_material_transfer_renames_the_element_and_copies_the_shaders() {
        let mut m = image();
        let mut cid = m.read(SI_CID);
        cid.mat = 7;
        m.write(SI_CID, cid);
        let dict = current_dict(&m).addr();
        let (from, to) = (m.calloc(1, 0xdcc), m.calloc(1, 0xdcc));
        m.put_cstr(from, b"shiny");
        for k in 0..4 {
            m.set_u32(from + 0xdb0 + 4 * k, 0x8000 + 0x10 * k);
            m.set_u32(to + 0xdb0 + 4 * k, 0x9000 + 0x10 * k);
        }
        m.set_u32(from + 0xdc4, 0x4242);
        m.set_u16(GLB_DUPLIC_USERDATA_DUP, 0x100);
        let (elem, record) = (m.calloc(1, 0x84), m.calloc(1, 0x34));
        m.set_u32(elem, record);
        m.set_u32(record + 0x10, 0x5555);
        let mut o = Renaming(Vec::new(), elem);
        material_transfer(&mut o, &mut m, from, to, 0x100);
        let calls: Vec<u32> = o.0.iter().map(|c| c.0).collect();
        #[rustfmt::skip]
        assert_eq!(calls, [
            crate::edtools::SI_ELEMDICT_GET_ELEM_FROM_DATA, SI_ELEMDICT_GET_NEW_NAME, SI_ELEMDICT_CHANGE_NAME,
            SI_ED_SOFT_ELEM_CHANGE_NAME, SI_UDF_DISPOSE, SI_UDF_DISPOSE, SI_UDF_DISPOSE, SI_UDF_DISPOSE,
            SI_UDF_COPY, SI_UDF_COPY, SI_UDF_COPY, SI_UDF_COPY, SI_USER_DATA_COPY_FROM_SETUP,
        ]);
        assert_eq!(o.0[0].1[..3], [dict, 7, to]);
        assert_eq!(o.0[1].1, [dict, 7, to]);
        assert_eq!(o.0[2].1, [dict, 7, elem, 0x5555, 0x7300]);
        assert_eq!(o.0[3].1, [elem, 7, 0x7300]);
        assert_eq!((o.0[4].1.clone(), o.0[7].1.clone(), o.0[8].1.clone()), (vec![0x9000], vec![0x9030], vec![0x8000]));
        assert_eq!((m.u32(to + 0xdb0), m.u32(to + 0xdbc)), (0x8001, 0x8031));
        assert_eq!(o.0[12].1, [0x4242, to + 0xdc4]);
    }

    #[test]
    fn a_material_copy_starts_from_the_default_and_heads_the_list() {
        let mut m = image();
        // the graph list of index 1
        let proc = m[SI_PROC_DATA].addr();
        m.set_u32(proc + 4, 1);
        let lists = m.malloc(0x48);
        m.set_u32(0x0867_9498, lists);
        m.set_u32(lists + 0x24 + 0x14, 0x5555);
        for k in 0..0x373 {
            m.set_u32(SI_DEFAULT_MATERIAL + 4 * k, k);
        }
        let mut o = Log(Vec::new(), 0);
        assert_eq!(material_copy(&mut o, &mut m, 0), 0);
        assert!(o.0.is_empty());
        let copy = material_copy(&mut o, &mut m, 0x4444);
        assert_eq!(o.0, [(SI_MATERIAL_TRANSFER, vec![0x4444, copy, 0])]);
        assert_eq!(m.u32(copy + 4), 1);
        assert_eq!(m.u32(copy + 0x6e4), 0x5555);
        assert_eq!(m.u32(lists + 0x24 + 0x14), copy);
    }

    /// Every call logged; `SI_elemdictGetNewName` answers the string at `self.1`,
    /// `SI_udfCopy` its argument + 1.
    struct Naming(Vec<(u32, Vec<u32>)>, u32);
    impl ByAddr for Naming {
        fn call_addr(&mut self, _m: &mut Mem, f: u32, args: &[u32]) -> u32 {
            self.0.push((f, args.to_vec()));
            match f {
                SI_ELEMDICT_GET_NEW_NAME => self.1,
                SI_UDF_COPY => args[0] + 1,
                _ => 0,
            }
        }
    }

    #[test]
    fn a_texture_transfer_copies_the_strings_and_the_fields() {
        let mut m = image();
        crate::globals::store(&mut m);
        let mut cid = m.read(SI_CID);
        cid.t2d = 26;
        m.write(SI_CID, cid);
        let dict = current_dict(&m).addr();
        let (from, to) = (m.calloc(1, 0x1d8), m.calloc(1, 0x1d8));
        for k in 0..0x76 {
            m.set_u32(from + 4 * k, 0x100 + k);
            m.set_u32(to + 4 * k, 0xdead_dead);
        }
        let string = |m: &mut Mem, text: &[u8]| {
            let s = m.malloc(text.len() as u32 + 1);
            m.put_cstr(s, text);
            s
        };
        let picture = string(&mut m, b"brick");
        let (old_picture, old_name, new_name) = (string(&mut m, b"old"), string(&mut m, b"t2d1"), string(&mut m, b"t2d4"));
        m.set_u32(from + 0x14, picture);
        m.set_u32(from + 0x18, 0);
        m.set_u32(from + 0x20, picture);
        m.set_u32(from + 0x10, 0);
        m.set_u32(to + 0x14, old_picture);
        m.set_u32(to + 0x18, old_name);
        m.set_u32(to + 0x20, 0);
        // a signalling NaN goes through the unit; the factors and the method drive the flags
        m.set_u32(from + 0x58, 0x7f80_0001);
        m.set_u32(from + 0x28, 0x0007_0003);
        m.set_u32(from + 0x128, 0.5f32.to_bits());
        m.set_u32(from + 0x198, 0x8000);
        m.set_u32(to + 0x198, 0x9000);
        let mut o = Naming(Vec::new(), new_name);
        txt2d_transfer(&mut o, &mut m, from, to, 1);
        assert_eq!(o.0, [(SI_ELEMDICT_GET_NEW_NAME, vec![dict, 26, S_T2D]), (SI_UDF_DISPOSE, vec![0x9000]), (SI_UDF_COPY, vec![0x8000])]);
        assert_eq!(m.cstr(S_T2D), b"t2d");
        assert!(m.is_freed(old_picture) && !m.is_freed(old_name));
        let (p, n, x) = (m.u32(to + 0x14), m.u32(to + 0x18), m.u32(to + 0x20));
        assert!(p != picture && n != new_name && x != picture);
        assert_eq!((m.cstr(p), m.cstr(n), m.cstr(x), m.u32(to + 0x10)), (b"brick".to_vec(), b"t2d4".to_vec(), b"brick".to_vec(), 0));
        assert_eq!((m.u32(to + 0x58), m.u32(to + 0x28), m.u32(to + 0x128)), (0x7fc0_0001, 0x0007_0003, 0.5f32.to_bits()));
        assert_eq!((m.u16(to + 0x1c8), m.u16(to + 0x1ca), m.u32(to + 0x174), m.u32(to + 0x16c)), (0x172, 0xdead, 0x15d, 0xdead_dead));
        assert_eq!((m.u32(to + 0x198), m.u16(to + 0x19c), m.u16(to + 0x19e), m.u32(to + 0x1d0)), (0x8001, 0x167, 0xdead, 0));
        assert_eq!((m.u16(to + 0x1d4), m.u16(to + 0x1d6), m.u32(to + 0x1bc)), (0x175, 0xdead, 0xdead_dead));
        // the flags SI_updTxt2dEffects derives: ambient in use, a reflection map
        assert_eq!((m.u16(to + 0x112), m.u16(to + 0x124)), (1, 1));
        // a name of its own: the old one freed; the shader by `how`
        m.set_u32(from + 0x18, picture);
        m.set_u16(GLB_DUPLIC_USERDATA_DUP, 1);
        let mut o = Naming(Vec::new(), new_name);
        txt2d_transfer(&mut o, &mut m, from, to, 2);
        assert!(m.is_freed(n) && m.is_freed(p) && m.is_freed(x));
        assert_eq!(o.0, [(SI_USER_DATA_COPY_FROM_SETUP, vec![0x100 + 0x74, to + 0x1d0])]);
        assert_eq!(m.u32(to + 0x198), 0);
        for (how, want) in [(0, 0x8000), (3, 0x4242), (-1, 0x4242)] {
            m.set_u32(to + 0x198, 0x4242);
            txt2d_transfer(&mut o, &mut m, from, to, how);
            assert_eq!(m.u32(to + 0x198), want);
        }
    }

    #[test]
    fn a_texture_copy_is_marked_after_the_transfer() {
        let mut m = image();
        let mut o = Log(Vec::new(), 0);
        let copy = txt2d_copy(&mut o, &mut m, 0x4444);
        assert_eq!(o.0, [(SI_TXT2D_TRANSFER, vec![0x4444, copy, 1])]);
        assert_eq!((m.u16(copy + 0x194), m.u32(copy + 0x1bc), m.u32(copy)), (7, 0xffff_ffff, 0));
    }

    #[test]
    fn a_3d_texture_transfer_leaves_a_made_name_in_the_static_word() {
        let mut m = image();
        crate::globals::store(&mut m);
        let mut cid = m.read(SI_CID);
        cid.t3d = 27;
        m.write(SI_CID, cid);
        let dict = current_dict(&m).addr();
        let (from, to) = (m.calloc(1, 0x158), m.calloc(1, 0x158));
        for k in 0..0x56 {
            m.set_u32(from + 4 * k, 0x100 + k);
            m.set_u32(to + 4 * k, 0xdead_dead);
        }
        let new_name = m.malloc(5);
        m.put_cstr(new_name, b"t3d3");
        m.set_u32(from, 0);
        // a signalling NaN goes through the unit
        m.set_u32(from + 0x84, 0x7f80_0001);
        m.set_u32(from + 0x14c, 0x8000);
        m.set_u32(to + 0x14c, 0x9000);
        let mut o = Naming(Vec::new(), new_name);
        txt3d_transfer(&mut o, &mut m, from, to, 1);
        assert_eq!(
            o.0,
            [(SI_ELEMDICT_GET_NEW_NAME, vec![dict, 27, S_T3D]), (SI_UDF_DISPOSE, vec![0x9000]), (SI_UDF_COPY, vec![0x8000]), (SI_UPD_TXT3D_TURB, vec![to])]
        );
        assert_eq!(m.cstr(S_T3D), b"t3d");
        // the name went to `txt`, not to the texture
        assert_eq!((m.u32(to), m.cstr(m.u32(T3D_DIAL_TXT))), (0xdead_dead, b"t3d3".to_vec()));
        assert_eq!((m.u32(to + 0x0c), m.u32(to + 0x84), m.u16(to + 0x24), m.u16(to + 0x26)), (0x103, 0x7fc0_0001, 0x109, 0xdead));
        // +0x8c and +0xb4 to +0x104 are not copied; the eight words at +0x108 are
        assert_eq!((m.u32(to + 0x8c), m.u32(to + 0xb4), m.u32(to + 0x104), m.u32(to + 0x108), m.u32(to + 0x124)), (0xdead_dead, 0xdead_dead, 0xdead_dead, 0x142, 0x149));
        assert_eq!((m.u32(to + 0x14c), m.u16(to + 0x150), m.u16(to + 0x152), m.u32(to + 0x154)), (0x8001, 0x154, 0xdead, 0));
        // a name of its own is copied, the old one not freed; the shader by `how`
        let (name, old) = (m.malloc(4), m.u32(to));
        m.put_cstr(name, b"tex");
        m.set_u32(from, name);
        m.set_u16(GLB_DUPLIC_USERDATA_DUP, 1);
        let mut o = Naming(Vec::new(), new_name);
        txt3d_transfer(&mut o, &mut m, from, to, 2);
        assert_eq!(o.0, [(SI_UPD_TXT3D_TURB, vec![to]), (SI_USER_DATA_COPY_FROM_SETUP, vec![0x100 + 0x55, to + 0x154])]);
        assert!(m.u32(to) != name && m.cstr(m.u32(to)) == b"tex" && old == 0xdead_dead);
        assert_eq!(m.u32(to + 0x14c), 0);
        for (how, want) in [(0, 0x8000), (3, 0x4242), (-1, 0x4242)] {
            m.set_u32(to + 0x14c, 0x4242);
            txt3d_transfer(&mut o, &mut m, from, to, how);
            assert_eq!(m.u32(to + 0x14c), want);
        }
    }

    #[test]
    fn a_3d_texture_copy_starts_from_the_default() {
        let mut m = image();
        m.set_u32(SI_DEFAULT_TXT3D, 0x1234_5678);
        m.set_u32(SI_DEFAULT_TXT3D + 0x154, 0x9abc_def0);
        let mut o = Log(Vec::new(), 0);
        let copy = txt3d_copy(&mut o, &mut m, 0);
        assert_eq!(o.0, [(SI_TXT3D_TRANSFER, vec![0, copy, 1])]);
        assert_eq!((m.u32(copy), m.u32(copy + 0x154)), (0x1234_5678, 0x9abc_def0));
    }

    #[test]
    fn a_3d_texture_gets_a_copy_of_the_shader_or_hands_its_own() {
        let mut m = image();
        let mut cid = m.read(SI_DBCID);
        cid.t3d = 27;
        m.write(SI_DBCID, cid);
        let t = m.malloc(0x158);
        m.set_u32(t + 0x14c, 0);
        let mut o = Log(Vec::new(), 0);
        copy_t3d_shader(&mut o, &mut m, t, 0);
        copy_t3d_shader(&mut o, &mut m, 0, 0x6000);
        assert!(o.0.is_empty());
        copy_t3d_shader(&mut o, &mut m, t, 0x6000);
        assert_eq!(o.0, [(SI_UDF_COPY, vec![0x6000]), (SI_ED_ADD_UDF, vec![t, 27, 0x7100])]);
        assert_eq!(m.u32(t + 0x14c), 0x7100);
        let mut o = Log(Vec::new(), 0);
        copy_t3d_shader(&mut o, &mut m, t, 0x6000);
        assert_eq!(o.0, [(SI_ED_ADD_UDF, vec![t, 27, 0x7100])]);
    }

    #[test]
    fn a_texture_keeps_its_own_shader() {
        let mut m = image();
        let mut cid = m.read(SI_DBCID);
        cid.t2d = 26;
        m.write(SI_DBCID, cid);
        let t = m.malloc(0x1d8);
        m.set_u32(t + 0x198, 0);
        let mut o = Log(Vec::new(), 0);
        copy_t2d_shader(&mut o, &mut m, t, 0);
        copy_t2d_shader(&mut o, &mut m, 0, 0x6000);
        assert!(o.0.is_empty());
        copy_t2d_shader(&mut o, &mut m, t, 0x6000);
        assert_eq!(o.0, [(SI_UDF_COPY, vec![0x6000]), (SI_ED_ADD_UDF, vec![t, 26, 0x7100])]);
        assert_eq!(m.u32(t + 0x198), 0x7100);
        let mut o = Log(Vec::new(), 0);
        copy_t2d_shader(&mut o, &mut m, t, 0x6000);
        assert_eq!(o.0, [(SI_ED_ADD_UDF, vec![t, 26, 0x7100])]);
    }

    #[test]
    fn a_shader_with_an_element_is_added_only_when_forced() {
        let mut m = image();
        let dict = current_dict(&m).addr();
        for (index, force, adds) in [(3, 0, false), (3, 1, true), (-1i32 as u32, 0, true), (0, -0x8000, true)] {
            let mut o = Log(Vec::new(), index);
            db_convert_udf2db(&mut o, &mut m, 0x6000, force, 30, 0x4444);
            assert_eq!(o.0[0], (SI_ELEMDICT_FIND_ELEM_BY_DATA, vec![dict, 30, 0x6000]));
            assert_eq!(o.0.get(1).cloned(), adds.then(|| (SI_ED_ADD_UDF, vec![0x4444, 30, 0x6000])));
        }
        let mut o = Log(Vec::new(), 0);
        db_convert_udf2db(&mut o, &mut m, 0, 1, 30, 0x4444);
        assert!(o.0.is_empty());
    }

    /// The rest of the program for the conversion: every call logged. The objects the graph
    /// list was handed get an element (a block of 0x84 bytes) that `SI_elemdictGetElemFromData`
    /// answers; `SI_elemdictGetElem` answers `root`; `SI_txt2dCopy` and `SI_materialCopy`
    /// answer new blocks; `SI_elemdictFindElemByData` answers −1.
    struct Converting {
        calls: Vec<(u32, Vec<u32>)>,
        elems: Vec<(u32, u32)>,
        root: u32,
    }
    impl ByAddr for Converting {
        fn call_addr(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
            self.calls.push((f, args.to_vec()));
            match f {
                crate::edtools::SI_ELEMDICT_GET_ELEM_FROM_DATA => self.elems.iter().find(|e| e.0 == args[2]).map_or(0, |e| e.1),
                SI_ELEMDICT_GET_ELEM => self.root,
                SI_ELEMDICT_FIND_ELEM_BY_DATA => 0xffff_ffff,
                SI_GRAFLIST_ADD_DATA => {
                    let e = m.calloc(1, 0x84);
                    self.elems.push((args[1], e));
                    0
                }
                SI_TXT2D_COPY => {
                    let c = m.calloc(1, 0x1d8);
                    m.set_u16(c + 0x2a, 4);
                    c
                }
                SI_MATERIAL_COPY => m.calloc(1, 0xdcc),
                _ => 0,
            }
        }
    }

    fn conv_image() -> Mem {
        let mut m = image();
        let mut cid = m.read(SI_CID);
        (cid.mat, cid.model, cid.t2d, cid.t3d) = (2, 3, 4, 5);
        m.write(SI_CID, cid);
        let proc = m[SI_PROC_DATA].addr();
        m.set_u32(proc + 4, 0);
        let lists = m.calloc(1, 0x24);
        m.set_u32(0x0867_9498, lists);
        m
    }

    #[test]
    fn the_load_s_conversion_gives_a_node_s_material_and_texture_elements_and_lays_them_out() {
        let mut m = conv_image();
        m.set_u32(SI_GLB_SCHEM.addr(), 2);
        let (node, mat, t, array) = (m.calloc(1, 0x360), m.calloc(1, 0xdcc), m.calloc(1, 0x1d8), m.calloc(1, 4));
        m.set_u32(array, mat);
        m.set_u32(node + 0x20c, array);
        m.set_u16(node + 0x206, 1);
        m.set_u32(mat + 0x6ec, t);
        // the node's element is its hierarchy's root, of box (1, 2) to (3, 4)
        let root = m.calloc(1, 0x84);
        for (k, v) in [1.0f32, 2.0, 3.0, 4.0].into_iter().enumerate() {
            m.set_f32(root + 0x2c + 4 * k as u32, v);
        }
        let mut o = Converting { calls: Vec::new(), elems: Vec::new(), root };
        db_convert_model_mat_txt2db(&mut o, &mut m, node, 0, 1, 0);
        let added: Vec<Vec<u32>> = o.calls.iter().filter(|c| c.0 == SI_GRAFLIST_ADD_DATA).map(|c| c.1.clone()).collect();
        let list = m.u32(0x0867_9498);
        assert_eq!(added, [vec![list, mat, 0xe, 0], vec![list, t, 0xf, 0]]);
        assert!(!o.calls.iter().any(|c| c.0 == SI_SCHEM_COMPUTE_ELEM_BBOX));
        // right of the hierarchy: the material by 2 and 0.5, the texture by 5
        let (em, et) = (o.elems[0].1, o.elems[1].1);
        assert_eq!((m.f32(em + 0x0c), m.f32(em + 0x10)), (5.5, 4.0));
        assert_eq!((m.f32(et + 0x0c), m.f32(et + 0x10)), (8.0, 4.0));
        // below it, with a box of zeros measured first (the stand-in leaves it as it is)
        for k in 0..4 {
            m.set_u32(root + 0x2c + 4 * k, if k == 1 { 0x8000_0000 } else { 0 });
        }
        m.set_u32(SI_GLB_SCHEM.addr(), 0);
        let mut o = Converting { calls: Vec::new(), elems: Vec::new(), root };
        db_convert_model_mat_txt2db(&mut o, &mut m, node, 1, 0, 0);
        let bbox: Vec<_> = o.calls.iter().filter(|c| c.0 == SI_SCHEM_COMPUTE_ELEM_BBOX).map(|c| c.1[..3].to_vec()).collect();
        assert_eq!(bbox, [vec![current_dict(&m).addr(), root, 3]]);
        let (em, et) = (o.elems[0].1, o.elems[1].1);
        assert_eq!((m.f32(em + 0x0c), m.f32(em + 0x10)), (0.0, -2.0));
        assert_eq!((m.f32(et + 0x0c), m.f32(et + 0x10)), (0.0, -4.0));
    }

    #[test]
    fn a_later_scene_s_node_gets_the_made_material_and_the_textures_handed_on() {
        let mut m = conv_image();
        m[SI_ED_SUMATRA_TO_3X] = 1;
        let (node, own3d, son, son_mat, son_array) = (m.calloc(1, 0x360), m.calloc(1, 0x158), m.calloc(1, 0x360), m.calloc(1, 0xdcc), m.calloc(1, 4));
        m.set_u32(node + 0x30, 4);
        m.set_u32(node + 0x224, own3d);
        m.set_u32(node + 0x2c4, son);
        // a leaf of type 0 with a material loses it
        m.set_u32(son_array, son_mat);
        m.set_u32(son + 0x20c, son_array);
        m.set_u16(son + 0x206, 1);
        let (t1, t2) = (m.calloc(1, 0x1d8), m.calloc(1, 0x1d8));
        m.set_u32(t1 + 0x16c, t2);
        m.set_u32(GLB_2D_TXT_TO_PROPAGATE, t1);
        let mut o = Converting { calls: Vec::new(), elems: Vec::new(), root: 0 };
        db_convert_model_mat_txt2db(&mut o, &mut m, node, 0, 1, 0);
        let made = m.u32(SUMATRA_MATERIAL);
        assert!(made != 0 && m[SI_ADD_IN_DICTIONARY] == 0);
        assert!(o.calls.contains(&(SI_ED_ADD_MATERIAL, vec![0, 1, made])));
        assert_eq!((m.i16(node + 0x206), m.u32(m.u32(node + 0x20c))), (1, made));
        let (c1, c2) = (m.u32(node + 0x21c), m.u32(m.u32(node + 0x21c) + 0x16c));
        assert_eq!((m.i16(node + 0x216), m.u32(node + 0x218), m.u32(node + 0x224)), (2, c1, own3d));
        assert!(c2 != 0 && m.u16(c1 + 0x2a) == 6 && m.u16(c2 + 0x2a) == 6);
        assert_eq!(o.calls.iter().filter(|c| c.0 == SI_TEXTURE2D_LOAD_FCURVE_ALL).map(|c| c.1.clone()).collect::<Vec<_>>(), [vec![c1, t1], vec![c2, t2]]);
        // handed on to the son, who lost his own
        assert_eq!((m.u32(GLB_2D_TXT_TO_PROPAGATE), m.u32(GLB_3D_TXT_TO_PROPAGATE)), (c1, own3d));
        assert_eq!((m.u32(son + 0x20c), m.i16(son + 0x206)), (0, 0));
        assert!(o.calls.iter().any(|c| c.0 == crate::edtools::SI_ELEMDICT_REMOVE_RELATION_PAIR && c.1[4] == 300));
    }
}
