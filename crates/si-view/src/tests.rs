// SPDX-License-Identifier: MIT
//! Unit tests on made-up data: our own reading tested against itself. The comparison with
//! the original is `view-emucheck`'s.

use crate::globals::*;
use crate::graflist as g;
use crate::{GrafList, GrafNode, Mem, Ptr32};

fn image() -> Mem {
    let mut m = Mem::new();
    si_mem::init_globals(&mut m, ALL);
    m
}

fn chain(m: &Mem, list: Ptr32<GrafList>) -> Vec<Ptr32<GrafNode>> {
    let mut out = Vec::new();
    let mut n = m[list].first;
    while !n.is_null() {
        out.push(n);
        n = m[n].next;
    }
    out
}

#[test]
fn the_first_list_is_made_once() {
    let mut m = image();
    g::graf_list_create(&mut m);
    let lists = m[SI_GRAF_LISTS];
    assert!(!lists.is_null());
    assert_eq!((m[SI_NB_GRAF_LISTS], m[lists].unk_0e, g::graf_list_exists(&m, 0), g::graf_list_exists(&m, 1)), (1, 1, 1, 0));
    g::graf_list_create(&mut m);
    assert_eq!(m[SI_GRAF_LISTS], lists);
    assert_eq!(g::graf_list_add(&mut m), 1);
    assert_eq!((m[SI_NB_GRAF_LISTS], g::graf_list_exists(&m, 1)), (2, 1));
    // the current list cannot be given up, another can
    assert_eq!((g::graf_list_remove(&mut m, 0), g::graf_list_remove(&mut m, 1), g::graf_list_exists(&m, 1)), (0, 1, 0));
    assert!(m.fault().is_none());
}

#[test]
fn nodes_are_chained_both_ways() {
    let mut m = image();
    let list = g::graf_list_allocate(&mut m);
    let a = g::grafnode_allocate(&mut m);
    let b = g::grafnode_allocate(&mut m);
    let c = g::grafnode_allocate(&mut m);
    assert_eq!(m[a].wire_type, 2);
    g::graflist_insert_grafnode(&mut m, list, Ptr32::NULL, a);
    g::graflist_insert_grafnode(&mut m, list, Ptr32::NULL, b); // new head
    g::graflist_insert_grafnode(&mut m, list, b, c); // between b and a
    assert_eq!(chain(&m, list), vec![b, c, a]);
    assert_eq!((m[b].prev, m[c].prev, m[a].prev), (Ptr32::NULL, b, c));
    let mut o = crate::NoOutside::default();
    g::graflist_set_active(&mut m, list, c);
    g::graflist_remove_grafnode(&mut m, &mut o, list, c);
    assert_eq!(chain(&m, list), vec![b, a]);
    assert_eq!((m[list].active, m[a].prev, m[c].prev, m[c].next), (Ptr32::NULL, b, Ptr32::NULL, a));
    assert_eq!(o.calls.len(), 1);
    assert!(m.fault().is_none());
}

/// An outside world for the tests: answers the two questions about the model tree that
/// `SI_graflistAddData` asks (as `SI_modelNextNode (0x08478340)` and `SI_modelNbSubModels
/// (0x0846d5ec)` are described in finding 0027; `si-model` has them on the image) and 0
/// to everything else.
struct Tree {
    calls: Vec<&'static str>,
}

impl crate::Outside for Tree {
    fn call(&mut self, m: &mut Mem, routine: &'static crate::Routine, args: &[u32]) -> u32 {
        use crate::Model;
        self.calls.push(routine.name);
        let model: Ptr32<Model> = Ptr32::new(args.first().copied().unwrap_or(0));
        match routine.name {
            "SI_modelNextNode" => {
                if !m[model].son.is_null() {
                    return m[model].son.addr();
                }
                let mut at = model;
                while !at.is_null() {
                    if !m[at].brother.is_null() {
                        return m[at].brother.addr();
                    }
                    at = m[at].father;
                }
                0
            }
            "SI_modelNbSubModels" => {
                fn count(m: &Mem, p: Ptr32<Model>) -> u32 {
                    let mut n = 0;
                    let mut s = m[p].son;
                    while !s.is_null() {
                        n += 1 + count(m, s);
                        s = m[s].brother;
                    }
                    n
                }
                count(m, model)
            }
            _ => 0,
        }
    }
}

