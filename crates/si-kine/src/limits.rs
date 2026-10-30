// SPDX-License-Identifier: MIT
//! Position and rotation limits: the data records of the two limit constraints and the
//! routines of the original's `constraint.c` that work on them alone (finding 0022,
//! "Limits: records and damping").
//!
//! A limit keeps each of three values (a node's translation, or its rotation angles)
//! between a lower and an upper bound, each bound with its own "active" word. **Damping**
//! softens a bound: inside a band of width w before the bound the value is bent so that it
//! approaches the bound exponentially instead of running into it.

use core::mem::{offset_of, size_of};

use si_model::node::{Models, NodeId};
use si_model::setters::set_global_translation;
use si_model::transform::set_rotation;
use si_types::geom::Vector;
use si_x87::trig;
use si_x87::{fmov, x, F80};

use crate::invkin::inv_kin_set_goal;
use crate::rec::IkStore;
use crate::skeleton::is_effector;
pub use crate::RotlimData;
use crate::{fault, Globals, Host};

/// The double 0.0174532925 (0x0864d028, 0x0864d030): degrees to radians, ten digits.
const DEG_TO_RAD: f64 = 0.0174532925;
/// The double 0.0001 (0x0864cfc0).
const EPS: f64 = 1e-4;

/// `SI_RelPoslimData`, the data of relation 102 `CnsPosLim`, 0x34 bytes. The field names
/// follow the keywords of the scene description (finding 0006).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoslimData {
    /// +0x00 `MIN_LIMIT`.
    pub min: Vector,
    /// +0x0c `MAX_LIMIT`.
    pub max: Vector,
    /// +0x18 `MIN_ACTIVE`.
    pub min_active: [i16; 3],
    /// +0x1e `MAX_ACTIVE`.
    pub max_active: [i16; 3],
    /// +0x24, +0x28 `DAMPING`: the band's width and the strength.
    pub damping: [f32; 2],
    /// +0x2c `IS_REF_WORLD`.
    pub is_ref_world: i16,
    /// +0x2e `IS_AREA_BOX`.
    pub is_area_box: i16,
    /// +0x30 `LIMIT_RADIUS`: the radius of a spherical limit.
    pub radius: f32,
}

const _: () = {
    assert!(size_of::<PoslimData>() == 0x34);
    assert!(offset_of!(PoslimData, max) == 0x0c);
    assert!(offset_of!(PoslimData, min_active) == 0x18);
    assert!(offset_of!(PoslimData, max_active) == 0x1e);
    assert!(offset_of!(PoslimData, damping) == 0x24);
    assert!(offset_of!(PoslimData, is_ref_world) == 0x2c);
    assert!(offset_of!(PoslimData, is_area_box) == 0x2e);
    assert!(offset_of!(PoslimData, radius) == 0x30);
    assert!(size_of::<RotlimData>() == 0x44);
    assert!(offset_of!(RotlimData, max_deg) == 0x0c);
    assert!(offset_of!(RotlimData, min_active) == 0x18);
    assert!(offset_of!(RotlimData, max_active) == 0x1e);
    assert!(offset_of!(RotlimData, min) == 0x24);
    assert!(offset_of!(RotlimData, max) == 0x30);
    assert!(offset_of!(RotlimData, damping) == 0x3c);
};

/// A new position-limit record [soft 0x081f5d4c poslimCreateData]: a copy, field by field,
/// of the program's current default record (the global `poslimData`, 0x08789520, which the
/// set-up dialog edits).
pub fn poslim_create_data(defaults: &PoslimData) -> PoslimData {
    *defaults
}

/// A new rotation-limit record [soft 0x081f6ec4 rotlimCreateData]: a copy of the global
/// `rotlimData` (0x08789560).
pub fn rotlim_create_data(defaults: &RotlimData) -> RotlimData {
    *defaults
}

fn arr(v: Vector) -> [f32; 3] {
    [v.x, v.y, v.z]
}

fn vec(a: [f32; 3]) -> Vector {
    Vector { x: a[0], y: a[1], z: a[2] }
}

