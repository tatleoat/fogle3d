// SPDX-License-Identifier: MIT
//! `curveInter.c`, second half (package 15, finding 0044 section "Trim4"): building the intersection objects of a surface's
//! trim curves and intersecting two lists of them. Record layouts: see [`crate::curveinter`]; the object's further words
//! (names ours): +0x38 a 16-bit "inside" word, +0x3a the curve's closed word, +0x3c / +0x40 two numbers handed in
//! (`BuildNewInterList`) or computed (`BuildOldInterList`), +0x44 the number of the composite, +0x48 flag bits (0x8 the object
//! was crossed, 0x10 a marker that spreads over a crossing); a segment record's +0x78 / +0x7c are its previous / next piece
//! after a split (list elements), +0x80 the object it belongs to, its bytes +0x42 / +0x46 get 0x11 where an end was cut.

use crate::composite::comp_get_closed_nc;
use crate::curveinter::*;
use crate::inter::genlist;
use crate::inter::Fl;
use si_hrc::image::nurbs::nurbs_crv_dispose;
use si_mem::{Mem, Outside, Ret, Routine};
use si_x87::F80;

/// `SI_statusDisplayMessage (0x083cca94)` (not ported): `(text, level)`.
pub const SI_STATUS_DISPLAY_MESSAGE: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);
/// `SI_nurbsCrvDispose (0x084db870)` as the generic lists call it back.
pub const SI_NURBS_CRV_DISPOSE: Routine = Routine::new("SI_nurbsCrvDispose", 0x084d_b870);
pub const CURVE_INTER_DESTROY: Routine = Routine::new("si_nurbsCurveInterDestroy", 0x0850_758c);
/// `SI_genListInsertE`-style callbacks of this file's neighbours are in [`crate::curveinter`].
const MSG_NO_AREA: u32 = 0x086d_b460;
const MSG_SELF_INTER: u32 = 0x086d_b4a0;
const MSG_TOO_FEW: u32 = 0x086d_b4e0;

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

fn data(m: &Mem, e: u32) -> u32 {
    genlist::elem_get_data(m, e)
}

/// [soft 0x08508e04 setIntersectionDirection]: `(e1, e2, e3, e4, orient, op)`: links the pieces of the two cut segments to
/// each other through their +0x78 / +0x7c words, by the orientation (1..4) and the operation (1 or 2); answers 1. The writes
/// through a null data pointer that the original makes unconditionally are made here too (its caller never passes one).
pub fn set_intersection_direction(m: &mut Mem, e1: u32, e2: u32, e3: u32, e4: u32, orient: i32, op: i32) -> u32 {
    let (d1, d2, d3, d4) = (data(m, e1), data(m, e2), data(m, e3), data(m, e4));
    let tail = |m: &mut Mem| {
        m.set_u32(d1.wrapping_add(0x7c), e4);
        m.set_u32(d4.wrapping_add(0x78), e1);
    };
    let mid = |m: &mut Mem| {
        m.set_u32(d3.wrapping_add(0x7c), e2);
        m.set_u32(d2.wrapping_add(0x78), e3);
    };
    match orient {
        2 => {
            if op != 1 {
                if op != 2 {
                    return 1;
                }
                if d3 != 0 {
                    m.set_u32(d3 + 0x7c, e2);
                }
                if d2 != 0 {
                    m.set_u32(d2 + 0x78, e3);
                }
                return 1;
            }
            tail(m);
        }
        1 => {
            if op == 1 {
                mid(m);
                return 1;
            }
            if op != 2 {
                return 1;
            }
            if d1 != 0 {
                m.set_u32(d1 + 0x7c, e4);
            }
            if d4 != 0 {
                m.set_u32(d4 + 0x78, e1);
            }
        }
        3 => tail(m),
        4 => mid(m),
        _ => {}
    }
    1
}

