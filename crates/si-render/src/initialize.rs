// SPDX-License-Identifier: MIT
//! Set-up once per render: the original's `initialize.c` (finding 0012, sections 4 and 16).
//!
//! Two kinds of port live here. `initData` is on this crate's own records, as are
//! `si_initViewport`, `initBuffers` and `initFilter` in [`crate::update`], beside the record
//! they fill. The routines that walk the scene's dictionary and its nodes before the first
//! frame (`SI_initialize`, `SI_initTxtCnt`, `mdlInitTxtCnt`, `SI_propagate`, `pushMaterial`,
//! `haveRelation`, `propagateInstances`, `pushInstance`, `testUvPropagation`,
//! `SI_setUvWrapFlags`, `setUvWrapFlags_model`, `SI_printWarnUV`) are ported **in place**, on
//! the memory image (the module [`crate::img`] names the offsets), and call the dictionary
//! and the instance code through [`crate::img::Outside`].

use crate::types::Rgbazo;
use crate::update::{stat, Prepared, UpdateEnv};
use si_x87::{fmov, x};

/// 0x086545c0, float: what `initData` stores at 0x08790dc0, the share of a cell that must
/// be empty on one side for the tree build to cut it off.
const EMPTY_FRACTION: f32 = f32::from_bits(0x3e19_999a);
/// 0x086545c8, double.
const THIRD: f64 = 0.33;
/// 0x086545d0, 0x086545d4, floats: −100000 and 100000.
const FAR: f32 = 100000.0;
/// 0x0869dad8, 0x0869dae4: two texts of twelve bytes `initData` copies to 0x08790c80 and
/// 0x08790d68 (no reader found in the files read).
pub const RENDER_VERSION: &str = "render v1.0";
pub const UNSPECIFIED: &str = "unspecified";
/// 0x0869dafb.
pub fn msg_no_stats() -> &'static str {
    si_rt::text(0x0869_dafb)
}

/// The render's fixed starting state [soft 0x082765e8 initData]: `initData()`.
///
/// - the ray tracing word (0x08790c7c) is cleared; two texts are copied
///   ([`RENDER_VERSION`], [`UNSPECIFIED`]); the float at 0x08790dc0 becomes 0.15; the word
///   at 0x08790db8 and the statistics file pointer are cleared;
/// - the movement threshold of motion blur (0x08790e00) = r32(sampling level × the float
///   at 0x08790dfc), **with the level as it is before the next step**; the shutter time is
///   copied to 0x08790df8;
/// - with any statistics switch on, `<name>.stats` is opened for writing; if that fails a
///   message is shown and all three switches are cleared;
/// - without adaptive supersampling the filing routine is `si_pointSort`; with it the
///   sampling level is forced to 1, the filing routine is `si_areaSort` and 0x08790d40 =
///   r32(contrast threshold × 0.33);
/// - a scene without a light gets a copy of `SI_defaultLight`;
/// - five words at 0x08790b74 are cleared (no reader found), the blank sample becomes (0,
///   0, 0, 0, −100000, −100000), the two "empty" boxes are set (least values 100000,
///   greatest −100000);
/// - `SI_stats` is cleared, **except the word at +0x0c** (the depth of field byte count).
///
/// Class: single (one product each).
pub fn init_data(p: &mut Prepared, env: &mut dyn UpdateEnv) {
    p.options.raytrace = 0;
    p.bsp_params.empty_fraction = fmov(EMPTY_FRACTION);
    p.blur_threshold = (x(p.settings.level as i32) * p.options.blur_step).to_f32();
    p.shutter_copy = fmov(p.options.shutter);
    let o = &mut p.options;
    if o.timing != 0 || o.tree_stats != 0 || o.mem_stats != 0 {
        let mut name = o.name.clone();
        name.extend_from_slice(b".stats");
        if !env.stats_open(&name) {
            env.status_message(msg_no_stats());
            o.tree_stats = 0;
            o.mem_stats = 0;
            o.timing = 0;
        }
    }
    if o.adaptive == 0 {
        p.g.area_sort = false;
    } else {
        p.contrast_third = (x(o.contrast) * THIRD).to_f32();
        p.settings.level = 1;
        p.g.area_sort = true;
    }
    if p.st.light_head.is_none() {
        let light = env.default_light();
        p.st.light_head = Some(p.st.lights.len());
        p.st.lights.push(light);
    }
    p.blank = Rgbazo { r: 0.0, g: 0.0, b: 0.0, a: 0.0, z: -FAR, o: -FAR };
    p.bsp_params.empty_box = [FAR, -FAR, FAR, -FAR, FAR, -FAR];
    p.empty_bounds = [FAR, -FAR, FAR, -FAR];
    let dof = p.stats[stat::DOF];
    p.stats = [0; 17];
    // (+0x34, the texture pixels, is kept with the pictures)
    p.st.texture_pixels = 0;
    p.stats[stat::DOF] = dof;
    p.g.bins.accepted = 0;
    p.g.bins.made = 0;
    p.g.trace.made = 0;
    p.g.clipper.made_vertices = 0;
    p.g.clipper.made_pairs = 0;
    p.g.made_tns = 0;
    p.tree_counts = [0, 0];
}

