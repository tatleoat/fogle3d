// SPDX-License-Identifier: MIT
//! The calls out of this crate that its own ports answer: [`Routed`] is an [`Outside`] that takes a call by the
//! original's address, hands it to the port in this crate (`constr.rs`, `playback.rs`, `settings.rs`, `dynamic.rs`)
//! and gives every other call to the outside world it wraps (a recorded one in a check, a host's in the engine).
//!
//! Nothing here reproduces a function of the original: it wires ported functions together (finding 0029
//! section 20, finding 0035 "Routing5"). The ports' own calls out come back here first, so a port that calls
//! another port reaches it. [`Routed::counts`] says how often each routine was answered by a port.

use crate::{constr, dynamic, playback, settings};
use si_mem::{Mem, Outside, Ret, Routine};
use std::collections::BTreeMap;

/// The routines [`Routed::route`] answers by a port of this crate, by address.
pub const ROUTED: &[u32] = &[
    0x0849_5544, // si_constraintPositionGet
    0x0849_5624, // si_constraintOrientationGet
    0x0849_5e5c, // SI_dynaConstraintGet
    0x0849_58d0, // SI_conslistConstrAllocate
    0x0849_5ad8, // SI_conslistAddConstraint
    0x0849_5d90, // SI_conslistDispose
    0x0849_d760, // SI_dynaSimulationDispose
    0x0849_d8f8, // SI_dynaTransitionCrvDispose
    0x0849_fe94, // si_modelDynaRel2AbsMatrixUpdate
    0x0849_ff5c, // si_modelDynaAbs2RelMatrixUpdate
    0x084a_0080, // si_modelDynaJointsUpdate
    0x0849_739c, // dynaSlowbandInbound
    0x0849_81f8, // dynaFastbandInbound
    0x0849_88f4, // dynaFastbandOutbound
    0x0849_e030, // SI_simulationRawFcrvSaveKeys
    // si-model's ports on the image (routeD, count1 "Routing debt"); `Routed` has `si-model` as a dependency
    0x0844_490c, // SI_meshCentroid
    0x0844_5054, // SI_meshComputesVolumeSurface
    0x0844_4df8, // SI_meshPolygonVolumeSurface
    0x0844_04a0, // SI_meshDispose
    0x0846_7e70, // SI_modelComputeBounding
];

/// Whoever answers a call a port of this crate does not.
pub struct Routed<'a> {
    pub inner: &'a mut dyn Outside,
    /// Per routine, how many calls a port answered.
    pub counts: BTreeMap<&'static str, u32>,
    /// Switch: with `false` every call goes to the wrapped world, as before the routing.
    pub enabled: bool,
    /// Switch `DYNAMICS_ROUTING_SKIP=0xADDR,0xADDR`: these addresses stay with the wrapped world.
    pub skip: Vec<u32>,
}

