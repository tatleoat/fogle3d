// SPDX-License-Identifier: MIT
//! `envtools.c`: the envelope records (allocate, copy, free), the questions asked of a
//! node about its envelope (which skeleton, which level, active or not) and the working
//! copy of a skin's geometry.
//!
//! An envelope is a skin (a mesh, patch, spline or NURBS node) whose points follow the
//! bones of a skeleton. The bones carry the records ([`EnvInfo`] at `Model` +0x324, with one
//! [`EnvList`] per skin); the skin carries only its envelope word (`Model` +0x31c,
//! [`word`]). Finding 0029 has the records and the meaning of the words.

use crate::cur_dict;
use crate::env_outside::*;
use crate::env_records::{word, EnvInfo, EnvList, EnvVert};
use crate::globals::{CID_MODELS, SI_CID, SI_IDENTITE, SI_RD};
use crate::outside::{Outside, SI_ED_GET_ELEM_FROM_MODEL, SI_ELEMDICT_ADD_RELATION, SI_ELEMDICT_FIND_ELEM_BY_DATA, SI_ELEMDICT_FIND_RELATED_ELEM};
use si_math::algebra::matrix_copy;
use si_mem::{field, Mem, Ptr32};
use si_types::model::Model;
use si_x87::{fmov, x};

type Node = Ptr32<Model>;

/// A node's envelope record.
pub(crate) fn info(m: &Mem, node: Node) -> Ptr32<EnvInfo> {
    m[node].env_skel_jnt.cast()
}

/// "A null whose algorithm is 1 or 2": the root of a chain.
pub(crate) fn is_chain_root(m: &Mem, node: Node) -> bool {
    m[node].kind == 0 && (m[node].algo_type as u32).wrapping_sub(1) <= 1
}

/// "A null whose algorithm is 3 or 4": a chain's effector.
pub(crate) fn is_effector(m: &Mem, node: Node) -> bool {
    m[node].kind == 0 && (m[node].algo_type as u32).wrapping_sub(3) <= 1
}

/// The level of a node's envelope record [soft 0x084bb7b0 SI_envelopLevel]: −1 for no node
/// or a node without record.
pub fn envelop_level(m: &Mem, node: Node) -> i32 {
    if node.is_null() {
        return -1;
    }
    let e = info(m, node);
    if e.is_null() {
        return -1;
    }
    m[e].level
}

/// The topmost node of the envelope a skin belongs to
/// [soft 0x084bb7d4 SI_envGetRootFromEnvElem].
///
/// A global skin (word 6 or 7): the node itself or the nearest father that is the main node
/// of a global envelope ([`main_glob_env`]); null when there is none. A local skin (5 or
/// 8): the highest of the unbroken run of fathers with word 5 or 8, or the node itself.
/// Anything else: null.
pub fn env_get_root_from_env_elem(m: &Mem, node: Node) -> Node {
    if node.is_null() {
        return Node::NULL;
    }
    let mut cur = node;
    let w = m[cur].envelop;
    if (w as u32).wrapping_sub(6) <= 1 {
        loop {
            if main_glob_env(m, cur) as i16 != 0 {
                return cur;
            }
            cur = m[cur].father;
            if cur.is_null() {
                return cur;
            }
        }
    }
    if w == 5 || w == 8 {
        loop {
            let up = m[cur].father;
            if up.is_null() {
                return cur;
            }
            let w = m[up].envelop;
            if w != 5 && w != 8 {
                return cur;
            }
            cur = up;
        }
    }
    Node::NULL
}

/// The local skin of a chain [soft 0x084bb860 SI_envLocalGet]: the first of the first
/// joint's sons whose word is 5 or 8. For a chain's root the search starts at its first son
/// (which is read without a test for null); for any other node at the node itself.
pub fn env_local_get(m: &Mem, node: Node) -> Node {
    if node.is_null() {
        return Node::NULL;
    }
    let mut from = node;
    if is_chain_root(m, from) {
        from = m[from].son;
    }
    let mut s = m[from].son;
    while !s.is_null() {
        let w = m[s].envelop;
        if w == 5 || w == 8 {
            return s;
        }
        s = m[s].brother;
    }
    Node::NULL
}

/// The root of the skeleton a bone belongs to [soft 0x084bb8b0 SI_skelRootGet]; null for no
/// node, a node without record, or one whose word is not 4, 9, 10 or 11.
///
/// A bone without lists answers itself, and so does a chain's root whose first son has the
/// word 4. Otherwise the node and its fathers are searched for the first with word 4, 10 or
/// 11 (null when the top is passed); that one is the answer, except that a joint with word
/// 4 answers its father, the chain's root.
pub fn skel_root_get(m: &Mem, node: Node) -> Node {
    if node.is_null() {
        return Node::NULL;
    }
    let e = info(m, node);
    if e.is_null() || !matches!(m[node].envelop, 9 | 4 | 10 | 11) {
        return Node::NULL;
    }
    if m[e].lists.is_null() {
        return node;
    }
    if is_chain_root(m, node) {
        let son = m[node].son;
        if m[son].envelop == 4 {
            return node;
        }
    }
    let mut cur = node;
    loop {
        let w = m[cur].envelop;
        if matches!(w, 4 | 10 | 11) {
            if w != 4 || m[cur].kind != 5 {
                return cur;
            }
            return m[cur].father;
        }
        cur = m[cur].father;
        if cur.is_null() {
            return cur;
        }
    }
}

/// 1 when a node carries a global envelope whose own sons include its skeleton's root, else
/// 0 [soft 0x084bb94c SI_mainGlobEnv]: the node's word is 6 or 7, and one of its sons has a
/// record of level 0 and the word 4 or 10.
pub fn main_glob_env(m: &Mem, node: Node) -> i32 {
    if node.is_null() || (m[node].envelop as u32).wrapping_sub(6) > 1 {
        return 0;
    }
    let mut son = m[node].son;
    while !son.is_null() {
        if envelop_level(m, son) == 0 && matches!(m[son].envelop, 4 | 10) {
            return 1;
        }
        son = m[son].brother;
    }
    0
}

/// Whether a skin's envelope is switched on [soft 0x084bb9b4 SI_envelopActive]: 0 for a node
/// whose word is not 5, 6 or 8 and for one without element; else the 16-bit word at +0x14
/// of the relation of type 0xcf between the node's element and itself in the models'
/// chapter (0 when there is no such relation).
pub fn envelop_active(m: &mut Mem, o: &mut dyn Outside, node: Node) -> i32 {
    let w = m[node].envelop;
    if (w as u32).wrapping_sub(5) > 1 && w != 8 {
        return 0;
    }
    let index = m.local(4);
    let elem = o.call(m, &SI_ED_GET_ELEM_FROM_MODEL, &[node.addr(), index]);
    let mut answer = 0;
    if elem != 0 {
        let chapter = m[SI_CID][CID_MODELS] as u32;
        let i = m.u32(index);
        let dict = cur_dict(m);
        let rel = o.call(m, &SI_ELEMDICT_GET_RELATION_FROM_IDS, &[dict, elem, 0xcf, chapter, i, 0xffff_ffff]);
        if rel != 0 {
            answer = m.i16(rel.wrapping_add(0x14)) as i32;
        }
    }
    m.release(4);
    answer
}

/// A new envelope vertex [soft 0x084bba28 SI_envelopVertAllocate]: 0x14 bytes from
/// `SI_malloc`, cleared but for the weight, which is 100 (the float constant at
/// 0x08661108). The block is written without a test for null.
pub fn envelop_vert_allocate(m: &mut Mem) -> Ptr32<EnvVert> {
    let v: Ptr32<EnvVert> = Ptr32::new(m.si_malloc(0x14));
    m[v].ratio = 0.0;
    m[v].dist = 0.0;
    m[v].weight = FULL_WEIGHT;
    m[v].index = 0;
    m[v].next = Ptr32::NULL;
    v
}

/// A new envelope list [soft 0x084bba60 SI_envelopListAllocate]: 0x10 bytes from
/// `SI_malloc`, the skin in the first word, the rest cleared; written without a test for
/// null.
pub fn envelop_list_allocate(m: &mut Mem, owner: Node) -> Ptr32<EnvList> {
    let l: Ptr32<EnvList> = Ptr32::new(m.si_malloc(0x10));
    m[l].owner = owner;
    m[l].verts = Ptr32::NULL;
    m[l].prev = Ptr32::NULL;
    m[l].next = Ptr32::NULL;
    l
}

