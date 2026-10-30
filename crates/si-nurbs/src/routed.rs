// SPDX-License-Identifier: MIT
//! The calls between the NURBS routines that a port in this crate answers: [`Routed`] is an [`Outside`] that takes a
//! call by the original's address, hands it to the port and gives every other call to the world it wraps (a recorded
//! one in a check, a host's in the engine). The ports' own calls out come back here first, so a port that calls another
//! port reaches it.
//!
//! Nothing here reproduces a function of the original: it wires ported functions together (finding 0025 section 20,
//! the way finding 0035 "Routing5" does for si-dynamics). [`Routed::counts`] says how often a port answered each routine.
//! Switches: `NURBS_NO_ROUTING` (every call to the wrapped world), `NURBS_ROUTING_SKIP=0xADDR,..` (these stay there).

use crate::{blend2, bridge, curveinter, curveinter2, geom3, image, inter};
use si_mem::{Mem, Outside, Ret, Routine};
use std::collections::BTreeMap;

/// The routines [`Routed::route`] answers by a port, by address.
pub const ROUTED: &[u32] = &[
    0x084d_4130, // SI_nurbsCrvKnotInsert
    0x084d_96bc, // SI_nurbsSrfKnotInsert
    0x084d_c5f4, // SI_nurbsSrfSwapUV
    0x084e_6960, // si_nurbsClampFactor
    0x084d_caf8, // SI_nurbsSrfInverse
    0x084d_d29c, // SI_nurbsSrfInverseForConversion
    0x084d_ed40, // SI_knotsSrfReparametrization
    0x0850_2904, // nurbsFrenetCoord
    0x084e_ec00, // SI_nurbsCrvIndexHit
    0x084c_b168, // nurbsCrvExtendEndKnot
    0x084c_b334, // nurbsCrvExtendBeginKnot
    0x084c_cc98, // nurbsSrfExtendEndKnot
    0x084c_cfd0, // nurbsSrfExtendBeginKnot
    0x084c_ac38, // si_nurbsCrvExtendKnot
    0x084c_ae78, // si_nurbsSrfExtendKnot
    0x084c_b560, // SI_nurbsCrvMerge
    0x084c_b730, // SI_nurbsCrvBlend
    0x084c_bb0c, // SI_nurbsCrvMergeLinear
    0x084c_be80, // SI_nurbsCrvMergeJoin
    0x084c_c920, // SI_nurbsSrfMerge
    0x084c_d2a0, // SI_nurbsSrfBlend
    0x084c_d9b8, // SI_nurbsSrfMergeLinear
    0x084c_e028, // SI_nurbsSrfMergeJoin
    0x084d_bbf4, // SI_nurbsCrvCopy
    0x084d_c1a8, // SI_nurbsSrfCopy
    0x084d_ba5c, // SI_nurbsSrfDispose
    0x084d_b870, // SI_nurbsCrvDispose (curveinter2)
    0x084e_9644, // SI_nurbsSrfApplyMatrix
    0x084e_9750, // SI_nurbsCrvApplyMatrix
    0x084d_6824, // SI_nurbsSrfValueAndNormal
    0x084d_6728, // SI_nurbsSrfValue (trimtri1: the trimmed triangulation's evaluator)
    0x084d_5268, // SI_nurbsSrfExtractCurve (trimtri1)
    0x084d_bb14, // composite dispose
    0x084f_0134, // SI_nurbsSrfForceCapping
    0x084d_ca24, // SI_nurbsCrvInverse
    0x0850_ca74, // si_nurbsInterCreateP
    0x0850_cc48, // si_nurbsInterComputeRayT
    0x0850_dc64, // si_nurbsRayAddInter
    0x0850_db00, // si_nurbsRayDestroy
    0x084e_62f4, // SI_nurbsCrvFlatten (answered by inter::route, which this falls through to)
    0x0850_d448, // InterCopy
    0x0850_d4a4, // InterCompT
    0x0850_cacc, // InterDestroy
    0x0850_7778, // segment inter destroy (curveinter)
    0x0850_792c, // curveinter
    0x0850_758c, // CurveInterDestroy
    0x0850_8250, // si_nurbsCurveInterUpdateBbox
    0x0850_9a44, // si_nurbsCurveInterPointOnLine
    0x084e_e4a8, // nurbsCrvAddKnotNonUniform (geom8)
    0x084e_a5ec, // markDirtyCcrvs (geom5)
    0x0850_20a8, // SI_nurbsUpdateFrenet (geom11)
];

/// Whoever answers a call a port of this crate does not.
pub struct Routed<'a> {
    pub inner: &'a mut dyn Outside,
    /// Per routine, how many calls a port answered.
    pub counts: BTreeMap<&'static str, u32>,
    /// Switch: with `false` every call goes to the wrapped world.
    pub enabled: bool,
    /// Switch `NURBS_ROUTING_SKIP=0xADDR,0xADDR`: these addresses stay with the wrapped world.
    pub skip: Vec<u32>,
}

