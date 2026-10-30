// SPDX-License-Identifier: MIT
//! The copy-from-setup routines of `matter.c`: what `SI_modelCopyFromSetup (0x084633e8)` calls to give a model the materials and
//! textures of the setup model (unit cus1, finding 0041, 2026-10-29), and the matter start-up `SI_matterInit`.
//!
//! The model records are read and written on the memory image: +0x206 the material count (short), +0x208 the current material
//! (short), +0x20c the array of material records, +0x218 / +0x21c the tail and head of the model's list of global 2D textures
//! (next at texture +0x16c), +0x224 its global 3D texture; a material record has its local 2D texture list at +0x6ec (tail
//! +0x6f0) and its local 3D texture at +0x6fc, and the list of materials of the graph list at +0x6e4. Every other routine is
//! an outside call (the texture, shader and graph-list routines belong to other sources).

use si_mem::{Mem, Outside, Routine};
use si_x87::x;

pub const SI_PROC_DATA: u32 = 0x0867_9314;
pub const SI_GRAF_LISTS: u32 = 0x0867_9498;
pub const SI_ELEM_DICTS: u32 = 0x0867_931c;
pub const SI_CID_MODELS: u32 = 0x0878_1680;
pub const SI_CID_MATERIALS: u32 = 0x0878_167c;
pub const SI_CID_TEXTURES2D: u32 = 0x0878_16c8;
pub const SI_CID_TEXTURES3D: u32 = 0x0878_16cc;
pub const SI_DEFAULT_MATERIAL: u32 = 0x0869_dde0;
pub const SI_SHW_TXT: u32 = 0x0867_93ea;
/// `SI_glbDuplic` and the copy switches (shorts) of the setup's global flags.
pub const SI_GLB_DUPLIC: u32 = 0x0878_14c0;
pub const G_ASSOC_MAT: u32 = 0x0878_14c2;
pub const G_FCV_MAT: u32 = 0x0878_14c4;
pub const G_L2D: u32 = 0x0878_14c6;
pub const G_L3D_ASSOC: u32 = 0x0878_14ce;
pub const G_L2D_FCV: u32 = 0x0878_14ca;
pub const G_L3D: u32 = 0x0878_14cc;
pub const G_L3D_FCV: u32 = 0x0878_14d0;
pub const G_G2D: u32 = 0x0878_14d2;
pub const G_G2D_FCV: u32 = 0x0878_14d6;
pub const G_G3D: u32 = 0x0878_14d8;
pub const G_G3D_ASSOC: u32 = 0x0878_14da;
pub const G_G3D_FCV: u32 = 0x0878_14dc;

pub const SI_ELEMDICT_GET_ELEM_FROM_DATA: Routine = Routine::new("SI_elemdictGetElemFromData", 0x0813_7f94);
pub const SI_ED_DISPOSE_TEXTURE2D: Routine = Routine::new("SI_edDisposeTexture2D", 0x0810_253c);
pub const SI_ED_DISPOSE_TEXTURE3D: Routine = Routine::new("SI_edDisposeTexture3D", 0x0810_28d0);
pub const SI_ED_MODEL_RELAX_LTEXTURE3D: Routine = Routine::new("SI_edModelRelaxLTexture3D", 0x0810_1098);
pub const SI_ED_MODEL_RELAX_GTEXTURE3D: Routine = Routine::new("SI_edModelRelaxGTexture3D", 0x0810_0ef0);
pub const SI_TEXTURE3D_STILL_USED: Routine = Routine::new("SI_texture3DStillUsed", 0x082b_56c0);
pub const SI_TXT2D_COPY: Routine = Routine::new("SI_txt2dCopy", 0x082a_dfbc);
pub const SI_TXT2D_SET_WRAP: Routine = Routine::new("SI_txt2dSetWrap", 0x082a_d9f8);
pub const SI_TXT3D_COPY: Routine = Routine::new("SI_txt3dCopy", 0x082b_4008);
pub const SI_GRAFLIST_ADD_DATA: Routine = Routine::new("SI_graflistAddData", 0x0832_d240);
pub const SI_COPY_T2D_SHADER: Routine = Routine::new("SI_copyT2DShader", 0x082a_6864);
pub const SI_COPY_T3D_SHADER: Routine = Routine::new("SI_copyT3DShader", 0x082a_68cc);
pub const SI_ED_MODEL_SET_LTEXTURE2D: Routine = Routine::new("SI_edModelSetLTexture2D", 0x0810_126c);
pub const SI_ED_MODEL_SET_LTEXTURE3D: Routine = Routine::new("SI_edModelSetLTexture3D", 0x0810_0f88);
pub const SI_ED_MODEL_SET_GTEXTURE2D: Routine = Routine::new("SI_edModelSetGTexture2D", 0x0810_1130);
pub const SI_ED_MODEL_SET_GTEXTURE3D: Routine = Routine::new("SI_edModelSetGTexture3D", 0x0810_0e4c);
pub const SI_TEXTURE2D_LOAD_FCURVE_ALL: Routine = Routine::new("SI_texture2DLoadFcurveAll", 0x0822_d890);
pub const SI_TEXTURE3D_LOAD_FCURVE_ALL: Routine = Routine::new("SI_texture3DLoadFcurveAll", 0x0822_d914);
pub const SI_TEXTURE3D_LASSOCIATE: Routine = Routine::new("si_texture3Dlassociate", 0x082b_5524);
pub const SI_TEXTURE3D_GASSOCIATE: Routine = Routine::new("si_texture3Dgassociate", 0x082b_55e4);
pub const SI_ED_MODEL_SET_MATERIAL: Routine = Routine::new("SI_edModelSetMaterial", 0x0810_0c34);
pub const SI_MATERIAL_COPY: Routine = Routine::new("SI_materialCopy", 0x0828_4888);
pub const SI_COPY_MATERIAL_SHADERS: Routine = Routine::new("SI_copyMaterialShaders", 0x082a_6808);
pub const SI_MODEL_LOAD_FCURVE_MATERIAL: Routine = Routine::new("SI_modelLoadFcurveMaterial", 0x0822_d7f0);
pub const SI_MATERIAL_ASSOCIATE: Routine = Routine::new("SI_materialAssociate", 0x0828_5a20);
pub const SI_CURRENT_CURRENT_MAP: Routine = Routine::new("SI_currentCurrentMap", 0x0829_2130);
pub const SI_LIGHT_INITIALIZE: Routine = Routine::new("si_lightInitialize", 0x0828_19c8);
pub const SI_LINK_DEFAULT_MATERIAL: Routine = Routine::new("si_linkDefaultMaterial", 0x0828_205c);
pub const SI_INIT_RENDER_GPROPS: Routine = Routine::new("si_initRenderGprops", 0x082a_5920);
pub const SI_REN_CONTROL_INIT: Routine = Routine::new("SI_renControlInit", 0x0829_12e4);
pub const SI_MATTER_FADE_INIT: Routine = Routine::new("SI_matterFadeInit", 0x0829_1bd4);
pub const FOG_INIT: Routine = Routine::new("fogInit", 0x0829_6200);
pub const SI_CHUNK_ALLOCATE: Routine = Routine::new("SI_chunkAllocate", 0x0857_ca00);
pub const PLT_FOPEN: Routine = Routine::new("plt:fopen", 0x080c_4e18);
pub const PLT_FSCANF: Routine = Routine::new("plt:fscanf", 0x080c_4058);
pub const PLT_FCLOSE: Routine = Routine::new("plt:fclose", 0x080c_4958);

