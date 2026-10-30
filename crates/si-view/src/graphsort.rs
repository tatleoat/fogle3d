// SPDX-License-Identifier: MIT
//! `graphsort.c` (0x08339a70 to 0x0833cb6f, 31 functions): the layout of the schematic
//! view, on the element dictionary.
//!
//! Ported here: which relations make the schematic view's hierarchies, the walks that
//! mark and reset the elements before a layout, the comparison the layout sorts
//! brothers with, the boxes of an element and of what hangs under it, and the dispatch
//! of the operations on a hierarchy (`SI_schematicHierOperation`, finding 0018, section
//! 23.8) with the operation on an element and its sons (`si_schemElemOper`, section
//! 23.11). Not ported: the layout itself (positions, widths) and the operations' own
//! routines; finding 0027 lists them.
//!
//! **The schematic hierarchy.** An element's sons are, in its *backward* lists, the
//! relations 0x6e (a model's sons), 0x456 and 0x83e; in its *forward* lists the relations
//! 0x4e22, 0x12c, 0x190, 0x191, 0x1f4, 0x1f5, 0x2bc, 0xc8, 0xd2, 0xd7 and 0x3a98 to
//! 0x3aa2. A material or a texture that several elements use hangs under the first of
//! them only (`si_isPrimaryMatterRelation`).

use crate::globals::*;
use crate::outside::{self, Outside};
use si_mem::{Mem, Ptr32};
use si_types::dict::{ARelation, Elem, ElemDict, ElemList, RelList, Relations};
use si_x87::F80;

/// Offsets in `SI_glbSchem` this file reads beside the layout direction in its first word.
mod schem {
    /// A byte of switches: 4 the curves are shown, 8 the materials and textures.
    pub const SHOWN: u32 = 0x0c;
    pub const SHOW_CURVES: u8 = 4;
    pub const SHOW_MATTER: u8 = 8;
    /// 32 bits: 0 while only the user's own placing is taken back for selected elements.
    pub const WHOLE: u32 = 0x10;
    /// 16 bits: the user's own placing of every element is forgotten.
    pub const FORGET_USER: u32 = 0x46;
    /// 16 bits: positions are kept as they are (no walk makes them relative).
    pub const KEEP: u32 = 0x4c;
    /// 16 bits: with [`KEEP`] set, the operation 23 still moves the elements
    /// (`si_schemElemOper`).
    pub const MOVE_KEPT: u32 = 0x4e;
    /// 16 bits: while set, a son of a chapter the view does not show (materials and
    /// textures without bit 8 of [`SHOWN`], curves without bit 4) is marked laid out and
    /// not measured (`schemComputeElemRelBBox`). What sets it is not read.
    pub const SKIP_HIDDEN: u32 = 0x20;
}

/// Whether a relation of a list makes a son in the schematic view: by the side the list
/// is on (see the top of the file).
fn makes_a_son(relid: i32, backward: bool) -> bool {
    if backward {
        matches!(relid, 0x6e | 0x456 | 0x83e)
    } else {
        matches!(relid, 0x4e22 | 0x12c | 0x190 | 0x191 | 0x1f4 | 0x1f5 | 0x2bc | 0xc8 | 0xd2 | 0xd7) || (relid.wrapping_sub(0x3a98) as u32) <= 0xa
    }
}

/// The element at the other end of a relation, read straight from the dictionary's
/// arrays, with its chapter.
fn other_end(m: &Mem, ed: Ptr32<ElemDict>, rel: Ptr32<ARelation>) -> (Ptr32<Elem>, i32) {
    let chap = m[rel].chapid as i32;
    let elems = m.u32(m[ed].lists.addr().wrapping_add((chap as u32) << 6).wrapping_add(8));
    (Ptr32::new(elems.wrapping_add((m[rel].elemid as u32).wrapping_mul(0x84))), chap)
}

/// Whether a chapter is one of the three whose elements may be shared: materials, 2D and
/// 3D textures.
fn is_matter(m: &Mem, chap: i32) -> bool {
    let ids = m[SI_CID];
    chap == ids[cid::MATERIALS] || chap == ids[cid::TEXTURES2D] || chap == ids[cid::TEXTURES3D]
}

/// Whether `elem` hangs under `parent` in the schematic view
/// [soft 0x08339a70 si_isPrimaryMatterRelation]: yes when it has no backward lists, or
/// at most one relation in the first of them; otherwise only when the element that first
/// relation names (`SI_elemdictGetElem (0x08132b50)`) is `parent`.
pub fn is_primary_matter_relation(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, parent: Ptr32<Elem>) -> i32 {
    let lists: Ptr32<RelList> = m[elem].backward.groups;
    if lists.is_null() || m[lists].nb <= 1 {
        return 1;
    }
    let rel: Ptr32<ARelation> = m[lists].rels;
    let (c, e) = (m[rel].chapid as i32 as u32, m[rel].elemid as u32);
    let first = o.call(m, &outside::SI_ELEMDICT_GET_ELEM, &[ed.addr(), c, e]);
    (first == parent.addr()) as i32
}

