// SPDX-License-Identifier: MIT
//! The part of the original's `materials.c` the sampler and the shading tables rest on:
//! `unirand`, `si_initJitter` and `decayTable` (finding 0012, "Jitter" and "Decay tables").
//!
//! Random numbers are the C library's `drand48`, whose state belongs to the whole process:
//! the dither set-up (`SI_fsDitherInit`) and the particle code call `srand48`, the renderer's
//! set-up does not. So the sequence a render sees depends on what ran before it. The
//! functions here take the generator as a closure; [`Drand48`] is the C library's algorithm
//! (from the library's documentation, not from the `soft` binary).

use crate::shade::DecayTable;
use si_x87::{trig, x, F80};

/// The C library's 48-bit linear congruential generator behind `drand48` and `srand48`:
/// X ← (0x5DEECE66D × X + 0xB) mod 2⁴⁸, value X / 2⁴⁸; `srand48(s)` sets X = s × 2¹⁶ +
/// 0x330E. Not read from the `soft` binary: this is the documented behaviour of the library
/// it imports the two functions from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drand48(pub u64);

impl Drand48 {
    pub fn seeded(seed: u32) -> Drand48 {
        Drand48(((seed as u64) << 16) | 0x330e)
    }

    pub fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(0x5_deec_e66d).wrapping_add(0xb) & 0xffff_ffff_ffff;
        self.0 as f64 / (1u64 << 48) as f64
    }
}

/// `drand48()` rounded to float [soft 0x0825969c unirand]. The float nearest a value just
/// below 1 is 1.0, so the range is 0..=1. Class: copy.
pub fn unirand(drand48: &mut dyn FnMut() -> f64) -> f32 {
    x(drand48()).to_f32()
}

/// `SI_Jitter`: +0x00 a float amplitude, +0x04 the table, +0x08 a short the set-up clears
/// (by its use elsewhere: the next entry to hand out).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Jitter {
    /// +0x00
    pub amplitude: f32,
    /// +0x04: `count` rows of (x, y) pairs.
    pub table: Vec<Vec<[f32; 2]>>,
    /// +0x08
    pub next: i16,
}

/// The warning `si_initJitter` gives (through `SI_warning`, severity 2) when the sampling
/// level is below 4; it then carries on.
pub fn msg_jitter() -> &'static str {
    si_rt::text(0x0869_be80)
}

/// Fills a jitter record's table with `count` rows [soft 0x082596b4 si_initJitter]:
/// `si_initJitter(count, jitter)`. `level` is the short at 0x08790d38 (the sampling level of
/// the render set-up).
///
/// - Level 1: each row is one pair: (u₁ × amplitude, (u₂ + u₂) − 1).
/// - Otherwise each row has m = (2 × level − 1)² pairs, points scattered round a circle:
///   with step = float(2π / (m − 1)) and an angle that starts at 0, pair j is
///   (sin a × r, cos a × r) with r = u₁ × amplitude and a = u₂ × step + angle (a rounded to
///   float for each of the two library calls, r kept in the register), after which the angle
///   grows by step. The middle pair, j = m / 2, is (0, 0), draws no numbers and does not
///   advance the angle.
///
/// u₁, u₂ are successive [`unirand`] values. Returns the warning when `level` is below 4.
/// Class: libm (`sinf`, `cosf`); level 1 is model.
pub fn init_jitter(count: i32, jitter: &mut Jitter, level: i16, drand48: &mut dyn FnMut() -> f64) -> Option<&'static str> {
    let warning = if level < 4 { Some(msg_jitter()) } else { None };
    let amplitude = jitter.amplitude;
    let mut table = Vec::new();
    if level == 1 {
        for _ in 0..count {
            let a = (x(unirand(drand48)) * amplitude).to_f32();
            let u = unirand(drand48);
            table.push(vec![[a, ((x(u) + u) - 1.0f32).to_f32()]]);
        }
    } else {
        let side = level as i32 * 2 - 1;
        let m = side * side;
        let step = (x(core::f64::consts::TAU) / x(m - 1)).to_f32();
        for _ in 0..count {
            let mut row = Vec::new();
            let mut angle = 0.0f32;
            for j in 0..m {
                if j == m / 2 {
                    row.push([0.0, 0.0]);
                    continue;
                }
                let r = x(unirand(drand48)) * amplitude;
                let a = (x(unirand(drand48)) * step + angle).to_f32();
                row.push([(trig::sinf(a) * r).to_f32(), (trig::cosf(a) * r).to_f32()]);
                angle += step;
            }
            table.push(row);
        }
    }
    jitter.table = table;
    jitter.next = 0;
    warning
}