/// The graph list of the current process: `SI_grafLists + SI_procData[1] * 0x24`.
fn graf_list(m: &Mem) -> u32 {
    let pd = m.u32(SI_PROC_DATA);
    m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(pd.wrapping_add(4)).wrapping_mul(0x24))
}

/// The dictionary of the current process: `SI_elemDicts + SI_procData[0] * 0x70`.
fn proc_dict(m: &Mem) -> u32 {
    let pd = m.u32(SI_PROC_DATA);
    m.u32(pd).wrapping_mul(8).wrapping_sub(m.u32(pd)).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS))
}

fn or_byte(m: &mut Mem, a: u32, bits: u8) {
    let v = m.u8(a) | bits;
    m.set_u8(a, v);
}

/// [soft 0x0828d790 matterCopySchemInfo]: the texture's schematic position, copied from the source texture into the destination
/// texture relative to the owning materials. `src_item`/`dst_item` (materials or models, `item_cid` their chapter) and
/// `src_tex`/`dst_tex` (textures, `tex_cid` their chapter) are looked up in the dictionary of the process; when all four
/// have an element: the destination's word +0x5c, short +0x2a and word +0x18 are those of the source (the destination's +0x18
/// is raised to the destination item's when smaller), and the destination's two floats at +0xc and +0x10 are the source's
/// minus the source item's plus the destination item's (the difference is held in the register, the sum stored once).
pub fn matter_copy_schem_info(m: &mut Mem, o: &mut dyn Outside, src_item: u32, dst_item: u32, item_cid: u32, src_tex: u32, dst_tex: u32, tex_cid: u32) {
    let cell = m.local(4);
    let dict = proc_dict(m);
    let e_src_tex = o.call(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict, tex_cid, src_tex, cell]).eax;
    let dict = proc_dict(m);
    let e_dst_tex = o.call(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict, tex_cid, dst_tex, cell]).eax;
    let dict = proc_dict(m);
    let e_src_item = o.call(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict, item_cid, src_item, cell]).eax;
    let dict = proc_dict(m);
    let e_dst_item = o.call(m, SI_ELEMDICT_GET_ELEM_FROM_DATA, &[dict, item_cid, dst_item, cell]).eax;
    if e_src_tex != 0 && e_dst_tex != 0 && e_src_item != 0 && e_dst_item != 0 {
        let (s, d, si, di) = (e_src_tex, e_dst_tex, e_src_item, e_dst_item);
        let v = m.u32(s.wrapping_add(0x5c));
        m.set_u32(d.wrapping_add(0x5c), v);
        let v = m.u16(s.wrapping_add(0x2a));
        m.set_u16(d.wrapping_add(0x2a), v);
        let v = m.u32(s.wrapping_add(0x18));
        m.set_u32(d.wrapping_add(0x18), v);
        let lim = m.u32(di.wrapping_add(0x18)) as i32;
        if (v as i32) < lim {
            m.set_u32(d.wrapping_add(0x18), lim as u32);
        }
        // fld src.c; fsub srcItem.c; fst dst.c; fld src.10; fsub srcItem.10; fst dst.10; fxch; fadd dstItem.c; fstp dst.c; fadd dstItem.10; fstp dst.10
        let d1 = x(m.f32(s.wrapping_add(0xc))) - m.f32(si.wrapping_add(0xc));
        m.set_f32(d.wrapping_add(0xc), d1.to_f32());
        let d2 = x(m.f32(s.wrapping_add(0x10))) - m.f32(si.wrapping_add(0x10));
        m.set_f32(d.wrapping_add(0x10), d2.to_f32());
        let r1 = d1 + m.f32(di.wrapping_add(0xc));
        m.set_f32(d.wrapping_add(0xc), r1.to_f32());
        let r2 = d2 + m.f32(di.wrapping_add(0x10));
        m.set_f32(d.wrapping_add(0x10), r2.to_f32());
    }
    m.release(4);
}