/// The model a material or texture hangs under
/// [soft 0x08339ab0 getPrimaryMatterRelationParent]: the element related to it by 0x12c
/// in the MODELS chapter (`SI_elemdictFindRelatedElem (0x0813508c)`), with the chapter
/// that look-up found stored through `chap` (when there is such an element and `chap` is
/// not null). Null when the element has no backward lists or nothing is related.
pub fn get_primary_matter_relation_parent(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, chap: Ptr32<i32>) -> Ptr32<Elem> {
    if m[elem].backward.groups.is_null() {
        return Ptr32::NULL;
    }
    let index = m.local(4);
    let found = m.local(4);
    let models = m[SI_CID][cid::MODELS] as u32;
    let parent = o.call(m, &outside::SI_ELEMDICT_FIND_RELATED_ELEM, &[ed.addr(), elem.addr(), 0, 0, 0x12c, models, found, index]);
    let c = m.i32(found);
    m.release(8);
    if parent != 0 && !chap.is_null() {
        m[chap] = c;
    }
    Ptr32::new(parent)
}

/// Clears the layout mark of the elements below `elem` on one side
/// [soft 0x08339b00 schemElemRelFlag]: every son (see the top of the file; `backward`
/// says which side `rels` is) goes to `si_schemElemFlag` with `elem` as its parent and
/// `relative` handed on. A material or texture is passed over unless `elem` is the one it
/// hangs under.
pub fn schem_elem_rel_flag(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, rels: Ptr32<Relations>, backward: i16, relative: i16) {
    let mut g: i32 = 0;
    while g < m[rels].nb_groups && m.fault().is_none() {
        let list: Ptr32<RelList> = m[rels].groups.add(g);
        g += 1;
        let mut k: i32 = 0;
        while k < m[list].nb && m.fault().is_none() {
            let rel: Ptr32<ARelation> = m[list].rels.add(k);
            k += 1;
            if !makes_a_son(m[rel].relid, backward != 0) {
                continue;
            }
            let (other, chap) = other_end(m, ed, rel);
            if is_matter(m, chap) && is_primary_matter_relation(m, o, ed, other, elem) as i16 == 0 {
                continue;
            }
            schem_elem_flag(m, o, ed, elem, other, relative);
        }
    }
}

/// `a − b` of two floats on the x87 unit, stored as a float.
fn minus(a: f32, b: f32) -> f32 {
    (F80::from_f32(a) - F80::from_f32(b)).to_f32()
}

/// Clears the layout mark (+0x58) of an element and of everything below it in the
/// schematic view [soft 0x08339c4c si_schemElemFlag]: nothing for a null element or one
/// that is not in use; its backward side first, then its forward side. With `relative`,
/// a parent, and the "keep positions" word of `SI_glbSchem` (+0x4c) clear, the element's
/// position (+0x0c, +0x10) is made relative to the parent's **after** its own sons were
/// done: each coordinate less the parent's, rounded once.
pub fn schem_elem_flag(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, parent: Ptr32<Elem>, elem: Ptr32<Elem>, relative: i16) {
    if elem.is_null() || m[elem].status != 0 {
        return;
    }
    m[elem].work_flag = 0;
    schem_elem_rel_flag(m, o, ed, elem, Ptr32::new(elem.addr().wrapping_add(0x68)), 1, relative);
    schem_elem_rel_flag(m, o, ed, elem, Ptr32::new(elem.addr().wrapping_add(0x60)), 0, relative);
    if relative != 0 && !parent.is_null() && m.i16(SI_GLB_SCHEM.addr() + schem::KEEP) == 0 {
        m[elem].schem_x = minus(m[elem].schem_x, m[parent].schem_x);
        m[elem].schem_y = minus(m[elem].schem_y, m[parent].schem_y);
    }
}

/// Prepares the elements below `elem` on one side for a layout
/// [soft 0x08339cd8 elemRelGraphSortInit]: every son that is in use and not marked yet
/// (a material or texture only under the element it hangs under) goes to
/// `elemGraphSortInit` with `elem` as its parent.
pub fn elem_rel_graph_sort_init(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, rels: Ptr32<Relations>, backward: i16) {
    let mut g: i32 = 0;
    while g < m[rels].nb_groups && m.fault().is_none() {
        let list: Ptr32<RelList> = m[rels].groups.add(g);
        g += 1;
        let mut k: i32 = 0;
        while k < m[list].nb && m.fault().is_none() {
            let rel: Ptr32<ARelation> = m[list].rels.add(k);
            k += 1;
            if !makes_a_son(m[rel].relid, backward != 0) {
                continue;
            }
            let (other, chap) = other_end(m, ed, rel);
            if m[other].work_flag != 0 || m[other].status != 0 {
                continue;
            }
            if is_matter(m, chap) && is_primary_matter_relation(m, o, ed, other, elem) as i16 == 0 {
                continue;
            }
            elem_graph_sort_init(m, o, ed, other, elem);
        }
    }
}

/// Prepares an element and everything below it for a layout
/// [soft 0x08339e28 elemGraphSortInit]: nothing for an element that is marked (+0x58 not
/// 0) or not in use. Its sons first, the backward side before the forward. Then: with
/// the word at +0x46 of `SI_glbSchem`, bit 0 of the element's byte at +0x2a (the user
/// placed it) is cleared; unless the word at +0x4c is set, the position is made relative
/// to the parent's (each coordinate less the parent's, rounded once), or, without a
/// parent, set to 0 with that bit cleared; the width (+0x14) becomes 0 and the mark 1.
pub fn elem_graph_sort_init(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, parent: Ptr32<Elem>) {
    if m[elem].work_flag != 0 || m[elem].status != 0 {
        return;
    }
    elem_rel_graph_sort_init(m, o, ed, elem, Ptr32::new(elem.addr().wrapping_add(0x68)), 1);
    elem_rel_graph_sort_init(m, o, ed, elem, Ptr32::new(elem.addr().wrapping_add(0x60)), 0);
    if m.i16(SI_GLB_SCHEM.addr() + schem::FORGET_USER) != 0 {
        m[elem].schem_flags &= 0xfe;
    }
    if m.i16(SI_GLB_SCHEM.addr() + schem::KEEP) == 0 {
        if !parent.is_null() {
            m[elem].schem_x = minus(m[elem].schem_x, m[parent].schem_x);
            m[elem].schem_y = minus(m[elem].schem_y, m[parent].schem_y);
        } else {
            m[elem].schem_x = 0.0;
            m[elem].schem_y = 0.0;
            m[elem].schem_flags &= 0xfe;
        }
    }
    m[elem].schem_width = 0.0;
    m[elem].work_flag = 1;
}

