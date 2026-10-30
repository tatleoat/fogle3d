// SPDX-License-Identifier: MIT
//! Path animation: `motionPath.c` (finding 0017, "Paths").
//!
//! An object follows a path through three things that these routines keep in step:
//!
//! - the **path model**: a spline node (kind 6) or a NURBS curve node, which the object
//!   points at (model + 0x2c8, camera + 0xa0 / + 0xa4, light + 0x144 / + 0x148) and to which
//!   its element is related twice, in lists 1 and 2, by the path relation of its kind (112
//!   models, 612 waves, 1112 / 1114 cameras, 2112 / 2114 lights);
//! - the **percentage curve** (curve types 7, 172, 9, 10, 31, 32), filed under the same
//!   relation: time → percentage of the path's length, 0 to 100;
//! - **path keys**: keys of the percentage curve whose type (low four bits of the key's
//!   flag word) is 2 and whose word at +0x1e names a point of the path's curve. Their value
//!   is not free: it is the percentage of the length at which that point lies, recomputed
//!   whenever the curve changes ([`update_fcurve_path_keys`]). The curve's "keyframed" word
//!   (spline + 0x70, NURBS curve + 0x4c) says that such keys exist.
//!
//! The evaluation at a time is in [`crate::update::transfo`] (`finalComputePathTrans`).
//! The geometry of the curves (length, parameter at a distance, adding a point) belongs to
//! the spline and NURBS packages and goes through [`PathHost`].

use crate::ed;
use crate::fcv;
use crate::glob::{self, cid};
use crate::key::{self, KeyHost};
use si_mem::{Addr, Mem};
use crate::off::{arel, elem, fcurve as fo, graf, grafnode, key as ko, keyflag, model, nurbs, pathslot, rellist, spline};
use crate::reset::{self, ResetHost};
use crate::update::transfo::TransfoHost;
use crate::update::sec_to_frame;
use crate::utils;
use si_fcurve::{Fcurve, Key};
use si_x87::{fmov, Round, F80};

/// Strings of the binary's data that these routines hand to the status line and to the
/// confirmation dialogue, by address.
pub mod text {
    use si_mem::Addr;
    /// "path": the name of a new path model.
    pub const PATH_NAME: Addr = 0x0869_74a0;
    /// "Select a root model."
    pub const SELECT_ROOT: Addr = 0x0869_74ad;
    /// "The selected item cannot be assigned to a path."
    pub const CANNOT_ASSIGN: Addr = 0x0869_74e0;
    /// "The selected item already has explicit translation"
    pub const HAS_EXPLICIT: Addr = 0x0869_7520;
    /// "Cancel", "Ok".
    pub const CANCEL: Addr = 0x0869_7553;
    pub const OK: Addr = 0x0869_755a;
    /// "unless deactivated. Put on path anyway?"
    pub const ANYWAY: Addr = 0x0869_7560;
    /// "some item. Constraint will override path"
    pub const OVERRIDE: Addr = 0x0869_75a0;
    /// "The selected item is already constrained to "
    pub const CONSTRAINED: Addr = 0x0869_75e0;
    /// "No path translation exists for the selected item."
    pub const NO_PATH: Addr = 0x0869_7620;
    /// "PATH", twice: the name stem of a new action.
    pub const ACTION_NAME_1: Addr = 0x0869_7652;
    pub const ACTION_NAME_2: Addr = 0x0869_7657;
}

/// 1e-05 (the doubles at 0x0864d940, 0x0864d970, 0x0864db78, 0x0864db80): "the same point",
/// "the same time", "no length".
const SMALL: f64 = 1e-05;
/// 0.01 (0x0864d948, 0x0864d978): percent to fraction.
const PERCENT: f64 = 0.01;
/// 100.0 (0x0864d960, 0x0864d980, 0x0864db68, 0x0864db70).
const HUNDRED: f64 = 100.0;

crate::host_trait! {
    /// What `motionPath.c` calls outside this package, beyond the hosts it builds on.
    pub trait PathHost: TransfoHost, ResetHost {
        /// `SI_splineAllocate()` (spline.c).
        fn spline_allocate() -> Addr;
        /// `SI_splineUpdateMatrix(spline)` (spline.c).
        fn spline_update_matrix(spline: Addr);
        /// `SI_allDummy(spline)` (spline.c).
        fn all_dummy(spline: Addr);
        /// `SI_modelAllocate(name)` (model.c).
        fn model_allocate(name: Addr) -> Addr;
        /// `SI_modelAddDefinition(model, kind, definition)` (model.c).
        fn model_add_definition(mdl: Addr, kind: i32, def: Addr);
        /// `SI_graflistAddData(graf list, object, graf kind, 0)`.
        fn graflist_add_data(list: Addr, obj: Addr, kind: i32, flag: i32) -> Addr;
        /// `SI_splineAddPoint(spline, index, &point)` (spline.c): a new point before `index`.
        fn spline_add_point(spline: Addr, index: i32, pt: Addr);
        /// `SI_bezierMovePoint(spline, index, &point)` (spline.c).
        fn bezier_move_point(spline: Addr, index: i32, pt: Addr);
        /// `SI_splineChangePoint(spline, index, &point)` (spline.c).
        fn spline_change_point(spline: Addr, index: i32, pt: Addr);
        /// `SI_splineFactorToDistance(spline, factor)` (spline.c).
        fn spline_factor_to_distance(spline: Addr, factor: f32) -> F80;
        /// `SI_splineUpdateLength(spline, samples, flag)` (spline.c): fills the length table.
        fn spline_update_length(spline: Addr, samples: i32, flag: i32);
        /// `SI_splineNbSegment(spline)` (spline.c).
        fn spline_nb_segment(spline: Addr) -> i32;
        /// `SI_nurbsCurveUpdateLength(curve, samples, flag)` (nrb_geometry.c).
        fn nurbs_curve_update_length(crv: Addr, samples: i32, flag: i32);
        /// `SI_nurbsCrvIndexHit(curve, &index, &u, 1)`: the control point at a parameter.
        fn nurbs_crv_index_hit(crv: Addr, index: Addr, u: Addr, flag: i32);
        /// `SI_nurbsCrvAddPoint(curve, index, u, x, y, z, w)`: five doubles.
        fn nurbs_crv_add_point(crv: Addr, index: i32, u: f64, x: f64, y: f64, z: f64, w: f64);
        /// `SI_nurbsFactorToDistance(curve, u)` (nrb_geometry.c); a `double` argument.
        fn nurbs_factor_to_distance(crv: Addr, u: f64) -> F80;
        /// `SI_dbGrafType2Chapter(graf kind)`: the chapter of that kind of object.
        fn db_graf_type2chapter(kind: i32) -> i32;
        /// `SI_elemdictAddRelation(dict, reldict, from, to, relation, list, 0)`.
        fn elemdict_add_relation(dict: Addr, reldict: Addr, from: i32, to: i32, relid: i32, group: i32, flag: i32);
        /// `SI_elemdictGetRelation(dict, elem, relation, chapter, n)`: the n-th entry of that
        /// relation to that chapter.
        fn elemdict_get_relation(dict: Addr, e: Addr, relid: i32, chap: i32, nth: i32) -> Addr;
        /// `SI_edGetElemFromWave(wave, &index)` (edtools.c).
        fn ed_get_elem_from_wave(wave: Addr, index: Addr) -> Addr;
        /// `si_setFcvTableFromMenu(menu entry, sub entry)`: raises the "selected" word of the
        /// attributes a menu entry stands for.
        fn set_fcv_table_from_menu(entry: i32, sub: i32);
        /// `si_getDicInfoFromGrafnode(graf node, entry, sub, &chapter, &index)`: the element
        /// of a graf node.
        fn get_dic_info_from_grafnode(gn: Addr, entry: i32, sub: i32, chap: Addr, index: Addr) -> Addr;
        /// `SI_reldictCheckRelAnim(reldict, relation, &out, chapter ids)`: whether curves are
        /// filed under the relation.
        fn reldict_check_rel_anim(reldict: Addr, relid: i32, out: Addr, cids: Addr) -> i16;
        /// `SI_attdictSetFcurveFlag(attribute dictionary, curve type, 1)`.
        fn attdict_set_fcurve_flag(atd: Addr, kind: i32, on: i32);
        /// `SI_unselectGrafnode(graf list, graf node)`.
        fn unselect_grafnode(list: Addr, gn: Addr);
        /// `SI_nextGrafnode(graf list, &graf node)`: steps the pointer to the next node, null
        /// at the end.
        fn next_grafnode(list: Addr, gn: Addr);
        /// `SI_edElementSetConstraint(object, graf kind, model, 2, relation, 0)`.
        fn ed_element_set_constraint(obj: Addr, kind: i32, target: Addr, a: i32, relid: i32, b: i32);
        /// `si_relationActiveRangeSetKey(relation entry, elem, chapter, index, from, to, on)`
        /// (motionKey.c, not ported): the entry's activation curve is switched over a range.
        fn relation_active_range_set_key(rel: Addr, e: Addr, chap: i32, index: i32, from: f32, to: f32, on: i32);
        /// `si_setExplicitPathTiming(&start frame)` (a dialogue).
        fn set_explicit_path_timing(start: Addr);
        /// `si_pathTimeDialogBox(&start, &end, &keep, new)` (a dialogue): 0 when cancelled.
        fn path_time_dialog_box(start: Addr, end: Addr, keep: Addr, new: i32) -> i16;
        /// `si_pathActionDialogBox(&start, &end, &keep, new, animation, &action)`: 0
        /// cancelled, 1 a new action, else an existing one.
        fn path_action_dialog_box(start: Addr, end: Addr, keep: Addr, new: i32, anim: Addr, action: Addr) -> i32;
        /// `SI_elemdictRemoveUncompleteRelPair(dict, reldict, chapter, chapter 2, index,
        /// index 2, relation, list, 0, −1)`.
        fn elemdict_remove_uncomplete_rel_pair(dict: Addr, reldict: Addr, chap: i32, chap2: i32, index: i32, index2: i32,
                                               relid: i32, group: i32, a: i32, b: i32);
        /// `SI_actionsGetNewName(actions, stem)`.
        fn actions_get_new_name(actions: Addr, stem: Addr) -> Addr;
        /// `SI_actionsAddElem(actions, name, start, end, 1, 2)`: the new action's number.
        fn actions_add_elem(actions: Addr, name: Addr, start: i32, end: i32, a: i32, b: i32) -> i32;
        /// `SI_actionsSecToStartFrm(seconds)`, `SI_actionsSecToEndFrm(seconds)` (actions.c).
        fn actions_sec_to_start_frm(sec: f32) -> i32;
        fn actions_sec_to_end_frm(sec: f32) -> i32;
        /// `SI_edGetActions(chapter, index)`.
        fn ed_get_actions(chap: i32, index: i32) -> Addr;
        /// `SI_elemdictGetElemFromData(dict, chapter, object, &index)`.
        fn elemdict_get_elem_from_data(dict: Addr, chap: i32, obj: Addr, index: Addr) -> Addr;
    }
}

