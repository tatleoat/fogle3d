// SPDX-License-Identifier: MIT
//! The curves of an animation file tied to the elements they animate (`dictin.c`), the
//! step of a load that a save reads back: the animation files are written again from these
//! relations ([`crate::dictout`]).
//!
//! After [`crate::load::expand_animations`] every curve of an animation file is an element
//! of the ANIMATION chapter. Here each becomes the destination of a relation whose source
//! is the animated element and whose id is the id of the attribute that owns the curve.
//!
//! The dictionary is the image's (`si-dict`'s routines on `Ptr32<Elem>`); the hierarchy
//! animations are the values `si-elemio` read, in [`Scene::objects`]. One routine is still
//! here in a store form, for the saver: [`find_elem_by_data`].

use crate::calls::{Arg, Unported};
use crate::dbnode;
use crate::dbvsfcv;
use crate::dict::ElemId;
use crate::load::Scene;
use crate::object::Object;
use crate::world::{cid, find_list_by_name, find_related, list, slot, with_text, Out};
use si_dict::globals::{SI_CID, SI_RD};
use si_dict::{elemdict, elemname, elemrel};
use si_elemio::anim::ItemData;
use si_mem::{Mem, Ptr32};
use si_types::dict::{Elem, ElemDict, ELEM_DELETED, ELEM_VALID};

/// The first element in use of a chapter whose object is `data`, or -1: what
/// `SI_elemdictFindElemByData` (0x08134364) answers when it goes through the chapter in
/// order. (With the environment variable `SI_LOAD_OPTIM` set and the loader's table in
/// use it bisects a table sorted by pointer value instead, and with several elements on
/// one object it then answers another of them; that mode is not reproduced, so the routine
/// is not cited.)
pub fn find_elem_by_data(sc: &Scene, chap: i32, data: u32) -> i32 {
    sc.dict.ed.lists.get(chap as usize).and_then(|l| l.elems.iter().position(|e| e.rec.status == ELEM_VALID && e.rec.data.0 == data)).map_or(-1, |i| i as i32)
}

/// The curve id an item carries (the first word of a function curve or of a static value).
fn item_id(data: &ItemData) -> Option<i32> {
    match data {
        ItemData::Curve(c) => Some(c.curve_type),
        ItemData::Static(s) => Some(s.curve_type),
        ItemData::None => None,
    }
}

/// [soft 0x08128bc8 isPathAnim]: whether an item is one of the curves that drive an element
/// along a path (ids 7, 9, 10, 0x1f, 0x20, 0xac), and the item's id. False, and no id,
/// for an item that is not there or holds nothing. (For an item whose type is neither 0 nor
/// 1 the original tests a register it never loaded; such an item holds nothing, so it does
/// not get that far.)
pub fn is_path_anim(data: Option<&ItemData>) -> (bool, Option<i32>) {
    match data.and_then(item_id) {
        Some(id) => (matches!(id, 7 | 9 | 10 | 0x1f | 0x20 | 0xac), Some(id)),
        None => (false, None),
    }
}

/// The element a name of a hierarchy animation designates
/// [soft 0x081286e4 SI_elemdictGetElemFromHierAnim]: 0 for an animation of the SCENES
/// chapter; otherwise the name is split as a file name is (`dbfilenameExpand`, into five
/// buffers of the routine's own) and its name part is looked up: without a target element,
/// or in any chapter but MODELS, by name alone among the elements whose root index is
/// `root` (`SI_elemdictFindElemByNameOnly`); with a target model, the node of that name in
/// the target's hierarchy ([`crate::load::model_get_from_hierarchy`]) and the element that
/// points at it (`SI_elemdictFindElemByData`; 1 when the target has no object).
///
/// `chapter` and `name` are the hierarchy animation's chapter (`SI_HierAnim` +8) and its
/// element's name (`SI_HierAnimElem` +0), which are values of `si-elemio` here.
pub fn get_elem_from_hier_anim(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, chapter: i32, name: &[u8], target: Option<Ptr32<Elem>>, root: i32, un: &mut dyn Unported) -> i32 {
    let cid = cid(m);
    if chapter == cid.dsc {
        return 0;
    }
    let suff = m.local(12);
    let prec = m.local(12);
    let vers = m.local(12);
    let nam = m.local(0x100);
    let proj = m.local(0x100);
    with_text(m, name, |m, p| dbnode::filename_expand(m, p, [proj, nam, vers, prec, suff]));
    let found = match target {
        Some(t) if chapter == cid.model => match sc.objects.get(sc.obj(m[t].data)).cloned() {
            Some(Object::ModelNode { tree, .. }) => {
                let part = m.cstr(nam);
                let node = crate::load::model_get_from_hierarchy(m, sc, ed, tree, 0, &part);
                let handle = node.and_then(|n| sc.models.get(tree).map(|t| t.handles[n])).unwrap_or(0);
                {
                    let data = sc.obj_addr(handle);
                    elemdict::find_elem_by_data(m, &mut Out { sc, un }, ed, chapter, data)
                }
            }
            // no object (or one that is no model: a cube put in place of a missing file)
            _ => 1,
        },
        _ => elemname::find_elem_by_name_only(m, ed, chapter, Ptr32::NULL, Ptr32::new(nam), root, 0),
    };
    m.release(12 + 12 + 12 + 0x100 + 0x100);
    found
}

