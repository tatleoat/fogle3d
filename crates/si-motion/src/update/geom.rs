// SPDX-License-Identifier: MIT
//! Geometry updates of `motionUpdate.c`: shapes, clusters, lattices, deformations by spline
//! and patch, waves, metaball weights, envelopes, and the generators (extrusion,
//! revolution, skinning …), whose own update routines live in their source files.
//!
//! A word of state, the **post info**, travels along the nodes of one update
//! (`SI_updListUpdate` owns it): a deformation step leaves in it what remains to be done
//! to the model's geometry, and the model's own "geometry update" step (relation −200,
//! which the topological sort places after the deformations of the node) does it:
//!
//! | Post info | Left by | Finished as |
//! |---|---|---|
//! | 1 | a branch lattice (215), a node wave (601 → 2, 600 → 1: see [`anim_wave_update`]) | `SI_postAnimTouch(model, 1)` |
//! | 2 | a node lattice (210), a branch wave | `SI_postAnimTouch(model, 0)` |
//! | 3 | quick stretch (239) | `SI_modelReadjustDefinition(model)` |
//! | 4 | cluster centre (247) | `SI_modelReadjustDefinition(model)` |
//!
//! A mesh then gets its polygon normals recomputed, and the word is cleared.

use super::{anim_elem_selected, arg, data, nb_args, node_elem, relid, UpdateHost};
use crate::ed;
use crate::fcv;
use crate::glob::{self, anim, cid};
use si_mem::{Addr, Mem};
use crate::off::{model, updelem, updnode};
use si_x87::{Round, F80};

/// `SI_dblIdentity` (0x086d3940): the identity matrix in doubles.
pub const SI_DBL_IDENTITY: Addr = 0x086d_3940;

crate::host_trait! {
    /// What the geometry updates call outside this package.
    pub trait GeomHost: UpdateHost {
        /// `SI_animBooleanUpdate(node, time)` (animBool.c).
        fn anim_boolean_update(node: Addr, time: f32);
        /// `SI_extractionUpdate(node, time)` (curve.c).
        fn extraction_update(node: Addr, time: f32);
        /// `SI_revolutionUpdate(node, time)` (revolution.c).
        fn revolution_update(node: Addr, time: f32);
        /// `SI_extrusionUpdate(node, time)` (extrusion.c).
        fn extrusion_update(node: Addr, time: f32);
        /// `SI_fourSidedUpdate(node, time)` (surface.c).
        fn four_sided_update(node: Addr, time: f32);
        /// `SI_skinningUpdate(node, time)` (cross.c).
        fn skinning_update(node: Addr, time: f32);
        /// `SI_zipUpdate(node, time)` (zip.c).
        fn zip_update(node: Addr, time: f32);
        /// `SI_trimUpdate(node, time)` (trimDial.c).
        fn trim_update(node: Addr, time: f32);
        /// `SI_inverseUpdate(node, time)` (mod_effect.c).
        fn inverse_update(node: Addr, time: f32);
        /// `SI_postAnimTouch(model, branch)` (deformation.c).
        fn post_anim_touch(model: Addr, branch: i32);
        /// `SI_modelReadjustDefinition(model)` (model.c).
        fn model_readjust_definition(model: Addr);
        /// `SI_meshComputesPolygonNormal(mesh, -1)` (mesh_b.c).
        fn mesh_computes_polygon_normal(mesh: Addr, which: i32);
        /// `SI_edGetElemFromLattice(model, branch, &index)` (edtools.c).
        fn ed_get_elem_from_lattice(model: Addr, branch: i16, index: Addr) -> Addr;
        /// `SI_latticeInterpolate(result, fraction, from, to)` (lattice.c).
        fn lattice_interpolate(result: Addr, fraction: f32, a: Addr, b: Addr);
        /// `VCCreatePrim(0, identity, &id)` (user.c: the metaball library).
        fn vc_create_prim(kind: i32, matrix: Addr, id: Addr);
        /// `VCSetPrimParam(id, which, &value)`.
        fn vc_set_prim_param(id: i32, which: i32, value: Addr);
        /// `VCGetPrimParam(id, which, &value)`.
        fn vc_get_prim_param(id: i32, which: i32, value: Addr);
        /// `VCDeletePrim(id)`.
        fn vc_delete_prim(id: i32);
        /// `SI_metaballTouch(metaball, 0)` (metaball.c).
        fn metaball_touch(metaball: Addr, system: Addr);
        /// `SI_envelopDispDef(model)` (envtools.c).
        fn envelop_disp_def(model: Addr);
        /// `SI_envelopCopyDef(model, 1)` (envtools.c).
        fn envelop_copy_def(model: Addr, flag: i32);
        /// `SI_envelopCompUpdate(model)` (envelop.c): the envelope's deformation.
        fn envelop_comp_update(model: Addr);
        /// `SI_envelopSkelRoot(model)` (envtools.c).
        fn envelop_skel_root(model: Addr) -> Addr;
        /// `SI_envSkelSetFlag(model, root, 0, 0, local)` (envtools.c).
        fn env_skel_set_flag(model: Addr, root: Addr, a: i32, b: i32, local: i16);
        /// `SI_envelopNeedUpdate(model, root, 0, local)` (envtools.c).
        fn envelop_need_update(model: Addr, root: Addr, a: i32, local: i16) -> i16;
        /// `SI_modelLattice(model, &flag)` (lattice.c): the model's active lattice.
        fn model_lattice(model: Addr, flag: Addr) -> Addr;
        /// `SI_modelPatchFitting(model, &flag)` (model.c).
        fn model_patch_fitting(model: Addr, flag: Addr) -> Addr;
        /// `SI_modelSplineFitting(model, &flag)` (model.c).
        fn model_spline_fitting(model: Addr, flag: Addr) -> Addr;
        /// `SI_modelWave(model, &flag)` (wave_sys.c).
        fn model_wave(model: Addr, flag: Addr) -> Addr;
        /// `SI_quickStretchActive(model)` (qstretch_misc.c).
        fn quick_stretch_active(model: Addr) -> i16;
        /// `SI_modelHasConstrainedClusters(model)` (constraint.c).
        fn model_has_constrained_clusters(model: Addr) -> i16;
    }
}

