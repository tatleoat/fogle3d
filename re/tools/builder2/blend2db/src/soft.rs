// SPDX-License-Identifier: MIT
//! Soft shadows of Blender lights with a size (unit soft1; our own code, nothing here reproduces a function of the original).
//!
//! The Softimage renderer's shadows are hard: ray traced (SHADOW_TYPE 2), or a depth map (3), or a depth map filtered
//! over a window of map cells (4, "soft"). None of them has a light with a size: the light's jitter table (+0x130) is
//! filled by `si_lights` but nothing in the shading reads it (read in crates/si-render lights.rs and shade.rs). Cycles'
//! area lights, point and spot lights with a radius and suns with an angle give a penumbra whose width grows with the
//! distance between the shadow caster and the surface it falls on. The filtered depth map does not: as ported
//! (shade.rs, shadow type 4) it filters only where the centre cell is in shadow, over a fixed window of map cells, so
//! it softens a band inside the hard edge whatever the distances; its map is a perspective view over the cone plus
//! spread, which a cosine-lobe area light (spread pi) does not have; unit blender3 measured it no better than ray-traced
//! shadows. So a light with a size is written as a set of n lights spread over its shape ("light array", the way users
//! of the time made area lights), each with 1/n of the fitted colour and ray-traced shadows: where part of the shape is
//! hidden, that part of the lights is, and the penumbra follows the geometry. The points are a Hammersley set (n
//! distinct positions along both axes, so a straight shadow edge gets n small steps, not sqrt(n) large ones), mapped
//! area-uniformly onto discs. Deterministic: the same scene gives the same database.
//!
//! Speed (unit softspeed1): n lights cost n shadow rays a shaded point, and the original's ray walk tests every
//! triangle listed in every BSP cell a ray crosses (80% of the still life's render time with a 49-light array).
//! build.rs lists the arrays' lights in `soft-arrays.txt` beside the database (a file the original never writes or
//! reads); render-frames answers their shadow rays first with a bounding volume hierarchy in doubles and keeps the
//! answer only where it is certainly the walk's (crates/si-render/src/softarray.rs). Rays from nearby points to the
//! lights of one array share a shaft (unit softspeed3): the triangles that can cut any ray from a small box of points
//! to the array's lights are listed once, so most rays test a few triangles instead of descending the hierarchy. Unit
//! soft4: each triangle is tested as the walk tests it (in doubles, with error bounds), rays through glass get the
//! walk's factor directly, and a single light with ray-traced shadows is listed too (an array of one). The picture is
//! the same (byte-identical to the walk's; the check mode found 0 disagreements in 14.07 million rays at 480x320);
//! the still life at 1920x1280 took 24.3 s wall, 369 CPU s at 16 threads before unit soft4, 16.3 s and 241 CPU s
//! after (14.9 s at 21 threads). A database without the file, every Softimage scene's, is rendered as before.
//!
//! What stays different from Cycles (re/tools/soft1, calib): the renderer adds lights in display values, so a light
//! half hidden leaves half the display value where Cycles' leaves about 73% (linear half, sRGB-encoded): penumbrae are
//! darker in the middle and the umbra measures larger; n lights give n steps across a penumbra (2 per degree keeps them
//! faint); each light of the array has the fitted cosine lobe of the whole light.

/// Below this angular diameter (radians) seen from the scene a light stays one light: Blender's default sun angle
/// (0.0092 rad) and a radius of 0 stay hard, as before.
pub const MIN_ANGLE: f64 = 0.0175; // 1 degree
/// Lights per degree of the angular diameter (at least 4, at most the setting: the add-on's default is 24, see
/// [`DEFAULT_MAX`]). Each light of the array is a step of 1/n in the penumbra: with 16 lights for the gallery's
/// 24-degree area light the steps showed as rings around the shadows (re/out/soft1/still); 2 per degree would give 49
/// there, the default cap 24.
pub const PER_DEGREE: f64 = 2.0;

/// The add-on's default for Soft shadow lights (and build.rs' when the scene does not say): 24 lights a light (unit
/// softspeed3). Against 49 lights on the gallery's still life, 24 leave 1.5% of the pixels more than 2.5/255 off (16:
/// 3.2%, 32: 1.4%) at half the shadow rays; the picture against Cycles is as close (rmse 0.106 for 16, 24, 32 and 49).
pub const DEFAULT_MAX: usize = 24;