/// [soft 0x08509764 crossSegments]: `(seg1, seg2)`: the cross product of the two segments' direction vectors, the second's
/// first (end point minus start point, in 80 bits), divided by its length rounded to a float when that is over 1e-4 (else unscaled).
pub fn cross_segments(m: &Mem, s1: u32, s2: u32) -> [f64; 3] {
    let d = |s: u32, i: u32| x(m.f64(s + 0x18 + 8 * i)) - x(m.f64(s + 8 * i));
    let (a0, a1, a2) = (d(s2, 0), d(s2, 1), d(s2, 2));
    let (b0, b1, b2) = (d(s1, 0), d(s1, 1), d(s1, 2));
    let c0 = a1 * b2 - a2 * b1;
    let c1 = a2 * b0 - a0 * b2;
    let c2 = a0 * b1 - a1 * b0;
    let len = F80::from_f32((c0 * c0 + c1 * c1 + c2 * c2).sqrt().to_f32());
    if x(1e-4).lt(len.abs()) {
        // 0x08661f70
        [(c0 / len).to_f64(), (c1 / len).to_f64(), (c2 / len).to_f64()]
    } else {
        [c0.to_f64(), c1.to_f64(), c2.to_f64()]
    }
}

/// [soft 0x08508ee4 splitSegment]: `(pe, pt[3 doubles], t, list, uv, pe1, pe2)`: cuts the segment of the list element `*pe`
/// at `pt` into two new segments (first `p1 .. pt` with parameters `t * a` and `(1 - t) * a`, second `pt .. p2` with `(1 - t) * a`
/// and `b`; each remembers its object (+0x80) and the original's neighbour link (+0x78 / +0x7c), the cut ends carry 0x110000 in
/// their words), makes two list elements for them (stored at `*pe1`, `*pe2`), links them in after `*pe`, hands the run record
/// `uv` its first / last element where that was `*pe`, removes `*pe` from `list` and sets `*pe` to the second new element.
pub fn split_segment(m: &mut Mem, o: &mut dyn Outside, pe: u32, pt: [u32; 6], t: f64, list: u32, uv: u32, pe1: u32, pe2: u32) {
    let o = &mut crate::curveinter::named(o);
    let seg = data(m, m.u32(pe));
    let tt = x(t);
    let omt = x(1.0) - tt;
    let a = x(m.f64(seg + 0x30));
    let b1 = (omt * a).to_f64();
    let a1 = (tt * a).to_f64();
    let p1 = std::array::from_fn::<u32, 6, _>(|i| m.u32(seg + 4 * i as u32));
    let w40 = m.u32(seg + 0x40);
    let n1 = segment_inter_create(m, p1, pt, a1, b1, w40, 0x0011_0000);
    m.set_u32(n1.wrapping_add(0x80), m.u32(seg + 0x80));
    m.set_u32(n1.wrapping_add(0x78), m.u32(seg + 0x78));
    let w44 = m.u32(seg + 0x44);
    let b2 = m.f64(seg + 0x38);
    let a2 = (omt * a).to_f64();
    let p2 = std::array::from_fn::<u32, 6, _>(|i| m.u32(seg + 0x18 + 4 * i as u32));
    let n2 = segment_inter_create(m, pt, p2, a2, b2, 0x0011_0000, w44);
    m.set_u32(n2.wrapping_add(0x80), m.u32(seg + 0x80));
    m.set_u32(n2.wrapping_add(0x7c), m.u32(seg + 0x7c));
    let e1 = genlist::elem_create(m);
    m.set_u32(pe1, e1);
    let e2 = genlist::elem_create(m);
    m.set_u32(pe2, e2);
    let prev = m.u32(seg + 0x78);
    if prev != 0 {
        let d = data(m, prev);
        m.set_u32(d.wrapping_add(0x7c), m.u32(pe1));
    }
    let next = m.u32(seg + 0x7c);
    if next != 0 {
        let d = data(m, next);
        m.set_u32(d.wrapping_add(0x78), m.u32(pe2));
    }
    genlist::elem_set_data(m, m.u32(pe1), n1);
    genlist::elem_set_data(m, m.u32(pe2), n2);
    genlist::insert_elem_after(m, m.u32(pe), m.u32(pe1));
    genlist::insert_elem_after(m, m.u32(pe1), m.u32(pe2));
    if m.u32(uv + 0x30) == m.u32(pe) {
        m.set_u32(uv + 0x30, m.u32(pe1));
    }
    if m.u32(uv + 0x34) == m.u32(pe) {
        m.set_u32(uv + 0x34, m.u32(pe2));
    }
    genlist::remove(m, o, list, m.u32(pe), SEGMENT_INTER_DESTROY.addr);
    m.set_u32(pe, m.u32(pe2));
}

