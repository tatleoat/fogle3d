// SPDX-License-Identifier: MIT
//! The moving frames along a spline (`spline.c`) on the one memory image (finding 0020,
//! section 22; finding 0024): `si_splineUpdateFrenet` builds one frame a sample of the
//! length table, `splineFrenetCoord` and `SI_splineDistanceToValueFrenet` read them. They
//! serve the deformation along a spline.
//!
//! A frame is `SI_Coordsys` ([`Coordsys`], 0x24 bytes): three vectors. The middle one is
//! the curve's unit tangent; the first is carried from sample to sample by the smallest
//! rotation that turns one tangent into the next (no twist about the tangent), and the
//! third is their cross product.

use si_math::algebra::{matrix_axis_rotation, matrix_copy, matrix_inverse, matrix_multiply, matrix_z_align, vector_angle, vector_multiply_matrix};
use si_math::types::{Matrix, Vector as MVector};
use si_mem::{record, Mem, Ptr32};
use si_types::geom::Spline;
use si_x87::{fmov, x, F80};

use crate::intersect_image::{load_curve, spline_nb_segment};
use crate::patch_grid::matrix_derivative_precision;
use crate::spline_image::{spline_touch, Outside, SI_IDENTITE};

record! {
    /// A frame on a curve. Laid out from `si_splineUpdateFrenet`, which makes them
    /// (`SI_calloc(n, 0x24)`) and writes every field; the names are this port's.
    pub struct Coordsys = "SI_Coordsys", size 0x24 {
        0x00 side: [f32; 3], "The X axis of the first frame's alignment, carried along the curve.";
        0x0c tangent: [f32; 3], "The unit tangent (not normalised when its length is not above 0.0001).";
        0x18 up: [f32; 3], "side × tangent.";
    }
}

/// `SI_xAxis` (0x086dc1ec): the unit vector (1, 0, 0), three floats in `.data`. Declared
/// outside a `globals!` block because its initial value is three floats; [`init`] stores
/// them.
pub const SI_X_AXIS: Ptr32<[f32; 3]> = Ptr32::new(0x086d_c1ec);

/// The initial values of this module's globals (and of [`crate::spline_image`]'s, which
/// the routines here read).
pub fn init(m: &mut Mem) {
    crate::spline_image::init(m);
    m.write(SI_X_AXIS, [1.0, 0.0, 0.0]);
}

/// `SI_splineUpdateLength` (0x0848cda4), with its calls: the touch when asked
/// ([`spline_touch`]), the sample count stored at +0x68 (1 for a linear spline),
/// `SI_calloc(samples × segments + 1, 4)` stored at +0x6c without a look at what was
/// there. The table's entries are computed by the value form
/// [`crate::length::update_length`] (cited and compared there). A spline without
/// segments is left as it is.
// adapter: not in place
pub fn spline_update_length(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>, samples: i16, touch_first: i16) {
    if touch_first == 1 {
        spline_touch(m, o, s);
    }
    if spline_nb_segment(m, s) == 0 {
        return;
    }
    let mut c = load_curve(m, s);
    crate::length::update_length(&mut c, samples, 0, &mut crate::NoHost);
    m[s].nb_lengths = c.rec.nb_lengths;
    let table = m.si_calloc(c.lengths.len() as u32, 4);
    m[s].lengths = Ptr32::new(table);
    for (i, v) in c.lengths.iter().enumerate() {
        m.set_f32(table + 4 * i as u32, *v);
    }
}

/// The whole length of a spline [soft 0x0848df48 SI_splineLength]: without a table,
/// [`spline_update_length`] with 100 samples and the touch; then the table's last entry
/// (`samples × segments`), or 0 for a spline without segments.
pub fn spline_length(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>) -> f32 {
    if m[s].lengths.is_null() {
        spline_update_length(m, o, s, 100, 1);
    }
    let segments = spline_nb_segment(m, s);
    if segments == 0 {
        return 0.0;
    }
    m.f32(m[s].lengths.addr().wrapping_add(((m[s].nb_lengths as i32).wrapping_mul(segments) as u32) << 2))
}

fn v3(m: &Mem, a: u32) -> [f32; 3] {
    [m.f32(a), m.f32(a + 4), m.f32(a + 8)]
}

