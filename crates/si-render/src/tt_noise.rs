// SPDX-License-Identifier: MIT
//! The 3D textures' noise: the original's `tt_noise.c` (finding 0012, section 14). A table
//! of 65,536 random floats, and a look-up that blends 27 of them with quadratic B-spline
//! weights.

use crate::drand48::{drand48, srand48, Seed};
use si_x87::{x, Round, F80};

/// 0x08657c70 (double): added to each coordinate so that it is positive before it is cut
/// to a whole number.
const SHIFT: f64 = 39999.5;
/// 0x08657c78 (double).
const HALF: f64 = 0.5;
/// The multipliers of the three whole coordinates in the table index.
const KX: u32 = 0x53d;
const KY: u32 = 0x2cf;
const KZ: u32 = 0x7ef;

/// `table` (0x087bafc0, 65,536 floats), `first_call` (0x086b9a80, 1 in the file) and the
/// generator's `seed` (0x086ba000).
#[derive(Clone, Debug, PartialEq)]
pub struct Noise {
    pub table: Vec<f32>,
    pub first_call: i16,
    pub seed: Seed,
}

impl Default for Noise {
    fn default() -> Noise {
        Noise { table: Vec::new(), first_call: 1, seed: Seed::default() }
    }
}

/// Fills the table on the first call [soft 0x08317270 SI_ttnInitNoise]: `SI_srand48(1)`,
/// then 65,536 values of `SI_drand48()`, each rounded to float; `first_call` cleared. Later
/// calls do nothing: every 3D texture of a session shares one table, and its values do not
/// depend on anything the user did. Class: model (exact).
pub fn init_noise(n: &mut Noise) {
    if n.first_call == 0 {
        return;
    }
    srand48(&mut n.seed, 1);
    n.table = (0..0x10000).map(|_| drand48(&mut n.seed).to_f32()).collect();
    n.first_call = 0;
}

/// The noise at a point [soft 0x083172bc SI_ttnGetNoise]: `SI_ttnGetNoise(x, y, z)`,
/// returned in the register.
///
/// Per coordinate: c = float(coordinate + 39999.5); i = trunc(c); f = c − i; the three
/// weights of a quadratic B-spline, each rounded to float: w2 = (f × 0.5) × f, w0 = (0.5 −
/// f) + w2, w1 = (1 − w0) − w2. The table index of the cell (a, b, c) of the 3 × 3 × 3 block
/// is the low 16 bits of ix × 0x53d + iy × 0x2cf + iz × 0x7ef + a × 0x53d + b × 0x2cf + c ×
/// 0x7ef. Each row along x is summed in the register in the order the listing has (below),
/// times its y weight; the three rows of a layer are added and the layer multiplied by its z
/// weight; the layers are added as (middle + first) + last. Nothing but the nine weights is
/// rounded on the way.
///
/// The table must have been filled ([`init_noise`]); the original reads it as it is
/// (zeros before the first 3D texture is made). Class: model.
pub fn get_noise(n: &Noise, px: f32, py: f32, pz: f32) -> F80 {
    let cx = (x(SHIFT) + px).to_f32();
    let ix = x(cx).to_i32(Round::Zero);
    let cy = (x(py) + SHIFT).to_f32();
    let cz = (x(pz) + SHIFT).to_f32();
    let iy = x(cy).to_i32(Round::Zero);
    let iz = x(cz).to_i32(Round::Zero);
    let weights = |f: F80| {
        let w2 = ((f * HALF) * f).to_f32();
        let w0 = ((x(HALF) - f) + w2).to_f32();
        let w1 = ((F80::ONE - w0) - w2).to_f32();
        (w0, w1, w2)
    };
    let (bx, cxw, ax) = weights(x(cx) - x(ix));
    let (by, cyw, ay) = weights(x(cy) - x(iy));
    let (bz, czw, az) = weights(x((x(cz) - x(iz)).to_f32()));
    let h = (ix as u32).wrapping_mul(KX).wrapping_add((iy as u32).wrapping_mul(KY)).wrapping_add((iz as u32).wrapping_mul(KZ));
    let t = |a: u32, b: u32, c: u32| {
        let i = (h.wrapping_add(a * KX).wrapping_add(b * KY).wrapping_add(c * KZ) & 0xffff) as usize;
        x(n.table.get(i).copied().unwrap_or(0.0))
    };
    let (b, c, a) = (x(bx), x(cxw), x(ax));
    // the three row forms of the listing: which product is added to which first
    let row_far = |j: u32, k: u32| ((b * t(0, j, k)) + (c * t(1, j, k))) + (a * t(2, j, k));
    let row_mid = |j: u32, k: u32| ((c * t(1, j, k)) + (b * t(0, j, k))) + (a * t(2, j, k));
    let row_near = |j: u32, k: u32| (a * t(2, j, k)) + ((b * t(0, j, k)) + (c * t(1, j, k)));
    let layer = |k: u32| (row_far(2, k) * ay) + ((row_mid(1, k) * cyw) + (row_near(0, k) * by));
    let middle = layer(1) * czw;
    let last = {
        let t4 = row_mid(1, 2) * cyw;
        let near = ((b * t(0, 0, 2)) + (c * t(1, 0, 2))) + (a * t(2, 0, 2));
        let far = (a * t(2, 2, 2)) + ((c * t(1, 2, 2)) + (b * t(0, 2, 2)));
        ((t4 + (near * by)) + (far * ay)) * az
    };
    let first = layer(0) * bz;
    (middle + first) + last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_constant_table_gives_the_constant() {
        // the weights of each direction sum to 1 (up to their rounding)
        let n = Noise { table: vec![0.25; 0x10000], first_call: 0, seed: Seed::default() };
        for p in [[0.0f32, 0.0, 0.0], [1.25, -3.5, 7.75], [100.1, 200.2, -300.3]] {
            let v = get_noise(&n, p[0], p[1], p[2]).to_f64();
            assert!((v - 0.25).abs() < 1e-6, "{v}");
        }
    }

    #[test]
    fn the_table_is_filled_once() {
        let mut n = Noise::default();
        init_noise(&mut n);
        assert_eq!(n.table.len(), 0x10000);
        assert!(n.table.iter().all(|v| (0.0..=1.0).contains(v)));
        let first = n.table[0];
        n.table[0] = 9.0;
        init_noise(&mut n);
        assert_eq!(n.table[0], 9.0);
        // the first value: one step from srand48(1)
        let x1 = (0x1_330eu64.wrapping_mul(0x5_deec_e66d).wrapping_add(0xb)) & 0xffff_ffff_ffff;
        assert_eq!(first, (x1 as f64 / (1u64 << 48) as f64) as f32);
    }
}