/// Updates of relation list 2, by relation id [soft 0x0822a650 updNodeGeometryUpdate]:
///
/// | Relation ids | Routine |
/// |---|---|
/// | −200 | [`anim_geom_update`]: finishes what the deformations left |
/// | 200 | shape animation, skipped while the element is selected and the interaction mode (`SI_glbAnim` + 0x10) is 4 |
/// | 207 | [`anim_envelop_update`] |
/// | 210, 215 | [`anim_lattice_update`]; 211, 216 [`anim_lattice_interp_update`] |
/// | 220, 221 | deformation by spline; 230, 231 by patch |
/// | 239 | quick stretch: skipped while the element is selected and the mode is 5; with the dynamics switch off the node's kinematic state is recomputed; post info 3 |
/// | 241, 242 | [`anim_vclay_update`] |
/// | 245 | boolean; 246 `SI_animMdlClusterCnsUpdate(node, 1, 0, post, time)`; 247 post info 4 |
/// | 250 | extrusion, 251 revolution, 252 skinning, 253 four-sided, 255 zip, 256 extraction, 257 trim, 260 inverse |
/// | 600, 601 | [`anim_wave_update`] |
/// | 700 | cluster animation |
/// | 20000 | the custom effect; 20010 the expression |
pub fn upd_node_geometry_update<H: super::shape::ShapeHost + super::constr::ConstrHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32,
                                                                              post: Addr) {
    match relid(m, node) {
        0xf5 => h.anim_boolean_update(m, node, time),
        0xd3 | 0xd8 => anim_lattice_interp_update(m, node, time),
        0xe6 | 0xe7 => super::shape::anim_patch_def_update(m, h, node, time, post),
        0xdc | 0xdd => super::shape::anim_spline_def_update(m, h, node, time, post),
        0xf1 | 0xf2 => anim_vclay_update(m, h, node, time),
        0xef => {
            let d = glob::cur_dict(m);
            if m.i32(glob::SI_GLB_ANIM + anim::MODE) == 5 && anim_elem_selected(m, d, node) {
                return;
            }
            if m.i16(glob::SI_GLB_ANIM + anim::DYNAMICS) == 0 {
                super::updn_kine_state_update(m, h, node);
            }
            if post != 0 {
                m.set_i32(post, 3);
            }
        }
        0xcf => anim_envelop_update(m, h, node),
        -200 => anim_geom_update(m, h, node, post),
        200 => {
            let d = glob::cur_dict(m);
            if m.i32(glob::SI_GLB_ANIM + anim::MODE) == 4 && anim_elem_selected(m, d, node) {
                return;
            }
            super::shape::anim_shape_update(m, h, node, time);
        }
        0xd2 | 0xd7 => anim_lattice_update(m, h, node, post),
        0x100 => h.extraction_update(m, node, time),
        0xfb => h.revolution_update(m, node, time),
        0xf7 => {
            if post != 0 {
                m.set_i32(post, 4);
            }
        }
        0xf6 => super::constr::anim_mdl_cluster_cns_update(m, h, node, 1, 0, post, time),
        0xfa => h.extrusion_update(m, node, time),
        0xfd => h.four_sided_update(m, node, time),
        0xfc => h.skinning_update(m, node, time),
        0xff => h.zip_update(m, node, time),
        0x101 => h.trim_update(m, node, time),
        0x104 => h.inverse_update(m, node, time),
        600 | 601 => anim_wave_update(m, node, post),
        20000 => h.cus_update_effect(m, node, time),
        700 => super::shape::anim_clusters_update(m, h, node, time),
        0x4e2a => h.anim_expression_evaluate(m, node, time),
        _ => {}
    }
}

