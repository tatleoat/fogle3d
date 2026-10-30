// SPDX-License-Identifier: MIT
//! The relations the program adds to a dictionary by itself, for the topological sort
//! (`edtools.c`): `SI_dbCheckExtraUnToldRelation` and what it calls. The sort calls it with
//! `on` 1 before it orders the update list and with 0 after; between the two calls the
//! dictionary holds relations no description names, and the update list keeps the nodes
//! they gave it (relation 120 `InvKin`, which gives an effector its solve; 121 `JntModel`;
//! the untold dependencies −102 … −110 and −400; 206 and 207 for envelopes).
//!
//! Read from the decompile and the instruction listings (finding 0018, section 19). Every
//! routine the original calls goes through [`ByAddr`] by its address, as the original
//! calls it, except this crate's own ports of `edtools.c` ([`crate::edtools`]); the
//! answers are the loader's (`crate::image::Prog`). Checked against the emulated original
//! on 18 sample scenes' images (finding 0018, section 20: `re/tools/si-scene/emu_scene8.py`,
//! `scene-worldcheck`): every routine cited here agreed on every case compared.
//! `edCnsSetJntMdlExtraRel` is not cited: no sample scene reaches its path that adds a
//! relation (105 of its 278 bytes were run).
//!
//! The product-tier offset at the loop's start (`SI_lightLicense − SI_lightLicense2`) is
//! taken as the full product's, 0 (AGENTS.md: the Rust takes the full product's path).

use crate::edtools::{self, ByAddr};
use crate::world::current_dict;
use si_dict::globals::{SI_CID, SI_GLB_MANIP, SI_RD};
use si_mem::{Mem, Ptr32};
use si_types::dict::{Elem, ElemDict};

/// `SI_elemdictAddRelation (0x08132f78)`: (dictionary, relation dictionary, from, to, relation, rule, position).
pub const SI_ELEMDICT_ADD_RELATION: u32 = 0x0813_2f78;
/// `SI_elemdictRemoveRelationPair (0x0813362c)`: (dictionary, relation dictionary, from, to, relation, rule, unused).
pub const SI_ELEMDICT_REMOVE_RELATION_PAIR: u32 = 0x0813_362c;
/// `SI_elemdictRemoveRelationId (0x08133998)`: (dictionary, relation dictionary, from, relation).
pub const SI_ELEMDICT_REMOVE_RELATION_ID: u32 = 0x0813_3998;
/// `SI_elemdictFindRelatedElem (0x0813508c)`, `SI_elemdictFindNextRelatedElem (0x08135154)`:
/// (dictionary, element, forward, list, relation, chapter, &chapter out, &index out).
pub const SI_ELEMDICT_FIND_RELATED_ELEM: u32 = 0x0813_508c;
pub const SI_ELEMDICT_FIND_NEXT_RELATED_ELEM: u32 = 0x0813_5154;
/// `SI_reldictIsRelActive (0x0813a56c)`: (relation dictionary, relation, dependency).
pub const SI_RELDICT_IS_REL_ACTIVE: u32 = 0x0813_a56c;
/// `SI_skeChainRoot (0x081d20c0)`, `skeleton.c`: (node).
pub const SI_SKE_CHAIN_ROOT: u32 = 0x081d_20c0;
/// `SI_invKinIsBreakJnt (0x084ae3ac)`, `invkin.c`: (node, flag, time as a float).
pub const SI_INV_KIN_IS_BREAK_JNT: u32 = 0x084a_e3ac;
/// `SI_mainGlobEnv (0x084bb94c)`, `SI_envelopLevel (0x084bb7b0)`, `SI_envelopActive
/// (0x084bb9b4)`, `envtools.c`: (node).
pub const SI_MAIN_GLOB_ENV: u32 = 0x084b_b94c;
pub const SI_ENVELOP_LEVEL: u32 = 0x084b_b7b0;
pub const SI_ENVELOP_ACTIVE: u32 = 0x084b_b9b4;
/// `SI_edIsItemConstrained (0x08106bbc)`: (dictionary, object, chapter, relation, flag, time as a float).
pub const SI_ED_IS_ITEM_CONSTRAINED: u32 = 0x0810_6bbc;
/// `SI_findGlData (0x0832ee68)`, `graflist.c`: (graph list, object, kind).
pub const SI_FIND_GL_DATA: u32 = 0x0832_ee68;
/// `SI_modelNextSon (0x0846d70c)`, `model.c`: (node, son).
pub const SI_MODEL_NEXT_SON: u32 = 0x0846_d70c;
/// `SI_edMdlChapRelBackUp (0x08105c0c)`, `SI_edSwapUnToldRel (0x0810594c)`, `edtools.c`:
/// the back-up of the model elements' relations the optimised path keeps (ported below).
pub const SI_ED_MDL_CHAP_REL_BACK_UP: u32 = 0x0810_5c0c;
pub const SI_ED_SWAP_UN_TOLD_REL: u32 = 0x0810_594c;

