// SPDX-License-Identifier: MIT
//! The original's `composite.c` (finding 0044), on the memory image: the composite curve
//! record and the small border helpers of the trim-curve rebuilding.
//!
//! Records (names are ours): a composite curve is 0x44 bytes: the 48-byte parameter box at
//! +0 (copied from the caller), the NURBS curve pointer at +0x30, a 16-bit flag at +0x34,
//! -1 at +0x3c. A trim polyline point is 0x24 bytes: x at +0, y at +8, border flags at +0x20
//! (bit 1 closed marker, bits 2 4 8 0x10 the left, bottom, right, top border of the box).

use si_hrc::image::nurbs::{knots_allocate, nurbs_crv_allocate, nurbs_crv_dispose, nurbs_points_allocate};
use si_mem::{Mem, Outside, Routine};
use si_x87::F80;

/// Tolerance of `onBorderFlags` (the double at 0x08661ef8).
const BORDER_TOL: f64 = 1.0e-6;

/// [soft 0x08503830 si_nurbsCompositeCurveCreate]: `(curve, box[12 words], flag)`: a zeroed
/// 0x44-byte block holding the box, the curve pointer at +0x30, the flag at +0x34 and -1 at
/// +0x3c; 0 when the allocation fails.
pub fn composite_curve_create(m: &mut Mem, curve: u32, boxw: [u32; 12], flag: u32) -> u32 {
    let p = m.si_calloc(1, 0x44);
    if p != 0 {
        for (i, w) in boxw.iter().enumerate() {
            m.set_u32(p + 4 * i as u32, *w);
        }
        m.set_u32(p + 0x30, curve);
        m.set_u16(p + 0x34, flag as u16);
        m.set_u32(p + 0x3c, 0xffff_ffff);
    }
    p
}

/// [soft 0x08503878 si_nurbsCompositeCurveDestroy]: `(pp)`: disposes the curve at `*pp + 0x30`,
/// frees the block and clears `*pp`.
pub fn composite_curve_destroy(m: &mut Mem, pp: u32) {
    if pp == 0 {
        return;
    }
    let p = m.u32(pp);
    if p == 0 {
        return;
    }
    nurbs_crv_dispose(m, p + 0x30);
    let p = m.u32(pp);
    if p != 0 {
        m.free(p);
    }
    m.set_u32(pp, 0);
}

/// [soft 0x08504a2c SI_nurbsCompGetOpenNC]: `(n, order, flag)`: an open curve of `n` points
/// of the given order with the clamped uniform knot vector (`order - 1` zeros, 1 2 3 ..., the
/// last value repeated `order - 1` times); 0 when the allocation fails.
pub fn comp_get_open_nc(m: &mut Mem, n: i32, order: i32, flag: u32) -> u32 {
    let c = nurbs_crv_allocate(m).addr();
    if c == 0 {
        return 0;
    }
    let count = order.wrapping_add(n).wrapping_sub(2);
    m.set_u32(c + 8, count as u32);
    let k = knots_allocate(m, count);
    m.set_u32(c + 0xc, k);
    let tail = order.wrapping_sub(1);
    let mut i: i32 = 0;
    while i < tail {
        let kk = m.u32(c + 0xc);
        m.set_f64(kk.wrapping_add((i as u32).wrapping_mul(8)), 0.0);
        i += 1;
    }
    let mut v: i32 = 1;
    loop {
        let cnt = m.u32(c + 8) as i32;
        if !(i < cnt.wrapping_sub(order.wrapping_mul(2)).wrapping_add(order).wrapping_add(1)) {
            break;
        }
        let kk = m.u32(c + 0xc);
        m.set_f64(kk.wrapping_add((i as u32).wrapping_mul(8)), v as f64);
        v += 1;
        i += 1;
    }
    if tail > 0 {
        let mut left = tail;
        let val = v as f64;
        while left != 0 {
            let kk = m.u32(c + 0xc);
            m.set_f64(kk.wrapping_add((i as u32).wrapping_mul(8)), val);
            i += 1;
            left -= 1;
        }
    }
    let kk = m.u32(c + 0xc);
    let cnt = m.u32(c + 8) as i32;
    let lo = m.f64(kk.wrapping_add((order as u32).wrapping_mul(8)).wrapping_sub(0x10));
    m.set_f64(c + 0x10, lo);
    let hi = m.f64(kk.wrapping_add((cnt.wrapping_sub(order) as u32).wrapping_mul(8)).wrapping_add(8));
    m.set_f64(c + 0x18, hi);
    m.set_u16(c + 0x20, 0);
    let pts = nurbs_points_allocate(m, n);
    m.set_u32(c + 0x2c, pts);
    m.set_u32(c + 4, n as u32);
    m.set_u32(c, order as u32);
    m.set_u32(c + 0x34, flag);
    c
}

/// [soft 0x08504b28 SI_nurbsCompGetClosedNC]: `(n, order, flag)`: a closed (periodic) curve of
/// `n` points: knots 0 1 2 .. n (n + 1 of them), the range the first to the last knot.
pub fn comp_get_closed_nc(m: &mut Mem, n: i32, order: i32, flag: u32) -> u32 {
    let c = nurbs_crv_allocate(m).addr();
    if c == 0 {
        return 0;
    }
    m.set_u32(c + 8, n.wrapping_add(1) as u32);
    let k = knots_allocate(m, n.wrapping_add(1));
    m.set_u32(c + 0xc, k);
    let mut i: i32 = 0;
    while i < m.u32(c + 8) as i32 {
        let kk = m.u32(c + 0xc);
        m.set_f64(kk.wrapping_add((i as u32).wrapping_mul(8)), i as f64);
        i += 1;
    }
    m.set_u16(c + 0x20, 1);
    let pts = nurbs_points_allocate(m, n);
    m.set_u32(c + 0x2c, pts);
    m.set_u32(c + 4, n as u32);
    m.set_u32(c, order as u32);
    m.set_u32(c + 0x34, flag);
    let kk = m.u32(c + 0xc);
    let cnt = m.u32(c + 8);
    let lo = m.f64(kk);
    m.set_f64(c + 0x10, lo);
    let hi = m.f64(kk.wrapping_add(cnt.wrapping_mul(8)).wrapping_sub(8));
    m.set_f64(c + 0x18, hi);
    c
}

/// [soft 0x08505db4 getBorder]: `(comp, which)`: from the border bits of the point record's
/// flags at +0x20 (mask 0x1e) the border where a run starts (`which` nonzero) or ends (zero):
/// one border bit gives itself; two adjacent bits (a corner) give the one the run leaves by
/// or enters by; any other mask gives 0.
pub fn get_border(m: &Mem, comp: u32, which: u16) -> u32 {
    let f = m.u32(comp + 0x20) & 0x1e;
    let w = which != 0;
    match f {
        2 | 4 | 8 | 0x10 => f,
        6 => f & if w { 4 } else { 2 },
        0x12 => f & if w { 2 } else { 0x10 },
        0xc => f & if w { 8 } else { 4 },
        0x18 => f & if w { 0x10 } else { 8 },
        _ => 0,
    }
}

/// [soft 0x08505ea8 nextBorder]: the border after `b` going round the box: 2, 4, 8, 0x10, 2;
/// any other value comes back unchanged.
pub fn next_border(b: u32) -> u32 {
    match b {
        2 => 4,
        4 => 8,
        8 => 0x10,
        0x10 => 2,
        o => o,
    }
}