impl<'a> Routed<'a> {
    pub fn new(inner: &'a mut dyn Outside) -> Routed<'a> {
        let skip = std::env::var("DYNAMICS_ROUTING_SKIP")
            .map(|v| v.split(',').filter_map(|t| u32::from_str_radix(t.trim().trim_start_matches("0x"), 16).ok()).collect())
            .unwrap_or_default();
        Routed { inner, counts: BTreeMap::new(), enabled: std::env::var_os("DYNAMICS_NO_ROUTING").is_none(), skip }
    }

    /// The port for the routine at `addr` with the stack words `a`, or `None` where this crate has none.
    pub fn route(&mut self, m: &mut Mem, addr: u32, a: &[u32]) -> Option<Ret> {
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        let r = match addr {
            0x0849_5544 => {
                constr::si_constraint_position_get(m, w(0), w(1), w(2) as i16, w(3));
                Ret::eax(0)
            }
            0x0849_5624 => {
                constr::si_constraint_orientation_get(m, w(0), w(1), w(2) as i16, w(3) as i16, w(4) as u8, w(5));
                Ret::eax(0)
            }
            0x0849_5e5c => Ret::eax(constr::si_dyna_constraint_get(m, self, w(0) as i16, w(1) as i16)),
            0x0849_58d0 => Ret::eax(constr::si_conslist_constr_allocate(m, w(0) as i16, w(1) as i16, w(2))),
            0x0849_5ad8 => Ret::eax(constr::si_conslist_add_constraint(m, self, w(0), w(1), w(2) as i16) as u32),
            0x0849_5d90 => {
                constr::si_conslist_dispose(m, self, w(0), w(1) as i16, w(2) as i16);
                Ret::eax(0)
            }
            0x0849_d760 => {
                playback::si_dyna_simulation_dispose(m, self, w(0), w(1) as i16);
                Ret::eax(0)
            }
            0x0849_d8f8 => {
                playback::si_dyna_transition_crv_dispose(m, self, w(0));
                Ret::eax(0)
            }
            0x0849_fe94 => {
                settings::si_model_dyna_rel2abs_matrix_update(m, w(0));
                Ret::eax(0)
            }
            0x0849_ff5c => {
                settings::si_model_dyna_abs2rel_matrix_update(m, w(0));
                Ret::eax(0)
            }
            0x084a_0080 => {
                settings::si_model_dyna_joints_update(m, w(0));
                Ret::eax(0)
            }
            0x0849_739c => {
                dynamic::dyna_slowband_inbound(m, self, w(0), f32::from_bits(w(1)), w(2) as u16);
                Ret::eax(0)
            }
            0x0849_81f8 => {
                dynamic::dyna_fastband_inbound(m, self, w(0));
                Ret::eax(0)
            }
            0x0849_88f4 => {
                dynamic::dyna_fastband_outbound(m, w(0));
                Ret::eax(0)
            }
            0x0849_e030 => {
                playback::si_simulation_raw_fcrv_save_keys(m, self, w(0), f32::from_bits(w(1)));
                Ret::eax(0)
            }
            // [soft 0x0844490c SI_meshCentroid] (mesh, out vector)
            0x0844_490c => {
                si_model::mesh_image::measure::mesh_centroid(m, w(0), w(1));
                Ret::eax(0)
            }
            // [soft 0x08445054 SI_meshComputesVolumeSurface] (mesh, out volume, out surface)
            0x0844_5054 => {
                si_model::mesh_image::measure::mesh_computes_volume_surface(m, w(0), w(1), w(2));
                Ret::eax(0)
            }
            // [soft 0x08444df8 SI_meshPolygonVolumeSurface] (mesh, polygon, scale flag, out volume, out surface)
            0x0844_4df8 => {
                si_model::mesh_image::measure::mesh_polygon_volume_surface(m, w(0), w(1) as i32, w(2) as i16, w(3), w(4));
                Ret::eax(0)
            }
            // [soft 0x084404a0 SI_meshDispose] (address of the mesh pointer)
            0x0844_04a0 => {
                si_model::mesh_image::obj::mesh_dispose(m, w(0));
                Ret::eax(0)
            }
            // [soft 0x08467e70 SI_modelComputeBounding] (model)
            0x0846_7e70 => {
                si_model::model_image::bounds::compute_bounding(m, si_mem::Ptr32::new(w(0)));
                Ret::eax(0)
            }
            _ => return None,
        };
        Some(r)
    }
}

impl Outside for Routed<'_> {
    fn call(&mut self, m: &mut Mem, routine: Routine, args: &[u32]) -> Ret {
        if self.enabled && !self.skip.contains(&routine.addr) {
            if let Some(r) = self.route(m, routine.addr, args) {
                *self.counts.entry(routine.name).or_default() += 1;
                return r;
            }
        }
        self.inner.call(m, routine, args)
    }
    fn print(&mut self, stream: u32, text: &[u8]) {
        self.inner.print(stream, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_mem::Trace;

    /// Every address of [`ROUTED`] reaches a port of this crate (the wrapped world is never asked), on a made-up zeroed
    /// model and constraint; the counts say each was answered once.
    #[test]
    fn every_routed_address_reaches_its_port() {
        let mut m = Mem::new();
        let model = m.si_malloc(0x400);
        let body = m.si_malloc(0x400);
        let out = m.si_malloc(0x40);
        let mut inner = Trace::with(|_m, r, _a| panic!("{} ({:08x}) went to the wrapped world", r.name, r.addr));
        let mut routed = Routed::new(&mut inner);
        routed.enabled = true;
        for (name, addr, args) in [
            ("si_constraintPositionGet", 0x0849_5544u32, vec![model, body, 1, out]),
            ("si_constraintOrientationGet", 0x0849_5624, vec![model, body, 1, 0, 0x78, out]),
            ("SI_conslistConstrAllocate", 0x0849_58d0, vec![3, 0xffff_ffff, 0]),
            ("SI_conslistAddConstraint", 0x0849_5ad8, vec![model, model, 0]),
            ("si_modelDynaRel2AbsMatrixUpdate", 0x0849_fe94, vec![model]),
            ("si_modelDynaAbs2RelMatrixUpdate", 0x0849_ff5c, vec![model]),
            ("dynaFastbandOutbound", 0x0849_88f4, vec![model]),
        ] {
            let r = routed.call(&mut m, Routine::new(name, addr), &args);
            let _ = r;
            assert_eq!(routed.counts.get(name), Some(&1), "{name}");
        }
        assert!(routed.route(&mut m, 0x0123_4567, &[]).is_none());
        // the others are in the list too
        for a in [0x0844_490c, 0x0844_5054, 0x0844_4df8, 0x0844_04a0, 0x0846_7e70] {
            assert!(ROUTED.contains(&a));
        }
        for a in [0x0849_5e5c, 0x0849_5d90, 0x0849_d760, 0x0849_d8f8, 0x084a_0080, 0x0849_739c, 0x0849_81f8] {
            assert!(ROUTED.contains(&a));
        }
    }

    /// With the switch off every call goes to the wrapped world.
    #[test]
    fn switch_off_leaves_calls_to_the_wrapped_world() {
        let mut m = Mem::new();
        let mut inner = Trace::with(|_m, _r, _a| Ret::eax(7));
        let mut routed = Routed::new(&mut inner);
        routed.enabled = false;
        let r = routed.call(&mut m, Routine::new("si_constraintPositionGet", 0x0849_5544), &[1, 2, 3, 4]);
        assert_eq!(r.eax, 7);
        assert!(routed.counts.is_empty());
    }
}
