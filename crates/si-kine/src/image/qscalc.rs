// SPDX-License-Identifier: MIT
//! The quick stretch's deformation on the image: `SI_QuickStretchCalculation` (`qstretch.c`,
//! 12,748 bytes; finding 0032, "The deformation"), which `SI_animQuickStretchUpdate` calls
//! twice.
//!
//! The node's kinematic record gives four vectors: linear velocity and acceleration,
//! angular velocity and acceleration. Each is turned into the model's frame. Each drives
//! three of the twelve groups of the relation's data (`SI_RelQSData`, 0x18 bytes a group,
//! see [`grp`]): a group with its enable word set gets its stretch vector at set-up and,
//! for every point of the geometry, adds to the point's displacement. The groups:
//!
//! | Vector | Groups | First group | Second | Third |
//! |---|---|---|---|---|
//! | linear velocity, acceleration | 0–2, 3–5 | pull along the vector, by the radial distance | bulge by the distance along the vector | swell by both |
//! | angular velocity, acceleration | 6–8, 9–11 | twist about the vector (a rotation) | as the linear second group, the threshold's sign the other way | swell, option 1 only |
//!
//! Every value is computed in the unit's register and rounded where the original stores it.
//! Each place says which: a stored `float` is `to_f32` of the `F80` the original holds.
//!
//! **Calls out** go through [`Outside`] (`SI_modelUpdateSonState`,
//! `SI_modelComputeBoundingEL`, the three vertex accessors, `fprintf` for an unknown
//! option). The root walk (`SI_modelRoot`) is si-model's, the matrices si-math's.

use si_math::algebra as alg;
use si_math::types::{Matrix, Vector};
use si_mem::Mem;
use si_x87::{fmov, x, F80};

use super::qstretch::quick_stretch_get_elem;
use crate::outside as out;
use crate::outside::Outside;

/// Offsets in a group of the relation's data (group `k` at `0x18 k`).
pub mod grp {
    /// 16-bit: the group is used.
    pub const ENABLE: u32 = 0x00;
    /// 16-bit: the option (0 or 1; anything else prints "Unexpected default").
    pub const OPTION: u32 = 0x04;
    /// 16-bit each: the displacement's x, y, z left alone.
    pub const SKIP_X: u32 = 0x06;
    pub const SKIP_Y: u32 = 0x08;
    pub const SKIP_Z: u32 = 0x0a;
    /// The amount (the update stores the curve's value × 0.005 here).
    pub const AMOUNT: u32 = 0x0c;
    /// The threshold of the stretch vector's length.
    pub const THRESHOLD: u32 = 0x10;
    /// The size of a group.
    pub const SIZE: u32 = 0x18;
}

/// Fields of the model record read here.
mod md {
    pub const LATDEP: u32 = 0x44;
    pub const BBOX_MIN: u32 = 0x10;
    pub const BBOX_MAX: u32 = 0x1c;
    /// The radius `SI_modelComputeBoundingEL` stores.
    pub const RADIUS: u32 = 0x2c;
    pub const GSCL: u32 = 0x16c;
    pub const GROT: u32 = 0x178;
    pub const GTRS: u32 = 0x1b8;
    pub const GMAT: u32 = 0x1c4;
}

/// The double 1e-4 (0x08672b38) of every threshold here.
const EPS: f64 = f64::from_bits(0x3f1a_36e2_eb1c_432d);
/// The float ±1e-4 a scaling too small is replaced with (0x08672b40, 0x08672b44).
const SCALE_MIN: f32 = f32::from_bits(0x38d1_b717);
/// 0.5773 (0x08672b48), 1.17 (0x08672b50), π (0x08672b58): doubles.
const C_0_5773: f64 = f64::from_bits(0x3fe2_793d_d97f_62b7);
const C_1_17: f64 = f64::from_bits(0x3ff2_b851_eb85_1eb8);
const PI: f64 = f64::from_bits(0x4009_21fb_5444_2d18);

