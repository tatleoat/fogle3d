// SPDX-License-Identifier: MIT
//! The tangency constraint's search (`constraint.c`), on the shared memory image (finding
//! 0024): the direction of a path where a constrained object is, or at a place along the
//! path the constraint's data names. The orientation step that uses the answer is
//! `limits::tan_constr_map_direction` (first pass, old form).
//!
//! The path's own routines (`spline.c`, `nrb_geometry.c`, `eval.c`) are outside this
//! package and reached through [`Outside`]; the two matrices a segment is stepped with are
//! `si-spline`'s and `si-math`'s value functions.
//!
//! Rounding points are from the instruction listing (finding 0008): r32(…) is a store to
//! float, everything inside is in the x87 register in the order written.

use si_math::algebra::matrix_multiply;
use si_math::types::Matrix;
use si_mem::{record, Mem, Ptr32};
use si_spline::basis::{matrix_precision, spline_basis_matrix};
use si_types::geom::{Spline, SplineKey, Vector};
use si_types::model::Model;
use si_x87::{fmov, x};

use crate::norconstr::{offset_in_frame, through_grot};
use crate::outside::{
    si_nurbs_crv_value, si_nurbs_curve_length, si_nurbs_distance_to_factor, si_spline_distance_to_factor, si_spline_length, si_spline_nb_segment, si_spline_update_length, si_spline_value, Outside,
};

record! {
    /// The data of a tangency constraint (`tangent_data`; `si_tanConstrCreateData` makes
    /// it, first pass). The field meanings are finding 0017's but for `fixed`, read here.
    pub struct TangentData = "tangent_data", size 0x18 {
        0x00 set: i16, "set";
        0x02 fixed: i16, "not 0: the place on the path is `distance` and `point`, not `percent` of the length";
        0x04 percent: f32, "the place along the path in hundredths of its length";
        0x08 distance: f32, "the place as a distance along the path";
        0x0c point: Vector, "the path's point at that distance";
    }
}

/// Two thirds and one third as the binary has them (doubles at 0x0864ce78, 0x0864ce80).
const TWO_THIRDS: f64 = f64::from_bits(0x3fe5_5555_673a_a1bc);
const ONE_THIRD: f64 = f64::from_bits(0x3fd5_5555_318a_bc87);