/// [soft 0x08295084 textureCopyDisableUVOnMesh]: a mesh model (kind word +0x30 is 4) whose texture's projection short (+0x2a)
/// is 4 gets projection 6.
pub fn texture_copy_disable_uv_on_mesh(m: &mut Mem, model: u32, tex: u32) {
    if m.u32(model.wrapping_add(0x30)) == 4 && m.u16(tex.wrapping_add(0x2a)) == 4 {
        m.set_u16(tex.wrapping_add(0x2a), 6);
    }
}

/// [soft 0x08294fac materialAddDefault]: a model without a material array gets one with a copy of `SI_defaultMaterial` (0xdcc bytes,
/// linked in front of the graph list's materials, count 1, current 0, added to the graph list as kind 0xe and set on the model
/// by `SI_edModelSetMaterial`); then its flag bytes +0xa and +0x3fa get bit 0x40, as in every case where the model had no
/// array. (The original writes the new record's address through the array block even when that block could not be made, which
/// faults; the port does the same on the image.)
pub fn material_add_default(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    if m.u32(model.wrapping_add(0x20c)) != 0 {
        return;
    }
    let mat = m.si_malloc(0xdcc);
    let arr = m.si_malloc(4);
    m.set_u32(model.wrapping_add(0x20c), arr);
    if mat != 0 {
        si_mem::clib::memcpy(m, mat, SI_DEFAULT_MATERIAL, 0xdcc);
        let pd = m.u32(SI_PROC_DATA);
        let gl = m.u32(SI_GRAF_LISTS);
        let idx = m.u32(pd.wrapping_add(4));
        let head = m.u32(gl.wrapping_add(idx.wrapping_mul(0x24)).wrapping_add(0x14));
        m.set_u32(mat.wrapping_add(0x6e4), head);
        let idx = m.u32(pd.wrapping_add(4));
        m.set_u32(gl.wrapping_add(idx.wrapping_mul(0x24)).wrapping_add(0x14), mat);
        let arr = m.u32(model.wrapping_add(0x20c));
        m.set_u32(arr, mat);
        m.set_u16(model.wrapping_add(0x206), 1);
        m.set_u16(model.wrapping_add(0x208), 0);
        let list = graf_list(m);
        o.call(m, SI_GRAFLIST_ADD_DATA, &[list, mat, 0xe, 0]);
        o.call(m, SI_ED_MODEL_SET_MATERIAL, &[model, mat]);
    }
    or_byte(m, model.wrapping_add(0xa), 0x40);
    or_byte(m, model.wrapping_add(0x3fa), 0x40);
}

/// Disposes the 2D textures chained from `first` (next at +0x16c).
fn dispose_2d_chain(m: &mut Mem, o: &mut dyn Outside, first: u32) {
    let mut t = first;
    while t != 0 {
        let next = m.u32(t.wrapping_add(0x16c));
        o.call(m, SI_ED_DISPOSE_TEXTURE2D, &[t]);
        t = next;
    }
}

