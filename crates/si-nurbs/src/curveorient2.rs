// SPDX-License-Identifier: MIT
//! `curveOrient.c`, the rest (finding 0044, section "Trim8"): the orientation of a loop of intersections by a test point
//! (`Orient`), the search for the test points (`findLocForOrientTest`, `findNeighborInPlane`, `findTestPoints`,
//! `PointInCurve`), the correction for crossings of the surface boundary (`checkBoundaryCross`, `setCorrection`,
//! `findUvCriticalInters`) and `MarkBCross`, on the memory image. Records as in [`crate::curveorient`].

use crate::curveorient::{dv3, find_point_in_curve, point_relative_to_plane, project_crv_samples, reverse_curve, unit_dir};
use crate::inter::{self, Fl};
use crate::trim::{compute_line_inter, ray_cast, RayCastArgs, CRV_VALUE, STATUS_MESSAGE};
use si_mem::{Mem, Outside};
use si_x87::F80;

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

/// Six words (three doubles) at `at`, copied as the original does with `rep movs`.
fn w6(m: &Mem, at: u32) -> [u32; 6] {
    let mut w = [0u32; 6];
    for (i, v) in w.iter_mut().enumerate() {
        *v = m.u32(at + 4 * i as u32);
    }
    w
}

fn bits3(v: [f64; 3]) -> [u32; 6] {
    let mut w = [0u32; 6];
    for k in 0..3 {
        let b = v[k].to_bits();
        w[2 * k] = b as u32;
        w[2 * k + 1] = (b >> 32) as u32;
    }
    w
}

/// A stored point (u, v, 0) as the original writes it: two doubles through the x87 stack and two zero words.
fn put_uv(m: &mut Mem, at: u32, u: f64, v: f64) {
    m.set_f64(at, si_x87::fmov64(u));
    m.set_f64(at + 8, si_x87::fmov64(v));
    m.set_u32(at + 0x10, 0);
    m.set_u32(at + 0x14, 0);
}

/// [soft 0x0850bd64 findTestPoints]: `(trim, inter, p, out1, out2, out3)`: casts a ray at the point `p` (from the view point when the trim's
/// word at +0x3c is 1, else along the trim's direction), and of the intersections it finds takes the one whose (u, v) is nearest
/// (strictly under 3.4028234663852886e38, squared distance in 80 bits) to the intersection `inter`'s; where one is found writes the
/// (u, v, 0) of `inter`, of its successor and of the nearest to `out1`, `out2`, `out3` and answers 1, else 0.
pub fn find_test_points(m: &mut Mem, o: &mut dyn Outside, trim: u32, inter: u32, p: [f64; 3], out1: u32, out2: u32, out3: u32) -> u16 {
    let sp = m.sp();
    let status = m.local(4);
    let cell = m.local(4);
    let mut best = 3.4028234663852886e38f64; // 0x08661ff0
    let mut best_inter = 0u32;
    let mut found = 0u16;
    let (dir, org);
    if m.u32(trim + 0x3c) == 1 {
        let vp = dv3(m, trim + 0x44);
        dir = bits3([(x(p[0]) - x(vp[0])).to_f64(), (x(p[1]) - x(vp[1])).to_f64(), (x(p[2]) - x(vp[2])).to_f64()]);
        org = w6(m, trim + 0x44);
    } else {
        dir = w6(m, trim + 0x5c);
        org = bits3(p);
    }
    let args = RayCastArgs { origin: org, dir, t: 0.0, end: bits3(p), status, perturb: 1 };
    let ray = ray_cast(m, o, trim, &args);
    m.set_u32(cell, ray);
    if ray == 0 {
        m.set_sp(sp);
        return 0;
    }
    let list = inter::ray_get_inters(m, ray);
    let fl = Fl::new(m);
    let mut e = fl.init(m, list);
    while m.i16(fl.0 + 2) != 1 {
        let d = crate::inter::genlist::elem_get_data(m, e);
        let dist = inter::inter_dist_2d(m.f64(inter + 0x30), m.f64(inter + 0x38), m.f64(d + 0x30), m.f64(d + 0x38));
        if dist.lt(x(best)) {
            best = dist.to_f64();
            best_inter = d;
            found = 1;
        }
        e = fl.next(m, list);
    }
    if found != 0 {
        put_uv(m, out1, m.f64(inter + 0x30), m.f64(inter + 0x38));
        let nx = m.u32(inter + 0x74);
        put_uv(m, out2, m.f64(nx + 0x30), m.f64(nx + 0x38));
        put_uv(m, out3, m.f64(best_inter + 0x30), m.f64(best_inter + 0x38));
    }
    inter::ray_destroy(m, o, cell);
    m.set_sp(sp);
    found
}

