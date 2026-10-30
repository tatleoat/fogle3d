// SPDX-License-Identifier: MIT
//! The scene-load fix-up passes of `db2scn.c`, `dbloadsave.c` and `scn2db.c` that "Get Scene"
//! runs after the elements are read: the pointers of the user-defined-function (UDF)
//! records tied from the dictionary, the weight-curve names, the lens-interest objects, the
//! load-optimisation switch, the texture-domain renumbering, the custom-relation icon swap,
//! the patch end-point conversion and the model internals handed to the editor (finding
//! 0018, section 29).
//!
//! Every routine works on the memory image; callees outside the file (the dictionary, the
//! editor, the UDF store) are called by address through [`ByAddr`], so the same routine
//! runs in the program (the dispatcher routes the address to the port) and against the
//! emulated original (`re/tools/loadfix1`).
//!
//! The counters of the licence scheme (`SI_secureLicAlloc`) the allocations bump are not
//! reproduced; the licence code is neither called nor ported.

use crate::edtools::{
    ByAddr, SI_DB_FORMAT_FULL_NAME, SI_DB_LIST_NODE_ALLOCATE, SI_ELEMDICT_ADD_ELEM,
    SI_ELEMDICT_ADD_RELATION, SI_ELEMDICT_FIND_ELEM_BY_DATA, SI_ELEMDICT_GET_ELEM,
    SI_ELEMDICT_UPDATE_HIER_INFO,
};
use si_mem::Mem;
use si_x87::x;

/// `SI_dbListNodeCopyInfo (0x08131ba8)`: (dictionary, source node, new node).
pub const SI_DB_LIST_NODE_COPY_INFO: u32 = 0x0813_1ba8;
/// `SI_findGlData (0x0832ee68)`: (graph list, object, kind); answers the entry or 0.
pub const SI_FIND_GL_DATA: u32 = 0x0832_ee68;
/// `SI_elemdictFindRelatedElem (0x0813508c)`: (dictionary, element, 1, 3, relation id,
/// chapter, &out, &out); answers the related element or 0.
pub const SI_ELEMDICT_FIND_RELATED_ELEM: u32 = 0x0813_508c;
/// `SI_elemdictSetDeleteElem (0x08135ad4)`: (dictionary, chapter, element, index, -1, 0, 1).
pub const SI_ELEMDICT_SET_DELETE_ELEM: u32 = 0x0813_5ad4;
/// `SI_udfDispose (0x0854c3c0)`: (record).
pub const SI_UDF_DISPOSE: u32 = 0x0854_c3c0;
/// `SI_udfInitializeFromDummy (0x08544330)`: (new record, old record).
pub const SI_UDF_INITIALIZE_FROM_DUMMY: u32 = 0x0854_4330;
/// `SI_udflistCompact (0x0854bf94)`: (&array, &count).
pub const SI_UDFLIST_COMPACT: u32 = 0x0854_bf94;
/// `SI_edGetElemFromScene (0x080fd620)`: (&index); answers the scene's element.
pub const SI_ED_GET_ELEM_FROM_SCENE: u32 = 0x080f_d620;
/// `SI_edFindFcv (0x080edb20)` / `SI_edFindNextFcv (0x080edb74)`: (element, a, b).
pub const SI_ED_FIND_FCV: u32 = 0x080e_db20;
pub const SI_ED_FIND_NEXT_FCV: u32 = 0x080e_db74;
/// `SI_fcvGetName (0x0810f678)`: (curve); answers the name.
pub const SI_FCV_GET_NAME: u32 = 0x0810_f678;
/// `SI_edAddCluster (0x08103510)`: (model, cluster).
pub const SI_ED_ADD_CLUSTER: u32 = 0x0810_3510;
/// `SI_edElementSetDynamics (0x0810173c)`: (model, dynamics owner).
pub const SI_ED_ELEMENT_SET_DYNAMICS: u32 = 0x0810_173c;
/// `SI_dbConvertModelMatTxt2DB (0x08108490)`: (model, force, 0, 0) (ported in `matconv`).
pub const SI_DB_CONVERT_MODEL_MAT_TXT2DB: u32 = 0x0810_8490;
/// `SI_dynaLinkRootToGravity (0x084a10c4)`: (model).
pub const SI_DYNA_LINK_ROOT_TO_GRAVITY: u32 = 0x084a_10c4;
/// `SI_edAddLattice (0x0810383c)`: (model, 0 or 1).
pub const SI_ED_ADD_LATTICE: u32 = 0x0810_383c;
/// `SI_edGetElemFromModel (0x080fcf20)`: (model, &index).
pub const SI_ED_GET_ELEM_FROM_MODEL: u32 = 0x080f_cf20;
/// `SI_edCheckForUneditable (0x08107a44)`: (element, chapter); answers a 16-bit flag.
pub const SI_ED_CHECK_FOR_UNEDITABLE: u32 = 0x0810_7a44;
/// `SI_edSetUneditable (0x081074c0)`: (dictionary, element).
pub const SI_ED_SET_UNEDITABLE: u32 = 0x0810_74c0;
/// `getenv (0x080c3bb8)`.
pub const GETENV: u32 = 0x080c_3bb8;

