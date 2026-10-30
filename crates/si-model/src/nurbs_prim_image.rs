// SPDX-License-Identifier: MIT
//! The NURBS primitives of `primitive.c` and `revolution.c` on `si-mem`'s memory image (finding
//! 0023, unit "prim2", section 44): `SI_revolveNurbs` and the primitives that call it. A
//! function takes the image and the arguments the original takes and makes the original's
//! calls in its order.
//!
//! Floating point follows finding 0008: the original keeps each intermediate value in an x87
//! register and rounds it only where it stores it; the order of the operations below is the
//! order of the listing.

use si_hrc::image::nurbs::{knots_allocate, nurbs_crv_allocate, nurbs_crv_dispose, nurbs_points_allocate, nurbs_srf_allocate};
use si_hrc::image::spline::spline_copy;
use si_math::algebra::{matrix_axis_rotation, matrix_x_rotation, matrix_y_rotation, matrix_z_rotation};
use si_math::types::Vector;
use si_mem::{Mem, Outside, Routine};

/// `SI_statusDisplayMessage` (0x083cca94).
const STATUS_DISPLAY_MESSAGE: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);
use si_x87::{trig, x, F80};


const FULL_TURN_LIMIT: f64 = f64::from_bits(0x4019_21e1_1d61_41fc);
const TWO_PI: f64 = f64::from_bits(0x4019_21fb_5444_2d18);
const TINY: f64 = f64::from_bits(0x3f1a_36e2_eb1c_432d);

fn f(v: f32) -> F80 {
    x(v as f64)
}

/// One point through the matrix at `mat` (16 floats, row vector times matrix) in the register,
/// each coordinate rounded to `double` once: the sums run in the order of the listing.
fn transform_point(m: &mut Mem, src: u32, dst: u32, mat: u32) {
    let e = |m: &Mem, r: u32, c: u32| f(m.f32(mat + 16 * r + 4 * c));
    let (px, py, pz, pw) = (x(m.f64(src)), x(m.f64(src + 8)), x(m.f64(src + 0x10)), x(m.f64(src + 0x18)));
    let nx = (pz * e(m, 2, 0) + (py * e(m, 1, 0) + px * e(m, 0, 0))) + pw * e(m, 3, 0);
    let ny = ((py * e(m, 1, 1) + px * e(m, 0, 1)) + pz * e(m, 2, 1)) + pw * e(m, 3, 1);
    let nz = ((py * e(m, 1, 2) + px * e(m, 0, 2)) + pz * e(m, 2, 2)) + pw * e(m, 3, 2);
    let nw = (pz * e(m, 2, 3) + (py * e(m, 1, 3) + px * e(m, 0, 3))) + pw * e(m, 3, 3);
    m.set_f64(dst, nx.to_f64());
    m.set_f64(dst + 8, ny.to_f64());
    m.set_f64(dst + 0x10, nz.to_f64());
    m.set_f64(dst + 0x18, nw.to_f64());
}

fn put_matrix(m: &mut Mem, at: u32, mat: &[[f32; 4]; 4]) {
    for r in 0..4 {
        for c in 0..4 {
            m.set_f32(at + 16 * r as u32 + 4 * c as u32, mat[r][c]);
        }
    }
}

/// The axis of a model-defined revolution and its reference point, as the listing takes them
/// from the model's spline copy or curve copy.
struct Axis {
    dir: [f32; 3],
    ref_pt: [f32; 3],
    /// the spline copy (`L_a0`) and the curve copy (`L_a4`), 0 when not made.
    spline: u32,
    curve: u32,
    /// "bVar6": the first key is the reference (false for a phantom first key of a type 2 or 3 spline).
    first_key: bool,
}

fn normalise(d: [f32; 3]) -> [f32; 3] {
    let len = (((f(d[1]) * f(d[1])) + (f(d[0]) * f(d[0]))) + f(d[2]) * f(d[2])).sqrt().to_f32();
    if (len.abs() as f64) > TINY {
        let l = f(len);
        [(f(d[0]) / l).to_f32(), (f(d[1]) / l).to_f32(), (f(d[2]) / l).to_f32()]
    } else {
        d
    }
}

