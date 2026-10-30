// SPDX-License-Identifier: MIT
//! `klusters.c` and `klusters_b.c`: clusters of points. A cluster is a named, ascending
//! list of point numbers of one model node; a node's clusters hang in a chain from its
//! word at +0x348. Three more cluster records carry the per-vertex, per-polygon and
//! per-edge user data of a node (+0x408, +0x410, +0x418, each beside an array of user
//! data chains).
//!
//! A cluster's `state` says whether it is the active one (the one the tagged points are
//! shown and edited as) and whether it is marked for disposal: see [`crate::records::state`].
//!
//! Not here, because they are dialogs: `SI_dialogModelSetCluster (0x0843b9ac)`,
//! `klusterPrepareTextLine (0x0843be18)`, `klusterInfoUpdateItems (0x0843bf1c)`,
//! `SI_dialogClusterInfo (0x0843c014)`, `WrathStayGhosted (0x0843d884)`.

use crate::globals::*;
use crate::outside::*;
use crate::records::{geom, state, Kluster};
use crate::{cur_dict, rd3, V3};
use si_math::algebra::{matrix_inverse, matrix_multiply};
use si_mem::{clib, Mem, Ptr32};
use si_types::model::Model;
use si_x87::{fmov, x};

/// Offsets in the model node of the three (cluster, user data array) pairs.
pub const VERTEX_UD: u32 = 0x408;
pub const POLYGON_UD: u32 = 0x410;
pub const EDGE_UD: u32 = 0x418;

fn head(m: &Mem, model: Ptr32<Model>) -> Ptr32<Kluster> {
    m[model].klusters.cast()
}

fn set_head(m: &mut Mem, model: Ptr32<Model>, k: Ptr32<Kluster>) {
    m[model].klusters = k.cast();
}

fn index_at(m: &Mem, k: Ptr32<Kluster>, n: i32) -> u32 {
    m[k].indexes.addr().wrapping_add((n as u32).wrapping_mul(4))
}

/// The definition a routine works on: the stored one numbered `cur_def`.
fn current_def(m: &Mem, model: Ptr32<Model>) -> u32 {
    m.u32(m[model].defs.addr().wrapping_add(m[model].cur_def as u32 * 4))
}

/// A new cluster [soft 0x0843a9b0 SI_klusterAllocate]: 0x20 bytes from `SI_malloc`, no
/// name, no points, state 1 (not active). The word at +0x14 is not written.
pub fn kluster_allocate(m: &mut Mem) -> Ptr32<Kluster> {
    let k: Ptr32<Kluster> = Ptr32::new(m.si_malloc(0x20));
    if !k.is_null() {
        m[k].name = Ptr32::NULL;
        m[k].unk_08 = 0;
        m[k].count = 0;
        m[k].indexes = Ptr32::NULL;
        m[k].state = state::INACTIVE;
        m[k].anim = Ptr32::NULL;
        m[k].next = Ptr32::NULL;
    }
    k
}

/// The point number at a place of a cluster [soft 0x0843a9f8 SI_klusterGetTrueIndex]: for
/// a face (kind 2) the second word of pair `i`, where the bound tested is **twice** the
/// count; else word `i`. 0 for anything missing or out of range.
pub fn kluster_get_true_index(m: &Mem, model: Ptr32<Model>, k: Ptr32<Kluster>, i: i32) -> i32 {
    if model.is_null() || k.is_null() || m[k].indexes.is_null() {
        return 0;
    }
    let idx = m[k].indexes.addr();
    if m[model].kind == 2 {
        if i >= 0 && i < m[k].count.wrapping_add(m[k].count) {
            return m.i32(idx.wrapping_add((i as u32).wrapping_mul(8)).wrapping_add(4));
        }
    } else if i >= 0 && i < m[k].count {
        return m.i32(idx.wrapping_add((i as u32).wrapping_mul(4)));
    }
    0
}

/// The centre of a cluster's points [soft 0x0843aa54 klusterGetCOG]: the float sum of the
/// points `SI_elemGetVertex` answers for (rounded at every addition), each coordinate
/// divided by the cluster's count (not by the number of points found). 1 when the cluster
/// has points.
pub fn kluster_get_cog(m: &mut Mem, o: &mut dyn Outside, elem: u32, k: Ptr32<Kluster>, out: Ptr32<V3>) -> i32 {
    let frame = m.local(0x10);
    let (v_at, flag_at) = (frame, frame + 0xc);
    m.set_u16(flag_at, 0);
    let oa = out.addr();
    m.set_u32(oa + 8, 0);
    m.set_u32(oa + 4, 0);
    m.set_u32(oa, 0);
    let mut i = 0i32;
    while i < m[k].count {
        let model: Ptr32<Model> = Ptr32::new(m.u32(elem.wrapping_add(4)));
        let t = kluster_get_true_index(m, model, k, i);
        let chap = m[SI_CID][CID_MODELS] as u32;
        let dict = cur_dict(m);
        if o.call(m, &SI_ELEM_GET_VERTEX, &[dict, elem, chap, 2, 0, t as u32, v_at, flag_at]) != 0 {
            for c in [0u32, 4, 8] {
                let s = (x(m.f32(oa + c)) + m.f32(v_at + c)).to_f32();
                m.set_f32(oa + c, s);
            }
        }
        i += 1;
    }
    if m[k].count > 0 {
        for c in [0u32, 4, 8] {
            let q = (x(m.f32(oa + c)) / x(m[k].count)).to_f32();
            m.set_f32(oa + c, q);
        }
    }
    m.release(0x10);
    (m[k].count > 0) as i32
}

/// A new cluster at the end of a node's chain [soft 0x0843ab28 SI_klusterAddToList]. The
/// name pointer is kept, not copied. With `register` the cluster gets its element
/// (`SI_edAddCluster`). Null for no node.
pub fn kluster_add_to_list(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, name: u32, register: i16) -> Ptr32<Kluster> {
    if model.is_null() {
        return Ptr32::NULL;
    }
    let k = kluster_allocate(m);
    m[k].next = Ptr32::NULL;
    m[k].name = Ptr32::new(name);
    let mut last = head(m, model);
    if last.is_null() {
        set_head(m, model, k);
    } else {
        while !m[last].next.is_null() {
            last = m[last].next;
        }
        m[last].next = k;
    }
    if register != 0 {
        o.call(m, &SI_ED_ADD_CLUSTER, &[model.addr(), k.addr()]);
    }
    k
}

/// Room for the numbers of `n` points [soft 0x0843ab8c klusterAllocateIndexes]: `n` words,
/// twice that for a face (kind 2), from `SI_malloc`.
pub fn kluster_allocate_indexes(m: &mut Mem, kind: i32, n: i32) -> u32 {
    let n = if kind == 2 { n.wrapping_add(n) } else { n };
    m.si_malloc((n as u32).wrapping_shl(2))
}

