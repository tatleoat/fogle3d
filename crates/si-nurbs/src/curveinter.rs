// SPDX-License-Identifier: MIT
//! `curveInter.c` (package 16, finding 0044 section "Trim2"): the intersection objects of the trim-curve
//! rebuilding, on the memory image.
//!
//! Records (names are ours, read from what the routines do with them):
//! - a CurveInter object, `SI_calloc(1, 0x4c)`: +0 the list of segment records, +4 / +0xc the minimum x / y
//!   (starts at 3.4028234663852886e38), +0x1c / +0x24 the maximum x / y (starts at the negative), +0x20 a
//!   16-bit "closed" word, +0x2c the points, +0x34 the list of monotone runs, +0x44 a word (-1 at the start);
//! - a segment record, `SI_calloc(1, 0x88)`: +0 and +0x18 the two end points (x, y, z = 0), +0x30 and +0x38 the
//!   parameters of the ends, +0x40 and +0x44 two words, +0x48 / +0x50 the minimum x / y, +0x60 / +0x68 the
//!   maximum, +0x80 a word, +0x84 flag bits (1 and 2: the segment starts a run);
//! - a monotone run (UvMonSeg), `SI_calloc(1, 0x3c)`: +0 / +8 min x / y, +0x18 / +0x20 max x / y of the two
//!   segments' end points, +0x30 / +0x34 the list elements of its first and last segment.
//!
//! A 48-byte box passed by value is six doubles: minimum x, y, z, maximum x, y, z.

use crate::composite::{comp_get_closed_nc, comp_get_open_nc, composite_curve_point_on_border};
use crate::inter::genlist;
use crate::inter::Fl;
use si_hrc::image::nurbs::{knots_allocate, nurbs_crv_allocate, nurbs_crv_dispose, nurbs_points_allocate};
use si_mem::{Mem, Outside, Ret, Routine};
use si_x87::F80;

/// The callbacks the generic lists call by address.
pub const SEGMENT_INTER_DESTROY: Routine = Routine::new("si_nurbsSegmentInterDestroy", 0x0850_7778);
pub const UV_MON_SEG_DESTROY: Routine = Routine::new("si_nurbsUvMonSegDestroy", 0x0850_792c);

/// The generic lists call their callbacks by bare address; this gives this file's two destructors their names on the way out
/// (as in `inter.rs`), so that a check or a run-time can route them.
pub(crate) struct Named<'a>(pub(crate) &'a mut dyn Outside);

pub(crate) fn named<'a>(o: &'a mut dyn Outside) -> Named<'a> {
    Named(o)
}

impl Outside for Named<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, args: &[u32]) -> Ret {
        let r = match r.addr {
            0x0850_7778 => SEGMENT_INTER_DESTROY,
            0x0850_792c => UV_MON_SEG_DESTROY,
            0x0850_758c => crate::curveinter2::CURVE_INTER_DESTROY,
            0x084d_b870 => crate::curveinter2::SI_NURBS_CRV_DISPOSE,
            _ => r,
        };
        self.0.call(m, r, args)
    }
    fn print(&mut self, stream: u32, text: &[u8]) {
        self.0.print(stream, text)
    }
}

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

const FLT_MAX_D: f64 = 3.4028234663852886e38; // 0x08661f20

/// [soft 0x08507550 si_nurbsCurveInterCreate]: a zeroed 0x4c-byte object with the x and y minima at +4 / +0xc set to
/// 3.4028234663852886e38, the maxima at +0x1c / +0x24 to its negative, and -1 at +0x44.
pub fn curve_inter_create(m: &mut Mem) -> u32 {
    let p = m.si_calloc(1, 0x4c);
    if p != 0 {
        m.set_f64(p + 0xc, FLT_MAX_D);
        m.set_f64(p + 4, FLT_MAX_D);
        m.set_f64(p + 0x24, -FLT_MAX_D);
        m.set_f64(p + 0x1c, -FLT_MAX_D);
        m.set_u32(p + 0x44, 0xffff_ffff);
    }
    p
}

