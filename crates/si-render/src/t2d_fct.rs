// SPDX-License-Identifier: MIT
//! The two routines of the original's `t2d_fct.c` the renderer needs (finding 0012,
//! section 13): the projection of a point into a 2D map's coordinates, and the words that
//! say what a map drives.

use crate::shade::ShadeMap;
use crate::types::Vector;
use si_x87::{trig, x, F80};

/// 0x08655e88, 0x08655e90 (double): a factor beyond these drives its channel.
const SOME: f64 = 1e-4;
/// 0x08655ea0 (double): the least normal float; 0x08655ec8 (double): the float 1e-20. Below
/// these in both x and z the azimuth is taken as 0 (cylinder; sphere).
const TINY_CYLINDER: f64 = 1.1754943508222875e-38;
const TINY_SPHERE: f64 = 9.999999682655225e-21;
/// 0x08655ea8, 0x08655eb0, 0x08655eb8, 0x08655ec0 (double).
const PI: f64 = core::f64::consts::PI;
const HALF: f64 = 0.5;
const SEAM_LOW: f64 = 0.25;
const SEAM_HIGH: f64 = 0.75;

/// What a 2D map drives [soft 0x082b289c SI_updTxt2dEffects]: `SI_updTxt2dEffects(map)`.
/// +0x112 ambient: the ambient factor (+0x128) is above 0.0001; +0x114 diffuse and +0x116
/// specular likewise (+0x12c, +0x130); +0x11a transparency: the factor (+0x138) is below
/// −0.0001 or above 0.0001, or the "black and white" key (+0x18e) is set; +0x118
/// reflectivity (+0x134) and +0x120 roughness (+0x140): beyond ±0.0001; +0x124: the method
/// (+0x2a) is 7 or 8 (a reflection map). Class: copy.
pub fn upd_txt2d_effects(m: &mut ShadeMap) {
    let above = |v: f32| i16::from(x(v).gt(x(SOME)));
    let beyond = |v: f32| x(v).lt(x(-SOME)) || x(v).gt(x(SOME));
    m.fx_ambient = above(m.factors[0]);
    m.fx_diffuse = above(m.factors[1]);
    m.fx_specular = above(m.factors[2]);
    m.fx_transp = i16::from(beyond(m.factors[4]) || m.black_white != 0);
    m.fx_reflect = i16::from(beyond(m.factors[3]));
    m.bump = i16::from(beyond(m.factors[6]));
    m.skip = i16::from(m.method == 7 || m.method == 8);
}

