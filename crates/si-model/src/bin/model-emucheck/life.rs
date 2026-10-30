// SPDX-License-Identifier: MIT
//! Checks of the node routines of `node_copy.rs` (vectors of `emu_gen2.py`, group "life").

use si_model::node::Models;
use si_model::node_copy as nc;
use si_model::transform::Globals;
use si_model::NodeId;
use si_x87::vectors::{Case, Verdict, Word};

use crate::world::{load, same_defs, same_extras, same_node, update_nodes, Space, Trace};

pub fn node_arg(c: &Case, name: &str) -> Option<NodeId> {
    match &c.input(name).words()[0] {
        Word::Ref { name, .. } => name.strip_prefix('m').and_then(|k| k.parse().ok()),
        _ => None,
    }
}

/// Runs a port on the nodes `m0` … of a case, then compares the calls it made and every
/// node with the emulator's.
pub fn on_nodes(c: &Case, count: usize, skip: &[&str], run: impl FnOnce(&mut Models, &mut Trace)) -> Verdict {
    let (mut models, mut space) = load(c, count);
    let upd = update_nodes(c, &mut space, count);
    // SI_modelMultiplyDef is a stub in these vectors (the definitions are opaque blocks), so
    // what the port's own multiply_def calls for a NURBS definition has no counterpart
    let mut skip = skip.to_vec();
    if !c.name.starts_with("SI_modelMultiplyDefinition") {
        skip.extend(["SI_nurbsCrvApplyMatrix", "SI_nurbsSrfApplyMatrix"]);
    }
    let mut t = Trace::new(c, space, &skip);
    t.upd = upd;
    t.graf = graf_list(c, &mut t.space);
    si_model::env::stale::reset();
    run(&mut models, &mut t);
    if si_model::env::stale::count() != 0 {
        // the original went on with stack bytes after an inverse that does not exist; in a
        // routine that calls others those bytes are an earlier call's, so the harness sees
        // nothing undefined, but the port cannot have them
        return Verdict::Skip;
    }
    let mut v = t.verdict();
    for k in 0..count {
        if v != Verdict::Same {
            break;
        }
        let name = format!("m{k}");
        v = same_node(c, &name, &models, k, &mut t.space).and(|| same_defs(c, &name, &models, k, &mut t.space)).and(|| same_extras(c, &models, k, &t.space));
    }
    v
}

/// The graf nodes `gn0`, `gn1`, … of a case, in the order of the list.
pub fn graf_list(c: &Case, space: &mut Space) -> Option<Vec<(i32, u32, i32)>> {
    c.try_input("gn0")?;
    let mut out = Vec::new();
    let mut k = 0;
    while let Some(f) = c.try_input(&format!("gn{k}")) {
        let w = f.words();
        out.push((w[0].value().unwrap_or(0) as i32, space.of(&w[1]), w[2].value().unwrap_or(0) as i32));
        k += 1;
    }
    Some(out)
}

pub fn dispose_node(c: &Case) -> Verdict {
    on_nodes(c, 2, &[], |m, t| nc::dispose_node(m, 0, &Globals::default(), t))
}

pub fn delete_definition(c: &Case) -> Verdict {
    on_nodes(c, 1, &[], |m, t| nc::delete_definition(m, 0, c.input("keep").i16() != 0, t))
}

pub fn copy_org_def(c: &Case) -> Verdict {
    on_nodes(c, 1, &[], |m, t| {
        let def = t.space.of(&c.input("def").words()[0]);
        let d = (def != 0).then(|| crate::world::tagged(def));
        nc::copy_org_def(m, 0, d.as_ref(), t)
    })
}

pub fn update_dependency(c: &Case) -> Verdict {
    on_nodes(c, 5, &[], |m, t| nc::update_dependency(m, node_arg(c, "node").unwrap(), node_arg(c, "holder"), &Globals::default(), t))
}

pub fn touch_geometry(c: &Case) -> Verdict {
    on_nodes(c, 5, &[], |m, t| nc::touch_geometry(m, node_arg(c, "node").unwrap(), &Globals::default(), t))
}

pub fn touch_definition(c: &Case) -> Verdict {
    on_nodes(c, 5, &[], |m, t| nc::touch_definition(m, node_arg(c, "node").unwrap(), t))
}

pub fn remove_all_fittings(c: &Case) -> Verdict {
    on_nodes(c, 5, &[], |m, t| nc::remove_all_fittings(m, node_arg(c, "node").unwrap(), &Globals::default(), t))
}

