// SPDX-License-Identifier: MIT
//! Unit tests on made-up chains. They pin what the finding says the solver does; the
//! bit-for-bit comparison with the original is `kine-emucheck`'s.

use si_model::node::Models;
use si_model::transform::update_son_state;
use si_model::NodeId;
use si_types::geom::Vector;

use crate::iktools::{dist, volume_projection, VOL_AT_ROOT, VOL_INSIDE, VOL_TOO_FAR, VOL_TOO_NEAR};
use crate::rec::{IkStore, InvKin, CHAIN_CPX_2D, CHAIN_CPX_3D, CHAIN_DBL_2D, CHAIN_SGL_2D};
use crate::limits::{self, PoslimData};
use crate::{invkin, jacobian, rotlim, skeleton, sketransform, Globals, Host, RotlimData, RotlimRelation};

fn v(x: f32, y: f32, z: f32) -> Vector {
    Vector { x, y, z }
}

struct Chain {
    models: Models,
    ik: IkStore,
    root: NodeId,
    joints: Vec<NodeId>,
    effector: NodeId,
}

/// A chain of bones of the given lengths along X from the origin, the root at `at`.
fn chain(lengths: &[f32], spatial: bool, at: Vector) -> Chain {
    let mut models = Models::default();
    let mut ik = IkStore::default();
    let root = models.allocate(Some(b"chn"));
    models.m_mut(root).algo_type = 1;
    models.m_mut(root).trans = at;
    let p = invkin::inv_kin_allocate(&mut ik);
    invkin::inv_kin_root_allocate(&mut ik, p, lengths.len() as i32, spatial as i16);
    models.m_mut(root).inv_kin = p;
    let mut joints = Vec::new();
    let mut father = root;
    for i in 0..=lengths.len() {
        let n = models.allocate(None);
        let m = models.m_mut(n);
        if i < lengths.len() {
            m.kind = 5;
            m.algo_type = 1;
            joints.push(n);
        } else {
            m.algo_type = 3;
        }
        if i > 0 {
            m.trans = v(lengths[i - 1], 0.0, 0.0);
        }
        let mut r = InvKin::allocate();
        r.prot = v(0.0, 0.0, 0.5);
        let p = ik.add(r);
        models.m_mut(n).inv_kin = p;
        models.arena.create_parentship(father, n);
        father = n;
    }
    let g = Globals::default();
    update_son_state(&mut models, None, root, true, &g.model, &mut ());
    for n in 0..models.len() {
        crate::rec::push_tx(&mut models, &ik, n);
    }
    Chain { models, ik, root, joints, effector: father }
}

fn solve(c: &mut Chain, goal: Vector, host: &mut dyn Host) {
    let g = Globals::default();
    invkin::model_solve_inv_kin(&mut c.models, &mut c.ik, Some(c.effector), goal, 0.0, &g, host);
}

fn off(c: &Chain, goal: Vector) -> f64 {
    dist(goal, c.models.m(c.effector).gtrs).to_f64()
}

#[test]
fn the_root_record_names_the_chain_type() {
    let mut ik = IkStore::default();
    for (n, spatial, kind, dims) in [(1, 0, CHAIN_SGL_2D, None), (2, 0, CHAIN_DBL_2D, None), (5, 0, CHAIN_CPX_2D, Some((2, 5))), (1, 1, CHAIN_CPX_3D, Some((3, 3))), (4, 1, CHAIN_CPX_3D, Some((3, 12)))] {
        let p = invkin::inv_kin_allocate(&mut ik);
        invkin::inv_kin_root_allocate(&mut ik, p, n, spatial);
        let r = *ik.get(p).unwrap();
        assert_eq!((r.kind, r.nb_joints, r.use_pref), (kind, n, 1));
        assert_eq!(ik.matrix(r.jacobian).map(|m| (m.rows, m.cols)), dims);
        assert_eq!(ik.matrix(r.inverse).map(|m| (m.rows, m.cols)), dims.map(|(a, b)| (b, a)));
    }
}

