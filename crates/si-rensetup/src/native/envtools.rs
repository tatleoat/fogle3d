// SPDX-License-Identifier: MIT
//! `envtools.c`: what the copy of a master's hierarchy needs of a node's envelope: the
//! level of an envelope record, the test for a global envelope's main node, the three
//! allocators, the copies of a vertex list and of a record, and `SI_envelopCopy`, which
//! decides what the copy of a node keeps of its master's envelope. (The file's other
//! routines build and deform envelopes.)

use super::globals::SI_IDENTITE;
use super::rec::{EnvInfo, EnvList, EnvVert};
use super::skeleton::ske_chain_root;
use si_math::algebra::matrix_copy;
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::x87::F80;

type Node = Ptr32<Model>;

/// A float loaded into the unit and stored again.
fn through(v: f32) -> f32 {
    F80::from_f32(v).to_f32()
}

/// The level of a node's envelope record [soft 0x084bb7b0 SI_envelopLevel]: −1 for no node
/// or a node without record.
pub fn envelop_level(m: &Mem, model: Node) -> i32 {
    if model.is_null() {
        return -1;
    }
    let info: Ptr32<EnvInfo> = m[model].env_skel_jnt.cast();
    if info.is_null() {
        return -1;
    }
    m[info].level
}

/// A new envelope vertex [soft 0x084bba28 SI_envelopVertAllocate]: 0x14 bytes from
/// `SI_malloc`, cleared but for the weight, which is 100. The block is written without a
/// test for null.
pub fn envelop_vert_allocate(m: &mut Mem) -> Ptr32<EnvVert> {
    let v: Ptr32<EnvVert> = Ptr32::new(m.si_malloc(0x14));
    m[v].ratio = 0.0;
    m[v].thickness = 0.0;
    m[v].weight = 100.0;
    m[v].index = 0;
    m[v].next = Ptr32::NULL;
    v
}

/// A new envelope list [soft 0x084bba60 SI_envelopListAllocate]: 0x10 bytes from
/// `SI_malloc`, the argument in the first word, the rest cleared; written without a test
/// for null.
pub fn envelop_list_allocate(m: &mut Mem, owner: u32) -> Ptr32<EnvList> {
    let l: Ptr32<EnvList> = Ptr32::new(m.si_malloc(0x10));
    m[l].owner = owner;
    m[l].verts = Ptr32::NULL;
    m[l].prev = Ptr32::NULL;
    m[l].next = Ptr32::NULL;
    l
}

/// A new envelope record [soft 0x084bba94 SI_envelopInfoAllocate]: 0xa0 bytes from
/// `SI_malloc`; null when there is no memory. Level −1, no list, the identity matrix
/// (copied from `SI_identite`), nine words of zero, joint area 0, both curves absent and
/// both settings 0, 1, 0, 1.
pub fn envelop_info_allocate(m: &mut Mem) -> Ptr32<EnvInfo> {
    let e: Ptr32<EnvInfo> = Ptr32::new(m.si_malloc(0xa0));
    if e.is_null() {
        return e;
    }
    m[e].level = -1;
    m[e].lists = Ptr32::NULL;
    // SI_matrixCopy (0x08516224) is si-math's
    let identity = m[SI_IDENTITE];
    m[e].matrix = matrix_copy(&identity);
    m[e].unk_48 = [0; 9];
    m[e].joint_area = 0.0;
    m[e].inner = [0.0, 1.0, 0.0, 1.0];
    m[e].outer = [0.0, 1.0, 0.0, 1.0];
    m[e].in_type = 0;
    m[e].in_curve = Ptr32::NULL;
    m[e].out_type = 0;
    m[e].out_curve = Ptr32::NULL;
    e
}

/// 1 when a node carries a global envelope whose own sons include its main node, else 0
/// [soft 0x084bb94c SI_mainGlobEnv]: the node's envelope word (+0x31c) is 6 or 7, and one
/// of its sons has an envelope record of level 0 and the envelope word 4 or 10.
pub fn main_glob_env(m: &Mem, model: Node) -> i32 {
    if model.is_null() || (m[model].envelop as u32).wrapping_sub(6) > 1 {
        return 0;
    }
    let mut son = m[model].son;
    while !son.is_null() {
        if envelop_level(m, son) == 0 && matches!(m[son].envelop, 4 | 10) {
            return 1;
        }
        son = m[son].brother;
    }
    0
}