pub fn spline_fitting_reset(c: &Case) -> Verdict {
    on_nodes(c, 5, &[], |m, t| nc::spline_fitting_reset(m, node_arg(c, "node").unwrap(), c.input("which").i16() != 0, &Globals::default(), t))
}

pub fn patch_fitting_reset(c: &Case) -> Verdict {
    on_nodes(c, 5, &[], |m, t| nc::patch_fitting_reset(m, node_arg(c, "node").unwrap(), c.input("which").i16() != 0, &Globals::default(), t))
}

/// A chain of user data from the head in `list`: the buffers are those of node 0's group,
/// so the chain is rebuilt by giving a made-up node that head.
pub fn user_data_dispose(c: &Case) -> Verdict {
    let mut space = Space::default();
    let head = c.input("list").words()[0].clone();
    let mut chain = crate::world::user_chain(c, &mut space, head.clone());
    let mut h = space.of(&head);
    let mut t = Trace::new(c, space, &[]);
    nc::user_data_dispose(&mut chain, &mut h, &mut t);
    t.verdict().and(|| Verdict::from(h == 0 && c.output("list").u32s() == [0], || "the head is not cleared".to_string()))
}

pub fn subel_user_data_dispose(c: &Case) -> Verdict {
    let mut space = Space::default();
    let (cl, arr) = (c.input("cl").words()[0].clone(), c.input("arr").words()[0].clone());
    let mut data = crate::world::subel(c, &mut space, &cl, &arr);
    let (mut hc, mut ha) = (space.of(&cl), space.of(&arr));
    let mut t = Trace::new(c, space, &[]);
    nc::subel_user_data_dispose(&mut data, &mut hc, &mut ha, &mut t);
    t.verdict().and(|| Verdict::from(hc == 0 && ha == 0 && c.output("cl").u32s() == [0] && c.output("arr").u32s() == [0], || "a pointer is not cleared".to_string()))
}

/// A predicate that answers a pointer, and for three of them which of the two it is.
fn predicate(c: &Case, port: fn(&Models, NodeId) -> Option<(u32, Option<i16>)>) -> Verdict {
    let (models, mut space) = load(c, 1);
    let got = port(&models, 0);
    let want = space.of(&c.output("ptr").words()[0]);
    Verdict::from(got.map_or(0, |g| g.0) ^ crate::world::mutate() as u32 == want, || format!("the original answers {want:#x}, the port {got:?}")).and(|| match (c.try_output("which"), got) {
        (Some(w), Some((_, Some(which)))) => Verdict::from(w.written() == [Some(which as u16 as u128)], || format!("which: {:?} against {which}", w.written())),
        (Some(w), None) => Verdict::from(w.written() == [None], || "the original writes which".to_string()),
        _ => Verdict::Same,
    })
}

pub fn lattice(c: &Case) -> Verdict {
    predicate(c, |m, n| nc::model_lattice(m, n).map(|(p, w)| (p.0, Some(w))))
}
pub fn spline_fitting(c: &Case) -> Verdict {
    predicate(c, |m, n| nc::model_spline_fitting(m, n).map(|(p, w)| (p, Some(w))))
}
pub fn patch_fitting(c: &Case) -> Verdict {
    predicate(c, |m, n| nc::model_patch_fitting(m, n).map(|(p, w)| (p, Some(w))))
}
pub fn tree_lattice(c: &Case) -> Verdict {
    predicate(c, |m, n| nc::model_tree_lattice(m, n).map(|p| (p.0, None)))
}
pub fn node_wave(c: &Case) -> Verdict {
    predicate(c, |m, n| nc::model_node_wave(m, n).map(|p| (p, None)))
}
pub fn branch_wave(c: &Case) -> Verdict {
    predicate(c, |m, n| nc::model_branch_wave(m, n).map(|p| (p, None)))
}
pub fn node_spline_fitting(c: &Case) -> Verdict {
    predicate(c, |m, n| nc::model_node_spline_fitting(m, n).map(|p| (p, None)))
}
pub fn tree_spline_fitting(c: &Case) -> Verdict {
    predicate(c, |m, n| nc::model_tree_spline_fitting(m, n).map(|p| (p, None)))
}
pub fn node_patch_fitting(c: &Case) -> Verdict {
    predicate(c, |m, n| nc::model_node_patch_fitting(m, n).map(|p| (p, None)))
}
pub fn branch_patch_fitting(c: &Case) -> Verdict {
    predicate(c, |m, n| nc::model_branch_patch_fitting(m, n).map(|p| (p, None)))
}

