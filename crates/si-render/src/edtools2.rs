// SPDX-License-Identifier: MIT
//! The second half of `edtools.c` (0x08101421 and up) that the renderer's load and unload
//! paths reach, on the memory image (finding 0012, "edtools2, 2026-10-29"): the dynamics
//! relations, the shader removal group, the texture, material and wave removers, the
//! element adders for waves and clusters, and the model-type test.
//!
//! The element dictionary's own routines (`SI_elemdict*`), the delete flag
//! (`SI_edElementSetDeleteFlag`, the first half of the file), the user-defined-function
//! disposer and the graph-list calls are the rest of the program and are called through
//! [`si_mem::Outside`] by address. The current dictionary is
//! `SI_elemDicts + 0x70 * *SI_procData` (7 * 16 bytes per scene); `SI_RD` (0x087815f0) is
//! the relation dictionary passed by address.

use si_mem::{Mem, Outside};

si_mem::outside_calls! {
    /// What the second half of `edtools.c` calls.
    pub trait Ed2Calls {
        /// `SI_edGetElemFromModel (0x080fcf20)`: `(model, &index)`; the element.
        fn ed_get_elem_from_model(model: u32, index: u32) -> u32 = "SI_edGetElemFromModel", 0x080fcf20;
        /// `SI_edGetElemFromMaterial (0x080fd288)`: `(material, &index)`.
        fn ed_get_elem_from_material(material: u32, index: u32) -> u32 = "SI_edGetElemFromMaterial", 0x080fd288;
        /// `SI_edGetElemFromTexture2D (0x080fd3e0)`: `(texture, &index)`.
        fn ed_get_elem_from_texture2d(texture: u32, index: u32) -> u32 = "SI_edGetElemFromTexture2D", 0x080fd3e0;
        /// `SI_edGetElemFromTexture3D (0x080fd414)`: `(texture, &index)`.
        fn ed_get_elem_from_texture3d(texture: u32, index: u32) -> u32 = "SI_edGetElemFromTexture3D", 0x080fd414;
        /// `SI_edGetElemFromWave (0x080fd448)`: `(wave, &index)`.
        fn ed_get_elem_from_wave(wave: u32, index: u32) -> u32 = "SI_edGetElemFromWave", 0x080fd448;
        /// `SI_edElementSetDeleteFlag (0x080fd910)`: `(element, chapter, number, 1, 0, 0, pass)`.
        fn ed_element_set_delete_flag(elem: u32, cid: u32, number: u32, flag: u32, a: u32, b: u32, pass: u32) = "SI_edElementSetDeleteFlag", 0x080fd910;
        /// `SI_edDisposeUDF (0x08103ca8)`: `(shader, chapter)`.
        fn ed_dispose_udf(udf: u32, cid: u32) = "SI_edDisposeUDF", 0x08103ca8;
        /// `SI_edFcvDispose (0x080ee2f0)`.
        fn ed_fcv_dispose(owner: u32, cid: u32, a: u32, id0: u32, id1: u32) = "SI_edFcvDispose", 0x080ee2f0;
        /// `SI_edModelRelaxMaterial (0x08100cd8)`.
        fn ed_model_relax_material(model: u32, material: u32) = "SI_edModelRelaxMaterial", 0x08100cd8;
        /// `SI_edModelRelaxGTexture3D (0x08100ef0)`.
        fn ed_model_relax_gtexture3d(model: u32, texture: u32) = "SI_edModelRelaxGTexture3D", 0x08100ef0;
        /// `SI_edModelRelaxLTexture3D (0x08101098)`.
        fn ed_model_relax_ltexture3d(material: u32, texture: u32) = "SI_edModelRelaxLTexture3D", 0x08101098;
        /// `SI_edModelRelaxLTexture2D (0x0810137c)`.
        fn ed_model_relax_ltexture2d(material: u32, texture: u32) = "SI_edModelRelaxLTexture2D", 0x0810137c;
        /// `SI_dbListNodeAllocate (0x0812250c)`.
        fn db_list_node_allocate() -> u32 = "SI_dbListNodeAllocate", 0x0812250c;
        /// `SI_dbFormatFullName (0x081223dc)`.
        fn db_format_full_name(node: u32) -> u32 = "SI_dbFormatFullName", 0x081223dc;
        /// `SI_elemdictAddElem (0x08132a5c)`: `(dict, chapter, node, data, 0, 0, 0)`; the number.
        fn elemdict_add_elem(dict: u32, cid: u32, node: u32, data: u32, a: u32, b: u32, c: u32) -> u32 = "SI_elemdictAddElem", 0x08132a5c;
        /// `SI_elemdictGetElem (0x08132b50)`.
        fn elemdict_get_elem(dict: u32, cid: u32, number: u32) -> u32 = "SI_elemdictGetElem", 0x08132b50;
        /// `SI_elemdictAddRelation (0x08132f78)`: `(dict, &SI_RD, from, to, id, 1, 0)`.
        fn elemdict_add_relation(dict: u32, rd: u32, from: u32, to: u32, id: u32, side: u32, a: u32) = "SI_elemdictAddRelation", 0x08132f78;
        /// `SI_elemdictRemoveRelationPair (0x0813362c)`.
        fn elemdict_remove_relation_pair(dict: u32, rd: u32, from: u32, to: u32, id: u32, side: u32, a: u32) = "SI_elemdictRemoveRelationPair", 0x0813362c;
        /// `SI_elemdictRemoveRelationId (0x08133998)`: `(dict, &SI_RD, element, id)`.
        fn elemdict_remove_relation_id(dict: u32, rd: u32, elem: u32, id: u32) = "SI_elemdictRemoveRelationId", 0x08133998;
        /// `SI_dbSortTblSetDirty (0x0813425c)`.
        fn db_sort_tbl_set_dirty(list: u32) = "SI_dbSortTblSetDirty", 0x0813425c;
        /// `SI_elemdictFindElemByData (0x08134364)`: the element's number, below 0 when absent.
        fn elemdict_find_elem_by_data(dict: u32, cid: u32, data: u32) -> u32 = "SI_elemdictFindElemByData", 0x08134364;
        /// `SI_elemdictSetDeleteElem (0x08135ad4)`.
        fn elemdict_set_delete_elem(dict: u32, cid: u32, elem: u32, number: u32, a: u32, b: u32, c: u32) = "SI_elemdictSetDeleteElem", 0x08135ad4;
        /// `SI_udfDispose (0x0854c3c0)`.
        fn udf_dispose(udf: u32) = "SI_udfDispose", 0x0854c3c0;
        /// `SI_grafnodeCutConnections (0x0832c92c)`.
        fn grafnode_cut_connections(node: u32) = "SI_grafnodeCutConnections", 0x0832c92c;
        /// `SI_graflistRemoveGrafnode (0x0832c9ac)`: `(list, node)`.
        fn graflist_remove_grafnode(list: u32, node: u32) = "SI_graflistRemoveGrafnode", 0x0832c9ac;
        /// `SI_global2dMapRemove (0x082ae294)`.
        fn global2d_map_remove(map: u32) = "SI_global2dMapRemove", 0x082ae294;
        /// `SI_materialStillUsed (0x08285db0)`.
        fn material_still_used(material: u32) -> i16 = "SI_materialStillUsed", 0x08285db0;
        /// `SI_texture3DStillUsed (0x082b56c0)`.
        fn texture3d_still_used(texture: u32) -> i16 = "SI_texture3DStillUsed", 0x082b56c0;
        /// `SI_elemdictFindRelatedElem (0x0813508c)`: `(dict, element, 0, 0, relation id, chapter, &a, &b)`; the related element or null.
        fn elemdict_find_related_elem(dict: u32, elem: u32, a: u32, b: u32, id: u32, cid: u32, out_a: u32, out_b: u32) -> u32 = "SI_elemdictFindRelatedElem", 0x0813508c;
        /// `SI_elemdictFindNextRelatedElem (0x08135154)`: the same arguments.
        fn elemdict_find_next_related_elem(dict: u32, elem: u32, a: u32, b: u32, id: u32, cid: u32, out_a: u32, out_b: u32) -> u32 = "SI_elemdictFindNextRelatedElem", 0x08135154;
        /// `SI_materialDisassociate (0x08285b64)`: `(model, material, 0, 1)`.
        fn material_disassociate(model: u32, material: u32, a: u32, b: u32) = "SI_materialDisassociate", 0x08285b64;
        /// `SI_materialDispose (0x08284900)`: `(model, &material)`.
        fn material_dispose(model: u32, material_ptr: u32) = "SI_materialDispose", 0x08284900;
        /// `SI_texture2Dgdisassociate (0x082b0fe8)`: `(model, texture, 0)`.
        fn texture2d_gdisassociate(model: u32, texture: u32, a: u32) = "SI_texture2Dgdisassociate", 0x082b0fe8;
        /// `SI_texture2Dldisassociate (0x082b10d8)`: `(material, texture, 0)`.
        fn texture2d_ldisassociate(material: u32, texture: u32, a: u32) = "SI_texture2Dldisassociate", 0x082b10d8;
        /// `SI_texture3Dgdisassociate (0x082b5658)`: `(model, texture, 0)`.
        fn texture3d_gdisassociate(model: u32, texture: u32, a: u32) = "SI_texture3Dgdisassociate", 0x082b5658;
        /// `SI_texture3Dldisassociate (0x082b5578)`: `(material, texture, 0)`.
        fn texture3d_ldisassociate(material: u32, texture: u32, a: u32) = "SI_texture3Dldisassociate", 0x082b5578;
        /// `SI_txt2dDispose (0x082ae014)`: `(model, &texture)`.
        fn txt2d_dispose(model: u32, texture_ptr: u32) = "SI_txt2dDispose", 0x082ae014;
        /// `SI_txt3dFree (0x082b4054)`: `(model, texture)`.
        fn txt3d_free(model: u32, texture: u32) = "SI_txt3dFree", 0x082b4054;
        /// `SI_realloc (0x080d0f28)`: `(block, bytes)`.
        fn realloc(block: u32, bytes: u32) -> u32 = "SI_realloc", 0x080d0f28;
        /// `SI_texture3DCopy (0x082b9298)`.
        fn texture3d_copy(texture: u32) -> u32 = "SI_texture3DCopy", 0x082b9298;
    }
}

