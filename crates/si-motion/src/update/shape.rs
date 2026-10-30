// SPDX-License-Identifier: MIT
//! Shape animation and the deformation parameters of `motionUpdate.c`: which of a model's
//! shapes are blended at a time, and the curves of the deformations by spline and by patch.
//!
//! The blending itself is `shape.c`'s (one interpolator per kind of geometry and per mode),
//! reached through host methods: what is decided here is **which definitions** go in and
//! with which fraction.

use super::geom::GeomHost;
use super::params::DEG2RAD;
use super::{arg, data, nb_args, relid};
use crate::ed;
use crate::fcv;
use crate::glob::{self, cid};
use si_mem::{Addr, Mem};
use crate::off::{model, updelem, updnode};
use si_x87::{x, Round, F80};

/// Three words each: a count, a pointer and a second pointer that `SI_getExprFcurveUsed`
/// fills; statics of the three routines that ask it (`nbwshp.366`, `wfcv.367`,
/// `wfcvSubid.368` and the two sets `nbExprFcrv`, `dummy1`, `dummy2`).
pub const EXPR_STATICS_SHAPE: Addr = 0x0869_7724;
pub const EXPR_STATICS_SPLINE_DEF: Addr = 0x0869_7824;
pub const EXPR_STATICS_PATCH_DEF: Addr = 0x0869_7830;

/// The texts of `animShapeUpdate`, at their addresses in the binary's data.
pub const MSG_UNKNOWN_OBJECT: Addr = 0x0869_77c0;
pub const MSG_UNKNOWN_INTERP_CRV: Addr = 0x0869_7740;
pub const MSG_UNKNOWN_INTERP_SRF: Addr = 0x0869_7780;

/// `SI_SplFitting` and `SI_PtchFitting` (finding 0017, section 10).
pub mod fit {
    pub const SPL_SCALING: u32 = 0x0c;
    pub const SPL_ROLL: u32 = 0x18;
    pub const SPL_TRANSLATION: u32 = 0x1c;
    pub const PTCH_SCALING: u32 = 0x08;
    pub const PTCH_ROTATION: u32 = 0x14;
    pub const PTCH_TRANSLATION: u32 = 0x20;
}

