// SPDX-License-Identifier: MIT
//! The conversions of `nrb_convert.c` that make a spline from a curve (finding 0025, section 21), on the memory image:
//! `getBezierPoint`, `initNurbsKnotsFromBezier`, `nurbsCrvConvertToLinearBezier` and `SI_nurbsCrvConvertToBezier`.
//! The spline package's two updates (`SI_splineUpdateMatrix`, `SI_splineTouch`) are `si_mem::Outside` calls: that package
//! depends on this crate, so it cannot be called from here.

use crate::bridge::crv_knot_insert;
use crate::image::{at8, crv_copy};
use si_hrc::image::nurbs::{knots_allocate, nurbs_crv_allocate, nurbs_crv_dispose, nurbs_points_allocate};
use si_hrc::image::spline::{spline_allocate, spline_key_allocate};
use si_mem::{Mem, Outside, Routine};
use si_x87::{x, F80};

pub const SI_SPLINE_UPDATE_MATRIX: Routine = Routine::new("SI_splineUpdateMatrix", 0x0848_abd0);
pub const SI_SPLINE_TOUCH: Routine = Routine::new("SI_splineTouch", 0x0848_c924);

/// [soft 0x084e2df8 getBezierPoint]: the index of the control point (`a`, `b`) of a patch (`u16` width at +0xe; the flags at
/// +0x10 and +0x12 say that a direction wraps): with both wrapping the first two columns follow the last (column - 2, plus
/// the width for the first two); with only the second a mirrored column; otherwise `b * width + a`.
pub fn get_bezier_point(m: &Mem, p: u32, a: i32, b: i32) -> i32 {
    let w = m.u16(p + 0xe) as i32;
    if m.i16(p + 0x10) != 0 {
        if m.i16(p + 0x12) == 0 {
            return b.wrapping_mul(w).wrapping_add(a);
        }
        if a > 1 {
            return b.wrapping_mul(w).wrapping_add(a).wrapping_sub(2);
        }
        return b.wrapping_mul(w).wrapping_add(a).wrapping_sub(2).wrapping_add(w);
    }
    if m.i16(p + 0x12) != 0 {
        return w.wrapping_sub(a.wrapping_add(1)).wrapping_add(b.wrapping_mul(w));
    }
    b.wrapping_mul(w).wrapping_add(a)
}

/// [soft 0x084dfaf4 initNurbsKnotsFromBezier]: the knots of a bezier direction. The old block is freed; the count (+8) is
/// order + points - 2 for an open direction (+0x20 zero), else points + 1; the knots are `si_knotsAllocate(count)` filled in threes with the running multiple of 3 / (count - 5)
/// (3 when the count is 5), the sum kept in the register; a closed direction's last knot is that sum.
pub fn init_nurbs_knots_from_bezier(m: &mut Mem, d: u32) {
    let old = m.u32(d + 0xc);
    if old != 0 {
        m.free(old);
        m.set_u32(d + 0xc, 0);
    }
    let n = if m.i16(d + 0x20) == 0 { m.i32(d).wrapping_add(m.i32(d + 4)).wrapping_sub(2) } else { m.i32(d + 4).wrapping_add(1) };
    m.set_i32(d + 8, n);
    let k = knots_allocate(m, n);
    m.set_u32(d + 0xc, k);
    let count = m.i32(d + 8);
    let step = if count == 5 { x(3.0) } else { x(3.0) / (F80::from_i32(count) - x(5.0)) };
    let mut acc = step;
    let mut i: i32 = 0;
    while i < m.i32(d + 8).wrapping_sub(1) {
        let kk = m.u32(d + 0xc);
        let v = acc.to_f64();
        m.set_f64(at8(kk, i), v);
        m.set_f64(at8(m.u32(d + 0xc), i.wrapping_add(1)), v);
        m.set_f64(at8(m.u32(d + 0xc), i.wrapping_add(2)), v);
        acc = acc + step;
        i = i.wrapping_add(3);
    }
    if m.i16(d + 0x20) != 0 {
        let v = acc.to_f64();
        m.set_f64(at8(m.u32(d + 0xc), m.i32(d + 8).wrapping_sub(1)), v);
    }
}

/// A homogeneous point's x/w, y/w, z/w written to three consecutive floats at `at`: x straight from the register, y and z
/// through a double.
fn key_xyz(m: &mut Mem, p: u32, at: u32) {
    let w = x(m.f64(p + 0x18));
    m.set_f32(at, (x(m.f64(p)) / w).to_f32());
    m.set_f32(at + 4, x((x(m.f64(p + 8)) / w).to_f64()).to_f32());
    m.set_f32(at + 8, x((x(m.f64(p + 0x10)) / x(m.f64(p + 0x18))).to_f64()).to_f32());
}