const CID: u32 = 0x0878_1660;
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_ELEM_DICTS: u32 = 0x0867_931c;
const SI_GRAF_LISTS: u32 = 0x0867_9498;
const SI_RD: u32 = 0x0878_15f0;
/// `workStr` (0x087855a0), the shared scratch string of `db2scn.c`.
const WORK_STR: u32 = 0x0878_55a0;

fn cid(m: &Mem, off: u32) -> u32 {
    m.u32(CID + off)
}

/// The current dictionary, `SI_elemDicts + *SI_procData * 0x70`.
fn cur_dict(m: &Mem) -> u32 {
    m.u32(SI_ELEM_DICTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA)).wrapping_mul(0x70))
}

/// The chapter record, `dict->chapters + chapter * 0x40` (no test of the index).
fn chapter_at(m: &Mem, dict: u32, chapter: u32) -> u32 {
    m.u32(dict + 4).wrapping_add(chapter << 6)
}

/// Element `i` of a chapter (0x84 bytes each).
fn elem_at(m: &Mem, chap: u32, i: u32) -> u32 {
    m.u32(chap + 8).wrapping_add(i.wrapping_mul(0x84))
}

/// Ties one UDF slot `*slot` to the record of the related element
/// [soft 0x0810a488 updateUdfFromElem]: with an element, its object; the old record (if any)
/// takes the new one's settings from the dummy and is disposed; without one the old record
/// is disposed and the slot cleared.
pub fn update_udf_from_elem<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, elem: u32, slot: u32) {
    let mut p = 0;
    if elem != 0 {
        p = m.u32(elem + 4);
        let old = m.u32(slot);
        if old == 0 {
            m.set_u32(slot, p);
            return;
        }
        if p != 0 {
            o.call_addr(m, SI_UDF_INITIALIZE_FROM_DUMMY, &[p, old]);
        }
    }
    let old = m.u32(slot);
    if old != 0 {
        o.call_addr(m, SI_UDF_DISPOSE, &[old]);
    }
    m.set_u32(slot, p);
}

/// The related shader element of `e` in `chapter` (relation 0x226), two scratch words
/// on the stack.
fn find_related<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32, e: u32, chapter: u32) -> u32 {
    let l = m.local(8);
    let r = o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, e, 1, 3, 0x226, chapter, l, l + 4]);
    m.release(8);
    r
}

fn set_delete<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32, chapter: u32, e: u32, idx: u32) {
    o.call_addr(m, SI_ELEMDICT_SET_DELETE_ELEM, &[dict, chapter, e, idx, 0xffff_ffff, 0, 1]);
}

/// The shader records of the materials [soft 0x0810a4d4 connectUdfToMaterials]: for each
/// new material, its material, volume, shadow and photon shader.
pub fn connect_udf_to_materials<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32) {
    let chap = chapter_at(m, dict, cid(m, 0x1c));
    let n = m.i32(chap + 4);
    let mut i = 0;
    while i < n {
        let e = elem_at(m, chap, i as u32);
        i += 1;
        let obj = m.u32(e + 4);
        if m.i16(e + 0x46) != 1 || m.i16(e + 0x48) != 0 {
            continue;
        }
        for (c, off) in [(0x30u32, 0xdb0u32), (0x3c, 0xdb4), (0x40, 0xdb8), (0x88, 0xdbc)] {
            let ch = cid(m, c);
            let p = find_related(o, m, dict, e, ch);
            update_udf_from_elem(o, m, p, obj.wrapping_add(off));
            if p != 0 && m.u32(p + 4) == 0 {
                set_delete(o, m, dict, ch, p, 0);
            }
        }
    }
}

