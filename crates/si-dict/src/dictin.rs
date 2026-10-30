// SPDX-License-Identifier: MIT
//! `dictin.c`: reading the dictionary in (the part that works on the dictionary alone).

use crate::globals::SI_CID;
use crate::outside::{si_actions_add_master_keys_to_ha, si_actions_init_path_anim, si_actions_tmp_activate_all, Outside};
use si_mem::{Mem, Ptr32};
use si_types::dict::{Elem, ElemDict};

/// [soft 0x08129244 SI_elemdictRestoreActions]: after a load, for every element of the
/// ANIMATION chapter that is new (`new_flag` == 1), is a root of its hierarchy
/// (`hier_level` == 0) and has an animation record (`data`): its actions (the record's
/// list at +0x0c) are all turned on (`SI_actionsTmpActivateAll(ed, actions, 1)`), the
/// masters' keys are added to the hierarchy animation (`SI_actionsAddMasterKeysToHA(ed,
/// element, 0)`) and the path animation of the actions is initialised
/// (`SI_actionsInitPathAnim(ed, actions)`). The count of elements is read again after
/// every element, as the original does.
pub fn restore_actions(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>) {
    let list = m[ed].lists.add(m[SI_CID].ani);
    let mut k = 0;
    let mut at = 0u32;
    if 0 < m[list].nb_elems {
        loop {
            let e: Ptr32<Elem> = Ptr32::new(m[list].elems.addr().wrapping_add(at));
            if m[e].new_flag == 1 && m[e].hier_level == 0 && !m[e].data.is_null() {
                let actions = m[e].data.addr().wrapping_add(0xc);
                si_actions_tmp_activate_all(o, m, ed.addr(), actions, 1);
                si_actions_add_master_keys_to_ha(o, m, ed.addr(), e.addr(), 0);
                si_actions_init_path_anim(o, m, ed.addr(), actions);
            }
            at = at.wrapping_add(0x84);
            k += 1;
            if k >= m[list].nb_elems {
                break;
            }
        }
    }
}