/// [soft 0x084e1028 nurbsCrvConvertToLinearBezier]: a spline (closed flag from the curve's +0x20, type 10, one key per
/// control point) whose keys sit at the points' x/w, y/w, z/w, both tangents on the position (the key's three words
/// at +0, +0x18 copied from +0xc, with the two flag words 2 at +0x28 and 1 at +0x2a); then `SI_splineUpdateMatrix` and
/// `SI_splineTouch`. Answers the spline, 0 when it could not be allocated.
pub fn crv_convert_to_linear_bezier(m: &mut Mem, o: &mut dyn Outside, c: u32) -> u32 {
    let s = spline_allocate(m).addr();
    if s == 0 {
        return 0;
    }
    m.set_u16(s + 8, m.u16(c + 0x20));
    m.set_u16(s + 0x20, 10);
    let n = m.u16(c + 4);
    m.set_u16(s + 0x10, n);
    let keys = spline_key_allocate(m, n as i32);
    m.set_u32(s + 0x14, keys);
    m.set_u32(s + 0xc, 1);
    let mut i: i32 = 0;
    if m.u16(s + 0x10) != 0 {
        let mut kb: u32 = 0;
        let mut pb: u32 = 0;
        loop {
            let p = m.u32(c + 0x2c).wrapping_add(pb);
            let k = m.u32(s + 0x14);
            key_xyz(m, p, k.wrapping_add(kb).wrapping_add(0xc));
            let k = m.u32(s + 0x14).wrapping_add(kb);
            pb = pb.wrapping_add(0x24);
            m.set_u32(k + 0x1c, m.u32(k + 0x10));
            m.set_u32(k + 0x18, m.u32(k + 0xc));
            m.set_u32(k + 0x20, m.u32(k + 0x14));
            m.set_u32(k, m.u32(k + 0xc));
            m.set_u32(k + 4, m.u32(k + 0x1c));
            m.set_u32(k + 8, m.u32(k + 0x20));
            m.set_u16(k + 0x2a, 1);
            m.set_u16(k + 0x28, 2);
            i += 1;
            kb = kb.wrapping_add(0x30);
            if i >= m.u16(s + 0x10) as i32 {
                break;
            }
        }
    }
    o.call(m, SI_SPLINE_UPDATE_MATRIX, &[s]);
    o.call(m, SI_SPLINE_TOUCH, &[s]);
    s
}

/// `(f32[a] + f32[a]) - f32[b]` in the register, stored as a float at `at`.
fn mirror(m: &mut Mem, at: u32, a: u32, b: u32) {
    let (fa, fb) = (x(m.f32(a) as f64), x(m.f32(b) as f64));
    m.set_f32(at, (fa + fa - fb).to_f32());
}

