// SPDX-License-Identifier: MIT
//! nurbsgeom9: the scratch that `SI_nurbsSrfExtractCurveSeg` (0x084d603c) leaves in its static arrays, for the heap-faithful
//! `SI_nurbsSrfValue` (finding 0025, section 26).

use crate::eval::knot_distances;
use crate::knots::interv;
use crate::{co, set_coord, NurbsPoint, SrfRef, Statics, EPS};
use si_types::Zero;
use si_x87::F80;

/// What the static arrays `dl`, `dr` (each `order_u - 1` doubles) and `aj` (`order_u * order_v` points, entry `j * order_u + i`
/// for row j of the span in V and column i of the span in U) hold after the extraction at (u, v) with no derivative level: the
/// original keeps them in malloc blocks it never frees, so a later check of the heap sees them. The arithmetic is that of
/// `eval::seg_kernel` (de Boor in U on all rows, factor decided once per column), which fills only the curve's points; this
/// one keeps the working array. `None` where the extraction refuses.
pub fn seg_scratch(st: &mut Statics, s: SrfRef, u: f64, v: f64) -> Option<(Vec<f64>, Vec<f64>, Vec<NurbsPoint>)> {
    let (du, dv) = (s.u(), s.v());
    let lu = interv(st, du, u, (du.rec.closed == 0) as i16)?;
    let lv = interv(st, dv, v, (dv.rec.closed == 0) as i16)?;
    let (nu, ou) = (du.rec.nb_points, du.rec.order);
    let (nv, ov) = (dv.rec.nb_points, dv.rec.order);
    let deg = ou.wrapping_sub(1);
    if (ou as i64) * (ov as i64) > 1 << 20 {
        return None;
    }
    let (dl, dr) = knot_distances(du, lu, u, true)?;
    let first = |left: i32, order: i32, n: i32| {
        let f = left.wrapping_sub(order).wrapping_add(2);
        if f < 0 { f.wrapping_add(n) } else { f }
    };
    let (fu, fv) = (first(lu, ou, nu), first(lv, ov, nv));
    let mut aj = vec![NurbsPoint::zero(); (ou.max(0) as usize) * (ov.max(0) as usize)];
    let mut iu = fu;
    for i in 0..ou.max(0) {
        if fu.wrapping_add(i) == nu {
            iu = 0;
        }
        let mut src = fv.wrapping_mul(nu).wrapping_add(iu);
        for j in 0..ov.max(0) {
            if fv.wrapping_add(j) == nv {
                src = iu;
            }
            aj[(j * ou + i) as usize] = *s.points.get(usize::try_from(src).ok()?)?;
            src = src.wrapping_add(nu);
        }
        iu = iu.wrapping_add(1);
    }
    let (ou_, rows) = (ou.max(0) as usize, ov.max(0) as usize);
    let eps = F80::from_f64(EPS);
    for cc in 0..deg.max(0) {
        let m = deg - cc;
        for b in 0..m.max(0) as usize {
            let li = m as usize - 1 - b;
            let r = F80::from_f64(*dr.get(b)?);
            let a = r / (F80::from_f64(*dl.get(li)?) + r);
            if (F80::ONE - a).lt(eps) {
                continue;
            }
            let copy = a.lt(eps);
            for j in 0..rows {
                let next = *aj.get(j * ou_ + b + 1)?;
                let cur = aj.get_mut(j * ou_ + b)?;
                if copy {
                    *cur = next;
                } else {
                    for q in 0..4 {
                        set_coord(cur, q, (co(&next, q) + a * (co(cur, q) - co(&next, q))).to_f64());
                    }
                }
            }
        }
    }
    Some((dl, dr, aj))
}

use crate::fit::{compute_segment_normal_equations, evaluate_segment, free_eval_temps, x, SI_SHOW_ACTIVITY};
use si_mem::{Mem, Outside};

/// The 1e-6 at 0x08661658 (a span shorter than this is no span).
const SPAN_EPS: f64 = 1e-6;