/// A scene-wide list of UDF records (`count` at `count_at`, array pointer at `arr_at`)
/// matched by name to the new records of `chapter` [soft 0x0810a6e8 connectUdfToSceneElems].
pub fn connect_udf_to_scene_elems<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, count_at: u32, arr_at: u32, chapter: u32, dict: u32) {
    let arr = m.u32(arr_at);
    let n = m.i32(count_at);
    let mut marks = 0;
    if n > 0 {
        marks = m.si_calloc(n as u32, 2);
        for k in 0..n as u32 {
            m.set_u16(marks + 2 * k, 0);
        }
    }
    let chap = chapter_at(m, dict, chapter);
    let cnt = m.i32(chap + 4);
    let mut i = 0;
    while i < cnt {
        let e = elem_at(m, chap, i as u32);
        if m.i16(e + 0x46) == 1 && m.i16(e + 0x48) == 0 {
            let udf = m.u32(e + 4);
            let mut found = false;
            if udf != 0 {
                for k in 0..n.max(0) as u32 {
                    let a = m.u32(m.u32(arr + 4 * k));
                    let b = m.u32(udf);
                    if si_mem::clib::strcmp(m, a, b) == 0 {
                        update_udf_from_elem(o, m, e, arr + 4 * k);
                        m.set_u16(marks + 2 * k, 1);
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                o.call_addr(m, SI_UDF_DISPOSE, &[udf]);
                set_delete(o, m, dict, chapter, e, i as u32);
            }
        }
        i += 1;
    }
    for k in 0..n.max(0) as u32 {
        if m.u16(marks + 2 * k) == 0 {
            let r = m.u32(arr + 4 * k);
            o.call_addr(m, SI_UDF_DISPOSE, &[r]);
            m.set_u32(arr + 4 * k, 0);
        }
    }
    o.call_addr(m, SI_UDFLIST_COMPACT, &[arr_at, count_at]);
    if marks != 0 {
        m.free(marks);
    }
}

/// The scene's render-control UDF lists and its volume shader
/// [soft 0x0810a894 connectUdfToRenControl].
pub fn connect_udf_to_ren_control<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32) {
    let l = m.local(4);
    let e = o.call_addr(m, SI_ED_GET_ELEM_FROM_SCENE, &[l]);
    m.release(4);
    if e == 0 || m.i16(e + 0x48) != 0 {
        return;
    }
    let (out, cam) = (cid(m, 0x4c), cid(m, 0x48));
    connect_udf_to_scene_elems(o, m, 0x0878_1be4, 0x0878_1bec, out, dict);
    connect_udf_to_scene_elems(o, m, 0x0878_1be8, 0x0878_1bf0, cam, dict);
    let vol = cid(m, 0x3c);
    let p = find_related(o, m, dict, e, vol);
    update_udf_from_elem(o, m, p, 0x0878_1c4c);
    if p != 0 && m.u32(p + 4) == 0 {
        set_delete(o, m, dict, vol, p, 0);
    }
}

/// The shader records of the lights, 2D and 3D textures
/// [soft 0x0810a950 connectUdfToChapterElems]; 0 for any other chapter.
pub fn connect_udf_to_chapter_elems<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32, chap_in: u32) -> u32 {
    let mut chapter = chap_in;
    let lights = cid(m, 0x18);
    let (t2d, t2d_sh) = (cid(m, 0x68), cid(m, 0x34));
    let (t3d, t3d_sh) = (cid(m, 0x6c), cid(m, 0x38));
    let (lig_sh, ph_sh, env_sh) = (cid(m, 0x44), cid(m, 0x8c), cid(m, 0x50));
    let shader;
    if chapter == lights {
        shader = lig_sh;
    } else if chapter == ph_sh {
        chapter = lights;
        shader = ph_sh;
    } else if chapter == t2d {
        shader = t2d_sh;
    } else if chapter == t3d {
        shader = t3d_sh;
    } else {
        return 0;
    }
    let chap = chapter_at(m, dict, chapter);
    let n = m.i32(chap + 4);
    let mut i = 0;
    while i < n {
        let e = elem_at(m, chap, i as u32);
        i += 1;
        if m.i16(e + 0x46) != 1 || m.i16(e + 0x48) != 0 {
            continue;
        }
        if chapter == lights && m.u32(e + 0x54) != 0 {
            continue;
        }
        let mut p = find_related(o, m, dict, e, shader);
        if p == 0 {
            if shader == t2d_sh {
                p = find_related(o, m, dict, e, env_sh);
            }
            if p == 0 {
                continue;
            }
        }
        let obj = m.u32(e + 4);
        // The original prints "Unknown chapter" for any other chapter and carries on with a
        // null slot; none is reachable once the chapter passed the choice above.
        let slot = if chapter == lights {
            obj + if shader == ph_sh { 0x17c } else { 0x178 }
        } else if chapter == t2d {
            obj + 0x198
        } else {
            obj + 0x14c
        };
        update_udf_from_elem(o, m, p, slot);
        if m.u32(p + 4) == 0 {
            // The shader chapter given to the search, also when the element came from the
            // environment-shader chapter.
            set_delete(o, m, dict, shader, p, 0);
        }
    }
    1
}