/// [soft 0x084e1148 SI_nurbsCrvConvertToBezier]: a bezier spline of the curve. Order 2 is
/// [`crv_convert_to_linear_bezier`]. Otherwise a copy of the curve gets every one of the curve's knots inserted up to
/// multiplicity order - 1 (`SI_nurbsCrvKnotInsert` with correction), and the spline (closed flag and type word of the copy;
/// points / 3 keys, one more when open) takes its keys from the copy's points in threes: in-tangent, position, out-tangent,
/// as x/w, y/w, z/w; a closed spline's first key is then redone from the point after the last one; an open spline's first
/// and last tangents are mirrored (2 * position - neighbour, in floats, the key type word 2). The copy is disposed.
pub fn crv_convert_to_bezier(m: &mut Mem, o: &mut dyn Outside, c: u32) -> u32 {
    if m.i32(c) == 2 {
        return crv_convert_to_linear_bezier(m, o, c);
    }
    let cp = crv_copy(m, c);
    let local = m.local(4);
    m.set_u32(local, cp);
    let mut i: i32 = 0;
    while i < m.i32(c + 8) {
        let t = m.f64(at8(m.u32(c + 0xc), i));
        crv_knot_insert(m, m.u32(local), t, m.i32(m.u32(local)).wrapping_sub(1), 1);
        i += 1;
    }
    let s = spline_allocate(m).addr();
    if s != 0 {
        let cp = m.u32(local);
        let closed = m.u16(cp + 0x20);
        m.set_u16(s + 8, closed);
        m.set_u16(s + 0x20, m.u16(cp + 0x34));
        let mut cnt = m.i32(cp + 4) / 3;
        if closed == 0 {
            cnt += 1;
        }
        m.set_u16(s + 0x10, cnt as u16);
        let keys = spline_key_allocate(m, (cnt as u16) as i32);
        m.set_u32(s + 0x14, keys);
        m.set_u32(s + 0xc, 1);
        let mut j: i32 = 0;
        let mut pi: i32 = 0;
        if m.u16(s + 0x10) != 0 {
            let (mut pb, mut kb) = (0u32, 0u32);
            loop {
                if j != 0 {
                    let p = m.u32(cp + 0x2c).wrapping_add(pb);
                    let k = m.u32(s + 0x14).wrapping_add(kb);
                    key_xyz(m, p, k);
                    pi += 1;
                    pb = pb.wrapping_add(0x24);
                }
                let p = m.u32(cp + 0x2c).wrapping_add(pb);
                let k = m.u32(s + 0x14).wrapping_add(kb);
                key_xyz(m, p, k + 0xc);
                pi += 1;
                pb = pb.wrapping_add(0x24);
                if m.u16(s + 8) != 0 || j != (m.u16(s + 0x10) as i32).wrapping_sub(1) {
                    let p = m.u32(cp + 0x2c).wrapping_add(pb);
                    let k = m.u32(s + 0x14).wrapping_add(kb);
                    key_xyz(m, p, k + 0x18);
                    pi += 1;
                    pb = pb.wrapping_add(0x24);
                }
                j += 1;
                kb = kb.wrapping_add(0x30);
                if j >= m.u16(s + 0x10) as i32 {
                    break;
                }
            }
        }
        let k0 = m.u32(s + 0x14);
        if m.u16(s + 8) == 0 {
            mirror(m, k0, k0 + 0xc, k0 + 0x18);
            mirror(m, k0 + 4, k0 + 0x10, k0 + 0x1c);
            m.set_u16(k0 + 0x28, 2);
            mirror(m, k0 + 8, k0 + 0x14, k0 + 0x20);
            let l = k0.wrapping_add((m.u16(s + 0x10) as u32).wrapping_mul(0x30)).wrapping_sub(0x30);
            mirror(m, l + 0x18, l + 0xc, l);
            mirror(m, l + 0x1c, l + 0x10, l + 4);
            m.set_u16(l + 0x28, 2);
            mirror(m, l + 0x20, l + 0x14, l + 8);
        } else {
            let p = m.u32(cp + 0x2c).wrapping_add((pi as u32).wrapping_mul(0x24));
            key_xyz(m, p, k0);
        }
        o.call(m, SI_SPLINE_UPDATE_MATRIX, &[s]);
        o.call(m, SI_SPLINE_TOUCH, &[s]);
    }
    nurbs_crv_dispose(m, local);
    m.release(4);
    s
}

pub const SI_SPLINE_ADD_POINT: Routine = Routine::new("SI_splineAddPoint", 0x0848_acc0);
/// `SI_origin` (0x086dc1e0), the vector of zeros the end points are added at.
pub const SI_ORIGIN: u32 = 0x086d_c1e0;

/// [soft 0x084e0cf8 SI_nurbsCrv2SplineKeepCP]: the spline of a curve's control points (a curve with no point answers 0).
/// The spline takes the curve's closed flag (+0x20) and type word (+0x34), one key per point (`SI_splineKeyAllocate`), the
/// step word (+0xc) 0 for an order-2 curve and `kind` otherwise; for `kind` 3 the float at +0x1c is 0.5. A key's position is
/// the point's x/w, y/w, z/w (x from the register, y and z through a double), the point's bit 0 of its flag byte (+0x20)
/// is key flag bit 0 (+0x2c), and both tangents are the position. An open spline of kind 2 or 3 then gets
/// `SI_splineAddPoint` at the origin before the first key and after the last, its +6 set to 1 and `SI_splineTouch`;
/// `SI_splineUpdateMatrix` ends.
pub fn crv_to_spline_keep_cp(m: &mut Mem, o: &mut dyn Outside, c: u32, kind: i32) -> u32 {
    if m.i32(c + 4) < 1 {
        return 0;
    }
    let s = spline_allocate(m).addr();
    if s == 0 {
        return 0;
    }
    m.set_u16(s + 8, m.u16(c + 0x20));
    m.set_u16(s + 0x20, m.u16(c + 0x34));
    let n = m.u16(c + 4);
    m.set_u16(s + 0x10, n);
    let keys = spline_key_allocate(m, n as i32);
    m.set_u32(s + 0x14, keys);
    m.set_u32(s + 0xc, if m.i32(c) == 2 { 0 } else { kind as u32 });
    if kind == 3 {
        m.set_f32(s + 0x1c, 0.5);
    }
    let (mut kb, mut pb) = (0u32, 0u32);
    let mut i: i32 = 0;
    if m.u16(s + 0x10) != 0 {
        loop {
            let p = m.u32(c + 0x2c).wrapping_add(pb);
            key_xyz(m, p, m.u32(s + 0x14).wrapping_add(kb).wrapping_add(0xc));
            if m.u8(m.u32(c + 0x2c).wrapping_add(pb).wrapping_add(0x20)) & 1 != 0 {
                let a = m.u32(s + 0x14).wrapping_add(kb).wrapping_add(0x2c);
                m.set_u8(a, m.u8(a) | 1);
            }
            let k = m.u32(s + 0x14).wrapping_add(kb);
            for j in 0..3u32 {
                m.set_u32(k + 0x18 + 4 * j, m.u32(k + 0xc + 4 * j));
            }
            for j in 0..3u32 {
                m.set_u32(k + 4 * j, m.u32(k + 0x18 + 4 * j));
            }
            kb = kb.wrapping_add(0x30);
            pb = pb.wrapping_add(0x24);
            i += 1;
            if i >= m.u16(s + 0x10) as i32 {
                break;
            }
        }
    }
    if m.u16(s + 8) == 0 && m.u32(s + 0xc).wrapping_sub(2) < 2 {
        o.call(m, SI_SPLINE_ADD_POINT, &[s, 0, SI_ORIGIN]);
        o.call(m, SI_SPLINE_ADD_POINT, &[s, m.u16(s + 0x10) as u32, SI_ORIGIN]);
        m.set_u16(s + 6, 1);
        o.call(m, SI_SPLINE_TOUCH, &[s]);
    }
    o.call(m, SI_SPLINE_UPDATE_MATRIX, &[s]);
    s
}

