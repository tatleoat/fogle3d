// SPDX-License-Identifier: MIT
//! `edanim.c`: the editing layer over the animation chapter of the element dictionary
//! (finding 0030, section 14). An animation element (the chapter `SI_cid[0]`, 0x84 bytes
//! each) stands for a function curve (flag word at +0x54 not 0, curve at +4) or for an
//! animation file (flag 0; the file's number is the word at +0x50 of the elements that
//! belong to it); the dictionary relates a model, light, camera ... element to the animation
//! elements of its curves and files by relations of type 0x3a97 (and the types 0x3a98 to
//! 0x3aa2 of the files' own relations).
//!
//! Layouts read from the code:
//! * an element: +0 the head record (+0x10 and +0x14 two names, +0x24 the full name made by
//!   `SI_dbFormatFullName`), +4 its data (a curve, a model ...), +8 its graph node, +0x42
//!   and +0x48 half words, +0x50 the animation file's number, +0x54 the curve flag;
//! * the dictionary (0x70 bytes at `SI_elemDicts + SI_procData->dict × 0x70`): +4 the array of
//!   chapters, 0x40 bytes each: +4 the count, +8 the elements.
//!
//! Everything of another source file is reached through [`Outside`].

use crate::globals as g;
use crate::outside::{self as out, Outside, Routine};
use si_mem::{clib, Mem, Ptr32};
use si_x87::{x, F80};

si_mem::globals! {
    /// edanim.c's text buffer: the animation files' names are made in it.
    ED_WORK_STR: [u8; 0x100] = 0x08784c40, "workStr";
}

/// Writes the initial values of this module's globals.
pub fn init(m: &mut Mem) {
    si_mem::init_globals(m, ALL);
    // the one-character literals of `SI_edElementAttCompValue` and `getDefaultValue`
    for a in [0x086809ce_u32, 0x086809d0, 0x086809d2] {
        m.put_rom_cstr(a, b" ");
    }
}

/// The address `SI_RD`, the relation dictionary (passed by address).
const SI_RD: u32 = 0x087815f0;
/// The address `SI_ATD`, the attribute dictionary (passed by address).
const SI_ATD: u32 = 0x08781438;
/// The address `SI_EVD`, the enumeration dictionary (passed by address).
const SI_EVD: u32 = 0x08781454;

fn call(m: &mut Mem, o: &mut dyn Outside, r: &'static Routine, args: &[u32]) -> u32 {
    o.call(m, r, args)
}

fn call_f(m: &mut Mem, o: &mut dyn Outside, r: &'static Routine, args: &[u32]) -> F80 {
    o.call_f(m, r, args)
}

fn dict(m: &Mem) -> u32 {
    g::cur_dict(m)
}

fn cid(m: &Mem, i: usize) -> u32 {
    m[g::SI_CID][i] as u32
}

/// The animation chapter of the current dictionary: `dict->+4 + SI_cid[0] × 0x40`.
fn anim_chapter(m: &Mem) -> u32 {
    let d = dict(m);
    m.u32(d.wrapping_add(4)).wrapping_add(cid(m, 0) << 6)
}

/// The current graph list: `SI_grafLists + SI_procData->+4 × 0x24`.
fn graflist(m: &Mem) -> u32 {
    let proc = m[g::SI_PROC_DATA];
    m[crate::scene1::SI_GRAF_LISTS].wrapping_add(m.u32(proc.wrapping_add(4)).wrapping_mul(0x24))
}

/// `SI_elemdictSortAnimElements` on the current dictionary's animation chapter.
fn sort_anim(m: &mut Mem, o: &mut dyn Outside) {
    let ch = anim_chapter(m);
    call(m, o, &out::SI_ELEMDICT_SORT_ANIM_ELEMENTS, &[ch]);
}

/// [soft 0x080edd90 SI_edIsElementAnimated]
///
/// Whether `elem` (of chapter `chapter`) has animation, 1 or 0 (a 16-bit answer). Nothing
/// without the dictionary or the element. With `cams` (the fifth argument) and a camera or
/// light chapter the element's graph node is read: node types 5 and 7 note an "interest" kind
/// and types 4 and 6 replace the element by the one `SI_elemdictFindElemFromGrafNode` finds
/// (which may also change the chapter). Then, for the relation modes 0, 1, 2 and 5, the
/// elements related by type 0x3a97 in the animation chapter are walked: relation types
/// 0x3a98 to 0x3aa2 never count; with no camera or light chapter any other counts; with one,
/// for a kind-1 node only the types 0x45a, 0x45b, 0x842, 0x843 count and for any other node
/// all but those. For a model (and `models`, the fourth argument) the answer is that of the
/// elements the model's data names at +0x2c4 and then +0x2c0 (each looked up with
/// `SI_edGetElemFromModel` and asked again with both flags 0).
pub fn ed_is_element_animated(m: &mut Mem, o: &mut dyn Outside, dict: u32, chapter: u32, elem: u32, models: i16, cams: i16) -> i32 {
    let sp = m.sp();
    let (lc, l4, l8, lcm) = (m.local(4), m.local(4), m.local(4), m.local(4));
    m.set_u32(lc, chapter);
    let r = (|| {
        let mut elem = elem;
        if dict == 0 || elem == 0 {
            return 0;
        }
        let ch = m.u32(lc);
        let l16 = cams != 0 && (ch == cid(m, 1) || ch == cid(m, 6));
        let mut l18 = false;
        if l16 {
            let gn = m.u32(elem.wrapping_add(8));
            if gn != 0 {
                let ty = m.u32(gn);
                if ty == 5 || ty == 7 {
                    l18 = true;
                }
                if ty == 4 || ty == 6 {
                    elem = call(m, o, &out::SI_ELEMDICT_FIND_ELEM_FROM_GRAF_NODE, &[gn, lc, l4]);
                }
            }
        }
        let cid0 = cid(m, 0);
        for mode in 0u32..=5 {
            if mode.wrapping_sub(3) <= 1 {
                continue;
            }
            let args = [dict, elem, 1, mode, 0, 0x3a97, cid0, l8, l4];
            let mut r = call(m, o, &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM, &args);
            while r != 0 {
                let code = m.u32(l8);
                let skip = if code.wrapping_sub(0x3a98) <= 10 {
                    true
                } else if !l16 {
                    return 1;
                } else {
                    let in_set = code.wrapping_sub(0x45a) <= 1 || code == 0x842 || code == 0x843;
                    if in_set == l18 {
                        return 1;
                    }
                    true
                };
                debug_assert!(skip);
                r = call(m, o, &out::SI_ELEMDICT_FIND_RANGE_NEXT_RELATED_ELEM, &args);
            }
        }
        let ch = m.u32(lc);
        let mut res: i32 = 0;
        if models != 0 && ch == cid(m, 8) {
            let model = m.u32(elem.wrapping_add(4));
            let a = m.u32(model.wrapping_add(0x2c4));
            if a != 0 {
                let e = call(m, o, &out::SI_ED_GET_ELEM_FROM_MODEL, &[a, lcm]);
                if e != 0 {
                    res = ed_is_element_animated(m, o, dict, ch, e, models, 0);
                }
            }
            if res as i16 == 0 {
                let b = m.u32(model.wrapping_add(0x2c0));
                if b != 0 {
                    let e = call(m, o, &out::SI_ED_GET_ELEM_FROM_MODEL, &[b, lcm]);
                    if e != 0 {
                        res = ed_is_element_animated(m, o, dict, ch, e, models, 0);
                    }
                }
            }
        }
        res as i16 as i32
    })();
    m.set_sp(sp);
    r
}

