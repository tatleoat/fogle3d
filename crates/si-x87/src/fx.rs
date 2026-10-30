// SPDX-License-Identifier: MIT
//! `fx!`: one expression of [`F80`] operations as one block on the x87 unit.
//!
//! Each operator of [`F80`] on the hardware backend loads its operands from memory and
//! stores its result there (`fld m80` … `fstp m80`), and the store of the 80-bit result and
//! the load of the next operation's operand cost more than the operation itself. `fx!` takes
//! a whole expression, written as a tree, and evaluates it in the unit's registers: its
//! leaves are loaded once (`fld m32`, `fld m64`, `fld m80`), its operations are the register
//! forms the operators use, and only the root is stored. Keeping a value in a register
//! instead of storing it as 80 bits and loading it back changes nothing (the store and the
//! load are exact for every encoding the unit produces), so the block's answer is the
//! answer of the same expression written with the operators: the software model's. The
//! guard is the operators' guard, taken once for the block: every [`F80`] leaf canonical
//! and the control word 0x037f, or the software model computes the whole expression.
//!
//! ```text
//! fx!(tree)       -> F80
//! fx!(f32: tree)  -> f32   (the root stored as a float: `fstp m32`, as `.to_f32()`)
//! tree := (add tree tree) | (sub tree tree) | (mul tree tree) | (div tree tree)
//!       | (sqrt tree) | (abs tree) | (neg tree)
//!       | (f <f32 expression>) | (d <f64 expression>) | (x <F80 expression>)
//! ```
//!
//! `(sub a b)` is a − b and `(div a b)` is a ÷ b. The left operand is evaluated first, as
//! with the operators. So `x(d.z) * n.z + (x(d.y) * n.y + x(n.x) * d.x)` is
//! `fx!((add (mul (f d.z) (f n.z)) (add (mul (f d.y) (f n.y)) (mul (f n.x) (f d.x)))))`.
//! A leaf's expression is evaluated once for the block and once more when the software model
//! takes over, so it must have no side effects (a field, a local, an element). An
//! expression that needs more than the unit's eight registers does not compile.
//!
//! ```
//! use si_x87::{fx, x, F80};
//! let (d, n) = ([0.6f32, 0.0, 0.8], [0.0f32, 1.0, 0.5]);
//! let dn = fx!((add (mul (f d[2]) (f n[2])) (add (mul (f d[1]) (f n[1])) (mul (f n[0]) (f d[0])))));
//! assert_eq!(dn.to_bits(), (x(d[2]) * n[2] + (x(d[1]) * n[1] + x(n[0]) * d[0])).to_bits());
//! let r: f32 = fx!(f32: (div (x F80::ONE) (sqrt (x dn))));
//! assert_eq!(r, (F80::ONE / dn.sqrt()).to_f32());
//! ```
//!
//! Nine values at once do not fit:
//!
//! ```compile_fail
//! let v = si_x87::fx!((add (f 1.0) (add (f 1.0) (add (f 1.0) (add (f 1.0) (add (f 1.0)
//!     (add (f 1.0) (add (f 1.0) (add (f 1.0) (f 1.0))))))))));
//! ```
//!
//! Without the hardware backend (the default; other processors) the macro is the software
//! model's expression. Checked by `fx::tests` against the operators' software model on random
//! and edge operands, with mutants.

#[allow(unused_imports)]
use crate::x87::F80; // for the links above

/// True when the values a block keeps in the unit's registers never exceed eight: `d` is
/// +1 for each leaf, −1 for each two-operand operation, in order of evaluation.
#[doc(hidden)]
pub const fn fits(d: &[i32]) -> bool {
    let mut s = 0;
    let mut i = 0;
    while i < d.len() {
        s += d[i];
        if s > 8 {
            return false;
        }
        i += 1;
    }
    true
}

