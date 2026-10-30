// SPDX-License-Identifier: MIT
//! Copying, selecting and testing the animation of an element (`motionUtils.c`; finding
//! 0017, section 9f): "does this element have animation of the selected kinds", "put its
//! curves in the curve editor", "give this element that element's curves".
//!
//! All three walk an element the same way: the forward relations to the ANIMATION chapter
//! that are not links to an animation file (15000 to 15010), a curve taking part when the
//! attribute table has its type selected; and for a model first the parts that carry
//! animation of their own: the SHAPES element (relation 200 in list 2), every CLUSTERS
//! element (700), the node and the branch lattice (210, 215).

use crate::ed;
use crate::fcv;
use crate::glob::{self, cid};
use crate::key::{self, KeyHost};
use crate::off::{arel, elem, fcurve as fo, rellist};
use crate::utils::{self, UtilsHost};
use si_mem::{Addr, Mem};

/// `SI_edSumatraTo3x` (0x08680e50): while set, every curve is copied whatever the attribute
/// table says *[the name: set while a scene of the newer program is converted]*.
pub const SI_ED_SUMATRA_TO_3X: Addr = 0x0868_0e50;

/// Fields these routines read beyond [`crate::off`].
mod f {
    /// `__SI_Model`: index of the current material (16 bits), array of material pointers.
    pub const MDL_CUR_MATERIAL: u32 = 0x208;
    pub const MDL_MATERIALS: u32 = 0x20c;
    /// `__SI_Material`: first local 2D texture, local 3D texture.
    pub const MAT_TEXTURE2D: u32 = 0x6f0;
    pub const MAT_TEXTURE3D: u32 = 0x6fc;
    /// `light`: its type, 16 bits (2 spot, 3 infinite, 4 sun; finding 0017, section 9e).
    pub const LIGHT_TYPE: u32 = 0x1e;
    /// The record an element's first word points at: +0x14 is the element's name (a C
    /// string), as `si_elemdictSelectElemAnim` hands it to the curve editor.
    pub const NODE_NAME: u32 = 0x14;
}

/// The texts of [`elemdict_copy_elem_anim`] (formats at 0x08697940, 0x08697980; a plain text
/// at 0x086979c0).
mod text {
    pub fn state() -> &'static str {
        si_rt::text(0x0869_7940)
    }
    pub fn object() -> &'static str {
        si_rt::text(0x0869_7980)
    }
    pub fn shapes() -> &'static str {
        si_rt::text(0x0869_79c0)
    }
}

crate::host_trait! {
    /// What these routines call outside this package.
    pub trait CopyHost: UtilsHost {
        /// `SI_fcvScnglAdd(curve, name, first)` (0x0823c7a8): the curve enters the curve
        /// editor's list under the element's name.
        fn fcv_scngl_add(curve: Addr, name: Addr, first: i32);
        /// `SI_reldictGetRelName(&SI_RD, relation)` (0x0813a0f4): the relation's name, a C string.
        fn reldict_get_rel_name(reldict: Addr, relid: i32) -> Addr;
    }
}

/// The forward relation entries of `e` that lead to a curve: (list number, entry). Read
/// fresh at every step, as the original's loops do.
fn curve_entry(m: &Mem, e: Addr, g: i32, i: i32) -> Option<Addr> {
    let l = m.ptr(e + elem::FORWARD + 4).wrapping_add((g as u32).wrapping_mul(rellist::SIZE));
    let r = m.ptr(l + rellist::RELS).wrapping_add((i as u32).wrapping_mul(arel::SIZE));
    (m.i16(r + arel::CHAPID) as i32 == glob::chap(m, cid::ANIMATION) && (m.i32(r + arel::RELID).wrapping_sub(15000) as u32) > 10).then_some(r)
}

fn list_len(m: &Mem, e: Addr, g: i32) -> i32 {
    m.i32(m.ptr(e + elem::FORWARD + 4).wrapping_add((g as u32).wrapping_mul(rellist::SIZE)) + rellist::NB)
}

