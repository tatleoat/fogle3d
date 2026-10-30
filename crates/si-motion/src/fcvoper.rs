// SPDX-License-Identifier: MIT
//! One operation on all the curves of an element, of a model with everything that hangs on
//! it, of the selection or of the scene (`motionUtils.c`; finding 0017, section 9f): the
//! "offset and scale" of a sequence, and the operations the dope sheet and the curve editor
//! run through the same walk.
//!
//! Written in the fifth pass, measured in the sixth: each of the six routines was held
//! against its instruction listing and run against the original in the emulation harness on
//! 2,000 made-up cases (finding 0017, sections 9f and 12). One fault was found that way, in
//! the bridge to the curve code and not in the walk: the frame stretch of operation 0x17
//! has to run step by step on the image ([`fcv::frame_stretch`]).

use crate::ed;
use crate::fcv;
use crate::glob::{self, cid};
use crate::off::{arel, elem, grafnode, list, model, rellist};
use crate::path::PathHost;
use crate::update::shape::ShapeHost;
use crate::utils::UtilsHost;
use si_mem::{Addr, Mem};
use si_x87::{Round, F80};

/// The operations of [`elemdict_active_fcv_oper`] (its fifth argument).
pub mod op {
    /// Is there a struct key between two times (`si_fcvCheckStructKey`); the walk stops at
    /// the first curve that has one.
    pub const CHECK_STRUCT_KEY: i32 = 0x14;
    /// `SI_fcurveOffsetAndScale(curve, start, pivot, end, offset, scale)`.
    pub const OFFSET_AND_SCALE: i32 = 0x15;
    /// `SI_fcvSetKeysOnFrames(curve, from, to)`.
    pub const KEYS_ON_FRAMES: i32 = 0x16;
    /// `si_fcvFrameStretch(curve, first, last, gap, step, insert)`, the five numbers
    /// truncated from the five `float` arguments.
    pub const FRAME_STRETCH: i32 = 0x17;
    /// The caller's function, called with (curve, first `float` argument).
    pub const FUNCTION: i32 = 0x18;
}

/// Fields read here beyond [`crate::off`].
mod f {
    /// `__SI_Elem` + 0x58: the work flag ("done in this pass"), 32 bits; cleared for a whole
    /// chapter by `SI_elemlistClearWorkFlag`.
    pub const ELEM_WORK: u32 = 0x58;
    /// `__SI_Model`: number of definitions (shapes), 16 bits unsigned.
    pub const MDL_NB_DEFS: u32 = 0x34;
    /// `__SI_Model`: number of materials (16 bits), array of material pointers.
    pub const MDL_NB_MATERIALS: u32 = 0x206;
    pub const MDL_MATERIALS: u32 = 0x20c;
    /// `__SI_Model`: list of 2D textures, the global 3D texture, list of key clusters.
    pub const MDL_TEXTURES2D: u32 = 0x21c;
    pub const MDL_TEXTURE3D: u32 = 0x224;
    pub const MDL_KLUSTERS: u32 = 0x348;
    /// `__SI_Material`: list of local 2D textures, the local 3D texture.
    pub const MAT_TEXTURES2D: u32 = 0x6ec;
    pub const MAT_TEXTURE3D: u32 = 0x6fc;
    /// `__SI_LclMap`: the next texture of a list.
    pub const TEX_NEXT: u32 = 0x16c;
    /// A key cluster: a word that must be set for the cluster to have an element looked up
    /// (+0x18), the next cluster (+0x1c).
    pub const KLUSTER_USED: u32 = 0x18;
    pub const KLUSTER_NEXT: u32 = 0x1c;
    /// `SI_defaultControl` + 0x100, 16 bits: while set, the scene's own curves (depth fading,
    /// fog) are moved with a whole-scene offset and scale *[guess: "depth fading on"]*.
    pub const CTL_SCENE_ANIMATED: u32 = 0x100;
}