/// [soft 0x08505e18 allCorners]: `(a, b, x, y)`: 0 unless the two borders are equal; then
/// whether the point `b` is on the far side of `a` along the border's axis (border 2: `b.y >= a.y`
/// ; 4: not `b.x > a.x`; 8: not `b.y > a.y`; 0x10: `b.x >= a.x`; the x87 compare, so a NaN
/// satisfies the "not greater" forms only).
pub fn all_corners(m: &Mem, a: u32, b: u32, x: u32, y: u32) -> u16 {
    if x != y {
        return 0;
    }
    let (ax, ay, bx, by) = (m.f64(a), m.f64(a + 8), m.f64(b), m.f64(b + 8));
    let r = match x {
        2 => by >= ay,
        4 => !(bx > ax),
        8 => !(by > ay),
        0x10 => bx >= ax,
        _ => return 0,
    };
    r as u16
}

/// [soft 0x08505ef0 addCorner]: `(comp, idx, border, box)`: writes the box corner of the
/// border (2: min x, min y; 4: max x, min y; 8: max x, max y; 0x10: min x, max y) as the
/// point `*idx` of the polyline at comp+0x2c (x at +0, y at +8) and steps `*idx`; any other
/// border does nothing.
pub fn add_corner(m: &mut Mem, comp: u32, idx: u32, border: u32, bx: u32) {
    let (xi, yi) = match border {
        2 => (0, 1),
        4 => (3, 1),
        8 => (3, 4),
        0x10 => (0, 4),
        _ => return,
    };
    let xv = m.f64(bx + 8 * xi);
    let yv = m.f64(bx + 8 * yi);
    let i = m.u32(idx);
    let pts = m.u32(comp + 0x2c);
    m.set_f64(pts.wrapping_add(i.wrapping_mul(0x24)), xv);
    let i = m.u32(idx);
    let pts = m.u32(comp + 0x2c);
    m.set_f64(pts.wrapping_add(i.wrapping_mul(0x24)).wrapping_add(8), yv);
    let i = m.u32(idx);
    m.set_u32(idx, i.wrapping_add(1));
}

/// [soft 0x08505f78 insertCorners]: `(comp, idx, from, to, box)`: the corners of the box
/// between the border `from` leaves by and the border `to` enters by: [`add_corner`] and
/// [`next_border`] from the first until the second is reached; when [`all_corners`] says the
/// two points already lie in order, the first corner is written before the test (a full turn
/// when the two borders are equal). No guard against a zero border (the original loops).
pub fn insert_corners(m: &mut Mem, comp: u32, idx: u32, from: u32, to: u32, bx: u32) {
    let mut e = get_border(m, from, 1);
    let s = get_border(m, to, 0);
    if all_corners(m, from, to, e, s) != 0 {
        add_corner(m, comp, idx, e, bx);
        e = next_border(e);
    }
    while e != s {
        add_corner(m, comp, idx, e, bx);
        e = next_border(e);
    }
}

/// [soft 0x08505fec onBorderFlags]: `(p, box)`: marks the point's flags (byte at +0x20) with
/// 2 / 8 when x is within 1e-6 (strictly) of the box's min x / max x, with 4 / 0x10 for y
/// against min y / max y; 1 when any was set.
pub fn on_border_flags(m: &mut Mem, p: u32, bx: u32) -> u16 {
    let mut any = 0u16;
    let t = F80::from_f64(BORDER_TOL);
    let px = F80::from_f64(m.f64(p));
    let flag = |m: &mut Mem, v: u8| {
        let o = m.u8(p + 0x20);
        m.set_u8(p + 0x20, o | v);
    };
    if t.gt((px - F80::from_f64(m.f64(bx))).abs()) {
        flag(m, 2);
        any = 1;
    } else if (px - F80::from_f64(m.f64(bx + 0x18))).abs().lt(t) {
        flag(m, 8);
        any = 1;
    }
    let py = F80::from_f64(m.f64(p + 8));
    if t.gt((py - F80::from_f64(m.f64(bx + 8))).abs()) {
        flag(m, 4);
        any = 1;
    } else if (py - F80::from_f64(m.f64(bx + 0x20))).abs().lt(t) {
        flag(m, 0x10);
        any = 1;
    }
    any
}

/// [soft 0x08506088 indexDestroy]: `(pp)`: frees `*pp` if set and clears it.
pub fn index_destroy(m: &mut Mem, pp: u32) {
    let p = m.u32(pp);
    if p != 0 {
        m.free(p);
    }
    m.set_u32(pp, 0);
}

/// [soft 0x085060b4 indexEqual]: `(a, b)`: whether `*a == *b` (words).
pub fn index_equal(m: &Mem, a: u32, b: u32) -> u32 {
    (m.u32(a) == m.u32(b)) as u32
}

/// [soft 0x08507508 insertCornerWrap]: `(comp)`: whether a corner has to be inserted between
/// the last point (flags at point `n - 1`, `n` at +4) and the first (+0x2c): 1 when the last
/// has bit 1; 0 when it has no border bit; else 1 when the first has bit 1, 0 when it has no
/// border bit, and otherwise 1 only if the two share no flag bit.
pub fn insert_corner_wrap(m: &Mem, comp: u32) -> u32 {
    let n = m.u32(comp + 4);
    let pts = m.u32(comp + 0x2c);
    let last = m.u32(pts.wrapping_add(n.wrapping_mul(0x24)).wrapping_sub(4));
    if last & 1 != 0 {
        return 1;
    }
    if last & 0x1e == 0 {
        return 0;
    }
    let first = m.u32(pts + 0x20);
    if first & 1 != 0 {
        return 1;
    }
    if first & 0x1e == 0 {
        return 0;
    }
    (first & last == 0) as u32
}

/// `SI_genListIterInit (0x08514fb8)` (genList.c, `si-gen`): `(list, &first, &last)`. This crate does not depend on `si-gen`, so
/// the list is reached through [`Outside`].
pub const SI_GEN_LIST_ITER_INIT: Routine = Routine::new("SI_genListIterInit", 0x0851_4fb8);
/// `SI_genListIterNext (0x08514ff4)`: `(list, &first, &last)`.
pub const SI_GEN_LIST_ITER_NEXT: Routine = Routine::new("SI_genListIterNext", 0x0851_4ff4);
/// `SI_genListElemGetData (0x085153d4)`: `(element)`.
pub const SI_GEN_LIST_ELEM_GET_DATA: Routine = Routine::new("SI_genListElemGetData", 0x0851_53d4);
/// `SI_genListInsertE (0x08515154)`: `(list, data)`.
pub const SI_GEN_LIST_INSERT_E: Routine = Routine::new("SI_genListInsertE", 0x0851_5154);
/// `SI_statusDisplayMessage (0x083cca94)`: `(text, level)`.
pub const SI_STATUS_DISPLAY_MESSAGE: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);
/// `si_nurbsCurveInterUpdateBbox (0x08508250)` (curveInter.c, not ported): `(&result box[6 doubles], box[6 doubles] by value,
/// point[3 doubles] by value)`.
pub const SI_NURBS_CURVE_INTER_UPDATE_BBOX: Routine = Routine::new("si_nurbsCurveInterUpdateBbox", 0x0850_8250);
/// `si_nurbsCurveInterPointOnLine (0x08509a44)` (curveInter.c, not ported): `(three points of 3 doubles by value)`, a 16-bit answer.
pub const SI_NURBS_CURVE_INTER_POINT_ON_LINE: Routine = Routine::new("si_nurbsCurveInterPointOnLine", 0x0850_9a44);