/// `SI_grafLists` (0x08679498): the pointer to the array of graph lists, 0x24 bytes each.
const SI_GRAF_LISTS: u32 = 0x0867_9498;
/// `SI_procData` (0x08679314): the pointer to the per-process words.
const SI_PROC_DATA: u32 = 0x0867_9314;

/// The model node's words this file reads.
mod node {
    pub const KIND: u32 = 0x30;
    pub const GEOM: u32 = 0x48;
    pub const FLAG_54: u32 = 0x54;
    pub const WORD_64: u32 = 0x64;
    pub const FATHER: u32 = 0x2bc;
    pub const BROTHER: u32 = 0x2c0;
    pub const SON: u32 = 0x2c4;
    pub const WORD_2DC: u32 = 0x2dc;
    /// The skeleton kind (+0x2ec): 3 and 4 are effectors, 1 and 2 roots.
    pub const SKE: u32 = 0x2ec;
    /// The envelope's kind (+0x31c).
    pub const ENV_KIND: u32 = 0x31c;
    pub const ENVELOPE: u32 = 0x324;
    pub const MASTER: u32 = 0x402;
}

/// `SI_glbManip` +0x2e (−110 `ChainUpdate`) and +0x36 (the optimised path).
const MANIP_2E: u32 = 0x2e;
const MANIP_36: u32 = 0x36;

fn cur(m: &Mem) -> u32 {
    current_dict(m).addr()
}

fn chapter_list(m: &Mem, dict: u32, chap: i32) -> u32 {
    m.u32(dict + 4).wrapping_add((chap as u32).wrapping_mul(0x40))
}

fn add(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, from: i32, to: i32, relid: i32) {
    let d = cur(m);
    o.call_addr(m, SI_ELEMDICT_ADD_RELATION, &[d, SI_RD.addr(), from as u32, to as u32, relid as u32, 1, 0]);
}

fn remove_pair(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, from: i32, to: i32, relid: i32) {
    let d = cur(m);
    o.call_addr(m, SI_ELEMDICT_REMOVE_RELATION_PAIR, &[d, SI_RD.addr(), from as u32, to as u32, relid as u32, 1, 0]);
}

/// Add when `on`, else remove the pair.
fn pair(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, on: i16, from: i32, to: i32, relid: i32) {
    if on != 0 {
        add(o, m, from, to, relid);
    } else {
        remove_pair(o, m, from, to, relid);
    }
}

/// `SI_edGetElemFromModel(model, &index)` with the index in a local of the image's stack.
fn elem_index(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, model: u32) -> i32 {
    let l = m.local(4);
    let idx = Ptr32::<i32>::new(l);
    edtools::ed_get_elem_from_model(o, m, Ptr32::new(model), idx);
    let v = m[idx];
    m.release(4);
    v
}

/// `SI_elemdictFindRelatedElem` (or the next one) with its two result words in locals.
#[allow(clippy::too_many_arguments)]
fn related(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, next: bool, dict: u32, e: u32, forward: i32, group: i32, relid: i32, chap: i32) -> u32 {
    let l = m.local(8);
    let f = if next { SI_ELEMDICT_FIND_NEXT_RELATED_ELEM } else { SI_ELEMDICT_FIND_RELATED_ELEM };
    let r = o.call_addr(m, f, &[dict, e, forward as u32, group as u32, relid as u32, chap as u32, l, l + 4]);
    m.release(8);
    r
}

/// Down by first sons from `q`'s son to a null (+0x30 = 0) that is an effector (+0x2ec 3
/// or 4), as three of these routines walk. The original has no test for the end of the
/// sons: a subtree without such a node makes it read through a null pointer. Here the read
/// is made (the image records the fault) and `None` is answered: the original would have
/// been killed there.
fn effector_below(m: &mut Mem, q: u32) -> Option<u32> {
    let mut q = m.u32(q + node::SON);
    loop {
        if q == 0 {
            m.u32(node::KIND);
            return None;
        }
        if m.i32(q + node::KIND) != 0 || m.i32(q + node::SKE).wrapping_sub(3) as u32 > 1 {
            q = m.u32(q + node::SON);
            continue;
        }
        return Some(q);
    }
}

/// [soft 0x08104fb8 SI_dbCheckExtraUnToldRelation](dictionary, on, textures, flag): the plain
/// path ([`db_check_extra_untold_relation_plain`], with the flag 0) when `SI_glbManip`
/// +0x36 is 0, else the optimised one ([`db_check_extra_untold_relation_optimized`]).
pub fn db_check_extra_untold_relation(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, dict: u32, on: i16, textures: i16, flag: i16) {
    if m.i16(SI_GLB_MANIP.addr() + MANIP_36) == 0 {
        db_check_extra_untold_relation_plain(o, m, dict, on, textures, 0);
    } else {
        db_check_extra_untold_relation_optimized(o, m, dict, on, textures, flag);
    }
}