/// [soft 0x084d2fe0 updateNurbsEvaluationMatrix]: `(dir, mat, seg, params)` brings a segment chain (cells of 0x98 bytes: previous
/// +0, next +4, knot +8, first parameter row +0x10, count +0x14) up to date with a knot vector that gained knots: a sweep with
/// a knot index from `order - 2` (open; 0 closed) walks the chain and the knots together. A knot more than 1e-6 above the end of
/// the current segment (its successor's knot, or the direction's range end +0x18) moves to the next segment. A knot more than 1e-6
/// above the current segment's own knot splits it when it holds rows (count above 0): a new cell is linked in after it
/// (`SI_calloc(1, 0x98)`), the parameter row index `bc` (kept across the sweep) runs up while `params[bc] < knot + 1e-6` (never past
/// the matrix's row count), the new cell takes the knot, first row `bc` and the rows of the old cell above it, and the old cell keeps the rest;
/// the sweep goes on with the cell after the new one, and a segment without rows leaves the sweep with the *previous* new cell's
/// successor (an uninitialised stack word on the first split: the port counts the read and takes the successor of cell 0). Spans
/// of knots shorter than 1e-6 are skipped. Then every cell of the chain is re-evaluated, `evaluateSegment` (a knot index that also
/// skips short spans) and `computeSegmentNormalEquations`; either failing answers 0. Returns 1. The scratch of an order of 5 or more
/// comes from the heap and is freed at the end; below, from the stack. `SI_showActivity (0x083d1a60)` is called once per cell.
pub fn update_nurbs_evaluation_matrix(m: &mut Mem, o: &mut dyn Outside, dir: u32, mat: u32, seg: u32, params: u32) -> i32 {
    let rows = m.i32(mat);
    let closed = m.i16(dir.wrapping_add(0x20)) as i32;
    let order = m.i32(dir);
    let knots = m.u32(dir.wrapping_add(0xc));
    let (out, tab);
    if order > 4 {
        out = m.si_calloc(order as u32, 8);
        if out == 0 {
            return 0;
        }
        tab = m.si_calloc(order as u32, 4);
        if tab == 0 {
            m.free(out);
            return 0;
        }
        let mut b = 0;
        while b < order {
            let r = m.si_calloc(order as u32, 8);
            m.set_u32(tab.wrapping_add(4 * b as u32), r);
            if r == 0 {
                free_eval_temps(m, order, tab, out);
                return 0;
            }
            b += 1;
        }
    } else {
        let o0 = m.local(0x20);
        let mut rws = [0u32; 4];
        for r in rws.iter_mut() {
            *r = m.local(0x20);
        }
        let t = m.local(16);
        for (k, r) in rws.iter().enumerate() {
            m.set_u32(t.wrapping_add(4 * k as u32), *r);
        }
        out = o0;
        tab = t;
    }
    let kd = |m: &Mem, i: i32| x(m.f64(knots.wrapping_add((i as u32).wrapping_mul(8))));
    let eps = x(SPAN_EPS);
    let range_end = |m: &Mem| m.f64(dir.wrapping_add(0x18));
    let end_of = |m: &Mem, s: u32| -> f64 {
        let nx = m.u32(s.wrapping_add(4));
        if nx != 0 { m.f64(nx.wrapping_add(8)) } else { range_end(m) }
    };
    let mut d1 = m.f64(seg.wrapping_add(8));
    let mut d2 = end_of(m, seg);
    let first_k = if closed == 0 { order.wrapping_sub(2) } else { 0 };
    let nk = m.i32(dir.wrapping_add(8));
    let (lim_a, lim_b) = if closed == 0 { (nk.wrapping_add(1).wrapping_sub(order), nk.wrapping_add(2).wrapping_sub(order)) } else { (nk.wrapping_sub(1), nk) };
    let mut bc = 0i32;
    let mut cur = seg;
    let mut i = first_k;
    let mut newcell: Option<u32> = None;
    while i < lim_a && cur != 0 {
        let k = kd(m, i);
        if (x(d2) - eps).lt(k) {
            // the knot is past this segment: on to the next
            cur = m.u32(cur.wrapping_add(4));
            d1 = m.f64(cur.wrapping_add(8));
            d2 = end_of(m, cur);
            continue;
        }
        if (x(d1) + eps).lt(k) {
            if m.i32(cur.wrapping_add(0x14)) > 0 {
                let nc = m.si_calloc(1, 0x98);
                let nx = m.u32(cur.wrapping_add(4));
                if nx != 0 {
                    m.set_u32(nx, nc);
                }
                m.set_u32(nc.wrapping_add(4), nx);
                m.set_u32(nc, cur);
                m.set_u32(cur.wrapping_add(4), nc);
                let limit = k + eps;
                while bc < rows && x(m.f64(params.wrapping_add((bc as u32).wrapping_mul(8)))).lt(limit) {
                    bc += 1;
                }
                m.set_f64(nc.wrapping_add(8), m.f64(knots.wrapping_add((i as u32).wrapping_mul(8))));
                m.set_i32(nc.wrapping_add(0x10), bc);
                let (f, c) = (m.i32(cur.wrapping_add(0x10)), m.i32(cur.wrapping_add(0x14)));
                m.set_i32(nc.wrapping_add(0x14), c.wrapping_sub(bc.wrapping_sub(f)));
                m.set_i32(cur.wrapping_add(0x14), bc.wrapping_sub(f));
                newcell = Some(nc);
            }
            let nc = newcell.unwrap_or_else(|| {
                // the stack word the original reads was never set in this call
                crate::fit2::undefined_read();
                0
            });
            cur = m.u32(nc.wrapping_add(4));
            if cur != 0 {
                d1 = m.f64(cur.wrapping_add(8));
                d2 = end_of(m, cur);
                i = i.wrapping_add(1);
                if i < lim_b && kd(m, i + 1).sub_lt(kd(m, i), eps) {
                    loop {
                        i = i.wrapping_add(1);
                        if i >= lim_b || !kd(m, i + 1).sub_lt(kd(m, i), eps) {
                            break;
                        }
                    }
                }
            }
        } else {
            i = i.wrapping_add(1);
            if i < lim_b.wrapping_sub(1) && kd(m, i + 1).sub_lt(kd(m, i), eps) {
                loop {
                    i = i.wrapping_add(1);
                    if i >= lim_b.wrapping_sub(1) || !kd(m, i + 1).sub_lt(kd(m, i), eps) {
                        break;
                    }
                }
            }
        }
    }
    // every cell of the chain, re-evaluated
    let mut i = first_k;
    let mut s = seg;
    let fail = |m: &mut Mem| {
        if order > 4 {
            free_eval_temps(m, order, tab, out);
        }
        0
    };
    while s != 0 {
        o.call(m, SI_SHOW_ACTIVITY, &[]);
        if evaluate_segment(m, o, dir, s, i, mat, params, tab, out) as u16 == 0 {
            return fail(m);
        }
        if compute_segment_normal_equations(m, s, order, closed, mat) as u16 == 0 {
            return fail(m);
        }
        let next = m.u32(s.wrapping_add(4));
        i = i.wrapping_add(1);
        if i < lim_a && kd(m, i + 1).sub_lt(kd(m, i), eps) {
            loop {
                i = i.wrapping_add(1);
                if i >= lim_a || !kd(m, i + 1).sub_lt(kd(m, i), eps) {
                    break;
                }
            }
        }
        s = next;
    }
    if order > 4 {
        free_eval_temps(m, order, tab, out);
    }
    1
}

trait SubLt {
    fn sub_lt(self, b: si_x87::F80, eps: si_x87::F80) -> bool;
}

impl SubLt for si_x87::F80 {
    /// `(self - b) < eps` on the register, ordered.
    fn sub_lt(self, b: si_x87::F80, eps: si_x87::F80) -> bool {
        (self - b).lt(eps)
    }
}
