// SPDX-License-Identifier: MIT
//! The shader-reference copies of `shd_fct.c` (finding 0023, section 36) on the memory image:
//! a material keeps up to four user-defined-function shaders (+0xdb0 material, +0xdb4 volume,
//! +0xdb8 shadow, +0xdbc photon), a light two (+0x178 light, +0x17c photon). These routines copy,
//! connect and dispose them. The user-defined-function calls (`SI_udfCopy`, `SI_udfDispose`) and
//! the element dictionary's (`SI_edAddUDF`, `SI_edDisposeUDF`) are hooks. The list dialogs of the
//! file stay hooks of the user interface.

use si_mem::Mem;

si_mem::outside_calls! {
    /// What the copies call.
    pub trait ShdCalls {
        /// `SI_udfCopy (0x0854c70c)`: a copy of the shader, or null.
        fn udf_copy(udf: u32) -> u32 = "SI_udfCopy", 0x0854c70c;
        /// `SI_udfDispose (0x0854c3c0)`.
        fn udf_dispose(udf: u32) = "SI_udfDispose", 0x0854c3c0;
        /// `SI_edAddUDF (0x08103b5c)`: `(owner, chapter, shader)`.
        fn ed_add_udf(owner: u32, chapter: u32, udf: u32) = "SI_edAddUDF", 0x08103b5c;
        /// `SI_edDisposeUDF (0x08103ca8)`: `(shader, its word at +4)`.
        fn ed_dispose_udf(udf: u32, cid: u32) = "SI_edDisposeUDF", 0x08103ca8;
    }
}

/// `SI_cid.MATERIAL_SHADERS`, `VOLUME_SHADERS`, `SHADOW_SHADERS`, `MATERIAL_PHOTON_SHADERS`.
const MATERIAL_CIDS: [u32; 4] = [0x0878_1690, 0x0878_169c, 0x0878_16a0, 0x0878_16e8];
/// The material's four slots, in the same order.
const MATERIAL_SLOTS: [u32; 4] = [0xdb0, 0xdb4, 0xdb8, 0xdbc];
/// `SI_cid.LIGHT_SHADERS`, `LIGHT_PHOTON_SHADERS`; the light's slots.
const LIGHT_CIDS: [u32; 2] = [0x0878_16a4, 0x0878_16ec];
const LIGHT_SLOTS: [u32; 2] = [0x178, 0x17c];
/// `SI_cid.MATERIALS` and the two chapter ids the light copy and the connect pass to `SI_edAddUDF`.
const CID_MATERIALS: u32 = 0x0878_167c;
const CHAPTER_LIGHT_COPY: u32 = 0x0878_1718;
const CHAPTER_CONNECT: u32 = 0x0878_171c;

fn copy_shader(m: &mut Mem, o: &mut dyn si_mem::Outside, dst: u32, src: u32, cids: &[u32], slots: &[u32], chapter: u32) {
    use ShdCalls as _;
    if dst == 0 || src == 0 {
        return;
    }
    let cid = m.u32(src + 4);
    let Some(i) = cids.iter().position(|&a| m.u32(a) == cid) else { return };
    let slot = dst + slots[i];
    m.set_u32(slot, 0);
    let copy = o.udf_copy(m, src);
    if copy != 0 {
        let chapter = m.u32(chapter);
        o.ed_add_udf(m, dst, chapter, copy);
        m.set_u32(slot, copy);
    }
}

/// Copies one shader reference into a material [soft 0x082a677c copyMaterialShader]:
/// `copyMaterialShader(material, shader)`; the slot is chosen by the shader's chapter id (+0x4);
/// no slot, nothing; else the slot is cleared, the shader copied and, if that gave one, attached
/// with `SI_edAddUDF(material, SI_cid.MATERIALS, copy)` and stored.
pub fn copy_material_shader(m: &mut Mem, o: &mut dyn si_mem::Outside, material: u32, shader: u32) {
    copy_shader(m, o, material, shader, &MATERIAL_CIDS, &MATERIAL_SLOTS, CID_MATERIALS);
}

/// Copies the four shader references of a material [soft 0x082a6808 SI_copyMaterialShaders]:
/// `SI_copyMaterialShaders(dst, src)`.
pub fn copy_material_shaders(m: &mut Mem, o: &mut dyn si_mem::Outside, dst: u32, src: u32) {
    for s in MATERIAL_SLOTS {
        let shader = m.u32(src + s);
        copy_material_shader(m, o, dst, shader);
    }
}

/// Copies one shader reference into a light [soft 0x082a6930 copyLightShader]: as
/// [`copy_material_shader`] with the light's two slots and the chapter word at 0x08781718.
pub fn copy_light_shader(m: &mut Mem, o: &mut dyn si_mem::Outside, light: u32, shader: u32) {
    copy_shader(m, o, light, shader, &LIGHT_CIDS, &LIGHT_SLOTS, CHAPTER_LIGHT_COPY);
}

/// Copies a light's two shader references [soft 0x082a69a4 SI_copyLightShader]:
/// `SI_copyLightShader(dst, src)`.
pub fn copy_light_shaders(m: &mut Mem, o: &mut dyn si_mem::Outside, dst: u32, src: u32) {
    for s in LIGHT_SLOTS {
        let shader = m.u32(src + s);
        copy_light_shader(m, o, dst, shader);
    }
}

/// Disposes a material's shader references [soft 0x082a69dc SI_deleteMaterialShaders]:
/// `SI_deleteMaterialShaders(material)`; for each set slot `SI_edDisposeUDF(shader, shader[1])`,
/// `SI_udfDispose(shader)` and the slot cleared.
pub fn delete_material_shaders(m: &mut Mem, o: &mut dyn si_mem::Outside, material: u32) {
    use ShdCalls as _;
    if material == 0 {
        return;
    }
    for s in MATERIAL_SLOTS {
        let shader = m.u32(material + s);
        if shader != 0 {
            let cid = m.u32(shader + 4);
            o.ed_dispose_udf(m, shader, cid);
            let shader = m.u32(material + s);
            o.udf_dispose(m, shader);
            m.set_u32(material + s, 0);
        }
    }
}

/// Registers a material's shaders with the dictionary [soft 0x082a6ac0 SI_connectMaterialShaders]:
/// `SI_connectMaterialShaders(material)`; `SI_edAddUDF(material, *0x0878171c, shader)` for each set slot.
pub fn connect_material_shaders(m: &mut Mem, o: &mut dyn si_mem::Outside, material: u32) {
    use ShdCalls as _;
    if material == 0 {
        return;
    }
    for s in MATERIAL_SLOTS {
        let shader = m.u32(material + s);
        if shader != 0 {
            let chapter = m.u32(CHAPTER_CONNECT);
            o.ed_add_udf(m, material, chapter, shader);
        }
    }
}
