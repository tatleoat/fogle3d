// SPDX-License-Identifier: MIT
//! The camera's viewing matrix and its up-vector constraint: `SI_camMat` (finding 0032,
//! "The viewing matrix").

use crate::globals::{cur_dict, SI_CID_MODEL, SI_FRAME_CURRENT, SI_FRAME_RATE, SI_IDENTITE, SI_Y_AXIS, SI_Z_AXIS};
use crate::outside::CamCalls;
use si_elemio::image::off::camera as cam;
use si_math::algebra as alg;
use si_math::types::{Matrix, Vector};
use si_mem::{Mem, Outside};
use si_x87::{fmov, x, F80};

/// 0x08645700, a double: 0.0001, the least squared distance between position and
/// interest, and the least |x| + |z| of the viewing direction that is not vertical.
pub const EPS: f64 = f64::from_bits(0x3f1a_36e2_eb1c_432d);
/// 0x08645708, a float: the up vector's z when the camera looks straight down.
pub const MINUS_ONE: f32 = -1.0;
/// 0x0864570c, a float: π/2, the angle of the two quarter turns.
pub const QUARTER_TURN: f32 = f32::from_bits(0x3fc9_0fdb);
/// Relation 1117 (0x45d): the camera's up vector follows a model.
pub const REL_CAMERA_UP_VECTOR: i32 = 0x45d;
/// The 16-bit word of a relation entry (`SI_ARelation`) that must be 1 for the constraint to
/// act (`active` in finding 0017's relation entry).
const REL_ACTIVE: u32 = 0x14;
/// A model's global translation (`__SI_Model` + 0x1b8).
const MODEL_GTRS: u32 = 0x1b8;

/// The sixteen floats of a matrix in the image.
pub fn read_matrix(m: &Mem, a: u32) -> Matrix {
    let mut out = [[0.0f32; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = f32::from_bits(m.u32(a.wrapping_add(16 * i as u32 + 4 * j as u32)));
        }
    }
    out
}

/// Stores the sixteen floats of a matrix (their bits as they are).
pub fn write_matrix(m: &mut Mem, a: u32, v: &Matrix) {
    for (i, row) in v.iter().enumerate() {
        for (j, f) in row.iter().enumerate() {
            m.set_u32(a.wrapping_add(16 * i as u32 + 4 * j as u32), f.to_bits());
        }
    }
}

fn vec3(m: &Mem, a: u32) -> [f32; 3] {
    [m.f32(a), m.f32(a.wrapping_add(4)), m.f32(a.wrapping_add(8))]
}

/// The difference of two points as the original forms it: each component subtracted in the
/// register and kept there unrounded (`fsub` of two floats), returned with the floats it
/// stores.
fn diff(a: [f32; 3], b: [f32; 3]) -> ([F80; 3], Vector) {
    let d = [x(a[0]) - b[0], x(a[1]) - b[1], x(a[2]) - b[2]];
    (d, Vector::new(d[0].to_f32(), d[1].to_f32(), d[2].to_f32()))
}

/// `a × b` with each component formed from the two exact products in the register and
/// rounded to float once it is stored: (a.y b.z − a.z b.y, a.z b.x − a.x b.z,
/// a.x b.y − a.y b.x). The three cross products of `SI_camMat` are all compiled to this.
fn cross(a: &Vector, b: [f32; 3]) -> Vector {
    Vector::new(
        (x(a.y) * b[2] - x(a.z) * b[1]).to_f32(),
        (x(a.z) * b[0] - x(a.x) * b[2]).to_f32(),
        (x(a.x) * b[1] - x(a.y) * b[0]).to_f32(),
    )
}

