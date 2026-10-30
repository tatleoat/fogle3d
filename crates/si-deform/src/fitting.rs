// SPDX-License-Identifier: MIT
//! `fitting.c`: the records of a spline deformation (a model follows a curve) and of a
//! patch deformation (a model follows a surface), their matrices, and their copies when a
//! node is duplicated. The deformations themselves are another package's; these are the
//! records it reads.

use crate::globals::*;
use crate::outside::*;
use crate::records::{PtchFitting, SplFitting};
use crate::cur_dict;
use si_math::algebra::{matrix_transpose_inverse, matrix_xyz_rotation};
use si_math::types::Vector;
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_types::scene::GrafNode;
use si_x87::{fmov, trig, x};

/// The identity matrix as `SI_matrixCopy` brings it out of the global: sixteen floats, each
/// moved through the unit.
fn identity(m: &Mem) -> [[f32; 4]; 4] {
    let id = m[SI_IDENTITE];
    id.map(|row| row.map(fmov))
}

/// A new spline deformation [soft 0x0843a130 SI_fittingAllocate]: 0xa8 bytes from
/// `SI_malloc`; scale 1, 1, 1, roll and translation 0, active, no model, no frames, both
/// matrices the identity. The two bytes at +2 are not written.
pub fn fitting_allocate(m: &mut Mem) -> Ptr32<SplFitting> {
    let f: Ptr32<SplFitting> = Ptr32::new(m.si_malloc(0xa8));
    if !f.is_null() {
        m[f].scale = [1.0; 3];
        m[f].roll = 0.0;
        m[f].trans = [0.0; 3];
        m[f].active = 1;
        m[f].model = Ptr32::NULL;
        m[f].frenet = Ptr32::NULL;
        m[f].matrix = identity(m);
        m[f].normal_matrix = identity(m);
    }
    f
}

/// A copy of a spline deformation [soft 0x0843a1bc fittingCopy]: same model, frames of its
/// own (built by `SI_nurbsUpdateFrenet` when the model followed is a NURBS curve, kind 9,
/// else by `si_splineUpdateFrenet`, from that model's working definition), the seven
/// parameters and the matrix copied, then [`fitting_update_matrix`].
pub fn fitting_copy(m: &mut Mem, o: &mut dyn Outside, src: Ptr32<SplFitting>) -> Ptr32<SplFitting> {
    let dst = fitting_allocate(m);
    if dst.is_null() {
        return dst;
    }
    m[dst].active = m[src].active;
    m[dst].model = m[src].model;
    let followed = m[dst].model;
    let builder = if m[followed].kind == 9 { &SI_NURBS_UPDATE_FRENET } else { &SI_SPLINE_UPDATE_FRENET };
    let followed = m[dst].model;
    let def = m[followed].definition.addr();
    o.call(m, builder, &[def, dst.addr() + 8]);
    m[dst].scale = m[src].scale.map(fmov);
    m[dst].roll = fmov(m[src].roll);
    m[dst].trans = m[src].trans.map(fmov);
    m[dst].matrix = m[src].matrix.map(|row| row.map(fmov));
    fitting_update_matrix(m, dst);
    dst
}

/// The matrix of a spline deformation from its parameters
/// [soft 0x0843a264 SI_fittingUpdateMatrix].
///
/// s = float(sin(roll)), c = float(cos(roll)), the library called with the roll widened to
/// double, sine first. Seven elements are written and the rest of the matrix is left as it
/// is: [0][0] = float(c × sx), [0][2] = float(sx × −s), [1][1] = sy, [2][0] = float(s × sz),
/// [2][2] = float(c × sz), and row 3 the translation. So: a scaling, then a turn about y by
/// the roll, then the translation. Then the transposed inverse, for normals.
pub fn fitting_update_matrix(m: &mut Mem, f: Ptr32<SplFitting>) {
    let s = trig::sin(x(m[f].roll).to_f64()).to_f32();
    let c = trig::cos(x(m[f].roll).to_f64()).to_f32();
    let sx = m[f].scale[0];
    m[f].matrix[0][0] = (x(c) * sx).to_f32();
    m[f].matrix[0][2] = (x(sx) * -x(s)).to_f32();
    m[f].matrix[1][1] = fmov(m[f].scale[1]);
    let sz = m[f].scale[2];
    let (m20, m22) = ((x(s) * sz).to_f32(), (x(c) * sz).to_f32());
    for a in 0..3 {
        m[f].matrix[3][a] = fmov(m[f].trans[a]);
    }
    m[f].matrix[2][0] = m20;
    m[f].matrix[2][2] = m22;
    let matrix = m[f].matrix;
    m[f].normal_matrix = matrix_transpose_inverse(&matrix).0;
}

