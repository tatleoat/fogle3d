// SPDX-License-Identifier: MIT
//! `index.c`: the index of modifications a modeller edit leaves (how the old point numbers
//! map to the new ones), and its use on the clusters and envelopes.

use super::DeformS;
use si_deform::{envtools, klusters};
use si_mem::{Mem, Outside, Ptr32};
use si_types::model::Model;

/// An array of `count` ints in the program's allocator, filled by `kind` [soft 0x08176740 indexModifGeneralAllocate]:
/// 0 the identity (`i`), 1 all -1, 2 the reverse (`count - 1 - i`), any other kind left zero.
/// The answer is the array.
pub fn index_modif_general_allocate(m: &mut Mem, count: u32, kind: i32) -> u32 {
    let a = m.si_calloc(count, 4);
    let n = count as i32;
    match kind {
        0 => {
            let mut i = 0;
            while i < n {
                m.set_i32(a.wrapping_add((i as u32).wrapping_mul(4)), i);
                i += 1;
            }
        }
        1 => {
            let mut i = 0;
            while i < n {
                m.set_i32(a.wrapping_add((i as u32).wrapping_mul(4)), -1);
                i += 1;
            }
        }
        2 => {
            let mut i = 0;
            let mut v = n.wrapping_sub(1);
            while i < n {
                m.set_i32(a.wrapping_add((i as u32).wrapping_mul(4)), v);
                i += 1;
                v = v.wrapping_sub(1);
            }
        }
        _ => {}
    }
    a
}

/// Frees an index and clears the pointer to it [soft 0x08176a00 SI_indexModifFree]; `pp` is the
/// address of the pointer.
pub fn index_modif_free(m: &mut Mem, pp: u32) {
    let p = m.u32(pp);
    if p != 0 {
        m.free(p);
        m.set_u32(pp, 0);
    }
}

/// An index for a node's geometry, sized by its first definition [soft 0x08176860 SI_indexModifAllocate]:
/// 0 when the node has none of the four modelling words (+0x348, +0x408, +0x410, +0x418) and
/// its +0x31c is neither 6 nor 5; else by kind (1 patch grid `u16 +0xc * u16 +0xe`, 4 mesh
/// `+0x14`, 6 spline `u16 +0x10`, 9 NURBS curve `+4`, 10 NURBS surface `+0x48`; any other kind
/// answers 0), a general index of `kind_of_index`.
pub fn index_modif_allocate(m: &mut Mem, node: u32, kind_of_index: i32) -> u32 {
    if m.u32(node + 0x348) == 0 && m.u32(node + 0x408) == 0 && m.u32(node + 0x410) == 0 && m.u32(node + 0x418) == 0
        && m.i32(node + 0x31c) != 6 && m.i32(node + 0x31c) != 5
    {
        return 0;
    }
    let def = m.u32(m.u32(node + 0x38));
    let count = match m.i32(node + 0x30) {
        1 => (m.u16(def + 0xc) as u32).wrapping_mul(m.u16(def + 0xe) as u32),
        4 => m.u32(def + 0x14),
        6 => m.u16(def + 0x10) as u32,
        9 => m.u32(def + 4),
        10 => m.u32(def + 0x48),
        _ => return 0,
    };
    index_modif_general_allocate(m, count, kind_of_index)
}

/// Moves a mesh's per-edge user data by an index and frees the index [soft 0x08176954 SI_indexEdgePropagate];
/// `pp` the address of the index pointer.
pub fn index_edge_propagate(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, pp: u32) {
    if !node.is_null() && m.u32(pp) != 0 {
        let map = m.u32(pp);
        klusters::kluster_edge_move_indexes(m, &mut DeformS(o), node, map);
        index_modif_free(m, pp);
    }
}

/// Moves the clusters and the envelopes of a node by an index, drops the empty clusters and frees
/// the index [soft 0x08176988 SI_indexModifPropagate]; `pp` the address of the index pointer.
pub fn index_modif_propagate(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, pp: u32, full: i16) {
    if node.is_null() || m.u32(pp) == 0 {
        return;
    }
    let map = m.u32(pp);
    klusters::kluster_move_all_indexes(m, &mut DeformS(o), node, map);
    klusters::kluster_dispose_empty(m, &mut DeformS(o), node, full);
    let root = envtools::envelop_skel_root(m, node);
    if !root.is_null() {
        let brothers = (m.i32(node.addr() + 0x31c) == 6) as i16;
        envtools::envelop_renum(m, node, root, Ptr32::new(map), brothers);
    }
    index_modif_free(m, pp);
}