/// The curve a relation entry leads to. The element is not tested for null: a relation to a
/// deleted ANIMATION element is read near address 4 by the original; the image records a fault.
fn entry_curve(m: &Mem, d: Addr, r: Addr) -> Addr {
    let ce = ed::get_elem(m, d, m.i16(r + arel::CHAPID) as i32, m.i32(r + arel::ELEMID));
    m.ptr(ce.wrapping_add(elem::DATA))
}

/// Whether an element has animation of the kinds its chapter selects
/// [soft 0x0822b660 si_elemdictValidFcurve]: 1 when one of its curves has a key.
///
/// - A WAVES element: one of the twelve curves of its icon model ([`utils::get_wave_fcv_pointer`]).
/// - A MODELS element: first its parts, each with the attribute table set to the part's
///   chapter: the SHAPES element, every CLUSTERS element, the two lattices; then the current
///   material (model + 0x20c, entry + 0x208) through its element, and that material's first
///   local 2D texture and local 3D texture; then the TEXTURES2D element of relation 400 and
///   the TEXTURES3D element of relation 500 in list 3 (the global textures).
/// - Then the element's own curves, with the table set to its chapter: a selected curve
///   with at least one key.
///
/// When nothing is found the table is cleared (`SI_fcvTableSet(0)`); when something is, it
/// is left as the search set it. (For a wave it is not touched at all.)
pub fn elemdict_valid_fcurve<H: UtilsHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, e: Addr, chap: i32) -> i32 {
    if chap == glob::chap(m, cid::WAVES) {
        let out = m.local(48);
        let mdl = m.ptr(e + elem::DATA);
        utils::get_wave_fcv_pointer(m, h, mdl, out);
        let mut found = 0;
        for k in 0..12u32 {
            let c = m.ptr(out + 4 * k);
            if c != 0 && m.i32(c + fo::NB_KEYS) > 0 {
                found = 1;
                break;
            }
        }
        m.release(48);
        return found;
    }
    if chap == glob::chap(m, cid::MODELS) {
        let mdl = m.ptr(e + elem::DATA);
        let part = |m: &mut Mem, h: &mut H, pe: Addr, pchap: i32| -> bool {
            h.set_fcv_table_entries_from_chap(m, pchap);
            elemdict_valid_fcurve(m, h, d, pe, pchap) as i16 != 0
        };
        let shapes = glob::chap(m, cid::SHAPES);
        let r = ed::find_related_elem(m, d, e, true, 2, 200, shapes);
        if r.elem != 0 && part(m, h, r.elem, shapes) {
            return 1;
        }
        let clusters = glob::chap(m, cid::CLUSTERS);
        let mut r = ed::find_related_elem(m, d, e, true, 2, 700, clusters);
        while r.elem != 0 {
            if part(m, h, r.elem, clusters) {
                return 1;
            }
            r = ed::find_next_related_elem(m, d, e, true, 2, 700, clusters);
        }
        let lattices = glob::chap(m, cid::LATTICES);
        for relid in [0xd2, 0xd7] {
            let r = ed::find_related_elem(m, d, e, true, 2, relid, lattices);
            if r.elem != 0 && part(m, h, r.elem, lattices) {
                return 1;
            }
        }
        let mats = m.ptr(mdl.wrapping_add(f::MDL_MATERIALS));
        if mats != 0 {
            let mat = m.ptr(mats.wrapping_add((m.i16(mdl + f::MDL_CUR_MATERIAL) as i32 as u32).wrapping_mul(4)));
            if mat != 0 {
                let idx = m.local(4);
                let me = h.ed_get_elem_from_material(m, mat, idx);
                m.release(4);
                if me != 0 {
                    let (materials, t2d, t3d) = (glob::chap(m, cid::MATERIALS), glob::chap(m, cid::TEXTURES2D), glob::chap(m, cid::TEXTURES3D));
                    if part(m, h, me, materials) {
                        return 1;
                    }
                    let tex = m.ptr(mat + f::MAT_TEXTURE2D);
                    if tex != 0 {
                        let idx = m.local(4);
                        let te = h.ed_get_elem_from_texture2d(m, tex, idx);
                        m.release(4);
                        if te != 0 && part(m, h, te, t2d) {
                            return 1;
                        }
                    }
                    let tex = m.ptr(mat + f::MAT_TEXTURE3D);
                    if tex != 0 {
                        let idx = m.local(4);
                        let te = h.ed_get_elem_from_texture3d(m, tex, idx);
                        m.release(4);
                        if te != 0 && part(m, h, te, t3d) {
                            return 1;
                        }
                    }
                }
            }
        }
        for (relid, which) in [(400, cid::TEXTURES2D), (500, cid::TEXTURES3D)] {
            let c = glob::chap(m, which);
            let r = ed::find_related_elem(m, d, e, true, 3, relid, c);
            if r.elem != 0 && part(m, h, r.elem, c) {
                return 1;
            }
        }
    }
    h.set_fcv_table_entries_from_chap(m, chap);
    let mut g = 0;
    while g < m.i32(e + elem::FORWARD) {
        let mut i = 0;
        while i < list_len(m, e, g) {
            if let Some(r) = curve_entry(m, e, g, i) {
                let c = entry_curve(m, d, r);
                if c != 0 && h.attdict_get_fcurve_flag(m, glob::SI_ATD, fcv::kind(m, c)) != 0 && m.i32(c + fo::NB_KEYS) > 0 {
                    return 1;
                }
            }
            i += 1;
        }
        g += 1;
    }
    utils::fcv_table_set(m, 0);
    0
}