/// All UDF pointers of a loaded dictionary [soft 0x0810ab40 si_edUpdateUdfPointers].
pub fn ed_update_udf_pointers<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32, flag: i16) {
    connect_udf_to_materials(o, m, dict);
    for c in [0x18, 0x8c, 0x68, 0x6c] {
        let ch = cid(m, c);
        connect_udf_to_chapter_elems(o, m, dict, ch);
    }
    if flag != 0 {
        connect_udf_to_ren_control(o, m, dict);
    }
}

/// `sprintf(dst, "%s%d", s, n)`.
fn sprintf_s_d(m: &mut Mem, dst: u32, s: u32, n: i32) {
    let mut v = m.cstr(s);
    v.extend_from_slice(n.to_string().as_bytes());
    m.put_cstr(dst, &v);
}

/// The names of the weight curves of one element, numbered and tied to the animation
/// chapter's nodes [soft 0x0810b094 SI_updateWeightFcvNames]: (element, a, b) as for
/// `SI_edFindFcv`.
pub fn update_weight_fcv_names<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, elem: u32, a: u32, b: u32) {
    let mut i = 0i32;
    let mut f = o.call_addr(m, SI_ED_FIND_FCV, &[elem, a, b]);
    while f != 0 {
        let dict = cur_dict(m);
        let ani = cid(m, 0);
        let idx = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, ani, f]);
        let e2 = o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict, ani, idx]);
        let name = o.call_addr(m, SI_FCV_GET_NAME, &[f]);
        sprintf_s_d(m, WORK_STR, name, i);
        let node = m.u32(e2);
        let old = m.u32(node + 0x14);
        if old != 0 {
            m.free(old);
            m.set_u32(m.u32(e2) + 0x14, 0);
        }
        let dup = m.strdup(WORK_STR);
        let node = m.u32(e2);
        m.set_u32(node + 0x14, dup);
        let full = o.call_addr(m, SI_DB_FORMAT_FULL_NAME, &[m.u32(e2)]);
        let node = m.u32(e2);
        m.set_u32(node + 0x24, full);
        let enode = m.u32(elem);
        let ename = m.u32(enode + 0x14);
        if ename == 0 {
            // sprintf(name, curve name): the name is the format; a '%' in it is not
            // expanded here (the names of the attribute table have none).
            let nm = o.call_addr(m, SI_FCV_GET_NAME, &[f]);
            let v = m.cstr(nm);
            m.put_cstr(f + 0x2e, &v);
        } else {
            // sprintf(name, "%.8s.%.6s", element name, workStr)
            let mut v: Vec<u8> = m.cstr(ename).into_iter().take(8).collect();
            v.push(b'.');
            v.extend(m.cstr(WORK_STR).into_iter().take(6));
            m.put_cstr(f + 0x2e, &v);
        }
        i += 1;
        f = o.call_addr(m, SI_ED_FIND_NEXT_FCV, &[elem, a, b]);
    }
}

/// The weight-curve names of the shapes and the clusters
/// [soft 0x0810b1dc si_edUpdateWeightFcvNames].
pub fn ed_update_weight_fcv_names<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32) {
    for (c, a, b) in [(0x54u32, 0u32, 0xcau32), (0x08, 2, 0x2be)] {
        let chap = chapter_at(m, dict, cid(m, c));
        let n = m.i32(chap + 4);
        for i in 0..n.max(0) as u32 {
            let e = elem_at(m, chap, i);
            if m.i16(e + 0x48) == 0 && m.i16(e + 0x46) == 1 {
                update_weight_fcv_names(o, m, e, a, b);
            }
        }
    }
}

