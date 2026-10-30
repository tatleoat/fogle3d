// SPDX-License-Identifier: MIT
//! The copies Duplicate makes "from setup" on the image: what a copied node takes over
//! from its original beyond the record, each kind governed by a setting of the Duplicate
//! command (`SI_glbDuplic`).
//!
//! Records without an established layout are addressed by named offsets, each said where
//! it is used: the user data record `__SI_UserNode` (0x14 bytes), the cluster of tagged
//! components `kluster`, and the scene element `__SI_Elem`.

use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::x;

use super::{callee, defs, elem_dict, g, Outside};

/// `__SI_UserNode`, by offsets: the name (a string), a 16-bit word, the size of the data,
/// the data, the next record.
mod user {
    pub const NAME: u32 = 0x00;
    pub const WORD: u32 = 0x04;
    pub const SIZE: u32 = 0x08;
    pub const DATA: u32 = 0x0c;
    pub const NEXT: u32 = 0x10;
}

/// The two names of user data that are not copied (strings of `.data` at 0x086d5e24 and
/// 0x086d5e2e), compared with their terminating zero.
const NOT_COPIED: [&[u8]; 2] = [b"CUSELEMCB\0", b"CUSNEXTELEMCB\0"];

/// The bytes at `a` equal `text` (a comparison that stops at the first difference).
fn bytes_equal(m: &Mem, a: u32, text: &[u8]) -> bool {
    text.iter().enumerate().all(|(i, &c)| m.u8(a.wrapping_add(i as u32)) == c)
}

/// Copies a chain of user data records [soft 0x084630dc SI_userDataCopyFromSetup]: every
/// record whose name is not one of the two custom-element callbacks is copied (a record
/// of 0x14 bytes from `SI_calloc`, the name from `strdup`, the data into a block from
/// `SI_malloc`) and appended; the first copy is stored at `out`, which is not written
/// when nothing is copied. A record without a name is copied too: the original hands the
/// null pointer to `strdup`, and so does this.
pub fn user_data_copy_from_setup(m: &mut Mem, source: u32, out: u32) {
    let mut last = 0u32;
    let mut at = source;
    while at != 0 {
        let name = m.u32(at + user::NAME);
        let skip = name != 0 && (bytes_equal(m, name, NOT_COPIED[0]) || bytes_equal(m, name, NOT_COPIED[1]));
        if !skip {
            let copy = m.si_calloc(1, 0x14);
            let name = m.u32(at + user::NAME);
            let s = m.strdup(name);
            m.set_u32(copy + user::NAME, s);
            let w = m.u16(at + user::WORD);
            m.set_u16(copy + user::WORD, w);
            let size = m.u32(at + user::SIZE);
            m.set_u32(copy + user::SIZE, size);
            let block = m.si_malloc(size);
            m.set_u32(copy + user::DATA, block);
            let (dst, src, n) = (m.u32(copy + user::DATA), m.u32(at + user::DATA), m.u32(at + user::SIZE));
            si_mem::clib::memcpy(m, dst, src, n);
            if last == 0 {
                m.set_u32(out, copy);
            } else {
                m.set_u32(last + user::NEXT, copy);
            }
            last = copy;
        }
        at = m.u32(at + user::NEXT);
    }
}