/// [soft 0x080edfd4 si_edIsAnimRootElemSelected]
///
/// 1 when the animation chapter has an element with neither of the flag words (+0x48, +0x54)
/// set (a root) and one of those roots has a related element (relation types 0x3a98 to
/// 0x3aa2, any chapter) that has data, both flag words 0 and a non-zero half word at +0x42.
pub fn ed_is_anim_root_elem_selected(m: &mut Mem, o: &mut dyn Outside, dict: u32) -> u32 {
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    let ch = m.u32(dict.wrapping_add(4)).wrapping_add(cid(m, 0) << 6);
    let mut roots = 0u32;
    let n = m.i32(ch.wrapping_add(4));
    if n > 0 {
        let mut e = m.u32(ch.wrapping_add(8));
        for _ in 0..n {
            if m.u16(e.wrapping_add(0x48)) == 0 && m.u32(e.wrapping_add(0x54)) == 0 {
                roots += 1;
            }
            e = e.wrapping_add(0x84);
        }
    }
    let mut result = 0;
    if roots != 0 {
        let mut i = 0i32;
        let mut off = 0u32;
        while i < m.i32(ch.wrapping_add(4)) {
            let e = off.wrapping_add(m.u32(ch.wrapping_add(8)));
            if m.u16(e.wrapping_add(0x48)) == 0 && m.u32(e.wrapping_add(0x54)) == 0 {
                let r = call(m, o, &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM, &[dict, e, 0, 0, 0x3a98, 0x3aa2, u32::MAX, l8, l4]);
                if r != 0
                    && m.u32(r.wrapping_add(4)) != 0
                    && m.u16(r.wrapping_add(0x48)) == 0
                    && m.u32(r.wrapping_add(0x54)) == 0
                    && m.u16(r.wrapping_add(0x42)) != 0
                {
                    result = 1;
                    break;
                }
            }
            off = off.wrapping_add(0x84);
            i += 1;
        }
    }
    m.set_sp(sp);
    result
}

/// [soft 0x080ef100 SI_edCheckAnimFileDispose]
///
/// After curves were taken off an element: when the element's animation file (number at
/// +0x50, 0 for none; a negative number answers 0) has no related animation element left
/// (relation `SI_relGetAnimFRelFromChapter(chapter)`) that is not animated, the file's
/// element loses its hierarchy record (disposed and freed), its graph node and itself
/// (`SI_elemdictSetDeleteElem`). The animation chapter is sorted and 1 is answered whenever
/// the file number was not negative.
pub fn ed_check_anim_file_dispose(m: &mut Mem, o: &mut dyn Outside, elem: u32, chapter: u32) -> u32 {
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    let file = m.i32(elem.wrapping_add(0x50));
    if file < 0 {
        m.set_sp(sp);
        return 0;
    }
    let d = dict(m);
    let esi = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, chapter, file as u32]);
    let rel = call(m, o, &out::SI_REL_GET_ANIM_F_REL_FROM_CHAPTER, &[chapter]);
    let cid0 = cid(m, 0);
    let d = dict(m);
    let ebx = call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, esi, 1, 0, rel, cid0, l8, l4]);
    if ebx != 0 {
        let d = dict(m);
        let animated = ed_is_element_animated(m, o, d, chapter, esi, 1, 0) as i16;
        if animated == 0 {
            let h = m.u32(ebx.wrapping_add(4));
            if h != 0 {
                call(m, o, &out::SI_HIERANIM_DISPOSE, &[h]);
            }
            let gn = m.u32(ebx.wrapping_add(8));
            if gn != 0 {
                let gl = graflist(m);
                call(m, o, &out::SI_GRAFLIST_DELETE_GRAFNODE, &[gl, gn]);
            }
            let h = m.u32(ebx.wrapping_add(4));
            if h != 0 {
                m.free(h);
                m.set_u32(ebx.wrapping_add(4), 0);
            }
            let (d, idx) = (dict(m), m.u32(l4));
            call(m, o, &out::SI_ELEMDICT_SET_DELETE_ELEM, &[d, cid0, ebx, idx, u32::MAX, 1, 1]);
        }
    }
    sort_anim(m, o);
    m.set_sp(sp);
    1
}

/// [soft 0x080ee0a0 SI_edFcvFilterDispose]
///
/// Deletes the animation elements of the curves related to an element of `chapter` whose
/// data is `data` (`SI_elemdictFindElemByData`), by relations `(rel_a, rel_b)`, whose curve
/// type is in the range `ty` (`SI_fcvCheckTypeRange(curve, ty, ty)`), from the `from`-th
/// matching one on (`from` ≤ 0: all of them; the count restarts when a curve does not
/// match). A positive `from` ends the walk at the first deleted curve with a check of the
/// element's animation file (and a sort when that left it); otherwise, after the walk, the
/// same check and sort.
pub fn ed_fcv_filter_dispose(m: &mut Mem, o: &mut dyn Outside, data: u32, chapter: u32, rel_a: u32, rel_b: u32, ty: u32, from: i32) {
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    let mut count = 0i32;
    let cid0 = cid(m, 0);
    let d = dict(m);
    let idx = call(m, o, &out::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[d, chapter, data]);
    let d = dict(m);
    let x = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, chapter, idx]);
    if x == 0 {
        m.set_sp(sp);
        return;
    }
    let d = dict(m);
    let mut edi = call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, x, 1, rel_a, rel_b, cid0, l8, l4]);
    while edi != 0 {
        let fcv = m.u32(edi.wrapping_add(4));
        let hit = call(m, o, &out::SI_FCV_CHECK_TYPE_RANGE, &[fcv, ty, ty]) as u16 != 0;
        let mut del = false;
        if hit {
            count = count.wrapping_add(1);
            del = from <= 0 || count == from;
        } else {
            count = 0;
        }
        if del {
            let (d, i4) = (dict(m), m.u32(l4));
            call(m, o, &out::SI_ELEMDICT_SET_DELETE_ELEM, &[d, cid0, edi, i4, idx, 1, 1]);
            if from > 0 {
                if ed_check_anim_file_dispose(m, o, x, chapter) as u16 != 0 {
                    m.set_sp(sp);
                    return;
                }
                sort_anim(m, o);
                m.set_sp(sp);
                return;
            }
        }
        let d = dict(m);
        let args = [d, x, 1, rel_a, rel_b, cid0, l8, l4];
        edi = if del {
            call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &args)
        } else {
            call(m, o, &out::SI_ELEMDICT_FIND_NEXT_RELATED_ELEM, &args)
        };
    }
    if ed_check_anim_file_dispose(m, o, x, chapter) as u16 == 0 {
        sort_anim(m, o);
    }
    m.set_sp(sp);
}

/// [soft 0x080ee2f0 SI_edFcvDispose]
///
/// Deletes every animation element related (range `rel_a` to `rel_b`, any mode `mode`) to the
/// element of `chapter` whose data is `data`, one at a time (the walk starts again from the
/// first after each deletion), then checks the element's animation file
/// (`SI_edCheckAnimFileDispose`) and sorts the chapter when that left it. Nothing when the
/// data has no element.
pub fn ed_fcv_dispose(m: &mut Mem, o: &mut dyn Outside, data: u32, chapter: u32, mode: u32, rel_a: u32, rel_b: u32) {
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    let cid0 = cid(m, 0);
    let d = dict(m);
    let idx = call(m, o, &out::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[d, chapter, data]);
    if idx == u32::MAX {
        m.set_sp(sp);
        return;
    }
    let d = dict(m);
    let edi = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, chapter, idx]);
    if edi == 0 {
        m.set_sp(sp);
        return;
    }
    let d = dict(m);
    let args = [d, edi, 1, mode, rel_a, rel_b, cid0, l8, l4];
    let mut r = call(m, o, &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM, &args);
    while r != 0 {
        let (d, i4) = (dict(m), m.u32(l4));
        call(m, o, &out::SI_ELEMDICT_SET_DELETE_ELEM, &[d, cid0, r, i4, idx, 1, 1]);
        let d = dict(m);
        let args = [d, edi, 1, mode, rel_a, rel_b, cid0, l8, l4];
        r = call(m, o, &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM, &args);
    }
    if ed_check_anim_file_dispose(m, o, edi, chapter) as u16 == 0 {
        sort_anim(m, o);
    }
    m.set_sp(sp);
}

