// SPDX-License-Identifier: MIT
//! The bilinear lookup of the shader preview texture (`shdb_global.c` neighbourhood, finding 0041), in x87.
//!
//! The original keeps t1/t2 (the fractions) unrounded in 80-bit registers and only the weights `1 - t` go through a
//! float store, so the arithmetic is replayed on a model of the register stack, instruction by instruction.

use si_mem::Mem;
use si_x87::{Round, F80};

/// A model of the x87 register stack (top is `st(0)`), only the instructions this routine uses.
pub struct Fp {
    s: Vec<F80>,
}

impl Fp {
    pub fn new() -> Fp {
        Fp { s: Vec::new() }
    }
    fn ix(&self, i: usize) -> usize {
        self.s.len() - 1 - i
    }
    pub fn st(&self, i: usize) -> F80 {
        self.s[self.ix(i)]
    }
    fn set(&mut self, i: usize, v: F80) {
        let k = self.ix(i);
        self.s[k] = v;
    }
    /// `fld` of an exact value.
    pub fn ld(&mut self, v: F80) {
        self.s.push(v);
    }
    pub fn ld_st(&mut self, i: usize) {
        let v = self.st(i);
        self.s.push(v);
    }
    pub fn fxch(&mut self, i: usize) {
        let (a, b) = (self.st(0), self.st(i));
        self.set(0, b);
        self.set(i, a);
    }
    pub fn pop(&mut self) -> F80 {
        self.s.pop().unwrap()
    }
    /// `fstp dword`.
    pub fn stp32(&mut self) -> f32 {
        self.pop().to_f32()
    }
    /// `fst dword` (the register keeps its 80 bits).
    pub fn st32(&self) -> f32 {
        self.st(0).to_f32()
    }
    /// `fstp st(i)`.
    pub fn stp_st(&mut self, i: usize) {
        let v = self.st(0);
        self.set(i, v);
        self.pop();
    }
    pub fn chs(&mut self) {
        let v = -self.st(0);
        self.set(0, v);
    }
    /// `faddp st(i),st`.
    pub fn faddp(&mut self, i: usize) {
        let v = self.st(i) + self.st(0);
        self.set(i, v);
        self.pop();
    }
    /// `fsubp st(i),st`: st(i) - st(0).
    pub fn fsubp(&mut self, i: usize) {
        let v = self.st(i) - self.st(0);
        self.set(i, v);
        self.pop();
    }
    /// `fsubrp st(i),st`: st(0) - st(i).
    pub fn fsubrp(&mut self, i: usize) {
        let v = self.st(0) - self.st(i);
        self.set(i, v);
        self.pop();
    }
    pub fn fmulp(&mut self, i: usize) {
        let v = self.st(i) * self.st(0);
        self.set(i, v);
        self.pop();
    }
    /// `fdivrp st(i),st`: st(0) / st(i).
    pub fn fdivrp(&mut self, i: usize) {
        let v = self.st(0) / self.st(i);
        self.set(i, v);
        self.pop();
    }
    /// `fsub st(i),st` (destination st(i)).
    pub fn fsub_to(&mut self, i: usize) {
        let v = self.st(i) - self.st(0);
        self.set(i, v);
    }
    /// `fsub st,st(i)`.
    pub fn fsub_from(&mut self, i: usize) {
        let v = self.st(0) - self.st(i);
        self.set(0, v);
    }
    /// `fdiv st(i),st`.
    pub fn fdiv_to(&mut self, i: usize) {
        let v = self.st(i) / self.st(0);
        self.set(i, v);
    }
    /// `fdiv st,st(i)`.
    pub fn fdiv_from(&mut self, i: usize) {
        let v = self.st(0) / self.st(i);
        self.set(0, v);
    }
    /// `fmul st,st(i)`.
    pub fn fmul_from(&mut self, i: usize) {
        let v = self.st(0) * self.st(i);
        self.set(0, v);
    }
    /// `fmul dword [m]`.
    pub fn fmul_m(&mut self, m: f32) {
        let v = self.st(0) * m;
        self.set(0, v);
    }
    /// `fadd dword [m]`.
    pub fn fadd_m(&mut self, m: f32) {
        let v = self.st(0) + m;
        self.set(0, v);
    }
    /// `fistp dword` under truncation (the control word set to RC = 11 around it).
    pub fn istp_trunc(&mut self) -> i32 {
        self.pop().to_i32(Round::Zero)
    }
    /// `fist dword` under truncation.
    pub fn ist_trunc(&self) -> i32 {
        self.st(0).to_i32(Round::Zero)
    }
}

impl Default for Fp {
    fn default() -> Fp {
        Fp::new()
    }
}

const TWO: f64 = 2.0; // [0x086636d0]
const FF: f64 = 255.0; // [0x086636d8]

