// SPDX-License-Identifier: MIT
//! The chunks that hang on a node, on the image: clusters, envelopes with their vertex
//! lists, the inverse kinematics chunk, the dynamics of matter and of a joint (finding
//! 0004, "Dynamics, joints, envelopes, clusters"), with the allocators they call from
//! `klusters.c`, `envtools.c`, `invkin.c`, `settings.c` and `matrix_gen_dbl.c`.
//!
//! `si-types` has none of these records; their fields are addressed by the offsets of
//! [`envelop`], [`invkin`], [`dyna`] and [`kluster`] below, each read from the listing.
//! `si-kine`, `si-rensetup` and `si-math` have some of the allocators in their own forms;
//! they depend on this crate or are not on the image, so the allocators are ported here
//! from the listing (the lead reconciles duplicates).

use super::globals::{FILE_VERSION, SI_IDENTITE, SI_MATRIX3_ID};
use super::io::{alloc_fits, byteswap_fread, file_skip_field, float_fread, matrix_fread, string_binary_read, vector_fread};
use super::matio::next_token;
use super::node::{kluster_allocate, node};
use super::outside::{Outside, Routine};
use super::proto::{slot, Prototypes};
use super::texture::matrix_copy;
use si_mem::{Mem, Ptr32};

/// Offsets in an envelope (`SI_EnvelopInfo`, 0xa0 bytes), in one of its lists (0x10) and
/// in a vertex of a list (0x14).
pub mod envelop {
    pub const LEVEL: u32 = 0x00;
    /// The first of the envelope's lists.
    pub const LISTS: u32 = 0x04;
    pub const MATRIX: u32 = 0x08;
    pub const JOINT_AREA: u32 = 0x6c;
    /// The inner curve: type, the curve's floats in a block of their own, then begin,
    /// end, amplitude, scale.
    pub const INNER_TYPE: u32 = 0x70;
    pub const INNER_CURVE: u32 = 0x74;
    pub const INNER: u32 = 0x78;
    /// The outer curve, the same six.
    pub const OUTER_TYPE: u32 = 0x88;
    pub const OUTER_CURVE: u32 = 0x8c;
    pub const OUTER: u32 = 0x90;
    pub const SIZE: u32 = 0xa0;
    /// A list: the model it belongs to, its first vertex, the list before it and the one
    /// after it.
    pub const LIST_MODEL: u32 = 0x00;
    pub const LIST_FIRST: u32 = 0x04;
    pub const LIST_PREVIOUS: u32 = 0x08;
    pub const LIST_NEXT: u32 = 0x0c;
    pub const LIST_SIZE: u32 = 0x10;
    /// A vertex: its index in the geometry, ratio, thickness, weight (100 in a new one)
    /// and the vertex after it.
    pub const VERT_INDEX: u32 = 0x00;
    pub const VERT_RATIO: u32 = 0x04;
    pub const VERT_THICKNESS: u32 = 0x08;
    pub const VERT_WEIGHT: u32 = 0x0c;
    pub const VERT_NEXT: u32 = 0x10;
    pub const VERT_SIZE: u32 = 0x14;
}

/// Offsets in an inverse kinematics record (`SI_InvKin`, 0x74 bytes) and in a matrix of
/// doubles (0xc bytes: rows, columns, an array of row pointers).
pub mod invkin {
    /// The type: 10, 20 or 30 for a chain of one, two or more joints, 40 for the other
    /// kind of chain.
    pub const TYPE: u32 = 0x00;
    pub const NB_JOINTS: u32 = 0x04;
    pub const ROTATION: u32 = 0x20;
    /// Two matrices of doubles, made for a chain of three joints or more.
    pub const MATRIX_A: u32 = 0x2c;
    pub const MATRIX_B: u32 = 0x30;
    /// A short: 1 in a record read from a file, 0 after the matrices were (re)made.
    pub const FLAG: u32 = 0x34;
    pub const SIZE: u32 = 0x74;
}

/// Offsets in the dynamics of matter (`__SI_DynaMatterInfo`, 0x84 bytes) and of a joint
/// (`__SI_DynaJntInfo`, 0xc bytes: the friction vector).
pub mod dyna {
    pub const DENSITY: u32 = 0x00;
    pub const ROUGHNESS: u32 = 0x0c;
    pub const KINETIC_ROUGHNESS: u32 = 0x10;
    pub const ELASTICITY: u32 = 0x14;
    pub const RIGIDITY: u32 = 0x18;
    pub const PLASTICITY: u32 = 0x1c;
    /// Two 3 × 3 matrices, the identity in a new record.
    pub const MATRIX_A: u32 = 0x2c;
    pub const MATRIX_B: u32 = 0x50;
    pub const COLLISION: u32 = 0x80;
    pub const SIZE: u32 = 0x84;
    pub const JOINT_SIZE: u32 = 0x0c;
}