/// [soft 0x082950a4 txtCopyFromSetupLcl]: copies the local textures of the setup material `src_mat` to `dst_mat` of `model`
/// (nothing when they are the same record). With the local-2D switch (`G_L2D`) or `all` set, the destination's 2D textures
/// are disposed and replaced by copies of the source's; with the local-3D switch (`G_L3D`) or `all` set, likewise for the 3D
/// texture; otherwise, with the 3D-association switch set and no 3D texture on the destination, the destination is
/// associated with the source's 3D texture. Each copy gets its wrap set, is added to the graph list (kind 0xf, 3D 0x10),
/// takes the source's shader, is set on the material, and has its schematic position copied; the f-curves are copied when
/// the matching switch or `all` is set. `uv` makes `textureCopyDisableUVOnMesh` run on each 2D copy.
pub fn txt_copy_from_setup_lcl(m: &mut Mem, o: &mut dyn Outside, model: u32, src_mat: u32, dst_mat: u32, all: i16, uv: i16) {
    if src_mat == dst_mat {
        return;
    }
    let all_on = all != 0;
    if m.u16(G_L2D) != 0 || all_on {
        let first = m.u32(dst_mat.wrapping_add(0x6ec));
        dispose_2d_chain(m, o, first);
        m.set_u32(dst_mat.wrapping_add(0x6ec), 0);
        m.set_u32(dst_mat.wrapping_add(0x6f0), 0);
    }
    if m.u16(G_L3D) != 0 || all_on {
        let t3 = m.u32(dst_mat.wrapping_add(0x6fc));
        if t3 != 0 {
            o.call(m, SI_ED_MODEL_RELAX_LTEXTURE3D, &[dst_mat, t3]);
            let t3 = m.u32(dst_mat.wrapping_add(0x6fc));
            if o.call(m, SI_TEXTURE3D_STILL_USED, &[t3]).eax as u16 == 0 {
                let t3 = m.u32(dst_mat.wrapping_add(0x6fc));
                o.call(m, SI_ED_DISPOSE_TEXTURE3D, &[t3]);
            }
            m.set_u32(dst_mat.wrapping_add(0x6fc), 0);
        }
    }
    if m.u16(G_L2D) != 0 || all_on {
        let mut s = m.u32(src_mat.wrapping_add(0x6ec));
        let mut tail = 0u32;
        while s != 0 {
            if m.u32(dst_mat.wrapping_add(0x6ec)) == 0 {
                let t = o.call(m, SI_TXT2D_COPY, &[s]).eax;
                m.set_u32(dst_mat.wrapping_add(0x6ec), t);
                if uv != 0 {
                    texture_copy_disable_uv_on_mesh(m, model, t);
                }
                let cur = m.u32(dst_mat.wrapping_add(0x6ec));
                o.call(m, SI_TXT2D_SET_WRAP, &[model, cur]);
                let list = graf_list(m);
                let t = m.u32(dst_mat.wrapping_add(0x6ec));
                o.call(m, SI_GRAFLIST_ADD_DATA, &[list, t, 0xf, 0]);
                let shader = m.u32(s.wrapping_add(0x198));
                o.call(m, SI_COPY_T2D_SHADER, &[m.u32(dst_mat.wrapping_add(0x6ec)), shader]);
                o.call(m, SI_ED_MODEL_SET_LTEXTURE2D, &[dst_mat, m.u32(dst_mat.wrapping_add(0x6ec))]);
                if m.u16(G_L2D_FCV) != 0 || all_on {
                    o.call(m, SI_TEXTURE2D_LOAD_FCURVE_ALL, &[m.u32(dst_mat.wrapping_add(0x6ec)), s]);
                }
                let first = m.u32(dst_mat.wrapping_add(0x6ec));
                m.set_u32(dst_mat.wrapping_add(0x6f0), first);
                tail = first;
                matter_copy_schem_info(m, o, src_mat, dst_mat, m.u32(SI_CID_MATERIALS), s, tail, m.u32(SI_CID_TEXTURES2D));
            } else {
                let t = o.call(m, SI_TXT2D_COPY, &[s]).eax;
                m.set_u32(tail.wrapping_add(0x16c), t);
                if uv != 0 {
                    texture_copy_disable_uv_on_mesh(m, model, t);
                }
                let nxt = m.u32(tail.wrapping_add(0x16c));
                o.call(m, SI_TXT2D_SET_WRAP, &[model, nxt]);
                let list = graf_list(m);
                o.call(m, SI_GRAFLIST_ADD_DATA, &[list, m.u32(tail.wrapping_add(0x16c)), 0xf, 0]);
                let shader = m.u32(s.wrapping_add(0x198));
                o.call(m, SI_COPY_T2D_SHADER, &[m.u32(tail.wrapping_add(0x16c)), shader]);
                o.call(m, SI_ED_MODEL_SET_LTEXTURE2D, &[dst_mat, m.u32(tail.wrapping_add(0x16c))]);
                let nxt = m.u32(tail.wrapping_add(0x16c));
                matter_copy_schem_info(m, o, src_mat, dst_mat, m.u32(SI_CID_MATERIALS), s, nxt, m.u32(SI_CID_TEXTURES2D));
                if m.u16(G_L2D_FCV) != 0 || all_on {
                    o.call(m, SI_TEXTURE2D_LOAD_FCURVE_ALL, &[m.u32(tail.wrapping_add(0x16c)), s]);
                }
                tail = m.u32(tail.wrapping_add(0x16c));
            }
            s = m.u32(s.wrapping_add(0x16c));
        }
    }
    if m.u16(G_L3D) != 0 || all_on {
        let s3 = m.u32(src_mat.wrapping_add(0x6fc));
        if s3 != 0 {
            let t = o.call(m, SI_TXT3D_COPY, &[s3]).eax;
            m.set_u32(dst_mat.wrapping_add(0x6fc), t);
            let list = graf_list(m);
            o.call(m, SI_GRAFLIST_ADD_DATA, &[list, t, 0x10, 0]);
            let shader = m.u32(m.u32(src_mat.wrapping_add(0x6fc)).wrapping_add(0x14c));
            o.call(m, SI_COPY_T3D_SHADER, &[m.u32(dst_mat.wrapping_add(0x6fc)), shader]);
            o.call(m, SI_ED_MODEL_SET_LTEXTURE3D, &[dst_mat, m.u32(dst_mat.wrapping_add(0x6fc))]);
            let (s3, d3) = (m.u32(src_mat.wrapping_add(0x6fc)), m.u32(dst_mat.wrapping_add(0x6fc)));
            matter_copy_schem_info(m, o, src_mat, dst_mat, m.u32(SI_CID_MATERIALS), s3, d3, m.u32(SI_CID_TEXTURES3D));
            if m.u16(G_L3D_FCV) != 0 || all_on {
                let (s3, d3) = (m.u32(src_mat.wrapping_add(0x6fc)), m.u32(dst_mat.wrapping_add(0x6fc)));
                o.call(m, SI_TEXTURE3D_LOAD_FCURVE_ALL, &[d3, s3]);
            }
        }
    } else if m.u16(G_L3D_ASSOC) != 0 && m.u32(dst_mat.wrapping_add(0x6fc)) == 0 {
        let s3 = m.u32(src_mat.wrapping_add(0x6fc));
        o.call(m, SI_TEXTURE3D_LASSOCIATE, &[dst_mat, s3]);
    }
}