/// The texture coordinates of a point for a projected 2D map
/// [soft 0x082b3090 SI_mapTcoordCompute]: `SI_mapTcoordCompute(map, point, &pair, &bound,
/// &size, &centre)`; `bound` is x min, x max, y min, y max, z min, z max. Returns the seam
/// bits: 1 for a u at or below 0.25, 2 for one at or above 0.75 (methods 5 and 6 only).
///
/// The point first goes through the first three rows and columns of the map's matrix
/// (+0xb8; each coordinate a sum of three products in the register, rounded to float):
/// P = M · point. Then by method (+0x2a):
///
/// - **1** (XY): u = (P.x − x min) / size.x; v = 1 − (P.y − y min) / size.y.
/// - **2** (XZ): u as 1; v = 1 − (P.z − z min) / size.z.
/// - **3** (YZ): u = (P.z − z min) / size.z; v as 1.
///   A size that is exactly 0 gives 0 for its coordinate.
/// - **5** (cylinder): d = P − centre (each component rounded to float), divided by its
///   length unless that is 0; u = ((atan2(d.z, −d.x) + π) / π) × 0.5, the arc tangent rounded
///   to float first; 0 when both |d.z| and |d.x| are below the least normal float; v as 1.
/// - **6** (sphere): the same u (threshold the float 1e-20); v = acos(d.y) / π.
/// - any other: nothing is written and 0 returned.
///
/// Class: model for 1 to 3; libm for 5 and 6 (`fpatan`, `acos`).
pub fn map_tcoord_compute(m: &ShadeMap, p: Vector, pair: &mut [f32; 2], bound: &[f32; 6], size: &Vector, centre: &Vector) -> u16 {
    let t = &m.tmat;
    let px = (((x(t[0][1]) * p.y) + (x(t[0][0]) * p.x)) + (x(t[0][2]) * p.z)).to_f32();
    let py = (((x(t[1][0]) * p.x) + (x(t[1][1]) * p.y)) + (x(t[1][2]) * p.z)).to_f32();
    let pz = (((x(p.y) * t[2][1]) + (x(p.x) * t[2][0])) + (x(p.z) * t[2][2])).to_f32();
    let zero = x(0.0f32);
    let along = |c: f32, least: f32, extent: f32| if x(extent).eq(zero) { 0.0f32 } else { ((x(c) - least) / x(extent)).to_f32() };
    let down = |c: f32, least: f32, extent: f32| if x(extent).eq(zero) { 0.0f32 } else { (F80::ONE - ((x(c) - least) / x(extent))).to_f32() };
    let seam_of = |u: f32| {
        let mut bits = 0u16;
        if x(u).le(x(SEAM_LOW)) {
            bits |= 1;
        }
        if x(u).ge(x(SEAM_HIGH)) {
            bits |= 2;
        }
        bits
    };
    // the direction from the centre and the azimuth of methods 5 and 6
    let round = |tiny: f64| {
        let (dx, dy, dz) = (x(px) - centre.x, x(py) - centre.y, x(pz) - centre.z);
        let len = (((dy * dy) + (dx * dx)) + (dz * dz)).sqrt();
        let d = if zero.eq(len) { [dx.to_f32(), dy.to_f32(), dz.to_f32()] } else { [(dx / len).to_f32(), (dy / len).to_f32(), (dz / len).to_f32()] };
        let u = if x(tiny).le(x(d[2]).abs()) || x(tiny).unordered(x(d[2]).abs()) || !x(d[0]).abs().lt(x(tiny)) {
            let a = trig::fpatan(x(d[2]), -x(d[0])).to_f32();
            (((x(a) + PI) / PI) * HALF).to_f32()
        } else {
            0.0
        };
        (d, u)
    };
    match m.method {
        1 => {
            pair[0] = along(px, bound[0], size.x);
            pair[1] = down(py, bound[2], size.y);
            0
        }
        2 => {
            pair[0] = along(px, bound[0], size.x);
            pair[1] = down(pz, bound[4], size.z);
            0
        }
        3 => {
            pair[0] = along(pz, bound[4], size.z);
            pair[1] = down(py, bound[2], size.y);
            0
        }
        5 => {
            let (_, u) = round(TINY_CYLINDER);
            pair[0] = u;
            let bits = seam_of(u);
            pair[1] = down(py, bound[2], size.y);
            bits
        }
        6 => {
            let (d, u) = round(TINY_SPHERE);
            pair[0] = u;
            pair[1] = (trig::acos(d[1] as f64) / PI).to_f32();
            seam_of(u)
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ShadeMap {
        ShadeMap { tmat: si_math::types::IDENTITY, ..ShadeMap::default() }
    }

    #[test]
    fn planes() {
        let bound = [0.0, 2.0, 0.0, 4.0, 0.0, 8.0];
        let (size, centre) = (Vector::new(2.0, 4.0, 8.0), Vector::new(1.0, 2.0, 4.0));
        let p = Vector::new(0.5, 1.0, 2.0);
        let mut pair = [9.0; 2];
        let mut m = identity();
        m.method = 1;
        assert_eq!(map_tcoord_compute(&m, p, &mut pair, &bound, &size, &centre), 0);
        assert_eq!(pair, [0.25, 0.75]);
        m.method = 2;
        map_tcoord_compute(&m, p, &mut pair, &bound, &size, &centre);
        assert_eq!(pair, [0.25, 0.75]);
        m.method = 3;
        map_tcoord_compute(&m, p, &mut pair, &bound, &size, &centre);
        assert_eq!(pair, [0.25, 0.75]);
        // a size of 0 gives 0, and method 4 writes nothing
        m.method = 1;
        map_tcoord_compute(&m, p, &mut pair, &bound, &Vector::new(0.0, 0.0, 1.0), &centre);
        assert_eq!(pair, [0.0, 0.0]);
        m.method = 4;
        pair = [7.0; 2];
        assert_eq!(map_tcoord_compute(&m, p, &mut pair, &bound, &size, &centre), 0);
        assert_eq!(pair, [7.0; 2]);
    }

    #[test]
    fn the_seam_of_a_cylinder() {
        let bound = [-1.0, 1.0, 0.0, 1.0, -1.0, 1.0];
        let (size, centre) = (Vector::new(2.0, 1.0, 2.0), Vector::new(0.0, 0.0, 0.0));
        let mut m = identity();
        m.method = 5;
        let mut pair = [0.0; 2];
        // towards −x: atan2(0, 1) = 0: u = 0.5, no seam bit
        assert_eq!(map_tcoord_compute(&m, Vector::new(-1.0, 0.5, 0.0), &mut pair, &bound, &size, &centre), 0);
        assert_eq!(pair, [0.5, 0.5]);
        // towards +x, a little to −z: just above −π: u near 0, bit 1
        assert_eq!(map_tcoord_compute(&m, Vector::new(1.0, 0.5, -0.01), &mut pair, &bound, &size, &centre), 1);
        assert!(pair[0] < 0.01);
        // a little to +z: just below π: u near 1, bit 2
        assert_eq!(map_tcoord_compute(&m, Vector::new(1.0, 0.5, 0.01), &mut pair, &bound, &size, &centre), 2);
        assert!(pair[0] > 0.99);
        // on the axis: u = 0
        assert_eq!(map_tcoord_compute(&m, Vector::new(0.0, 0.5, 0.0), &mut pair, &bound, &size, &centre), 1);
        assert_eq!(pair[0], 0.0);
    }

    #[test]
    fn effects() {
        let mut m = ShadeMap { factors: [0.75, 1.0, 0.0, -0.5, 0.0, 9.0, 0.00005, 1.0], method: 4, ..ShadeMap::default() };
        upd_txt2d_effects(&mut m);
        assert_eq!((m.fx_ambient, m.fx_diffuse, m.fx_specular, m.fx_reflect, m.fx_transp, m.bump, m.skip), (1, 1, 0, 1, 0, 0, 0));
        m.black_white = 1;
        m.method = 8;
        upd_txt2d_effects(&mut m);
        assert_eq!((m.fx_transp, m.skip), (1, 1));
    }
}