/// [soft 0x084e0b14 SI_spline2NurbsCrvKeepCP]: the curve of a spline's keys taken as control points (weight 1), then
/// `SI_knotsCrvReparametrization`. Step word (+0xc) 0: order 2, one point per key (a spline with fewer than 2 keys answers
/// 0, the curve already allocated and left); step 0 to 3: order 4, as many points as keys (4 needed), two fewer for an open
/// spline whose +6 flag is set, starting at the second key; any other step word goes straight to the reparametrization.
/// The curve takes the closed flag (+0x20 from +8) and the type word (+0x34 from +0x20); each point is the key position as
/// doubles, +0x18 = 1.0, bit 0 of the key flag (+0x2c) as bit 0 of the point's flag byte (+0x20).
pub fn spline_to_crv_keep_cp(m: &mut Mem, s: u32) -> Result<u32, crate::image::Halt> {
    let c = nurbs_crv_allocate(m).addr();
    if c == 0 {
        return Ok(0);
    }
    let kind = m.i32(s + 0xc);
    let fill = |m: &mut Mem, first: u32| {
        let pts = nurbs_points_allocate(m, m.i32(c + 4));
        m.set_u32(c + 0x2c, pts);
        let (mut kb, mut pb) = (first.wrapping_mul(0x30), 0u32);
        let mut i: i32 = 0;
        while i < m.i32(c + 4) {
            let k = m.u32(s + 0x14).wrapping_add(kb);
            let p = m.u32(c + 0x2c).wrapping_add(pb);
            for j in 0..3u32 {
                m.set_f64(p + 8 * j, m.f32(k + 0xc + 4 * j) as f64);
            }
            let p2 = m.u32(c + 0x2c).wrapping_add(pb);
            m.set_u32(p2 + 0x18, 0);
            m.set_u32(p2 + 0x1c, 0x3ff0_0000);
            if m.u8(k + 0x2c) & 1 != 0 {
                m.set_u8(p2 + 0x20, m.u8(p2 + 0x20) | 1);
            }
            i += 1;
            kb = kb.wrapping_add(0x30);
            pb = pb.wrapping_add(0x24);
        }
    };
    if kind == 0 {
        if m.u16(s + 0x10) < 2 {
            return Ok(0);
        }
        m.set_i32(c, 2);
        let n = m.u16(s + 0x10);
        m.set_i32(c + 4, n as i32);
        m.set_u16(c + 0x20, m.u16(s + 8));
        m.set_i32(c + 0x34, m.u16(s + 0x20) as i32);
        fill(m, 0);
    } else if (0..4).contains(&kind) {
        if m.u16(s + 0x10) < 4 {
            return Ok(0);
        }
        m.set_i32(c, 4);
        let n = m.u16(s + 0x10);
        m.set_i32(c + 4, n as i32);
        let mut first = 0;
        if m.i16(s + 6) != 0 && m.i16(s + 8) == 0 {
            m.set_i32(c + 4, (n as i32).wrapping_sub(2));
            first = 1;
        }
        m.set_u16(c + 0x20, m.u16(s + 8));
        m.set_i32(c + 0x34, m.u16(s + 0x20) as i32);
        fill(m, first);
    }
    crate::image::knots_crv_reparametrization(m, c)?;
    Ok(c)
}