/// A new envelope record [soft 0x084bba94 SI_envelopInfoAllocate]: 0xa0 bytes from
/// `SI_malloc`; null when there is no memory. Level −1, no list, the identity matrix
/// (copied from `SI_identite`), the three vectors and the joint area 0, both profiles of
/// kind 0 without table and with the settings 0, 1, 0, 1.
pub fn envelop_info_allocate(m: &mut Mem) -> Ptr32<EnvInfo> {
    let e: Ptr32<EnvInfo> = Ptr32::new(m.si_malloc(0xa0));
    if e.is_null() {
        return e;
    }
    m[e].level = -1;
    m[e].lists = Ptr32::NULL;
    let identity = m[SI_IDENTITE];
    m[e].matrix = matrix_copy(&identity);
    m[e].v48 = [0.0; 3];
    m[e].saved_rot = [0.0; 3];
    m[e].v60 = [0.0; 3];
    m[e].joint_area = 0.0;
    for c in [field!(e, EnvInfo, inner), field!(e, EnvInfo, outer)] {
        m[c].lo = 0.0;
        m[c].hi = 1.0;
        m[c].amplitude = 0.0;
        m[c].scale = 1.0;
    }
    m[e].inner.kind = 0;
    m[e].inner.table = Ptr32::NULL;
    m[e].outer.kind = 0;
    m[e].outer.table = Ptr32::NULL;
    e
}

/// A copy of a chain of envelope vertices [soft 0x084bbb9c SI_envelopVertCopy]: null for an
/// empty chain, and null as soon as a vertex cannot be had (what was copied so far is
/// lost). The three floats go through the unit.
pub fn envelop_vert_copy(m: &mut Mem, from: Ptr32<EnvVert>) -> Ptr32<EnvVert> {
    if from.is_null() {
        return from;
    }
    let mut from = from;
    let mut head = Ptr32::NULL;
    let mut last: Ptr32<EnvVert> = Ptr32::NULL;
    loop {
        let v = envelop_vert_allocate(m);
        if v.is_null() {
            return v;
        }
        m[v].index = m[from].index;
        m[v].ratio = fmov(m[from].ratio);
        m[v].dist = fmov(m[from].dist);
        m[v].weight = fmov(m[from].weight);
        if last.is_null() {
            head = v;
        } else {
            m[last].next = v;
        }
        last = v;
        m[last].next = Ptr32::NULL;
        from = m[from].next;
        if from.is_null() {
            return head;
        }
    }
}

/// A copy of an envelope record [soft 0x084bbbfc envelopInfoCopy]; null for no record or
/// when no new record can be had.
///
/// With `lists`: the level is copied, and for each of the record's lists a new list **whose
/// skin is null**, with a copy of its vertices, chained in order. Without, the copy keeps
/// the level −1 and has no list. Either way the joint area, the two profiles' kinds and
/// settings (floats through the unit) and, where the source has them, the two tables of 200
/// floats (0x320 bytes from `SI_malloc`, written without a test for null) are copied. The
/// matrix and the three vectors stay as the allocator set them.
pub fn envelop_info_copy(m: &mut Mem, from: Ptr32<EnvInfo>, lists: i16) -> Ptr32<EnvInfo> {
    if from.is_null() {
        return from;
    }
    let to = envelop_info_allocate(m);
    if to.is_null() {
        return to;
    }
    if lists != 0 {
        m[to].level = m[from].level;
        let mut src = m[from].lists;
        let mut last: Ptr32<EnvList> = Ptr32::NULL;
        while !src.is_null() {
            let l = envelop_list_allocate(m, Node::NULL);
            let verts = m[src].verts;
            m[l].verts = envelop_vert_copy(m, verts);
            if m[to].lists.is_null() {
                m[to].lists = l;
            } else {
                m[last].next = l;
                m[l].prev = last;
            }
            last = l;
            src = m[src].next;
        }
    }
    m[to].joint_area = fmov(m[from].joint_area);
    for (a, b) in [(field!(from, EnvInfo, inner), field!(to, EnvInfo, inner)), (field!(from, EnvInfo, outer), field!(to, EnvInfo, outer))] {
        m[b].kind = m[a].kind;
        m[b].lo = fmov(m[a].lo);
        m[b].hi = fmov(m[a].hi);
        m[b].amplitude = fmov(m[a].amplitude);
        m[b].scale = fmov(m[a].scale);
    }
    for (a, b) in [(field!(from, EnvInfo, inner), field!(to, EnvInfo, inner)), (field!(from, EnvInfo, outer), field!(to, EnvInfo, outer))] {
        if !m[a].table.is_null() {
            m[b].table = Ptr32::new(m.si_malloc(0x320));
            for i in 0..200 {
                let v = fmov(m[m[a].table.add(i)]);
                let slot = m[b].table.add(i);
                m[slot] = v;
            }
        }
    }
    to
}

/// Frees a chain of envelope vertices and clears the pointer to it
/// [soft 0x084bbefc si_envelopVertListDispose].
pub fn envelop_vert_list_dispose(m: &mut Mem, slot: Ptr32<Ptr32<EnvVert>>) {
    let mut v = m[slot];
    if v.is_null() {
        return;
    }
    loop {
        let next = m[v].next;
        m.free(v.addr());
        v = next;
        if v.is_null() {
            break;
        }
    }
    m[slot] = Ptr32::NULL;
}

/// Takes one list out of a bone's chain and frees it with its vertices
/// [soft 0x084bbf38 envelopDispose]. Nothing for no list or no record.
pub fn envelop_dispose(m: &mut Mem, list: Ptr32<EnvList>, record: Ptr32<EnvInfo>) {
    if list.is_null() || record.is_null() {
        return;
    }
    envelop_vert_list_dispose(m, field!(list, EnvList, verts));
    let (prev, next) = (m[list].prev, m[list].next);
    m.free(list.addr());
    if prev.is_null() {
        m[record].lists = next;
    } else {
        m[prev].next = next;
    }
    if !next.is_null() {
        m[next].prev = prev;
    }
}

/// Frees a node's envelope record and clears the pointer to it
/// [soft 0x084bbf8c SI_envelopInfoDispose]: the vertices of every list, **the first list
/// only** (the records of the others are left allocated), the two tables, the record.
pub fn envelop_info_dispose(m: &mut Mem, slot: Ptr32<Ptr32<EnvInfo>>) {
    let e = m[slot];
    if e.is_null() {
        return;
    }
    let mut l = m[e].lists;
    if !l.is_null() {
        loop {
            envelop_vert_list_dispose(m, field!(l, EnvList, verts));
            l = m[l].next;
            if l.is_null() {
                break;
            }
        }
        let e = m[slot];
        let first = m[e].lists;
        if !first.is_null() {
            m.free(first.addr());
            let e = m[slot];
            m[e].lists = Ptr32::NULL;
        }
    }
    let e = m[slot];
    let t = m[e].inner.table;
    if !t.is_null() {
        m.free(t.addr());
        let e = m[slot];
        m[e].inner.table = Ptr32::NULL;
    }
    let e = m[slot];
    let t = m[e].outer.table;
    if !t.is_null() {
        m.free(t.addr());
        let e = m[slot];
        m[e].outer.table = Ptr32::NULL;
    }
    let e = m[slot];
    if !e.is_null() {
        m.free(e.addr());
        m[slot] = Ptr32::NULL;
    }
}

/// Takes a skin, or every skin, off the bones of a branch
/// [soft 0x084bc03c envelopInfoTreeDispose]. Only nodes whose record has the level expected
/// are looked at (`level` for `node`, one more for each generation of sons).
///
/// For no skin (`skin` null): the vertices of every list are freed and the first list's
/// record with them (the others are left allocated, as in [`envelop_info_dispose`]), and the
/// bone has no list. For a skin: the first list of that skin is disposed
/// ([`envelop_dispose`]). A bone that then has no list becomes a plain part again: word 9,
/// level −1.
pub fn envelop_info_tree_dispose(m: &mut Mem, skin: Node, node: Node, level: i32) {
    if envelop_level(m, node) != level {
        return;
    }
    let e = info(m, node);
    let mut l = m[e].lists;
    let mut cleared = false;
    if !l.is_null() {
        if skin.is_null() {
            loop {
                envelop_vert_list_dispose(m, field!(l, EnvList, verts));
                l = m[l].next;
                if l.is_null() {
                    break;
                }
            }
            let e = info(m, node);
            let first = m[e].lists;
            if first.is_null() {
                cleared = true;
            } else {
                m.free(first.addr());
                let e = info(m, node);
                m[e].lists = Ptr32::NULL;
            }
        } else {
            while !l.is_null() && m[l].owner != skin {
                l = m[l].next;
            }
            if !l.is_null() {
                let e = info(m, node);
                envelop_dispose(m, l, e);
            }
        }
    }
    let e = info(m, node);
    if cleared || m[e].lists.is_null() {
        let e = info(m, node);
        m[node].envelop = word::PART;
        m[e].level = -1;
    }
    let mut son = m[node].son;
    while !son.is_null() {
        envelop_info_tree_dispose(m, skin, son, level.wrapping_add(1));
        son = m[son].brother;
    }
}