/// [soft 0x08294dac SI_materialCopyShareSetup]: gives `dst` the materials of the setup model `src`. With `SI_glbDuplic` set
/// and `src` holding materials (count short +0x206 not 0): the count and current index are copied, an array of that many
/// pointers is made (`SI_calloc`), and every material is copied (`SI_materialCopy`), added to the graph list as kind 0xe, given
/// the source's shaders, set on `dst` and has its schematic position copied; with the f-curve switch (`G_FCV_MAT`) set the
/// f-curves are copied by running `SI_modelLoadFcurveMaterial` once per material with both current indices stepped from 0
/// (and put back). Without `SI_glbDuplic`, with the association switch (`G_ASSOC_MAT`) set every material is associated
/// (`SI_materialAssociate(dst, material, 1, 1, 0)`) and the current index is copied.
pub fn si_material_copy_share_setup(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32) {
    if m.u16(SI_GLB_DUPLIC) != 0 {
        let cnt = m.u16(src.wrapping_add(0x206));
        if cnt == 0 {
            return;
        }
        m.set_u16(dst.wrapping_add(0x206), cnt);
        let cur = m.u16(src.wrapping_add(0x208));
        m.set_u16(dst.wrapping_add(0x208), cur);
        let n = m.i16(src.wrapping_add(0x206)) as i32 as u32;
        let arr = m.si_calloc(n, 4);
        m.set_u32(dst.wrapping_add(0x20c), arr);
        let mut i = 0i32;
        if m.i16(src.wrapping_add(0x206)) > 0 {
            loop {
                let sarr = m.u32(src.wrapping_add(0x20c));
                let smat = m.u32(sarr.wrapping_add((i as u32).wrapping_mul(4)));
                let copy = o.call(m, SI_MATERIAL_COPY, &[smat]).eax;
                let darr = m.u32(dst.wrapping_add(0x20c));
                m.set_u32(darr.wrapping_add((i as u32).wrapping_mul(4)), copy);
                let list = graf_list(m);
                let dmat = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add((i as u32).wrapping_mul(4)));
                o.call(m, SI_GRAFLIST_ADD_DATA, &[list, dmat, 0xe, 0]);
                let smat = m.u32(m.u32(src.wrapping_add(0x20c)).wrapping_add((i as u32).wrapping_mul(4)));
                let dmat = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add((i as u32).wrapping_mul(4)));
                o.call(m, SI_COPY_MATERIAL_SHADERS, &[dmat, smat]);
                let dmat = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add((i as u32).wrapping_mul(4)));
                o.call(m, SI_ED_MODEL_SET_MATERIAL, &[dst, dmat]);
                let smat = m.u32(m.u32(src.wrapping_add(0x20c)).wrapping_add((i as u32).wrapping_mul(4)));
                let dmat = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add((i as u32).wrapping_mul(4)));
                let (mc, mt) = (m.u32(SI_CID_MODELS), m.u32(SI_CID_MATERIALS));
                matter_copy_schem_info(m, o, src, dst, mc, smat, dmat, mt);
                i += 1;
                if i >= m.i16(src.wrapping_add(0x206)) as i32 {
                    break;
                }
            }
        }
        if m.u16(G_FCV_MAT) == 0 {
            return;
        }
        let save_src = m.i16(src.wrapping_add(0x208)) as i32;
        let save_dst = m.i16(dst.wrapping_add(0x208)) as i32;
        m.set_u16(src.wrapping_add(0x208), 0);
        m.set_u16(dst.wrapping_add(0x208), 0);
        let mut i = 0i32;
        if m.i16(src.wrapping_add(0x206)) > 0 {
            loop {
                o.call(m, SI_MODEL_LOAD_FCURVE_MATERIAL, &[dst, src]);
                i += 1;
                let v = m.u16(src.wrapping_add(0x208)).wrapping_add(1);
                m.set_u16(src.wrapping_add(0x208), v);
                let v = m.u16(dst.wrapping_add(0x208)).wrapping_add(1);
                m.set_u16(dst.wrapping_add(0x208), v);
                if i >= m.i16(src.wrapping_add(0x206)) as i32 {
                    break;
                }
            }
        }
        m.set_u16(src.wrapping_add(0x208), save_src as u16);
        m.set_u16(dst.wrapping_add(0x208), save_dst as u16);
    } else {
        if m.u16(G_ASSOC_MAT) == 0 {
            return;
        }
        let mut i = 0i32;
        if m.i16(src.wrapping_add(0x206)) > 0 {
            loop {
                let smat = m.u32(m.u32(src.wrapping_add(0x20c)).wrapping_add((i as u32).wrapping_mul(4)));
                o.call(m, SI_MATERIAL_ASSOCIATE, &[dst, smat, 1, 1, 0]);
                i += 1;
                if i >= m.i16(src.wrapping_add(0x206)) as i32 {
                    break;
                }
            }
        }
        let cur = m.u16(src.wrapping_add(0x208));
        m.set_u16(dst.wrapping_add(0x208), cur);
    }
}