crate::host_trait! {
    /// What these routines call outside this package.
    pub trait OperHost: PathHost, UtilsHost, ShapeHost {
        /// `SI_edGetElemFromCluster(key cluster, &index)` (0x080fd510).
        fn ed_get_elem_from_cluster(kluster: Addr, index: Addr) -> Addr;
        /// `SI_edGetElemFromAnimFile(curve, &index)` (0x080fd5ec): the ANIMATION element of
        /// an activation curve.
        fn ed_get_elem_from_anim_file(curve: Addr, index: Addr) -> Addr;
        /// The call through the function pointer `fcurve *(*)(fcurve *, float)` that
        /// [`elemdict_active_fcv_oper`] is handed for operation 0x18: `func` is the code
        /// address, as the original holds it.
        fn call_fcurve_function(func: Addr, curve: Addr, value: f32) -> Addr;
        /// `SI_elemlistClearWorkFlag(dict, chapter)` (0x08132c30).
        fn elemlist_clear_work_flag(dict: Addr, chap: i32);
        /// `SI_actionsTmpActivateAll(dict, actions, restore)` (0x081dfa68).
        fn actions_tmp_activate_all(dict: Addr, actions: Addr, restore: i32);
        /// `SI_actionsOffsetAndScaleDef(actions, start, pivot, end, offset, scale)` (0x081e4488).
        fn actions_offset_and_scale_def(actions: Addr, start: f32, pivot: f32, end: f32, offset: f32, scale: f32);
        /// `SI_shaderFcurveOffsetAndScaleSelected` (0x08531b54), `…All` (0x08531a4c),
        /// `SI_customFcurveOffsetAndScaleSelected` (0x08532158), `…All` (0x0853227c): the
        /// same operation on the curves of shaders and of custom effects.
        fn shader_fcurve_offset_and_scale_selected(start: f32, pivot: f32, end: f32, offset: f32, scale: f32);
        fn custom_fcurve_offset_and_scale_selected(start: f32, pivot: f32, end: f32, offset: f32, scale: f32);
        fn shader_fcurve_offset_and_scale_all(start: f32, pivot: f32, end: f32, offset: f32, scale: f32);
        fn custom_fcurve_offset_and_scale_all(start: f32, pivot: f32, end: f32, offset: f32, scale: f32);
    }
}

/// `SI_fcurveOffsetAndScale(curve, start, pivot, end, offset, scale)` (0x08201210, ported in
/// `si-fcurve`) on a curve of the image.
fn offset_and_scale<H: OperHost + ?Sized>(m: &mut Mem, h: &mut H, curve: Addr, a: [f32; 5]) {
    fcv::Ops::on(m, h, curve, |f, env| f.offset_and_scale(a[0], a[1], a[2], a[3], a[4], env));
}

/// The work flag of ANIMATION element `index` of the **current** dictionary.
fn anim_work_flag(m: &Mem, index: i32) -> Addr {
    let d = glob::cur_dict(m);
    let l = m.ptr(d + 4).wrapping_add((glob::chap(m, cid::ANIMATION) as u32).wrapping_mul(list::SIZE));
    m.ptr(l + list::ELEMS).wrapping_add((index as u32).wrapping_mul(elem::SIZE)).wrapping_add(f::ELEM_WORK)
}

