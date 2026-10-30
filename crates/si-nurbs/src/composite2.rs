// SPDX-License-Identifier: MIT
//! The original's `composite.c`, second part (finding 0044, unit trim5): the tree-based remainder and the
//! point-marking and fitting routines that need the generic list or tree routines of `si-gen` (reached through
//! [`Outside`], since this crate does not depend on `si-gen`).

use crate::composite::{on_border_flags, SI_GEN_LIST_INSERT_E};
use si_mem::{Mem, Outside, Routine};
use si_x87::F80;

/// `SI_genListCreate (0x08514ef0)`: `()`, a new list.
pub const SI_GEN_LIST_CREATE: Routine = Routine::new("SI_genListCreate", 0x0851_4ef0);
/// `SI_genListElemOf (0x0851579c)`: `(list, &key, compare)`: the first element whose data the 16-bit compare accepts.
pub const SI_GEN_LIST_ELEM_OF: Routine = Routine::new("SI_genListElemOf", 0x0851_579c);
/// `indexEqual (0x085060b4)`, the callback address handed to `SI_genListElemOf`.
pub const INDEX_EQUAL: u32 = 0x0850_60b4;

/// Tolerance of the point match (the double at 0x08661f00).
const MATCH_TOL: f64 = 1.0e-6;

/// Whether the x and y of two polyline points (0x24-byte records) both differ by less than 1e-6 (strictly), the second
/// only tested when the first holds, in 80 bits.
fn same_xy(m: &Mem, a: u32, b: u32) -> bool {
    let t = F80::from_f64(MATCH_TOL);
    (F80::from_f64(m.f64(a)) - F80::from_f64(m.f64(b))).abs().lt(t)
        && (F80::from_f64(m.f64(a + 8)) - F80::from_f64(m.f64(b + 8))).abs().lt(t)
}