/// Puts an element's selected curves into the curve editor
/// [soft 0x0822bad8 si_elemdictSelectElemAnim] and returns how many went in.
///
/// For a model its parts first (SHAPES, every CLUSTERS element, the two lattices), each by
/// this routine. Then every selected curve of the element itself is handed to
/// `SI_fcvScnglAdd(curve, the element's name, first)`, `first` being 1 for the element's
/// first curve only. If the count (parts included) is above zero the editor's list is
/// rebuilt (`SI_fcvScrollBuild`), so a part with curves rebuilds it once more than its model.
pub fn elemdict_select_elem_anim<H: CopyHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, e: Addr, chap: i32, _index: i32) -> i32 {
    let mut n: i32 = 0;
    if chap == glob::chap(m, cid::MODELS) {
        let shapes = glob::chap(m, cid::SHAPES);
        let r = ed::find_related_elem(m, d, e, true, 2, 200, shapes);
        if r.elem != 0 {
            n = elemdict_select_elem_anim(m, h, d, r.elem, shapes, r.index);
        }
        let clusters = glob::chap(m, cid::CLUSTERS);
        let mut r = ed::find_related_elem(m, d, e, true, 2, 700, clusters);
        while r.elem != 0 {
            n = n.wrapping_add(elemdict_select_elem_anim(m, h, d, r.elem, clusters, r.index));
            r = ed::find_next_related_elem(m, d, e, true, 2, 700, clusters);
        }
        let lattices = glob::chap(m, cid::LATTICES);
        for relid in [0xd2, 0xd7] {
            let r = ed::find_related_elem(m, d, e, true, 2, relid, lattices);
            if r.elem != 0 {
                n = n.wrapping_add(elemdict_select_elem_anim(m, h, d, r.elem, lattices, r.index));
            }
        }
    }
    let mut first: i16 = 1;
    let mut g = 0;
    while g < m.i32(e + elem::FORWARD) {
        let mut i = 0;
        while i < list_len(m, e, g) {
            if let Some(r) = curve_entry(m, e, g, i) {
                let c = entry_curve(m, d, r);
                if c != 0 && h.attdict_get_fcurve_flag(m, glob::SI_ATD, fcv::kind(m, c)) != 0 {
                    let name = m.ptr(m.ptr(e + elem::NODE).wrapping_add(f::NODE_NAME));
                    h.fcv_scngl_add(m, c, name, first as i32);
                    n = n.wrapping_add(1);
                    first = 0;
                }
            }
            i += 1;
        }
        g += 1;
    }
    if n > 0 {
        h.fcv_scroll_build(m);
    }
    n
}