/// The model's own geometry step (relation −200) [soft 0x08227594 animGeomUpdate]: see the
/// table at the top of this file. The post info is read without a null test.
pub fn anim_geom_update<H: GeomHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, post: Addr) {
    let mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    match m.i32(post) {
        2 => h.post_anim_touch(m, mdl, 0),
        1 => h.post_anim_touch(m, mdl, 1),
        3 | 4 => h.model_readjust_definition(m, mdl),
        _ => {}
    }
    if m.i32(mdl + model::KIND) == 4 {
        let mesh = m.ptr(mdl + model::DEFINITION);
        h.mesh_computes_polygon_normal(m, mesh, -1);
    }
    m.set_i32(post, 0);
}

/// A wave acting on a model (relations 600 node, 601 branch)
/// [soft 0x0822741c animWaveUpdate]: nothing is computed here; when the first argument is
/// active the post info becomes 1 for relation 601 and 2 for 600 (the deformation itself is
/// applied by `SI_postAnimTouch`).
pub fn anim_wave_update(m: &mut Mem, node: Addr, post: Addr) {
    if data(m, node) != 0 && m.i16(arg(m, node, 0) + updelem::ACTIVE) != 0 && post != 0 {
        m.set_i32(post, if relid(m, node) == 0x259 { 1 } else { 2 });
    }
}

/// The interpolation position of a lattice (relations 211 `NLatInterp`, curve 66; 216
/// `BLatInterp`, curve 67) [soft 0x08226e44 animLatticeInterpUpdate]: the first curve's
/// value goes to the float at +0x40 of the node's object, a lattice record. **new**:
/// lattice + 0x40, the position along the lattice's keys.
pub fn anim_lattice_interp_update(m: &mut Mem, node: Addr, time: f32) {
    let lat = data(m, node);
    if lat == 0 {
        return;
    }
    let c = m.ptr(arg(m, node, 0) + updelem::DATA);
    if c != 0 {
        let v = fcv::value(m, c, time, 1).to_f32();
        m.set_f32(lat + 0x40, v);
    }
}

