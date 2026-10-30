// SPDX-License-Identifier: MIT
//! The C library's `rand` and `srand`, which the random functions of the expression
//! language call (finding 0013).
//!
//! **Not read from the binary**: `rand` and `srand` are dynamic imports. This is the default
//! generator of the GNU C library (its `random_r` with the 31-word state, "TYPE_3"), as its
//! sources have had it since before this program was built: an additive feedback generator
//! `r[i] = r[i-3] + r[i-31]` whose output is the sum shifted right by one, seeded by a
//! multiplicative congruential sequence and run 310 steps before the first output. The
//! state is one per process: every caller in the program shares it.

/// The generator's state.
#[derive(Clone, Debug)]
pub struct CRand {
    r: [i32; 31],
    f: usize,
    b: usize,
}

impl Default for CRand {
    /// The state of a process that has not called `srand`: as after `srand(1)`.
    fn default() -> Self {
        CRand::seeded(1)
    }
}

impl CRand {
    pub fn seeded(seed: u32) -> CRand {
        let mut g = CRand { r: [0; 31], f: 3, b: 0 };
        g.srand(seed);
        g
    }

    /// `srand(seed)`; a seed of 0 is taken as 1.
    pub fn srand(&mut self, seed: u32) {
        let seed = if seed == 0 { 1 } else { seed };
        self.r[0] = seed as i32;
        for i in 1..31 {
            // 16807 * r[i-1] mod 2147483647, by Schrage's method on signed words
            let prev = self.r[i - 1];
            let hi = prev / 127773;
            let lo = prev % 127773;
            let mut word = 16807i32.wrapping_mul(lo).wrapping_sub(2836i32.wrapping_mul(hi));
            if word < 0 {
                word = word.wrapping_add(2147483647);
            }
            self.r[i] = word;
        }
        self.f = 3;
        self.b = 0;
        for _ in 0..310 {
            self.rand();
        }
    }

    /// `rand()`: 0 ..= 2147483647.
    pub fn rand(&mut self) -> i32 {
        self.r[self.f] = self.r[self.f].wrapping_add(self.r[self.b]);
        let out = ((self.r[self.f] as u32) >> 1) as i32;
        self.f = (self.f + 1) % 31;
        self.b = (self.b + 1) % 31;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_sequence_for_seed_one() {
        // the first outputs of an unseeded C program linked with the GNU C library
        let mut g = CRand::default();
        assert_eq!(g.rand(), 1804289383);
        assert_eq!(g.rand(), 846930886);
        assert_eq!(g.rand(), 1681692777);
        assert_eq!(g.rand(), 1714636915);
    }

    #[test]
    fn seed_zero_is_seed_one() {
        assert_eq!(CRand::seeded(0).rand(), CRand::seeded(1).rand());
    }
}
