// SPDX-License-Identifier: MIT
//! The end of `motionKey.c`: a shape key taken back into its slot, the key edit of the scene
//! (the key under the current frame rewritten from the present transformation, with the
//! mirrored frames of a symmetry), and the activation curve of a relation.

use crate::mem::MemExt;
use crate::ed::{self, EdHost};
use crate::fcv;
use crate::glob::{self, cid};
use crate::key::{self, KeyHost};
use si_mem::{Addr, Mem};
use crate::off::{arel, dict, elem, fcurve as fo, grafnode, key as ko, list, model, rellist};
use crate::play::PlayHost;
use crate::reset::{self, ResetHost};
use crate::savekey::RAD2DEG;
use crate::update::constr::ConstrHost;
use crate::update::params::o::{IK_GOAL, IK_STATE};
use crate::utils::UtilsHost;
use si_fcurve::Key;
use si_x87::{x, F80};

/// `SI_RelDict` (the global `SI_RD`) and its rules, as `elemdictEditElemKey` reads them
/// (the record is `RelDict` / `RelRule` of `si-types`).
pub mod reldict {
    /// Pointer to the array of rules.
    pub const RULES: u32 = 0x08;
    /// Size of a rule.
    pub const RULE_SIZE: u32 = 0x50;
    /// 16 bits: the rule's "active" word (the `act` column of `Relations.rsrc`; what
    /// `SI_reldictIsRelActive(…, 0)` answers).
    pub const RULE_ACTIVE: u32 = 0x1a;
}

/// The data record of a symmetry relation (`SI_RelSymmData`), as `SI_sceneKeyEdit` reads it.
/// The meanings are from the arithmetic only: the keys of the two sides alternate every
/// `STEP` frames from `START` on, for `COUNT + 1` steps *[guess for the names]*.
pub mod symm {
    /// 16 bits: frames from one side's key to the other's; the divisor of the routine.
    pub const STEP: u32 = 0x02;
    /// 16 bits: the first frame.
    pub const START: u32 = 0x04;
    /// 16 bits: number of further steps.
    pub const COUNT: u32 = 0x06;
}

/// `SI_glbManip` + 0x1e, 16 bits: "also the branch below" for the key edit; raised around
/// the setters in [`model_pair_apply_symmetry`].
pub const MANIP_BRANCH: u32 = 0x1e;

/// The double 0.01 at 0x0864d920: a key within that many seconds of the time is the key
/// edited.
pub const KEY_WINDOW: f64 = 0.01;

/// The float −1,000,000 at 0x0864d930: the time of the single key of a new activation curve.
pub const SINCE_EVER: f32 = -1000000.0;

crate::host_trait! {
    /// What the end of `motionKey.c` calls outside this package.
    pub trait KeyEditHost: ConstrHost, UtilsHost {
        /// `SI_patchTransferPoints(working patch, key patch)` and its kin (patch.c, face.c,
        /// mesh.c, spline.c, the NURBS files): the points of one definition into another.
        fn patch_transfer_points(a: Addr, b: Addr);
        /// `SI_patchUpdateSurface(patch)`.
        fn patch_update_surface(patch: Addr);
        fn face_transfer_points(a: Addr, b: Addr);
        fn mesh_transfer_vertices(a: Addr, b: Addr);
        fn spline_transfer_points(a: Addr, b: Addr);
        fn nurbs_crv_transfer_points_knots(a: Addr, b: Addr);
        fn nurbs_srf_transfer_points_knots(a: Addr, b: Addr);
        /// `SI_reldictGetRelIndexFromId(reldict, relation)` (reldict.c): the index of the
        /// relation's rule.
        fn reldict_get_rel_index_from_id(reldict: Addr, relid: i32) -> i32;
        /// `SI_symmetryGetElem(model, &index, &data, 1, 0)` (constraint.c): the element the
        /// model is symmetric to, its index, and the symmetry relation's data.
        fn symmetry_get_elem(mdl: Addr, index: Addr, data: Addr, a: i32, b: i32) -> Addr;
        /// `SI_modelSetScaling(model, &vector, 0, 0)`, `SI_modelSetTranslation(…)` (model.c).
        fn model_set_scaling(mdl: Addr, v: Addr, mode: i32, geom: i32);
        fn model_set_translation(mdl: Addr, v: Addr, mode: i32, geom: i32);
    }
}

