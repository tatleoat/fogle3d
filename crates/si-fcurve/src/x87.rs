// SPDX-License-Identifier: MIT
//! The arithmetic of the x87 unit (finding 0009, "The x87 analysis").
//!
//! The Linux `soft` binary is i386 code compiled by GCC 2.95: every floating-point expression
//! is evaluated in the x87 registers, 80 bits wide, and is rounded to `float` or `double` only
//! when it is stored to memory. Nothing in the binary changes the precision control of the
//! unit (it contains no `finit` and no `fldcw` of a constant; each `fldcw` belongs to a
//! float-to-integer conversion and changes the rounding field only), so the precision is the
//! Linux default: a 64-bit significand, round to nearest even.
//!
//! [`X87`] is one register value. `+ - * /` round to 64 bits exactly as `fadd`, `fsub`,
//! `fmul` and `fdiv` do; [`X87::to_f32`] and [`X87::to_f64`] are the stores (`fstp m32`,
//! `fstp m64`); [`X87::from_f32`], [`X87::from_f64`] and [`X87::from_i32`] are the loads
//! (`fld m32`, `fld m64`, `fild m32`), which are exact.
//!
//! The arithmetic itself is `si_x87::F80`, the software unit every crate shares: this type
//! keeps the shape the curve code and its users were written against (three public fields,
//! the method names below) and hands each operation to it. Until finding 0011 the crate had
//! a second software unit of its own here; the emulation check of the curve code gives the
//! same results with either. The tests compare each operation with the x87 unit of the
//! machine they run on, when that is an x86-64.
//!
//! Not modelled: the exception flags, and a control word other than the default.

use si_x87::{Round, F80};
use std::cmp::Ordering;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// A value in an x87 register: sign, 15-bit biased exponent, 64-bit significand with an
/// explicit integer bit (the layout of the 10-byte `long double`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct X87 {
    pub sign: bool,
    pub exp: u16,
    pub man: u64,
}

const EXP_MAX: u16 = 0x7fff;
const INT_BIT: u64 = 1 << 63;
const QUIET: u64 = 1 << 62;

impl X87 {
    pub const ZERO: X87 = X87 { sign: false, exp: 0, man: 0 };
    pub const ONE: X87 = X87 { sign: false, exp: 0x3fff, man: INT_BIT };
    /// What an invalid operation produces: the "real indefinite" quiet NaN.
    pub const INDEFINITE: X87 = X87 { sign: true, exp: EXP_MAX, man: INT_BIT | QUIET };

    /// The same register value in the shared unit.
    fn f80(self) -> F80 {
        F80::from_bits(self.man, (self.sign as u16) << 15 | self.exp)
    }

    fn of(v: F80) -> X87 {
        let (man, se) = v.to_bits();
        X87 { sign: se >> 15 != 0, exp: se & EXP_MAX, man }
    }

    pub fn is_nan(self) -> bool {
        self.exp == EXP_MAX && self.man != INT_BIT
    }

    pub fn is_inf(self) -> bool {
        self.exp == EXP_MAX && self.man == INT_BIT
    }

    pub fn is_zero(self) -> bool {
        self.exp == 0 && self.man == 0
    }

    /// The ten bytes of the register as `fstp tbyte` writes them.
    pub fn to_bytes(self) -> [u8; 10] {
        let mut b = [0u8; 10];
        b[..8].copy_from_slice(&self.man.to_le_bytes());
        b[8..].copy_from_slice(&((self.sign as u16) << 15 | self.exp).to_le_bytes());
        b
    }

    pub fn from_bytes(b: [u8; 10]) -> X87 {
        let se = u16::from_le_bytes([b[8], b[9]]);
        X87 { sign: se >> 15 != 0, exp: se & EXP_MAX, man: u64::from_le_bytes(b[..8].try_into().unwrap()) }
    }

    /// `fld m32`: exact. A signalling NaN is made quiet, as the unit does when it loads one.
    pub fn from_f32(v: f32) -> X87 {
        X87::of(F80::from_f32(v))
    }

    /// `fld m64`: exact.
    pub fn from_f64(v: f64) -> X87 {
        X87::of(F80::from_f64(v))
    }

    /// `fild m32`: exact.
    pub fn from_i32(v: i32) -> X87 {
        X87::of(F80::from_i32(v))
    }

    /// `fstp m32`: round to nearest even. Overflow gives an infinity, underflow a subnormal
    /// or zero; a NaN keeps the top 23 bits of its payload.
    pub fn to_f32(self) -> f32 {
        self.f80().to_f32()
    }

    /// `fstp m64`.
    pub fn to_f64(self) -> f64 {
        self.f80().to_f64()
    }