/// How [`set_anim_relation`] was asked to work.
#[derive(Clone, Copy, Debug, Default)]
pub struct AnimOptions {
    /// The sixth argument: only elements whose node in the scene graph is selected take
    /// their curves (elements without a node always do).
    pub selected_only: bool,
    /// The seventh: the animation comes with a scene being loaded. Each curve is related
    /// to its element as it is. Without it a curve is pasted into the curve the element
    /// already has for that attribute (the curve editor's code, not ported).
    pub loading: bool,
    /// The eighth: only the first element of the hierarchy animation is used, on the
    /// target itself.
    pub first_only: bool,
    /// The ninth: a number of frames by which pasted keys are moved.
    pub offset: i32,
}

/// `fcvAccess.277` (0x086830b0), the record of the binary's data `SI_elemdictSetAnimRelation`
/// hands to the static value setter.
const FCV_ACCESS_277: u32 = 0x0868_30b0;

/// The routines `SI_edExprRelGetFromFcvID` (`si-anim`) calls, which are `si-dict`'s on the image.
pub(crate) struct AnimSide;
impl si_anim::outside::Outside for AnimSide {
    fn call(&mut self, m: &mut Mem, r: &'static si_anim::outside::Routine, a: &[u32]) -> u32 {
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let (ed, e) = (Ptr32::new(w(0)), Ptr32::new(w(1)));
        match r.addr {
            0x0813_4fdc => elemrel::set_curr_rel_pointer(m, ed, e, w(2) as i32, Ptr32::new(w(3))) as u32,
            0x0813_4db0 => elemrel::get_cur_relation(m, ed, e, w(2) as i16, w(3) as i32).addr(),
            0x0813_335c => elemrel::find_serial_relation(m, ed, e, w(2) as i32, Ptr32::new(w(3)), w(4) as i32, w(5) as i16, w(6) as i16).addr(),
            0x0813_354c => elemrel::set_rel_to_serialid(m, ed, e, w(2) as i32, w(3) as i32, w(4) as i16).addr(),
            _ => 0,
        }
    }
    fn call_f(&mut self, _m: &mut Mem, _r: &'static si_anim::outside::Routine, _a: &[u32]) -> si_x87::F80 {
        si_x87::F80::ZERO
    }
}

