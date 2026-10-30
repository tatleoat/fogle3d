// SPDX-License-Identifier: MIT
//! Transformation updates of `motionUpdate.c`: scaling, rotation, translation, the node's
//! matrices, positions of constraining objects, and paths.
//!
//! The curves write the **local** transformation of a model node (scaling +0x100, rotation
//! angles +0x10c, translation +0x158) and raise bits in the node's update flags (+0x0a and
//! its copy at +0x3fa): 2 scaling, 4 rotation, 8 translation. The matrices are rebuilt
//! later, by the node's own "transformation update" step (relation −100), which the
//! topological sort places after every step that feeds the transformation.

use super::{arg, arg_curve, data, nb_args, relid, UpdateHost};
use crate::mem::MemExt;
use crate::ed;
use crate::fcv;
use crate::glob::{self, cid};
use si_mem::{Addr, Mem};
use crate::off::{arel, elem, model, updelem, updflag, updnode};
use si_x87::{fmov, F80};

use super::params::o::{camera, light, IK_GOAL, IK_STATE};
use super::params::DEG2RAD;

crate::host_trait! {
    /// What the path code calls in the spline and NURBS files and in `constraint.c`.
    pub trait TransfoHost: UpdateHost {
        /// `SI_splineLength(spline)` (spline.c).
        fn spline_length(spline: Addr) -> F80;
        /// `SI_splineDistanceToFactor(spline, distance)` (spline.c): the spline parameter at
        /// a distance along the curve.
        fn spline_distance_to_factor(spline: Addr, distance: f32) -> F80;
        /// `SI_splineValue(spline, factor, &point)` (spline.c): three floats.
        fn spline_value(spline: Addr, factor: f32, out: Addr);
        /// `SI_nurbsCurveLength(curve)` (nrb_geometry.c).
        fn nurbs_curve_length(curve: Addr) -> F80;
        /// `SI_nurbsDistanceToFactor(curve, distance)` (nrb_geometry.c); a `double` argument.
        fn nurbs_distance_to_factor(curve: Addr, distance: f64) -> F80;
        /// `SI_nurbsCrvValue(curve, u, flag)` (eval.c): returns a record of three doubles
        /// through a hidden pointer, here `out`. The path code passes flag 0.
        fn nurbs_crv_value(out: Addr, curve: Addr, u: f64, flag: i32);
        /// `si_tanConstrCreateData()` (constraint.c): a new tangent-constraint record.
        fn tan_constr_create_data() -> Addr;
        /// `SI_edGetElemFromCamera(camera, &index)` (edtools.c).
        fn ed_get_elem_from_camera(camera: Addr, index: Addr) -> Addr;
        /// `SI_edGetElemFromLight(light, &index)` (edtools.c).
        fn ed_get_elem_from_light(light: Addr, index: Addr) -> Addr;
    }
}

fn raise(m: &mut Mem, mdl: Addr, bit: u8) {
    m.or_u8(mdl + model::UPDATE_FLAGS, bit);
    m.or_u8(mdl + model::UPDATE_FLAGS2, bit);
}

/// Scaling of a model (relation 100) or of a wave's icon (610)
/// [soft 0x08222d5c animScalingUpdate]: curves 1/163 x (+0x100), 2/164 y (+0x104), 3/165 z
/// (+0x108). A curve without keys gives 1. The scaling bit of the update flags is raised
/// even when no curve wrote anything.
pub fn anim_scaling_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let mut mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    if relid(m, node) != 100 {
        mdl = h.wave_get_icon_model(m, mdl);
    }
    let mut i = 0;
    while i < nb_args(m, node) {
        let c = arg_curve(m, node, i);
        if c != 0 {
            let v = if fcv::nb_keys(m, c) < 1 { F80::ONE } else { fcv::value(m, c, time, 1) };
            match fcv::kind(m, c) {
                1 | 0xa3 => m.set_f32(mdl + model::SCAL, v.to_f32()),
                2 | 0xa4 => m.set_f32(mdl + model::SCAL + 4, v.to_f32()),
                3 | 0xa5 => m.set_f32(mdl + model::SCAL + 8, v.to_f32()),
                _ => {}
            }
        }
        i += 1;
    }
    raise(m, mdl, updflag::SCALING);
}

