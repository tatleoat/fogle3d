// SPDX-License-Identifier: MIT
//! invdynamic.c of the dynamics simulation: the forces of fans, winds and constraints on a body (finding 0035).

use crate::dynamic::si_vector_clean;
use crate::outside::DynaCalls;
use si_mem::{Mem, Outside};
use si_x87::{x, F80};

/// The fan/wind model record and its amplitude (globals `wMdl` and `wAmpl`).
const W_MDL: u32 = 0x0882_2010;
const W_AMPL: u32 = 0x0882_2014;

fn fl(m: &Mem, a: u32) -> F80 {
    x(m.f32(a))
}

/// Which sides of the fan's box the point `(vx, vy, vz)` (the float words) of the body `model` lies outside, and the
/// strength of the fan there [soft 0x08499540 invdynaCheckPtFanEffect]. The point is moved to the fan's frame: for a
/// model of kind 2 by its own 4 x 4 matrix (+0x178, translation +0x1a8 and +0x1b8 .. +0x1c0), for kind 12 by the
/// matrix of the parent at +0x2bc when that is set (same sum), or, with no parent, by subtracting the translation
/// of the fan model `wMdl` and turning the result with its matrix; any other kind leaves the three coordinates
/// untouched stack words. In the fan's frame (x along the axis): bit 0 is set when x <= 0, bits 1/2 when y is below
/// -|limit| / above |limit| (`wMdl+0x108`), bits 3/4 the same for z. The word written to `out` is 0 when
/// |`wMdl+0x100`| <= x or a bit is set, else `wAmpl - x * wAmpl / |wMdl+0x100|`. Returns the bits.
pub fn invdyna_check_pt_fan_effect(m: &mut Mem, model: u32, vx: u32, vy: u32, vz: u32, out: u32) -> i32 {
    let w = m.u32(W_MDL);
    let ampl = fl(m, W_AMPL);
    let (a1, a2, a3) = (x(f32::from_bits(vx)), x(f32::from_bits(vy)), x(f32::from_bits(vz)));
    let fr = m.local(0x28);
    // the three result slots ebp-0xc, ebp-8, ebp-4 (stack words nobody wrote when the kind is neither 2 nor 12)
    let (sc, s8, s4) = (fr + 0x1c, fr + 0x20, fr + 0x24);
    let kind = m.i32(model + 0x2ec);
    let base = if kind == 2 {
        Some(model)
    } else if kind == 12 {
        let b = m.u32(model + 0x2bc);
        if b == 0 { None } else { Some(b) }
    } else {
        // no transform: the slots stay as they are
        return finish(m, w, ampl, out, sc, s8, s4);
    };
    match base {
        None => {
            let t1 = a1 - fl(m, w + 0x1b8);
            let t2 = a2 - fl(m, w + 0x1bc);
            let t3 = a3 - fl(m, w + 0x1c0);
            m.set_f32(sc, t1.to_f32());
            m.set_f32(s8, t2.to_f32());
            m.set_f32(s4, t3.to_f32());
            let c = ((fl(m, w + 0x178) * t1) + (fl(m, w + 0x17c) * t2)) + (fl(m, w + 0x180) * t3);
            m.set_f32(sc, c.to_f32());
            let e = ((fl(m, w + 0x188) * t1) + (fl(m, w + 0x18c) * t2)) + (fl(m, w + 0x190) * t3);
            m.set_f32(s8, e.to_f32());
            let p4 = ((t2 * fl(m, w + 0x19c)) + (t1 * fl(m, w + 0x198))) + (t3 * fl(m, w + 0x1a0));
            m.set_f32(s4, p4.to_f32());
        }
        Some(b) => {
            let phi3 = ((((a3 * fl(m, b + 0x198)) + ((a2 * fl(m, b + 0x188)) + (a1 * fl(m, b + 0x178)))) + fl(m, b + 0x1a8))) + fl(m, b + 0x1b8);
            let phi1 = ((a3 * fl(m, b + 0x19c)) + ((a2 * fl(m, b + 0x18c)) + (a1 * fl(m, b + 0x17c)))) + fl(m, b + 0x1ac);
            let phi2 = ((a3 * fl(m, b + 0x1a0)) + ((a2 * fl(m, b + 0x190)) + (a1 * fl(m, b + 0x180)))) + fl(m, b + 0x1b0);
            let t7 = phi1 + fl(m, b + 0x1bc);
            let t8 = phi2 + fl(m, b + 0x1c0);
            let t9 = phi3 - fl(m, w + 0x1b8);
            let t10 = t7 - fl(m, w + 0x1bc);
            let t11 = t8 - fl(m, w + 0x1c0);
            let c = ((fl(m, w + 0x178) * t9) + (fl(m, w + 0x17c) * t10)) + (fl(m, w + 0x180) * t11);
            m.set_f32(sc, c.to_f32());
            let e = ((fl(m, w + 0x188) * t9) + (fl(m, w + 0x18c) * t10)) + (fl(m, w + 0x190) * t11);
            m.set_f32(s8, e.to_f32());
            let p4 = (t11 * fl(m, w + 0x1a0)) + ((t10 * fl(m, w + 0x19c)) + (t9 * fl(m, w + 0x198)));
            m.set_f32(s4, p4.to_f32());
        }
    }
    finish(m, w, ampl, out, sc, s8, s4)
}