/// [soft 0x080d62d4 SI_camMat]: the camera's viewing matrix into `mat` (16 floats); on the
/// way, if the camera's up vector is constrained to a model (relation 1117) and the
/// constraint is active, the camera's roll (+0x30) is set from that model.
///
/// In the original's order, x87 as compiled (finding 0032 has the derivation):
///
/// 1. `R` = `SI_matrixZRotation(roll)` from the roll as the call finds it (a local).
/// 2. d = interest − position, each component kept in the register; stored as floats.
///    If not 0.0001 (a double) < dₓ² + d_y² + d_z² (the unrounded components; a NaN
///    fails): `mat` = `SI_identite` and only the translation is set (step 7).
/// 3. d is normalised (`SI_vectorNormalize`). The up vector u:
///    - if 0.0001 > |dₓ| + |d_z| (looking straight up or down): (0, 0, −1) when
///      0.0001 > d_y, `SI_zAxis` when d_y > 0.0001; d_y cannot equal the double 0.0001
///      (no float does), so the original's third case, u left unset, does not occur;
///    - otherwise the axis a = d × `SI_yAxis` is normalised, `mat` =
///      `SI_matrixAxisRotation(π/2, a)` and u = `SI_vectorMultiplyMatrix(d, mat)`: d
///      turned a quarter towards Y.
/// 4. The up-vector constraint: the camera's element (`SI_edGetElemFromCamera`), the model
///    element related to it by 1117 in list 1 (`SI_elemdictFindRelatedElem`), the
///    relation entry (`SI_elemdictGetCurRelation`) and its activation curve
///    (`SI_edGetFcvFromRelation`, asked for whether or not the entry is used). With an
///    entry whose word +0x14 is 1 and either no curve or `SI_fcurveIntegerValue(curve,
///    t, 0, 1)` > 0, t = float(`SI_frameCurrent`) / `SI_frameRate` rounded to float:
///    v = the model's global translation − position (stored as floats), normalised;
///    c = d × v, **not** normalised; `mat` = `SI_matrixAxisRotation(π/2, c)`; w = d × `mat`;
///    the roll becomes `SI_vectorAngle(u, w)`, negated when c · u < 0 (the dot product
///    (cₓuₓ + c_yu_y) + c_zu_z kept at 80 bits; a NaN is not negative).
/// 5. s = d × u. `mat` = `SI_identite`; rows 0, 1, 2 = s, u, −d.
/// 6. `mat` = `R` × `mat` (`SI_matrixMultiply`): the roll of step 1, not the one step 4 set.
/// 7. Row 3's first three floats = the position.
///
/// Every float the original moves through the unit is moved with [`fmov`] (a signalling
/// NaN arrives quiet). The intermediate matrices are written into `mat` and read back from
/// it, as the original does.
pub fn cam_mat(m: &mut Mem, o: &mut dyn Outside, camera: u32, mat: u32) {
    let roll = alg::matrix_z_rotation(fmov(m.f32(camera + cam::ROLL)));
    let (wide, mut d) = diff(vec3(m, camera + cam::INTEREST), vec3(m, camera + cam::POSITION));
    let len2 = (wide[1] * wide[1] + wide[0] * wide[0]) + wide[2] * wide[2];
    if x(EPS).lt(len2) {
        alg::vector_normalize(&mut d);
        let up: [f32; 3];
        if x(EPS).gt(x(d.x).abs() + x(d.z).abs()) {
            if x(EPS).gt(x(d.y)) {
                up = [0.0, 0.0, fmov(MINUS_ONE)];
            } else if x(d.y).gt(x(EPS)) {
                // copied as words (`mov`), not through the unit
                up = m.read(SI_Z_AXIS);
            } else {
                // d_y equal to the double 0.0001 or a NaN: unreachable (see above); the
                // original would leave u as its stack held it
                up = [0.0; 3];
            }
        } else {
            let mut axis = cross(&d, m.read(SI_Y_AXIS));
            alg::vector_normalize(&mut axis);
            write_matrix(m, mat, &alg::matrix_axis_rotation(fmov(QUARTER_TURN), &axis));
            let u = alg::vector_multiply_matrix(&d, &read_matrix(m, mat));
            up = [u.x, u.y, u.z];
        }
        up_vector_constraint(m, o, camera, mat, &d, up);
        let side = cross(&d, up);
        write_matrix(m, mat, &alg::matrix_copy(&m.read(SI_IDENTITE)));
        for (k, v) in [side.x, side.y, side.z].into_iter().enumerate() {
            m.set_u32(mat + 4 * k as u32, fmov(v).to_bits());
        }
        for (k, v) in up.into_iter().enumerate() {
            m.set_u32(mat + 0x10 + 4 * k as u32, fmov(v).to_bits());
        }
        for (k, v) in [d.x, d.y, d.z].into_iter().enumerate() {
            m.set_u32(mat + 0x20 + 4 * k as u32, (-x(v)).to_f32().to_bits());
        }
        let product = alg::matrix_multiply(&roll, &read_matrix(m, mat));
        write_matrix(m, mat, &product);
    } else {
        write_matrix(m, mat, &alg::matrix_copy(&m.read(SI_IDENTITE)));
    }
    // the position, copied as words into a local and from there through the unit
    let p = vec3(m, camera + cam::POSITION);
    for (k, v) in p.into_iter().enumerate() {
        m.set_u32(mat + 0x30 + 4 * k as u32, fmov(v).to_bits());
    }
}