/// [soft 0x0850bf64 si_nurbsCurveOrientPointInCurve]: `(pts, n, p, q, dv)`: whether the point `p` lies inside the closed
/// polygon of the `n` 24-byte points `pts` as seen along the line `p`..`q`: the plane through `p` normal to the unit vector from
/// `p` to `q`, a second plane through `p` normal to that and `dv`, and the number of polygon edges the line crosses (an edge
/// that touches the second plane is followed back to a vertex that does not; edges crossing the first plane are tested with
/// [`compute_line_inter`]), odd = 1. Writes nothing.
pub fn point_in_curve(m: &mut Mem, o: &mut dyn Outside, pts: u32, n: i32, p: [f64; 3], q: [f64; 3], dv: [f64; 3]) -> u16 {
    let _ = o;
    let eps = x(1e-6); // 0x08662000
    let n1 = unit_dir(q, p, true);
    let t4 = -((x(n1[2]) * x(p[2])) + F80::from_f64((x(n1[1]) * x(p[1]) + F80::from_f64((x(n1[0]) * x(p[0])).to_f64())).to_f64()));
    let d1 = if t4.abs().lt(eps) { 0.0 } else { t4.to_f64() };
    let t5 = x(dv[1]) * x(n1[2]) - x(dv[2]) * x(n1[1]);
    let t7 = x(n1[1]) * x(dv[0]) - x(dv[1]) * x(n1[0]);
    let t6 = x(n1[0]) * x(dv[2]) - x(n1[2]) * x(dv[0]);
    let sum = (t7 * t7) + ((t5 * t5) + (t6 * t6));
    if sum.lt(eps) {
        return 0;
    }
    let len = F80::from_f32(sum.sqrt().to_f32());
    let n2 = if x(1e-4).lt(len.abs()) {
        [(t5 / len).to_f64(), (t6 / len).to_f64(), (t7 / len).to_f64()]
    } else {
        [t5.to_f64(), t6.to_f64(), t7.to_f64()]
    };
    let t8 = -((x(n2[2]) * x(p[2])) + F80::from_f64((x(n2[1]) * x(p[1]) + F80::from_f64((x(n2[0]) * x(p[0])).to_f64())).to_f64()));
    let d2 = if t8.abs().lt(eps) { 0.0 } else { t8.to_f64() };
    let mut cnt = 0i32;
    let mut i = 0i32;
    if n <= 0 {
        return 0;
    }
    let sp = m.sp();
    let fr = m.local(0x28);
    let zero = x(0.0);
    let big = |v: F80| eps.lt(v.abs());
    let gt0 = |v: F80| zero.lt(v);
    let lt0 = |v: F80| v.lt(zero);
    let rel1 = |m: &Mem, k: i32| point_relative_to_plane(n1, d1, dv3(m, pts.wrapping_add(24u32.wrapping_mul(k as u32))));
    let rel2 = |m: &Mem, k: i32| point_relative_to_plane(n2, d2, dv3(m, pts.wrapping_add(24u32.wrapping_mul(k as u32))));
    loop {
        let j = i.wrapping_add(1).wrapping_rem(n);
        let a = rel1(m, i);
        let b = rel1(m, j);
        let skip = big(a) && lt0(a) && big(b) && lt0(b);
        if !skip {
            let b3 = big(a) && gt0(a) && big(b) && gt0(b);
            let a2 = rel2(m, i);
            let b2 = rel2(m, j);
            let mut toggle = false;
            let mut line = false;
            if !eps.gt(a2.abs()) {
                if gt0(a2) && b3 && big(b2) && lt0(b2) {
                    toggle = true;
                } else if lt0(a2) && b3 && big(b2) && gt0(b2) {
                    toggle = true;
                } else if big(b2) && ((lt0(a2) && gt0(b2)) || (gt0(a2) && lt0(b2))) {
                    line = true;
                }
            } else {
                let mut c0 = 1i32;
                let mut k = i - 1;
                if i == 0 {
                    k = n - 1;
                }
                let mut r = rel2(m, k);
                while c0 < n && eps.gt(r.abs()) {
                    c0 += 1;
                    k -= 1;
                    if k < 0 {
                        k = n - 1;
                    }
                    r = rel2(m, k);
                }
                if gt0(r) && big(r) && b3 && big(b2) && lt0(b2) {
                    toggle = true;
                } else if lt0(r) && big(r) && b3 && big(b2) && gt0(b2) {
                    toggle = true;
                } else if big(b2) && ((gt0(b2) && lt0(r)) || (lt0(b2) && gt0(r))) {
                    line = true;
                }
            }
            if line {
                let pt = fr;
                let (s, t) = (fr + 0x18, fr + 0x20);
                let r = compute_line_inter(
                    m,
                    w6(m, pts.wrapping_add(24u32.wrapping_mul(i as u32))),
                    w6(m, pts.wrapping_add(24u32.wrapping_mul(j as u32))),
                    bits3(p),
                    bits3(q),
                    pt,
                    s,
                    t,
                );
                if r & 0xffff != 0 {
                    let (sv, tv) = (x(m.f64(s)), x(m.f64(t)));
                    if zero.lt(sv) && x(1.0).gt(sv) && zero.lt(tv) {
                        toggle = true;
                    }
                }
            }
            if toggle {
                cnt += 1;
            }
        }
        i = i.wrapping_add(1);
        if !(i < n) {
            break;
        }
    }
    m.set_sp(sp);
    (cnt & 1) as u16
}