/// [soft 0x08105b64 dbCheckExtraUnToldRelationOptimized]: the plain path run only on the
/// first call that switches the relations on (`SI_ElemDict` +0x6c is the "on" mark, +0x6e
/// a second mark it clears), after the model elements' relations are backed up; otherwise
/// the backed-up relations are swapped in ([`ed_swap_untold_rel`]).
pub fn db_check_extra_untold_relation_optimized(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, dict: u32, on: i16, textures: i16, flag: i16) {
    if m.i16(dict + 0x6c) == 0 || on != 0 {
        if m.i32(dict + 0x6c) != 0 {
            if on == 0 || m.i16(dict + 0x6c) != 0 {
                return;
            }
            ed_mdl_chap_rel_back_up(o, m, dict, flag);
            m.set_i16(dict + 0x6c, 1);
            if flag != 0 {
                ed_swap_untold_rel(o, m, dict, on, textures);
            }
            db_check_extra_untold_relation_plain(o, m, dict, on, textures, flag);
            m.set_i16(dict + 0x6e, 0);
            return;
        }
        if on == 0 {
            return;
        }
    }
    ed_swap_untold_rel(o, m, dict, on, textures);
}

/// The back-up of the model elements' relation lists (`SI_ElemDict` +0x5c/+0x60 for the
/// lists at +0x68/+0x6c of an element, +0x64/+0x68 for those at +0x60/+0x64): one pair
/// (count, array of 0x18-byte lists) per element of the MODELS chapter.
const BACKUPS: [(u32, u32, u32, u32); 2] = [(0x5c, 0x60, 0x68, 0x6c), (0x64, 0x68, 0x60, 0x64)];

/// [soft 0x08105c0c SI_edMdlChapRelBackUp](dictionary, flag): the back-up made big enough
/// ([`ed_alloc_mdl_rel_back_up`]), then every model element's lists backed up
/// ([`ed_mdl_rel_back_up`]); with `flag`, only those whose father element (+0x50) is new.
/// The chapter's count is read once.
pub fn ed_mdl_chap_rel_back_up(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, dict: u32, flag: i16) {
    let models = m[SI_CID].model;
    let list = chapter_list(m, dict, models);
    let n = m.i32(list + 4);
    ed_alloc_mdl_rel_back_up(m, dict);
    for i in 0..n.max(0) {
        let e = m.u32(list + 8) + 0x84 * i as u32;
        let take = flag == 0 || {
            let father = o.call_addr(m, edtools::SI_ELEMDICT_GET_ELEM, &[dict, models as u32, m.u32(e + 0x50)]);
            father != 0 && m.i16(father + 0x46) == 1
        };
        if take {
            ed_mdl_rel_back_up(m, dict, e, i);
        }
    }
}

/// [soft 0x08105cb0 SI_edAllocMdlRelBackUp](dictionary): each of the two back-up arrays grown
/// (`SI_realloc`, 8 bytes an element) to the MODELS chapter's count, the new pairs cleared.
pub fn ed_alloc_mdl_rel_back_up(m: &mut Mem, dict: u32) {
    let n = m.i32(chapter_list(m, dict, m[SI_CID].model) + 4);
    for (count, array, _, _) in BACKUPS {
        if m.i32(dict + count) < n {
            let p = m.si_realloc(m.u32(dict + array), (n as u32).wrapping_mul(8));
            m.set_u32(dict + array, p);
            while m.i32(dict + count) < n {
                let k = m.i32(dict + count) as u32;
                m.set_u32(p + 8 * k, 0);
                m.set_u32(p + 8 * k + 4, 0);
                m.set_i32(dict + count, k as i32 + 1);
            }
        }
    }
}

/// [soft 0x08105d7c SI_edMdlRelBackUp](dictionary, element, index): for an element in use
/// with an object, each of its two relation-list arrays is copied into the element's pair
/// of the back-up (the array grown with `SI_realloc` to the element's count of lists; each
/// list's count, cursor, word +0x10 and flag copied, its entries copied into a block of the
/// back-up's list, grown with `SI_realloc` or made with `SI_malloc` for a list the back-up
/// did not have, whose capacity becomes the count), the back-up's count set, and then the
/// element and the back-up exchange the two arrays: the element works on the copy.
pub fn ed_mdl_rel_back_up(m: &mut Mem, dict: u32, e: u32, index: i32) {
    if m.i16(e + 0x48) != 0 || m.u32(e + 4) == 0 {
        return;
    }
    let i = index as u32;
    for (_, array, en, ep) in BACKUPS {
        let bk = |m: &Mem| m.u32(dict + array).wrapping_add(8 * i);
        if m.i32(bk(m)) < m.i32(e + en) {
            let p = m.si_realloc(m.u32(bk(m) + 4), (m.i32(e + en) as u32).wrapping_mul(0x18));
            m.set_u32(bk(m) + 4, p);
        }
        let mut k = 0i32;
        let mut n = m.i32(e + en);
        while k < n {
            let off = 0x18 * k as u32;
            let src = m.u32(e + ep) + off;
            let dst = m.u32(bk(m) + 4) + off;
            m.set_u32(dst, m.u32(src));
            m.set_u32(dst + 8, m.u32(src + 8));
            m.set_u32(dst + 0x10, m.u32(src + 0x10));
            m.set_i16(dst + 0x14, m.i16(src + 0x14));
            let nb = m.i32(src);
            if k < m.i32(bk(m)) {
                if m.i32(dst + 4) < nb {
                    let p = m.si_realloc(m.u32(dst + 0xc), (nb as u32) << 5);
                    m.set_u32(dst + 0xc, p);
                    m.set_i32(dst + 4, nb);
                }
            } else {
                let p = m.si_malloc((nb as u32) << 5);
                m.set_u32(dst + 0xc, p);
                m.set_i32(dst + 4, nb);
            }
            if nb > 0 {
                let bytes = m.bytes(m.u32(src + 0xc), (nb as u32) << 5).to_vec();
                m.set_bytes(m.u32(dst + 0xc), &bytes);
            }
            k += 1;
            n = m.i32(e + en);
        }
        let b = bk(m);
        m.set_i32(b, n);
        let saved = m.u32(b + 4);
        m.set_u32(b + 4, m.u32(e + ep));
        m.set_u32(e + ep, saved);
    }
}

