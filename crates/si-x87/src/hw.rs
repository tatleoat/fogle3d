// SPDX-License-Identifier: MIT
//! The processor's own x87 unit as the backend of [`F80`]'s operations (x86-64 only).
//!
//! The original is i386 code for the x87 unit, so on a processor that has one the same
//! instructions give the same bits: `+ − × ÷ √` are correctly rounded in the format, the
//! stores round as the format and rounding control say, and the comparisons set the same
//! condition codes. This module runs them on the unit instead of in the software model
//! (`F80::soft_*`), which stays the reference and the fallback. It answers `None` for what it
//! does not take, and the caller then uses the software model:
//!
//! - an operand whose encoding no load or operation produces (an unnormal, a pseudo-NaN or
//!   pseudo-infinity, a pseudo-denormal: the integer bit not set exactly when the exponent is
//!   non-zero). The unit treats those as invalid operands or normalises them; the software
//!   model computes with them as numbers. They reach the model only through
//!   [`F80::from_bits`].
//! - a control word other than 0x037f (64-bit precision, round to nearest, every exception
//!   masked), the one Linux starts a process with and the one the software model computes in.
//!   Another program in the process could have changed it (Windows starts x64 threads with
//!   53-bit precision, 0x027f), so it is read before every operation (`fnstcw`, about a cycle).
//!
//! Every answer given is the software model's answer: `cargo test --release -p si-x87` checks
//! each operation against `F80::soft_*` on random and edge operands (`hw::tests`), and each
//! of a set of deliberately wrong variants must fail that check. The transcendental
//! functions (`trig`), `fprem`, `fscale` and `fmod` stay in the software model: `fsin`,
//! `fcos`, `fptan` and `fpatan` differ between processors, and `fprem`'s partial step is the
//! processor's choice (see [`F80::fprem`]).
//!
//! Each operation here loads its operands from memory and stores its result (the 80-bit
//! store and load cost more than the operation); a whole expression in one block is
//! [`crate::fx!`], several times faster again (re/tools/x87hw1/opbench: a dot product of
//! floats 55.5 ns in the software model, 16.2 through the operators here, 2.6 as one block).
//!
//! Selection is at compile time: this module is used by default when the crate is built for x86-64
//! (`SI_X87=soft` turns it off, `SI_X87=hw` demands it; see build.rs). Comparisons then read the bits (`F80::bits_compare`) rather
//! than `fucomip`, which measured slower; the `raw` comparisons stay tested here.
//!
//! The x87 register stack is empty on entry to every block here (all eight registers are
//! declared clobbered, which Rust guarantees means an empty stack) and is left empty.

use crate::x87::{Round, F80};
use core::arch::asm;
use core::cmp::Ordering;
use core::mem::MaybeUninit;

/// The control word the backend runs under: Linux's initial one, the original's.
pub const CW: u16 = 0x037f;

/// The four rounding-control settings with everything else as in [`CW`].
const fn cw_of(mode: Round) -> u16 {
    match mode {
        Round::Nearest => CW,
        Round::Down => CW | 0x0400,
        Round::Up => CW | 0x0800,
        Round::Zero => CW | 0x0c00,
    }
}
static CW_DOWN: u16 = cw_of(Round::Down);
static CW_UP: u16 = cw_of(Round::Up);
static CW_ZERO: u16 = cw_of(Round::Zero);
static CW_STD: u16 = CW;

fn cw_ptr(mode: Round) -> *const u16 {
    match mode {
        Round::Nearest => &CW_STD,
        Round::Down => &CW_DOWN,
        Round::Up => &CW_UP,
        Round::Zero => &CW_ZERO,
    }
}

/// An `asm!` block on the x87 stack: the eight registers are declared clobbered.
macro_rules! x87 {
    ([$($code:expr),* $(,)?], [$($op:tt)*], $opts:tt) => {
        asm!($($code),*, $($op)* out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
            out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _, options $opts)
    };
}

/// The unit's control word now (`fnstcw`).
#[inline(always)]
pub fn control_word() -> u16 {
    let mut c = MaybeUninit::<u16>::uninit();
    unsafe {
        asm!("fnstcw word ptr [{c}]", c = in(reg) c.as_mut_ptr(), options(nostack, preserves_flags));
        c.assume_init()
    }
}

/// Sets the unit's control word (`fnclex`, then `fldcw`, so that no exception flag left from
/// earlier operations becomes pending under a word that unmasks it); for tests and for
/// callers that know what they do.
pub fn set_control_word(cw: u16) {
    unsafe { asm!("fnclex", "fldcw word ptr [{c}]", c = in(reg) &cw, options(nostack, preserves_flags, readonly)) };
}

/// True when the encoding is one a load or an operation of the unit produces: the integer
/// bit is set exactly when the biased exponent is not zero.
#[inline(always)]
pub fn canonical(v: F80) -> bool {
    ((v.se & 0x7fff) != 0) == ((v.m as i64) < 0)
}

#[inline(always)]
fn cw_ok() -> bool {
    #[cfg(test)]
    if tests::mutant() == tests::M_GUARD_CW {
        return true;
    }
    control_word() == CW
}

#[inline(always)]
fn ok1(a: F80) -> bool {
    #[cfg(test)]
    if tests::mutant() == tests::M_GUARD_CANON {
        return cw_ok();
    }
    canonical(a) & cw_ok()
}

#[inline(always)]
fn ok2(a: F80, b: F80) -> bool {
    #[cfg(test)]
    if tests::mutant() == tests::M_GUARD_CANON {
        return cw_ok();
    }
    (canonical(a) & canonical(b)) & cw_ok()
}

/// The guard of an `fx!` block, for one [`F80`] leaf: canonical (see the module).
#[doc(hidden)]
#[inline(always)]
pub fn fx_ok(a: F80) -> bool {
    #[cfg(test)]
    if tests::mutant() == tests::M_GUARD_CANON {
        return true;
    }
    canonical(a)
}

