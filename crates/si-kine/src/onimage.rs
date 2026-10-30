// SPDX-License-Identifier: MIT
//! Functions of the first pass again, on the shared memory image (finding 0024): the ones
//! the image-form modules call. The first-pass forms (`skeleton`, `invkin`, `limits`,
//! `rotlim`) stay in use where they are; each function here was written from the
//! original's listing, not from them, and checked with vectors of its own.

use si_dict::globals::{SI_CID, SI_ELEM_DICTS};
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::{fmov, x};

use crate::globals::{POSLIM_DATA, ROTLIM_DATA, SI_PROC_DATA};
use crate::outside as out;
use crate::outside::Outside;
use crate::tanconstr::TangentData;

/// Offsets in `SI_InvKin` (the record at `__SI_Model` + 0x318; finding 0022's table).
const IK_KIND: u32 = 0x00;
const IK_PREF: u32 = 0x20;

/// The root of the chain a skeleton node belongs to [soft 0x081d20c0 SI_skeChainRoot].
///
/// Null answers null; a chain root (a null of algorithm 1 or 2) and any node that is
/// neither a joint (kind 5) nor an effector (a null of algorithm 3 or 4) answer
/// themselves. From a joint the walk goes to the father first, from an effector it starts
/// at the node itself, and climbs until it stands on a null of algorithm 1 or 2. No
/// father is tested for null on the way.
pub fn ske_chain_root(m: &Mem, node: Ptr32<Model>) -> Ptr32<Model> {
    if node.is_null() {
        return node;
    }
    let kind = m[node].kind;
    if kind == 0 && (m[node].algo_type.wrapping_sub(1) as u32) <= 1 {
        return node;
    }
    if kind != 5 && (kind != 0 || (m[node].algo_type.wrapping_sub(3) as u32) > 1) {
        return node;
    }
    let mut at = node;
    let mut climb = kind != 0;
    loop {
        if climb {
            at = m[at].father;
            if m[at].kind != 0 {
                if m.fault().is_some() {
                    return at;
                }
                continue;
            }
        }
        climb = true;
        if (m[at].algo_type.wrapping_sub(1) as u32) <= 1 || m.fault().is_some() {
            return at;
        }
    }
}

/// Whether a constraint between two items is allowed as far as skeletons go
/// [soft 0x084adca4 SI_ikValidConstraint]: 0 when both are models (graf type 2 or 3; the
/// second is tested first), the first is a skeleton node (algorithm 1 to 4) and the second
/// is the root of its chain; 1 otherwise.
pub fn ik_valid_constraint(m: &Mem, item: Ptr32<Model>, kind: i32, other: Ptr32<Model>, other_kind: i32) -> i32 {
    if (other_kind.wrapping_sub(2) as u32) <= 1 && (kind.wrapping_sub(2) as u32) <= 1 && (m[item].algo_type.wrapping_sub(1) as u32) <= 3 && ske_chain_root(m, item) == other {
        return 0;
    }
    1
}

/// A new tangency-constraint record [soft 0x081f4bc0 si_tanConstrCreateData]: 0x18 bytes
/// from `SI_malloc`, every field cleared. The block is not tested for null.
pub fn tan_constr_create_data(m: &mut Mem) -> Ptr32<TangentData> {
    let p: Ptr32<TangentData> = Ptr32::new(m.si_malloc(TangentData::SIZE));
    m.set_bytes(p.addr().wrapping_add(4), &[0u8; 0x14]);
    m.set_i16(p.addr(), 0);
    m.set_i16(p.addr().wrapping_add(2), 0);
    p
}

/// A new rotation-limit record [soft 0x081f6ec4 rotlimCreateData]: 0x44 bytes from
/// `SI_calloc(1, 0x44)`, a copy of the program's default record `rotlimData` (every byte
/// of it, in eighteen stores). The block is not tested for null.
pub fn rotlim_create_data(m: &mut Mem) -> u32 {
    let p = m.si_calloc(1, 0x44);
    m.copy(p, ROTLIM_DATA.addr(), 0x44);
    p
}

/// A new position-limit record [soft 0x081f5d4c poslimCreateData]: 0x34 bytes from
/// `SI_calloc(1, 0x34)`, a copy of the default record `poslimData`.
pub fn poslim_create_data(m: &mut Mem) -> u32 {
    let p = m.si_calloc(1, 0x34);
    m.copy(p, POSLIM_DATA.addr(), 0x34);
    p
}