/// One operation on the curves of an element [soft 0x0822c8a4 SI_elemdictActiveFcvOper].
/// Returns 1 when the operation ran on a curve (for 0x14: when a curve has a struct key),
/// 0 for a null element.
///
/// For the operations 0x15 and 0x18 the attribute table is first set to the element's
/// chapter (`SI_setFcvTableEntriesFromChap`). Then every forward relation entry of the
/// element, list by list:
///
/// - With `with_act` the entry's **activation curve** is fetched first (`SI_edGetFcvFromRelation`).
/// - **An entry that leads to a curve** (ANIMATION chapter, not 15000 to 15010): the curve
///   must exist and its type be selected; with `once`, the curve's element must not carry
///   the work flag (+0x58) yet, and gets it afterwards. Then, by `oper` ([`op`]):
///   0x14 returns 1 at once when the curve has a struct key in [a0, a1]; 0x15 offsets and
///   scales the entry's activation curve (with `once` only if that curve's element, as
///   `SI_edGetElemFromAnimFile` finds it, does not carry the flag; the flag is not set
///   here) and then the curve; 0x16 puts the keys in [a0, a1] on frames; 0x17 stretches
///   ([`fcv::frame_stretch`]; a step that truncates to zero stops the original in its
///   division); 0x18 calls `func(curve, a0)`. Any other number does nothing but the flag.
/// - **Any other entry** (a constraint, a path, an animation-file link): for 0x15 only, its
///   activation curve is offset and scaled, unless `once` and the curve's ANIMATION element
///   (index entry + 8, in the current dictionary) carries the flag; the flag is then set,
///   `once` or not.
///
/// A relation to a deleted ANIMATION element is read through a null pointer by the
/// original; the image records a fault.
#[allow(clippy::too_many_arguments)]
pub fn elemdict_active_fcv_oper<H: OperHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, e: Addr, chap: i32, _index: i32, oper: i32, func: Addr,
                                                     a: [f32; 5], with_act: i16, once: i16)
                                                     -> i16 {
    if e == 0 {
        return 0;
    }
    if oper == op::OFFSET_AND_SCALE || oper == op::FUNCTION {
        h.set_fcv_table_entries_from_chap(m, chap);
    }
    let mut done: i16 = 0;
    let mut g = 0;
    while g < m.i32(e + elem::FORWARD) {
        let l = m.ptr(e + elem::FORWARD + 4).wrapping_add((g as u32).wrapping_mul(rellist::SIZE));
        let mut i = 0;
        while i < m.i32(l + rellist::NB) {
            let r = m.ptr(l + rellist::RELS).wrapping_add((i as u32).wrapping_mul(arel::SIZE));
            let act = if with_act != 0 { ed::get_fcv_from_relation(m, r) } else { 0 };
            let ce = ed::get_elem(m, d, m.i16(r + arel::CHAPID) as i32, m.i32(r + arel::ELEMID));
            let to_curve = m.i16(r + arel::CHAPID) as i32 == glob::chap(m, cid::ANIMATION) && (m.i32(r + arel::RELID).wrapping_sub(15000) as u32) > 10;
            'entry: {
                if !to_curve {
                    if act != 0 && oper == op::OFFSET_AND_SCALE {
                        if once != 0 && m.i32(anim_work_flag(m, m.i32(r + arel::ACTIVE_ELEM))) != 0 {
                            break 'entry;
                        }
                        offset_and_scale(m, h, act, a);
                        let flag = anim_work_flag(m, m.i32(r + arel::ACTIVE_ELEM));
                        m.set_i32(flag, 1);
                    }
                    break 'entry;
                }
                let c = m.ptr(ce.wrapping_add(elem::DATA));
                if c == 0 || h.attdict_get_fcurve_flag(m, glob::SI_ATD, fcv::kind(m, c)) == 0 {
                    break 'entry;
                }
                if once != 0 && m.i32(ce + f::ELEM_WORK) != 0 {
                    break 'entry;
                }
                match oper {
                    op::CHECK_STRUCT_KEY => {
                        if fcv::load(m, c).check_struct_key(a[0], a[1]) {
                            return 1;
                        }
                    }
                    op::OFFSET_AND_SCALE => {
                        if act != 0 {
                            let mut free = true;
                            if once != 0 {
                                let idx = m.local(4);
                                let ae = h.ed_get_elem_from_anim_file(m, act, idx);
                                m.release(4);
                                if ae != 0 {
                                    free = m.i32(ae + f::ELEM_WORK) == 0;
                                }
                            }
                            if free {
                                offset_and_scale(m, h, act, a);
                            }
                        }
                        offset_and_scale(m, h, c, a);
                        done = 1;
                    }
                    op::KEYS_ON_FRAMES => {
                        fcv::Ops::on(m, h, c, |f, env| f.set_keys_on_frames(a[0], a[1], env));
                        done = 1;
                    }
                    op::FRAME_STRETCH => {
                        // the five floats, each truncated towards zero; the last as 16 bits
                        let n = |v: f32| F80::from_f32(v).to_i32(Round::Zero);
                        let (first, last, gap, step, insert) = (n(a[0]), n(a[1]), n(a[2]), n(a[3]), n(a[4]) as i16);
                        fcv::frame_stretch(m, h, c, first, last, gap, step, insert);
                        done = 1;
                    }
                    op::FUNCTION => {
                        h.call_fcurve_function(m, func, c, a[0]);
                        done = 1;
                    }
                    _ => {}
                }
                if once != 0 {
                    m.set_i32(ce + f::ELEM_WORK, 1);
                }
            }
            i += 1;
        }
        g += 1;
    }
    done
}

