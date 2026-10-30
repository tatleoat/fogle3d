// SPDX-License-Identifier: MIT
//! `dictin.c`: the step of `elemdictCheckPasteFcv` that ties one pasted curve to its element
//! (finding 0004, "dictin.c"). Records are addressed by named offsets in the image: the
//! dictionary (+4 its chapters, 0x40 bytes each, whose +8 is the array of 0x84-byte
//! elements), an element (+4 the object a curve belongs to, +0x50 the root index) and a
//! curve (+0 its type, +4 the object it belongs to). Everything else is reached through
//! [`Outside`].

use crate::globals as g;
use crate::outside::{self as out, Outside};
use si_mem::Mem;
use si_x87::{x, F80};

/// The attribute dictionary `SI_ATD` (0x08781438; the callee reads it).
const ATD: u32 = 0x0878_1438;

/// A frame count as the paste turns it into seconds: the count stored as float, divided
/// by `SI_frameRate` in the register, stored as float again (the original's `frndint` under a
/// rounding mode it sets and restores is the identity on an integer).
fn seconds(m: &Mem, frames: i32) -> f32 {
    (F80::from(frames as f32) / x(m[g::SI_FRAME_RATE])).to_f32()
}

/// [soft 0x081287c0 elemdictCheckPasteFcv]
///
/// Ties the curve `*fcv` (a pointer to the caller's pointer) to the element `elem` of the
/// dictionary `dict`, whose relation dictionary is `rd`. Nothing is done for a curve no
/// attribute owns (`SI_attdictGetInfoFromFcurve` answers relation type 0). Otherwise the
/// curve learns its element's object (+4), and with `complete` (the tenth argument) the
/// element is related to the curve's element `p7` completely (`p5` the first element's
/// index, `active` the entry's word); without it the element's own curves of that
/// relation's chapter are searched for one of the same type: none, and the curve's keys
/// are moved (all times, by `frames` frames as seconds), the relation is added and, if
/// `root` is not negative, it becomes the root index of the curve's element; one, and the
/// keys are pasted into it, the pasted curve removed (`*fcv` is then the old one) and its
/// element deleted. `_p4` is not read.
#[allow(clippy::too_many_arguments)]
pub fn elemdict_check_paste_fcv(
    m: &mut Mem,
    o: &mut dyn Outside,
    dict: u32,
    rd: u32,
    elem: u32,
    _p4: i32,
    p5: i32,
    fcv: u32,
    p7: i32,
    active: i16,
    root: i32,
    complete: i16,
    frames: i32,
) {
    let sp = m.sp();
    let (rel, grp, n1, n2, chap) = (m.local(4), m.local(4), m.local(4), m.local(4), m.local(4));
    let kind = m.u32(m.u32(fcv));
    o.call(m, &out::SI_ATTDICT_GET_INFO_FROM_FCURVE, &[ATD, kind, rel, grp, n1, n2]);
    if m.i32(rel) != 0 {
        let c = m.u32(fcv);
        let eo = m.u32(elem.wrapping_add(4));
        m.set_u32(c.wrapping_add(4), eo);
        let mut found = 0u32;
        if complete == 0 {
            o.call(m, &out::SI_RELDICT_GET_REL_CHAP_GRP, &[rd, m.u32(rel), chap]);
            found = o.call(m, &out::SI_ED_FIND_FCV, &[elem, m.u32(chap), m.u32(rel)]);
            while found != 0 && m.u32(found) != m.u32(m.u32(fcv)) {
                found = o.call(m, &out::SI_ED_FIND_NEXT_FCV, &[elem, m.u32(chap), m.u32(rel)]);
            }
        }
        if complete != 0 {
            o.call(
                m,
                &out::SI_ELEMDICT_ADD_COMPLETE_RELATION,
                &[dict, rd, p5 as u32, p7 as u32, m.u32(rel), m.u32(n1), m.u32(n2), 0, active as i32 as u32],
            );
        } else if found == 0 {
            let t = seconds(m, frames);
            let c = m.u32(fcv);
            o.call(m, &out::SI_FCV_MOVE_KEY_TIME, &[c, (-1.0e10f32).to_bits(), 1.0e10f32.to_bits(), t.to_bits(), 0]);
            o.call(m, &out::SI_ELEMDICT_ADD_RELATION, &[dict, rd, p5 as u32, p7 as u32, m.u32(rel), m.u32(n1), m.u32(n2)]);
            if root >= 0 {
                let chapters = m.u32(dict.wrapping_add(4));
                let ani = m[g::SI_CID][0] as u32;
                let elems = m.u32(chapters.wrapping_add(ani.wrapping_shl(6)).wrapping_add(8));
                m.set_u32(elems.wrapping_add((p7 as u32).wrapping_mul(0x84)).wrapping_add(0x50), root as u32);
            }
        } else {
            let t = seconds(m, frames).to_bits();
            let c = m.u32(fcv);
            o.call(m, &out::SI_FCV_SIMPLE_PASTE_INSERT, &[c, found, t, t]);
            o.call(m, &out::SI_FCURVE_REMOVE, &[fcv]);
            m.set_u32(fcv, found);
            let ani = m[g::SI_CID][0] as u32;
            let chapters = m.u32(dict.wrapping_add(4));
            let elems = m.u32(chapters.wrapping_add(ani.wrapping_shl(6)).wrapping_add(8));
            let e = elems.wrapping_add((p7 as u32).wrapping_mul(0x84));
            o.call(m, &out::SI_ELEMDICT_SET_DELETE_ELEM, &[dict, ani, e, p7 as u32, u32::MAX, 0, 1]);
        }
    }
    m.set_sp(sp);
}