fn status<H: KeyHost + ?Sized>(m: &mut Mem, h: &mut H, text: Addr) {
    KeyHost::status_display_message(h, m, text, 3);
}

fn key_at(m: &Mem, curve: Addr, i: i32) -> Addr {
    m.ptr(curve + fo::KEYS).wrapping_add((i as u32).wrapping_mul(ko::SIZE))
}

fn is_path_key(m: &Mem, k: Addr) -> bool {
    m.u16(k + ko::FLAGS) & keyflag::TYPE_MASK == keyflag::PATH_KEY
}

fn make_path_key(m: &mut Mem, k: Addr) {
    let f = m.u16(k + ko::FLAGS);
    m.set_u16(k + ko::FLAGS, (f & keyflag::KEPT) | keyflag::PATH_KEY);
}

/// `(float)frame / SI_frameRate`, unrounded in the register: the compiler's conversion of an
/// integer to `float` (through a store) and the division.
fn frame_sec(m: &Mem, frame: i32) -> F80 {
    F80::from_f32(F80::from_i32(frame).to_f32()) / glob::frame_rate(m)
}

/// `|a − b|` of two floats, in the register.
fn dist(a: f32, b: f32) -> F80 {
    (F80::from_f32(a) - b).abs()
}

/// A new path: a spline of the given type with tension 0.5 as the definition (kind 6) of a
/// new model named "path", put on screen [soft 0x0821d750 SI_pathNew]. Returns the model.
pub fn path_new<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, kind: i32) -> Addr {
    let spl = h.spline_allocate(m);
    m.set_f32(spl + spline::TENSION, 0.5);
    m.set_i32(spl + spline::KIND, kind);
    h.spline_update_matrix(m, spl);
    h.all_dummy(m, spl);
    let mdl = h.model_allocate(m, text::PATH_NAME);
    h.model_add_definition(m, mdl, 6, spl);
    let gl = glob::cur_graf_list(m);
    h.graflist_add_data(m, gl, mdl, graf::MODEL, 0);
    mdl
}

/// The curve as `si-fcurve` sees it, evaluated at a frame the way
/// `SI_fcurveFrameValue(curve, frame)` does, with the evaluation state written back.
fn frame_value(m: &mut Mem, curve: Addr, frame: i32) -> F80 {
    let mut env = fcv::env(m);
    let mut f = fcv::load(m, curve);
    match f.frame_value(frame, &mut env) {
        Ok(v) => {
            m.set_f32(curve + fo::LAST_TIME, f.last_time);
            m.set_f32(curve + fo::LAST_VALUE, f.last_value);
            m.set_i32(curve + fo::CACHE, f.cache);
            fcv::f80(v)
        }
        Err(_) => F80::ZERO,
    }
}

/// What the two "add a path key" routines share once the path's curve has its new point:
/// the key of the percentage curve and the key of the scratch curve are inserted (unless a
/// point was overwritten instead), with the percentage key made a path key when `keyed` is 1.
///
/// Both curves are records of the image and are written back here, in the original's order,
/// so that the blocks lie as the original's do: `SI_fcurveAddKey` on the percentage curve
/// (a first key's new array, or the array grown), the key's own block freed, then the same
/// for the scratch curve. `rec_a` and `rec_b` are the two keys' blocks ([`fcv::key_record`]).
#[allow(clippy::too_many_arguments)]
fn insert_keys(m: &mut Mem, env: &mut si_fcurve::Env, curve: Addr, f: &mut Fcurve, tmp_at: Addr, tmp: Option<&mut Fcurve>, mut key_a: Key,
               key_b: Key, rec: (Addr, Addr), keyed: i16, add: bool, hit: i32)
               -> bool {
    let mut marked = false;
    if keyed == 1 {
        key_a.spline_index = if add {
            (hit as i16).wrapping_add(1)
        } else {
            // the point number of the curve's last key
            f.k().last().map_or(0, |k| k.spline_index)
        };
        key_a.flags = (key_a.flags & keyflag::KEPT) | keyflag::PATH_KEY;
        marked = true;
        // the original changes the key in its block
        if rec.0 != 0 {
            fcv::store_key(m, rec.0, &key_a, 0);
        }
    }
    if m.i32(curve + fo::NB_KEYS) == 0 {
        // a first key goes into a new array whatever pointer the curve holds ([`fcv::add_key`])
        m.set_ptr(curve + fo::KEYS, 0);
    }
    f.add_key(key_a, env);
    fcv::store(m, curve, f);
    m.free(rec.0);
    // with no scratch curve (the percentage curve had no key) the original hands a null
    // curve to SI_fcurveAddKey, which frees the key and returns
    if let Some(t) = tmp {
        t.add_key(key_b, env);
        fcv::store(m, tmp_at, t);
    }
    m.free(rec.1);
    marked
}

/// A key on the path of a spline: the object is at `pt` (in the path's coordinates) at the
/// current frame [soft 0x0821d7cc SI_pathAddKeyPct].
///
/// The percentage curve is first rewritten as "time → spline parameter" in a scratch copy
/// (each key value v becomes `SI_splineDistanceToFactor(length × 0.01 × v)`), because
/// adding a point changes the length and so every percentage, but leaves the parameter of
/// the old points alone up to a shift by one.
///
/// - **No key yet**: the point is appended (index: point count − phantom count), and both
///   curves get a key of value 0 at the current frame.
/// - **Before the first key**: the point is inserted in front (index: the phantom count),
///   every parameter above −1 moves up by 1, and the new key stands for parameter 0. The
///   percentage key's value is not set by the original on this path (an uninitialised
///   local; 0 here): it is overwritten by the conversion back below.
/// - **Otherwise**: the percentage at the current frame (100 when the curve has one key)
///   gives the parameter where the object is now; the point index is that parameter + 0.001
///   truncated. If a key sits on this frame and stands exactly on that point, the point is
///   moved instead ([`path_overwrite`]) and no key is added. Else the point is inserted
///   after it and the parameters above move up by 1; the new key stands for index + 1.
/// - A point within 1e-05 of the path's last point in all three coordinates is not added
///   again; only the keys are.
///
/// With `keyed` = 1 the percentage key becomes a path key of the new point (of the last
/// key's point when no point was added) and the spline is marked keyframed. Finally the
/// scratch curve is converted back, percentage = `SI_splineFactorToDistance(parameter) ×
/// 100 / length` (0 for a length not above 1e-05), the slopes are recomputed, and the path
/// keys are brought to their points ([`update_fcurve_path_keys`]).
pub fn path_add_key_pct<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, spl: Addr, curve: Addr, pt: Addr, keyed: i16) {
    let mut o = fcv::Ops::new(m);
    // the scratch curve is a record of the image like any other: the original allocates it
    let mut tmp_at = 0;
    let frame = m.i32(glob::SI_FRAME_CURRENT);
    let mut hit: i16 = -1;
    let mut add = true;
    let mut overwrote = false;
    let mut tmp: Option<Fcurve> = None;
    let key_a;
    let rec_a;
    let value_b: f32;
    let interp_b: i16;
    let mut f;
    if m.i32(curve + fo::NB_KEYS) < 1 {
        let at = m.u16(spl + spline::NB_POINTS) as i32 - m.i16(spl + spline::PHANTOM) as i32;
        h.spline_add_point(m, spl, at, pt);
        f = fcv::load(m, curve);
        key_a = Key::frame_create(frame, 0.0, 0.0, 0, f.interp, &o.env);
        rec_a = fcv::key_record(m, &key_a);
        interp_b = f.interp;
        value_b = 0.0;
    } else {
        let phantoms = (m.i32(spl + spline::KIND).wrapping_sub(2) as u32) < 2;
        let last = m.u16(spl + spline::NB_POINTS) as i32 - if phantoms { 2 } else { 1 };
        if last >= 0 {
            let p = m.ptr(spl + spline::POINTS).wrapping_add((last as u32).wrapping_mul(spline::POINT_SIZE)) + spline::POINT_POS;
            let small = F80::from_f64(SMALL);
            if small.gt(dist(m.f32(p), m.f32(pt))) && dist(m.f32(p + 4), m.f32(pt + 4)).lt(small) && dist(m.f32(p + 8), m.f32(pt + 8)).lt(small) {
                add = false;
            }
        }
        let len = h.spline_length(m, spl).to_f32();
        f = fcv::load(m, curve);
        tmp_at = key::fcurve_allocate(m, f.kind, 0);
        let mut t = Fcurve::allocate(f.kind, 0, &o.env);
        t.transfer_from(&f, &mut o.env);
        // `SI_fcurveTransfer` gives the scratch curve its own key array
        fcv::store(m, tmp_at, &t);
        o.flush(m, h);
        let n = f.nkeys();
        for i in 0..n as usize {
            let d = ((F80::from_f32(f.k()[i].value) * F80::from_f64(PERCENT)) * len).to_f32();
            let fac = h.spline_distance_to_factor(m, spl, d).to_f32();
            if let Some(k) = t.keys.as_mut().and_then(|k| k.get_mut(i)) {
                k.value = fac;
            }
        }
        // the percentage key's value: not set on the "before the first key" path
        let mut pct = 0.0f32;
        if frame < f.k()[0].frame(&o.env) {
            hit = -1;
            if add {
                let at = m.i16(spl + spline::PHANTOM) as i32;
                h.spline_add_point(m, spl, at, pt);
                t.inc_value_above(-1.0, 1.0, &mut o.env);
                o.flush(m, h);
            }
        } else {
            pct = if n > 1 { frame_value(m, curve, frame).to_f32() } else { 100.0 };
            if n > 1 {
                // the evaluation left its state in the curve
                f.last_time = m.f32(curve + fo::LAST_TIME);
                f.last_value = m.f32(curve + fo::LAST_VALUE);
                f.cache = m.i32(curve + fo::CACHE);
            }
            let d = ((F80::from_f64(PERCENT) * pct) * len).to_f32();
            let fac = h.spline_distance_to_factor(m, spl, d);
            hit = (F80::from_f64(0.001) + fac).to_i32(Round::Zero) as i16;
            if add {
                if path_overwrite(m, h, spl, curve, fac.to_f32(), hit, pt) != 0 {
                    overwrote = true;
                } else {
                    let at = hit as i32 + if phantoms { 2 } else { 1 };
                    h.spline_add_point(m, spl, at, pt);
                    t.inc_value_above(F80::from_i32(hit as i32).to_f32(), 1.0, &mut o.env);
                    o.flush(m, h);
                }
            }
        }
        key_a = Key::frame_create(frame, pct, 0.0, 0, f.interp, &o.env);
        rec_a = fcv::key_record(m, &key_a);
        interp_b = t.interp;
        value_b = F80::from_i32(hit as i32 + 1).to_f32();
        tmp = Some(t);
    }
    let key_b = Key::frame_create(frame, value_b, 0.0, 0, interp_b, &o.env);
    let rec_b = fcv::key_record(m, &key_b);
    if overwrote {
        // the point was moved instead: the two keys are freed unused
        m.free(rec_a);
        m.free(rec_b);
    } else if insert_keys(m, &mut o.env, curve, &mut f, tmp_at, tmp.as_mut(), key_a, key_b, (rec_a, rec_b), keyed, add, hit as i32) {
        m.set_u16(spl + spline::KEYFRAMED, 1);
    }
    // the scratch curve as the original has it in memory (its values are parameters now)
    if let Some(t) = &tmp {
        fcv::store(m, tmp_at, t);
    }
    o.flush(m, h);
    // the scratch curve's mark of a changed curve: `SI_fcurveRemove` takes a marked curve
    // off the counter again
    let tmp_dirty = tmp.as_ref().map_or(0, |t| t.dirty);
    if let Some(t) = tmp {
        let len = h.spline_length(m, spl);
        for i in 0..t.nkeys() as usize {
            let v = if len.gt(F80::from_f64(SMALL)) || len.unordered(F80::from_f64(SMALL)) {
                let d = h.spline_factor_to_distance(m, spl, fmov(t.k()[i].value));
                ((d * F80::from_f64(HUNDRED)) / len).to_f32()
            } else {
                0.0
            };
            // the original writes key i of the percentage curve whatever its count
            if let Some(k) = f.keys.as_mut().and_then(|k| k.get_mut(i)) {
                k.value = v;
            }
        }
        f.default_slope(&mut o.env);
    }
    fcv::store(m, curve, &f);
    o.flush(m, h);
    if tmp_at != 0 {
        m.set_i16(tmp_at + fo::DIRTY, tmp_dirty);
    }
    reset::fcurve_remove(m, h, tmp_at);
    if m.i16(spl + spline::KEYFRAMED) == 1 {
        update_fcurve_path_keys(m, h, curve, spl);
    }
}