/// The damping both box limits share. `w` is the band's width, `strength` the second
/// damping value. Per value v with upper bound hi and lower bound lo, with k =
/// r32((strength − 1) / w):
///
/// - upper bound active and v > hi − w: v = r32(hi − expf(r32((v − (hi − w)) × k)) × w);
/// - then, lower bound active and v < lo + w: v = r32(expf(r32(((lo + w) − v) × k)) × w + lo).
///
/// All three upper bounds are done before the lower ones. The band's edge hi − w is kept
/// in the register for x and rounded to float for y and z; lo + w is a float for all three.
fn damp(value: &mut Vector, lo: Vector, hi: Vector, min_active: [i16; 3], max_active: [i16; 3], w: f32, strength: f32) {
    let mut v = arr(*value);
    let (lo, hi) = (arr(lo), arr(hi));
    let low_edge = [(x(w) + lo[0]).to_f32(), (x(w) + lo[1]).to_f32(), (x(w) + lo[2]).to_f32()];
    let high_edge = [x(hi[0]) - w, x((x(hi[1]) - w).to_f32()), x((x(hi[2]) - w).to_f32())];
    let k = ((x(strength) - F80::ONE) / w).to_f32();
    for i in 0..3 {
        if max_active[i] != 0 && x(v[i]).gt(high_edge[i]) {
            let e = trig::expf(((x(v[i]) - high_edge[i]) * k).to_f32());
            v[i] = (x(hi[i]) - e * w).to_f32();
        }
    }
    for i in 0..3 {
        if min_active[i] != 0 && x(low_edge[i]).gt(x(v[i])) {
            let e = trig::expf(((x(low_edge[i]) - v[i]) * k).to_f32());
            v[i] = (e * w + lo[i]).to_f32();
        }
    }
    *value = vec(v);
}

/// Bends a position towards the bounds of a box limit [soft 0x081f5f08 poslimDamping]: the
/// shared damping with the record's first damping value as the band's width. Class: libm
/// (`expf`).
pub fn poslim_damping(position: &mut Vector, lo: Vector, hi: Vector, d: &PoslimData) {
    damp(position, lo, hi, d.min_active, d.max_active, d.damping[0], d.damping[1]);
}

/// The same for rotation angles [soft 0x081f6f7c rotlimDamping]: the band's width is the
/// record's first damping value **taken as degrees**, r32(width × 0.0174532925), while the
/// angles and bounds are radians. Class: libm.
pub fn rotlim_damping(angles: &mut Vector, lo: Vector, hi: Vector, d: &RotlimData) {
    let w = (x(d.damping[0]) * DEG_TO_RAD).to_f32();
    damp(angles, lo, hi, d.min_active, d.max_active, w, d.damping[1]);
}

/// Bends a position towards the surface of a spherical limit
/// [soft 0x081f5df0 sphericalPoslimDamping]. Class: libm.
///
/// With t = position − centre and D = r32(|t|): nothing unless D > radius − w (w the
/// first damping value). Else e = expf(r32((w + (D − radius)) × r32((strength − 1) / w)));
/// t is scaled to the length radius − e × w (when its float length is above 0.0001) and the
/// position becomes centre + t.
pub fn spherical_poslim_damping(position: &mut Vector, centre: Vector, d: &PoslimData) {
    let t = [x(position.x) - centre.x, x(position.y) - centre.y, x(position.z) - centre.z];
    let mut t32 = [t[0].to_f32(), t[1].to_f32(), t[2].to_f32()];
    let dist = ((t[1] * t[1] + t[0] * t[0]) + t[2] * t[2]).sqrt().to_f32();
    let (w, radius) = (d.damping[0], d.radius);
    if !x(dist).gt(x(radius) - w) {
        return;
    }
    let k = ((x(d.damping[1]) - F80::ONE) / w).to_f32();
    let e = trig::expf(((x(w) + (x(dist) - radius)) * k).to_f32());
    let len = ((x(t32[1]) * t32[1] + x(t32[0]) * t32[0]) + x(t32[2]) * t32[2]).sqrt().to_f32();
    if x(len).abs().gt(x(EPS)) {
        let f = (x(radius) - e * w) / len;
        t32 = [(x(t32[0]) * f).to_f32(), (x(t32[1]) * f).to_f32(), (f * t32[2]).to_f32()];
    }
    *position = Vector { x: (x(centre.x) + t32[0]).to_f32(), y: (x(centre.y) + t32[1]).to_f32(), z: (x(centre.z) + t32[2]).to_f32() };
}