fn model(m: &mut Mem, father: Ptr32<crate::Model>) -> Ptr32<crate::Model> {
    let p: Ptr32<crate::Model> = Ptr32::new(m.calloc(1, 0x430));
    m[p].father = father;
    if !father.is_null() {
        let mut last = m[father].son;
        if last.is_null() {
            m[father].son = p;
        } else {
            while !m[last].brother.is_null() {
                last = m[last].brother;
            }
            m[last].brother = p;
        }
    }
    p
}

#[test]
fn objects_enter_at_the_head_and_hierarchies_stay_together() {
    let mut m = image();
    let mut o = Tree { calls: Vec::new() };
    g::graf_list_create(&mut m);
    let list = g::scene_list(&m);
    // the camera first, as the program does when it empties a scene
    let camera = Ptr32::new(0x7000);
    let cam = g::graflist_add_data(&mut m, &mut o, list, camera, 4, 0);
    // a hierarchy: a, with sons b (with son d) and c
    let a = model(&mut m, Ptr32::NULL);
    let b = model(&mut m, a);
    let c = model(&mut m, a);
    let d = model(&mut m, b);
    let root = g::graflist_add_data(&mut m, &mut o, list, a.cast(), 2, 1);
    // a second root, alone
    let e = model(&mut m, Ptr32::NULL);
    let lone = g::graflist_add_data(&mut m, &mut o, list, e.cast(), 2, 0);
    let nodes = chain(&m, list);
    let what: Vec<(i32, u32, i16)> = nodes.iter().map(|&n| (m[n].kind, m[n].data.addr(), m[n].nb_sub)).collect();
    assert_eq!(
        what,
        vec![(2, e.addr(), 0), (2, a.addr(), 3), (3, b.addr(), 1), (3, d.addr(), 0), (3, c.addr(), 0), (5, 0x7000, 1), (4, 0x7000, 0)]
    );
    assert_eq!((nodes[0], nodes[1], nodes[5]), (lone, root, cam));
    // selected as a branch: 3 on the root, 2 below; single selection made it the active node
    assert_eq!(nodes[1..5].iter().map(|&n| m[n].selection).collect::<Vec<_>>(), vec![3, 2, 2, 2]);
    assert_eq!(m[list].active, root);
    // the look-ups the render commands make
    assert_eq!(g::find_gl_data(&m, list, d.cast(), 0), nodes[3]);
    assert_eq!(g::graflist_find_root(&m, nodes[3]), root);
    assert_eq!(g::graflist_count_sel_hierarchies(&m, list, 0), 1);
    // cutting b loose puts its branch at the head; linking it under e puts it behind e
    g::graflist_cut_model(&mut m, &mut o, list, nodes[2], a, 0);
    let after_cut: Vec<u32> = chain(&m, list).iter().map(|&n| m[n].data.addr()).collect();
    assert_eq!(after_cut, vec![b.addr(), d.addr(), e.addr(), a.addr(), c.addr(), 0x7000, 0x7000]);
    assert_eq!((m[root].nb_sub, m[nodes[2]].kind), (1, 2));
    g::graflist_link_model(&mut m, &mut o, list, lone, nodes[2], 0, 0);
    let after_link: Vec<u32> = chain(&m, list).iter().map(|&n| m[n].data.addr()).collect();
    assert_eq!(after_link, vec![e.addr(), b.addr(), d.addr(), a.addr(), c.addr(), 0x7000, 0x7000]);
    assert_eq!((m[lone].nb_sub, m[nodes[2]].kind), (2, 3));
    assert!(m.fault().is_none());
    assert!(o.calls.contains(&"SI_edAddModel") && o.calls.contains(&"SI_edAddCamera"));
}

/// An outside world that knows the model tree (the two routines `SI_graflistAddData`
/// needs answered truthfully) and answers 1 to the rest.
struct ModelTree;