fn put3(m: &mut Mem, a: u32, v: [f32; 3]) {
    for (k, c) in v.iter().enumerate() {
        m.set_f32(a + 4 * k as u32, *c);
    }
}

fn mv(v: [f32; 3]) -> MVector {
    MVector::new(v[0], v[1], v[2])
}

/// A vector divided by its float length `float(√((y² + x²) + z²))` when that is above
/// 0.0001 in magnitude: every component `float(c / length)`; else as it was. The
/// components come in unrounded where the original has them in the register.
fn unit(v: [F80; 3], rounded: [f32; 3]) -> [f32; 3] {
    let len = (((v[1] * v[1]) + (v[0] * v[0])) + (v[2] * v[2])).sqrt().to_f32();
    if x(len).abs().gt(x(0.0001f64)) {
        [(v[0] / len).to_f32(), (v[1] / len).to_f32(), (v[2] / len).to_f32()]
    } else {
        rounded
    }
}

/// The tangent of the difference matrix's row 3, stored at `frame + 0xc` and normalised
/// there.
fn tangent(m: &mut Mem, frame: u32, r: &Matrix) {
    let t = [fmov(r[3][0]), fmov(r[3][1]), fmov(r[3][2])];
    put3(m, frame + 0xc, t);
    let len = (((x(t[1]) * t[1]) + (x(t[0]) * t[0])) + (x(t[2]) * t[2])).sqrt().to_f32();
    if x(len).abs().gt(x(0.0001f64)) {
        put3(m, frame + 0xc, [(x(t[0]) / len).to_f32(), (x(t[1]) / len).to_f32(), (x(t[2]) / len).to_f32()]);
    }
}

/// A frame's first and third vector from the vector `a` to carry and a matrix:
/// `side = a × matrix` (a point transformed: the translation row is added), each
/// component `((a.y m[1][j] + a.x m[0][j]) + a.z m[2][j]) + m[3][j]` rounded on the store;
/// `up = side × tangent` from the **unrounded** side.
fn carry(m: &mut Mem, frame: u32, a: [f32; 3], mat: &Matrix) {
    let col = |j: usize| ((x(a[1]) * mat[1][j] + x(a[0]) * mat[0][j]) + x(a[2]) * mat[2][j]) + mat[3][j];
    let sx = col(0);
    m.set_f32(frame, sx.to_f32());
    let sy = ((x(a[0]) * mat[0][1] + x(a[1]) * mat[1][1]) + x(a[2]) * mat[2][1]) + mat[3][1];
    m.set_f32(frame + 4, sy.to_f32());
    let sz = (x(a[2]) * mat[2][2] + (x(a[1]) * mat[1][2] + x(a[0]) * mat[0][2])) + mat[3][2];
    let (ty, tz) = (m.f32(frame + 0x10), m.f32(frame + 0x14));
    m.set_f32(frame + 8, sz.to_f32());
    m.set_f32(frame + 0x18, (sy * tz - sz * ty).to_f32());
    let tx = m.f32(frame + 0xc);
    m.set_f32(frame + 0x1c, (sz * tx - x(tz) * sx).to_f32());
    m.set_f32(frame + 0x20, (x(ty) * sx - x(tx) * sy).to_f32());
}

