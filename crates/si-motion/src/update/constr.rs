// SPDX-License-Identifier: MIT
//! Constraints of `motionUpdate.c`: a model held at, turned to or scaled like other objects,
//! on a plane, mirrored, between two or three points, on the points of a cluster, and the
//! trajectory (a path with one point a frame).
//!
//! A constraint is a relation of the constrained element's transformation list whose
//! arguments are the constraining objects. Every routine here takes the update node of such
//! a relation. An argument counts when it is active, has an object, and its activation
//! curve (if it has one) reads above zero at the time. What the constraint then sets goes
//! through the model's own setters (`SI_modelSetGlobalTranslation` and its kin, `model.c`,
//! finding 0015), the inverse-kinematics goal for a joint or an effector, or one of the
//! routines of `constraint.c`; those are host methods.
//!
//! The registers of the original are followed where a value stays in them: an average is
//! `1 / count` kept unrounded and multiplied into each sum, a sum is rounded to `float` at
//! every step, a direction is divided by its length rounded to `float`.

use super::transfo::{self, TransfoHost};
use super::{arg, data, nb_args, relid};
use crate::mem::MemExt;
use crate::ed;
use crate::fcv;
use crate::glob::{self, cid};
use si_mem::{Addr, Mem};
use crate::off::{arel, dict, elem, face, kluster, list, model, nurbs, splpoint, spline, updelem, updflag, updnode};
use si_math::algebra;
use si_math::types::Vector;
use si_x87::{fmov, x, Round, F80};

/// The double 0.0001 at 0x0864dca8 (and its copies 0x0864dc98, 0x0864dd48): a direction
/// shorter than this is not normalised; a frame nearer than this to a whole number is one.
pub const EPS: f64 = 0.0001;
/// The double at 0x0864dbd0: 0.333333, six digits, not a third.
pub const THIRD: f64 = 0.333333;
/// The float at 0x0864dcb0: the "no limit" distance of the plane constraint.
pub const FAR: f32 = 1e10;

/// The kinematic state record (model + 0x2f0), as `constrGetParamOrientation` reads it
/// (**new**): two vectors of three floats.
pub mod kine {
    /// Read under algorithm type 0x10.
    pub const VEC_30: u32 = 0x30;
    /// Read under algorithm type 0x11, and turned by the object's global rotation.
    pub const VEC_48: u32 = 0x48;
}

crate::host_trait! {
    /// What the constraint routines call outside this package.
    pub trait ConstrHost: TransfoHost {
        // ---- model.c (finding 0015: ported in `si-model` on typed records)
        /// `SI_modelSetGlobalTranslation(model, &vector, mode, geom)`.
        fn model_set_global_translation(mdl: Addr, v: Addr, mode: i32, geom: i32);
        /// `SI_modelSetGlobalRotation(model, &angles, mode, geom)`.
        fn model_set_global_rotation(mdl: Addr, v: Addr, mode: i32, geom: i32);
        /// `SI_modelSetGlobalScaling(model, &factors, mode, geom)`.
        fn model_set_global_scaling(mdl: Addr, v: Addr, mode: i32, geom: i32);
        /// `SI_modelSetRotation(model, &angles, mode, geom)`.
        fn model_set_rotation(mdl: Addr, v: Addr, mode: i32, geom: i32);
        /// `SI_modelComputeBounding(model)`.
        fn model_compute_bounding(mdl: Addr);
        /// `SI_elemGetVertex(dict, elem, chapter, geometry, 0, index, &vector, &flag)`.
        fn elem_get_vertex(dict: Addr, e: Addr, chap: i32, geom: i32, a: i32, index: i32, out: Addr, flag: Addr) -> i32;
        /// `SI_elemSetVertex(dict, elem, chapter, geometry, 0, index, &vector)`.
        fn elem_set_vertex(dict: Addr, e: Addr, chap: i32, geom: i32, a: i32, index: i32, v: Addr);
        /// `SI_modelGetData(model, 2, 0)`: the definition the constraint moves points of.
        fn model_get_data(mdl: Addr, geom: i32, a: i32) -> Addr;
        /// `SI_modelGetVertexPtr(definition, kind, index, &flag)`: three floats, or null.
        fn model_get_vertex_ptr(def: Addr, kind: i32, index: i32, flag: Addr) -> Addr;
        /// `SI_modelCheckCreateDefinition(model)`.
        fn model_check_create_definition(mdl: Addr);
        /// `SI_modelTouchGeometry(model)`.
        fn model_touch_geometry(mdl: Addr);

        // ---- klusters.c
        /// `SI_klusterGetTrueIndex(model, cluster, i)`: the vertex number of member `i`.
        fn kluster_get_true_index(mdl: Addr, k: Addr, i: i32) -> i32;
        /// `SI_klusterCountVertexRef(model, vertex, 1)`: in how many clusters the vertex is.
        fn kluster_count_vertex_ref(mdl: Addr, index: i32, flag: i32) -> i32;

        // ---- constraint.c
        /// `si_rotlimConstraint(model, index)`.
        fn rotlim_constraint(mdl: Addr, index: i32);
        /// `si_poslimConstraint(model, position, index)`: the vector by value.
        fn poslim_constraint(mdl: Addr, px: f32, py: f32, pz: f32, index: i32);
        /// `si_tanVectorFromPath(data, path, &vector)`.
        fn tan_vector_from_path(rec: Addr, path: Addr, out: Addr) -> i16;
        /// `si_tanConstrComputeTanVector(model, path, &vector)`.
        fn tan_constr_compute_tan_vector(mdl: Addr, path: Addr, out: Addr) -> i16;
        /// `si_tanConstrMapDirection(model, vector)`: the vector by value.
        fn tan_constr_map_direction(mdl: Addr, vx: f32, vy: f32, vz: f32) -> i16;
        /// `si_norConstrComputeNormalVec(model, surface, &normal)`.
        fn nor_constr_compute_normal_vec(mdl: Addr, surface: Addr, out: Addr) -> i16;
        /// `si_norConstrMapDirection(model, axis, normal)`: both vectors by value.
        fn nor_constr_map_direction(mdl: Addr, ax: f32, ay: f32, az: f32, nx: f32, ny: f32, nz: f32) -> i16;
        /// `si_constr3PtsMapDirection(model, direction, normal)`: both vectors by value.
        fn constr3pts_map_direction(mdl: Addr, dx: f32, dy: f32, dz: f32, nx: f32, ny: f32, nz: f32);
        /// `si_animModelBoundPlaneUpdate(model, plane object, distance, index, flag)`.
        fn anim_model_bound_plane_update(mdl: Addr, obj: Addr, dist: f32, index: i32, flag: i32);
        /// `si_animModelPivotPlaneUpdate(model, plane object, index, node)`.
        fn anim_model_pivot_plane_update(mdl: Addr, obj: Addr, index: i32, node: Addr);
        /// `si_animPivotConstrInit(node)`.
        fn anim_pivot_constr_init(node: Addr);

        // ---- other packages
        /// `SI_invKinSetCnsPosition(model, kind, position, time)` (invkin.c): the vector by
        /// value; kind 1 the preferred axis (relation 129), 2 the up vector (128).
        fn inv_kin_set_cns_position(mdl: Addr, kind: i32, px: f32, py: f32, pz: f32, time: f32);
        /// `si_symmetryVector(model, other, component, data, 0, &vector)` (symmetry.c).
        fn symmetry_vector(mdl: Addr, other: Addr, component: i32, rec: Addr, flag: i32, out: Addr);
        /// `SI_edIsElementAnimated(dict, chapter, elem, 1, 0)` (edanim.c).
        fn ed_is_element_animated(dict: Addr, chap: i32, e: Addr, a: i32, b: i32) -> i16;
        /// `si_getExplPathFirstFrameFromRelation(relation)` (convert.c).
        fn get_expl_path_first_frame_from_relation(rel: Addr) -> i32;
        /// `SI_nurbsCrvMovePoint(curve, index, x, y, z)` (nrb_misc.c).
        fn nurbs_crv_move_point(crv: Addr, index: i32, px: f64, py: f64, pz: f64);
        /// `SI_nurbsSrfMovePoint(surface, index, x, y, z, w)` (nrb_misc.c).
        fn nurbs_srf_move_point(srf: Addr, index: i32, px: f64, py: f64, pz: f64, w: f64);
        /// `SI_knotsCrvReparametrization(curve)` (nrb_b.c).
        fn knots_crv_reparametrization(crv: Addr);
        /// `SI_knotsSrfReparametrization(surface)` (nrb_b.c).
        fn knots_srf_reparametrization(srf: Addr);
        /// `SI_patchComputeSurface(patch, 0)` (patch.c).
        fn patch_compute_surface(patch: Addr, flag: i32);
        /// `SI_splineUpdateMatrix(spline)` (spline.c).
        fn spline_update_matrix(spl: Addr);
        /// `SI_splineTouch(spline)` (spline.c).
        fn spline_touch(spl: Addr);
    }
}