impl crate::Outside for ModelTree {
    fn call(&mut self, m: &mut Mem, routine: &'static crate::Routine, a: &[u32]) -> u32 {
        use si_types::model::Model;
        let sons = |m: &Mem, of: Ptr32<Model>| {
            fn count(m: &Mem, of: Ptr32<Model>) -> u32 {
                let (mut n, mut s) = (0, m[of].son);
                while !s.is_null() {
                    n += 1 + count(m, s);
                    s = m[s].brother;
                }
                n
            }
            count(m, of)
        };
        match routine.name {
            "SI_modelNbSubModels" => sons(m, Ptr32::new(a[0])),
            "SI_modelNextNode" => {
                let mut at: Ptr32<Model> = Ptr32::new(a[0]);
                if !m[at].son.is_null() {
                    return m[at].son.addr();
                }
                while !at.is_null() {
                    if !m[at].brother.is_null() {
                        return m[at].brother.addr();
                    }
                    at = m[at].father;
                }
                0
            }
            "SI_cusReadingAnimModel" => 0,
            _ => 1,
        }
    }
}

#[test]
fn a_loaded_dictionary_becomes_the_list() {
    use si_types::dict::{ChapIds, Elem, ElemDict, ElemList};
    use si_types::model::Model;
    let mut m = image();
    // two root models, the second with a son; one element each, in the MODELS chapter
    let model = |m: &mut Mem| -> Ptr32<Model> { Ptr32::new(m.si_calloc(1, 0x430)) };
    let (a, b, b1) = (model(&mut m), model(&mut m), model(&mut m));
    m[b].son = b1;
    m[b1].father = b;
    let elems: Ptr32<Elem> = Ptr32::new(m.si_calloc(3, 0x84));
    for (k, object) in [a, b, b1].into_iter().enumerate() {
        let e = elems.add(k as i32);
        m[e].data = object.cast();
        m[e].new_flag = 1;
        m[e].selection = 1;
        m[e].wire_type = 1;
    }
    let chapters: Ptr32<ElemList> = Ptr32::new(m.si_calloc(2, 0x40));
    m[chapters.add(1)].nb_elems = 3;
    m[chapters.add(1)].elems = elems;
    let ed: Ptr32<ElemDict> = Ptr32::new(m.si_calloc(1, 0x70));
    m[ed].nb_lists = 2;
    m[ed].lists = chapters;
    let cids: Ptr32<ChapIds> = Ptr32::new(m.si_calloc(1, 0x90));
    for k in 0..36u32 {
        m.set_i32(cids.addr() + 4 * k, 50 + k as i32);
    }
    m[cids].model = 1;
    crate::db2scn::graflist_build_from_elem_dict(&mut m, &mut ModelTree, ed, Ptr32::NULL, cids, 1);
    assert!(m.fault().is_none(), "{:?}", m.fault());
    // the last root is the chain's head; a hierarchy stays together behind its root
    let list = g::scene_list(&m);
    let nodes = chain(&m, list);
    let objects: Vec<u32> = nodes.iter().map(|&n| m[n].data.addr()).collect();
    assert_eq!(objects, vec![b.addr(), b1.addr(), a.addr()]);
    assert_eq!(nodes.iter().map(|&n| (m[n].kind, m[n].nb_sub)).collect::<Vec<_>>(), vec![(2, 1), (3, 0), (2, 0)]);
    // every element is tied to its node and gave it its display state
    for (k, object) in [a, b, b1].into_iter().enumerate() {
        let node = g::find_gl_data(&m, list, object.cast(), 0);
        assert_eq!(m[elems.add(k as i32)].aux_data.addr(), node.addr());
        assert_eq!((m[node].selection, m[node].wire_type), (1, 1));
    }
}

#[test]
fn the_seam_is_closed_by_wired() {
    let make = |m: &mut Mem| {
        g::graf_list_create(m);
        let list = g::scene_list(m);
        let mut o = crate::NoOutside::default();
        let node = g::graflist_add_data(m, &mut o, list, Ptr32::new(0x7100), 0x15, 1);
        (list, node)
    };
    // alone, the Delete command of a selected node is a call by address …
    let mut m = image();
    let (list, node) = make(&mut m);
    let mut o = crate::NoOutside::default();
    g::graflist_delete_selection(&mut m, &mut o, list, node);
    assert_eq!(o.calls.last().map(|c| c.0.name), Some("SI_graflistDeleteNode"));
    assert_eq!(chain(&m, list), vec![node]);
    // … and under `Wired` the port runs: the node leaves the chain. The Delete command ends
    // with `SI_schemComputeHierarchiesBBox`, whose port reads the current dictionary (here
    // one MODELS chapter without elements) and the scene's element, which the world makes
    let mut m = image();
    let (list, node) = make(&mut m);
    let (ed, _) = schem_dict(&mut m, 0, &[]);
    m[SI_ELEM_DICTS] = ed;
    let scene = m.si_calloc(1, 0x84);
    let mut o = crate::Wired::new(SceneWorld(crate::NoOutside::default(), scene));
    g::graflist_delete_selection(&mut m, &mut o, list, node);
    assert!(chain(&m, list).is_empty());
    assert!(o.inner.0.calls.iter().any(|c| c.0.name == "SI_schematicCheckDeleteExpand"));
    assert!(o.inner.0.calls.iter().any(|c| c.0.name == "SI_elemdictCreateDefaultDsc"));
    assert!(m.fault().is_none(), "{:?}", m.fault());
    // no root was measured: the scene's box is four zeros
    assert_eq!((0..4).map(|k| m.u32(scene + 0x2c + 4 * k)).collect::<Vec<_>>(), [0, 0, 0, 0]);
}

