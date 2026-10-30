// SPDX-License-Identifier: MIT
//! Removing animation: `motionReset.c` (finding 0017, "Resetting").
//!
//! "Reset" means: the curves of an element that are **selected in the attribute table**
//! are deleted. The attribute dictionary (`SI_ATD`) carries one 16-bit word per curve type
//! ([`crate::utils::fcv_table_set`] sets them all; menu entries and chapters raise theirs);
//! a reset command first raises the words of what it wants gone and then walks the
//! element's forward relations ([`elemdict_reset_elem_chap_anim`]).

use crate::ed;
use crate::fcv;
use crate::glob::{self, cid};
use crate::key::KeyHost;
use si_mem::{Addr, Mem};
use crate::off::{arel, elem, fcurve as fo, model, pathslot, rellist};
use crate::path::{self, PathHost};
use crate::update::UpdateHost;
use si_x87::F80;

/// `SI_cusValNumCallback`, `SI_fcurveDirty`, `SI_cusFcurveDirty`: the counters a disposed
/// curve leaves (finding 0009).
const SI_CUS_VAL_NUM_CALLBACK: Addr = 0x086e_49e8;
const SI_FCURVE_DIRTY: Addr = 0x086e_49dc;
const SI_CUS_FCURVE_DIRTY: Addr = 0x086e_49e0;

/// The message `SI_modelResetFcv` formats a shape number into (the format at 0x08697660).
fn invalid_shape(n: i32) -> String {
    format!("ModelResetShape: invalid index: {n}.")
}

/// Fields of a model that only these routines use.
mod mo {
    /// 16 bits: number of keys of the node lattice, of the branch lattice (above 1: the
    /// lattice is animated).
    pub const NODE_LATTICE_KEYS: u32 = 0x4c;
    pub const BRANCH_LATTICE_KEYS: u32 = 0x54;
    /// The model's cluster record (`SI_Cluster *`) and its list of key clusters (`kluster`,
    /// next at +0x1c).
    pub const CLUSTER: u32 = 0x344;
    pub const KLUSTERS: u32 = 0x348;
    pub const KLUSTER_NEXT: u32 = 0x1c;
}

/// A wave's icon model (`wave_list_struct` + 0x74).
const WAVE_ICON_MODEL: u32 = 0x74;