/// Removes the relation between a cluster and the object that is its centre
/// [soft 0x0843abb8 klusterCenterRelationDelete] (relation type 0xf7 in the MODELS
/// chapter): the relation found from the cluster's element gives the serial number under
/// which the model's element holds it.
pub fn kluster_center_relation_delete(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, k: Ptr32<Kluster>) {
    let frame = m.local(0x14);
    let (i1, i2, a, b, c) = (frame + 0x10, frame + 0xc, frame + 4, frame + 8, frame);
    let done = |m: &mut Mem| m.release(0x14);
    let ke = o.call(m, &SI_ED_GET_ELEM_FROM_CLUSTER, &[k.addr(), i1]);
    if ke == 0 {
        return done(m);
    }
    let me = o.call(m, &SI_ED_GET_ELEM_FROM_MODEL, &[model.addr(), i2]);
    if me == 0 {
        return done(m);
    }
    let (dict, chap) = (cur_dict(m), m[SI_CID][CID_MODELS] as u32);
    if o.call(m, &SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, ke, 0, 0, 0xf7, chap, a, b]) == 0 {
        return done(m);
    }
    let dict = cur_dict(m);
    let rel = o.call(m, &SI_ELEMDICT_GET_CUR_RELATION, &[dict, ke, 0, 0]);
    if rel == 0 {
        return done(m);
    }
    let serial = m.u32(rel.wrapping_add(0xc));
    let (dict, chap) = (cur_dict(m), m[SI_CID][CID_MODELS] as u32);
    if o.call(m, &SI_ELEMDICT_FIND_RELATED_ELEM_SERIAL, &[dict, me, 1, 2, 0xf7, chap, a, c, serial]) == 0 {
        return done(m);
    }
    let serial = m.u32(rel.wrapping_add(0xc));
    let (dict, chap, own) = (cur_dict(m), m[SI_CID][CID_MODELS] as u32, m.u32(i2));
    o.call(m, &SI_ELEMDICT_REMOVE_SERIAL_REL_ID, &[dict, SI_RD.addr(), chap, own, 0xf7, serial]);
    done(m)
}

/// Takes a cluster out of its node's chain and frees it [soft 0x0843ace4 klusterDispose].
/// A cluster that is not in the chain is left alone. With `full`: its centre relation, its
/// animation and its element go (three calls); without: only its curves of types 0x2bd and
/// 0x2be and its key data. Then the name, the numbers and the record are freed.
pub fn kluster_dispose(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, k: Ptr32<Kluster>, full: i16) {
    if k.is_null() {
        return;
    }
    let first = head(m, model);
    if k == first {
        let next = m[k].next;
        set_head(m, model, next);
    } else {
        let mut prev = first;
        let mut found = false;
        let mut cur = m[prev].next;
        if !cur.is_null() {
            loop {
                found = cur == k;
                if !found {
                    prev = cur;
                }
                cur = m[prev].next;
                if cur.is_null() || found {
                    break;
                }
            }
        }
        if !found {
            return;
        }
        m[prev].next = m[k].next;
    }
    if full != 0 {
        kluster_center_relation_delete(m, o, model, k);
        o.call(m, &SI_MODEL_DELETE_CLUSTER_ANIM, &[model.addr(), k.addr()]);
        o.call(m, &SI_ED_DISPOSE_CLUSTER, &[model.addr(), k.addr()]);
    } else {
        let chap = m[SI_CID][CID_CLUSTERS] as u32;
        o.call(m, &SI_ED_FCV_DISPOSE, &[k.addr(), chap, 2, 0x2bd, 0x2be]);
        o.call(m, &SI_CLUSTER_DATA_DISPOSE, &[k.addr() + 0x18]);
        m[k].anim = Ptr32::NULL;
    }
    free_parts(m, k);
    m.free(k.addr());
}

/// Frees a cluster's name and numbers and clears both pointers.
fn free_parts(m: &mut Mem, k: Ptr32<Kluster>) {
    let name = m[k].name.addr();
    if name != 0 {
        m.free(name);
        m[k].name = Ptr32::NULL;
    }
    let idx = m[k].indexes.addr();
    if idx != 0 {
        m.free(idx);
        m[k].indexes = Ptr32::NULL;
    }
}

/// Disposes every cluster of a node, fully [soft 0x0843adf0 SI_klusterDisposeAll].
pub fn kluster_dispose_all(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) {
    if model.is_null() || head(m, model).is_null() {
        return;
    }
    loop {
        let first = head(m, model);
        kluster_dispose(m, o, model, first, 1);
        if head(m, model).is_null() {
            break;
        }
    }
}

/// The numbers of a node's tagged points [soft 0x0843ae2c klusterGetIndexes]: counted, and
/// unless `count_only` stored in `out` in ascending order. A point is tagged when bit 0 of
/// its flag byte is set: a mesh vertex's at +0x12, a patch point's in the patch's flag
/// array, a spline key's at +0x2c, a NURBS control point's at +0x20. The geometry is the
/// stored definition numbered `cur_def`; for a face it is the first definition and the
/// work is `SI_faceGetIndexes`'s. 0 for no node, no array or no definition; other kinds
/// have no points.
pub fn kluster_get_indexes(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, out: u32, count_only: i16) -> i32 {
    use geom::*;
    if model.is_null() || (count_only == 0 && out == 0) {
        return 0;
    }
    // (count, how many stored)
    let mut found = (0i32, 0u32);
    let hit = |m: &mut Mem, found: &mut (i32, u32), i: i32| {
        if count_only == 0 {
            m.set_i32(out.wrapping_add(found.1.wrapping_mul(4)), i);
            found.1 += 1;
        }
        found.0 += 1;
    };
    match m[model].kind {
        4 => {
            let def = current_def(m, model);
            if def == 0 {
                return 0;
            }
            let mut i = 0i32;
            while i < m.i32(def + MESH_NB_VERTS) {
                let v = m.u32(def + MESH_VERTS).wrapping_add((i as u32).wrapping_mul(VERT_SIZE));
                if m.u8(v.wrapping_add(VERT_FLAGS)) & 1 != 0 {
                    hit(m, &mut found, i);
                }
                i += 1;
            }
        }
        1 => {
            let def = current_def(m, model);
            if def == 0 {
                return 0;
            }
            let total = |m: &Mem| (m.u16(def + PATCH_NU) as i32).wrapping_mul(m.u16(def + PATCH_NV) as i32);
            let mut i = 0i32;
            while i < total(m) {
                let flags = m.u32(def + PATCH_FLAGS);
                if m.u8(flags.wrapping_add((i as u32).wrapping_mul(2))) & 1 != 0 {
                    hit(m, &mut found, i);
                }
                i += 1;
            }
        }
        2 => {
            let def = m.u32(m[model].defs.addr());
            let slot = m.local(4);
            m.set_u32(slot, out);
            found.0 = o.call(m, &SI_FACE_GET_INDEXES, &[def, slot, count_only as u16 as u32, 1]) as i32;
            m.release(4);
        }
        6 => {
            let def = current_def(m, model);
            if def == 0 {
                return 0;
            }
            let mut i = 0i32;
            while i < m.u16(def + SPLINE_NB_KEYS) as i32 {
                let key = m.u32(def + SPLINE_KEYS).wrapping_add((i as u32).wrapping_mul(KEY_SIZE));
                if m.u8(key.wrapping_add(KEY_FLAGS)) & 1 != 0 {
                    hit(m, &mut found, i);
                }
                i += 1;
            }
        }
        9 | 10 => {
            let def = current_def(m, model);
            if def == 0 {
                return 0;
            }
            let (nb, pts) = if m[model].kind == 9 { (CRV_NB_POINTS, CRV_POINTS) } else { (SRF_NB_POINTS, SRF_POINTS) };
            let mut i = 0i32;
            while i < m.i32(def + nb) {
                let cp = m.u32(def + pts).wrapping_add((i as u32).wrapping_mul(CP_SIZE));
                if m.u8(cp.wrapping_add(CP_FLAGS)) & 1 != 0 {
                    hit(m, &mut found, i);
                }
                i += 1;
            }
        }
        _ => {}
    }
    found.0
}