// ------------------------------------------------------------- in place, on the image

use crate::img::{dict, f, g, lclmap, material, model, sprintf, FmtArg, Outside};
use si_mem::Mem;

/// 0x0869db20.
pub fn fmt_warn_uv() -> &'static [u8] {
    si_rt::message(0x0869_db20)
}

/// The start of a render [soft 0x08276580 SI_initialize]: `SI_initialize(dictionary)`.
///
/// `mallopt(1, 100000)`, `SI_chunkInitialize(SI_renChunk)`, `initData`, `si_initViewport`,
/// `initBuffers`, `initFilter` (all through [`Outside`]: they are ported on this crate's own
/// records), then on the image [`init_txt_cnt`], [`propagate`], [`set_uv_wrap_flags`].
/// Answers 1. Class: copy.
pub fn initialize(m: &mut Mem, env: &mut dyn Outside, dictionary: u32) -> u32 {
    env.call(m, f::MALLOPT, &[1, 100000]);
    let chunk = m.u32(g::SI_REN_CHUNK);
    env.call(m, f::SI_CHUNK_INITIALIZE, &[chunk]);
    env.call(m, f::INIT_DATA, &[]);
    env.call(m, f::SI_INIT_VIEWPORT, &[]);
    env.call(m, f::INIT_BUFFERS, &[]);
    env.call(m, f::INIT_FILTER, &[]);
    init_txt_cnt(m, dictionary);
    propagate(m, env, dictionary);
    set_uv_wrap_flags(m, env, dictionary);
    1
}

/// Counts a node's texture lists [soft 0x08276d28 mdlInitTxtCnt]: `mdlInitTxtCnt(node)`.
///
/// For every material of the node the word at +0x754 becomes the length of the material's
/// 2D texture list (+0x6ec, linked through +0x16c); the node's own count (+0x216) becomes
/// the length of its list (+0x21c); the node's flag word (+0x214) is cleared. The counts
/// are sixteen-bit and are counted in memory, one store per texture. Class: copy.
pub fn mdl_init_txt_cnt(m: &mut Mem, node: u32) {
    let mut i: i32 = 0;
    if m.i16(node + model::NB_MATERIALS) > 0 {
        loop {
            let mat = m.u32(m.u32(node + model::MATERIALS).wrapping_add(4 * i as u32));
            m.set_i16(mat + material::TXT_COUNT, 0);
            i += 1;
            let mut t = m.u32(mat + material::TEXTURES);
            while t != 0 {
                let n = m.i16(mat + material::TXT_COUNT).wrapping_add(1);
                m.set_i16(mat + material::TXT_COUNT, n);
                t = m.u32(t + lclmap::NEXT);
            }
            if i >= m.i16(node + model::NB_MATERIALS) as i32 {
                break;
            }
        }
    }
    let mut t = m.u32(node + model::TEXTURES);
    m.set_i16(node + model::NB_TEXTURES, 0);
    while t != 0 {
        let n = m.i16(node + model::NB_TEXTURES).wrapping_add(1);
        m.set_i16(node + model::NB_TEXTURES, n);
        t = m.u32(t + lclmap::NEXT);
    }
    m.set_u16(node + model::TXT_FLAGS, 0);
}

/// The models chapter of a dictionary: its address.
fn models_chapter(m: &Mem, dictionary: u32) -> u32 {
    m.u32(g::CID_MODELS).wrapping_mul(dict::LIST_SIZE).wrapping_add(m.u32(dictionary + dict::DICT_LISTS))
}

