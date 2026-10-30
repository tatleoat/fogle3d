// SPDX-License-Identifier: MIT
//! A node's kinematic state from one evaluated frame to the next: `SI_modelKineStateCompute`
//! of `settings.c` (the dynamics' settings file), which the animation update calls for a
//! node with a kinematic record (quick stretch, dynamics). It lives here because what it
//! reads is the node's transformation and `SI_modelGlobalState`.
//!
//! The record at `__SI_Model` + 0x2f0 (`kine_info`; layout read from this routine, 0x60
//! bytes): +0x00 the position, +0x0c the orientation (a 3 × 3 matrix, rows of three
//! floats), +0x30 the velocity, +0x3c the acceleration, +0x48 the angular velocity, +0x54
//! the angular acceleration, all floats.

use si_math::algebra as a;
use si_math::matrix3tools as m3;
use si_math::types::Matrix3;
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::x;

use super::{at, g, mat};

/// The double at 0x08660b58: π. The angular velocity is the angle ÷ (π × the time step).
const PI: f64 = std::f64::consts::PI;

fn get3(m: &Mem, a: u32) -> Matrix3 {
    let mut r = [[0.0f32; 3]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, e) in row.iter_mut().enumerate() {
            *e = m.f32(a + (12 * i + 4 * j) as u32);
        }
    }
    r
}

fn put3(m: &mut Mem, a: u32, v: &Matrix3) {
    for (i, row) in v.iter().enumerate() {
        for (j, e) in row.iter().enumerate() {
            m.set_f32(a + (12 * i + 4 * j) as u32, *e);
        }
    }
}

/// Updates a node's kinematic record for a step of `dt` seconds
/// [soft 0x0849f7d8 SI_modelKineStateCompute].
///
/// Nothing for a node without a record; nor, while the 16-bit word `SI_glbAnim` + 8 is
/// set, for a node of algorithm 12, or of algorithm 2 that is a joint or whose chain
/// record (+0x304) exists with its 16-bit word at +0x38 zero. Otherwise, from the global
/// position and rotation (`SI_modelGlobalState`, which is handed one block for the scaling
/// and the angles):
///
/// - velocity = (position − old position) ÷ dt, each component in the register and stored
///   rounded; acceleration = (that **unrounded** velocity − old velocity) ÷ dt;
/// - the turn since the last frame is `grot` (its 3 × 3 corner) × (the old orientation)ᵀ,
///   as a quaternion and then as an axis and an angle (`SI_matrixToQuaternion`,
///   `SI_quaternionToAxisAngle`); the angular velocity is the axis × angle ÷ (π × dt), the
///   quotient in the register and each product rounded to a float; the angular
///   acceleration is (that float − the old one) ÷ dt;
/// - all of it is stored in the record, the position and the new orientation last.
pub fn kine_state_compute(m: &mut Mem, node: Ptr32<Model>, dt: f32) {
    let k = m[node].kine_info.addr();
    if k == 0 {
        return;
    }
    if m[g::SI_GLB_ANIM][4] != 0 {
        let algo = m[node].algo_type;
        if algo == 0xc {
            return;
        }
        if algo == 2 {
            if m[node].kind == 5 {
                return;
            }
            let chain = m[node].dyna_chn_vars.addr();
            if chain != 0 && m.i16(chain + 0x38) == 0 {
                return;
            }
        }
    }
    let pos = m.local(12);
    let scal_rot = m.local(12);
    super::setters::global_state(m, node, scal_rot, scal_rot, pos);
    let rotation = m3::matrix3_get(&mat(m, at!(node, grot)));
    let p = [m.f32(pos), m.f32(pos + 4), m.f32(pos + 8)];
    let mut velocity = [0.0f32; 3];
    let mut acceleration = [0.0f32; 3];
    for i in 0..3 {
        let v = (x(p[i]) - m.f32(k + 4 * i as u32)) / dt;
        velocity[i] = v.to_f32();
        acceleration[i] = ((v - m.f32(k + 0x30 + 4 * i as u32)) / dt).to_f32();
    }
    let old = m3::matrix3_transpose(&get3(m, k + 0xc));
    let turn = m3::matrix3_multiply(&rotation, &old);
    let q = a::matrix_to_quaternion(&turn);
    let (axis, angle) = a::quaternion_to_axis_angle(&q);
    let w = x(angle) / (x(PI) * dt);
    let omega = [(w * axis.x).to_f32(), (w * axis.y).to_f32(), (w * axis.z).to_f32()];
    let mut alpha = [0.0f32; 3];
    for i in 0..3 {
        alpha[i] = ((x(omega[i]) - m.f32(k + 0x48 + 4 * i as u32)) / dt).to_f32();
    }
    for i in 0..3u32 {
        m.set_f32(k + 0x3c + 4 * i, acceleration[i as usize]);
    }
    for i in 0..3u32 {
        m.set_f32(k + 0x30 + 4 * i, velocity[i as usize]);
    }
    for i in 0..3u32 {
        m.set_f32(k + 0x54 + 4 * i, alpha[i as usize]);
    }
    for i in 0..3u32 {
        m.set_f32(k + 0x48 + 4 * i, omega[i as usize]);
    }
    // (the position goes through the register on its way: `fld`, `fstp`)
    for i in 0..3u32 {
        m.set_f32(k + 4 * i, x(p[i as usize]).to_f32());
    }
    put3(m, k + 0xc, &m3::matrix3_copy(&rotation));
    m.release(24);
}
