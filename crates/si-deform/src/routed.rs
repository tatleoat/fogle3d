// SPDX-License-Identifier: MIT
//! The calls out of this crate answered by the ports other crates have on the image:
//! [`Routed`] is an [`Outside`] that takes a call by the original's address, hands it to
//! the port (`si-dict`, `si-model`, `si-spline`, `si-nurbs`, `si-hrc`, `si-kine`,
//! `si-view`, `si-anim`, `si-motion`) in the port's own convention, and gives every other
//! call to the outside world it wraps (a recorded one in a check, a host's in the engine).
//!
//! Nothing here reproduces a function of the original: it wires ported functions together
//! (finding 0029 section 20). The ports' own calls out come back here first (the adapters
//! at the foot), so a port that calls another port reaches it, and only a call no port
//! answers goes to the wrapped world. [`Routed::counts`] says how often each routine was
//! answered by a port.

use crate::outside::{Outside, Routine};
use si_mem::{Mem, Ptr32};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// A [`Routine`] for a name and address a port handed over: one leaked record per address.
fn routine_of(name: &'static str, addr: u32) -> &'static Routine {
    static CACHE: Mutex<Vec<&'static Routine>> = Mutex::new(Vec::new());
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = c.iter().find(|r| r.addr == addr) {
        return r;
    }
    let r: &'static Routine = Box::leak(Box::new(Routine { name, addr }));
    c.push(r);
    r
}

/// The routines [`Routed::route`] answers by a port, by address.
pub const ROUTED: &[u32] = &[
    0x0813_4e9c, 0x0813_2b50, 0x0813_4364, 0x0813_508c, 0x0813_5554, 0x0813_4db0, 0x0813_3a6c, 0x0813_5770, 0x0813_2f78, 0x0813_96d8,
    0x0813_3998, 0x0811_ff18,
    0x0847_a2b8, 0x0844_04e0, 0x0848_aa74, 0x084d_bbf4, 0x084d_c1a8, 0x0847_a530, 0x0844_04a0, 0x0848_ab48, 0x084d_b870, 0x084d_ba5c,
    0x0846_62f8, 0x084a_d480, 0x0846_d530, 0x081d_20c0, 0x0844_67b8, 0x0832_ee68, 0x0846_37dc, 0x0846_3f54, 0x0845_a7c8, 0x0847_402c,
    0x0847_69b4, 0x0847_6b2c, 0x0847_2a0c, 0x0847_2b44, 0x0847_2c3c, 0x0846_f200, 0x0815_3140, 0x0845_99f4, 0x0845_9e8c, 0x0847_c1d0,
    0x084e_9eec, 0x084e_9f2c, 0x0848_d298, 0x0846_d70c, 0x0848_f6f8, 0x080e_e2f0, 0x080f_cf20,
    0x0822_bd8c, 0x0822_0f08, 0x0822_1224,
    0x084b_c4b8,
    0x0850_3068,
    0x0850_20a8,
];

/// Whoever answers a call a port does not.
pub struct Routed<'a> {
    pub inner: &'a mut dyn Outside,
    /// Per routine, how many calls a port answered.
    pub counts: BTreeMap<&'static str, u32>,
    /// Switch: with `false` every call goes to the wrapped world, as before the routing.
    pub enabled: bool,
    /// Switch `DEFORM_ROUTING_SKIP=0xADDR,0xADDR`: these addresses stay with the wrapped world
    /// (for bisecting a routed address that makes a picture worse).
    pub skip: Vec<u32>,
}