/// One element of a chapter before a layout [soft 0x08339ec0 relGraphSortInitElement]:
/// nothing for one that is not in use. With `reset` its mark is cleared and that is all.
/// While the word at +0x10 of `SI_glbSchem` is 0, a selected element (+0x42) that is not
/// a root loses the "placed by the user" bit, and that is all. Otherwise, for a root
/// (level 0) while the word at +0x4c is clear, and not for materials and textures unless
/// bit 8 of the byte at +0x0c is set, nor for curves unless bit 4 is: with `deep` the
/// element goes to `elemGraphSortInit` without a parent, else its position and width are
/// set to 0.
pub fn rel_graph_sort_init_element(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, chap: i32, reset: i16, deep: i16) {
    if m[elem].status != 0 {
        return;
    }
    if reset != 0 {
        m[elem].work_flag = 0;
        return;
    }
    let glb = SI_GLB_SCHEM.addr();
    if m.i32(glb + schem::WHOLE) == 0 {
        if m[elem].selected != 0 && m[elem].hier_level != 0 {
            m[elem].schem_flags &= 0xfe;
        }
        return;
    }
    if is_matter(m, chap) && m.u8(glb + schem::SHOWN) & schem::SHOW_MATTER == 0 {
        return;
    }
    if chap == m[SI_CID][cid::ANIMATION] && m.u8(glb + schem::SHOWN) & schem::SHOW_CURVES == 0 {
        return;
    }
    if m[elem].hier_level != 0 || m.i16(glb + schem::KEEP) != 0 {
        return;
    }
    if deep != 0 {
        elem_graph_sort_init(m, o, ed, elem, Ptr32::NULL);
    } else {
        m[elem].schem_x = 0.0;
        m[elem].schem_y = 0.0;
        m[elem].schem_width = 0.0;
    }
}

/// Every element in use of a chapter before a layout
/// [soft 0x08339f84 relGraphSortInitChapter], each handed to `relGraphSortInitElement`.
/// With `effects_only`, only the elements whose object, read as a model, has algorithm
/// 0xf (the word at +0x2ec of the element's object).
pub fn rel_graph_sort_init_chapter(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chapter: Ptr32<ElemList>, chap: i32, reset: i16, deep: i16, effects_only: i16) {
    let mut i: i32 = 0;
    while i < m[chapter].nb_elems && m.fault().is_none() {
        let elem: Ptr32<Elem> = m[chapter].elems.add(i);
        i += 1;
        if m[elem].status != 0 {
            continue;
        }
        if effects_only != 0 {
            let object = m[elem].data.addr();
            if m.i32(object.wrapping_add(0x2ec)) != 0xf {
                continue;
            }
        }
        rel_graph_sort_init_element(m, o, ed, elem, chap, reset, deep);
    }
}

/// The chapters the schematic view shows: animation, cameras, lights, materials, models,
/// named selections, 2D and 3D textures, clusters, shapes, lattices, waves.
const SHOWN: [usize; 12] = [
    cid::ANIMATION,
    cid::CAMERAS,
    cid::LIGHTS,
    cid::MATERIALS,
    cid::MODELS,
    cid::NSELECTIONS,
    cid::TEXTURES2D,
    cid::TEXTURES3D,
    cid::CLUSTERS,
    cid::SHAPES,
    cid::LATTICES,
    cid::WAVES,
];

fn is_shown(m: &Mem, chap: i32) -> bool {
    let ids = m[SI_CID];
    SHOWN.iter().any(|&k| chap == ids[k])
}

/// Prepares a dictionary for a layout of the schematic view
/// [soft 0x0833a004 si_schemSortInit]. With `deep`, the MODELS chapter is done first and
/// twice: once for the models with algorithm 0xf only, once for all. Then every chapter
/// the schematic view shows, in index order (the MODELS chapter not again when `deep`),
/// through `relGraphSortInitChapter` with `reset` and `deep` handed on.
pub fn schem_sort_init(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, reset: i16, deep: i16) {
    if deep != 0 {
        let models = m[SI_CID][cid::MODELS];
        let chapter: Ptr32<ElemList> = Ptr32::new(m[ed].lists.addr().wrapping_add((models as u32) << 6));
        rel_graph_sort_init_chapter(m, o, ed, chapter, models, reset, deep, 1);
        let models = m[SI_CID][cid::MODELS];
        rel_graph_sort_init_chapter(m, o, ed, chapter, models, reset, deep, 0);
    }
    let mut chap: i32 = 0;
    while chap < m[ed].nb_lists && m.fault().is_none() {
        if is_shown(m, chap) && !(chap == m[SI_CID][cid::MODELS] && deep != 0) {
            let chapter: Ptr32<ElemList> = Ptr32::new(m[ed].lists.addr().wrapping_add((chap as u32) << 6));
            rel_graph_sort_init_chapter(m, o, ed, chapter, chap, reset, deep, 0);
        }
        chap += 1;
    }
}