/// The sample of a spline nearest to `p` [soft 0x081f3fb0 tanConstrFindClosePoint]:
/// `*out_len` gets the cumulated length at that sample (read from the spline's table at
/// +0x6c, which must have been built with the same `step`) and `*out_pos` the sample.
/// Neither is written when the spline has no segment.
///
/// Every segment is walked in `step` forward differences (one for a linear spline): the
/// segment's four control rows (fourth column 1) are multiplied by the precision matrix
/// times the type's basis, and each step adds row 2 to row 3, row 1 to row 2, row 0 to
/// row 1, every sum rounded to float; row 3 is the sample. A sample is nearer when its
/// distance to `p`, rounded to float, is below the smallest so far (100000 at first). A
/// Bézier segment's rows are the key, its outgoing handle, the next key's incoming handle
/// and the next key (the first key again after the last); when the next key marks the
/// segment as linear (+0x2a is 2) the two inner rows are 2/3 and 1/3 of the way instead.
/// The other types take four keys in a row, wrapping at the key count.
pub fn tan_constr_find_close_point(m: &mut Mem, o: &mut dyn Outside, spline: Ptr32<Spline>, step: i32, p: Vector, out_len: Ptr32<f32>, out_pos: Ptr32<Vector>) {
    let mut step = step;
    let mut best = 100000.0f32;
    let mut sample = 0i32;
    if m[spline].kind == 0 {
        step = 1;
    }
    let segments = si_spline_nb_segment(o, m, spline);
    if segments == 0 {
        return;
    }
    // the basis is a local the callee leaves as it found it for a type outside 0..=3
    let basis_at: Ptr32<Matrix> = Ptr32::new(m.local(0x40));
    let mut basis = m[basis_at];
    spline_basis_matrix(m[spline].kind, m[spline].tension, &mut basis);
    let precision = matrix_precision(step);
    let stepper = matrix_multiply(&precision, &basis);
    m.release(0x40);
    let mut rows: Matrix = [[0.0, 0.0, 0.0, 1.0]; 4];
    let mut first = 0i32;
    let mut segment = 0i32;
    let mut key: Ptr32<SplineKey> = m[spline].keys;
    while segment < segments {
        if m[spline].kind == 1 {
            let next = if segment == m[spline].nb_keys as u16 as i32 - 1 { m[spline].keys } else { m[spline].keys.add(segment + 1) };
            let (a, b) = (m[key].pos, m[next].pos);
            rows[0][..3].copy_from_slice(&[fmov(a.x), fmov(a.y), fmov(a.z)]);
            rows[3][..3].copy_from_slice(&[fmov(b.x), fmov(b.y), fmov(b.z)]);
            if m[next].segment == 2 {
                let (w, t) = (x(TWO_THIRDS), x(ONE_THIRD));
                rows[1][0] = (w * a.x + t * b.x).to_f32();
                rows[1][1] = (t * b.y + w * a.y).to_f32();
                rows[1][2] = (t * b.z + w * a.z).to_f32();
                rows[2][0] = (w * b.x + t * a.x).to_f32();
                rows[2][1] = (w * b.y + t * a.y).to_f32();
                rows[2][2] = (w * b.z + t * a.z).to_f32();
            } else {
                let (h, g) = (m[key].next, m[next].prev);
                rows[1][..3].copy_from_slice(&[fmov(h.x), fmov(h.y), fmov(h.z)]);
                rows[2][..3].copy_from_slice(&[fmov(g.x), fmov(g.y), fmov(g.z)]);
            }
        } else {
            for (j, row) in rows.iter_mut().enumerate() {
                // a key count of 0 here is a division by zero in the original
                let nb = m[spline].nb_keys as u16 as i32;
                let Some(index) = (first + j as i32).checked_rem(nb) else {
                    m.record_fault(0, 0, si_mem::Access::Read);
                    return;
                };
                let k = m[m[spline].keys.add(index)].pos;
                row[..3].copy_from_slice(&[fmov(k.x), fmov(k.y), fmov(k.z)]);
            }
        }
        segment += 1;
        key = key.add(1);
        let mut d = matrix_multiply(&stepper, &rows);
        let mut look = |m: &mut Mem, d: &Matrix, sample: i32| {
            let (dx, dy, dz) = (x(p.x) - d[3][0], x(p.y) - d[3][1], x(p.z) - d[3][2]);
            let dist = (dz * dz + (dy * dy + dx * dx)).sqrt().to_f32();
            if x(dist).lt(x(best)) {
                let len = m[m[spline].lengths.cast::<f32>().add(sample)];
                m[out_len] = fmov(len);
                m[out_pos] = Vector { x: fmov(d[3][0]), y: fmov(d[3][1]), z: fmov(d[3][2]) };
                best = dist;
            }
        };
        if sample == 0 {
            look(m, &d, 0);
            sample = 1;
        }
        let mut s = 0;
        while s < step {
            s += 1;
            for r in (1..4).rev() {
                for c in 0..4 {
                    d[r][c] = (x(d[r][c]) + d[r - 1][c]).to_f32();
                }
            }
            look(m, &d, sample);
            sample += 1;
        }
        first += 1;
    }
}

/// The direction a tangency constraint aims `node` along: the path's direction at the
/// point nearest to the node [soft 0x081f449c si_tanConstrComputeTanVector]. Answers 1
/// and writes `out`; 0 when `path` is null, not a spline (kind 6) or without a definition.
///
/// The node's place goes into the path's space ([`offset_in_frame`]). The path's length is
/// asked first (rounded to float), then its length table is rebuilt at 100 samples a
/// segment and the nearest sample found ([`tan_constr_find_close_point`]). A second point
/// is taken 0.001 further along, or 0.001 back when the sample's length is within 0.0001
/// of the whole length, and the direction is the difference from the first point to the
/// one further along. It goes back through `grot` with the fourth row added
/// ([`through_grot`]). When the search writes nothing (a spline without segments) the
/// original goes on with what its stack held; here those words are the image's fill.
pub fn tan_constr_compute_tan_vector(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, path: Ptr32<Model>, out: Ptr32<Vector>) -> i32 {
    if path.is_null() || m[path].kind != 6 {
        return 0;
    }
    let spline: Ptr32<Spline> = m[path].definition.cast();
    if spline.is_null() {
        return 0;
    }
    let local = offset_in_frame(m, node, path);
    let whole = si_spline_length(o, m, spline).to_f32();
    si_spline_update_length(o, m, spline, 100, 1);
    let sp = m.sp();
    let at: Ptr32<f32> = Ptr32::new(m.local(4));
    let near: Ptr32<Vector> = Ptr32::new(m.local(12));
    let ahead: Ptr32<Vector> = Ptr32::new(m.local(12));
    tan_constr_find_close_point(m, o, spline, 100, local, at, near);
    let at_end = (x(m[at]) - whole).abs().lt(x(0.0001f64));
    let second = if at_end { (x(m[at]) - 0.001f32).to_f32() } else { (x(m[at]) + 0.001f32).to_f32() };
    let u = si_spline_distance_to_factor(o, m, spline, second).to_f32();
    si_spline_value(o, m, spline, u, ahead.addr());
    let (a, b) = (m[near], m[ahead]);
    let at_end = (x(m[at]) - whole).abs().lt(x(0.0001f64));
    m[out] = if at_end {
        Vector { x: (x(a.x) - b.x).to_f32(), y: (x(a.y) - b.y).to_f32(), z: (x(a.z) - b.z).to_f32() }
    } else {
        Vector { x: (x(b.x) - a.x).to_f32(), y: (x(b.y) - a.y).to_f32(), z: (x(b.z) - a.z).to_f32() }
    };
    m.set_sp(sp);
    through_grot(m, path, out);
    1
}

