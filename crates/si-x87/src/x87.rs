// SPDX-License-Identifier: MIT
//! A software model of the x87 arithmetic the original was compiled for (finding 0008).
//!
//! The Linux `soft` binary is i386 code for the x87 unit. Every intermediate value lives in an
//! 80-bit register (sign, 15-bit exponent, 64-bit significand with an explicit leading bit)
//! and is rounded to `float` or `double` only when an instruction stores it to memory. The
//! program never changes the precision field of the control word (its 2,465 `fnstcw` sites
//! are all the compiler's "change the rounding field, convert, restore" sequence), and Linux
//! starts a process with the control word 0x037f: 64-bit significand, round to nearest even,
//! all exceptions masked. That is the mode modelled here.
//!
//! [`F80`] is one such register value. `+ - * /` and [`F80::sqrt`] round to 64 bits exactly as
//! the unit does (they are correctly rounded IEEE operations, so the model is determined by
//! the format alone); [`F80::to_f32`] and [`F80::to_f64`] are the stores. A port of an
//! original function loads its operands into `F80`, repeats the instruction sequence's
//! operations in the same order, and rounds where the original stores.
//!
//! The tests of this module compare every operation with the processor's own x87 unit, bit
//! for bit, on machines that have one (x86-64).

use core::cmp::Ordering;
use core::ops::{Add, Div, Mul, Neg, Sub};

/// One x87 register value: the 80-bit double-extended format.
///
/// There is deliberately no `==`: two register values can be compared as numbers
/// ([`F80::eq`], [`F80::compare`]: −0 equals +0, a NaN equals nothing, as the unit's
/// comparison instructions have it) or as bit patterns ([`F80::to_bits`]), and the two differ.
///
/// The layout is the format's own memory image (`repr(C)`: the significand's 8 bytes, then
/// sign and exponent, little-endian), so the hardware backend ([`crate::hw`]) loads and stores
/// a value in place with `fld`/`fstp tbyte`; bytes 10–15 are padding.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct F80 {
    /// Significand, with the explicit integer bit at bit 63.
    pub(crate) m: u64,
    /// Sign in bit 15, biased exponent (bias 16383) in bits 0–14.
    pub(crate) se: u16,
}

const BIAS: i32 = 16383;
const EXP_MAX: u16 = 0x7fff;

/// Rounding-control settings the original uses. Arithmetic always runs in `Nearest`; the
/// others occur only around a `frndint` (floor, ceil) or a `fistp` (truncate), set and
/// restored by compiler-generated code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Round {
    /// RC = 00: to nearest, ties to even (the default).
    Nearest,
    /// RC = 01: toward minus infinity (C `floor`).
    Down,
    /// RC = 10: toward plus infinity (C `ceil`).
    Up,
    /// RC = 11: toward zero (a C cast to an integer type).
    Zero,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    Zero,
    Finite,
    Inf,
    Nan,
}

impl F80 {
    pub const ZERO: F80 = F80 { m: 0, se: 0 };
    /// What `fld1` loads.
    pub const ONE: F80 = F80 { m: 1 << 63, se: BIAS as u16 };
    pub const INFINITY: F80 = F80 { m: 1 << 63, se: EXP_MAX };
    /// The "real indefinite" value, the unit's answer to an invalid operation (0/0, ∞ − ∞,
    /// the square root of a negative number): a negative quiet NaN.
    pub const INDEFINITE: F80 = F80 { m: 0xc000_0000_0000_0000, se: 0xffff };
    /// What `fldpi` loads: π rounded to 64 bits.
    pub const PI: F80 = F80 { m: 0xc90f_daa2_2168_c235, se: 0x4000 };

    /// From the 10 bytes of the format: significand, then sign and exponent.
    pub const fn from_bits(m: u64, se: u16) -> F80 {
        F80 { m, se }
    }

    /// The 10 bytes of the format as (significand, sign and exponent).
    pub const fn to_bits(self) -> (u64, u16) {
        (self.m, self.se)
    }

    #[inline]
    pub(crate) fn class(self) -> Class {
        if self.se & EXP_MAX == EXP_MAX {
            if self.m << 1 == 0 {
                Class::Inf
            } else {
                Class::Nan
            }
        } else if self.m == 0 {
            Class::Zero
        } else {
            Class::Finite
        }
    }

    #[inline]
    pub fn is_nan(self) -> bool {
        self.class() == Class::Nan
    }

    #[inline]
    pub fn is_sign_negative(self) -> bool {
        self.se & 0x8000 != 0
    }

    fn neg_bit(self) -> bool {
        self.se & 0x8000 != 0
    }

    /// A finite non-zero value as (e, m): the value is m × 2^(e − 63) with bit 63 of m set.
    pub(crate) fn parts(self) -> (i32, u64) {
        let e = (self.se & EXP_MAX) as i32;
        let e = if e == 0 { 1 } else { e }; // denormals share the smallest exponent
        let sh = self.m.leading_zeros();
        (e - BIAS - sh as i32, self.m << sh)
    }

    pub(crate) fn zero(neg: bool) -> F80 {
        F80 { m: 0, se: (neg as u16) << 15 }
    }

    fn inf(neg: bool) -> F80 {
        F80 { m: 1 << 63, se: (neg as u16) << 15 | EXP_MAX }
    }

    pub(crate) fn quiet(self) -> F80 {
        F80 { m: self.m | 0xc000_0000_0000_0000, se: self.se }
    }

    /// The result when at least one operand is a NaN: the operand NaN, made quiet; of two
    /// NaNs the one with the larger significand, and of two with equal significands the
    /// positive one (checked against the processor in the tests).
    pub(crate) fn nan2(a: F80, b: F80) -> F80 {
        match (a.is_nan(), b.is_nan()) {
            (true, false) => a.quiet(),
            (false, true) => b.quiet(),
            _ => {
                if a.m > b.m {
                    a.quiet()
                } else if b.m > a.m {
                    b.quiet()
                } else if a.neg_bit() {
                    b.quiet()
                } else {
                    a.quiet()
                }
            }
        }
    }

    /// Rounds to 64 bits, to nearest even, and packs. The value is `sig` × 2^(exp − 127)
    /// with bit 127 of `sig` set; `sticky` says that non-zero bits lie below `sig`.
    pub(crate) fn round_pack(neg: bool, exp: i32, mut sig: u128, mut sticky: bool) -> F80 {
        debug_assert!(sig >> 127 == 1);
        let mut biased = exp + BIAS;
        if biased <= 0 {
            // below the normal range: a denormal has the exponent of biased 1
            let sh = (1 - biased) as u32;
            if sh >= 128 {
                sticky = true;
                sig = 0;
            } else {
                sticky |= sig & ((1u128 << sh) - 1) != 0;
                sig >>= sh;
            }
            biased = 0;
        }
        let mut hi = (sig >> 64) as u64;
        let lo = sig as u64;
        let half = lo >> 63 != 0;
        let rest = lo << 1 != 0 || sticky;
        if half && (rest || hi & 1 == 1) {
            hi = hi.wrapping_add(1);
            if hi == 0 {
                hi = 1 << 63;
                biased += 1;
            }
        }
        if biased == 0 && hi >> 63 == 1 {
            biased = 1; // rounded up into the normal range
        }
        if biased >= EXP_MAX as i32 {
            return F80::inf(neg);
        }
        F80 { m: hi, se: (neg as u16) << 15 | biased as u16 }
    }

    /// The 256-bit integer `hi:lo` times 2^`e2`, rounded to the register format.
    pub(crate) fn from_wide(neg: bool, hi: u128, lo: u128, e2: i32) -> F80 {
        if hi == 0 && lo == 0 {
            return F80::zero(neg);
        }
        if hi == 0 {
            let lz = lo.leading_zeros();
            return F80::round_pack(neg, 127 - lz as i32 + e2, lo << lz, false);
        }
        let lz = hi.leading_zeros();
        let (sig, sticky) = if lz == 0 { (hi, lo != 0) } else { (hi << lz | lo >> (128 - lz), lo << lz != 0) };
        F80::round_pack(neg, 255 - lz as i32 + e2, sig, sticky)
    }