/// The revolution of a NURBS curve into a surface [soft 0x08198014 SI_revolveNurbs]: the curve
/// is reversed in place; `n_steps` below the order minus 1 prints "You must have at least a
/// step of %d" (status line, level 3) and answers 0; a revolution of `|angle|` of 6.2830853 or
/// more is a full turn (`n_steps` columns, closed, step 2π/`n_steps` with the sign of the angle),
/// otherwise `n_steps + 1` columns with the step `angle / n_steps`. The other surface order is 2
/// for `spline_type` 1, 3 for 2 and 4 otherwise; the knots are 0..; a full turn of a type other
/// than 1 with `scale_flag` scales the points by 3 / (cos(step) + 2) in the two coordinates
/// other than the axis. `axis` 0, 1, 2 is X, Y, Z; 3 and 4 take the axis and the point from
/// the spline (type 6) or the curve (type 9) of `model`, whose second key or point is the
/// direction, and then turn the points about it. The first column is the curve's points moved
/// by the rotation of angle 0, each next column the previous one turned by the step; the
/// surface is given back with U and V exchanged. The original leaves the matrix, the axis and
/// the reference point unset for an axis above 4 and for 3 and 4 without a model of type 6 or
/// 9; the port takes them as 0.
#[allow(clippy::too_many_arguments)]
pub fn revolve_nurbs(m: &mut Mem, o: &mut dyn Outside, crv: u32, n_steps: i32, spline_type: i32, angle: f32, axis: i32, model: u32, scale_flag: i16, param8: u32) -> u32 {
    let mut full: u16 = 0;
    if crv != 0 {
        si_nurbs::image::crv_inverse(m, o, crv);
    }
    let srf = nurbs_srf_allocate(m).addr();
    let sp0 = m.sp();
    let slots = m.local(8);
    let (slot_spline, slot_curve) = (slots, slots + 4);
    m.set_u32(slot_spline, 0);
    m.set_u32(slot_curve, 0);
    let mut ax = Axis { dir: [0.0; 3], ref_pt: [0.0; 3], spline: 0, curve: 0, first_key: true };
    if srf != 0 {
        if m.i32(crv + 4) == 0 {
            m.set_sp(sp0);
            return 0;
        }
        if n_steps < m.i32(crv).wrapping_sub(1) {
            // sprintf(buffer, "<format with one %d>", curve points - 1); the format is at 0x0868db60
            let text = si_rt::text(0x0868_db60).replacen("%d", &m.i32(crv).wrapping_sub(1).to_string(), 1);
            let buf = m.local(64);
            m.put_cstr(buf, text.as_bytes());
            o.call(m, STATUS_DISPLAY_MESSAGE, &[buf, 3]);
            m.set_sp(sp0);
            return 0;
        }
        let (count, step);
        if !((angle.abs() as f64) >= FULL_TURN_LIMIT) {
            count = n_steps.wrapping_add(1);
            step = (f(angle) / F80::from_i32(n_steps)).to_f32();
        } else {
            full = 1;
            count = n_steps;
            let s = (x(TWO_PI) / F80::from_i32(n_steps)).to_f32();
            step = if angle < 0.0 { -s } else { s };
        }
        let order = m.i32(crv);
        let n = m.i32(crv + 4);
        m.set_i32(srf, order);
        m.set_i32(srf + 0x24, match spline_type { 1 => 2, 2 => 3, _ => 4 });
        let w = m.u16(crv + 0x20);
        m.set_u16(srf + 0x20, w);
        let v = m.u32(crv + 0x34);
        m.set_u32(srf + 0x70, v);
        for off in [0x10u32, 0x14, 0x18, 0x1c] {
            let v = m.u32(crv + off);
            m.set_u32(srf + off, v);
        }
        let v = m.u32(crv + 4);
        m.set_u32(srf + 4, v);
        let v = m.u32(crv + 8);
        m.set_u32(srf + 8, v);
        m.set_u32(srf + 0x74, param8);
        m.set_u16(srf + 0x44, full);
        m.set_i32(srf + 0x28, count);
        m.set_u16(srf + 0x50, 0);
        m.set_u16(srf + 0x5c, 0);
        m.set_u32(srf + 0x60, 0);
        let v = m.u32(crv + 0x24);
        m.set_u32(srf + 0x4c, v);
        let nk = if full != 0 { count.wrapping_add(1) } else { count.wrapping_add(m.i32(srf + 0x24)).wrapping_sub(2) };
        m.set_i32(srf + 0x2c, nk);
        let k = knots_allocate(m, nk);
        m.set_u32(srf + 0x30, k);
        m.set_i32(srf + 0x48, count.wrapping_mul(n));
        let p = nurbs_points_allocate(m, count.wrapping_mul(n));
        m.set_u32(srf + 0x54, p);
        let k = knots_allocate(m, m.i32(srf + 8));
        m.set_u32(srf + 0xc, k);

        let (mut sc_b8, mut sc_b4, mut sc_b0) = (F80::ONE, F80::ONE, F80::ONE);
        if full != 0 && scale_flag != 0 && spline_type != 1 {
            if axis <= 2 {
                let c = trig::cos(step as f64);
                let s = (x(3.0) / (c + 2.0f64)).to_f32();
                match axis {
                    0 => sc_b0 = f(s),
                    1 => sc_b4 = f(s),
                    _ => sc_b8 = f(s),
                }
            }
        }
        let sp = m.u32(srf + 0x54);
        let cp = m.u32(crv + 0x2c);
        for i in 0..n {
            let (s, d) = (cp.wrapping_add(0x24 * i as u32), sp.wrapping_add(0x24 * i as u32));
            let vx = ((x(m.f64(s)) * sc_b4) * sc_b8).to_f64();
            let vy = (x(m.f64(s + 8)) * sc_b0).to_f64();
            m.set_f64(d, vx);
            m.set_f64(d + 8, vy);
            let (z, w) = (m.f64(s + 0x10), m.f64(s + 0x18));
            m.set_f64(d + 0x10, z);
            m.set_f64(d + 0x18, w);
        }

        let mx = m.local(64);
        // The matrix of the first switch.
        let mat = match axis {
            0 => Some(matrix_x_rotation(0.0)),
            1 => Some(matrix_y_rotation(0.0)),
            2 => Some(matrix_z_rotation(0.0)),
            _ => None,
        };
        if let Some(mt) = mat {
            put_matrix(m, mx, &mt);
        } else if axis == 3 || axis == 4 {
            if model != 0 {
                let kind = m.i32(model + 0x30);
                if kind == 6 {
                    let cpy = spline_copy(m, m.u32(model + 0x48)).addr();
                    m.set_u32(slot_spline, cpy);
                    ax.spline = cpy;
                    si_spline::convert_image::spline_multiply_definition(m, &mut si_spline::spline_image::NoOutside, si_mem::Ptr32::new(cpy), model + 0x1c4);
                    let phantom_end = (m.i32(cpy + 0xc).wrapping_sub(2) as u32) <= 1 && m.i16(cpy + 6) != 0;
                    ax.first_key = !phantom_end;
                    let keys = m.u32(cpy + 0x14);
                    let pos = |m: &Mem, k: u32| [m.f32(keys + 0x30 * k + 0xc), m.f32(keys + 0x30 * k + 0x10), m.f32(keys + 0x30 * k + 0x14)];
                    let (a, b) = if ax.first_key { (pos(m, 0), pos(m, 1)) } else { (pos(m, 1), pos(m, 2)) };
                    let d = [(f(b[0]) - f(a[0])).to_f32(), (f(b[1]) - f(a[1])).to_f32(), (f(b[2]) - f(a[2])).to_f32()];
                    ax.dir = normalise(d);
                    ax.ref_pt = a;
                } else if kind == 9 {
                    let cpy = si_nurbs::image::crv_copy(m, m.u32(model + 0x48));
                    m.set_u32(slot_curve, cpy);
                    ax.curve = cpy;
                    si_nurbs::image::crv_apply_matrix(m, cpy, model + 0x1c4, 0);
                    let p = m.u32(cpy + 0x2c);
                    let d = |m: &Mem, o: u32| (x(m.f64(p + 0x24 + o)) - x(m.f64(p + o))).to_f32();
                    ax.dir = normalise([d(m, 0), d(m, 8), d(m, 0x10)]);
                    ax.ref_pt = ref_of_curve(m, cpy);
                }
                let mt = matrix_axis_rotation(0.0, &Vector { x: ax.dir[0], y: ax.dir[1], z: ax.dir[2] });
                put_matrix(m, mx, &mt);
            }
            // Without a model the matrix and the point are whatever the stack held: 0 here.
            let r = ax.ref_pt;
            si_model_ref(m, r, mx);
        }
        for i in 0..n {
            let at = sp.wrapping_add(0x24 * i as u32);
            transform_point(m, at, at, mx);
        }
        // The matrix of the second switch.
        let mat = match axis {
            0 => Some(matrix_x_rotation(step)),
            1 => Some(matrix_y_rotation(step)),
            2 => Some(matrix_z_rotation(step)),
            _ => None,
        };
        if let Some(mt) = mat {
            put_matrix(m, mx, &mt);
        } else if axis == 3 || axis == 4 {
            let mt = matrix_axis_rotation(step, &Vector { x: ax.dir[0], y: ax.dir[1], z: ax.dir[2] });
            put_matrix(m, mx, &mt);
            if model != 0 {
                let kind = m.i32(model + 0x30);
                if kind == 6 {
                    let keys = m.u32(ax.spline + 0x14);
                    let k = if ax.first_key { 0 } else { 1 };
                    ax.ref_pt = [m.f32(keys + 0x30 * k + 0xc), m.f32(keys + 0x30 * k + 0x10), m.f32(keys + 0x30 * k + 0x14)];
                } else if kind == 9 {
                    ax.ref_pt = ref_of_curve(m, ax.curve);
                }
            }
            let r = ax.ref_pt;
            si_model_ref(m, r, mx);
        }
        let mut i = 1;
        while i < count {
            for col in 0..n {
                let s = sp.wrapping_add(0x24u32.wrapping_mul((i - 1).wrapping_mul(n).wrapping_add(col) as u32));
                let d = sp.wrapping_add(0x24u32.wrapping_mul(i.wrapping_mul(n).wrapping_add(col) as u32));
                transform_point(m, s, d, mx);
            }
            i += 1;
        }
        for i in 0..m.i32(srf + 8) {
            let v = m.f64(m.u32(crv + 0xc).wrapping_add(8 * i as u32));
            let d = m.u32(srf + 0xc).wrapping_add(8 * i as u32);
            m.set_f64(d, v);
        }
        // The knots.
        let (order2, nk, kn) = (m.i32(srf + 0x24), m.i32(srf + 0x2c), m.u32(srf + 0x30));
        let kat = |i: i32| kn.wrapping_add(8u32.wrapping_mul(i as u32));
        if spline_type == 1 || full != 0 {
            for i in 0..nk {
                m.set_f64(kat(i), i as f64);
            }
        } else {
            let mut i = 0;
            while i < order2 - 1 {
                m.set_f64(kat(i), 0.0);
                i += 1;
            }
            let mut v = 1;
            while i < (nk - order2) + 1 {
                m.set_f64(kat(i), v as f64);
                v += 1;
                i += 1;
            }
            while i < nk {
                m.set_f64(kat(i), (m.i32(srf + 0x28) - order2 + 1) as f64);
                i += 1;
            }
        }
        if full != 0 {
            m.set_f64(srf + 0x34, 0.0);
            let v = m.f64(kat(nk - 1));
            m.set_f64(srf + 0x3c, v);
        } else {
            let v = m.f64(kat(order2 - 2));
            m.set_f64(srf + 0x34, v);
            let v = m.f64(kat(m.i32(srf + 0x28) - 1));
            m.set_f64(srf + 0x3c, v);
        }
    }
    if m.u32(slot_spline) != 0 {
        si_hrc::image::spline::spline_dispose(m, slot_spline);
    } else if m.u32(slot_curve) != 0 {
        nurbs_crv_dispose(m, slot_curve);
    }
    m.set_sp(sp0);
    if srf == 0 {
        return 0;
    }
    si_nurbs::bridge::srf_swap_uv(m, srf);
    srf
}