/// [soft 0x0850b70c findNeighborInPlane]: `(dir, p, pts, n, idx, op, out)`: `out` = the point of `pts[idx]` moved by 0.001 along the cross
/// product of `dir` and the unit vector from `pts[idx]` to `pts[idx + 1]` (answers 0 where that is shorter than 1e-3 squared);
/// where [`point_in_curve`] of it is 1 and `op` is 0, or 0 and `op` is 1 or 2, moves it to the other side instead (cross product the
/// other way round) and asks again (answer ignored); answers 1.
pub fn find_neighbor_in_plane(m: &mut Mem, o: &mut dyn Outside, dir: [f64; 3], p: [f64; 3], pts: u32, n: i32, idx: i32, op: i32, out: u32) -> u16 {
    let a = pts.wrapping_add(24u32.wrapping_mul(idx as u32));
    let cur = dv3(m, a);
    let nxt = dv3(m, a.wrapping_add(24));
    let v = unit_dir(nxt, cur, false);
    let eps = x(1e-6); // 0x08661fc8
    let k = x(0.001); // 0x08661fd0
    let t4 = x(dir[1]) * x(v[2]) - x(dir[2]) * x(v[1]);
    let t6 = x(dir[0]) * x(v[1]) - x(v[0]) * x(dir[1]);
    let t5 = x(dir[2]) * x(v[0]) - x(v[2]) * x(dir[0]);
    if ((t6 * t6) + ((t4 * t4) + (t5 * t5))).lt(eps) {
        return 0;
    }
    m.set_f64(out, (t4 * k + x(cur[0])).to_f64());
    m.set_f64(out + 8, (t5 * k + x(cur[1])).to_f64());
    m.set_f64(out + 16, (t6 * k + x(cur[2])).to_f64());
    let r1 = point_in_curve(m, o, pts, n, dv3(m, out), p, dir);
    let second = if r1 != 0 { op == 0 } else { (op.wrapping_sub(1) as u32) <= 1 };
    if second {
        let t10 = x(v[1]) * x(dir[2]) - x(v[2]) * x(dir[1]);
        let t12 = x(v[0]) * x(dir[1]) - x(dir[0]) * x(v[1]);
        let t11 = x(v[2]) * x(dir[0]) - x(dir[2]) * x(v[0]);
        if ((t12 * t12) + ((t10 * t10) + (t11 * t11))).lt(eps) {
            return 0;
        }
        m.set_f64(out, (t10 * k + x(cur[0])).to_f64());
        m.set_f64(out + 8, (t11 * k + x(cur[1])).to_f64());
        m.set_f64(out + 16, (t12 * k + x(cur[2])).to_f64());
        point_in_curve(m, o, pts, n, dv3(m, out), p, dir);
    }
    1
}