    // ---- loads (all exact)

    /// `fld m32`
    #[inline]
    pub fn from_f32(v: f32) -> F80 {
        let b = v.to_bits();
        let neg = (b >> 31) as u16;
        let e = ((b >> 23) & 0xff) as i32;
        let frac = (b & 0x7f_ffff) as u64;
        if e == 0xff {
            let m = if frac == 0 { 1 << 63 } else { 0xc000_0000_0000_0000 | frac << 40 };
            return F80 { m, se: neg << 15 | EXP_MAX };
        }
        if e == 0 {
            if frac == 0 {
                return F80 { m: 0, se: neg << 15 };
            }
            let sh = frac.leading_zeros();
            return F80 { m: frac << sh, se: neg << 15 | (-126 + 40 - sh as i32 + BIAS) as u16 };
        }
        F80 { m: 1 << 63 | frac << 40, se: neg << 15 | (e - 127 + BIAS) as u16 }
    }

    /// `fld m64`
    #[inline]
    pub fn from_f64(v: f64) -> F80 {
        let b = v.to_bits();
        let neg = (b >> 63) as u16;
        let e = ((b >> 52) & 0x7ff) as i32;
        let frac = b & 0xf_ffff_ffff_ffff;
        if e == 0x7ff {
            let m = if frac == 0 { 1 << 63 } else { 0xc000_0000_0000_0000 | frac << 11 };
            return F80 { m, se: neg << 15 | EXP_MAX };
        }
        if e == 0 {
            if frac == 0 {
                return F80 { m: 0, se: neg << 15 };
            }
            let sh = frac.leading_zeros();
            return F80 { m: frac << sh, se: neg << 15 | (-1022 + 11 - sh as i32 + BIAS) as u16 };
        }
        F80 { m: 1 << 63 | frac << 11, se: neg << 15 | (e - 1023 + BIAS) as u16 }
    }

    /// `fild m64` (and, widened, `fild m32` and `fild m16`)
    #[inline]
    pub fn from_i64(v: i64) -> F80 {
        if v == 0 {
            return F80::ZERO;
        }
        let a = v.unsigned_abs();
        let sh = a.leading_zeros();
        F80 { m: a << sh, se: ((v < 0) as u16) << 15 | (63 - sh as i32 + BIAS) as u16 }
    }

    #[inline]
    pub fn from_i32(v: i32) -> F80 {
        F80::from_i64(v as i64)
    }

    // ---- stores (round to nearest even)

    /// `fstp m32` / `fst m32`: rounds the register value to single precision.
    #[inline]
    pub fn to_f32(self) -> f32 {
        #[cfg(hw_x87)]
        if let Some(r) = crate::hw::to_f32(self) {
            return r;
        }
        self.soft_to_f32()
    }

    /// [`F80::to_f32`] in the software model.
    pub fn soft_to_f32(self) -> f32 {
        let s = (self.neg_bit() as u32) << 31;
        match self.class() {
            Class::Zero => f32::from_bits(s),
            Class::Inf => f32::from_bits(s | 0x7f80_0000),
            Class::Nan => f32::from_bits(s | 0x7fc0_0000 | ((self.m >> 40) & 0x3f_ffff) as u32),
            Class::Finite => {
                let (exp, mut man) = self.parts();
                let mut e = exp + 127;
                let mut sticky = false;
                if e <= 0 {
                    let sh = (1 - e) as u32;
                    if sh >= 64 {
                        sticky = true;
                        man = 0;
                    } else {
                        sticky = man & ((1u64 << sh) - 1) != 0;
                        man >>= sh;
                    }
                    e = 0;
                }
                let mut hi = man >> 40;
                let half = (man >> 39) & 1 == 1;
                let rest = man & ((1 << 39) - 1) != 0 || sticky;
                if half && (rest || hi & 1 == 1) {
                    hi += 1;
                }
                if e == 0 {
                    if hi >> 23 == 1 {
                        e = 1;
                    }
                } else if hi >> 24 == 1 {
                    hi >>= 1;
                    e += 1;
                }
                if e >= 0xff {
                    return f32::from_bits(s | 0x7f80_0000);
                }
                f32::from_bits(s | (e as u32) << 23 | (hi as u32 & 0x7f_ffff))
            }
        }
    }

    /// `fstp m64` / `fst m64`: rounds the register value to double precision.
    #[inline]
    pub fn to_f64(self) -> f64 {
        #[cfg(hw_x87)]
        if let Some(r) = crate::hw::to_f64(self) {
            return r;
        }
        self.soft_to_f64()
    }

    /// [`F80::to_f64`] in the software model.
    pub fn soft_to_f64(self) -> f64 {
        let s = (self.neg_bit() as u64) << 63;
        match self.class() {
            Class::Zero => f64::from_bits(s),
            Class::Inf => f64::from_bits(s | 0x7ff0_0000_0000_0000),
            Class::Nan => f64::from_bits(s | 0x7ff8_0000_0000_0000 | ((self.m >> 11) & 0x7_ffff_ffff_ffff)),
            Class::Finite => {
                let (exp, mut man) = self.parts();
                let mut e = exp + 1023;
                let mut sticky = false;
                if e <= 0 {
                    let sh = (1 - e) as u32;
                    if sh >= 64 {
                        sticky = true;
                        man = 0;
                    } else {
                        sticky = man & ((1u64 << sh) - 1) != 0;
                        man >>= sh;
                    }
                    e = 0;
                }
                let mut hi = man >> 11;
                let half = (man >> 10) & 1 == 1;
                let rest = man & 0x3ff != 0 || sticky;
                if half && (rest || hi & 1 == 1) {
                    hi += 1;
                }
                if e == 0 {
                    if hi >> 52 == 1 {
                        e = 1;
                    }
                } else if hi >> 53 == 1 {
                    hi >>= 1;
                    e += 1;
                }
                if e >= 0x7ff {
                    return f64::from_bits(s | 0x7ff0_0000_0000_0000);
                }
                f64::from_bits(s | (e as u64) << 52 | (hi & 0xf_ffff_ffff_ffff))
            }
        }
    }

    // ---- arithmetic

    /// `self + b` in the software model (the reference of the hardware backend).
    pub fn soft_add(self, b: F80) -> F80 {
        F80::add_signed(self, b, false)
    }

    /// `self − b` in the software model.
    pub fn soft_sub(self, b: F80) -> F80 {
        F80::add_signed(self, b, true)
    }

    /// `self × b` in the software model.
    pub fn soft_mul(self, b: F80) -> F80 {
        F80::mul_impl(self, b)
    }

    /// `self ÷ b` in the software model.
    pub fn soft_div(self, b: F80) -> F80 {
        F80::div_impl(self, b)
    }

    fn add_signed(a: F80, b: F80, flip_b: bool) -> F80 {
        let bn = b.neg_bit() ^ flip_b;
        match (a.class(), b.class()) {
            (Class::Nan, _) | (_, Class::Nan) => F80::nan2(a, b),
            (Class::Inf, Class::Inf) => {
                if a.neg_bit() == bn {
                    a
                } else {
                    F80::INDEFINITE
                }
            }
            (Class::Inf, _) => a,
            (_, Class::Inf) => F80::inf(bn),
            (Class::Zero, Class::Zero) => F80::zero(a.neg_bit() && bn),
            (Class::Zero, _) => F80 { m: b.m, se: (b.se & EXP_MAX) | (bn as u16) << 15 },
            (_, Class::Zero) => a,
            (Class::Finite, Class::Finite) => {
                let (ea, ma) = a.parts();
                let (eb, mb) = b.parts();
                // x is the operand of larger magnitude
                let (xn, xe, xm, yn, ye, ym) =
                    if (ea, ma) >= (eb, mb) { (a.neg_bit(), ea, ma, bn, eb, mb) } else { (bn, eb, mb, a.neg_bit(), ea, ma) };
                let d = (xe - ye) as u32;
                // one bit of headroom for the carry: the value is x × 2^(xe − 126)
                let x = (xm as u128) << 63;
                let y0 = (ym as u128) << 63;
                let (y, sticky) = if d == 0 {
                    (y0, false)
                } else if d < 127 {
                    (y0 >> d, y0 & ((1u128 << d) - 1) != 0)
                } else {
                    (0, true)
                };
                if xn == yn {
                    let s = x + y;
                    if s >> 127 == 1 {
                        F80::round_pack(xn, xe + 1, s, sticky)
                    } else {
                        F80::round_pack(xn, xe, s << 1, sticky)
                    }
                } else {
                    // bits of y below the window make the true difference a little smaller
                    let s = x - y - sticky as u128;
                    if s == 0 {
                        return F80::ZERO; // exact cancellation gives +0 when rounding to nearest
                    }
                    let lz = s.leading_zeros();
                    F80::round_pack(xn, xe + 1 - lz as i32, s << lz, sticky)
                }
            }
        }
    }

