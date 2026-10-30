// SPDX-License-Identifier: MIT
//! `dynamic.c` of Softimage|3D 4.0 (0x08496130 … 0x08499453): the simulation loop of the dynamics.
//!
//! Sources are functions of the Linux `soft` binary, cited as `[soft 0xADDRESS name]`. The floating-point
//! code was compiled for the x87 unit; compares are made on 80-bit values (every float here is exact in
//! them), stores round to f32.

use crate::outside::DynaCalls;
use si_mem::{Mem, Outside};
use si_x87::{x, F80};

/// The global `impactCleaningMode` (16 bits): which set of limits `si_vectorClean` takes.
const IMPACT_CLEANING_MODE: u32 = 0x0882_200c;

/// Cleans a vector of three floats [soft 0x08496130 si_vectorClean]: the limits (a lower one, below which
/// a component is set to 0, and an upper one, whose magnitude no component may exceed) come from the
/// kind (0..3, anything else takes the loosest pair) and `impactCleaningMode`; the float constants are
/// 1e-4 / 1e20 (kind 0), 1e-4 / 1e22 (kinds 1..3 with the mode on), 1e-6 / 1e10 (kinds 1..3 with
/// the mode off) and 0 / 1e25. NaN components pass every test.
pub fn si_vector_clean(m: &mut Mem, v: u32, kind: i32) {
    let mode = m.u16(IMPACT_CLEANING_MODE) != 0;
    let (lo, hi) = match (kind, mode) {
        (1..=3, true) => (m.f32(0x0866_093c), m.f32(0x0866_0940)),
        (1..=3, false) => (m.f32(0x0866_0948), m.f32(0x0866_094c)),
        (0, _) => (m.f32(0x0866_093c), m.f32(0x0866_0944)),
        _ => (0.0f32, m.f32(0x0866_0938)),
    };
    let (lo, hi) = (x(lo), x(hi));
    for k in 0..3u32 {
        // the fabs of the component is compared with the lower limit: less (ordered) clears it
        if x(m.f32(v + 4 * k)).abs().lt(lo) {
            m.set_u32(v + 4 * k, 0);
        }
    }
    for k in 0..3u32 {
        if hi.lt(x(m.f32(v + 4 * k))) {
            m.set_f32(v + 4 * k, hi.to_f32());
        }
    }
    let nhi = -hi;
    for k in 0..3u32 {
        if nhi.gt(x(m.f32(v + 4 * k))) {
            m.set_f32(v + 4 * k, nhi.to_f32());
        }
    }
}

/// Clears the accumulated rotation increments of a model chain [soft 0x08496f48 dynaSlowbandInit]: the walk starts at
/// the model itself when it has a parent link (+0x30), else at its first child (+0x2c4), and follows the sibling link
/// (+0x30c); each model's simulation record (+0x300) gets its three words at +0x1a4 / +0x1a8 / +0x1ac set to 0.
pub fn dyna_slowband_init(m: &mut Mem, model: u32) {
    let mut p = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
    while p != 0 {
        let rec = m.u32(p + 0x300);
        m.set_u32(rec + 0x1ac, 0);
        m.set_u32(rec + 0x1a8, 0);
        m.set_u32(rec + 0x1a4, 0);
        p = m.u32(p + 0x30c);
    }
}

fn read_m3(m: &Mem, a: u32) -> si_math::types::Matrix3 {
    let mut o = [[0.0f32; 3]; 3];
    for (i, row) in o.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = m.f32(a + 4 * (3 * i + j) as u32);
        }
    }
    o
}

fn write_m3(m: &mut Mem, a: u32, v: &si_math::types::Matrix3) {
    for (i, row) in v.iter().enumerate() {
        for (j, c) in row.iter().enumerate() {
            m.set_f32(a + 4 * (3 * i + j) as u32, *c);
        }
    }
}

/// Folds the angular-velocity increment of every body of a model's child list into its rotation matrix
/// [soft 0x084991e4 dynaSlowbandIntegrate]: for each child (first at +0x314, next at +0x310) with record
/// r = [child+0x300], the tilde matrix of the three floats at r+0x1a4 gets 1.0 added to its diagonal
/// (each sum rounded to a float), the rotation at r+0x1d8 is multiplied by it (rotation first) and the product is
/// made orthonormal back into r+0x1d8.
pub fn dyna_slowband_integrate(m: &mut Mem, model: u32) {
    use si_math::matrix3tools::{matrix3_multiply, matrix3_orthonormal, matrix3_tilde};
    let mut p = m.u32(model + 0x314);
    while p != 0 {
        let r = m.u32(p + 0x300);
        let v = si_math::types::Vector::new(m.f32(r + 0x1a4), m.f32(r + 0x1a8), m.f32(r + 0x1ac));
        let mut t = matrix3_tilde(v);
        for k in 0..3 {
            t[k][k] = (x(1.0f32) + x(t[k][k])).to_f32();
        }
        let prod = matrix3_multiply(&read_m3(m, r + 0x1d8), &t);
        let mut out = read_m3(m, r + 0x1d8);
        matrix3_orthonormal(&prod, &mut out);
        write_m3(m, r + 0x1d8, &out);
        p = m.u32(p + 0x310);
    }
}

/// The reaction force on a body from a contact velocity [soft 0x0849801c calculeReaction]: `v` is the
/// three-float velocity (by value), `out` receives three floats. The relative velocity l is the body's (via
/// [model+0x2f0] + 0x48..0x50), minus, when the model has a parent (+0x310 nonzero), the parent's velocity rotated by
/// the body record r = [model+0x300]'s matrix at r+0x1fc..0x21c (the parent is [r+0x25c]); each component of the
/// result is `(v * gain[model+0x2f8]) * l`, limited in magnitude by r+0xd8 / 0xdc / 0xe0 (the x component is
/// compared before rounding, the others after), negated, and set to 0 when l * limit is negative.
pub fn calcule_reaction(m: &mut Mem, model: u32, v: [f32; 3], out: u32) {
    let r = m.u32(model + 0x300);
    let parent_model = if r != 0 { m.u32(r + 0x25c) } else { 0 };
    let gain = m.u32(model + 0x2f8);
    let f = |m: &Mem, a: u32| x(m.f32(a));
    let l: [f32; 3];
    if m.u32(model + 0x310) == 0 {
        let d = m.u32(model + 0x2f0);
        l = [m.f32(d + 0x48), m.f32(d + 0x4c), m.f32(d + 0x50)];
    } else {
        let pd = m.u32(parent_model + 0x2f0);
        let (ax, ay, az) = (f(m, pd + 0x48), f(m, pd + 0x4c), f(m, pd + 0x50));
        let t1 = (f(m, r + 0x1fc) * ax + f(m, r + 0x200) * ay) + f(m, r + 0x204) * az;
        let t2 = (f(m, r + 0x208) * ax + f(m, r + 0x20c) * ay) + f(m, r + 0x210) * az;
        let t3 = az * f(m, r + 0x21c) + (ay * f(m, r + 0x218) + ax * f(m, r + 0x214));
        let d = m.u32(model + 0x2f0);
        l = [(f(m, d + 0x48) - t1).to_f32(), (f(m, d + 0x4c) - t2).to_f32(), (f(m, d + 0x50) - t3).to_f32()];
    }
    let o0 = (x(v[0]) * f(m, gain)) * x(l[0]);
    m.set_f32(out, o0.to_f32());
    m.set_f32(out + 4, ((x(v[1]) * f(m, gain + 4)) * l[1]).to_f32());
    m.set_f32(out + 8, ((x(v[2]) * f(m, gain + 8)) * l[2]).to_f32());
    // |limit| < |value| (ordered) replaces the value by the limit; the first value is still the 80-bit one
    if f(m, r + 0xd8).abs().lt(o0.abs()) {
        m.set_f32(out, m.f32(r + 0xd8));
    }
    if f(m, r + 0xdc).abs().lt(f(m, out + 4).abs()) {
        m.set_f32(out + 4, m.f32(r + 0xdc));
    }
    if f(m, r + 0xe0).abs().lt(f(m, out + 8).abs()) {
        m.set_f32(out + 8, m.f32(r + 0xe0));
    }
    for k in 0..3 {
        m.set_f32(out + 4 * k, (-f(m, out + 4 * k)).to_f32());
    }
    for k in 0..3u32 {
        if (x(l[k as usize]) * f(m, r + 0xd8 + 4 * k)).lt(x(0.0f32)) {
            m.set_u32(out + 4 * k, 0);
        }
    }
}

