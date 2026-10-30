// SPDX-License-Identifier: MIT
//! `kine-emucheck <vectors directory>`: compares this crate with the original, routine by
//! routine, on the test vectors of the emulation harness (finding 0011, `re/tools/emu`).
//!
//! The vectors are what a routine of the Linux `soft` binary left behind when its
//! instructions were stepped through in a CPU emulator on made-up skeleton chains
//! (`re/tools/si-kine/emu_gen.py` makes them and says what they hold). They are derived
//! from the binary and are not in the repository. Per vector file the report gives the
//! number of cases, how many the port reproduces bit for bit, how many it reproduces given
//! the recorded results of the C library and of `fpatan`, and how many differ.
//!
//! A case of a routine that works on a chain compares **everything the routine left**: every
//! word of every node record, every inverse-kinematics record, both matrices of the root,
//! the routine's own results, and the number of rounds of the iteration. A case in which
//! the port had to leave the original's path (a null pointer the original reads through:
//! [`si_kine::fault`]; a matrix inverse that does not exist: `si_model::env::stale`) is
//! skipped. `EMUCHECK_MUTATE=1` flips one bit of everything the port produces.

use std::path::Path;

mod image;
mod image_table;
mod third;
mod world;

use si_kine::rec::{IkStore, InvKin};
use si_kine::limits::{self, PoslimData};
use si_kine::{iktools, invkin, jacobian, rotlim, skeleton, sketransform, RotlimData};
use si_math::matrix_gen_dbl::GenMatrixDbl;
use si_math::types::Matrix;
use si_types::geom::Vector;
use si_x87::vectors::{check_dir_with, print_report, Case, Check, Verdict, Word};
use si_x87::F80;
use world::{load, mut32, mutate, node_arg, same, same_node, value, values, vec3, World};

fn int(c: &Case, name: &str) -> i32 {
    value(&c.input(name).words()[0]) as i32
}

fn flt(c: &Case, name: &str) -> f32 {
    c.input(name).f32()
}

/// Runs a port on a case's world; skips the case when the port left the original's path.
fn on_world(c: &Case, f: impl FnOnce(&mut World) -> Verdict) -> Verdict {
    let mut w = load(c);
    si_kine::fault::reset();
    si_model::env::stale::reset();
    jacobian::rounds::take();
    let first = f(&mut w);
    if si_kine::fault::count() != 0 || si_model::env::stale::count() != 0 {
        return Verdict::Skip;
    }
    first.and(|| same(c, &w))
}

fn st0(c: &Case, got: F80) -> Verdict {
    let got = if mutate() {
        let (m, se) = got.to_bits();
        F80::from_bits(m ^ 1, se)
    } else {
        got
    };
    Verdict::from(c.output("st0").same_f80(got), || format!("st0 {:?} against the port's {got:?}", c.output("st0").f80()))
}

fn eax(c: &Case, got: i32) -> Verdict {
    let got = mut32(got as u32);
    Verdict::from(c.output("eax").u32() == got, || format!("eax {:#x} against the port's {got:#x}", c.output("eax").u32()))
}

fn ax(c: &Case, got: bool) -> Verdict {
    let got = (got as u32) ^ mutate() as u32;
    let want = value(&c.output("ax").words()[0]);
    Verdict::from(want == got, || format!("ax {want} against the port's {got}"))
}

fn floats(c: &Case, name: &str, got: &[f32]) -> Verdict {
    let got: Vec<f32> = got.iter().map(|g| f32::from_bits(mut32(g.to_bits()))).collect();
    Verdict::from(c.output(name).same_f32(&got), || format!("{name}: {:08x?} against the port's {:08x?}", c.output(name).u32s(), got.iter().map(|g| g.to_bits()).collect::<Vec<_>>()))
}

fn vector(c: &Case, name: &str, got: Vector) -> Verdict {
    floats(c, name, &[got.x, got.y, got.z])
}

fn matrix_out(c: &Case, name: &str, m: &Matrix) -> Verdict {
    let flat: Vec<f32> = m.iter().flatten().copied().collect();
    floats(c, name, &flat)
}

fn matrix_in(c: &Case, name: &str) -> Matrix {
    let v = c.input(name).f32s();
    let mut m = [[0.0; 4]; 4];
    for i in 0..16 {
        m[i / 4][i % 4] = v[i];
    }
    m
}

// ---------------------------------------------------------------------------- iktools.c

fn limb_length(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        let (sum, max, min) = iktools::limb_length(&w.models, node, int(c, "n"), &w.g);
        st0(c, sum).and(|| floats(c, "mx", &[max])).and(|| floats(c, "mn", &[min]))
    })
}

fn volume_projection(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        let (code, p) = iktools::volume_projection(&w.models, node, int(c, "n"), vec3(c, "goal"), &w.g);
        eax(c, code).and(|| vector(c, "out", p))
    })
}