    fn mul_impl(a: F80, b: F80) -> F80 {
        let neg = a.neg_bit() ^ b.neg_bit();
        match (a.class(), b.class()) {
            (Class::Nan, _) | (_, Class::Nan) => F80::nan2(a, b),
            (Class::Inf, Class::Zero) | (Class::Zero, Class::Inf) => F80::INDEFINITE,
            (Class::Inf, _) | (_, Class::Inf) => F80::inf(neg),
            (Class::Zero, _) | (_, Class::Zero) => F80::zero(neg),
            (Class::Finite, Class::Finite) => {
                let (ea, ma) = a.parts();
                let (eb, mb) = b.parts();
                let p = ma as u128 * mb as u128;
                if p >> 127 == 1 {
                    F80::round_pack(neg, ea + eb + 1, p, false)
                } else {
                    F80::round_pack(neg, ea + eb, p << 1, false)
                }
            }
        }
    }

    fn div_impl(a: F80, b: F80) -> F80 {
        let neg = a.neg_bit() ^ b.neg_bit();
        match (a.class(), b.class()) {
            (Class::Nan, _) | (_, Class::Nan) => F80::nan2(a, b),
            (Class::Inf, Class::Inf) | (Class::Zero, Class::Zero) => F80::INDEFINITE,
            (Class::Inf, _) => F80::inf(neg),
            (_, Class::Inf) => F80::zero(neg),
            (Class::Zero, _) => F80::zero(neg),
            (_, Class::Zero) => F80::inf(neg), // division by zero, masked
            (Class::Finite, Class::Finite) => {
                let (ea, ma) = a.parts();
                let (eb, mb) = b.parts();
                let mb = mb as u128;
                let n = (ma as u128) << 64;
                let (q1, r1) = (n / mb, n % mb);
                let n2 = r1 << 64;
                let (q2, r2) = (n2 / mb, n2 % mb);
                if q1 >> 64 != 0 {
                    F80::round_pack(neg, ea - eb, q1 << 63 | q2 >> 1, q2 & 1 != 0 || r2 != 0)
                } else {
                    F80::round_pack(neg, ea - eb - 1, q1 << 64 | q2, r2 != 0)
                }
            }
        }
    }

    /// `fsqrt`: correctly rounded to 64 bits.
    #[inline]
    pub fn sqrt(self) -> F80 {
        #[cfg(hw_x87)]
        if let Some(r) = crate::hw::sqrt(self) {
            return r;
        }
        self.soft_sqrt()
    }

    /// [`F80::sqrt`] in the software model (the reference of the hardware backend).
    pub fn soft_sqrt(self) -> F80 {
        match self.class() {
            Class::Nan => self.quiet(),
            Class::Zero => self,
            _ if self.neg_bit() => F80::INDEFINITE,
            Class::Inf => self,
            Class::Finite => {
                let (e, m) = self.parts();
                // the value is n × 2^k with k even
                let (n, k) = if e & 1 != 0 { ((m as u128) << 64, e - 127) } else { ((m as u128) << 63, e - 126) };
                let root = isqrt(n);
                let rem = n - root as u128 * root as u128;
                // the exact root is above root + ½ exactly when rem > root; a tie cannot occur
                let mut exp = k / 2 + 63;
                let mut man = root;
                if rem > root as u128 {
                    man = man.wrapping_add(1);
                    if man == 0 {
                        man = 1 << 63;
                        exp += 1;
                    }
                }
                F80 { m: man, se: (exp + BIAS) as u16 }
            }
        }
    }

    /// `fprem` with st0 = `self` and st1 = `y`: the partial remainder, and whether the
    /// reduction is incomplete (the C2 flag).
    ///
    /// With D the difference of the two exponents: below 64 the instruction finishes in one
    /// step, st0 − Q × st1 with Q = st0 / st1 truncated toward zero, and the result is exact
    /// (it has the sign of st0 and is smaller than st1 in size). From 64 on it reduces the
    /// exponent by a step and asks to be called again; Intel leaves the size of that step to
    /// the processor ("between 32 and 63"); here it is 32 + D mod 32. The loop
    /// `fprem; fnstsw; sahf; jp` that the compiler emits for C's `fmod` ends in the same
    /// value whatever the step, because the final remainder is unique: that loop is
    /// [`F80::fmod`], and it is what the tests compare with the processor for D ≥ 64.
    pub fn fprem(self, y: F80) -> (F80, bool) {
        match (self.class(), y.class()) {
            (Class::Nan, _) | (_, Class::Nan) => (F80::nan2(self, y), false),
            (Class::Inf, _) | (_, Class::Zero) => (F80::INDEFINITE, false),
            (_, Class::Inf) | (Class::Zero, _) => (self, false),
            (Class::Finite, Class::Finite) => {
                let (ex, mx) = self.parts();
                let (ey, my) = y.parts();
                let d = ex - ey;
                if d < 0 {
                    return (self, false);
                }
                let neg = self.neg_bit();
                if d < 64 {
                    let r = ((mx as u128) << d) % my as u128;
                    (F80::from_wide(neg, 0, r, ey - 63), false)
                } else {
                    let n = 32 + d % 32;
                    let r = ((mx as u128) << n) % my as u128;
                    (F80::from_wide(neg, 0, r, ey - 63 + (d - n)), true)
                }
            }
        }
    }

    /// C's `fmod(self, y)` as the original's code computes it inline: `fprem` repeated while
    /// the unit reports the reduction incomplete. Exact: the result is `self − n × y` for the
    /// whole number n that leaves it smaller than `y` in size, with the sign of `self`.
    pub fn fmod(self, y: F80) -> F80 {
        let (mut r, mut more) = self.fprem(y);
        while more {
            (r, more) = r.fprem(y);
        }
        r
    }

    /// `fscale` with st0 = `self` and st1 = `n`: `self` × 2^trunc(n), rounded to the register
    /// format only if the result leaves the normal range.
    pub fn fscale(self, n: F80) -> F80 {
        match (self.class(), n.class()) {
            (Class::Nan, _) | (_, Class::Nan) => F80::nan2(self, n),
            (Class::Zero, Class::Inf) if !n.neg_bit() => F80::INDEFINITE,
            (Class::Inf, Class::Inf) if n.neg_bit() => F80::INDEFINITE,
            (Class::Inf, _) | (Class::Zero, _) => self,
            (_, Class::Inf) => {
                if n.neg_bit() {
                    F80::zero(self.neg_bit())
                } else {
                    F80::inf(self.neg_bit())
                }
            }
            (Class::Finite, _) => {
                // beyond ±40000 every result is an overflow or an underflow already
                let k = match n.class() {
                    Class::Zero => 0,
                    _ if n.parts().0 >= 20 => {
                        if n.neg_bit() {
                            -40000
                        } else {
                            40000
                        }
                    }
                    _ => n.to_i64(Round::Zero).clamp(-40000, 40000) as i32,
                };
                let (e, m) = self.parts();
                F80::round_pack(self.neg_bit(), e + k, (m as u128) << 64, false)
            }
        }
    }