/// The same for a path that is a NURBS curve [soft 0x0821dc98 SI_nurbsPathAddKeyPct].
///
/// The scratch curve holds "time → curve parameter u" (`SI_nurbsDistanceToFactor`, with
/// the length and the products as `double`s). A new point goes in at parameter u with
/// weight 1; the parameters above u then move up by the amount the parameter range grew
/// (range end after − before, rounded to `float`).
///
/// - **No key yet**: the point is added at (range end − 1e-05), at the control point index
///   `SI_nurbsCrvIndexHit` gives for that parameter; both curves get a key of value 0.
/// - **Before the first key**: added at index −1, parameter (range start + 1e-05); the
///   scratch values above that parameter move up. Both keys' values are not set by the
///   original on this path (uninitialised locals; 0 here).
/// - **Otherwise**: u = `SI_nurbsDistanceToFactor(percentage × 0.01 × length)` (percentage
///   100 for a curve with one key); the control point there is overwritten when a key sits
///   on this frame ([`nurbs_path_overwrite`]), else a point is added at u.
///
/// The closing conversion back is percentage = `SI_nurbsFactorToDistance(u) × 100 /
/// length`, in `double`s, 0 for a length not above 1e-05.
pub fn nurbs_path_add_key_pct<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, crv: Addr, curve: Addr, pt: Addr, keyed: i16) {
    let mut o = fcv::Ops::new(m);
    // the scratch curve is a record of the image like any other: the original allocates it
    let mut tmp_at = 0;
    let frame = m.i32(glob::SI_FRAME_CURRENT);
    let small = F80::from_f64(SMALL);
    let hit_at = m.local(4);
    let u_at = m.local(8);
    m.set_i32(hit_at, -1);
    let mut add = true;
    let mut overwrote = false;
    let mut tmp: Option<Fcurve> = None;
    let key_a;
    let key_b;
    let (rec_a, rec_b);
    let mut f;
    let p = |m: &Mem, k: u32| m.f32(pt + 4 * k) as f64;
    if m.i32(curve + fo::NB_KEYS) < 1 {
        let u = (F80::from_f64(m.f64(crv + nurbs::RANGE_END)) - small).to_f64();
        m.set_f64(u_at, u);
        h.nurbs_crv_index_hit(m, crv, hit_at, u_at, 1);
        // the parameter is computed again, with the constant kept in the register
        let u2 = (F80::from_f64(m.f64(crv + nurbs::RANGE_END)) - small).to_f64();
        let at = m.i32(hit_at);
        let (x, y, z) = (p(m, 0), p(m, 1), p(m, 2));
        h.nurbs_crv_add_point(m, crv, at, u2, x, y, z, 1.0);
        h.nurbs_curve_update_length(m, crv, 100, 1);
        f = fcv::load(m, curve);
        key_a = Key::frame_create(frame, 0.0, 0.0, 0, f.interp, &o.env);
        rec_a = fcv::key_record(m, &key_a);
        key_b = Key::frame_create(frame, 0.0, 0.0, 0, f.interp, &o.env);
        rec_b = fcv::key_record(m, &key_b);
        m.set_i32(hit_at, -1);
    } else {
        let last = m.i32(crv + nurbs::NB_POINTS) - 1;
        if last >= 0 {
            let q = m.ptr(crv + nurbs::POINTS).wrapping_add((last as u32).wrapping_mul(nurbs::POINT_SIZE));
            let d = |m: &Mem, k: u32| (F80::from_f64(m.f64(q + 8 * k)) - m.f32(pt + 4 * k)).abs();
            if small.gt(d(m, 0)) && d(m, 1).lt(small) && d(m, 2).lt(small) {
                add = false;
            }
        }
        h.nurbs_curve_update_length(m, crv, 100, 1);
        let len = h.nurbs_curve_length(m, crv).to_f64();
        f = fcv::load(m, curve);
        tmp_at = key::fcurve_allocate(m, f.kind, 0);
        let mut t = Fcurve::allocate(f.kind, 0, &o.env);
        t.transfer_from(&f, &mut o.env);
        // `SI_fcurveTransfer` gives the scratch curve its own key array
        fcv::store(m, tmp_at, &t);
        o.flush(m, h);
        let n = f.nkeys();
        for i in 0..n as usize {
            let d = ((F80::from_f32(f.k()[i].value) * F80::from_f64(PERCENT)) * len).to_f64();
            let u = h.nurbs_distance_to_factor(m, crv, d);
            m.set_f64(u_at, u.to_f64());
            if let Some(k) = t.keys.as_mut().and_then(|k| k.get_mut(i)) {
                k.value = u.to_f32();
            }
        }
        // the percentage: not set on the "before the first key" path
        let mut pct = 0.0f64;
        let mut shift: Option<(f32, f32)> = None;
        if frame < f.k()[0].frame(&o.env) {
            m.set_i32(hit_at, -1);
            if add {
                let end = m.f64(crv + nurbs::RANGE_END);
                let u = (small + m.f64(crv + nurbs::RANGE_START)).to_f64();
                let (x, y, z) = (p(m, 0), p(m, 1), p(m, 2));
                h.nurbs_crv_add_point(m, crv, -1, u, x, y, z, 1.0);
                h.nurbs_curve_update_length(m, crv, 100, 1);
                let by = (F80::from_f64(m.f64(crv + nurbs::RANGE_END)) - end).to_f32();
                let above = (small + m.f64(crv + nurbs::RANGE_START)).to_f32();
                shift = Some((above, by));
            }
        } else {
            pct = if n > 1 { frame_value(m, curve, frame).to_f64() } else { 100.0 };
            if n > 1 {
                f.last_time = m.f32(curve + fo::LAST_TIME);
                f.last_value = m.f32(curve + fo::LAST_VALUE);
                f.cache = m.i32(curve + fo::CACHE);
            }
            let d = ((F80::from_f64(pct) * F80::from_f64(PERCENT)) * len).to_f64();
            let u = h.nurbs_distance_to_factor(m, crv, d).to_f64();
            m.set_f64(u_at, u);
            h.nurbs_crv_index_hit(m, crv, hit_at, u_at, 1);
            if add {
                let (u, at) = (m.f64(u_at), m.i32(hit_at));
                if nurbs_path_overwrite(m, h, crv, curve, u, at, pt) != 0 {
                    overwrote = true;
                } else {
                    h.nurbs_crv_index_hit(m, crv, hit_at, u_at, 1);
                    let end = m.f64(crv + nurbs::RANGE_END);
                    let (u, at) = (m.f64(u_at), m.i32(hit_at));
                    let (x, y, z) = (p(m, 0), p(m, 1), p(m, 2));
                    h.nurbs_crv_add_point(m, crv, at, u, x, y, z, 1.0);
                    h.nurbs_curve_update_length(m, crv, 100, 1);
                    let by = (F80::from_f64(m.f64(crv + nurbs::RANGE_END)) - end).to_f32();
                    let above = F80::from_f64(m.f64(u_at)).to_f32();
                    shift = Some((above, by));
                }
            }
        }
        if let Some((above, by)) = shift {
            t.inc_value_above(above, by, &mut o.env);
            o.flush(m, h);
        }
        key_a = Key::frame_create(frame, F80::from_f64(pct).to_f32(), 0.0, 0, f.interp, &o.env);
        rec_a = fcv::key_record(m, &key_a);
        key_b = Key::frame_create(frame, F80::from_f64(m.f64(u_at)).to_f32(), 0.0, 0, t.interp, &o.env);
        rec_b = fcv::key_record(m, &key_b);
        tmp = Some(t);
    }
    let hit = m.i32(hit_at);
    if overwrote {
        m.free(rec_a);
        m.free(rec_b);
    } else if insert_keys(m, &mut o.env, curve, &mut f, tmp_at, tmp.as_mut(), key_a, key_b, (rec_a, rec_b), keyed, add, hit) {
        m.set_u16(crv + nurbs::KEYFRAMED, 1);
    }
    if let Some(t) = &tmp {
        fcv::store(m, tmp_at, t);
    }
    o.flush(m, h);
    let tmp_dirty = tmp.as_ref().map_or(0, |t| t.dirty);
    if let Some(t) = tmp {
        let len = h.nurbs_curve_length(m, crv).to_f64();
        for i in 0..t.nkeys() as usize {
            let l = F80::from_f64(len);
            let v = if l.gt(small) || l.unordered(small) {
                let d = h.nurbs_factor_to_distance(m, crv, t.k()[i].value as f64);
                F80::from_f64(((d * F80::from_f64(HUNDRED)) / len).to_f64()).to_f32()
            } else {
                0.0
            };
            if let Some(k) = f.keys.as_mut().and_then(|k| k.get_mut(i)) {
                k.value = v;
            }
        }
        f.default_slope(&mut o.env);
    }
    fcv::store(m, curve, &f);
    o.flush(m, h);
    if tmp_at != 0 {
        m.set_i16(tmp_at + fo::DIRTY, tmp_dirty);
    }
    reset::fcurve_remove(m, h, tmp_at);
    m.release(8);
    m.release(4);
    if m.i16(crv + nurbs::KEYFRAMED) == 1 {
        update_fcurve_nurbs_path_keys(m, h, curve, crv);
    }
}