/// Rotation of a model (relation 101) or of a wave's icon (611)
/// [soft 0x08222e54 animRotationUpdate]: curves 4/166 x (+0x10c), 5/167 y (+0x110), 6/168 z
/// (+0x114), degrees → radians. The x and y angles are also copied to +0x164 and +0x168.
/// For a joint (kind 5) with algorithm 1, each curve that has keys also resets the
/// inverse-kinematics preferred angle of its axis (`SI_invKinResetPrefAngle(model, 1 | 2 |
/// 3, the three angles as they stand)`).
///
/// For a relation other than 101 and 611 the original uses an uninitialised model pointer;
/// the dispatcher never sends one.
pub fn anim_rotation_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let obj = data(m, node);
    if obj == 0 {
        return;
    }
    let mut joint = false;
    let mdl = match relid(m, node) {
        0x65 => {
            if m.i32(obj + model::KIND) == 5 {
                joint = m.i32(obj + model::ALGO_TYPE) == 1;
            }
            obj
        }
        0x263 => h.wave_get_icon_model(m, obj),
        _ => return,
    };
    let mut i = 0;
    while i < nb_args(m, node) {
        let c = arg_curve(m, node, i);
        if c != 0 {
            let v = (fcv::value(m, c, time, 1) * DEG2RAD).to_f32();
            let axis = match fcv::kind(m, c) {
                4 | 0xa6 => {
                    m.set_f32(mdl + model::VROT, v);
                    m.set_f32(mdl + model::VROT_XY_COPY, v);
                    1
                }
                5 | 0xa7 => {
                    m.set_f32(mdl + model::VROT + 4, v);
                    m.set_f32(mdl + model::VROT_XY_COPY + 4, v);
                    2
                }
                6 | 0xa8 => {
                    m.set_f32(mdl + model::VROT + 8, v);
                    3
                }
                _ => 0,
            };
            if axis != 0 && joint && fcv::nb_keys(m, c) > 0 {
                let (x, y, z) = (m.f32(mdl + model::VROT), m.f32(mdl + model::VROT + 4), m.f32(mdl + model::VROT + 8));
                h.inv_kin_reset_pref_angle(m, mdl, axis, x, y, z);
            }
        }
        i += 1;
    }
    raise(m, mdl, updflag::ROTATION);
}

/// The time of a curve's last key (`si_fcurveLastKey`, fcurve.c, returns the record).
fn last_key_time(m: &Mem, curve: Addr) -> f32 {
    let n = m.i32(curve + 0xc);
    m.f32(m.ptr(curve + 8).wrapping_add((n.wrapping_sub(1) as u32).wrapping_mul(0x24)))
}