fn limb_position(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        vector(c, "ret", iktools::limb_position(&w.models, node, int(c, "n")))
    })
}

fn ortho_normal_inverse(c: &Case) -> Verdict {
    let out = iktools::matrix_ortho_normal_inverse(&matrix_in(c, "m"));
    matrix_out(c, if c.try_output("out").is_some() { "out" } else { "m" }, &out)
}

fn zyz_euler(c: &Case) -> Verdict {
    matrix_out(c, "out", &iktools::matrix_zyz_euler_rotation(flt(c, "a"), flt(c, "b"), flt(c, "c")))
}

fn solve_planar_limb(c: &Case) -> Verdict {
    let (a, b) = iktools::solve_planar_limb(flt(c, "l1"), flt(c, "l2"), flt(c, "x"), flt(c, "y"), int(c, "up") as i16);
    floats(c, "a", &[a]).and(|| floats(c, "b", &[b]))
}

fn special_update(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        iktools::model_special_update_son_state(&mut w.models, &w.ik, node_arg(c, "father"), node, &w.g);
        Verdict::Same
    })
}

fn chain_update(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        iktools::chain_update(&mut w.models, &w.ik, node, int(c, "sons") as i16, &w.g, &mut w.host);
        Verdict::Same
    })
}

// ---------------------------------------------------------------------------- jacobian.c

fn point(c: &Case, name: &str) -> Vector {
    vec3(c, name)
}

fn point_side_2d(c: &Case) -> Verdict {
    eax(c, jacobian::point_side_2d(&point(c, "a"), &point(c, "b"), &point(c, "c")))
}

fn points_angle_signed_2d(c: &Case) -> Verdict {
    st0(c, jacobian::points_angle_signed_2d(&point(c, "a"), &point(c, "b"), &point(c, "c")))
}

fn joint_angle_in_range(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        jacobian::joint_angle_in_range(&mut w.models, node);
        Verdict::Same
    })
}

/// The matrix a pointer argument names: 0 the Jacobian, 1 the pseudo-inverse.
fn which(c: &Case, name: &str) -> usize {
    match &c.input(name).words()[0] {
        Word::Ref { name, .. } if name == "jb" => 1,
        _ => 0,
    }
}

fn compute_jacobian(c: &Case, spatial: bool) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        let i = which(c, "j");
        let mut m: GenMatrixDbl = w.ik.mats[i].clone().unwrap();
        if spatial {
            jacobian::compute_jacobian_3d(&w.models, node, &mut m, int(c, "n"));
        } else {
            jacobian::compute_jacobian_2d(&w.models, node, &mut m, int(c, "n"));
        }
        w.ik.mats[i] = Some(m);
        Verdict::Same
    })
}

fn compute_jacobian_2d(c: &Case) -> Verdict {
    compute_jacobian(c, false)
}

fn compute_jacobian_3d(c: &Case) -> Verdict {
    compute_jacobian(c, true)
}

fn stop_of(c: &Case) -> i16 {
    value(&c.input("stop").words()[0]) as i16
}

fn same_stop(c: &Case, stop: i16) -> Verdict {
    let got = (stop as u16 as u32) ^ mutate() as u32;
    let want = value(&c.output("stop").words()[0]);
    Verdict::from(want == got, || format!("stop {want} against the port's {got}"))
}

fn compute_joints_2d(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        jacobian::compute_joints_2d_stored(&mut w.models, &mut w.ik, 1, int(c, "depth"), node, int(c, "n"), int(c, "recompute") != 0, flt(c, "tol"), vec3(c, "target"), &w.g, &mut w.host);
        Verdict::Same
    })
}

fn compute_joints_3d(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        let mut stop = stop_of(c);
        jacobian::compute_joints_3d_stored(&mut w.models, &mut w.ik, 1, int(c, "depth"), node, int(c, "n"), int(c, "recompute") != 0, flt(c, "tol"), vec3(c, "target"), &mut stop, &w.g, &mut w.host);
        same_stop(c, stop)
    })
}

fn iter_compute_joints_2d(c: &Case) -> Verdict {
    on_world(c, |w| {
        let (Some(root), Some(node)) = (node_arg(c, "root"), node_arg(c, "node")) else { return Verdict::Skip };
        jacobian::iter_compute_joints_2d_stored(&mut w.models, &mut w.ik, root, node, int(c, "n"), flt(c, "reach"), flt(c, "frac"), flt(c, "tol"), vec3(c, "goal"), &w.g, &mut w.host);
        Verdict::Same
    })
}

fn iter_compute_joints_3d(c: &Case) -> Verdict {
    on_world(c, |w| {
        let (Some(root), Some(node)) = (node_arg(c, "root"), node_arg(c, "node")) else { return Verdict::Skip };
        let mut stop = stop_of(c);
        jacobian::iter_compute_joints_3d_stored(&mut w.models, &mut w.ik, root, node, int(c, "n"), flt(c, "reach"), flt(c, "frac"), flt(c, "tol"), vec3(c, "goal"), &mut stop, &w.g, &mut w.host);
        same_stop(c, stop)
    })
}