/// Whether a light can take the animation of a relation [soft 0x0822bd18 subChapCheckOK]:
/// always for an element outside the LIGHTS chapter or without an object; else by the
/// light's type (+0x1e): 2112, 2113 (position) not a sun (type 4); 2114, 2115 (interest),
/// 2131, 2132 (cone, spread) a spot (2) only; 2120 (sun time) a sun only; 2134, 2135
/// (fall-off), 2201, 2202 (area) the types 1 and 2 only.
pub fn sub_chap_check_ok(m: &Mem, e: Addr, chap: i32, relid: i32) -> bool {
    if chap != glob::chap(m, cid::LIGHTS) {
        return true;
    }
    let light = m.ptr(e + elem::DATA);
    if light == 0 {
        return true;
    }
    let ty = m.i16(light + f::LIGHT_TYPE);
    match relid {
        0x840 | 0x841 => ty != 4,
        0x842 | 0x843 | 0x853 | 0x854 => ty == 2,
        0x848 => ty == 4,
        0x856 | 0x857 | 0x899 | 0x89a => (ty.wrapping_sub(1) as u16) < 2,
        _ => true,
    }
}

/// Formats a text with one `%s` into a local and shows it on the status line.
fn status<H: KeyHost + ?Sized>(m: &mut Mem, h: &mut H, format: &str, name: Option<Addr>, kind: i32) {
    let mut b = match name {
        Some(a) => {
            let (head, tail) = format.split_once("%s").unwrap_or((format, ""));
            let mut b = head.as_bytes().to_vec();
            b.extend_from_slice(&m.cstr(a));
            b.extend_from_slice(tail.as_bytes());
            b
        }
        None => format.as_bytes().to_vec(),
    };
    b.push(0);
    let at = m.local(b.len() as u32);
    m.set_bytes(at, &b);
    KeyHost::status_display_message(h, m, at, kind);
    m.release(b.len() as u32);
}