fn v3(m: &Mem, a: Addr) -> [f32; 3] {
    [m.f32(a), m.f32(a + 4), m.f32(a + 8)]
}

fn set_v3(m: &mut Mem, a: Addr, v: [f32; 3]) {
    m.set_f32(a, v[0]);
    m.set_f32(a + 4, v[1]);
    m.set_f32(a + 8, v[2]);
}

/// Three words moved as integers.
fn copy3(m: &mut Mem, to: Addr, from: Addr) {
    for k in 0..3 {
        let w = m.u32(from + 4 * k);
        m.set_u32(to + 4 * k, w);
    }
}

/// A sum divided by a count as these routines do it: the reciprocal stays in the register
/// and multiplies each component, which the store rounds.
fn average(count: i32, s: [f32; 3]) -> [f32; 3] {
    let r = F80::ONE / F80::from_i32(count);
    [(r * s[0]).to_f32(), (r * s[1]).to_f32(), (r * s[2]).to_f32()]
}

/// `sum += v`, each component rounded to `float`.
fn accumulate(sum: &mut [f32; 3], v: [f32; 3]) {
    for k in 0..3 {
        sum[k] = (x(sum[k]) + v[k]).to_f32();
    }
}

/// Whether an argument's activation curve lets it through: no curve, or an integer value
/// above zero at the time (`SI_fcurveIntegerValue(curve, time, 0, 1)`).
fn activated(m: &mut Mem, a: Addr, time: f32) -> bool {
    let act = m.ptr(a + updelem::ACT_FCV);
    act == 0 || fcv::integer_value(m, act, time, 0, 1) > 0
}

/// A joint, or a null that is an effector (algorithm 3 or 4): such a node is placed through
/// its inverse-kinematics goal, not through its own transformation.
fn ik_node(m: &Mem, mdl: Addr) -> bool {
    let kind = m.i32(mdl + model::KIND);
    kind == 5 || (kind == 0 && (m.i32(mdl + model::ALGO_TYPE).wrapping_sub(3) as u32) <= 1)
}

/// The node's element by plain indexing of the chapter's array (no range or deletion test).
fn node_elem_raw(m: &Mem, node: Addr) -> Addr {
    let d = glob::cur_dict(m);
    let l = m.ptr(d + dict::LISTS).wrapping_add((m.i16(node + updnode::CHAPID) as u32).wrapping_mul(list::SIZE));
    m.ptr(l + list::ELEMS).wrapping_add((m.i32(node + updnode::ELEMID) as u32).wrapping_mul(elem::SIZE))
}

/// The inlined `SI_secToFrame` with its halves as doubles: `(int)(v + 0.5)` for v above
/// zero, `(int)(v − 0.5)` otherwise, truncated.
fn nearest(v: F80) -> i32 {
    let h = if F80::ZERO.lt(v) { 0.5f64 } else { -0.5f64 };
    (v + h).to_i32(Round::Zero)
}

/// The centre of a cluster's points in world space [soft 0x082225bc constrAvgPntLocation]:
/// the points (`SI_elemGetVertex` of the model's element, geometry 2) are summed in
/// `float`s, divided by their number when there are two or more, and brought through the
/// global matrix of the node the model's geometry lives in (model + 0x44). The result of
/// `SI_elemGetVertex` is not looked at: a vertex it does not deliver adds whatever the
/// local held (here: the previous vertex, or zero).
pub fn constr_avg_pnt_location<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, k: Addr, out: Addr) {
    let (e, _) = ed::get_elem_from_model(m, h, mdl);
    let v = m.local(12);
    let flag = m.local(4);
    let mut sum = [0.0f32; 3];
    let mut i = 0;
    while i < m.i32(k + kluster::NB) {
        let idx = h.kluster_get_true_index(m, mdl, k, i);
        let d = glob::cur_dict(m);
        let models = glob::chap(m, cid::MODELS);
        h.elem_get_vertex(m, d, e, models, 2, 0, idx, v, flag);
        let p = v3(m, v);
        accumulate(&mut sum, p);
        i += 1;
    }
    m.release(4);
    m.release(12);
    if i > 1 {
        sum = average(i, sum);
    }
    let g = m.ptr(mdl + model::LATDEP) + model::GMAT;
    let e = |r: u32, c: u32| m.f32(g + 16 * r + 4 * c);
    let [sx, sy, sz] = sum;
    let ox = ((x(sy) * e(1, 0) + x(sx) * e(0, 0)) + x(sz) * e(2, 0)) + e(3, 0);
    let oy = ((x(sx) * e(0, 1) + x(sy) * e(1, 1)) + x(sz) * e(2, 1)) + e(3, 1);
    let oz = ((x(sy) * e(1, 2) + x(sx) * e(0, 2)) + x(sz) * e(2, 2)) + e(3, 2);
    let r = [ox.to_f32(), oy.to_f32(), oz.to_f32()];
    set_v3(m, out, r);
}

/// The direction a quick-stretch control hands to its node
/// [soft 0x082227ac constrGetParamOrientation]: for a model object (graf kinds 2, 3) the
/// output is zeroed; then, if the object has a kinematic state record (+0x2f0), by the
/// algorithm type of the **constrained** node: 0x10 the record's vector at +0x30 as it is,
/// 0x11 its vector at +0x48 through the object's global rotation matrix (+0x178, row 3
/// added).
pub fn constr_get_param_orientation(m: &mut Mem, obj: Addr, kind: i32, mdl: Addr, out: Addr) {
    if obj == 0 || (kind.wrapping_sub(2) as u32) > 1 {
        return;
    }
    set_v3(m, out, [0.0; 3]);
    let k = m.ptr(obj + model::KINE_INFO);
    if k == 0 {
        return;
    }
    match m.i32(mdl + model::ALGO_TYPE) {
        0x10 => {
            let v = v3(m, k + kine::VEC_30);
            set_v3(m, out, [fmov(v[0]), fmov(v[1]), fmov(v[2])]);
        }
        0x11 => {
            let [vx, vy, vz] = v3(m, k + kine::VEC_48);
            let g = obj + model::GROT;
            let e = |r: u32, c: u32| m.f32(g + 16 * r + 4 * c);
            let ox = ((x(vx) * e(0, 0) + x(vy) * e(1, 0)) + x(vz) * e(2, 0)) + e(3, 0);
            let oy = ((x(vy) * e(1, 1) + x(vx) * e(0, 1)) + x(vz) * e(2, 1)) + e(3, 1);
            let oz = ((x(vy) * e(1, 2) + x(vx) * e(0, 2)) + x(vz) * e(2, 2)) + e(3, 2);
            let r = [ox.to_f32(), oy.to_f32(), oz.to_f32()];
            set_v3(m, out, r);
        }
        _ => {}
    }
}

/// Turns a model so that its y axis points along `v` [soft 0x082228b4 modelSetParamRotation]:
/// `SI_matrixAlignYAxes(SI_xAxis, v)`, its angles (taken out with zeros as the previous
/// solution), `SI_modelSetRotation(model, &angles, 1, 0)`. Nothing for a vector the
/// alignment refuses.
pub fn model_set_param_rotation<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, v: [f32; 3]) {
    let xa = m.vector(glob::SI_X_AXIS);
    let Some(mat) = algebra::matrix_align_y_axes(&xa, &Vector::new(v[0], v[1], v[2])) else { return };
    let keep = m.i16(super::params::KEEP_TURNS) != 0;
    let r = algebra::matrix_extract_xyz_rotation(&mat, &Vector::new(0.0, 0.0, 0.0), keep);
    let ang = m.local(12);
    m.set_vector(ang, r);
    h.model_set_rotation(m, mdl, ang, 1, 0);
    m.release(12);
}