/// [soft 0x085060cc ptsMarkForDelete]: `(curves, poly, box)`: `poly` is the polyline of a trim record (count at +4, points at +0x2c,
/// 0x24 bytes each, the index of the polyline a point belongs to in the word at +0x20); `curves[i]` the polylines themselves (count
/// +4, points +0x2c, a saved position at +0x38). Walks `poly`'s points in order, finds each one (within 1e-6 in x and y) on the
/// polyline it belongs to, going round the ring from the last position, marks the points visited with bit 1 unless they lie
/// on the box (`on_border_flags`), takes the bit off the ends of a run that lie on it, and remembers the position of a polyline
/// in its +0x38 when the walk moves to another one. Answers a list of the indexes of the polylines met (each once, a calloc(1, 4)
/// word per element). A point that cannot be found loops forever (kept; the budget stops it).
pub fn pts_mark_for_delete(m: &mut Mem, o: &mut dyn Outside, curves: u32, poly: u32, bx: u32) -> u32 {
    let first = m.u32(m.u32(poly + 0x2c) + 0x20);
    let mut ci = first;
    let mut cur = m.u32(curves.wrapping_add(ci.wrapping_mul(4)));
    let list = o.call(m, SI_GEN_LIST_CREATE, &[]).eax;
    let mut k: i32 = 0;
    let mut j: i32 = 0;
    let mut prev: i32 = m.i32(cur + 4).wrapping_sub(1);
    let node = m.si_calloc(1, 4);
    m.set_u32(node, ci);
    o.call(m, SI_GEN_LIST_INSERT_E, &[list, node]);
    if k >= m.i32(poly + 4) {
        return list;
    }
    let step = |m: &Mem, cur: u32, j: i32| if j < m.i32(cur + 4).wrapping_sub(1) { j.wrapping_add(1) } else { 0 };
    let at = |m: &Mem, cur: u32, j: i32| m.u32(cur + 0x2c).wrapping_add((j as u32).wrapping_mul(0x24));
    let flag_or = |m: &mut Mem, p: u32, v: u8| {
        let f = m.u8(p + 0x20);
        m.set_u8(p + 0x20, f | v);
    };
    loop {
        // find the point k of the polyline on the ring
        let a = m.u32(poly + 0x2c).wrapping_add((k as u32).wrapping_mul(0x24));
        while !same_xy(m, a, at(m, cur, j)) {
            prev = j;
            j = step(m, cur, j);
        }
        let pj = at(m, cur, j);
        if on_border_flags(m, pj, bx) == 0 {
            flag_or(m, at(m, cur, j), 1);
            let pp = at(m, cur, prev);
            if on_border_flags(m, pp, bx) == 0 {
                flag_or(m, at(m, cur, prev), 1);
            }
        }
        k = k.wrapping_add(1);
        prev = j;
        j = step(m, cur, j);
        if k < m.i32(poly + 4) && m.u32(m.u32(poly + 0x2c).wrapping_add((k as u32).wrapping_mul(0x24)) + 0x20) == ci {
            loop {
                let q = m.u32(poly + 0x2c).wrapping_add((k as u32).wrapping_mul(0x24));
                if !same_xy(m, q, at(m, cur, j)) {
                    break;
                }
                flag_or(m, at(m, cur, j), 1);
                k = k.wrapping_add(1);
                prev = j;
                j = step(m, cur, j);
                if k >= m.i32(poly + 4) || m.u32(m.u32(poly + 0x2c).wrapping_add((k as u32).wrapping_mul(0x24)) + 0x20) != ci {
                    break;
                }
            }
        }
        let pp = at(m, cur, prev);
        if on_border_flags(m, pp, bx) != 0 {
            let p = at(m, cur, prev);
            let f = m.u8(p + 0x20);
            m.set_u8(p + 0x20, f & 0xfe);
        } else {
            let pn = at(m, cur, j);
            if on_border_flags(m, pn, bx) != 0 {
                let p = at(m, cur, j);
                let f = m.u8(p + 0x20);
                m.set_u8(p + 0x20, f & 0xfe);
            }
        }
        if k >= m.i32(poly + 4) {
            return list;
        }
        let nk = m.u32(m.u32(poly + 0x2c).wrapping_add((k as u32).wrapping_mul(0x24)) + 0x20);
        if nk != ci {
            m.set_i32(cur + 0x38, prev);
            ci = nk;
            cur = m.u32(curves.wrapping_add(ci.wrapping_mul(4)));
            let sp = m.sp();
            let key = m.local(4);
            m.set_u32(key, ci);
            let found = o.call(m, SI_GEN_LIST_ELEM_OF, &[list, key, INDEX_EQUAL]).eax;
            m.set_sp(sp);
            if found == 0 {
                let node = m.si_calloc(1, 4);
                m.set_u32(node, ci);
                o.call(m, SI_GEN_LIST_INSERT_E, &[list, node]);
                j = m.i32(cur + 4).wrapping_sub(1);
            } else {
                j = m.i32(cur + 0x38);
            }
        }
        prev = j;
        j = step(m, cur, j);
        if k >= m.i32(poly + 4) {
            return list;
        }
    }
}

/// Six words of a point by value for `si_nurbsCurveInterPointOnLine`: x, y and a zero z.
fn pt_words(x: f64, y: f64) -> [u32; 6] {
    let (a, b) = (x.to_bits(), y.to_bits());
    [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32, 0, 0]
}