/// A copy of a list of envelope vertices [soft 0x084bbb9c SI_envelopVertCopy]: null for an
/// empty list, and null as soon as a vertex cannot be had (what was copied so far is
/// lost). The three floats go through the unit.
pub fn envelop_vert_copy(m: &mut Mem, mut from: Ptr32<EnvVert>) -> Ptr32<EnvVert> {
    if from.is_null() {
        return from;
    }
    let mut head = Ptr32::NULL;
    let mut last: Ptr32<EnvVert> = Ptr32::NULL;
    loop {
        let v = envelop_vert_allocate(m);
        if v.is_null() {
            return v;
        }
        m[v].index = m[from].index;
        m[v].ratio = through(m[from].ratio);
        m[v].thickness = through(m[from].thickness);
        m[v].weight = through(m[from].weight);
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
/// With `lists`: the level is copied, and for each of the record's lists a new list (whose
/// first word is 0, not the source's) with a copy of its vertices, chained in order.
/// Without, the copy keeps the level −1 and has no list. Either way the joint area, the
/// two curve types, the two groups of four settings (floats through the unit) and, where
/// the source has them, the two curves of 200 floats each (0x320 bytes from `SI_malloc`,
/// written without a test for null) are copied. The matrix and the nine words after it
/// stay as the allocator set them.
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
            let l = envelop_list_allocate(m, 0);
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
    m[to].joint_area = through(m[from].joint_area);
    m[to].in_type = m[from].in_type;
    for i in 0..4 {
        m[to].inner[i] = through(m[from].inner[i]);
    }
    m[to].out_type = m[from].out_type;
    for i in 0..4 {
        m[to].outer[i] = through(m[from].outer[i]);
    }
    if !m[from].in_curve.is_null() {
        m[to].in_curve = Ptr32::new(m.si_malloc(0x320));
        for i in 0..200 {
            let v = through(m[m[from].in_curve.add(i)]);
            let slot = m[to].in_curve.add(i);
            m[slot] = v;
        }
    }
    if !m[from].out_curve.is_null() {
        m[to].out_curve = Ptr32::new(m.si_malloc(0x320));
        for i in 0..200 {
            let v = through(m[m[from].out_curve.add(i)]);
            let slot = m[to].out_curve.add(i);
            m[slot] = v;
        }
    }
    to
}

/// What the copy of a node keeps of its master's envelope [soft 0x084bc660 SI_envelopCopy].
/// `src` is the master's node, `father` the copy's father (or null), `copy` the copy, whose
/// envelope word (+0x31c) the caller has set already.
///
/// A source whose envelope word is 5 to 8 never gets a record. For 6 and 7 the copy's word
/// is cleared unless the source is the main node of a global envelope
/// ([`main_glob_env`]) or the father's word is 6 or 7; for 5 and 8 it is cleared unless
/// the father's word is 5, 8, 4 or 11.
///
/// Any other source: with the word 4 or 10 and no father the copy's word becomes 9. Then
/// the record (+0x324), when the source has one, is copied ([`envelop_info_copy`]), with
/// its lists unless one of two things holds. Both are asked of the chain roots
/// ([`ske_chain_root`]) of source and copy when the source is a joint or an effector, else
/// of the two nodes themselves: the source's record has level 0, its word is 4, 10 or 11
/// and the copy's word is 9; or the level is above 0 and the copy's father is null or has
/// a record without lists (that father's record is read without a test for null).
pub fn envelop_copy(m: &mut Mem, src: Node, father: Node, copy: Node) {
    let mut lists: i16 = 1;
    let word = m[src].envelop;
    if (word as u32).wrapping_sub(5) <= 3 {
        if (word as u32).wrapping_sub(6) <= 1 && main_glob_env(m, src) as i16 == 0 && (father.is_null() || (m[father].envelop as u32).wrapping_sub(6) > 1) {
            m[copy].envelop = 0;
        }
        let word = m[src].envelop;
        if word == 5 || word == 8 {
            if !father.is_null() && matches!(m[father].envelop, 5 | 8 | 4 | 0xb) {
                return;
            }
            m[copy].envelop = 0;
        }
        return;
    }
    if (word == 4 || word == 10) && father.is_null() {
        m[copy].envelop = 9;
    }
    let kind = m[src].kind;
    let (a, b) = if kind == 5 || (kind == 0 && (m[src].algo_type as u32).wrapping_sub(3) <= 1) { (ske_chain_root(m, src), ske_chain_root(m, copy)) } else { (src, copy) };
    let level = envelop_level(m, a);
    if level == 0 && matches!(m[a].envelop, 4 | 10 | 11) && m[b].envelop == 9 {
        lists = 0;
    } else if level > 0 {
        let up = m[b].father;
        if up.is_null() {
            lists = 0;
        } else {
            let record: Ptr32<EnvInfo> = m[up].env_skel_jnt.cast();
            if m[record].lists.is_null() {
                lists = 0;
            }
        }
    }
    let record: Ptr32<EnvInfo> = m[src].env_skel_jnt.cast();
    if !record.is_null() {
        m[copy].env_skel_jnt = envelop_info_copy(m, record, lists).cast();
    }
}