/// The direction constraint's turn [soft 0x08222914 directionConstraint]: the model's x axis
/// along `dir`, its y axis towards "up".
///
/// Up is (0, 1, 0), through the father's global rotation matrix when the node has a father
/// (`SI_vectorMultiplyMatrix`, row 3 added); or, when the element has an up-vector
/// constraint (relation 128 to a model, list 1), the vector from the node to that model
/// (differences of the global translations). `SI_matrixAlignAxes(dir, up)`; the local
/// rotation matrix (+0x118) becomes Rx(the float at +0x164) × that; its angles (previous
/// solution zero) go to `SI_modelSetGlobalRotation(model, &angles, 1, 0)`. False when the
/// alignment refuses the direction (shorter than 0.0001 in every component).
pub fn direction_constraint<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, dir: Addr) -> bool {
    let d = v3(m, dir);
    let d = Vector::new(fmov(d[0]), fmov(d[1]), fmov(d[2]));
    let mut up = Vector::new(0.0, 1.0, 0.0);
    let (e, _) = ed::get_elem_from_model(m, h, mdl);
    let dc = glob::cur_dict(m);
    let models = glob::chap(m, cid::MODELS);
    let r = ed::find_related_elem(m, dc, e, true, 1, 0x80, models);
    if r.elem != 0 {
        let o = m.ptr(r.elem + elem::DATA);
        let (a, b) = (v3(m, o + model::GTRS), v3(m, mdl + model::GTRS));
        up = Vector::new((x(a[0]) - b[0]).to_f32(), (x(a[1]) - b[1]).to_f32(), (x(a[2]) - b[2]).to_f32());
    } else {
        let father = m.ptr(mdl + model::FATHER);
        if father != 0 {
            up = algebra::vector_multiply_matrix(&up, &m.matrix(father + model::GROT));
        }
    }
    let (align, ok) = algebra::matrix_align_axes(&d, &up);
    if !ok {
        return false;
    }
    let rx = algebra::matrix_x_rotation(fmov(m.f32(mdl + model::VROT_XY_COPY)));
    let mrot = algebra::matrix_multiply(&rx, &align);
    m.set_matrix(mdl + model::MROT, &mrot);
    let keep = m.i16(super::params::KEEP_TURNS) != 0;
    let r = algebra::matrix_extract_xyz_rotation(&mrot, &Vector::new(0.0, 0.0, 0.0), keep);
    let ang = m.local(12);
    m.set_vector(ang, r);
    h.model_set_global_rotation(m, mdl, ang, 1, 0);
    m.release(12);
    true
}

/// Whether a node may be turned by a tangency or normal constraint: not a chain root (a
/// null with algorithm 1 or 2), not a joint, not an effector (a null with algorithm 3, 4).
fn turnable(m: &Mem, mdl: Addr) -> bool {
    let kind = m.i32(mdl + model::KIND);
    let algo = m.i32(mdl + model::ALGO_TYPE);
    !(kind == 0 && (algo.wrapping_sub(1) as u32) <= 1) && kind != 5 && !(kind == 0 && (algo.wrapping_sub(3) as u32) <= 1)
}

/// The tangency constraint (relation 124) [soft 0x08222a88 tangencyConstraint]: the node's
/// x axis along the tangent of a curve (`path`: a spline, kind 6, or a NURBS curve, kind 9).
///
/// The tangent comes from the relation's data record when it is set (its first word; the
/// path step fills it, finding 0017 section 8: the node is **on** that path), through
/// `si_tanVectorFromPath`; else from `si_tanConstrComputeTanVector(model, path)`. A node
/// that reaches here without the record gets it from its element's relation 124 (list 1)
/// first. The vector starts as (1, 0, 0); the turn is `si_tanConstrMapDirection`, whose
/// result is returned.
pub fn tangency_constraint<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, mdl: Addr, path: Addr) -> i32 {
    if mdl == 0 || path == 0 || !turnable(m, mdl) {
        return 0;
    }
    let pk = m.i32(path + model::KIND);
    if pk != 6 && pk != 9 {
        return 0;
    }
    let v = m.local(12);
    set_v3(m, v, [1.0, 0.0, 0.0]);
    if m.ptr(node + updnode::REL_DATA) == 0 {
        let e = node_elem_raw(m, node);
        let d = glob::cur_dict(m);
        let models = glob::chap(m, cid::MODELS);
        if ed::find_related_elem(m, d, e, true, 1, 0x7c, models).elem != 0 {
            let r = ed::get_cur_relation(m, e, true, 1);
            if r != 0 {
                let rec = m.ptr(r + arel::DATA);
                m.set_ptr(node + updnode::REL_DATA, rec);
            }
        }
    }
    let rec = m.ptr(node + updnode::REL_DATA);
    let ok = if rec != 0 && m.i16(rec) != 0 {
        h.tan_vector_from_path(m, rec, path, v)
    } else {
        h.tan_constr_compute_tan_vector(m, mdl, path, v)
    };
    let r = if ok == 0 {
        0
    } else {
        let t = v3(m, v);
        h.tan_constr_map_direction(m, mdl, t[0], t[1], t[2]) as i32
    };
    m.release(12);
    r
}

/// The normal constraint (relation 125) [soft 0x08222c18 normalToSurfaceConstraint]: the
/// node's x axis along the normal of a surface (a patch, kind 1, or a mesh, kind 4) as
/// `si_norConstrComputeNormalVec` gives it; a normal whose three components are all below
/// 0.0001 in size is not applied. The turn is `si_norConstrMapDirection(model, (1, 0, 0),
/// normal)`, whose result is returned.
pub fn normal_to_surface_constraint<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, surface: Addr) -> i32 {
    if mdl == 0 || surface == 0 || !turnable(m, mdl) {
        return 0;
    }
    let sk = m.i32(surface + model::KIND);
    if sk != 1 && sk != 4 {
        return 0;
    }
    let n = m.local(12);
    let ok = h.nor_constr_compute_normal_vec(m, mdl, surface, n);
    let v = v3(m, n);
    m.release(12);
    if ok == 0 {
        return 0;
    }
    let eps = x(EPS);
    let small = |c: f32| (x(0.0f32) - c).abs().lt(eps);
    // the first component is tested the other way round: "the tolerance is above it"
    if eps.gt((x(0.0f32) - v[0]).abs()) && small(v[1]) && small(v[2]) {
        return 0;
    }
    h.nor_constr_map_direction(m, mdl, 1.0, 0.0, 0.0, v[0], v[1], v[2]) as i32
}

/// The constraints with one rule for all their arguments (relations 102 position limit,
/// 103 rotation limit, 111 position, 114 orientation, 115 scaling, 116 direction, 124
/// tangency, 125 normal, 128 up vector, 129 preferred axis, 238 quick-stretch control,
/// 806 nail) [soft 0x08223ab8 si_animModelConstraintUpdate].
///
/// **Collecting.** For every argument that is active, passes its activation curve and has
/// an object, unless the node is a dynamics node (algorithm 2, 12 or 4): the object's graf
/// kind is asked (`SI_dbChapter2GrafType`); an argument of list 1 or 2 counts, and by
/// relation:
///
/// | Relation | Taken from the object |
/// |---|---|
/// | 111, 806 | its position ([`transfo::constr_location`]), summed |
/// | 114 | its orientation ([`transfo::constr_orientation`]), summed |
/// | 115 | a model's global scaling (+0x16c; under the classical method, `SI_glbManip` = 1, the scaling at +0x3a0), summed; an object that is no model adds the previous object's again |
/// | 116 | the direction from the node (row 3 of its global matrix) to the object's position, normalised when longer than 0.0001 (the length rounded to `float`, the differences unrounded), summed |
/// | 102, 128, 129 | its position, kept (the last one wins) |
/// | 103 | its orientation, kept |
/// | 238 | its position, kept, and [`constr_get_param_orientation`] |
///
/// **Applying**, when at least one argument counted. For a joint or an effector: 111 and
/// 806 make the average position the inverse-kinematics goal (806 not for a dynamics
/// node); 128 and 129 hand the last position to `SI_invKinSetCnsPosition` (kind 2 and 1);
/// 102 and 103 go to the limit routines as for any node; 114 as below; nothing else. For
/// any other node: 111 `SI_modelSetGlobalTranslation(average)`; 114 the average angles are
/// written into the local rotation (+0x10c) and given to `SI_modelSetGlobalRotation`; 115
/// `SI_modelSetGlobalScaling(average)`; 116 [`direction_constraint`] of the average
/// direction; 124 [`tangency_constraint`] and 125 [`normal_to_surface_constraint`] with the
/// **last** argument that passed the first three tests (it may be null); 238 the position
/// and [`model_set_param_rotation`]; 102 `si_poslimConstraint(model, last position,
/// index)`; 103 `si_rotlimConstraint(model, index)`.
///
/// For a joint under 103 the orientation of the last object is taken once more before the
/// limit routine, with the graf kind the loop computed last.
pub fn anim_model_constraint_update<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32, index: i32) {
    let mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    let loc = m.local(12);
    let ori = m.local(12);
    let out = m.local(12);
    apply_constraint(m, h, node, time, index, mdl, loc, ori, out);
    m.release(12);
    m.release(12);
    m.release(12);
}