/// Brings a joint's preferred angles inside its rotation limits
/// [soft 0x081f9f6c invKinPrefAngleUpdate].
///
/// Needs the node's record, a relation 103 on its element and that entry's data (the
/// entry itself is read without a test for null). Per axis, an active upper bound below
/// the preferred angle replaces it, else an active lower bound above it does: x always, y
/// only when the chain's root record is of type 40, z always, except that a z bound
/// smaller than 0.05 in size is not stored: the preferred bend becomes +0.05 for a bound
/// above 0 and −0.05 otherwise.
pub fn inv_kin_pref_angle_update(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>) {
    if node.is_null() || m[node].inv_kin.is_null() {
        return;
    }
    let sp = m.sp();
    let index = m.local(4);
    let (found_chap, found_index) = (m.local(4), m.local(4));
    let scene = |m: &Mem| m[SI_ELEM_DICTS].addr().wrapping_add((m[m[SI_PROC_DATA]] as u32).wrapping_mul(0x70));
    let elem = o.call(m, &out::SI_ED_GET_ELEM_FROM_MODEL, &[node.addr(), index]).eax;
    let hit = o.call(m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[scene(m), elem, 1, 1, 0x67, m[SI_CID].model as u32, found_chap, found_index]).eax;
    m.set_sp(sp);
    if hit == 0 {
        return;
    }
    let rel = o.call(m, &out::SI_ELEMDICT_GET_CUR_RELATION, &[scene(m), elem, 1, 1]).eax;
    let data = m.u32(rel.wrapping_add(0x18));
    let rec = m[node].inv_kin.addr();
    let pref = [fmov(m.f32(rec + IK_PREF)), fmov(m.f32(rec + IK_PREF + 4)), fmov(m.f32(rec + IK_PREF + 8))];
    if data == 0 {
        return;
    }
    let root = ske_chain_root(m, node);
    let spatial = m.i32(m[root].inv_kin.addr().wrapping_add(IK_KIND)) == 0x28;
    // +0x18, +0x1e: lower, upper bound in force per axis; +0x24, +0x30: the bounds in radians
    let active = |m: &Mem, base: u32, axis: u32| m.i16(data + base + 2 * axis) != 0;
    let bound = |m: &Mem, base: u32, axis: u32| m.f32(data + base + 4 * axis);
    let clamp = |m: &mut Mem, axis: u32| {
        let p = pref[axis as usize];
        let take = if active(m, 0x1e, axis) && x(bound(m, 0x30, axis)).lt(x(p)) {
            Some(bound(m, 0x30, axis))
        } else if active(m, 0x18, axis) && x(bound(m, 0x24, axis)).gt(x(p)) {
            Some(bound(m, 0x24, axis))
        } else {
            None
        };
        if let Some(v) = take {
            let rec = m[node].inv_kin.addr();
            m.set_f32(rec + IK_PREF + 4 * axis, fmov(v));
        }
    };
    clamp(m, 0);
    if spatial {
        clamp(m, 1);
    }
    let z = if active(m, 0x1e, 2) && x(bound(m, 0x30, 2)).lt(x(pref[2])) {
        Some(bound(m, 0x30, 2))
    } else if active(m, 0x18, 2) && x(bound(m, 0x24, 2)).gt(x(pref[2])) {
        Some(bound(m, 0x24, 2))
    } else {
        None
    };
    if let Some(v) = z {
        let stored = if x(v).abs().lt(x(0.05f64)) {
            if x(v).gt(x(0.0f32)) {
                f32::from_bits(0x3d4c_cccd)
            } else {
                f32::from_bits(0xbd4c_cccd)
            }
        } else {
            fmov(v)
        };
        let rec = m[node].inv_kin.addr();
        m.set_f32(rec + IK_PREF + 8, stored);
    }
}

/// The C library's `%g` of a double: six significant digits, the shorter of the two
/// notations, trailing zeros dropped. (Not a function of the original; `invKinDump`
/// prints through it. The digits are rounded from the exact value, as the library does.)
pub fn percent_g(v: f64) -> String {
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".into() } else { "inf".into() };
    }
    let e = format!("{v:.5e}");
    let (mantissa, exponent) = e.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    let trim = |t: &str| if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.').to_string() } else { t.to_string() };
    if (-4..6).contains(&exponent) {
        trim(&format!("{:.*}", (5 - exponent) as usize, v))
    } else {
        format!("{}e{}{:02}", trim(mantissa), if exponent < 0 { '-' } else { '+' }, exponent.abs())
    }
}

/// Prints an inverse-kinematics record [soft 0x084adbe8 invKinDump]: five lines on the
/// standard output, the type and the joint count as integers, then the goal, the last
/// step and the preferred angles, three numbers each with `%g`. Nothing for null.
pub fn inv_kin_dump(m: &Mem, o: &mut dyn Outside, rec: u32) {
    if rec == 0 {
        return;
    }
    let g = |at: u32| percent_g(m.f32(rec + at) as f64);
    o.stdout(format!("chain type: {}\n", m.i32(rec)).as_bytes());
    o.stdout(format!("nb joints: {}\n", m.i32(rec + 4)).as_bytes());
    for (name, at) in [("goal", 0x08u32), ("drot", 0x14), ("prot", 0x20)] {
        o.stdout(format!("{name}: {} {} {}\n", g(at), g(at + 4), g(at + 8)).as_bytes());
    }
}