/// The reference point of a model curve: the second point divided by its weight, in the
/// register, rounded to float.
fn ref_of_curve(m: &Mem, c: u32) -> [f32; 3] {
    let p = m.u32(c + 0x2c);
    let w = x(m.f64(p + 0x3c));
    [(x(m.f64(p + 0x24)) / w).to_f32(), (x(m.f64(p + 0x2c)) / w).to_f32(), (x(m.f64(p + 0x34)) / w).to_f32()]
}

fn si_model_ref(m: &mut Mem, r: [f32; 3], mx: u32) {
    crate::model_image::state::correct_trans_of_ref_matrix(m, r, mx);
}

/// A torus as a NURBS surface [soft 0x08486d14 SI_nurbsTorus]: the profile is
/// `SI_nurbsCircle(radius, order, n)`, every point of it moved by `offset` in x (in the register,
/// stored as `double`), and turned once about the Y axis by [`revolve_nurbs`] (`steps` columns,
/// spline type `spline_type - 1`, scaled, the last word 1 for type 2 and 3 otherwise); then the
/// surface's words at +0x70 and +0x74 are 1 for `spline_type` 2 and `order` 2 and 3 otherwise
/// and the profile is freed. A circle that could not be made ends the routine at the first
/// read of its record (`Err`).
#[allow(clippy::too_many_arguments)]
pub fn nurbs_torus(m: &mut Mem, o: &mut dyn Outside, radius: f32, offset: f32, n: i32, steps: i32, spline_type: i32, order: i32) -> Result<u32, si_nurbs::image::Halt> {
    let last = if spline_type == 2 { 1 } else { 3 };
    let circle = crate::primitive_image::nurbs_circle(m, radius, order, n)?;
    let sp = m.sp();
    let slot = m.local(4);
    m.set_u32(slot, circle);
    let mut i = 0;
    while i < m.i32(circle + 4) {
        let a = m.u32(circle + 0x2c).wrapping_add(0x24 * i as u32);
        let v = (x(m.f64(a)) + offset).to_f64();
        m.set_f64(a, v);
        i += 1;
    }
    let s = revolve_nurbs(m, o, circle, steps, spline_type - 1, 6.2831855f32, 1, 0, 1, last);
    m.set_u32(s + 0x70, if spline_type == 2 { 1 } else { 3 });
    m.set_u32(s + 0x74, if order == 2 { 1 } else { 3 });
    nurbs_crv_dispose(m, slot);
    m.set_sp(sp);
    Ok(s)
}