/// Gives element `to` the animation of element `from`
/// [soft 0x0822bd8c SI_elemdictCopyElemAnim]; returns how many curves were copied (parts
/// included). Nothing when the two are the same element.
///
/// For a model the parts are copied first, pairwise, where **both** elements have the part:
/// the SHAPES elements, the CLUSTERS elements in list order (as long as both have a next
/// one), the node lattices, the branch lattices.
///
/// Then every curve of `from` (in relation list g, relation id r) whose type is selected
/// (every curve while `SI_edSumatraTo3x` is set):
///
/// 1. `SI_fcvGetRelInfo(type)` gives the curve's relation and how many components that
///    has. For a relation of several components the destination must have, in its list g,
///    an entry of that relation that is **not** a curve (the path model of a path, say);
///    without one the status line says "Object state incompatible. Cannot copy <relation
///    name> animation" (kind 1) and the curve is skipped.
/// 2. [`sub_chap_check_ok`] for the destination; a refusal says "Object incompatibility.
///    Cannot copy <relation name> animation" (kind 2).
/// 3. Otherwise the curve counts, and its destination is looked up: the destination's first
///    curve of relation r in list g that has the same type. When the source curve is the
///    n-th in a row of the same type (n counted from 0 over the curves met, selected or
///    not), the look-up goes n curves further (stopping at the first that is missing). A
///    destination that exists is emptied (`SI_fcurveRemoveAllKeys`); for a shape
///    interpolation curve (relation 201) with another number of keys the status line first
///    says "The selected model number of shape interpolation keys has changed." (kind 2).
///    A missing one is made (`SI_fcurveAllocate(type, the destination's object)`) and filed
///    (`SI_edFcvAdd`). Then `SI_fcurveTransfer(source, destination)`.
///
/// The texts are formatted into a 100-byte local in the original (a relation name of more
/// than 53 characters overruns it); here the local is as long as the text. The count of
/// equal types in a row is a local the original first reads when two curves in a row have
/// the same type, after having set it for the first; a first curve of type −1 would read it
/// unset (0 here).
pub fn elemdict_copy_elem_anim<H: CopyHost + ?Sized>(m: &mut Mem, h: &mut H, d: Addr, from: Addr, to: Addr, chap: i32, _from_index: i32,
                                                    to_index: i32)
                                                    -> i32 {
    if from == to {
        return 0;
    }
    let mut n: i32 = 0;
    if chap == glob::chap(m, cid::MODELS) {
        let shapes = glob::chap(m, cid::SHAPES);
        let a = ed::find_related_elem(m, d, from, true, 2, 200, shapes);
        let b = ed::find_related_elem(m, d, to, true, 2, 200, shapes);
        if a.elem != 0 && b.elem != 0 {
            n = elemdict_copy_elem_anim(m, h, d, a.elem, b.elem, shapes, a.index, b.index);
        }
        let clusters = glob::chap(m, cid::CLUSTERS);
        let mut a = ed::find_related_elem(m, d, from, true, 2, 700, clusters);
        let mut b = ed::find_related_elem(m, d, to, true, 2, 700, clusters);
        while a.elem != 0 && b.elem != 0 {
            n = n.wrapping_add(elemdict_copy_elem_anim(m, h, d, a.elem, b.elem, clusters, a.index, b.index));
            a = ed::find_next_related_elem(m, d, from, true, 2, 700, clusters);
            b = ed::find_next_related_elem(m, d, to, true, 2, 700, clusters);
        }
        let lattices = glob::chap(m, cid::LATTICES);
        for relid in [0xd2, 0xd7] {
            let a = ed::find_related_elem(m, d, from, true, 2, relid, lattices);
            let b = ed::find_related_elem(m, d, to, true, 2, relid, lattices);
            if a.elem != 0 && b.elem != 0 {
                n = n.wrapping_add(elemdict_copy_elem_anim(m, h, d, a.elem, b.elem, lattices, a.index, b.index));
            }
        }
    }
    let ani = glob::chap(m, cid::ANIMATION);
    let mut last_kind: i32 = -1;
    let mut same: i32 = 0;
    let mut g = 0;
    while g < m.i32(from + elem::FORWARD) {
        let mut i = 0;
        while i < list_len(m, from, g) {
            if let Some(r) = curve_entry(m, from, g, i) {
                let c = entry_curve(m, d, r);
                if c != 0 {
                    same = if fcv::kind(m, c) == last_kind { same.wrapping_add(1) } else { 0 };
                    let selected = if m.i16(SI_ED_SUMATRA_TO_3X) == 0 { h.attdict_get_fcurve_flag(m, glob::SI_ATD, fcv::kind(m, c)) } else { 1 };
                    'curve: {
                        if selected == 0 {
                            break 'curve;
                        }
                        let info = m.local(12);
                        h.fcv_get_rel_info(m, fcv::kind(m, c), info, info + 4, info + 8);
                        let (rel_of, width) = (m.i32(info), m.i32(info + 4));
                        m.release(12);
                        if width > 1 {
                            let tl = m.ptr(to + elem::FORWARD + 4).wrapping_add((g as u32).wrapping_mul(rellist::SIZE));
                            let mut others = 0;
                            let mut left = m.i32(tl + rellist::NB);
                            if left > 0 {
                                let mut p = m.ptr(tl + rellist::RELS);
                                while left != 0 {
                                    if m.i32(p + arel::RELID) == rel_of && m.i16(p + arel::CHAPID) as i32 != ani {
                                        others += 1;
                                    }
                                    p = p.wrapping_add(arel::SIZE);
                                    left -= 1;
                                }
                            }
                            if others < 1 {
                                let name = h.reldict_get_rel_name(m, glob::SI_RD, rel_of);
                                status(m, h, text::state(), Some(name), 1);
                                break 'curve;
                            }
                        }
                        if !sub_chap_check_ok(m, to, chap, m.i32(r + arel::RELID)) {
                            let name = h.reldict_get_rel_name(m, glob::SI_RD, m.i32(r + arel::RELID));
                            status(m, h, text::object(), Some(name), 2);
                            break 'curve;
                        }
                        n = n.wrapping_add(1);
                        let mut t = ed::find_fcv(m, to, g, m.i32(r + arel::RELID));
                        while t != 0 && fcv::kind(m, t) != fcv::kind(m, c) {
                            t = ed::find_next_fcv(m, to, g, m.i32(r + arel::RELID));
                        }
                        if same >= 1 && t != 0 {
                            let mut k = 0;
                            loop {
                                t = ed::find_next_fcv(m, to, g, m.i32(r + arel::RELID));
                                k += 1;
                                if same <= k || t == 0 {
                                    break;
                                }
                            }
                        }
                        if t != 0 {
                            if rel_of == 0xc9 && m.i32(c + fo::NB_KEYS) != m.i32(t + fo::NB_KEYS) {
                                status(m, h, text::shapes(), None, 2);
                            }
                            utils::fcurve_remove_all_keys(m, h, t);
                        } else {
                            t = key::fcurve_allocate(m, fcv::kind(m, c), m.ptr(to + elem::DATA));
                            h.ed_fcv_add(m, t, to, chap, to_index);
                        }
                        utils::fcurve_transfer(m, h, c, t);
                    }
                    last_kind = fcv::kind(m, c);
                }
            }
            i += 1;
        }
        g += 1;
    }
    n
}