/// The software model's value of an `fx!` tree (also the fallback of the hardware block).
#[doc(hidden)]
#[macro_export]
macro_rules! __fx_soft {
    ((add $a:tt $b:tt)) => { $crate::F80::soft_add($crate::__fx_soft!($a), $crate::__fx_soft!($b)) };
    ((sub $a:tt $b:tt)) => { $crate::F80::soft_sub($crate::__fx_soft!($a), $crate::__fx_soft!($b)) };
    ((mul $a:tt $b:tt)) => { $crate::F80::soft_mul($crate::__fx_soft!($a), $crate::__fx_soft!($b)) };
    ((div $a:tt $b:tt)) => { $crate::F80::soft_div($crate::__fx_soft!($a), $crate::__fx_soft!($b)) };
    ((sqrt $a:tt)) => { $crate::F80::soft_sqrt($crate::__fx_soft!($a)) };
    ((abs $a:tt)) => { $crate::F80::abs($crate::__fx_soft!($a)) };
    ((neg $a:tt)) => { ::core::ops::Neg::neg($crate::__fx_soft!($a)) };
    ((f $($e:tt)+)) => { $crate::F80::from_f32($($e)+) };
    ((d $($e:tt)+)) => { $crate::F80::from_f64($($e)+) };
    ((x $($e:tt)+)) => {{ let v: $crate::F80 = $($e)+; v }};
}

/// The walk of an `fx!` tree in order of evaluation (a work list): each leaf bound to a
/// local of its own (each step of the walk is an expansion of its own, so the locals are
/// distinct) and loaded by a `{}` operand of its own, each operation one instruction; then
/// [`__fx_block`] with what was gathered.
#[doc(hidden)]
#[macro_export]
macro_rules! __fx_walk {
    // [work] [lets] [checks] [instructions] [operands] [stack depth steps] kind tree
    (@go [(add $a:tt $b:tt) $($w:tt)*] $l:tt $c:tt $t:tt $o:tt $d:tt $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$a $b @add $($w)*] $l $c $t $o $d $k $tree)
    };
    (@go [(sub $a:tt $b:tt) $($w:tt)*] $l:tt $c:tt $t:tt $o:tt $d:tt $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$a $b @sub $($w)*] $l $c $t $o $d $k $tree)
    };
    (@go [(mul $a:tt $b:tt) $($w:tt)*] $l:tt $c:tt $t:tt $o:tt $d:tt $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$a $b @mul $($w)*] $l $c $t $o $d $k $tree)
    };
    (@go [(div $a:tt $b:tt) $($w:tt)*] $l:tt $c:tt $t:tt $o:tt $d:tt $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$a $b @div $($w)*] $l $c $t $o $d $k $tree)
    };
    (@go [(sqrt $a:tt) $($w:tt)*] $l:tt $c:tt $t:tt $o:tt $d:tt $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$a @sqrt $($w)*] $l $c $t $o $d $k $tree)
    };
    (@go [(abs $a:tt) $($w:tt)*] $l:tt $c:tt $t:tt $o:tt $d:tt $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$a @abs $($w)*] $l $c $t $o $d $k $tree)
    };
    (@go [(neg $a:tt) $($w:tt)*] $l:tt $c:tt $t:tt $o:tt $d:tt $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$a @neg $($w)*] $l $c $t $o $d $k $tree)
    };
    // leaves
    (@go [(f $($e:tt)+) $($w:tt)*] [$($l:tt)*] $c:tt [$($t:tt)*] [$($o:tt)*] [$($d:tt)*] $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$($w)*] [$($l)* let v: f32 = $($e)+;] $c [$($t)* "fld dword ptr [{}]",] [$($o)* in(reg) &v,] [$($d)* 1,] $k $tree)
    };
    (@go [(d $($e:tt)+) $($w:tt)*] [$($l:tt)*] $c:tt [$($t:tt)*] [$($o:tt)*] [$($d:tt)*] $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$($w)*] [$($l)* let v: f64 = $($e)+;] $c [$($t)* "fld qword ptr [{}]",] [$($o)* in(reg) &v,] [$($d)* 1,] $k $tree)
    };
    (@go [(x $($e:tt)+) $($w:tt)*] [$($l:tt)*] [$($c:tt)*] [$($t:tt)*] [$($o:tt)*] [$($d:tt)*] $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$($w)*] [$($l)* let v: $crate::F80 = $($e)+;] [$($c)* $crate::hw::fx_ok(v) &] [$($t)* "fld tbyte ptr [{}]",] [$($o)* in(reg) &v,] [$($d)* 1,] $k $tree)
    };
    // operations: the register forms of the operators (DE C1 faddp, DE E9 fsubp st1 − st0,
    // DE C9 fmulp, DE F9 fdivp st1 ÷ st0), encoded by hand as in hw::raw
    (@go [@add $($w:tt)*] $l:tt $c:tt [$($t:tt)*] $o:tt [$($d:tt)*] $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$($w)*] $l $c [$($t)* ".byte 0xde, 0xc1",] $o [$($d)* -1,] $k $tree)
    };
    (@go [@sub $($w:tt)*] $l:tt $c:tt [$($t:tt)*] $o:tt [$($d:tt)*] $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$($w)*] $l $c [$($t)* ".byte 0xde, 0xe9",] $o [$($d)* -1,] $k $tree)
    };
    (@go [@mul $($w:tt)*] $l:tt $c:tt [$($t:tt)*] $o:tt [$($d:tt)*] $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$($w)*] $l $c [$($t)* ".byte 0xde, 0xc9",] $o [$($d)* -1,] $k $tree)
    };
    (@go [@div $($w:tt)*] $l:tt $c:tt [$($t:tt)*] $o:tt [$($d:tt)*] $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$($w)*] $l $c [$($t)* ".byte 0xde, 0xf9",] $o [$($d)* -1,] $k $tree)
    };
    (@go [@sqrt $($w:tt)*] $l:tt $c:tt [$($t:tt)*] $o:tt $d:tt $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$($w)*] $l $c [$($t)* "fsqrt",] $o $d $k $tree)
    };
    (@go [@abs $($w:tt)*] $l:tt $c:tt [$($t:tt)*] $o:tt $d:tt $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$($w)*] $l $c [$($t)* "fabs",] $o $d $k $tree)
    };
    (@go [@neg $($w:tt)*] $l:tt $c:tt [$($t:tt)*] $o:tt $d:tt $k:ident $tree:tt) => {
        $crate::__fx_walk!(@go [$($w)*] $l $c [$($t)* "fchs",] $o $d $k $tree)
    };
    // the block
    (@go [] $l:tt $c:tt $t:tt $o:tt $d:tt $k:ident $tree:tt) => {
        $crate::__fx_block!($l $c $t $o $d $k $tree)
    };
}