/// [soft 0x084df864 initNurbsPointsVKnotsFromBspline]: the V knots of a surface read as a uniform bspline (its V count
/// is the point count): the knot count (+0x2c) is order - 2 + points (open) or points + 1 (closed), the knots are 0, 1, 2...
/// (the old block freed first), the range (+0x34, +0x3c) taken from knots 2 and (open) count - order + 1, 0 and ... (closed).
/// An open (+0x44 zero) surface then gets that range's two end knots inserted into V (order - 2 times each, so the net
/// is clamped), the span of the range start is found first and the rows from that span on are copied to a new point
/// array of `points * u-count` records (each record's flag word +0x20 kept from the old position), and the knots are
/// cut to the middle ones (count order + points - 2, starting at knot 2).
pub fn init_points_vknots_from_bspline(m: &mut Mem, o: &mut dyn Outside, s: u32) -> Result<(), crate::image::Halt> {
    use crate::image::{free_clear, mov64, point_copy, LEFT};
    use crate::{Dir, NurbsDir, Statics};
    let order = m.i32(s + 0x24);
    let nv = m.i32(s + 0x28);
    let open = m.i16(s + 0x44) == 0;
    let cnt = if open { order.wrapping_sub(2).wrapping_add(nv) } else { nv.wrapping_add(1) };
    m.set_i32(s + 0x2c, cnt);
    let old = m.u32(s + 0x30);
    if old != 0 {
        m.free(old);
        m.set_u32(s + 0x30, 0);
    }
    let k = knots_allocate(m, m.i32(s + 0x2c));
    m.set_u32(s + 0x30, k);
    let mut i: i32 = 0;
    while i < m.i32(s + 0x2c) {
        m.set_f64(at8(m.u32(s + 0x30), i), i as f64);
        i += 1;
    }
    let u2 = open as i32;
    mov64(m, at8(m.u32(s + 0x30), u2.wrapping_mul(2)), s + 0x34);
    mov64(m, at8(m.u32(s + 0x30), 1i32.wrapping_add(u2.wrapping_mul(2).wrapping_add(nv).wrapping_sub(order))), s + 0x3c);
    if open {
        let pts = m.u32(s + 0x54);
        let flags = m.si_calloc(m.i32(s + 0x48) as u32, 4);
        let mut i: i32 = 0;
        while i < m.i32(s + 0x48) {
            m.set_u32(at4(flags, i), m.u32(pts.wrapping_add(0x20).wrapping_add(0x24u32.wrapping_mul(i as u32))));
            i += 1;
        }
        let rec: NurbsDir = m.read(si_mem::Ptr32::new(s + 0x24));
        let kv = crate::fit2::knot_vec(m, &rec);
        let mut st = Statics { left: m.i32(LEFT) };
        let r = crate::knots::interv(&mut st, Dir { rec: &rec, knots: &kv }, m.f64(s + 0x34), 1);
        m.set_i32(LEFT, st.left);
        let iv = r.ok_or(crate::image::Halt::Refused("initNurbsPointsVKnotsFromBspline"))?;
        crate::bridge::srf_knot_insert(m, o, s, m.f64(s + 0x34), order.wrapping_sub(2), 1);
        crate::bridge::srf_knot_insert(m, o, s, m.f64(s + 0x3c), order.wrapping_sub(2), 1);
        m.set_i32(s + 0x28, nv);
        let pts = m.u32(s + 0x54);
        m.set_i32(s + 0x48, nv.wrapping_mul(m.i32(s + 4)));
        let newp = nurbs_points_allocate(m, nv.wrapping_mul(m.i32(s + 4)));
        let mut src = (iv.wrapping_mul(m.i32(s + 4)).wrapping_mul(0x24)) as u32;
        let mut dst: u32 = 0;
        let mut i: i32 = 0;
        while i < m.i32(s + 0x48) {
            point_copy(m, pts.wrapping_add(src), newp.wrapping_add(dst));
            src = src.wrapping_add(0x24);
            m.set_u32(newp.wrapping_add(dst).wrapping_add(0x20), m.u32(at4(flags, i)));
            dst = dst.wrapping_add(0x24);
            i += 1;
        }
        free_clear(m, s + 0x54);
        m.set_u32(s + 0x54, newp);
        if flags != 0 {
            m.free(flags);
        }
        let kk = m.u32(s + 0x30);
        let n2 = order.wrapping_add(m.i32(s + 0x28)).wrapping_sub(2);
        let arr = m.si_calloc(n2 as u32, 8);
        let mut i: i32 = 0;
        while i < n2 {
            mov64(m, at8(kk, i.wrapping_add(2)), at8(arr, i));
            i += 1;
        }
        free_clear(m, s + 0x30);
        m.set_u32(s + 0x30, arr);
        m.set_i32(s + 0x2c, n2);
    }
    Ok(())
}

fn at4(base: u32, i: i32) -> u32 {
    base.wrapping_add(4u32.wrapping_mul(i as u32))
}