fn finish(m: &mut Mem, w: u32, ampl: F80, out: u32, sc: u32, s8: u32, s4: u32) -> i32 {
    let (lc, l8, l4) = (m.f32(sc), m.f32(s8), m.f32(s4));
    let lim = m.f32(w + 0x108).abs();
    let mut bits = 0;
    if lc <= 0.0 {
        bits |= 1;
    }
    if l8 < -lim {
        bits |= 2;
    }
    if lim < l8 {
        bits |= 4;
    }
    if l4 < -lim {
        bits |= 8;
    }
    if lim < l4 {
        bits |= 0x10;
    }
    let d = m.f32(w + 0x100).abs();
    m.release(0x28);
    if d <= lc || bits != 0 {
        m.set_f32(out, 0.0);
    } else {
        let q = (x(lc) * ampl) / x(d);
        m.set_f32(out, (ampl - q).to_f32());
    }
    bits
}

fn r32(v: F80) -> F80 {
    x(v.to_f32())
}

/// The force of the fan on the quadrilateral `v` (four points, three float words each) [soft 0x084998b0
/// invdynaGetBboxFaceFanFrce]: the four corners are tested with [`invdyna_check_pt_fan_effect`]. When no corner is
/// in the fan's way, or `depth` has reached the limit `(i16)[[model+0x304]+0x5c]`, or the fan's limit `|wMdl+0x108|`
/// is above the size `thr`, the result is leaf-like: the sum of the four strengths (zero or up to 1e-4 gives the
/// centre of the quadrilateral as the point), else the point is the strength-weighted mean of the corners; `o2` is
/// `thr`, `o3` a quarter of the strength sum. When every corner is out on the same side everything is zero. Else the
/// quadrilateral is cut into four at the midpoints and its centre (size a quarter of `thr`, depth + 1) and the four
/// results are averaged (sums for the second output, quarters for the point and the third).
pub fn invdyna_get_bbox_face_fan_frce(m: &mut Mem, model: u32, v: [u32; 12], thr: u32, out: u32, o2: u32, o3: u32, depth: i32) {
    let w = m.u32(W_MDL);
    let c25 = x(m.f64(0x0866_09c0));
    let c5 = x(m.f64(0x0866_09d0));
    let tiny = m.f64(0x0866_09c8);
    let frame = 0x108u32;
    let fr = m.local(frame);
    let ebp = fr + frame;
    let l = |k: u32| ebp - k;
    let fv = |i: usize| x(f32::from_bits(v[i]));
    let ld = |m: &Mem, k: u32| x(m.f32(ebp - k));
    let xs = ((fv(0) + fv(3)) + fv(6)) + fv(9);
    let ys = ((fv(1) + fv(4)) + fv(7)) + fv(10);
    let zs = ((fv(2) + fv(5)) + fv(8)) + fv(11);
    m.set_f32(l(0x18), (c25 * xs).to_f32());
    m.set_f32(l(0x10), (c25 * zs).to_f32());
    m.set_f32(l(0x14), (c25 * ys).to_f32());
    let u0 = invdyna_check_pt_fan_effect(m, model, v[0], v[1], v[2], l(0x98));
    let u1 = invdyna_check_pt_fan_effect(m, model, v[3], v[4], v[5], l(0x9c));
    let u2 = invdyna_check_pt_fan_effect(m, model, v[6], v[7], v[8], l(0xa0));
    let u3 = invdyna_check_pt_fan_effect(m, model, v[9], v[10], v[11], l(0xa4));
    let all = u0 & u1 & u2 & u3;
    let thr_f = f32::from_bits(thr);
    let mut recurse = false;
    if u0 != 0 || u1 != 0 || u2 != 0 || u3 != 0 {
        let lim = m.i16(m.u32(model + 0x304) + 0x5c) as i32;
        if depth < lim {
            // taken when |wMdl+0x108| <= size or unordered
            recurse = !(m.f32(w + 0x108).abs() > thr_f);
        }
    }
    if recurse {
        if all != 0 {
            for k in 0..3 {
                m.set_f32(out + 4 * k, 0.0);
            }
            m.set_f32(o2, 0.0);
            m.set_f32(o3, 0.0);
            m.release(frame);
            return;
        }
        // midpoints of the four edges (and the centre, L_18 / L_14 / L_10)
        m.set_f32(l(0x24), ((fv(3) + fv(0)) * c5).to_f32());
        m.set_f32(l(0x20), ((fv(4) + fv(1)) * c5).to_f32());
        m.set_f32(l(0x1c), ((fv(5) + fv(2)) * c5).to_f32());
        let e4 = r32(fv(3) + fv(6));
        let e8 = r32(fv(4) + fv(7));
        m.set_f32(l(0x30), (c5 * e4).to_f32());
        m.set_f32(l(0x2c), (c5 * e8).to_f32());
        m.set_f32(l(0x28), ((fv(5) + fv(8)) * c5).to_f32());
        let f0 = r32(fv(8) + fv(11));
        m.set_f32(l(0x3c), ((fv(6) + fv(9)) * c5).to_f32());
        m.set_f32(l(0x38), ((fv(7) + fv(10)) * c5).to_f32());
        m.set_f32(l(0x34), (c5 * f0).to_f32());
        m.set_f32(l(0x48), (c5 * (fv(9) + fv(0))).to_f32());
        m.set_f32(l(0x44), (c5 * (fv(10) + fv(1))).to_f32());
        m.set_f32(l(0x40), (c5 * (fv(11) + fv(2))).to_f32());
        for k in [0x4c, 0x50, 0x54, 0x58, 0x5c, 0x60, 0x64, 0x68, 0x6c, 0x70, 0x74, 0x78, 0xb4, 0xb0, 0xac, 0xa8, 0xa4, 0xa0, 0x9c, 0x98] {
            m.set_u32(l(k), 0);
        }
        let f4 = (x(thr_f) * c25).to_f32().to_bits();
        let wd = |m: &Mem, k: u32| m.u32(ebp - k);
        let ctr = [wd(m, 0x18), wd(m, 0x14), wd(m, 0x10)];
        let m01 = [wd(m, 0x24), wd(m, 0x20), wd(m, 0x1c)];
        let m12 = [wd(m, 0x30), wd(m, 0x2c), wd(m, 0x28)];
        let m23 = [wd(m, 0x3c), wd(m, 0x38), wd(m, 0x34)];
        let m30 = [wd(m, 0x48), wd(m, 0x44), wd(m, 0x40)];
        let (p0, p1, p2, p3) = ([v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]], [v[9], v[10], v[11]]);
        let cat = |a: [u32; 3], b: [u32; 3], c: [u32; 3], d: [u32; 3]| [a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2], d[0], d[1], d[2]];
        let d1 = depth + 1;
        invdyna_get_bbox_face_fan_frce(m, model, cat(p0, m01, ctr, m30), f4, l(0x54), l(0xa8), l(0x98), d1);
        invdyna_get_bbox_face_fan_frce(m, model, cat(m01, p1, m12, ctr), f4, l(0x60), l(0xac), l(0x9c), d1);
        invdyna_get_bbox_face_fan_frce(m, model, cat(ctr, m12, p2, m23), f4, l(0x6c), l(0xb0), l(0xa0), d1);
        invdyna_get_bbox_face_fan_frce(m, model, cat(m30, ctr, m23, p3), f4, l(0x78), l(0xb4), l(0xa4), d1);
        let s2 = ((ld(m, 0xa8) + ld(m, 0xac)) + ld(m, 0xb0)) + ld(m, 0xb4);
        m.set_f32(o2, s2.to_f32());
        let s3 = (((ld(m, 0x98) + ld(m, 0x9c)) + ld(m, 0xa0)) + ld(m, 0xa4)) * c25;
        m.set_f32(o3, s3.to_f32());
        let sx = ((ld(m, 0x54) + ld(m, 0x60)) + ld(m, 0x6c)) + ld(m, 0x78);
        let sy = ((ld(m, 0x50) + ld(m, 0x5c)) + ld(m, 0x68)) + ld(m, 0x74);
        let sz = ((ld(m, 0x4c) + ld(m, 0x58)) + ld(m, 0x64)) + ld(m, 0x70);
        m.set_f32(out, (c25 * sx).to_f32());
        m.set_f32(out + 4, (sy * c25).to_f32());
        m.set_f32(out + 8, (sz * c25).to_f32());
        m.release(frame);
        return;
    }
    // the leaf: strengths a0..a3 of the four corners
    let (a0, a1, a2, a3) = (ld(m, 0x98), ld(m, 0x9c), ld(m, 0xa0), ld(m, 0xa4));
    let b8 = r32(a0 + a1);
    let t1 = (b8 + a2) + a3;
    let sum_f = t1.to_f32();
    let d0 = t1.to_f64();
    if d0 <= tiny || d0.is_nan() {
        for (k, c) in [0x18u32, 0x14, 0x10].iter().enumerate() {
            let c = x(m.f32(l(*c))).to_f32();
            m.set_f32(out + 4 * k as u32, c);
        }
    } else {
        let t2 = a1 * fv(3);
        let t3 = a0 * fv(2);
        let t4 = a3 * fv(11);
        let t5 = a0 * fv(0);
        let t6 = a1 * fv(5);
        let t7 = a1 * fv(4);
        let t8 = a2 * fv(8);
        let t9 = a0 * fv(1);
        let t10 = a2 * fv(6);
        let t11 = a3 * fv(10);
        let t12 = a3 * fv(9);
        let t13 = a2 * fv(7);
        let t14 = ((t5 + r32(t2)) + t10) + t12;
    let t16 = t11 + (t13 + (t9 + r32(t7)));
        let t17 = t4 + ((t3 + r32(t6)) + r32(t8));
        let t15 = F80::from_f64(1.0) / F80::from_f64(d0);
        m.set_f32(out, (t14 * t15).to_f32());
        m.set_f32(out + 8, (t17 * t15).to_f32());
        m.set_f32(out + 4, (t16 * t15).to_f32());
    }
    m.set_f32(o2, x(thr_f).to_f32());
    m.set_f32(o3, (c25 * x(sum_f)).to_f32());
    m.release(frame);
}