const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_ELEM_DICTS: u32 = 0x0867_931c;
const SI_GRAF_LISTS: u32 = 0x0867_9498;
const SI_ADD_IN_DICTIONARY: u32 = 0x0867_92b0;
/// `SI_RD`, the relation dictionary, passed by address.
const SI_RD: u32 = 0x0878_15f0;

/// `SI_cid` words (each a 32-bit chapter id in the data segment, read when used).
mod cid {
    pub const FIRST: u32 = 0x0878_1660;
    pub const LIGHTS: u32 = 0x0878_1678;
    pub const CID_1670: u32 = 0x0878_1670;
    pub const CID_1684: u32 = 0x0878_1684;
    pub const CID_16E4: u32 = 0x0878_16e4;
    pub const CLUSTERS: u32 = 0x0878_1668;
    pub const LATTICES: u32 = 0x0878_1674;
    pub const MATERIALS: u32 = 0x0878_167c;
    pub const MODELS: u32 = 0x0878_1680;
    pub const MATERIAL_SHADERS: u32 = 0x0878_1690;
    pub const TEXTURE2D_SHADERS: u32 = 0x0878_1694;
    pub const TEXTURE3D_SHADERS: u32 = 0x0878_1698;
    pub const VOLUME_SHADERS: u32 = 0x0878_169c;
    pub const SHADOW_SHADERS: u32 = 0x0878_16a0;
    pub const LIGHT_SHADERS: u32 = 0x0878_16a4;
    pub const CAMERA_SHADERS: u32 = 0x0878_16a8;
    pub const OUTPUT_SHADERS: u32 = 0x0878_16ac;
    pub const SHAPES: u32 = 0x0878_16b4;
    pub const ENVIRON_SHADERS: u32 = 0x0878_16b0;
    pub const TEXTURES2D: u32 = 0x0878_16c8;
    pub const TEXTURES3D: u32 = 0x0878_16cc;
    pub const WAVES: u32 = 0x0878_16d0;
    pub const PHOTON_SHADERS: u32 = 0x0878_16e8;
    pub const LIGHT_PHOTON_SHADERS: u32 = 0x0878_16ec;
}

/// The current scene's dictionary: `SI_elemDicts + 0x70 * *SI_procData`.
fn cur_dict(m: &mut Mem) -> u32 {
    let p = m.u32(m.u32(SI_PROC_DATA));
    p.wrapping_mul(7).wrapping_mul(16).wrapping_add(m.u32(SI_ELEM_DICTS))
}

/// [soft 0x08101598 si_relGetDynConsType]: the dynamics constraint type of a relation id
/// (0x321 to 0x327), -1 for any other id.
pub fn rel_get_dyn_cons_type(id: i32) -> i32 {
    match id {
        0x321 => 10,
        0x322 => 0,
        0x323 => 4,
        0x324 => 6,
        0x325 => 0xd,
        0x326 => 7,
        0x327 => 9,
        _ => -1,
    }
}

/// [soft 0x081015f0 relGetDynConsRelid]: the relation id of a dynamics constraint type
/// (0, 4, 6, 7, 9, 10, 13), -1 for any other.
pub fn rel_get_dyn_cons_relid(kind: i32) -> i32 {
    match kind {
        0 => 0x322,
        4 => 0x323,
        6 => 0x324,
        7 => 0x326,
        9 => 0x327,
        10 => 0x321,
        13 => 0x325,
        _ => -1,
    }
}