/// Forgets the user's own placing of every element of the chapters the schematic view
/// shows [soft 0x0833a11c si_schemResetUserPos]: bit 0 of the byte at +0x2a is cleared,
/// for elements in use or not.
pub fn schem_reset_user_pos(m: &mut Mem, ed: Ptr32<ElemDict>) {
    let mut chap: i32 = 0;
    while chap < m[ed].nb_lists && m.fault().is_none() {
        if is_shown(m, chap) {
            let chapter: Ptr32<ElemList> = Ptr32::new(m[ed].lists.addr().wrapping_add((chap as u32) << 6));
            let mut i: i32 = 0;
            while i < m[chapter].nb_elems && m.fault().is_none() {
                let elem: Ptr32<Elem> = m[chapter].elems.add(i);
                m[elem].schem_flags &= 0xfe;
                i += 1;
            }
        }
        chap += 1;
    }
}

/// The order of two sons in a layout [soft 0x0833a1d0 schemCompareElems], as `qsort`
/// asks: `a` and `b` point to two indices into the relation list `schem_sort_prl`, whose
/// relations name elements of the dictionary `schem_sort_ped`. 1 when the second's
/// position is below the first's, −1 when it is above, 0 when they are equal or do not
/// compare (a NaN), when an index is outside the list or an element is at address 0. The
/// position is the element's x (+0x0c) while the layout direction (`SI_glbSchem`'s first
/// word) is 0, else its y (+0x10).
pub fn schem_compare_elems(m: &Mem, a: Ptr32<i32>, b: Ptr32<i32>) -> i32 {
    let ia = m[a];
    if ia < 0 {
        return 0;
    }
    let list: Ptr32<RelList> = m[SCHEM_SORT_PRL];
    let n = m[list].nb;
    if ia >= n {
        return 0;
    }
    let ib = m[b];
    if ib < 0 || ib >= n {
        return 0;
    }
    let rels: Ptr32<ARelation> = m[list].rels;
    let ed: Ptr32<ElemDict> = m[SCHEM_SORT_PED];
    let (ea, _) = other_end(m, ed, rels.add(ia));
    let (eb, _) = other_end(m, ed, rels.add(ib));
    if ea.is_null() || eb.is_null() {
        return 0;
    }
    let (pa, pb) = if m.i32(SI_GLB_SCHEM.addr()) == 0 { (m[ea].schem_x, m[eb].schem_x) } else { (m[ea].schem_y, m[eb].schem_y) };
    let (pa, pb) = (F80::from_f32(pa), F80::from_f32(pb));
    if pb.lt(pa) {
        1
    } else if pb.gt(pa) {
        -1
    } else {
        0
    }
}

// ---------------------------------------------------------------- boxes and operations
//
// Compared with the emulated original on made-up dictionaries (the fifth part of
// `re/tools/si-scene/emu_gen_edmat.py`, `scene-emucheck`; finding 0018, section 23.8).

/// 1e10 as a float (.rodata 0x086589c8 and 0x08658a30): what a box's low corner starts at;
/// its negative (0x086589cc, 0x08658a34) is what the high corner starts at.
const FAR: f32 = f32::from_bits(0x5015_02f9);
const NEAR: f32 = f32::from_bits(0xd015_02f9);

