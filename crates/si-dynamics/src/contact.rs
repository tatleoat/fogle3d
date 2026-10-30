// SPDX-License-Identifier: MIT
//! contact.c of the dynamics simulation: clipping of a contact line against a surface of contact (finding 0035).

use si_mem::Mem;
use si_x87::{x, F80};

/// Clips the line `line` (origin at +0, direction at +0xc, six floats) against the polygon of the surface of contact
/// `soc` [soft 0x084a8e10 linePolygonClipping]: the points are `soc+8` (12-byte records, count at +0), the edge
/// normals are written to the array `soc+0xc`, facing the same side as the third point after each edge, from the
/// plane normal at `soc+0x10..0x18`. Walks the edges for the largest entering and the smallest leaving parameter
/// (tmin in 0..1 from 0, tmax down from 1); returns 0 when the line misses (a parallel line outside an edge, or
/// tmin > tmax), else 1 with `*out_start` / `*out_end` set to 1 when the line starts / ends inside, and the two
/// clipped end points in `p1` / `p2`. The block run when the polygon has two points normalises vectors but leaves
/// nothing in memory; it is not reproduced.
pub fn line_polygon_clipping(m: &mut Mem, line: u32, soc: u32, out_start: u32, out_end: u32, p1: u32, p2: u32) -> i32 {
    let mut tmin = 0.0f32;
    let mut tmax = 1.0f32;
    m.set_u16(out_start, 0);
    m.set_u16(out_end, 0);
    let n = m.u32(soc) as i32;
    let f = |m: &Mem, a: u32| x(m.f32(a));
    let mut i: i32 = 0;
    while i < n {
        let a = ((i + 1) % n) as u32 * 12;
        let c = ((i + 2) % n) as u32 * 12;
        let b = i as u32 * 12;
        let pts = m.u32(soc + 8);
        let nrm = m.u32(soc + 0xc);
        let e = [f(m, pts + a) - f(m, pts + b), f(m, pts + a + 4) - f(m, pts + b + 4), f(m, pts + a + 8) - f(m, pts + b + 8)];
        let er = [e[0].to_f32(), e[1].to_f32(), e[2].to_f32()];
        let (n10, n14, n18) = (f(m, soc + 0x10), f(m, soc + 0x14), f(m, soc + 0x18));
        let rr = |v: f32| x(v);
        let nx = e[2] * n14 - e[1] * n18;
        let ny = n18 * rr(er[0]) - n10 * rr(er[2]);
        let nz = n10 * rr(er[1]) - n14 * rr(er[0]);
        let nrm = nrm + b;
        m.set_f32(nrm, nx.to_f32());
        m.set_f32(nrm + 4, ny.to_f32());
        m.set_f32(nrm + 8, nz.to_f32());
        let pts = m.u32(soc + 8);
        let t9 = f(m, pts + c) - f(m, pts + b);
        let t10 = f(m, pts + c + 4) - f(m, pts + b + 4);
        let t11 = f(m, pts + c + 8) - f(m, pts + b + 8);
        let dot = t11 * f(m, nrm + 8) + (t10 * f(m, nrm + 4) + t9 * f(m, nrm));
        if dot.lt(x(0.0f32)) {
            for k in 0..3 {
                m.set_f32(nrm + 4 * k, (-f(m, nrm + 4 * k)).to_f32());
            }
        }
        let pts = m.u32(soc + 8);
        let t12 = f(m, line) - f(m, pts + b);
        let t13 = f(m, line + 4) - f(m, pts + b + 4);
        let t14 = f(m, line + 8) - f(m, pts + b + 8);
        let (q0, q1, q2) = (f(m, nrm), f(m, nrm + 4), f(m, nrm + 8));
        let t15 = f(m, line + 0x14) * q2 + (f(m, line + 0x10) * q1 + f(m, line + 0xc) * q0);
        let num = q2 * t14 + (q0 * t12 + q1 * t13);
        let zero = x(0.0f32);
        if t15.eq(zero) {
            if zero.gt(num) {
                return 0;
            }
        } else {
            let t16 = (-num) / t15;
            if t15.gt(zero) && !x(1.0f32).lt(t16) && x(tmin).lt(t16) {
                tmin = t16.to_f32();
            }
            if zero.gt(t15) && t16.ge(zero) && x(tmax).gt(t16) {
                tmax = t16.to_f32();
            }
        }
        i += 1;
    }
    if x(tmin).gt(x(tmax)) {
        return 0;
    }
    if x(0.0f32).ge(x(tmin)) {
        m.set_u16(out_start, 1);
    }
    if !x(1.0f32).gt(x(tmax)) {
        m.set_u16(out_end, 1);
    }
    for k in 0..3u32 {
        let d = f(m, line + 0xc + 4 * k);
        m.set_f32(p1 + 4 * k, (x(tmin) * d).to_f32());
        m.set_f32(p2 + 4 * k, (x(tmax) * d).to_f32());
    }
    for k in 0..3u32 {
        let o = f(m, line + 4 * k);
        m.set_f32(p1 + 4 * k, (f(m, p1 + 4 * k) + o).to_f32());
        m.set_f32(p2 + 4 * k, (f(m, p2 + 4 * k) + o).to_f32());
    }
    1
}

/// Whether a contact surface `soc` of a body is stable for a velocity `(vx, vy, vz)` (the float words) of the body
/// `model` [soft 0x084ab1b0 si_dynaEvaluateContactStability]: the vector is turned into the body's frame by the 3 x 3
/// matrix at the body record +0x220, then a line from the model's reference point (the settings record's +0x64 when
/// [model+0x304] is set, else [model+0x2f4]+0x20) is cut with the plane of the surface (its first point and normal)
/// once along that vector and once along the negated normal; the line from the second point to the first is
/// clipped against the surface (`line_polygon_clipping`). The clipped end point goes to `out`; the result is the
/// 16-bit "line ends inside" flag of the clipping.
pub fn si_dyna_evaluate_contact_stability(m: &mut Mem, model: u32, soc: u32, vx: u32, vy: u32, vz: u32, out: u32) -> i32 {
    use si_math::algebra::{line_plane_intersection, Line, Plane};
    use si_math::types::Vector;
    let r = m.u32(model + 0x300);
    let f = |m: &Mem, a: u32| x(m.f32(a));
    let (ux, uy, uz) = (x(f32::from_bits(vx)), x(f32::from_bits(vy)), x(f32::from_bits(vz)));
    let d0 = ((f(m, r + 0x224) * uy + f(m, r + 0x220) * ux) + f(m, r + 0x228) * uz).to_f32();
    let d1 = ((f(m, r + 0x22c) * ux + f(m, r + 0x230) * uy) + f(m, r + 0x234) * uz).to_f32();
    let d2 = ((uy * f(m, r + 0x23c) + ux * f(m, r + 0x238)) + uz * f(m, r + 0x240)).to_f32();
    let w = |m: &Mem, a: u32| Vector::new(m.f32(a), m.f32(a + 4), m.f32(a + 8));
    let sett = m.u32(model + 0x304);
    let org = if sett != 0 { sett + 0x64 } else { m.u32(model + 0x2f4) + 0x20 };
    let origin = w(m, org);
    let pts = m.u32(soc + 8);
    let plane = Plane { point: w(m, pts), normal: w(m, soc + 0x10) };
    let p_a = line_plane_intersection(&Line { point: origin, dir: Vector::new(d0, d1, d2) }, &plane);
    let n = w(m, soc + 0x10);
    let p_b = line_plane_intersection(&Line { point: origin, dir: Vector::new(-n.x, -n.y, -n.z) }, &plane);
    let fr = m.local(0x40);
    let vals = [
        p_b.x,
        p_b.y,
        p_b.z,
        (x(p_a.x) - p_b.x).to_f32(),
        (x(p_a.y) - p_b.y).to_f32(),
        (x(p_a.z) - p_b.z).to_f32(),
    ];
    for (k, v) in vals.iter().enumerate() {
        m.set_f32(fr + 4 * k as u32, *v);
    }
    line_polygon_clipping(m, fr, soc, fr + 0x18, fr + 0x1c, fr + 0x20, fr + 0x2c);
    for k in 0..3u32 {
        let v = m.f32(fr + 0x2c + 4 * k);
        m.set_f32(out + 4 * k, v);
    }
    let end = m.i16(fr + 0x1c) as i32;
    m.release(0x40);
    end
}

