// SPDX-License-Identifier: MIT
//! The transcendental instructions of the x87 unit and the C library calls built on them
//! (finding 0008, "What cannot be exact").
//!
//! The original calls `sin`, `cos`, `tan`, `asin`, `acos` (and the `float` forms) in the
//! system's `libm.so.6` and uses `fpatan` inline. On i386 the C library of that time computes
//! all of them with the unit's `fsin`, `fcos`, `fptan` and `fpatan` instructions. Unlike
//! `+ − × ÷ √` those are not correctly rounded: the last bits of their 80-bit results differ
//! between processor families, and the library's wrappers differ between releases. So there
//! is no single "original result" to reproduce, and nothing here is claimed bit-exact.
//!
//! What this module does instead: it computes each function to about 120 bits and rounds
//! once to the 64-bit register format, in integer arithmetic only, so the result is the same
//! on every platform and does not depend on any library. For `fsin`/`fcos`/`fptan` the
//! argument is reduced with the 66-bit approximation of π that Intel documents for the unit,
//! not with π itself, which is what makes the unit's results near multiples of π/2 what they
//! are. The tests measure the distance to the processor's own instructions.

use crate::x87::{Class, F80};

/// Replaying recorded results, for the emulation checks (finding 0011).
///
/// The emulation harness cannot vouch for the C library's mathematics or for `fpatan`: it
/// answers them with the host's and writes each call into the vector, arguments and result.
/// While a tape is running on this thread, the functions of this module answer from it
/// instead of computing: the next record must carry the function's name and exactly the
/// argument bits the port passes, and its result is returned. A port that agrees with the
/// original's outputs this way has the same instructions around every such call, argument
/// bits included; what the call itself returns on a given machine stays outside the proof.
pub mod replay {
    use super::F80;
    use std::cell::RefCell;

    /// One recorded call: the name the harness wrote, the argument words, the result.
    #[derive(Clone, Debug)]
    pub struct Record {
        pub name: String,
        pub args: Vec<u128>,
        pub result: F80,
    }

    struct Tape {
        records: Vec<Record>,
        next: usize,
        error: Option<String>,
    }

    thread_local! {
        static TAPE: RefCell<Option<Tape>> = const { RefCell::new(None) };
    }

    /// Starts replaying on this thread.
    pub fn start(records: Vec<Record>) {
        TAPE.with(|t| *t.borrow_mut() = Some(Tape { records, next: 0, error: None }));
    }

    /// Stops replaying. `Err` says where the port's calls left the tape: a different
    /// function, different argument bits, a call too many, or records left over.
    pub fn finish() -> Result<(), String> {
        TAPE.with(|t| match t.borrow_mut().take() {
            None => Ok(()),
            Some(tape) => match tape.error {
                Some(e) => Err(e),
                None if tape.next != tape.records.len() => {
                    Err(format!("{} of {} recorded calls were not made (next: {})", tape.records.len() - tape.next, tape.records.len(), tape.records[tape.next].name))
                }
                None => Ok(()),
            },
        })
    }

    /// The recorded result of the next call if a tape is running and the call matches it.
    /// Public so that a crate with library functions of its own can answer from the same tape.
    pub fn next(name: &str, args: &[u128]) -> Option<F80> {
        TAPE.with(|t| {
            let mut t = t.borrow_mut();
            let tape = t.as_mut()?;
            if tape.error.is_some() {
                return None;
            }
            match tape.records.get(tape.next) {
                Some(r) if r.name == name && r.args == args => {
                    tape.next += 1;
                    Some(r.result)
                }
                Some(r) => {
                    tape.error = Some(format!("call {}: the port calls {name}{args:x?}, the original {}{:x?}", tape.next, r.name, r.args));
                    None
                }
                None => {
                    tape.error = Some(format!("call {}: the port calls {name}{args:x?}, the original made no further call", tape.next));
                    None
                }
            }
        })
    }

    pub(super) fn f80(v: F80) -> u128 {
        let (m, se) = v.to_bits();
        (se as u128) << 64 | m as u128
    }
}

const ONE: u128 = 1 << 126; // fixed point with 126 fraction bits: values in [0, 4)
const M64: u128 = u64::MAX as u128;