/// [soft 0x08507008 elevateFromLinear]: `(comp, &cur, &total, stop)`: builds an open curve of `n = comp[0x38]` points and order `n`
/// (`SI_nurbsCompGetOpenNC(n, n, 1)`; 0 when that fails, after `*total` has been raised by `n - 1`) from the linear polyline
/// of `comp`'s curve (+0x30; count +4, points +0x2c). The first point is the polyline point before `*cur` (taken modulo the
/// count, C remainder), A and B are that point and the one at `*cur`; the walk advances `*cur` while
/// `si_nurbsCurveInterPointOnLine(A, B, next)` (outside) says the next point lies on the line and the index modulo the count is
/// not `stop`. The last point is the polyline point at the final `*cur`; the ones between lie on the straight line between first and
/// last at steps of 1/(n-1) (an 80-bit accumulator, the products and sums in 80 bits, stored as doubles); the knots are 0 for the
/// first `n - 1` and 1 for the rest of the `2n - 2`. A zero polyline count faults in the original.
pub fn elevate_from_linear(m: &mut Mem, o: &mut dyn Outside, comp: u32, cur_p: u32, total: u32, stop: i32) -> u32 {
    let mut cur = m.i32(cur_p);
    let t = m.i32(total).wrapping_sub(1).wrapping_add(m.i32(comp + 0x38));
    m.set_i32(total, t);
    let n = m.i32(comp + 0x38);
    let nc = crate::composite::comp_get_open_nc(m, n, n, 1);
    if nc == 0 {
        return 0;
    }
    let pt = |m: &Mem, i: i32| {
        let crv = m.u32(comp + 0x30);
        m.u32(crv + 0x2c).wrapping_add((i.wrapping_rem(m.i32(crv + 4)) as u32).wrapping_mul(0x24))
    };
    let src = pt(m, cur.wrapping_sub(1));
    let dst = m.u32(nc + 0x2c);
    for w in 0..9u32 {
        let v = m.u32(src + 4 * w);
        m.set_u32(dst + 4 * w, v);
    }
    let pa = pt(m, cur.wrapping_sub(1));
    let a = pt_words(m.f64(pa), m.f64(pa + 8));
    let pb = pt(m, cur);
    let b = pt_words(m.f64(pb), m.f64(pb + 8));
    loop {
        let crv = m.u32(comp + 0x30);
        if cur.wrapping_rem(m.i32(crv + 4)) == stop {
            break;
        }
        let next = cur.wrapping_add(1);
        let pc = pt(m, next);
        let c = pt_words(m.f64(pc), m.f64(pc + 8));
        let mut args = Vec::with_capacity(18);
        args.extend_from_slice(&a);
        args.extend_from_slice(&b);
        args.extend_from_slice(&c);
        let r = o.call(m, crate::composite::SI_NURBS_CURVE_INTER_POINT_ON_LINE, &args).eax;
        if r as u16 == 0 {
            break;
        }
        cur = next;
    }
    m.set_i32(cur_p, cur);
    let n = m.i32(comp + 0x38);
    let last_src = pt(m, cur);
    let last_dst = m.u32(nc + 0x2c).wrapping_add((n as u32).wrapping_mul(0x24)).wrapping_sub(0x24);
    for w in 0..9u32 {
        let v = m.u32(last_src + 4 * w);
        m.set_u32(last_dst + 4 * w, v);
    }
    let step = F80::from_f64(1.0) / F80::from_i32(n.wrapping_sub(1));
    let mut acc = step;
    let mut i = 1i32;
    while i <= m.i32(comp + 0x38).wrapping_sub(2) {
        let nn = m.i32(comp + 0x38);
        let base = m.u32(nc + 0x2c);
        let (p0x, lx) = (F80::from_f64(m.f64(base)), F80::from_f64(m.f64(base.wrapping_add((nn as u32).wrapping_mul(0x24)).wrapping_sub(0x24))));
        let x = p0x + acc * (lx - p0x);
        m.set_f64(base.wrapping_add((i as u32).wrapping_mul(0x24)), x.to_f64());
        let base = m.u32(nc + 0x2c);
        let (p0y, ly) = (F80::from_f64(m.f64(base + 8)), F80::from_f64(m.f64(base.wrapping_add((nn as u32).wrapping_mul(0x24)).wrapping_sub(0x1c))));
        let y = p0y + acc * (ly - p0y);
        acc = acc + step;
        m.set_f64(base.wrapping_add((i as u32).wrapping_mul(0x24)).wrapping_add(8), y.to_f64());
        i += 1;
    }
    let n = m.i32(comp + 0x38);
    let mut k = 0i32;
    while k < n.wrapping_mul(2).wrapping_sub(2) {
        let kn = m.u32(nc + 0xc);
        m.set_u32(kn.wrapping_add((k as u32).wrapping_mul(8)), 0);
        m.set_u32(kn.wrapping_add((k as u32).wrapping_mul(8)).wrapping_add(4), if k < n.wrapping_sub(1) { 0 } else { 0x3ff0_0000 });
        k += 1;
    }
    nc
}