#[allow(clippy::too_many_arguments)]
fn apply_constraint<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32, index: i32, mdl: Addr, loc: Addr,
                                           ori: Addr, out: Addr) {
    let mut scl = [0.0f32; 3];
    let (mut sum_loc, mut sum_ori, mut sum_scl) = ([0.0f32; 3], [0.0f32; 3], [0.0f32; 3]);
    let own = v3(m, mdl + model::GMAT + 0x30);
    let own = [fmov(own[0]), fmov(own[1]), fmov(own[2])];
    let mut count = 0;
    // the original keeps these two in a register and a local it never initialises; both
    // are set before any use that a non-zero count allows
    let mut obj: Addr = 0;
    let mut kind = 0;
    let rel = relid(m, node);
    let mut i = 0;
    while i < nb_args(m, node) {
        let a = arg(m, node, i);
        i += 1;
        if m.i16(a + updelem::ACTIVE) == 0 || !activated(m, a, time) {
            continue;
        }
        obj = m.ptr(a + updelem::DATA);
        if obj == 0 {
            continue;
        }
        let algo = m.i32(mdl + model::ALGO_TYPE);
        if algo == 2 || algo == 0xc || algo == 4 {
            continue;
        }
        kind = h.db_chapter2graf_type(m, m.i16(a + updelem::CHAPID) as i32, m.i32(a + updelem::ELEMID));
        if m.u16(a + updelem::GROUP).wrapping_sub(1) > 1 {
            continue;
        }
        count += 1;
        match rel {
            0x73 => {
                if (kind.wrapping_sub(2) as u32) <= 1 {
                    let from = if m.i32(glob::SI_GLB_MANIP) == 1 { obj + model::CSCL } else { obj + model::GSCL };
                    scl = v3(m, from);
                }
                accumulate(&mut sum_scl, scl);
            }
            0x67 => transfo::constr_orientation(m, obj, kind, ori),
            0x66 | 0x80 | 0x81 => transfo::constr_location(m, obj, kind, loc),
            0x6f | 0x326 => {
                transfo::constr_location(m, obj, kind, loc);
                let p = v3(m, loc);
                accumulate(&mut sum_loc, p);
            }
            0x72 => {
                transfo::constr_orientation(m, obj, kind, ori);
                let p = v3(m, ori);
                accumulate(&mut sum_ori, p);
            }
            0x74 => {
                transfo::constr_location(m, obj, kind, loc);
                let p = v3(m, loc);
                let (dx, dy, dz) = (x(p[0]) - own[0], x(p[1]) - own[1], x(p[2]) - own[2]);
                let len = (dz * dz + (dy * dy + dx * dx)).sqrt().to_f32();
                let d = if x(len).abs().gt(x(EPS)) {
                    [(dx / len).to_f32(), (dy / len).to_f32(), (dz / len).to_f32()]
                } else {
                    [dx.to_f32(), dy.to_f32(), dz.to_f32()]
                };
                set_v3(m, loc, d);
                accumulate(&mut sum_loc, d);
            }
            0xee => {
                transfo::constr_location(m, obj, kind, loc);
                constr_get_param_orientation(m, obj, kind, mdl, ori);
            }
            _ => {}
        }
    }
    if count == 0 {
        return;
    }
    let last = v3(m, loc);
    let poslim = |m: &mut Mem, h: &mut H| h.poslim_constraint(m, mdl, last[0], last[1], last[2], index);
    let orient = |m: &mut Mem, h: &mut H| {
        let a = average(count, sum_ori);
        set_v3(m, out, a);
        copy3(m, mdl + model::VROT, out);
        h.model_set_global_rotation(m, mdl, out, 1, 0);
    };
    if ik_node(m, mdl) {
        match rel {
            0x81 => h.inv_kin_set_cns_position(m, mdl, 1, last[0], last[1], last[2], time),
            0x80 => h.inv_kin_set_cns_position(m, mdl, 2, last[0], last[1], last[2], time),
            0x6f | 0x326 => {
                let algo = m.i32(mdl + model::ALGO_TYPE);
                if rel == 0x326 && (algo == 2 || algo == 0xc) {
                    return;
                }
                let a = average(count, sum_loc);
                h.inv_kin_set_goal(m, mdl, a[0], a[1], a[2]);
            }
            0x67 => {
                if obj == 0 {
                    return;
                }
                transfo::constr_orientation(m, obj, kind, ori);
                h.rotlim_constraint(m, mdl, index);
            }
            0x66 => poslim(m, h),
            0x72 => orient(m, h),
            _ => {}
        }
        return;
    }
    match rel {
        0x73 => {
            let a = average(count, sum_scl);
            set_v3(m, out, a);
            h.model_set_global_scaling(m, mdl, out, 1, 0);
        }
        0x67 => h.rotlim_constraint(m, mdl, index),
        0x66 => poslim(m, h),
        0x6f => {
            let a = average(count, sum_loc);
            set_v3(m, out, a);
            h.model_set_global_translation(m, mdl, out, 1, 0);
        }
        0x72 => orient(m, h),
        0x7c => {
            tangency_constraint(m, h, node, mdl, obj);
        }
        0x74 => {
            let a = average(count, sum_loc);
            set_v3(m, out, a);
            direction_constraint(m, h, mdl, out);
        }
        0x7d => {
            normal_to_surface_constraint(m, h, mdl, obj);
        }
        0xee => {
            h.model_set_global_translation(m, mdl, loc, 1, 0);
            let o = v3(m, ori);
            model_set_param_rotation(m, h, mdl, o);
        }
        _ => {}
    }
}

/// Whether a trajectory point is locked at this time
/// [soft 0x08221f88 animCheckTrajectoryLock]: the node's element follows a trajectory
/// (relation 160 to a model, list 1) whose spline has one point a frame from the first
/// frame kept in the relation (`si_getExplPathFirstFrameFromRelation`); the point of the
/// time's frame is locked when its 16-bit word at +0x2c is set. Under the pivot plane
/// constraint (118) a locked point followed by a free one starts the pivot again
/// (`si_animPivotConstrInit(node)`).
pub fn anim_check_trajectory_lock<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) -> bool {
    let d = glob::cur_dict(m);
    let e = ed::get_elem(m, d, m.i16(node + updnode::CHAPID) as i32, m.i32(node + updnode::ELEMID));
    let models = glob::chap(m, cid::MODELS);
    let r = ed::find_related_elem(m, d, e, true, 1, 0xa0, models);
    if r.elem == 0 {
        return false;
    }
    let spl = m.ptr(m.ptr(r.elem + elem::DATA) + model::DEFINITION);
    let rel = ed::get_cur_relation(m, e, true, 1);
    let first = h.get_expl_path_first_frame_from_relation(m, rel);
    let f = x(time) * glob::frame_rate(m);
    let h5 = if F80::ZERO.lt(x(time)) { 0.5f64 } else { -0.5f64 };
    let i = (f + h5).to_i32(Round::Zero).wrapping_sub(first);
    let n = m.u16(spl + spline::NB_POINTS) as i32;
    if i < 0 || i >= n {
        return false;
    }
    let p = m.ptr(spl + spline::POINTS).wrapping_add((i as u32).wrapping_mul(splpoint::SIZE));
    if m.i16(p + splpoint::LOCKED) == 0 {
        return false;
    }
    if relid(m, node) == 0x76 && i < n - 1 && m.i16(p + splpoint::SIZE + splpoint::LOCKED) == 0 {
        h.anim_pivot_constr_init(m, node);
    }
    true
}