/// The working geometry of a model into its shape number `n`
/// [soft 0x0821cbc4 si_modelSetIDefinition]: 0 for a number below 0 or not below the number
/// of definitions (the unsigned 16-bit word +0x34); otherwise, by the model's kind, the
/// transfer routine of that geometry is called with the first definition and definition `n`
/// (a patch then has its surface recomputed), and the answer is 1, also for a kind without
/// a transfer routine.
pub fn model_set_i_definition<H: KeyEditHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr, n: i16) -> i32 {
    if n < 0 || m.u16(mdl + model::NB_DEFS) as i32 <= n as i32 {
        return 0;
    }
    let kind = m.i32(mdl + model::KIND);
    if !matches!(kind, 1 | 2 | 4 | 6 | 9 | 10) {
        return 1;
    }
    let defs = m.ptr(mdl + model::DEFS);
    let a = m.ptr(defs);
    let b = m.ptr(defs.wrapping_add(4 * n as u32));
    match kind {
        1 => {
            h.patch_transfer_points(m, a, b);
            let b = m.ptr(m.ptr(mdl + model::DEFS).wrapping_add(4 * n as u32));
            h.patch_update_surface(m, b);
        }
        2 => h.face_transfer_points(m, a, b),
        4 => h.mesh_transfer_vertices(m, a, b),
        6 => h.spline_transfer_points(m, a, b),
        9 => h.nurbs_crv_transfer_points_knots(m, a, b),
        _ => h.nurbs_srf_transfer_points_knots(m, a, b),
    }
    1
}

