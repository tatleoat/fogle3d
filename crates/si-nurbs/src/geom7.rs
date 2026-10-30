// SPDX-License-Identifier: MIT
//! nurbsgeom7: the leftovers of `nrb_convert.c` and `nrb_point.c` on the memory image (finding 0025, section 22).

use crate::image::at8;
use si_mem::Mem;
use si_x87::{x, F80};

/// [soft 0x084eb194 checkValidOpenLinear]: a direction whose end knots repeat too much is emptied. Counting from the front
/// (`i` from 0 while below count - 2) how many successive knot differences are below 1e-6 (each takes one from order - 1),
/// and from the back (`i` from count - 1 down to 1; knot[i + 1] - knot[i], the first of which is one past the knot block)
/// the same from the second order - 1. If either result is not above zero, the knot block is freed and the knot pointer and
/// count zeroed.
pub fn check_valid_open_linear(m: &mut Mem, d: u32) {
    let tol = x(1e-06);
    let order = m.i32(d);
    let nk = m.i32(d + 8);
    let k = |m: &Mem, i: i32| -> F80 { x(m.f64(at8(m.u32(d + 0xc), i))) };
    let mut a = order.wrapping_sub(1);
    let mut i: i32 = 0;
    let lim = nk.wrapping_sub(2);
    while i < lim {
        if (k(m, i.wrapping_add(1)) - k(m, i)).abs().lt(tol) {
            a = a.wrapping_sub(1);
            i = i.wrapping_add(1);
        } else {
            break;
        }
    }
    let mut b = order.wrapping_sub(1);
    let mut j = m.i32(d + 8).wrapping_sub(1);
    while j > 0 {
        if (k(m, j.wrapping_add(1)) - k(m, j)).abs().lt(tol) {
            b = b.wrapping_sub(1);
            j = j.wrapping_sub(1);
        } else {
            break;
        }
    }
    if a <= 0 || b <= 0 {
        let p = m.u32(d + 0xc);
        if p != 0 {
            m.free(p);
            m.set_u32(d + 0xc, 0);
        }
        m.set_u32(d + 0xc, 0);
        m.set_i32(d + 8, 0);
    }
}

/// [soft 0x084ed2f4 nurbsSrfAveragePointsDistance]: the curve of cumulative mean chord lengths between neighbouring rows of a
/// surface's net, for `SI_nurbsSrfClose`. With `along_u` (the short argument nonzero) the curve takes the U direction's order,
/// count and closed flag; each of its points k (from 1) is the mean over the V rows of the distance between net points
/// (row, k - 1) and (row, k) after dividing x, y, z by w (every step in the register; the sum of squares dx, dy, dz in that
/// order, `fsqrt`, and a second `fsqrt` when the new curve's parametrisation word is 4), divided by the row count, stored
/// as x (y = z = 0, w = 1), then added to the previous point's x. Without it the V direction and the neighbouring V rows,
/// divided by the U count. Point 0 is the allocated (0, 0, 0, 1). The flag word of every point k >= 1 is a stale stack word in the
/// original (written here as 0). The type word (+0x24) is copied from the surface (+0x4c); a 2 is made 3 for
/// `SI_knotsCrvReparametrization` and restored. Answers the curve (0 if none).
pub fn srf_average_points_distance(m: &mut Mem, s: u32, along_u: u16) -> Result<u32, crate::image::Halt> {
    use si_hrc::image::nurbs::{nurbs_crv_allocate, nurbs_points_allocate};
    let c = nurbs_crv_allocate(m).addr();
    if c == 0 {
        return Ok(0);
    }
    if along_u != 0 {
        m.set_i32(c, m.i32(s));
        m.set_i32(c + 4, m.i32(s + 4));
        m.set_u16(c + 0x20, m.u16(s + 0x20));
    } else {
        m.set_i32(c, m.i32(s + 0x24));
        m.set_i32(c + 4, m.i32(s + 0x28));
        m.set_u16(c + 0x20, m.u16(s + 0x44));
    }
    m.set_i32(c + 0x24, m.i32(s + 0x4c));
    let pts = nurbs_points_allocate(m, m.i32(c + 4));
    m.set_u32(c + 0x2c, pts);
    // the point (x/w, y/w, z/w) of net index `i`, each division in the register
    let net = |m: &Mem, i: i32| -> [F80; 3] {
        let p = m.u32(s + 0x54).wrapping_add((i as u32).wrapping_mul(0x24));
        let w = x(m.f64(p + 0x18));
        [x(m.f64(p)) / w, x(m.f64(p + 8)) / x(m.f64(p + 0x18)), x(m.f64(p + 0x10)) / x(m.f64(p + 0x18))]
    };
    let chord = |m: &Mem, i: i32, j: i32, quad: bool| -> F80 {
        let (a, b) = (net(m, i), net(m, j));
        let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
        let mut d = (dz * dz + (dy * dy + dx * dx)).sqrt();
        if quad {
            d = d.sqrt();
        }
        d
    };
    let (steps, per, stride_i, stride_j) = if along_u != 0 {
        (m.i32(s + 4).wrapping_sub(1), m.i32(s + 0x28), 1i32, m.i32(c + 4))
    } else {
        (m.i32(s + 0x28).wrapping_sub(1), m.i32(s + 4), m.i32(s + 4), 1i32)
    };
    let quad = m.i32(c + 0x24) == 4;
    let mut k: i32 = 1;
    let mut e: i32 = 0;
    while e < steps {
        let mut acc = F80::ZERO;
        let mut r: i32 = 0;
        while r < per {
            let base = if along_u != 0 { r.wrapping_mul(stride_j).wrapping_add(e) } else { e.wrapping_mul(stride_i).wrapping_add(r) };
            let other = base.wrapping_add(if along_u != 0 { stride_i } else { stride_i });
            acc = acc + chord(m, base, other, quad);
            r += 1;
        }
        let avg = (acc / F80::from_i32(per)).to_f64();
        let p = m.u32(c + 0x2c).wrapping_add((k as u32).wrapping_mul(0x24));
        m.set_f64(p, avg);
        m.set_f64(p + 8, 0.0);
        m.set_f64(p + 0x10, 0.0);
        m.set_u32(p + 0x18, 0);
        m.set_u32(p + 0x1c, 0x3ff0_0000);
        m.set_u32(p + 0x20, 0); // a stale stack word in the original
        let prev = m.f64(p.wrapping_sub(0x24));
        m.set_f64(p, (x(m.f64(p)) + x(prev)).to_f64());
        k += 1;
        e += 1;
    }
    let mut restore = false;
    if m.i32(c + 0x24) == 2 {
        m.set_i32(c + 0x24, 3);
        restore = true;
    }
    crate::image::knots_crv_reparametrization(m, c)?;
    if restore {
        m.set_i32(c + 0x24, 2);
    }
    Ok(c)
}

