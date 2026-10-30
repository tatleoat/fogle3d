// SPDX-License-Identifier: MIT
//! `trimUtils.c`, the rest of the trim helpers (finding 0044, section "Trim9"): the line-to-two-planes split, the image-data insertion,
//! the trim-curve extraction and the removal of points on the border. Arithmetic follows the original's x87 sequences on [`F80`].

use si_mem::Mem;
use si_x87::F80;

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

fn xf(v: f32) -> F80 {
    F80::from_f32(v)
}

/// A store to a float and the reload from that slot (the compiler's spill of a rounded temporary).
fn r32(v: F80) -> F80 {
    xf(v.to_f32())
}

/// [soft 0x084f7c28 si_divLnTo2Planes]: the line through the point `(p1, p2, p3)` with the direction `(d1, d2, d3)` (the doubles / floats
/// of the original's stack: `d1` a double, `d2` and `d3` floats) as two planes `(a, b, c, d)` written to `out[0..4]` and `out[4..8]` (floats,
/// all cleared first); 1, or 0 when the direction is shorter than 1e-5 in every coordinate. The first plane is the one without the
/// third coordinate (normal `(d2, -d1, 0)`) when `d1` and `d2` allow, else a coordinate plane; the second is perpendicular to the first
/// and the direction.
#[allow(clippy::too_many_arguments)]
pub fn div_ln_to_2_planes(m: &mut Mem, p1: f32, p2: f32, p3: f32, d1: f64, d2: f32, d3: f32, out: u32) -> u32 {
    let (l1, l2, l3) = (x(d1), xf(d2), xf(d3));
    let (a1, a2, a3) = (xf(p1), xf(p2), xf(p3));
    for i in 0..8 {
        m.set_u32(out + 4 * i, 0);
    }
    let one = x(1.0);
    let eps = x(1e-5);
    let sf = |i: u32, v: F80, m: &mut Mem| m.set_u32(out + 4 * i, v.to_f32().to_bits());
    let sc = |i: u32, v: f32, m: &mut Mem| m.set_u32(out + 4 * i, v.to_bits());
    if !eps.gt(l1.abs()) {
        // |d1| >= eps (the compare is eps against |d1|: unordered or equal also take this arm)
        let big2 = l2.abs().gt(eps);
        if big2 {
            let big3 = xf(d3).abs().gt(eps);
            if big3 {
                let n = r32(l2 * l2 + l1 * l1);
                let r = r32(one / n.sqrt());
                sf(0, l2 * r, m);
                sf(1, -l1 * r, m);
                sf(3, (l1 * a2 - l2 * a1) * r, m);
                let t3 = l3 * l2;
                let t1 = r32(l1 * xf(d3));
                let q = r32(one / (n * n + t1 * t1 + t3 * t3).sqrt());
                sf(4, t1 * q, m);
                sf(5, t3 * q, m);
                sf(6, -n * q, m);
                sf(7, ((n * a3 - t1 * a1) - t3 * a2) * q, m);
                return 1;
            }
            sc(6, 1.0, m);
            sc(7, -p3, m);
            let r = r32(one / r32(l1 * l1 + l2 * l2).sqrt());
            sf(0, l2 * r, m);
            sf(1, -l1 * r, m);
            sf(3, (l1 * a2 - l2 * a1) * r, m);
            return 1;
        }
        sc(5, 1.0, m);
        sc(7, -p2, m);
        if !l3.abs().gt(eps) {
            sc(2, 1.0, m);
            sc(3, -p3, m);
        } else {
            let r = r32(one / r32(l3 * l3 + l1 * l1).sqrt());
            sf(0, l3 * r, m);
            sf(2, -l1 * r, m);
            sf(3, (l1 * a3 - l3 * a1) * r, m);
        }
        return 1;
    }
    sc(0, 1.0, m);
    sc(3, -p1, m);
    let mut last = p2;
    if !l2.abs().gt(eps) {
        if !l3.abs().gt(eps) {
            return 0;
        }
        sc(5, 1.0, m);
    } else if l3.abs().gt(eps) {
        let q = r32(one / (l3 * l3 + l2 * l2).sqrt());
        sf(5, l3 * q, m);
        sf(6, -l2 * q, m);
        sf(7, q * (l2 * a3 - l3 * a2), m);
        return 1;
    } else {
        sc(6, 1.0, m);
        last = p3;
    }
    sc(7, -last, m);
    1
}