/// [soft 0x082954c0 SI_txtCopyFromSetup]: gives the model `dst` the global textures and the materials' textures of the setup
/// model `src`. Nothing happens unless `src` has something to copy: a global 2D (+0x21c) or 3D (+0x224) texture, or, when its
/// current material index (short +0x208) is below its count (+0x206), a material with local textures; with a current index
/// not below the count only the globals count. Then `dst` gets a default material if it has none, and flag bit 0x80 on its
/// bytes +0xa and +0x3fa. With the global-2D switch (`G_G2D`) or `all` set its global 2D textures are replaced by copies of
/// the source's; with the global-3D switch (`G_G3D`) or `all` its global 3D texture likewise, otherwise it is associated with
/// the source's when `G_G3D_ASSOC` is set, `dst` has materials and no 3D texture. Finally, when a material was seen and
/// (`SI_glbDuplic` and a local texture switch is set, or `all` is set), the local textures of `dst`'s material(s) are copied
/// from `src`'s by `txtCopyFromSetupLcl` (the current one with `all`, else every one), and the current map is reset.
pub fn si_txt_copy_from_setup(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32, all: i16, uv: i16) {
    let mut mat = 0u32;
    let mut found = false;
    let cnt = m.i16(src.wrapping_add(0x206));
    let cur = m.i16(src.wrapping_add(0x208));
    let go = if cnt > 0 && cur < cnt {
        let n = cnt as i32;
        let arr = m.u32(src.wrapping_add(0x20c));
        let glob2d = m.u32(src.wrapping_add(0x21c));
        let mut i = 0i32;
        while i < n {
            mat = m.u32(arr.wrapping_add((i as u32).wrapping_mul(4)));
            if glob2d != 0 || m.u32(src.wrapping_add(0x224)) != 0 || m.u32(mat.wrapping_add(0x6ec)) != 0 || m.u32(mat.wrapping_add(0x6fc)) != 0 {
                found = true;
            }
            i += 1;
        }
        found
    } else {
        mat = 0;
        m.u32(src.wrapping_add(0x21c)) != 0 || m.u32(src.wrapping_add(0x224)) != 0
    };
    if !go {
        return;
    }
    let all_on = all != 0;
    material_add_default(m, o, dst);
    or_byte(m, dst.wrapping_add(0xa), 0x80);
    or_byte(m, dst.wrapping_add(0x3fa), 0x80);
    if m.u16(G_G2D) != 0 || all_on {
        let first = m.u32(dst.wrapping_add(0x21c));
        dispose_2d_chain(m, o, first);
        m.set_u32(dst.wrapping_add(0x21c), 0);
        m.set_u32(dst.wrapping_add(0x218), 0);
    }
    if m.u16(G_G3D) != 0 || all_on {
        let t3 = m.u32(dst.wrapping_add(0x224));
        if t3 != 0 {
            o.call(m, SI_ED_MODEL_RELAX_GTEXTURE3D, &[dst, t3]);
            let t3 = m.u32(dst.wrapping_add(0x224));
            if o.call(m, SI_TEXTURE3D_STILL_USED, &[t3]).eax as u16 == 0 {
                let t3 = m.u32(dst.wrapping_add(0x224));
                o.call(m, SI_ED_DISPOSE_TEXTURE3D, &[t3]);
            }
            m.set_u32(dst.wrapping_add(0x224), 0);
        }
    }
    if m.u16(G_G2D) != 0 || all_on {
        let mut s = m.u32(src.wrapping_add(0x21c));
        let mut tail = 0u32;
        while s != 0 {
            let (mc, tc) = (m.u32(SI_CID_MODELS), m.u32(SI_CID_TEXTURES2D));
            if m.u32(dst.wrapping_add(0x21c)) == 0 {
                let t = o.call(m, SI_TXT2D_COPY, &[s]).eax;
                m.set_u32(dst.wrapping_add(0x21c), t);
                if uv != 0 {
                    texture_copy_disable_uv_on_mesh(m, dst, t);
                }
                let cur = m.u32(dst.wrapping_add(0x21c));
                o.call(m, SI_TXT2D_SET_WRAP, &[dst, cur]);
                let cur = m.u32(dst.wrapping_add(0x21c));
                m.set_u32(dst.wrapping_add(0x218), cur);
                tail = cur;
                let list = graf_list(m);
                o.call(m, SI_GRAFLIST_ADD_DATA, &[list, tail, 0xf, 0]);
                let shader = m.u32(s.wrapping_add(0x198));
                let cur = m.u32(dst.wrapping_add(0x21c));
                o.call(m, SI_COPY_T2D_SHADER, &[cur, shader]);
                let cur = m.u32(dst.wrapping_add(0x21c));
                o.call(m, SI_ED_MODEL_SET_GTEXTURE2D, &[dst, cur]);
                matter_copy_schem_info(m, o, src, dst, mc, s, tail, tc);
                if m.u16(G_G2D_FCV) != 0 || all_on {
                    let cur = m.u32(dst.wrapping_add(0x21c));
                    o.call(m, SI_TEXTURE2D_LOAD_FCURVE_ALL, &[cur, s]);
                }
            } else {
                let t = o.call(m, SI_TXT2D_COPY, &[s]).eax;
                m.set_u32(tail.wrapping_add(0x16c), t);
                if uv != 0 {
                    texture_copy_disable_uv_on_mesh(m, dst, t);
                }
                let nxt = m.u32(tail.wrapping_add(0x16c));
                o.call(m, SI_TXT2D_SET_WRAP, &[dst, nxt]);
                let list = graf_list(m);
                let nxt = m.u32(tail.wrapping_add(0x16c));
                o.call(m, SI_GRAFLIST_ADD_DATA, &[list, nxt, 0xf, 0]);
                let shader = m.u32(s.wrapping_add(0x198));
                let nxt = m.u32(tail.wrapping_add(0x16c));
                o.call(m, SI_COPY_T2D_SHADER, &[nxt, shader]);
                let nxt = m.u32(tail.wrapping_add(0x16c));
                o.call(m, SI_ED_MODEL_SET_GTEXTURE2D, &[dst, nxt]);
                let nxt = m.u32(tail.wrapping_add(0x16c));
                matter_copy_schem_info(m, o, src, dst, mc, s, nxt, tc);
                if m.u16(G_G2D_FCV) != 0 || all_on {
                    let nxt = m.u32(tail.wrapping_add(0x16c));
                    o.call(m, SI_TEXTURE2D_LOAD_FCURVE_ALL, &[nxt, s]);
                }
                tail = m.u32(tail.wrapping_add(0x16c));
            }
            s = m.u32(s.wrapping_add(0x16c));
        }
    }
    if m.u16(G_G3D) != 0 || all_on {
        let s3 = m.u32(src.wrapping_add(0x224));
        if s3 != 0 {
            let t = o.call(m, SI_TXT3D_COPY, &[s3]).eax;
            m.set_u32(dst.wrapping_add(0x224), t);
            let list = graf_list(m);
            o.call(m, SI_GRAFLIST_ADD_DATA, &[list, t, 0x10, 0]);
            let shader = m.u32(m.u32(src.wrapping_add(0x224)).wrapping_add(0x14c));
            let d3 = m.u32(dst.wrapping_add(0x224));
            o.call(m, SI_COPY_T3D_SHADER, &[d3, shader]);
            let d3 = m.u32(dst.wrapping_add(0x224));
            o.call(m, SI_ED_MODEL_SET_GTEXTURE3D, &[dst, d3]);
            let (s3, d3) = (m.u32(src.wrapping_add(0x224)), m.u32(dst.wrapping_add(0x224)));
            let (mc, tc) = (m.u32(SI_CID_MODELS), m.u32(SI_CID_TEXTURES3D));
            matter_copy_schem_info(m, o, src, dst, mc, s3, d3, tc);
            if m.u16(G_G3D_FCV) != 0 || all_on {
                let (s3, d3) = (m.u32(src.wrapping_add(0x224)), m.u32(dst.wrapping_add(0x224)));
                o.call(m, SI_TEXTURE3D_LOAD_FCURVE_ALL, &[d3, s3]);
            }
        }
    } else if m.u16(G_G3D_ASSOC) != 0 && m.i16(dst.wrapping_add(0x206)) > 0 && m.u32(dst.wrapping_add(0x224)) == 0 {
        let s3 = m.u32(src.wrapping_add(0x224));
        o.call(m, SI_TEXTURE3D_GASSOCIATE, &[dst, s3]);
    }
    if mat == 0 {
        return;
    }
    let any_local = m.u16(G_L2D) != 0 || m.u32(G_L3D) != 0;
    if !((m.u16(SI_GLB_DUPLIC) != 0 && any_local) || all_on) {
        return;
    }
    let dcnt = m.i16(dst.wrapping_add(0x206));
    if !(dcnt > 0 && m.i16(dst.wrapping_add(0x208)) < dcnt) {
        return;
    }
    if all_on {
        let si = m.i16(src.wrapping_add(0x208)) as i32 as u32;
        let smat = m.u32(m.u32(src.wrapping_add(0x20c)).wrapping_add(si.wrapping_mul(4)));
        let di = m.i16(dst.wrapping_add(0x208)) as i32 as u32;
        let dmat = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add(di.wrapping_mul(4)));
        txt_copy_from_setup_lcl(m, o, dst, smat, dmat, all, uv);
    } else {
        let mut i = 0i32;
        if m.i16(src.wrapping_add(0x206)) > 0 {
            loop {
                let smat = m.u32(m.u32(src.wrapping_add(0x20c)).wrapping_add((i as u32).wrapping_mul(4)));
                let dmat = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add((i as u32).wrapping_mul(4)));
                txt_copy_from_setup_lcl(m, o, dst, smat, dmat, all, uv);
                i += 1;
                if i >= m.i16(src.wrapping_add(0x206)) as i32 {
                    break;
                }
            }
        }
    }
    let shw = m.i16(SI_SHW_TXT) as i32 as u32;
    o.call(m, SI_CURRENT_CURRENT_MAP, &[shw, 0]);
}