/// Copies the animation of a model's current material from another model's
/// [soft 0x0822d7f0 SI_modelLoadFcurveMaterial]: the attribute table is set to the
/// MATERIALS chapter, the two current materials' elements are looked up (the source's
/// first), and when the destination's exists [`elemdict_copy_elem_anim`] runs (with a
/// source element that may be null: the original then reads through null, the image
/// records a fault).
pub fn model_load_fcurve_material<H: CopyHost + ?Sized>(m: &mut Mem, h: &mut H, to: Addr, from: Addr) {
    let materials = glob::chap(m, cid::MATERIALS);
    h.set_fcv_table_entries_from_chap(m, materials);
    let cur = |m: &Mem, mdl: Addr| m.ptr(m.ptr(mdl + f::MDL_MATERIALS).wrapping_add((m.i16(mdl + f::MDL_CUR_MATERIAL) as i32 as u32).wrapping_mul(4)));
    let to_mat = cur(m, to);
    let from_mat = cur(m, from);
    let idx = m.local(8);
    let fe = h.ed_get_elem_from_material(m, from_mat, idx + 4);
    let te = h.ed_get_elem_from_material(m, to_mat, idx);
    let (fi, ti) = (m.i32(idx + 4), m.i32(idx));
    m.release(8);
    if te != 0 {
        let d = glob::cur_dict(m);
        let materials = glob::chap(m, cid::MATERIALS);
        elemdict_copy_elem_anim(m, h, d, fe, te, materials, fi, ti);
    }
}

/// The same for a 2D texture [soft 0x0822d890 SI_texture2DLoadFcurveAll]: here both
/// elements must exist.
pub fn texture2d_load_fcurve_all<H: CopyHost + ?Sized>(m: &mut Mem, h: &mut H, to: Addr, from: Addr) {
    let t2d = glob::chap(m, cid::TEXTURES2D);
    h.set_fcv_table_entries_from_chap(m, t2d);
    let idx = m.local(8);
    let fe = h.ed_get_elem_from_texture2d(m, from, idx + 4);
    let te = h.ed_get_elem_from_texture2d(m, to, idx);
    let (fi, ti) = (m.i32(idx + 4), m.i32(idx));
    m.release(8);
    if te != 0 && fe != 0 {
        let d = glob::cur_dict(m);
        let t2d = glob::chap(m, cid::TEXTURES2D);
        elemdict_copy_elem_anim(m, h, d, fe, te, t2d, fi, ti);
    }
}

/// The same for a 3D texture [soft 0x0822d914 SI_texture3DLoadFcurveAll]: only the
/// destination's element is tested, as for materials.
pub fn texture3d_load_fcurve_all<H: CopyHost + ?Sized>(m: &mut Mem, h: &mut H, to: Addr, from: Addr) {
    let t3d = glob::chap(m, cid::TEXTURES3D);
    h.set_fcv_table_entries_from_chap(m, t3d);
    let idx = m.local(8);
    let fe = h.ed_get_elem_from_texture3d(m, from, idx + 4);
    let te = h.ed_get_elem_from_texture3d(m, to, idx);
    let (fi, ti) = (m.i32(idx + 4), m.i32(idx));
    m.release(8);
    if te != 0 {
        let d = glob::cur_dict(m);
        let t3d = glob::chap(m, cid::TEXTURES3D);
        elemdict_copy_elem_anim(m, h, d, fe, te, t3d, fi, ti);
    }
}