crate::host_trait! {
    /// What `motionReset.c` calls outside this package.
    pub trait ResetHost: KeyHost, UpdateHost {
        /// `SI_attdictGetFcurveFlag(attribute dictionary, curve type)`: the type's
        /// "selected" word.
        fn attdict_get_fcurve_flag(atd: Addr, kind: i32) -> i16;
        /// `SI_elemdictRemoveBackwardRelId(dict, chapter, index, relation, −1)`.
        fn elemdict_remove_backward_rel_id(dict: Addr, chap: i32, index: i32, relid: i32, a: i32);
        /// `SI_elemdictRemoveRelationId(dict, reldict, index, relation)`.
        fn elemdict_remove_relation_id(dict: Addr, reldict: Addr, index: i32, relid: i32);
        /// `si_fcvScnglDelete(curve)`: the curve leaves the curve editor.
        fn fcv_scngl_delete(curve: Addr);
        /// `SI_fastSetDirty(NULL, −1, 1)`.
        fn fast_set_dirty(port: Addr, a: i32, b: i32);
        /// `SI_dialogRequest2(line 1, line 2, line 3, button 1, button 2)`: the number of the
        /// button pressed.
        fn dialog_request2(a: Addr, b: Addr, c: Addr, yes: Addr, no: Addr) -> i16;
        /// `SI_modelDeleteIDefinition(model, number)` (model.c): 0 when the number is invalid.
        fn model_delete_i_definition(mdl: Addr, n: i16) -> i16;
        /// `SI_edDisposeShape(model)`.
        fn ed_dispose_shape(mdl: Addr);
        /// `si_modelDeleteAllClusterAnim(model)`.
        fn model_delete_all_cluster_anim(mdl: Addr);
        /// `SI_modelDeleteClusterAnim(model, key cluster)`.
        fn model_delete_cluster_anim(mdl: Addr, kluster: Addr);
        /// `SI_waveInitDecay(wave)`, `SI_waveInitSpread(wave)`.
        fn wave_init_decay(wave: Addr);
        fn wave_init_spread(wave: Addr);
        /// `SI_fcvScrollBuild()`: the curve list of the editor is rebuilt.
        fn fcv_scroll_build();
        /// `si_tanConstrUpdTanData(elem)` (constraint.c).
        fn tan_constr_upd_tan_data(e: Addr);
        /// `SI_modelSplineFittingReset(model, branch)`, `SI_modelPatchFittingReset(model, branch)`.
        fn model_spline_fitting_reset(mdl: Addr, branch: i32);
        fn model_patch_fitting_reset(mdl: Addr, branch: i32);
        /// `SI_edModelRelaxFitting(model, spline, branch)`.
        fn ed_model_relax_fitting(mdl: Addr, spline: i16, branch: i32);
        /// `SI_setFcvTableEntriesFromChap(chapter)`: raises the "selected" word of every
        /// curve type of a chapter.
        fn set_fcv_table_entries_from_chap(chap: i32);
        /// `SI_edDisposeCluster(model, key cluster)`, `SI_edDisposeLattice(model, branch)`.
        fn ed_dispose_cluster(mdl: Addr, kluster: Addr);
        fn ed_dispose_lattice(mdl: Addr, branch: i32);
        /// `SI_edFcvDispose(object, chapter, list, first relation, last relation)` (edanim.c).
        fn ed_fcv_dispose(obj: Addr, chap: i32, group: i32, lo: i32, hi: i32);
        /// `SI_edFcvFilterDispose(object, chapter, list, relation, curve type, 0)`.
        fn ed_fcv_filter_dispose(obj: Addr, chap: i32, group: i32, relid: i32, kind: i32, a: i32);
        /// `SI_modelDeleteLatticeAnimation(model, branch)`.
        fn model_delete_lattice_animation(mdl: Addr, branch: i32);
        /// `si_shapeWeightFcurveDispose(model)`, `si_resetShapeWeightFcurve(model)` (shape.c).
        fn shape_weight_fcurve_dispose(mdl: Addr);
        fn reset_shape_weight_fcurve(mdl: Addr);
        /// `SI_modelRemoveLattice(model, 0, 0)`, `SI_klusterDisposeAll(model)`,
        /// `SI_clusterDispose(&model.cluster)`, `SI_modelUnlinkAllWaves(model)`.
        fn model_remove_lattice(mdl: Addr, a: i32, b: i32);
        fn kluster_dispose_all(mdl: Addr);
        fn cluster_dispose(slot: Addr);
        fn model_unlink_all_waves(mdl: Addr);
    }
}

/// `SI_fcurveDispose(&curve)` (0x0820a928, ported in `si-fcurve`) on a curve in the image: a
/// dirty curve leaves its counter, the key array, the snapshot (the same way) and the record
/// are freed.
pub fn fcurve_dispose(m: &mut Mem, c: Addr) {
    if c == 0 {
        return;
    }
    if m.i32(SI_CUS_VAL_NUM_CALLBACK) > 0 && m.u8(c + fo::DIRTY) & 1 != 0 {
        let counter = if fcv::kind(m, c) == 0x236 { SI_CUS_FCURVE_DIRTY } else { SI_FCURVE_DIRTY };
        let v = m.i32(counter).wrapping_sub(1);
        m.set_i32(counter, v);
    }
    let keys = m.ptr(c + fo::KEYS);
    if keys != 0 {
        m.free(keys);
    }
    m.set_ptr(c + fo::KEYS, 0);
    let aux = m.ptr(c + fo::AUX);
    if aux != 0 {
        fcurve_dispose(m, aux);
        m.set_ptr(c + fo::AUX, 0);
    }
    m.set_i32(c + fo::NB_KEYS, 0);
    m.free(c);
}