/// The elements that carry a model's animation, in the order the two model walks visit
/// them, each handed to `each(element, chapter)`: the model's own; its node lattice (relation
/// 210 in list 2) and branch lattice (215); every material (model + 0x20c, count + 0x206)
/// with an element, and for it the material's 2D textures (list at + 0x6ec) and its local 3D
/// texture (+ 0x6fc); the model's 2D textures (list at + 0x21c) and global 3D texture
/// (+ 0x224); every key cluster in use (list at + 0x348, word + 0x18 set); and, for a model
/// with more than one definition, its SHAPES element. A part without an element is passed
/// over (`SI_modelAFunction` hands some of them on as null, to a routine that returns at
/// once for null).
fn model_walk<H: OperHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, mut each: impl FnMut(&mut Mem, &mut H, Addr, i32)) {
    let idx = m.local(4);
    let chapter = |m: &Mem, which: u32| glob::chap(m, which);
    let (e, _) = ed::get_elem_from_model(m, h, mdl);
    if e != 0 {
        let c = chapter(m, cid::MODELS);
        each(m, h, e, c);
    }
    for relid in [0xd2, 0xd7] {
        let (d, lattices) = (glob::cur_dict(m), chapter(m, cid::LATTICES));
        let r = ed::find_related_elem(m, d, e, true, 2, relid, lattices);
        if r.elem != 0 {
            each(m, h, r.elem, lattices);
        }
    }
    if m.ptr(mdl + f::MDL_MATERIALS) != 0 && m.i16(mdl + f::MDL_NB_MATERIALS) > 0 {
        let mut i: i32 = 0;
        loop {
            let mat = m.ptr(m.ptr(mdl + f::MDL_MATERIALS).wrapping_add((i as u32).wrapping_mul(4)));
            let me = h.ed_get_elem_from_material(m, mat, idx);
            i += 1;
            if me != 0 {
                let c = chapter(m, cid::MATERIALS);
                each(m, h, me, c);
                let mut tex = m.ptr(mat + f::MAT_TEXTURES2D);
                while tex != 0 {
                    let te = h.ed_get_elem_from_texture2d(m, tex, idx);
                    if te != 0 {
                        let c = chapter(m, cid::TEXTURES2D);
                        each(m, h, te, c);
                    }
                    tex = m.ptr(tex + f::TEX_NEXT);
                }
                let tex = m.ptr(mat + f::MAT_TEXTURE3D);
                if tex != 0 {
                    let te = h.ed_get_elem_from_texture3d(m, tex, idx);
                    if te != 0 {
                        let c = chapter(m, cid::TEXTURES3D);
                        each(m, h, te, c);
                    }
                }
            }
            if i >= m.i16(mdl + f::MDL_NB_MATERIALS) as i32 {
                break;
            }
        }
    }
    let mut tex = m.ptr(mdl + f::MDL_TEXTURES2D);
    while tex != 0 {
        let te = h.ed_get_elem_from_texture2d(m, tex, idx);
        if te != 0 {
            let c = chapter(m, cid::TEXTURES2D);
            each(m, h, te, c);
        }
        tex = m.ptr(tex + f::TEX_NEXT);
    }
    let tex = m.ptr(mdl + f::MDL_TEXTURE3D);
    if tex != 0 {
        let te = h.ed_get_elem_from_texture3d(m, tex, idx);
        if te != 0 {
            let c = chapter(m, cid::TEXTURES3D);
            each(m, h, te, c);
        }
    }
    let mut k = m.ptr(mdl + f::MDL_KLUSTERS);
    while k != 0 {
        if m.i32(k + f::KLUSTER_USED) != 0 {
            let ke = h.ed_get_elem_from_cluster(m, k, idx);
            if ke != 0 {
                let c = chapter(m, cid::CLUSTERS);
                each(m, h, ke, c);
            }
        }
        k = m.ptr(k + f::KLUSTER_NEXT);
    }
    if m.u16(mdl + f::MDL_NB_DEFS) > 1 {
        let se = h.ed_get_elem_from_shape(m, mdl, idx);
        if se != 0 {
            let c = chapter(m, cid::SHAPES);
            each(m, h, se, c);
        }
    }
    m.release(4);
}

