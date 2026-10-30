// SPDX-License-Identifier: MIT
use super::*;
use crate::mesh::Mesh;
use crate::node::{MJNT, MNILL};

/// root(0) with sons a(1) and b(2); a has c(3) and d(4); d has f(6); b has e(5).
fn tree() -> Models {
    let mut m = Models::default();
    for name in ["root", "a", "b", "c", "d", "e", "f"] {
        m.allocate(Some(name.as_bytes()));
    }
    for (father, son) in [(0, 1), (0, 2), (1, 3), (1, 4), (2, 5), (4, 6)] {
        assert!(m.arena.create_parentship(father, son));
    }
    m
}

#[test]
fn depth_first_orders() {
    let m = tree();
    let mut order = vec![0];
    while let Some(n) = next_node(&m, *order.last().unwrap()) {
        order.push(n);
    }
    assert_eq!(order, [0, 1, 3, 4, 6, 2, 5]);
    // from inside a subtree the walk runs on into the rest of the tree
    assert_eq!(next_node(&m, 6), Some(2));
    // the subtree walk does not
    let mut sub = Vec::new();
    let mut at = None;
    while let Some(n) = next_son(&m, 1, at) {
        sub.push(n);
        at = Some(n);
    }
    assert_eq!(sub, [3, 4, 6]);
    assert_eq!(next_son(&m, 6, Some(6)), None);
    assert_eq!(root(&m, 6), 0);
    assert_eq!((nb_sub_models(&m, 0), nb_sub_models(&m, 1), nb_sub_models(&m, 2), nb_sub_models(&m, 6)), (6, 3, 1, 0));
    assert_eq!((model_depth(&m, Some(6)), model_depth(&m, Some(0)), model_depth(&m, None)), (4, 1, 0));
}

#[test]
fn ancestors_and_names() {
    let m = tree();
    assert!(is_father(&m, 1, Some(6)) && is_father(&m, 6, Some(6)) && !is_father(&m, 2, Some(6)) && !is_father(&m, 0, None));
    assert_eq!(common_ancestor(&m, Some(6), Some(5)), Some(0));
    assert_eq!(common_ancestor(&m, Some(3), Some(6)), Some(1));
    assert_eq!(common_ancestor(&m, Some(1), Some(6)), Some(1));
    assert_eq!(common_ancestor(&m, Some(6), None), None);
    // the search leaves the subtree it started in
    assert_eq!(node_find_by_name(&m, Some(1), b"e"), Some(5));
    assert_eq!(node_find_by_name(&m, Some(2), b"c"), None);
    assert_eq!(node_find_by_name(&m, Some(0), b"d\0junk"), Some(4));
    let mut other = tree();
    assert!(compare_topology(&m, 0, 0));
    other.allocate(Some(b"g"));
    other.arena.create_parentship(5, 7);
    // compared through a second arena: copy the shape into one
    let mut both = tree();
    let copy = super::copy(&mut both, 0, None, &Globals::default(), &mut ());
    assert!(compare_topology(&both, 0, copy));
    let extra = both.allocate(None);
    both.arena.create_parentship(copy, extra);
    assert!(!compare_topology(&both, 0, copy));
}

/// Notes the order in which nodes are freed: the first thing `SI_modelDisposeNode` does to
/// a node is to take its animation off.
struct Log(Vec<String>);
impl Hooks for Log {}
impl crate::transform::Hooks for Log {}
impl crate::transform::GeometryHooks for Log {}
impl crate::node_copy::Hooks for Log {
    fn model_reset_all(&mut self, models: &mut Models, node: NodeId) {
        self.0.push(String::from_utf8_lossy(&models.data[node].name).into_owned());
    }
}

#[test]
fn dispose_frees_later_brothers_and_deeper_nodes_first() {
    let mut m = tree();
    let mut log = Log(Vec::new());
    dispose(&mut m, 0, false, &Globals::default(), &mut log);
    assert_eq!(log.0, ["e", "b", "f", "d", "c", "a", "root"]);
    // a subtree alone: its father's list closes over it
    let mut m = tree();
    let mut log = Log(Vec::new());
    dispose(&mut m, 1, false, &Globals::default(), &mut log);
    assert_eq!(log.0, ["f", "d", "c", "a"]);
    assert_eq!(m.son(0), Some(2));
    assert!(!m.data[2].disposed && m.data[4].disposed);
}

#[test]
fn copy_makes_the_node_then_its_brothers_then_its_sons() {
    let mut m = tree();
    let top = copy(&mut m, 0, None, &Globals::default(), &mut ());
    let names: Vec<_> = (7..m.len()).map(|i| String::from_utf8_lossy(&m.data[i].name).into_owned()).collect();
    assert_eq!(names, ["root", "a", "b", "e", "c", "d", "f"]);
    assert_eq!(top, 7);
    let mut order = vec![top];
    while let Some(n) = next_node(&m, *order.last().unwrap()) {
        order.push(n);
    }
    assert_eq!(order, [7, 8, 11, 12, 13, 9, 10]);
    // a copy without a father leaves the source's brothers behind
    let only = copy(&mut m, 1, None, &Globals::default(), &mut ());
    assert_eq!(m.brother(only), None);
    assert_eq!(nb_sub_models(&m, only), 3);
}