/// [`mdl_init_txt_cnt`] for every model element in use that has a node
/// [soft 0x08276db0 SI_initTxtCnt]: `SI_initTxtCnt(dictionary)`. Answers 1. Class: copy.
pub fn init_txt_cnt(m: &mut Mem, dictionary: u32) -> u32 {
    let list = models_chapter(m, dictionary);
    let mut i: i32 = 0;
    while i < m.i32(list + dict::LIST_COUNT) {
        let e = m.u32(list + dict::LIST_ELEMS).wrapping_add((i as u32).wrapping_mul(dict::ELEM_SIZE));
        if m.i16(e + dict::ELEM_UNUSED) == 0 {
            let node = m.u32(e + dict::ELEM_DATA);
            if node != 0 {
                mdl_init_txt_cnt(m, node);
            }
        }
        i += 1;
    }
    1
}

/// Whether an element has a relation of a kind into a chapter
/// [soft 0x082771cc haveRelation]: `haveRelation(dictionary, element, relation, chapter)`
/// asks `SI_elemdictFindRelatedElem (0x0813508c)`(dictionary, element, 1, 3, relation,
/// chapter, &local, &local) and answers 1 when that is not 0. Class: copy.
pub fn have_relation(m: &mut Mem, env: &mut dyn Outside, dictionary: u32, element: u32, relation: u32, chapter: u32) -> u32 {
    let out = m.local(4);
    let r = env.call(m, f::SI_ELEMDICT_FIND_RELATED_ELEM, &[dictionary, element, 1, 3, relation, chapter, out, out]);
    m.release(4);
    (r != 0) as u32
}

/// Whether one of the first `count` textures of a list takes the mesh's own coordinates
/// [soft 0x08277400 testUvPropagation]: `testUvPropagation(texture, count)`: 1 when a
/// method word (+0x2a) is 4. The list is followed `count` times without a null test.
/// Class: copy.
pub fn test_uv_propagation(m: &Mem, mut texture: u32, count: i32) -> u32 {
    let mut i = 0;
    while i < count {
        if m.i16(texture + lclmap::METHOD) == 4 {
            return 1;
        }
        texture = m.u32(texture + lclmap::NEXT);
        i += 1;
    }
    0
}

/// The warning of a UV texture handed down to a node that is no patch
/// [soft 0x0827759c SI_printWarnUV]: `SI_printWarnUV(name)` formats [`FMT_WARN_UV`] into a
/// local of 4096 bytes and shows it with `SI_statusDisplayMessage (0x083cca94)`(text, 2).
/// Class: copy.
pub fn print_warn_uv(m: &mut Mem, env: &mut dyn Outside, name: u32) {
    let text = sprintf(fmt_warn_uv(), &[FmtArg::Text(m.cstr(name))]);
    let buffer = m.local(4096);
    m.put_cstr(buffer, &text);
    env.call(m, f::SI_STATUS_DISPLAY_MESSAGE, &[buffer, 2]);
    m.release(4096);
}

/// True for the kinds of node `pushMaterial` does not warn about: 0, 1, 4, 5, 6, 9, 10.
fn quiet_kind(kind: i32) -> bool {
    matches!(kind, 0 | 1 | 4 | 5 | 6 | 9 | 10)
}