/// Builds the frames of a spline [soft 0x0848d298 si_splineUpdateFrenet] and stores
/// their address through `out`; 0 for a spline without segments (nothing stored), else 1.
///
/// 1. [`spline_touch`], then [`spline_length`] (so the length table exists; its value
///    is dropped). `samples` is the table's (+0x68).
/// 2. `SI_calloc(samples × segments + 1, 0x24)`.
/// 3. **Tangents.** With D = `SI_matrixDerivativePrecision(samples)` × the type's basis:
///    per segment the geometry G (four key positions from key `seg` on, the index taken
///    modulo the key count; for a Bézier spline the segment's four control points, a
///    straight segment's inner two `float(0.6666667 a + 0.3333333 b)`, with the doubles
///    0x3fe55555673aa1bc and 0x3fd55555318abc87) and R = D × G. The first frame takes
///    row 3 of the first R; then, `samples` times a segment: row 3 += row 2, row 2 += row
///    1, row 1 += row 0 (each element rounded to float) and the next frame takes row 3.
///    A tangent is divided by its float length when that is above 0.0001.
/// 4. **The first frame**: with M the inverse of `SI_matrixZAlign(tangent)`, `side` is
///    `SI_xAxis × M` and `up = side × tangent`.
/// 5. **Every next frame**: the axis `previous tangent × tangent`, divided by its float
///    length when that is above 0.0001; when the sum of its components' magnitudes is
///    above 0.5 the rotation `SI_matrixAxisRotation(SI_vectorAngle(previous tangent,
///    tangent), axis)`, else the identity; `side = previous side × rotation`,
///    `up = side × tangent`.
/// 6. **A closed spline**: the angle between the first frame's `side` and the last
///    one's (`SI_vectorAngle`), negated when a component of their cross product and the
///    same component of the first tangent have opposite signs; frame i of n then turns
///    about its own tangent by `float((−angle / n) × i)`: its three vectors through
///    `SI_vectorMultiplyMatrix`, each normalised again.
pub fn spline_update_frenet(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>, out: u32) -> i32 {
    spline_touch(m, o, s);
    spline_length(m, o, s);
    let segments = spline_nb_segment(m, s);
    if segments == 0 {
        return 0;
    }
    let samples = m[s].nb_lengths as i32;
    let frames = m.si_calloc(samples.wrapping_mul(segments).wrapping_add(1) as u32, Coordsys::SIZE as u32);
    m.set_u32(out, frames);
    let mut frame = frames;
    // a type outside 0..=3 leaves the basis a local nobody set; zeros here
    let mut basis = [[0.0f32; 4]; 4];
    crate::basis::spline_basis_matrix(m[s].kind, m[s].tension, &mut basis);
    let d = matrix_multiply(&matrix_derivative_precision(samples as i16), &basis);
    let mut g = [[0.0f32; 4]; 4];
    for row in g.iter_mut() {
        row[3] = 1.0;
    }
    let (c6, c3) = (x(f64::from_bits(0x3fe5_5555_673a_a1bc)), x(f64::from_bits(0x3fd5_5555_318a_bc87)));
    let mut first = true;
    let mut key = m[s].keys.addr();
    let mut seg = 0;
    while seg < segments && m.fault().is_none() {
        if m[s].kind != 1 {
            for (k, row) in g.iter_mut().enumerate() {
                let (_, i) = crate::patch_image::idiv(m, seg + k as i32, m[s].nb_keys as u16 as i32);
                let p = v3(m, m[s].keys.addr().wrapping_add((i as u32).wrapping_mul(0x30)) + 0xc);
                (row[0], row[1], row[2]) = (fmov(p[0]), fmov(p[1]), fmov(p[2]));
            }
        } else {
            let next = if seg == m[s].nb_keys as u16 as i32 - 1 { m[s].keys.addr() } else { m[s].keys.addr().wrapping_add(((seg + 1) as u32).wrapping_mul(0x30)) };
            let (p0, p3) = (v3(m, key + 0xc), v3(m, next + 0xc));
            for k in 0..3 {
                g[0][k] = fmov(p0[k]);
                g[3][k] = fmov(p3[k]);
            }
            if m.u16(next + 0x2a) == 2 {
                for k in 0..3 {
                    g[1][k] = (c6 * p0[k] + c3 * p3[k]).to_f32();
                    g[2][k] = (c6 * p3[k] + c3 * p0[k]).to_f32();
                }
            } else {
                let (p1, p2) = (v3(m, key + 0x18), v3(m, next));
                for k in 0..3 {
                    g[1][k] = fmov(p1[k]);
                    g[2][k] = fmov(p2[k]);
                }
            }
            key = next;
        }
        let mut r = matrix_multiply(&d, &g);
        if first {
            tangent(m, frame, &r);
            frame += 0x24;
            first = false;
        }
        let mut step = 0;
        while step < samples && m.fault().is_none() {
            for row in [3usize, 2, 1] {
                for col in 0..4 {
                    r[row][col] = (x(r[row][col]) + r[row - 1][col]).to_f32();
                }
            }
            tangent(m, frame, &r);
            frame += 0x24;
            step += 1;
        }
        seg += 1;
    }

    // the first frame
    let mut frame = m.u32(out);
    let align = matrix_z_align(&mv(v3(m, frame + 0xc)));
    // a matrix without an inverse leaves the original's result a local nobody set
    let inverse = matrix_inverse(&align).unwrap_or([[0.0; 4]; 4]);
    let axis = m.read(SI_X_AXIS);
    carry(m, frame, axis, &inverse);
    let mut side = v3(m, frame);
    let mut before = v3(m, frame + 0xc);
    frame += 0x24;
    let count = (m[s].nb_lengths as i32).wrapping_mul(segments);
    let mut left = count;
    while left > 0 && m.fault().is_none() {
        let t = v3(m, frame + 0xc);
        let cross = [x(before[1]) * t[2] - x(before[2]) * t[1], x(before[2]) * t[0] - x(before[0]) * t[2], x(before[0]) * t[1] - x(before[1]) * t[0]];
        let axis = unit(cross, [cross[0].to_f32(), cross[1].to_f32(), cross[2].to_f32()]);
        let turn = if ((x(axis[1]).abs() + x(axis[0]).abs()) + x(axis[2]).abs()).gt(x(0.5f64)) {
            let angle = vector_angle(&mv(before), &mv(t));
            matrix_axis_rotation(angle, &mv(axis))
        } else {
            matrix_copy(&m[SI_IDENTITE])
        };
        carry(m, frame, side, &turn);
        side = v3(m, frame);
        before = v3(m, frame + 0xc);
        frame += 0x24;
        left -= 1;
    }

    if m[s].closed != 0 {
        let frames = m.u32(out);
        let (a0, t0) = (v3(m, frames), v3(m, frames + 0xc));
        let an = v3(m, frames.wrapping_add((count as u32).wrapping_mul(0x24)));
        let mut angle = vector_angle(&mv(a0), &mv(an));
        let cross = [x(a0[1]) * an[2] - x(a0[2]) * an[1], x(a0[2]) * an[0] - x(an[2]) * a0[0], x(a0[0]) * an[1] - x(a0[1]) * an[0]];
        let zero = F80::ZERO;
        let against = |c: F80, t: f32| (c.gt(zero) && zero.gt(x(t))) || (c.lt(zero) && zero.lt(x(t)));
        if against(cross[0], t0[0]) || against(cross[1], t0[1]) || against(cross[2], t0[2]) {
            angle = (-x(angle)).to_f32();
        }
        let mut i = 1;
        while i <= count && m.fault().is_none() {
            let f = m.u32(out).wrapping_add((i as u32).wrapping_mul(0x24));
            let a = (((-x(angle)) / F80::from_i32(count)) * F80::from_i32(i)).to_f32();
            let turn = matrix_axis_rotation(a, &mv(v3(m, f + 0xc)));
            let mut moved = [[0.0f32; 3]; 3];
            for (k, slot) in moved.iter_mut().enumerate() {
                let v = vector_multiply_matrix(&mv(v3(m, f + 0xc * k as u32)), &turn);
                let v = [v.x, v.y, v.z];
                *slot = unit([x(v[0]), x(v[1]), x(v[2])], v);
            }
            for (k, v) in moved.iter().enumerate() {
                for (j, c) in v.iter().enumerate() {
                    m.set_u32(f + 0xc * k as u32 + 4 * j as u32, c.to_bits());
                }
            }
            i += 1;
        }
    }
    1
}