/// Rewrites the keys under a time from the present transformation of a model
/// [soft 0x0821cca0 elemdictEditElemKey]. `which`: 0 scaling, 1 rotation, 2 translation.
/// Returns 1 when a key was rewritten on this element.
///
/// Every forward relation entry of the element, in all its lists, is looked at when its
/// rule in the relation dictionary is active (the 16-bit word +0x1a of the rule
/// `SI_reldictGetRelIndexFromId` finds), it points into the ANIMATION chapter and it is not
/// one of the links to animation files (15000 to 15010). Its curve is skipped when its type
/// is a scaling (1–3), rotation (4–6) or translation (139–141) other than the one asked
/// for; curves of any other type are **not** skipped. In the curve, every key whose time is
/// less than 0.01 s from `time` (|key − time| < 0.01, the difference unrounded) gets a new
/// value by the curve's type: 1, 2, 3 the local scaling (+0x100 …); 4, 5, 6 the local angles
/// (+0x10c …) × 57.29577951308232 rounded to `float`; 139, 140, 141 the translation: the
/// local one (+0x158 …), or for a node of algorithm 3 its inverse-kinematics goal when it
/// has one set (else its global translation); any other type keeps the value it has. The
/// slopes of the key before it and of the key itself are recomputed (`SI_fcnManageSlope`).
/// The walk over the keys stops behind the first key later than `time`.
///
/// With `deep`, the same for every son of the element whose graf node is not selected,
/// in the current dictionary; what the sons answer is not counted.
///
/// The model is the element's object only when `chap` is the MODELS chapter: for any other
/// chapter the original computes with a register it never set. Here that is a null model,
/// whose first use is a recorded fault.
pub fn elemdict_edit_elem_key<H: KeyEditHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, e: Addr, chap: i32, which: i32, time: f32,
                                                      deep: i16) -> i16 {
    if e == 0 {
        return 0;
    }
    let mut done = 0i16;
    let mdl = if chap == glob::chap(m, cid::MODELS) { m.ptr(e + elem::DATA) } else { 0 };
    let mut trs = [0f32; 3];
    let mut g = 0;
    while g < m.i32(e + elem::FORWARD) {
        let l = m.ptr(e + elem::FORWARD + 4).wrapping_add((g as u32).wrapping_mul(rellist::SIZE));
        let mut j = 0;
        while j < m.i32(l + rellist::NB) {
            let r = m.ptr(l + rellist::RELS).wrapping_add((j as u32).wrapping_mul(arel::SIZE));
            j += 1;
            let relid = m.i32(r + arel::RELID);
            let rule = h.reldict_get_rel_index_from_id(m, glob::SI_RD, relid);
            let rules = m.ptr(glob::SI_RD + reldict::RULES);
            if m.i16(rules.wrapping_add((rule as u32).wrapping_mul(reldict::RULE_SIZE)) + reldict::RULE_ACTIVE) == 0 {
                continue;
            }
            let to = m.i16(r + arel::CHAPID) as i32;
            if to != glob::chap(m, cid::ANIMATION) || (m.i32(r + arel::RELID).wrapping_sub(15000) as u32) <= 10 {
                continue;
            }
            let ce = ed::get_elem(m, d, to, m.i32(r + arel::ELEMID));
            let curve = m.ptr(ce + elem::DATA);
            let kind = m.i32(curve + fo::KIND);
            let family = |lo: i32| (kind.wrapping_sub(lo) as u32) <= 2;
            if (family(0x8b) && which != 2) || (family(4) && which != 1) || (family(1) && which != 0) {
                continue;
            }
            if which == 2 {
                let from = if m.i32(mdl + model::ALGO_TYPE) == 3 {
                    let ik = m.ptr(mdl + model::INV_KIN);
                    if ik != 0 && m.i32(ik + IK_STATE) != 0 { ik + IK_GOAL } else { mdl + model::GTRS }
                } else {
                    mdl + model::TRANS
                };
                trs = [m.f32(from), m.f32(from + 4), m.f32(from + 8)];
            }
            if curve == 0 {
                continue;
            }
            let mut f = fcv::load(m, curve);
            let mut changed = false;
            let mut i = 0;
            while i < m.i32(curve + fo::NB_KEYS) {
                let kt = m.f32(m.ptr(curve + fo::KEYS).wrapping_add(i as u32 * ko::SIZE) + ko::TIME);
                if (x(kt) - time).abs().lt(F80::from_f64(KEY_WINDOW)) {
                    let degrees = |m: &Mem, at: u32| (x(m.f32(mdl + at)) * RAD2DEG).to_f32();
                    let old = f.k().get(i as usize).map_or(0.0, |k| k.value);
                    let v = match kind {
                        1 => m.f32(mdl + model::SCAL),
                        2 => m.f32(mdl + model::SCAL + 4),
                        3 => m.f32(mdl + model::SCAL + 8),
                        4 => degrees(m, model::VROT),
                        5 => degrees(m, model::VROT + 4),
                        6 => degrees(m, model::VROT + 8),
                        0x8b => trs[0],
                        0x8c => trs[1],
                        0x8d => trs[2],
                        _ => old,
                    };
                    if let Some(k) = f.keys.as_mut().and_then(|k| k.get_mut(i as usize)) {
                        k.value = si_x87::fmov(v);
                    }
                    if i - 1 >= 0 {
                        f.manage_slope(i - 1);
                    }
                    f.manage_slope(i);
                    changed = true;
                    done = 1;
                }
                if x(time).lt(x(kt)) {
                    break;
                }
                i += 1;
            }
            if changed {
                fcv::store(m, curve, &f);
            }
        }
        g += 1;
    }
    if deep != 0 {
        let mut son = ed::find_son(m, h, e);
        while son != 0 {
            if m.i32(m.ptr(son + elem::AUX_DATA) + grafnode::SELECTION) == 0 {
                let cur = glob::cur_dict(m);
                elemdict_edit_elem_key(m, h, cur, son, chap, which, time, deep);
            }
            son = ed::find_next_son(m, h, e);
        }
    }
    done
}