/// `stderr` (the C library's, copied into the binary's `.bss`).
const STDERR: u32 = 0x0878_0c88;
/// The texts of the "Unexpected default" messages, by group: (message, line, file, format).
const DEFAULTS: [(u32, u32, u32, u32); 12] = [
    (0x086f_5f80, 0x3c7, 0x086f_5fc0, 0x086f_6040),
    (0x086f_6080, 0x435, 0x086f_60c0, 0x086f_6140),
    (0x086f_6180, 0x4bd, 0x086f_61c0, 0x086f_6240),
    (0x086f_6280, 0x51c, 0x086f_62c0, 0x086f_6340),
    (0x086f_6380, 0x579, 0x086f_63c0, 0x086f_6440),
    (0x086f_6480, 0x5be, 0x086f_64c0, 0x086f_6540),
    (0x086f_6580, 0x632, 0x086f_65c0, 0x086f_6640),
    (0x086f_6680, 0x69f, 0x086f_66c0, 0x086f_6740),
    (0x086f_6780, 0x707, 0x086f_67c0, 0x086f_6840),
    (0x086f_6880, 0x778, 0x086f_68c0, 0x086f_6940),
    (0x086f_6980, 0x7da, 0x086f_69c0, 0x086f_6a40),
    (0x086f_6a80, 0x81c, 0x086f_6ac0, 0x086f_6b40),
];

type V3 = [f32; 3];
type E3 = [F80; 3];

fn eps() -> F80 {
    F80::from_f64(EPS)
}

fn v3(m: &Mem, a: u32) -> V3 {
    [m.f32(a), m.f32(a + 4), m.f32(a + 8)]
}

fn put_v3(m: &mut Mem, a: u32, v: V3) {
    m.set_f32(a, v[0]);
    m.set_f32(a + 4, v[1]);
    m.set_f32(a + 8, v[2]);
}

fn mat(m: &Mem, a: u32) -> Matrix {
    let mut out = [[0.0f32; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, e) in row.iter_mut().enumerate() {
            *e = m.f32(a + (16 * i + 4 * j) as u32);
        }
    }
    out
}

fn put_mat(m: &mut Mem, a: u32, v: &Matrix) {
    for (i, row) in v.iter().enumerate() {
        for (j, e) in row.iter().enumerate() {
            m.set_f32(a + (16 * i + 4 * j) as u32, *e);
        }
    }
}

fn f32s(v: E3) -> V3 {
    [v[0].to_f32(), v[1].to_f32(), v[2].to_f32()]
}

fn ext(v: V3) -> E3 {
    [x(v[0]), x(v[1]), x(v[2])]
}

/// `p` as a row vector through a 4 × 4 matrix with its translation row:
/// `((p₀ M₀c + p₁ M₁c) + p₂ M₂c) + M₃c`, in the register.
fn through(p: E3, a: &Matrix) -> E3 {
    let c = |k: usize| ((p[0] * a[0][k] + p[1] * a[1][k]) + p[2] * a[2][k]) + a[3][k];
    [c(0), c(1), c(2)]
}

/// The vector in a node's frame: each row of the 3 × 3 corner dotted with `v`,
/// `(Mᵢ₀ v₀ + Mᵢ₁ v₁) + Mᵢ₂ v₂`, rounded to float.
fn local(v: V3, a: &Matrix) -> V3 {
    let r = |i: usize| ((x(a[i][0]) * v[0] + x(a[i][1]) * v[1]) + x(a[i][2]) * v[2]).to_f32();
    [r(0), r(1), r(2)]
}

/// `(a₀ b₀ + a₁ b₁) + a₂ b₂` in the register, of floats.
fn dot(a: V3, b: V3) -> F80 {
    (x(a[0]) * b[0] + x(a[1]) * b[1]) + x(a[2]) * b[2]
}

/// What the set-up leaves for one group: its stretch vector and the vector's length (and
/// squared length) as the original stores them; groups that store none leave 0.
#[derive(Clone, Copy, Default)]
struct Stretch {
    g: V3,
    n: f32,
    sq: f32,
}

/// The four vectors' blocks: the local vector, its squared length, the three groups.
struct Block {
    on: bool,
    l: V3,
    s: f32,
    unit: V3,
    st: [Stretch; 3],
}

/// The factor of a group's stretch vector: `threshold / len` when the threshold is below the
/// length, else 1.
fn factor(m: &Mem, data: u32, k: u32, len: f32) -> F80 {
    let thr = x(m.f32(data + grp::SIZE * k + grp::THRESHOLD));
    if thr.lt(x(len)) {
        thr / len
    } else {
        F80::ONE
    }
}