/// Copies the user data of a mesh's vertices, polygons or edges
/// [soft 0x08463260 SI_subelUserDataCopyFromSetup]: `cluster` is a `kluster` (the count at
/// +4, the array of component numbers at +0xc: named offsets), `chains` one chain of user
/// data per component. Both outputs are cleared first. A new cluster (`SI_klusterAllocate`)
/// gets the count and a copy of the numbers; a new array of chains gets a copy of each
/// chain ([`user_data_copy_from_setup`]). When the array of numbers cannot be had the new
/// cluster is freed; when the array of chains cannot, both are.
pub fn subel_user_data_copy_from_setup(m: &mut Mem, o: &mut dyn Outside, cluster: u32, chains: u32, out_cluster: u32, out_chains: u32) {
    m.set_u32(out_cluster, 0);
    m.set_u32(out_chains, 0);
    if m.i32(cluster + 4) < 1 {
        return;
    }
    let new = o.call(m, &callee::SI_KLUSTER_ALLOCATE, &[]);
    if new == 0 {
        return;
    }
    let count = m.i32(cluster + 4);
    m.set_i32(new + 4, count);
    let numbers = m.si_malloc((count as u32) << 2);
    m.set_u32(new + 0xc, numbers);
    if numbers != 0 {
        let mut i = 0;
        while i < m.i32(new + 4) {
            let v = m.u32(m.u32(cluster + 0xc).wrapping_add(i as u32 * 4));
            let dst = m.u32(new + 0xc).wrapping_add(i as u32 * 4);
            m.set_u32(dst, v);
            i += 1;
        }
        let array = m.si_calloc(m.u32(new + 4), 4);
        m.set_u32(out_chains, array);
        if array != 0 {
            let mut i = 0;
            while i < m.i32(new + 4) {
                let chain = m.u32(chains.wrapping_add(i as u32 * 4));
                let slot = m.u32(out_chains).wrapping_add(i as u32 * 4);
                user_data_copy_from_setup(m, chain, slot);
                i += 1;
            }
            m.set_u32(out_cluster, new);
            return;
        }
        let numbers = m.u32(new + 0xc);
        if numbers != 0 {
            m.free(numbers);
            m.set_u32(new + 0xc, 0);
        }
    }
    m.free(new);
}

/// The three kinds of component user data of a node, each copied when the node has both
/// its cluster and its chains [soft 0x08463350 SI_vrtPolEdgUserDataCopyFromSetup].
pub fn vrt_pol_edg_user_data_copy_from_setup(m: &mut Mem, o: &mut dyn Outside, from: Ptr32<Model>, to: Ptr32<Model>) {
    for off in [0x408u32, 0x410, 0x418] {
        let (cluster, chains) = (m.u32(from.addr() + off), m.u32(from.addr() + off + 4));
        if cluster != 0 && chains != 0 {
            subel_user_data_copy_from_setup(m, o, cluster, chains, to.addr() + off, to.addr() + off + 4);
        }
    }
}

/// The clusters of a node duplicated for its copy [soft 0x08462ed4 klusterCopyFromSetup]:
/// `SI_modelklusterListDuplicate(from, 0, 1, the 16-bit setting at SI_glbDuplic + 0x24)`,
/// the list assigned to the copy, and the copy's cluster animation updated when the
/// original has one (+0x344).
pub fn kluster_copy_from_setup(m: &mut Mem, o: &mut dyn Outside, from: Ptr32<Model>, to: Ptr32<Model>) {
    let setting = m.i16(g::SI_GLB_DUPLIC.addr() + 0x24) as i32 as u32;
    let list = o.call(m, &callee::SI_MODELKLUSTER_LIST_DUPLICATE, &[from.addr(), 0, 1, setting]);
    o.call(m, &callee::SI_KLUSTER_LIST_ASSIGN_TO_MODEL, &[to.addr(), list]);
    if !m[from].cluster.is_null() {
        o.call(m, &callee::SI_CLUSTER_MODEL_UPDATE, &[to.addr()]);
    }
}