/// Translation of a model (relation 113) or of a wave's icon (613)
/// [soft 0x08224594 animExplicitPosUpdate]: curves 139/169 x, 140/170 y, 141/171 z.
///
/// 1. The three components start from what the node has: for an inverse-kinematics target
///    (a joint with algorithm 1, or any node with algorithm 3) the goal of its IK record
///    when that is active (state word non-zero), else its global translation (+0x1b8);
///    for any other node its local translation (+0x158).
/// 2. Each active curve is evaluated (whatever its type) and replaces its component.
/// 3. With no curve of the six types, nothing more happens.
/// 4. A node that follows a **path** (+0x2c8) keeps its path position unless the time lies
///    within the translation curve's keys (first key ≤ time ≤ last key, of the last curve
///    seen) or the node is a dynamics node (algorithm 2 or 12) whose chain record is absent
///    or has its root-constraint word clear.
/// 5. A node that is position-constrained at this time (`SI_edIsItemConstrained(dict,
///    model, MODELS, 111, 1, time)`) is left alone.
/// 6. Otherwise the result goes to the IK goal (`SI_invKinSetGoal`) or to the local
///    translation, and the translation bit of the update flags is raised.
pub fn anim_explicit_pos_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let mut mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    if relid(m, node) != 0x71 {
        mdl = h.wave_get_icon_model(m, mdl);
    }
    let algo = m.i32(mdl + model::ALGO_TYPE);
    let ik_target = (m.i32(mdl + model::KIND) == 5 && algo == 1) || algo == 3;
    let from = if ik_target {
        let ik = m.ptr(mdl + model::INV_KIN);
        if ik == 0 || m.i32(ik + IK_STATE) == 0 {
            mdl + model::GTRS
        } else {
            ik + IK_GOAL
        }
    } else {
        mdl + model::TRANS
    };
    let mut v = [fmov(m.f32(from)), fmov(m.f32(from + 4)), fmov(m.f32(from + 8))];
    let mut last = 0;
    let mut i = 0;
    while i < nb_args(m, node) {
        let c = arg_curve(m, node, i);
        if c != 0 {
            let x = fcv::value(m, c, time, 1);
            match fcv::kind(m, c) {
                0x8b | 0xa9 => {
                    v[0] = x.to_f32();
                    last = c;
                }
                0x8c | 0xaa => {
                    v[1] = x.to_f32();
                    last = c;
                }
                0x8d | 0xab => {
                    v[2] = x.to_f32();
                    last = c;
                }
                _ => {}
            }
        }
        i += 1;
    }
    if last == 0 {
        return;
    }
    let algo = m.i32(mdl + model::ALGO_TYPE);
    let dyn_node = algo == 2 || algo == 0xc;
    let chn = m.ptr(mdl + model::DYNA_CHN_VARS);
    let mut over_path = dyn_node && (chn == 0 || m.i16(chn + 0x38) == 0);
    if !over_path && !dyn_node && m.ptr(mdl + model::PATH) != 0 {
        let t = F80::from_f32(time);
        let first = F80::from_f32(m.f32(m.ptr(last + 8)));
        if !t.lt(first) {
            let end = F80::from_f32(last_key_time(m, last));
            over_path = t.lt(end) || t.eq(end);
        }
    }
    if m.ptr(mdl + model::PATH) == 0 || over_path {
        let d = glob::cur_dict(m);
        let models = glob::chap(m, cid::MODELS);
        if h.ed_is_item_constrained(m, d, mdl, models, 0x6f, 1, time) == 0 {
            if ik_target {
                h.inv_kin_set_goal(m, mdl, v[0], v[1], v[2]);
            } else {
                m.set_f32(mdl + model::TRANS, v[0]);
                m.set_f32(mdl + model::TRANS + 4, v[1]);
                m.set_f32(mdl + model::TRANS + 8, v[2]);
            }
            raise(m, mdl, updflag::TRANSLATION);
        }
    }
}

/// The node's own step (relation −100; −101 for a wave, whose icon model is taken)
/// [soft 0x08225de0 SI_animTransfoUpdate]: the node's matrices are recomputed from its
/// local transformation and its father's state, `SI_modelUpdateSonState(father, node, 1)`
/// (with its descendants). With bit 2 of `SI_viewerUpdOpt` the fast variant is used
/// instead ([`fast_model_update_son_state`]).
pub fn anim_transfo_update<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, _time: f32) {
    let mut mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    if relid(m, node) != -100 {
        mdl = h.wave_get_icon_model(m, mdl);
    }
    if m.u8(glob::SI_VIEWER_UPD_OPT) & 4 == 0 {
        let father = m.ptr(mdl + model::FATHER);
        h.model_update_son_state(m, father, mdl, 1);
    } else {
        fast_model_update_son_state(m, h, mdl, node);
    }
}

/// The fast hierarchy update [soft 0x08225d40 fastModelUpdateSonState]: the node alone is
/// recomputed (`SI_modelUpdateSonState(father, node, 0)`), then each son in turn,
/// recursively, **unless** a later node of the update list is that son's own
/// transformation step (object equal to the son, relation −100 or −101): such a son will
/// be reached by the list. The list is scanned from the node after `node` to the record
/// whose chapter is −1, which `SI_elemdictTopoSort` appends when this mode is on.
pub fn fast_model_update_son_state<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, node: Addr) {
    let father = m.ptr(mdl + model::FATHER);
    h.model_update_son_state(m, father, mdl, 0);
    let mut son = m.ptr(mdl + model::SON);
    while son != 0 {
        let mut later = false;
        let mut n = node + updnode::SIZE;
        while m.i16(n + updnode::CHAPID) != -1 {
            if m.ptr(n + updnode::DATA) == son && (m.i32(n + updnode::RELID).wrapping_add(0x65) as u32) <= 1 {
                later = true;
                break;
            }
            n += updnode::SIZE;
        }
        if !later {
            fast_model_update_son_state(m, h, son, node);
        }
        son = m.ptr(son + model::BROTHER);
    }
}