/// [soft 0x0850758c si_nurbsCurveInterDestroy]: `(pp)`: destroys the segment list at `*pp + 0` and the run list at
/// `*pp + 0x34`, frees the object and clears `*pp`.
pub fn curve_inter_destroy(m: &mut Mem, o: &mut dyn Outside, pp: u32) {
    let o = &mut Named(o);
    if pp == 0 {
        return;
    }
    let p = m.u32(pp);
    if p == 0 {
        return;
    }
    genlist::destroy(m, o, p, SEGMENT_INTER_DESTROY.addr);
    let q = m.u32(pp);
    genlist::destroy(m, o, q.wrapping_add(0x34), UV_MON_SEG_DESTROY.addr);
    let q = m.u32(pp);
    if q != 0 {
        m.free(q);
        m.set_u32(pp, 0);
    }
    m.set_u32(pp, 0);
}

fn free_cell(m: &mut Mem, pp: u32) {
    if pp != 0 {
        let p = m.u32(pp);
        if p != 0 {
            m.free(p);
            m.set_u32(pp, 0);
        }
    }
}

/// [soft 0x08507778 si_nurbsSegmentInterDestroy]: `(pp)`: frees `*pp` and clears it.
pub fn segment_inter_destroy(m: &mut Mem, pp: u32) {
    free_cell(m, pp);
}

/// [soft 0x0850792c si_nurbsUvMonSegDestroy]: `(pp)`: frees `*pp` and clears it.
pub fn uv_mon_seg_destroy(m: &mut Mem, pp: u32) {
    free_cell(m, pp);
}

/// [soft 0x085076b4 si_nurbsSegmentInterCreate]: `(p1[3 doubles], p2[3 doubles], a, b, w1, w2)`: a segment record with the two
/// end points (z set to 0), `a` / `b` at +0x30 / +0x38, the words at +0x40 / +0x44 and the box of the x and y of the two
/// points (when `p2.x > p1.x` the minimum is `p1.x`, else `p2.x`; likewise for y).
pub fn segment_inter_create(m: &mut Mem, p1: [u32; 6], p2: [u32; 6], a: f64, b: f64, w1: u32, w2: u32) -> u32 {
    let p = m.si_calloc(1, 0x88);
    if p == 0 {
        return 0;
    }
    for i in 0..6u32 {
        m.set_u32(p + 4 * i, p1[i as usize]);
        m.set_u32(p + 0x18 + 4 * i, p2[i as usize]);
    }
    m.set_u32(p + 0x10, 0);
    m.set_u32(p + 0x14, 0);
    m.set_u32(p + 0x28, 0);
    m.set_u32(p + 0x2c, 0);
    m.set_f64(p + 0x30, si_x87::fmov64(a));
    m.set_f64(p + 0x38, si_x87::fmov64(b));
    m.set_u32(p + 0x40, w1);
    m.set_u32(p + 0x44, w2);
    let f = |i: usize| f64::from_bits(p1[i] as u64 | (p1[i + 1] as u64) << 32);
    let g = |i: usize| f64::from_bits(p2[i] as u64 | (p2[i + 1] as u64) << 32);
    for (k, (off_min, off_max)) in [(0usize, (0x48u32, 0x60u32)), (2, (0x50, 0x68))] {
        let (a1, a2) = (f(k), g(k));
        if x(a2).gt(x(a1)) {
            m.set_f64(p + off_min, a1);
            m.set_f64(p + off_max, a2);
        } else {
            m.set_f64(p + off_min, a2);
            m.set_f64(p + off_max, a1);
        }
    }
    p
}