/// The box of an element and of everything below it in the schematic view
/// [soft 0x0833b864 SI_schemComputeElemBBox]. Nothing for an element that is not in use,
/// or, with `done`, one already laid out (+0x58). An element whose sons are folded away
/// (+0x18 above 0, not a root, bit 1 of +0x2a clear) is left too. Otherwise a box of its
/// own (two corners on the image's stack, at 1e10 and −1e10) is grown by its sons
/// (`schemComputeElemRelBBox`, the backward side first) and by its own position: each
/// low coordinate is kept only when it is below the position, each high one only when
/// above it (a NaN gives the position). The element's box (+0x2c) is the low corner and
/// the high corner plus 2 and 0.5 (4 and 1 for the chapter of `SI_cid`'s ninth word, the
/// named selections), each sum rounded once to a float. Then the corners passed grow:
/// `min` takes a low coordinate below its own, `max` a high one above (without the
/// margins; a NaN keeps theirs). Every float goes through the x87 unit.
pub fn schem_compute_elem_bbox(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, chap: i32, min: u32, max: u32, done: i16) {
    if done != 0 && m[elem].work_flag != 0 {
        return;
    }
    if m[elem].status != 0 {
        return;
    }
    // the corners: high at -0x10, low at -0x8 of the original's frame
    let hi = m.local(16);
    let lo = hi + 8;
    for (at, v) in [(lo, FAR), (lo + 4, FAR), (hi, NEAR), (hi + 4, NEAR)] {
        m.set_f32(at, F80::from_f32(v).to_f32());
    }
    if m[elem].schem_flags & 2 == 0 && m[elem].schem_collapse > 0 && m[elem].hier_level != 0 {
        m.release(16);
        return;
    }
    let e = elem.addr();
    schem_compute_elem_rel_bbox(m, o, ed, elem, Ptr32::new(e.wrapping_add(0x68)), 1, lo, hi, done);
    schem_compute_elem_rel_bbox(m, o, ed, elem, Ptr32::new(e.wrapping_add(0x60)), 0, lo, hi, done);
    let ld = |m: &Mem, at: u32| F80::from_f32(m.f32(at));
    let x = F80::from_f32(m[elem].schem_x);
    let l0 = ld(m, lo);
    m.set_f32(lo, if l0.lt(x) { l0 } else { x }.to_f32());
    let y = F80::from_f32(m[elem].schem_y);
    let l1 = ld(m, lo + 4);
    m.set_f32(lo + 4, if l1.lt(y) { l1 } else { y }.to_f32());
    let h0 = ld(m, hi);
    m.set_f32(hi, if h0.gt(x) { h0 } else { x }.to_f32());
    let h1 = ld(m, hi + 4);
    let top = if h1.gt(y) { h1 } else { y };
    m.set_f32(hi + 4, top.to_f32());
    let (dy, dx) = if chap == m[SI_CID][cid::GROUPS] { (F80::from_f32(1.0), F80::from_f32(4.0)) } else { (F80::from_f32(0.5), F80::from_f32(2.0)) };
    let l0 = ld(m, lo);
    m[elem].schem_bbox[0] = l0.to_f32();
    let l1 = ld(m, lo + 4);
    m[elem].schem_bbox[1] = l1.to_f32();
    let h0 = ld(m, hi);
    m[elem].schem_bbox[2] = (dx + h0).to_f32();
    m[elem].schem_bbox[3] = (dy + top).to_f32();
    let v = ld(m, min);
    m.set_f32(min, if v.gt(l0) { l0 } else { v }.to_f32());
    let v = ld(m, min + 4);
    m.set_f32(min + 4, if v.gt(l1) { l1 } else { v }.to_f32());
    let v = ld(m, max);
    m.set_f32(max, if v.lt(h0) { h0 } else { v }.to_f32());
    let v = ld(m, max + 4);
    m.set_f32(max + 4, if v.lt(top) { top } else { v }.to_f32());
    m.release(16);
}

/// Grows two corners by the sons of an element on one side
/// [soft 0x0833ba2c schemComputeElemRelBBox] (`backward` says which side `rels` is; the
/// counts are read again at each step). A son of a chapter the view does not show while
/// the word at +0x20 of `SI_glbSchem` is set (a material or texture without bit 8 of the
/// byte at +0x0c, a curve without bit 4) is only marked laid out (+0x58 = 1). Otherwise,
/// with `done`, a son already laid out is passed over, and so is a material or texture
/// unless `elem` is the one it hangs under; the others go to `SI_schemComputeElemBBox`
/// with the chapter of the relation, and are marked laid out after.
pub fn schem_compute_elem_rel_bbox(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, rels: Ptr32<Relations>, backward: i16, min: u32, max: u32, done: i16) {
    let glb = SI_GLB_SCHEM.addr();
    let mut g: i32 = 0;
    while g < m[rels].nb_groups && m.fault().is_none() {
        let list: Ptr32<RelList> = m[rels].groups.add(g);
        g += 1;
        let mut k: i32 = 0;
        while k < m[list].nb && m.fault().is_none() {
            let rel: Ptr32<ARelation> = m[list].rels.add(k);
            k += 1;
            if !makes_a_son(m[rel].relid, backward != 0) {
                continue;
            }
            let (other, chap) = other_end(m, ed, rel);
            let (shown, skip) = (m.u8(glb + schem::SHOWN), m.i16(glb + schem::SKIP_HIDDEN) != 0);
            let hidden_matter = is_matter(m, chap) && shown & schem::SHOW_MATTER == 0 && skip;
            if hidden_matter || (chap == m[SI_CID][cid::ANIMATION] && shown & schem::SHOW_CURVES == 0 && skip) {
                m[other].work_flag = 1;
                continue;
            }
            if done != 0 && m[other].work_flag != 0 {
                continue;
            }
            if is_matter(m, chap) && is_primary_matter_relation(m, o, ed, other, elem) as i16 == 0 {
                continue;
            }
            let chap = m[rel].chapid as i32;
            schem_compute_elem_bbox(m, o, ed, other, chap, min, max, done);
            m[other].work_flag = 1;
        }
    }
}