/// [soft 0x0850b45c findLocForOrientTest]: `(trim, co, out_a, out_b, out_c)`: takes an intersection of the loop `co` ([`find_point_in_curve`]),
/// projects the trim's curve samples ([`project_crv_samples`]), builds a point a little off the projected curve next to the
/// intersection ([`find_neighbor_in_plane`], from the ray's origin and the trim's direction, or from the curve's first point
/// and the unit vector from the view point when the trim's word at +0x3c is not 0) and looks for the nearest intersection
/// there ([`find_test_points`]): 1 with `out_a` = the intersection's (u, v, 0), `out_c` = its successor's, `out_b` = the nearest's; else 0.
pub fn find_loc_for_orient_test(m: &mut Mem, o: &mut dyn Outside, trim: u32, co: u32, out_a: u32, out_b: u32, out_c: u32) -> u16 {
    let sp = m.sp();
    let fr = m.local(0x20);
    let (pts_cell, idx_cell, l18) = (fr, fr + 4, fr + 8);
    let inter = find_point_in_curve(m, co, trim);
    let r;
    if inter == 0 {
        r = 0;
    } else if project_crv_samples(m, trim, co, inter, pts_cell, idx_cell) != 1 {
        r = 0;
    } else {
        let (dir, org);
        if m.u32(trim + 0x3c) == 0 {
            dir = dv3(m, trim + 0x5c);
            org = dv3(m, m.u32(inter + 0x40));
        } else {
            let crv = m.u32(trim + 0x74);
            let p0 = m.u32(crv + 0x2c);
            let a = [si_x87::fmov64(m.f64(p0)), si_x87::fmov64(m.f64(p0 + 8)), si_x87::fmov64(m.f64(p0 + 16))];
            org = a;
            dir = unit_dir(a, dv3(m, trim + 0x44), false);
        }
        let crv = m.u32(trim + 0x74);
        let n = m.i32(crv + 4);
        let ok = find_neighbor_in_plane(m, o, dir, org, m.u32(pts_cell), n, m.i32(idx_cell), m.i32(trim + 0x38), l18);
        if ok != 0 && find_test_points(m, o, trim, inter, dv3(m, l18), out_a, out_c, out_b) != 0 {
            r = 1;
        } else {
            r = 0;
        }
        let a = m.u32(pts_cell);
        if a != 0 {
            m.free(a);
            m.set_u32(pts_cell, 0);
        }
    }
    m.set_sp(sp);
    r
}