/// The text at 0x086daea0.
const TXT_IGNORING_A: u32 = 0x086d_aea0;
/// The text at 0x086db000: "Loop detected in building composite curve."
const TXT_LOOP: u32 = 0x086d_b000;
/// The text at 0x086db040: "Ignoring projected curve with point count less than curve order."
const TXT_IGNORING_B: u32 = 0x086d_b040;

/// [soft 0x08506f94 si_nurbsCompositeCurvePointOnBorder]: `(x, y, box)` with the box as the six doubles the original
/// receives by value (the third and sixth unused): whether the point is within 1e-6 (strictly) of the box's min x, max x,
/// min y or max y.
pub fn composite_curve_point_on_border(px: f64, py: f64, bx: [f64; 6]) -> u32 {
    let t = F80::from_f64(BORDER_TOL);
    let (fx, fy) = (F80::from_f64(px), F80::from_f64(py));
    if t.gt((fx - F80::from_f64(bx[0])).abs()) {
        return 1;
    }
    if (fx - F80::from_f64(bx[3])).abs().lt(t) {
        return 1;
    }
    if (fy - F80::from_f64(bx[1])).abs().lt(t) {
        return 1;
    }
    ((fy - F80::from_f64(bx[4])).abs().lt(t)) as u32
}

/// [soft 0x085039fc SI_nurbsSrfCrvOnBorder]: `(srf, comp)`: 1 when every point of the polyline `comp` (count at +4, points at
/// +0x2c) is on the border of the surface's parameter box (u range at +0x10 / +0x18, v range at +0x34 / +0x3c), else 0 at the
/// first that is not. The two unset cells of the original's box are never read.
pub fn srf_crv_on_border(m: &Mem, srf: u32, comp: u32) -> u32 {
    let bx = [m.f64(srf + 0x10), m.f64(srf + 0x34), 0.0, m.f64(srf + 0x18), m.f64(srf + 0x3c), 0.0];
    let n = m.i32(comp + 4);
    let mut at = 0u32;
    let mut i = 0;
    while i < n {
        let pts = m.u32(comp + 0x2c);
        let (px, py) = (m.f64(pts.wrapping_add(at)), m.f64(pts.wrapping_add(at).wrapping_add(8)));
        if composite_curve_point_on_border(px, py, bx) == 0 {
            return 0;
        }
        i += 1;
        at = at.wrapping_add(0x24);
    }
    1
}

fn elem_data(m: &mut Mem, o: &mut dyn Outside, e: u32) -> u32 {
    o.call(m, SI_GEN_LIST_ELEM_GET_DATA, &[e]).eax
}

fn insert_e(m: &mut Mem, o: &mut dyn Outside, list: u32, data: u32) {
    o.call(m, SI_GEN_LIST_INSERT_E, &[list, data]);
}

fn message(m: &mut Mem, o: &mut dyn Outside, text: u32, level: u32) {
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[text, level]);
}

/// [soft 0x08503a98 addCompositeNoInter]: `(item, list_a, list_b)`: builds the composite curve of a projected curve that
/// has no self-intersection record. `item`: the list of its points at +0 (data records: x +0, y +8, knot +0x30, flag word
/// +0x40; the last also x +0x18, y +0x20, knot +0x38, flag +0x44), the box (12 words) at +4, a 16-bit flag at +0x38, a 16-bit
/// "open" word at +0x3a, the order at +0x3c, the curve's +0x34 value at +0x40, +0x44 and +0x48 for the record. A curve of fewer
/// points than the order is refused with a message (level 2) and answers 0; an open one gets one more point than the list
/// (the last record's end point). The composite goes to `list_b` when its flag word is set, else to `list_a`. Answers 1, or
/// 0 when an allocation failed.
pub fn add_composite_no_inter(m: &mut Mem, o: &mut dyn Outside, item: u32, list_a: u32, list_b: u32) -> u32 {
    let sp = m.sp();
    let r = no_inter(m, o, item, list_a, list_b);
    m.set_sp(sp);
    r
}

fn no_inter(m: &mut Mem, o: &mut dyn Outside, item: u32, list_a: u32, list_b: u32) -> u32 {
    let open = m.u16(item + 0x3a) == 0;
    let list = m.u32(item);
    let n = m.i32(list + 0x10);
    if n < m.i32(item + 0x3c) {
        message(m, o, TXT_IGNORING_A, 2);
        return 0;
    }
    let nc = if !open { comp_get_closed_nc(m, n, 2, 1) } else { comp_get_open_nc(m, n.wrapping_add(1), 2, 1) };
    if nc == 0 {
        return 0;
    }
    let v = m.u32(item + 0x40);
    m.set_u32(nc + 0x34, v);
    let flags = m.local(4);
    let (first, last) = (flags, flags + 2);
    let mut i: u32 = 0;
    let mut at: u32 = 0;
    let mut data = 0u32;
    let mut e = o.call(m, SI_GEN_LIST_ITER_INIT, &[m.u32(item), first, last]).eax;
    if m.u16(first) != 1 {
        loop {
            data = elem_data(m, o, e);
            let pts = m.u32(nc + 0x2c);
            let (dx, dy) = (m.f64(data), m.f64(data + 8));
            m.set_f64(pts.wrapping_add(at), dx);
            let pts = m.u32(nc + 0x2c);
            m.set_f64(pts.wrapping_add(at).wrapping_add(8), dy);
            let kn = m.u32(nc + 0xc);
            let k = m.f64(data + 0x30);
            m.set_f64(kn.wrapping_add(i.wrapping_mul(8)), k);
            let pts = m.u32(nc + 0x2c);
            let f = m.u32(pts.wrapping_add(at).wrapping_add(0x20)) | m.u32(data + 0x40);
            m.set_u32(pts.wrapping_add(at).wrapping_add(0x20), f);
            e = o.call(m, SI_GEN_LIST_ITER_NEXT, &[m.u32(item), first, last]).eax;
            at = at.wrapping_add(0x24);
            i += 1;
            if m.u16(first) == 1 {
                break;
            }
        }
    }
    if open {
        let off = i.wrapping_mul(0x24);
        let pts = m.u32(nc + 0x2c);
        let v = m.f64(data + 0x18);
        m.set_f64(pts.wrapping_add(off), v);
        let pts = m.u32(nc + 0x2c);
        let v = m.f64(data + 0x20);
        m.set_f64(pts.wrapping_add(off).wrapping_add(8), v);
        let kn = m.u32(nc + 0xc);
        let v = m.f64(data + 0x38);
        m.set_f64(kn.wrapping_add(i.wrapping_mul(8)), v);
        let pts = m.u32(nc + 0x2c);
        let v = m.u32(data + 0x44);
        m.set_u32(pts.wrapping_add(off).wrapping_add(0x20), v);
    }
    let mut bx = [0u32; 12];
    for (w, c) in bx.iter_mut().enumerate() {
        *c = m.u32(item + 4 + 4 * w as u32);
    }
    let flag = m.i16(item + 0x38) as i32 as u32;
    let comp = composite_curve_create(m, nc, bx, flag);
    if comp == 0 {
        return 0;
    }
    let v = m.u32(item + 0x48);
    m.set_u32(comp + 0x40, v);
    let v = m.u32(item + 0x3c);
    m.set_u32(comp + 0x38, v);
    let v = m.u32(item + 0x44);
    m.set_u32(comp + 0x3c, v);
    if m.u16(comp + 0x34) != 0 {
        insert_e(m, o, list_b, comp);
    } else {
        insert_e(m, o, list_a, comp);
    }
    1
}