/// Offsets in a cluster (`kluster`, 0x20 bytes; the allocator is in `node.rs`).
pub mod kluster {
    pub const NAME: u32 = 0x00;
    pub const COUNT: u32 = 0x04;
    pub const INDICES: u32 = 0x0c;
    pub const STATE: u32 = 0x10;
    /// The cluster after it in the node's list.
    pub const NEXT: u32 = 0x1c;
}

/// Further offsets in a node (see `node::node`).
pub mod more {
    /// The friction of a joint (`__SI_DynaJntInfo`) and the working state of its dynamics.
    pub const DYNA_JOINT: u32 = 0x2f8;
    pub const DYNA_JOINT_STATE: u32 = 0x300;
    /// The first cluster of the node's list.
    pub const CLUSTERS: u32 = 0x348;
}

/// `SI_edAddCluster (0x08103510)`, `ed_kluster.c`: tells the cluster editor of a new cluster.
pub const SI_ED_ADD_CLUSTER: Routine = Routine { name: "SI_edAddCluster", addr: 0x08103510 };
/// `SI_dynajntvarAllocate (0x0849fa88)`, `settings.c`: the working state of a joint's dynamics.
pub const SI_DYNAJNTVAR_ALLOCATE: Routine = Routine { name: "SI_dynajntvarAllocate", addr: 0x0849fa88 };

/// [soft 0x0851b088 SI_matrix3Copy]: nine floats from `src` to `dst`, each through the
/// x87 register (a signalling NaN would come out quiet; the callers here copy the
/// identity).
pub fn matrix3_copy(m: &mut Mem, src: u32, dst: u32) {
    for k in 0..9 {
        let v = m.f32(src.wrapping_add(4 * k));
        m.set_f32(dst.wrapping_add(4 * k), v);
    }
}

/// [soft 0x0849f19c SI_dynamatterSetDefault]: density 0.2, roughness 0.9, kinetic
/// roughness 0.3, elasticity 0.6 (the floats at 0x08660b40), rigidity 1, plasticity 0, a
/// zero at +8, collision level −2.
pub fn dynamatter_set_default(m: &mut Mem, p: u32) {
    m.set_f32(p + dyna::DENSITY, 0.2);
    m.set_u32(p + 8, 0);
    m.set_f32(p + dyna::ROUGHNESS, 0.9);
    m.set_f32(p + dyna::KINETIC_ROUGHNESS, 0.3);
    m.set_f32(p + dyna::ELASTICITY, 0.6);
    m.set_f32(p + dyna::RIGIDITY, 1.0);
    m.set_u32(p + dyna::PLASTICITY, 0);
    m.set_i32(p + dyna::COLLISION, -2);
}

/// [soft 0x0849f1e8 dynageometrySetDefault]: zeros at +4 and +0x20 to +0x2b, and the two
/// 3 × 3 matrices the identity (`SI_matrix3ID`).
pub fn dynageometry_set_default(m: &mut Mem, p: u32) {
    for off in [0x04, 0x28, 0x24, 0x20] {
        m.set_u32(p + off, 0);
    }
    matrix3_copy(m, SI_MATRIX3_ID.addr(), p + dyna::MATRIX_A);
    matrix3_copy(m, SI_MATRIX3_ID.addr(), p + dyna::MATRIX_B);
}

/// [soft 0x0849fa24 SI_dynamatterAllocate]: a new record with both sets of defaults. The
/// twelve bytes from +0x74 are not written.
pub fn dynamatter_allocate(m: &mut Mem) -> u32 {
    let p = m.si_malloc(dyna::SIZE);
    if p != 0 {
        dynamatter_set_default(m, p);
        dynageometry_set_default(m, p);
    }
    p
}

/// [soft 0x0849f238 SI_dynajntpropSetDefault]: the friction 60, 15, 15 (the floats at
/// 0x08660b50).
pub fn dynajntprop_set_default(m: &mut Mem, p: u32) {
    m.set_f32(p, 60.0);
    m.set_f32(p + 4, 15.0);
    m.set_f32(p + 8, 15.0);
}