/// [soft 0x080ef280 elemdictGetAnimFileName]
///
/// The name an animation file takes: for the relation types 0x3a99, 0x3a9b to 0x3aa0 the
/// text `name` followed by the first three characters of the relation's name (`sprintf
/// "%s%.3s"` into edanim.c's `workStr`), for any other type `name` itself.
pub fn elemdict_get_anim_file_name(m: &mut Mem, o: &mut dyn Outside, name: u32, ty: u32) -> u32 {
    let k = ty.wrapping_sub(0x3a98);
    if k <= 10 && matches!(k, 1 | 3 | 4 | 5 | 6 | 7 | 8) {
        let rn = call(m, o, &out::SI_RELDICT_GET_REL_NAME, &[SI_RD, ty]);
        let mut text = cstr_p(m, name);
        let rel = cstr_p(m, rn);
        text.extend(rel.iter().take(3));
        m.put_cstr(ED_WORK_STR.addr(), &text);
        ED_WORK_STR.addr()
    } else {
        name
    }
}

/// Frees the block at the word `at` and stores 0 there, when it holds one.
fn free_field(m: &mut Mem, at: u32) {
    let p = m.u32(at);
    if p != 0 {
        m.free(p);
        m.set_u32(at, 0);
    }
}

/// [soft 0x080ee73c SI_edSetAnimFileName]
///
/// Names the animation file element `el` (file number `idx`). With `set` (a 16-bit flag):
/// `name_a`, when given, becomes a copy as the head record's first name (+0x10, the old one
/// freed); the second name (+0x14) becomes a copy of `elemdictGetAnimFileName(name_b, ty)`,
/// made unique by `SI_elemdictGetNewName` when `unique` (the sixth flag) finds another
/// animation element with that name and prefix, otherwise taken as it is; the full name
/// (+0x24) is made again (`SI_dbFormatFullName`). With `rename` (16-bit) and a `prefix`: every
/// other element of the animation chapter with flag word +0x48 0, file number `idx`, and a
/// first name starting with `prefix` takes `name_b` as a copy for its first name and gets its
/// full name made again.
#[allow(clippy::too_many_arguments)]
pub fn ed_set_anim_file_name(m: &mut Mem, o: &mut dyn Outside, d: u32, el: u32, idx: u32, name_a: u32, name_b: u32, prefix: u32, ty: u32, set: i16, rename: i16, unique: i16, check: i16) {
    let sp = m.sp();
    let cid0 = cid(m, 0);
    let mut unique = unique;
    if set != 0 {
        if name_a != 0 {
            let head = m.u32(el);
            free_field(m, head.wrapping_add(0x10));
            let c = sdup(m, name_a);
            let head = m.u32(el);
            m.set_u32(head.wrapping_add(0x10), c);
        }
        let head = m.u32(el);
        free_field(m, head.wrapping_add(0x24));
        let nm = elemdict_get_anim_file_name(m, o, name_b, ty);
        let ebx = sdup(m, nm);
        if check != 0 {
            let r = call(m, o, &out::SI_ELEMDICT_FIND_ELEM_BY_PRJ_NAME, &[d, cid0, el, name_a, ebx, 0, u32::MAX, 0]);
            if r == u32::MAX {
                unique = 0;
            }
        }
        if unique != 0 {
            let head = m.u32(el);
            free_field(m, head.wrapping_add(0x14));
            let nn = call(m, o, &out::SI_ELEMDICT_GET_NEW_NAME, &[d, cid0, ebx]);
            let c = sdup(m, nn);
            let head = m.u32(el);
            m.set_u32(head.wrapping_add(0x14), c);
            if ebx != 0 {
                m.free(ebx);
            }
        } else {
            let head = m.u32(el);
            free_field(m, head.wrapping_add(0x14));
            let head = m.u32(el);
            m.set_u32(head.wrapping_add(0x14), ebx);
        }
        let head = m.u32(el);
        let full = call(m, o, &out::SI_DB_FORMAT_FULL_NAME, &[head]);
        let head = m.u32(el);
        m.set_u32(head.wrapping_add(0x24), full);
    }
    if rename != 0 && prefix != 0 {
        let n = clib::strlen(m, prefix);
        let ch = m.u32(d.wrapping_add(4)).wrapping_add(cid0 << 6);
        let mut i = 0i32;
        let mut off = 0u32;
        while i < m.i32(ch.wrapping_add(4)) {
            let e = off.wrapping_add(m.u32(ch.wrapping_add(8)));
            if m.u16(e.wrapping_add(0x48)) == 0 && m.u32(e.wrapping_add(0x50)) == idx && e != el {
                let head = m.u32(e);
                let first = m.u32(head.wrapping_add(0x10));
                if first != 0 && clib::strncmp(m, first, prefix, n) == 0 {
                    let head = m.u32(e);
                    free_field(m, head.wrapping_add(0x10));
                    free_field(m, head.wrapping_add(0x24));
                    let c = sdup(m, name_b);
                    let head = m.u32(e);
                    m.set_u32(head.wrapping_add(0x10), c);
                    let head = m.u32(e);
                    let full = call(m, o, &out::SI_DB_FORMAT_FULL_NAME, &[head]);
                    let head = m.u32(e);
                    m.set_u32(head.wrapping_add(0x24), full);
                }
            }
            off = off.wrapping_add(0x84);
            i += 1;
        }
    }
    m.set_sp(sp);
}

/// [soft 0x080ef2d0 edAddAnimFile]
///
/// The animation file element of `e`'s file number in `chapter`: if the file element already
/// has a related animation element (relation `SI_relGetAnimFRelFromChapter`), that one is
/// answered, its index stored through `out_idx`. Otherwise, with `ask` (16 bits) set and
/// `e` not animated (`SI_edIsElementAnimated(chapter, file, 1, 0)`), 0; else a new element is
/// made: a list node (`SI_dbListNodeAllocate`) added to the animation chapter, named
/// (`SI_edSetAnimFileName` with the file's own names, relation type, all flags set but the
/// second) with a new hierarchy record (`SI_hieranimAllocate`), a graph node (type 0x12) and
/// the relation to the file; the schematic is told (operation 0x14). The new element is
/// answered and its index stored. (The original takes five arguments, the third unused;
/// the callers pass 0.)
pub fn ed_add_anim_file(m: &mut Mem, o: &mut dyn Outside, e: u32, chapter: u32, ask: i16, out_idx: u32) -> u32 {
    let sp = m.sp();
    let l4 = m.local(4);
    let _ = l4;
    let rel = call(m, o, &out::SI_REL_GET_ANIM_F_REL_FROM_CHAPTER, &[chapter]);
    let res = (|| {
        if (rel as i32) < 0 {
            return 0;
        }
        let d = dict(m);
        let file = m.u32(e.wrapping_add(0x50));
        let edi = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, chapter, file]);
        let cid0 = cid(m, 0);
        let d = dict(m);
        let found = call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, edi, 1, 0, rel, cid0, l4, out_idx]);
        if found != 0 {
            return found;
        }
        if ask != 0 {
            let d = dict(m);
            if ed_is_element_animated(m, o, d, chapter, edi, 1, 0) as i16 == 0 {
                return 0;
            }
        }
        let node = call(m, o, &out::SI_DB_LIST_NODE_ALLOCATE, &[]);
        let mut name = 0u32;
        let head = m.u32(edi);
        let n10 = m.u32(head.wrapping_add(0x10));
        if n10 != 0 {
            name = sdup(m, n10);
        }
        let d = dict(m);
        let esi = call(m, o, &out::SI_ELEMDICT_ADD_ELEM, &[d, cid0, node, 0, 0, 0, 0]);
        let d = dict(m);
        let ebx = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, cid0, esi]);
        let head = m.u32(edi);
        let n14 = m.u32(head.wrapping_add(0x14));
        let d = dict(m);
        ed_set_anim_file_name(m, o, d, ebx, esi, name, n14, 0, rel, 1, 0, 1, 1);
        let h = call(m, o, &out::SI_HIERANIM_ALLOCATE, &[chapter]);
        m.set_u32(ebx.wrapping_add(4), h);
        let gl = graflist(m);
        let gn = call(m, o, &out::SI_GRAFLIST_ADD_DATA, &[gl, h, 0x12, 0]);
        m.set_u32(ebx.wrapping_add(8), gn);
        let d = dict(m);
        let file = m.u32(e.wrapping_add(0x50));
        call(m, o, &out::SI_ELEMDICT_ADD_RELATION, &[d, SI_RD, file, esi, rel, 1, 0]);
        let d = dict(m);
        call(m, o, &out::SI_SCHEMATIC_HIER_OPERATION, &[0, d, e, chapter, 0x14]);
        m.set_u32(out_idx, esi);
        ebx
    })();
    m.set_sp(sp);
    res
}