/// `SI_fcurveRemove(&curve)` (0x081ff0a0, ported in `si-fcurve`) on a curve in the image.
pub fn fcurve_remove<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, c: Addr) {
    if c == 0 {
        return;
    }
    if m.i16(glob::SI_STANDALONE_RENDERER) == 0 {
        h.fcv_scngl_delete(m, c);
    }
    fcurve_dispose(m, c);
    h.fast_set_dirty(m, 0, -1, 1);
}

/// When a relation of an element goes, the pointer its object kept for it goes too
/// [soft 0x082207c0 si_itemRelaxRelationPtrs].
///
/// - MODELS: 112 (path) clears the model's path (+0x2c8) and has the tangency data
///   refreshed (`si_tanConstrUpdTanData(elem)`); 220 / 221 (deformation by spline, node /
///   branch) reset the spline fitting and 230 / 231 the patch fitting, each followed by
///   `SI_edModelRelaxFitting(model, spline?, relation == 221)`.
/// - WAVES: 612 clears the path of the wave's icon model (wave + 0x74).
/// - CAMERAS: 1112 clears +0xa0, 1114 clears +0xa4.
/// - LIGHTS: 2112 clears +0x144, 2114 clears +0x148.
pub fn item_relax_relation_ptrs<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, _dict: Addr, chap: i32, e: Addr, relid: i32) {
    let obj = m.ptr(e + elem::DATA);
    if chap == glob::chap(m, cid::MODELS) {
        match relid {
            0x70 => {
                m.set_ptr(obj + pathslot::MODEL, 0);
                h.tan_constr_upd_tan_data(m, e);
            }
            0xdc | 0xdd => {
                h.model_spline_fitting_reset(m, obj, (relid == 0xdd) as i32);
                h.ed_model_relax_fitting(m, obj, 1, (relid == 0xdd) as i32);
            }
            0xe6 | 0xe7 => {
                h.model_patch_fitting_reset(m, obj, (relid == 0xe7) as i32);
                h.ed_model_relax_fitting(m, obj, 0, (relid == 0xdd) as i32);
            }
            _ => {}
        }
    } else if chap == glob::chap(m, cid::WAVES) {
        if relid == 0x264 {
            let icon = m.ptr(obj + WAVE_ICON_MODEL);
            m.set_ptr(icon + pathslot::MODEL, 0);
        }
    } else if chap == glob::chap(m, cid::CAMERAS) {
        if relid == 0x458 {
            m.set_ptr(obj + pathslot::CAMERA_POSITION, 0);
        } else if relid == 0x45a {
            m.set_ptr(obj + pathslot::CAMERA_INTEREST, 0);
        }
    } else if chap == glob::chap(m, cid::LIGHTS) {
        if relid == 0x840 {
            m.set_ptr(obj + pathslot::LIGHT_POSITION, 0);
        } else if relid == 0x842 {
            m.set_ptr(obj + pathslot::LIGHT_INTEREST, 0);
        }
    }
}

