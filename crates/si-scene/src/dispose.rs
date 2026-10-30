// SPDX-License-Identifier: MIT
//! The three disposals `SI_scnRelationsFromElemDict` makes when a model node or a material
//! gives up the materials and textures its own file gave it (finding 0018, section 17):
//! `SI_materialDispose` (`mat_dial.c`), `SI_txt2dDispose` (`t2d_dial.c`), `SI_txt3dFree`
//! (`t3d_dial.c`), on the memory image. Every callee outside these three files goes
//! through [`ByAddr`].
//!
//! **Compared with the emulated original** (`re/tools/si-scene/emu_gen_scnrel.py`,
//! `scene-emucheck re/out/si-scene/vec_scnrel`), each callee a recorded stand-in.

use crate::edtools::ByAddr;
use si_mem::Mem;

/// `SI_removeTriangle (0x085efc38)`, `softtriangle.c`: (model, 1).
pub const SI_REMOVE_TRIANGLE: u32 = 0x085e_fc38;
/// `SI_findGlData (0x0832ee68)`, `graflist.c`: (list, object, kind).
pub const SI_FIND_GL_DATA: u32 = 0x0832_ee68;
/// `SI_edDisposeTexture2D (0x0810253c)`, `edtools.c`: (texture).
pub const SI_ED_DISPOSE_TEXTURE2D: u32 = 0x0810_253c;
/// `SI_graflistRemoveGrafnode (0x0832c9ac)`, `graflist.c`: (list, graf node).
pub const SI_GRAFLIST_REMOVE_GRAFNODE: u32 = 0x0832_c9ac;
/// `SI_texture3DStillUsed (0x082b56c0)`, `t3d_dial.c`: (texture); answers 16 bits.
pub const SI_TEXTURE3D_STILL_USED: u32 = 0x082b_56c0;
/// `SI_edDisposeTexture3D (0x081028d0)`, `edtools.c`: (texture).
pub const SI_ED_DISPOSE_TEXTURE3D: u32 = 0x0810_28d0;
/// `SI_modelResetMaterial (0x08221468)`, `fcvreset.c`: (model, material).
pub const SI_MODEL_RESET_MATERIAL: u32 = 0x0822_1468;
/// `SI_modelResetTexture (0x082214b4)`: (model, texture).
pub const SI_MODEL_RESET_TEXTURE: u32 = 0x0822_14b4;
/// `SI_modelResetTexture3D (0x08221500)`: (model, texture).
pub const SI_MODEL_RESET_TEXTURE3D: u32 = 0x0822_1500;
/// `SI_udfDispose (0x0854c3c0)`, `udf.c`: (shader).
pub const SI_UDF_DISPOSE: u32 = 0x0854_c3c0;
/// `SI_userDataDispose (0x084619e4)`, `model.c`: (&chain).
pub const SI_USER_DATA_DISPOSE: u32 = 0x0846_19e4;
/// `SI_global2dMapRemove (0x082ae294)`, `t2d_dial.c`: (&map).
pub const SI_GLOBAL2D_MAP_REMOVE: u32 = 0x082a_e294;
/// `si_cropViewFree (0x082c0fe0)`, `crop_view.c`: (view).
pub const SI_CROP_VIEW_FREE: u32 = 0x082c_0fe0;

/// `SI_procData` and `SI_grafLists`: the scene's graph list is `SI_grafLists` + 0x24 ×
/// the process record's second word.
const PROC_DATA: u32 = 0x0867_9314;
const GRAF_LISTS: u32 = 0x0867_9498;

pub(crate) fn graf_list(m: &Mem) -> u32 {
    let index = m.u32(m.u32(PROC_DATA).wrapping_add(4));
    m.u32(GRAF_LISTS).wrapping_add(index.wrapping_mul(0x24))
}