/// `SI_fitNurbsCurve (0x084ce350)` (curveFit.c, another unit): `(crv, order, tolerance, angle)` (two doubles by value); answers a new
/// curve or 0.
pub const SI_FIT_NURBS_CURVE: Routine = Routine::new("SI_fitNurbsCurve", 0x084c_e350);
/// The curve error tolerance (a float; read by `SI_nurbsCrvErrorToleranceGet (0x08149170)`).
pub const NURBS_ERROR_TOLERANCE: u32 = 0x0868_74d8;
/// The angle discontinuity (an int; read by `SI_nurbsCrvAngleDiscontinuityGet (0x08149180)`).
pub const NURBS_ANGLE_DISCONTINUITY: u32 = 0x0868_74dc;

/// [soft 0x085072b8 fitHighDegree]: `(comp, &cur, stop, &total, box)` (the box six doubles by value): takes the points of the linear
/// polyline of `comp`'s curve from the one before `*cur` up to the one at `stop` (modulo the count), advancing `*cur` while the
/// point just passed is not on the box (`si_nurbsCompositeCurvePointOnBorder`) and counting them (one more when `*cur` ends at `stop`);
/// copies them (x, y, the knot, the flag word) into an open order-2 curve (`SI_nurbsCompGetOpenNC(n, 2, 1)`; 0: answers 0),
/// rebuilds its knots for the order `comp[0x38]` ([`crate::composite::rebuild_knot_vector`]) and fits it
/// (`SI_fitNurbsCurve(crv, order, errorTolerance / 10, angleDiscontinuity)` outside). The knot array the rebuild leaves in its scratch
/// is freed, `*total` gains the fit's point count - 1, the scratch curve is disposed (`SI_nurbsCrvDispose`, outside) and the fit answered.
pub fn fit_high_degree(m: &mut Mem, o: &mut dyn Outside, comp: u32, cur_p: u32, stop: i32, total: u32, bx: [f64; 6]) -> u32 {
    let sp = m.sp();
    let mut n: i32 = 1;
    let first = m.i32(cur_p).wrapping_sub(1);
    let mut crv = m.u32(comp + 0x30);
    let mut cur = m.i32(cur_p);
    while cur.wrapping_rem(m.i32(crv + 4)) != stop {
        n = n.wrapping_add(1);
        let k = cur.wrapping_rem(m.i32(crv + 4)) as u32;
        let p = m.u32(crv + 0x2c).wrapping_add(k.wrapping_mul(0x24));
        let (px, py) = (m.f64(p), m.f64(p + 8));
        if crate::composite::composite_curve_point_on_border(px, py, bx) as u16 != 0 {
            break;
        }
        cur = m.i32(cur_p).wrapping_add(1);
        m.set_i32(cur_p, cur);
        crv = m.u32(comp + 0x30);
    }
    let crv = m.u32(comp + 0x30);
    if m.i32(cur_p).wrapping_rem(m.i32(crv + 4)) == stop {
        n = n.wrapping_add(1);
    }
    let nc = crate::composite::comp_get_open_nc(m, n, 2, 1);
    if nc == 0 {
        return 0;
    }
    let slot = m.local(4);
    m.set_u32(slot, nc);
    let scratch = m.local(0x30);
    let mut i = 0i32;
    while i < n {
        let crv = m.u32(comp + 0x30);
        let k = first.wrapping_add(i).wrapping_rem(m.i32(crv + 4)) as u32;
        let src = m.u32(crv + 0x2c).wrapping_add(k.wrapping_mul(0x24));
        let dst = m.u32(nc + 0x2c).wrapping_add((i as u32).wrapping_mul(0x24));
        let (x, y) = (m.f64(src), m.f64(src + 8));
        m.set_f64(dst, x);
        m.set_f64(dst + 8, y);
        let kv = m.f64(m.u32(crv + 0xc).wrapping_add(k.wrapping_mul(8)));
        m.set_f64(m.u32(nc + 0xc).wrapping_add((i as u32).wrapping_mul(8)), kv);
        let fl = m.u32(src + 0x20);
        m.set_u32(dst + 0x20, fl);
        i += 1;
    }
    crate::composite::rebuild_knot_vector(m, o, nc, scratch, m.i32(comp + 0x38));
    let angle = m.i32(NURBS_ANGLE_DISCONTINUITY) as f64;
    let tol = (F80::from_f32(m.f32(NURBS_ERROR_TOLERANCE)) / F80::from_f64(10.0)).to_f64();
    let mut args = vec![nc, m.u32(comp + 0x38), 0, 0, 0, 0];
    args[2] = tol.to_bits() as u32;
    args[3] = (tol.to_bits() >> 32) as u32;
    args[4] = angle.to_bits() as u32;
    args[5] = (angle.to_bits() >> 32) as u32;
    let fit = o.call(m, SI_FIT_NURBS_CURVE, &args).eax;
    let kp = m.u32(scratch + 0xc);
    if kp != 0 {
        m.free(kp);
        m.set_u32(scratch + 0xc, 0);
    }
    if fit != 0 {
        let t = m.i32(total).wrapping_sub(1).wrapping_add(m.i32(fit + 4));
        m.set_i32(total, t);
    }
    o.call(m, crate::composite::CRV_DISPOSE, &[slot]);
    m.set_sp(sp);
    fit
}