#[test]
fn a_copy_does_not_take_the_goal() {
    let mut ik = IkStore::default();
    let p = invkin::inv_kin_allocate(&mut ik);
    invkin::inv_kin_root_allocate(&mut ik, p, 3, 0);
    {
        let r = ik.get_mut(p).unwrap();
        r.goal = v(1.0, 2.0, 3.0);
        r.state = 2;
        r.prot = v(0.1, 0.2, 0.3);
        r.cns_type = 2;
    }
    let q = invkin::inv_kin_copy(&mut ik, p);
    let (a, b) = (*ik.get(p).unwrap(), *ik.get(q).unwrap());
    assert_eq!((b.kind, b.nb_joints, b.prot, b.cns_type), (a.kind, 3, a.prot, 2));
    assert_eq!((b.goal, b.state), (v(0.0, 0.0, 0.0), 0));
    assert_ne!(b.jacobian, a.jacobian);
    assert_eq!(ik.matrix(b.jacobian).map(|m| (m.rows, m.cols)), Some((2, 3)));
}

#[test]
fn the_volume_a_limb_reaches() {
    let c = chain(&[3.0, 1.0], false, v(0.0, 0.0, 0.0));
    let g = Globals::default();
    let first = c.joints[0];
    assert_eq!(volume_projection(&c.models, first, 2, v(0.00001, 0.0, 0.0), &g).0, VOL_AT_ROOT);
    assert_eq!(volume_projection(&c.models, first, 2, v(0.0, 3.0, 0.0), &g), (VOL_INSIDE, v(0.0, 3.0, 0.0)));
    // beyond 3 + 1: pulled in onto the outer sphere
    assert_eq!(volume_projection(&c.models, first, 2, v(0.0, 8.0, 0.0), &g), (VOL_TOO_FAR, v(0.0, 4.0, 0.0)));
    // inside 3 - 1: pushed out onto the inner sphere, a little beyond it
    let (code, p) = volume_projection(&c.models, first, 2, v(0.0, 0.5, 0.0), &g);
    assert_eq!(code, VOL_TOO_NEAR);
    assert!((p.y - 2.0001).abs() < 1e-6 && p.x == 0.0);
}

#[test]
fn two_joints_bend_to_the_side_of_the_preferred_angle() {
    for sign in [1.0f32, -1.0] {
        let mut c = chain(&[2.0, 2.0], false, v(1.0, 0.0, 0.0));
        let second = c.joints[1];
        c.ik.of_mut(&c.models, second).unwrap().prot.z = 0.5 * sign;
        let goal = v(3.0, 2.0, 0.0);
        solve(&mut c, goal, &mut ());
        assert!(off(&c, goal) < 1e-5, "the effector is {} from the goal", off(&c, goal));
        // a right angle at the second joint, on the preferred side
        let bend = c.models.m(second).vrot.z;
        assert!((bend - sign * core::f32::consts::FRAC_PI_2).abs() < 1e-5, "bend {bend}");
    }
}

#[test]
fn a_goal_out_of_reach_stretches_the_chain_at_it() {
    for lengths in [&[1.0f32, 1.0][..], &[1.0, 1.0, 1.0, 1.0]] {
        let mut c = chain(lengths, false, v(0.0, 0.0, 0.0));
        let total: f32 = lengths.iter().sum();
        solve(&mut c, v(0.0, 100.0, 0.0), &mut ());
        let e = c.models.m(c.effector).gtrs;
        assert!((e.y - total).abs() < 1e-5 && e.x.abs() < 1e-5, "the effector ends at {e:?}");
        for &j in &c.joints[1..] {
            assert_eq!(c.models.m(j).vrot.z, 0.0);
        }
    }
}

#[test]
fn a_longer_planar_chain_is_walked_to_the_goal() {
    let mut c = chain(&[1.0, 1.5, 1.0, 0.5], false, v(0.0, 1.0, 0.0));
    let goal = v(1.5, 2.0, 1.0);
    jacobian::rounds::take();
    solve(&mut c, goal, &mut ());
    let rounds = jacobian::rounds::take();
    assert!(off(&c, goal) < 0.0008 * 4.0, "the effector is {} from the goal", off(&c, goal));
    assert!(rounds[0] >= 1 && rounds[1] == 0 && rounds[2] >= 1, "rounds {rounds:?}");
    // the joints after the first turn about Z only: the chain stays in a plane
    for &j in &c.joints[1..] {
        let r = c.models.m(j).vrot;
        assert_eq!((r.x, r.y), (0.0, 0.0));
    }
    // solved once, the chain is left alone: the goal is within the tolerance
    let before: Vec<Vector> = c.joints.iter().map(|&j| c.models.m(j).vrot).collect();
    solve(&mut c, goal, &mut ());
    let after: Vec<Vector> = c.joints.iter().map(|&j| c.models.m(j).vrot).collect();
    assert_eq!(before, after);
    assert_eq!(jacobian::rounds::take(), [0, 0, 0]);
}