    /// `fabs`
    #[inline]
    pub fn abs(self) -> F80 {
        F80 { m: self.m, se: self.se & EXP_MAX }
    }

    /// The x87 comparison (`fcom`, `fucom` and their popping forms): `None` is the
    /// "unordered" outcome (C3 = C2 = C0 = 1), which the original's branches treat in
    /// whatever way their flag tests happen to.
    #[inline]
    pub fn compare(self, other: F80) -> Option<Ordering> {
        if let Some(r) = self.fast_compare(other) {
            return r;
        }
        self.soft_compare(other)
    }

    /// [`F80::soft_compare`] read straight from the bits, for two canonical encodings (the
    /// explicit integer bit set exactly when the exponent is nonzero: every value the unit
    /// produces); `None` for any other encoding, which the software model then takes. A NaN
    /// is unordered, zeros of either sign are equal, and otherwise sign-magnitude order on
    /// (exponent, significand) is the order of the values. A comparison does not depend on
    /// the control word, so there is no guard to take; on the hardware backend this is faster
    /// than `fucomip` (which needs both operands stored and loaded as 80 bits) and the
    /// compiler can fold it (`x(c).lt(x(0.0f32))` becomes a test of the float's bits). Equal
    /// to the software model and to the unit: `tests::bits_compare_is_the_model` here, the
    /// differential tests of `hw` for the unit.
    #[inline(always)]
    pub fn bits_compare(self, other: F80) -> Option<Option<Ordering>> {
        #[cfg(test)]
        let mutant = tests::cmp_mutant();
        #[cfg(not(test))]
        let mutant = 0u8;
        let canonical = |v: F80| (v.se & EXP_MAX != 0) == ((v.m as i64) < 0) || mutant == 4;
        if !(canonical(self) && canonical(other)) {
            return None;
        }
        let nan = |v: F80| v.se & EXP_MAX == EXP_MAX && v.m != 1 << 63 && mutant != 2;
        if nan(self) || nan(other) {
            return Some(None);
        }
        let key = |v: F80| {
            // the magnitude, negated for a negative value without a branch (s is 0 or −1)
            let mag = ((v.se & EXP_MAX) as i128) << 64 | v.m as i128;
            let s = -((v.se >> 15) as i128);
            match mutant {
                3 => mag,
                1 => mag ^ s,
                _ => (mag ^ s) - s,
            }
        };
        Some(Some(key(self).cmp(&key(other))))
    }

    /// The comparisons' fast path: [`F80::bits_compare`] on the hardware backend; nothing on
    /// the software build, which keeps [`F80::soft_compare`] alone as before.
    #[inline(always)]
    fn fast_compare(self, other: F80) -> Option<Option<Ordering>> {
        #[cfg(hw_x87)]
        {
            self.bits_compare(other)
        }
        #[cfg(not(hw_x87))]
        {
            let _ = other;
            None
        }
    }

    /// [`F80::compare`] in the software model.
    pub fn soft_compare(self, other: F80) -> Option<Ordering> {
        let (ca, cb) = (self.class(), other.class());
        if ca == Class::Nan || cb == Class::Nan {
            return None;
        }
        if ca == Class::Zero && cb == Class::Zero {
            return Some(Ordering::Equal);
        }
        let key = |x: F80, c: Class| -> (i32, i32, u64) {
            // (sign, exponent, significand), comparable for values of one sign
            match c {
                Class::Zero => (0, i32::MIN, 0),
                Class::Inf => (if x.neg_bit() { -1 } else { 1 }, i32::MAX, 0),
                _ => {
                    let (e, m) = x.parts();
                    (if x.neg_bit() { -1 } else { 1 }, e, m)
                }
            }
        };
        let (sa, ea, ma) = key(self, ca);
        let (sb, eb, mb) = key(other, cb);
        if sa != sb {
            return Some(sa.cmp(&sb));
        }
        let mag = (ea, ma).cmp(&(eb, mb));
        Some(if sa < 0 { mag.reverse() } else { mag })
    }

    /// `st0 < x` as the original's usual test reads it: false when unordered.
    #[inline]
    pub fn lt(self, x: F80) -> bool {
        if let Some(r) = self.fast_compare(x) {
            return r == Some(Ordering::Less);
        }
        self.soft_compare(x) == Some(Ordering::Less)
    }

    /// `st0 > x`: false when unordered.
    #[inline]
    pub fn gt(self, x: F80) -> bool {
        if let Some(r) = self.fast_compare(x) {
            return r == Some(Ordering::Greater);
        }
        self.soft_compare(x) == Some(Ordering::Greater)
    }

    /// `st0 == x`: false when unordered.
    #[allow(clippy::should_implement_trait)]
    #[inline]
    pub fn eq(self, x: F80) -> bool {
        if let Some(r) = self.fast_compare(x) {
            return r == Some(Ordering::Equal);
        }
        self.soft_compare(x) == Some(Ordering::Equal)
    }

    /// `st0 <= x`: false when unordered.
    #[inline]
    pub fn le(self, x: F80) -> bool {
        if let Some(r) = self.fast_compare(x) {
            return matches!(r, Some(Ordering::Less | Ordering::Equal));
        }
        matches!(self.soft_compare(x), Some(Ordering::Less | Ordering::Equal))
    }

    /// `st0 >= x`: false when unordered.
    #[inline]
    pub fn ge(self, x: F80) -> bool {
        if let Some(r) = self.fast_compare(x) {
            return matches!(r, Some(Ordering::Greater | Ordering::Equal));
        }
        matches!(self.soft_compare(x), Some(Ordering::Greater | Ordering::Equal))
    }

    /// True when the comparison is unordered (an operand is a NaN).
    #[inline]
    pub fn unordered(self, x: F80) -> bool {
        if let Some(r) = self.fast_compare(x) {
            return r.is_none();
        }
        self.soft_compare(x).is_none()
    }

    /// `frndint` under the given rounding control: the value rounded to an integer, kept in
    /// the register format. The sign survives, so −0.25 rounds to −0.
    #[inline]
    pub fn round_int(self, mode: Round) -> F80 {
        #[cfg(hw_x87)]
        if let Some(r) = crate::hw::round_int(self, mode) {
            return r;
        }
        self.soft_round_int(mode)
    }

    /// [`F80::round_int`] in the software model.
    pub fn soft_round_int(self, mode: Round) -> F80 {
        if self.class() != Class::Finite {
            return if self.is_nan() { self.quiet() } else { self };
        }
        let neg = self.neg_bit();
        let (e, m) = self.parts();
        if e >= 63 {
            return self;
        }
        // integer part and what lies below it
        let (int, half, rest) = if e < 0 {
            (0u64, e == -1, e < -1 || m << 1 != 0)
        } else {
            let fb = 63 - e as u32; // fraction bits, 1..=63
            let frac = m & ((1u64 << fb) - 1);
            (m >> fb, frac >> (fb - 1) == 1, frac & ((1u64 << (fb - 1)) - 1) != 0)
        };
        let up = match mode {
            Round::Nearest => half && (rest || int & 1 == 1),
            Round::Zero => false,
            Round::Down => neg && (half || rest),
            Round::Up => !neg && (half || rest),
        };
        let int = int + up as u64; // at most 2^63
        if int == 0 {
            return F80::zero(neg);
        }
        let sh = int.leading_zeros();
        F80 { m: int << sh, se: (neg as u16) << 15 | (63 - sh as i32 + BIAS) as u16 }
    }

    /// `fistp m32` under the given rounding control. A NaN, an infinity or a value that does
    /// not fit gives the "integer indefinite", 0x80000000.
    #[inline]
    pub fn to_i32(self, mode: Round) -> i32 {
        #[cfg(hw_x87)]
        if let Some(r) = crate::hw::to_i32(self, mode) {
            return r;
        }
        self.soft_to_i32(mode)
    }