/// An object for the interest of a camera or a spot light
/// [soft 0x08109db8 SI_edCreateObjectFromInterest]:
/// (dictionary, relation dictionary, chapter, element it hangs from, &source element, object).
pub fn ed_create_object_from_interest<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32, reldict: u32, chapter: u32, rel_from: u32, pelem: u32, object: u32) {
    let (name, kind, relid) = if chapter == cid(m, 4) {
        (0x0868_0e80u32, 4u32, 0x456u32)
    } else {
        (0x0868_0e87, 6, 0x83e)
    };
    let node = o.call_addr(m, SI_DB_LIST_NODE_ALLOCATE, &[]);
    let src = m.u32(pelem);
    let saved = m.u32(src + 0x14);
    m.set_u32(src + 0x14, name);
    o.call_addr(m, SI_DB_LIST_NODE_COPY_INFO, &[dict, m.u32(pelem), node]);
    let src = m.u32(pelem);
    m.set_u32(src + 0x14, saved);
    let list = m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(m.u32(SI_PROC_DATA) + 4).wrapping_mul(0x24));
    let g = o.call_addr(m, SI_FIND_GL_DATA, &[list, object, kind]);
    let object = if g != 0 { m.u32(g + 4) } else { object };
    let idx = o.call_addr(m, SI_ELEMDICT_ADD_ELEM, &[dict, chapter, node, object, g, 0, 0]);
    o.call_addr(m, SI_ELEMDICT_ADD_RELATION, &[dict, reldict, idx, rel_from, relid, 1, 0]);
    o.call_addr(m, SI_ELEMDICT_UPDATE_HIER_INFO, &[dict, chapter, 1, relid]);
    let chap = chapter_at(m, dict, chapter);
    let e = elem_at(m, chap, idx);
    m.set_u16(e + 0x46, 1);
}

const LOAD_OPTIM_TMP: u32 = 0x0878_3124;
const LOAD_OPTIM_VAL: u32 = 0x0878_3120;

/// Whether the load-optimisation switch (environment variable `SI_LOAD_OPTIM`) is set,
/// asked once [soft 0x080d94b0 SI_getLoadOptim].
pub fn get_load_optim<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem) -> i16 {
    if m.u32(LOAD_OPTIM_TMP) == 0 {
        let r = o.call_addr(m, GETENV, &[0x0867_d7c3]);
        m.set_u16(LOAD_OPTIM_VAL, (r != 0) as u16);
        m.set_u32(LOAD_OPTIM_TMP, 1);
    }
    m.i16(LOAD_OPTIM_VAL)
}

/// The texture-domain numbers of the loaded scene made into positions
/// [soft 0x080d94f4 SI_sceneUVDomainInit]: each 2D texture's `_UVDomainID_` attribute is
/// taken off its list and read; the models' domain lists (at +0x428, one per extra
/// level) are rewritten from the old numbers to the textures' positions.
pub fn scene_uv_domain_init(m: &mut Mem) {
    let dict = cur_dict(m);
    let tex = chapter_at(m, dict, cid(m, 0x68));
    let models = chapter_at(m, dict, cid(m, 0x20));
    let mut remap = 0;
    let n = m.i32(tex + 4);
    if n > 0 {
        remap = m.si_calloc(n as u32, 4);
    }
    let mut max: i32 = -1;
    for i in 0..n.max(0) as u32 {
        let e = elem_at(m, tex, i);
        let obj = m.u32(e + 4);
        if m.i16(e + 0x48) != 0 || obj == 0 || m.i16(e + 0x46) != 1 {
            m.set_i32(remap + 4 * i, -1);
            continue;
        }
        let mut node = m.u32(obj + 0x1d0);
        let mut prev = 0;
        while node != 0 {
            // The original compares 13 bytes of the name with "_UVDomainID_\0".
            if m.cstr(m.u32(node)) == b"_UVDomainID_" {
                let id = si_mem::clib::strtol10(m, m.u32(node + 0xc));
                m.set_i32(remap + 4 * i, id);
                if id > max {
                    max = id;
                }
                if prev != 0 {
                    let nx = m.u32(node + 0x10);
                    m.set_u32(prev + 0x10, nx);
                } else {
                    // The whole list is dropped, not just the head: as the original.
                    m.set_u32(obj + 0x1d0, 0);
                }
                for off in [0, 0xc] {
                    let p = m.u32(node + off);
                    if p != 0 {
                        m.free(p);
                        m.set_u32(node + off, 0);
                    }
                }
                m.free(node);
                break;
            }
            prev = node;
            node = m.u32(node + 0x10);
        }
    }
    max += 1;
    let mut tab = 0;
    if max > 0 {
        tab = m.si_calloc(max as u32, 4);
        for k in 0..max as u32 {
            m.set_i32(tab + 4 * k, -1);
        }
        for i in 0..m.i32(tex + 4).max(0) as u32 {
            let r = m.i32(remap + 4 * i);
            if r != -1 {
                // A negative number writes before the table, as the original does.
                m.set_u32(tab.wrapping_add((r as u32).wrapping_mul(4)), i);
            }
        }
    }
    if remap != 0 {
        m.free(remap);
    }
    for i in 0..m.i32(models + 4).max(0) as u32 {
        let e = elem_at(m, models, i);
        let obj = m.u32(e + 4);
        if m.i16(e + 0x48) != 0 || obj == 0 || m.i16(e + 0x46) != 1 {
            continue;
        }
        let cnt = m.i32(obj + 0x424);
        if cnt <= 1 {
            continue;
        }
        if m.u32(obj + 0x428) == 0 {
            let p = m.si_calloc((cnt - 1) as u32, 8);
            m.set_u32(obj + 0x428, p);
            continue;
        }
        let mut k = 0i32;
        while k < m.i32(obj + 0x424) - 1 {
            let rec = m.u32(obj + 0x428).wrapping_add((k as u32).wrapping_mul(8));
            let mut out = 0i32;
            let items = m.u32(rec);
            for j in 0..m.i32(rec + 4).max(0) as u32 {
                let v = m.i32(items + 4 * j);
                // With no table (no numbered domain) the original reads through a null
                // pointer for a negative number; here such a number is dropped.
                if v < max && tab != 0 {
                    let t = m.i32(tab.wrapping_add((v as u32).wrapping_mul(4)));
                    if t != -1 {
                        m.set_i32(items + 4 * out as u32, t);
                        out += 1;
                    }
                }
            }
            if m.i32(rec + 4) != out {
                m.set_i32(rec + 4, out);
                let p = m.si_realloc(m.u32(rec), (out as u32).wrapping_mul(4));
                m.set_u32(rec, p);
            }
            k += 1;
        }
    }
    if tab != 0 {
        m.free(tab);
    }
}