impl<'a> Routed<'a> {
    pub fn new(inner: &'a mut dyn Outside) -> Routed<'a> {
        let skip = std::env::var("DEFORM_ROUTING_SKIP")
            .map(|v| v.split(',').filter_map(|t| u32::from_str_radix(t.trim().trim_start_matches("0x"), 16).ok()).collect())
            .unwrap_or_default();
        Routed { inner, counts: BTreeMap::new(), enabled: std::env::var_os("DEFORM_NO_ROUTING").is_none(), skip }
    }

    /// `name` at `addr` with the stack words `a`: the port's answer, else the wrapped world's.
    pub fn dispatch(&mut self, m: &mut Mem, name: &'static str, addr: u32, a: &[u32]) -> u32 {
        if self.enabled && !self.skip.contains(&addr) {
            if let Some(v) = self.route(m, addr, a) {
                *self.counts.entry(name).or_default() += 1;
                return v;
            }
        }
        self.inner.call(m, routine_of(name, addr), a)
    }

    /// The port for the routine at `addr`, or `None` where there is none (or it cannot be had
    /// without a cycle of crates or a record the image does not hold: finding 0029 section 20).
    pub fn route(&mut self, m: &mut Mem, addr: u32, a: &[u32]) -> Option<u32> {
        use si_model::mesh_image::{obj, rebuild};
        use si_model::model_image::{defs, points, readjust, setters, state, touch, tree};
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        macro_rules! p {
            ($k:expr) => {
                Ptr32::new(w($k))
            };
        }
        Some(match addr {
            // ---- si-dict
            0x0813_4e9c => si_dict::elemrel::get_relation_from_ids(m, p!(0), p!(1), w(2) as i32, w(3) as i32, w(4) as i32, w(5) as i32).addr(),
            0x0813_2b50 => si_dict::elemdict::get_elem(m, p!(0), w(1) as i32, w(2) as i32).addr(),
            0x0813_4364 => si_dict::elemdict::find_elem_by_data(m, &mut DictSide(self), p!(0), w(1) as i32, p!(2)) as u32,
            0x0813_508c => si_dict::elemrel::find_related_elem(m, p!(0), p!(1), w(2) as i16, w(3) as i32, w(4) as i32, w(5) as i32, p!(6), p!(7)).addr(),
            0x0813_5554 => si_dict::elemrel::find_related_elem_serial(m, p!(0), p!(1), w(2) as i16, w(3) as i32, w(4) as i32, w(5) as i32, p!(6), p!(7), w(8) as i32).addr(),
            0x0813_4db0 => si_dict::elemrel::get_cur_relation(m, p!(0), p!(1), w(2) as i16, w(3) as i32).addr(),
            0x0813_3a6c => si_dict::elemrel::remove_serial_rel_id(m, &mut DictSide(self), p!(0), p!(1), w(2) as i32, w(3) as i32, w(4) as i32, w(5) as i32) as u32,
            0x0813_5770 => si_dict::elemrel::range_relations_count(m, p!(0), p!(1), w(2) as i16, w(3) as i32, w(4) as i32, w(5) as i32, w(6) as i32) as u32,
            0x0813_2f78 => si_dict::elemrel::add_relation(m, p!(0), p!(1), w(2) as i32, w(3) as i32, w(4) as i32, w(5) as i32, w(6) as i32) as u32,
            0x0813_96d8 => si_dict::relations::rel_remove_relations(m, &mut DictSide(self), p!(0), p!(1), w(2) as i32, w(3) as i32, w(4) as i16) as u32,
            0x0813_3998 => si_dict::elemrel::remove_relation_id(m, &mut DictSide(self), p!(0), p!(1), w(2) as i32, w(3) as i32) as u32,
            0x0811_ff18 => si_dict::attdict::set_fcurve_flag(m, p!(0), w(1) as i32, w(2) as i32) as u32,
            // ---- copies and disposals of the geometry
            0x0847_a2b8 => si_spline::patch_image::patch_copy(m, p!(0)).addr(),
            0x0844_04e0 => obj::mesh_copy(m, &mut ModelSide(self), w(0)),
            0x0848_aa74 => si_hrc::image::spline::spline_copy(m, w(0)).addr(),
            0x084d_bbf4 => si_nurbs::image::crv_copy(m, w(0)),
            0x084d_c1a8 => si_nurbs::image::srf_copy(m, &mut NurbsSide(self), w(0)),
            0x0847_a530 => {
                si_spline::patch_image::patch_dispose(m, w(0));
                0
            }
            0x0844_04a0 => {
                obj::mesh_dispose(m, w(0));
                0
            }
            0x0848_ab48 => {
                si_hrc::image::spline::spline_dispose(m, w(0));
                0
            }
            0x084d_b870 => {
                si_hrc::image::nurbs::nurbs_crv_dispose(m, w(0));
                0
            }
            0x084d_ba5c => {
                si_nurbs::image::srf_dispose(m, &mut NurbsSide(self), w(0));
                0
            }
            // ---- the model tree
            0x0846_62f8 => {
                state::update_son_state(m, &mut ModelSide(self), p!(0), p!(1), w(2) as i16);
                0
            }
            0x084a_d480 => {
                si_kine::image::tools::model_special_update_son_state(m, p!(0), p!(1));
                0
            }
            0x0846_d530 => {
                touch::touch_definition(m, &mut ModelSide(self), p!(0));
                0
            }
            0x081d_20c0 => si_kine::onimage::ske_chain_root(m, p!(0)).addr(),
            0x0844_67b8 => {
                // the value form of the mesh answers: the point stored through the third argument
                let mesh = si_model::mesh_image::to_value(m, w(0));
                let (c, _) = si_model::obj_global::mesh_polygon_centroid(&mesh, w(1) as i32);
                crate::wr3(m, w(2), c);
                0
            }
            0x0832_ee68 => si_view::graflist::find_gl_data(m, p!(0), p!(1), w(2) as i32).0,
            0x0846_37dc => {
                setters::global_state(m, p!(0), w(1), w(2), w(3));
                0
            }
            0x0846_3f54 => {
                setters::set_rotation(m, &mut ModelSide(self), p!(0), w(1), w(2) as i32, w(3) as i16);
                0
            }
            0x0845_a7c8 => {
                rebuild::optimize_display(m, &mut ModelSide(self), w(0));
                0
            }
            0x0847_402c => points::get_vertex_ptr(m, w(0), w(1) as i32, w(2) as i32, w(3)),
            0x0847_69b4 => points::elem_get_nb_vertices(m, &mut ModelSide(self), w(0), w(1), w(2) as i32),
            0x0847_6b2c => points::elem_get_vertex(m, &mut ModelSide(self), w(0), w(1), w(2), w(3) as i32, w(4) as i32, w(5) as i32, w(6), w(7)),
            0x0847_2a0c => {
                defs::add_ilattice(m, p!(0), w(1) as i16, w(2) as i16, w(3));
                0
            }
            0x0847_2b44 => defs::spline_fitting(m, p!(0), w(1)),
            0x0847_2c3c => defs::patch_fitting(m, p!(0), w(1)),
            0x0846_f200 => {
                readjust::update_dependency(m, &mut ModelSide(self), p!(0), w(1));
                0
            }
            0x0815_3140 => {
                // SI_latticeTouch, SI_splineFitTouch and SI_postAnimTouch are one body (deformation.c)
                crate::deformation::post_anim_touch(m, self, p!(0), w(1) as i16);
                0
            }
            0x0845_99f4 => {
                rebuild::computes_polygon_normal(m, w(0), w(1) as i32);
                0
            }
            0x0845_9e8c => {
                rebuild::computes_vertex_normal(m, w(0), w(1) as i32);
                0
            }
            // SI_modelReadjustDefinition (0x0846e340) stays a hook (finding 0029 section 21): routed
            // through `ModelSide` it loses the callees the hosts answer by their ports (wave
            // transform, spline and patch fit, NURBS apply-matrix): 13 first frames got worse.
            0x0846_d70c => tree::next_son(m, p!(0), p!(1)).addr(),
            // ---- the curves and surfaces
            0x0847_c1d0 => {
                si_spline::patch_image::patch_compute_surface(m, p!(0), w(1) as i16);
                0
            }
            0x084e_9eec => si_nurbs::image::crv_move_point(m, w(0), w(1) as i32, dbl(a, 2), dbl(a, 4), dbl(a, 6)) as u32,
            0x084e_9f2c => si_nurbs::image::srf_move_point(m, &mut NurbsSide(self), w(0), w(1) as i32, dbl(a, 2), dbl(a, 4), dbl(a, 6), dbl(a, 8)) as u32,
            0x0848_d298 => si_spline::frenet_image::spline_update_frenet(m, &mut SplineSide(self), p!(0), w(1)) as u32,
            0x0848_f6f8 => {
                si_spline::frenet_image::spline_distance_to_value_frenet(m, &mut SplineSide(self), p!(0), f32::from_bits(w(1)), w(2), w(3), w(4));
                0
            }
            // routing9: [soft 0x08503068 SI_nurbsDistanceToValueFrenet] (si-nurbs geom7); `SI_nurbsUpdateFrenet (0x085020a8)` is answered
            // by its port (geom11). A refusal (`Halt`) answers nothing, as the original stops.
            0x0850_3068 => {
                let cell = std::cell::RefCell::new(&mut *self);
                let mut side = CellSide(&cell);
                let mut update = |m: &mut Mem, crv: u32, slot: u32| {
                    si_nurbs::geom11::nurbs_update_frenet(m, &mut CellSide(&cell), crv, slot).ok();
                };
                si_nurbs::geom7::nurbs_distance_to_value_frenet(m, &mut side, w(0), f32::from_bits(w(1)), w(2), w(3), w(4), &mut update).ok();
                0
            }
            0x0850_20a8 => {
                si_nurbs::geom11::nurbs_update_frenet(m, &mut NurbsSide(self), w(0), w(1)).ok();
                0
            }
            // ---- the editing layer
            0x080e_e2f0 => {
                si_anim::edanim::ed_fcv_dispose(m, &mut AnimSide(self), w(0), w(1), w(2), w(3), w(4));
                0
            }
            0x080f_cf20 => {
                let (e, id) = si_motion::ed::get_elem_from_model(m, &mut EdSide(self), w(0));
                if w(1) != 0 {
                    m.set_i32(w(1), id);
                }
                e
            }
            // ---- animation (si-motion's hosts: MotionSide)
            0x0822_bd8c => si_motion::animcopy::elemdict_copy_elem_anim(m, &mut MotionSide(self), w(0), w(1), w(2), w(3) as i32, w(4) as i32, w(5) as i32) as u32,
            0x0822_0f08 => si_motion::reset::elemdict_reset_elem_anim(m, &mut MotionSide(self), w(0), w(1), w(2) as i32, w(3) as i32, w(4) as i16) as u32,
            0x0822_1224 => {
                si_motion::reset::model_reset_fcv(m, &mut MotionSide(self), w(0), w(1) as i32, w(2) as i32, w(3) as i32, w(4) as i16, w(5) as i32, w(6) as i32, w(7));
                0
            }
            // ---- the other way: a call of the model tree's into this crate (SI_modelCutFather, 0x0846d790)
            0x084b_c4b8 => { crate::envtools::envelop_cut(m, self, p!(0)); 0 }
            _ => return None,
        })
    }
}

/// A double argument: words `k` and `k + 1`, low word first.
fn dbl(a: &[u32], k: usize) -> f64 {
    let w = |k: usize| a.get(k).copied().unwrap_or(0) as u64;
    f64::from_bits(w(k) | w(k + 1) << 32)
}

impl Outside for Routed<'_> {
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        self.dispatch(m, routine.name, routine.addr, args)
    }
    fn drand48(&mut self) -> f64 {
        self.inner.drand48()
    }
}