    /// [`F80::to_i32`] in the software model.
    pub fn soft_to_i32(self, mode: Round) -> i32 {
        match self.soft_to_i64(mode) {
            v if v >= i32::MIN as i64 && v <= i32::MAX as i64 => v as i32,
            _ => i32::MIN,
        }
    }

    /// `fistp m16`; the integer indefinite is 0x8000.
    #[inline]
    pub fn to_i16(self, mode: Round) -> i16 {
        #[cfg(hw_x87)]
        if let Some(r) = crate::hw::to_i16(self, mode) {
            return r;
        }
        self.soft_to_i16(mode)
    }

    /// [`F80::to_i16`] in the software model.
    pub fn soft_to_i16(self, mode: Round) -> i16 {
        match self.soft_to_i64(mode) {
            v if v >= i16::MIN as i64 && v <= i16::MAX as i64 => v as i16,
            _ => i16::MIN,
        }
    }

    /// `fistp m64`; the integer indefinite is 0x8000000000000000.
    #[inline]
    pub fn to_i64(self, mode: Round) -> i64 {
        #[cfg(hw_x87)]
        if let Some(r) = crate::hw::to_i64(self, mode) {
            return r;
        }
        self.soft_to_i64(mode)
    }

    /// [`F80::to_i64`] in the software model.
    pub fn soft_to_i64(self, mode: Round) -> i64 {
        let r = self.soft_round_int(mode);
        match r.class() {
            Class::Zero => 0,
            Class::Finite => {
                let (e, m) = r.parts();
                if e > 63 {
                    return i64::MIN;
                }
                let mag = m >> (63 - e);
                if r.neg_bit() {
                    if mag > 1 << 63 {
                        i64::MIN
                    } else {
                        (mag as i64).wrapping_neg()
                    }
                } else if mag >= 1 << 63 {
                    i64::MIN
                } else {
                    mag as i64
                }
            }
            _ => i64::MIN,
        }
    }
}

/// Integer square root of a value in [2^126, 2^128).
fn isqrt(n: u128) -> u64 {
    let mut r = ((n as f64).sqrt() as u128).min(u64::MAX as u128);
    r = (r + n / r) >> 1;
    r = ((r + n / r) >> 1).min(u64::MAX as u128);
    while r * r > n {
        r -= 1;
    }
    while r < u64::MAX as u128 && (r + 1) * (r + 1) <= n {
        r += 1;
    }
    r as u64
}

impl core::fmt::Debug for F80 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "F80({:04x}:{:016x} ~{:e})", self.se, self.m, self.to_f64())
    }
}

impl From<f32> for F80 {
    #[inline]
    fn from(v: f32) -> F80 {
        F80::from_f32(v)
    }
}

impl From<f64> for F80 {
    #[inline]
    fn from(v: f64) -> F80 {
        F80::from_f64(v)
    }
}

impl From<i32> for F80 {
    #[inline]
    fn from(v: i32) -> F80 {
        F80::from_i32(v)
    }
}

/// A `float` moved through the unit (`fld m32` then `fstp m32`), which is how the original
/// copies most floats. The value is unchanged except that a signalling NaN comes out quiet.
#[inline]
pub fn fmov(v: f32) -> f32 {
    F80::from_f32(v).to_f32()
}

/// The same for a `double` (`fld m64`, `fstp m64`).
#[inline]
pub fn fmov64(v: f64) -> f64 {
    F80::from_f64(v).to_f64()
}

/// Shorthand for a load: `x(v)` is the register value after `fld` of a `float` or `double`.
#[inline]
pub fn x<T: Into<F80>>(v: T) -> F80 {
    v.into()
}

impl Neg for F80 {
    type Output = F80;
    /// `fchs`
    #[inline]
    fn neg(self) -> F80 {
        F80 { m: self.m, se: self.se ^ 0x8000 }
    }
}

// Every arithmetic operator goes through the hardware backend where there is one (it answers
// `None` for what it does not take, and the software model answers that); a memory operand
// (`fadd m32`, `fmul m64`, …) is loaded exactly, as `fld` does, then used.
macro_rules! binop {
    ($tr:ident, $f:ident, $soft:ident, $hw:ident, $hw32:ident, $hw32r:ident, $hw64:ident, $hw64r:ident) => {
        impl $tr for F80 {
            type Output = F80;
            #[inline]
            fn $f(self, rhs: F80) -> F80 {
                #[cfg(hw_x87)]
                if let Some(r) = crate::hw::$hw(self, rhs) {
                    return r;
                }
                F80::$soft(self, rhs)
            }
        }
        impl $tr<f32> for F80 {
            type Output = F80;
            #[inline]
            fn $f(self, rhs: f32) -> F80 {
                #[cfg(hw_x87)]
                if let Some(r) = crate::hw::$hw32(self, rhs) {
                    return r;
                }
                F80::$soft(self, F80::from_f32(rhs))
            }
        }
        impl $tr<f64> for F80 {
            type Output = F80;
            #[inline]
            fn $f(self, rhs: f64) -> F80 {
                #[cfg(hw_x87)]
                if let Some(r) = crate::hw::$hw64(self, rhs) {
                    return r;
                }
                F80::$soft(self, F80::from_f64(rhs))
            }
        }
        impl $tr<F80> for f32 {
            type Output = F80;
            #[inline]
            fn $f(self, rhs: F80) -> F80 {
                #[cfg(hw_x87)]
                if let Some(r) = crate::hw::$hw32r(self, rhs) {
                    return r;
                }
                F80::$soft(F80::from_f32(self), rhs)
            }
        }
        impl $tr<F80> for f64 {
            type Output = F80;
            #[inline]
            fn $f(self, rhs: F80) -> F80 {
                #[cfg(hw_x87)]
                if let Some(r) = crate::hw::$hw64r(self, rhs) {
                    return r;
                }
                F80::$soft(F80::from_f64(self), rhs)
            }
        }
    };
}

binop!(Add, add, soft_add, add, add_f32, f32_add, add_f64, f64_add);
binop!(Sub, sub, soft_sub, sub, sub_f32, f32_sub, sub_f64, f64_sub);
binop!(Mul, mul, soft_mul, mul, mul_f32, f32_mul, mul_f64, f64_mul);
binop!(Div, div, soft_div, div, div_f32, f32_div, div_f64, f64_div);

/// Test support shared with the crates built on this one: a generator of register values.
pub mod testing {
    use super::*;