/// The root of the skeleton that deforms a skin [soft 0x084bc124 SI_envelopSkelRoot]; null
/// for no node, a node that is no skin, or a skin without skeleton.
///
/// A local skin (word 5 or 8): the nearest father whose word is neither 5 nor 8 must have a
/// record (level 0 or more); with word 4 and kind joint the answer is its father (the
/// chain's root), with word 11 the node itself, else null. A global skin (6 or 7): the
/// first of its sons with level 0 and word 4 or 10; when it has none, the same search among
/// the sons of its father, and so on upwards while the fathers' words are 6 or 7.
pub fn envelop_skel_root(m: &Mem, node: Node) -> Node {
    if node.is_null() {
        return Node::NULL;
    }
    let mut cur = node;
    let w = m[cur].envelop;
    if w == 5 || w == 8 {
        loop {
            cur = m[cur].father;
            if cur.is_null() {
                break;
            }
            let w = m[cur].envelop;
            if w != 5 && w != 8 {
                break;
            }
        }
        if envelop_level(m, cur) < 0 {
            return Node::NULL;
        }
        let w = m[cur].envelop;
        if w == 4 && m[cur].kind == 5 {
            return m[cur].father;
        }
        if w == 11 {
            return cur;
        }
        return Node::NULL;
    }
    if (w as u32).wrapping_sub(6) > 1 {
        return Node::NULL;
    }
    loop {
        let mut s = m[cur].son;
        while !s.is_null() {
            if envelop_level(m, s) == 0 && matches!(m[s].envelop, 4 | 10) {
                return s;
            }
            s = m[s].brother;
        }
        cur = m[cur].father;
        if cur.is_null() || (m[cur].envelop as u32).wrapping_sub(6) > 1 {
            return Node::NULL;
        }
    }
}

/// Gives up a skin's working definition [soft 0x084bc244 SI_envelopDispDef]: when the node
/// has one that is not its current stored definition, it is disposed by the routine of the
/// node's kind (patch, mesh, spline, NURBS curve or surface; other kinds dispose nothing)
/// and the current stored definition becomes the working one again.
pub fn envelop_disp_def(m: &mut Mem, o: &mut dyn Outside, node: Node) {
    if node.is_null() {
        return;
    }
    let d = m[node].definition;
    if d.is_null() {
        return;
    }
    let stored = m[m[node].defs.add(m[node].cur_def as i32)];
    if d == stored {
        return;
    }
    let slot = field!(node, Model, definition).addr();
    let routine = match m[node].kind {
        1 => Some(&SI_PATCH_DISPOSE),
        4 => Some(&SI_MESH_DISPOSE),
        6 => Some(&SI_SPLINE_DISPOSE),
        9 => Some(&SI_NURBS_CRV_DISPOSE),
        10 => Some(&SI_NURBS_SRF_DISPOSE),
        _ => None,
    };
    if let Some(r) = routine {
        o.call(m, r, &[slot]);
    }
    let stored = m[m[node].defs.add(m[node].cur_def as i32)];
    m[node].definition = stored;
}

/// Offset in a patch of the flag byte whose bit 0 `SI_envelopCopyDef` sets around the copy
/// (and `envelopSkel` sets while the user drags): the surface is not computed.
pub const PATCH_FLAGS_1E4: u32 = 0x1e4;

/// Gives a skin a working definition of its own [soft 0x084bd6c8 SI_envelopCopyDef]: when
/// the node has none, or its working definition is the first stored one, a copy of the
/// first stored definition (by the routine of the node's kind; other kinds: nothing)
/// becomes the working definition. For a patch and `flag` 0, bit 0 of the source's byte at
/// +0x1e4 is set for the copy and cleared afterwards in both.
pub fn envelop_copy_def(m: &mut Mem, o: &mut dyn Outside, node: Node, flag: i16) {
    if node.is_null() {
        return;
    }
    let d = m[node].definition;
    if !d.is_null() && d != m[m[node].defs] {
        return;
    }
    // (the first stored definition is read only by the kinds that copy)
    let routine = match m[node].kind {
        1 => {
            let first = m[m[node].defs].addr();
            if flag == 0 {
                m.or_u8(first.wrapping_add(PATCH_FLAGS_1E4), 1);
            }
            let copy = o.call(m, &SI_PATCH_COPY, &[first]);
            m[node].definition = Ptr32::new(copy);
            if flag == 0 {
                m.and_u8(first.wrapping_add(PATCH_FLAGS_1E4), 0xfe);
                let copy = m[node].definition.addr();
                m.and_u8(copy.wrapping_add(PATCH_FLAGS_1E4), 0xfe);
            }
            return;
        }
        4 => &SI_MESH_COPY,
        6 => &SI_SPLINE_COPY,
        9 => &SI_NURBS_CRV_COPY,
        10 => &SI_NURBS_SRF_COPY,
        _ => return,
    };
    let first = m[m[node].defs].addr();
    m[node].definition = Ptr32::new(o.call(m, routine, &[first]));
}

/// `Model` +0x08, the 16-bit flag word, as the envelope files use it.
pub mod flag {
    /// Set by [`super::env_skel_set_flag`] in a skeleton's root (level 0).
    pub const ROOT_SEEN: u16 = 0x0002;
    /// Set by `SI_envelopSkelUpdate` in a skeleton's root: the rest matrices are made for
    /// this frame.
    pub const REST_DONE: u16 = 0x0004;
    /// A bone that moved since the flags were last reset (or one below such a bone).
    pub const MOVED: u16 = 0x4000;
}

/// A bone has not moved while each of the nine numbers of its pose is within this of the
/// pose noted at the last reset (the double at 0x08661160).
pub const STILL: f64 = 0.0001;
pub const STILL_AT: u32 = 0x0866_1160;
/// The weight of a new envelope vertex (the float at 0x08661108).
pub const FULL_WEIGHT: f32 = 100.0;
pub const FULL_WEIGHT_AT: u32 = 0x0866_1108;

/// Whether a bone is left out of a global skin's walk: its word is 11, or it is a chain's
/// root whose first son has the word 4 (both carry a local envelope of their own). The
/// test `envelopChain`, `SI_envelopNeedUpdate` and `SI_envSkelSetFlag` each make.
fn left_out(m: &Mem, skin: Node, node: Node) -> bool {
    if (m[skin].envelop as u32).wrapping_sub(6) > 1 {
        return false;
    }
    if m[node].envelop == 11 {
        return true;
    }
    if is_chain_root(m, node) {
        let son = m[node].son;
        return m[son].envelop == 4;
    }
    false
}

/// A node's pose as the flag routines compare it: global scaling (+0x3a0 under classical
/// scaling), the three angles, the local translation.
fn pose(m: &Mem, node: Node) -> [f32; 9] {
    let s = if m.i32(crate::env_globals::SI_GLB_MANIP.addr()) == 1 { m[node].cscl } else { m[node].gscl };
    let (r, t) = (m[node].vrot, m[node].trans);
    [s.x, s.y, s.z, r.x, r.y, r.z, t.x, t.y, t.z]
}

/// Whether a skin has to be deformed anew [soft 0x084bd900 SI_envelopNeedUpdate]: 1 when
/// some bone of the branch (levels as in `envelopChain`, bones left out as there) is marked
/// as moved (bit 0x4000 of its flag word) and has a list for the skin with at least one
/// vertex; else 0.
pub fn envelop_need_update(m: &Mem, skin: Node, node: Node, level: i32, brothers: i16) -> i16 {
    if envelop_level(m, node) == level && !left_out(m, skin, node) {
        if m[node].flags_08 & flag::MOVED != 0 {
            let e = info(m, node);
            let mut l = m[e].lists;
            while !l.is_null() && m[l].owner != skin {
                l = m[l].next;
            }
            if !l.is_null() && !m[l].verts.is_null() {
                return 1;
            }
        }
        let son = m[node].son;
        if !son.is_null() && envelop_need_update(m, skin, son, level.wrapping_add(1), 1) != 0 {
            return 1;
        }
    }
    let brother = m[node].brother;
    if !brother.is_null() && brothers != 0 && envelop_need_update(m, skin, brother, level, 1) != 0 {
        return 1;
    }
    0
}