// ---- the ports' own outside worlds: back to `Routed::dispatch`

struct DictSide<'r, 'a>(&'r mut Routed<'a>);
impl si_dict::Outside for DictSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, r: &'static si_dict::Routine, a: &[u32]) -> u32 {
        self.0.dispatch(m, r.name, r.addr, a)
    }
}

struct ModelSide<'r, 'a>(&'r mut Routed<'a>);
impl si_model::model_image::Outside for ModelSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, c: &si_model::model_image::Callee, a: &[u32]) -> u32 {
        self.0.dispatch(m, c.name, c.addr, a)
    }
}

struct SplineSide<'r, 'a>(&'r mut Routed<'a>);
impl si_spline::spline_image::Outside for SplineSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, c: &si_spline::spline_image::Callee, a: &[u32]) -> u32 {
        self.0.dispatch(m, c.name, c.addr, a)
    }
}

struct AnimSide<'r, 'a>(&'r mut Routed<'a>);
impl si_anim::outside::Outside for AnimSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, r: &'static si_anim::outside::Routine, a: &[u32]) -> u32 {
        self.0.dispatch(m, r.name, r.addr, a)
    }
    fn call_f(&mut self, _m: &mut Mem, _r: &'static si_anim::outside::Routine, _a: &[u32]) -> si_x87::F80 {
        si_x87::F80::ZERO
    }
}