/// [soft 0x0810173c SI_edElementSetDynamics]: the second model's element is related to
/// itself with id 800 (side 1). The first model is not read.
pub fn ed_element_set_dynamics(m: &mut Mem, o: &mut dyn Outside, _model: u32, model2: u32) {
    use Ed2Calls as _;
    let index = m.local(4);
    o.ed_get_elem_from_model(m, model2, index);
    let e = m.u32(index);
    let d = cur_dict(m);
    o.elemdict_add_relation(m, d, SI_RD, e, e, 800, 1, 0);
}

/// [soft 0x08101788 SI_edElementRelaxDynamics]: the relation 800 of the second model's
/// element is removed.
pub fn ed_element_relax_dynamics(m: &mut Mem, o: &mut dyn Outside, _model: u32, model2: u32) {
    use Ed2Calls as _;
    let index = m.local(4);
    o.ed_get_elem_from_model(m, model2, index);
    let e = m.u32(index);
    let d = cur_dict(m);
    o.elemdict_remove_relation_id(m, d, SI_RD, e, 800);
}

/// [soft 0x0810164c SI_edSelectDynaConstraint]: relates the two models' elements with the
/// relation id of the constraint type, when both elements exist (index not negative).
pub fn ed_select_dyna_constraint(m: &mut Mem, o: &mut dyn Outside, model1: u32, model2: u32, kind: i32) {
    use Ed2Calls as _;
    let (i1, i2) = (m.local(4), m.local(4));
    o.ed_get_elem_from_model(m, model1, i1);
    o.ed_get_elem_from_model(m, model2, i2);
    let (a, b) = (m.i32(i1), m.i32(i2));
    if a > -1 && b > -1 {
        let id = rel_get_dyn_cons_relid(kind);
        let d = cur_dict(m);
        o.elemdict_add_relation(m, d, SI_RD, a as u32, b as u32, id as u32, 1, 0);
    }
}

/// [soft 0x081016c8 SI_edRelaxDynaConstraint]: removes that relation pair (no test of the
/// indices).
pub fn ed_relax_dyna_constraint(m: &mut Mem, o: &mut dyn Outside, model1: u32, model2: u32, kind: i32) {
    use Ed2Calls as _;
    let (i1, i2) = (m.local(4), m.local(4));
    o.ed_get_elem_from_model(m, model1, i1);
    o.ed_get_elem_from_model(m, model2, i2);
    let id = rel_get_dyn_cons_relid(kind);
    let (a, b) = (m.u32(i1), m.u32(i2));
    let d = cur_dict(m);
    o.elemdict_remove_relation_pair(m, d, SI_RD, a, b, id as u32, 1, 0);
}

/// [soft 0x081014f8 SI_edModelUnlinkWave]: removes the relation pair (model, wave) of id 600,
/// or 0x259 when the wave's word at +0x70 is set.
pub fn ed_model_unlink_wave(m: &mut Mem, o: &mut dyn Outside, model: u32, wave: u32) {
    use Ed2Calls as _;
    let d = cur_dict(m);
    let c = m.u32(cid::MODELS);
    let a = o.elemdict_find_elem_by_data(m, d, c, model);
    let d = cur_dict(m);
    let c = m.u32(cid::WAVES);
    let b = o.elemdict_find_elem_by_data(m, d, c, wave);
    let id = if m.u32(wave + 0x70) != 0 { 0x259 } else { 600 };
    let d = cur_dict(m);
    o.elemdict_remove_relation_pair(m, d, SI_RD, a, b, id, 1, 0);
}

/// [soft 0x08106a50 SI_edCheckGroupRelations]: whether the element's table (at +100) has a
/// positive count at `index * 0x18`.
pub fn ed_check_group_relations(m: &mut Mem, elem: u32, _unused: i32, index: i32) -> bool {
    let table = m.u32(elem.wrapping_add(100));
    m.i32(table.wrapping_add((index as u32).wrapping_mul(0x18))) > 0
}

/// [soft 0x08107b90 SI_wrathUnsupportedMdlType2]: 1 for node kinds 1, 2, 5, 7, 8, 9, 10 and for
/// a kind 6 whose data (second argument) is set and holds 4 at +0xc; else 0.
pub fn wrath_unsupported_mdl_type2(m: &mut Mem, kind: i32, data: u32) -> i16 {
    match kind {
        1 | 2 | 5 | 7 | 8 | 9 | 10 => 1,
        6 => (data != 0 && m.i32(data + 0xc) == 4) as i16,
        _ => 0,
    }
}

/// [soft 0x08107b68 SI_wrathUnsupportedMdlType]: 0 for a null model, else the test above on
/// the model's kind (+0x30) and data (+0x38).
pub fn wrath_unsupported_mdl_type(m: &mut Mem, model: u32) -> i32 {
    if model == 0 {
        return 0;
    }
    let (k, d) = (m.i32(model + 0x30), m.u32(model + 0x38));
    wrath_unsupported_mdl_type2(m, k, d) as i32
}

// ---------------------------------------------------------------- shader removal

/// Removes one shader slot: `FindElemByData(dict, cid, shader)`; when the element is there
/// `SI_edDisposeUDF(shader, cid)`; then `SI_udfDispose(shader)`. (The caller clears the slot.)
fn drop_shader(m: &mut Mem, o: &mut dyn Outside, shader: u32, cid_addr: u32) {
    use Ed2Calls as _;
    let d = cur_dict(m);
    let c = m.u32(cid_addr);
    if (o.elemdict_find_elem_by_data(m, d, c, shader) as i32) > -1 {
        let c = m.u32(cid_addr);
        ed_dispose_udf(m, o, shader, c);
    }
    o.udf_dispose(m, shader);
}

/// [soft 0x08106ddc SI_edMaterialRemoveShader]: for each of the material's four shader slots
/// (+0xdb0 material, +0xdb4 volume, +0xdb8 shadow, +0xdbc photon) that is set: the element is
/// disposed if the dictionary has it, the shader disposed, the slot cleared.
pub fn ed_material_remove_shader(m: &mut Mem, o: &mut dyn Outside, material: u32) {
    if material == 0 {
        return;
    }
    for (slot, c) in [
        (0xdb0, cid::MATERIAL_SHADERS),
        (0xdb4, cid::VOLUME_SHADERS),
        (0xdb8, cid::SHADOW_SHADERS),
        (0xdbc, cid::PHOTON_SHADERS),
    ] {
        let s = m.u32(material + slot);
        if s != 0 {
            drop_shader(m, o, s, c);
            m.set_u32(material + slot, 0);
        }
    }
}

/// [soft 0x08106fa0 SI_edTexture2DRemoveShader]: the 2D texture's shader (+0x198), when set:
/// its element in the texture shaders chapter and in the environment shaders chapter is
/// disposed if there; the shader disposed; the slot cleared.
pub fn ed_texture2d_remove_shader(m: &mut Mem, o: &mut dyn Outside, texture: u32) {
    use Ed2Calls as _;
    if texture == 0 || m.u32(texture + 0x198) == 0 {
        return;
    }
    for c in [cid::TEXTURE2D_SHADERS, cid::ENVIRON_SHADERS] {
        let d = cur_dict(m);
        let cc = m.u32(c);
        let s = m.u32(texture + 0x198);
        if (o.elemdict_find_elem_by_data(m, d, cc, s) as i32) > -1 {
            let cc = m.u32(c);
            let s = m.u32(texture + 0x198);
            ed_dispose_udf(m, o, s, cc);
        }
    }
    let s = m.u32(texture + 0x198);
    o.udf_dispose(m, s);
    m.set_u32(texture + 0x198, 0);
}