crate::host_trait! {
    /// What the shape and deformation updates call outside this package.
    pub trait ShapeHost: GeomHost {
        /// `SI_edGetElemFromShape(model, &index)` (edtools.c): the model's SHAPES element.
        fn ed_get_elem_from_shape(mdl: Addr, index: Addr) -> Addr;
        /// `SI_getExprFcurveUsed(elem, &count, list, 0, &second list, curve type)`
        /// (exp_fct.c): how many expressions drive that parameter of the element; returns
        /// a list the caller frees, and may leave a second one.
        fn get_expr_fcurve_used(e: Addr, count: Addr, list: Addr, zero: i32, second: Addr, kind: i32) -> Addr;
        /// `SI_modelTouchDefinition(model)` (model.c).
        fn model_touch_definition(mdl: Addr);
        /// `SI_cusTransferToDefinition(model, 0)` (custom effects).
        fn cus_transfer_to_definition(mdl: Addr, flag: i32);
        /// `SI_fittingUpdateMatrix(fitting)` (fitting.c).
        fn fitting_update_matrix(fitting: Addr);
        /// `SI_pfittingUpdateMatrix(fitting)` (fitting.c).
        fn pfitting_update_matrix(fitting: Addr);

        // ---- shape.c: (from, to, fraction, result), (before, from, to, after, fraction,
        // result), (model, node, time)
        fn patch_interpolate(a: Addr, b: Addr, t: f32, out: Addr);
        fn patch_cardinal_interpolate(p0: Addr, p1: Addr, p2: Addr, p3: Addr, t: f32, out: Addr);
        fn patch_weight_interpolate(mdl: Addr, node: Addr, time: f32);
        fn face_interpolate(a: Addr, b: Addr, t: f32, out: Addr);
        fn face_cardinal_interpolate(p0: Addr, p1: Addr, p2: Addr, p3: Addr, t: f32, out: Addr);
        fn face_weight_interpolate(mdl: Addr, node: Addr, time: f32);
        fn mesh_interpolate(a: Addr, b: Addr, t: f32, out: Addr);
        fn mesh_cardinal_interpolate(p0: Addr, p1: Addr, p2: Addr, p3: Addr, t: f32, out: Addr);
        fn mesh_weight_interpolate(mdl: Addr, node: Addr, time: f32);
        fn spline_interpolate(a: Addr, b: Addr, t: f32, out: Addr);
        fn spline_cardinal_interpolate(p0: Addr, p1: Addr, p2: Addr, p3: Addr, t: f32, out: Addr);
        fn spline_weight_interpolate(mdl: Addr, node: Addr, time: f32);
        fn nurbs_crv_interpolate(a: Addr, b: Addr, t: f32, out: Addr);
        fn nurbs_crv_cardinal_interpolate(p0: Addr, p1: Addr, p2: Addr, p3: Addr, t: f32, out: Addr);
        fn nurbs_crv_weight_interpolate(mdl: Addr, node: Addr, time: f32);
        fn nurbs_srf_interpolate(a: Addr, b: Addr, t: f32, out: Addr);
        fn nurbs_srf_cardinal_interpolate(p0: Addr, p1: Addr, p2: Addr, p3: Addr, t: f32, out: Addr);
        fn nurbs_srf_weight_interpolate(mdl: Addr, node: Addr, time: f32);

        // ---- cluster.c and what the cluster update touches afterwards
        /// `SI_modelKlusterResetCluInd(model)` (klusters.c).
        fn model_kluster_reset_clu_ind(mdl: Addr);
        /// `si_clusterMeshInterpolate(original, vertex, key clusters, mode, time, 1, &point,
        /// node)`: non-zero when the vertex is animated; the point through the pointer.
        fn cluster_mesh_interpolate(org: Addr, index: i32, keys: Addr, mode: i32, time: f32, one: i32, out: Addr, node: Addr) -> i32;
        /// `si_clusterPatchInterpolate`, the same for a patch's control points.
        fn cluster_patch_interpolate(org: Addr, index: i32, keys: Addr, mode: i32, time: f32, one: i32, out: Addr, node: Addr) -> i32;
        /// `si_clusterSplineInterpolate(original, point, key clusters, mode, time, 1, &a, &b,
        /// &c, node)`: the three vectors of a spline point.
        fn cluster_spline_interpolate(org: Addr, index: i32, keys: Addr, mode: i32, time: f32, one: i32, o1: Addr, o2: Addr, o3: Addr,
                                      node: Addr) -> i32;
        /// `si_clusterFaceInterpolate(original spline, curve number, point, …)`: curve −1 is
        /// the outline, 0 … the holes.
        fn cluster_face_interpolate(org: Addr, curve: i32, index: i32, keys: Addr, mode: i32, time: f32, one: i32, o1: Addr, o2: Addr,
                                    o3: Addr, node: Addr) -> i32;
        /// `si_clusterNurbsFaceInterpolate`, the same for a face of NURBS curves: three
        /// four-component vectors.
        fn cluster_nurbs_face_interpolate(org: Addr, curve: i32, index: i32, keys: Addr, mode: i32, time: f32, one: i32, o1: Addr,
                                          o2: Addr, o3: Addr, node: Addr) -> i32;
        /// `si_clusterNurbsCrvInterpolate(original, point, …, &(x, y, z, w), node)`.
        fn cluster_nurbs_crv_interpolate(org: Addr, index: i32, keys: Addr, mode: i32, time: f32, one: i32, out: Addr, node: Addr) -> i32;
        /// `si_clusterNurbsSrfInterpolate`, the same for a surface.
        fn cluster_nurbs_srf_interpolate(org: Addr, index: i32, keys: Addr, mode: i32, time: f32, one: i32, out: Addr, node: Addr) -> i32;
        /// `SI_meshComputeHardEdge(mesh, -1)` (mesh.c).
        fn mesh_compute_hard_edge(mesh: Addr, which: i32);
        /// `SI_meshComputesVertexNormal(mesh, -1)` (mesh.c).
        fn mesh_computes_vertex_normal(mesh: Addr, which: i32);
    }
}

