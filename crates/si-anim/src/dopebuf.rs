// SPDX-License-Identifier: MIT
//! `actions.c`, fourth pass: the dope sheet's buffer. The buffer is a tree built from the
//! elements under a root: a node per element (its name and its lists of curves) and a
//! list per kind of curve, each entry a copy of a function curve's keys inside the
//! actions' windows, shifted by an offset.
//!
//! Layouts read from the code (finding 0030, section 5b):
//! * a curve node (0x6c bytes, `SI_calloc`): +0 a copy of the curve's name, +4 a function
//!   curve record (0x64 bytes), +0x68 the next curve node;
//! * a dope node (0x1c bytes): +0 the element's name (copy), +4 the curve list of the
//!   element's own curves, +8 the first of the model's second kind of node (0xc bytes: +0
//!   name, +4 curve list, +8 next) hanging at +0xc, +0x10 and +0x14 two more curve lists,
//!   +0x18 the next sibling.

use crate::actedit as ae;
use crate::actions::{self as ac, Actions};
use crate::walk::{self, step};
use crate::globals as g;
use crate::outside::{self as out, Outside};
use si_mem::{clib, Mem, Ptr32};
use si_x87::x;

/// The size of a curve node.
const CURVE_NODE: u32 = 0x6c;

/// [soft 0x081dfc9c bufferAddFcvNode]
///
/// Makes a curve node for the function curve of element `fcv` (its data, +4) and stores it
/// through `pp`: nothing without the element, the action list, a positive count of actions
/// or the place. The node's name is a copy of the string at +0x14 of the record at the
/// element's +0; its curve record (`SI_fcurveInitialize`) takes, for each selected action
/// (flag bit 0), the keys within the action's window (`SI_fcvCopyKeysBetweenTimes`, the
/// window's start + `t` as the new start, a last float of 1.0), then the curve's first
/// word, the times `t` + the first action's start and the last window's end + `t`, and
/// three of the curve's 16-bit words (+0x18, +0x1a, +0x1e).
///
/// With no selected action the window's end is read from a local nobody set (the original
/// reads whatever the stack holds there).
pub fn buffer_add_fcv_node(m: &mut Mem, o: &mut dyn Outside, pp: u32, a: Ptr32<Actions>, fcv: u32, t: f32) {
    if fcv == 0 || a.is_null() || m[a].count <= 0 || pp == 0 {
        return;
    }
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    let node = m.si_calloc(1, CURVE_NODE);
    m.set_u32(pp, node);
    let head = m.u32(fcv);
    let nm = m.u32(head.wrapping_add(0x14));
    let d = m.strdup(nm);
    let node = m.u32(pp);
    m.set_u32(node, d);
    let curve = node.wrapping_add(4);
    let owner = m.u32(fcv.wrapping_add(4));
    m.set_u32(l4, owner);
    o.call(m, &out::SI_FCURVE_INITIALIZE, &[curve]);
    let mut i = 0i32;
    while i < m[a].count {
        let e = m[a].elems.add(i);
        if m[e].flags & 1 != 0 {
            let (s, en) = (m[e].start, m[e].end);
            m.set_f32(l8, en);
            let ns = (x(s) + x(t)).to_f32();
            o.call(m, &out::SI_FCV_COPY_KEYS_BETWEEN_TIMES, &[curve, owner, ns.to_bits(), s.to_bits(), en.to_bits(), 1.0f32.to_bits()]);
        }
        i += 1;
    }
    let w0 = m.u32(owner);
    m.set_u32(curve, w0);
    let first = m[a].elems.add(0);
    let lo = (x(t) + x(m[first].start)).to_f32();
    m.set_f32(curve.wrapping_add(0x24), lo);
    let hi = (x(m.f32(l8)) + x(t)).to_f32();
    m.set_f32(curve.wrapping_add(0x28), hi);
    for off in [0x18u32, 0x1a, 0x1e] {
        let v = m.u16(owner.wrapping_add(off));
        m.set_u16(curve.wrapping_add(off), v);
    }
    m.set_sp(sp);
}