/// Marks the bones of a skeleton that moved [soft 0x084bd9e4 SI_envSkelSetFlag].
///
/// For a bone of the level expected that is not left out: at level 0 bit 1 of its flag
/// word is set. With `moved` (a bone above it moved) bit 0x4000 is set. Otherwise the bone's
/// pose (global scaling, or +0x3a0 under classical scaling; angles; translation) is held
/// against the nine floats noted at `Model` +0x32c: when there are none, or one of the nine
/// differs by 0.0001 or more, the bit is set too. The sons follow one level down, marked
/// when this bone was; the brothers (with `brothers`) with `moved` as it came.
pub fn env_skel_set_flag(m: &mut Mem, skin: Node, node: Node, moved: i16, level: i32, brothers: i16) {
    if envelop_level(m, node) == level && !left_out(m, skin, node) {
        let mut here: i16 = 0;
        if level == 0 {
            m[node].flags_08 |= flag::ROOT_SEEN;
        }
        if moved != 0 {
            m[node].flags_08 |= flag::MOVED;
        } else {
            let now = pose(m, node);
            let noted: Ptr32<[f32; 9]> = m[node].unk_32c.cast();
            let mut still = !noted.is_null();
            if still {
                let limit = x(STILL);
                for k in 0..9 {
                    let d = (x(now[k]) - m[noted][k]).abs();
                    if !d.lt(limit) {
                        still = false;
                        break;
                    }
                }
            }
            if !still {
                m[node].flags_08 |= flag::MOVED;
                here = 1;
            }
        }
        let son = m[node].son;
        if !son.is_null() {
            env_skel_set_flag(m, skin, son, here | moved, level.wrapping_add(1), 1);
        }
    }
    let brother = m[node].brother;
    if !brother.is_null() && brothers != 0 {
        env_skel_set_flag(m, skin, brother, moved, level, 1);
    }
}

/// Offsets in the dictionary records `SI_envSkelResetFlag` walks (as `klusters::dict` has
/// them): the dictionary's chapter array at +4; a chapter is 0x40 bytes with its element
/// count at +4 and its elements at +8; an element is 0x84 bytes with its object at +4 and
/// its status at +0x48 (0: in use).
mod dict {
    pub const CHAPTERS: u32 = 0x04;
    pub const CHAPTER_SIZE: u32 = 0x40;
    pub const CHAPTER_COUNT: u32 = 0x04;
    pub const CHAPTER_ELEMS: u32 = 0x08;
    pub const ELEM_SIZE: u32 = 0x84;
    pub const ELEM_DATA: u32 = 0x04;
    pub const ELEM_STATUS: u32 = 0x48;
}

/// Clears the marks of every bone of a scene and notes its pose
/// [soft 0x084bdbe0 SI_envSkelResetFlag]: for each element in use of the models' chapter
/// whose node has an envelope record, bits 1, 2 and 0x4000 of the flag word are cleared and
/// the node's pose (see [`env_skel_set_flag`]) is copied, as nine words, to the block at
/// `Model` +0x32c (0x24 bytes from `SI_calloc` when the node has none; written without a
/// test for null).
pub fn env_skel_reset_flag(m: &mut Mem, dictionary: u32) {
    let chapter = (m[SI_CID][CID_MODELS] as u32).wrapping_shl(6).wrapping_add(m.u32(dictionary.wrapping_add(dict::CHAPTERS)));
    let mut i: i32 = 0;
    while i < m.i32(chapter.wrapping_add(dict::CHAPTER_COUNT)) {
        let elem = m.u32(chapter.wrapping_add(dict::CHAPTER_ELEMS)).wrapping_add((i as u32).wrapping_mul(dict::ELEM_SIZE));
        let _ = dict::CHAPTER_SIZE;
        if m.i16(elem.wrapping_add(dict::ELEM_STATUS)) == 0 {
            let node: Node = Ptr32::new(m.u32(elem.wrapping_add(dict::ELEM_DATA)));
            if !node.is_null() && envelop_level(m, node) >= 0 {
                m[node].flags_08 &= !(flag::ROOT_SEEN | flag::REST_DONE | flag::MOVED);
                if m[node].unk_32c.is_null() {
                    m[node].unk_32c = Ptr32::new(m.si_calloc(1, 0x24));
                }
                let now = pose(m, node);
                let noted: Ptr32<[f32; 9]> = m[node].unk_32c.cast();
                m[noted] = now;
            }
        }
        i += 1;
    }
}

/// Moves the skeletons under a node by a vector [soft 0x084bd89c SI_envelopCompensate]:
/// the vector is added to the rest translation of every son whose word is 4 or 10 (a float
/// sum per coordinate; the rest transformation is read without a test for null).
pub fn envelop_compensate(m: &mut Mem, node: Node, by: Ptr32<[f32; 3]>) {
    let mut s = m[node].son;
    while !s.is_null() {
        if matches!(m[s].envelop, 4 | 10) {
            for k in 0..3 {
                let rest: Ptr32<crate::env_records::InitSrt> = m[s].init_srt.cast();
                let v = (x(m[rest].trans[k]) + m[by][k]).to_f32();
                let rest: Ptr32<crate::env_records::InitSrt> = m[s].init_srt.cast();
                m[rest].trans[k] = v;
            }
        }
        s = m[s].brother;
    }
}

// ------------------------------------------------------------ second pass: rest poses

/// The inverse of a matrix whose upper 3 × 3 is orthonormal
/// [soft 0x084ad170 SI_matrixOrthoNormalInverse] (`iktools.c`; `si-kine` ports it too, and
/// the lead may reconcile the two): the 3 × 3 and the translation are copied through the
/// unit first (a signalling NaN arrives quiet), the result is their transpose with row 3
/// = −((t.x m[i][0] + t.y m[i][1]) + t.z m[i][2]) for column i, the last column 0, 0, 0, 1.
pub fn matrix_ortho_normal_inverse(src: &si_math::types::Matrix) -> si_math::types::Matrix {
    let mut l = [[0.0f32; 3]; 4];
    for r in 0..4 {
        for c in 0..3 {
            l[r][c] = fmov(src[r][c]);
        }
    }
    let mut out = [[0.0f32; 4]; 4];
    out[3][3] = 1.0;
    let t = l[3];
    for i in 0..3 {
        out[0][i] = l[i][0];
        out[1][i] = l[i][1];
        out[2][i] = l[i][2];
        out[3][i] = (-((x(t[0]) * l[i][0] + x(t[1]) * l[i][1]) + x(t[2]) * l[i][2])).to_f32();
    }
    out
}

/// Three words from one address to another, as the original's `mov`s copy them.
fn copy3(m: &mut Mem, from: u32, to: u32) {
    for k in 0..3u32 {
        let v = m.u32(from.wrapping_add(4 * k));
        m.set_u32(to.wrapping_add(4 * k), v);
    }
}

/// Offsets of the local transformation in `Model` and of the parked one in the record.
const SCAL: u32 = 0x100;
const VROT: u32 = 0x10c;
const TRANS: u32 = 0x158;
const PARKED_SCAL: u32 = 0x48;
const PARKED_ROT: u32 = 0x54;
const PARKED_TRANS: u32 = 0x60;