    /// `fistp m32` under the control word GCC sets for a C cast (round toward zero). A NaN, an
    /// infinity or a value outside the range of `int` stores 0x80000000, the unit's "integer
    /// indefinite".
    pub fn to_i32_trunc(self) -> i32 {
        self.f80().to_i32(Round::Zero)
    }

    /// `fabs`: clears the sign, of a NaN too.
    pub fn abs(self) -> X87 {
        X87 { sign: false, ..self }
    }

    /// The comparison `fcom` makes; `None` is "unordered" (a NaN on either side).
    pub fn compare(self, other: X87) -> Option<Ordering> {
        self.f80().compare(other.f80())
    }

    /// True if ordered and less.
    pub fn lt(self, o: X87) -> bool {
        self.compare(o) == Some(Ordering::Less)
    }

    /// True if ordered and less or equal.
    pub fn le(self, o: X87) -> bool {
        matches!(self.compare(o), Some(Ordering::Less | Ordering::Equal))
    }

    /// True if ordered and greater.
    pub fn gt(self, o: X87) -> bool {
        self.compare(o) == Some(Ordering::Greater)
    }

    /// True if ordered and greater or equal.
    pub fn ge(self, o: X87) -> bool {
        matches!(self.compare(o), Some(Ordering::Greater | Ordering::Equal))
    }

    /// True if ordered and equal (+0 equals −0).
    pub fn equals(self, o: X87) -> bool {
        self.compare(o) == Some(Ordering::Equal)
    }
}

impl Add for X87 {
    type Output = X87;
    /// `fadd`
    fn add(self, b: X87) -> X87 {
        X87::of(self.f80() + b.f80())
    }
}

impl Sub for X87 {
    type Output = X87;
    /// `fsub`
    fn sub(self, b: X87) -> X87 {
        X87::of(self.f80() - b.f80())
    }
}

impl Neg for X87 {
    type Output = X87;
    /// `fchs`: flips the sign, of a NaN and of a zero too.
    fn neg(self) -> X87 {
        X87 { sign: !self.sign, ..self }
    }
}

impl Mul for X87 {
    type Output = X87;
    /// `fmul`
    fn mul(self, b: X87) -> X87 {
        X87::of(self.f80() * b.f80())
    }
}