#[test]
fn a_spatial_chain_is_walked_to_the_goal() {
    let mut c = chain(&[1.0, 1.0, 1.0], true, v(0.0, 0.0, 0.0));
    let goal = v(1.0, 1.5, 0.5);
    jacobian::rounds::take();
    solve(&mut c, goal, &mut ());
    let rounds = jacobian::rounds::take();
    assert!(off(&c, goal) < 0.0008 * 3.0, "the effector is {} from the goal", off(&c, goal));
    assert!(rounds[0] == 0 && rounds[1] >= 1, "rounds {rounds:?}");
}

/// A host whose joints all have the same rotation limit on z.
struct Limits(f32, f32);

impl si_model::transform::Hooks for Limits {}
impl si_model::transform::GeometryHooks for Limits {}
impl si_model::hierarchy::Hooks for Limits {}
impl si_model::node_copy::Hooks for Limits {}

impl Host for Limits {
    fn find_rotlim(&mut self, models: &Models, node: NodeId) -> bool {
        models.m(node).kind == 5
    }

    fn cur_rotlim(&mut self, _: &Models, _: NodeId) -> Option<RotlimRelation> {
        let z = v(0.0, 0.0, 0.0);
        let data = RotlimData { min_deg: z, max_deg: z, min_active: [0, 0, 1], max_active: [0, 0, 1], min: v(0.0, 0.0, self.0), max: v(0.0, 0.0, self.1), damping: [0.0, 0.0] };
        Some(RotlimRelation { active: 1, data: Some(data) })
    }
}

#[test]
fn rotation_limits_clamp_and_count() {
    let mut c = chain(&[1.0, 1.0, 1.0], false, v(0.0, 0.0, 0.0));
    let mut host = Limits(-0.25, 0.25);
    for (i, &j) in c.joints.iter().enumerate() {
        c.models.m_mut(j).vrot = v(0.0, 0.0, [1.0, 0.1, -1.0][i]);
    }
    assert!(rotlim::rotlim_constrained_branch(&c.models, c.joints[0], 3, &mut host));
    let (mut node, mut n) = (c.joints[1], 2);
    // the second joint is inside its limits, the third is clamped: the walk stops below it
    assert_eq!(rotlim::check_rotation_limit(&mut c.models, &mut node, &mut n, &mut host), 1);
    assert_eq!((node, n), (c.effector, 0));
    assert_eq!(c.models.m(c.joints[2]).vrot.z, -0.25);
    assert_eq!(c.models.m(c.joints[0]).vrot.z, 1.0);
    // only one of three angles has limits, so no joint is "fully" constrained
    assert_eq!(rotlim::constrained_jnt(&mut c.models, c.joints[0], 3, &mut host), 0);
    assert_eq!(c.models.m(c.joints[0]).vrot.z, 0.25);
    // a limited joint does not start from its preferred angle
    rotlim::jnt_rotlim_set_flag(&mut c.models, &mut c.ik, c.joints[0], 3, &mut host);
    assert!(c.joints.iter().all(|&j| c.ik.of(&c.models, j).unwrap().use_pref == 0));
}

#[test]
fn a_solve_with_limits_leaves_every_joint_inside_them() {
    let mut c = chain(&[1.0, 1.0, 1.0, 1.0], false, v(0.0, 0.0, 0.0));
    let mut host = Limits(-0.6, 0.6);
    solve(&mut c, v(1.0, 1.0, 0.0), &mut host);
    for &j in &c.joints[1..] {
        let z = c.models.m(j).vrot.z;
        assert!((-0.6..=0.6).contains(&z), "angle {z}");
    }
}