/// The block on the unit, with the software model when the guard refuses.
#[cfg(hw_x87)]
#[doc(hidden)]
#[macro_export]
macro_rules! __fx_block {
    ([$($l:tt)*] [$($c:tt)*] [$($t:tt)*] [$($o:tt)*] [$($d:tt)*] F80 $tree:tt) => {{
        const _: () = assert!($crate::fx::fits(&[$($d)*]), "fx!: the expression needs more than the unit's eight registers");
        $($l)*
        if $($c)* $crate::hw::fx_cw_ok() {
            let mut dst = ::core::mem::MaybeUninit::<$crate::F80>::uninit();
            unsafe {
                ::core::arch::asm!($($t)* "fstp tbyte ptr [{dst}]", $($o)* dst = in(reg) dst.as_mut_ptr(),
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                    out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                    options(nostack, preserves_flags));
                dst.assume_init()
            }
        } else {
            $crate::__fx_soft!($tree)
        }
    }};
    ([$($l:tt)*] [$($c:tt)*] [$($t:tt)*] [$($o:tt)*] [$($d:tt)*] f32 $tree:tt) => {{
        const _: () = assert!($crate::fx::fits(&[$($d)*]), "fx!: the expression needs more than the unit's eight registers");
        $($l)*
        if $($c)* $crate::hw::fx_cw_ok() {
            let mut dst = ::core::mem::MaybeUninit::<f32>::uninit();
            unsafe {
                ::core::arch::asm!($($t)* "fstp dword ptr [{dst}]", $($o)* dst = in(reg) dst.as_mut_ptr(),
                    out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                    out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                    options(nostack, preserves_flags));
                dst.assume_init()
            }
        } else {
            $crate::F80::soft_to_f32($crate::__fx_soft!($tree))
        }
    }};
}


