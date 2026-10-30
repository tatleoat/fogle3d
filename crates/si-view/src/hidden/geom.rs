// SPDX-License-Identifier: MIT
//! The geometry of `hidden.c`: the normal of a triangle, the point of a vector in the
//! fixed-point screen space, where two edges cross and on which side of a plane a point
//! lies. Scratch `static` locals of the original are written at their addresses.

use crate::outside::{self, Outside};
use si_mem::Mem;
use si_x87::{Round, F80};

/// `Error` (0x086bafe0): the hidden-line code's error word; `fp` (0x086bafe4): the stream
/// it prints to.
pub const ERROR: u32 = 0x086b_afe0;
pub const FP: u32 = 0x086b_afe4;
const MSG_VCT_X_MAT: u32 = 0x086b_b380;

fn f(m: &Mem, a: u32) -> F80 {
    F80::from_f32(m.f32(a))
}

fn d(m: &Mem, a: u32) -> F80 {
    F80::from_f64(m.f64(a))
}

/// Stores a register as a `double`.
fn put_d(m: &mut Mem, a: u32, v: F80) {
    m.set_f64(a, v.to_f64());
}

/// A point times a 4 x 4 matrix of floats, divided by the fourth coordinate and scaled to
/// the fixed-point screen space (2^29): three integers truncated toward zero
/// [soft 0x08344fd0 vctXMat]. `v` is a vector of three floats, `mat` sixteen floats, `out`
/// three longs. When the fourth coordinate is no more than 1e-5 in size the routine prints
/// a message to `fp` and sets `Error` to 4 and leaves `out` alone.
pub fn vct_x_mat(m: &mut Mem, o: &mut dyn Outside, v: u32, mat: u32, out: u32) {
    let (x, y, z) = (f(m, v), f(m, v + 4), f(m, v + 8));
    let col = |m: &Mem, c: u32| ((x * f(m, mat + 4 * c) + y * f(m, mat + 16 + 4 * c)) + z * f(m, mat + 32 + 4 * c)) + f(m, mat + 48 + 4 * c);
    let t1 = col(m, 0);
    put_d(m, 0x087f_b7c0, t1);
    let t_y = col(m, 1);
    put_d(m, 0x087f_b7c8, t_y);
    let t_z = col(m, 2);
    put_d(m, 0x087f_b7d0, t_z);
    let w = col(m, 3);
    put_d(m, 0x087f_b7d8, w);
    if w.abs().le(F80::from_f64(1e-5)) || w.abs().unordered(F80::from_f64(1e-5)) {
        let fp = m.u32(FP);
        o.call(m, &outside::FPRINTF, &[fp, MSG_VCT_X_MAT]);
        m.set_i32(ERROR, 4);
        return;
    }
    let t3 = F80::from_i32(1) / w;
    put_d(m, 0x087f_b7e0, t3);
    let k = F80::from_f64(536870912.0);
    m.set_i32(out, ((t1 * t3) * k).to_i32(Round::Zero));
    let ym = d(m, 0x087f_b7c8);
    m.set_i32(out + 4, ((ym * t3) * k).to_i32(Round::Zero));
    let zm = d(m, 0x087f_b7d0);
    m.set_i32(out + 8, (k * (t3 * zm)).to_i32(Round::Zero));
}