#[test]
fn chains_as_objects() {
    let c = chain(&[1.0, 2.0, 0.5], false, v(0.0, 0.0, 0.0));
    let g = Globals::default();
    assert_eq!(skeleton::ske_joint_number(&c.models, Some(c.root)), 3);
    assert_eq!(skeleton::ske_joint_number(&c.models, Some(c.joints[0])), 0);
    assert_eq!(skeleton::ske_chain_total_length(&c.models, Some(c.root), &g).to_f64(), 3.5);
    for &n in c.joints.iter().chain([&c.effector]) {
        assert_eq!(skeleton::ske_chain_root(&c.models, Some(n)), Some(c.root));
    }
    assert_eq!(skeleton::ske_chain_root(&c.models, Some(c.root)), Some(c.root));
    assert_eq!(skeleton::ske_chain_leaf(&c.models, Some(c.root)), Some(c.effector));
    assert_eq!(skeleton::ske_chain_leaf(&c.models, Some(c.joints[1])), Some(c.effector));
    // a chain node may be copied or deleted as a node; a root not as a branch
    assert_eq!(skeleton::ske_validate_copy_delete(&c.models, c.root, 1), 0);
    assert_eq!(skeleton::ske_validate_copy_delete(&c.models, c.root, 3), 1);
    assert_eq!(skeleton::ske_validate_copy_delete(&c.models, c.joints[0], 3), 0);
    // without break joints the sub-chain is the chain
    assert_eq!(invkin::inv_kin_get_sub_chain_first_jnt(&c.models, Some(c.joints[2]), 1, 0.0, &mut ()), (Some(c.joints[0]), 2));
    assert_eq!(invkin::inv_kin_get_sub_chain_effector(&c.models, Some(c.joints[0]), 1, 0.0, &mut ()), (Some(c.effector), 3));
}

#[test]
fn the_preferred_bend_is_never_flat() {
    let mut c = chain(&[1.0, 1.0], false, v(0.0, 0.0, 0.0));
    let j = c.joints[1];
    for (given, kept) in [(0.01f32, 0.05f32), (-0.01, -0.05), (0.0, 0.05), (0.7, 0.7), (-2.0, -2.0)] {
        assert!(invkin::inv_kin_reset_pref_angle(&c.models, &mut c.ik, Some(j), 0, v(0.3, 0.9, given)));
        let p = c.ik.of(&c.models, j).unwrap().prot;
        // the y angle is kept in spatial chains only
        assert_eq!((p.x, p.y, p.z), (0.3, 0.0, kept));
    }
    assert!(!invkin::inv_kin_reset_pref_angle(&c.models, &mut c.ik, Some(c.root), 0, v(0.0, 0.0, 0.0)));
}

#[test]
fn small_routines_by_hand() {
    // a quarter of the way from p to q
    assert_eq!(jacobian::get_insert_point(1.0, 4.0, v(0.0, 0.0, 0.0), v(4.0, 8.0, 0.00001)), v(1.0, 2.0, 0.00001));
    assert_eq!(jacobian::get_insert_point(0.0, 4.0, v(1.0, 2.0, 3.0), v(4.0, 8.0, 9.0)), v(1.0, 2.0, 3.0));
    assert_eq!(jacobian::get_insert_point(4.0, 4.0, v(1.0, 2.0, 3.0), v(4.0, 8.0, 9.0)), v(4.0, 8.0, 9.0));
    // one whole turn of the preferred angle is carried over
    let two_pi = 6.283_185_5f32;
    assert_eq!(jacobian::rotation_find_closer(7.0, 0.5).to_f32(), 0.5 + two_pi);
    assert_eq!(jacobian::rotation_find_closer(-4.0, 0.5).to_f32(), 0.5 - two_pi);
    assert_eq!(jacobian::rotation_find_closer(3.0, 0.5).to_f32(), 0.5);
    assert_eq!(jacobian::point_side_2d(&v(0.0, 1.0, 0.0), &v(0.0, 0.0, 0.0), &v(1.0, 0.0, 0.0)), 1);
    assert_eq!(jacobian::point_side_2d(&v(0.0, -1.0, 0.0), &v(0.0, 0.0, 0.0), &v(1.0, 0.0, 0.0)), -1);
    assert_eq!(jacobian::point_side_2d(&v(5.0, 0.0, 0.0), &v(0.0, 0.0, 0.0), &v(1.0, 0.0, 0.0)), 1);
}

fn rotlim(lo: f32, hi: f32, damping: [f32; 2]) -> RotlimData {
    let z = v(0.0, 0.0, 0.0);
    RotlimData { min_deg: z, max_deg: z, min_active: [1; 3], max_active: [1; 3], min: v(lo, lo, lo), max: v(hi, hi, hi), damping }
}