/// The animation of a node's transformation copied to its copy
/// [soft 0x08462f1c SI_modelTransfoAnimCopy]: the function-curve table is reset, ten
/// attributes are marked (1 to 6, 0x8b to 0x8d, 7: scaling, rotation, translation and the
/// path), the path is taken over (+0x2c8; during an export to the successor product it is
/// first looked up as the element related by 0x70), and the animation of the original's
/// element is copied to the copy's when the copy has one.
pub fn transfo_anim_copy(m: &mut Mem, o: &mut dyn Outside, from: Ptr32<Model>, to: Ptr32<Model>) {
    let id_from = m.local(4);
    let scratch = m.local(4);
    let id_to = m.local(4);
    o.call(m, &callee::SI_FCV_TABLE_SET, &[0]);
    for attribute in [1, 2, 3, 4, 5, 6, 0x8b, 0x8c, 0x8d, 7] {
        o.call(m, &callee::SI_ATTDICT_SET_FCURVE_FLAG, &[g::SI_ATD.addr(), attribute, 1]);
    }
    let mut element = 0;
    if m.i16(g::SI_ED_SUMATRA_TO_3X.addr()) != 0 {
        element = o.call(m, &callee::SI_ED_GET_ELEM_FROM_MODEL, &[from.addr(), id_from]);
        let chapter = super::models_chapter(m);
        let dict = elem_dict(m);
        let related = o.call(m, &callee::SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, element, 1, 1, 0x70, chapter, scratch, scratch]);
        if related != 0 {
            let path = m.u32(related + 4);
            m[from].path = Ptr32::new(path);
        }
    }
    let path = m[from].path.addr();
    if path != 0 {
        m[to].path = Ptr32::new(path);
        o.call(m, &callee::SI_PATH_ELEMENT_SET, &[to.addr(), 2, path]);
    }
    if m.i16(g::SI_ED_SUMATRA_TO_3X.addr()) == 0 {
        element = o.call(m, &callee::SI_ED_GET_ELEM_FROM_MODEL, &[from.addr(), id_from]);
    }
    let copy = o.call(m, &callee::SI_ED_GET_ELEM_FROM_MODEL, &[to.addr(), id_to]);
    if copy != 0 {
        let chapter = super::models_chapter(m);
        let (a, b) = (m.u32(id_from), m.u32(id_to));
        let dict = elem_dict(m);
        o.call(m, &callee::SI_ELEMDICT_COPY_ELEM_ANIM, &[dict, element, copy, chapter, a, b]);
    }
    m.release(12);
}

/// Appends a stored definition [soft 0x08467038 SI_modelAddDefinition]: a node without
/// definitions takes `kind` and gets an array of one pointer (`SI_malloc(4)`), otherwise
/// the array grows by one (`SI_realloc`); if the working definition was the one that had
/// been first (null without definitions) it becomes the first of the array now; then the
/// name's flag bits ([`defs::change_name`]). Answers 1.
pub fn add_definition(m: &mut Mem, node: Ptr32<Model>, kind: i32, definition: u32) -> u32 {
    let first;
    if m[node].nb_defs == 0 {
        m[node].kind = kind;
        let block = m.si_malloc(4);
        m[node].defs = Ptr32::new(block);
        first = 0;
    } else {
        first = m.u32(m[node].defs.addr());
        let block = m.si_realloc(m[node].defs.addr(), m[node].nb_defs as u32 * 4 + 4);
        m[node].defs = Ptr32::new(block);
    }
    let slot = m[node].defs.addr().wrapping_add(m[node].nb_defs as u32 * 4);
    m.set_u32(slot, definition);
    let n = m[node].nb_defs;
    m[node].nb_defs = n.wrapping_add(1);
    if m[node].definition.addr() == first {
        let v = m.u32(m[node].defs.addr());
        m[node].definition = Ptr32::new(v);
    }
    let name = m[node].name.addr();
    defs::change_name(m, node, name);
    1
}

/// The shapes of a node (its stored definitions after the first) copied to its copy, with
/// their animation [soft 0x08462c54 shapeCopyFromSetup]: the function-curve table is set
/// from the chapter of the shapes (`SI_cid` + 0x54); each further definition of a
/// geometry kind is copied and appended to the copy ([`add_definition`]); the copy gets a
/// shape element (`SI_edAddShape`), and when it has one the animation of the original's
/// shape element is copied to it and its weight curves renamed.
pub fn shape_copy_from_setup(m: &mut Mem, o: &mut dyn Outside, from: Ptr32<Model>, to: Ptr32<Model>) {
    let id_from = m.local(4);
    let id_to = m.local(4);
    let chapter = m[g::SI_CID][0x15];
    o.call(m, &callee::SI_SET_FCV_TABLE_ENTRIES_FROM_CHAP, &[chapter]);
    let kind = m[from].kind;
    let copy = match kind {
        1 => Some(&callee::SI_PATCH_COPY),
        2 => Some(&callee::SI_FACE_COPY),
        4 => Some(&callee::SI_MESH_COPY),
        6 => Some(&callee::SI_SPLINE_COPY),
        9 => Some(&callee::SI_NURBS_CRV_COPY),
        10 => Some(&callee::SI_NURBS_SRF_COPY),
        _ => None,
    };
    if let Some(copy) = copy {
        let mut i = 1i32;
        while i < m[from].nb_defs as i32 {
            let d = m.u32(m[from].defs.addr().wrapping_add(i as u32 * 4));
            let c = o.call(m, copy, &[d]);
            add_definition(m, to, kind, c);
            i += 1;
        }
    }
    o.call(m, &callee::SI_ED_ADD_SHAPE, &[to.addr()]);
    let shape = o.call(m, &callee::SI_ED_GET_ELEM_FROM_SHAPE, &[from.addr(), id_from]);
    let copy = o.call(m, &callee::SI_ED_GET_ELEM_FROM_SHAPE, &[to.addr(), id_to]);
    if copy != 0 {
        let chapter = m[g::SI_CID][0x15];
        let (a, b) = (m.u32(id_from), m.u32(id_to));
        let dict = elem_dict(m);
        o.call(m, &callee::SI_ELEMDICT_COPY_ELEM_ANIM, &[dict, shape, copy, chapter, a, b]);
        o.call(m, &callee::SI_UPDATE_WEIGHT_FCV_NAMES, &[copy, 0, 0xca]);
    }
    m.release(8);
}