/// For every body of a model chain (the model itself when it has a parent link, else its children, walked by the
/// sibling link +0x30c), stores in its record r = [p+0x300] the body's offset from the model rotated into the
/// model's frame (r+0x244.. = the difference of the world positions at +0x1b8.. times the model's matrix at
/// +0x178..), the 3 x 3 upper left corner of its local matrix (+0x118, a 4 x 4) at r+0x1fc, the absolute scale
/// terms |+0x16c..| times +0x158.. at r+0x250.., and the transpose of the corner at r+0x1b4
/// [soft 0x08496c38 dynachnGetMatrices].
pub fn dynachn_get_matrices(m: &mut Mem, model: u32) {
    use si_math::matrix3tools::{matrix3_get, matrix3_transpose};
    let f = |m: &Mem, a: u32| x(m.f32(a));
    let mut p = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
    while p != 0 {
        let t1 = f(m, p + 0x1b8) - f(m, model + 0x1b8);
        let r = m.u32(p + 0x300);
        let t2 = f(m, p + 0x1bc) - f(m, model + 0x1bc);
        let t3 = f(m, p + 0x1c0) - f(m, model + 0x1c0);
        m.set_f32(r + 0x244, ((f(m, model + 0x178) * t1 + f(m, model + 0x17c) * t2) + f(m, model + 0x180) * t3).to_f32());
        m.set_f32(r + 0x248, ((f(m, model + 0x188) * t1 + f(m, model + 0x18c) * t2) + f(m, model + 0x190) * t3).to_f32());
        m.set_f32(r + 0x24c, ((t2 * f(m, model + 0x19c) + t1 * f(m, model + 0x198)) + t3 * f(m, model + 0x1a0)).to_f32());
        let mut src = [[0.0f32; 4]; 4];
        for (i, row) in src.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = m.f32(p + 0x118 + 4 * (4 * i + j) as u32);
            }
        }
        let g = matrix3_get(&src);
        write_m3(m, r + 0x1fc, &g);
        m.set_f32(r + 0x250, (f(m, p + 0x16c).abs() * f(m, p + 0x158)).to_f32());
        m.set_f32(r + 0x254, (f(m, p + 0x170).abs() * f(m, p + 0x15c)).to_f32());
        m.set_f32(r + 0x258, (f(m, p + 0x174).abs() * f(m, p + 0x160)).to_f32());
        let tr = matrix3_transpose(&read_m3(m, r + 0x1fc));
        write_m3(m, r + 0x1b4, &tr);
        p = m.u32(p + 0x30c);
    }
}

/// One explicit Euler step of the fast band [soft 0x08498f88 dynaFastbandIntegrate] (`dt` is the step): for each body of
/// the chain (as in [`dynachn_get_matrices`]) the velocity d+0x48 gains `dt` times the acceleration at d+0x54 and is
/// cleaned (kind 2), then the body record's rotation increment at r+0x1a4 gains `dt` times it; then, for the first
/// body only, the angular velocity d+0x30 gains `dt` times the torque terms from the rotation at r+0x1d8 / 0x1e4 / 0x1f0
/// and the body's d+0x3c, is cleaned, and (when the settings record [model+0x304]+0x38 is 0) the model's position
/// +0x158 gains `dt` times it; the settings flags at +0x3a / 0x48 / 0x4a / 0x4c clear the increments. Products stored
/// to a float local and read back are rounded to a float first (the x87 listing shows which).
pub fn dyna_fastband_integrate(m: &mut Mem, model: u32, dt: f32) {
    let f = |m: &Mem, a: u32| x(m.f32(a));
    let dt = x(dt);
    let first = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
    let mut p = first;
    while p != 0 {
        let d = m.u32(p + 0x2f0);
        let r = m.u32(p + 0x300);
        let t1 = dt * f(m, d + 0x54);
        let l8 = (dt * f(m, d + 0x58)).to_f32();
        let l4 = (dt * f(m, d + 0x5c)).to_f32();
        m.set_f32(d + 0x48, (t1 + f(m, d + 0x48)).to_f32());
        m.set_f32(d + 0x4c, (f(m, d + 0x4c) + x(l8)).to_f32());
        m.set_f32(d + 0x50, (f(m, d + 0x50) + x(l4)).to_f32());
        si_vector_clean(m, d + 0x48, 2);
        let t2 = dt * f(m, d + 0x48);
        let l8 = (dt * f(m, d + 0x4c)).to_f32();
        let l4 = (dt * f(m, d + 0x50)).to_f32();
        m.set_f32(r + 0x1a4, (t2 + f(m, r + 0x1a4)).to_f32());
        m.set_f32(r + 0x1a8, (f(m, r + 0x1a8) + x(l8)).to_f32());
        m.set_f32(r + 0x1ac, (f(m, r + 0x1ac) + x(l4)).to_f32());
        p = m.u32(p + 0x30c);
    }
    let p = first;
    let d = m.u32(p + 0x2f0);
    let r = m.u32(p + 0x300);
    let sett = m.u32(model + 0x304);
    let t3 = (f(m, r + 0x1dc) * f(m, d + 0x40) + f(m, r + 0x1d8) * f(m, d + 0x3c)) + f(m, r + 0x1e0) * f(m, d + 0x44);
    let t4 = (f(m, r + 0x1e4) * f(m, d + 0x3c) + f(m, r + 0x1e8) * f(m, d + 0x40)) + f(m, r + 0x1ec) * f(m, d + 0x44);
    let t6 = f(m, d + 0x44) * f(m, r + 0x1f8) + (f(m, d + 0x40) * f(m, r + 0x1f4) + f(m, d + 0x3c) * f(m, r + 0x1f0));
    let l14 = (t4 * dt).to_f32();
    let t5 = dt * t3;
    let l10 = (dt * t6).to_f32();
    m.set_f32(d + 0x30, (t5 + f(m, d + 0x30)).to_f32());
    m.set_f32(d + 0x34, (f(m, d + 0x34) + x(l14)).to_f32());
    m.set_f32(d + 0x38, (f(m, d + 0x38) + x(l10)).to_f32());
    si_vector_clean(m, d + 0x30, 2);
    let u6 = dt * f(m, d + 0x30);
    let u7 = dt * f(m, d + 0x34);
    let u8 = dt * f(m, d + 0x38);
    if m.i16(sett + 0x38) == 0 {
        m.set_f32(model + 0x158, (u6 + f(m, model + 0x158)).to_f32());
        m.set_f32(model + 0x15c, (u7 + f(m, model + 0x15c)).to_f32());
        m.set_f32(model + 0x160, (u8 + f(m, model + 0x160)).to_f32());
    }
    if m.i16(sett + 0x3a) != 0 {
        if m.i16(sett + 0x48) != 0 {
            m.set_u32(r + 0x1a4, 0);
        }
        if m.i16(sett + 0x4a) != 0 {
            m.set_u32(r + 0x1a8, 0);
        }
        if m.i16(sett + 0x4c) != 0 {
            m.set_u32(r + 0x1ac, 0);
        }
    }
}

