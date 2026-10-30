// SPDX-License-Identifier: MIT
//! The ports of unit last2 (finding 0041, 2026-10-29): SI_cusReadingAnimModel, SI_cusDeleteIcon and
//! SI_cusElemCallbackUpdateEffectsForDeletion (cases: re/tools/last2/gen_cus.py). Chained from `cus1::with`.

use si_checkkit::Port;
use si_custom::custom_del::*;

/// `base` followed by this unit's ports.
pub fn with(base: &[(&'static str, Port)]) -> Vec<(&'static str, Port)> {
    let mut v: Vec<(&'static str, Port)> = base.to_vec();
    v.extend_from_slice(&[
        ("SI_cusReadingAnimModel", |m, _o, _a| (si_cus_reading_anim_model(m) as u32).into()),
        ("SI_cusDeleteIcon", |m, o, a| (si_cus_delete_icon(m, o, a.v("icon")) as u32).into()),
        ("SI_cusElemCallbackUpdateEffectsForDeletion", |m, _o, a| { si_cus_elem_callback_update_effects_for_deletion(m, a.v("kind"), a.v("elem")); ().into() }),
    ]);
    v
}