/// Frees a spline deformation with its frames and clears the pointer to it
/// [soft 0x0843a2fc SI_fittingDispose], after `SI_modelResetFcv` removed the model's curves
/// of type 0xdc (node) or 0xdd (branch) in the MODELS chapter.
pub fn fitting_dispose(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, slot: Ptr32<Ptr32<SplFitting>>, which: i16) {
    if m.read(slot).is_null() {
        return;
    }
    let chap = m[SI_CID][CID_MODELS] as u32;
    o.call(m, &SI_MODEL_RESET_FCV, &[model.addr(), chap, 2, if which != 0 { 0xdd } else { 0xdc }, 0, 0, 0, 0]);
    let f = m.read(slot);
    let frames = m[f].frenet.addr();
    if frames != 0 {
        m.free(frames);
        let f = m.read(slot);
        m[f].frenet = Ptr32::NULL;
    }
    let f = m.read(slot);
    if !f.is_null() {
        m.free(f.addr());
        m.write(slot, Ptr32::NULL);
    }
    m.write(slot, Ptr32::NULL);
}

/// A new patch deformation [soft 0x0843a384 SI_pfittingAllocate]: 0xac bytes from
/// `SI_malloc`; scale 1, 1, 1, rotation and translation 0, active, no model, the matrix the
/// identity. The two bytes at +2 and **the second matrix** are not written.
pub fn pfitting_allocate(m: &mut Mem) -> Ptr32<PtchFitting> {
    let f: Ptr32<PtchFitting> = Ptr32::new(m.si_malloc(0xac));
    if !f.is_null() {
        m[f].scale = [1.0; 3];
        m[f].rot = [0.0; 3];
        m[f].trans = [0.0; 3];
        m[f].active = 1;
        m[f].model = Ptr32::NULL;
        m[f].matrix = identity(m);
    }
    f
}

/// A copy of a patch deformation [soft 0x0843a408 pfittingCopy]: model, the nine
/// parameters and the matrix copied, then [`pfitting_update_matrix`].
pub fn pfitting_copy(m: &mut Mem, src: Ptr32<PtchFitting>) -> Ptr32<PtchFitting> {
    let dst = pfitting_allocate(m);
    if dst.is_null() {
        return dst;
    }
    m[dst].active = m[src].active;
    m[dst].model = m[src].model;
    m[dst].scale = m[src].scale.map(fmov);
    m[dst].rot = m[src].rot.map(fmov);
    m[dst].trans = m[src].trans.map(fmov);
    m[dst].matrix = m[src].matrix.map(|row| row.map(fmov));
    pfitting_update_matrix(m, dst);
    dst
}

/// The matrix of a patch deformation from its parameters
/// [soft 0x0843a484 SI_pfittingUpdateMatrix]: row r of `SI_matrixXyzRotation(rot)` times
/// the r-th scaling factor (one product per element, rounded to float), row 3 the
/// translation; the fourth column is left as it is. Then the transposed inverse.
pub fn pfitting_update_matrix(m: &mut Mem, f: Ptr32<PtchFitting>) {
    let rot = m[f].rot;
    let r = matrix_xyz_rotation(&Vector::new(rot[0], rot[1], rot[2]));
    for row in 0..3 {
        let scale = m[f].scale[row];
        for col in 0..3 {
            m[f].matrix[row][col] = (x(r[row][col]) * scale).to_f32();
        }
    }
    for a in 0..3 {
        m[f].matrix[3][a] = fmov(m[f].trans[a]);
    }
    let matrix = m[f].matrix;
    m[f].normal_matrix = matrix_transpose_inverse(&matrix).0;
}

/// Frees a patch deformation and clears the pointer to it
/// [soft 0x0843a514 SI_pfittingDispose], after `SI_modelResetFcv` removed the model's
/// curves of type 0xe6 (node) or 0xe7 (branch).
pub fn pfitting_dispose(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, slot: Ptr32<Ptr32<PtchFitting>>, which: i16) {
    if m.read(slot).is_null() {
        return;
    }
    let chap = m[SI_CID][CID_MODELS] as u32;
    o.call(m, &SI_MODEL_RESET_FCV, &[model.addr(), chap, 2, if which != 0 { 0xe7 } else { 0xe6 }, 0, 0, 0, 0]);
    let f = m.read(slot);
    if !f.is_null() {
        m.free(f.addr());
        m.write(slot, Ptr32::NULL);
    }
    m.write(slot, Ptr32::NULL);
}