/// A name no cluster of the node has [soft 0x0843b044 klusterGetDistinctName]: the first
/// of `cls0`, `cls1`, … up to `cls999999` that is free, as a new string; null when the
/// node is null or all are taken. A cluster without a name is skipped.
pub fn kluster_get_distinct_name(m: &mut Mem, model: Ptr32<Model>) -> u32 {
    if model.is_null() {
        return 0;
    }
    let buf = m.local(0x14);
    let mut n = 0i32;
    loop {
        m.put_cstr(buf, format!("cls{n}").as_bytes());
        let mut k = head(m, model);
        loop {
            if k.is_null() {
                let copy = m.strdup(buf);
                m.release(0x14);
                return copy;
            }
            let name = m[k].name.addr();
            if name != 0 && clib::strcmp(m, buf, name) == 0 {
                break;
            }
            k = m[k].next;
        }
        n += 1;
        if n > 999_999 {
            break;
        }
    }
    m.release(0x14);
    0
}

/// The cluster of a node with a given name [soft 0x0843b0b4 SI_klusterGetByName]; null
/// when either argument is null or no cluster has it.
pub fn kluster_get_by_name(m: &Mem, model: Ptr32<Model>, name: u32) -> Ptr32<Kluster> {
    if model.is_null() || name == 0 {
        return Ptr32::NULL;
    }
    let mut k = head(m, model);
    while !k.is_null() {
        if clib::strcmp(m, m[k].name.addr(), name) == 0 {
            break;
        }
        k = m[k].next;
    }
    k
}

/// The n-th cluster of a node, not counting those marked for disposal
/// [soft 0x0843b114 klusterGetByNumber].
pub fn kluster_get_by_number(m: &Mem, model: Ptr32<Model>, n: i32) -> Ptr32<Kluster> {
    let mut at = -1i32;
    let mut k = head(m, model);
    while !k.is_null() {
        if m[k].state != state::DISPOSE {
            at = at.wrapping_add(1);
        }
        if at == n {
            return k;
        }
        k = m[k].next;
    }
    Ptr32::NULL
}

/// Tags exactly a cluster's points in one definition of its node
/// [soft 0x0843b148 klusterTagCVs]: bit 0 of every point's flag byte is set when the point
/// is the next one in the cluster's (ascending) list and cleared otherwise.
///
/// `def_index` below the node's number of definitions names a stored definition (nothing
/// happens when its pointer is null); any other value means the working definition, except
/// for a face, where it means nothing at all. A face's cluster holds pairs (curve, point),
/// the outline being curve -1 of a face made of splines.
pub fn kluster_tag_cvs(m: &mut Mem, model: Ptr32<Model>, k: Ptr32<Kluster>, def_index: i32) {
    use geom::*;
    if k.is_null() {
        return;
    }
    let count = m[k].count;
    let kind = m[model].kind;
    if !matches!(kind, 1 | 2 | 4 | 6 | 9 | 10) {
        return;
    }
    let stored = def_index < m[model].nb_defs as i32;
    let def = if stored {
        m.u32(m[model].defs.addr().wrapping_add((def_index as u32).wrapping_mul(4)))
    } else if kind == 2 {
        0
    } else {
        m[model].definition.addr()
    };
    if (stored || kind == 2) && def == 0 {
        return;
    }
    // a face's list is pairs: twice the words
    let cnt = if kind == 2 { count.wrapping_add(count) } else { count };
    let mut j = 0i32;
    // sets or clears one flag byte: `is_next` says whether the point is the cluster's next;
    // answers how far the place in the list moves
    let tag = |m: &mut Mem, at: u32, is_next: bool, step: i32| -> i32 {
        if is_next {
            m.or_u8(at, 1);
            step
        } else {
            m.and_u8(at, 0xfe);
            0
        }
    };
    let single = |m: &Mem, j: i32, n: i32| j < cnt && m.i32(index_at(m, k, j)) == n;
    match kind {
        4 => {
            let mut v = m.u32(def + MESH_VERTS);
            let mut i = 0i32;
            while i < m.i32(def + MESH_NB_VERTS) {
                let is = single(m, j, i);
                j += tag(m, v.wrapping_add(VERT_FLAGS), is, 1);
                i += 1;
                v = v.wrapping_add(VERT_SIZE);
            }
        }
        1 => {
            let total = |m: &Mem| (m.u16(def + PATCH_NU) as i32).wrapping_mul(m.u16(def + PATCH_NV) as i32);
            let mut i = 0i32;
            while i < total(m) {
                let is = single(m, j, i);
                let flags = m.u32(def + PATCH_FLAGS);
                j += tag(m, flags.wrapping_add((i as u32).wrapping_mul(2)), is, 1);
                i += 1;
            }
        }
        2 => {
            let pair = |m: &Mem, j: i32, curve: i32, point: i32| j < cnt && m.i32(index_at(m, k, j)) == curve && m.i32(index_at(m, k, j + 1)) == point;
            if m.i16(def + FACE_IS_SPLINE) != 0 {
                if m.u8(def + FACE_NB_HOLES + 1) & 0x80 != 0 {
                    return;
                }
                let mut s = -1i32;
                loop {
                    let sp = if s == -1 { def + FACE_OUTLINE } else { m.u32(def + FACE_HOLES).wrapping_add((s as u32).wrapping_mul(FACE_SPLINE_SIZE)) };
                    if sp != 0 && m.u16(sp + SPLINE_NB_KEYS) != 0 {
                        let mut b = 0i32;
                        loop {
                            let is = pair(m, j, s, b);
                            let key = m.u32(sp + SPLINE_KEYS).wrapping_add((b as u32).wrapping_mul(KEY_SIZE));
                            j += tag(m, key.wrapping_add(KEY_FLAGS), is, 2);
                            b += 1;
                            if b >= m.u16(sp + SPLINE_NB_KEYS) as i32 {
                                break;
                            }
                        }
                    }
                    s += 1;
                    if s >= m.i16(def + FACE_NB_HOLES) as i32 {
                        break;
                    }
                }
            } else {
                let mut s = 0i32;
                if m.i16(def + FACE_NB_HOLES) <= 0 {
                    return;
                }
                loop {
                    let c = m.u32(def + FACE_NURBS).wrapping_add((s as u32).wrapping_mul(FACE_NURBS_SIZE));
                    let mut b = 0i32;
                    while b < m.i32(c + CRV_NB_POINTS) {
                        let is = pair(m, j, s, b);
                        let cp = m.u32(c + CRV_POINTS).wrapping_add((b as u32).wrapping_mul(CP_SIZE));
                        j += tag(m, cp.wrapping_add(CP_FLAGS), is, 2);
                        b += 1;
                    }
                    s += 1;
                    if s >= m.i16(def + FACE_NB_HOLES) as i32 {
                        break;
                    }
                }
            }
        }
        6 => {
            let mut b = 0i32;
            if m.u16(def + SPLINE_NB_KEYS) != 0 {
                loop {
                    let is = single(m, j, b);
                    let key = m.u32(def + SPLINE_KEYS).wrapping_add((b as u32).wrapping_mul(KEY_SIZE));
                    j += tag(m, key.wrapping_add(KEY_FLAGS), is, 1);
                    b += 1;
                    if b >= m.u16(def + SPLINE_NB_KEYS) as i32 {
                        break;
                    }
                }
            }
        }
        _ => {
            let (nb, pts) = if kind == 9 { (CRV_NB_POINTS, CRV_POINTS) } else { (SRF_NB_POINTS, SRF_POINTS) };
            let mut i = 0i32;
            while i < m.i32(def + nb) {
                let is = single(m, j, i);
                let cp = m.u32(def + pts).wrapping_add((i as u32).wrapping_mul(CP_SIZE));
                j += tag(m, cp.wrapping_add(CP_FLAGS), is, 1);
                i += 1;
            }
        }
    }
}