/// Whether angles are at or beyond a rotation limit [soft 0x081f715c rotExceedsLimits]: 0
/// no; 2 beyond an active bound (lower: angle < bound; upper: angle > bound); with damping
/// in force (both damping values above 0) instead 1 when an angle is inside a damping
/// band (below lower + w, or above upper − w, w = r32(0.0174532925 × the first damping
/// value)), and never 2. Class: model.
pub fn rot_exceeds_limits(angles: Vector, d: &RotlimData) -> i32 {
    let a = arr(angles);
    let (lo, hi) = (arr(d.min), arr(d.max));
    let w = (x(DEG_TO_RAD) * d.damping[0]).to_f32();
    if F80::ZERO.lt(x(d.damping[1])) && F80::ZERO.lt(x(d.damping[0])) {
        for i in 0..3 {
            if d.min_active[i] != 0 && (x(lo[i]) + w).gt(x(a[i])) {
                return 1;
            }
            if d.max_active[i] != 0 && (x(hi[i]) - w).lt(x(a[i])) {
                return 1;
            }
        }
        return 0;
    }
    for i in 0..3 {
        if d.min_active[i] != 0 && x(lo[i]).gt(x(a[i])) {
            return 2;
        }
        if d.max_active[i] != 0 && x(hi[i]).lt(x(a[i])) {
            return 2;
        }
    }
    0
}

/// Brings a rotation limit's damping values into range
/// [soft 0x081f7540 rotlimDampingInRange]: the strength into [0, 1] (a NaN becomes 0); the
/// width to 0 when negative; then the width is cut down to the size of each active bound
/// **in degrees** that is smaller (lower x, y, z, then upper x, y, z). Class: copy.
pub fn rotlim_damping_in_range(d: &mut RotlimData) {
    let s = d.damping[1];
    if x(s).ge(F80::ZERO) {
        if x(s).gt(F80::ONE) {
            d.damping[1] = 1.0;
        }
    } else {
        d.damping[1] = 0.0;
    }
    if x(d.damping[0]).lt(F80::ZERO) {
        d.damping[0] = 0.0;
    }
    let bounds = arr(d.min_deg).into_iter().zip(d.min_active).chain(arr(d.max_deg).into_iter().zip(d.max_active));
    for (bound, active) in bounds {
        if active != 0 && x(bound).abs().lt(x(d.damping[0])) {
            d.damping[0] = x(bound).abs().to_f32();
        }
    }
}

/// Puts a rotation limit's bounds in order [soft 0x081f764c rotlimValueSwap]: where both
/// bounds of an angle are active and the upper one (in degrees) is below the lower, the
/// two are exchanged. The bounds in radians are not touched. Class: copy.
pub fn rotlim_value_swap(d: &mut RotlimData) {
    let (mut lo, mut hi) = (arr(d.min_deg), arr(d.max_deg));
    for i in 0..3 {
        if d.min_active[i] != 0 && d.max_active[i] != 0 && x(hi[i]).lt(x(lo[i])) {
            let (a, b) = (fmov(lo[i]), fmov(hi[i]));
            lo[i] = b;
            hi[i] = a;
        }
    }
    d.min_deg = vec(lo);
    d.max_deg = vec(hi);
}

/// A rotation limit that limits nothing [soft 0x081f76d8 resetRotlimData]: bounds of ∓999
/// degrees and ∓17.435839 radians (0x0864d038 to …d048; 999 × 0.0174532925 as a float), no
/// bound active, damping width 0 and strength 0.5.
pub fn reset_rotlim_data(d: &mut RotlimData) {
    let rad = f32::from_bits(0x418b_7c99);
    d.min_deg = vec([-999.0; 3]);
    d.max_deg = vec([999.0; 3]);
    d.min = vec([-rad; 3]);
    d.max = vec([rad; 3]);
    d.damping = [0.0, 0.5];
    d.min_active = [0; 3];
    d.max_active = [0; 3];
}