/// A null that travels a path over a frame range, for an action
/// [soft 0x0821e268 SI_actionCreateNullOnPath]: a new model named `PATH_<name of the path
/// model>` is put on screen and given a percentage curve (type 7) with the keys (start
/// frame, 0) and (end frame, 100), filed under the MODELS chapter, and the path relation to
/// the path model. The null's element index is written through `index`. Returns the null.
pub fn action_create_null_on_path<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, path: Addr, start: i32, end: i32, index: Addr) -> Addr {
    let name = m.cstr(m.ptr(path + model::NAME));
    let buf = m.si_calloc(name.len() as u32 + 6, 1);
    let mut s = b"PATH_".to_vec();
    s.extend_from_slice(&name);
    s.push(0);
    m.set_bytes(buf, &s);
    let mdl = h.model_allocate(m, buf);
    if buf != 0 {
        m.free(buf);
    }
    let gl = glob::cur_graf_list(m);
    h.graflist_add_data(m, gl, mdl, graf::MODEL, 0);
    let (e, i) = ed::get_elem_from_model(m, h, mdl);
    m.set_i32(index, i);
    let c = key::fcurve_allocate(m, 7, mdl);
    add_frame_key(m, h, c, start, 0.0);
    add_frame_key(m, h, c, end, 100.0);
    let models = glob::chap(m, cid::MODELS);
    let i = m.i32(index);
    h.ed_fcv_add(m, c, e, models, i);
    path_element_set(m, h, mdl, graf::MODEL, path);
    mdl
}

/// `SI_fkeyFrameCreate(frame, value, 0.0, 0, curve's interpolation)` and `SI_fcurveAddKey`.
fn add_frame_key<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, curve: Addr, frame: i32, value: f32) {
    let k = Key::frame_create(frame, value, 0.0, 0, m.i16(curve + fo::INTERP), &fcv::env(m));
    fcv::add_key(m, h, curve, k);
}

/// Constrains an element to a null for the frames `start ..= end` only
/// [soft 0x0821e3c0 SI_actionCnsToNull].
///
/// Unless `keep` is set, the constraint `relid` to `target` is created
/// (`SI_edElementSetConstraint(object, kind, target, 2, relid, 0)`). The element's relation
/// of that id to the MODELS element `target_index` is then looked up (the n-th such
/// relation, by `SI_elemdictGetRelation`) and its activation is keyed three times: **off**
/// from (first frame − 500) to `start`, **off** from `end` to (last frame + 500), **on**
/// from `start` to `end`; each time as `(float)frame / rate`.
#[allow(clippy::too_many_arguments)]
pub fn action_cns_to_null<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, chap: i32, index: i32, kind: i32, target: Addr,
                                               target_index: i32, relid: i32, start: i32, end: i32, keep: i16) {
    let d = glob::cur_dict(m);
    let e = ed::get_elem(m, d, chap, index);
    if keep == 0 {
        let obj = m.ptr(e + elem::DATA);
        h.ed_element_set_constraint(m, obj, kind, target, 2, relid as i16 as i32, 0);
    }
    let models = glob::chap(m, cid::MODELS);
    let d = glob::cur_dict(m);
    let mut r = ed::find_related_elem(m, d, e, true, 1, relid, models);
    let mut nth = 0;
    loop {
        if r.elem == 0 {
            return;
        }
        if r.index == target_index {
            break;
        }
        let d = glob::cur_dict(m);
        r = ed::find_next_related_elem(m, d, e, true, 1, relid, models);
        nth += 1;
    }
    let d = glob::cur_dict(m);
    let rel = h.elemdict_get_relation(m, d, e, relid, models, nth);
    if rel == 0 {
        return;
    }
    let sec = |m: &Mem, f: i32| frame_sec(m, f).to_f32();
    let (a, b) = (sec(m, m.i32(glob::SI_FRAME_START).wrapping_sub(500)), sec(m, start));
    h.relation_active_range_set_key(m, rel, e, chap, index, a, b, 0);
    let (a, b) = (sec(m, end), sec(m, m.i32(glob::SI_FRAME_END).wrapping_add(500)));
    h.relation_active_range_set_key(m, rel, e, chap, index, a, b, 0);
    let (a, b) = (sec(m, start), sec(m, end));
    h.relation_active_range_set_key(m, rel, e, chap, index, a, b, 1);
}

/// What the path commands need to know of an object by its graf kind.
struct Kind {
    e: Addr,
    index: i32,
    chap: i32,
    /// The path relation, the explicit translation relation, the position constraint.
    rel: i32,
    trans_rel: i32,
    cns_rel: i32,
    /// Type of the percentage curve.
    curve: i32,
    /// Where the object keeps its path model.
    slot: Addr,
}