// Constants from re/tools/si-math/consts.py.
const PI_Q126: u128 = 0xc90fdaa22168c234c4c6628b80dc1cd1;
const HALF_PI_Q126: u128 = 0x6487ed5110b4611a62633145c06e0e69;
/// arctan(i/8) for i = 0..=8.
const ATAN_EIGHTHS_Q126: [u128; 9] = [
    0x00000000000000000000000000000000,
    0x07f56ea6ab0bdb719644bcc4f9f44478,
    0x0fadbafc96406eb156dc79ef5f7a217e,
    0x16f61941e4def08e715464245b9fc890,
    0x1dac670561bb4f68adfc88bd978751a0,
    0x23c01757bdfd67e6d720d78599710dd2,
    0x292f1f464d3dc249066a1fca915f6b28,
    0x2e014f8af08c679cf2cb69548429110f,
    0x3243f6a8885a308d313198a2e0370734,
];
/// The unit's π for argument reduction, as an integer: π66 = `PI66` × 2^−66. Intel documents
/// it as 4 × 0.C90FDAA22168C234C (hexadecimal), 66 significant bits.
const PI66: u128 = 0xc_90fd_aa22_168c_234c;
/// π/2, π/4 and 3π/4 rounded to the register format (what `fpatan` returns on the axes).
pub const HALF_PI: F80 = F80::from_bits(0xc90f_daa2_2168_c235, 0x3fff);
pub const QUARTER_PI: F80 = F80::from_bits(0xc90f_daa2_2168_c235, 0x3ffe);
pub const THREE_QUARTER_PI: F80 = F80::from_bits(0x96cb_e3f9_990e_91a8, 0x4000);
/// The significand of 2 × `fldpi`, the modulus the C library's fallback reduces by.
const TWO_PI_M: u128 = 0xc90f_daa2_2168_c235;

fn mul_wide(a: u128, b: u128) -> (u128, u128) {
    let (a1, a0) = (a >> 64, a & M64);
    let (b1, b0) = (b >> 64, b & M64);
    let (p00, p01, p10, p11) = (a0 * b0, a0 * b1, a1 * b0, a1 * b1);
    let mid = (p00 >> 64) + (p01 & M64) + (p10 & M64);
    (p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64), (p00 & M64) | (mid << 64))
}

/// Product of two fixed-point numbers.
fn mul_q(a: u128, b: u128) -> u128 {
    let (hi, lo) = mul_wide(a, b);
    (hi << 2) | (lo >> 126)
}

/// Quotient of two fixed-point numbers, for n < 4d and d ≤ 2^127.
fn div_q(n: u128, d: u128) -> u128 {
    let mut rem = n >> 2;
    debug_assert!(rem < d);
    let low = (n & 3) << 126;
    let mut q = 0u128;
    for i in (0..128).rev() {
        rem = (rem << 1) | ((low >> i) & 1);
        q <<= 1;
        if rem >= d {
            rem -= d;
            q |= 1;
        }
    }
    q
}

/// |x| = k × π66/2 + y for a finite non-zero |x| = mx × 2^(ex − 63) below 2^63. Returns k mod 4
/// and y as (negative, m, e): y = ±m × 2^e, |y| ≤ π66/4. The subtraction is exact.
fn reduce(ex: i32, mx: u64) -> (u32, bool, u128, i32) {
    if ex < -1 {
        return (0, false, mx as u128, ex - 63); // below one half: nothing to remove
    }
    // in units of 2^−67, where π66/2 is the integer PI66
    let mut r = mx as u128;
    let mut k = 0u32;
    for _ in 0..ex + 4 {
        r <<= 1;
        k <<= 1;
        if r >= PI66 {
            r -= PI66;
            k |= 1;
        }
    }
    if 2 * r > PI66 {
        (k.wrapping_add(1) & 3, true, PI66 - r, -67)
    } else {
        (k & 3, false, r, -67)
    }
}

/// sin(y)/y and cos(y) for y = m × 2^e, |y| ≤ π/4, in fixed point.
fn kernels(m: u128, e: i32) -> (u128, u128) {
    let sh = e + 126;
    let yq = if sh >= 0 {
        m << sh
    } else if sh > -128 {
        m >> -sh
    } else {
        0
    };
    let z = mul_q(yq, yq);
    let (mut s, mut c) = (ONE, ONE);
    for k in (1..=17u128).rev() {
        s = ONE - mul_q(z, s) / (2 * k * (2 * k + 1));
        c = ONE - mul_q(z, c) / ((2 * k - 1) * 2 * k);
    }
    (s, c)
}