/// [soft 0x08107070 SI_edTexture3DRemoveShader]: the same for a 3D texture's shader (+0x14c)
/// and its one chapter.
pub fn ed_texture3d_remove_shader(m: &mut Mem, o: &mut dyn Outside, texture: u32) {
    if texture == 0 || m.u32(texture + 0x14c) == 0 {
        return;
    }
    let s = m.u32(texture + 0x14c);
    drop_shader(m, o, s, cid::TEXTURE3D_SHADERS);
    m.set_u32(texture + 0x14c, 0);
}

/// [soft 0x081070f0 SI_edLightRemoveShader]: a light's shaders at +0x178 (light) and +0x17c
/// (photon), as the material's.
pub fn ed_light_remove_shader(m: &mut Mem, o: &mut dyn Outside, light: u32) {
    if light == 0 {
        return;
    }
    for (slot, c) in [(0x178, cid::LIGHT_SHADERS), (0x17c, cid::LIGHT_PHOTON_SHADERS)] {
        let s = m.u32(light + slot);
        if s != 0 {
            drop_shader(m, o, s, c);
            m.set_u32(light + slot, 0);
        }
    }
}

/// [soft 0x08106cd0 SI_edModelRemoveShader]: every material of the model (the table at +0x20c,
/// the count a 16-bit word at +0x206): its local 2D textures (list at +0x6ec, link at +0x16c)
/// and 3D texture (+0x6fc), then the material's shaders; then the model's own 2D textures
/// (list at +0x21c), 3D texture (+0x224), and the two sub-models at +0x2c4 and +0x2c0.
pub fn ed_model_remove_shader(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    if m.u32(model + 0x20c) != 0 {
        let mut i = 0i32;
        if 0 < m.i16(model + 0x206) as i32 {
            loop {
                let next = i + 1;
                let material = m.u32(m.u32(model + 0x20c).wrapping_add((i as u32).wrapping_mul(4)));
                if material != 0 {
                    let mut t = m.u32(material + 0x6ec);
                    while t != 0 {
                        ed_texture2d_remove_shader(m, o, t);
                        t = m.u32(t + 0x16c);
                    }
                    if m.u32(material + 0x6fc) != 0 {
                        let t3 = m.u32(material + 0x6fc);
                        ed_texture3d_remove_shader(m, o, t3);
                    }
                    ed_material_remove_shader(m, o, material);
                }
                i = next;
                if i >= m.i16(model + 0x206) as i32 {
                    break;
                }
            }
        }
    }
    let mut t = m.u32(model + 0x21c);
    while t != 0 {
        ed_texture2d_remove_shader(m, o, t);
        t = m.u32(t + 0x16c);
    }
    if m.u32(model + 0x224) != 0 {
        let t3 = m.u32(model + 0x224);
        ed_texture3d_remove_shader(m, o, t3);
    }
    if m.u32(model + 0x2c4) != 0 {
        let s = m.u32(model + 0x2c4);
        ed_model_remove_shader(m, o, s);
    }
    if m.u32(model + 0x2c0) != 0 {
        let s = m.u32(model + 0x2c0);
        ed_model_remove_shader(m, o, s);
    }
}

/// [soft 0x0810731c edRemoveMultipleShader]: every element of the dictionary's chapter (0x40
/// bytes per chapter at `*(dict+4)`; count at +4, elements of 0x84 bytes at +8): its shader
/// (+4) disposed, the element deleted (`SI_elemdictSetDeleteElem(dict, chapter, element,
/// number, -1, 0, 1)`); then `SI_dbSortTblSetDirty(chapter record)`.
pub fn ed_remove_multiple_shader(m: &mut Mem, o: &mut dyn Outside, dict: u32, chapter: u32) {
    use Ed2Calls as _;
    let list = chapter.wrapping_mul(0x40).wrapping_add(m.u32(dict + 4));
    let mut n = 0i32;
    if 0 < m.i32(list + 4) {
        let mut at = 0u32;
        loop {
            let base = m.u32(list + 8);
            let s = m.u32(at.wrapping_add(base).wrapping_add(4));
            o.udf_dispose(m, s);
            o.elemdict_set_delete_elem(m, dict, chapter, at.wrapping_add(base), n as u32, u32::MAX, 0, 1);
            at = at.wrapping_add(0x84);
            n = n.wrapping_add(1);
            if n >= m.i32(list + 4) {
                break;
            }
        }
    }
    o.db_sort_tbl_set_dirty(m, list);
}

/// `SI_edCameraRemoveShader` / `SI_edOutputRemoveShader` also clear four globals each and
/// free one block.
fn clear_multiple(m: &mut Mem, free_at: u32, zero: [u32; 3]) {
    let p = m.u32(free_at);
    if p != 0 {
        m.free(p);
        m.set_u32(free_at, 0);
    }
    m.set_u32(zero[0], 0);
    m.set_u32(zero[1], 0);
    m.set_u32(zero[2], 0);
}

/// [soft 0x081071dc SI_edCameraRemoveShader]: `edRemoveMultipleShader(dict, SI_cid.CAMERA_SHADERS)`,
/// then the block at 0x08781bf0 freed and cleared and the words 0x08781be8, 0x08790fd0,
/// 0x08790fc8 cleared.
pub fn ed_camera_remove_shader(m: &mut Mem, o: &mut dyn Outside, dict: u32) {
    let c = m.u32(cid::CAMERA_SHADERS);
    ed_remove_multiple_shader(m, o, dict, c);
    clear_multiple(m, 0x0878_1bf0, [0x0878_1be8, 0x0879_0fd0, 0x0879_0fc8]);
}

/// [soft 0x08107238 SI_edOutputRemoveShader]: as the camera's with the output shaders chapter,
/// the block at 0x08781bec and the words 0x08781be4, 0x08790fcc, 0x08790fc4.
pub fn ed_output_remove_shader(m: &mut Mem, o: &mut dyn Outside, dict: u32) {
    let c = m.u32(cid::OUTPUT_SHADERS);
    ed_remove_multiple_shader(m, o, dict, c);
    clear_multiple(m, 0x0878_1bec, [0x0878_1be4, 0x0879_0fcc, 0x0879_0fc4]);
}

