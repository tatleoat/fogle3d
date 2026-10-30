// SPDX-License-Identifier: MIT
//! The revolution of a spline into a patch, on the one memory image [soft 0x081977b0 SI_splineRevolutionToPatch]: the
//! spline's keys are the first column of the patch's points and each following column is
//! the previous one turned by the step angle about an axis (a coordinate axis, or the
//! axis a model gives: a spline's first two keys or a NURBS curve's first two points).
//!
//! Checked against the original in the emulator (re/tools/si-spline/emu_gen8.py, vec8; 2,000
//! cases, all identical once the library's sin/cos results are given).

use si_math::algebra::{
    hvector_multiply_matrix, matrix_axis_rotation, matrix_x_rotation, matrix_y_rotation, matrix_z_rotation,
};
use si_math::types::{Hvector, Matrix, Vector};
use si_mem::{Mem, Ptr32};
use si_types::geom::{Patch, Spline};
use si_x87::{trig, x};

use crate::convert_image::{spline_bezier_expand, spline_multiply_definition};
use crate::intersect_image::{spline_copy, spline_dispose};
use crate::spline_image::Outside;

const IDENTITY_NONE: Matrix = [[0.0; 4]; 4];

/// The scale that keeps a revolved profile's outline round in a rational-less patch:
/// `3.0 / (cos(step) + 2.0)` in the register, rounded where stored.
fn cos_scale(step: f32) -> f32 {
    (x(3.0f64) / (trig::cos(step as f64) + 2.0f64)).to_f32()
}

/// The turn about a point: the translation row of `r` set so that the point stays fixed.
/// The formula of `SI_correctTransOfRefMatrix` (0x08463e20, in `si-model`, which this crate
/// cannot call): row 3 is `p - p x R`, per component formed in the register, rounded once.
fn correct_trans(p: [f32; 3], r: &mut Matrix) {
    let nx = -x(p[0]);
    let e = |i: usize, j: usize| r[i][j];
    let v0 = x(p[0]) + ((nx * e(0, 0) - x(p[1]) * e(1, 0)) - x(p[2]) * e(2, 0));
    let v1 = ((nx * e(0, 1) - x(p[1]) * e(1, 1)) - x(p[2]) * e(2, 1)) + p[1];
    let v2 = x(p[2]) + ((nx * e(0, 2) - x(p[1]) * e(1, 2)) - x(p[2]) * e(2, 2));
    r[3][0] = v0.to_f32();
    r[3][1] = v1.to_f32();
    r[3][2] = v2.to_f32();
}

/// The axis normalisation of both model branches: length `sqrt((x^2 + y^2) + z^2)` in the
/// register, stored as `float`; when its magnitude exceeds 1e-4 (a double) each component is
/// divided by it in the register and stored as `float`.
fn normalise(v: [f32; 3]) -> [f32; 3] {
    let len = (((x(v[0]) * v[0] + x(v[1]) * v[1]) + x(v[2]) * v[2]).sqrt()).to_f32();
    if (len.abs() as f64) > 1e-4 {
        [(x(v[0]) / len).to_f32(), (x(v[1]) / len).to_f32(), (x(v[2]) / len).to_f32()]
    } else {
        v
    }
}

fn rotation(axis: i32, angle: f32, v: &Vector) -> Matrix {
    match axis {
        0 => matrix_x_rotation(angle),
        1 => matrix_y_rotation(angle),
        2 => matrix_z_rotation(angle),
        _ => matrix_axis_rotation(angle, v),
    }
}