#[test]
fn damping_bends_a_value_inside_the_band_before_a_bound() {
    let d = PoslimData { min: v(0.0, 0.0, 0.0), max: v(0.0, 0.0, 0.0), min_active: [1; 3], max_active: [1; 3], damping: [0.5, 1.0], is_ref_world: 0, is_area_box: 1, radius: 0.0 };
    // strength 1: the factor is 0, expf(0) = 1, every value in a band lands on the band's edge
    let mut p = v(1.9, -1.75, 0.0);
    limits::poslim_damping(&mut p, v(-2.0, -2.0, -2.0), v(2.0, 2.0, 2.0), &d);
    assert_eq!(p, v(1.5, -1.5, 0.0));
    // strength 0.5: at the band's edge nothing moves, beyond it the value approaches the bound
    let d = PoslimData { damping: [0.5, 0.5], ..d };
    let mut p = v(1.5, 2.0, 100.0);
    limits::poslim_damping(&mut p, v(-2.0, -2.0, -2.0), v(2.0, 2.0, 2.0), &d);
    assert_eq!(p.x, 1.5);
    assert!(p.y > 1.5 && p.y < 2.0 && p.z > p.y && p.z <= 2.0, "{p:?}");
    // a rotation limit's band is given in degrees
    let mut a = v(0.99, 0.0, 0.0);
    limits::rotlim_damping(&mut a, v(-1.0, -1.0, -1.0), v(1.0, 1.0, 1.0), &rotlim(-1.0, 1.0, [10.0, 1.0]));
    assert!((a.x - (1.0 - 10.0 * 0.0174532925)).abs() < 1e-6, "{a:?}");
}

#[test]
fn a_rotation_limit_says_how_it_is_exceeded() {
    let hard = rotlim(-1.0, 1.0, [0.0, 0.0]);
    assert_eq!(limits::rot_exceeds_limits(v(0.5, 0.0, -0.5), &hard), 0);
    assert_eq!(limits::rot_exceeds_limits(v(1.0, 0.0, 0.0), &hard), 0);
    assert_eq!(limits::rot_exceeds_limits(v(0.0, 1.5, 0.0), &hard), 2);
    let soft = rotlim(-1.0, 1.0, [10.0, 0.5]);
    assert_eq!(limits::rot_exceeds_limits(v(0.5, 0.0, 0.0), &soft), 0);
    assert_eq!(limits::rot_exceeds_limits(v(0.9, 0.0, 0.0), &soft), 1);
    assert_eq!(limits::rot_exceeds_limits(v(0.0, 0.0, -5.0), &soft), 1);
    let mut d = rotlim(0.0, 0.0, [30.0, 7.0]);
    d.min_deg = v(-20.0, 45.0, -90.0);
    d.max_deg = v(-40.0, 90.0, 90.0);
    limits::rotlim_value_swap(&mut d);
    assert_eq!((d.min_deg.x, d.max_deg.x, d.min_deg.y), (-40.0, -20.0, 45.0));
    limits::rotlim_damping_in_range(&mut d);
    assert_eq!(d.damping, [20.0, 1.0]);
    limits::reset_rotlim_data(&mut d);
    assert_eq!((d.min_deg.z, d.max_deg.z, d.min_active, d.damping), (-999.0, 999.0, [0; 3], [0.0, 0.5]));
    assert!((d.max.x - 999.0 * 0.0174532925).abs() < 1e-5);
}

/// A host with one position limit for every node: a box of ±1 round the centre.
struct Box1;

impl si_model::transform::Hooks for Box1 {}
impl si_model::transform::GeometryHooks for Box1 {}
impl si_model::hierarchy::Hooks for Box1 {}
impl si_model::node_copy::Hooks for Box1 {}

impl Host for Box1 {
    fn cur_poslim(&mut self, _: &Models, _: NodeId) -> Option<PoslimData> {
        Some(PoslimData { min: v(-1.0, -1.0, -1.0), max: v(1.0, 1.0, 1.0), min_active: [1; 3], max_active: [1; 3], damping: [0.0, 0.0], is_ref_world: 0, is_area_box: 1, radius: 0.0 })
    }
}