/// One side of a symmetric pair set from the other, and the same down the two hierarchies
/// [soft 0x0821d014 modelPairApplySymmetry]. `which`: 0 scaling, 1 rotation, 2 translation.
///
/// `si_symmetryVector(a, b, which, data, 1, &v)` gives the mirrored component, which goes to
/// `SI_modelSetScaling`, `SI_modelSetRotation` or `SI_modelSetTranslation(a, &v, 0, 0)`;
/// around the two calls the 16-bit word `SI_glbManip` + 0x1e is 1, and afterwards what it
/// was. Then, with `deep` and for two different nodes: when both have a first son and the
/// son of `a` is symmetric to something with relation data (`SI_symmetryGetElem(son, &index,
/// &data, 1, 0)`), the same for the two sons with that data; and likewise for the two next
/// brothers, with the data the look-ups left (the brother's own when it has one).
pub fn model_pair_apply_symmetry<H: KeyEditHost + ?Sized>(m: &mut Mem, h: &mut H, a: Addr, b: Addr, which: i32, data: Addr, deep: i16) {
    let flag = glob::SI_GLB_MANIP + MANIP_BRANCH;
    let saved = m.u16(flag);
    m.set_u16(flag, 1);
    let v = m.local(12);
    h.symmetry_vector(m, a, b, which, data, 1, v);
    match which {
        0 => h.model_set_scaling(m, a, v, 0, 0),
        1 => h.model_set_rotation(m, a, v, 0, 0),
        2 => h.model_set_translation(m, a, v, 0, 0),
        _ => {}
    }
    m.release(12);
    m.set_u16(flag, saved);
    if a == b || deep == 0 {
        return;
    }
    let out = m.local(8);
    m.set_ptr(out + 4, data);
    for link in [model::SON, model::BROTHER] {
        let (na, nb) = (m.ptr(a + link), m.ptr(b + link));
        if na != 0 && nb != 0 && h.symmetry_get_elem(m, na, out, out + 4, 1, 0) != 0 && m.ptr(out + 4) != 0 {
            let (na, nb, d) = (m.ptr(a + link), m.ptr(b + link), m.ptr(out + 4));
            model_pair_apply_symmetry(m, h, na, nb, which, d, 1);
        }
    }
    m.release(8);
}

/// The time of a frame as `SI_sceneKeyEdit` forms it: the frame as a `float`, divided by the
/// rate in the register, rounded to `float` where it is passed on.
fn frame_time(m: &Mem, frame: i32) -> f32 {
    (x(frame as f32) / glob::frame_rate(m)).to_f32()
}

