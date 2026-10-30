// SPDX-License-Identifier: MIT
//! `SI_elemdictSetAnimRelation` (`dictin.c`) on the image: the curves of one animation
//! file's element tied to the elements they animate, reading the file's `SI_HierAnim`
//! record that the reader on the image made ([`crate::image::read_animations`]).
//!
//! [`crate::animrel::set_anim_relation`] is the same routine on the loader's values (the
//! hierarchy animation of [`crate::load::Scene::objects`]); the load uses this one when the
//! animation file is in the image (finding 0018, "SI_elemdictSetAnimRelation on the image").
//!
//! Every callee is called by its address ([`ByAddr`]); [`Loading`] is the outside world a
//! scene load gives it, which sends each to the port that exists (`si-dict`, `si-anim`,
//! `si-expr`, this crate) and leaves the curve editor's routines hooks.
//!
//! The records, as this routine reads them (offsets in bytes):
//!
//! - an element (`SI_Elem`, 0x84 bytes): its object +0x04, its graph node +0x08, its root
//!   index +0x50, its depth +0x54;
//! - the graph node: its selection word +0x08;
//! - `SI_HierAnim`: the count of its elements +0x00, their array +0x04 (0x28 bytes each),
//!   the chapter it animates +0x08;
//! - an element of it: its name +0x00, the count of its items +0x08, and per item: the type
//!   (16 bits; 0 a curve, 1 a static value) at +0x0c, the relation id (32 bits) at +0x10, the
//!   relation's occurrence (16 bits) at +0x14, the item (a pointer) at +0x18, the flag that
//!   keeps it outside a load (16 bits) at +0x1c, the active flag of its relation (16 bits)
//!   at +0x20 (each an array);
//! - an item: its curve id +0x00; a static value's value (`float`) +0x04.

use crate::edtools::ByAddr;
use si_mem::Mem;

/// `SI_relGetAnimFRelFromChapter (0x0810f540)`: (chapter).
pub const SI_REL_GET_ANIM_F_REL_FROM_CHAPTER: u32 = 0x0810_f540;
/// `SI_elemdictFindRelatedElem (0x0813508c)`: (dictionary, element, list, side, relation
/// id, chapter, &element, &index).
pub const SI_ELEMDICT_FIND_RELATED_ELEM: u32 = 0x0813_508c;
/// `SI_elemdictAddRelation (0x08132f78)`: (dictionary, relation dictionary, from, to, id,
/// rule, count).
pub const SI_ELEMDICT_ADD_RELATION: u32 = 0x0813_2f78;
/// `SI_elemdictFindElemByData (0x08134364)`: (dictionary, chapter, object).
pub const SI_ELEMDICT_FIND_ELEM_BY_DATA: u32 = 0x0813_4364;
/// `SI_elemdictGetElemFromHierAnim (0x081286e4)`: (dictionary, hierarchy animation, its
/// element, target, root).
pub const SI_ELEMDICT_GET_ELEM_FROM_HIER_ANIM: u32 = 0x0812_86e4;
/// `SI_statusDisplayMessage (0x083cca94)`: (text, channel).
pub const SI_STATUS_DISPLAY_MESSAGE: u32 = 0x083c_ca94;
/// `SI_elemdictSetDeleteElem (0x08135ad4)`: (dictionary, chapter, element, index, -1, 0, 1).
pub const SI_ELEMDICT_SET_DELETE_ELEM: u32 = 0x0813_5ad4;
/// `SI_elemdictGetElem (0x08132b50)`: (dictionary, chapter, index).
pub const SI_ELEMDICT_GET_ELEM: u32 = 0x0813_2b50;
/// `SI_fcvGetRelInfo (0x0810f698)`: (curve id, &relation id, &a, &b).
pub const SI_FCV_GET_REL_INFO: u32 = 0x0810_f698;
/// `SI_fcurveRemove (0x081ff0a0)`: (&curve).
pub const SI_FCURVE_REMOVE: u32 = 0x081f_f0a0;
/// `SI_attdictGetInfoFromFcurve (0x08120264)`: (attribute dictionary, curve id, &relation
/// id, &group, &rule, &count).
pub const SI_ATTDICT_GET_INFO_FROM_FCURVE: u32 = 0x0812_0264;
/// `SI_edExprRelGetFromFcvID (0x080f086c)`: (dictionary, element, parameter id).
pub const SI_ED_EXPR_REL_GET_FROM_FCV_ID: u32 = 0x080f_086c;
/// `SI_elemdictGetRelation (0x08134dec)`: (dictionary, element, relation id, -1, occurrence).
pub const SI_ELEMDICT_GET_RELATION: u32 = 0x0813_4dec;
/// `SI_elemdictAddUnCompleteRelation (0x08132fb4)`.
pub const SI_ELEMDICT_ADD_UN_COMPLETE_RELATION: u32 = 0x0813_2fb4;
/// `elemdictCheckPasteFcv (0x081287c0)`, `dictin.c` (`si-anim`'s port on the image).
pub const ELEMDICT_CHECK_PASTE_FCV: u32 = 0x0812_87c0;
/// `SI_edElementStaticValueSet (0x080f06e4)`: (element, chapter, curve id, access, &value,
/// animation index, 0).
pub const SI_ED_ELEMENT_STATIC_VALUE_SET: u32 = 0x080f_06e4;

