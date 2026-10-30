// SPDX-License-Identifier: MIT
//! `mod_effect.c`: the update of the inverse effect and the mirror copy of a model.

use super::{elem_dict, graf_list, DeformS, ModelS, ViewS, CID_MODELS};
use super::{SI_ED_GET_ELEM_FROM_MODEL, SI_ED_RELAX_REL_FROM_DEL_MODEL, SI_STATUS_DISPLAY_MESSAGE, SLEEP};
use si_mem::{Mem, Outside, Ptr32, Routine};
use si_model::model_image::{defs, mx, state, touch, tree};
use si_types::model::Model;
use si_x87::{fmov, x, F80};

/// The text of the status line when the relation is gone: the binary's string at 0x0868a5e0.
pub const MSG_RELATION_DELETED: u32 = 0x0868_a5e0;

/// Whether a model is the object of a modelling relation: the relation ids 0xfa to 0x101 or 0xf5
/// from its element, forward, in the models chapter [soft 0x0817cd20 SI_modelHasModellingRel].
/// The element comes from `SI_edGetElemFromModel (0x080fcf20)`, which also writes the
/// element's index to a local.
pub fn model_has_modelling_rel(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) -> bool {
    let sp = m.sp();
    let local = m.local(4);
    let elem = o.call(m, SI_ED_GET_ELEM_FROM_MODEL, &[model.addr(), local]).eax;
    let scratch = m.local(4);
    let ed = Ptr32::new(elem_dict(m));
    let chap = m.i32(CID_MODELS) ;
    let found = si_dict::elemrel::find_range_related_elem(m, ed, Ptr32::new(elem), 1, 2, 0xfa, 0x101, chap, Ptr32::new(scratch), Ptr32::new(scratch));
    if !found.is_null() {
        m.set_sp(sp);
        return true;
    }
    let ed = Ptr32::new(elem_dict(m));
    let found = si_dict::elemrel::find_related_elem(m, ed, Ptr32::new(elem), 1, 2, 0xf5, chap, Ptr32::new(scratch), Ptr32::new(scratch));
    m.set_sp(sp);
    !found.is_null()
}

/// The update of an inverse effect node (`upd` the update node: the model at +8): without a
/// modelling relation the user is told and the relation 0x104 is dropped; else the model is
/// turned inside out and its graph node's routine is updated [soft 0x08177248 SI_inverseUpdate].
pub fn inverse_update(m: &mut Mem, o: &mut dyn Outside, upd: u32) {
    let model = Ptr32::<Model>::new(m.u32(upd + 8));
    if model.is_null() {
        return;
    }
    if !model_has_modelling_rel(m, o, model) {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[MSG_RELATION_DELETED, 2]);
        o.call(m, SI_ED_RELAX_REL_FROM_DEL_MODEL, &[model.addr(), 0x104]);
        o.call(m, SLEEP, &[1]);
    } else {
        defs::inverse(m, &mut ModelS(o), model);
        touch::touch_geometry(m, &mut ModelS(o), model);
        let gl = graf_list(m);
        let gn = si_view::graflist::find_gl_data(m, gl, Ptr32::new(model.addr()), 2);
        si_view::func::update_gn_func(m, &mut ViewS(o), gn);
    }
}

/// `SI_modelCopy (0x084622cc)`, `model.c`: a copy of a branch (ported on the store,
/// `si_model::hierarchy`; on the image the outside world's).
pub const SI_MODEL_COPY: Routine = Routine::new("SI_modelCopy", 0x0846_22cc);
/// `SI_modelCopyNode (0x0846234c)`, `model.c`: a copy of one node (the same).
pub const SI_MODEL_COPY_NODE: Routine = Routine::new("SI_modelCopyNode", 0x0846_234c);

/// `SI_glbManip`: its first word 1 for the classical scaling method; the second word is read
/// next to it (the original's two tests of 0x08781560 and 0x08781564).
const GLB_MANIP: u32 = 0x0878_1560;
/// The third scaling factor of the mirror, the binary's float at 0x0864ad70.
const MIRROR_Z: f32 = -1.0;
/// The length below which the plane's normal is used as it is, the binary's double at 0x0864ad78.
const NORMAL_MIN: f64 = 1.0e-4;

fn neg32(m: &mut Mem, a: u32) {
    let v = (-x(m.f32(a))).to_f32();
    m.set_f32(a, v);
}

fn copy3(m: &mut Mem, from: u32, to: u32) {
    for i in 0..3 {
        let w = m.u32(from + 4 * i);
        m.set_u32(to + 4 * i, w);
    }
}

fn vec_at(m: &Mem, a: u32) -> si_math::types::Vector {
    si_math::types::Vector::new(m.f32(a), m.f32(a + 4), m.f32(a + 8))
}

fn put_matrix(m: &mut Mem, a: u32, v: &si_math::types::Matrix) {
    for (i, row) in v.iter().enumerate() {
        for (j, e) in row.iter().enumerate() {
            m.set_f32(a + (16 * i + 4 * j) as u32, *e);
        }
    }
}