/// [soft 0x08508918 initCurveInterObj]: `(crv, inside)`: an object for a curve of at least 3 points (else the message "Ignoring
/// projected curve containing less than 3 points" at level 3 and 0): its words +0x38 `inside`, +0x3a the curve's closed word,
/// +0x40 the curve's step word (+0x34), the segment list (`buildSegList`, which also writes the object's box at +4), the run
/// list (`buildUvMonList` with the flag set); 0 where a list cannot be made (the object is then left as it is, not freed).
pub fn init_curve_inter_obj(m: &mut Mem, o: &mut dyn Outside, crv: u32, inside: i16) -> u32 {
    if crv == 0 || m.i32(crv.wrapping_add(4)) < 3 {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[MSG_TOO_FEW, 3]);
        return 0;
    }
    let ci = curve_inter_create(m);
    if ci == 0 {
        return 0;
    }
    m.set_i16(ci + 0x38, inside);
    let closed = m.u16(crv + 0x20);
    m.set_u16(ci + 0x3a, closed);
    let step = m.u32(crv + 0x34);
    m.set_u32(ci + 0x40, step);
    let segs = genlist::create(m);
    m.set_u32(ci, segs);
    if segs == 0 {
        return 0;
    }
    let bx = crate::curveinter3::build_seg_list(m, segs, crv, ci);
    for (i, w) in bx.iter().enumerate() {
        m.set_u32(ci + 4 + 4 * i as u32, *w);
    }
    let uvs = genlist::create(m);
    m.set_u32(ci + 0x34, uvs);
    if uvs == 0 {
        return 0;
    }
    let bx: [u32; 12] = std::array::from_fn(|i| m.u32(ci + 4 + 4 * i as u32));
    build_uv_mon_list(m, uvs, segs, bx, 1);
    ci
}

fn box12(m: &Mem, ci: u32) -> [u32; 12] {
    std::array::from_fn(|i| m.u32(ci.wrapping_add(4 + 4 * i as u32)))
}

fn words_f64(w: &[u32], i: usize) -> f64 {
    f64::from_bits(w[i] as u64 | (w[i + 1] as u64) << 32)
}

/// [soft 0x085079f0 si_nurbsCurveInterBuildNewInterList]: `(curves, a, b, pdst, pdst2, bbox)`: for every curve of the list
/// `curves` an object (words +0x3c `a`, +0x40 `b`, +0x48 |= 0x10, +0x38 = 1 for a closed curve flagged 2 at +0x38, +0x3a the
/// closed word) with its segments and runs; an object whose box is no use (`validBbox`) or that has too few segments (none, or
/// one when closed) is dropped with the message "no area"; one whose runs intersect themselves (`CheckSelfInt`) with the "self
/// intersect" message; else it is added to `*pdst` (or to `*pdst2` when that was asked for and the curve is not closed).
pub fn build_new_inter_list(m: &mut Mem, o: &mut dyn Outside, curves: u32, a: u32, b: u32, pdst: u32, pdst2: u32, bbox: u32) {
    if curves == 0 {
        return;
    }
    let mut l_closed = 0u32;
    let mut dst = m.u32(pdst);
    if dst == 0 {
        dst = genlist::create(m);
        m.set_u32(pdst, dst);
        if dst == 0 {
            return;
        }
    }
    if pdst2 != 0 {
        l_closed = genlist::create(m);
        m.set_u32(pdst2, l_closed);
        if l_closed == 0 {
            return;
        }
    }
    let fl = Fl::new(m);
    let mut e = fl.init(m, curves);
    let cell = m.local(4);
    while fl.first(m) != 1 {
        let src = data(m, e);
        let ci = curve_inter_create(m);
        if ci == 0 {
            break;
        }
        m.set_u32(cell, ci);
        if m.u8(src + 0x38) & 2 != 0 && m.u16(src + 0x20) != 0 {
            m.set_u16(ci + 0x38, 1);
        }
        m.set_u32(ci + 0x3c, a);
        m.set_u32(ci + 0x40, b);
        m.set_u8(ci + 0x48, m.u8(ci + 0x48) | 0x10);
        let cl = m.u16(src + 0x20);
        m.set_u16(ci + 0x3a, cl);
        let segs = genlist::create(m);
        m.set_u32(ci, segs);
        if segs == 0 {
            break;
        }
        let bx = crate::curveinter3::build_seg_list(m, segs, src, ci);
        for (i, w) in bx.iter().enumerate() {
            m.set_u32(ci + 4 + 4 * i as u32, *w);
        }
        let w = box12(m, ci);
        let p1 = [words_f64(&w, 0), words_f64(&w, 2), words_f64(&w, 4)];
        let p2 = [words_f64(&w, 6), words_f64(&w, 8), words_f64(&w, 10)];
        let mut bad = false;
        if valid_bbox(m, p1, p2, src, bbox) == 0 {
            bad = true;
        } else {
            let cnt = m.i32(segs + 0x10);
            if m.u16(src + 0x20) == 0 {
                if cnt <= 0 {
                    bad = true;
                }
            } else if cnt <= 1 {
                bad = true;
            }
        }
        let mut msg = 0;
        if bad {
            msg = MSG_NO_AREA;
        } else {
            let uvs = genlist::create(m);
            m.set_u32(ci + 0x34, uvs);
            if uvs == 0 {
                break;
            }
            let flag = m.i16(src + 0x20);
            build_uv_mon_list(m, uvs, m.u32(ci), box12(m, ci), flag);
            if curve_inter_check_self_int(m, o, ci) as u16 != 0 {
                msg = MSG_SELF_INTER;
            }
        }
        if msg != 0 {
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[msg, 2]);
            curve_inter_destroy(m, o, cell);
        } else {
            let to = if l_closed != 0 && m.u16(ci + 0x3a) == 0 { l_closed } else { dst };
            genlist::insert_e(m, to, ci);
        }
        e = fl.next(m, curves);
    }
    m.release(4);
    fl.done(m);
}