/// Puts an object on a path, or changes the timing of the path it is on: the command
/// behind "Path → Pick path" and "Path → Set timing" [soft 0x0821e790 si_itemSetPathTiming].
///
/// `set` = 1: `path` becomes the object's path; 0: the object's present path is retimed.
/// `action` ≠ 0: the path becomes an **action** (a null travels the path and the object is
/// constrained to the null for the action's frames) instead of a percentage curve.
///
/// 1. By graf kind (2, 3 model; 4, 5 camera position, interest; 6, 8, 0x13 light; 7 light
///    interest; 0xb wave, whose icon model holds the path) the element, chapter, relations,
///    curve type and path slot are chosen. As an action, a model must be a root ("Select a
///    root model."). Other kinds: "The selected item cannot be assigned to a path."
/// 2. An object with explicit translation curves is refused.
/// 3. Not as an action: if the object is position-constrained at the current frame, the
///    user is asked whether to go on ("… Constraint will override path unless deactivated.
///    Put on path anyway?"; a non-zero answer stops).
/// 4. A model that has a trajectory (relation 160 to a model in list 1) gets the explicit
///    path timing dialogue on the relation's data (a start frame, created as
///    `SI_frameStart`), and nothing else.
/// 5. The present timing is read from the percentage curve ([`get_path_timing`]).
/// 6. Not as an action: the timing dialogue; then a missing curve is made with the keys
///    (start, 0) and (end, 100) and filed; an existing one is either stretched to the new
///    range when the dialogue's "keep" word is 0 and it has more than one key
///    (`SI_fcurveOffsetAndScale(curve, t₀, t₀, t₁, start/rate − t₀, (end/rate − start/rate)
///    / (t₁ − t₀))`, scale 1 for a curve shorter than 1e-06 s) or replaced by the two keys.
///    With `set`, the half-relations left from the old path are removed, the slot is set
///    and the relations made ([`path_element_set`]).
/// 7. As an action: the action dialogue (new action, or an existing one whose range is
///    then used); [`action_create_null_on_path`], [`action_cns_to_null`]; the action's
///    record gets a 0x1c-byte block holding the path model's and the null's element indices.
pub fn item_set_path_timing<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, obj: Addr, kind: i32, path: Addr, set: i16, action: i16) {
    let (mut spl, mut crv) = (0, 0);
    if obj == 0 {
        return;
    }
    if set == 1 {
        if m.i32(path + model::KIND) == 6 {
            spl = m.ptr(path + model::DEFINITION);
        } else {
            crv = m.ptr(path + model::DEFINITION);
        }
    }
    let idx = m.local(4);
    let models = glob::chap(m, cid::MODELS);
    let cameras = glob::chap(m, cid::CAMERAS);
    let lights = glob::chap(m, cid::LIGHTS);
    let k = match kind {
        2 | 3 => {
            if action != 0 && m.ptr(obj + model::FATHER) != 0 {
                status(m, h, text::SELECT_ROOT);
                m.release(4);
                return;
            }
            let (e, i) = ed::get_elem_from_model(m, h, obj);
            m.set_i32(idx, i);
            Kind { e, index: 0, chap: models, rel: 0x70, trans_rel: 0x71, cns_rel: 0x6f, curve: 7, slot: obj + pathslot::MODEL }
        }
        4 => {
            let e = h.ed_get_elem_from_camera(m, obj, idx);
            Kind { e, index: 0, chap: cameras, rel: 0x458, trans_rel: 0x459, cns_rel: 0x457, curve: 9, slot: obj + pathslot::CAMERA_POSITION }
        }
        5 => {
            let e = h.ed_get_elem_from_camera(m, obj, idx);
            Kind { e, index: 0, chap: cameras, rel: 0x45a, trans_rel: 0x45b, cns_rel: 0x45c, curve: 10, slot: obj + pathslot::CAMERA_INTEREST }
        }
        6 | 8 | 0x13 => {
            let e = h.ed_get_elem_from_light(m, obj, idx);
            Kind { e, index: 0, chap: lights, rel: 0x840, trans_rel: 0x841, cns_rel: 0x83f, curve: 0x1f, slot: obj + pathslot::LIGHT_POSITION }
        }
        7 => {
            let e = h.ed_get_elem_from_light(m, obj, idx);
            Kind { e, index: 0, chap: lights, rel: 0x842, trans_rel: 0x843, cns_rel: 0x844, curve: 0x20, slot: obj + pathslot::LIGHT_INTEREST }
        }
        0xb => {
            let icon = h.wave_get_icon_model(m, obj);
            if icon == 0 {
                m.release(4);
                return;
            }
            let e = h.ed_get_elem_from_wave(m, obj, idx);
            Kind { e, index: 0, chap: glob::chap(m, cid::WAVES), rel: 0x264, trans_rel: 0x265, cns_rel: 0x6f, curve: 0xac, slot: icon + pathslot::MODEL }
        }
        _ => {
            status(m, h, text::CANNOT_ASSIGN);
            m.release(4);
            return;
        }
    };
    let k = Kind { index: m.i32(idx), ..k };
    m.release(4);
    if k.e == 0 {
        return;
    }
    if ed::find_fcv(m, k.e, 1, k.trans_rel) != 0 {
        status(m, h, text::HAS_EXPLICIT);
        return;
    }
    let cchap = if kind == 0xb { models } else { k.chap };
    if action == 0 {
        let t = frame_sec(m, m.i32(glob::SI_FRAME_CURRENT)).to_f32();
        let d = glob::cur_dict(m);
        if h.ed_is_item_constrained(m, d, obj, cchap, k.cns_rel, 1, t) != 0
            && h.dialog_request2(m, text::CONSTRAINED, text::OVERRIDE, text::ANYWAY, text::OK, text::CANCEL) != 0
        {
            return;
        }
    }
    if k.chap == models {
        let d = glob::cur_dict(m);
        if ed::find_related_elem(m, d, k.e, true, 1, 0xa0, k.chap).elem != 0 {
            let rel = ed::get_cur_relation(m, k.e, true, 1);
            if m.ptr(rel + arel::DATA) == 0 {
                let p = m.si_malloc(4);
                let start = m.i32(glob::SI_FRAME_START);
                m.set_i32(p, start);
                m.set_ptr(rel + arel::DATA, p);
            }
            let p = m.ptr(rel + arel::DATA);
            h.set_explicit_path_timing(m, p);
            return;
        }
    }
    let mut curve = ed::find_fcv(m, k.e, 1, k.rel);
    let (start0, end0) = get_path_timing(m, curve);
    if set == 0 {
        let pm = m.ptr(k.slot);
        if pm == 0 {
            status(m, h, text::NO_PATH);
            return;
        }
        if m.i32(pm + model::KIND) == 6 {
            spl = m.ptr(pm + model::DEFINITION);
        } else {
            crv = m.ptr(pm + model::DEFINITION);
        }
    }
    if spl == 0 && crv == 0 {
        return;
    }
    // start, end, the dialogue's "keep" word, the chosen action, two element indices
    let l = m.local(24);
    let (start_at, end_at, keep_at, act_at, null_at, pidx_at) = (l, l + 4, l + 8, l + 12, l + 16, l + 20);
    m.set_i32(start_at, start0);
    m.set_i32(end_at, end0);
    if action == 0 {
        'cmd: {
            if h.path_time_dialog_box(m, start_at, end_at, keep_at, (curve == 0) as i32) == 0 {
                break 'cmd;
            }
            let (start, end) = (m.i32(start_at), m.i32(end_at));
            if curve == 0 {
                curve = key::fcurve_allocate(m, k.curve, obj);
                add_frame_key(m, h, curve, start, 0.0);
                add_frame_key(m, h, curve, end, 100.0);
                h.ed_fcv_add(m, curve, k.e, k.chap, k.index);
            } else {
                if m.i16(keep_at) == 0 && m.i32(curve + fo::NB_KEYS) > 1 {
                    let a = frame_sec(m, start);
                    let b = frame_sec(m, end);
                    let first = m.f32(key_at(m, curve, 0));
                    let last = m.f32(key_at(m, curve, m.i32(curve + fo::NB_KEYS) - 1));
                    let span = F80::from_f32(last) - first;
                    let scale = if span.abs().lt(F80::from_f64(1e-06)) { F80::ONE } else { (b - a) / span };
                    let offset = a - first;
                    let (offset, scale) = (offset.to_f32(), scale.to_f32());
                    fcv::Ops::on(m, h, curve, |f, env| f.offset_and_scale(fmov(first), fmov(first), fmov(last), offset, scale, env));
                } else {
                    fcv::Ops::on(m, h, curve, |f, env| f.remove_all_keys(env));
                    add_frame_key(m, h, curve, start, 0.0);
                    add_frame_key(m, h, curve, end, 100.0);
                }
                if set == 0 {
                    break 'cmd;
                }
                let old = m.ptr(k.slot);
                let (_, old_index) = ed::get_elem_from_model(m, h, old);
                if old_index > -1 {
                    for group in [1, 2] {
                        let d = glob::cur_dict(m);
                        h.elemdict_remove_uncomplete_rel_pair(m, d, glob::SI_RD, k.chap, models, k.index, old_index, k.rel, group, 0, -1);
                    }
                }
            }
            if set == 0 {
                break 'cmd;
            }
            m.set_ptr(k.slot, path);
            path_element_set(m, h, obj, kind, path);
        }
        m.release(24);
        return;
    }
    'act: {
        let d = glob::cur_dict(m);
        let ani = glob::chap(m, cid::ANIMATION);
        let r = ed::find_range_related_elem(m, d, k.e, true, 0, 15000, 0x3aa2, ani);
        let hier = if r.elem != 0 { m.ptr(r.elem + elem::DATA) } else { 0 };
        let mut actions = if hier != 0 { hier + 0xc } else { 0 };
        let res = h.path_action_dialog_box(m, start_at, end_at, keep_at, (curve == 0) as i32, hier, act_at);
        if res == 0 {
            break 'act;
        }
        if res == 1 {
            if actions != 0 {
                let name = h.actions_get_new_name(m, actions, text::ACTION_NAME_1);
                let (s, e2) = (m.i32(start_at), m.i32(end_at));
                let a = h.actions_add_elem(m, actions, name, s, e2, 1, 2);
                m.set_i32(act_at, a);
            }
        } else {
            // with no animation element the original reads the action list through a null
            // pointer here
            let rec = m.ptr(actions + 4).wrapping_add((m.i32(act_at) as u32).wrapping_mul(0x30));
            let s = h.actions_sec_to_start_frm(m, fmov(m.f32(rec + 8)));
            m.set_i32(start_at, s);
            let rec = m.ptr(actions + 4).wrapping_add((m.i32(act_at) as u32).wrapping_mul(0x30));
            let e2 = h.actions_sec_to_end_frm(m, fmov(m.f32(rec + 0xc)));
            m.set_i32(end_at, e2);
        }
        let (s, e2) = (m.i32(start_at), m.i32(end_at));
        let null = action_create_null_on_path(m, h, path, s, e2, null_at);
        let (s, e2, ni) = (m.i32(start_at), m.i32(end_at), m.i32(null_at));
        action_cns_to_null(m, h, k.chap, k.index, kind, null, ni, k.cns_rel, s, e2, 0);
        if res == 1 && actions == 0 {
            actions = h.ed_get_actions(m, k.chap, k.index);
            if actions == 0 {
                break 'act;
            }
            let name = h.actions_get_new_name(m, actions, text::ACTION_NAME_2);
            let (s, e2) = (m.i32(start_at), m.i32(end_at));
            let a = h.actions_add_elem(m, actions, name, s, e2, 1, 2);
            m.set_i32(act_at, a);
        }
        if actions == 0 {
            break 'act;
        }
        let rec = m.ptr(actions + 4).wrapping_add((m.i32(act_at) as u32).wrapping_mul(0x30));
        if m.ptr(rec + 0x28) == 0 {
            let blk = m.si_calloc(1, 0x1c);
            m.set_ptr(rec + 0x28, blk);
            let d = glob::cur_dict(m);
            h.elemdict_get_elem_from_data(m, d, models, path, pidx_at);
            let blk = m.ptr(rec + 0x28);
            let v = m.i32(pidx_at);
            m.set_i32(blk, v);
            let v = m.i32(null_at);
            m.set_i32(blk + 4, v);
        }
    }
    m.release(24);
}

/// [soft 0x0821f174 SI_modelSetPath]: the model's path model (+0x2c8). Returns the model.
pub fn model_set_path(m: &mut Mem, mdl: Addr, path: Addr) -> Addr {
    m.set_ptr(mdl + pathslot::MODEL, path);
    mdl
}

/// [soft 0x0821f18c si_lightSetPositionPath]: light + 0x144.
pub fn light_set_position_path(m: &mut Mem, light: Addr, path: Addr) -> Addr {
    m.set_ptr(light + pathslot::LIGHT_POSITION, path);
    light
}

/// [soft 0x0821f1a4 si_lightSetInterestPath]: light + 0x148.
pub fn light_set_interest_path(m: &mut Mem, light: Addr, path: Addr) -> Addr {
    m.set_ptr(light + pathslot::LIGHT_INTEREST, path);
    light
}

/// Removes the path animation of the object of a graf node [soft 0x0821f1bc si_itemResetPath]:
/// every attribute's "selected" word is cleared, the ones of the kind's path menu entry are
/// raised (entry 701 for models, 702 cameras, 703 lights, 809 waves; sub-entry 3, 1 or 2 for
/// position or interest), and the element's animation is reset with deletion
/// ([`reset::elemdict_reset_elem_anim`]).
///
/// For a graf kind outside 2 … 8, 0xb, 0x13 the original passes two uninitialised registers
/// as the menu entry; here 0, 0.
pub fn item_reset_path<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, gn: Addr) {
    utils::fcv_table_set(m, 0);
    let (entry, sub) = match m.i32(gn + grafnode::KIND) {
        2 | 3 => (0x2bd, 3),
        4 => (0x2be, 1),
        5 => (0x2be, 2),
        6 | 8 | 0x13 => (0x2bf, 1),
        7 => (0x2bf, 2),
        0xb => (0x329, 3),
        _ => (0, 0),
    };
    h.set_fcv_table_from_menu(m, entry, sub);
    let out = m.local(8);
    let e = h.get_dic_info_from_grafnode(m, gn, entry, sub, out, out + 4);
    let (chap, index) = (m.i32(out), m.i32(out + 4));
    m.release(8);
    let d = glob::cur_dict(m);
    reset::elemdict_reset_elem_anim(m, h, d, e, chap, index, 1);
}

