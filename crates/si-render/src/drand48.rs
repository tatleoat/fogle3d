// SPDX-License-Identifier: MIT
//! The program's own 48-bit random numbers: the original's `drand48.c` (finding 0012,
//! section 14). Not the C library's generator, whose state `unirand` and the dither use:
//! this one has a state of its own (`seed`, three 16-bit words at 0x086ba000, 0x330e,
//! 0xabcd, 0x1234 in the file) and one user, the 3D textures' noise table.

use si_x87::{x, F80};

/// `seed` (0x086ba000): the generator's 48 bits, least significant word first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seed(pub [u16; 3]);

impl Default for Seed {
    /// The words the file holds before any call.
    fn default() -> Seed {
        Seed([0x330e, 0xabcd, 0x1234])
    }
}

/// Starts the sequence [soft 0x083193b0 SI_srand48]: `SI_srand48(seed)`: word 0 = 0x330e,
/// word 1 = the low half of `seed`, word 2 = its high half. Class: integer.
pub fn srand48(state: &mut Seed, seed: i32) {
    state.0 = [0x330e, seed as u16, (seed >> 16) as u16];
}

/// One step of the generator [soft 0x083193d4 makeseed]: `makeseed(words)`: X ←
/// 0x5DEECE66D × X + 0xB modulo 2⁴⁸, in 16-bit limbs (multipliers 0xe66d, 0xdeec, 5; the
/// carries of the two lower limbs are kept, the third limb wraps). Class: integer.
pub fn makeseed(s: &mut [u16; 3]) {
    let (x0, x1, x2) = (s[0] as u32, s[1] as u32, s[2] as u32);
    let low = x0.wrapping_mul(0xe66d).wrapping_add(0xb);
    let mid = (low >> 16).wrapping_add(x1.wrapping_mul(0xe66d)).wrapping_add(x0.wrapping_mul(0xdeec));
    let high = (mid >> 16).wrapping_add(x2.wrapping_mul(0xe66d)).wrapping_add(x1.wrapping_mul(0xdeec)).wrapping_add(x0.wrapping_mul(5));
    *s = [low as u16, mid as u16, high as u16];
}

/// The next number in [0, 1) [soft 0x0831944c erand48]: `erand48(words)`: one
/// [`makeseed`], then word 0 × 2⁻⁴⁸, word 1 × 2⁻³² and word 2 × 2⁻¹⁶ (each scaled in the
/// register with `fscale` and stored as a double), summed in that order: X / 2⁴⁸ exactly,
/// left in the register. (The binary defines this name itself; the C library's `erand48` is
/// not called.) Class: model (exact).
pub fn erand48(s: &mut [u16; 3]) -> F80 {
    makeseed(s);
    let part = |w: u16, shift: i32| x(x(w as i32).fscale(x(shift)).to_f64());
    (part(s[0], -48) + part(s[1], -32)) + part(s[2], -16)
}

/// `erand48(seed)` [soft 0x083194d0 SI_drand48]. Class: model (exact).
pub fn drand48(state: &mut Seed) -> F80 {
    erand48(&mut state.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recurrence() {
        // srand48(1): X = 0x0000_0001_330e; one step by 48-bit arithmetic
        let mut s = Seed::default();
        srand48(&mut s, 1);
        assert_eq!(s.0, [0x330e, 1, 0]);
        let x0: u64 = 0x1_330e;
        let x1 = (x0.wrapping_mul(0x5_deec_e66d).wrapping_add(0xb)) & 0xffff_ffff_ffff;
        let v = drand48(&mut s);
        assert_eq!(s.0, [x1 as u16, (x1 >> 16) as u16, (x1 >> 32) as u16]);
        assert_eq!(v.to_f64(), x1 as f64 / (1u64 << 48) as f64);
        // a negative seed: the high word is its sign-extended upper half
        srand48(&mut s, -2);
        assert_eq!(s.0, [0x330e, 0xfffe, 0xffff]);
    }
}