/// Marks the models the later program cannot edit [soft 0x080da4c0 flagWrathUnsupportedElem].
pub fn flag_wrath_unsupported_elem<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, dict: u32) {
    let models = cid(m, 0x20);
    if dict == 0 || models as i32 >= m.i32(dict) {
        return;
    }
    let chap = chapter_at(m, dict, models);
    for i in 0..m.i32(chap + 4).max(0) as u32 {
        let e = elem_at(m, chap, i);
        let obj = m.u32(e + 8);
        if obj != 0 && m.u8(obj + 0x17) & 0x10 == 0 {
            let r = o.call_addr(m, SI_ED_CHECK_FOR_UNEDITABLE, &[e, models]);
            if r as u16 != 0 {
                o.call_addr(m, SI_ED_SET_UNEDITABLE, &[dict, e]);
            }
        }
    }
}

/// Swaps the icon entry of a custom-relation list with the last one of the same key
/// [soft 0x080dca78 cusSwapIcon]: (element, list index, key, model index).
pub fn cus_swap_icon(m: &mut Mem, elem: u32, list_idx: u32, key: u32, idx: u32) {
    let list = m.u32(elem + 100).wrapping_add(list_idx.wrapping_mul(0x18));
    let count = m.i32(list);
    let ents = m.u32(list + 0xc);
    let models = cid(m, 0x20);
    let mut i = 0;
    while i < count {
        let e = ents + 32 * i as u32;
        if m.u32(e + 0xc) == key && m.u16(e + 0x12) == 5 && m.u32(e) == 20000
            && m.i16(e + 0x10) as i32 as u32 == models && m.u32(e + 4) == idx
        {
            break;
        }
        i += 1;
    }
    // With no first match the original goes on with an uninitialised index (stack
    // garbage) and may swap two entries at random; here nothing is swapped.
    if i >= count {
        return;
    }
    let mut j = i;
    let mut k = i;
    while k < count {
        let e = ents + 32 * k as u32;
        if m.u32(e + 0xc) == key && m.u32(e) == 20000 {
            j = k;
        }
        k += 1;
    }
    if i != j {
        let (a, b) = (ents + 32 * i as u32, ents + 32 * j as u32);
        let mut t = [0u32; 8];
        for w in 0..8 {
            t[w] = m.u32(a + 4 * w as u32);
        }
        for w in 0..8 {
            let v = m.u32(b + 4 * w as u32);
            m.set_u32(a + 4 * w as u32, v);
        }
        for w in 0..8 {
            m.set_u32(b + 4 * w as u32, t[w]);
        }
    }
}

/// The globals of the five custom-record kinds (model, motion, actor, matter, tools).
const CUS_RECORDS: [u32; 5] = [0x0882_2c4c, 0x0882_2dac, 0x0882_298c, 0x0882_2aec, 0x0882_2f0c];