/// A lattice acting on a model (relations 210 node, 215 branch)
/// [soft 0x08226e80 animLatticeUpdate].
///
/// The model keeps two lists of lattices (node: count +0x4c, array +0x50; branch: +0x54,
/// +0x58). Entry 0 of a list is the working lattice, entries 1 … n − 1 its keys. The post
/// info becomes 2 (node) or 1 (branch). Then, only if the lattice's element has an
/// interpolation curve (relation 211 or 216 in list 2): with p the position at +0x40 of the
/// node's first argument (the lattice), k = p truncated, the working lattice is
/// `SI_latticeInterpolate(working, p − k, key[clamp(k + 1)], key[clamp(k + 2)])`, the
/// indices clamped to 1 … n − 1.
pub fn anim_lattice_update<H: GeomHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, post: Addr) {
    let mdl = data(m, node);
    if mdl == 0 {
        return;
    }
    let lat = m.ptr(arg(m, node, 0) + updelem::DATA);
    if lat == 0 {
        return;
    }
    let id = relid(m, node);
    let idx = m.local(4);
    let e = h.ed_get_elem_from_lattice(m, mdl, (id == 0xd7) as i16, idx);
    m.release(4);
    if e == 0 {
        return;
    }
    let (count_at, list_at, post_value, interp_rel) = match id {
        0xd2 => (0x4c, 0x50, 2, 0xd3),
        0xd7 => (0x54, 0x58, 1, 0xd8),
        // any other relation: the original goes on with uninitialised locals; the
        // dispatcher sends only these two
        _ => return,
    };
    if m.ptr(mdl + list_at) == 0 || m.i16(mdl + count_at) == 0 {
        return;
    }
    let last = m.u16(mdl + count_at) as i32 - 1;
    if post != 0 {
        m.set_i32(post, post_value);
    }
    if ed::find_fcv(m, e, 2, interp_rel) == 0 {
        return;
    }
    let p = m.f32(lat + 0x40);
    let k = F80::from_f32(p).to_i32(Round::Zero);
    let clamp = |i: i32| if i < 1 { 1 } else if last < i { last } else { i };
    let (i1, i2) = (clamp(k.wrapping_add(1)), clamp(k.wrapping_add(2)));
    let frac = (F80::from_f32(m.f32(lat + 0x40)) - F80::from_i32(k)).to_f32();
    let list = m.ptr(mdl + list_at);
    let (work, a, b) = (m.ptr(list), m.ptr(list.wrapping_add(4 * i1 as u32)), m.ptr(list.wrapping_add(4 * i2 as u32)));
    h.lattice_interpolate(m, work, frac, a, b);
}

/// Weights of a metaball (relations 241 `ClayWeight`, curve 185; 242 `ClaySuperExp`, curves
/// 186–188) [soft 0x0822745c animVClayUpdate]: for a model of kind 8, each curve's value is
/// stored as a **double** in the metaball record (the model's definition): 185 → +0xd4,
/// 186 → +0xdc, 187 → +0xe4, 188 → +0xec. For 185 a throw-away primitive of the metaball
/// library converts the weight (parameter 2, set from +0xd4) into the value at +0xcc
/// (parameter 1). Then `SI_metaballTouch(metaball, 0)`.
pub fn anim_vclay_update<H: GeomHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr, time: f32) {
    let mdl = data(m, node);
    if mdl == 0 || m.i32(mdl + model::KIND) != 8 {
        return;
    }
    let mb = m.ptr(mdl + model::DEFINITION);
    let mut i = 0;
    while i < nb_args(m, node) {
        let c = super::arg_curve(m, node, i);
        if c != 0 {
            let v = fcv::value(m, c, time, 1);
            match fcv::kind(m, c) {
                0xba => m.set_f64(mb + 0xdc, v.to_f64()),
                0xb9 => {
                    m.set_f64(mb + 0xd4, v.to_f64());
                    let id = m.local(4);
                    h.vc_create_prim(m, 0, SI_DBL_IDENTITY, id);
                    let prim = m.i32(id);
                    h.vc_set_prim_param(m, prim, 2, mb + 0xd4);
                    let prim = m.i32(id);
                    h.vc_get_prim_param(m, prim, 1, mb + 0xcc);
                    let prim = m.i32(id);
                    h.vc_delete_prim(m, prim);
                    m.release(4);
                }
                0xbb => m.set_f64(mb + 0xe4, v.to_f64()),
                0xbc => m.set_f64(mb + 0xec, v.to_f64()),
                _ => {}
            }
        }
        i += 1;
    }
    h.metaball_touch(m, mb, 0);
}