/// The dictionary the relation look-ups are handed.
fn dict(m: &Mem) -> u32 {
    g::cur_dict(m)
}

/// [soft 0x081dfdac bufferFillFcvList]
///
/// Appends to the list at `pp` a curve node (`bufferAddFcvNode`) for every animation
/// element related to `elem` in chapter `SI_cid[0]`: first by relations of type 0x3a97
/// with the look-up's mode 0 of the third argument 1, skipping the types 0x3a98 to 0x3aa2
/// and 0x2af8, then by the same type with the fourth argument 1, then 2 (no filter). After
/// each node the place becomes the node's link (+0x68), when a node was made.
pub fn buffer_fill_fcv_list(m: &mut Mem, o: &mut dyn Outside, pp: u32, elem: u32, a: Ptr32<Actions>, t: f32) {
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    let cid0 = m[g::SI_CID][0] as u32;
    let mut pp = pp;
    for (mode, filter) in [(0u32, true), (1, false), (2, false)] {
        let args = [dict(m), elem, 1, mode, 0, 0x3a97, cid0, l8, l4];
        let mut r = o.call(m, &out::SI_ELEMDICT_FIND_RANGE_RELATED_ELEM, &args);
        while r != 0 {
            let code = m.u32(l8);
            if !filter || (code.wrapping_sub(0x3a98) > 10 && code != 0x2af8) {
                buffer_add_fcv_node(m, o, pp, a, r, t);
                let n = m.u32(pp);
                if n != 0 {
                    pp = n.wrapping_add(0x68);
                }
            }
            let args = [dict(m), elem, 1, mode, 0, 0x3a97, cid0, l8, l4];
            r = o.call(m, &out::SI_ELEMDICT_FIND_RANGE_NEXT_RELATED_ELEM, &args);
        }
    }
    m.set_sp(sp);
}

/// [soft 0x081dff94 SI_dopeBufAddNode]
///
/// Makes the dope node of `elem` and stores it through `pp`; then the nodes of its sons
/// are appended after it. Its name is a copy of the string at +0x14 of the record at the
/// element's +0. With `b` (a 16-bit word) not 0 the curves of the element fill the list
/// at +4 (`bufferFillFcvList`). When the chapter `chapter` is `SI_cid[8]` the relations of
/// the element with types 0xc8 (chapter `SI_cid[21]`: `b` not 0, list +8), 0x2bc (chapter
/// `SI_cid[2]`: a chain of 0xc-byte nodes at +0xc, one per related element, each with its
/// name and, with `b`, its curves) and, with `b`, 0xd2 (list +0x14) and 0xd7 (list +0x10,
/// chapter `SI_cid[5]`) add their lists. With `a` (the first 16-bit word) not 0 the sons
/// of the element follow, each by a call of this routine at the end of the chain of
/// siblings (+0x18); in any other chapter the routine ends after the name and the first list.
#[allow(clippy::too_many_arguments)]
pub fn dope_buf_add_node(m: &mut Mem, o: &mut dyn Outside, pp: u32, elem: u32, acts: Ptr32<Actions>, chapter: i32, t: f32, a: i16, b: i16) {
    let sp = m.sp();
    let node = m.si_calloc(1, 0x1c);
    m.set_u32(pp, node);
    let head = m.u32(elem);
    let nm = m.u32(head.wrapping_add(0x14));
    let d = m.strdup(nm);
    let node = m.u32(pp);
    m.set_u32(node, d);
    if b != 0 {
        let p = m.u32(pp).wrapping_add(4);
        buffer_fill_fcv_list(m, o, p, elem, acts, t);
    }
    if chapter == m[g::SI_CID][8] {
        let (l4, l8) = (m.local(4), m.local(4));
        let (c21, c2, c5) = (m[g::SI_CID][21] as u32, m[g::SI_CID][2] as u32, m[g::SI_CID][5] as u32);
        if b != 0 {
            let args = [dict(m), elem, 1, 2, 0xc8, c21, l8, l4];
            let r = o.call(m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &args);
            if r != 0 {
                let p = m.u32(pp).wrapping_add(8);
                buffer_fill_fcv_list(m, o, p, r, acts, t);
            }
        }
        let args = [dict(m), elem, 1, 2, 0x2bc, c2, l8, l4];
        let mut r = o.call(m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &args);
        let mut place = m.u32(pp).wrapping_add(0xc);
        while r != 0 {
            let n = m.si_calloc(1, 0xc);
            m.set_u32(place, n);
            let head = m.u32(r);
            let nm = m.u32(head.wrapping_add(0x14));
            let d = m.strdup(nm);
            let n = m.u32(place);
            m.set_u32(n, d);
            if b != 0 {
                let p = m.u32(place).wrapping_add(4);
                buffer_fill_fcv_list(m, o, p, r, acts, t);
            }
            place = m.u32(place).wrapping_add(8);
            let args = [dict(m), elem, 1, 2, 0x2bc, c2, l8, l4];
            r = o.call(m, &out::SI_ELEMDICT_FIND_NEXT_RELATED_ELEM, &args);
        }
        if b != 0 {
            let args = [dict(m), elem, 1, 2, 0xd2, c5, l8, l4];
            let r = o.call(m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &args);
            if r != 0 {
                let p = m.u32(pp).wrapping_add(0x14);
                buffer_fill_fcv_list(m, o, p, r, acts, t);
            }
            let args = [dict(m), elem, 1, 2, 0xd7, c5, l8, l4];
            let r = o.call(m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &args);
            if r != 0 {
                let p = m.u32(pp).wrapping_add(0x10);
                buffer_fill_fcv_list(m, o, p, r, acts, t);
            }
        }
        if a != 0 {
            let mut s = o.call(m, &out::SI_ED_FIND_SON, &[elem]);
            let mut pp = pp;
            while s != 0 {
                while m.u32(pp) != 0 {
                    pp = m.u32(pp).wrapping_add(0x18);
                }
                dope_buf_add_node(m, o, pp, s, acts, chapter, t, a, b);
                s = o.call(m, &out::SI_ED_FIND_NEXT_SON, &[elem]);
            }
        }
    }
    m.set_sp(sp);
}