/// `SI_splineRevolutionToPatch(spline, type, steps, angle, axis, model, compensate, def)`
/// [soft 0x081977b0 SI_splineRevolutionToPatch].
///
/// Rounding points (read from the disassembly, then confirmed by the check above):
/// - x87 rounding points were taken from the instruction sequence; the one that is least
///   certain is the step-angle pair of the type 2/3 branch (here: the product stored as
///   `float`, the quotient `q` kept in the register, `-q / (steps + 2)` kept in the register
///   and used both for the step (stored) and for `q + that` (the start angle, stored));
/// - `cos` is `trig::cos` (the program calls libm's `cos` through the PLT with the step
///   widened to a double); the result is used as returned, as an extended number;
/// - the divisions of the axis normalisation are done in the register on the stored
///   `float` operands and rounded once on storing (double rounding possible, as the original).
///
/// Where the original leaves locals uninitialised the port uses zeros (the generator must avoid
/// these): a small angle with a type other than 0, 2, 3 (start angle, step, column count);
/// an axis of 3 or 4 with a null model or a model of kind other than 6 or 9 (axis and
/// pivot); an axis above 4 (the rotation matrix). Leaks, as the original: the copied spline
/// and the NURBS copy; the patch when the spline has no keys (null is returned).
///
/// `axis`: 0, 1, 2 the coordinate axes, 3 and 4 the model's. `compensate` is the short
/// before the last argument; `def` is stored as a 16-bit word at +0x1c.
#[allow(clippy::too_many_arguments)]
pub fn spline_revolution_to_patch(
    m: &mut Mem,
    o: &mut dyn Outside,
    spline: Ptr32<Spline>,
    kind: i32,
    steps: i16,
    angle: f32,
    axis: i32,
    model: u32,
    compensate: i16,
    def: i32,
) -> Ptr32<Patch> {
    let orig = spline;
    let mut spline = spline;
    let mut copied_spline: Ptr32<Spline> = Ptr32::NULL;
    let mut nurbs_copy: u32 = 0;
    let mut profile_bezier = false;
    if m[orig].kind == 1 {
        spline = spline_bezier_expand(m, orig);
        profile_bezier = true;
    }
    let patch = crate::patch_image::patch_allocate(m);
    if !patch.is_null() {
        let nb = |m: &Mem| m.u16(spline.addr().wrapping_add(0x10)) as u32;
        if nb(m) == 0 {
            return Ptr32::NULL; // leaks the patch (and an expanded spline), as the original
        }
        // Start angle, step angle, number of columns, closed flag.
        let (mut start, mut step, mut cols, mut closed) = (0.0f32, 0.0f32, 0i32, 0i16);
        // The large branch is taken only when |angle| >= the double 0x401921e11d6141fc (a bit
        // under 2 pi), compared and ordered (the status bits tested are C0 and C2): a NaN angle
        // takes the small branch.
        let large = angle.abs() as f64 >= f64::from_bits(0x4019_21e1_1d61_41fc);
        if !large {
            if kind == 0 {
                start = angle;
                step = ((-x(angle)) / x(steps as i32)).to_f32();
                cols = steps as i32 + 1;
            } else if kind == 2 || kind == 3 {
                cols = steps as i32 + 3;
                let n2 = x(steps as i32 + 2);
                let a = (x(angle) * n2).to_f32();
                let q = x(a) / x(steps as i32);
                let d = (-q) / n2;
                step = d.to_f32();
                start = (q + d).to_f32();
            }
        } else {
            start = f32::from_bits(0x40c9_0fdb); // 2 pi as a float
            step = (x(f32::from_bits(0xc0c9_0fdb)) / x(steps as i32)).to_f32();
            closed = 1;
            if angle < 0.0 {
                step = -step;
            }
            cols = steps as i32;
        }
        let n = nb(m);
        let total = n.wrapping_mul(cols as u32);
        let pts = m.si_calloc(total, 0x10);
        m[patch].points = Ptr32::new(pts);
        let tags = m.si_calloc(total, 2);
        m[patch].tags = Ptr32::new(tags);
        let mut c = total;
        while c != 0 {
            m.set_u16(tags.wrapping_add(c.wrapping_mul(2)).wrapping_sub(2), 0);
            c -= 1;
        }
        // Scale factors that round off the outline when `compensate` and the type is 2.
        let (mut sx1, mut sy, mut sx2) = (1.0f32, 1.0f32, 1.0f32);
        if compensate != 0 && kind == 2 {
            match axis {
                1 => sx1 = cos_scale(step),
                0 => sy = cos_scale(step),
                2 => sx2 = cos_scale(step),
                _ => {}
            }
        }
        // First column: the spline's key positions.
        let keys = |m: &Mem| m.u32(spline.addr().wrapping_add(0x14));
        let mut i = 0u32;
        loop {
            let k = keys(m).wrapping_add(i.wrapping_mul(0x30));
            let at = pts.wrapping_add(i.wrapping_mul(0x10));
            let px = m.f32(k.wrapping_add(0x0c));
            let py = m.f32(k.wrapping_add(0x10));
            m.set_f32(at, ((x(sx1) * px) * sx2).to_f32());
            m.set_f32(at.wrapping_add(4), (x(sy) * py).to_f32());
            let pz = m.u32(k.wrapping_add(0x14));
            m.set_u32(at.wrapping_add(8), pz);
            m.set_u32(at.wrapping_add(0xc), 0x3f80_0000);
            i += 1;
            if (i as i32) >= nb(m) as i32 {
                break;
            }
        }

        // The model's axis (types 3 and 4).
        let mut ax = [0.0f32; 3];
        let mut pivot = [0.0f32; 3];
        let mut mat = IDENTITY_NONE;
        let mut mk = 0u32; // 6: spline axis, 9: NURBS axis, else none
        let mut first_two = true; // the original's bVar4: keys 0,1 (else 1,2)
        let axis_vec = |ax: [f32; 3]| Vector::new(ax[0], ax[1], ax[2]);
        let spline_pivot = |m: &Mem, cs: Ptr32<Spline>, first_two: bool| -> [f32; 3] {
            let k = m.u32(cs.addr().wrapping_add(0x14)).wrapping_add(if first_two { 0 } else { 0x30 });
            [m.f32(k.wrapping_add(0x0c)), m.f32(k.wrapping_add(0x10)), m.f32(k.wrapping_add(0x14))]
        };
        let nurbs_pivot = |m: &Mem, c: u32| -> [f32; 3] {
            let p = m.u32(c.wrapping_add(0x2c));
            let w = x(m.f64(p.wrapping_add(0x3c)));
            [
                (x(m.f64(p.wrapping_add(0x24))) / w).to_f32(),
                (x(m.f64(p.wrapping_add(0x2c))) / w).to_f32(),
                (x(m.f64(p.wrapping_add(0x34))) / w).to_f32(),
            ]
        };
        if axis == 3 || axis == 4 {
            if model != 0 {
                mk = m.u32(model.wrapping_add(0x30));
                if mk == 6 {
                    let def_spline: Ptr32<Spline> = Ptr32::new(m.u32(model.wrapping_add(0x48)));
                    copied_spline = spline_copy(m, def_spline);
                    spline_multiply_definition(m, o, copied_spline, model.wrapping_add(0x1c4));
                    let ck = m[copied_spline].kind;
                    if (ck as u32).wrapping_sub(2) < 2 && m[copied_spline].phantom != 0 {
                        first_two = false;
                    }
                    let k = m.u32(copied_spline.addr().wrapping_add(0x14));
                    let (a, b) = if first_two { (0u32, 0x30u32) } else { (0x30, 0x60) };
                    let mut v = [0.0f32; 3];
                    for j in 0..3u32 {
                        v[j as usize] = (x(m.f32(k.wrapping_add(b + 0x0c + 4 * j))) - m.f32(k.wrapping_add(a + 0x0c + 4 * j))).to_f32();
                    }
                    ax = normalise(v);
                    mat = matrix_axis_rotation(start, &axis_vec(ax));
                    pivot = spline_pivot(m, copied_spline, first_two);
                } else if mk == 9 {
                    let crv = m.u32(model.wrapping_add(0x48));
                    nurbs_copy = si_nurbs::image::crv_copy(m, crv);
                    si_nurbs::image::crv_apply_matrix(m, nurbs_copy, model.wrapping_add(0x1c4), 0);
                    let p = m.u32(nurbs_copy.wrapping_add(0x2c));
                    let d = |m: &Mem, a: u32, b: u32| (x(m.f64(p.wrapping_add(a))) - m.f64(p.wrapping_add(b))).to_f32();
                    let v = [d(m, 0x24, 0), d(m, 0x2c, 8), d(m, 0x34, 0x10)];
                    ax = normalise(v);
                    mat = matrix_axis_rotation(start, &axis_vec(ax));
                    pivot = nurbs_pivot(m, nurbs_copy);
                }
            }
            correct_trans(pivot, &mut mat);
        } else if (0..=2).contains(&axis) {
            mat = rotation(axis, start, &axis_vec(ax));
        }
        // Turn the first column by the start angle (in place).
        let cnt = nb(m) as u16 as i16;
        for k in 0..cnt as i32 {
            let at: Ptr32<[f32; 4]> = Ptr32::new(pts.wrapping_add((k as u32).wrapping_mul(0x10)));
            let v = hvector_multiply_matrix(&Hvector::from_array(m.read(at)), &mat);
            m.write(at, v.to_array());
        }
        // The step matrix.
        if axis == 3 || axis == 4 {
            mat = matrix_axis_rotation(step, &axis_vec(ax));
            if model != 0 {
                if mk == 6 {
                    pivot = spline_pivot(m, copied_spline, first_two);
                } else if mk == 9 {
                    pivot = nurbs_pivot(m, nurbs_copy);
                }
            }
            correct_trans(pivot, &mut mat);
        } else if (0..=2).contains(&axis) {
            mat = rotation(axis, step, &axis_vec(ax));
        }
        // Each further column is the previous one turned by the step matrix.
        let mut col = 1i32;
        while col < cols {
            for k in 0..cnt as i32 {
                let src: Ptr32<[f32; 4]> =
                    Ptr32::new(pts.wrapping_add(((col - 1) as u32).wrapping_mul(n).wrapping_add(k as u32).wrapping_mul(0x10)));
                let dst: Ptr32<[f32; 4]> =
                    Ptr32::new(pts.wrapping_add(n.wrapping_mul(col as u32).wrapping_add(k as u32).wrapping_mul(0x10)));
                let v = hvector_multiply_matrix(&Hvector::from_array(m.read(src)), &mat);
                m.write(dst, v.to_array());
            }
            col += 1;
        }
        // The header, written by offset as the original does.
        let pa = patch.addr();
        let sa = spline.addr();
        let w = m.u32(sa.wrapping_add(0x0c));
        m.set_u32(pa.wrapping_add(8), w);
        let w = m.u16(sa.wrapping_add(0x10));
        m.set_u16(pa.wrapping_add(0x0e), w);
        let w = m.u16(sa.wrapping_add(0x08));
        m.set_u16(pa.wrapping_add(0x12), w);
        let w = m.u32(sa.wrapping_add(0x1c));
        m.set_u32(pa.wrapping_add(0x18), w);
        m.set_u32(pa.wrapping_add(4), kind as u32);
        if kind == 3 {
            m.set_f32(pa.wrapping_add(0x14), 0.5);
        }
        m.set_u16(pa.wrapping_add(0x0c), cols as u16);
        m.set_u16(pa.wrapping_add(0x10), closed as u16);
        let w = m.u16(sa.wrapping_add(0x20));
        m.set_u16(pa.wrapping_add(0x1e), w);
        m.set_u16(pa.wrapping_add(0x1c), def as u16);
        crate::patch_eval_image::patch_update_matrix(m, patch);
    }
    if profile_bezier {
        // SI_splineDispose(&spline) through a stack slot.
        let sp = m.sp();
        let slot = m.local(4);
        m.set_u32(slot, spline.addr());
        spline_dispose(m, slot);
        m.set_sp(sp);
    }
    patch
}