/// `SI_incrust` (shorts: +0 flag, +2 mode, +8 the table of 0x680 shorts, 0xcfe bytes on to 0x879d908), its font name pointer
/// at +4, `SI_resdir`, `SI_softFont`.
pub const SI_INCRUST: u32 = 0x0879_cc00;
pub const SI_INCRUST_END: u32 = 0x0879_d908;
pub const SI_RESDIR: u32 = 0x0878_1628;
pub const SI_SOFT_FONT: u32 = 0x0867_94c8;
pub const SI_REN_CHUNK: u32 = 0x0869_bc84;

/// [soft 0x082964b8 incrustInit]: the picture-insert table. Flag 0, mode 3, the font-file name pointer a copy of the text at
/// 0x086a17f8, the end cell 0x879d908 cleared; then the file `SI_resdir` + `SI_softFont` is opened (mode text at 0x086a180a)
/// and 0x680 numbers (format at 0x086a180c) are read into the table of shorts from 0x879cc08, the loop running on whether
/// or not a read succeeded; the file is closed. The path is built in a 0x400-byte buffer without a length check, as the
/// original's `sprintf` does (a null pointer prints as `(null)`).
pub fn incrust_init(m: &mut Mem, o: &mut dyn Outside) {
    m.set_u16(SI_INCRUST, 0);
    m.set_u16(SI_INCRUST + 2, 3);
    let name = si_mem::clib::strdup(m, 0x086a_17f8);
    m.set_u32(SI_INCRUST + 4, name);
    let font = m.u32(SI_SOFT_FONT);
    m.set_u16(SI_INCRUST_END, 0);
    let dir = m.u32(SI_RESDIR);
    let buf = m.local(0x400);
    let mut path: Vec<u8> = if dir == 0 { b"(null)".to_vec() } else { m.cstr(dir) };
    path.extend(if font == 0 { b"(null)".to_vec() } else { m.cstr(font) });
    m.put_cstr(buf, &path);
    let fp = o.call(m, PLT_FOPEN, &[buf, 0x086a_180a]).eax;
    if fp != 0 {
        let mut at = SI_INCRUST + 8;
        let mut k = 0x67f_i32;
        while k >= 0 {
            o.call(m, PLT_FSCANF, &[fp, 0x086a_180c, at]);
            at = at.wrapping_add(2);
            k -= 1;
        }
        o.call(m, PLT_FCLOSE, &[fp]);
    }
    m.release(0x400);
}