/// [soft 0x0810594c SI_edSwapUnToldRel](dictionary, on, textures): the "on" mark (+0x6c) =
/// `on`; with `textures` and `on` 0 the 2D textures' −400 pairs removed first; every model
/// element's arrays exchanged with the back-up's ([`ed_swap_mdl_untold_rel`], the chapter's
/// count read once); with `textures` and `on` set the −400 relations added after.
pub fn ed_swap_untold_rel(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, dict: u32, on: i16, textures: i16) {
    let first = 0; // the full product's offset
    m.set_i16(dict + 0x6c, on);
    if textures != 0 && on == 0 {
        textures_pass(o, m, dict, on);
    }
    let list = chapter_list(m, dict, m[SI_CID].model);
    let n = m.i32(list + 4);
    for i in first..n {
        let e = m.u32(list + 8) + 0x84 * i as u32;
        ed_swap_mdl_untold_rel(m, dict, e, i);
    }
    if textures != 0 && on != 0 {
        textures_pass(o, m, dict, on);
    }
}

/// The 2D textures' −400 pairs of [`ed_swap_untold_rel`]: for every element in use whose
/// object's word +0x28 is 1 or 2 (the chapter's count read at each step).
fn textures_pass(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, dict: u32, on: i16) {
    let list = chapter_list(m, dict, m[SI_CID].t2d);
    let mut i = 0;
    while i < m.i32(list + 4) {
        let e = m.u32(list + 8) + 0x84 * i as u32;
        if m.i16(e + 0x48) == 0 {
            let obj = m.u32(e + 4);
            if obj != 0 && m.u16(obj + 0x28).wrapping_sub(1) < 2 {
                pair(o, m, on, i, i, -400);
            }
        }
        i += 1;
    }
}

/// [soft 0x08105b10 SI_edSwapMdlUnToldRel](dictionary, element, index): for an element in use
/// with an object, each of its two arrays (+0x6c, +0x64) exchanged with the back-up's
/// (+0x60, +0x68 of the dictionary, the pointer of pair `index`) when `index` is at most
/// the back-up's count; **at most**: for `index` equal to the count the original reads
/// and writes the pair behind the array's end, and so does this.
pub fn ed_swap_mdl_untold_rel(m: &mut Mem, dict: u32, e: u32, index: i32) {
    if m.i16(e + 0x48) != 0 || m.u32(e + 4) == 0 {
        return;
    }
    for (count, array, _, ep) in BACKUPS {
        if index <= m.i32(dict + count) {
            let slot = m.u32(dict + array).wrapping_add(4 + 8 * index as u32);
            let saved = m.u32(e + ep);
            m.set_u32(e + ep, m.u32(slot));
            m.set_u32(slot, saved);
        }
    }
}