fn radical_inverse2(mut i: u32) -> f64 {
    i = i.reverse_bits();
    i as f64 / 4_294_967_296.0
}

/// n points of the square [-0.5, 0.5]^2 (Hammersley, cell-centred).
pub fn square(n: usize) -> Vec<[f64; 2]> {
    (0..n).map(|i| [(i as f64 + 0.5) / n as f64 - 0.5, radical_inverse2(i as u32) + 0.5 / n as f64 - 0.5]).collect()
}

/// n points of the unit disc (radius 1), spread evenly over its area.
pub fn disc(n: usize) -> Vec<[f64; 2]> {
    square(n)
        .into_iter()
        .map(|[u, v]| {
            let r = (u + 0.5).sqrt();
            let a = 2.0 * std::f64::consts::PI * (v + 0.5);
            [r * a.cos(), r * a.sin()]
        })
        .collect()
}

/// How many lights stand in for a light whose angular diameter seen from the scene is `alpha` (radians); `max` is the
/// add-on's setting (Render Properties > Fogle3D > Soft shadow lights); 0 or 1 turns the arrays off.
pub fn count(alpha: f64, max: usize) -> usize {
    if max <= 1 || !(alpha >= MIN_ANGLE) {
        return 1;
    }
    let k = (alpha.to_degrees() * PER_DEGREE).round().min(1e6) as usize;
    k.clamp(4.min(max), max)
}

/// Two unit vectors at right angles to the unit vector `a` and to each other.
pub fn basis(a: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let t = if a[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let c = |p: [f64; 3], q: [f64; 3]| [p[1] * q[2] - p[2] * q[1], p[2] * q[0] - p[0] * q[2], p[0] * q[1] - p[1] * q[0]];
    let e1 = c(a, t);
    let l = (e1[0] * e1[0] + e1[1] * e1[1] + e1[2] * e1[2]).sqrt();
    let e1 = [e1[0] / l, e1[1] / l, e1[2] / l];
    (e1, c(a, e1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_points_have_distinct_rows_and_columns() {
        let p = square(16);
        let mut xs: Vec<i64> = p.iter().map(|q| (q[0] * 1e6).round() as i64).collect();
        let mut ys: Vec<i64> = p.iter().map(|q| (q[1] * 1e6).round() as i64).collect();
        xs.sort();
        xs.dedup();
        ys.sort();
        ys.dedup();
        assert_eq!((xs.len(), ys.len()), (16, 16));
        assert!(p.iter().all(|q| q[0].abs() < 0.5 && q[1].abs() < 0.5));
        let mean = p.iter().fold([0.0, 0.0], |m, q| [m[0] + q[0] / 16.0, m[1] + q[1] / 16.0]);
        assert!(mean[0].abs() < 1e-9 && mean[1].abs() < 0.04);
    }

    #[test]
    fn disc_points_inside() {
        assert!(disc(25).iter().all(|q| q[0] * q[0] + q[1] * q[1] <= 1.0));
    }

    #[test]
    fn counts() {
        assert_eq!(count(0.0092, 64), 1);
        assert_eq!(count(0.5, 16), 16);
        assert_eq!(count(0.5, 64), 57);
        assert_eq!(count(0.5, 1), 1);
        assert_eq!(count(0.5, 0), 1);
        assert_eq!(count(0.02, 64), 4);
        assert_eq!(count(0.02, 2), 2);
        assert_eq!(count(0.07, 64), 8);
        assert_eq!(count(3.0, 64), 64);
        assert_eq!(count(f64::NAN, 64), 1);
    }

    #[test]
    fn basis_is_orthonormal() {
        for a in [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.6, 0.0, -0.8]] {
            let (e1, e2) = basis(a);
            let d = |p: [f64; 3], q: [f64; 3]| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
            assert!(d(a, e1).abs() < 1e-12 && d(a, e2).abs() < 1e-12 && d(e1, e2).abs() < 1e-12);
            assert!((d(e1, e1) - 1.0).abs() < 1e-12 && (d(e2, e2) - 1.0).abs() < 1e-12);
        }
    }
}