/// Both damping values of a limit are above zero: damping is in force.
fn damped(d: [f32; 2]) -> bool {
    F80::ZERO.lt(x(d[1])) && F80::ZERO.lt(x(d[0]))
}

/// The update of a rotation limit, relation 103 [soft 0x081f72dc si_rotlimConstraint]:
/// the node's angles are bent and clamped to the limit and set again. Class: libm.
///
/// 1. If the node's element also has relation 116, that entry's data word becomes
///    [`rot_exceeds_limits`] of the angles as they are.
/// 2. Damping ([`rotlim_damping`], bounds in radians) when it is in force, **except for
///    `index` 1 on a node whose graf node is selected** (a node being manipulated runs
///    into its bounds undamped).
/// 3. The clamp: per angle, below an active lower bound it becomes the bound, then above
///    an active upper bound it becomes that.
/// 4. `SI_modelSetRotation(node, angles, mode, 0)`: mode 1 (the sons are taken along),
///    or 0 when the program is interactive and the graf node's selection state is 1.
///
/// The original reads the relation entry and its data without testing either pointer;
/// without data nothing is done here, counted as a fault.
pub fn rotlim_constraint(models: &mut Models, node: NodeId, index: i32, g: &Globals, host: &mut dyn Host) {
    host.find_rotlim(models, node);
    let Some(d) = host.cur_rotlim(models, node).and_then(|r| r.data) else {
        fault::note();
        return;
    };
    let selection = host.elem_selection(models, node);
    let r = models.m(node).vrot;
    let mut a = Vector { x: fmov(r.x), y: fmov(r.y), z: fmov(r.z) };
    host.set_rotlim_state(models, node, rot_exceeds_limits(a, &d) as i16);
    if (index != 1 || selection == 0) && damped(d.damping) {
        rotlim_damping(&mut a, d.min, d.max, &d);
    }
    let (mut v, lo, hi) = (arr(a), arr(d.min), arr(d.max));
    for i in 0..3 {
        if d.min_active[i] != 0 && x(v[i]).lt(x(lo[i])) {
            v[i] = fmov(lo[i]);
        }
        if d.max_active[i] != 0 && x(hi[i]).lt(x(v[i])) {
            v[i] = fmov(hi[i]);
        }
    }
    let mode = if g.interactive && selection == 1 { 0 } else { 1 };
    set_rotation(models, node, vec(v), mode, 0, &g.model, host);
}