/// [soft 0x0850aabc checkBoundaryCross]: `(inter, a, inter2, b, c, d, flag, trim)` with the registers the original leaves unset
/// (`esi`, `edi` at entry; `setCorrection` has both equal to its `flag` word) given: whether the correction `a`/`b` against
/// `c`/`d` is a real crossing of the surface boundary: see [`check_boundary_cross`].
#[allow(clippy::too_many_arguments)]
pub fn check_boundary_cross_regs(m: &mut Mem, o: &mut dyn Outside, p1: u32, a: f64, p3: u32, b: f64, c: f64, d: f64, flag: i16, trim: u32, esi0: u32, edi0: u32) -> u16 {
    let eps = x(1e-6); // 0x08661fb0
    let ray_of = |m: &Mem, i: u32| if i == 0 { 0 } else { m.u32(i + 0x40) };
    let prev = m.u32(p1 + 0x70);
    let a_ray = ray_of(m, prev);
    let r1 = m.u32(p1 + 0x40);
    let r3 = m.u32(p3 + 0x40);
    let nx = m.u32(p3 + 0x74);
    let b_ray = ray_of(m, nx);
    if r1 == r3 {
        return 1;
    }
    let (p_ray, q_ray, la, mut esi, mut rb, mut edi);
    if a_ray == 0 && b_ray != 0 {
        la = r1;
        q_ray = r3;
        p_ray = b_ray;
        edi = m.u32(nx + 0x74);
        rb = m.u32(edi + 0x40);
        esi = esi0;
    } else if a_ray != 0 && b_ray == 0 {
        let pp = m.u32(p1 + 0x70);
        esi = m.u32(pp + 0x70);
        rb = r3;
        p_ray = r1;
        q_ray = a_ray;
        la = m.u32(esi + 0x40);
        edi = edi0;
    } else {
        la = a_ray;
        esi = p1;
        edi = p3;
        q_ray = r1;
        p_ray = r3;
        rb = b_ray;
    }
    let pv = x(m.f64(p_ray + 0x30));
    let qv = x(m.f64(q_ray + 0x30));
    let mut e4 = 0i32;
    if pv.gt(qv) {
        e4 = 1;
    }
    let mut la = la;
    if la != 0 {
        if esi != 0 {
            loop {
                if !(x(m.f64(la + 0x30)) - qv).abs().lt(eps) {
                    break;
                }
                esi = m.u32(esi + 0x70);
                la = m.u32(esi + 0x40);
                if la == 0 || esi == 0 {
                    break;
                }
            }
        }
        if la != 0 && qv.gt(x(m.f64(la + 0x30))) {
            e4 += 1;
        }
    }
    if rb != 0 {
        if edi != 0 {
            loop {
                if !(pv - x(m.f64(rb + 0x30))).abs().lt(eps) {
                    break;
                }
                edi = m.u32(edi + 0x74);
                rb = m.u32(edi + 0x40);
                if rb == 0 || edi == 0 {
                    break;
                }
            }
        }
        if rb != 0 && pv.lt(x(m.f64(rb + 0x30))) {
            e4 += 1;
        }
    }
    let crv = m.u32(trim + 0xc);
    let (c18, c10) = (m.f64(crv + 0x18), m.f64(crv + 0x10));
    let (t1, t3) = (m.f64(r1 + 0x30), m.f64(r3 + 0x30));
    let tt = if e4 > 1 { inter::ray_binary_split_rays(t3, t1, c18, c10) } else { inter::ray_binary_split_rays(t1, t3, c18, c10) }.to_f64();
    let sp = m.sp();
    let val = m.local(24);
    let status = m.local(4);
    let cell = m.local(4);
    let (tlo, thi) = (tt.to_bits() as u32, (tt.to_bits() >> 32) as u32);
    let (dir, org, end);
    if m.u32(trim + 0x3c) == 1 {
        let vp = dv3(m, trim + 0x44);
        let mut dd = [0f64; 3];
        let mut pt = [0u32; 6];
        for k in 0..3usize {
            o.call(m, CRV_VALUE, &[val, m.u32(trim + 0xc), tlo, thi, 0]);
            pt = w6(m, val);
            let comp = f64::from_bits(pt[2 * k] as u64 | (pt[2 * k + 1] as u64) << 32);
            dd[k] = (x(comp) - x(vp[k])).to_f64();
        }
        dir = bits3(dd);
        org = w6(m, trim + 0x44);
        end = pt;
    } else {
        let d0 = w6(m, trim + 0x5c);
        o.call(m, CRV_VALUE, &[val, m.u32(trim + 0xc), tlo, thi, 0]);
        let pt = w6(m, val);
        dir = d0;
        org = pt;
        end = pt;
    }
    let args = RayCastArgs { origin: org, dir, t: tt, end, status, perturb: 1 };
    let ray = ray_cast(m, o, trim, &args);
    m.set_u32(cell, ray);
    if ray == 0 {
        m.set_sp(sp);
        return 0;
    }
    let (sel, rr) = if m.u32(ray + 0x50) == m.u32(r1 + 0x50) { (p1, r1) } else { (p3, r3) };
    let l1 = inter::ray_get_inters(m, rr);
    let l2 = inter::ray_get_inters(m, ray);
    let (f1, f2) = (Fl::new(m), Fl::new(m));
    let mut e1 = f1.init(m, l1);
    let mut e2 = f2.init(m, l2);
    while crate::inter::genlist::elem_get_data(m, e1) != sel {
        if e1 == 0 {
            inter::ray_destroy(m, o, cell);
            m.set_sp(sp);
            return 0;
        }
        e1 = f1.next(m, l1);
        e2 = f2.next(m, l2);
    }
    let dd = crate::inter::genlist::elem_get_data(m, e2);
    if dd == 0 {
        inter::ray_destroy(m, o, cell);
        m.set_sp(sp);
        return 0;
    }
    let val_c = if flag == 0 { x(m.f64(dd + 0x38)) } else { x(m.f64(dd + 0x30)) };
    let (ra, rc1, rc3) = (m.u32(ray + 0x50), m.u32(r1 + 0x50), m.u32(r3 + 0x50));
    let cmp1 = |sub: f64, other: f64, base: f64| (x(sub) - x(base)).abs().gt((val_c - x(base)).abs()) && {
        let _ = other;
        true
    };
    let first = (x(a) - x(c)).abs().gt((val_c - x(c)).abs());
    let second = (x(b) - x(d)).abs().gt((val_c - x(d)).abs());
    let _ = cmp1;
    let res = if ra == rc1 {
        if ra != rc3 {
            first
        } else if (val_c - x(c)).abs().lt((x(a) - x(c)).abs()) {
            true
        } else {
            second
        }
    } else {
        second
    };
    inter::ray_destroy(m, o, cell);
    m.set_sp(sp);
    res as u16
}