/// The guard of an `fx!` block: the control word is [`CW`].
#[doc(hidden)]
#[inline(always)]
pub fn fx_cw_ok() -> bool {
    cw_ok()
}

/// The operations on the unit without the checks: the caller vouches for canonical operands
/// and the control word [`CW`]. Public for the differential tests and the benchmarks.
pub mod raw {
    use super::*;

    macro_rules! two {
        ($name:ident, $bytes:literal) => {
            /// `st1 op st0` with st1 = `a`, st0 = `b`, popped: the register form the compiler
            /// emits (`faddp`, `fsubp`, `fmulp`, `fdivp`, encoded by hand so that no
            /// assembler's operand-order convention is involved).
            #[inline(always)]
            pub fn $name(a: F80, b: F80) -> F80 {
                #[cfg(test)]
                if let Some(r) = tests::mutated2(stringify!($name), a, b) {
                    return r;
                }
                let mut o = MaybeUninit::<F80>::uninit();
                unsafe {
                    x87!(["fld tbyte ptr [{a}]", "fld tbyte ptr [{b}]", $bytes, "fstp tbyte ptr [{o}]"],
                        [a = in(reg) &a, b = in(reg) &b, o = in(reg) o.as_mut_ptr(),], (nostack, preserves_flags));
                    o.assume_init()
                }
            }
        };
    }
    // DE C1 faddp st1,st0; DE E9 fsubp st1,st0 (st1 − st0); DE C9 fmulp; DE F9 fdivp (st1 ÷ st0)
    two!(add, ".byte 0xde, 0xc1");
    two!(sub, ".byte 0xde, 0xe9");
    two!(mul, ".byte 0xde, 0xc9");
    two!(div, ".byte 0xde, 0xf9");

    macro_rules! mixed {
        ($name:ident, $rname:ident, $t:ty, $ld:literal, $bytes:literal) => {
            /// `a op b` with `b` loaded from memory by `fld` first (exact, a signalling NaN
            /// made quiet), then the register form; as [`F80`]'s mixed operators define it.
            #[inline(always)]
            pub fn $name(a: F80, b: $t) -> F80 {
                #[cfg(test)]
                if let Some(r) = tests::mutated_mixed(stringify!($name), a, b as f64) {
                    return r;
                }
                let mut o = MaybeUninit::<F80>::uninit();
                unsafe {
                    x87!(["fld tbyte ptr [{a}]", concat!("fld ", $ld, " ptr [{b}]"), $bytes, "fstp tbyte ptr [{o}]"],
                        [a = in(reg) &a, b = in(reg) &b, o = in(reg) o.as_mut_ptr(),], (nostack, preserves_flags));
                    o.assume_init()
                }
            }
            /// `a op b` with `a` loaded from memory.
            #[inline(always)]
            pub fn $rname(a: $t, b: F80) -> F80 {
                let mut o = MaybeUninit::<F80>::uninit();
                unsafe {
                    x87!([concat!("fld ", $ld, " ptr [{a}]"), "fld tbyte ptr [{b}]", $bytes, "fstp tbyte ptr [{o}]"],
                        [a = in(reg) &a, b = in(reg) &b, o = in(reg) o.as_mut_ptr(),], (nostack, preserves_flags));
                    o.assume_init()
                }
            }
        };
    }
    mixed!(add_f32, f32_add, f32, "dword", ".byte 0xde, 0xc1");
    mixed!(sub_f32, f32_sub, f32, "dword", ".byte 0xde, 0xe9");
    mixed!(mul_f32, f32_mul, f32, "dword", ".byte 0xde, 0xc9");
    mixed!(div_f32, f32_div, f32, "dword", ".byte 0xde, 0xf9");
    mixed!(add_f64, f64_add, f64, "qword", ".byte 0xde, 0xc1");
    mixed!(sub_f64, f64_sub, f64, "qword", ".byte 0xde, 0xe9");
    mixed!(mul_f64, f64_mul, f64, "qword", ".byte 0xde, 0xc9");
    mixed!(div_f64, f64_div, f64, "qword", ".byte 0xde, 0xf9");

    /// `fsqrt`
    #[inline(always)]
    pub fn sqrt(a: F80) -> F80 {
        #[cfg(test)]
        if tests::mutant() == tests::M_SQRT_PC53 {
            return tests::off(|| tests::under(0x027f, || sqrt(a)));
        }
        let mut o = MaybeUninit::<F80>::uninit();
        unsafe {
            x87!(["fld tbyte ptr [{a}]", "fsqrt", "fstp tbyte ptr [{o}]"],
                [a = in(reg) &a, o = in(reg) o.as_mut_ptr(),], (nostack, preserves_flags));
            o.assume_init()
        }
    }

    /// `fstp m32`
    #[inline(always)]
    pub fn to_f32(a: F80) -> f32 {
        #[cfg(test)]
        if tests::mutant() == tests::M_F32_TWICE {
            return tests::off(|| to_f64(a) as f32); // rounded twice: differs on the ties the first rounding makes
        }
        let mut o = MaybeUninit::<f32>::uninit();
        unsafe {
            x87!(["fld tbyte ptr [{a}]", "fstp dword ptr [{o}]"], [a = in(reg) &a, o = in(reg) o.as_mut_ptr(),], (nostack, preserves_flags));
            o.assume_init()
        }
    }

    /// `fstp m64`
    #[inline(always)]
    pub fn to_f64(a: F80) -> f64 {
        #[cfg(test)]
        if tests::mutant() == tests::M_F64_RC_ZERO {
            return tests::off(|| tests::under(cw_of(Round::Zero), || to_f64(a)));
        }
        let mut o = MaybeUninit::<f64>::uninit();
        unsafe {
            x87!(["fld tbyte ptr [{a}]", "fstp qword ptr [{o}]"], [a = in(reg) &a, o = in(reg) o.as_mut_ptr(),], (nostack, preserves_flags));
            o.assume_init()
        }
    }