/// `SI_cid`: ANIMATION +0x00, CAMERAS +0x04, LIGHTS +0x18.
const SI_CID: u32 = 0x0878_1660;
/// `SI_ATD`, the attribute dictionary.
const SI_ATD: u32 = 0x0878_1438;
/// `SI_procData` (a pointer; its first word is the index of the current dictionary) and
/// `SI_elemDicts` (a pointer to the dictionaries, 0x70 bytes each).
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_ELEM_DICTS: u32 = 0x0867_931c;
/// `workStr` (0x200 bytes) of `dictin.c`.
pub const WORK_STR: u32 = 0x0878_6e00;
/// `fcvAccess.277`, the access record of the binary's data handed to the static value setter.
pub const FCV_ACCESS_277: u32 = 0x0868_30b0;
/// The binary's text `Cannot connect path animation to item without path.`
pub const NO_PATH_TEXT: u32 = 0x0868_3100;
/// The text the format `No element [%s] found to connect fcurve` (0x086830c0) gives.
fn no_element(name: &[u8]) -> Vec<u8> {
    [b"No element [".as_slice(), name, b"] found to connect fcurve"].concat()
}

/// [soft 0x08128bc8 isPathAnim] on the image: whether item `j` of the hierarchy
/// animation's element `hel` is one of the curves that drive an element along a path (ids
/// 7, 9, 10, 0x1f, 0x20, 0xac); its id goes to `id`. False, and nothing stored, for no
/// element, an item out of range or null, or no `id`. (For an item whose type is neither 0
/// nor 1 the original compares, and stores, a register it never loaded; such an item is
/// taken as no path here and nothing is stored. The reader on the image leaves those items
/// null, so the case does not arise from a file.)
pub fn is_path_anim(m: &mut Mem, hel: u32, j: i32, id: u32) -> bool {
    if hel == 0 || j < 0 || j >= m.i32(hel + 8) {
        return false;
    }
    let item = m.u32(m.u32(hel + 0x18).wrapping_add(4 * j as u32));
    if item == 0 || id == 0 {
        return false;
    }
    let ty = m.u16(m.u32(hel + 0xc).wrapping_add(2 * j as u32));
    if ty != 0 && ty != 1 {
        return false;
    }
    let v = m.u32(item);
    m.set_u32(id, v);
    matches!(v, 7 | 9 | 0x1f | 10 | 0x20 | 0xac)
}

