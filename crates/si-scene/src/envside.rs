// SPDX-License-Identifier: MIT
//! The routines `si-deform`'s envelope (`envelop.c`, `envtools.c`; finding 0029) calls out of
//! its package, answered by the port another crate has on the image: what the load's
//! `SI_envelopRebuild` and the update's `SI_envelopCompUpdate` need, for every host that runs
//! them (the loader's [`crate::image::Prog`], `si-rensetup`'s update, the evaluation check).
//!
//! Nothing here reproduces a function of the original: it wires ported functions together.
//! A routine without a port on the image is not answered here ([`envelope_call`] answers
//! `None`, and the host answers it as it answers any routine it lacks). The NURBS curves'
//! and surfaces' copies, disposals, point moves and transfers are `si-nurbs`' image forms;
//! what those call outside `nrb_b.c` and `nrb_misc.c` (`SI_genTreeDestroy`,
//! `markDirtyCcrvs`, `SI_nurbsSrfForceCapping`) goes back to the host as a call of
//! `si-model`'s outside world ([`NurbsSide`]), which answers it as it answers any routine it
//! lacks. So every host runs the load's `SI_envelopRebuild` on every hierarchy, NURBS skins
//! included; nothing is refused for its kind. What is not on the image is answered at the
//! call and counted: a capped surface's `SI_nurbsSrfForceCapping` (Actor-Walk's skin; its
//! 50 frames are identical without it).

use crate::edtools::{self, ByAddr};
use si_mem::{Mem, Ptr32};