/// Without the backend: the software model's expression (the register limit still holds, so
/// that what compiles on one processor compiles on all).
#[cfg(not(hw_x87))]
#[doc(hidden)]
#[macro_export]
macro_rules! __fx_block {
    ($l:tt $c:tt $t:tt $o:tt [$($d:tt)*] F80 $tree:tt) => {{
        const _: () = assert!($crate::fx::fits(&[$($d)*]), "fx!: the expression needs more than the unit's eight registers");
        $crate::__fx_soft!($tree)
    }};
    ($l:tt $c:tt $t:tt $o:tt [$($d:tt)*] f32 $tree:tt) => {{
        const _: () = assert!($crate::fx::fits(&[$($d)*]), "fx!: the expression needs more than the unit's eight registers");
        $crate::F80::soft_to_f32($crate::__fx_soft!($tree))
    }};
}

/// One expression of [`F80`] operations as one block on the x87 unit (see the module).
#[macro_export]
macro_rules! fx {
    (f32: $tree:tt) => { $crate::__fx_walk!(@go [$tree] [] [] [] [] [] f32 $tree) };
    ($tree:tt) => { $crate::__fx_walk!(@go [$tree] [] [] [] [] [] F80 $tree) };
}

#[cfg(all(test, hw_x87))]
mod tests {
    use crate::hw::tests::{edge, f32_edge, f64_edge, non_canonical, set_mutant, under, Tally, M_GUARD_CANON, M_GUARD_CW, M_NONE};
    use crate::x87::testing::Rng;
    use crate::F80;

