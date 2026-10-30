// SPDX-License-Identifier: MIT
//! `cluster.c`: cluster animation. A cluster of points (`kluster`, finding 0028) may carry
//! shape keys of its own (`SI_ClusterData` and a chain of `clusterKey`s); at every frame the
//! scene update asks the interpolators here where each point of the working geometry goes
//! (finding 0030, section 4). Ported: the records, the activity switches, the key look-ups,
//! the cardinal weights, the total weight, the seven interpolators (mesh and patch in the
//! first pass; NURBS curve and surface, spline, face of splines and face of NURBS curves in
//! the second), and (second pass) the key commands, the key and weight curves, copying a key
//! into the geometry, the vertex bookkeeping and the two loaders without callers, and (third
//! pass) adding a point to every key and the copy for a duplicated model. Not ported:
//! `si_clusterAddKey (0x081e859c)` (the menu command, with its request). Fourth pass:
//! `clusterAnimDump (0x081f1524)` (printing, no caller; printed text goes through `Outside::print`).

use si_mem::Mem;
use si_x87::{x, F80};

/// The weights of the cardinal (Catmull-Rom, tension ½) blend at `t` as
/// `si_getCardinalCoeff` computes them: t², t³ and the four weights, each rounded to float
/// where the original stores it. With T² = t·t and T³ = T²·t in the register and t2, t3
/// their stored floats:
/// c0 = (−½·T³ + t2) − ½t, c1 = (1.5·t3 − 2.5·t2) + 1, c2 = ½t + (−1.5·t3 + 2·t2),
/// c3 = ½·t3 − ½·t2.
pub fn cardinal_coeff(t: f32) -> [f32; 6] {
    let t = x(t);
    let tt = t * t;
    let t2 = tt.to_f32();
    let ttt = tt * t;
    let t3 = ttt.to_f32();
    let ht = t * 0.5f32;
    let c0 = ((ttt * -0.5f32 + t2) - ht).to_f32();
    let c1 = ((x(t3) * 1.5f32 - x(t2) * 2.5f32) + F80::ONE).to_f32();
    let c2 = (ht + (x(t3) * -1.5f32 + (x(t2) + x(t2)))).to_f32();
    let c3 = (x(t3) * 0.5f32 - F80::from(0.5f32) * t2).to_f32();
    [t2, t3, c0, c1, c2, c3]
}

/// [soft 0x081e9e0c si_getCardinalCoeff]
///
/// `si_getCardinalCoeff(t, &t2, &t3, &c0, &c1, &c2, &c3)`: [`cardinal_coeff`], stored in
/// the original's order, the squares read back from where they were stored.
#[allow(clippy::too_many_arguments)]
pub fn get_cardinal_coeff(m: &mut Mem, t: f32, p_t2: u32, p_t3: u32, p_c0: u32, p_c1: u32, p_c2: u32, p_c3: u32) {
    let t = x(t);
    let tt = t * t;
    m.set_f32(p_t2, tt.to_f32());
    let ttt = tt * t;
    m.set_f32(p_t3, ttt.to_f32());
    let ht = t * 0.5f32;
    let c0 = (ttt * -0.5f32 + m.f32(p_t2)) - ht;
    m.set_f32(p_c0, c0.to_f32());
    let c1 = (x(m.f32(p_t3)) * 1.5f32 - x(m.f32(p_t2)) * 2.5f32) + F80::ONE;
    m.set_f32(p_c1, c1.to_f32());
    let b = x(m.f32(p_t3)) * -1.5f32;
    let t2 = x(m.f32(p_t2));
    let c2 = ht + (b + (t2 + t2));
    m.set_f32(p_c2, c2.to_f32());
    let c3 = x(m.f32(p_t3)) * 0.5f32 - F80::from(0.5f32) * m.f32(p_t2);
    m.set_f32(p_c3, c3.to_f32());
}

use crate::outside::{self as out, Outside};
use crate::records::{Cluster, ClusterData, ClusterKey, Kluster};
use si_mem::Ptr32;
use si_types::model::Model;

/// Offsets of the model node used here beyond `si_types::model::Model`'s typed fields.
const MODEL_FLAGS_0A: u32 = 0x0a;
const MODEL_FLAGS_3FA: u32 = 0x3fa;

/// [soft 0x081e7e40 SI_clusterAllocate]
pub fn cluster_allocate(m: &mut Mem) -> Ptr32<Cluster> {
    let p: Ptr32<Cluster> = Ptr32::new(m.si_malloc(0xc));
    if p.is_null() {
        return Ptr32::NULL;
    }
    m[p].block = Ptr32::NULL;
    m[p].active = 0;
    m[p].unk_08 = 0;
    p
}

/// [soft 0x081e7e78 SI_clusterDispose]
///
/// Frees the record and the block it holds and clears the pointer: 1, or 0 for no record.
pub fn cluster_dispose(m: &mut Mem, slot: Ptr32<Ptr32<Cluster>>) -> i32 {
    if slot.is_null() || m[slot].is_null() {
        return 0;
    }
    let c = m[slot];
    let b = m[c].block;
    if !b.is_null() {
        m.free(b.addr());
        let c = m[slot];
        m[c].block = Ptr32::NULL;
    }
    let c = m[slot];
    if !c.is_null() {
        m.free(c.addr());
        m[slot] = Ptr32::NULL;
    }
    1
}

/// [soft 0x081e7ecc SI_clusterDataAllocate]
pub fn cluster_data_allocate(m: &mut Mem) -> Ptr32<ClusterData> {
    let p: Ptr32<ClusterData> = Ptr32::new(m.si_malloc(0x1c));
    if p.is_null() {
        return Ptr32::NULL;
    }
    m[p].keys = Ptr32::NULL;
    m[p].current = Ptr32::NULL;
    m[p].nb_keys = 0;
    m[p].interp = 0;
    m[p].active = 1;
    p
}

/// [soft 0x081e7f0c SI_clusterDataDispose]
///
/// Frees every key of the chain (taking each off the front first), then the record, and
/// clears the pointer: 1, or 0 for no record.
pub fn cluster_data_dispose(m: &mut Mem, slot: Ptr32<Ptr32<ClusterData>>) -> i32 {
    if slot.is_null() || m[slot].is_null() {
        return 0;
    }
    let local: Ptr32<Ptr32<ClusterKey>> = Ptr32::new(m.local(4));
    loop {
        let d = m[slot];
        if m[d].keys.is_null() {
            break;
        }
        let k = m[d].keys;
        m[local] = k;
        let next = m[k].next;
        m[d].keys = next;
        cluster_key_dispose(m, local);
    }
    m.release(4);
    let d = m[slot];
    if !d.is_null() {
        m.free(d.addr());
        m[slot] = Ptr32::NULL;
    }
    1
}

/// [soft 0x081e7f78 SI_clusterKeyAllocate]
///
/// A key for `n` points of a model of `kind`: the record and room for the positions (12
/// bytes a point for a mesh, 16 for a patch or NURBS curve or surface, 36 for a face or
/// spline). 0 for no point, and for a kind without positions the room is not asked for:
/// the record is then kept if its first word happens not to be 0 (it is whatever
/// `SI_malloc` left there) and freed otherwise.
pub fn cluster_key_allocate(m: &mut Mem, n: i32, kind: i32) -> Ptr32<ClusterKey> {
    if n <= 0 {
        return Ptr32::NULL;
    }
    let k: Ptr32<ClusterKey> = Ptr32::new(m.si_malloc(8));
    if k.is_null() {
        return Ptr32::NULL;
    }
    let size = match kind {
        4 => Some((n as u32).wrapping_mul(12)),
        2 | 6 => Some((n as u32).wrapping_mul(36)),
        1 | 9 | 10 => Some((n as u32) << 4),
        _ => None,
    };
    if let Some(size) = size {
        let p = m.si_malloc(size);
        m[k].points = Ptr32::new(p);
    }
    if m[k].points.is_null() {
        m.free(k.addr());
        return Ptr32::NULL;
    }
    m[k].next = Ptr32::NULL;
    k
}

/// [soft 0x081e801c clusterKeyDispose]
pub fn cluster_key_dispose(m: &mut Mem, slot: Ptr32<Ptr32<ClusterKey>>) -> i32 {
    if slot.is_null() || m[slot].is_null() {
        return 0;
    }
    let k = m[slot];
    let p = m[k].points;
    if !p.is_null() {
        m.free(p.addr());
        let k = m[slot];
        m[k].points = Ptr32::NULL;
    }
    let k = m[slot];
    if !k.is_null() {
        m.free(k.addr());
        m[slot] = Ptr32::NULL;
    }
    1
}

