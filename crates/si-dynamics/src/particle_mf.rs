// SPDX-License-Identifier: MIT
//! `particle_mf.c` of Softimage|3D 4.0 (0x084c4590 … 0x084c4a4f): the vector and 4 x 4 matrix helpers of the
//! particle code (finding 0045). Floats are x87 values: sums and products keep 80 bits, stores round to f32.
//!
//! The random jitter matrices call the C library's `drand48`; [`Drand`] stands for it.

use si_mem::Mem;
use si_x87::{trig, x, F80};

/// The C library's `drand48` (the generator the particle jitter calls; not the program's own `drand48.c`).
pub trait Drand {
    fn drand48(&mut self) -> f64;
    /// `srand48(seed)`.
    fn srand48(&mut self, seed: u32);
}

/// The library generator: X <- 0x5DEECE66D * X + 0xB (mod 2^48), result X / 2^48.
#[derive(Clone, Copy, Debug)]
pub struct Lcg48(pub u64);

impl Lcg48 {
    /// `srand48(seed)`: the high 32 bits are the seed, the low 16 are 0x330e.
    pub fn seeded(seed: i32) -> Lcg48 {
        Lcg48(((seed as u32 as u64) << 16) | 0x330e)
    }
}

impl Drand for Lcg48 {
    fn drand48(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(0x5DEE_CE66D).wrapping_add(0xB) & 0xffff_ffff_ffff;
        self.0 as f64 / 281_474_976_710_656.0
    }
    fn srand48(&mut self, seed: u32) {
        *self = Lcg48::seeded(seed as i32);
    }
}

fn f(m: &Mem, a: u32) -> F80 {
    x(m.f32(a))
}

/// Dot product of two 3-vectors [soft 0x084c4590 si_particleDotProduct]: `(a0*b0 + a1*b1) + a2*b2`, left in st0 (80 bits).
pub fn si_particle_dot_product(m: &Mem, a: u32, b: u32) -> F80 {
    (f(m, a) * f(m, b) + f(m, a + 4) * f(m, b + 4)) + f(m, a + 8) * f(m, b + 8)
}

/// Cross product a x b into `out` [soft 0x084c45b8 si_particleCrossProduct]; each component is one rounded store, and
/// `out` may overlap an input only as far as the original's order of stores allows (x, then y, then z).
pub fn si_particle_cross_product(m: &mut Mem, a: u32, b: u32, out: u32) {
    let v = f(m, a + 4) * f(m, b + 8) - f(m, a + 8) * f(m, b + 4);
    m.set_f32(out, v.to_f32());
    let v = f(m, a + 8) * f(m, b) - f(m, a) * f(m, b + 8);
    m.set_f32(out + 4, v.to_f32());
    let v = f(m, a) * f(m, b + 4) - f(m, a + 4) * f(m, b);
    m.set_f32(out + 8, v.to_f32());
}

/// Length of a 3-vector [soft 0x084c46c0 si_particleNorm]: the square root of the sum of squares
/// (`(x*x + y*y) + z*z`), left in st0 (80 bits).
pub fn si_particle_norm(m: &Mem, a: u32) -> F80 {
    ((f(m, a) * f(m, a) + f(m, a + 4) * f(m, a + 4)) + f(m, a + 8) * f(m, a + 8)).sqrt()
}

/// Normalises a 3-vector in place [soft 0x084c4604 si_particleNormalize]: the length is rounded to f32 and compared with
/// the double 0.0001 (global at 0x08661330); strictly less (not unordered, not equal) gives -1 and leaves the vector,
/// else each component is divided by the length (a rounded store each) and 1 is returned.
pub fn si_particle_normalize(m: &mut Mem, v: u32) -> i32 {
    let len = x(si_particle_norm(m, v).to_f32());
    if len.lt(x(m.f64(0x0866_1330))) {
        return -1;
    }
    for k in 0..3 {
        let q = f(m, v + 4 * k) / len;
        m.set_f32(v + 4 * k, q.to_f32());
    }
    1
}

/// The angle between two 3-vectors [soft 0x084c4660 particleAngle]: the dot product and the two lengths are each rounded to f32, the
/// second length times the first is rounded again, the quotient (80 bits, rounded to a double only for the call) goes
/// to the library's `acos`, and the answer is rounded to f32 (left in st0).
pub fn particle_angle(m: &Mem, a: u32, b: u32) -> F80 {
    let dot = x(si_particle_dot_product(m, a, b).to_f32());
    let na = x(si_particle_norm(m, a).to_f32());
    let prod = (x(si_particle_norm(m, b).to_f32()) * na).to_f32();
    let q = dot / x(prod);
    x(trig::acos(q.to_f64()).to_f32())
}

/// Transforms a point by a 4 x 4 matrix (row vectors, translation in row 3) [soft 0x084c46ec si_particleTransformPoint]:
/// the four results `((m[4i]*x + m[4i+1]*y) + m[4i+2]*z) + m[4i+3]` are rounded to f32 into a local; when the fourth is
/// exactly 1.0 the first three are stored, otherwise each is divided by it.
pub fn si_particle_transform_point(m: &mut Mem, mat: u32, p: u32, out: u32) {
    let (px, py, pz) = (f(m, p), f(m, p + 4), f(m, p + 8));
    let mut l = [0.0f32; 4];
    for (i, v) in l.iter_mut().enumerate() {
        let r = mat + 16 * i as u32;
        *v = (((f(m, r) * px + f(m, r + 4) * py) + f(m, r + 8) * pz) + f(m, r + 12)).to_f32();
    }
    if x(l[3]).eq(x(1.0f32)) {
        for (k, v) in l.iter().take(3).enumerate() {
            m.set_f32(out + 4 * k as u32, *v);
        }
    } else {
        for (k, v) in l.iter().take(3).enumerate() {
            m.set_f32(out + 4 * k as u32, (x(*v) / x(l[3])).to_f32());
        }
    }
}

