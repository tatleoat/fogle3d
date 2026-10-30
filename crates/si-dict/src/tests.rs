// SPDX-License-Identifier: MIT
//! Unit tests on made-up dictionaries: the routines working together on one image.

use crate::globals::*;
use crate::outside::{Outside, Routine};
use crate::records::UpdNode;
use crate::*;
use si_mem::{field, Mem, Ptr32};
use si_types::dict::{Elem, ElemDict, UpdList};

/// An outside world for the tests: the chapter suffixes come from the chapter table, the
/// rest answers 0; every call is counted by name.
#[derive(Default)]
struct World {
    calls: Vec<&'static str>,
}

impl Outside for World {
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        self.calls.push(routine.name);
        match routine.name {
            "SI_edGetSuffixFromChapter" => rodata::CHAP_INFO[args[0] as usize % 36].2,
            "SI_grafnodeDefColor" => 0x107,
            _ => {
                let _ = m;
                0
            }
        }
    }
}

/// A scene dictionary with the 36 chapters and three relations: parent (110, models,
/// list 1, fan-out 1), a curve relation (100, model to animation, list 0) and a
/// constraint (120, model to model, list 1, rank 7).
fn scene(m: &mut Mem, o: &mut World) -> Ptr32<ElemDict> {
    si_mem::init_globals(m, ALL);
    rodata::store(m);
    assert_eq!(elemmisc::add(m), 0);
    let ed = m[SI_ELEM_DICTS];
    elemmisc::load_chapters(m, o, ed, SI_CID, 1);
    reldict::init(m, SI_RD);
    let name = Ptr32::new(m.place_cstr(b"Parent"));
    let (model, ani) = (m[SI_CID].model, m[SI_CID].ani);
    reldict::add_relation(m, SI_RD, name, 110, model, model, 1, 1, 10, 1, 0, 1, 0, 1, Ptr32::NULL, 0, 0);
    reldict::add_relation(m, SI_RD, name, 120, model, model, 1, 1, 7, 0x7fff, 0, 0, 0, 1, Ptr32::NULL, 0, 0);
    reldict::add_relation(m, SI_RD, name, 100, model, ani, 0, 0, 3, 3, 0, 0, 0, 1, Ptr32::NULL, 0, 0);
    ed
}

#[test]
fn chapters_are_the_table_in_order() {
    let mut m = Mem::new();
    let mut o = World::default();
    let ed = scene(&mut m, &mut o);
    assert_eq!(m[ed].nb_lists, 36);
    assert_eq!((m[SI_CID].dsc, m[SI_CID].ani, m[SI_CID].model, m[SI_CID].shd_lig_photon), (0, 1, 8, 35));
    let models = m[ed].lists.add(8);
    assert_eq!(m.cstr(m[models].name.addr()), b"MODELS");
    assert_eq!(m.cstr(m[models].suffix.addr()), b"hrc");
    assert_eq!((m[m[ed].lists.add(9)].kind, m[m[ed].lists.add(31)].kind, m[models].kind), (0, 2, 1));
    let name = Ptr32::new(m.place_cstr(b"render_pictures"));
    assert_eq!(elemdict::find_list_by_name(&m, ed, name), 11);
    assert!(m.fault().is_none());
}