fn special(x: F80) -> Option<F80> {
    match x.class() {
        Class::Nan => Some(x.quiet()),
        Class::Inf => Some(F80::INDEFINITE),
        Class::Zero => None,
        Class::Finite => None,
    }
}

/// `fsin`. For |x| ≥ 2^63 the instruction leaves the operand in place and sets C2; this
/// returns `None` then.
pub fn fsin(x: F80) -> Option<F80> {
    if let Some(v) = special(x) {
        return Some(v);
    }
    if x.class() == Class::Zero {
        return Some(x);
    }
    let (ex, mx) = x.parts();
    if ex >= 63 {
        return None;
    }
    let neg = x.is_sign_negative();
    let (k, yneg, m, e) = reduce(ex, mx);
    let (s, c) = kernels(m, e);
    let (hi, lo) = mul_wide(m, s);
    Some(match k {
        0 => F80::from_wide(neg ^ yneg, hi, lo, e - 126),
        1 => F80::from_wide(neg, 0, c, -126),
        2 => F80::from_wide(!(neg ^ yneg), hi, lo, e - 126),
        _ => F80::from_wide(!neg, 0, c, -126),
    })
}

/// `fcos`; `None` for |x| ≥ 2^63 as for [`fsin`].
pub fn fcos(x: F80) -> Option<F80> {
    if let Some(v) = special(x) {
        return Some(v);
    }
    if x.class() == Class::Zero {
        return Some(F80::ONE);
    }
    let (ex, mx) = x.parts();
    if ex >= 63 {
        return None;
    }
    let (k, yneg, m, e) = reduce(ex, mx);
    let (s, c) = kernels(m, e);
    let (hi, lo) = mul_wide(m, s);
    Some(match k {
        0 => F80::from_wide(false, 0, c, -126),
        1 => F80::from_wide(!yneg, hi, lo, e - 126),
        2 => F80::from_wide(true, 0, c, -126),
        _ => F80::from_wide(yneg, hi, lo, e - 126),
    })
}

/// `fptan`, without the 1.0 it pushes afterwards; `None` for |x| ≥ 2^63.
pub fn fptan(x: F80) -> Option<F80> {
    if let Some(v) = special(x) {
        return Some(v);
    }
    if x.class() == Class::Zero {
        return Some(x);
    }
    let (ex, mx) = x.parts();
    if ex >= 63 {
        return None;
    }
    let neg = x.is_sign_negative();
    let (k, yneg, m, e) = reduce(ex, mx);
    let (s, c) = kernels(m, e);
    if k & 1 == 0 {
        // tan y = y × (sin y / y) / cos y
        let (hi, lo) = mul_wide(m, div_q(s, c));
        return Some(F80::from_wide(neg ^ yneg, hi, lo, e - 126));
    }
    // −cot y = −(cos y / (sin y / y)) / y
    let w = div_q(c, s);
    let lz = m.leading_zeros();
    let d = m << lz;
    let (mut rem, mut q) = (w, 0u128);
    for _ in 0..128 {
        let carry = rem >> 127 != 0;
        rem <<= 1;
        q <<= 1;
        if carry || rem >= d {
            rem = rem.wrapping_sub(d);
            q |= 1;
        }
    }
    Some(F80::from_wide(!(neg ^ yneg), 0, q | (rem != 0) as u128, lz as i32 - 128 - 126 - e))
}

/// Σ (−1)^k z^k / (2k + 1): arctan(u)/u for z = u² ≤ 2^−8.
fn atan_series(z: u128) -> u128 {
    let mut a = ONE / 35;
    for k in (0..17u128).rev() {
        a = ONE / (2 * k + 1) - mul_q(z, a);
    }
    a
}

/// `fpatan` with st1 = `y` and st0 = `x`: the angle of the point (x, y), in (−π, π].
pub fn fpatan(y: F80, x: F80) -> F80 {
    if let Some(v) = replay::next("fpatan", &[replay::f80(y), replay::f80(x)]) {
        return v;
    }
    fpatan_computed(y, x)
}

