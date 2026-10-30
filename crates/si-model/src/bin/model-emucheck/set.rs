// SPDX-License-Identifier: MIT
//! Checks of the transformation setters and what they call (vectors of `emu_gen2.py`,
//! group "set"): `setters.rs`, the freeze routines, the readjusting of a definition.

use si_model::node::Models;
use si_model::setters::{self as st, View};
use si_model::transform::{self as tr, Globals};
use si_model::NodeId;
use si_types::geom::Vector;
use si_x87::vectors::{Case, Check, Verdict, Word};

use crate::life::{node_arg, on_nodes};
use crate::world::Trace;

/// The program-wide settings of a case (`SETG` of the generator).
pub fn globals(c: &Case) -> Globals {
    let m = c.input("manip").words();
    let word = |i: usize| m[i].value().unwrap_or(0) as u32;
    let ref_model = match &c.input("refp").words()[1] {
        Word::Ref { name, .. } => name.strip_prefix('m').and_then(|k| k.parse().ok()),
        _ => None,
    };
    let time = c.input("time").words();
    Globals {
        classical_scaling: word(0) == 1,
        rot_mode: word(2) as i32,
        trans_mode: word(3) as i32,
        ref_model,
        hierarchy_manipulation: word(8) != 0,
        keep_turns: word(9) != 0,
        viewer_upd_opt: c.input("upd").u32(),
        init_pose_compensate: c.input("ipc").u16s()[0] != 0,
        envelope_hold: c.input("hold").u16s()[0] as i16,
        update_time: (time[0].value().unwrap_or(0) != 0).then(|| f32::from_bits(time[1].value().unwrap_or(0) as u32)),
        frame_current: c.input("frame").i32(),
        frame_rate: c.input("rate").f32(),
        interactive: c.input("inter").u16s()[0] != 0,
        ..Globals::default()
    }
}

fn count(c: &Case) -> usize {
    (0..16).take_while(|k| c.try_input(&format!("m{k}")).is_some()).count()
}

fn vec3(c: &Case, name: &str) -> Vector {
    let f = c.input(name).f32s();
    Vector { x: f[0], y: f[1], z: f[2] }
}

fn matrix(c: &Case, name: &str) -> si_math::types::Matrix {
    let w = c.input(name).f32s();
    let mut m = [[0.0f32; 4]; 4];
    for i in 0..16 {
        m[i / 4][i % 4] = w[i];
    }
    m
}

/// Runs a port on the case's nodes with the case's settings.
fn run(c: &Case, f: impl FnOnce(&mut Models, NodeId, &Globals, &mut Trace)) -> Verdict {
    let g = globals(c);
    let node = if c.try_input("node").is_some() { node_arg(c, "node").unwrap_or(0) } else { 0 };
    on_nodes(c, count(c), &[], |m, t| f(m, node, &g, t))
}

type Setter = fn(&mut Models, NodeId, Vector, i32, i16, &Globals, &mut dyn si_model::Env);

fn setter(c: &Case, f: Setter) -> Verdict {
    run(c, |m, n, g, t| f(m, n, vec3(c, "v"), c.input("mode").i32(), c.input("geom").i16(), g, t))
}

type Safe = fn(&mut Models, Option<NodeId>, Option<Vector>, bool, i32, i16, &Globals, &mut dyn si_model::Env);

fn safe(c: &Case, f: Safe) -> Verdict {
    run(c, |m, n, g, t| f(m, Some(n), Some(vec3(c, "v")), c.input("local").i16() != 0, c.input("mode").i32(), c.input("geom").i16(), g, t))
}

fn view(c: &Case) -> View {
    let w = c.input("cam").words();
    let f = |i: usize| f32::from_bits(w[i].value().unwrap_or(0) as u32);
    View { position: Vector { x: f(4), y: f(5), z: f(6) }, interest: Vector { x: f(7), y: f(8), z: f(9) } }
}

fn rotation_refer(c: &Case) -> Verdict {
    let g = globals(c);
    let (models, _) = crate::world::load(c, count(c));
    let got = st::matrix_rotation_refer(&models, Some(&view(c)), vec3(c, "v"), &g);
    let flat: Vec<f32> = got.iter().flatten().copied().collect();
    Verdict::from(crate::world::same_f32_prefix(c.output("out"), &flat), || format!("the port's matrix is {flat:?}"))
}