/// Deletes the selected curves of an element [soft 0x08220964 elemdictResetElemChapAnim].
/// Returns how many curves went.
///
/// 1. For a MODELS element with `deep`: first the same for the elements that hang on it in
///    list 2: its SHAPES element (relation 200), every CLUSTERS element (700), the node
///    lattice (210) and the branch lattice (215) of the LATTICES chapter; their counts add up.
/// 2. Every forward relation list of the element, every entry to the ANIMATION chapter that
///    is not an animation-file link (15000 … 15010): when the curve's type is selected in
///    the attribute table (`SI_attdictGetFcurveFlag`), the curve's element is marked for
///    deletion (`SI_elemdictRemoveBackwardRelId(dict, ANIMATION, index, 10000, −1)`, then
///    `SI_elemdictSetDeleteElem(dict, ANIMATION, curve element, its index, index, 1, del)`;
///    this takes the entry out of the list, so the same position is looked at again).
///    If the curve's relation has more than one component and is not one of 220 … 239 and,
///    after the removal, the list still holds entries of that relation but none to a curve
///    (the path model of a path, for instance), the object's pointer is cleared
///    ([`item_relax_relation_ptrs`]), the relation removed from the element
///    (`SI_elemdictRemoveRelationId`) and the list walked again from its start.
///    With `del` the curve itself is freed and its element's object cleared.
/// 3. With `del`: if anything went, `SI_edCheckAnimFileDispose(elem, chapter)`. A SHAPES
///    element that lost a shape curve (relations 201, 202) loses every shape of its model
///    but the first (`SI_modelDeleteIDefinition` from the last down to 1) and the shape
///    record; a CLUSTERS element that lost a cluster curve (701 … 702) has the cluster
///    animation of the model it belongs to deleted; a WAVES element that lost its decay
///    (614) or spread (615) curve has that parameter initialised again.
/// 4. In the interactive program, if anything went: the curve editor's list is rebuilt and
///    the key marks of the scene are set for the current frame (`SI_sceneKeySetColor((float)
///    frame / rate, 1, 0)`).
///
/// The original reads the curve through the curve element without testing the element for
/// null (a relation to a deleted ANIMATION element); the image records that as a fault.
pub fn elemdict_reset_elem_chap_anim<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, e: Addr, chap: i32, index: i32, del: i16,
                                                           deep: i16) -> i32 {
    let models = glob::chap(m, cid::MODELS);
    let ani = glob::chap(m, cid::ANIMATION);
    let mut count = 0i32;
    if chap == models && deep != 0 {
        let shapes = glob::chap(m, cid::SHAPES);
        let r = ed::find_related_elem(m, d, e, true, 2, 200, shapes);
        if r.elem != 0 {
            count = elemdict_reset_elem_anim(m, h, d, r.elem, shapes, r.index, del);
        }
        let clusters = glob::chap(m, cid::CLUSTERS);
        let mut r = ed::find_related_elem(m, d, e, true, 2, 700, clusters);
        while r.elem != 0 {
            count = count.wrapping_add(elemdict_reset_elem_anim(m, h, d, r.elem, clusters, r.index, del));
            r = ed::find_next_related_elem(m, d, e, true, 2, 700, clusters);
        }
        let lattices = glob::chap(m, cid::LATTICES);
        for rel in [0xd2, 0xd7] {
            let r = ed::find_related_elem(m, d, e, true, 2, rel, lattices);
            if r.elem != 0 {
                count = count.wrapping_add(elemdict_reset_elem_anim(m, h, d, r.elem, lattices, r.index, del));
            }
        }
    }
    let (mut shape, mut cluster, mut decay, mut spread) = (false, false, false, false);
    let mut g = 0;
    while g < m.i32(e + elem::FORWARD) {
        let l = m.ptr(e + elem::FORWARD + 4).wrapping_add((g as u32).wrapping_mul(rellist::SIZE));
        g += 1;
        let mut i = 0;
        while i < m.i32(l + rellist::NB) {
            let r = m.ptr(l + rellist::RELS).wrapping_add((i as u32).wrapping_mul(arel::SIZE));
            let mut selected = 0i16;
            if m.i16(r + arel::CHAPID) as i32 == ani && (m.i32(r + arel::RELID).wrapping_sub(15000) as u32) > 10 {
                let ci = m.i32(r + arel::ELEMID);
                let ce = ed::get_elem(m, d, m.i16(r + arel::CHAPID) as i32, ci);
                let c = m.ptr(ce + elem::DATA);
                if c != 0 {
                    selected = h.attdict_get_fcurve_flag(m, glob::SI_ATD, fcv::kind(m, c));
                    if selected != 0 {
                        count = count.wrapping_add(1);
                        h.elemdict_remove_backward_rel_id(m, d, ani, index, 10000, -1);
                        // the entry is re-read for its element index after the call above
                        let ci = m.i32(r + arel::ELEMID);
                        h.elemdict_set_delete_elem(m, d, ani, ce, ci, index, 1, del as i32);
                        let out = m.local(12);
                        h.fcv_get_rel_info(m, fcv::kind(m, c), out, out + 4, out + 8);
                        let (rel, parts) = (m.i32(out), m.i32(out + 4));
                        m.release(12);
                        if parts > 1 && (rel.wrapping_sub(0xdc) as u32) > 0x13 {
                            let (mut total, mut curves) = (0, 0);
                            let mut j = 0;
                            while j < m.i32(l + rellist::NB) {
                                let rr = m.ptr(l + rellist::RELS).wrapping_add((j as u32).wrapping_mul(arel::SIZE));
                                if m.i32(rr + arel::RELID) == rel {
                                    total += 1;
                                    if m.i16(rr + arel::CHAPID) as i32 == ani {
                                        curves += 1;
                                    }
                                }
                                j += 1;
                            }
                            if total > 0 && curves == 0 {
                                item_relax_relation_ptrs(m, h, d, chap, e, rel);
                                h.elemdict_remove_relation_id(m, d, glob::SI_RD, index, rel);
                                i = 0;
                            }
                        }
                        match rel {
                            0x266 => decay = true,
                            0x267 => spread = true,
                            201..=202 => shape = true,
                            701..=0x2be => cluster = true,
                            _ => {}
                        }
                        if del != 0 {
                            fcurve_remove(m, h, c);
                            m.set_ptr(ce + elem::DATA, 0);
                        }
                    }
                }
            }
            if selected == 0 {
                i += 1;
            }
        }
    }
    if del != 0 {
        if count > 0 {
            h.ed_check_anim_file_dispose(m, e, chap);
        }
        if shape && chap == glob::chap(m, cid::SHAPES) {
            let mdl = m.ptr(e + elem::DATA);
            let mut n = m.u16(mdl + model::NB_DEFS) as i32;
            loop {
                n -= 1;
                if n <= 0 {
                    break;
                }
                h.model_delete_i_definition(m, mdl, n as i16);
            }
            h.ed_dispose_shape(m, mdl);
        }
        if cluster && chap == glob::chap(m, cid::CLUSTERS) {
            let r = ed::find_related_elem(m, d, e, false, 0, 700, models);
            let mdl = m.ptr(r.elem + elem::DATA);
            h.model_delete_all_cluster_anim(m, mdl);
        }
        if (decay || spread) && chap == glob::chap(m, cid::WAVES) {
            let w = m.ptr(e + elem::DATA);
            if decay {
                h.wave_init_decay(m, w);
            }
            if spread {
                h.wave_init_spread(m, w);
            }
        }
    }
    if m.i16(glob::SI_STANDALONE_RENDERER) == 0 && count > 0 {
        h.fcv_scroll_build(m);
        let f = m.i32(glob::SI_FRAME_CURRENT);
        let t = (F80::from_f32(F80::from_i32(f).to_f32()) / glob::frame_rate(m)).to_f32();
        h.scene_key_set_color(m, t, 1, 0);
    }
    count
}