/// [soft 0x085082c8 si_nurbsCurveInterBuildOldInterList] (checked with a recorded-by-body stand-in for `BldLinFromComp`; every byte the original defines agrees, the records' own uninitialised bytes cannot be compared): `(srf, pdst, pdst2)`: the objects of a surface: a new list at `*pdst`
/// (0 left when it cannot be made); with no trim composites or a word 1 at srf+0x5c, one of the surface's border (number at +0x3c
/// the larger of the two u / v orders, +0x40 from the counts of the directions times srf+0x70 / +0x74); with `pdst2` a second list
/// holding one border object (+0x3c = 2); then one object per trim composite (srf+0x64 table of 0x10-byte records, the curves
/// at +8 and their number at +4; +0x3c the highest order, +0x44 the composite's number) until a polyline cannot be built.
pub fn build_old_inter_list(m: &mut Mem, o: &mut dyn Outside, srf: u32, pdst: u32, pdst2: u32) {
    let mut second = 0u32;
    let mut obj = 0u32;
    let list = genlist::create(m);
    m.set_u32(pdst, list);
    if list == 0 {
        return;
    }
    if pdst2 != 0 {
        second = genlist::create(m);
        m.set_u32(pdst2, second);
        if second == 0 {
            return;
        }
    }
    let cell = m.local(4);
    let maxo = m.local(4);
    if m.i32(srf + 0x60) == 0 || m.i16(srf + 0x5c) == 1 {
        let c = build_linear_border(m, srf);
        m.set_u32(cell, c);
        if c != 0 {
            obj = init_curve_inter_obj(m, o, c, 0);
            if obj != 0 {
                let mut v = m.i32(srf + 0x24);
                if v < m.i32(srf) {
                    v = m.i32(srf);
                }
                m.set_i32(obj + 0x3c, v);
                let n1 = if m.u16(srf + 0x20) == 0 {
                    m.i32(srf + 8).wrapping_add((m.i32(srf).wrapping_sub(2)).wrapping_mul(-2))
                } else {
                    m.i32(srf + 8)
                };
                let n2 = if m.u16(srf + 0x44) == 0 {
                    m.i32(srf + 0x2c).wrapping_add((m.i32(srf + 0x24).wrapping_sub(1)).wrapping_mul(-2))
                } else {
                    m.i32(srf + 0x2c)
                };
                if n2.wrapping_sub(1) < n1.wrapping_sub(1) {
                    m.set_i32(obj + 0x40, n1.wrapping_sub(1).wrapping_mul(m.i32(srf + 0x70)));
                } else {
                    m.set_i32(obj + 0x40, n2.wrapping_sub(1).wrapping_mul(m.i32(srf + 0x74)));
                }
            }
            nurbs_crv_dispose(m, cell);
        }
        if obj != 0 {
            genlist::insert_e(m, list, obj);
        }
    }
    if second != 0 {
        let c = build_linear_border(m, srf);
        m.set_u32(cell, c);
        if c != 0 {
            obj = init_curve_inter_obj(m, o, c, 0);
            if obj != 0 {
                m.set_u32(obj + 0x3c, 2);
            }
            nurbs_crv_dispose(m, cell);
        }
        if obj != 0 {
            genlist::insert_e(m, second, obj);
        }
    }
    let mut i: i32 = 0;
    while i < m.i32(srf + 0x60) {
        let comp = m.u32(m.u32(srf + 0x64).wrapping_add((i as u32).wrapping_mul(4)));
        let c = o.call(m, SI_CURVE_INTER_BLD_LIN_FROM_COMP, &[m.u32(comp + 8), m.u32(comp + 4), maxo]).eax;
        m.set_u32(cell, c);
        if c == 0 {
            break;
        }
        let inside = (m.i16(comp) == 0) as i16;
        obj = init_curve_inter_obj(m, o, c, inside);
        nurbs_crv_dispose(m, cell);
        if obj != 0 {
            let mo = m.u32(maxo);
            m.set_u32(obj + 0x3c, mo);
            m.set_i32(obj + 0x44, i);
            genlist::insert_e(m, list, obj);
        }
        i += 1;
    }
    m.release(8);
}