/// The unit normal of the triangle `p1 p2 p3` (three longs each) as three doubles at
/// `out`; `out + 0x10` gets the third component [soft 0x0833f478 triNrmLong]. When the
/// cross product is no longer than 1e-5 the first two components are zeroed as four
/// 32-bit words (the second double only in its upper words, as the original does) and the
/// third is -1.0.
pub fn tri_nrm_long(m: &mut Mem, p1: u32, p2: u32, p3: u32, out: u32) {
    let mut ab = [0i32; 6];
    for k in 0..3u32 {
        ab[k as usize] = m.i32(p2 + 4 * k).wrapping_sub(m.i32(p1 + 4 * k));
        ab[3 + k as usize] = m.i32(p3 + 4 * k).wrapping_sub(m.i32(p1 + 4 * k));
    }
    let g = |i: usize| F80::from_i32(ab[i]);
    for k in 0..3u32 {
        put_d(m, 0x087f_b518 + 8 * k, g(k as usize));
    }
    for k in 0..3u32 {
        put_d(m, 0x087f_b530 + 8 * k, g(3 + k as usize));
    }
    let (ax, ay, az, bx, by, bz) = (g(0), g(1), g(2), g(3), g(4), g(5));
    let t2 = az * bx - ax * bz;
    put_d(m, 0x087f_b550, t2);
    let t3 = by * ax - bx * ay;
    put_d(m, 0x087f_b558, t3);
    let t1 = ay * bz - az * by;
    put_d(m, 0x087f_b548, t1);
    let t4 = (t3 * t3 + (t2 * t2 + t1 * t1)).sqrt();
    put_d(m, 0x087f_b510, t4);
    let z;
    if t4.abs().gt(F80::from_f64(1e-5)) {
        let t5 = F80::from_i32(1) / t4;
        put_d(m, 0x087f_b510, t5);
        put_d(m, out, t1 * t5);
        let y = d(m, 0x087f_b550);
        put_d(m, out + 8, y * t5);
        z = t5 * d(m, 0x087f_b558);
    } else {
        m.set_u32(out + 8, 0);
        m.set_u32(out + 0xc, 0);
        m.set_u32(out, 0);
        m.set_u32(out + 4, 0);
        z = F80::from_f64(-1.0);
    }
    put_d(m, out + 0x10, z);
}

/// The unit normal of the triangle `p1 p2 p3` (three floats each) in `out`, and whether it
/// has a component longer than 1e-5 [soft 0x0833f5d4 triangleOrgNrm]. When the cross
/// product is no longer than 1e-5 nothing is written to `out` and the answer is 0.
pub fn triangle_org_nrm(m: &mut Mem, p1: u32, p2: u32, p3: u32, out: u32) -> u32 {
    let t1 = f(m, p2) - f(m, p1);
    put_d(m, 0x087f_b568, t1);
    let t2 = f(m, p2 + 4) - f(m, p1 + 4);
    put_d(m, 0x087f_b570, t2);
    let t3 = f(m, p2 + 8) - f(m, p1 + 8);
    put_d(m, 0x087f_b578, t3);
    let t4 = f(m, p3) - f(m, p1);
    put_d(m, 0x087f_b580, t4);
    let t5 = f(m, p3 + 4) - f(m, p1 + 4);
    put_d(m, 0x087f_b588, t5);
    let t6 = f(m, p3 + 8) - f(m, p1 + 8);
    put_d(m, 0x087f_b590, t6);
    let t7 = t4 * t3 - t1 * t6;
    put_d(m, 0x087f_b5a0, t7);
    let t8 = t5 * t1 - t2 * t4;
    put_d(m, 0x087f_b5a8, t8);
    let tx = t2 * t6 - t3 * t5;
    put_d(m, 0x087f_b598, tx);
    let t9 = (t8 * t8 + (t7 * t7 + tx * tx)).sqrt();
    put_d(m, 0x087f_b560, t9);
    let k = F80::from_f64(1e-5);
    if !t9.abs().gt(k) {
        return 0;
    }
    let t10 = F80::from_i32(1) / t9;
    put_d(m, 0x087f_b560, t10);
    let o0 = (tx * t10).to_f32();
    m.set_f32(out, o0);
    let o1 = (d(m, 0x087f_b5a0) * t10).to_f32();
    m.set_f32(out + 4, o1);
    let o2 = (t10 * d(m, 0x087f_b5a8)).to_f32();
    m.set_f32(out + 8, o2);
    let big = |v: f32| {
        let a = F80::from_f32(v).abs();
        a.gt(k) || a.unordered(k)
    };
    (big(o0) || big(o1) || big(o2)) as u32
}