/// [soft 0x08504f10 findOrientation]: `(elem)`: walks the ring of elements from `elem` (next at +8) until three successive
/// points are not on a line (`si_nurbsCurveInterPointOnLine` answers 0) or the ring is back at `elem`; the points are the
/// element's data x y z at +0 and at +0x18 and the data of its neighbour (the element at data+0x78, else the previous one
/// at element+4). Answers 0 when the z of the cross product of (second - first) and (neighbour - first) is positive, else 1
/// (the x87 sequence in 80 bits, nothing rounded between).
pub fn find_orientation(m: &mut Mem, o: &mut dyn Outside, start: u32) -> u32 {
    let mut e = start;
    let (mut p0, mut p1, mut p2) = ([0f64; 3], [0f64; 3], [0f64; 3]);
    loop {
        let d = elem_data(m, o, e);
        for k in 0..3 {
            p0[k] = m.f64(d + 8 * k as u32);
            p1[k] = m.f64(d + 0x18 + 8 * k as u32);
        }
        let d = elem_data(m, o, e);
        let mut alt = m.u32(d + 0x78);
        if alt == 0 {
            alt = m.u32(e + 4);
        }
        let d2 = elem_data(m, o, alt);
        for k in 0..3 {
            p2[k] = m.f64(d2 + 8 * k as u32);
        }
        e = m.u32(e + 8);
        let mut args = [0u32; 18];
        for (i, p) in [p2, p1, p0].iter().enumerate() {
            for k in 0..3 {
                let b = p[k].to_bits();
                args[6 * i + 2 * k] = b as u32;
                args[6 * i + 2 * k + 1] = (b >> 32) as u32;
            }
        }
        let r = o.call(m, SI_NURBS_CURVE_INTER_POINT_ON_LINE, &args).eax as u16;
        if r == 0 || e == start {
            break;
        }
    }
    let f = F80::from_f64;
    let dx1 = f(p1[0]) - f(p0[0]);
    let dy1 = f(p1[1]) - f(p0[1]);
    let dx2 = f(p2[0]) - f(p0[0]);
    let dy2 = f(p2[1]) - f(p0[1]);
    let cz = dx1 * dy2 - dx2 * dy1;
    (!F80::from_f64(0.0).lt(cz)) as u32
}

/// [soft 0x08504bb0 addCompositeInter]: `(item, list_a, list_b, list_c)`: for a projected curve with self-intersection
/// records. Walks the ring of elements of the item's list (`item + 0`); an element whose data has neither +0x78 nor +0x7c, or
/// has bit 4 of +0x84 set (already taken), is passed over. Otherwise a loop is followed from it (next = the element at
/// data+0x7c, else the next one at element+8) until it is back at the first (marking each data +0x84 bit 4, widening a
/// box that starts at +/-3.4028235e38 through `si_nurbsCurveInterUpdateBbox` with the point at data+0, remembering the
/// element whose x is greatest); more than 100000 steps is "Loop detected in building composite curve." (level 3, answer
/// 0); fewer points than the largest order met so far is "Ignoring projected curve with point count less than curve
/// order." (level 2, the element is passed over). Then a closed two-order curve of those points
/// (x, y, knot data+0x30, flags data+0x40), whose orientation is [`find_orientation`] of the element with the greatest x; the
/// order (record at data+0x80, +0x3c) and the +0x40 value of that record are kept as the largest met (the order) and
/// the value of the record of that order (the curve's +0x34). With `list_c` set the curve itself goes there, else a composite
/// of it (box, orientation as the flag, +0x40 = the item's +0x48, +0x38 = the order) to `list_b` when its flag word is set,
/// else to `list_a`. Answers 1, or 0 on the loop or an allocation failure.
pub fn add_composite_inter(m: &mut Mem, o: &mut dyn Outside, item: u32, list_a: u32, list_b: u32, list_c: u32) -> u32 {
    let sp = m.sp();
    let r = inter(m, o, item, list_a, list_b, list_c);
    m.set_sp(sp);
    r
}

fn inter(m: &mut Mem, o: &mut dyn Outside, item: u32, list_a: u32, list_b: u32, list_c: u32) -> u32 {
    let big = f64::from_bits(0x47ef_ffff_e000_0000);
    let mut best_x = -big;
    let mut best: u32 = 0;
    let mut max_order: i32 = 0;
    let mut kept: i32 = 0;
    let flags = m.local(4);
    let (first, last) = (flags, flags + 2);
    let bbox = m.local(0x30);
    let upd = m.local(0x30);
    let mut e = o.call(m, SI_GEN_LIST_ITER_INIT, &[m.u32(item), first, last]).eax;
    while m.u16(first) != 1 {
        let data = elem_data(m, o, e);
        let skip = (m.u32(data + 0x78) == 0 && m.u32(data + 0x7c) == 0) || (m.u8(data + 0x84) & 4) != 0;
        if !skip {
            m.set_f64(bbox, big);
            m.set_f64(bbox + 8, big);
            m.set_f64(bbox + 0x18, -big);
            m.set_f64(bbox + 0x20, -big);
            let mut cur = e;
            let mut count: i32 = 0;
            loop {
                count += 1;
                if count > 100000 {
                    message(m, o, TXT_LOOP, 3);
                    return 0;
                }
                let d = elem_data(m, o, cur);
                let b = m.u8(d + 0x84);
                m.set_u8(d + 0x84, b | 4);
                let mut args = [0u32; 19];
                args[0] = upd;
                for w in 0..12 {
                    args[1 + w] = m.u32(bbox + 4 * w as u32);
                }
                for w in 0..6 {
                    args[13 + w] = m.u32(d + 4 * w as u32);
                }
                o.call(m, SI_NURBS_CURVE_INTER_UPDATE_BBOX, &args);
                for w in 0..12 {
                    let v = m.u32(upd + 4 * w as u32);
                    m.set_u32(bbox + 4 * w as u32, v);
                }
                let xv = F80::from_f64(m.f64(d));
                if xv.gt(F80::from_f64(best_x)) {
                    best_x = xv.to_f64();
                    best = cur;
                }
                let nx = m.u32(d + 0x7c);
                cur = if nx != 0 { nx } else { m.u32(cur + 8) };
                if cur == e {
                    break;
                }
            }
            if count < max_order {
                message(m, o, TXT_IGNORING_B, 2);
            } else {
                let orient = find_orientation(m, o, best);
                let nc = comp_get_closed_nc(m, count, 2, 1);
                if nc == 0 {
                    return 0;
                }
                let mut cur = e;
                let mut i: i32 = 0;
                let mut at: u32 = 0;
                loop {
                    let d = elem_data(m, o, cur);
                    let rec = m.u32(d + 0x80);
                    let (ord, val) = (m.i32(rec + 0x3c), m.i32(rec + 0x40));
                    if ord == max_order && val > kept {
                        kept = val;
                    } else if ord > max_order {
                        kept = val;
                    } else if kept == 0 {
                        kept = val;
                    }
                    if ord > max_order {
                        max_order = ord;
                    }
                    let pts = m.u32(nc + 0x2c);
                    let v = m.f64(d);
                    m.set_f64(pts.wrapping_add(at), v);
                    let pts = m.u32(nc + 0x2c);
                    let v = m.f64(d + 8);
                    m.set_f64(pts.wrapping_add(at).wrapping_add(8), v);
                    let kn = m.u32(nc + 0xc);
                    let v = m.f64(d + 0x30);
                    m.set_f64(kn.wrapping_add((i as u32).wrapping_mul(8)), v);
                    let pts = m.u32(nc + 0x2c);
                    let v = m.u32(d + 0x40);
                    m.set_u32(pts.wrapping_add(at).wrapping_add(0x20), v);
                    let nx = m.u32(d + 0x7c);
                    cur = if nx != 0 { nx } else { m.u32(cur + 8) };
                    at = at.wrapping_add(0x24);
                    i += 1;
                    if cur == e {
                        break;
                    }
                }
                if list_c != 0 {
                    insert_e(m, o, list_c, nc);
                } else {
                    let mut bx = [0u32; 12];
                    for (w, c) in bx.iter_mut().enumerate() {
                        *c = m.u32(bbox + 4 * w as u32);
                    }
                    let comp = composite_curve_create(m, nc, bx, orient as u16 as i16 as i32 as u32);
                    if comp == 0 {
                        return 0;
                    }
                    let v = m.u32(item + 0x48);
                    m.set_u32(comp + 0x40, v);
                    m.set_u32(comp + 0x38, max_order as u32);
                    let c = m.u32(comp + 0x30);
                    m.set_u32(c + 0x34, kept as u32);
                    if m.u16(comp + 0x34) != 0 {
                        insert_e(m, o, list_b, comp);
                    } else {
                        insert_e(m, o, list_a, comp);
                    }
                }
            }
        }
        e = o.call(m, SI_GEN_LIST_ITER_NEXT, &[m.u32(item), first, last]).eax;
    }
    1
}