/// The plane constraints (117 bounding plane, 118 pivot plane)
/// [soft 0x08224434 si_animModelPlaneConstrUpdate].
///
/// With `index` 1 a zero `v` ends the routine (the vector is read only then). A locked
/// trajectory point ([`anim_check_trajectory_lock`]) ends it too. Otherwise the node is
/// brought up to date (`SI_modelUpdateSonState(father, node, 1)`; the node is not tested
/// for null), and for every argument of list 1 that is active, has an object and passes
/// its activation curve: the object's bounding box is computed and, under 117,
/// `si_animModelBoundPlaneUpdate(node, object, 1e10, index, 0)`; under 118
/// `si_animModelPivotPlaneUpdate(node, object, index, update node)`, or for `index` 2 the
/// bounding form with a distance of 0.
pub fn anim_model_plane_constr_update<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32, index: i32,
                                                               v: Addr) {
    if index == 1 {
        let p = v3(m, v);
        if F80::ZERO.eq(x(p[0])) && x(p[1]).eq(F80::ZERO) && x(p[2]).eq(F80::ZERO) {
            return;
        }
    }
    if anim_check_trajectory_lock(m, h, node, time) {
        return;
    }
    let mdl = data(m, node);
    let father = m.ptr(mdl + model::FATHER);
    h.model_update_son_state(m, father, mdl, 1);
    let mut i = 0;
    while i < nb_args(m, node) {
        let a = arg(m, node, i);
        i += 1;
        if m.ptr(a + updelem::DATA) == 0 || m.i16(a + updelem::ACTIVE) == 0 || m.i16(a + updelem::GROUP) != 1 {
            continue;
        }
        if !activated(m, a, time) {
            continue;
        }
        let obj = m.ptr(a + updelem::DATA);
        h.model_compute_bounding(m, obj);
        match relid(m, node) {
            0x75 => h.anim_model_bound_plane_update(m, mdl, obj, FAR, index, 0),
            0x76 => {
                if index == 2 {
                    h.anim_model_bound_plane_update(m, mdl, obj, 0.0, 2, 0);
                } else {
                    h.anim_model_pivot_plane_update(m, mdl, obj, index, node);
                }
            }
            _ => {}
        }
    }
}

/// The symmetry constraint (150) [soft 0x082241dc si_animModelSymConstrUpdate]: two models
/// mirror each other; which one follows is decided here.
///
/// The node's object and its first argument's object are the pair; the relation's data
/// (the symmetry record) must be there with its 16-bit word at +2 clear. In the update
/// (`index` −1) the follower is the node, unless the node's element is animated
/// (`SI_edIsElementAnimated`) and the other's is not: then the other follows. In a
/// manipulation (`index` 0, 1 or 2) both must be on screen and the one that is **not**
/// selected follows (nothing when both are selected). A follower that is a dynamics node
/// (algorithm 2, 12, 4) is left alone. Then, for scaling (0), rotation (1) and translation
/// (2), all three in the update: `si_symmetryVector(follower, leader, component, record,
/// 0, &v)` and the matching global setter, or the inverse-kinematics goal for the
/// translation of a joint with algorithm 1 or any node with algorithm 3.
pub fn anim_model_sym_constr_update<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, _time: f32, index: i32) {
    let all = index == -1;
    let mut follower = data(m, node);
    if follower == 0 {
        return;
    }
    let first = m.ptr(m.ptr(node + updnode::ARGS) + updelem::DATA);
    let mut leader = first;
    if first == 0 {
        return;
    }
    let rec = m.ptr(node + updnode::REL_DATA);
    if rec == 0 || m.i16(rec + 2) != 0 {
        return;
    }
    let swap;
    if all {
        let (ea, _) = ed::get_elem_from_model(m, h, follower);
        let (eb, _) = ed::get_elem_from_model(m, h, first);
        let d = glob::cur_dict(m);
        let models = glob::chap(m, cid::MODELS);
        swap = h.ed_is_element_animated(m, d, models, ea, 1, 0) != 0 && {
            let d = glob::cur_dict(m);
            h.ed_is_element_animated(m, d, models, eb, 1, 0) == 0
        };
    } else {
        let gl = glob::cur_graf_list(m);
        let ga = h.find_gl_data(m, gl, follower, 2);
        let gl = glob::cur_graf_list(m);
        let gb = h.find_gl_data(m, gl, first, 2);
        if ga == 0 || gb == 0 {
            return;
        }
        if m.i32(ga + 8) != 0 {
            if m.i32(gb + 8) != 0 {
                return;
            }
            swap = true;
        } else {
            swap = false;
        }
    }
    if swap {
        leader = follower;
        follower = first;
    }
    if leader == 0 {
        return;
    }
    let algo = m.i32(follower + model::ALGO_TYPE);
    if algo == 2 || algo == 0xc || algo == 4 {
        return;
    }
    let (lo, hi) = if all { (0, 2) } else { (index, index) };
    let v = m.local(12);
    let mut i = lo;
    while i <= hi {
        h.symmetry_vector(m, follower, leader, i, rec, 0, v);
        match i {
            0 => h.model_set_global_scaling(m, follower, v, 1, 0),
            1 => h.model_set_global_rotation(m, follower, v, 1, 0),
            2 => {
                let algo = m.i32(follower + model::ALGO_TYPE);
                if (m.i32(follower + model::KIND) == 5 && algo == 1) || algo == 3 {
                    let p = v3(m, v);
                    h.inv_kin_set_goal(m, follower, p[0], p[1], p[2]);
                } else {
                    h.model_set_global_translation(m, follower, v, 1, 0);
                }
            }
            _ => {}
        }
        i += 1;
    }
    m.release(12);
}

/// Whether every argument of the node passes its activation curve (the two- and
/// three-point constraints give up on the first that does not; the arguments' own active
/// words are not looked at).
fn all_activated(m: &mut Mem, node: Addr, time: f32) -> bool {
    let mut i = 0;
    while i < nb_args(m, node) {
        let a = arg(m, node, i);
        if !activated(m, a, time) {
            return false;
        }
        i += 1;
    }
    true
}

/// The object of argument `i`, read without regard to the argument count.
fn arg_obj(m: &Mem, node: Addr, i: u32) -> Addr {
    m.ptr(m.ptr(node + updnode::ARGS) + i * updelem::SIZE + updelem::DATA)
}

/// The two-point constraint (127) [soft 0x08221e68 animCns2PtsUpdate]: the node goes to the
/// middle of its first two arguments' objects, 0.5 × (a + b) of the global translations in
/// the register, and its x axis along b − a (`si_tanConstrMapDirection`). The second
/// argument is read whatever the argument count.
pub fn anim_cns_2pts_update<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let mdl = data(m, node);
    if mdl == 0 || !all_activated(m, node, time) {
        return;
    }
    let (a, b) = (arg_obj(m, node, 0), arg_obj(m, node, 1));
    if a == 0 || b == 0 {
        return;
    }
    let (pa, pb) = (v3(m, a + model::GTRS), v3(m, b + model::GTRS));
    let half = x(0.5f64);
    let mid = m.local(12);
    let c = [(half * (x(pa[0]) + pb[0])).to_f32(), (half * (x(pa[1]) + pb[1])).to_f32(), (half * (x(pa[2]) + pb[2])).to_f32()];
    set_v3(m, mid, c);
    h.model_set_global_translation(m, mdl, mid, 1, 0);
    m.release(12);
    let (pa, pb) = (v3(m, a + model::GTRS), v3(m, b + model::GTRS));
    let d = [(x(pb[0]) - pa[0]).to_f32(), (x(pb[1]) - pa[1]).to_f32(), (x(pb[2]) - pa[2]).to_f32()];
    h.tan_constr_map_direction(m, mdl, d[0], d[1], d[2]);
}