/// [soft 0x0850aabc checkBoundaryCross] as `setCorrection` calls it (`esi` = `edi` = the flag word).
pub fn check_boundary_cross(m: &mut Mem, o: &mut dyn Outside, p1: u32, a: f64, p3: u32, b: f64, c: f64, d: f64, flag: i16, trim: u32) -> u16 {
    check_boundary_cross_regs(m, o, p1, a, p3, b, c, d, flag, trim, flag as u16 as u32, flag as u16 as u32)
}

/// [soft 0x0850a958 setCorrection]: `(inter, a, inter2, b, c, d, flag, trim, out)`: where `a` is nearer `b` than `c` and `b` nearer `d` than
/// `a` (or `a` nearer `d` than `b` and `b` nearer `c` than `a`) and [`check_boundary_cross`] agrees: `*out` moves by -(d - c)
/// (or +(d - c)) and the intersection's flags (+0x50) get 0x410 / 0x840 (or 0x420 / 0x880), for `flag` set / clear.
pub fn set_correction(m: &mut Mem, o: &mut dyn Outside, inter: u32, a: f64, inter2: u32, b: f64, c: f64, d: f64, flag: i16, trim: u32, out: u32) {
    let (xa, xb, xc, xd) = (x(a), x(b), x(c), x(d));
    let ab = (xa - xb).abs();
    if ab.gt((xa - xc).abs()) && (xb - xd).abs().lt(ab) {
        if check_boundary_cross(m, o, inter, a, inter2, b, c, d, flag, trim) != 0 {
            m.set_f64(out, (x(m.f64(out)) - (xd - xc)).to_f64());
            let f = m.u32(inter + 0x50);
            m.set_u32(inter + 0x50, f | if flag != 0 { 0x410 } else { 0x840 });
        }
        return;
    }
    if (xb - xa).abs().gt((xa - xd).abs()) && (xb - xc).abs().lt(ab) {
        if check_boundary_cross(m, o, inter, a, inter2, b, c, d, flag, trim) != 0 {
            m.set_f64(out, ((xd - xc) + x(m.f64(out))).to_f64());
            let f = m.u32(inter + 0x50);
            m.set_u32(inter + 0x50, f | if flag != 0 { 0x420 } else { 0x880 });
        }
    }
}