    /// A small deterministic generator (xorshift64*), so the tests need no dependency.
    pub struct Rng(pub u64);
    impl Rng {
        pub fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
        /// A register value of a random kind: mostly finite, with every special case present.
        pub fn f80(&mut self) -> F80 {
            let r = self.next();
            let neg = ((r >> 60) & 1) as u16;
            match r % 64 {
                0 => F80::zero(neg == 1),
                1 => F80::inf(neg == 1),
                2 => F80 { m: 0xc000_0000_0000_0000 | (self.next() >> 2), se: neg << 15 | EXP_MAX },
                3 => F80 { m: 0x8000_0000_0000_0000 | (self.next() >> 2) | 1, se: neg << 15 | EXP_MAX }, // signalling
                4 => F80 { m: self.next() >> (1 + self.next() % 63), se: neg << 15 }, // denormal
                5..=7 => F80 { m: 1 << 63 | self.next(), se: neg << 15 | (1 + self.next() % 40) as u16 }, // tiny
                8..=10 => F80 { m: 1 << 63 | self.next(), se: neg << 15 | (EXP_MAX - 1 - (self.next() % 40) as u16) }, // huge
                11..=20 => F80::from_f32(f32::from_bits(self.next() as u32)),
                21..=30 => F80::from_f64(f64::from_bits(self.next())),
                31..=40 => {
                    // few significant bits: sums and products that are exact or exact ties
                    let bits = 1 + self.next() % 12;
                    let m = (1u64 << 63) | ((self.next() >> (64 - bits)) << (63 - bits - self.next() % 40));
                    F80 { m, se: neg << 15 | (BIAS + (self.next() % 80) as i32 - 40) as u16 }
                }
                _ => F80 { m: 1 << 63 | self.next(), se: neg << 15 | (BIAS + (self.next() % 200) as i32 - 100) as u16 },
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) use super::testing::Rng;

    fn bits(v: F80) -> String {
        format!("{:04x}:{:016x}", v.se, v.m)
    }

    std::thread_local! {
        static CMP_MUTANT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    }
    /// The deliberately wrong variant of [`F80::bits_compare`] this thread runs: 0 none,
    /// 1 −0 below +0, 2 a NaN taken for an infinity, 3 the sign ignored, 4 every encoding
    /// read from the bits (the pseudo encodings too).
    pub(crate) fn cmp_mutant() -> u8 {
        CMP_MUTANT.with(|m| m.get())
    }

    /// Pairs on which [`F80::bits_compare`] or the comparisons (`compare`, `lt`, `gt`, `le`,
    /// `ge`, `eq`, `unordered`; through it on the hardware backend) and the software model
    /// disagree, of how many: every
    /// pair of a list of edge encodings (canonical and not), then `n` random pairs, a third
    /// of them close (one or both of sign, exponent and low bits shared).
    fn bits_compare_cases(n: u64) -> (u64, u64, String) {
        let edges: Vec<F80> = [
            (0, 0), (1, 0), (0x7fff_ffff_ffff_ffff, 0), (1 << 63, 1), (1 << 63 | 1, 1), (1 << 63, 0x3fff), (1 << 63 | 1, 0x3fff),
            (u64::MAX, 0x3ffe), (u64::MAX, 0x7ffe), (1 << 63, 0x7fff), (0xc000_0000_0000_0000, 0x7fff), (0x8000_0000_0000_0001, 0x7fff),
            (0xffff_ffff_ffff_ffff, 0x7fff),
            // not produced by the unit: unnormals, pseudo-denormals, pseudo-infinity, pseudo-NaN, an unnormal zero
            (0x4000_0000_0000_0000, 0x3fff), (1 << 63, 0), (u64::MAX, 0), (0, 0x7fff), (0x4000_0000_0000_0001, 0x7fff), (0, 0x3fff), (1, 0x0001),
        ]
        .iter()
        .flat_map(|&(m, e)| [F80 { m, se: e }, F80 { m, se: e | 0x8000 }])
        .collect();
        let (mut compared, mut different, mut first) = (0u64, 0u64, String::new());
        let mut check = |a: F80, b: F80| {
            let c = a.soft_compare(b);
            // bits_compare itself (both builds), then the operators (through it on hw only)
            let ok = a.bits_compare(b).is_none_or(|r| r == c)
                && a.compare(b) == c
                && a.lt(b) == (c == Some(Ordering::Less))
                && a.gt(b) == (c == Some(Ordering::Greater))
                && a.le(b) == matches!(c, Some(Ordering::Less | Ordering::Equal))
                && a.ge(b) == matches!(c, Some(Ordering::Greater | Ordering::Equal))
                && a.eq(b) == (c == Some(Ordering::Equal))
                && a.unordered(b) == c.is_none();
            compared += 1;
            if !ok {
                if different == 0 {
                    first = format!("{} {}", bits(a), bits(b));
                }
                different += 1;
            }
        };
        for &a in &edges {
            for &b in &edges {
                check(a, b);
            }
        }
        let mut rng = Rng(0x5eed_c0de_1234_5679);
        for i in 0..n {
            let a = rng.f80();
            let b = match i % 6 {
                0 => F80 { m: a.m ^ (rng.next() & 0xff), se: a.se },
                1 => F80 { m: a.m, se: a.se ^ 0x8000 },
                2 => F80 { m: rng.next() | 1 << 63, se: a.se },
                _ => rng.f80(),
            };
            check(a, b);
            check(b, a);
        }
        (compared, different, first)
    }

    #[test]
    fn bits_compare_is_the_model() {
        let n = std::env::var("SI_X87_HW_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(1_000_000u64);
        let (compared, different, first) = bits_compare_cases(n);
        println!("bits_compare: {compared} pairs, {different} different");
        assert_eq!(different, 0, "first difference {first}");
        for (m, name) in [(1u8, "-0 below +0"), (2, "NaN as infinity"), (3, "sign ignored"), (4, "pseudo encodings from the bits")] {
            CMP_MUTANT.with(|c| c.set(m));
            let (compared, different, _) = bits_compare_cases(n / 10);
            CMP_MUTANT.with(|c| c.set(0));
            println!("bits_compare mutant {name}: {different} of {compared} pairs different");
            assert!(different > 0, "the mutant {name} went unnoticed");
        }
    }

    #[test]
    fn loads_and_stores_round_trip() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..200_000 {
            let a = f32::from_bits(rng.next() as u32);
            if !a.is_nan() {
                assert_eq!(F80::from_f32(a).to_f32().to_bits(), a.to_bits());
                assert_eq!(F80::from_f32(a).to_f64().to_bits(), (a as f64).to_bits());
            }
            let d = f64::from_bits(rng.next());
            if !d.is_nan() {
                assert_eq!(F80::from_f64(d).to_f64().to_bits(), d.to_bits());
                assert_eq!(F80::from_f64(d).to_f32().to_bits(), (d as f32).to_bits(), "{d:e}");
            }
            let i = rng.next() as i64;
            assert_eq!(F80::from_i64(i).to_i64(Round::Nearest), i);
            assert_eq!(F80::from_i32(i as i32).to_f64(), i as i32 as f64);
        }
        assert_eq!(bits(F80::from_f32(1.0)), "3fff:8000000000000000");
        assert_eq!(bits(F80::from_f64(-2.5)), "c000:a000000000000000");
        assert_eq!(bits(F80::from_f32(f32::from_bits(1))), "3f6a:8000000000000000"); // 2^-149
        assert_eq!(F80::from_f64(core::f64::consts::PI).to_f64(), core::f64::consts::PI);
        assert_eq!(F80::PI.to_f64(), core::f64::consts::PI);
    }

    #[test]
    fn single_operations_on_floats_equal_float_arithmetic() {
        // one operation on float operands, stored to a float: the 64-bit intermediate is wide
        // enough (2 × 24 + 2 bits) that rounding twice equals rounding once
        let mut rng = Rng(42);
        for _ in 0..300_000 {
            let a = f32::from_bits(rng.next() as u32);
            let b = f32::from_bits(rng.next() as u32);
            if a.is_nan() || b.is_nan() {
                continue;
            }
            let (xa, xb) = (F80::from(a), F80::from(b));
            for (got, want) in [((xa + xb).to_f32(), a + b), ((xa - xb).to_f32(), a - b), ((xa * xb).to_f32(), a * b), ((xa / xb).to_f32(), a / b)] {
                assert!(got.to_bits() == want.to_bits() || (got.is_nan() && want.is_nan()), "{a:e} {b:e}: {got:e} {want:e}");
            }
            if a >= 0.0 {
                assert_eq!(xa.sqrt().to_f32().to_bits(), a.sqrt().to_bits());
            }
        }
    }

    #[test]
    fn double_rounding_differs_from_double_arithmetic() {
        // the classic case: 1 + (2^-53 + 2^-64) is above the midpoint between two doubles,
        // but the 64-bit register rounds it to exactly the midpoint first, and the store then
        // rounds the tie to even
        let a = 1.0f64;
        let b = f64::from_bits(0x3ca0_0000_0000_0000) + f64::from_bits(0x3bf0_0000_0000_0000); // 2^-53 + 2^-64
        assert_eq!(a + b, 1.0 + f64::EPSILON); // correctly rounded double arithmetic rounds up
        assert_eq!((x(a) + b).to_f64(), 1.0); // the x87 result
    }

    #[test]
    fn special_values() {
        let (z, o, inf) = (F80::ZERO, F80::ONE, F80::INFINITY);
        assert_eq!(bits(o - o), "0000:0000000000000000");
        assert_eq!(bits(-z + -z), "8000:0000000000000000");
        assert_eq!(bits(-z + z), "0000:0000000000000000");
        assert_eq!(bits(o / z), "7fff:8000000000000000");
        assert_eq!(bits(-o / z), "ffff:8000000000000000");
        assert_eq!(bits(z / z), "ffff:c000000000000000");
        assert_eq!(bits(inf - inf), "ffff:c000000000000000");
        assert_eq!(bits(inf * z), "ffff:c000000000000000");
        assert_eq!(bits((-o).sqrt()), "ffff:c000000000000000");
        assert_eq!(bits((-z).sqrt()), "8000:0000000000000000");
        assert!((z / z).to_f32().is_nan());
        assert_eq!((z / z).to_f32().to_bits(), 0xffc0_0000);
        assert_eq!((z / z).to_f64().to_bits(), 0xfff8_0000_0000_0000);
        assert_eq!(z.compare(-z), Some(Ordering::Equal));
        assert_eq!((z / z).compare(o), None);
        assert!(x(-1.0f32).lt(z) && o.gt(z) && !(z / z).lt(o) && !(z / z).ge(o));
        assert_eq!(bits(x(2.0f32).sqrt()), "3fff:b504f333f9de6484");
    }

    #[test]
    fn rounding_to_integers() {
        let cases: [(f64, [f64; 4]); 8] = [
            // value, [nearest, down, up, zero]
            (2.5, [2.0, 2.0, 3.0, 2.0]),
            (3.5, [4.0, 3.0, 4.0, 3.0]),
            (-2.5, [-2.0, -3.0, -2.0, -2.0]),
            (0.5, [0.0, 0.0, 1.0, 0.0]),
            (0.75, [1.0, 0.0, 1.0, 0.0]),
            (-0.25, [-0.0, -1.0, -0.0, -0.0]),
            (1e20, [1e20, 1e20, 1e20, 1e20]),
            (-7.0, [-7.0, -7.0, -7.0, -7.0]),
        ];
        for (v, want) in cases {
            for (mode, w) in [Round::Nearest, Round::Down, Round::Up, Round::Zero].into_iter().zip(want) {
                assert_eq!(x(v).round_int(mode).to_f64().to_bits(), w.to_bits(), "{v} {mode:?}");
            }
        }
        assert_eq!(x(2.5f64).to_i32(Round::Nearest), 2);
        assert_eq!(x(-2.5f64).to_i32(Round::Zero), -2);
        assert_eq!(x(3e9f64).to_i32(Round::Zero), i32::MIN);
        assert_eq!(x(-2147483648.0f64).to_i32(Round::Zero), i32::MIN);
        assert_eq!(x(2147483647.5f64).to_i32(Round::Zero), i32::MAX);
        assert_eq!(x(2147483647.5f64).to_i32(Round::Nearest), i32::MIN); // rounds to 2^31: does not fit
        assert_eq!(x(f64::NAN).to_i32(Round::Zero), i32::MIN);
        assert_eq!(x(40000.0f64).to_i16(Round::Zero), i16::MIN);
        assert_eq!(x(-32768.4f64).to_i16(Round::Zero), -32768);
    }

    /// The processor's own x87 unit, where there is one. Nothing of the original program is
    /// run here: these are single instructions on our own operands.
    #[cfg(target_arch = "x86_64")]
    pub(crate) mod hw {
        use super::super::*;
        use core::arch::asm;

        fn tb(v: F80) -> [u8; 16] {
            let mut b = [0u8; 16];
            b[..8].copy_from_slice(&v.m.to_le_bytes());
            b[8..10].copy_from_slice(&v.se.to_le_bytes());
            b
        }

        fn ft(b: &[u8; 16]) -> F80 {
            F80 { m: u64::from_le_bytes(b[..8].try_into().unwrap()), se: u16::from_le_bytes(b[8..10].try_into().unwrap()) }
        }

        /// Runs `code` (raw x87 opcode bytes, chosen by `op`) with st0 = b, st1 = a under the
        /// control word `cw`, and returns st0 afterwards.
        pub fn run(op: u8, a: F80, b: F80, cw: u16) -> F80 {
            let (ta, tb_, mut out) = (tb(a), tb(b), [0u8; 16]);
            let mut old: u16 = 0;
            unsafe {
                asm!(
                    "fnstcw word ptr [{old}]",
                    "fldcw word ptr [{cw}]",
                    "fld tbyte ptr [{a}]",
                    "fld tbyte ptr [{b}]",
                    "cmp {op:e}, 0", "jne 2f", ".byte 0xde, 0xc1", "jmp 9f",       // faddp: st1 = st1 + st0
                    "2:", "cmp {op:e}, 1", "jne 3f", ".byte 0xde, 0xe9", "jmp 9f", // fsubp: st1 = st1 - st0
                    "3:", "cmp {op:e}, 2", "jne 4f", ".byte 0xde, 0xc9", "jmp 9f", // fmulp: st1 = st1 * st0
                    "4:", "cmp {op:e}, 3", "jne 5f", ".byte 0xde, 0xf9", "jmp 9f", // fdivp: st1 = st1 / st0
                    "5:", "cmp {op:e}, 4", "jne 6f", ".byte 0xd9, 0xfa", ".byte 0xdd, 0xd9", "jmp 9f", // fsqrt; fstp st1
                    "6:", ".byte 0xd9, 0xfc", ".byte 0xdd, 0xd9",                  // frndint; fstp st1
                    "9:",
                    "fstp tbyte ptr [{o}]",
                    "fnclex",
                    "fldcw word ptr [{old}]",
                    old = in(reg) &mut old, cw = in(reg) &cw, a = in(reg) ta.as_ptr(), b = in(reg) tb_.as_ptr(),
                    o = in(reg) out.as_mut_ptr(), op = in(reg) op as u32,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                    out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                );
            }
            ft(&out)
        }

        pub fn store_f32(a: F80) -> u32 {
            let ta = tb(a);
            let mut out: u32 = 0;
            let (cw, mut old) = (0x037fu16, 0u16);
            unsafe {
                asm!("fnstcw word ptr [{old}]", "fldcw word ptr [{cw}]", "fld tbyte ptr [{a}]", "fstp dword ptr [{o}]", "fnclex", "fldcw word ptr [{old}]",
                    old = in(reg) &mut old, cw = in(reg) &cw, a = in(reg) ta.as_ptr(), o = in(reg) &mut out,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _, out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _);
            }
            out
        }

        pub fn store_f64(a: F80) -> u64 {
            let ta = tb(a);
            let mut out: u64 = 0;
            let (cw, mut old) = (0x037fu16, 0u16);
            unsafe {
                asm!("fnstcw word ptr [{old}]", "fldcw word ptr [{cw}]", "fld tbyte ptr [{a}]", "fstp qword ptr [{o}]", "fnclex", "fldcw word ptr [{old}]",
                    old = in(reg) &mut old, cw = in(reg) &cw, a = in(reg) ta.as_ptr(), o = in(reg) &mut out,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _, out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _);
            }
            out
        }

        pub fn store_i32(a: F80, cw: u16) -> i32 {
            let ta = tb(a);
            let mut out: i32 = 0;
            let mut old = 0u16;
            unsafe {
                asm!("fnstcw word ptr [{old}]", "fldcw word ptr [{cw}]", "fld tbyte ptr [{a}]", "fistp dword ptr [{o}]", "fnclex", "fldcw word ptr [{old}]",
                    old = in(reg) &mut old, cw = in(reg) &cw, a = in(reg) ta.as_ptr(), o = in(reg) &mut out,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _, out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _);
            }
            out
        }

        pub fn load_f32(v: u32) -> F80 {
            let mut out = [0u8; 16];
            unsafe {
                asm!("fld dword ptr [{a}]", "fstp tbyte ptr [{o}]", "fnclex", a = in(reg) &v, o = in(reg) out.as_mut_ptr(),
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _, out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _);
            }
            ft(&out)
        }

        pub fn load_f64(v: u64) -> F80 {
            let mut out = [0u8; 16];
            unsafe {
                asm!("fld qword ptr [{a}]", "fstp tbyte ptr [{o}]", "fnclex", a = in(reg) &v, o = in(reg) out.as_mut_ptr(),
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _, out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _);
            }
            ft(&out)
        }

        /// `fprem` (op 0) or `fscale` (op 1) with st0 = a and st1 = b: st0 afterwards and
        /// the status word.
        pub fn two(op: u8, a: F80, b: F80) -> (F80, u16) {
            let (ta, tb_, mut out) = (tb(a), tb(b), [0u8; 16]);
            let mut sw: u16 = 0;
            let (cw, mut old) = (0x037fu16, 0u16);
            unsafe {
                asm!(
                    "fnstcw word ptr [{old}]", "fldcw word ptr [{cw}]", "fnclex",
                    "fld tbyte ptr [{b}]", "fld tbyte ptr [{a}]",
                    "cmp {op:e}, 0", "jne 2f", ".byte 0xd9, 0xf8", "jmp 9f",       // fprem
                    "2:", ".byte 0xd9, 0xfd",                                      // fscale
                    "9:", "fnstsw word ptr [{s}]",
                    "fstp tbyte ptr [{o}]", ".byte 0xdd, 0xd8", "fnclex", "fldcw word ptr [{old}]",
                    old = in(reg) &mut old, cw = in(reg) &cw, a = in(reg) ta.as_ptr(), b = in(reg) tb_.as_ptr(),
                    o = in(reg) out.as_mut_ptr(), s = in(reg) &mut sw, op = in(reg) op as u32,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                    out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                );
            }
            (ft(&out), sw)
        }

        /// 0 less, 1 equal, 2 greater, 3 unordered: `fucompp` of st0 = a against st1 = b.
        pub fn compare(a: F80, b: F80) -> u8 {
            let (ta, tb_) = (tb(a), tb(b));
            let mut sw: u16 = 0;
            unsafe {
                asm!("fld tbyte ptr [{b}]", "fld tbyte ptr [{a}]", ".byte 0xda, 0xe9", "fnstsw word ptr [{s}]", "fnclex",
                    a = in(reg) ta.as_ptr(), b = in(reg) tb_.as_ptr(), s = in(reg) &mut sw,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _, out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _);
            }
            match (sw >> 8) & 0x45 {
                0x00 => 2,
                0x01 => 0,
                0x40 => 1,
                _ => 3,
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn every_operation_matches_the_processor() {
        // SI_MATH_SOAK=k runs k times as many cases, each batch from another seed
        let rounds: u64 = std::env::var("SI_MATH_SOAK").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
        for round in 0..rounds {
            compare_with_the_processor(0x1234_5678_9abc_def1 ^ round.wrapping_mul(0x9e37_79b9_7f4a_7c15), 3_000_000);
        }
    }

    /// `fprem`, the `fmod` loop built on it, and `fscale` against the processor.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn remainder_and_scale_match_the_processor() {
        let mut rng = Rng(0x0f1e_2d3c_4b5a_6978);
        let (mut complete, mut partial) = (0u32, 0u32);
        for i in 0..1_500_000u64 {
            let a = rng.f80();
            let mut b = rng.f80();
            if i % 3 == 0 && a.class() == Class::Finite {
                // exponents close enough for the one-step reduction, and quotients worth having
                b = F80 { m: 1 << 63 | rng.next(), se: (b.se & 0x8000) | ((a.se & EXP_MAX).wrapping_sub((rng.next() % 70) as u16) & EXP_MAX).max(1) };
            }
            for v in [a, b] {
                if ((v.se & EXP_MAX != 0) != (v.m >> 63 == 1) && v.m != 0) || (v.se & EXP_MAX != 0 && v.m == 0) {
                    b = F80::ONE; // an unnormal: not something a load or an operation produces
                }
            }
            if ((a.se & EXP_MAX != 0) != (a.m >> 63 == 1) && a.m != 0) || (a.se & EXP_MAX != 0 && a.m == 0) {
                continue;
            }
            let (want, sw) = hw::two(0, a, b);
            let (got, more) = a.fprem(b);
            if sw & 0x0400 == 0 {
                assert!(!more, "fprem {} {}: the processor finished in one step", bits(a), bits(b));
                assert_eq!(bits(got), bits(want), "fprem {} {}", bits(a), bits(b));
                complete += 1;
            } else {
                assert!(more, "fprem {} {}: the processor did not finish in one step", bits(a), bits(b));
                // the step is the processor's own; the end of the loop is not
                let (mut r, mut s) = (want, sw);
                while s & 0x0400 != 0 {
                    (r, s) = hw::two(0, r, b);
                }
                assert_eq!(bits(a.fmod(b)), bits(r), "fmod {} {}", bits(a), bits(b));
                partial += 1;
            }
            let n = if i % 2 == 0 { F80::from_i32((rng.next() % 40000) as i32 - 20000) } else { b };
            assert_eq!(bits(a.fscale(n)), bits(hw::two(1, a, n).0), "fscale {} {}", bits(a), bits(n));
        }
        assert!(complete > 400_000 && partial > 100_000, "{complete} one-step and {partial} partial reductions");
    }

    #[cfg(target_arch = "x86_64")]
    fn compare_with_the_processor(seed: u64, n: u64) {
        let mut rng = Rng(seed);
        for i in 0..n {
            let (a, b) = (rng.f80(), rng.f80());
            // sometimes make the operands close, for cancellation and ties
            let b = if i % 7 == 0 { F80 { m: a.m ^ (rng.next() & 0xff), se: a.se ^ ((rng.next() % 3 == 0) as u16) << 15 } } else { b };
            let b = if i % 11 == 0 { F80 { m: b.m, se: (b.se & 0x8000) | ((a.se & EXP_MAX).wrapping_add((rng.next() % 140) as u16).wrapping_sub(70) & EXP_MAX) } } else { b };
            if (b.se & EXP_MAX != 0) != (b.m >> 63 == 1) && b.m != 0 || (b.se & EXP_MAX != 0 && b.m == 0) {
                continue; // an unnormal or pseudo value: not something a load or an operation produces
            }
            // the software model itself (the operators may run on the unit: crate::hw)
            let ops: [(u8, F80, &str); 4] = [(0, a.soft_add(b), "+"), (1, a.soft_sub(b), "-"), (2, a.soft_mul(b), "*"), (3, a.soft_div(b), "/")];
            for (op, got, name) in ops {
                let want = hw::run(op, a, b, 0x037f);
                assert_eq!(bits(got), bits(want), "{} {name} {}", bits(a), bits(b));
            }
            assert_eq!(bits(a.soft_sqrt()), bits(hw::run(4, a, a, 0x037f)), "sqrt {}", bits(a));
            for (mode, cw) in [(Round::Nearest, 0x037f), (Round::Down, 0x077f), (Round::Up, 0x0b7f), (Round::Zero, 0x0f7f)] {
                assert_eq!(bits(a.soft_round_int(mode)), bits(hw::run(5, a, a, cw)), "rndint {mode:?} {}", bits(a));
                assert_eq!(a.soft_to_i32(mode), hw::store_i32(a, cw), "fist {mode:?} {}", bits(a));
            }
            assert_eq!(a.soft_to_f32().to_bits(), hw::store_f32(a), "to_f32 {}", bits(a));
            assert_eq!(a.soft_to_f64().to_bits(), hw::store_f64(a), "to_f64 {}", bits(a));
            let c = match a.soft_compare(b) {
                Some(Ordering::Less) => 0,
                Some(Ordering::Equal) => 1,
                Some(Ordering::Greater) => 2,
                None => 3,
            };
            assert_eq!(c, hw::compare(a, b), "compare {} {}", bits(a), bits(b));
            let (f, d) = (rng.next() as u32, rng.next());
            assert_eq!(bits(F80::from_f32(f32::from_bits(f))), bits(hw::load_f32(f)), "load {f:08x}");
            assert_eq!(bits(F80::from_f64(f64::from_bits(d))), bits(hw::load_f64(d)), "load {d:016x}");
        }
    }
}