/// A point through the inverse of a node's global matrix, in the three operand orders the
/// compiler chose for the face update's nine sums (the association decides the rounding).
fn through(inv: &[[f32; 4]; 4], p: [f32; 3], j: usize, order: u8) -> F80 {
    let t = |k: usize| x(p[k]) * inv[k][j];
    match order {
        // ((z·m2 + (y·m1 + x·m0)) + m3
        0 => (t(2) + (t(1) + t(0))) + inv[3][j],
        // ((y·m1 + x·m0) + z·m2) + m3
        1 => ((t(1) + t(0)) + t(2)) + inv[3][j],
        // m3 + (m2·z + (y·m1 + x·m0))
        _ => x(inv[3][j]) + (x(inv[2][j]) * p[2] + (t(1) + t(0))),
    }
}

/// A face follows the three points of its constraint [soft 0x082217c0 animCns3PtsFaceUpdate]:
/// the three world positions are brought into the node's space (the inverse of its global
/// matrix, +0x1c4; a matrix without inverse leaves the original with an uninitialised one,
/// here zeros) and become the face's outline.
///
/// - A face kept as a spline (the 16-bit word at +0x84 of the definition set; the spline is
///   at +4) must have three points: each point's three vectors (+0, +0xc, +0x18 of its
///   0x30 bytes) all get the position; then `SI_splineUpdateMatrix` and `SI_splineTouch`.
/// - A face kept as a NURBS curve (+0x80) with more points than its first word:
///   `SI_nurbsCrvMovePoint(curve, 0 | 1 | 2, x, y, z)`. The first point's coordinates go
///   over as doubles of the unrounded sums, the other two after a rounding to `float`.
///   Then the knots are reparametrised for a curve of type 3 or 4 (+0x24).
///
/// Then `SI_modelTouchGeometry(model)`.
pub fn anim_cns_3pts_face_update<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, p1: [f32; 3], p2: [f32; 3],
                                                          p3: [f32; 3]) {
    let def = m.ptr(mdl + model::DEFINITION);
    let inverse = |m: &Mem| algebra::matrix_inverse(&m.matrix(mdl + model::GMAT)).unwrap_or([[0.0; 4]; 4]);
    if m.i16(def + face::IS_SPLINE) != 0 {
        if m.i16(def + face::SPLINE + spline::NB_POINTS) != 3 {
            return;
        }
        let inv = inverse(m);
        let q1 = [through(&inv, p1, 0, 0).to_f32(), through(&inv, p1, 1, 0).to_f32(), through(&inv, p1, 2, 0).to_f32()];
        let q2 = [through(&inv, p2, 0, 1).to_f32(), through(&inv, p2, 1, 0).to_f32(), through(&inv, p2, 2, 1).to_f32()];
        let q3 = [through(&inv, p3, 0, 1).to_f32(), through(&inv, p3, 1, 1).to_f32(), through(&inv, p3, 2, 2).to_f32()];
        for (i, q) in [q1, q2, q3].iter().enumerate() {
            for part in [splpoint::POS, 0, 0x18] {
                let pts = m.ptr(def + face::SPLINE + spline::POINTS);
                set_v3(m, pts + i as u32 * splpoint::SIZE + part, *q);
            }
        }
        h.spline_update_matrix(m, def + face::SPLINE);
        h.spline_touch(m, def + face::SPLINE);
    } else {
        let crv = m.ptr(def + face::NURBS);
        if m.i32(crv + nurbs::NB_POINTS) <= m.i32(crv) {
            return;
        }
        let inv = inverse(m);
        let (ax, ay, az) = (through(&inv, p1, 0, 0), through(&inv, p1, 1, 0), through(&inv, p1, 2, 0));
        let q2 = [through(&inv, p2, 0, 1).to_f32(), through(&inv, p2, 1, 0).to_f32(), through(&inv, p2, 2, 1).to_f32()];
        let q3 = [through(&inv, p3, 0, 1).to_f32(), through(&inv, p3, 1, 1).to_f32(), through(&inv, p3, 2, 0).to_f32()];
        let crv = m.ptr(def + face::NURBS);
        h.nurbs_crv_move_point(m, crv, 0, ax.to_f64(), ay.to_f64(), az.to_f64());
        let crv = m.ptr(def + face::NURBS);
        h.nurbs_crv_move_point(m, crv, 1, x(q2[0]).to_f64(), x(q2[1]).to_f64(), x(q2[2]).to_f64());
        let crv = m.ptr(def + face::NURBS);
        h.nurbs_crv_move_point(m, crv, 2, x(q3[0]).to_f64(), x(q3[1]).to_f64(), x(q3[2]).to_f64());
        let crv = m.ptr(def + face::NURBS);
        if (m.i32(crv + nurbs::KIND).wrapping_sub(3) as u32) <= 1 && m.i32(crv + nurbs::NB_POINTS) >= m.i32(crv) {
            h.knots_crv_reparametrization(m, crv);
        }
    }
    h.model_touch_geometry(m, mdl);
}

/// The three-point constraint (119) [soft 0x08221c88 animCns3PtsUpdate]: the node goes to
/// the centre of its first three arguments' objects, 0.333333 × ((a + b) + c) in the
/// register (the constant has six digits), and is turned by `si_constr3PtsMapDirection`
/// with the vector from the centre to the first object and the triangle's normal
/// (`SI_triangleNormal`). A face (kind 2) then takes the three points as its outline
/// ([`anim_cns_3pts_face_update`]). Arguments two and three are read whatever the count.
pub fn anim_cns_3pts_update<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let mdl = data(m, node);
    if mdl == 0 || !all_activated(m, node, time) {
        return;
    }
    let (a, b, c) = (arg_obj(m, node, 0), arg_obj(m, node, 1), arg_obj(m, node, 2));
    if a == 0 || b == 0 || c == 0 {
        return;
    }
    let (pa, pb, pc) = (v3(m, a + model::GTRS), v3(m, b + model::GTRS), v3(m, c + model::GTRS));
    let third = x(THIRD);
    let mid = m.local(12);
    let ctr: [f32; 3] = core::array::from_fn(|k| (third * ((x(pa[k]) + pb[k]) + pc[k])).to_f32());
    set_v3(m, mid, ctr);
    h.model_set_global_translation(m, mdl, mid, 1, 0);
    let ctr = v3(m, mid);
    m.release(12);
    let pa = v3(m, a + model::GTRS);
    let d: [f32; 3] = core::array::from_fn(|k| (x(pa[k]) - ctr[k]).to_f32());
    let (n, _) = algebra::triangle_normal(&m.vector(a + model::GTRS), &m.vector(b + model::GTRS), &m.vector(c + model::GTRS));
    h.constr3pts_map_direction(m, mdl, d[0], d[1], d[2], n.x, n.y, n.z);
    if m.i32(mdl + model::KIND) == 2 {
        let (pa, pb, pc) = (v3(m, a + model::GTRS), v3(m, b + model::GTRS), v3(m, c + model::GTRS));
        anim_cns_3pts_face_update(m, h, mdl, pa, pb, pc);
    }
}

/// A node held on the centre of a cluster of another model (126)
/// [soft 0x0822303c animModelObjPntCnsUpdate]: among the arguments, the last MODELS one is
/// the model and the last CLUSTERS one the cluster; a MODELS argument that is inactive or
/// fails its activation curve ends the search. With both, the centre
/// ([`constr_avg_pnt_location`]) becomes the node's global translation, or the
/// inverse-kinematics goal of a joint or effector.
pub fn anim_model_obj_pnt_cns_update<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    let (mut owner, mut k) = (0, 0);
    let mut i = 0;
    while i < nb_args(m, node) {
        let a = arg(m, node, i);
        let chap = m.i16(a + updelem::CHAPID) as i32;
        if chap == glob::chap(m, cid::MODELS) {
            if m.i16(a + updelem::ACTIVE) == 0 || !activated(m, a, time) {
                break;
            }
            owner = m.ptr(a + updelem::DATA);
        } else if chap == glob::chap(m, cid::CLUSTERS) {
            k = m.ptr(a + updelem::DATA);
        }
        i += 1;
    }
    if owner == 0 || k == 0 {
        return;
    }
    let p = m.local(12);
    constr_avg_pnt_location(m, h, owner, k, p);
    if ik_node(m, mdl) {
        let v = v3(m, p);
        h.inv_kin_set_goal(m, mdl, v[0], v[1], v[2]);
    } else {
        h.model_set_global_translation(m, mdl, p, 1, 0);
    }
    m.release(12);
}