const SI_AXES: [u32; 3] = [0x086d_c1ec, 0x086d_c1f8, 0x086d_c204];

/// [soft 0x08503068 SI_nurbsDistanceToValueFrenet]: `(curve, distance (float), point (3 floats), frames slot, axes (9 floats))`.
/// The point is cleared and the nine axes floats set to `SI_xAxis`, `SI_yAxis`, `SI_zAxis`. With the curve's length (cumulated
/// lengths, `SI_nurbsCurveLength`): `|length|` below 0.0001 puts the point at parameter 0 (axes stay). A closed curve takes the
/// distance's remainder by the length (`fprem`, the result through a double and a float). An open curve before its start
/// (distance below 0): the frame at the range start (`nurbsFrenetCoord`), the point there, then moved along the frame's second
/// vector (axes + 0xc) by the distance, `float(distance * axis + point)` per component in the register; beyond the end (distance
/// above the length): the same at the range end with `distance - length` and each product rounded to float first. Otherwise
/// the factor `SI_nurbsDistanceToFactor(distance)` gives the parameter: the point there (floats) and the frame there.
/// `update` stands for `SI_nurbsUpdateFrenet (0x085020a8)` (unported; reached when `*frames_slot` is 0).
pub fn nurbs_distance_to_value_frenet(
    m: &mut Mem,
    o: &mut dyn si_mem::Outside,
    c: u32,
    distance: f32,
    point: u32,
    frames: u32,
    axes: u32,
    update: &mut dyn FnMut(&mut Mem, u32, u32),
) -> Result<(), crate::image::Halt> {
    use crate::image::{crv_value, nurbs_curve_length, nurbs_distance_to_factor};
    let mut dist = distance;
    for off in [8u32, 4, 0] {
        m.set_u32(point + off, 0);
    }
    for (k, a) in SI_AXES.iter().enumerate() {
        for j in 0..3u32 {
            let w = m.u32(a + 4 * j);
            m.set_u32(axes + 0xc * k as u32 + 4 * j, w);
        }
    }
    let length = nurbs_curve_length(m, o, c)?;
    let len64 = length.to_f64();
    let put = |m: &mut Mem, v: [f64; 3]| {
        for (j, w) in v.iter().enumerate() {
            m.set_f32(point + 4 * j as u32, x(*w).to_f32());
        }
    };
    if length.abs().lt(x(0.0001)) {
        let v = crv_value(m, c, 0.0, 0)?;
        put(m, v);
        return Ok(());
    }
    // the point moved along the frame's second vector by `by`; `round` rounds each product to float first
    let shift = |m: &mut Mem, by: F80, round: bool| {
        let mut r = [0f32; 3];
        for (j, slot) in r.iter_mut().enumerate() {
            let mut prod = by * x(m.f32(axes + 0xc + 4 * j as u32));
            if round {
                prod = x(prod.to_f32());
            }
            *slot = (prod + x(m.f32(point + 4 * j as u32))).to_f32();
        }
        for (j, v) in r.iter().enumerate() {
            m.set_f32(point + 4 * j as u32, *v);
        }
    };
    let beyond;
    if m.i16(c + 0x20) != 0 {
        let mut r = x(dist);
        loop {
            let (next, again) = r.fprem(x(len64));
            r = next;
            if !again {
                break;
            }
        }
        dist = x(r.to_f64()).to_f32();
        beyond = 0;
    } else if x(dist as f64).lt(F80::ZERO) {
        beyond = -1;
    } else if x(len64).lt(x(dist as f64)) {
        beyond = 1;
    } else {
        beyond = 0;
    }
    if beyond != 0 {
        let t = if beyond < 0 { m.f64(c + 0x10) } else { m.f64(c + 0x18) };
        crate::geom3::nurbs_frenet_coord(m, c, t, frames, axes, update);
        let v = crv_value(m, c, t, 0)?;
        put(m, v);
        if beyond < 0 {
            shift(m, x(dist as f64), false);
        } else {
            shift(m, x(dist as f64) - x(len64), true);
        }
        return Ok(());
    }
    let f = nurbs_distance_to_factor(m, o, c, dist as f64)?.to_f64();
    let v = crv_value(m, c, f, 0)?;
    put(m, v);
    crate::geom3::nurbs_frenet_coord(m, c, f, frames, axes, update);
    Ok(())
}