fn rotation_find_closer(c: &Case) -> Verdict {
    st0(c, jacobian::rotation_find_closer(flt(c, "a"), flt(c, "b")))
}

fn map_goal_to_root(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        vector(c, "out", jacobian::map_goal_to_root(&w.models, node, vec3(c, "goal"), &w.g))
    })
}

fn find_resolution_plane(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        let mut goal = vec3(c, "goal");
        let plane = jacobian::find_resolution_plane(&mut w.models, &mut w.ik, node, int(c, "type"), int(c, "roll") as i16, &mut goal, &w.g);
        vector(c, "goal", goal).and(|| matrix_out(c, "plane", &plane))
    })
}

fn chain_2d_roll_apply(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        jacobian::chain_2d_roll_apply(&mut w.models, &mut w.ik, node, int(c, "n"), &w.g);
        Verdict::Same
    })
}

fn invkin0(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        jacobian::invkin0(&mut w.models, &mut w.ik, node, vec3(c, "goal"), &w.g, &mut w.host);
        Verdict::Same
    })
}

fn invkin1(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        jacobian::invkin1(&mut w.models, &mut w.ik, node, vec3(c, "goal"), &w.g, &mut w.host);
        Verdict::Same
    })
}

fn get_insert_point(c: &Case) -> Verdict {
    vector(c, "out", jacobian::get_insert_point(flt(c, "d1"), flt(c, "d2"), vec3(c, "p"), vec3(c, "q")))
}

fn invkin2d(c: &Case) -> Verdict {
    on_world(c, |w| {
        let (Some(root), Some(node)) = (node_arg(c, "root"), node_arg(c, "node")) else { return Verdict::Skip };
        jacobian::invkin2d(&mut w.models, &mut w.ik, root, node, int(c, "n"), vec3(c, "goal"), int(c, "first") as i16, &w.g, &mut w.host);
        Verdict::Same
    })
}

fn invkin3d(c: &Case) -> Verdict {
    on_world(c, |w| {
        let (Some(root), Some(node)) = (node_arg(c, "root"), node_arg(c, "node")) else { return Verdict::Skip };
        let mut stop = stop_of(c);
        jacobian::invkin3d(&mut w.models, &mut w.ik, root, node, int(c, "n"), vec3(c, "goal"), int(c, "first") as i16, &mut stop, &w.g, &mut w.host);
        same_stop(c, stop)
    })
}

fn check_branch_orient(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        let r = jacobian::check_branch_orient(&mut w.models, &mut w.ik, node, vec3(c, "goal"), int(c, "n"), &w.g, &mut w.host);
        ax(c, r)
    })
}

fn check_branch_pullout(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        let r = jacobian::check_branch_pullout(&mut w.models, &mut w.ik, node, vec3(c, "goal"), int(c, "n"), &w.g, &mut w.host);
        ax(c, r)
    })
}

fn solve_partial_inv_kin(c: &Case) -> Verdict {
    on_world(c, |w| {
        let mut stop = stop_of(c);
        jacobian::solve_partial_inv_kin(&mut w.models, &mut w.ik, node_arg(c, "root"), node_arg(c, "first"), int(c, "n"), vec3(c, "goal"), int(c, "pass") as i16, &mut stop, &w.g, &mut w.host);
        same_stop(c, stop)
    })
}

// ---------------------------------------------------------------------------- invkin.c

/// The calls of `SI_classicSolvePartialInvKin` the emulator recorded against the host's.
fn same_classic(c: &Case, w: &World) -> Verdict {
    let want: Vec<(i32, [u32; 3], i16)> = c
        .calls
        .iter()
        .filter(|k| k.name == "SI_classicSolvePartialInvKin")
        .map(|k| {
            let a: Vec<u32> = k.args.iter().map(value).collect();
            (a[2] as i32, [a[3], a[4], a[5]], a[6] as i16)
        })
        .collect();
    let mut got = w.host.classic.clone();
    if mutate() {
        got.push((0, [0; 3], 0));
    }
    Verdict::from(want == got, || format!("calls of SI_classicSolvePartialInvKin {want:x?} against the port's {got:x?}"))
}

fn model_solve_ik(c: &Case) -> Verdict {
    on_world(c, |w| {
        let (Some(root), Some(first), Some(eff)) = (node_arg(c, "root"), node_arg(c, "first"), node_arg(c, "eff")) else { return Verdict::Skip };
        invkin::model_solve_ik(&mut w.models, &mut w.ik, root, first, eff, vec3(c, "goal"), int(c, "n"), &w.g, &mut w.host);
        same_classic(c, w)
    })
}

