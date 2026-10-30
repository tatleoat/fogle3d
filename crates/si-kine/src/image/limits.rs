// SPDX-License-Identifier: MIT
//! The updates of the two limit constraints on the image (`constraint.c`; finding 0022,
//! "Limits: records and damping"; the value forms are in [`crate::limits`]).
//!
//! The damping routines (`rotlimDamping`, `poslimDamping`, `sphericalPoslimDamping`,
//! `rotExceedsLimits`) change only the caller's local vector and read the limit's record:
//! they are called on values ([`crate::limits`]), the record read from the image.

use si_mem::Mem;
use si_types::geom::Vector;
use si_x87::{fmov, x, F80};

use super::invkin::inv_kin_set_goal;
use super::{cur_relation, elem_from_model, find_related, ik, models_chapter, set_rotation, Node};
use crate::globals::SI_INTERACTIVE;
use crate::limits::{poslim_damping, rot_exceeds_limits, rotlim_damping, spherical_poslim_damping, PoslimData};
use crate::outside as out;
use crate::outside::Outside;
use crate::RotlimData;

fn vec_at(m: &Mem, a: u32) -> Vector {
    Vector { x: m.f32(a), y: m.f32(a + 4), z: m.f32(a + 8) }
}

fn shorts(m: &Mem, a: u32) -> [i16; 3] {
    [m.i16(a), m.i16(a + 2), m.i16(a + 4)]
}

/// An `SI_RelRotlimData` (0x44 bytes) read from the image.
pub fn rotlim_data(m: &Mem, d: u32) -> RotlimData {
    RotlimData {
        min_deg: vec_at(m, d),
        max_deg: vec_at(m, d + 0x0c),
        min_active: shorts(m, d + 0x18),
        max_active: shorts(m, d + 0x1e),
        min: vec_at(m, d + 0x24),
        max: vec_at(m, d + 0x30),
        damping: [m.f32(d + 0x3c), m.f32(d + 0x40)],
    }
}

/// An `SI_RelPoslimData` (0x34 bytes) read from the image.
pub fn poslim_data(m: &Mem, d: u32) -> PoslimData {
    PoslimData {
        min: vec_at(m, d),
        max: vec_at(m, d + 0x0c),
        min_active: shorts(m, d + 0x18),
        max_active: shorts(m, d + 0x1e),
        damping: [m.f32(d + 0x24), m.f32(d + 0x28)],
        is_ref_world: m.i16(d + 0x2c),
        is_area_box: m.i16(d + 0x2e),
        radius: m.f32(d + 0x30),
    }
}

/// Both damping values above zero.
fn damped(d: [f32; 2]) -> bool {
    F80::ZERO.lt(x(d[1])) && F80::ZERO.lt(x(d[0]))
}