/// Where an object is, for a constraint [soft 0x08222538 SI_constrLocation]: by graf kind,
/// 2 and 3 a model's global translation (+0x1b8); 4 a camera's position (+0x10), 5 its
/// interest (+0x1c); 6, 8, 0x13, 0x14 a light's position (+0x34), 7 its interest (+0x58).
/// Other kinds leave the output as it was.
pub fn constr_location(m: &mut Mem, obj: Addr, kind: i32, out: Addr) {
    if obj == 0 {
        return;
    }
    let from = match kind {
        2 | 3 => obj + model::GTRS,
        4 => obj + camera::POSITION,
        5 => obj + camera::INTEREST,
        6 | 8 | 0x13 | 0x14 => obj + light::POSITION,
        7 => obj + light::INTEREST,
        _ => return,
    };
    for k in 0..3 {
        let w = m.u32(from + 4 * k);
        m.set_u32(out + 4 * k, w);
    }
}

/// How an object is turned, for an orientation constraint
/// [soft 0x08222730 constrOrientation]: a model's local rotation angles (+0x10c), replaced,
/// when it has a father, by the angles extracted from its global rotation matrix (+0x178)
/// with the local ones as the "previous" angles; a camera's three floats at +0x28 (yaw,
/// pitch, roll: finding 0005); a light's (kinds 6, 8, 0x13) three floats at +0x4c.
pub fn constr_orientation(m: &mut Mem, obj: Addr, kind: i32, out: Addr) {
    if obj == 0 {
        return;
    }
    let from = match kind {
        2 | 3 => obj + model::VROT,
        4 => obj + 0x28,
        6 | 8 | 0x13 => obj + 0x4c,
        _ => return,
    };
    for k in 0..3 {
        let w = m.u32(from + 4 * k);
        m.set_u32(out + 4 * k, w);
    }
    if matches!(kind, 2 | 3) && m.ptr(obj + model::FATHER) != 0 {
        let grot = m.matrix(obj + model::GROT);
        let prev = m.vector(out);
        let keep = m.i16(super::params::KEEP_TURNS) != 0;
        let r = si_math::algebra::matrix_extract_xyz_rotation(&grot, &prev, keep);
        m.set_vector(out, r);
    }
}

/// A point times the 4 × 4 matrix at `mat`, as these routines compute it in the register:
/// component j = ((x·m[0][j] + y·m[1][j]) + z·m[2][j]) + m[3][j], rounded once by the
/// caller's store.
fn xform(m: &Mem, p: [f32; 3], mat: Addr) -> [F80; 3] {
    let e = |r: u32, c: u32| m.f32(mat + 16 * r + 4 * c);
    let mut out = [F80::ZERO; 3];
    for (j, o) in out.iter_mut().enumerate() {
        let j = j as u32;
        *o = ((F80::from_f32(p[1]) * e(1, j) + F80::from_f32(p[0]) * e(0, j)) + F80::from_f32(p[2]) * e(2, j)) + e(3, j);
    }
    out
}