/// Custom relations of the models moved to the order the icons need
/// [soft 0x080dc93c cusModifyRelations]: (pointer to the dictionary pointer).
pub fn cus_modify_relations(m: &mut Mem, pdict: u32) {
    if CUS_RECORDS.iter().all(|&g| m.u32(m.u32(g) + 0xfc) == 0) {
        return;
    }
    let dict = m.u32(pdict);
    let models = cid(m, 0x20);
    let chap = chapter_at(m, dict, models);
    for i in 0..m.i32(chap + 4).max(0) as u32 {
        let e = elem_at(m, chap, i);
        let obj = m.u32(e + 4);
        if obj == 0 || m.u32(obj + 0x2ec) != 13 || m.i16(e + 0x46) != 1 {
            continue;
        }
        let rl = m.u32(e + 0x6c) + 0x78;
        let n = m.i32(rl);
        for j in 0..n.max(0) as u32 {
            let ent = m.u32(rl + 0xc) + 32 * j;
            if m.u32(ent) != 20000 {
                continue;
            }
            let list_idx = m.i16(ent + 0x12) as i32 as u32;
            let key = m.u32(ent + 0xc);
            let tchap = chapter_at(m, m.u32(pdict), m.i16(ent + 0x10) as i32 as u32);
            let target = elem_at(m, tchap, m.u32(ent + 4));
            cus_swap_icon(m, target, list_idx, key, i);
        }
    }
}

/// The corner groups of the end-point conversion: (destination, A, B), byte offsets from
/// the point array, for each of the three coordinates.
fn convert_groups(case: u32, p2: i32, p4: i32, p6: i32, p7: i32, h: i32) -> Vec<(i32, i32, i32)> {
    let s = 16i32.wrapping_mul(p2);
    let hh = 16i32.wrapping_mul(p2.wrapping_add(h));
    let hm = 16i32.wrapping_mul(p2.wrapping_sub(h));
    let w = |v: i32| 16i32.wrapping_mul(v);
    let p6p2 = w(p6.wrapping_add(p2));
    let p62p2 = w(p6.wrapping_mul(2).wrapping_add(p2));
    match case {
        1 => {
            let d = w(p4.wrapping_sub(p6).wrapping_add(p7));
            vec![(d + 0x10, s + 0x10, hh + 0x30), (d, s, hh + 0x20), (d - 0x10, s - 0x10, hh + 0x10)]
        }
        2 => vec![
            (s - 0x10, hm - 0x20, hm - 0x10),
            (p6p2 - 0x10, s, s + 0x10),
            (p62p2 - 0x10, hh + 0x20, hh + 0x30),
        ],
        3 => vec![
            (w(p4 - 1), s, hh + 0x30),
            (w((p4 - 1).wrapping_sub(p6 - 1)), s, hh + 0x20),
            (w((p4 - 1).wrapping_sub(p6 - 2)), s + 0x10, hh + 0x30),
            (w(p6) - 0x10, s, s + 0x10),
            (w(p6.wrapping_mul(2)) - 0x10, hh + 0x20, hh + 0x30),
        ],
        4 => vec![
            (p6p2 - 0x10, s - 0x10, hm - 0x30),
            (p6p2, s, hm - 0x20),
            (p6p2 + 0x10, s + 0x10, hm - 0x10),
        ],
        6 => vec![
            (p62p2 - 0x10, s, hm - 0x10),
            (p6p2 - 0x10, s, s + 0x10),
            (s - 0x10, hm - 0x20, hm - 0x10),
            (p6p2, s, hm - 0x20),
            (p6p2 + 0x10, s + 0x10, hm - 0x10),
        ],
        8 => vec![
            (p6p2 + 0x10, hh + 0x20, hh + 0x10),
            (s + 0x10, s, s - 0x10),
            (w(p2.wrapping_sub(p6 - 1)), hm - 0x20, hm - 0x30),
        ],
        9 => vec![
            (w(p4 - 1) - 0x10, s, hh + 0x10),
            (w(p2.wrapping_add(p6)) + 0x10, hh + 0x20, hh + 0x10),
            (s + 0x10, s, s - 0x10),
            (w(p4 - 1) - 0x20, s, hh + 0x20),
            (w(p4 - 1) - 0x30, s - 0x10, hh + 0x10),
        ],
        0xc => vec![
            (p6p2 + 0x10, s, hm - 0x30),
            (p6p2 - 0x10, s - 0x10, hm - 0x30),
            (p6p2, s, hm - 0x20),
            (s + 0x10, s, s - 0x10),
            (w(p2.wrapping_sub(p6 - 1)), hm - 0x20, hm - 0x30),
        ],
        _ => vec![],
    }
}

