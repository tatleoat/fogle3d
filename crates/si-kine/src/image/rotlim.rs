// SPDX-License-Identifier: MIT
//! The joints' rotation limits as the solver uses them, on the image (`constraint.c`;
//! finding 0022, "Rotation limits"; the value forms are [`crate::rotlim`]).
//!
//! A joint's limits are the data of its element's relation 103: `SI_edGetElemFromModel`,
//! `SI_elemdictFindRelatedElem(scene, elem, 1, 1, 0x67, SI_cid's models chapter, …)` and
//! `SI_elemdictGetCurRelation(scene, elem, 1, 1)`, whose entry has the activation flag at
//! +0x14 and the data (`SI_RelRotlimData`) at +0x18.

use si_mem::Mem;
use si_x87::{fmov, x};

use super::{cur_relation, elem_from_model, find_related, ik, models_chapter, null_walk, son, Node};
use crate::outside::Outside;

/// `SI_RelRotlimData` offsets: lower and upper "active" words, the bounds in radians.
const MIN_ACTIVE: u32 = 0x18;
const MAX_ACTIVE: u32 = 0x1e;
const MIN_RAD: u32 = 0x24;
const MAX_RAD: u32 = 0x30;

/// The joint's relation 103 entry when the search finds one (null otherwise, and null
/// when the dictionary answers null).
fn rotlim_entry(m: &mut Mem, o: &mut dyn Outside, node: Node) -> Option<u32> {
    let elem = elem_from_model(m, o, node);
    let chapter = models_chapter(m);
    if find_related(m, o, elem, 0x67, chapter) == 0 {
        return None;
    }
    Some(cur_relation(m, o, elem))
}

/// [soft 0x081f9bc4 SI_jntRotlimSetFlag]: `use_pref` of each of the `n` joints from `node`
/// becomes 1, then 0 again when the joint has a relation 103 whose data has any of its six
/// "active" words set. The record is written without a test.
pub fn jnt_rotlim_set_flag(m: &mut Mem, o: &mut dyn Outside, node: Node, n: i32) {
    let mut p = node;
    for i in 0..n.max(0) {
        let r = ik(m, p);
        m[r].use_pref = 1;
        if let Some(rel) = rotlim_entry(m, o, p) {
            if rel != 0 {
                let d = m.u32(rel + 0x18);
                if d != 0 && [MAX_ACTIVE, MIN_ACTIVE, MAX_ACTIVE + 2, MIN_ACTIVE + 2, MAX_ACTIVE + 4, MIN_ACTIVE + 4].iter().any(|&o| m.i16(d + o) != 0) {
                    let r = ik(m, p);
                    m[r].use_pref = 0;
                }
            }
        }
        if i + 1 < n {
            match son(m, p) {
                Some(s) => p = s,
                None => return,
            }
        }
    }
}

/// [soft 0x081f9cb0 SI_rotlimConstrainedBranch]: whether the limb has a rotation limit;
/// the loop asks about the first joint `n` times and never moves on.
pub fn rotlim_constrained_branch(m: &mut Mem, o: &mut dyn Outside, node: Node, n: i32) -> bool {
    for _ in 0..n.max(0) {
        let elem = elem_from_model(m, o, node);
        let chapter = models_chapter(m);
        if find_related(m, o, elem, 0x67, chapter) != 0 {
            return true;
        }
    }
    false
}

/// [soft 0x081f9d2c si_checkJntRotLim]: a joint's angles clamped to its active bounds;
/// returns how many were clamped. Nothing without the relation, without data or while the
/// entry is inactive. The entry is read without a test (a null one is a fault on the
/// image). Per angle: an active upper bound not above the angle replaces it, else an active
/// lower bound not below it.
pub fn check_jnt_rot_lim(m: &mut Mem, o: &mut dyn Outside, node: Node) -> i32 {
    let Some(rel) = rotlim_entry(m, o, node) else { return 0 };
    let d = m.u32(rel.wrapping_add(0x18));
    let angles = [m[node].vrot.x, m[node].vrot.y, m[node].vrot.z];
    if d == 0 || m.i16(rel.wrapping_add(0x14)) == 0 {
        return 0;
    }
    let mut count = 0;
    let mut out = angles;
    for k in 0..3u32 {
        let a = angles[k as usize];
        let limit = if m.i16(d + MAX_ACTIVE + 2 * k) != 0 && x(m.f32(d + MAX_RAD + 4 * k)).le(x(a)) {
            Some(m.f32(d + MAX_RAD + 4 * k))
        } else if m.i16(d + MIN_ACTIVE + 2 * k) != 0 && x(m.f32(d + MIN_RAD + 4 * k)).ge(x(a)) {
            Some(m.f32(d + MIN_RAD + 4 * k))
        } else {
            None
        };
        if let Some(l) = limit {
            out[k as usize] = fmov(l);
            count += 1;
            match k {
                0 => m[node].vrot.x = out[0],
                1 => m[node].vrot.y = out[1],
                _ => m[node].vrot.z = out[2],
            }
        }
    }
    count
}

/// [soft 0x081f9e94 SI_checkRotationLimit]: the joints of a limb clamped in turn; at the
/// first that had an angle clamped `*node` becomes its son (stored as it is) and `*n` the
/// joints left below it, the rest are clamped too, and that joint's count is returned.
/// With `*n` ≤ 0 the original returns a local it never set; 0 here.
pub fn check_rotation_limit(m: &mut Mem, o: &mut dyn Outside, node: &mut Node, n: &mut i32) -> i32 {
    let total = *n;
    let mut p = *node;
    let mut i = 0;
    while i < total {
        let lim = check_jnt_rot_lim(m, o, p);
        if lim != 0 {
            let s = m[p].son;
            *node = s;
            *n = (*n - i) - 1;
            p = s;
            for _ in (i + 1)..total {
                if p.is_null() {
                    null_walk(m, 0x2c4);
                    break;
                }
                check_jnt_rot_lim(m, o, p);
                p = m[p].son;
            }
            return lim;
        }
        i += 1;
        if i < total {
            match son(m, p) {
                Some(s) => p = s,
                None => return 0,
            }
        }
    }
    0
}

/// [soft 0x081f9f28 SI_constrainedJnt]: how many of the `n` joints from `node` have all
/// three angles at a limit (clamping as it counts).
pub fn constrained_jnt(m: &mut Mem, o: &mut dyn Outside, node: Node, n: i32) -> i32 {
    let mut count = 0;
    let mut p = node;
    for i in 0..n.max(0) {
        if check_jnt_rot_lim(m, o, p) == 3 {
            count += 1;
        }
        if i + 1 < n {
            match son(m, p) {
                Some(s) => p = s,
                None => break,
            }
        }
    }
    count
}