/// Hands materials and textures down the hierarchy
/// [soft 0x08276e0c pushMaterial]: `pushMaterial(dictionary, chapter, element, depth,
/// material count, materials, texture count, textures, 3D texture)`: what a node without
/// materials of its own renders with.
///
/// 1. The element's work byte (+0x5b) is set. The node's render blocks (+0x330, +0x22c,
///    +0x234) are freed and cleared, its two counts (+0x228, +0x230) and the "lent" word
///    (+0x204) cleared.
/// 2. An instance (algorithm 15) asks `SI_modelMasterFromInstance (0x081a7490)` for its
///    master; when the instance has no material array, the master's element is found
///    (`SI_edGetElemFromModel (0x080fcf20)`), and when that element has a material relation
///    (300 into the materials chapter) and the master a material array, **the master
///    lends**: the instance takes the master's array and count (lent bit 0); with a 2D
///    texture relation of the master's element (400) and a list on the master, its list
///    and count (bit 2); with a 3D texture relation (500) and a 3D texture, that (bit 3).
/// 3. Otherwise a node without a material array **takes what it is handed**: the array and
///    count (bit 0); when its own element has no 2D texture relation and a list is handed
///    down, that list with its count (bit 2); when it has no 3D texture relation and one
///    is handed down, that (bit 3). Before each of the first two, unless the node's kind
///    is 0, 1, 4, 5, 6, 9 or 10: [`print_warn_uv`] with the element's name for every handed
///    material whose list holds a texture of method 4, and once for the handed list.
/// 4. A node with materials of its own keeps them; its lent word stays 0.
/// 5. For every relation of the element of kind 0x6e (a son) whose element is in use:
///    the same routine with the node's materials, textures and 3D texture as they now are,
///    and depth + 1. (The depth is passed on and never read.)
///
/// Class: copy.
#[allow(clippy::too_many_arguments)]
pub fn push_material(m: &mut Mem, env: &mut dyn Outside, dictionary: u32, chapter: u32, element: u32, depth: i32, nb_materials: i16, materials: u32, nb_textures: i16, textures: u32, texture3d: u32) {
    m.set_u8(element + dict::ELEM_WORK, 1);
    let node = m.u32(element + dict::ELEM_DATA);
    crate::rnd_main::free_model_tcoord(m, node);
    for off in [model::REN_VERTICES, model::REN_TRIANGLES] {
        let p = m.u32(node + off);
        if p != 0 {
            m.free(p);
            m.set_u32(node + off, 0);
        }
    }
    m.set_i32(node + model::REN_NB_VERTICES, 0);
    m.set_i32(node + model::REN_NB_TRIANGLES, 0);
    m.set_u16(node + model::LENT, 0);
    let mut master = 0;
    let mut master_elem = 0;
    let mut master_lends = false;
    if m.i32(node + model::ALGO_TYPE) == 15 {
        master = env.call(m, f::SI_MODEL_MASTER_FROM_INSTANCE, &[node]);
        if m.u32(node + model::MATERIALS) == 0 {
            let out = m.local(4);
            master_elem = env.call(m, f::SI_ED_GET_ELEM_FROM_MODEL, &[master, out]);
            m.release(4);
            let chapter_mat = m.u32(g::CID_MATERIALS);
            if have_relation(m, env, dictionary, master_elem, 300, chapter_mat) != 0 {
                master_lends = m.u32(master + model::MATERIALS) != 0;
            }
        }
    }
    if master_lends {
        let (a, n) = (m.u32(master + model::MATERIALS), m.u16(master + model::NB_MATERIALS));
        m.set_u32(node + model::MATERIALS, a);
        m.set_u16(node + model::NB_MATERIALS, n);
        m.set_u16(node + model::LENT, 1);
        let chapter_2d = m.u32(g::CID_TEXTURES_2D);
        if have_relation(m, env, dictionary, master_elem, 400, chapter_2d) != 0 && m.u32(master + model::TEXTURES) != 0 {
            m.or_u8(node + model::LENT, 4);
            let t = m.u32(master + model::TEXTURES);
            m.set_u32(node + model::TEXTURE_218, t);
            m.set_u32(node + model::TEXTURES, t);
            let n = m.u16(master + model::NB_TEXTURES);
            m.set_u16(node + model::NB_TEXTURES, n);
        }
        let chapter_3d = m.u32(g::CID_TEXTURES_3D);
        if have_relation(m, env, dictionary, master_elem, 500, chapter_3d) != 0 {
            let t = m.u32(master + model::TEXTURE3D);
            if t != 0 {
                m.set_u32(node + model::TEXTURE3D, t);
                m.or_u8(node + model::LENT, 8);
            }
        }
    } else if m.u32(node + model::MATERIALS) == 0 {
        if !quiet_kind(m.i32(node + model::KIND)) {
            let mut i: i32 = 0;
            while i < nb_materials as i32 {
                let mat = m.u32(materials.wrapping_add(4 * i as u32));
                if test_uv_propagation(m, m.u32(mat + material::TEXTURES), m.i16(mat + material::NB_TEXTURES) as i32) != 0 {
                    let name = m.u32(m.u32(element + dict::ELEM_NAME_REC) + dict::NAME_REC_TEXT);
                    print_warn_uv(m, env, name);
                }
                i += 1;
            }
        }
        m.set_u32(node + model::MATERIALS, materials);
        m.set_i16(node + model::NB_MATERIALS, nb_materials);
        m.set_u16(node + model::LENT, 1);
        let chapter_2d = m.u32(g::CID_TEXTURES_2D);
        if have_relation(m, env, dictionary, element, 400, chapter_2d) == 0 && textures != 0 {
            if !quiet_kind(m.i32(node + model::KIND)) && test_uv_propagation(m, textures, nb_textures as i32) != 0 {
                let name = m.u32(m.u32(element + dict::ELEM_NAME_REC) + dict::NAME_REC_TEXT);
                print_warn_uv(m, env, name);
            }
            m.or_u8(node + model::LENT, 4);
            m.set_u32(node + model::TEXTURE_218, textures);
            m.set_u32(node + model::TEXTURES, textures);
            m.set_i16(node + model::NB_TEXTURES, nb_textures);
        }
        let chapter_3d = m.u32(g::CID_TEXTURES_3D);
        if have_relation(m, env, dictionary, element, 500, chapter_3d) == 0 && texture3d != 0 {
            m.set_u32(node + model::TEXTURE3D, texture3d);
            m.or_u8(node + model::LENT, 8);
        }
    } else {
        m.set_u16(node + model::LENT, 0);
    }
    let relations = m.u32(element + dict::ELEM_RELATIONS);
    let count = m.i32(relations + dict::REL_COUNT);
    let mut i: i32 = 0;
    while i < count {
        let r = m.u32(relations + dict::REL_ARRAY).wrapping_add((i as u32) << 5);
        if m.i32(r) == dict::REL_SON as i32 {
            let son = m.u32(chapter + dict::LIST_ELEMS).wrapping_add(m.u32(r + 4).wrapping_mul(dict::ELEM_SIZE));
            if m.i16(son + dict::ELEM_UNUSED) == 0 {
                let (n, a, tn, t, t3) =
                    (m.i16(node + model::NB_MATERIALS), m.u32(node + model::MATERIALS), m.i16(node + model::NB_TEXTURES), m.u32(node + model::TEXTURES), m.u32(node + model::TEXTURE3D));
                push_material(m, env, dictionary, chapter, son, depth.wrapping_add(1), n, a, tn, t, t3);
            }
        }
        i += 1;
    }
}