/// Ties the curves of one animation file's element to the elements they animate
/// (`SI_elemdictSetAnimRelation`, 0x08128c3c); returns how many curves and static values
/// were attached. The value form, used when the animation element's data is a boxed object;
/// the image form ([`crate::animrel_img::elemdict_set_anim_relation`], cited and checked) is
/// taken when it is an image record. This one is not compared with the original: the two calls that were
/// recorded are now made (a static value is written by si-expr's `SI_edElementStaticValueSet`
/// port, an expression's relation found by si-anim's `SI_edExprRelGetFromFcvID` port), but
/// outside a load the routine still records `SI_fcurveRemove` and (in [`check_paste_fcv`])
/// `SI_edFindFcv` and `SI_fcvMoveKeyTime`, which edit a boxed curve and not the image, and
/// the hierarchy animation it reads is the loader's own object, not a record of the image, so
/// the original cannot be run on the same input (finding 0018, "Where it stands"). The
/// sample corpus has no static value and no expression curve in a load (8,103 items, all
/// curves), so the two new calls were not exercised there.
///
/// `anim` is an element of the ANIMATION chapter that is a root (depth 0) and points at a
/// hierarchy animation. The relation between the animation and what it animates is the one
/// of the animation's chapter ([`dbvsfcv::rel_get_anim_f_rel_from_chapter`]).
///
/// - Without `target`: the animated root is the element that relation leads to from the
///   animation (backward side, list 0); without one nothing is done.
/// - With `target`: if the animation has no such relation yet, the target's root is related
///   to it; the animation whose index the curves' elements get as root is the one the
///   target is already related to (none: -1); the animated root is the element that points
///   at the target's object, or for LIGHTS and CAMERAS the target's own root.
///
/// For each element of the hierarchy animation that has items, the dictionary's element is
/// looked up ([`get_elem_from_hier_anim`]). If there is none: the status line says `No
/// element [<name>] found to connect fcurve` and the elements of its curves are deleted.
/// Otherwise, item by item:
///
/// - a path curve is attached only if the element has a path (some relation of the curve's
///   attribute in its second forward list); else `Cannot connect path animation to item
///   without path.`;
/// - a static value is set on the element (→ `SI_edElementStaticValueSet`);
/// - a curve whose attribute is 11000 belongs to a relation of the element (the item names
///   the relation and its occurrence): that relation's entry learns the curve's element
///   (+8), and the element is related to the curve by 11000. For an expression the relation
///   is found by the curve's parameter (→ `SI_edExprRelGetFromFcvID`); a custom effect's
///   parameter (0x236) is passed over;
/// - any other curve goes through [`check_paste_fcv`].
///
/// Outside a load, an item whose flag (+0x1c) is clear is dropped instead: its curve is
/// removed (→ `SI_fcurveRemove`) and its element deleted.
#[allow(clippy::too_many_arguments)]
pub fn set_anim_relation(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, anim: Ptr32<Elem>, anim_index: i32, target: Option<(Ptr32<Elem>, i32)>, opt: AnimOptions, un: &mut dyn Unported) -> i32 {
    let cid = cid(m);
    if m[anim].hier_level != 0 {
        return 0;
    }
    let anim_data = sc.obj(m[anim].data);
    let Some(Object::Animation(ha)) = sc.objects.get(anim_data) else { return 0 };
    let chapter = match ha.chapter.clone() {
        Some(n) => find_list_by_name(m, ed, &n),
        None => cid.model,
    };
    let relid = dbvsfcv::rel_get_anim_f_rel_from_chapter(m, chapter);
    let root: i32;
    let anim_root: i32;
    match target {
        None => {
            let Some((_, t)) = find_related(m, ed, anim, 0, 0, relid, chapter, true) else { return 0 };
            root = t.index;
            anim_root = anim_index;
        }
        Some((t, _)) => {
            let (t_root, t_data) = (m[t].root_id, m[t].data);
            if find_related(m, ed, anim, 0, 0, relid, chapter, true).is_none() {
                elemrel::add_relation(m, ed, SI_RD, t_root, anim_index, relid, 1, 0);
            }
            anim_root = find_related(m, ed, t, 1, 0, relid, cid.ani, true).map_or(-1, |(_, x)| x.index);
            root = if chapter == cid.lig || chapter == cid.cam { t_root } else { elemdict::find_elem_by_data(m, &mut Out { sc, un }, ed, chapter, t_data) };
        }
    }
    let mut attached = 0;
    let nb_elems = match sc.objects.get(anim_data) {
        Some(Object::Animation(ha)) => ha.elems.len(),
        _ => 0,
    };
    for k in 0..nb_elems {
        if opt.first_only && k >= 1 {
            break;
        }
        let (name, nb_items) = match sc.objects.get(anim_data) {
            Some(Object::Animation(ha)) => (ha.elems[k].name.clone(), ha.elems[k].items.len()),
            _ => break,
        };
        if nb_items == 0 {
            continue;
        }
        let idx = if target.is_none() || !opt.first_only { get_elem_from_hier_anim(m, sc, ed, chapter, &name, target.map(|t| t.0), root, un) } else { root };
        // the element of the ANIMATION chapter that stands for item j of this element
        let curve_elem = |m: &Mem, sc: &Scene, j: usize| -> i32 {
            let l = list(m, ed, m[SI_CID].ani);
            (0..m[l].nb_elems)
                .find(|&i| {
                    let e = m[l].elems.add(i);
                    m[e].status == ELEM_VALID && matches!(sc.objects.get(sc.obj(m[e].data)), Some(Object::Curve { anim: h, elem, item }) if *h == anim_data && *elem == k && *item == j)
                })
                .unwrap_or(-1)
        };
        let item_of = |sc: &Scene, j: usize| match sc.objects.get(anim_data) {
            Some(Object::Animation(ha)) => ha.elems[k].items.get(j).cloned(),
            _ => None,
        };
        let delete_curve = |m: &mut Mem, sc: &mut Scene, un: &mut dyn Unported, ci: i32| {
            let ce = crate::world::elem(m, ed, ElemId::new(cid.ani, ci));
            elemdict::set_delete_elem(m, &mut Out { sc, un }, ed, cid.ani, ce, ci, -1, 0, 1);
        };
        if idx < 0 {
            sc.messages.push(crate::calls::Message { channel: crate::calls::Channel::Status(2), text: [b"No element [".as_slice(), &name, b"] found to connect fcurve"].concat() });
            for j in 0..nb_items {
                if item_of(sc, j).is_some_and(|it| it.kind == 0) {
                    let ci = curve_elem(m, sc, j);
                    if ci >= 0 {
                        delete_curve(m, sc, un, ci);
                    }
                }
            }
            continue;
        }
        // `SI_elemdictGetElem`: a slot that is out of range or freed gives a null pointer,
        // which the original then reads through; here that element takes nothing
        let Some(el) = slot(m, ed, ElemId::new(chapter, idx)).filter(|&e| m[e].status != ELEM_DELETED) else { continue };
        let el_id = ElemId::new(chapter, idx);
        let aux = m[el].aux_data;
        let node_selected = sc.graf.get(sc.graf_handle(aux)).map(|n| n.selection != 0);
        if !(aux.is_null() || !opt.selected_only || node_selected.unwrap_or(false)) {
            continue;
        }
        for j in 0..nb_items {
            let Some(it) = item_of(sc, j) else { break };
            let (path, id) = is_path_anim(Some(&it.data));
            if path {
                let (prel, _, _) = crate::world::fcv_rel_info(m, id.unwrap_or(0));
                if find_related(m, ed, el, 1, 1, prel, -1, true).is_none() {
                    sc.messages.push(crate::calls::Message { channel: crate::calls::Channel::Status(2), text: si_rt::message(0x0868_3100).to_vec() });
                    continue;
                }
            }
            if !opt.loading && it.w_1c == 0 {
                if it.kind == 0 {
                    let ci = curve_elem(m, sc, j);
                    if ci >= 0 {
                        un.call("SI_fcurveRemove", &[Arg::Elem(ElemId::new(cid.ani, ci))]);
                        delete_curve(m, sc, un, ci);
                    }
                }
                continue;
            }
            match it.kind {
                1 => {
                    // `SI_AttributeValue local = { 1, item->value }` (a local of the frame), written to
                    // the element by `SI_edElementStaticValueSet` with the binary's `fcvAccess.277`
                    let value = match &it.data {
                        ItemData::Static(s) => s.value.to_bits(),
                        _ => 0,
                    };
                    let local = m.local(8);
                    m.set_u32(local, 1);
                    m.set_u32(local + 4, value);
                    let mut p = crate::image::Prog { sc: &mut *sc, un: &mut *un, fs: None, db: None };
                    si_expr::image::elem::ed_element_static_value_set(m, &mut crate::image::ExprSide(&mut p), el, chapter, id.unwrap_or(0), Ptr32::new(FCV_ACCESS_277), Ptr32::new(local), anim_index, 0);
                    m.release(8);
                    attached += 1;
                }
                0 => {
                    let Some(curve_id) = id else { continue };
                    let info = crate::world::fcv_info(m, curve_id);
                    let ci = curve_elem(m, sc, j);
                    if ci < 0 {
                        continue;
                    }
                    if info.relid == 11000 {
                        let (rel, occ) = (it.act_rel_id, it.act_occ_id as i32);
                        let entry = if rel == 0x4e2a {
                            if occ == 0x236 {
                                continue;
                            }
                            Ptr32::new(si_anim::edanim::ed_expr_rel_get_from_fcv_id(m, &mut AnimSide, ed.addr(), el.addr(), occ as u32))
                        } else {
                            // SI_elemdictGetRelation: the entry of that relation and occurrence
                            elemrel::get_relation(m, ed, el, rel, -1, occ)
                        };
                        if !entry.is_null() {
                            m[entry].active_elem = ci;
                            elemrel::add_uncomplete_relation(m, ed, SI_RD, chapter, cid.ani, idx, ci, 11000, 1, 0, Ptr32::NULL, 1);
                        }
                    } else {
                        check_paste_fcv(m, ed, el_id, ci, curve_id, it.w_20, anim_root, opt, un);
                        attached += 1;
                    }
                }
                _ => {}
            }
        }
    }
    attached
}