/// The mirror copy of a model in a plane [soft 0x08179cd8 SI_modelSymmetry]: `plane` is
/// six floats, a point and a normal; with `hierarchy` the whole branch is copied
/// (`SI_modelCopy` with 0, then `SI_envelopRebuild` with 0, 0), else the node alone
/// (`SI_modelCopyNode` with 1). Answers the copy.
///
/// A copy under a father first takes its global place: its scaling times the father's
/// (`cscl` for the classical method with the second word of `SI_glbManip` also 1, else
/// `gscl`), its rotation matrix times the father's `grot` (the angles taken out of it
/// against the copy's present ones) and its position through the father's `gmat`. After
/// `SI_modelUpdateSonState(null, copy, 1)`, each node of the copy (the copy, then
/// `SI_modelNextSon`'s order) has its geometry put through the scaling (1, 1, −1) and
/// turned inside out, and gets as local values the transformations taken out of
/// S × G × T(−p) × R × T(p): S that scaling, G the node's global matrix (`cmat` for the
/// classical method with the second word not 1, else `gmat`), p the plane's point and R the
/// reflection in a plane through the origin with the normal (divided by its length, the sum
/// of squares and the root at 80 bits, when that length rounded to float is above 0.0001).
/// The scaling is kept only outside the classical method with the second word not 1, the
/// angles are taken out against zeros. With `hierarchy`, a node under a father then has
/// these brought back below the father: the scaling divided by the father's (`gscl`
/// outside the classical method, `cscl` with both words 1, unchanged otherwise), the
/// rotation matrix times the inverse of the father's `grot` (not for a node of algorithm 3
/// under a chain root of inverse kinematics kind 1) and the position through the inverse of
/// the father's global matrix, each coordinate summed at 80 bits and rounded once. Then the
/// node is updated (`SI_modelUpdateSonState(father, node, 0)`); a chain root (kind 5) has
/// the floats at +0x20 and +0x24 of its kinematics record negated; a node with an envelope
/// record and the word 9, 4, 10 or 11 has the floats at +0x0c and +0x10 of its rest record
/// negated, and the one at +0x20 too unless it is a chain root or a null of algorithm 3 or
/// 4. The plane's point is negated and put back on the way (a signalling NaN comes back quiet).
pub fn model_symmetry(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, plane: u32, hierarchy: i16) -> Ptr32<Model> {
    let sp = m.sp();
    let copy = model_symmetry_inner(m, o, model.addr(), plane, hierarchy);
    m.set_sp(sp);
    Ptr32::new(copy)
}