fn state(c: &Case, global: bool) -> Verdict {
    let g = globals(c);
    let (models, _) = crate::world::load(c, count(c));
    let (s, r, t) = if global { tr::global_state(&models, 0, &g) } else { tr::state(&models, 0) };
    let same = |name: &str, v: Vector| Verdict::from(crate::world::same_f32_prefix(c.output(name), &[v.x, v.y, v.z]), || format!("{name}: the port has {v:?}"));
    same("s", s).and(|| same("r", r)).and(|| same("t", t))
}

/// The scratch list of initial poses the routine left, against the port's.
fn same_list(c: &Case, models: &Models) -> Verdict {
    let got: Vec<f32> = models.glb_init_srt.iter().flatten().flat_map(|e| [e.gscl.x, e.gscl.y, e.gscl.z].into_iter().chain(e.grot.iter().flatten().copied()).chain([e.gtrs.x, e.gtrs.y, e.gtrs.z])).collect();
    let want = c.output("list");
    // the original's block has room only for the sons with a pose; computeInit_srtParenting
    // writes past it, which the dump does not show
    let n = want.len().min(got.len());
    let null = matches!(c.after("gis").words()[0], Word::Value { value: 0, .. });
    Verdict::from(null == models.glb_init_srt.is_none(), || format!("the list is {} in the port", if null { "there" } else { "missing" }))
        .and(|| Verdict::from(want.len() <= got.len() && crate::world::same_f32_prefix(want, &got[..n]), || format!("list of {} floats against the port's {}", want.len(), got.len())))
}

/// The settings `SI_modelReadjustDefinition` reads (its cases have no other).
fn readjust_globals(c: &Case) -> Globals {
    let time = c.input("time").words();
    Globals {
        envelope_hold: c.input("hold").u16s()[0] as i16,
        update_time: (time[0].value().unwrap_or(0) != 0).then(|| f32::from_bits(time[1].value().unwrap_or(0) as u32)),
        frame_current: c.input("frame").i32(),
        frame_rate: c.input("rate").f32(),
        interactive: c.input("inter").u16s()[0] != 0,
        ..Globals::default()
    }
}

fn readjust(c: &Case) -> Verdict {
    let g = readjust_globals(c);
    let node = node_arg(c, "node").unwrap();
    on_nodes(c, count(c), &[], |m, t| si_model::node_copy::readjust_definition(m, node, &g, t))
}