/// The settings of the Duplicate command this file reads, as offsets into `SI_glbDuplic`
/// (16-bit unless said; the meaning is read from what each one switches on here).
mod duplic {
    /// Materials (tested as 32 bits for the materials, as 16 for the textures).
    pub const MATERIALS: u32 = 0x00;
    pub const TEXTURES_A: u32 = 0x06;
    /// 32 bits.
    pub const TEXTURES_B: u32 = 0x0c;
    pub const TEXTURES_C: u32 = 0x12;
    /// 32 bits.
    pub const TEXTURES_D: u32 = 0x18;
    pub const LATTICES: u32 = 0x1e;
    pub const CLUSTERS: u32 = 0x22;
    pub const USER_DATA: u32 = 0x26;
    pub const ANIMATION: u32 = 0x28;
    pub const SHAPES: u32 = 0x2a;
    pub const FITTINGS: u32 = 0x2c;
    /// 32 bits: 2 keeps a word of the element (+0x18), any other value clears it.
    pub const ELEMENT_WORD: u32 = 0x30;
}

/// Everything a copied tree takes over from its original by the Duplicate settings
/// [soft 0x084633e8 SI_modelCopyFromSetup]; `from` and `to` are two trees of the same
/// shape.
///
/// First the two scene elements (`__SI_Elem`, by named offsets): the 16-bit word at +0x12
/// of the record at +8, when both have one; when the original's element has a word at
/// +0x54 and both nodes have fathers whose elements exist, the two floats at +0xc and
/// +0x10 become (original − original's father) + copy's father, the difference kept in
/// the register, both rounded on their stores (the difference is stored first, so an
/// element that is its own operand is read after that store); the 16-bit word at +0x2a;
/// and the word at +0x18, kept or cleared by a setting.
///
/// Then, each by its setting: the transformation's animation; and, unless the copy's
/// algorithm is 0xf: shapes (only with more than one definition), clusters, materials,
/// textures, lattices (only with an active one), the envelope relation of an enveloped
/// copy, user data (the node's chain and the components'), fittings (only with an active
/// one); then the following brothers (not for a copy without a father) and the sons,
/// pairwise. The original's links are followed without a test.
pub fn copy_from_setup(m: &mut Mem, o: &mut dyn Outside, from: Ptr32<Model>, to: Ptr32<Model>) {
    let id = m.local(4);
    let set = m.local(4);
    let d = g::SI_GLB_DUPLIC.addr();
    let e_from = o.call(m, &callee::SI_ED_GET_ELEM_FROM_MODEL, &[from.addr(), id]);
    let e_to = o.call(m, &callee::SI_ED_GET_ELEM_FROM_MODEL, &[to.addr(), id]);
    if e_from != 0 && e_to != 0 {
        let (a, b) = (m.u32(e_from + 8), m.u32(e_to + 8));
        if a != 0 && b != 0 {
            let v = m.u16(a + 0x12);
            m.set_u16(b + 0x12, v);
        }
        if m.u32(e_from + 0x54) != 0 && !m[to].father.is_null() && !m[from].father.is_null() {
            let father = m[from].father.addr();
            let f_from = o.call(m, &callee::SI_ED_GET_ELEM_FROM_MODEL, &[father, id]);
            let father = m[to].father.addr();
            let f_to = o.call(m, &callee::SI_ED_GET_ELEM_FROM_MODEL, &[father, id]);
            if f_from != 0 && f_to != 0 {
                let t1 = x(m.f32(e_from + 0xc)) - m.f32(f_from + 0xc);
                m.set_f32(e_to + 0xc, t1.to_f32());
                let t2 = x(m.f32(e_from + 0x10)) - m.f32(f_from + 0x10);
                m.set_f32(e_to + 0x10, t2.to_f32());
                let v = (t1 + m.f32(f_to + 0xc)).to_f32();
                m.set_f32(e_to + 0xc, v);
                let v = (t2 + m.f32(f_to + 0x10)).to_f32();
                m.set_f32(e_to + 0x10, v);
            }
        }
        let v = m.u16(e_from + 0x2a);
        m.set_u16(e_to + 0x2a, v);
        if m.u32(d + duplic::ELEMENT_WORD) == 2 {
            let v = m.u32(e_from + 0x18);
            m.set_u32(e_to + 0x18, v);
        } else {
            m.set_u32(e_to + 0x18, 0);
        }
    }
    if m.u16(d + duplic::ANIMATION) != 0 {
        transfo_anim_copy(m, o, from, to);
    }
    if m[to].algo_type != 0xf {
        if m.u16(d + duplic::SHAPES) != 0 && m[from].nb_defs > 1 {
            shape_copy_from_setup(m, o, from, to);
        }
        if m.u16(d + duplic::CLUSTERS) != 0 {
            kluster_copy_from_setup(m, o, from, to);
        }
        if m.u32(d + duplic::MATERIALS) != 0 {
            o.call(m, &callee::SI_MATERIAL_COPY_SHARE_SETUP, &[from.addr(), to.addr()]);
        }
        let textures = m.u16(d + duplic::TEXTURES_A) != 0 || m.u32(d + duplic::TEXTURES_B) != 0;
        if (m.u16(d + duplic::MATERIALS) != 0 && textures) || m.u16(d + duplic::TEXTURES_C) != 0 || m.u32(d + duplic::TEXTURES_D) != 0 {
            o.call(m, &callee::SI_TXT_COPY_FROM_SETUP, &[from.addr(), to.addr(), 0, 0]);
        }
        if m.u16(d + duplic::LATTICES) != 0 && defs::lattice(m, from, set) != 0 {
            o.call(m, &callee::SI_LATTICE_COPY_FROM_SETUP, &[from.addr(), to.addr()]);
        }
        let envelope = m[to].envelop;
        if (envelope.wrapping_sub(5) as u32) < 2 || envelope == 8 {
            o.call(m, &callee::SI_ED_GET_ELEM_FROM_MODEL, &[to.addr(), id]);
            let v = m.u32(id);
            let dict = elem_dict(m);
            o.call(m, &callee::SI_ELEMDICT_ADD_RELATION, &[dict, g::SI_RD.addr(), v, v, 0xcf, 1, 0]);
        }
        if m.u16(d + duplic::USER_DATA) != 0 {
            let chain = m[from].user_data.addr();
            if chain != 0 {
                user_data_copy_from_setup(m, chain, super::at!(to, user_data));
            }
            vrt_pol_edg_user_data_copy_from_setup(m, o, from, to);
        }
        if m.u16(d + duplic::FITTINGS) != 0 && (defs::spline_fitting(m, from, set) != 0 || defs::patch_fitting(m, from, set) != 0) {
            o.call(m, &callee::SI_DEFORMATION_COPY_FROM_SETUP, &[from.addr(), to.addr()]);
        }
        if !m[to].father.is_null() && !m[to].brother.is_null() {
            let (a, b) = (m[from].brother, m[to].brother);
            copy_from_setup(m, o, a, b);
        }
        if !m[to].son.is_null() {
            let (a, b) = (m[from].son, m[to].son);
            copy_from_setup(m, o, a, b);
        }
    }
    m.release(8);
}
