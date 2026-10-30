// SPDX-License-Identifier: MIT
//! Checks of the routines that work on trees of nodes and on a node's list of definitions
//! (vectors of `emu_gen2.py`, group "tree").

use si_model::hierarchy::{self as h, ChainCounters, DefSlot};
use si_model::node::{Current, Models};
use si_model::transform::Globals;
use si_model::NodeId;
use si_x87::vectors::{Case, Check, Verdict, Word};

use crate::life::{globals, node_arg, on_nodes};
use crate::world::{load, same_defs, same_new_node, same_node, tag_of, tagged, Space, Trace};

const N: usize = 6;

fn cut_father(c: &Case) -> Verdict {
    on_nodes(c, N, &[], |m, t| h::cut_father(m, node_arg(c, "node").unwrap(), t))
}

fn dispose(c: &Case) -> Verdict {
    on_nodes(c, N, &[], |m, t| h::dispose(m, node_arg(c, "pn").unwrap(), c.input("brothers").i16() != 0, &Globals::default(), t))
}

/// `SI_modelCopy`: the blocks of 0x430 bytes the original allocated are the new nodes, in
/// order; the port's new nodes are numbered from N in the order it makes them.
fn copy(c: &Case) -> Verdict {
    let (mut models, mut space) = load(c, N);
    models.arena.counter = c.input("cnt").u32() as i32;
    let new = c.output("new").words().to_vec();
    for (i, w) in new.iter().enumerate() {
        if let Word::Ref { name, .. } = w {
            space.alias.insert(name.clone(), format!("m{}", N + i));
        }
    }
    let mut t = Trace::new(c, space, &["SI_meshOptimizeDisplay"]);
    t.selection = Some(c.input("gn").words()[2].value().unwrap_or(0) as i32);
    let top = h::copy(&mut models, node_arg(c, "node").unwrap(), node_arg(c, "father"), &globals(c), &mut t);
    let mut v = t.verdict().and(|| Verdict::from(models.len() == N + new.len(), || format!("{} new nodes against the port's {}", new.len(), models.len() - N)));
    v = v.and(|| Verdict::from(t.space.of(&c.output("ptr").words()[0]) == Models::ptr(top).0, || "another node is returned".to_string()));
    for i in 0..new.len() {
        if v != Verdict::Same {
            break;
        }
        let (id, name) = (N + i, format!("new{i}"));
        let m = *models.m(id);
        let links: Vec<u32> = c.output(&format!("{name}.links")).words().iter().map(|w| t.space.of(w)).collect();
        v = same_new_node(c, &name, &m)
            .and(|| Verdict::from(c.output(&format!("{name}.name")).to_bytes() == models.data[id].name, || format!("{name}: name")))
            .and(|| Verdict::from(links == [m.father.0, m.brother.0, m.son.0, m.latdep.0], || format!("{name}: links {links:x?} against {:x?}", [m.father.0, m.brother.0, m.son.0, m.latdep.0])));
    }
    v
}

fn def_count(c: &Case) -> Verdict {
    let (models, _) = load(c, 1);
    let got = h::def_count(&models, Some(0)) ^ crate::world::mutate() as u32;
    Verdict::from(c.output("eax").u32() == got, || format!("the port counts {got}"))
}

fn slot_handle(models: &Models, slot: Option<DefSlot>) -> u32 {
    let d = &models.data[0];
    match slot {
        None => 0,
        Some(DefSlot::Stored(i)) => tag_of(&d.defs[i]),
        Some(DefSlot::Current) => match &d.current {
            Current::Own(b) => tag_of(b),
            Current::Def(i) => tag_of(&d.defs[*i]),
            Current::None => 0,
        },
        Some(DefSlot::Original) => d.org_def.as_deref().map_or(0, tag_of),
    }
}

fn next_def_get(c: &Case) -> Verdict {
    let (models, mut space) = load(c, 1);
    let mut cursor = c.input("cursor").i32();
    let got = slot_handle(&models, h::next_def_get(&models, 0, &mut cursor)) ^ crate::world::mutate() as u32;
    let want = space.of(&c.output("ptr").words()[0]);
    Verdict::from(got == want, || format!("the original answers {}, the port {}", space.show(want), space.show(got)))
        .and(|| Verdict::from(c.after("cursor").i32s() == [cursor], || format!("the port's cursor is {cursor}")))
}

fn get_data(c: &Case) -> Verdict {
    let (models, mut space) = load(c, 1);
    // (a node whose array pointer is set while it counts no definitions is the generator's
    // own; the original then answers what the array holds)
    if models.m(0).nb_defs == 0 && !models.m(0).defs.is_null() && c.input("which").i32() == 0 {
        return Verdict::Skip;
    }
    let got = slot_handle(&models, h::get_data(&models, 0, c.input("which").i32(), c.input("index").i32())) ^ crate::world::mutate() as u32;
    let want = space.of(&c.output("ptr").words()[0]);
    Verdict::from(got == want, || format!("the original answers {}, the port {}", space.show(want), space.show(got)))
}