/// [soft 0x080ee440 SI_edFcvInsert]
///
/// Makes the function curve `fcv` an animation element related to `elem` (of `chapter`),
/// answering the new element's index (−1 without a curve). The element's animation file is
/// found or made first (`edAddAnimFile`); a list node takes the element's information with
/// the curve's name standing as the element's second name for the copy; the node is added
/// to the animation chapter with the curve as data, flagged as a curve (+0x54 = 1, file
/// number in +0x50). The curve's type gives the relation (`SI_attdictGetInfoFromFcurve`):
/// types 0x2af8 and 0x2af9 make an "uncomplete" relation (with `kind` as its last number),
/// any other an ordinary one; a positive `number` replaces the relation's last number. The
/// chapter is sorted and the dope sheet's flags and views are marked.
pub fn ed_fcv_insert(m: &mut Mem, o: &mut dyn Outside, fcv: u32, elem: u32, chapter: u32, idx: u32, number: i32) -> u32 {
    let sp = m.sp();
    if fcv == 0 {
        return u32::MAX;
    }
    let (l4, l8, lc, l10, l14) = (m.local(4), m.local(4), m.local(4), m.local(4), m.local(4));
    ed_add_anim_file(m, o, elem, chapter, 0, l4);
    let head = m.u32(elem);
    let saved = m.u32(head.wrapping_add(0x10));
    let ebx = m.u32(head.wrapping_add(0x14));
    m.set_u32(head.wrapping_add(0x10), ebx);
    let name = call(m, o, &out::SI_FCV_GET_NAME, &[fcv]);
    let head = m.u32(elem);
    m.set_u32(head.wrapping_add(0x14), name);
    let esi = call(m, o, &out::SI_DB_LIST_NODE_ALLOCATE, &[]);
    let (d, head) = (dict(m), m.u32(elem));
    call(m, o, &out::SI_DB_LIST_NODE_COPY_INFO, &[d, head, esi]);
    let head = m.u32(elem);
    m.set_u32(head.wrapping_add(0x10), saved);
    m.set_u32(head.wrapping_add(0x14), ebx);
    let cid0 = cid(m, 0);
    let d = dict(m);
    let node = call(m, o, &out::SI_ELEMDICT_ADD_ELEM, &[d, cid0, esi, fcv, 0, 0, 0]);
    let d = dict(m);
    let e = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, cid0, node]);
    m.set_u32(e.wrapping_add(0x54), 1);
    let file = m.u32(l4);
    m.set_u32(e.wrapping_add(0x50), file);
    let head = m.u32(fcv);
    call(m, o, &out::SI_ATTDICT_GET_INFO_FROM_FCURVE, &[SI_ATD, head, l14, l10, lc, l8]);
    if number > 0 {
        m.set_u32(l8, number as u32);
    }
    let ty = m.u32(l14);
    let d = dict(m);
    if ty.wrapping_sub(0x2af8) <= 1 {
        let (a, b) = (m.u32(lc), m.u32(l8));
        call(m, o, &out::SI_ELEMDICT_ADD_UN_COMPLETE_RELATION, &[d, SI_RD, chapter, cid0, idx, node, ty, a, b, 0, 1]);
    } else {
        let (a, b) = (m.u32(lc), m.u32(l8));
        call(m, o, &out::SI_ELEMDICT_ADD_RELATION, &[d, SI_RD, idx, node, ty, a, b]);
    }
    sort_anim(m, o);
    call(m, o, &out::SI_FAST_SET_DIRTY, &[0, u32::MAX, 1]);
    call(m, o, &out::SI_DOPE_FLAGS_SET, &[1, 0, 1, 1]);
    m.set_sp(sp);
    node
}

/// [soft 0x080ee650 SI_edFcvAdd]
///
/// `SI_edFcvInsert` with no number.
pub fn ed_fcv_add(m: &mut Mem, o: &mut dyn Outside, fcv: u32, elem: u32, chapter: u32, idx: u32) -> u32 {
    ed_fcv_insert(m, o, fcv, elem, chapter, idx, 0)
}

/// [soft 0x080ee678 edElementFcvAdd]
///
/// Adds the curve `fcv` to the element of graph type `graf_type` whose data is `data`: the
/// chapter of the graph type (`SI_dbGrafType2Chapter`), the element found by its data and
/// fetched, then `SI_edFcvAdd`.
pub fn ed_element_fcv_add(m: &mut Mem, o: &mut dyn Outside, data: u32, graf_type: u32, fcv: u32) -> u32 {
    let ch = call(m, o, &out::SI_DB_GRAF_TYPE2_CHAPTER, &[graf_type]);
    let d = dict(m);
    let idx = call(m, o, &out::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[d, ch, data]);
    let d = dict(m);
    let e = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, ch, idx]);
    ed_fcv_add(m, o, fcv, e, ch, idx)
}

/// [soft 0x080eea0c SI_edChapterAnimElemChangeName]
///
/// Renames the animation file of `elem` (of `chapter`): the element is the one itself, or for
/// a curve element (+0x54 not 0) the element its file number names; the animation file
/// element related to it (`SI_relGetAnimFRelFromChapter`) is renamed by
/// `SI_edSetAnimFileName(… name_a, name_b, prefix, rel, set = elem is a file, 1, 1, 1)`.
pub fn ed_chapter_anim_elem_change_name(m: &mut Mem, o: &mut dyn Outside, elem: u32, chapter: u32, prefix: u32, name_a: u32, name_b: u32) {
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    let rel = call(m, o, &out::SI_REL_GET_ANIM_F_REL_FROM_CHAPTER, &[chapter]);
    let mut x = elem;
    if m.u32(elem.wrapping_add(0x54)) != 0 {
        let file = m.u32(elem.wrapping_add(0x50));
        m.set_u32(l4, file);
        let d = dict(m);
        x = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, chapter, file]);
    }
    let cid0 = cid(m, 0);
    let d = dict(m);
    let r = call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, x, 1, 0, rel, cid0, l8, l4]);
    if r != 0 {
        let d = dict(m);
        let set = (m.u32(elem.wrapping_add(0x54)) == 0) as i16;
        let i4 = m.u32(l4);
        ed_set_anim_file_name(m, o, d, r, i4, name_a, name_b, prefix, rel, set, 1, 1, 1);
    }
    m.set_sp(sp);
}