/// The points of a cluster follow the node that is the cluster's centre
/// [soft 0x0822315c cnsClusterCtrSetPtsLocation].
///
/// `mat` is how the centre moved, in the centre's own space (the centre's global matrix ×
/// the model's inverse × the relation's rest matrix, built by the caller). For every
/// member: the vertex as the element has it with geometry 0 (`SI_elemGetVertex`; a vertex
/// it refuses is skipped) goes to world space through the global matrix of the node the
/// model's geometry lives in (+0x44), into the centre's space (inverse of **its** such
/// matrix), through `mat`, back to world space and back into the model's. The difference
/// to the vertex, divided by the number of clusters the vertex belongs to
/// (`SI_klusterCountVertexRef`) when that is two or more, is added to the vertex of
/// geometry 2 and stored (`SI_elemSetVertex`). A patch then has its surface recomputed;
/// the post info becomes 4.
pub fn cns_cluster_ctr_set_pts_location<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, mdl: Addr, k: Addr,
                                                                 ctr: Addr, mat: Addr, post: Addr) {
    let e = node_elem_raw(m, node);
    let chap = m.i16(node + updnode::CHAPID) as i32;
    let v = m.local(12);
    let flag = m.local(4);
    let nv = m.local(12);
    m.set_u16(flag, 0);
    let mut i = 0;
    while i < m.i32(k + kluster::NB) {
        let idx = h.kluster_get_true_index(m, mdl, k, i);
        let refs = h.kluster_count_vertex_ref(m, mdl, idx, 1);
        let d = glob::cur_dict(m);
        if h.elem_get_vertex(m, d, e, chap, 0, 0, idx, v, flag) != 0 {
            let p = v3(m, v);
            let g = m.matrix(m.ptr(mdl + model::LATDEP) + model::GMAT);
            let w: [f32; 3] =
                core::array::from_fn(|j| (((x(p[1]) * g[1][j] + x(p[0]) * g[0][j]) + x(p[2]) * g[2][j]) + g[3][j]).to_f32());
            let inv = algebra::matrix_inverse(&m.matrix(m.ptr(ctr + model::LATDEP) + model::GMAT)).unwrap_or([[0.0; 4]; 4]);
            // into the centre's space: kept in the register
            let t1 = (x(w[2]) * inv[2][0] + (x(w[1]) * inv[1][0] + x(w[0]) * inv[0][0])) + inv[3][0];
            let t2 = (x(w[2]) * inv[2][2] + (x(w[1]) * inv[1][2] + x(w[0]) * inv[0][2])) + inv[3][2];
            let t3 = ((x(w[1]) * inv[1][1] + x(w[0]) * inv[0][1]) + x(w[2]) * inv[2][1]) + inv[3][1];
            let e4 = |r: u32, c: u32| m.f32(mat + 16 * r + 4 * c);
            let t4 = ((t1 * e4(0, 0) + t3 * e4(1, 0)) + t2 * e4(2, 0)) + e4(3, 0);
            let t5 = ((t1 * e4(0, 1) + t3 * e4(1, 1)) + t2 * e4(2, 1)) + e4(3, 1);
            let t6 = (t2 * e4(2, 2) + (t3 * e4(1, 2) + t1 * e4(0, 2))) + e4(3, 2);
            let g = m.matrix(m.ptr(ctr + model::LATDEP) + model::GMAT);
            let w = [
                (((t4 * g[0][0] + t5 * g[1][0]) + t6 * g[2][0]) + g[3][0]).to_f32(),
                (((t5 * g[1][1] + t4 * g[0][1]) + t6 * g[2][1]) + g[3][1]).to_f32(),
                (((t5 * g[1][2] + t4 * g[0][2]) + t6 * g[2][2]) + g[3][2]).to_f32(),
            ];
            let inv = algebra::matrix_inverse(&m.matrix(m.ptr(mdl + model::LATDEP) + model::GMAT)).unwrap_or([[0.0; 4]; 4]);
            let t7 = (x(w[2]) * inv[2][0] + (x(w[1]) * inv[1][0] + x(w[0]) * inv[0][0])) + inv[3][0];
            let t10 = (x(w[2]) * inv[2][2] + (x(w[1]) * inv[1][2] + x(w[0]) * inv[0][2])) + inv[3][2];
            let t9 = (x(w[2]) * inv[2][1] + (x(w[1]) * inv[1][1] + x(w[0]) * inv[0][1])) + inv[3][1];
            let (dx, dz, dy) = (t7 - p[0], t10 - p[2], t9 - p[1]);
            let delta = if refs > 1 {
                let r = F80::ONE / F80::from_i32(refs);
                [(dx * r).to_f32(), (r * dy).to_f32(), (dz * r).to_f32()]
            } else {
                [dx.to_f32(), dy.to_f32(), dz.to_f32()]
            };
            let d = glob::cur_dict(m);
            h.elem_get_vertex(m, d, e, chap, 2, 0, idx, v, flag);
            let p = v3(m, v);
            set_v3(m, nv, [(x(p[0]) + delta[0]).to_f32(), (x(p[1]) + delta[1]).to_f32(), (x(p[2]) + delta[2]).to_f32()]);
            let d = glob::cur_dict(m);
            h.elem_set_vertex(m, d, e, chap, 2, 0, idx, nv);
        }
        i += 1;
    }
    m.release(12);
    m.release(4);
    m.release(12);
    if m.i32(mdl + model::KIND) == 1 {
        let patch = m.ptr(mdl + model::DEFINITION);
        h.patch_compute_surface(m, patch, 0);
    }
    if post != 0 {
        m.set_i32(post, 4);
    }
}

/// The points of a cluster are put on an object [soft 0x08223618 cnsFPntObjSetPtsLocation]:
/// every member of the cluster gets the position `p` (already in the model's space).
///
/// By the model's kind: a NURBS curve (9) `SI_nurbsCrvMovePoint(curve, member's number, p)`
/// and a reparametrisation of the knots for a curve of type 3 or 4 with at least as many
/// points as its first word; a NURBS surface (10) `SI_nurbsSrfMovePoint` with the point's
/// own weight, and likewise; anything else the vertex itself (`SI_modelGetVertexPtr` of the
/// member's true index). The member numbers of the NURBS kinds are the cluster's own array
/// (+0xc), not the true indices. A patch then has its surface recomputed; the post info
/// becomes 2. Nothing without the definition `SI_modelGetData(model, 2, 0)`.
pub fn cns_fpnt_obj_set_pts_location<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, k: Addr, p: [f32; 3],
                                                              post: Addr) {
    let def = h.model_get_data(m, mdl, 2, 0);
    if def == 0 {
        return;
    }
    let flag = m.local(4);
    let mut i = 0;
    while i < m.i32(k + kluster::NB) {
        let member = |m: &Mem| m.i32(m.ptr(k + kluster::MEMBERS) + 4 * i as u32);
        match m.i32(mdl + model::KIND) {
            9 => {
                let n = member(m);
                h.nurbs_crv_move_point(m, def, n, x(p[0]).to_f64(), x(p[1]).to_f64(), x(p[2]).to_f64());
                if (m.i32(def + nurbs::KIND).wrapping_sub(3) as u32) <= 1 && m.i32(def + nurbs::NB_POINTS) >= m.i32(def) {
                    h.knots_crv_reparametrization(m, def);
                }
            }
            10 => {
                let n = member(m);
                let w = m.f64(m.ptr(def + nurbs::SRF_POINTS).wrapping_add((n as u32).wrapping_mul(nurbs::POINT_SIZE)) + nurbs::POINT_W);
                h.nurbs_srf_move_point(m, def, n, x(p[0]).to_f64(), x(p[1]).to_f64(), x(p[2]).to_f64(), si_x87::fmov64(w));
                if (m.i32(def + nurbs::SRF_KIND).wrapping_sub(3) as u32) <= 1 {
                    h.knots_srf_reparametrization(m, def);
                }
            }
            kind => {
                let idx = h.kluster_get_true_index(m, mdl, k, i);
                let at = h.model_get_vertex_ptr(m, def, kind, idx, flag);
                if at != 0 {
                    set_v3(m, at, [fmov(p[0]), fmov(p[1]), fmov(p[2])]);
                }
            }
        }
        i += 1;
    }
    m.release(4);
    if m.i32(mdl + model::KIND) == 1 {
        let patch = m.ptr(mdl + model::DEFINITION);
        h.patch_compute_surface(m, patch, 0);
    }
    if post != 0 {
        m.set_i32(post, 2);
    }
}