/// `si_nurbsCurveInterBldLinFromComp (0x085085e8)` as the original's callers reach it.
pub const SI_CURVE_INTER_BLD_LIN_FROM_COMP: Routine = Routine::new("si_nurbsCurveInterBldLinFromComp", 0x0850_85e8);
/// `SI_nurbsCrvFlatten (0x084e62f4)`: see [`crate::curveinter::SI_NURBS_CRV_FLATTEN`].
/// [soft 0x0850a0f8 si_nurbsCurveInterCheckFitSelfInter]: `(crv, bbox)`: whether the polyline of the curve, laid in a new closed
/// composite (the points' x and y, z 0, w 1), lies inside `bbox` and makes an object that survives `BuildNewInterList` with no
/// self-intersection: 1 when a point is outside the box (everything freed) or when the list that function leaves is empty,
/// 0 when the lists or the polyline cannot be made.
pub fn check_fit_self_inter(m: &mut Mem, o: &mut dyn Outside, crv: u32, bbox: u32) -> u16 {
    let o = &mut crate::curveinter::named(o);
    let l1c = m.local(4);
    let l2c = m.local(4);
    let flc = m.local(4);
    let ncc = m.local(4);
    let r = (|| {
        let l1 = genlist::create(m);
        m.set_u32(l1c, l1);
        if l1 == 0 {
            return 0;
        }
        let l2 = genlist::create(m);
        m.set_u32(l2c, l2);
        if l2 == 0 {
            genlist::destroy(m, o, l1c, SI_NURBS_CRV_DISPOSE.addr);
            return 0;
        }
        let fl = o.call(m, SI_NURBS_CRV_FLATTEN, &[crv]).eax;
        m.set_u32(flc, fl);
        if fl == 0 {
            genlist::destroy(m, o, l1c, SI_NURBS_CRV_DISPOSE.addr);
            genlist::destroy(m, o, l2c, SI_NURBS_CRV_DISPOSE.addr);
            return 0;
        }
        let nc = comp_get_closed_nc(m, m.i32(fl + 4), 2, 1);
        m.set_u32(ncc, nc);
        let mut bad = false;
        let n = m.i32(fl + 4);
        let mut i: i32 = 0;
        while i < n {
            let off = (i as u32).wrapping_mul(0x24);
            let src = m.u32(fl + 0x2c);
            let (px, py) = (m.f64(src.wrapping_add(off)), m.f64(src.wrapping_add(off).wrapping_add(8)));
            let dst = m.u32(nc + 0x2c);
            m.set_f64(dst.wrapping_add(off), px);
            m.set_f64(dst.wrapping_add(off).wrapping_add(8), py);
            m.set_u32(dst.wrapping_add(off).wrapping_add(0x10), 0);
            m.set_u32(dst.wrapping_add(off).wrapping_add(0x14), 0);
            m.set_u32(dst.wrapping_add(off).wrapping_add(0x18), 0);
            m.set_u32(dst.wrapping_add(off).wrapping_add(0x1c), 0x3ff0_0000);
            let (bx0, by0, bx1, by1) = (m.f64(bbox), m.f64(bbox + 8), m.f64(bbox + 0x18), m.f64(bbox + 0x20));
            if (px > bx1) || (px < bx0) || (py > by1) || (py < by0) {
                bad = true;
                break;
            }
            i += 1;
        }
        let res: u16;
        if bad {
            res = 1;
            nurbs_crv_dispose(m, ncc);
            nurbs_crv_dispose(m, flc);
            genlist::destroy(m, o, l1c, SI_NURBS_CRV_DISPOSE.addr);
        } else {
            genlist::insert_e(m, l1, nc);
            build_new_inter_list(m, o, l1, 2, 1, l2c, 0, bbox);
            res = (m.i32(m.u32(l2c) + 0x10) == 0) as u16;
            nurbs_crv_dispose(m, flc);
            genlist::destroy(m, o, l1c, SI_NURBS_CRV_DISPOSE.addr);
        }
        genlist::destroy(m, o, l2c, CURVE_INTER_DESTROY.addr);
        res
    })();
    m.release(16);
    r
}