/// [soft 0x084e1490 SI_nurbsSrfConvertToLinear] (the temporary blocks and static scratch of `SI_nurbsSrfValue` are reproduced by
/// [`crate::geom8::srf_value_heap`], vec133: 0 different): a surface as a linear (order 2 x 2) net sampled from it. Nothing for a null
/// surface or one without points or knots; a surface already of order 2 x 2 is copied (`SI_nurbsSrfCopy`). Otherwise the knot
/// vectors are sampled (U with the per-span count at +0x70, V with +0x74), a new surface of order 2 x 2 is made whose counts
/// are the sample counts (less one for a direction closed in the *new* record, which `SI_nurbsSrfAllocate` leaves open, so
/// never), the closed flags copied from the old one, +0x70/+0x74 set to 1, and each point (u index fastest) is the surface's
/// value at the sample (x, y, z through f64; the weight stays 1, the flag 0); the samples are freed and
/// `SI_knotsSrfReparametrization` makes the knots.
pub fn srf_convert_to_linear(m: &mut Mem, o: &mut dyn Outside, s: u32) -> Result<u32, crate::image::Halt> {
    use crate::image::{free_some, sampled, srf_copy, Halt};
    if s == 0 || m.i32(s + 4) <= 0 || m.u32(s + 0xc) == 0 || m.i32(s + 0x28) <= 0 || m.u32(s + 0x30) == 0 {
        return Ok(0);
    }
    if m.i32(s + 0x24) == 2 && m.i32(s) == 2 {
        return Ok(srf_copy(m, o, s));
    }
    let (au, nu) = sampled(m, s, m.i32(s + 0x70))?.unwrap_or((0, 0));
    let (av, nv) = sampled(m, s + 0x24, m.i32(s + 0x74))?.unwrap_or((0, 0));
    let r = si_hrc::image::nurbs::nurbs_srf_allocate(m).addr();
    if r != 0 {
        m.set_i32(r, 2);
        m.set_i32(r + 0x24, 2);
        m.set_i32(r + 4, if m.i16(r + 0x20) != 0 { nu.wrapping_sub(1) } else { nu });
        m.set_i32(r + 0x28, if m.i16(r + 0x44) != 0 { nv.wrapping_sub(1) } else { nv });
        m.set_i32(r + 0x48, m.i32(r + 4).wrapping_mul(m.i32(r + 0x28)));
        m.set_u16(r + 0x20, m.u16(s + 0x20));
        m.set_u16(r + 0x44, m.u16(s + 0x44));
        m.set_i32(r + 0x70, 1);
        m.set_i32(r + 0x74, 1);
        let pts = nurbs_points_allocate(m, m.i32(r + 0x48));
        m.set_u32(r + 0x54, pts);
        let mut j: i32 = 0;
        while j < m.i32(r + 0x28) {
            let mut i: i32 = 0;
            while i < m.i32(r + 4) {
                let (u, v) = (m.f64(at8(au, i)), m.f64(at8(av, j)));
                let val = crate::geom8::srf_value_heap(m, s, u, v);
                let val = val.ok_or(Halt::Refused("SI_nurbsSrfValue"))?;
                let p = m.u32(r + 0x54).wrapping_add((0x24i32.wrapping_mul(i.wrapping_add(m.i32(r + 4).wrapping_mul(j)))) as u32);
                for (k, w) in val.iter().enumerate() {
                    m.set_f64(p + 8 * k as u32, si_x87::fmov64(*w));
                }
                m.set_u32(p + 0x20, 0);
                i += 1;
            }
            j += 1;
        }
    }
    free_some(m, au);
    free_some(m, av);
    if r != 0 {
        crate::geom3::knots_srf_reparametrization(m, o, r);
    }
    Ok(r)
}

pub const SI_SPLINE_CONVERT_TO_BEZIER: Routine = Routine::new("SI_splineConvertToBezier", 0x0848_ee64);

/// One control point from three floats at `src` (x, y, z to doubles), weight 1.0 (+0x18/+0x1c written as the two words), bit 0 of
/// the key's flag byte (`key + 0x2c`) or-ed into the point's flag byte (+0x20).
fn key_point(m: &mut Mem, p: u32, src: u32, key: u32) {
    for j in 0..3u32 {
        m.set_f64(p + 8 * j, m.f32(src + 4 * j) as f64);
    }
    m.set_u32(p + 0x18, 0);
    m.set_u32(p + 0x1c, 0x3ff0_0000);
    if m.u8(key + 0x2c) & 1 != 0 {
        m.set_u8(p + 0x20, m.u8(p + 0x20) | 1);
    }
}