/// [soft 0x08107294 SI_edAtmosphereRemoveShader]: the atmosphere shader (0x08781c4c), when set:
/// its element in the volume shaders chapter is deleted (`SI_elemdictSetDeleteElem(dict,
/// chapter, element, number, -1, 0, 1)`) when found, the shader disposed and the slot cleared;
/// then the word 0x0879102c cleared.
pub fn ed_atmosphere_remove_shader(m: &mut Mem, o: &mut dyn Outside, dict: u32) {
    use Ed2Calls as _;
    let s = m.u32(0x0878_1c4c);
    if s != 0 {
        let c = m.u32(cid::VOLUME_SHADERS);
        let n = o.elemdict_find_elem_by_data(m, dict, c, s);
        if (n as i32) > -1 {
            let c = m.u32(cid::VOLUME_SHADERS);
            let e = o.elemdict_get_elem(m, dict, c, n);
            if e != 0 {
                let c = m.u32(cid::VOLUME_SHADERS);
                o.elemdict_set_delete_elem(m, dict, c, e, n, u32::MAX, 0, 1);
            }
        }
        o.udf_dispose(m, s);
        m.set_u32(0x0878_1c4c, 0);
    }
    m.set_u32(0x0879_102c, 0);
}

// ---------------------------------------------------------------- disposers

/// Takes the element of `data` out of the dictionary in two passes of the delete flag:
/// `FindElemByData`, `GetElem`, `SI_edElementSetDeleteFlag(element, chapter, number, 1, 0, 0, 0)`
/// and again with a last argument of 1.
fn dispose_by_data(m: &mut Mem, o: &mut dyn Outside, data: u32, cid_addr: u32, owner: Option<u32>) {
    use Ed2Calls as _;
    let d = cur_dict(m);
    let c = m.u32(cid_addr);
    let n = o.elemdict_find_elem_by_data(m, d, c, data);
    let d = cur_dict(m);
    let c = m.u32(cid_addr);
    let e = o.elemdict_get_elem(m, d, c, n);
    if let Some(w) = owner {
        let c = m.u32(cid_addr);
        o.ed_fcv_dispose(m, w, c, 0, 0x266, 0x267);
    }
    let c = m.u32(cid_addr);
    o.ed_element_set_delete_flag(m, e, c, n, 1, 0, 0, 0);
    let c = m.u32(cid_addr);
    o.ed_element_set_delete_flag(m, e, c, n, 1, 0, 0, 1);
}

/// [soft 0x08103974 SI_edDisposeLattice]: the lattice at the model's +0x50 (second argument
/// 0) or +0x58 (else): its first word is the data looked up.
pub fn ed_dispose_lattice(m: &mut Mem, o: &mut dyn Outside, model: u32, which: i16) {
    let slot = if which == 0 { m.u32(model + 0x50) } else { m.u32(model + 0x58) };
    let data = m.u32(slot);
    dispose_by_data(m, o, data, cid::LATTICES, None);
}

/// [soft 0x08103454 SI_edDisposeWave]: `SI_edFcvDispose(wave, chapter, 0, 0x266, 0x267)` between
/// the look-up and the two delete passes.
pub fn ed_dispose_wave(m: &mut Mem, o: &mut dyn Outside, wave: u32) {
    dispose_by_data(m, o, wave, cid::WAVES, Some(wave));
}

/// [soft 0x0810360c SI_edDisposeCluster]: the cluster's element out of the clusters chapter.
pub fn ed_dispose_cluster(m: &mut Mem, o: &mut dyn Outside, _model: u32, cluster: u32) {
    dispose_by_data(m, o, cluster, cid::CLUSTERS, None);
}

/// [soft 0x081037a0 SI_edDisposeShape]: the model's element out of the shapes chapter.
pub fn ed_dispose_shape(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    dispose_by_data(m, o, model, cid::SHAPES, None);
}

/// [soft 0x08103ca8 SI_edDisposeUDF]: nothing for a null shader; else, when the chapter given
/// has its element (number not below 0) and `SI_elemdictGetElem` finds it, the two delete passes.
pub fn ed_dispose_udf(m: &mut Mem, o: &mut dyn Outside, udf: u32, chapter: u32) {
    use Ed2Calls as _;
    if udf == 0 {
        return;
    }
    let d = cur_dict(m);
    let n = o.elemdict_find_elem_by_data(m, d, chapter, udf);
    if (n as i32) > -1 {
        let d = cur_dict(m);
        let e = o.elemdict_get_elem(m, d, chapter, n);
        if e != 0 {
            o.ed_element_set_delete_flag(m, e, chapter, n, 1, 0, 0, 0);
            o.ed_element_set_delete_flag(m, e, chapter, n, 1, 0, 0, 1);
        }
    }
}

/// The element of a record, its graph node cut from its connections and removed from the
/// current graph list; the element's two delete passes.
fn drop_elem_and_node(m: &mut Mem, o: &mut dyn Outside, elem: u32, cid_addr: u32, index: u32) {
    use Ed2Calls as _;
    let node = m.u32(elem + 8);
    if node != 0 {
        o.grafnode_cut_connections(m, node);
        let lists = m.u32(SI_GRAF_LISTS);
        let which = m.u32(m.u32(SI_PROC_DATA) + 4);
        o.graflist_remove_grafnode(m, lists.wrapping_add(which.wrapping_mul(0x24)), node);
    }
    let c = m.u32(cid_addr);
    let n = m.u32(index);
    o.ed_element_set_delete_flag(m, elem, c, n, 1, 0, 0, 0);
    let c = m.u32(cid_addr);
    let n = m.u32(index);
    o.ed_element_set_delete_flag(m, elem, c, n, 1, 0, 0, 1);
}

/// [soft 0x08102fcc edTexture2DRemove]: the map record at +0x24 given back
/// (`SI_global2dMapRemove`); when the texture has an element: node and element dropped, the
/// shader at +0x198 (when set) disposed in the texture and environment shader chapters, then
/// `SI_udfDispose` and the slot cleared.
pub fn ed_texture2d_remove(m: &mut Mem, o: &mut dyn Outside, texture: u32) {
    use Ed2Calls as _;
    o.global2d_map_remove(m, texture + 0x24);
    let index = m.local(4);
    let e = o.ed_get_elem_from_texture2d(m, texture, index);
    if e != 0 {
        drop_elem_and_node(m, o, e, cid::TEXTURES2D, index);
        if m.u32(texture + 0x198) != 0 {
            let s = m.u32(texture + 0x198);
            let c = m.u32(cid::TEXTURE2D_SHADERS);
            ed_dispose_udf(m, o, s, c);
            let s = m.u32(texture + 0x198);
            let c = m.u32(cid::ENVIRON_SHADERS);
            ed_dispose_udf(m, o, s, c);
        }
        let s = m.u32(texture + 0x198);
        o.udf_dispose(m, s);
        m.set_u32(texture + 0x198, 0);
    }
}

/// [soft 0x081030c0 edTexture3DRemove]: as the 2D one without the map and with the shader at
/// +0x14c disposed unconditionally in the 3D shader chapter.
pub fn ed_texture3d_remove(m: &mut Mem, o: &mut dyn Outside, texture: u32) {
    use Ed2Calls as _;
    let index = m.local(4);
    let e = o.ed_get_elem_from_texture3d(m, texture, index);
    if e != 0 {
        drop_elem_and_node(m, o, e, cid::TEXTURES3D, index);
        let s = m.u32(texture + 0x14c);
        let c = m.u32(cid::TEXTURE3D_SHADERS);
        ed_dispose_udf(m, o, s, c);
        let s = m.u32(texture + 0x14c);
        o.udf_dispose(m, s);
        m.set_u32(texture + 0x14c, 0);
    }
}