/// Where the edge `p1[0]..p1[1]` / `p2[0]..p2[1]` (the x and y of an edge's end points as
/// longs) meets the edge of the triangle `p3 p4` between corner `idx` and the next one
/// (mod 3); the two parameters go to `out_a` and `out_b` as doubles. 1 when the
/// determinant is larger than 1e-5, else 0 [soft 0x0833f728 edgeIntersection].
pub fn edge_intersection(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, idx: i32, out_a: u32, out_b: u32) -> u32 {
    let a0 = m.i32(p1 + 4).wrapping_sub(m.i32(p1));
    let b0 = m.i32(p2 + 4).wrapping_sub(m.i32(p2));
    let j = (idx.wrapping_add(1) % 3) as u32;
    let ix = idx as u32;
    let a1 = m.i32(p3.wrapping_add(4 * ix)).wrapping_sub(m.i32(p3.wrapping_add(4 * j)));
    let b1 = m.i32(p4.wrapping_add(4 * ix)).wrapping_sub(m.i32(p4.wrapping_add(4 * j)));
    let x = m.i32(p3.wrapping_add(4 * ix)).wrapping_sub(m.i32(p1));
    let y = m.i32(p4.wrapping_add(4 * ix)).wrapping_sub(m.i32(p2));
    let fi = F80::from_i32;
    put_d(m, 0x087f_b5c8, fi(a0));
    put_d(m, 0x087f_b5d8, fi(b0));
    put_d(m, 0x087f_b5d0, fi(a1));
    put_d(m, 0x087f_b5e0, fi(b1));
    put_d(m, 0x087f_b5e8, fi(x));
    put_d(m, 0x087f_b5f0, fi(y));
    let t1 = fi(a0) * fi(b1) - fi(b0) * fi(a1);
    put_d(m, 0x087f_b5c0, t1);
    if !t1.abs().gt(F80::from_f64(1e-5)) {
        return 0;
    }
    let t2 = fi(b1) * fi(x) - fi(a1) * fi(y);
    put_d(m, 0x087f_b5b0, t2);
    let num1 = fi(a0) * fi(y) - fi(x) * fi(b0);
    put_d(m, 0x087f_b5b8, num1);
    let inv = fi(1) / t1;
    put_d(m, 0x087f_b5c0, inv);
    put_d(m, out_a, t2 * inv);
    let r = d(m, 0x087f_b5b8) * d(m, 0x087f_b5c0);
    put_d(m, out_b, r);
    1
}

/// Classifies two signed distances (the x and the y components of a weighted sum of
/// differences, scaled by 2^-29) against 1e-7: 0 when the first is not below -1e-7 and the
/// second not below it either, 1 when the first is negative and the second above 1e-7,
/// 2 when the first is above 1e-7 and the second negative, else 3 [soft 0x08343c70 edgeTestVertices].
#[allow(clippy::too_many_arguments)]
pub fn edge_test_vertices(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, n0: f64, n1: f64, n2: f64) -> u32 {
    let k = F80::from_f64(1.862645149230957e-09);
    let (n0, n1, n2) = (F80::from_f64(n0), F80::from_f64(n1), F80::from_f64(n2));
    let e = |m: &Mem, a: u32, b: u32| F80::from_i32(m.i32(a).wrapping_sub(m.i32(b)));
    let (a, b, c) = (e(m, p1, p4), e(m, p2, p5), e(m, p3, p6));
    let t1 = ((c * k) * n2) + (((a * k) * n0) + ((b * k) * n1));
    put_d(m, 0x087f_b728, t1);
    let (a, b, c) = (e(m, p1 + 4, p4), e(m, p2 + 4, p5), e(m, p3 + 4, p6));
    let t2 = ((k * c) * n2) + ((n1 * (b * k)) + ((a * k) * n0));
    put_d(m, 0x087f_b730, t2);
    let lo = F80::from_f64(-1e-7);
    let hi = F80::from_f64(1e-7);
    let zero = F80::from_i32(0);
    // the first test is made on the registers, the later ones on the doubles stored
    if t1.lt(lo) || t1.unordered(lo) || lo.gt(t2) || lo.unordered(t2) {
        let d0 = d(m, 0x087f_b728);
        let d1 = d(m, 0x087f_b730);
        if d0.lt(zero) && hi.lt(d1) {
            1
        } else if hi.lt(d0) && d1.lt(zero) {
            2
        } else {
            3
        }
    } else {
        0
    }
}