/// [soft 0x080efc64 SI_edDisposeAnimFile]
///
/// Deletes the animation file `a` (its data at `+4` names the file). Nothing (0) without the
/// file or its data or when no element answers to it (`SI_edGetElemFromAnimFile`). When
/// curves belong to the file (animation elements with flag word +0x48 0, other than the
/// file's element, of its number) and the file element's information has a positive count at
/// its +0x6c target, a status message is shown and 0 answered. Otherwise each such curve is
/// freed and its element deleted, then the file's hierarchy record is disposed and freed and
/// the file element deleted; the chapter is sorted and 1 answered.
pub fn ed_dispose_anim_file(m: &mut Mem, o: &mut dyn Outside, a: u32) -> u32 {
    let sp = m.sp();
    let l4 = m.local(4);
    let l8 = m.local(4);
    let res = (|| {
        if a == 0 {
            return 0;
        }
        let data = m.u32(a.wrapping_add(4));
        if data == 0 {
            return 0;
        }
        let edi = call(m, o, &out::SI_ED_GET_ELEM_FROM_ANIM_FILE, &[data, l4]);
        if edi == 0 {
            return 0;
        }
        let ch = anim_chapter(m);
        let info = m.u32(edi.wrapping_add(0x6c));
        let n = m.u32(ch.wrapping_add(4)) as i32;
        let idx = m.u32(l4);
        let mut count = 0;
        let mut e = m.u32(ch.wrapping_add(8));
        for _ in 0..n.max(0) {
            if m.u16(e.wrapping_add(0x48)) == 0 && e != edi && m.u32(e.wrapping_add(0x50)) == idx {
                count += 1;
            }
            e = e.wrapping_add(0x84);
        }
        if count > 0 && (m.i32(info)) > 0 {
            call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x086809b0, 1]);
            return 0;
        }
        let cid0 = cid(m, 0);
        let mut i = 0i32;
        while i < m.i32(ch.wrapping_add(4)) {
            let e = m.u32(ch.wrapping_add(8)).wrapping_add((i as u32).wrapping_mul(0x84));
            if m.u16(e.wrapping_add(0x48)) == 0 && e != edi && m.u32(e.wrapping_add(0x50)) == m.u32(l4) {
                let c = m.u32(e.wrapping_add(4));
                m.set_u32(l8, c);
                call(m, o, &out::SI_FCURVE_REMOVE, &[l8]);
                let d = dict(m);
                call(m, o, &out::SI_ELEMDICT_SET_DELETE_ELEM, &[d, cid0, e, i as u32, u32::MAX, 0, 1]);
            }
            i += 1;
        }
        let h = m.u32(edi.wrapping_add(4));
        call(m, o, &out::SI_HIERANIM_DISPOSE, &[h]);
        let h = m.u32(edi.wrapping_add(4));
        if h != 0 {
            m.free(h);
            m.set_u32(edi.wrapping_add(4), 0);
        }
        let (d, idx) = (dict(m), m.u32(l4));
        call(m, o, &out::SI_ELEMDICT_SET_DELETE_ELEM, &[d, cid0, edi, idx, u32::MAX, 0, 1]);
        call(m, o, &out::SI_ELEMDICT_SORT_ANIM_ELEMENTS, &[ch]);
        1
    })();
    m.set_sp(sp);
    res
}

/// [soft 0x080ef508 si_edMergeAnimFiles]
///
/// Folds the animation file `file_b` into `file_a` (both numbers in `chapter`). Nothing (0)
/// when the chapter has no file relation or `file_b` has no file element. The file element
/// of `file_a` is found, or made (`edAddAnimFile`). With `flag` 0 and both hierarchy records
/// present the lists of actions are merged: an empty first list takes the other's (the
/// other's own disposed first with a status message when it has any), then the second
/// lists' actions are appended with names made unique (`SI_actionsGetNewName`) and the
/// emptied list released. Every element of `file_b`'s number gets `file_a`'s; `file_b`'s
/// graph node (type 0x12) and record are freed and its element deleted. 1 answered.
pub fn ed_merge_anim_files(m: &mut Mem, o: &mut dyn Outside, chapter: u32, file_a: u32, file_b: u32, flag: i16) -> u32 {
    use si_mem::Ptr32;
    let sp = m.sp();
    let l4 = m.local(4);
    let l8 = m.local(4);
    let lc = m.local(4);
    let res = (|| {
        let rel = call(m, o, &out::SI_REL_GET_ANIM_F_REL_FROM_CHAPTER, &[chapter]);
        if (rel as i32) < 0 {
            return 0;
        }
        let cid0 = cid(m, 0);
        let d = dict(m);
        let eb = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, chapter, file_b]);
        let d = dict(m);
        let mut y_elem = call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, eb, 1, 0, rel, cid0, l8, l4]);
        if y_elem == 0 {
            return 0;
        }
        let d = dict(m);
        let edi = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, chapter, file_a]);
        let d = dict(m);
        let mut x_elem = call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, edi, 1, 0, rel, cid0, l8, lc]);
        if x_elem == 0 {
            x_elem = ed_add_anim_file(m, o, edi, chapter, 0, lc);
            let (d, i) = (dict(m), m.u32(l4));
            y_elem = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, cid0, i]);
        }
        if flag == 0 {
            let x = m.u32(x_elem.wrapping_add(4));
            let y = m.u32(y_elem.wrapping_add(4));
            if x != 0 && y != 0 {
                let (xa, ya) = (x.wrapping_add(0xc), y.wrapping_add(0xc));
                if m.i32(xa) == 0 {
                    if m.i32(ya) > 0 {
                        crate::actions::actions_dispose(m, Ptr32::new(ya));
                        call(m, o, &out::SI_STATUS_DISPLAY_MESSAGE, &[0x08680980, 2]);
                    }
                    let p = m.u32(ya.wrapping_add(4));
                    m.set_u32(xa.wrapping_add(4), p);
                    let c = m.u32(y.wrapping_add(0xc));
                    m.set_u32(x.wrapping_add(0xc), c);
                    m.set_u32(ya.wrapping_add(4), 0);
                    m.set_u32(y.wrapping_add(0xc), 0);
                }
                let (x, y) = (m.u32(x_elem.wrapping_add(4)), m.u32(y_elem.wrapping_add(4)));
                let (xl, yl) = (x.wrapping_add(0x14), y.wrapping_add(0x14));
                let yc = m.i32(yl);
                if yc > 0 {
                    let size = ((yc.wrapping_add(m.i32(xl))) as u32).wrapping_mul(0x30);
                    let old = m.u32(xl.wrapping_add(4));
                    let np = m.si_realloc(old, size);
                    m.set_u32(xl.wrapping_add(4), np);
                    let xc = m.u32(xl);
                    let dst = np.wrapping_add(xc.wrapping_mul(0x30));
                    let src = m.u32(yl.wrapping_add(4));
                    clib::memcpy(m, dst, src, (yc as u32).wrapping_mul(0x30));
                    let mut off = 0u32;
                    let mut k = 0;
                    while k < m.i32(yl) {
                        let nm = m.u32(m.u32(yl.wrapping_add(4)).wrapping_add(off));
                        let new = crate::actions::actions_get_new_name(m, Ptr32::new(xl), nm);
                        let (xc, xp) = (m.u32(xl), m.u32(xl.wrapping_add(4)));
                        m.set_u32(xp.wrapping_add(xc.wrapping_mul(0x30)), new);
                        let yp = m.u32(yl.wrapping_add(4));
                        let old = m.u32(yp.wrapping_add(off));
                        if old != 0 {
                            m.free(old);
                            let yp = m.u32(yl.wrapping_add(4));
                            m.set_u32(yp.wrapping_add(off), 0);
                        }
                        let xc = m.u32(xl);
                        m.set_u32(xl, xc.wrapping_add(1));
                        off = off.wrapping_add(0x30);
                        k += 1;
                    }
                    free_field(m, yl.wrapping_add(4));
                    m.set_u32(yl.wrapping_add(4), 0);
                    m.set_u32(yl, 0);
                }
            }
        }
        let ch = anim_chapter(m);
        let (n, base) = (m.i32(ch.wrapping_add(4)), m.u32(ch.wrapping_add(8)));
        let (from, to) = (m.u32(l4), m.u32(lc));
        for i in 0..n.max(0) as u32 {
            let e = base.wrapping_add(i.wrapping_mul(0x84));
            if m.u32(e.wrapping_add(0x50)) == from {
                m.set_u32(e.wrapping_add(0x50), to);
            }
        }
        if m.u32(y_elem.wrapping_add(4)) != 0 {
            let gl = graflist(m);
            let dat = m.u32(y_elem.wrapping_add(4));
            let r = call(m, o, &out::SI_FIND_GL_DATA, &[gl, dat, 0x12]);
            if r != 0 {
                let gl = graflist(m);
                call(m, o, &out::SI_GRAFLIST_DELETE_GRAFNODE, &[gl, r]);
            }
            let dat = m.u32(y_elem.wrapping_add(4));
            if dat != 0 {
                m.free(dat);
                m.set_u32(y_elem.wrapping_add(4), 0);
            }
        }
        let (d, i) = (dict(m), m.u32(l4));
        call(m, o, &out::SI_ELEMDICT_SET_DELETE_ELEM, &[d, cid0, y_elem, i, u32::MAX, 0, 1]);
        1
    })();
    m.set_sp(sp);
    res
}