/// A 3D texture that is still used elsewhere gets a copy, without its shader.
fn replace_used_texture3d(m: &mut Mem, o: &mut dyn Outside, holder: u32, slot: u32) {
    use Ed2Calls as _;
    let t = m.u32(holder + slot);
    let copy = o.texture3d_copy(m, t);
    m.set_u32(holder + slot, copy);
    if copy != 0 {
        let s = m.u32(copy + 0x14c);
        o.udf_dispose(m, s);
        let t = m.u32(holder + slot);
        m.set_u32(t + 0x14c, 0);
    }
}

/// [soft 0x0810318c edModelMaterialRemove]: `SI_edModelRelaxMaterial(model, material)`; when
/// the material is not used elsewhere: its local 2D textures (relaxed, removed), its 3D
/// texture (relaxed, removed or, if still used, replaced by a copy), its four shaders
/// disposed (elements, then the shaders) and the slots cleared, and its element dropped.
/// Answers the material.
pub fn ed_model_material_remove(m: &mut Mem, o: &mut dyn Outside, model: u32, material: u32) -> u32 {
    use Ed2Calls as _;
    o.ed_model_relax_material(m, model, material);
    if o.material_still_used(m, material) == 0 {
        let mut t = m.u32(material + 0x6ec);
        while t != 0 {
            o.ed_model_relax_ltexture2d(m, material, t);
            ed_texture2d_remove(m, o, t);
            t = m.u32(t + 0x16c);
        }
        if m.u32(material + 0x6fc) != 0 {
            let t3 = m.u32(material + 0x6fc);
            o.ed_model_relax_ltexture3d(m, material, t3);
            let t3 = m.u32(material + 0x6fc);
            if o.texture3d_still_used(m, t3) == 0 {
                let t3 = m.u32(material + 0x6fc);
                ed_texture3d_remove(m, o, t3);
            } else {
                replace_used_texture3d(m, o, material, 0x6fc);
            }
        }
        for (slot, c) in [
            (0xdb0, cid::MATERIAL_SHADERS),
            (0xdb4, cid::VOLUME_SHADERS),
            (0xdb8, cid::SHADOW_SHADERS),
            (0xdbc, cid::PHOTON_SHADERS),
        ] {
            let s = m.u32(material + slot);
            let c = m.u32(c);
            ed_dispose_udf(m, o, s, c);
        }
        for slot in [0xdb0, 0xdb4, 0xdb8, 0xdbc] {
            let s = m.u32(material + slot);
            o.udf_dispose(m, s);
        }
        for slot in [0xdb0, 0xdb4, 0xdb8, 0xdbc] {
            m.set_u32(material + slot, 0);
        }
        let index = m.local(4);
        let e = o.ed_get_elem_from_material(m, material, index);
        if e != 0 {
            drop_elem_and_node(m, o, e, cid::MATERIALS, index);
        }
    }
    material
}

/// [soft 0x08102ec4 edModelRemoveMatTxt]: for an element with an object (+4, a model): each
/// of the model's materials removed from it, its 2D textures removed, and its 3D texture
/// (+0x224) relaxed from the model and removed, or, if still used, replaced by a copy without
/// its shader.
pub fn ed_model_remove_mat_txt(m: &mut Mem, o: &mut dyn Outside, elem: u32) {
    use Ed2Calls as _;
    if elem == 0 {
        return;
    }
    let model = m.u32(elem + 4);
    if model == 0 {
        return;
    }
    if m.u32(model + 0x20c) != 0 {
        let mut i = 0u32;
        if 0 < m.i16(model + 0x206) as i32 {
            loop {
                let material = m.u32(m.u32(model + 0x20c).wrapping_add(i.wrapping_mul(4)));
                if material != 0 {
                    ed_model_material_remove(m, o, model, material);
                }
                i = i.wrapping_add(1);
                if (i as i32) >= m.i16(model + 0x206) as i32 {
                    break;
                }
            }
        }
    }
    let mut t = m.u32(model + 0x21c);
    while t != 0 {
        ed_texture2d_remove(m, o, t);
        t = m.u32(t + 0x16c);
    }
    if m.u32(model + 0x224) != 0 {
        let t3 = m.u32(model + 0x224);
        o.ed_model_relax_gtexture3d(m, model, t3);
        let t3 = m.u32(model + 0x224);
        if o.texture3d_still_used(m, t3) == 0 {
            let t3 = m.u32(model + 0x224);
            ed_texture3d_remove(m, o, t3);
        } else {
            replace_used_texture3d(m, o, model, 0x224);
        }
    }
}

// ---------------------------------------------------------------- adders

/// [soft 0x081033b4 SI_edAddWave]: nothing while `SI_addInDictionary` is 0. With a wave: a name
/// record (`SI_dbListNodeAllocate`) whose +0x14 is a copy of the wave's name (the text pointer
/// at the wave's +4), whose +0x24 is `SI_dbFormatFullName(record)`, entered in the waves
/// chapter with the wave as its data. Without a wave, with a graph node: the element of the
/// wave the node's +4 points to gets the node at +8.
pub fn ed_add_wave(m: &mut Mem, o: &mut dyn Outside, node: u32, wave: u32) {
    use Ed2Calls as _;
    let c = m.u32(cid::WAVES);
    if m.i16(SI_ADD_IN_DICTIONARY) == 0 {
        return;
    }
    if wave == 0 {
        if node != 0 {
            let index = m.local(4);
            let w = m.u32(node + 4);
            let e = o.ed_get_elem_from_wave(m, w, index);
            m.set_u32(e + 8, node);
        }
    } else {
        let rec = o.db_list_node_allocate(m);
        let name = m.u32(wave + 4);
        let copy = m.strdup(name);
        m.set_u32(rec + 0x14, copy);
        let full = o.db_format_full_name(m, rec);
        m.set_u32(rec + 0x24, full);
        let d = cur_dict(m);
        o.elemdict_add_elem(m, d, c, rec, wave, 0, 0, 0);
    }
}

// ---------------------------------------------------------------- disposers with relations