#[test]
fn relations_hierarchy_sort_and_deletion() {
    let mut m = Mem::new();
    let mut o = World::default();
    let ed = scene(&mut m, &mut o);
    let (model, ani) = (m[SI_CID].model, m[SI_CID].ani);
    // three models (the first is the father of the other two) and one curve
    for k in 0..3 {
        assert_eq!(elemdict::add_elem(&mut m, &mut o, ed, model, Ptr32::NULL, Ptr32::new(0x7000 + k), Ptr32::NULL, 0, 0), k as i32);
    }
    assert_eq!(elemdict::add_elem(&mut m, &mut o, ed, ani, Ptr32::NULL, Ptr32::new(0x7100), Ptr32::NULL, 0, 0), 0);
    assert_eq!(elemrel::add_relation(&mut m, ed, SI_RD, 1, 0, 110, 1, 0), 1);
    assert_eq!(elemrel::add_relation(&mut m, ed, SI_RD, 2, 0, 110, 1, 0), 1);
    // a second father is refused by the fan-out of 1, and nothing is added at the father
    let father = elemdict::get_elem(&m, ed, model, 0);
    assert_eq!(elemrel::add_relation(&mut m, ed, SI_RD, 2, 1, 110, 1, 0), 1);
    assert_eq!(m[m[father].backward.groups.add(1)].nb, 2);
    let second = elemdict::get_elem(&m, ed, model, 1);
    assert_eq!(m[m[second].backward.groups.add(1)].nb, 0);
    // the son's curve, and a constraint of son 2 on son 1
    assert_eq!(elemrel::add_relation(&mut m, ed, SI_RD, 1, 0, 100, 1, 0), 1);
    assert_eq!(elemrel::add_relation(&mut m, ed, SI_RD, 2, 1, 120, 1, 0), 1);
    elemdict::update_hier_info(&mut m, ed, model, 1, 110);
    assert_eq!((m[second].root_id, m[second].hier_level, m[father].hier_level), (0, 1, 0));
    assert_eq!(elemmisc::find_son(&mut m, ed, father), second);
    assert_eq!(toposort::find_father(&m, second), 0);

    // the sort: the curve step of son 1, then son 1's transformation, then son 2's constraint
    let upd: Ptr32<UpdList> = field!(ed, ElemDict, upd_list);
    assert_eq!(toposort::topo_sort_dict(&mut m, &mut o, SI_RD, ed, upd, 0, 0), 1);
    let nodes: Ptr32<UpdNode> = m[upd].nodes.cast();
    let steps: Vec<(i32, i32, i16)> = (0..m[upd].nb).map(|k| (m[nodes.add(k)].elemid, m[nodes.add(k)].relid, m[nodes.add(k)].group)).collect();
    assert_eq!(steps, vec![(1, 100, 0), (2, 120, 1), (2, -100, 1)]);
    assert_eq!(m[upd].dirty, 0);
    assert!(!toposort::check_cycle(&mut m, &mut o, SI_RD, ed));

    // a cycle: son 1 constrained by son 2 as well
    assert_eq!(elemrel::add_relation(&mut m, ed, SI_RD, 1, 2, 120, 1, 0), 1);
    assert_eq!(m[upd].dirty, 1);
    assert!(toposort::check_cycle(&mut m, &mut o, SI_RD, ed));

    // deleting son 1 takes its entries away at both ends and keeps the slot
    elemdict::set_delete_elem(&mut m, &mut o, ed, model, second, 1, -1, 0, 1);
    assert_eq!(m[second].status, 2);
    assert!(elemdict::get_elem(&m, ed, model, 1).is_null());
    assert_eq!(m[m[father].backward.groups.add(1)].nb, 1);
    assert_eq!(elemdict::elemlist_count_valid_elems(&m, m[ed].lists.add(model)), 2);
    // the next element takes the deleted slot
    assert_eq!(elemdict::add_elem(&mut m, &mut o, ed, model, Ptr32::NULL, Ptr32::new(0x7009), Ptr32::NULL, 0, 0), 1);
    assert_eq!(elemdict::find_elem_by_data(&mut m, &mut o, ed, model, Ptr32::new(0x7009)), 1);
    assert!(m.fault().is_none());
}

#[test]
fn disposal_frees_what_the_dictionary_owns() {
    let mut m = Mem::new();
    let mut o = World::default();
    let ed = scene(&mut m, &mut o);
    let model = m[SI_CID].model;
    for k in 0..4 {
        elemdict::add_elem(&mut m, &mut o, ed, model, Ptr32::NULL, Ptr32::new(0x7000 + k), Ptr32::NULL, 0, 0);
    }
    elemrel::add_relation(&mut m, ed, SI_RD, 1, 0, 110, 1, 0);
    let e: Ptr32<Elem> = elemdict::get_elem(&m, ed, model, 3);
    assert!(!e.is_null());
    elemdict::dispose(&mut m, &mut o, ed, 0);
    assert_eq!((m[ed].nb_lists, m[ed].lists.is_null(), m[ed].upd_list.dirty), (0, true, 1));
    // Everything is freed but the 36 chapter names (which the dictionary never frees) and
    // the blocks a reallocation left behind, which the allocator notes as freed itself.
    let live = m.heap_blocks().iter().filter(|b| !m.is_freed(b.0)).count();
    let dictionaries_and_rules = 2 + 3 + 3; // SI_elemDicts and its flags; the rules, their sort table and its index array; three rule names
    assert_eq!(live, 36 + dictionaries_and_rules);
    reldict::dispose(&mut m, SI_RD);
    assert_eq!(m.heap_blocks().iter().filter(|b| !m.is_freed(b.0)).count(), 36 + 2);
    assert!(m.fault().is_none());
}