/// [soft 0x08503978 si_nurbsCompositeCurveBuild]: `(list, list_a, list_b, list_c)`: for each item of `list`, a projected curve
/// with bit 8 of its +0x48 word clear goes to [`add_composite_no_inter`] (`list_a`, `list_b`), one with it set to
/// [`add_composite_inter`] (`list_a`, `list_b`, `list_c`); the answers are not looked at. Answers 1.
pub fn composite_curve_build(m: &mut Mem, o: &mut dyn Outside, list: u32, list_a: u32, list_b: u32, list_c: u32) -> u32 {
    let sp = m.sp();
    let flags = m.local(4);
    let (first, last) = (flags, flags + 2);
    let mut e = o.call(m, SI_GEN_LIST_ITER_INIT, &[list, first, last]).eax;
    while m.u16(first) != 1 {
        let item = elem_data(m, o, e);
        if m.u8(item + 0x48) & 8 == 0 {
            add_composite_no_inter(m, o, item, list_a, list_b);
        } else {
            add_composite_inter(m, o, item, list_a, list_b, list_c);
        }
        e = o.call(m, SI_GEN_LIST_ITER_NEXT, &[list, first, last]).eax;
    }
    m.set_sp(sp);
    1
}

/// `SI_genListCreate (0x08514ef0)`: `()`, a new empty list.
pub const SI_GEN_LIST_CREATE: Routine = Routine::new("SI_genListCreate", 0x0851_4ef0);
/// `SI_genListDestroy (0x08514f08)`: `(&list, destroy)`.
pub const SI_GEN_LIST_DESTROY: Routine = Routine::new("SI_genListDestroy", 0x0851_4f08);
/// `si_nurbsRayDist3D (0x0850dcd8)` (ray.c, not ported): `(a[3 doubles], b[3 doubles])`, the **squared** distance of the two
/// points in st0 (unrounded).
pub const SI_NURBS_RAY_DIST_3D: Routine = Routine::new("si_nurbsRayDist3D", 0x0850_dcd8);
/// The C library's `fprintf` through the binary's stub.
pub const FPRINTF: Routine = Routine::new("fprintf", 0x080c_3b88);
/// "Error: Bad KNOT count in rebuild Knot Vector!\n"
const TXT_BAD_KNOT_COUNT: u32 = 0x086d_af00;
/// The word holding `stderr`.
const STDERR: u32 = 0x0878_0c88;

fn st0_f64(r: si_mem::Ret) -> f64 {
    r.st0.map_or(0.0, |b| F80::from_bits(u64::from_le_bytes(b[..8].try_into().unwrap()), u16::from_le_bytes([b[8], b[9]])).to_f64())
}

fn dbl_words(args: &mut [u32], at: usize, v: f64) {
    let b = v.to_bits();
    args[at] = b as u32;
    args[at + 1] = (b >> 32) as u32;
}

/// [soft 0x08503c64 rebuildKnotVector]: `(crv, out, order)`: replaces the knots of the trim curve `crv` (order 2 = a polyline:
/// points at +0x2c, 0x24 bytes each, count at +4; knots at +0xc, count at +8; closed word at +0x20) by the cumulative
/// chord lengths, and hands the knots for `order` to `out` (count at +8, array at +0xc, order +0, closed word +0x20, range
/// +0x10 / +0x18, point count +4).
/// - Order 2: the knots become 0 1 2 ..., the range their first and last, `out`'s knot pointer 0; answers 1.
/// - Otherwise the curve is rotated to start at its first point whose flag word (+0x20) has bit 16 (none: at index 0 when
///   it has points, which is the count when none is flagged), the running length `acc` starts at 0 and, for each point in
///   that order, the point's knot becomes `acc` and `acc` grows by the old knot (or, when that is within 1e-6 of zero,
///   by `si_nurbsRayDist3D` of the point and the next, z = 0: a squared distance, as the original does). A point with bit 16
///   puts `acc` on a list, one with bit 20 puts `order - 2` more copies; the point's flag word is then cleared. A closed
///   curve puts the final `acc` on the list and in the last knot. A rotated curve gets new knot and point arrays
///   (`calloc`, the old ones freed; a failure answers 0). `out`'s knots are the list in order, the list is destroyed.
/// A failed allocation answers 0 (the list not given back, as in the original).
pub fn rebuild_knot_vector(m: &mut Mem, o: &mut dyn Outside, crv: u32, out: u32, order: i32) -> u32 {
    let sp = m.sp();
    let r = rebuild_knots(m, o, crv, out, order);
    m.set_sp(sp);
    r
}