fn model_solve_inv_kin(c: &Case) -> Verdict {
    on_world(c, |w| {
        invkin::model_solve_inv_kin(&mut w.models, &mut w.ik, node_arg(c, "node"), vec3(c, "goal"), flt(c, "time"), &w.g, &mut w.host);
        same_classic(c, w)
    })
}

fn inv_kin_allocate(c: &Case) -> Verdict {
    world::same_new_record(c, "heap0", Some(&InvKin::allocate()))
}

/// The record a pointer argument names in a case's world.
fn rec_arg(c: &Case, name: &str) -> u32 {
    match &c.input(name).words()[0] {
        Word::Ref { .. } => value(&c.input(name).words()[0]),
        _ => 0,
    }
}

/// The dimensions of the matrices a record of the original points to: its two pointer words
/// (+0x2c, +0x30) name heap blocks whose first two words are rows and columns. (A matrix
/// the allocator gave up on half-way leaves blocks behind that no record points to.)
fn heap_matrices(c: &Case, record: &str) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for i in [11, 12] {
        if let Word::Ref { name, .. } = &c.output(record).words()[i] {
            if let Some(b) = c.try_output(name) {
                out.push((value(&b.words()[0]) as i32, value(&b.words()[1]) as i32));
            }
        }
    }
    out
}

fn port_matrices(ik: &IkStore, r: &InvKin) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = [r.jacobian, r.inverse].iter().filter_map(|&p| ik.matrix(p)).map(|m| (m.rows as i32, m.cols as i32)).collect();
    if mutate() {
        out.push((0, 0));
    }
    out
}

fn inv_kin_root_allocate(c: &Case) -> Verdict {
    on_world(c, |w| {
        let rec = rec_arg(c, "rec");
        if rec == 0 {
            return Verdict::Same;
        }
        // the record starts without matrices: the case's own are not this record's
        invkin::inv_kin_root_allocate(&mut w.ik, world::ptr(rec), int(c, "n"), int(c, "spatial") as i16);
        let r = w.ik.get(world::ptr(rec)).copied().unwrap();
        let (want, got) = (heap_matrices(c, &format!("ik{}", rec - 1)), port_matrices(&w.ik, &r));
        // the record's two pointers are checked by the matrices they lead to
        let rr = w.ik.get_mut(world::ptr(rec)).unwrap();
        let out = c.output(&format!("ik{}", rec - 1));
        rr.jacobian = si_types::Ptr32::new(value(&out.words()[11]));
        rr.inverse = si_types::Ptr32::new(value(&out.words()[12]));
        Verdict::from(want == got, || format!("matrices {want:?} against the port's {got:?}"))
    })
}

fn inv_kin_copy(c: &Case) -> Verdict {
    on_world(c, |w| {
        let rec = rec_arg(c, "rec");
        let copy = invkin::inv_kin_copy(&mut w.ik, world::ptr(rec));
        let want_null = !matches!(c.output("ptr").words()[0], Word::Ref { .. });
        if rec == 0 || want_null {
            return Verdict::from(copy.is_null() == want_null && !mutate(), || "a null copy on one side only".to_string());
        }
        let r = w.ik.get(copy).copied().unwrap();
        let (want, got) = (heap_matrices(c, "heap0"), port_matrices(&w.ik, &r));
        world::same_new_record(c, "heap0", Some(&r)).and(|| Verdict::from(want == got, || format!("matrices {want:?} against the port's {got:?}")))
    })
}

fn inv_kin_dispose(c: &Case) -> Verdict {
    on_world(c, |w| {
        let mut p = world::ptr(rec_arg(c, "hp"));
        invkin::inv_kin_dispose(&mut w.ik, &mut p);
        let want = value(&c.output("hp").words()[0]);
        let got = mut32(p.0);
        Verdict::from(want == got, || format!("the pointer afterwards: {want:#x} against the port's {got:#x}"))
    })
}

fn ik_valid_constraint(c: &Case) -> Verdict {
    on_world(c, |w| {
        let (Some(a), Some(b)) = (node_arg(c, "a"), node_arg(c, "b")) else { return Verdict::Skip };
        eax(c, invkin::ik_valid_constraint(&w.models, a, int(c, "ta"), b, int(c, "tb")))
    })
}

fn inv_kin_reset_pref_angle(c: &Case) -> Verdict {
    on_world(c, |w| {
        let r = invkin::inv_kin_reset_pref_angle(&w.models, &mut w.ik, node_arg(c, "node"), int(c, "group") as u32, vec3(c, "angles"));
        ax(c, r)
    })
}

fn check_scale_before_ik(c: &Case) -> Verdict {
    on_world(c, |w| {
        let (Some(first), Some(last)) = (node_arg(c, "first"), node_arg(c, "last")) else { return Verdict::Skip };
        let r = invkin::check_scale_before_ik(&w.models, first, last, int(c, "n"));
        let want = match &c.output("out").words()[0] {
            Word::Ref { .. } => Some(value(&c.output("out").words()[0]) as usize - 1),
            _ => None,
        };
        ax(c, r.is_some()).and(|| Verdict::from(r.is_none() || r == want, || format!("the joint {want:?} against the port's {r:?}")))
    })
}