/// The update of a position limit, relation 102 [soft 0x081f60d4 si_poslimConstraint]:
/// the node's position is bent and clamped into the limit's area around `centre`.
/// Class: libm.
///
/// - `IS_REF_WORLD` makes the centre the origin.
/// - The position limited: for a joint or an effector its goal if it has one (a record
///   whose state is not 0), else its `gtrs`; for any other node `SI_constrLocation`'s
///   answer (a host method).
/// - A **sphere** (`IS_AREA_BOX` 0): [`spherical_poslim_damping`] when damping is in
///   force, then a position further than the radius from the centre is pulled in onto
///   the sphere: centre + d × (radius / r32(|d|)), each component rounded once.
/// - A **box**: the bounds are centre + `MIN_LIMIT` and centre + `MAX_LIMIT` (floats);
///   [`poslim_damping`] when damping is in force; then per axis the clamp to an active
///   lower and then an active upper bound.
/// - Damping is left out for `index` 2 on a node whose graf node is selected.
/// - The result becomes a skeleton part's goal ([`inv_kin_set_goal`]) or the node's
///   global translation (`SI_modelSetGlobalTranslation`, mode 1).
///
/// Nothing is done without the relation's data.
pub fn poslim_constraint(models: &mut Models, ik: &mut IkStore, node: NodeId, centre: Vector, index: i32, g: &Globals, host: &mut dyn Host) {
    let Some(d) = host.cur_poslim(models, node) else { return };
    let selection = host.elem_selection(models, node);
    let c = if d.is_ref_world != 0 { Vector { x: 0.0, y: 0.0, z: 0.0 } } else { centre };
    let skeleton = models.m(node).kind == 5 || is_effector(models, node);
    let mut p = if skeleton {
        let at = match ik.of(models, node) {
            Some(r) if r.state != 0 => r.goal,
            _ => models.m(node).gtrs,
        };
        Vector { x: fmov(at.x), y: fmov(at.y), z: fmov(at.z) }
    } else {
        host.constr_location(models, node)
    };
    let damp = (index != 2 || selection == 0) && damped(d.damping);
    if d.is_area_box == 0 {
        if damp {
            spherical_poslim_damping(&mut p, c, &d);
        }
        let (dx, dy, dz) = (x(p.x) - c.x, x(p.y) - c.y, x(p.z) - c.z);
        let len = (dz * dz + (dy * dy + dx * dx)).sqrt().to_f32();
        if x(d.radius).lt(x(len)) {
            let q = x(d.radius) / len;
            p = Vector { x: (dx * q + c.x).to_f32(), y: (dy * q + c.y).to_f32(), z: (x(c.z) + dz * q).to_f32() };
        }
    } else {
        let lo = Vector { x: (x(c.x) + d.min.x).to_f32(), y: (x(c.y) + d.min.y).to_f32(), z: (x(c.z) + d.min.z).to_f32() };
        let hi = Vector { x: (x(c.x) + d.max.x).to_f32(), y: (x(c.y) + d.max.y).to_f32(), z: (x(c.z) + d.max.z).to_f32() };
        if damp {
            poslim_damping(&mut p, lo, hi, &d);
        }
        let (mut v, lo, hi) = (arr(p), arr(lo), arr(hi));
        for i in 0..3 {
            if d.min_active[i] != 0 && x(lo[i]).gt(x(v[i])) {
                v[i] = fmov(lo[i]);
            }
            if d.max_active[i] != 0 && x(hi[i]).lt(x(v[i])) {
                v[i] = fmov(hi[i]);
            }
        }
        p = vec(v);
    }
    if skeleton {
        inv_kin_set_goal(models, ik, node, p);
    } else {
        set_global_translation(models, node, p, 1, 0, &g.model, host);
    }
}

/// The data of a tangency constraint (0x18 bytes; the fields are finding 0017's).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TangentData {
    /// +0x00: set.
    pub set: i16,
    /// +0x02: same path.
    pub same_path: i16,
    /// +0x04: percent.
    pub percent: f32,
    /// +0x08: distance.
    pub distance: f32,
    /// +0x0c: point.
    pub point: [f32; 3],
}

const _: () = assert!(size_of::<TangentData>() == 0x18);

/// A new tangency-constraint record [soft 0x081f4bc0 si_tanConstrCreateData]: 0x18 bytes
/// from `SI_malloc`, every field cleared.
pub fn tan_constr_create_data() -> TangentData {
    TangentData::default()
}

/// A global point in the frame of a node's father, as the three orientation routines below
/// write it out: through the inverse of the father's `gmat`, row 3 included,
/// x = ((y·m₁₀ + x·m₀₀) + z·m₂₀) + m₃₀, y = (z·m₂₁ + (y·m₁₁ + x·m₀₁)) + m₃₁,
/// z = (z·m₂₂ + (y·m₁₂ + x·m₀₂)) + m₃₂. A node without a father keeps the point. When the
/// father's matrix has no inverse the original computes with whatever its stack slot
/// held; here the point is kept and a fault counted.
fn into_father(models: &Models, node: NodeId, p: Vector) -> Vector {
    let Some(f) = models.father(node) else { return p };
    let Some(m) = si_math::algebra::matrix_inverse(&models.m(f).gmat) else {
        fault::note();
        return p;
    };
    Vector {
        x: (((x(p.y) * m[1][0] + x(p.x) * m[0][0]) + x(p.z) * m[2][0]) + m[3][0]).to_f32(),
        y: ((x(p.z) * m[2][1] + (x(p.y) * m[1][1] + x(p.x) * m[0][1])) + m[3][1]).to_f32(),
        z: ((x(p.z) * m[2][2] + (x(p.y) * m[1][2] + x(p.x) * m[0][2])) + m[3][2]).to_f32(),
    }
}

