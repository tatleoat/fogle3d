// SPDX-License-Identifier: MIT
//! nurbs15 (finding 0025 section 33): the two squared-distance helpers that answer in st0.

use si_x87::F80;

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

/// [soft 0x0850d31c si_nurbsInterDist2D]: `(a, b, c, d)` by value: `(a - c)^2 + (b - d)^2`, each difference and square and the sum
/// in 80-bit registers, answered in st0.
pub fn inter_dist_2d(a: f64, b: f64, c: f64, d: f64) -> F80 {
    let (da, db) = (x(a) - x(c), x(b) - x(d));
    da * da + db * db
}

/// [soft 0x0850dcd8 si_nurbsRayDist3D]: two points `(x, y, z)` by value (six doubles): `((x - x')^2 + (y - y')^2) + (z - z')^2`
/// in 80-bit registers, in that order of addition (x and y first, z added last), answered in st0.
pub fn ray_dist_3d(a: [f64; 3], b: [f64; 3]) -> F80 {
    let (dx, dy, dz) = (x(a[0]) - x(b[0]), x(a[1]) - x(b[1]), x(a[2]) - x(b[2]));
    dx * dx + dy * dy + dz * dz
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_values() {
        assert_eq!(inter_dist_2d(3.0, 4.0, 0.0, 0.0).to_f64(), 25.0);
        assert_eq!(ray_dist_3d([1.0, 2.0, 3.0], [0.0, 0.0, 0.0]).to_f64(), 14.0);
    }
}