/// Before the first frame, the materials every node renders with
/// [soft 0x08277204 SI_propagate]: `SI_propagate(dictionary)`.
///
/// `SI_elemlistClearLowByteWorkFlag (0x08132c70)`(dictionary, models chapter); then for
/// every model element that has no father (+0x54 below 1), is in use and whose work byte
/// is still clear: [`push_material`] with depth 1 and **one material, `SI_defaultMaterial`**
/// (the array is the pointer `ptrDefault`, 0x0869cf60), no textures. Then
/// [`propagate_instances`] over the root chain (0x087907e8). The count of elements is read
/// once. Class: copy.
pub fn propagate(m: &mut Mem, env: &mut dyn Outside, dictionary: u32) {
    let chapter_no = m.u32(g::CID_MODELS);
    env.call(m, f::SI_ELEMLIST_CLEAR_LOW_BYTE_WORK_FLAG, &[dictionary, chapter_no]);
    let chapter = models_chapter(m, dictionary);
    let mut left = m.i32(chapter + dict::LIST_COUNT);
    if left > 0 {
        let mut off: u32 = 0;
        loop {
            let e = m.u32(chapter + dict::LIST_ELEMS).wrapping_add(off);
            if m.i32(e + dict::ELEM_NB_FATHERS) < 1 && m.i16(e + dict::ELEM_UNUSED) == 0 && m.u8(e + dict::ELEM_WORK) == 0 {
                push_material(m, env, dictionary, chapter, e, 1, 1, g::PTR_DEFAULT, 0, 0, 0);
            }
            off = off.wrapping_add(dict::ELEM_SIZE);
            left -= 1;
            if left == 0 {
                break;
            }
        }
    }
    let root = m.u32(g::DB_ROOT);
    propagate_instances(m, dictionary, root);
}