/// `routine(a…)` for the routines of the envelope's outside world that have a port on the
/// image, by address; `None` for the others. `h` is the host: the dictionary look-ups of
/// `SI_edGetElemFromModel` and the calls out of `si-model`'s routines go back to it.
pub fn envelope_call<H: ByAddr + si_model::model_image::Outside>(h: &mut H, m: &mut Mem, addr: u32, a: &[u32]) -> Option<u32> {
    use si_model::mesh_image::{obj, rebuild};
    use si_model::model_image::{points, setters, state, touch};
    let w = |k: usize| a.get(k).copied().unwrap_or(0);
    Some(match addr {
        // si-model: SI_modelUpdateSonState (0x084662f8), SI_modelTouchDefinition (0x0846d530),
        // SI_modelGlobalState (0x084637dc), SI_modelSetRotation (0x08463f54),
        // SI_modelGetVertexPtr (0x0847402c), SI_elemGetNbVertices (0x084769b4)
        0x0846_62f8 => {
            state::update_son_state(m, h, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i16);
            0
        }
        0x0846_d530 => {
            touch::touch_definition(m, h, Ptr32::new(w(0)));
            0
        }
        0x0846_37dc => {
            setters::global_state(m, Ptr32::new(w(0)), w(1), w(2), w(3));
            0
        }
        0x0846_3f54 => {
            setters::set_rotation(m, h, Ptr32::new(w(0)), w(1), w(2) as i32, w(3) as i16);
            0
        }
        0x0847_402c => points::get_vertex_ptr(m, w(0), w(1) as i32, w(2) as i32, w(3)),
        0x0847_69b4 => points::elem_get_nb_vertices(m, h, w(0), w(1), w(2) as i32),
        // si-model, meshes on the image: SI_meshCopy (0x084404e0), SI_meshDispose
        // (0x084404a0), SI_meshComputesPolygonNormal (0x084599f4),
        // SI_meshComputesVertexNormal (0x08459e8c)
        0x0844_04e0 => obj::mesh_copy(m, h, w(0)),
        0x0844_04a0 => {
            obj::mesh_dispose(m, w(0));
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
        // si-hrc: SI_splineCopy (0x0848aa74), SI_splineDispose (0x0848ab48)
        0x0848_aa74 => si_hrc::image::spline::spline_copy(m, w(0)).addr(),
        0x0848_ab48 => {
            si_hrc::image::spline::spline_dispose(m, w(0));
            0
        }
        // si-spline: SI_patchCopy (0x0847a2b8), SI_patchDispose (0x0847a530),
        // SI_patchComputeSurface (0x0847c1d0, an adapter there)
        0x0847_a2b8 => si_spline::patch_image::patch_copy(m, Ptr32::new(w(0))).addr(),
        0x0847_a530 => {
            si_spline::patch_image::patch_dispose(m, w(0));
            0
        }
        0x0847_c1d0 => {
            si_spline::patch_image::patch_compute_surface(m, Ptr32::new(w(0)), w(1) as i16);
            0
        }
        // si-kine: SI_modelSpecialUpdateSonState (0x084ad480), SI_skeChainRoot (0x081d20c0)
        0x084a_d480 => {
            si_kine::image::tools::model_special_update_son_state(m, Ptr32::new(w(0)), Ptr32::new(w(1)));
            0
        }
        0x081d_20c0 => si_kine::onimage::ske_chain_root(m, Ptr32::new(w(0))).addr(),
        // si-view: SI_findGlData (0x0832ee68)
        0x0832_ee68 => si_view::graflist::find_gl_data(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32).0,
        // si-nurbs on the image: SI_nurbsCrvCopy (0x084dbbf4), SI_nurbsSrfCopy (0x084dc1a8),
        // SI_nurbsSrfDispose (0x084dba5c), SI_nurbsSrfMovePoint (0x084e9f2c),
        // SI_nurbsSrfTransferPointsKnots (0x084ea07c), SI_nurbsCrvMovePoint (0x084e9eec);
        // si-hrc: SI_nurbsCrvDispose (0x084db870)
        0x084d_bbf4 => si_nurbs::image::crv_copy(m, w(0)),
        0x084d_c1a8 => si_nurbs::image::srf_copy(m, &mut NurbsSide(h), w(0)),
        0x084d_ba5c => {
            si_nurbs::image::srf_dispose(m, &mut NurbsSide(h), w(0));
            0
        }
        0x084d_b870 => {
            si_hrc::image::nurbs::nurbs_crv_dispose(m, w(0));
            0
        }
        // x, y, z (and w): doubles, two words each (low word first)
        0x084e_9f2c => si_nurbs::image::srf_move_point(m, &mut NurbsSide(h), w(0), w(1) as i32, dbl(a, 2), dbl(a, 4), dbl(a, 6), dbl(a, 8)) as u32,
        0x084e_9eec => si_nurbs::image::crv_move_point(m, w(0), w(1) as i32, dbl(a, 2), dbl(a, 4), dbl(a, 6)) as u32,
        0x084e_a07c => {
            si_nurbs::image::srf_transfer_points_knots(m, &mut NurbsSide(h), w(0), w(1));
            0
        }
        // si-dict: SI_elemdictGetRelationFromIds (0x08134e9c)
        0x0813_4e9c => si_dict::elemrel::get_relation_from_ids(m, Ptr32::new(w(0)), Ptr32::new(w(1)), w(2) as i32, w(3) as i32, w(4) as i32, w(5) as i32).addr(),
        // this crate: SI_edGetElemFromModel (0x080fcf20)
        0x080f_cf20 => edtools::ed_get_elem_from_model(h, m, Ptr32::new(w(0)), Ptr32::new(w(1))).0,
        // this crate: edtools.c, first half ([`crate::edfirst`], checked on the vectors of
        // re/tools/edtools1/emu_gen_edfirst.py)
        0x080f_caf0 => crate::edfirst::db_graf_type2chapter(m, w(0) as i32) as u32,
        0x080f_cba0 => crate::edfirst::rel_get_cons_rel_from_graf_type(w(0) as i32) as u32,
        0x0810_03e8 => crate::edfirst::rel_get_fitting_rel_id(w(0) as i16, w(1) as i16) as u32,
        0x080f_cbec => crate::edfirst::elemdict_find_elem_from_graf_node(h, m, w(0), w(1), w(2)),
        0x080f_cde0 => crate::edfirst::elemdict_find_elem_from_del_graf_node(h, m, w(0), w(1), w(2)),
        0x080f_ce80 => crate::edfirst::ed_find_parent(h, m, w(0)),
        0x080f_d544 => crate::edfirst::ed_get_cluster_from_model(h, m, w(0), w(1) as i32),
        0x080f_d5b8 => crate::edfirst::ed_get_elem_from_nselection(h, m, w(0), Ptr32::new(w(1))).0,
        0x080f_d5ec => crate::edfirst::ed_get_elem_from_anim_file(h, m, w(0), Ptr32::new(w(1))).0,
        0x080f_e894 => crate::edfirst::ed_model_get_new_name(h, m, w(0)),
        // SI_edElementChangeName (0x080febf4): `(element, chapter, prefix, &name)`; the dialogs' rename
        0x080f_ebf4 => crate::edname::ed_element_change_name(h, m, w(0), w(1) as i32, w(2), w(3)),
        0x080f_d6c8 => {
            crate::edfirst::elemdict_element_set_delete_flag(h, m, w(0), w(1), w(2) as i32, w(3) as i32, w(4) as i16, w(5), w(6), w(7) as i16);
            0
        }
        0x080f_d910 => {
            crate::edfirst::ed_element_set_delete_flag(h, m, w(0), w(1) as i32, w(2) as i32, w(3) as i16, w(4), w(5), w(6) as i16);
            0
        }
        0x080f_dc18 => {
            crate::edfirst::ed_model_delete_flag(h, m, w(0), w(1) as i16, w(2) as i16, w(3) as i16);
            0
        }
        0x080f_dcf4 => {
            crate::edfirst::ed_graf_node_set_delete_flag(h, m, w(0), w(1) as i16, w(2), w(3) as i16);
            0
        }
        0x080f_e330 => {
            crate::edfirst::transfer_constraint_to_root(h, m, w(0), w(1) as i32, w(2) as i32, w(3), w(4) as i32);
            0
        }
        0x080f_e41c => {
            crate::edfirst::ed_link_model(h, m, w(0), w(1), w(2) as i16);
            0
        }
        0x080f_e5ec => {
            crate::edfirst::ed_cut_model(h, m, w(0), w(1), w(2) as i16);
            0
        }
        0x080f_e8c8 => {
            crate::edfirst::ed_model_change_name(h, m, w(0), w(1));
            0
        }
        0x080f_f5f0 => {
            crate::edfirst::ed_relax_rel_from_del_model(h, m, w(0), w(1) as i32);
            0
        }
        0x080f_ffac => crate::edfirst::ed_element_set_constraint(h, m, w(0), w(1) as i32, w(2), w(3) as i32, w(4) as i16, w(5)),
        0x0810_0238 => crate::edfirst::ed_element_relax_constraint(h, m, w(0), w(1) as i32, w(2) as i16),
        0x0810_041c => {
            crate::edfirst::ed_model_set_fitting(h, m, w(0), w(1), w(2) as i16, w(3) as i16);
            0
        }
        0x0810_0500 => {
            crate::edfirst::ed_model_relax_fitting(h, m, w(0), w(1) as i16, w(2) as i16);
            0
        }
        0x0810_0d70 => {
            crate::edfirst::ed_model_set_lig_selection(h, m, w(0), Ptr32::new(w(1)));
            0
        }
        0x0810_0de4 => {
            crate::edfirst::ed_model_remove_lig_selection(h, m, w(0), Ptr32::new(w(1)));
            0
        }
        0x0810_1098 => {
            crate::edfirst::ed_model_relax_l_texture3d(h, m, w(0), w(1));
            0
        }
        0x0810_137c => {
            crate::edfirst::ed_model_relax_l_texture2d(h, m, w(0), w(1));
            0
        }
        0x0810_1420 => {
            crate::edfirst::ed_model_link_wave(h, m, Ptr32::new(w(0)), w(1));
            0
        }
        // what si_waveGeomUpdate (crate::wave_geom) calls besides: SI_elemGetVertex
        // (0x08476b2c), SI_elemSetVertex (0x08476d70) and SI_computeMeshVertexNormal
        // (0x08458960, its three floats through the hidden first argument), si-model's;
        // SI_patchValueAndNormal (0x0847fe84), si-spline's; SI_nurbsSrfValueAndNormal
        // (0x084d6824, curves null), si-nurbs'. The point routines' calls out (a NURBS
        // point's move) come back here first ([`Via`]).
        0x0847_6b2c => points::elem_get_vertex(m, &mut Via(h), w(0), w(1), w(2), w(3) as i32, w(4) as i32, w(5) as i32, w(6), w(7)),
        0x0847_6d70 => points::elem_set_vertex(m, &mut Via(h), w(0), w(1), w(2), w(3) as i32, w(4) as i32, w(5) as i32, w(6)),
        0x0845_8960 => {
            let n = si_model::mesh_image::cleanup::compute_mesh_vertex_normal(m, w(1), w(2) as i32);
            for (k, v) in n.into_iter().enumerate() {
                m.set_f32(w(0).wrapping_add(4 * k as u32), v);
            }
            w(0)
        }
        0x0847_fe84 => si_spline::patch_eval_image::patch_value_and_normal(m, Ptr32::new(w(0)), f32::from_bits(w(1)), f32::from_bits(w(2)), w(3), w(4)) as u32,
        0x084d_6824 => {
            si_nurbs::image::srf_value_and_normal(m, w(0), w(1), dbl(a, 2), dbl(a, 4), w(6));
            w(0)
        }
        // the other routines of si-nurbs that call each other (`si_nurbs::routed::ROUTED`: knot insertions, SwapUV,
        // SI_nurbsSrfInverse, the blend/merge/extend group, intersections, ...), but those answering in st0; what
        // they call that no port answers goes back to the host through `NurbsSide`
        0x0850_cc48 => return None,
        addr if si_nurbs::routed::ROUTED.contains(&addr) => {
            let mut side = NurbsSide(h);
            return si_nurbs::routed::Routed::new(&mut side).route(m, addr, a).map(|r| r.eax);
        }
        _ => return None,
    })
}

/// `si-model`'s outside world that tries [`envelope_call`] first and hands the rest to the
/// host: for the point routines, whose NURBS point moves are answered there.
pub struct Via<'a, H>(pub &'a mut H);
impl<H: ByAddr + si_model::model_image::Outside> si_model::model_image::Outside for Via<'_, H> {
    fn call(&mut self, m: &mut Mem, callee: &si_model::model_image::Callee, a: &[u32]) -> u32 {
        match envelope_call(self.0, m, callee.addr, a) {
            Some(v) => v,
            None => self.0.call(m, callee, a),
        }
    }
}

/// A double argument: words `k` and `k + 1`, low word first.
fn dbl(a: &[u32], k: usize) -> f64 {
    let w = |k: usize| a.get(k).copied().unwrap_or(0) as u64;
    f64::from_bits(w(k) | w(k + 1) << 32)
}

/// The calls out of `si-nurbs`' image forms, handed to the host as calls of `si-model`'s
/// outside world (by the routine's name and address), so that it answers and counts them as
/// it does the model code's.
pub struct NurbsSide<'a, H>(pub &'a mut H);
impl<H: si_model::model_image::Outside> si_mem::Outside for NurbsSide<'_, H> {
    fn call(&mut self, m: &mut Mem, r: si_mem::Routine, a: &[u32]) -> si_mem::Ret {
        let callee = si_model::model_image::Callee { name: r.name, addr: r.addr };
        si_mem::Ret::eax(self.0.call(m, &callee, a))
    }
}