/// [soft 0x08220f08 SI_elemdictResetElemAnim]: [`elemdict_reset_elem_chap_anim`] with
/// `deep` = 1. The original is declared `void`, but its callers use what it leaves in the
/// register: the count.
pub fn elemdict_reset_elem_anim<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, e: Addr, chap: i32, index: i32, del: i16) -> i32 {
    elemdict_reset_elem_chap_anim(m, h, d, e, chap, index, del, 1)
}

/// Removes **all** animation of an element [soft 0x08220f38 SI_edRemoveElemChapAnim]: the
/// attribute table is set to a whole chapter before each reset.
///
/// For a MODELS element with `deep`: the SHAPES element (relation 200, list 2), every
/// CLUSTERS element (700) and then every key cluster of the model (the list at model +
/// 0x348: `SI_edDisposeCluster`), the node lattice (210) and the branch lattice (215), each
/// with its `SI_edDisposeLattice(model, 0 | 1)`. Then the element itself. Returns 0.
pub fn ed_remove_elem_chap_anim<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, chap: i32, e: Addr, index: i32, del: i16,
                                                      deep: i16) -> i32 {
    if chap == glob::chap(m, cid::MODELS) && deep != 0 {
        let mdl = m.ptr(e + elem::DATA);
        let shapes = glob::chap(m, cid::SHAPES);
        let r = ed::find_related_elem(m, d, e, true, 2, 200, shapes);
        if r.elem != 0 {
            h.set_fcv_table_entries_from_chap(m, shapes);
            elemdict_reset_elem_chap_anim(m, h, d, r.elem, shapes, r.index, del, deep);
        }
        let clusters = glob::chap(m, cid::CLUSTERS);
        let mut r = ed::find_related_elem(m, d, e, true, 2, 700, clusters);
        while r.elem != 0 {
            h.set_fcv_table_entries_from_chap(m, clusters);
            elemdict_reset_elem_chap_anim(m, h, d, r.elem, clusters, r.index, del, deep);
            r = ed::find_next_related_elem(m, d, e, true, 2, 700, clusters);
        }
        let mut k = m.ptr(mdl + mo::KLUSTERS);
        while k != 0 {
            h.ed_dispose_cluster(m, mdl, k);
            k = m.ptr(k + mo::KLUSTER_NEXT);
        }
        let lattices = glob::chap(m, cid::LATTICES);
        for (rel, branch) in [(0xd2, 0), (0xd7, 1)] {
            let r = ed::find_related_elem(m, d, e, true, 2, rel, lattices);
            if r.elem != 0 {
                h.set_fcv_table_entries_from_chap(m, lattices);
                elemdict_reset_elem_chap_anim(m, h, d, r.elem, lattices, r.index, del, deep);
                h.ed_dispose_lattice(m, mdl, branch);
            }
        }
    }
    h.set_fcv_table_entries_from_chap(m, chap);
    elemdict_reset_elem_chap_anim(m, h, d, e, chap, index, del, deep);
    0
}