/// [soft 0x08496d88 SI_dynachnPrepareSimulationData]: prepares a chain for a simulation: the matrices, the chain's
/// time step and constants, and for every body of the chain (as in [`dynachn_get_matrices`]) the joint defaults and
/// constants and the body's three turn counters `r+0x268 / 0x26c / 0x270`: `floor(|angle| / 2 pi + 0.5)` of its
/// rotation angles at `+0x10c / 0x110 / 0x114`, negative for a negative angle. The settings routines run here (finding 0035).
pub fn si_dynachn_prepare_simulation_data(m: &mut Mem, _o: &mut dyn Outside, model: u32) {
    dynachn_get_matrices(m, model);
    crate::settings::si_model_dyna_rel2abs_matrix_update(m, model);
    crate::settings::si_model_dyna_joints_update(m, model);
    let vars = m.u32(model + 0x304);
    crate::settings::si_dynachn_deltat_compute(m, vars);
    let rec = m.u32(model + 0x304);
    m.set_u32(rec + 0x28, 0);
    m.set_u32(rec + 0x24, 0);
    m.set_u32(rec + 0x20, 0);
    crate::settings_rec::si_dynachncte_compute(m, model, 0);
    let mut p = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
    while p != 0 {
        let jv = m.u32(p + 0x300);
        crate::settings_rec::si_dynajntvar_set_default(m, jv);
        crate::settings_rec::si_dynajntcte_compute(m, model, p, 0);
        let r = m.u32(p + 0x300);
        for (src, dst) in [(0x10cu32, 0x268u32), (0x110, 0x26c), (0x114, 0x270)] {
            let t = ((x(m.f32(p + src)).abs() / x(m.f64(0x0866_0960))) + x(m.f64(0x0866_0968))).to_f32();
            m.set_f32(r + dst, t.floor());
        }
        for (src, dst) in [(0x10cu32, 0x268u32), (0x110, 0x26c), (0x114, 0x270)] {
            if m.f32(p + src) < 0.0 {
                m.set_f32(r + dst, -m.f32(r + dst));
            }
        }
        p = m.u32(p + 0x30c);
    }
}

/// [soft 0x0849727c dynaEvaluateCollisions]: for each constraint record of the body `mdl2` (chain `+0x2fc`, next
/// link `+0x3c`) that is a collision (`+4` is 9, `+8` not 0), by its sub-kind (`+10`) the multiple-collision
/// resolution (1, also sets `impactCleaningMode` to 1), the sphere support (2) or the box support (3), every one
/// clearing `si_firstStepOfDynamicFrame` (0x086d7140); `dt` is the step and `v1`, `v2` the two result vectors.
pub fn dyna_evaluate_collisions(m: &mut Mem, o: &mut dyn Outside, mdl1: u32, mdl2: u32, dt: f32, v1: u32, v2: u32) {
    let mut c = m.u32(mdl2 + 0x2fc);
    while c != 0 {
        if m.i32(c + 4) == 9 && m.u16(c + 8) != 0 {
            match m.u16(c + 10) {
                1 => {
                    let jv = m.u32(mdl2 + 0x300);
                    crate::collisions::si_resolve_multiple_collision(m, o, mdl1, mdl2, jv, c, dt);
                    m.set_u16(0x086d_7140, 0);
                    m.set_u16(IMPACT_CLEANING_MODE, 1);
                }
                2 => {
                    let jv = m.u32(mdl2 + 0x300);
                    crate::contact::si_dyna_evaluate_sphere_support(m, mdl1, mdl2, jv, c, v1, v2);
                    m.set_u16(0x086d_7140, 0);
                }
                3 => {
                    let jv = m.u32(mdl2 + 0x300);
                    let pa = [m.u32(v1), m.u32(v1 + 4), m.u32(v1 + 8)];
                    let pb = [m.u32(v2), m.u32(v2 + 4), m.u32(v2 + 8)];
                    crate::contact_bbox::si_dyna_evaluate_bbox_support(m, mdl1, mdl2, jv, c, pa, pb);
                    m.set_u16(0x086d_7140, 0);
                }
                _ => {}
            }
        }
        c = m.u32(c + 0x3c);
    }
}

/// [soft 0x08499290 dynaSimulationStep]: one step `dt` of the simulation of a chain: the slow band is initialised and
/// taken inbound, the fast band inbound, the first body's `+0xe4 .. 0xec` cleared when the settings flag `+0x38` is
/// set, then the fast band outbound, both bands integrated and the matrices and joints brought back. The band passes
/// run here now (no hook is left).
pub fn dyna_simulation_step(m: &mut Mem, o: &mut dyn Outside, model: u32, dt: f32, flag: i16) {
    let first = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
    let rec = m.u32(first + 0x300);
    let sett = m.u32(model + 0x304);
    dyna_slowband_init(m, model);
    dyna_slowband_inbound(m, o, model, dt, flag as u16);
    dyna_fastband_inbound(m, o, model);
    if m.u16(sett + 0x38) != 0 {
        m.set_u32(rec + 0xec, 0);
        m.set_u32(rec + 0xe8, 0);
        m.set_u32(rec + 0xe4, 0);
    }
    dyna_fastband_outbound(m, model);
    dyna_fastband_integrate(m, model, dt);
    dyna_slowband_integrate(m, model);
    crate::settings::si_model_dyna_abs2rel_matrix_update(m, model);
    crate::settings::si_model_dyna_joints_update(m, model);
}

/// [soft 0x084968f8 environmentAnimationPreUpdate]: at the time `t` sets the first coordinate of the model's scale
/// vector from its animation: the first function curve of kind 1 found for the model (channel 100) is evaluated
/// (`SI_fcurveValue`, rounded to a float), put into the state read by `SI_modelState`, and the state set again; the
/// chain's constants are recomputed.
pub fn environment_animation_pre_update(m: &mut Mem, o: &mut dyn Outside, model: u32, t: f32) {
    let fr = m.local(0x3c);
    let ebp = fr + 0x3c;
    let elem = o.si_ed_get_elem_from_model(m, model, ebp - 0x28);
    if elem != 0 {
        let mut fcv = o.si_ed_find_fcv(m, elem, 1, 100);
        while fcv != 0 {
            if m.i32(fcv) == 1 {
                o.si_model_state(m, model, ebp - 0x18, ebp - 0xc, ebp - 0x24);
                let r = o.si_fcurve_value(m, fcv, t, 1);
                let st = r.st0.expect("SI_fcurveValue leaves its value in st0");
                let v = F80::from_bits(u64::from_le_bytes(st[0..8].try_into().unwrap()), u16::from_le_bytes([st[8], st[9]]));
                m.set_f32(ebp - 0x18, v.to_f32());
                o.si_model_half_set_state(m, model, ebp - 0x18, ebp - 0xc, ebp - 0x24);
                crate::settings_rec::si_dynachncte_compute(m, model, 0);
                break;
            }
            fcv = o.si_ed_find_next_fcv(m, elem, 1, 100);
        }
    }
    m.release(0x3c);
}

/// [soft 0x0849934c SI_dynaSimulationFrame]: runs the simulation frame by frame for the count at `[model+0x304]+0x10`
/// steps of the size at `+0xc`, starting at the time `t0`; each pass first reads the tablet and two buttons (101 and
/// 7; either one down stops the whole run, the keys are then not saved), pre-updates the environment animation,
/// takes the step (flag `+0x3e`) and updates the branch; the activity indicator is shown every fifth pass. When
/// `save` is not 0 the raw curve keys are saved at `t0 + t1` at the end.
pub fn si_dyna_simulation_frame(m: &mut Mem, o: &mut dyn Outside, model: u32, t0: f32, t1: f32, save: i16) {
    let sett = m.u32(model + 0x304);
    let count = m.u16(sett + 0x10) as i16;
    let end_time = (x(t0) + x(t1)).to_f32();
    let mut time = t0;
    let step = m.f32(sett + 0xc);
    m.set_u16(0x086d_7140, 1);
    m.set_u16(IMPACT_CLEANING_MODE, 0);
    o.si_show_activity(m);
    let mut i: i16 = 0;
    let mut s: i16 = 0;
    if i < count {
        loop {
            if m.u32(0x0867_93f8) != 0 {
                o.si_tablet_get_button_up_event(m);
            }
            if o.getbutton(m, 0x65) != 0 {
                return;
            }
            if m.u32(0x0867_94b4) == 0x65 {
                return;
            }
            if o.getbutton(m, 7) != 0 {
                return;
            }
            environment_animation_pre_update(m, o, model, time);
            let flag = m.u16(m.u32(model + 0x304) + 0x3e) as i16;
            dyna_simulation_step(m, o, model, step, flag);
            mymodel_branch_update(m, o, model);
            s = s.wrapping_add(1);
            if s == 5 {
                o.si_show_activity(m);
                s = 0;
            }
            time = (x(time) + x(step)).to_f32();
            i = i.wrapping_add(1);
            if !(i < count) {
                break;
            }
        }
    }
    if save != 0 {
        o.si_simulation_raw_fcrv_save_keys(m, model, end_time);
    }
}