/// The set-up of one vector's three groups (groups `first` … `first + 2`), as the four blocks
/// of the original do it: the vector in the frame of `frame` (a node's `grot`), the length
/// `float(√s)`, then each enabled group. `angular` blocks give their first group the unit
/// vector and a length; the linear ones do not.
fn set_up(m: &Mem, data: u32, first: u32, on: bool, v: V3, s: f32, frame: &Matrix, angular: bool) -> Block {
    let mut b = Block { on, l: [0.0; 3], s, unit: [0.0; 3], st: [Stretch::default(); 3] };
    if !on {
        return b;
    }
    let l = local(v, frame);
    b.l = l;
    let len = x(s).sqrt().to_f32();
    let enabled = |k: u32| m.u16(data + grp::SIZE * k) != 0;
    // the first group
    if enabled(first) {
        if angular {
            let inv = F80::ONE / len;
            b.unit = [(inv * l[0]).to_f32(), (inv * l[1]).to_f32(), (inv * l[2]).to_f32()];
        }
        let f = factor(m, data, first, len);
        let p = [x(l[0]) * f, x(l[1]) * f, x(l[2]) * f];
        b.st[0].g = f32s(p);
        if angular {
            b.st[0].n = ((p[0] * p[0] + p[1] * p[1]) + p[2] * p[2]).sqrt().to_f32();
        }
    }
    // the second and third: the products stay in the register for the length
    for (j, k) in [(1usize, first + 1), (2, first + 2)] {
        if enabled(k) {
            let f = factor(m, data, k, len);
            let p = [f * l[0], f * l[1], f * l[2]];
            let sum = (p[0] * p[0] + p[1] * p[1]) + p[2] * p[2];
            b.st[j].g = f32s(p);
            if j == 1 {
                b.st[j].sq = sum.to_f32();
            }
            b.st[j].n = sum.sqrt().to_f32();
        }
    }
    b
}

/// `SI_QuickStretchCalculation`'s scratch, on the image's stack: what the original keeps in
/// its frame and hands out by address.
struct Frame {
    base: u32,
}

impl Frame {
    const SIZE: u32 = 0xd0;
    const INDEX: u32 = 0x00;
    const DATA: u32 = 0x04;
    const FLAG: u32 = 0x08;
    const POS: u32 = 0x0c;
    const NEG_T: u32 = 0x18;
    /// The translation matrix, later the inverse of a global matrix (-0x1e0).
    const T: u32 = 0x24;
    /// The twist's rotation (-0x184).
    const ROT: u32 = 0x64;
    fn at(&self, off: u32) -> u32 {
        self.base + off
    }
}

/// `fprintf(stderr, "Unexpected default in file '%s' at line %d:\n   %s", …)` of group `k`.
fn unexpected(m: &mut Mem, o: &mut dyn Outside, k: usize) {
    let (msg, line, file, fmt) = DEFAULTS[k];
    let stream = m.u32(STDERR);
    o.call(m, &out::FPRINTF, &[stream, fmt, file, line, msg]);
}

/// The radial direction of `p` off the vector `l` (`s` its squared length): the component of
/// `p` across `l`, made a unit vector; 0 when `s` or that component's length is below 1e-4.
/// The answer is the unit vector (as the original stores it) and `p · e` in the register.
fn radial(p: V3, l: V3, s: f32) -> (V3, F80) {
    let mut e = [0.0f32; 3];
    if x(s).ge(eps()) {
        let k = ((x(p[1]) * l[1] + x(p[0]) * l[0]) + x(p[2]) * l[2]) / s;
        let q = [x(p[0]) - k * l[0], x(p[1]) - k * l[1], x(p[2]) - k * l[2]];
        let sum = (q[1] * q[1] + q[0] * q[0]) + q[2] * q[2];
        let n = sum.sqrt().to_f32();
        if x(n).ge(eps()) {
            let inv = F80::ONE / n;
            e = [(q[0] * inv).to_f32(), (q[1] * inv).to_f32(), (q[2] * inv).to_f32()];
        }
    }
    let r = (x(p[0]) * e[0] + x(p[1]) * e[1]) + x(p[2]) * e[2];
    (e, r)
}

/// What the per-point code reads besides the groups.
struct PointCtx {
    p: V3,
    /// `|p|²` as the original stores it (-0x1fc).
    d2: f32,
    /// `|p|` (-0x1f8).
    plen: f32,
    /// The bounding radius and its square (-0x218, -0x21c).
    radius: f32,
    radius2: f32,
}