/// [soft 0x08221190 SI_edRemoveElemAnim]: [`ed_remove_elem_chap_anim`] with `deep` = 1.
pub fn ed_remove_elem_anim<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, chap: i32, e: Addr, index: i32, del: i16) {
    ed_remove_elem_chap_anim(m, h, d, chap, e, index, del, 1);
}

/// Removes every curve of a model [soft 0x082211c0 SI_modelResetAll]: the attribute table
/// is set to the MODELS chapter and the element reset with deletion.
pub fn model_reset_all<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    let (e, index) = ed::get_elem_from_model(m, h, mdl);
    if e != 0 {
        let models = glob::chap(m, cid::MODELS);
        h.set_fcv_table_entries_from_chap(m, models);
        let d = glob::cur_dict(m);
        elemdict_reset_elem_anim(m, h, d, e, models, index, 1);
    }
}

/// Removes the animation of one relation of a model [soft 0x08221224 SI_modelResetFcv].
///
/// With `filtered` = 0: the path (relation 112) is dropped (model + 0x2c8 cleared,
/// [`path::path_element_relax`]); any other relation has its curves disposed of
/// (`SI_edFcvDispose(model, chapter, list, relation, relation)`), and a lattice relation
/// (210 node, 215 branch) whose lattice has more than one key also loses the lattice
/// animation.
///
/// With `filtered`: shapes (200): for the curve type 8 the weight curves are disposed of
/// and every shape but the first deleted (an invalid number is reported on the status
/// line), for any other type the weight curves are reset. The fittings (220, 221, 230,
/// 231), when `flag` points at a non-zero word: the curves of the types `first ..= last`
/// are disposed of one by one. The cluster relations 701 … 702: the cluster animation of
/// the model's first key cluster is deleted.
#[allow(clippy::too_many_arguments)]
pub fn model_reset_fcv<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, chap: i32, group: i32, relid: i32, filtered: i16,
                                            first: i32, last: i32, flag: Addr) {
    if filtered == 0 {
        if relid == 0x70 {
            m.set_ptr(mdl + pathslot::MODEL, 0);
            path::path_element_relax(m, h, mdl, 2);
        } else {
            h.ed_fcv_dispose(m, mdl, chap, group, relid, relid);
            if relid == 0xd2 {
                if m.u16(mdl + mo::NODE_LATTICE_KEYS) > 1 {
                    h.model_delete_lattice_animation(m, mdl, 0);
                }
            } else if relid == 0xd7 && m.u16(mdl + mo::BRANCH_LATTICE_KEYS) > 1 {
                h.model_delete_lattice_animation(m, mdl, 1);
            }
        }
        return;
    }
    let fitting = |m: &mut Mem, h: &mut H| {
        if flag != 0 && m.i16(flag) != 0 {
            let mut k = first;
            while (k as i16) as i32 <= last {
                h.ed_fcv_filter_dispose(m, mdl, chap, group, relid, k as i16 as i32, 0);
                k = k.wrapping_add(1);
            }
        }
    };
    if relid < 0xe8 {
        if relid < 0xe6 {
            if relid == 200 {
                if first == 8 {
                    h.shape_weight_fcurve_dispose(m, mdl);
                    let defs = m.u16(mdl + model::NB_DEFS);
                    if defs > 1 {
                        let mut n = defs.wrapping_sub(1) as i16;
                        while n > 0 {
                            if h.model_delete_i_definition(m, mdl, n) == 0 {
                                // sprintf into a 100-byte local, then the status line
                                let buf = m.local(100);
                                let mut b = invalid_shape(n as i32).into_bytes();
                                b.push(0);
                                m.set_bytes(buf, &b);
                                KeyHost::status_display_message(h, m, buf, 3);
                                m.release(100);
                            }
                            n -= 1;
                        }
                    }
                } else {
                    h.reset_shape_weight_fcurve(m, mdl);
                }
            } else if relid == 0xdc || relid == 0xdd {
                fitting(m, h);
            }
        } else {
            fitting(m, h);
        }
    } else if relid > 700 && relid < 0x2bf {
        let k = m.ptr(mdl + mo::KLUSTERS);
        h.model_delete_cluster_anim(m, mdl, k);
    }
}