/// Takes every patch deformation onto a given surface off the scene
/// [soft 0x0843a580 SI_patchRemoveFitting] (the surface is being deleted): for each model
/// node of the scene's graph list (kinds 2 and 3), `SI_modelPatchFittingReset(node, 0)` if
/// its node deformation follows `model`, and `(node, 1)` if its branch deformation does.
pub fn patch_remove_fitting(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>) {
    let proc = m[SI_PROC_DATA];
    let list = m[SI_GRAF_LISTS].add(m[proc].list);
    let mut node: Ptr32<GrafNode> = m[list].first;
    while !node.is_null() {
        if (m[node].kind as u32).wrapping_sub(2) <= 1 {
            let obj: Ptr32<Model> = m[node].data.cast();
            for which in 0..2usize {
                let f: Ptr32<PtchFitting> = m[obj].patch_fit[which].cast();
                if !f.is_null() && m[f].model == model {
                    o.call(m, &SI_MODEL_PATCH_FITTING_RESET, &[obj.addr(), which as u32]);
                }
            }
        }
        node = m[node].next;
    }
}

/// Gives a copy of a node the deformations of the node it was copied from
/// [soft 0x0843a600 si_deformationCopyFromSetup].
///
/// The attributes 0x6b to 0x8a are marked as animated ones (32 calls of
/// `SI_attdictSetFcurveFlag`). The source's active spline deformation (node or branch, as
/// `SI_modelSplineFitting` says) is copied into the same place of the copy and
/// `SI_edModelSetFitting` records it (kind 1); the same for the active patch deformation
/// (kind 0). Then the model element's animation is copied and `SI_modelUpdateDependency`
/// settles the copy.
pub fn deformation_copy_from_setup(m: &mut Mem, o: &mut dyn Outside, src: Ptr32<Model>, dst: Ptr32<Model>) {
    let frame = m.local(0x10);
    let (which_at, i1_at, i2_at) = (frame + 0xe, frame + 8, frame + 4);
    for attr in 0x6bu32..=0x8a {
        o.call(m, &SI_ATTDICT_SET_FCURVE_FLAG, &[SI_ATD.addr(), attr, 1]);
    }
    if o.call(m, &SI_MODEL_SPLINE_FITTING, &[src.addr(), which_at]) != 0 {
        let w = (m.i16(which_at) != 0) as usize;
        let from: Ptr32<SplFitting> = m[src].spline_fit[w].cast();
        let copy = fitting_copy(m, o, from);
        m[dst].spline_fit[w] = copy.cast();
        let from: Ptr32<SplFitting> = m[src].spline_fit[w].cast();
        let followed = m[from].model.addr();
        let which = m.u16(which_at) as u32;
        o.call(m, &SI_ED_MODEL_SET_FITTING, &[dst.addr(), followed, 1, which]);
    }
    if o.call(m, &SI_MODEL_PATCH_FITTING, &[src.addr(), which_at]) != 0 {
        let w = (m.i16(which_at) != 0) as usize;
        let from: Ptr32<PtchFitting> = m[src].patch_fit[w].cast();
        let copy = pfitting_copy(m, from);
        m[dst].patch_fit[w] = copy.cast();
        let from: Ptr32<PtchFitting> = m[src].patch_fit[w].cast();
        let followed = m[from].model.addr();
        let which = m.u16(which_at) as u32;
        o.call(m, &SI_ED_MODEL_SET_FITTING, &[dst.addr(), followed, 0, which]);
    }
    let e1 = o.call(m, &SI_ED_GET_ELEM_FROM_MODEL, &[src.addr(), i1_at]);
    let e2 = o.call(m, &SI_ED_GET_ELEM_FROM_MODEL, &[dst.addr(), i2_at]);
    if e2 != 0 {
        let chap = m[SI_CID][CID_MODELS] as u32;
        let (i1, i2) = (m.u32(i1_at), m.u32(i2_at));
        let dict = cur_dict(m);
        o.call(m, &SI_ELEMDICT_COPY_ELEM_ANIM, &[dict, e1, e2, chap, i1, i2]);
    }
    o.call(m, &SI_MODEL_UPDATE_DEPENDENCY, &[dst.addr(), 0]);
    m.release(0x10);
}
