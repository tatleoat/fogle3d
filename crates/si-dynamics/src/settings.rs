// SPDX-License-Identifier: MIT
//! `settings.c` of Softimage|3D 4.0 (0x0849e0f0 … 0x084a1794): the setup of the dynamics records of a model chain.
//! This file holds the matrix and joint updates the simulation step calls and the velocity of a body point.
//!
//! Sources are functions of the Linux `soft` binary, cited as `[soft 0xADDRESS name]`. Float code was compiled for
//! the x87 unit: sums are formed in 80 bits and rounded to f32 at the store; a value stored with `fst` stays
//! unrounded in the register.

use si_math::matrix3tools::{matrix3_copy, matrix3_multiply, matrix3_transpose};
use si_mem::Mem;
use si_x87::{x, F80};

fn read_m3(m: &Mem, a: u32) -> si_math::types::Matrix3 {
    let mut o = [[0.0f32; 3]; 3];
    for (i, row) in o.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = m.f32(a + 4 * (3 * i + j) as u32);
        }
    }
    o
}

fn write_m3(m: &mut Mem, a: u32, v: &si_math::types::Matrix3) {
    for (i, row) in v.iter().enumerate() {
        for (j, c) in row.iter().enumerate() {
            m.set_f32(a + 4 * (3 * i + j) as u32, *c);
        }
    }
}

fn f(m: &Mem, a: u32) -> F80 {
    x(m.f32(a))
}

/// The model itself, or its first link when it has a parent link (`+0x30` clear): the head of a dynamics chain.
fn chain_head(m: &Mem, model: u32) -> u32 {
    if m.u32(model + 0x30) == 0 {
        m.u32(model + 0x2c4)
    } else {
        model
    }
}

/// The velocity of a point of a body [soft 0x0849f0ac si_dynaLinearSpeedCompute]: with `s = [model+0x2f0]` (state) and
/// `r = [model+0x300]` (record), the point `(px, py, pz)` is carried to `w = R * s[0x48..0x50]` (the 3 x 3 at `r+0x1d8`
/// applied to the angular velocity, sums of three products in 80 bits) and the output is `w x p + s[0x30..0x38]`, each
/// component rounded to f32 once. The cross product is formed from the unrounded sums.
pub fn si_dyna_linear_speed_compute(m: &mut Mem, model: u32, px: f32, py: f32, pz: f32, out: u32) {
    let s = m.u32(model + 0x2f0);
    let r = m.u32(model + 0x300);
    let (a, b, c) = (f(m, s + 0x48), f(m, s + 0x4c), f(m, s + 0x50));
    let t1 = (f(m, r + 0x1dc) * b + f(m, r + 0x1d8) * a) + f(m, r + 0x1e0) * c;
    let t2 = (f(m, r + 0x1e4) * a + f(m, r + 0x1e8) * b) + f(m, r + 0x1ec) * c;
    let t3 = c * f(m, r + 0x1f8) + (b * f(m, r + 0x1f4) + a * f(m, r + 0x1f0));
    let (xx, yy, zz) = (x(px), x(py), x(pz));
    let t4 = t2 * zz - t3 * yy;
    let t5 = yy * t1 - t2 * xx;
    let t6 = xx * t3 - zz * t1;
    m.set_f32(out, (t4 + f(m, s + 0x30)).to_f32());
    let s = m.u32(model + 0x2f0);
    m.set_f32(out + 4, (t6 + f(m, s + 0x34)).to_f32());
    let s = m.u32(model + 0x2f0);
    m.set_f32(out + 8, (t5 + f(m, s + 0x38)).to_f32());
}

/// The relative to absolute update of a chain's rotation matrices [soft 0x0849fe94 si_modelDynaRel2AbsMatrixUpdate]:
/// for the head `p` of the chain (see [`chain_head`]) with record `r = [p+0x300]`: `r+0x1fc` is the transpose of
/// `r+0x1b4`, `r+0x1d8` and `r+0x220` copies of `r+0x1b4` and `r+0x1fc`; then every later model of the chain (next
/// link `+0x30c`) with record `q` gets `q+0x1d8 = parent(q+0x25c)'s 0x1d8 times q+0x1b4` and `q+0x220` the transpose of
/// the new `q+0x1d8`.
pub fn si_model_dyna_rel2abs_matrix_update(m: &mut Mem, model: u32) {
    let p = chain_head(m, model);
    let r = m.u32(p + 0x300);
    let t = matrix3_transpose(&read_m3(m, r + 0x1b4));
    write_m3(m, r + 0x1fc, &t);
    let c = matrix3_copy(&read_m3(m, r + 0x1b4));
    write_m3(m, r + 0x1d8, &c);
    let c = matrix3_copy(&read_m3(m, r + 0x1fc));
    write_m3(m, r + 0x220, &c);
    let mut n = m.u32(p + 0x30c);
    while n != 0 {
        let q = m.u32(n + 0x300);
        let par = m.u32(q + 0x25c);
        let pr = m.u32(par + 0x300);
        let prod = matrix3_multiply(&read_m3(m, pr + 0x1d8), &read_m3(m, q + 0x1b4));
        write_m3(m, q + 0x1d8, &prod);
        let t = matrix3_transpose(&read_m3(m, q + 0x1d8));
        write_m3(m, q + 0x220, &t);
        n = m.u32(n + 0x30c);
    }
}