/// `SI_genListDestroy (0x08514f08)`: `(&list, destroy)`.
pub const SI_GEN_LIST_DESTROY: Routine = Routine::new("SI_genListDestroy", 0x0851_4f08);
/// `SI_nurbsCrvDispose (0x084db870)`, the element destructor handed to the list destroy.
const CRV_DISPOSE_ADDR: u32 = 0x084d_b870;

/// `SI_genListDestroy (0x08514f08)` as `si-gen` reads it (this crate may not depend on it), so that the element destructor's calls
/// are the port's own calls: the walk from the head hands each element's address to `destroy` once the iteration has moved past it
/// (the last one after the walk), frees it and clears the slot; then the list is freed and `*plist` cleared.
fn list_destroy(m: &mut Mem, o: &mut dyn Outside, plist: u32, destroy: u32) {
    use crate::composite::{SI_GEN_LIST_ITER_INIT, SI_GEN_LIST_ITER_NEXT};
    if plist == 0 || m.u32(plist) == 0 {
        return;
    }
    let frame = m.local(8);
    let (cur, first, start) = (frame, frame + 4, frame + 6);
    let list = m.u32(plist);
    let e = o.call(m, SI_GEN_LIST_ITER_INIT, &[list, first, start]).eax;
    m.set_u32(cur, e);
    let elem_destroy = |m: &mut Mem, o: &mut dyn Outside| {
        let e = m.u32(cur);
        if e == 0 {
            return;
        }
        if destroy != 0 {
            let r = if destroy == CRV_DISPOSE_ADDR { crate::composite::CRV_DISPOSE } else { Routine::at(destroy) };
            o.call(m, r, &[e]);
        }
        let e = m.u32(cur);
        if e != 0 {
            m.free(e);
        }
        m.set_u32(cur, 0);
    };
    loop {
        let list = m.u32(plist);
        let next = o.call(m, SI_GEN_LIST_ITER_NEXT, &[list, first, start]).eax;
        if next == 0 || m.i16(first) == 1 {
            break;
        }
        elem_destroy(m, o);
        m.set_u32(cur, next);
    }
    if m.u32(cur) != 0 {
        elem_destroy(m, o);
    }
    let list = m.u32(plist);
    if list != 0 {
        m.free(list);
    }
    m.set_u32(plist, 0);
    m.release(8);
}