/// Product of two 4 x 4 matrices [soft 0x084c478c si_particleMatrixMultiply]: result element (i, j) is
/// `((a[i][1]*b[1][j] + a[i][0]*b[0][j]) + a[i][2]*b[2][j]) + a[i][3]*b[3][j]` rounded to f32, built in a local
/// (so `out` may be an input) and copied.
pub fn si_particle_matrix_multiply(m: &mut Mem, a: u32, b: u32, out: u32) {
    let mut l = [0.0f32; 16];
    for i in 0..4u32 {
        for j in 0..4u32 {
            let e = |r: u32, c: u32, m: &Mem| f(m, a + 4 * (4 * i + r)) * f(m, b + 4 * (4 * r + c));
            let v = ((e(1, j, m) + e(0, j, m)) + e(2, j, m)) + e(3, j, m);
            l[(4 * i + j) as usize] = v.to_f32();
        }
    }
    for (k, v) in l.iter().enumerate() {
        m.set_f32(out + 4 * k as u32, *v);
    }
}

/// Sets a 4 x 4 matrix to the identity [soft 0x084c480c si_particleMakeIdentity].
pub fn si_particle_make_identity(m: &mut Mem, a: u32) {
    for k in 0..16 {
        m.set_u32(a + 4 * k, 0);
    }
    for k in 0..4 {
        m.set_f32(a + 4 * (5 * k), 1.0);
    }
}

/// A random scale matrix [soft 0x084c4844 si_particleJitterScaleMatrix]: identity, then one `drand48` (rounded to f32)
/// times each of the three amounts is added to the diagonal.
pub fn si_particle_jitter_scale_matrix(m: &mut Mem, r: &mut dyn Drand, amount: u32, out: u32) {
    si_particle_make_identity(m, out);
    let d = x(r.drand48() as f32);
    for k in 0..3u32 {
        let v = d * f(m, amount + 4 * k) + f(m, out + 20 * k);
        m.set_f32(out + 20 * k, v.to_f32());
    }
}

/// A random translation matrix [soft 0x084c4890 si_particleJitterTranslateMatrix]: identity, then for each axis a fresh
/// `drand48` (rounded to f32) times the amount is added to the fourth element of the row.
pub fn si_particle_jitter_translate_matrix(m: &mut Mem, r: &mut dyn Drand, amount: u32, out: u32) {
    si_particle_make_identity(m, out);
    for k in 0..3u32 {
        let d = x(r.drand48() as f32);
        let v = d * f(m, amount + 4 * k) + f(m, out + 16 * k + 12);
        m.set_f32(out + 16 * k + 12, v.to_f32());
    }
}

/// A random rotation matrix [soft 0x084c48d8 si_particleJitterRotateMatrix]: three rotations, about z (amount[2]),
/// then y (amount[1]) then x (amount[0]), each by `((drand48 as f32) * 6.283 (the double at 0x08661338) * amount)`
/// rounded to f32, through the library's `sin` and `cos` of that value as a double; the results are rounded to f32
/// into the matrix, and the later rotations are multiplied on with [`si_particle_matrix_multiply`].
pub fn si_particle_jitter_rotate_matrix(m: &mut Mem, r: &mut dyn Drand, amount: u32, out: u32) {
    let two_pi = x(m.f64(0x0866_1338));
    let angle = |m: &Mem, r: &mut dyn Drand, k: u32| -> f64 {
        let d = x(r.drand48() as f32);
        ((d * two_pi) * f(m, amount + 4 * k)).to_f32() as f64
    };
    si_particle_make_identity(m, out);
    let a = angle(m, r, 2);
    let (c, s) = (trig::cos(a).to_f32(), trig::sin(a).to_f32());
    m.set_f32(out, c);
    m.set_f32(out + 16, s);
    m.set_f32(out + 4, -s);
    m.set_f32(out + 20, c);
    let tmp = m.local(64);
    si_particle_make_identity(m, tmp);
    let a = angle(m, r, 1);
    let (c, s) = (trig::cos(a).to_f32(), trig::sin(a).to_f32());
    m.set_f32(tmp, c);
    m.set_f32(tmp + 32, s);
    m.set_f32(tmp + 8, -s);
    m.set_f32(tmp + 40, c);
    si_particle_matrix_multiply(m, out, tmp, out);
    si_particle_make_identity(m, tmp);
    let a = angle(m, r, 0);
    let (c, s) = (trig::cos(a).to_f32(), trig::sin(a).to_f32());
    m.set_f32(tmp + 20, c);
    m.set_f32(tmp + 36, s);
    m.set_f32(tmp + 24, -s);
    m.set_f32(tmp + 40, c);
    si_particle_matrix_multiply(m, out, tmp, out);
    m.release(64);
}