// ------------------------------------------------------------------------------------------
// the second slice (anim26)

/// The cids of the chapters this slice names: `SI_cid` indices (models 8, lights 6, clusters 2,
/// lattices 5, shapes 21, expressions 0x87816d4 → 29).
const CID_LIGHTS: usize = 6;
const CID_MODELS: usize = 8;
const CID_CLUSTERS: usize = 2;
const CID_LATTICES: usize = 5;
const CID_SHAPES: usize = 21;
const CID_EXPRESSIONS: usize = 29;

/// One text of a name the way `sprintf(buf, "%s<suffix>", ...)` makes it: the pieces joined.
fn join(parts: &[&[u8]]) -> Vec<u8> {
    parts.iter().flat_map(|p| p.iter().copied()).collect()
}

/// The renaming of one related element in `si_edAnimElemChangeName`: the head record's first
/// name replaced by a copy of `a` when given, the second name and the full name made again
/// from `b` + `tail` (also written to `workStr`), then the element's own animation elements
/// renamed (`SI_edChapterAnimElemChangeName` with `old` + `tail` made in a local buffer).
/// (The texts are made after the old names are freed, as the original does.)
/// `strdup` as the stand-in library has it: a null source gives a copy of the empty string
/// (the real C library would fault there; the check vectors reach it with a null name).
fn sdup(m: &mut Mem, p: u32) -> u32 {
    if p == 0 {
        let c = m.malloc(1);
        if c != 0 {
            m.set_u8(c, 0);
        }
        c
    } else {
        m.strdup(p)
    }
}

/// A `%s` argument: the C library prints "(null)" for a null pointer.
fn cstr_p(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 { b"(null)".to_vec() } else { m.cstr(p) }
}

fn rename_related(m: &mut Mem, o: &mut dyn Outside, e: u32, chapter: u32, a: u32, b: u32, old: u32, tail: &[u8], buf: u32) {
    let head = m.u32(e);
    if a != 0 {
        free_field(m, head.wrapping_add(0x10));
        let c = sdup(m, a);
        let head = m.u32(e);
        m.set_u32(head.wrapping_add(0x10), c);
    }
    let head = m.u32(e);
    free_field(m, head.wrapping_add(0x14));
    let head = m.u32(e);
    free_field(m, head.wrapping_add(0x24));
    let nb = join(&[&cstr_p(m, b), tail]);
    m.put_cstr(ED_WORK_STR.addr(), &nb);
    let no = join(&[&cstr_p(m, old), tail]);
    m.put_cstr(buf, &no);
    let c = sdup(m, ED_WORK_STR.addr());
    let head = m.u32(e);
    m.set_u32(head.wrapping_add(0x14), c);
    let head = m.u32(e);
    let full = call(m, o, &out::SI_DB_FORMAT_FULL_NAME, &[head]);
    let head = m.u32(e);
    m.set_u32(head.wrapping_add(0x24), full);
    ed_chapter_anim_elem_change_name(m, o, e, chapter, buf, a, ED_WORK_STR.addr());
}

/// [soft 0x080eeae4 si_edAnimElemChangeName]
///
/// Renames what hangs on an element and then its own animation elements
/// (`SI_edChapterAnimElemChangeName`). `old` and `b` are the old and new names (`a`, when
/// given, becomes the related elements' first name). For a model the elements related with
/// it are renamed first: its shape (relation 0xc8 to the SHAPES chapter: the names with
/// "Shp" appended), every cluster (0x2bc: the name followed by the cluster's own name), a
/// lattice (0xd2: "NLat") and another (0xd7: "BLat"). For a light whose data has the word
/// 2 at +0x1e the element is replaced by the light's own one (`SI_edGetElemFromLight`), the
/// old name by a copy of its second name and the new by a copy of `b` + "_int" (both freed
/// at the end).
pub fn ed_anim_elem_change_name(m: &mut Mem, o: &mut dyn Outside, elem: u32, chapter: u32, old: u32, a: u32, b: u32) {
    let sp = m.sp();
    let buf = m.local(0x48);
    let (l80, l84, l88, l8c, l90) = (m.local(4), m.local(4), m.local(4), m.local(4), m.local(4));
    let (mut elem, mut old, mut b) = (elem, old, b);
    let mut made = false;
    if chapter == cid(m, CID_MODELS) {
        let d = dict(m);
        let cs = cid(m, CID_SHAPES);
        let e = call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, elem, 1, 2, 0xc8, cs, l84, l80]);
        if e != 0 {
            rename_related(m, o, e, cs, a, b, old, b"Shp", buf);
        }
        let d = dict(m);
        let cc = cid(m, CID_CLUSTERS);
        let mut e = call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, elem, 1, 2, 0x2bc, cc, l84, l88]);
        while e != 0 {
            let data = m.u32(e.wrapping_add(4));
            let nm = m.u32(data);
            let name = cstr_p(m, nm);
            rename_related(m, o, e, cc, a, b, old, &name, buf);
            let d = dict(m);
            e = call(m, o, &out::SI_ELEMDICT_FIND_NEXT_RELATED_ELEM, &[d, elem, 1, 2, 0x2bc, cc, l84, l88]);
        }
        let cl = cid(m, CID_LATTICES);
        for (rel, suffix) in [(0xd2_u32, &b"NLat"[..]), (0xd7, &b"BLat"[..])] {
            let d = dict(m);
            let e = call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, elem, 1, 2, rel, cl, l84, l8c]);
            if e != 0 {
                rename_related(m, o, e, cl, a, b, old, suffix, buf);
            }
        }
    } else if chapter == cid(m, CID_LIGHTS) {
        let data = m.u32(elem.wrapping_add(4));
        if m.u16(data.wrapping_add(0x1e)) == 2 {
            elem = call(m, o, &out::SI_ED_GET_ELEM_FROM_LIGHT, &[data, l90]);
            let head = m.u32(elem);
            let n14 = m.u32(head.wrapping_add(0x14));
            old = sdup(m, n14);
            let t = join(&[&cstr_p(m, b), b"_int"]);
            m.put_cstr(ED_WORK_STR.addr(), &t);
            b = sdup(m, ED_WORK_STR.addr());
            made = true;
        }
    }
    ed_chapter_anim_elem_change_name(m, o, elem, chapter, old, a, b);
    if made {
        if old != 0 {
            m.free(old);
        }
        if b != 0 {
            m.free(b);
        }
    }
    m.set_sp(sp);
}