/// Adds `w × v` (or subtracts, `sub`) to the displacement's components the group does not skip.
fn apply(m: &Mem, data: u32, k: u32, d: &mut V3, mut term: impl FnMut(usize) -> F80) {
    for (c, off) in [grp::SKIP_X, grp::SKIP_Y, grp::SKIP_Z].into_iter().enumerate() {
        if m.u16(data + grp::SIZE * k + off) == 0 {
            d[c] = (term(c) + d[c]).to_f32();
        }
    }
}

fn apply_sub(m: &Mem, data: u32, k: u32, d: &mut V3, mut term: impl FnMut(usize) -> F80) {
    for (c, off) in [grp::SKIP_X, grp::SKIP_Y, grp::SKIP_Z].into_iter().enumerate() {
        if m.u16(data + grp::SIZE * k + off) == 0 {
            d[c] = (x(d[c]) - term(c)).to_f32();
        }
    }
}

/// The per-point code of a linear block (groups `first` … `first + 2`).
fn linear_point(m: &mut Mem, o: &mut dyn Outside, data: u32, first: u32, b: &Block, cx: &PointCtx, d: &mut V3) {
    let p = cx.p;
    let (e, r) = radial(p, b.l, b.s);
    let at = |k: u32| data + grp::SIZE * k;
    let amount = |m: &Mem, k: u32| m.f32(at(k) + grp::AMOUNT);
    // the pull
    let k = first;
    if m.u16(at(k)) != 0 {
        let g = b.st[0].g;
        match m.u16(at(k) + grp::OPTION) {
            0 => {
                if x(cx.radius).gt(eps()) {
                    let w = ((r * r) * amount(m, k)) / cx.radius;
                    apply_sub(m, data, k, d, |c| w * g[c]);
                }
            }
            1 => {
                let w = (((r * F80::from_f64(C_0_5773)) * (x(e[2].abs()) + (x(e[1].abs()) + e[0].abs()))) * amount(m, k)).to_f32();
                apply_sub(m, data, k, d, |c| x(w) * g[c]);
            }
            _ => unexpected(m, o, k as usize),
        }
    }
    // the bulge
    let k = first + 1;
    if m.u16(at(k)) != 0 {
        let st = b.st[1];
        let h = dot(p, st.g);
        let t = amount(m, k);
        match m.u16(at(k) + grp::OPTION) {
            0 => {
                let (u, v) = if F80::ZERO.lt(x(t)) {
                    let u = if x(st.n).gt(eps()) { (h * t) / st.n } else { F80::ZERO };
                    let v = ((r / ((x(st.n) * t) + F80::ONE).sqrt()) + -r).to_f32();
                    (u, x(v))
                } else {
                    let u = if x(st.sq).gt(eps()) {
                        let q = x(-t) * st.n;
                        let s = q * q + (q + q);
                        let kq = (-s / (F80::ONE + s)).to_f32();
                        (h * kq) / st.sq
                    } else {
                        F80::ZERO
                    };
                    (u, (r * st.n) * -t)
                };
                apply(m, data, k, d, |c| x(st.g[c]) * u + x(e[c]) * v);
            }
            1 => {
                if x(t).gt(F80::ZERO) {
                    if x(st.n).gt(eps()) {
                        let u = (h * t) / st.n;
                        apply(m, data, k, d, |c| x(st.g[c]) * u);
                    }
                } else {
                    let v = (r * st.n) * -t;
                    apply(m, data, k, d, |c| x(e[c]) * v);
                }
            }
            _ => unexpected(m, o, k as usize),
        }
    }
    // the swell
    let k = first + 2;
    if m.u16(at(k)) != 0 {
        let st = b.st[2];
        let h = dot(p, st.g);
        match m.u16(at(k) + grp::OPTION) {
            0 => {
                if x(cx.radius).gt(eps()) {
                    let w = ((h * r) * amount(m, k)) / cx.radius;
                    apply_sub(m, data, k, d, |c| x(e[c]) * w);
                }
            }
            1 => {
                let c0 = x(cx.plen) * st.n;
                if c0.gt(eps()) {
                    let root = ((F80::ONE - h / c0).abs() * F80::from_f64(C_1_17)).sqrt();
                    let w = (((r * (F80::ONE - root)) * st.n) * amount(m, k)).to_f32();
                    apply_sub(m, data, k, d, |c| x(e[c]) * w);
                }
            }
            _ => unexpected(m, o, k as usize),
        }
    }
}