/// `SI_yAxis` (0x086dc1f8) and `SI_zAxis` (0x086dc204): the other two unit vectors,
/// beside [`SI_X_AXIS`] in `.data`; [`init_axes`] stores all three.
pub const SI_Y_AXIS: Ptr32<[f32; 3]> = Ptr32::new(0x086d_c1f8);
pub const SI_Z_AXIS: Ptr32<[f32; 3]> = Ptr32::new(0x086d_c204);

/// [`init`], and the two other axes.
pub fn init_axes(m: &mut Mem) {
    init(m);
    m.write(SI_Y_AXIS, [0.0, 1.0, 0.0]);
    m.write(SI_Z_AXIS, [0.0, 0.0, 1.0]);
}

/// `SI_splineDistanceToFactor` (0x0848ca64) for a spline that has its length table:
/// through the value form [`crate::length::distance_to_factor`] (cited and compared
/// there), which then allocates nothing and writes nothing.
// adapter: not in place
pub fn spline_distance_to_factor(m: &mut Mem, s: Ptr32<Spline>, distance: f32) -> F80 {
    let mut c = load_curve(m, s);
    let n = (c.rec.nb_lengths as i32).wrapping_mul(spline_nb_segment(m, s)).wrapping_add(1).max(0) as u32;
    if !c.rec.lengths.is_null() {
        c.lengths = (0..n).map(|i| m.f32(c.rec.lengths.addr() + 4 * i)).collect();
    }
    crate::length::distance_to_factor(&mut c, distance, &mut crate::NoHost)
}