fn rebuild_knots(m: &mut Mem, o: &mut dyn Outside, crv: u32, out: u32, order: i32) -> u32 {
    let tol = F80::from_f64(BORDER_TOL);
    if order == 2 {
        let nk = m.i32(crv + 8);
        let mut i = 0;
        while i < nk {
            let kn = m.u32(crv + 0xc);
            m.set_f64(kn.wrapping_add((i as u32).wrapping_mul(8)), i as f64);
            i += 1;
        }
        let kn = m.u32(crv + 0xc);
        let v = m.f64(kn);
        m.set_f64(crv + 0x10, v);
        let nk = m.u32(crv + 8);
        let v = m.f64(kn.wrapping_add(nk.wrapping_mul(8)).wrapping_sub(8));
        m.set_f64(crv + 0x18, v);
        m.set_u32(out + 0xc, 0);
        return 1;
    }
    let list = o.call(m, SI_GEN_LIST_CREATE, &[]).eax;
    if list == 0 {
        return 0;
    }
    let plist = m.local(4);
    m.set_u32(plist, list);
    let n = m.i32(crv + 4);
    // the first point with bit 16 of its flag word (the count when there is none)
    let mut s: i32 = 0;
    if s < n {
        let pts = m.u32(crv + 0x2c);
        if m.u8(pts.wrapping_add(0x22)) & 1 == 0 {
            let mut at = pts.wrapping_add(0x20);
            loop {
                at = at.wrapping_add(0x24);
                s += 1;
                if s >= n {
                    break;
                }
                if m.u8(at.wrapping_add(2)) & 1 != 0 {
                    break;
                }
            }
        }
    }
    let mut acc: f64 = 0.0;
    let mut i: i32 = 0;
    let mut n = m.i32(crv + 4);
    'outer: while i < n {
        let idx = (s.wrapping_add(i)).wrapping_rem(n);
        let kv = m.f64(m.u32(crv + 0xc).wrapping_add((idx as u32).wrapping_mul(8)));
        let mut k = kv;
        if F80::from_f64(kv).abs().lt(tol) {
            let pts = m.u32(crv + 0x2c);
            let nx = (s.wrapping_add(i).wrapping_add(1)).wrapping_rem(m.i32(crv + 4));
            let a_at = pts.wrapping_add((idx as u32).wrapping_mul(0x24));
            let b_at = pts.wrapping_add((nx as u32).wrapping_mul(0x24));
            let mut args = [0u32; 12];
            dbl_words(&mut args, 0, m.f64(a_at));
            dbl_words(&mut args, 2, m.f64(a_at + 8));
            dbl_words(&mut args, 6, m.f64(b_at));
            dbl_words(&mut args, 8, m.f64(b_at + 8));
            let r = o.call(m, SI_NURBS_RAY_DIST_3D, &args);
            k = st0_f64(r);
        }
        let kn = m.u32(crv + 0xc);
        m.set_f64(kn.wrapping_add((idx as u32).wrapping_mul(8)), acc);
        let off = (idx as u32).wrapping_mul(0x24);
        let pts = m.u32(crv + 0x2c);
        if m.u8(pts.wrapping_add(off).wrapping_add(0x22)) & 1 != 0 {
            let c = m.si_calloc(1, 8);
            if c == 0 {
                break 'outer;
            }
            m.set_f64(c, acc);
            o.call(m, SI_GEN_LIST_INSERT_E, &[list, c]);
        }
        let nacc = (F80::from_f64(acc) + F80::from_f64(k)).to_f64();
        let pts = m.u32(crv + 0x2c);
        if m.u8(pts.wrapping_add(off).wrapping_add(0x22)) & 0x10 != 0 {
            let mut j = 0;
            while j < order.wrapping_sub(2) {
                let c = m.si_calloc(1, 8);
                if c == 0 {
                    break;
                }
                m.set_f64(c, acc);
                o.call(m, SI_GEN_LIST_INSERT_E, &[list, c]);
                j += 1;
            }
        }
        let pts = m.u32(crv + 0x2c);
        m.set_u32(pts.wrapping_add(off).wrapping_add(0x20), 0);
        acc = nacc;
        i += 1;
        n = m.i32(crv + 4);
    }
    if m.u16(crv + 0x20) != 0 {
        let c = m.si_calloc(1, 8);
        if c != 0 {
            let nk = m.u32(crv + 8);
            let kn = m.u32(crv + 0xc);
            m.set_f64(kn.wrapping_add(nk.wrapping_mul(8)).wrapping_sub(8), acc);
            m.set_f64(c, acc);
            o.call(m, SI_GEN_LIST_INSERT_E, &[list, c]);
        }
    }
    if s > 0 {
        let old = m.u32(crv + 0xc);
        let nk = m.i32(crv + 8);
        let nw = m.si_calloc(nk as u32, 8);
        m.set_u32(crv + 0xc, nw);
        if nw == 0 {
            return 0;
        }
        let nk = m.i32(crv + 8);
        let mut j = 0;
        while j < nk {
            let src = s.wrapping_add(j).wrapping_rem(nk);
            let v = m.f64(old.wrapping_add((src as u32).wrapping_mul(8)));
            let kn = m.u32(crv + 0xc);
            m.set_f64(kn.wrapping_add((j as u32).wrapping_mul(8)), v);
            j += 1;
        }
        if old != 0 {
            m.free(old);
        }
        let oldp = m.u32(crv + 0x2c);
        let np = m.si_calloc(m.u32(crv + 4), 0x24);
        m.set_u32(crv + 0x2c, np);
        if np == 0 {
            return 0;
        }
        let nn = m.i32(crv + 4);
        let mut j = 0;
        let mut dst = 0u32;
        while j < nn {
            let src = s.wrapping_add(j).wrapping_rem(nn);
            let from = oldp.wrapping_add((src as u32).wrapping_mul(0x24));
            let to = m.u32(crv + 0x2c).wrapping_add(dst);
            for w in 0..9 {
                let v = m.u32(from + 4 * w);
                m.set_u32(to + 4 * w, v);
            }
            dst = dst.wrapping_add(0x24);
            j += 1;
        }
        if oldp != 0 {
            m.free(oldp);
        }
    }
    let kn = m.u32(crv + 0xc);
    let nk = m.u32(crv + 8);
    let v = m.f64(kn);
    m.set_f64(crv + 0x10, v);
    let v = m.f64(kn.wrapping_add(nk.wrapping_mul(8)).wrapping_sub(8));
    m.set_f64(crv + 0x18, v);
    let count = m.u32(list + 0x10);
    m.set_u32(out + 8, count);
    let arr = m.si_calloc(count, 8);
    m.set_u32(out + 0xc, arr);
    if arr == 0 {
        return 0;
    }
    let flags = m.local(4);
    let (first, last) = (flags, flags + 2);
    let mut j: i32 = 0;
    let mut e = o.call(m, SI_GEN_LIST_ITER_INIT, &[list, first, last]).eax;
    if m.u16(first) != 1 {
        loop {
            let data = elem_data(m, o, e);
            if j < m.i32(out + 8) {
                let v = m.f64(data);
                let a = m.u32(out + 0xc);
                m.set_f64(a.wrapping_add((j as u32).wrapping_mul(8)), v);
            } else {
                let err = m.u32(STDERR);
                o.call(m, FPRINTF, &[err, TXT_BAD_KNOT_COUNT]);
            }
            if data != 0 {
                m.free(data);
            }
            e = o.call(m, SI_GEN_LIST_ITER_NEXT, &[list, first, last]).eax;
            j += 1;
            if m.u16(first) == 1 {
                break;
            }
        }
    }
    let _ = e;
    o.call(m, SI_GEN_LIST_DESTROY, &[plist, 0]);
    m.set_u32(out, order as u32);
    let c = m.u16(crv + 0x20);
    m.set_u16(out + 0x20, c);
    let ka = m.u32(out + 0xc);
    let nk = m.u32(out + 8);
    let v = m.f64(ka);
    m.set_f64(out + 0x10, v);
    let v = m.f64(ka.wrapping_add(nk.wrapping_mul(8)).wrapping_sub(8));
    m.set_f64(out + 0x18, v);
    m.set_u32(out + 4, nk.wrapping_sub(1));
    1
}