/// Puts a skeleton into its rest pose, parks its present pose, or brings that back
/// [soft 0x084bc7e4 SI_envelopSkelPosReset]: `what` 0 rest, 1 park and rest, 2 restore.
///
/// A bone is taken when it has a record and the word 9, 4, 10 or 11, except that a root of
/// word 11 and a chain root whose first joint has word 4 are taken only at the top of the
/// walk (`brothers` 0); with `what` 0 every volume (algorithm 18 to 29) is taken too. What
/// is done to a taken node:
///
/// - a joint (kind 5): its rotation angles. Rest: those of its rest transformation (+0x328).
///   Park: the present angles go to the record (+0x54) and the node takes the rest angles,
///   or zeros when `auto` is set (the chain of an automatic local skin). Restore: from the
///   record.
/// - an effector (a null of algorithm 3 or 4): the same with the rest angles; then, if the
///   chain's inverse kinematics record (`Model` +0x318 of its father) has its first word set,
///   that word is cleared, the angles of the present rotation against the global one
///   (`SI_matrixOrthoNormalInverse`, `SI_matrixMultiply`, `SI_matrixExtractXyzRotation`
///   from zeros) are handed to `SI_modelSetRotation`, the global rotation is copied to the
///   rotation matrix and the node's angles extracted from it, and the word is set to 1.
/// - any other node: scaling, angles and translation. Rest: from the rest transformation
///   when there is one. Park: the three go to the record (+0x48, +0x54, +0x60), the node
///   takes the rest ones (read without a test). Restore: from the record.
///
/// Then the first son, one level down, and the brothers when `brothers` is set. A node not
/// taken is passed over with its sons. Everything is copied word for word.
pub fn envelop_skel_pos_reset(m: &mut Mem, o: &mut dyn Outside, node: Node, what: i32, auto: i16, brothers: i16) {
    let mut taken = false;
    if !node.is_null() && !m[node].env_skel_jnt.is_null() && matches!(m[node].envelop, 9 | 4 | 10 | 11) {
        let w = m[node].envelop;
        let top_only = (is_chain_root(m, node) && {
            let s = m[node].son;
            m[s].envelop == 4
        }) || w == 11;
        taken = !top_only || brothers == 0;
    }
    if !taken && what == 0 {
        let algo = m[node].algo_type;
        taken = (18..=29).contains(&algo);
    }
    if taken {
        let n = node.addr();
        let kind = m[node].kind;
        let effector = kind == 0 && (m[node].algo_type as u32).wrapping_sub(3) <= 1;
        let srt = |m: &Mem| m[node].init_srt.addr();
        let rec = |m: &Mem| m[node].env_skel_jnt.addr();
        if kind == 5 {
            match what {
                0 => copy3(m, srt(m) + 0xc, n + VROT),
                1 => {
                    let zeros = m.local(0xc);
                    if auto != 0 {
                        for k in 0..3 {
                            m.set_u32(zeros + 4 * k, 0);
                        }
                    }
                    copy3(m, n + VROT, rec(m) + PARKED_ROT);
                    let from = if auto != 0 { zeros } else { srt(m) + 0xc };
                    copy3(m, from, n + VROT);
                    m.release(0xc);
                }
                2 => copy3(m, rec(m) + PARKED_ROT, n + VROT),
                _ => {}
            }
        } else if effector {
            match what {
                0 => copy3(m, srt(m) + 0xc, n + VROT),
                1 => {
                    copy3(m, n + VROT, rec(m) + PARKED_ROT);
                    copy3(m, srt(m) + 0xc, n + VROT);
                }
                2 => copy3(m, rec(m) + PARKED_ROT, n + VROT),
                _ => {}
            }
            let father = m[node].father;
            let ik = m[father].inv_kin.addr();
            if m.u32(ik) != 0 {
                m.set_u32(ik, 0);
                let inv = matrix_ortho_normal_inverse(&m[node].grot);
                let product = si_math::algebra::matrix_multiply(&m[node].mrot, &inv);
                let zero = si_math::types::Vector::new(0.0, 0.0, 0.0);
                let keep = m.i16(crate::env_globals::SI_GLB_MANIP.addr().wrapping_add(0x20)) != 0;
                let angles = si_math::algebra::matrix_extract_xyz_rotation(&product, &zero, keep);
                let at = m.local(0xc);
                m.set_f32(at, angles.x);
                m.set_f32(at + 4, angles.y);
                m.set_f32(at + 8, angles.z);
                o.call(m, &SI_MODEL_SET_ROTATION, &[n, at, 0, 0]);
                m.release(0xc);
                let g = m[node].grot;
                m[node].mrot = matrix_copy(&g);
                let v = m[node].vrot;
                let previous = si_math::types::Vector::new(v.x, v.y, v.z);
                let keep = m.i16(crate::env_globals::SI_GLB_MANIP.addr().wrapping_add(0x20)) != 0;
                let mr = m[node].mrot;
                let angles = si_math::algebra::matrix_extract_xyz_rotation(&mr, &previous, keep);
                m[node].vrot.x = angles.x;
                m[node].vrot.y = angles.y;
                m[node].vrot.z = angles.z;
                let father = m[node].father;
                let ik = m[father].inv_kin.addr();
                m.set_u32(ik, 1);
            }
        } else {
            match what {
                0 => {
                    if srt(m) != 0 {
                        copy3(m, srt(m), n + SCAL);
                        copy3(m, srt(m) + 0xc, n + VROT);
                        copy3(m, srt(m) + 0x18, n + TRANS);
                    }
                }
                1 => {
                    copy3(m, n + SCAL, rec(m) + PARKED_SCAL);
                    copy3(m, n + VROT, rec(m) + PARKED_ROT);
                    copy3(m, n + TRANS, rec(m) + PARKED_TRANS);
                    copy3(m, srt(m), n + SCAL);
                    copy3(m, srt(m) + 0xc, n + VROT);
                    copy3(m, srt(m) + 0x18, n + TRANS);
                }
                2 => {
                    copy3(m, rec(m) + PARKED_SCAL, n + SCAL);
                    copy3(m, rec(m) + PARKED_ROT, n + VROT);
                    copy3(m, rec(m) + PARKED_TRANS, n + TRANS);
                }
                _ => {}
            }
        }
        let son = m[node].son;
        if !son.is_null() {
            envelop_skel_pos_reset(m, o, son, what, auto, 1);
        }
    }
    let b = m[node].brother;
    if !b.is_null() && brothers != 0 {
        envelop_skel_pos_reset(m, o, b, what, auto, 1);
    }
}

/// The skeleton of a skin into its rest pose for the time of an assignment, and back
/// [soft 0x084bccc4 SI_envelopTmpReset] (`what` as for [`envelop_skel_pos_reset`]).
///
/// A local skin (word 5 or 8): the skeleton from its root (`auto` for word 8), then a chain
/// root's chain is solved (`SI_modelSpecialUpdateSonState`) and the root's branch updated
/// (`SI_modelUpdateSonState`). A global skin (6 or 7): the root and each brother that is no
/// root of word 11 and no chain root whose first joint has word 4, each without brothers;
/// then the branch of the root's father is updated from the grandfather (both read without
/// tests). Nothing without a skeleton root.
pub fn envelop_tmp_reset(m: &mut Mem, o: &mut dyn Outside, skin: Node, what: i32) {
    let root = envelop_skel_root(m, skin);
    if root.is_null() {
        return;
    }
    let w = m[skin].envelop;
    if w == 5 || w == 8 {
        envelop_skel_pos_reset(m, o, root, what, (w == 8) as i16, 0);
        if is_chain_root(m, root) {
            let son = m[root].son;
            o.call(m, &SI_MODEL_SPECIAL_UPDATE_SON_STATE, &[0, son.addr()]);
        }
        let son = m[root].son;
        o.call(m, &SI_MODEL_UPDATE_SON_STATE, &[root.addr(), son.addr(), 1]);
    } else if (w as u32).wrapping_sub(6) <= 1 {
        let mut b = root;
        while !b.is_null() {
            let mut passed = false;
            if (m[skin].envelop as u32).wrapping_sub(6) <= 1 {
                if m[b].envelop == 11 {
                    passed = true;
                } else if is_chain_root(m, b) {
                    let s = m[b].son;
                    passed = m[s].envelop == 4;
                }
            }
            if !passed {
                envelop_skel_pos_reset(m, o, b, what, 0, 0);
            }
            b = m[b].brother;
        }
        let father = m[root].father;
        let grand = m[father].father;
        o.call(m, &SI_MODEL_UPDATE_SON_STATE, &[grand.addr(), father.addr(), 1]);
    }
}

// ------------------------------------------------------------ second pass: after a load

/// Drops the vertex entries whose number is no point of their skin
/// [soft 0x084bd00c envelopCheckVrtInd]: for each list of the bone's record with a skin whose
/// element exists (`SI_edGetElemFromModel`), the skin's number of points in the models'
/// chapter (`SI_elemGetNbVertices`); an entry numbered below 0 or not below it is unlinked and
/// freed. Nothing for a bone without record or lists.
pub fn envelop_check_vrt_ind(m: &mut Mem, o: &mut dyn Outside, node: Node) {
    let e = info(m, node);
    if e.is_null() {
        return;
    }
    let mut l = m[e].lists;
    if l.is_null() {
        return;
    }
    let index = m.local(4);
    loop {
        let owner = m[l].owner;
        if !owner.is_null() {
            let elem = o.call(m, &SI_ED_GET_ELEM_FROM_MODEL, &[owner.addr(), index]);
            if elem != 0 {
                let chapter = m[SI_CID][CID_MODELS] as u32;
                let n = o.call(m, &SI_ELEM_GET_NB_VERTICES, &[elem, chapter, 0]) as i32;
                let mut prev: Ptr32<EnvVert> = Ptr32::NULL;
                let mut v = m[l].verts;
                while !v.is_null() {
                    let next = m[v].next;
                    let i = m[v].index;
                    if i >= 0 && i < n {
                        prev = v;
                    } else {
                        if prev.is_null() {
                            m[l].verts = next;
                        } else {
                            m[prev].next = next;
                        }
                        m.free(v.addr());
                    }
                    v = next;
                }
            }
        }
        l = m[l].next;
        if l.is_null() {
            break;
        }
    }
    m.release(4);
}