/// The key edit of the scene [soft 0x0821d17c SI_sceneKeyEdit]: for every selected model
/// node of the current graf list (kinds 2 and 3) that has an element, the keys under frame
/// `frame` are rewritten from the node's present transformation ([`elemdict_edit_elem_key`]
/// at `(float)frame / rate`, with the 16-bit word `SI_glbManip` + 0x1e as "also the sons").
/// `which`: 0 scaling, 1 rotation, 2 translation. Returns 1 when a key was rewritten, for
/// this node or any before it.
///
/// A node that is symmetric to another (`SI_symmetryGetElem(model, &index, &data, 1, 0)`)
/// takes its partner along once a key was rewritten. With n = current frame − the data's
/// start frame (16 bits at +4) and s its step (+2): when n / s is odd the two sides change
/// places first (the partner is set from the node, [`model_pair_apply_symmetry`]). Then for
/// every frame f from n mod s + start, in steps of s, below start + s × (count + 1): the keys
/// of the side whose turn it is are rewritten at frame f, and, while another frame follows,
/// the sides change places and the other side is set from this one. A step of zero divides
/// by zero in the original; here that is a recorded fault.
///
/// At the end, when a symmetry was met, a key was rewritten and `update` is set, the scene
/// is updated at the current frame (`SI_sceneUpdate`).
///
/// Two things the original does that look unintended, kept: the selected nodes that come
/// after a symmetric one are edited at the time of that node's last mirrored frame (see the
/// comment at `time`); and a negative step never reaches the end of the mirrored frames (the
/// loop runs for ever).
pub fn scene_key_edit<H: KeyEditHost + PlayHost + ?Sized>(m: &mut Mem, h: &mut H, which: i32, frame: i32, update: i16) -> i16 {
    // the time lives in one slot of the original's frame, which the loop over the mirrored
    // frames of a symmetry reuses: the nodes after a symmetric one are edited at the time of
    // its last mirrored frame, not at the frame asked for
    let mut time = frame_time(m, frame);
    let mut done = 0i16;
    let mut symmetric = false;
    let out = m.local(16);
    let (chap_at, idx_at, sidx_at, sdata_at) = (out, out + 4, out + 8, out + 12);
    let mut gn = m.ptr(glob::cur_graf_list(m) + 4);
    while gn != 0 {
        'node: {
            if m.i32(gn + grafnode::SELECTION) == 0 || (m.i32(gn + grafnode::KIND).wrapping_sub(2) as u32) >= 2 {
                break 'node;
            }
            let mut e3c = h.elemdict_find_elem_from_graf_node(m, gn, chap_at, idx_at);
            if e3c == 0 {
                break 'node;
            }
            let mut m2c = m.ptr(gn + grafnode::DATA);
            let branch = m.i16(glob::SI_GLB_MANIP + MANIP_BRANCH);
            let d = glob::cur_dict(m);
            let chap = m.i32(chap_at);
            let r = elemdict_edit_elem_key(m, h, d, e3c, chap, which, time, branch);
            let flag = (r != 0 || done != 0) as i16;
            let se = h.symmetry_get_elem(m, m2c, sidx_at, sdata_at, 1, 0);
            done = flag;
            if flag == 0 || se == 0 {
                break 'node;
            }
            symmetric = true;
            let sdata = m.ptr(sdata_at);
            let word = |m: &Mem, at: u32| m.i16(sdata + at) as i32;
            let mut m30 = m.ptr(se + elem::DATA);
            let n = m.i32(glob::SI_FRAME_CURRENT).wrapping_sub(word(m, symm::START));
            let step = word(m, symm::STEP);
            if step == 0 || (n == i32::MIN && step == -1) {
                // the original divides here
                m.crash();
                m.release(16);
                return done;
            }
            let mut e40 = se;
            if (n / step) & 1 != 0 {
                let idx = m.i32(idx_at);
                let sidx = m.i32(sidx_at);
                m.set_i32(idx_at, sidx);
                m.set_i32(sidx_at, idx);
                model_pair_apply_symmetry(m, h, m30, m2c, which, sdata, 1);
                e40 = e3c;
                e3c = se;
                core::mem::swap(&mut m30, &mut m2c);
            }
            let mut cur = e3c;
            let (mut m38, mut m34) = (m30, m2c);
            let mut f = n % step + word(m, symm::START);
            let limit = |m: &Mem| word(m, symm::START).wrapping_add(word(m, symm::STEP).wrapping_mul(word(m, symm::COUNT) + 1));
            while f < limit(m) {
                time = frame_time(m, f);
                let branch = m.i16(glob::SI_GLB_MANIP + MANIP_BRANCH);
                let d = glob::cur_dict(m);
                let chap = m.i32(chap_at);
                let r = elemdict_edit_elem_key(m, h, d, cur, chap, which, time, branch);
                let flag = (r != 0 || done != 0) as i16;
                if f.wrapping_add(word(m, symm::STEP)) < limit(m) {
                    cur = if cur == e40 { e3c } else { e40 };
                    m34 = if m34 == m30 { m2c } else { m30 };
                    m38 = if m38 == m30 { m2c } else { m30 };
                    model_pair_apply_symmetry(m, h, m34, m38, which, sdata, 1);
                }
                f = f.wrapping_add(word(m, symm::STEP));
                done = flag;
            }
        }
        gn = m.ptr(gn + grafnode::NEXT);
    }
    m.release(16);
    if symmetric && done != 0 && update != 0 {
        let now = m.i32(glob::SI_FRAME_CURRENT);
        let t = frame_time(m, now);
        PlayHost::scene_update(h, m, t);
    }
    done
}