/// The absolute to relative update [soft 0x0849ff5c si_modelDynaAbs2RelMatrixUpdate]: for the head `p` with record `r`:
/// `r+0x1b4` is a copy of `r+0x1d8` and `r+0x1fc` its transpose. Then for every model `n` of the chain (the head
/// first, next link `+0x30c`) with record `q`: `q+0x220` is the transpose of `q+0x1d8`; and for each of its bodies
/// (first `[q+0x260]`, next `[rec+0x264]`) of kind 5 with `+0x2ec == 2` and record `c`: `c+0x1b4 = q+0x220 times c+0x1d8`
/// and `c+0x1fc` its transpose.
pub fn si_model_dyna_abs2rel_matrix_update(m: &mut Mem, model: u32) {
    let p = chain_head(m, model);
    let r = m.u32(p + 0x300);
    let c = matrix3_copy(&read_m3(m, r + 0x1d8));
    write_m3(m, r + 0x1b4, &c);
    let t = matrix3_transpose(&read_m3(m, r + 0x1b4));
    write_m3(m, r + 0x1fc, &t);
    let mut n = p;
    while n != 0 {
        let q = m.u32(n + 0x300);
        let t = matrix3_transpose(&read_m3(m, q + 0x1d8));
        write_m3(m, q + 0x220, &t);
        let mut ch = if m.u32(n + 0x300) != 0 { m.u32(q + 0x260) } else { 0 };
        while ch != 0 {
            if m.u32(ch + 0x30) == 5 && m.u32(ch + 0x2ec) == 2 {
                let c = m.u32(ch + 0x300);
                let prod = matrix3_multiply(&read_m3(m, q + 0x220), &read_m3(m, c + 0x1d8));
                write_m3(m, c + 0x1b4, &prod);
                let t = matrix3_transpose(&read_m3(m, c + 0x1b4));
                write_m3(m, c + 0x1fc, &t);
            }
            let cr = m.u32(ch + 0x300);
            ch = if cr != 0 { m.u32(cr + 0x264) } else { 0 };
        }
        n = m.u32(n + 0x30c);
    }
}

/// The joints of a chain after a step [soft 0x084a0080 si_modelDynaJointsUpdate]: the model's point `a = [model+0x158..
/// 0x160]` goes to `model+0x1b8..0x1c0`: with a parent record `e = [model+0x2bc]` it is `a` carried through the 3 x 4 matrix
/// `e+0x1c4..0x1fc` (each component `((b*m1 + a*m0) + c*m2) + m3`, rounded once), without it a plain copy. Then for the head
/// `p` of the chain and each following model (next link `+0x30c`), with record `r`, for every body of kind 5 with
/// `+0x2ec == 2` (record `c`): `w = r+0x1d8..0x1f8 (3 x 3) applied to c+0x250..0x258`; `c+0x244.. = r+0x244.. + w` (the first
/// component from the unrounded `w0`, the others from the rounded floats) and the body's speed `+0x2f0 + 0x30` is
/// `si_dynaLinearSpeedCompute(p, w)`.
pub fn si_model_dyna_joints_update(m: &mut Mem, model: u32) {
    let e = m.u32(model + 0x2bc);
    if e != 0 {
        let (a, b, c) = (f(m, model + 0x158), f(m, model + 0x15c), f(m, model + 0x160));
        let o0 = ((b * f(m, e + 0x1d4) + a * f(m, e + 0x1c4)) + c * f(m, e + 0x1e4)) + f(m, e + 0x1f4);
        let o1 = ((b * f(m, e + 0x1d8) + a * f(m, e + 0x1c8)) + c * f(m, e + 0x1e8)) + f(m, e + 0x1f8);
        let o2 = ((b * f(m, e + 0x1dc) + a * f(m, e + 0x1cc)) + c * f(m, e + 0x1ec)) + f(m, e + 0x1fc);
        m.set_f32(model + 0x1b8, o0.to_f32());
        m.set_f32(model + 0x1bc, o1.to_f32());
        m.set_f32(model + 0x1c0, o2.to_f32());
    } else {
        for k in 0..3 {
            let w = m.u32(model + 0x158 + 4 * k);
            m.set_u32(model + 0x1b8 + 4 * k, w);
        }
    }
    let mut n = chain_head(m, model);
    while n != 0 {
        let r = m.u32(n + 0x300);
        let mut ch = if r != 0 { m.u32(r + 0x260) } else { 0 };
        while ch != 0 {
            if m.u32(ch + 0x30) == 5 && m.u32(ch + 0x2ec) == 2 {
                let c = m.u32(ch + 0x300);
                let (a, b, cc) = (f(m, c + 0x250), f(m, c + 0x254), f(m, c + 0x258));
                let t1 = (f(m, r + 0x1dc) * b + f(m, r + 0x1d8) * a) + f(m, r + 0x1e0) * cc;
                let t2 = (f(m, r + 0x1e4) * a + f(m, r + 0x1e8) * b) + f(m, r + 0x1ec) * cc;
                let t3 = cc * f(m, r + 0x1f8) + (b * f(m, r + 0x1f4) + a * f(m, r + 0x1f0));
                let (w0, w1, w2) = (t1.to_f32(), t2.to_f32(), t3.to_f32());
                m.set_f32(c + 0x244, (t1 + f(m, r + 0x244)).to_f32());
                m.set_f32(c + 0x248, (f(m, r + 0x248) + x(w1)).to_f32());
                m.set_f32(c + 0x24c, (f(m, r + 0x24c) + x(w2)).to_f32());
                let st = m.u32(ch + 0x2f0);
                si_dyna_linear_speed_compute(m, n, w0, w1, w2, st + 0x30);
            }
            let cr = m.u32(ch + 0x300);
            ch = if cr != 0 { m.u32(cr + 0x264) } else { 0 };
        }
        n = m.u32(n + 0x30c);
    }
}