/// [soft 0x08105000 dbCheckExtraUnToldRelation](dictionary, on, textures, flag): the "on"
/// mark (+0x6c) = `on`; the master flags cleared ([`ed_clear_all_master_flags`]) when the
/// second mark (+0x6e) is set, `on` is set and `flag` is 0; then every model element
/// ([`db_mdl_check_extra_untold_relation`]) — with `flag` set, only those whose father
/// element (+0x50) is new (+0x46 = 1); the chapter's count is read again at each step.
/// With `textures`: every 2D texture element in use whose object's word +0x28 is 1 or 2 is
/// related to itself by −400 (or the pair removed).
pub fn db_check_extra_untold_relation_plain(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, dict: u32, on: i16, textures: i16, flag: i16) {
    let first = 0; // the full product's offset
    m.set_i16(dict + 0x6c, on);
    if m.i16(dict + 0x6e) != 0 && on != 0 && flag == 0 {
        ed_clear_all_master_flags(m, dict);
    }
    let models = m[SI_CID].model;
    let list = chapter_list(m, dict, models);
    let mut i = first;
    while i < m.i32(list + 4) {
        let e = o.call_addr(m, edtools::SI_ELEMDICT_GET_ELEM, &[dict, models as u32, i as u32]);
        if e != 0 {
            let father = o.call_addr(m, edtools::SI_ELEMDICT_GET_ELEM, &[dict, models as u32, m.u32(e + 0x50)]);
            if flag == 0 || (father != 0 && m.i16(father + 0x46) == 1) {
                db_mdl_check_extra_untold_relation(o, m, dict, e, i, on);
            }
        }
        i += 1;
    }
    if textures != 0 {
        let list = chapter_list(m, dict, m[SI_CID].t2d);
        let mut i = first;
        while i < m.i32(list + 4) {
            let e = m.u32(list + 8) + 0x84 * i as u32;
            if m.i16(e + 0x48) == 0 {
                let obj = m.u32(e + 4);
                if obj != 0 && m.u16(obj + 0x28).wrapping_sub(1) < 2 {
                    pair(o, m, on, i, i, -400);
                }
            }
            i += 1;
        }
    }
}

/// [soft 0x081051b0 SI_dbMdlCheckExtraUnToldRelation](dictionary, element, index, on): for a
/// model element in use with a node:
///
/// 1. an envelope's shape relations ([`ed_add_env_shp_relation`] on the sons) when the
///    node is an envelope of kind 5 whose father's kind is 4 or 11, or a main global
///    envelope (`SI_mainGlobEnv`), and has sons;
/// 2. an effector (a null of skeleton kind 3 or 4): 120 `InvKin` to its chain root's
///    element (`SI_skeChainRoot`), and with `SI_glbManip` +0x2e set −110 back; with `on` 0
///    every 120 of the element is removed (`SI_elemdictRemoveRelationId`) and the −110 pair;
/// 3. a break joint (`SI_invKinIsBreakJnt(node, 0, 0.0)`): 120 to the chain root;
/// 4. a node that is neither a joint (+0x30 = 5) nor an effector, whose father is a joint:
///    121 `JntModel` to the effector below the father ([`effector_below`]);
/// 5. the constraints that make a joint's chain depend on the node
///    ([`ed_cns_set_jnt_mdl_extra_rel`]: 111, 114, 116, 20010 on list 1; 20010, 246 on list 2);
/// 6. envelopes ([`ed_model_set_skel_env`]) for a node with an envelope level that is not a
///    skeleton root; the hierarchy dependencies ([`ed_model_set_hierarchy_dep`]) −105 for a
///    node with +0x64 or +0x2dc or the 16-bit +0x54 set (twice when both), −102 and −103
///    for a root with a 117 or 118 relation (when those relations are active), −108 for
///    one with an animation relation 12, else −106 and −107 on itself for one with 10, −104
///    for one with 250 or 251 on list 2;
/// 7. with `on`, the master flag ([`ed_set_master_flag`]) of a node with a 20002 relation
///    on the backward list 4.
pub fn db_mdl_check_extra_untold_relation(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, dict: u32, e: u32, index: i32, on: i16) {
    if m.i16(e + 0x48) != 0 {
        return;
    }
    let model = m.u32(e + 4);
    if model == 0 {
        return;
    }
    let cid_model = m[SI_CID].model;
    // 1. shapes of envelopes
    let father = m.u32(model + node::FATHER);
    let kind5 = m.i32(model + node::ENV_KIND) == 5 && father != 0 && [4, 0xb].contains(&m.i32(father + node::ENV_KIND));
    if (kind5 || o.call_addr(m, SI_MAIN_GLOB_ENV, &[model]) as i16 != 0) && m.u32(model + node::SON) != 0 {
        let son = m.u32(model + node::SON);
        ed_add_env_shp_relation(o, m, e, model, index, son, on);
    }
    let chain_update = m.i16(SI_GLB_MANIP.addr() + MANIP_2E) != 0;
    // 2. an effector
    if m.i32(model + node::KIND) == 0 && m.i32(model + node::SKE).wrapping_sub(3) as u32 <= 1 {
        let root = o.call_addr(m, SI_SKE_CHAIN_ROOT, &[model]);
        let r = elem_index(o, m, root);
        if on == 0 {
            let d = cur(m);
            o.call_addr(m, SI_ELEMDICT_REMOVE_RELATION_ID, &[d, SI_RD.addr(), index as u32, 0x78]);
            if chain_update {
                remove_pair(o, m, r, index, -0x6e);
            }
        } else {
            add(o, m, index, r, 0x78);
            if chain_update {
                add(o, m, r, index, -0x6e);
            }
        }
    }
    // 3. a break joint
    if o.call_addr(m, SI_INV_KIN_IS_BREAK_JNT, &[model, 0, 0]) as i16 != 0 {
        let root = o.call_addr(m, SI_SKE_CHAIN_ROOT, &[model]);
        if on == 0 {
            let d = cur(m);
            o.call_addr(m, SI_ELEMDICT_REMOVE_RELATION_ID, &[d, SI_RD.addr(), index as u32, 0x78]);
        } else {
            let r = elem_index(o, m, root);
            add(o, m, index, r, 0x78);
        }
    }
    // 4. a node under a joint
    let father = m.u32(model + node::FATHER);
    let kind = m.i32(model + node::KIND);
    let effector = kind == 0 && m.i32(model + node::SKE).wrapping_sub(3) as u32 <= 1;
    if father != 0 && kind != 5 && !effector && m.i32(father + node::KIND) == 5 {
        let Some(q) = effector_below(m, father) else { return };
        let r = elem_index(o, m, q);
        pair(o, m, on, index, r, 0x79);
    }
    // 5. constraints of joints
    for (group, relid) in [(1, 0x6f), (1, 0x72), (1, 0x74), (1, 0x4e2a), (2, 0x4e2a), (2, 0xf6)] {
        let d = cur(m);
        ed_cns_set_jnt_mdl_extra_rel(o, m, d, model, index, group, relid, on);
    }
    // 6. envelopes and the hierarchy dependencies
    let level = o.call_addr(m, SI_ENVELOP_LEVEL, &[model]) as i32;
    if level >= 0 && (m.i32(model + node::KIND) != 0 || m.i32(model + node::SKE).wrapping_sub(1) as u32 > 1) {
        ed_model_set_skel_env(o, m, model, index, on);
    }
    if m.u32(model + node::WORD_64) != 0 || m.u32(model + node::WORD_2DC) != 0 {
        ed_model_set_hierarchy_dep(o, m, model, index, e, model, index, on, -0x69);
    }
    if m.i16(model + node::FLAG_54) != 0 {
        ed_model_set_hierarchy_dep(o, m, model, index, e, model, index, on, -0x69);
    }
    if m.u32(model + node::FATHER) == 0 {
        let mut a = 0;
        let mut b = 0;
        if o.call_addr(m, SI_RELDICT_IS_REL_ACTIVE, &[SI_RD.addr(), 0x75, 1]) as i16 != 0 {
            let d = cur(m);
            a = related(o, m, false, d, e, 1, 1, 0x75, cid_model);
        }
        if o.call_addr(m, SI_RELDICT_IS_REL_ACTIVE, &[SI_RD.addr(), 0x76, 1]) as i16 != 0 {
            let d = cur(m);
            b = related(o, m, false, d, e, 1, 1, 0x76, cid_model);
        }
        if a != 0 || b != 0 {
            ed_model_set_hierarchy_dep(o, m, model, index, e, model, index, on, -0x66);
            ed_model_set_hierarchy_dep(o, m, model, index, e, model, index, on, -0x67);
        }
    }
    let ani = m[SI_CID].ani;
    let d = cur(m);
    if related(o, m, false, d, e, 1, 0, 0xc, ani) != 0 {
        ed_model_set_hierarchy_dep(o, m, model, index, e, model, index, on, -0x6c);
    } else {
        let d = cur(m);
        if related(o, m, false, d, e, 1, 0, 0xa, ani) != 0 {
            pair(o, m, on, index, index, -0x6a);
            pair(o, m, on, index, index, -0x6b);
        }
    }
    if related(o, m, false, dict, e, 1, 2, 0xfa, -1) != 0 || related(o, m, false, dict, e, 1, 2, 0xfb, -1) != 0 {
        ed_model_set_hierarchy_dep(o, m, model, index, e, model, index, on, -0x68);
    }
    // 7. masters of instances
    if on != 0 && related(o, m, false, dict, e, 0, 4, 0x4e22, cid_model) != 0 {
        ed_set_master_flag(o, m, model);
    }
}