fn fpatan_computed(y: F80, x: F80) -> F80 {
    let (cy, cx) = (y.class(), x.class());
    if cy == Class::Nan || cx == Class::Nan {
        return F80::nan2(y, x);
    }
    let neg = y.is_sign_negative();
    let xneg = x.is_sign_negative();
    let signed = |v: F80| if neg { -v } else { v };
    match (cy, cx) {
        (Class::Zero, _) => return if xneg { signed(F80::PI) } else { y },
        (_, Class::Zero) => return signed(HALF_PI),
        (Class::Inf, Class::Inf) => return signed(if xneg { THREE_QUARTER_PI } else { QUARTER_PI }),
        (Class::Inf, _) => return signed(HALF_PI),
        (_, Class::Inf) => return if xneg { signed(F80::PI) } else { F80::zero(neg) },
        _ => {}
    }
    let (ea, ma) = y.parts();
    let (eb, mb) = x.parts();
    let swap = (ea, ma) > (eb, mb);
    let ((en, mn), (ed, md)) = if swap { ((eb, mb), (ea, ma)) } else { ((ea, ma), (eb, mb)) };
    // t = min/max as sig × 2^(te − 127), bit 127 of sig set
    let md = md as u128;
    let n1 = (mn as u128) << 64;
    let (q1, r1) = (n1 / md, n1 % md);
    let q2 = (r1 << 64) / md;
    let (sig, te) = if q1 >> 64 != 0 { (q1 << 63 | q2 >> 1, en - ed) } else { (q1 << 64 | q2, en - ed - 1) };
    let sh = (1 - te) as u32; // t in fixed point is sig >> (1 − te)
    let tq = if sh < 128 { sig >> sh } else { 0 };
    let r = if te < -4 {
        // t < 1/16: arctan t = t × series, kept with its own exponent when nothing is added
        let a = atan_series(mul_q(tq, tq));
        if !swap && !xneg {
            let (hi, lo) = mul_wide(sig, a);
            return F80::from_wide(neg, hi, lo, te - 127 - 126);
        }
        mul_q(tq, a)
    } else {
        // arctan t = arctan c + arctan((t − c)/(1 + t c)), c the nearest eighth
        let i = ((tq + (ONE >> 4)) >> 123) as usize;
        let c = (i as u128) << 123;
        let d = ONE + mul_q(tq, c);
        let (un, uneg) = if tq >= c { (tq - c, false) } else { (c - tq, true) };
        let u = div_q(un, d);
        let v = mul_q(u, atan_series(mul_q(u, u)));
        if uneg {
            ATAN_EIGHTHS_Q126[i] - v
        } else {
            ATAN_EIGHTHS_Q126[i] + v
        }
    };
    let r = if swap { HALF_PI_Q126 - r } else { r };
    let r = if xneg { PI_Q126 - r } else { r };
    F80::from_wide(neg, 0, r, -126)
}

/// The C library's fallback when `fsin`/`fcos`/`fptan` report an operand out of range: the
/// IEEE remainder of x by 2 × `fldpi`, which `fprem1` computes exactly.
fn remainder_two_pi(x: F80) -> F80 {
    let (ex, mx) = x.parts();
    let mut r = mx as u128 % TWO_PI_M;
    let mut odd = mx as u128 >= TWO_PI_M;
    for _ in 0..ex - 2 {
        r <<= 1;
        odd = r >= TWO_PI_M;
        if odd {
            r -= TWO_PI_M;
        }
    }
    let neg = x.is_sign_negative();
    if 2 * r > TWO_PI_M || (2 * r == TWO_PI_M && odd) {
        F80::from_wide(!neg, 0, TWO_PI_M - r, -61)
    } else {
        F80::from_wide(neg, 0, r, -61)
    }
}

/// `sin` of the C library the original links against (`libm.so.6`, i386): `fsin` on the
/// argument; if the unit reports it out of range (|x| ≥ 2^63), on its remainder modulo
/// 2 × `fldpi`. The 80-bit result is returned in st0 unrounded.
pub fn sin(x: f64) -> F80 {
    if let Some(v) = replay::next("sin", &[x.to_bits() as u128]) {
        return v;
    }
    sin_computed(x)
}

fn sin_computed(x: f64) -> F80 {
    let x = F80::from_f64(x);
    fsin(x).unwrap_or_else(|| fsin(remainder_two_pi(x)).unwrap())
}

/// `cos` of the C library, as [`sin`].
pub fn cos(x: f64) -> F80 {
    if let Some(v) = replay::next("cos", &[x.to_bits() as u128]) {
        return v;
    }
    cos_computed(x)
}

fn cos_computed(x: f64) -> F80 {
    let x = F80::from_f64(x);
    fcos(x).unwrap_or_else(|| fcos(remainder_two_pi(x)).unwrap())
}