/// [soft 0x08506b44 buildHighDegree]: `(comp, box)` (the box six doubles by value): the high-degree curve for the composite `comp`
/// (its linear polyline curve at +0x30, the target order at +0x38). 0 for a null composite or curve. Without a polyline point on
/// the box (`si_nurbsCompositeCurvePointOnBorder`) the whole curve is fitted ([`crate::composite::rebuild_knot_vector`], then
/// `SI_fitNurbsCurve(crv, order, errorTolerance / 10, angleDiscontinuity)` outside) and the fit's +0x34 word set from the polyline's.
/// Otherwise, from the first point on the box, the polyline is cut into pieces that start at a box point: a piece whose next
/// point is off the box is fitted ([`fit_high_degree`]), one whose next point is on it is raised from the straight line
/// ([`elevate_from_linear`]); the pieces go on a list, the running point total is gathered in a local, and a closed curve
/// (`SI_nurbsCompGetClosedNC(total, order, crv[0x34])`; 0: answers 0, the list left) receives the pieces' points (all but the last of
/// each) and a chained knot vector (each piece's knot differences, the join's difference carried from the previous piece, in 80 bits
/// rounded to doubles). The list is destroyed with `SI_nurbsCrvDispose` on each element.
pub fn build_high_degree(m: &mut Mem, o: &mut dyn Outside, comp: u32, bx: [f64; 6]) -> u32 {
    let sp = m.sp();
    let r = build_hd(m, o, comp, bx);
    m.set_sp(sp);
    r
}