fn find_sub_goal_before_ik(c: &Case) -> Verdict {
    on_world(c, |w| {
        let (Some(first), Some(cut), Some(eff)) = (node_arg(c, "first"), node_arg(c, "cut"), node_arg(c, "eff")) else { return Verdict::Skip };
        vector(c, "out", invkin::find_sub_goal_before_ik(&w.models, first, cut, eff, vec3(c, "goal")))
    })
}

fn inv_kin_is_break_jnt(c: &Case) -> Verdict {
    on_world(c, |w| {
        let r = invkin::inv_kin_is_break_jnt(&w.models, node_arg(c, "node"), int(c, "flag") as i16, flt(c, "time"), &mut w.host);
        ax(c, r)
    })
}

fn same_count(c: &Case, got: i32) -> Verdict {
    let got = mut32(got as u32);
    Verdict::from(c.output("cnt").same_u32(&[got]), || format!("count {:?} against the port's {got}", c.output("cnt").u32s()))
}

fn sub_chain_first_jnt(c: &Case) -> Verdict {
    on_world(c, |w| {
        let (p, n) = invkin::inv_kin_get_sub_chain_first_jnt(&w.models, node_arg(c, "node"), int(c, "flag") as i16, flt(c, "time"), &mut w.host);
        same_node(c, p).and(|| same_count(c, n))
    })
}

fn sub_chain_effector(c: &Case) -> Verdict {
    on_world(c, |w| {
        let (p, n) = invkin::inv_kin_get_sub_chain_effector(&w.models, node_arg(c, "node"), int(c, "flag") as i16, flt(c, "time"), &mut w.host);
        same_node(c, p).and(|| same_count(c, n))
    })
}

fn inv_kin_set_goal(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        invkin::inv_kin_set_goal(&mut w.models, &mut w.ik, node, vec3(c, "goal"));
        Verdict::Same
    })
}

fn inv_kin_set_cns_position(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        invkin::inv_kin_set_cns_position(&mut w.models, &mut w.ik, node, int(c, "type"), vec3(c, "pos"), flt(c, "time"), &w.g, &mut w.host);
        Verdict::Same
    })
}

fn inv_kin_goal_invalidate(c: &Case) -> Verdict {
    on_world(c, |w| {
        invkin::inv_kin_goal_invalidate(&mut w.models, &mut w.ik, node_arg(c, "node"));
        Verdict::Same
    })
}

// ---------------------------------------------------------------------------- constraint.c

fn jnt_rotlim_set_flag(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        rotlim::jnt_rotlim_set_flag(&mut w.models, &mut w.ik, node, int(c, "n"), &mut w.host);
        Verdict::Same
    })
}

fn rotlim_constrained_branch(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        let r = rotlim::rotlim_constrained_branch(&w.models, node, int(c, "n"), &mut w.host);
        ax(c, r)
    })
}

fn check_jnt_rot_lim(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        let r = rotlim::check_jnt_rot_lim(&mut w.models, node, &mut w.host);
        eax(c, r)
    })
}

fn check_rotation_limit(c: &Case) -> Verdict {
    on_world(c, |w| {
        let mut node = value(&c.input("pn").words()[0]) as usize - 1;
        let mut n = c.input("pc").i32();
        let r = rotlim::check_rotation_limit(&mut w.models, &mut node, &mut n, &mut w.host);
        let want_node = value(&c.output("pn").words()[0]) as usize;
        eax(c, r)
            .and(|| Verdict::from(want_node == node + 1, || format!("the node afterwards: {want_node} against the port's {}", node + 1)))
            .and(|| Verdict::from(c.output("pc").i32() == n, || format!("the count afterwards: {} against the port's {n}", c.output("pc").i32())))
    })
}

fn constrained_jnt(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        let r = rotlim::constrained_jnt(&mut w.models, node, int(c, "n"), &mut w.host);
        eax(c, r)
    })
}

fn pref_angle_update(c: &Case) -> Verdict {
    on_world(c, |w| {
        rotlim::inv_kin_pref_angle_update(&mut w.models, &mut w.ik, node_arg(c, "node"), &mut w.host);
        Verdict::Same
    })
}

// ---------------------------------------------------------------------------- skeleton.c

fn ske_chain_root(c: &Case) -> Verdict {
    on_world(c, |w| same_node(c, skeleton::ske_chain_root(&w.models, node_arg(c, "node"))))
}

fn ske_chain_leaf(c: &Case) -> Verdict {
    on_world(c, |w| same_node(c, skeleton::ske_chain_leaf(&w.models, node_arg(c, "node"))))
}