/// The point of a path at a distance, as the two branches of
/// [`tan_vector_from_path`] ask for it: a spline through `SI_splineDistanceToFactor` and
/// `SI_splineValue` (the parameter rounded to float between them), a NURBS curve through
/// `SI_nurbsDistanceToFactor` and `SI_nurbsCrvValue` (the distance widened to double, the
/// parameter rounded to double, the three doubles of the answer rounded to float). Not a
/// function of the original: the sequence is inlined twice.
fn path_point(m: &mut Mem, o: &mut dyn Outside, kind: i32, curve: Ptr32<si_mem::Opaque>, distance: f32) -> Vector {
    if kind == 6 {
        let at: Ptr32<Vector> = Ptr32::new(m.local(12));
        let u = si_spline_distance_to_factor(o, m, curve, distance).to_f32();
        si_spline_value(o, m, curve, u, at.addr());
        let v = m[at];
        m.release(12);
        v
    } else {
        // the structure the curve's evaluation returns: room for five doubles in the
        // original's frame, of which the first three are read
        let result = m.local(0x28);
        let u = si_nurbs_distance_to_factor(o, m, curve, distance as f64).to_f64();
        si_nurbs_crv_value(o, m, result, curve, u, 0);
        let v = Vector { x: x(m.f64(result)).to_f32(), y: x(m.f64(result + 8)).to_f32(), z: x(m.f64(result + 16)).to_f32() };
        m.release(0x28);
        v
    }
}

/// The direction of a path at the place a tangency constraint's data names
/// [soft 0x081f46d0 si_tanVectorFromPath]. Answers 1 and writes `out`; 0 when `data` is
/// null, the path has no definition, or it is neither a spline (kind 6) nor a NURBS curve
/// (kind 9).
///
/// L is the path's length rounded to float. The place is `data.distance` with the stored
/// point when `data.fixed` is set, otherwise r32((percent × 0.01) × L) and the path's
/// point there. A second point lies e further along, or at L − e when the place is at or
/// beyond L; e is L/10000 kept in the register, or the float 0.001 when that is below
/// 0.001. The direction runs from the first point to the one further along and goes
/// through `grot` with the fourth row added ([`through_grot`]).
pub fn tan_vector_from_path(m: &mut Mem, o: &mut dyn Outside, data: Ptr32<TangentData>, path: Ptr32<Model>, out: Ptr32<Vector>) -> i32 {
    if data.is_null() {
        return 0;
    }
    let curve = m[path].definition;
    if curve.is_null() {
        return 0;
    }
    let kind = m[path].kind;
    if kind != 6 && kind != 9 {
        return 0;
    }
    let whole = if m[path].kind == 6 { si_spline_length(o, m, curve) } else { si_nurbs_curve_length(o, m, curve) };
    let whole = whole.to_f32();
    let (first, place) = if m[data].fixed != 0 {
        let p = m[data].point;
        (Vector { x: fmov(p.x), y: fmov(p.y), z: fmov(p.z) }, fmov(m[data].distance))
    } else {
        let place = ((x(m[data].percent) * 0.01f64) * whole).to_f32();
        (path_point(m, o, m[path].kind, curve, place), place)
    };
    let mut e = x(whole) / 10000.0f32;
    if e.lt(x(0.001f64)) {
        e = x(f32::from_bits(0x3a83_126f));
    }
    let past = x(place).ge(x(whole));
    let second_at = if past { (x(whole) - e).to_f32() } else { (e + place).to_f32() };
    let second = path_point(m, o, m[path].kind, curve, second_at);
    m[out] = if past {
        Vector { x: (x(first.x) - second.x).to_f32(), y: (x(first.y) - second.y).to_f32(), z: (x(first.z) - second.z).to_f32() }
    } else {
        Vector { x: (x(second.x) - first.x).to_f32(), y: (x(second.y) - first.y).to_f32(), z: (x(second.z) - first.z).to_f32() }
    };
    through_grot(m, path, out);
    1
}