/// [`crate::NoOutside`], but `SI_elemdictCreateDefaultDsc` answers the element given.
struct SceneWorld(crate::NoOutside, u32);
impl crate::Outside for SceneWorld {
    fn call(&mut self, m: &mut Mem, routine: &'static crate::Routine, args: &[u32]) -> u32 {
        let v = self.0.call(m, routine, args);
        if routine.addr == crate::outside::SI_ELEMDICT_CREATE_DEFAULT_DSC.addr {
            return self.1;
        }
        v
    }
}

#[test]
fn the_hierarchies_box_takes_in_the_roots() {
    use crate::graphsort as gs;
    let mut m = image();
    // three models, the second a son of the first: two roots
    let (ed, elems) = schem_dict(&mut m, 3, &[(0, 1)]);
    m[SI_ELEM_DICTS] = ed;
    for (k, (x, y, level)) in [(1.0f32, 2.0f32, 0), (5.0, 9.0, 1), (-3.0, 0.5, 0)].into_iter().enumerate() {
        let e = elems.add(k as i32);
        m[e].schem_x = x;
        m[e].schem_y = y;
        m[e].hier_level = level;
        m[e].aux_data = Ptr32::new(0x7200);
    }
    let scene = m.si_calloc(1, 0x84);
    let mut o = SceneWorld(crate::NoOutside::default(), scene);
    gs::schem_compute_hierarchies_bbox(&mut m, &mut o, ed, 0, 0);
    assert!(m.fault().is_none(), "{:?}", m.fault());
    // the roots are marked laid out, and the son by its root's box
    assert_eq!((0..3).map(|k| m[elems.add(k)].work_flag).collect::<Vec<_>>(), [1, 1, 1]);
    // the scene's box: the lowest corner of the roots' boxes and the highest (a box is
    // the position, its son's taken in, plus 2 and 0.5 at the top)
    let b: Vec<f32> = (0..4).map(|k| m.f32(scene + 0x2c + 4 * k)).collect();
    assert_eq!(b, [-3.0, 0.5, 7.0, 9.5]);
}

/// A dictionary of one chapter (row MODELS of `SI_cid`, the other rows past it) with `n`
/// elements, each with one backward list holding the relations 110 to the sons `sons`
/// gives it: (father, son).
fn schem_dict(m: &mut Mem, n: i32, sons: &[(i32, i32)]) -> (Ptr32<si_types::dict::ElemDict>, Ptr32<si_types::dict::Elem>) {
    use si_types::dict::{ARelation, Elem, ElemDict, ElemList, RelList};
    let mut ids = [0i32; 36];
    for (k, id) in ids.iter_mut().enumerate() {
        *id = 100 + k as i32;
    }
    ids[cid::MODELS] = 0;
    m[SI_CID] = ids;
    let elems: Ptr32<Elem> = Ptr32::new(m.si_calloc(n as u32, 0x84));
    for e in 0..n {
        let mine: Vec<i32> = sons.iter().filter(|s| s.0 == e).map(|s| s.1).collect();
        let list: Ptr32<RelList> = Ptr32::new(m.si_calloc(1, 0x18));
        let rels: Ptr32<ARelation> = Ptr32::new(m.si_calloc(mine.len().max(1) as u32, 0x20));
        for (k, &son) in mine.iter().enumerate() {
            let r = rels.add(k as i32);
            m[r].relid = 110;
            m[r].elemid = son;
        }
        m[list].nb = mine.len() as i32;
        m[list].rels = rels;
        let el = elems.add(e);
        m[el].backward.nb_groups = 1;
        m[el].backward.groups = list;
    }
    let chapter: Ptr32<ElemList> = Ptr32::new(m.si_calloc(1, 0x40));
    m[chapter].nb_elems = n;
    m[chapter].elems = elems;
    let ed: Ptr32<ElemDict> = Ptr32::new(m.si_calloc(1, 0x70));
    m[ed].nb_lists = 1;
    m[ed].lists = chapter;
    (ed, elems)
}