/// A point of a path, given in the path model's own coordinates, as a **local translation**
/// of the model that follows the path [soft 0x082247e4 si_modelComputePathTranslation].
///
/// - A model with no father, or with algorithm 3, or a joint with algorithm 1: the point in
///   world coordinates, point × path model's global matrix (+0x1c4).
/// - Both under the same father: the point through the path model's **local**
///   transformation: scaling (+0x100) times rotation matrix (+0x118), plus translation
///   (+0x158). (The original scales the nine rotation entries first; two of the nine
///   products are used unrounded, seven after a store to `float`.)
/// - Otherwise: the world point, then times the inverse of the father's global matrix.
///   When that matrix has no inverse the original multiplies by whatever its local matrix
///   holds (it is not initialised); here by zeros.
pub fn model_compute_path_translation(m: &mut Mem, mdl: Addr, path: Addr, point: Addr) {
    let p = [m.f32(point), m.f32(point + 4), m.f32(point + 8)];
    let father = m.ptr(mdl + model::FATHER);
    let algo = m.i32(mdl + model::ALGO_TYPE);
    let r: [f32; 3];
    if father == 0 || algo == 3 || (m.i32(mdl + model::KIND) == 5 && algo == 1) {
        let w = xform(m, p, path + model::GMAT);
        r = [w[0].to_f32(), w[1].to_f32(), w[2].to_f32()];
    } else if m.ptr(path + model::FATHER) == father {
        let rot = m.matrix(path + model::MROT);
        let s = [m.f32(path + model::SCAL), m.f32(path + model::SCAL + 4), m.f32(path + model::SCAL + 8)];
        let t = [m.f32(path + model::TRANS), m.f32(path + model::TRANS + 4), m.f32(path + model::TRANS + 8)];
        // scaled rotation, row i times s[i]; kept: the unrounded product, r: after the store
        let kept = |i: usize, j: usize| F80::from_f32(s[i]) * rot[i][j];
        let rd = |i: usize, j: usize| kept(i, j).to_f32();
        let (x, y, z) = (p[0], p[1], p[2]);
        let rx = F80::from_f32(t[0]) + ((F80::from_f32(y) * rd(1, 0) + kept(0, 0) * x) + F80::from_f32(z) * rd(2, 0));
        let ry = F80::from_f32(t[1]) + (F80::from_f32(z) * rd(2, 1) + (F80::from_f32(y) * rd(1, 1) + F80::from_f32(x) * rd(0, 1)));
        let rz = (kept(2, 2) * z + (F80::from_f32(y) * rd(1, 2) + F80::from_f32(x) * rd(0, 2))) + t[2];
        r = [rx.to_f32(), ry.to_f32(), rz.to_f32()];
    } else {
        let w = xform(m, p, path + model::GMAT);
        let w = [w[0].to_f32(), w[1].to_f32(), w[2].to_f32()];
        let inv = si_math::algebra::matrix_inverse(&m.matrix(father + model::GMAT)).unwrap_or([[0.0; 4]; 4]);
        let e = |r: usize, c: usize| inv[r][c];
        let mut out = [0.0f32; 3];
        for (j, o) in out.iter_mut().enumerate() {
            *o = (((F80::from_f32(w[1]) * e(1, j) + F80::from_f32(w[0]) * e(0, j)) + F80::from_f32(w[2]) * e(2, j)) + e(3, j))
                .to_f32();
        }
        r = out;
    }
    m.set_f32(point, r[0]);
    m.set_f32(point + 4, r[1]);
    m.set_f32(point + 8, r[2]);
}