/// [soft 0x084969a8 mymodelBranchUpdate]: after a step brings every body's rotation angles of the chain back from its
/// matrix: the body's 3 x 3 matrix is copied to its record (`+0x1fc`), the XYZ angles extracted, each turn counter
/// (`r+0x268 / 0x26c / 0x270`) is stepped by one when the angle lies more than pi from the stored angle less its
/// turns, and the angles `+0x10c / 0x110 / 0x114` are rebuilt as `2 pi * turns + angle` (for the first body of a
/// parentless chain only the axes whose settings flags `+0x48 / 0x4a / 0x4c` are 0, and only when `+0x3a` is set).
/// The sons of the model are updated at the end.
pub fn mymodel_branch_update(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let sett = m.u32(model + 0x304);
    let mut p = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
    let fr = m.local(0x50);
    let ebp = fr + 0x50;
    let ang = ebp - 0xc;
    let two_pi = x(m.f64(0x0866_0950));
    let pi = x(m.f64(0x0866_0958));
    while p != 0 {
        let r = m.u32(p + 0x300);
        o.si_matrix3_put(m, r + 0x1fc, p + 0x118);
        m.set_f32(ang, 0.0);
        m.set_f32(ang + 4, 0.0);
        m.set_f32(ang + 8, 0.0);
        o.si_matrix_extract_xyz_rotation(m, p + 0x118, ang);
        // axis k: stored angle at p+src, turns at r+turn, extracted angle at ang+4*k
        let axes = [(0x10cu32, 0x268u32, 0u32), (0x110, 0x26c, 4), (0x114, 0x270, 8)];
        for (src, turn, off) in axes {
            let a = (x(m.f32(p + src)) - two_pi * x(m.f32(r + turn))).to_f32();
            let e = m.f32(ang + off);
            let t = x(m.f32(r + turn));
            if (x(e) - x(a)).gt(pi) {
                m.set_f32(r + turn, (t - x(1.0f32)).to_f32());
            } else if pi.lt(x(a) - x(e)) {
                m.set_f32(r + turn, (x(1.0f32) + t).to_f32());
            }
        }
        let first = if m.u32(model + 0x30) == 0 { p == m.u32(model + 0x2c4) } else { p == model };
        let flags = [(0x48u32, 0x10cu32, 0x268u32, 0u32), (0x4a, 0x110, 0x26c, 4), (0x4c, 0x114, 0x270, 8)];
        if first && m.u16(sett + 0x3a) != 0 {
            for (fl, dst, turn, off) in flags {
                if m.u16(sett + fl) == 0 {
                    let v = (x(m.f32(r + turn)) * two_pi + x(m.f32(ang + off))).to_f32();
                    m.set_f32(p + dst, v);
                }
            }
        } else {
            for (_, dst, turn, off) in flags {
                let v = (two_pi * x(m.f32(r + turn)) + x(m.f32(ang + off))).to_f32();
                m.set_f32(p + dst, v);
            }
        }
        p = m.u32(p + 0x30c);
    }
    m.release(0x50);
    let son = m.u32(model + 0x2bc);
    o.si_model_update_son_state(m, son, model, 1);
}

/// [soft 0x08496f94 dynaEvaluateConstraints]: walks the constraint records of the body `mdl2` (chain `+0x2fc`, next
/// link `+0x3c`) of the chain `mdl1` and clears each one's two result vectors (`+0x14` and `+0x2c`), then by the
/// record's kind (`+4`): 0 and 10 add the constraint orientation of the record's object (`+0x10`) to a running sum
/// (kind 10 only while no kind 0 has been seen), average it over the count when the next record is missing or not
/// of kind 0, and store a changed sum to the settings (`[mdl1+0x304]+0x20..0x28`, then `SI_dynachncteCompute`);
/// 4 copies the vector of the settings (`+0x64`, else of `[mdl2+0x2f4]+0x20`) to `+0x20` and takes the orientation
/// into `+0x14`; 13 and 6 (while the settings short at `+0x40` is not 0) clear `+0x20..0x28` and call the fan and wind
/// forces; 7 (while `+8` is not 0) calls the constraint force. The others do nothing.
pub fn dyna_evaluate_constraints(m: &mut Mem, o: &mut dyn Outside, mdl1: u32, mdl2: u32) {
    let fr = m.local(0x5c);
    let ebp = fr + 0x5c;
    let out = ebp - 0x30;
    let mut flag = false;
    let mut c = m.u32(mdl2 + 0x2fc);
    let (mut sx, mut sy, mut sz) = (0.0f32, 0.0f32, 0.0f32);
    let mut count: i32 = 0;
    while c != 0 {
        for off in [0x1cu32, 0x18, 0x14, 0x34, 0x30, 0x2c] {
            m.set_u32(c + off, 0);
        }
        let obj = m.u32(c + 0x10);
        let kind = m.u32(c + 4);
        let model_arg = |m: &Mem| if m.u32(mdl1 + 0x2ec) == 2 { mdl1 } else { m.u32(mdl1 + 0x2bc) };
        match kind {
            0 | 10 if kind == 0 || !flag => {
                let b = model_arg(m);
                o.si_constraint_orientation_get(m, obj, b, 1, 0, 0x79, out);
                let ox = m.f32(out);
                let oy = m.f32(out + 4);
                let oz = m.f32(out + 8);
                let rx = x(sx) + x(ox);
                let ry = x(sy) + x(oy);
                let rz = x(sz) + x(oz);
                sx = rx.to_f32();
                sy = ry.to_f32();
                sz = rz.to_f32();
                count += 1;
                let next = m.u32(c + 0x3c);
                if next == 0 || m.u32(next + 4) != 0 {
                    if count > 1 {
                        let inv = x(1.0f32) / x(count);
                        sx = (rx * inv).to_f32();
                        sz = (rz * inv).to_f32();
                        sy = (ry * inv).to_f32();
                    }
                    let sett = m.u32(mdl1 + 0x304);
                    let same = x(m.f32(sett + 0x20)).eq(x(sx)) && x(m.f32(sett + 0x24)).eq(x(sy)) && x(m.f32(sett + 0x28)).eq(x(sz));
                    if !same {
                        m.set_f32(sett + 0x20, sx);
                        let sett = m.u32(mdl1 + 0x304);
                        m.set_f32(sett + 0x24, sy);
                        let sett = m.u32(mdl1 + 0x304);
                        m.set_f32(sett + 0x28, sz);
                        crate::settings_rec::si_dynachncte_compute(m, mdl1, 1);
                    }
                }
                if m.u32(c + 4) == 0 {
                    flag = true;
                }
            }
            4 => {
                let sett = m.u32(mdl2 + 0x304);
                let src = if sett != 0 { sett + 0x64 } else { m.u32(mdl2 + 0x2f4) + 0x20 };
                for k in [0u32, 4, 8] {
                    let w = m.u32(src + k);
                    m.set_u32(c + 0x20 + k, w);
                }
                let b = model_arg(m);
                o.si_constraint_orientation_get(m, obj, b, 1, 0, 0x79, c + 0x14);
            }
            13 | 6 => {
                let sett = m.u32(mdl1 + 0x304);
                if m.u16(sett + 0x40) != 0 {
                    m.set_u32(c + 0x28, 0);
                    m.set_u32(c + 0x24, 0);
                    m.set_u32(c + 0x20, 0);
                    if kind == 13 {
                        crate::invdynamic_node::si_invdyna_get_fan_frce(m, o, mdl1, mdl2, c, c + 0x14, c + 0x2c);
                    } else {
                        crate::invdynamic_node::si_invdyna_get_wind_frce(m, o, mdl1, mdl2, c, c + 0x14, c + 0x2c);
                    }
                }
            }
            7 => {
                if m.u16(c + 8) != 0 {
                    crate::invdynamic::si_invdyna_get_constr_frce(m, o, mdl1, mdl2, c, c + 0x14);
                }
            }
            _ => {}
        }
        c = m.u32(c + 0x3c);
    }
    m.release(0x5c);
}