/// [soft 0x080ef8cc si_edSeparateAnimFiles]
///
/// Moves the animation of a model off the animation file it shares with the file of element
/// `b` onto a new file made for it: 0 when the chapter has no animation-file relation, `a`'s
/// element has no related animation element, or `b`'s element is not animated. The new file
/// (`edAddAnimFile` for `b`'s element) takes, without `keep` (the 16-bit last argument), a
/// copy of the old file's master and slave action lists (`SI_actionsCopyAll`, +0xc and +0x14
/// of the files' data). Every animation element of the old file number whose lists hold
/// an entry of the MODELS chapter with root `model->+4` (`SI_modelRoot`) takes the new number
/// (the first entry found, per element, ends its search). Without `keep` the copies are then
/// turned off (`SI_actionsTmpActivateAll` with 1) and the masters' keys written back into the
/// two files (`SI_actionsAddMasterKeysToHA` with 0). Then the old file is checked
/// (`SI_edCheckAnimFileDispose`). Answers 1.
pub fn ed_separate_anim_files(m: &mut Mem, o: &mut dyn Outside, chapter: u32, a: u32, b: u32, model: u32, keep: i16) -> u32 {
    let sp = m.sp();
    let (l4, l8, lc) = (m.local(4), m.local(4), m.local(4));
    let rel = call(m, o, &out::SI_REL_GET_ANIM_F_REL_FROM_CHAPTER, &[chapter]);
    let res = (|| {
        if (rel as i32) < 0 {
            return 0;
        }
        let d = dict(m);
        let l24 = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, chapter, a]);
        let cid0 = cid(m, 0);
        let d = dict(m);
        if call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, l24, 1, 0, rel, cid0, l8, l4]) == 0 {
            return 0;
        }
        let d = dict(m);
        let ebx = call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, chapter, b]);
        let d = dict(m);
        if ed_is_element_animated(m, o, d, chapter, ebx, 1, 0) as i16 == 0 {
            return 0;
        }
        let l28 = ed_add_anim_file(m, o, ebx, chapter, 0, lc);
        let d = dict(m);
        let l2c = call(m, o, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, l24, 1, 0, rel, cid0, l8, l4]);
        let (mut l38, mut l3c) = (0u32, 0u32);
        if keep == 0 {
            let (od, nd) = (m.u32(l2c.wrapping_add(4)), m.u32(l28.wrapping_add(4)));
            if od != 0 && nd != 0 {
                l38 = od.wrapping_add(0xc);
                l3c = nd.wrapping_add(0xc);
                crate::actions::actions_copy_all(m, o, Ptr32::new(l3c), Ptr32::new(l38));
                crate::actions::actions_copy_all(m, o, Ptr32::new(nd.wrapping_add(0x14)), Ptr32::new(od.wrapping_add(0x14)));
            }
        }
        let ch = anim_chapter(m);
        let mut i = 0;
        while i < m.i32(ch.wrapping_add(4)) {
            let e = m.u32(ch.wrapping_add(8)).wrapping_add((i as u32).wrapping_mul(0x84));
            i += 1;
            if m.u32(e.wrapping_add(0x50)) != m.u32(l4) {
                continue;
            }
            let list = e.wrapping_add(0x68);
            let mut found = false;
            let mut j = 0;
            while j < m.i32(list) && !found {
                let item = m.u32(list.wrapping_add(4)).wrapping_add((j as u32).wrapping_mul(24));
                j += 1;
                let mut k = 0;
                while k < m.i32(item) && !found {
                    let ent = m.u32(item.wrapping_add(0xc)).wrapping_add((k as u32).wrapping_mul(32));
                    let ech = m.i16(ent.wrapping_add(0x10)) as i32 as u32;
                    if ech == cid(m, CID_MODELS) {
                        let d = m.u32(dict(m).wrapping_add(4));
                        let elems = m.u32(d.wrapping_add(ech << 6).wrapping_add(8));
                        let x = m.u32(ent.wrapping_add(4));
                        let data = m.u32(elems.wrapping_add(x.wrapping_mul(0x84)).wrapping_add(4));
                        let root = call(m, o, &out::SI_MODEL_ROOT, &[data]);
                        if root == m.u32(model.wrapping_add(4)) {
                            let nf = m.u32(lc);
                            m.set_u32(e.wrapping_add(0x50), nf);
                            found = true;
                        }
                    }
                    k += 1;
                }
            }
        }
        if keep == 0 {
            let d = dict(m);
            crate::actions::actions_tmp_activate_all(m, o, d, Ptr32::new(l38), 1);
            let d = dict(m);
            crate::actions::actions_tmp_activate_all(m, o, d, Ptr32::new(l3c), 1);
            let d = dict(m);
            crate::actedit::actions_add_master_keys_to_ha(m, o, d, l2c, 0);
            let d = dict(m);
            crate::actedit::actions_add_master_keys_to_ha(m, o, d, l28, 0);
        }
        ed_check_anim_file_dispose(m, o, l24, chapter);
        1
    })();
    m.set_sp(sp);
    res
}

/// [soft 0x080f0184 SI_edElementAttValueSet]
///
/// Sets attribute `attr` of the element record `rec` (+0 chapter, +4 index) to `value` through
/// the attribute group's setter (the code address at +0x1c of the group's record, called with
/// the record, the index and the value, answering 16 bits): 0 when the attribute is not valid
/// for the record, has no group, the element does not exist or the group has no setter.
pub fn ed_element_att_value_set(m: &mut Mem, o: &mut dyn Outside, rec: u32, attr: u32, value: u32) -> i32 {
    let v = call(m, o, &out::SI_ATTDICT_VALIDATE_ATTR, &[rec, attr]);
    if v as u16 == 0 {
        return 0;
    }
    let grp = call(m, o, &out::SI_ATTDICT_ATTRIB_GROUP_GET_BY_INDEX, &[SI_ATD, attr]);
    if grp == 0 {
        return 0;
    }
    let d = dict(m);
    let (c, i) = (m.u32(rec), m.u32(rec.wrapping_add(4)));
    if call(m, o, &out::SI_ELEMDICT_GET_ELEM, &[d, c, i]) == 0 {
        return 0;
    }
    let f = m.u32(grp.wrapping_add(0x1c));
    if f == 0 {
        return 0;
    }
    o.call_addr(m, f, &[rec, attr, value]) as u16 as i16 as i32
}

/// [soft 0x080f0218 getDefaultValue]
///
/// The starting value of attribute `attr` in the value record `out` (+0 its type, the value
/// from +4): type 1 and 4 and 11 a word 0 at +4 (type 4 and 11: the whole word), 9 a word 0
/// at +8, 5 the address of a one-character literal, 7 a byte 0x20, 2 and 8 a half word 0;
/// the other types leave the value alone.
pub fn get_default_value(m: &mut Mem, o: &mut dyn Outside, attr: u32, out: u32) {
    let at = call(m, o, &out::SI_ATTDICT_ATTRIBUTE_GET_BY_INDEX, &[SI_ATD, attr]);
    let ty = m.u32(at.wrapping_add(0x2c));
    m.set_u32(out, ty);
    match ty.wrapping_sub(1) {
        0 | 3 | 10 => m.set_u32(out.wrapping_add(4), 0),
        8 => m.set_u32(out.wrapping_add(8), 0),
        4 => m.set_u32(out.wrapping_add(4), 0x086809ce),
        6 => m.set_u8(out.wrapping_add(4), 0x20),
        1 | 7 => m.set_u16(out.wrapping_add(4), 0),
        _ => {}
    }
}