/// The address of the buffer's root pointer in the dope sheet's record (+0x28).
const DOPE_BUF: u32 = 0x0878_fac8;

/// [soft 0x081e025c SI_actionsCopySelected]
///
/// Copies the selected actions into the dope sheet's buffer. With a list that has a selected
/// action, an animation file and a root element: every action is turned off
/// (`SI_actionsTmpActivateAll`), the old buffer freed (`SI_freeDopeShBuf`), the file data's
/// word at +8 kept in the dope sheet (+0x2c); the first selected action's start, negated,
/// is the offset; the selected actions are copied (`SI_actionsElemCopy`) into a new array
/// of 0x30-byte entries (`SI_calloc`, kept at +0x34 with its length at +0x30), their start
/// and end moved by the offset; the tree of the root's elements (`SI_dopeBufAddNode`, both
/// flags 1) is built into the buffer; the actions are turned on again.
pub fn actions_copy_selected(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>) {
    if a.is_null() || ac::actions_count_selected(m, a) <= 0 {
        return;
    }
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    let l10 = m.local(4);
    let d = g::cur_dict(m);
    let file = ac::ed_get_anim_file_from_actions(m, d, a.addr());
    let d = g::cur_dict(m);
    let root = ac::ed_get_root_elem_from_anim_file(m, o, d, file, l8, l4);
    if root == 0 || file == 0 {
        m.set_sp(sp);
        return;
    }
    let d = g::cur_dict(m);
    ac::actions_tmp_activate_all(m, o, d, a, 0);
    o.call(m, &out::SI_FREE_DOPE_SH_BUF, &[DOPE_BUF]);
    let w = m.u32(m.u32(file.wrapping_add(4)).wrapping_add(8));
    m[g::SI_DOPE_SH][0x2c / 4] = w;
    let mut sel = 0i32;
    let count = m[a].count;
    if count > 0 {
        let mut i = 0;
        while i < count {
            let e = m[a].elems.add(i);
            if m[e].flags & 1 != 0 {
                sel += 1;
                if sel == 1 {
                    m.set_f32(l10, -m[e].start);
                }
            }
            i += 1;
        }
    }
    if sel > 0 {
        let buf = m.si_calloc(sel as u32, 0x30);
        m[g::SI_DOPE_SH][0x34 / 4] = buf;
        m[g::SI_DOPE_SH][0x30 / 4] = sel as u32;
        let mut copied = 0i32;
        let (mut i, mut dst_off) = (0i32, 0u32);
        if 0 < m[a].count && 0 < m[g::SI_DOPE_SH][0x30 / 4] as i32 {
            loop {
                let e = m[a].elems.add(i);
                if m[e].flags & 1 != 0 {
                    let dst = m[g::SI_DOPE_SH][0x34 / 4].wrapping_add(dst_off);
                    ac::actions_elem_copy(m, o, Ptr32::new(dst), e);
                    let b = m[g::SI_DOPE_SH][0x34 / 4].wrapping_add(dst_off);
                    let v = (x(m.f32(l10)) + x(m.f32(b.wrapping_add(8)))).to_f32();
                    m.set_f32(b.wrapping_add(8), v);
                    let b = m[g::SI_DOPE_SH][0x34 / 4].wrapping_add(dst_off);
                    let v = (x(m.f32(l10)) + x(m.f32(b.wrapping_add(0xc)))).to_f32();
                    m.set_f32(b.wrapping_add(0xc), v);
                    copied += 1;
                    dst_off = dst_off.wrapping_add(0x30);
                }
                i += 1;
                if i >= m[a].count || copied >= m[g::SI_DOPE_SH][0x30 / 4] as i32 {
                    break;
                }
            }
        }
    }
    let chapter = m[g::SI_DOPE_SH][0x2c / 4] as i32;
    let t = m.f32(l10);
    dope_buf_add_node(m, o, DOPE_BUF, root, a, chapter, t, 1, 1);
    let d = g::cur_dict(m);
    ac::actions_tmp_activate_all(m, o, d, a, 1);
    m.set_sp(sp);
}