/// [soft 0x08221468 SI_modelResetMaterial]: the curves of the material relations 301 … 312
/// are disposed of (`SI_edFcvDispose(material, MATERIALS, 0, 301, 312)`).
pub fn model_reset_material<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, _mdl: Addr, mat: Addr) {
    let chap = glob::chap(m, cid::MATERIALS);
    h.ed_fcv_dispose(m, mat, chap, 0, 0x12d, 0x138);
}

/// [soft 0x08221490 modelResetTxtFcv]: one relation of a 2D texture.
pub fn model_reset_txt_fcv<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, _mdl: Addr, tex: Addr, relid: i32) {
    let chap = glob::chap(m, cid::TEXTURES2D);
    h.ed_fcv_dispose(m, tex, chap, 0, relid, relid);
}

/// [soft 0x082214b4 SI_modelResetTexture]: the relations 401 … 426 of a 2D texture.
pub fn model_reset_texture<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, _mdl: Addr, tex: Addr) {
    let chap = glob::chap(m, cid::TEXTURES2D);
    h.ed_fcv_dispose(m, tex, chap, 0, 0x191, 0x1aa);
}

/// [soft 0x082214dc modelResetTx3DFcv]: one relation of a 3D texture.
pub fn model_reset_tx3d_fcv<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, _mdl: Addr, tex: Addr, relid: i32) {
    let chap = glob::chap(m, cid::TEXTURES3D);
    h.ed_fcv_dispose(m, tex, chap, 0, relid, relid);
}