#[test]
fn a_position_limit_moves_an_effectors_goal_and_a_tool_moves_a_joint_by_its_bone() {
    let mut c = chain(&[1.0, 1.0], false, v(0.0, 0.0, 0.0));
    let g = Globals::default();
    // the effector is at (2, 0, 0); a box of ±1 round (0, 0.5, 0) pulls its goal to x = 1
    limits::poslim_constraint(&mut c.models, &mut c.ik, c.effector, v(0.0, 0.5, 0.0), 0, &g, &mut Box1);
    let r = *c.ik.of(&c.models, c.effector).unwrap();
    assert_eq!((r.goal, r.state), (v(1.0, 0.0, 0.0), 1));
    // a relative move of an effector adds to that goal
    sketransform::ske_effector_trans(&mut c.models, &mut c.ik, c.effector, v(0.0, 0.25, 0.0), 0, 0);
    assert_eq!(c.ik.of(&c.models, c.effector).unwrap().goal, v(1.0, 0.25, 0.0));
    // moving the second joint up by 1: the first bone turns and stretches to reach (1, 1, 0),
    // and the effector is put back at (2, 0, 0) by turning and stretching the second bone
    let second = c.joints[1];
    sketransform::ske_joint_branch_translate(&mut c.models, &mut c.ik, second, v(0.0, 1.0, 0.0), 1, 0, &g, &mut ());
    let (at, end) = (c.models.m(second).gtrs, c.models.m(c.effector).gtrs);
    assert!((at.x - 1.0).abs() < 1e-5 && (at.y - 1.0).abs() < 1e-5 && at.z.abs() < 1e-5, "{at:?}");
    assert!((end.x - 2.0).abs() < 1e-5 && end.y.abs() < 1e-5, "{end:?}");
    let bone = c.models.m(second).trans;
    assert!((bone.x - 2.0f32.sqrt()).abs() < 1e-5 && bone.y == 0.0 && bone.z == 0.0, "{bone:?}");
}

// ---------------------------------------------------------------- second pass: on the image

mod image {
    use crate::outside::NoOutside;
    use crate::{cnsedit, onimage, pivot, skeimage};
    use si_mem::{Mem, Ptr32};
    use si_types::model::Model;

    fn node(m: &mut Mem, kind: i32, algo: i32, father: Ptr32<Model>) -> Ptr32<Model> {
        let p: Ptr32<Model> = Ptr32::new(m.calloc(1, 0x430));
        m[p].kind = kind;
        m[p].algo_type = algo;
        m[p].father = father;
        if !father.is_null() {
            m[father].son = p;
        }
        p
    }

    /// A chain of two joints on the image: root, joint, joint, effector.
    fn chain(m: &mut Mem) -> [Ptr32<Model>; 4] {
        let root = node(m, 0, 1, Ptr32::NULL);
        let a = node(m, 5, 1, root);
        let b = node(m, 5, 1, a);
        let e = node(m, 0, 3, b);
        m[a].gtrs.x = 0.0;
        m[b].gtrs.x = 3.0;
        m[e].gtrs.x = 3.0;
        m[e].gtrs.y = 4.0;
        [root, a, b, e]
    }

    #[test]
    fn a_chain_on_the_image_has_a_root_a_count_and_a_length() {
        let mut m = Mem::new();
        let [root, a, b, e] = chain(&mut m);
        assert_eq!(onimage::ske_chain_root(&m, e), root);
        assert_eq!(onimage::ske_chain_root(&m, b), root);
        assert_eq!(onimage::ske_chain_root(&m, root), root);
        assert_eq!(skeimage::ske_joint_number(&m, root), 2);
        assert_eq!(skeimage::ske_joint_number(&m, a), 0);
        assert_eq!(skeimage::ske_chain_total_length(&m, root).to_f32(), 7.0);
        // a constraint from a chain's node onto its own root is refused
        assert_eq!(onimage::ik_valid_constraint(&m, b, 2, root, 2), 0);
        assert_eq!(onimage::ik_valid_constraint(&m, b, 2, a, 2), 1);
        assert_eq!(onimage::ik_valid_constraint(&m, b, 4, root, 2), 1);
        assert!(m.fault().is_none());
    }

    #[test]
    fn percent_g_is_the_c_librarys() {
        for (v, text) in [(0.0, "0"), (1.0, "1"), (0.5, "0.5"), (100000.0, "100000"), (1e6, "1e+06"), (999999.5, "1e+06"), (0.0001, "0.0001"), (0.00001, "1e-05"), (123456.7, "123457"), (-1.25, "-1.25"), (3.14159265, "3.14159")] {
            assert_eq!(onimage::percent_g(v), text);
        }
    }