/// The time step of a chain's settings record [soft 0x0849e0f0 si_dynachnDeltatCompute]: the smallest division of one
/// frame (`1 / SI_frameRate`, rounded to a float) into `n = 1, 2, ...` equal parts whose length `dt = frame / n` is
/// below the constant at 0x08660ae8 (compared in 80 bits); `vars+0xc` gets `dt` as a float, `vars+0x10` the count `n`
/// (a 16-bit counter, converted as signed like the original's `fild`).
pub fn si_dynachn_deltat_compute(m: &mut Mem, vars: u32) {
    let frame = (F80::ONE / x(m.f32(0x0878_204c))).to_f32();
    let limit = x(m.f64(0x0866_0ae8));
    let mut n: u16 = 0;
    let dt = loop {
        n = n.wrapping_add(1);
        let dt = x(frame) / F80::from_i32(n as i16 as i32);
        // the loop goes on while the limit is not above... `limit <= dt` (ordered, equal included)
        if !(dt.ge(limit)) {
            break dt;
        }
    };
    m.set_f32(vars + 0xc, dt.to_f32());
    m.set_u16(vars + 0x10, n);
}

/// The factors of the inertia tensor of a box [soft 0x0849e13c inertiaTensorElemBoxFactors]: corners `lo = (x0, y0, z0)`
/// and `hi = (x1, y1, z1)` (a side shorter than 0.0001 is widened to 0.005 first, in the callee's copy of the argument),
/// six outputs in the order of the original's pointer arguments (`out[0]` .. `out[5]`).
pub fn inertia_tensor_elem_box_factors(m: &mut Mem, lo: [f32; 3], hi: [f32; 3], out: [u32; 6]) {
    let (x0, y0, z0) = (lo[0], lo[1], lo[2]);
    let (mut x1, mut y1, mut z1) = (hi[0], hi[1], hi[2]);
    let c1 = x(m.f64(0x0866_0af0));
    let c2 = x(m.f64(0x0866_0af8));
    let c3 = x(m.f64(0x0866_0b00));
    let c4 = x(m.f64(0x0866_0b08));
    if (x(x1) - x(x0)).lt(c1) {
        x1 = (x(x0) + c2).to_f32();
    }
    if (x(y1) - x(y0)).lt(c1) {
        y1 = (x(y0) + c2).to_f32();
    }
    if (x(z1) - x(z0)).lt(c1) {
        z1 = (x(z0) + c2).to_f32();
    }
    let (fx0, fy0, fz0, fx1, fy1, fz1) = (x(x0), x(y0), x(z0), x(x1), x(y1), x(z1));
    let l30 = (fz0 * fz0).to_f32();
    let l14 = (fy1 - fy0).to_f32();
    let l18 = (fz1 - fz0).to_f32();
    let t2 = fy1 * fy1;
    let l1c = (t2 - fy0 * fy0).to_f32();
    let t3 = fz1 * fz1;
    let l20 = (t3 - x(l30)).to_f32();
    let t1 = fx1 * fx1;
    let (l14x, l18x, l1cx, l20x) = (x(l14), x(l18), x(l1c), x(l20));
    m.set_f32(out[0], ((((t1 * fx1 - (fx0 * fx0) * fx0) * l14x) * l18x) * c3).to_f32());
    m.set_f32(out[1], ((((t2 * fy1 - (fy0 * fy0) * fy0) * (fx1 - fx0)) * l18x) * c3).to_f32());
    m.set_f32(out[2], ((((t3 * fz1 - x(l30) * fz0) * (fx1 - fx0)) * l14x) * c3).to_f32());
    m.set_f32(out[3], (((-(fx1 - fx0) * l1cx) * l20x) * c4).to_f32());
    let t4 = -(t1 - fx0 * fx0);
    m.set_f32(out[4], (((t4 * l14x) * l20x) * c4).to_f32());
    m.set_f32(out[5], (c4 * ((t4 * l1cx) * l18x)).to_f32());
}