/// `SI_nurbsCompDispose (0x084dbb14)` (not ported here): `(&slot)`, frees the composite record the slot points to.
pub const SI_NURBS_COMP_DISPOSE: Routine = Routine::new("SI_nurbsCompDispose", 0x084d_bb14);

/// [soft 0x08505d00 SI_nurbsProjectedCurveDelete]: `(obj, idx)` (idx a signed 16-bit value): removes entry `idx` of the pointer
/// array at obj+0x6c (count at obj+0x68). The count drops by one first, a new array of that many words is `calloc`ed, the
/// entries before `idx` and the ones after it are copied over, `SI_nurbsCompDispose` is given the address of the old entry
/// `idx`, the old array is freed and the new one stored. A null `obj` does nothing. Always answers 1. (No range check on
/// `idx`: the original reads and copies whatever the arithmetic says, as this does.)
pub fn projected_curve_delete(m: &mut Mem, o: &mut dyn Outside, obj: u32, idx: u32) -> u32 {
    if obj == 0 {
        return 1;
    }
    let idx = idx as u16 as i16 as i32;
    let cnt = m.i32(obj + 0x68).wrapping_sub(1);
    m.set_i32(obj + 0x68, cnt);
    let nw = m.si_calloc(cnt as u32, 4);
    let mut i = 0;
    while i < idx {
        let v = m.u32(m.u32(obj + 0x6c).wrapping_add((i as u32).wrapping_mul(4)));
        m.set_u32(nw.wrapping_add((i as u32).wrapping_mul(4)), v);
        i += 1;
    }
    let mut i = idx;
    while i < m.i32(obj + 0x68) {
        let v = m.u32(m.u32(obj + 0x6c).wrapping_add((i as u32).wrapping_add(1).wrapping_mul(4)));
        m.set_u32(nw.wrapping_add((i as u32).wrapping_mul(4)), v);
        i += 1;
    }
    let old = m.u32(obj + 0x6c);
    o.call(m, SI_NURBS_COMP_DISPOSE, &[old.wrapping_add((idx as u32).wrapping_mul(4))]);
    let old = m.u32(obj + 0x6c);
    if old != 0 {
        m.free(old);
        m.set_u32(obj + 0x6c, 0);
    }
    m.set_u32(obj + 0x6c, nw);
    1
}

/// `SI_nurbsCrvCopy (0x084dbbf4)` (own.rs / image.rs hold the port; reached through `Outside` here): `(crv)`, a new curve.
pub const SI_NURBS_CRV_COPY: Routine = Routine::new("SI_nurbsCrvCopy", 0x084d_bbf4);

/// [soft 0x085049d8 fitFailed]: `(crv)`: the fallback of a failed fit: a copy of the curve (`SI_nurbsCrvCopy`; null: answers 0)
/// whose knots go through [`rebuild_knot_vector`] (order = the copy's first word) into a scratch curve record on the stack;
/// the knot array that call leaves in the scratch record (at +0xc) is freed and cleared. Answers the copy.
pub fn fit_failed(m: &mut Mem, o: &mut dyn Outside, crv: u32) -> u32 {
    let c = o.call(m, SI_NURBS_CRV_COPY, &[crv]).eax;
    if c == 0 {
        return 0;
    }
    let sp = m.sp();
    let out = m.local(0x24);
    rebuild_knot_vector(m, o, c, out, m.i32(c));
    let k = m.u32(out + 0xc);
    if k != 0 {
        m.free(k);
        m.set_u32(out + 0xc, 0);
    }
    m.set_sp(sp);
    c
}

/// [soft 0x08506730 nurbsSrfLinearizeTrimCrvs]: `(srf)`: a pointer array (`calloc(count, 4)`, count at srf+0x60) with, for each
/// trim composite of the table at srf+0x64, the polyline `si_nurbsCurveInterBldLinFromComp` (outside) builds from the
/// composite's curve table (+8) and count (+4); the word that call hands back through its third argument is stored in the curve's
/// first word and the composite's index in the curve's +0x38. Answers the array.
pub fn nurbs_srf_linearize_trim_crvs(m: &mut Mem, o: &mut dyn Outside, srf: u32) -> u32 {
    let sp = m.sp();
    let slot = m.local(4);
    let arr = m.si_calloc(m.u32(srf + 0x60), 4);
    let mut i = 0i32;
    while i < m.i32(srf + 0x60) {
        let comp = m.u32(m.u32(srf + 0x64).wrapping_add((i as u32).wrapping_mul(4)));
        let crv = o.call(m, crate::trimline::SI_NURBS_CURVE_INTER_BLD_LIN_FROM_COMP, &[m.u32(comp + 8), m.u32(comp + 4), slot]).eax;
        m.set_u32(arr.wrapping_add((i as u32).wrapping_mul(4)), crv);
        m.set_u32(crv, m.u32(slot));
        m.set_i32(crv + 0x38, i);
        i += 1;
    }
    m.set_sp(sp);
    arr
}

/// `si_nurbsCurveOrientPointInCurve (0x0850bf64)` (not ported here): `(poly, n, b[3 doubles], a[3 doubles], c[3 doubles])` with
/// the three points passed by value (18 words); answers a 16-bit class of the position.
pub const SI_NURBS_CURVE_ORIENT_POINT_IN_CURVE: Routine = Routine::new("si_nurbsCurveOrientPointInCurve", 0x0850_bf64);

/// [soft 0x08505150 curveInCurve]: `(a, b)` (composites; their curves at +0x30 with the point count at +4 and the polyline at
/// +0x2c, 0x24 bytes a point): 0 when either composite is missing or has no points. Otherwise A is the first point of `a`'s
/// polyline (x, y, 0); B the first point of that polyline which is not within 1e-4 of A in x, y and z-from-0 (the last one when
/// there is none; always written at least once); C is (0, 0, 1). `b`'s polyline is copied to a `calloc(count, 0x18)` array of
/// (x, y, 0) points and `si_nurbsCurveOrientPointInCurve(array, count, B, A, C)` (outside) decides; the array is freed and the
/// 16-bit answer sign-extended. A failed allocation answers 0.
pub fn curve_in_curve(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32) -> u32 {
    if a == 0 || b == 0 {
        return 0;
    }
    let (ac, bc) = (m.u32(a + 0x30), m.u32(b + 0x30));
    if m.i32(ac + 4) <= 0 || m.i32(bc + 4) == 0 {
        return 0;
    }
    let pts = m.u32(ac + 0x2c);
    let (x0, y0) = (F80::from_f64(m.f64(pts)), F80::from_f64(m.f64(pts + 8)));
    let pa = [m.f64(pts), m.f64(pts + 8), 0.0];
    let eps = F80::from_f64(1.0e-4);
    let zero = F80::from_f64(0.0);
    let mut pb = [0.0f64; 3];
    let mut i = 0i32;
    let mut at = 0u32;
    while i < m.i32(ac + 4) {
        let q = m.u32(ac + 0x2c);
        pb = [m.f64(q.wrapping_add(at)), m.f64(q.wrapping_add(at).wrapping_add(8)), 0.0];
        let q2 = m.u32(m.u32(a + 0x30) + 0x2c);
        if !(x0 - F80::from_f64(m.f64(q2.wrapping_add(at)))).abs().lt(eps) {
            break;
        }
        if !(y0 - F80::from_f64(m.f64(q2.wrapping_add(at).wrapping_add(8)))).abs().lt(eps) {
            break;
        }
        if !(zero - F80::from_f64(m.f64(q2.wrapping_add(at).wrapping_add(0x10)))).abs().lt(eps) {
            break;
        }
        at = at.wrapping_add(0x24);
        i += 1;
    }
    let n = m.i32(bc + 4);
    let arr = m.si_calloc(n as u32, 0x18);
    if arr == 0 {
        return 0;
    }
    let mut k = 0u32;
    while (k as i32) < n {
        let src = m.u32(m.u32(b + 0x30) + 0x2c).wrapping_add(k.wrapping_mul(0x24));
        let dst = arr.wrapping_add(k.wrapping_mul(0x18));
        m.set_f64(dst, m.f64(src));
        m.set_f64(dst + 8, m.f64(src + 8));
        m.set_u32(dst + 0x10, 0);
        m.set_u32(dst + 0x14, 0);
        k += 1;
    }
    let mut args = vec![arr, n as u32];
    for p in [pb, pa, [0.0, 0.0, 1.0]] {
        for v in p {
            args.push(v.to_bits() as u32);
            args.push((v.to_bits() >> 32) as u32);
        }
    }
    let r = o.call(m, SI_NURBS_CURVE_ORIENT_POINT_IN_CURVE, &args).eax;
    m.free(arr);
    r as u16 as i16 as i32 as u32
}