/// The copies under the instance roots get their instances' materials
/// [soft 0x082772a0 propagateInstances]: `propagateInstances(dictionary, root)`.
///
/// For each node of the root chain (through +0x2c0) of algorithm 15 that has a son (the
/// copy of the master's hierarchy): [`push_instance`] on the son with the materials,
/// textures and 3D texture of **the node the dictionary holds for the son's element
/// number** (the copy carries the instance's number: that node is the instance), and that
/// number. Class: copy.
pub fn propagate_instances(m: &mut Mem, dictionary: u32, mut root: u32) {
    while root != 0 {
        let son = m.u32(root + model::SON);
        if son != 0 && m.i32(root + model::ALGO_TYPE) == 15 {
            let elem = m.i32(son + model::ELEM_ID);
            let chapter = models_chapter(m, dictionary);
            let inst = m.u32(m.u32(chapter + dict::LIST_ELEMS).wrapping_add((elem as u32).wrapping_mul(dict::ELEM_SIZE)) + dict::ELEM_DATA);
            let (n, a, tn, t, t3) =
                (m.i16(inst + model::NB_MATERIALS) as i32, m.u32(inst + model::MATERIALS), m.i16(inst + model::NB_TEXTURES) as i32, m.u32(inst + model::TEXTURES), m.u32(inst + model::TEXTURE3D));
            push_instance(m, son, n, a, tn, t, t3, elem);
        }
        root = m.u32(root + model::BROTHER);
    }
}

/// [soft 0x0827732c pushInstance]: `pushInstance(node, material count, materials, texture
/// count, textures, 3D texture, element number)`, down a copied hierarchy.
///
/// [`mdl_init_txt_cnt`]; a node without materials (count 0) takes the element number, the
/// count and the array; a node without a 2D texture list takes a list handed down with a
/// count above 0 (count, second pointer, list); a node without a 3D texture takes the one
/// handed down. Then every son gets the node's own, as they now are. Class: copy.
#[allow(clippy::too_many_arguments)]
pub fn push_instance(m: &mut Mem, node: u32, nb_materials: i32, materials: u32, nb_textures: i32, textures: u32, texture3d: u32, element: i32) {
    mdl_init_txt_cnt(m, node);
    if m.i16(node + model::NB_MATERIALS) == 0 {
        m.set_i32(node + model::ELEM_ID, element);
        m.set_i16(node + model::NB_MATERIALS, nb_materials as i16);
        m.set_u32(node + model::MATERIALS, materials);
    }
    if m.u32(node + model::TEXTURES) == 0 && nb_textures > 0 {
        m.set_i16(node + model::NB_TEXTURES, nb_textures as i16);
        m.set_u32(node + model::TEXTURE_218, textures);
        m.set_u32(node + model::TEXTURES, textures);
    }
    if m.u32(node + model::TEXTURE3D) == 0 && texture3d != 0 {
        m.set_u32(node + model::TEXTURE3D, texture3d);
    }
    let mut son = m.u32(node + model::SON);
    while son != 0 {
        let (n, a, tn, t, t3, e) = (
            m.i16(node + model::NB_MATERIALS) as i32,
            m.u32(node + model::MATERIALS),
            m.i16(node + model::NB_TEXTURES) as i32,
            m.u32(node + model::TEXTURES),
            m.u32(node + model::TEXTURE3D),
            m.i32(node + model::ELEM_ID),
        );
        push_instance(m, son, n, a, tn, t, t3, e);
        son = m.u32(son + model::BROTHER);
    }
}

/// One texture of [`set_uv_wrap_flags_model`]: its element is found with
/// `SI_edGetElemFromTexture2D (0x080fd3e0)`; when there is one and its work byte is clear,
/// `SI_txt2dSetWrap (0x082ad9f8)`(node, texture) and the byte is set.
fn wrap_once(m: &mut Mem, env: &mut dyn Outside, node: u32, texture: u32) {
    let out = m.local(4);
    let e = env.call(m, f::SI_ED_GET_ELEM_FROM_TEXTURE2D, &[texture, out]);
    m.release(4);
    if e != 0 && m.u8(e + dict::ELEM_WORK) == 0 {
        env.call(m, f::SI_TXT2D_SET_WRAP, &[node, texture]);
        m.set_u8(e + dict::ELEM_WORK, 1);
    }
}