struct NurbsSide<'r, 'a>(&'r mut Routed<'a>);
impl si_mem::Outside for NurbsSide<'_, '_> {
    fn call(&mut self, m: &mut Mem, r: si_mem::Routine, a: &[u32]) -> si_mem::Ret {
        si_mem::Ret::eax(self.0.dispatch(m, r.name, r.addr, a))
    }
}

/// [`NurbsSide`] for a `Routed` that is also lent to a closure: each call borrows the cell on its own.
struct CellSide<'c, 'r, 'a>(&'c std::cell::RefCell<&'r mut Routed<'a>>);
impl si_mem::Outside for CellSide<'_, '_, '_> {
    fn call(&mut self, m: &mut Mem, r: si_mem::Routine, a: &[u32]) -> si_mem::Ret {
        si_mem::Ret::eax(self.0.borrow_mut().dispatch(m, r.name, r.addr, a))
    }
}

struct EdSide<'r, 'a>(&'r mut Routed<'a>);
impl si_motion::ed::EdHost for EdSide<'_, '_> {
    fn elemdict_find_elem_by_data(&mut self, m: &mut Mem, dict: si_mem::Addr, chap: i32, data: si_mem::Addr) -> i32 {
        si_dict::elemdict::find_elem_by_data(m, &mut DictSide(self.0), Ptr32::new(dict), chap, Ptr32::new(data))
    }
    fn elemdict_find_son(&mut self, m: &mut Mem, dict: si_mem::Addr, e: si_mem::Addr) -> si_mem::Addr {
        si_dict::elemmisc::find_son(m, Ptr32::new(dict), Ptr32::new(e)).addr()
    }
    fn elemdict_find_next_son(&mut self, m: &mut Mem, dict: si_mem::Addr, e: si_mem::Addr) -> si_mem::Addr {
        si_dict::elemmisc::find_next_son(m, Ptr32::new(dict), Ptr32::new(e)).addr()
    }
}