fn ske_chain_total_length(c: &Case) -> Verdict {
    on_world(c, |w| st0(c, skeleton::ske_chain_total_length(&w.models, node_arg(c, "node"), &w.g)))
}

fn ske_joint_number(c: &Case) -> Verdict {
    on_world(c, |w| eax(c, skeleton::ske_joint_number(&w.models, node_arg(c, "node"))))
}

fn ske_chain_reset(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        skeleton::ske_chain_reset(&mut w.models, node, &w.g, &mut w.host);
        Verdict::Same
    })
}

fn ske_validate_copy_delete(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        eax(c, skeleton::ske_validate_copy_delete(&w.models, node, int(c, "state")))
    })
}

// ---------------------------------------------------------------------------- sketransform.c

fn side_vector(c: &Case) -> Vector {
    vec3(c, "tg")
}

fn ske_effector_trans(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        sketransform::ske_effector_trans(&mut w.models, &mut w.ik, node, vec3(c, "by"), int(c, "absolute") as i16, int(c, "from") as i16);
        Verdict::Same
    })
}

fn ske_move_joint_branch(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        sketransform::ske_move_joint_branch(&mut w.models, &w.ik, node, side_vector(c), int(c, "turn") as i16, int(c, "keep") as i16, &w.g, &mut w.host);
        Verdict::Same
    })
}

fn ske_joint_branch_translate(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        let t = sketransform::ske_joint_branch_translate(&mut w.models, &mut w.ik, node, side_vector(c), int(c, "turn") as i16, int(c, "keep") as i16, &w.g, &mut w.host);
        vector(c, "tg", t).and(|| world::same_tools(c, w))
    })
}

fn ske_joint_scale(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        let r = sketransform::ske_joint_scale(&mut w.models, node, side_vector(c), int(c, "absolute") as i16, &w.g, &mut w.host);
        eax(c, r as i32).and(|| world::same_tools(c, w))
    })
}

fn ske_joint_rotate(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        sketransform::ske_joint_rotate(&mut w.models, &mut w.ik, None, node, side_vector(c), int(c, "absolute") as i16, &w.g, &mut w.host);
        world::same_tools(c, w)
    })
}

// ---------------------------------------------------------------------------- constraint.c: limits

fn rotlim_words(d: &RotlimData) -> Vec<u32> {
    let v = |a: Vector| [a.x.to_bits(), a.y.to_bits(), a.z.to_bits()];
    let s = |a: [i16; 3]| [a[0] as u16 as u32, a[1] as u16 as u32, a[2] as u16 as u32];
    [v(d.min_deg), v(d.max_deg), s(d.min_active), s(d.max_active), v(d.min), v(d.max)].concat().into_iter().chain([d.damping[0].to_bits(), d.damping[1].to_bits()]).collect()
}

fn poslim_data(f: &si_x87::vectors::Field) -> PoslimData {
    let d = values(f);
    let fl = |i: usize| f32::from_bits(d[i]);
    PoslimData {
        min: Vector { x: fl(0), y: fl(1), z: fl(2) },
        max: Vector { x: fl(3), y: fl(4), z: fl(5) },
        min_active: [d[6] as i16, d[7] as i16, d[8] as i16],
        max_active: [d[9] as i16, d[10] as i16, d[11] as i16],
        damping: [fl(12), fl(13)],
        is_ref_world: d[14] as i16,
        is_area_box: d[15] as i16,
        radius: fl(16),
    }
}

fn poslim_words(d: &PoslimData) -> Vec<u32> {
    let v = |a: Vector| [a.x.to_bits(), a.y.to_bits(), a.z.to_bits()];
    let s = |a: [i16; 3]| [a[0] as u16 as u32, a[1] as u16 as u32, a[2] as u16 as u32];
    [v(d.min), v(d.max), s(d.min_active), s(d.max_active)]
        .concat()
        .into_iter()
        .chain([d.damping[0].to_bits(), d.damping[1].to_bits(), d.is_ref_world as u16 as u32, d.is_area_box as u16 as u32, d.radius.to_bits()])
        .collect()
}

/// A record's words against an output field, word for word (unwritten words skipped).
fn same_words(c: &Case, name: &str, got: &[u32]) -> Verdict {
    let want = c.output(name).words();
    if want.len() != got.len() {
        return Verdict::Different(format!("{name}: {} words against the port's {}", want.len(), got.len()));
    }
    for (i, (a, &b)) in want.iter().zip(got).enumerate() {
        let b = mut32(b);
        if !matches!(a, Word::Unwritten { .. }) && value(a) != b {
            return Verdict::Different(format!("{name} word {i}: {:08x} against the port's {b:08x}", value(a)));
        }
    }
    Verdict::Same
}

fn rotlim_damping(c: &Case) -> Verdict {
    let mut a = vec3(c, "a");
    limits::rotlim_damping(&mut a, vec3(c, "mn"), vec3(c, "mx"), &world::rotlim_data(c.input("d")));
    vector(c, "a", a)
}