/// [soft 0x0850789c si_nurbsUvMonSegCreate]: `(e1, e2)`: a run record over the segment elements `e1` and `e2` (at +0x30 / +0x34)
/// with the box of the first segment's first point and the second segment's second point (x at +0 / +0x18, y at +8 / +0x20).
pub fn uv_mon_seg_create(m: &mut Mem, e1: u32, e2: u32) -> u32 {
    let p = m.si_calloc(1, 0x3c);
    if p == 0 {
        return 0;
    }
    m.set_u32(p + 0x30, e1);
    m.set_u32(p + 0x34, e2);
    let s = genlist::elem_get_data(m, e1);
    let t = genlist::elem_get_data(m, e2);
    for (so, to, lo, hi) in [(0u32, 0x18u32, 0u32, 0x18u32), (8, 0x20, 8, 0x20)] {
        let (a, b) = (m.f64(s + so), m.f64(t + to));
        if x(b).gt(x(a)) {
            m.set_f64(p + lo, a);
            m.set_f64(p + hi, b);
        } else {
            m.set_f64(p + lo, b);
            m.set_f64(p + hi, a);
        }
    }
    p
}

/// [soft 0x08508250 si_nurbsCurveInterUpdateBbox]: `(result, box[12 words], x, y)`: copies the box to `result`, the x and
/// y minima lowered (strictly) to the point and the maxima raised; answers `result`.
pub fn curve_inter_update_bbox(m: &mut Mem, result: u32, bx: [u32; 12], px: f64, py: f64) -> u32 {
    let mut w = bx;
    let d = |w: &[u32; 12], i: usize| f64::from_bits(w[i] as u64 | (w[i + 1] as u64) << 32);
    let put = |w: &mut [u32; 12], i: usize, v: f64| {
        let b = v.to_bits();
        w[i] = b as u32;
        w[i + 1] = (b >> 32) as u32;
    };
    if x(px).lt(x(d(&w, 0))) {
        put(&mut w, 0, px);
    }
    if x(px).gt(x(d(&w, 6))) {
        put(&mut w, 6, px);
    }
    if x(py).lt(x(d(&w, 2))) {
        put(&mut w, 2, py);
    }
    if x(py).gt(x(d(&w, 8))) {
        put(&mut w, 8, py);
    }
    for (i, v) in w.iter().enumerate() {
        m.set_u32(result + 4 * i as u32, *v);
    }
    result
}

/// [soft 0x085089d4 si_nurbsCurveInterBboxInter]: `(a, b)` boxes by value: 0 when `b` lies beyond `a` in x or y by more than
/// 1e-4 (b.max + 1e-4 < a.min, b.min - 1e-4 > a.max, in x and in y), else 1.
pub fn curve_inter_bbox_inter(a: [f64; 6], b: [f64; 6]) -> u32 {
    let eps = x(1e-4); // 0x08661f58
    if (x(b[3]) + eps).lt(x(a[0])) {
        return 0;
    }
    if (x(b[0]) - eps).gt(x(a[3])) {
        return 0;
    }
    if (x(b[4]) + eps).lt(x(a[1])) {
        return 0;
    }
    if (x(b[1]) - eps).gt(x(a[4])) {
        return 0;
    }
    1
}