/// [soft 0x0850a764 findUvCriticalInters]: `(co, trim, p3, p4, p5, p6)`: walks the loop's intersections adding the running corrections
/// (`accU`, `accV`, doubles) to (u, v) at +0x54 / +0x5c, records the one with the least u in `*p3`, the greatest u in `*p4`, the
/// least v in `*p5`, the greatest v in `*p6` (against +-3.4028234663852886e38), and lets [`set_correction`] adjust the sums at
/// the surface's U / V wraps (surface words at +0x20 / +0x44, ends +0x10 /+0x18 and +0x34 /+0x3c); a marked intersection
/// (flags byte +0x51 & 0xc) marks the loop (byte +0x25 |= 0xc). Answers 1 where a sum is over 1e-6 or the loop is marked in both
/// directions.
#[allow(clippy::too_many_arguments)]
pub fn find_uv_critical_inters(m: &mut Mem, o: &mut dyn Outside, co: u32, trim: u32, p3: u32, p4: u32, p5: u32, p6: u32) -> u16 {
    let sp = m.sp();
    let fr = m.local(16);
    let (acc_u, acc_v) = (fr, fr + 8);
    let flt = 3.4028234663852886e38f64; // 0x08661f98 / 0x08661fa0
    let (mut min_u, mut max_u, mut min_v, mut max_v) = (flt, -flt, flt, -flt);
    m.set_f64(acc_u, 0.0);
    m.set_f64(acc_v, 0.0);
    let mut e = m.u32(co);
    while e != 0 {
        let t1 = x(m.f64(e + 0x30)) + x(m.f64(acc_u));
        m.set_f64(e + 0x54, t1.to_f64());
        m.set_f64(e + 0x5c, (x(m.f64(e + 0x38)) + x(m.f64(acc_v))).to_f64());
        if t1.lt(x(min_u)) {
            min_u = t1.to_f64();
            m.set_u32(p3, e);
        }
        if x(m.f64(e + 0x54)).gt(x(max_u)) {
            max_u = m.f64(e + 0x54);
            m.set_u32(p4, e);
        }
        if x(m.f64(e + 0x5c)).lt(x(min_v)) {
            min_v = m.f64(e + 0x5c);
            m.set_u32(p5, e);
        }
        if x(m.f64(e + 0x5c)).gt(x(max_v)) {
            max_v = m.f64(e + 0x5c);
            m.set_u32(p6, e);
        }
        let srf = m.u32(trim);
        if m.u16(srf + 0x20) != 0 && m.u32(e + 0x74) != 0 {
            let nx = m.u32(e + 0x74);
            set_correction(m, o, e, m.f64(e + 0x30), nx, m.f64(nx + 0x30), m.f64(srf + 0x10), m.f64(srf + 0x18), 1, trim, acc_u);
            if m.u8(e + 0x51) & 0xc != 0 {
                m.set_u8(co + 0x25, m.u8(co + 0x25) | 0xc);
            }
        }
        let srf = m.u32(trim);
        if m.u16(srf + 0x44) != 0 && m.u32(e + 0x74) != 0 {
            let nx = m.u32(e + 0x74);
            set_correction(m, o, e, m.f64(e + 0x38), nx, m.f64(nx + 0x38), m.f64(srf + 0x34), m.f64(srf + 0x3c), 0, trim, acc_v);
            if m.u8(e + 0x51) & 0xc != 0 {
                m.set_u8(co + 0x25, m.u8(co + 0x25) | 0xc);
            }
        }
        e = m.u32(e + 0x74);
        if e == m.u32(co) && m.u16(co + 0xc) != 0 {
            break;
        }
    }
    let eps = x(1e-6); // 0x08661fa8
    let srf = m.u32(trim);
    let r = eps.lt(x(m.f64(acc_u)).abs())
        || x(m.f64(acc_v)).abs().gt(eps)
        || (m.u16(srf + 0x20) != 0 && m.u16(srf + 0x44) != 0 && m.u8(co + 0x25) & 0xc != 0);
    m.set_sp(sp);
    r as u16
}

/// [soft 0x0850b0c0 findNonLinearInter]: `(i1, i2, i3, i4)`: the first of the four intersections whose (u, v) at +0x54 / +0x5c is not on
/// the line through its neighbours' ([`crate::curveinter::curve_inter_point_on_line`], previous, next, itself) -- and only while
/// `i1`'s flags byte +0x51 has no bit 0xc -- else 0.
pub fn find_non_linear_inter(m: &Mem, i1: u32, i2: u32, i3: u32, i4: u32) -> u32 {
    let uv = |e: u32| [si_x87::fmov64(m.f64(e + 0x54)), si_x87::fmov64(m.f64(e + 0x5c)), 0.0];
    for c in [i1, i2, i3, i4] {
        let (sel, nx, pv) = (uv(c), uv(m.u32(c + 0x74)), uv(m.u32(c + 0x70)));
        let mut ok = false;
        if crate::curveinter::curve_inter_point_on_line(pv, nx, sel) & 0xffff == 0 {
            ok = m.u8(i1 + 0x51) & 0xc == 0;
        }
        if ok {
            return c;
        }
    }
    0
}