/// The cluster record of a model (+0x344) and what `animClustersUpdate` reads of the
/// geometries (**new**).
pub mod clu {
    /// Model + 0x344: the record; + 0x348: the list of key clusters.
    pub const RECORD: u32 = 0x344;
    pub const KEYS: u32 = 0x348;
    /// Of the record: a word that must be set; the 16-bit interpolation mode.
    pub const SET: u32 = 0x04;
    pub const MODE: u32 = 0x08;
    /// `SI_Mesh`: the vertex array (0x14 bytes a vertex, the position first) and count.
    pub const MESH_VERTICES: u32 = 0x00;
    pub const MESH_NB: u32 = 0x14;
    /// `SI_Patch`: the two 16-bit point counts and the points (0x10 bytes each).
    pub const PATCH_NU: u32 = 0x0c;
    pub const PATCH_NV: u32 = 0x0e;
    pub const PATCH_POINTS: u32 = 0x1a4;
    /// `SI_Face`: 16 bits, the number of holes; the holes as splines (0x74 bytes each) or as
    /// NURBS curves (100 bytes each, the outline first).
    pub const FACE_NB_HOLES: u32 = 0x78;
    pub const FACE_HOLES: u32 = 0x7c;
    /// A byte of the NURBS curve (+0x3b) and of the surface (+0x5b) that gets the bits 0x60
    /// when a point moved.
    pub const CRV_DIRTY: u32 = 0x3b;
    pub const SRF_DIRTY: u32 = 0x5b;
    /// `SI_NurbsSrf`: number of control points.
    pub const SRF_NB_POINTS: u32 = 0x48;
}

/// The text of `animClustersUpdate`, at its address in the binary's data.
pub const MSG_CLUSTER: Addr = 0x0869_7800;

/// A curve's value for a parameter an expression may drive: `SI_getExprFcurveUsed` is asked
/// how many expressions drive the parameter `kind` of the element (its two lists are freed
/// at once); with one or more the curve is **not** evaluated and its last value (+0x14) is
/// taken, otherwise `SI_fcurveValue(curve, time, 1)`. `statics`: the routine's three words.
fn value_or_last<H: ShapeHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, statics: Addr, curve: Addr, kind: i32, time: f32) -> F80 {
    let list = m.ptr(statics + 4);
    let r = h.get_expr_fcurve_used(m, e, statics, list, 0, statics + 8, kind);
    m.set_ptr(statics + 4, r);
    if r != 0 {
        m.free(r);
        m.set_ptr(statics + 4, 0);
    }
    let second = m.ptr(statics + 8);
    if second != 0 {
        m.free(second);
        m.set_ptr(statics + 8, 0);
    }
    if m.i32(statics) > 0 {
        x(m.f32(curve + 0x14))
    } else {
        fcv::value(m, curve, time, 1)
    }
}