/// The force a constraint (`info`) exerts on the body `mdl2` [soft 0x0849cf54 si_invdynaGetConstrFrce]: does nothing
/// without both, or when the constraint record's (`info+0x10`) +0x170 is not above 0. The constraint's stiffness
/// share (`+0x16c` clamped to 0..1) times the body's mass slot (`[mdl2+0x2c4]+0x158`) times `mdl2+0x16c` is stored as
/// the weight vector `info+0x20` (x only; y and z 0, the whole vector 0 without `mdl2+0x2c4`); the weight is turned
/// by the body's matrix (`[mdl2+0x300]+0x1d8`) and the position and speed errors (position from
/// `si_constraintPositionGet`, speed from `si_dynaLinearSpeedCompute`, minus the constraint's own speed words at
/// `[+0x2f0]+0x30..0x44`) make a spring-damper force in `out`, from which the body's `+0x180` is subtracted and
/// the `0x260` chain of children (kind 5 with a record kind 2) subtracts theirs; then the vector is cleaned.
pub fn si_invdyna_get_constr_frce(m: &mut Mem, o: &mut dyn Outside, mdl1: u32, mdl2: u32, info: u32, out: u32) {
    if mdl2 == 0 || info == 0 {
        return;
    }
    let cp = m.u32(info + 0x10);
    let f = |m: &Mem, a: u32| x(m.f32(a));
    if m.f32(cp + 0x170) <= 0.0 {
        return;
    }
    m.set_u32(info + 0x28, 0);
    m.set_u32(info + 0x24, 0);
    m.set_u32(info + 0x20, 0);
    if m.u32(mdl2 + 0x2c4) != 0 {
        let v = m.f32(cp + 0x16c);
        let ph = if 0.0 > v {
            x(0.0f32)
        } else if v > 1.0 {
            x(1.0f32)
        } else {
            x(v)
        };
        let ph = ph;
        let w = (ph * f(m, m.u32(mdl2 + 0x2c4) + 0x158)) * f(m, mdl2 + 0x16c);
        m.set_u32(info + 0x24, 0);
        m.set_u32(info + 0x28, 0);
        m.set_f32(info + 0x20, w.to_f32());
    }
    let fr = m.local(0xb8);
    let ebp = fr + 0xb8;
    let l = |k: u32| ebp - k;
    for (k, a) in [(0xcu32, 0x20u32), (8, 0x24), (4, 0x28)] {
        let w = m.u32(info + a);
        m.set_u32(l(k), w);
    }
    o.si_constraint_position_get(m, cp, mdl1, 1, l(0x18));
    let q = m.u32(cp + 0x2f0);
    if q != 0 {
        for (k, a) in [(0x24u32, 0x30u32), (0x20, 0x34), (0x1c, 0x38), (0x30, 0x3c), (0x2c, 0x40), (0x28, 0x44)] {
            let w = m.u32(q + a);
            m.set_u32(l(k), w);
        }
    } else {
        for k in [0x1cu32, 0x20, 0x24, 0x28, 0x2c, 0x30] {
            m.set_u32(l(k), 0);
        }
    }
    let mut l9c = m.f32(m.u32(mdl2 + 0x2f4) + 8);
    if mdl2 == mdl1 {
        l9c = m.f32(m.u32(mdl2 + 0x304) + 0x60);
    }
    m.set_f32(l(0x9c), l9c);
    let a170 = m.f32(cp + 0x170).abs();
    let phi3 = if x(a170).le(F80::from_f64(m.f64(0x0866_0ab0))) || a170.is_nan() { x(m.f32(0x0866_0ab8)) } else { x(a170) };
    m.set_f32(l(0x98), (x(m.f64(0x0866_0ac0)) / phi3).to_f32());
    let b = m.u32(mdl2 + 0x300);
    let (c, e8, e4) = (f(m, l(0xc)), f(m, l(8)), f(m, l(4)));
    let s25 = ((f(m, b + 0x1dc) * e8) + (f(m, b + 0x1d8) * c)) + (f(m, b + 0x1e0) * e4);
    m.set_f32(l(0x48), s25.to_f32());
    let s26 = ((f(m, b + 0x1e4) * c) + (f(m, b + 0x1e8) * e8)) + (f(m, b + 0x1ec) * e4);
    m.set_f32(l(0x44), s26.to_f32());
    let s27 = ((e8 * f(m, b + 0x1f4)) + (c * f(m, b + 0x1f0))) + (e4 * f(m, b + 0x1f8));
    m.set_f32(l(0x40), s27.to_f32());
    let (vx, vy, vz) = (m.f32(l(0x48)), m.f32(l(0x44)), m.f32(l(0x40)));
    crate::settings::si_dyna_linear_speed_compute(m, mdl2, vx, vy, vz, l(0x3c));
    let t1 = f(m, l(0x48)) + f(m, b + 0x244);
    let t3 = f(m, l(0x44)) + f(m, b + 0x248);
    let t2 = f(m, l(0x3c)) - f(m, l(0x24));
    m.set_f32(l(0xa0), t2.to_f32());
    m.set_f32(l(0x6c), t2.to_f32());
    let t6 = t1 - f(m, l(0x18));
    let t4 = f(m, l(0x38)) - f(m, l(0x20));
    m.set_f32(l(0xa4), t4.to_f32());
    m.set_f32(l(0x68), t4.to_f32());
    let t5 = f(m, l(0x34)) - f(m, l(0x1c));
    let t7 = f(m, l(0x40)) + f(m, b + 0x24c);
    let t8 = t3 - f(m, l(0x14));
    let t9 = t7 - f(m, l(0x10));
    m.set_f32(l(0x7c), ((t5 * t5) + ((t4 * t4) + (t2 * t2))).sqrt().to_f32());
    let s43 = (f(m, l(0x7c)) * x(m.f64(0x0866_0ac8))) + f(m, l(0x98));
    m.set_f32(l(0x7c), s43.to_f32());
    let g9c = f(m, l(0x9c));
    let s44 = (-g9c) / (f(m, l(0x7c)) * f(m, l(0x7c)));
    let s43v = f(m, l(0x7c));
    m.set_f32(l(0x7c), s44.to_f32());
    let s45 = (g9c * x(m.f64(0x0866_0ad0))) / s43v;
    let k44 = f(m, l(0x7c));
    m.set_f32(l(0x7c), s45.to_f32());
    let k45 = f(m, l(0x7c));
    let t10 = t6 * k44;
    let t11 = t8 * k44;
    let t12 = k44 * t9;
    let t13 = f(m, l(0xa0)) * k45;
    let t14 = f(m, l(0xa4)) * k45;
    let t15 = k45 * t5;
    let t16 = x(0.0f32) * (-g9c);
    let t17 = (t13 + t10) + t16;
    let t18 = (t14 + t11) + t16;
    let t19 = (t15 + t12) + t16;
    m.set_f32(out, (t17 - f(m, b + 0x180)).to_f32());
    m.set_f32(out + 4, (t18 - f(m, b + 0x184)).to_f32());
    m.set_f32(out + 8, (t19 - f(m, b + 0x188)).to_f32());
    m.release(0xb8);
    let mut it = if b != 0 { m.u32(b + 0x260) } else { 0 };
    while it != 0 {
        if m.i32(it + 0x30) == 5 && m.i32(it + 0x2ec) == 2 {
            let c = m.u32(it + 0x300);
            for (k, a) in [(0u32, 0xf0u32), (4, 0xf4), (8, 0xf8)] {
                let v = f(m, out + k) - f(m, c + a);
                m.set_f32(out + k, v.to_f32());
            }
        }
        let c = m.u32(it + 0x300);
        it = if c != 0 { m.u32(c + 0x264) } else { 0 };
    }
    si_vector_clean(m, out, 1);
}