fn next_def_set(c: &Case) -> Verdict {
    let mut cursor = c.input("cursor").i32();
    let mut more = false;
    on_nodes(c, 1, &[], |m, t| {
        let new = tagged(t.space.of(&c.input("new").words()[0]));
        more = h::next_def_set(m, 0, &mut cursor, new, t);
    })
    .and(|| Verdict::from((c.output("eax").u32() & 0xff != 0) == more, || format!("the port answers {more}")))
    .and(|| Verdict::from(c.after("cursor").i32s() == [cursor], || format!("the port's cursor is {cursor}")))
}

fn check_create_definition(c: &Case) -> Verdict {
    on_nodes(c, 1, &[], |m, t| h::check_create_definition(m, 0, t))
}

/// What `defs_after` of the generator dumps, against node `id`.
fn same_after(c: &Case, models: &Models, id: NodeId, space: &mut Space) -> Verdict {
    let d = &models.data[id];
    let want: Vec<u32> = c.output("after.defs").words().iter().map(|w| space.of(w)).collect();
    let got: Vec<u32> = d.defs.iter().map(tag_of).collect();
    let cur = space.of(&c.output("after.cur").words()[0]);
    let got_cur = match &d.current {
        Current::None => 0,
        Current::Def(i) => got[*i],
        Current::Own(b) => tag_of(b),
    };
    Verdict::from(want == got, || format!("definitions {want:x?} against the port's {got:x?}"))
        .and(|| Verdict::from(cur == got_cur, || format!("working definition {cur:x} against the port's {got_cur:x}")))
        .and(|| Verdict::from(c.output("after.name").to_bytes() == d.name, || format!("name {:?}", String::from_utf8_lossy(&d.name))))
        .and(|| same_new_node(c, "after.rec", models.m(id)))
}

fn add_definition(c: &Case) -> Verdict {
    let (mut models, mut space) = load(c, 1);
    let new = tagged(space.of(&c.input("new").words()[0]));
    let ok = h::add_definition(&mut models, 0, c.input("kind").i32(), new);
    Verdict::from(c.output("eax").u32() == ok as u32, || "result".to_string()).and(|| same_after(c, &models, 0, &mut space))
}

fn get_node(c: &Case) -> Verdict {
    let mut models = Models::default();
    let mut space = Space::default();
    models.arena.counter = c.input("cnt").u32() as i32;
    let new = tagged(space.of(&c.input("new").words()[0]));
    let name = match &c.input("name").words()[0] {
        Word::Ref { name, .. } => {
            let b = c.input(name).to_bytes();
            Some(b[..b.iter().position(|&x| x == 0).unwrap_or(b.len())].to_vec())
        }
        _ => None,
    };
    let id = h::get_node(&mut models, new, c.input("kind").i32(), name.as_deref());
    same_after(c, &models, id, &mut space).and(|| Verdict::from(c.after("cnt").u32() == models.arena.counter as u32, || "the counter".to_string()))
}

fn change_name(c: &Case) -> Verdict {
    let (mut models, mut space) = load(c, 1);
    let Word::Ref { name, .. } = &c.input("name").words()[0] else { return Verdict::Skip };
    let b = c.input(name).to_bytes();
    h::change_name(&mut models, 0, &b);
    same_after(c, &models, 0, &mut space)
}

fn counters(c: &Case) -> ChainCounters {
    ChainCounters { root: c.input("cr").i16(), joint: c.input("cj").i16() }
}

fn same_counters(c: &Case, k: &ChainCounters) -> Verdict {
    Verdict::from(c.after("cr").i16s() == [k.root] && c.after("cj").i16s() == [k.joint], || format!("the port's counters are {k:?}"))
}

fn ske_chain_rename(c: &Case) -> Verdict {
    let mut k = counters(c);
    on_nodes(c, N, &[], |m, t| h::ske_chain_rename(m, node_arg(c, "node").unwrap(), c.input("root").i16() != 0, &mut k, t)).and(|| same_counters(c, &k))
}

fn rename(c: &Case) -> Verdict {
    let mut k = counters(c);
    on_nodes(c, N, &[], |m, t| h::rename(m, node_arg(c, "node").unwrap(), c.input("recurse").i16() != 0, c.input("root").i16() != 0, &mut k, t)).and(|| same_counters(c, &k))
}

pub fn lookup(name: &str) -> Option<Check> {
    let _ = (same_node, same_defs);
    Some(match name {
        "SI_modelCutFather" => cut_father,
        "SI_modelDispose" => dispose,
        "SI_modelCopy" => copy,
        "SI_modelDefCount" => def_count,
        "SI_modelNextDefGet" => next_def_get,
        "SI_modelNextDefSet" => next_def_set,
        "SI_modelGetData" => get_data,
        "SI_modelCheckCreateDefinition" => check_create_definition,
        "SI_modelAddDefinition" => add_definition,
        "SI_modelGetNode" => get_node,
        "SI_modelChangeName" => change_name,
        "SI_skeChainRename" => ske_chain_rename,
        "SI_modelRename" => rename,
        _ => return None,
    })
}