/// Shape animation of a model (relation 200) [soft 0x08225e34 animShapeUpdate].
///
/// A model keeps its shapes as definitions (count +0x34, array +0x38): entry 0 is the
/// working geometry, the entries from 1 on are the keys. The current-definition word
/// (+0x36) is cleared, and a working pointer (+0x48) that points at one of the keys is put
/// back on entry 0.
///
/// Unless the model blends by weights (mode 2 at +0x2e0), the position among the keys is
/// the shape curve's value (type 8, relation 201 in list 0 of the model's SHAPES element;
/// its last value instead when an expression drives it). With v the value and n = v
/// truncated: the blend is between keys n + 1 and n + 2, each limited to [1, count − 1],
/// with the fraction v − n (in the register, rounded to `float` when passed); the cardinal
/// mode also takes the key before the first (if that is above 1) and the key after the
/// second (if that is below the last). Without the SHAPES element or the curve nothing
/// happens.
///
/// | Mode (+0x2e0) | Call |
/// |---|---|
/// | 0 | `si_<kind>Interpolate(key₁, key₂, fraction, entry 0)` |
/// | 1 | `si_<kind>CardinalInterpolate(key₀, key₁, key₂, key₃, fraction, entry 0)` |
/// | 2 | `si_<kind>WeightInterpolate(model, node, time)` |
///
/// for patches (kind 1), faces (2), meshes (4), splines (6), NURBS curves (9) and surfaces
/// (10). Any other kind: the message "animShapeUpdate: Unknown object type!"; another mode
/// is silently nothing, except for the NURBS kinds ("Unknown shape interpolation type!").
/// Then `SI_modelTouchDefinition(model)` and, for a model under a custom effect (+0x352
/// above 0), `SI_cusTransferToDefinition(model, 0)`.
pub fn anim_shape_update<H: ShapeHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    m.set_u16(mdl + model::CUR_DEF, 0);
    let nb = m.u16(mdl + model::NB_DEFS) as i32;
    if nb != 0 {
        let defs = m.ptr(mdl + model::DEFS);
        let cur = m.ptr(mdl + model::DEFINITION);
        if cur != m.ptr(defs) {
            let mut i = 1;
            while i < nb {
                if cur == m.ptr(defs.wrapping_add(4 * i as u32)) {
                    let first = m.ptr(defs.wrapping_add(4 * m.u16(mdl + model::CUR_DEF) as u32));
                    m.set_ptr(mdl + model::DEFINITION, first);
                    break;
                }
                i += 1;
            }
        }
    }
    let mode = m.i16(mdl + model::SHAPE_INTERP);
    let mut frac = F80::ZERO;
    // the four keys; the original leaves them unset under mode 2, which does not use them
    let (mut k0, mut k1, mut k2, mut k3) = (0i32, 0i32, 0i32, 0i32);
    if mode != 2 {
        let idx = m.local(4);
        let e = h.ed_get_elem_from_shape(m, mdl, idx);
        m.release(4);
        if e == 0 {
            return;
        }
        let curve = ed::find_fcv(m, e, 0, 0xc9);
        if curve == 0 {
            return;
        }
        let last = m.u16(mdl + model::NB_DEFS) as i32 - 1;
        let d = glob::cur_dict(m);
        let en = ed::get_elem(m, d, m.i16(node + updnode::CHAPID) as i32, m.i32(node + updnode::ELEMID));
        let v = value_or_last(m, h, en, EXPR_STATICS_SHAPE, curve, 8, time);
        let n = v.to_i32(Round::Zero);
        let limit = |k: i32| if k <= 0 { 1 } else if k > last { last } else { k };
        k1 = limit(n.wrapping_add(1));
        k2 = limit(n.wrapping_add(2));
        frac = v - F80::from_i32(n);
        k0 = if k1 > 1 { k1 - 1 } else { k1 };
        k3 = if k2 < last { k2 + 1 } else { k2 };
    }
    let key = |m: &Mem, k: i32| m.ptr(m.ptr(mdl + model::DEFS).wrapping_add((k as u32).wrapping_mul(4)));
    let t = frac.to_f32();
    let kind = m.i32(mdl + model::KIND);
    macro_rules! blend {
        ($lin:ident, $card:ident, $weight:ident, $unknown:expr) => {
            match mode {
                0 => {
                    let (a, b, out) = (key(m, k1), key(m, k2), key(m, 0));
                    h.$lin(m, a, b, t, out);
                }
                1 => {
                    let (p0, p1, p2, p3, out) = (key(m, k0), key(m, k1), key(m, k2), key(m, k3), key(m, 0));
                    h.$card(m, p0, p1, p2, p3, t, out);
                }
                2 => h.$weight(m, mdl, node, time),
                _ => {
                    if $unknown != 0 {
                        h.status_display_message(m, $unknown, 0);
                    }
                }
            }
        };
    }
    match kind {
        1 => blend!(patch_interpolate, patch_cardinal_interpolate, patch_weight_interpolate, 0),
        2 => blend!(face_interpolate, face_cardinal_interpolate, face_weight_interpolate, 0),
        4 => blend!(mesh_interpolate, mesh_cardinal_interpolate, mesh_weight_interpolate, 0),
        6 => blend!(spline_interpolate, spline_cardinal_interpolate, spline_weight_interpolate, 0),
        9 => blend!(nurbs_crv_interpolate, nurbs_crv_cardinal_interpolate, nurbs_crv_weight_interpolate, MSG_UNKNOWN_INTERP_CRV),
        10 => blend!(nurbs_srf_interpolate, nurbs_srf_cardinal_interpolate, nurbs_srf_weight_interpolate, MSG_UNKNOWN_INTERP_SRF),
        _ => h.status_display_message(m, MSG_UNKNOWN_OBJECT, 0),
    }
    h.model_touch_definition(m, mdl);
    if m.i16(mdl + model::CUSTOM_COUNT) > 0 {
        h.cus_transfer_to_definition(m, mdl, 0);
    }
}

