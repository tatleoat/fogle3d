// SPDX-License-Identifier: MIT
//! Unit tests of the envelope modules, on made-up data only.

use crate::envelop::exp_fct;
use crate::envelopset::comp_ratio;
use crate::envtools::{envelop_info_allocate, envelop_info_dispose, envelop_list_allocate, envelop_vert_allocate};
use si_mem::{Mem, Ptr32};

#[test]
fn power_with_the_profiles_guards() {
    assert_eq!(exp_fct(0.0, 2.0), 0.0);
    assert_eq!(exp_fct(-1.0, 2.0), 0.0);
    assert_eq!(exp_fct(2.0, 0.0), 1.0);
    assert_eq!(exp_fct(2.0, 3.0), 8.0);
}

#[test]
fn place_along_a_bone_and_distance() {
    let mut m = Mem::new();
    let dist: Ptr32<f32> = Ptr32::new(m.place(4, 4));
    let t = comp_ratio(&mut m, [1.0, 1.0, 0.0], [0.0, 0.0, 0.0], [2.0, 0.0, 0.0], dist);
    assert_eq!((t.to_f32(), m[dist]), (0.5, 1.0));
    // beyond the end: the distance is to the end
    let t = comp_ratio(&mut m, [5.0, 0.0, 0.0], [0.0, 0.0, 0.0], [2.0, 0.0, 0.0], dist);
    assert_eq!((t.to_f32(), m[dist]), (2.5, 3.0));
    // a bone of no length: distance to its start, place 0
    let t = comp_ratio(&mut m, [0.0, 3.0, 4.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], dist);
    assert_eq!((t.to_f32(), m[dist]), (0.0, 5.0));
}

#[test]
fn records_are_made_and_freed_block_for_block() {
    let mut m = Mem::new();
    let e = envelop_info_allocate(&mut m);
    let l = envelop_list_allocate(&mut m, Ptr32::NULL);
    let v = envelop_vert_allocate(&mut m);
    assert_eq!((m[e].level, m[v].weight, m[e].inner.scale), (-1, 100.0, 1.0));
    m[l].verts = v;
    m[e].lists = l;
    let slot: Ptr32<Ptr32<crate::env_records::EnvInfo>> = Ptr32::new(m.place(4, 4));
    m[slot] = e;
    envelop_info_dispose(&mut m, slot);
    assert!(m[slot].is_null());
    assert_eq!(m.freed(), &[v.addr(), l.addr(), e.addr()]);
}

/// A node of `Model`'s size from the heap, zeroed, with a kind and an algorithm.
fn node(m: &mut Mem, kind: i32, algo: i32) -> Ptr32<si_types::model::Model> {
    let n: Ptr32<si_types::model::Model> = Ptr32::new(m.calloc(1, 0x430));
    m[n].kind = kind;
    m[n].algo_type = algo;
    n
}

#[test]
fn an_earlier_chain_is_numbered_and_given_lists() {
    use crate::envtools::{envelop_data_convert, envelop_rebuild};
    let mut m = Mem::new();
    // a chain root without record, its joint with a record of level 0 and two lists, and the
    // effector with a record of its own
    let (root, joint, eff) = (node(&mut m, 0, 1), node(&mut m, 5, 0), node(&mut m, 0, 3));
    m[root].son = joint;
    m[joint].father = root;
    m[joint].son = eff;
    m[eff].father = joint;
    let (ej, ee) = (envelop_info_allocate(&mut m), envelop_info_allocate(&mut m));
    m[ej].level = 0;
    let (l1, l2) = (envelop_list_allocate(&mut m, Ptr32::NULL), envelop_list_allocate(&mut m, Ptr32::NULL));
    m[l1].next = l2;
    m[l2].prev = l1;
    m[ej].lists = l1;
    m[joint].env_skel_jnt = ej.cast();
    m[eff].env_skel_jnt = ee.cast();
    m[root].scal = si_types::geom::Vector { x: 1.0, y: 2.0, z: 3.0 };
    envelop_data_convert(&mut m, root, 0);
    let er: Ptr32<crate::env_records::EnvInfo> = m[root].env_skel_jnt.cast();
    assert!(!er.is_null());
    assert_eq!((m[er].level, m[ej].level, m[ee].level), (0, 1, 2));
    assert_eq!((m[root].envelop, m[joint].envelop, m[eff].envelop), (9, 9, 9));
    // one empty list per list of the first son, chained both ways; the effector the same
    // from its father
    let a = m[er].lists;
    let b = m[a].next;
    assert!(!a.is_null() && !b.is_null() && m[b].next.is_null() && m[b].prev == a && m[a].owner.is_null());
    let c = m[ee].lists;
    assert!(!c.is_null() && !m[c].next.is_null());
    // the root's rest transformation from its local one
    let srt = m[root].init_srt;
    assert_eq!((m.f32(srt.addr()), m.f32(srt.addr() + 4), m.f32(srt.addr() + 8)), (1.0, 2.0, 3.0));
    // nothing for no node
    envelop_rebuild(&mut m, &mut crate::NoOutside { calls: Vec::new(), random: crate::Drand48::default() }, Ptr32::NULL, 1, 0);
}