/// [soft 0x08104cf0 edAddEnvShpRelation](element, envelope, index, node, on): a node of the
/// same envelope kind (+0x31c) that is not a main global envelope gets −107 from the
/// element (or the pair removed), and its sons are walked; its brothers are walked in any
/// case.
pub fn ed_add_env_shp_relation(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, e: u32, model: u32, index: i32, n: u32, on: i16) {
    if m.i32(model + node::ENV_KIND) == m.i32(n + node::ENV_KIND) && o.call_addr(m, SI_MAIN_GLOB_ENV, &[n]) as i16 == 0 {
        let r = elem_index(o, m, n);
        pair(o, m, on, index, r, -0x6b);
        let son = m.u32(n + node::SON);
        if son != 0 {
            ed_add_env_shp_relation(o, m, e, model, index, son, on);
        }
    }
    let brother = m.u32(n + node::BROTHER);
    if brother != 0 {
        ed_add_env_shp_relation(o, m, e, model, index, brother, on);
    }
}

/// [soft 0x080fe0e8 edModelSetHierarchyDep](node, index, element, node2, index2, on, relation):
/// when the relation applies to `node2` (for −102: it has no sons; else it has a geometry,
/// +0x48), relation `relation` from `index` to `index2` (for −104 and −108 the other way,
/// and for −108 first −106 and −107 on `index2` itself), added or removed; −105 becomes
/// −104 for this call when the element has a 245 relation on list 2. Then the same for
/// every son element of `element`, with the son's node and index as `node2`, `index2`.
#[allow(clippy::too_many_arguments)]
pub fn ed_model_set_hierarchy_dep(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, model: u32, index: i32, e: u32, model2: u32, index2: i32, on: i16, relid: i32) {
    let mut relid = relid;
    let applies = if relid == -0x66 { m.u32(model2 + node::SON) == 0 } else { m.u32(model2 + node::GEOM) != 0 };
    if applies {
        let mut changed = false;
        if relid == -0x69 {
            let d = cur(m);
            let cid_model = m[SI_CID].model;
            // both result words into one local
            let l = m.local(4);
            let r = o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[d, e, 1, 2, 0xf5, cid_model as u32, l, l]);
            m.release(4);
            if r != 0 {
                relid = -0x68;
                changed = true;
            }
        }
        let (mut from, mut to) = (index, index2);
        if relid == -0x68 || relid == -0x6c {
            (from, to) = (index2, index);
            if relid == -0x6c {
                pair(o, m, on, from, from, -0x6a);
                pair(o, m, on, from, from, -0x6b);
            }
        }
        pair(o, m, on, from, to, relid);
        if changed {
            relid = -0x69;
        }
    }
    let mut son = edtools::ed_find_son(o, m, Ptr32::new(e));
    while !son.is_null() {
        let node2 = m.u32(son.addr() + 4);
        let i2 = elem_index(o, m, node2);
        ed_model_set_hierarchy_dep(o, m, model, index, son.addr(), node2, i2, on, relid);
        son = edtools::ed_find_next_son(o, m, Ptr32::new(e));
    }
}