/// The curve arguments of a deformation step, each with its value: an argument of the
/// ANIMATION chapter that is active and has a curve.
fn def_curves<H: ShapeHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32, statics: Addr,
                                    mut store: impl FnMut(&mut Mem, i32, F80, bool)) {
    let d = glob::cur_dict(m);
    let e = ed::get_elem(m, d, m.i16(node + updnode::CHAPID) as i32, m.i32(node + updnode::ELEMID));
    let mut i = 0;
    while i < nb_args(m, node) {
        let a = arg(m, node, i);
        i += 1;
        if m.i16(a + updelem::CHAPID) as i32 != glob::chap(m, cid::ANIMATION) || m.i16(a + updelem::ACTIVE) == 0 {
            continue;
        }
        let c = m.ptr(a + updelem::DATA);
        if c == 0 {
            continue;
        }
        let kind = fcv::kind(m, c);
        let v = value_or_last(m, h, e, statics, c, kind, time);
        let has_keys = fcv::nb_keys(m, c) > 0;
        store(m, kind, v, has_keys);
    }
}

/// The curves of a deformation by spline (relations 220 node, 221 branch)
/// [soft 0x0822701c animSplineDefUpdate]: the fitting record of the node (+0x60) or of the
/// branch (+0x64); the post info becomes 2 or 1. Curves 107/114, 108/115, 109/116 scaling
/// (+0x0c …; 1 for a curve without keys), 110/117 roll (+0x18, degrees → radians), 111–113 /
/// 118–120 translation (+0x1c …). A curve whose parameter an expression drives gives its
/// last value. Then `SI_fittingUpdateMatrix(record)`.
pub fn anim_spline_def_update<H: ShapeHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32, post: Addr) {
    let mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    let (rec, info) = match relid(m, node) {
        0xdc => (m.ptr(mdl + model::SPL_FITTING), 2),
        0xdd => (m.ptr(mdl + model::SPL_FITTING_BRANCH), 1),
        _ => return,
    };
    if rec == 0 {
        return;
    }
    if post != 0 {
        m.set_i32(post, info);
    }
    def_curves(m, h, node, time, EXPR_STATICS_SPLINE_DEF, |m, kind, v, has_keys| {
        let k = kind.wrapping_sub(0x6b);
        if (k as u32) > 0xd {
            return;
        }
        match k % 7 {
            c @ 0..=2 => m.set_f32(rec + fit::SPL_SCALING + 4 * c as u32, if has_keys { v } else { F80::ONE }.to_f32()),
            3 => m.set_f32(rec + fit::SPL_ROLL, (v * DEG2RAD).to_f32()),
            c => m.set_f32(rec + fit::SPL_TRANSLATION + 4 * (c - 4) as u32, v.to_f32()),
        }
    });
    h.fitting_update_matrix(m, rec);
}