/// [soft 0x081e0444 SI_actionsCutSelected]
///
/// Copies the selected actions into the dope sheet's buffer and clears them
/// (`SI_actionsClearSelected`); nothing without a selected action.
pub fn actions_cut_selected(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>) {
    if !a.is_null() && ac::actions_count_selected(m, a) > 0 {
        actions_copy_selected(m, o, a);
        ae::actions_clear_selected(m, o, a);
    }
}

/// [soft 0x081e059c SI_emptyFcvWarning]
///
/// Walks the root element's hierarchy with `haFcvProcessCheckEmpty` (the walk's chapter is
/// the animation file data's word at +8) and, when a curve without keys was met (the
/// 16-bit flag the step sets), asks the request dialog (`SI_dialogRequest`, four lines of
/// the binary's own text). Nothing without an animation file, its data or a root element.
pub fn empty_fcv_warning(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Actions>) {
    let sp = m.sp();
    let flag = m.local(2);
    m.set_u16(flag, 0);
    let (l4, l8) = (m.local(4), m.local(4));
    let d = g::cur_dict(m);
    let file = ac::ed_get_anim_file_from_actions(m, d, a.addr());
    let d = g::cur_dict(m);
    let root = ac::ed_get_root_elem_from_anim_file(m, o, d, file, l8, l4);
    if file != 0 && m.u32(file.wrapping_add(4)) != 0 && root != 0 {
        let chapter = m.u32(m.u32(file.wrapping_add(4)).wrapping_add(8)) as i32;
        let d = g::cur_dict(m);
        walk::hieranim_fcv_process(m, o, d, step::CHECK_EMPTY, root, flag, chapter, 0);
        if m.u16(flag) != 0 {
            o.call(m, &out::SI_DIALOG_REQUEST, &[0x0869_4980, 0x0869_4940, 0x0869_4900, 0x0869_48e0]);
        }
    }
    m.set_sp(sp);
}

/// The attribute dictionary `SI_ATD` (0x08781438; the callee reads it).
const ATD: u32 = 0x0878_1438;