/// Attaches one curve to its element (`elemdictCheckPasteFcv`, 0x081287c0; reproduced for
/// a load, which is all a scene load uses; outside a load only the branch "the element has
/// no curve of that id yet" is taken, so the routine is not cited). Nothing is done for a
/// curve no attribute owns. Otherwise the curve learns its element's object
/// (`fcurve` +4), and:
///
/// - during a load: the element is related to the curve's element by the attribute's id,
///   with the rule number and count limit the attribute dictionary gives
///   (`SI_attdictGetInfoFromFcurve`, [`crate::world::fcv_info`]) and the item's own flag as the
///   entry's active word;
/// - otherwise the element's existing curves of that attribute are looked through
///   (→ `SI_edFindFcv`): without one of the same id the curve's keys are moved by the
///   offset (→ `SI_fcvMoveKeyTime`), the relation is added (active), and the curve's
///   element takes `anim_root` as its root index when that is not negative; with one, the
///   keys are pasted into it (→ `SI_fcvSimplePasteInsert`), the curve is removed and its
///   element deleted. The curve package's side of that is recorded, not done: the port
///   takes the "no existing curve" branch.
#[allow(clippy::too_many_arguments)]
pub fn check_paste_fcv(m: &mut Mem, ed: Ptr32<ElemDict>, elem: ElemId, curve_elem: i32, curve_id: i32, flag: i16, anim_root: i32, opt: AnimOptions, un: &mut dyn Unported) {
    let ani = m[SI_CID].ani;
    let info = crate::world::fcv_info(m, curve_id);
    if info.relid == 0 {
        return;
    }
    if opt.loading {
        elemrel::add_complete_relation(m, ed, SI_RD, elem.index, curve_elem, info.relid, info.rule, info.limit, Ptr32::NULL, flag);
    } else {
        un.call("SI_edFindFcv", &[Arg::Elem(elem), Arg::Int(info.relid as i64)]);
        un.call("SI_fcvMoveKeyTime", &[Arg::Elem(ElemId::new(ani, curve_elem)), Arg::Int(opt.offset as i64)]);
        elemrel::add_relation(m, ed, SI_RD, elem.index, curve_elem, info.relid, info.rule, info.limit);
        if anim_root >= 0 {
            if let Some(c) = slot(m, ed, ElemId::new(ani, curve_elem)) {
                m[c].root_id = anim_root;
            }
        }
    }
}