/// [soft 0x081f72dc si_rotlimConstraint]: relation 103's update ([`crate::limits::rotlim_constraint`]).
///
/// The search for relation 103 is made and its answer not looked at; the entry and its
/// data are read without a test. The selection state is the word at +8 of the graf node
/// the element's +8 points to. If the element also has relation 116 with an entry, that
/// entry's data (two bytes from `SI_malloc(2)` when it has none) takes `rotExceedsLimits`
/// of the angles as they are. Then damping (not for `index` 1 on a selected node), the
/// clamp, and `SI_modelSetRotation(node, angles, mode, 0)` with mode 0 when the program is
/// interactive and the selection state is 1, else 1.
pub fn rotlim_constraint(m: &mut Mem, o: &mut dyn Outside, node: Node, index: i32) {
    let elem = elem_from_model(m, o, node);
    let chapter = models_chapter(m);
    find_related(m, o, elem, 0x67, chapter);
    let rel = cur_relation(m, o, elem);
    let data = m.u32(rel.wrapping_add(0x18));
    let gn = m.u32(elem.wrapping_add(8));
    let r = m[node].vrot;
    let mut a = Vector { x: r.x, y: r.y, z: r.z };
    let d = rotlim_data(m, data);
    if find_related(m, o, elem, 0x74, chapter) != 0 {
        let e = cur_relation(m, o, elem);
        if e != 0 {
            let mut block = m.u32(e + 0x18);
            if block == 0 {
                block = m.si_malloc(2);
                m.set_u32(e + 0x18, block);
            }
            if block != 0 {
                let state = rot_exceeds_limits(a, &d) as i16;
                let block = m.u32(e + 0x18);
                m.set_i16(block, state);
            }
        }
    }
    let selection = m.i32(gn.wrapping_add(8));
    if (index != 1 || selection == 0) && damped(d.damping) {
        rotlim_damping(&mut a, d.min, d.max, &d);
    }
    let mut v = [a.x, a.y, a.z];
    let (lo, hi) = ([d.min.x, d.min.y, d.min.z], [d.max.x, d.max.y, d.max.z]);
    for i in 0..3 {
        if d.min_active[i] != 0 && x(v[i]).lt(x(lo[i])) {
            v[i] = fmov(lo[i]);
        }
        if d.max_active[i] != 0 && x(hi[i]).lt(x(v[i])) {
            v[i] = fmov(hi[i]);
        }
    }
    let mode = if m[SI_INTERACTIVE] != 0 && selection == 1 { 0 } else { 1 };
    set_rotation(m, o, node, Vector { x: v[0], y: v[1], z: v[2] }, mode, 0);
}

/// [soft 0x081f60d4 si_poslimConstraint]: relation 102's update
/// ([`crate::limits::poslim_constraint`]): the position (a joint's or an effector's goal if
/// its record has one set, else its `gtrs`; any other node `SI_constrLocation(node, 2,
/// &position)`) bent and clamped into the limit's sphere or box around `centre` (the
/// origin with `IS_REF_WORLD`); the result becomes the skeleton part's goal
/// (`SI_invKinSetGoal`) or the node's global translation (`SI_modelSetGlobalTranslation`,
/// mode 1). Nothing without the entry's data; the entry is read without a test.
pub fn poslim_constraint(m: &mut Mem, o: &mut dyn Outside, node: Node, centre: Vector, index: i32) {
    let elem = elem_from_model(m, o, node);
    find_related(m, o, elem, 0x66, -1);
    let rel = cur_relation(m, o, elem);
    let data = m.u32(rel.wrapping_add(0x18));
    if data == 0 {
        return;
    }
    let gn = m.u32(elem.wrapping_add(8));
    let d = poslim_data(m, data);
    let c = if d.is_ref_world != 0 { Vector { x: 0.0, y: 0.0, z: 0.0 } } else { centre };
    let skeleton = m[node].kind == 5 || super::is_effector(m, node);
    let sp = m.sp();
    let mut p = if skeleton {
        let r = ik(m, node);
        let at = if r.is_null() || m[r].state == 0 { m[node].gtrs } else { m[r].goal };
        Vector { x: fmov(at.x), y: fmov(at.y), z: fmov(at.z) }
    } else {
        let at = m.local(12);
        o.call(m, &out::SI_CONSTR_LOCATION, &[node.addr(), 2, at]);
        vec_at(m, at)
    };
    let selection = m.i32(gn.wrapping_add(8));
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
        let (mut v, lo, hi) = ([p.x, p.y, p.z], [lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
        for i in 0..3 {
            if d.min_active[i] != 0 && x(lo[i]).gt(x(v[i])) {
                v[i] = fmov(lo[i]);
            }
            if d.max_active[i] != 0 && x(hi[i]).lt(x(v[i])) {
                v[i] = fmov(hi[i]);
            }
        }
        p = Vector { x: v[0], y: v[1], z: v[2] };
    }
    m.set_sp(sp);
    if skeleton {
        inv_kin_set_goal(m, node, p);
    } else {
        let at = m.local(12);
        m.set_f32(at, p.x);
        m.set_f32(at + 4, p.y);
        m.set_f32(at + 8, p.z);
        out::si_model_set_global_translation(o, m, node, at, 1, 0);
        m.set_sp(sp);
    }
}