impl Div for X87 {
    type Output = X87;
    /// `fdiv`
    fn div(self, b: X87) -> X87 {
        X87::of(self.f80() / b.f80())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x(v: f64) -> X87 {
        X87::from_f64(v)
    }

    #[test]
    fn loads_and_stores_are_exact_where_they_can_be() {
        for v in [0.0f32, -0.0, 1.0, -1.5, 0.1, 3.4028235e38, 1e-45, 1.1754942e-38, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(X87::from_f32(v).to_f32().to_bits(), v.to_bits(), "{v}");
            assert_eq!(X87::from_f32(v).to_f64().to_bits(), (v as f64).to_bits(), "{v}");
        }
        for v in [0.0f64, -0.0, 1e-5, 57.29577951308232, 5e-324, 2.2250738585072014e-308, f64::MAX] {
            assert_eq!(X87::from_f64(v).to_f64().to_bits(), v.to_bits(), "{v}");
            assert_eq!(X87::from_f64(v).to_f32().to_bits(), (v as f32).to_bits(), "{v}");
        }
        assert_eq!(X87::ONE, X87::from_f32(1.0));
        assert_eq!(X87::from_i32(-3), X87::from_f32(-3.0));
        assert_eq!(X87::from_i32(i32::MIN), x(-2147483648.0));
        // a signalling NaN is quiet once loaded
        assert_eq!(X87::from_f32(f32::from_bits(0x7f80_0001)).to_f32().to_bits(), 0x7fc0_0001);
    }

    #[test]
    fn sixty_four_bits_are_kept() {
        // 1 + 2^-60 is not a double, but it is a register value
        let tiny = x(2f64.powi(-60));
        let s = X87::ONE + tiny;
        assert_eq!(s, X87 { sign: false, exp: 0x3fff, man: INT_BIT | 8 });
        assert_eq!((s - X87::ONE).to_f64(), 2f64.powi(-60));
        // 1 + 2^-64 is a tie and rounds to even (down); 1 + 3·2^-65 rounds up
        assert_eq!(X87::ONE + x(2f64.powi(-64)), X87::ONE);
        assert_eq!((X87::ONE + x(3.0 * 2f64.powi(-65))).man, INT_BIT | 1);
        // 1/3: 64 bits of 0.0101…, rounded to nearest
        assert_eq!(X87::ONE / x(3.0), X87 { sign: false, exp: 0x3ffd, man: 0xaaaa_aaaa_aaaa_aaab });
        assert_eq!(x(3.0) * (X87::ONE / x(3.0)), X87::ONE);
    }

    #[test]
    fn double_rounding_is_visible() {
        // exact sum 1 + 2^-24 + 2^-70: stored straight to a float it would round up (above the
        // tie), but the register first rounds it to 1 + 2^-24, a tie, which then goes to even
        let v = (X87::ONE + x(2f64.powi(-24))) + x(2f64.powi(-70));
        assert_eq!(v.to_f32(), 1.0);
        assert_eq!((X87::ONE + x(2f64.powi(-24) + 2f64.powi(-50))).to_f32(), 1.0 + f32::EPSILON);
    }

    #[test]
    fn special_values() {
        let (inf, zero) = (X87::from_f32(f32::INFINITY), X87::ZERO);
        assert_eq!(inf - inf, X87::INDEFINITE);
        assert_eq!(zero / zero, X87::INDEFINITE);
        assert_eq!(zero * inf, X87::INDEFINITE);
        assert_eq!(X87::INDEFINITE.to_f32().to_bits(), 0xffc0_0000);
        assert_eq!((X87::ONE / zero), inf);
        assert_eq!((-X87::ONE / zero), -inf);
        assert_eq!((X87::ONE - X87::ONE), X87::ZERO);
        assert_eq!((-zero + -zero), -zero);
        assert_eq!((-zero + zero), zero);
        assert_eq!(X87::INDEFINITE.compare(X87::ONE), None);
        assert!(zero.equals(-zero));
        assert!(x(-2.0).lt(x(-1.0)) && x(-1.0).lt(zero) && zero.lt(x(1e-300)) && x(1.0).lt(inf));
        assert!(!X87::INDEFINITE.le(X87::INDEFINITE) && !X87::INDEFINITE.ge(X87::INDEFINITE));
    }

    #[test]
    fn integer_stores_truncate_and_saturate_to_indefinite() {
        for (v, i) in [(2.9, 2), (-2.9, -2), (0.5, 0), (-0.5, 0), (2147483647.0, i32::MAX), (2147483648.0, i32::MIN),
                       (-2147483648.0, i32::MIN), (-2147483649.0, i32::MIN), (1e30, i32::MIN), (f64::NAN, i32::MIN)] {
            assert_eq!(x(v).to_i32_trunc(), i, "{v}");
        }
    }

    // ---- comparison with the x87 unit of the host -------------------------------------

    #[cfg(target_arch = "x86_64")]
    mod hardware {
        use super::super::*;
        use std::arch::asm;

        /// Runs `a op b` on the unit under the default control word and returns the register
        /// and what it stores as float, double and (truncated) int.
        fn run(op: u8, a: X87, b: X87) -> (X87, u32, u64, i32) {
            let (ab, bb) = (a.to_bytes(), b.to_bytes());
            let mut out = [0u8; 10];
            let (mut f, mut d, mut i) = (0u32, 0u64, 0i32);
            let mut cw = [0x037fu16, 0x0f7f, 0];
            unsafe {
                asm!(
                    "fnstcw word ptr [{cw} + 4]",
                    "fldcw word ptr [{cw}]",
                    "fld tbyte ptr [{a}]",
                    "fld tbyte ptr [{b}]",
                    "cmp {op:e}, 1",
                    "je 3f",
                    "cmp {op:e}, 2",
                    "je 4f",
                    "cmp {op:e}, 3",
                    "je 5f",
                    ".byte 0xde, 0xc1", // faddp st(1), st
                    "jmp 6f",
                    "3:",
                    ".byte 0xde, 0xe9", // fsubp st(1), st: st(1) - st
                    "jmp 6f",
                    "4:",
                    ".byte 0xde, 0xc9", // fmulp st(1), st
                    "jmp 6f",
                    "5:",
                    ".byte 0xde, 0xf9", // fdivp st(1), st: st(1) / st
                    "6:",
                    "fld st(0)",
                    "fstp tbyte ptr [{o}]",
                    "fst dword ptr [{f}]",
                    "fst qword ptr [{d}]",
                    "fldcw word ptr [{cw} + 2]",
                    "fistp dword ptr [{i}]",
                    "fnclex",
                    "fldcw word ptr [{cw} + 4]",
                    cw = in(reg) cw.as_mut_ptr(),
                    a = in(reg) ab.as_ptr(),
                    b = in(reg) bb.as_ptr(),
                    op = in(reg) op as u32,
                    o = in(reg) out.as_mut_ptr(),
                    f = in(reg) &mut f as *mut u32,
                    d = in(reg) &mut d as *mut u64,
                    i = in(reg) &mut i as *mut i32,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                    out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                );
            }
            (X87::from_bytes(out), f, d, i)
        }

        struct Rng(u64);
        impl Rng {
            fn next(&mut self) -> u64 {
                // xorshift64*
                self.0 ^= self.0 >> 12;
                self.0 ^= self.0 << 25;
                self.0 ^= self.0 >> 27;
                self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
            }

            /// Operands of the kinds the curve code meets, and harder ones.
            fn operand(&mut self) -> X87 {
                let r = self.next();
                match r % 16 {
                    0..=5 => X87::from_f32(f32::from_bits(self.next() as u32)),
                    6..=8 => X87::from_f64(f64::from_bits(self.next())),
                    9 | 10 => {
                        // floats of moderate size, so that sums cancel and ties occur
                        let m = (self.next() % (1 << 24)) as f32;
                        let e = (self.next() % 60) as i32 - 30;
                        X87::from_f32(if r & 16 != 0 { -m } else { m } * 2f32.powi(e))
                    }
                    11 | 12 => {
                        // a full 64-bit significand over a moderate exponent range
                        let e = 0x3fff - 70 + (self.next() % 140) as u16;
                        X87 { sign: r & 32 != 0, exp: e, man: self.next() | 1 << 63 }
                    }
                    13 => {
                        // any exponent, denormals included
                        let e = (self.next() % 0x7fff) as u16;
                        let man = self.next();
                        if e == 0 { X87 { sign: r & 32 != 0, exp: 0, man: man >> 1 } } else { X87 { sign: r & 32 != 0, exp: e, man: man | 1 << 63 } }
                    }
                    14 => X87::from_i32(self.next() as i32),
                    _ => [X87::ZERO, -X87::ZERO, X87::ONE, X87::from_f32(f32::INFINITY), X87::from_f32(f32::NEG_INFINITY),
                          X87::from_f32(f32::NAN), X87::from_f64(1e-5), X87::from_f64(0.5)][(self.next() % 8) as usize],
                }
            }
        }

        #[test]
        fn every_operation_agrees_with_the_unit() {
            let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
            let mut checked = 0u32;
            for _ in 0..400_000 {
                let (a, b) = (rng.operand(), rng.operand());
                for op in 0..4u8 {
                    let ours = match op {
                        0 => a + b,
                        1 => a - b,
                        2 => a * b,
                        _ => a / b,
                    };
                    let (reg, f, d, i) = run(op, a, b);
                    assert_eq!(ours, reg, "op {op}: {a:x?} {b:x?}");
                    assert_eq!(ours.to_f32().to_bits(), f, "to_f32 of {ours:x?}");
                    assert_eq!(ours.to_f64().to_bits(), d, "to_f64 of {ours:x?}");
                    assert_eq!(ours.to_i32_trunc(), i, "to_i32_trunc of {ours:x?}");
                    checked += 1;
                }
            }
            assert_eq!(checked, 1_600_000);
        }

        #[test]
        fn nan_operands_agree_with_the_unit() {
            let mut rng = Rng(7);
            let nan = |rng: &mut Rng| {
                let r = rng.next();
                // quiet NaNs, as every loaded NaN is; equal payloads and opposite signs included
                let payload = if r & 1 != 0 { rng.next() >> 2 } else { (rng.next() % 4) << 40 };
                X87 { sign: r & 2 != 0, exp: EXP_MAX, man: INT_BIT | QUIET | payload }
            };
            for n in 0..100_000 {
                let a = nan(&mut rng);
                let b = if n % 3 == 0 { rng.operand() } else { nan(&mut rng) };
                for op in 0..4u8 {
                    for (p, q) in [(a, b), (b, a)] {
                        let ours = match op {
                            0 => p + q,
                            1 => p - q,
                            2 => p * q,
                            _ => p / q,
                        };
                        let (reg, f, d, _) = run(op, p, q);
                        assert_eq!(ours, reg, "op {op}: {p:x?} {q:x?}");
                        assert_eq!(ours.to_f32().to_bits(), f);
                        assert_eq!(ours.to_f64().to_bits(), d);
                    }
                }
            }
        }

        #[test]
        fn comparison_agrees_with_the_unit() {
            let mut rng = Rng(42);
            for _ in 0..200_000 {
                let (a, b) = (rng.operand(), rng.operand());
                let (ab, bb) = (a.to_bytes(), b.to_bytes());
                let mut sw = 0u16;
                unsafe {
                    asm!(
                        "fld tbyte ptr [{b}]",
                        "fld tbyte ptr [{a}]",
                        "fucompp",
                        "fnstsw word ptr [{sw}]",
                        "fnclex",
                        a = in(reg) ab.as_ptr(),
                        b = in(reg) bb.as_ptr(),
                        sw = in(reg) &mut sw as *mut u16,
                        out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                        out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                    );
                }
                let expect = match sw >> 8 & 0x45 {
                    0x00 => Some(Ordering::Greater),
                    0x01 => Some(Ordering::Less),
                    0x40 => Some(Ordering::Equal),
                    _ => None,
                };
                assert_eq!(a.compare(b), expect, "{a:x?} {b:x?}");
            }
        }
    }
}