/// The curves of a deformation by patch (relations 230 node, 231 branch)
/// [soft 0x08227208 animPatchDefUpdate]: the fitting record of the node (+0x2d8) or of the
/// branch (+0x2dc); the post info becomes 2 or 1. Curves 121–123 / 130–132 scaling (+0x08 …;
/// 1 for a curve without keys), 124–126 / 133–135 rotation (+0x14 …, degrees → radians),
/// 127–129 / 136–138 translation (+0x20 …). Then `SI_pfittingUpdateMatrix(record)`.
pub fn anim_patch_def_update<H: ShapeHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32, post: Addr) {
    let mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    let (rec, info) = match relid(m, node) {
        0xe6 => (m.ptr(mdl + model::PTCH_FITTING), 2),
        0xe7 => (m.ptr(mdl + model::PTCH_FITTING_BRANCH), 1),
        _ => return,
    };
    if rec == 0 {
        return;
    }
    if post != 0 {
        m.set_i32(post, info);
    }
    def_curves(m, h, node, time, EXPR_STATICS_PATCH_DEF, |m, kind, v, has_keys| {
        let k = kind.wrapping_sub(0x79);
        if (k as u32) > 0x11 {
            return;
        }
        let c = (k % 9) as u32;
        match c {
            0..=2 => m.set_f32(rec + fit::PTCH_SCALING + 4 * c, if has_keys { v } else { F80::ONE }.to_f32()),
            3..=5 => m.set_f32(rec + fit::PTCH_ROTATION + 4 * (c - 3), (v * DEG2RAD).to_f32()),
            _ => m.set_f32(rec + fit::PTCH_TRANSLATION + 4 * (c - 6), v.to_f32()),
        }
    });
    h.pfitting_update_matrix(m, rec);
}

/// Nine floats (the three vectors of a spline point) from three locals, through the unit.
fn store_point(m: &mut Mem, p: Addr, v: [Addr; 3]) {
    for (i, from) in v.iter().enumerate() {
        for k in 0..3 {
            let w = si_x87::fmov(m.f32(from + 4 * k));
            m.set_f32(p + 12 * i as u32 + 4 * k, w);
        }
    }
}