/// The node's active cluster [soft 0x0843b4b8 SI_klusterGetActive]: the first whose state
/// is 0 or 3.
pub fn kluster_get_active(m: &Mem, model: Ptr32<Model>) -> Ptr32<Kluster> {
    let mut k = head(m, model);
    while !k.is_null() {
        if m[k].state == state::ACTIVE || m[k].state == state::ACTIVE_FLAGGED {
            break;
        }
        k = m[k].next;
    }
    k
}

/// Makes one cluster of a node the active one, or none
/// [soft 0x0843b4f0 SI_klusterActivate]: every other cluster goes from 0 to 1 or from 3 to
/// 4, the one given from 1 to 0 or from 4 to 3. With `tag`: the cluster's points are
/// tagged in every stored definition and in the working one ([`kluster_tag_cvs`] for the
/// numbers 0 to `nb_defs` inclusive); for no cluster `SI_tagClear(clear)` is called.
pub fn kluster_activate(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, k: Ptr32<Kluster>, tag: i16, clear: i16) {
    if model.is_null() {
        return;
    }
    let mut c = head(m, model);
    while !c.is_null() {
        let is = c == k;
        m[c].state = match (m[c].state, is) {
            (state::ACTIVE, false) => state::INACTIVE,
            (state::INACTIVE, true) => state::ACTIVE,
            (state::ACTIVE_FLAGGED, false) => state::INACTIVE_FLAGGED,
            (state::INACTIVE_FLAGGED, true) => state::ACTIVE_FLAGGED,
            (s, _) => s,
        };
        c = m[c].next;
    }
    if tag == 0 {
        return;
    }
    if k.is_null() {
        o.call(m, &SI_TAG_CLEAR, &[clear as u16 as u32]);
        return;
    }
    let mut i = 0i32;
    loop {
        kluster_tag_cvs(m, model, k, i);
        i += 1;
        if i >= m[model].nb_defs as i32 + 1 {
            break;
        }
    }
}

/// [`kluster_activate`] for the cluster of a name, with tagging
/// [soft 0x0843b5e0 klusterActivateByName]. Called from nowhere in the program.
pub fn kluster_activate_by_name(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, name: u32) {
    let k = kluster_get_by_name(m, model, name);
    kluster_activate(m, o, model, k, 1, 1);
}

/// Carries out what was marked [soft 0x0843b60c SI_klusterDisposeFlagged]: a cluster in
/// state 3 or 4 loses its animation and returns to 0 or 1; one in state 2 is disposed.
pub fn kluster_dispose_flagged(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) {
    let mut k = head(m, model);
    while !k.is_null() {
        if (m[k].state as u32).wrapping_sub(3) <= 1 {
            o.call(m, &SI_MODEL_DELETE_CLUSTER_ANIM, &[model.addr(), k.addr()]);
            m[k].state = (m[k].state != state::ACTIVE_FLAGGED) as i32;
        }
        if m[k].state == state::DISPOSE {
            let next = m[k].next;
            kluster_dispose(m, o, model, k, 1);
            k = next;
        } else {
            k = m[k].next;
        }
    }
}

/// Marks a cluster [soft 0x0843b680 klusterSetToDispose]: for disposal (state 2), or with
/// `anim_only` for losing its animation (0 becomes 3, 1 becomes 4).
pub fn kluster_set_to_dispose(m: &mut Mem, k: Ptr32<Kluster>, anim_only: i16) {
    if anim_only == 0 {
        m[k].state = state::DISPOSE;
    } else if m[k].state == state::ACTIVE {
        m[k].state = state::ACTIVE_FLAGGED;
    } else if m[k].state == state::INACTIVE {
        m[k].state = state::INACTIVE_FLAGGED;
    }
}

/// Takes every mark back [soft 0x0843b6bc klusterResetDisposeFlags]: 3 becomes 0, 2 and 4
/// become 1 (a cluster marked for disposal comes back not active).
pub fn kluster_reset_dispose_flags(m: &mut Mem, model: Ptr32<Model>) {
    let mut k = head(m, model);
    while !k.is_null() {
        match m[k].state {
            state::ACTIVE_FLAGGED => m[k].state = state::ACTIVE,
            state::DISPOSE | state::INACTIVE_FLAGGED => m[k].state = state::INACTIVE,
            _ => {}
        }
        k = m[k].next;
    }
}

/// Brings a cluster's shape keys in step with a new list of points
/// [soft 0x0843b708 klusterCompare]: the old and the new ascending lists are merged; a
/// number only in the old one is `SI_clusterAnimRemoveVertex` at the current place, one
/// only in the new one `SI_clusterAnimAddVertex` there.
pub fn kluster_compare(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, k: Ptr32<Kluster>, old: u32, new: u32, n_old: i32, n_new: i32) {
    let (mut pos, mut j, mut i) = (0i32, 0i32, 0i32);
    let args = |pos: i32| [model.addr(), k.addr(), pos as u32];
    if j < n_old {
        let mut old_left = true;
        if j < n_new {
            loop {
                let a = m.i32(old.wrapping_add((i as u32).wrapping_mul(4)));
                let b = m.i32(new.wrapping_add((j as u32).wrapping_mul(4)));
                if a == b {
                    i += 1;
                    j += 1;
                    pos += 1;
                } else if a < b {
                    o.call(m, &SI_CLUSTER_ANIM_REMOVE_VERTEX, &args(pos));
                    i += 1;
                } else {
                    o.call(m, &SI_CLUSTER_ANIM_ADD_VERTEX, &args(pos));
                    j += 1;
                    pos += 1;
                }
                if i >= n_old {
                    old_left = false;
                    break;
                }
                if j >= n_new {
                    break;
                }
            }
        }
        if old_left && i < n_old {
            let mut left = n_old.wrapping_sub(i);
            loop {
                o.call(m, &SI_CLUSTER_ANIM_REMOVE_VERTEX, &args(pos));
                left = left.wrapping_sub(1);
                if left == 0 {
                    break;
                }
            }
        }
    }
    if j < n_new {
        let mut left = n_new.wrapping_sub(j);
        loop {
            o.call(m, &SI_CLUSTER_ANIM_ADD_VERTEX, &args(pos));
            pos += 1;
            left = left.wrapping_sub(1);
            if left == 0 {
                break;
            }
        }
    }
}