/// Calls a function on every selected curve of a model and of what hangs on it
/// [soft 0x0822cc48 SI_modelAFunction]: [`elemdict_active_fcv_oper`] with operation 0x18 on
/// the elements of [`model_walk`]. (The curve editor's "remove from the list" is run this
/// way, with `SI_fcvScnglBDelete`.)
pub fn model_a_function<H: OperHost + ?Sized>(m: &mut Mem, h: &mut H, func: Addr, mdl: Addr, value: f32) {
    if mdl == 0 {
        return;
    }
    model_walk(m, h, mdl, |m, h, e, chap| {
        let d = glob::cur_dict(m);
        elemdict_active_fcv_oper(m, h, d, e, chap, 0, op::FUNCTION, func, [value, 0.0, 0.0, 0.0, 0.0], 0, 0);
    });
}

/// Offsets and scales the curves of a model and of what hangs on it
/// [soft 0x0822e270 modelOffsetAndScale]: operation 0x15 on the elements of [`model_walk`],
/// each curve once (`once` = 1).
pub fn model_offset_and_scale<H: OperHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, a: [f32; 5], with_act: i16) {
    if mdl == 0 {
        return;
    }
    model_walk(m, h, mdl, |m, h, e, chap| {
        let d = glob::cur_dict(m);
        elemdict_active_fcv_oper(m, h, d, e, chap, 0, op::OFFSET_AND_SCALE, 0, a, with_act, 1);
    });
}

/// Offsets and scales the curves of one object on screen
/// [soft 0x0822e840 objSeqOffsetAndScale]: for the graf kinds 2, 3 (a model: by
/// [`model_offset_and_scale`]), 4, 6, 8, 0xb, 0x13, 0x14 (by the element's own curves, each
/// once); nothing for the other kinds (the interests 5 and 7 among them) or without an
/// element. Returns 1 for a camera (kind 4), else 0.
///
/// The five floats pass through the unit on their way to the callee (`fld`, `fstp`: a
/// signalling NaN goes on quiet), unlike in the routines around this one, which copy the
/// words. Nothing a case can show depends on it: every use of the five is arithmetic.
#[allow(clippy::too_many_arguments)]
pub fn obj_seq_offset_and_scale<H: OperHost + ?Sized>(m: &mut Mem, h: &mut H, gn: Addr, e: Addr, a: [f32; 5], with_act: i16, chap: i32,
                                                     index: i32)
                                                     -> i16 {
    if e == 0 {
        return 0;
    }
    let kind = m.i32(gn + grafnode::KIND);
    let camera = match kind {
        4 => 1,
        2 | 3 | 6 | 8 | 0xb | 0x13 | 0x14 => 0,
        _ => return 0,
    };
    let a = a.map(si_x87::fmov);
    if (kind.wrapping_sub(2) as u32) < 2 {
        let mdl = m.ptr(gn + grafnode::DATA);
        model_offset_and_scale(m, h, mdl, a, with_act);
    } else {
        let d = glob::cur_dict(m);
        elemdict_active_fcv_oper(m, h, d, e, chap, index, op::OFFSET_AND_SCALE, 0, a, with_act, 1);
    }
    camera
}

/// The same for an element and all its descendants
/// [soft 0x0822e744 objSeqOffsetAndScaleHier]: the element (when it has a graf node), then
/// each MODELS element that names it as father (the backward entries of relation 110 in
/// list 1), recursively, each with its own graf node (element + 8). The results are or-ed.
#[allow(clippy::too_many_arguments)]
pub fn obj_seq_offset_and_scale_hier<H: OperHost + ?Sized>(m: &mut Mem, h: &mut H, gn: Addr, e: Addr, a: [f32; 5], with_act: i16, chap: i32,
                                                          index: i32)
                                                          -> i16 {
    let mut any: i16 = 0;
    if gn != 0 && e != 0 {
        any = obj_seq_offset_and_scale(m, h, gn, e, a, with_act, chap, index);
    }
    let models = glob::chap(m, cid::MODELS);
    let d = glob::cur_dict(m);
    let mut s = ed::find_related_elem(m, d, e, false, 1, 0x6e, models);
    while s.elem != 0 {
        let son_gn = m.ptr(s.elem + elem::AUX_DATA);
        any |= obj_seq_offset_and_scale_hier(m, h, son_gn, s.elem, a, with_act, s.first, s.index);
        let (d, models) = (glob::cur_dict(m), glob::chap(m, cid::MODELS));
        s = ed::find_next_related_elem(m, d, e, false, 1, 0x6e, models);
    }
    any
}