/// The inertia tensor of a box of the density `scale` [soft 0x0849e2d8 inertiaTensorElemBox]: nine floats at `out`
/// (rows of three, symmetric) from [`inertia_tensor_elem_box_factors`], then every entry with `0 < v < 0.5` is raised to
/// 0.5 and every `-0.5 < v < 0` lowered to -0.5.
pub fn inertia_tensor_elem_box(m: &mut Mem, lo: [f32; 3], hi: [f32; 3], scale: f32, out: u32) {
    let fr = m.local(0x40);
    let l = |k: u32| fr + 0x40 - k;
    inertia_tensor_elem_box_factors(m, lo, hi, [l(0x28), l(0x24), l(0x20), l(0x1c), l(0x18), l(0x14)]);
    let s = x(scale);
    let g = |m: &Mem, k: u32| x(m.f32(l(k)));
    let (a, b, c, d, e, f_) = (g(m, 0x28), g(m, 0x24), g(m, 0x20), g(m, 0x1c), g(m, 0x18), g(m, 0x14));
    m.set_f32(out, (s * (b + c)).to_f32());
    m.set_f32(out + 0x10, ((c + a) * s).to_f32());
    m.set_f32(out + 0x20, ((b + a) * s).to_f32());
    let t1 = (s * f_).to_f32();
    m.set_f32(out + 0xc, t1);
    m.set_f32(out + 4, t1);
    let t2 = (s * e).to_f32();
    m.set_f32(out + 0x18, t2);
    m.set_f32(out + 8, t2);
    let t3 = (s * d).to_f32();
    m.set_f32(out + 0x1c, t3);
    m.set_f32(out + 0x14, t3);
    m.release(0x40);
    for k in 0..9u32 {
        let p = out + 4 * k;
        let v = m.f32(p);
        if 0.0 < v && x(v).lt(x(0.5f64)) {
            m.set_f32(p, 0.5);
        }
        let v = m.f32(p);
        if v < 0.0 && x(v).gt(x(-0.5f64)) {
            m.set_f32(p, -0.5);
        }
    }
}

/// The parallel-axis shift of an inertia tensor [soft 0x0849e438 inertiaTensorTranslate]: `out = a + sign * s * (|d|^2 I - d d^T)`
/// for the offset `d = (x, y, z)` and the mass `s`; `sign` is -1 when `flag` is 0 (the constant at 0x08660b2c), else +1.
pub fn inertia_tensor_translate(m: &mut Mem, a: u32, s: f32, d: [f32; 3], flag: i16, out: u32) {
    let phi = if flag == 0 { x(m.f32(0x0866_0b2c)) } else { F80::ONE };
    let t2 = phi * x(s);
    let (px, py, pz) = (x(d[0]), x(d[1]), x(d[2]));
    let t1 = py * py;
    let g = |m: &Mem, k: u32| x(m.f32(a + k));
    m.set_f32(out, ((t2 * (t1 + pz * pz)) + g(m, 0)).to_f32());
    let t3 = px * px;
    m.set_f32(out + 0x10, (((pz * pz + t3) * t2) + g(m, 0x10)).to_f32());
    m.set_f32(out + 0x20, (((t1 + t3) * t2) + g(m, 0x20)).to_f32());
    let t4 = px * t2;
    let t5 = (t4 * py) + g(m, 4);
    m.set_f32(out + 4, t5.to_f32());
    let t6 = (t4 * pz) + g(m, 8);
    m.set_f32(out + 8, t6.to_f32());
    m.set_f32(out + 0xc, t5.to_f32());
    m.set_f32(out + 0x18, t6.to_f32());
    let t7 = (pz * (t2 * py)) + g(m, 0x14);
    m.set_f32(out + 0x14, t7.to_f32());
    m.set_f32(out + 0x1c, t7.to_f32());
}