/// [soft 0x081e8070 clusterKeyNumber]
///
/// The place of the active cluster's current key in its chain (the length of the chain if
/// it is not in it); −1 without an active cluster, keys or a current key.
pub fn cluster_key_number(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> i32 {
    if model.is_null() || m[model].klusters.is_null() {
        return -1;
    }
    let k: Ptr32<Kluster> = Ptr32::new(o.call(m, &out::SI_KLUSTER_GET_ACTIVE, &[model.addr()]));
    if k.is_null() {
        return -1;
    }
    let d = m[k].anim;
    if d.is_null() {
        return -1;
    }
    let mut e = m[d].keys;
    if e.is_null() {
        return -1;
    }
    let cur = m[d].current;
    if cur.is_null() {
        return -1;
    }
    let mut n = 0;
    while e != cur {
        e = m[e].next;
        n += 1;
        if e.is_null() {
            break;
        }
    }
    n
}

/// [soft 0x081e80c8 SI_modelActiveCluster]
///
/// Turns the model's cluster animation on or off (its word at +4 between 0 and 1): 1, or 0
/// without one.
pub fn model_active_cluster(m: &mut Mem, model: Ptr32<Model>) -> i32 {
    if model.is_null() {
        return 0;
    }
    let c: Ptr32<Cluster> = Ptr32::new(m[model].cluster.addr());
    if c.is_null() {
        return 0;
    }
    m[c].active = (m[c].active == 0) as i32;
    1
}

/// [soft 0x081e80f8 SI_clusterActive]
///
/// Turns the active cluster's keys on or off: 1, or 0 without an active cluster with keys.
pub fn cluster_active(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> i32 {
    if model.is_null() || m[model].klusters.is_null() {
        return 0;
    }
    let k: Ptr32<Kluster> = Ptr32::new(o.call(m, &out::SI_KLUSTER_GET_ACTIVE, &[model.addr()]));
    if k.is_null() {
        return 0;
    }
    let d = m[k].anim;
    if d.is_null() {
        return 0;
    }
    m[d].active = (m[d].active == 0) as i32;
    1
}

/// [soft 0x081e813c SI_clusterAnimDispose]
///
/// Takes a cluster's animation away: the curves of the related range of elements (relation
/// types 0x2bd–0x2be, each one's curve deleted), the cluster element's curves of the two
/// types, and the keys. 1 when related elements were found.
pub fn cluster_anim_dispose(m: &mut Mem, o: &mut dyn Outside, k: Ptr32<Kluster>) -> i32 {
    let mut found: i16 = 0;
    let index = m.local(4);
    let e = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[k.addr(), index]);
    if e != 0 {
        let (chap, idx) = (m.local(4), m.local(4));
        let cid0 = m[crate::globals::SI_CID][0] as u32;
        let dict = crate::globals::cur_dict(m);
        let mut r = o.call(m, &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM, &[dict, e, 1, 2, 0x2bd, 0x2be, cid0, idx, chap]);
        if r != 0 {
            found = 1;
            loop {
                o.call(m, &out::SI_FCV_SCNGL_DELETE, &[m.u32(r.wrapping_add(4))]);
                let cid0 = m[crate::globals::SI_CID][0] as u32;
                let dict = crate::globals::cur_dict(m);
                r = o.call(m, &out::SI_ELEMDICT_FIND_RANGE_NEXT_RELATED_ELEM, &[dict, e, 1, 2, 0x2bd, 0x2be, cid0, idx, chap]);
                if r == 0 {
                    break;
                }
            }
        }
        m.release(8);
    }
    m.release(4);
    let cid2 = m[crate::globals::SI_CID][2] as u32;
    crate::edanim::ed_fcv_dispose(m, o, k.addr(), cid2, 2, 0x2bd, 0x2be);
    if !m[k].anim.is_null() {
        cluster_data_dispose(m, si_mem::field!(k, Kluster, anim));
    }
    m[k].anim = Ptr32::NULL;
    found as i32
}

/// [soft 0x081e8248 si_modelDeleteAllClusterAnim]
///
/// The animation of every cluster of the model taken away, then its cluster animation
/// record; the node marked as changed (bit 4 of the bytes at +0xa and +0x3fa). 1 when any
/// cluster had related curves.
pub fn model_delete_all_cluster_anim(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> i32 {
    let mut any: i16 = 0;
    if model.is_null() || m[model].klusters.is_null() {
        return 0;
    }
    let mut k: Ptr32<Kluster> = Ptr32::new(m[model].klusters.addr());
    loop {
        if cluster_anim_dispose(m, o, k) as u16 != 0 {
            any = 1;
        }
        k = m[k].next;
        if k.is_null() {
            break;
        }
    }
    let slot: Ptr32<Ptr32<Cluster>> = Ptr32::new(model.addr() + 0x344);
    if !m[slot].is_null() {
        cluster_dispose(m, slot);
    }
    m[slot] = Ptr32::NULL;
    m.or_u8(model.addr() + MODEL_FLAGS_0A, 0x10);
    m.or_u8(model.addr() + MODEL_FLAGS_3FA, 0x10);
    any as i32
}

/// [soft 0x081e82c8 SI_modelDeleteClusterAnim]
///
/// One cluster's animation taken away; when no cluster of the model has keys left, the
/// model's cluster animation record too. The node is marked as changed and the curve
/// display refreshed. 1, or 0 when the cluster had no keys.
pub fn model_delete_cluster_anim(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, k: Ptr32<Kluster>) -> i32 {
    if model.is_null() || k.is_null() || m[k].anim.is_null() {
        return 0;
    }
    cluster_anim_dispose(m, o, k);
    let mut c: Ptr32<Kluster> = Ptr32::new(m[model].klusters.addr());
    while !c.is_null() && m[c].anim.is_null() {
        c = m[c].next;
    }
    let slot: Ptr32<Ptr32<Cluster>> = Ptr32::new(model.addr() + 0x344);
    if c.is_null() && !m[slot].is_null() {
        cluster_dispose(m, slot);
        m[slot] = Ptr32::NULL;
    }
    m.or_u8(model.addr() + MODEL_FLAGS_0A, 0x10);
    m.or_u8(model.addr() + MODEL_FLAGS_3FA, 0x10);
    o.call(m, &out::SI_REFRESH_FCURVE, &[1]);
    1
}

/// [soft 0x081e96f0 getClusterKeyPointer]
///
/// The key at place `n` of the chain (the first for `n` ≤ 0; 0 past the end).
pub fn get_cluster_key_pointer(m: &Mem, d: Ptr32<ClusterData>, n: i32) -> Ptr32<ClusterKey> {
    if d.is_null() {
        return Ptr32::NULL;
    }
    let mut k = m[d].keys;
    if k.is_null() {
        return Ptr32::NULL;
    }
    let mut n = n;
    if n <= 0 {
        return k;
    }
    loop {
        k = m[k].next;
        n -= 1;
        if n <= 0 || k.is_null() {
            return k;
        }
    }
}

/// [soft 0x081e9d68 getTotalWeight]
///
/// The sum of the values at `time` of the weight curves (type 0x2be, chapter 2) of a
/// cluster's element, one per key, each at most 1e-5 counted as 0; the key's number is
/// written through `info` before each value is asked. 0 without keys or curves.
#[allow(clippy::too_many_arguments)]
pub fn get_total_weight(m: &mut Mem, o: &mut dyn Outside, d: Ptr32<ClusterData>, elem: u32, time: f32, count: i32, list: u32, info: u32) -> F80 {
    let mut sum: f32 = 0.0;
    let mut fcv = o.call(m, &out::SI_ED_FIND_FCV, &[elem, 2, 0x2be]);
    if d.is_null() || fcv == 0 {
        return F80::ZERO;
    }
    let mut i: i32 = 0;
    while i < m[d].nb_keys {
        m.set_i32(info, i);
        let mut v = crate::shape::fcrv_val_with_expr(m, o, fcv, time, 0, count, list, info);
        if v.le(x(crate::shape::MIN_WEIGHT)) {
            v = F80::ZERO;
        }
        sum = (v + sum).to_f32();
        fcv = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[elem, 2, 0x2be]);
        i += 1;
    }
    x(sum)
}

// ---------------------------------------------------------------- moving the points

use crate::shape::{virt, MIN_WEIGHT};
use si_types::geom::Mesh;
use si_x87::Round;

/// The cluster's key positions at a point of the cluster: `n` floats from `key + j × size`.
fn kpos(m: &Mem, key: Ptr32<ClusterKey>, j: i32, size: u32) -> u32 {
    m[key].points.addr().wrapping_add((j as u32).wrapping_mul(size))
}

/// [soft 0x081e9e90 si_clusterMeshInterpolate]
///
/// `si_clusterMeshInterpolate(org, point, clusters, mode, time, which, &out, update node)`:
/// where the clusters' animation puts vertex `point` of a mesh at `time`. `out` is cleared,
/// then every cluster of the chain is asked in turn; one whose keys are on and whose next
/// point (from its cursor on) is `point` contributes and moves its cursor past it:
///
/// - linear and cardinal clusters: the value v of the cluster's key curve (type 0x2bd)
///   gives keys n = trunc(v) and n + 1 (each kept in 0 … keys − 1) and the fraction
///   v − n; the position is K1 + (K2 − K1)·f, or the cardinal blend of the four keys round
///   them (`si_getCardinalCoeff`; a missing neighbour made up as for shapes);
/// - weighted clusters: Σ v_k·K_k over the keys whose weight curve (type 0x2be) reads above
///   1e-5, each weight divided by their total (`getTotalWeight`); a total below 1e-5 ends
///   the routine at once with 0, whatever earlier clusters gave.
///
/// In mode 0 the contributions are summed in `out` and averaged at the end when there were
/// several; in mode 1 their differences from the vertex's original position are summed and
/// `out` becomes the original position plus that sum. 1 when a cluster contributed.
#[allow(clippy::too_many_arguments)]
pub fn cluster_mesh_interpolate(m: &mut Mem, o: &mut dyn Outside, org: Ptr32<Mesh>, point: i32, clusters: Ptr32<Kluster>, mode: i16, time: f32, which: i32, outp: u32, upd: u32) -> i32 {
    let sp = m.sp();
    let r = cluster_interpolate(m, o, &MESH, org.addr(), point, clusters, mode, time, which, outp, upd);
    m.set_sp(sp);
    r
}

/// [soft 0x081ea820 si_clusterPatchInterpolate]
///
/// The same for a control point of a patch (positions of 16 bytes), with three differences
/// read from its code: a cluster is asked whatever its cursor, the key curve is read for a
/// weighted cluster too (and not used), and a weighted cluster's cursor is not moved.
#[allow(clippy::too_many_arguments)]
pub fn cluster_patch_interpolate(m: &mut Mem, o: &mut dyn Outside, org: Ptr32<si_types::geom::Patch>, point: i32, clusters: Ptr32<Kluster>, mode: i16, time: f32, which: i32, outp: u32, upd: u32) -> i32 {
    let sp = m.sp();
    let r = cluster_interpolate(m, o, &PATCH, org.addr(), point, clusters, mode, time, which, outp, upd);
    m.set_sp(sp);
    r
}

/// What tells the cluster interpolators apart.
struct CKind {
    /// Bytes of one position in a key.
    size: u32,
    /// The original position of point `i`: its address in the original definition.
    base: fn(&Mem, u32, i32) -> u32,
    /// Whether a cluster whose cursor has reached its count is passed over.
    cursor_test: bool,
    /// Whether the key curve is read for a weighted cluster too.
    prelude_always: bool,
    /// Whether a weighted cluster's cursor moves past the point.
    weighted_cursor: bool,
    statics: (u32, u32, u32),
}

const MESH: CKind = CKind {
    size: 12,
    base: |m, org, i| m[Ptr32::<Mesh>::new(org)].vertices.addr().wrapping_add((i as u32).wrapping_mul(0x14)),
    cursor_test: true,
    prelude_always: false,
    weighted_cursor: true,
    statics: (crate::globals::CMESH_NBWCLS.0, crate::globals::CMESH_WFCV.0, crate::globals::CMESH_WFCV_SUBID.0),
};

const PATCH: CKind = CKind {
    size: 16,
    base: |m, org, i| m[Ptr32::<si_types::geom::Patch>::new(org)].points.addr().wrapping_add((i as u32) << 4),
    cursor_test: false,
    prelude_always: true,
    weighted_cursor: false,
    statics: (crate::globals::CPATCH_NBWCLS.0, crate::globals::CPATCH_WFCV.0, crate::globals::CPATCH_WFCV_SUBID.0),
};

#[allow(clippy::too_many_arguments)]
fn cluster_interpolate(m: &mut Mem, o: &mut dyn Outside, kd: &CKind, org: u32, point: i32, clusters: Ptr32<Kluster>, mode: i16, time: f32, which: i32, outp: u32, upd: u32) -> i32 {
    let size = kd.size;
    let (cnt_g, buf_g, list_g) = kd.statics;
    let mut ret: i16 = 0;
    let mut matched: i32 = 0;
    for c in [8, 4, 0] {
        m.set_u32(outp.wrapping_add(c), 0);
    }
    let mut base = [0f32; 3];
    let mut acc = [0f32; 3];
    if mode == 1 {
        if org == 0 {
            return 0;
        }
        let v = (kd.base)(m, org, point);
        base = [m.f32(v), m.f32(v.wrapping_add(4)), m.f32(v.wrapping_add(8))];
    }
    let dict = crate::globals::cur_dict(m);
    let e = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[dict, m.i16(upd) as i32 as u32, m.u32(upd.wrapping_add(4))]);
    let l64 = m.local(4);
    let info = m.local(8);
    // what the original keeps between the clusters in locals
    let (mut frac, mut ks, mut first_real, mut last_real) = (0f32, [Ptr32::<ClusterKey>::NULL; 4], false, false);
    let mut k = clusters;
    while !k.is_null() {
        let ce = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[k.addr(), l64]);
        let fcv = o.call(m, &out::SI_ED_FIND_FCV, &[ce, 2, 0x2bd]);
        let d = m[k].anim;
        let go = (!kd.cursor_test || m[k].cursor < m[k].count) && !d.is_null() && m[d].active == 1 && fcv != 0;
        if go {
            m.set_u32(info + 4, m.u32(l64));
            if kd.prelude_always || m[d].interp != 2 {
                let nk1 = m[d].nb_keys.wrapping_sub(1);
                let w = m.u32(buf_g);
                let r = o.call(m, &out::SI_GET_EXPR_FCURVE_USED, &[e, cnt_g, w, 0, list_g, 0xb7]);
                m.set_u32(buf_g, r);
                let (cnt, list) = (m.i32(cnt_g), m.u32(list_g));
                let v = crate::shape::fcrv_val_with_expr(m, o, fcv, time, which as i16, cnt, list, info);
                let n = v.to_i32(Round::Zero);
                let clamp = |i: i32| if i < 0 { 0 } else if i > nk1 { nk1 } else { i };
                let i1 = clamp(n);
                let i2 = clamp(n.wrapping_add(1));
                frac = (v - F80::from(n)).to_f32();
                let i0 = if i1 > 0 { i1 - 1 } else { i1 };
                let i3 = if i2 < nk1 { i2 + 1 } else { i2 };
                ks = [get_cluster_key_pointer(m, d, i0), get_cluster_key_pointer(m, d, i1), get_cluster_key_pointer(m, d, i2), get_cluster_key_pointer(m, d, i3)];
                first_real = ks[1] != ks[0];
                last_real = ks[3] != ks[2];
            }
            let idx = m[k].indexes.addr();
            match m[d].interp {
                0 => {
                    let mut j = m[k].cursor;
                    let count = m[k].count;
                    while j < count {
                        if m.i32(idx.wrapping_add(4 * j as u32)) == point {
                            let (p1, p2) = (kpos(m, ks[1], j, size), kpos(m, ks[2], j, size));
                            let p: [F80; 3] = core::array::from_fn(|c| {
                                let a = x(m.f32(p1 + 4 * c as u32));
                                a + (x(m.f32(p2 + 4 * c as u32)) - a) * frac
                            });
                            contribute(m, mode, outp, &base, &mut acc, p);
                            matched += 1;
                            ret = 1;
                            m[k].cursor = j + 1;
                            break;
                        }
                        j += 1;
                    }
                }
                1 => {
                    let w = cardinal_coeff(frac);
                    let cc = [w[2], w[3], w[4], w[5]];
                    let mut j = m[k].cursor;
                    while j < m[k].count {
                        if m.i32(idx.wrapping_add(4 * j as u32)) == point {
                            let q: [u32; 4] = core::array::from_fn(|i| kpos(m, ks[i], j, size));
                            let g = |m: &Mem, i: usize, c: usize| m.f32(q[i] + 4 * c as u32);
                            let f: [f32; 3] = core::array::from_fn(|c| if first_real { g(m, 0, c) } else { virt(g(m, 1, c), g(m, 2, c), g(m, 3, c)) });
                            let l: [f32; 3] = core::array::from_fn(|c| if last_real { g(m, 3, c) } else { virt(g(m, 2, c), g(m, 1, c), g(m, 0, c)) });
                            let p: [F80; 3] = core::array::from_fn(|c| card_ext(cc, [f[c], g(m, 1, c), g(m, 2, c), l[c]]));
                            contribute(m, mode, outp, &base, &mut acc, p);
                            matched += 1;
                            ret = 1;
                            m[k].cursor = j + 1;
                            break;
                        }
                        j += 1;
                    }
                }
                2 => {
                    let w = m.u32(buf_g);
                    let r = o.call(m, &out::SI_GET_EXPR_FCURVE_USED, &[e, cnt_g, w, 0, list_g, 0xb8]);
                    m.set_u32(buf_g, r);
                    let (cnt, list) = (m.i32(cnt_g), m.u32(list_g));
                    let total = get_total_weight(m, o, d, ce, time, cnt, list, info).to_f32();
                    if x(total).lt(x(MIN_WEIGHT)) {
                        return 0;
                    }
                    let mut j = m[k].cursor;
                    let count = m[k].count;
                    while j < count {
                        if m.i32(idx.wrapping_add(4 * j as u32)) == point {
                            let mut dsum = [0f32; 3];
                            let mut fcv2 = o.call(m, &out::SI_ED_FIND_FCV, &[ce, 2, 0x2be]);
                            let mut kk: i32 = 0;
                            while kk < m[d].nb_keys {
                                let kp = get_cluster_key_pointer(m, d, kk);
                                if !kp.is_null() && !m[kp].points.is_null() {
                                    m.set_i32(info, kk);
                                    let (cnt, list) = (m.i32(cnt_g), m.u32(list_g));
                                    let v = crate::shape::fcrv_val_with_expr(m, o, fcv2, time, 0, cnt, list, info);
                                    if v.gt(x(MIN_WEIGHT)) {
                                        let v = v / total;
                                        let at = kpos(m, kp, j, size);
                                        let qv: [F80; 3] = core::array::from_fn(|c| v * m.f32(at + 4 * c as u32));
                                        match mode {
                                            0 => {
                                                for (c, q) in qv.iter().enumerate() {
                                                    let a = outp.wrapping_add(4 * c as u32);
                                                    let s = (*q + m.f32(a)).to_f32();
                                                    m.set_f32(a, s);
                                                }
                                            }
                                            1 => {
                                                for (c, q) in qv.iter().enumerate() {
                                                    dsum[c] = (*q + dsum[c]).to_f32();
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                fcv2 = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[ce, 2, 0x2be]);
                                kk += 1;
                            }
                            if mode == 1 {
                                for c in 0..3 {
                                    acc[c] = ((x(dsum[c]) - base[c]) + acc[c]).to_f32();
                                }
                            }
                            matched += 1;
                            ret = 1;
                            if kd.weighted_cursor {
                                m[k].cursor = j + 1;
                            }
                            break;
                        }
                        j += 1;
                    }
                }
                _ => {}
            }
        }
        k = m[k].next;
    }
    if mode == 0 && matched > 1 {
        let n = F80::from(matched);
        for c in 0..3u32 {
            let a = outp.wrapping_add(4 * c);
            let s = (x(m.f32(a)) / n).to_f32();
            m.set_f32(a, s);
        }
    } else if mode == 1 {
        for c in 0..3usize {
            m.set_f32(outp.wrapping_add(4 * c as u32), (x(base[c]) + acc[c]).to_f32());
        }
    }
    ret as i32
}

// ---------------------------------------------------------------- NURBS curves and surfaces

/// The float 1e-5 the NURBS routines put in place of a smaller weight (the constant they
/// compare with is the double 1e-5, [`MIN_WEIGHT`]).
const F_MIN_WEIGHT: f32 = 1e-5;

/// A homogeneous point of four floats in Euclidean form as the cluster interpolators make
/// it: x, y and z divided by w unless w is 0 (each quotient rounded to float; a NaN weight
/// divides), then a weight below 1e-5 raised to the float 1e-5.
fn euclid4(p: [f32; 4]) -> [f32; 4] {
    let mut v = p;
    let w = x(p[3]);
    if !w.eq(F80::ZERO) {
        for c in 0..3 {
            v[c] = (x(p[c]) / w).to_f32();
        }
    }
    if x(v[3]).lt(x(MIN_WEIGHT)) {
        v[3] = F_MIN_WEIGHT;
    }
    v
}

fn read4(m: &Mem, a: u32) -> [f32; 4] {
    core::array::from_fn(|c| m.f32(a.wrapping_add(4 * c as u32)))
}

/// What tells the two NURBS interpolators apart: where the original definition keeps its
/// points (+0x2c of a curve, +0x54 of a surface) and the static variables.
struct NKind {
    points: u32,
    statics: (u32, u32, u32),
}

const NCRV: NKind = NKind {
    points: 0x2c,
    statics: (crate::globals::CNCRV_NBWCLS.0, crate::globals::CNCRV_WFCV.0, crate::globals::CNCRV_WFCV_SUBID.0),
};

const NSRF: NKind = NKind {
    points: 0x54,
    statics: (crate::globals::CNSRF_NBWCLS.0, crate::globals::CNSRF_WFCV.0, crate::globals::CNSRF_WFCV_SUBID.0),
};

/// [soft 0x081eebac si_clusterNurbsCrvInterpolate]
///
/// `si_clusterNurbsCrvInterpolate(org, point, clusters, mode, time, which, &out, update
/// node)`: where the clusters' animation puts control point `point` of a NURBS curve (four
/// floats: x, y, z, w). As the mesh routine, with these differences read from its code:
///
/// - every key position is taken in Euclidean form ([`euclid4`]) before it is blended, and
///   so is the original point in mode 1 (its doubles rounded to floats first);
/// - the key curve is read for a weighted cluster too, and a weighted cluster's cursor is
///   not moved (as the patch routine);
/// - in mode 1 the weight is not taken relative to the original: the clusters' weights are
///   summed onto the original weight;
/// - at the end (unless a weighted cluster's total weight ended the routine) a weight of
///   `out` below 1e-5 is raised to the float 1e-5.
#[allow(clippy::too_many_arguments)]
pub fn cluster_nurbs_crv_interpolate(m: &mut Mem, o: &mut dyn Outside, org: Ptr32<si_types::geom::NurbsCrv>, point: i32, clusters: Ptr32<Kluster>, mode: i16, time: f32, which: i32, outp: u32, upd: u32) -> i32 {
    let sp = m.sp();
    let r = cluster_nurbs(m, o, &NCRV, org.addr(), point, clusters, mode, time, which, outp, upd);
    m.set_sp(sp);
    r
}

/// [soft 0x081efad4 si_clusterNurbsSrfInterpolate]
///
/// The same for a control point of a NURBS surface (the original's points at +0x54); its
/// code is the curve routine's but for that offset, its constants and its static variables.
#[allow(clippy::too_many_arguments)]
pub fn cluster_nurbs_srf_interpolate(m: &mut Mem, o: &mut dyn Outside, org: Ptr32<si_types::geom::NurbsSrf>, point: i32, clusters: Ptr32<Kluster>, mode: i16, time: f32, which: i32, outp: u32, upd: u32) -> i32 {
    let sp = m.sp();
    let r = cluster_nurbs(m, o, &NSRF, org.addr(), point, clusters, mode, time, which, outp, upd);
    m.set_sp(sp);
    r
}

/// What the key curve gives a linear or cardinal cluster: the four keys round its value
/// (the first and last repeated at the ends), whether the outer two are real, and the
/// fraction. `nk1` is the number of keys less one.
struct Around {
    ks: [Ptr32<ClusterKey>; 4],
    first_real: bool,
    last_real: bool,
    frac: f32,
}

fn around(m: &Mem, d: Ptr32<ClusterData>, v: F80) -> Around {
    let nk1 = m[d].nb_keys.wrapping_sub(1);
    let n = v.to_i32(Round::Zero);
    let clamp = |i: i32| if i < 0 { 0 } else if i > nk1 { nk1 } else { i };
    let i1 = clamp(n);
    let i2 = clamp(n.wrapping_add(1));
    let frac = (v - F80::from(n)).to_f32();
    let i0 = if i1 > 0 { i1 - 1 } else { i1 };
    let i3 = if i2 < nk1 { i2 + 1 } else { i2 };
    let ks = [get_cluster_key_pointer(m, d, i0), get_cluster_key_pointer(m, d, i1), get_cluster_key_pointer(m, d, i2), get_cluster_key_pointer(m, d, i3)];
    Around { first_real: ks[1] != ks[0], last_real: ks[3] != ks[2], ks, frac }
}

/// The key curve's value for a cluster, as every interpolator asks it: the curves
/// expressions drive (`SI_getExprFcurveUsed`, kind 0xb7, its buffer kept in the routine's
/// static), then the curve's value at the time.
#[allow(clippy::too_many_arguments)]
fn key_value(m: &mut Mem, o: &mut dyn Outside, statics: (u32, u32, u32), e: u32, fcv: u32, time: f32, which: i32, info: u32) -> F80 {
    let (cnt_g, buf_g, list_g) = statics;
    let w = m.u32(buf_g);
    let r = o.call(m, &out::SI_GET_EXPR_FCURVE_USED, &[e, cnt_g, w, 0, list_g, 0xb7]);
    m.set_u32(buf_g, r);
    let (cnt, list) = (m.i32(cnt_g), m.u32(list_g));
    crate::shape::fcrv_val_with_expr(m, o, fcv, time, which as i16, cnt, list, info)
}

/// The total weight of a weighted cluster (`SI_getExprFcurveUsed` with kind 0xb8 first).
#[allow(clippy::too_many_arguments)]
fn total_weight(m: &mut Mem, o: &mut dyn Outside, statics: (u32, u32, u32), e: u32, d: Ptr32<ClusterData>, ce: u32, time: f32, info: u32) -> f32 {
    let (cnt_g, buf_g, list_g) = statics;
    let w = m.u32(buf_g);
    let r = o.call(m, &out::SI_GET_EXPR_FCURVE_USED, &[e, cnt_g, w, 0, list_g, 0xb8]);
    m.set_u32(buf_g, r);
    let (cnt, list) = (m.i32(cnt_g), m.u32(list_g));
    get_total_weight(m, o, d, ce, time, cnt, list, info).to_f32()
}

#[allow(clippy::too_many_arguments)]
fn cluster_nurbs(m: &mut Mem, o: &mut dyn Outside, kd: &NKind, org: u32, point: i32, clusters: Ptr32<Kluster>, mode: i16, time: f32, which: i32, outp: u32, upd: u32) -> i32 {
    let (cnt_g, _, list_g) = kd.statics;
    let mut ret: i16 = 0;
    let mut matched: i32 = 0;
    for c in [8, 4, 0, 0xc] {
        m.set_u32(outp.wrapping_add(c), 0);
    }
    let mut base = [0f32; 4];
    let mut acc = [0f32; 4];
    if mode == 1 {
        if org == 0 {
            return 0;
        }
        let p = m.u32(org.wrapping_add(kd.points)).wrapping_add((point as u32).wrapping_mul(0x24));
        let v: [f32; 4] = core::array::from_fn(|c| F80::from_f64(m.f64(p.wrapping_add(8 * c as u32))).to_f32());
        base = euclid4(v);
    }
    let dict = crate::globals::cur_dict(m);
    let e = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[dict, m.i16(upd) as i32 as u32, m.u32(upd.wrapping_add(4))]);
    let l64 = m.local(4);
    let info = m.local(8);
    let mut k = clusters;
    while !k.is_null() {
        let ce = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[k.addr(), l64]);
        let fcv = o.call(m, &out::SI_ED_FIND_FCV, &[ce, 2, 0x2bd]);
        let d = m[k].anim;
        if m[k].cursor < m[k].count && !d.is_null() && m[d].active == 1 && fcv != 0 {
            m.set_u32(info + 4, m.u32(l64));
            let v = key_value(m, o, kd.statics, e, fcv, time, which, info);
            let ar = around(m, d, v);
            let ks = ar.ks;
            let idx = m[k].indexes.addr();
            let at = |j: i32| idx.wrapping_add((j as u32).wrapping_mul(4));
            match m[d].interp {
                0 => {
                    let mut j = m[k].cursor;
                    let (p1, p2) = (m[ks[1]].points.addr(), m[ks[2]].points.addr());
                    let count = m[k].count;
                    while j < count {
                        if m.i32(at(j)) == point {
                            let off = (j as u32).wrapping_mul(16);
                            let a = euclid4(read4(m, p1.wrapping_add(off)));
                            let b = euclid4(read4(m, p2.wrapping_add(off)));
                            let p: [F80; 4] = core::array::from_fn(|c| x(a[c]) + (x(b[c]) - x(a[c])) * ar.frac);
                            contribute4(m, mode, outp, &base, &mut acc, p);
                            matched += 1;
                            ret = 1;
                            m[k].cursor = j + 1;
                            break;
                        }
                        j += 1;
                    }
                }
                1 => {
                    let w = cardinal_coeff(ar.frac);
                    let cc = [w[2], w[3], w[4], w[5]];
                    let ps: [u32; 4] = core::array::from_fn(|i| m[ks[i]].points.addr());
                    let mut j = m[k].cursor;
                    while j < m[k].count {
                        if m.i32(at(j)) == point {
                            let off = (j as u32).wrapping_mul(16);
                            let q: [[f32; 4]; 4] = core::array::from_fn(|i| euclid4(read4(m, ps[i].wrapping_add(off))));
                            let f: [f32; 4] = if ar.first_real { q[0] } else { core::array::from_fn(|c| virt(q[1][c], q[2][c], q[3][c])) };
                            let l: [f32; 4] = if ar.last_real { q[3] } else { core::array::from_fn(|c| virt(q[2][c], q[1][c], q[0][c])) };
                            let p: [F80; 4] = core::array::from_fn(|c| card_ext(cc, [f[c], q[1][c], q[2][c], l[c]]));
                            contribute4(m, mode, outp, &base, &mut acc, p);
                            matched += 1;
                            ret = 1;
                            m[k].cursor = j + 1;
                            break;
                        }
                        j += 1;
                    }
                }
                2 => {
                    let total = total_weight(m, o, kd.statics, e, d, ce, time, info);
                    if x(total).lt(x(MIN_WEIGHT)) {
                        return 0;
                    }
                    let mut j = m[k].cursor;
                    let count = m[k].count;
                    while j < count {
                        if m.i32(at(j)) == point {
                            let mut dsum = [0f32; 4];
                            let mut fcv2 = o.call(m, &out::SI_ED_FIND_FCV, &[ce, 2, 0x2be]);
                            let mut kk: i32 = 0;
                            while kk < m[d].nb_keys {
                                let kp = get_cluster_key_pointer(m, d, kk);
                                if !kp.is_null() && !m[kp].points.is_null() {
                                    m.set_i32(info, kk);
                                    let (cnt, list) = (m.i32(cnt_g), m.u32(list_g));
                                    let v = crate::shape::fcrv_val_with_expr(m, o, fcv2, time, 0, cnt, list, info);
                                    if v.gt(x(MIN_WEIGHT)) {
                                        let vt = v / total;
                                        let e4 = euclid4(read4(m, kpos(m, kp, j, 16)));
                                        let q: [F80; 4] = core::array::from_fn(|c| vt * e4[c]);
                                        match mode {
                                            0 => {
                                                for (c, v) in q.iter().enumerate() {
                                                    let a = outp.wrapping_add(4 * c as u32);
                                                    let s = (*v + m.f32(a)).to_f32();
                                                    m.set_f32(a, s);
                                                }
                                            }
                                            1 => {
                                                for c in 0..4 {
                                                    dsum[c] = (q[c] + dsum[c]).to_f32();
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                fcv2 = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[ce, 2, 0x2be]);
                                kk += 1;
                            }
                            if mode == 1 {
                                for c in 0..3 {
                                    acc[c] = ((x(dsum[c]) - base[c]) + acc[c]).to_f32();
                                }
                                acc[3] = (x(acc[3]) + dsum[3]).to_f32();
                            }
                            matched += 1;
                            ret = 1;
                            break;
                        }
                        j += 1;
                    }
                }
                _ => {}
            }
        }
        k = m[k].next;
    }
    if mode == 0 && matched > 1 {
        let n = F80::from(matched);
        for c in 0..4u32 {
            let a = outp.wrapping_add(4 * c);
            let s = (x(m.f32(a)) / n).to_f32();
            m.set_f32(a, s);
        }
    } else if mode == 1 {
        for c in 0..4usize {
            m.set_f32(outp.wrapping_add(4 * c as u32), (x(base[c]) + acc[c]).to_f32());
        }
    }
    let wa = outp.wrapping_add(0xc);
    if x(m.f32(wa)).lt(x(MIN_WEIGHT)) {
        m.set_f32(wa, F_MIN_WEIGHT);
    }
    ret as i32
}

/// One NURBS cluster's position for the point, unrounded: summed into `out` (mode 0), or,
/// in mode 1, x, y and z as their differences from the original position and w as it is,
/// summed into the accumulator.
fn contribute4(m: &mut Mem, mode: i16, outp: u32, base: &[f32; 4], acc: &mut [f32; 4], p: [F80; 4]) {
    match mode {
        0 => {
            for (c, v) in p.iter().enumerate() {
                let a = outp.wrapping_add(4 * c as u32);
                let s = (*v + m.f32(a)).to_f32();
                m.set_f32(a, s);
            }
        }
        1 => {
            for c in 0..3 {
                acc[c] = ((p[c] - base[c]) + acc[c]).to_f32();
            }
            acc[3] = (p[3] + acc[3]).to_f32();
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- splines and faces

/// What tells the three interpolators of nine floats a point apart (a spline key: three
/// vectors; a point of a face of NURBS curves is given three vectors too).
struct K9 {
    /// The original position (mode 1) of point `b` of the original definition.
    base: fn(&Mem, u32, i32) -> [f32; 9],
    /// The cluster's points are pairs (curve, point) and both must match (the faces), or
    /// single point numbers (the spline).
    pair: bool,
    statics: (u32, u32, u32),
}

/// A spline key's three vectors, copied as they are (+0x14 the keys, 0x30 bytes each).
fn base_spline(m: &Mem, org: u32, b: i32) -> [f32; 9] {
    let k = m.u32(org.wrapping_add(0x14)).wrapping_add((b as u32).wrapping_mul(0x30));
    core::array::from_fn(|i| m.f32(k.wrapping_add(4 * i as u32)))
}

/// A NURBS curve's point (+0x2c the points, 0x24 bytes each) as x/w, y/w, z/w (doubles
/// divided in the register, each quotient rounded to float), three times.
fn base_nurbs_face(m: &Mem, org: u32, b: i32) -> [f32; 9] {
    let p = m.u32(org.wrapping_add(0x2c)).wrapping_add((b as u32).wrapping_mul(0x24));
    let w = F80::from_f64(m.f64(p.wrapping_add(0x18)));
    let v: [f32; 3] = core::array::from_fn(|c| (F80::from_f64(m.f64(p.wrapping_add(8 * c as u32))) / w).to_f32());
    core::array::from_fn(|i| v[i % 3])
}

const SPLINE: K9 = K9 {
    base: base_spline,
    pair: false,
    statics: (crate::globals::CSPLINE_NBWCLS.0, crate::globals::CSPLINE_WFCV.0, crate::globals::CSPLINE_WFCV_SUBID.0),
};

const FACE: K9 = K9 {
    base: base_spline,
    pair: true,
    statics: (crate::globals::CFACE_NBWCLS.0, crate::globals::CFACE_WFCV.0, crate::globals::CFACE_WFCV_SUBID.0),
};

const NFACE: K9 = K9 {
    base: base_nurbs_face,
    pair: true,
    statics: (crate::globals::CNFACE_NBWCLS.0, crate::globals::CNFACE_WFCV.0, crate::globals::CNFACE_WFCV_SUBID.0),
};

/// [soft 0x081ed884 si_clusterSplineInterpolate]
///
/// `si_clusterSplineInterpolate(org, point, clusters, mode, time, which, &out0, &out1, &out2,
/// update node)`: where the clusters' animation puts key `point` of a spline: its three
/// vectors (36 bytes in a cluster key) in `out0`, `out1`, `out2`. As the mesh routine, with
/// these differences read from its code: no cursor test (as the patch routine); the key
/// curve is read for a weighted cluster too; a weighted cluster's cursor moves. Where the
/// original keeps values in the register and where it stores them differs from vector to
/// vector (see [`card9`]); the made-up first neighbour of a cardinal cluster takes its second
/// vector from the first key twice and the second key, (2·K1 − 2·K1) + K2, not from K3.
#[allow(clippy::too_many_arguments)]
pub fn cluster_spline_interpolate(m: &mut Mem, o: &mut dyn Outside, org: Ptr32<si_types::geom::Spline>, point: i32, clusters: Ptr32<Kluster>, mode: i16, time: f32, which: i32, out0: u32, out1: u32, out2: u32, upd: u32) -> i32 {
    let sp = m.sp();
    let r = cluster_nine(m, o, &SPLINE, org.addr(), point, point, clusters, mode, time, which, [out0, out1, out2], upd);
    m.set_sp(sp);
    r
}

/// [soft 0x081ec504 si_clusterFaceInterpolate]
///
/// `si_clusterFaceInterpolate(org, curve, point, clusters, mode, time, which, &out0, &out1,
/// &out2, update node)`: the same for key `point` of curve `curve` of a face of splines
/// (`org` is that curve's spline in the original definition). A face cluster's points are
/// pairs (curve, point): both must match; the cursor counts words and moves by two, the
/// loop runs to twice the count, and the key positions are those of the pair's number
/// (the cursor halved, toward zero).
#[allow(clippy::too_many_arguments)]
pub fn cluster_face_interpolate(m: &mut Mem, o: &mut dyn Outside, org: Ptr32<si_types::geom::Spline>, curve: i32, point: i32, clusters: Ptr32<Kluster>, mode: i16, time: f32, which: i32, out0: u32, out1: u32, out2: u32, upd: u32) -> i32 {
    let sp = m.sp();
    let r = cluster_nine(m, o, &FACE, org.addr(), curve, point, clusters, mode, time, which, [out0, out1, out2], upd);
    m.set_sp(sp);
    r
}

/// [soft 0x081eb170 si_clusterNurbsFaceInterpolate]
///
/// The same for point `point` of curve `curve` of a face of NURBS curves (`org` is that
/// NURBS curve): its keys hold three vectors as a spline's do, the original position is the
/// point's x/w, y/w, z/w three times; no weight is blended.
#[allow(clippy::too_many_arguments)]
pub fn cluster_nurbs_face_interpolate(m: &mut Mem, o: &mut dyn Outside, org: Ptr32<si_types::geom::NurbsCrv>, curve: i32, point: i32, clusters: Ptr32<Kluster>, mode: i16, time: f32, which: i32, out0: u32, out1: u32, out2: u32, upd: u32) -> i32 {
    let sp = m.sp();
    let r = cluster_nine(m, o, &NFACE, org.addr(), curve, point, clusters, mode, time, which, [out0, out1, out2], upd);
    m.set_sp(sp);
    r
}

/// `f32(c3·l + f32(c2·k2 + f32(c1·k1 + f32(c0·f))))`: the cardinal blend rounded at every
/// sum, as the spline routines compute some coordinates.
fn card_stored(c: [f32; 4], f: f32, k1: f32, k2: f32, l: f32) -> f32 {
    let s = (x(c[0]) * f).to_f32();
    let s = (x(c[1]) * k1 + s).to_f32();
    let s = (x(c[2]) * k2 + s).to_f32();
    (x(c[3]) * l + s).to_f32()
}

/// The cardinal blend of nine floats as the three routines compute it (the same code in
/// each): the first vector's x and y rounded at every sum ([`card_stored`]), its z
/// unrounded; the second vector unrounded; the third vector's x rounded at every sum, its y
/// unrounded, its z as `f32(c3·l + f32((f32(c0·f) + f32(c1·k1)) + c2·k2))`. A rounded
/// coordinate is returned as its float's value.
fn card9(c: [f32; 4], f: &[f32; 9], k1: &[f32; 9], k2: &[f32; 9], l: &[f32; 9]) -> [F80; 9] {
    let ext = |i: usize| card_ext(c, [f[i], k1[i], k2[i], l[i]]);
    let st = |i: usize| x(card_stored(c, f[i], k1[i], k2[i], l[i]));
    let z2 = {
        let t = (x(c[0]) * f[8]).to_f32();
        let u = (x(c[1]) * k1[8]).to_f32();
        let v = ((x(t) + u) + x(c[2]) * k2[8]).to_f32();
        x((x(c[3]) * l[8] + v).to_f32())
    };
    [st(0), st(1), ext(2), ext(3), ext(4), ext(5), st(6), ext(7), z2]
}

#[allow(clippy::too_many_arguments)]
fn cluster_nine(m: &mut Mem, o: &mut dyn Outside, kd: &K9, org: u32, a: i32, b: i32, clusters: Ptr32<Kluster>, mode: i16, time: f32, which: i32, outs: [u32; 3], upd: u32) -> i32 {
    let (cnt_g, _, list_g) = kd.statics;
    let step: i32 = if kd.pair { 2 } else { 1 };
    let mut ret: i16 = 0;
    let mut matched: i32 = 0;
    for o3 in outs {
        for c in [8, 4, 0] {
            m.set_u32(o3.wrapping_add(c), 0);
        }
    }
    let oa = |i: usize| outs[i / 3].wrapping_add(4 * (i % 3) as u32);
    let mut base = [0f32; 9];
    let mut acc = [0f32; 9];
    if mode == 1 {
        if org == 0 {
            return 0;
        }
        base = (kd.base)(m, org, b);
    }
    let dict = crate::globals::cur_dict(m);
    let e = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[dict, m.i16(upd) as i32 as u32, m.u32(upd.wrapping_add(4))]);
    let l64 = m.local(4);
    let info = m.local(8);
    let mut k = clusters;
    while !k.is_null() {
        let ce = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[k.addr(), l64]);
        let fcv = o.call(m, &out::SI_ED_FIND_FCV, &[ce, 2, 0x2bd]);
        let d = m[k].anim;
        if !d.is_null() && m[d].active == 1 && fcv != 0 {
            m.set_u32(info + 4, m.u32(l64));
            let v = key_value(m, o, kd.statics, e, fcv, time, which, info);
            let ar = around(m, d, v);
            let ks = ar.ks;
            let idx = m[k].indexes.addr();
            let hit = |m: &Mem, j: i32| {
                let at = idx.wrapping_add((j as u32).wrapping_mul(4));
                m.i32(at) == a && (!kd.pair || m.i32(at.wrapping_add(4)) == b)
            };
            // the position of the cursor's point in a key: the pair's number for a face
            let first = |j: i32| (if kd.pair { j / 2 } else { j } as u32).wrapping_mul(0x24);
            let read9 = |m: &Mem, p: u32| -> [f32; 9] { core::array::from_fn(|i| m.f32(p.wrapping_add(4 * i as u32))) };
            match m[d].interp {
                0 => {
                    let mut j = m[k].cursor;
                    let mut off = first(j);
                    let (p1, p2) = (m[ks[1]].points.addr(), m[ks[2]].points.addr());
                    let limit = m[k].count.wrapping_mul(step);
                    while j < limit {
                        if hit(m, j) {
                            let (k1, k2) = (read9(m, p1.wrapping_add(off)), read9(m, p2.wrapping_add(off)));
                            let mut p: [F80; 9] = core::array::from_fn(|i| x(k1[i]) + (x(k2[i]) - x(k1[i])) * ar.frac);
                            // the third vector is stored before it is used
                            for v in p.iter_mut().skip(6) {
                                *v = x(v.to_f32());
                            }
                            match mode {
                                0 => {
                                    for (i, v) in p.iter().enumerate() {
                                        let s = (*v + m.f32(oa(i))).to_f32();
                                        m.set_f32(oa(i), s);
                                    }
                                }
                                1 => {
                                    for i in 0..9 {
                                        acc[i] = ((p[i] - base[i]) + acc[i]).to_f32();
                                    }
                                }
                                _ => {}
                            }
                            matched += 1;
                            ret = 1;
                            m[k].cursor = j + step;
                            break;
                        }
                        off = off.wrapping_add(0x24);
                        j += step;
                    }
                }
                1 => {
                    let w = cardinal_coeff(ar.frac);
                    let cc = [w[2], w[3], w[4], w[5]];
                    let ps: [u32; 4] = core::array::from_fn(|i| m[ks[i]].points.addr());
                    let mut j = m[k].cursor;
                    let mut off = first(j);
                    while j < m[k].count.wrapping_mul(step) {
                        if hit(m, j) {
                            let q: [[f32; 9]; 4] = core::array::from_fn(|i| read9(m, ps[i].wrapping_add(off)));
                            let f: [f32; 9] = if ar.first_real {
                                q[0]
                            } else {
                                // the second vector from the first key twice and the second key
                                core::array::from_fn(|i| if (3..6).contains(&i) { virt(q[1][i], q[1][i], q[2][i]) } else { virt(q[1][i], q[2][i], q[3][i]) })
                            };
                            let l: [f32; 9] = if ar.last_real { q[3] } else { core::array::from_fn(|i| virt(q[2][i], q[1][i], q[0][i])) };
                            let p = card9(cc, &f, &q[1], &q[2], &l);
                            match mode {
                                0 => {
                                    for (i, v) in p.iter().enumerate() {
                                        let s = (*v + m.f32(oa(i))).to_f32();
                                        m.set_f32(oa(i), s);
                                    }
                                }
                                1 => {
                                    for i in 0..9 {
                                        // the first vector's x and y: the difference stored first
                                        let dlt = if i < 2 { x((p[i] - base[i]).to_f32()) } else { p[i] - base[i] };
                                        acc[i] = (dlt + acc[i]).to_f32();
                                    }
                                }
                                _ => {}
                            }
                            matched += 1;
                            ret = 1;
                            m[k].cursor = j + step;
                            break;
                        }
                        off = off.wrapping_add(0x24);
                        j += step;
                    }
                }
                2 => {
                    let total = total_weight(m, o, kd.statics, e, d, ce, time, info);
                    if x(total).lt(x(MIN_WEIGHT)) {
                        return 0;
                    }
                    let mut j = m[k].cursor;
                    let mut off = first(j);
                    let limit = m[k].count.wrapping_mul(step);
                    while j < limit {
                        if hit(m, j) {
                            let mut dsum = [0f32; 9];
                            let mut fcv2 = o.call(m, &out::SI_ED_FIND_FCV, &[ce, 2, 0x2be]);
                            let mut kk: i32 = 0;
                            while kk < m[d].nb_keys {
                                let kp = get_cluster_key_pointer(m, d, kk);
                                if !kp.is_null() && !m[kp].points.is_null() {
                                    m.set_i32(info, kk);
                                    let (cnt, list) = (m.i32(cnt_g), m.u32(list_g));
                                    let v = crate::shape::fcrv_val_with_expr(m, o, fcv2, time, 0, cnt, list, info);
                                    if v.gt(x(MIN_WEIGHT)) {
                                        let vt = v / total;
                                        let kv = read9(m, m[kp].points.addr().wrapping_add(off));
                                        let q: [F80; 9] = core::array::from_fn(|i| if i < 6 { vt * kv[i] } else { x((vt * kv[i]).to_f32()) });
                                        match mode {
                                            0 => {
                                                for (i, v) in q.iter().enumerate() {
                                                    let s = (*v + m.f32(oa(i))).to_f32();
                                                    m.set_f32(oa(i), s);
                                                }
                                            }
                                            1 => {
                                                for i in 0..9 {
                                                    dsum[i] = (q[i] + dsum[i]).to_f32();
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                fcv2 = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[ce, 2, 0x2be]);
                                kk += 1;
                            }
                            if mode == 1 {
                                for i in 0..9 {
                                    acc[i] = ((x(dsum[i]) - base[i]) + acc[i]).to_f32();
                                }
                            }
                            matched += 1;
                            ret = 1;
                            m[k].cursor = j + step;
                            break;
                        }
                        off = off.wrapping_add(0x24);
                        j += step;
                    }
                }
                _ => {}
            }
        }
        k = m[k].next;
    }
    if mode == 0 && matched > 1 {
        let n = F80::from(matched);
        for i in 0..9 {
            let s = (x(m.f32(oa(i))) / n).to_f32();
            m.set_f32(oa(i), s);
        }
    } else if mode == 1 {
        for i in 0..9 {
            m.set_f32(oa(i), (x(base[i]) + acc[i]).to_f32());
        }
    }
    ret as i32
}

/// The cardinal blend of four values, unrounded.
fn card_ext(c: [f32; 4], p: [f32; 4]) -> F80 {
    ((x(c[0]) * p[0] + x(c[1]) * p[1]) + x(c[2]) * p[2]) + x(c[3]) * p[3]
}

/// One cluster's position for the point, unrounded: summed into `out` (mode 0), or its
/// difference from the original position summed into the accumulator (mode 1).
fn contribute(m: &mut Mem, mode: i16, outp: u32, base: &[f32; 3], acc: &mut [f32; 3], p: [F80; 3]) {
    match mode {
        0 => {
            for (c, v) in p.iter().enumerate() {
                let a = outp.wrapping_add(4 * c as u32);
                let s = (*v + m.f32(a)).to_f32();
                m.set_f32(a, s);
            }
        }
        1 => {
            for c in 0..3 {
                acc[c] = ((p[c] - base[c]) + acc[c]).to_f32();
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- second pass: keys and curves

use crate::globals as g;

/// The flag bytes at +0xa and +0x3fa of a model node: bit 4 marks it as changed.
fn touch_flags(m: &mut Mem, model: u32) {
    m.or_u8(model.wrapping_add(MODEL_FLAGS_0A), 0x10);
    m.or_u8(model.wrapping_add(MODEL_FLAGS_3FA), 0x10);
}

fn active_kluster(m: &mut Mem, o: &mut dyn Outside, model: u32) -> Ptr32<Kluster> {
    Ptr32::new(o.call(m, &out::SI_KLUSTER_GET_ACTIVE, &[model]))
}

/// [soft 0x081e84e4 modelLoadFcurveCluster]
///
/// Copies the key curve (type 0x2bd) of `src`'s active cluster onto `dst`'s
/// (`si_singleFcurveCopy`, with the index of `dst`'s cluster element) and marks `dst` as
/// changed: `dst`, or 0 with a message ("Cannot copy cluster.") when either model has no
/// active cluster with keys. No caller in the program.
pub fn model_load_fcurve_cluster(m: &mut Mem, o: &mut dyn Outside, dst: Ptr32<Model>, src: Ptr32<Model>) -> u32 {
    let refuse = |m: &mut Mem, o: &mut dyn Outside| {
        o.call(m, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08694a30, 3]);
        0
    };
    if dst.is_null() || src.is_null() || m[dst].klusters.is_null() {
        return refuse(m, o);
    }
    let kd = active_kluster(m, o, dst.addr());
    if kd.is_null() || m[src].klusters.is_null() {
        return refuse(m, o);
    }
    let ks = active_kluster(m, o, src.addr());
    if ks.is_null() || m[kd].anim.is_null() || m[ks].anim.is_null() {
        return refuse(m, o);
    }
    let index = m.local(4);
    let es = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[ks.addr(), index]);
    let ed = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[kd.addr(), index]);
    o.call(m, &out::SI_SINGLE_FCURVE_COPY, &[es, ed, m.u32(index), 0x2bd]);
    m.release(4);
    touch_flags(m, dst.addr());
    dst.addr()
}

/// [soft 0x081e8358 modelLoadFcurveClusterWeight]
///
/// Copies the weight curves (type 0x2be) of `src`'s active cluster onto `dst`'s, one by
/// one in list order (`SI_fcurveTransfer`; a curve `dst` lacks is made, type 0xb8, and
/// added in chapter `SI_cid[2]`, an existing one emptied first), and marks `dst` as
/// changed: `dst`, or 0 with a message when a model has no active cluster with keys, or
/// the numbers of keys differ. No caller in the program.
pub fn model_load_fcurve_cluster_weight(m: &mut Mem, o: &mut dyn Outside, dst: Ptr32<Model>, src: Ptr32<Model>) -> u32 {
    let refuse = |m: &mut Mem, o: &mut dyn Outside, msg: u32| {
        o.call(m, &out::SI_STATUS_DISPLAY_MESSAGE, &[msg, 3]);
        0
    };
    if dst.is_null() || src.is_null() || m[dst].klusters.is_null() {
        return refuse(m, o, 0x086949e0);
    }
    let kd = active_kluster(m, o, dst.addr());
    if kd.is_null() || m[src].klusters.is_null() {
        return refuse(m, o, 0x086949e0);
    }
    let ks = active_kluster(m, o, src.addr());
    if ks.is_null() || m[kd].anim.is_null() || m[ks].anim.is_null() || m[m[ks].anim].nb_keys < 0 {
        return refuse(m, o, 0x086949e0);
    }
    if m[m[ks].anim].nb_keys != m[m[kd].anim].nb_keys {
        return refuse(m, o, 0x08694a00);
    }
    let index = m.local(4);
    let es = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[ks.addr(), index]);
    let mut fs = if es != 0 { o.call(m, &out::SI_ED_FIND_FCV, &[es, 2, 0x2be]) } else { 0 };
    if fs != 0 {
        let ed = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[kd.addr(), index]);
        let mut fd = if ed != 0 { o.call(m, &out::SI_ED_FIND_FCV, &[ed, 2, 0x2be]) } else { 0 };
        loop {
            if fd != 0 {
                o.call(m, &out::SI_FCURVE_REMOVE_ALL_KEYS, &[fd]);
            } else {
                fd = o.call(m, &out::SI_FCURVE_ALLOCATE, &[0xb8, kd.addr()]);
                let cid2 = m[g::SI_CID][2] as u32;
                crate::edanim::ed_fcv_add(m, o, fd, ed, cid2, m.u32(index));
            }
            o.call(m, &out::SI_FCURVE_TRANSFER, &[fs, fd]);
            fs = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[es, 2, 0x2be]);
            fd = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[ed, 2, 0x2be]);
            if fs == 0 {
                break;
            }
        }
    }
    m.release(4);
    touch_flags(m, dst.addr());
    dst.addr()
}

/// The active cluster's key data when the model's cluster animation is on and the cluster
/// has keys and a current key (what the key commands test first).
fn key_command_data(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> Option<Ptr32<ClusterData>> {
    if model.is_null() || m[model].klusters.is_null() {
        return None;
    }
    let k = active_kluster(m, o, model.addr());
    if k.is_null() {
        return None;
    }
    let c: Ptr32<Cluster> = Ptr32::new(m[model].cluster.addr());
    if c.is_null() || m[c].active == 0 {
        return None;
    }
    let d = m[k].anim;
    if d.is_null() || m[d].keys.is_null() || m[d].nb_keys <= 0 {
        return None;
    }
    Some(d)
}

/// [soft 0x081e9090 si_clusterNextKey]
///
/// The menu command: the active cluster's next key (after the last, the first) made
/// current, the points put there (`copyClusterKey`), the geometry marked as changed. The
/// key's place (`clusterKeyNumber`), or −1 when the model's cluster animation is off or
/// the cluster has no keys.
pub fn cluster_next_key(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> i32 {
    let Some(d) = key_command_data(m, o, model) else { return -1 };
    let cur = m[d].current;
    m[d].current = if !cur.is_null() && !m[cur].next.is_null() { m[cur].next } else { m[d].keys };
    copy_cluster_key(m, o, model);
    o.call(m, &out::SI_MODEL_TOUCH_GEOMETRY, &[model.addr()]);
    cluster_key_number(m, o, model)
}

/// [soft 0x081e9120 si_clusterPreviousKey]
///
/// The same with the previous key (before the first, the last; when the current key is not
/// in the chain, none).
pub fn cluster_previous_key(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> i32 {
    let Some(d) = key_command_data(m, o, model) else { return -1 };
    let (keys, cur) = (m[d].keys, m[d].current);
    let mut e = keys;
    if keys == cur {
        while !e.is_null() && !m[e].next.is_null() {
            e = m[e].next;
        }
    } else if !cur.is_null() {
        while m[e].next != cur {
            e = m[e].next;
            if e.is_null() {
                break;
            }
        }
    }
    m[d].current = e;
    copy_cluster_key(m, o, model);
    o.call(m, &out::SI_MODEL_TOUCH_GEOMETRY, &[model.addr()]);
    cluster_key_number(m, o, model)
}

/// [soft 0x081e91d8 clusterAddFcurve]
///
/// Adds the key of a new cluster key `n` to the cluster element's key curve (type 0x2bd,
/// made when missing by `SI_fcurveCheckAdd1` in chapter `chapter`): when the curve has keys,
/// its values above `above` are raised by 1 first; the key is at the current frame with
/// value n. 1, or 0 without a model or key data.
#[allow(clippy::too_many_arguments)]
pub fn cluster_add_fcurve(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, d: Ptr32<ClusterData>, elem: u32, chapter: i32, n: i32, above: f32) -> i32 {
    if model.is_null() || d.is_null() {
        return 0;
    }
    let l = m.local(4);
    o.call(m, &out::SI_FCURVE_CHECK_ADD1, &[elem, chapter as u32, 0x2bd, l]);
    let f = m.u32(l);
    if m.i32(f.wrapping_add(0xc)) > 0 {
        o.call(m, &out::SI_FCURVE_INC_VALUE_ABOVE, &[f, above.to_bits(), 1f32.to_bits()]);
    }
    let f = m.u32(l);
    let w = m.u16(f.wrapping_add(0x1e)) as u32;
    let k = o.call(m, &out::SI_FKEY_FRAME_CREATE, &[m[g::SI_FRAME_CURRENT] as u32, F80::from(n).to_f32().to_bits(), 0, 0, w]);
    o.call(m, &out::SI_FCURVE_ADD_KEY, &[m.u32(l), k]);
    m.release(4);
    1
}

/// [soft 0x081e9268 clusterAddWeightFcurve]
///
/// Makes the weight curve of new cluster key `n + 1` (0 when `n` is not below the number
/// of keys, or without a model or key data): every existing weight curve (type 0x2be) gets
/// a key of value 0 at the current frame; a new curve (type 0xb8) is put in the cluster
/// element's list as number n + 1 (`index` its index), the curves renamed
/// (`SI_updateWeightFcvNames`), the new one given a key of value 1 at the current frame
/// and a key of value 0 at every key time of the other curves lying more than 1e-5 s from
/// now where it has none within 1e-5 s. 1.
#[allow(clippy::too_many_arguments)]
pub fn cluster_add_weight_fcurve(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, k: Ptr32<Kluster>, d: Ptr32<ClusterData>, elem: u32, index: i32, n: i32) -> i32 {
    if model.is_null() || d.is_null() || n >= m[d].nb_keys {
        return 0;
    }
    let mut f = o.call(m, &out::SI_ED_FIND_FCV, &[elem, 2, 0x2be]);
    while f != 0 {
        let w = m.u16(f.wrapping_add(0x1e)) as u32;
        let key = o.call(m, &out::SI_FKEY_FRAME_CREATE, &[m[g::SI_FRAME_CURRENT] as u32, 0, 0, 1, w]);
        o.call(m, &out::SI_FCURVE_ADD_KEY, &[f, key]);
        f = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[elem, 2, 0x2be]);
    }
    let nf = o.call(m, &out::SI_FCURVE_ALLOCATE, &[0xb8, k.addr()]);
    let cid2 = m[g::SI_CID][2] as u32;
    crate::edanim::ed_fcv_insert(m, o, nf, elem, cid2, index as u32, n + 1);
    o.call(m, &out::SI_UPDATE_WEIGHT_FCV_NAMES, &[elem, 2, 0x2be]);
    let w = m.u16(nf.wrapping_add(0x1e)) as u32;
    let key = o.call(m, &out::SI_FKEY_FRAME_CREATE, &[m[g::SI_FRAME_CURRENT] as u32, 1f32.to_bits(), 0, 1, w]);
    o.call(m, &out::SI_FCURVE_ADD_KEY, &[nf, key]);
    let now = o.call_f(m, &out::SI_FRAME_TO_SEC, &[m[g::SI_FRAME_CURRENT] as u32]).to_f32();
    let eps = x(1e-5f64);
    let mut c = o.call(m, &out::SI_ED_FIND_FCV, &[elem, 2, 0x2be]);
    while c != 0 {
        if c != nf {
            let mut j: i32 = 0;
            while j < m.i32(c.wrapping_add(0xc)) {
                let at = m.u32(c.wrapping_add(8)).wrapping_add((j as u32).wrapping_mul(0x24));
                let kt = x(m.f32(at));
                if (kt - now).abs().gt(eps) {
                    let mut q: i32 = 0;
                    while q < m.i32(nf.wrapping_add(0xc)) {
                        let qt = m.f32(m.u32(nf.wrapping_add(8)).wrapping_add((q as u32).wrapping_mul(0x24)));
                        if (x(qt) - kt).abs().lt(eps) {
                            break;
                        }
                        q += 1;
                    }
                    if q >= m.i32(nf.wrapping_add(0xc)) {
                        let w = m.u16(nf.wrapping_add(0x1e)) as u32;
                        let at = m.u32(c.wrapping_add(8)).wrapping_add((j as u32).wrapping_mul(0x24));
                        let key = o.call(m, &out::SI_FKEY_CREATE, &[m.u32(at), 0, 0, 1, w]);
                        o.call(m, &out::SI_FCURVE_ADD_KEY, &[nf, key]);
                    }
                }
                j += 1;
            }
        }
        c = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[elem, 2, 0x2be]);
    }
    1
}

/// [soft 0x081e9484 clusterWeightFcurveDispose]
///
/// Frees a cluster's weight curves (types 0x2be, chapter `SI_cid[2]`) when its element has
/// any: 1, else 0 (also without key data).
pub fn cluster_weight_fcurve_dispose(m: &mut Mem, o: &mut dyn Outside, k: Ptr32<Kluster>, d: Ptr32<ClusterData>, elem: u32) -> i32 {
    if d.is_null() || o.call(m, &out::SI_ED_FIND_FCV, &[elem, 2, 0x2be]) == 0 {
        return 0;
    }
    let cid2 = m[g::SI_CID][2] as u32;
    crate::edanim::ed_fcv_dispose(m, o, k.addr(), cid2, 2, 0x2be, 0x2be);
    1
}

/// [soft 0x081e94d8 clusterRemoveWeightFcurve]
///
/// Takes the weight curve of key `n` away: the curves from number n + 1 on given up
/// (`SI_edFcvFilterDispose`, types 0x2be and 0xb8) and the n-th of the list freed. 1, or 0
/// for `n` outside the keys, no key data, or a list that ends first.
pub fn cluster_remove_weight_fcurve(m: &mut Mem, o: &mut dyn Outside, k: Ptr32<Kluster>, d: Ptr32<ClusterData>, elem: u32, n: i32) -> i32 {
    if d.is_null() || n < 0 || n >= m[d].nb_keys {
        return 0;
    }
    let l = m.local(4);
    let r = 'r: {
        let f = o.call(m, &out::SI_ED_FIND_FCV, &[elem, 2, 0x2be]);
        m.set_u32(l, f);
        if f == 0 {
            break 'r 0;
        }
        let mut i = 0;
        while i < n {
            let f = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[elem, 2, 0x2be]);
            m.set_u32(l, f);
            if f == 0 {
                break 'r 0;
            }
            i += 1;
        }
        let cid2 = m[g::SI_CID][2] as u32;
        crate::edanim::ed_fcv_filter_dispose(m, o, k.addr(), cid2, 2, 0x2be, 0xb8, n + 1);
        o.call(m, &out::SI_FCURVE_REMOVE, &[l]);
        1
    };
    m.release(4);
    r
}

/// [soft 0x081e9588 si_clusterRenameWeightFcurve]
///
/// Gives the active cluster's key curve and weight curves their display names
/// (`SI_getFcurveDname`) wherever the scene's curve list (`SI_scngl`) shows them: the key
/// curve `%s` (the cluster's name), the weight curves `%s.%d` with their number. Every
/// weight curve named is the first of the list (`SI_edFindFcv` is asked again each time).
/// 1, or 0 without an active cluster with key data or when no weight curve is found. No
/// caller in the program.
pub fn cluster_rename_weight_fcurve(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> i32 {
    if model.is_null() || m[model].klusters.is_null() {
        return 0;
    }
    let k = active_kluster(m, o, model.addr());
    if k.is_null() || m[k].anim.is_null() {
        return 0;
    }
    let d = m[k].anim;
    let index = m.local(4);
    let buf = m.local(0x50);
    let ce = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[k.addr(), index]);
    let f = o.call(m, &out::SI_ED_FIND_FCV, &[ce, 2, 0x2bd]);
    let name = |m: &mut Mem, o: &mut dyn Outside, f: u32, n: Option<i32>| {
        let mut node = m.u32(m[g::SI_SCNGL].wrapping_add(4));
        while node != 0 {
            if m.i32(node) == 9 && m.u32(node.wrapping_add(4)) == f {
                let mut text = m.cstr(m[k].name.addr());
                if let Some(n) = n {
                    text.push(b'.');
                    text.extend(si_mem::clib::decimal(n));
                }
                m.put_cstr(buf, &text);
                o.call(m, &out::SI_GET_FCURVE_DNAME, &[f, buf]);
            }
            node = m.u32(node.wrapping_add(0x1c));
        }
    };
    name(m, o, f, None);
    let mut f = o.call(m, &out::SI_ED_FIND_FCV, &[ce, 2, 0x2be]);
    let mut i = 0;
    let mut r = 1;
    while i < m[d].nb_keys {
        if f == 0 {
            r = 0;
            break;
        }
        name(m, o, f, Some(i));
        f = o.call(m, &out::SI_ED_FIND_FCV, &[ce, 2, 0x2be]);
        i += 1;
    }
    m.release(0x54);
    r
}

/// [soft 0x081e8ec4 si_clusterDeleteKey]
///
/// The menu command: the active cluster's current key taken out of the chain and freed;
/// the key before it (none for the first) becomes current; its weight curve removed; the
/// key curve loses its keys between the key's place and the next (when it has more than
/// one key or the cluster one key) and its values above that place are lowered by 1 (above
/// 0 otherwise), and is freed when no key is left; the weight curves too when no cluster
/// key is left. The model is marked as changed. The key's place, or −1 when there is none
/// (also when the cluster has no element, after the key was taken out of the chain).
pub fn cluster_delete_key(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> i32 {
    if model.is_null() || m[model].klusters.is_null() {
        return -1;
    }
    let k = active_kluster(m, o, model.addr());
    if k.is_null() {
        return -1;
    }
    let d = m[k].anim;
    if d.is_null() || m[d].current.is_null() || m[d].keys.is_null() {
        return -1;
    }
    let (cur, keys) = (m[d].current, m[d].keys);
    let place: i32;
    let prev: Ptr32<ClusterKey>;
    if cur == keys {
        place = 0;
        m[d].keys = m[cur].next;
        prev = m[cur].next;
    } else {
        let mut n = 1;
        let mut e = keys;
        while m[e].next != cur {
            e = m[e].next;
            n += 1;
            if e.is_null() {
                return -1;
            }
        }
        let after = m[cur].next;
        m[e].next = after;
        place = n;
        prev = e;
    }
    let index = m.local(4);
    let ce = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[k.addr(), index]);
    m.release(4);
    if ce == 0 {
        return -1;
    }
    cluster_remove_weight_fcurve(m, o, k, d, ce, place);
    let f = o.call(m, &out::SI_ED_FIND_FCV, &[ce, 2, 0x2bd]);
    if f != 0 {
        let minus1 = (-1f32).to_bits();
        if m.i32(f.wrapping_add(0xc)) > 1 || m[d].nb_keys <= 1 {
            let at = F80::from(place).to_f32();
            o.call(m, &out::SI_FCURVE_REMOVE_KEY_BETWEEN, &[f, at.to_bits(), F80::from(place + 1).to_f32().to_bits()]);
            o.call(m, &out::SI_FCURVE_INC_VALUE_ABOVE, &[f, at.to_bits(), minus1]);
        } else {
            o.call(m, &out::SI_FCURVE_INC_VALUE_ABOVE, &[f, 0, minus1]);
        }
        if m.i32(f.wrapping_add(0xc)) <= 0 {
            let cid2 = m[g::SI_CID][2] as u32;
            crate::edanim::ed_fcv_dispose(m, o, k.addr(), cid2, 2, 0x2bd, 0x2bd);
        }
    }
    m[d].nb_keys -= 1;
    if m[d].nb_keys <= 0 {
        cluster_weight_fcurve_dispose(m, o, k, d, ce);
    }
    cluster_key_dispose(m, si_mem::field!(d, ClusterData, current));
    m[d].current = prev;
    touch_flags(m, model.addr());
    place
}

/// [soft 0x081e9724 copyClusterKey]
///
/// Puts the points of the model's first definition where the active cluster's current key
/// says (the model's cluster animation on): the key's positions taken in order for the
/// points the cluster names, by kind: a mesh's vertex positions, a patch's control points
/// (then its surface rebuilt), a face of splines' key vectors (outline, then holes; pairs
/// (curve, key)), a face of NURBS curves' points (pairs (curve, point); a position of 12
/// bytes made a point with weight 1, the pair's place moving by one word, not two), a
/// spline's key vectors, a NURBS curve's or surface's points (moved with
/// `SI_nurbsCrvMovePoint`, the weight stored first, or `SI_nurbsSrfMovePoint`). Another
/// kind shows a message. A model whose word at +0x31c is 5, 6 or 8 then has its envelope
/// brought up to date. 1, or 0 when there is nothing to copy or no first definition.
pub fn copy_cluster_key(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> i32 {
    if model.is_null() {
        return 0;
    }
    let c: Ptr32<Cluster> = Ptr32::new(m[model].cluster.addr());
    if c.is_null() || m[c].active == 0 || m[model].klusters.is_null() {
        return 0;
    }
    let k = active_kluster(m, o, model.addr());
    if k.is_null() || m[k].anim.is_null() {
        return 0;
    }
    let cur = m[m[k].anim].current;
    if cur.is_null() {
        return 0;
    }
    let ma = model.addr();
    let n = m[k].count;
    let idx = |m: &Mem, i: i32| m.i32(m[k].indexes.addr().wrapping_add((i as u32).wrapping_mul(4)));
    let kind = m.i32(ma.wrapping_add(0x30));
    if !matches!(kind, 1 | 2 | 4 | 6 | 9 | 10) {
        o.call(m, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08694b20, 2]);
    } else {
        let geo = m.u32(m.u32(ma.wrapping_add(0x38)));
        if geo == 0 {
            return 0;
        }
        let copy = |m: &mut Mem, from: u32, to: u32, words: u32| {
            for w in 0..words {
                let v = m.u32(from.wrapping_add(4 * w));
                m.set_u32(to.wrapping_add(4 * w), v);
            }
        };
        match kind {
            4 => {
                let (mut to, mut from) = (m.u32(geo), m[cur].points.addr());
                let (mut i, mut j) = (0, 0);
                while j < m.i32(geo.wrapping_add(0x14)) && i < n {
                    if idx(m, i) == j {
                        copy(m, from, to, 3);
                        from = from.wrapping_add(12);
                        i += 1;
                    }
                    j += 1;
                    to = to.wrapping_add(0x14);
                }
            }
            1 => {
                let total = |m: &Mem| (m.u16(geo.wrapping_add(0xc)) as i32).wrapping_mul(m.u16(geo.wrapping_add(0xe)) as i32);
                let (mut to, mut from) = (m.u32(geo.wrapping_add(0x1a4)), m[cur].points.addr());
                let (mut i, mut j) = (0, 0);
                if i < total(m) {
                    while i < n {
                        if idx(m, i) == j {
                            copy(m, from, to, 4);
                            from = from.wrapping_add(16);
                            i += 1;
                        }
                        j += 1;
                        to = to.wrapping_add(16);
                        if j >= total(m) {
                            break;
                        }
                    }
                }
                o.call(m, &out::SI_PATCH_COMPUTE_SURFACE, &[geo, 0]);
            }
            2 => {
                let mut from = m[cur].points.addr();
                let n2 = n.wrapping_mul(2);
                let mut i = 0;
                let nh = |m: &Mem| m.i16(geo.wrapping_add(0x78)) as i32;
                if m.u16(geo.wrapping_add(0x84)) != 0 {
                    let mut j: i32 = -1;
                    if nh(m) >= 0 && i < n2 {
                        loop {
                            let s = if j == -1 { geo.wrapping_add(4) } else { m.u32(geo.wrapping_add(0x7c)).wrapping_add((j as u32).wrapping_mul(0x74)) };
                            if s != 0 {
                                let mut p = 0;
                                let mut key = m.u32(s.wrapping_add(0x14));
                                if m.u16(s.wrapping_add(0x10)) != 0 && i < n2 && key != 0 {
                                    loop {
                                        if idx(m, i) == j && idx(m, i + 1) == p {
                                            copy(m, from, key, 9);
                                            from = from.wrapping_add(36);
                                            i += 2;
                                        }
                                        p += 1;
                                        key = key.wrapping_add(0x30);
                                        if p >= m.u16(s.wrapping_add(0x10)) as i32 || i >= n2 || key == 0 {
                                            break;
                                        }
                                    }
                                }
                            }
                            j += 1;
                            if j >= nh(m) || i >= n2 {
                                break;
                            }
                        }
                    }
                } else {
                    let mut j = 0;
                    if i < nh(m) && i < n2 {
                        loop {
                            let crv = m.u32(geo.wrapping_add(0x80)).wrapping_add((j as u32).wrapping_mul(0x64));
                            let mut p = 0;
                            let mut off = 0u32;
                            if p < m.i32(crv.wrapping_add(4)) && i < n2 {
                                loop {
                                    if idx(m, i) == j && idx(m, i + 1) == p {
                                        for c in 0..3u32 {
                                            let v = F80::from(m.f32(from.wrapping_add(4 * c))).to_f64();
                                            let pts = m.u32(crv.wrapping_add(0x2c));
                                            m.set_f64(pts.wrapping_add(off).wrapping_add(8 * c), v);
                                        }
                                        let pts = m.u32(crv.wrapping_add(0x2c));
                                        m.set_u32(pts.wrapping_add(off).wrapping_add(0x18), 0);
                                        m.set_u32(pts.wrapping_add(off).wrapping_add(0x1c), 0x3ff0_0000);
                                        from = from.wrapping_add(12);
                                        i += 1;
                                    }
                                    off = off.wrapping_add(0x24);
                                    p += 1;
                                    if p >= m.i32(crv.wrapping_add(4)) || i >= n2 {
                                        break;
                                    }
                                }
                            }
                            j += 1;
                            if j >= nh(m) || i >= n2 {
                                break;
                            }
                        }
                    }
                }
            }
            6 => {
                let mut from = m[cur].points.addr();
                let mut key = m.u32(geo.wrapping_add(0x14));
                let (mut i, mut p) = (0, 0);
                if m.u16(geo.wrapping_add(0x10)) != 0 && i < n && key != 0 {
                    loop {
                        if idx(m, i) == p {
                            copy(m, from, key, 9);
                            from = from.wrapping_add(36);
                            i += 1;
                        }
                        p += 1;
                        key = key.wrapping_add(0x30);
                        if p >= m.u16(geo.wrapping_add(0x10)) as i32 || i >= n || key == 0 {
                            break;
                        }
                    }
                }
            }
            _ => {
                // 9 and 10: NURBS curve (points +0x2c, count +4) and surface (+0x54, +0x48)
                let surface = kind == 10;
                let (pts_at, count_at) = if surface { (0x54, 0x48) } else { (0x2c, 4) };
                let mut from = m[cur].points.addr();
                let mut at = m.u32(geo.wrapping_add(pts_at));
                let mut off = 0u32;
                let (mut i, mut p) = (0, 0);
                if p < m.i32(geo.wrapping_add(count_at)) && i < n && at != 0 {
                    loop {
                        if idx(m, i) == p {
                            let d: [f64; 4] = core::array::from_fn(|c| F80::from(m.f32(from.wrapping_add(4 * c as u32))).to_f64());
                            let w = |v: f64| out::dw(v);
                            if surface {
                                let a: Vec<u32> = [geo, p as u32].into_iter().chain(w(d[0])).chain(w(d[1])).chain(w(d[2])).chain(w(d[3])).collect();
                                o.call(m, &out::SI_NURBS_SRF_MOVE_POINT, &a);
                            } else {
                                let pts = m.u32(geo.wrapping_add(0x2c));
                                m.set_f64(pts.wrapping_add(off).wrapping_add(0x18), d[3]);
                                let a: Vec<u32> = [geo, p as u32].into_iter().chain(w(d[0])).chain(w(d[1])).chain(w(d[2])).collect();
                                o.call(m, &out::SI_NURBS_CRV_MOVE_POINT, &a);
                            }
                            i += 1;
                            from = from.wrapping_add(16);
                        }
                        if !surface {
                            off = off.wrapping_add(0x24);
                        }
                        p += 1;
                        at = at.wrapping_add(0x24);
                        if p >= m.i32(geo.wrapping_add(count_at)) || i >= n || at == 0 {
                            break;
                        }
                    }
                }
            }
        }
    }
    let t = m.i32(ma.wrapping_add(0x31c));
    if (t.wrapping_sub(5) as u32) <= 1 || t == 8 {
        o.call(m, &out::SI_ENVELOP_COMP_UPDATE, &[ma]);
    }
    1
}

// ---------------------------------------------------------------- second pass: the points of the keys

/// Bytes of one point's position in a cluster key by the model's kind, as the vertex
/// routines size it: 12 for a mesh, 16 for a patch or NURBS curve or surface, 36 for a
/// spline, and for a face 36 when its working definition is a face of splines (+0x84 not
/// 0), 12 when it is a face of NURBS curves; none for another kind.
fn vertex_size(m: &Mem, model: u32) -> Option<u32> {
    match m.i32(model.wrapping_add(0x30)) {
        4 => Some(12),
        1 | 9 | 10 => Some(16),
        2 => Some(if m.u16(m.u32(model.wrapping_add(0x48)).wrapping_add(0x84)) != 0 { 36 } else { 12 }),
        6 => Some(36),
        _ => None,
    }
}

/// The checks the vertex routines share: the cluster's key data with a chain of keys and
/// the number of points; `None` when there are none.
fn vertex_keys(m: &Mem, k: Ptr32<Kluster>) -> Option<(Ptr32<ClusterKey>, i32)> {
    if k.is_null() {
        return None;
    }
    let d = m[k].anim;
    if d.is_null() || m[d].keys.is_null() {
        return None;
    }
    Some((m[d].keys, m[k].count))
}

fn copy_words(m: &mut Mem, from: u32, to: u32, bytes: u32) {
    for w in (0..bytes).step_by(4) {
        let v = m.u32(from.wrapping_add(w));
        m.set_u32(to.wrapping_add(w), v);
    }
}

/// [soft 0x081f09b4 SI_clusterAnimRemoveVertex]
///
/// Takes point `i` out of every key of a cluster: the positions after it moved down by one.
/// The original moves `count − i` positions, the last from one place past the cluster's
/// count (memory the key may not have). Nothing for `i` outside 0 … count − 1 or a kind
/// without positions.
pub fn cluster_anim_remove_vertex(m: &mut Mem, model: Ptr32<Model>, k: Ptr32<Kluster>, i: i32) {
    let Some((mut key, count)) = vertex_keys(m, k) else { return };
    if i < 0 || count <= 0 || i >= count {
        return;
    }
    let Some(size) = vertex_size(m, model.addr()) else { return };
    while !key.is_null() {
        let mut p = m[key].points.addr().wrapping_add((i as u32).wrapping_mul(size));
        for _ in 0..count - i {
            copy_words(m, p.wrapping_add(size), p, size);
            p = p.wrapping_add(size);
        }
        key = m[key].next;
    }
}

/// [soft 0x081f0c24 SI_clusterAnimSwapVertices]
///
/// Exchanges points `a` and `b` in every key of a cluster (a face of splines' positions
/// as three exchanges of 12 bytes). Nothing for a point outside 0 … count − 1.
pub fn cluster_anim_swap_vertices(m: &mut Mem, model: Ptr32<Model>, k: Ptr32<Kluster>, a: i32, b: i32) {
    let Some((mut key, count)) = vertex_keys(m, k) else { return };
    if a < 0 || b < 0 || count <= 0 || a >= count || b >= count {
        return;
    }
    let Some(size) = vertex_size(m, model.addr()) else { return };
    let chunk = if size == 36 { 12 } else { size };
    let tmp = m.local(chunk);
    while !key.is_null() {
        let p = m[key].points.addr();
        let (pa, pb) = (p.wrapping_add((a as u32).wrapping_mul(size)), p.wrapping_add((b as u32).wrapping_mul(size)));
        let mut c = 0;
        while c < size {
            copy_words(m, pa.wrapping_add(c), tmp, chunk);
            copy_words(m, pb.wrapping_add(c), pa.wrapping_add(c), chunk);
            copy_words(m, tmp, pb.wrapping_add(c), chunk);
            c += chunk;
        }
        key = m[key].next;
    }
    m.release(chunk);
}

/// [soft 0x081f1a1c SI_clusterModelUpdate]
///
/// Gives a model its cluster animation record when it has none (`SI_clusterAllocate`,
/// turned on), and makes the model the owner (word at +4) of every key curve (type 0x2bd)
/// and weight curve (0x2be, one per key, the list read without testing its end) of its
/// clusters with key data.
pub fn cluster_model_update(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) {
    if model.is_null() || !m[model].cluster.is_null() {
        return;
    }
    let c = cluster_allocate(m);
    m.set_u32(model.addr() + 0x344, c.addr());
    if c.is_null() {
        return;
    }
    m[c].active = 1;
    let mut k: Ptr32<Kluster> = Ptr32::new(m[model].klusters.addr());
    let index = m.local(4);
    while !k.is_null() {
        let ce = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[k.addr(), index]);
        let f = o.call(m, &out::SI_ED_FIND_FCV, &[ce, 2, 0x2bd]);
        let d = m[k].anim;
        if !d.is_null() {
            if f != 0 {
                m.set_u32(f.wrapping_add(4), model.addr());
            }
            let mut g = o.call(m, &out::SI_ED_FIND_FCV, &[ce, 2, 0x2be]);
            if g != 0 {
                let mut i = 0;
                while i < m[d].nb_keys {
                    m.set_u32(g.wrapping_add(4), model.addr());
                    g = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[ce, 2, 0x2be]);
                    i += 1;
                }
            }
        }
        k = m[k].next;
    }
    m.release(4);
}

/// [soft 0x081f1b00 SI_clusterAnimCvtToFace]
///
/// When a model's working definition became a face of NURBS curves (+0x84 of the face 0),
/// a cluster's keys of homogeneous points (16 bytes) are replaced by keys of the face's
/// kind (`SI_clusterKeyAllocate`): every point (x/w, y/w, z/w, the reciprocal of w in the
/// register, each product rounded) three times; each old key freed after its copy, the
/// first new key made current. When an allocation fails the conversion stops (the rest of
/// the old chain is lost).
pub fn cluster_anim_cvt_to_face(m: &mut Mem, model: Ptr32<Model>, k: Ptr32<Kluster>) {
    if k.is_null() {
        return;
    }
    let d = m[k].anim;
    if d.is_null() || m.u16(m.u32(model.addr() + 0x48).wrapping_add(0x84)) != 0 {
        return;
    }
    let nk = m[d].nb_keys;
    let mut old = m[d].keys;
    m[d].keys = Ptr32::NULL;
    let mut last: Ptr32<ClusterKey> = Ptr32::NULL;
    let n = m[k].count;
    let slot: Ptr32<Ptr32<ClusterKey>> = Ptr32::new(m.local(4));
    let mut i = 0;
    while i < nk {
        let kind = m.i32(model.addr() + 0x30);
        let key = cluster_key_allocate(m, n, kind);
        if key.is_null() {
            break;
        }
        if m[d].keys.is_null() {
            m[d].keys = key;
        } else {
            m[last].next = key;
        }
        last = key;
        m[key].next = Ptr32::NULL;
        i += 1;
        let (mut to, mut from) = (m[key].points.addr(), m[old].points.addr());
        for _ in 0..n.max(0) {
            let inv = F80::ONE / m.f32(from.wrapping_add(12));
            for c in 0..3u32 {
                m.set_f32(to.wrapping_add(4 * c), (inv * m.f32(from.wrapping_add(4 * c))).to_f32());
            }
            from = from.wrapping_add(16);
            copy_words(m, to, to.wrapping_add(12), 12);
            copy_words(m, to, to.wrapping_add(24), 12);
            to = to.wrapping_add(36);
        }
        m[slot] = old;
        old = m[old].next;
        cluster_key_dispose(m, slot);
    }
    m.release(4);
    m[d].current = m[d].keys;
}

/// [soft 0x081f1c50 SI_clusterAnimApplyTransfo]
///
/// Moves every key of a cluster by a matrix: homogeneous points for a patch or NURBS
/// curve or surface (`SI_hvectorMultiplyMatrix`), vectors for a mesh, a face of NURBS
/// curves (count) or a spline or face of splines (three a point; `SI_vectorMultiplyMatrix`),
/// in place, the count as a `short`. The chain is followed for the number of keys without
/// testing its end.
pub fn cluster_anim_apply_transfo(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, matrix: u32, k: Ptr32<Kluster>) {
    if k.is_null() || m[k].anim.is_null() {
        return;
    }
    let d = m[k].anim;
    let nk = m[d].nb_keys;
    let mut key = m[d].keys;
    let count = m[k].count;
    let mut i = 0;
    while i < nk {
        let ma = model.addr();
        let call = match m.i32(ma + 0x30) {
            1 | 9 | 10 => Some((&out::SI_HVECTOR_MULTIPLY_MATRIX, count)),
            4 => Some((&out::SI_VECTOR_MULTIPLY_MATRIX, count)),
            2 => Some((&out::SI_VECTOR_MULTIPLY_MATRIX, if m.u16(m.u32(ma + 0x48).wrapping_add(0x84)) != 0 { count.wrapping_mul(3) } else { count })),
            6 => Some((&out::SI_VECTOR_MULTIPLY_MATRIX, count.wrapping_mul(3))),
            _ => None,
        };
        if let Some((r, n)) = call {
            let p = m[key].points.addr();
            o.call(m, r, &[p, matrix, p, n as u16 as u32]);
        }
        key = m[key].next;
        i += 1;
    }
}

/// The message both vertex routines show for a kind of geometry they cannot add a point
/// to: "Unable to add those type of vertices..." (two copies of the text in the binary's
/// data, at 0x08694ba0 for a face without three vectors a point and 0x08694be0 otherwise).
const MSG_ADD_FACE: u32 = 0x08694ba0;
const MSG_ADD_KIND: u32 = 0x08694be0;

/// [soft 0x081f0fac SI_clusterAnimAddVertex]
///
/// Puts point `i` of a cluster into every key: the cluster's count already holds it, so
/// each key's positions are made room for (`SI_realloc` to count × 16 bytes for a mesh,
/// patch or NURBS, count × 48 for a face or spline), those from `i` on moved up one (the
/// last one dropped) and the point's position written at `i`: the working geometry's
/// (definition 0) position of point number `indexes[i]` when that number lies within it,
/// else zeros. A mesh's vertex (20 bytes a vertex) gives x, y, z; a patch's control point
/// four floats; a NURBS point its four doubles rounded to floats; a spline key its three
/// vectors. For a face the curve is `indexes[i]` (−1 the outline at +4, else a hole of
/// the array at +0x7c, 0x74 bytes each, below the count at +0x78) and the key
/// `indexes[i + 1]`; a face without the flag at +0x84 gets the message and zeros; a curve
/// number beyond the face's is read as a spline from the definition itself. Other kinds:
/// the message, nothing else.
pub fn cluster_anim_add_vertex(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, k: Ptr32<Kluster>, i: i32) {
    let Some((mut key, n)) = vertex_keys(m, k) else { return };
    if i < 0 || n <= 0 || i >= n {
        return;
    }
    let mdl = model.addr();
    let def = m.u32(m.u32(mdl.wrapping_add(0x38)));
    let index = |m: &Mem, j: i32| m.i32(m.u32(k.addr().wrapping_add(0xc)).wrapping_add((j as u32).wrapping_mul(4)));
    let mut pos = [0u32; 9];
    let words = |m: &Mem, p: u32, pos: &mut [u32; 9], n: usize| {
        for (w, v) in pos.iter_mut().take(n).enumerate() {
            *v = m.u32(p.wrapping_add(4 * w as u32));
        }
    };
    let doubles = |m: &Mem, p: u32, pos: &mut [u32; 9]| {
        for (w, v) in pos.iter_mut().take(4).enumerate() {
            *v = x(m.f64(p.wrapping_add(8 * w as u32))).to_f32().to_bits();
        }
    };
    let (size, alloc) = match m.i32(mdl.wrapping_add(0x30)) {
        4 => {
            if def != 0 {
                let p = index(m, i);
                if p < m.i32(def.wrapping_add(0x14)) {
                    let v = m.u32(def).wrapping_add((p as u32).wrapping_mul(20));
                    words(m, v, &mut pos, 3);
                }
            }
            (12u32, (n as u32).wrapping_mul(16))
        }
        1 => {
            if def != 0 {
                let p = index(m, i);
                let total = i32::from(m.u16(def.wrapping_add(0xc))) * i32::from(m.u16(def.wrapping_add(0xe)));
                if p < total {
                    let v = m.u32(def.wrapping_add(0x1a4)).wrapping_add((p as u32).wrapping_mul(16));
                    words(m, v, &mut pos, 4);
                }
            }
            (16, (n as u32).wrapping_mul(16))
        }
        kind @ (2 | 6) => {
            let mut src = 0u32;
            let mut as_spline = true;
            if kind == 2 && def != 0 {
                let c = index(m, i);
                if c < i32::from(m.i16(def.wrapping_add(0x78))) {
                    as_spline = false;
                    if m.u16(def.wrapping_add(0x84)) == 0 {
                        o.call(m, &out::SI_STATUS_DISPLAY_MESSAGE, &[MSG_ADD_FACE, 2]);
                    } else {
                        let sp = if c == -1 { def.wrapping_add(4) } else { m.u32(def.wrapping_add(0x7c)).wrapping_add((c as u32).wrapping_mul(0x74)) };
                        if sp != 0 {
                            let p = index(m, i.wrapping_add(1));
                            if p < i32::from(m.u16(sp.wrapping_add(0x10))) {
                                src = m.u32(sp.wrapping_add(0x14)).wrapping_add((p as u32).wrapping_mul(0x30));
                            }
                        }
                    }
                }
            }
            if as_spline && def != 0 {
                let p = index(m, i);
                if p < i32::from(m.u16(def.wrapping_add(0x10))) {
                    src = m.u32(def.wrapping_add(0x14)).wrapping_add((p as u32).wrapping_mul(0x30));
                }
            }
            if src != 0 {
                words(m, src, &mut pos, 9);
            }
            (36, (n as u32).wrapping_mul(48))
        }
        9 => {
            if def != 0 {
                let p = index(m, i);
                if p < m.i32(def.wrapping_add(4)) {
                    let v = m.u32(def.wrapping_add(0x2c)).wrapping_add((p as u32).wrapping_mul(36));
                    doubles(m, v, &mut pos);
                }
            }
            (16, (n as u32).wrapping_mul(16))
        }
        10 => {
            if def != 0 {
                let p = index(m, i);
                if p < m.i32(def.wrapping_add(0x48)) {
                    let v = m.u32(def.wrapping_add(0x54)).wrapping_add((p as u32).wrapping_mul(36));
                    doubles(m, v, &mut pos);
                }
            }
            (16, (n as u32).wrapping_mul(16))
        }
        _ => {
            o.call(m, &out::SI_STATUS_DISPLAY_MESSAGE, &[MSG_ADD_KIND, 2]);
            return;
        }
    };
    while !key.is_null() {
        let old = m[key].points.addr();
        let p = m.si_realloc(old, alloc);
        m.set_u32(key.addr(), p);
        let mut c = n - 1;
        let mut at = p.wrapping_add((c as u32).wrapping_mul(size));
        while c > i {
            copy_words(m, at.wrapping_sub(size), at, size);
            at = at.wrapping_sub(size);
            c -= 1;
        }
        for (w, v) in pos.iter().take(size as usize / 4).enumerate() {
            m.set_u32(at.wrapping_add(4 * w as u32), *v);
        }
        key = m[key].next;
    }
}

/// [soft 0x081f1680 SI_clusterAnimDuplicate]
///
/// The cluster animation of `src` for its copy `dst` on a duplicated model: new key data
/// (`SI_clusterDataAllocate`, then the source's 0x1c bytes copied over it), a copy of the
/// key curve (type 0x2bd → a new curve of type 0xb7) and of each weight curve (0x2be → 0xb8,
/// one per key, `SI_fcurveTransfer`) added to the copy's element in chapter `SI_cid[2]`
/// (`SI_edFcvAdd`) and renamed (`SI_updateWeightFcvNames`), and new keys
/// (`SI_clusterKeyAllocate`) holding the source keys' positions, moved by the model's
/// matrix at +0x1c4 when `xform` is 1 (`SI_vectorMultiplyMatrix`, or
/// `SI_hvectorMultiplyMatrix` for homogeneous points). The new data's current key is its
/// first. 0 without source data, memory, or either element (the new data is then lost).
/// The source chain is followed for the number of keys without a test of its end.
pub fn cluster_anim_duplicate(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, dst: Ptr32<Kluster>, src: Ptr32<Kluster>, xform: i16) -> Ptr32<ClusterData> {
    if src.is_null() {
        return Ptr32::NULL;
    }
    let sd = m[src].anim;
    if sd.is_null() {
        return Ptr32::NULL;
    }
    let nd = cluster_data_allocate(m);
    if nd.is_null() {
        return Ptr32::NULL;
    }
    m.copy(nd.addr(), sd.addr(), 0x1c);
    let l4 = m.local(4);
    let es = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[src.addr(), l4]);
    let ed = o.call(m, &out::SI_ED_GET_ELEM_FROM_CLUSTER, &[dst.addr(), l4]);
    if es == 0 || ed == 0 {
        m.release(4);
        return Ptr32::NULL;
    }
    let add = |m: &mut Mem, o: &mut dyn Outside, from: u32, kind: u32| {
        let c = o.call(m, &out::SI_FCURVE_ALLOCATE, &[kind, 0]);
        o.call(m, &out::SI_FCURVE_TRANSFER, &[from, c]);
        let (c2, idx) = (m[crate::globals::SI_CID][2] as u32, m.u32(l4));
        crate::edanim::ed_fcv_add(m, o, c, ed, c2, idx);
    };
    let f = o.call(m, &out::SI_ED_FIND_FCV, &[es, 2, 0x2bd]);
    add(m, o, f, 0xb7);
    let mut f = o.call(m, &out::SI_ED_FIND_FCV, &[es, 2, 0x2be]);
    let n = m[sd].nb_keys;
    let mut left = n;
    while left > 0 {
        add(m, o, f, 0xb8);
        f = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[es, 2, 0x2be]);
        left -= 1;
    }
    o.call(m, &out::SI_UPDATE_WEIGHT_FCV_NAMES, &[ed, 2, 0x2be]);
    let mut sk = m[sd].keys;
    m[nd].keys = Ptr32::NULL;
    let mut prev: Ptr32<ClusterKey> = Ptr32::NULL;
    let count = m[src].count;
    let mdl = model.addr();
    let mut i = 0;
    while i < n {
        let kind = m.i32(mdl.wrapping_add(0x30));
        let key = cluster_key_allocate(m, count, kind);
        if key.is_null() {
            break;
        }
        if m[nd].keys.is_null() {
            m[nd].keys = key;
        } else {
            m[prev].next = key;
        }
        prev = key;
        m[key].next = Ptr32::NULL;
        let matrix = mdl.wrapping_add(0x1c4);
        let copy = |m: &mut Mem, o: &mut dyn Outside, size: u32, many: i32, homogeneous: bool| {
            let (to, from) = (m[key].points.addr(), m[sk].points.addr());
            if many > 0 {
                copy_words(m, from, to, size.wrapping_mul(many as u32));
            }
            if xform == 1 {
                let r = if homogeneous { &out::SI_HVECTOR_MULTIPLY_MATRIX } else { &out::SI_VECTOR_MULTIPLY_MATRIX };
                let p = m[key].points.addr();
                o.call(m, r, &[p, matrix, p, many as u32 & 0xffff]);
            }
        };
        match m.i32(mdl.wrapping_add(0x30)) {
            4 => copy(m, o, 12, count, false),
            1 | 9 | 10 => copy(m, o, 16, count, true),
            2 => {
                if m.u16(m.u32(mdl.wrapping_add(0x48)).wrapping_add(0x84)) != 0 {
                    copy(m, o, 12, count.wrapping_mul(3), false);
                } else {
                    copy(m, o, 12, count, false);
                }
            }
            6 => copy(m, o, 12, count.wrapping_mul(3), false),
            _ => {}
        }
        sk = m[sk].next;
        i += 1;
    }
    m[nd].current = m[nd].keys;
    m.release(4);
    nd
}

/// `%f` of a `double` as the C library prints it (six decimals; `nan`/`inf` with their sign).
fn c_f(v: f64) -> String {
    if v.is_nan() {
        if v.is_sign_negative() { "-nan" } else { "nan" }.to_string()
    } else if v.is_infinite() {
        if v < 0.0 { "-inf" } else { "inf" }.to_string()
    } else {
        format!("{:.6}", v)
    }
}

/// [soft 0x081f1524 clusterAnimDump]
///
/// The debugging printout of the clusters' animation (no caller): for every cluster of the
/// chain starting at `k`, print the point numbers (`count` of them, twice as many when
/// `faces` is not 0) as ` index[i] n`, then for every key of the cluster's data
/// `key (n) address: a` and its positions: four floats a point when `kind` is 1, three
/// otherwise (`count × 3` points when `kind` is 3). The key's address is printed as the
/// 32-bit value it has in the image. The original's `printf` is the C library's: its text
/// goes to [`Outside::print`]. A cluster without data is a crash in the original (it reads
/// through the null `anim`). Answers 1.
pub fn cluster_anim_dump(m: &mut Mem, o: &mut dyn Outside, k: Ptr32<Kluster>, kind: i32, faces: i32) -> i32 {
    let mut k = k;
    while !k.is_null() {
        let mut key = m[m[k].anim].keys;
        let mut n = m[k].count;
        let idx = m[k].indexes.addr();
        let total = if faces != 0 { n.wrapping_add(n) } else { n };
        let mut i = 0i32;
        while i < total {
            let v = m.u32(idx.wrapping_add((i as u32).wrapping_mul(4))) as i32;
            o.print(format!(" index[{}] {}\n", i, v).as_bytes());
            i += 1;
        }
        if kind == 3 {
            n = n.wrapping_mul(3);
        }
        let mut num = 0i32;
        while !key.is_null() {
            o.print(format!("key ({}) address: {}\n", num, key.addr() as i32).as_bytes());
            num += 1;
            let (per, stride) = if kind == 1 { (4u32, 0x10u32) } else { (3, 0xc) };
            let mut p = m[key].points.addr();
            let mut c = n;
            while c > 0 {
                let mut line = String::new();
                for j in 0..per {
                    if j > 0 {
                        line.push(' ');
                    }
                    line.push_str(&c_f(f32::from_bits(m.u32(p.wrapping_add(4 * j))) as f64));
                }
                line.push('\n');
                o.print(line.as_bytes());
                p = p.wrapping_add(stride);
                c -= 1;
            }
            key = m[key].next;
        }
        k = m[k].next;
    }
    1
}