/// [soft 0x0810057c edModelSetSkelEnv](node, index, on): for a node with an envelope record
/// (+0x324) that is not a joint, or a joint constrained by 111, 112, 113 or 160
/// (`SI_edIsItemConstrained`, flag 0, time 0.0): an effector whose son one envelope level
/// down has a graph node that is not of kind 1 (`SI_findGlData`) stops here when
/// `SI_glbManip` +0x2e is set; then each model of the envelope's list (+4, next at +0xc)
/// that is active (`SI_envelopActive`) or of envelope kind 7, and is not a kind-7 main
/// global envelope, gets 206 (kind 7) or 207 to `index` (or the pair removed).
pub fn ed_model_set_skel_env(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, model: u32, index: i32, on: i16) {
    let env = m.u32(model + node::ENVELOPE);
    if env == 0 {
        return;
    }
    let cid_model = m[SI_CID].model as u32;
    let mut go = m.i32(model + node::KIND) != 5;
    for relid in [0x6f, 0x70, 0x71, 0xa0] {
        if go {
            break;
        }
        let d = cur(m);
        go = o.call_addr(m, SI_ED_IS_ITEM_CONSTRAINED, &[d, model, cid_model, relid, 0, 0]) as i16 != 0;
    }
    if !go {
        return;
    }
    if m.i32(model + node::KIND) == 0 && m.i32(model + node::SKE).wrapping_sub(3) as u32 <= 1 {
        let level = o.call_addr(m, SI_ENVELOP_LEVEL, &[model]) as i32;
        let mut p = m.u32(model + node::SON);
        while p != 0 {
            if o.call_addr(m, SI_ENVELOP_LEVEL, &[p]) as i32 == level.wrapping_add(1) {
                let lists = m.u32(SI_GRAF_LISTS);
                let which = m.i32(m.u32(SI_PROC_DATA) + 4);
                let g = o.call_addr(m, SI_FIND_GL_DATA, &[lists.wrapping_add((which as u32).wrapping_mul(0x24)), model, 0]);
                if m.i16(SI_GLB_MANIP.addr() + MANIP_2E) != 0 && m.i32(g + 8) != 1 {
                    return;
                }
                break;
            }
            p = m.u32(p + node::BROTHER);
        }
    }
    let mut q = m.u32(m.u32(model + node::ENVELOPE) + 4);
    while q != 0 {
        let mm = m.u32(q);
        if mm != 0 {
            let active = o.call_addr(m, SI_ENVELOP_ACTIVE, &[mm]) as i16 != 0;
            if (active || m.i32(mm + node::ENV_KIND) == 7) && (m.i32(mm + node::ENV_KIND) != 7 || o.call_addr(m, SI_MAIN_GLOB_ENV, &[mm]) as i16 == 0) {
                let r = elem_index(o, m, mm);
                let relid = if m.i32(mm + node::ENV_KIND) == 7 { 0xce } else { 0xcf };
                pair(o, m, on, r, index, relid);
            }
        }
        q = m.u32(q + 0xc);
    }
}