/// One operation of the schematic view on an element's hierarchy
/// [soft 0x0833c5dc SI_schematicHierOperation]. 0 for a null element or one not in use.
/// With a grafport, the 32 bits at +0x0c of `SI_glbSchem` become the 16-bit mode word
/// (+6) of what its +0x2c points to, and those at +0x10 become 1. By `op`:
///
/// - 10 (only for an element whose +0x18 is 0), 11 (only when it is above 0): for the
///   chapter of `SI_cid`'s ninth word, `schemCollapseNslElem(grafport, dictionary,
///   element, op)`. For any other, the root (`SI_elemdictGetElem` of the current
///   dictionary, the chapter, the element's +0x50) has its marks cleared
///   (`si_schemElemFlag`, no parent, not relative), `si_schemElemOper(dictionary,
///   element, op, 1, 0.0, 0.0)` runs, a material or texture takes the chapter of the
///   element it hangs under (`getPrimaryMatterRelationParent`, when there is one), and
///   `schemUpdateHierarchy(dictionary, chapter, root, grafport, op)` runs.
/// - 14 (only when +0x18 is above 0): the same but for the matter and
///   `schemUpdateHierarchy`.
/// - 20: while the 16 bits at +0x48 of `SI_glbSchem` are set, the root and
///   `schemUpdateHierarchy`.
/// - 22: the element's box (`SI_schemComputeElemBBox`, corners at 1e10 and −1e10 on the
///   stack, `done` 0), then `SI_schemComputeHierarchiesBBox(dictionary, 1, 0)`.
///
/// Anything else does nothing more. 0 where an element's +0x18 refuses the operation,
/// 1 otherwise.
pub fn schematic_hier_operation(m: &mut Mem, o: &mut dyn Outside, port: u32, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, chap: i32, op: i32) -> i32 {
    if elem.is_null() || m[elem].status != 0 {
        return 0;
    }
    let glb = SI_GLB_SCHEM.addr();
    if port != 0 {
        let what = m.u32(port.wrapping_add(0x2c));
        let mode = m.i16(what.wrapping_add(6)) as i32;
        m.set_i32(glb + schem::SHOWN, mode);
        m.set_i32(glb + schem::WHOLE, 1);
    }
    let k = op.wrapping_sub(10) as u32;
    // (the original tests the element for null again in the cases 10, 11, 14, 20, 21, 23)
    match k {
        0 if m[elem].schem_collapse != 0 => return 0,
        1 | 4 if m[elem].schem_collapse < 1 => return 0,
        _ => {}
    }
    let root = |m: &mut Mem, o: &mut dyn Outside| {
        let cur = current_dict(m);
        let id = m[elem].root_id as u32;
        Ptr32::<Elem>::new(o.call(m, &outside::SI_ELEMDICT_GET_ELEM, &[cur, chap as u32, id]))
    };
    match k {
        0 | 1 | 4 if chap == m[SI_CID][cid::GROUPS] => {
            o.call(m, &outside::SCHEM_COLLAPSE_NSL_ELEM, &[port, ed.addr(), elem.addr(), op as u32]);
        }
        0 | 1 | 4 => {
            let p = root(m, o);
            schem_elem_flag(m, o, ed, Ptr32::NULL, p, 0);
            o.call(m, &outside::SI_SCHEM_ELEM_OPER, &[ed.addr(), elem.addr(), op as u32, 1, 0, 0]);
            if k == 4 {
                return 1;
            }
            let mut chap = chap;
            let ids = m[SI_CID];
            if chap == ids[cid::MATERIALS] || chap == ids[cid::TEXTURES2D] || chap == ids[cid::TEXTURES3D] {
                let found = m.local(4);
                let cur = Ptr32::new(current_dict(m));
                if !get_primary_matter_relation_parent(m, o, cur, elem, Ptr32::new(found)).is_null() {
                    chap = m.i32(found);
                }
                m.release(4);
            }
            o.call(m, &outside::SCHEM_UPDATE_HIERARCHY, &[ed.addr(), chap as u32, p.addr(), port, op as u32]);
        }
        10 if m.i16(glb + GLB_SCHEM_48) != 0 => {
            let p = root(m, o);
            o.call(m, &outside::SCHEM_UPDATE_HIERARCHY, &[ed.addr(), chap as u32, p.addr(), port, op as u32]);
        }
        12 => {
            // the corners: high at -0x14, low at -0xc of the original's frame
            let hi = m.local(16);
            let lo = hi + 8;
            for (at, v) in [(lo, FAR), (lo + 4, FAR), (hi, NEAR), (hi + 4, NEAR)] {
                m.set_f32(at, F80::from_f32(v).to_f32());
            }
            schem_compute_elem_bbox(m, o, ed, elem, chap, lo, hi, 0);
            m.release(16);
            o.call(m, &outside::SI_SCHEM_COMPUTE_HIERARCHIES_BBOX, &[ed.addr(), 1, 0]);
        }
        _ => {}
    }
    1
}

/// The boxes of a chapter's roots, and the scene's box grown by them
/// [soft 0x0833bbf0 schemChapterComputeHierarchiesBBox]; 1 when one root was measured,
/// else 0. The elements are walked in index order (the count and the array read again at
/// each step). A root is an element not yet laid out (+0x58), in use, of level 0
/// (+0x54), with a graph node (+0x08) and, with `only_new`, read by a load (+0x46 = 1);
/// in the chapters of the curves, materials and textures one whose sons are not folded
/// away (+0x18 not above 0). Unless `keep`, its box is measured
/// (`SI_schemComputeElemBBox`, the corners on the stack at 1e10 and −1e10, `done` 1).
/// It is marked laid out, and with a scene element the scene's box (+0x2c) takes each low
/// coordinate unless it is above the scene's, and each high one unless it is below (the
/// root's on a tie or a NaN), through the x87 unit.
pub fn schem_chapter_compute_hierarchies_bbox(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, scene: Ptr32<Elem>, keep: i16, only_new: i16) -> i16 {
    let list: Ptr32<ElemList> = Ptr32::new(m[ed].lists.addr().wrapping_add((chap as u32) << 6));
    let mut answer: i16 = 0;
    let mut i: i32 = 0;
    while i < m[list].nb_elems && m.fault().is_none() {
        let e: Ptr32<Elem> = Ptr32::new(m[list].elems.addr().wrapping_add((i as u32).wrapping_mul(0x84)));
        i += 1;
        if m[e].work_flag != 0 || m[e].status != 0 || m[e].hier_level != 0 || (only_new != 0 && m[e].new_flag != 1) || m[e].aux_data.is_null() {
            continue;
        }
        let ids = m[SI_CID];
        let folding = chap == ids[cid::MATERIALS] || chap == ids[cid::TEXTURES2D] || chap == ids[cid::TEXTURES3D] || chap == ids[cid::ANIMATION];
        if folding && m[e].schem_collapse > 0 {
            continue;
        }
        // the corners: high at -0x10, low at -0x8 of the original's frame
        let hi = m.local(16);
        let lo = hi + 8;
        for (at, v) in [(lo, FAR), (lo + 4, FAR), (hi, NEAR), (hi + 4, NEAR)] {
            m.set_f32(at, F80::from_f32(v).to_f32());
        }
        if keep == 0 {
            schem_compute_elem_bbox(m, o, ed, e, chap, lo, hi, 1);
        }
        m.release(16);
        m[e].work_flag = 1;
        if !scene.is_null() {
            for k in 0..4 {
                let s = F80::from_f32(m[scene].schem_bbox[k]);
                let r = F80::from_f32(m[e].schem_bbox[k]);
                let keep_scene = if k < 2 { r.gt(s) } else { r.lt(s) };
                m[scene].schem_bbox[k] = if keep_scene { s } else { r }.to_f32();
            }
        }
        answer = 1;
    }
    answer
}

