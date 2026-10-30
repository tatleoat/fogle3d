// SPDX-License-Identifier: MIT
//! `SI_dynamicChainsUpdate (0x081cc1cc)`, which the scene loader calls after the elements are in (finding 0017,
//! section 15): `si-dynamics`' port on the image, with the routines it reaches routed to the ports that exist.
//! Nothing here reproduces a function of the original; a routine with no port is answered 0 and counted.

use super::*;
use si_mem::Ret as MRet;
use si_mem::Routine;

/// On (the default); `RENSETUP_NO_DYNCHAINS=1` leaves the loader's `SI_dynamicChainsUpdate` recorded only.
pub(super) fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_DYNCHAINS").is_none())
}

/// On (the default); `RENSETUP_NO_PTRMAT=1` leaves `SI_patchConvertToMesh` a hook in the load's dynamics update.
pub(super) fn ptrmat_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RENSETUP_NO_PTRMAT").is_none())
}

/// The outside world of `si_dynamics::dynadial::si_dynamic_chains_update`, by address.
pub(super) struct DynaSide<'a>(pub &'a RefCell<Calls>);
impl si_mem::Outside for DynaSide<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, a: &[u32]) -> MRet {
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let calls = self.0;
        let v: Option<u32> = match r.addr {
            // SI_edGetElemFromModel, SI_edFindFcv, SI_edFindNextFcv
            0x080f_cf20 => Some(si_scene::edtools::ed_get_elem_from_model(&mut Lookups(calls), m, Ptr32::new(w(0)), Ptr32::new(w(1))).addr()),
            0x080e_db20 => Some(si_motion::ed::find_fcv(m, w(0), w(1) as i32, w(2) as i32)),
            0x080e_db74 => Some(si_motion::ed::find_next_fcv(m, w(0), w(1) as i32, w(2) as i32)),
            // SI_fcurveCheckForRawKeys (0x08202e54): si-fcurve's port on the curve the image holds. A null curve
            // is a fault of the original (it reads the key count at +0x5c of address 0); here no raw keys.
            0x0820_2e54 => Some(if w(0) == 0 { 0 } else { si_motion::fcv::load(m, w(0)).check_for_raw_keys() as u32 }),
            // SI_modelGlobalState (0x084637dc)
            0x0846_37dc => {
                si_model::model_image::setters::global_state(m, Ptr32::new(w(0)), w(1), w(2), w(3));
                Some(0)
            }
            // SI_patchConvertToMesh (0x0847b69c): si-model's port, which reads the default material through SI_ptrmat
            // (set by the load world, `RENSETUP_NO_PTRMAT=1` leaves it a hook as before); SI_nurbsConvertToMesh
            // (0x084df320) stays a hook.
            0x0847_b69c if ptrmat_on() => Some(si_model::mesh_image::patch::patch_convert_to_mesh(m, &mut ModelSide(calls), w(0), w(1))),
            _ => None,
        };
        let mut t = calls.borrow_mut();
        let e = t.entry(if r.name.is_empty() { "(dynamic chains, by address)" } else { r.name }).or_default();
        match v {
            Some(v) => {
                e.0 += 1;
                MRet::eax(v)
            }
            None => {
                e.1 += 1;
                MRet::eax(0)
            }
        }
    }
}

/// `SI_dynamicChainsUpdate (0x081cc1cc)` on the image: the loader's call, for a host that runs the load's calls
/// (`motion-evalcheck`, a render command's world); `calls` takes the tally.
pub fn dynamic_chains_update(m: &mut Mem, calls: &mut Calls) {
    let cell = RefCell::new(std::mem::take(calls));
    {
        let mut base = DynaSide(&cell);
        let mut routed = si_dynamics::routed::Routed::new(&mut base);
        si_dynamics::dynadial::si_dynamic_chains_update(m, &mut routed);
    }
    *calls = cell.into_inner();
}

/// What the loader's model reader asks by address that `si-scene` cannot reach: `SI_dynachnvarAllocate (0x0849fad8)` and
/// `SI_dynajntvarAllocate (0x0849fa88)` (`si-dynamics`' ports: the chain record a model with dynamics gets at +0x304, token 0x12 of its file; the argument is
/// the 16-bit word of its defaults). `None` for anything else, and with the switch off.
pub fn loader_answer(m: &mut Mem, addr: u32, args: &[u32]) -> Option<u32> {
    if !on() {
        return None;
    }
    match addr {
        0x0849_fad8 => Some(si_dynamics::settings_dyn::si_dynachnvar_allocate(m, args.first().copied().unwrap_or(0) as i16)),
        0x0849_fa88 => Some(si_dynamics::settings_dyn::si_dynajntvar_allocate(m)),
        _ => None,
    }
}