/// Gives skins their lists anew [soft 0x084bd0c0 envelopReassignPtr]: every node of the
/// branch (and its brothers) with the envelope word `word` that is not the main node of a
/// global envelope takes the first list without skin found from `lists` backwards (through
/// the `prev` links), if any; the sons of such a node are visited, the others' not.
pub fn envelop_reassign_ptr(m: &mut Mem, node: Node, word: i32, lists: Ptr32<EnvList>) {
    if m[node].envelop == word && main_glob_env(m, node) as i16 == 0 {
        let mut l = lists;
        while !l.is_null() && !m[l].owner.is_null() {
            l = m[l].prev;
        }
        if !l.is_null() {
            m[l].owner = node;
        }
        let son = m[node].son;
        if !son.is_null() {
            envelop_reassign_ptr(m, son, word, lists);
        }
    }
    let b = m[node].brother;
    if !b.is_null() {
        envelop_reassign_ptr(m, b, word, lists);
    }
}

/// Rebuilds a branch's lists after a load [soft 0x084bd140 envelopRebuildList].
///
/// A bone whose record has the level expected and lists: its last list takes the skin (but
/// for a rigid node, word 7), the skin's sons take the lists before it
/// ([`envelop_reassign_ptr`] with the skin's word, from the last), and entries numbered
/// beyond the skin's points are dropped ([`envelop_check_vrt_ind`]). Then the sons, one level
/// down; and when the bone is a chain root or a root of word 11 whose first son has a record,
/// the bone's lists are made again from the son's: the first list record is freed (the
/// others are left where they are), and one list per list of the son, for the same skin, is
/// chained in order. Brothers are visited at the same level when the level is above 0. A bone
/// of another level is passed over with its sons.
pub fn envelop_rebuild_list(m: &mut Mem, o: &mut dyn Outside, skin: Node, node: Node, level: i32) {
    'mine: {
        if envelop_level(m, node) != level {
            break 'mine;
        }
        let lists = m[info(m, node)].lists;
        if !lists.is_null() {
            let w = m[skin].envelop;
            let mut last = lists;
            while !m[last].next.is_null() {
                last = m[last].next;
            }
            if w != 7 {
                m[last].owner = skin;
            }
            let son = m[skin].son;
            if !son.is_null() {
                let w = m[skin].envelop;
                envelop_reassign_ptr(m, son, w, last);
            }
            envelop_check_vrt_ind(m, o, node);
        }
        let son = m[node].son;
        if son.is_null() {
            break 'mine;
        }
        envelop_rebuild_list(m, o, skin, son, level.wrapping_add(1));
        if !(is_chain_root(m, node) || m[node].envelop == 11) {
            break 'mine;
        }
        let son = m[node].son;
        let theirs = info(m, son);
        if theirs.is_null() {
            break 'mine;
        }
        let e = info(m, node);
        let first = m[e].lists;
        if !first.is_null() {
            m.free(first.addr());
            let e = info(m, node);
            m[e].lists = Ptr32::NULL;
        }
        let mut t = m[theirs].lists;
        let mut prev: Ptr32<EnvList> = Ptr32::NULL;
        while !t.is_null() {
            let owner = m[t].owner;
            let nl = envelop_list_allocate(m, owner);
            let e = info(m, node);
            if m[e].lists.is_null() {
                m[e].lists = nl;
            } else {
                m[prev].next = nl;
                m[nl].prev = prev;
            }
            t = m[t].next;
            prev = nl;
        }
    }
    let b = m[node].brother;
    if !b.is_null() && level > 0 {
        envelop_rebuild_list(m, o, skin, b, level);
    }
}

// ------------------------------------------------- third pass: the envelopes of a loaded file

/// Copies a node's local scaling, angles and translation into a new rest transformation
/// (`SI_calloc(1, 0x24)` at `Model` +0x328), as `envelopDataConvert` and `SI_envelopRebuild`
/// both write it out: the block's address is stored first, then the nine words, the
/// pointer read again before the angles and before the translation.
fn rest_from_local(m: &mut Mem, node: Node) {
    let p = m.si_calloc(1, 0x24);
    let n = node.addr();
    let v = m.u32(n.wrapping_add(0x100));
    m[node].init_srt = Ptr32::new(p);
    m.set_u32(p, v);
    for k in 1..3u32 {
        let v = m.u32(n.wrapping_add(0x100 + 4 * k));
        m.set_u32(p.wrapping_add(4 * k), v);
    }
    for (from, to) in [(0x10cu32, 0xcu32), (0x158, 0x18)] {
        let p = m[node].init_srt.addr();
        for k in 0..3u32 {
            let v = m.u32(n.wrapping_add(from + 4 * k));
            m.set_u32(p.wrapping_add(to + 4 * k), v);
        }
    }
}

/// Chains one new empty list (`SI_envelopListAllocate(null)`) into the node's record for
/// each list of `from`, in order; `prev` is the list chained last (null at the start: a
/// record that has lists already is then written through null, as in the original).
fn lists_like(m: &mut Mem, node: Node, from: Ptr32<EnvList>) {
    let mut prev: Ptr32<EnvList> = Ptr32::NULL;
    let mut t = from;
    while !t.is_null() {
        let nl = envelop_list_allocate(m, Node::NULL);
        let e = info(m, node);
        if m[e].lists.is_null() {
            m[e].lists = nl;
        } else {
            m[prev].next = nl;
            m[nl].prev = prev;
        }
        t = m[t].next;
        prev = nl;
    }
}

/// Converts the envelope records of the earlier format of a branch
/// [soft 0x084bcddc envelopDataConvert]: the levels are numbered from `level` down the
/// first sons, and the chain's ends get lists.
///
/// - **A chain's root** (a null of algorithm 1 or 2) whose first son has a record (the son
///   is read without a test for null), when that record's level is not 0 or `level` is not
///   above 0: word 9 if it had none, a new record ([`envelop_info_allocate`], written
///   without a test for null) of level `level`, one empty list per list of the first son's
///   record, and, when it has none, a rest transformation from its local one. The first son
///   is converted at `level` + 1.
/// - **A joint** (kind 5) with a record: word 9 unless it is 4, the record's level `level`;
///   its first son at `level` + 1.
/// - **An effector** (a null of algorithm 3 or 4) with a record: word 9, level `level`, one
///   empty list per list of its father's record (the father read without a test); its first
///   son at `level` + 1.
/// - Anything else: the first son is not converted.
///
/// The next brother is converted at `level` when `level` is above 0.
pub fn envelop_data_convert(m: &mut Mem, node: Node, level: i32) {
    let mut down = 0;
    let chain = is_chain_root(m, node) && {
        let theirs = info(m, m[node].son);
        !theirs.is_null() && (m[theirs].level != 0 || level <= 0)
    };
    if chain {
        if m[node].envelop == 0 {
            m[node].envelop = word::PART;
        }
        let e = envelop_info_allocate(m);
        m[node].env_skel_jnt = e.cast();
        m[e].level = level;
        let theirs = info(m, m[node].son);
        let from = m[theirs].lists;
        down = level.wrapping_add(1);
        lists_like(m, node, from);
        if m[node].init_srt.is_null() {
            rest_from_local(m, node);
        }
    } else {
        let kind = m[node].kind;
        if kind == 5 && !info(m, node).is_null() {
            if m[node].envelop != word::CHAIN_FIRST {
                m[node].envelop = word::PART;
            }
            let e = info(m, node);
            m[e].level = level;
            down = level.wrapping_add(1);
        } else if kind == 0 && (m[node].algo_type as u32).wrapping_sub(3) <= 1 && !info(m, node).is_null() {
            let e = info(m, node);
            m[node].envelop = word::PART;
            m[e].level = level;
            let theirs = info(m, m[node].father);
            let from = m[theirs].lists;
            down = level.wrapping_add(1);
            lists_like(m, node, from);
        }
    }
    let son = m[node].son;
    if !son.is_null() && down > 0 {
        envelop_data_convert(m, son, down);
    }
    let b = m[node].brother;
    if !b.is_null() && level > 0 {
        envelop_data_convert(m, b, level);
    }
}

