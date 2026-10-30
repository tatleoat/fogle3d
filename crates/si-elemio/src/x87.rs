// SPDX-License-Identifier: MIT
//! Arithmetic as the original's x87 code does it, for the few places where the rounding of
//! an intermediate result can change the bits that reach a file or a record.
//!
//! The Linux build keeps intermediate results in the x87's 80-bit registers (64-bit
//! significand, precision control at its default) and rounds when a value is stored. A
//! product of a double constant and a single therefore is rounded twice: to 64 bits in the
//! register, then to 24 bits by the store. Computing in `f64` rounds to 53 bits first, which
//! is not the same thing.

use si_x87::F80;

/// `fldl a; fmuls b; fstps`: the product of a double and a single, rounded to the x87's
/// 64-bit significand and then to a single (both to nearest, ties to even).
pub fn mul_double_single(a: f64, b: f32) -> f32 {
    // the shared register model (`si-x87`); a routine of this crate's own that did the two
    // roundings by hand gave the same bits on three million pairs before it was removed
    (F80::from_f64(a) * F80::from_f32(b)).to_f32()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrees_with_plain_arithmetic_where_that_is_exact() {
        assert_eq!(mul_double_single(2.0, 1.5), 3.0);
        assert_eq!(mul_double_single(-0.5, 3.0), -1.5);
        assert_eq!(mul_double_single(57.295_779_513_082_32, std::f32::consts::PI), 180.0);
        assert_eq!(mul_double_single(0.0, 5.0), 0.0);
        assert!(mul_double_single(f64::NAN, 1.0).is_nan());
        assert_eq!(mul_double_single(1e300, 1e30), f32::INFINITY);
        assert_eq!(mul_double_single(-1e300, 1e30), f32::NEG_INFINITY);
        // a result in the single's subnormal range
        assert_eq!(mul_double_single(1e-30, 1e-10), 1e-40);
        assert_eq!(mul_double_single(0.5, f32::from_bits(1)), 0.0); // half of the smallest: ties to even
        assert_eq!(mul_double_single(0.75, f32::from_bits(1)), f32::from_bits(1));
    }

    #[test]
    fn many_values_against_a_wider_reference() {
        // For products that fit 64 bits exactly the register holds the exact product, so the
        // result is the correctly rounded single: compare with f64 arithmetic on values
        // chosen so that the f64 product is exact too (a constant with few bits).
        let mut state = 12345u32;
        for _ in 0..20_000 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let b = f32::from_bits((state >> 1) % 0x7f00_0000);
            for a in [3.0f64, 0.125, 1.0 + 1.0 / 1024.0, -5.5] {
                let exact = (a * b as f64) as f32;
                assert_eq!(mul_double_single(a, b).to_bits(), exact.to_bits(), "{a} × {b}");
            }
        }
    }

    #[test]
    fn the_register_rounding_shows() {
        // Constructed case: b is the single after 1.0 and a is chosen so that the exact
        // product is 1 + 200001 × 2^-24 + d × 2^-75 with d = 1988641: a little above the
        // midpoint between two singles. The excess is more than half a unit of the 64-bit
        // register (2^-64), so the register keeps the product above the midpoint and the
        // store rounds up. A double (53 bits) loses the excess, lands on the midpoint and the
        // tie goes to the even neighbour below.
        let a = f64::from_bits(0x3ff0_30d3_ef9e_5821);
        let b = f32::from_bits(0x3f80_0001);
        assert_eq!(mul_double_single(a, b).to_bits(), 0x3f81_86a1);
        assert_eq!(((a * b as f64) as f32).to_bits(), 0x3f81_86a0);
    }
}