/// What `fld` of a `float` and `fstp` back to a `float` leave of its bits: the same, but a
/// signalling NaN comes back quiet.
fn through_x87(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// [soft 0x08128c3c SI_elemdictSetAnimRelation]: ties the curves of the animation file's
/// element `anim` (index `anim_index` in the ANIMATION chapter) to the elements they
/// animate; answers how many curves and static values were attached.
///
/// The arguments after `target`: `selected_only` (only elements whose graph node is
/// selected, or that have none, take their curves), `loading` (the animation comes with a
/// scene being loaded: every curve is related as it is; without it an item whose flag +0x1c
/// is clear is dropped, its curve removed and its element deleted), `first_only` (only the
/// first element of the hierarchy animation, on the target itself) and `offset` (frames by
/// which pasted keys move).
///
/// Nothing for an element below the top (depth +0x54 not 0) or without a hierarchy
/// animation. The relation between the animation and what it animates is the one of the
/// animation's chapter (`SI_relGetAnimFRelFromChapter`).
///
/// - Without `target`: the animated root is the element that relation leads to from the
///   animation (list 0, side 0); without one nothing is done. The curves' elements keep
///   `anim_index` as their root.
/// - With `target`: if the animation has no such relation yet, the target's root is related
///   to it (rule 1, count 0); the root the pasted curves' elements get is the animation the
///   target is already related to (list 1; -1 for none); the animated root is the element
///   whose object is the target's, or for LIGHTS and CAMERAS the target's own root.
///
/// For each element of the hierarchy animation that has items: the dictionary's element it
/// names (`SI_elemdictGetElemFromHierAnim`; with a target and `first_only`, the animated
/// root itself). None: the status line says `No element [<name>] found to connect fcurve`
/// and the elements of its curves are deleted. Otherwise (unless `selected_only` and the
/// element's node is not selected), item by item:
///
/// - a path curve is attached only if the element has a path: a relation of the curve's
///   attribute (`SI_fcvGetRelInfo`) in list 1, side 1, of the current dictionary; else
///   the status line says `Cannot connect path animation to item without path.`;
/// - outside a load, an item whose flag +0x1c is clear is dropped (a curve: removed,
///   `SI_fcurveRemove`, and its element deleted);
/// - a static value is set on the element (`SI_edElementStaticValueSet`, the binary's
///   `fcvAccess.277`, a local `{1, value}`), counted;
/// - a curve whose attribute's relation is 11000 belongs to a relation of the element (the
///   item names the relation and its occurrence): for an expression (0x4e2a) the relation
///   is found by the parameter (`SI_edExprRelGetFromFcvID`; a custom effect's parameter,
///   0x236, is passed over), otherwise `SI_elemdictGetRelation`; its entry learns the curve's
///   element (+8) and the element is related to it by 11000 (`SI_elemdictAddUnCompleteRelation`);
/// - any other curve goes through `elemdictCheckPasteFcv`, counted.
///
/// A curve whose element is not found (`SI_elemdictFindElemByData` on its item) is passed
/// over; types other than 0 and 1 are passed over.
#[allow(clippy::too_many_arguments)]
pub fn elemdict_set_anim_relation<O: ByAddr + ?Sized>(
    o: &mut O,
    m: &mut Mem,
    dict: u32,
    rd: u32,
    anim: u32,
    anim_index: i32,
    target: u32,
    selected_only: i16,
    loading: i16,
    first_only: i16,
    offset: i32,
) -> i32 {
    let ani = m.u32(SI_CID);
    let mut count = 0i32;
    if m.u32(anim + 0x54) != 0 {
        return 0;
    }
    let ha = m.u32(anim + 4);
    if ha == 0 {
        return 0;
    }
    let sp = m.sp();
    // the frame's locals: the related element's index and element, the root of pasted
    // curves, the path's id and relation, the attribute's four answers, the static value
    let (root, rel_elem, anim_root, path_id, path_rel) = (m.local(4), m.local(4), m.local(4), m.local(4), m.local(4));
    let (att_rel, att_grp, att_rule, att_count) = (m.local(4), m.local(4), m.local(4), m.local(4));
    let value = m.local(8);
    let relid = o.call_addr(m, SI_REL_GET_ANIM_F_REL_FROM_CHAPTER, &[m.u32(ha + 8)]);
    if target == 0 {
        let chap = m.u32(ha + 8);
        if o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, anim, 0, 0, relid, chap, rel_elem, root]) == 0 {
            m.set_sp(sp);
            return 0;
        }
        m.set_u32(anim_root, anim_index as u32);
    } else {
        let chap = m.u32(ha + 8);
        if o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, anim, 0, 0, relid, chap, rel_elem, root]) == 0 {
            let troot = m.u32(target + 0x50);
            o.call_addr(m, SI_ELEMDICT_ADD_RELATION, &[dict, rd, troot, anim_index as u32, relid, 1, 0]);
        }
        let cid_ani = m.u32(SI_CID);
        if o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, target, 1, 0, relid, cid_ani, rel_elem, anim_root]) == 0 {
            m.set_u32(anim_root, u32::MAX);
        }
        let (tdata, chap) = (m.u32(target + 4), m.u32(ha + 8));
        let r = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, chap, tdata]);
        m.set_u32(root, r);
        let chap = m.u32(ha + 8);
        if chap == m.u32(SI_CID + 0x18) || chap == m.u32(SI_CID + 4) {
            let troot = m.u32(target + 0x50);
            m.set_u32(root, troot);
        }
    }
    let mut k = 0i32;
    if k < m.i32(ha) {
        let mut hel = m.u32(ha + 4);
        loop {
            'hel: {
                if m.i32(hel + 8) <= 0 {
                    break 'hel;
                }
                let idx = if target == 0 || first_only == 0 {
                    let r = m.u32(root);
                    o.call_addr(m, SI_ELEMDICT_GET_ELEM_FROM_HIER_ANIM, &[dict, ha, hel, target, r]) as i32
                } else {
                    m.i32(root)
                };
                let item_at = |m: &Mem, j: i32| m.u32(hel + 0x18).wrapping_add(4 * j as u32);
                let type_of = |m: &Mem, j: i32| m.u16(m.u32(hel + 0xc).wrapping_add(2 * j as u32));
                let delete = |o: &mut O, m: &mut Mem, e: i32| {
                    let elems = m.u32(m.u32(dict + 4).wrapping_add(ani.wrapping_shl(6)).wrapping_add(8));
                    let el = elems.wrapping_add((e as u32).wrapping_mul(0x84));
                    o.call_addr(m, SI_ELEMDICT_SET_DELETE_ELEM, &[dict, ani, el, e as u32, u32::MAX, 0, 1]);
                };
                if idx < 0 {
                    let name = m.u32(hel);
                    let text = if name == 0 { b"(null)".to_vec() } else { m.cstr(name) };
                    m.put_cstr(WORK_STR, &no_element(&text));
                    o.call_addr(m, SI_STATUS_DISPLAY_MESSAGE, &[WORK_STR, 2]);
                    let mut j = 0i32;
                    while j < m.i32(hel + 8) {
                        if type_of(m, j) == 0 {
                            let item = m.u32(item_at(m, j));
                            let e = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, ani, item]) as i32;
                            if e >= 0 {
                                delete(o, m, e);
                            }
                        }
                        j += 1;
                    }
                    break 'hel;
                }
                let chap = m.u32(ha + 8);
                let el = o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict, chap, idx as u32]);
                let aux = m.u32(el + 8);
                if aux != 0 && selected_only != 0 && m.u32(aux + 8) == 0 {
                    break 'hel;
                }
                let mut j = 0i32;
                while j < m.i32(hel + 8) {
                    'item: {
                        m.set_u32(path_id, 0);
                        if is_path_anim(m, hel, j, path_id) {
                            m.set_u32(path_rel, 0);
                            let id = m.u32(path_id);
                            o.call_addr(m, SI_FCV_GET_REL_INFO, &[id, path_rel, rel_elem, rel_elem]);
                            let prel = m.u32(path_rel);
                            let cur = m.u32(m.u32(SI_PROC_DATA)).wrapping_mul(0x70).wrapping_add(m.u32(SI_ELEM_DICTS));
                            if o.call_addr(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[cur, el, 1, 1, prel, u32::MAX, rel_elem, rel_elem]) == 0 {
                                o.call_addr(m, SI_STATUS_DISPLAY_MESSAGE, &[NO_PATH_TEXT, 2]);
                                break 'item;
                            }
                        }
                        if loading == 0 && m.u16(m.u32(hel + 0x1c).wrapping_add(2 * j as u32)) == 0 {
                            if type_of(m, j) != 0 {
                                break 'item;
                            }
                            let item = m.u32(item_at(m, j));
                            let e = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, ani, item]) as i32;
                            if e < 0 {
                                break 'item;
                            }
                            let p = item_at(m, j);
                            o.call_addr(m, SI_FCURVE_REMOVE, &[p]);
                            delete(o, m, e);
                            break 'item;
                        }
                        match type_of(m, j) as i16 {
                            1 => {
                                let item = m.u32(item_at(m, j));
                                m.set_u32(value, 1);
                                let v = through_x87(m.u32(item + 4));
                                m.set_u32(value + 4, v);
                                let (id, chap) = (m.u32(item), m.u32(ha + 8));
                                o.call_addr(m, SI_ED_ELEMENT_STATIC_VALUE_SET, &[el, chap, id, FCV_ACCESS_277, value, anim_index as u32, 0]);
                                count += 1;
                            }
                            0 => {
                                let pitem = item_at(m, j);
                                let id = m.u32(m.u32(pitem));
                                o.call_addr(m, SI_ATTDICT_GET_INFO_FROM_FCURVE, &[SI_ATD, id, att_rel, att_grp, att_rule, att_count]);
                                let item = m.u32(item_at(m, j));
                                let e = o.call_addr(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, ani, item]) as i32;
                                if e < 0 {
                                    break 'item;
                                }
                                if m.i32(att_rel) == 0x2af8 {
                                    let rel = m.u32(m.u32(hel + 0x10).wrapping_add(4 * j as u32));
                                    m.set_u32(att_rel, rel);
                                    let occ = m.u16(m.u32(hel + 0x14).wrapping_add(2 * j as u32)) as i16 as i32;
                                    m.set_u32(att_count, occ as u32);
                                    let entry = if rel == 0x4e2a {
                                        if occ == 0x236 {
                                            break 'item;
                                        }
                                        o.call_addr(m, SI_ED_EXPR_REL_GET_FROM_FCV_ID, &[dict, el, occ as u32])
                                    } else {
                                        o.call_addr(m, SI_ELEMDICT_GET_RELATION, &[dict, el, rel, u32::MAX, occ as u32])
                                    };
                                    if entry == 0 {
                                        break 'item;
                                    }
                                    m.set_u32(entry + 8, e as u32);
                                    let (chap, cid_ani) = (m.u32(ha + 8), m.u32(SI_CID));
                                    o.call_addr(m, SI_ELEMDICT_ADD_UN_COMPLETE_RELATION, &[dict, rd, chap, cid_ani, idx as u32, e as u32, 0x2af8, 1, 0, 0, 1]);
                                } else {
                                    let active = m.u16(m.u32(hel + 0x20).wrapping_add(2 * j as u32)) as i16 as i32;
                                    let (r, chap) = (m.u32(anim_root), m.u32(ha + 8));
                                    o.call_addr(
                                        m,
                                        ELEMDICT_CHECK_PASTE_FCV,
                                        &[dict, rd, el, chap, idx as u32, pitem, e as u32, active as u32, r, loading as i32 as u32, offset as u32],
                                    );
                                    count += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    j += 1;
                }
            }
            k += 1;
            if k >= m.i32(ha) {
                break;
            }
            hel = m.u32(ha + 4).wrapping_add((k as u32).wrapping_mul(0x28));
            if first_only != 0 && k > 0 {
                break;
            }
        }
    }
    m.set_sp(sp);
    count
}