#[test]
fn an_elements_box_takes_in_its_sons() {
    use crate::graphsort as gs;
    let mut m = image();
    let (ed, elems) = schem_dict(&mut m, 3, &[(0, 1), (1, 2)]);
    let at = |k: i32| elems.add(k);
    for (k, (x, y)) in [(0.0f32, 0.0f32), (-3.0, 5.0), (16777215.0, -1.0)].into_iter().enumerate() {
        m[at(k as i32)].schem_x = x;
        m[at(k as i32)].schem_y = y;
    }
    let corners = m.si_calloc(1, 16);
    for (k, v) in [10.0f32, 10.0, -10.0, -10.0].into_iter().enumerate() {
        m.set_f32(corners + 4 * k as u32, v);
    }
    let mut o = crate::NoOutside::default();
    gs::schem_compute_elem_bbox(&mut m, &mut o, ed, at(0), 0, corners, corners + 8, 0);
    assert!(m.fault().is_none(), "{:?}", m.fault());
    // the margins: 2 across and 0.5 up, the sum rounded once (16777217 is not a float)
    assert_eq!(m[at(2)].schem_bbox, [16777215.0, -1.0, 16777216.0, -0.5]);
    assert_eq!(m[at(0)].schem_bbox, [-3.0, -1.0, 16777216.0, 5.5]);
    assert_eq!((m[at(0)].work_flag, m[at(1)].work_flag, m[at(2)].work_flag), (0, 1, 1));
    // the corners passed grow by the box without its margins
    let got: Vec<f32> = (0..4).map(|k| m.f32(corners + 4 * k)).collect();
    assert_eq!(got, vec![-3.0, -1.0, 16777215.0, 5.0]);
    assert!(o.calls.is_empty());
    // with `done`, a son already laid out is passed over
    m[at(0)].schem_bbox = [0.0; 4];
    gs::schem_compute_elem_bbox(&mut m, &mut o, ed, at(0), 0, corners, corners + 8, 1);
    assert_eq!(m[at(0)].schem_bbox, [0.0, 0.0, 2.0, 0.5]);
}

#[test]
fn a_hierarchy_operation_is_refused_or_dispatched() {
    use crate::graphsort as gs;
    let mut m = image();
    let (ed, elems) = schem_dict(&mut m, 2, &[(0, 1)]);
    let e = elems;
    m[e].schem_collapse = 1;
    let mut o = crate::NoOutside::default();
    // 10 wants a hierarchy that is not folded; 20 does nothing while no view is laid out
    assert_eq!(gs::schematic_hier_operation(&mut m, &mut o, 0, ed, e, 0, 10), 0);
    assert_eq!(gs::schematic_hier_operation(&mut m, &mut o, 0, ed, e, 0, 20), 1);
    assert!(o.calls.is_empty());
    // 14 on the named selections' chapter goes to their own routine
    let groups = m[SI_CID][cid::GROUPS];
    assert_eq!(gs::schematic_hier_operation(&mut m, &mut o, 0, ed, e, groups, 14), 1);
    assert_eq!(o.calls.last().map(|c| (c.0.name, c.1.clone())), Some(("schemCollapseNslElem", vec![0, ed.addr(), e.addr(), 14])));
    // 22 measures the element and lays the hierarchies out again
    m[elems.add(1)].schem_x = 4.0;
    assert_eq!(gs::schematic_hier_operation(&mut m, &mut o, 0, ed, e, 0, 22), 1);
    assert_eq!(m[e].schem_bbox, [0.0, 0.0, 6.0, 0.5]);
    assert_eq!(o.calls.last().map(|c| (c.0.name, c.1.clone())), Some(("SI_schemComputeHierarchiesBBox", vec![ed.addr(), 1, 0])));
    // a deleted element is refused before anything
    m[e].status = 2;
    assert_eq!(gs::schematic_hier_operation(&mut m, &mut o, 0, ed, e, 0, 22), 0);
}