/// Before a path model is deleted: everything that follows it lets go
/// [soft 0x0821f284 SI_pathDelete].
///
/// Every **backward** relation of the path's element is visited (every list, in order). For
/// a relation under which curves are filed (`SI_reldictCheckRelAnim`), the attributes of the
/// animation it carries are selected (the path relations 112, 612, 1112, 1114, 2112, 2114:
/// the percentage curve's type; 220 and 221, deformation by spline: menu entries 793 and
/// 792) and the animation of the element at the other end is reset with deletion; if that
/// removed entries from the list, the same position is visited again. Then, whenever the
/// other element still has its object, its pointer to the path is cleared
/// ([`reset::item_relax_relation_ptrs`]).
pub fn path_delete<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, mdl: Addr) {
    let (e, _) = ed::get_elem_from_model(m, h, mdl);
    let mut g = 0;
    while g < m.i32(e + elem::BACKWARD) {
        let l = m.ptr(e + elem::BACKWARD + 4).wrapping_add((g as u32).wrapping_mul(rellist::SIZE));
        g += 1;
        let mut i = 0;
        while i < m.i32(l + rellist::NB) {
            let r = m.ptr(l + rellist::RELS).wrapping_add((i as u32).wrapping_mul(arel::SIZE));
            let relid = m.i32(r + arel::RELID);
            let chap = m.i16(r + arel::CHAPID) as i32;
            let index = m.i32(r + arel::ELEMID);
            let d = glob::cur_dict(m);
            let other = ed::get_elem(m, d, chap, index);
            let out = m.local(4);
            let anim = h.reldict_check_rel_anim(m, glob::SI_RD, relid, out, glob::SI_CID);
            m.release(4);
            let mut next = i + 1;
            if anim != 0 {
                utils::fcv_table_set(m, 0);
                match relid {
                    0x70 | 0x264 | 0x458 | 0x45a | 0x840 | 0x842 => {
                        let t = h.rel_get_fcv_type(m, relid);
                        h.attdict_set_fcurve_flag(m, glob::SI_ATD, t, 1);
                    }
                    0xdc => h.set_fcv_table_from_menu(m, 0x319, 0),
                    0xdd => h.set_fcv_table_from_menu(m, 0x318, 0),
                    _ => {}
                }
                let before = m.i32(l + rellist::NB);
                let d = glob::cur_dict(m);
                reset::elemdict_reset_elem_anim(m, h, d, other, chap, index, 1);
                if m.i32(l + rellist::NB) != before {
                    next = i;
                }
            }
            i = next;
            if m.ptr(other + elem::DATA) != 0 {
                let d = glob::cur_dict(m);
                reset::item_relax_relation_ptrs(m, h, d, chap, other, relid);
            }
        }
    }
}

/// Shows or hides the path of an object [soft 0x0821f45c si_itemShowPath]: the display
/// type (graf node + 0x0c) of the path model's graf node becomes `show`, and a path that is
/// hidden while selected as a node is unselected. A model's **trajectory** (the model its
/// element is related to by 160 in list 1) is shown or hidden the same way.
///
/// For a graf kind without a path slot the original tests a register it never set; here
/// that is "no path".
pub fn item_show_path<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, obj: Addr, kind: i32, show: i32) {
    let path = match kind {
        2 | 3 => m.ptr(obj + pathslot::MODEL),
        4 => m.ptr(obj + pathslot::CAMERA_POSITION),
        5 => m.ptr(obj + pathslot::CAMERA_INTEREST),
        6 | 8 | 0x13 => m.ptr(obj + pathslot::LIGHT_POSITION),
        7 => m.ptr(obj + pathslot::LIGHT_INTEREST),
        _ => 0,
    };
    if path != 0 {
        let gl = glob::cur_graf_list(m);
        let gn = h.find_gl_data(m, gl, path, 0);
        if gn != 0 {
            show_node(m, h, gn, show);
        }
    }
    if (kind.wrapping_sub(2) as u32) < 2 {
        let (e, _) = ed::get_elem_from_model(m, h, obj);
        let d = glob::cur_dict(m);
        let models = glob::chap(m, cid::MODELS);
        let r = ed::find_related_elem(m, d, e, true, 1, 0xa0, models);
        if r.elem != 0 {
            let gn = m.ptr(r.elem + elem::AUX_DATA);
            if gn != 0 {
                show_node(m, h, gn, show);
            }
        }
    }
}

fn show_node<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, gn: Addr, show: i32) {
    m.set_i32(gn + grafnode::DISPLAY, show);
    if m.i32(gn + grafnode::SELECTION) == 1 && show == 0 {
        let gl = glob::cur_graf_list(m);
        h.unselect_grafnode(m, gl, gn);
    }
}

/// [soft 0x0821f594 si_itemShowPathAll]: [`item_show_path`] for every graf node of the
/// current list (by `SI_nextGrafnode`), then for the current camera's position and interest
/// paths.
pub fn item_show_path_all<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, show: i32) {
    let at = m.local(4);
    loop {
        let gl = glob::cur_graf_list(m);
        h.next_grafnode(m, gl, at);
        let gn = m.ptr(at);
        if gn == 0 {
            break;
        }
        let (obj, kind) = (m.ptr(gn + grafnode::DATA), m.i32(gn + grafnode::KIND));
        item_show_path(m, h, obj, kind, show);
    }
    m.release(4);
    let cam = m.ptr(m.ptr(glob::SI_PROC_DATA) + 8);
    item_show_path(m, h, cam, graf::CAMERA, show);
    let cam = m.ptr(m.ptr(glob::SI_PROC_DATA) + 8);
    item_show_path(m, h, cam, graf::CAMERA_INTEREST, show);
}

/// The percentage curve of whoever follows a path, given the path's **curve** (the path
/// model's definition, a spline) [soft 0x0821f620 SI_findPathFcurveOwner].
///
/// Asked in this order: the current camera's position path, then its interest path; then
/// every graf node of the current list: a model's path (kinds 2, 3), a camera's position
/// path (kind 4), a light's position and interest paths (kinds 6, 8, 0x13). The first
/// follower whose element exists gives the curve of its path relation in list 1 (112, 1112,
/// 1114, 2112, 2114). Null when nobody follows the curve.
///
/// Not found this way: the interest path of a camera other than the current one, a wave's
/// path, and a second follower of the same path.
pub fn find_path_fcurve_owner<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, curve_geom: Addr) -> Addr {
    if curve_geom == 0 {
        return 0;
    }
    let follows = |m: &Mem, slot: Addr| {
        let p = m.ptr(slot);
        p != 0 && m.ptr(p + model::DEFINITION) == curve_geom
    };
    let idx = m.local(4);
    let mut found: Option<(Addr, i32)> = None;
    let cam = m.ptr(m.ptr(glob::SI_PROC_DATA) + 8);
    if follows(m, cam + pathslot::CAMERA_POSITION) {
        let cam = m.ptr(m.ptr(glob::SI_PROC_DATA) + 8);
        let e = h.ed_get_elem_from_camera(m, cam, idx);
        if e != 0 {
            found = Some((e, 0x458));
        }
    }
    if found.is_none() {
        let cam = m.ptr(m.ptr(glob::SI_PROC_DATA) + 8);
        if follows(m, cam + pathslot::CAMERA_INTEREST) {
            let cam = m.ptr(m.ptr(glob::SI_PROC_DATA) + 8);
            let e = h.ed_get_elem_from_camera(m, cam, idx);
            if e != 0 {
                found = Some((e, 0x45a));
            }
        }
    }
    if found.is_none() {
        let mut gn = m.ptr(glob::cur_graf_list(m) + 4);
        while gn != 0 && found.is_none() {
            let obj = m.ptr(gn + grafnode::DATA);
            match m.i32(gn + grafnode::KIND) {
                2 | 3 => {
                    if follows(m, obj + pathslot::MODEL) {
                        let (e, _) = ed::get_elem_from_model(m, h, obj);
                        if e != 0 {
                            found = Some((e, 0x70));
                        }
                    }
                }
                4 => {
                    if follows(m, obj + pathslot::CAMERA_POSITION) {
                        let e = h.ed_get_elem_from_camera(m, obj, idx);
                        if e != 0 {
                            found = Some((e, 0x458));
                        }
                    }
                }
                6 | 8 | 0x13 => {
                    if follows(m, obj + pathslot::LIGHT_POSITION) {
                        let e = h.ed_get_elem_from_light(m, obj, idx);
                        if e != 0 {
                            found = Some((e, 0x840));
                        }
                    }
                    if found.is_none() && follows(m, obj + pathslot::LIGHT_INTEREST) {
                        let e = h.ed_get_elem_from_light(m, obj, idx);
                        if e != 0 {
                            found = Some((e, 0x842));
                        }
                    }
                }
                _ => {}
            }
            if found.is_none() {
                gn = m.ptr(gn + grafnode::NEXT);
            }
        }
    }
    m.release(4);
    match found {
        Some((e, rel)) => ed::find_fcv(m, e, 1, rel),
        None => 0,
    }
}

/// The same for a path that is a NURBS curve [soft 0x0821f7e4 SI_findNURBSPathFcurveOwner]:
/// instruction for instruction the routine above (the definition pointer is compared, its
/// type is not looked at).
pub fn find_nurbs_path_fcurve_owner<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, crv: Addr) -> Addr {
    find_path_fcurve_owner(m, h, crv)
}

/// The path's curve, given the percentage curve [soft 0x0821f9a4 si_findPathSplineOwner]:
/// the curve's ANIMATION element, back along the path relation of its type (7 → 112 from a
/// model, 9 / 10 → 1112 / 1114 from a camera, 31 / 32 → 2112 / 2114 from a light; backward
/// list 0) to the follower, forward along the same relation in list 1 to the path model,
/// whose definition is returned. Null when a link is missing.
///
/// For another curve type the original uses an uninitialised relation and chapter (a wave's
/// percentage curve, type 172, among them); here that is "not found".
pub fn find_path_spline_owner<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, curve: Addr) -> Addr {
    if curve == 0 {
        return 0;
    }
    let d = glob::cur_dict(m);
    let ani = glob::chap(m, cid::ANIMATION);
    let i = h.elemdict_find_elem_by_data(m, d, ani, curve);
    let d = glob::cur_dict(m);
    let e = ed::get_elem(m, d, ani, i);
    if e == 0 {
        return 0;
    }
    let (rel, chap) = match fcv::kind(m, curve) {
        7 => (0x70, glob::chap(m, cid::MODELS)),
        9 => (0x458, glob::chap(m, cid::CAMERAS)),
        10 => (0x45a, glob::chap(m, cid::CAMERAS)),
        0x1f => (0x840, glob::chap(m, cid::LIGHTS)),
        0x20 => (0x842, glob::chap(m, cid::LIGHTS)),
        _ => return 0,
    };
    let d = glob::cur_dict(m);
    let follower = ed::find_related_elem(m, d, e, false, 0, rel, chap).elem;
    if follower == 0 {
        return 0;
    }
    let d = glob::cur_dict(m);
    let models = glob::chap(m, cid::MODELS);
    let path = ed::find_related_elem(m, d, follower, true, 1, rel, models).elem;
    if path == 0 || m.ptr(path + elem::DATA) == 0 {
        return 0;
    }
    m.ptr(m.ptr(path + elem::DATA) + model::DEFINITION)
}