/// `tan` of the C library, as [`sin`].
pub fn tan(x: f64) -> F80 {
    if let Some(v) = replay::next("tan", &[x.to_bits() as u128]) {
        return v;
    }
    tan_computed(x)
}

fn tan_computed(x: f64) -> F80 {
    let x = F80::from_f64(x);
    fptan(x).unwrap_or_else(|| fptan(remainder_two_pi(x)).unwrap())
}

/// `sinf`: the same instruction on a `float` argument; the result is not rounded either.
pub fn sinf(x: f32) -> F80 {
    if let Some(v) = replay::next("sinf", &[x.to_bits() as u128]) {
        return v;
    }
    sin_computed(x as f64)
}

pub fn cosf(x: f32) -> F80 {
    if let Some(v) = replay::next("cosf", &[x.to_bits() as u128]) {
        return v;
    }
    cos_computed(x as f64)
}

pub fn tanf(x: f32) -> F80 {
    if let Some(v) = replay::next("tanf", &[x.to_bits() as u128]) {
        return v;
    }
    tan_computed(x as f64)
}

/// `asin` of the C library: arctan(x / √(1 − x²)) by `fpatan`, the square root by `fsqrt`.
///
/// The library's wrapper around this keeps the result in a `double` across a call before
/// returning it, so, unlike [`sin`], the value comes back rounded to double. That is read
/// from the C library's source of the period, not from the `soft` binary: an assumption about
/// the machine the original ran on. |x| > 1 gives the NaN of 0/0.
pub fn asin(x: f64) -> F80 {
    if let Some(v) = replay::next("asin", &[x.to_bits() as u128]) {
        return v;
    }
    let x = F80::from_f64(x);
    F80::from_f64(fpatan_computed(x, (F80::ONE - x * x).sqrt()).to_f64())
}

/// `acos` of the C library: arctan(√(1 − x²) / x) by `fpatan`; rounded to double as [`asin`].
pub fn acos(x: f64) -> F80 {
    if let Some(v) = replay::next("acos", &[x.to_bits() as u128]) {
        return v;
    }
    let x = F80::from_f64(x);
    F80::from_f64(fpatan_computed((F80::ONE - x * x).sqrt(), x).to_f64())
}

// ---- the rest of the C library's mathematics that the original imports
//
// `exp`, `log`, `log10`, `pow` and the `float` forms are, on the machines the original ran
// on, built from the unit's `fyl2x`, `f2xm1` and `fscale` with special cases around them
// (`pow` multiplies out small whole exponents). Like the trigonometry they have no single
// original result. They are here for two reasons: so that a port has one place to call, and
// so that an emulation check can replay the recorded results through them (see [`replay`]).
// Outside a replay they answer with **the host's double-precision function, widened**: good
// to the last place of a double, not to the register's 64 bits, and not the same on every
// platform. Nothing computed through them may be called exact.

fn replayed1(name: &str, x: u128, compute: impl FnOnce() -> F80) -> F80 {
    replay::next(name, &[x]).unwrap_or_else(compute)
}

/// `exp` of the C library (see the note above: replayed, or the host's double result).
pub fn exp(x: f64) -> F80 {
    replayed1("exp", x.to_bits() as u128, || F80::from_f64(x.exp()))
}

/// `expf`: the same on a `float` argument.
pub fn expf(x: f32) -> F80 {
    replayed1("expf", x.to_bits() as u128, || F80::from_f64((x as f64).exp()))
}

/// `log`, the natural logarithm.
pub fn log(x: f64) -> F80 {
    replayed1("log", x.to_bits() as u128, || F80::from_f64(x.ln()))
}

/// `log10`.
pub fn log10(x: f64) -> F80 {
    replayed1("log10", x.to_bits() as u128, || F80::from_f64(x.log10()))
}

/// `pow`.
pub fn pow(x: f64, y: f64) -> F80 {
    replay::next("pow", &[x.to_bits() as u128, y.to_bits() as u128]).unwrap_or_else(|| F80::from_f64(x.powf(y)))
}

/// `powf`.
pub fn powf(x: f32, y: f32) -> F80 {
    replay::next("powf", &[x.to_bits() as u128, y.to_bits() as u128]).unwrap_or_else(|| F80::from_f64((x as f64).powf(y as f64)))
}

/// `cbrt`.
pub fn cbrt(x: f64) -> F80 {
    replayed1("cbrt", x.to_bits() as u128, || F80::from_f64(x.cbrt()))
}