/// [soft 0x0854ea9c si_lookup_preview_texture]: the colour (4 floats at `out`) of the preview texture `tex` (width short +0, height
/// short +2, pixels +0xc: 4 bytes each) at (u, v): both coordinates are wrapped into [0, 1] with truncating folds (a negative one
/// first gets `1 - c` truncated added), scaled by (size - 2), and the four neighbouring texels (indices taken modulo the pixel count)
/// are blended by the fractions. Answers 0 for a missing texture or pixel block, else 1. Unlike the decompiler's reading, the
/// fractions stay in 80-bit registers; only `1 - fraction` and the texel quotients are rounded to float.
pub fn si_lookup_preview_texture(m: &mut Mem, tex: u32, u: f64, v: f64, out: u32) -> u32 {
    if tex == 0 {
        return 0;
    }
    let pix = m.u32(tex.wrapping_add(0xc));
    if pix == 0 {
        return 0;
    }
    let mut f = Fp::new();
    // 0854eac0..0854ead5: st = [V, U'] ... (U and V pass through a float store)
    f.ld(F80::from_f64(u));
    let t = f.stp32();
    f.ld(F80::from_f32(t));
    f.ld(F80::from_f64(v));
    f.ld(F80::ZERO);
    f.fxch(1);
    let t = f.stp32();
    f.ld_st(1);
    f.ld(F80::from_f32(t));
    f.fxch(1);
    // 0854eada: U < 0 ?
    if f.st(0).lt(f.st(2)) {
        f.ld(F80::ONE);
        f.fxch(1);
        f.chs();
        f.faddp(1);
        let t = f.stp32();
        f.ld(F80::from_f32(t));
        let i = f.istp_trunc();
        f.ld(F80::from_i32(i));
        f.faddp(3);
    } else {
        f.pop();
    }
    // 0854eb1c: V < 0 ?
    f.ld_st(0);
    let neg = f.st(0).lt(f.st(2));
    f.stp_st(2);
    if neg {
        f.ld(F80::ONE);
        f.fxch(2);
        f.chs();
        f.faddp(2);
        f.fxch(1);
        let t = f.stp32();
        f.ld(F80::from_f32(t));
        let i = f.istp_trunc();
        f.ld(F80::from_i32(i));
        f.faddp(1);
    } else {
        f.stp_st(1);
    }
    // 0854eb64: fold above 1
    f.ld(F80::ONE);
    if f.st(0).lt(f.st(2)) {
        f.fxch(2);
        let i = f.ist_trunc();
        f.ld(F80::from_i32(i));
        f.fsubp(1);
        f.fxch(2);
    }
    if f.st(0).lt(f.st(1)) {
        f.fxch(1);
        let i = f.ist_trunc();
        f.ld(F80::from_i32(i));
        f.fsubp(1);
        f.fxch(1);
    }
    // 0854ebce: scale by the size minus two
    let w = m.u16(tex) as i16;
    f.ld(F80::from_i32(w as i32));
    f.ld(F80::from_f64(TWO));
    f.fsub_to(1);
    f.fxch(1);
    f.fmulp(4);
    f.fxch(3);
    let t = f.stp32();
    f.ld(F80::from_f32(t));
    let h = m.u16(tex.wrapping_add(2)) as i16;
    f.ld(F80::from_i32(h as i32));
    f.fsubrp(4);
    f.fxch(3);
    f.fmulp(2);
    f.fxch(1);
    let t = f.stp32();
    f.ld(F80::from_f32(t));
    f.fxch(2);
    let ix = f.ist_trunc();
    f.fxch(2);
    let iy = f.ist_trunc();
    f.ld(F80::from_i32(ix));
    f.fsubp(3);
    f.ld(F80::from_i32(iy));
    f.fsubp(1);
    f.ld_st(1);
    f.fsub_from(3);
    let ra = f.stp32(); // [ebp-0x68]
    f.fsub_to(1);
    f.fxch(1);
    let rb = f.stp32(); // [ebp-0x6c]
    // the pixel indices (32-bit wrapping integer arithmetic)
    let ww = w as i32;
    let hh = h as i32;
    let idx0 = ix.wrapping_add(iy.wrapping_mul(ww));
    let total = hh.wrapping_mul(ww);
    let modp = |c: i32| -> i32 {
        // `cmp ecx,esi; jl skip; cdq; idiv esi`: a modulo when not below the count (a zero count faults in the original)
        if c < total {
            c
        } else if total == 0 || (c == i32::MIN && total == -1) {
            0 // the original faults (divide error); unreachable for a texture with texels
        } else {
            c.wrapping_rem(total)
        }
    };
    let byte = |m: &Mem, i: i32, k: u32| -> u64 { m.u8(pix.wrapping_add((i as u32).wrapping_mul(4)).wrapping_add(k)) as u64 };
    let q = |b: u64| F80::from_i64(b as i64);
    // pixel 0 (idx0): st = [255, t2, t1]
    f.ld(q(byte(m, idx0, 0)));
    f.ld(F80::from_f64(FF));
    f.fdiv_to(1);
    f.fxch(1);
    let p00 = f.stp32();
    let mut p0 = [p00, 0.0, 0.0, 0.0];
    for k in 1..4u32 {
        f.ld(q(byte(m, idx0, k)));
        f.fdiv_from(1);
        p0[k as usize] = f.stp32();
    }
    // pixel 1 (idx0 + 1): channel 0 stays on the stack
    let i1 = modp(idx0.wrapping_add(1));
    f.ld(q(byte(m, i1, 0)));
    f.fdiv_from(1);
    let q0 = f.stp32();
    f.ld(F80::from_f32(q0));
    let mut p1 = [q0, 0.0, 0.0, 0.0];
    for k in 1..4u32 {
        f.ld(q(byte(m, i1, k)));
        f.fdiv_from(2);
        p1[k as usize] = f.stp32();
    }
    // pixel 2 (idx0 + width)
    let i2 = modp(idx0.wrapping_add(ww));
    let mut p2 = [0f32; 4];
    for k in 0..4u32 {
        f.ld(q(byte(m, i2, k)));
        f.fdiv_from(2);
        p2[k as usize] = f.stp32();
    }
    // pixel 3 (width + 1 + idx0): channels 0..2 stay on the stack, channel 3 ends in a store
    let i3 = modp(ww.wrapping_add(1).wrapping_add(idx0));
    let mut p3 = [0f32; 4];
    for k in 0..3u32 {
        f.ld(q(byte(m, i3, k)));
        f.fdiv_from(2 + k as usize);
        let t = f.stp32();
        p3[k as usize] = t;
        f.ld(F80::from_f32(t));
    }
    f.ld(q(byte(m, i3, 3)));
    f.fdivrp(5);
    f.fxch(4);
    let r3 = f.stp32();
    p3[3] = r3;
    // st = [r1, r0, q0, r2, t2, t1]
    // 0854ee63: the blend
    f.ld_st(5);
    f.ld(F80::from_f32(ra));
    f.fmul_m(p0[0]);
    f.fxch(1);
    f.fmulp(4);
    f.faddp(3);
    f.ld(F80::from_f32(rb));
    f.ld_st(6);
    f.fxch(1);
    f.fmulp(4);
    f.ld(F80::from_f32(ra));
    f.fmul_m(p2[0]);
    f.fxch(1);
    f.fmulp(3);
    f.faddp(2);
    f.ld_st(4);
    f.fmulp(2);
    f.fxch(2);
    f.faddp(1);
    f.ld_st(4);
    f.fxch(1);
    m.set_f32(out, f.stp32());
    f.ld(F80::from_f32(ra));
    f.fmul_m(p0[1]);
    f.fxch(1);
    f.fmul_m(p1[1]);
    f.faddp(1);
    f.ld(F80::from_f32(rb));
    f.ld_st(5);
    f.fxch(1);
    f.fmulp(2);
    f.ld(F80::from_f32(ra));
    f.fmul_m(p2[1]);
    f.fxch(1);
    f.fmulp(3);
    f.faddp(2);
    f.ld_st(3);
    f.fmulp(2);
    f.faddp(1);
    f.ld_st(3);
    f.fxch(1);
    m.set_f32(out.wrapping_add(4), f.stp32());
    f.ld(F80::from_f32(ra));
    f.fmul_m(p0[2]);
    f.fxch(1);
    f.fmul_m(p1[2]);
    f.faddp(1);
    f.ld(F80::from_f32(rb));
    f.ld_st(4);
    f.fxch(1);
    f.fmulp(2);
    f.ld(F80::from_f32(ra));
    f.fmul_m(p2[2]);
    f.fxch(1);
    f.fmulp(3);
    f.faddp(2);
    f.ld_st(2);
    f.fmulp(2);
    f.faddp(1);
    f.ld_st(2);
    f.fxch(1);
    m.set_f32(out.wrapping_add(8), f.stp32());
    f.ld(F80::from_f32(ra));
    f.fmul_m(p0[3]);
    f.fxch(1);
    f.fmul_m(p1[3]);
    f.fxch(3);
    f.fmul_m(p3[3]);
    f.fxch(1);
    f.faddp(3);
    f.ld(F80::from_f32(ra));
    f.fmul_m(p2[3]);
    f.faddp(1);
    f.fxch(2);
    f.fmul_m(rb);
    f.fxch(1);
    f.fmul_from(2);
    f.fxch(1);
    let t = f.stp32(); // [ebp-0x6c]
    f.fadd_m(t);
    f.fxch(1);
    let _ra2 = f.stp32(); // [ebp-0x68]
    m.set_f32(out.wrapping_add(0xc), f.st32());
    1
}