/// `si-motion`'s hosts (`ResetHost`, `KeyHost`, `UtilsHost`, `CopyHost`, `PathHost` and what they
/// stand on): the methods that have a port answer by it, the routines of `deformation`'s own
/// outside table go to the wrapped world by their address, the rest (dialogs, the curve
/// editor's list, drawing) do nothing, as the hooks they replace did.
struct MotionSide<'r, 'a>(&'r mut Routed<'a>);

impl MotionSide<'_, '_> {
    fn out(&mut self, m: &mut Mem, r: &crate::outside::Routine, a: &[u32]) -> u32 {
        self.0.dispatch(m, r.name, r.addr, a)
    }
}

impl si_motion::ed::EdHost for MotionSide<'_, '_> {
    fn elemdict_find_elem_by_data(&mut self, m: &mut Mem, dict: si_mem::Addr, chap: i32, data: si_mem::Addr) -> i32 {
        EdSide(self.0).elemdict_find_elem_by_data(m, dict, chap, data)
    }
    fn elemdict_find_son(&mut self, m: &mut Mem, dict: si_mem::Addr, e: si_mem::Addr) -> si_mem::Addr {
        EdSide(self.0).elemdict_find_son(m, dict, e)
    }
    fn elemdict_find_next_son(&mut self, m: &mut Mem, dict: si_mem::Addr, e: si_mem::Addr) -> si_mem::Addr {
        EdSide(self.0).elemdict_find_next_son(m, dict, e)
    }
}
impl si_motion::update::UpdateHost for MotionSide<'_, '_> {}
impl si_motion::update::transfo::TransfoHost for MotionSide<'_, '_> {}
impl si_motion::key::KeyHost for MotionSide<'_, '_> {
    fn reldict_get_rel_chap_grp(&mut self, m: &mut Mem, reldict: si_mem::Addr, relid: i32, group: si_mem::Addr) -> i32 {
        si_dict::reldict::get_rel_chap_grp(m, Ptr32::new(reldict), relid, Ptr32::new(group))
    }
}
impl si_motion::reset::ResetHost for MotionSide<'_, '_> {
    fn attdict_get_fcurve_flag(&mut self, m: &mut Mem, atd: si_mem::Addr, kind: i32) -> i16 {
        si_dict::attdict::get_fcurve_flag(m, Ptr32::new(atd), kind) as i16
    }
    fn elemdict_remove_backward_rel_id(&mut self, m: &mut Mem, dict: si_mem::Addr, chap: i32, index: i32, relid: i32, a: i32) {
        si_dict::elemrel::remove_backward_rel_id(m, &mut DictSide(self.0), Ptr32::new(dict), chap, index, relid, a);
    }
    fn elemdict_remove_relation_id(&mut self, m: &mut Mem, dict: si_mem::Addr, reldict: si_mem::Addr, index: i32, relid: i32) {
        si_dict::elemrel::remove_relation_id(m, &mut DictSide(self.0), Ptr32::new(dict), Ptr32::new(reldict), index, relid);
    }
    fn ed_fcv_dispose(&mut self, m: &mut Mem, obj: si_mem::Addr, chap: i32, group: i32, lo: i32, hi: i32) {
        si_anim::edanim::ed_fcv_dispose(m, &mut AnimSide(self.0), obj, chap as u32, group as u32, lo as u32, hi as u32);
    }
    fn set_fcv_table_entries_from_chap(&mut self, m: &mut Mem, chap: i32) {
        self.out(m, &crate::outside::SI_SET_FCV_TABLE_ENTRIES_FROM_CHAP, &[chap as u32]);
    }
    fn model_delete_cluster_anim(&mut self, m: &mut Mem, mdl: si_mem::Addr, kluster: si_mem::Addr) {
        self.out(m, &crate::outside::SI_MODEL_DELETE_CLUSTER_ANIM, &[mdl, kluster]);
    }
    fn ed_dispose_cluster(&mut self, m: &mut Mem, mdl: si_mem::Addr, kluster: si_mem::Addr) {
        self.out(m, &crate::outside::SI_ED_DISPOSE_CLUSTER, &[mdl, kluster]);
    }
}
impl si_motion::utils::UtilsHost for MotionSide<'_, '_> {}
impl si_motion::animcopy::CopyHost for MotionSide<'_, '_> {
    fn reldict_get_rel_name(&mut self, m: &mut Mem, reldict: si_mem::Addr, relid: i32) -> si_mem::Addr {
        si_dict::reldict::get_rel_name(m, Ptr32::new(reldict), relid).addr()
    }
}
impl si_motion::path::PathHost for MotionSide<'_, '_> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outside::NoOutside;

    /// Every address in [`ROUTED`] has an arm in [`Routed::route`] (a port may fault on a null
    /// argument; it must not be missing), and a routine without a port goes to the wrapped world.
    #[test]
    fn every_listed_routine_has_a_port_and_the_rest_goes_on() {
        std::panic::set_hook(Box::new(|_| {}));
        for &addr in ROUTED {
            let mut m = Mem::new();
            let mut o = NoOutside::default();
            let mut r = Routed::new(&mut o);
            let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.route(&mut m, addr, &[0; 12]).is_some()));
            assert!(ran.unwrap_or(true), "no arm for {addr:#010x}");
        }
        let mut m = Mem::new();
        let mut o = NoOutside::default();
        {
            let mut r = Routed::new(&mut o);
            r.dispatch(&mut m, "SI_dialogRequest2", 0x0814_1d5c, &[1, 2]);
            assert!(r.counts.is_empty());
        }
        assert_eq!(o.calls.len(), 1);
        assert_eq!(o.calls[0].0.addr, 0x0814_1d5c);
        assert_eq!(o.calls[0].1, vec![1, 2]);
    }

    /// `SI_modelSplineFitting` by its port: the fitting of set 1 when its first 16 bits are not
    /// zero, with the set's number stored as 16 bits.
    #[test]
    fn spline_fitting_goes_to_the_port() {
        let mut m = Mem::new();
        let node = m.si_calloc(1, 0x430);
        let fit = m.si_calloc(1, 0x10);
        m.set_u16(fit, 1);
        let at = Ptr32::<si_types::model::Model>::new(node);
        let f1 = si_mem::field!(at, si_types::model::Model, spline_fit).addr() + 4;
        m.set_u32(f1, fit);
        let set = m.local(4);
        let mut o = NoOutside::default();
        let mut r = Routed::new(&mut o);
        let got = r.dispatch(&mut m, "SI_modelSplineFitting", 0x0847_2b44, &[node, set]);
        assert_eq!(got, fit);
        assert_eq!(m.i16(set), 1);
        assert_eq!(r.counts["SI_modelSplineFitting"], 1);
        r.enabled = false;
        assert_eq!(r.dispatch(&mut m, "SI_modelSplineFitting", 0x0847_2b44, &[node, set]), 0);
    }
}