// Reads of the image by a signed address, as the drafts of the larger routines use them.
pub(crate) fn rf(m: &Mem, a: i32) -> F80 {
    x(m.f32(a as u32))
}
pub(crate) fn ri(m: &Mem, a: i32) -> i32 {
    m.i32(a as u32)
}

/// [soft 0x084a927c si_dynaEvaluateSphereSupport]: the support point of a sphere-shaped contact of a joint-driven body
/// (see the finding for what is read from the code).
pub fn si_dyna_evaluate_sphere_support(m: &mut Mem, mdl1: u32, mdl2: u32, jv: u32, ci: u32, pa: u32, pb: u32) {
    let fr = m.local(0xbc);
    sphere_support_body(m, (fr + 0xbc) as i32, mdl1 as i32, mdl2 as i32, jv as i32, ci as i32, pa as i32, pb as i32);
    m.release(0xbc);
}

/// The body of si_dynaEvaluateSphereSupport: control flow written from the listing, arithmetic expanded from the
/// symbolic x87 order (re/tools/si-dynamics/x87tpl.py); `ebp` is the frame base, slots are ebp-0xNN.
#[allow(unused_assignments, unused_mut, unused_variables, unused_parens, non_snake_case, clippy::all)]
fn sphere_support_body(m: &mut Mem, ebp: i32, M1: i32, M2: i32, JV: i32, CI: i32, PA: i32, PB: i32) {
    macro_rules! L {
        ($o:expr) => {
            x(m.f32((ebp - $o) as u32))
        };
    }
    fn neg(a: F80) -> F80 {
        -a
    }
    fn abs(a: F80) -> F80 {
        a.abs()
    }
    let z = x(0.0f32);
    let mut vl_14_54 = x(0.0f32);
    let mut vl_18_54 = x(0.0f32);
    let mut vl_2c_85 = x(0.0f32);
    let mut vl_30_85 = x(0.0f32);
    let mut vl_44_96 = x(0.0f32);
    let mut vl_48_96 = x(0.0f32);
    let mut vl_68_125 = x(0.0f32);
    let mut vl_6c_125 = x(0.0f32);
    let mut vl_74_161 = x(0.0f32);
    let mut vl_78_161 = x(0.0f32);
    let mut vl_8_14 = x(0.0f32);
    let mut vl_94_128 = x(0.0f32);
    let mut vl_94_164 = x(0.0f32);
    let mut vl_94_166 = x(0.0f32);
    let mut vl_c_14 = x(0.0f32);
    let mut vr0_30_139 = x(0.0f32);
    let mut vr0_30_145 = x(0.0f32);
    let mut vr0_34_139 = x(0.0f32);
    let (mut phi2, mut phi3, mut phi4, mut phi5) = (z, z, z, z);
    let (mut phi9, mut phi10, mut phi11, mut phi12, mut phi13, mut phi14) = (z, z, z, z, z, z);
    let (mut phi15, mut phi16, mut phi17, mut phi18, mut phi19) = (z, z, z, z, z);
    let (mut phi24, mut phi25, mut phi26) = (z, z, z);
    let (mut phi27, mut phi28, mut phi29, mut phi30, mut phi31, mut phi32, mut phi33) = (z, z, z, z, z, z, z);
    let (mut phi34, mut phi35, mut phi36, mut phi37, mut phi38, mut phi39, mut phi40, mut phi41) = (z, z, z, z, z, z, z, z);
    let kind = ri(m, M1 + 0x2ec);
    let G = ri(m, CI + 0x10);
    let R4 = ri(m, M2 + 0x2f4);
    let R3 = ri(m, M2 + 0x304);
    m.set_f32((ebp - 0xa4) as u32, (rf(m, R4 + 0xc)).to_f32());
    m.set_f32((ebp - 0xa8) as u32, (rf(m, R4 + 0x10)).to_f32());
    m.set_f32((ebp - 0xac) as u32, ((x(m.f64(0x08660c80)) * rf(m, R4 + 0x10))).to_f32());
    m.set_i32((ebp - 0xb0) as u32, R3);
    let phi1 = if R3 == 0 { rf(m, R4 + 0x8) } else { rf(m, R3 + 0x60) };
    if kind == 2 {
        let t3 = ((((rf(m, PA + 0x4) * rf(m, M1 + 0x188)) + (rf(m, PA + 0x0) * rf(m, M1 + 0x178))) + (rf(m, PA + 0x8) * rf(m, M1 + 0x198))) + rf(m, M1 + 0x1a8));
        m.set_f32((ebp - 0xc) as u32, (t3).to_f32());
        let t4 = ((((rf(m, PA + 0x0) * rf(m, M1 + 0x17c)) + (rf(m, PA + 0x4) * rf(m, M1 + 0x18c))) + (rf(m, PA + 0x8) * rf(m, M1 + 0x19c))) + rf(m, M1 + 0x1ac));
        m.set_f32((ebp - 0x8) as u32, (t4).to_f32());
        phi2 = t4;
        phi3 = t3;
        phi4 = (((rf(m, PA + 0x8) * rf(m, M1 + 0x1a0)) + ((rf(m, PA + 0x4) * rf(m, M1 + 0x190)) + (rf(m, PA + 0x0) * rf(m, M1 + 0x180)))) + rf(m, M1 + 0x1b0));
        m.set_f32((ebp - 0x4) as u32, (phi4).to_f32());
        m.set_f32((ebp - 0xc) as u32, ((((rf(m, G + 0x178) * phi3) + (rf(m, G + 0x17c) * phi2)) + (rf(m, G + 0x180) * phi4))).to_f32());
        m.set_f32((ebp - 0x8) as u32, ((((rf(m, G + 0x188) * phi3) + (rf(m, G + 0x18c) * phi2)) + (rf(m, G + 0x190) * phi4))).to_f32());
        phi5 = ((phi4 * rf(m, G + 0x1a0)) + ((phi2 * rf(m, G + 0x19c)) + (phi3 * rf(m, G + 0x198))));
        m.set_f32((ebp - 0x4) as u32, (phi5).to_f32());
        vl_8_14 = L!(0x8);
        vl_c_14 = L!(0xc);
    } else if kind == 12 {
        let B = ri(m, M1 + 0x2bc);
        if B == 0 {
            m.set_f32((ebp - 0xc) as u32, ((((rf(m, G + 0x17c) * rf(m, PA + 0x4)) + (rf(m, G + 0x178) * rf(m, PA + 0x0))) + (rf(m, G + 0x180) * rf(m, PA + 0x8)))).to_f32());
            m.set_f32((ebp - 0x8) as u32, ((((rf(m, G + 0x188) * rf(m, PA + 0x0)) + (rf(m, G + 0x18c) * rf(m, PA + 0x4))) + (rf(m, G + 0x190) * rf(m, PA + 0x8)))).to_f32());
            phi5 = (((rf(m, PA + 0x4) * rf(m, G + 0x19c)) + (rf(m, PA + 0x0) * rf(m, G + 0x198))) + (rf(m, PA + 0x8) * rf(m, G + 0x1a0)));
        } else {
            let t1 = ((((rf(m, PA + 0x4) * rf(m, B + 0x188)) + (rf(m, PA + 0x0) * rf(m, B + 0x178))) + (rf(m, PA + 0x8) * rf(m, B + 0x198))) + rf(m, B + 0x1a8));
            m.set_f32((ebp - 0xc) as u32, (t1).to_f32());
            let t2 = ((((rf(m, PA + 0x0) * rf(m, B + 0x17c)) + (rf(m, PA + 0x4) * rf(m, B + 0x18c))) + (rf(m, PA + 0x8) * rf(m, B + 0x19c))) + rf(m, B + 0x1ac));
            m.set_f32((ebp - 0x8) as u32, (t2).to_f32());
            phi2 = t2;
            phi3 = t1;
            phi4 = (((rf(m, PA + 0x8) * rf(m, B + 0x1a0)) + ((rf(m, PA + 0x4) * rf(m, B + 0x190)) + (rf(m, PA + 0x0) * rf(m, B + 0x180)))) + rf(m, B + 0x1b0));
            m.set_f32((ebp - 0x4) as u32, (phi4).to_f32());
            m.set_f32((ebp - 0xc) as u32, ((((rf(m, G + 0x178) * phi3) + (rf(m, G + 0x17c) * phi2)) + (rf(m, G + 0x180) * phi4))).to_f32());
            m.set_f32((ebp - 0x8) as u32, ((((rf(m, G + 0x188) * phi3) + (rf(m, G + 0x18c) * phi2)) + (rf(m, G + 0x190) * phi4))).to_f32());
            phi5 = ((phi4 * rf(m, G + 0x1a0)) + ((phi2 * rf(m, G + 0x19c)) + (phi3 * rf(m, G + 0x198))));
        }
        m.set_f32((ebp - 0x4) as u32, (phi5).to_f32());
        vl_8_14 = L!(0x8);
        vl_c_14 = L!(0xc);
    }
    let H = ri(m, CI + 0x38);
    let t5 = (((rf(m, H + 0x38) * L!(0x8)) + (rf(m, H + 0x34) * L!(0xc))) + (rf(m, H + 0x3c) * L!(0x4)));
    m.set_f32((ebp - 0xc) as u32, (t5).to_f32());
    let t6 = (((rf(m, H + 0x44) * vl_c_14) + (rf(m, H + 0x48) * vl_8_14)) + (rf(m, H + 0x4c) * L!(0x4)));
    m.set_f32((ebp - 0x8) as u32, (t6).to_f32());
    m.set_f32((ebp - 0x14) as u32, (neg(t6)).to_f32());
    let t8 = ((L!(0x4) * rf(m, H + 0x5c)) + ((vl_8_14 * rf(m, H + 0x58)) + (vl_c_14 * rf(m, H + 0x54))));
    m.set_f32((ebp - 0x4) as u32, (t8).to_f32());
    m.set_i32((ebp - 0x50) as u32, 0x0);
    let t7 = neg(L!(0xa8));
    m.set_f32((ebp - 0x18) as u32, ((t7 * t5)).to_f32());
    m.set_f32((ebp - 0x10) as u32, ((t7 * t8)).to_f32());
    m.set_f32((ebp - 0x54) as u32, (((t5 + (t7 * t5)) / phi1)).to_f32());
    m.set_f32((ebp - 0x4c) as u32, (((t8 + (t7 * t8)) / phi1)).to_f32());
    let a16c = abs(rf(m, M2 + 0x16c));
    let a170 = abs(rf(m, M2 + 0x170));
    let phi6 = if a170.lt(a16c) { a16c } else { a170 };
    let a174 = abs(rf(m, M2 + 0x174));
    let phi8 = if a174.lt(phi6) { phi6 } else { a174 };
    m.set_f32((ebp - 0x94) as u32, (phi8).to_f32());
    m.set_i32((ebp - 0x5c) as u32, 0x0);
    let t9 = (L!(0xa4) / (L!(0x94) * rf(m, M2 + 0x2c)));
    let t10 = (t9 * L!(0x4c));
    m.set_f32((ebp - 0x60) as u32, (t10).to_f32());
    let t11 = (neg(t9) * L!(0x54));
    m.set_f32((ebp - 0x58) as u32, (t11).to_f32());
    let t12 = ((((t10 * rf(m, H + 0x34)) + (rf(m, H + 0x44) * x(0.0f32))) + (t11 * rf(m, H + 0x54))) + rf(m, H + 0x64));
    m.set_f32((ebp - 0x60) as u32, (t12).to_f32());
    let t13 = ((((t10 * rf(m, H + 0x38)) + (rf(m, H + 0x48) * x(0.0f32))) + (t11 * rf(m, H + 0x58))) + rf(m, H + 0x68));
    m.set_f32((ebp - 0x5c) as u32, (t13).to_f32());
    let t14 = (((t11 * rf(m, H + 0x5c)) + ((x(0.0f32) * rf(m, H + 0x4c)) + (t10 * rf(m, H + 0x3c)))) + rf(m, H + 0x6c));
    m.set_f32((ebp - 0x58) as u32, (t14).to_f32());
    if kind == 2 {
        let t18 = ((((t12 * rf(m, G + 0x178)) + (t13 * rf(m, G + 0x188))) + (t14 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
        m.set_f32((ebp - 0x84) as u32, (t18).to_f32());
        let t19 = ((((t12 * rf(m, G + 0x17c)) + (t13 * rf(m, G + 0x18c))) + (t14 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
        m.set_f32((ebp - 0x80) as u32, (t19).to_f32());
        m.set_f32((ebp - 0x84) as u32, ((((rf(m, M1 + 0x17c) * t19) + (rf(m, M1 + 0x178) * t18)) + (rf(m, M1 + 0x180) * ((((t13 * rf(m, G + 0x190)) + (t12 * rf(m, G + 0x180))) + (t14 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))))).to_f32());
        m.set_f32((ebp - 0x80) as u32, ((((rf(m, M1 + 0x188) * t18) + (rf(m, M1 + 0x18c) * t19)) + (rf(m, M1 + 0x190) * ((((t13 * rf(m, G + 0x190)) + (t12 * rf(m, G + 0x180))) + (t14 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))))).to_f32());
        phi9 = (t19 * rf(m, M1 + 0x19c));
        phi10 = (t18 * rf(m, M1 + 0x198));
        phi11 = (((((t13 * rf(m, G + 0x190)) + (t12 * rf(m, G + 0x180))) + (t14 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0)) * rf(m, M1 + 0x1a0));
        m.set_f32((ebp - 0x7c) as u32, (((phi9 + phi10) + phi11)).to_f32());
    } else if kind == 12 {
        let B = ri(m, M1 + 0x2bc);
        if B == 0 {
            m.set_f32((ebp - 0x84) as u32, (((((t12 * rf(m, G + 0x178)) + (t13 * rf(m, G + 0x188))) + (t14 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
            m.set_f32((ebp - 0x80) as u32, (((((t13 * rf(m, G + 0x18c)) + (t12 * rf(m, G + 0x17c))) + (t14 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
            m.set_f32((ebp - 0x7c) as u32, ((((t14 * rf(m, G + 0x1a0)) + ((t13 * rf(m, G + 0x190)) + (t12 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0))).to_f32());
        } else {
            let t15 = ((((t12 * rf(m, G + 0x178)) + (t13 * rf(m, G + 0x188))) + (t14 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
            m.set_f32((ebp - 0x84) as u32, (t15).to_f32());
            let t16 = ((((t12 * rf(m, G + 0x17c)) + (t13 * rf(m, G + 0x18c))) + (t14 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
            m.set_f32((ebp - 0x80) as u32, (t16).to_f32());
            let t17 = (((t14 * rf(m, G + 0x1a0)) + ((t13 * rf(m, G + 0x190)) + (t12 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0));
            m.set_f32((ebp - 0x7c) as u32, (t17).to_f32());
            m.set_f32((ebp - 0x84) as u32, ((((rf(m, B + 0x178) * t15) + (rf(m, B + 0x17c) * t16)) + (rf(m, B + 0x180) * t17))).to_f32());
            m.set_f32((ebp - 0x80) as u32, ((((rf(m, B + 0x188) * t15) + (rf(m, B + 0x18c) * t16)) + (rf(m, B + 0x190) * t17))).to_f32());
            phi9 = (t16 * rf(m, B + 0x19c));
            phi10 = (t15 * rf(m, B + 0x198));
            phi11 = (t17 * rf(m, B + 0x1a0));
            m.set_f32((ebp - 0x7c) as u32, (((phi9 + phi10) + phi11)).to_f32());
        }
    }
    if kind == 2 {
        let t20 = (((rf(m, JV + 0x224) * L!(0x80)) + (rf(m, JV + 0x220) * L!(0x84))) + (rf(m, JV + 0x228) * L!(0x7c)));
        m.set_f32((ebp - 0x90) as u32, (t20).to_f32());
        let t21 = (((rf(m, JV + 0x22c) * L!(0x84)) + (rf(m, JV + 0x230) * L!(0x80))) + (rf(m, JV + 0x234) * L!(0x7c)));
        m.set_f32((ebp - 0x8c) as u32, (t21).to_f32());
        let t22 = ((L!(0x7c) * rf(m, JV + 0x240)) + ((L!(0x80) * rf(m, JV + 0x23c)) + (L!(0x84) * rf(m, JV + 0x238))));
        m.set_f32((ebp - 0x88) as u32, (t22).to_f32());
        m.set_f32((ebp - 0x3c) as u32, ((((rf(m, R4 + 0x2c) * t20) + (rf(m, R4 + 0x30) * t21)) + (rf(m, R4 + 0x34) * t22))).to_f32());
        m.set_f32((ebp - 0x38) as u32, ((((rf(m, R4 + 0x38) * t20) + (rf(m, R4 + 0x3c) * t21)) + (rf(m, R4 + 0x40) * t22))).to_f32());
        phi12 = (t20 * rf(m, R4 + 0x44));
        phi13 = (t21 * rf(m, R4 + 0x48));
        phi14 = (t22 * rf(m, R4 + 0x4c));
    } else {
        { let v = m.i32((ebp - 0x84) as u32); m.set_i32((ebp - 0x90) as u32, v); }
        { let v = m.i32((ebp - 0x80) as u32); m.set_i32((ebp - 0x8c) as u32, v); }
        { let v = m.i32((ebp - 0x7c) as u32); m.set_i32((ebp - 0x88) as u32, v); }
        m.set_f32((ebp - 0x3c) as u32, ((((rf(m, R3 + 0x74) * L!(0x8c)) + (rf(m, R3 + 0x70) * L!(0x90))) + (rf(m, R3 + 0x78) * L!(0x88)))).to_f32());
        m.set_f32((ebp - 0x38) as u32, ((((rf(m, R3 + 0x7c) * L!(0x90)) + (rf(m, R3 + 0x80) * L!(0x8c))) + (rf(m, R3 + 0x84) * L!(0x88)))).to_f32());
        phi12 = (L!(0x90) * rf(m, R3 + 0x88));
        phi13 = (L!(0x8c) * rf(m, R3 + 0x8c));
        phi14 = (L!(0x88) * rf(m, R3 + 0x90));
    }
    m.set_f32((ebp - 0x34) as u32, ((phi14 + (phi13 + phi12))).to_f32());
    vl_14_54 = L!(0x14);
    vl_18_54 = L!(0x18);
    let t23 = ((((L!(0x14) * rf(m, H + 0x44)) + (L!(0x18) * rf(m, H + 0x34))) + (L!(0x10) * rf(m, H + 0x54))) + rf(m, H + 0x64));
    m.set_f32((ebp - 0x18) as u32, (t23).to_f32());
    let t24 = ((((vl_18_54 * rf(m, H + 0x38)) + (vl_14_54 * rf(m, H + 0x48))) + (L!(0x10) * rf(m, H + 0x58))) + rf(m, H + 0x68));
    m.set_f32((ebp - 0x14) as u32, (t24).to_f32());
    let t25 = (((L!(0x10) * rf(m, H + 0x5c)) + ((vl_14_54 * rf(m, H + 0x4c)) + (vl_18_54 * rf(m, H + 0x3c)))) + rf(m, H + 0x6c));
    m.set_f32((ebp - 0x10) as u32, (t25).to_f32());
    if kind == 2 {
        let t29 = ((((t23 * rf(m, G + 0x178)) + (t24 * rf(m, G + 0x188))) + (t25 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
        m.set_f32((CI + 0x14) as u32, (t29).to_f32());
        let t30 = ((((t23 * rf(m, G + 0x17c)) + (t24 * rf(m, G + 0x18c))) + (t25 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
        m.set_f32((CI + 0x18) as u32, (t30).to_f32());
        let t31 = (((t25 * rf(m, G + 0x1a0)) + ((t24 * rf(m, G + 0x190)) + (t23 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0));
        m.set_f32((CI + 0x1c) as u32, (t31).to_f32());
        m.set_f32((CI + 0x14) as u32, ((((rf(m, M1 + 0x178) * t29) + (rf(m, M1 + 0x17c) * t30)) + (rf(m, M1 + 0x180) * t31))).to_f32());
        m.set_f32((CI + 0x18) as u32, ((((rf(m, M1 + 0x188) * t29) + (rf(m, M1 + 0x18c) * t30)) + (rf(m, M1 + 0x190) * t31))).to_f32());
        m.set_f32((CI + 0x1c) as u32, ((((t30 * rf(m, M1 + 0x19c)) + (t29 * rf(m, M1 + 0x198))) + (t31 * rf(m, M1 + 0x1a0)))).to_f32());
    } else if kind == 12 {
        let B = ri(m, M1 + 0x2bc);
        if B == 0 {
            m.set_f32((CI + 0x14) as u32, (((((t23 * rf(m, G + 0x178)) + (t24 * rf(m, G + 0x188))) + (t25 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
            m.set_f32((CI + 0x18) as u32, (((((t24 * rf(m, G + 0x18c)) + (t23 * rf(m, G + 0x17c))) + (t25 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
            m.set_f32((CI + 0x1c) as u32, ((((t25 * rf(m, G + 0x1a0)) + ((t24 * rf(m, G + 0x190)) + (t23 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0))).to_f32());
        } else {
            let t26 = ((((t23 * rf(m, G + 0x178)) + (t24 * rf(m, G + 0x188))) + (t25 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
            m.set_f32((CI + 0x14) as u32, (t26).to_f32());
            let t27 = ((((t23 * rf(m, G + 0x17c)) + (t24 * rf(m, G + 0x18c))) + (t25 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
            m.set_f32((CI + 0x18) as u32, (t27).to_f32());
            let t28 = (((t25 * rf(m, G + 0x1a0)) + ((t24 * rf(m, G + 0x190)) + (t23 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0));
            m.set_f32((CI + 0x1c) as u32, (t28).to_f32());
            m.set_f32((CI + 0x14) as u32, ((((rf(m, B + 0x178) * t26) + (rf(m, B + 0x17c) * t27)) + (rf(m, B + 0x180) * t28))).to_f32());
            m.set_f32((CI + 0x18) as u32, ((((rf(m, B + 0x188) * t26) + (rf(m, B + 0x18c) * t27)) + (rf(m, B + 0x190) * t28))).to_f32());
            m.set_f32((CI + 0x1c) as u32, ((((t27 * rf(m, B + 0x19c)) + (t26 * rf(m, B + 0x198))) + (t28 * rf(m, B + 0x1a0)))).to_f32());
        }
    }
    let t32 = (((rf(m, JV + 0x1dc) * rf(m, PB + 0x4)) + (rf(m, JV + 0x1d8) * rf(m, PB + 0x0))) + (rf(m, JV + 0x1e0) * rf(m, PB + 0x8)));
    m.set_f32((ebp - 0x24) as u32, (t32).to_f32());
    let t33 = (((rf(m, JV + 0x1e4) * rf(m, PB + 0x0)) + (rf(m, JV + 0x1e8) * rf(m, PB + 0x4))) + (rf(m, JV + 0x1ec) * rf(m, PB + 0x8)));
    m.set_f32((ebp - 0x20) as u32, (t33).to_f32());
    let t34 = ((rf(m, PB + 0x8) * rf(m, JV + 0x1f8)) + ((rf(m, PB + 0x4) * rf(m, JV + 0x1f4)) + (rf(m, PB + 0x0) * rf(m, JV + 0x1f0))));
    m.set_f32((ebp - 0x1c) as u32, (t34).to_f32());
    if kind == 2 {
        let t37 = ((((t32 * rf(m, M1 + 0x178)) + (t33 * rf(m, M1 + 0x188))) + (t34 * rf(m, M1 + 0x198))) + rf(m, M1 + 0x1a8));
        m.set_f32((ebp - 0x30) as u32, (t37).to_f32());
        let t38 = ((((t32 * rf(m, M1 + 0x17c)) + (t33 * rf(m, M1 + 0x18c))) + (t34 * rf(m, M1 + 0x19c))) + rf(m, M1 + 0x1ac));
        m.set_f32((ebp - 0x2c) as u32, (t38).to_f32());
        phi15 = t37;
        phi16 = t38;
        phi17 = (((t34 * rf(m, M1 + 0x1a0)) + ((t33 * rf(m, M1 + 0x190)) + (t32 * rf(m, M1 + 0x180)))) + rf(m, M1 + 0x1b0));
        m.set_f32((ebp - 0x28) as u32, (phi17).to_f32());
        m.set_f32((ebp - 0x30) as u32, ((((rf(m, G + 0x178) * phi15) + (rf(m, G + 0x17c) * phi16)) + (rf(m, G + 0x180) * phi17))).to_f32());
        m.set_f32((ebp - 0x2c) as u32, ((((rf(m, G + 0x188) * phi15) + (rf(m, G + 0x18c) * phi16)) + (rf(m, G + 0x190) * phi17))).to_f32());
        phi18 = ((phi16 * rf(m, G + 0x19c)) + (phi15 * rf(m, G + 0x198)));
        phi19 = (phi17 * rf(m, G + 0x1a0));
        m.set_f32((ebp - 0x28) as u32, ((phi18 + phi19)).to_f32());
        vl_2c_85 = L!(0x2c);
        vl_30_85 = L!(0x30);
    } else if kind == 12 {
        let B = ri(m, M1 + 0x2bc);
        if B == 0 {
            m.set_f32((ebp - 0x30) as u32, ((((rf(m, G + 0x178) * t32) + (rf(m, G + 0x17c) * t33)) + (rf(m, G + 0x180) * t34))).to_f32());
            m.set_f32((ebp - 0x2c) as u32, ((((rf(m, G + 0x188) * t32) + (rf(m, G + 0x18c) * t33)) + (rf(m, G + 0x190) * t34))).to_f32());
            phi18 = ((t33 * rf(m, G + 0x19c)) + (t32 * rf(m, G + 0x198)));
            phi19 = (t34 * rf(m, G + 0x1a0));
            m.set_f32((ebp - 0x28) as u32, ((phi18 + phi19)).to_f32());
            vl_2c_85 = L!(0x2c);
            vl_30_85 = L!(0x30);
        } else {
            let t35 = ((((t32 * rf(m, B + 0x178)) + (t33 * rf(m, B + 0x188))) + (t34 * rf(m, B + 0x198))) + rf(m, B + 0x1a8));
            m.set_f32((ebp - 0x30) as u32, (t35).to_f32());
            let t36 = ((((t32 * rf(m, B + 0x17c)) + (t33 * rf(m, B + 0x18c))) + (t34 * rf(m, B + 0x19c))) + rf(m, B + 0x1ac));
            m.set_f32((ebp - 0x2c) as u32, (t36).to_f32());
            phi15 = t35;
            phi16 = t36;
            phi17 = (((t34 * rf(m, B + 0x1a0)) + ((t33 * rf(m, B + 0x190)) + (t32 * rf(m, B + 0x180)))) + rf(m, B + 0x1b0));
            m.set_f32((ebp - 0x28) as u32, (phi17).to_f32());
            m.set_f32((ebp - 0x30) as u32, ((((rf(m, G + 0x178) * phi15) + (rf(m, G + 0x17c) * phi16)) + (rf(m, G + 0x180) * phi17))).to_f32());
            m.set_f32((ebp - 0x2c) as u32, ((((rf(m, G + 0x188) * phi15) + (rf(m, G + 0x18c) * phi16)) + (rf(m, G + 0x190) * phi17))).to_f32());
            phi18 = ((phi16 * rf(m, G + 0x19c)) + (phi15 * rf(m, G + 0x198)));
            phi19 = (phi17 * rf(m, G + 0x1a0));
            m.set_f32((ebp - 0x28) as u32, ((phi18 + phi19)).to_f32());
            vl_2c_85 = L!(0x2c);
            vl_30_85 = L!(0x30);
        }
    }
    m.set_f32((ebp - 0x30) as u32, ((((rf(m, H + 0x38) * L!(0x2c)) + (rf(m, H + 0x34) * L!(0x30))) + (rf(m, H + 0x3c) * L!(0x28)))).to_f32());
    let t39 = (((rf(m, H + 0x44) * vl_30_85) + (rf(m, H + 0x48) * vl_2c_85)) + (rf(m, H + 0x4c) * L!(0x28)));
    m.set_f32((ebp - 0x2c) as u32, (t39).to_f32());
    m.set_i32((ebp - 0x40) as u32, 0x0);
    m.set_i32((ebp - 0x44) as u32, 0x0);
    m.set_i32((ebp - 0x48) as u32, 0x0);
    m.set_f32((ebp - 0x28) as u32, ((((vl_2c_85 * rf(m, H + 0x58)) + (vl_30_85 * rf(m, H + 0x54))) + (L!(0x28) * rf(m, H + 0x5c)))).to_f32());
    if L!(0x8).lt(z) {
        let lim = neg(L!(0x8)) * L!(0xa4);
        let phi21;
        if abs(t39).le(lim) {
            phi21 = neg(t39);
        } else if t39.gt(z) {
            let t40 = L!(0x8) * L!(0xa8);
            m.set_f32((ebp - 0xa8) as u32, t40.to_f32());
            phi21 = t40;
        } else {
            phi21 = neg(L!(0x8) * L!(0xa8));
        }
        m.set_f32((ebp - 0x44) as u32, phi21.to_f32());
        if z.lt(L!(0x44)) {
            let phi22;
            if abs(L!(0x2c)).gt(L!(0x44)) {
                phi22 = L!(0x44);
            } else {
                m.set_f32((ebp - 0x94) as u32, abs(L!(0x2c)).to_f32());
                phi22 = L!(0x94);
            }
            m.set_f32((ebp - 0x44) as u32, phi22.to_f32());
        } else {
            let phi23 = if abs(L!(0x2c)).gt(neg(L!(0x44))) { neg(L!(0x44)) } else { abs(L!(0x2c)) };
            m.set_f32((ebp - 0x44) as u32, neg(phi23).to_f32());
        }
        vl_44_96 = L!(0x44);
        vl_48_96 = L!(0x48);
        let t41 = ((((L!(0x44) * rf(m, H + 0x44)) + (L!(0x48) * rf(m, H + 0x34))) + (L!(0x40) * rf(m, H + 0x54))) + rf(m, H + 0x64));
        m.set_f32((ebp - 0x48) as u32, (t41).to_f32());
        let t42 = ((((vl_48_96 * rf(m, H + 0x38)) + (vl_44_96 * rf(m, H + 0x48))) + (L!(0x40) * rf(m, H + 0x58))) + rf(m, H + 0x68));
        m.set_f32((ebp - 0x44) as u32, (t42).to_f32());
        let t43 = (((L!(0x40) * rf(m, H + 0x5c)) + ((vl_44_96 * rf(m, H + 0x4c)) + (vl_48_96 * rf(m, H + 0x3c)))) + rf(m, H + 0x6c));
        m.set_f32((ebp - 0x40) as u32, (t43).to_f32());
        if kind == 12 {
            let B = ri(m, M1 + 0x2bc);
            if B == 0 {
                m.set_f32((ebp - 0x48) as u32, (((((t41 * rf(m, G + 0x178)) + (t42 * rf(m, G + 0x188))) + (t43 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
                m.set_f32((ebp - 0x44) as u32, (((((t42 * rf(m, G + 0x18c)) + (t41 * rf(m, G + 0x17c))) + (t43 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
                m.set_f32((ebp - 0x40) as u32, ((((t43 * rf(m, G + 0x1a0)) + ((t42 * rf(m, G + 0x190)) + (t41 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0))).to_f32());
            } else {
                let t44 = ((((t41 * rf(m, G + 0x178)) + (t42 * rf(m, G + 0x188))) + (t43 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
                m.set_f32((ebp - 0x48) as u32, (t44).to_f32());
                let t45 = ((((t41 * rf(m, G + 0x17c)) + (t42 * rf(m, G + 0x18c))) + (t43 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
                m.set_f32((ebp - 0x44) as u32, (t45).to_f32());
                let t46 = (((t43 * rf(m, G + 0x1a0)) + ((t42 * rf(m, G + 0x190)) + (t41 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0));
                m.set_f32((ebp - 0x40) as u32, (t46).to_f32());
                m.set_f32((ebp - 0x48) as u32, ((((rf(m, B + 0x178) * t44) + (rf(m, B + 0x17c) * t45)) + (rf(m, B + 0x180) * t46))).to_f32());
                m.set_f32((ebp - 0x44) as u32, ((((rf(m, B + 0x188) * t44) + (rf(m, B + 0x18c) * t45)) + (rf(m, B + 0x190) * t46))).to_f32());
                phi24 = (t45 * rf(m, B + 0x19c));
                phi25 = (t44 * rf(m, B + 0x198));
                phi26 = (t46 * rf(m, B + 0x1a0));
                m.set_f32((ebp - 0x40) as u32, (((phi24 + phi25) + phi26)).to_f32());
            }
        } else if kind == 2 {
            let t47 = ((((t41 * rf(m, G + 0x178)) + (t42 * rf(m, G + 0x188))) + (t43 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
            m.set_f32((ebp - 0x48) as u32, (t47).to_f32());
            let t48 = ((((t41 * rf(m, G + 0x17c)) + (t42 * rf(m, G + 0x18c))) + (t43 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
            m.set_f32((ebp - 0x44) as u32, (t48).to_f32());
            m.set_f32((ebp - 0x48) as u32, ((((rf(m, M1 + 0x17c) * t48) + (rf(m, M1 + 0x178) * t47)) + (rf(m, M1 + 0x180) * ((((t42 * rf(m, G + 0x190)) + (t41 * rf(m, G + 0x180))) + (t43 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))))).to_f32());
            m.set_f32((ebp - 0x44) as u32, ((((rf(m, M1 + 0x188) * t47) + (rf(m, M1 + 0x18c) * t48)) + (rf(m, M1 + 0x190) * ((((t42 * rf(m, G + 0x190)) + (t41 * rf(m, G + 0x180))) + (t43 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))))).to_f32());
            phi24 = (t48 * rf(m, M1 + 0x19c));
            phi25 = (t47 * rf(m, M1 + 0x198));
            phi26 = (((((t42 * rf(m, G + 0x190)) + (t41 * rf(m, G + 0x180))) + (t43 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0)) * rf(m, M1 + 0x1a0));
            m.set_f32((ebp - 0x40) as u32, (((phi24 + phi25) + phi26)).to_f32());
        }
        m.set_f32((CI + 0x2c) as u32, ((((rf(m, JV + 0x224) * L!(0x44)) + (rf(m, JV + 0x220) * L!(0x48))) + (rf(m, JV + 0x228) * L!(0x40)))).to_f32());
        m.set_f32((CI + 0x30) as u32, ((((rf(m, JV + 0x22c) * L!(0x48)) + (rf(m, JV + 0x230) * L!(0x44))) + (rf(m, JV + 0x234) * L!(0x40)))).to_f32());
        m.set_f32((CI + 0x34) as u32, ((((L!(0x44) * rf(m, JV + 0x23c)) + (L!(0x48) * rf(m, JV + 0x238))) + (L!(0x40) * rf(m, JV + 0x240)))).to_f32());
    }
    let eps = x(m.f64(0x08660c88));
    if m.u16(0x086d7140) != 0 && abs(L!(0x18)).lt(eps) && abs(L!(0x10)).lt(eps) {
        let R0 = ri(m, M2 + 0x2f0);
        if kind == 12 {
            let B = ri(m, M1 + 0x2bc);
            if B == 0 {
                m.set_f32((ebp - 0x6c) as u32, ((((rf(m, G + 0x17c) * rf(m, R0 + 0x34)) + (rf(m, G + 0x178) * rf(m, R0 + 0x30))) + (rf(m, G + 0x180) * rf(m, R0 + 0x38)))).to_f32());
                m.set_f32((ebp - 0x68) as u32, ((((rf(m, G + 0x188) * rf(m, R0 + 0x30)) + (rf(m, G + 0x18c) * rf(m, R0 + 0x34))) + (rf(m, G + 0x190) * rf(m, R0 + 0x38)))).to_f32());
                phi30 = rf(m, R0 + 0x34);
                phi31 = rf(m, R0 + 0x38);
                phi32 = rf(m, R0 + 0x30);
                m.set_f32((ebp - 0x64) as u32, (((phi31 * rf(m, G + 0x1a0)) + ((phi30 * rf(m, G + 0x19c)) + (phi32 * rf(m, G + 0x198))))).to_f32());
                vl_68_125 = L!(0x68);
                vl_6c_125 = L!(0x6c);
            } else {
                let t49 = ((((rf(m, R0 + 0x34) * rf(m, B + 0x188)) + (rf(m, R0 + 0x30) * rf(m, B + 0x178))) + (rf(m, R0 + 0x38) * rf(m, B + 0x198))) + rf(m, B + 0x1a8));
                m.set_f32((ebp - 0x6c) as u32, (t49).to_f32());
                let t50 = ((((rf(m, R0 + 0x30) * rf(m, B + 0x17c)) + (rf(m, R0 + 0x34) * rf(m, B + 0x18c))) + (rf(m, R0 + 0x38) * rf(m, B + 0x19c))) + rf(m, B + 0x1ac));
                m.set_f32((ebp - 0x68) as u32, (t50).to_f32());
                phi27 = t50;
                phi28 = t49;
                phi29 = (((rf(m, R0 + 0x38) * rf(m, B + 0x1a0)) + ((rf(m, R0 + 0x34) * rf(m, B + 0x190)) + (rf(m, R0 + 0x30) * rf(m, B + 0x180)))) + rf(m, B + 0x1b0));
                m.set_f32((ebp - 0x64) as u32, (phi29).to_f32());
                m.set_f32((ebp - 0x6c) as u32, ((((rf(m, G + 0x178) * phi28) + (rf(m, G + 0x17c) * phi27)) + (rf(m, G + 0x180) * phi29))).to_f32());
                m.set_f32((ebp - 0x68) as u32, ((((rf(m, G + 0x188) * phi28) + (rf(m, G + 0x18c) * phi27)) + (rf(m, G + 0x190) * phi29))).to_f32());
                phi30 = phi27;
                phi31 = phi29;
                phi32 = phi28;
                m.set_f32((ebp - 0x64) as u32, (((phi31 * rf(m, G + 0x1a0)) + ((phi30 * rf(m, G + 0x19c)) + (phi32 * rf(m, G + 0x198))))).to_f32());
                vl_68_125 = L!(0x68);
                vl_6c_125 = L!(0x6c);
            }
        } else if kind == 2 {
            let t51 = ((((rf(m, R0 + 0x34) * rf(m, M1 + 0x188)) + (rf(m, R0 + 0x30) * rf(m, M1 + 0x178))) + (rf(m, R0 + 0x38) * rf(m, M1 + 0x198))) + rf(m, M1 + 0x1a8));
            m.set_f32((ebp - 0x6c) as u32, (t51).to_f32());
            let t52 = ((((rf(m, R0 + 0x30) * rf(m, M1 + 0x17c)) + (rf(m, R0 + 0x34) * rf(m, M1 + 0x18c))) + (rf(m, R0 + 0x38) * rf(m, M1 + 0x19c))) + rf(m, M1 + 0x1ac));
            m.set_f32((ebp - 0x68) as u32, (t52).to_f32());
            phi27 = t52;
            phi28 = t51;
            phi29 = (((rf(m, R0 + 0x38) * rf(m, M1 + 0x1a0)) + ((rf(m, R0 + 0x34) * rf(m, M1 + 0x190)) + (rf(m, R0 + 0x30) * rf(m, M1 + 0x180)))) + rf(m, M1 + 0x1b0));
            m.set_f32((ebp - 0x64) as u32, (phi29).to_f32());
            m.set_f32((ebp - 0x6c) as u32, ((((rf(m, G + 0x178) * phi28) + (rf(m, G + 0x17c) * phi27)) + (rf(m, G + 0x180) * phi29))).to_f32());
            m.set_f32((ebp - 0x68) as u32, ((((rf(m, G + 0x188) * phi28) + (rf(m, G + 0x18c) * phi27)) + (rf(m, G + 0x190) * phi29))).to_f32());
            phi30 = phi27;
            phi31 = phi29;
            phi32 = phi28;
            m.set_f32((ebp - 0x64) as u32, (((phi31 * rf(m, G + 0x1a0)) + ((phi30 * rf(m, G + 0x19c)) + (phi32 * rf(m, G + 0x198))))).to_f32());
            vl_68_125 = L!(0x68);
            vl_6c_125 = L!(0x6c);
        }
        let t53 = (((rf(m, H + 0x38) * L!(0x68)) + (rf(m, H + 0x34) * L!(0x6c))) + (rf(m, H + 0x3c) * L!(0x64)));
        m.set_f32((ebp - 0x6c) as u32, (t53).to_f32());
        let t54 = (((rf(m, H + 0x44) * vl_6c_125) + (rf(m, H + 0x48) * vl_68_125)) + (rf(m, H + 0x4c) * L!(0x64)));
        m.set_f32((ebp - 0x68) as u32, (t54).to_f32());
        let t55 = (x(1.0f32) - L!(0xac));
        m.set_f32((ebp - 0x94) as u32, ((t53 * t55)).to_f32());
        vl_94_128 = L!(0x94);
        m.set_f32((ebp - 0x94) as u32, ((((L!(0x64) * rf(m, H + 0x5c)) + ((vl_68_125 * rf(m, H + 0x58)) + (vl_6c_125 * rf(m, H + 0x54)))) * t55)).to_f32());
        m.set_f32((ebp - 0x64) as u32, (L!(0x94)).to_f32());
        m.set_f32((ebp - 0x6c) as u32, (vl_94_128).to_f32());
        let t56 = ((((t54 * rf(m, H + 0x44)) + (vl_94_128 * rf(m, H + 0x34))) + (L!(0x94) * rf(m, H + 0x54))) + rf(m, H + 0x64));
        m.set_f32((ebp - 0x6c) as u32, (t56).to_f32());
        let t57 = ((((vl_94_128 * rf(m, H + 0x38)) + (t54 * rf(m, H + 0x48))) + (L!(0x94) * rf(m, H + 0x58))) + rf(m, H + 0x68));
        m.set_f32((ebp - 0x68) as u32, (t57).to_f32());
        let t58 = (((L!(0x94) * rf(m, H + 0x5c)) + ((t54 * rf(m, H + 0x4c)) + (vl_94_128 * rf(m, H + 0x3c)))) + rf(m, H + 0x6c));
        m.set_f32((ebp - 0x64) as u32, (t58).to_f32());
        if kind == 2 {
            m.set_f32((R0 + 0x30) as u32, (((((t56 * rf(m, G + 0x178)) + (t57 * rf(m, G + 0x188))) + (t58 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
            m.set_f32((R0 + 0x34) as u32, (((((t57 * rf(m, G + 0x18c)) + (t56 * rf(m, G + 0x17c))) + (t58 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
            m.set_f32((R0 + 0x38) as u32, (((((t57 * rf(m, G + 0x190)) + (t56 * rf(m, G + 0x180))) + (t58 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))).to_f32());
            vr0_30_145 = rf(m, R0 + 0x30);
            phi33 = ((rf(m, R0 + 0x38) * rf(m, M1 + 0x1a0)) + ((rf(m, R0 + 0x34) * rf(m, M1 + 0x19c)) + (vr0_30_145 * rf(m, M1 + 0x198))));
            m.set_f32((R0 + 0x30) as u32, ((((rf(m, M1 + 0x17c) * rf(m, R0 + 0x34)) + (rf(m, M1 + 0x178) * rf(m, R0 + 0x30))) + (rf(m, M1 + 0x180) * rf(m, R0 + 0x38)))).to_f32());
            m.set_f32((R0 + 0x34) as u32, ((((rf(m, M1 + 0x188) * vr0_30_145) + (rf(m, M1 + 0x18c) * rf(m, R0 + 0x34))) + (rf(m, M1 + 0x190) * rf(m, R0 + 0x38)))).to_f32());
            m.set_f32((R0 + 0x38) as u32, (phi33).to_f32());
        } else if kind == 12 {
            let B = ri(m, M1 + 0x2bc);
            if B == 0 {
                m.set_f32((R0 + 0x30) as u32, (((((t56 * rf(m, G + 0x178)) + (t57 * rf(m, G + 0x188))) + (t58 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
                m.set_f32((R0 + 0x34) as u32, (((((t57 * rf(m, G + 0x18c)) + (t56 * rf(m, G + 0x17c))) + (t58 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
                phi33 = ((((t57 * rf(m, G + 0x190)) + (t56 * rf(m, G + 0x180))) + (t58 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0));
                m.set_f32((R0 + 0x38) as u32, (phi33).to_f32());
            } else {
                m.set_f32((R0 + 0x30) as u32, (((((t56 * rf(m, G + 0x178)) + (t57 * rf(m, G + 0x188))) + (t58 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
                m.set_f32((R0 + 0x34) as u32, (((((t57 * rf(m, G + 0x18c)) + (t56 * rf(m, G + 0x17c))) + (t58 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
                m.set_f32((R0 + 0x38) as u32, (((((t57 * rf(m, G + 0x190)) + (t56 * rf(m, G + 0x180))) + (t58 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))).to_f32());
                vr0_30_139 = rf(m, R0 + 0x30);
                vr0_34_139 = rf(m, R0 + 0x34);
                m.set_f32((R0 + 0x30) as u32, ((((rf(m, B + 0x17c) * rf(m, R0 + 0x34)) + (rf(m, B + 0x178) * rf(m, R0 + 0x30))) + (rf(m, B + 0x180) * rf(m, R0 + 0x38)))).to_f32());
                m.set_f32((R0 + 0x34) as u32, ((((rf(m, B + 0x188) * vr0_30_139) + (rf(m, B + 0x18c) * vr0_34_139)) + (rf(m, B + 0x190) * rf(m, R0 + 0x38)))).to_f32());
                m.set_f32((R0 + 0x38) as u32, ((((vr0_34_139 * rf(m, B + 0x19c)) + (vr0_30_139 * rf(m, B + 0x198))) + (rf(m, R0 + 0x38) * rf(m, B + 0x1a0)))).to_f32());
            }
        }
        let t59 = (((rf(m, JV + 0x1dc) * rf(m, R0 + 0x4c)) + (rf(m, JV + 0x1d8) * rf(m, R0 + 0x48))) + (rf(m, JV + 0x1e0) * rf(m, R0 + 0x50)));
        m.set_f32((ebp - 0x78) as u32, (t59).to_f32());
        let t60 = (((rf(m, JV + 0x1e4) * rf(m, R0 + 0x48)) + (rf(m, JV + 0x1e8) * rf(m, R0 + 0x4c))) + (rf(m, JV + 0x1ec) * rf(m, R0 + 0x50)));
        m.set_f32((ebp - 0x74) as u32, (t60).to_f32());
        let t61 = ((rf(m, R0 + 0x50) * rf(m, JV + 0x1f8)) + ((rf(m, R0 + 0x4c) * rf(m, JV + 0x1f4)) + (rf(m, R0 + 0x48) * rf(m, JV + 0x1f0))));
        m.set_f32((ebp - 0x70) as u32, (t61).to_f32());
        if kind == 2 {
            let t64 = ((((t59 * rf(m, M1 + 0x178)) + (t60 * rf(m, M1 + 0x188))) + (t61 * rf(m, M1 + 0x198))) + rf(m, M1 + 0x1a8));
            m.set_f32((ebp - 0x78) as u32, (t64).to_f32());
            let t65 = ((((t59 * rf(m, M1 + 0x17c)) + (t60 * rf(m, M1 + 0x18c))) + (t61 * rf(m, M1 + 0x19c))) + rf(m, M1 + 0x1ac));
            m.set_f32((ebp - 0x74) as u32, (t65).to_f32());
            phi34 = t64;
            phi35 = t65;
            phi36 = (((t61 * rf(m, M1 + 0x1a0)) + ((t60 * rf(m, M1 + 0x190)) + (t59 * rf(m, M1 + 0x180)))) + rf(m, M1 + 0x1b0));
            m.set_f32((ebp - 0x70) as u32, (phi36).to_f32());
            m.set_f32((ebp - 0x78) as u32, ((((rf(m, G + 0x178) * phi34) + (rf(m, G + 0x17c) * phi35)) + (rf(m, G + 0x180) * phi36))).to_f32());
            m.set_f32((ebp - 0x74) as u32, ((((rf(m, G + 0x188) * phi34) + (rf(m, G + 0x18c) * phi35)) + (rf(m, G + 0x190) * phi36))).to_f32());
            phi37 = ((phi35 * rf(m, G + 0x19c)) + (phi34 * rf(m, G + 0x198)));
            phi38 = (phi36 * rf(m, G + 0x1a0));
            m.set_f32((ebp - 0x70) as u32, ((phi37 + phi38)).to_f32());
            vl_74_161 = L!(0x74);
            vl_78_161 = L!(0x78);
        } else if kind == 12 {
            let B = ri(m, M1 + 0x2bc);
            if B == 0 {
                m.set_f32((ebp - 0x78) as u32, ((((rf(m, G + 0x178) * t59) + (rf(m, G + 0x17c) * t60)) + (rf(m, G + 0x180) * t61))).to_f32());
                m.set_f32((ebp - 0x74) as u32, ((((rf(m, G + 0x188) * t59) + (rf(m, G + 0x18c) * t60)) + (rf(m, G + 0x190) * t61))).to_f32());
                phi37 = ((t60 * rf(m, G + 0x19c)) + (t59 * rf(m, G + 0x198)));
                phi38 = (t61 * rf(m, G + 0x1a0));
                m.set_f32((ebp - 0x70) as u32, ((phi37 + phi38)).to_f32());
                vl_74_161 = L!(0x74);
                vl_78_161 = L!(0x78);
            } else {
                let t62 = ((((t59 * rf(m, B + 0x178)) + (t60 * rf(m, B + 0x188))) + (t61 * rf(m, B + 0x198))) + rf(m, B + 0x1a8));
                m.set_f32((ebp - 0x78) as u32, (t62).to_f32());
                let t63 = ((((t59 * rf(m, B + 0x17c)) + (t60 * rf(m, B + 0x18c))) + (t61 * rf(m, B + 0x19c))) + rf(m, B + 0x1ac));
                m.set_f32((ebp - 0x74) as u32, (t63).to_f32());
                phi34 = t62;
                phi35 = t63;
                phi36 = (((t61 * rf(m, B + 0x1a0)) + ((t60 * rf(m, B + 0x190)) + (t59 * rf(m, B + 0x180)))) + rf(m, B + 0x1b0));
                m.set_f32((ebp - 0x70) as u32, (phi36).to_f32());
                m.set_f32((ebp - 0x78) as u32, ((((rf(m, G + 0x178) * phi34) + (rf(m, G + 0x17c) * phi35)) + (rf(m, G + 0x180) * phi36))).to_f32());
                m.set_f32((ebp - 0x74) as u32, ((((rf(m, G + 0x188) * phi34) + (rf(m, G + 0x18c) * phi35)) + (rf(m, G + 0x190) * phi36))).to_f32());
                phi37 = ((phi35 * rf(m, G + 0x19c)) + (phi34 * rf(m, G + 0x198)));
                phi38 = (phi36 * rf(m, G + 0x1a0));
                m.set_f32((ebp - 0x70) as u32, ((phi37 + phi38)).to_f32());
                vl_74_161 = L!(0x74);
                vl_78_161 = L!(0x78);
            }
        }
        let t66 = (((rf(m, H + 0x38) * L!(0x74)) + (rf(m, H + 0x34) * L!(0x78))) + (rf(m, H + 0x3c) * L!(0x70)));
        m.set_f32((ebp - 0x78) as u32, (t66).to_f32());
        let t67 = (((rf(m, H + 0x44) * vl_78_161) + (rf(m, H + 0x48) * vl_74_161)) + (rf(m, H + 0x4c) * L!(0x70)));
        m.set_f32((ebp - 0x74) as u32, (t67).to_f32());
        let t68 = (x(1.0f32) - L!(0xac));
        m.set_f32((ebp - 0x94) as u32, ((t66 * t68)).to_f32());
        vl_94_164 = L!(0x94);
        m.set_f32((ebp - 0x78) as u32, (L!(0x94)).to_f32());
        m.set_f32((ebp - 0x94) as u32, ((t67 * t68)).to_f32());
        vl_94_166 = L!(0x94);
        m.set_f32((ebp - 0x94) as u32, ((t68 * ((L!(0x70) * rf(m, H + 0x5c)) + ((vl_78_161 * rf(m, H + 0x54)) + (vl_74_161 * rf(m, H + 0x58)))))).to_f32());
        m.set_f32((ebp - 0x70) as u32, (L!(0x94)).to_f32());
        m.set_f32((ebp - 0x74) as u32, (vl_94_166).to_f32());
        let t69 = ((((vl_94_164 * rf(m, H + 0x34)) + (vl_94_166 * rf(m, H + 0x44))) + (L!(0x94) * rf(m, H + 0x54))) + rf(m, H + 0x64));
        m.set_f32((ebp - 0x78) as u32, (t69).to_f32());
        let t70 = ((((vl_94_164 * rf(m, H + 0x38)) + (vl_94_166 * rf(m, H + 0x48))) + (L!(0x94) * rf(m, H + 0x58))) + rf(m, H + 0x68));
        m.set_f32((ebp - 0x74) as u32, (t70).to_f32());
        let t71 = (((L!(0x94) * rf(m, H + 0x5c)) + ((vl_94_166 * rf(m, H + 0x4c)) + (vl_94_164 * rf(m, H + 0x3c)))) + rf(m, H + 0x6c));
        m.set_f32((ebp - 0x70) as u32, (t71).to_f32());
        if kind == 2 {
            let t75 = ((((t69 * rf(m, G + 0x178)) + (t70 * rf(m, G + 0x188))) + (t71 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
            m.set_f32((ebp - 0x78) as u32, (t75).to_f32());
            let t76 = ((((t69 * rf(m, G + 0x17c)) + (t70 * rf(m, G + 0x18c))) + (t71 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
            m.set_f32((ebp - 0x74) as u32, (t76).to_f32());
            m.set_f32((ebp - 0x78) as u32, ((((rf(m, M1 + 0x17c) * t76) + (rf(m, M1 + 0x178) * t75)) + (rf(m, M1 + 0x180) * ((((t70 * rf(m, G + 0x190)) + (t69 * rf(m, G + 0x180))) + (t71 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))))).to_f32());
            m.set_f32((ebp - 0x74) as u32, ((((rf(m, M1 + 0x188) * t75) + (rf(m, M1 + 0x18c) * t76)) + (rf(m, M1 + 0x190) * ((((t70 * rf(m, G + 0x190)) + (t69 * rf(m, G + 0x180))) + (t71 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0))))).to_f32());
            phi39 = (t76 * rf(m, M1 + 0x19c));
            phi40 = (t75 * rf(m, M1 + 0x198));
            phi41 = (((((t70 * rf(m, G + 0x190)) + (t69 * rf(m, G + 0x180))) + (t71 * rf(m, G + 0x1a0))) + rf(m, G + 0x1b0)) * rf(m, M1 + 0x1a0));
            m.set_f32((ebp - 0x70) as u32, (((phi39 + phi40) + phi41)).to_f32());
        } else if kind == 12 {
            let B = ri(m, M1 + 0x2bc);
            if B == 0 {
                m.set_f32((ebp - 0x78) as u32, (((((t69 * rf(m, G + 0x178)) + (t70 * rf(m, G + 0x188))) + (t71 * rf(m, G + 0x198))) + rf(m, G + 0x1a8))).to_f32());
                m.set_f32((ebp - 0x74) as u32, (((((t70 * rf(m, G + 0x18c)) + (t69 * rf(m, G + 0x17c))) + (t71 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac))).to_f32());
                m.set_f32((ebp - 0x70) as u32, ((((t71 * rf(m, G + 0x1a0)) + ((t70 * rf(m, G + 0x190)) + (t69 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0))).to_f32());
            } else {
                let t72 = ((((t69 * rf(m, G + 0x178)) + (t70 * rf(m, G + 0x188))) + (t71 * rf(m, G + 0x198))) + rf(m, G + 0x1a8));
                m.set_f32((ebp - 0x78) as u32, (t72).to_f32());
                let t73 = ((((t69 * rf(m, G + 0x17c)) + (t70 * rf(m, G + 0x18c))) + (t71 * rf(m, G + 0x19c))) + rf(m, G + 0x1ac));
                m.set_f32((ebp - 0x74) as u32, (t73).to_f32());
                let t74 = (((t71 * rf(m, G + 0x1a0)) + ((t70 * rf(m, G + 0x190)) + (t69 * rf(m, G + 0x180)))) + rf(m, G + 0x1b0));
                m.set_f32((ebp - 0x70) as u32, (t74).to_f32());
                m.set_f32((ebp - 0x78) as u32, ((((rf(m, B + 0x178) * t72) + (rf(m, B + 0x17c) * t73)) + (rf(m, B + 0x180) * t74))).to_f32());
                m.set_f32((ebp - 0x74) as u32, ((((rf(m, B + 0x188) * t72) + (rf(m, B + 0x18c) * t73)) + (rf(m, B + 0x190) * t74))).to_f32());
                phi39 = (t73 * rf(m, B + 0x19c));
                phi40 = (t72 * rf(m, B + 0x198));
                phi41 = (t74 * rf(m, B + 0x1a0));
                m.set_f32((ebp - 0x70) as u32, (((phi39 + phi40) + phi41)).to_f32());
            }
        }
        m.set_f32((R0 + 0x48) as u32, ((((rf(m, JV + 0x224) * L!(0x74)) + (rf(m, JV + 0x220) * L!(0x78))) + (rf(m, JV + 0x228) * L!(0x70)))).to_f32());
        m.set_f32((R0 + 0x4c) as u32, ((((rf(m, JV + 0x22c) * L!(0x78)) + (rf(m, JV + 0x230) * L!(0x74))) + (rf(m, JV + 0x234) * L!(0x70)))).to_f32());
        m.set_f32((R0 + 0x50) as u32, ((((L!(0x74) * rf(m, JV + 0x23c)) + (L!(0x78) * rf(m, JV + 0x238))) + (L!(0x70) * rf(m, JV + 0x240)))).to_f32());
    }
    m.set_f32((CI + 0x2c) as u32, ((L!(0x3c) + L!(0x48))).to_f32());
    m.set_f32((CI + 0x30) as u32, ((L!(0x38) + L!(0x44))).to_f32());
    let v34 = L!(0x34) + L!(0x40);
    m.set_i32((CI + 0x28) as u32, 0);
    m.set_i32((CI + 0x24) as u32, 0);
    m.set_i32((CI + 0x20) as u32, 0);
    m.set_f32((CI + 0x34) as u32, v34.to_f32());
}