/// Where the line through the point `p4 p5 p6`-minus-`p1 p2 p3` meets the plane of a
/// triangle, as a parameter stored at `out` [soft 0x0833fc5c edgInterTri2]. The nine
/// differences of the coordinates, the three offsets and the determinant are exact in the
/// extended registers; they are also kept as doubles in the routine's statics. When the
/// determinant is no more than 1e-5 in size three messages go to `fp` and `Error` is 3.
#[allow(clippy::too_many_arguments)]
pub fn edg_inter_tri2(m: &mut Mem, o: &mut dyn Outside, p1: u32, p2: u32, p3: u32, p4: u32, p5: u32, p6: u32, out: u32) {
    let df = |m: &Mem, a: u32, b: u32| F80::from_i32(m.i32(a).wrapping_sub(m.i32(b)));
    let a0 = df(m, p1 + 4, p1);
    put_d(m, 0x087f_b688, a0);
    let b0 = df(m, p2 + 4, p2);
    put_d(m, 0x087f_b6a0, b0);
    let c0 = df(m, p3 + 4, p3);
    put_d(m, 0x087f_b6b8, c0);
    let a1 = df(m, p4, p4 + 4);
    put_d(m, 0x087f_b690, a1);
    let b1 = df(m, p5, p5 + 4);
    put_d(m, 0x087f_b6a8, b1);
    let c1 = df(m, p6, p6 + 4);
    put_d(m, 0x087f_b6c0, c1);
    let a2 = df(m, p4, p4 + 8);
    put_d(m, 0x087f_b698, a2);
    let b2 = df(m, p5, p5 + 8);
    put_d(m, 0x087f_b6b0, b2);
    let c2 = df(m, p6, p6 + 8);
    put_d(m, 0x087f_b6c8, c2);
    let x = df(m, p4, p1);
    put_d(m, 0x087f_b6d0, x);
    let y = df(m, p5, p2);
    put_d(m, 0x087f_b6d8, y);
    let z = df(m, p6, p3);
    put_d(m, 0x087f_b6e0, z);
    let denom = ((((a2 * b0) * c1 + ((a0 * b1) * c2 + (a1 * b2) * c0)) - (c0 * (a2 * b1))) - ((b0 * a1) * c2)) - ((a0 * b2) * c1);
    put_d(m, 0x087f_b680, denom);
    let lim = F80::from_f64(1e-5);
    if denom.abs().le(lim) || denom.abs().unordered(lim) {
        let fp = m.u32(FP);
        o.call(m, &outside::FPRINTF, &[fp, 0x086b_b0e0]);
        o.call(m, &outside::FPRINTF, &[fp, 0x086b_b115]);
        o.call(m, &outside::FPRINTF, &[fp, 0x086b_b140]);
        m.set_i32(ERROR, 3);
        return;
    }
    let num = (((((a2 * y) * c1) + (((a1 * b2) * z) + ((x * b1) * c2))) - ((a2 * b1) * z)) - ((a1 * y) * c2)) - ((x * b2) * c1);
    put_d(m, 0x087f_b678, num);
    put_d(m, out, num / denom);
}