/// [soft 0x0849fa5c SI_dynajntpropAllocate]: a new friction vector with its defaults.
pub fn dynajntprop_allocate(m: &mut Mem) -> u32 {
    let p = m.si_malloc(dyna::JOINT_SIZE);
    if p != 0 {
        dynajntprop_set_default(m, p);
    }
    p
}

/// [soft 0x083f1674 dynamatterReadBinaryChunk]: the dynamics of a node's matter: six
/// floats (tokens 1 to 6: density, roughness, elasticity, rigidity, plasticity, kinetic
/// roughness) and the collision level (7, an int). Null if the record cannot be allocated
/// (the original then prints a line).
pub fn dynamatter_read_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>) -> u32 {
    let p = dynamatter_allocate(m);
    if p == 0 {
        return 0;
    }
    const FRAME: u32 = 4;
    let token_at = m.local(FRAME);
    let list_at = protos.addr().wrapping_add(4 * slot::DYNA_MATTER as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        let off = match m.i16(token_at) {
            1 => Some(dyna::DENSITY),
            2 => Some(dyna::ROUGHNESS),
            3 => Some(dyna::ELASTICITY),
            4 => Some(dyna::RIGIDITY),
            5 => Some(dyna::PLASTICITY),
            6 => Some(dyna::KINETIC_ROUGHNESS),
            _ => None,
        };
        match (m.i16(token_at), off) {
            (_, Some(off)) => {
                float_fread(m, o, p + off, 4, 1, file);
            }
            (7, _) => {
                byteswap_fread(m, o, p + dyna::COLLISION, 4, 1, file);
            }
            (t, _) => {
                file_skip_field(m, o, file, t);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    m.release(FRAME);
    p
}

/// [soft 0x083f0b08 dynajntpropReadBinaryChunk]: the friction of a joint, stored in the
/// node at +0x2f8: a new vector with its defaults, the vector of token 1, and then the
/// working state of the joint's dynamics (`SI_dynajntvarAllocate`, at +0x300) and the
/// node's kinematic state (`SI_modelKineStateAllocate`, at +0x2f0). If the vector cannot
/// be allocated the node gets null and nothing is read (the original prints a line).
pub fn dynajntprop_read_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, model: u32, protos: Ptr32<Prototypes>) {
    let p = dynajntprop_allocate(m);
    m.set_u32(model.wrapping_add(more::DYNA_JOINT), p);
    if p == 0 {
        return;
    }
    const FRAME: u32 = 4;
    let token_at = m.local(FRAME);
    let list_at = protos.addr().wrapping_add(4 * slot::JOINT as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        match m.i16(token_at) {
            1 => {
                vector_fread(m, o, p, 0xc, 1, file);
            }
            t => {
                file_skip_field(m, o, file, t);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    let state = o.call(m, &SI_DYNAJNTVAR_ALLOCATE, &[]);
    m.set_u32(model.wrapping_add(more::DYNA_JOINT_STATE), state);
    let kine = o.call(m, &super::node::SI_MODEL_KINE_STATE_ALLOCATE, &[]);
    m.set_u32(model.wrapping_add(node::KINE_STATE), kine);
    m.release(FRAME);
}

/// [soft 0x0843ab28 SI_klusterAddToList]: a new cluster with the name `name` (the pointer
/// is kept, not a copy) at the end of the node's list of clusters; with `tell` set the
/// cluster editor is told (`SI_edAddCluster`). Null for no node.
pub fn kluster_add_to_list(m: &mut Mem, o: &mut dyn Outside, model: u32, name: u32, tell: i16) -> u32 {
    if model == 0 {
        return 0;
    }
    let k = kluster_allocate(m);
    m.set_u32(k.wrapping_add(kluster::NEXT), 0);
    m.set_u32(k, name);
    let mut last = m.u32(model + more::CLUSTERS);
    if last == 0 {
        m.set_u32(model + more::CLUSTERS, k);
    } else {
        while m.u32(last + kluster::NEXT) != 0 {
            last = m.u32(last + kluster::NEXT);
        }
        m.set_u32(last + kluster::NEXT, k);
    }
    if tell != 0 {
        o.call(m, &SI_ED_ADD_CLUSTER, &[model, k]);
    }
    k
}

/// [soft 0x083f0be4 clustersReadBinaryChunk]: one cluster of a node.
///
/// Token 1 is the name: a new cluster with it is appended to the node's list
/// ([`kluster_add_to_list`]). Token 2 is the count, token 4 the state: each is read into a
/// local and stored only if there is a cluster. Token 3 is the indices: as many ints as
/// the cluster's count says (twice as many when the node is a face) in a new block, read
/// one by one; the block is kept only if there is a cluster and the allocation succeeded.
///
/// **The cluster is a register the routine never sets before token 1**: for a list in
/// which another token comes first, the original works on whatever its caller left in
/// `edi`. The port keeps it in a local of the image's stack that it does not set either.
pub fn clusters_read_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, model: u32, protos: Ptr32<Prototypes>) {
    // the frame: the cluster (never set here), the two ints read into locals, the token
    const FRAME: u32 = 16;
    let frame = m.local(FRAME);
    let (cluster_at, count_at, state_at, token_at) = (frame, frame + 4, frame + 8, frame + 12);
    let list_at = protos.addr().wrapping_add(4 * slot::CLUSTER as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        match m.i16(token_at) {
            2 => {
                byteswap_fread(m, o, count_at, 4, 1, file);
                let k = m.u32(cluster_at);
                if k != 0 {
                    let v = m.u32(count_at);
                    m.set_u32(k.wrapping_add(kluster::COUNT), v);
                }
            }
            1 => {
                let name = string_binary_read(m, o, file);
                let k = kluster_add_to_list(m, o, model, name, 1);
                m.set_u32(cluster_at, k);
            }
            3 => {
                let k = m.u32(cluster_at);
                let mut n = m.i32(k.wrapping_add(kluster::COUNT));
                if m.i32(model.wrapping_add(node::TYPE)) == 2 {
                    n = n.wrapping_mul(2);
                }
                let bytes = (n as u32).wrapping_mul(4);
                let block = if alloc_fits(m, o, file, bytes, n as u32) { m.si_malloc(bytes) } else { 0 };
                if k != 0 && block != 0 {
                    m.set_u32(k + kluster::INDICES, block);
                    let mut i: i32 = 0;
                    while i < n {
                        let at = m.u32(k + kluster::INDICES).wrapping_add(4 * i as u32);
                        byteswap_fread(m, o, at, 4, 1, file);
                        i += 1;
                    }
                }
            }
            4 => {
                byteswap_fread(m, o, state_at, 4, 1, file);
                let k = m.u32(cluster_at);
                if k != 0 {
                    let v = m.u32(state_at);
                    m.set_u32(k.wrapping_add(kluster::STATE), v);
                }
            }
            t => {
                file_skip_field(m, o, file, t);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    m.release(FRAME);
}

/// [soft 0x0851bed0 SI_generalMatrixAllocateDbl]: a matrix of doubles: a record of rows,
/// columns and a pointer to an array of row pointers, each row a block of `cols` doubles
/// (nothing is cleared). Null as soon as one allocation fails; what was allocated before
/// stays allocated.
pub fn general_matrix_allocate_dbl(m: &mut Mem, rows: i32, cols: i32) -> u32 {
    let p = m.si_malloc(0xc);
    if p != 0 {
        m.set_i32(p, rows);
        m.set_i32(p + 4, cols);
        let array = m.si_malloc((rows as u32) << 2);
        m.set_u32(p + 8, array);
        if array != 0 {
            let mut i: i32 = 0;
            if rows < 1 {
                return p;
            }
            loop {
                let row = m.si_malloc((cols as u32) << 3);
                let at = m.u32(p + 8).wrapping_add(4 * i as u32);
                m.set_u32(at, row);
                if m.u32(at) == 0 {
                    break;
                }
                i += 1;
                if rows <= i {
                    return p;
                }
            }
        }
    }
    0
}

/// [soft 0x084ad8f0 SI_invKinAllocate]: a new inverse kinematics record: zeros, and 1 in
/// the short at +0x3c. The shorts at +0x36 and +0x3e are not written.
pub fn inv_kin_allocate(m: &mut Mem) -> u32 {
    let p = m.si_malloc(invkin::SIZE);
    if p != 0 {
        for w in (2..=10).chain(0x10..=0x12).chain(0x14..=0x1c).chain([0, 1, 0xb, 0xc, 0xe, 0x13]) {
            m.set_u32(p + 4 * w, 0);
        }
        m.set_u16(p + invkin::FLAG, 0);
        m.set_u16(p + 0x3c, 1);
    }
    p
}

/// [soft 0x084ad9d4 SI_invKinRootAllocate]: makes the record that of a chain root with
/// `joints` joints: the count, the flag at +0x34 cleared, and by `other` either type 40
/// with matrices of 3 × 3n and 3n × 3 doubles, or type 10, 20 or 30 for one, two or more
/// joints, with matrices of 2 × n and n × 2 for three or more (and nothing but the count
/// for none).
pub fn inv_kin_root_allocate(m: &mut Mem, ik: u32, joints: i32, other: i16) {
    if ik == 0 {
        return;
    }
    m.set_i32(ik + invkin::NB_JOINTS, joints);
    m.set_u16(ik + invkin::FLAG, 0);
    let (n, k);
    if other == 0 {
        match joints {
            1 => return m.set_i32(ik + invkin::TYPE, 10),
            2 => return m.set_i32(ik + invkin::TYPE, 0x14),
            _ if joints < 3 => return,
            _ => {}
        }
        m.set_i32(ik + invkin::TYPE, 0x1e);
        let a = general_matrix_allocate_dbl(m, 2, joints);
        m.set_u32(ik + invkin::MATRIX_A, a);
        (n, k) = (joints, 2);
    } else {
        m.set_i32(ik + invkin::TYPE, 0x28);
        n = joints.wrapping_mul(3);
        let a = general_matrix_allocate_dbl(m, 3, n);
        m.set_u32(ik + invkin::MATRIX_A, a);
        k = 3;
    }
    let b = general_matrix_allocate_dbl(m, n, k);
    m.set_u32(ik + invkin::MATRIX_B, b);
}

/// [soft 0x084adb78 SI_invKinReadBinaryChunk]: the inverse kinematics chunk: a new record,
/// its type (an int), the number of joints (an int, into a local), the rotation vector;
/// with a positive number of joints the record is made a chain root's
/// ([`inv_kin_root_allocate`], of the other kind if the type read is 40), and at last the
/// short at +0x34 is set to 1.
pub fn inv_kin_read_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32) -> u32 {
    let ik = inv_kin_allocate(m);
    if ik != 0 {
        const FRAME: u32 = 4;
        let joints_at = m.local(FRAME);
        byteswap_fread(m, o, ik, 4, 1, file);
        byteswap_fread(m, o, joints_at, 4, 1, file);
        vector_fread(m, o, ik + invkin::ROTATION, 0xc, 1, file);
        let joints = m.i32(joints_at);
        // (the bound of this project's memory rule: one row is allocated per joint)
        if 0 < joints && alloc_fits(m, o, file, (joints as u32).saturating_mul(72), joints as u32) {
            inv_kin_root_allocate(m, ik, joints, (m.i32(ik) == 0x28) as i16);
        }
        m.set_u16(ik + invkin::FLAG, 1);
        m.release(FRAME);
    }
    ik
}

/// [soft 0x084bba94 SI_envelopInfoAllocate]: a new envelope: level −1, no lists, the
/// identity matrix, three zero vectors, no curves, and 0, 1, 0, 1 as begin, end, amplitude
/// and scale of both curves.
pub fn envelop_info_allocate(m: &mut Mem) -> u32 {
    let p = m.si_malloc(envelop::SIZE);
    if p != 0 {
        m.set_i32(p + envelop::LEVEL, -1);
        m.set_u32(p + envelop::LISTS, 0);
        matrix_copy(m, SI_IDENTITE.addr(), p + envelop::MATRIX);
        for w in (0x12..=0x1b).chain([0x1c, 0x1d, 0x22, 0x23]) {
            m.set_u32(p + 4 * w, 0);
        }
        for base in [envelop::INNER, envelop::OUTER] {
            m.set_f32(p + base, 0.0);
            m.set_f32(p + base + 4, 1.0);
            m.set_f32(p + base + 8, 0.0);
            m.set_f32(p + base + 0xc, 1.0);
        }
    }
    p
}

/// [soft 0x084bba60 SI_envelopListAllocate]: a new list for `model`, empty and unlinked.
/// The allocation is not checked.
pub fn envelop_list_allocate(m: &mut Mem, model: u32) -> u32 {
    let p = m.si_malloc(envelop::LIST_SIZE);
    m.set_u32(p, model);
    m.set_u32(p.wrapping_add(4), 0);
    m.set_u32(p.wrapping_add(8), 0);
    m.set_u32(p.wrapping_add(0xc), 0);
    p
}

/// [soft 0x084bba28 SI_envelopVertAllocate]: a new envelope vertex: zeros and the weight
/// 100 (the float at 0x08661108). The allocation is not checked.
pub fn envelop_vert_allocate(m: &mut Mem) -> u32 {
    let p = m.si_malloc(envelop::VERT_SIZE);
    m.set_u32(p.wrapping_add(4), 0);
    m.set_u32(p.wrapping_add(8), 0);
    m.set_f32(p.wrapping_add(envelop::VERT_WEIGHT), 100.0);
    m.set_u32(p, 0);
    m.set_u32(p.wrapping_add(envelop::VERT_NEXT), 0);
    p
}

/// [soft 0x084bbefc si_envelopVertListDispose]: frees the vertices of the list `*pp`
/// heads, each through its link, and clears `*pp`.
pub fn envelop_vert_list_dispose(m: &mut Mem, pp: u32) {
    let mut v = m.u32(pp);
    if v != 0 {
        loop {
            let next = m.u32(v + envelop::VERT_NEXT);
            m.free(v);
            v = next;
            if v == 0 {
                break;
            }
        }
        m.set_u32(pp, 0);
    }
}

/// [soft 0x084bbf8c SI_envelopInfoDispose]: frees the envelope `*pp` points to and clears
/// `*pp`: the vertices of every list, **the first list's record only** (the records of
/// further lists stay allocated), the two curves, the envelope.
pub fn envelop_info_dispose(m: &mut Mem, pp: u32) {
    if m.u32(pp) == 0 {
        return;
    }
    let mut list = m.u32(m.u32(pp) + envelop::LISTS);
    if list != 0 {
        loop {
            envelop_vert_list_dispose(m, list + envelop::LIST_FIRST);
            list = m.u32(list + envelop::LIST_NEXT);
            if list == 0 {
                break;
            }
        }
    }
    for off in [envelop::LISTS, envelop::INNER_CURVE, envelop::OUTER_CURVE] {
        let e = m.u32(pp);
        // (the list pointer is looked at again only if there was a list: the same test)
        if m.u32(e + off) != 0 {
            m.free(m.u32(e + off));
            let e = m.u32(pp);
            m.set_u32(e + off, 0);
        }
    }
    if m.u32(pp) != 0 {
        m.free(m.u32(pp));
        m.set_u32(pp, 0);
    }
}

/// [soft 0x083f0d70 envelopVertReadBinaryChunk]: one vertex of an envelope's list: index
/// (1), ratio (3), thickness (4), weight (6); a vector that is dropped (2); and a matrix
/// (5), read into entry `index` of `matrices` if that is not null and dropped otherwise.
pub fn envelop_vert_read_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, matrices: u32, index: i32, protos: Ptr32<Prototypes>) -> u32 {
    let v = envelop_vert_allocate(m);
    if v == 0 {
        return 0;
    }
    // the frame: the dropped matrix, the dropped vector, the token
    const FRAME: u32 = 0x50;
    let frame = m.local(FRAME);
    let (matrix, vector, token_at) = (frame, frame + 0x40, frame + 0x4c);
    let list_at = protos.addr().wrapping_add(4 * slot::ENVELOP_VERT as u32);
    let mut at: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut at);
    while m.i16(token_at) != 0 {
        match m.i16(token_at) {
            1 => {
                byteswap_fread(m, o, v + envelop::VERT_INDEX, 4, 1, file);
            }
            2 => {
                vector_fread(m, o, vector, 0xc, 1, file);
            }
            3 => {
                float_fread(m, o, v + envelop::VERT_RATIO, 4, 1, file);
            }
            4 => {
                float_fread(m, o, v + envelop::VERT_THICKNESS, 4, 1, file);
            }
            5 => {
                let to = if matrices == 0 { matrix } else { matrices.wrapping_add((index as u32).wrapping_mul(0x40)) };
                matrix_fread(m, o, to, 0x40, 1, file);
            }
            6 => {
                float_fread(m, o, v + envelop::VERT_WEIGHT, 4, 1, file);
            }
            t => {
                file_skip_field(m, o, file, t);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut at);
    }
    m.release(FRAME);
    v
}

/// [soft 0x083f0e98 envelopReadBinaryChunk]: the envelope of the node `model`.
///
/// First the node gets an initial transformation if it has none: a block of three
/// vectors holding its scaling, rotation and translation.
///
/// Tokens: the level (1), the joint area (5), the inner curve (4 its type; 7, 8, 9, 11
/// begin, end, amplitude, scale; 17 a short count and, if positive, that many floats in a
/// new block) and the outer curve the same (12; 13 to 16; 18); the vertex lists (20 the
/// number of lists into a local; 21 per list a new list, an int count and that many
/// vertices, the lists linked both ways); the one list of older files (19 with a short
/// count, 10 with an int count). Vectors that go elsewhere: 2 into the node's initial
/// rotation; 22, 23, 24 into the initial scaling, rotation, translation of the node's
/// father if that is a null with algorithm 1 or 2 (a chain root), which gets a zeroed
/// block for them if it has none; otherwise, like 3, into a local. The float of 6 goes
/// into a local.
///
/// **In a file up to version 2.59** (`binary.c`'s `fileVersion` not above the double at
/// 0x0865bd90) the matrices of the vertices (vertex token 5) are kept: one zeroed array
/// of matrices per list, a table of the arrays, and a zeroed record of 0x14 bytes **stored
/// in the node's user data pointer** (+0x34c, replacing what was there) whose data word
/// is the table. `SI_envelopRebuild` uses them after the hierarchy is read.
///
/// The number of lists (20) and the counts lie in locals: token 21 without token 20
/// before it runs on what the stack held.
pub fn envelop_read_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, model: u32, protos: Ptr32<Prototypes>) -> u32 {
    let e = envelop_info_allocate(m);
    if e == 0 {
        return 0;
    }
    if m.u32(model.wrapping_add(node::INITIAL_SRT)) == 0 {
        let block = m.si_calloc(1, 0x24);
        m.set_u32(model + node::INITIAL_SRT, block);
        for (k, from) in [node::SCALING, node::ROTATION, node::TRANSLATION].into_iter().enumerate() {
            for w in 0..3 {
                let v = m.u32(model + from + 4 * w);
                let to = if k == 0 { block } else { m.u32(model + node::INITIAL_SRT) };
                m.set_u32(to.wrapping_add(0xc * k as u32 + 4 * w), v);
            }
        }
    }
    // the frame: the dropped vector and float, the counts and scratch words read through
    // a pointer, the token
    const FRAME: u32 = 0x24;
    let frame = m.local(FRAME);
    let (vector, float_at, count_at, list_count_at, short_at, nb_lists_at, token_at) = (frame, frame + 0xc, frame + 0x10, frame + 0x14, frame + 0x18, frame + 0x1a, frame + 0x1c);
    let old = |m: &Mem| (m[FILE_VERSION] as f64) <= 2.59;
    let (mut last_list, mut last_vert, mut table, mut matrices) = (0u32, 0u32, 0u32, 0u32);
    let list_at = protos.addr().wrapping_add(4 * slot::ENVELOP as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        let token = m.i16(token_at);
        match token {
            1 => {
                byteswap_fread(m, o, e + envelop::LEVEL, 4, 1, file);
            }
            2 => {
                let at = m.u32(model + node::INITIAL_SRT).wrapping_add(0xc);
                vector_fread(m, o, at, 0xc, 1, file);
            }
            3 => {
                vector_fread(m, o, vector, 0xc, 1, file);
            }
            4 | 0xc => {
                byteswap_fread(m, o, e + if token == 4 { envelop::INNER_TYPE } else { envelop::OUTER_TYPE }, 4, 1, file);
            }
            5 | 7 | 8 | 9 | 0xb | 0xd | 0xe | 0xf | 0x10 => {
                let off = match token {
                    5 => envelop::JOINT_AREA,
                    7 => envelop::INNER,
                    8 => envelop::INNER + 4,
                    9 => envelop::INNER + 8,
                    0xb => envelop::INNER + 0xc,
                    0xd => envelop::OUTER,
                    0xe => envelop::OUTER + 4,
                    0xf => envelop::OUTER + 8,
                    _ => envelop::OUTER + 0xc,
                };
                float_fread(m, o, e + off, 4, 1, file);
            }
            6 => {
                float_fread(m, o, float_at, 4, 1, file);
            }
            10 | 0x13 => {
                let list = envelop_list_allocate(m, 0);
                m.set_u32(e + envelop::LISTS, list);
                if token == 10 {
                    byteswap_fread(m, o, count_at, 4, 1, file);
                } else {
                    byteswap_fread(m, o, short_at, 2, 1, file);
                    let v = m.i16(short_at) as i32;
                    m.set_i32(count_at, v);
                }
                if old(m) && 0 < m.i32(count_at) {
                    let record = m.si_calloc(1, 0x14);
                    m.set_u32(model + node::USER_DATA, record);
                    table = m.si_calloc(1, 4);
                    let n = m.u32(count_at);
                    matrices = if alloc_fits(m, o, file, n.wrapping_mul(0x40), n) { m.si_calloc(n, 0x40) } else { 0 };
                }
                let mut i: i32 = 0;
                while i < m.i32(count_at) {
                    let v = envelop_vert_read_binary_chunk(m, o, file, matrices, i, protos);
                    if v != 0 {
                        let list = m.u32(e + envelop::LISTS);
                        if m.u32(list.wrapping_add(envelop::LIST_FIRST)) == 0 {
                            m.set_u32(list.wrapping_add(envelop::LIST_FIRST), v);
                        } else {
                            m.set_u32(last_vert.wrapping_add(envelop::VERT_NEXT), v);
                        }
                        last_vert = v;
                    }
                    i += 1;
                }
                if old(m) && 0 < m.i32(count_at) {
                    m.set_u32(table, matrices);
                    let record = m.u32(model + node::USER_DATA);
                    m.set_u32(record.wrapping_add(0xc), table);
                    let record = m.u32(model + node::USER_DATA);
                    m.set_u32(record.wrapping_add(0x10), 0);
                }
            }
            0x11 | 0x12 => {
                byteswap_fread(m, o, short_at, 2, 1, file);
                if 0 < m.i16(short_at) {
                    let curve = m.si_malloc((m.i16(short_at) as i32 as u32) << 2);
                    let mut i: i16 = 0;
                    while i < m.i16(short_at) {
                        float_fread(m, o, curve.wrapping_add((i as i32 as u32).wrapping_mul(4)), 4, 1, file);
                        i = i.wrapping_add(1);
                    }
                    m.set_u32(e + if token == 0x11 { envelop::INNER_CURVE } else { envelop::OUTER_CURVE }, curve);
                }
            }
            0x14 => {
                byteswap_fread(m, o, nb_lists_at, 2, 1, file);
            }
            0x15 => {
                if old(m) && 0 < m.i16(nb_lists_at) {
                    let record = m.si_calloc(1, 0x14);
                    m.set_u32(model + node::USER_DATA, record);
                    table = m.si_calloc(m.i16(nb_lists_at) as i32 as u32, 4);
                }
                let mut k: i16 = 0;
                while k < m.i16(nb_lists_at) {
                    let list = envelop_list_allocate(m, 0);
                    byteswap_fread(m, o, list_count_at, 4, 1, file);
                    if old(m) {
                        let n = m.i32(list_count_at);
                        matrices = if n < 1 {
                            0
                        } else if alloc_fits(m, o, file, (n as u32).wrapping_mul(0x40), n as u32) {
                            m.si_calloc(n as u32, 0x40)
                        } else {
                            0
                        };
                    }
                    let mut i: i32 = 0;
                    while i < m.i32(list_count_at) {
                        let v = envelop_vert_read_binary_chunk(m, o, file, matrices, i, protos);
                        if v != 0 {
                            if m.u32(list.wrapping_add(envelop::LIST_FIRST)) == 0 {
                                m.set_u32(list.wrapping_add(envelop::LIST_FIRST), v);
                            } else {
                                m.set_u32(last_vert.wrapping_add(envelop::VERT_NEXT), v);
                            }
                            last_vert = v;
                        }
                        i += 1;
                    }
                    if m.u32(e + envelop::LISTS) == 0 {
                        m.set_u32(e + envelop::LISTS, list);
                    } else {
                        m.set_u32(last_list.wrapping_add(envelop::LIST_NEXT), list);
                        m.set_u32(list.wrapping_add(envelop::LIST_PREVIOUS), last_list);
                    }
                    if old(m) {
                        m.set_u32(table.wrapping_add((k as i32 as u32).wrapping_mul(4)), matrices);
                    }
                    last_list = list;
                    k = k.wrapping_add(1);
                }
                if old(m) && 0 < m.i16(nb_lists_at) {
                    let record = m.u32(model + node::USER_DATA);
                    m.set_u32(record.wrapping_add(0xc), table);
                    let record = m.u32(model + node::USER_DATA);
                    m.set_u32(record.wrapping_add(0x10), 0);
                }
            }
            0x16 | 0x17 | 0x18 => {
                let father = m.u32(model + node::FATHER);
                if father == 0 || m.i32(father + node::TYPE) != 0 || 1 < m.u32(father + node::ALGORITHM).wrapping_sub(1) {
                    vector_fread(m, o, vector, 0xc, 1, file);
                } else {
                    if m.u32(father + node::INITIAL_SRT) == 0 {
                        let block = m.si_calloc(1, 0x24);
                        let father = m.u32(model + node::FATHER);
                        m.set_u32(father + node::INITIAL_SRT, block);
                    }
                    let father = m.u32(model + node::FATHER);
                    let at = m.u32(father + node::INITIAL_SRT).wrapping_add(0xc * (token as u32 - 0x16));
                    vector_fread(m, o, at, 0xc, 1, file);
                }
            }
            t => {
                file_skip_field(m, o, file, t);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
    m.release(FRAME);
    e
}