/// Makes a cluster of the node's tagged points [soft 0x0843b7d0 klusterAddToModel]: a new
/// one of the given name (`create`), or the active one with its list replaced. 0 when
/// there is no cluster to work on, -1 when no point is tagged or there is no memory (a
/// newly created cluster then stays in the chain, empty), else 1. A replaced list goes
/// through [`kluster_compare`] and is freed. The cluster ends up active, without tagging.
pub fn kluster_add_to_model(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, name: u32, create: i16) -> i32 {
    let k = if create != 0 { kluster_add_to_list(m, o, model, name, 1) } else { kluster_get_active(m, model) };
    if k.is_null() {
        return 0;
    }
    let (mut old, mut n_old) = (0u32, 0i32);
    if create == 0 {
        old = m[k].indexes.addr();
        n_old = m[k].count;
    }
    let n = kluster_get_indexes(m, o, model, 0, 1);
    if n <= 0 {
        return -1;
    }
    let idx = kluster_allocate_indexes(m, m[model].kind, n);
    m[k].indexes = Ptr32::new(idx);
    m[k].count = n;
    if idx == 0 {
        return -1;
    }
    let idx = m[k].indexes.addr();
    kluster_get_indexes(m, o, model, idx, 0);
    if create == 0 {
        let (new, n_new) = (m[k].indexes.addr(), m[k].count);
        kluster_compare(m, o, model, k, old, new, n_old, n_new);
        if old != 0 {
            m.free(old);
        }
    }
    kluster_activate(m, o, model, k, 0, 1);
    1
}