/// [soft 0x08509de0 si_nurbsCurveInterIntersectLists] (70 vectors compared of 295, 0 different; the rest the original stops on or reads unwritten memory): `(list1, list2, op)`: for every pair of objects of the two lists whose
/// boxes meet, every pair of their runs (the second object's against the first's) whose boxes meet, every pair of segments of
/// those runs whose boxes meet: `SegInter` (segments of the second object's run first).
pub fn intersect_lists(m: &mut Mem, o: &mut dyn Outside, l1: u32, l2: u32, op: i32) {
    if l1 == 0 || l2 == 0 {
        return;
    }
    let f1 = Fl::new(m);
    let f2 = Fl::new(m);
    let f3 = Fl::new(m);
    let f4 = Fl::new(m);
    let cells = m.local(8);
    let (outer, inner) = (cells, cells + 4);
    f1.init(m, l1);
    f2.init(m, l2);
    let mut e1 = f1.init(m, l1);
    while f1.first(m) != 1 {
        let ci1 = data(m, e1);
        let mut e2 = f2.init(m, l2);
        while f2.first(m) != 1 {
            let ci2 = data(m, e2);
            if curve_inter_bbox_inter(read_box(m, ci1 + 4), read_box(m, ci2 + 4)) != 0 {
                let mut ea = f3.init(m, m.u32(ci2 + 0x34));
                while f3.first(m) != 1 {
                    let uva = data(m, ea);
                    if curve_inter_bbox_inter(read_box(m, ci1 + 4), read_box(m, uva)) != 0 {
                        let mut eb = f4.init(m, m.u32(ci1 + 0x34));
                        while f4.first(m) != 1 {
                            let uvb = data(m, eb);
                            if curve_inter_bbox_inter(read_box(m, uvb), read_box(m, uva)) != 0 {
                                m.set_u32(outer, m.u32(uva + 0x30));
                                loop {
                                    let d1 = data(m, m.u32(outer));
                                    if curve_inter_bbox_inter(read_box(m, uvb), read_box(m, d1 + 0x48)) != 0 {
                                        m.set_u32(inner, m.u32(uvb + 0x30));
                                        loop {
                                            let d2 = data(m, m.u32(inner));
                                            if curve_inter_bbox_inter(read_box(m, d2 + 0x48), read_box(m, d1 + 0x48)) != 0 {
                                                seg_inter(m, o, outer, inner, ci2, ci1, uva, uvb, op);
                                            }
                                            m.set_u32(inner, m.u32(m.u32(inner) + 8));
                                            if m.u32(inner) == m.u32(m.u32(uvb + 0x34) + 8) {
                                                break;
                                            }
                                        }
                                    }
                                    m.set_u32(outer, m.u32(m.u32(outer) + 8));
                                    if m.u32(outer) == m.u32(m.u32(uva + 0x34) + 8) {
                                        break;
                                    }
                                }
                            }
                            eb = f4.next(m, m.u32(ci1 + 0x34));
                        }
                    }
                    ea = f3.next(m, m.u32(ci2 + 0x34));
                }
            }
            e2 = f2.next(m, l2);
        }
        e1 = f1.next(m, l1);
    }
    m.release(8 + 16);
}