    /// `fld m32` (the software model's [`F80::from_f32`] is as fast and is what is used; this
    /// is the load the mixed operators make, checked exhaustively against it)
    #[inline(always)]
    pub fn from_f32(v: f32) -> F80 {
        let mut o = MaybeUninit::<F80>::uninit();
        unsafe {
            x87!(["fld dword ptr [{a}]", "fstp tbyte ptr [{o}]"], [a = in(reg) &v, o = in(reg) o.as_mut_ptr(),], (nostack, preserves_flags));
            o.assume_init()
        }
    }

    /// `fld m64`
    #[inline(always)]
    pub fn from_f64(v: f64) -> F80 {
        let mut o = MaybeUninit::<F80>::uninit();
        unsafe {
            x87!(["fld qword ptr [{a}]", "fstp tbyte ptr [{o}]"], [a = in(reg) &v, o = in(reg) o.as_mut_ptr(),], (nostack, preserves_flags));
            o.assume_init()
        }
    }

    /// `fucomip` of st0 = `a` against st1 = `b`: the ordering, `None` when unordered.
    #[inline(always)]
    pub fn compare(a: F80, b: F80) -> Option<Ordering> {
        let (g, l, u): (u8, u8, u8);
        unsafe {
            x87!(["fld tbyte ptr [{b}]", "fld tbyte ptr [{a}]", "fucomip st, st(1)", "fstp st(0)", "seta {g}", "setb {l}", "setp {u}"],
                [a = in(reg) &a, b = in(reg) &b, g = out(reg_byte) g, l = out(reg_byte) l, u = out(reg_byte) u,], (nostack));
        }
        if u != 0 {
            None
        } else if g != 0 {
            Some(Ordering::Greater)
        } else if l != 0 {
            Some(Ordering::Less)
        } else {
            Some(Ordering::Equal)
        }
    }

    /// `a > b` (false when unordered): `fucomip` then `seta`
    #[inline(always)]
    pub fn gt(a: F80, b: F80) -> bool {
        #[cfg(test)]
        if tests::mutant() == tests::M_GT_UNORDERED {
            return tests::off(|| gt(a, b) | unordered(a, b));
        }
        let r: u8;
        unsafe {
            x87!(["fld tbyte ptr [{b}]", "fld tbyte ptr [{a}]", "fucomip st, st(1)", "fstp st(0)", "seta {r}"],
                [a = in(reg) &a, b = in(reg) &b, r = out(reg_byte) r,], (nostack));
        }
        r != 0
    }

    /// `a >= b` (false when unordered): `fucomip` then `setae`
    #[inline(always)]
    pub fn ge(a: F80, b: F80) -> bool {
        let r: u8;
        unsafe {
            x87!(["fld tbyte ptr [{b}]", "fld tbyte ptr [{a}]", "fucomip st, st(1)", "fstp st(0)", "setae {r}"],
                [a = in(reg) &a, b = in(reg) &b, r = out(reg_byte) r,], (nostack));
        }
        r != 0
    }

    /// `a == b` (false when unordered): `fucomip` then `sete` and `setnp`
    #[inline(always)]
    pub fn eq(a: F80, b: F80) -> bool {
        let (r, p): (u8, u8);
        unsafe {
            x87!(["fld tbyte ptr [{b}]", "fld tbyte ptr [{a}]", "fucomip st, st(1)", "fstp st(0)", "sete {r}", "setnp {p}"],
                [a = in(reg) &a, b = in(reg) &b, r = out(reg_byte) r, p = out(reg_byte) p,], (nostack));
        }
        r & p != 0
    }

    /// unordered: `fucomip` then `setp`
    #[inline(always)]
    pub fn unordered(a: F80, b: F80) -> bool {
        let r: u8;
        unsafe {
            x87!(["fld tbyte ptr [{b}]", "fld tbyte ptr [{a}]", "fucomip st, st(1)", "fstp st(0)", "setp {r}"],
                [a = in(reg) &a, b = in(reg) &b, r = out(reg_byte) r,], (nostack));
        }
        r != 0
    }

    /// `frndint` under the rounding control `mode` (the control word is set and put back to
    /// [`CW`], as the compiler's sequence around it does).
    #[inline(always)]
    pub fn round_int(a: F80, mode: Round) -> F80 {
        let mut o = MaybeUninit::<F80>::uninit();
        #[cfg(test)]
        let mode = if tests::mutant() == tests::M_FLOOR_AS_TRUNC && mode == Round::Down { Round::Zero } else { mode };
        unsafe {
            if mode == Round::Nearest {
                x87!(["fld tbyte ptr [{a}]", "frndint", "fstp tbyte ptr [{o}]"], [a = in(reg) &a, o = in(reg) o.as_mut_ptr(),], (nostack, preserves_flags));
            } else {
                x87!(["fldcw word ptr [{c}]", "fld tbyte ptr [{a}]", "frndint", "fstp tbyte ptr [{o}]", "fldcw word ptr [{s}]"],
                    [a = in(reg) &a, o = in(reg) o.as_mut_ptr(), c = in(reg) cw_ptr(mode), s = in(reg) &CW_STD,], (nostack, preserves_flags));
            }
            o.assume_init()
        }
    }

    macro_rules! fist {
        ($name:ident, $t:ty, $w:literal) => {
            /// `fistp` under the rounding control `mode`; the integer indefinite when the
            /// value does not fit.
            #[inline(always)]
            pub fn $name(a: F80, mode: Round) -> $t {
                let mut o = MaybeUninit::<$t>::uninit();
                #[cfg(test)]
                let mode = if tests::mutant() == tests::M_FIST_NEAREST { Round::Nearest } else { mode };
                unsafe {
                    if mode == Round::Nearest {
                        x87!(["fld tbyte ptr [{a}]", concat!("fistp ", $w, " ptr [{o}]")], [a = in(reg) &a, o = in(reg) o.as_mut_ptr(),], (nostack, preserves_flags));
                    } else {
                        x87!(["fldcw word ptr [{c}]", "fld tbyte ptr [{a}]", concat!("fistp ", $w, " ptr [{o}]"), "fldcw word ptr [{s}]"],
                            [a = in(reg) &a, o = in(reg) o.as_mut_ptr(), c = in(reg) cw_ptr(mode), s = in(reg) &CW_STD,], (nostack, preserves_flags));
                    }
                    o.assume_init()
                }
            }
        };
    }
    fist!(to_i16, i16, "word");
    fist!(to_i32, i32, "dword");
    fist!(to_i64, i64, "qword");
}