/// [soft 0x0850a424 si_nurbsCurveOrientOrient]: `(co, trim)`: orients the loop of intersections `co` against the trim: finds
/// its extreme intersections ([`find_uv_critical_inters`]), for a closed loop with no crossing a non-collinear intersection
/// ([`find_non_linear_inter`]) else a test point ([`find_loc_for_orient_test`], the status message "OR: Unable to determine
/// proper curve orientation." and 0 where that fails); the sign of the z of the cross product of the unit vectors from the point to
/// its successor and to its predecessor decides whether the loop is reversed ([`reverse_curve`]). Answers `co` (0 for a null
/// `co` or `trim` or a loop whose byte +0x24 has bit 1).
pub fn orient(m: &mut Mem, o: &mut dyn Outside, co: u32, trim: u32) -> u32 {
    if co == 0 || trim == 0 || m.u8(co + 0x24) & 1 != 0 {
        return 0;
    }
    let sp = m.sp();
    let fr = m.local(0x48);
    let (l78, l60, l48) = (fr, fr + 0x18, fr + 0x30);
    let a2 = m.u32(trim + 0x38) == 0;
    let mut di = find_uv_critical_inters(m, o, co, trim, co + 0x10, co + 0x14, co + 0x18, co + 0x1c);
    let closed = m.u16(co + 0xc) != 0;
    let mut esi = 0;
    if closed && di == 0 {
        esi = find_non_linear_inter(m, m.u32(co + 0x10), m.u32(co + 0x14), m.u32(co + 0x18), m.u32(co + 0x1c));
    }
    if esi != 0 {
        put_uv(m, l60, m.f64(esi + 0x54), m.f64(esi + 0x5c));
        let nx = m.u32(esi + 0x74);
        put_uv(m, l48, m.f64(nx + 0x54), m.f64(nx + 0x5c));
        let pv = m.u32(esi + 0x70);
        put_uv(m, l78, m.f64(pv + 0x54), m.f64(pv + 0x5c));
    } else {
        di = 1;
    }
    if !(closed && di == 0) && find_loc_for_orient_test(m, o, trim, co, l60, l78, l48) != 1 {
        o.call(m, STATUS_MESSAGE, &[0x086d_b5c0, 2]);
        m.set_sp(sp);
        return 0;
    }
    let p = dv3(m, l60);
    let nx = dv3(m, l48);
    let c = dv3(m, l78);
    let t1 = x(nx[0]) - x(p[0]);
    let t2 = x(nx[1]) - x(p[1]);
    let t3 = x(nx[2]) - x(p[2]);
    let mut v = [t1.to_f64(), t2.to_f64(), t3.to_f64()];
    let wv = [(x(c[0]) - x(p[0])).to_f64(), (x(c[1]) - x(p[1])).to_f64(), (x(c[2]) - x(p[2])).to_f64()];
    let eps = x(1e-4); // 0x08661f90
    let len = F80::from_f32(((t3 * t3) + ((t1 * t1) + (t2 * t2))).sqrt().to_f32());
    if eps.lt(len.abs()) {
        v = [(t1 / len).to_f64(), (t2 / len).to_f64(), (t3 / len).to_f64()];
    }
    let mut w = wv;
    let len2 = F80::from_f32((((x(w[1]) * x(w[1])) + (x(w[0]) * x(w[0]))) + (x(w[2]) * x(w[2]))).sqrt().to_f32());
    if len2.abs().gt(eps) {
        w = [(x(w[0]) / len2).to_f64(), (x(w[1]) / len2).to_f64(), (x(w[2]) / len2).to_f64()];
    }
    let t4 = x(w[1]) * x(v[0]) - x(w[0]) * x(v[1]);
    let l80 = t4.to_f64();
    let zero = x(0.0);
    if closed && di == 0 {
        if (zero.lt(t4) && a2) || (zero.gt(x(l80)) && !a2) {
            reverse_curve(m, co);
        }
    } else if zero.gt(x(l80)) {
        reverse_curve(m, co);
    }
    m.set_sp(sp);
    co
}

/// [soft 0x0850a720 si_nurbsCurveOrientMarkBCross]: `(co, trim)`: unless either is null or the loop's byte +0x24 has bit 1,
/// runs [`find_uv_critical_inters`] on the loop (its four extreme pointers at +0x10 .. +0x1c) and answers `co`; else 0.
pub fn mark_b_cross(m: &mut Mem, o: &mut dyn Outside, co: u32, trim: u32) -> u32 {
    if co == 0 || trim == 0 || m.u8(co + 0x24) & 1 != 0 {
        return 0;
    }
    find_uv_critical_inters(m, o, co, trim, co + 0x10, co + 0x14, co + 0x18, co + 0x1c);
    co
}