/// `sleep` (0x080c4468, the binary's stub) and `SI_mdlStatusMessages` (0x0818129c).
const SLEEP: Routine = Routine::new("sleep", 0x080c_4468);
const MDL_STATUS_MESSAGES: Routine = Routine::new("SI_mdlStatusMessages", 0x0818_129c);
const HALF: f64 = 0.5;
const EPS_1E6: f64 = f64::from_bits(0x3eb0_c6f7_a0b5_ed8d);

/// The failure ending of the arc: the message "Unable to create this object with those
/// parameters. Please retry with new values." (the binary's copy at `text`) on the status line
/// at level 3, a second's pause, `SI_mdlStatusMessages`, the curve freed, 0.
fn arc_fail(m: &mut Mem, o: &mut dyn Outside, text: u32, slot: u32) -> u32 {
    o.call(m, STATUS_DISPLAY_MESSAGE, &[text, 3]);
    o.call(m, SLEEP, &[1]);
    o.call(m, MDL_STATUS_MESSAGES, &[]);
    nurbs_crv_dispose(m, slot);
    0
}

/// A circular arc as a NURBS curve [soft 0x08488394 SI_nurbsArc]: `n` spans of the arc from
/// angle `end` down to `start` (the points run from the end to the start; a sweep that comes
/// out negative gets 2π added), radius `radius`, of order 2, 3 or 4.
///
/// - Order 3 (a rational quadratic): the weight of the interior points is |cos(sweep / n)|
///   (below 1e-6: the failure ending) and the interior points are the intersections of the
///   tangents at the span ends, `x = (sin b − sin a) K`, `y = (cos a − cos b) K` with
///   `K = radius w / (sin b cos a − cos b sin a)`; the end points sit on the circle.
/// - Order 4: `n - 2 + 1` points on the circle, then the interpolating curve through them with
///   end tangents from [`crate::cross_image::bessel_end_nurbs_crv`] (flag 1); the sweep per step
///   2π (within 1e-4) is the failure ending. The result is the new curve; the points are freed.
/// - Otherwise (order 2: the step field set to 1): the points on the circle, z 0 and w 1.
///
/// Every curve gets `SI_knotsCrvReparametrization`. The libm calls follow the order of the
/// listing (the tape of a check answers them).
pub fn nurbs_arc(m: &mut Mem, o: &mut dyn Outside, radius: f32, a: f32, b: f32, order: i32, n: i32) -> Result<u32, si_nurbs::image::Halt> {
    let sp0 = m.sp();
    let slot = m.local(4);
    m.set_u32(slot, 0);
    let crv = nurbs_crv_allocate(m).addr();
    m.set_u32(slot, crv);
    if crv == 0 {
        m.set_sp(sp0);
        return Ok(0);
    }
    let mut n = n;
    m.set_i32(crv, order);
    if order == 4 {
        n = n.wrapping_sub(2);
    } else if order == 2 {
        m.set_u32(crv + 0x34, 1);
    }
    let n1 = n.wrapping_add(1);
    m.set_i32(crv + 4, n1);
    m.set_u32(crv + 0x24, 0);
    m.set_i32(crv + 8, n1.wrapping_add(order).wrapping_sub(2));
    let p = nurbs_points_allocate(m, n1);
    m.set_u32(crv + 0x2c, p);
    let l5c = m.i32(crv + 4).wrapping_sub(1);
    let mut l50 = (f(b) - f(a)).to_f64();
    if 0.0 > l50 {
        l50 = (x(l50) + x(TWO_PI)).to_f64();
    }
    let ratio = x(l50) / F80::from_i32(n);
    let l48 = ratio.to_f64();
    let pt = |m: &Mem, i: i32| m.u32(crv + 0x2c).wrapping_add(0x24u32.wrapping_mul(i as u32));
    let msg_a = 0x086d_6840;
    let msg_b = 0x086d_68a0;
    let msg_c = 0x086d_6900;
    let mut phi = f(b);
    if order == 4 {
        if n1 > 0 {
            for i in 0..n1 {
                let at = pt(m, i);
                let cs = trig::cos(phi.to_f64());
                m.set_f64(at, (cs * f(radius)).to_f64());
                let sn = trig::sin(phi.to_f64());
                m.set_f64(at + 8, (sn * f(radius)).to_f64());
                phi = phi - x(l48);
            }
        }
        let p0 = pt(m, 0);
        let w = x(m.f64(p0 + 0x18));
        let dx = (x(m.f64(p0)) / w).to_f32();
        let dy = (x(m.f64(p0 + 8)) / w).to_f32();
        let dz = (x(m.f64(p0 + 0x10)) / w).to_f32();
        let len = (((f(dy) * f(dy)) + f(dx) * f(dx)) + f(dz) * f(dz)).sqrt().to_f32();
        let ratio = x(l50) / F80::from_i32(n);
        if (x(TWO_PI) - ratio).abs().lt(x(TINY)) {
            let r = arc_fail(m, o, msg_a, slot);
            m.set_sp(sp0);
            return Ok(r);
        }
        let t1 = ratio * x(HALF);
        let l78 = t1.to_f64();
        let l70 = (f(len) * x(4.0)).to_f64();
        let cs = trig::cos(l78);
        let l80 = ((F80::ONE - cs) * x(l70)).to_f64();
        let sn = trig::sin(l78);
        let t2 = x(l80) / (sn * x(3.0));
        let tk = t2.to_f32();
        let (tkx, lenx) = (f(tk), f(len));
        let (px, py) = (x(m.f64(p0)), x(m.f64(p0 + 8)));
        let t0x = (((py * tkx) / lenx) + px).to_f32();
        let t0y = (py - (px * tkx) / lenx).to_f32();
        let pl = pt(m, l5c);
        let (lx, ly) = (x(m.f64(pl)), x(m.f64(pl + 8)));
        let t1x = (lx - (ly * tkx) / lenx).to_f32();
        let t1y = (((tkx * lx) / lenx) + ly).to_f32();
        let r = crate::cross_image::bessel_end_nurbs_crv(m, crv, 1, [t0x, t0y, 0.0, 1.0], [t1x, t1y, 0.0, 1.0]);
        si_nurbs::image::knots_crv_reparametrization(m, r)?;
        nurbs_crv_dispose(m, slot);
        m.set_sp(sp0);
        return Ok(r);
    }
    if order == 3 {
        let arg = (x(l50) / F80::from_i32(l5c)).to_f64();
        let mut l58 = trig::cos(arg).to_f64();
        if 0.0 > l58 {
            l58 = -l58;
        }
        if x(l58).lt(x(EPS_1E6)) {
            let r = arc_fail(m, o, msg_b, slot);
            m.set_sp(sp0);
            return Ok(r);
        }
        let (l90, l88) = (a as f64, radius as f64);
        let last = pt(m, l5c);
        m.set_f64(last, (trig::cos(l90) * x(l88)).to_f64());
        m.set_f64(last + 8, (trig::sin(l90) * x(l88)).to_f64());
        let l98 = b as f64;
        let first = pt(m, 0);
        m.set_f64(first, (trig::cos(l98) * x(l88)).to_f64());
        m.set_f64(first + 8, (trig::sin(l98) * x(l88)).to_f64());
        let l3c = n.wrapping_sub(1);
        let l48 = (x(l50) / F80::from_i32(l3c)).to_f64();
        let mut cur = a;
        let mut i = n1.wrapping_sub(2);
        while i > 0 {
            let at = pt(m, i);
            let l_a0 = cur as f64;
            let next = (x(l_a0) + x(l48)).to_f32();
            let l_a8 = trig::cos(l_a0).to_f64();
            let l_b0 = next as f64;
            let l_b8 = (trig::sin(l_b0) * x(l_a8)).to_f64();
            let sc = trig::sin(l_a0);
            let cn = trig::cos(l_b0);
            let d = x(l_b8) - cn * sc;
            if d.lt(x(EPS_1E6)) {
                let r = arc_fail(m, o, msg_c, slot);
                m.set_sp(sp0);
                return Ok(r);
            }
            let inv = F80::ONE / d;
            let w = x(l58);
            m.set_f64(at + 0x18, l58);
            let k = inv * (w * f(radius));
            let sn_n = trig::sin(l_b0);
            let sn_c = trig::sin(l_a0);
            m.set_f64(at, ((sn_n - sn_c) * k).to_f64());
            let cs_c = trig::cos(l_a0);
            let cs_n = trig::cos(l_b0);
            m.set_f64(at + 8, ((cs_c - cs_n) * k).to_f64());
            cur = next;
            i -= 1;
        }
        si_nurbs::image::knots_crv_reparametrization(m, crv)?;
        m.set_sp(sp0);
        return Ok(crv);
    }
    if n1 > 0 {
        for i in 0..n1 {
            let at = pt(m, i);
            let cs = trig::cos(phi.to_f64());
            m.set_f64(at, (cs * f(radius)).to_f64());
            let sn = trig::sin(phi.to_f64());
            m.set_f64(at + 8, (sn * f(radius)).to_f64());
            m.set_u32(at + 0x10, 0);
            m.set_u32(at + 0x14, 0);
            m.set_u32(at + 0x18, 0);
            m.set_u32(at + 0x1c, 0x3ff0_0000);
            phi = phi - x(l48);
        }
    }
    si_nurbs::image::knots_crv_reparametrization(m, crv)?;
    m.set_sp(sp0);
    Ok(crv)
}