/// `(float)frame / SI_frameRate`, rounded to `float`.
fn frame_time(m: &Mem, frame: i32) -> f32 {
    (F80::from_f32(F80::from_i32(frame).to_f32()) / glob::frame_rate(m)).to_f32()
}

/// The "offset and scale" command on a sequence [soft 0x0822d0bc SI_sequenceOffsetAndScale]:
/// every key between the frames `start` and `end` of the curves concerned is moved by
/// `offset` seconds and scaled by `scale` about the frame `pivot`
/// (`SI_fcurveOffsetAndScale`, finding 0009); the three frames are handed on as times,
/// `(float)frame / rate` rounded to `float`.
///
/// The work flags of the ANIMATION chapter are cleared first, so that a curve reached twice
/// is moved once.
///
/// **The selection** (`all` = 0): every selected node of the current graf list with an
/// element: [`obj_seq_offset_and_scale`]. The actions of the element's hierarchy root
/// (`SI_edGetActions(chapter, root index)`) are opened for the duration
/// (`SI_actionsTmpActivateAll(…, 0)` before, `(…, 1)` after) and moved too
/// (`SI_actionsOffsetAndScaleDef`) when the element is that root, is not a model selected
/// otherwise than as a branch (selection 3), and the node is not a camera's or a light's
/// interest (kinds 5, 7). Then the shaders' and custom effects' selected curves.
///
/// **The scene** (`all` set): every node of the list with an element. A model counts only
/// as a root (no father); its actions are opened and moved, and its whole hierarchy is
/// walked ([`obj_seq_offset_and_scale_hier`]). Any other element: its actions (moved unless
/// the node is an interest) and [`obj_seq_offset_and_scale`]. If no camera was among the
/// nodes and there is a current camera (the process record's third word), that camera's
/// curves and actions are moved as well. With the word `SI_defaultControl` + 0x100 set the
/// scene element's curves follow. Then all curves of shaders and custom effects.
///
/// The two locals the element look-up writes through start as zero here.
#[allow(clippy::too_many_arguments)]
pub fn sequence_offset_and_scale<H: OperHost + ?Sized>(m: &mut Mem, h: &mut H, all: i16, with_act: i16, start: i32, pivot: i32, end: i32,
                                                      offset: f32, scale: f32) {
    let ani = glob::chap(m, cid::ANIMATION);
    let d = glob::cur_dict(m);
    h.elemlist_clear_work_flag(m, d, ani);
    let a = [frame_time(m, start), frame_time(m, pivot), frame_time(m, end), offset, scale];
    let moved = |m: &mut Mem, h: &mut H, actions: Addr| h.actions_offset_and_scale_def(m, actions, a[0], a[1], a[2], a[3], a[4]);
    let open = |m: &mut Mem, h: &mut H, actions: Addr, restore: i32| {
        let d = glob::cur_dict(m);
        h.actions_tmp_activate_all(m, d, actions, restore);
    };
    let interest = |m: &Mem, chap: i32, gn: Addr| {
        (chap == glob::chap(m, cid::CAMERAS) && m.i32(gn + grafnode::KIND) == 5) || (chap == glob::chap(m, cid::LIGHTS) && m.i32(gn + grafnode::KIND) == 7)
    };
    let slot = m.local(4);
    let out = m.local(8);
    m.set_ptr(slot, 0);
    if all == 0 {
        loop {
            let gl = glob::cur_graf_list(m);
            h.next_grafnode(m, gl, slot);
            let gn = m.ptr(slot);
            if gn == 0 {
                break;
            }
            if m.i32(gn + grafnode::SELECTION) == 0 {
                continue;
            }
            let e = h.elemdict_find_elem_from_graf_node(m, gn, out, out + 4);
            if e == 0 {
                continue;
            }
            let actions = h.ed_get_actions(m, m.i32(out), m.i32(e + elem::ROOT_ID));
            if actions != 0 {
                open(m, h, actions, 0);
                let chap = m.i32(out);
                let gn = m.ptr(slot);
                if (chap != glob::chap(m, cid::MODELS) || m.i32(gn + grafnode::SELECTION) == 3)
                   && m.i32(out + 4) == m.i32(e + elem::ROOT_ID)
                   && !interest(m, chap, gn)
                {
                    moved(m, h, actions);
                }
            }
            let (gn, chap, index) = (m.ptr(slot), m.i32(out), m.i32(out + 4));
            obj_seq_offset_and_scale(m, h, gn, e, a, with_act, chap, index);
            if actions != 0 {
                open(m, h, actions, 1);
            }
        }
        h.shader_fcurve_offset_and_scale_selected(m, a[0], a[1], a[2], a[3], a[4]);
        h.custom_fcurve_offset_and_scale_selected(m, a[0], a[1], a[2], a[3], a[4]);
    } else {
        let mut camera_seen: i16 = 0;
        loop {
            let gl = glob::cur_graf_list(m);
            h.next_grafnode(m, gl, slot);
            if m.ptr(slot) == 0 {
                break;
            }
            let gn = m.ptr(slot);
            let e = h.elemdict_find_elem_from_graf_node(m, gn, out, out + 4);
            if e == 0 {
                continue;
            }
            let chap = m.i32(out);
            let actions;
            let r;
            if chap == glob::chap(m, cid::MODELS) {
                if m.ptr(m.ptr(e + elem::DATA).wrapping_add(model::FATHER)) != 0 {
                    continue;
                }
                actions = h.ed_get_actions(m, chap, m.i32(out + 4));
                if actions != 0 {
                    open(m, h, actions, 0);
                    moved(m, h, actions);
                }
                let (son_gn, chap, index) = (m.ptr(e + elem::AUX_DATA), m.i32(out), m.i32(out + 4));
                r = obj_seq_offset_and_scale_hier(m, h, son_gn, e, a, with_act, chap, index);
            } else {
                actions = h.ed_get_actions(m, chap, m.i32(out + 4));
                if actions != 0 {
                    open(m, h, actions, 0);
                    let (chap, gn) = (m.i32(out), m.ptr(slot));
                    if !interest(m, chap, gn) {
                        moved(m, h, actions);
                    }
                }
                let (gn, chap, index) = (m.ptr(slot), m.i32(out), m.i32(out + 4));
                r = obj_seq_offset_and_scale(m, h, gn, e, a, with_act, chap, index);
            }
            camera_seen |= r;
            if actions != 0 {
                open(m, h, actions, 1);
            }
        }
        let camera = m.ptr(m.ptr(glob::SI_PROC_DATA) + 8);
        if camera_seen == 0 && camera != 0 {
            let cameras = glob::chap(m, cid::CAMERAS);
            let e = h.ed_get_elem_from_camera(m, camera, out + 4);
            let (d, index) = (glob::cur_dict(m), m.i32(out + 4));
            elemdict_active_fcv_oper(m, h, d, e, cameras, index, op::OFFSET_AND_SCALE, 0, a, with_act, 1);
            let actions = h.ed_get_actions(m, glob::chap(m, cid::CAMERAS), m.i32(out + 4));
            if actions != 0 {
                open(m, h, actions, 0);
                moved(m, h, actions);
                open(m, h, actions, 1);
            }
        }
        if m.i16(glob::SI_DEFAULT_CONTROL + f::CTL_SCENE_ANIMATED) != 0 {
            let e = h.ed_get_elem_from_scene(m, out + 4);
            let (d, scenes, index) = (glob::cur_dict(m), glob::chap(m, cid::SCENES), m.i32(out + 4));
            elemdict_active_fcv_oper(m, h, d, e, scenes, index, op::OFFSET_AND_SCALE, 0, a, with_act, 1);
        }
        h.shader_fcurve_offset_and_scale_all(m, a[0], a[1], a[2], a[3], a[4]);
        h.custom_fcurve_offset_and_scale_all(m, a[0], a[1], a[2], a[3], a[4]);
    }
    m.release(8);
    m.release(4);
}