/// Clusters constrained to objects (246), and clusters that follow their centre
/// [soft 0x08223788 SI_animMdlClusterCnsUpdate].
///
/// The node's model must have definitions (count +0x34 and array +0x38). With `centre` the
/// model gets a working definition first (`SI_modelCheckCreateDefinition`). With
/// `plain_only` a model that is deformed (a lattice, a fitting, or geometry living in
/// another node's space) is left alone. Then, for every MODELS argument that is active
/// (one that fails its activation curve **ends the routine**): it is paired with the next
/// CLUSTERS argument not yet used, searching from the argument after the last cluster
/// taken (no cluster left ends the routine too). For a pair with an object and a cluster
/// whose type (+0x10) is not 2:
///
/// - the object's global translation through the inverse of the model's global matrix;
/// - without `centre`: [`cns_fpnt_obj_set_pts_location`] of that point;
/// - with `centre`: object's global matrix × the model's inverse, × the relation's data
///   (a matrix) when there is one, and [`cns_cluster_ctr_set_pts_location`]. Without
///   relation data the original hands on a matrix it never set; here zeros.
pub fn anim_mdl_cluster_cns_update<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, plain_only: i16, centre: i16,
                                                            post: Addr, time: f32) {
    let mdl = data(m, node);
    if mdl == 0 || m.i16(mdl + model::NB_DEFS) == 0 || m.ptr(mdl + model::DEFS) == 0 {
        return;
    }
    if centre != 0 {
        h.model_check_create_definition(m, mdl);
    }
    if plain_only != 0
       && (m.ptr(mdl + model::NODE_LATTICES) != 0
           || m.ptr(mdl + model::BRANCH_LATTICES) != 0
           || m.ptr(mdl + model::SPL_FITTING) != 0
           || m.ptr(mdl + model::SPL_FITTING_BRANCH) != 0
           || m.ptr(mdl + model::PTCH_FITTING) != 0
           || m.ptr(mdl + model::PTCH_FITTING_BRANCH) != 0
           || m.ptr(mdl + model::LATDEP) != mdl)
    {
        return;
    }
    let inv = algebra::matrix_inverse(&m.matrix(mdl + model::GMAT)).unwrap_or([[0.0; 4]; 4]);
    let models = glob::chap(m, cid::MODELS);
    let clusters = glob::chap(m, cid::CLUSTERS);
    let mat1 = m.local(64);
    let mat2 = m.local(64);
    let mut next = 0;
    let mut i = 0;
    'args: while i < nb_args(m, node) {
        let a = arg(m, node, i);
        i += 1;
        if m.i16(a + updelem::CHAPID) as i32 != models || m.i16(a + updelem::ACTIVE) == 0 {
            continue;
        }
        let obj = m.ptr(a + updelem::DATA);
        if !activated(m, a, time) {
            break 'args;
        }
        let n = nb_args(m, node);
        let mut j = next;
        while j < n && m.i16(arg(m, node, j) + updelem::CHAPID) as i32 != clusters {
            j += 1;
        }
        if j == n {
            break 'args;
        }
        let k = m.ptr(arg(m, node, j) + updelem::DATA);
        if obj != 0 && k != 0 && m.i32(k + kluster::KIND) != 2 {
            let g = v3(m, obj + model::GTRS);
            let px = ((x(g[1]) * inv[1][0] + x(g[0]) * inv[0][0]) + x(g[2]) * inv[2][0]) + inv[3][0];
            let pz = (x(g[2]) * inv[2][2] + (x(g[1]) * inv[1][2] + x(g[0]) * inv[0][2])) + inv[3][2];
            let py = ((x(g[1]) * inv[1][1] + x(g[0]) * inv[0][1]) + x(g[2]) * inv[2][1]) + inv[3][1];
            let p = [px.to_f32(), py.to_f32(), pz.to_f32()];
            if centre != 0 {
                let prod = algebra::matrix_multiply(&m.matrix(obj + model::GMAT), &inv);
                m.set_matrix(mat1, &prod);
                let rest = m.ptr(node + updnode::REL_DATA);
                if rest != 0 {
                    let prod = algebra::matrix_multiply(&m.matrix(mat1), &m.matrix(rest));
                    m.set_matrix(mat2, &prod);
                }
                cns_cluster_ctr_set_pts_location(m, h, node, mdl, k, obj, mat2, post);
            } else {
                cns_fpnt_obj_set_pts_location(m, h, mdl, k, p, post);
            }
        }
        next = j + 1;
    }
    m.release(64);
    m.release(64);
}

/// The trajectory (160) [soft 0x08224b0c animTrajectoryUpdate]: a path whose curve has one
/// point a frame.
///
/// The curve belongs to the first argument's object: a spline (kind 6) or else a NURBS
/// curve. The position on it is `SI_secAsFrame(time)` less the first frame (the first word
/// of the relation's data, or `SI_frameStart` without data), in the register. Within
/// 0.0001 of a whole number, that point of the curve is taken as it is (the number limited
/// to the curve's points; a NURBS point divided by its weight: 1 / w in the register times
/// each coordinate); otherwise the curve is evaluated there (`SI_splineValue`,
/// `SI_nurbsCrvValue`). The point becomes a local translation
/// ([`transfo::model_compute_path_translation`]) and goes to the inverse-kinematics goal (a
/// joint with algorithm 1, or algorithm 3) or to the local translation; the translation
/// bit of the update flags is raised. The first argument is not tested for an object.
pub fn anim_trajectory_update<H: ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    let path = arg_obj(m, node, 0);
    let geom = m.ptr(path + model::DEFINITION);
    let frame = fcv::f80(si_fcurve::eval::sec_as_frame(time, &fcv::env(m)));
    let rec = m.ptr(node + updnode::REL_DATA);
    let first = if rec != 0 { m.i32(rec) } else { m.i32(glob::SI_FRAME_START) };
    let t = frame - F80::from_i32(first);
    let pt = m.local(12);
    let n = nearest(t);
    let whole = (t - F80::from_i32(n)).abs().lt(x(EPS));
    if m.i32(path + model::KIND) == 6 {
        if whole {
            let nb = m.u16(geom + spline::NB_POINTS) as i32;
            let i = if n < 0 { 0 } else if n >= nb { nb - 1 } else { n };
            let p = m.ptr(geom + spline::POINTS).wrapping_add((i as u32).wrapping_mul(splpoint::SIZE)) + splpoint::POS;
            copy3(m, pt, p);
        } else {
            h.spline_value(m, geom, t.to_f32(), pt);
        }
    } else if whole {
        let nb = m.i32(geom + nurbs::NB_POINTS);
        let i = if n < 0 { 0 } else if n >= nb { nb.wrapping_sub(1) } else { n };
        let p = m.ptr(geom + nurbs::POINTS).wrapping_add((i as u32).wrapping_mul(nurbs::POINT_SIZE));
        let w = F80::ONE / m.f64(p + nurbs::POINT_W);
        let v = [(x(m.f64(p)) * w).to_f32(), (x(m.f64(p + 8)) * w).to_f32(), (w * m.f64(p + 16)).to_f32()];
        set_v3(m, pt, v);
    } else {
        let ret = m.local(24);
        h.nurbs_crv_value(m, ret, geom, t.to_f64(), 0);
        for k in 0..3 {
            let v = x(m.f64(ret + 8 * k)).to_f32();
            m.set_f32(pt + 4 * k, v);
        }
        m.release(24);
    }
    transfo::model_compute_path_translation(m, mdl, path, pt);
    let p = v3(m, pt);
    m.release(12);
    let algo = m.i32(mdl + model::ALGO_TYPE);
    if (m.i32(mdl + model::KIND) == 5 && algo == 1) || algo == 3 {
        h.inv_kin_set_goal(m, mdl, p[0], p[1], p[2]);
    } else {
        set_v3(m, mdl + model::TRANS, p);
    }
    m.or_u8(mdl + model::UPDATE_FLAGS, updflag::TRANSLATION);
    m.or_u8(mdl + model::UPDATE_FLAGS2, updflag::TRANSLATION);
}