    #[test]
    fn relaxing_a_model_asks_for_seventeen_relations_and_anything_else_for_two() {
        let mut m = Mem::new();
        let mut o = NoOutside::default();
        cnsedit::item_relax_constraint(&mut m, &mut o, 0x6100_0000, 2);
        assert_eq!(o.calls.len(), 17);
        assert_eq!(o.calls[0].1, [0x6100_0000, 2, 0x6f]);
        assert_eq!(o.calls[16].1[2], 0xf7);
        let mut o = NoOutside::default();
        cnsedit::item_relax_constraint(&mut m, &mut o, 0x6100_0000, 4);
        assert_eq!(o.calls.iter().map(|c| c.1[2]).collect::<Vec<_>>(), [0x6f, 0x80]);
        let mut o = NoOutside::default();
        cnsedit::item_relax_constraint(&mut m, &mut o, 0, 2);
        assert!(o.calls.is_empty());
    }

    #[test]
    fn a_pivot_is_between_two_points_only_with_a_point_to_stand_on() {
        let mut m = Mem::new();
        let upd: Ptr32<si_dict::records::UpdNode> = Ptr32::new(m.calloc(1, 0x2c));
        assert_eq!(pivot::anim_pivot_check_transition(&m, upd), 0);
        let state: Ptr32<pivot::PivotData> = Ptr32::new(m.calloc(1, 0x38));
        m[upd].extra = state.cast();
        assert_eq!(pivot::anim_pivot_check_transition(&m, upd), 0);
        m[state].from = Ptr32::new(0x6200_1000);
        m[state].to = Ptr32::new(0x6200_1000);
        assert_eq!(pivot::anim_pivot_check_transition(&m, upd), 0);
        m[state].to_index = 3;
        assert_eq!(pivot::anim_pivot_check_transition(&m, upd), 1);
    }

    /// Third pass: an effector without a record gets one from `SI_malloc(0x74)`, cleared
    /// but for the two pad halves, `use_pref` 1, and the goal with state 1.
    #[test]
    fn setting_a_goal_on_the_image_makes_the_record() {
        let mut m = Mem::new();
        let node: Ptr32<si_types::model::Model> = Ptr32::new(m.calloc(1, 0x430));
        let blocks = m.heap_blocks().len();
        let goal = si_types::geom::Vector { x: 1.0, y: -2.0, z: 3.5 };
        crate::image::invkin::inv_kin_set_goal(&mut m, node, goal);
        let r = crate::image::ik(&m, node);
        assert!(!r.is_null());
        assert_eq!(m.heap_blocks().len(), blocks + 1);
        assert_eq!(m.heap_blocks()[blocks], (r.addr(), 0x74));
        assert_eq!((m[r].state, m[r].goal, m[r].use_pref, m[r].kind), (1, goal, 1, 0));
        // a second goal reuses the record
        crate::image::invkin::inv_kin_set_goal(&mut m, node, si_types::geom::Vector { x: 0.0, y: 0.0, z: 0.0 });
        assert_eq!(m.heap_blocks().len(), blocks + 1);
        assert_eq!(m[r].goal.z, 0.0);
    }

    /// Third pass: the pseudo-inverse on the image's matrices is `si-math`'s on values, and
    /// takes and gives back its four work vectors.
    #[test]
    fn greville_on_the_image_is_si_maths() {
        use si_math::matrix_gen_dbl as g;
        let (rows, cols) = (2u32, 3u32);
        let a_vals = [[1.0, 2.0, -0.5], [0.25, -1.0, 4.0]];
        let mut m = Mem::new();
        let make = |m: &mut Mem, r: u32, c: u32| {
            let p = m.calloc(1, 12);
            let table = m.calloc(r, 4);
            m.set_i32(p, r as i32);
            m.set_i32(p + 4, c as i32);
            m.set_u32(p + 8, table);
            for i in 0..r {
                let row = m.calloc(c, 8);
                m.set_u32(table + 4 * i, row);
            }
            p
        };
        let a = make(&mut m, rows, cols);
        let out = make(&mut m, cols, rows);
        for i in 0..rows {
            for j in 0..cols {
                let at = crate::image::mat_at(&m, Ptr32::new(a), i, j);
                m.set_f64(at, a_vals[i as usize][j as usize]);
            }
        }
        let before = m.heap_blocks().len();
        assert_eq!(crate::image::greville::greville_inverse_dbl(&mut m, Ptr32::new(a), Ptr32::new(out)), 1);
        assert_eq!(m.heap_blocks().len(), before + 4);
        assert_eq!(m.freed().len(), 4);
        let mut va = g::general_matrix_allocate_dbl(rows as i32, cols as i32).unwrap();
        for i in 0..rows as usize {
            for j in 0..cols as usize {
                va.set(i, j, a_vals[i][j]);
            }
        }
        let mut vo = g::general_matrix_allocate_dbl(cols as i32, rows as i32).unwrap();
        g::general_matrix_greville_inverse_dbl(&va, &mut vo);
        for i in 0..cols {
            for j in 0..rows {
                let got = m.f64(crate::image::mat_at(&m, Ptr32::new(out), i, j));
                assert_eq!(got.to_bits(), vo.get(i as usize, j as usize).to_bits());
            }
        }
    }