/// A sphere as a NURBS surface [soft 0x08486acc SI_nurbsSphere]: the half circle from −π/2 to
/// π/2 of [`nurbs_arc`] (`radius`, order `order`, `segs` spans), the order of the arc other
/// than 2 giving it the step field 3, turned once about the Y axis by [`revolve_nurbs`]
/// (`steps` columns, spline type `spline_type - 1`, scaled, the last word 1 for type 2 and 3
/// otherwise) and the arc freed. The arc that could not be made gives 0.
#[allow(clippy::too_many_arguments)]
pub fn nurbs_sphere(m: &mut Mem, o: &mut dyn Outside, radius: f32, segs: i32, steps: i32, spline_type: i32, order: i32) -> Result<u32, si_nurbs::image::Halt> {
    let last = if spline_type == 2 { 1 } else { 3 };
    let sp = m.sp();
    let slot = m.local(4);
    m.set_u32(slot, 0);
    let arc = nurbs_arc(m, o, radius, -1.5707963705062866f32, 1.5707963705062866f32, order, segs)?;
    m.set_u32(slot, arc);
    if arc == 0 {
        m.set_sp(sp);
        return Ok(0);
    }
    if m.i32(arc) != 2 {
        m.set_u32(arc + 0x34, 3);
    }
    let s = revolve_nurbs(m, o, arc, steps, spline_type - 1, 6.2831855f32, 1, 0, 1, last);
    nurbs_crv_dispose(m, slot);
    m.set_sp(sp);
    Ok(s)
}