/// [soft 0x080f02a0 SI_edElementAttCompValue]
///
/// Compares attribute `attr` of two element records (`SI_edElementAttValueGet` of each into a
/// value record; one that has no value takes the attribute's default). Stores through `res`
/// the order of the first against the second (negative: smaller) and answers 1; answers 0 and
/// stores 0 when neither has a value or the type is not one that compares (6, 10 and above
/// 11). Floats (types 1 and 9, at +4 and +8) compare with −1, 0 or 1 (equal and unordered
/// both 0, the value order being first against second); enumerations (3) by the names of
/// their codes (`SI_enmdictEnumCodeGetByValue`; "no name" is smaller) with `strcmp`;
/// strings (5) with `strcmp` (a null string is smaller); byte, half word and word types by
/// the difference.
pub fn ed_element_att_comp_value(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, attr: u32, res: u32) -> u32 {
    let sp = m.sp();
    let v1 = m.local(12);
    let v2 = m.local(12);
    let r1 = call(m, o, &out::SI_ED_ELEMENT_ATT_VALUE_GET, &[a, attr, v1]) as u16;
    let r2 = call(m, o, &out::SI_ED_ELEMENT_ATT_VALUE_GET, &[b, attr, v2]) as u16;
    let ok = (|| {
        if r1 != 0 {
            if r2 == 0 {
                get_default_value(m, o, attr, v2);
            }
        } else if r2 != 0 {
            get_default_value(m, o, attr, v1);
        } else {
            return false;
        }
        let ty = m.u32(v1);
        let (a4, a8) = (v1.wrapping_add(4), v1.wrapping_add(8));
        let (b4, b8) = (v2.wrapping_add(4), v2.wrapping_add(8));
        // `fcom`: the second against the first; equal and unordered both give 0
        let order = |x: f32, y: f32| if y > x { -1i32 } else if y < x { 1 } else { 0 };
        let r = match ty.wrapping_sub(1) {
            0 => {
                order(m.f32(a4), m.f32(b4))
            }
            8 => {
                order(m.f32(a8), m.f32(b8))
            }
            2 => {
                let n1 = if r1 != 0 { call(m, o, &out::SI_ENMDICT_ENUM_CODE_GET_BY_VALUE, &[SI_EVD, m.u32(a4), m.u32(a8)]) } else { 0x086809d0 };
                let n2 = if r2 != 0 { call(m, o, &out::SI_ENMDICT_ENUM_CODE_GET_BY_VALUE, &[SI_EVD, m.u32(b4), m.u32(b8)]) } else { 0x086809d2 };
                if n1 == 0 {
                    -1
                } else if n2 == 0 {
                    1
                } else {
                    clib::strcmp(m, n1, n2)
                }
            }
            4 => {
                let (s1, s2) = (m.u32(a4), m.u32(b4));
                if s1 == 0 {
                    -1
                } else if s2 == 0 {
                    1
                } else {
                    clib::strcmp(m, s1, s2)
                }
            }
            6 => (m.u8(a4) as i8 as i32).wrapping_sub(m.u8(b4) as i8 as i32),
            1 | 7 => (m.i16(a4) as i32).wrapping_sub(m.i16(b4) as i32),
            3 | 10 => m.i32(a4).wrapping_sub(m.i32(b4)),
            _ => return false,
        };
        m.set_u32(res, r as u32);
        true
    })();
    if !ok {
        m.set_u32(res, 0);
    }
    m.set_sp(sp);
    ok as u32
}

/// The tolerance `si_customGetParam` allows a curve's time (`double` at 0x08646318).
const CUSTOM_TIME_EPS: f64 = 0.0001;

/// [soft 0x080f0450 si_customGetParam]
///
/// The value of a custom parameter into the record `out` (+0 the count 1, +4 the value as a
/// float, +4 cleared first): from the parameter `p` (a float at +0x10) when given; otherwise
/// from `q` (nothing when it is null): while its word +0x60 is set and the time (the current
/// frame in seconds against the float at +0x10) is within the tolerance it keeps the value at
/// +0x14, a time further away clears the word; then the curve (word +0x20 not 0, its value at
/// the current frame, `SI_fcurveFrameValue`) or the float at +0x14. Answers 1.
pub fn custom_get_param(m: &mut Mem, o: &mut dyn Outside, p: u32, q: u32, out: u32) -> u32 {
    m.set_u32(out.wrapping_add(4), 0);
    m.set_u32(out, 1);
    let cur = m[g::SI_FRAME_CURRENT] as u32;
    let value = if p != 0 {
        Some(x(m.f32(p.wrapping_add(0x10))))
    } else if q == 0 {
        None
    } else {
        let mut direct = false;
        if m.u16(q.wrapping_add(0x60)) != 0 {
            let t = call_f(m, o, &out::SI_FRAME_TO_SEC, &[cur]);
            let d = (t - x(m.f32(q.wrapping_add(0x10)))).abs();
            if d.gt(x(CUSTOM_TIME_EPS)) {
                m.set_u16(q.wrapping_add(0x60), 0);
            }
            direct = m.u16(q.wrapping_add(0x60)) != 0;
        }
        if direct || m.u16(q.wrapping_add(0x20)) == 0 {
            Some(x(m.f32(q.wrapping_add(0x14))))
        } else {
            Some(call_f(m, o, &out::SI_FCURVE_FRAME_VALUE, &[q, cur]))
        }
    };
    if let Some(v) = value {
        m.set_u32(out.wrapping_add(4), v.to_f32().to_bits());
    }
    1
}

/// [soft 0x080f06a8 si_customSetParam]
///
/// Sets a custom parameter `p` to the float at +4 of the record `val`: its float at +0x10 and
/// the word +0xc set to 1. (The current frame is turned into seconds and the result dropped.)
/// Answers 1. The original writes the word at +0xc without testing `p`; with a null `p` it
/// would fault there.
pub fn custom_set_param(m: &mut Mem, o: &mut dyn Outside, p: u32, _unused: u32, val: u32) -> u32 {
    let cur = m[g::SI_FRAME_CURRENT] as u32;
    call_f(m, o, &out::SI_FRAME_TO_SEC, &[cur]);
    if p != 0 {
        let v = m.f32(val.wrapping_add(4));
        m.set_u32(p.wrapping_add(0x10), x(v).to_f32().to_bits());
    }
    m.set_u16(p.wrapping_add(0xc), 1);
    1
}

/// [soft 0x080f086c SI_edExprRelGetFromFcvID]
///
/// Finds the expression an element's relation of type 0x4e2a (to an expression whose data's
/// first word is the curve id `id`, any for -1) names, makes the relation the current one by
/// serial number again, and answers its record (0 when none). When the element has a current
/// relation of that type at the start its data word at +8 of that relation's slot
/// (`elem->+0x64`, 24 bytes each) is cleared first and the serial number it had is the one
/// set back.
pub fn ed_expr_rel_get_from_fcv_id(m: &mut Mem, o: &mut dyn Outside, d: u32, elem: u32, id: u32) -> u32 {
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    if call(m, o, &out::SI_ELEMDICT_SET_CURR_REL_POINTER, &[d, elem, 0x4e2a, l4]) as u16 != 0 {
        let r = call(m, o, &out::SI_ELEMDICT_GET_CUR_RELATION, &[d, elem, 1, m.u32(l4)]);
        let serial = m.u32(r.wrapping_add(0xc));
        m.set_u32(l8, serial);
        let tab = m.u32(elem.wrapping_add(0x64));
        let at = tab.wrapping_add(m.u32(l4).wrapping_mul(24)).wrapping_add(8);
        m.set_u32(at, 0);
    }
    let mut first = true;
    let res = loop {
        let r = call(m, o, &out::SI_ELEMDICT_FIND_SERIAL_RELATION, &[d, elem, 0x4e2a, l4, u32::MAX, 1, first as u32]);
        first = false;
        if r == 0 {
            call(m, o, &out::SI_ELEMDICT_SET_REL_TO_SERIALID, &[d, elem, 0x4e2a, m.u32(l8), 1]);
            break 0;
        }
        if m.i16(r.wrapping_add(0x10)) as i32 as u32 == cid(m, CID_EXPRESSIONS) {
            let e = m.u32(r.wrapping_add(0x18));
            if e != 0 && (id == u32::MAX || id == m.u32(e)) {
                call(m, o, &out::SI_ELEMDICT_SET_REL_TO_SERIALID, &[d, elem, 0x4e2a, m.u32(l8), 1]);
                break r;
            }
        }
    };
    m.set_sp(sp);
    res
}
