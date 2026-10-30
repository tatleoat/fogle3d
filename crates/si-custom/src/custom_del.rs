// SPDX-License-Identifier: MIT
//! Three small `custom_util.c` / `custom_mn.c` routines left over from the earlier units (unit last2, finding 0041,
//! 2026-10-29): the reading-anim-model flag, the icon delete over the five custom-effect statuses, and the effect use
//! counts of an element that is being deleted. The last two are called only from `SI_graflistDeleteGrafnode`.

use crate::custom_scene::{elem_get_user_data_list, si_cus_user_data_get_callback_nodes};
use crate::custom_util::{CUS_ACTOR, CUS_MATTER, CUS_MODEL, CUS_MOTION, CUS_TOOLS};
use crate::custom_util2::cus_status_delete_icon;
use si_mem::{Mem, Outside};

/// The short that `setReadingAnimModel` stores and `SI_cusReadingAnimModel` reads.
pub const READING_ANIM_MODEL: u32 = 0x086e_0ca2;

/// [soft 0x0853b274 SI_cusReadingAnimModel]: the short at 0x086e0ca2, sign-extended.
pub fn si_cus_reading_anim_model(m: &Mem) -> i32 {
    m.u16(READING_ANIM_MODEL) as i16 as i32
}

/// The statuses `SI_cusDeleteIcon` tries, in its order: motion, actor, model, matter, tools.
pub const DELETE_ICON_ORDER: [u32; 5] = [CUS_MOTION, CUS_ACTOR, CUS_MODEL, CUS_MATTER, CUS_TOOLS];

/// [soft 0x0854a094 SI_cusDeleteIcon]: runs `cusStatusDeleteIcon` on the motion, actor, model, matter and tools
/// statuses in turn and stops at the first answer that is not negative; the answer is the last one got (so -1 when no
/// status held an effect for the icon, or every delete failed).
pub fn si_cus_delete_icon(m: &mut Mem, o: &mut dyn Outside, icon: u32) -> i32 {
    let mut r = -1;
    for status in DELETE_ICON_ORDER {
        r = cus_status_delete_icon(m, o, status, icon);
        if r >= 0 {
            break;
        }
    }
    r
}

/// [soft 0x0854bc44 SI_cusElemCallbackUpdateEffectsForDeletion]: for an element (`elem`, of kind `kind`) whose user
/// data list holds both the "CUSELEMCB" and the "CUSNEXTELEMCB" nodes: each entry (12 bytes, effect first) of the
/// array of the "CUSELEMCB" node (+0xc, its byte size at +8) takes one off the use count (short at +0x3c) of its
/// effect's record (effect +0x104); then the array is freed and the node's array and size are cleared. Nothing when
/// `elem` or its record (+4) is null, a node is missing, or the array is null.
pub fn si_cus_elem_callback_update_effects_for_deletion(m: &mut Mem, kind: u32, elem: u32) {
    if elem == 0 || m.u32(elem.wrapping_add(4)) == 0 {
        return;
    }
    let loc = m.local(8);
    let (cb, next) = (loc, loc.wrapping_add(4));
    m.set_u32(cb, 0);
    m.set_u32(next, 0);
    let list = elem_get_user_data_list(m, elem, kind);
    si_cus_user_data_get_callback_nodes(m, list, cb, next);
    let (node, nxt) = (m.u32(cb), m.u32(next));
    m.release(8);
    if nxt == 0 || node == 0 {
        return;
    }
    let arr = m.u32(node.wrapping_add(0xc));
    if arr == 0 {
        return;
    }
    // the byte size over 12, by the multiply-high with 0xaaaaaaab and a shift by 3 the original uses
    let n = m.u32(node.wrapping_add(8)) / 12;
    for i in 0..n {
        let eff = m.u32(arr.wrapping_add(i.wrapping_mul(0xc)));
        let rec = m.u32(eff.wrapping_add(0x104));
        let c = m.u16(rec.wrapping_add(0x3c));
        m.set_u16(rec.wrapping_add(0x3c), c.wrapping_sub(1));
    }
    let arr = m.u32(node.wrapping_add(0xc));
    if arr != 0 {
        m.free(arr);
        m.set_u32(node.wrapping_add(0xc), 0);
    }
    m.set_u32(node.wrapping_add(0xc), 0);
    m.set_u32(node.wrapping_add(8), 0);
}