/// Rebuilds the envelopes of a branch read from a file or copied
/// [soft 0x084bd2a0 SI_envelopRebuild]: `SI_fdmodelBinaryRead` calls it on the root it
/// read with `all` 1 and `old` set for a file below version 2.6.
///
/// For each node, before its sons and brothers:
///
/// 1. A chain's root without record whose first son's record has level 0 (the earlier
///    format: the son is read without a test for null) is converted
///    ([`envelop_data_convert`] at level 0).
/// 2. A node of level 0 (a skeleton's root) finds its skin: for a chain's root whose first
///    son has the word 4 the chain's local skin ([`env_local_get`], made word 5 if it has
///    none); for a node of word 4 its father (made word 6 if it has none), of word 10 its
///    father, of word 11 its local skin. **Without a skin, or for any other word, the
///    routine returns at once**, sons and brothers not visited. The lists get their skins
///    ([`envelop_rebuild_list`] from level 0), and the skeleton of a local envelope (a chain
///    root whose first son has word 4, or a root of word 11) deforms its skins
///    ([`envelop_hier_update`]; before it, with `all` and `old`, [`crate::envelop::oldenvelop_reset`] on the node).
/// 3. A node of another level (or without record) whose algorithm is 18 to 29 (a volume)
///    and whose father has a record gets a rest transformation from its local one when it
///    has none.
///
/// Then the first son and the next brother are rebuilt (with the same `all` and `old`), and
/// last, for the main node of a global envelope ([`main_glob_env`]) with a skeleton
/// ([`envelop_skel_root`]): without `all`, its matrices are rebuilt from no father
/// (`SI_modelUpdateSonState(null, node, 1)`) and the skeleton deforms its skins; with
/// `all`, [`crate::envelop::oldenvelop_reset`] on the skeleton's root if `old`, the skins deformed, and a
/// rigidly carried node (word 7) without rest transformation gets one from its global state
/// (`SI_calloc(1, 0x24)`, `SI_modelGlobalState (0x084637dc)` into its three vectors).
pub fn envelop_rebuild(m: &mut Mem, o: &mut dyn Outside, node: Node, all: i16, old: i16) {
    if node.is_null() {
        return;
    }
    if is_chain_root(m, node) && m[node].env_skel_jnt.is_null() {
        let theirs = info(m, m[node].son);
        if !theirs.is_null() && m[theirs].level == 0 {
            envelop_data_convert(m, node, 0);
        }
    }
    let chain_local = |m: &Mem| is_chain_root(m, node) && m[m[node].son].envelop == word::CHAIN_FIRST;
    if envelop_level(m, node) == 0 {
        let skin;
        if chain_local(m) {
            skin = env_local_get(m, node);
            if skin.is_null() {
                return;
            }
            if m[skin].envelop == 0 {
                m[skin].envelop = word::LOCAL;
            }
        } else {
            match m[node].envelop {
                word::CHAIN_FIRST => {
                    skin = m[node].father;
                    if skin.is_null() {
                        return;
                    }
                    if m[skin].envelop == 0 {
                        m[skin].envelop = word::GLOBAL;
                    }
                }
                word::ROOT => {
                    skin = m[node].father;
                    if skin.is_null() {
                        return;
                    }
                }
                word::LOCAL_ROOT => {
                    skin = env_local_get(m, node);
                    if skin.is_null() {
                        return;
                    }
                }
                _ => return,
            }
        }
        envelop_rebuild_list(m, o, skin, node, 0);
        if chain_local(m) || m[node].envelop == word::LOCAL_ROOT {
            if all != 0 && old != 0 {
                crate::envelop::oldenvelop_reset(m, o, node);
            }
            crate::envelop::envelop_hier_update(m, o, node);
        }
    } else if (m[node].algo_type as u32).wrapping_sub(0x12) <= 0xb {
        let father = m[node].father;
        if !father.is_null() && !m[father].env_skel_jnt.is_null() && m[node].init_srt.is_null() {
            rest_from_local(m, node);
        }
    }
    let son = m[node].son;
    if !son.is_null() {
        envelop_rebuild(m, o, son, all, old);
    }
    let b = m[node].brother;
    if !b.is_null() {
        envelop_rebuild(m, o, b, all, old);
    }
    if main_glob_env(m, node) as i16 == 0 {
        return;
    }
    let root = envelop_skel_root(m, node);
    if root.is_null() {
        return;
    }
    if all == 0 {
        o.call(m, &SI_MODEL_UPDATE_SON_STATE, &[0, node.addr(), 1]);
        crate::envelop::envelop_hier_update(m, o, root);
    } else {
        if old != 0 {
            crate::envelop::oldenvelop_reset(m, o, root);
        }
        crate::envelop::envelop_hier_update(m, o, root);
        if m[node].envelop == word::GLOBAL_RIGID && m[node].init_srt.is_null() {
            let p = m.si_calloc(1, 0x24);
            m[node].init_srt = Ptr32::new(p);
            o.call(m, &SI_MODEL_GLOBAL_STATE, &[node.addr(), p, p.wrapping_add(0xc), p.wrapping_add(0x18)]);
        }
    }
}

/// Switches on the envelopes of the skins a load brought in
/// [soft 0x084bd5a8 SI_envelopRelationSet]: the relations of type 0xcf (207, `FlexEnv`) are
/// not read from a scene's description, and this routine makes them. `dictionary` is the
/// dictionary loaded into.
///
/// For each element of its models' chapter (the chapter's address taken once; the count
/// and the elements' array read again at each step) that is in use (+0x48 0), new to this
/// load (+0x46 1) and has a node whose word is 5, 6 or 8 (a skin): unless the element
/// already has a relation 0xcf in list 2 of its forward side, in the models' chapter, to
/// itself (`SI_elemdictFindRelatedElem` on `dictionary`, both stores through one local),
/// the node's element is found again in the **current** dictionary
/// (`SI_elemdictFindElemByData`) and related to itself there by a relation 0xcf, rule 1,
/// no position (`SI_elemdictAddRelation` on `SI_RD`): the switch `SI_envelopActive` reads.
pub fn envelop_relation_set(m: &mut Mem, o: &mut dyn Outside, dictionary: u32) {
    const NEW_FLAG: u32 = 0x46;
    let chapter = (m[SI_CID][CID_MODELS] as u32).wrapping_shl(6).wrapping_add(m.u32(dictionary.wrapping_add(dict::CHAPTERS)));
    let found = m.local(4);
    let mut i: i32 = 0;
    while i < m.i32(chapter.wrapping_add(dict::CHAPTER_COUNT)) {
        let elem = (i as u32).wrapping_mul(dict::ELEM_SIZE).wrapping_add(m.u32(chapter.wrapping_add(dict::CHAPTER_ELEMS)));
        if elem != 0 && m.i16(elem.wrapping_add(dict::ELEM_STATUS)) == 0 && m.i16(elem.wrapping_add(NEW_FLAG)) == 1 {
            let node: Node = Ptr32::new(m.u32(elem.wrapping_add(dict::ELEM_DATA)));
            let w = if node.is_null() { 0 } else { m[node].envelop };
            if !node.is_null() && ((w as u32).wrapping_sub(5) <= 1 || w == 8) {
                let models = m[SI_CID][CID_MODELS] as u32;
                let related = o.call(m, &SI_ELEMDICT_FIND_RELATED_ELEM, &[dictionary, elem, 1, 2, 0xcf, models, found, found]);
                if related == 0 || related != elem {
                    let models = m[SI_CID][CID_MODELS] as u32;
                    let dict = cur_dict(m);
                    let index = o.call(m, &SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, models, node.addr()]);
                    let dict = cur_dict(m);
                    o.call(m, &SI_ELEMDICT_ADD_RELATION, &[dict, SI_RD.addr(), index, index, 0xcf, 1, 0]);
                }
            }
        }
        i += 1;
    }
    m.release(4);
}

// ------------------------------------------------ fourth pass, deform2: renumbering