fn poslim_damping(c: &Case) -> Verdict {
    let mut a = vec3(c, "a");
    limits::poslim_damping(&mut a, vec3(c, "mn"), vec3(c, "mx"), &poslim_data(c.input("d")));
    vector(c, "a", a)
}

fn spherical_poslim_damping(c: &Case) -> Verdict {
    let mut a = vec3(c, "a");
    limits::spherical_poslim_damping(&mut a, vec3(c, "c"), &poslim_data(c.input("d")));
    floats(c, "a", &[a.x, a.y, a.z])
}

fn rot_exceeds_limits(c: &Case) -> Verdict {
    eax(c, limits::rot_exceeds_limits(vec3(c, "r"), &world::rotlim_data(c.input("d"))))
}

fn rotlim_damping_in_range(c: &Case) -> Verdict {
    let mut d = world::rotlim_data(c.input("d"));
    limits::rotlim_damping_in_range(&mut d);
    same_words(c, "d", &rotlim_words(&d))
}

fn rotlim_value_swap(c: &Case) -> Verdict {
    let mut d = world::rotlim_data(c.input("d"));
    limits::rotlim_value_swap(&mut d);
    same_words(c, "d", &rotlim_words(&d))
}

fn reset_rotlim_data(c: &Case) -> Verdict {
    let z = Vector { x: 7.0, y: 7.0, z: 7.0 };
    let mut d = RotlimData { min_deg: z, max_deg: z, min_active: [7; 3], max_active: [7; 3], min: z, max: z, damping: [7.0; 2] };
    limits::reset_rotlim_data(&mut d);
    same_words(c, "d", &rotlim_words(&d))
}

/// A record the routine allocated, as the words of the vector's `heap0` (32-bit words).
fn heap_bytes(words: &[u32], sizes: &[usize]) -> Vec<u32> {
    let mut bytes = Vec::new();
    for (w, s) in words.iter().zip(sizes) {
        bytes.extend_from_slice(&w.to_le_bytes()[..*s]);
    }
    bytes.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

const LIMIT_SIZES: [usize; 20] = [4, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2, 2, 4, 4, 4, 4, 4, 4, 4, 4];

fn rotlim_create_data(c: &Case) -> Verdict {
    let d = limits::rotlim_create_data(&world::rotlim_data(c.input("def")));
    same_words(c, "heap0", &heap_bytes(&rotlim_words(&d), &LIMIT_SIZES))
}

fn poslim_create_data(c: &Case) -> Verdict {
    let d = limits::poslim_create_data(&poslim_data(c.input("def")));
    let sizes = [4, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2, 2, 4, 4, 2, 2, 4];
    same_words(c, "heap0", &heap_bytes(&poslim_words(&d), &sizes))
}

fn constr_3pts(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        ax(c, limits::constr_3pts_map_direction(&mut w.models, node, vec3(c, "a"), vec3(c, "b"), &w.g, &mut w.host))
    })
}

fn nor_constr(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        ax(c, limits::nor_constr_map_direction(&mut w.models, node, vec3(c, "a"), vec3(c, "b"), &w.g, &mut w.host))
    })
}

fn tan_constr(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        ax(c, limits::tan_constr_map_direction(&mut w.models, node, vec3(c, "a"), &w.g, &mut w.host))
    })
}

fn tan_create(c: &Case) -> Verdict {
    let d = limits::tan_constr_create_data();
    let words = [(d.set as u16 as u32) | (d.same_path as u16 as u32) << 16, d.percent.to_bits(), d.distance.to_bits(), d.point[0].to_bits(), d.point[1].to_bits(), d.point[2].to_bits()];
    same_words(c, "heap0", &words)
}

fn rotlim_constraint(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        limits::rotlim_constraint(&mut w.models, node, int(c, "index"), &w.g, &mut w.host);
        Verdict::Same
    })
}

fn poslim_constraint(c: &Case) -> Verdict {
    on_world(c, |w| {
        let Some(node) = node_arg(c, "node") else { return Verdict::Skip };
        limits::poslim_constraint(&mut w.models, &mut w.ik, node, vec3(c, "centre"), int(c, "index"), &w.g, &mut w.host);
        Verdict::Same
    })
}

fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "si_limbLength" => limb_length,
        "SI_volumeProjection" => volume_projection,
        "si_limbPosition" => limb_position,
        "SI_matrixOrthoNormalInverse" => ortho_normal_inverse,
        "SI_matrixZyzeulerRotation" => zyz_euler,
        "si_solvePlanarLimb" => solve_planar_limb,
        "SI_modelSpecialUpdateSonState" => special_update,
        "SI_chainUpdate" => chain_update,
        "pointSide2d" => point_side_2d,
        "pointsAngleSigned2d" => points_angle_signed_2d,
        "jointAngleInRange" => joint_angle_in_range,
        "computeJacobian2d" => compute_jacobian_2d,
        "computeJacobian3d" => compute_jacobian_3d,
        "computeJoints2d" => compute_joints_2d,
        "computeJoints3d" => compute_joints_3d,
        "iterComputeJoints2d" => iter_compute_joints_2d,
        "iterComputeJoints3d" => iter_compute_joints_3d,
        "rotationFindCloser" => rotation_find_closer,
        "mapGoalToRoot" => map_goal_to_root,
        "findResolutionPlane" => find_resolution_plane,
        "chain2DRollApply" => chain_2d_roll_apply,
        "invkin0" => invkin0,
        "invkin1" => invkin1,
        "si_getInsertPoint" => get_insert_point,
        "invkin2d" => invkin2d,
        "invkin3d" => invkin3d,
        "checkBranchOrient" => check_branch_orient,
        "si_checkBranchPullout" => check_branch_pullout,
        "SI_solvePartialInvKin" => solve_partial_inv_kin,
        "modelSolveIK" => model_solve_ik,
        "SI_modelSolveInvKin" => model_solve_inv_kin,
        "SI_invKinAllocate" => inv_kin_allocate,
        "SI_invKinRootAllocate" => inv_kin_root_allocate,
        "SI_invKinCopy" => inv_kin_copy,
        "SI_invKinDispose" => inv_kin_dispose,
        "SI_ikValidConstraint" => ik_valid_constraint,
        "SI_invKinResetPrefAngle" => inv_kin_reset_pref_angle,
        "checkScaleBeforeIK" => check_scale_before_ik,
        "findSubGoalBeforeIK" => find_sub_goal_before_ik,
        "SI_invKinIsBreakJnt" => inv_kin_is_break_jnt,
        "SI_invKinGetSubChainFirstJnt" => sub_chain_first_jnt,
        "SI_invKinGetSubChainEffector" => sub_chain_effector,
        "SI_invKinSetGoal" => inv_kin_set_goal,
        "SI_invKinSetCnsPosition" => inv_kin_set_cns_position,
        "SI_invKinGoalInvalidate" => inv_kin_goal_invalidate,
        "SI_jntRotlimSetFlag" => jnt_rotlim_set_flag,
        "SI_rotlimConstrainedBranch" => rotlim_constrained_branch,
        "si_checkJntRotLim" => check_jnt_rot_lim,
        "SI_checkRotationLimit" => check_rotation_limit,
        "SI_constrainedJnt" => constrained_jnt,
        "invKinPrefAngleUpdate" => pref_angle_update,
        "SI_skeChainRoot" => ske_chain_root,
        "skeChainLeaf" => ske_chain_leaf,
        "skeChainTotalLength" => ske_chain_total_length,
        "si_skeJointNumber" => ske_joint_number,
        "si_skeChainReset" => ske_chain_reset,
        "SI_skeValidateCopyDelete" => ske_validate_copy_delete,
        "si_constr3PtsMapDirection" => constr_3pts,
        "si_norConstrMapDirection" => nor_constr,
        "si_tanConstrMapDirection" => tan_constr,
        "si_tanConstrCreateData" => tan_create,
        "si_rotlimConstraint" => rotlim_constraint,
        "si_poslimConstraint" => poslim_constraint,
        "rotlimDamping" => rotlim_damping,
        "poslimDamping" => poslim_damping,
        "sphericalPoslimDamping" => spherical_poslim_damping,
        "rotExceedsLimits" => rot_exceeds_limits,
        "rotlimDampingInRange" => rotlim_damping_in_range,
        "rotlimValueSwap" => rotlim_value_swap,
        "resetRotlimData" => reset_rotlim_data,
        "rotlimCreateData" => rotlim_create_data,
        "poslimCreateData" => poslim_create_data,
        "SI_skeEffectorTrans" => ske_effector_trans,
        "si_skeMoveJointBranch" => ske_move_joint_branch,
        "si_skeJointBranchTranslate" => ske_joint_branch_translate,
        "SI_skeJointScale" => ske_joint_scale,
        "SI_skeJointRotate" => ske_joint_rotate,
        _ => return image_table::lookup(name),
    })
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `--image`: the first pass's vectors against the image forms of the third pass
    // (`third.rs`); the second pass's routines as always
    let third = args.first().is_some_and(|a| a == "--image");
    if third {
        args.remove(0);
    }
    let dir = args.first().cloned().unwrap_or_else(|| {
        eprintln!("usage: kine-emucheck [--image] <vectors directory>");
        std::process::exit(2);
    });
    let _ = values;
    let pick = |name: &str| if third { third::lookup(name).or_else(|| image_table::lookup(name)) } else { lookup(name) };
    let rows = check_dir_with(Path::new(&dir), &pick, true).unwrap_or_else(|e| {
        eprintln!("kine-emucheck: {e}");
        std::process::exit(2);
    });
    std::process::exit(if print_report(&rows) { 0 } else { 1 });
}