/// True when nothing else deforms the model: no lattice, no patch or spline fitting, no
/// wave, no quick stretch, its lattice dependency is itself (+0x44) and the word at +0x352
/// is below 1. The calls are made in this order and stop at the first that answers.
fn undeformed<H: GeomHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, clusters: bool) -> bool {
    let flag = m.local(2);
    let r = h.model_lattice(m, mdl, flag) == 0
            && h.model_patch_fitting(m, mdl, flag) == 0
            && h.model_spline_fitting(m, mdl, flag) == 0
            && h.model_wave(m, mdl, flag) == 0
            && (!clusters || h.model_has_constrained_clusters(m, mdl) == 0)
            && h.quick_stretch_active(m, mdl) == 0
            && m.ptr(mdl + 0x44) == mdl
            && m.i16(mdl + 0x352) < 1;
    m.release(2);
    r
}

/// The envelope of a skeleton (relations 206 and 207) [soft 0x08226b7c animEnvelopUpdate],
/// for a model whose envelope kind (+0x31c) is 5 to 8.
///
/// - Node or first argument inactive: the envelope shows its reference shape
///   (`SI_envelopDispDef`); if anything else deforms the model, that shape is copied back
///   as the base for those deformations (`SI_envelopCopyDef(model, 1)`).
/// - Envelope optimisation off (`SI_glbAnim` + 0x38 zero): the envelope is recomputed.
/// - Otherwise, with the skeleton's root: unless the root is already flagged (bit 1 of its
///   byte +8), the skeleton is flagged (`SI_envSkelSetFlag`; the last argument says whether
///   the kind is 6 or 7). The recomputation is **skipped** when all of this holds: the word
///   `SI_glbAnim` + 0x3c is zero, the model has fewer than two definitions and no cluster
///   animation (+0x344), the skeleton did not move (`SI_envelopNeedUpdate` answers 0),
///   nothing else deforms the model, and the model's element is neither the source of a
///   custom-effect relation in list 2 nor the destination of one in list 4.
pub fn anim_envelop_update<H: GeomHost + ?Sized>(m: &mut Mem, h: &mut H, node: Addr) {
    if node == 0 || m.ptr(node + updnode::ARGS) == 0 {
        return;
    }
    let mdl = data(m, node);
    if mdl == 0 || (m.i32(mdl + model::ENVELOP).wrapping_sub(5) as u32) >= 4 {
        return;
    }
    if m.i16(node + updnode::ACTIVE) == 0 || m.i16(arg(m, node, 0) + updelem::ACTIVE) == 0 {
        h.envelop_disp_def(m, mdl);
        if undeformed(m, h, mdl, false) {
            return;
        }
        h.envelop_copy_def(m, mdl, 1);
    } else if m.i16(glob::SI_GLB_ANIM + anim::ENVELOP_OPTIM) == 0 {
        h.envelop_comp_update(m, mdl);
    } else {
        let root = h.envelop_skel_root(m, mdl);
        if root == 0 {
            return;
        }
        let e = node_elem(m, node);
        let local = ((m.i32(mdl + model::ENVELOP).wrapping_sub(6) as u32) < 2) as i16;
        if m.u8(root + 8) & 2 == 0 {
            h.env_skel_set_flag(m, mdl, root, 0, 0, local);
        }
        if m.i16(glob::SI_GLB_ANIM + 0x3c) == 0
           && m.u16(mdl + model::NB_DEFS) < 2
           && m.ptr(mdl + 0x344) == 0
           && h.envelop_need_update(m, mdl, root, 0, local) == 0
           && undeformed(m, h, mdl, true)
        {
            if e == 0 {
                return;
            }
            let d = glob::cur_dict(m);
            let models = glob::chap(m, cid::MODELS);
            if ed::find_related_elem(m, d, e, true, 2, 20000, models).elem == 0
               && ed::find_related_elem(m, d, e, false, 4, 20000, models).elem == 0
            {
                return;
            }
        }
        h.envelop_comp_update(m, mdl);
    }
}