/// Cluster animation of a model (relation 700) [soft 0x08226444 animClustersUpdate]: every
/// point of the working geometry (entry 0 of the definitions, +0x38) is asked of
/// `cluster.c` ("is this point in an animated cluster, and where is it at this time?",
/// one interpolator per kind of geometry, given the original definition at +0x3c, the key
/// clusters at +0x348 and the record's mode) and moved there.
///
/// Nothing without the cluster record (+0x344), its word +4, the key clusters, or, in
/// mode 1, the original definition. `SI_modelKlusterResetCluInd(model)` first. Then by
/// kind:
///
/// | Kind | Points | Afterwards, if any moved |
/// |---|---|---|
/// | 4 mesh | the vertices' positions | polygon normals, hard edges, vertex normals recomputed |
/// | 1 patch | the control points (0x10 bytes apart) | `SI_patchComputeSurface` |
/// | 6 spline | the three vectors of each point | `SI_splineTouch` |
/// | 2 face, splines | the outline (curve −1) and each hole, point by point | `SI_splineTouch` of each curve from the first that moved on |
/// | 2 face, NURBS | `SI_nurbsCrvMovePoint` per point of each curve, and the knots | |
/// | 9 NURBS curve | the point's weight, then `SI_nurbsCrvMovePoint`, and the knots | bits 0x60 of the curve's byte +0x3b |
/// | 10 NURBS surface | `SI_nurbsSrfMovePoint` with the weight, and the knots | bits 0x60 of the byte +0x5b |
///
/// Another kind: the message "Unable to animate that cluster...". If any point moved,
/// `SI_modelTouchDefinition(model)`; for a model under a custom effect,
/// `SI_cusTransferToDefinition(model, 0)` in any case.
pub fn anim_clusters_update<H: ShapeHost + super::constr::ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    use crate::off::{face, nurbs, spline, splpoint};
    let mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    let rec = m.ptr(mdl + clu::RECORD);
    if rec == 0 || m.i32(rec + clu::SET) == 0 || m.ptr(mdl + clu::KEYS) == 0 {
        return;
    }
    let mode = m.i16(rec + clu::MODE) as i32;
    if mode == 1 && m.ptr(mdl + model::ORG_DEF) == 0 {
        return;
    }
    h.model_kluster_reset_clu_ind(m, mdl);
    let def = m.ptr(m.ptr(mdl + model::DEFS));
    let org = m.ptr(mdl + model::ORG_DEF);
    let keys = |m: &Mem| m.ptr(mdl + clu::KEYS);
    // three vectors and three four-component vectors (0, 0, 0, 1), as the original's frame
    let v: [Addr; 3] = [m.local(12), m.local(12), m.local(12)];
    let hv: [Addr; 3] = [m.local(16), m.local(16), m.local(16)];
    for a in hv {
        m.set_f32(a + 12, 1.0);
    }
    let mut moved = 0u32;
    let knots = |m: &mut Mem, h: &mut H, crv: Addr| {
        if (m.i32(crv + nurbs::KIND).wrapping_sub(3) as u32) <= 1 && m.i32(crv + nurbs::NB_POINTS) >= m.i32(crv) {
            h.knots_crv_reparametrization(m, crv);
        }
    };
    let move_crv = |m: &mut Mem, h: &mut H, crv: Addr, j: i32, from: Addr| {
        let (px, py, pz) = (x(m.f32(from)).to_f64(), x(m.f32(from + 4)).to_f64(), x(m.f32(from + 8)).to_f64());
        h.nurbs_crv_move_point(m, crv, j, px, py, pz);
    };
    match m.i32(mdl + model::KIND) {
        4 => {
            let mut p = m.ptr(def + clu::MESH_VERTICES);
            let mut i = 0;
            while i < m.i32(def + clu::MESH_NB) {
                let k = keys(m);
                let r = h.cluster_mesh_interpolate(m, org, i, k, mode, time, 1, v[0], node) as u32;
                moved |= r;
                if r != 0 {
                    for q in 0..3 {
                        let w = si_x87::fmov(m.f32(v[0] + 4 * q));
                        m.set_f32(p + 4 * q, w);
                    }
                }
                i += 1;
                p = p.wrapping_add(0x14);
            }
            if moved != 0 {
                h.mesh_computes_polygon_normal(m, def, -1);
                h.mesh_compute_hard_edge(m, def, -1);
                h.mesh_computes_vertex_normal(m, def, -1);
            }
        }
        1 => {
            let mut p = m.ptr(def + clu::PATCH_POINTS);
            let mut i = 0;
            while i < m.u16(def + clu::PATCH_NU) as i32 * m.u16(def + clu::PATCH_NV) as i32 {
                let k = keys(m);
                let r = h.cluster_patch_interpolate(m, org, i, k, mode, time, 1, v[0], node) as u32;
                moved |= r;
                if r != 0 {
                    for q in 0..3 {
                        let w = si_x87::fmov(m.f32(v[0] + 4 * q));
                        m.set_f32(p + 4 * q, w);
                    }
                }
                i += 1;
                p = p.wrapping_add(0x10);
            }
            if moved != 0 {
                h.patch_compute_surface(m, def, 0);
            }
        }
        6 => {
            let mut p = m.ptr(def + spline::POINTS);
            let mut j = 0;
            if m.u16(def + spline::NB_POINTS) != 0 {
                while p != 0 {
                    let k = keys(m);
                    let r = h.cluster_spline_interpolate(m, org, j, k, mode, time, 1, v[0], v[1], v[2], node) as u32;
                    moved |= r;
                    if r != 0 {
                        store_point(m, p, v);
                    }
                    j += 1;
                    p = p.wrapping_add(splpoint::SIZE);
                    if j >= m.u16(def + spline::NB_POINTS) as i32 {
                        break;
                    }
                }
            }
            if moved != 0 {
                h.spline_touch(m, def);
            }
        }
        2 if m.i16(def + face::IS_SPLINE) != 0 => {
            let mut idx = -1i32;
            if m.i16(def + clu::FACE_NB_HOLES) >= 0 {
                loop {
                    let (spl, ospl) = if idx == -1 {
                        (def + face::SPLINE, if org != 0 { org + face::SPLINE } else { 0 })
                    } else {
                        let off = (idx as u32).wrapping_mul(0x74);
                        (m.ptr(def + clu::FACE_HOLES).wrapping_add(off), if org != 0 { m.ptr(org + clu::FACE_HOLES).wrapping_add(off) } else { 0 })
                    };
                    if spl != 0 {
                        let mut p = m.ptr(spl + spline::POINTS);
                        let mut j = 0;
                        if m.u16(spl + spline::NB_POINTS) != 0 {
                            while p != 0 {
                                let k = keys(m);
                                let r = h.cluster_face_interpolate(m, ospl, idx, j, k, mode, time, 1, v[0], v[1], v[2], node) as u32;
                                moved |= r;
                                if r != 0 {
                                    store_point(m, p, v);
                                }
                                j += 1;
                                p = p.wrapping_add(splpoint::SIZE);
                                if j >= m.u16(spl + spline::NB_POINTS) as i32 {
                                    break;
                                }
                            }
                        }
                        if moved != 0 {
                            h.spline_touch(m, spl);
                        }
                    }
                    idx += 1;
                    if idx >= m.i16(def + clu::FACE_NB_HOLES) as i32 {
                        break;
                    }
                }
            }
        }
        2 => {
            let mut idx = 0i32;
            while idx < m.i16(def + clu::FACE_NB_HOLES) as i32 {
                let off = (idx as u32).wrapping_mul(100);
                let crv = m.ptr(def + face::NURBS).wrapping_add(off);
                let ocrv = if org != 0 { m.ptr(org + face::NURBS).wrapping_add(off) } else { 0 };
                // the points are read before the curve is tested for null
                let mut p = m.ptr(crv + nurbs::POINTS);
                if crv != 0 {
                    let mut j = 0;
                    while j < m.i32(crv + nurbs::NB_POINTS) && p != 0 {
                        let k = keys(m);
                        let r = h.cluster_nurbs_face_interpolate(m, ocrv, idx, j, k, mode, time, 1, hv[0], hv[1], hv[2], node) as u32;
                        moved |= r;
                        if r != 0 {
                            move_crv(m, h, crv, j, hv[0]);
                            knots(m, h, crv);
                        }
                        j += 1;
                        p = p.wrapping_add(nurbs::POINT_SIZE);
                    }
                }
                idx += 1;
            }
        }
        9 => {
            let mut i = 0;
            while i < m.i32(def + nurbs::NB_POINTS) {
                let k = keys(m);
                let r = h.cluster_nurbs_crv_interpolate(m, org, i, k, mode, time, 1, hv[0], node) as u32;
                moved |= r;
                if r != 0 {
                    let w = x(m.f32(hv[0] + 12)).to_f64();
                    let at = m.ptr(def + nurbs::POINTS).wrapping_add((i as u32).wrapping_mul(nurbs::POINT_SIZE)) + nurbs::POINT_W;
                    m.set_f64(at, w);
                    move_crv(m, h, def, i, hv[0]);
                    knots(m, h, def);
                }
                i += 1;
            }
            if moved != 0 {
                m.or_u8(def + clu::CRV_DIRTY, 0x60);
            }
        }
        10 => {
            let mut i = 0;
            while i < m.i32(def + clu::SRF_NB_POINTS) {
                let k = keys(m);
                let r = h.cluster_nurbs_srf_interpolate(m, org, i, k, mode, time, 1, hv[0], node) as u32;
                moved |= r;
                if r != 0 {
                    let c: [f64; 4] = core::array::from_fn(|q| x(m.f32(hv[0] + 4 * q as u32)).to_f64());
                    h.nurbs_srf_move_point(m, def, i, c[0], c[1], c[2], c[3]);
                    if (m.i32(def + nurbs::SRF_KIND).wrapping_sub(3) as u32) <= 1 {
                        h.knots_srf_reparametrization(m, def);
                    }
                }
                i += 1;
            }
            if moved != 0 {
                m.or_u8(def + clu::SRF_DIRTY, 0x60);
            }
        }
        _ => h.status_display_message(m, MSG_CLUSTER, 2),
    }
    for _ in 0..3 {
        m.release(16);
    }
    for _ in 0..3 {
        m.release(12);
    }
    if moved != 0 {
        h.model_touch_definition(m, mdl);
    }
    if m.i16(mdl + model::CUSTOM_COUNT) > 0 {
        h.cus_transfer_to_definition(m, mdl, 0);
    }
}