/// The outside world of [`elemdict_set_anim_relation`] during a scene load (what
/// [`crate::animrel::set_animation_relations`] gives it): each callee goes to the port that
/// exists for it, everything else to the loader's [`crate::image::Prog`].
///
/// - `SI_elemdictGetElemFromHierAnim`: [`crate::animrel::get_elem_from_hier_anim`] on the
///   record's chapter (+0x08) and its element's name (+0x00);
/// - `elemdictCheckPasteFcv`: `si-anim`'s port on the image (its callees here too);
/// - `SI_edElementStaticValueSet`: `si-expr`'s; `SI_edExprRelGetFromFcvID`: `si-anim`'s;
/// - `SI_elemdictGetRelation`, `SI_elemdictSetDeleteElem`, `SI_attdictGetInfoFromFcurve`,
///   `SI_elemdictAddCompleteRelation`, `SI_reldictGetRelChapGrp`: `si-dict`'s;
/// - `SI_statusDisplayMessage`: the scene's messages, on the status line at its level.
///
/// The curve editor's routines that only a paste outside a load reaches (`SI_fcurveRemove`,
/// `SI_edFindFcv`, `SI_fcvMoveKeyTime`, `SI_fcvSimplePasteInsert`) stay the loader's hooks.
pub struct Loading<'p, 'a>(pub &'p mut crate::image::Prog<'a>);