/// The per-point code of an angular block (groups `first` … `first + 2`); `twist_out` is the
/// matrix argument the first pass copies the twist into.
#[allow(clippy::too_many_arguments)]
fn angular_point(m: &mut Mem, o: &mut dyn Outside, data: u32, first: u32, b: &Block, cx: &PointCtx, d: &mut V3, fr: &Frame, fresh: bool, twist_out: u32) {
    let p = cx.p;
    let (e, r) = radial(p, b.l, b.s);
    let at = |k: u32| data + grp::SIZE * k;
    let amount = |m: &Mem, k: u32| m.f32(at(k) + grp::AMOUNT);
    // the twist
    let k = first;
    if m.u16(at(k)) != 0 {
        let n = b.st[0].n;
        let angle = match m.u16(at(k) + grp::OPTION) {
            0 => x(cx.radius2).gt(eps()).then(|| (((x(amount(m, k)) * F80::from_f64(PI)) * n) * cx.d2) / cx.radius2),
            1 => x(cx.radius2).gt(eps()).then(|| ((((x(amount(m, k)) * F80::from_f64(PI)) * n) * r) * r) / cx.radius2),
            _ => {
                unexpected(m, o, k as usize);
                None
            }
        };
        if let Some(angle) = angle {
            let a = -angle.to_f32();
            let u = b.unit;
            let rot = alg::matrix_axis_rotation(a, &Vector { x: u[0], y: u[1], z: u[2] });
            put_mat(m, fr.at(Frame::ROT), &rot);
            if fresh {
                let c = alg::matrix_copy(&mat(m, fr.at(Frame::ROT)));
                put_mat(m, twist_out, &c);
            }
            let rot = mat(m, fr.at(Frame::ROT));
            let q = through(ext(p), &rot);
            let qf = f32s(q);
            let skip = |off: u32| m.u16(at(k) + off) != 0;
            if !skip(grp::SKIP_X) {
                d[0] = ((q[0] - p[0]) + d[0]).to_f32();
            }
            if !skip(grp::SKIP_Y) {
                d[1] = ((x(qf[1]) - p[1]) + d[1]).to_f32();
            }
            if !skip(grp::SKIP_Z) {
                d[2] = ((x(qf[2]) - p[2]) + d[2]).to_f32();
            }
        }
    }
    // the bulge, the threshold's sign the other way from the linear one
    let k = first + 1;
    if m.u16(at(k)) != 0 {
        let st = b.st[1];
        let h = dot(p, st.g);
        let t = amount(m, k);
        match m.u16(at(k) + grp::OPTION) {
            0 => {
                let (u, v) = if F80::ZERO.lt(x(t)) {
                    let u = if x(st.sq).gt(eps()) {
                        let q = x(st.n) * t;
                        let s = q * q + (q + q);
                        let kq = (-s / (F80::ONE + s)).to_f32();
                        (h * kq) / st.sq
                    } else {
                        F80::ZERO
                    };
                    (u, (r * st.n) * t)
                } else {
                    let v = ((r / ((x(-t) * st.n) + F80::ONE).sqrt()) + -r).to_f32();
                    let u = if x(st.n).gt(eps()) { (h * -t) / st.n } else { F80::ZERO };
                    (u, x(v))
                };
                apply(m, data, k, d, |c| x(st.g[c]) * u + x(e[c]) * v);
            }
            1 => {
                if x(t).gt(F80::ZERO) {
                    let v = x(t) * (r * st.n);
                    apply(m, data, k, d, |c| x(e[c]) * v);
                } else if x(st.n).gt(eps()) {
                    let u = (h * -t) / st.n;
                    apply(m, data, k, d, |c| x(st.g[c]) * u);
                }
            }
            _ => unexpected(m, o, k as usize),
        }
    }
    // the swell: option 1 only
    let k = first + 2;
    if m.u16(at(k)) != 0 {
        let st = b.st[2];
        let h = dot(p, st.g);
        match m.u16(at(k) + grp::OPTION) {
            0 => {}
            1 => {
                let c0 = x(st.n) * cx.radius;
                if c0.gt(eps()) {
                    let w = ((h * ((r + r) - cx.radius)) * amount(m, k)) / c0;
                    apply(m, data, k, d, |c| x(st.g[c]) * w);
                }
            }
            _ => unexpected(m, o, k as usize),
        }
    }
}