/// [soft 0x084e049c SI_spline2NurbsCrvKeepCurve]: a spline as a curve that keeps its shape. Step word (+0xc) 0: as
/// `SI_spline2NurbsCrvKeepCP` (order 2, the keys as points). 2: order 4, the keys as points (4 needed); the knots are
/// 0, 1, 2 ... (count order + points - 2, or points + 1 closed), the range from knot 2 and knot points - 1 (open); an open
/// curve then has the range ends inserted twice each (`SI_nurbsCrvKnotInsert`, 2 times, the span of the start found
/// first), its points taken from that span on (flags kept from the old position) and its knots cut to points + 2 starting at
/// knot 2. 1 and 3 (3 first through `SI_splineConvertToBezier`, which may answer null: then nothing, 0): a bezier curve of order 4 with
/// 3 points per key (less 2 when open: the first key has no in tangent, the last no out tangent), in tangent, position,
/// out tangent each with weight 1 and the key's flag bit, the knots in threes of 3 / (count - 5) with the sum in the
/// register (a closed curve starts with two zero knots); a closed curve of step 1 or 3 then has its points rotated one
/// back (the first goes last, flag word kept) and `SI_shiftDirKnotsClosed(curve, 1)`. All end in `SI_knotsCrvReparametrization`.
/// Another step word goes straight to it. A spline with too few keys answers 0 with the curve left allocated.
pub fn spline_to_crv_keep_curve(m: &mut Mem, o: &mut dyn Outside, s0: u32) -> Result<u32, crate::image::Halt> {
    use crate::image::{free_clear, mov64, point_copy, LEFT};
    use crate::{Dir, NurbsDir, Statics};
    let c = nurbs_crv_allocate(m).addr();
    if c == 0 {
        return Ok(0);
    }
    let mut s = s0;
    let kind = m.i32(s + 0xc);
    match kind {
        0 | 2 => {
            let need = if kind == 0 { 2 } else { 4 };
            if (m.u16(s + 0x10) as u32) < need {
                return Ok(0);
            }
            m.set_i32(c, if kind == 0 { 2 } else { 4 });
            let n = m.u16(s + 0x10) as i32;
            m.set_i32(c + 4, n);
            m.set_u16(c + 0x20, m.u16(s + 8));
            m.set_i32(c + 0x34, m.u16(s + 0x20) as i32);
            let pts = nurbs_points_allocate(m, n);
            m.set_u32(c + 0x2c, pts);
            let mut i: i32 = 0;
            while i < m.i32(c + 4) {
                let k = m.u32(s + 0x14).wrapping_add(0x30u32.wrapping_mul(i as u32));
                let p = m.u32(c + 0x2c).wrapping_add(0x24u32.wrapping_mul(i as u32));
                key_point(m, p, k + 0xc, k);
                i += 1;
            }
            if kind == 2 {
                let cnt = if m.i16(c + 0x20) == 0 { m.i32(c).wrapping_add(n).wrapping_sub(2) } else { n.wrapping_add(1) };
                m.set_i32(c + 8, cnt);
                let k = knots_allocate(m, cnt);
                m.set_u32(c + 0xc, k);
                let mut i: i32 = 0;
                while i < m.i32(c + 8) {
                    m.set_f64(at8(m.u32(c + 0xc), i), i as f64);
                    i += 1;
                }
                let open = m.i16(c + 0x20) == 0;
                let u2 = if open { 2 } else { 0 };
                mov64(m, at8(m.u32(c + 0xc), u2), c + 0x10);
                mov64(m, at8(m.u32(c + 0xc), u2.wrapping_add(n).wrapping_sub(3)), c + 0x18);
                if open {
                    let oldp = m.u32(c + 0x2c);
                    let flags = m.si_calloc(n as u32, 4);
                    for i in 0..n {
                        m.set_u32(at4(flags, i), m.u32(oldp.wrapping_add(0x20).wrapping_add(0x24u32.wrapping_mul(i as u32))));
                    }
                    let rec: NurbsDir = m.read(si_mem::Ptr32::new(c));
                    let kv = crate::fit2::knot_vec(m, &rec);
                    let mut st = Statics { left: m.i32(LEFT) };
                    let r = crate::knots::interv(&mut st, Dir { rec: &rec, knots: &kv }, m.f64(c + 0x10), 1);
                    m.set_i32(LEFT, st.left);
                    let iv = r.ok_or(crate::image::Halt::Refused("SI_spline2NurbsCrvKeepCurve"))?;
                    crv_knot_insert(m, c, m.f64(c + 0x10), 2, 1);
                    crv_knot_insert(m, c, m.f64(c + 0x18), 2, 1);
                    let oldp = m.u32(c + 0x2c);
                    m.set_i32(c + 4, n);
                    let newp = nurbs_points_allocate(m, n);
                    let mut src = (iv.wrapping_mul(0x24)) as u32;
                    let mut dst: u32 = 0;
                    let mut i: i32 = 0;
                    while i < m.i32(c + 4) {
                        point_copy(m, oldp.wrapping_add(src), newp.wrapping_add(dst));
                        m.set_u32(newp.wrapping_add(dst).wrapping_add(0x20), m.u32(at4(flags, i)));
                        src = src.wrapping_add(0x24);
                        dst = dst.wrapping_add(0x24);
                        i += 1;
                    }
                    free_clear(m, c + 0x2c);
                    m.set_u32(c + 0x2c, newp);
                    if flags != 0 {
                        m.free(flags);
                    }
                    let kk = m.u32(c + 0xc);
                    let n2 = m.i32(c + 4).wrapping_add(2);
                    let arr = m.si_calloc(n2 as u32, 8);
                    for i in 0..n2 {
                        mov64(m, at8(kk, i.wrapping_add(2)), at8(arr, i));
                    }
                    free_clear(m, c + 0xc);
                    m.set_u32(c + 0xc, arr);
                    m.set_i32(c + 8, n2);
                }
            }
        }
        1 | 3 => {
            if kind == 3 {
                s = o.call(m, SI_SPLINE_CONVERT_TO_BEZIER, &[s]).eax;
                if s == 0 {
                    return Ok(0);
                }
            }
            if m.u16(s + 0x10) <= 1 {
                return Ok(0);
            }
            let keys = m.u16(s + 0x10) as i32;
            let closed = m.u16(s + 8) != 0;
            m.set_i32(c, 4);
            let mut n = keys.wrapping_mul(3);
            if !closed {
                n = n.wrapping_sub(2);
            }
            m.set_i32(c + 4, n);
            m.set_u16(c + 0x20, m.u16(s + 8));
            m.set_i32(c + 0x34, m.u16(s + 0x20) as i32);
            m.set_i32(c + 0x24, 2);
            let cnt = if closed { n.wrapping_add(1) } else { n.wrapping_add(2) };
            m.set_i32(c + 8, cnt);
            let k = knots_allocate(m, cnt);
            m.set_u32(c + 0xc, k);
            // 3 / (count - 5): the division stays in the register
            let step = x(3.0) / (F80::from_i32(m.i32(c + 8)) - x(5.0));
            let mut i: i32 = if m.i16(c + 0x20) != 0 {
                m.set_u32(k, 0);
                m.set_u32(k + 4, 0);
                1
            } else {
                0
            };
            let mut acc = step;
            while i < m.i32(c + 8).wrapping_sub(1) {
                let kk = m.u32(c + 0xc);
                let v = acc.to_f64();
                m.set_f64(at8(kk, i), v);
                m.set_f64(at8(m.u32(c + 0xc), i.wrapping_add(1)), v);
                m.set_f64(at8(m.u32(c + 0xc), i.wrapping_add(2)), v);
                acc = acc + step;
                i = i.wrapping_add(3);
            }
            let pts = nurbs_points_allocate(m, m.i32(c + 4));
            m.set_u32(c + 0x2c, pts);
            let (mut p, mut kb) = (pts, 0u32);
            let mut i: i32 = 0;
            while i < m.u16(s + 0x10) as i32 {
                let key = m.u32(s + 0x14).wrapping_add(kb);
                if closed || i != 0 {
                    key_point(m, p, key, key);
                    p = p.wrapping_add(0x24);
                }
                key_point(m, p, key + 0xc, key);
                p = p.wrapping_add(0x24);
                if closed || i != m.u16(s + 0x10) as i32 - 1 {
                    key_point(m, p, key + 0x18, key);
                    p = p.wrapping_add(0x24);
                }
                kb = kb.wrapping_add(0x30);
                i += 1;
            }
            let ks = m.i32(s + 0xc);
            if closed && (ks == 1 || ks == 3) {
                let pts = m.u32(c + 0x2c);
                let mut first = [0u32; 9];
                for (j, w) in first.iter_mut().enumerate() {
                    *w = m.u32(pts + 4 * j as u32);
                }
                let flag0 = m.u32(pts + 0x20);
                let mut pb: u32 = 0;
                let mut i: i32 = 0;
                while i < m.i32(c + 4).wrapping_sub(1) {
                    point_copy(m, pts.wrapping_add(pb).wrapping_add(0x24), pts.wrapping_add(pb));
                    m.set_u32(pts.wrapping_add(pb).wrapping_add(0x20), m.u32(pts.wrapping_add(pb).wrapping_add(0x44)));
                    pb = pb.wrapping_add(0x24);
                    i += 1;
                }
                let last = pts.wrapping_add((m.i32(c + 4) as u32).wrapping_mul(0x24)).wrapping_sub(0x24);
                for (j, w) in first.iter().enumerate() {
                    m.set_u32(last + 4 * j as u32, *w);
                }
                m.set_u32(last + 0x20, flag0);
                crate::geom3::shift_dir_knots_closed(m, c, 1);
            }
        }
        _ => {}
    }
    crate::image::knots_crv_reparametrization(m, c)?;
    Ok(c)
}