/// [soft 0x08508a30 si_nurbsCurveInterSegInter]: `(pe1, pe2, ci1, ci2, uv1, uv2, op)`: intersects the segments of the list
/// elements `*pe1` and `*pe2` (`si_nurbsTrimComputeLineInter`); no crossing, or a parameter outside 0..1 (within 1e-4 of an end
/// counts as that end), answers 0; the orientation of the crossing comes from `computeIntersectionOrientation` (0 answers 0);
/// each segment is cut at the crossing (`splitSegment`) unless the crossing is at an end, where the end is marked (+0x42 or +0x46
/// |= 0x11) and the neighbour elements are picked; `setIntersectionDirection` links them and the objects' flags (+0x48) get
/// 0x8 (and 0x10 passed from one to the other); answers 1.
pub fn seg_inter(m: &mut Mem, o: &mut dyn Outside, pe1: u32, pe2: u32, ci1: u32, ci2: u32, uv1: u32, uv2: u32, op: i32) -> u32 {
    let cells = m.local(16);
    let l = m.local(40);
    let r = seg_inter_body(m, o, pe1, pe2, ci1, ci2, uv1, uv2, op, cells, l);
    m.release(56);
    r
}

fn seg_inter_body(m: &mut Mem, o: &mut dyn Outside, pe1: u32, pe2: u32, ci1: u32, ci2: u32, uv1: u32, uv2: u32, op: i32, cells: u32, l: u32) -> u32 {
    let (cx1, cx2, cy1, cy2) = (cells, cells + 4, cells + 8, cells + 12);
    for k in 0..4 {
        m.set_u32(cells + 4 * k, 0);
    }
    let (mut fa, mut fb, mut fc, mut fd) = (0i16, 0i16, 0i16, 0i16);
    let d1 = data(m, m.u32(pe1));
    let d2 = data(m, m.u32(pe2));
    let mut args = [0u32; 27];
    for i in 0..6u32 {
        args[i as usize] = m.u32(d1 + 4 * i);
        args[6 + i as usize] = m.u32(d1 + 0x18 + 4 * i);
        args[12 + i as usize] = m.u32(d2 + 4 * i);
        args[18 + i as usize] = m.u32(d2 + 0x18 + 4 * i);
    }
    let (pt, tp, sp) = (l, l + 24, l + 32);
    args[24] = pt;
    args[25] = tp;
    args[26] = sp;
    let r = o.call(m, SI_NURBS_TRIM_COMPUTE_LINE_INTER, &args).eax as u16;
    if r == 0 {
        return 0;
    }
    let (zero, one, eps) = (x(0.0), x(1.0), x(1e-4)); // eps 0x08661f60
    let (t, s) = (x(m.f64(tp)), x(m.f64(sp)));
    if t.abs().lt(eps) {
        fa = 1;
        fc = 1;
    } else if (one - t).abs().lt(eps) {
        fa = 1;
    } else if t.lt(zero) || one.lt(t) {
        return 0;
    }
    if s.abs().lt(eps) {
        fb = 1;
        fd = 1;
    } else if (one - s).abs().lt(eps) {
        fb = 1;
    } else if s.lt(zero) || one.lt(s) {
        return 0;
    }
    let ori = compute_intersection_orientation(m, o, m.u32(pe2), m.u32(pe1), ci2, ci1, fb, fa, fd, fc, op);
    if ori == 0 {
        return 0;
    }
    let ptw: [u32; 6] = std::array::from_fn(|i| m.u32(pt + 4 * i as u32));
    let t64 = m.f64(tp);
    let s64 = m.f64(sp);
    let list_of = |m: &Mem, ci: u32| m.u32(ci);
    // the first segment
    if fa == 0 {
        let list = list_of(m, ci1);
        split_segment(m, o, pe1, ptw, t64, list, uv1, cx1, cx2);
    } else if fc != 0 {
        if m.u16(ci1 + 0x3a) != 0 || m.u32(pe1) != m.u32(m.u32(ci1)) {
            let e = m.u32(pe1);
            m.set_u32(cx1, m.u32(e + 4));
        }
        m.set_u32(cx2, m.u32(pe1));
        m.set_u8(d1 + 0x42, m.u8(d1 + 0x42) | 0x11);
    } else {
        if m.u16(ci1 + 0x3a) != 0 || m.u32(pe1) != m.u32(m.u32(ci1) + 4) {
            let e = m.u32(pe1);
            m.set_u32(cx2, m.u32(e + 8));
        }
        m.set_u32(cx1, m.u32(pe1));
        m.set_u8(d1 + 0x46, m.u8(d1 + 0x46) | 0x11);
    }
    // the second segment
    if fb == 0 {
        let list = list_of(m, ci2);
        split_segment(m, o, pe2, ptw, s64, list, uv2, cy1, cy2);
    } else if fd != 0 {
        if m.u16(ci2 + 0x3a) != 0 || m.u32(pe2) != m.u32(m.u32(ci2)) {
            let e = m.u32(pe2);
            m.set_u32(cy1, m.u32(e + 4));
        }
        m.set_u32(cy2, m.u32(pe2));
        m.set_u8(d2 + 0x42, m.u8(d2 + 0x42) | 0x11);
    } else {
        if m.u16(ci2 + 0x3a) != 0 || m.u32(pe2) != m.u32(m.u32(ci2) + 4) {
            let e = m.u32(pe2);
            m.set_u32(cy2, m.u32(e + 8));
        }
        m.set_u32(cy1, m.u32(pe2));
        m.set_u8(d2 + 0x46, m.u8(d2 + 0x46) | 0x11);
    }
    let (x1, x2, y1, y2) = (m.u32(cx1), m.u32(cx2), m.u32(cy1), m.u32(cy2));
    set_intersection_direction(m, x1, x2, y1, y2, ori as i32, op);
    m.set_u8(ci1 + 0x48, m.u8(ci1 + 0x48) | 8);
    let old2 = m.u32(ci2 + 0x48);
    m.set_u32(ci2 + 0x48, old2 | 8);
    if m.u32(ci1 + 0x48) & 0x10 != 0 {
        m.set_u32(ci2 + 0x48, old2 | 0x18);
    } else if (old2 | 8) & 0x10 != 0 {
        m.set_u32(ci1 + 0x48, m.u32(ci1 + 0x48) | 0x10);
    }
    1
}