/// `asinf`: [`asin`] on a `float` argument (the same construction and the same assumption
/// about the rounding of the result).
pub fn asinf(x: f32) -> F80 {
    replayed1("asinf", x.to_bits() as u128, || {
        let x = F80::from_f32(x);
        F80::from_f64(fpatan_computed(x, (F80::ONE - x * x).sqrt()).to_f64())
    })
}

/// `acosf`: the C library's float form. **The result is a float**: measured against the
/// original in the reflection-map look-up of `Matter-News_Logo` frame 12 (7,571 samples of
/// `SI_Sample` recorded live: every red, green and blue value equal with this rounding, 1,009
/// of them different with the double rounding `acos` has).
/// The value is the `fpatan` form (the correctly rounded float of the register result), not
/// the double the argument is widened to. `asinf` is not measured and keeps its old form.
pub fn acosf(x: f32) -> F80 {
    replayed1("acosf", x.to_bits() as u128, || {
        let x = F80::from_f32(x);
        F80::from_f32(fpatan_computed((F80::ONE - x * x).sqrt(), x).to_f32())
    })
}

/// `atanf`: `fpatan` of the argument and 1.
pub fn atanf(x: f32) -> F80 {
    replayed1("atanf", x.to_bits() as u128, || fpatan_computed(F80::from_f32(x), F80::ONE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x87::tests::Rng;
    use crate::x87::x;

    fn close(a: F80, b: f64, ulps: f64) -> bool {
        let d = (a.to_f64() - b).abs();
        d <= ulps * f64::EPSILON * b.abs().max(f64::MIN_POSITIVE)
    }

    #[test]
    fn known_values() {
        assert_eq!(sin(0.0).to_f64().to_bits(), 0);
        assert_eq!(sin(-0.0).to_f64().to_bits(), 1 << 63);
        assert_eq!(cos(0.0).to_bits(), F80::ONE.to_bits());
        // sin(π/6) = ½, cos(π/3) = ½, tan(π/4) = 1, to within the argument's rounding
        assert!(close(sin(core::f64::consts::FRAC_PI_6), 0.5, 2.0));
        assert!(close(cos(core::f64::consts::FRAC_PI_3), 0.5, 4.0));
        assert!(close(tan(core::f64::consts::FRAC_PI_4), 1.0, 2.0));
        assert!(close(tan(2.0), -2.185039863261519, 2.0));
        // a large argument shows the unit's 66-bit π: the true sine of 1e10 is
        // −0.48750602508751067, and removing 6.4e9 quarter turns of the short π leaves the
        // result wrong from the eleventh digit on
        let err = (sin(1e10).to_f64() - -0.48750602508751067f64).abs();
        assert!(err > 1e-12 && err < 1e-10, "{err:e}");
        assert!(close(cos(-100.0), 0.8623188722876839, 2.0));
        // 16 arctan(1/5) − 4 arctan(1/239) = π
        let machin = x(16.0f64) * fpatan(F80::ONE, x(5.0f64)) - x(4.0f64) * fpatan(F80::ONE, x(239.0f64));
        assert_eq!(machin.to_f64(), core::f64::consts::PI);
        assert_eq!(fpatan(F80::ONE, F80::ONE).to_bits(), QUARTER_PI.to_bits());
        assert_eq!(fpatan(F80::ONE, -F80::ONE).to_bits(), THREE_QUARTER_PI.to_bits());
        assert_eq!(fpatan(-F80::ONE, F80::ZERO).to_bits(), (-HALF_PI).to_bits());
        assert_eq!(fpatan(F80::ZERO, -F80::ONE).to_bits(), F80::PI.to_bits());
        assert_eq!(fpatan(-F80::ZERO, -F80::ZERO).to_bits(), (-F80::PI).to_bits());
        assert_eq!(fpatan(-F80::ZERO, F80::ONE).to_bits(), (-F80::ZERO).to_bits());
        assert_eq!(acos(1.0).to_f64(), 0.0);
        assert_eq!(acos(-1.0).to_f64(), core::f64::consts::PI);
        assert_eq!(acos(0.0).to_f64(), core::f64::consts::FRAC_PI_2);
        assert_eq!(asin(1.0).to_f64(), core::f64::consts::FRAC_PI_2);
        assert_eq!(asin(-0.0).to_f64().to_bits(), 1 << 63);
        assert!(close(asin(0.5), core::f64::consts::FRAC_PI_6, 1.0));
        assert!(acos(1.5).is_nan() && asin(-2.0).is_nan());
    }

    #[test]
    fn identities_hold_to_the_last_bits() {
        let mut rng = Rng(77);
        for _ in 0..20_000 {
            let a = (rng.next() >> 11) as f64 / (1u64 << 53) as f64 * 200.0 - 100.0;
            let (s, c) = (sin(a), cos(a));
            // sin² + cos² = 1 within a few units of the 64-bit format
            let one = (s * s + c * c - F80::ONE).abs();
            assert!(one.to_f64() < 4.0 * 2f64.powi(-64), "{a}");
            // tan = sin / cos
            let t = tan(a);
            let d = ((t - s / c) / t).abs();
            assert!(d.to_f64() < 8.0 * 2f64.powi(-64), "{a}");
            // the angle of (cos a, sin a) is a, for a in (−π, π)
            if a.abs() < 3.0 {
                let back = fpatan(s, c);
                assert!((back - x(a)).abs().to_f64() < 8.0 * 2f64.powi(-64), "{a}");
            }
        }
    }

    #[test]
    fn special_operands() {
        assert_eq!(fsin(F80::INFINITY).unwrap().to_bits(), F80::INDEFINITE.to_bits());
        assert_eq!(fcos(-F80::INFINITY).unwrap().to_bits(), F80::INDEFINITE.to_bits());
        assert!(fsin(x(f64::NAN)).unwrap().is_nan());
        // out of the instructions' range: 2^63 and above
        let big = x(9223372036854775808.0f64);
        assert!(fsin(big).is_none() && fcos(big).is_none() && fptan(big).is_none());
        assert!(fsin(x(9223372036854774784.0f64)).is_some()); // the double below 2^63
        // tiny arguments come back unchanged, denormals included
        for v in [1e-300f64, -4.9e-324, 2.5e-20] {
            assert_eq!(sin(v).to_bits(), x(v).to_bits());
            assert_eq!(tan(v).to_bits(), x(v).to_bits());
            assert_eq!(cos(v).to_bits(), F80::ONE.to_bits());
        }
        // the library's reduction of huge arguments stays in range
        for v in [1e19f64, -3e30, 1e300, f64::MAX] {
            assert!(sin(v).abs().le(F80::ONE) && cos(v).abs().le(F80::ONE), "{v:e}");
            let (s, c) = (sin(v), cos(v));
            assert!(((s * s + c * c) - F80::ONE).abs().to_f64() < 1e-18);
        }
    }

    /// The C library's `sin` for arguments the instruction refuses, on the processor: `fsin`,
    /// and if C2 is set, `fprem1` against 2 × `fldpi` until done, then `fsin` again.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn huge_arguments_match_the_library_sequence() {
        use core::arch::asm;
        fn hw_sin(v: f64) -> F80 {
            let mut out = [0u8; 16];
            let (cw, mut old) = (0x037fu16, 0u16);
            unsafe {
                asm!(
                    "fnstcw word ptr [{old}]", "fldcw word ptr [{cw}]",
                    "fld qword ptr [{a}]", "fsin", "fnstsw ax", "test eax, 0x400", "jz 3f",
                    "fldpi", "fadd st(0), st(0)", "fxch st(1)",
                    "2:", "fprem1", "fnstsw ax", "test eax, 0x400", "jnz 2b",
                    "fstp st(1)", "fsin",
                    "3:", "fstp tbyte ptr [{o}]", "fnclex", "fldcw word ptr [{old}]",
                    old = in(reg) &mut old, cw = in(reg) &cw, a = in(reg) &v, o = in(reg) out.as_mut_ptr(),
                    out("eax") _,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                    out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                );
            }
            F80::from_bits(u64::from_le_bytes(out[..8].try_into().unwrap()), u16::from_le_bytes(out[8..10].try_into().unwrap()))
        }
        let mut rng = Rng(63);
        for i in 0..20_000 {
            // doubles from 2^63 up to the largest
            let e = 1023 + 63 + rng.next() % 961;
            let v = f64::from_bits((rng.next() & 0x800f_ffff_ffff_ffff) | e << 52);
            let (got, want) = (sin(v), hw_sin(v));
            let ((mg, sg), (mw, sw)) = (got.to_bits(), want.to_bits());
            assert!(sg == sw && mg.abs_diff(mw) <= 1 || got.to_f64() == want.to_f64(), "{i}: sin({v:e}) = {got:?}, processor {want:?}");
        }
    }

    /// Against the processor's instructions: how often the 80-bit results agree, the largest
    /// difference in units of the last place, and whether a store to `float` or `double`
    /// ever differs. These are measurements of one machine, printed with `--nocapture`.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn distance_to_the_processor() {
        use core::arch::asm;
        fn hw(op: u8, a: F80, b: F80) -> F80 {
            let pack = |v: F80| {
                let (m, se) = v.to_bits();
                let mut t = [0u8; 16];
                t[..8].copy_from_slice(&m.to_le_bytes());
                t[8..10].copy_from_slice(&se.to_le_bytes());
                t
            };
            let (ta, tb, mut out) = (pack(a), pack(b), [0u8; 16]);
            let (cw, mut old) = (0x037fu16, 0u16);
            unsafe {
                asm!(
                    "fnstcw word ptr [{old}]", "fldcw word ptr [{cw}]",
                    "fld tbyte ptr [{a}]", "fld tbyte ptr [{b}]",
                    "cmp {op:e}, 0", "jne 2f", "fstp st(1)", "fsin", "jmp 9f",
                    "2:", "cmp {op:e}, 1", "jne 3f", "fstp st(1)", "fcos", "jmp 9f",
                    "3:", "cmp {op:e}, 2", "jne 4f", "fstp st(1)", "fptan", "fstp st(0)", "jmp 9f",
                    "4:", "fpatan",
                    "9:", "fstp tbyte ptr [{o}]", "fnclex", "fldcw word ptr [{old}]",
                    old = in(reg) &mut old, cw = in(reg) &cw, a = in(reg) ta.as_ptr(), b = in(reg) tb.as_ptr(),
                    o = in(reg) out.as_mut_ptr(), op = in(reg) op as u32,
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                    out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                );
            }
            F80::from_bits(u64::from_le_bytes(out[..8].try_into().unwrap()), u16::from_le_bytes(out[8..10].try_into().unwrap()))
        }
        fn ulps(a: F80, b: F80) -> u64 {
            let ((ma, sa), (mb, sb)) = (a.to_bits(), b.to_bits());
            if sa == sb {
                ma.abs_diff(mb)
            } else if sa == sb + 1 && ma == 1 << 63 && mb == u64::MAX || sb == sa + 1 && mb == 1 << 63 && ma == u64::MAX {
                1
            } else {
                u64::MAX
            }
        }
        let mut rng = Rng(2002);
        let names = ["fsin", "fcos", "fptan", "fpatan"];
        let ranges: [(&str, f64); 3] = [("|x| < 4", 4.0), ("|x| < 1000", 1000.0), ("|x| < 1e9", 1e9)];
        for (label, range) in ranges {
            for op in 0..4u8 {
                let (mut same, mut worst, mut f32_diff, mut f64_diff) = (0u32, 0u64, 0u32, 0u32);
                let n = 200_000;
                for _ in 0..n {
                    // arguments as the program has them: floats widened to double
                    let a = ((rng.next() >> 40) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * range as f32;
                    let b = ((rng.next() >> 40) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * range as f32;
                    let (xa, xb) = (F80::from_f32(a), F80::from_f32(b));
                    let (got, want) = match op {
                        0 => (fsin(xa).unwrap(), hw(0, xa, xa)),
                        1 => (fcos(xa).unwrap(), hw(1, xa, xa)),
                        2 => (fptan(xa).unwrap(), hw(2, xa, xa)),
                        _ => (fpatan(xa, xb), hw(3, xa, xb)),
                    };
                    let u = ulps(got, want);
                    same += (u == 0) as u32;
                    worst = worst.max(u);
                    f32_diff += (got.to_f32().to_bits() != want.to_f32().to_bits()) as u32;
                    f64_diff += (got.to_f64().to_bits() != want.to_f64().to_bits()) as u32;
                }
                println!("{:7} {label:11}: identical 80-bit results {same} of {n}, worst difference {worst} ulp, stores differing: float {f32_diff}, double {f64_diff}",
                    names[op as usize]);
                assert!(worst <= 2, "{} {label}: {worst} ulp", names[op as usize]);
                assert_eq!(f32_diff, 0, "{} {label}", names[op as usize]);
            }
        }
    }
}