/// The frame at a parameter [soft 0x0848f3b8 splineFrenetCoord], stored at `out`
/// (cleared first); 0 for a spline without segments, else 1.
///
/// `*frames` null: [`spline_update_frenet`] makes them. The parameter (in the register
/// throughout) is brought into the curve: on a closed spline the number of segments is
/// subtracted while it is below the parameter and then added while the parameter is
/// negative; on an open one the parameter is held between 0 and the number of segments.
/// With `i = trunc(u × samples)`: at or beyond the last frame that frame is copied
/// (nine words, from wherever `i` leads); else every vector is
/// `a + (b − a) × (u × samples − i)` between frame `i` and the next, stored rounded,
/// and then divided by its float length when that is above 0.0001 (from the unrounded
/// components).
pub fn spline_frenet_coord(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>, u: f32, frames: u32, out: u32) -> i32 {
    let mut u = x(u);
    for off in [8u32, 4, 0, 0x14, 0x10, 0xc, 0x20, 0x1c, 0x18] {
        m.set_u32(out + off, 0);
    }
    if m.u32(frames) == 0 {
        spline_update_frenet(m, o, s, frames);
    }
    let segments = spline_nb_segment(m, s);
    if segments == 0 {
        return 0;
    }
    let n = F80::from_i32(segments);
    let mut turns = 0u32;
    let mut more = |m: &Mem| {
        turns += 1;
        if turns > 1 << 22 {
            m.record_fault(crate::merge_image::ENDLESS, 0, si_mem::Access::Read);
            return false;
        }
        true
    };
    if m[s].closed != 0 {
        while n.lt(u) && more(m) {
            u = u - n;
        }
        if F80::ZERO.gt(u) {
            loop {
                u = u + n;
                if !(u.lt(F80::ZERO) && more(m)) {
                    break;
                }
            }
        }
    } else {
        if n.lt(u) {
            u = n;
        }
        if F80::ZERO.gt(u) {
            u = F80::ZERO;
        }
    }
    let samples = m[s].nb_lengths as i32;
    let scaled = u * F80::from_i32(samples);
    let i = scaled.to_i32(si_x87::Round::Zero);
    let at = m.u32(frames).wrapping_add((i as u32).wrapping_mul(0x24));
    if i >= segments.wrapping_mul(samples) {
        m.copy(out, at, 0x24);
        return 1;
    }
    let f = scaled - F80::from_i32(i);
    for k in [0u32, 0xc, 0x18] {
        let mut r = [F80::ZERO; 3];
        for (j, slot) in r.iter_mut().enumerate() {
            let a = m.f32(m.u32(frames).wrapping_add((i as u32).wrapping_mul(0x24)) + k + 4 * j as u32);
            let b = m.f32(m.u32(frames).wrapping_add((i as u32).wrapping_mul(0x24)) + 0x24 + k + 4 * j as u32);
            *slot = ((x(b) - a) * f) + a;
            m.set_f32(out + k + 4 * j as u32, slot.to_f32());
        }
        let v = unit(r, [r[0].to_f32(), r[1].to_f32(), r[2].to_f32()]);
        put3(m, out + k, v);
    }
    1
}