/// Gives up a material [soft 0x08284900 SI_materialDispose]: `SI_materialDispose(model,
/// &material)`.
///
/// `SI_removeTriangle(model, 1)` first, whatever the material. For a material that is not
/// null: it is taken out of the graph list's material list (+0x14 of the list, linked
/// through +0x6e4; a material behind the head is found by its predecessor); each of its
/// 2D textures, taken off its list (+0x6ec, through +0x16c) one by one, is looked up in
/// the graph list (`SI_findGlData(list, texture, 0xf)`), handed to
/// `SI_edDisposeTexture2D` and its graph node, if it had one, removed
/// (`SI_graflistRemoveGrafnode`); its 3D texture (+0x6fc), unless
/// `SI_texture3DStillUsed`, the same with kind 0x10 and `SI_edDisposeTexture3D`.
/// `SI_modelResetMaterial(model, material)`; the four shaders (+0xdb0 … +0xdbc)
/// `SI_udfDispose`; user data (+0xdc4) `SI_userDataDispose`. Eleven fields are cleared
/// (the shaders, the texture list and its tail and count, the 3D texture, +0x6f4, +0x6f8,
/// the list link) and so is the caller's pointer. **The record itself is not freed.**
pub fn material_dispose<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, slot: u32) {
    let mat = m.u32(slot);
    o.call_addr(m, SI_REMOVE_TRIANGLE, &[model, 1]);
    if mat == 0 {
        return;
    }
    let head = graf_list(m).wrapping_add(0x14);
    let first = m.u32(head);
    if first == mat {
        let next = m.u32(mat + 0x6e4);
        m.set_u32(head, next);
    } else {
        let mut p = first;
        while p != 0 {
            if m.u32(p + 0x6e4) == mat {
                let next = m.u32(m.u32(p + 0x6e4) + 0x6e4);
                m.set_u32(p + 0x6e4, next);
                break;
            }
            p = m.u32(p + 0x6e4);
        }
    }
    while m.u32(mat + 0x6ec) != 0 {
        let tex = m.u32(mat + 0x6ec);
        let next = m.u32(tex + 0x16c);
        m.set_u32(mat + 0x6ec, next);
        let list = graf_list(m);
        let node = o.call_addr(m, SI_FIND_GL_DATA, &[list, tex, 0xf]);
        o.call_addr(m, SI_ED_DISPOSE_TEXTURE2D, &[tex]);
        if node != 0 {
            let list = graf_list(m);
            o.call_addr(m, SI_GRAFLIST_REMOVE_GRAFNODE, &[list, node]);
        }
    }
    let t3 = m.u32(mat + 0x6fc);
    if t3 != 0 && o.call_addr(m, SI_TEXTURE3D_STILL_USED, &[t3]) as u16 == 0 {
        let list = graf_list(m);
        let t3 = m.u32(mat + 0x6fc);
        let node = o.call_addr(m, SI_FIND_GL_DATA, &[list, t3, 0x10]);
        let t3 = m.u32(mat + 0x6fc);
        o.call_addr(m, SI_ED_DISPOSE_TEXTURE3D, &[t3]);
        if node != 0 {
            let list = graf_list(m);
            o.call_addr(m, SI_GRAFLIST_REMOVE_GRAFNODE, &[list, node]);
        }
    }
    o.call_addr(m, SI_MODEL_RESET_MATERIAL, &[model, mat]);
    for at in [0xdb0, 0xdb4, 0xdb8, 0xdbc] {
        let shader = m.u32(mat + at);
        o.call_addr(m, SI_UDF_DISPOSE, &[shader]);
    }
    if m.u32(mat + 0xdc4) != 0 {
        o.call_addr(m, SI_USER_DATA_DISPOSE, &[mat + 0xdc4]);
    }
    for at in [0xdb0, 0xdb4, 0xdb8, 0xdbc, 0x6ec, 0x6f4] {
        m.set_u32(mat + at, 0);
    }
    m.set_u16(mat + 0x6e8, 0);
    for at in [0x6fc, 0x6f8, 0x6e4, 0x6f0] {
        m.set_u32(mat + at, 0);
    }
    m.set_u32(slot, 0);
}

/// Gives up a 2D texture [soft 0x082ae014 SI_txt2dDispose]: `SI_txt2dDispose(model,
/// &texture)`. For a texture that is not null: the blocks at +0x14, +0x18, +0x20 freed
/// and cleared; `SI_global2dMapRemove(&texture->map)` (+0x24); the shader (+0x198)
/// `SI_udfDispose` and cleared; `SI_modelResetTexture(model, texture)`; the crop view
/// (+0x170) `si_cropViewFree`; the block at +0x10 freed and cleared; user data (+0x1d0)
/// `SI_userDataDispose`; the record freed and the caller's pointer cleared.
pub fn txt2d_dispose<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, slot: u32) {
    let t = m.u32(slot);
    if t == 0 {
        return;
    }
    for at in [0x14, 0x18, 0x20] {
        let b = m.u32(t + at);
        if b != 0 {
            m.free(b);
            m.set_u32(t + at, 0);
        }
    }
    o.call_addr(m, SI_GLOBAL2D_MAP_REMOVE, &[t + 0x24]);
    let shader = m.u32(t + 0x198);
    o.call_addr(m, SI_UDF_DISPOSE, &[shader]);
    m.set_u32(t + 0x198, 0);
    o.call_addr(m, SI_MODEL_RESET_TEXTURE, &[model, t]);
    let view = m.u32(t + 0x170);
    if view != 0 {
        o.call_addr(m, SI_CROP_VIEW_FREE, &[view]);
    }
    let b = m.u32(t + 0x10);
    if b != 0 {
        m.free(b);
        m.set_u32(t + 0x10, 0);
    }
    if m.u32(t + 0x1d0) != 0 {
        o.call_addr(m, SI_USER_DATA_DISPOSE, &[t + 0x1d0]);
    }
    m.free(t);
    m.set_u32(slot, 0);
}

/// Gives up a 3D texture [soft 0x082b4054 SI_txt3dFree]: `SI_txt3dFree(model, texture)`:
/// the blocks at +0x00, +0x04 and the turbulence object (+0x8c) freed and cleared; the
/// shader (+0x14c) `SI_udfDispose` and cleared; `SI_modelResetTexture3D(model,
/// texture)`; user data (+0x154) `SI_userDataDispose`; the record freed. (The texture is
/// read through before the test for null that guards the last `free`.)
pub fn txt3d_free<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, t: u32) {
    for at in [0x00, 0x04, 0x8c] {
        let b = m.u32(t.wrapping_add(at));
        if b != 0 {
            m.free(b);
            m.set_u32(t.wrapping_add(at), 0);
        }
    }
    let shader = m.u32(t.wrapping_add(0x14c));
    o.call_addr(m, SI_UDF_DISPOSE, &[shader]);
    m.set_u32(t.wrapping_add(0x14c), 0);
    o.call_addr(m, SI_MODEL_RESET_TEXTURE3D, &[model, t]);
    if m.u32(t.wrapping_add(0x154)) != 0 {
        o.call_addr(m, SI_USER_DATA_DISPOSE, &[t.wrapping_add(0x154)]);
    }
    if t != 0 {
        m.free(t);
    }
}