/// The percentage of a path as a position [soft 0x08224eec finalComputePathTrans].
///
/// `rel` is the path relation: 112 a model's, 612 a wave's (its icon model moves), 1112 /
/// 1114 a camera's position / interest, 2112 / 2114 a light's. `path` is the path model: a
/// spline (kind 6) or, for any other kind, a NURBS curve. `percent` is the value of the
/// percentage curve, 0 to 100.
///
/// 1. distance = length × (0.01 × percent), rounded to `float`. (For a NURBS curve the
///    length is rounded to `float` first.)
/// 2. The curve parameter at that distance, and the point there.
/// 3. For a model or wave: the point becomes a local translation
///    ([`model_compute_path_translation`]) and goes to the IK goal (joint with algorithm 1,
///    or algorithm 3) or to the local translation (+0x158). No update flag is raised here.
///    Then, if the element has a tangency constraint (relation 124 to a model, list 1), the
///    constraint's data record is created if missing and told where on the path the model
///    is: +0 = 1, +2 = whether the constraint's path is this path, +4 the percentage, +8
///    the distance, +0xc the point in the path's coordinates.
/// 4. For a camera or light: the point in world coordinates (× the path model's global
///    matrix) goes to the position or interest.
pub fn final_compute_path_trans<H: TransfoHost + ?Sized>(m: &mut Mem, h: &mut H, rel: i32, e: Addr, obj: Addr, path: Addr,
                                                        percent: f32) {
    let mdl = match rel {
        0x70 => obj,
        0x264 => h.wave_get_icon_model(m, obj),
        0x458 | 0x45a | 0x840 | 0x842 => 0,
        _ => return,
    };
    let pt = m.local(12);
    let dist: f32;
    let geom = m.ptr(path + model::DEFINITION);
    if m.i32(path + model::KIND) == 6 {
        let len = h.spline_length(m, geom);
        dist = (len * (F80::from_f64(0.01) * percent)).to_f32();
        let f = h.spline_distance_to_factor(m, geom, dist).to_f32();
        h.spline_value(m, geom, f, pt);
    } else {
        let len = h.nurbs_curve_length(m, geom).to_f32();
        dist = (F80::from_f32(len) * (F80::from_f64(0.01) * percent)).to_f32();
        let u = h.nurbs_distance_to_factor(m, geom, dist as f64).to_f64();
        let ret = m.local(24);
        h.nurbs_crv_value(m, ret, geom, u, 0);
        for k in 0..3 {
            let v = F80::from_f64(m.f64(ret + 8 * k)).to_f32();
            m.set_f32(pt + 4 * k, v);
        }
        m.release(24);
    }
    if rel == 0x70 || rel == 0x264 {
        let raw = [fmov(m.f32(pt)), fmov(m.f32(pt + 4)), fmov(m.f32(pt + 8))];
        model_compute_path_translation(m, mdl, path, pt);
        let (x, y, z) = (m.f32(pt), m.f32(pt + 4), m.f32(pt + 8));
        let algo = m.i32(mdl + model::ALGO_TYPE);
        if (m.i32(mdl + model::KIND) == 5 && algo == 1) || algo == 3 {
            h.inv_kin_set_goal(m, mdl, x, y, z);
        } else {
            m.set_f32(mdl + model::TRANS, x);
            m.set_f32(mdl + model::TRANS + 4, y);
            m.set_f32(mdl + model::TRANS + 8, z);
        }
        let d = glob::cur_dict(m);
        let models = glob::chap(m, cid::MODELS);
        let tan = ed::find_related_elem(m, d, e, true, 1, 0x7c, models);
        if tan.elem != 0 {
            let r = ed::get_cur_relation(m, e, true, 1);
            if r != 0 {
                let mut rec = m.ptr(r + arel::DATA);
                if rec == 0 {
                    rec = h.tan_constr_create_data(m);
                }
                m.set_ptr(r + arel::DATA, rec);
                let tan_path = m.ptr(tan.elem + elem::DATA);
                m.set_f32(rec + 4, fmov(percent));
                m.set_u16(rec + 2, (tan_path == path) as u16);
                m.set_u16(rec, 1);
                m.set_f32(rec + 8, dist);
                m.set_f32(rec + 0xc, raw[0]);
                m.set_f32(rec + 0x10, raw[1]);
                m.set_f32(rec + 0x14, raw[2]);
            }
        }
    } else {
        let p = [m.f32(pt), m.f32(pt + 4), m.f32(pt + 8)];
        let w = xform(m, p, path + model::GMAT);
        let at = match rel {
            0x458 => obj + camera::POSITION,
            0x45a => obj + camera::INTEREST,
            0x840 => obj + light::POSITION,
            _ => obj + light::INTEREST,
        };
        for k in 0..3 {
            m.set_f32(at + 4 * k as u32, w[k].to_f32());
        }
    }
    m.release(12);
}