/// The point and the frame at a distance along a spline
/// [soft 0x0848f6f8 SI_splineDistanceToValueFrenet]. The point is cleared and the frame
/// set to the three axes (`SI_xAxis`, `SI_yAxis`, `SI_zAxis`) first. Then, with `length`
/// from [`spline_length`]:
///
/// - `|length|` below 0.0001: the point at parameter 0, and the axes stay;
/// - a closed spline: the distance becomes its remainder by the length (`fprem`, the
///   result through a double), and the general case follows;
/// - an open spline, a negative distance: the frame and the point at parameter 0, the
///   point then moved along the frame's middle vector by the distance
///   (`float(distance × t + p)`);
/// - an open spline, a distance beyond the length: the same at the last parameter (the
///   number of segments), moved by `distance − length`;
/// - else: `u = SI_splineDistanceToFactor(distance)`, the point at `float(u)` and the
///   frame at `float(u)`.
pub fn spline_distance_to_value_frenet(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>, distance: f32, point: u32, frames: u32, out: u32) {
    let mut distance = distance;
    for off in [8u32, 4, 0] {
        m.set_u32(point + off, 0);
    }
    for (k, axis) in [SI_X_AXIS, SI_Y_AXIS, SI_Z_AXIS].into_iter().enumerate() {
        for j in 0..3 {
            let w = m.u32(axis.addr() + 4 * j);
            m.set_u32(out + 0xc * k as u32 + 4 * j, w);
        }
    }
    let length = spline_length(m, o, s);
    if x(length).abs().lt(x(0.0001f64)) {
        crate::intersect_image::spline_value(m, s, 0.0, point);
        return;
    }
    // the point moved along the frame's middle vector
    let shift = |m: &mut Mem, by: F80| {
        let moved: Vec<f32> = (0..3u32).map(|j| ((by * m.f32(out + 0xc + 4 * j)) + m.f32(point + 4 * j)).to_f32()).collect();
        put3(m, point, [moved[0], moved[1], moved[2]]);
    };
    if m[s].closed != 0 {
        let mut r = x(distance);
        loop {
            let (next, again) = r.fprem(x(length));
            r = next;
            if !again {
                break;
            }
        }
        distance = x(r.to_f64()).to_f32();
    } else if F80::ZERO.gt(x(distance)) {
        spline_frenet_coord(m, o, s, 0.0, frames, out);
        crate::intersect_image::spline_value(m, s, 0.0, point);
        shift(m, x(distance));
        return;
    } else if x(distance).gt(x(length)) {
        let last = F80::from_i32(spline_nb_segment(m, s)).to_f32();
        spline_frenet_coord(m, o, s, last, frames, out);
        crate::intersect_image::spline_value(m, s, last, point);
        shift(m, x(distance) - length);
        return;
    }
    let u = spline_distance_to_factor(m, s, distance).to_f32();
    crate::intersect_image::spline_value(m, s, u, point);
    spline_frenet_coord(m, o, s, u, frames, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spline_image::{self, spline_allocate, spline_key_allocate, NoOutside};

    #[test]
    fn frames_along_a_straight_line_all_point_the_same_way() {
        let mut m = Mem::new();
        init_axes(&mut m);
        let lists = m.calloc(1, 0x24);
        m[spline_image::SI_GRAF_LISTS] = lists;
        let s = spline_allocate(&mut m);
        let keys = spline_key_allocate(&mut m, 3);
        m[s].keys = keys;
        m[s].nb_keys = 3;
        for i in 0..3 {
            m[keys.add(i)].pos.z = i as f32;
        }
        crate::spline_image::spline_update_matrix(&mut m, s);
        let frames = m.local(4);
        m.set_u32(frames, 0);
        let out = m.local(0x24);
        // a linear spline has one sample a segment: three frames
        assert_eq!(spline_frenet_coord(&mut m, &mut NoOutside, s, 0.5, frames, out), 1);
        assert_eq!(m.block_size(m.u32(frames)), Some(3 * 0x24));
        let f: Ptr32<Coordsys> = Ptr32::new(out);
        assert_eq!(m[f].tangent, [0.0, 0.0, 1.0]);
        assert_eq!(m[f].side, [1.0, 0.0, 0.0]);
        // half way along: the middle of the line
        let point = m.local(12);
        spline_distance_to_value_frenet(&mut m, &mut NoOutside, s, 1.0, point, frames, out);
        assert_eq!(m.f32(point + 8), 1.0);
        assert!(m.fault().is_none());
    }
}