/// [soft 0x08101f18 SI_edDisposeLight]: the light's element in the lights chapter gets the
/// delete flag (first pass only) unconditionally, and once more when the look-up still finds
/// it; then the light's two shader slots (+0x178, +0x17c) are disposed. Answers 1.
pub fn ed_dispose_light(m: &mut Mem, o: &mut dyn Outside, light: u32) -> u32 {
    use Ed2Calls as _;
    let d = cur_dict(m);
    let c = m.u32(cid::LIGHTS);
    let n = o.elemdict_find_elem_by_data(m, d, c, light);
    let d = cur_dict(m);
    let c = m.u32(cid::LIGHTS);
    let e = o.elemdict_get_elem(m, d, c, n);
    let c = m.u32(cid::LIGHTS);
    o.ed_element_set_delete_flag(m, e, c, n, 1, 0, 0, 0);
    let d = cur_dict(m);
    let c = m.u32(cid::LIGHTS);
    let n = o.elemdict_find_elem_by_data(m, d, c, light);
    if (n as i32) > -1 {
        let d = cur_dict(m);
        let c = m.u32(cid::LIGHTS);
        let e = o.elemdict_get_elem(m, d, c, n);
        let c = m.u32(cid::LIGHTS);
        o.ed_element_set_delete_flag(m, e, c, n, 1, 0, 0, 0);
    }
    let (s0, c0) = (m.u32(light + 0x178), m.u32(cid::LIGHT_SHADERS));
    ed_dispose_udf(m, o, s0, c0);
    let (s1, c1) = (m.u32(light + 0x17c), m.u32(cid::LIGHT_PHOTON_SHADERS));
    ed_dispose_udf(m, o, s1, c1);
    1
}

/// [soft 0x08102498 si_updateUVDomainInfo]: for a model of kind 4 with more than one entry in
/// its table of (array, count) pairs (+0x424 the count, +0x428 the table): each of the first
/// count-1 pairs with a positive count has the elements equal to `number` squeezed out of its
/// array; when any went, the count is lowered, `SI_secureLicAlloc` (0x08679660) counted up
/// and the array reallocated (`SI_realloc`) to the new count.
pub fn update_uv_domain_info(m: &mut Mem, o: &mut dyn Outside, model: u32, number: i32) {
    use Ed2Calls as _;
    if m.i32(model + 0x30) != 4 {
        return;
    }
    let mut count = m.i32(model + 0x424);
    if count <= 1 {
        return;
    }
    let mut i = 0i32;
    while i < count - 1 {
        let pair = m.u32(model + 0x428).wrapping_add((i as u32).wrapping_mul(8));
        if 0 < m.i32(pair + 4) && m.u32(pair) != 0 {
            let (mut kept, mut k) = (0i32, 0i32);
            if 0 < m.i32(pair + 4) {
                loop {
                    let v = m.i32(m.u32(pair).wrapping_add((k as u32).wrapping_mul(4)));
                    if v != number {
                        m.set_i32(m.u32(pair).wrapping_add((kept as u32).wrapping_mul(4)), v);
                        kept += 1;
                    }
                    k += 1;
                    if k >= m.i32(pair + 4) {
                        break;
                    }
                }
            }
            if kept != m.i32(pair + 4) {
                m.set_i32(pair + 4, kept);
                let n = m.u32(0x0867_9660).wrapping_add(1);
                m.set_u32(0x0867_9660, n);
                let block = m.u32(pair);
                let r = o.realloc(m, block, (kept as u32).wrapping_shl(2));
                m.set_u32(pair, r);
            }
        }
        i += 1;
        count = m.i32(model + 0x424);
    }
}

/// [soft 0x081028d0 SI_edDisposeTexture3D]: nothing (answer 1) for a null texture, 0 for one
/// without an element. Else every model related to the element by 500 and every material by
/// 0x1f5 has the texture disassociated; the graph node dropped, the delete flag passed twice,
/// the shader disposed and the texture freed. Answers 1.
pub fn ed_dispose_texture3d(m: &mut Mem, o: &mut dyn Outside, texture: u32) -> u32 {
    use Ed2Calls as _;
    if texture == 0 {
        return 1;
    }
    let index = m.local(4);
    let e = o.ed_get_elem_from_texture3d(m, texture, index);
    if e == 0 {
        return 0;
    }
    let (la, lb) = (m.local(4), m.local(4));
    for (id, cidaddr, models) in [(500u32, cid::MODELS, true), (0x1f5, cid::MATERIALS, false)] {
        let d = cur_dict(m);
        let c = m.u32(cidaddr);
        let mut r = o.elemdict_find_related_elem(m, d, e, 0, 0, id, c, la, lb);
        while r != 0 {
            let obj = m.u32(r + 4);
            if models {
                o.texture3d_gdisassociate(m, obj, texture, 0);
            } else {
                o.texture3d_ldisassociate(m, obj, texture, 0);
            }
            let d = cur_dict(m);
            let c = m.u32(cidaddr);
            r = o.elemdict_find_next_related_elem(m, d, e, 0, 0, id, c, la, lb);
        }
    }
    drop_elem_and_node(m, o, e, cid::TEXTURES3D, index);
    let s = m.u32(texture + 0x14c);
    let c = m.u32(cid::TEXTURE3D_SHADERS);
    ed_dispose_udf(m, o, s, c);
    o.txt3d_free(m, 0, texture);
    1
}

/// [soft 0x0810253c SI_edDisposeTexture2D]: as the 3D one for a 2D texture: every model
/// related by 400 has it disassociated and its UV domain info updated with the texture's
/// element number; every material related by 0x191 likewise (local disassociate), with the
/// UV update for every model related to that material by 300; then node and element dropped,
/// the shader (+0x198) disposed in the texture and environment chapters when set, and the
/// texture disposed (`SI_txt2dDispose(null, &texture)`). Answers 1; 0 without an element.
pub fn ed_dispose_texture2d(m: &mut Mem, o: &mut dyn Outside, texture: u32) -> u32 {
    use Ed2Calls as _;
    if texture == 0 {
        return 1;
    }
    let index = m.local(4);
    let e = o.ed_get_elem_from_texture2d(m, texture, index);
    if e == 0 {
        return 0;
    }
    let (la, lb) = (m.local(4), m.local(4));
    let d = cur_dict(m);
    let c = m.u32(cid::MODELS);
    let mut r = o.elemdict_find_related_elem(m, d, e, 0, 0, 400, c, la, lb);
    while r != 0 {
        let model = m.u32(r + 4);
        o.texture2d_gdisassociate(m, model, texture, 0);
        let number = m.i32(index);
        update_uv_domain_info(m, o, model, number);
        let d = cur_dict(m);
        let c = m.u32(cid::MODELS);
        r = o.elemdict_find_next_related_elem(m, d, e, 0, 0, 400, c, la, lb);
    }
    let d = cur_dict(m);
    let c = m.u32(cid::MATERIALS);
    let mut r = o.elemdict_find_related_elem(m, d, e, 0, 0, 0x191, c, la, lb);
    while r != 0 {
        let material = m.u32(r + 4);
        o.texture2d_ldisassociate(m, material, texture, 0);
        let d = cur_dict(m);
        let c = m.u32(cid::MODELS);
        let mut q = o.elemdict_find_related_elem(m, d, r, 0, 0, 300, c, la, lb);
        while q != 0 {
            let model = m.u32(q + 4);
            let number = m.i32(index);
            update_uv_domain_info(m, o, model, number);
            let d = cur_dict(m);
            let c = m.u32(cid::MODELS);
            q = o.elemdict_find_next_related_elem(m, d, r, 0, 0, 300, c, la, lb);
        }
        let d = cur_dict(m);
        let c = m.u32(cid::MATERIALS);
        r = o.elemdict_find_next_related_elem(m, d, e, 0, 0, 0x191, c, la, lb);
    }
    drop_elem_and_node(m, o, e, cid::TEXTURES2D, index);
    if m.u32(texture + 0x198) != 0 {
        let s = m.u32(texture + 0x198);
        let c = m.u32(cid::TEXTURE2D_SHADERS);
        ed_dispose_udf(m, o, s, c);
        let s = m.u32(texture + 0x198);
        let c = m.u32(cid::ENVIRON_SHADERS);
        ed_dispose_udf(m, o, s, c);
    }
    let holder = m.local(4);
    m.set_u32(holder, texture);
    o.txt2d_dispose(m, 0, holder);
    1
}