/// The path step of a node (relations 112, 612, 1112, 1114, 2112, 2114)
/// [soft 0x08225290 animAllTypePathUpdate].
///
/// Among the node's arguments, the first whose chapter is MODELS is the path model and the
/// **last** whose chapter is ANIMATION is the percentage curve (curve 7, 172, 9, 10, 31,
/// 32 by relation). With both present the curve is evaluated and
/// [`final_compute_path_trans`] places the object.
///
/// For a model or wave the path yields to the node's translation curves: if the element
/// has a translation curve (relation 113 in the node's list) and either the node is a
/// dynamics node (algorithm 2 or 12) with no chain record or a clear root-constraint word,
/// or the node has a path and the time lies within that curve's keys, the path is not
/// applied (the translation step applies the curves instead: the two tests mirror each
/// other, see [`anim_explicit_pos_update`]).
pub fn anim_all_type_path_update<H: TransfoHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let rel = relid(m, node);
    let obj = data(m, node);
    if obj == 0 {
        return;
    }
    let d = glob::cur_dict(m);
    let models = glob::chap(m, cid::MODELS);
    let mut e = 0;
    let mut mdl = 0;
    match rel {
        0x458 | 0x45a | 0x840 | 0x842 => {}
        0x70 => {
            e = ed::get_elem(m, d, models, m.i32(node + updnode::ELEMID));
            mdl = obj;
        }
        0x264 => {
            mdl = h.wave_get_icon_model(m, obj);
            let d = glob::cur_dict(m);
            let waves = glob::chap(m, cid::WAVES);
            e = ed::get_elem(m, d, waves, m.i32(node + updnode::ELEMID));
        }
        _ => return,
    }
    let ani = glob::chap(m, cid::ANIMATION);
    let mut path = 0;
    let mut curve = 0;
    let n = nb_args(m, node);
    for i in 0..n {
        let a = arg(m, node, i);
        let c = m.i16(a + updelem::CHAPID) as i32;
        if c == models {
            if path == 0 {
                path = m.ptr(a + updelem::DATA);
            }
        } else if c == ani {
            curve = m.ptr(a + updelem::DATA);
        }
    }
    let mut yields = false;
    if rel == 0x70 || rel == 0x264 {
        let tc = ed::find_fcv(m, e, m.i16(node + updnode::GROUP) as i32, 0x71);
        let algo = m.i32(mdl + model::ALGO_TYPE);
        let dyn_node = algo == 2 || algo == 0xc;
        let chn = m.ptr(mdl + model::DYNA_CHN_VARS);
        if dyn_node && tc != 0 && (chn == 0 || m.i16(chn + 0x38) == 0) {
            yields = true;
        } else if !dyn_node && m.ptr(mdl + model::PATH) != 0 && tc != 0 && m.ptr(tc + 8) != 0 {
            let t = F80::from_f32(time);
            if !t.lt(F80::from_f32(m.f32(m.ptr(tc + 8)))) {
                let end = F80::from_f32(last_key_time(m, tc));
                yields = t.lt(end) || t.eq(end);
            }
        }
    }
    if path != 0 && curve != 0 && !yields {
        let v = fcv::value(m, curve, time, 1).to_f32();
        let obj = data(m, node);
        final_compute_path_trans(m, h, rel, e, obj, path, v);
    }
}

/// Places an object on its path from the percentage curve's **last evaluated** value (the
/// curve's field +0x14), without evaluating it [soft 0x082254e8 SI_setPathTranslation].
/// The element is found from the object (camera, light, or by a search of the chapter), the
/// path model from the element's relation `rel` to a model in list 1.
pub fn set_path_translation<H: TransfoHost + ?Sized>(m: &mut Mem, h: &mut H, rel: i32, chap: i32, obj: Addr, curve: Addr) {
    if curve == 0 {
        return;
    }
    let idx = m.local(4);
    let e = if chap == glob::chap(m, cid::CAMERAS) {
        h.ed_get_elem_from_camera(m, obj, idx)
    } else if chap == glob::chap(m, cid::LIGHTS) {
        h.ed_get_elem_from_light(m, obj, idx)
    } else {
        let d = glob::cur_dict(m);
        let i = h.elemdict_find_elem_by_data(m, d, chap, obj);
        if i < 0 {
            m.release(4);
            return;
        }
        let d = glob::cur_dict(m);
        ed::get_elem(m, d, chap, i)
    };
    m.release(4);
    if e == 0 {
        return;
    }
    let d = glob::cur_dict(m);
    let models = glob::chap(m, cid::MODELS);
    let r = ed::find_related_elem(m, d, e, true, 1, rel, models);
    if r.elem != 0 && m.ptr(r.elem + elem::DATA) != 0 {
        let path = m.ptr(r.elem + elem::DATA);
        let v = m.f32(curve + 0x14);
        final_compute_path_trans(m, h, rel, e, obj, path, v);
    }
}

/// Whether a model has any curve in its transformation or geometry lists
/// [soft 0x08229a90 SI_modelMove].
pub fn model_move<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) -> bool {
    let (e, _) = ed::get_elem_from_model(m, h, mdl);
    if e == 0 {
        return false;
    }
    ed::find_range_fcv(m, e, 1, 0, 14999) != 0 || ed::find_range_fcv(m, e, 2, 0, 14999) != 0
}

/// Whether a model is position-constrained [soft 0x08229aec SI_modelConstraintMove]:
/// `SI_edIsItemConstrained(dict, model, MODELS, 111, 0, 0.0)`.
pub fn model_constraint_move<H: UpdateHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) -> i32 {
    let d = glob::cur_dict(m);
    let models = glob::chap(m, cid::MODELS);
    h.ed_is_item_constrained(m, d, mdl, models, 0x6f, 0, 0.0) as i32
}