/// `SI_elemdictAddCompleteRelation (0x08132dc4)` and `SI_reldictGetRelChapGrp (0x0813a464)`,
/// which `elemdictCheckPasteFcv` calls.
const SI_ELEMDICT_ADD_COMPLETE_RELATION: u32 = 0x0813_2dc4;
const SI_RELDICT_GET_REL_CHAP_GRP: u32 = 0x0813_a464;

impl ByAddr for Loading<'_, '_> {
    fn call_addr(&mut self, m: &mut Mem, f: u32, a: &[u32]) -> u32 {
        use si_mem::Ptr32;
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        match f {
            SI_ELEMDICT_GET_ELEM_FROM_HIER_ANIM => {
                let (ha, hel, target) = (w(1), w(2), w(3));
                let chapter = m.i32(ha + 8);
                let name = match m.u32(hel) {
                    0 => Vec::new(),
                    p => m.cstr(p),
                };
                let target = (target != 0).then(|| Ptr32::new(target));
                let p = &mut *self.0;
                crate::animrel::get_elem_from_hier_anim(m, p.sc, Ptr32::new(w(0)), chapter, &name, target, w(4) as i32, p.un) as u32
            }
            ELEMDICT_CHECK_PASTE_FCV => {
                si_anim::dictin::elemdict_check_paste_fcv(m, &mut AnimBy(self), w(0), w(1), w(2), w(3) as i32, w(4) as i32, w(5), w(6) as i32, w(7) as i16, w(8) as i32, w(9) as i16, w(10) as i32);
                0
            }
            SI_ED_ELEMENT_STATIC_VALUE_SET => {
                let mut side = crate::image::ExprSide(&mut *self.0);
                si_expr::image::elem::ed_element_static_value_set(m, &mut side, Ptr32::new(w(0)), w(1) as i32, w(2) as i32, Ptr32::new(w(3)), Ptr32::new(w(4)), w(5) as i32, w(6)) as u32
            }
            SI_ED_EXPR_REL_GET_FROM_FCV_ID => si_anim::edanim::ed_expr_rel_get_from_fcv_id(m, &mut crate::animrel::AnimSide, w(0), w(1), w(2)),
            SI_ELEMDICT_GET_RELATION => si_dict::elemrel::get_relation(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32, w(3) as i32, w(4) as i32).addr(),
            SI_ELEMDICT_SET_DELETE_ELEM => {
                let p = &mut *self.0;
                let mut out = crate::world::Out { sc: &mut *p.sc, un: &mut *p.un };
                si_dict::elemdict::set_delete_elem(m, &mut out, Ptr32::new(w(0)), w(1) as i32, Ptr32::new(w(2)), w(3) as i32, w(4) as i32, w(5) as i16, w(6) as i16) as u32
            }
            SI_ATTDICT_GET_INFO_FROM_FCURVE => {
                si_dict::attdict::get_info_from_fcurve(m, Ptr32::new(w(0)), w(1) as i32, Ptr32::new(w(2)), Ptr32::new(w(3)), Ptr32::new(w(4)), Ptr32::new(w(5)));
                0
            }
            SI_ELEMDICT_ADD_COMPLETE_RELATION => {
                let (ed, rd) = (Ptr32::new(w(0)), Ptr32::new(w(1)));
                si_dict::elemrel::add_complete_relation(m, ed, rd, w(2) as i32, w(3) as i32, w(4) as i32, w(5) as i32, w(6) as i32, Ptr32::new(w(7)), w(8) as i16) as u32
            }
            SI_RELDICT_GET_REL_CHAP_GRP => si_dict::reldict::get_rel_chap_grp(m, Ptr32::new(w(0)), w(1) as i32, Ptr32::new(w(2))) as u32,
            SI_STATUS_DISPLAY_MESSAGE => {
                let text = m.cstr(w(0));
                self.0.sc.messages.push(crate::calls::Message { channel: crate::calls::Channel::Status(w(1) as i32), text });
                0
            }
            _ => self.0.call_addr(m, f, a),
        }
    }
}

/// `si-anim`'s outside world over a [`ByAddr`]: by the routine's address.
struct AnimBy<'b, B: ByAddr + ?Sized>(&'b mut B);

impl<B: ByAddr + ?Sized> si_anim::outside::Outside for AnimBy<'_, B> {
    fn call(&mut self, m: &mut Mem, r: &'static si_anim::outside::Routine, a: &[u32]) -> u32 {
        self.0.call_addr(m, r.addr, a)
    }
    fn call_f(&mut self, _m: &mut Mem, _r: &'static si_anim::outside::Routine, _a: &[u32]) -> si_x87::F80 {
        si_x87::F80::ZERO
    }
}