/// [soft 0x08102114 SI_edDisposeMaterial]: 0 for a null material. Else the material's element
/// (none: the flag is 0). Every model related to it by 300 has the material disassociated
/// (`SI_materialDisassociate(model, material, 0, 1)`) unless the model has two or more
/// materials and this is its first, which clears the flag. When the flag is still set: node
/// and element dropped and the four shader slots' elements disposed. Then, when `dispose` is
/// not 0 and (the flag is set or there is no element), `SI_materialDispose(last model,
/// &material)`. Answers the flag. (The last model starts as 0, as the original's register.)
pub fn ed_dispose_material(m: &mut Mem, o: &mut dyn Outside, material: u32, dispose: i16) -> u32 {
    use Ed2Calls as _;
    let mut model = 0u32;
    if material == 0 {
        return 0;
    }
    let index = m.local(4);
    let e = o.ed_get_elem_from_material(m, material, index);
    let mut flag = (e != 0) as u16;
    let (la, lb) = (m.local(4), m.local(4));
    let mut r = 0;
    if e != 0 {
        let d = cur_dict(m);
        let c = m.u32(cid::MODELS);
        r = o.elemdict_find_related_elem(m, d, e, 0, 0, 300, c, la, lb);
    }
    while r != 0 && e != 0 {
        model = m.u32(r + 4);
        if m.i16(model + 0x206) < 2 || m.u32(m.u32(model + 0x20c)) != material {
            o.material_disassociate(m, model, material, 0, 1);
        } else {
            flag = 0;
        }
        let d = cur_dict(m);
        let c = m.u32(cid::MODELS);
        r = o.elemdict_find_next_related_elem(m, d, e, 0, 0, 300, c, la, lb);
    }
    if flag != 0 {
        drop_elem_and_node(m, o, e, cid::MATERIALS, index);
        for (slot, c) in [(0xdb0, cid::MATERIAL_SHADERS), (0xdb4, cid::VOLUME_SHADERS), (0xdb8, cid::SHADOW_SHADERS), (0xdbc, cid::PHOTON_SHADERS)] {
            let s = m.u32(material + slot);
            let c = m.u32(c);
            ed_dispose_udf(m, o, s, c);
        }
    }
    if dispose != 0 && (flag != 0 || e == 0) {
        let holder = m.local(4);
        m.set_u32(holder, material);
        o.material_dispose(m, model, holder);
    }
    flag as u32
}

// ---------------------------------------------------------------- dictionary scans

/// [soft 0x08104928 SI_edUnselectedNseInDict]: in every chapter of the dictionary (count at
/// +0, chapters at +4) except two (the words at 0x087816e4 and 0x08781670), each element in use
/// (+0x48 = 0) whose graph node (+8) has bit 4 of its byte at +0x17 set has its 16-bit word at
/// +0x42 cleared.
pub fn ed_unselected_nse_in_dict(m: &mut Mem, dict: u32) {
    let mut ch = 0u32;
    if m.u32(dict) == 0 {
        return;
    }
    loop {
        let next = ch.wrapping_add(1);
        if ch != m.u32(cid::CID_16E4) && ch != m.u32(cid::CID_1670) {
            let list = ch.wrapping_mul(0x40).wrapping_add(m.u32(dict + 4));
            let mut k = 0u32;
            if m.u32(list + 4) != 0 {
                let mut at = 0u32;
                loop {
                    let e = at.wrapping_add(m.u32(list + 8));
                    if m.i16(e + 0x48) == 0 && m.u32(e + 8) != 0 && (m.u8(m.u32(e + 8) + 0x17) & 0x10) != 0 {
                        m.set_u16(e + 0x42, 0);
                    }
                    at = at.wrapping_add(0x84);
                    k = k.wrapping_add(1);
                    if k >= m.u32(list + 4) {
                        break;
                    }
                }
            }
        }
        ch = next;
        if ch >= m.u32(dict) {
            break;
        }
    }
}

/// [soft 0x08107fc4 SI_edIsNseByDependence]: 0 for a null dictionary or element. Else over the
/// element's relation groups (+0x68 the count, +0x6c the table of 0x18 bytes each, a count at
/// +0 and a table of 0x20-byte relations at +0xc): the first relation whose id is none of
/// 0xee, 800, 0x321 to 0x327, whose chapter (16 bits at +0x10) is one of eight listed
/// chapters, and whose element (`SI_elemdictGetElem(dict, chapter, relation +4)`) exists, is
/// another element, is not in state 2 and has a graph node: answers that node's 16-bit word at
/// +0x16 masked with 0x1000.
pub fn ed_is_nse_by_dependence(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32) -> u16 {
    use Ed2Calls as _;
    let mut r: u16 = 0;
    if dict == 0 || elem == 0 {
        return 0;
    }
    if m.u32(elem + 0x6c) != 0 && 0 < m.i32(elem + 0x68) {
        let mut g = 0i32;
        loop {
            let next = g + 1;
            let group = m.u32(elem + 0x6c).wrapping_add((g as u32).wrapping_mul(0x18));
            let mut k = 0i32;
            while r == 0 && k < m.i32(group) {
                let rel = (k as u32).wrapping_mul(0x20).wrapping_add(m.u32(group + 12));
                let id = m.i32(rel);
                if id != 0xee && id != 800 && !(0x321..=0x327).contains(&id) {
                    let chapter = m.i16(rel + 0x10) as i32 as u32;
                    let listed = [cid::FIRST, cid::CID_1684, cid::MATERIALS, cid::TEXTURES2D, cid::TEXTURES3D, cid::LIGHTS, cid::WAVES, cid::MODELS]
                        .iter()
                        .any(|&a| chapter == m.u32(a));
                    if listed {
                        let number = m.u32(rel + 4);
                        let other = o.elemdict_get_elem(m, dict, chapter, number);
                        if other != 0 && other != elem && m.i16(other + 0x48) != 2 && m.u32(other + 8) != 0 {
                            r = m.u16(m.u32(other + 8) + 0x16) & 0x1000;
                        }
                    }
                }
                k += 1;
            }
            if r != 0 || next >= m.i32(elem + 0x68) {
                break;
            }
            g = next;
        }
    }
    r
}