/// [soft 0x08221500 SI_modelResetTexture3D]: the relations 501 … 532 of a 3D texture.
pub fn model_reset_texture3d<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, _mdl: Addr, tex: Addr) {
    let chap = glob::chap(m, cid::TEXTURES3D);
    h.ed_fcv_dispose(m, tex, chap, 0, 0x1f5, 0x214);
}

/// Removes the curves of the dynamics parameters of a model
/// [soft 0x08221528 modelResetDynamics]: all of them with `all`, else the one of curve type
/// `kind`: the root constraint (relation 53, type 182) when the model has a chain record
/// (+0x304); elasticity (50, type 179), static roughness (51, 180) and kinetic roughness
/// (52, 181) when it has a matter record (+0x2f4). Nothing for a model without an element.
pub fn model_reset_dynamics<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, all: i16, kind: i32) {
    let chn = m.ptr(mdl + model::DYNA_CHN_VARS);
    let matter = m.ptr(mdl + model::DYNA_MATTER);
    let (e, _) = ed::get_elem_from_model(m, h, mdl);
    if e == 0 {
        return;
    }
    let models = glob::chap(m, cid::MODELS);
    if chn != 0 && (all != 0 || kind == 0xb6) {
        h.ed_fcv_dispose(m, mdl, models, 0, 0x35, 0x35);
    }
    if matter != 0 {
        for (t, rel) in [(0xb3, 0x32), (0xb4, 0x33), (0xb5, 0x34)] {
            if all != 0 || kind == t {
                h.ed_fcv_dispose(m, mdl, models, 0, rel, rel);
            }
        }
    }
}

/// Removes the shape animation of a model [soft 0x08221614 SI_modelResetShape]: the SHAPES
/// element the model's element is related to by 200 in list 2 is reset with deletion, the
/// attribute table set to the SHAPES chapter, not deep.
pub fn model_reset_shape<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    let (e, _) = ed::get_elem_from_model(m, h, mdl);
    if e == 0 {
        return;
    }
    let d = glob::cur_dict(m);
    let shapes = glob::chap(m, cid::SHAPES);
    let r = ed::find_related_elem(m, d, e, true, 2, 200, shapes);
    if r.elem != 0 {
        h.set_fcv_table_entries_from_chap(m, shapes);
        let d = glob::cur_dict(m);
        elemdict_reset_elem_chap_anim(m, h, d, r.elem, shapes, r.index, 1, 0);
    }
}

/// Before a model's geometry is replaced: everything that was tied to the old geometry
/// goes [soft 0x082216b4 SI_modelResetGeomRelatedAnim], in this order: shape animation,
/// the node lattice, the key clusters and the cluster record, the spline and patch
/// fittings of node and branch, the links to waves, and whatever followed the model as a
/// path ([`path::path_delete`]).
pub fn model_reset_geom_related_anim<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, _dict: Addr, mdl: Addr) {
    let (e, _) = ed::get_elem_from_model(m, h, mdl);
    if e == 0 {
        return;
    }
    model_reset_shape(m, h, mdl);
    h.model_remove_lattice(m, mdl, 0, 0);
    h.kluster_dispose_all(m, mdl);
    h.cluster_dispose(m, mdl + mo::CLUSTER);
    h.model_spline_fitting_reset(m, mdl, 0);
    h.model_spline_fitting_reset(m, mdl, 1);
    h.model_patch_fitting_reset(m, mdl, 0);
    h.model_patch_fitting_reset(m, mdl, 1);
    h.model_unlink_all_waves(m, mdl);
    path::path_delete(m, h, mdl);
}