/// The boxes of the schematic view's hierarchies, and the scene's box around them
/// [soft 0x0833bd74 SI_schemComputeHierarchiesBBox].
///
/// The dictionary is prepared (`si_schemSortInit(dictionary, 1, 0)`). The scene's element
/// (`SI_edGetElemFromScene`) counts as *empty* when there is none or its box is four zeros
/// (−0 among them, a NaN not); without one, `SI_elemdictCreateDefaultDsc(dictionary)`
/// makes it. Its box starts at 1e10, 1e10, −1e10, −1e10. Then the roots
/// ([`schem_chapter_compute_hierarchies_bbox`], `keep` and `only_new` handed on) of the
/// models' chapter, then of every other chapter the view shows but the curves', in index
/// order (the count read again at each step): materials and textures only while bit 8 of
/// the byte at +0x0c of `SI_glbSchem` is set, the cameras not when the scene was not
/// empty and `only_new` is set. Last the curves' chapter while bit 4 is set. When no
/// root was measured the scene's box is four zeros (written through null when there is
/// no scene element, as in the original; the image records the fault).
pub fn schem_compute_hierarchies_bbox(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, keep: i16, only_new: i16) {
    schem_sort_init(m, o, ed, 1, 0);
    let index = m.local(4);
    let mut scene: Ptr32<Elem> = Ptr32::new(o.call(m, &outside::SI_ED_GET_ELEM_FROM_SCENE, &[index]));
    let mut empty = scene.is_null();
    if !empty {
        let zero = F80::ZERO;
        empty = (0..4).all(|k| F80::from_f32(m[scene].schem_bbox[k]).eq(zero));
    }
    if scene.is_null() {
        scene = Ptr32::new(o.call(m, &outside::SI_ELEMDICT_CREATE_DEFAULT_DSC, &[ed.addr()]));
    }
    if !scene.is_null() {
        let (far, near) = (F80::from_f32(FAR).to_f32(), F80::from_f32(NEAR).to_f32());
        m[scene].schem_bbox = [far, far, near, near];
    }
    let models = m[SI_CID][cid::MODELS];
    let mut answer = schem_chapter_compute_hierarchies_bbox(m, o, ed, models, scene, keep, only_new);
    let mut chap: i32 = 0;
    while chap < m[ed].nb_lists && m.fault().is_none() {
        let ids = m[SI_CID];
        let shown = m.u8(SI_GLB_SCHEM.addr() + schem::SHOWN);
        let wanted = chap != ids[cid::MODELS]
            && chap != ids[cid::ANIMATION]
            && is_shown(m, chap)
            && (!is_matter(m, chap) || shown & schem::SHOW_MATTER != 0)
            && (empty || chap != ids[cid::CAMERAS] || only_new == 0);
        if wanted {
            answer |= schem_chapter_compute_hierarchies_bbox(m, o, ed, chap, scene, keep, only_new);
        }
        chap += 1;
    }
    if m.u8(SI_GLB_SCHEM.addr() + schem::SHOWN) & schem::SHOW_CURVES != 0 {
        let curves = m[SI_CID][cid::ANIMATION];
        answer |= schem_chapter_compute_hierarchies_bbox(m, o, ed, curves, scene, keep, only_new);
    }
    if answer == 0 {
        let s = scene.addr();
        for k in 0..4 {
            m.set_u32(s.wrapping_add(0x2c + 4 * k), 0);
        }
    }
    m.release(4);
}