pub fn globals(c: &Case) -> Globals {
    let d = c.input("dup").u16s();
    Globals {
        classical_scaling: c.input("manip").u32() == 1,
        sumatra_to_3x: c.input("sum").u32() != 0,
        duplic: nc::Duplic { lattice_dup: d[0x1e / 2] != 0, shape_anim: d[0x2a / 2] != 0, deform_anim: d[0x2c / 2] != 0, geometry_curr: d[0x36 / 2] != 0 },
        ..Globals::default()
    }
}

/// `SI_modelCopyNode`: the calls outside, the new record, its name, its definition and its
/// initial transformation. With the buffer `mesh` the source's only definition is a real
/// mesh and the copy's mesh is compared too.
pub fn copy_node(c: &Case) -> Verdict {
    let count = if c.try_input("m1").is_some() { 2 } else { 1 };
    let (mut models, mut space) = load(c, count);
    models.arena.counter = c.input("cnt").u32() as i32;
    if let Word::Ref { name, .. } = &c.output("ptr").words()[0] {
        space.alias.insert(name.clone(), format!("m{count}"));
    }
    let real = c.try_input("mesh").is_some();
    if real {
        crate::allocate_cosine();
        models.data[0].defs[0] = si_model::Definition::Mesh(crate::mesh_of(c));
    }
    let mut t = Trace::new(c, space, &["SI_meshOptimizeDisplay", "SI_showActivity", "SI_statusDisplayMessage", "SI_meshUpdatePolygonLists"]);
    t.selection = Some(c.input("gn").words()[2].value().unwrap_or(0) as i32);
    let copy = nc::copy_node(&mut models, 0, c.input("as_env").i16() != 0, &globals(c), &mut t);
    let d = &models.data[copy];
    let srt: Vec<f32> = d.init_srt.iter().flat_map(|s| s.iter().flat_map(|v| [v.x, v.y, v.z])).collect();
    t.verdict()
        .and(|| crate::world::same_new_node(c, "copy", models.m(copy)))
        .and(|| Verdict::from(c.output("copy.name").to_bytes() == d.name, || format!("the copy is named {:?}", String::from_utf8_lossy(&d.name))))
        .and(|| Verdict::from(c.output("copy.srt").len() == srt.len() && c.output("copy.srt").same_f32(&srt), || "the initial transformation differs".to_string()))
        .and(|| {
            let cur = c.output("copy.cur").u32() == 1;
            Verdict::from(cur == (d.current == si_model::node::Current::Def(0)), || format!("working definition: {:?}", d.current))
        })
        .and(|| match (real, d.defs.first()) {
            (true, Some(si_model::Definition::Mesh(m))) => crate::same_mesh_as(c, m, "copy."),
            (true, _) => Verdict::Different("the port's copy has no mesh".to_string()),
            _ => Verdict::Same,
        })
}

pub fn lookup(name: &str) -> Option<si_x87::vectors::Check> {
    Some(match name {
        "SI_modelDisposeNode" => dispose_node,
        "SI_modelCopyNode" => copy_node,
        "SI_modelDeleteDefinition" => delete_definition,
        "SI_modelCopyOrgDef" => copy_org_def,
        "SI_modelUpdateDependency" => update_dependency,
        "SI_modelTouchGeometry" => touch_geometry,
        "SI_modelTouchDefinition" => touch_definition,
        "SI_modelRemoveAllFittings" => remove_all_fittings,
        "SI_modelSplineFittingReset" => spline_fitting_reset,
        "SI_modelPatchFittingReset" => patch_fitting_reset,
        "SI_userDataDispose" => user_data_dispose,
        "SI_subelUserDataDispose" => subel_user_data_dispose,
        "SI_modelLattice" => lattice,
        "SI_modelSplineFitting" => spline_fitting,
        "SI_modelPatchFitting" => patch_fitting,
        "SI_modelTreeLattice" => tree_lattice,
        "SI_modelNodeWave" => node_wave,
        "SI_modelBranchWave" => branch_wave,
        "SI_modelNodeSplineFitting" => node_spline_fitting,
        "SI_modelTreeSplineFitting" => tree_spline_fitting,
        "SI_modelNodePatchFitting" => node_patch_fitting,
        "SI_modelBranchPatchFitting" => branch_patch_fitting,
        _ => return None,
    })
}