// ---- the checked operations F80's methods and operators call

macro_rules! checked2 {
    ($($name:ident),*) => {$(
        #[inline(always)]
        pub fn $name(a: F80, b: F80) -> Option<F80> {
            if ok2(a, b) { Some(raw::$name(a, b)) } else { None }
        }
    )*};
}
checked2!(add, sub, mul, div);

macro_rules! checked_mixed {
    ($($name:ident, $rname:ident, $t:ty);*) => {$(
        #[inline(always)]
        pub fn $name(a: F80, b: $t) -> Option<F80> {
            if ok1(a) { Some(raw::$name(a, b)) } else { None }
        }
        #[inline(always)]
        pub fn $rname(a: $t, b: F80) -> Option<F80> {
            if ok1(b) { Some(raw::$rname(a, b)) } else { None }
        }
    )*};
}
checked_mixed!(add_f32, f32_add, f32; sub_f32, f32_sub, f32; mul_f32, f32_mul, f32; div_f32, f32_div, f32;
    add_f64, f64_add, f64; sub_f64, f64_sub, f64; mul_f64, f64_mul, f64; div_f64, f64_div, f64);

#[inline(always)]
pub fn sqrt(a: F80) -> Option<F80> {
    if ok1(a) {
        Some(raw::sqrt(a))
    } else {
        None
    }
}

#[inline(always)]
pub fn to_f32(a: F80) -> Option<f32> {
    if ok1(a) {
        Some(raw::to_f32(a))
    } else {
        None
    }
}

#[inline(always)]
pub fn to_f64(a: F80) -> Option<f64> {
    if ok1(a) {
        Some(raw::to_f64(a))
    } else {
        None
    }
}

#[inline(always)]
pub fn compare(a: F80, b: F80) -> Option<Option<Ordering>> {
    if ok2(a, b) {
        Some(raw::compare(a, b))
    } else {
        None
    }
}

/// The comparisons [`F80`]'s `lt`, `gt`, `eq`, `le`, `ge` and `unordered` are made of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Test {
    /// `a > b`
    Gt,
    /// `a >= b`
    Ge,
    /// `a == b`
    Eq,
    /// unordered
    Unordered,
}

#[inline(always)]
pub fn test(t: Test, a: F80, b: F80) -> Option<bool> {
    if !ok2(a, b) {
        return None;
    }
    Some(match t {
        Test::Gt => raw::gt(a, b),
        Test::Ge => raw::ge(a, b),
        Test::Eq => raw::eq(a, b),
        Test::Unordered => raw::unordered(a, b),
    })
}

#[inline(always)]
pub fn round_int(a: F80, mode: Round) -> Option<F80> {
    if ok1(a) {
        Some(raw::round_int(a, mode))
    } else {
        None
    }
}

#[inline(always)]
pub fn to_i16(a: F80, mode: Round) -> Option<i16> {
    if ok1(a) {
        Some(raw::to_i16(a, mode))
    } else {
        None
    }
}

#[inline(always)]
pub fn to_i32(a: F80, mode: Round) -> Option<i32> {
    if ok1(a) {
        Some(raw::to_i32(a, mode))
    } else {
        None
    }
}