/// `computeIntersectionOrientation (0x085090b8)`.
pub const COMPUTE_INTERSECTION_ORIENTATION: Routine = Routine::new("computeIntersectionOrientation", 0x0850_90b8);

#[allow(clippy::too_many_arguments)]
fn compute_intersection_orientation(m: &mut Mem, o: &mut dyn Outside, e2: u32, e1: u32, ci2: u32, ci1: u32, b: i16, a: i16, d: i16, c: i16, op: i32) -> u32 {
    o.call(m, COMPUTE_INTERSECTION_ORIENTATION, &[e2, e1, ci2, ci1, b as i32 as u32, a as i32 as u32, d as i32 as u32, c as i32 as u32, op as u32]).eax
}

/// Routes the callbacks of this file; `None` for any other routine.
pub fn route2(m: &mut Mem, r: Routine, args: &[u32]) -> Option<Ret> {
    match r.addr {
        0x084d_b870 => {
            nurbs_crv_dispose(m, args[0]);
            Some(Ret::eax(0))
        }
        0x0850_90b8 => Some(Ret::eax(crate::curveinter3::compute_intersection_orientation(
            m, args[0], args[1], args[2], args[3], args[4] as i16, args[5] as i16, args[6] as i16, args[7] as i16, args[8] as i32,
        ))),
        0x0850_758c => {
            struct Own;
            impl Outside for Own {
                fn call(&mut self, m: &mut Mem, r: Routine, args: &[u32]) -> Ret {
                    route(m, r, args).unwrap_or(Ret::eax(0))
                }
                fn print(&mut self, _: u32, _: &[u8]) {}
            }
            curve_inter_destroy(m, &mut Own, args[0]);
            Some(Ret::eax(0))
        }
        _ => None,
    }
}