/// `edCnsSetJntMdlExtraRel (0x08104df8)(dictionary, node, index, list, relation, on)`;
/// unchecked (no sample scene reaches its adding path):
/// every item that constrains the node by `relation` on the forward list `list`
/// ([`ed_get_constraining_item`]) that is a model (kinds 2 and 3) and a joint (+0x30 = 5)
/// makes the node's element depend on the effector below the joint by 121 (added or the
/// pair removed).
#[allow(clippy::too_many_arguments)]
pub fn ed_cns_set_jnt_mdl_extra_rel(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, dict: u32, model: u32, index: i32, group: i32, relid: i32, on: i16) {
    let kind_at = m.local(4);
    let cid_model = m[SI_CID].model;
    let mut item = ed_get_constraining_item(o, m, dict, model, cid_model, group, relid, true, kind_at);
    while item != 0 {
        if m.i32(kind_at).wrapping_sub(2) as u32 <= 1 && m.i32(item + node::KIND) == 5 {
            let Some(q) = effector_below(m, item) else { break };
            let r = elem_index(o, m, q);
            pair(o, m, on, index, r, 0x79);
        }
        item = ed_get_constraining_item(o, m, dict, model, cid_model, group, relid, false, kind_at);
    }
    m.release(4);
}

/// [soft 0x08106a70 edGetConstrainingItem](dictionary, item, chapter, list, relation, first,
/// &kind): the item's element (a model, camera or light: `SI_edGetElemFromModel`,
/// `…Camera`, `…Light`), then the first (or next) element related to it on the forward
/// list by the relation, in any chapter; the answer is that element's object, and `kind`
/// says what it is: a model 2 (3 with +0x54 set on the element), a camera 5 (4), a light 8
/// when its +0x1e is 1, else 7 (6). `kind` is 0 and the answer null when there is none.
/// The original leaves the element unset for another chapter; it is only called with
/// MODELS here.
#[allow(clippy::too_many_arguments)]
pub fn ed_get_constraining_item(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, dict: u32, item: u32, chap: i32, group: i32, relid: i32, first: bool, kind: u32) -> u32 {
    m.set_i32(kind, 0);
    let l = m.local(8);
    let (oc, idx) = (l, l + 4);
    let cid = m[SI_CID];
    let e = if chap == cid.model {
        edtools::ed_get_elem_from_model(o, m, Ptr32::new(item), Ptr32::new(idx)).addr()
    } else if chap == cid.cam {
        edtools::ed_get_elem_from_camera(o, m, item, Ptr32::new(idx)).addr()
    } else if chap == cid.lig {
        edtools::ed_get_elem_from_light(o, m, item, Ptr32::new(idx)).addr()
    } else {
        0
    };
    let mut out = 0;
    if e != 0 {
        let f = if first { SI_ELEMDICT_FIND_RELATED_ELEM } else { SI_ELEMDICT_FIND_NEXT_RELATED_ELEM };
        let r = o.call_addr(m, f, &[dict, e, 1, group as u32, relid as u32, u32::MAX, oc, idx]);
        if r != 0 {
            let c = m.i32(oc);
            if c == cid.model {
                m.set_i32(kind, if m.u32(r + 0x54) == 0 { 2 } else { 3 });
            } else if c == cid.cam {
                m.set_i32(kind, if m.u32(r + 0x54) == 0 { 5 } else { 4 });
            } else if c == cid.lig {
                let v = if m.i16(m.u32(r + 4) + 0x1e) == 1 {
                    8
                } else if m.u32(r + 0x54) == 0 {
                    7
                } else {
                    6
                };
                m.set_i32(kind, v);
            }
            out = m.u32(r + 4);
        }
    }
    m.release(8);
    out
}

/// [soft 0x08104f68 edSetMasterFlag](node): a node whose 16-bit word +0x402 is 0 gets 1 there,
/// and so does each of its sons (`SI_modelNextSon`).
pub fn ed_set_master_flag(o: &mut (impl ByAddr + ?Sized), m: &mut Mem, model: u32) {
    if model == 0 || m.i16(model + node::MASTER) != 0 {
        return;
    }
    m.set_i16(model + node::MASTER, 1);
    let mut p = 0;
    loop {
        p = o.call_addr(m, SI_MODEL_NEXT_SON, &[model, p]);
        if p == 0 {
            break;
        }
        m.set_i16(p + node::MASTER, 1);
    }
}

/// [soft 0x08104f18 edClearAllMasterFlags](dictionary): +0x402 of every model element's node
/// in use cleared.
pub fn ed_clear_all_master_flags(m: &mut Mem, dict: u32) {
    let list = chapter_list(m, dict, m[SI_CID].model);
    for i in 0..m.i32(list + 4).max(0) as u32 {
        let e = m.u32(list + 8) + 0x84 * i;
        if m.i16(e + 0x48) == 0 {
            let obj = m.u32(e + 4);
            if obj != 0 {
                m.set_i16(obj + node::MASTER, 0);
            }
        }
    }
}

/// The element pointer type of [`crate::edtools`], for callers.
pub type ElemPtr = Ptr32<Elem>;
/// The dictionary pointer type, for callers.
pub type DictPtr = Ptr32<ElemDict>;