/// Angles of a matrix with zeros as the previous solution, set as the node's rotation with
/// mode 1 and no geometry compensation: the common end of the three routines below.
fn orient(models: &mut Models, node: NodeId, m: &si_math::types::Matrix, g: &Globals, host: &mut dyn Host) {
    use si_model::node::{mv, tv};
    let zero = Vector { x: 0.0, y: 0.0, z: 0.0 };
    let angles = tv(si_math::algebra::matrix_extract_xyz_rotation(m, &mv(zero), g.model.keep_turns));
    set_rotation(models, node, angles, 1, 0, &g.model, host);
}

/// Orients a node by two vectors, for the three-point constraint
/// [soft 0x081f3e48 si_constr3PtsMapDirection]: both are brought into the father's frame
/// (as points, see `into_father`), `SI_matrixAlignYAxes(first, second)` makes the frame
/// whose Y axis is the second vector, and its angles become the node's rotation. False,
/// and nothing set, when the second vector is zero. Class: libm.
pub fn constr_3pts_map_direction(models: &mut Models, node: NodeId, first: Vector, second: Vector, g: &Globals, host: &mut dyn Host) -> bool {
    use si_model::node::mv;
    let (a, b) = (into_father(models, node, first), into_father(models, node, second));
    match si_math::algebra::matrix_align_y_axes(&mv(a), &mv(b)) {
        Some(m) => {
            orient(models, node, &m, g, host);
            true
        }
        None => false,
    }
}

/// The same for the normal constraint [soft 0x081f3ad0 si_norConstrMapDirection], with a
/// turn about the new Y axis first: the frame is multiplied from the left by
/// Ry(`__SI_Model` + 0x168), the copy of the node's y angle that the rotation curves'
/// update keeps (finding 0017). Class: libm.
pub fn nor_constr_map_direction(models: &mut Models, node: NodeId, first: Vector, second: Vector, g: &Globals, host: &mut dyn Host) -> bool {
    use si_model::node::mv;
    let (a, b) = (into_father(models, node, first), into_father(models, node, second));
    match si_math::algebra::matrix_align_y_axes(&mv(a), &mv(b)) {
        Some(m) => {
            let turn = si_math::algebra::matrix_y_rotation(models.m(node).unk_164[1]);
            orient(models, node, &si_math::algebra::matrix_multiply(&turn, &m), g, host);
            true
        }
        None => false,
    }
}

/// Orients a node along a tangent [soft 0x081f4998 si_tanConstrMapDirection]: X along the
/// tangent, Y towards an up direction. The up direction is (0, 1, 0), or, when the node's
/// element has an up-vector relation (128), the vector from the node to that relation's
/// object (`gtrs` minus `gtrs`, floats). Both go into the father's frame;
/// `SI_matrixAlignAxes(tangent, up)` makes the frame, which is multiplied from the left by
/// Rx(`__SI_Model` + 0x164), the kept copy of the node's x angle. False, and nothing set,
/// when the tangent is zero. Class: libm.
pub fn tan_constr_map_direction(models: &mut Models, node: NodeId, tangent: Vector, g: &Globals, host: &mut dyn Host) -> bool {
    use si_model::node::mv;
    let mut up = Vector { x: 0.0, y: 1.0, z: 0.0 };
    if let Some(o) = host.up_vector_object(models, node) {
        let (a, b) = (models.m(o).gtrs, models.m(node).gtrs);
        up = Vector { x: (x(a.x) - b.x).to_f32(), y: (x(a.y) - b.y).to_f32(), z: (x(a.z) - b.z).to_f32() };
    }
    let (a, b) = (into_father(models, node, tangent), into_father(models, node, up));
    let (m, ok) = si_math::algebra::matrix_align_axes(&mv(a), &mv(b));
    if ok {
        let turn = si_math::algebra::matrix_x_rotation(models.m(node).unk_164[0]);
        orient(models, node, &si_math::algebra::matrix_multiply(&turn, &m), g, host);
    }
    ok
}