/// Fills a decay table from its exponent [soft 0x082595e0 decayTable]: value[i] =
/// pow(b, exponent) for i = 199 down to 0 with b starting at the double 0.995 (0x086541c8)
/// and going down by 0.005 (0x086541b8) each time, rounded to double at each step;
/// step[i] = value[i + 1] − value[i], with 1 standing in for value[200]. A call of `pow`
/// that sets `errno` to a domain or range error counts as 0. The loop stops after storing
/// the first value below 1e-6 (0x086541c0); the entries below stay (0, 0).
///
/// `pow` is the C library's; it comes in as a closure returning the value and whether
/// `errno` was set. [`pow_host`] is this machine's. Class: libm.
pub fn decay_table(table: &mut DecayTable, pow: &dyn Fn(f64, f64) -> (F80, bool)) {
    table.pairs = vec![[0.0; 2]; 200];
    let mut base = f64::from_bits(0x3fef_d70a_3d70_a3d7);
    let exponent = table.exponent as f64;
    let mut above = F80::ONE;
    for i in (0..200).rev() {
        let (mut v, failed) = pow(base, exponent);
        if failed {
            v = x(0.0f32);
        }
        table.pairs[i] = [v.to_f32(), (above - v).to_f32()];
        if v.lt(x(1e-6f64)) {
            break;
        }
        base = (x(base) - 0.005f64).to_f64();
        above = v;
    }
}

/// `pow` by the host's library, and the C library's two error cases as far as they can
/// arise here: a result that is not finite, or zero from a non-zero base.
pub fn pow_host(b: f64, e: f64) -> (F80, bool) {
    let v = b.powf(e);
    (x(v), !v.is_finite() || (v == 0.0 && b != 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drand48_is_the_library_sequence() {
        // the first value after srand48(0), as the C library documents the recurrence:
        // X = 0x330E → (0x5DEECE66D × 0x330E + 0xB) mod 2^48 = 0x2BBB62DC5101
        let mut g = Drand48::seeded(0);
        let v = g.next();
        assert_eq!(g.0, 0x2bbb_62dc_5101);
        assert!((v - 0.170828036106).abs() < 1e-11, "{v}");
    }

    #[test]
    fn jitter_level_one() {
        si_rt::skip_without_install!();
        let mut values = [0.25f64, 0.75, 0.5, 0.0].into_iter();
        let mut gen = move || values.next().unwrap();
        let mut j = Jitter { amplitude: 2.0, next: 5, ..Jitter::default() };
        assert_eq!(init_jitter(2, &mut j, 1, &mut gen), Some(msg_jitter()));
        assert_eq!(j.table, vec![vec![[0.5, 0.5]], vec![[1.0, -1.0]]]);
        assert_eq!(j.next, 0);
    }

    #[test]
    fn jitter_ring() {
        si_rt::skip_without_install!();
        // level 2: m = 9 pairs, step = 2π / 8; all numbers 1: r = amplitude, a = step + angle
        let mut gen = || 1.0f64;
        let mut j = Jitter { amplitude: 1.0, ..Jitter::default() };
        assert_eq!(init_jitter(1, &mut j, 2, &mut gen), Some(msg_jitter()));
        let row = &j.table[0];
        assert_eq!(row.len(), 9);
        assert_eq!(row[4], [0.0, 0.0]);
        // pair 0: angle π/4; pair 5 (the fifth drawn, the middle skipped): angle 5π/4
        assert!((row[0][0] - 0.70710677).abs() < 1e-6 && (row[0][1] - 0.70710677).abs() < 1e-6);
        assert!((row[5][0] + 0.70710677).abs() < 1e-6 && (row[5][1] + 0.70710677).abs() < 1e-6);
        assert_eq!(init_jitter(1, &mut j, 4, &mut gen), None);
        assert_eq!(j.table[0].len(), 49);
    }

    #[test]
    fn decay_table_shape() {
        let mut t = DecayTable { exponent: 1.0, ..DecayTable::default() };
        decay_table(&mut t, &pow_host);
        // exponent 1: value[i] is about i / 200 and every step about 0.005
        assert_eq!(t.pairs[199][0], 0.995);
        assert!((t.pairs[199][1] - 0.005).abs() < 1e-7);
        assert!((t.pairs[100][0] - 0.5).abs() < 1e-6);
        // the base reaches (about) zero at i = 0: stored, and it ends the loop
        assert!(t.pairs[0][0].abs() < 1e-6);
        // a large exponent: the loop stops at the first value below 1e-6, the rest stay zero
        let mut t = DecayTable { exponent: 300.0, ..DecayTable::default() };
        decay_table(&mut t, &pow_host);
        let first = t.pairs.iter().position(|p| p[0] != 0.0).unwrap();
        assert!(t.pairs[first][0] < 1e-6 && t.pairs[first + 1][0] >= 1e-6);
        assert_eq!(t.pairs[first - 1], [0.0, 0.0]);
        // an error from pow counts as zero, and zero ends the loop at once
        let mut t = DecayTable { exponent: 2.0, ..DecayTable::default() };
        decay_table(&mut t, &|_, _| (x(5.0f32), true));
        assert_eq!(t.pairs[199], [0.0, 1.0]);
        assert_eq!(t.pairs[198], [0.0, 0.0]);
    }
}