/// [soft 0x084981f8 dynaFastbandInbound]: for each body of the chain `[model+0x314]` (next link `+0x310`) with record
/// `r = [body+0x300]`: the linear force sum of the body's constraint children (the list at `[r+0x260]`, next link
/// `[[x+0x300]+0x264]`, only models of kind 5 with `+0x2ec` = 2 whose record has the short `+0x1b0` set) is rotated into
/// `r+0xd8`; the cross terms of those children are subtracted; the reaction (`calculeReaction`, when the body has a
/// successor or the settings short `+0x38` is set) and the children's moments are added into `r+0xd8 .. 0xf8`. The vectors
/// are cleaned with `si_vectorClean`.
pub fn dyna_fastband_inbound(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    let f = |m: &Mem, a: u32| x(m.f32(a));
    let _ = o;
    let mut fc = m.u32(model + 0x314);
    while fc != 0 {
        let r = m.u32(fc + 0x300);
        m.set_u32(r + 0x104, 0);
        m.set_u32(r + 0x100, 0);
        m.set_u32(r + 0xfc, 0);
        let (mut s24, mut s20, mut s1c) = (0.0f32, 0.0f32, 0.0f32);
        let first = |m: &Mem, fc: u32| {
            let rr = m.u32(fc + 0x300);
            if rr != 0 { m.u32(rr + 0x260) } else { 0 }
        };
        let step = |m: &Mem, xp: u32| {
            let rr = m.u32(xp + 0x300);
            if rr != 0 { m.u32(rr + 0x264) } else { 0 }
        };
        // is x a constraint child whose record counts (kind 5, +0x2ec = 2, +0x1b0 set)?
        let counts = |m: &Mem, xp: u32| m.u32(xp + 0x30) == 5 && m.u32(xp + 0x2ec) == 2 && m.u16(m.u32(xp + 0x300) + 0x1b0) != 0;
        let mut xp = first(m, fc);
        while xp != 0 {
            if counts(m, xp) {
                let w = m.u32(xp + 0x300);
                let t1 = ((f(m, w + 0x10) * f(m, w + 4)) + (f(m, w + 0xc) * f(m, w))) + (f(m, w + 0x14) * f(m, w + 8));
                let t2 = ((f(m, w + 0x18) * f(m, w)) + (f(m, w + 0x1c) * f(m, w + 4))) + (f(m, w + 0x20) * f(m, w + 8));
                let t3 = (f(m, w + 8) * f(m, w + 0x2c)) + ((f(m, w + 4) * f(m, w + 0x28)) + (f(m, w) * f(m, w + 0x24)));
                let t4 = t1 + f(m, w + 0xf0);
                let t5 = t2 + f(m, w + 0xf4);
                let t6 = t3 + f(m, w + 0xf8);
                let t7 = (f(m, w + 0x254) * t6) - (f(m, w + 0x258) * t5);
                let t8 = (f(m, w + 0x258) * t4) - (t6 * f(m, w + 0x250));
                let t9 = (t5 * f(m, w + 0x250)) - (t4 * f(m, w + 0x254));
                s24 = (t7 + x(s24)).to_f32();
                s20 = (t8 + x(s20)).to_f32();
                s1c = (t9 + x(s1c)).to_f32();
            }
            xp = step(m, xp);
        }
        let t10 = f(m, r + 0xc0) + x(s24);
        let t11 = f(m, r + 0xc4) + x(s20);
        let t12 = f(m, r + 0xc8) + x(s1c);
        m.set_f32(r + 0xd8, (((f(m, r + 0x54) * t10) + (f(m, r + 0x58) * t11)) + (f(m, r + 0x5c) * t12)).to_f32());
        m.set_f32(r + 0xdc, ((f(m, r + 0x68) * t12) + ((f(m, r + 0x64) * t11) + (f(m, r + 0x60) * t10))).to_f32());
        m.set_f32(r + 0xe0, ((t12 * f(m, r + 0x74)) + ((t11 * f(m, r + 0x70)) + (t10 * f(m, r + 0x6c)))).to_f32());
        si_vector_clean(m, r + 0xd8, 1);
        let (mut s18, mut s14, mut s10) = (0.0f32, 0.0f32, 0.0f32);
        let mut xp = first(m, fc);
        while xp != 0 {
            if counts(m, xp) {
                let w = m.u32(xp + 0x300);
                let t13 = ((f(m, w + 0x1b8) * f(m, w + 0x100)) + (f(m, w + 0x1b4) * f(m, w + 0xfc))) + (f(m, w + 0x1bc) * f(m, w + 0x104));
                let t14 = ((f(m, w + 0x1c0) * f(m, w + 0xfc)) + (f(m, w + 0x1c4) * f(m, w + 0x100))) + (f(m, w + 0x1c8) * f(m, w + 0x104));
                let t15 = (f(m, w + 0x104) * f(m, w + 0x1d4)) + ((f(m, w + 0x100) * f(m, w + 0x1d0)) + (f(m, w + 0xfc) * f(m, w + 0x1cc)));
                s18 = (t13 + x(s18)).to_f32();
                s14 = (t14 + x(s14)).to_f32();
                s10 = (t15 + x(s10)).to_f32();
            }
            xp = step(m, xp);
        }
        m.set_f32(r + 0xd8, (f(m, r + 0xd8) - x(s18)).to_f32());
        m.set_f32(r + 0xdc, (f(m, r + 0xdc) - x(s14)).to_f32());
        m.set_f32(r + 0xe0, (f(m, r + 0xe0) - x(s10)).to_f32());
        let sett = m.u32(model + 0x304);
        if m.u32(fc + 0x310) != 0 || m.u16(sett + 0x38) != 0 {
            let sett = m.u32(model + 0x304);
            let v = [m.f32(sett + 0x2c), m.f32(sett + 0x30), m.f32(sett + 0x34)];
            calcule_reaction(m, fc, v, r + 0xfc);
        }
        let a = f(m, r + 0xd8) + f(m, r + 0xfc);
        let b = f(m, r + 0xdc) + f(m, r + 0x100);
        let c = f(m, r + 0xe0) + f(m, r + 0x104);
        let e4 = (((f(m, r + 0x178) * c) - (f(m, r + 0x17c) * b)) + f(m, r + 0xcc)).to_f32();
        let ec = (((f(m, r + 0x174) * b) - (f(m, r + 0x178) * a)) + f(m, r + 0xd4)).to_f32();
        let e8 = (((f(m, r + 0x17c) * a) - (f(m, r + 0x174) * c)) + f(m, r + 0xd0)).to_f32();
        m.set_f32(r + 0xd8, a.to_f32());
        m.set_f32(r + 0xdc, b.to_f32());
        m.set_f32(r + 0xe0, c.to_f32());
        m.set_f32(r + 0xe4, e4);
        m.set_f32(r + 0xec, ec);
        m.set_f32(r + 0xe8, e8);
        let mut xp = first(m, fc);
        while xp != 0 {
            if counts(m, xp) {
                let w = m.u32(xp + 0x300);
                let t16 = (f(m, w + 0x254) * f(m, r + 0xe0)) - (f(m, w + 0x258) * f(m, r + 0xdc));
                let t17 = (f(m, w + 0x258) * f(m, r + 0xd8)) - (f(m, w + 0x250) * f(m, r + 0xe0));
                let t18 = (f(m, w + 0x250) * f(m, r + 0xdc)) - (f(m, w + 0x254) * f(m, r + 0xd8));
                let t19 = f(m, w) - t16;
                let t20 = f(m, w + 4) - t17;
                let t21 = f(m, w + 8) - t18;
                let t22 = ((f(m, w + 0xc) * t19) + (f(m, w + 0x10) * t20)) + (f(m, w + 0x14) * t21);
                let t23 = ((f(m, w + 0x18) * t19) + (f(m, w + 0x1c) * t20)) + (f(m, w + 0x20) * t21);
                let t24 = (t21 * f(m, w + 0x2c)) + ((t20 * f(m, w + 0x28)) + (t19 * f(m, w + 0x24)));
                let t25 = t22 + f(m, w + 0xf0);
                let t26 = (t23 + f(m, w + 0xf4)).to_f32();
                let t27 = (t24 + f(m, w + 0xf8)).to_f32();
                m.set_f32(r + 0xe4, (t25 + f(m, r + 0xe4)).to_f32());
                m.set_f32(r + 0xe8, (f(m, r + 0xe8) + x(t26)).to_f32());
                m.set_f32(r + 0xec, (f(m, r + 0xec) + x(t27)).to_f32());
            }
            xp = step(m, xp);
        }
        si_vector_clean(m, r + 0xe4, 1);
        let (e4, e8, ec) = (f(m, r + 0xe4), f(m, r + 0xe8), f(m, r + 0xec));
        m.set_f32(r + 0xf0, (((f(m, r + 0x1b8) * e8) + (f(m, r + 0x1b4) * e4)) + (f(m, r + 0x1bc) * ec)).to_f32());
        m.set_f32(r + 0xf4, ((f(m, r + 0x1c8) * ec) + ((f(m, r + 0x1c4) * e8) + (f(m, r + 0x1c0) * e4))).to_f32());
        m.set_f32(r + 0xf8, ((ec * f(m, r + 0x1d4)) + ((e8 * f(m, r + 0x1d0)) + (e4 * f(m, r + 0x1cc)))).to_f32());
        si_vector_clean(m, r + 0xf0, 1);
        fc = m.u32(fc + 0x310);
    }
}