    fn same(a: F80, b: F80) -> bool {
        a.to_bits() == b.to_bits()
    }
    fn xf(v: f32) -> F80 {
        F80::from_f32(v)
    }
    fn cases(default: u64) -> u64 {
        std::env::var("SI_X87_HW_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(default)
    }

    /// Five shapes of the renderer's expressions against the same expressions written with
    /// the software model's operations; `bad` mixes in encodings the guard must refuse.
    fn shapes(seed: u64, n: u64, bad: bool, t: &mut Tally) {
        let mut r = Rng(seed | 1);
        for i in 0..n {
            let fs: Vec<f32> = (0..8).map(|_| f32_edge(&mut r)).collect();
            let (d0, d1) = (f64_edge(&mut r), f64_edge(&mut r));
            let a = if bad && i % 2 == 0 { non_canonical(&mut r) } else { edge(&mut r) };
            let b = if bad && i % 3 == 0 { non_canonical(&mut r) } else { edge(&mut r) };
            let (f0, f1, f2, f3, f4, f5, f6, f7) = (fs[0], fs[1], fs[2], fs[3], fs[4], fs[5], fs[6], fs[7]);
            let why = || format!("{:?} {d0:e} {d1:e} {:04x}:{:016x} {:04x}:{:016x}", fs, a.se, a.m, b.se, b.m);
            // a dot product in the renderer's order, then a plane distance divided by it
            let dn = fx!((add (mul (f f0) (f f1)) (add (mul (f f2) (f f3)) (mul (f f4) (f f5)))));
            let dn_s = xf(f0).soft_mul(xf(f1)).soft_add(xf(f2).soft_mul(xf(f3)).soft_add(xf(f4).soft_mul(xf(f5))));
            t.note(0, "dot product (3 mul, 2 add)", same(dn, dn_s), why);
            let tt = fx!((div (sub (f f6) (add (mul (f f0) (f f5)) (add (mul (f f1) (f f4)) (mul (f f2) (f f3))))) (x dn_s)));
            let tt_s = xf(f6).soft_sub(xf(f0).soft_mul(xf(f5)).soft_add(xf(f1).soft_mul(xf(f4)).soft_add(xf(f2).soft_mul(xf(f3))))).soft_div(dn_s);
            t.note(1, "plane distance (div, sub, 3 mul, 2 add)", same(tt, tt_s), why);
            // F80 leaves, a square root, a division and a float store
            let s = fx!(f32: (div (x a) (sqrt (add (mul (x a) (x a)) (x b)))));
            let s_s = a.soft_div(a.soft_mul(a).soft_add(b).soft_sqrt()).soft_to_f32();
            t.note(2, "F80 leaves, sqrt, div, store to float", s.to_bits() == s_s.to_bits(), why);
            // abs, neg, doubles
            let m = fx!((mul (d d0) (sub (abs (x b)) (neg (add (d d1) (f f7))))));
            let m_s = F80::from_f64(d0).soft_mul(b.abs().soft_sub(-(F80::from_f64(d1).soft_add(xf(f7)))));
            t.note(3, "abs, neg, f64 leaves", same(m, m_s), why);
            // eight values in the registers at once
            let deep = fx!((add (f f0) (add (f f1) (add (f f2) (add (f f3) (add (f f4) (add (f f5) (add (f f6) (f f7)))))))));
            let deep_s = xf(f0).soft_add(xf(f1).soft_add(xf(f2).soft_add(xf(f3).soft_add(xf(f4).soft_add(xf(f5).soft_add(xf(f6).soft_add(xf(f7))))))));
            t.note(4, "eight registers deep (7 add)", same(deep, deep_s), why);
        }
    }

    #[test]
    fn fused_blocks_equal_the_software_model() {
        let n = cases(400_000) / 4;
        let mut t = Tally::default();
        shapes(0x1234, n, false, &mut t);
        let mut g = Tally::default();
        shapes(0x5678, n / 4, true, &mut g);
        // foreign control words: the block must leave everything to the software model
        let mut c = Tally::default();
        for (k, w) in [0x027fu16, 0x007f, 0x0b7f, 0x077f, 0x0f7f, 0x0372].into_iter().enumerate() {
            under(w, || shapes(0x9abc ^ k as u64, n / 32, false, &mut c));
        }
        for (what, tally) in [("fx", &t), ("fx guard", &g), ("fx control words", &c)] {
            for (name, cases, diff) in &tally.cases {
                eprintln!("{what}\t{name}\t{cases}\t{diff}");
            }
            assert_eq!(tally.different(), 0, "{what}: {:?}", tally.first);
        }
    }

    #[test]
    fn leaves_named_like_the_macros_own_locals() {
        let (v, dst) = (3.0f32, F80::from_f32(0.5));
        let r = fx!((sub (mul (f v) (x dst)) (div (f v) (f v))));
        assert!(same(r, xf(3.0).soft_mul(dst).soft_sub(xf(3.0).soft_div(xf(3.0)))));
        assert_eq!(fx!(f32: (add (f v) (x dst))), 3.5);
    }

    #[test]
    fn wrong_blocks_are_caught() {
        let n = (cases(400_000) / 16).max(10_000);
        let mut r = Rng(0x7777);
        let mut caught = [0u64; 3];
        for _ in 0..n {
            let (f0, f1, f2) = (f32_edge(&mut r), f32_edge(&mut r), f32_edge(&mut r));
            // operands of a subtraction and of a division the wrong way round in the block
            caught[0] += !same(fx!((sub (f f1) (f f0))), xf(f0).soft_sub(xf(f1))) as u64;
            caught[1] += !same(fx!((div (mul (f f2) (f f0)) (f f1))), xf(f1).soft_div(xf(f2).soft_mul(xf(f0)))) as u64;
            // one rounding too many: the inner product stored to float in between
            let once = xf(f0).soft_mul(xf(f1)).soft_add(xf(f2));
            caught[2] += !same(fx!((add (f fx!(f32: (mul (f f0) (f f1)))) (f f2))), once) as u64;
        }
        // the guard's mutants on blocks
        let mut g = Tally::default();
        set_mutant(M_GUARD_CANON);
        shapes(0x4444, n, true, &mut g);
        set_mutant(M_NONE);
        let mut c = Tally::default();
        set_mutant(M_GUARD_CW);
        under(0x027f, || shapes(0x5555, n, false, &mut c));
        under(0x0b7f, || shapes(0x6666, n, false, &mut c));
        set_mutant(M_NONE);
        let all = [("sub reversed", caught[0]), ("div reversed", caught[1]), ("stored to float between", caught[2]),
            ("guard lets non-canonical leaves through", g.different()), ("guard ignores the control word", c.different())];
        for (name, k) in all {
            eprintln!("fx mutant\t{name}\t{n}\t{k}");
        }
        assert!(all.iter().all(|&(_, k)| k > 0), "{all:?}");
    }
}