fn find_related(m: &mut Mem, o: &mut dyn Outside, next: bool, elem: u32, group: u32, ty: u32, chapter: u32, p1: u32, p2: u32) -> u32 {
    let r = if next { &out::SI_ELEMDICT_FIND_NEXT_RELATED_ELEM } else { &out::SI_ELEMDICT_FIND_RELATED_ELEM };
    let d = g::cur_dict(m);
    o.call(m, r, &[d, elem, 1, group, ty, chapter, p1, p2])
}

/// [soft 0x081e20ac actionsPasteFcvNode]
///
/// Pastes the keys of a list of curve nodes (+0x68 links) of the buffer onto the element's
/// own curves, for each node, for each action `pasted` (the buffer's copy of the selected
/// actions: count and array) that holds keys of the node's curve. The node's curve record
/// is at +4 (its first word the curve type); `SI_attdictGetInfoFromFcurve` gives the
/// relation type, group and two counts for the type. For the types 7, 9, 0x1f, 0xa, 0x20
/// and 0xac the element must have a curve of the relation type (`SI_elemdictFindRelatedElem`,
/// any chapter) or the node is skipped. An action's window is mapped to the paste times
/// start = (s − `base`) × `scale` + `at` + `base`, end likewise; the keys of the node's curve
/// in the window are counted (`SI_fcvCntKeysBetweenTimes`) and nothing is done for none, nor
/// when the frames do not fit the list `acts` (`SI_actionsValidateElem`, cycles of the
/// action, skip −1). The target curve is the data of the first element related to `elem`
/// (group, type, chapter `SI_cid[0]`) whose curve has the node's type word and, with `byname`
/// not 0, whose name is the node's (`strcmp`); if none, and the pasting is allowed (not
/// `byname`, the first count not above 1, the type not one of twelve listed, and for
/// chapter `SI_cid[8]` the model's word at +0x2ec not 4 for the types 0x8b–0x8d nor 2 for
/// 4–6), a new curve is made (`SI_fcurveAllocate`, `SI_edFcvAdd`, `SI_fcurveSaveHistory`
/// of the element it answers). Then `SI_fcvCopyKeysBetweenTimes` copies the keys onto it
/// and, when `SI_glbAnim`'s word at +0x32 is not 0, `SI_fcvSetKeysOnFrames` puts them on
/// frames.
#[allow(clippy::too_many_arguments)]
pub fn actions_paste_fcv_node(m: &mut Mem, o: &mut dyn Outside, node: u32, elem: u32, acts: Ptr32<Actions>, pasted: Ptr32<Actions>, chapter: i32, at: f32, scale: f32, base: f32, byname: i16) {
    let sp = m.sp();
    let (l4, l8, lc, l10) = (m.local(4), m.local(4), m.local(4), m.local(4));
    let mut node = node;
    while node != 0 {
        let curve = node.wrapping_add(4);
        let ty = m.u32(curve);
        o.call(m, &out::SI_ATTDICT_GET_INFO_FROM_FCURVE, &[ATD, ty, l10, lc, l8, l4]);
        let mut skip = false;
        if matches!(ty, 7 | 9 | 0x1f | 0xa | 0x20 | 0xac) {
            let r = find_related(m, o, false, elem, 1, m.u32(l10), 0xffff_ffff, l4, l4);
            if r == 0 {
                skip = true;
            }
        }
        if !skip {
            let mut i = 0i32;
            while i < m[pasted].count {
                let e = m[pasted].elems.add(i);
                let fs = m[e].start;
                let fe = m[e].end;
                let s20 = (x(at) + x(base)).to_f32();
                let prod1 = (x(fs) - x(base)) * x(scale);
                let t20 = (prod1 + x(s20)).to_f32();
                let s = x(at) + x(base);
                let prod2 = (x(fe) - x(base)) * x(scale);
                let t24 = (s + prod2).to_f32();
                let n = o.call(m, &out::SI_FCV_CNT_KEYS_BETWEEN_TIMES, &[curve, fs.to_bits(), fe.to_bits()]) as i32;
                i += 1;
                if n == 0 {
                    continue;
                }
                let end_fr = ac::actions_sec_to_end_frm(m, t24);
                let start_fr = ac::actions_sec_to_start_frm(m, t20);
                let cyc = m[e].cycles;
                if ac::actions_validate_elem(m, acts, start_fr, end_fr, cyc, -1) == 0 {
                    continue;
                }
                let (lcv, l10v) = (m.u32(lc), m.u32(l10));
                let cid0 = m[g::SI_CID][0] as u32;
                let mut r = find_related(m, o, false, elem, lcv, l10v, cid0, l4, l4);
                let mut target;
                loop {
                    target = if r != 0 { m.u32(r.wrapping_add(4)) } else { 0 };
                    if target == 0 {
                        break;
                    }
                    if m.u32(curve) == m.u32(target) {
                        if byname == 0 {
                            break;
                        }
                        let a = m.u32(node);
                        let b = m.u32(m.u32(r).wrapping_add(0x14));
                        if clib::strcmp(m, a, b) == 0 {
                            break;
                        }
                    }
                    r = find_related(m, o, true, elem, lcv, l10v, cid0, l4, l4);
                }
                // whether a new curve may be made
                let mut ok = true;
                let t = l10v;
                if byname != 0 || (m.i32(l8) > 1) || matches!(t, 0xc9 | 0xca | 0x2bd | 0x2be | 0xd3 | 0xd8 | 0x32 | 0x33 | 0x34 | 0x35 | 0xf1 | 0xf2) {
                    ok = false;
                } else if chapter == m[g::SI_CID][8] {
                    let model = m.u32(elem.wrapping_add(4));
                    if t.wrapping_sub(0x8b) <= 2 && m.u32(model.wrapping_add(0x2ec)) == 4 {
                        ok = false;
                    }
                    if t.wrapping_sub(4) <= 2 && m.u32(model.wrapping_add(0x2ec)) == 2 {
                        ok = false;
                    }
                }
                if target == 0 {
                    if !ok {
                        continue;
                    }
                    let owner = m.u32(elem.wrapping_add(4));
                    let tyw = m.u32(curve);
                    target = o.call(m, &out::SI_FCURVE_ALLOCATE, &[tyw, owner]);
                    let d = g::cur_dict(m);
                    let idx = o.call(m, &out::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[d, chapter as u32, owner]);
                    let added = crate::edanim::ed_fcv_add(m, o, target, elem, chapter as u32, idx);
                    let d = g::cur_dict(m);
                    let e2 = o.call(m, &out::SI_ELEMDICT_GET_ELEM, &[d, cid0, added]);
                    if e2 != 0 {
                        o.call(m, &out::SI_FCURVE_SAVE_HISTORY, &[e2]);
                    }
                }
                if target == 0 {
                    continue;
                }
                o.call(m, &out::SI_FCV_COPY_KEYS_BETWEEN_TIMES, &[target, curve, t20.to_bits(), fs.to_bits(), fe.to_bits(), scale.to_bits()]);
                if m[g::SI_GLB_ANIM][0x32 / 2] != 0 {
                    o.call(m, &out::SI_FCV_SET_KEYS_ON_FRAMES, &[target, t20.to_bits(), t24.to_bits()]);
                }
            }
        }
        node = m.u32(node.wrapping_add(0x68));
    }
    m.set_sp(sp);
}