fn build_hd(m: &mut Mem, o: &mut dyn Outside, comp: u32, bx: [f64; 6]) -> u32 {
    use crate::composite::{composite_curve_point_on_border as on_border, SI_GEN_LIST_ITER_INIT, SI_GEN_LIST_ITER_NEXT};
    let total = m.local(4);
    let ip = m.local(4);
    m.set_i32(total, 0);
    let mut start: i32 = -1;
    let mut first_piece = true;
    if comp == 0 {
        return 0;
    }
    let crv = m.u32(comp + 0x30);
    if crv == 0 {
        return 0;
    }
    let pt_xy = |m: &Mem, i: i32| {
        let p = m.u32(crv + 0x2c).wrapping_add((i as u32).wrapping_mul(0x24));
        (m.f64(p), m.f64(p + 8))
    };
    let mut i: i32 = 0;
    while i < m.i32(crv + 4) {
        let (x, y) = pt_xy(m, i);
        if on_border(x, y, bx) as u16 != 0 {
            break;
        }
        i = i.wrapping_add(1);
    }
    m.set_i32(ip, i);
    if i == m.i32(crv + 4) {
        let scratch = m.local(0x30);
        let crv = m.u32(comp + 0x30);
        crate::composite::rebuild_knot_vector(m, o, crv, scratch, m.i32(comp + 0x38));
        let angle = m.i32(NURBS_ANGLE_DISCONTINUITY) as f64;
        let tol = (F80::from_f32(m.f32(NURBS_ERROR_TOLERANCE)) / F80::from_f64(10.0)).to_f64();
        let args = [m.u32(comp + 0x30), m.u32(comp + 0x38), tol.to_bits() as u32, (tol.to_bits() >> 32) as u32, angle.to_bits() as u32, (angle.to_bits() >> 32) as u32];
        let fit = o.call(m, SI_FIT_NURBS_CURVE, &args).eax;
        let kp = m.u32(scratch + 0xc);
        if kp != 0 {
            m.free(kp);
            m.set_u32(scratch + 0xc, 0);
        }
        if fit == 0 {
            return 0;
        }
        let w = m.u32(m.u32(comp + 0x30) + 0x34);
        m.set_u32(fit + 0x34, w);
        return fit;
    }
    let plist = m.local(4);
    let list = o.call(m, SI_GEN_LIST_CREATE, &[]).eax;
    m.set_u32(plist, list);
    if list == 0 {
        return 0;
    }
    if i.wrapping_rem(m.i32(crv + 4)) != -1 {
        loop {
            if first_piece {
                start = i;
                first_piece = false;
            }
            i = i.wrapping_add(1);
            m.set_i32(ip, i);
            let (x, y) = pt_xy(m, i.wrapping_rem(m.i32(crv + 4)));
            let piece = if on_border(x, y, bx) as u16 == 0 {
                fit_high_degree(m, o, comp, ip, start, total, bx)
            } else {
                elevate_from_linear(m, o, comp, ip, total, start)
            };
            if piece != 0 {
                o.call(m, SI_GEN_LIST_INSERT_E, &[list, piece]);
            }
            i = m.i32(ip);
            if i.wrapping_rem(m.i32(crv + 4)) == start {
                break;
            }
        }
    }
    let closed = crate::composite::comp_get_closed_nc(m, m.i32(total), m.i32(comp + 0x38), m.u32(m.u32(comp + 0x30) + 0x34));
    if closed == 0 {
        return 0;
    }
    let flags = m.local(4);
    let (f_first, f_last) = (flags, flags + 2);
    let mut e = o.call(m, SI_GEN_LIST_ITER_INIT, &[list, f_first, f_last]).eax;
    let mut prev = 0.0f64;
    let mut g: i32 = 0;
    if m.i16(f_first) != 1 {
        let mut dst_off: u32 = 0;
        loop {
            let d = elem_data(m, o, e);
            let mut k: i32 = 0;
            while k < m.i32(d + 4).wrapping_sub(1) {
                let src = m.u32(d + 0x2c).wrapping_add((k as u32).wrapping_mul(0x24));
                let dst = m.u32(closed + 0x2c).wrapping_add(dst_off);
                for w in 0..9u32 {
                    let v = m.u32(src + 4 * w);
                    m.set_u32(dst + 4 * w, v);
                }
                dst_off = dst_off.wrapping_add(0x24);
                k += 1;
            }
            let mut k: i32 = 0;
            let cnt = |m: &Mem| m.i32(d + 8).wrapping_sub(m.i32(d)).wrapping_add(1);
            if cnt(m) > 0 {
                loop {
                    let kn = m.u32(closed + 0xc);
                    let at = kn.wrapping_add((g as u32).wrapping_mul(8));
                    if g == 0 {
                        m.set_u32(kn, 0);
                        m.set_u32(kn + 4, 0);
                    } else if k == 0 {
                        let v = F80::from_f64(prev) + F80::from_f64(m.f64(at.wrapping_sub(8)));
                        m.set_f64(at, v.to_f64());
                    } else {
                        let dk = m.u32(d + 0xc);
                        let a = F80::from_f64(m.f64(dk.wrapping_add((k as u32).wrapping_mul(8))));
                        let b = F80::from_f64(m.f64(dk.wrapping_add((k as u32).wrapping_mul(8)).wrapping_sub(8)));
                        let v = (a - b) + F80::from_f64(m.f64(at.wrapping_sub(8)));
                        m.set_f64(at, v.to_f64());
                    }
                    k += 1;
                    g += 1;
                    if !(k < cnt(m)) {
                        break;
                    }
                }
            }
            let dk = m.u32(d + 0xc);
            let a = F80::from_f64(m.f64(dk.wrapping_add((k as u32).wrapping_mul(8))));
            let b = F80::from_f64(m.f64(dk.wrapping_add((k as u32).wrapping_mul(8)).wrapping_sub(8)));
            prev = (a - b).to_f64();
            e = o.call(m, SI_GEN_LIST_ITER_NEXT, &[list, f_first, f_last]).eax;
            if m.i16(f_first) == 1 {
                break;
            }
        }
    }
    let nk = m.i32(closed + 8) as u32;
    let kn = m.u32(closed + 0xc);
    let v = F80::from_f64(prev) + F80::from_f64(m.f64(kn.wrapping_add(nk.wrapping_mul(8)).wrapping_sub(0x10)));
    m.set_f64(kn.wrapping_add(nk.wrapping_mul(8)).wrapping_sub(8), v.to_f64());
    let lo = m.f64(m.u32(closed + 0xc));
    m.set_f64(closed + 0x10, lo);
    let hi = m.f64(m.u32(closed + 0xc).wrapping_add(nk.wrapping_mul(8)).wrapping_sub(8));
    m.set_f64(closed + 0x18, hi);
    list_destroy(m, o, plist, CRV_DISPOSE_ADDR);
    closed
}

fn elem_data(m: &mut Mem, o: &mut dyn Outside, e: u32) -> u32 {
    o.call(m, crate::composite::SI_GEN_LIST_ELEM_GET_DATA, &[e]).eax
}