/// The deformation of an element's geometry by quick stretch
/// [soft 0x0859e280 SI_QuickStretchCalculation]: `(dict, elem, number, other, &buffer,
/// matrix1, matrix2, vector1, vector2, kine, _, tagged_only)`.
///
/// 1. Nothing for a null element, or a null `other` when it differs from `elem`. A null
///    `*buffer` gets `SI_calloc(1, 12)` (zeroed); with `other == elem` nothing more.
/// 2. The quick stretch's element and data (`SI_QuickStretchGetElem`): of `other`'s model
///    (`elem` kept), or of `elem`'s own (`elem` replaced by the answer). Nothing without an
///    element, data or `kine`.
/// 3. The four vectors from `kine` (+0x30, +0x3c, +0x48, +0x54). The roots of both models
///    updated (`SI_modelUpdateSonState(null, root, 1)`, the second when another root). The
///    angular two through the model's `grot` with its translation row.
/// 4. A linear velocity whose three components are all below 1e-4 in size becomes
///    `vector1`; otherwise it is stretched by `(|v| + |vector1|) / |v|` (the lengths
///    floats). The angular velocity likewise with `vector2`.
/// 5. The four squared lengths, and a flag per vector: positive, with one of its three
///    groups enabled.
/// 6. The reference node S (`other`'s model, else the model): its box through the
///    translation by −`gtrs` (`SI_modelComputeBoundingEL`), the radius R (+0x2c) and R².
///    Each vector's set-up ([`set_up`]) in S's frame. S's scaling, each component below
///    1e-4 in size replaced by ±1e-4.
/// 7. For each point (one, the buffer's, on the pass that allocated it; else
///    `SI_elemGetNbVertices`, `SI_elemGetVertex(…, 2, 0, i, …)`): scaled by the scaling
///    when `other == elem`; through `matrix1` and `matrix2`; with `other`, through the
///    model's `latdep` global matrix, the inverse of `other`'s, and the scaling. Unless
///    `tagged_only` and the point is not tagged: its displacement from the four blocks
///    ([`linear_point`], [`angular_point`]) added; with `other`, divided by the scaling and
///    through `other`'s `latdep` global matrix and the inverse of the model's.
/// 8. The allocating pass keeps the point in the buffer. The others: divided by the
///    scaling when `other == elem`, the buffer added, `SI_elemSetVertex`.
#[allow(clippy::too_many_arguments)]
pub fn quick_stretch_calculation(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, number: i32, other: u32, pbuf: u32, matrix1: u32, matrix2: u32, vector1: u32, vector2: u32, kine: u32, _kine2: u32, tagged_only: i16) {
    let differ = elem != other;
    if (differ && other == 0) || elem == 0 {
        return;
    }
    let fresh = m.u32(pbuf) == 0;
    if fresh {
        let b = m.si_calloc(1, 12);
        m.set_u32(pbuf, b);
        for off in [8, 4, 0] {
            m.set_u32(b + off, 0);
        }
        if !differ {
            return;
        }
    }
    let fr = Frame { base: m.local(Frame::SIZE) };
    run(m, o, &fr, dict, elem, number, other, differ, fresh, pbuf, matrix1, matrix2, vector1, vector2, kine, tagged_only);
    m.release(Frame::SIZE);
}