#[inline(always)]
pub fn to_i64(a: F80, mode: Round) -> Option<i64> {
    if ok1(a) {
        Some(raw::to_i64(a, mode))
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::Cell;

    // ---- the mutation switch: a deliberately wrong hardware operation, per thread
    pub const M_NONE: u8 = 0;
    pub const M_ADD_PC53: u8 = 1;
    pub const M_SUB_REVERSED: u8 = 2;
    pub const M_MUL_RC_UP: u8 = 3;
    pub const M_DIV_REVERSED: u8 = 4;
    pub const M_SQRT_PC53: u8 = 5;
    pub const M_F32_TWICE: u8 = 6;
    pub const M_F64_RC_ZERO: u8 = 7;
    pub const M_GT_UNORDERED: u8 = 8;
    pub const M_MEMORY_OPERAND: u8 = 9;
    pub const M_FLOOR_AS_TRUNC: u8 = 10;
    pub const M_FIST_NEAREST: u8 = 11;
    pub const M_GUARD_CANON: u8 = 12;
    pub const M_GUARD_CW: u8 = 13;
    pub const MUTANTS: [(u8, &str); 13] = [
        (M_ADD_PC53, "add under 53-bit precision"),
        (M_SUB_REVERSED, "subtract with the operands reversed (fsubrp)"),
        (M_MUL_RC_UP, "multiply rounding up"),
        (M_DIV_REVERSED, "divide with the operands reversed (fdivrp)"),
        (M_SQRT_PC53, "square root under 53-bit precision"),
        (M_F32_TWICE, "store to float through double (rounded twice)"),
        (M_F64_RC_ZERO, "store to double rounding toward zero"),
        (M_GT_UNORDERED, "> true when unordered"),
        (M_MEMORY_OPERAND, "F80 + f32/f64 as fadd from memory (no load first)"),
        (M_FLOOR_AS_TRUNC, "frndint down as toward zero"),
        (M_FIST_NEAREST, "fistp ignoring the rounding mode"),
        (M_GUARD_CANON, "guard lets non-canonical operands through"),
        (M_GUARD_CW, "guard ignores the control word"),
    ];

    /// Runs `f` with the mutation switch off (a mutant built from the right operation).
    pub fn off<R>(f: impl FnOnce() -> R) -> R {
        let m = mutant();
        set_mutant(M_NONE);
        let r = f();
        set_mutant(m);
        r
    }

    /// Runs `f` under the control word `cw`.
    pub fn under<R>(cw: u16, f: impl FnOnce() -> R) -> R {
        let saved = control_word();
        set_control_word(cw);
        let r = f();
        set_control_word(saved);
        r
    }

    pub fn mutated2(op: &str, a: F80, b: F80) -> Option<F80> {
        match (mutant(), op) {
            (M_ADD_PC53, "add") => Some(off(|| under(0x027f, || raw::add(a, b)))),
            (M_SUB_REVERSED, "sub") => Some(off(|| raw::sub(b, a))),
            (M_MUL_RC_UP, "mul") => Some(off(|| under(cw_of(Round::Up), || raw::mul(a, b)))),
            (M_DIV_REVERSED, "div") => Some(off(|| raw::div(b, a))),
            _ => None,
        }
    }

    /// `fadd m32` / `fadd m64` straight from memory, without the load first.
    pub fn mutated_mixed(op: &str, a: F80, b: f64) -> Option<F80> {
        if mutant() != M_MEMORY_OPERAND {
            return None;
        }
        let mut o = MaybeUninit::<F80>::uninit();
        unsafe {
            match op {
                "add_f32" => {
                    let b = b as f32; // exact: the value came from an f32 (a signalling NaN stays one: SSE conversion)
                    x87!(["fld tbyte ptr [{a}]", "fadd dword ptr [{b}]", "fstp tbyte ptr [{o}]"], [a = in(reg) &a, b = in(reg) &b, o = in(reg) o.as_mut_ptr(),], (nostack));
                }
                "add_f64" => {
                    x87!(["fld tbyte ptr [{a}]", "fadd qword ptr [{b}]", "fstp tbyte ptr [{o}]"], [a = in(reg) &a, b = in(reg) &b, o = in(reg) o.as_mut_ptr(),], (nostack));
                }
                _ => return None,
            }
            Some(o.assume_init())
        }
    }

    thread_local! {
        static MUTANT: Cell<u8> = const { Cell::new(M_NONE) };
    }
    #[inline(always)]
    pub fn mutant() -> u8 {
        MUTANT.with(|m| m.get())
    }
    pub fn set_mutant(m: u8) {
        MUTANT.with(|c| c.set(m));
    }

    // ---- operands
    use crate::x87::testing::Rng;

    fn bits(v: F80) -> String {
        format!("{:04x}:{:016x}", v.se, v.m)
    }

    /// The software model's encodings of interest, all canonical: what loads and operations
    /// produce, with the edges of every operation over-represented.
    pub fn edge(r: &mut Rng) -> F80 {
        let k = r.next() % 32;
        let neg = ((r.next() >> 7) & 1) as u16;
        let s = neg << 15;
        let any = r.next();
        let f = |m: u64, e: u16| F80 { m, se: s | e };
        let v = match k {
            0..=5 => r.f80(),
            6 => f(0, 0),
            7 => f(1 << 63, 0x7fff),
            // quiet NaNs with payloads in every part of the significand, and signalling ones
            8 => f(0xc000_0000_0000_0000 | any >> (2 + r.next() % 62), 0x7fff),
            9 => f(0x8000_0000_0000_0000 | (any >> (2 + r.next() % 62)).max(1), 0x7fff),
            // denormals of every width, and the smallest normals
            10 => f(any >> (1 + r.next() % 63), 0),
            11 => f(1 << 63 | any >> (r.next() % 64), 1 + (r.next() % 3) as u16),
            // around the edges of float and double: overflow, the smallest normal, denormals,
            // underflow to zero
            12 => {
                let e = [127i32, 128, -126, -127, -149, -150, -151, 1023, 1024, -1022, -1023, -1074, -1075, -1076][(r.next() % 14) as usize];
                f(1 << 63 | any >> (r.next() % 64), (16383 + e + (r.next() % 3) as i32 - 1) as u16)
            }
            // ties and near-ties of the float and double stores: the bits below the kept ones
            // are 100…0, 011…1 or 100…01
            13 | 14 => {
                let keep = [24u32, 53, 23, 52, 1, 2, 63][(r.next() % 7) as usize];
                let hi = (1u64 << 63) | (any & !(u64::MAX >> keep));
                let below = 64 - keep;
                let tail = match r.next() % 4 {
                    0 => 1u64 << (below - 1),
                    1 => (1u64 << (below - 1)) - 1,
                    2 => (1u64 << (below - 1)) | 1,
                    _ => (1u64 << (below - 1)) | (r.next() & ((1u64 << (below - 1)) - 1)),
                };
                let e = [0i32, 127, -126, -127, -140, -149, 1023, -1022, -1030, -1074, 30, -1][(r.next() % 12) as usize];
                f(hi | tail, (16383 + e) as u16)
            }
            // integers and halves around the limits of the integer stores
            15 | 16 => {
                let n = [0u32, 1, 2, 14, 15, 16, 30, 31, 32, 52, 53, 62, 63, 64][(r.next() % 14) as usize];
                let base = F80::from_i64(1).soft_mul(F80 { m: 1 << 63, se: (16383 + n as i32) as u16 });
                let d = [0.0f64, 0.5, -0.5, 1.0, -1.0, 0.25, 0.75, -0.25, 1.5, 2.5][(r.next() % 10) as usize];
                let v = base.soft_add(F80::from_f64(d));
                if neg == 1 {
                    -v
                } else {
                    v
                }
            }
            17 => F80::from_i64(any as i64 >> (r.next() % 64)).soft_add(F80::from_f64([0.0, 0.5, 0.25, 0.75][(r.next() % 4) as usize])),
            // small fractions for frndint
            18 => f(1 << 63 | any >> (r.next() % 64), (16383 - 3 + (r.next() % 70) as i32) as u16),
            // floats and doubles as loaded
            19..=22 => F80::from_f32(f32_edge(r)),
            23..=26 => F80::from_f64(f64_edge(r)),
            // products and sums of floats, as the renderer makes them
            _ => {
                let a = F80::from_f32(f32_edge(r)).soft_mul(F80::from_f32(f32_edge(r)));
                a.soft_add(F80::from_f32(f32_edge(r)))
            }
        };
        debug_assert!(canonical(v), "{}", bits(v));
        v
    }

    pub fn f32_edge(r: &mut Rng) -> f32 {
        let a = r.next();
        let s = (a >> 63) as u32 * 0x8000_0000;
        f32::from_bits(match a % 16 {
            0 => s | 0x7f80_0000 | ((a >> 8) as u32 & 0x3f_ffff).max(1), // signalling NaN
            1 => s | 0x7fc0_0000 | (a >> 8) as u32 & 0x3f_ffff,         // quiet NaN
            2 => s | 0x7f80_0000,
            3 => s,
            4 => s | (a >> 8) as u32 & 0x7f_ffff, // denormal
            5..=7 => s | ((((a >> 40) % 254 + 1) as u32) << 23) | (a >> 8) as u32 & 0x7f_ffff,
            8..=10 => s | (((127 + (a >> 40) % 30 - 15) as u32) << 23) | (a >> 8) as u32 & 0x7f_ffff,
            _ => (a >> 16) as u32,
        })
    }

    pub fn f64_edge(r: &mut Rng) -> f64 {
        let a = r.next();
        let s = a & (1 << 63);
        let fr = (a >> 3) & 0xf_ffff_ffff_ffff;
        f64::from_bits(match a % 16 {
            0 => s | 0x7ff0_0000_0000_0000 | (fr & 0x7_ffff_ffff_ffff).max(1),
            1 => s | 0x7ff8_0000_0000_0000 | fr,
            2 => s | 0x7ff0_0000_0000_0000,
            3 => s,
            4 => s | fr,
            5..=7 => s | (((a >> 52) % 2046 + 1) << 52) | fr,
            8..=10 => s | ((1023 + (a >> 52) % 60 - 30) << 52) | fr,
            11 => (f32_edge(r) as f64).to_bits(),
            _ => r.next(),
        })
    }

    /// An encoding no load or operation produces (for the guard).
    pub fn non_canonical(r: &mut Rng) -> F80 {
        let a = r.next();
        let s = ((a >> 63) as u16) << 15;
        match a % 4 {
            0 => F80 { m: r.next() >> 1, se: s | (1 + r.next() % 0x7ffe) as u16 }, // unnormal (or pseudo-zero)
            1 => F80 { m: r.next() >> 1, se: s | 0x7fff },                       // pseudo-NaN / pseudo-infinity
            2 => F80 { m: 1 << 63 | r.next(), se: s },                           // pseudo-denormal
            _ => F80 { m: (r.next() >> (1 + r.next() % 63)) & !(1 << 63), se: s | (r.next() % 3 + 1) as u16 },
        }
    }

    const MODES: [Round; 4] = [Round::Nearest, Round::Down, Round::Up, Round::Zero];

    fn cmp_code(c: Option<Ordering>) -> u8 {
        match c {
            Some(Ordering::Less) => 0,
            Some(Ordering::Equal) => 1,
            Some(Ordering::Greater) => 2,
            None => 3,
        }
    }

    fn same(x: F80, y: F80) -> bool {
        x.to_bits() == y.to_bits()
    }

    /// Per operation: cases compared and the first difference.
    #[derive(Default)]
    pub struct Tally {
        pub cases: Vec<(&'static str, u64, u64)>, // name, compared, different
        pub first: Option<String>,
    }
    impl Tally {
        pub fn note(&mut self, i: usize, name: &'static str, ok: bool, what: impl FnOnce() -> String) {
            if self.cases.len() <= i {
                self.cases.resize(i + 1, ("", 0, 0));
            }
            let c = &mut self.cases[i];
            c.0 = name;
            c.1 += 1;
            if !ok {
                c.2 += 1;
                if self.first.is_none() {
                    self.first = Some(format!("{name}: {}", what()));
                }
            }
        }
        pub fn different(&self) -> u64 {
            self.cases.iter().map(|c| c.2).sum()
        }
        pub fn merge(&mut self, o: Tally) {
            for (i, c) in o.cases.iter().enumerate() {
                if self.cases.len() <= i {
                    self.cases.resize(i + 1, ("", 0, 0));
                }
                self.cases[i].0 = c.0;
                self.cases[i].1 += c.1;
                self.cases[i].2 += c.2;
            }
            if self.first.is_none() {
                self.first = o.first;
            }
        }
    }

    /// Every raw operation against the software model on `n` operand pairs.
    pub fn differential(seed: u64, n: u64, t: &mut Tally) {
        let mut r = Rng(seed | 1);
        for i in 0..n {
            let a = edge(&mut r);
            let mut b = edge(&mut r);
            if i % 5 == 0 {
                // close operands: cancellation, equal values, ties
                b = F80 { m: a.m ^ (r.next() & (u64::MAX >> (1 + r.next() % 63)) & !(1 << 63)), se: a.se ^ (((r.next() % 3 == 0) as u16) << 15) };
                if !canonical(b) {
                    b = a;
                }
            } else if i % 5 == 1 && a.se & 0x7fff != 0 && a.se & 0x7fff != 0x7fff {
                // exponents a few apart: the alignment shifts of addition and the sticky bit
                let e = ((a.se & 0x7fff) as i32 + (r.next() % 140) as i32 - 70).clamp(1, 0x7ffe) as u16;
                b = F80 { m: 1 << 63 | r.next(), se: (b.se & 0x8000) | e };
            }
            let (f, d) = (f32_edge(&mut r), f64_edge(&mut r));
            let fx = || format!("{} {}", bits(a), bits(b));
            t.note(0, "add", same(raw::add(a, b), a.soft_add(b)), fx);
            t.note(1, "sub", same(raw::sub(a, b), a.soft_sub(b)), fx);
            t.note(2, "mul", same(raw::mul(a, b), a.soft_mul(b)), fx);
            t.note(3, "div", same(raw::div(a, b), a.soft_div(b)), fx);
            let ff = || format!("{} {:08x} {:016x}", bits(a), f.to_bits(), d.to_bits());
            let (xf, xd) = (F80::from_f32(f), F80::from_f64(d));
            let m32 = same(raw::add_f32(a, f), a.soft_add(xf)) & same(raw::sub_f32(a, f), a.soft_sub(xf)) & same(raw::mul_f32(a, f), a.soft_mul(xf)) & same(raw::div_f32(a, f), a.soft_div(xf));
            let m32r = same(raw::f32_add(f, a), xf.soft_add(a)) & same(raw::f32_sub(f, a), xf.soft_sub(a)) & same(raw::f32_mul(f, a), xf.soft_mul(a)) & same(raw::f32_div(f, a), xf.soft_div(a));
            let m64 = same(raw::add_f64(a, d), a.soft_add(xd)) & same(raw::sub_f64(a, d), a.soft_sub(xd)) & same(raw::mul_f64(a, d), a.soft_mul(xd)) & same(raw::div_f64(a, d), a.soft_div(xd));
            let m64r = same(raw::f64_add(d, a), xd.soft_add(a)) & same(raw::f64_sub(d, a), xd.soft_sub(a)) & same(raw::f64_mul(d, a), xd.soft_mul(a)) & same(raw::f64_div(d, a), xd.soft_div(a));
            t.note(4, "F80 op f32 (4 ops)", m32, ff);
            t.note(5, "f32 op F80 (4 ops)", m32r, ff);
            t.note(6, "F80 op f64 (4 ops)", m64, ff);
            t.note(7, "f64 op F80 (4 ops)", m64r, ff);
            let fa = || bits(a);
            t.note(8, "sqrt", same(raw::sqrt(a), a.soft_sqrt()), fa);
            t.note(9, "to_f32", raw::to_f32(a).to_bits() == a.soft_to_f32().to_bits(), fa);
            t.note(10, "to_f64", raw::to_f64(a).to_bits() == a.soft_to_f64().to_bits(), fa);
            let c = a.soft_compare(b);
            t.note(11, "compare", cmp_code(raw::compare(a, b)) == cmp_code(c), fx);
            let tests_ok = raw::gt(a, b) == (c == Some(Ordering::Greater))
                && raw::gt(b, a) == (c == Some(Ordering::Less))
                && raw::ge(a, b) == matches!(c, Some(Ordering::Greater | Ordering::Equal))
                && raw::ge(b, a) == matches!(c, Some(Ordering::Less | Ordering::Equal))
                && raw::eq(a, b) == (c == Some(Ordering::Equal))
                && raw::unordered(a, b) == c.is_none();
            t.note(12, "lt gt le ge eq unordered", tests_ok, fx);
            for mode in MODES {
                let fm = || format!("{mode:?} {}", bits(a));
                t.note(13, "round_int (4 modes)", same(raw::round_int(a, mode), a.soft_round_int(mode)), fm);
                t.note(14, "to_i16 (4 modes)", raw::to_i16(a, mode) == a.soft_to_i16(mode), fm);
                t.note(15, "to_i32 (4 modes)", raw::to_i32(a, mode) == a.soft_to_i32(mode), fm);
                t.note(16, "to_i64 (4 modes)", raw::to_i64(a, mode) == a.soft_to_i64(mode), fm);
            }
            let fd = || format!("{:08x} {:016x}", f.to_bits(), d.to_bits());
            t.note(17, "fld m32 / m64", same(raw::from_f32(f), xf) & same(raw::from_f64(d), xd), fd);
        }
    }

    /// The operators and methods of [`F80`] (hardware with its guard) against the software
    /// model, on operands that include encodings the guard must send to the software model.
    pub fn through_the_guard(seed: u64, n: u64, t: &mut Tally) {
        let mut r = Rng(seed | 1);
        for i in 0..n {
            let a = if i % 2 == 0 { non_canonical(&mut r) } else { edge(&mut r) };
            let b = if i % 3 == 0 { non_canonical(&mut r) } else { edge(&mut r) };
            let f = f32_edge(&mut r);
            let fx = || format!("{} {}", bits(a), bits(b));
            let arith = same(a + b, a.soft_add(b)) & same(a - b, a.soft_sub(b)) & same(a * b, a.soft_mul(b)) & same(a / b, a.soft_div(b));
            t.note(0, "+ - * / (F80)", arith, fx);
            let xf = F80::from_f32(f);
            let mixed = same(a + f, a.soft_add(xf)) & same(f - a, xf.soft_sub(a)) & same(a * f as f64, a.soft_mul(F80::from_f64(f as f64))) & same(a / f, a.soft_div(xf));
            t.note(1, "+ - * / (mixed)", mixed, fx);
            let un = same(a.sqrt(), a.soft_sqrt()) & (a.to_f32().to_bits() == a.soft_to_f32().to_bits()) & (a.to_f64().to_bits() == a.soft_to_f64().to_bits());
            t.note(2, "sqrt to_f32 to_f64", un, fx);
            let c = a.soft_compare(b);
            let cmp = cmp_code(a.compare(b)) == cmp_code(c)
                && a.lt(b) == (c == Some(Ordering::Less))
                && a.gt(b) == (c == Some(Ordering::Greater))
                && a.le(b) == matches!(c, Some(Ordering::Less | Ordering::Equal))
                && a.ge(b) == matches!(c, Some(Ordering::Greater | Ordering::Equal))
                && a.eq(b) == (c == Some(Ordering::Equal))
                && a.unordered(b) == c.is_none();
            t.note(3, "compare and tests", cmp, fx);
            let mut ints = true;
            for mode in MODES {
                ints &= same(a.round_int(mode), a.soft_round_int(mode)) && a.to_i16(mode) == a.soft_to_i16(mode) && a.to_i32(mode) == a.soft_to_i32(mode) && a.to_i64(mode) == a.soft_to_i64(mode);
            }
            t.note(4, "round_int to_i16 to_i32 to_i64", ints, fx);
        }
    }

    /// The same under every other precision and rounding control (exceptions still masked,
    /// and one with exceptions unmasked): the guard must hand all of it to the software model.
    pub fn foreign_control_words(seed: u64, n: u64, t: &mut Tally) {
        // bits 0-5 exception masks, bit 6 reserved (set), 8-9 precision, 10-11 rounding
        let mut words: Vec<u16> = (0..16u16).map(|k| 0x007f | (k & 3) << 8 | (k >> 2) << 10).filter(|&w| w != CW).collect();
        // and words that unmask exceptions (left out for the mutant whose guard ignores the
        // word: the unit then traps, SIGFPE, which ends the test run instead of failing it)
        if mutant() != M_GUARD_CW {
            words.push(0x0372); // invalid, denormal and zero-divide unmasked
            words.push(0x0340); // every exception unmasked
        }
        let saved = control_word();
        for (k, &w) in words.iter().enumerate() {
            let mut sub = Tally::default();
            set_control_word(w);
            through_the_guard(seed ^ (k as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15), n, &mut sub);
            set_control_word(saved);
            if sub.different() > 0 && t.first.is_none() {
                t.first = Some(format!("control word {w:04x}: {}", sub.first.clone().unwrap_or_default()));
            }
            t.merge(sub);
        }
    }

    fn cases(default: u64) -> u64 {
        std::env::var("SI_X87_HW_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(default)
    }

    fn report(name: &str, t: &Tally) {
        for (op, n, d) in &t.cases {
            eprintln!("{name}\t{op}\t{n}\t{d}");
        }
    }

    #[test]
    fn hardware_equals_software() {
        assert_eq!(control_word(), CW, "the test process's control word");
        // several threads, each its own seed: SI_X87_HW_CASES pairs in all
        let n = cases(400_000);
        let k = std::thread::available_parallelism().map_or(1, |p| p.get()).min(4) as u64;
        let parts: Vec<Tally> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..k)
                .map(|j| {
                    s.spawn(move || {
                        let mut t = Tally::default();
                        differential(0x5eed_0000_0000_0001 ^ j.wrapping_mul(0x9e37_79b9_7f4a_7c15), n / k, &mut t);
                        t
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let mut all = Tally::default();
        for p in parts {
            all.merge(p);
        }
        report("differential", &all);
        assert_eq!(all.different(), 0, "{}", all.first.unwrap_or_default());
    }

    #[test]
    fn the_guard_sends_what_hardware_would_answer_differently_to_software() {
        let mut t = Tally::default();
        through_the_guard(0x0bad_cafe, cases(400_000) / 4, &mut t);
        report("guard", &t);
        assert_eq!(t.different(), 0, "{}", t.first.unwrap_or_default());
        // and the guard is needed: the raw unit answers many of those operands differently
        let mut r = Rng(77);
        let mut raw_differs = 0;
        for _ in 0..10_000 {
            let (a, b) = (non_canonical(&mut r), edge(&mut r));
            raw_differs += (!same(raw::add(a, b), a.soft_add(b))) as u32;
        }
        eprintln!("guard\traw unit on non-canonical operands, add differing\t10000\t{raw_differs}");
        assert!(raw_differs > 1000);
    }

    #[test]
    fn foreign_control_words_go_to_software() {
        let mut t = Tally::default();
        foreign_control_words(0x00c0_ffee, cases(400_000) / 64, &mut t);
        report("control words", &t);
        assert_eq!(t.different(), 0, "{}", t.first.unwrap_or_default());
        assert_eq!(control_word(), CW);
    }

    /// `fld m32` (what the mixed operators load with) against `F80::from_f32` for every
    /// float when SI_X87_HW_EXHAUSTIVE is set, otherwise for every 251st.
    #[test]
    fn float_loads() {
        let step = if std::env::var_os("SI_X87_HW_EXHAUSTIVE").is_some() { 1 } else { 251 };
        let k = 4u64;
        let bad: u64 = std::thread::scope(|s| {
            let hs: Vec<_> = (0..k)
                .map(|j| {
                    s.spawn(move || {
                        let mut bad = 0u64;
                        let mut v = j * step;
                        while v <= u32::MAX as u64 {
                            let f = f32::from_bits(v as u32);
                            bad += (!same(raw::from_f32(f), F80::from_f32(f))) as u64;
                            v += k * step;
                        }
                        bad
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).sum()
        });
        eprintln!("fld m32\tfloats\t{}\t{bad}", (1u64 << 32) / step);
        assert_eq!(bad, 0);
    }

    /// Each deliberately wrong variant of the hardware path must fail one of the checks above.
    #[test]
    fn every_mutant_is_caught() {
        let n = (cases(400_000) / 8).max(20_000);
        let mut missed = Vec::new();
        for (m, name) in MUTANTS {
            set_mutant(m);
            let mut t = Tally::default();
            differential(0xfeed ^ m as u64, n, &mut t);
            let mut g = Tally::default();
            through_the_guard(0xbeef ^ m as u64, n / 4, &mut g);
            let mut c = Tally::default();
            foreign_control_words(0xface ^ m as u64, n / 64, &mut c);
            set_mutant(M_NONE);
            let caught = t.different() + g.different() + c.different();
            eprintln!("mutant\t{name}\t{n}\t{caught}");
            if caught == 0 {
                missed.push(name);
            }
        }
        assert!(missed.is_empty(), "mutants not caught: {missed:?}");
    }
}