/// The "closed" flags of a hierarchy's textures
/// [soft 0x08277434 setUvWrapFlags_model]: `setUvWrapFlags_model(element)`.
///
/// Every texture of the node's own list, then of each of its materials' lists, goes
/// through `SI_txt2dSetWrap` once (the element's work byte remembers it): **with the first
/// node that is met using it**. Then the element's sons (`SI_edFindSon (0x080fcec0)`,
/// `SI_edFindNextSon (0x080fcef0)`) the same way. Class: copy.
pub fn set_uv_wrap_flags_model(m: &mut Mem, env: &mut dyn Outside, element: u32) {
    let node = m.u32(element + dict::ELEM_DATA);
    let mut t = m.u32(node + model::TEXTURES);
    while t != 0 {
        wrap_once(m, env, node, t);
        t = m.u32(t + lclmap::NEXT);
    }
    let mut i: i32 = 0;
    if m.i16(node + model::NB_MATERIALS) > 0 {
        loop {
            let mat = m.u32(m.u32(node + model::MATERIALS).wrapping_add(4 * i as u32));
            i += 1;
            let mut t = m.u32(mat + material::TEXTURES);
            while t != 0 {
                wrap_once(m, env, node, t);
                t = m.u32(t + lclmap::NEXT);
            }
            if i >= m.i16(node + model::NB_MATERIALS) as i32 {
                break;
            }
        }
    }
    let mut son = env.call(m, f::SI_ED_FIND_SON, &[element]);
    while son != 0 {
        set_uv_wrap_flags_model(m, env, son);
        son = env.call(m, f::SI_ED_FIND_NEXT_SON, &[element]);
    }
}

/// [soft 0x08277530 SI_setUvWrapFlags]: `SI_setUvWrapFlags(dictionary)`:
/// `SI_elemlistClearLowByteWorkFlag` on the 2D textures chapter, then
/// [`set_uv_wrap_flags_model`] for every model element in use that has a node and no father
/// (+0x54 = 0). The count of elements is read once. Class: copy.
pub fn set_uv_wrap_flags(m: &mut Mem, env: &mut dyn Outside, dictionary: u32) {
    let chapter_2d = m.u32(g::CID_TEXTURES_2D);
    env.call(m, f::SI_ELEMLIST_CLEAR_LOW_BYTE_WORK_FLAG, &[dictionary, chapter_2d]);
    let chapter = models_chapter(m, dictionary);
    let mut left = m.i32(chapter + dict::LIST_COUNT);
    if left > 0 {
        let mut off: u32 = 0;
        loop {
            let e = m.u32(chapter + dict::LIST_ELEMS).wrapping_add(off);
            if m.i16(e + dict::ELEM_UNUSED) == 0 && m.u32(e + dict::ELEM_DATA) != 0 && m.i32(e + dict::ELEM_NB_FATHERS) == 0 {
                set_uv_wrap_flags_model(m, env, e);
            }
            off = off.wrapping_add(dict::ELEM_SIZE);
            left -= 1;
            if left == 0 {
                break;
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::GeometryEnv;
    use crate::shade::ShadeState;

    struct Quiet(bool);
    impl GeometryEnv for Quiet {
        fn collect(&mut self, _: &mut ShadeState, _: &mut crate::geometry::Geometry, _: usize) {}
    }
    impl UpdateEnv for Quiet {
        fn stats_open(&mut self, _: &[u8]) -> bool {
            self.0
        }
    }

    #[test]
    fn adaptive_forces_level_one_after_the_blur_threshold() {
        si_rt::skip_without_install!();
        let mut p = Prepared::default();
        p.settings.level = 3;
        p.options.adaptive = 1;
        p.options.blur_step = 0.5;
        p.options.contrast = 1.0;
        p.options.mem_stats = 1;
        p.stats[stat::DOF] = 77;
        init_data(&mut p, &mut Quiet(false));
        assert_eq!(p.blur_threshold, 1.5);
        assert_eq!(p.settings.level, 1);
        assert!(p.g.area_sort);
        assert_eq!(p.contrast_third, (0.33f64) as f32);
        // the statistics file could not be opened
        assert_eq!(p.options.mem_stats, 0);
        assert_eq!(p.st.light_head, Some(0));
        assert_eq!(p.stats[stat::DOF], 77);
        assert_eq!(p.blank.z, -100000.0);
    }
}