/// [soft 0x081291cc SI_elemdictSetAnimationRelations]: `SI_elemdictSetAnimRelation` for
/// every animation file's element of this load (new-flag 1, depth 0), without a target, as
/// part of a load. Where the file is in the image (its element points at the `SI_HierAnim`
/// the reader on the image made) the routine runs on that record
/// ([`crate::animrel_img::elemdict_set_anim_relation`], with the load's outside world
/// [`crate::animrel_img::Loading`]); otherwise, or with `SCENE_NO_ANIMREL_ON_IMAGE=1`, on the
/// loader's values ([`set_anim_relation`]).
pub fn set_animation_relations(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, selected_only: bool, un: &mut dyn Unported) {
    let l = list(m, ed, m[SI_CID].ani);
    let mut i = 0;
    while i < m[l].nb_elems {
        let e = m[l].elems.add(i);
        if m[e].new_flag == 1 && m[e].hier_level == 0 {
            let data = m[e].data;
            if !data.is_null() && crate::world::obj_of(data) == 0 && on_image() {
                let mut p = crate::image::Prog { sc: &mut *sc, un: &mut *un, fs: None, db: None };
                let mut o = crate::animrel_img::Loading(&mut p);
                crate::animrel_img::elemdict_set_anim_relation(&mut o, m, ed.addr(), SI_RD.addr(), e.addr(), i, 0, selected_only as i16, 1, 0, 0);
            } else {
                set_anim_relation(m, sc, ed, e, i, None, AnimOptions { selected_only, loading: true, first_only: false, offset: 0 }, un);
            }
        }
        i += 1;
    }
}

/// `SCENE_NO_ANIMREL_ON_IMAGE=1` keeps [`set_animation_relations`] on the loader's values.
fn on_image() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SCENE_NO_ANIMREL_ON_IMAGE").is_none())
}