#[test]
fn cutting_closes_the_list_and_keeps_the_sons() {
    let mut m = tree();
    cut_father(&mut m, 4, &mut ());
    assert_eq!((m.father(4), m.brother(3), m.son(4)), (None, None, Some(6)));
    cut_father(&mut m, 1, &mut ());
    assert_eq!((m.son(0), m.brother(1), m.father(1)), (Some(2), None, None));
    cut_father(&mut m, 0, &mut ()); // a root: nothing to do
    assert_eq!(m.son(0), Some(2));
}

struct Names(i32);
impl Hooks for Names {
    fn new_name_id(&mut self, prefix: &[u8]) -> (Vec<u8>, i32) {
        self.0 += 1;
        (format!("{}{}", String::from_utf8_lossy(prefix), self.0).into_bytes(), self.0)
    }
}

#[test]
fn skeleton_chains_are_renamed_and_nothing_else() {
    // top(0) → chain root(1) → joint(2) → joint(3) → effector(4) → chain root(5) → joint(6) → effector(7); plus a cube(8) under top
    let mut m = Models::default();
    for _ in 0..9 {
        m.allocate(None);
    }
    for (f, s) in [(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (6, 7), (0, 8)] {
        m.arena.create_parentship(f, s);
    }
    for (n, kind, algo) in [(1, MNILL, 1), (2, MJNT, 0), (3, MJNT, 0), (4, MNILL, 3), (5, MNILL, 2), (6, MJNT, 0), (7, MNILL, 4), (8, 4, 0)] {
        m.m_mut(n).kind = kind;
        m.m_mut(n).algo_type = algo;
    }
    let mut c = ChainCounters::default();
    rename(&mut m, 0, true, true, &mut c, &mut Names(0));
    let names: Vec<_> = (0..9).map(|i| String::from_utf8_lossy(&m.data[i].name).into_owned()).collect();
    assert_eq!(names, ["obj1", "chn1", "jnt1_1", "jnt1_2", "eff1", "chn2", "jnt2_1", "eff2", "obj9"]);
    assert_eq!(c, ChainCounters { root: 2, joint: 1 });
    assert_eq!(m.m(2).update_flags & 1, 1);
    // without renaming the root the number is one less than the dictionary's next
    let mut c = ChainCounters::default();
    ske_chain_rename(&mut m, 1, false, &mut c, &mut Names(6));
    assert_eq!(m.data[1].name, b"chn1");
    assert_eq!(m.data[2].name, b"jnt6_1");
    assert_eq!(m.data[4].name, b"eff6");
    // not a chain root: untouched
    ske_chain_rename(&mut m, 8, true, &mut c, &mut Names(0));
    assert_eq!(m.data[8].name, b"obj9");
}

fn mesh(n: usize) -> Definition {
    let mut m = Mesh::with_vertices(&vec![[0.0; 3]; n]);
    m.push_polygon(&(0..n as i32).collect::<Vec<_>>());
    Definition::Mesh(m)
}

#[test]
fn definitions_stored_working_and_original() {
    let mut m = Models::default();
    let n = get_node(&mut m, mesh(3), 4, Some(b"tri"));
    assert_eq!((m.m(n).kind, m.m(n).nb_defs, m.data[n].current.clone()), (4, 1, Current::Def(0)));
    assert_eq!(m.m(n).update_flags & 1, 1);
    add_definition(&mut m, n, 4, mesh(4));
    assert_eq!(def_count(&m, Some(n)), 2);
    assert_eq!(def_count(&m, None), 0);
    // the walk: stored, stored, (no working copy), original (none) → ends
    let mut cur = 0;
    assert_eq!(next_def_get(&m, n, &mut cur), Some(DefSlot::Stored(0)));
    assert_eq!(next_def_get(&m, n, &mut cur), Some(DefSlot::Stored(1)));
    assert_eq!(next_def_get(&m, n, &mut cur), None);
    assert_eq!(cur, 4);
    assert_eq!(next_def_get(&m, n, &mut cur), None);
    // a working copy is made from the first stored definition
    check_create_definition(&mut m, n, &mut ());
    assert!(matches!(m.data[n].current, Current::Own(_)));
    assert_eq!(def_count(&m, Some(n)), 3);
    check_create_definition(&mut m, n, &mut ()); // already has one: nothing
    m.data[n].org_def = Some(Box::new(mesh(5)));
    let mut cur = 0;
    let mut seen = Vec::new();
    while let Some(s) = next_def_get(&m, n, &mut cur) {
        seen.push(s);
    }
    assert_eq!(seen, [DefSlot::Stored(0), DefSlot::Stored(1), DefSlot::Current, DefSlot::Original]);
    // replacing them all, the answer says whether one is left
    let mut cur = 0;
    assert!(next_def_set(&mut m, n, &mut cur, mesh(6), &mut ()));
    assert!(next_def_set(&mut m, n, &mut cur, mesh(6), &mut ()));
    assert!(next_def_set(&mut m, n, &mut cur, mesh(6), &mut ()));
    assert!(!next_def_set(&mut m, n, &mut cur, mesh(6), &mut ()));
    assert!((0..2).all(|i| matches!(&m.data[n].defs[i], Definition::Mesh(x) if x.vertices.len() == 6)));
    assert!(matches!(definition(&m, n, DefSlot::Original), Some(Definition::Mesh(x)) if x.vertices.len() == 6));
    // triangles: the stored definition number cur_def, the branch sons first
    let k = get_node(&mut m, mesh(5), 4, None);
    m.arena.create_parentship(n, k);
    assert_eq!(nb_triangles(&m, n, &mut ()), 4);
    assert_eq!(nb_triangles_branch(&m, n, &mut ()), 4 + 3);
    assert_eq!(m.data[k].name, b"obj2");
}