/// A value stored as a float and loaded again.
fn r32(v: F80) -> F80 {
    x(v.to_f32())
}

/// `(a*x + b*y) + c*z` on 80-bit values, the association of the compiled sums of three products.
fn sum3(a: F80, b: F80, c: F80) -> F80 {
    (a + b) + c
}

/// Brings a model chain's fast band back out [soft 0x084988f4 dynaFastbandOutbound]: `p` is the model (or its first child
/// when the model has a parent link), `r = [p+0x300]` its simulation record, `s = [p+0x2f0]` the state record. With the
/// settings short `+0x38` clear the body's position state `s+0x3c..0x44` is minus the rotation-inverse of `r+0xe4..0xec`;
/// otherwise it comes from the model's own state, rotated by `r+0x220..0x240` when the settings short `+0x4e` is set
/// (zeros when not). The vector is cleaned, rotated by `r+0x78..0x98` and offset by `r+0xd8..0xe0` into `s+0x54..0x5c`; with
/// the short `+0x3a` set the velocities are replaced per axis (shorts `+0x48/4a/4c`). Then every child (next link `+0x30c`)
/// gets the same, the joint form (a child record's short `+0x1b0` set) built from the parent's state and the child's
/// vector `+0x250..0x258`.
pub fn dyna_fastband_outbound(m: &mut Mem, model: u32) {
    let f = |m: &Mem, a: u32| x(m.f32(a));
    let sett = m.u32(model + 0x304);
    let p = if m.u32(model + 0x30) == 0 { m.u32(model + 0x2c4) } else { model };
    let r = m.u32(p + 0x300);
    let s = m.u32(p + 0x2f0);
    // m (3 x 3, a copy of what SI_matrix3FastInverse makes of r+0x9c) times the three floats at `v`, summed as
    // (a + b) + c
    let inv_apply = |m: &Mem, r: u32, v: u32| -> [f32; 3] {
        let mut inv = read_m3(m, r + 0x9c);
        let a = inv;
        let _ = si_math::matrix3tools::matrix3_fast_inverse(&a, &mut inv);
        let (e0, e1, e2) = (f(m, v), f(m, v + 4), f(m, v + 8));
        let row = |k: usize| sum3(x(inv[k][0]) * e0, x(inv[k][1]) * e1, x(inv[k][2]) * e2).to_f32();
        [row(0), row(1), row(2)]
    };
    // rec(0x220..) times a vector of the state record `v`
    let rot220 = |m: &Mem, r: u32, v: u32| -> [f32; 3] {
        let (a0, a1, a2) = (f(m, v), f(m, v + 4), f(m, v + 8));
        [
            sum3(f(m, r + 0x220) * a0, f(m, r + 0x224) * a1, f(m, r + 0x228) * a2).to_f32(),
            sum3(f(m, r + 0x22c) * a0, f(m, r + 0x230) * a1, f(m, r + 0x234) * a2).to_f32(),
            sum3(a0 * f(m, r + 0x238), a1 * f(m, r + 0x23c), a2 * f(m, r + 0x240)).to_f32(),
        ]
    };
    if m.u16(sett + 0x38) == 0 {
        let v = inv_apply(m, r, r + 0xe4);
        m.set_f32(s + 0x3c, -v[0]);
        m.set_f32(s + 0x40, -v[1]);
        m.set_f32(s + 0x44, -v[2]);
    } else {
        let (a, b);
        if m.u16(sett + 0x4e) == 0 {
            a = [0.0f32; 3];
            b = [0.0f32; 3];
        } else {
            let st = m.u32(model + 0x2f0);
            a = rot220(m, r, st + 0x3c);
            b = [m.f32(st + 0x30), m.f32(st + 0x34), m.f32(st + 0x38)];
        }
        m.set_f32(s + 0x3c, a[0]);
        m.set_f32(s + 0x40, a[1]);
        m.set_f32(s + 0x44, a[2]);
        m.set_f32(s + 0x30, b[0]);
        m.set_f32(s + 0x34, b[1]);
        m.set_f32(s + 0x38, b[2]);
    }
    si_vector_clean(m, s + 0x3c, 3);
    let (v0, v1, v2) = (f(m, s + 0x3c), f(m, s + 0x40), f(m, s + 0x44));
    let l: [F80; 3] = [
        sum3(f(m, r + 0x78) * v0, f(m, r + 0x7c) * v1, f(m, r + 0x80) * v2),
        r32(sum3(f(m, r + 0x84) * v0, f(m, r + 0x88) * v1, f(m, r + 0x8c) * v2)),
        r32(sum3(v0 * f(m, r + 0x90), v1 * f(m, r + 0x94), v2 * f(m, r + 0x98))),
    ];
    m.set_f32(s + 0x54, (l[0] + f(m, r + 0xd8)).to_f32());
    m.set_f32(s + 0x58, (f(m, r + 0xdc) + l[1]).to_f32());
    m.set_f32(s + 0x5c, (f(m, r + 0xe0) + l[2]).to_f32());
    if m.u16(sett + 0x3a) != 0 {
        let (a, b);
        if m.u16(sett + 0x50) == 0 {
            a = [0.0f32; 3];
            b = [0.0f32; 3];
        } else {
            let st = m.u32(model + 0x2f0);
            a = rot220(m, r, st + 0x54);
            b = rot220(m, r, st + 0x48);
        }
        if m.u16(sett + 0x48) != 0 {
            m.set_f32(s + 0x54, a[0]);
            m.set_f32(s + 0x48, b[0]);
        }
        if m.u16(sett + 0x4a) != 0 {
            m.set_f32(s + 0x58, a[1]);
            m.set_f32(s + 0x4c, b[1]);
        }
        if m.u16(sett + 0x4c) != 0 {
            m.set_f32(s + 0x5c, a[2]);
            m.set_f32(s + 0x50, b[2]);
        }
    }
    si_vector_clean(m, s + 0x54, 3);
    let mut c = m.u32(p + 0x30c);
    while c != 0 {
        let q = m.u32(c + 0x300);
        let cs = m.u32(c + 0x2f0);
        if m.u16(q + 0x1b0) == 0 {
            let v = inv_apply(m, q, q + 0xe4);
            m.set_f32(cs + 0x3c, -v[0]);
            m.set_f32(cs + 0x40, -v[1]);
            m.set_f32(cs + 0x44, -v[2]);
        } else {
            let par = if q != 0 { m.u32(q + 0x25c) } else { 0 };
            let ps = m.u32(par + 0x2f0);
            let a = (f(m, q + 0x254) * f(m, ps + 0x5c)) - (f(m, q + 0x258) * f(m, ps + 0x58));
            let b = (f(m, q + 0x258) * f(m, ps + 0x54)) - (f(m, q + 0x250) * f(m, ps + 0x5c));
            let cc = (f(m, q + 0x250) * f(m, ps + 0x58)) - (f(m, q + 0x254) * f(m, ps + 0x54));
            let vx = (f(m, ps + 0x3c) - a) + f(m, q);
            let vy = (f(m, ps + 0x40) - b) + f(m, q + 4);
            let vz = (f(m, ps + 0x44) - cc) + f(m, q + 8);
            m.set_f32(cs + 0x3c, sum3(f(m, q + 0x1fc) * vx, f(m, q + 0x200) * vy, f(m, q + 0x204) * vz).to_f32());
            m.set_f32(cs + 0x40, sum3(f(m, q + 0x208) * vx, f(m, q + 0x20c) * vy, f(m, q + 0x210) * vz).to_f32());
            m.set_f32(cs + 0x44, sum3(vx * f(m, q + 0x214), vy * f(m, q + 0x218), vz * f(m, q + 0x21c)).to_f32());
        }
        si_vector_clean(m, cs + 0x3c, 3);
        let (v0, v1, v2) = (f(m, cs + 0x3c), f(m, cs + 0x40), f(m, cs + 0x44));
        let l: [F80; 3] = [
            sum3(f(m, q + 0x78) * v0, f(m, q + 0x7c) * v1, f(m, q + 0x80) * v2),
            r32(sum3(f(m, q + 0x84) * v0, f(m, q + 0x88) * v1, f(m, q + 0x8c) * v2)),
            r32(sum3(v0 * f(m, q + 0x90), v1 * f(m, q + 0x94), v2 * f(m, q + 0x98))),
        ];
        m.set_f32(cs + 0x54, (l[0] + f(m, q + 0xd8)).to_f32());
        m.set_f32(cs + 0x58, (f(m, q + 0xdc) + l[1]).to_f32());
        m.set_f32(cs + 0x5c, (f(m, q + 0xe0) + l[2]).to_f32());
        si_vector_clean(m, cs + 0x54, 3);
        c = m.u32(c + 0x30c);
    }
}