/// How many points of a node are tagged [soft 0x0843b8d8 modelTagCount].
pub fn model_tag_count(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> i32 {
    kluster_get_indexes(m, o, model, 0, 1)
}

/// The matrix a cluster's centre constraint keeps [soft 0x0843b8f4 SI_klusterCnsSetData]:
/// the model's global matrix times the inverse of the centre object's, copied to `out`.
/// Nothing when an argument is null. When the centre's matrix has no inverse,
/// `SI_matrixInverse` leaves its result alone and the product is made from a local nobody
/// set (here: a local of the image's stack).
pub fn kluster_cns_set_data(m: &mut Mem, model: Ptr32<Model>, centre: Ptr32<Model>, out: Ptr32<[[f32; 4]; 4]>) {
    if model.is_null() || centre.is_null() || out.is_null() {
        return;
    }
    let inv_at: Ptr32<[[f32; 4]; 4]> = Ptr32::new(m.local(0x40));
    let centre_matrix = m[centre].gmat;
    if let Some(inv) = matrix_inverse(&centre_matrix) {
        m.write(inv_at, inv);
    }
    let (a, b) = (m[model].gmat, m.read(inv_at));
    let product = matrix_multiply(&a, &b);
    m.write(out, product.map(|row| row.map(fmov)));
    m.release(0x40);
}

/// Whether no other cluster of the node has a name [soft 0x0843b958 SI_isClusterNameUnique].
pub fn is_cluster_name_unique(m: &Mem, model: Ptr32<Model>, k: Ptr32<Kluster>, name: u32) -> i32 {
    let mut c = head(m, model);
    while !c.is_null() {
        if c != k && clib::strcmp(m, m[c].name.addr(), name) == 0 {
            return 0;
        }
        c = m[c].next;
    }
    1
}

/// Offsets in the dictionary records the two renaming routines below reach (`si-types` has
/// them as `Elem`, `DbListNode`, `ElemDict` and `ElemList`; they are addressed by offset
/// here because the routines get elements as plain addresses from the editing layer).
mod dict {
    /// `Elem` (0x84 bytes): +0 the name record, +0x48 status (0 in use), +0x50 the index of
    /// the hierarchy's root.
    pub const ELEM_SIZE: u32 = 0x84;
    pub const ELEM_STATUS: u32 = 0x48;
    pub const ELEM_ROOT: u32 = 0x50;
    /// `DbListNode`: three strings of its own.
    pub const NODE_PREFIX: u32 = 0x10;
    pub const NODE_NAME: u32 = 0x14;
    pub const NODE_FULL: u32 = 0x24;
    /// `ElemDict` +4: the chapters, 0x40 bytes each: +4 how many elements, +8 the elements.
    pub const DICT_LISTS: u32 = 0x04;
    pub const LIST_SIZE: u32 = 0x40;
    pub const LIST_NB: u32 = 0x04;
    pub const LIST_ELEMS: u32 = 0x08;
}

/// Frees the string a word points to and clears the word.
fn free_str(m: &mut Mem, at: u32) {
    let s = m.u32(at);
    if s != 0 {
        m.free(s);
        m.set_u32(at, 0);
    }
}

/// Stores a copy of a string in a word.
fn dup_into(m: &mut Mem, at: u32, s: u32) {
    let copy = m.strdup(s);
    m.set_u32(at, copy);
}

/// Appends chain `b` to chain `a` [soft 0x0843c9d4 SI_klusterListMerge] and answers the
/// merged chain (`b` when `a` is null).
///
/// Before that every cluster of `b` whose name a cluster of `a` has is renamed to that name
/// with `_` appended (compared again with the rest of `a` under its new name): the cluster,
/// its element's name record (name; the full name rebuilt by `SI_dbFormatFullName`), the
/// name of the element related to it by relation 0x3a9a, and **the prefix** of every
/// element in use of the dictionary's first chapter (`SI_cid[0]`) that has the same
/// hierarchy root as that related element and is not it.
pub fn kluster_list_merge(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Kluster>, b: Ptr32<Kluster>) -> Ptr32<Kluster> {
    use dict::*;
    if a.is_null() {
        return b;
    }
    let mut last = a;
    while !m[last].next.is_null() {
        last = m[last].next;
    }
    let frame = m.local(0x4c);
    let (buf, idx_at, chap_at, root_at) = (frame + 0x10, frame + 0xc, frame + 4, frame + 8);
    let mut kb = b;
    while !kb.is_null() {
        let eb = o.call(m, &SI_ED_GET_ELEM_FROM_CLUSTER, &[kb.addr(), idx_at]);
        let mut ka = a;
        while !ka.is_null() {
            if clib::strcmp(m, m[ka].name.addr(), m[kb].name.addr()) == 0 {
                let mut text = m.cstr(m[ka].name.addr());
                text.push(b'_');
                m.put_cstr(buf, &text);
                free_str(m, kb.addr());
                dup_into(m, kb.addr(), buf);
                free_str(m, m.u32(eb) + NODE_NAME);
                free_str(m, m.u32(eb) + NODE_FULL);
                dup_into(m, m.u32(eb) + NODE_NAME, buf);
                let node = m.u32(eb);
                let full = o.call(m, &SI_DB_FORMAT_FULL_NAME, &[node]);
                m.set_u32(m.u32(eb) + NODE_FULL, full);
                let (dict_at, first) = (cur_dict(m), m[SI_CID][CID_FIRST] as u32);
                let rel = o.call(m, &SI_ELEMDICT_FIND_RELATED_ELEM, &[dict_at, eb, 1, 0, 0x3a9a, first, chap_at, root_at]);
                if rel != 0 {
                    free_str(m, m.u32(rel) + NODE_NAME);
                    dup_into(m, m.u32(rel) + NODE_NAME, buf);
                    let lists = m.u32(cur_dict(m) + DICT_LISTS);
                    let chapter = lists.wrapping_add((m[SI_CID][CID_FIRST] as u32).wrapping_mul(LIST_SIZE));
                    let (mut n, mut off) = (0i32, 0u32);
                    while n < m.i32(chapter + LIST_NB) {
                        let el = m.u32(chapter + LIST_ELEMS).wrapping_add(off);
                        if m.i16(el + ELEM_STATUS) == 0 && m.u32(el + ELEM_ROOT) == m.u32(root_at) && el != rel {
                            free_str(m, m.u32(el) + NODE_PREFIX);
                            dup_into(m, m.u32(el) + NODE_PREFIX, buf);
                        }
                        n += 1;
                        off = off.wrapping_add(ELEM_SIZE);
                    }
                }
            }
            ka = m[ka].next;
        }
        kb = m[kb].next;
    }
    m[last].next = b;
    m.release(0x4c);
    a
}

/// Gives a node a chain of clusters [soft 0x0843cd6c SI_klusterListAssignToModel] and
/// brings the dictionary in step: per cluster its element's relations in the CLUSTERS
/// chapter are removed and the relation 0x2bc from the model's element to it is made; the
/// element's name becomes the node's name followed by the cluster's (formatted in the
/// file's static buffer), its prefix the model element's (when that has one), its full
/// name is rebuilt; the element related to it by relation 0x3a9a gets the same prefix and
/// name. A cluster without an element only gets the relation.
pub fn kluster_list_assign_to_model(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, list: Ptr32<Kluster>) {
    use dict::*;
    set_head(m, model, list);
    let frame = m.local(0x10);
    let (mi_at, ki_at, chap_at, root_at) = (frame + 0xc, frame + 8, frame, frame + 4);
    let me = o.call(m, &SI_ED_GET_ELEM_FROM_MODEL, &[model.addr(), mi_at]);
    let mut k = head(m, model);
    while !k.is_null() {
        let ke = o.call(m, &SI_ED_GET_ELEM_FROM_CLUSTER, &[k.addr(), ki_at]);
        if ke != 0 {
            let (dict_at, chap, ki) = (cur_dict(m), m[SI_CID][CID_CLUSTERS] as u32, m.u32(ki_at));
            o.call(m, &SI_REL_REMOVE_RELATIONS, &[dict_at, ke, chap, ki, 1]);
        }
        let (dict_at, mi, ki) = (cur_dict(m), m.u32(mi_at), m.u32(ki_at));
        o.call(m, &SI_ELEMDICT_ADD_RELATION, &[dict_at, SI_RD.addr(), mi, ki, 0x2bc, 1, 0]);
        if ke != 0 {
            free_str(m, m.u32(ke) + NODE_NAME);
            free_str(m, m.u32(ke) + NODE_FULL);
            if m.u32(m.u32(me) + NODE_PREFIX) != 0 {
                free_str(m, m.u32(ke) + NODE_PREFIX);
                let prefix = m.u32(m.u32(me) + NODE_PREFIX);
                dup_into(m, m.u32(ke) + NODE_PREFIX, prefix);
            }
            let mut text = m.cstr(m[model].name.addr());
            text.extend(m.cstr(m[k].name.addr()));
            m.put_cstr(WORK_STR.addr(), &text);
            dup_into(m, m.u32(ke) + NODE_NAME, WORK_STR.addr());
            let node = m.u32(ke);
            let full = o.call(m, &SI_DB_FORMAT_FULL_NAME, &[node]);
            m.set_u32(m.u32(ke) + NODE_FULL, full);
            let (dict_at, first) = (cur_dict(m), m[SI_CID][CID_FIRST] as u32);
            let rel = o.call(m, &SI_ELEMDICT_FIND_RELATED_ELEM, &[dict_at, ke, 1, 0, 0x3a9a, first, chap_at, root_at]);
            if rel != 0 {
                free_str(m, m.u32(rel) + NODE_NAME);
                free_str(m, m.u32(rel) + NODE_FULL);
                if m.u32(m.u32(me) + NODE_PREFIX) != 0 {
                    free_str(m, m.u32(rel) + NODE_PREFIX);
                    let prefix = m.u32(m.u32(me) + NODE_PREFIX);
                    dup_into(m, m.u32(rel) + NODE_PREFIX, prefix);
                }
                let name = m.u32(m.u32(ke) + NODE_NAME);
                dup_into(m, m.u32(rel) + NODE_NAME, name);
                let node = m.u32(rel);
                let full = o.call(m, &SI_DB_FORMAT_FULL_NAME, &[node]);
                m.set_u32(m.u32(rel) + NODE_FULL, full);
            }
        }
        k = m[k].next;
    }
    m.release(0x10);
}

/// A copy of a node's chain of clusters [soft 0x0843cc70 SI_modelklusterListDuplicate],
/// answered as a chain of its own while the node keeps the old one. Each copy has a copy
/// of the name, the state, the count and the numbers (twice the count of words for a
/// face); `register` is handed to [`kluster_add_to_list`]; with `anim` = 1 the shape keys
/// are copied too (`SI_clusterAnimDuplicate`, which gets `mode`).
pub fn model_kluster_list_duplicate(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, mode: i16, register: i16, anim: i16) -> Ptr32<Kluster> {
    let old = head(m, model);
    let mut s = old;
    set_head(m, model, Ptr32::NULL);
    while !s.is_null() {
        let name = m.strdup(m[s].name.addr());
        let n = kluster_add_to_list(m, o, model, name, register);
        m[n].state = m[s].state;
        m[n].count = m[s].count;
        let idx = kluster_allocate_indexes(m, m[model].kind, m[s].count);
        m[n].indexes = Ptr32::new(idx);
        let mut words = m[s].count;
        if m[model].kind == 2 {
            words = words.wrapping_add(words);
        }
        let mut i = 0i32;
        while i < words {
            let v = m.u32(index_at(m, s, i));
            m.set_u32(index_at(m, n, i), v);
            i += 1;
        }
        if anim == 1 {
            let a = o.call(m, &SI_CLUSTER_ANIM_DUPLICATE, &[model.addr(), n.addr(), s.addr(), mode as u16 as u32]);
            m[n].anim = Ptr32::new(a);
        }
        s = m[s].next;
    }
    let new = head(m, model);
    set_head(m, model, old);
    new
}

/// Takes one point out of a cluster [soft 0x0843cfec klusterRemoveIndex]: the numbers
/// behind `index` move down (for a face `index` counts words and a pair goes), the count
/// drops by one and, while points are left, the array is shrunk with `SI_realloc`. `ud` is
/// null or points at the pointer to a parallel array of user data chains, which loses the
/// same place (`SI_userDataDispose`, then the same move and `SI_realloc`).
pub fn kluster_remove_index(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, k: Ptr32<Kluster>, ud: u32, index: i16) {
    if model.is_null() {
        return;
    }
    let face = m[model].kind == 2;
    let slot = |i: i16| ((if face { i / 2 } else { i }) as i32 as u32).wrapping_mul(4);
    if ud != 0 {
        let arr = m.u32(ud);
        o.call(m, &SI_USER_DATA_DISPOSE, &[arr.wrapping_add(slot(index))]);
    }
    let mut at = index;
    let mut n = m[k].count;
    loop {
        let c = at as i32;
        let limit = if face { n.wrapping_mul(2).wrapping_sub(2) } else { n.wrapping_sub(1) };
        if c >= limit {
            break;
        }
        let words = if face { 2 } else { 1 };
        for w in 0..words {
            let v = m.u32(index_at(m, k, c + w + words));
            m.set_u32(index_at(m, k, c + w), v);
        }
        if ud != 0 {
            let arr = m.u32(ud).wrapping_add(slot(at));
            let v = m.u32(arr.wrapping_add(4));
            m.set_u32(arr, v);
        }
        n = m[k].count;
        at = at.wrapping_add(words as i16);
    }
    let left = n.wrapping_sub(1);
    m[k].count = left;
    if left > 0 {
        let size = (left as u32).wrapping_shl(if face { 3 } else { 2 });
        let grown = m.si_realloc(m[k].indexes.addr(), size);
        m[k].indexes = Ptr32::new(grown);
        if ud != 0 {
            let grown = m.si_realloc(m.u32(ud), (m[k].count as u32).wrapping_shl(2));
            m.set_u32(ud, grown);
        }
    }
}

/// Sorts a cluster's numbers [soft 0x0843d140 klusterSortIndexes] with `SI_quicksort`
/// (which moves the parallel user data array with them). When the cluster has shape keys
/// the sort notes its exchanges and each is repeated on the keys
/// (`SI_clusterAnimSwapVertices`).
pub fn kluster_sort_indexes(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, k: Ptr32<Kluster>, ud: u32) {
    let has_keys = !m[k].anim.is_null();
    let parallel = if ud != 0 { m.u32(ud) } else { 0 };
    let last = m[k].count.wrapping_sub(1) as u32;
    let idx = m[k].indexes.addr();
    o.call(m, &SI_QUICKSORT, &[idx, parallel, 0, last, has_keys as u32]);
    if has_keys {
        let frame = m.local(8);
        while o.call(m, &SI_SWAP_GET, &[frame, frame + 4]) != 0 {
            let (a, b) = (m.u32(frame), m.u32(frame + 4));
            o.call(m, &SI_CLUSTER_ANIM_SWAP_VERTICES, &[model.addr(), k.addr(), a, b]);
        }
        o.call(m, &SI_SWAP_DISPOSE, &[]);
        m.release(8);
    }
}

/// Renumbers a cluster's points after its node's points were renumbered
/// [soft 0x0843d1dc klusterMoveIndexes]: `map[old]` is a point's new number, negative for
/// a point that is gone; such a point leaves the keys (`SI_clusterAnimRemoveVertex`) and
/// the cluster ([`kluster_remove_index`]). Then [`kluster_sort_indexes`].
pub fn kluster_move_indexes(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, k: Ptr32<Kluster>, ud: u32, map: u32) {
    let mut i = 0i32;
    while i < m[k].count {
        let t = kluster_get_true_index(m, model, k, i);
        let v = m.i32(map.wrapping_add((t as u32).wrapping_mul(4)));
        if v >= 0 {
            if m[model].kind == 2 {
                m.set_i32(m[k].indexes.addr().wrapping_add((i as u32).wrapping_mul(8)).wrapping_add(4), v);
            } else {
                m.set_i32(index_at(m, k, i), v);
            }
        } else {
            o.call(m, &SI_CLUSTER_ANIM_REMOVE_VERTEX, &[model.addr(), k.addr(), i as u32]);
            kluster_remove_index(m, o, model, k, ud, i as i16);
            i -= 1;
        }
        i += 1;
    }
    kluster_sort_indexes(m, o, model, k, ud);
}

/// The same for one of a mesh's user data clusters and its array.
fn move_user_data(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, pair: u32, map: u32) {
    let k = m.u32(model.addr() + pair);
    if k != 0 && m.u32(model.addr() + pair + 4) != 0 {
        kluster_move_indexes(m, o, model, Ptr32::new(k), model.addr() + pair + 4, map);
    }
}

/// [`kluster_move_indexes`] for a mesh's per-polygon user data
/// [soft 0x0843d268 SI_klusterPolygonMoveIndexes].
pub fn kluster_polygon_move_indexes(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, map: u32) {
    if !model.is_null() && m[model].kind == 4 {
        move_user_data(m, o, model, POLYGON_UD, map);
    }
}

/// [`kluster_move_indexes`] for a mesh's per-edge user data
/// [soft 0x0843d2a4 SI_klusterEdgeMoveIndexes].
pub fn kluster_edge_move_indexes(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, map: u32) {
    if !model.is_null() && m[model].kind == 4 {
        move_user_data(m, o, model, EDGE_UD, map);
    }
}

/// [`kluster_move_indexes`] for every cluster of a node and for its per-vertex user data
/// [soft 0x0843d2e0 SI_klusterMoveIndexes].
pub fn kluster_move_all_indexes(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, map: u32) {
    let mut k = head(m, model);
    while !k.is_null() {
        kluster_move_indexes(m, o, model, k, 0, map);
        k = m[k].next;
    }
    move_user_data(m, o, model, VERTEX_UD, map);
}

/// Disposes every cluster without points [soft 0x0843d344 SI_klusterDisposeEmpty], and
/// each of the three user data clusters that has none, with its array (freed here, not
/// through [`kluster_dispose`]).
pub fn kluster_dispose_empty(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, full: i16) {
    let mut k = head(m, model);
    while !k.is_null() {
        if m[k].count <= 0 {
            let empty = k;
            k = m[k].next;
            kluster_dispose(m, o, model, empty, full);
        } else {
            k = m[k].next;
        }
    }
    for pair in [VERTEX_UD, POLYGON_UD, EDGE_UD] {
        let c: Ptr32<Kluster> = Ptr32::new(m.u32(model.addr() + pair));
        if c.is_null() || m[c].count > 0 {
            continue;
        }
        free_parts(m, c);
        m.free(c.addr());
        let arr = m.u32(model.addr() + pair + 4);
        m.set_u32(model.addr() + pair, 0);
        if arr != 0 {
            m.free(arr);
            m.set_u32(model.addr() + pair + 4, 0);
        }
    }
}

/// Clears the word at +8 of every cluster of a node that has cluster animation
/// [soft 0x0843d4fc SI_modelKlusterResetCluInd].
pub fn model_kluster_reset_clu_ind(m: &mut Mem, model: Ptr32<Model>) {
    if model.is_null() {
        return;
    }
    let anim = m[model].cluster.addr();
    if anim == 0 || m.u32(anim + 4) == 0 {
        return;
    }
    let mut k = head(m, model);
    while !k.is_null() {
        m[k].unk_08 = 0;
        k = m[k].next;
    }
}

/// Turns the clusters of a spline or curve into clusters of the face it became
/// [soft 0x0843d538 SI_modelklusterListCvtToFace]: each list of numbers becomes a list of
/// pairs (curve, number), the curve being -1 (the outline) for a face made of splines and
/// 0 for one made of NURBS curves; the array is grown with `SI_realloc` and filled from
/// the back. Shape keys follow for a NURBS face (`SI_clusterAnimCvtToFace`). -1 when there
/// is no memory, else 1 (also when the node has no current definition).
pub fn model_kluster_list_cvt_to_face(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> i32 {
    let def = current_def(m, model);
    if def == 0 {
        return 1;
    }
    let mut k = head(m, model);
    while !k.is_null() {
        let mut w = m[k].count.wrapping_add(m[k].count);
        let grown = m.si_realloc(m[k].indexes.addr(), (m[k].count as u32).wrapping_shl(3));
        m[k].indexes = Ptr32::new(grown);
        if grown == 0 {
            return -1;
        }
        w = w.wrapping_sub(1);
        let curve: i32 = if m.i16(def + geom::FACE_IS_SPLINE) != 0 { -1 } else { 0 };
        let mut c = m[k].count.wrapping_sub(1);
        while c >= 0 {
            let v = m.u32(index_at(m, k, c));
            m.set_u32(index_at(m, k, w), v);
            w = w.wrapping_sub(1);
            m.set_i32(index_at(m, k, w), curve);
            w = w.wrapping_sub(1);
            c -= 1;
        }
        if !m[k].anim.is_null() && m.i16(def + geom::FACE_IS_SPLINE) == 0 {
            o.call(m, &SI_CLUSTER_ANIM_CVT_TO_FACE, &[model.addr(), k.addr()]);
        }
        k = m[k].next;
    }
    1
}

/// Hands every cluster of a node to `SI_clusterAnimApplyTransfo` with a matrix
/// [soft 0x0843d608 SI_clustersApplyTransformation].
pub fn clusters_apply_transformation(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, matrix: u32) {
    let mut k = head(m, model);
    while !k.is_null() {
        o.call(m, &SI_CLUSTER_ANIM_APPLY_TRANSFO, &[model.addr(), matrix, k.addr()]);
        k = m[k].next;
    }
}

/// Moves every shape key of a chain of clusters by a vector
/// [soft 0x0843d644 SI_klustersAddVector]: in each key (a chain from the key data's word at
/// +0x18, linked through +4, its first word the points) three vectors per point of the
/// cluster get float(coordinate + v).
pub fn klusters_add_vector(m: &mut Mem, k: Ptr32<Kluster>, v: V3) {
    let mut k = k;
    while !k.is_null() {
        let anim = m[k].anim.addr();
        let mut key = if anim != 0 { m.u32(anim + 0x18) } else { 0 };
        while key != 0 {
            let mut p = m.u32(key);
            let mut i = 0i32;
            while i < m[k].count {
                for _ in 0..3 {
                    let cur = rd3(m, p);
                    for c in 0..3 {
                        m.set_f32(p + 4 * c as u32, (x(cur[c]) + v[c]).to_f32());
                    }
                    p = p.wrapping_add(0xc);
                }
                i += 1;
            }
            key = m.u32(key + 4);
        }
        k = m[k].next;
    }
}

/// In how many clusters of a node a point is [soft 0x0843d6ec SI_klusterCountVertexRef].
/// With `by_centre` a cluster counts as many times as it has centre relations (type 0xf7)
/// and not at all without one. The search of a cluster's ascending list stops at the first
/// number above the point.
pub fn kluster_count_vertex_ref(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, vertex: i32, by_centre: i16) -> i32 {
    let mut total = 0i32;
    let mut weight = 1i32;
    let frame = m.local(4);
    let mut k = head(m, model);
    while !k.is_null() {
        if by_centre != 0 {
            let e = o.call(m, &SI_ED_GET_ELEM_FROM_CLUSTER, &[k.addr(), frame]);
            let chap = m[SI_CID][CID_MODELS] as u32;
            weight = o.call(m, &SI_ELEMDICT_RANGE_RELATIONS_COUNT, &[0, e, 0, 0, 0xf7, 0xf7, chap]) as i32;
        }
        let next = m[k].next;
        if weight > 0 || by_centre == 0 {
            let n = m[k].count;
            let mut j = 0i32;
            while j < n {
                let v = m.i32(index_at(m, k, j));
                if v == vertex {
                    total = total.wrapping_add(weight);
                    break;
                }
                if v > vertex {
                    break;
                }
                j += 1;
            }
        }
        k = next;
    }
    m.release(4);
    total
}

/// Whether two clusters have the same name and the same points
/// [soft 0x0843d7a0 klsEqual] (`kind` 2: pairs).
pub fn kls_equal(m: &Mem, a: Ptr32<Kluster>, b: Ptr32<Kluster>, kind: i32) -> i32 {
    if a.is_null() || b.is_null() || m[a].count != m[b].count {
        return 0;
    }
    let (na, nb) = (m[a].name.addr(), m[b].name.addr());
    if na == 0 {
        if nb != 0 {
            return 0;
        }
    } else if nb == 0 || clib::strcmp(m, na, nb) != 0 {
        return 0;
    }
    let words = if kind == 2 { m[a].count.wrapping_add(m[a].count) } else { m[a].count };
    let mut i = 0i32;
    while i < words {
        if m.u32(index_at(m, a, i)) != m.u32(index_at(m, b, i)) {
            return 0;
        }
        i += 1;
    }
    1
}

/// The first cluster of a chain equal to a given one [soft 0x0843d828 SI_findCluster].
pub fn find_cluster(m: &Mem, k: Ptr32<Kluster>, list: Ptr32<Kluster>, model: Ptr32<Model>) -> Ptr32<Kluster> {
    if k.is_null() || list.is_null() {
        return Ptr32::NULL;
    }
    let mut c = list;
    while !c.is_null() {
        if kls_equal(m, k, c, m[model].kind) as i16 != 0 {
            return c;
        }
        c = m[c].next;
    }
    Ptr32::NULL
}

/// How many clusters a node has [soft 0x0843d8f0 SI_clusterCount] (`klusters_b.c`).
pub fn cluster_count(m: &Mem, model: Ptr32<Model>) -> i32 {
    let mut n = 0i32;
    let mut k = head(m, model);
    while !k.is_null() {
        k = m[k].next;
        n += 1;
    }
    n
}