/// Applies an operation to the sons of an element on one side
/// [soft 0x0833bf8c schemElemRelOper] (`backward` says which side `rels` is; the counts
/// are read again at each step) and answers how many it reached. A son (see the top of
/// the file) is passed over when the operation is 10 and the relation 0x4e22, when it is
/// not in use or already laid out (+0x58), and, for a material or texture, unless `elem`
/// is the one it hangs under. For 11 and 14 the son's +0x18 is brought near `elem`'s
/// first: two more when it is at least three more, one more when it is two more and bit
/// 1 of +0x2a is clear (the differences wrap as 32-bit integers). Each son then goes to
/// `si_schemElemOper(dictionary, son, op, 0, dx, dy)` and is marked laid out.
#[allow(clippy::too_many_arguments)]
pub fn schem_elem_rel_oper(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, rels: Ptr32<Relations>, backward: i16, op: i32, dx: u32, dy: u32) -> i32 {
    let mut count: i32 = 0;
    let mut g: i32 = 0;
    while g < m[rels].nb_groups && m.fault().is_none() {
        let list: Ptr32<RelList> = m[rels].groups.add(g);
        g += 1;
        let mut k: i32 = 0;
        while k < m[list].nb && m.fault().is_none() {
            let rel: Ptr32<ARelation> = m[list].rels.add(k);
            k += 1;
            let relid = m[rel].relid;
            if !makes_a_son(relid, backward != 0) || (op == 10 && relid == 0x4e22) {
                continue;
            }
            let (other, chap) = other_end(m, ed, rel);
            if m[other].status != 0 || m[other].work_flag != 0 {
                continue;
            }
            if is_matter(m, chap) && is_primary_matter_relation(m, o, ed, other, elem) as i16 == 0 {
                continue;
            }
            if op == 11 || op == 14 {
                let own = m[elem].schem_collapse;
                let d = m[other].schem_collapse.wrapping_sub(own);
                if d > 2 {
                    m[other].schem_collapse = own.wrapping_add(2);
                } else if d == 2 && m[other].schem_flags & 2 == 0 {
                    m[other].schem_collapse = own.wrapping_add(1);
                }
            }
            schem_elem_oper(m, o, ed, other, op, 0, dx, dy);
            count += 1;
            m[other].work_flag = 1;
        }
    }
    count
}

/// One operation of the schematic view on an element and everything below it
/// [soft 0x0833c15c si_schemElemOper]. Nothing for an element already laid out (+0x58)
/// or not in use. `dx` and `dy` are the bits of two floats. By `op`:
///
/// - 10: +0x18 one more, and with `first` bit 1 of +0x2a set;
/// - 11: +0x18 one less, not below 0, and with `first` bit 1 cleared;
/// - 14: +0x18 0 and bit 1 cleared;
/// - 21: unless the 16 bits at +0x4c of `SI_glbSchem` are set, `dx` is added to the
///   position, the box's high and low corner: on x (+0x0c, +0x34, +0x2c) when the first
///   word of `SI_glbSchem` is 0, else on y (+0x10, +0x38, +0x30);
/// - 23: unless +0x4c is set and +0x4e clear, as 24;
/// - 24: `dx` added to x, `dy` to y; the sons then get 23.
///
/// The sons follow (`schemElemRelOper`, the backward side first). With `first`, an
/// operation 10 that reached no son takes +0x18 back down and clears bit 1. Last, the
/// element is marked laid out. Each sum is an x87 addition rounded once to a float; `dx`
/// goes on through the x87 unit, `dy` as it came.
#[allow(clippy::too_many_arguments)]
pub fn schem_elem_oper(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, elem: Ptr32<Elem>, op: i32, first: i16, dx: u32, dy: u32) {
    if m[elem].work_flag != 0 || m[elem].status != 0 {
        return;
    }
    let glb = SI_GLB_SCHEM.addr();
    let d = F80::from_f32(f32::from_bits(dx));
    let add = |v: f32, d: F80| (F80::from_f32(v) + d).to_f32();
    let mut op = op;
    match op {
        10 => {
            m[elem].schem_collapse = m[elem].schem_collapse.wrapping_add(1);
            if first != 0 {
                m[elem].schem_flags |= 2;
            }
        }
        11 => {
            let v = m[elem].schem_collapse.wrapping_sub(1);
            m[elem].schem_collapse = v.max(0);
            if first != 0 {
                m[elem].schem_flags &= !2;
            }
        }
        14 => {
            m[elem].schem_collapse = 0;
            m[elem].schem_flags &= !2;
        }
        21 if m.i16(glb + schem::KEEP) == 0 => {
            if m.i32(glb) == 0 {
                m[elem].schem_x = add(m[elem].schem_x, d);
                m[elem].schem_bbox[2] = add(m[elem].schem_bbox[2], d);
                m[elem].schem_bbox[0] = add(m[elem].schem_bbox[0], d);
            } else {
                m[elem].schem_y = add(m[elem].schem_y, d);
                m[elem].schem_bbox[3] = add(m[elem].schem_bbox[3], d);
                m[elem].schem_bbox[1] = add(m[elem].schem_bbox[1], d);
            }
        }
        23 | 24 if op == 24 || m.i16(glb + schem::KEEP) == 0 || m.i16(glb + schem::MOVE_KEPT) != 0 => {
            m[elem].schem_x = add(m[elem].schem_x, d);
            m[elem].schem_y = (F80::from_f32(f32::from_bits(dy)) + F80::from_f32(m[elem].schem_y)).to_f32();
            op = 23;
        }
        _ => {}
    }
    // the float handed on is the one loaded on the x87 unit
    let dx = d.to_f32().to_bits();
    let e = elem.addr();
    let c = schem_elem_rel_oper(m, o, ed, elem, Ptr32::new(e.wrapping_add(0x68)), 1, op, dx, dy);
    let c = c.wrapping_add(schem_elem_rel_oper(m, o, ed, elem, Ptr32::new(e.wrapping_add(0x60)), 0, op, dx, dy));
    if first != 0 && c == 0 && op == 10 {
        m[elem].schem_collapse = m[elem].schem_collapse.wrapping_sub(1);
        m[elem].schem_flags &= !2;
    }
    m[elem].work_flag = 1;
}