    /// `SI_QuickStretchCalculation`: nothing for a null element or a null other element; the
    /// first pass of an element with itself only takes its buffer (`SI_calloc(1, 12)`,
    /// zero) and calls nothing.
    #[test]
    fn quick_stretch_calculation_first_pass_on_itself_only_takes_the_buffer() {
        use crate::image::qscalc::quick_stretch_calculation as calc;
        let mut m = Mem::new();
        let pbuf = m.calloc(1, 4);
        let elem = m.calloc(1, 0x84);
        let blocks = m.heap_blocks().len();
        let mut o = NoOutside::default();
        calc(&mut m, &mut o, 0, 0, 8, elem, pbuf, 0, 0, 0, 0, 0, 0, 0);
        calc(&mut m, &mut o, 0, elem, 8, 0, pbuf, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!((m.u32(pbuf), m.heap_blocks().len()), (0, blocks));
        calc(&mut m, &mut o, 0, elem, 8, elem, pbuf, 0, 0, 0, 0, 0, 0, 1);
        let b = m.u32(pbuf);
        assert_eq!(m.heap_blocks()[blocks], (b, 12));
        assert_eq!(m.bytes(b, 12), &[0u8; 12]);
        assert!(o.calls.is_empty());
        assert!(m.fault().is_none());
    }

    /// `SI_quickStretchActive`: 0 without an element (one call); with one, the relation
    /// 239 entry asked with the element, the MODELS chapter and the index the first call
    /// stored, and its 16-bit word at +0x14 answered sign-extended (0 without an entry).
    #[test]
    fn quick_stretch_active_reads_the_entry_word_sign_extended() {
        use crate::image::qstretch::{quick_stretch_active, REL_QSTRETCH, SI_CID};
        use crate::outside::{self as out, Outside, Ret, Routine};
        struct World {
            elem: u32,
            entry: u32,
            seen: Vec<(&'static str, Vec<u32>)>,
        }
        impl Outside for World {
            fn call(&mut self, m: &mut Mem, r: &'static Routine, a: &[u32]) -> Ret {
                self.seen.push((r.name, a.to_vec()));
                if r.addr == out::SI_ED_GET_ELEM_FROM_MODEL.addr {
                    m.set_u32(a[1], 17);
                    return Ret::int(self.elem);
                }
                Ret::int(self.entry)
            }
        }
        let mut m = Mem::new();
        let dicts = m.calloc(2, 0x70);
        let proc_data = m.calloc(1, 4);
        m.set_u32(proc_data, 1);
        m.set_u32(crate::globals::SI_PROC_DATA.addr(), proc_data);
        m.set_u32(0x0867_931c, dicts);
        m.set_u32(SI_CID + 0x20, 3);
        let entry = m.calloc(1, 0x20);
        m.set_u32(entry + 0x14, 0x5a5a_8001);
        let mut w = World { elem: 0, entry, seen: Vec::new() };
        assert_eq!(quick_stretch_active(&mut m, &mut w, 0x40), 0);
        assert_eq!(w.seen.len(), 1);
        w.elem = 0x1234;
        assert_eq!(quick_stretch_active(&mut m, &mut w, 0x40), -0x7fff);
        assert_eq!(w.seen[2].1, vec![dicts + 0x70, 0x1234, REL_QSTRETCH, 3, 17, u32::MAX]);
        w.entry = 0;
        assert_eq!(quick_stretch_active(&mut m, &mut w, 0x40), 0);
    }
}