/// Adds the products `a*b + c*d` ... as the compiled sums of three terms were associated by the compiler in
/// `dynaSlowbandInbound`: the rotation of a vector `(v0, v1, v2)` by the nine floats at `r` (`+0x220 ..`); the first two
/// rows are `(a + b) + c`, the third `c + (b + a)`; all three stay on the 80-bit stack until stored.
fn rot_220(m: &Mem, r: u32, v: [F80; 3]) -> [F80; 3] {
    let f = |a: u32| x(m.f32(a));
    [
        (f(r + 0x224) * v[1] + f(r + 0x220) * v[0]) + f(r + 0x228) * v[2],
        (f(r + 0x22c) * v[0] + f(r + 0x230) * v[1]) + f(r + 0x234) * v[2],
        v[2] * f(r + 0x240) + (v[1] * f(r + 0x23c) + v[0] * f(r + 0x238)),
    ]
}

/// The slow band inbound pass [soft 0x0849739c dynaSlowbandInbound]: for every body `fc` of the chain (`[model+0x314]`,
/// next link `+0x310`) with record `r = [fc+0x300]`: the inertia sums `A` and `D` of its joint constraints (the list at
/// `[r+0x260]`, next `[[x+0x300]+0x264]`, only kind-5 models with `+0x2ec` = 2 whose record has the short `+0x1b0`
/// set: each gets its vector at `[q+0..8]` from the body's angular velocity `[state+0x48..0x50]` and `q`'s lever `+0x250`,
/// and the matrices `q+0xc`, `q+0x30`) give the inverse mass matrix `r+0x54`, the products `r+0x78`, `r+0x9c`
/// (cleaned with 1e-6); then the forces and moments of the body's constraint records (`[fc+0x2fc]`, next `+0x3c`) are summed
/// (with the settings' gravity when the short `+0x3c` is set), the collisions are detected and evaluated when `flag`
/// is not 0, and the results are stored to `r+0xcc..0xd4` (`si_vectorClean` kind 1) and `r+0xc0..0xc8`.
pub fn dyna_slowband_inbound(m: &mut Mem, o: &mut dyn Outside, model: u32, dt: f32, flag: u16) {
    use si_math::matrix3tools::{matrix3_add, matrix3_clean, matrix3_fast_inverse, matrix3_multiply, matrix3_sub};
    type M3 = si_math::types::Matrix3;
    const EPS: f32 = f32::from_bits(0x3586_37bd);
    const NULL: M3 = [[0.0; 3]; 3];
    const ID: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let fr = m.local(0x12c);
    let ebp = fr + 0x12c;
    let v1 = ebp - 0x30; // L_30, L_2c, L_28
    let v2 = ebp - 0x3c; // L_3c, L_38, L_34
    dyna_evaluate_constraints(m, o, model, model);
    let sett = m.u32(model + 0x304);
    let mut fc = m.u32(model + 0x314);
    while fc != 0 {
        let r = m.u32(fc + 0x300);
        let fc2f4 = m.u32(fc + 0x2f4);
        let s = m.u32(fc + 0x2f0);
        let mut a = NULL;
        let mut d = NULL;
        let head = if r != 0 { m.u32(r + 0x260) } else { 0 };
        let next = |m: &Mem, xp: u32| {
            let rr = m.u32(xp + 0x300);
            if rr != 0 { m.u32(rr + 0x264) } else { 0 }
        };
        let joint = |m: &Mem, xp: u32| m.u32(xp + 0x30) == 5 && m.u32(xp + 0x2ec) == 2 && m.u16(m.u32(xp + 0x300) + 0x1b0) != 0;
        let mut xp = head;
        while xp != 0 {
            if joint(m, xp) {
                let q = m.u32(xp + 0x300);
                let f = |m: &Mem, a: u32| x(m.f32(a));
                let c = (f(m, s + 0x4c) * f(m, q + 0x258) - f(m, s + 0x50) * f(m, q + 0x254)).to_f32();
                let t1 = f(m, s + 0x50) * f(m, q + 0x250) - f(m, s + 0x48) * f(m, q + 0x258);
                let l8 = t1.to_f32();
                let t2 = f(m, s + 0x48) * f(m, q + 0x254) - f(m, s + 0x4c) * f(m, q + 0x250);
                let l4 = t2.to_f32();
                m.set_f32(q, (t2 * f(m, s + 0x4c) - t1 * f(m, s + 0x50)).to_f32());
                m.set_f32(q + 4, (f(m, s + 0x50) * x(c) - f(m, s + 0x48) * x(l4)).to_f32());
                m.set_f32(q + 8, (f(m, s + 0x48) * x(l8) - f(m, s + 0x4c) * x(c)).to_f32());
                si_vector_clean(m, q, 3);
                let u = matrix3_multiply(&read_m3(m, q + 0x9c), &read_m3(m, q + 0x1fc));
                let mut mc = matrix3_multiply(&read_m3(m, q + 0x1b4), &u);
                matrix3_clean(&mut mc, EPS);
                write_m3(m, q + 0xc, &mc);
                let mut m30 = matrix3_multiply(&read_m3(m, q + 0x150), &mc);
                matrix3_clean(&mut m30, EPS);
                write_m3(m, q + 0x30, &m30);
                let t = matrix3_multiply(&m30, &read_m3(m, q + 0x150));
                a = matrix3_add(&t, &a);
                d = matrix3_add(&m30, &d);
            }
            xp = next(m, xp);
        }
        let src = if model == fc { read_m3(m, sett + 0x70) } else { read_m3(m, fc2f4 + 0x2c) };
        let t = matrix3_add(&src, &a);
        let mut inv = t;
        let _ = matrix3_fast_inverse(&t, &mut inv);
        write_m3(m, r + 0x54, &inv);
        let t = matrix3_sub(&d, &read_m3(m, r + 0x12c));
        let m78 = matrix3_multiply(&inv, &t);
        write_m3(m, r + 0x78, &m78);
        let m9c = matrix3_multiply(&read_m3(m, r + 0x12c), &m78);
        let mut m9c = matrix3_sub(&m9c, &read_m3(m, r + 0x108));
        let mut xp = head;
        while xp != 0 {
            if joint(m, xp) {
                let q = m.u32(xp + 0x300);
                let t = matrix3_multiply(&read_m3(m, q + 0x150), &m78);
                let t = matrix3_sub(&ID, &t);
                let u = matrix3_multiply(&read_m3(m, q + 0xc), &t);
                m9c = matrix3_add(&m9c, &u);
            }
            xp = next(m, xp);
        }
        matrix3_clean(&mut m9c, EPS);
        write_m3(m, r + 0x9c, &m9c);
        if m.u32(fc + 0x2ec) == 2 {
            dyna_evaluate_constraints(m, o, model, fc);
        }
        let f = |m: &Mem, a: u32| x(m.f32(a));
        for k in 0..3 {
            m.set_u32(v1 + 4 * k, 0);
        }
        let mut c = m.u32(fc + 0x2fc);
        while c != 0 {
            for k in 0..3u32 {
                let w = (f(m, v1 + 4 * k) + f(m, c + 0x14 + 4 * k)).to_f32();
                m.set_f32(v1 + 4 * k, w);
            }
            c = m.u32(c + 0x3c);
        }
        if m.u16(m.u32(model + 0x304) + 0x3c) != 0 {
            for k in 0..3u32 {
                let w = (f(m, v1 + 4 * k) + f(m, r + 0x180 + 4 * k)).to_f32();
                m.set_f32(v1 + 4 * k, w);
            }
        }
        for k in 0..3 {
            m.set_u32(v2 + 4 * k, 0);
        }
        // v2 = (L_3c, L_38, L_34) = word 0, 1, 2
        let mut c = m.u32(fc + 0x2fc);
        while c != 0 {
            let t3 = f(m, v2) + f(m, c + 0x2c);
            let l104 = t3.to_f32();
            m.set_f32(v2, l104);
            let t4 = f(m, v2 + 4) + f(m, c + 0x30);
            m.set_f32(v2 + 4, t4.to_f32());
            let t5 = f(m, v2 + 8) + f(m, c + 0x34);
            m.set_f32(v2 + 8, t5.to_f32());
            let w = [f(m, c + 0x14), f(m, c + 0x18), f(m, c + 0x1c)];
            let [t6, t7, t8] = rot_220(m, r, w);
            let t9 = f(m, c + 0x24) * t8 - f(m, c + 0x28) * t7;
            let t10 = f(m, c + 0x28) * t6 - t8 * f(m, c + 0x20);
            let t11 = t7 * f(m, c + 0x20) - t6 * f(m, c + 0x24);
            m.set_f32(v2 + 4, (t10 + t4).to_f32());
            let t12 = t9 + x(l104);
            m.set_f32(v2, t12.to_f32());
            m.set_f32(v2 + 8, (t11 + t5).to_f32());
            c = m.u32(c + 0x3c);
        }
        if m.u16(m.u32(model + 0x304) + 0x3c) != 0 {
            let g = m.u32(model + 0x304);
            let [t13, t14, t15] = {
                let (g0, g1, g2) = (f(m, g + 0x20), f(m, g + 0x24), f(m, g + 0x28));
                [
                    (f(m, r + 0x220) * g0 + f(m, r + 0x224) * g1) + f(m, r + 0x228) * g2,
                    (f(m, r + 0x22c) * g0 + f(m, r + 0x230) * g1) + f(m, r + 0x234) * g2,
                    g2 * f(m, r + 0x240) + (g1 * f(m, r + 0x23c) + g0 * f(m, r + 0x238)),
                ]
            };
            let t16 = f(m, r + 0x178) * t15 - f(m, r + 0x17c) * t14;
            let t17 = f(m, r + 0x17c) * t13 - t15 * f(m, r + 0x174);
            let t18 = t14 * f(m, r + 0x174) - t13 * f(m, r + 0x178);
            m.set_f32(v2, (t16 + f(m, v2)).to_f32());
            m.set_f32(v2 + 4, (t17 + f(m, v2 + 4)).to_f32());
            m.set_f32(v2 + 8, (t18 + f(m, v2 + 8)).to_f32());
        }
        if flag != 0 {
            crate::collisions::si_dyna_detect_collision(m, o, model, fc);
            let vv = [m.u32(v1), m.u32(v1 + 4), m.u32(v1 + 8)];
            crate::collisions::si_dyna_find_collision_state(m, o, model, fc, vv);
            dyna_evaluate_collisions(m, o, model, fc, dt, v1, v2);
        }
        let mut c = m.u32(fc + 0x2fc);
        while c != 0 {
            if m.i32(c + 4) == 9 {
                for k in 0..3u32 {
                    let w = (f(m, v1 + 4 * k) + f(m, c + 0x14 + 4 * k)).to_f32();
                    m.set_f32(v1 + 4 * k, w);
                }
                let w = [f(m, c + 0x14), f(m, c + 0x18), f(m, c + 0x1c)];
                let [t19, t20, t21] = rot_220(m, r, w);
                let t22 = f(m, c + 0x24) * t21 - f(m, c + 0x28) * t20;
                let t23 = f(m, c + 0x28) * t19 - t21 * f(m, c + 0x20);
                let t24 = t20 * f(m, c + 0x20) - t19 * f(m, c + 0x24);
                let t25 = t22 + f(m, v2);
                let t26 = t23 + f(m, v2 + 4);
                let t27 = t24 + f(m, v2 + 8);
                m.set_f32(v2, (t25 + f(m, c + 0x2c)).to_f32());
                m.set_f32(v2 + 4, (t26 + f(m, c + 0x30)).to_f32());
                m.set_f32(v2 + 8, (t27 + f(m, c + 0x34)).to_f32());
            }
            c = m.u32(c + 0x3c);
        }
        let t28 = f(m, s + 0x4c) * f(m, r + 0x17c) - f(m, s + 0x50) * f(m, r + 0x178);
        let t29 = f(m, s + 0x50) * f(m, r + 0x174) - f(m, s + 0x48) * f(m, r + 0x17c);
        let t30 = f(m, s + 0x48) * f(m, r + 0x178) - f(m, s + 0x4c) * f(m, r + 0x174);
        let t31 = f(m, s + 0x4c) * t30 - f(m, s + 0x50) * t29;
        let l14 = (f(m, s + 0x50) * t28 - t30 * f(m, s + 0x48)).to_f32();
        let l10 = (t29 * f(m, s + 0x48) - t28 * f(m, s + 0x4c)).to_f32();
        let (b0, b1, b2) = (f(m, v1), f(m, v1 + 4), f(m, v1 + 8));
        let t32 = (f(m, r + 0x224) * b1 + f(m, r + 0x220) * b0) + f(m, r + 0x228) * b2;
        let l20 = ((f(m, r + 0x22c) * b0 + f(m, r + 0x230) * b1) + f(m, r + 0x234) * b2).to_f32();
        let l1c = (b2 * f(m, r + 0x240) + (b1 * f(m, r + 0x23c) + b0 * f(m, r + 0x238))).to_f32();
        m.set_f32(r + 0xcc, (t32 - t31).to_f32());
        m.set_f32(r + 0xd0, (x(l20) - x(l14)).to_f32());
        m.set_f32(r + 0xd4, (x(l1c) - x(l10)).to_f32());
        si_vector_clean(m, r + 0xcc, 1);
        let (s48, s4c, s50) = (f(m, s + 0x48), f(m, s + 0x4c), f(m, s + 0x50));
        let (lc, l8, l4);
        if model != fc {
            let p = fc2f4;
            lc = ((f(m, p + 0x30) * s4c + f(m, p + 0x2c) * s48) + f(m, p + 0x34) * s50).to_f32();
            l8 = ((f(m, p + 0x38) * s48 + f(m, p + 0x3c) * s4c) + f(m, p + 0x40) * s50).to_f32();
            l4 = ((s4c * f(m, p + 0x48) + s48 * f(m, p + 0x44)) + s50 * f(m, p + 0x4c)).to_f32();
        } else {
            let p = sett;
            lc = ((f(m, p + 0x74) * s4c + f(m, p + 0x70) * s48) + f(m, p + 0x78) * s50).to_f32();
            l8 = ((f(m, p + 0x7c) * s48 + f(m, p + 0x80) * s4c) + f(m, p + 0x84) * s50).to_f32();
            l4 = ((s4c * f(m, p + 0x8c) + s48 * f(m, p + 0x88)) + s50 * f(m, p + 0x90)).to_f32();
        }
        let (lc, l8, l4) = (x(lc), x(l8), x(l4));
        let t33 = s4c * l4 - s50 * l8;
        let l14 = (s50 * lc - l4 * s48).to_f32();
        let l10 = (l8 * s48 - lc * s4c).to_f32();
        m.set_f32(r + 0xc0, (f(m, v2) - t33).to_f32());
        m.set_f32(r + 0xc4, (f(m, v2 + 4) - x(l14)).to_f32());
        m.set_f32(r + 0xc8, (f(m, v2 + 8) - x(l10)).to_f32());
        si_vector_clean(m, r + 0xc0, 1);
        fc = m.u32(fc + 0x310);
    }
    m.release(0x12c);
}