use crate::composite as cp;
use crate::composite2::{NURBS_ANGLE_DISCONTINUITY, NURBS_ERROR_TOLERANCE, SI_FIT_NURBS_CURVE};
use crate::trimmed::SI_NURBS_SRF_VALUE;
use si_mem::Outside;

/// "Unable to fit high degree curve to extracted curve. Using linear curve."
const MSG_NO_FIT: u32 = 0x086d_a700;

fn dwords(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

/// [soft 0x084f85d8 SI_nurbsSrfExtractTrimCurve]: `(srf, index (short), table)`: the 3-D curve of trim composite `table[index]` (order at
/// +0, point count at +4, closed flag (short) at +0x20, points (0x24 bytes each, the parameter pair first) at +0x2c, the word at +0x34
/// kept): a linear curve of the same number of points (open, or closed when the flag is set; `SI_nurbsCompGetOpenNC` /
/// `GetClosedNC (n, 2, 1)`), each point replaced by the surface point at its (u, v) (`SI_nurbsSrfValue` outside) with weight 1.0. A composite
/// of order below 3 (capped at 4) answers that linear curve; else it is fitted (`SI_fitNurbsCurve(crv, order, tolerance / 10, angle)`
/// outside): on success the linear curve is disposed of and the fit answered with the saved word at +0x34, else the message
/// "Unable to fit high degree curve ..." (level 3) is shown and the linear curve answered. A negative index, a null table or a null
/// composite answers 0.
pub fn srf_extract_trim_curve(m: &mut Mem, o: &mut dyn Outside, srf: u32, index: i16, table: u32) -> u32 {
    if index < 0 || table == 0 {
        return 0;
    }
    let sp = m.sp();
    let p = m.u32(table.wrapping_add((index as i32 as u32).wrapping_mul(4)));
    let mut order = m.i32(p);
    let saved = m.u32(p.wrapping_add(0x34));
    if p == 0 {
        return 0;
    }
    let n = m.i32(p + 4);
    let c = if m.u16(p + 0x20) == 0 { cp::comp_get_open_nc(m, n, 2, 1) } else { cp::comp_get_closed_nc(m, n, 2, 1) };
    let buf = m.local(24);
    let mut i: i32 = 0;
    while i < m.i32(p + 4) {
        let off = (i as u32).wrapping_mul(0x24);
        let pts = m.u32(p + 0x2c);
        let (u, v) = (m.f64(pts.wrapping_add(off)), m.f64(pts.wrapping_add(8).wrapping_add(off)));
        let mut args = vec![buf, srf];
        args.extend_from_slice(&dwords(u));
        args.extend_from_slice(&dwords(v));
        o.call(m, SI_NURBS_SRF_VALUE, &args);
        let d = m.u32(c + 0x2c).wrapping_add(off);
        for k in 0..3u32 {
            let w = m.f64(buf + 8 * k);
            m.set_f64(d + 8 * k, w);
        }
        m.set_u32(d + 0x18, 0);
        m.set_u32(d + 0x1c, 0x3ff0_0000);
        i += 1;
    }
    if order > 4 {
        order = 4;
    }
    if order < 3 {
        m.set_sp(sp);
        return c;
    }
    let slot = m.local(4);
    m.set_u32(slot, c);
    let angle = m.i32(NURBS_ANGLE_DISCONTINUITY) as f64;
    let tol = (xf(m.f32(NURBS_ERROR_TOLERANCE)) / x(10.0)).to_f64();
    let mut args = vec![c, order as u32];
    args.extend_from_slice(&dwords(tol));
    args.extend_from_slice(&dwords(angle));
    let fit = o.call(m, SI_FIT_NURBS_CURVE, &args).eax;
    let r = if fit != 0 {
        o.call(m, cp::CRV_DISPOSE, &[slot]);
        m.set_u32(fit + 0x34, saved);
        fit
    } else {
        o.call(m, cp::SI_STATUS_DISPLAY_MESSAGE, &[MSG_NO_FIT, 3]);
        m.u32(slot)
    };
    m.set_sp(sp);
    r
}

use crate::curveinter2::SI_CURVE_INTER_BLD_LIN_FROM_COMP;
use crate::trimstat::{get_crv_index, link_sep_crv, pts_on_border, update_step_and_order};
use si_hrc::image::nurbs::{knots_allocate, nurbs_crv_allocate, nurbs_points_allocate};

/// Copies the 0x24-byte point `src` to `dst` (nine words).
fn copy_point(m: &mut Mem, dst: u32, src: u32) {
    for w in 0..9u32 {
        let v = m.u32(src + 4 * w);
        m.set_u32(dst + 4 * w, v);
    }
}

/// [soft 0x084f8c94 SI_nurbsTrimRemvPtsOnBorder]: `(srf, *count)`: cuts the linear forms of the surface's trim composites (count at +0x60,
/// table at +0x64) at the places where they touch the border of the parameter rectangle (ends at +0x10, +0x18, +0x34, +0x3c;
/// `ptsOnBorder`). `*count` is first set to 0; no composites answer 0. Each composite is linearised (`si_nurbsCurveInterBldLinFromComp`
/// outside; its +0x50 set to -1, step / order words from the composite). A composite with no point on the border is taken whole (+0x50 = its
/// index); one with exactly one on the border (or with the scan ending at a single one) makes the answer the table of all the linear
/// curves unchanged (`*count` = the number of composites, the work arrays freed). Otherwise the points of the others are gathered in one
/// array starting at each one's first point off the border after a border point (points tagged with the composite index at +0x20, the
/// end of each composite's run kept), the runs between border points are found, `linkSepCrv` joins runs that meet across a closed
/// direction, and each joined run becomes a new order-2 curve with knots 0 1 2 ..; the answer holds the untouched linearised curves
/// first, then the new ones; the linearised curves still in the first table are disposed of. `*count` is the answer's length.
pub fn trim_remv_pts_on_border(m: &mut Mem, o: &mut dyn Outside, srf: u32, pcount: u32) -> u32 {
    let sp = m.sp();
    let mut n_pairs: i32 = 0; // local_4c: borders left going off the border
    let mut n_rise: i32 = 0; // local_50
    let mut n_whole: i32 = 0; // local_54
    let mut total: i32 = 0; // local_68
    let mut done: i32 = 0; // local_70
    m.set_u32(pcount, 0);
    let ncomp = m.i32(srf + 0x60);
    if ncomp == 0 {
        m.set_sp(sp);
        return 0;
    }
    let (u0, u1, v0, v1) = (m.f64(srf + 0x10), m.f64(srf + 0x18), m.f64(srf + 0x34), m.f64(srf + 0x3c));
    let table = |m: &Mem, i: i32| m.u32(m.u32(srf + 0x64).wrapping_add((i as u32).wrapping_mul(4)));
    let idx = m.si_calloc(400, 4);
    let lins = m.si_calloc(ncomp as u32, 4);
    let ends = m.si_calloc(ncomp as u32, 4);
    let at4 = |base: u32, i: i32| base.wrapping_add((i as u32).wrapping_mul(4));
    let lc = m.local(4);
    let mut i = 0i32;
    while i < ncomp {
        let c = table(m, i);
        let l = o.call(m, SI_CURVE_INTER_BLD_LIN_FROM_COMP, &[m.u32(c + 8), m.u32(c + 4), lc]).eax;
        m.set_u32(at4(lins, i), l);
        m.set_u32(l + 0x50, 0xffff_ffff);
        let c = table(m, i);
        update_step_and_order(m, c, m.u32(at4(lins, i)));
        total = total.wrapping_add(m.i32(m.u32(at4(lins, i)) + 4));
        i += 1;
    }
    let pts = m.si_calloc(total as u32, 0x24);
    total = 0;
    m.set_u32(ends, 0);
    let on_border = |m: &Mem, p: u32| pts_on_border(m.f64(p), m.f64(p + 8), u0, u1, v0, v1) as i16;
    let free_work = |m: &mut Mem, with_lins: bool| {
        if with_lins && lins != 0 {
            m.free(lins);
        }
        if pts != 0 {
            m.free(pts);
        }
        if idx != 0 {
            m.free(idx);
        }
        if ends != 0 {
            m.free(ends);
        }
    };
    i = 0;
    while i < ncomp {
        let lin = m.u32(at4(lins, i));
        let cnt = m.i32(lin + 4);
        let mut first: i32 = 0; // local_10
        let mut found: i32 = 0; // local_74
        let mut second: i32 = 0; // local_5c
        let pt = |m: &Mem, k: i32| m.u32(lin + 0x2c).wrapping_add((k as u32).wrapping_mul(0x24));
        let mut k = 0i32;
        let mut hit = false;
        while k < cnt {
            if on_border(m, pt(m, k)) != 0 {
                first = k;
                found = 1;
                hit = true;
                break;
            }
            k += 1;
        }
        if !hit {
            first = k.max(0);
        } else {
            let mut j = first + 1;
            while j < cnt {
                if on_border(m, pt(m, j)) != 0 {
                    found += 1;
                    second = j;
                }
                if 2 < found {
                    break;
                }
                j += 1;
            }
            if found < 2 {
                let mut q = 0i32;
                while q < ncomp {
                    let l = m.u32(at4(lins, q));
                    m.set_i32(l + 0x50, q);
                    q += 1;
                }
                free_work(m, false);
                m.set_i32(pcount, ncomp);
                m.set_sp(sp);
                return lins;
            }
            if found == 2 {
                first = second;
            }
        }
        // LAB_084f8f84
        let mut skip = false;
        if first == 0 {
            let last = lin_pt_before_end(m, lin, cnt);
            if on_border(m, last) == 0 {
                first += 1;
            }
        } else if found == 0 {
            m.set_i32(lin + 0x50, i);
            n_whole += 1;
            m.set_u32(at4(ends, done), 0xffff_ffff);
            skip = true;
        }
        if !skip {
            if first < cnt {
                let mut s = first;
                let mut d = total;
                while s < cnt {
                    let dst = pts.wrapping_add((d as u32).wrapping_mul(0x24));
                    copy_point(m, dst, pt(m, s));
                    m.set_i32(dst + 0x20, i);
                    d += 1;
                    total += 1;
                    s += 1;
                }
            }
            let mut s = 0i32;
            while s < first {
                let dst = pts.wrapping_add((total as u32).wrapping_mul(0x24));
                copy_point(m, dst, pt(m, s));
                m.set_i32(dst + 0x20, i);
                total += 1;
                s += 1;
            }
            m.set_i32(at4(ends, done), total);
        }
        done += 1;
        i += 1;
    }
    // LAB_084f90fb: the runs between border points
    let mut s = on_border(m, pts);
    let mut ni = 0i32;
    let mut k = 1i32;
    while k < total {
        let s2 = on_border(m, pts.wrapping_add((k as u32).wrapping_mul(0x24)));
        if s != s2 {
            if s == 0 {
                n_rise += 1;
                m.set_i32(at4(idx, ni), k);
            } else {
                m.set_i32(at4(idx, ni), k - 1);
                n_pairs += 1;
            }
            ni += 1;
        }
        s = s2;
        k += 1;
    }
    if n_pairs != n_rise {
        m.set_i32(at4(idx, ni), total.wrapping_sub(1));
    }
    let nlim = n_pairs.wrapping_mul(2);
    let nout = n_pairs.wrapping_add(n_whole);
    let out = m.si_calloc(nout as u32, 4);
    let mut w = 0i32;
    i = 0;
    while i < ncomp {
        let l = m.u32(at4(lins, i));
        if m.i32(l + 0x50) != -1 {
            m.set_u32(at4(out, w), l);
            m.set_u32(at4(lins, i), 0);
            w += 1;
        }
        i += 1;
    }
    if n_whole == nout {
        free_work(m, true);
        m.set_i32(pcount, n_whole);
        m.set_sp(sp);
        return out;
    }
    let mut u = n_whole;
    while u < nout {
        m.set_u32(at4(out, u), 0);
        u += 1;
    }
    let mut wr = n_whole;
    let mut cur = 0i32; // local_58
    let outs = m.local(12);
    let (p_first, p_cnt, p_closed) = (outs, outs + 4, outs + 8);
    while 0 < nlim && cur < nlim {
        let c = nurbs_crv_allocate(m).addr();
        m.set_u32(at4(out, wr), c);
        wr += 1;
        link_sep_crv(m, srf, pts, idx, nlim, cur, p_first, c + 4, c + 0x20);
        let _ = (p_cnt, p_closed);
        m.set_u32(c + 0x50, 0xffff_ffff);
        m.set_u32(c + 0x34, 1);
        m.set_u32(c, 2);
        let mut dst = nurbs_points_allocate(m, m.i32(c + 4));
        m.set_u32(c + 0x2c, dst);
        let last = m.i32(p_first);
        if cur < last {
            loop {
                let ci = get_crv_index(m, done, ends, m.i32(at4(idx, cur + 1)));
                if ci >= 0 {
                    let comp = table(m, ci);
                    update_step_and_order(m, comp, c);
                }
                let mut a = m.i32(at4(idx, cur));
                let next = cur + 2;
                let mut src = pts.wrapping_add((a as u32).wrapping_mul(0x24));
                while a <= m.i32(at4(idx, cur + 1)) {
                    copy_point(m, dst, src);
                    a += 1;
                    dst = dst.wrapping_add(0x24);
                    src = src.wrapping_add(0x24);
                }
                cur = next;
                if !(cur < last) {
                    break;
                }
            }
        }
        m.set_u32(c + 0x24, 0);
        let cn = m.i32(c + 4);
        m.set_i32(c + 8, cn.wrapping_add(1));
        let kn = knots_allocate(m, cn.wrapping_add(1));
        m.set_u32(c + 0xc, kn);
        let mut q = 0i32;
        while q < m.i32(c + 8) {
            let kk = m.u32(c + 0xc);
            m.set_f64(kk.wrapping_add((q as u32).wrapping_mul(8)), q as f64);
            q += 1;
        }
        m.set_u32(c + 0x10, 0);
        m.set_u32(c + 0x14, 0);
        let hi = m.i32(c + 8).wrapping_sub(1);
        m.set_f64(c + 0x18, hi as f64);
        cur = last.wrapping_add(1);
    }
    let mut q = 0i32;
    while q < ncomp {
        o.call(m, cp::CRV_DISPOSE, &[at4(lins, q)]);
        q += 1;
    }
    free_work(m, true);
    m.set_i32(pcount, wr);
    m.set_sp(sp);
    out
}

/// The point before the end of a linear curve's point array (`pts + count * 0x24 - 0x24`).
fn lin_pt_before_end(m: &Mem, lin: u32, cnt: i32) -> u32 {
    m.u32(lin + 0x2c).wrapping_add((cnt as u32).wrapping_mul(0x24)).wrapping_sub(0x24)
}

use crate::trimutils::si_trm_data_ins;

/// The squared distance of the point record `p` (x, y, z doubles) from `r`, summed in the order z, y, x on the register and stored as a double.
fn dist2(m: &Mem, p: u32, r: [f64; 3]) -> f64 {
    let d = |k: u32| x(m.f64(p + 8 * k)) - x(r[k as usize]);
    (d(2) * d(2) + d(1) * d(1) + d(0) * d(0)).to_f64()
}

/// [soft 0x084f7f1c si_addImageData]: `(st, srf, ref, n, uv, pts, flag (short))`: for the `n` parameter points `uv` (u, v doubles, 16 bytes each)
/// with their surface points written to `pts` (`SI_nurbsSrfValue` outside; 0x18 bytes each), the squared distances to the reference point
/// `st[+0x118][ref]` are sorted ascending (a selection sort that keeps the order index). When `n` is the status's line count (+0x24) the
/// first `min(n, count)` lines (`st[+0x114]`) each get the next nearest point (`si_trmDataIns`, the node's flag bit 0 set when `flag`, the
/// line's counter at `st[+0x28 + 4k]` raised). Otherwise the sorted points are walked in groups whose neighbouring distances differ by at
/// most 1.1, each measured again from the line's last point (its surface point, or the sorted point when the line is empty); a group whose
/// spread is under 3.0 gives its nearest to the line, a wider one its nearest to the line and its farthest to the next line.
pub fn add_image_data(m: &mut Mem, o: &mut dyn Outside, st: u32, srf: u32, rf: i32, n: i32, uv: u32, pts: u32, flag: i16) {
    let sp = m.sp();
    let rec = m.u32(st + 0x118).wrapping_add((rf as u32).wrapping_mul(0x18));
    let mut r = [m.f64(rec), m.f64(rec + 8), m.f64(rec + 0x10)];
    let dist = m.si_calloc(n as u32, 8);
    let mut lim = n;
    if m.i32(st + 0x24) <= n {
        lim = m.i32(st + 0x24);
    }
    let order = m.si_calloc(n as u32, 4);
    let buf = m.local(24);
    let d8 = |i: i32| dist.wrapping_add((i as u32).wrapping_mul(8));
    let o4 = |i: i32| order.wrapping_add((i as u32).wrapping_mul(4));
    let pt = |i: i32| pts.wrapping_add((i as u32).wrapping_mul(0x18));
    let call_value = |m: &mut Mem, o: &mut dyn Outside, u: f64, v: f64| {
        let mut args = vec![buf, srf];
        args.extend_from_slice(&dwords(u));
        args.extend_from_slice(&dwords(v));
        o.call(m, SI_NURBS_SRF_VALUE, &args);
    };
    let mut k = 0i32;
    while k < n {
        m.set_i32(o4(k), k);
        let (u, v) = (m.f64(uv.wrapping_add((k as u32).wrapping_mul(16))), m.f64(uv.wrapping_add((k as u32).wrapping_mul(16)).wrapping_add(8)));
        call_value(m, o, u, v);
        for w in 0..6u32 {
            let val = m.u32(buf + 4 * w);
            m.set_u32(pt(k) + 4 * w, val);
        }
        let dv = dist2(m, pt(k), r);
        m.set_f64(d8(k), dv);
        k += 1;
    }
    k = 0;
    while k < n - 1 {
        let mut j = k + 1;
        while j < n {
            if m.f64(d8(j)) < m.f64(d8(k)) {
                let t = m.u32(o4(k));
                let s = m.u32(o4(j));
                m.set_u32(o4(k), s);
                m.set_u32(o4(j), t);
                let (a, b) = (m.f64(d8(j)), m.f64(d8(k)));
                m.set_f64(d8(j), b);
                m.set_f64(d8(k), a);
            }
            j += 1;
        }
        k += 1;
    }
    let line_at = |m: &Mem, k: i32| m.u32(m.u32(st + 0x114).wrapping_add((k as u32).wrapping_mul(4)));
    let insert = |m: &mut Mem, k: i32, sorted: i32| {
        let line = line_at(m, k);
        let q = m.u32(o4(sorted)).wrapping_mul(16).wrapping_add(uv);
        let (u, v) = (m.f64(q), m.f64(q + 8));
        si_trm_data_ins(m, line, u, v);
        if flag != 0 {
            let line = line_at(m, k);
            let cur = m.u32(line + 0xc);
            let b = m.u8(cur + 0x11);
            m.set_u8(cur + 0x11, b | 1);
        }
        let at = st + 0x28 + 4 * k as u32;
        let c = m.i32(at);
        m.set_i32(at, c.wrapping_add(1));
    };
    if n == m.i32(st + 0x24) {
        k = 0;
        while k < lim {
            insert(m, k, k);
            k += 1;
        }
    } else if 0 < lim {
        k = 0;
        let mut start = 0i32;
        loop {
            let line = line_at(m, k);
            if m.u32(line + 8) == 0 {
                let src = pt(m.i32(o4(start)));
                for w in 0..6u32 {
                    let val = m.u32(src + 4 * w);
                    m.set_u32(buf + 4 * w, val);
                }
            } else {
                let cur = m.u32(line + 0xc);
                let (u, v) = (m.f64(cur), m.f64(cur + 8));
                call_value(m, o, u, v);
            }
            r = [m.f64(buf), m.f64(buf + 8), m.f64(buf + 0x10)];
            let mut end = start;
            loop {
                if n <= end {
                    break;
                }
                end += 1;
                if end >= n {
                    break;
                }
                if (x(m.f64(d8(end))) - x(m.f64(d8(end - 1)))).abs().gt(x(1.1)) {
                    break;
                }
            }
            let mut g = start;
            while g < end {
                let p = pt(m.i32(o4(g)));
                let dv = dist2(m, p, r);
                m.set_f64(d8(g), dv);
                g += 1;
            }
            let (mut mx, mut mn) = (start, start);
            g = start + 1;
            while g < end {
                let (d3, d4) = (x(m.f64(d8(g))), x(m.f64(d8(mn))));
                if !d3.gt(d4) {
                    mn = g;
                }
                if m.f64(d8(mx)) < m.f64(d8(g)) {
                    mx = g;
                }
                g += 1;
            }
            let spread = x(m.f64(d8(mx))) - x(m.f64(d8(mn)));
            if !spread.gt(x(3.0)) {
                insert(m, k, mn);
            } else {
                insert(m, k, mn);
                k += 1;
                if lim <= k {
                    break;
                }
                insert(m, k, mx);
            }
            if n <= end {
                break;
            }
            k += 1;
            start = end;
            if lim <= k {
                break;
            }
        }
    }
    if dist != 0 {
        m.free(dist);
    }
    if order != 0 {
        m.free(order);
    }
    m.set_sp(sp);
}