/// Renumbers a skin's points in a skeleton's lists [soft 0x084bd78c SI_envelopRenum]: for each
/// node of the branch (`node`, its first son's branch, and its brothers' when `brothers` is
/// set) that has a level, the list owned by `skin` is walked; an entry whose number is
/// negative or whose place in `map` (an array of ints, one per old number) is negative is
/// unlinked and freed, any other gets the number `map` gives. The sons are always visited
/// (with brothers), the brothers of a node only while `brothers` is set.
pub fn envelop_renum(m: &mut Mem, skin: Node, node: Node, map: Ptr32<i32>, brothers: i16) {
    if skin.is_null() || node.is_null() {
        return;
    }
    if envelop_level(m, node) >= 0 {
        let mut l = m[info(m, node)].lists;
        while !l.is_null() && m[l].owner != skin {
            l = m[l].next;
        }
        if !l.is_null() {
            let mut prev: Ptr32<EnvVert> = Ptr32::NULL;
            let mut v = m[l].verts;
            while !v.is_null() {
                let next = m[v].next;
                let idx = m[v].index;
                let new = if idx >= 0 { m.i32(map.addr().wrapping_add((idx as u32).wrapping_mul(4))) } else { -1 };
                if idx >= 0 && new >= 0 {
                    m[v].index = new;
                    prev = v;
                } else {
                    if !prev.is_null() {
                        m[prev].next = next;
                    } else {
                        m[l].verts = next;
                    }
                    m.free(v.addr());
                }
                v = next;
            }
        }
        let son = m[node].son;
        if !son.is_null() {
            envelop_renum(m, skin, son, map, 1);
        }
    }
    let b = m[node].brother;
    if !b.is_null() && brothers != 0 {
        envelop_renum(m, skin, b, map, 1);
    }
}

// ------------------------------------------------ fourth pass, deform2: relaxing a skin

/// Switches a skin's envelope off [soft 0x084bc2d0 SI_envelopRelax]: nothing for a null `node`.
/// A node whose word is not 0 and that has a skeleton root (`root`, else
/// [`envelop_skel_root`]; none: stop) loses its envelope entries: a local skin (word 5 or 8)
/// from `root`'s branch ([`envelop_info_tree_dispose`] with level 0), a global one (6 or 7)
/// from `root` and each of its brothers but roots of word 11 and chain roots whose first
/// joint has word 4. With `sons` set, every son with the same word that is not a main node
/// of a global envelope is relaxed too (root and `quiet` as given, sons set). The word is
/// then 0; unless `quiet`, the node's relation of type 0xcf with itself is removed
/// (`SI_elemdictRemoveRelationId (0x08133998)`), its dependencies settled
/// (`SI_modelUpdateDependency`) and a patch's surface (`SI_patchComputeSurface`) or a mesh's
/// display (`SI_meshOptimizeDisplay (0x0845a7c8)`) rebuilt. Last, a joint (kind 5, word 1)
/// father whose second son is `node` and which `node` ends gets word 0.
pub fn envelop_relax(m: &mut Mem, o: &mut dyn Outside, node: Node, root: Node, quiet: i16, sons: i16) {
    if node.is_null() {
        return;
    }
    let mut root = root;
    'on: {
        if m[node].envelop == 0 {
            break 'on;
        }
        if root.is_null() {
            root = envelop_skel_root(m, node);
            if root.is_null() {
                break 'on;
            }
        }
        let w = m[node].envelop;
        if w == 5 || w == 8 {
            envelop_info_tree_dispose(m, node, root, 0);
        } else if (w as u32).wrapping_sub(6) < 2 {
            let mut p = root;
            while !p.is_null() {
                let skip = m[p].envelop == word::LOCAL_ROOT || (m[p].kind == 0 && (m[p].algo_type as u32).wrapping_sub(1) <= 1 && m[m[p].son].envelop == word::CHAIN_FIRST);
                if !skip {
                    envelop_info_tree_dispose(m, node, p, 0);
                }
                p = m[p].brother;
            }
        }
        if sons != 0 {
            let mut c = m[node].son;
            while !c.is_null() {
                if m[c].envelop == m[node].envelop && main_glob_env(m, c) as i16 == 0 {
                    envelop_relax(m, o, c, root, quiet, 1);
                }
                c = m[c].brother;
            }
        }
        m[node].envelop = 0;
        if quiet == 0 {
            let index = m.local(4);
            o.call(m, &SI_ED_GET_ELEM_FROM_MODEL, &[node.addr(), index]);
            let dict = cur_dict(m);
            let i = m.u32(index);
            o.call(m, &SI_ELEMDICT_REMOVE_RELATION_ID, &[dict, SI_RD.addr(), i, 0xcf]);
            o.call(m, &crate::outside::SI_MODEL_UPDATE_DEPENDENCY, &[node.addr(), 0]);
            match m[node].kind {
                1 => {
                    let patch = m.u32(m[node].defs.addr());
                    o.call(m, &crate::outside::SI_PATCH_COMPUTE_SURFACE, &[patch, 0]);
                }
                4 => {
                    let mesh = m[node].definition.addr();
                    o.call(m, &SI_MESH_OPTIMIZE_DISPLAY, &[mesh]);
                }
                _ => {}
            }
            m.release(4);
        }
    }
    let father = m[node].father;
    if !father.is_null() && m[father].kind == 5 && m[father].envelop == 1 {
        let second = m[m[father].son].brother;
        if node == second && m[node].brother.is_null() {
            m[father].envelop = 0;
        }
    }
}

/// What a cut does to a node's envelope [soft 0x084bc4b8 SI_envelopCut]: nothing for a node
/// with no father. A node of level below 0 (not on any bone) that is not the main node of a
/// global envelope is relaxed ([`envelop_relax`] with no root, not quiet, with its sons).
/// A node of level 0, unless its word is 11 or it is a chain root (a null of algorithm 1 or
/// 2) whose first son has word 4: when its word is 4 or 10 and no other son of its father
/// has word 4 or 10, a father of word 7 gets word 0, and every skin of the bone's lists
/// loses its word, its relation of type 0xcf is removed
/// (`SI_elemdictRemoveRelationId (0x08133998)`), its dependencies are settled
/// (`SI_modelUpdateDependency`) and a patch's surface (`SI_patchComputeSurface`) or a mesh's
/// display (`SI_meshOptimizeDisplay (0x0845a7c8)`) rebuilt. Then, for any node of level 0 or
/// above that was not left early, its branch is taken off the skins
/// ([`envelop_info_tree_dispose`] with no skin).
pub fn envelop_cut(m: &mut Mem, o: &mut dyn Outside, node: Node) {
    if m[node].father.is_null() {
        return;
    }
    let level = envelop_level(m, node);
    if level < 0 {
        if main_glob_env(m, node) as i16 == 0 {
            envelop_relax(m, o, node, Ptr32::NULL, 0, 1);
        }
        return;
    }
    if level == 0 {
        let w = m[node].envelop;
        if w == word::LOCAL_ROOT {
            return;
        }
        if is_chain_root(m, node) && m[m[node].son].envelop == word::CHAIN_FIRST {
            return;
        }
        if (w == 4 || w == 10) && !other_skeleton_son(m, node) {
            let father = m[node].father;
            if m[father].envelop == word::GLOBAL_RIGID {
                m[father].envelop = 0;
            }
            let mut l = m[info(m, node)].lists;
            while !l.is_null() {
                let skin = m[l].owner;
                if !skin.is_null() {
                    m[skin].envelop = 0;
                    let index = m.local(4);
                    o.call(m, &SI_ED_GET_ELEM_FROM_MODEL, &[skin.addr(), index]);
                    let dict = cur_dict(m);
                    let i = m.u32(index);
                    o.call(m, &SI_ELEMDICT_REMOVE_RELATION_ID, &[dict, SI_RD.addr(), i, 0xcf]);
                    o.call(m, &crate::outside::SI_MODEL_UPDATE_DEPENDENCY, &[skin.addr(), 0]);
                    match m[skin].kind {
                        1 => {
                            let patch = m.u32(m[skin].defs.addr());
                            o.call(m, &crate::outside::SI_PATCH_COMPUTE_SURFACE, &[patch, 0]);
                        }
                        4 => {
                            let mesh = m[skin].definition.addr();
                            o.call(m, &SI_MESH_OPTIMIZE_DISPLAY, &[mesh]);
                        }
                        _ => {}
                    }
                    m.release(4);
                }
                l = m[l].next;
            }
        }
    }
    envelop_info_tree_dispose(m, Ptr32::NULL, node, level);
}

/// Whether a brother of `node` (a son of its father) other than itself has the word 4 or 10.
fn other_skeleton_son(m: &Mem, node: Node) -> bool {
    let mut s = m[m[node].father].son;
    while !s.is_null() {
        let w = m[s].envelop;
        if (w == 4 || w == 10) && s != node {
            return true;
        }
        s = m[s].brother;
    }
    false
}