/// [soft 0x08509a44 si_nurbsCurveInterPointOnLine]: `(a, b, c)` points (3 doubles each): whether
/// |(b.y - a.y)(c.x - a.x) - (b.x - a.x)(c.y - a.y)| < 1e-6, the differences and products kept in 80 bits.
pub fn curve_inter_point_on_line(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> u32 {
    let q = (x(b[1]) - x(a[1])) * (x(c[0]) - x(a[0]));
    let p = (x(b[0]) - x(a[0])) * (x(c[1]) - x(a[1]));
    (x(1e-6).gt((q - p).abs())) as u32 // 0x08661f80
}

/// Reads a box of six doubles.
pub fn read_box(m: &Mem, p: u32) -> [f64; 6] {
    let mut b = [0.0; 6];
    for (i, v) in b.iter_mut().enumerate() {
        *v = m.f64(p.wrapping_add(8 * i as u32));
    }
    b
}

/// The callbacks the generic lists call by address (the destructors of this file). `None` for any other routine.
pub fn route(m: &mut Mem, r: Routine, args: &[u32]) -> Option<Ret> {
    match r.addr {
        0x0850_7778 => {
            segment_inter_destroy(m, args[0]);
            Some(Ret::eax(0))
        }
        0x0850_792c => {
            uv_mon_seg_destroy(m, args[0]);
            Some(Ret::eax(0))
        }
        _ => None,
    }
}


/// `SI_nurbsCrvFlatten (0x084e62f4)` (nrb_draw.c, not ported): `(curve)` a polyline curve for the curve.
pub const SI_NURBS_CRV_FLATTEN: Routine = Routine::new("SI_nurbsCrvFlatten", 0x084e_62f4);
/// `si_nurbsTrimComputeLineInter (0x08510aac)` (trim.c, not ported): `(a1, a2, b1, b2` points of 3 doubles by value, `&o1, &o2, &o3)`:
/// a 16-bit answer; the second output is the parameter along the first segment, the third along the second.
pub const SI_NURBS_TRIM_COMPUTE_LINE_INTER: Routine = Routine::new("si_nurbsTrimComputeLineInter", 0x0851_0aac);

fn data(m: &Mem, e: u32) -> u32 {
    genlist::elem_get_data(m, e)
}

/// [soft 0x085084b0 buildUvMonList]: `(out, list, box[12 words], flag)`: walks the segment list (the 16-bit `flag` set: skipping to
/// the first segment with a start bit in +0x84 & 3) and adds to `out` a run record for every stretch from one start-marked segment
/// to the one before the next (with `flag` clear the last element also closes a stretch); with `flag` set and nothing added, one run
/// over the single remaining element is made and its first 12 words overwritten with the box.
pub fn build_uv_mon_list(m: &mut Mem, out: u32, list: u32, bx: [u32; 12], flag: i16) {
    let fl = Fl::new(m);
    let mut cnt: i32 = 0;
    let mut e = fl.init(m, list);
    while fl.first(m) != 1 && flag != 0 {
        let d = data(m, e);
        if m.u8(d.wrapping_add(0x84)) & 3 != 0 {
            break;
        }
        e = fl.next(m, list);
    }
    let mut b = e;
    if cnt < m.i32(list.wrapping_add(0x10)) {
        loop {
            let e2 = fl.next(m, list);
            let d = data(m, e2);
            let make = m.u8(d.wrapping_add(0x84)) & 3 != 0 || (flag == 0 && cnt == m.i32(list.wrapping_add(0x10)).wrapping_sub(1));
            if make {
                let seg = uv_mon_seg_create(m, b, m.u32(e2.wrapping_add(4)));
                if seg == 0 {
                    break;
                }
                genlist::insert_e(m, out, seg);
                b = e2;
            }
            cnt += 1;
            if !(cnt < m.i32(list.wrapping_add(0x10))) {
                break;
            }
        }
    }
    if flag != 0 && m.i32(out.wrapping_add(0x10)) == 0 {
        let seg = uv_mon_seg_create(m, b, m.u32(b.wrapping_add(4)));
        for (i, w) in bx.iter().enumerate() {
            m.set_u32(seg.wrapping_add(4 * i as u32), *w);
        }
        if seg != 0 {
            genlist::insert_e(m, out, seg);
        }
    }
    fl.done(m);
}

/// [soft 0x08508840 buildLinearBorder]: `(srf)`: the closed square polyline of the surface's parameter box: a curve of order 2
/// with 4 points (corners (u0, v0), (u1, v0), (u1, v1), (u0, v1); u range at +0x10 / +0x18, v at +0x34 / +0x3c of the surface),
/// 5 knots 0..4 and the flag words +0x20 = 1, +0x34 = 1; 0 when the curve cannot be allocated.
pub fn build_linear_border(m: &mut Mem, srf: u32) -> u32 {
    let c = nurbs_crv_allocate(m).addr();
    if c == 0 {
        return 0;
    }
    m.set_u32(c + 8, 5);
    let k = knots_allocate(m, 5);
    m.set_u32(c + 0xc, k);
    m.set_u16(c + 0x20, 1);
    let mut i = 0i32;
    while i < m.i32(c + 8) {
        let kk = m.u32(c + 0xc);
        m.set_f64(kk.wrapping_add(8 * i as u32), i as f64);
        i += 1;
    }
    let kk = m.u32(c + 0xc);
    let n = m.i32(c + 8);
    m.set_f64(c + 0x10, m.f64(kk));
    m.set_f64(c + 0x18, m.f64(kk.wrapping_add(8u32.wrapping_mul(n as u32)).wrapping_sub(8)));
    let p = nurbs_points_allocate(m, 4);
    m.set_u32(c + 0x2c, p);
    m.set_u32(c + 4, 4);
    m.set_u32(c, 2);
    m.set_u32(c + 0x34, 1);
    for (off, from) in [(0u32, 0x10u32), (8, 0x34), (0x24, 0x18), (0x2c, 0x34), (0x48, 0x18), (0x50, 0x3c), (0x6c, 0x10), (0x74, 0x3c)] {
        let p = m.u32(c + 0x2c);
        m.set_f64(p.wrapping_add(off), si_x87::fmov64(m.f64(srf.wrapping_add(from))));
    }
    c
}

/// [soft 0x085085e8 si_nurbsCurveInterBldLinFromComp]: `(curves, n, &max_order)`: flattens each of the `n` curves (`SI_nurbsCrvFlatten`
/// with the step word at +0x34 of an order-2 curve set to 1 meanwhile), stores the highest order in `*max_order`, and joins the
/// polylines into one curve (closed when `n > 1` or the last curve is closed) whose points are those of the pieces without the
/// first point of every piece but the first and without the last of the last piece (when `n > 1`), the knots the differences of
/// the pieces' consecutive knots, the point flag words copied, and +0x34 the greatest step word seen. Disposes the pieces.
/// 0 when the table cannot be allocated or the joined curve cannot be made.
pub fn bld_lin_from_comp(m: &mut Mem, o: &mut dyn Outside, curves: u32, n: i32, pmax: u32) -> u32 {
    let mut count: i32 = 0;
    let mut maxstep: i32 = 0;
    m.set_i32(pmax, 0);
    let tmp = m.si_calloc(n as u32, 4);
    if tmp == 0 {
        return 0;
    }
    let at = |base: u32, i: i32| base.wrapping_add((i as u32).wrapping_mul(4));
    let mut i: i32 = 0;
    while i < n {
        let c = m.u32(at(curves, i));
        let ord = m.i32(c);
        if ord > m.i32(pmax) {
            m.set_i32(pmax, ord);
        }
        let step = m.i32(c.wrapping_add(0x34));
        if step > maxstep {
            maxstep = step;
        }
        if m.i32(c) == 2 {
            m.set_i32(c.wrapping_add(0x34), 1);
        }
        let f = o.call(m, SI_NURBS_CRV_FLATTEN, &[c]).eax;
        m.set_u32(at(tmp, i), f);
        m.set_i32(c.wrapping_add(0x34), step);
        if f == 0 {
            break;
        }
        count = count.wrapping_add(m.i32(f.wrapping_add(4)));
        i += 1;
    }
    let closed = if n > 1 {
        count = count.wrapping_sub(n);
        true
    } else {
        let c = m.u32(at(curves, n.wrapping_sub(1)));
        m.u16(c.wrapping_add(0x20)) != 0
    };
    let new = if closed { comp_get_closed_nc(m, count, 2, 1) } else { comp_get_open_nc(m, count, 2, 1) };
    if new != 0 {
        count = 0;
        m.set_i32(new + 0x34, maxstep);
        let mut off: u32 = 0;
        let mut i: i32 = 0;
        while i < n {
            let mut k: i32 = 0;
            loop {
                let src = m.u32(at(tmp, i));
                let lim = m.i32(src.wrapping_add(4));
                if k >= lim {
                    break;
                }
                if n > 1 && i == n - 1 && k == lim.wrapping_sub(1) {
                    break;
                }
                if k > 0 || n == 1 || i == 0 {
                    let so = (k as u32).wrapping_mul(0x24);
                    let (np, sp) = (m.u32(new + 0x2c), m.u32(src.wrapping_add(0x2c)));
                    m.set_f64(np.wrapping_add(off), si_x87::fmov64(m.f64(sp.wrapping_add(so))));
                    let (np, sp) = (m.u32(new + 0x2c), m.u32(src.wrapping_add(0x2c)));
                    m.set_f64(np.wrapping_add(off).wrapping_add(8), si_x87::fmov64(m.f64(sp.wrapping_add(so).wrapping_add(8))));
                    if k < m.i32(src.wrapping_add(8)).wrapping_sub(1) {
                        let kn = m.u32(src.wrapping_add(0xc));
                        let k8 = (k as u32).wrapping_mul(8);
                        let v = x(m.f64(kn.wrapping_add(k8).wrapping_add(8))) - x(m.f64(kn.wrapping_add(k8)));
                        let nk = m.u32(new + 0xc);
                        m.set_f64(nk.wrapping_add((count as u32).wrapping_mul(8)), v.to_f64());
                    }
                    let (np, sp) = (m.u32(new + 0x2c), m.u32(src.wrapping_add(0x2c)));
                    let w = m.u32(sp.wrapping_add(so).wrapping_add(0x20));
                    m.set_u32(np.wrapping_add(off).wrapping_add(0x20), w);
                    off = off.wrapping_add(0x24);
                    count = count.wrapping_add(1);
                }
                k += 1;
            }
            i += 1;
        }
    }
    let mut i: i32 = 0;
    while i < n {
        if m.u32(at(tmp, i)) != 0 {
            nurbs_crv_dispose(m, at(tmp, i));
        }
        i += 1;
    }
    m.free(tmp);
    new
}

/// [soft 0x08509848 validBbox]: `(p1, p2, comp, box)`: whether the segment from `p1` to `p2` is of use: 0 when its |dx| * |dy| is
/// under 1e-6 and an end point is not on the border of `box`, 0 when `comp` is closed (+0x20) and every point of it (count at +4,
/// points at +0x2c, 0x24 apart) lies on the line of the segment; else 1.
pub fn valid_bbox(m: &mut Mem, p1: [f64; 3], p2: [f64; 3], comp: u32, bx: u32) -> i16 {
    let mut result: i16 = 1;
    let area = (x(p2[0]) - x(p1[0])).abs() * (x(p2[1]) - x(p1[1])).abs();
    let bxv = read_box(m, bx);
    if area.lt(x(1e-6)) {
        // 0x08661f78
        let mut bad = false;
        if composite_curve_point_on_border(p1[0], p1[1], bxv) == 0 {
            bad = true;
        } else if composite_curve_point_on_border(p2[0], p2[1], bxv) == 0 {
            bad = true;
        }
        if bad {
            return 0;
        }
    }
    if m.u16(comp.wrapping_add(0x20)) != 0 {
        let count = m.i32(comp.wrapping_add(4));
        let mut i: i32 = 0;
        // the original runs the loop twice (the second is the same); [`curve_inter_point_on_line`] has no effect, so once
        while i < count {
            let pts = m.u32(comp.wrapping_add(0x2c));
            let a = pts.wrapping_add((i as u32).wrapping_mul(0x24));
            let q = [m.f64(a), m.f64(a.wrapping_add(8)), m.f64(a.wrapping_add(0x10))];
            if curve_inter_point_on_line(p1, p2, q) as u16 == 0 {
                break;
            }
            i += 1;
        }
        if i == count {
            result = 0;
        }
    }
    result
}

/// [soft 0x08509c90 si_nurbsCurveInterSelfSegInter]: `(e1, e2)` segment list elements: 0 when the two segments (end points at +0 / +0x18)
/// have no intersection (`si_nurbsTrimComputeLineInter` answering 0, or a parameter t or s outside 0..1), or meet only at the shared
/// end of neighbours (t within 1e-6 of 0 and s of 1 with `e2.next == e1`, or t within 1e-6 of 1 and |s| < 1e-6 with `e1.next == e2`);
/// else 1.
pub fn curve_inter_self_seg_inter(m: &mut Mem, o: &mut dyn Outside, e1: u32, e2: u32) -> u32 {
    let a = data(m, e1);
    let b = data(m, e2);
    let l = m.local(24);
    let mut args = [0u32; 27];
    for i in 0..6u32 {
        args[i as usize] = m.u32(a.wrapping_add(4 * i));
        args[6 + i as usize] = m.u32(a.wrapping_add(0x18 + 4 * i));
        args[12 + i as usize] = m.u32(b.wrapping_add(4 * i));
        args[18 + i as usize] = m.u32(b.wrapping_add(0x18 + 4 * i));
    }
    args[24] = l;
    args[25] = l + 8;
    args[26] = l + 16;
    let r = o.call(m, SI_NURBS_TRIM_COMPUTE_LINE_INTER, &args).eax as u16;
    let (t, s) = (x(m.f64(l + 8)), x(m.f64(l + 16)));
    m.release(24);
    if r == 0 {
        return 0;
    }
    let zero = x(0.0);
    let one = x(1.0);
    let eps = x(1e-6); // 0x08661f88
    if t.lt(zero) || one.lt(t) || s.lt(zero) || one.lt(s) {
        return 0;
    }
    if eps.gt(t.abs()) {
        if (one - s).abs().lt(eps) && m.u32(e2.wrapping_add(8)) == e1 {
            return 0;
        }
    }
    if !(one - t).abs().lt(eps) {
        return 1;
    }
    if !s.abs().lt(eps) {
        return 1;
    }
    if m.u32(e1.wrapping_add(8)) == e2 {
        return 0;
    }
    1
}

/// [soft 0x08509ac0 si_nurbsCurveInterCheckSelfInt]: `(ci)`: 1 when two different runs of the object's run list (+0x34) have
/// segments, with overlapping boxes (`bboxInter`), that intersect (`selfSegInter`); 0 for none or a null object.
pub fn curve_inter_check_self_int(m: &mut Mem, o: &mut dyn Outside, ci: u32) -> u32 {
    if ci == 0 {
        return 0;
    }
    let fl = Fl::new(m);
    let list = m.u32(ci.wrapping_add(0x34));
    let mut e = fl.init(m, list);
    while fl.first(m) != 1 {
        let uv1 = data(m, e);
        let mut cur = m.u32(e.wrapping_add(8));
        if cur != m.u32(m.u32(ci.wrapping_add(0x34))) {
            loop {
                let uv2 = data(m, cur);
                if curve_inter_bbox_inter(read_box(m, uv1), read_box(m, uv2)) != 0 {
                    let mut se = m.u32(uv2.wrapping_add(0x30));
                    loop {
                        let sd = data(m, se);
                        if curve_inter_bbox_inter(read_box(m, uv1), read_box(m, sd.wrapping_add(0x48))) == 0 {
                            se = m.u32(se.wrapping_add(8));
                        } else {
                            let mut s2 = m.u32(uv1.wrapping_add(0x30));
                            loop {
                                let sd2 = data(m, s2);
                                if curve_inter_bbox_inter(read_box(m, sd2.wrapping_add(0x48)), read_box(m, sd.wrapping_add(0x48))) != 0
                                    && curve_inter_self_seg_inter(m, o, se, s2) as u16 != 0
                                {
                                    fl.done(m);
                                    return 1;
                                }
                                s2 = m.u32(s2.wrapping_add(8));
                                if s2 == m.u32(m.u32(uv1.wrapping_add(0x34)).wrapping_add(8)) {
                                    break;
                                }
                            }
                            se = m.u32(se.wrapping_add(8));
                        }
                        if se == m.u32(m.u32(uv2.wrapping_add(0x34)).wrapping_add(8)) {
                            break;
                        }
                    }
                }
                cur = m.u32(cur.wrapping_add(8));
                if cur == m.u32(m.u32(ci.wrapping_add(0x34))) {
                    break;
                }
            }
        }
        e = fl.next(m, m.u32(ci.wrapping_add(0x34)));
    }
    fl.done(m);
    0
}