/// Step 4 of [`cam_mat`]: the roll from the model the camera's up vector follows.
fn up_vector_constraint(m: &mut Mem, o: &mut dyn Outside, camera: u32, mat: u32, d: &Vector, up: [f32; 3]) {
    // two ints of the original's frame: the camera's index, then (reused) the related
    // element's chapter; the related element's index
    let index = m.local(4);
    let other = m.local(4);
    let elem = o.ed_get_elem_from_camera(m, camera, index);
    let related = if elem == 0 {
        0
    } else {
        let ed = cur_dict(m);
        let chap = m[SI_CID_MODEL];
        o.elemdict_find_related_elem(m, ed, elem, 1, 1, REL_CAMERA_UP_VECTOR, chap, index, other)
    };
    if related != 0 {
        // `fild`, `frndint` (exact on an integer whatever the rounding mode the original
        // sets for it), stored as a float, divided by the rate, stored as a float
        let frame = m[SI_FRAME_CURRENT] as f32;
        let time = (x(frame) / m[SI_FRAME_RATE]).to_f32();
        let ed = cur_dict(m);
        let rel = o.elemdict_get_cur_relation(m, ed, elem, 1, 1);
        if rel != 0 {
            let curve = o.ed_get_fcv_from_relation(m, rel);
            if m.i16(rel + REL_ACTIVE) == 1 && (curve == 0 || o.fcurve_integer_value(m, curve, time, 0, 1) > 0) {
                let model = m.ptr(related + 4);
                let (_, mut v) = diff(vec3(m, model + MODEL_GTRS), vec3(m, camera + cam::POSITION));
                alg::vector_normalize(&mut v);
                let c = cross(d, [v.x, v.y, v.z]);
                write_matrix(m, mat, &alg::matrix_axis_rotation(fmov(QUARTER_TURN), &c));
                let w = alg::vector_multiply_matrix(d, &read_matrix(m, mat));
                let dot = (x(c.x) * up[0] + x(c.y) * up[1]) + x(c.z) * up[2];
                let angle = alg::vector_angle(&Vector::new(up[0], up[1], up[2]), &w);
                m.set_u32(camera + cam::ROLL, angle.to_bits());
                if dot.lt(F80::ZERO) {
                    m.set_u32(camera + cam::ROLL, (-x(angle)).to_f32().to_bits());
                }
            }
        }
    }
    m.release(4);
    m.release(4);
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_mem::{Ret, Trace};

    fn camera(m: &mut Mem, pos: [f32; 3], int: [f32; 3], roll: f32) -> u32 {
        let c = m.malloc(cam::SIZE);
        for k in 0..3 {
            m.set_f32(c + cam::POSITION + 4 * k, pos[k as usize]);
            m.set_f32(c + cam::INTEREST + 4 * k, int[k as usize]);
        }
        m.set_f32(c + cam::ROLL, roll);
        c
    }

    #[test]
    fn constants_are_the_binarys() {
        assert_eq!(EPS, 1e-4);
        assert_eq!(QUARTER_TURN, std::f32::consts::FRAC_PI_2);
    }

    #[test]
    fn looking_down_minus_z_is_the_identity_rotation() {
        let mut m = Mem::new();
        crate::globals::init(&mut m);
        let c = camera(&mut m, [1.0, 2.0, 3.0], [1.0, 2.0, -7.0], 0.0);
        let mat = m.malloc(64);
        let mut o = Trace::new();
        cam_mat(&mut m, &mut o, c, mat);
        let r = read_matrix(&m, mat);
        // d = (0, 0, −1); u = d turned a quarter about d × Y = (1, 0, 0): (0, 1, 0)
        assert_eq!(r[2], [0.0, 0.0, 1.0, 0.0]);
        assert!((r[1][1] - 1.0).abs() < 1e-6 && (r[0][0] - 1.0).abs() < 1e-6);
        assert_eq!(&r[3][..3], &[1.0, 2.0, 3.0]);
        assert_eq!(o.names(), ["SI_edGetElemFromCamera"]);
    }

    #[test]
    fn too_close_gives_the_identity() {
        let mut m = Mem::new();
        crate::globals::init(&mut m);
        let c = camera(&mut m, [5.0, 5.0, 5.0], [5.0, 5.0, 5.005], 0.3);
        let mat = m.malloc(64);
        let mut o = Trace::new();
        cam_mat(&mut m, &mut o, c, mat);
        let r = read_matrix(&m, mat);
        assert_eq!(r[0], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(r[3], [5.0, 5.0, 5.0, 1.0]);
        assert!(o.calls.is_empty());
    }

    #[test]
    fn the_up_vector_sets_the_roll() {
        let mut m = Mem::new();
        crate::globals::init(&mut m);
        let c = camera(&mut m, [0.0, 0.0, 10.0], [0.0, 0.0, 0.0], 0.0);
        let mat = m.malloc(64);
        // a model at (1, 0, 10) + y: the up vector leans towards +x
        let model = m.malloc(0x320);
        m.set_f32(model + MODEL_GTRS, 1.0);
        m.set_f32(model + MODEL_GTRS + 4, 1.0);
        m.set_f32(model + MODEL_GTRS + 8, 10.0);
        let elem = m.malloc(0x60);
        m.set_u32(elem + 4, model);
        let rel = m.malloc(0x20);
        m.set_i16(rel + REL_ACTIVE, 1);
        let mut o = Trace::with(move |_, r, _| match r.name {
            "SI_edGetElemFromCamera" => Ret::eax(0x1234),
            "SI_elemdictFindRelatedElem" => Ret::eax(elem),
            "SI_elemdictGetCurRelation" => Ret::eax(rel),
            _ => Ret::NONE,
        });
        cam_mat(&mut m, &mut o, c, mat);
        assert_eq!(o.names(), ["SI_edGetElemFromCamera", "SI_elemdictFindRelatedElem", "SI_elemdictGetCurRelation", "SI_edGetFcvFromRelation"]);
        let roll = m.f32(c + cam::ROLL);
        assert!((roll.abs() - std::f32::consts::FRAC_PI_4).abs() < 1e-5, "{roll}");
    }
}