/// The end points of a padded patch grid rebuilt from the neighbours by reflection
/// [soft 0x080dcb54 SI_convertPatch]: (patch, point number, point array, point count, rows,
/// columns, source number). Each coordinate is `(A - B) + A` in extended precision, stored
/// as a float.
///
/// A column count of 0 is a division by zero in the original; nothing is done here.
pub fn convert_patch(m: &mut Mem, patch: u32, p2: i32, base: u32, p4: i32, p5: i32, p6: i32, p7: i32) {
    if p6 == 0 {
        return;
    }
    let bit0 = (p2 < p6) as u32;
    let bit1 = (p2.wrapping_rem(p6) == 0) as u32;
    let bit2 = (p6.wrapping_mul(3).wrapping_add(p2) > p5.wrapping_mul(p6).wrapping_sub(1)) as u32;
    let bit3 = (p2.wrapping_rem(p6) == p6.wrapping_sub(3)) as u32;
    let case = bit0 | (bit1 << 1) | (bit2 << 2) | (bit3 << 3);
    let h = m.u16(patch + 0xe) as i32;
    for (d, a, b) in convert_groups(case, p2, p4, p6, p7, h) {
        for c in [0u32, 4, 8] {
            let at = |off: i32| base.wrapping_add(off as u32).wrapping_add(c);
            let av = m.f32(at(a));
            let bv = m.f32(at(b));
            let r = ((x(av) - bv) + av).to_f32();
            m.set_f32(at(d), r);
        }
    }
}

/// The internals of a model handed to the editor's structures, recursively
/// [soft 0x081094b0 SI_dbConvertModelInternals2DB]: clusters, dynamics, materials and
/// textures, the gravity link, lattices and the light/camera relation.
pub fn db_convert_model_internals2db<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, model: u32, force: i16) {
    let mut k = m.u32(model + 0x348);
    while k != 0 {
        o.call_addr(m, SI_ED_ADD_CLUSTER, &[model, k]);
        k = m.u32(k + 0x1c);
    }
    if m.u32(model + 0x304) != 0 {
        let mut p = model;
        let alg = m.u32(p + 0x2ec);
        // Up the chain of the links at +0x2c4 to the first record with no word at +0x30
        // whose kind is 3 or 4; only a kind-1 or kind-2 model without that word starts it.
        if m.u32(p + 0x30) == 0 && alg.wrapping_sub(1) <= 1 && alg.wrapping_sub(3) > 1 {
            loop {
                loop {
                    p = m.u32(p + 0x2c4);
                    if m.u32(p + 0x30) == 0 {
                        break;
                    }
                }
                if m.u32(p + 0x2ec).wrapping_sub(3) <= 1 {
                    break;
                }
            }
        }
        o.call_addr(m, SI_ED_ELEMENT_SET_DYNAMICS, &[model, p]);
    }
    o.call_addr(m, SI_DB_CONVERT_MODEL_MAT_TXT2DB, &[model, force as u32, 0, 0]);
    if m.u32(model + 0x304) != 0 {
        o.call_addr(m, SI_DYNA_LINK_ROOT_TO_GRAVITY, &[model]);
    }
    if m.u16(model + 0x4c) != 0 {
        o.call_addr(m, SI_ED_ADD_LATTICE, &[model, 0]);
    }
    if m.u16(model + 0x54) != 0 {
        o.call_addr(m, SI_ED_ADD_LATTICE, &[model, 1]);
    }
    let t = m.u32(model + 0x31c);
    if t == 5 || t == 6 || t == 8 {
        let l = m.local(8);
        let e = o.call_addr(m, SI_ED_GET_ELEM_FROM_MODEL, &[model, l]);
        if e != 0 {
            let dict = cur_dict(m);
            let models = cid(m, 0x20);
            let r = o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, e, 1, 2, 0xcf, models, l + 4, l + 4]);
            if r == 0 || r != e {
                let idx = m.u32(l);
                o.call_addr(m, SI_ELEMDICT_ADD_RELATION, &[dict, SI_RD, idx, idx, 0xcf, 1, 0]);
            }
        }
        m.release(8);
    }
    let up = m.u32(model + 0x2c4);
    if up != 0 {
        db_convert_model_internals2db(o, m, up, force);
    }
    let side = m.u32(model + 0x2c0);
    if side != 0 {
        db_convert_model_internals2db(o, m, side, force);
    }
}