fn model_symmetry_inner(m: &mut Mem, o: &mut dyn Outside, model: u32, plane: u32, hierarchy: i16) -> u32 {
    let classic = |m: &Mem| m.i32(GLB_MANIP) == 1;
    let second = |m: &Mem| m.i32(GLB_MANIP + 4) == 1;
    // the locals: the inverse of a father's matrix, the reflection, the scaled global matrix,
    // the geometry's scaling, a translation, the scaling factors, the angles, the position
    // and the normal
    let inv = m.local(0x40);
    let refl = m.local(0x40);
    let glob = m.local(0x40);
    let geom = m.local(0x40);
    let trs_m = m.local(0x40);
    let factors = m.local(12);
    let angles = m.local(12);
    let pos = m.local(12);
    let normal = m.local(12);
    let copy = if hierarchy != 0 {
        let c = o.call(m, SI_MODEL_COPY, &[model, 0]).eax;
        si_deform::envtools::envelop_rebuild(m, &mut DeformS(o), Ptr32::new(c), 0, 0);
        c
    } else {
        o.call(m, SI_MODEL_COPY_NODE, &[model, 1]).eax
    };
    if m.u32(model + 0x2bc) != 0 {
        let by = if classic(m) && second(m) { 0x3a0 } else { 0x16c };
        for i in 0..3 {
            let father = m.u32(model + 0x2bc);
            let v = (x(m.f32(copy + 0x100 + 4 * i)) * m.f32(father + by + 4 * i)).to_f32();
            m.set_f32(copy + 0x100 + 4 * i, v);
        }
        let father = m.u32(model + 0x2bc);
        mx::multiply_sub3(m, copy + 0x118, father + 0x178, copy + 0x118);
        mx::extract_xyz_rotation(m, copy + 0x118, copy + 0x10c);
        let father = m.u32(model + 0x2bc);
        mx::vector_multiply_matrix(m, copy + 0x158, father + 0x1c4, copy + 0x158);
    }
    state::update_son_state(m, &mut ModelS(o), Ptr32::NULL, Ptr32::new(copy), 1);
    let mut node = copy;
    while node != 0 {
        m.set_u32(factors, 0x3f80_0000);
        m.set_u32(factors + 4, 0x3f80_0000);
        m.set_f32(factors + 8, fmov(MIRROR_Z));
        mx::scaling(m, factors, geom);
        touch::multiply_definition(m, &mut ModelS(o), Ptr32::new(node), geom);
        defs::inverse(m, &mut ModelS(o), Ptr32::new(node));
        // the normal, by its length when that is not too small
        let n = [x(m.f32(plane + 0xc)), x(m.f32(plane + 0x10)), x(m.f32(plane + 0x14))];
        for (i, v) in n.iter().enumerate() {
            m.set_f32(normal + 4 * i as u32, v.to_f32());
        }
        let len: F80 = x(((n[0] * n[0] + n[1] * n[1]) + n[2] * n[2]).sqrt().to_f32());
        if len.abs().gt(x(NORMAL_MIN)) {
            for (i, v) in n.iter().enumerate() {
                m.set_f32(normal + 4 * i as u32, (*v / len).to_f32());
            }
        }
        put_matrix(m, refl, &si_math::algebra::matrix_symmetry(&vec_at(m, normal)));
        mx::scaling(m, factors, glob);
        let g = if classic(m) && !second(m) { node + 0x360 } else { node + 0x1c4 };
        mx::multiply(m, glob, g, glob);
        for i in 0..3 {
            neg32(m, plane + 4 * i);
        }
        put_matrix(m, trs_m, &si_math::algebra::matrix_translation(&vec_at(m, plane)));
        mx::multiply(m, trs_m, refl, refl);
        for i in 0..3 {
            neg32(m, plane + 4 * i);
        }
        put_matrix(m, trs_m, &si_math::algebra::matrix_translation(&vec_at(m, plane)));
        mx::multiply(m, refl, trs_m, refl);
        mx::multiply(m, glob, refl, refl);
        for i in 0..3 {
            m.set_u32(angles + 4 * i, 0);
        }
        mx::extract_transformations(m, refl, factors, angles, pos);
        if !(classic(m) && !second(m)) {
            copy3(m, factors, node + 0x100);
        }
        copy3(m, angles, node + 0x10c);
        mx::xyz_rotation(m, node + 0x10c, node + 0x118);
        copy3(m, pos, node + 0x158);
        if hierarchy != 0 && m.u32(node + 0x2bc) != 0 {
            let father = m.u32(node + 0x2bc);
            let by = if !classic(m) {
                Some(0x16c)
            } else if second(m) {
                Some(0x3a0)
            } else {
                None
            };
            if let Some(by) = by {
                for i in 0..3 {
                    let v = (x(m.f32(node + 0x100 + 4 * i)) / m.f32(father + by + 4 * i)).to_f32();
                    m.set_f32(node + 0x100 + 4 * i, v);
                }
            }
            let father = m.u32(node + 0x2bc);
            mx::inverse(m, father + 0x178, inv);
            let ik_chain = m.i32(node + 0x2ec) == 3 && m.i32(m.u32(m.u32(node + 0x2bc) + 0x318)) == 1;
            if !ik_chain {
                mx::multiply_sub3(m, node + 0x118, inv, node + 0x118);
            }
            mx::extract_xyz_rotation(m, node + 0x118, node + 0x10c);
            let father = m.u32(node + 0x2bc);
            let g = if classic(m) && !second(m) { father + 0x360 } else { father + 0x1c4 };
            mx::inverse(m, g, inv);
            let t = [x(m.f32(node + 0x158)), x(m.f32(node + 0x15c)), x(m.f32(node + 0x160))];
            for j in 0..3u32 {
                let e = |m: &Mem, i: u32| m.f32(inv + 16 * i + 4 * j);
                let v = (((t[0] * e(m, 0) + t[1] * e(m, 1)) + t[2] * e(m, 2)) + e(m, 3)).to_f32();
                m.set_f32(node + 0x158 + 4 * j, v);
            }
        }
        let father = m.u32(node + 0x2bc);
        state::update_son_state(m, &mut ModelS(o), Ptr32::new(father), Ptr32::new(node), 0);
        if m.i32(node + 0x30) == 5 {
            let k = m.u32(node + 0x318);
            neg32(m, k + 0x20);
            let k = m.u32(node + 0x318);
            neg32(m, k + 0x24);
        }
        if m.u32(node + 0x324) != 0 && matches!(m.i32(node + 0x31c), 9 | 4 | 10 | 11) {
            let kind = m.i32(node + 0x30);
            let keep = kind == 5 || (kind == 0 && (m.i32(node + 0x2ec).wrapping_sub(3) as u32) <= 1);
            if !keep {
                let r = m.u32(node + 0x328);
                neg32(m, r + 0x20);
            }
            let r = m.u32(node + 0x328);
            neg32(m, r + 0xc);
            let r = m.u32(node + 0x328);
            neg32(m, r + 0x10);
        }
        let at = if node == copy { Ptr32::NULL } else { Ptr32::new(node) };
        node = tree::next_son(m, Ptr32::new(copy), at).addr();
    }
    copy
}