/// Ties the keys of a percentage curve to the points of a spline, one to one
/// [soft 0x0821faf0 si_keySplineToFcurve].
///
/// With n keys and p points (the point count less 2 for the spline types 2 and 3): the
/// first min(n, p) − 1 keys become path keys of the points 0, 1, 2 …; the keys from there
/// to the one before last lose their point (−1) and their type; the **last** key becomes
/// the path key of point p − 1. The spline is marked keyframed and the values follow
/// ([`update_fcurve_path_keys`]). 0 for fewer than two keys.
///
/// With p < 1 the original's second loop starts at a negative key index and writes in
/// front of the key array; the image records that as a fault.
pub fn key_spline_to_fcurve<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, curve: Addr, spl: Addr) -> i32 {
    if spl == 0 || curve == 0 {
        return 0;
    }
    let n = m.i32(curve + fo::NB_KEYS);
    if n < 2 {
        return 0;
    }
    let mut points = m.u16(spl + spline::NB_POINTS) as i32;
    if (m.i32(spl + spline::KIND).wrapping_sub(2) as u32) < 2 {
        points -= 2;
    }
    let mut last = (if points < n { points } else { n }) - 1;
    let mut i = 0;
    while i < last {
        let k = key_at(m, curve, i);
        m.set_i16(k + ko::SPLINE_INDEX, i as i16);
        make_path_key(m, k);
        i += 1;
    }
    while last < m.i32(curve + fo::NB_KEYS) - 1 {
        let k = key_at(m, curve, last);
        m.set_i16(k + ko::SPLINE_INDEX, -1);
        let f = m.u16(k + ko::FLAGS);
        m.set_u16(k + ko::FLAGS, f & keyflag::KEPT);
        last += 1;
    }
    let k = key_at(m, curve, m.i32(curve + fo::NB_KEYS) - 1);
    m.set_i16(k + ko::SPLINE_INDEX, (points as i16).wrapping_sub(1));
    let k = key_at(m, curve, m.i32(curve + fo::NB_KEYS) - 1);
    make_path_key(m, k);
    m.set_u16(spl + spline::KEYFRAMED, 1);
    update_fcurve_path_keys(m, h, curve, spl);
    1
}

fn first_path_key(m: &Mem, curve: Addr) -> bool {
    let n = m.i32(curve + fo::NB_KEYS);
    (0..n).any(|i| is_path_key(m, key_at(m, curve, i)))
}

/// Sets the spline's "keyframed" word (+0x70) from the curve: 1 when the curve has a path
/// key, else 0 [soft 0x0821fc30 SI_checkSplineToFcurve]. The result is the opposite: 1 when
/// there is **no** path key, 0 when there is one, and 0 (the word untouched) for a null
/// argument or a curve without keys.
pub fn check_spline_to_fcurve(m: &mut Mem, curve: Addr, spl: Addr) -> i32 {
    check_keyframed(m, curve, spl, spline::KEYFRAMED)
}

/// The same for a NURBS curve (+0x4c) [soft 0x0821fca0 SI_checkNrbCrvToFcurve].
pub fn check_nrb_crv_to_fcurve(m: &mut Mem, curve: Addr, crv: Addr) -> i32 {
    check_keyframed(m, curve, crv, nurbs::KEYFRAMED)
}

fn check_keyframed(m: &mut Mem, curve: Addr, geom: Addr, word: u32) -> i32 {
    if geom == 0 || curve == 0 || m.i32(curve + fo::NB_KEYS) < 1 {
        return 0;
    }
    if first_path_key(m, curve) {
        m.set_u16(geom + word, 1);
        0
    } else {
        m.set_u16(geom + word, 0);
        1
    }
}

/// After the path's curve was reversed: every path key's point number p becomes (count − 1)
/// − p [soft 0x0821fd10 SI_invertFcurvePathKeys]. The count is the spline's point count
/// (+0x10) or, with no spline, the NURBS direction record's (+4), as 16 bits. Keys without
/// a point (−1) are left. 0 for no curve, no key, or neither geometry.
pub fn invert_fcurve_path_keys(m: &mut Mem, curve: Addr, spl: Addr, dir: Addr) -> i32 {
    if curve == 0 || m.i32(curve + fo::NB_KEYS) < 1 {
        return 0;
    }
    let count = if spl == 0 {
        if dir == 0 {
            return 0;
        }
        m.i32(dir + nurbs::NB_POINTS) as i16
    } else {
        m.i16(spl + spline::NB_POINTS)
    };
    let mut i = 0;
    while i < m.i32(curve + fo::NB_KEYS) {
        let k = key_at(m, curve, i);
        if is_path_key(m, k) {
            let p = m.i16(k + ko::SPLINE_INDEX);
            if p != -1 {
                m.set_i16(k + ko::SPLINE_INDEX, count.wrapping_sub(1).wrapping_sub(p));
            }
        }
        i += 1;
    }
    1
}

/// Brings the path keys of a percentage curve to their control points on a NURBS curve
/// [soft 0x0821fd94 SI_updateFcurveNURBSPathKeys].
///
/// Only for a keyframed curve (+0x4c) with control points and a curve with keys. The
/// length L is the last entry of the curve's length table (built first if missing:
/// `SI_nurbsCurveUpdateLength(curve, 100, 0)`); it must be above 0 (a NaN passes). Each path
/// key of point p, 0 ≤ p < point count, gets the value `(((p × L) / count) × 100) / L`
/// rounded to `float`: the points are taken as **evenly spaced** along the curve, unlike on
/// a spline. Returns 1 when the keys were visited.
pub fn update_fcurve_nurbs_path_keys<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, curve: Addr, crv: Addr) -> i32 {
    if crv == 0 || curve == 0 || m.i16(crv + nurbs::KEYFRAMED) == 0 || m.i32(crv + nurbs::NB_POINTS) < 1 || m.i32(curve + fo::NB_KEYS) < 1 {
        return 0;
    }
    if m.i32(crv + nurbs::NB_LENGTHS) == 0 {
        h.nurbs_curve_update_length(m, crv, 100, 0);
    }
    let at = m.ptr(crv + nurbs::LENGTHS).wrapping_add((m.i32(crv + nurbs::NB_LENGTHS) as u32).wrapping_mul(8)).wrapping_sub(8);
    let len = F80::from_f64(m.f64(at));
    if !(len.gt(F80::ZERO) || len.unordered(F80::ZERO)) {
        return 0;
    }
    let mut i = 0;
    while i < m.i32(curve + fo::NB_KEYS) {
        let k = key_at(m, curve, i);
        let p = m.i16(k + ko::SPLINE_INDEX) as i32;
        let count = m.i32(crv + nurbs::NB_POINTS);
        if is_path_key(m, k) && p >= 0 && p < count {
            let v = (((F80::from_i32(p) * len) / F80::from_i32(count)) * F80::from_f64(HUNDRED)) / len;
            m.set_f32(k + ko::VALUE, v.to_f32());
        }
        i += 1;
    }
    1
}

/// Brings the path keys of a percentage curve to their points on a spline
/// [soft 0x0821fe68 SI_updateFcurvePathKeys].
///
/// Only for a keyframed spline (+0x70) with points and a curve with keys. The length table
/// (+0x6c; built first if missing: `SI_splineUpdateLength(spline, 100, 0)`) holds the length
/// from the start to every sample; entry `point × step` (step: the 16-bit word +0x68) is the
/// length up to a point, entry `segments × step` the whole length L (0 with no segment). L
/// must be above 0 (a NaN passes). Each path key of point p, 0 ≤ p < point count, gets
/// `(100 × table[p × step]) / L` rounded to `float`. Returns 1 when the keys were visited.
pub fn update_fcurve_path_keys<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, curve: Addr, spl: Addr) -> i32 {
    if spl == 0 || curve == 0 || m.i16(spl + spline::KEYFRAMED) == 0 || m.i16(spl + spline::NB_POINTS) == 0 || m.i32(curve + fo::NB_KEYS) < 1 {
        return 0;
    }
    if m.ptr(spl + spline::LENGTHS) == 0 {
        h.spline_update_length(m, spl, 100, 0);
    }
    let seg = h.spline_nb_segment(m, spl);
    let mut len = 0.0f32;
    if seg != 0 {
        let i = (m.u16(spl + spline::LENGTH_STEP) as u32).wrapping_mul(seg as u32);
        len = fmov(m.f32(m.ptr(spl + spline::LENGTHS).wrapping_add(i.wrapping_mul(4))));
    }
    let l = F80::from_f32(len);
    if !(F80::ZERO.lt(l) || F80::ZERO.unordered(l)) {
        return 0;
    }
    let mut i = 0;
    while i < m.i32(curve + fo::NB_KEYS) {
        let k = key_at(m, curve, i);
        if is_path_key(m, k) {
            let p = m.i16(k + ko::SPLINE_INDEX) as i32;
            if p >= 0 && p < m.u16(spl + spline::NB_POINTS) as i32 {
                let j = (p as u32).wrapping_mul(m.u16(spl + spline::LENGTH_STEP) as u32);
                let at = m.f32(m.ptr(spl + spline::LENGTHS).wrapping_add(j.wrapping_mul(4)));
                m.set_f32(k + ko::VALUE, ((F80::from_f64(HUNDRED) * at) / len).to_f32());
            }
        }
        i += 1;
    }
    1
}

/// After point `point` of the path's curve was deleted
/// [soft 0x0821ff50 SI_deleteFcurvePathKey]: the path keys of later points move down by one
/// and the path keys of that point are removed from the curve (they are first marked with
/// the flag word 0x4d2, then removed from the last key backwards with
/// `SI_fcurveRemoveKeyIndex`). 0 for no curve or no key.
pub fn delete_fcurve_path_key<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, curve: Addr, point: i16) -> i32 {
    if curve == 0 || m.i32(curve + fo::NB_KEYS) < 1 {
        return 0;
    }
    let mut i = 0;
    while i < m.i32(curve + fo::NB_KEYS) {
        let k = key_at(m, curve, i);
        if is_path_key(m, k) {
            let p = m.i16(k + ko::SPLINE_INDEX);
            if point < p {
                m.set_i16(k + ko::SPLINE_INDEX, p.wrapping_sub(1));
            } else if p == point {
                m.set_u16(k + ko::FLAGS, keyflag::DOOMED);
            }
        }
        i += 1;
    }
    let mut i = m.i32(curve + fo::NB_KEYS) - 1;
    while i >= 0 {
        if m.u16(key_at(m, curve, i) + ko::FLAGS) == keyflag::DOOMED {
            fcv::Ops::on(m, h, curve, |f, env| f.remove_key_index(i + 1, env));
        }
        i -= 1;
    }
    1
}