/// The element index of a relation's activation curve, made if there is none
/// [soft 0x0821d5ac relationCheckAddActiveFcv]: the entry's own index (+8) when it has a
/// curve (`SI_edGetFcvFromRelation`); otherwise, with `keep_missing` set, −1; otherwise a new
/// curve of type 196 (owned by nothing; constant interpolation, as its type gets) with one
/// key of value 1 at −1,000,000 s ("on since ever"), filed with `SI_edFcvAdd(curve, elem,
/// chapter, index)`, whose answer is returned.
pub fn relation_check_add_active_fcv<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, rel: Addr, e: Addr, chap: i32, index: i32,
                                                           keep_missing: i16) -> i32 {
    if ed::get_fcv_from_relation(m, rel) != 0 {
        return m.i32(rel + arel::ACTIVE_ELEM);
    }
    if keep_missing != 0 {
        return -1;
    }
    let c = key::fcurve_allocate(m, 0xc4, 0);
    let k = Key::create(SINCE_EVER, 1.0, 0.0, 0, m.i16(c + fo::INTERP));
    fcv::add_key(m, h, c, k);
    h.ed_fcv_add(m, c, e, chap, index)
}

/// Switches a relation on or off for a span of time
/// [soft 0x0821d640 si_relationActiveRangeSetKey]: the entry gets an activation curve if it
/// has none ([`relation_check_add_active_fcv`]; none is made when switching **on**, `on`
/// being handed on as "keep missing": a relation without a curve is on already), and the
/// curve is made to read `on` from `from` to `to` (`si_fcurveActiveRangeSet`). A curve left
/// with a single key says nothing any more: it is removed (`SI_fcurveRemove`), its
/// ANIMATION element deleted, the entry's index set to −1 and the element's animation file
/// looked after (`SI_edCheckAnimFileDispose`). The dope sheet is flagged. Returns 0.
#[allow(clippy::too_many_arguments)]
pub fn relation_active_range_set_key<H: ResetHost + ?Sized>(m: &mut Mem, h: &mut H, rel: Addr, e: Addr, chap: i32, index: i32, from: f32,
                                                           to: f32, on: i16) -> i32 {
    let i = relation_check_add_active_fcv(m, h, rel, e, chap, index, on);
    m.set_i32(rel + arel::ACTIVE_ELEM, i);
    if i == -1 {
        return 0;
    }
    let c = ed::get_fcv_from_relation(m, rel);
    fcv::active_range_set(m, h, c, from, to, on);
    if m.i32(c + fo::NB_KEYS) == 1 {
        reset::fcurve_remove(m, h, c);
        let d = glob::cur_dict(m);
        let ani = glob::chap(m, cid::ANIMATION);
        let at = m.i32(rel + arel::ACTIVE_ELEM);
        let l = m.ptr(d + dict::LISTS).wrapping_add((ani as u32).wrapping_mul(list::SIZE));
        let ce = m.ptr(l + list::ELEMS).wrapping_add((at as u32).wrapping_mul(elem::SIZE));
        h.elemdict_set_delete_elem(m, d, ani, ce, at, -1, 0, 1);
        m.set_i32(rel + arel::ACTIVE_ELEM, -1);
        h.ed_check_anim_file_dispose(m, e, chap);
    }
    h.dope_flags_set(m, 1, 0, 1, 1);
    0
}

#[allow(unused_imports)]
use {EdHost as _, KeyHost as _};