#[allow(clippy::too_many_arguments)]
fn run(m: &mut Mem, o: &mut dyn Outside, fr: &Frame, dict: u32, mut elem: u32, number: i32, other: u32, differ: bool, fresh: bool, pbuf: u32, matrix1: u32, matrix2: u32, vector1: u32, vector2: u32, kine: u32, tagged_only: i16) {
    let model = m.u32(elem + 4);
    let model2 = if differ { m.u32(other + 4) } else { 0 };
    if differ {
        quick_stretch_get_elem(m, o, model2, fr.at(Frame::INDEX), fr.at(Frame::DATA));
    } else {
        elem = quick_stretch_get_elem(m, o, model, fr.at(Frame::INDEX), fr.at(Frame::DATA));
    }
    let data = m.u32(fr.at(Frame::DATA));
    if elem == 0 || data == 0 || kine == 0 {
        return;
    }
    let mut lv = v3(m, kine + 0x30).map(fmov);
    let la = v3(m, kine + 0x3c).map(fmov);
    let mut av = v3(m, kine + 0x48).map(fmov);
    let mut aa = v3(m, kine + 0x54).map(fmov);
    let root = si_model::model_image::tree::root(m, si_mem::Ptr32::new(model)).addr();
    o.call(m, &out::SI_MODEL_UPDATE_SON_STATE, &[0, root, 1]);
    if differ {
        let root2 = si_model::model_image::tree::root(m, si_mem::Ptr32::new(model2)).addr();
        if root2 != root {
            o.call(m, &out::SI_MODEL_UPDATE_SON_STATE, &[0, root2, 1]);
        }
    }
    let grot = mat(m, model + md::GROT);
    av = f32s(through(ext(av), &grot));
    aa = f32s(through(ext(aa), &grot));
    // the two velocities stretched by the vectors handed in
    let stretch = |m: &Mem, v: V3, by: u32| -> V3 {
        let small = |c: f32| (x(c) - F80::ZERO).abs().lt(eps());
        if small(v[0]) && small(v[1]) && small(v[2]) {
            return v3(m, by).map(fmov);
        }
        let len = ((x(v[1]) * v[1] + x(v[0]) * v[0]) + x(v[2]) * v[2]).sqrt().to_f32();
        let w = v3(m, by);
        let wlen = ((x(w[0]) * w[0] + x(w[1]) * w[1]) + x(w[2]) * w[2]).sqrt().to_f32();
        let f = (x(len) + wlen) / len;
        f32s([x(v[0]) * f, x(v[1]) * f, x(v[2]) * f])
    };
    lv = stretch(m, lv, vector1);
    av = stretch(m, av, vector2);
    // the squared lengths, rounded where the original stores them
    let sq_running = |v: V3| -> f32 {
        let a = (x(v[0]) * v[0]).to_f32();
        let b = (x(v[1]) * v[1] + a).to_f32();
        (x(v[2]) * v[2] + b).to_f32()
    };
    let sq_paired = |v: V3| -> f32 {
        let a = (x(v[0]) * v[0] + x(v[1]) * v[1]).to_f32();
        (x(v[2]) * v[2] + a).to_f32()
    };
    let (s_lv, s_la, s_av, s_aa) = (sq_running(lv), sq_paired(la), sq_running(av), sq_paired(aa));
    let flag = |m: &Mem, s: f32, first: u32| F80::ZERO.lt(x(s)) && (0..3).any(|j| m.u16(data + grp::SIZE * (first + j)) != 0);
    let (on_lv, on_la, on_av, on_aa) = (flag(m, s_lv, 0), flag(m, s_la, 3), flag(m, s_av, 6), flag(m, s_aa, 9));
    // the reference node: its box, radius and frame
    let s_node = if differ { model2 } else { model };
    let neg_t = v3(m, s_node + md::GTRS).map(|c| -c);
    put_v3(m, fr.at(Frame::NEG_T), neg_t);
    let t = alg::matrix_translation(&Vector { x: neg_t[0], y: neg_t[1], z: neg_t[2] });
    put_mat(m, fr.at(Frame::T), &t);
    o.call(m, &out::SI_MODEL_COMPUTE_BOUNDING_EL, &[s_node, s_node + md::BBOX_MIN, s_node + md::BBOX_MAX, fr.at(Frame::T)]);
    let radius = fmov(m.f32(s_node + md::RADIUS));
    let radius2 = (x(m.f32(s_node + md::RADIUS)) * m.f32(s_node + md::RADIUS)).to_f32();
    let frame = mat(m, s_node + md::GROT);
    let blocks = [
        set_up(m, data, 0, on_lv, lv, s_lv, &frame, false),
        set_up(m, data, 3, on_la, la, s_la, &frame, false),
        set_up(m, data, 6, on_av, av, s_av, &frame, true),
        set_up(m, data, 9, on_aa, aa, s_aa, &frame, true),
    ];
    let scale = v3(m, s_node + md::GSCL).map(|c| {
        if x(c).abs().lt(eps()) {
            if x(c).lt(F80::ZERO) {
                -SCALE_MIN
            } else {
                SCALE_MIN
            }
        } else {
            fmov(c)
        }
    });
    let count = if fresh { 1 } else { o.call(m, &out::SI_ELEM_GET_NB_VERTICES, &[elem, number as u32, 0]).eax as i32 };
    let pos = fr.at(Frame::POS);
    let flag_at = fr.at(Frame::FLAG);
    let mut i = 0i32;
    while i < count {
        let buf = m.u32(pbuf);
        if fresh {
            put_v3(m, pos, v3(m, buf).map(fmov));
        } else {
            o.call(m, &out::SI_ELEM_GET_VERTEX, &[dict, elem, number as u32, 2, 0, i as u32, pos, flag_at]);
            if !differ {
                let p = v3(m, pos);
                put_v3(m, pos, [(x(p[0]) * scale[0]).to_f32(), (x(p[1]) * scale[1]).to_f32(), (x(p[2]) * scale[2]).to_f32()]);
            }
        }
        let p2 = through(through(ext(v3(m, pos)), &mat(m, matrix1)), &mat(m, matrix2));
        put_v3(m, pos, f32s(p2));
        // On the allocating pass no `SI_elemGetVertex` wrote the tag: the original tests a
        // stack word it never set (-0x1ea), the port its frame's word as `Mem::local` left it
        // (zero, or the image's fill pattern); neither was 1 in any case checked.
        let skip = tagged_only != 0 && m.i16(flag_at) != 1;
        if !skip {
            if differ {
                let latdep = m.u32(model + md::LATDEP);
                let c = mat(m, latdep + md::GMAT);
                put_v3(m, pos, f32s(through(p2, &c)));
                let latdep2 = m.u32(model2 + md::LATDEP);
                if let Some(inv) = alg::matrix_inverse(&mat(m, latdep2 + md::GMAT)) {
                    put_mat(m, fr.at(Frame::T), &inv);
                }
                let q = through(ext(v3(m, pos)), &mat(m, fr.at(Frame::T)));
                put_v3(m, pos, [(q[0] * scale[0]).to_f32(), (q[1] * scale[1]).to_f32(), (q[2] * scale[2]).to_f32()]);
            }
            let p = v3(m, pos);
            let qx = (x(p[0]) * p[0]).to_f32();
            let qy = (x(p[1]) * p[1] + qx).to_f32();
            let sum = x(p[2]) * p[2] + qy;
            let cx = PointCtx { p, d2: sum.to_f32(), plen: sum.sqrt().to_f32(), radius, radius2 };
            let mut d = [0.0f32; 3];
            if blocks[0].on {
                linear_point(m, o, data, 0, &blocks[0], &cx, &mut d);
            }
            if blocks[1].on {
                linear_point(m, o, data, 3, &blocks[1], &cx, &mut d);
            }
            if blocks[2].on {
                angular_point(m, o, data, 6, &blocks[2], &cx, &mut d, fr, fresh, matrix1);
            }
            if blocks[3].on {
                angular_point(m, o, data, 9, &blocks[3], &cx, &mut d, fr, fresh, matrix2);
            }
            let moved = [x(p[0]) + d[0], x(p[1]) + d[1], x(p[2]) + d[2]];
            put_v3(m, pos, f32s(moved));
            if differ {
                let back = [moved[0] / scale[0], moved[1] / scale[1], moved[2] / scale[2]];
                put_v3(m, pos, f32s(back));
                let latdep2 = m.u32(model2 + md::LATDEP);
                let c = mat(m, latdep2 + md::GMAT);
                put_v3(m, pos, f32s(through(back, &c)));
                let latdep = m.u32(model + md::LATDEP);
                if let Some(inv) = alg::matrix_inverse(&mat(m, latdep + md::GMAT)) {
                    put_mat(m, fr.at(Frame::T), &inv);
                }
                let q = through(ext(v3(m, pos)), &mat(m, fr.at(Frame::T)));
                put_v3(m, pos, f32s(q));
            }
        }
        if fresh {
            let buf = m.u32(pbuf);
            put_v3(m, buf, v3(m, pos).map(fmov));
        } else {
            if !differ {
                let p = v3(m, pos);
                put_v3(m, pos, [(x(p[0]) / scale[0]).to_f32(), (x(p[1]) / scale[1]).to_f32(), (x(p[2]) / scale[2]).to_f32()]);
            }
            let buf = m.u32(pbuf);
            let p = v3(m, pos);
            let b = v3(m, buf);
            put_v3(m, pos, [(x(p[0]) + b[0]).to_f32(), (x(p[1]) + b[1]).to_f32(), (x(p[2]) + b[2]).to_f32()]);
            o.call(m, &out::SI_ELEM_SET_VERTEX, &[dict, elem, number as u32, 2, 0, i as u32, pos]);
        }
        i += 1;
    }
}