/// `SI_nurbsCrvDispose (0x084db870)`: `(&slot)`.
pub(crate) const CRV_DISPOSE: Routine = Routine::new("SI_nurbsCrvDispose", 0x084d_b870);

/// [soft 0x08506424 linearTrimCrvsAdjust]: `(curves, list, box)`: for every element of the genList `list` (its data a pointer to an
/// index word) the polyline `curves[index]` (point count at +4, points at +0x2c, 0x24 bytes each, border flags at +0x20) is
/// rewritten into a new closed order-2 curve (`SI_nurbsCompGetClosedNC(n + 4, 2, 1)` as the scratch, `n` points counted in `cnt`):
/// points with no border bits (0x1e) are copied unless they have bit 1; a point with a border bit is copied (the first such
/// index and the latest one are remembered), then the following run of bit 1 points is skipped and, when the point after it also
/// has a border bit, [`insert_corners`] adds the corners between the two. After the points [`insert_corner_wrap`] decides whether the
/// corners between the last and the first border point are added the same way. The result is another closed curve made for
/// the final count whose +0 and +0x34 words are the old curve's and +0x38 is -1, with the points copied over; the scratch curve and
/// the old one are disposed (`SI_nurbsCrvDispose`, outside) and the new one stored at `curves[index]`.
pub fn linear_trim_crvs_adjust(m: &mut Mem, o: &mut dyn Outside, curves: u32, list: u32, bx: u32) {
    let sp = m.sp();
    let fl = m.local(4);
    let (first, last) = (fl, fl + 2);
    let count = m.local(4);
    let comp_slot = m.local(4);
    let mut e = o.call(m, SI_GEN_LIST_ITER_INIT, &[list, first, last]).eax;
    while m.u16(first) == 0 {
        let dp = elem_data(m, o, e);
        let idx = m.u32(dp);
        let src = m.u32(curves.wrapping_add(idx.wrapping_mul(4)));
        m.set_u32(comp_slot, 0);
        m.set_i32(count, 0);
        let mut first_border = 0u32;
        let mut last_border = 0u32;
        let mut is_first = true;
        let mut i = 0i32;
        let comp = comp_get_closed_nc(m, m.i32(src + 4).wrapping_add(4), 2, 1);
        m.set_u32(comp_slot, comp);
        let n = |m: &Mem| m.i32(src + 4);
        let pt = |m: &Mem, k: i32| m.u32(src + 0x2c).wrapping_add((k as u32).wrapping_mul(0x24));
        let push = |m: &mut Mem, p: u32| {
            let at = m.u32(comp + 0x2c).wrapping_add((m.u32(count)).wrapping_mul(0x24));
            for w in 0..9u32 {
                let v = m.u32(p + 4 * w);
                m.set_u32(at + 4 * w, v);
            }
            m.set_u32(count, m.u32(count).wrapping_add(1));
        };
        if n(m) > 0 {
            'outer: loop {
                if m.u8(pt(m, i) + 0x20) & 0x1e == 0 {
                    loop {
                        if (m.u8(pt(m, i) + 0x20) ^ 1) & 1 != 0 {
                            let p = pt(m, i);
                            push(m, p);
                        }
                        i += 1;
                        if i >= n(m) {
                            break 'outer;
                        }
                        if m.u8(pt(m, i) + 0x20) & 0x1e != 0 {
                            break;
                        }
                    }
                }
                if i >= n(m) {
                    break 'outer;
                }
                if is_first {
                    first_border = i as u32;
                    is_first = false;
                }
                last_border = i as u32;
                let p = pt(m, i);
                push(m, p);
                i += 1;
                if i >= n(m) {
                    break 'outer;
                }
                if m.u8(pt(m, i) + 0x20) & 1 != 0 {
                    loop {
                        i += 1;
                        if i >= n(m) {
                            break 'outer;
                        }
                        if m.u8(pt(m, i) + 0x20) & 1 == 0 {
                            break;
                        }
                    }
                }
                if i >= n(m) {
                    break 'outer;
                }
                if m.u8(pt(m, i) + 0x20) & 0x1e != 0 {
                    let (from, to) = (pt(m, last_border as i32), pt(m, i));
                    insert_corners(m, comp, count, from, to, bx);
                }
                if i >= n(m) {
                    break 'outer;
                }
            }
        }
        if insert_corner_wrap(m, src) as u16 != 0 {
            let (from, to) = (pt(m, last_border as i32), pt(m, first_border as i32));
            insert_corners(m, comp, count, from, to, bx);
        }
        let nc = comp_get_closed_nc(m, m.i32(count), 2, 1);
        let w0 = m.u32(src);
        m.set_u32(nc, w0);
        let w34 = m.u32(src + 0x34);
        m.set_u32(nc + 0x34, w34);
        m.set_u32(nc + 0x38, 0xffff_ffff);
        let mut k = 0i32;
        while k < m.i32(count) {
            let off = (k as u32).wrapping_mul(0x24);
            let (d, s2) = (m.u32(nc + 0x2c).wrapping_add(off), m.u32(comp + 0x2c).wrapping_add(off));
            for w in 0..9u32 {
                let v = m.u32(s2 + 4 * w);
                m.set_u32(d + 4 * w, v);
            }
            k += 1;
        }
        o.call(m, CRV_DISPOSE, &[comp_slot]);
        // the old curve is disposed through its own slot in the table (found by a heap-held table in finding 0044, Trim7)
        o.call(m, CRV_DISPOSE, &[curves.wrapping_add(idx.wrapping_mul(4))]);
        m.set_u32(curves.wrapping_add(idx.wrapping_mul(4)), nc);
        e = o.call(m, SI_GEN_LIST_ITER_NEXT, &[list, first, last]).eax;
    }
    m.set_sp(sp);
}