fn dbl(a: &[u32], k: usize) -> f64 {
    f64::from_bits(a.get(k).copied().unwrap_or(0) as u64 | (a.get(k + 1).copied().unwrap_or(0) as u64) << 32)
}

impl<'a> Routed<'a> {
    pub fn new(inner: &'a mut dyn Outside) -> Routed<'a> {
        let skip = std::env::var("NURBS_ROUTING_SKIP")
            .map(|v| v.split(',').filter_map(|t| u32::from_str_radix(t.trim().trim_start_matches("0x"), 16).ok()).collect())
            .unwrap_or_default();
        Routed { inner, counts: BTreeMap::new(), enabled: std::env::var_os("NURBS_NO_ROUTING").is_none(), skip }
    }

    /// The port for the routine at `addr` with the stack words `a`, or `None` where this crate has none.
    pub fn route(&mut self, m: &mut Mem, addr: u32, a: &[u32]) -> Option<Ret> {
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        if std::env::var_os("NURBS_ROUTED_TRACE").is_some() && ROUTED.contains(&addr) {
            eprintln!("nurbs-routed {addr:08x} {a:x?}");
        }
        let r = match addr {
            0x084d_4130 => Ret::eax(bridge::crv_knot_insert(m, w(0), dbl(a, 1), w(3) as i32, w(4) as i16) as u32),
            0x084d_96bc => Ret::eax(bridge::srf_knot_insert(m, self, w(0), dbl(a, 1), w(3) as i32, w(4) as i32) as u32),
            0x084d_c5f4 => Ret::eax(bridge::srf_swap_uv(m, w(0)) as u32),
            0x084e_6960 => {
                bridge::si_nurbs_clamp_factor(m, w(0), w(1));
                Ret::eax(0)
            }
            0x084d_caf8 => Ret::eax(geom3::si_nurbs_srf_inverse(m, self, w(0), w(1) as i16, w(2) as i16) as u32),
            0x084d_d29c => Ret::eax(geom3::si_nurbs_srf_inverse_for_conversion(m, self, w(0), w(1) as i16, w(2) as i16) as u32),
            0x084d_ed40 => {
                geom3::knots_srf_reparametrization(m, self, w(0));
                Ret::eax(0)
            }
            0x0850_2904 => {
                let inner = &mut *self.inner;
                // routing10: `SI_nurbsUpdateFrenet` is answered by its port (geom11); a refusal leaves the slot as it is
                geom3::nurbs_frenet_coord(m, w(0), dbl(a, 1), w(3), w(4), &mut |m, crv, slot| {
                    crate::geom11::nurbs_update_frenet(m, &mut *inner, crv, slot).ok();
                });
                Ret::eax(0)
            }
            0x0850_20a8 => {
                // routing10: [soft 0x085020a8 SI_nurbsUpdateFrenet]; a refusal goes to the inner world
                crate::geom11::nurbs_update_frenet(m, &mut *self.inner, w(0), w(1)).ok()?;
                Ret::eax(0)
            }
            0x084e_ec00 => Ret::eax(geom3::nurbs_crv_index_hit(m, w(0), w(1), w(2), w(3) as i16) as u32),
            0x084c_b168 => {
                blend2::nurbs_crv_extend_end_knot(m, w(0), w(1), w(2) as i32);
                Ret::eax(0)
            }
            0x084c_b334 => {
                blend2::nurbs_crv_extend_begin_knot(m, w(0), w(1), w(2) as i32);
                Ret::eax(0)
            }
            0x084c_cc98 => {
                blend2::nurbs_srf_extend_end_knot(m, w(0), w(1), w(2) as i32);
                Ret::eax(0)
            }
            0x084c_cfd0 => {
                blend2::nurbs_srf_extend_begin_knot(m, w(0), w(1), w(2) as i32);
                Ret::eax(0)
            }
            0x084c_ac38 => {
                blend2::si_nurbs_crv_extend_knot(m, w(0), w(1), w(2) as i32, dbl(a, 3), dbl(a, 5), dbl(a, 7));
                Ret::eax(0)
            }
            0x084c_ae78 => {
                blend2::si_nurbs_srf_extend_knot(m, w(0), w(1), w(2) as i32, dbl(a, 3), dbl(a, 5), dbl(a, 7));
                Ret::eax(0)
            }
            0x084c_b560 => Ret::eax(blend2::si_nurbs_crv_merge(m, w(0), w(1), dbl(a, 2), dbl(a, 4), dbl(a, 6))),
            0x084c_b730 => Ret::eax(blend2::si_nurbs_crv_blend(m, w(0), w(1), dbl(a, 2), dbl(a, 4), dbl(a, 6))),
            0x084c_bb0c => Ret::eax(blend2::si_nurbs_crv_merge_linear(m, w(0), w(1), dbl(a, 2))),
            0x084c_be80 => Ret::eax(blend2::si_nurbs_crv_merge_join(m, w(0), w(1))),
            0x084c_c920 => Ret::eax(blend2::si_nurbs_srf_merge(m, self, w(0), w(1), dbl(a, 2), dbl(a, 4), dbl(a, 6))),
            0x084c_d2a0 => Ret::eax(blend2::si_nurbs_srf_blend(m, self, w(0), w(1), dbl(a, 2), dbl(a, 4), dbl(a, 6))),
            0x084c_d9b8 => Ret::eax(blend2::si_nurbs_srf_merge_linear(m, self, w(0), w(1), dbl(a, 2))),
            0x084c_e028 => Ret::eax(blend2::si_nurbs_srf_merge_join(m, self, w(0), w(1))),
            0x084d_bbf4 => Ret::eax(image::crv_copy(m, w(0))),
            0x084d_c1a8 => Ret::eax(image::srf_copy(m, self, w(0))),
            0x084d_ba5c => {
                image::srf_dispose(m, self, w(0));
                Ret::eax(0)
            }
            0x084e_9644 => {
                image::srf_apply_matrix(m, self, w(0), w(1), w(2) as i32);
                Ret::eax(0)
            }
            0x084e_9750 => {
                image::crv_apply_matrix(m, w(0), w(1), w(2) as i32);
                Ret::eax(0)
            }
            0x084d_6824 => {
                // the caller's two curves (words 7 and 8) are used when given (trimtri1: EvaluateVertices passes its iso-curves)
                image::srf_value_and_normal_curves(m, w(0), w(1), dbl(a, 2), dbl(a, 4), w(6), w(7), w(8));
                Ret::eax(w(0))
            }
            0x084d_6728 => {
                // trimtri1: [soft 0x084d6728 SI_nurbsSrfValue] (geom8::srf_value_heap): the point goes to the caller's 24 bytes and the
                // address comes back; a refusal of the evaluator goes to the inner world
                let val = crate::geom8::srf_value_heap(m, w(1), dbl(a, 2), dbl(a, 4))?;
                for (k, c) in val.iter().enumerate() {
                    m.set_f64(w(0) + 8 * k as u32, si_x87::fmov64(*c));
                }
                Ret::eax(w(0))
            }
            0x084d_5268 => {
                // trimtri1: [soft 0x084d5268 SI_nurbsSrfExtractCurve] (image::srf_extract_curve); a refusal goes to the inner world
                Ret::eax(image::srf_extract_curve(m, w(0), dbl(a, 1), w(3) as i32, w(4) as i32).ok()?)
            }
            0x084d_bb14 => {
                image::comp_dispose(m, w(0));
                Ret::eax(0)
            }
            0x084f_0134 => {
                image::srf_force_capping(m, self, w(0), w(1) as i16, w(2) as i16);
                Ret::eax(0)
            }
            0x084d_ca24 => Ret::eax(image::crv_inverse(m, self, w(0)) as u32),
            0x0850_ca74 => {
                let p = |k: usize| [w(k), w(k + 1), w(k + 2), w(k + 3), w(k + 4), w(k + 5)];
                Ret::eax(inter::inter_create_p(m, dbl(a, 0), dbl(a, 2), p(4), p(10), dbl(a, 16), w(18)))
            }
            0x0850_cc48 => {
                let (mn, se) = inter::inter_compute_ray_t(m, w(0)).to_bits();
                let mut b = [0u8; 10];
                b[..8].copy_from_slice(&mn.to_le_bytes());
                b[8..].copy_from_slice(&se.to_le_bytes());
                Ret::st0(b)
            }
            0x0850_dc64 => Ret::eax(inter::ray_add_inter(m, self, w(0), w(1), w(2))),
            0x0850_db00 => {
                inter::ray_destroy(m, self, w(0));
                Ret::eax(0)
            }
            0x0850_8250 => {
                let bx: [u32; 12] = std::array::from_fn(|k| w(1 + k));
                Ret::eax(curveinter::curve_inter_update_bbox(m, w(0), bx, dbl(a, 13), dbl(a, 15)))
            }
            0x0850_9a44 => {
                let p = |k: usize| [dbl(a, k), dbl(a, k + 2), dbl(a, k + 4)];
                Ret::eax(curveinter::curve_inter_point_on_line(p(0), p(6), p(12)))
            }
            0x084e_a5ec => {
                // routing9: [soft 0x084ea5ec markDirtyCcrvs]; a refusal (division by zero) goes to the inner world
                crate::geom5::mark_dirty_ccrvs(m, w(0), w(1) as i32).ok()?;
                Ret::eax(0)
            }
            0x084e_e4a8 => {
                // routing9: [soft 0x084ee4a8 nurbsCrvAddKnotNonUniform]; a refusal (span search fails) goes to the inner world
                crate::geom8::crv_add_knot_non_uniform(m, w(0), w(1) as i32, dbl(a, 2)).ok()?;
                Ret::eax(0)
            }
            _ => {
                let r = Routine::new("routed", addr);
                return inter::route(m, r, a).or_else(|| curveinter::route(m, r, a)).or_else(|| curveinter2::route2(m, r, a));
            }
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