/// [soft 0x08291bec SI_matterInit]: the matter module's start-up. `si_lightInitialize(0x8781e00, 1)`; the three floats at
/// 0x8781e34, 0x8781e38 and 0x8781e3c are set to the float constant at 0x086551e8 (100.0); then `si_linkDefaultMaterial()`,
/// `si_initRenderGprops(0x8782120)`, `SI_renControlInit(0x8781800)`, `SI_matterFadeInit()`, `fogInit()`, `incrustInit()`, and
/// `SI_renChunk` (0x869bc84) = `SI_chunkAllocate(200, 5000000)`.
pub fn si_matter_init(m: &mut Mem, o: &mut dyn Outside) {
    o.call(m, SI_LIGHT_INITIALIZE, &[0x0878_1e00, 1]);
    // the float constant at 0x086551e8 is 100.0 (the image of the original is not part of the port's memory)
    let v = 100.0f32;
    m.set_f32(0x0878_1e34, v);
    m.set_f32(0x0878_1e38, v);
    m.set_f32(0x0878_1e3c, v);
    o.call(m, SI_LINK_DEFAULT_MATERIAL, &[]);
    o.call(m, SI_INIT_RENDER_GPROPS, &[0x0878_2120]);
    o.call(m, SI_REN_CONTROL_INIT, &[0x0878_1800]);
    o.call(m, SI_MATTER_FADE_INIT, &[]);
    o.call(m, FOG_INIT, &[]);
    incrust_init(m, o);
    let chunk = o.call(m, SI_CHUNK_ALLOCATE, &[0xc8, 0x4c4b40]).eax;
    m.set_u32(SI_REN_CHUNK, chunk);
}