/// After a point was inserted before point `point` of the path's curve
/// [soft 0x08220008 SI_addFcurvePathKey]: the path keys of that point and of later ones
/// move up by one.
pub fn add_fcurve_path_key(m: &mut Mem, curve: Addr, point: i32) -> i32 {
    shift_path_keys(m, curve, |p| if point <= p as i32 { p.wrapping_add(1) } else { p })
}

/// Adds `by` to the point number of every path key
/// [soft 0x08220068 SI_incrementFcurvePathKey] (16-bit arithmetic).
pub fn increment_fcurve_path_key(m: &mut Mem, curve: Addr, by: i32) -> i32 {
    shift_path_keys(m, curve, |p| p.wrapping_add(by as i16))
}

fn shift_path_keys(m: &mut Mem, curve: Addr, f: impl Fn(i16) -> i16) -> i32 {
    if curve == 0 || m.i32(curve + fo::NB_KEYS) < 1 {
        return 0;
    }
    let mut i = 0;
    while i < m.i32(curve + fo::NB_KEYS) {
        let k = key_at(m, curve, i);
        if is_path_key(m, k) {
            let p = m.i16(k + ko::SPLINE_INDEX);
            let q = f(p);
            if q != p {
                m.set_i16(k + ko::SPLINE_INDEX, q);
            }
        }
        i += 1;
    }
    1
}

/// Relates an object to its path [soft 0x082200c0 SI_pathElementSet]: the object's element
/// (a camera's or light's by its own look-up for the kinds 4 to 7, otherwise by a search of
/// the kind's chapter) gets the path relation of its kind to the path model's element, once
/// in list 1 and once in list 2.
pub fn path_element_set<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, obj: Addr, kind: i32, path: Addr) {
    let chap = h.db_graf_type2chapter(m, kind);
    let idx = m.local(4);
    if kind > 3 && kind < 6 {
        h.ed_get_elem_from_camera(m, obj, idx);
    } else if kind > 3 && kind < 8 {
        h.ed_get_elem_from_light(m, obj, idx);
    } else {
        let d = glob::cur_dict(m);
        let i = h.elemdict_find_elem_by_data(m, d, chap, obj);
        m.set_i32(idx, i);
    }
    let d = glob::cur_dict(m);
    let models = glob::chap(m, cid::MODELS);
    let to = h.elemdict_find_elem_by_data(m, d, models, path);
    let rel = rel_get_path_rel_from_graf_type(kind);
    let from = m.i32(idx);
    m.release(4);
    for group in [1, 2] {
        let d = glob::cur_dict(m);
        h.elemdict_add_relation(m, d, glob::SI_RD, from, to, rel, group, 0);
    }
}

/// Takes an object off its path [soft 0x082201d4 SI_pathElementRelax]: the curves of the
/// path relation are disposed of (`SI_edFcvDispose(object, chapter, 1, relation, relation)`)
/// and the relation removed from the element.
pub fn path_element_relax<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, obj: Addr, kind: i32) {
    let chap = h.db_graf_type2chapter(m, kind);
    let d = glob::cur_dict(m);
    let i = h.elemdict_find_elem_by_data(m, d, chap, obj);
    let rel = rel_get_path_rel_from_graf_type(kind);
    h.ed_fcv_dispose(m, obj, chap, 1, rel, rel);
    let d = glob::cur_dict(m);
    h.elemdict_remove_relation_id(m, d, glob::SI_RD, i, rel);
}

/// The element of the path an element follows [soft 0x08220284 SI_findElementPath]: the
/// element it is related to by the path relation of `kind` in list 1, in any chapter. For
/// the kinds 4 and 5 the element is first replaced by the one `SI_edGetElemFromCamera` /
/// `SI_edGetElemFromLight` gives for its object (kind 5, a camera's interest, goes through
/// the **light** look-up: reproduced). Null for a null element or a kind without a path
/// relation.
pub fn find_element_path<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, e: Addr, _unused: i32, _index: i32, kind: i32) -> Addr {
    if e == 0 {
        return 0;
    }
    let rel = rel_get_path_rel_from_graf_type(kind);
    let mut e = e;
    let idx = m.local(4);
    if kind == 4 {
        let obj = m.ptr(e + elem::DATA);
        e = h.ed_get_elem_from_camera(m, obj, idx);
    } else if kind == 5 {
        let obj = m.ptr(e + elem::DATA);
        e = h.ed_get_elem_from_light(m, obj, idx);
    }
    m.release(4);
    if rel == -1 {
        return 0;
    }
    let d = glob::cur_dict(m);
    ed::find_related_elem(m, d, e, true, 1, rel, -1).elem
}

/// The time of the current frame against the keys of the percentage curve: whether a key
/// sits on this frame that is not doubled by an equal neighbour, and `test` for the last
/// such key. Shared by the two overwrite routines.
///
/// The keys are walked while their time is below `t + SI_glbAnim.key_tolerance` (the
/// tolerance, 0.15, is in frames and is added to seconds as it is), t = `(float)frame /
/// rate` unrounded. A key within 1e-05 s of t counts unless the key before or the key after
/// has a value within 1e-05 of its own.
fn key_on_frame(m: &Mem, curve: Addr, mut test: impl FnMut() -> bool) -> bool {
    let small = F80::from_f64(SMALL);
    let t = frame_sec(m, m.i32(glob::SI_FRAME_CURRENT));
    let limit = t + m.f32(glob::SI_GLB_ANIM + glob::anim::KEY_TOLERANCE);
    let mut found = false;
    let mut i = 0;
    while i < m.i32(curve + fo::NB_KEYS) && limit.gt(F80::from_f32(m.f32(key_at(m, curve, i)))) {
        let k = key_at(m, curve, i);
        if (t - m.f32(k)).abs().lt(small) {
            let v = m.f32(k + ko::VALUE);
            let before = i > 0 && dist(m.f32(k.wrapping_sub(ko::SIZE) + ko::VALUE), v).lt(small);
            let after = i < m.i32(curve + fo::NB_KEYS) - 1 && dist(m.f32(k + ko::SIZE + ko::VALUE), v).lt(small);
            if !before && !after {
                found = test();
            }
        }
        i += 1;
    }
    found
}

/// Whether adding a path key should **move** an existing point instead of adding one, and
/// the move [soft 0x0822031c pathOverwrite].
///
/// Yes when a key of the percentage curve sits on the current frame (see `key_on_frame`)
/// and the spline parameter there is within 1e-05 of the whole number `point`: the object
/// is exactly on a point at a frame that has a key. The point must exist: for the types 2
/// and 3 the spline needs three points and `point + 1` must be below the count − 1 (the
/// point moved is `point + 1`, past the phantom); otherwise `point` must be below the count.
/// A Bézier spline (type 1) moves its control `3 × point + 1` (`SI_bezierMovePoint`), any
/// other type changes the point (`SI_splineChangePoint`). Returns 1 when a point was moved.
pub fn path_overwrite<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, spl: Addr, curve: Addr, factor: f32, point: i16, pt: Addr) -> i32 {
    let on_point = key_on_frame(m, curve, || (F80::from_f32(factor) - F80::from_i32(point as i32)).abs().lt(F80::from_f64(SMALL)));
    if !on_point {
        return 0;
    }
    let phantoms = (m.i32(spl + spline::KIND).wrapping_sub(2) as u32) <= 1;
    let count = m.u16(spl + spline::NB_POINTS);
    if phantoms {
        if count <= 2 {
            return 0;
        }
    } else if count == 0 {
        return 0;
    }
    let mut point = point;
    if phantoms {
        point = point.wrapping_add(1);
        if point as i32 >= count as i32 - 1 {
            return 0;
        }
    } else if point as i32 >= count as i32 {
        return 0;
    }
    if m.i32(spl + spline::KIND) == 1 {
        h.bezier_move_point(m, spl, point as i32 * 3 + 1, pt);
    } else {
        h.spline_change_point(m, spl, point as i32, pt);
    }
    1
}

/// The same for a NURBS curve [soft 0x0822053c nurbsPathOverwrite]: yes when a key sits on
/// the current frame and the control point hit at parameter `u` (`SI_nurbsCrvIndexHit`) is
/// `point`, and `point` is not the last control point. The control point then becomes the
/// position times its weight: x, y, z (doubles) = `(float)coordinate × weight`, each rounded
/// to `double`. Returns 1 when the point was written.
pub fn nurbs_path_overwrite<H: PathHost + ?Sized>(m: &mut Mem, h: &mut H, crv: Addr, curve: Addr, u: f64, point: i32, pt: Addr) -> i32 {
    let l = m.local(12);
    m.set_f64(l + 4, u);
    h.nurbs_crv_index_hit(m, crv, l, l + 4, 1);
    let hit = m.i32(l);
    m.release(12);
    let on_point = key_on_frame(m, curve, || hit == point);
    if on_point && point < m.i32(crv + nurbs::NB_POINTS) - 1 {
        for k in 0..3u32 {
            let q = m.ptr(crv + nurbs::POINTS).wrapping_add((point as u32).wrapping_mul(nurbs::POINT_SIZE));
            let v = (F80::from_f32(m.f32(pt + 4 * k)) * m.f64(q + nurbs::POINT_W)).to_f64();
            m.set_f64(q + 8 * k, v);
        }
        1
    } else {
        0
    }
}

/// The frame range a percentage curve covers [soft 0x08220704 getPathTiming]: the frames of
/// its first and last keys (`SI_secToFrame`), or the scene's first and last frames when
/// there is no curve, fewer than two keys, or no key array.
pub fn get_path_timing(m: &Mem, curve: Addr) -> (i32, i32) {
    if curve == 0 || m.i32(curve + fo::NB_KEYS) < 2 || m.ptr(curve + fo::KEYS) == 0 {
        return (m.i32(glob::SI_FRAME_START), m.i32(glob::SI_FRAME_END));
    }
    let first = sec_to_frame(m, fmov(m.f32(key_at(m, curve, 0))));
    let last = sec_to_frame(m, fmov(m.f32(key_at(m, curve, m.i32(curve + fo::NB_KEYS) - 1))));
    (first, last)
}

/// The path relation of a graf kind [soft 0x08220770 relGetPathRelFromGrafType]: 2, 3 → 112;
/// 4 → 1112; 5 → 1114; 6, 8, 0x13 → 2112; 7 → 2114; 0xb → 612; anything else −1.
pub fn rel_get_path_rel_from_graf_type(kind: i32) -> i32 {
    match kind {
        2 | 3 => 0x70,
        4 => 0x458,
        5 => 0x45a,
        6 | 8 | 0x13 => 0x840,
        7 => 0x842,
        0xb => 0x264,
        _ => -1,
    }
}
