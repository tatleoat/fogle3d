// SPDX-License-Identifier: MIT
//! `metaball.c`, handing the points of a metaball system or a metaball to a callback
//! (finding 0038): the vertices of the surface and the three outline rings.
//!
//! The callback is `void (SI_Vector *point, int index, void **data)`: a code address read
//! from the caller's arguments, reached through [`Outside`] as a routine known by its
//! address only ([`Routine::at`]).

use crate::metaball::SYS_TRIANGLES;
use crate::metaball::SYS_TRI_COUNT;
use crate::surface::{si_meta_sys_get_surface, SI_META_CIRCLE};
use si_hrc::image::meta::ball;
use si_mem::{Mem, Outside, Routine};
use si_model::model_image::mx;
use si_x87::{x, Round, F80};

/// One row of the matrix as the loops read it: the float at row `i`, column `j`.
fn mat(m: &Mem, matrix: u32, i: u32, j: u32) -> f32 {
    m.f32(matrix + 16 * i + 4 * j)
}

/// A point through the matrix, all in the register, stored to a float once: the products of
/// the three terms (`t`) in row order, summed left to right, then the fourth row.
fn through(m: &Mem, matrix: u32, t: [F80; 3], j: u32) -> f32 {
    (((t[0] * mat(m, matrix, 0, j) + t[1] * mat(m, matrix, 1, j)) + t[2] * mat(m, matrix, 2, j)) + mat(m, matrix, 3, j)).to_f32()
}

/// [soft 0x084614b0 si_metaballContourTraversePnts]: the vertices of the surface of the
/// system (built first with `SI_metaSysGetSurface(sys, 0, 0)`; 0x48 bytes each, the position
/// the first three doubles) through `matrix` (16 floats), handed to `callback` with their
/// index. The point is rounded to a float first, then multiplied and added in the register
/// as `((x·m0j + y·m1j) + z·m2j) + m3j`, each coordinate stored as a float. The count and
/// the list are read again after every call.
pub fn si_metaball_contour_traverse_pnts(m: &mut Mem, o: &mut dyn Outside, sys: u32, matrix: u32, callback: u32, data: u32) {
    si_meta_sys_get_surface(m, o, sys, 0, 0);
    let point = m.local(12);
    let mut index: i32 = 0;
    while index < m.i32(sys + SYS_TRI_COUNT) {
        let at = m.u32(sys + SYS_TRIANGLES).wrapping_add((index as u32).wrapping_mul(0x48));
        let t = [x(x(m.f64(at)).to_f32()), x(x(m.f64(at + 8)).to_f32()), x(x(m.f64(at + 0x10)).to_f32())];
        for j in 0..3 {
            let v = through(m, matrix, t, j);
            m.set_f32(point + 4 * j, v);
        }
        o.call(m, Routine::at(callback), &[point, index as u32, data]);
        index += 1;
    }
}

/// The ring a diameter exponent selects: `trunc(((e - 1) * 140) / 7)` in the register, 0 when
/// below 1 (also the integer indefinite of a NaN), at most 0x8c.
fn ring(e: f64) -> u32 {
    let v = ((x(e) - x(1.0f64)) * x(140.0f64)) / x(7.0f64);
    let i = v.to_i32(Round::Zero);
    if i > 0 {
        if i > 0x8b {
            0x8c
        } else {
            i as u32
        }
    } else {
        0
    }
}

/// [soft 0x08461594 SI_metaballTraversePnts]: the three outline rings of a metaball (24
/// points each, indices 0 to 0x47) through its matrix times `matrix`, handed to `callback`.
/// **`matrix` is overwritten** with the product of the ball's matrix (+0xc) and itself, as
/// the original does. The rings are those of the table `metaCircleInit` fills, picked by the
/// ball's three exponents (+0xdc, +0xe4, +0xec); every point is scaled by the ratio (+0xcc),
/// or by 1 when the weight (+0xd4) is not above 1 (a NaN weight is above) or `flag` is not 0.
/// Ring 1 (index 0..) spans x and y, ring 2 (0x18..) y and z, ring 3 (0x30..) x and z; the
/// missing coordinate is a product with a zero (it only matters for non-finite matrix
/// entries). Each coordinate is `((t0·m0j + t1·m1j) + t2·m2j) + m3j` in the register.
pub fn si_metaball_traverse_pnts(m: &mut Mem, o: &mut dyn Outside, pball: u32, matrix: u32, flag: i16, callback: u32, data: u32) {
    let mut scale = m.f64(pball + ball::RATIO);
    mx::multiply(m, pball + ball::MATRIX, matrix, matrix);
    if x(1.0f64).ge(x(m.f64(pball + ball::WEIGHT))) || flag != 0 {
        scale = 1.0;
    }
    let a = ring(m.f64(pball + ball::EXPONENT));
    let b = ring(m.f64(pball + 0xe4));
    let c = ring(m.f64(pball + 0xec));
    let point = m.local(12);
    let s = x(scale);
    // a ring's coordinate scaled and stored as a float
    let comp = |m: &Mem, r: u32, i: u32, k: u32| -> F80 { x((s * m.f32(SI_META_CIRCLE + r * 0xc0 + i * 8 + 4 * k)).to_f32()) };
    let zero = F80::ZERO;
    for pass in 0..3u32 {
        for i in 0..0x18u32 {
            let t = match pass {
                0 => [comp(m, a, i, 0), comp(m, b, i, 1), zero],
                1 => [zero, comp(m, b, i, 0), comp(m, c, i, 1)],
                _ => [comp(m, a, i, 1), zero, comp(m, c, i, 0)],
            };
            for j in 0..3 {
                let v = through(m, matrix, t, j);
                m.set_f32(point + 4 * j, v);
            }
            o.call(m, Routine::at(callback), &[point, pass * 0x18 + i, data]);
        }
    }
}