/// The walk variant: the working definition of the node is a real spline of two keys
/// (buffers `spl` and `keys`), and what is compared beside the calls is its keys.
fn readjust_walk(c: &Case) -> Verdict {
    let g = readjust_globals(c);
    let node = node_arg(c, "node").unwrap();
    let d = c.input("spl").words();
    let k = c.input("keys").f32s();
    let mut sp = si_hrc::Spline { kind: d[3].value().unwrap_or(0) as i32, ..si_hrc::Spline::default() };
    for q in k.chunks(12) {
        sp.keys.push(si_hrc::SplineKey { prev: [q[0], q[1], q[2]], pos: [q[3], q[4], q[5]], next: [q[6], q[7], q[8]], ..si_hrc::SplineKey::default() });
    }
    let mut got = k.clone();
    let (mut models, mut space) = crate::world::load(c, count(c));
    models.data[node].current = si_model::node::Current::Own(Box::new(si_model::Definition::Spline(sp)));
    let mut t = Trace::new(c, core::mem::take(&mut space), &["SI_splineTransferPoints", "SI_latticeTransform", "SI_splineFit", "SI_patchFit", "SI_splineTouch"]);
    t.upd = crate::world::update_nodes(c, &mut t.space, count(c));
    si_model::node_copy::readjust_definition(&mut models, node, &g, &mut t);
    // (the calls that name the working definition are left out: the port's is not a buffer)
    let keys = match &models.data[node].current {
        si_model::node::Current::Own(b) => match &**b {
            si_model::Definition::Spline(s) => s.keys.clone(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    for (i, key) in keys.iter().enumerate() {
        got[12 * i..12 * i + 3].copy_from_slice(&key.prev);
        got[12 * i + 3..12 * i + 6].copy_from_slice(&key.pos);
        got[12 * i + 6..12 * i + 9].copy_from_slice(&key.next);
    }
    t.verdict().and(|| Verdict::from(crate::world::same_f32_prefix(c.after("keys"), &got), || "the keys differ".to_string()))
}

/// `SI_modelMultiplyDefinition`: the kinds' transform routines in order, and for a metaball
/// (whose definitions the case lays out with a matrix of floats at +0xc) the matrices.
fn multiply_definition(c: &Case) -> Verdict {
    use si_model::node::Current;
    use si_model::Definition;
    let g = readjust_globals(c);
    let (mut models, space) = crate::world::load(c, 1);
    let ball = models.m(0).kind == 8;
    // the buffer each of the node's definitions came from, in the order the port holds them
    let name_of = |d: &Definition| space.show(crate::world::tag_of(d)).trim_start_matches('&').split('+').next().unwrap_or("").to_string();
    let as_ball = |d: &Definition| {
        let f = c.input(&name_of(d)).words();
        let mut b = si_hrc::Metaball::default();
        for i in 0..16 {
            b.matrix[i] = f32::from_bits(f[3 + i].value().unwrap_or(0) as u32);
        }
        Definition::Metaball(b)
    };
    let mut names = Vec::new();
    if ball {
        let d = &mut models.data[0];
        for def in d.defs.iter_mut() {
            names.push(name_of(def));
            *def = as_ball(def);
        }
        if let Current::Own(b) = &mut d.current {
            names.push(name_of(b));
            **b = as_ball(b);
        }
        if let Some(b) = d.org_def.as_mut() {
            names.push(name_of(b));
            **b = as_ball(b);
        }
    }
    let mut t = Trace::new(c, space, &["SI_meshTransform"]);
    tr::multiply_definition(&mut models, 0, &matrix(c, "m"), &g, &mut t);
    let d = &models.data[0];
    let own = match &d.current {
        Current::Own(b) => Some(&**b),
        _ => None,
    };
    let held: Vec<&Definition> = d.defs.iter().chain(own).chain(d.org_def.as_deref()).collect();
    let mut v = t.verdict().and(|| crate::world::same_node(c, "m0", &models, 0, &mut t.space));
    if ball {
        for (name, def) in names.iter().zip(held) {
            let Definition::Metaball(b) = def else { continue };
            let want = c.after(name).written();
            for i in 0..16 {
                if want[3 + i].is_some_and(|w| w as u32 != b.matrix[i].to_bits() ^ crate::world::mutate() as u32) {
                    v = v.and(|| Verdict::Different(format!("{name}: matrix element {i}")));
                }
            }
        }
    }
    v
}

fn lookup_fn(name: &str) -> Option<Check> {
    Some(match name {
        "SI_modelMultiplyDefinition" => multiply_definition,
        "SI_modelReadjustDefinition#walk" => readjust_walk,
        "SI_modelReadjustDefinition" => readjust,
        "SI_modelSetScaling" => |c| setter(c, tr::set_scaling),
        "SI_modelSetRotation" => |c| setter(c, tr::set_rotation),
        "SI_modelSetTranslation" => |c| setter(c, tr::set_translation),
        "SI_modelSetGlobalScaling" => |c| setter(c, st::set_global_scaling),
        "SI_modelSetGlobalRotation" => |c| setter(c, st::set_global_rotation),
        "SI_modelSetGlobalTranslation" => |c| setter(c, st::set_global_translation),
        "SI_modelAddScaling" => |c| setter(c, st::add_scaling),
        "SI_modelAddTranslation" => |c| setter(c, st::add_translation),
        "SI_modelSafeAddScaling" => |c| run(c, |m, n, g, t| st::safe_add_scaling(m, Some(n), Some(vec3(c, "v")), c.input("mode").i32(), c.input("geom").i16(), g, t)),
        "SI_modelSafeSetScaling" => |c| safe(c, st::safe_set_scaling),
        "SI_modelSafeSetRotation" => |c| safe(c, st::safe_set_rotation),
        "SI_modelSafeAddRotation" => |c| safe(c, st::safe_add_rotation),
        "SI_modelSafeSetTranslation" => |c| safe(c, st::safe_set_translation),
        "SI_modelSafeAddTranslation" => |c| safe(c, st::safe_add_translation),
        "SI_modelAddRotation" => |c| run(c, |m, n, g, t| st::add_rotation(m, Some(&view(c)), n, vec3(c, "v"), c.input("mode").i32(), c.input("geom").i16(), g, t)),
        "SI_matrixRotationRefer" => rotation_refer,
        "SI_modelTransformationMatrix" => |c| {
            run(c, |m, n, g, t| st::transformation_matrix(m, n, &matrix(c, "m"), c.input("local").i16() != 0, c.input("mode").i32(), c.input("geom").i16(), c.input("rel").i16() != 0, g, t))
        },
        "SI_modelSafeTransformationMatrix" => |c| {
            run(c, |m, n, g, t| st::safe_transformation_matrix(m, Some(n), &matrix(c, "m"), c.input("local").i16() != 0, c.input("mode").i32(), c.input("geom").i16(), c.input("rel").i16() != 0, g, t))
        },
        "SI_modelHalfSetState" => |c| run(c, |m, n, g, t| st::half_set_state(m, n, vec3(c, "s"), vec3(c, "r"), vec3(c, "t"), g, t)),
        "SI_modelSetMasterTransfoFromInstance" => |c| run(c, |m, n, g, t| st::set_master_transfo_from_instance(m, n, vec3(c, "s"), vec3(c, "r"), vec3(c, "t"), g, t)),
        "SI_modelState" => |c| state(c, false),
        "SI_modelGlobalState" => |c| state(c, true),
        "toggleEffRotConstraint" => |c| run(c, |m, n, _, _| st::toggle_eff_rot_constraint(m, n)),
        "modelHalfUpdateSonState" => |c| run(c, |m, n, g, t| st::half_update_son_state(m, n, g, t)),
        "SI_modelFreeze" => |c| run(c, |m, n, g, t| tr::freeze(m, n, g, t)),
        "SI_modelFreezeScaling" => |c| run(c, |m, n, g, t| tr::freeze_scaling(m, n, g, t)),
        "SI_modelFreezeRotation" => |c| run(c, |m, n, g, t| tr::freeze_rotation(m, n, g, t)),
        "SI_modelFreezeTranslation" => |c| run(c, |m, n, g, t| tr::freeze_translation(m, n, g, t)),
        "SI_modelCompensateMatrix" => |c| run(c, |m, n, g, t| st::compensate_matrix(m, n, c.input("mode").i32(), c.input("geom").i16(), g, t)),
        "computeInit_srtGlobalTransfo" => |c| {
            let mut list = Verdict::Same;
            run(c, |m, n, g, t| {
                st::compute_init_srt_global_transfo(m, n, node_arg(c, "first"), g, t);
                list = same_list(c, m);
            })
            .and(|| list)
        },
        "computeInit_srtParenting" => |c| {
            let mut list = Verdict::Same;
            run(c, |m, _, _, _| {
                st::compute_init_srt_parenting(m, node_arg(c, "first"));
                list = same_list(c, m);
            })
            .and(|| list)
        },
        "compensateInit_srt" => |c| {
            run(c, |m, n, g, _| {
                let f = c.input("gisb").f32s();
                let entry = |k: usize| {
                    let e = &f[22 * k..22 * k + 22];
                    let mut grot = [[0.0f32; 4]; 4];
                    for i in 0..16 {
                        grot[i / 4][i % 4] = e[3 + i];
                    }
                    st::InitSrtEntry { gscl: Vector { x: e[0], y: e[1], z: e[2] }, grot, gtrs: Vector { x: e[19], y: e[20], z: e[21] } }
                };
                m.glb_init_srt = Some(vec![entry(0), entry(1)]);
                st::compensate_init_srt(m, n, node_arg(c, "first"), g);
            })
        },
        "textureFreeze" => |c| run(c, |m, n, g, t| st::texture_freeze(m, n, c.input("t").i16() != 0, c.input("s").i16() != 0, c.input("r").i16() != 0, g, t)),
        _ => return None,
    })
}

pub fn lookup(name: &str) -> Option<Check> {
    lookup_fn(name)
}