/// [soft 0x081e2470 actionsPasteBufNode]
///
/// Pastes the buffer's node for `elem` and, below it, for the elements' sons. In the buffer
/// `buf` (the dope sheet's record at +0x28: +0 the nodes, +4 the chapter, +8 and +0xc the
/// copied actions) the node of the element's name (`strcmp` against the nodes' +0, linked
/// through +0x18) has its curve list at +4 pasted onto the element (`actionsPasteFcvNode`,
/// with the buffer's chapter; then the buffer's chapter becomes `SI_cid[8]`). When that is
/// not 0 the node's other lists are pasted onto the related elements: +8 (type 0xc8,
/// chapter `SI_cid[21]`), the chain at +0xc (type 0x2bc, chapter `SI_cid[2]`; each related
/// element to the chain entry of its name), +0x10 (0xd7) and +0x14 (0xd2) (chapter
/// `SI_cid[5]`); all with the by-name flag 1. Then every son of the element
/// (`SI_edFindSon`) is pasted the same way with the flag `s`. Nothing without the element,
/// the actions or the buffer.
#[allow(clippy::too_many_arguments)]
pub fn actions_paste_buf_node(m: &mut Mem, o: &mut dyn Outside, elem: u32, acts: Ptr32<Actions>, buf: u32, f1: f32, f2: f32, f3: f32, s: i16) {
    if elem == 0 || acts.is_null() || buf == 0 {
        return;
    }
    let sp = m.sp();
    let (l4, l8) = (m.local(4), m.local(4));
    let pasted = Ptr32::<Actions>::new(buf.wrapping_add(8));
    let mut node = m.u32(buf);
    while node != 0 {
        let name = m.u32(m.u32(elem).wrapping_add(0x14));
        let nn = m.u32(node);
        if clib::strcmp(m, nn, name) == 0 {
            break;
        }
        node = m.u32(node.wrapping_add(0x18));
    }
    if node != 0 {
        let ch = m.u32(buf.wrapping_add(4)) as i32;
        let list = m.u32(node.wrapping_add(4));
        actions_paste_fcv_node(m, o, list, elem, acts, pasted, ch, f1, f2, f3, 0);
        let c8 = m[g::SI_CID][8];
        m.set_u32(buf.wrapping_add(4), c8 as u32);
        if c8 != 0 {
            let (c21, c2, c5) = (m[g::SI_CID][21] as u32, m[g::SI_CID][2] as u32, m[g::SI_CID][5] as u32);
            if m.u32(node.wrapping_add(8)) != 0 {
                let r = find_related(m, o, false, elem, 2, 0xc8, c21, l8, l4);
                if r != 0 {
                    let list = m.u32(node.wrapping_add(8));
                    actions_paste_fcv_node(m, o, list, r, acts, pasted, c21 as i32, f1, f2, f3, 1);
                }
            }
            if m.u32(node.wrapping_add(0xc)) != 0 {
                let mut r = find_related(m, o, false, elem, 2, 0x2bc, c2, l8, l4);
                while r != 0 {
                    let mut b = m.u32(node.wrapping_add(0xc));
                    let rn = m.u32(m.u32(r).wrapping_add(0x14));
                    let mut hit = false;
                    while b != 0 {
                        let bn = m.u32(b);
                        if clib::strcmp(m, bn, rn) == 0 {
                            hit = true;
                            break;
                        }
                        b = m.u32(b.wrapping_add(8));
                    }
                    if hit {
                        let list = m.u32(b.wrapping_add(4));
                        actions_paste_fcv_node(m, o, list, r, acts, pasted, c2 as i32, f1, f2, f3, 1);
                    }
                    r = find_related(m, o, true, elem, 2, 0x2bc, c2, l4, l4);
                }
            }
            if m.u32(node.wrapping_add(0x10)) != 0 {
                let r = find_related(m, o, false, elem, 2, 0xd7, c5, l8, l4);
                if r != 0 {
                    let list = m.u32(node.wrapping_add(0x10));
                    actions_paste_fcv_node(m, o, list, r, acts, pasted, c5 as i32, f1, f2, f3, 1);
                }
            }
            if m.u32(node.wrapping_add(0x14)) != 0 {
                let r = find_related(m, o, false, elem, 2, 0xd2, c5, l8, l4);
                if r != 0 {
                    let list = m.u32(node.wrapping_add(0x14));
                    actions_paste_fcv_node(m, o, list, r, acts, pasted, c5 as i32, f1, f2, f3, 1);
                }
            }
        }
    }
    let mut son = o.call(m, &out::SI_ED_FIND_SON, &[elem]);
    while son != 0 {
        actions_paste_buf_node(m, o, son, acts, buf, f1, f2, f3, s);
        son = o.call(m, &out::SI_ED_FIND_NEXT_SON, &[elem]);
    }
    m.set_sp(sp);
}
